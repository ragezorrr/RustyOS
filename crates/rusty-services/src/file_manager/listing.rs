//! Directory scanning, target listing, search, sorting and filtering.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::metadata::percent_decode;
use super::trash::list_trash_entries;
use super::types::{
    FileEntry, FileEntryKind, FileSortField, FileSortOrder, FileTagColor, FileTagsConfig,
    ListingTarget, UserFileLocations, discover_mounts,
};

/// Lists entries for any sidebar/navigation `ListingTarget`.
///
/// # Errors
/// Returns an `io::Error` if listing the target directory fails.
pub fn list_entries_for_target(
    target: &ListingTarget,
    locations: &UserFileLocations,
    tags: &FileTagsConfig,
) -> io::Result<Vec<FileEntry>> {
    match target {
        ListingTarget::Directory(path) => list_directory_at(path),
        ListingTarget::Applications => Ok(scan_applications_in(&locations.app_dirs)),
        ListingTarget::Recents => Ok(collect_recent_entries(locations)),
        ListingTarget::Shared => Ok(collect_shared_entries(locations)),
        ListingTarget::Network => Ok(collect_network_entries(locations)),
        ListingTarget::Trash => list_trash_entries(&locations.trash_dir),
        ListingTarget::Tag(color) => Ok(collect_tagged_entries(tags, *color)),
    }
}

/// Reads metadata for all entries immediately inside `dir`.
///
/// # Errors
/// Returns an `io::Error` if `dir` cannot be opened or read.
pub fn list_directory_at(dir: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry_res in fs::read_dir(dir)? {
        let Ok(entry) = entry_res else {
            continue;
        };
        let path = entry.path();
        if let Some(item) = build_file_entry(&path, None) {
            entries.push(item);
        }
    }
    Ok(entries)
}

/// Bounded iterative search under `root` for entries matching `query` (`RUSTYOS-071`).
#[must_use]
pub fn search_directory_recursive(
    root: &Path,
    query: &str,
    show_hidden: bool,
    max_results: usize,
) -> Vec<FileEntry> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() || max_results == 0 {
        return Vec::new();
    }
    let mut results = Vec::new();
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0_u8)]);
    let mut visited_dirs = 0_usize;
    while let Some((dir, depth)) = queue.pop_front() {
        if visited_dirs >= 1_500 || results.len() >= max_results {
            break;
        }
        visited_dirs += 1;
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry_res in read {
            let Ok(entry) = entry_res else {
                continue;
            };
            let path = entry.path();
            let Some(item) = build_file_entry(&path, None) else {
                continue;
            };
            if !show_hidden && item.hidden {
                continue;
            }
            if item.name.to_ascii_lowercase().contains(&needle) {
                results.push(item.clone());
                if results.len() >= max_results {
                    break;
                }
            }
            if depth < 4 && item.kind == FileEntryKind::Directory {
                queue.push_back((item.path, depth + 1));
            }
        }
    }
    results
}

pub(crate) fn build_file_entry(path: &Path, name_override: Option<String>) -> Option<FileEntry> {
    let sym_meta = fs::symlink_metadata(path).ok()?;
    let file_type = sym_meta.file_type();
    let name = name_override.unwrap_or_else(|| {
        path.file_name().map_or_else(
            || path.to_string_lossy().into_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    });
    let hidden = name.starts_with('.');
    let mut target_meta = sym_meta.clone();
    let symlink_target_is_dir = if file_type.is_symlink() {
        match fs::metadata(path) {
            Ok(resolved) => {
                let is_dir = resolved.is_dir();
                target_meta = resolved;
                is_dir
            }
            Err(_) => false,
        }
    } else {
        false
    };

    let kind = if file_type.is_dir() {
        FileEntryKind::Directory
    } else if file_type.is_symlink() {
        FileEntryKind::Symlink
    } else if file_type.is_file() {
        FileEntryKind::RegularFile
    } else {
        FileEntryKind::Other
    };
    let size_bytes = if target_meta.is_file() {
        target_meta.len()
    } else {
        0
    };
    let modified_unix_secs = target_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

    Some(FileEntry {
        name,
        path: path.to_path_buf(),
        kind,
        size_bytes,
        modified_unix_secs,
        hidden,
        symlink_target_is_dir,
        desktop_exec: None,
        trash_original_path: None,
        trash_deletion_date: None,
        subtitle: None,
    })
}

/// Discovers `.desktop` application entries across ordered search directories.
#[must_use]
pub fn scan_applications_in(dirs: &[PathBuf]) -> Vec<FileEntry> {
    let mut seen_ids = BTreeSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(read_dir) = fs::read_dir(dir) else {
            continue;
        };
        for item in read_dir.flatten() {
            let path = item.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !file_name.ends_with(".desktop") {
                continue;
            }
            if !seen_ids.insert(file_name.to_owned()) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            if let Some(entry) = parse_desktop_entry(&path, &content) {
                apps.push(entry);
            }
        }
    }
    apps
}

fn parse_desktop_entry(path: &Path, content: &str) -> Option<FileEntry> {
    let mut in_main = false;
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut entry_type: Option<String> = None;
    let mut hidden = false;

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" if name.is_none() => name = Some(v.trim().to_owned()),
            "Exec" => exec = Some(v.trim().to_owned()),
            "Type" => entry_type = Some(v.trim().to_owned()),
            "NoDisplay" | "Hidden" if v.trim().eq_ignore_ascii_case("true") => hidden = true,
            _ => {}
        }
    }

    if hidden || entry_type.as_deref() != Some("Application") {
        return None;
    }
    let name = name?;
    let meta = fs::metadata(path).ok();
    let modified_unix_secs = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

    Some(FileEntry {
        name,
        path: path.to_path_buf(),
        kind: FileEntryKind::Application,
        size_bytes: meta.map_or(0, |m| m.len()),
        modified_unix_secs,
        hidden: false,
        symlink_target_is_dir: false,
        desktop_exec: exec,
        trash_original_path: None,
        trash_deletion_date: None,
        subtitle: Some(String::from("Application")),
    })
}

fn collect_recent_entries(locations: &UserFileLocations) -> Vec<FileEntry> {
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    let xbel_path = locations.data_home.join("recently-used.xbel");
    if let Ok(xml) = fs::read_to_string(&xbel_path) {
        for path in parse_recently_used_xbel_paths(&xml) {
            if results.len() >= 120 {
                break;
            }
            if seen.insert(path.clone())
                && let Some(entry) = build_file_entry(&path, None)
            {
                results.push(entry);
            }
        }
    }

    for folder in [
        &locations.documents,
        &locations.downloads,
        &locations.desktop,
    ] {
        collect_shallow_recent_files(folder, 1, &mut seen, &mut results);
    }
    results.sort_by_key(|a| std::cmp::Reverse(a.modified_unix_secs));
    results.truncate(120);
    results
}

fn collect_shallow_recent_files(
    dir: &Path,
    depth_remaining: u8,
    seen: &mut BTreeSet<PathBuf>,
    out: &mut Vec<FileEntry>,
) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    for entry_res in read_dir.flatten() {
        if out.len() >= 200 {
            return;
        }
        let path = entry_res.path();
        let Some(item) = build_file_entry(&path, None) else {
            continue;
        };
        if item.hidden {
            continue;
        }
        if item.kind == FileEntryKind::Directory {
            if depth_remaining > 0 {
                collect_shallow_recent_files(&path, depth_remaining - 1, seen, out);
            }
            continue;
        }
        if item.kind == FileEntryKind::RegularFile && seen.insert(path) {
            out.push(item);
        }
    }
}

fn parse_recently_used_xbel_paths(xml: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut rest = xml;
    while let Some(pos) = rest.find("href=\"file://") {
        let after = &rest[pos + "href=\"file://".len()..];
        let Some(end_quote) = after.find('"') else {
            break;
        };
        let encoded = &after[..end_quote];
        let decoded = percent_decode(encoded);
        let p = PathBuf::from(&decoded);
        if p.exists() {
            paths.push(p);
        }
        rest = &after[end_quote + 1..];
    }
    paths
}

fn collect_shared_entries(locations: &UserFileLocations) -> Vec<FileEntry> {
    let mut items = Vec::new();
    if locations.public_share.is_dir() {
        items.push(FileEntry {
            name: String::from("Public"),
            path: locations.public_share.clone(),
            kind: FileEntryKind::Directory,
            size_bytes: 0,
            modified_unix_secs: None,
            hidden: false,
            symlink_target_is_dir: false,
            desktop_exec: None,
            trash_original_path: None,
            trash_deletion_date: None,
            subtitle: Some(String::from("Public Share")),
        });
    }
    items.extend(collect_network_entries(locations));
    items
}

fn collect_network_entries(locations: &UserFileLocations) -> Vec<FileEntry> {
    let mut items = Vec::new();
    for vol in discover_mounts() {
        if !vol.is_network {
            continue;
        }
        items.push(FileEntry {
            name: vol.name,
            path: vol.mount_point,
            kind: FileEntryKind::MountPoint,
            size_bytes: 0,
            modified_unix_secs: None,
            hidden: false,
            symlink_target_is_dir: false,
            desktop_exec: None,
            trash_original_path: None,
            trash_deletion_date: None,
            subtitle: Some(format!("{} ({})", vol.device, vol.fs_type)),
        });
    }
    let uid_gvfs = locations
        .data_home
        .parent()
        .and_then(|_| env::var("XDG_RUNTIME_DIR").ok())
        .map(|rt| PathBuf::from(rt).join("gvfs"));
    if let Some(gvfs_dir) = uid_gvfs
        && let Ok(rd) = fs::read_dir(&gvfs_dir)
    {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(name) = p.file_name().and_then(|n| n.to_str())
            {
                items.push(FileEntry {
                    name: name.to_owned(),
                    path: p,
                    kind: FileEntryKind::MountPoint,
                    size_bytes: 0,
                    modified_unix_secs: None,
                    hidden: false,
                    symlink_target_is_dir: false,
                    desktop_exec: None,
                    trash_original_path: None,
                    trash_deletion_date: None,
                    subtitle: Some(String::from("Network Mount")),
                });
            }
        }
    }
    items
}

fn collect_tagged_entries(tags: &FileTagsConfig, color: FileTagColor) -> Vec<FileEntry> {
    tags.paths_with_tag(color)
        .into_iter()
        .filter_map(|p| build_file_entry(&p, None))
        .collect()
}

/// Filters and sorts `entries` according to visibility, search query, field, and order.
#[must_use]
pub fn filter_and_sort_entries(
    mut entries: Vec<FileEntry>,
    show_hidden: bool,
    query: &str,
    sort_field: FileSortField,
    sort_order: FileSortOrder,
) -> Vec<FileEntry> {
    let needle = query.trim().to_ascii_lowercase();
    entries.retain(|entry| {
        if !show_hidden && entry.hidden {
            return false;
        }
        if needle.is_empty() {
            return true;
        }
        entry.name.to_ascii_lowercase().contains(&needle)
    });

    entries.sort_by(|a, b| compare_entries(a, b, sort_field, sort_order));
    entries
}

fn compare_entries(
    a: &FileEntry,
    b: &FileEntry,
    field: FileSortField,
    order: FileSortOrder,
) -> std::cmp::Ordering {
    let a_dir = a.is_navigable_dir();
    let b_dir = b.is_navigable_dir();
    let dir_cmp = b_dir.cmp(&a_dir);
    if dir_cmp != std::cmp::Ordering::Equal {
        return dir_cmp;
    }

    let primary = match field {
        FileSortField::Name => a
            .name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.name.cmp(&b.name)),
        FileSortField::DateModified => a
            .modified_unix_secs
            .unwrap_or(0)
            .cmp(&b.modified_unix_secs.unwrap_or(0)),
        FileSortField::Size => a.size_bytes.cmp(&b.size_bytes),
        FileSortField::Kind => a.kind_label().cmp(b.kind_label()).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        }),
    };
    let ordered = match order {
        FileSortOrder::Ascending => primary,
        FileSortOrder::Descending => primary.reverse(),
    };
    ordered.then_with(|| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    })
}
