//! FreeDesktop.org Trash specification implementation (`FreeDesktop Trash`).

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::listing::build_file_entry;
use super::metadata::{absolutize_path, format_iso8601_utc, percent_decode, percent_encode_path};
use super::operations::{
    move_entry_no_follow, pick_unique_sibling_path, remove_path_no_follow,
    validate_single_component_name,
};
use super::types::FileEntry;

/// Lists `FreeDesktop` Trash items from `<trash_dir>/files` joined with `.trashinfo`.
///
/// # Errors
/// Returns an `io::Error` if `<trash_dir>/files` exists and cannot be read.
pub fn list_trash_entries(trash_dir: &Path) -> io::Result<Vec<FileEntry>> {
    let files_dir = trash_dir.join("files");
    if !files_dir.exists() {
        return Ok(Vec::new());
    }
    let info_dir = trash_dir.join("info");
    let mut items = Vec::new();
    for entry_res in fs::read_dir(&files_dir)? {
        let Ok(entry) = entry_res else {
            continue;
        };
        let path = entry.path();
        let Some(name_os) = path.file_name() else {
            continue;
        };
        let name = name_os.to_string_lossy().into_owned();
        let Some(mut item) = build_file_entry(&path, Some(name.clone())) else {
            continue;
        };
        let info_path = info_dir.join(format!("{name}.trashinfo"));
        if let Ok(info_text) = fs::read_to_string(&info_path) {
            let (orig, del_date) = parse_trashinfo(&info_text);
            item.trash_original_path = orig;
            item.trash_deletion_date = del_date;
        }
        items.push(item);
    }
    Ok(items)
}

fn parse_trashinfo(content: &str) -> (Option<PathBuf>, Option<String>) {
    let mut orig = None;
    let mut del_date = None;
    for raw in content.lines() {
        let line = raw.trim();
        if let Some(v) = line.strip_prefix("Path=") {
            let decoded = percent_decode(v);
            if !decoded.is_empty() {
                orig = Some(PathBuf::from(decoded));
            }
        } else if let Some(v) = line.strip_prefix("DeletionDate=") {
            del_date = Some(v.to_owned());
        }
    }
    (orig, del_date)
}

/// Moves `source` into the `FreeDesktop` Trash directory (`files/` + `info/<name>.trashinfo`).
///
/// # Errors
/// Returns an `io::Error` if the path is invalid, protected, or filesystem operations fail.
pub fn move_to_trash(source: &Path, trash_dir: &Path) -> io::Result<PathBuf> {
    if source == Path::new("/") {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Refusing to trash filesystem root",
        ));
    }
    if source == trash_dir || source.starts_with(trash_dir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Path is already inside Trash",
        ));
    }
    let meta = fs::symlink_metadata(source)?;
    let orig_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing file name"))?;

    let files_dir = trash_dir.join("files");
    let info_dir = trash_dir.join("info");
    fs::create_dir_all(&files_dir)?;
    fs::create_dir_all(&info_dir)?;

    let abs_source = absolutize_path(source)?;
    let (trash_name, info_path) =
        reserve_trash_slot(&files_dir, &info_dir, orig_name, &abs_source)?;
    let dest = files_dir.join(&trash_name);

    let move_res = move_entry_no_follow(source, &dest, meta.file_type().is_dir());
    if let Err(err) = move_res {
        let _ = fs::remove_file(&info_path);
        return Err(err);
    }
    Ok(dest)
}

fn reserve_trash_slot(
    files_dir: &Path,
    info_dir: &Path,
    orig_name: &str,
    abs_source: &Path,
) -> io::Result<(String, PathBuf)> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_i64, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let del_date = format_iso8601_utc(now_secs);
    let encoded_orig = percent_encode_path(&abs_source.to_string_lossy());
    let trashinfo_body = format!("[Trash Info]\nPath={encoded_orig}\nDeletionDate={del_date}\n");

    for attempt in 1..=9999_u32 {
        let candidate = if attempt == 1 {
            orig_name.to_owned()
        } else {
            format!("{orig_name}.{attempt}")
        };
        let dest_path = files_dir.join(&candidate);
        if dest_path.symlink_metadata().is_ok() {
            continue;
        }
        let info_path = info_dir.join(format!("{candidate}.trashinfo"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_path)
        {
            Ok(mut f) => {
                if let Err(err) = f
                    .write_all(trashinfo_body.as_bytes())
                    .and_then(|()| f.flush())
                {
                    let _ = fs::remove_file(&info_path);
                    return Err(err);
                }
                return Ok((candidate, info_path));
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Exhausted unique Trash entry names",
    ))
}

/// Restores a trashed entry named `trash_entry_name` back to its `.trashinfo` original path.
///
/// # Errors
/// Returns an `io::Error` if the `.trashinfo` file is missing/malformed or move fails.
pub fn restore_from_trash(trash_entry_name: &str, trash_dir: &Path) -> io::Result<PathBuf> {
    let clean = validate_single_component_name(trash_entry_name)
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidInput, msg))?;
    let trashed_path = trash_dir.join("files").join(clean);
    let info_path = trash_dir.join("info").join(format!("{clean}.trashinfo"));
    let meta = fs::symlink_metadata(&trashed_path)?;
    let info_text = fs::read_to_string(&info_path)?;
    let (orig_opt, _) = parse_trashinfo(&info_text);
    let orig_path = orig_opt.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing original Path= in .trashinfo",
        )
    })?;

    if let Some(parent) = orig_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let final_dest = pick_non_colliding_restore_path(&orig_path);
    move_entry_no_follow(&trashed_path, &final_dest, meta.file_type().is_dir())?;
    let _ = fs::remove_file(&info_path);
    Ok(final_dest)
}

fn pick_non_colliding_restore_path(orig_path: &Path) -> PathBuf {
    if orig_path.symlink_metadata().is_err() {
        return orig_path.to_path_buf();
    }
    pick_unique_sibling_path(orig_path, "restored", |n| {
        if n == 1 {
            String::from(" (restored)")
        } else {
            format!(" (restored {n})")
        }
    })
}

/// Permanently deletes all contents inside `<trash_dir>/files` and `<trash_dir>/info`.
///
/// Returns the number of trashed items deleted.
///
/// # Errors
/// Returns an `io::Error` if iterating or deleting entries fails.
pub fn empty_trash(trash_dir: &Path) -> io::Result<usize> {
    let files_dir = trash_dir.join("files");
    let info_dir = trash_dir.join("info");
    let mut removed = 0_usize;

    if files_dir.is_dir() {
        for entry_res in fs::read_dir(&files_dir)? {
            let entry = entry_res?;
            remove_path_no_follow(&entry.path())?;
            removed += 1;
        }
    }
    if info_dir.is_dir() {
        for entry_res in fs::read_dir(&info_dir)? {
            let entry = entry_res?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("trashinfo") {
                remove_path_no_follow(&path)?;
            }
        }
    }
    Ok(removed)
}
