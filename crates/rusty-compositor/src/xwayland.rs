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
use smithay::xwayland::xwm::{Reorder, ResizeEdge, X11Window, XwmId};
use smithay::xwayland::{X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler};
use tracing::{debug, info, warn};

use crate::shell_render::{ResizeEdges, TITLEBAR_HEIGHT};
use crate::state::{ClientState, RustyCompositorState, WindowDragState, WindowResizeState};

delegate_xwayland_shell!(RustyCompositorState);

/// Computes the inner client rectangle assigned to an X11 window below RustyOS's SSD titlebar.
#[must_use]
pub fn compute_x11_client_rect(wm_rect: Rect) -> Rectangle<i32, Logical> {
    let client_h = (wm_rect.height - TITLEBAR_HEIGHT).max(64);
    Rectangle::new(
        Point::from((wm_rect.x, wm_rect.y + TITLEBAR_HEIGHT)),
        Size::from((wm_rect.width.max(64), client_h)),
    )
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
        if let Some(existing_id) = find_bound_window_id(self, x11_id) {
            if let Some(wm_win) = self.wm.window(existing_id) {
                let _ = window.configure(compute_x11_client_rect(wm_win.geometry));
            }
            let _ = window.set_mapped(true);
            let serial = SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            return;
        }

        let win_id = self.wm.next_window_id();
        let rect = self.wm.map_window(win_id, None);
        if let Err(err) = window.set_mapped(true) {
            warn!(x11_id, error = %err, "Failed to mark X11 window mapped");
            self.wm.unmap_window(win_id);
            return;
        }
        if let Err(err) = window.configure(compute_x11_client_rect(rect)) {
            warn!(x11_id, error = %err, "Failed to configure mapped X11 window");
        }
        let title = window.title();
        self.window_bindings
            .push((win_id, Window::new_x11_window(window)));

        let serial = SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);

        info!(
            window_id = win_id.0,
            x11_id,
            title = %title,
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
            && let Some(wm_win) = self.wm.window(win_id)
        {
            let _ = window.configure(compute_x11_client_rect(wm_win.geometry));
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

    fn maximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            self.wm.focus_and_raise(win_id);
            if !self.wm.window(win_id).is_some_and(|w| w.maximized) {
                let serial = SERIAL_COUNTER.next_serial();
                self.apply_wm_action(rusty_wm::WmAction::ToggleMaximizeFocused, serial);
            }
        }
    }

    fn unmaximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id()) {
            self.wm.focus_and_raise(win_id);
            if self.wm.window(win_id).is_some_and(|w| w.maximized) {
                let serial = SERIAL_COUNTER.next_serial();
                self.apply_wm_action(rusty_wm::WmAction::ToggleMaximizeFocused, serial);
            }
        }
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        _button: u32,
        resize_edge: ResizeEdge,
    ) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id())
            && let Some(wm_win) = self.wm.window(win_id)
        {
            self.active_window_drag = None;
            self.active_window_resize = Some(WindowResizeState {
                window_id: win_id,
                edges: map_x11_resize_edge(resize_edge),
                start_pointer: self.pointer_location,
                start_rect: wm_win.geometry,
            });
        }
    }

    fn move_request(&mut self, _xwm: XwmId, window: X11Surface, _button: u32) {
        if let Some(win_id) = find_bound_window_id(self, window.window_id())
            && let Some(wm_win) = self.wm.window(win_id)
        {
            self.active_window_resize = None;
            self.active_window_drag = Some(WindowDragState {
                window_id: win_id,
                start_pointer: self.pointer_location,
                start_origin: (wm_win.geometry.x, wm_win.geometry.y),
            });
        }
    }

    fn allow_selection_access(&mut self, _xwm: XwmId, selection: SelectionTarget) -> bool {
        !self.screen_locked
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
        if self.screen_locked {
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

fn find_bound_window_id(state: &RustyCompositorState, x11_id: X11Window) -> Option<WindowId> {
    state.window_bindings.iter().find_map(|(win_id, sw)| {
        sw.x11_surface()
            .is_some_and(|x| x.window_id() == x11_id)
            .then_some(*win_id)
    })
}

fn remove_x11_window(state: &mut RustyCompositorState, x11_id: X11Window, reason: &str) {
    if let Some(win_id) = find_bound_window_id(state, x11_id) {
        if state
            .active_window_drag
            .is_some_and(|drag| drag.window_id == win_id)
        {
            state.active_window_drag = None;
        }
        if state
            .active_window_resize
            .is_some_and(|resize| resize.window_id == win_id)
        {
            state.active_window_resize = None;
        }
        if let Some((_, sw)) = state.window_bindings.iter().find(|(id, _)| *id == win_id) {
            let sw_clone = sw.clone();
            state.space.unmap_elem(&sw_clone);
        }
        state.window_bindings.retain(|(id, _)| *id != win_id);
        state.wm.unmap_window(win_id);
        state.animations.remove_window(win_id.0);
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
    }
}
