//! Application state, navigation history, background directory loading, and actions
//! for `rusty-files` (`RUSTYOS-065`, ADR-0030).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use rusty_services::file_manager::{
    FileEntry, FileEntryKind, FileSortField, FileSortOrder, FileTagColor, FileTagsConfig,
    FileViewMode, FilesThemeMode, ListingTarget, SidebarLocation, UserFileLocations, VolumeInfo,
    create_unique_folder, discover_mounts, empty_trash, filter_and_sort_entries,
    list_entries_for_target, move_to_trash, restore_from_trash,
};

/// Fixed column width in Miller Columns mode.
pub(crate) const MILLER_COLUMN_WIDTH: i32 = 216;
/// Row height for Miller column rows.
pub(crate) const ROW_HEIGHT: i32 = 26;
/// Row height for Golden Gate sidebar rows.
pub(crate) const SIDEBAR_ROW_HEIGHT: i32 = 23;
/// Section header height in the Golden Gate sidebar.
pub(crate) const SIDEBAR_SECTION_HEIGHT: i32 = 18;
/// List view row height.
pub(crate) const LIST_ROW_HEIGHT: i32 = 28;

/// Icon classification for Golden Gate sidebar entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarIconKind {
    Recents,
    Shared,
    Applications,
    Desktop,
    Documents,
    Downloads,
    Home,
    Volume,
    Network,
    Trash,
    Tag(FileTagColor),
}

/// One row rendered inside the floating translucent sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidebarRowItem {
    /// Optional uppercase-style section caption displayed immediately above this row.
    pub section: Option<&'static str>,
    /// Display label.
    pub label: String,
    /// Target navigation location.
    pub location: SidebarLocation,
    /// Icon to draw left of the label.
    pub icon: SidebarIconKind,
}

/// Single Miller column (or root listing in List view).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnPane {
    /// Backing target directory or virtual folder.
    pub target: ListingTarget,
    /// Header/breadcrumb title.
    pub title: String,
    /// Unfiltered raw entries returned by background loader.
    pub raw_entries: Vec<FileEntry>,
    /// Filtered & sorted entries currently visible.
    pub visible_entries: Vec<FileEntry>,
    /// Currently selected row index in `visible_entries`.
    pub selected_index: Option<usize>,
    /// Vertical scroll offset in pixels (`>= 0`).
    pub scroll_y: i32,
    /// True while background thread is still loading this column.
    pub loading: bool,
    /// Last load error if directory failed to read.
    pub error: Option<String>,
}

impl ColumnPane {
    fn new_loading(target: ListingTarget, title: String) -> Self {
        Self {
            target,
            title,
            raw_entries: Vec::new(),
            visible_entries: Vec::new(),
            selected_index: None,
            scroll_y: 0,
            loading: true,
            error: None,
        }
    }

    /// Returns currently selected entry if valid.
    #[must_use]
    pub(crate) fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_index
            .and_then(|idx| self.visible_entries.get(idx))
    }

    /// Re-applies active search filter and sort settings, preserving selection by path.
    pub(crate) fn refresh_view(
        &mut self,
        show_hidden: bool,
        query: &str,
        sort_field: FileSortField,
        sort_order: FileSortOrder,
    ) {
        let selected_path = self.selected_entry().map(|entry| entry.path.clone());
        self.visible_entries = filter_and_sort_entries(
            self.raw_entries.clone(),
            show_hidden,
            query,
            sort_field,
            sort_order,
        );
        self.selected_index = selected_path
            .as_ref()
            .and_then(|p| self.visible_entries.iter().position(|e| &e.path == p));
    }
}

/// Recorded history snapshot for `<` Back and `>` Forward pills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavigationHistoryEntry {
    pub target: ListingTarget,
    pub title: String,
    pub sidebar: Option<SidebarLocation>,
}

/// Active floating popover menu inside `rusty-files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenPopupMenu {
    /// Sort criterion & order menu anchored below Sort pill.
    SortMenu,
    /// Actions menu anchored below `...` More pill.
    ActionsMenu,
    /// Quick tag color assignment menu anchored below Tag pill.
    TagPicker { target_path: PathBuf },
    /// Right-click context menu anchored at cursor `(x, y)`.
    ContextMenu {
        x: i32,
        y: i32,
        target_entry: Option<FileEntry>,
        target_dir: Option<PathBuf>,
    },
}

/// Completed background listing payload delivered over `mpsc`.
#[derive(Debug)]
pub(crate) struct LoadJobResult {
    pub generation: u64,
    pub col_idx: usize,
    pub target: ListingTarget,
    pub entries: Result<Vec<FileEntry>, String>,
}

/// Startup options parsed from CLI flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StartupOptions {
    pub initial_path: Option<PathBuf>,
    pub open_trash: bool,
    pub open_recents: bool,
    pub theme_override: Option<FilesThemeMode>,
}

/// Window maximize state requested by compositor or traffic-light toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WindowSizeState {
    #[default]
    Normal,
    Maximized,
}

/// Application loop run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AppRunState {
    #[default]
    Running,
    Exiting,
}

/// Complete application model for `rusty-files`.
#[derive(Debug)]
pub(crate) struct FilesAppState {
    pub locations: UserFileLocations,
    pub tags_config: FileTagsConfig,
    pub volumes: Vec<VolumeInfo>,
    pub sidebar_items: Vec<SidebarRowItem>,
    pub active_sidebar: Option<SidebarLocation>,
    pub view_mode: FileViewMode,
    pub theme: FilesThemeMode,
    pub sort_field: FileSortField,
    pub sort_order: FileSortOrder,
    pub show_hidden: bool,
    pub search_active: bool,
    pub search_query: String,
    pub columns: Vec<ColumnPane>,
    pub active_column: usize,
    pub column_scroll_x: i32,
    pub list_scroll_y: i32,
    pub sidebar_scroll_y: i32,
    pub back_stack: Vec<NavigationHistoryEntry>,
    pub forward_stack: Vec<NavigationHistoryEntry>,
    pub popup: Option<OpenPopupMenu>,
    pub status_message: Option<String>,
    pub window_title: String,
    pub window_size: WindowSizeState,
    pub dirty: bool,
    pub run_state: AppRunState,
    pub loader_gen: u64,
    result_tx: mpsc::Sender<LoadJobResult>,
    result_rx: mpsc::Receiver<LoadJobResult>,
}

impl FilesAppState {
    /// Creates application state using XDG locations, persisted `file_tags.toml`, and CLI options.
    #[must_use]
    pub(crate) fn new(locations: UserFileLocations, opts: &StartupOptions) -> Self {
        let tags_config = FileTagsConfig::load_from_path(&locations.tags_config_path());
        let volumes = discover_mounts();
        let (result_tx, result_rx) = mpsc::channel();
        let sidebar_items = build_sidebar_items(&locations, &volumes);

        let theme = opts.theme_override.unwrap_or(tags_config.theme);
        let view_mode = if tags_config.view_mode.is_stage_a_supported() {
            tags_config.view_mode
        } else {
            FileViewMode::Columns
        };

        let mut app = Self {
            sort_field: tags_config.sort_field,
            sort_order: tags_config.sort_order,
            show_hidden: tags_config.show_hidden,
            locations,
            tags_config,
            volumes,
            sidebar_items,
            active_sidebar: None,
            view_mode,
            theme,
            search_active: false,
            search_query: String::new(),
            columns: Vec::new(),
            active_column: 0,
            column_scroll_x: 0,
            list_scroll_y: 0,
            sidebar_scroll_y: 0,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            popup: None,
            status_message: None,
            window_title: String::from("Files"),
            window_size: WindowSizeState::Normal,
            dirty: true,
            run_state: AppRunState::Running,
            loader_gen: 1,
            result_tx,
            result_rx,
        };

        if opts.open_trash {
            app.navigate_sidebar(SidebarLocation::Trash, false);
        } else if opts.open_recents {
            app.navigate_sidebar(SidebarLocation::Recents, false);
        } else if let Some(path) = &opts.initial_path {
            app.open_initial_path(path);
        } else {
            app.navigate_sidebar(SidebarLocation::Home, false);
        }
        app
    }

    /// True while event loop should stay alive.
    #[must_use]
    pub(crate) const fn is_running(&self) -> bool {
        matches!(self.run_state, AppRunState::Running)
    }

    /// Marks the window to exit cleanly on the next event loop tick.
    pub(crate) fn request_quit(&mut self) {
        self.run_state = AppRunState::Exiting;
    }

    /// True if compositor or traffic-light toggle has the window maximized.
    #[must_use]
    pub(crate) const fn is_maximized(&self) -> bool {
        matches!(self.window_size, WindowSizeState::Maximized)
    }

    /// Updates maximized state flag.
    pub(crate) fn set_maximized(&mut self, maximized: bool) {
        self.window_size = if maximized {
            WindowSizeState::Maximized
        } else {
            WindowSizeState::Normal
        };
    }

    fn open_initial_path(&mut self, path: &Path) {
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
        self.loader_gen = self.loader_gen.wrapping_add(1);
        self.active_sidebar = sidebar;
        self.active_column = 0;
        self.column_scroll_x = 0;
        self.list_scroll_y = 0;
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

    /// Polls completed background directory reads without blocking. Returns `true` if updated.
    pub(crate) fn poll_background_loads(&mut self) -> bool {
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
            self.dirty = true;
        }
        changed
    }

    fn spawn_load_job(&self, col_idx: usize, target: ListingTarget) {
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
    }

    /// Handles row selection inside `col_idx` (expands Miller child column if directory).
    pub(crate) fn select_entry(&mut self, col_idx: usize, row_idx: usize, double_click: bool) {
        self.popup = None;
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
                let child_col = ColumnPane::new_loading(child_target.clone(), entry.name);
                self.columns.push(child_col);
                self.window_title.clone_from(&self.columns[0].title);
                self.spawn_load_job(col_idx + 1, child_target);
            } else if double_click {
                self.launch_entry(&entry);
            }
        } else if double_click {
            if entry.is_navigable_dir() && !self.is_trash_view() {
                self.open_root_target(ListingTarget::Directory(entry.path), entry.name, None, true);
            } else {
                self.launch_entry(&entry);
            }
        }
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

    /// Changes view mode (`Columns`/`List` active in Stage A; `Icons`/`Gallery` guarded for Stage B).
    pub(crate) fn set_view_mode(&mut self, mode: FileViewMode) {
        self.popup = None;
        if !mode.is_stage_a_supported() {
            self.status_message = Some(format!(
                "{} view arrives in Stage B (RUSTYOS-066)",
                mode.label()
            ));
            self.dirty = true;
            return;
        }
        if self.view_mode != mode {
            self.view_mode = mode;
            if mode == FileViewMode::List && self.columns.len() > 1 {
                // Keep the focused deepest folder or keep active column at root in List mode.
                let keep_idx = self.active_column.min(self.columns.len() - 1);
                let chosen = self.columns.swap_remove(keep_idx);
                self.columns = vec![chosen];
                self.active_column = 0;
                self.window_title.clone_from(&self.columns[0].title);
            }
            self.tags_config.view_mode = mode;
            self.persist_preferences();
            self.status_message = Some(format!("Switched to {} view", mode.label()));
            self.dirty = true;
        }
    }

    /// Toggles Light Golden Gate <-> Dark Purple Glass theme and saves `file_tags.toml`.
    pub(crate) fn toggle_theme(&mut self) {
        self.theme = self.theme.toggle();
        self.tags_config.theme = self.theme;
        self.persist_preferences();
        self.status_message = Some(match self.theme {
            FilesThemeMode::LightGoldenGate => String::from("Light Golden Gate appearance"),
            FilesThemeMode::DarkPurpleGlass => String::from("Dark Purple Glass appearance"),
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

    /// Updates search filter query and refreshes visible column entries.
    pub(crate) fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.reapply_filters_all_columns();
        self.dirty = true;
    }

    fn reapply_filters_all_columns(&mut self) {
        for col in &mut self.columns {
            col.refresh_view(
                self.show_hidden,
                &self.search_query,
                self.sort_field,
                self.sort_order,
            );
        }
    }

    /// Creates a unique folder in the active directory and refreshes its listing.
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

    /// Toggles `tag` color on `path` and persists `file_tags.toml`.
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

    fn reload_directory(&mut self, dir: &Path) {
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

    fn reload_active_column(&mut self) {
        if let Some(col) = self.columns.get_mut(self.active_column) {
            let target = col.target.clone();
            col.loading = true;
            self.spawn_load_job(self.active_column, target);
        }
    }

    fn persist_preferences(&self) {
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

    /// Handles keyboard navigation and search text input.
    pub(crate) fn handle_key_input(&mut self, key_name: &str, utf8: Option<&str>, ctrl: bool) {
        if ctrl && key_name.eq_ignore_ascii_case("f") {
            self.search_active = true;
            self.popup = None;
            self.dirty = true;
            return;
        }
        if ctrl && key_name.eq_ignore_ascii_case("n") {
            self.action_new_folder();
            return;
        }
        if key_name == "Escape" {
            if self.popup.is_some() {
                self.popup = None;
            } else if self.search_active {
                if self.search_query.is_empty() {
                    self.search_active = false;
                } else {
                    self.set_search_query(String::new());
                }
            }
            self.dirty = true;
            return;
        }
        if self.search_active {
            if key_name == "BackSpace" {
                self.search_query.pop();
                self.reapply_filters_all_columns();
                self.dirty = true;
                return;
            }
            if let Some(text) = utf8
                && !ctrl
                && text.chars().all(|c| !c.is_control())
                && !text.is_empty()
            {
                self.search_query.push_str(text);
                self.reapply_filters_all_columns();
                self.dirty = true;
                return;
            }
        }
        self.handle_navigation_key(key_name);
    }

    fn handle_navigation_key(&mut self, key_name: &str) {
        match key_name {
            "Down" => self.move_selection_delta(1),
            "Up" => self.move_selection_delta(-1),
            "Right" | "Return" | "KP_Enter" => self.activate_selected(),
            "Left" => {
                if self.view_mode == FileViewMode::Columns && self.active_column > 0 {
                    self.active_column -= 1;
                    self.columns.truncate(self.active_column + 1);
                    self.dirty = true;
                }
            }
            "BackSpace" => self.go_back(),
            "Delete" => self.action_move_selected_to_trash(),
            _ => {}
        }
    }

    fn move_selection_delta(&mut self, delta: isize) {
        let Some(col_idx) = Some(self.active_column).filter(|&i| i < self.columns.len()) else {
            return;
        };
        let len = self.columns[col_idx].visible_entries.len();
        if len == 0 {
            return;
        }
        let next_idx = match self.columns[col_idx].selected_index {
            None => 0,
            Some(cur) => {
                let raw = cur.saturating_add_signed(delta);
                raw.min(len - 1)
            }
        };
        self.select_entry(col_idx, next_idx, false);
    }

    /// Scrolls the specified column (or list view / sidebar) vertically, clamped to valid range.
    pub(crate) fn scroll_vertical_at(
        &mut self,
        area: ScrollTargetArea,
        delta_px: i32,
        viewport_h: i32,
    ) {
        match area {
            ScrollTargetArea::Sidebar => {
                let content_h = sidebar_total_height(&self.sidebar_items);
                let max_scroll = (content_h - viewport_h + 6).max(0);
                self.sidebar_scroll_y = (self.sidebar_scroll_y + delta_px).clamp(0, max_scroll);
            }
            ScrollTargetArea::List => {
                let count = self
                    .columns
                    .first()
                    .map_or(0, |c| i32::try_from(c.visible_entries.len()).unwrap_or(0));
                let content_h = count * LIST_ROW_HEIGHT;
                let max_scroll = (content_h - viewport_h + 8).max(0);
                self.list_scroll_y = (self.list_scroll_y + delta_px).clamp(0, max_scroll);
            }
            ScrollTargetArea::Column(col_idx) => {
                if let Some(col) = self.columns.get_mut(col_idx) {
                    let count = i32::try_from(col.visible_entries.len()).unwrap_or(0);
                    let content_h = count * ROW_HEIGHT;
                    let max_scroll = (content_h - viewport_h + 12).max(0);
                    col.scroll_y = (col.scroll_y + delta_px).clamp(0, max_scroll);
                }
            }
        }
        self.dirty = true;
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

/// Target scrollable region under pointer for wheel events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollTargetArea {
    Sidebar,
    List,
    Column(usize),
}

/// Computes total pixel height consumed by sidebar items + section headers.
#[must_use]
pub(crate) fn sidebar_total_height(items: &[SidebarRowItem]) -> i32 {
    let mut h = 0_i32;
    for item in items {
        if item.section.is_some() {
            h += SIDEBAR_SECTION_HEIGHT;
        }
        h += SIDEBAR_ROW_HEIGHT;
    }
    h
}

fn build_sidebar_items(
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
) -> Vec<SidebarRowItem> {
    let mut items = vec![
        SidebarRowItem {
            section: None,
            label: String::from("Recents"),
            location: SidebarLocation::Recents,
            icon: SidebarIconKind::Recents,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Shared"),
            location: SidebarLocation::Shared,
            icon: SidebarIconKind::Shared,
        },
        SidebarRowItem {
            section: Some("Favorites"),
            label: String::from("Applications"),
            location: SidebarLocation::Applications,
            icon: SidebarIconKind::Applications,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Desktop"),
            location: SidebarLocation::Desktop,
            icon: SidebarIconKind::Desktop,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Documents"),
            location: SidebarLocation::Documents,
            icon: SidebarIconKind::Documents,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Downloads"),
            location: SidebarLocation::Downloads,
            icon: SidebarIconKind::Downloads,
        },
        SidebarRowItem {
            section: Some("Locations"),
            label: locations.user_label.clone(),
            location: SidebarLocation::Home,
            icon: SidebarIconKind::Home,
        },
    ];

    for vol in volumes {
        if !vol.is_network {
            items.push(SidebarRowItem {
                section: None,
                label: vol.name.clone(),
                location: SidebarLocation::Volume(vol.mount_point.clone()),
                icon: SidebarIconKind::Volume,
            });
        }
    }

    items.push(SidebarRowItem {
        section: None,
        label: String::from("Network"),
        location: SidebarLocation::Network,
        icon: SidebarIconKind::Network,
    });
    items.push(SidebarRowItem {
        section: None,
        label: String::from("Trash"),
        location: SidebarLocation::Trash,
        icon: SidebarIconKind::Trash,
    });

    for (idx, tag) in FileTagColor::ALL.iter().enumerate() {
        items.push(SidebarRowItem {
            section: (idx == 0).then_some("Tags"),
            label: String::from(tag.label()),
            location: SidebarLocation::Tag(*tag),
            icon: SidebarIconKind::Tag(*tag),
        });
    }
    items
}

fn match_path_to_sidebar(
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
    path: &Path,
) -> Option<SidebarLocation> {
    if path == locations.home {
        return Some(SidebarLocation::Home);
    }
    if path == locations.desktop {
        return Some(SidebarLocation::Desktop);
    }
    if path == locations.documents {
        return Some(SidebarLocation::Documents);
    }
    if path == locations.downloads {
        return Some(SidebarLocation::Downloads);
    }
    if path == locations.trash_dir || path == locations.trash_dir.join("files") {
        return Some(SidebarLocation::Trash);
    }
    for vol in volumes {
        if path == vol.mount_point {
            return Some(SidebarLocation::Volume(vol.mount_point.clone()));
        }
    }
    None
}

fn sidebar_label_for(
    loc: &SidebarLocation,
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
) -> String {
    match loc {
        SidebarLocation::Recents => String::from("Recents"),
        SidebarLocation::Shared => String::from("Shared"),
        SidebarLocation::Applications => String::from("Applications"),
        SidebarLocation::Desktop => String::from("Desktop"),
        SidebarLocation::Documents => String::from("Documents"),
        SidebarLocation::Downloads => String::from("Downloads"),
        SidebarLocation::Home => locations.user_label.clone(),
        SidebarLocation::Volume(path) => volumes
            .iter()
            .find(|v| &v.mount_point == path)
            .map_or_else(|| path_display_title(path), |v| v.name.clone()),
        SidebarLocation::Network => String::from("Network"),
        SidebarLocation::Trash => String::from("Trash"),
        SidebarLocation::Tag(tag) => format!("{} Tag", tag.label()),
    }
}

fn path_display_title(path: &Path) -> String {
    if path == Path::new("/") {
        return String::from("RustyOS HD");
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .map_or_else(|| path.to_string_lossy().into_owned(), ToOwned::to_owned)
}

fn parse_desktop_exec_tokens(exec_line: &str) -> Option<Vec<String>> {
    let tokens: Vec<String> = exec_line
        .split_whitespace()
        .filter(|tok| !matches!(*tok, "%u" | "%U" | "%f" | "%F" | "%i" | "%c" | "%k"))
        .map(ToOwned::to_owned)
        .collect();
    (!tokens.is_empty()).then_some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestHomeGuard {
        root: PathBuf,
    }

    impl TestHomeGuard {
        fn new(label: &str) -> std::io::Result<Self> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let root = std::env::temp_dir().join(format!(
                "rustyos-files-state-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("home/Documents/SubDir"))?;
            fs::create_dir_all(root.join("home/Downloads"))?;
            fs::create_dir_all(root.join("home/Desktop"))?;
            fs::write(root.join("home/Documents/notes.txt"), b"hello")?;
            fs::write(root.join("home/Documents/SubDir/deep.rs"), b"fn main() {}")?;
            fs::write(root.join("home/Documents/.hidden"), b"secret")?;
            Ok(Self { root })
        }

        fn locations(&self) -> UserFileLocations {
            UserFileLocations::with_roots(
                &self.root.join("home"),
                &self.root.join("config"),
                &self.root.join("data"),
            )
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn miller_columns_history_and_stage_b_guard_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("miller")?;
        let mut state = FilesAppState::new(guard.locations(), &StartupOptions::default());
        state.complete_pending_loads_sync();
        assert_eq!(state.theme, FilesThemeMode::LightGoldenGate);
        assert_eq!(state.view_mode, FileViewMode::Columns);

        state.navigate_sidebar(SidebarLocation::Documents, true);
        state.complete_pending_loads_sync();
        assert!(state.can_go_back());
        assert_eq!(state.columns.len(), 1);
        assert_eq!(state.columns[0].visible_entries.len(), 2);

        // Select SubDir (sorted first since directories lead) -> opens column 1
        state.select_entry(0, 0, false);
        state.complete_pending_loads_sync();
        assert_eq!(state.columns.len(), 2);
        assert_eq!(state.columns[1].visible_entries[0].name, "deep.rs");

        // History Back & Forward
        state.go_back();
        state.complete_pending_loads_sync();
        assert_eq!(state.window_title, "home");
        assert!(state.can_go_forward());

        state.go_forward();
        state.complete_pending_loads_sync();
        assert_eq!(state.window_title, "Documents");

        // Stage B view slots keep active Columns/List intact and display notice
        state.set_view_mode(FileViewMode::Icons);
        assert_eq!(state.view_mode, FileViewMode::Columns);
        assert!(
            state
                .status_message
                .as_deref()
                .is_some_and(|m| m.contains("Stage B"))
        );
        Ok(())
    }

    #[test]
    fn theme_toggle_tags_hidden_and_trash_actions_persist() -> Result<(), Box<dyn std::error::Error>>
    {
        let guard = TestHomeGuard::new("persist")?;
        let locs = guard.locations();
        let mut state = FilesAppState::new(
            locs.clone(),
            &StartupOptions {
                initial_path: Some(locs.documents.clone()),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();

        state.toggle_theme();
        assert_eq!(state.theme, FilesThemeMode::DarkPurpleGlass);
        state.toggle_show_hidden();
        assert!(state.show_hidden);
        state.set_view_mode(FileViewMode::List);
        assert_eq!(state.view_mode, FileViewMode::List);

        let notes_path = locs.documents.join("notes.txt");
        state.toggle_tag_on_path(&notes_path, FileTagColor::Green);
        assert_eq!(state.tags_for(&notes_path), &[FileTagColor::Green]);

        // Reload fresh app state -> persisted preferences & tags are restored
        let reloaded = FilesAppState::new(locs.clone(), &StartupOptions::default());
        assert_eq!(reloaded.theme, FilesThemeMode::DarkPurpleGlass);
        assert_eq!(reloaded.view_mode, FileViewMode::List);
        assert!(reloaded.show_hidden);
        assert_eq!(reloaded.tags_for(&notes_path), &[FileTagColor::Green]);

        // Test New Folder & Move to Trash + Empty Trash
        state.action_new_folder();
        state.complete_pending_loads_sync();
        assert!(locs.documents.join("untitled folder").is_dir());

        let idx = state.columns[0]
            .visible_entries
            .iter()
            .position(|e| e.name == "notes.txt")
            .ok_or("notes.txt missing")?;
        state.select_entry(0, idx, false);
        state.action_move_selected_to_trash();
        state.complete_pending_loads_sync();
        assert!(!notes_path.exists());
        assert_eq!(state.tags_for(&notes_path), []);

        state.navigate_sidebar(SidebarLocation::Trash, false);
        state.complete_pending_loads_sync();
        assert_eq!(state.columns[0].visible_entries.len(), 1);
        state.action_empty_trash();
        state.complete_pending_loads_sync();
        assert_eq!(state.columns[0].visible_entries, []);
        Ok(())
    }
}
