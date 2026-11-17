//! Tab management implementation for `FilesAppState` (`RUSTYOS-071`).

use rusty_services::file_manager::ListingTarget;

use super::*;

impl FilesAppState {
    pub(crate) fn snapshot_active_tab(&self) -> FileTabState {
        FileTabState {
            columns: self.columns.clone(),
            active_column: self.active_column,
            column_scroll_x: self.column_scroll_x,
            list_scroll_y: self.list_scroll_y,
            icons_scroll_y: self.icons_scroll_y,
            gallery_strip_scroll_x: self.gallery_strip_scroll_x,
            active_sidebar: self.active_sidebar.clone(),
            back_stack: self.back_stack.clone(),
            forward_stack: self.forward_stack.clone(),
            search_query: self.search_query.clone(),
            window_title: self.window_title.clone(),
            loader_gen: self.loader_gen,
        }
    }

    fn apply_tab_snapshot(&mut self, tab: FileTabState) {
        self.columns = tab.columns;
        self.active_column = tab.active_column;
        self.column_scroll_x = tab.column_scroll_x;
        self.list_scroll_y = tab.list_scroll_y;
        self.icons_scroll_y = tab.icons_scroll_y;
        self.gallery_strip_scroll_x = tab.gallery_strip_scroll_x;
        self.column_scroll_x_anim.set_immediate(tab.column_scroll_x);
        self.list_scroll_anim.set_immediate(tab.list_scroll_y);
        self.icons_scroll_anim.set_immediate(tab.icons_scroll_y);
        self.gallery_strip_scroll_anim
            .set_immediate(tab.gallery_strip_scroll_x);
        self.follow_right_column = true;
        self.active_sidebar = tab.active_sidebar;
        self.back_stack = tab.back_stack;
        self.forward_stack = tab.forward_stack;
        self.search_query = tab.search_query;
        self.window_title = tab.window_title;
        self.loader_gen = tab.loader_gen;
        self.popup = None;
        self.inline_rename = None;
        self.reapply_filters_all_columns();
        self.ensure_stage_b_assets_requested();
    }

    fn sync_current_tab(&mut self) {
        let snapshot = self.snapshot_active_tab();
        if let Some(slot) = self.tabs.get_mut(self.active_tab) {
            *slot = snapshot;
        }
    }

    /// Opens a new Finder tab (cloning active root or opening `target_override`) and switches to it.
    pub(crate) fn new_tab(&mut self, target_override: Option<ListingTarget>) {
        self.sync_current_tab();
        let target = target_override.unwrap_or_else(|| {
            self.active_writable_directory().map_or_else(
                || {
                    self.columns
                        .first()
                        .map_or(ListingTarget::Directory(self.locations.home.clone()), |c| {
                            c.target.clone()
                        })
                },
                ListingTarget::Directory,
            )
        });
        let (title, sidebar) = match &target {
            ListingTarget::Directory(p) => (
                path_display_title(p),
                match_path_to_sidebar(&self.locations, &self.volumes, p),
            ),
            _ => (self.window_title.clone(), self.active_sidebar.clone()),
        };
        let placeholder = self.snapshot_active_tab();
        self.tabs.push(placeholder);
        self.active_tab = self.tabs.len() - 1;
        self.back_stack.clear();
        self.forward_stack.clear();
        self.open_root_target(target, title, sidebar, false);
        self.sync_current_tab();
        self.dirty = true;
    }

    /// Switches to open tab `idx`.
    pub(crate) fn switch_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active_tab {
            return;
        }
        self.sync_current_tab();
        self.active_tab = idx;
        if let Some(tab) = self.tabs.get(idx).cloned() {
            self.apply_tab_snapshot(tab);
        }
        self.dirty = true;
    }

    /// Closes tab `idx` (keeps at least 1 open tab).
    pub(crate) fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return;
        }
        self.sync_current_tab();
        self.tabs.remove(idx);
        if idx < self.active_tab {
            self.active_tab -= 1;
        } else if idx == self.active_tab {
            self.active_tab = self.active_tab.min(self.tabs.len() - 1);
            if let Some(tab) = self.tabs.get(self.active_tab).cloned() {
                self.apply_tab_snapshot(tab);
            }
        }
        self.dirty = true;
    }

    /// Closes the currently active tab if multiple tabs are open.
    pub(crate) fn close_active_tab(&mut self) {
        self.close_tab(self.active_tab);
    }

    /// Cycles active tab forward or backward (`Ctrl+Tab` / `Ctrl+Shift+Tab`).
    pub(crate) fn cycle_next_tab(&mut self, reverse: bool) {
        if self.tabs.len() <= 1 {
            return;
        }
        let next = if reverse {
            if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            }
        } else {
            (self.active_tab + 1) % self.tabs.len()
        };
        self.switch_tab(next);
    }

    /// Toggles persistent visibility of the top Tabs bar.
    pub(crate) fn toggle_tab_bar(&mut self) {
        self.show_tab_bar = !self.show_tab_bar;
        self.popup = None;
        self.dirty = true;
    }

    /// True if top Tabs strip should currently be laid out and drawn.
    #[must_use]
    pub(crate) fn tab_bar_visible(&self) -> bool {
        self.tabs.len() > 1 || self.show_tab_bar
    }

    /// Ordered `(tab_index, title, is_active)` list reflecting live active tab title.
    #[must_use]
    pub(crate) fn tab_titles(&self) -> Vec<(usize, String, bool)> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let active = i == self.active_tab;
                let title = if active {
                    self.window_title.clone()
                } else {
                    tab.window_title.clone()
                };
                (i, title, active)
            })
            .collect()
    }
}
