//! Core surface, SHM, output, XDG shell, layer shell, decoration, and scaling handlers.

use rusty_wm::{Rect, WmAction};
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
    utils::Serial,
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
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                decoration::XdgDecorationHandler,
            },
        },
        shm::{ShmHandler, ShmState},
    },
    xwayland::XWaylandClientData,
};
use tracing::{info, warn};

use super::super::{
    ClientState, PRIMARY_OUTPUT_ID, RustyCompositorState, WindowDragState, WindowResizeState,
};

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
                self.sync_toplevel_icon_from_surface(win_id, surface);
                self.sync_foreign_toplevel_metadata(win_id);
                if self.ensure_floating_min_size(win_id) {
                    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                    self.sync_space_with_wm(serial);
                }
                if !self.is_window_minimized(win_id)
                    && let Some(wm_win) = self.wm.window(win_id)
                    && wm_win.workspace == self.wm.active_workspace_id()
                {
                    let origin = self
                        .animations
                        .animated_window_origin(win_id.0)
                        .unwrap_or((wm_win.geometry.x, wm_win.geometry.y));
                    let titlebar_h = self.window_titlebar_height(win_id);
                    let geo_loc = window.geometry().loc;
                    let client_origin = (origin.0 - geo_loc.x, origin.1 + titlebar_h - geo_loc.y);
                    let is_focused = self.wm.focused_window() == Some(win_id);
                    self.space.map_element(window, client_origin, is_focused);
                    if is_focused && !self.screen_locked {
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
    }
}

impl RustyCompositorState {
    fn commit_layer_surface(&mut self, surface: &WlSurface) {
        let layer_work_area = {
            let mut layer_map = layer_map_for_output(&self.output);
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
                let non_exclusive = layer_map.non_exclusive_zone();
                Some(Rect::new(
                    non_exclusive.loc.x,
                    non_exclusive.loc.y,
                    non_exclusive.size.w,
                    non_exclusive.size.h,
                ))
            } else {
                None
            }
        };

        if let Some(wm_rect) = layer_work_area {
            let primary_geo = self.output_map.primary().logical_geometry();
            let full_area = Rect::new(
                primary_geo.loc.x,
                primary_geo.loc.y,
                primary_geo.size.w.max(100),
                primary_geo.size.h.max(100),
            );
            self.wm
                .upsert_output_with_full_area(PRIMARY_OUTPUT_ID, wm_rect, full_area);
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

        let window = Window::new_wayland_window(surface);
        self.window_bindings.push((win_id, window));
        self.register_foreign_toplevel(win_id);

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);

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
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.wm.focus_and_raise(win_id);
            self.sync_space_with_wm(serial);
            if let Some(wm_win) = self.wm.window(win_id) {
                self.active_window_resize = None;
                self.active_window_drag = Some(WindowDragState {
                    window_id: win_id,
                    start_pointer: self.pointer_location,
                    start_origin: (wm_win.geometry.x, wm_win.geometry.y),
                });
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
        let Some(mapped_edges) = map_xdg_resize_edge(edges) else {
            return;
        };
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
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
            self.wm.focus_and_raise(win_id);
            if !self.wm.window(win_id).is_some_and(|w| w.maximized) {
                let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
            }
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.wm.focus_and_raise(win_id);
            if self.wm.window(win_id).is_some_and(|w| w.maximized) {
                let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
            }
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<WlOutput>) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.wm.focus_and_raise(win_id);
            self.wm.set_window_fullscreen(win_id, true);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.wm.focus_and_raise(win_id);
            self.wm.set_window_fullscreen(win_id, false);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
        }
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.minimize_window(win_id, serial);
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
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
            self.sync_foreign_toplevel_metadata(win_id);
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(win_id) = self.window_id_for_toplevel(&surface) {
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
            self.window_bindings.retain(|(wid, _)| *wid != win_id);
            self.unregister_window_metadata(win_id);
            self.minimized_window_ids.remove(&win_id);
            self.remove_from_switcher(win_id);
            self.wm.unmap_window(win_id);
            self.animations.remove_window(win_id.0);
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
        let mut layer_map = layer_map_for_output(&self.output);
        let target_layer = layer_map
            .layers()
            .find(|l| l.layer_surface() == &surface)
            .cloned();

        if let Some(layer) = target_layer {
            layer_map.unmap_layer(&layer);
            layer_map.arrange();
            info!(namespace = %layer.namespace(), "Unmapped destroyed layer surface");
        }
    }
}

impl XdgDecorationHandler for RustyCompositorState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        self.apply_xdg_decoration_mode(&toplevel, false);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: DecorationMode) {
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
