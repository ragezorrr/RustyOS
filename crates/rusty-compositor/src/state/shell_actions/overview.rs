//! Overview mode and workspace-strip actions.

use rusty_wm::WindowId;

use super::super::RustyCompositorState;

impl RustyCompositorState {
    /// Toggles Workspace Overview / Expose mode (`RUSTYOS-023`).
    pub fn toggle_overview(&mut self) {
        self.overview_active = !self.overview_active;
        if self.overview_active {
            self.close_dock_stack();
            self.close_context_menu();
            self.close_launcher();
            self.close_quick_settings();
            self.close_updates_popover();
            self.close_wifi_popover();
            self.close_notification_center();
            self.active_window_drag = None;
            self.active_window_resize = None;
            self.overview_hovered_card = None;
            self.overview_hovered_close = None;
            self.overview_hovered_space = None;
            self.overview_drag = None;
            let ids = self.overview_window_ids();
            let focused = self.wm.focused_window();
            self.overview_selected_card = if ids.is_empty() {
                None
            } else {
                focused
                    .and_then(|fid| ids.iter().position(|&id| id == fid))
                    .or(Some(0))
            };
        } else {
            self.overview_filter_app = None;
            self.overview_hovered_card = None;
            self.overview_hovered_close = None;
            self.overview_hovered_space = None;
            self.overview_selected_card = None;
            self.overview_drag = None;
        }
        self.retarget_overview_anim();
    }

    /// Exits Workspace Overview mode (`RUSTYOS-023`).
    pub fn close_overview(&mut self) {
        self.overview_active = false;
        self.overview_filter_app = None;
        self.overview_hovered_card = None;
        self.overview_hovered_close = None;
        self.overview_hovered_space = None;
        self.overview_selected_card = None;
        self.overview_drag = None;
        self.retarget_overview_anim();
    }

    /// Moves the keyboard-selected Overview card by `(dx, dy)` in the Overview grid (`RUSTYOS-167`).
    pub fn overview_move_selection(&mut self, dx: isize, dy: isize) {
        let count = self.overview_window_ids().len();
        if count == 0 {
            self.overview_selected_card = None;
            return;
        }
        let cols = isize::try_from(crate::shell_render::overview_grid_cols(count)).unwrap_or(1);
        let cur =
            isize::try_from(self.overview_selected_card.unwrap_or(0).min(count - 1)).unwrap_or(0);
        let count_i = isize::try_from(count).unwrap_or(1);
        let next = if dy != 0 {
            let candidate = cur + dy * cols;
            if (0..count_i).contains(&candidate) {
                candidate
            } else {
                cur.rem_euclid(count_i)
            }
        } else {
            (cur + dx).rem_euclid(count_i)
        };
        self.overview_selected_card = Some(usize::try_from(next).unwrap_or(0));
        self.shell_redraw_needed = true;
    }

    /// Activates the keyboard-selected window card in Overview and exits Overview (`RUSTYOS-167`).
    pub fn overview_activate_selected(&mut self, serial: smithay::utils::Serial) {
        let ids = self.overview_window_ids();
        let idx = self.overview_selected_card.unwrap_or(0);
        if let Some(&win_id) = ids.get(idx) {
            self.restore_window(win_id, serial);
            self.close_overview();
        } else {
            self.close_overview();
        }
    }

    /// Requests closing the keyboard-selected window card in Overview (`RUSTYOS-167`).
    pub fn overview_close_selected(&mut self) {
        let ids = self.overview_window_ids();
        let Some(idx) = self.overview_selected_card else {
            return;
        };
        if let Some(&win_id) = ids.get(idx) {
            self.request_close_window(win_id);
            let remaining = ids.len().saturating_sub(1);
            self.overview_selected_card = (remaining > 0).then_some(idx.min(remaining - 1));
            self.shell_redraw_needed = true;
        }
    }

    /// Toggles Workspace Overview filtered to windows of a single Dock app (App Exposé, `RUSTYOS-073`).
    pub fn toggle_overview_for_app(&mut self, app_key: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        if self.overview_active && self.overview_filter_app.as_deref() == Some(key.as_str()) {
            self.close_overview();
            return;
        }
        self.overview_filter_app = Some(key);
        if self.overview_active {
            self.shell_redraw_needed = true;
        } else {
            self.toggle_overview();
        }
    }

    /// Ordered window IDs shown in Workspace Overview (filtered when App Exposé is active).
    #[must_use]
    pub fn overview_window_ids(&self) -> Vec<WindowId> {
        let ids = self.ordered_visible_window_ids();
        let Some(ref filter) = self.overview_filter_app else {
            return ids;
        };
        ids.into_iter()
            .filter(|&id| {
                let title = self.window_title(id);
                let app_id = self.window_app_id(id);
                crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, id)
                    == *filter
            })
            .collect()
    }
}
