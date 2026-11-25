//! Clipboard operations, file duplicate, transfer worker orchestration, and trash.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::thread;

use rusty_services::file_manager::{
    FileEntry, TransferConflictDecision, TransferKind, TransferProgress,
    copy_or_move_with_progress, create_unique_folder, empty_trash, move_to_trash, rename_entry,
    restore_from_trash,
};

use super::*;

const MAX_UNDO_HISTORY: usize = 32;

impl FilesAppState {
    pub(crate) fn push_undo(&mut self, action: UndoAction) {
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(action);
    }

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
                    .unwrap_or("untitled folder")
                    .to_owned();
                self.push_undo(UndoAction::RemoveCreated {
                    created_paths: vec![created],
                });
                self.status_message = Some(format!("Created {name}"));
                self.reload_directory(&dir);
            }
            Err(err) => {
                self.status_message = Some(format!("New Folder failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Moves all currently selected entries to Trash, or restores if inside Trash (`RUSTYOS-150`).
    pub(crate) fn action_move_selected_to_trash(&mut self) {
        self.popup = None;
        let entries = self.selected_entries();
        if entries.is_empty() {
            self.status_message = Some(String::from("Select an item first"));
            self.dirty = true;
            return;
        }
        if entries.len() == 1 {
            self.trash_entry(&entries[0]);
            return;
        }
        if self.is_trash_view() {
            let mut restored = 0_usize;
            for entry in &entries {
                if restore_from_trash(&entry.name, &self.locations.trash_dir).is_ok() {
                    restored += 1;
                }
            }
            self.status_message = Some(format!("Restored {restored} items"));
            self.reload_active_column();
            self.dirty = true;
            return;
        }
        let mut trash_names = Vec::new();
        for entry in &entries {
            if let Ok(dest) = move_to_trash(&entry.path, &self.locations.trash_dir) {
                self.tags_config.remove_tags_for(&entry.path);
                if let Some(name) = dest.file_name().and_then(|n| n.to_str()) {
                    trash_names.push(name.to_owned());
                }
            }
        }
        let trashed = trash_names.len();
        if !trash_names.is_empty() {
            self.push_undo(UndoAction::RestoreTrashed { trash_names });
        }
        self.persist_preferences();
        self.status_message = Some(format!("Moved {trashed} items to Trash"));
        self.reload_active_column();
        self.dirty = true;
    }

    /// Moves specific `entry` to `FreeDesktop` Trash (updating tags & reloading).
    pub(crate) fn trash_entry(&mut self, entry: &FileEntry) {
        if self.is_trash_view() {
            self.restore_entry(entry);
            return;
        }
        match move_to_trash(&entry.path, &self.locations.trash_dir) {
            Ok(dest) => {
                self.tags_config.remove_tags_for(&entry.path);
                if let Some(name) = dest.file_name().and_then(|n| n.to_str()) {
                    self.push_undo(UndoAction::RestoreTrashed {
                        trash_names: vec![name.to_owned()],
                    });
                }
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

    /// Moves arbitrary dropped `sources` into `FreeDesktop` Trash (called by Trash `DnD` drop).
    pub(crate) fn action_trash_paths(&mut self, sources: Vec<PathBuf>) {
        self.popup = None;
        if sources.is_empty() || self.is_trash_view() {
            return;
        }
        let mut trash_names = Vec::new();
        let mut parents = Vec::new();
        for src in sources {
            if let Ok(dest) = move_to_trash(&src, &self.locations.trash_dir) {
                self.tags_config.remove_tags_for(&src);
                if let Some(name) = dest.file_name().and_then(|n| n.to_str()) {
                    trash_names.push(name.to_owned());
                }
                if let Some(p) = src.parent() {
                    let parent_buf = p.to_path_buf();
                    if !parents.contains(&parent_buf) {
                        parents.push(parent_buf);
                    }
                }
            }
        }
        let trashed = trash_names.len();
        if trashed == 0 {
            self.status_message = Some(String::from("Move to Trash failed"));
            self.dirty = true;
            return;
        }
        self.push_undo(UndoAction::RestoreTrashed { trash_names });
        self.persist_preferences();
        for p in &parents {
            self.reload_directory(p);
        }
        self.reload_active_column();
        self.status_message = Some(format!(
            "Moved {trashed} item{} to Trash",
            if trashed == 1 { "" } else { "s" }
        ));
        self.dirty = true;
    }

    /// Reverses the most recent file operation recorded on [`FilesAppState::undo_stack`] (`Ctrl+Z`).
    pub(crate) fn action_undo(&mut self) {
        self.popup = None;
        let Some(action) = self.undo_stack.pop() else {
            self.status_message = Some(String::from("Nothing to undo"));
            self.dirty = true;
            return;
        };
        let mut reload_dirs: Vec<PathBuf> = Vec::new();
        let mut collect_parent = |p: &Path| {
            if let Some(parent) = p.parent() {
                let pb = parent.to_path_buf();
                if !reload_dirs.contains(&pb) {
                    reload_dirs.push(pb);
                }
            }
        };
        let msg = match action {
            UndoAction::Rename { new_path, old_path } => {
                match Self::undo_single_rename(&mut self.tags_config, &new_path, &old_path) {
                    Ok(restored_name) => {
                        collect_parent(&old_path);
                        format!("Undid rename to {restored_name}")
                    }
                    Err(err) => format!("Undo rename failed: {err}"),
                }
            }
            UndoAction::BatchRename { pairs } => {
                let mut restored = 0_usize;
                for (new_path, old_path) in pairs.into_iter().rev() {
                    if Self::undo_single_rename(&mut self.tags_config, &new_path, &old_path).is_ok()
                    {
                        collect_parent(&old_path);
                        restored += 1;
                    }
                }
                format!("Undid batch rename ({restored} items)")
            }
            UndoAction::RemoveCreated { created_paths } => {
                let mut trashed = 0_usize;
                for p in created_paths {
                    if move_to_trash(&p, &self.locations.trash_dir).is_ok() {
                        self.tags_config.remove_tags_for(&p);
                        collect_parent(&p);
                        trashed += 1;
                    }
                }
                format!("Undid creation ({trashed} moved to Trash)")
            }
            UndoAction::RestoreTrashed { trash_names } => {
                let mut restored = 0_usize;
                for name in trash_names {
                    if let Ok(orig) = restore_from_trash(&name, &self.locations.trash_dir) {
                        collect_parent(&orig);
                        restored += 1;
                    }
                }
                format!("Undid trash ({restored} restored)")
            }
            UndoAction::MoveBack { pairs } => {
                let mut restored = 0_usize;
                let cancel = AtomicBool::new(false);
                for (current, original) in pairs {
                    if let Some(orig_dir) = original.parent() {
                        let ok = if current.file_name() == original.file_name() {
                            copy_or_move_with_progress(
                                std::slice::from_ref(&current),
                                orig_dir,
                                TransferKind::Move,
                                &cancel,
                                |_| {},
                                |_, _| TransferConflictDecision::Skip,
                            )
                            .is_ok_and(|o| !o.created_paths.is_empty())
                        } else {
                            std::fs::rename(&current, &original).is_ok()
                        };
                        if ok {
                            collect_parent(&current);
                            collect_parent(&original);
                            restored += 1;
                        }
                    }
                }
                format!("Undid move ({restored} restored)")
            }
        };
        self.persist_preferences();
        for dir in &reload_dirs {
            self.reload_directory(dir);
        }
        self.reload_active_column();
        self.status_message = Some(msg);
        self.dirty = true;
    }

    fn undo_single_rename(
        tags_config: &mut rusty_services::file_manager::FileTagsConfig,
        new_path: &Path,
        old_path: &Path,
    ) -> std::io::Result<String> {
        let old_name = old_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing old name")
            })?
            .to_owned();
        let renamed = rename_entry(new_path, &old_name)?;
        let existing_tags: Vec<rusty_services::file_manager::FileTagColor> =
            tags_config.tags_for(new_path).to_vec();
        if !existing_tags.is_empty() {
            tags_config.remove_tags_for(new_path);
            for tag in existing_tags {
                tags_config.toggle_tag(&renamed, tag);
            }
        }
        Ok(old_name)
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
        let entries = self.selected_entries();
        if entries.is_empty() {
            self.status_message = Some(String::from("Select an item first"));
            self.dirty = true;
            return;
        }
        if self.is_trash_view() {
            return;
        }
        let paths: Vec<PathBuf> = entries.iter().map(|e| e.path.clone()).collect();
        let staged = ClipboardItemSet {
            kind,
            paths: paths.clone(),
        };
        self.clipboard = Some(staged.clone());
        self.pending_uri_clipboard = Some(staged);
        self.has_external_uri_clipboard = false;
        let label = match kind {
            TransferKind::Copy => "Copied",
            TransferKind::Move => "Cut",
        };
        self.status_message = if entries.len() == 1 {
            Some(format!("{label} \"{}\"", entries[0].name))
        } else {
            Some(format!("{label} {} items", entries.len()))
        };
        self.dirty = true;
    }

    pub(crate) fn take_pending_uri_clipboard(&mut self) -> Option<ClipboardItemSet> {
        self.pending_uri_clipboard.take()
    }

    pub(crate) fn take_pending_external_uri_paste_dir(&mut self) -> Option<PathBuf> {
        self.pending_external_uri_paste_dir.take()
    }

    /// Pastes clipboard items into active directory (`Ctrl+V`) in background thread.
    pub(crate) fn action_paste_into_active(&mut self) {
        self.popup = None;
        if self.active_transfer.is_some() {
            self.status_message = Some(String::from("Transfer already in progress"));
            self.dirty = true;
            return;
        }
        let Some(dest_dir) = self.active_writable_directory() else {
            self.status_message = Some(String::from("Cannot paste into virtual location"));
            self.dirty = true;
            return;
        };
        if self.has_external_uri_clipboard {
            self.pending_external_uri_paste_dir = Some(dest_dir);
            self.dirty = true;
            return;
        }
        let Some(clip) = self.clipboard.clone() else {
            self.status_message = Some(String::from("Clipboard is empty"));
            self.dirty = true;
            return;
        };
        self.start_background_transfer(clip.paths, dest_dir, clip.kind);
    }

    /// Duplicates all selected items in their parent folder (`Ctrl+D`).
    pub(crate) fn action_duplicate_selected(&mut self) {
        self.popup = None;
        if self.active_transfer.is_some() || self.is_trash_view() {
            return;
        }
        let selected = self.selected_entries();
        if selected.is_empty() {
            return;
        }
        let Some(dest_dir) = selected[0].path.parent().map(Path::to_path_buf) else {
            return;
        };
        let paths: Vec<PathBuf> = selected.into_iter().map(|e| e.path).collect();
        self.start_background_transfer(paths, dest_dir, TransferKind::Copy);
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
        self.pending_transfer_sources.clone_from(&sources);
        self.pending_transfer_record_undo = true;
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
        let ping = self.worker_ping.clone();
        thread::spawn(move || {
            let res = copy_or_move_with_progress(
                &sources,
                &dest_dir,
                kind,
                &cancel_flag,
                |progress| {
                    let _ = tx.send(TransferWorkerMsg::Progress(progress.clone()));
                    if let Some(p) = &ping {
                        p.ping();
                    }
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
                    if let Some(p) = &ping {
                        p.ping();
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
            if let Some(p) = ping {
                p.ping();
            }
        });
        self.dirty = true;
    }
}
