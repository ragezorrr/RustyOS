//! Core Wayland compositor state and protocol handler implementations.

use std::os::unix::io::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusty_wm::{OutputId, Rect, WindowId, WindowManager, WmAction};
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_xdg_shell,
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{
        Seat, SeatHandler, SeatState,
        keyboard::{KeyboardHandle, XkbConfig},
        pointer::{CursorImageStatus, PointerHandle},
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::LoopSignal,
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{IsAlive, Logical, Point, Serial, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
        output::{OutputHandler, OutputManagerState},
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
    },
};
use tracing::{debug, info, warn};

use crate::{
    CompositorError,
    frame_timing::{DEFAULT_SAMPLE_CAPACITY, FrameTimingCollector},
};

/// Primary output identifier in the window manager.
pub const PRIMARY_OUTPUT_ID: OutputId = OutputId(1);

/// Per-client state stored inside `wayland-server`.
#[derive(Debug, Default)]
pub struct ClientState {
    /// Compositor-specific client state required by Smithay.
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, client_id: ClientId) {
        debug!(?client_id, "Wayland client initialized");
    }

    fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
        debug!(?client_id, ?reason, "Wayland client disconnected");
    }
}

/// Primary state container for `rusty-compositor`.
#[derive(Debug)]
pub struct RustyCompositorState {
    /// Monotonic start instant used for frame callback timestamps.
    pub start_time: Instant,
    /// Handle to register globals and clients on the Wayland display.
    pub display_handle: DisplayHandle,
    /// Wayland listening socket name (e.g. `wayland-2`).
    pub socket_name: String,
    /// Loop signal used to wake or stop the `calloop` event loop.
    pub loop_signal: LoopSignal,
    /// Flag indicating whether the compositor should continue running.
    pub running: bool,
    /// Deterministic window management policy and workspace state (`RUSTYOS-004`/`005`).
    pub wm: WindowManager,
    /// Mapping between `rusty-wm` [`WindowId`] and Smithay [`Window`] handles.
    pub window_bindings: Vec<(WindowId, Window)>,
    /// Desktop space managing mapped windows and outputs for the active workspace.
    pub space: Space<Window>,
    /// Popup manager tracking `xdg_popup` surfaces.
    pub popups: PopupManager,
    /// Active virtual or physical output.
    pub output: Output,
    /// Core `wl_compositor` protocol state.
    pub compositor_state: CompositorState,
    /// `xdg_wm_base` shell protocol state.
    pub xdg_shell_state: XdgShellState,
    /// `wl_shm` shared-memory buffer state.
    pub shm_state: ShmState,
    /// `wl_output` and `xdg_output` manager state.
    pub output_manager_state: OutputManagerState,
    /// `wl_seat` manager state.
    pub seat_state: SeatState<Self>,
    /// `wl_data_device` clipboard and drag-and-drop state.
    pub data_device_state: DataDeviceState,
    /// Primary input seat (`winit` in Phase 1).
    pub seat: Seat<Self>,
    /// Keyboard handle attached to the primary seat.
    pub keyboard: KeyboardHandle<Self>,
    /// Pointer handle attached to the primary seat.
    pub pointer: PointerHandle<Self>,
    /// Current logical pointer coordinates in the compositor space.
    pub pointer_location: Point<f64, Logical>,
    /// Low-overhead frame timing and pacing collector (`RUSTYOS-003`).
    pub frame_timing: FrameTimingCollector,
}

impl RustyCompositorState {
    /// Initializes the Wayland globals, seat, [`WindowManager`], and desktop [`Space`].
    ///
    /// # Errors
    ///
    /// Returns [`CompositorError::SeatInitialization`] if the XKB keyboard or pointer cannot
    /// be initialized on the seat.
    pub fn new(
        display: &Display<Self>,
        socket_name: String,
        loop_signal: LoopSignal,
        initial_size: Size<i32, smithay::utils::Physical>,
    ) -> Result<Self, CompositorError> {
        let display_handle = display.handle();

        let compositor_state = CompositorState::new::<Self>(&display_handle);
        let xdg_shell_state = XdgShellState::new::<Self>(&display_handle);
        let shm_state = ShmState::new::<Self>(&display_handle, Vec::new());
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&display_handle);
        let data_device_state = DataDeviceState::new::<Self>(&display_handle);

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&display_handle, "rusty-seat-0");

        let keyboard = seat
            .add_keyboard(XkbConfig::default(), 200, 25)
            .map_err(|err| CompositorError::SeatInitialization(err.to_string()))?;
        let pointer = seat.add_pointer();

        let output = Output::new(
            "winit-0".to_owned(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "RustyOS".to_owned(),
                model: "Nested Winit Output".to_owned(),
            },
        );
        let _output_global = output.create_global::<Self>(&display_handle);

        let mode = Mode {
            size: initial_size,
            refresh: 60_000,
        };
        output.change_current_state(
            Some(mode),
            Some(Transform::Flipped180),
            Some(Scale::Integer(1)),
            Some((0, 0).into()),
        );
        output.set_preferred(mode);

        let mut space = Space::default();
        space.map_output(&output, (0, 0));

        let wm = WindowManager::new(Rect::new(0, 0, initial_size.w, initial_size.h));

        Ok(Self {
            start_time: Instant::now(),
            display_handle,
            socket_name,
            loop_signal,
            running: true,
            wm,
            window_bindings: Vec::new(),
            space,
            popups: PopupManager::default(),
            output,
            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            seat,
            keyboard,
            pointer,
            pointer_location: (0.0, 0.0).into(),
            frame_timing: FrameTimingCollector::new(DEFAULT_SAMPLE_CAPACITY, 60_000),
        })
    }

    /// Requests a clean shutdown of the compositor event loop.
    pub fn request_shutdown(&mut self) {
        if self.running {
            info!("Shutdown requested; stopping compositor loop");
            self.running = false;
            self.loop_signal.stop();
        }
    }

    /// Returns the elapsed duration since compositor startup.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
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

    /// Removes dead/closed client windows from `window_bindings` and [`WindowManager`],
    /// then synchronizes [`Space`] if any window disappeared.
    pub fn prune_dead_windows(&mut self) {
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
            self.window_bindings.retain(|(wid, _)| *wid != id);
            self.wm.unmap_window(id);
            info!(window_id = id.0, "Pruned closed Wayland window from WM");
        }
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
    }

    /// Applies a [`WmAction`] and synchronizes the compositor [`Space`], `xdg_toplevel` configure
    /// states, and keyboard focus.
    pub fn apply_wm_action(&mut self, action: WmAction, serial: Serial) {
        info!(?action, "Applying window manager action");
        if let Some(close_id) = self.wm.apply_action(action) {
            if let Some(win) = self.smithay_window_for_id(close_id)
                && let Some(toplevel) = win.toplevel()
            {
                info!(
                    window_id = close_id.0,
                    "Sending xdg_toplevel::close to focused client"
                );
                toplevel.send_close();
            }
            return;
        }

        self.sync_space_with_wm(serial);
    }

    /// Synchronizes Smithay's [`Space<Window>`], `xdg_toplevel` pending configure geometry/states,
    /// and Wayland keyboard focus with the deterministic state in [`WindowManager`].
    pub fn sync_space_with_wm(&mut self, serial: Serial) {
        let visible: Vec<(WindowId, Rect, bool)> = self
            .wm
            .visible_windows()
            .into_iter()
            .map(|w| (w.id, w.geometry, w.maximized))
            .collect();
        let focused_id = self.wm.focused_window();

        // 1. Unmap windows that are no longer on an active workspace (e.g. moved/switched workspace).
        let current_space_windows: Vec<Window> = self.space.elements().cloned().collect();
        for sw in current_space_windows {
            let still_visible = self
                .window_id_for_smithay(&sw)
                .is_some_and(|id| visible.iter().any(|(vid, _, _)| *vid == id));
            if !still_visible {
                self.space.unmap_elem(&sw);
            }
        }

        // 2. Map/update visible windows in bottom-to-top z-order and send xdg_toplevel configures.
        for (win_id, rect, maximized) in visible {
            let is_focused = focused_id == Some(win_id);
            if let Some(window) = self.smithay_window_for_id(win_id) {
                if let Some(toplevel) = window.toplevel() {
                    toplevel.with_pending_state(|state| {
                        state.size = Some((rect.width, rect.height).into());
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
                    toplevel.send_pending_configure();
                }
                self.space.map_element(window, (rect.x, rect.y), is_focused);
            }
        }

        // 3. Update Wayland keyboard focus to match the WM's focused window.
        let focus_surface = focused_id
            .and_then(|id| self.smithay_window_for_id(id))
            .and_then(|w| w.toplevel().map(|tl| tl.wl_surface().clone()));
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, focus_surface, serial);
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
}

impl BufferHandler for RustyCompositorState {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl CompositorHandler for RustyCompositorState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(state) = client.get_data::<ClientState>() {
            &state.compositor_state
        } else {
            static DEFAULT_CLIENT_STATE: std::sync::LazyLock<CompositorClientState> =
                std::sync::LazyLock::new(CompositorClientState::default);
            &DEFAULT_CLIENT_STATE
        }
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);

        if let Some(window) = self.window_bindings.iter().find_map(|(_, w)| {
            w.toplevel()
                .is_some_and(|tl| tl.wl_surface() == surface)
                .then(|| w.clone())
        }) {
            window.on_commit();
        }

        self.popups.commit(surface);
    }
}

impl ShmHandler for RustyCompositorState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl OutputHandler for RustyCompositorState {}

impl SeatHandler for RustyCompositorState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, focused: Option<&WlSurface>) {
        debug!(has_focus = focused.is_some(), "Keyboard focus changed");
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}
}

impl SelectionHandler for RustyCompositorState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for RustyCompositorState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for RustyCompositorState {}

impl ServerDndGrabHandler for RustyCompositorState {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl XdgShellHandler for RustyCompositorState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let win_id = self.wm.next_window_id();
        let rect = self.wm.map_window(win_id, None);

        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
            state.size = Some((rect.width, rect.height).into());
        });
        surface.send_configure();

        let window = Window::new_wayland_window(surface);
        self.window_bindings.push((win_id, window));

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);

        info!(
            window_id = win_id.0,
            workspace = self.wm.active_workspace_id().0,
            ?rect,
            "Mapped new xdg_toplevel window via WindowManager"
        );
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        if let Err(err) = self
            .popups
            .track_popup(smithay::desktop::PopupKind::from(surface))
        {
            warn!(error = %err, "Failed to track xdg_popup");
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }
}

delegate_compositor!(RustyCompositorState);
delegate_shm!(RustyCompositorState);
delegate_output!(RustyCompositorState);
delegate_seat!(RustyCompositorState);
delegate_data_device!(RustyCompositorState);
delegate_xdg_shell!(RustyCompositorState);
