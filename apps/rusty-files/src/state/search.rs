//! Search queries, tagging, asset preview caching, and path sharing.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use rusty_services::file_manager::{
    FileDetailedInfo, FileEntry, FileEntryKind, FileTagColor, FileViewMode, ListingTarget,
    inspect_entry_info, load_image_preview_bgra, read_text_preview, search_directory_recursive,
};

use super::*;

impl FilesAppState {
    pub(crate) fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.search_field.buffer.set_text(&self.search_query);
        self.refresh_search_results();
        self.reset_caret_blink();
        self.dirty = true;
    }

    /// Switches active search scope between active folder and recursive Home search (`RUSTYOS-071`).
    pub(crate) fn set_search_scope(&mut self, scope: SearchScope) {
        if self.search_scope != scope {
            self.search_scope = scope;
            self.refresh_search_results();
            self.dirty = true;
        }
    }

    /// Returns human label for the current-folder chip in the Search Scope bar.
    #[must_use]
    pub(crate) fn search_this_folder_label(&self) -> String {
        self.columns
            .first()
            .map_or_else(|| String::from("This Folder"), |c| c.title.clone())
    }

    pub(crate) fn refresh_search_results(&mut self) {
        if self.search_scope == SearchScope::Home && !self.search_query.trim().is_empty() {
            if self.saved_root_raw.is_none()
                && let Some(col) = self.columns.first()
            {
                self.saved_root_raw = Some((col.target.clone(), col.raw_entries.clone()));
            }
            self.columns.truncate(1);
            self.active_column = 0;
            self.spawn_home_search_job();
            return;
        }
        self.pending_home_search = false;
        if let Some((saved_target, saved_entries)) = self.saved_root_raw.take()
            && let Some(col) = self.columns.first_mut()
            && col.target == saved_target
        {
            col.raw_entries = saved_entries;
        }
        self.reapply_filters_all_columns();
    }

    fn spawn_home_search_job(&mut self) {
        let tx = self.home_search_tx.clone();
        let gen_id = self.loader_gen;
        let root = self.locations.home.clone();
        let query = self.search_query.clone();
        let show_hidden = self.show_hidden;
        self.pending_home_search = true;
        thread::spawn(move || {
            let entries = search_directory_recursive(&root, &query, show_hidden, 250);
            let _ = tx.send(HomeSearchResult {
                generation: gen_id,
                query,
                entries,
            });
        });
    }

    /// Resolves selected path or active directory for Share actions (`RUSTYOS-071`).
    #[must_use]
    pub(crate) fn share_target_path(&self) -> Option<PathBuf> {
        self.selected_entry()
            .map(|e| e.path.clone())
            .or_else(|| self.active_writable_directory())
    }

    /// Copies text to Wayland clipboard via non-blocking `wl-copy` (if available) and updates status.
    pub(crate) fn share_copy_text(&mut self, text: String, notice_prefix: &str) {
        self.popup = None;
        self.status_message = Some(format!("{notice_prefix}: {text}"));
        self.dirty = true;
        thread::spawn(move || {
            use std::io::Write;
            if let Ok(mut child) = Command::new("wl-copy")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                && let Some(mut stdin) = child.stdin.take()
            {
                let _ = stdin.write_all(text.as_bytes());
                drop(stdin);
                let _ = child.wait();
            }
        });
    }

    pub(crate) fn toggle_tag_on_path(&mut self, path: &Path, tag: FileTagColor) {
        let added = self.tags_config.toggle_tag(path, tag);
        self.persist_preferences();
        let verb = if added { "Tagged" } else { "Removed tag" };
        self.status_message = Some(format!("{verb} {}", tag.label()));
        if matches!(
            self.columns.first().map(|c| &c.target),
            Some(ListingTarget::Tag(_))
        ) {
            self.reload_active_column();
        }
        self.dirty = true;
    }

    pub(crate) fn reload_directory(&mut self, dir: &Path) {
        let target = ListingTarget::Directory(dir.to_path_buf());
        let indices: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (c.target == target).then_some(i))
            .collect();
        for idx in indices {
            self.columns[idx].loading = true;
            self.spawn_load_job(idx, target.clone());
        }
    }

    pub(crate) fn reload_active_column(&mut self) {
        if let Some(col) = self.columns.get_mut(self.active_column) {
            let target = col.target.clone();
            col.loading = true;
            self.spawn_load_job(self.active_column, target);
        }
    }

    pub(crate) fn persist_preferences(&self) {
        let _ = self
            .tags_config
            .save_to_path(&self.locations.tags_config_path());
    }

    /// Currently focused entry across active column/list.
    #[must_use]
    pub(crate) fn selected_entry(&self) -> Option<&FileEntry> {
        self.columns
            .get(self.active_column)
            .and_then(ColumnPane::selected_entry)
    }

    /// True if root location is `ListingTarget::Trash`.
    #[must_use]
    pub(crate) fn is_trash_view(&self) -> bool {
        matches!(
            self.columns.first().map(|c| &c.target),
            Some(ListingTarget::Trash)
        )
    }

    /// Directory where `New Folder` can be created (active column target or parent directory).
    #[must_use]
    pub(crate) fn active_writable_directory(&self) -> Option<PathBuf> {
        if let Some(col) = self.columns.get(self.active_column)
            && let ListingTarget::Directory(path) = &col.target
        {
            return Some(path.clone());
        }
        self.columns.iter().rev().find_map(|c| match &c.target {
            ListingTarget::Directory(p) => Some(p.clone()),
            _ => None,
        })
    }

    /// Returns assigned tag colors for `path`.
    #[must_use]
    pub(crate) fn tags_for(&self, path: &Path) -> &[FileTagColor] {
        self.tags_config.tags_for(path)
    }

    /// Returns cached preview for `entry` if available in the LRU cache.
    #[must_use]
    pub(crate) fn cached_preview_for(
        &self,
        entry: &FileEntry,
        max_w: usize,
        max_h: usize,
    ) -> Option<&CachedPreview> {
        let key = (entry.path.clone(), entry.modified_unix_secs, max_w, max_h);
        self.preview_cache
            .iter()
            .rev()
            .find(|item| item.key == key)
            .map(|item| &item.content)
    }

    /// Returns cached [`FileDetailedInfo`] for `path` if inspected.
    #[must_use]
    pub(crate) fn cached_info_for(&self, path: &Path) -> Option<&FileDetailedInfo> {
        self.cached_info
            .as_ref()
            .filter(|(p, _)| p == path)
            .and_then(|(_, res)| res.as_ref().ok())
    }

    pub(crate) fn insert_preview_cache(&mut self, key: PreviewCacheKey, content: CachedPreview) {
        if let Some(pos) = self.preview_cache.iter().position(|it| it.key == key) {
            self.preview_cache.remove(pos);
        } else if self.preview_cache.len() >= MAX_PREVIEW_CACHE_ITEMS {
            self.preview_cache.remove(0);
        }
        self.preview_cache.push(PreviewCacheEntry { key, content });
    }

    pub(crate) fn request_preview_if_needed(
        &mut self,
        entry: &FileEntry,
        max_w: usize,
        max_h: usize,
    ) {
        if entry.kind != FileEntryKind::RegularFile && entry.kind != FileEntryKind::Symlink {
            return;
        }
        let key = (entry.path.clone(), entry.modified_unix_secs, max_w, max_h);
        if self.preview_cache.iter().any(|it| it.key == key) || self.pending_previews.contains(&key)
        {
            return;
        }
        let is_raster = is_supported_raster_name(&entry.name);
        self.pending_previews.insert(key.clone());
        let tx = self.preview_tx.clone();
        let path = entry.path.clone();
        thread::spawn(move || {
            let content = if is_raster {
                load_image_preview_bgra(&path, max_w, max_h)
                    .map_or(CachedPreview::Unavailable, CachedPreview::Image)
            } else {
                read_text_preview(&path, 24 * 1024, 48)
                    .map_or(CachedPreview::Unavailable, CachedPreview::Text)
            };
            let _ = tx.send(PreviewJobResult { key, content });
        });
    }

    pub(crate) fn request_info_if_needed(&mut self, path: &Path) {
        if self.cached_info.as_ref().is_some_and(|(p, _)| p == path)
            || self.pending_info_path.as_deref() == Some(path)
        {
            return;
        }
        self.pending_info_path = Some(path.to_path_buf());
        let tx = self.info_tx.clone();
        let target = path.to_path_buf();
        let locs = self.locations.clone();
        thread::spawn(move || {
            let info = inspect_entry_info(&target, &locs).map_err(|e| e.to_string());
            let _ = tx.send(InfoJobResult { path: target, info });
        });
    }

    pub(crate) fn ensure_stage_b_assets_for_selection(&mut self, entry: &FileEntry) {
        if self.view_mode == FileViewMode::Gallery {
            self.request_preview_if_needed(entry, 760, 480);
            self.request_info_if_needed(&entry.path);
        }
        if self.quick_look.is_some() {
            self.request_preview_if_needed(entry, 760, 500);
            self.request_info_if_needed(&entry.path);
        }
    }

    pub(crate) fn ensure_stage_b_assets_requested(&mut self) {
        if self.view_mode == FileViewMode::Icons || self.view_mode == FileViewMode::Gallery {
            let entries: Vec<FileEntry> = self
                .columns
                .first()
                .map(|c| c.visible_entries.iter().take(48).cloned().collect())
                .unwrap_or_default();
            for entry in &entries {
                if is_supported_raster_name(&entry.name) {
                    self.request_preview_if_needed(entry, 96, 96);
                }
            }
        }
        if let Some(entry) = self.selected_entry().cloned() {
            self.ensure_stage_b_assets_for_selection(&entry);
        }
    }
}
