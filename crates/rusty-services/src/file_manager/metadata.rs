//! Metadata inspection, Get Info statistics, `TextPreview`, Go to Folder and formatting.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::listing::build_file_entry;
use super::types::{FileEntry, UserFileLocations};

/// Detailed metadata displayed in Finder-style `Get Info` and `Gallery -> Information`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDetailedInfo {
    /// Display basename.
    pub name: String,
    /// Full entry path.
    pub path: PathBuf,
    /// Human-readable kind string (`Folder`, `Image`, `Source Code`, `Alias`, ...).
    pub kind_label: String,
    /// File length or bounded recursive directory byte total.
    pub size_bytes: u64,
    /// For directories: immediate direct child count.
    pub item_count: Option<usize>,
    /// True if folder stats hit the traversal safety cap.
    pub size_truncated: bool,
    /// Parent path formatted with `~` when under the user's home directory.
    pub where_parent: String,
    /// Creation timestamp (Unix seconds), when supported by filesystem.
    pub created_unix_secs: Option<i64>,
    /// Modification timestamp (Unix seconds).
    pub modified_unix_secs: Option<i64>,
    /// Image pixel dimensions `(width, height)` for valid PNG/PPM files.
    pub dimensions: Option<(u32, u32)>,
    /// Symbolic Unix permission string (`rwxr-xr-x`).
    pub permissions: String,
    /// Symlink target if the entry itself is a symlink.
    pub symlink_target: Option<PathBuf>,
    /// True when write bit / readonly flag denies mutation.
    pub is_readonly: bool,
}

/// Inspects metadata for `Get Info` and the `Gallery` right-hand inspector pane.
///
/// Directory byte/item calculation is bounded to avoid blocking on massive trees.
///
/// # Errors
/// Returns an `io::Error` if `path` cannot be `lstat`ed.
pub fn inspect_entry_info(
    path: &Path,
    locations: &UserFileLocations,
) -> io::Result<FileDetailedInfo> {
    let sym_meta = fs::symlink_metadata(path)?;
    let entry_stub = build_file_entry(path, None);
    let name = entry_stub.as_ref().map_or_else(
        || {
            path.file_name()
                .map_or_else(|| String::from("/"), |n| n.to_string_lossy().into_owned())
        },
        |e| e.name.clone(),
    );
    let kind_label = entry_stub
        .as_ref()
        .map_or("Item", FileEntry::kind_label)
        .to_owned();

    let is_symlink = sym_meta.file_type().is_symlink();
    let symlink_target = if is_symlink {
        fs::read_link(path).ok()
    } else {
        None
    };
    let resolved_meta = if is_symlink {
        fs::metadata(path).ok()
    } else {
        Some(sym_meta.clone())
    };
    let effective_meta = resolved_meta.as_ref().unwrap_or(&sym_meta);

    let (size_bytes, item_count, size_truncated) = if sym_meta.file_type().is_dir() {
        let (total_bytes, direct_children, truncated) = collect_dir_stats_bounded(path, 5_000);
        (total_bytes, Some(direct_children), truncated)
    } else {
        (effective_meta.len(), None, false)
    };

    let created_unix_secs = effective_meta
        .created()
        .ok()
        .and_then(system_time_to_unix_i64);
    let modified_unix_secs = effective_meta
        .modified()
        .ok()
        .and_then(system_time_to_unix_i64);
    let dimensions = if effective_meta.is_file() {
        crate::image_decode::read_image_dimensions(path)
    } else {
        None
    };

    let where_parent = format_where_parent(path, &locations.home);
    let permissions = format_unix_permissions(sym_meta.permissions().mode());
    let is_readonly = effective_meta.permissions().readonly();

    Ok(FileDetailedInfo {
        name,
        path: path.to_path_buf(),
        kind_label,
        size_bytes,
        item_count,
        size_truncated,
        where_parent,
        created_unix_secs,
        modified_unix_secs,
        dimensions,
        permissions,
        symlink_target,
        is_readonly,
    })
}

fn system_time_to_unix_i64(ts: SystemTime) -> Option<i64> {
    ts.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn format_where_parent(path: &Path, home: &Path) -> String {
    let Some(parent) = path.parent() else {
        return String::from("/");
    };
    if parent.as_os_str().is_empty() {
        return String::from("/");
    }
    if let Ok(rel) = parent.strip_prefix(home) {
        if rel.as_os_str().is_empty() {
            return String::from("~");
        }
        return format!("~/{}", rel.display());
    }
    parent.display().to_string()
}

fn collect_dir_stats_bounded(root: &Path, max_entries: usize) -> (u64, usize, bool) {
    let mut total_bytes = 0_u64;
    let mut direct_children = 0_usize;
    let mut visited = 0_usize;
    let mut truncated = false;
    let mut stack = vec![(root.to_path_buf(), true)];

    while let Some((dir, is_root)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for item in rd.flatten() {
            if is_root {
                direct_children += 1;
            }
            visited += 1;
            if visited > max_entries {
                truncated = true;
                return (total_bytes, direct_children, truncated);
            }
            let item_path = item.path();
            let Ok(meta) = fs::symlink_metadata(&item_path) else {
                continue;
            };
            let ft = meta.file_type();
            if ft.is_file() {
                total_bytes = total_bytes.saturating_add(meta.len());
            } else if ft.is_dir() {
                stack.push((item_path, false));
            }
        }
    }
    (total_bytes, direct_children, truncated)
}

fn format_unix_permissions(mode: u32) -> String {
    let mut out = String::with_capacity(9);
    let triples = [
        (0o400, 0o200, 0o100),
        (0o040, 0o020, 0o010),
        (0o004, 0o002, 0o001),
    ];
    for (read_bit, write_bit, exec_bit) in triples {
        out.push(if mode & read_bit != 0 { 'r' } else { '-' });
        out.push(if mode & write_bit != 0 { 'w' } else { '-' });
        out.push(if mode & exec_bit != 0 { 'x' } else { '-' });
    }
    out
}

/// UTF-8 plain-text snippet for `Quick Look` and `Gallery` previews.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPreview {
    /// Normalized display lines (`\r` stripped).
    pub lines: Vec<String>,
    /// Total file size in bytes on disk.
    pub total_bytes: u64,
    /// True if capped by `max_bytes` or `max_lines`.
    pub truncated: bool,
}

/// Reads a bounded UTF-8 text preview from `path`, returning `None` for missing or binary files.
#[must_use]
pub fn read_text_preview(path: &Path, max_bytes: usize, max_lines: usize) -> Option<TextPreview> {
    let mut file = File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let cap = max_bytes.clamp(1, 256 * 1024);
    let line_cap = max_lines.max(1);
    let mut buf = vec![0_u8; cap + 1];
    let read_len = file.read(&mut buf).ok()?;
    buf.truncate(read_len);

    let bytes_truncated = buf.len() > cap;
    if bytes_truncated {
        buf.truncate(cap);
    }
    if buf.contains(&0) {
        return None;
    }
    let text = match std::str::from_utf8(&buf) {
        Ok(valid) => valid,
        Err(err) => {
            if err.error_len().is_none() && err.valid_up_to() > 0 {
                std::str::from_utf8(&buf[..err.valid_up_to()]).ok()?
            } else {
                return None;
            }
        }
    };

    let ctrl_count = text
        .bytes()
        .filter(|b| *b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t'))
        .count();
    if !text.is_empty() && ctrl_count * 20 > text.len() {
        return None;
    }

    let mut lines = Vec::new();
    let mut lines_truncated = false;
    for raw_line in text.lines() {
        if lines.len() >= line_cap {
            lines_truncated = true;
            break;
        }
        lines.push(raw_line.trim_end_matches('\r').to_owned());
    }

    Some(TextPreview {
        lines,
        total_bytes: meta.len(),
        truncated: bytes_truncated || lines_truncated,
    })
}

/// Expands `~`, `~/...`, or relative inputs against `current_dir` and `locations.home`.
#[must_use]
pub fn expand_go_to_input(
    raw_input: &str,
    current_dir: Option<&Path>,
    locations: &UserFileLocations,
) -> Option<PathBuf> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let joined = if trimmed == "~" {
        locations.home.clone()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        locations.home.join(rest)
    } else {
        let candidate = Path::new(trimmed);
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            let base = current_dir.unwrap_or(locations.home.as_path());
            base.join(candidate)
        }
    };
    Some(normalize_lexical_path(&joined))
}

/// Resolves a user-entered `Go to Folder` path when it exists on disk.
#[must_use]
pub fn resolve_go_to_path(
    raw_input: &str,
    current_dir: Option<&Path>,
    locations: &UserFileLocations,
) -> Option<PathBuf> {
    let expanded = expand_go_to_input(raw_input, current_dir, locations)?;
    expanded.exists().then_some(expanded)
}

/// Returns up to `limit` completion suggestions for the `Go to Folder` input sheet.
#[must_use]
pub fn suggest_go_to_paths(
    raw_input: &str,
    current_dir: Option<&Path>,
    locations: &UserFileLocations,
    limit: usize,
) -> Vec<PathBuf> {
    if limit == 0 {
        return Vec::new();
    }
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return default_go_to_suggestions(current_dir, locations, limit);
    }

    let (search_dir, prefix) = if trimmed == "~" {
        let mut out = vec![locations.home.clone()];
        if let Ok(rd) = fs::read_dir(&locations.home) {
            append_sorted_matching_entries(rd, "", false, &mut out);
        }
        out.truncate(limit);
        return out;
    } else if trimmed.ends_with('/') {
        let Some(dir) = expand_go_to_input(trimmed, current_dir, locations) else {
            return Vec::new();
        };
        (dir, "")
    } else if let Some((parent_raw, name_prefix)) = trimmed.rsplit_once('/') {
        let parent_spec = if parent_raw.is_empty() {
            "/"
        } else {
            parent_raw
        };
        let Some(dir) = expand_go_to_input(parent_spec, current_dir, locations) else {
            return Vec::new();
        };
        (dir, name_prefix)
    } else {
        let base = current_dir
            .unwrap_or(locations.home.as_path())
            .to_path_buf();
        (base, trimmed)
    };

    let Ok(rd) = fs::read_dir(&search_dir) else {
        return Vec::new();
    };
    let mut matches = Vec::new();
    let include_hidden = prefix.starts_with('.');
    append_sorted_matching_entries(rd, prefix, include_hidden, &mut matches);
    matches.truncate(limit);
    matches
}

fn default_go_to_suggestions(
    current_dir: Option<&Path>,
    locations: &UserFileLocations,
    limit: usize,
) -> Vec<PathBuf> {
    let candidates = [
        current_dir,
        Some(locations.home.as_path()),
        Some(locations.desktop.as_path()),
        Some(locations.documents.as_path()),
        Some(locations.downloads.as_path()),
        Some(Path::new("/")),
    ];
    let mut out = Vec::new();
    for dir in candidates.into_iter().flatten() {
        let clean = normalize_lexical_path(dir);
        if clean.is_dir() && !out.contains(&clean) {
            out.push(clean);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

fn append_sorted_matching_entries(
    rd: fs::ReadDir,
    prefix: &str,
    include_hidden: bool,
    out: &mut Vec<PathBuf>,
) {
    let prefix_lower = prefix.to_ascii_lowercase();
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in rd.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if !include_hidden && name_str.starts_with('.') {
            continue;
        }
        if !prefix_lower.is_empty() && !name_str.to_ascii_lowercase().starts_with(&prefix_lower) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else {
            files.push(path);
        }
    }

    let cmp_name = |a: &PathBuf, b: &PathBuf| {
        let an = a
            .file_name()
            .map_or_else(String::new, |s| s.to_string_lossy().to_ascii_lowercase());
        let bn = b
            .file_name()
            .map_or_else(String::new, |s| s.to_string_lossy().to_ascii_lowercase());
        an.cmp(&bn).then_with(|| a.cmp(b))
    };
    dirs.sort_by(cmp_name);
    files.sort_by(cmp_name);
    out.extend(dirs);
    out.extend(files);
}

fn normalize_lexical_path(raw: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for comp in raw.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    clean
}

pub(crate) fn absolutize_path(path: &Path) -> io::Result<PathBuf> {
    let raw = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    Ok(normalize_lexical_path(&raw))
}

pub(crate) fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        let safe = byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~');
        if safe {
            out.push(*byte as char);
        } else {
            out.push('%');
            out.push(hex_upper(byte >> 4));
            out.push(hex_upper(byte & 0x0F));
        }
    }
    out
}

const fn hex_upper(nibble: u8) -> char {
    match nibble & 0x0F {
        0..=9 => (b'0' + (nibble & 0x0F)) as char,
        10..=15 => (b'A' + ((nibble & 0x0F) - 10)) as char,
        _ => '0',
    }
}

pub(crate) fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) =
                (from_hex_digit(bytes[i + 1]), from_hex_digit(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const fn from_hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Formats byte sizes in macOS decimal Finder units (`1 KB = 1000 B`).
#[must_use]
pub fn format_byte_size(bytes: u64) -> String {
    const KB: u64 = 1_000;
    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000_000_000;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        let tenths = (bytes * 10) / KB;
        format!("{}.{} KB", tenths / 10, tenths % 10)
    } else if bytes < GB {
        let tenths = (bytes * 10) / MB;
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else {
        let tenths = (bytes * 10) / GB;
        format!("{}.{} GB", tenths / 10, tenths % 10)
    }
}

/// Formats Unix seconds into ISO-like UTC date-time (`YYYY-MM-DD HH:MM`) using pure arithmetic.
#[must_use]
pub fn format_unix_timestamp(unix_secs: i64) -> String {
    let (y, m, d, hh, mm, _ss) = decompose_unix_utc(unix_secs);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

pub(crate) fn format_iso8601_utc(unix_secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = decompose_unix_utc(unix_secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}")
}

fn decompose_unix_utc(unix_secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let clamped = unix_secs.max(0);
    let days = clamped.div_euclid(86_400);
    let secs_of_day = u32::try_from(clamped.rem_euclid(86_400)).unwrap_or(0);
    let hh = secs_of_day / 3_600;
    let mm = (secs_of_day % 3_600) / 60;
    let ss = secs_of_day % 60;

    // Howard Hinnant `civil_from_days` algorithm (pure safe integer math).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m_signed = mp + if mp < 10 { 3 } else { -9 };
    let y_adj = y + i64::from(m_signed <= 2);
    let m = u32::try_from(m_signed).unwrap_or(1);
    (y_adj, m, d, hh, mm, ss)
}
