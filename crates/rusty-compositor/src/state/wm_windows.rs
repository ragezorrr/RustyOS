//! Window manager actions, client geometry, switcher, popup tracking, and IPC actions.

use std::sync::Arc;
use std::time::Duration;

use rusty_animation::{AnimatedValue, Easing, LogicalRect};
use rusty_ipc::{
    CompositorEvent, DesktopStateSnapshot, NotificationSummary, ShellRequest, WindowSummary,
};
use rusty_wm::{LayoutMode, Rect, WindowId, WmAction, WorkspaceId};
use smithay::{
    desktop::{
        PopupKind, PopupManager, Window, WindowSurfaceType, find_popup_root_surface,
        get_popup_toplevel_coords, layer_map_for_output,
        utils::{bbox_from_surface_tree, send_frames_surface_tree},
    },
    input::pointer::CursorImageStatus,
    output::Output,
    reexports::{
        wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{IsAlive, Logical, Point, Rectangle, Serial},
    wayland::{
        compositor::{self, TraversalAction},
        input_method::{InputMethodHandler, PopupSurface as ImePopupSurface},
        seat::WaylandFocus,
        shell::{
            wlr_layer::Layer,
            xdg::{PopupSurface, ToplevelSurface, XdgToplevelSurfaceData},
        },
    },
    xwayland::X11Surface,
};
use tracing::{info, warn};

use super::{
    ClientState, RustyCompositorState, WindowSwitcherState, compute_ime_popup_origin,
    wayland_handlers::desktop_protocols::ToplevelIconSurfaceState,
};

impl RustyCompositorState {
    /// Returns `true` if `win_id` is currently minimized to the Dock (`RUSTYOS-032`).
    #[must_use]
    pub fn is_window_minimized(&self, win_id: WindowId) -> bool {
        self.minimized_window_ids.contains(&win_id)
    }

    fn compute_minimize_target_rect(&self, win_id: WindowId) -> Rect {
        let title = self.window_title(win_id);
        let app_id = self.window_app_id(win_id);
        let app_key =
            crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, win_id);
        let tiles = self.build_dock_tiles();
        let layout = self.active_dock_layout(&tiles);
        let target_slot = layout
            .slots
            .iter()
            .find(|slot| {
                !self.user_config.dock_minimize_into_app
                    && slot.tile.hit == crate::shell_render::DockHit::MinimizedWindow(win_id)
            })
            .or_else(|| {
                layout
                    .slots
                    .iter()
                    .find(|slot| slot.tile.app_key == app_key)
            });
        if let Some(slot) = target_slot {
            let r = slot.icon_rect;
            return Rect::new(r.loc.x, r.loc.y, r.size.w.max(16), r.size.h.max(16));
        }
        let c = layout.capsule_rect;
        Rect::new(
            c.loc.x + (c.size.w / 2 - 24).max(0),
            c.loc.y + (c.size.h / 2 - 24).max(0),
            48,
            48,
        )
    }

    /// Minimizes `win_id` to the Dock with Genie/Scale animation, shifting focus cleanly (`RUSTYOS-032`/`073`).
    pub fn minimize_window(&mut self, win_id: WindowId, serial: Serial) {
        let Some(wm_win) = self.wm.window(win_id) else {
            return;
        };
        let window_rect = wm_win.geometry;
        if self
            .active_window_drag
            .is_some_and(|drag| drag.window_id == win_id)
        {
            self.active_window_drag = None;
        }
        if self
            .active_window_resize
            .is_some_and(|resize| resize.window_id == win_id)
        {
            self.active_window_resize = None;
        }
        self.minimized_window_ids.insert(win_id);
        if self.wm.focused_window() == Some(win_id) {
            let next_visible = self
                .wm
                .visible_windows()
                .into_iter()
                .rev()
                .find(|w| w.id != win_id && !self.is_window_minimized(w.id))
                .map(|w| w.id);
            if let Some(next_id) = next_visible {
                self.wm.focus_and_raise(next_id);
            }
        }
        self.sync_space_with_wm(serial);

        self.minimize_anims.retain(|anim| anim.win_id != win_id);
        if !self.animations.reduced_motion() {
            let dock_rect = self.compute_minimize_target_rect(win_id);
            let mut progress = AnimatedValue::new_settled(0.0);
            progress.retarget(
                1.0,
                self.elapsed(),
                Duration::from_millis(280),
                Easing::EaseInOutCubic,
                false,
            );
            self.minimize_anims.push(super::WindowMinimizeAnimState {
                win_id,
                restoring: false,
                progress,
                window_rect,
                dock_rect,
                snapshot_pending: true,
            });
        }
        self.shell_redraw_needed = true;
    }

    /// Restores a minimized window from the Dock with Genie/Scale animation (`RUSTYOS-032`/`073`).
    pub fn restore_window(&mut self, win_id: WindowId, serial: Serial) {
        let Some(wm_win) = self.wm.window(win_id) else {
            return;
        };
        let window_rect = wm_win.geometry;
        let dock_rect = self.compute_minimize_target_rect(win_id);
        self.minimized_window_ids.remove(&win_id);
        self.wm.focus_and_raise(win_id);
        self.sync_space_with_wm(serial);

        self.minimize_anims.retain(|anim| anim.win_id != win_id);
        if !self.animations.reduced_motion() {
            let mut progress = AnimatedValue::new_settled(1.0);
            progress.retarget(
                0.0,
                self.elapsed(),
                Duration::from_millis(250),
                Easing::EaseOutCubic,
                false,
            );
            self.minimize_anims.push(super::WindowMinimizeAnimState {
                win_id,
                restoring: true,
                progress,
                window_rect,
                dock_rect,
                snapshot_pending: false,
            });
        }
        self.shell_redraw_needed = true;
    }

    /// Returns the Wayland `app_id` or `XWayland` `WM_CLASS` identifier for `win_id`, if set (`RUSTYOS-033`).
    #[must_use]
    pub fn window_app_id(&self, win_id: WindowId) -> Option<String> {
        let win = self.smithay_window_for_id(win_id)?;
        if let Some(tl) = win.toplevel() {
            let app_id = compositor::with_states(tl.wl_surface(), |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|d| d.lock().ok())
                    .and_then(|d| d.app_id.clone())
            });
            return app_id.filter(|s| !s.trim().is_empty());
        }
        if let Some(x11) = win.x11_surface() {
            let class = x11.class();
            if !class.trim().is_empty() {
                return Some(class);
            }
            let instance = x11.instance();
            if !instance.trim().is_empty() {
                return Some(instance);
            }
        }
        None
    }

    /// Opens (if closed) or steps the `Alt+Tab` window switcher HUD selection by `delta` (`RUSTYOS-033`).
    pub fn switcher_step(&mut self, delta: isize) {
        if let Some(ref mut switcher) = self.window_switcher {
            if switcher.window_ids.is_empty() {
                self.window_switcher = None;
                return;
            }
            let len = isize::try_from(switcher.window_ids.len()).unwrap_or(1);
            let cur = isize::try_from(switcher.selected).unwrap_or(0);
            let next = (cur + delta).rem_euclid(len);
            switcher.selected = usize::try_from(next).unwrap_or(0);
            return;
        }

        // Order: active workspace top-to-bottom unminimized MRU windows first, then minimized ones.
        let mut ordered = Vec::new();
        let mut minimized = Vec::new();
        for w in self.wm.visible_windows().into_iter().rev() {
            if self.is_window_minimized(w.id) {
                minimized.push(w.id);
            } else {
                ordered.push(w.id);
            }
        }
        ordered.extend(minimized);

        if ordered.is_empty() {
            return;
        }
        self.close_launcher();
        self.close_quick_settings();
        self.close_updates_popover();
        self.close_wifi_popover();
        self.close_notification_center();
        self.close_overview();

        let len = ordered.len();
        let selected = if len == 1 {
            0
        } else if delta >= 0 {
            let step = usize::try_from(delta).unwrap_or(1);
            step.rem_euclid(len)
        } else {
            let len_i = isize::try_from(len).unwrap_or(1);
            usize::try_from(delta.rem_euclid(len_i)).unwrap_or(0)
        };
        self.window_switcher = Some(WindowSwitcherState {
            window_ids: ordered,
            selected,
        });
    }

    /// Confirms the currently highlighted `Alt+Tab` window, restoring if minimized and focusing it (`RUSTYOS-033`).
    pub fn switcher_commit(&mut self, serial: Serial) {
        let Some(switcher) = self.window_switcher.take() else {
            return;
        };
        if let Some(&win_id) = switcher.window_ids.get(switcher.selected) {
            self.restore_window(win_id, serial);
        }
    }

    /// Confirms a clicked `Alt+Tab` card at index `idx`, restoring and focusing it (`RUSTYOS-033`).
    pub fn switcher_commit_index(&mut self, idx: usize, serial: Serial) {
        let Some(switcher) = self.window_switcher.take() else {
            return;
        };
        if let Some(&win_id) = switcher.window_ids.get(idx) {
            self.restore_window(win_id, serial);
        }
    }

    /// Cancels the `Alt+Tab` switcher HUD without changing focus or window state (`RUSTYOS-033`).
    pub fn switcher_cancel(&mut self) {
        self.window_switcher = None;
    }

    /// Removes `win_id` from an active `Alt+Tab` switcher if that window closed while open (`RUSTYOS-033`).
    pub fn remove_from_switcher(&mut self, win_id: WindowId) {
        if let Some(ref mut switcher) = self.window_switcher {
            switcher.window_ids.retain(|id| *id != win_id);
            if switcher.window_ids.is_empty() {
                self.window_switcher = None;
            } else {
                switcher.selected = switcher.selected.min(switcher.window_ids.len() - 1);
            }
        }
    }

    /// Finds the topmost window and surface under the given logical point.
    #[must_use]
    pub fn surface_under_pointer(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(Window, WlSurface, Point<f64, Logical>)> {
        let (window, window_loc) = self.space.element_under(point)?;
        let relative_point = point - window_loc.to_f64();
        let (surface, surface_offset) =
            window.surface_under(relative_point, WindowSurfaceType::ALL)?;
        let surface_origin = (window_loc + surface_offset).to_f64();
        Some((window.clone(), surface, surface_origin))
    }

    /// Finds the topmost surface (layer shell or window) under the given logical point,
    /// respecting Wayland layer-shell stacking order (Overlay -> Top -> Windows -> Bottom -> Background).
    #[must_use]
    pub fn target_surface_under(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(Option<Window>, WlSurface, Point<f64, Logical>)> {
        let layer_map = layer_map_for_output(&self.output);

        // 1. Overlay & Top layers (including overflowing layer popups)
        for layer in layer_map.layers().rev() {
            if matches!(layer.layer(), Layer::Overlay | Layer::Top)
                && let Some(geo) = layer_map.layer_geometry(layer)
                && (geo.to_f64().contains(point)
                    || PopupManager::popups_for_surface(layer.wl_surface())
                        .next()
                        .is_some())
            {
                let rel = point - geo.loc.to_f64();
                if let Some((surf, offset)) = layer.surface_under(rel, WindowSurfaceType::ALL) {
                    let origin = (geo.loc + offset).to_f64();
                    return Some((None, surf, origin));
                }
            }
        }

        // 2. Windows in desktop space (Window::bbox includes popups via bbox_with_popups)
        if let Some((window, surface, origin)) = self.surface_under_pointer(point) {
            return Some((Some(window), surface, origin));
        }

        // 3. Bottom & Background layers (including overflowing layer popups)
        for layer in layer_map.layers().rev() {
            if matches!(layer.layer(), Layer::Bottom | Layer::Background)
                && let Some(geo) = layer_map.layer_geometry(layer)
                && (geo.to_f64().contains(point)
                    || PopupManager::popups_for_surface(layer.wl_surface())
                        .next()
                        .is_some())
            {
                let rel = point - geo.loc.to_f64();
                if let Some((surf, offset)) = layer.surface_under(rel, WindowSurfaceType::ALL) {
                    let origin = (geo.loc + offset).to_f64();
                    return Some((None, surf, origin));
                }
            }
        }

        None
    }

    /// Returns `true` if `surface` belongs to a tracked `xdg_popup` surface tree (`RUSTYOS-034`).
    #[must_use]
    pub fn is_surface_in_popup(&self, surface: &WlSurface) -> bool {
        if self.popups.find_popup(surface).is_some() {
            return true;
        }
        for (_, win) in &self.window_bindings {
            if let Some(root) = win.wl_surface() {
                for (popup, _) in PopupManager::popups_for_surface(&root) {
                    if Self::surface_in_tree(popup.wl_surface(), surface) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Returns `true` if `surface` is inside any currently grabbed popup's surface tree (`RUSTYOS-034`).
    #[must_use]
    pub fn is_surface_in_active_popup_grab(&self, surface: &WlSurface) -> bool {
        self.active_popup_grabs
            .iter()
            .filter(|popup| popup.alive())
            .any(|popup| Self::surface_in_tree(popup.wl_surface(), surface))
    }

    /// Dismisses all active grabbed `xdg_popup` surfaces (`xdg_popup.popup_done`) (`RUSTYOS-034`).
    pub fn dismiss_active_popup_grabs(&mut self) {
        let grabs = std::mem::take(&mut self.active_popup_grabs);
        for popup in grabs.into_iter().rev() {
            if !popup.alive() {
                continue;
            }
            let kind = PopupKind::from(popup);
            if let Ok(root) = find_popup_root_surface(&kind) {
                let _ = PopupManager::dismiss_popup(&root, &kind);
            }
        }
    }

    pub(in crate::state) fn popup_parent_target_rect(
        &self,
        surface: &PopupSurface,
    ) -> Option<Rectangle<i32, Logical>> {
        let output_geo = self.space.output_geometry(&self.output)?;
        let kind = PopupKind::from(surface.clone());
        let root = find_popup_root_surface(&kind).ok()?;

        // Locate the root surface origin in global logical coordinates (window or layer surface).
        let root_origin = self
            .window_bindings
            .iter()
            .find_map(|(_, win)| {
                let matches = win.toplevel().is_some_and(|tl| tl.wl_surface() == &root);
                if matches {
                    self.space.element_location(win)
                } else {
                    None
                }
            })
            .or_else(|| {
                let layer_map = layer_map_for_output(&self.output);
                layer_map
                    .layers()
                    .find(|l| l.wl_surface() == &root)
                    .and_then(|l| layer_map.layer_geometry(l).map(|g| g.loc))
            })?;

        let parent_offset = if surface.get_parent_surface().as_ref() == Some(&root) {
            Point::from((0, 0))
        } else {
            get_popup_toplevel_coords(&kind)
        };
        let mut target = output_geo;
        target.loc -= root_origin + parent_offset;
        Some(target)
    }

    pub(in crate::state) fn surface_in_tree(root: &WlSurface, target: &WlSurface) -> bool {
        if root == target {
            return true;
        }
        let found = std::cell::Cell::new(false);
        compositor::with_surface_tree_downward(
            root,
            (),
            |_, _, ()| {
                if found.get() {
                    TraversalAction::SkipChildren
                } else {
                    TraversalAction::DoChildren(())
                }
            },
            |node, _, ()| {
                if node == target {
                    found.set(true);
                }
            },
            |_, _, ()| !found.get(),
        );
        found.get()
    }

    /// Looks up the [`WindowId`] associated with a Smithay [`Window`].
    #[must_use]
    pub fn window_id_for_smithay(&self, window: &Window) -> Option<WindowId> {
        self.window_bindings
            .iter()
            .find_map(|(id, w)| (w == window).then_some(*id))
    }

    /// Looks up the Smithay [`Window`] associated with a [`WindowId`].
    #[must_use]
    pub fn smithay_window_for_id(&self, id: WindowId) -> Option<Window> {
        self.window_bindings
            .iter()
            .find_map(|(wid, w)| (*wid == id).then(|| w.clone()))
    }

    /// Looks up the [`WindowId`] associated with a `ToplevelSurface`.
    #[must_use]
    pub fn window_id_for_toplevel(&self, surface: &ToplevelSurface) -> Option<WindowId> {
        let target_wl = surface.wl_surface();
        self.window_bindings.iter().find_map(|(win_id, sw)| {
            sw.toplevel()
                .is_some_and(|tl| tl.wl_surface() == target_wl)
                .then_some(*win_id)
        })
    }

    /// Looks up the [`WindowId`] matching the raw `xdg_toplevel` protocol resource (`RUSTYOS-047`).
    #[must_use]
    pub fn window_id_for_xdg_toplevel_proto(
        &self,
        xdg_tl: &xdg_toplevel::XdgToplevel,
    ) -> Option<WindowId> {
        self.window_bindings.iter().find_map(|(win_id, sw)| {
            sw.toplevel()
                .is_some_and(|tl| tl.xdg_toplevel() == xdg_tl)
                .then_some(*win_id)
        })
    }

    /// Clears decoration-negotiation tracking for a closed or unmapped window (`RUSTYOS-030`).
    pub fn unregister_window_decoration(&mut self, win_id: WindowId) {
        self.xdg_decoration_requested.remove(&win_id);
        self.csd_window_ids.remove(&win_id);
    }

    /// Releases decoration state, `ext_foreign_toplevel_list_v1` handle, and `xdg_toplevel_icon_v1`
    /// metadata for a closed or pruned window (`RUSTYOS-030` / `RUSTYOS-047`).
    pub fn unregister_window_metadata(&mut self, win_id: WindowId) {
        self.unregister_window_decoration(win_id);
        self.close_foreign_toplevel(win_id);
        self.window_toplevel_icons.remove(&win_id);
    }

    fn record_closed_app_if_last(&mut self, win_id: WindowId) {
        let title = self.window_title(win_id);
        let app_id = self.window_app_id(win_id);
        let app_key =
            crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, win_id);
        let has_other = self.window_bindings.iter().any(|(other_id, _)| {
            *other_id != win_id
                && crate::shell_render::canonical_dock_app_key(
                    self.window_app_id(*other_id).as_deref(),
                    &self.window_title(*other_id),
                    *other_id,
                ) == app_key
        });
        if has_other {
            return;
        }
        self.app_bounces.retain(|b| b.app_key != app_key);
        let is_pinned = self
            .user_config
            .dock_pinned_apps
            .iter()
            .any(|pin| pin.eq_ignore_ascii_case(&app_key));
        if is_pinned || app_key.starts_with("win-") {
            return;
        }
        let label = crate::shell_render::dock_label_for_key(&app_key);
        let icon_name = self
            .launcher
            .icon_for_app_hints(app_id.as_deref(), &title)
            .or_else(|| self.launcher.icon_for_dock_key(&app_key));
        self.recent_unpinned_apps.retain(|r| r.app_key != app_key);
        self.recent_unpinned_apps.insert(
            0,
            super::RecentDockApp {
                app_key,
                label,
                icon_name,
            },
        );
        self.recent_unpinned_apps.truncate(4);
    }

    /// Settles continuous launch bounces and drops closed-app recents once a matching window opens.
    pub fn refresh_dock_window_presence(&mut self, win_id: WindowId) {
        if self.app_bounces.is_empty() && self.recent_unpinned_apps.is_empty() {
            return;
        }
        let title = self.window_title(win_id);
        let app_id = self.window_app_id(win_id);
        let app_key =
            crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, win_id);
        self.settle_app_bounce(&app_key);
        self.recent_unpinned_apps.retain(|r| r.app_key != app_key);
    }

    /// Shared teardown executed when a Wayland or XWayland window closes (`RUSTYOS-073`).
    pub fn finish_window_teardown(&mut self, win_id: WindowId) {
        if self
            .active_window_drag
            .is_some_and(|drag| drag.window_id == win_id)
        {
            self.active_window_drag = None;
        }
        if self
            .active_window_resize
            .is_some_and(|resize| resize.window_id == win_id)
        {
            self.active_window_resize = None;
        }
        if let Some((_, sw)) = self.window_bindings.iter().find(|(id, _)| *id == win_id) {
            let sw_clone = sw.clone();
            self.space.unmap_elem(&sw_clone);
        }
        self.record_closed_app_if_last(win_id);
        self.window_bindings.retain(|(wid, _)| *wid != win_id);
        self.unregister_window_metadata(win_id);
        self.minimize_anims.retain(|anim| anim.win_id != win_id);
        self.minimized_window_ids.remove(&win_id);
        self.remove_from_switcher(win_id);
        self.wm.unmap_window(win_id);
        self.animations.remove_window(win_id.0);
    }

    /// Resolves the live UI title for `win_id` (falling back to `app_id`, then `Window #N`) (`RUSTYOS-047`).
    #[must_use]
    pub fn window_title(&self, win_id: WindowId) -> String {
        let Some(window) = self.smithay_window_for_id(win_id) else {
            return format!("Window #{}", win_id.0);
        };
        if let Some(toplevel) = window.toplevel() {
            let title = compositor::with_states(toplevel.wl_surface(), |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|data| data.lock().ok())
                    .and_then(|attrs| attrs.title.clone().or_else(|| attrs.app_id.clone()))
            });
            if let Some(t) = title
                && !t.trim().is_empty()
            {
                return t;
            }
        } else if let Some(x11) = window.x11_surface() {
            let title = x11.title();
            if !title.trim().is_empty() {
                return title;
            }
        }
        format!("Window #{}", win_id.0)
    }

    fn window_protocol_title(&self, win_id: WindowId) -> String {
        let Some(window) = self.smithay_window_for_id(win_id) else {
            return String::new();
        };
        if let Some(toplevel) = window.toplevel() {
            return compositor::with_states(toplevel.wl_surface(), |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|data| data.lock().ok())
                    .and_then(|attrs| attrs.title.clone())
            })
            .unwrap_or_default();
        }
        if let Some(x11) = window.x11_surface() {
            return x11.title();
        }
        String::new()
    }

    /// Creates and announces an `ext_foreign_toplevel_handle_v1` for a newly mapped window (`RUSTYOS-047`).
    pub fn register_foreign_toplevel(&mut self, win_id: WindowId) {
        if self.foreign_toplevel_handles.contains_key(&win_id) {
            self.sync_foreign_toplevel_metadata(win_id);
            return;
        }
        let title = self.window_protocol_title(win_id);
        let app_id = self.window_app_id(win_id).unwrap_or_default();
        let handle = self
            .foreign_toplevel_list_state
            .new_toplevel::<Self>(title, app_id);
        self.foreign_toplevel_handles.insert(win_id, handle);
    }

    /// Emits updated `title`/`app_id` + `done` on `win_id`'s `ext_foreign_toplevel_handle_v1` (`RUSTYOS-047`).
    pub fn sync_foreign_toplevel_metadata(&self, win_id: WindowId) {
        let Some(handle) = self.foreign_toplevel_handles.get(&win_id) else {
            return;
        };
        let title = self.window_protocol_title(win_id);
        let app_id = self.window_app_id(win_id).unwrap_or_default();
        let mut changed = false;
        if handle.title() != title {
            handle.send_title(&title);
            changed = true;
        }
        if handle.app_id() != app_id {
            handle.send_app_id(&app_id);
            changed = true;
        }
        if changed {
            handle.send_done();
        }
    }

    /// Sends `closed` and retires the `ext_foreign_toplevel_handle_v1` owned by `win_id` (`RUSTYOS-047`).
    pub fn close_foreign_toplevel(&mut self, win_id: WindowId) {
        if let Some(handle) = self.foreign_toplevel_handles.remove(&win_id) {
            self.foreign_toplevel_list_state.remove_toplevel(&handle);
        }
    }

    /// Syncs committed `xdg_toplevel_icon_v1` `ToplevelIconCachedState` for `win_id` (`RUSTYOS-047`).
    pub fn sync_toplevel_icon_from_surface(&mut self, win_id: WindowId, surface: &WlSurface) {
        let (icon_name, buffers) = compositor::with_states(surface, |states| {
            let mut guard = states.cached_state.get::<ToplevelIconSurfaceState>();
            let cur = guard.current();
            (cur.icon_name.clone(), cur.buffers.clone())
        });
        if icon_name.is_none() && buffers.is_empty() {
            self.window_toplevel_icons.remove(&win_id);
            return;
        }
        if self
            .window_toplevel_icons
            .get(&win_id)
            .is_some_and(|existing| existing.icon_name == icon_name && existing.buffers == buffers)
        {
            return;
        }
        self.next_toplevel_icon_rev = self.next_toplevel_icon_rev.saturating_add(1);
        self.window_toplevel_icons.insert(
            win_id,
            crate::icons::ShmToplevelIcon {
                revision: self.next_toplevel_icon_rev,
                icon_name,
                buffers,
            },
        );
    }

    /// Removes dead IME popup surfaces (`RUSTYOS-047`).
    pub fn cleanup_ime_popups(&mut self) {
        self.ime_popups.retain(ImePopupSurface::alive);
    }

    /// Computes output-local physical render positions for alive IME `PopupSurface` instances belonging
    /// to `target_output` (returns nothing while the session is locked) (`RUSTYOS-047`).
    #[must_use]
    pub fn ime_popups_for_output(
        &self,
        target_output: &Output,
    ) -> Vec<(ImePopupSurface, Point<i32, Logical>)> {
        if self.is_session_locked() {
            return Vec::new();
        }
        let target_geo = self
            .output_map
            .find_by_output(target_output)
            .map(crate::outputs::OutputEntry::logical_geometry)
            .or_else(|| self.space.output_geometry(target_output))
            .unwrap_or_else(|| self.output_map.primary().logical_geometry());

        let mut out = Vec::new();
        for popup in &self.ime_popups {
            if !popup.alive() {
                continue;
            }
            let parent_geo = popup
                .get_parent()
                .map_or_else(|| target_geo, |p| self.parent_geometry(&p.surface));
            let cursor_rect = popup.text_input_rectangle();
            let anchor = Point::from((
                f64::from(parent_geo.loc.x + cursor_rect.loc.x),
                f64::from(parent_geo.loc.y + cursor_rect.loc.y),
            ));
            let owner_id = self
                .output_map
                .output_under(anchor)
                .unwrap_or(self.output_map.primary().id);
            let target_id = self
                .output_map
                .find_by_output(target_output)
                .map_or(self.output_map.primary().id, |e| e.id);
            if owner_id != target_id {
                continue;
            }

            let popup_size = bbox_from_surface_tree(popup.wl_surface(), (0, 0)).size;
            let global_loc =
                compute_ime_popup_origin(parent_geo, cursor_rect, popup_size, target_geo);
            popup.set_location(global_loc - parent_geo.loc);
            out.push((popup.clone(), global_loc - target_geo.loc));
        }
        out
    }

    /// Emits Wayland frame callbacks for alive IME popups visible on `output` (`RUSTYOS-047`).
    pub fn send_ime_popup_frames(&self, output: &Output, elapsed: Duration) {
        if self.is_session_locked() {
            return;
        }
        for (popup, _) in self.ime_popups_for_output(output) {
            send_frames_surface_tree(
                popup.wl_surface(),
                output,
                elapsed,
                Some(Duration::ZERO),
                |_, _| Some(output.clone()),
            );
        }
    }

    /// Returns `true` if the window uses client-side decorations (CSD) and should not
    /// be wrapped with a RustyOS SSD titlebar (`RUSTYOS-030`).
    #[must_use]
    pub fn window_uses_csd(&self, win_id: WindowId) -> bool {
        if let Some(window) = self.smithay_window_for_id(win_id)
            && let Some(x11) = window.x11_surface()
        {
            return x11.is_decorated();
        }
        self.csd_window_ids.contains(&win_id)
    }

    /// Returns the SSD titlebar height reserved above the client area (`0` for CSD windows).
    #[must_use]
    pub fn window_titlebar_height(&self, win_id: WindowId) -> i32 {
        if self.is_window_fullscreen(win_id) || self.window_uses_csd(win_id) {
            0
        } else {
            crate::shell_render::TITLEBAR_HEIGHT
        }
    }

    /// Computes `(client_width, client_height)` inside the WM frame box (`RUSTYOS-030`).
    #[must_use]
    pub fn effective_client_size(&self, win_id: WindowId, rect: Rect) -> (i32, i32) {
        let titlebar_h = self.window_titlebar_height(win_id);
        (rect.width.max(64), (rect.height - titlebar_h).max(64))
    }

    /// Returns any client-declared minimum `(width, height)` in logical coordinates (`RUSTYOS-030`).
    #[must_use]
    pub fn window_min_size(&self, win_id: WindowId) -> Option<(i32, i32)> {
        let window = self.smithay_window_for_id(win_id)?;
        if let Some(x11) = window.x11_surface() {
            return x11.min_size().and_then(|s| {
                let w = s.w.max(0);
                let h = s.h.max(0);
                (w > 0 || h > 0).then_some((w, h))
            });
        }
        if let Some(toplevel) = window.toplevel() {
            let min = compositor::with_states(toplevel.wl_surface(), |states| {
                let mut guard = states
                    .cached_state
                    .get::<smithay::wayland::shell::xdg::SurfaceCachedState>();
                guard.current().min_size
            });
            let w = min.w.max(0);
            let h = min.h.max(0);
            return (w > 0 || h > 0).then_some((w, h));
        }
        None
    }

    /// Computes client-declared minimum `(width, height)` including SSD titlebar and clamped to work area (`RUSTYOS-112`).
    #[must_use]
    pub fn floating_min_bounds(&self, win_id: WindowId) -> Option<(i32, i32)> {
        let (min_w, min_h) = self.window_min_size(win_id)?;
        let titlebar_h = self.window_titlebar_height(win_id);
        let work_area = self.floating_work_area();
        let desired_w = min_w.min(work_area.width);
        let desired_h = min_h.saturating_add(titlebar_h).min(work_area.height);
        Some((desired_w, desired_h))
    }

    /// Returns the firm interactive resize minimum `(width, height)` for `win_id` (`RUSTYOS-112`).
    #[must_use]
    pub fn effective_window_min_size(&self, win_id: WindowId) -> (i32, i32) {
        self.floating_min_bounds(win_id)
            .map_or((240, 160), |(w, h)| (w.max(240), h.max(160)))
    }

    /// Expands a non-maximized floating window if client `min_size` exceeds its current box (`RUSTYOS-030`).
    pub fn ensure_floating_min_size(&mut self, win_id: WindowId) -> bool {
        if self.wm.active_layout_mode() != LayoutMode::Floating
            || self
                .active_window_resize
                .is_some_and(|resize| resize.window_id == win_id)
        {
            return false;
        }
        let Some(wm_win) = self.wm.window(win_id) else {
            return false;
        };
        if wm_win.maximized {
            return false;
        }
        let current = wm_win.geometry;
        let Some((desired_w, desired_h)) = self.floating_min_bounds(win_id) else {
            return false;
        };
        let work_area = self.floating_work_area();
        if desired_w <= current.width && desired_h <= current.height {
            return false;
        }
        let new_w = current.width.max(desired_w);
        let new_h = current.height.max(desired_h);
        let max_x = work_area.x + (work_area.width - new_w).max(0);
        let max_y = work_area.y + (work_area.height - new_h).max(0);
        let target = Rect::new(
            current.x.clamp(work_area.x, max_x),
            current.y.clamp(work_area.y, max_y),
            new_w,
            new_h,
        );
        if self
            .wm
            .resize_window_with_min(win_id, target, desired_w, desired_h)
        {
            self.animations.snap_window_immediate(
                win_id.0,
                LogicalRect::new(target.x, target.y, target.width, target.height),
            );
            return true;
        }
        false
    }

    fn floating_work_area(&self) -> Rect {
        self.space.output_geometry(&self.output).map_or_else(
            || Rect::new(0, crate::shell_render::TOP_BAR_HEIGHT, 1280, 744),
            |geo| {
                crate::shell_render::usable_work_area_with_dock(
                    geo.size.to_physical(1),
                    self.user_config.dock_position,
                    self.user_config.dock_icon_size,
                    self.user_config.dock_auto_hide,
                )
            },
        )
    }

    fn apply_preferred_initial_size(&mut self, win_id: WindowId) {
        if self.wm.active_layout_mode() != LayoutMode::Floating {
            return;
        }
        let Some(wm_win) = self.wm.window(win_id) else {
            return;
        };
        if wm_win.maximized {
            return;
        }
        let Some(app_id) = self.window_app_id(win_id) else {
            return;
        };
        if !(app_id.eq_ignore_ascii_case("org.rustyos.settings")
            || app_id.eq_ignore_ascii_case("rusty-settings"))
        {
            return;
        }
        let work_area = self.floating_work_area();
        let w = 882.min(work_area.width);
        let h = 600.min(work_area.height);
        let x = work_area.x + (work_area.width - w).max(0) / 2;
        let y = work_area.y + (work_area.height - h).max(0) / 2;
        let target = Rect::new(x, y, w, h);
        if self.wm.resize_window(win_id, target) {
            self.animations.snap_window_immediate(
                win_id.0,
                LogicalRect::new(target.x, target.y, target.width, target.height),
            );
        }
    }

    /// Flushes initial `xdg_toplevel` configure events once per dispatch turn after
    /// `zxdg_toplevel_decoration_v1` requests in the same batch have been processed (`RUSTYOS-030`).
    pub fn flush_pending_toplevel_configures(&mut self) {
        let focused_id = self.wm.focused_window();
        let pending_wins: Vec<(WindowId, ToplevelSurface)> = self
            .window_bindings
            .iter()
            .filter_map(|(win_id, sw)| {
                let toplevel = sw.toplevel()?.clone();
                (!toplevel.is_initial_configure_sent()).then_some((*win_id, toplevel))
            })
            .collect();

        if pending_wins.is_empty() {
            return;
        }

        for (win_id, toplevel) in pending_wins {
            if !self.xdg_decoration_requested.contains(&win_id) {
                self.csd_window_ids.insert(win_id);
            }
            self.apply_preferred_initial_size(win_id);
            if let Some(wm_win) = self.wm.window(win_id) {
                let rect = wm_win.geometry;
                let maximized = wm_win.maximized;
                let (client_w, client_h) = self.effective_client_size(win_id, rect);
                let is_focused = focused_id == Some(win_id);
                let dec_mode = if self.csd_window_ids.contains(&win_id) {
                    DecorationMode::ClientSide
                } else {
                    DecorationMode::ServerSide
                };
                toplevel.with_pending_state(|state| {
                    state.size = Some((client_w, client_h).into());
                    state.decoration_mode = Some(dec_mode);
                    if is_focused {
                        state.states.set(xdg_toplevel::State::Activated);
                    } else {
                        state.states.unset(xdg_toplevel::State::Activated);
                    }
                    if maximized {
                        state.states.set(xdg_toplevel::State::Maximized);
                    } else {
                        state.states.unset(xdg_toplevel::State::Maximized);
                    }
                });
            }
            toplevel.send_configure();
        }
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
    }

    pub(in crate::state) fn apply_xdg_decoration_mode(
        &mut self,
        toplevel: &ToplevelSurface,
        use_csd: bool,
    ) {
        let dec_mode = if use_csd {
            DecorationMode::ClientSide
        } else {
            DecorationMode::ServerSide
        };
        if let Some(win_id) = self.window_id_for_toplevel(toplevel) {
            self.xdg_decoration_requested.insert(win_id);
            if use_csd {
                self.csd_window_ids.insert(win_id);
            } else {
                self.csd_window_ids.remove(&win_id);
            }
            let client_size = self
                .wm
                .window(win_id)
                .map(|w| self.effective_client_size(win_id, w.geometry));
            toplevel.with_pending_state(|state| {
                state.decoration_mode = Some(dec_mode);
                if let Some((w, h)) = client_size {
                    state.size = Some((w, h).into());
                }
            });
            if toplevel.is_initial_configure_sent() {
                let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                self.sync_space_with_wm(serial);
                toplevel.send_configure();
            }
        } else {
            toplevel.with_pending_state(|state| {
                state.decoration_mode = Some(dec_mode);
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_configure();
            }
        }
    }

    /// Removes dead/closed client windows from `window_bindings` and [`WindowManager`],
    /// then synchronizes [`Space`] if any window disappeared.
    pub fn prune_dead_windows(&mut self) {
        if self.dnd_icon.as_ref().is_some_and(|surf| !surf.alive()) {
            self.dnd_icon = None;
        }
        if matches!(&self.cursor_status, CursorImageStatus::Surface(surf) if !surf.alive()) {
            self.cursor_status = CursorImageStatus::default_named();
        }
        self.active_popup_grabs.retain(PopupSurface::alive);
        let mut dead_ids = Vec::new();
        for (id, win) in &self.window_bindings {
            if !win.alive() {
                dead_ids.push(*id);
            }
        }
        if dead_ids.is_empty() {
            return;
        }

        for id in dead_ids {
            self.finish_window_teardown(id);
            info!(window_id = id.0, "Pruned closed Wayland window from WM");
        }
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
    }

    /// Applies a [`WmAction`] and synchronizes the compositor [`Space`], `xdg_toplevel` configure
    /// states, and keyboard focus.
    pub fn apply_wm_action(&mut self, action: WmAction, serial: Serial) {
        info!(?action, "Applying window manager action");
        let focused_is_minimized = self
            .wm
            .focused_window()
            .is_some_and(|id| self.is_window_minimized(id));
        if focused_is_minimized
            && matches!(
                action,
                WmAction::CloseFocused
                    | WmAction::ToggleMaximizeFocused
                    | WmAction::ToggleFullscreen
                    | WmAction::SnapFocused(_)
            )
        {
            return;
        }

        let prev_ws = self.wm.active_workspace_id();
        if let Some(close_id) = self.wm.apply_action(action) {
            if let Some(win) = self.smithay_window_for_id(close_id) {
                if let Some(toplevel) = win.toplevel() {
                    info!(
                        window_id = close_id.0,
                        "Sending xdg_toplevel::close to focused client"
                    );
                    toplevel.send_close();
                } else if let Some(x11) = win.x11_surface() {
                    info!(
                        window_id = close_id.0,
                        x11_id = x11.window_id(),
                        "Sending WM_DELETE_WINDOW close to XWayland client"
                    );
                    let _ = x11.close();
                }
            }
            return;
        }

        if matches!(action, WmAction::FocusNext | WmAction::FocusPrev) {
            let visible_count = self.wm.visible_windows().len();
            let has_unminimized = self
                .wm
                .visible_windows()
                .into_iter()
                .any(|w| !self.is_window_minimized(w.id));
            if has_unminimized {
                for _ in 0..visible_count {
                    if !self
                        .wm
                        .focused_window()
                        .is_some_and(|id| self.is_window_minimized(id))
                    {
                        break;
                    }
                    let _ = self.wm.apply_action(action);
                }
            }
        }

        if let WmAction::SwitchWorkspace(target_ws) = action
            && prev_ws != target_ws
        {
            self.window_switcher = None;
            let output_width = self.output.current_mode().map_or(1280, |m| m.size.w);
            self.animations.trigger_workspace_slide(
                prev_ws.0,
                target_ws.0,
                output_width,
                self.elapsed(),
            );
        }

        self.sync_space_with_wm(serial);
    }

    /// Synchronizes Smithay's [`Space<Window>`], `xdg_toplevel` pending configure geometry/states,
    /// and Wayland keyboard focus with the deterministic state in [`WindowManager`].
    pub fn sync_space_with_wm(&mut self, serial: Serial) {
        self.desktop_focused = false;
        let now = self.elapsed();
        let visible: Vec<(WindowId, Rect, bool, bool)> = self
            .wm
            .visible_windows()
            .into_iter()
            .filter(|w| !self.is_window_minimized(w.id))
            .map(|w| (w.id, w.geometry, w.maximized, w.fullscreen))
            .collect();
        let focused_id = self
            .wm
            .focused_window()
            .filter(|id| !self.is_window_minimized(*id));

        // 1. Unmap managed windows that are no longer on an active workspace, preserving override-redirect X11 popups.
        let current_space_windows: Vec<Window> = self.space.elements().cloned().collect();
        for sw in current_space_windows {
            if sw
                .x11_surface()
                .is_some_and(X11Surface::is_override_redirect)
            {
                continue;
            }
            let still_visible = self
                .window_id_for_smithay(&sw)
                .is_some_and(|id| visible.iter().any(|(vid, _, _, _)| *vid == id));
            if !still_visible {
                self.space.unmap_elem(&sw);
            }
        }

        // 2. Map/update visible windows in bottom-to-top z-order and send pending configures.
        for (win_id, rect, maximized, fullscreen) in visible {
            let is_focused = focused_id == Some(win_id);
            let center = Point::from((
                f64::from(rect.x) + f64::from(rect.width) * 0.5,
                f64::from(rect.y) + f64::from(rect.height) * 0.5,
            ));
            let output_scale = self
                .output_map
                .output_under(center)
                .and_then(|id| self.output_map.get(id))
                .map_or_else(|| self.output_map.primary().scale, |e| e.scale);
            self.animations.set_window_target(
                win_id.0,
                LogicalRect::new(rect.x, rect.y, rect.width, rect.height),
                now,
            );
            let origin = self
                .animations
                .animated_window_origin(win_id.0)
                .unwrap_or((rect.x, rect.y));

            if let Some(window) = self.smithay_window_for_id(win_id) {
                let has_ssd = !fullscreen && !self.window_uses_csd(win_id);
                let titlebar_h = self.window_titlebar_height(win_id);
                let (client_width, client_height) = self.effective_client_size(win_id, rect);
                // Smithay's `Space::render_location` already subtracts `window.geometry().loc`,
                // so `map_element` receives the target geometry origin (never `origin - geo_loc`).
                let content_origin = (origin.0, origin.1 + titlebar_h);
                if let Some(toplevel) = window.toplevel() {
                    toplevel.with_pending_state(|state| {
                        state.size = Some((client_width, client_height).into());
                        if is_focused {
                            state.states.set(xdg_toplevel::State::Activated);
                        } else {
                            state.states.unset(xdg_toplevel::State::Activated);
                        }
                        if maximized && !fullscreen {
                            state.states.set(xdg_toplevel::State::Maximized);
                        } else {
                            state.states.unset(xdg_toplevel::State::Maximized);
                        }
                        if fullscreen {
                            state.states.set(xdg_toplevel::State::Fullscreen);
                        } else {
                            state.states.unset(xdg_toplevel::State::Fullscreen);
                        }
                    });
                    if toplevel.is_initial_configure_sent() {
                        toplevel.send_pending_configure();
                    }
                } else if let Some(x11) = window.x11_surface() {
                    let _ = x11.configure(crate::xwayland::compute_x11_client_rect_for_decoration(
                        Rect::new(origin.0, origin.1, rect.width, rect.height),
                        has_ssd,
                    ));
                    let _ = x11.set_activated(is_focused);
                    let _ = x11.set_maximized(maximized && !fullscreen);
                    let _ = x11.set_fullscreen(fullscreen);
                }
                if let Some(root_ws) = WaylandFocus::wl_surface(&window) {
                    Self::apply_surface_scale(&root_ws, output_scale);
                }
                self.space.map_element(window, content_origin, is_focused);
            }
        }

        self.sync_keyboard_focus_with_wm(focused_id, serial);
    }

    fn sync_keyboard_focus_with_wm(&mut self, focused_id: Option<WindowId>, serial: Serial) {
        let focus_surface = if self.ext_session_locked {
            self.ext_lock_surfaces
                .iter()
                .rev()
                .find(|(_, ls)| ls.alive())
                .map(|(_, ls)| ls.wl_surface().clone())
        } else {
            focused_id
                .and_then(|id| self.smithay_window_for_id(id))
                .and_then(|w| WaylandFocus::wl_surface(&w).map(std::borrow::Cow::into_owned))
        };
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, focus_surface, serial);
    }

    /// Returns `true` if any compositor window, workspace, dock hover, or shell overlay animation
    /// (`Overview`, `Spotlight`, `Quick Settings / Control Center`, `Lock Screen`) is currently in flight.
    #[must_use]
    pub fn has_active_compositor_animations(&self) -> bool {
        self.overview_anim.is_active()
            || self.launcher_anim.is_active()
            || self.quick_settings_anim.is_active()
            || self.lock_screen_anim.is_active()
            || self.dock_hover_anim.is_active()
            || self.dock_auto_hide_anim.is_active()
            || !self.app_bounces.is_empty()
            || self
                .minimize_anims
                .iter()
                .any(|anim| anim.progress.is_active() || anim.snapshot_pending)
            || self.animations.has_active_animations()
    }

    /// Advances active window, workspace, and shell overlay transitions to the current monotonic timestamp
    /// and updates window positions in [`Space<Window>`].
    pub fn tick_animations(&mut self) {
        let now = self.elapsed();
        let _ = self.overview_anim.tick(now);
        let _ = self.launcher_anim.tick(now);
        let _ = self.quick_settings_anim.tick(now);
        let _ = self.lock_screen_anim.tick(now);
        let _ = self.dock_hover_anim.tick(now);
        let _ = self.dock_auto_hide_anim.tick(now);

        if !self.app_bounces.is_empty() {
            for bounce in &mut self.app_bounces {
                bounce.elapsed_ms = now.saturating_sub(bounce.started_at).as_secs_f64() * 1000.0;
            }
            let before = self.app_bounces.len();
            self.app_bounces.retain(|b| {
                let limit_ms = if b.continuous { 2600.0 } else { 980.0 };
                b.elapsed_ms < limit_ms
            });
            if self.app_bounces.len() != before {
                self.shell_redraw_needed = true;
            }
        }

        if !self.minimize_anims.is_empty() {
            let mut finished_any = false;
            for anim in &mut self.minimize_anims {
                let _ = anim.progress.tick(now);
                if !anim.progress.is_active() && !anim.snapshot_pending {
                    finished_any = true;
                }
            }
            self.minimize_anims
                .retain(|anim| anim.progress.is_active() || anim.snapshot_pending);
            if finished_any {
                self.shell_redraw_needed = true;
            }
        }

        if !self.animations.has_active_animations() {
            return;
        }
        let _ = self.animations.tick(now);

        let focused_id = self
            .wm
            .focused_window()
            .filter(|id| !self.is_window_minimized(*id));
        let visible_ids: Vec<WindowId> = self
            .wm
            .visible_windows()
            .into_iter()
            .filter(|w| !self.is_window_minimized(w.id))
            .map(|w| w.id)
            .collect();
        for win_id in visible_ids {
            if let Some(origin) = self.animations.animated_window_origin(win_id.0)
                && let Some(window) = self.smithay_window_for_id(win_id)
            {
                let is_focused = focused_id == Some(win_id);
                let titlebar_h = self.window_titlebar_height(win_id);
                let content_origin = (origin.0, origin.1 + titlebar_h);
                self.space.map_element(window, content_origin, is_focused);
            }
        }
    }

    /// Inserts a newly accepted Unix client stream into the Wayland display.
    pub fn insert_client_stream(&mut self, stream: std::os::unix::net::UnixStream) {
        if let Err(err) = self
            .display_handle
            .insert_client(stream, Arc::new(ClientState::default()))
        {
            warn!(error = %err, "Failed to insert Wayland client into display");
        }
    }

    /// Builds a [`DesktopStateSnapshot`] reflecting the current WM, animation, and frame-timing state.
    #[must_use]
    pub fn build_ipc_snapshot(&self) -> DesktopStateSnapshot {
        let active_ws = self.wm.active_workspace_id();
        let layout_str = match self.wm.active_layout_mode() {
            LayoutMode::Floating => "floating",
            LayoutMode::MasterStack => "master-stack",
        };
        let focused_id = self.wm.focused_window();
        let windows = self
            .wm
            .visible_windows()
            .into_iter()
            .map(|w| WindowSummary {
                id: w.id.0,
                workspace: w.workspace.0,
                focused: focused_id == Some(w.id),
                maximized: w.maximized,
            })
            .collect();

        let timing = self.frame_timing.summary();
        let notifications = self
            .notifications
            .iter()
            .map(|n| NotificationSummary {
                id: n.id,
                urgency: n.urgency,
                title: n.title.clone(),
                body: n.body.clone(),
            })
            .collect();
        DesktopStateSnapshot {
            active_workspace: active_ws.0,
            layout_mode: layout_str.to_owned(),
            reduced_motion: self.animations.reduced_motion(),
            overview_active: self.overview_active,
            launcher_active: self.launcher_active,
            launcher_query: self.launcher.query.clone(),
            screen_locked: self.screen_locked,
            notification_count: self.notification_count,
            frame_interval_p50_us: timing.interval_p50_us,
            input_latency_p95_us: timing.input_p95_us,
            windows,
            notifications,
        }
    }

    /// Processes a single [`ShellRequest`] over the versioned local IPC bus and returns a [`CompositorEvent`].
    pub fn handle_ipc_request(&mut self, request: ShellRequest) -> CompositorEvent {
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        match request {
            ShellRequest::Handshake {
                protocol_version,
                client_name,
            } => {
                info!(
                    protocol_version,
                    client = %client_name,
                    "Accepted IPC handshake from desktop shell component"
                );
                return CompositorEvent::HandshakeAck {
                    protocol_version,
                    server_version: env!("CARGO_PKG_VERSION").to_owned(),
                };
            }
            ShellRequest::GetStateSnapshot => {}
            ShellRequest::SwitchWorkspace { workspace } => {
                let ws = WorkspaceId::new_clamped(workspace);
                self.apply_wm_action(WmAction::SwitchWorkspace(ws), serial);
            }
            ShellRequest::FocusWindow { window_id } => {
                self.minimized_window_ids.remove(&WindowId(window_id));
                if self.wm.focus_and_raise(WindowId(window_id)) {
                    self.sync_space_with_wm(serial);
                }
            }
            ShellRequest::ToggleLayoutMode => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            ShellRequest::ToggleOverview => {
                self.toggle_overview();
                info!(
                    overview_active = self.overview_active,
                    "Toggled workspace overview via IPC"
                );
            }
            ShellRequest::ToggleLauncher => {
                self.toggle_launcher();
                info!(
                    launcher_active = self.launcher_active,
                    "Toggled Spotlight application launcher via IPC"
                );
            }
            ShellRequest::LaunchCommand { command } => {
                self.handle_launch_command_ipc(&command);
            }
            ShellRequest::SetReducedMotion { enabled } => {
                self.set_reduced_motion_pref(enabled);
                info!(reduced_motion = enabled, "Updated reduced-motion via IPC");
            }
            ShellRequest::SetLockScreen { locked } => {
                self.set_screen_locked(locked, serial);
                info!(
                    screen_locked = locked,
                    "Updated session lock screen state via IPC"
                );
            }
            ShellRequest::Notify {
                urgency,
                title,
                body,
            } => {
                let id = self.push_notification(urgency, title.clone(), body.clone());
                info!(
                    id,
                    urgency = urgency.as_str(),
                    title = %title,
                    body = %body,
                    count = self.notification_count,
                    "Received desktop notification via IPC"
                );
            }
            ShellRequest::DismissNotification { id } => {
                let dismissed = self.dismiss_notification(id);
                info!(
                    id,
                    dismissed,
                    count = self.notification_count,
                    "Processed notification dismissal via IPC"
                );
            }
            ShellRequest::ClearNotifications => {
                self.clear_notifications();
                info!("Cleared all active notifications via IPC");
            }
            ShellRequest::ReloadConfig => {
                let reloaded = self.reload_user_config();
                info!(reloaded, "Processed ReloadConfig request via IPC");
            }
        }
        CompositorEvent::StateSnapshot(self.build_ipc_snapshot())
    }

    fn handle_launch_command_ipc(&mut self, command: &str) {
        match command.trim() {
            "settings" | "rusty-settings" | "system-settings" => {
                self.close_launcher();
                self.open_settings();
            }
            "control-center" | "quick-settings" => {
                self.close_launcher();
                self.toggle_quick_settings();
            }
            other => {
                let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
                let _ = self
                    .launcher
                    .launch_command(other, &self.socket_name, &ipc_path);
                self.close_launcher();
            }
        }
    }
}
