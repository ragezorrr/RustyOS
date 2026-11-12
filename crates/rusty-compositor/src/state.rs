//! Core Wayland compositor state and protocol handler implementations.

use std::collections::HashSet;
use std::os::unix::io::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusty_animation::{AnimatedValue, AnimationEngine, Easing, LogicalRect, MotionTokens};
use rusty_ipc::{
    CompositorEvent, DesktopStateSnapshot, NotificationSummary, ShellRequest, Urgency,
    WindowSummary,
};
use rusty_wm::{LayoutMode, OutputId, Rect, WindowId, WindowManager, WmAction, WorkspaceId};
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_control, delegate_data_device, delegate_layer_shell,
    delegate_output, delegate_primary_selection, delegate_seat, delegate_shm,
    delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{
        LayerSurface as DesktopLayerSurface, PopupKind, PopupManager, Space, Window,
        WindowSurfaceType, find_popup_root_surface, get_popup_toplevel_coords,
        layer_map_for_output,
    },
    input::{
        Seat, SeatHandler, SeatState,
        keyboard::{KeyboardHandle, XkbConfig},
        pointer::{CursorImageStatus, PointerHandle},
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::{LoopHandle, LoopSignal},
        wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{
                wl_buffer, wl_data_source::WlDataSource, wl_output::WlOutput, wl_seat,
                wl_surface::WlSurface,
            },
        },
    },
    utils::{IsAlive, Logical, Point, Rectangle, Serial, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            self, CompositorClientState, CompositorHandler, CompositorState, TraversalAction,
        },
        output::{OutputHandler, OutputManagerState},
        seat::WaylandFocus,
        selection::{
            SelectionHandler, SelectionSource, SelectionTarget,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                set_data_device_focus,
            },
            primary_selection::{
                PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
            },
            wlr_data_control::{DataControlHandler, DataControlState},
        },
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData,
                decoration::{XdgDecorationHandler, XdgDecorationState},
            },
        },
        shm::{ShmHandler, ShmState},
        xwayland_shell::XWaylandShellState,
    },
    xwayland::{X11Surface, X11Wm, XWaylandClientData},
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

/// Active mouse-driven window resize state via SSD borders/corners (`RUSTYOS-022`).
#[derive(Debug, Clone, Copy)]
pub struct WindowResizeState {
    pub window_id: WindowId,
    pub edges: crate::shell_render::ResizeEdges,
    pub start_pointer: Point<f64, Logical>,
    pub start_rect: Rect,
}

/// Active `Alt+Tab` window switcher HUD state (`RUSTYOS-033`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSwitcherState {
    /// Active workspace window IDs in presentation order (unminimized MRU first, then minimized).
    pub window_ids: Vec<WindowId>,
    /// Currently highlighted entry index inside `window_ids`.
    pub selected: usize,
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
    /// Managed windows that bound `zxdg_toplevel_decoration_v1` (`RUSTYOS-030`).
    pub xdg_decoration_requested: HashSet<WindowId>,
    /// Managed windows using client-side decorations (CSD) without RustyOS SSD (`RUSTYOS-030`).
    pub csd_window_ids: HashSet<WindowId>,
    /// Managed windows currently minimized to the Dock (`RUSTYOS-032`).
    pub minimized_window_ids: HashSet<WindowId>,
    /// Desktop space managing mapped windows and outputs for the active workspace.
    pub space: Space<Window>,
    /// Popup manager tracking `xdg_popup` surfaces.
    pub popups: PopupManager,
    /// Grabbed `xdg_popup` surfaces awaiting outside-click or Escape dismissal (`RUSTYOS-034`).
    pub active_popup_grabs: Vec<PopupSurface>,
    /// Active cursor image state requested by focused Wayland clients (`RUSTYOS-034`).
    pub cursor_status: CursorImageStatus,
    /// Active virtual or physical primary output.
    pub output: Output,
    /// Multi-monitor coordinate layout and output registry (`RUSTYOS-040`).
    pub output_map: crate::outputs::OutputMap,
    /// Core `wl_compositor` protocol state.
    pub compositor_state: CompositorState,
    /// `xdg_wm_base` shell protocol state.
    pub xdg_shell_state: XdgShellState,
    /// `zxdg_decoration_manager_v1` SSD negotiation state.
    pub xdg_decoration_state: XdgDecorationState,
    /// `zwlr_layer_shell_v1` desktop panel/dock shell protocol state (`RUSTYOS-016`).
    pub wlr_layer_shell_state: WlrLayerShellState,
    /// `xwayland_shell_v1` surface-pairing protocol state (`RUSTYOS-026`).
    pub xwayland_shell_state: XWaylandShellState,
    /// Active `X11Wm` instance once `XWayland` has reported ready (`RUSTYOS-026`).
    pub xwm: Option<X11Wm>,
    /// Event loop handle used for `X11Wm` clipboard transfers (`RUSTYOS-029`).
    pub x11_loop_handle: Option<LoopHandle<'static, Self>>,
    /// Active `XWayland` display number (`:N`) if attached (`RUSTYOS-026`).
    pub xwayland_display: Option<u32>,
    /// Persistent user configuration loaded from `~/.config/rustyos/config.toml` (`RUSTYOS-029`).
    pub user_config: crate::config::UserConfig,
    /// Short active XKB layout label (`"EN"`, `"RU"`, etc.) displayed in `TopBar` (`RUSTYOS-029`).
    pub active_kbd_layout: String,
    /// `wl_shm` shared-memory buffer state.
    pub shm_state: ShmState,
    /// `wl_output` and `xdg_output` manager state.
    pub output_manager_state: OutputManagerState,
    /// `wl_seat` manager state.
    pub seat_state: SeatState<Self>,
    /// `wl_data_device` clipboard and drag-and-drop state.
    pub data_device_state: DataDeviceState,
    /// `zwp_primary_selection_v1` middle-click / terminal selection state.
    pub primary_selection_state: PrimarySelectionState,
    /// `zwlr_data_control_manager_v1` CLI/manager clipboard access state.
    pub data_control_state: DataControlState,
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
    /// Active interactive window resize via SSD borders/corners (`RUSTYOS-022`).
    pub active_window_resize: Option<WindowResizeState>,
    /// Low-overhead frame timing and pacing collector (`RUSTYOS-003`).
    pub frame_timing: FrameTimingCollector,
    /// Refresh-rate independent animation engine (`RUSTYOS-008`, ADR-0004).
    pub animations: AnimationEngine,
    /// Whether the workspace overview mode is active (`RUSTYOS-009`/`010`/`023`).
    pub overview_active: bool,
    /// Animated Overview transition progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub overview_anim: AnimatedValue,
    /// Currently hovered Dock slot index (`0..3 + windows.len()`) (`RUSTYOS-023`).
    pub dock_hover_slot: Option<usize>,
    /// Whether the Spotlight application launcher overlay is open (`RUSTYOS-018`).
    pub launcher_active: bool,
    /// Animated Spotlight transition progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub launcher_anim: AnimatedValue,
    /// Interactive Spotlight query, catalog, and selection state (`RUSTYOS-018b`).
    pub launcher: crate::launcher::LauncherState,
    /// Whether the Quick Settings Control Center popover is open (`RUSTYOS-021`).
    pub quick_settings_active: bool,
    /// Animated Quick Settings transition progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub quick_settings_anim: AnimatedValue,
    /// Non-blocking system telemetry & quick audio controls (`RUSTYOS-021`).
    pub telemetry: crate::telemetry::SystemMetricsHandle,
    /// Whether the session lock screen is active (`RUSTYOS-009`/`010`/`025`).
    pub screen_locked: bool,
    /// Animated Lock Screen transition progress (`0.0` unlocked -> `1.0` locked) (`RUSTYOS-025`).
    pub lock_screen_anim: AnimatedValue,
    /// Count of active notifications held in compositor HUD queue (`RUSTYOS-009`/`010`/`024`).
    pub notification_count: u32,
    /// Active desktop notification queue rendered in Notification HUD & exposed via IPC (`RUSTYOS-024`).
    pub notifications: Vec<CompositorNotification>,
    /// Non-blocking D-Bus `org.freedesktop.Notifications` bridge (`RUSTYOS-026`).
    pub dbus_notifications: crate::dbus_notifications::DbusNotificationHandle,
    /// Non-blocking D-Bus `org.kde.StatusNotifierWatcher` system tray bridge (`RUSTYOS-032`).
    pub tray: crate::tray::TrayWatcherHandle,
    /// Active system tray items rendered in `TopBar` (`RUSTYOS-032`).
    pub tray_items: Vec<crate::tray::TrayItemView>,
    /// Transient On-Screen Display (OSD) indicator banner (`RUSTYOS-031`).
    pub osd: Option<OsdBanner>,
    /// Queued on-demand screenshot capture mode executed on the next render pass (`RUSTYOS-031`).
    pub pending_screenshot: Option<crate::screenshot::ScreenshotRequest>,
    /// Async background PNG/clipboard completion queue (`RUSTYOS-031`).
    pub screenshot_results: crate::screenshot::ScreenshotResultQueue,
    /// Pending TTY virtual terminal switch (`Ctrl+Alt+F1..F12` -> `libseat::change_vt`).
    pub pending_vt_switch: Option<i32>,
    /// Active `Alt+Tab` window switcher HUD state (`RUSTYOS-033`).
    pub window_switcher: Option<WindowSwitcherState>,
    /// Active client Drag-and-Drop icon surface attached during `wl_data_device.start_drag` (`RUSTYOS-033`).
    pub dnd_icon: Option<WlSurface>,
    next_notification_id: u64,
}

/// Active transient On-Screen Display (OSD) banner (`RUSTYOS-031`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsdBanner {
    /// Short OSD title (`"Volume"`, `"Brightness"`, `"Play / Pause"`, etc.).
    pub label: String,
    /// Optional bar percentage (`0..=100`).
    pub value_percent: Option<u8>,
    /// True when audio is muted.
    pub muted: bool,
    /// Monotonic expiry offset from `start_time`.
    pub expires_at: Duration,
}

/// Active notification banner tracked by the compositor (`RUSTYOS-024`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositorNotification {
    /// Unique monotonic identifier.
    pub id: u64,
    /// Optional freedesktop D-Bus notification ID for `replaces_id` / `CloseNotification`.
    pub dbus_id: Option<u32>,
    /// Notification urgency level.
    pub urgency: Urgency,
    /// Short notification title.
    pub title: String,
    /// Notification body text.
    pub body: String,
    /// Monotonic creation offset from compositor startup (`start_time.elapsed()`).
    pub created_at: Duration,
    /// Auto-dismiss time-to-live.
    pub ttl: Duration,
}

impl RustyCompositorState {
    /// Initializes the Wayland globals, seat, [`WindowManager`], and desktop [`Space`].
    ///
    /// # Errors
    ///
    /// Returns [`CompositorError::SeatInitialization`] if the XKB keyboard or pointer cannot
    /// be initialized on the seat.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        display: &Display<Self>,
        socket_name: String,
        loop_signal: LoopSignal,
        initial_size: Size<i32, smithay::utils::Physical>,
    ) -> Result<Self, CompositorError> {
        let display_handle = display.handle();

        let compositor_state = CompositorState::new::<Self>(&display_handle);
        let xdg_shell_state = XdgShellState::new::<Self>(&display_handle);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&display_handle);
        let wlr_layer_shell_state = WlrLayerShellState::new::<Self>(&display_handle);
        let xwayland_shell_state = XWaylandShellState::new::<Self>(&display_handle);
        let shm_state = ShmState::new::<Self>(&display_handle, Vec::new());
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&display_handle);
        let data_device_state = DataDeviceState::new::<Self>(&display_handle);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&display_handle);
        let data_control_state = DataControlState::new::<Self, _>(
            &display_handle,
            Some(&primary_selection_state),
            |_| true,
        );

        let user_config = crate::config::UserConfig::load_default();
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&display_handle, "rusty-seat-0");

        let keyboard = Self::init_seat_keyboard(&mut seat, &user_config)?;
        let pointer = seat.add_pointer();

        let output = Self::create_initial_output(&display_handle, initial_size);

        let mut space = Space::default();
        space.map_output(&output, (0, 0));

        let work_area = crate::shell_render::usable_work_area(initial_size);
        let wm = WindowManager::new(work_area);

        let primary_entry = crate::outputs::OutputEntry::new(
            PRIMARY_OUTPUT_ID,
            "winit-0",
            output.clone(),
            (0, 0).into(),
            initial_size,
            1.0,
            60_000,
            true,
        );
        let output_map = crate::outputs::OutputMap::new(primary_entry);

        let mut state = Self {
            start_time: Instant::now(),
            display_handle,
            socket_name,
            loop_signal,
            running: true,
            wm,
            window_bindings: Vec::new(),
            xdg_decoration_requested: HashSet::new(),
            csd_window_ids: HashSet::new(),
            minimized_window_ids: HashSet::new(),
            space,
            popups: PopupManager::default(),
            active_popup_grabs: Vec::new(),
            cursor_status: CursorImageStatus::default_named(),
            output,
            output_map,
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            wlr_layer_shell_state,
            xwayland_shell_state,
            xwm: None,
            x11_loop_handle: None,
            xwayland_display: None,
            user_config,
            active_kbd_layout: "EN".to_owned(),
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            primary_selection_state,
            data_control_state,
            seat,
            keyboard,
            pointer,
            pointer_location: (0.0, 0.0).into(),
            active_window_drag: None,
            active_window_resize: None,
            frame_timing: FrameTimingCollector::new(DEFAULT_SAMPLE_CAPACITY, 60_000),
            animations: AnimationEngine::default(),
            overview_active: false,
            overview_anim: AnimatedValue::new_settled(0.0),
            dock_hover_slot: None,
            launcher_active: false,
            launcher_anim: AnimatedValue::new_settled(0.0),
            launcher: crate::launcher::LauncherState::new(),
            quick_settings_active: false,
            quick_settings_anim: AnimatedValue::new_settled(0.0),
            telemetry: crate::telemetry::SystemMetricsHandle::static_default(),
            screen_locked: false,
            lock_screen_anim: AnimatedValue::new_settled(0.0),
            notification_count: 0,
            notifications: Vec::new(),
            dbus_notifications: crate::dbus_notifications::DbusNotificationHandle::default(),
            tray: crate::tray::TrayWatcherHandle::default(),
            tray_items: Vec::new(),
            osd: None,
            pending_screenshot: None,
            screenshot_results: crate::screenshot::ScreenshotResultQueue::default(),
            pending_vt_switch: None,
            window_switcher: None,
            dnd_icon: None,
            next_notification_id: 0,
        };
        state
            .animations
            .set_reduced_motion(state.user_config.reduced_motion);
        state.refresh_keyboard_layout_label();
        Ok(state)
    }

    fn create_initial_output(
        display_handle: &DisplayHandle,
        initial_size: Size<i32, smithay::utils::Physical>,
    ) -> Output {
        let output = Output::new(
            "winit-0".to_owned(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "RustyOS".to_owned(),
                model: "Nested Winit Output".to_owned(),
            },
        );
        let _output_global = output.create_global::<Self>(display_handle);

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
        output
    }

    /// Dynamically registers and maps a new display output into the compositor workspace (`RUSTYOS-040`).
    #[allow(clippy::cast_possible_truncation)]
    pub fn add_output(
        &mut self,
        name: impl Into<String>,
        physical_props: PhysicalProperties,
        mode: Mode,
        scale: Scale,
        location: Point<i32, Logical>,
    ) -> OutputId {
        let name_str = name.into();
        let new_id = self.output_map.next_output_id();
        let output = Output::new(name_str.clone(), physical_props);
        let _output_global = output.create_global::<Self>(&self.display_handle);
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            Some(scale),
            Some(location),
        );
        output.set_preferred(mode);

        self.space.map_output(&output, location);

        let scale_val = match scale {
            Scale::Fractional(f) => f,
            Scale::Integer(i) => f64::from(i),
            Scale::Custom { .. } => 1.0,
        };
        let logical_w = (f64::from(mode.size.w) / scale_val.max(0.1)).round() as i32;
        let logical_h = (f64::from(mode.size.h) / scale_val.max(0.1)).round() as i32;

        let work_area = Rect::new(location.x, location.y, logical_w.max(1), logical_h.max(1));
        self.wm.upsert_output(new_id, work_area);

        let entry = crate::outputs::OutputEntry::new(
            new_id,
            name_str,
            output,
            location,
            mode.size,
            scale_val,
            mode.refresh,
            false,
        );
        self.output_map.add(entry);

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
        new_id
    }

    /// Dynamically unmaps and removes a display output (`RUSTYOS-040`).
    ///
    /// Preserves desktop invariants by migrating any windows on the removed output
    /// to the primary output and designating a new primary if needed.
    pub fn remove_output(&mut self, id: OutputId) -> bool {
        let Some(removed) = self.output_map.remove(id) else {
            return false;
        };

        self.space.unmap_output(&removed.output);
        self.wm.remove_output(id);

        if removed.is_primary {
            self.output = self.output_map.primary().output.clone();
        }

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
        true
    }

    /// Computes the total bounding rectangle enclosing all connected outputs (`RUSTYOS-040`).
    #[must_use]
    pub fn total_output_bounds(&self) -> Rectangle<i32, Logical> {
        self.output_map.total_bounds()
    }

    /// Returns the [`OutputId`] under the given logical coordinate, if any (`RUSTYOS-040`).
    #[must_use]
    pub fn output_under(&self, point: Point<f64, Logical>) -> Option<OutputId> {
        self.output_map.output_under(point)
    }

    /// Moves the focused window to the next connected output and synchronizes space (`RUSTYOS-040`).
    pub fn move_focused_to_next_output(&mut self, serial: Serial) -> Option<WindowId> {
        let moved = self.wm.move_focused_to_next_output()?;
        self.sync_space_with_wm(serial);
        Some(moved)
    }

    fn init_seat_keyboard(
        seat: &mut Seat<Self>,
        user_config: &crate::config::UserConfig,
    ) -> Result<KeyboardHandle<Self>, CompositorError> {
        match seat.add_keyboard(
            user_config.xkb_config(),
            user_config.repeat_delay,
            user_config.repeat_rate,
        ) {
            Ok(kb) => Ok(kb),
            Err(err) => {
                warn!(
                    layout = %user_config.keyboard_layout,
                    error = %err,
                    "Configured XKB keymap failed to compile; falling back to US default"
                );
                seat.add_keyboard(XkbConfig::default(), 200, 25)
                    .map_err(|fallback_err| {
                        CompositorError::SeatInitialization(fallback_err.to_string())
                    })
            }
        }
    }

    /// Refreshes the compact `TopBar` keyboard layout badge (`"EN"`, `"RU"`, etc.) from XKB state.
    pub fn refresh_keyboard_layout_label(&mut self) {
        let keyboard = self.keyboard.clone();
        let configured = self.user_config.keyboard_layout.clone();
        let label = keyboard.with_xkb_state(self, |ctx| {
            let Ok(xkb) = ctx.xkb().lock() else {
                return "EN".to_owned();
            };
            let active = xkb.active_layout();
            let name = xkb.layout_name(active).to_owned();
            let layout_idx = usize::try_from(active.0).unwrap_or(0);
            drop(xkb);
            crate::config::layout_short_code(&name, layout_idx, &configured)
        });
        self.active_kbd_layout = label;
    }

    /// Cycles forward to the next configured XKB layout (`us` ↔ `ru`, etc.) and updates `TopBar`.
    pub fn cycle_keyboard_layout(&mut self) {
        let keyboard = self.keyboard.clone();
        keyboard.with_xkb_state(self, |mut ctx| {
            ctx.cycle_next_layout();
        });
        self.refresh_keyboard_layout_label();
        info!(layout = %self.active_kbd_layout, "Switched active keyboard layout");
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
            self.window_switcher = None;
            self.close_quick_settings();
            self.close_overview();
        }
        self.launcher.reset();
        self.retarget_launcher_anim();
    }

    /// Closes the Spotlight overlay and clears its active search query.
    pub fn close_launcher(&mut self) {
        self.launcher_active = false;
        self.launcher.reset();
        self.retarget_launcher_anim();
    }

    /// Toggles the `TopBar` Quick Settings popover and triggers an async telemetry refresh when opened.
    pub fn toggle_quick_settings(&mut self) {
        self.quick_settings_active = !self.quick_settings_active;
        if self.quick_settings_active {
            self.window_switcher = None;
            self.close_launcher();
            self.close_overview();
            self.telemetry.request_refresh();
        }
        self.retarget_quick_settings_anim();
    }

    /// Closes the Quick Settings popover.
    pub fn close_quick_settings(&mut self) {
        self.quick_settings_active = false;
        self.retarget_quick_settings_anim();
    }

    /// Engages or disengages the interactive session Lock Screen with smooth transition (`RUSTYOS-025`).
    pub fn set_screen_locked(&mut self, locked: bool, serial: Serial) {
        self.screen_locked = locked;
        if locked {
            self.window_switcher = None;
            self.dnd_icon = None;
            self.close_launcher();
            self.close_quick_settings();
            self.close_overview();
            self.active_window_drag = None;
            self.active_window_resize = None;
            self.dock_hover_slot = None;
            let keyboard = self.keyboard.clone();
            keyboard.set_focus(self, None, serial);
        } else {
            self.sync_space_with_wm(serial);
        }
        let now = self.elapsed();
        let tokens = MotionTokens::default();
        self.lock_screen_anim.retarget(
            if locked { 1.0 } else { 0.0 },
            now,
            tokens.standard_duration,
            Easing::EaseOutQuart,
            self.animations.reduced_motion(),
        );
    }

    /// Updates accessibility `reduced_motion` and snaps in-flight shell transitions if enabled (`RUSTYOS-025`).
    pub fn set_reduced_motion_pref(&mut self, enabled: bool) {
        self.animations.set_reduced_motion(enabled);
        if enabled {
            self.overview_anim.snap_to_target();
            self.launcher_anim.snap_to_target();
            self.quick_settings_anim.snap_to_target();
            self.lock_screen_anim.snap_to_target();
        }
    }

    fn retarget_launcher_anim(&mut self) {
        let now = self.elapsed();
        let tokens = MotionTokens::default();
        self.launcher_anim.retarget(
            if self.launcher_active { 1.0 } else { 0.0 },
            now,
            tokens.short_duration,
            Easing::EaseOutQuart,
            self.animations.reduced_motion(),
        );
    }

    fn retarget_quick_settings_anim(&mut self) {
        let now = self.elapsed();
        let tokens = MotionTokens::default();
        self.quick_settings_anim.retarget(
            if self.quick_settings_active { 1.0 } else { 0.0 },
            now,
            tokens.short_duration,
            Easing::EaseOutCubic,
            self.animations.reduced_motion(),
        );
    }

    fn retarget_overview_anim(&mut self) {
        let now = self.elapsed();
        let tokens = MotionTokens::default();
        self.overview_anim.retarget(
            if self.overview_active { 1.0 } else { 0.0 },
            now,
            tokens.long_duration,
            Easing::EaseOutCubic,
            self.animations.reduced_motion(),
        );
    }

    /// Pushes a notification into the compositor HUD queue using current monotonic uptime (`RUSTYOS-024`).
    pub fn push_notification(&mut self, urgency: Urgency, title: String, body: String) -> u64 {
        let now = self.start_time.elapsed();
        self.push_notification_at(urgency, title, body, now)
    }

    /// Pushes a notification at explicit monotonic timestamp `now` (`RUSTYOS-024`).
    pub fn push_notification_at(
        &mut self,
        urgency: Urgency,
        title: String,
        body: String,
        now: Duration,
    ) -> u64 {
        const MAX_QUEUE_LEN: usize = 16;
        self.next_notification_id = self.next_notification_id.saturating_add(1);
        let ttl = match urgency {
            Urgency::Low => Duration::from_secs(4),
            Urgency::Normal => Duration::from_secs(6),
            Urgency::Critical => Duration::from_secs(12),
        };
        let id = self.next_notification_id;
        self.notifications.push(CompositorNotification {
            id,
            dbus_id: None,
            urgency,
            title,
            body,
            created_at: now,
            ttl,
        });
        if self.notifications.len() > MAX_QUEUE_LEN {
            let excess = self.notifications.len() - MAX_QUEUE_LEN;
            self.notifications.drain(0..excess);
        }
        self.sync_notification_count();
        id
    }

    /// Updates the active `XWayland` `DISPLAY` number (`:N`) on state and launcher (`RUSTYOS-026`).
    pub fn set_xwayland_display(&mut self, display_number: Option<u32>) {
        self.xwayland_display = display_number;
        self.launcher.x11_display = display_number.map(|num| format!(":{num}"));
    }

    /// Inserts or replaces a notification received from the D-Bus session server (`RUSTYOS-026`).
    pub fn push_dbus_notification(
        &mut self,
        dbus_id: u32,
        replaces_id: u32,
        urgency: Urgency,
        title: String,
        body: String,
        expire_after: Option<Duration>,
    ) {
        const MAX_QUEUE_LEN: usize = 12;
        let now = self.start_time.elapsed();
        let default_ttl = match urgency {
            Urgency::Low => Duration::from_secs(4),
            Urgency::Normal => Duration::from_secs(6),
            Urgency::Critical => Duration::from_secs(12),
        };
        let ttl = expire_after.unwrap_or(default_ttl);

        if replaces_id > 0
            && let Some(existing) = self
                .notifications
                .iter_mut()
                .find(|n| n.dbus_id == Some(replaces_id))
        {
            existing.dbus_id = Some(dbus_id);
            existing.urgency = urgency;
            existing.title = title;
            existing.body = body;
            existing.created_at = now;
            existing.ttl = ttl;
            self.sync_notification_count();
            return;
        }

        self.next_notification_id = self.next_notification_id.saturating_add(1);
        let id = self.next_notification_id;
        self.notifications.push(CompositorNotification {
            id,
            dbus_id: Some(dbus_id),
            urgency,
            title,
            body,
            created_at: now,
            ttl,
        });
        if self.notifications.len() > MAX_QUEUE_LEN {
            let excess = self.notifications.len() - MAX_QUEUE_LEN;
            self.notifications.drain(0..excess);
        }
        self.sync_notification_count();
    }

    /// Non-blockingly drains queued notifications from the D-Bus background thread (`RUSTYOS-026`).
    pub fn drain_dbus_notifications(&mut self) {
        for cmd in self.dbus_notifications.drain_commands() {
            match cmd {
                crate::dbus_notifications::DbusNotificationCommand::Notify {
                    id,
                    replaces_id,
                    urgency,
                    title,
                    body,
                    expire_after,
                } => {
                    self.push_dbus_notification(
                        id,
                        replaces_id,
                        urgency,
                        title,
                        body,
                        expire_after,
                    );
                }
                crate::dbus_notifications::DbusNotificationCommand::Close { id } => {
                    self.notifications.retain(|n| n.dbus_id != Some(id));
                    self.sync_notification_count();
                }
            }
        }
    }

    /// Non-blockingly drains system tray updates from the background SNI watcher (`RUSTYOS-032`).
    pub fn drain_tray_updates(&mut self) {
        if let Some(items) = self.tray.drain_latest() {
            self.tray_items = items;
        }
    }

    /// Returns `true` if `win_id` is currently minimized to the Dock (`RUSTYOS-032`).
    #[must_use]
    pub fn is_window_minimized(&self, win_id: WindowId) -> bool {
        self.minimized_window_ids.contains(&win_id)
    }

    /// Minimizes `win_id` to the Dock, shifting focus to the next topmost visible window (`RUSTYOS-032`).
    pub fn minimize_window(&mut self, win_id: WindowId, serial: Serial) {
        if self.wm.window(win_id).is_none() {
            return;
        }
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
    }

    /// Restores a minimized window from the Dock, focusing and raising it (`RUSTYOS-032`).
    pub fn restore_window(&mut self, win_id: WindowId, serial: Serial) {
        if self.wm.window(win_id).is_none() {
            return;
        }
        self.minimized_window_ids.remove(&win_id);
        self.wm.focus_and_raise(win_id);
        self.sync_space_with_wm(serial);
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

    /// Removes notifications whose TTL has elapsed as of current uptime (`RUSTYOS-024`).
    pub fn expire_notifications(&mut self) {
        let now = self.start_time.elapsed();
        self.expire_notifications_at(now);
    }

    /// Removes notifications and transient OSD banners whose TTL has elapsed at `now` (`RUSTYOS-024`/`031`).
    pub fn expire_notifications_at(&mut self, now: Duration) {
        self.notifications
            .retain(|n| now.saturating_sub(n.created_at) < n.ttl);
        self.sync_notification_count();
        if self.osd.as_ref().is_some_and(|osd| now >= osd.expires_at) {
            self.osd = None;
        }
    }

    /// Displays a transient glass OSD indicator pill above the Dock (`RUSTYOS-031`).
    pub fn show_osd(&mut self, label: impl Into<String>, value_percent: Option<u8>, muted: bool) {
        let expires_at = self.start_time.elapsed() + Duration::from_millis(1600);
        self.osd = Some(OsdBanner {
            label: label.into(),
            value_percent,
            muted,
            expires_at,
        });
    }

    /// Polls background screenshot encoding/saving results without blocking (`RUSTYOS-031`).
    pub fn drain_screenshot_results(&mut self) {
        while let Some(res) = self.screenshot_results.try_pop() {
            match res {
                Ok(path) => {
                    let filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("screenshot.png")
                        .to_owned();
                    let _ = self.push_notification(
                        Urgency::Low,
                        "Screenshot Saved".to_owned(),
                        format!("{filename} (copied to clipboard)"),
                    );
                    self.show_osd("Screenshot Saved", None, false);
                }
                Err(err) => {
                    warn!(error = %err, "Failed to save screenshot");
                    let _ = self.push_notification(
                        Urgency::Critical,
                        "Screenshot Failed".to_owned(),
                        err,
                    );
                    self.show_osd("Screenshot Failed", None, false);
                }
            }
        }
    }

    /// Dismisses a single active notification by ID (`RUSTYOS-024`).
    pub fn dismiss_notification(&mut self, id: u64) -> bool {
        let prev_len = self.notifications.len();
        self.notifications.retain(|n| n.id != id);
        self.sync_notification_count();
        self.notifications.len() < prev_len
    }

    /// Clears all active notifications from the HUD queue (`RUSTYOS-024`).
    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
        self.sync_notification_count();
    }

    /// Pushes a live workspace/telemetry status notification (`Alt+N`, `RUSTYOS-024`).
    pub fn trigger_demo_notification(&mut self) -> u64 {
        let next_idx = self.next_notification_id % 3;
        let urgency = match next_idx {
            0 => Urgency::Normal,
            1 => Urgency::Low,
            _ => Urgency::Critical,
        };
        let ws = self.wm.active_workspace_id().0;
        let win_count = self.wm.visible_windows().len();
        let tel = self.telemetry.snapshot();
        let (title, body) = match urgency {
            Urgency::Normal => (
                format!("Workspace {ws} Status"),
                format!("{win_count} windows • {}", tel.network.status_label),
            ),
            Urgency::Low => (
                "System Telemetry".to_owned(),
                format!(
                    "Vol {}%{} • {}",
                    tel.audio.volume_percent,
                    if tel.audio.muted { " (Muted)" } else { "" },
                    tel.clock_hhmm
                ),
            ),
            Urgency::Critical => (
                "Attention Required".to_owned(),
                "Click toast or Alt+Shift+N to dismiss".to_owned(),
            ),
        };
        self.push_notification(urgency, title, body)
    }

    fn sync_notification_count(&mut self) {
        self.notification_count = u32::try_from(self.notifications.len()).unwrap_or(u32::MAX);
    }

    /// Toggles Workspace Overview / Expose mode (`RUSTYOS-023`).
    pub fn toggle_overview(&mut self) {
        self.overview_active = !self.overview_active;
        if self.overview_active {
            self.close_launcher();
            self.close_quick_settings();
            self.active_window_drag = None;
            self.active_window_resize = None;
        }
        self.retarget_overview_anim();
    }

    /// Exits Workspace Overview mode (`RUSTYOS-023`).
    pub fn close_overview(&mut self) {
        self.overview_active = false;
        self.retarget_overview_anim();
    }

    /// Returns visible window IDs on the active workspace in stable ascending `WindowId` order (`RUSTYOS-023`).
    #[must_use]
    pub fn ordered_visible_window_ids(&self) -> Vec<WindowId> {
        let mut ids: Vec<WindowId> = self
            .wm
            .visible_windows()
            .into_iter()
            .map(|w| w.id)
            .collect();
        ids.sort_by_key(|id| id.0);
        ids
    }

    /// Focuses and raises the `idx`-th window in [`Self::ordered_visible_window_ids`],
    /// restoring it if minimized (`RUSTYOS-023` / `RUSTYOS-032`).
    pub fn focus_ordered_window(&mut self, idx: usize, serial: Serial) {
        let ids = self.ordered_visible_window_ids();
        if let Some(&win_id) = ids.get(idx) {
            self.restore_window(win_id, serial);
            self.close_overview();
        }
    }

    /// Executes an interactive Dock click action (`RUSTYOS-023` / `RUSTYOS-032`).
    pub fn handle_dock_hit(&mut self, hit: crate::shell_render::DockHit, serial: Serial) {
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        match hit {
            crate::shell_render::DockHit::PinnedTerminal => {
                self.close_overview();
                let launched = self.launcher.launch_preferred(
                    &[
                        "foot",
                        "kitty",
                        "alacritty",
                        "wezterm",
                        "ghostty",
                        "weston-terminal",
                        "gnome-terminal",
                        "konsole",
                        "xfce4-terminal",
                    ],
                    &self.socket_name,
                    &ipc_path,
                );
                if !launched {
                    self.toggle_launcher();
                }
            }
            crate::shell_render::DockHit::PinnedFiles => {
                self.close_overview();
                let launched = self.launcher.launch_preferred(
                    &["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"],
                    &self.socket_name,
                    &ipc_path,
                );
                if !launched {
                    self.toggle_launcher();
                }
            }
            crate::shell_render::DockHit::ToggleOverview => {
                self.toggle_overview();
            }
            crate::shell_render::DockHit::WindowSlot(idx) => {
                let ids = self.ordered_visible_window_ids();
                if let Some(&win_id) = ids.get(idx) {
                    if self.is_window_minimized(win_id) {
                        self.restore_window(win_id, serial);
                        self.close_overview();
                    } else if !self.overview_active && self.wm.focused_window() == Some(win_id) {
                        self.minimize_window(win_id, serial);
                    } else {
                        self.wm.focus_and_raise(win_id);
                        self.sync_space_with_wm(serial);
                        self.close_overview();
                    }
                }
            }
        }
    }

    /// Launches the currently highlighted candidate from Spotlight and closes the overlay.
    pub fn launch_spotlight_selected(&mut self) {
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        let _ = self.launcher.launch_selected(&self.socket_name, &ipc_path);
        self.close_launcher();
    }

    /// Launches the Spotlight candidate at visible row offset `row_offset` and closes the overlay.
    pub fn launch_spotlight_row(&mut self, row_offset: usize) {
        if let Some(cmd) = self.launcher.exec_at_visible_row(5, row_offset) {
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

    fn popup_parent_target_rect(&self, surface: &PopupSurface) -> Option<Rectangle<i32, Logical>> {
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
                    self.space
                        .element_location(win)
                        .map(|loc| loc + win.geometry().loc)
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

    fn surface_in_tree(root: &WlSurface, target: &WlSurface) -> bool {
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

    /// Clears decoration-negotiation tracking for a closed or unmapped window (`RUSTYOS-030`).
    pub fn unregister_window_decoration(&mut self, win_id: WindowId) {
        self.xdg_decoration_requested.remove(&win_id);
        self.csd_window_ids.remove(&win_id);
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
        if self.window_uses_csd(win_id) {
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
        let Some((min_w, min_h)) = self.window_min_size(win_id) else {
            return false;
        };
        let titlebar_h = self.window_titlebar_height(win_id);
        let work_area = self.space.output_geometry(&self.output).map_or_else(
            || Rect::new(0, crate::shell_render::TOP_BAR_HEIGHT, 1280, 744),
            |geo| crate::shell_render::usable_work_area(geo.size.to_physical(1)),
        );
        let desired_w = min_w.min(work_area.width);
        let desired_h = min_h.saturating_add(titlebar_h).min(work_area.height);
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
        if self.wm.resize_window(win_id, target) {
            self.animations.snap_window_immediate(
                win_id.0,
                LogicalRect::new(target.x, target.y, target.width, target.height),
            );
            return true;
        }
        false
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

    fn apply_xdg_decoration_mode(&mut self, toplevel: &ToplevelSurface, use_csd: bool) {
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
            if self
                .active_window_drag
                .is_some_and(|drag| drag.window_id == id)
            {
                self.active_window_drag = None;
            }
            if self
                .active_window_resize
                .is_some_and(|resize| resize.window_id == id)
            {
                self.active_window_resize = None;
            }
            self.window_bindings.retain(|(wid, _)| *wid != id);
            self.unregister_window_decoration(id);
            self.minimized_window_ids.remove(&id);
            self.remove_from_switcher(id);
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
        let focused_is_minimized = self
            .wm
            .focused_window()
            .is_some_and(|id| self.is_window_minimized(id));
        if focused_is_minimized
            && matches!(
                action,
                WmAction::CloseFocused | WmAction::ToggleMaximizeFocused | WmAction::SnapFocused(_)
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
        let now = self.elapsed();
        let visible: Vec<(WindowId, Rect, bool)> = self
            .wm
            .visible_windows()
            .into_iter()
            .filter(|w| !self.is_window_minimized(w.id))
            .map(|w| (w.id, w.geometry, w.maximized))
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
                .is_some_and(|id| visible.iter().any(|(vid, _, _)| *vid == id));
            if !still_visible {
                self.space.unmap_elem(&sw);
            }
        }

        // 2. Map/update visible windows in bottom-to-top z-order and send pending configures.
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
                let has_ssd = !self.window_uses_csd(win_id);
                let titlebar_h = self.window_titlebar_height(win_id);
                let (client_width, client_height) = self.effective_client_size(win_id, rect);
                let geo_loc = window.geometry().loc;
                let client_origin = (origin.0 - geo_loc.x, origin.1 + titlebar_h - geo_loc.y);
                if let Some(toplevel) = window.toplevel() {
                    toplevel.with_pending_state(|state| {
                        state.size = Some((client_width, client_height).into());
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
                    if toplevel.is_initial_configure_sent() {
                        toplevel.send_pending_configure();
                    }
                } else if let Some(x11) = window.x11_surface() {
                    let _ = x11.configure(crate::xwayland::compute_x11_client_rect_for_decoration(
                        Rect::new(origin.0, origin.1, rect.width, rect.height),
                        has_ssd,
                    ));
                    let _ = x11.set_activated(is_focused);
                    let _ = x11.set_maximized(maximized);
                }
                self.space.map_element(window, client_origin, is_focused);
            }
        }

        // 3. Update Wayland keyboard focus to match the WM's focused window (native or XWayland).
        let focus_surface = focused_id
            .and_then(|id| self.smithay_window_for_id(id))
            .and_then(|w| WaylandFocus::wl_surface(&w).map(std::borrow::Cow::into_owned));
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, focus_surface, serial);
    }

    /// Advances active window, workspace, and shell overlay transitions to the current monotonic timestamp
    /// and updates window positions in [`Space<Window>`].
    pub fn tick_animations(&mut self) {
        let now = self.elapsed();
        let _ = self.overview_anim.tick(now);
        let _ = self.launcher_anim.tick(now);
        let _ = self.quick_settings_anim.tick(now);
        let _ = self.lock_screen_anim.tick(now);

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
                let geo_loc = window.geometry().loc;
                let client_origin = (origin.0 - geo_loc.x, origin.1 + titlebar_h - geo_loc.y);
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
                let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
                let _ = self
                    .launcher
                    .launch_command(&command, &self.socket_name, &ipc_path);
                self.close_launcher();
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

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);

        if let Some(window) = self.window_bindings.iter().find_map(|(_, w)| {
            WaylandFocus::wl_surface(w)
                .is_some_and(|ws| &*ws == surface)
                .then(|| w.clone())
        }) {
            window.on_commit();
            if let Some(win_id) = self.window_id_for_smithay(&window) {
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
        }
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

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let client = focused.and_then(|s| self.display_handle.get_client(s.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client.clone());
        set_primary_focus(&self.display_handle, seat, client);
        let _ = self.display_handle.flush_clients();
        debug!(has_focus = focused.is_some(), "Keyboard focus changed");
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}

impl SelectionHandler for RustyCompositorState {
    type SelectionUserData = ();

    fn new_selection(
        &mut self,
        ty: SelectionTarget,
        source: Option<SelectionSource>,
        _seat: Seat<Self>,
    ) {
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(err) = xwm.new_selection(ty, source.map(|s| s.mime_types())) {
                warn!(?ty, error = ?err, "Failed to mirror Wayland selection to XWayland");
            } else {
                // `X11Wm::new_selection` buffers `SetSelectionOwner` in `x11rb` without flushing;
                // perform a lightweight RandR query so `Xwayland` observes ownership immediately.
                let _ = xwm.get_randr_primary_output();
            }
        }
    }

    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        _user_data: &Self::SelectionUserData,
    ) {
        if let (Some(xwm), Some(loop_handle)) = (self.xwm.as_mut(), self.x11_loop_handle.clone())
            && let Err(err) = xwm.send_selection(ty, mime_type, fd, loop_handle)
        {
            warn!(?ty, error = ?err, "Failed to transfer XWayland selection to Wayland");
        }
    }
}

impl DataDeviceHandler for RustyCompositorState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl PrimarySelectionHandler for RustyCompositorState {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}

impl DataControlHandler for RustyCompositorState {
    fn data_control_state(&self) -> &DataControlState {
        &self.data_control_state
    }
}

impl ClientDndGrabHandler for RustyCompositorState {
    fn started(
        &mut self,
        _source: Option<WlDataSource>,
        icon: Option<WlSurface>,
        _seat: Seat<Self>,
    ) {
        self.dnd_icon = icon;
    }

    fn dropped(&mut self, _target: Option<WlSurface>, _validated: bool, _seat: Seat<Self>) {
        self.dnd_icon = None;
    }
}

impl ServerDndGrabHandler for RustyCompositorState {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

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
            self.unregister_window_decoration(win_id);
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

delegate_compositor!(RustyCompositorState);
delegate_shm!(RustyCompositorState);
delegate_output!(RustyCompositorState);
delegate_seat!(RustyCompositorState);
delegate_data_device!(RustyCompositorState);
delegate_primary_selection!(RustyCompositorState);
delegate_data_control!(RustyCompositorState);
delegate_xdg_shell!(RustyCompositorState);
delegate_xdg_decoration!(RustyCompositorState);
delegate_layer_shell!(RustyCompositorState);
