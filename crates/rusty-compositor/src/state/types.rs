//! Auxiliary state structs and enums owned by [`super::RustyCompositorState`].

use std::time::{Duration, Instant};

use rusty_animation::AnimatedValue;
use rusty_ipc::Urgency;
use rusty_wm::{OutputId, Rect, WindowId};
use smithay::{
    reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason},
    utils::{Logical, Point, Size},
    wayland::compositor::CompositorClientState,
};
use tracing::debug;

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

/// Active mouse-driven window drag state via SSD titlebar or client `xdg_toplevel::move` (`RUSTYOS-019` / `RUSTYOS-166`).
#[derive(Debug, Clone, Copy)]
pub struct WindowDragState {
    pub window_id: WindowId,
    pub start_pointer: Point<f64, Logical>,
    pub start_origin: (i32, i32),
    /// `true` when the drag began while the window was maximized (enables 5px drag threshold + proportional anchor).
    pub started_maximized: bool,
    /// Original window width at drag start for proportional cursor anchoring on unmaximize.
    pub start_width: i32,
    /// `true` once a maximized drag passed the unmaximize threshold and anchored under the pointer.
    pub unmaximized_during_drag: bool,
    /// Active Edge/Corner Snap preview `(output_id, target, preview_rect)` shown during drag (`RUSTYOS-166`).
    pub snap_preview: Option<(OutputId, rusty_wm::SnapTarget, Rect)>,
}

impl WindowDragState {
    /// Constructs a new [`WindowDragState`] capturing the window's initial origin, width, and maximized state.
    #[must_use]
    pub const fn new(
        window_id: WindowId,
        start_pointer: Point<f64, Logical>,
        win: &rusty_wm::ManagedWindow,
    ) -> Self {
        Self {
            window_id,
            start_pointer,
            start_origin: (win.geometry.x, win.geometry.y),
            started_maximized: win.maximized,
            start_width: win.geometry.width,
            unmaximized_during_drag: false,
            snap_preview: None,
        }
    }
}

/// Active mouse-driven window resize state via SSD borders/corners (`RUSTYOS-022`).
#[derive(Debug, Clone, Copy)]
pub struct WindowResizeState {
    pub window_id: WindowId,
    pub edges: crate::shell_render::ResizeEdges,
    pub start_pointer: Point<f64, Logical>,
    pub start_rect: Rect,
}

/// Recently closed unpinned application entry retained for the Dock Recent section (`RUSTYOS-073`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentDockApp {
    pub app_key: String,
    pub label: String,
    pub icon_name: Option<String>,
}

/// Active launch bounce animation tracked per canonical Dock app key (`RUSTYOS-073`).
#[derive(Debug, Clone, PartialEq)]
pub struct DockBounceState {
    pub app_key: String,
    pub started_at: std::time::Duration,
    pub elapsed_ms: f64,
    pub continuous: bool,
}

/// Active pointer drag state inside the Dock for reordering/unpinning (`RUSTYOS-073` / `RUSTYOS-161` / `RUSTYOS-162`).
#[derive(Debug, Clone)]
pub struct DockDragState {
    pub output_id: OutputId,
    pub slot_idx: usize,
    pub app_key: String,
    pub source_pinned_idx: Option<usize>,
    pub source_minimized_win: Option<WindowId>,
    pub source_minimized_idx: Option<usize>,
    pub start_pointer: Point<f64, Logical>,
    pub current_pointer: Point<f64, Logical>,
    pub active_drag: bool,
    pub target_pinned_idx: usize,
    pub target_minimized_idx: usize,
    pub pulled_outside: bool,
    pub hit: crate::shell_render::DockHit,
}

/// Interactive open `Downloads` Stack popover state (`RUSTYOS-073` / `RUSTYOS-161`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockStackState {
    pub output_id: OutputId,
    pub title: String,
    pub anchor_rect: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
    pub entries: Vec<crate::shell_render::DockStackEntry>,
    pub hovered_item: Option<usize>,
    pub hovered_open_in_files: bool,
    pub selected_idx: Option<usize>,
}

/// Per-output Dock visibility, occlusion, auto-hide delay, and slot presentation state (`RUSTYOS-161`).
#[derive(Debug)]
pub struct OutputDockState {
    /// Animated reveal progress (`0.0` hidden offscreen -> `1.0` revealed).
    pub reveal_anim: AnimatedValue,
    /// True when the pointer is within this output's reveal strip, near the Dock, or inside its Stack.
    pub hovered: bool,
    /// True when a visible or maximized window on this output currently obstructs the resting Dock area.
    pub obstructed: bool,
    /// Optional 200 ms hide delay deadline armed when the pointer leaves an auto-hidden Dock.
    pub hide_deadline: Option<std::time::Duration>,
    /// Smooth per-output slot, divider, and capsule length presentation animator.
    pub presentation: crate::shell_render::DockPresentation,
}

impl OutputDockState {
    /// Creates an initial per-output Dock state with the requested reveal progress.
    #[must_use]
    pub fn new(initial_reveal: f64) -> Self {
        Self {
            reveal_anim: AnimatedValue::new_settled(initial_reveal),
            hovered: false,
            obstructed: false,
            hide_deadline: None,
            presentation: crate::shell_render::DockPresentation::new(),
        }
    }
}

/// Grace window tracking an in-flight graceful Quit request for a Dock application (`RUSTYOS-161`).
#[derive(Debug, Clone)]
pub struct PendingAppQuit {
    pub app_key: String,
    pub label: String,
    pub started_at: std::time::Duration,
    pub grace: std::time::Duration,
    pub notified_timeout: bool,
}

/// Cached desktop metadata for a pinned Dock application, preserving icons/labels across
/// temporary `.desktop` removals during package upgrades (`3000 ms` grace) (`RUSTYOS-161`).
#[derive(Debug, Clone)]
pub struct CachedDockPinMeta {
    pub icon_name: Option<String>,
    pub label: String,
    pub command: Option<String>,
    pub missing_since: Option<std::time::Duration>,
}

/// Auto-hide pointer-leave delay (`200 ms`) matching smooth desktop shelf semantics (`RUSTYOS-161`).
pub const DOCK_AUTO_HIDE_DELAY: std::time::Duration = std::time::Duration::from_millis(200);
/// Graceful quit window (`3000 ms`) before surfacing Force Quit escalation (`RUSTYOS-161`).
pub const DOCK_QUIT_GRACE: std::time::Duration = std::time::Duration::from_millis(3000);
/// Package-upgrade grace window (`3000 ms`) preserving pinned icon metadata if a `.desktop` file vanishes briefly (`RUSTYOS-161`).
pub const DOCK_PIN_UNINSTALL_GRACE: std::time::Duration = std::time::Duration::from_millis(3000);

/// Active per-window GLES Genie / Scale minimize or restore animation (`RUSTYOS-073`).
#[derive(Debug, Clone)]
pub struct WindowMinimizeAnimState {
    pub win_id: WindowId,
    pub restoring: bool,
    pub progress: AnimatedValue,
    pub window_rect: Rect,
    pub dock_rect: Rect,
    pub snapshot_pending: bool,
}

/// Direction of a window open/close lifecycle animation (`RUSTYOS-128`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowLifecycleKind {
    Open,
    Close,
}

/// Active per-window open or close scale+fade lifecycle animation (`RUSTYOS-128`).
#[derive(Debug, Clone)]
pub struct WindowLifecycleAnimState {
    pub win_id: WindowId,
    pub window_rect: Rect,
    pub kind: WindowLifecycleKind,
    pub focused: bool,
    pub snapshot_pending: bool,
    pub started_after_commit: bool,
    pub progress: AnimatedValue,
}

/// Active drag-to-workspace session for a window card inside `Workspace Overview` (`RUSTYOS-167`).
#[derive(Debug, Clone, PartialEq)]
pub struct OverviewCardDragState {
    /// Window being dragged across Overview.
    pub window_id: WindowId,
    /// Card index at the start of the drag.
    pub card_index: usize,
    /// Logical pointer location where LMB was pressed.
    pub start_pos: Point<f64, Logical>,
    /// Latest logical pointer location.
    pub current_pos: Point<f64, Logical>,
    /// True once pointer moved beyond the drag threshold (`>= 6 px`).
    pub dragging: bool,
    /// Destination workspace (`1..=9`) currently hovered in the top Spaces strip.
    pub target_workspace: Option<u8>,
}

/// Active `Alt+Tab` window switcher HUD state (`RUSTYOS-033`, `RUSTYOS-167`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSwitcherState {
    /// Active workspace window IDs in presentation order (unminimized MRU first, then minimized).
    pub window_ids: Vec<WindowId>,
    /// Currently highlighted entry index inside `window_ids`.
    pub selected: usize,
    /// Card index currently hovered by the pointer, if any (`RUSTYOS-167`).
    pub hovered: Option<usize>,
    /// Card index whose close (`×`) button is currently hovered, if any (`RUSTYOS-167`).
    pub hovered_close: Option<usize>,
    /// Window that held keyboard focus before the switcher opened (`RUSTYOS-167`).
    pub previous_focus: Option<WindowId>,
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

/// Decoded avatar image for notification sender (`RUSTYOS-173`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAvatar {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Active notification banner tracked by the compositor (`RUSTYOS-024` / `RUSTYOS-173`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositorNotification {
    /// Unique monotonic identifier.
    pub id: u64,
    /// Optional freedesktop D-Bus notification ID for `replaces_id` / `CloseNotification`.
    pub dbus_id: Option<u32>,
    /// Canonical Dock application key associated with this notification for unread badges (`RUSTYOS-162`).
    pub app_key: Option<String>,
    /// Optional human-readable application display name (`"Telegram Desktop"`, `"Google Chrome"`, ...).
    pub app_display_name: Option<String>,
    /// Notification urgency level.
    pub urgency: Urgency,
    /// Short notification title (e.g. sender contact name or subject).
    pub title: String,
    /// Notification body text.
    pub body: String,
    /// Optional contact avatar pixmap provided via D-Bus hints.
    pub avatar: Option<NotificationAvatar>,
    /// Monotonic creation offset from compositor startup (`start_time.elapsed()`).
    pub created_at: Duration,
    /// Auto-dismiss time-to-live.
    pub ttl: Duration,
}
