//! Core Wayland compositor state and protocol handler implementations.

use std::collections::{HashMap, HashSet};
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
    backend::{
        allocator::{Buffer as AllocatorBuffer, Format, dmabuf::Dmabuf},
        drm::DrmDeviceFd,
        renderer::{element::RenderElementStates, sync::Fence, utils::on_commit_buffer_handler},
    },
    delegate_compositor, delegate_cursor_shape, delegate_data_control, delegate_data_device,
    delegate_dmabuf, delegate_drm_syncobj, delegate_foreign_toplevel_list,
    delegate_fractional_scale, delegate_idle_inhibit, delegate_idle_notify,
    delegate_input_method_manager, delegate_keyboard_shortcuts_inhibit, delegate_layer_shell,
    delegate_output, delegate_pointer_constraints, delegate_pointer_gestures,
    delegate_presentation, delegate_primary_selection, delegate_relative_pointer, delegate_seat,
    delegate_session_lock, delegate_shm, delegate_text_input_manager, delegate_viewporter,
    delegate_xdg_activation, delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{
        LayerSurface as DesktopLayerSurface, PopupKind, PopupManager, Space, Window,
        WindowSurfaceType, find_popup_root_surface, get_popup_toplevel_coords,
        layer_map_for_output,
        utils::{
            OutputPresentationFeedback, bbox_from_surface_tree, send_frames_surface_tree,
            surface_presentation_feedback_flags_from_states,
            take_presentation_feedback_surface_tree,
        },
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
        wayland_protocols::xdg::toplevel_icon::v1::server::{
            xdg_toplevel_icon_manager_v1::{self, XdgToplevelIconManagerV1},
            xdg_toplevel_icon_v1::{self, XdgToplevelIconV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason, ObjectId},
            delegate_global_dispatch,
            protocol::{
                wl_buffer, wl_data_source::WlDataSource, wl_output::WlOutput, wl_seat,
                wl_surface::WlSurface,
            },
        },
    },
    utils::{Clock, IsAlive, Logical, Monotonic, Point, Rectangle, Serial, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            self, CompositorClientState, CompositorHandler, CompositorState, TraversalAction,
        },
        cursor_shape::CursorShapeManagerState,
        dmabuf::{DmabufFeedback, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        drm_syncobj::{
            DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState, supports_syncobj_eventfd,
        },
        foreign_toplevel_list::{
            ForeignToplevelHandle, ForeignToplevelListHandler, ForeignToplevelListState,
        },
        fractional_scale::{self, FractionalScaleHandler, FractionalScaleManagerState},
        idle_inhibit::{IdleInhibitHandler, IdleInhibitManagerState},
        idle_notify::{IdleNotifierHandler, IdleNotifierState},
        input_method::{
            InputMethodHandler, InputMethodManagerState, PopupSurface as ImePopupSurface,
        },
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState,
            KeyboardShortcutsInhibitor, KeyboardShortcutsInhibitorSeat as _,
        },
        output::{OutputHandler, OutputManagerState},
        pointer_constraints::{
            PointerConstraint, PointerConstraintsHandler, PointerConstraintsState,
            with_pointer_constraint,
        },
        pointer_gestures::PointerGesturesState,
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
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
        session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker},
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
        shm::{BufferAccessError, ShmHandler, ShmState, with_buffer_contents},
        tablet_manager::TabletSeatHandler,
        text_input::TextInputManagerState,
        viewporter::ViewporterState,
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
        xdg_toplevel_icon::{
            XdgToplevelIconHandler, XdgToplevelIconManager, XdgToplevelIconManagerUserData,
            XdgToplevelIconUserData,
        },
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

/// Double-buffered surface state for `xdg_toplevel_icon_v1`.
///
/// Snapshots icon metadata on `set_icon` so icon state survives `xdg_toplevel_icon_v1.destroy()`,
/// and bypasses Smithay 0.7.0's `ShmBufferUserData::remove_destruction_hook` (`!=`) bug.
#[derive(Debug, Clone, Default)]
struct ToplevelIconSurfaceState {
    icon_name: Option<String>,
    buffers: Vec<(wl_buffer::WlBuffer, i32)>,
}

impl compositor::Cacheable for ToplevelIconSurfaceState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

#[derive(Debug, Clone)]
struct ToplevelIconBufferEntry {
    buffer: wl_buffer::WlBuffer,
    width: i32,
    height: i32,
    scale: i32,
}

#[derive(Debug)]
struct ToplevelIconEntry {
    resource: XdgToplevelIconV1,
    immutable: bool,
    icon_name: Option<String>,
    buffers: Vec<ToplevelIconBufferEntry>,
}

impl ToplevelIconEntry {
    fn add_buffer_replacing_slot(
        &mut self,
        buffer: wl_buffer::WlBuffer,
        width: i32,
        height: i32,
        scale: i32,
    ) {
        if let Some(existing) = self
            .buffers
            .iter_mut()
            .find(|b| b.width == width && b.height == height && b.scale == scale)
        {
            existing.buffer = buffer;
        } else {
            self.buffers.push(ToplevelIconBufferEntry {
                buffer,
                width,
                height,
                scale,
            });
        }
    }

    fn snapshot_surface_state(&self) -> ToplevelIconSurfaceState {
        ToplevelIconSurfaceState {
            icon_name: self.icon_name.clone(),
            buffers: self
                .buffers
                .iter()
                .map(|b| (b.buffer.clone(), b.scale))
                .collect(),
        }
    }

    fn references_buffer(&self, buf_id: &ObjectId) -> bool {
        self.buffers.iter().any(|b| b.buffer.id() == *buf_id)
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
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&display_handle, "rusty-seat-0");

        let keyboard = Self::init_seat_keyboard(&mut seat, &user_config)?;
        let pointer = seat.add_pointer();

        let (output, primary_global) =
            Self::create_initial_output(&display_handle, initial_size, initial_scale);

        let mut space = Space::default();
        space.map_output(&output, (0, 0));

        let logical_size = crate::outputs::compute_logical_size(initial_size, initial_scale);
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
        );
        primary_entry.global_id = Some(primary_global);
        let output_map = crate::outputs::OutputMap::new(primary_entry);

        let initial_glass_opacity = user_config.glass_opacity;
        let initial_reduce_transparency = user_config.reduce_transparency;
        let initial_menubar_show_workspaces = user_config.menubar_show_workspaces;
        let initial_menubar_show_tray = user_config.menubar_show_tray;
        let initial_menubar_show_input_source = user_config.menubar_show_input_source;
        let initial_menubar_show_volume = user_config.menubar_show_volume;
        let initial_clock_24h = user_config.clock_24h;

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
            focus_mode: false,
            telemetry: crate::telemetry::SystemMetricsHandle::static_default(),
            screen_locked: false,
            lock_screen_anim: AnimatedValue::new_settled(0.0),
            lock_selected_user: 0,
            lock_power_menu_open: false,
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

    fn create_initial_output(
        display_handle: &DisplayHandle,
        initial_size: Size<i32, smithay::utils::Physical>,
        initial_scale: f64,
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
            Some(Transform::Flipped180),
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
        let output = Output::new(name_str.clone(), physical_props);
        let output_global = output.create_global::<Self>(&self.display_handle);
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
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
        let logical_size = crate::outputs::compute_logical_size(mode.size, scale_val);

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
        );
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

        self.reconfigure_ext_lock_surfaces();
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
        true
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
            || self.launcher_active
            || self.overview_active
            || self.quick_settings_active
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

    fn logical_u32_size_for_output(&self, output: &Output) -> Size<u32, Logical> {
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

    fn env_flag_enabled(name: &str) -> bool {
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

    fn handle_syncobj_pre_commit(&mut self, _dh: &DisplayHandle, surface: &WlSurface) {
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

    /// Returns the topmost unminimized fullscreen window on `output_id` if no active overlay
    /// occludes that display (`RUSTYOS-042`).
    #[must_use]
    pub fn active_unoccluded_fullscreen_window_on(&self, output_id: OutputId) -> Option<WindowId> {
        if self.is_session_locked() || self.lock_screen_anim.current() > 0.01 {
            return None;
        }
        if output_id == self.output_map.primary().id {
            let overlay_active = self.launcher_active
                || self.quick_settings_active
                || self.overview_active
                || self.window_switcher.is_some()
                || self.osd.is_some()
                || !self.notifications.is_empty()
                || self.launcher_anim.current() > 0.01
                || self.quick_settings_anim.current() > 0.01
                || self.overview_anim.current() > 0.01;
            if overlay_active {
                return None;
            }
        }

        self.wm
            .visible_windows_for_output(output_id)
            .into_iter()
            .rev()
            .find(|w| !self.is_window_minimized(w.id))
            .filter(|w| w.fullscreen)
            .map(|w| w.id)
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
            self.close_top_bar_menu();
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
        self.qs_slider_drag = None;
        if self.quick_settings_active {
            self.close_top_bar_menu();
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
        self.qs_slider_drag = None;
        self.retarget_quick_settings_anim();
    }

    /// Toggles a `TopBar` left menu dropdown popover (`RUSTYOS-048`).
    pub fn toggle_top_bar_menu(&mut self, menu: crate::shell_render::TopBarMenuHit) {
        if self.active_top_bar_menu == Some(menu) {
            self.close_top_bar_menu();
        } else {
            self.close_quick_settings();
            self.close_launcher();
            self.active_top_bar_menu = Some(menu);
            self.top_bar_menu_hover_row = None;
        }
    }

    /// Closes any open `TopBar` menu dropdown popover (`RUSTYOS-048`).
    pub fn close_top_bar_menu(&mut self) {
        self.active_top_bar_menu = None;
        self.top_bar_menu_hover_row = None;
    }

    /// Clears transient shell hover and active slider drag states (`RUSTYOS-051`).
    pub fn clear_shell_hover_states(&mut self) {
        self.set_dock_hover_slot(None);
        self.top_bar_menu_hover_row = None;
        self.qs_slider_drag = None;
        self.hovered_notification_idx = None;
        self.hovered_traffic_light_window = None;
    }

    /// Updates hovered Dock slot and triggers a 120 ms lift transition (`RUSTYOS-048`).
    pub fn set_dock_hover_slot(&mut self, next: Option<usize>) {
        if self.dock_hover_slot != next {
            self.dock_hover_slot = next;
            let now = self.elapsed();
            self.dock_hover_anim.retarget(
                if next.is_some() { 1.0 } else { 0.0 },
                now,
                Duration::from_millis(120),
                Easing::EaseOutCubic,
                self.animations.reduced_motion(),
            );
        }
    }

    /// Updates shell glass opacity clamped to `0.35..=1.0` (`RUSTYOS-048`).
    pub fn set_glass_opacity(&mut self, opacity: f32) {
        self.glass_opacity = crate::config::clamp_glass_opacity(opacity);
    }

    /// Toggles Focus (Do Not Disturb) notification popup suppression (`RUSTYOS-053`).
    pub fn toggle_focus_mode(&mut self) -> bool {
        self.focus_mode = !self.focus_mode;
        if self.focus_mode {
            self.hovered_notification_idx = None;
        }
        self.focus_mode
    }

    /// Toggles opaque high-contrast shell panels (`reduce_transparency`) (`RUSTYOS-048`).
    pub fn toggle_reduce_transparency(&mut self) {
        self.reduce_transparency = !self.reduce_transparency;
    }

    /// Executes an action chosen from an open `TopBar` menu dropdown (`RUSTYOS-048`).
    pub fn dispatch_top_bar_menu_action(
        &mut self,
        action: crate::shell_render::TopBarMenuAction,
        serial: Serial,
    ) {
        self.close_top_bar_menu();
        match action {
            crate::shell_render::TopBarMenuAction::AboutRustyOs => {
                let _ = self.push_notification(
                    Urgency::Low,
                    "RustyOS Desktop".to_owned(),
                    "Wayland compositor • Purple Glass UI".to_owned(),
                );
            }
            crate::shell_render::TopBarMenuAction::OpenOverview => self.toggle_overview(),
            crate::shell_render::TopBarMenuAction::OpenControlCenter => {
                self.toggle_quick_settings();
            }
            crate::shell_render::TopBarMenuAction::OpenSpotlight => self.toggle_launcher(),
            crate::shell_render::TopBarMenuAction::NewTerminal => {
                self.handle_dock_hit(crate::shell_render::DockHit::PinnedTerminal, serial);
            }
            crate::shell_render::TopBarMenuAction::OpenFiles => {
                self.handle_dock_hit(crate::shell_render::DockHit::PinnedFiles, serial);
            }
            crate::shell_render::TopBarMenuAction::ToggleLayout => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            crate::shell_render::TopBarMenuAction::ToggleReducedMotion => {
                let next = !self.animations.reduced_motion();
                self.set_reduced_motion_pref(next);
            }
            crate::shell_render::TopBarMenuAction::ToggleReduceTransparency => {
                self.toggle_reduce_transparency();
            }
            crate::shell_render::TopBarMenuAction::RefreshMetrics => {
                self.telemetry.request_refresh();
            }
            crate::shell_render::TopBarMenuAction::LockSession => {
                self.set_screen_locked(true, serial);
            }
            crate::shell_render::TopBarMenuAction::FocusNextWindow => {
                self.apply_wm_action(WmAction::FocusNext, serial);
            }
            crate::shell_render::TopBarMenuAction::NextWorkspace => {
                self.switch_workspace_relative(1, serial);
            }
            crate::shell_render::TopBarMenuAction::PrevWorkspace => {
                self.switch_workspace_relative(-1, serial);
            }
            crate::shell_render::TopBarMenuAction::ToggleMaximizeFocused => {
                self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
            }
            crate::shell_render::TopBarMenuAction::MinimizeFocused => {
                if let Some(focused) = self.wm.focused_window() {
                    self.minimize_window(focused, serial);
                }
            }
            crate::shell_render::TopBarMenuAction::CloseFocused => {
                self.apply_wm_action(WmAction::CloseFocused, serial);
            }
            crate::shell_render::TopBarMenuAction::ShowShortcuts => {
                let _ = self.push_notification(
                    Urgency::Low,
                    "RustyOS Shortcuts".to_owned(),
                    "Super+Space: Search • Super+Tab: Switcher • Super+1..9: Workspaces".to_owned(),
                );
            }
        }
    }

    /// Engages or disengages the interactive session Lock Screen with smooth transition (`RUSTYOS-025`).
    pub fn set_screen_locked(&mut self, locked: bool, serial: Serial) {
        self.screen_locked = locked;
        self.lock_power_menu_open = false;
        self.lock_selected_user = 0;
        if locked {
            self.window_switcher = None;
            self.dnd_icon = None;
            self.close_launcher();
            self.close_quick_settings();
            self.close_top_bar_menu();
            self.close_overview();
            self.active_window_drag = None;
            self.active_window_resize = None;
            self.clear_shell_hover_states();
            self.touchpad_swipe = TouchpadSwipeState::default();
            self.deactivate_focused_shortcut_inhibitor();
            self.telemetry.request_refresh();
            let keyboard = self.keyboard.clone();
            keyboard.set_focus(self, None, serial);
        } else {
            self.suppressed_inhibitor_surface = None;
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

    /// Toggles the Lock Screen top-right power popover (`RUSTYOS-054`).
    pub fn toggle_lock_power_menu(&mut self) {
        self.lock_power_menu_open = !self.lock_power_menu_open;
    }

    /// Selects an interactive user row index on the Lock Screen (`RUSTYOS-054`).
    pub fn select_lock_user(&mut self, idx: usize) {
        let user_count = self.telemetry.snapshot().users.len().max(1);
        self.lock_selected_user = idx.min(user_count - 1);
    }

    /// Updates accessibility `reduced_motion` and snaps in-flight shell transitions if enabled (`RUSTYOS-025`).
    pub fn set_reduced_motion_pref(&mut self, enabled: bool) {
        self.animations.set_reduced_motion(enabled);
        if enabled {
            self.overview_anim.snap_to_target();
            self.launcher_anim.snap_to_target();
            self.quick_settings_anim.snap_to_target();
            self.lock_screen_anim.snap_to_target();
            self.dock_hover_anim.snap_to_target();
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

    /// Returns the active frontmost application label shown beside the `RustyOS` emblem (`RUSTYOS-049`/`051`).
    #[must_use]
    pub fn active_top_bar_app_name(&self) -> String {
        self.wm
            .focused_window()
            .filter(|id| self.wm.visible_windows().iter().any(|w| w.id == *id))
            .map_or_else(
                || "RustyOS".to_owned(),
                |id| crate::shell_render::extract_app_name(&self.window_title(id)),
            )
    }

    /// Builds miniature Space entries (`Desktop 1..=9`) for Workspace Overview (`RUSTYOS-051`).
    #[must_use]
    pub fn overview_workspace_spaces(&self) -> Vec<crate::shell_render::OverviewWorkspaceSpace> {
        let primary_geo = self.output_map.primary().logical_geometry();
        let active_ws = self.wm.active_workspace_id().0;
        let top_bar = f32::from(i16::try_from(crate::shell_render::TOP_BAR_HEIGHT).unwrap_or(28));
        #[allow(clippy::cast_precision_loss)]
        let avail_w = primary_geo.size.w.max(320) as f32;
        #[allow(clippy::cast_precision_loss)]
        let avail_h = (primary_geo.size.h.max(240) as f32 - top_bar).max(120.0);

        (rusty_wm::WorkspaceId::MIN..=rusty_wm::WorkspaceId::MAX)
            .map(|ws_id| {
                let mut preview_rects = [(0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32); 4];
                let mut count = 0_usize;
                if let Some(ws) = self.wm.workspace(rusty_wm::WorkspaceId(ws_id)) {
                    for &win_id in &ws.stacking {
                        if self.is_window_minimized(win_id) {
                            continue;
                        }
                        if let Some(win) = self.wm.window(win_id) {
                            if count < preview_rects.len() {
                                #[allow(clippy::cast_precision_loss)]
                                let (rx, ry, rw, rh) = (
                                    (win.geometry.x - primary_geo.loc.x) as f32 / avail_w,
                                    ((win.geometry.y - primary_geo.loc.y) as f32 - top_bar)
                                        / avail_h,
                                    win.geometry.width as f32 / avail_w,
                                    win.geometry.height as f32 / avail_h,
                                );
                                let nx = rx.clamp(0.02, 0.85);
                                let ny = ry.clamp(0.02, 0.85);
                                let nw = rw.clamp(0.10, 0.96 - nx);
                                let nh = rh.clamp(0.10, 0.96 - ny);
                                preview_rects[count] = (nx, ny, nw, nh);
                            }
                            count += 1;
                        }
                    }
                }
                crate::shell_render::OverviewWorkspaceSpace {
                    id: ws_id,
                    window_count: count,
                    active: ws_id == active_ws,
                    preview_rects,
                }
            })
            .collect()
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

    fn dock_preferred_candidates(
        hit: crate::shell_render::DockHit,
    ) -> Option<&'static [&'static str]> {
        const FILES: &[&str] = &["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"];
        const TERMINAL: &[&str] = &[
            "foot",
            "kitty",
            "alacritty",
            "wezterm",
            "ghostty",
            "weston-terminal",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
        ];
        const BROWSER: &[&str] = &[
            "firefox",
            "chromium",
            "google-chrome",
            "brave-browser",
            "epiphany",
            "falkon",
        ];
        const NOTES: &[&str] = &[
            "gnome-text-editor",
            "gedit",
            "kate",
            "mousepad",
            "featherpad",
            "obsidian",
        ];
        match hit {
            crate::shell_render::DockHit::PinnedFiles => Some(FILES),
            crate::shell_render::DockHit::PinnedTerminal => Some(TERMINAL),
            crate::shell_render::DockHit::PinnedBrowser => Some(BROWSER),
            crate::shell_render::DockHit::PinnedNotes => Some(NOTES),
            _ => None,
        }
    }

    fn launch_dock_app_or_spotlight(&mut self, candidates: &[&str], ipc_path: &std::path::Path) {
        self.close_overview();
        if !self
            .launcher
            .launch_preferred(candidates, &self.socket_name, ipc_path)
        {
            self.toggle_launcher();
        }
    }

    /// Executes an interactive Dock click action (`RUSTYOS-023` / `RUSTYOS-032` / `RUSTYOS-050`).
    pub fn handle_dock_hit(&mut self, hit: crate::shell_render::DockHit, serial: Serial) {
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        if let Some(candidates) = Self::dock_preferred_candidates(hit) {
            self.launch_dock_app_or_spotlight(candidates, &ipc_path);
            return;
        }
        match hit {
            crate::shell_render::DockHit::PinnedSettings => {
                self.close_overview();
                if !self.launcher.launch_preferred(
                    &["rusty-settings", "gnome-control-center", "systemsettings"],
                    &self.socket_name,
                    &ipc_path,
                ) {
                    self.toggle_quick_settings();
                }
            }
            crate::shell_render::DockHit::PinnedDownloads => {
                self.close_overview();
                let opened = std::env::var_os("HOME")
                    .map(|h| std::path::PathBuf::from(h).join("Downloads"))
                    .filter(|p| p.exists())
                    .and_then(|p| p.to_str().map(str::to_owned))
                    .filter(|s| !s.chars().any(char::is_whitespace))
                    .is_some_and(|dir| {
                        self.launcher
                            .launch_command(
                                &format!("xdg-open {dir}"),
                                &self.socket_name,
                                &ipc_path,
                            )
                            .is_ok()
                    });
                if !opened {
                    self.launch_dock_app_or_spotlight(
                        &["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"],
                        &ipc_path,
                    );
                }
            }
            crate::shell_render::DockHit::PinnedTrash => {
                self.close_overview();
                let opened = self
                    .launcher
                    .launch_command("xdg-open trash:///", &self.socket_name, &ipc_path)
                    .is_ok();
                if !opened {
                    self.launch_dock_app_or_spotlight(
                        &["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"],
                        &ipc_path,
                    );
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
            _ => {}
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

    /// Looks up the [`WindowId`] matching the raw `xdg_toplevel` protocol resource (`RUSTYOS-047`).
    #[must_use]
    pub fn window_id_for_xdg_toplevel_proto(
        &self,
        xdg_tl: &xdg_toplevel::XdgToplevel,
    ) -> Option<WindowId> {
        self.window_bindings.iter().find_map(|(win_id, sw)| {
            sw.toplevel()
                .is_some_and(|tl| tl.xdg_toplevel() == xdg_tl)
                .then_some(*win_id)
        })
    }

    /// Clears decoration-negotiation tracking for a closed or unmapped window (`RUSTYOS-030`).
    pub fn unregister_window_decoration(&mut self, win_id: WindowId) {
        self.xdg_decoration_requested.remove(&win_id);
        self.csd_window_ids.remove(&win_id);
    }

    /// Releases decoration state, `ext_foreign_toplevel_list_v1` handle, and `xdg_toplevel_icon_v1`
    /// metadata for a closed or pruned window (`RUSTYOS-030` / `RUSTYOS-047`).
    pub fn unregister_window_metadata(&mut self, win_id: WindowId) {
        self.unregister_window_decoration(win_id);
        self.close_foreign_toplevel(win_id);
        self.window_toplevel_icons.remove(&win_id);
    }

    /// Resolves the live UI title for `win_id` (falling back to `app_id`, then `Window #N`) (`RUSTYOS-047`).
    #[must_use]
    pub fn window_title(&self, win_id: WindowId) -> String {
        let Some(window) = self.smithay_window_for_id(win_id) else {
            return format!("Window #{}", win_id.0);
        };
        if let Some(toplevel) = window.toplevel() {
            let title = compositor::with_states(toplevel.wl_surface(), |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|data| data.lock().ok())
                    .and_then(|attrs| attrs.title.clone().or_else(|| attrs.app_id.clone()))
            });
            if let Some(t) = title
                && !t.trim().is_empty()
            {
                return t;
            }
        } else if let Some(x11) = window.x11_surface() {
            let title = x11.title();
            if !title.trim().is_empty() {
                return title;
            }
        }
        format!("Window #{}", win_id.0)
    }

    fn window_protocol_title(&self, win_id: WindowId) -> String {
        let Some(window) = self.smithay_window_for_id(win_id) else {
            return String::new();
        };
        if let Some(toplevel) = window.toplevel() {
            return compositor::with_states(toplevel.wl_surface(), |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|data| data.lock().ok())
                    .and_then(|attrs| attrs.title.clone())
            })
            .unwrap_or_default();
        }
        if let Some(x11) = window.x11_surface() {
            return x11.title();
        }
        String::new()
    }

    /// Creates and announces an `ext_foreign_toplevel_handle_v1` for a newly mapped window (`RUSTYOS-047`).
    pub fn register_foreign_toplevel(&mut self, win_id: WindowId) {
        if self.foreign_toplevel_handles.contains_key(&win_id) {
            self.sync_foreign_toplevel_metadata(win_id);
            return;
        }
        let title = self.window_protocol_title(win_id);
        let app_id = self.window_app_id(win_id).unwrap_or_default();
        let handle = self
            .foreign_toplevel_list_state
            .new_toplevel::<Self>(title, app_id);
        self.foreign_toplevel_handles.insert(win_id, handle);
    }

    /// Emits updated `title`/`app_id` + `done` on `win_id`'s `ext_foreign_toplevel_handle_v1` (`RUSTYOS-047`).
    pub fn sync_foreign_toplevel_metadata(&self, win_id: WindowId) {
        let Some(handle) = self.foreign_toplevel_handles.get(&win_id) else {
            return;
        };
        let title = self.window_protocol_title(win_id);
        let app_id = self.window_app_id(win_id).unwrap_or_default();
        let mut changed = false;
        if handle.title() != title {
            handle.send_title(&title);
            changed = true;
        }
        if handle.app_id() != app_id {
            handle.send_app_id(&app_id);
            changed = true;
        }
        if changed {
            handle.send_done();
        }
    }

    /// Sends `closed` and retires the `ext_foreign_toplevel_handle_v1` owned by `win_id` (`RUSTYOS-047`).
    pub fn close_foreign_toplevel(&mut self, win_id: WindowId) {
        if let Some(handle) = self.foreign_toplevel_handles.remove(&win_id) {
            self.foreign_toplevel_list_state.remove_toplevel(&handle);
        }
    }

    /// Syncs committed `xdg_toplevel_icon_v1` `ToplevelIconCachedState` for `win_id` (`RUSTYOS-047`).
    pub fn sync_toplevel_icon_from_surface(&mut self, win_id: WindowId, surface: &WlSurface) {
        let (icon_name, buffers) = compositor::with_states(surface, |states| {
            let mut guard = states.cached_state.get::<ToplevelIconSurfaceState>();
            let cur = guard.current();
            (cur.icon_name.clone(), cur.buffers.clone())
        });
        if icon_name.is_none() && buffers.is_empty() {
            self.window_toplevel_icons.remove(&win_id);
            return;
        }
        if self
            .window_toplevel_icons
            .get(&win_id)
            .is_some_and(|existing| existing.icon_name == icon_name && existing.buffers == buffers)
        {
            return;
        }
        self.next_toplevel_icon_rev = self.next_toplevel_icon_rev.saturating_add(1);
        self.window_toplevel_icons.insert(
            win_id,
            crate::icons::ShmToplevelIcon {
                revision: self.next_toplevel_icon_rev,
                icon_name,
                buffers,
            },
        );
    }

    /// Removes dead IME popup surfaces (`RUSTYOS-047`).
    pub fn cleanup_ime_popups(&mut self) {
        self.ime_popups.retain(ImePopupSurface::alive);
    }

    /// Computes output-local physical render positions for alive IME `PopupSurface` instances belonging
    /// to `target_output` (returns nothing while the session is locked) (`RUSTYOS-047`).
    #[must_use]
    pub fn ime_popups_for_output(
        &self,
        target_output: &Output,
    ) -> Vec<(ImePopupSurface, Point<i32, Logical>)> {
        if self.is_session_locked() {
            return Vec::new();
        }
        let target_geo = self
            .output_map
            .find_by_output(target_output)
            .map(crate::outputs::OutputEntry::logical_geometry)
            .or_else(|| self.space.output_geometry(target_output))
            .unwrap_or_else(|| self.output_map.primary().logical_geometry());

        let mut out = Vec::new();
        for popup in &self.ime_popups {
            if !popup.alive() {
                continue;
            }
            let parent_geo = popup
                .get_parent()
                .map_or_else(|| target_geo, |p| self.parent_geometry(&p.surface));
            let cursor_rect = popup.text_input_rectangle();
            let anchor = Point::from((
                f64::from(parent_geo.loc.x + cursor_rect.loc.x),
                f64::from(parent_geo.loc.y + cursor_rect.loc.y),
            ));
            let owner_id = self
                .output_map
                .output_under(anchor)
                .unwrap_or(self.output_map.primary().id);
            let target_id = self
                .output_map
                .find_by_output(target_output)
                .map_or(self.output_map.primary().id, |e| e.id);
            if owner_id != target_id {
                continue;
            }

            let popup_size = bbox_from_surface_tree(popup.wl_surface(), (0, 0)).size;
            let global_loc =
                compute_ime_popup_origin(parent_geo, cursor_rect, popup_size, target_geo);
            popup.set_location(global_loc - parent_geo.loc);
            out.push((popup.clone(), global_loc - target_geo.loc));
        }
        out
    }

    /// Emits Wayland frame callbacks for alive IME popups visible on `output` (`RUSTYOS-047`).
    pub fn send_ime_popup_frames(&self, output: &Output, elapsed: Duration) {
        if self.is_session_locked() {
            return;
        }
        for (popup, _) in self.ime_popups_for_output(output) {
            send_frames_surface_tree(
                popup.wl_surface(),
                output,
                elapsed,
                Some(Duration::ZERO),
                |_, _| Some(output.clone()),
            );
        }
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
        if self.is_window_fullscreen(win_id) || self.window_uses_csd(win_id) {
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
            self.unregister_window_metadata(id);
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
                WmAction::CloseFocused
                    | WmAction::ToggleMaximizeFocused
                    | WmAction::ToggleFullscreen
                    | WmAction::SnapFocused(_)
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
        let visible: Vec<(WindowId, Rect, bool, bool)> = self
            .wm
            .visible_windows()
            .into_iter()
            .filter(|w| !self.is_window_minimized(w.id))
            .map(|w| (w.id, w.geometry, w.maximized, w.fullscreen))
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
                .is_some_and(|id| visible.iter().any(|(vid, _, _, _)| *vid == id));
            if !still_visible {
                self.space.unmap_elem(&sw);
            }
        }

        // 2. Map/update visible windows in bottom-to-top z-order and send pending configures.
        for (win_id, rect, maximized, fullscreen) in visible {
            let is_focused = focused_id == Some(win_id);
            let center = Point::from((
                f64::from(rect.x) + f64::from(rect.width) * 0.5,
                f64::from(rect.y) + f64::from(rect.height) * 0.5,
            ));
            let output_scale = self
                .output_map
                .output_under(center)
                .and_then(|id| self.output_map.get(id))
                .map_or_else(|| self.output_map.primary().scale, |e| e.scale);
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
                let has_ssd = !fullscreen && !self.window_uses_csd(win_id);
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
                        if maximized && !fullscreen {
                            state.states.set(xdg_toplevel::State::Maximized);
                        } else {
                            state.states.unset(xdg_toplevel::State::Maximized);
                        }
                        if fullscreen {
                            state.states.set(xdg_toplevel::State::Fullscreen);
                        } else {
                            state.states.unset(xdg_toplevel::State::Fullscreen);
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
                    let _ = x11.set_maximized(maximized && !fullscreen);
                    let _ = x11.set_fullscreen(fullscreen);
                }
                if let Some(root_ws) = WaylandFocus::wl_surface(&window) {
                    Self::apply_surface_scale(&root_ws, output_scale);
                }
                self.space.map_element(window, client_origin, is_focused);
            }
        }

        self.sync_keyboard_focus_with_wm(focused_id, serial);
    }

    fn sync_keyboard_focus_with_wm(&mut self, focused_id: Option<WindowId>, serial: Serial) {
        let focus_surface = if self.ext_session_locked {
            self.ext_lock_surfaces
                .iter()
                .rev()
                .find(|(_, ls)| ls.alive())
                .map(|(_, ls)| ls.wl_surface().clone())
        } else {
            focused_id
                .and_then(|id| self.smithay_window_for_id(id))
                .and_then(|w| WaylandFocus::wl_surface(&w).map(std::borrow::Cow::into_owned))
        };
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
        let _ = self.dock_hover_anim.tick(now);

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

impl SeatHandler for RustyCompositorState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        if let Some(prev) = self.active_shortcut_inhibitor.as_ref()
            && focused.is_none_or(|surf| prev.wl_surface() != surf)
        {
            prev.inactivate();
            self.active_shortcut_inhibitor = None;
        }
        if self.suppressed_inhibitor_surface.as_ref() != focused {
            self.suppressed_inhibitor_surface = None;
        }
        if !self.is_session_locked()
            && !self.has_blocking_shell_overlay()
            && self.suppressed_inhibitor_surface.as_ref() != focused
            && let Some(surf) = focused
            && let Some(inhibitor) = seat.keyboard_shortcuts_inhibitor_for_surface(surf)
        {
            inhibitor.activate();
            self.active_shortcut_inhibitor = Some(inhibitor);
        }
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

impl DmabufHandler for RustyCompositorState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        if dmabuf.num_planes() > 0 && dmabuf.width() > 0 && dmabuf.height() > 0 {
            let _ = notifier.successful::<Self>();
        } else {
            notifier.failed();
        }
    }
}

impl FractionalScaleHandler for RustyCompositorState {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self.preferred_scale_for_surface(&surface);
        Self::apply_surface_scale(&surface, scale);
    }
}

impl PointerConstraintsHandler for RustyCompositorState {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        if !self.has_blocking_shell_overlay() && pointer.current_focus().as_ref() == Some(surface) {
            with_pointer_constraint(surface, pointer, |constraint| {
                if let Some(c) = constraint
                    && !c.is_active()
                {
                    c.activate();
                }
            });
        }
    }

    fn cursor_position_hint(
        &mut self,
        surface: &WlSurface,
        pointer: &PointerHandle<Self>,
        location: Point<f64, Logical>,
    ) {
        let mut active_locked = false;
        with_pointer_constraint(surface, pointer, |constraint| {
            if let Some(c) = constraint
                && c.is_active()
                && matches!(&*c, PointerConstraint::Locked(_))
            {
                active_locked = true;
            }
        });
        if active_locked && let Some(bounds) = self.surface_logical_bounds(surface) {
            let total = self.total_output_bounds();
            let warped = Point::from((
                (bounds.loc.x + location.x).clamp(
                    f64::from(total.loc.x),
                    f64::from(total.loc.x + total.size.w - 1).max(f64::from(total.loc.x)),
                ),
                (bounds.loc.y + location.y).clamp(
                    f64::from(total.loc.y),
                    f64::from(total.loc.y + total.size.h - 1).max(f64::from(total.loc.y)),
                ),
            ));
            self.pointer_location = warped;
        }
    }
}

impl TabletSeatHandler for RustyCompositorState {}

impl XdgActivationHandler for RustyCompositorState {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let within_ttl = token_data.timestamp.elapsed() <= Duration::from_secs(10);
        self.xdg_activation_state.remove_token(&token);
        if !within_ttl || self.is_session_locked() {
            return;
        }
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let _ = self.activate_window_surface(&surface, serial);
    }
}

impl IdleNotifierHandler for RustyCompositorState {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.idle_notifier_state
    }
}

impl IdleInhibitHandler for RustyCompositorState {
    fn inhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitor_surfaces.retain(IsAlive::alive);
        if !self.idle_inhibitor_surfaces.iter().any(|s| s == &surface) {
            self.idle_inhibitor_surfaces.push(surface);
        }
        self.idle_notifier_state
            .set_is_inhibited(!self.idle_inhibitor_surfaces.is_empty());
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitor_surfaces
            .retain(|s| s.alive() && s != &surface);
        self.idle_notifier_state
            .set_is_inhibited(!self.idle_inhibitor_surfaces.is_empty());
    }
}

impl SessionLockHandler for RustyCompositorState {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        if self.ext_session_locked {
            drop(confirmation);
            return;
        }
        self.ext_session_locked = true;
        self.window_switcher = None;
        self.dnd_icon = None;
        self.close_launcher();
        self.close_quick_settings();
        self.close_overview();
        self.active_window_drag = None;
        self.active_window_resize = None;
        self.dock_hover_slot = None;
        self.touchpad_swipe = TouchpadSwipeState::default();
        self.deactivate_focused_shortcut_inhibitor();
        self.deactivate_active_pointer_constraint();

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, None, serial);
        let pointer = self.pointer.clone();
        pointer.motion(
            self,
            None,
            &smithay::input::pointer::MotionEvent {
                location: self.pointer_location,
                serial,
                time: u32::try_from(self.elapsed().as_millis()).unwrap_or(0),
            },
        );
        pointer.frame(self);
        self.pending_session_locker = Some(confirmation);
    }

    fn unlock(&mut self) {
        self.ext_session_locked = false;
        self.pending_session_locker = None;
        self.suppressed_inhibitor_surface = None;
        self.ext_lock_surfaces.clear();
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
    }

    fn new_surface(&mut self, surface: LockSurface, wl_output: WlOutput) {
        self.ext_lock_surfaces.retain(|(_, s)| s.alive());
        let resolved_output =
            Output::from_resource(&wl_output).unwrap_or_else(|| self.output.clone());
        self.ext_lock_surfaces
            .retain(|(out, _)| out != &resolved_output);

        let logical_size = self.logical_u32_size_for_output(&resolved_output);
        let output_scale = self
            .output_map
            .entries()
            .iter()
            .find(|e| e.output == resolved_output)
            .map_or(1.0, |e| e.scale);
        Self::apply_surface_scale(surface.wl_surface(), output_scale);
        surface.with_pending_state(|state| {
            state.size = Some(logical_size);
        });
        let lock_wl = surface.wl_surface().clone();
        self.ext_lock_surfaces.push((resolved_output, surface));

        if self.ext_session_locked {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let keyboard = self.keyboard.clone();
            keyboard.set_focus(self, Some(lock_wl), serial);
        }
    }
}

impl DrmSyncobjHandler for RustyCompositorState {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.syncobj_state.as_mut()
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

impl ForeignToplevelListHandler for RustyCompositorState {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.foreign_toplevel_list_state
    }
}

impl InputMethodHandler for RustyCompositorState {
    fn new_popup(&mut self, surface: ImePopupSurface) {
        self.ime_popups.retain(ImePopupSurface::alive);
        if !self
            .ime_popups
            .iter()
            .any(|existing| existing.wl_surface() == surface.wl_surface())
        {
            self.ime_popups.push(surface);
        }
    }

    fn dismiss_popup(&mut self, surface: ImePopupSurface) {
        self.ime_popups
            .retain(|existing| existing.wl_surface() != surface.wl_surface() && existing.alive());
    }

    fn popup_repositioned(&mut self, surface: ImePopupSurface) {
        self.ime_popups.retain(ImePopupSurface::alive);
        if !self
            .ime_popups
            .iter()
            .any(|existing| existing.wl_surface() == surface.wl_surface())
            && surface.alive()
        {
            self.ime_popups.push(surface);
        }
    }

    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        for (_, win) in &self.window_bindings {
            if win
                .wl_surface()
                .is_some_and(|root| Self::surface_in_tree(&root, parent))
                && let Some(loc) = self.space.element_location(win)
            {
                return Rectangle::new(loc + win.geometry().loc, win.geometry().size);
            }
        }
        for entry in self.output_map.entries() {
            let map = layer_map_for_output(&entry.output);
            for layer in map.layers() {
                if Self::surface_in_tree(layer.wl_surface(), parent)
                    && let Some(geo) = map.layer_geometry(layer)
                {
                    return Rectangle::new(entry.logical_geometry().loc + geo.loc, geo.size);
                }
            }
        }
        self.output_map.primary().logical_geometry()
    }
}

impl KeyboardShortcutsInhibitHandler for RustyCompositorState {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.keyboard_shortcuts_inhibit_state
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        if self
            .suppressed_inhibitor_surface
            .as_ref()
            .is_some_and(|surf| surf == inhibitor.wl_surface())
        {
            self.suppressed_inhibitor_surface = None;
        }
        if !self.is_session_locked()
            && !self.has_blocking_shell_overlay()
            && self
                .keyboard
                .current_focus()
                .as_ref()
                .is_some_and(|focused| focused == inhibitor.wl_surface())
        {
            inhibitor.activate();
            self.active_shortcut_inhibitor = Some(inhibitor);
        }
    }

    fn inhibitor_destroyed(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        if self
            .suppressed_inhibitor_surface
            .as_ref()
            .is_some_and(|surf| surf == inhibitor.wl_surface())
        {
            self.suppressed_inhibitor_surface = None;
        }
        if self
            .active_shortcut_inhibitor
            .as_ref()
            .is_some_and(|cur| cur.wl_surface() == inhibitor.wl_surface())
        {
            self.active_shortcut_inhibitor = None;
        }
    }
}

impl XdgToplevelIconHandler for RustyCompositorState {
    fn set_icon(&mut self, toplevel: xdg_toplevel::XdgToplevel, wl_surface: WlSurface) {
        if let Some(win_id) = self.window_id_for_xdg_toplevel_proto(&toplevel) {
            self.sync_toplevel_icon_from_surface(win_id, &wl_surface);
        }
    }
}

delegate_global_dispatch!(
    RustyCompositorState: [
        XdgToplevelIconManagerV1: XdgToplevelIconManagerUserData
    ] => XdgToplevelIconManager
);

impl Dispatch<XdgToplevelIconManagerV1, ()> for RustyCompositorState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &XdgToplevelIconManagerV1,
        request: xdg_toplevel_icon_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use xdg_toplevel_icon_manager_v1::Request;

        match request {
            Request::CreateIcon { id } => {
                let resource = data_init.init(id, XdgToplevelIconUserData::default());
                state.toplevel_icons.insert(
                    resource.id(),
                    ToplevelIconEntry {
                        resource,
                        immutable: false,
                        icon_name: None,
                        buffers: Vec::new(),
                    },
                );
            }
            Request::SetIcon { toplevel, icon } => {
                let Some(wl_surface) = state
                    .xdg_shell_state
                    .toplevel_surfaces()
                    .iter()
                    .find(|tl| tl.xdg_toplevel() == &toplevel)
                    .map(|tl| tl.wl_surface().clone())
                else {
                    return;
                };
                let snapshot = icon
                    .and_then(|icon_res| {
                        let entry = state.toplevel_icons.get_mut(&icon_res.id())?;
                        entry.immutable = true;
                        Some(entry.snapshot_surface_state())
                    })
                    .unwrap_or_default();
                compositor::with_states(&wl_surface, |states| {
                    let mut cached = states.cached_state.get::<ToplevelIconSurfaceState>();
                    *cached.pending() = snapshot;
                });
                state.set_icon(toplevel, wl_surface);
            }
            Request::Destroy | _ => {}
        }
    }
}

impl Dispatch<XdgToplevelIconV1, XdgToplevelIconUserData> for RustyCompositorState {
    fn request(
        state: &mut Self,
        _client: &Client,
        icon: &XdgToplevelIconV1,
        request: xdg_toplevel_icon_v1::Request,
        _data: &XdgToplevelIconUserData,
        dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use xdg_toplevel_icon_v1::Request;

        match request {
            Request::SetName { icon_name } => {
                let Some(entry) = state.toplevel_icons.get_mut(&icon.id()) else {
                    return;
                };
                if entry.immutable {
                    dh.post_error(
                        icon,
                        xdg_toplevel_icon_v1::Error::Immutable as u32,
                        "Request made after the icon has been assigned to a toplevel via 'set_icon'"
                            .to_owned(),
                    );
                    return;
                }
                entry.icon_name = Some(icon_name);
            }
            Request::AddBuffer { buffer, scale } => {
                let is_immutable = state
                    .toplevel_icons
                    .get(&icon.id())
                    .is_some_and(|entry| entry.immutable);
                if is_immutable {
                    dh.post_error(
                        icon,
                        xdg_toplevel_icon_v1::Error::Immutable as u32,
                        "Request made after the icon has been assigned to a toplevel via 'set_icon'"
                            .to_owned(),
                    );
                    return;
                }
                let (width, height) =
                    match with_buffer_contents(&buffer, |_, _, buf| (buf.width, buf.height)) {
                        Ok(dims) => dims,
                        Err(BufferAccessError::NotManaged) => {
                            dh.post_error(
                                icon,
                                xdg_toplevel_icon_v1::Error::InvalidBuffer as u32,
                                "The wl_buffer must be backed by wl_shm".to_owned(),
                            );
                            return;
                        }
                        Err(_) => return,
                    };
                if width <= 0 || width != height {
                    dh.post_error(
                        icon,
                        xdg_toplevel_icon_v1::Error::InvalidBuffer as u32,
                        "The wl_buffer must be a square".to_owned(),
                    );
                    return;
                }
                if let Some(entry) = state.toplevel_icons.get_mut(&icon.id()) {
                    entry.add_buffer_replacing_slot(buffer, width, height, scale);
                }
            }
            Request::Destroy => {
                state.toplevel_icons.remove(&icon.id());
            }
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        resource: &XdgToplevelIconV1,
        _data: &XdgToplevelIconUserData,
    ) {
        state.toplevel_icons.remove(&resource.id());
    }
}

delegate_compositor!(RustyCompositorState);
delegate_shm!(RustyCompositorState);
delegate_viewporter!(RustyCompositorState);
delegate_fractional_scale!(RustyCompositorState);
delegate_relative_pointer!(RustyCompositorState);
delegate_pointer_constraints!(RustyCompositorState);
delegate_pointer_gestures!(RustyCompositorState);
delegate_presentation!(RustyCompositorState);
delegate_cursor_shape!(RustyCompositorState);
delegate_xdg_activation!(RustyCompositorState);
delegate_idle_notify!(RustyCompositorState);
delegate_idle_inhibit!(RustyCompositorState);
delegate_session_lock!(RustyCompositorState);
delegate_dmabuf!(RustyCompositorState);
delegate_drm_syncobj!(RustyCompositorState);
delegate_output!(RustyCompositorState);
delegate_seat!(RustyCompositorState);
delegate_data_device!(RustyCompositorState);
delegate_primary_selection!(RustyCompositorState);
delegate_data_control!(RustyCompositorState);
delegate_xdg_shell!(RustyCompositorState);
delegate_xdg_decoration!(RustyCompositorState);
delegate_layer_shell!(RustyCompositorState);
delegate_text_input_manager!(RustyCompositorState);
delegate_input_method_manager!(RustyCompositorState);
delegate_keyboard_shortcuts_inhibit!(RustyCompositorState);
delegate_foreign_toplevel_list!(RustyCompositorState);

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::reexports::{
        calloop::EventLoop,
        wayland_server::backend::{Handle, ObjectData, protocol::Message},
    };
    use smithay::wayland::compositor::Cacheable;
    use std::error::Error;
    use std::io::Read;
    use std::os::unix::net::UnixStream;

    #[derive(Debug)]
    struct TestDummyObjectData;

    impl ObjectData<RustyCompositorState> for TestDummyObjectData {
        fn request(
            self: Arc<Self>,
            _handle: &Handle,
            _data: &mut RustyCompositorState,
            _client_id: ClientId,
            _msg: Message<ObjectId, OwnedFd>,
        ) -> Option<Arc<dyn ObjectData<RustyCompositorState>>> {
            None
        }

        fn destroyed(
            self: Arc<Self>,
            _handle: &Handle,
            _data: &mut RustyCompositorState,
            _client_id: ClientId,
            _object_id: ObjectId,
        ) {
        }
    }

    #[test]
    fn xdg_toplevel_icon_destroy_before_buffer_destroy_and_slot_override_avoid_false_no_buffer()
    -> Result<(), Box<dyn Error>> {
        let mut display: Display<RustyCompositorState> = Display::new()?;
        let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
        let mut dh = display.handle();
        let mut compositor = RustyCompositorState::new(
            &display,
            "rusty-test-toplevel-icon-cleanup".to_owned(),
            event_loop.get_signal(),
            event_loop.handle(),
            (1280, 800).into(),
        )?;

        let (server_stream, mut client_stream) = UnixStream::pair()?;
        client_stream.set_nonblocking(true)?;
        let client = dh.insert_client(server_stream, Arc::new(ClientState::default()))?;

        let buf_old = client
            .create_resource_from_objdata::<wl_buffer::WlBuffer, RustyCompositorState>(
                &dh,
                1,
                Arc::new(TestDummyObjectData),
            )?;
        let buf_new = client
            .create_resource_from_objdata::<wl_buffer::WlBuffer, RustyCompositorState>(
                &dh,
                1,
                Arc::new(TestDummyObjectData),
            )?;
        let icon = client
            .create_resource::<XdgToplevelIconV1, XdgToplevelIconUserData, RustyCompositorState>(
                &dh,
                1,
                XdgToplevelIconUserData::default(),
            )?;

        let mut entry = ToplevelIconEntry {
            resource: icon.clone(),
            immutable: true,
            icon_name: Some("utilities-terminal".to_owned()),
            buffers: Vec::new(),
        };
        entry.add_buffer_replacing_slot(buf_old.clone(), 16, 16, 1);
        entry.add_buffer_replacing_slot(buf_new.clone(), 16, 16, 1);
        let snapshot = entry.snapshot_surface_state();
        compositor.toplevel_icons.insert(icon.id(), entry);

        // Destroying superseded slot buffer while icon is alive must not raise no_buffer.
        compositor.buffer_destroyed(&buf_old);
        display.flush_clients()?;
        let mut wire_buf = [0_u8; 256];
        assert!(matches!(
            client_stream.read(&mut wire_buf),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
        ));

        // Simulate xdg_toplevel_icon_v1.destroy() prior to wl_buffer.destroy() (Qt6 sequence).
        compositor.toplevel_icons.remove(&icon.id());
        compositor.buffer_destroyed(&buf_new);
        display.flush_clients()?;
        assert!(matches!(
            client_stream.read(&mut wire_buf),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
        ));

        // Snapshot committed to double-buffered ToplevelIconSurfaceState preserves icon_name.
        let mut committed = ToplevelIconSurfaceState::default();
        let mut pending = snapshot;
        let next = pending.commit(&dh);
        next.merge_into(&mut committed, &dh);
        assert_eq!(committed.icon_name.as_deref(), Some("utilities-terminal"));
        assert_eq!(committed.buffers.len(), 1);

        Ok(())
    }

    #[test]
    fn xdg_toplevel_icon_buffer_destroyed_while_icon_alive_raises_no_buffer()
    -> Result<(), Box<dyn Error>> {
        let mut display: Display<RustyCompositorState> = Display::new()?;
        let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
        let mut dh = display.handle();
        let mut compositor = RustyCompositorState::new(
            &display,
            "rusty-test-toplevel-icon-error".to_owned(),
            event_loop.get_signal(),
            event_loop.handle(),
            (1280, 800).into(),
        )?;

        let (server_stream, mut client_stream) = UnixStream::pair()?;
        client_stream.set_nonblocking(true)?;
        let client = dh.insert_client(server_stream, Arc::new(ClientState::default()))?;

        let buf = client
            .create_resource_from_objdata::<wl_buffer::WlBuffer, RustyCompositorState>(
                &dh,
                1,
                Arc::new(TestDummyObjectData),
            )?;
        let icon = client
            .create_resource::<XdgToplevelIconV1, XdgToplevelIconUserData, RustyCompositorState>(
                &dh,
                1,
                XdgToplevelIconUserData::default(),
            )?;

        let mut entry = ToplevelIconEntry {
            resource: icon.clone(),
            immutable: false,
            icon_name: None,
            buffers: Vec::new(),
        };
        entry.add_buffer_replacing_slot(buf.clone(), 32, 32, 1);
        compositor.toplevel_icons.insert(icon.id(), entry);

        compositor.buffer_destroyed(&buf);
        display.flush_clients()?;

        let mut wire_buf = [0_u8; 512];
        let bytes = client_stream.read(&mut wire_buf)?;
        assert!(bytes > 0);
        let text = String::from_utf8_lossy(&wire_buf[..bytes]);
        assert!(text.contains("destroyed before the toplevel icon"));

        Ok(())
    }
}
