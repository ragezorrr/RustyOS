//! Core Wayland compositor state and protocol handler implementations.

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
        calloop::{LoopHandle, LoopSignal},
        wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer, wl_output::WlOutput, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{IsAlive, Logical, Point, Serial, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{self, CompositorClientState, CompositorHandler, CompositorState},
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
    /// Pending TTY virtual terminal switch (`Ctrl+Alt+F1..F12` -> `libseat::change_vt`).
    pub pending_vt_switch: Option<i32>,
    next_notification_id: u64,
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

        let mut state = Self {
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
            pending_vt_switch: None,
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

    /// Removes notifications whose TTL has elapsed as of current uptime (`RUSTYOS-024`).
    pub fn expire_notifications(&mut self) {
        let now = self.start_time.elapsed();
        self.expire_notifications_at(now);
    }

    /// Removes notifications whose TTL has elapsed at explicit monotonic timestamp `now` (`RUSTYOS-024`).
    pub fn expire_notifications_at(&mut self, now: Duration) {
        self.notifications
            .retain(|n| now.saturating_sub(n.created_at) < n.ttl);
        self.sync_notification_count();
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

    /// Focuses and raises the `idx`-th window in [`Self::ordered_visible_window_ids`] (`RUSTYOS-023`).
    pub fn focus_ordered_window(&mut self, idx: usize, serial: Serial) {
        let ids = self.ordered_visible_window_ids();
        if let Some(&win_id) = ids.get(idx) {
            self.wm.focus_and_raise(win_id);
            self.sync_space_with_wm(serial);
            self.close_overview();
        }
    }

    /// Executes an interactive Dock click action (`RUSTYOS-023`).
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
                self.focus_ordered_window(idx, serial);
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
            if self
                .active_window_resize
                .is_some_and(|resize| resize.window_id == id)
            {
                self.active_window_resize = None;
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
                let client_height = (rect.height - crate::shell_render::TITLEBAR_HEIGHT).max(64);
                let geo_loc = window.geometry().loc;
                let client_origin = (
                    origin.0 - geo_loc.x,
                    origin.1 + crate::shell_render::TITLEBAR_HEIGHT - geo_loc.y,
                );
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
                } else if let Some(x11) = window.x11_surface() {
                    let _ = x11.configure(crate::xwayland::compute_x11_client_rect(Rect::new(
                        origin.0,
                        origin.1,
                        rect.width,
                        rect.height,
                    )));
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
                let geo_loc = window.geometry().loc;
                let client_origin = (
                    origin.0 - geo_loc.x,
                    origin.1 + crate::shell_render::TITLEBAR_HEIGHT - geo_loc.y,
                );
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
            if let Some(win_id) = self.window_id_for_smithay(&window)
                && let Some(wm_win) = self.wm.window(win_id)
            {
                let origin = self
                    .animations
                    .animated_window_origin(win_id.0)
                    .unwrap_or((wm_win.geometry.x, wm_win.geometry.y));
                let geo_loc = window.geometry().loc;
                let client_origin = (
                    origin.0 - geo_loc.x,
                    origin.1 + crate::shell_render::TITLEBAR_HEIGHT - geo_loc.y,
                );
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

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let client = focused.and_then(|s| self.display_handle.get_client(s.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client.clone());
        set_primary_focus(&self.display_handle, seat, client);
        let _ = self.display_handle.flush_clients();
        debug!(has_focus = focused.is_some(), "Keyboard focus changed");
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}
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

        let client_height = (rect.height - crate::shell_render::TITLEBAR_HEIGHT).max(64);
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
            state.size = Some((rect.width, client_height).into());
            state.decoration_mode = Some(DecorationMode::ServerSide);
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

impl XdgDecorationHandler for RustyCompositorState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        toplevel.send_configure();
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
