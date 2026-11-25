//! Core surface, SHM, output, XDG shell, layer shell, decoration, and scaling handlers.

use rusty_wm::Rect;
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_fractional_scale, delegate_layer_shell, delegate_output,
    delegate_shm, delegate_viewporter, delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{
        LayerSurface as DesktopLayerSurface, PopupKind, Window, WindowSurfaceType,
        find_popup_root_surface, layer_map_for_output,
    },
    output::Output,
    reexports::{
        wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_protocols::xdg::toplevel_icon::v1::server::xdg_toplevel_icon_v1::{self},
        wayland_server::{
            Client, Resource,
            protocol::{wl_buffer, wl_output::WlOutput, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{self, CompositorClientState, CompositorHandler, CompositorState},
        fractional_scale::FractionalScaleHandler,
        output::OutputHandler,
        seat::WaylandFocus,
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::{
                Configure, PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler,
                XdgShellState, decoration::XdgDecorationHandler,
            },
        },
        shm::{ShmHandler, ShmState},
    },
    xwayland::XWaylandClientData,
};
use tracing::{info, warn};

use super::super::{ClientState, RustyCompositorState, WindowDragState, WindowResizeState};

impl BufferHandler for RustyCompositorState {
    fn buffer_destroyed(&mut self, buffer: &wl_buffer::WlBuffer) {
        let buf_id = buffer.id();
        for entry in self.toplevel_icons.values() {
            if entry.references_buffer(&buf_id) && entry.resource.is_alive() {
                entry.resource.post_error(
                    xdg_toplevel_icon_v1::Error::NoBuffer,
                    "The provided buffer has been destroyed before the toplevel icon",
                );
            }
        }
    }
}

impl CompositorHandler for RustyCompositorState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(state) = client.get_data::<ClientState>() {
            &state.compositor_state
        } else if let Some(xwayland_state) = client
            .get_data::<XWaylandClientData>()
            .and_then(|data| data.user_data().get::<ClientState>())
        {
            &xwayland_state.compositor_state
        } else {
            static DEFAULT_CLIENT_STATE: std::sync::LazyLock<CompositorClientState> =
                std::sync::LazyLock::new(CompositorClientState::default);
            &DEFAULT_CLIENT_STATE
        }
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        compositor::add_pre_commit_hook::<Self, _>(surface, Self::handle_syncobj_pre_commit);
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        let surface_scale = self.preferred_scale_for_surface(surface);
        Self::apply_surface_scale(surface, surface_scale);

        if let Some(window) = self.window_bindings.iter().find_map(|(_, w)| {
            WaylandFocus::wl_surface(w)
                .is_some_and(|ws| &*ws == surface)
                .then(|| w.clone())
        }) {
            window.on_commit();
            if let Some(win_id) = self.window_id_for_smithay(&window) {
                self.toplevel_initial_committed.insert(win_id);
                self.bump_window_commit_revision(win_id);
                self.sync_toplevel_icon_from_surface(win_id, surface);
                self.sync_foreign_toplevel_metadata(win_id);
                self.refresh_dock_window_presence(win_id);
                self.sync_window_max_size(win_id);
                if self.ensure_floating_min_size(win_id) {
                    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                    self.sync_space_with_wm(serial);
                }
                if !self.is_window_minimized(win_id)
                    && let Some(wm_win) = self.wm.window(win_id)
                    && self.wm.output_for_workspace(wm_win.workspace).is_some()
                {
                    let origin = self
                        .animations
                        .animated_window_origin(win_id.0)
                        .unwrap_or((wm_win.geometry.x, wm_win.geometry.y));
                    let titlebar_h = self.window_titlebar_height(win_id);
                    let content_origin = (origin.0, origin.1 + titlebar_h);
                    let is_focused = self.wm.focused_window() == Some(win_id);
                    // Preserve z-order (do not raise above override-redirect X11 popups on commit).
                    self.space.map_element(window, content_origin, false);
                    if is_focused && self.should_allow_window_keyboard_focus() {
                        let keyboard = self.keyboard.clone();
                        if keyboard.current_focus().as_ref() != Some(surface) {
                            keyboard.set_focus(
                                self,
                                Some(surface.clone()),
                                smithay::utils::SERIAL_COUNTER.next_serial(),
                            );
                        }
                    }
                }
            }
        } else if let Some(or_win) = self.space.elements().find_map(|el| {
            el.x11_surface()
                .is_some_and(smithay::xwayland::X11Surface::is_override_redirect)
                .then(|| {
                    WaylandFocus::wl_surface(el)
                        .is_some_and(|ws| &*ws == surface)
                        .then(|| el.clone())
                })
                .flatten()
        }) {
            or_win.on_commit();
        } else if let Some(parent_win) = self.window_bindings.iter().find_map(|(_, w)| {
            WaylandFocus::wl_surface(w)
                .is_some_and(|ws| Self::surface_in_tree(&ws, surface))
                .then(|| w.clone())
        }) {
            parent_win.on_commit();
            if let Some(win_id) = self.window_id_for_smithay(&parent_win) {
                self.bump_window_commit_revision(win_id);
            }
        }

        if let Some(PopupKind::Xdg(xdg)) = self.popups.find_popup(surface) {
            if !xdg.is_initial_configure_sent()
                && let Err(err) = xdg.send_configure()
            {
                warn!(error = %err, "Failed to send initial configure to xdg_popup");
            }
            if let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(xdg))
                && let Some(parent_win) = self.window_bindings.iter().find_map(|(_, w)| {
                    WaylandFocus::wl_surface(w)
                        .is_some_and(|ws| *ws == root)
                        .then(|| w.clone())
                })
            {
                parent_win.on_commit();
            }
        }
        self.popups.commit(surface);
        self.cleanup_ime_popups();
        self.commit_layer_surface(surface);
        self.shell_redraw_needed = true;
        self.client_surface_committed = true;
        self.wake_event_loop();
    }
}

impl RustyCompositorState {
    fn commit_layer_surface(&mut self, surface: &WlSurface) {
        let entries: Vec<_> = self.output_map.entries().to_vec();
        let mut matched_entry = None;
        for entry in entries {
            let found = {
                let mut layer_map = layer_map_for_output(&entry.output);
                if let Some(layer) = layer_map
                    .layer_for_surface(surface, WindowSurfaceType::ALL)
                    .cloned()
                {
                    let initial_configure_sent = compositor::with_states(surface, |states| {
                        states
                            .data_map
                            .get::<LayerSurfaceData>()
                            .map(|data| data.lock().is_ok_and(|d| d.initial_configure_sent))
                    })
                    .unwrap_or_default();

                    if !initial_configure_sent {
                        layer.layer_surface().send_configure();
                    }

                    layer_map.arrange();
                    true
                } else {
                    false
                }
            };
            if found {
                matched_entry = Some(entry);
                break;
            }
        }

        if let Some(entry) = matched_entry {
            let geo = entry.logical_geometry();
            let full_area = Rect::new(
                geo.loc.x,
                geo.loc.y,
                geo.size.w.max(100),
                geo.size.h.max(100),
            );
            let work_area = self.effective_work_area_for_output_entry(&entry);
            self.wm
                .upsert_output_with_full_area(entry.id, work_area, full_area);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
        }
    }
}

impl ShmHandler for RustyCompositorState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl OutputHandler for RustyCompositorState {}

impl XdgShellHandler for RustyCompositorState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let win_id = self.wm.next_window_id();
        let min = compositor::with_states(surface.wl_surface(), |states| {
            let mut guard = states
                .cached_state
                .get::<smithay::wayland::shell::xdg::SurfaceCachedState>();
            guard.current().min_size
        });
        let preferred_size = (min.w >= 64 && min.h >= 64).then_some((
            min.w,
            min.h.saturating_add(crate::shell_render::TITLEBAR_HEIGHT),
        ));
        let rect = self.wm.map_window(win_id, preferred_size);

        let client_height = (rect.height - crate::shell_render::TITLEBAR_HEIGHT).max(64);
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
            state.size = Some((rect.width, client_height).into());
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });

        let window = Window::new_wayland_window(surface.clone());
        self.window_bindings.push((win_id, window));
        self.sync_xdg_toplevel_parent_and_modal(win_id, &surface, true);
        self.register_foreign_toplevel(win_id);

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
        let mapped_rect = self.resolved_window_global_rect(win_id).unwrap_or(rect);
        self.register_window_open_animation(win_id, mapped_rect);

        info!(
            window_id = win_id.0,
            workspace = self.wm.active_workspace_id().0,
            ?rect,
            "Mapped new xdg_toplevel window via WindowManager (awaiting decoration negotiation)"
        );
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        let constrained = self.popup_parent_target_rect(&surface).map_or_else(
            || positioner.get_geometry(),
            |target| positioner.get_unconstrained_geometry(target),
        );
        surface.with_pending_state(|state| {
            state.geometry = constrained;
            state.positioner = positioner;
        });
        if let Err(err) = self.popups.track_popup(PopupKind::from(surface)) {
            warn!(error = %err, "Failed to track xdg_popup");
        }
    }

    fn move_request(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, serial: Serial) {
        if !self.is_valid_pointer_grab_request(Some(serial)) {
            return;
        }
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.wm.focus_and_raise(win_id);
            self.sync_space_with_wm(serial);
            if let Some(wm_win) = self.wm.window(win_id) {
                self.active_window_resize = None;
                self.active_window_drag =
                    Some(WindowDragState::new(win_id, self.pointer_location, wm_win));
            }
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        if !self.is_valid_pointer_grab_request(Some(serial)) {
            return;
        }
        let Some(mapped_edges) = map_xdg_resize_edge(edges) else {
            return;
        };
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            if self
                .wm
                .window(win_id)
                .is_none_or(|w| w.fullscreen || w.maximized)
            {
                return;
            }
            self.wm.focus_and_raise(win_id);
            self.sync_space_with_wm(serial);
            if let Some(wm_win) = self.wm.window(win_id) {
                self.active_window_drag = None;
                self.active_window_resize = Some(WindowResizeState {
                    window_id: win_id,
                    edges: mapped_edges,
                    start_pointer: self.pointer_location,
                    start_rect: wm_win.geometry,
                });
                self.sync_space_with_wm(serial);
            }
        }
    }

    fn grab(&mut self, surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        self.active_popup_grabs.retain(PopupSurface::alive);
        if surface.alive()
            && !self
                .active_popup_grabs
                .iter()
                .any(|existing| existing.wl_surface() == surface.wl_surface())
        {
            self.active_popup_grabs.push(surface);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            let _ = self.wm.set_window_maximized(win_id, true);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            if surface.is_initial_configure_sent() {
                surface.send_configure();
            }
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            let _ = self.wm.set_window_maximized(win_id, false);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            if surface.is_initial_configure_sent() {
                surface.send_configure();
            }
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, output: Option<WlOutput>) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            let target_output = output
                .as_ref()
                .and_then(Output::from_resource)
                .and_then(|out| {
                    self.output_map
                        .entries()
                        .iter()
                        .find(|e| e.output == out)
                        .map(|e| e.id)
                });
            let _ = self
                .wm
                .set_window_fullscreen_on_output(win_id, true, target_output);
            if self.wm.focused_window().is_none() {
                self.wm.focus_and_raise(win_id);
            }
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            if surface.is_initial_configure_sent() {
                surface.send_configure();
            }
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            let _ = self.wm.set_window_fullscreen(win_id, false);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            if surface.is_initial_configure_sent() {
                surface.send_configure();
            }
        }
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.minimize_window(win_id, serial);
        }
    }

    fn show_window_menu(
        &mut self,
        surface: ToplevelSurface,
        _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
        serial: Serial,
        location: Point<i32, Logical>,
    ) {
        if self.is_session_locked() || !self.is_valid_pointer_grab_request(Some(serial)) {
            return;
        }
        if let Some(win_id) = self.window_id_for_toplevel(&surface)
            && let Some(win) = self.wm.window(win_id)
        {
            let titlebar_h = self.window_titlebar_height(win_id);
            let global_pos = Point::<f64, Logical>::from((
                f64::from(win.geometry.x + location.x),
                f64::from(win.geometry.y + titlebar_h + location.y),
            ));
            self.wm.focus_and_raise(win_id);
            self.open_window_context_menu(global_pos, win_id);
        }
    }

    fn ack_configure(&mut self, surface: WlSurface, configure: Configure) {
        let serial = match configure {
            Configure::Toplevel(tl) => tl.serial,
            Configure::Popup(pp) => pp.serial,
        };
        if let Some(win_id) = self.window_bindings.iter().find_map(|(id, w)| {
            WaylandFocus::wl_surface(w)
                .is_some_and(|ws| *ws == surface)
                .then_some(*id)
        }) {
            self.last_acknowledged_configure.insert(win_id, serial);
        }
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        let constrained = self.popup_parent_target_rect(&surface).map_or_else(
            || positioner.get_geometry(),
            |target| positioner.get_unconstrained_geometry(target),
        );
        surface.with_pending_state(|state| {
            state.geometry = constrained;
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        self.active_popup_grabs
            .retain(|existing| existing.wl_surface() != surface.wl_surface() && existing.alive());
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.sync_foreign_toplevel_metadata(win_id);
            self.refresh_dock_window_presence(win_id);
            self.shell_redraw_needed = true;
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.sync_foreign_toplevel_metadata(win_id);
            self.refresh_dock_window_presence(win_id);
            self.shell_redraw_needed = true;
        }
    }

    fn parent_changed(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.sync_xdg_toplevel_parent_and_modal(win_id, &surface, true);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            self.shell_redraw_needed = true;
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.finish_window_teardown(win_id);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            info!(
                window_id = win_id.0,
                "Removed destroyed xdg_toplevel window"
            );
        }
    }
}

const fn map_xdg_resize_edge(
    edge: xdg_toplevel::ResizeEdge,
) -> Option<crate::shell_render::ResizeEdges> {
    match edge {
        xdg_toplevel::ResizeEdge::Top => Some(crate::shell_render::ResizeEdges {
            left: false,
            right: false,
            top: true,
            bottom: false,
        }),
        xdg_toplevel::ResizeEdge::Bottom => Some(crate::shell_render::ResizeEdges {
            left: false,
            right: false,
            top: false,
            bottom: true,
        }),
        xdg_toplevel::ResizeEdge::Left => Some(crate::shell_render::ResizeEdges {
            left: true,
            right: false,
            top: false,
            bottom: false,
        }),
        xdg_toplevel::ResizeEdge::Right => Some(crate::shell_render::ResizeEdges {
            left: false,
            right: true,
            top: false,
            bottom: false,
        }),
        xdg_toplevel::ResizeEdge::TopLeft => Some(crate::shell_render::ResizeEdges {
            left: true,
            right: false,
            top: true,
            bottom: false,
        }),
        xdg_toplevel::ResizeEdge::TopRight => Some(crate::shell_render::ResizeEdges {
            left: false,
            right: true,
            top: true,
            bottom: false,
        }),
        xdg_toplevel::ResizeEdge::BottomLeft => Some(crate::shell_render::ResizeEdges {
            left: true,
            right: false,
            top: false,
            bottom: true,
        }),
        xdg_toplevel::ResizeEdge::BottomRight => Some(crate::shell_render::ResizeEdges {
            left: false,
            right: true,
            top: false,
            bottom: true,
        }),
        _ => None,
    }
}

impl WlrLayerShellHandler for RustyCompositorState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.wlr_layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .unwrap_or_else(|| self.output.clone());

        let mut layer_map = layer_map_for_output(&output);
        let desktop_layer = DesktopLayerSurface::new(surface, namespace.clone());
        if let Err(err) = layer_map.map_layer(&desktop_layer) {
            warn!(error = ?err, namespace = %namespace, "Failed to map layer surface to output");
        } else {
            info!(
                namespace = %namespace,
                output = output.name(),
                "Mapped new zwlr_layer_surface_v1 layer to output"
            );
        }
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let entries: Vec<_> = self.output_map.entries().to_vec();
        let mut removed_any = false;
        for entry in entries {
            let mut layer_map = layer_map_for_output(&entry.output);
            let target_layer = layer_map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();

            if let Some(layer) = target_layer {
                layer_map.unmap_layer(&layer);
                layer_map.arrange();
                info!(
                    namespace = %layer.namespace(),
                    output = entry.output.name(),
                    "Unmapped destroyed layer surface"
                );
                removed_any = true;
                break;
            }
        }
        if removed_any {
            self.relayout_wm_for_all_outputs();
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
        }
    }
}

impl XdgDecorationHandler for RustyCompositorState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        self.apply_xdg_decoration_mode(&toplevel, false);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: DecorationMode) {
        // Respect ClientSide requests (Chrome tabs, PWAs, Firefox, Electron, GTK)
        // so browsers never receive a duplicate SSD bar above their tab strip;
        // clients requesting ServerSide (or leaving mode unset) get RustyOS SSD.
        self.apply_xdg_decoration_mode(&toplevel, mode == DecorationMode::ClientSide);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        self.apply_xdg_decoration_mode(&toplevel, false);
    }
}

impl FractionalScaleHandler for RustyCompositorState {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self.preferred_scale_for_surface(&surface);
        Self::apply_surface_scale(&surface, scale);
    }
}

delegate_compositor!(RustyCompositorState);
delegate_shm!(RustyCompositorState);
delegate_viewporter!(RustyCompositorState);
delegate_fractional_scale!(RustyCompositorState);
delegate_output!(RustyCompositorState);
delegate_xdg_shell!(RustyCompositorState);
delegate_xdg_decoration!(RustyCompositorState);
delegate_layer_shell!(RustyCompositorState);
