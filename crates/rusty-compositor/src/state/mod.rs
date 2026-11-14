//! Core Wayland compositor state and protocol handler implementations.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use rusty_animation::{AnimatedValue, AnimationEngine};
use rusty_ipc::Urgency;
use rusty_wm::{OutputId, Rect, WindowId, WindowManager, WmAction, WorkspaceId};
use smithay::{
    backend::{
        allocator::Format,
        drm::DrmDeviceFd,
        renderer::{element::RenderElementStates, sync::Fence},
    },
    desktop::{
        PopupKind, PopupManager, Space, Window, WindowSurfaceType, find_popup_root_surface,
        layer_map_for_output,
        utils::{
            OutputPresentationFeedback, surface_presentation_feedback_flags_from_states,
            take_presentation_feedback_surface_tree,
        },
    },
    input::{
        Seat, SeatState,
        keyboard::{KeyboardHandle, XkbConfig},
        pointer::{CursorImageStatus, PointerHandle},
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::{LoopHandle, LoopSignal},
        wayland_server::{
            Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason, ObjectId},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Clock, IsAlive, Logical, Monotonic, Point, Rectangle, Serial, Size, Transform},
    wayland::{
        compositor::{
            self, CompositorClientState, CompositorHandler, CompositorState, TraversalAction,
        },
        cursor_shape::CursorShapeManagerState,
        dmabuf::{DmabufFeedback, DmabufGlobal, DmabufState},
        drm_syncobj::{DrmSyncobjCachedState, DrmSyncobjState, supports_syncobj_eventfd},
        foreign_toplevel_list::{ForeignToplevelHandle, ForeignToplevelListState},
        fractional_scale::{self, FractionalScaleManagerState},
        idle_inhibit::IdleInhibitManagerState,
        idle_notify::IdleNotifierState,
        input_method::{InputMethodManagerState, PopupSurface as ImePopupSurface},
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor,
            KeyboardShortcutsInhibitorSeat as _,
        },
        output::OutputManagerState,
        pointer_constraints::{
            PointerConstraint, PointerConstraintsState, with_pointer_constraint,
        },
        pointer_gestures::PointerGesturesState,
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
        seat::WaylandFocus,
        selection::{
            data_device::DataDeviceState, primary_selection::PrimarySelectionState,
            wlr_data_control::DataControlState,
        },
        session_lock::{LockSurface, SessionLockManagerState, SessionLocker},
        shell::{
            wlr_layer::{Layer, WlrLayerShellState},
            xdg::{PopupSurface, XdgShellState, decoration::XdgDecorationState},
        },
        shm::ShmState,
        text_input::TextInputManagerState,
        viewporter::ViewporterState,
        xdg_activation::XdgActivationState,
        xdg_toplevel_icon::XdgToplevelIconManager,
        xwayland_shell::XWaylandShellState,
    },
    xwayland::X11Wm,
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

mod shell_actions;
mod system_bridges;
mod wayland_handlers;
mod wm_windows;
pub(in crate::state) use wayland_handlers::desktop_protocols::ToplevelIconEntry;

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

/// Active native `PolicyKit` authentication prompt rendered as a Liquid Glass modal (`RUSTYOS-056`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolkitPromptState {
    /// Opaque `PolicyKit` cookie identifying the active authorization session.
    pub cookie: String,
    /// Action identifier requested by the client (e.g. `org.freedesktop.policykit.exec`).
    pub action_id: String,
    /// Human-readable message describing the privileged operation.
    pub message: String,
    /// Target UNIX account display label (`"root"`, etc.).
    pub identity_user: String,
    /// Current helper prompt text (`"Password"` / PAM prompt).
    pub prompt_label: String,
    /// `true` when the helper requested visible input (`PAM_PROMPT_ECHO_ON`).
    pub echo_on: bool,
    /// `true` while submitted credentials are being verified asynchronously.
    pub checking: bool,
    /// Last authentication failure or warning from the helper (`PAM_ERROR_MSG` / `FAILURE`).
    pub error_message: Option<String>,
    /// Informational message emitted by the PAM stack (`PAM_TEXT_INFO`).
    pub info_message: Option<String>,
}

/// Selectable capture source candidate inside the XDG Portal Liquid Glass modal (`RUSTYOS-057`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalSourceCandidate {
    /// Primary label rendered in the source list (`"Monitor: eDP-1"`, window title).
    pub label: String,
    /// Secondary subtitle (resolution / refresh or `app_id`).
    pub detail: String,
    /// `true` when representing an application window, `false` for a monitor output.
    pub is_window: bool,
    /// Output identifier when `is_window` is `false`.
    pub output_id: Option<OutputId>,
    /// Window identifier when `is_window` is `true`.
    pub window_id: Option<WindowId>,
}

/// Active XDG Desktop Portal source-selection / consent modal state (`RUSTYOS-057`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalPromptState {
    /// Request ID awaited by `Screenshot` or `ScreenCast.Start`.
    pub request_id: u64,
    /// Requesting application identifier (`app_id`).
    pub app_id: String,
    /// `true` for `ScreenCast`, `false` for `Screenshot`.
    pub is_screencast: bool,
    /// Session handle path when `is_screencast` is `true`.
    pub session_handle: Option<String>,
    /// Default target flags (`SCREENSHOT_TARGET_*`).
    pub requested_target: u32,
    /// Bitmask of permitted `ScreenCast` source types.
    pub allowed_types: u32,
    /// Whether cursor should be embedded in the screencast stream.
    pub include_cursor: bool,
    /// True when the caller did not forbid embedded cursor (`ScreenCast` consent toggle).
    pub cursor_mode_editable: bool,
    /// Session persistence mode requested by `SelectSources`.
    pub persist_mode: u32,
    /// Selectable monitors and/or windows.
    pub candidates: Vec<PortalSourceCandidate>,
    /// Highlighted row index inside `candidates`.
    pub selected: usize,
}

/// Active interactive color-picking request waiting for pointer click or `Esc` (`RUSTYOS-057`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPickColorState {
    /// Request ID awaited by `Screenshot.PickColor`.
    pub request_id: u64,
    /// Requesting client application ID.
    pub app_id: String,
}

/// Queued portal GPU capture executed during the next backend render pass (`RUSTYOS-057`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PendingPortalCapture {
    /// Capture an output or cropped window to a unique PNG and return its `file://` URI.
    Screenshot {
        /// Request ID awaited by `Screenshot`.
        request_id: u64,
        /// Selected portal target (`SCREENSHOT_TARGET_*`).
        target: u32,
        /// Explicit monitor output ID, if selected.
        output_id: Option<OutputId>,
        /// Explicit window ID, if selected.
        window_id: Option<WindowId>,
    },
    /// Sample a single framebuffer pixel at `pointer_global` and return `(r, g, b)`.
    PickColorSample {
        /// Request ID awaited by `PickColor`.
        request_id: u64,
        /// Global logical pointer coordinate clicked by the user.
        pointer_global: Point<f64, Logical>,
    },
}

/// Target surface pushed into a `PipeWire` Video/Source screencast stream (`RUSTYOS-057`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreencastCaptureTarget {
    /// Stream an entire compositor output.
    Output(OutputId),
    /// Stream a cropped window region on its owning output.
    Window(WindowId),
}

/// Active `org.freedesktop.impl.portal.ScreenCast` session backed by `rusty-pipewire-stream` (`RUSTYOS-057`).
#[derive(Debug)]
pub struct ActiveScreencastSession {
    /// D-Bus session object path.
    pub session_handle: String,
    /// Captured compositor monitor or window target.
    pub target: ScreencastCaptureTarget,
    /// `true` when software cursor should be embedded in captured frames.
    pub include_cursor: bool,
    /// Handle to the isolated PipeWire worker subprocess.
    pub stream: rusty_services::PipeWireStreamHandle,
    /// Target frame pacing rate (Hz).
    pub fps: u32,
    /// Timestamp of the last pushed frame (or start time).
    pub last_pushed_at: Instant,
    /// `true` once at least one frame has initialized the stream geometry.
    pub pushed_initial_frame: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedPortalTarget {
    pub(crate) output_id: OutputId,
    pub(crate) capture_size: Size<i32, smithay::utils::Physical>,
    pub(crate) output_scale: f64,
    pub(crate) crop: Option<Rect>,
    pub(crate) origin: Point<i32, Logical>,
    pub(crate) mapping_id: String,
    pub(crate) refresh_hz: u32,
    pub(crate) stream_size: (u32, u32),
}

/// Compositor-level action recognized from a completed 3-finger touchpad swipe (`RUSTYOS-047`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreeFingerSwipeAction {
    /// Switch to the previous workspace (`WorkspaceId - 1`).
    PreviousWorkspace,
    /// Switch to the next workspace (`WorkspaceId + 1`).
    NextWorkspace,
    /// Open Workspace Overview if closed.
    OpenOverview,
    /// Close Workspace Overview if open.
    CloseOverview,
}

/// Active multi-finger touchpad swipe accumulator for compositor gestures (`RUSTYOS-047`).
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchpadSwipeState {
    /// Whether a touchpad swipe sequence is currently active.
    pub active: bool,
    /// Finger count reported by `GestureSwipeBegin`.
    pub fingers: u32,
    /// Accumulated horizontal swipe delta in logical units.
    pub dx: f64,
    /// Accumulated vertical swipe delta in logical units.
    pub dy: f64,
    /// True when the compositor claimed this 3-finger gesture exclusively.
    pub claimed_by_compositor: bool,
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
    /// Path used for atomic persistence and live reload (`None` in unit tests) (`RUSTYOS-061`).
    pub config_path: Option<std::path::PathBuf>,
    /// Short active XKB layout label (`"EN"`, `"RU"`, etc.) displayed in `TopBar` (`RUSTYOS-029`).
    pub active_kbd_layout: String,
    /// `wl_shm` shared-memory buffer state.
    pub shm_state: ShmState,
    /// `wp_viewporter` surface crop/destination scaling state (`RUSTYOS-043`).
    pub viewporter_state: ViewporterState,
    /// `wp_fractional_scale_v1` manager state (`RUSTYOS-043`).
    pub fractional_scale_manager_state: FractionalScaleManagerState,
    /// Monotonic clock source shared with `wp_presentation` feedback (`RUSTYOS-044`).
    pub clock: Clock<Monotonic>,
    /// `zwp_relative_pointer_manager_v1` relative input global (`RUSTYOS-044`).
    pub relative_pointer_manager_state: RelativePointerManagerState,
    /// `zwp_pointer_constraints_v1` lock/confine global (`RUSTYOS-044`).
    pub pointer_constraints_state: PointerConstraintsState,
    /// `wp_presentation` frame presentation timing global (`RUSTYOS-044`).
    pub presentation_state: PresentationState,
    /// `wp_cursor_shape_v1` server-side cursor icon global (`RUSTYOS-045`).
    pub cursor_shape_state: CursorShapeManagerState,
    /// `xdg_activation_v1` focus-transfer activation global (`RUSTYOS-045`).
    pub xdg_activation_state: XdgActivationState,
    /// `ext_idle_notify_v1` idle timeout notification global (`RUSTYOS-045`).
    pub idle_notifier_state: IdleNotifierState<Self>,
    /// `zwp_idle_inhibit_manager_v1` idle inhibitor global (`RUSTYOS-045`).
    pub idle_inhibit_state: IdleInhibitManagerState,
    /// Surfaces currently holding an active idle inhibitor (`RUSTYOS-045`).
    pub idle_inhibitor_surfaces: Vec<WlSurface>,
    /// `ext_session_lock_v1` external locker manager state (`RUSTYOS-045`).
    pub session_lock_state: SessionLockManagerState,
    /// `true` while an `ext_session_lock_v1` client holds an active session lock (`RUSTYOS-045`).
    pub ext_session_locked: bool,
    /// Pending lock confirmation awaiting a cleared/locked frame presentation (`RUSTYOS-045`).
    pub pending_session_locker: Option<SessionLocker>,
    /// Per-output surfaces submitted by the active `ext_session_lock_v1` client (`RUSTYOS-045`).
    pub ext_lock_surfaces: Vec<(Output, LockSurface)>,
    /// `zwp_linux_dmabuf_v1` client DMA-BUF import state (`RUSTYOS-041`).
    pub dmabuf_state: DmabufState,
    /// Active `zwp_linux_dmabuf_v1` global handle registered by the GPU backend (`RUSTYOS-041`).
    pub dmabuf_global: Option<DmabufGlobal>,
    /// Shared main event-loop handle used for async explicit-sync fence wakeups (`RUSTYOS-046`).
    pub loop_handle: LoopHandle<'static, Self>,
    /// `wp_linux_drm_syncobj_v1` explicit-sync manager state when DRM syncobj eventfd is supported (`RUSTYOS-046`).
    pub syncobj_state: Option<DrmSyncobjState>,
    /// Set when an explicit-sync `DrmSyncPointBlocker` clears asynchronously (`RUSTYOS-046`).
    pub syncobj_surface_unblocked: bool,
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
    /// Animated Dock hover lift progress (`0.0` rest -> `1.0` lifted, 120 ms) (`RUSTYOS-048`).
    pub dock_hover_anim: AnimatedValue,
    /// Open `TopBar` left dropdown menu popover if any (`RUSTYOS-048`).
    pub active_top_bar_menu: Option<crate::shell_render::TopBarMenuHit>,
    /// Currently hovered row index inside an open `TopBar` dropdown menu (`RUSTYOS-051`).
    pub top_bar_menu_hover_row: Option<usize>,
    /// Active Control Center slider drag state (`RUSTYOS-051`).
    pub qs_slider_drag: Option<crate::shell_render::QuickSettingsSliderDrag>,
    /// Index of currently hovered floating Notification HUD toast (`RUSTYOS-051`).
    pub hovered_notification_idx: Option<usize>,
    /// Managed SSD window whose traffic-light button cluster is currently hovered (`RUSTYOS-051`).
    pub hovered_traffic_light_window: Option<WindowId>,
    /// Live shell glass opacity multiplier (`0.35..=1.0`, default `0.78`) (`RUSTYOS-048`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels mode (`RUSTYOS-048`).
    pub reduce_transparency: bool,
    /// Show workspace digits `1..=9` and `Float`/`Tile` pill in Menu Bar (`RUSTYOS-049`).
    pub menubar_show_workspaces: bool,
    /// Show host SNI system tray icons in Menu Bar (`RUSTYOS-049`).
    pub menubar_show_tray: bool,
    /// Show keyboard layout badge in Menu Bar (`RUSTYOS-049`).
    pub menubar_show_input_source: bool,
    /// Show standalone speaker volume icon in Menu Bar (`RUSTYOS-049`).
    pub menubar_show_volume: bool,
    /// Display 24-hour clock format instead of 12-hour AM/PM (`RUSTYOS-049`).
    pub clock_24h: bool,
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
    /// Whether the built-in System Settings modal overlay is open (`RUSTYOS-061`).
    pub settings_active: bool,
    /// Active left-sidebar category in System Settings (`RUSTYOS-061`).
    pub settings_tab: crate::shell_render::SettingsTab,
    /// Footer persistence badge (`""`, `"Saved"`, `"Reloaded"`, or `"Error"`) (`RUSTYOS-061`).
    pub config_status_label: &'static str,
    /// Monotonic counter incremented when custom wallpaper configuration/files reload (`RUSTYOS-062`).
    pub wallpaper_revision: u64,
    /// Last configured custom `.png` wallpaper path recalled when toggling back from built-in Tahoe (`RUSTYOS-062`).
    pub last_custom_wallpaper: Option<std::path::PathBuf>,
    /// Whether Focus (Do Not Disturb) notification popup suppression is active (`RUSTYOS-053`).
    pub focus_mode: bool,
    /// Non-blocking system telemetry & quick audio controls (`RUSTYOS-021`).
    pub telemetry: crate::telemetry::SystemMetricsHandle,
    /// Whether the session lock screen is active (`RUSTYOS-009`/`010`/`025`).
    pub screen_locked: bool,
    /// Animated Lock Screen transition progress (`0.0` unlocked -> `1.0` locked) (`RUSTYOS-025`).
    pub lock_screen_anim: AnimatedValue,
    /// Selected account index on the macOS-style Lock Screen user switcher (`RUSTYOS-054`).
    pub lock_selected_user: usize,
    /// Whether the Lock Screen top-right power popover (`Sleep`/`Restart`/`Shut Down`) is open (`RUSTYOS-054`).
    pub lock_power_menu_open: bool,
    /// Zeroizing masked Lock Screen password input buffer (`RUSTYOS-056`).
    pub lock_password: rusty_services::SecretText,
    /// `true` while background `unix_chkpwd` verification is running (`RUSTYOS-056`).
    pub lock_auth_checking: bool,
    /// Request identifier matching the active in-flight Lock Screen authentication check (`RUSTYOS-056`).
    pub lock_pending_request_id: Option<u64>,
    /// Monotonic counter generating Lock Screen authentication request IDs (`RUSTYOS-056`).
    pub lock_next_request_id: u64,
    /// Last PAM rejection or service error message shown on the Lock Screen (`RUSTYOS-056`).
    pub lock_auth_error: Option<String>,
    /// Non-blocking PAM verification worker handle (`RUSTYOS-056`).
    pub auth: rusty_services::AsyncAuthHandle,
    /// Non-blocking `systemd-logind` sleep and session lock/unlock monitor (`RUSTYOS-056`).
    pub logind: rusty_services::LogindMonitorHandle,
    /// Non-blocking native `org.freedesktop.PolicyKit1.AuthenticationAgent` (`RUSTYOS-056`).
    pub polkit: rusty_services::PolkitAgentHandle,
    /// Active `PolicyKit` prompt displayed above normal windows (`RUSTYOS-056`).
    pub polkit_prompt: Option<PolkitPromptState>,
    /// Zeroizing secret input buffer typed into `polkit_prompt` (`RUSTYOS-056`).
    pub polkit_password: rusty_services::SecretText,
    /// Native XDG Desktop Portal D-Bus backend handle (`Screenshot` v3 / `ScreenCast` v6) (`RUSTYOS-057`).
    pub portal: rusty_services::PortalHandle,
    /// Active XDG Portal source selection modal (`RUSTYOS-057`).
    pub portal_prompt: Option<PortalPromptState>,
    /// Active `PickColor` interactive sampling banner state (`RUSTYOS-057`).
    pub portal_pick_color: Option<PendingPickColorState>,
    /// Queued portal GPU capture / pixel sample executed during the next frame (`RUSTYOS-057`).
    pub pending_portal_capture: Option<PendingPortalCapture>,
    /// Active PipeWire screencast worker sessions (`RUSTYOS-057`).
    pub active_screencasts: Vec<ActiveScreencastSession>,
    /// Signals DRM/Winit loops that asynchronous auth/logind/polkit updates need a frame (`RUSTYOS-056`).
    pub shell_redraw_needed: bool,
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
    /// `zwp_pointer_gestures_v1` swipe/pinch/hold gesture global state (`RUSTYOS-047`).
    pub pointer_gestures_state: PointerGesturesState,
    /// `zwp_text_input_manager_v3` global state for client IME text input (`RUSTYOS-047`).
    pub text_input_manager_state: TextInputManagerState,
    /// `zwp_input_method_manager_v2` global state for external IME engines (`RUSTYOS-047`).
    pub input_method_manager_state: InputMethodManagerState,
    /// Active IME popup surfaces (`zwp_input_popup_surface_v2`) awaiting draw (`RUSTYOS-047`).
    pub ime_popups: Vec<ImePopupSurface>,
    /// `zwp_keyboard_shortcuts_inhibit_manager_v1` global state (`RUSTYOS-047`).
    pub keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    /// Currently activated shortcut inhibitor for the focused client surface (`RUSTYOS-047`).
    pub active_shortcut_inhibitor: Option<KeyboardShortcutsInhibitor>,
    /// Surface whose shortcut inhibitor was explicitly released via `Super+Escape` until focus leaves (`RUSTYOS-047`).
    pub suppressed_inhibitor_surface: Option<WlSurface>,
    /// `ext_foreign_toplevel_list_v1` global state exposing mapped toplevels (`RUSTYOS-047`).
    pub foreign_toplevel_list_state: ForeignToplevelListState,
    /// Active `ext_foreign_toplevel_handle_v1` handles keyed by WM [`WindowId`] (`RUSTYOS-047`).
    pub foreign_toplevel_handles: HashMap<WindowId, ForeignToplevelHandle>,
    /// `xdg_toplevel_icon_v1` manager state advertising preferred icon sizes (`RUSTYOS-047`).
    pub xdg_toplevel_icon_manager: XdgToplevelIconManager,
    /// Committed `xdg_toplevel_icon_v1` icon metadata & SHM buffers per [`WindowId`] (`RUSTYOS-047`).
    pub window_toplevel_icons: HashMap<WindowId, crate::icons::ShmToplevelIcon>,
    /// Active 3-finger touchpad gesture accumulator (`RUSTYOS-047`).
    pub touchpad_swipe: TouchpadSwipeState,
    /// Optional CLI `--rotate` override applied across outputs (`RUSTYOS-055`).
    pub rotation_override: Option<crate::config::DisplayRotation>,
    toplevel_icons: HashMap<ObjectId, ToplevelIconEntry>,
    next_toplevel_icon_rev: u64,
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
        loop_handle: LoopHandle<'static, Self>,
        initial_size: Size<i32, smithay::utils::Physical>,
    ) -> Result<Self, CompositorError> {
        let display_handle = display.handle();

        let compositor_state = CompositorState::new::<Self>(&display_handle);
        let xdg_shell_state = XdgShellState::new::<Self>(&display_handle);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&display_handle);
        let wlr_layer_shell_state = WlrLayerShellState::new::<Self>(&display_handle);
        let xwayland_shell_state = XWaylandShellState::new::<Self>(&display_handle);
        let shm_state = ShmState::new::<Self>(&display_handle, Vec::new());
        let viewporter_state = ViewporterState::new::<Self>(&display_handle);
        let fractional_scale_manager_state =
            FractionalScaleManagerState::new::<Self>(&display_handle);
        let clock = Clock::new();
        let relative_pointer_manager_state =
            RelativePointerManagerState::new::<Self>(&display_handle);
        let pointer_constraints_state = PointerConstraintsState::new::<Self>(&display_handle);
        let presentation_state = PresentationState::new::<Self>(&display_handle, 1);
        let cursor_shape_state = CursorShapeManagerState::new::<Self>(&display_handle);
        let xdg_activation_state = XdgActivationState::new::<Self>(&display_handle);
        let idle_notifier_state =
            IdleNotifierState::<Self>::new(&display_handle, loop_handle.clone());
        let idle_inhibit_state = IdleInhibitManagerState::new::<Self>(&display_handle);
        let session_lock_state = SessionLockManagerState::new::<Self, _>(&display_handle, |_| true);
        let pointer_gestures_state = PointerGesturesState::new::<Self>(&display_handle);
        let text_input_manager_state = TextInputManagerState::new::<Self>(&display_handle);
        let input_method_manager_state =
            InputMethodManagerState::new::<Self, _>(&display_handle, |_| true);
        let keyboard_shortcuts_inhibit_state =
            KeyboardShortcutsInhibitState::new::<Self>(&display_handle);
        let foreign_toplevel_list_state = ForeignToplevelListState::new::<Self>(&display_handle);
        let mut xdg_toplevel_icon_manager = XdgToplevelIconManager::new::<Self>(&display_handle);
        for size in [16, 24, 32, 48, 64] {
            xdg_toplevel_icon_manager.add_icon_size(size);
        }
        let dmabuf_state = DmabufState::new();
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&display_handle);
        let data_device_state = DataDeviceState::new::<Self>(&display_handle);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&display_handle);
        let data_control_state = DataControlState::new::<Self, _>(
            &display_handle,
            Some(&primary_selection_state),
            |_| true,
        );

        let user_config = crate::config::UserConfig::load_default();
        let initial_scale = user_config.scale_for_output("winit-0");
        let initial_rotation = user_config.rotation_for_output("winit-0", None);
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&display_handle, "rusty-seat-0");

        let keyboard = Self::init_seat_keyboard(&mut seat, &user_config)?;
        let pointer = seat.add_pointer();

        let (output, primary_global) = Self::create_initial_output(
            &display_handle,
            initial_size,
            initial_scale,
            initial_rotation,
        );

        let mut space = Space::default();
        space.map_output(&output, (0, 0));

        let logical_size = crate::outputs::compute_logical_size_for_rotation(
            initial_size,
            initial_scale,
            initial_rotation,
        );
        let logical_phys_typed =
            Size::<i32, smithay::utils::Physical>::from((logical_size.w, logical_size.h));
        let work_area = crate::shell_render::usable_work_area(logical_phys_typed);
        let full_area = Rect::new(0, 0, logical_size.w.max(100), logical_size.h.max(100));
        let mut wm = WindowManager::new(work_area);
        wm.upsert_output_with_full_area(PRIMARY_OUTPUT_ID, work_area, full_area);

        let mut primary_entry = crate::outputs::OutputEntry::new(
            PRIMARY_OUTPUT_ID,
            "winit-0",
            output.clone(),
            (0, 0).into(),
            initial_size,
            initial_scale,
            60_000,
            true,
        )
        .with_rotation(initial_rotation);
        primary_entry.global_id = Some(primary_global);
        let output_map = crate::outputs::OutputMap::new(primary_entry);

        let initial_glass_opacity = user_config.glass_opacity;
        let initial_reduce_transparency = user_config.reduce_transparency;
        let initial_menubar_show_workspaces = user_config.menubar_show_workspaces;
        let initial_menubar_show_tray = user_config.menubar_show_tray;
        let initial_menubar_show_input_source = user_config.menubar_show_input_source;
        let initial_menubar_show_volume = user_config.menubar_show_volume;
        let initial_clock_24h = user_config.clock_24h;
        let initial_last_custom_wallpaper = user_config.wallpaper.clone();

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
            config_path: None,
            active_kbd_layout: "EN".to_owned(),
            shm_state,
            viewporter_state,
            fractional_scale_manager_state,
            clock,
            relative_pointer_manager_state,
            pointer_constraints_state,
            presentation_state,
            cursor_shape_state,
            xdg_activation_state,
            idle_notifier_state,
            idle_inhibit_state,
            idle_inhibitor_surfaces: Vec::new(),
            session_lock_state,
            ext_session_locked: false,
            pending_session_locker: None,
            ext_lock_surfaces: Vec::new(),
            dmabuf_state,
            dmabuf_global: None,
            loop_handle,
            syncobj_state: None,
            syncobj_surface_unblocked: false,
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
            dock_hover_anim: AnimatedValue::new_settled(0.0),
            active_top_bar_menu: None,
            top_bar_menu_hover_row: None,
            qs_slider_drag: None,
            hovered_notification_idx: None,
            hovered_traffic_light_window: None,
            glass_opacity: initial_glass_opacity,
            reduce_transparency: initial_reduce_transparency,
            menubar_show_workspaces: initial_menubar_show_workspaces,
            menubar_show_tray: initial_menubar_show_tray,
            menubar_show_input_source: initial_menubar_show_input_source,
            menubar_show_volume: initial_menubar_show_volume,
            clock_24h: initial_clock_24h,
            launcher_active: false,
            launcher_anim: AnimatedValue::new_settled(0.0),
            launcher: crate::launcher::LauncherState::new(),
            quick_settings_active: false,
            quick_settings_anim: AnimatedValue::new_settled(0.0),
            settings_active: false,
            settings_tab: crate::shell_render::SettingsTab::Appearance,
            config_status_label: "",
            wallpaper_revision: 0,
            last_custom_wallpaper: initial_last_custom_wallpaper,
            focus_mode: false,
            telemetry: crate::telemetry::SystemMetricsHandle::static_default(),
            screen_locked: false,
            lock_screen_anim: AnimatedValue::new_settled(0.0),
            lock_selected_user: 0,
            lock_power_menu_open: false,
            lock_password: rusty_services::SecretText::new(),
            lock_auth_checking: false,
            lock_pending_request_id: None,
            lock_next_request_id: 1,
            lock_auth_error: None,
            auth: rusty_services::AsyncAuthHandle::default(),
            logind: rusty_services::LogindMonitorHandle::default(),
            polkit: rusty_services::PolkitAgentHandle::default(),
            polkit_prompt: None,
            polkit_password: rusty_services::SecretText::new(),
            portal: rusty_services::PortalHandle::default(),
            portal_prompt: None,
            portal_pick_color: None,
            pending_portal_capture: None,
            active_screencasts: Vec::new(),
            shell_redraw_needed: false,
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
            pointer_gestures_state,
            text_input_manager_state,
            input_method_manager_state,
            ime_popups: Vec::new(),
            keyboard_shortcuts_inhibit_state,
            active_shortcut_inhibitor: None,
            suppressed_inhibitor_surface: None,
            foreign_toplevel_list_state,
            foreign_toplevel_handles: HashMap::new(),
            xdg_toplevel_icon_manager,
            window_toplevel_icons: HashMap::new(),
            touchpad_swipe: TouchpadSwipeState::default(),
            rotation_override: None,
            toplevel_icons: HashMap::new(),
            next_toplevel_icon_rev: 0,
            next_notification_id: 0,
        };
        state
            .animations
            .set_reduced_motion(state.user_config.reduced_motion);
        state.refresh_keyboard_layout_label();
        Ok(state)
    }

    pub(in crate::state) fn create_initial_output(
        display_handle: &DisplayHandle,
        initial_size: Size<i32, smithay::utils::Physical>,
        initial_scale: f64,
        initial_rotation: crate::config::DisplayRotation,
    ) -> (
        Output,
        smithay::reexports::wayland_server::backend::GlobalId,
    ) {
        let output = Output::new(
            "winit-0".to_owned(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "RustyOS".to_owned(),
                model: "Nested Winit Output".to_owned(),
            },
        );
        let output_global = output.create_global::<Self>(display_handle);

        let mode = Mode {
            size: initial_size,
            refresh: 60_000,
        };
        output.change_current_state(
            Some(mode),
            Some(initial_rotation.to_winit_transform()),
            Some(Scale::Fractional(crate::config::clamp_display_scale(
                initial_scale,
            ))),
            Some((0, 0).into()),
        );
        output.set_preferred(mode);
        (output, output_global)
    }

    /// Dynamically registers and maps a new display output into the compositor workspace (`RUSTYOS-040`).
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
        let rotation = self
            .user_config
            .rotation_for_output(&name_str, self.rotation_override);
        let output = Output::new(name_str.clone(), physical_props);
        let output_global = output.create_global::<Self>(&self.display_handle);
        output.change_current_state(
            Some(mode),
            Some(rotation.to_wl_transform()),
            Some(scale),
            Some(location),
        );
        output.set_preferred(mode);

        self.space.map_output(&output, location);

        let scale_val = crate::config::clamp_display_scale(match scale {
            Scale::Fractional(f) => f,
            Scale::Integer(i) => f64::from(i),
            Scale::Custom { .. } => 1.0,
        });
        let logical_size =
            crate::outputs::compute_logical_size_for_rotation(mode.size, scale_val, rotation);

        let work_area = Rect::new(
            location.x,
            location.y,
            logical_size.w.max(100),
            logical_size.h.max(100),
        );
        self.wm
            .upsert_output_with_full_area(new_id, work_area, work_area);

        let mut entry = crate::outputs::OutputEntry::new(
            new_id,
            name_str,
            output,
            location,
            mode.size,
            scale_val,
            mode.refresh,
            false,
        )
        .with_rotation(rotation);
        entry.global_id = Some(output_global);
        self.output_map.add(entry);

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
        new_id
    }

    /// Updates pixel resolution, `HiDPI` scale factor, refresh rate, and logical work area for an
    /// active display output (`RUSTYOS-043`).
    pub fn update_output_geometry(
        &mut self,
        output_id: OutputId,
        physical_size: Size<i32, smithay::utils::Physical>,
        scale: f64,
        refresh_milli_hz: i32,
    ) -> bool {
        let clamped_scale = crate::config::clamp_display_scale(scale);
        if !self.output_map.update_entry_geometry(
            output_id,
            physical_size,
            clamped_scale,
            refresh_milli_hz,
        ) {
            return false;
        }

        if let Some(target_output) = self.output_map.get(output_id).map(|e| e.output.clone()) {
            let mode = Mode {
                size: physical_size,
                refresh: refresh_milli_hz,
            };
            target_output.change_current_state(
                Some(mode),
                None,
                Some(Scale::Fractional(clamped_scale)),
                None,
            );
            target_output.set_preferred(mode);
        }

        self.relayout_wm_for_all_outputs();
        true
    }

    /// Updates display rotation for `output_id` and re-packs output/WM layout (`RUSTYOS-055`).
    ///
    /// Pass `use_winit_fbo_flip = true` for nested Winit OpenGL FBO outputs and `false` for DRM/KMS.
    pub fn set_output_rotation(
        &mut self,
        output_id: OutputId,
        rotation: crate::config::DisplayRotation,
        use_winit_fbo_flip: bool,
    ) -> bool {
        if !self.output_map.update_entry_rotation(output_id, rotation) {
            return false;
        }

        let wl_transform = if use_winit_fbo_flip {
            rotation.to_winit_transform()
        } else {
            rotation.to_wl_transform()
        };
        if let Some(target_output) = self.output_map.get(output_id).map(|e| e.output.clone()) {
            target_output.change_current_state(None, Some(wl_transform), None, None);
        }

        self.relayout_wm_for_all_outputs();
        true
    }

    pub(in crate::state) fn relayout_wm_for_all_outputs(&mut self) {
        let entries: Vec<_> = self.output_map.entries().to_vec();
        for entry in entries {
            entry
                .output
                .change_current_state(None, None, None, Some(entry.location));
            self.space.map_output(&entry.output, entry.location);
            let geo = entry.logical_geometry();
            let logical_phys =
                Size::<i32, smithay::utils::Physical>::from((geo.size.w, geo.size.h));
            let full_area = Rect::new(
                geo.loc.x,
                geo.loc.y,
                geo.size.w.max(100),
                geo.size.h.max(100),
            );
            let work_area = if entry.is_primary {
                let mut wa = crate::shell_render::usable_work_area(logical_phys);
                wa.x += geo.loc.x;
                wa.y += geo.loc.y;
                wa
            } else {
                full_area
            };
            self.wm
                .upsert_output_with_full_area(entry.id, work_area, full_area);
            layer_map_for_output(&entry.output).arrange();
        }

        let total = self.output_map.total_bounds();
        self.pointer_location.x = self.pointer_location.x.clamp(
            f64::from(total.loc.x),
            f64::from(total.loc.x + total.size.w.saturating_sub(1)),
        );
        self.pointer_location.y = self.pointer_location.y.clamp(
            f64::from(total.loc.y),
            f64::from(total.loc.y + total.size.h.saturating_sub(1)),
        );

        self.reconfigure_ext_lock_surfaces();
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
    }

    /// Resolves the active display scale factor for `surface` (`RUSTYOS-043`).
    #[must_use]
    pub fn preferred_scale_for_surface(&self, surface: &WlSurface) -> f64 {
        let mut root = surface.clone();
        while let Some(parent) = compositor::get_parent(&root) {
            root = parent;
        }
        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(&root)
            && let Ok(popup_root) = find_popup_root_surface(&PopupKind::Xdg(popup))
        {
            root = popup_root;
            while let Some(parent) = compositor::get_parent(&root) {
                root = parent;
            }
        }

        for (_, window) in &self.window_bindings {
            if WaylandFocus::wl_surface(window).is_some_and(|ws| *ws == root) {
                if let Some(win_id) = self.window_id_for_smithay(window)
                    && let Some(wm_win) = self.wm.window(win_id)
                {
                    let center = Point::from((
                        f64::from(wm_win.geometry.x) + f64::from(wm_win.geometry.width) * 0.5,
                        f64::from(wm_win.geometry.y) + f64::from(wm_win.geometry.height) * 0.5,
                    ));
                    if let Some(entry) = self
                        .output_map
                        .output_under(center)
                        .and_then(|id| self.output_map.get(id))
                    {
                        return entry.scale;
                    }
                }
                if let Some(first_out) = self.space.outputs_for_element(window).into_iter().next()
                    && let Some(entry) = self.output_map.find_by_output(&first_out)
                {
                    return entry.scale;
                }
            }
        }

        for entry in self.output_map.entries() {
            let layer_map = layer_map_for_output(&entry.output);
            if layer_map
                .layer_for_surface(&root, WindowSurfaceType::ALL)
                .is_some()
            {
                return entry.scale;
            }
        }

        self.output_map.primary().scale
    }

    /// Propagates fractional (`wp_fractional_scale_v1`) and integer (`wl_surface.preferred_buffer_scale`)
    /// scale hints across `surface` and all subsurfaces (`RUSTYOS-043`).
    #[allow(clippy::cast_possible_truncation)]
    pub fn apply_surface_scale(surface: &WlSurface, scale: f64) {
        let clamped = crate::config::clamp_display_scale(scale);
        let int_scale = clamped.ceil().clamp(1.0, 4.0) as i32;
        compositor::with_surface_tree_downward(
            surface,
            (),
            |_, _, ()| TraversalAction::DoChildren(()),
            |surf, states, ()| {
                compositor::send_surface_state(surf, states, int_scale, Transform::Normal);
                fractional_scale::with_fractional_scale(states, |fs| {
                    fs.set_preferred_scale(clamped);
                });
            },
            |_, _, ()| true,
        );
    }

    /// Returns `true` when either the built-in lock screen or an `ext_session_lock_v1`
    /// client currently locks the session (`RUSTYOS-045`).
    #[must_use]
    pub fn is_session_locked(&self) -> bool {
        self.screen_locked || self.ext_session_locked
    }

    /// Returns `true` when an interactive compositor overlay or lock screen should bypass any
    /// client pointer lock/confinement (`RUSTYOS-044` / `RUSTYOS-045`).
    #[must_use]
    pub fn has_blocking_shell_overlay(&self) -> bool {
        self.is_session_locked()
            || self.polkit_prompt.is_some()
            || self.portal_prompt.is_some()
            || self.launcher_active
            || self.overview_active
            || self.quick_settings_active
            || self.settings_active
            || self.window_switcher.is_some()
            || self.active_window_drag.is_some()
            || self.active_window_resize.is_some()
    }

    /// Returns `true` when the currently focused client has an activated
    /// `zwp_keyboard_shortcuts_inhibit_v1` inhibitor and no compositor lock/overlay overrides it (`RUSTYOS-047`).
    #[must_use]
    pub fn is_keyboard_shortcuts_inhibited(&self) -> bool {
        if self.is_session_locked() || self.has_blocking_shell_overlay() {
            return false;
        }
        self.keyboard
            .current_focus()
            .and_then(|surface| self.seat.keyboard_shortcuts_inhibitor_for_surface(&surface))
            .is_some_and(|inhibitor| inhibitor.is_active())
    }

    /// Deactivates any active keyboard shortcuts inhibitor and suppresses auto-reactivation until
    /// keyboard focus leaves that surface (`Super+Escape` & session lock isolation) (`RUSTYOS-047`).
    pub fn deactivate_focused_shortcut_inhibitor(&mut self) {
        let mut released_surface = None;
        if let Some(inhibitor) = self.active_shortcut_inhibitor.take() {
            released_surface = Some(inhibitor.wl_surface().clone());
            inhibitor.inactivate();
        }
        if let Some(focused) = self.keyboard.current_focus() {
            if let Some(inhibitor) = self.seat.keyboard_shortcuts_inhibitor_for_surface(&focused) {
                inhibitor.inactivate();
            }
            released_surface.get_or_insert(focused);
        }
        self.suppressed_inhibitor_surface = released_surface;
    }

    /// Returns `true` when multi-finger compositor touchpad gestures are permitted (`RUSTYOS-047`).
    #[must_use]
    pub fn can_handle_compositor_gesture(&self) -> bool {
        !self.is_session_locked()
            && !self.launcher_active
            && !self.quick_settings_active
            && self.window_switcher.is_none()
            && self.active_window_drag.is_none()
            && self.active_window_resize.is_none()
            && self.active_popup_grabs.is_empty()
    }

    /// Starts tracking a touchpad swipe; returns `true` when the compositor claims a 3-finger swipe (`RUSTYOS-047`).
    pub fn begin_touchpad_swipe(&mut self, fingers: u32) -> bool {
        if fingers == 3 && self.can_handle_compositor_gesture() {
            self.touchpad_swipe = TouchpadSwipeState {
                active: true,
                fingers,
                dx: 0.0,
                dy: 0.0,
                claimed_by_compositor: true,
            };
            true
        } else {
            self.touchpad_swipe = TouchpadSwipeState::default();
            false
        }
    }

    /// Accumulates touchpad swipe delta; returns `true` if the swipe is handled by the compositor (`RUSTYOS-047`).
    pub fn update_touchpad_swipe(&mut self, delta: Point<f64, Logical>) -> bool {
        if self.touchpad_swipe.active && self.touchpad_swipe.claimed_by_compositor {
            if !self.can_handle_compositor_gesture() {
                self.touchpad_swipe = TouchpadSwipeState::default();
                return true;
            }
            self.touchpad_swipe.dx += delta.x;
            self.touchpad_swipe.dy += delta.y;
            return true;
        }
        false
    }

    /// Completes a tracked touchpad swipe; returns `true` if handled by the compositor (`RUSTYOS-047`).
    pub fn end_touchpad_swipe(&mut self, cancelled: bool, serial: Serial) -> bool {
        if self.touchpad_swipe.active && self.touchpad_swipe.claimed_by_compositor {
            let swipe = std::mem::take(&mut self.touchpad_swipe);
            if !cancelled
                && self.can_handle_compositor_gesture()
                && let Some(action) = classify_three_finger_swipe(swipe.dx, swipe.dy)
            {
                match action {
                    ThreeFingerSwipeAction::PreviousWorkspace => {
                        self.switch_workspace_relative(-1, serial);
                    }
                    ThreeFingerSwipeAction::NextWorkspace => {
                        self.switch_workspace_relative(1, serial);
                    }
                    ThreeFingerSwipeAction::OpenOverview => {
                        if !self.overview_active {
                            self.toggle_overview();
                        }
                    }
                    ThreeFingerSwipeAction::CloseOverview => {
                        if self.overview_active {
                            self.close_overview();
                        }
                    }
                }
            }
            return true;
        }
        self.touchpad_swipe = TouchpadSwipeState::default();
        false
    }

    /// Steps the active workspace by `delta` clamped cleanly within `WorkspaceId::MIN..=WorkspaceId::MAX`.
    pub fn switch_workspace_relative(&mut self, delta: i8, serial: Serial) {
        let current = self.wm.active_workspace_id();
        let next_idx = (i16::from(current.0) + i16::from(delta))
            .clamp(i16::from(WorkspaceId::MIN), i16::from(WorkspaceId::MAX));
        let target = WorkspaceId::new_clamped(u8::try_from(next_idx).unwrap_or(WorkspaceId::MIN));
        if target != current {
            self.apply_wm_action(WmAction::SwitchWorkspace(target), serial);
        }
    }

    /// Notifies `ext_idle_notify_v1` subscribers of seat user activity while honoring live
    /// `zwp_idle_inhibit_manager_v1` surface inhibitors (`RUSTYOS-045`).
    pub fn notify_user_activity(&mut self) {
        self.idle_inhibitor_surfaces.retain(IsAlive::alive);
        self.idle_notifier_state
            .set_is_inhibited(!self.idle_inhibitor_surfaces.is_empty());
        self.idle_notifier_state.notify_activity(&self.seat);
    }

    /// Confirms any pending `ext_session_lock_v1` lock request once outputs have cleared and
    /// presented a locked frame (`RUSTYOS-045`).
    pub fn confirm_pending_session_lock(&mut self) {
        if let Some(locker) = self.pending_session_locker.take() {
            locker.lock();
        }
    }

    /// Returns the alive `ext_session_lock_v1` surface associated with `output`, if any (`RUSTYOS-045`).
    #[must_use]
    pub fn ext_lock_surface_for_output(&self, output: &Output) -> Option<LockSurface> {
        self.ext_lock_surfaces
            .iter()
            .rev()
            .find_map(|(out, surf)| (out == output && surf.alive()).then(|| surf.clone()))
    }

    /// Resolves the `ext_session_lock_v1` surface under `point` and its logical output origin (`RUSTYOS-045`).
    #[must_use]
    pub fn ext_lock_surface_under(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let entry = self
            .output_map
            .output_under(point)
            .and_then(|id| self.output_map.get(id))
            .unwrap_or_else(|| self.output_map.primary());
        let lock_surf = self.ext_lock_surface_for_output(&entry.output)?;
        Some((lock_surf.wl_surface().clone(), entry.location.to_f64()))
    }

    /// Reconfigures active `ext_session_lock_v1` surfaces to match updated output logical sizes (`RUSTYOS-045`).
    pub fn reconfigure_ext_lock_surfaces(&mut self) {
        self.ext_lock_surfaces.retain(|(_, surf)| surf.alive());
        for (output, lock_surf) in &self.ext_lock_surfaces {
            let size = self.logical_u32_size_for_output(output);
            lock_surf.with_pending_state(|s| s.size = Some(size));
            lock_surf.send_configure();
        }
    }

    pub(in crate::state) fn logical_u32_size_for_output(
        &self,
        output: &Output,
    ) -> Size<u32, Logical> {
        let entry = self
            .output_map
            .entries()
            .iter()
            .find(|e| &e.output == output)
            .unwrap_or_else(|| self.output_map.primary());
        let geo = entry.logical_geometry();
        Size::from((
            u32::try_from(geo.size.w.max(1)).unwrap_or(1),
            u32::try_from(geo.size.h.max(1)).unwrap_or(1),
        ))
    }

    /// Focuses and raises the window owning `surface` (`xdg_activation_v1`, `RUSTYOS-045`).
    pub fn activate_window_surface(&mut self, surface: &WlSurface, serial: Serial) -> bool {
        if self.is_session_locked() {
            return false;
        }
        let mut root = surface.clone();
        while let Some(parent) = compositor::get_parent(&root) {
            root = parent;
        }
        let Some(win_id) = self.window_bindings.iter().find_map(|(_, win)| {
            WaylandFocus::wl_surface(win)
                .is_some_and(|ws| *ws == root)
                .then(|| self.window_id_for_smithay(win))
                .flatten()
        }) else {
            return false;
        };
        self.minimized_window_ids.remove(&win_id);
        if let Some(target_ws) = self.wm.window(win_id).map(|w| w.workspace)
            && target_ws != self.wm.active_workspace_id()
        {
            self.wm.switch_workspace(target_ws);
        }
        let focused = self.wm.focus_and_raise(win_id);
        self.sync_space_with_wm(serial);
        focused
    }

    /// Computes global logical bounds for a mapped toplevel or layer `surface` (`RUSTYOS-044`).
    #[must_use]
    pub fn surface_logical_bounds(&self, surface: &WlSurface) -> Option<Rectangle<f64, Logical>> {
        let mut root = surface.clone();
        while let Some(parent) = compositor::get_parent(&root) {
            root = parent;
        }
        for (_, window) in &self.window_bindings {
            if WaylandFocus::wl_surface(window).is_some_and(|ws| *ws == root)
                && let Some(win_id) = self.window_id_for_smithay(window)
                && let Some(wm_win) = self.wm.window(win_id)
            {
                let rect = wm_win.geometry;
                let titlebar_h = self.window_titlebar_height(win_id);
                let (cw, ch) = self.effective_client_size(win_id, rect);
                return Some(Rectangle::new(
                    Point::from((f64::from(rect.x), f64::from(rect.y + titlebar_h))),
                    Size::from((f64::from(cw.max(1)), f64::from(ch.max(1)))),
                ));
            }
        }
        for entry in self.output_map.entries() {
            let layer_map = layer_map_for_output(&entry.output);
            if let Some(layer) = layer_map.layer_for_surface(&root, WindowSurfaceType::ALL)
                && let Some(geo) = layer_map.layer_geometry(layer)
            {
                return Some(Rectangle::new(
                    (entry.location + geo.loc).to_f64(),
                    geo.size.to_f64(),
                ));
            }
        }
        None
    }

    /// Deactivates any active pointer constraint on the current pointer focus, applying any
    /// client `cursor_position_hint` when unlocking a [`LockedPointer`] (`RUSTYOS-044`).
    pub fn deactivate_active_pointer_constraint(&mut self) {
        let Some(focused) = self.pointer.current_focus() else {
            return;
        };
        let mut warp_hint: Option<Point<f64, Logical>> = None;
        with_pointer_constraint(&focused, &self.pointer, |constraint| {
            if let Some(c) = constraint
                && c.is_active()
            {
                if let PointerConstraint::Locked(locked) = &*c {
                    warp_hint = locked.cursor_position_hint();
                }
                c.deactivate();
            }
        });
        if let Some(hint) = warp_hint
            && let Some(bounds) = self.surface_logical_bounds(&focused)
        {
            let total = self.total_output_bounds();
            self.pointer_location = Point::from((
                (bounds.loc.x + hint.x).clamp(
                    f64::from(total.loc.x),
                    f64::from(total.loc.x + total.size.w - 1).max(f64::from(total.loc.x)),
                ),
                (bounds.loc.y + hint.y).clamp(
                    f64::from(total.loc.y),
                    f64::from(total.loc.y + total.size.h - 1).max(f64::from(total.loc.y)),
                ),
            ));
        }
    }

    /// Drains queued `wp_presentation_feedback` callbacks for surfaces displayed on `output`
    /// (`RUSTYOS-044`).
    #[must_use]
    pub fn take_presentation_feedback(
        &self,
        output: &Output,
        render_element_states: &RenderElementStates,
    ) -> OutputPresentationFeedback {
        let mut feedback = OutputPresentationFeedback::new(output);
        if self.ext_session_locked {
            if let Some(lock_surf) = self.ext_lock_surface_for_output(output) {
                take_presentation_feedback_surface_tree(
                    lock_surf.wl_surface(),
                    &mut feedback,
                    |_, _| Some(output.clone()),
                    |surf, _| {
                        surface_presentation_feedback_flags_from_states(surf, render_element_states)
                    },
                );
            }
            return feedback;
        }
        for window in self.space.elements() {
            if self
                .space
                .outputs_for_element(window)
                .into_iter()
                .any(|out| &out == output)
            {
                window.take_presentation_feedback(
                    &mut feedback,
                    |_, _| Some(output.clone()),
                    |surf, _| {
                        surface_presentation_feedback_flags_from_states(surf, render_element_states)
                    },
                );
            }
        }
        let layer_map = layer_map_for_output(output);
        for layer in layer_map.layers() {
            layer.take_presentation_feedback(
                &mut feedback,
                |_, _| Some(output.clone()),
                |surf, _| {
                    surface_presentation_feedback_flags_from_states(surf, render_element_states)
                },
            );
        }
        feedback
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
        if let Some(gid) = removed.global_id {
            self.display_handle.remove_global::<Self>(gid);
        }
        self.wm.remove_output(id);

        if removed.is_primary {
            self.output = self.output_map.primary().output.clone();
        }

        self.reconfigure_ext_lock_surfaces();
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

    /// Registers a version-3 `zwp_linux_dmabuf_v1` global with explicit GPU formats (`RUSTYOS-041`).
    pub fn init_dmabuf_formats(&mut self, formats: impl IntoIterator<Item = Format>) {
        if self.dmabuf_global.is_some() {
            return;
        }
        let global = self
            .dmabuf_state
            .create_global::<Self>(&self.display_handle, formats);
        self.dmabuf_global = Some(global);
        info!("Registered zwp_linux_dmabuf_v1 global (v3 format list)");
    }

    /// Registers a version-4 `zwp_linux_dmabuf_v1` global with DRM default tranche feedback (`RUSTYOS-041`).
    pub fn init_dmabuf_feedback(&mut self, feedback: &DmabufFeedback) {
        if self.dmabuf_global.is_some() {
            return;
        }
        let global = self
            .dmabuf_state
            .create_global_with_default_feedback::<Self>(&self.display_handle, feedback);
        self.dmabuf_global = Some(global);
        info!("Registered zwp_linux_dmabuf_v1 global with default DRM feedback");
    }

    pub(in crate::state) fn env_flag_enabled(name: &str) -> bool {
        std::env::var(name)
            .is_ok_and(|val| matches!(val.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
    }

    /// Returns whether explicit synchronization is explicitly disabled via `RUSTYOS_DISABLE_EXPLICIT_SYNC` (`RUSTYOS-046`).
    #[must_use]
    pub fn explicit_sync_disabled_by_env() -> bool {
        Self::env_flag_enabled("RUSTYOS_DISABLE_EXPLICIT_SYNC")
    }

    /// Returns whether DRM hardware cursor planes are explicitly disabled via `RUSTYOS_DISABLE_HW_CURSOR` (`RUSTYOS-046`).
    #[must_use]
    pub fn hw_cursor_disabled_by_env() -> bool {
        Self::env_flag_enabled("RUSTYOS_DISABLE_HW_CURSOR")
    }

    /// Registers `wp_linux_drm_syncobj_v1` if `drm_fd` supports syncobj eventfd and explicit sync is enabled (`RUSTYOS-046`).
    pub fn init_drm_syncobj(&mut self, drm_fd: &DrmDeviceFd) -> bool {
        if self.syncobj_state.is_some() {
            return true;
        }
        if Self::explicit_sync_disabled_by_env() {
            info!(
                "Explicit sync disabled by RUSTYOS_DISABLE_EXPLICIT_SYNC; falling back to implicit sync"
            );
            return false;
        }
        if !supports_syncobj_eventfd(drm_fd) {
            info!("DRM device does not support syncobj eventfd; falling back to implicit sync");
            return false;
        }
        self.syncobj_state = Some(DrmSyncobjState::new::<Self>(
            &self.display_handle,
            drm_fd.clone(),
        ));
        info!("Registered wp_linux_drm_syncobj_v1 global for explicit sync");
        true
    }

    /// Returns whether `wp_linux_drm_syncobj_v1` is currently active on this compositor (`RUSTYOS-046`).
    #[must_use]
    pub const fn is_explicit_sync_active(&self) -> bool {
        self.syncobj_state.is_some()
    }

    /// Consumes and returns whether an explicit-sync blocker cleared since the last check (`RUSTYOS-046`).
    #[must_use]
    pub const fn take_syncobj_unblocked(&mut self) -> bool {
        std::mem::replace(&mut self.syncobj_surface_unblocked, false)
    }

    pub(in crate::state) fn handle_syncobj_pre_commit(
        &mut self,
        _dh: &DisplayHandle,
        surface: &WlSurface,
    ) {
        if self.syncobj_state.is_none() {
            return;
        }
        let acquire_point = compositor::with_states(surface, |states| {
            let has_new_buffer = matches!(
                states
                    .cached_state
                    .get::<compositor::SurfaceAttributes>()
                    .pending()
                    .buffer,
                Some(compositor::BufferAssignment::NewBuffer(_))
            );
            if !has_new_buffer {
                return None;
            }
            let mut guard = states.cached_state.get::<DrmSyncobjCachedState>();
            let pending = guard.pending();
            match (
                pending.acquire_point.as_ref(),
                pending.release_point.as_ref(),
            ) {
                (Some(acq), Some(_rel)) if !acq.is_signaled() => Some(acq.clone()),
                _ => None,
            }
        });

        let Some(acquire) = acquire_point else {
            return;
        };

        let (blocker, source) = match acquire.generate_blocker() {
            Ok(pair) => pair,
            Err(err) => {
                warn!(error = %err, "Failed to create DrmSyncPoint blocker; falling back to immediate commit");
                return;
            }
        };

        let weak_surface = surface.downgrade();
        if let Err(err) = self
            .loop_handle
            .insert_source(source, move |(), _metadata, app_state| {
                if let Ok(surf) = weak_surface.upgrade()
                    && let Some(client) = surf.client()
                {
                    let dh = app_state.display_handle.clone();
                    app_state
                        .client_compositor_state(&client)
                        .blocker_cleared(app_state, &dh);
                    app_state.syncobj_surface_unblocked = true;
                }
                Ok(())
            })
        {
            warn!(error = %err, "Failed to register DrmSyncPointSource; proceeding without commit blocker");
            return;
        }

        compositor::add_blocker(surface, blocker);
    }

    /// Returns whether `win_id` is currently in true unbordered fullscreen mode (`RUSTYOS-041`).
    #[must_use]
    pub fn is_window_fullscreen(&self, win_id: WindowId) -> bool {
        self.wm.window(win_id).is_some_and(|w| w.fullscreen)
    }

    /// Returns the topmost unminimized fullscreen window on the primary output if no modal
    /// shell overlay currently occludes it (`RUSTYOS-041`).
    #[must_use]
    pub fn active_unoccluded_fullscreen_window(&self) -> Option<WindowId> {
        self.active_unoccluded_fullscreen_window_on(self.output_map.primary().id)
    }

    /// Returns whether any `Layer::Overlay` or `Layer::Top` layer-shell surface is mapped on `output` (`RUSTYOS-060`).
    #[must_use]
    pub fn has_foreground_layer_surfaces_on(&self, output: &Output) -> bool {
        let layer_map = layer_map_for_output(output);
        layer_map.layers_on(Layer::Overlay).next().is_some()
            || layer_map.layers_on(Layer::Top).next().is_some()
    }

    /// Returns whether any `Layer::Overlay` or `Layer::Top` layer-shell surface is mapped on the primary output (`RUSTYOS-060`).
    #[must_use]
    pub fn has_foreground_layer_surfaces(&self) -> bool {
        self.has_foreground_layer_surfaces_on(&self.output_map.primary().output)
    }

    /// Returns the topmost unminimized fullscreen window on `output_id`, ignoring shell overlays (`RUSTYOS-060`).
    #[must_use]
    pub fn topmost_fullscreen_window_on(&self, output_id: OutputId) -> Option<WindowId> {
        self.wm
            .visible_windows_for_output(output_id)
            .into_iter()
            .rev()
            .find(|w| !self.is_window_minimized(w.id))
            .filter(|w| w.fullscreen)
            .map(|w| w.id)
    }

    /// Returns whether persistent desktop chrome (`TopBar` / `Dock`) should stay hidden for a
    /// fullscreen window on `output_id` unless an interactive shell overlay explicitly requests chrome (`RUSTYOS-060`).
    #[must_use]
    pub fn should_hide_shell_chrome_for_fullscreen_on(&self, output_id: OutputId) -> bool {
        if self.is_session_locked() {
            return true;
        }
        if output_id == self.output_map.primary().id {
            let interactive_shell_open = self.launcher_active
                || self.quick_settings_active
                || self.overview_active
                || self.settings_active
                || self.launcher_anim.current() > 0.01
                || self.quick_settings_anim.current() > 0.01
                || self.overview_anim.current() > 0.01;
            if interactive_shell_open {
                return false;
            }
        }
        self.topmost_fullscreen_window_on(output_id).is_some()
    }

    /// Returns whether persistent desktop chrome on the primary output should stay hidden (`RUSTYOS-060`).
    #[must_use]
    pub fn should_hide_shell_chrome_for_fullscreen(&self) -> bool {
        self.should_hide_shell_chrome_for_fullscreen_on(self.output_map.primary().id)
    }

    /// Returns the topmost unminimized fullscreen window on `output_id` if no active overlay
    /// occludes that display (`RUSTYOS-042`, `RUSTYOS-060`).
    #[must_use]
    pub fn active_unoccluded_fullscreen_window_on(&self, output_id: OutputId) -> Option<WindowId> {
        if self.is_session_locked() || self.lock_screen_anim.current() > 0.01 {
            return None;
        }
        if output_id == self.output_map.primary().id {
            let overlay_active = self.launcher_active
                || self.quick_settings_active
                || self.overview_active
                || self.settings_active
                || self.window_switcher.is_some()
                || self.polkit_prompt.is_some()
                || self.portal_prompt.is_some()
                || self.portal_pick_color.is_some()
                || self.osd.is_some()
                || !self.notifications.is_empty()
                || self.launcher_anim.current() > 0.01
                || self.quick_settings_anim.current() > 0.01
                || self.overview_anim.current() > 0.01;
            if overlay_active {
                return None;
            }
        }

        self.topmost_fullscreen_window_on(output_id)
    }

    pub(in crate::state) fn init_seat_keyboard(
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
        if self.active_kbd_layout != label {
            self.active_kbd_layout = label;
            self.shell_redraw_needed = true;
        }
    }

    /// Cycles forward to the next configured XKB layout (`us` ↔ `ru`, etc.) and updates `TopBar`.
    pub fn cycle_keyboard_layout(&mut self) {
        let keyboard = self.keyboard.clone();
        keyboard.with_xkb_state(self, |mut ctx| {
            ctx.cycle_next_layout();
        });
        self.refresh_keyboard_layout_label();
        self.shell_redraw_needed = true;
        info!(layout = %self.active_kbd_layout, "Switched active keyboard layout");
    }

    /// Returns the elapsed duration since compositor startup.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

/// Minimum accumulated logical pointer delta required to trigger a 3-finger compositor swipe (`RUSTYOS-047`).
pub const THREE_FINGER_SWIPE_THRESHOLD: f64 = 80.0;

/// Classifies a completed 3-finger swipe `(dx, dy)` delta into a workspace or Overview action (`RUSTYOS-047`).
#[must_use]
pub fn classify_three_finger_swipe(dx: f64, dy: f64) -> Option<ThreeFingerSwipeAction> {
    if !dx.is_finite() || !dy.is_finite() {
        return None;
    }
    if dx.abs() >= dy.abs() {
        if dx.abs() >= THREE_FINGER_SWIPE_THRESHOLD {
            Some(if dx < 0.0 {
                ThreeFingerSwipeAction::NextWorkspace
            } else {
                ThreeFingerSwipeAction::PreviousWorkspace
            })
        } else {
            None
        }
    } else if dy.abs() >= THREE_FINGER_SWIPE_THRESHOLD {
        Some(if dy < 0.0 {
            ThreeFingerSwipeAction::OpenOverview
        } else {
            ThreeFingerSwipeAction::CloseOverview
        })
    } else {
        None
    }
}

/// Computes the clamped global logical origin `(x, y)` for an IME `PopupSurface` relative to its
/// parent surface geometry and client `text_input_rectangle` (`RUSTYOS-047`).
#[must_use]
pub fn compute_ime_popup_origin(
    parent_geo: Rectangle<i32, Logical>,
    cursor_rect: Rectangle<i32, Logical>,
    popup_size: Size<i32, Logical>,
    output_geo: Rectangle<i32, Logical>,
) -> Point<i32, Logical> {
    let cursor_global_x = parent_geo.loc.x + cursor_rect.loc.x;
    let cursor_global_y = parent_geo.loc.y + cursor_rect.loc.y;
    let cursor_h = cursor_rect.size.h.max(0);

    let out_left = output_geo.loc.x;
    let out_top = output_geo.loc.y;
    let out_w = output_geo.size.w.max(1);
    let out_h = output_geo.size.h.max(1);
    let pop_w = popup_size.w.max(0);
    let pop_h = popup_size.h.max(0);

    let below_y = cursor_global_y + cursor_h;
    let y = if pop_h > 0 && below_y + pop_h > out_top + out_h && cursor_global_y - pop_h >= out_top
    {
        cursor_global_y - pop_h
    } else {
        below_y.clamp(out_top, (out_top + out_h - pop_h).max(out_top))
    };
    let x = cursor_global_x.clamp(out_left, (out_left + out_w - pop_w).max(out_left));
    Point::from((x, y))
}

#[cfg(test)]
mod tests;
