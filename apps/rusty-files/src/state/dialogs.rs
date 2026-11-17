//! Modal dialogs and overlay states (Quick Look, Get Info, Go to Folder, Rename, Transfer Conflict).

use std::path::Path;
use std::sync::atomic::Ordering;

use rusty_services::file_manager::{
    FileEntry, FileEntryKind, FileTagColor, TransferConflictDecision, rename_entry,
    resolve_go_to_path, suggest_go_to_paths,
};

use super::*;

impl FilesAppState {
    pub(crate) fn toggle_quick_look_selected(&mut self) {
        if self.quick_look.is_some() {
            self.quick_look = None;
            self.dirty = true;
            return;
        }
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = Some(String::from("Select an item for Quick Look"));
            self.dirty = true;
            return;
        };
        self.request_preview_if_needed(&entry, 760, 500);
        self.request_info_if_needed(&entry.path);
        self.quick_look = Some(QuickLookState { entry });
        self.popup = None;
        self.dirty = true;
    }

    /// Cycles active `Quick Look` preview backward (`-1`) or forward (`+1`).
    pub(crate) fn step_quick_look(&mut self, delta: isize) {
        let Some(col) = self.columns.get(self.active_column) else {
            return;
        };
        if col.visible_entries.is_empty() {
            return;
        }
        let len = col.visible_entries.len();
        let next = match col.selected_index {
            None => 0,
            Some(cur) => cur.saturating_add_signed(delta).min(len - 1),
        };
        self.select_entry(self.active_column, next, false);
    }

    /// Opens `Get Info` sheet for currently selected item (`Ctrl+I`) or active folder.
    pub(crate) fn open_get_info_selected(&mut self) {
        let target = self
            .selected_entry()
            .map(|e| e.path.clone())
            .or_else(|| self.active_writable_directory());
        if let Some(path) = target {
            self.open_get_info_for_path(&path);
        }
    }

    /// Opens `Get Info` sheet for `path` and schedules non-blocking metadata/preview loading.
    pub(crate) fn open_get_info_for_path(&mut self, path: &Path) {
        self.popup = None;
        if let Some(entry) = self.selected_entry().cloned()
            && entry.path == path
        {
            self.request_preview_if_needed(&entry, 96, 96);
        }
        let cached = self.cached_info_for(path).cloned();
        self.get_info = Some(GetInfoState {
            loading: cached.is_none(),
            target_path: path.to_path_buf(),
            info: cached,
            error: None,
            more_expanded: true,
        });
        self.pending_info_path = None;
        self.request_info_if_needed(path);
        self.dirty = true;
    }

    /// Opens `Go to Folder...` sheet (`Ctrl+Shift+G`).
    pub(crate) fn open_go_to_folder(&mut self) {
        self.popup = None;
        self.inline_rename = None;
        let current_dir = self.active_writable_directory();
        let seed = current_dir.as_deref().map_or_else(
            || String::from("~/"),
            |p| format_go_to_display(p, &self.locations),
        );
        let suggestions = suggest_go_to_paths(&seed, current_dir.as_deref(), &self.locations, 6);
        self.go_to_folder = Some(GoToFolderState {
            input: seed,
            suggestions,
            selected_suggestion: 0,
            error: None,
        });
        self.reset_caret_blink();
        self.dirty = true;
    }

    /// Confirms `Go to Folder...` input and navigates to folder (or file's parent).
    pub(crate) fn submit_go_to_folder(&mut self) {
        let Some(gt) = &self.go_to_folder else {
            return;
        };
        let raw_input = gt.input.clone();
        let current_dir = self.active_writable_directory();
        let Some(resolved) =
            resolve_go_to_path(&raw_input, current_dir.as_deref(), &self.locations)
        else {
            if let Some(gt_mut) = &mut self.go_to_folder {
                gt_mut.error = Some(String::from("The folder can't be found."));
            }
            self.dirty = true;
            return;
        };
        self.go_to_folder = None;
        if resolved.is_dir() {
            let title = path_display_title(&resolved);
            let sidebar = match_path_to_sidebar(&self.locations, &self.volumes, &resolved);
            self.open_root_target(ListingTarget::Directory(resolved), title, sidebar, true);
        } else if let Some(parent) = resolved.parent() {
            let parent_buf = parent.to_path_buf();
            let title = path_display_title(&parent_buf);
            let sidebar = match_path_to_sidebar(&self.locations, &self.volumes, &parent_buf);
            self.open_root_target(ListingTarget::Directory(parent_buf), title, sidebar, true);
            self.status_message = Some(format!("Navigated near {}", resolved.display()));
        }
        self.dirty = true;
    }

    /// Picks a specific `Go to Folder...` autocomplete row and opens it immediately.
    pub(crate) fn pick_go_to_suggestion(&mut self, idx: usize) {
        let Some(path) = self
            .go_to_folder
            .as_ref()
            .and_then(|gt| gt.suggestions.get(idx))
            .cloned()
        else {
            return;
        };
        if let Some(gt) = &mut self.go_to_folder {
            gt.input = path.display().to_string();
        }
        self.submit_go_to_folder();
    }

    // --- Inline Rename ---

    /// Begins inline rename (`F2`) for currently selected entry.
    pub(crate) fn start_inline_rename_selected(&mut self) {
        let Some(col) = self.columns.get(self.active_column) else {
            return;
        };
        let Some(row_idx) = col.selected_index else {
            return;
        };
        let Some(entry) = col.visible_entries.get(row_idx).cloned() else {
            return;
        };
        self.start_inline_rename_entry(self.active_column, row_idx, &entry);
    }

    /// Begins inline rename for a specific entry.
    pub(crate) fn start_inline_rename_entry(
        &mut self,
        col_idx: usize,
        row_idx: usize,
        entry: &FileEntry,
    ) {
        if self.is_trash_view()
            || matches!(
                entry.kind,
                FileEntryKind::MountPoint | FileEntryKind::Application
            )
        {
            self.status_message = Some(String::from("Cannot rename this item here"));
            self.dirty = true;
            return;
        }
        self.popup = None;
        self.inline_rename = Some(InlineRenameState {
            target_path: entry.path.clone(),
            col_idx,
            row_idx,
            draft: entry.name.clone(),
            error: None,
        });
        self.reset_caret_blink();
        self.dirty = true;
    }

    /// Commits active inline rename via [`rename_entry`], migrating tags & reloading view.
    pub(crate) fn commit_inline_rename(&mut self) {
        let Some(rn) = self.inline_rename.clone() else {
            return;
        };
        match rename_entry(&rn.target_path, &rn.draft) {
            Ok(new_path) => {
                let existing_tags: Vec<FileTagColor> =
                    self.tags_config.tags_for(&rn.target_path).to_vec();
                if !existing_tags.is_empty() {
                    self.tags_config.remove_tags_for(&rn.target_path);
                    for tag in existing_tags {
                        self.tags_config.toggle_tag(&new_path, tag);
                    }
                    self.persist_preferences();
                }
                if let Some(parent) = new_path.parent() {
                    self.reload_directory(parent);
                }
                self.reload_active_column();
                self.inline_rename = None;
                let new_name = new_path
                    .file_name()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                self.status_message = Some(format!("Renamed to {new_name}"));
            }
            Err(err) => {
                if let Some(edit) = &mut self.inline_rename {
                    edit.error = Some(err.to_string());
                }
                self.status_message = Some(format!("Rename failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Delivers interactive conflict resolution to background transfer thread.
    pub(crate) fn resolve_transfer_conflict(&mut self, decision: TransferConflictDecision) {
        if let Some(tr) = &mut self.active_transfer
            && let Some(mut conflict) = tr.conflict.take()
        {
            if decision == TransferConflictDecision::Stop {
                tr.cancel_flag.store(true, Ordering::Relaxed);
            }
            if let Some(reply) = conflict.reply_tx.take() {
                let _ = reply.send(decision);
            }
            self.dirty = true;
        }
    }

    /// Cancels an active background Copy/Move transfer.
    pub(crate) fn cancel_active_transfer(&mut self) {
        if let Some(tr) = &mut self.active_transfer {
            tr.cancel_flag.store(true, Ordering::Relaxed);
            if let Some(mut conflict) = tr.conflict.take()
                && let Some(reply) = conflict.reply_tx.take()
            {
                let _ = reply.send(TransferConflictDecision::Stop);
            }
            self.dirty = true;
        }
    }
}
