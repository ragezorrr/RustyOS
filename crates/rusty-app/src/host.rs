//! Shared Wayland/SCTK + `calloop` application host with GPU GLES2 and CPU `wl_shm` fallback.

use std::fmt;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::mpsc;
use std::time::Duration;

use rusty_ui::{
    GlesSurfaceRenderer, PixelFormat, UiCanvas, UiDrawList, UiFontEngine, UiPainter,
    WaylandClipboardBridge, render_draw_list_to_canvas,
};
pub use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape as CursorShape;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;
pub use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region, Surface},
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::DataSourceHandler,
    },
    delegate_compositor, delegate_data_device, delegate_keyboard, delegate_output,
    delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_xdg_popup,
    delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, ping},
        calloop_wayland_source::WaylandSource,
        protocols::xdg::shell::client::xdg_positioner::{Anchor, ConstraintAdjustment, Gravity},
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgPositioner, XdgShell, XdgSurface,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::registry_queue_init,
    protocol::{
        wl_data_device, wl_data_device_manager::DndAction, wl_data_source, wl_keyboard, wl_output,
        wl_pointer, wl_seat, wl_shm, wl_surface,
    },
};

use crate::app_menu::{
    AppMenuSectionSpec, RustyosAppMenuManagerV1, RustyosSurfaceAppMenuV1, SurfaceAppMenuEvent,
};
use crate::liquid_glass::{
    GlassRegion, LiquidGlassConfig, RustyosLiquidGlassManagerV1, RustyosSurfaceGlassV1,
};

/// Window decoration preference passed to `xdg-decoration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowDecorationMode {
    /// Request client-side decorations (custom RustyOS header / traffic lights).
    #[default]
    Client,
    /// Request server-side compositor decorations.
    Server,
}

impl WindowDecorationMode {
    const fn to_sctk(self) -> WindowDecorations {
        match self {
            Self::Client => WindowDecorations::RequestClient,
            Self::Server => WindowDecorations::RequestServer,
        }
    }
}

/// Startup configuration for a RustyOS native Wayland application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    /// Initial window title.
    pub title: String,
    /// Reverse-DNS `app_id` (e.g. `org.rustyos.view`).
    pub app_id: String,
    /// Initial logical window size `(width, height)`.
    pub default_size: (u32, u32),
    /// Optional minimum logical window size `(width, height)`.
    pub min_size: Option<(u32, u32)>,
    /// Client-side vs server-side window decorations.
    pub decorations: WindowDecorationMode,
    /// Whether to start in fullscreen mode.
    pub fullscreen: bool,
    /// Force CPU `wl_shm` rendering even when GLES2 is available.
    pub no_gpu: bool,
    /// Optional initial Liquid Glass backdrop configuration.
    pub glass: Option<LiquidGlassConfig>,
}

impl AppConfig {
    /// Creates a new application configuration with sensible RustyOS defaults.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        app_id: impl Into<String>,
        default_size: (u32, u32),
    ) -> Self {
        Self {
            title: title.into(),
            app_id: app_id.into(),
            default_size,
            min_size: None,
            decorations: WindowDecorationMode::Client,
            fullscreen: false,
            no_gpu: false,
            glass: None,
        }
    }

    /// Sets the minimum logical window size.
    #[must_use]
    pub const fn with_min_size(mut self, width: u32, height: u32) -> Self {
        self.min_size = Some((width, height));
        self
    }

    /// Sets the window decoration mode.
    #[must_use]
    pub const fn with_decorations(mut self, decorations: WindowDecorationMode) -> Self {
        self.decorations = decorations;
        self
    }

    /// Sets whether the window launches fullscreen.
    #[must_use]
    pub const fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Forces CPU `wl_shm` rendering instead of GLES2 when `no_gpu` is `true`.
    #[must_use]
    pub const fn with_no_gpu(mut self, no_gpu: bool) -> Self {
        self.no_gpu = no_gpu;
        self
    }

    /// Sets the initial Liquid Glass backdrop configuration.
    #[must_use]
    pub const fn with_liquid_glass(mut self, glass: LiquidGlassConfig) -> Self {
        self.glass = Some(glass);
        self
    }
}

/// Declarative specification for an `xdg_popup` surface anchored to the main window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupConfig {
    /// Popup surface logical width.
    pub width: u32,
    /// Popup surface logical height.
    pub height: u32,
    /// Anchor X coordinate relative to the parent window.
    pub anchor_x: i32,
    /// Anchor Y coordinate relative to the parent window.
    pub anchor_y: i32,
    /// Positioner X offset.
    pub offset_x: i32,
    /// Positioner Y offset.
    pub offset_y: i32,
    /// Optional seat input serial for `xdg_popup.grab`.
    pub grab_serial: Option<u32>,
    /// Optional Liquid Glass backdrop configuration for the popup surface.
    pub glass: Option<LiquidGlassConfig>,
}

/// Snapshot of a Wayland `xdg_toplevel` configure event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigureInfo {
    /// Resolved logical width (clamped to `min_size` if configured).
    pub width: u32,
    /// Resolved logical height (clamped to `min_size` if configured).
    pub height: u32,
    /// Whether the window is maximized.
    pub is_maximized: bool,
    /// Whether the window is fullscreen.
    pub is_fullscreen: bool,
    /// Whether the window currently holds active focus.
    pub is_activated: bool,
}

/// Errors returned when initializing or running a [`RustyApp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// Could not connect to the Wayland display (`WAYLAND_DISPLAY` missing/unavailable).
    WaylandConnect(String),
    /// Could not initialize the Wayland global registry.
    RegistryInit(String),
    /// Required Wayland global interface was not advertised by the compositor.
    MissingGlobal(&'static str),
    /// Failed to allocate the fallback `wl_shm` slot pool.
    ShmPool(String),
    /// Failed to create or run the `calloop` event loop.
    EventLoop(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WaylandConnect(msg) => write!(f, "failed to connect to Wayland display: {msg}"),
            Self::RegistryInit(msg) => write!(f, "failed to initialize Wayland registry: {msg}"),
            Self::MissingGlobal(name) => write!(f, "required Wayland global unavailable: {name}"),
            Self::ShmPool(msg) => write!(f, "failed to allocate wl_shm pool: {msg}"),
            Self::EventLoop(msg) => write!(f, "event loop error: {msg}"),
        }
    }
}

impl std::error::Error for AppError {}

struct MimeDataSource {
    source: wl_data_source::WlDataSource,
    entries: Vec<(String, Vec<u8>)>,
    selected_action: DndAction,
}

enum HostAsyncData {
    ClipboardData {
        mime: String,
        bytes: Vec<u8>,
    },
    DndDrop {
        x: f64,
        y: f64,
        is_move: bool,
        mime: String,
        bytes: Vec<u8>,
    },
}

/// Mutable window & runtime context passed to [`RustyApp`] event callbacks.
pub struct AppContext<'a> {
    conn: &'a Connection,
    qh: &'a QueueHandle<HostState>,
    window: &'a Window,
    seat: Option<&'a wl_seat::WlSeat>,
    data_device_manager: Option<&'a DataDeviceManagerState>,
    data_device: Option<&'a DataDevice>,
    clipboard: &'a mut WaylandClipboardBridge,
    active_mime_clipboard: &'a mut Option<MimeDataSource>,
    active_drag_source: &'a mut Option<MimeDataSource>,
    data_tx: &'a mpsc::Sender<HostAsyncData>,
    data_ping: &'a ping::Ping,
    fonts: &'a mut UiFontEngine,
    width: u32,
    height: u32,
    modifiers: Modifiers,
    pointer_pos: (f64, f64),
    dirty: &'a mut bool,
    exit: &'a mut bool,
    last_title: &'a mut String,
    desired_glass: &'a mut Option<LiquidGlassConfig>,
    glass_dirty: &'a mut bool,
    desired_menu: &'a mut Option<Vec<AppMenuSectionSpec>>,
    menu_dirty: &'a mut bool,
    desired_popup: &'a mut Option<PopupConfig>,
    popup_dirty: &'a mut bool,
    popup_redraw: &'a mut bool,
    desired_cursor: &'a mut Option<CursorShape>,
    gpu_active: bool,
    has_glass: bool,
}

impl fmt::Debug for AppContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppContext")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pointer_pos", &self.pointer_pos)
            .field("gpu_active", &self.gpu_active)
            .finish_non_exhaustive()
    }
}

impl AppContext<'_> {
    /// Current logical window width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Current logical window height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Current keyboard modifier state.
    #[must_use]
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Last known pointer position in surface-local logical coordinates.
    #[must_use]
    pub const fn pointer_pos(&self) -> (f64, f64) {
        self.pointer_pos
    }

    /// Returns `true` when frames are presented through hardware-accelerated GLES2.
    #[must_use]
    pub const fn is_gpu_accelerated(&self) -> bool {
        self.gpu_active
    }

    /// Returns `true` when the compositor advertised `rustyos_liquid_glass_v1` for this window.
    #[must_use]
    pub const fn has_liquid_glass(&self) -> bool {
        self.has_glass
    }

    /// Mutable access to the shared [`UiFontEngine`] for text measurement/editing.
    pub fn fonts_mut(&mut self) -> &mut UiFontEngine {
        self.fonts
    }

    /// Updates the window title if it changed.
    pub fn set_title(&mut self, title: &str) {
        if self.last_title != title {
            self.last_title.clear();
            self.last_title.push_str(title);
            self.window.set_title(title);
        }
    }

    /// Minimizes the window.
    pub fn set_minimized(&mut self) {
        self.window.set_minimized();
    }

    /// Maximizes or restores the window.
    pub fn set_maximized(&mut self, maximized: bool) {
        if maximized {
            self.window.set_maximized();
        } else {
            self.window.unset_maximized();
        }
        *self.dirty = true;
    }

    /// Enters or leaves fullscreen mode.
    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        if fullscreen {
            self.window.set_fullscreen(None);
        } else {
            self.window.unset_fullscreen();
        }
        *self.dirty = true;
    }

    /// Initiates an interactive compositor window move using `serial`.
    pub fn start_interactive_move(&mut self, serial: u32) {
        if let Some(seat) = self.seat {
            self.window.move_(seat, serial);
        }
    }

    /// Initiates an interactive compositor window resize along `edge` using `serial`.
    pub fn start_interactive_resize(&mut self, edge: ResizeEdge, serial: u32) {
        if let Some(seat) = self.seat {
            self.window.resize(seat, serial, edge);
        }
    }

    /// Updates the declarative Liquid Glass backdrop configuration for this window.
    pub fn set_liquid_glass(&mut self, config: Option<LiquidGlassConfig>) {
        if *self.desired_glass != config {
            *self.desired_glass = config;
            *self.glass_dirty = true;
            *self.dirty = true;
        }
    }

    /// Exports or updates this window's sections in the compositor's Global Menu Bar (`rustyos_app_menu_v1`).
    pub fn set_app_menu(&mut self, sections: Vec<AppMenuSectionSpec>) {
        let next = (!sections.is_empty()).then_some(sections);
        if *self.desired_menu != next {
            *self.desired_menu = next;
            *self.menu_dirty = true;
        }
    }

    /// Marks the window dirty so it will repaint on the next frame.
    pub fn request_redraw(&mut self) {
        *self.dirty = true;
    }

    /// Requests graceful shutdown of the application event loop.
    pub fn request_exit(&mut self) {
        *self.exit = true;
    }

    /// Sets the desired pointer cursor shape (`wp_cursor_shape_device_v1`).
    pub fn set_cursor_shape(&mut self, shape: CursorShape) {
        *self.desired_cursor = Some(shape);
    }

    /// Opens or replaces the active `xdg_popup` anchored to the main window.
    pub fn open_popup(&mut self, config: PopupConfig) {
        if *self.desired_popup != Some(config.clone()) {
            *self.desired_popup = Some(config);
            *self.popup_dirty = true;
        }
        *self.popup_redraw = true;
    }

    /// Closes the active `xdg_popup`, if any.
    pub fn close_popup(&mut self) {
        if self.desired_popup.take().is_some() {
            *self.popup_dirty = true;
        }
        *self.popup_redraw = false;
    }

    /// Requests a repaint of the active `xdg_popup`.
    pub fn request_popup_redraw(&mut self) {
        *self.popup_redraw = true;
    }

    /// Shows the compositor's native window menu at `(x, y)` using `serial`.
    pub fn show_window_menu(&mut self, serial: u32, x: i32, y: i32) {
        if let Some(seat) = self.seat {
            self.window.show_window_menu(seat, serial, (x, y));
        }
    }

    /// Mutable access to the shared Wayland UTF-8 text clipboard bridge.
    pub fn clipboard_mut(&mut self) -> &mut WaylandClipboardBridge {
        self.clipboard
    }

    /// Advertises arbitrary MIME entries on the Wayland clipboard using `serial`.
    pub fn set_clipboard_mimes(&mut self, entries: Vec<(String, Vec<u8>)>, serial: u32) -> bool {
        let (Some(ddm), Some(dd)) = (self.data_device_manager, self.data_device) else {
            return false;
        };
        let mimes: Vec<&str> = entries.iter().map(|(m, _)| m.as_str()).collect();
        let source = ddm.create_copy_paste_source(self.qh, mimes);
        source.set_selection(dd, serial);
        *self.active_mime_clipboard = Some(MimeDataSource {
            source: source.inner().clone(),
            entries,
            selected_action: DndAction::None,
        });
        let _ = self.conn.flush();
        true
    }

    /// Returns `true` if the current Wayland clipboard selection advertises `mime`.
    #[must_use]
    pub fn has_clipboard_mime(&self, mime: &str) -> bool {
        self.data_device
            .and_then(|dd| dd.data().selection_offer())
            .is_some_and(|offer| offer.with_mime_types(|mimes| mimes.iter().any(|m| m == mime)))
    }

    /// Requests the first matching MIME type from the current clipboard selection.
    ///
    /// When received, [`RustyApp::on_clipboard_data`] is invoked on the event loop.
    pub fn request_clipboard_mime(&mut self, candidates: &[&str]) -> bool {
        let Some(offer) = self.data_device.and_then(|dd| dd.data().selection_offer()) else {
            return false;
        };
        let mime = offer.with_mime_types(|mimes| {
            candidates
                .iter()
                .find(|cand| mimes.iter().any(|m| m == *cand))
                .map(|s| (*s).to_owned())
        });
        let Some(mime) = mime else {
            return false;
        };
        let Ok(mut pipe) = offer.receive(mime.clone()) else {
            return false;
        };
        let _ = self.conn.flush();
        let tx = self.data_tx.clone();
        let ping = self.data_ping.clone();
        let _ = std::thread::Builder::new()
            .name(String::from("rusty-app-clip-recv"))
            .spawn(move || {
                let mut bytes = Vec::new();
                let _ = pipe.by_ref().take(4 * 1024 * 1024).read_to_end(&mut bytes);
                if tx
                    .send(HostAsyncData::ClipboardData { mime, bytes })
                    .is_ok()
                {
                    ping.ping();
                }
            });
        true
    }

    /// Starts an outgoing Wayland drag-and-drop session with `entries` and `serial`.
    pub fn start_drag(&mut self, entries: Vec<(String, Vec<u8>)>, serial: u32) -> bool {
        if entries.is_empty() {
            return false;
        }
        let (Some(ddm), Some(dd)) = (self.data_device_manager, self.data_device) else {
            return false;
        };
        let mimes: Vec<&str> = entries.iter().map(|(m, _)| m.as_str()).collect();
        let source =
            ddm.create_drag_and_drop_source(self.qh, mimes, DndAction::Copy | DndAction::Move);
        source.start_drag(dd, self.window.wl_surface(), None, serial);
        let _ = self.conn.flush();
        *self.active_drag_source = Some(MimeDataSource {
            source: source.inner().clone(),
            entries,
            selected_action: DndAction::None,
        });
        true
    }
}

/// Application trait implemented by RustyOS native Wayland apps hosted by [`run_app`].
pub trait RustyApp {
    /// Called whenever the compositor configures window geometry or state.
    fn on_configure(&mut self, _ctx: &mut AppContext<'_>, _info: ConfigureInfo) {}

    /// Called on keyboard key press.
    fn on_key(&mut self, _ctx: &mut AppContext<'_>, _event: KeyEvent) {}

    /// Called for each batch of Wayland pointer events.
    fn on_pointer(&mut self, _ctx: &mut AppContext<'_>, _events: &[PointerEvent]) {}

    /// Called when the user activates an exported Menu Bar item (`rustyos_app_menu_v1`).
    fn on_menu_item(&mut self, _ctx: &mut AppContext<'_>, _item_id: i32) {}

    /// Called once per event-loop iteration before drawing (for background polling or animations).
    fn on_tick(&mut self, _ctx: &mut AppContext<'_>) {}

    /// Maximum wait duration before waking up for [`Self::on_tick`].
    fn poll_interval(&self) -> Duration {
        Duration::from_millis(250)
    }

    /// Background clear color `[r, g, b, a]` applied before painting `UiDrawList`.
    fn clear_color(&self) -> [u8; 4] {
        [0, 0, 0, 0]
    }

    /// Called when the compositor requests window close. Return `true` to exit the loop.
    fn on_close_requested(&mut self, _ctx: &mut AppContext<'_>) -> bool {
        true
    }

    /// Paints the current frame into `painter` using logical dimensions `width x height`.
    fn paint(
        &mut self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        width: u32,
        height: u32,
    );

    /// Optional direct premultiplied `BGRA8888` pixel-buffer paint callback.
    ///
    /// Return `true` when `canvas_bgra` (`width * height * 4` bytes) was populated
    /// and should be presented directly instead of calling [`Self::paint`].
    fn paint_bgra(
        &mut self,
        _canvas_bgra: &mut [u8],
        _width: u32,
        _height: u32,
        _has_glass: bool,
    ) -> bool {
        false
    }

    /// Called for pointer events targeting the active `xdg_popup` surface.
    fn on_popup_pointer(&mut self, _ctx: &mut AppContext<'_>, _events: &[PointerEvent]) {}

    /// Called when the compositor dismisses the active `xdg_popup` (`xdg_popup.popup_done`).
    fn on_popup_closed(&mut self, _ctx: &mut AppContext<'_>) {}

    /// Optional `BGRA8888` pixel-buffer paint callback for the active `xdg_popup`.
    fn paint_popup_bgra(
        &mut self,
        _canvas_bgra: &mut [u8],
        _width: u32,
        _height: u32,
        _has_glass: bool,
    ) -> bool {
        false
    }

    /// Called when asynchronous clipboard data requested via [`AppContext::request_clipboard_mime`] arrives.
    fn on_clipboard_data(&mut self, _ctx: &mut AppContext<'_>, _mime: &str, _data: &[u8]) {}

    /// Called when the compositor advertises a new clipboard selection offer.
    fn on_clipboard_offer_changed(&mut self, _ctx: &mut AppContext<'_>) {}

    /// Called when an external drag enters or moves over the window.
    ///
    /// Return `true` to accept `"text/uri-list"` copy/move drops.
    fn on_dnd_motion(
        &mut self,
        _ctx: &mut AppContext<'_>,
        _x: f64,
        _y: f64,
        _mimes: &[String],
    ) -> bool {
        false
    }

    /// Called when an external drag leaves the window surface.
    fn on_dnd_leave(&mut self, _ctx: &mut AppContext<'_>) {}

    /// Called when an accepted external drag drops onto the window.
    fn on_dnd_drop(
        &mut self,
        _ctx: &mut AppContext<'_>,
        _x: f64,
        _y: f64,
        _is_move: bool,
        _mime: &str,
        _data: &[u8],
    ) {
    }

    /// Called when an outgoing drag started with [`AppContext::start_drag`] finishes.
    fn on_drag_finished(&mut self, _ctx: &mut AppContext<'_>, _moved: bool) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderBackend {
    Gpu,
    Cpu,
}

struct HostPopup {
    popup: Popup,
    glass: Option<RustyosSurfaceGlassV1>,
    width: u32,
    height: u32,
    configured: bool,
    backbuf: Vec<u8>,
}

#[allow(clippy::struct_excessive_bools)]
struct HostState {
    conn: Connection,
    qh: QueueHandle<Self>,
    registry_state: RegistryState,
    compositor_state: CompositorState,
    xdg_shell: XdgShell,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    data_device_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    clipboard: WaylandClipboardBridge,
    active_mime_clipboard: Option<MimeDataSource>,
    active_drag_source: Option<MimeDataSource>,
    incoming_dnd_pos: (f64, f64),
    data_tx: mpsc::Sender<HostAsyncData>,
    data_rx: mpsc::Receiver<HostAsyncData>,
    data_ping: ping::Ping,
    last_input_serial: u32,
    pool: SlotPool,
    window: Window,
    glass_manager: Option<RustyosLiquidGlassManagerV1>,
    surface_glass: Option<RustyosSurfaceGlassV1>,
    desired_glass: Option<LiquidGlassConfig>,
    applied_glass: Option<LiquidGlassConfig>,
    glass_dirty: bool,
    surface_app_menu: Option<RustyosSurfaceAppMenuV1>,
    desired_menu: Option<Vec<AppMenuSectionSpec>>,
    menu_dirty: bool,
    active_popup: Option<HostPopup>,
    desired_popup: Option<PopupConfig>,
    popup_dirty: bool,
    popup_redraw: bool,
    gles: Option<GlesSurfaceRenderer>,
    render_backend: RenderBackend,
    min_size: Option<(u32, u32)>,
    default_size: (u32, u32),
    width: u32,
    height: u32,
    scale_factor: i32,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    exit: bool,
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shape_manager: Option<CursorShapeManager>,
    cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    pointer_enter_serial: u32,
    desired_cursor: Option<CursorShape>,
    last_cursor_shape: Option<CursorShape>,
    modifiers: Modifiers,
    pointer_pos: (f64, f64),
    last_title: String,
    fonts: UiFontEngine,
    draw_list: UiDrawList,
    bgra_backbuf: Vec<u8>,
    app: Box<dyn RustyApp>,
}

impl HostState {
    fn clamp_dimensions(&self, width: u32, height: u32) -> (u32, u32) {
        if let Some((min_w, min_h)) = self.min_size {
            (width.max(min_w).max(1), height.max(min_h).max(1))
        } else {
            (width.max(1), height.max(1))
        }
    }

    fn with_ctx<R>(&mut self, f: impl FnOnce(&mut dyn RustyApp, &mut AppContext<'_>) -> R) -> R {
        let mut ctx = AppContext {
            conn: &self.conn,
            qh: &self.qh,
            window: &self.window,
            seat: self.seat.as_ref(),
            data_device_manager: self.data_device_manager.as_ref(),
            data_device: self.data_device.as_ref(),
            clipboard: &mut self.clipboard,
            active_mime_clipboard: &mut self.active_mime_clipboard,
            active_drag_source: &mut self.active_drag_source,
            data_tx: &self.data_tx,
            data_ping: &self.data_ping,
            fonts: &mut self.fonts,
            width: self.width,
            height: self.height,
            modifiers: self.modifiers,
            pointer_pos: self.pointer_pos,
            dirty: &mut self.dirty,
            exit: &mut self.exit,
            last_title: &mut self.last_title,
            desired_glass: &mut self.desired_glass,
            glass_dirty: &mut self.glass_dirty,
            desired_menu: &mut self.desired_menu,
            menu_dirty: &mut self.menu_dirty,
            desired_popup: &mut self.desired_popup,
            popup_dirty: &mut self.popup_dirty,
            popup_redraw: &mut self.popup_redraw,
            desired_cursor: &mut self.desired_cursor,
            gpu_active: self.gles.is_some(),
            has_glass: self.surface_glass.is_some(),
        };
        let out = f(&mut *self.app, &mut ctx);
        self.sync_cursor_shape();
        self.sync_text_clipboard();
        out
    }

    fn sync_text_clipboard(&mut self) {
        if let Some(text) = self.clipboard.take_pending_copy()
            && let (Some(ddm), Some(dd)) = (&self.data_device_manager, &self.data_device)
        {
            let source = ddm.create_copy_paste_source(&self.qh, rusty_ui::CLIPBOARD_MIME_TYPES);
            source.set_selection(dd, self.last_input_serial);
            self.clipboard.register_source(source.inner().clone(), text);
        }
        if self.clipboard.take_pending_paste_request()
            && let Some(dd) = &self.data_device
            && let Some(offer) = dd.data().selection_offer()
        {
            let mime = offer.with_mime_types(WaylandClipboardBridge::pick_utf8_mime);
            if let Some(mime) = mime
                && let Ok(read_pipe) = offer.receive(mime)
            {
                let _ = self.conn.flush();
                let wake = self.data_ping.clone();
                self.clipboard
                    .spawn_receive_from_fd(OwnedFd::from(read_pipe), move || wake.ping());
            }
        }
    }

    fn sync_cursor_shape(&mut self) {
        let Some(shape) = self.desired_cursor else {
            return;
        };
        if self.last_cursor_shape == Some(shape) || self.pointer_enter_serial == 0 {
            return;
        }
        if let Some(dev) = &self.cursor_shape_device {
            dev.set_shape(self.pointer_enter_serial, shape);
            self.last_cursor_shape = Some(shape);
        }
    }

    fn sync_popup(&mut self, qh: &QueueHandle<Self>) {
        if !self.popup_dirty {
            return;
        }
        self.popup_dirty = false;
        let Some(cfg) = self.desired_popup.clone() else {
            self.active_popup = None;
            return;
        };
        if !self.configured {
            self.popup_dirty = true;
            return;
        }
        self.active_popup = None;
        let Ok(positioner) = XdgPositioner::new(&self.xdg_shell) else {
            return;
        };
        let pw = i32::try_from(cfg.width.max(1)).unwrap_or(1).max(1);
        let ph = i32::try_from(cfg.height.max(1)).unwrap_or(1).max(1);
        positioner.set_size(pw, ph);
        positioner.set_anchor_rect(cfg.anchor_x, cfg.anchor_y, 1, 1);
        positioner.set_anchor(Anchor::TopLeft);
        positioner.set_gravity(Gravity::BottomRight);
        positioner.set_offset(cfg.offset_x, cfg.offset_y);
        positioner.set_constraint_adjustment(
            ConstraintAdjustment::FlipX
                | ConstraintAdjustment::FlipY
                | ConstraintAdjustment::SlideX
                | ConstraintAdjustment::SlideY,
        );
        let Ok(surface) = Surface::new(&self.compositor_state, qh) else {
            return;
        };
        let Ok(popup) = Popup::from_surface(
            Some(self.window.xdg_surface()),
            &positioner,
            qh,
            surface,
            &self.xdg_shell,
        ) else {
            return;
        };
        let glass = self
            .glass_manager
            .as_ref()
            .zip(cfg.glass)
            .map(|(mgr, gcfg)| {
                let g = mgr.get_surface_glass(popup.wl_surface(), qh, ());
                match gcfg.region {
                    GlassRegion::FullSurface => g.set_region(None),
                    GlassRegion::Rect {
                        x,
                        y,
                        width,
                        height,
                    } => {
                        if let Ok(region) = Region::new(&self.compositor_state) {
                            region.add(x, y, width, height);
                            g.set_region(Some(region.wl_region()));
                        }
                    }
                    GlassRegion::Empty => {
                        if let Ok(region) = Region::new(&self.compositor_state) {
                            g.set_region(Some(region.wl_region()));
                        }
                    }
                }
                g.set_corner_radius(gcfg.corner_radius);
                g.set_style(gcfg.style);
                if let Some([r, g_c, b, a]) = gcfg.tint {
                    g.set_tint(u32::from(r), u32::from(g_c), u32::from(b), u32::from(a));
                }
                g.commit();
                g
            });
        if let (Some(seat), Some(serial)) = (&self.seat, cfg.grab_serial) {
            popup.xdg_popup().grab(seat, serial);
        }
        popup.wl_surface().commit();
        self.active_popup = Some(HostPopup {
            popup,
            glass,
            width: cfg.width.max(1),
            height: cfg.height.max(1),
            configured: false,
            backbuf: Vec::new(),
        });
    }

    fn draw_popup(&mut self) {
        let Some(mut ap) = self.active_popup.take() else {
            self.popup_redraw = false;
            return;
        };
        if !ap.configured {
            self.active_popup = Some(ap);
            return;
        }
        let w = ap.width.max(1);
        let h = ap.height.max(1);
        let len = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        if ap.backbuf.len() != len {
            ap.backbuf.resize(len, 0);
        }
        let has_glass = ap.glass.is_some();
        let painted = self.app.paint_popup_bgra(&mut ap.backbuf, w, h, has_glass);
        if painted {
            let wi = i32::try_from(w).unwrap_or(1).max(1);
            let hi = i32::try_from(h).unwrap_or(1).max(1);
            let stride = wi.saturating_mul(4);
            if let Ok((buffer, canvas_bytes)) =
                self.pool
                    .create_buffer(wi, hi, stride, wl_shm::Format::Argb8888)
            {
                let copy_len = canvas_bytes.len().min(ap.backbuf.len());
                canvas_bytes[..copy_len].copy_from_slice(&ap.backbuf[..copy_len]);
                let surf = ap.popup.wl_surface();
                surf.set_buffer_scale(1);
                surf.damage_buffer(0, 0, wi, hi);
                let _ = buffer.attach_to(surf);
                surf.commit();
            }
        }
        self.popup_redraw = false;
        self.active_popup = Some(ap);
    }

    fn ensure_gles_initialized(&mut self) {
        if self.render_backend != RenderBackend::Gpu || self.gles.is_some() {
            return;
        }
        let w = i32::try_from(self.width).unwrap_or(1).max(1);
        let h = i32::try_from(self.height).unwrap_or(1).max(1);
        match GlesSurfaceRenderer::new(
            self.conn.backend().display_ptr().cast(),
            self.window.wl_surface(),
            w,
            h,
        ) {
            Ok(renderer) => {
                self.gles = Some(renderer);
            }
            Err(err) => {
                tracing::warn!(%err, "rusty-app: GLES2 unavailable; falling back to CPU wl_shm");
                self.render_backend = RenderBackend::Cpu;
            }
        }
    }

    fn sync_liquid_glass(&mut self) {
        let Some(ref glass) = self.surface_glass else {
            self.glass_dirty = false;
            return;
        };
        if !self.glass_dirty && self.applied_glass == self.desired_glass {
            return;
        }
        self.glass_dirty = false;
        self.applied_glass = self.desired_glass;

        if let Some(cfg) = self.desired_glass {
            match cfg.region {
                GlassRegion::FullSurface => {
                    glass.set_region(None);
                }
                GlassRegion::Rect {
                    x,
                    y,
                    width,
                    height,
                } => {
                    if let Ok(region) = Region::new(&self.compositor_state) {
                        region.add(x, y, width.max(1), height.max(1));
                        glass.set_region(Some(region.wl_region()));
                    }
                }
                GlassRegion::Empty => {
                    if let Ok(region) = Region::new(&self.compositor_state) {
                        glass.set_region(Some(region.wl_region()));
                    }
                }
            }
            glass.set_corner_radius(cfg.corner_radius);
            glass.set_style(cfg.style);
            if let Some([r, g, b, a]) = cfg.tint {
                glass.set_tint(u32::from(r), u32::from(g), u32::from(b), u32::from(a));
            }
            glass.commit();
        } else {
            if let Ok(region) = Region::new(&self.compositor_state) {
                glass.set_region(Some(region.wl_region()));
            }
            glass.commit();
        }
    }

    fn sync_app_menu(&mut self) {
        let Some(ref menu_ctrl) = self.surface_app_menu else {
            self.menu_dirty = false;
            return;
        };
        if !self.menu_dirty {
            return;
        }
        self.menu_dirty = false;
        menu_ctrl.begin_update();
        if let Some(ref sections) = self.desired_menu {
            for sec in sections {
                menu_ctrl.begin_section(sec.id, sec.title.clone());
                for item in &sec.items {
                    let checked = match item.checked {
                        Some(true) => 1,
                        Some(false) => 0,
                        None => -1,
                    };
                    menu_ctrl.add_item(
                        item.id,
                        item.label.clone(),
                        item.shortcut.clone().unwrap_or_default(),
                        u32::from(item.enabled),
                        u32::from(item.separator),
                        checked,
                    );
                }
            }
        }
        menu_ctrl.commit();
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured {
            return;
        }

        let (w, h) = self.clamp_dimensions(self.width, self.height);
        self.width = w;
        self.height = h;
        let wi = i32::try_from(w).unwrap_or(1).max(1);
        let hi = i32::try_from(h).unwrap_or(1).max(1);

        self.sync_liquid_glass();
        self.sync_app_menu();

        let expected_len = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        if self.bgra_backbuf.len() != expected_len {
            self.bgra_backbuf.resize(expected_len, 0);
        }
        let has_glass = self.surface_glass.is_some();
        let used_bgra = self.app.paint_bgra(&mut self.bgra_backbuf, w, h, has_glass);

        if !used_bgra {
            self.draw_list.clear();
            let mut painter = UiPainter::new(&mut self.draw_list);
            self.app.paint(&mut painter, &mut self.fonts, w, h);
        }
        let clear = self.app.clear_color();

        let surf = self.window.wl_surface();
        surf.frame(qh, surf.clone());
        let scale = self.scale_factor.max(1);

        if let Some(gles) = self.gles.as_mut() {
            let res = if used_bgra {
                surf.set_buffer_scale(1);
                gles.present_bgra_frame(&self.bgra_backbuf, w, h, 1, None, clear)
            } else {
                surf.set_buffer_scale(scale);
                gles.render_draw_list_scaled(&self.draw_list, &mut self.fonts, wi, hi, scale, clear)
            };
            match res {
                Ok(()) => {
                    self.frame_pending = true;
                    self.dirty = false;
                    return;
                }
                Err(err) => {
                    tracing::warn!(%err, "rusty-app: GLES2 frame failed; switching to CPU wl_shm");
                    self.gles = None;
                    self.render_backend = RenderBackend::Cpu;
                }
            }
        }

        surf.set_buffer_scale(1);

        let stride = wi.saturating_mul(4);
        let Ok((buffer, canvas_bytes)) =
            self.pool
                .create_buffer(wi, hi, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };

        if used_bgra {
            let copy_len = canvas_bytes.len().min(self.bgra_backbuf.len());
            canvas_bytes[..copy_len].copy_from_slice(&self.bgra_backbuf[..copy_len]);
        } else {
            let stride_usize = usize::try_from(stride).unwrap_or(0);
            let mut canvas = UiCanvas::new(canvas_bytes, w, h, stride_usize, PixelFormat::Bgra8888);
            canvas.clear(clear);
            render_draw_list_to_canvas(&self.draw_list, &mut canvas, &mut self.fonts);
        }

        surf.damage_buffer(0, 0, wi, hi);
        let _ = buffer.attach_to(surf);
        surf.commit();
        self.frame_pending = true;
        self.dirty = false;
    }
}

impl Dispatch<RustyosLiquidGlassManagerV1, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &RustyosLiquidGlassManagerV1,
        _event: <RustyosLiquidGlassManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RustyosSurfaceGlassV1, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &RustyosSurfaceGlassV1,
        _event: <RustyosSurfaceGlassV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RustyosAppMenuManagerV1, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &RustyosAppMenuManagerV1,
        _event: <RustyosAppMenuManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RustyosSurfaceAppMenuV1, ()> for HostState {
    fn event(
        state: &mut Self,
        _proxy: &RustyosSurfaceAppMenuV1,
        event: SurfaceAppMenuEvent,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let SurfaceAppMenuEvent::ItemActivated { id } = event;
        state.with_ctx(|app, ctx| app.on_menu_item(ctx, id));
        state.sync_app_menu();
        if state.dirty && !state.exit && !state.frame_pending {
            state.draw(qh);
        }
    }
}

impl CompositorHandler for HostState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let scale = new_factor.max(1);
        if self.window.wl_surface() == surface && self.scale_factor != scale {
            self.scale_factor = scale;
            self.dirty = true;
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        if self.dirty {
            self.draw(qh);
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for HostState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for HostState {
    fn request_close(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &Window) {
        let should_exit = self.with_ctx(|app, ctx| app.on_close_requested(ctx));
        if should_exit {
            self.exit = true;
        } else if self.dirty {
            self.draw(qh);
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let (w_opt, h_opt) = configure.new_size;
        let raw_w = w_opt.map_or(self.default_size.0, std::num::NonZeroU32::get);
        let raw_h = h_opt.map_or(self.default_size.1, std::num::NonZeroU32::get);
        let (new_w, new_h) = self.clamp_dimensions(raw_w, raw_h);
        self.width = new_w;
        self.height = new_h;
        self.configured = true;
        self.frame_pending = false;
        self.ensure_gles_initialized();

        let info = ConfigureInfo {
            width: new_w,
            height: new_h,
            is_maximized: configure.is_maximized(),
            is_fullscreen: configure.is_fullscreen(),
            is_activated: configure.is_activated(),
        };
        self.with_ctx(|app, ctx| app.on_configure(ctx, info));
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for HostState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if self.data_device.is_none()
            && let Some(ddm) = &self.data_device_manager
        {
            self.data_device = Some(ddm.get_data_device(qh, &seat));
        }
        self.seat = Some(seat);
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if self.data_device.is_none()
            && let Some(ddm) = &self.data_device_manager
        {
            self.data_device = Some(ddm.get_data_device(qh, &seat));
        }
        self.seat = Some(seat.clone());
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            let ptr = self.seat_state.get_pointer(qh, &seat).ok();
            if let (Some(p), Some(mgr)) = (&ptr, &self.cursor_shape_manager) {
                self.cursor_shape_device = Some(mgr.get_shape_device(p, qh));
            }
            self.pointer = ptr;
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && let Some(kb) = self.keyboard.take()
        {
            kb.release();
        }
        if capability == Capability::Pointer
            && let Some(pt) = self.pointer.take()
        {
            if let Some(dev) = self.cursor_shape_device.take() {
                dev.destroy();
            }
            pt.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {
        self.seat = None;
    }
}

impl KeyboardHandler for HostState {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.last_input_serial = serial;
        self.with_ctx(|app, ctx| app.on_key(ctx, event));
        if self.dirty && !self.exit && !self.frame_pending {
            self.draw(qh);
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for HostState {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let popup_surface = self
            .active_popup
            .as_ref()
            .map(|ap| ap.popup.wl_surface().clone());
        let mut main_events = Vec::new();
        let mut popup_events = Vec::new();
        for ev in events {
            if popup_surface.as_ref() == Some(&ev.surface) {
                popup_events.push(ev.clone());
                continue;
            }
            if let PointerEventKind::Enter { serial } = ev.kind {
                self.pointer_enter_serial = serial;
                self.last_cursor_shape = None;
            } else if let PointerEventKind::Press { serial, .. } = ev.kind {
                self.last_input_serial = serial;
            }
            if matches!(
                ev.kind,
                PointerEventKind::Enter { .. }
                    | PointerEventKind::Motion { .. }
                    | PointerEventKind::Press { .. }
            ) {
                self.pointer_pos = ev.position;
            }
            main_events.push(ev.clone());
        }
        if !popup_events.is_empty() {
            self.with_ctx(|app, ctx| app.on_popup_pointer(ctx, &popup_events));
        }
        if !main_events.is_empty() {
            self.with_ctx(|app, ctx| app.on_pointer(ctx, &main_events));
        }
        self.sync_popup(qh);
        if self.popup_redraw {
            self.draw_popup();
        }
        if self.dirty && !self.exit && !self.frame_pending {
            self.draw(qh);
        }
    }
}

impl PopupHandler for HostState {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        if let Some(ap) = &mut self.active_popup
            && ap.popup.wl_surface() == popup.wl_surface()
        {
            ap.width = u32::try_from(config.width.max(1)).unwrap_or(1);
            ap.height = u32::try_from(config.height.max(1)).unwrap_or(1);
            ap.configured = true;
            self.popup_redraw = true;
            self.draw_popup();
        }
    }

    fn done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, popup: &Popup) {
        if let Some(ap) = &self.active_popup
            && ap.popup.wl_surface() == popup.wl_surface()
        {
            self.active_popup = None;
            self.desired_popup = None;
            self.popup_dirty = false;
            self.popup_redraw = false;
            self.with_ctx(|app, ctx| app.on_popup_closed(ctx));
        }
    }
}

impl ShmHandler for HostState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl DataOfferHandler for HostState {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl DataSourceHandler for HostState {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
        mime: String,
        mut write_pipe: WritePipe,
    ) {
        if let Some(active) = self
            .active_drag_source
            .as_ref()
            .filter(|a| a.source == *source)
            .or_else(|| {
                self.active_mime_clipboard
                    .as_ref()
                    .filter(|a| a.source == *source)
            })
        {
            let payload = active
                .entries
                .iter()
                .find(|(m, _)| m.eq_ignore_ascii_case(&mime))
                .or_else(|| active.entries.first())
                .map_or_default(|(_, bytes)| bytes.clone());
            let _ = std::thread::Builder::new()
                .name(String::from("rusty-app-data-send"))
                .spawn(move || {
                    let _ = write_pipe.write_all(&payload);
                });
            return;
        }
        self.clipboard
            .serve_send_request(source, &mime, OwnedFd::from(write_pipe));
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
    ) {
        if self
            .active_drag_source
            .as_ref()
            .is_some_and(|a| a.source == *source)
        {
            self.active_drag_source = None;
            source.destroy();
        }
        if self
            .active_mime_clipboard
            .as_ref()
            .is_some_and(|a| a.source == *source)
        {
            self.active_mime_clipboard = None;
            source.destroy();
        }
        self.clipboard.remove_source(source);
    }

    fn dnd_dropped(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }

    fn dnd_finished(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
    ) {
        if let Some(active) = self.active_drag_source.take()
            && active.source == *source
        {
            let moved = active.selected_action == DndAction::Move;
            source.destroy();
            self.with_ctx(|app, ctx| app.on_drag_finished(ctx, moved));
        }
    }

    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
        action: DndAction,
    ) {
        if let Some(active) = self.active_drag_source.as_mut()
            && active.source == *source
        {
            active.selected_action = action;
        }
    }
}

impl DataDeviceHandler for HostState {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        x: f64,
        y: f64,
        _: &wl_surface::WlSurface,
    ) {
        self.incoming_dnd_pos = (x, y);
        self.update_dnd_offer(x, y);
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {
        self.with_ctx(|app, ctx| app.on_dnd_leave(ctx));
    }

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        x: f64,
        y: f64,
    ) {
        self.incoming_dnd_pos = (x, y);
        self.update_dnd_offer(x, y);
    }

    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
        self.with_ctx(|app, ctx| app.on_clipboard_offer_changed(ctx));
    }

    fn drop_performed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
        let Some(offer) = self
            .data_device
            .as_ref()
            .and_then(|dd| dd.data().drag_offer())
        else {
            return;
        };
        let mime = String::from("text/uri-list");
        if !offer.with_mime_types(|mimes| mimes.iter().any(|m| m == &mime)) {
            return;
        }
        let is_move = offer.selected_action == DndAction::Move;
        let Ok(mut pipe) = offer.receive(mime.clone()) else {
            return;
        };
        offer.finish();
        let (x, y) = self.incoming_dnd_pos;
        let tx = self.data_tx.clone();
        let ping = self.data_ping.clone();
        let _ = std::thread::Builder::new()
            .name(String::from("rusty-app-dnd-recv"))
            .spawn(move || {
                let mut bytes = Vec::new();
                let _ = pipe.by_ref().take(4 * 1024 * 1024).read_to_end(&mut bytes);
                if tx
                    .send(HostAsyncData::DndDrop {
                        x,
                        y,
                        is_move,
                        mime,
                        bytes,
                    })
                    .is_ok()
                {
                    ping.ping();
                }
            });
    }
}

impl HostState {
    fn update_dnd_offer(&mut self, x: f64, y: f64) {
        let mimes = self
            .data_device
            .as_ref()
            .and_then(|dd| dd.data().drag_offer())
            .map_or_default(|offer| offer.with_mime_types(<[String]>::to_vec));
        if mimes.is_empty() {
            return;
        }
        let accepted = self.with_ctx(|app, ctx| app.on_dnd_motion(ctx, x, y, &mimes));
        if let Some(offer) = self
            .data_device
            .as_ref()
            .and_then(|dd| dd.data().drag_offer())
        {
            if accepted {
                offer.accept_mime_type(offer.serial, Some(String::from("text/uri-list")));
                offer.set_actions(DndAction::Copy | DndAction::Move, DndAction::Copy);
            } else {
                offer.accept_mime_type(offer.serial, None);
            }
        }
    }
}

delegate_data_device!(HostState);

impl ProvidesRegistryState for HostState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(HostState);
delegate_compositor!(HostState);
delegate_output!(HostState);
delegate_shm!(HostState);
delegate_seat!(HostState);
delegate_keyboard!(HostState);
delegate_pointer!(HostState);
delegate_xdg_shell!(HostState);
delegate_xdg_window!(HostState);
delegate_xdg_popup!(HostState);

/// Runs a [`RustyApp`] inside the shared RustyOS Wayland + GLES/SHM + Liquid Glass host.
///
/// # Errors
///
/// Returns [`AppError`] if connecting to Wayland, binding core globals, allocating the fallback
/// `wl_shm` pool, or initializing `calloop` fails.
pub fn run_app<A: RustyApp + 'static>(config: AppConfig, app: A) -> Result<(), AppError> {
    let conn =
        Connection::connect_to_env().map_err(|err| AppError::WaylandConnect(err.to_string()))?;
    let (globals, event_queue) = registry_queue_init::<HostState>(&conn)
        .map_err(|e| AppError::RegistryInit(e.to_string()))?;

    let qh = event_queue.handle();
    let compositor_state = CompositorState::bind(&globals, &qh)
        .map_err(|_| AppError::MissingGlobal("wl_compositor"))?;
    let xdg_shell =
        XdgShell::bind(&globals, &qh).map_err(|_| AppError::MissingGlobal("xdg_wm_base"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|_| AppError::MissingGlobal("wl_shm"))?;

    let init_w = config.default_size.0.max(1);
    let init_h = config.default_size.1.max(1);
    let pool_bytes = usize::try_from(init_w.saturating_mul(init_h).saturating_mul(4))
        .unwrap_or(1_048_576)
        .max(4096);
    let pool = SlotPool::new(pool_bytes, &shm).map_err(|err| AppError::ShmPool(err.to_string()))?;

    let surface = compositor_state.create_surface(&qh);
    let (data_tx, data_rx) = mpsc::channel();
    let (data_ping, data_ping_source) =
        ping::make_ping().map_err(|err| AppError::EventLoop(err.to_string()))?;

    let window = xdg_shell.create_window(surface, config.decorations.to_sctk(), &qh);
    window.set_title(&config.title);
    window.set_app_id(&config.app_id);
    if let Some((min_w, min_h)) = config.min_size {
        window.set_min_size(Some((min_w, min_h)));
    }
    if config.fullscreen {
        window.set_fullscreen(None);
    }
    window.commit();

    let host = build_host_state(
        &conn,
        &qh,
        &globals,
        pool,
        window,
        config,
        Box::new(app),
        data_tx,
        data_rx,
        data_ping,
    );
    run_host_loop(conn, event_queue, &qh, data_ping_source, host)
}

#[allow(clippy::too_many_arguments)]
fn build_host_state(
    conn: &Connection,
    qh: &QueueHandle<HostState>,
    globals: &wayland_client::globals::GlobalList,
    pool: SlotPool,
    window: Window,
    config: AppConfig,
    app: Box<dyn RustyApp>,
    data_tx: mpsc::Sender<HostAsyncData>,
    data_rx: mpsc::Receiver<HostAsyncData>,
    data_ping: ping::Ping,
) -> HostState {
    let init_w = config.default_size.0.max(1);
    let init_h = config.default_size.1.max(1);
    let compositor_state = CompositorState::bind(globals, qh)
        .unwrap_or_else(|_| unreachable!("wl_compositor already bound"));
    let xdg_shell =
        XdgShell::bind(globals, qh).unwrap_or_else(|_| unreachable!("xdg_wm_base already bound"));
    let shm = Shm::bind(globals, qh).unwrap_or_else(|_| unreachable!("wl_shm already bound"));
    let glass_manager = globals
        .bind::<RustyosLiquidGlassManagerV1, _, _>(qh, 1..=1, ())
        .ok();
    let surface_glass = glass_manager
        .as_ref()
        .map(|mgr| mgr.get_surface_glass(window.wl_surface(), qh, ()));
    let surface_app_menu = globals
        .bind::<RustyosAppMenuManagerV1, _, _>(qh, 1..=1, ())
        .ok()
        .map(|mgr| mgr.get_menu_surface(window.wl_surface(), qh, ()));
    let cursor_shape_manager = CursorShapeManager::bind(globals, qh).ok();
    let data_device_manager = DataDeviceManagerState::bind(globals, qh).ok();

    HostState {
        conn: conn.clone(),
        qh: qh.clone(),
        registry_state: RegistryState::new(globals),
        compositor_state,
        xdg_shell,
        seat_state: SeatState::new(globals, qh),
        output_state: OutputState::new(globals, qh),
        shm,
        data_device_manager,
        data_device: None,
        clipboard: WaylandClipboardBridge::new(),
        active_mime_clipboard: None,
        active_drag_source: None,
        incoming_dnd_pos: (0.0, 0.0),
        data_tx,
        data_rx,
        data_ping,
        last_input_serial: 0,
        pool,
        window,
        glass_manager,
        surface_glass,
        desired_glass: config.glass,
        applied_glass: None,
        glass_dirty: config.glass.is_some(),
        surface_app_menu,
        desired_menu: None,
        menu_dirty: false,
        active_popup: None,
        desired_popup: None,
        popup_dirty: false,
        popup_redraw: false,
        gles: None,
        render_backend: if config.no_gpu {
            RenderBackend::Cpu
        } else {
            RenderBackend::Gpu
        },
        min_size: config.min_size,
        default_size: (init_w, init_h),
        width: init_w,
        height: init_h,
        scale_factor: 1,
        configured: false,
        frame_pending: false,
        dirty: true,
        exit: false,
        seat: None,
        keyboard: None,
        pointer: None,
        cursor_shape_manager,
        cursor_shape_device: None,
        pointer_enter_serial: 0,
        desired_cursor: None,
        last_cursor_shape: None,
        modifiers: Modifiers::default(),
        pointer_pos: (0.0, 0.0),
        last_title: config.title,
        fonts: UiFontEngine::new(),
        draw_list: UiDrawList::new(),
        bgra_backbuf: Vec::new(),
        app,
    }
}

fn run_host_loop(
    conn: Connection,
    event_queue: wayland_client::EventQueue<HostState>,
    qh: &QueueHandle<HostState>,
    data_ping_source: ping::PingSource,
    mut host: HostState,
) -> Result<(), AppError> {
    let mut event_loop =
        EventLoop::<HostState>::try_new().map_err(|err| AppError::EventLoop(err.to_string()))?;
    WaylandSource::new(conn, event_queue)
        .insert(event_loop.handle())
        .map_err(|err| AppError::EventLoop(err.to_string()))?;
    event_loop
        .handle()
        .insert_source(data_ping_source, |(), (), state| {
            while let Ok(msg) = state.data_rx.try_recv() {
                match msg {
                    HostAsyncData::ClipboardData { mime, bytes } => {
                        state.with_ctx(|app, ctx| app.on_clipboard_data(ctx, &mime, &bytes));
                    }
                    HostAsyncData::DndDrop {
                        x,
                        y,
                        is_move,
                        mime,
                        bytes,
                    } => {
                        state.with_ctx(|app, ctx| {
                            app.on_dnd_drop(ctx, x, y, is_move, &mime, &bytes);
                        });
                    }
                }
            }
        })
        .map_err(|err| AppError::EventLoop(err.to_string()))?;

    while !host.exit {
        host.with_ctx(|app_ref, ctx| app_ref.on_tick(ctx));
        if host.exit {
            break;
        }
        host.sync_popup(qh);
        if host.popup_redraw {
            host.draw_popup();
        }
        if host.configured && host.dirty && !host.frame_pending {
            host.draw(qh);
        }
        let timeout = host.app.poll_interval();
        if let Err(err) = event_loop.dispatch(Some(timeout), &mut host) {
            return Err(AppError::EventLoop(err.to_string()));
        }
    }

    Ok(())
}
