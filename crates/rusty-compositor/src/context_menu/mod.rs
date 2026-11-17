//! Modular right-click Context Menu subsystem (`RUSTYOS-064`, `RUSTYOS-086`).
//!
//! Provides unified macOS 27 Golden Gate aligned context menus across all surfaces:
//! - Window context menu: tiling, workspaces, display hopping, minimize, close/force quit
//! - Desktop context menu: new folder, wallpapers, display settings, terminal, tiling
//! - Desktop item context menu: open, move to trash
//! - Dock context menus: app windows/options/quit, downloads fan/stack, trash

pub mod actions;
pub mod desktop_menu;
pub mod dock_menu;
pub mod window_menu;

pub use actions::*;
pub use desktop_menu::*;
pub use dock_menu::*;
pub use window_menu::*;

use rusty_wm::{LayoutMode, OutputId, WindowId};
use smithay::utils::{Logical, Point, Serial, Size};

use crate::shell_render::{ContextMenuEntry, ContextMenuState, ContextMenuTarget, DockHit};
use crate::state::RustyCompositorState;

impl RustyCompositorState {
    /// Closes any active right-click context menu (`RUSTYOS-064`).
    pub fn close_context_menu(&mut self) {
        if self.context_menu.take().is_some() {
            self.sync_dock_auto_hide_target();
            self.shell_redraw_needed = true;
        }
    }

    /// Dynamically swaps `Quit` <-> `Force Quit` when `Alt` is pressed or released while open (`RUSTYOS-064`).
    pub fn sync_context_menu_alt_modifier(&mut self, alt_held: bool) {
        if let Some(ref mut ctx) = self.context_menu
            && toggle_quit_force_in_entries(&mut ctx.entries, alt_held)
        {
            self.shell_redraw_needed = true;
        }
    }

    pub(crate) fn output_and_local_anchor(
        &self,
        global_pos: Point<f64, Logical>,
    ) -> (OutputId, Point<i32, Logical>) {
        let target = self
            .output_map
            .output_under(global_pos)
            .and_then(|id| self.output_map.get(id))
            .unwrap_or_else(|| self.output_map.primary());
        let geo = target.logical_geometry();
        #[allow(clippy::cast_possible_truncation)]
        let local = Point::from((
            (global_pos.x - f64::from(geo.loc.x)).round() as i32,
            (global_pos.y - f64::from(geo.loc.y)).round() as i32,
        ));
        (target.id, local)
    }

    pub(crate) fn install_context_menu(
        &mut self,
        output_id: OutputId,
        anchor_local: Point<i32, Logical>,
        target: ContextMenuTarget,
        entries: Vec<ContextMenuEntry>,
    ) {
        self.close_top_bar_menu();
        self.clear_shell_hover_states();
        self.context_menu = Some(ContextMenuState::new(
            output_id,
            anchor_local,
            target,
            entries,
        ));
        self.shell_redraw_needed = true;
    }

    /// Opens the macOS-aligned Desktop right-click context menu at `global_pos` (`RUSTYOS-064`).
    pub fn open_desktop_context_menu(&mut self, global_pos: Point<f64, Logical>) {
        if self.is_session_locked() {
            return;
        }
        let (output_id, anchor_local) = self.output_and_local_anchor(global_pos);
        let tiling_active = self.wm.active_layout_mode() == LayoutMode::MasterStack;
        let entries = build_desktop_context_entries(tiling_active);
        self.install_context_menu(output_id, anchor_local, ContextMenuTarget::Desktop, entries);
    }

    /// Opens the context menu for a specific desktop canvas file/directory item (`RUSTYOS-081`).
    pub fn open_desktop_item_context_menu(
        &mut self,
        point: Point<f64, Logical>,
        item: &crate::shell_render::DesktopCanvasItem,
    ) {
        let Some(output_id) = self.output_map.output_under(point) else {
            return;
        };
        let Some(entry) = self.output_map.get(output_id) else {
            return;
        };
        let anchor_diff = point - entry.location.to_f64();
        #[allow(clippy::cast_possible_truncation)]
        let anchor_local =
            Point::from((anchor_diff.x.round() as i32, anchor_diff.y.round() as i32));
        let entries = build_desktop_item_context_entries(item);
        self.install_context_menu(output_id, anchor_local, ContextMenuTarget::Desktop, entries);
    }

    /// Opens the macOS-aligned Dock slot context menu (`RUSTYOS-064`).
    pub fn open_dock_context_menu(
        &mut self,
        hit: DockHit,
        dock_screen_size: Size<i32, smithay::utils::Physical>,
        global_pos: Point<f64, Logical>,
        alt_held: bool,
    ) {
        if self.is_session_locked() {
            return;
        }
        self.close_dock_stack();
        self.close_updates_popover();
        let tiles = self.build_dock_tiles();
        let bounces = self.dock_bounce_offsets();
        let view = self.dock_view_state(&tiles, &[], &bounces, self.dock_hover_slot);
        let layout = crate::shell_render::compute_dock_layout(dock_screen_size, &view);

        let primary = self.output_map.primary();
        let output_id = primary.id;
        let geo = primary.logical_geometry();
        let anchor_local = layout
            .slots
            .iter()
            .find(|slot| slot.tile.hit == hit)
            .map_or_else(
                || {
                    #[allow(clippy::cast_possible_truncation)]
                    Point::from((
                        (global_pos.x - f64::from(geo.loc.x)).round() as i32,
                        (global_pos.y - f64::from(geo.loc.y)).round() as i32,
                    ))
                },
                |slot| {
                    let r = slot.icon_rect;
                    match self.user_config.dock_position {
                        crate::config::DockPosition::Bottom => Point::from((r.loc.x, r.loc.y - 6)),
                        crate::config::DockPosition::Left => {
                            Point::from((r.loc.x + r.size.w + 8, r.loc.y))
                        }
                        crate::config::DockPosition::Right => Point::from((r.loc.x - 8, r.loc.y)),
                    }
                },
            );

        let entries = match hit {
            DockHit::ToggleOverview => self.build_overview_context_entries(),
            DockHit::PinnedDownloads => self.build_downloads_context_entries(),
            DockHit::PinnedTrash => build_trash_context_entries(),
            DockHit::MinimizedWindow(win_id) => {
                let Some(items) = build_minimized_window_context_entries(
                    win_id,
                    &self.wm,
                    self.is_window_minimized(win_id),
                    alt_held,
                ) else {
                    return;
                };
                items
            }
            _ => {
                let Some(items) = self.build_dock_app_context_entries(hit, &tiles, alt_held) else {
                    return;
                };
                items
            }
        };
        self.install_context_menu(
            output_id,
            anchor_local,
            ContextMenuTarget::DockSlot(hit),
            entries,
        );
        self.sync_dock_auto_hide_target();
    }

    /// Opens the unified system Window context menu for `win_id` (`RUSTYOS-086`).
    pub fn open_window_context_menu(&mut self, global_pos: Point<f64, Logical>, win_id: WindowId) {
        if self.is_session_locked() {
            return;
        }
        if self.active_window_drag.is_some() || self.active_window_resize.is_some() {
            return;
        }
        let alt_held = self.keyboard.modifier_state().alt;
        let Some(entries) =
            build_window_context_entries(win_id, &self.wm, &self.output_map, alt_held)
        else {
            return;
        };
        let (output_id, anchor_local) = self.output_and_local_anchor(global_pos);
        self.install_context_menu(
            output_id,
            anchor_local,
            ContextMenuTarget::Window(win_id),
            entries,
        );
    }

    /// Dispatches a mouse click at `global_pos` when `self.context_menu` is open (`RUSTYOS-064`).
    ///
    /// Returns `Some(true)` if consumed inside/dismissing by LKM, or `Some(false)` if an outside
    /// right-click closed the menu so the caller can reopen a new menu under the pointer.
    pub fn handle_context_menu_pointer_click(
        &mut self,
        button: u32,
        global_pos: Point<f64, Logical>,
        serial: Serial,
    ) -> Option<bool> {
        let hit = {
            let ctx = self.context_menu.as_ref()?;
            let entry = self.output_map.get(ctx.output_id)?;
            let geo = entry.logical_geometry();
            let screen_size = geo.size.to_physical(1);
            let local_x = global_pos.x - f64::from(geo.loc.x);
            let local_y = global_pos.y - f64::from(geo.loc.y);
            crate::shell_render::hit_test_context_menu(
                screen_size,
                ctx,
                entry.is_primary,
                local_x,
                local_y,
            )
        };
        match hit {
            crate::shell_render::ContextMenuHit::Action(action) => {
                self.execute_context_menu_action(action, serial);
                Some(true)
            }
            crate::shell_render::ContextMenuHit::ExpandSubmenu(parent_idx) => {
                if let Some(ref mut ctx) = self.context_menu {
                    ctx.open_submenu_parent = Some(parent_idx);
                    ctx.selected_row = Some(parent_idx);
                    ctx.submenu_selected_row = None;
                    self.shell_redraw_needed = true;
                }
                Some(true)
            }
            crate::shell_render::ContextMenuHit::Inside => Some(true),
            crate::shell_render::ContextMenuHit::Outside => {
                self.close_context_menu();
                // Allow right-click (`273`) outside to fall through and immediately reopen on the new target.
                Some(button != 273)
            }
        }
    }

    /// Updates hovered row/submenu state for an open [`ContextMenuState`] (`RUSTYOS-064`).
    pub fn update_context_menu_hover_at(&mut self, global_pos: Point<f64, Logical>) {
        let Some(ref mut ctx) = self.context_menu else {
            return;
        };
        let Some(entry) = self.output_map.get(ctx.output_id) else {
            self.context_menu = None;
            return;
        };
        let geo = entry.logical_geometry();
        let screen_size = geo.size.to_physical(1);
        let local_x = global_pos.x - f64::from(geo.loc.x);
        let local_y = global_pos.y - f64::from(geo.loc.y);
        if crate::shell_render::update_context_menu_hover(
            screen_size,
            ctx,
            entry.is_primary,
            local_x,
            local_y,
        ) {
            self.shell_redraw_needed = true;
        }
    }

    /// Rebuilds the active context menu if `Keysym` executed an action, or updates hover state.
    pub fn handle_context_menu_key_event(
        &mut self,
        sym: smithay::input::keyboard::Keysym,
        serial: Serial,
    ) {
        let Some(ref mut ctx) = self.context_menu else {
            return;
        };
        match crate::shell_render::handle_context_menu_key(ctx, sym) {
            crate::shell_render::ContextMenuKeyOutcome::None => {}
            crate::shell_render::ContextMenuKeyOutcome::Redraw => {
                self.shell_redraw_needed = true;
            }
            crate::shell_render::ContextMenuKeyOutcome::Close => self.close_context_menu(),
            crate::shell_render::ContextMenuKeyOutcome::Execute(action) => {
                self.execute_context_menu_action(action, serial);
            }
        }
    }

    /// Opens the Dock `Downloads` menu directly in tests without depending on real pointer coordinates.
    #[cfg(test)]
    pub(crate) fn test_refresh_downloads_sort_and_entries(
        &mut self,
        downloads_dir: &std::path::Path,
        mode: crate::config::DownloadsSortMode,
    ) -> Vec<ContextMenuEntry> {
        self.user_config.downloads_sort = mode;
        let files = read_downloads_entries(downloads_dir, mode, MAX_DOWNLOADS_MENU_ITEMS);
        build_downloads_entries_from_list(files, mode)
    }
}
