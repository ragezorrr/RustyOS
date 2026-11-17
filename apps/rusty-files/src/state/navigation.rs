//! Navigation history, sidebar selection, directory loading, and column management.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use rusty_services::file_manager::{
    FileEntry, FileEntryKind, FileSortField, FileSortOrder, FileViewMode, FilesThemeMode,
    ListingTarget, SidebarLocation, TransferConflictDecision, TransferKind, TransferOutcome,
    list_entries_for_target,
};

use super::*;

impl FilesAppState {
    pub(crate) fn open_initial_path(&mut self, path: &Path) {
        let matched = match_path_to_sidebar(&self.locations, &self.volumes, path);
        self.active_sidebar.clone_from(&matched);
        let title = path_display_title(path);
        self.open_root_target(
            ListingTarget::Directory(path.to_path_buf()),
            title,
            matched,
            false,
        );
    }

    /// Navigates to a sidebar destination cleanly.
    pub(crate) fn navigate_sidebar(&mut self, location: SidebarLocation, push_history: bool) {
        let target = self.locations.resolve_target(&location);
        let title = sidebar_label_for(&location, &self.locations, &self.volumes);
        self.active_sidebar = Some(location.clone());
        self.open_root_target(target, title, Some(location), push_history);
    }

    /// Replaces the root view with `target` and schedules non-blocking background loading.
    pub(crate) fn open_root_target(
        &mut self,
        target: ListingTarget,
        title: String,
        sidebar: Option<SidebarLocation>,
        push_history: bool,
    ) {
        if push_history && let Some(first) = self.columns.first() {
            if first.target == target {
                return;
            }
            self.back_stack.push(NavigationHistoryEntry {
                target: first.target.clone(),
                title: first.title.clone(),
                sidebar: self.active_sidebar.clone(),
            });
            self.forward_stack.clear();
        }
        self.popup = None;
        self.inline_rename = None;
        self.loader_gen = self.loader_gen.wrapping_add(1);
        self.search_scope = SearchScope::ThisFolder;
        self.saved_root_raw = None;
        self.pending_home_search = false;
        self.active_sidebar = sidebar;
        self.active_column = 0;
        self.column_scroll_x = 0;
        self.list_scroll_y = 0;
        self.icons_scroll_y = 0;
        self.gallery_strip_scroll_x = 0;
        self.follow_right_column = true;
        self.column_scroll_x_anim.reset();
        self.list_scroll_anim.reset();
        self.icons_scroll_anim.reset();
        self.gallery_strip_scroll_anim.reset();
        self.window_title.clone_from(&title);
        self.columns = vec![ColumnPane::new_loading(target.clone(), title)];
        self.spawn_load_job(0, target);
        self.dirty = true;
    }

    /// True if history Back stack is non-empty.
    #[must_use]
    pub(crate) const fn can_go_back(&self) -> bool {
        !self.back_stack.is_empty()
    }

    /// True if history Forward stack is non-empty.
    #[must_use]
    pub(crate) const fn can_go_forward(&self) -> bool {
        !self.forward_stack.is_empty()
    }

    /// Navigates backward in history (`<`).
    pub(crate) fn go_back(&mut self) {
        let Some(prev) = self.back_stack.pop() else {
            return;
        };
        if let Some(cur) = self.columns.first() {
            self.forward_stack.push(NavigationHistoryEntry {
                target: cur.target.clone(),
                title: cur.title.clone(),
                sidebar: self.active_sidebar.clone(),
            });
        }
        self.open_root_target(prev.target, prev.title, prev.sidebar, false);
    }

    /// Navigates forward in history (`>`).
    pub(crate) fn go_forward(&mut self) {
        let Some(next) = self.forward_stack.pop() else {
            return;
        };
        if let Some(cur) = self.columns.first() {
            self.back_stack.push(NavigationHistoryEntry {
                target: cur.target.clone(),
                title: cur.title.clone(),
                sidebar: self.active_sidebar.clone(),
            });
        }
        self.open_root_target(next.target, next.title, next.sidebar, false);
    }

    /// Polls completed background directory reads, previews, info inspection, and transfers.
    pub(crate) fn poll_background_loads(&mut self) -> bool {
        let mut changed = self.poll_directory_jobs();
        if self.poll_home_search_jobs() {
            changed = true;
        }
        if self.poll_preview_jobs() {
            changed = true;
        }
        if self.poll_info_jobs() {
            changed = true;
        }
        if self.poll_transfer_jobs() {
            changed = true;
        }
        if changed {
            self.dirty = true;
        }
        changed
    }

    fn poll_home_search_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.home_search_rx.try_recv() {
            if msg.generation != self.loader_gen
                || self.search_scope != SearchScope::Home
                || msg.query != self.search_query
            {
                continue;
            }
            self.pending_home_search = false;
            if let Some(col) = self.columns.first_mut() {
                col.raw_entries = msg.entries;
                col.error = None;
            }
            self.reapply_filters_all_columns();
            changed = true;
        }
        if changed {
            self.ensure_stage_b_assets_requested();
        }
        changed
    }

    fn poll_directory_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.result_rx.try_recv() {
            if msg.generation != self.loader_gen {
                continue;
            }
            let Some(col) = self.columns.get_mut(msg.col_idx) else {
                continue;
            };
            if col.target != msg.target {
                continue;
            }
            col.loading = false;
            match msg.entries {
                Ok(entries) => {
                    col.raw_entries = entries;
                    col.error = None;
                    col.refresh_view(
                        self.show_hidden,
                        &self.search_query,
                        self.sort_field,
                        self.sort_order,
                    );
                }
                Err(err) => {
                    col.raw_entries.clear();
                    col.visible_entries.clear();
                    col.error = Some(err);
                }
            }
            changed = true;
        }
        if changed {
            self.ensure_stage_b_assets_requested();
        }
        changed
    }

    fn poll_preview_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(job) = self.preview_rx.try_recv() {
            self.pending_previews.remove(&job.key);
            self.insert_preview_cache(job.key, job.content);
            changed = true;
        }
        changed
    }

    fn poll_info_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(job) = self.info_rx.try_recv() {
            if self.pending_info_path.as_ref() == Some(&job.path) {
                self.pending_info_path = None;
            }
            if let Some(gi) = &mut self.get_info
                && gi.target_path == job.path
            {
                gi.loading = false;
                match &job.info {
                    Ok(info) => {
                        gi.info = Some(info.clone());
                        gi.error = None;
                    }
                    Err(err) => {
                        gi.error = Some(err.clone());
                    }
                }
            }
            self.cached_info = Some((job.path, job.info));
            changed = true;
        }
        changed
    }

    fn poll_transfer_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.transfer_rx.try_recv() {
            changed = true;
            match msg {
                TransferWorkerMsg::Progress(progress) => {
                    if let Some(tr) = &mut self.active_transfer {
                        tr.progress = progress;
                    }
                }
                TransferWorkerMsg::Conflict {
                    source,
                    dest,
                    reply_tx,
                } => {
                    if let Some(tr) = &mut self.active_transfer {
                        tr.conflict = Some(ConflictPrompt {
                            source,
                            dest,
                            reply_tx: Some(reply_tx),
                        });
                    } else {
                        let _ = reply_tx.send(TransferConflictDecision::Stop);
                    }
                }
                TransferWorkerMsg::Finished {
                    kind,
                    dest_dir,
                    result,
                } => {
                    self.active_transfer = None;
                    self.handle_transfer_finished(kind, &dest_dir, result);
                }
            }
        }
        changed
    }

    fn handle_transfer_finished(
        &mut self,
        kind: TransferKind,
        dest_dir: &Path,
        result: Result<TransferOutcome, String>,
    ) {
        match result {
            Ok(outcome) => {
                if kind == TransferKind::Move && !outcome.created_paths.is_empty() {
                    self.clipboard = None;
                }
                let verb = match kind {
                    TransferKind::Copy => "Copied",
                    TransferKind::Move => "Moved",
                };
                if outcome.cancelled {
                    self.status_message = Some(format!(
                        "{verb} stopped ({} completed)",
                        outcome.created_paths.len()
                    ));
                } else {
                    self.status_message = Some(format!(
                        "{verb} {} item{}",
                        outcome.created_paths.len(),
                        if outcome.created_paths.len() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    ));
                }
                self.reload_directory(dest_dir);
                self.reload_active_column();
            }
            Err(err) => {
                self.status_message = Some(format!("Transfer failed: {err}"));
            }
        }
    }

    pub(crate) fn spawn_load_job(&self, col_idx: usize, target: ListingTarget) {
        let tx = self.result_tx.clone();
        let gen_id = self.loader_gen;
        let locations = self.locations.clone();
        let tags = self.tags_config.clone();
        thread::spawn(move || {
            let res =
                list_entries_for_target(&target, &locations, &tags).map_err(|e| e.to_string());
            let _ = tx.send(LoadJobResult {
                generation: gen_id,
                col_idx,
                target,
                entries: res,
            });
        });
    }

    /// Synchronous helper for deterministic tests without thread timing jitter.
    #[cfg(test)]
    pub(crate) fn complete_pending_loads_sync(&mut self) {
        for idx in 0..self.columns.len() {
            if self.columns[idx].loading {
                let target = self.columns[idx].target.clone();
                let res = list_entries_for_target(&target, &self.locations, &self.tags_config);
                let col = &mut self.columns[idx];
                col.loading = false;
                match res {
                    Ok(entries) => {
                        col.raw_entries = entries;
                        col.error = None;
                        col.refresh_view(
                            self.show_hidden,
                            &self.search_query,
                            self.sort_field,
                            self.sort_order,
                        );
                    }
                    Err(e) => {
                        col.error = Some(e.to_string());
                    }
                }
            }
        }
        while self.result_rx.try_recv().is_ok() {}
        if self.pending_home_search
            && self.search_scope == SearchScope::Home
            && !self.search_query.trim().is_empty()
        {
            let found = rusty_services::file_manager::search_directory_recursive(
                &self.locations.home,
                &self.search_query,
                self.show_hidden,
                250,
            );
            if let Some(col) = self.columns.first_mut() {
                col.raw_entries = found;
                col.error = None;
            }
            self.pending_home_search = false;
            self.reapply_filters_all_columns();
        }
        while self.home_search_rx.try_recv().is_ok() {}
        self.ensure_stage_b_assets_requested();
    }

    /// Handles row selection inside `col_idx` (expands Miller child column if directory).
    pub(crate) fn select_entry(&mut self, col_idx: usize, row_idx: usize, double_click: bool) {
        self.popup = None;
        self.inline_rename = None;
        let Some(entry) = self
            .columns
            .get(col_idx)
            .and_then(|c| c.visible_entries.get(row_idx))
            .cloned()
        else {
            return;
        };
        if let Some(col) = self.columns.get_mut(col_idx) {
            col.selected_index = Some(row_idx);
        }
        self.active_column = col_idx;

        if self.view_mode == FileViewMode::Columns {
            self.columns.truncate(col_idx + 1);
            if entry.is_navigable_dir() && !self.is_trash_view() {
                let child_target = ListingTarget::Directory(entry.path.clone());
                let child_col = ColumnPane::new_loading(child_target.clone(), entry.name.clone());
                self.columns.push(child_col);
                self.window_title.clone_from(&self.columns[0].title);
                self.spawn_load_job(col_idx + 1, child_target);
            } else if double_click {
                self.launch_entry(&entry);
            }
        } else if double_click {
            if entry.is_navigable_dir() && !self.is_trash_view() {
                self.open_root_target(
                    ListingTarget::Directory(entry.path.clone()),
                    entry.name.clone(),
                    None,
                    true,
                );
            } else {
                self.launch_entry(&entry);
            }
        }

        if let Some(ql) = &mut self.quick_look {
            ql.entry = entry.clone();
        }
        self.ensure_stage_b_assets_for_selection(&entry);
        self.follow_right_column = true;
        self.dirty = true;
    }

    /// Activates the currently selected item (opens folder into next column or launches file).
    pub(crate) fn activate_selected(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if entry.is_navigable_dir() && !self.is_trash_view() {
            if self.view_mode == FileViewMode::Columns {
                if self.columns.get(self.active_column + 1).is_some() {
                    self.active_column += 1;
                    self.follow_right_column = true;
                    if let Some(next_col) = self.columns.get_mut(self.active_column)
                        && next_col.selected_index.is_none()
                        && !next_col.visible_entries.is_empty()
                    {
                        next_col.selected_index = Some(0);
                    }
                    self.dirty = true;
                }
            } else {
                self.open_root_target(ListingTarget::Directory(entry.path), entry.name, None, true);
            }
        } else {
            self.launch_entry(&entry);
        }
    }

    /// Launches a `.desktop` application or opens a regular document with `xdg-open`.
    pub(crate) fn launch_entry(&mut self, entry: &FileEntry) {
        if entry.kind == FileEntryKind::Application
            && let Some(exec_line) = &entry.desktop_exec
        {
            if let Some(parts) = parse_desktop_exec_tokens(exec_line)
                && let Some((program, args)) = parts.split_first()
            {
                let res = Command::new(program)
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                self.status_message = Some(match res {
                    Ok(_) => format!("Opened {}", entry.name),
                    Err(e) => format!("Failed to open {}: {e}", entry.name),
                });
                self.dirty = true;
            }
            return;
        }
        if !entry.is_navigable_dir() {
            let res = Command::new("xdg-open")
                .arg(&entry.path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            self.status_message = Some(match res {
                Ok(_) => format!("Opening {}", entry.name),
                Err(e) => format!("Failed to open {}: {e}", entry.name),
            });
            self.dirty = true;
        }
    }

    /// Changes view mode (`Icons`, `List`, `Columns`, `Gallery`) and persists preference.
    pub(crate) fn set_view_mode(&mut self, mode: FileViewMode) {
        self.popup = None;
        self.inline_rename = None;
        if self.view_mode != mode {
            self.view_mode = mode;
            if mode != FileViewMode::Columns && self.columns.len() > 1 {
                let keep_idx = self.active_column.min(self.columns.len() - 1);
                let chosen = self.columns.swap_remove(keep_idx);
                self.columns = vec![chosen];
                self.active_column = 0;
                self.window_title.clone_from(&self.columns[0].title);
            }
            if mode == FileViewMode::Gallery
                && let Some(col) = self.columns.first_mut()
                && col.selected_index.is_none()
                && !col.visible_entries.is_empty()
            {
                col.selected_index = Some(0);
            }
            self.tags_config.view_mode = mode;
            self.persist_preferences();
            self.status_message = Some(format!("Switched to {} view", mode.label()));
            self.ensure_stage_b_assets_requested();
            self.dirty = true;
        }
    }

    /// Toggles Golden Gate Light <-> Dark theme and saves `file_tags.toml`.
    pub(crate) fn toggle_theme(&mut self) {
        self.theme = self.theme.toggle();
        self.tags_config.theme = self.theme;
        self.persist_preferences();
        self.status_message = Some(match self.theme {
            FilesThemeMode::LightGoldenGate => String::from("Light Golden Gate appearance"),
            FilesThemeMode::DarkPurpleGlass => String::from("Dark Golden Gate appearance"),
        });
        self.dirty = true;
    }

    /// Changes active sort criterion (flipping order if same field is selected again).
    pub(crate) fn set_sort(&mut self, field: FileSortField) {
        if self.sort_field == field {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_field = field;
            self.sort_order = FileSortOrder::Ascending;
        }
        self.tags_config.sort_field = self.sort_field;
        self.tags_config.sort_order = self.sort_order;
        self.persist_preferences();
        self.reapply_filters_all_columns();
        self.popup = None;
        self.dirty = true;
    }

    /// Explicitly sets sort direction (`Ascending` or `Descending`).
    pub(crate) fn set_sort_order(&mut self, order: FileSortOrder) {
        self.sort_order = order;
        self.tags_config.sort_order = order;
        self.persist_preferences();
        self.reapply_filters_all_columns();
        self.popup = None;
        self.dirty = true;
    }

    /// Toggles dot-file visibility and persists preference.
    pub(crate) fn toggle_show_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.tags_config.show_hidden = self.show_hidden;
        self.persist_preferences();
        self.reapply_filters_all_columns();
        self.popup = None;
        self.dirty = true;
    }

    /// Toggles Path Bar breadcrumb visibility and persists preference (`RUSTYOS-071`).
    pub(crate) fn toggle_path_bar(&mut self) {
        self.show_path_bar = !self.show_path_bar;
        self.tags_config.show_path_bar = self.show_path_bar;
        self.persist_preferences();
        self.popup = None;
        self.dirty = true;
    }

    /// Updates search filter query and refreshes visible entries (including Home scope).
    pub(crate) fn reapply_filters_all_columns(&mut self) {
        for col in &mut self.columns {
            col.refresh_view(
                self.show_hidden,
                &self.search_query,
                self.sort_field,
                self.sort_order,
            );
        }
    }

    /// Returns breadcrumb segments `(label, path)` for the path bar and window title.
    pub(crate) fn path_breadcrumbs(&self) -> Vec<(String, PathBuf)> {
        let dir = self
            .columns
            .get(self.active_column)
            .and_then(|c| match &c.target {
                ListingTarget::Directory(p) => Some(p.as_path()),
                _ => None,
            })
            .or_else(|| match self.columns.first().map(|c| &c.target) {
                Some(ListingTarget::Directory(p)) => Some(p.as_path()),
                _ => None,
            });
        let Some(target_dir) = dir else {
            return Vec::new();
        };

        let mut crumbs = Vec::new();
        if let Ok(rel) = target_dir.strip_prefix(&self.locations.home) {
            crumbs.push((
                self.locations.user_label.clone(),
                self.locations.home.clone(),
            ));
            let mut acc = self.locations.home.clone();
            for comp in rel.components() {
                acc.push(comp.as_os_str());
                crumbs.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
            }
            return crumbs;
        }
        let mut acc = PathBuf::new();
        for comp in target_dir.components() {
            acc.push(comp.as_os_str());
            let label = if acc == Path::new("/") {
                String::from("RustyOS HD")
            } else {
                comp.as_os_str().to_string_lossy().into_owned()
            };
            crumbs.push((label, acc.clone()));
        }
        crumbs
    }

    /// Navigates cleanly when clicking a bottom `Path Bar` breadcrumb directory.
    pub(crate) fn navigate_to_directory(&mut self, path: PathBuf) {
        if path.is_dir() {
            let title = path_display_title(&path);
            let sidebar = match_path_to_sidebar(&self.locations, &self.volumes, &path);
            self.open_root_target(ListingTarget::Directory(path), title, sidebar, true);
        }
    }

    /// Formats right-edge status summary string for the bottom bar.
    #[must_use]
    pub(crate) fn status_summary(&self) -> String {
        if let Some(msg) = &self.status_message {
            return msg.clone();
        }
        if let Some(col) = self.columns.get(self.active_column) {
            if col.loading {
                return format!("Loading {}...", col.title);
            }
            if let Some(err) = &col.error {
                return err.clone();
            }
            let n = col.visible_entries.len();
            return if n == 1 {
                String::from("1 item")
            } else {
                format!("{n} items")
            };
        }
        String::new()
    }
}

pub(crate) fn key_matches_ascii(key_name: &str, utf8: Option<&str>, ch: char) -> bool {
    let mut tmp = [0_u8; 4];
    let s = ch.encode_utf8(&mut tmp);
    key_name.eq_ignore_ascii_case(s) || utf8.is_some_and(|u| u.eq_ignore_ascii_case(s))
}

pub(crate) fn is_supported_raster_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("png") || ext.eq_ignore_ascii_case("ppm"))
}

pub(crate) fn format_go_to_display(path: &Path, locations: &UserFileLocations) -> String {
    if let Ok(rel) = path.strip_prefix(&locations.home) {
        if rel.as_os_str().is_empty() {
            return String::from("~/");
        }
        return format!("~/{}", rel.display());
    }
    path.display().to_string()
}
