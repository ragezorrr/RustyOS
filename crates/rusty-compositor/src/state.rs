//! Core Wayland compositor state and protocol handler implementations.

use std::os::unix::io::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusty_animation::{AnimationEngine, LogicalRect};
use rusty_ipc::{CompositorEvent, DesktopStateSnapshot, ShellRequest, WindowSummary};
use rusty_wm::{LayoutMode, OutputId, Rect, WindowId, WindowManager, WmAction, WorkspaceId};
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_layer_shell, delegate_output,
    delegate_seat, delegate_shm, delegate_xdg_shell,
    desktop::{
        LayerSurface as DesktopLayerSurface, PopupManager, Space, Window, WindowSurfaceType,
        layer_map_for_output,
    },
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
            protocol::{wl_buffer, wl_output::WlOutput, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{IsAlive, Logical, Point, Serial, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{self, CompositorClientState, CompositorHandler, CompositorState},
        output::{OutputHandler, OutputManagerState},
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
        },
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState},
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

/// Active mouse-driven window drag state via SSD titlebar (`RUSTYOS-019`).
#[derive(Debug, Clone, Copy)]
pub struct WindowDragState {
    pub window_id: WindowId,
    pub start_pointer: Point<f64, Logical>,
    pub start_origin: (i32, i32),
}

/// Primary state container for `rusty-compositor`.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
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
    /// `zwlr_layer_shell_v1` desktop panel/dock shell protocol state (`RUSTYOS-016`).
    pub wlr_layer_shell_state: WlrLayerShellState,
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
    /// Active interactive window drag via SSD titlebar (`RUSTYOS-019`).
    pub active_window_drag: Option<WindowDragState>,
    /// Low-overhead frame timing and pacing collector (`RUSTYOS-003`).
    pub frame_timing: FrameTimingCollector,
    /// Refresh-rate independent animation engine (`RUSTYOS-008`, ADR-0004).
    pub animations: AnimationEngine,
    /// Whether the workspace overview mode is active (`RUSTYOS-009`/`010`).
    pub overview_active: bool,
    /// Whether the Spotlight application launcher overlay is open (`RUSTYOS-018`).
    pub launcher_active: bool,
    /// Interactive Spotlight query, catalog, and selection state (`RUSTYOS-018b`).
    pub launcher: crate::launcher::LauncherState,
    /// Whether the Quick Settings Control Center popover is open (`RUSTYOS-021`).
    pub quick_settings_active: bool,
    /// Non-blocking system telemetry & quick audio controls (`RUSTYOS-021`).
    pub telemetry: crate::telemetry::SystemMetricsHandle,
    /// Whether the session lock screen is active (`RUSTYOS-009`/`010`).
    pub screen_locked: bool,
    /// Count of notifications received via shell IPC (`RUSTYOS-009`/`010`).
    pub notification_count: u32,
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
        let wlr_layer_shell_state = WlrLayerShellState::new::<Self>(&display_handle);
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

        let work_area = crate::shell_render::usable_work_area(initial_size);
        let wm = WindowManager::new(work_area);

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
            wlr_layer_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            seat,
            keyboard,
            pointer,
            pointer_location: (0.0, 0.0).into(),
            active_window_drag: None,
            frame_timing: FrameTimingCollector::new(DEFAULT_SAMPLE_CAPACITY, 60_000),
            animations: AnimationEngine::default(),
            overview_active: false,
            launcher_active: false,
            launcher: crate::launcher::LauncherState::new(),
            quick_settings_active: false,
            telemetry: crate::telemetry::SystemMetricsHandle::static_default(),
            screen_locked: false,
            notification_count: 0,
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

    /// Toggles the Spotlight application launcher overlay and resets query state.
    pub fn toggle_launcher(&mut self) {
        self.launcher_active = !self.launcher_active;
        if self.launcher_active {
            self.quick_settings_active = false;
        }
        self.launcher.reset();
    }

    /// Closes the Spotlight overlay and clears its active search query.
    pub fn close_launcher(&mut self) {
        self.launcher_active = false;
        self.launcher.reset();
    }

    /// Toggles the `TopBar` Quick Settings popover and triggers an async telemetry refresh when opened.
    pub fn toggle_quick_settings(&mut self) {
        self.quick_settings_active = !self.quick_settings_active;
        if self.quick_settings_active {
            self.close_launcher();
            self.telemetry.request_refresh();
        }
    }

    /// Closes the Quick Settings popover.
    pub fn close_quick_settings(&mut self) {
        self.quick_settings_active = false;
    }

    /// Launches the currently highlighted candidate from Spotlight and closes the overlay.
    pub fn launch_spotlight_selected(&mut self) {
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        let _ = self.launcher.launch_selected(&self.socket_name, &ipc_path);
        self.close_launcher();
    }

    /// Launches the Spotlight candidate at visible row offset `row_offset` and closes the overlay.
    pub fn launch_spotlight_row(&mut self, row_offset: usize) {
        if let Some(cmd) = self.launcher.exec_at_visible_row(3, row_offset) {
            let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
            let _ = self
                .launcher
                .launch_command(&cmd, &self.socket_name, &ipc_path);
        }
        self.close_launcher();
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

    /// Finds the topmost surface (layer shell or window) under the given logical point,
    /// respecting Wayland layer-shell stacking order (Overlay -> Top -> Windows -> Bottom -> Background).
    #[must_use]
    pub fn target_surface_under(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(Option<Window>, WlSurface, Point<f64, Logical>)> {
        let layer_map = layer_map_for_output(&self.output);

        // 1. Overlay & Top layers
        for layer in layer_map.layers().rev() {
            if matches!(layer.layer(), Layer::Overlay | Layer::Top)
                && let Some(geo) = layer_map.layer_geometry(layer)
                && geo.to_f64().contains(point)
            {
                let rel = point - geo.loc.to_f64();
                if let Some((surf, offset)) = layer.surface_under(rel, WindowSurfaceType::ALL) {
                    let origin = (geo.loc + offset).to_f64();
                    return Some((None, surf, origin));
                }
            }
        }

        // 2. Windows in desktop space
        if let Some((window, surface, origin)) = self.surface_under_pointer(point) {
            return Some((Some(window), surface, origin));
        }

        // 3. Bottom & Background layers
        for layer in layer_map.layers().rev() {
            if matches!(layer.layer(), Layer::Bottom | Layer::Background)
                && let Some(geo) = layer_map.layer_geometry(layer)
                && geo.to_f64().contains(point)
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
            if self
                .active_window_drag
                .is_some_and(|drag| drag.window_id == id)
            {
                self.active_window_drag = None;
            }
            self.window_bindings.retain(|(wid, _)| *wid != id);
            self.wm.unmap_window(id);
            self.animations.remove_window(id.0);
            info!(window_id = id.0, "Pruned closed Wayland window from WM");
        }
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
    }

    /// Applies a [`WmAction`] and synchronizes the compositor [`Space`], `xdg_toplevel` configure
    /// states, and keyboard focus.
    pub fn apply_wm_action(&mut self, action: WmAction, serial: Serial) {
        info!(?action, "Applying window manager action");
        let prev_ws = self.wm.active_workspace_id();
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

        if let WmAction::SwitchWorkspace(target_ws) = action
            && prev_ws != target_ws
        {
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
        let now = self.elapsed();
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
                let client_height = (rect.height - crate::shell_render::TITLEBAR_HEIGHT).max(64);
                let client_origin = (origin.0, origin.1 + crate::shell_render::TITLEBAR_HEIGHT);
                if let Some(toplevel) = window.toplevel() {
                    toplevel.with_pending_state(|state| {
                        state.size = Some((rect.width, client_height).into());
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
                self.space.map_element(window, client_origin, is_focused);
            }
        }

        // 3. Update Wayland keyboard focus to match the WM's focused window.
        let focus_surface = focused_id
            .and_then(|id| self.smithay_window_for_id(id))
            .and_then(|w| w.toplevel().map(|tl| tl.wl_surface().clone()));
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, focus_surface, serial);
    }

    /// Advances active window and workspace transitions to the current monotonic timestamp
    /// and updates window positions in [`Space<Window>`].
    pub fn tick_animations(&mut self) {
        if !self.animations.has_active_animations() {
            return;
        }
        let now = self.elapsed();
        let _ = self.animations.tick(now);

        let focused_id = self.wm.focused_window();
        let visible_ids: Vec<WindowId> = self
            .wm
            .visible_windows()
            .into_iter()
            .map(|w| w.id)
            .collect();
        for win_id in visible_ids {
            if let Some(origin) = self.animations.animated_window_origin(win_id.0)
                && let Some(window) = self.smithay_window_for_id(win_id)
            {
                let is_focused = focused_id == Some(win_id);
                let client_origin = (origin.0, origin.1 + crate::shell_render::TITLEBAR_HEIGHT);
                self.space.map_element(window, client_origin, is_focused);
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
                if self.wm.focus_and_raise(WindowId(window_id)) {
                    self.sync_space_with_wm(serial);
                }
            }
            ShellRequest::ToggleLayoutMode => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            ShellRequest::ToggleOverview => {
                self.overview_active = !self.overview_active;
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
                let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
                let _ = self
                    .launcher
                    .launch_command(&command, &self.socket_name, &ipc_path);
                self.close_launcher();
            }
            ShellRequest::SetReducedMotion { enabled } => {
                self.animations.set_reduced_motion(enabled);
                info!(reduced_motion = enabled, "Updated reduced-motion via IPC");
            }
            ShellRequest::SetLockScreen { locked } => {
                self.screen_locked = locked;
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
                self.notification_count = self.notification_count.saturating_add(1);
                info!(
                    urgency = urgency.as_str(),
                    title = %title,
                    body = %body,
                    count = self.notification_count,
                    "Received desktop notification via IPC"
                );
            }
        }
        CompositorEvent::StateSnapshot(self.build_ipc_snapshot())
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

        // Handle zwlr_layer_surface_v1 commits and arrange
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
            self.wm.upsert_output(OutputId(1), wm_rect);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.sync_space_with_wm(serial);
            return;
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

delegate_compositor!(RustyCompositorState);
delegate_shm!(RustyCompositorState);
delegate_output!(RustyCompositorState);
delegate_seat!(RustyCompositorState);
delegate_data_device!(RustyCompositorState);
delegate_xdg_shell!(RustyCompositorState);
delegate_layer_shell!(RustyCompositorState);
