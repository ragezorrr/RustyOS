//! Shared Wayland/SCTK + `calloop` application host with GPU GLES2 and CPU `wl_shm` fallback.

use std::fmt;
use std::time::Duration;

use rusty_ui::{
    GlesSurfaceRenderer, PixelFormat, UiCanvas, UiDrawList, UiFontEngine, UiPainter,
    render_draw_list_to_canvas,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{calloop::EventLoop, calloop_wayland_source::WaylandSource},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
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

    /// Sets the initial Liquid Glass backdrop configuration.
    #[must_use]
    pub const fn with_liquid_glass(mut self, glass: LiquidGlassConfig) -> Self {
        self.glass = Some(glass);
        self
    }
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

/// Mutable window & runtime context passed to [`RustyApp`] event callbacks.
pub struct AppContext<'a> {
    window: &'a Window,
    seat: Option<&'a wl_seat::WlSeat>,
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
    gpu_active: bool,
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

    /// Updates the declarative Liquid Glass backdrop configuration for this window.
    pub fn set_liquid_glass(&mut self, config: Option<LiquidGlassConfig>) {
        if *self.desired_glass != config {
            *self.desired_glass = config;
            *self.glass_dirty = true;
            *self.dirty = true;
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
}

/// Application trait implemented by RustyOS native Wayland apps hosted by [`run_app`].
pub trait RustyApp {
    /// Called whenever the compositor configures window geometry or state.
    fn on_configure(&mut self, _ctx: &mut AppContext<'_>, _info: ConfigureInfo) {}

    /// Called on keyboard key press.
    fn on_key(&mut self, _ctx: &mut AppContext<'_>, _event: KeyEvent) {}

    /// Called for each batch of Wayland pointer events.
    fn on_pointer(&mut self, _ctx: &mut AppContext<'_>, _events: &[PointerEvent]) {}

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderBackend {
    Gpu,
    Cpu,
}

#[allow(clippy::struct_excessive_bools)]
struct HostState {
    conn: Connection,
    registry_state: RegistryState,
    compositor_state: CompositorState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    surface_glass: Option<RustyosSurfaceGlassV1>,
    desired_glass: Option<LiquidGlassConfig>,
    applied_glass: Option<LiquidGlassConfig>,
    glass_dirty: bool,
    gles: Option<GlesSurfaceRenderer>,
    render_backend: RenderBackend,
    min_size: Option<(u32, u32)>,
    default_size: (u32, u32),
    width: u32,
    height: u32,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    exit: bool,
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    modifiers: Modifiers,
    pointer_pos: (f64, f64),
    last_title: String,
    fonts: UiFontEngine,
    draw_list: UiDrawList,
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
            window: &self.window,
            seat: self.seat.as_ref(),
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
            gpu_active: self.gles.is_some(),
        };
        f(&mut *self.app, &mut ctx)
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
        self.draw_list.clear();
        {
            let mut painter = UiPainter::new(&mut self.draw_list);
            self.app.paint(&mut painter, &mut self.fonts, w, h);
        }
        let clear = self.app.clear_color();

        let surf = self.window.wl_surface();
        surf.frame(qh, surf.clone());

        if let Some(gles) = self.gles.as_mut() {
            match gles.render_draw_list(&self.draw_list, &mut self.fonts, wi, hi, clear) {
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

        let stride = wi.saturating_mul(4);
        let Ok((buffer, canvas_bytes)) =
            self.pool
                .create_buffer(wi, hi, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };

        let stride_usize = usize::try_from(stride).unwrap_or(0);
        let mut canvas = UiCanvas::new(canvas_bytes, w, h, stride_usize, PixelFormat::Bgra8888);
        canvas.clear(clear);
        render_draw_list_to_canvas(&self.draw_list, &mut canvas, &mut self.fonts);

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

impl CompositorHandler for HostState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
        self.dirty = true;
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

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.seat = Some(seat);
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        self.seat = Some(seat.clone());
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
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
        _: u32,
        event: KeyEvent,
    ) {
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
        for ev in events {
            if matches!(
                ev.kind,
                PointerEventKind::Enter { .. }
                    | PointerEventKind::Motion { .. }
                    | PointerEventKind::Press { .. }
            ) {
                self.pointer_pos = ev.position;
            }
        }
        self.with_ctx(|app, ctx| app.on_pointer(ctx, events));
        if self.dirty && !self.exit && !self.frame_pending {
            self.draw(qh);
        }
    }
}

impl ShmHandler for HostState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

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
    let surface_glass = globals
        .bind::<RustyosLiquidGlassManagerV1, _, _>(&qh, 1..=1, ())
        .ok()
        .map(|mgr| mgr.get_surface_glass(&surface, &qh, ()));

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

    let mut host = HostState {
        conn: conn.clone(),
        registry_state: RegistryState::new(&globals),
        compositor_state,
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        surface_glass,
        desired_glass: config.glass,
        applied_glass: None,
        glass_dirty: config.glass.is_some(),
        gles: None,
        render_backend: RenderBackend::Gpu,
        min_size: config.min_size,
        default_size: (init_w, init_h),
        width: init_w,
        height: init_h,
        configured: false,
        frame_pending: false,
        dirty: true,
        exit: false,
        seat: None,
        keyboard: None,
        pointer: None,
        modifiers: Modifiers::default(),
        pointer_pos: (0.0, 0.0),
        last_title: config.title,
        fonts: UiFontEngine::new(),
        draw_list: UiDrawList::new(),
        app: Box::new(app),
    };

    let mut event_loop =
        EventLoop::<HostState>::try_new().map_err(|err| AppError::EventLoop(err.to_string()))?;
    WaylandSource::new(conn, event_queue)
        .insert(event_loop.handle())
        .map_err(|err| AppError::EventLoop(err.to_string()))?;

    while !host.exit {
        host.with_ctx(|app_ref, ctx| app_ref.on_tick(ctx));
        if host.exit {
            break;
        }
        if host.configured && host.dirty && !host.frame_pending {
            host.draw(&qh);
        }
        let timeout = host.app.poll_interval();
        if let Err(err) = event_loop.dispatch(Some(timeout), &mut host) {
            return Err(AppError::EventLoop(err.to_string()));
        }
    }

    Ok(())
}
