//! Clipboard operations, file duplicate, transfer worker orchestration, and trash.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::thread;

use rusty_services::file_manager::{
    FileEntry, TransferConflictDecision, TransferKind, TransferProgress,
    copy_or_move_with_progress, create_unique_folder, empty_trash, move_to_trash,
    restore_from_trash,
};

use super::*;

impl FilesAppState {
    pub(crate) fn action_new_folder(&mut self) {
        self.popup = None;
        let Some(dir) = self.active_writable_directory() else {
            self.status_message = Some(String::from("Cannot create folder in virtual view"));
            self.dirty = true;
            return;
        };
        match create_unique_folder(&dir, "untitled folder") {
            Ok(created) => {
                let name = created
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("untitled folder");
                self.status_message = Some(format!("Created {name}"));
                self.reload_directory(&dir);
            }
            Err(err) => {
                self.status_message = Some(format!("New Folder failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Moves the currently selected entry to Trash, or prompts reload if inside Trash.
    pub(crate) fn action_move_selected_to_trash(&mut self) {
        self.popup = None;
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = Some(String::from("Select an item first"));
            self.dirty = true;
            return;
        };
        self.trash_entry(&entry);
    }

    /// Moves specific `entry` to `FreeDesktop` Trash (updating tags & reloading).
    pub(crate) fn trash_entry(&mut self, entry: &FileEntry) {
        if self.is_trash_view() {
            self.restore_entry(entry);
            return;
        }
        match move_to_trash(&entry.path, &self.locations.trash_dir) {
            Ok(_) => {
                self.tags_config.remove_tags_for(&entry.path);
                self.persist_preferences();
                self.status_message = Some(format!("Moved \"{}\" to Trash", entry.name));
                self.reload_active_column();
            }
            Err(err) => {
                self.status_message = Some(format!("Move to Trash failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Restores a trashed entry back to its original path.
    pub(crate) fn restore_entry(&mut self, entry: &FileEntry) {
        match restore_from_trash(&entry.name, &self.locations.trash_dir) {
            Ok(dest) => {
                self.status_message = Some(format!("Restored to {}", dest.display()));
                self.reload_active_column();
            }
            Err(err) => {
                self.status_message = Some(format!("Restore failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Empties the Trash cleanly.
    pub(crate) fn action_empty_trash(&mut self) {
        self.popup = None;
        match empty_trash(&self.locations.trash_dir) {
            Ok(count) => {
                self.status_message = Some(format!("Emptied Trash ({count} items removed)"));
                if self.is_trash_view() {
                    self.reload_active_column();
                }
            }
            Err(err) => {
                self.status_message = Some(format!("Empty Trash failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Stages selected path for Copy (`Ctrl+C`).
    pub(crate) fn action_copy_selected(&mut self) {
        self.stage_clipboard(TransferKind::Copy);
    }

    /// Stages selected path for Move (`Ctrl+X`).
    pub(crate) fn action_cut_selected(&mut self) {
        self.stage_clipboard(TransferKind::Move);
    }

    fn stage_clipboard(&mut self, kind: TransferKind) {
        self.popup = None;
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = Some(String::from("Select an item first"));
            self.dirty = true;
            return;
        };
        if self.is_trash_view() {
            return;
        }
        self.clipboard = Some(ClipboardItemSet {
            kind,
            paths: vec![entry.path],
        });
        let label = match kind {
            TransferKind::Copy => "Copied",
            TransferKind::Move => "Cut",
        };
        self.status_message = Some(format!("{label} \"{}\"", entry.name));
        self.dirty = true;
    }

    /// Pastes clipboard items into active directory (`Ctrl+V`) in background thread.
    pub(crate) fn action_paste_into_active(&mut self) {
        self.popup = None;
        if self.active_transfer.is_some() {
            self.status_message = Some(String::from("Transfer already in progress"));
            self.dirty = true;
            return;
        }
        let Some(clip) = self.clipboard.clone() else {
            self.status_message = Some(String::from("Clipboard is empty"));
            self.dirty = true;
            return;
        };
        let Some(dest_dir) = self.active_writable_directory() else {
            self.status_message = Some(String::from("Cannot paste into virtual location"));
            self.dirty = true;
            return;
        };
        self.start_background_transfer(clip.paths, dest_dir, clip.kind);
    }

    /// Duplicates selected item in its parent folder (`Ctrl+D`).
    pub(crate) fn action_duplicate_selected(&mut self) {
        self.popup = None;
        if self.active_transfer.is_some() {
            return;
        }
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if self.is_trash_view() {
            return;
        }
        let Some(dest_dir) = entry.path.parent().map(Path::to_path_buf) else {
            return;
        };
        self.start_background_transfer(vec![entry.path], dest_dir, TransferKind::Copy);
    }

    /// Drops dragged file paths onto `dest_dir` via the background transfer worker (`RUSTYOS-134`).
    pub(crate) fn action_drop_paths(
        &mut self,
        sources: Vec<PathBuf>,
        dest_dir: PathBuf,
        copy_mode: bool,
    ) {
        if self.active_transfer.is_some() || sources.is_empty() {
            return;
        }
        if sources
            .iter()
            .any(|src| src == &dest_dir || dest_dir.starts_with(src))
        {
            return;
        }
        let kind = if copy_mode {
            TransferKind::Copy
        } else {
            TransferKind::Move
        };
        self.start_background_transfer(sources, dest_dir, kind);
    }

    pub(crate) fn start_background_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        dest_dir: PathBuf,
        kind: TransferKind,
    ) {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let initial_name = sources
            .first()
            .and_then(|p| p.file_name())
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let initial_progress = TransferProgress {
            kind,
            current_item_name: initial_name,
            destination_dir: dest_dir.clone(),
            total_bytes: 0,
            completed_bytes: 0,
            total_items: sources.len(),
            completed_items: 0,
        };
        self.active_transfer = Some(ActiveTransferState {
            progress: initial_progress,
            cancel_flag: Arc::clone(&cancel_flag),
            conflict: None,
        });
        let tx = self.transfer_tx.clone();
        thread::spawn(move || {
            let res = copy_or_move_with_progress(
                &sources,
                &dest_dir,
                kind,
                &cancel_flag,
                |progress| {
                    let _ = tx.send(TransferWorkerMsg::Progress(progress.clone()));
                },
                |src, dst| {
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    if tx
                        .send(TransferWorkerMsg::Conflict {
                            source: src.to_path_buf(),
                            dest: dst.to_path_buf(),
                            reply_tx,
                        })
                        .is_err()
                    {
                        return TransferConflictDecision::Stop;
                    }
                    reply_rx.recv().unwrap_or(TransferConflictDecision::Stop)
                },
            )
            .map_err(|e| e.to_string());
            let _ = tx.send(TransferWorkerMsg::Finished {
                kind,
                dest_dir,
                result: res,
            });
        });
        self.dirty = true;
    }
}
