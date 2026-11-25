//! `XWayland` server lifecycle and X11 Window Manager (`X11Wm`) integration (`RUSTYOS-026`).
//!
//! Spawns `Xwayland` asynchronously when available on `$PATH`, registers `xwayland_shell_v1`,
//! manages X11 application windows inside [`crate::state::RustyCompositorState`] alongside
//! native Wayland windows (with SSD titlebars, Dock slots, Overview, tiling/resize, and
//! keyboard focus), and degrades gracefully if `Xwayland` is unavailable.

use std::os::unix::io::OwnedFd;
use std::process::Stdio;

use rusty_wm::{Rect, WindowId};
use smithay::delegate_xwayland_shell;
use smithay::desktop::Window;
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size};
use smithay::wayland::selection::{
    SelectionTarget,
    data_device::{
        clear_data_device_selection, request_data_device_client_selection,
        set_data_device_selection,
    },
    primary_selection::{
        clear_primary_selection, request_primary_client_selection, set_primary_selection,
    },
};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{
    Reorder, ResizeEdge, WmWindowProperty, WmWindowType, X11Window, XwmId, settings::Value,
};
use smithay::xwayland::{X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler};
use tracing::{debug, info, warn};

use crate::shell_render::{ResizeEdges, TITLEBAR_HEIGHT};
use crate::state::{ClientState, RustyCompositorState, WindowDragState, WindowResizeState};

delegate_xwayland_shell!(RustyCompositorState);

/// Computes the inner client rectangle assigned to an X11 window below RustyOS's SSD titlebar.
#[must_use]
pub fn compute_x11_client_rect(wm_rect: Rect) -> Rectangle<i32, Logical> {
    compute_x11_client_rect_for_decoration(wm_rect, true)
}

/// Computes the inner client rectangle assigned to an X11 window, reserving RustyOS's
/// SSD titlebar only when `has_ssd` is `true` (`RUSTYOS-030`).
#[must_use]
pub fn compute_x11_client_rect_for_decoration(
    wm_rect: Rect,
    has_ssd: bool,
) -> Rectangle<i32, Logical> {
    let top_reserve = if has_ssd { TITLEBAR_HEIGHT } else { 0 };
    let client_h = (wm_rect.height - top_reserve).max(64);
    Rectangle::new(
        Point::from((wm_rect.x, wm_rect.y + top_reserve)),
        Size::from((wm_rect.width.max(64), client_h)),
    )
}

/// Computes standard `(Xft/DPI, Gdk/WindowScalingFactor, Gdk/UnscaledDPI)` `XSettings`
/// values (in 1024ths of a DPI) for the given display scale.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn compute_xsettings_dpi_values(scale: f64) -> (i32, i32, i32) {
    let clamped = crate::config::clamp_display_scale(scale);
    let dpi = ((96.0 * clamped).round() as i32).clamp(72, 384);
    let int_scale = if clamped >= 1.75 {
        (clamped.round() as i32).clamp(1, 4)
    } else {
        1
    };
    let unscaled_dpi = (dpi / int_scale).max(72);
    (dpi * 1024, int_scale, unscaled_dpi * 1024)
}

impl RustyCompositorState {
    /// Publishes `Xft/DPI`, `Gdk/WindowScalingFactor`, and `Gdk/UnscaledDPI` via `XSETTINGS`
    /// so XWayland apps (including Chrome/Chromium) scale UI and page content uniformly.
    pub fn sync_xwayland_xsettings(&mut self) {
        let scale = self.output_map.primary().scale;
        let (xft_dpi, gdk_scale, gdk_unscaled_dpi) = compute_xsettings_dpi_values(scale);
        let Some(xwm) = self.xwm.as_mut() else {
            return;
        };
        let settings = [
            (String::from("Xft/DPI"), Value::Integer(xft_dpi)),
            (
                String::from("Gdk/WindowScalingFactor"),
                Value::Integer(gdk_scale),
            ),
            (
                String::from("Gdk/UnscaledDPI"),
                Value::Integer(gdk_unscaled_dpi),
            ),
        ];
        if let Err(err) = xwm.set_xsettings(settings.into_iter()) {
            warn!(error = %err, "Failed to update XWayland XSETTINGS DPI");
        }
    }
}

/// Starts `XWayland` in the compositor `calloop` event loop without blocking startup.
///
/// Logs a warning and returns `false` if the `Xwayland` binary is missing or cannot spawn.
pub fn spawn_xwayland_source(
    display_handle: &DisplayHandle,
    loop_handle: &LoopHandle<'static, RustyCompositorState>,
) -> bool {
    let spawned = XWayland::spawn(
        display_handle,
        None,
        std::iter::empty::<(String, String)>(),
        true,
        Stdio::null(),
        Stdio::null(),
        |user_data| {
            user_data.insert_if_missing(ClientState::default);
        },
    );

    let (xwayland, client) = match spawned {
        Ok(pair) => pair,
        Err(err) => {
            warn!(error = %err, "Failed to spawn Xwayland; X11 compatibility disabled");
            return false;
        }
    };

    let cb_loop = loop_handle.clone();
    if let Err(err) =
        loop_handle.insert_source(xwayland, move |event, _metadata, state| match event {
            XWaylandEvent::Ready {
                x11_socket,
                display_number,
            } => match X11Wm::start_wm(cb_loop.clone(), x11_socket, client.clone()) {
                Ok(wm) => {
                    state.xwm = Some(wm);
                    state.x11_loop_handle = Some(cb_loop.clone());
                    state.set_xwayland_display(Some(display_number));
                    state.sync_xwayland_xsettings();
                    info!(
                        display = %format!(":{display_number}"),
                        "XWayland ready and X11 Window Manager attached"
                    );
                }
                Err(err) => {
                    warn!(error = %err, "Failed to attach X11Wm after XWayland startup");
                }
            },
            XWaylandEvent::Error => {
                warn!("XWayland server exited unexpectedly");
                state.xwm = None;
                state.set_xwayland_display(None);
                let x11_win_ids: Vec<WindowId> = state
                    .window_bindings
                    .iter()
                    .filter_map(|(id, sw)| sw.x11_surface().is_some().then_some(*id))
                    .collect();
                for id in x11_win_ids {
                    state.finish_window_teardown(id);
                }
                let or_elems: Vec<Window> = state
                    .space
                    .elements()
                    .filter(|el| el.x11_surface().is_some())
                    .cloned()
                    .collect();
                for el in or_elems {
                    state.space.unmap_elem(&el);
                }
                let serial = SERIAL_COUNTER.next_serial();
                state.sync_space_with_wm(serial);
            }
        })
    {
        warn!(error = %err, "Failed to register XWayland event source");
        return false;
    }

    true
}

impl XWaylandShellHandler for RustyCompositorState {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }
}

impl XwmHandler for RustyCompositorState {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        #[allow(clippy::expect_used)]
        self.xwm
            .as_mut()
            .expect("XwmHandler invoked without initialized X11Wm")
    }

    fn new_window(&mut self, _xwm: XwmId, window: X11Surface) {
        debug!(x11_id = window.window_id(), "Discovered new X11 window");
    }

    fn new_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        debug!(
            x11_id = window.window_id(),
            "Discovered new override-redirect X11 window"
        );
    }

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if window.is_override_redirect() {
            return;
        }
        let x11_id = window.window_id();
        let has_ssd = !window.is_decorated();
        if let Some(existing_id) = find_bound_window_id(self, x11_id) {
            if let Some(wm_win) = self.wm.window(existing_id) {
                let _ = window.configure(compute_x11_client_rect_for_decoration(
                    wm_win.geometry,
                    has_ssd,
                ));
            }
            let _ = window.set_mapped(true);
            let serial = SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            return;
        }

        let extra_h = if has_ssd { TITLEBAR_HEIGHT } else { 0 };
        let geo_size = window.geometry().size;
        let min_size = window.min_size();
        let min_w = min_size.map_or(0, |m| m.w);
        let min_h = min_size.map_or(0, |m| m.h);
        let pref_w = geo_size.w.max(min_w);
        let pref_h_raw = geo_size.h.max(min_h);
        let preferred_size = (pref_w >= 64 && pref_h_raw >= 64)
            .then_some((pref_w, pref_h_raw.saturating_add(extra_h)));

        let win_id = self.wm.next_window_id();
        let mut rect = self.wm.map_window(win_id, preferred_size);
        if let Some(max_sz) = window.max_size() {
            let max_w = max_sz.w.max(0);
            let max_h = if max_sz.h > 0 {
                max_sz.h.saturating_add(extra_h)
            } else {
                0
            };
            let _ = self.wm.set_window_max_size(win_id, Some((max_w, max_h)));
        }
        sync_x11_transient_and_dialog(self, win_id, &window, true);
        if let Some(updated) = self.wm.window(win_id) {
            rect = updated.geometry;
        }
        if let Err(err) = window.set_mapped(true) {
            warn!(x11_id, error = %err, "Failed to mark X11 window mapped");
            self.wm.unmap_window(win_id);
            return;
        }
        if let Err(err) = window.configure(compute_x11_client_rect_for_decoration(rect, has_ssd)) {
            warn!(x11_id, error = %err, "Failed to configure mapped X11 window");
        }
        let title = window.title();
        self.window_bindings
            .push((win_id, Window::new_x11_window(window)));
        self.register_foreign_toplevel(win_id);

        let serial = SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
        let mapped_rect = self.resolved_window_global_rect(win_id).unwrap_or(rect);
        self.register_window_open_animation(win_id, mapped_rect);

        info!(
            window_id = win_id.0,
            x11_id,
            title = %title,
            csd = !has_ssd,
            workspace = self.wm.active_workspace_id().0,
            ?rect,
            "Mapped new XWayland window via WindowManager"
        );
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        let loc = window.geometry().loc;
        let x11_id = window.window_id();
        let wrapped = Window::new_x11_window(window);
        self.space.map_element(wrapped, loc, false);
        debug!(x11_id, ?loc, "Mapped override-redirect X11 popup");
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        remove_x11_window(self, window.window_id(), "unmapped");
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        remove_x11_window(self, window.window_id(), "destroyed");
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id())
            && let Some(wm_win) = self.wm.window(win_id).cloned()
        {
            let has_ssd = !window.is_decorated();
            let extra_h = if has_ssd { TITLEBAR_HEIGHT } else { 0 };
            let ws_is_floating = self
                .wm
                .workspace(wm_win.workspace)
                .is_none_or(|ws| ws.layout == rusty_wm::LayoutMode::Floating);
            if ws_is_floating
                && !wm_win.maximized
                && !wm_win.fullscreen
                && (w.is_some() || h.is_some())
            {
                let work_area = self.floating_work_area_for_window(win_id);
                let (min_w, min_h) = self.effective_window_min_size(win_id);
                let max_w = work_area.width.max(min_w);
                let max_h = work_area.height.max(min_h);
                let req_w = w
                    .and_then(|v| i32::try_from(v).ok())
                    .map_or(wm_win.geometry.width, |v| v.clamp(min_w, max_w));
                let req_h = h
                    .and_then(|v| i32::try_from(v).ok())
                    .map_or(wm_win.geometry.height, |v| {
                        v.saturating_add(extra_h).clamp(min_h, max_h)
                    });
                let max_x = work_area.x + (work_area.width - req_w).max(0);
                let max_y = work_area.y + (work_area.height - req_h).max(0);
                let next_x = x
                    .unwrap_or(wm_win.geometry.x)
                    .clamp(work_area.x, max_x.max(work_area.x));
                let next_y = y
                    .map_or(wm_win.geometry.y, |ny| ny.saturating_sub(extra_h))
                    .clamp(work_area.y, max_y.max(work_area.y));
                let target = Rect::new(next_x, next_y, req_w, req_h);
                if self.wm.resize_window_with_min(win_id, target, min_w, min_h)
                    && let Some(updated) = self.wm.window(win_id)
                {
                    let rect = updated.geometry;
                    self.animations.snap_window_immediate(
                        win_id.0,
                        rusty_animation::LogicalRect::new(rect.x, rect.y, rect.width, rect.height),
                    );
                    let serial = SERIAL_COUNTER.next_serial();
                    self.sync_space_with_wm(serial);
                    return;
                }
            }
            let client_rect = compute_x11_client_rect_for_decoration(wm_win.geometry, has_ssd);
            let _ = window.configure(client_rect);
            return;
        }
        let mut geo = window.geometry();
        if let Some(nx) = x {
            geo.loc.x = nx;
        }
        if let Some(ny) = y {
            geo.loc.y = ny;
        }
        if let Some(nw) = w.and_then(|v| i32::try_from(v).ok()) {
            geo.size.w = nw.max(1);
        }
        if let Some(nh) = h.and_then(|v| i32::try_from(v).ok()) {
            geo.size.h = nh.max(1);
        }
        let _ = window.configure(geo);
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<X11Window>,
    ) {
        if window.is_override_redirect() {
            let existing = self.space.elements().find_map(|el| {
                el.x11_surface()
                    .is_some_and(|x| x.window_id() == window.window_id())
                    .then(|| el.clone())
            });
            if let Some(el) = existing {
                self.space.map_element(el, geometry.loc, false);
            }
        }
    }

    fn property_notify(&mut self, _xwm: XwmId, window: X11Surface, property: WmWindowProperty) {
        let Some(win_id) = find_bound_window_id(self, window.window_id()) else {
            return;
        };
        match property {
            WmWindowProperty::MotifHints | WmWindowProperty::NormalHints => {
                self.sync_window_max_size(win_id);
                self.ensure_floating_min_size(win_id);
                let serial = SERIAL_COUNTER.next_serial();
                self.sync_space_with_wm(serial);
            }
            WmWindowProperty::Title | WmWindowProperty::Class => {
                self.sync_foreign_toplevel_metadata(win_id);
                self.refresh_dock_window_presence(win_id);
                self.shell_redraw_needed = true;
            }
            WmWindowProperty::TransientFor | WmWindowProperty::WindowType => {
                sync_x11_transient_and_dialog(self, win_id, &window, true);
                let serial = SERIAL_COUNTER.next_serial();
                self.sync_space_with_wm(serial);
                self.shell_redraw_needed = true;
            }
            _ => {}
        }
    }

    fn maximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            let _ = self.wm.set_window_maximized(win_id, true);
            let serial = SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
        }
    }

    fn unmaximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            let _ = self.wm.set_window_maximized(win_id, false);
            let serial = SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
        }
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            let _ = self.wm.set_window_fullscreen(win_id, true);
            if self.wm.focused_window().is_none() {
                self.wm.focus_and_raise(win_id);
            }
            let serial = SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
        }
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            let _ = self.wm.set_window_fullscreen(win_id, false);
            let serial = SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
        }
    }

    fn minimize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            let serial = SERIAL_COUNTER.next_serial();
            self.minimize_window(win_id, serial);
        }
    }

    fn unminimize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            let serial = SERIAL_COUNTER.next_serial();
            self.restore_window(win_id, serial);
        }
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        _button: u32,
        resize_edge: ResizeEdge,
    ) {
        if !self.is_valid_pointer_grab_request(None) {
            return;
        }
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            if self
                .wm
                .window(win_id)
                .is_none_or(|w| w.fullscreen || w.maximized)
            {
                return;
            }
            let serial = SERIAL_COUNTER.next_serial();
            self.wm.focus_and_raise(win_id);
            self.sync_space_with_wm(serial);
            if let Some(wm_win) = self.wm.window(win_id) {
                self.active_window_drag = None;
                self.active_window_resize = Some(WindowResizeState {
                    window_id: win_id,
                    edges: map_x11_resize_edge(resize_edge),
                    start_pointer: self.pointer_location,
                    start_rect: wm_win.geometry,
                });
                self.sync_space_with_wm(serial);
            }
        }
    }

    fn move_request(&mut self, _xwm: XwmId, window: X11Surface, _button: u32) {
        if !self.is_valid_pointer_grab_request(None) {
            return;
        }
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            let serial = SERIAL_COUNTER.next_serial();
            self.wm.focus_and_raise(win_id);
            self.sync_space_with_wm(serial);
            if let Some(wm_win) = self.wm.window(win_id) {
                self.active_window_resize = None;
                self.active_window_drag =
                    Some(WindowDragState::new(win_id, self.pointer_location, wm_win));
            }
        }
    }

    fn allow_selection_access(&mut self, _xwm: XwmId, selection: SelectionTarget) -> bool {
        !self.is_session_locked()
            && matches!(
                selection,
                SelectionTarget::Clipboard | SelectionTarget::Primary
            )
    }

    fn send_selection(
        &mut self,
        _xwm: XwmId,
        selection: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
    ) {
        let result = match selection {
            SelectionTarget::Clipboard => {
                request_data_device_client_selection(&self.seat, mime_type, fd)
            }
            SelectionTarget::Primary => request_primary_client_selection(&self.seat, mime_type, fd)
                .map_err(|err| match err {
                    smithay::wayland::selection::primary_selection::SelectionRequestError::InvalidMimetype => {
                        smithay::wayland::selection::data_device::SelectionRequestError::InvalidMimetype
                    }
                    smithay::wayland::selection::primary_selection::SelectionRequestError::ServerSideSelection => {
                        smithay::wayland::selection::data_device::SelectionRequestError::ServerSideSelection
                    }
                    smithay::wayland::selection::primary_selection::SelectionRequestError::NoSelection => {
                        smithay::wayland::selection::data_device::SelectionRequestError::NoSelection
                    }
                }),
        };
        if let Err(err) = result {
            warn!(?selection, error = ?err, "Failed to transfer Wayland selection to XWayland");
        } else {
            let _ = self.display_handle.flush_clients();
        }
    }

    fn new_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        if self.is_session_locked() {
            return;
        }
        match selection {
            SelectionTarget::Clipboard => {
                set_data_device_selection(&self.display_handle, &self.seat, mime_types, ());
            }
            SelectionTarget::Primary => {
                set_primary_selection(&self.display_handle, &self.seat, mime_types, ());
            }
        }
        let _ = self.display_handle.flush_clients();
    }

    fn cleared_selection(&mut self, _xwm: XwmId, selection: SelectionTarget) {
        match selection {
            SelectionTarget::Clipboard => {
                clear_data_device_selection(&self.display_handle, &self.seat);
            }
            SelectionTarget::Primary => {
                clear_primary_selection(&self.display_handle, &self.seat);
            }
        }
        let _ = self.display_handle.flush_clients();
    }
}

fn sync_x11_transient_and_dialog(
    state: &mut RustyCompositorState,
    win_id: WindowId,
    window: &X11Surface,
    center_if_newly_parented: bool,
) {
    let parent_id = window
        .is_transient_for()
        .and_then(|parent_x11| find_bound_window_id(state, parent_x11));
    let is_dialog_type = matches!(
        window.window_type(),
        Some(WmWindowType::Dialog | WmWindowType::Utility | WmWindowType::Splash)
    );
    let parent_changed = state.wm.set_window_parent(win_id, parent_id);
    let _ = state
        .wm
        .set_window_dialog_flags(win_id, is_dialog_type || parent_id.is_some(), false);
    if center_if_newly_parented
        && parent_changed
        && parent_id.is_some()
        && let Some(rect) = state.wm.center_window_on_parent(win_id)
    {
        state.animations.snap_window_immediate(
            win_id.0,
            rusty_animation::LogicalRect::new(rect.x, rect.y, rect.width, rect.height),
        );
    }
}

fn find_bound_window_id(state: &RustyCompositorState, x11_id: X11Window) -> Option<WindowId> {
    state.window_bindings.iter().find_map(|(win_id, sw)| {
        sw.x11_surface()
            .is_some_and(|x| x.window_id() == x11_id)
            .then_some(*win_id)
    })
}

fn remove_x11_window(state: &mut RustyCompositorState, x11_id: X11Window, reason: &str) {
    if let Some(win_id) = find_bound_window_id(state, x11_id) {
        state.finish_window_teardown(win_id);
        let serial = SERIAL_COUNTER.next_serial();
        state.sync_space_with_wm(serial);
        info!(
            window_id = win_id.0,
            x11_id, reason, "Removed XWayland window from WM"
        );
        return;
    }

    let or_elem = state.space.elements().find_map(|el| {
        el.x11_surface()
            .is_some_and(|x| x.window_id() == x11_id)
            .then(|| el.clone())
    });
    if let Some(el) = or_elem {
        state.space.unmap_elem(&el);
        debug!(x11_id, reason, "Unmapped override-redirect X11 window");
    }
}

const fn map_x11_resize_edge(edge: ResizeEdge) -> ResizeEdges {
    match edge {
        ResizeEdge::Top => ResizeEdges {
            left: false,
            right: false,
            top: true,
            bottom: false,
        },
        ResizeEdge::Bottom => ResizeEdges {
            left: false,
            right: false,
            top: false,
            bottom: true,
        },
        ResizeEdge::Left => ResizeEdges {
            left: true,
            right: false,
            top: false,
            bottom: false,
        },
        ResizeEdge::Right => ResizeEdges {
            left: false,
            right: true,
            top: false,
            bottom: false,
        },
        ResizeEdge::TopLeft => ResizeEdges {
            left: true,
            right: false,
            top: true,
            bottom: false,
        },
        ResizeEdge::TopRight => ResizeEdges {
            left: false,
            right: true,
            top: true,
            bottom: false,
        },
        ResizeEdge::BottomLeft => ResizeEdges {
            left: true,
            right: false,
            top: false,
            bottom: true,
        },
        ResizeEdge::BottomRight => ResizeEdges {
            left: false,
            right: true,
            top: false,
            bottom: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_x11_client_rect_reserves_ssd_titlebar_and_clamps_minimums() {
        let rect = compute_x11_client_rect(Rect::new(100, 80, 800, 600));
        assert_eq!(rect.loc.x, 100);
        assert_eq!(rect.loc.y, 80 + TITLEBAR_HEIGHT);
        assert_eq!(rect.size.w, 800);
        assert_eq!(rect.size.h, 600 - TITLEBAR_HEIGHT);

        let tiny = compute_x11_client_rect(Rect::new(0, 0, 20, 20));
        assert_eq!(tiny.size.w, 64);
        assert_eq!(tiny.size.h, 64);

        let csd = compute_x11_client_rect_for_decoration(Rect::new(100, 80, 800, 600), false);
        assert_eq!(csd.loc.x, 100);
        assert_eq!(csd.loc.y, 80);
        assert_eq!(csd.size.w, 800);
        assert_eq!(csd.size.h, 600);
    }
}
