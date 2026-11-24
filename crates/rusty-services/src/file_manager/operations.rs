//! File system operations: copy, move, rename, duplicate, and progress tracking.

use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Validates a single folder/file basename (no path separators, `.`, `..`, or NUL).
///
/// # Errors
/// Returns a static diagnostic message if invalid.
pub fn validate_single_component_name(raw: &str) -> Result<&str, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Name must not be empty");
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Reserved name '.' or '..' is not allowed");
    }
    if trimmed.contains('/') || trimmed.contains('\0') {
        return Err("Path separators are not allowed in entry names");
    }
    Ok(trimmed)
}

/// Creates a uniquely named directory inside `parent` (`<base_name>`, `<base_name> 2`, ...).
///
/// # Errors
/// Returns an `io::Error` if validation fails or directory creation cannot succeed.
pub fn create_unique_folder(parent: &Path, base_name: &str) -> io::Result<PathBuf> {
    let clean = validate_single_component_name(base_name)
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidInput, msg))?;
    for idx in 1..=9999_u32 {
        let candidate_name = if idx == 1 {
            clean.to_owned()
        } else {
            format!("{clean} {idx}")
        };
        let target = parent.join(&candidate_name);
        match fs::create_dir(&target) {
            Ok(()) => return Ok(target),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Exhausted folder name attempts",
    ))
}

pub(crate) fn move_entry_no_follow(source: &Path, dest: &Path, is_dir: bool) -> io::Result<()> {
    match fs::rename(source, dest) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => {
            if let Err(copy_err) = copy_recursive_no_follow(source, dest) {
                let _ = remove_path_no_follow(dest);
                return Err(copy_err);
            }
            if is_dir {
                fs::remove_dir_all(source)?;
            } else {
                fs::remove_file(source)?;
            }
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn copy_recursive_no_follow(source: &Path, dest: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(source)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        let target = fs::read_link(source)?;
        symlink(&target, dest)?;
    } else if ft.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            copy_recursive_no_follow(&child_src, &child_dest)?;
        }
    } else if ft.is_file() {
        fs::copy(source, dest)?;
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Cannot move special filesystem node across devices",
        ));
    }
    Ok(())
}

pub(crate) fn remove_path_no_follow(path: &Path) -> io::Result<()> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Returns a non-colliding sibling copy path for `dest` (e.g. `file copy.txt` or `file copy 2.txt`).
#[must_use]
pub fn pick_duplicate_copy_path(dest: &Path) -> PathBuf {
    pick_unique_sibling_path(dest, "copy", |n| {
        if n == 1 {
            String::from(" copy")
        } else {
            format!(" copy {n}")
        }
    })
}

/// Alias for [`pick_duplicate_copy_path`] (`RUSTYOS-168`).
#[must_use]
pub fn pick_unique_copy_path(base_path: &Path) -> PathBuf {
    pick_duplicate_copy_path(base_path)
}

pub(crate) fn pick_unique_sibling_path<F>(
    base_path: &Path,
    fallback_stem: &str,
    mut suffix_for: F,
) -> PathBuf
where
    F: FnMut(u32) -> String,
{
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FALLBACK_SEQ: AtomicU64 = AtomicU64::new(1);
    let parent = base_path.parent().unwrap_or_else(|| Path::new("/"));
    let stem = base_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(fallback_stem);
    let ext = base_path.extension().and_then(|e| e.to_str());
    for n in 1..=999_u32 {
        let suffix = suffix_for(n);
        let file_name = match ext {
            Some(e) => format!("{stem}{suffix}.{e}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = parent.join(file_name);
        if candidate.symlink_metadata().is_err() {
            return candidate;
        }
    }
    // Never fall back to returning the existing `base_path` (which would truncate on `File::create`).
    loop {
        let seq = FALLBACK_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let suffix = format!("{}-{nanos}-{seq}", suffix_for(1000));
        let file_name = match ext {
            Some(e) => format!("{stem}{suffix}.{e}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = parent.join(file_name);
        if candidate.symlink_metadata().is_err() {
            return candidate;
        }
    }
}

/// Safely renames `source` inside its parent directory after component validation and collision checks.
///
/// # Errors
/// Returns an `io::Error` if `new_name` is invalid, a target with that name already exists, or rename fails.
pub fn rename_entry(source: &Path, new_name: &str) -> io::Result<PathBuf> {
    let clean = validate_single_component_name(new_name)
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidInput, msg))?;
    let _src_meta = fs::symlink_metadata(source)?;
    let parent = source.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Cannot rename filesystem root")
    })?;
    let dest = parent.join(clean);
    if dest == source {
        return Ok(dest);
    }
    if dest.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("An item named '{clean}' already exists"),
        ));
    }
    fs::rename(source, &dest)?;
    Ok(dest)
}

/// File transfer operation kind (`Copy` vs `Move`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    /// Copy entries cleanly without modifying sources.
    Copy,
    /// Move entries into destination (`fs::rename` fast path or chunked cross-device copy + delete).
    Move,
}

/// User conflict resolution choice when a destination item already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferConflictDecision {
    /// Remove the colliding destination item first and write the incoming item in its place.
    Replace,
    /// Keep the existing destination item and rename the incoming copy (`name copy`).
    KeepBoth,
    /// Leave the colliding destination untouched and skip this source item.
    Skip,
    /// Cancel/stop the entire transfer batch immediately.
    Stop,
}

/// Live progress snapshot reported from [`copy_or_move_with_progress`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferProgress {
    /// Operation kind.
    pub kind: TransferKind,
    /// Basename of the entry currently being copied or moved.
    pub current_item_name: String,
    /// Target directory path.
    pub destination_dir: PathBuf,
    /// Total planned bytes across regular files.
    pub total_bytes: u64,
    /// Bytes completed so far.
    pub completed_bytes: u64,
    /// Planned item count.
    pub total_items: usize,
    /// Items processed so far.
    pub completed_items: usize,
}

/// Summary returned when [`copy_or_move_with_progress`] completes or stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOutcome {
    /// Destination paths created or moved into `dest_dir`.
    pub created_paths: Vec<PathBuf>,
    /// Number of skipped or uncopied items.
    pub skipped_items: usize,
    /// True if halted early via `cancel` flag or [`TransferConflictDecision::Stop`].
    pub cancelled: bool,
}

struct TransferPlanItem {
    source: PathBuf,
    bytes: u64,
    items: usize,
    is_dir: bool,
}

/// Copies or moves `sources` into `dest_dir` with chunked progress callbacks, cancellation,
/// cycle protection, and interactive conflict handling.
///
/// # Errors
/// Returns an `io::Error` if `dest_dir` is not an existing directory, a source would be copied
/// into itself or a descendant, or an I/O failure occurs.
pub fn copy_or_move_with_progress<P, C>(
    sources: &[PathBuf],
    dest_dir: &Path,
    kind: TransferKind,
    cancel: &AtomicBool,
    mut on_progress: P,
    mut on_conflict: C,
) -> io::Result<TransferOutcome>
where
    P: FnMut(&TransferProgress),
    C: FnMut(&Path, &Path) -> TransferConflictDecision,
{
    if !dest_dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Destination directory does not exist",
        ));
    }
    let canon_dest_dir = fs::canonicalize(dest_dir)?;
    let (plan, total_bytes, total_items) = build_transfer_plan(sources, &canon_dest_dir)?;

    let mut progress = TransferProgress {
        kind,
        current_item_name: String::new(),
        destination_dir: dest_dir.to_path_buf(),
        total_bytes,
        completed_bytes: 0,
        total_items,
        completed_items: 0,
    };
    on_progress(&progress);

    let mut created_paths = Vec::new();
    let mut skipped_items = 0_usize;
    let mut cancelled = false;

    for (idx, item) in plan.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            skipped_items += plan.len().saturating_sub(idx);
            break;
        }
        let Some(base_name) = item.source.file_name() else {
            skipped_items += 1;
            continue;
        };
        progress.current_item_name = base_name.to_string_lossy().into_owned();
        let raw_dest = dest_dir.join(base_name);

        match resolve_transfer_target(
            &item.source,
            &raw_dest,
            &canon_dest_dir,
            kind,
            &mut on_conflict,
        )? {
            TransferTarget::Proceed(final_dest) => {
                let done = execute_single_transfer(
                    item,
                    &final_dest,
                    kind,
                    cancel,
                    &mut progress,
                    &mut on_progress,
                )?;
                if done {
                    created_paths.push(final_dest);
                } else {
                    cancelled = true;
                    skipped_items += plan.len().saturating_sub(idx);
                    break;
                }
            }
            TransferTarget::Skip => {
                skipped_items += 1;
                progress.completed_items = progress.completed_items.saturating_add(item.items);
                progress.completed_bytes = progress.completed_bytes.saturating_add(item.bytes);
                on_progress(&progress);
            }
            TransferTarget::Stop => {
                cancelled = true;
                skipped_items += plan.len().saturating_sub(idx);
                break;
            }
        }
    }

    on_progress(&progress);
    Ok(TransferOutcome {
        created_paths,
        skipped_items,
        cancelled,
    })
}

enum TransferTarget {
    Proceed(PathBuf),
    Skip,
    Stop,
}

fn build_transfer_plan(
    sources: &[PathBuf],
    canon_dest_dir: &Path,
) -> io::Result<(Vec<TransferPlanItem>, u64, usize)> {
    let mut plan = Vec::with_capacity(sources.len());
    let mut total_bytes = 0_u64;
    let mut total_items = 0_usize;

    for src in sources {
        let meta = fs::symlink_metadata(src)?;
        let is_dir = meta.file_type().is_dir();
        if is_dir {
            let canon_src = fs::canonicalize(src)?;
            if canon_dest_dir == canon_src || canon_dest_dir.starts_with(&canon_src) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Cannot copy or move a folder into itself",
                ));
            }
        }
        let (bytes, items) = measure_transfer_tree(src)?;
        total_bytes = total_bytes.saturating_add(bytes);
        total_items = total_items.saturating_add(items);
        plan.push(TransferPlanItem {
            source: src.clone(),
            bytes,
            items,
            is_dir,
        });
    }
    Ok((plan, total_bytes, total_items))
}

fn measure_transfer_tree(root: &Path) -> io::Result<(u64, usize)> {
    let mut total_bytes = 0_u64;
    let mut total_items = 0_usize;
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        let meta = fs::symlink_metadata(&path)?;
        total_items = total_items.saturating_add(1);
        let ft = meta.file_type();
        if ft.is_file() {
            total_bytes = total_bytes.saturating_add(meta.len());
        } else if ft.is_dir() {
            for entry_res in fs::read_dir(&path)? {
                let entry = entry_res?;
                stack.push(entry.path());
            }
        }
    }
    Ok((total_bytes, total_items))
}

fn resolve_transfer_target<C>(
    source: &Path,
    raw_dest: &Path,
    canon_dest_dir: &Path,
    kind: TransferKind,
    on_conflict: &mut C,
) -> io::Result<TransferTarget>
where
    C: FnMut(&Path, &Path) -> TransferConflictDecision,
{
    let same_dir = source
        .parent()
        .and_then(|p| fs::canonicalize(p).ok())
        .is_some_and(|p| p == *canon_dest_dir);

    if same_dir {
        return Ok(match kind {
            TransferKind::Move => TransferTarget::Skip,
            TransferKind::Copy => TransferTarget::Proceed(pick_duplicate_copy_path(raw_dest)),
        });
    }

    if raw_dest.symlink_metadata().is_err() {
        return Ok(TransferTarget::Proceed(raw_dest.to_path_buf()));
    }

    match on_conflict(source, raw_dest) {
        TransferConflictDecision::Replace => {
            remove_path_no_follow(raw_dest)?;
            Ok(TransferTarget::Proceed(raw_dest.to_path_buf()))
        }
        TransferConflictDecision::KeepBoth => {
            Ok(TransferTarget::Proceed(pick_duplicate_copy_path(raw_dest)))
        }
        TransferConflictDecision::Skip => Ok(TransferTarget::Skip),
        TransferConflictDecision::Stop => Ok(TransferTarget::Stop),
    }
}

fn execute_single_transfer<P>(
    item: &TransferPlanItem,
    final_dest: &Path,
    kind: TransferKind,
    cancel: &AtomicBool,
    progress: &mut TransferProgress,
    on_progress: &mut P,
) -> io::Result<bool>
where
    P: FnMut(&TransferProgress),
{
    if kind == TransferKind::Move {
        match fs::rename(&item.source, final_dest) {
            Ok(()) => {
                progress.completed_bytes = progress.completed_bytes.saturating_add(item.bytes);
                progress.completed_items = progress.completed_items.saturating_add(item.items);
                on_progress(progress);
                return Ok(true);
            }
            Err(err) if err.kind() == io::ErrorKind::CrossesDevices => {}
            Err(err) => return Err(err),
        }
    }

    let copied_all =
        copy_tree_with_progress(&item.source, final_dest, cancel, progress, on_progress)?;
    if !copied_all {
        let _ = remove_path_no_follow(final_dest);
        return Ok(false);
    }
    if kind == TransferKind::Move {
        if item.is_dir {
            fs::remove_dir_all(&item.source)?;
        } else {
            fs::remove_file(&item.source)?;
        }
    }
    Ok(true)
}

fn copy_tree_with_progress<P>(
    src_path: &Path,
    dst_path: &Path,
    cancel: &AtomicBool,
    progress: &mut TransferProgress,
    on_progress: &mut P,
) -> io::Result<bool>
where
    P: FnMut(&TransferProgress),
{
    if cancel.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let meta = fs::symlink_metadata(src_path)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        let target = fs::read_link(src_path)?;
        symlink(&target, dst_path)?;
        progress.completed_items = progress.completed_items.saturating_add(1);
        on_progress(progress);
        return Ok(true);
    }
    if ft.is_file() {
        return copy_file_chunked(src_path, dst_path, &meta, cancel, progress, on_progress);
    }
    if ft.is_dir() {
        fs::create_dir_all(dst_path)?;
        progress.completed_items = progress.completed_items.saturating_add(1);
        for entry_res in fs::read_dir(src_path)? {
            let entry = entry_res?;
            let child_src = entry.path();
            let child_dst = dst_path.join(entry.file_name());
            if !copy_tree_with_progress(&child_src, &child_dst, cancel, progress, on_progress)? {
                return Ok(false);
            }
        }
        let _ = fs::set_permissions(dst_path, meta.permissions());
        on_progress(progress);
        return Ok(true);
    }
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Cannot copy special filesystem node",
    ))
}

fn copy_file_chunked<P>(
    src_path: &Path,
    dst_path: &Path,
    src_meta: &fs::Metadata,
    cancel: &AtomicBool,
    progress: &mut TransferProgress,
    on_progress: &mut P,
) -> io::Result<bool>
where
    P: FnMut(&TransferProgress),
{
    // Guard against truncating the source file if `src_path` and `dst_path` resolve to the same inode (`RUSTYOS-168`).
    if let Ok(dst_meta) = fs::metadata(dst_path) {
        use std::os::unix::fs::MetadataExt;
        if src_meta.dev() == dst_meta.dev() && src_meta.ino() == dst_meta.ino() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Refusing to overwrite source file with itself",
            ));
        }
    }
    let mut reader = File::open(src_path)?;
    let mut writer = File::create(dst_path)?;
    let mut chunk = vec![0_u8; 64 * 1024].into_boxed_slice();

    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(writer);
            let _ = fs::remove_file(dst_path);
            return Ok(false);
        }
        let read_bytes = reader.read(&mut chunk)?;
        if read_bytes == 0 {
            break;
        }
        writer.write_all(&chunk[..read_bytes])?;
        progress.completed_bytes = progress.completed_bytes.saturating_add(read_bytes as u64);
        on_progress(progress);
    }
    writer.flush()?;
    let _ = fs::set_permissions(dst_path, src_meta.permissions());
    progress.completed_items = progress.completed_items.saturating_add(1);
    on_progress(progress);
    Ok(true)
}
