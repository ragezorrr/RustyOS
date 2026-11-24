//! Wayland CSD (`xdg_toplevel`) event loop, EGL/SHM presentation, Liquid Glass,
//! XKB keyboard/repeat, pointer selection/scroll, and clipboard integration (`RUSTYOS-113`).

use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
    },
    delegate_compositor, delegate_data_device, delegate_keyboard, delegate_output,
    delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle, ping::make_ping},
        calloop_wayland_source::WaylandSource,
        protocols::{
            wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
                Shape as CursorShape, WpCursorShapeDeviceV1,
            },
            xdg::shell::client::xdg_toplevel::ResizeEdge,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{
            AxisScroll, BTN_LEFT, BTN_MIDDLE, PointerEvent, PointerEventKind, PointerHandler,
            cursor_shape::CursorShapeManager,
        },
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
    backend::WaylandError,
    globals::registry_queue_init,
    protocol::{
        wl_data_device, wl_data_device_manager::DndAction, wl_data_source, wl_keyboard, wl_output,
        wl_pointer, wl_seat, wl_shm, wl_surface,
    },
};

use crate::gpu_egl::GpuRenderer;
use crate::layout::{DEFAULT_HEIGHT, DEFAULT_WIDTH, MIN_HEIGHT, MIN_WIDTH, WINDOW_CORNER_RADIUS};
use crate::liquid_glass::{GlassStyle, RustyosLiquidGlassManagerV1, RustyosSurfaceGlassV1};
use crate::renderer_shm::ShmFallbackRenderer;
use crate::state::{DamageLevel, TerminalAction, TerminalAppState};

const CLIPBOARD_MIMES: &[&str] = &[
    "text/plain;charset=utf-8",
    "text/plain; charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
];
const RESIZE_BORDER_PX: f64 = 5.0;

/// Startup configuration passed from CLI arguments.
#[derive(Debug, Clone)]
pub(crate) struct StartupOptions {
    pub(crate) force_shm: bool,
    pub(crate) program: Option<PathBuf>,
    pub(crate) program_args: Vec<String>,
    pub(crate) working_dir: Option<PathBuf>,
    pub(crate) title: Option<String>,
}

#[derive(Debug)]
pub(crate) enum WaylandAppError {
    Connect(wayland_client::ConnectError),
    Global(wayland_client::globals::GlobalError),
    Bind(wayland_client::globals::BindError),
    Dispatch(wayland_client::DispatchError),
    Backend(WaylandError),
    EventLoop(String),
}

impl fmt::Display for WaylandAppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "Wayland connect error: {e}"),
            Self::Global(e) => write!(f, "Wayland global error: {e}"),
            Self::Bind(e) => write!(f, "Wayland bind error: {e}"),
            Self::Dispatch(e) => write!(f, "Wayland dispatch error: {e}"),
            Self::Backend(e) => write!(f, "Wayland backend error: {e}"),
            Self::EventLoop(e) => write!(f, "Event loop error: {e}"),
        }
    }
}

impl std::error::Error for WaylandAppError {}

impl From<wayland_client::ConnectError> for WaylandAppError {
    fn from(value: wayland_client::ConnectError) -> Self {
        Self::Connect(value)
    }
}
impl From<wayland_client::globals::GlobalError> for WaylandAppError {
    fn from(value: wayland_client::globals::GlobalError) -> Self {
        Self::Global(value)
    }
}
impl From<wayland_client::globals::BindError> for WaylandAppError {
    fn from(value: wayland_client::globals::BindError) -> Self {
        Self::Bind(value)
    }
}
impl From<wayland_client::DispatchError> for WaylandAppError {
    fn from(value: wayland_client::DispatchError) -> Self {
        Self::Dispatch(value)
    }
}
impl From<WaylandError> for WaylandAppError {
    fn from(value: WaylandError) -> Self {
        Self::Backend(value)
    }
}

enum RenderBackend {
    Gpu(Box<GpuRenderer>),
    Shm {
        renderer: ShmFallbackRenderer,
        pool: SlotPool,
    },
}

#[allow(clippy::struct_excessive_bools)]
struct TerminalWaylandApp {
    conn: Connection,
    qh: QueueHandle<Self>,
    loop_handle: LoopHandle<'static, Self>,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    data_device_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    copy_sources: Vec<(CopyPasteSource, String)>,
    paste_tx: mpsc::Sender<String>,
    paste_rx: mpsc::Receiver<String>,
    liquid_glass_mgr: Option<RustyosLiquidGlassManagerV1>,
    surface_glass: Option<RustyosSurfaceGlassV1>,
    window: Window,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shape_manager: Option<CursorShapeManager>,
    cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    pointer_enter_serial: u32,
    last_cursor_shape: Option<CursorShape>,
    active_seat: Option<wl_seat::WlSeat>,
    last_input_serial: u32,
    configured: bool,
    initial_pty_spawned: bool,
    initial_title: Option<String>,
    last_window_title: String,
    buffer_scale: i32,
    shm_scratch: Vec<u8>,
    frame_pending: bool,
    last_frame_sent: Instant,
    exit: bool,
    force_shm: bool,
    backend: Option<RenderBackend>,
    app: TerminalAppState,
    paste_ping: smithay_client_toolkit::reexports::calloop::ping::Ping,
}

impl Drop for TerminalWaylandApp {
    fn drop(&mut self) {
        // Destroy EGL surface/context strictly before `wl_surface` drops (`WlSurface` lives in `Window`).
        self.backend = None;
        if let Some(glass) = self.surface_glass.take() {
            glass.destroy();
        }
    }
}

/// Runs the Wayland event loop cleanly until window close or last tab exits.
pub(crate) fn run(opts: StartupOptions) -> Result<(), WaylandAppError> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh: QueueHandle<TerminalWaylandApp> = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let data_device_manager = DataDeviceManagerState::bind(&globals, &qh).ok();
    let liquid_glass_mgr = globals
        .bind::<RustyosLiquidGlassManagerV1, _, _>(&qh, 1..=1, ())
        .ok();

    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestClient, &qh);
    window.set_title(opts.title.as_deref().unwrap_or("Terminal"));
    window.set_app_id("org.rustyos.Terminal");
    window.set_min_size(Some((
        u32::try_from(MIN_WIDTH).unwrap_or(480),
        u32::try_from(MIN_HEIGHT).unwrap_or(320),
    )));
    window.commit();

    let mut event_loop: EventLoop<'static, TerminalWaylandApp> =
        EventLoop::try_new().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let loop_handle = event_loop.handle();
    let (wake_ping, wake_source) =
        make_ping().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    loop_handle
        .insert_source(wake_source, |(), (), state| {
            let _ = state.app.drain_pty_events();
        })
        .map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;

    WaylandSource::new(conn.clone(), event_queue)
        .insert(loop_handle.clone())
        .map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;

    let (paste_tx, paste_rx) = mpsc::channel();
    let app_state = TerminalAppState::new(
        DEFAULT_WIDTH,
        DEFAULT_HEIGHT,
        opts.program,
        opts.program_args,
        opts.working_dir,
        None,
        Some(wake_ping.clone()),
        false,
    );
    let seat_state = SeatState::new(&globals, &qh);
    let initial_seat = seat_state.seats().next();
    let initial_data_device = initial_seat.as_ref().and_then(|s| {
        data_device_manager
            .as_ref()
            .map(|m| m.get_data_device(&qh, s))
    });

    let mut wayland_app = TerminalWaylandApp {
        conn: conn.clone(),
        qh: qh.clone(),
        loop_handle: loop_handle.clone(),
        registry_state: RegistryState::new(&globals),
        seat_state,
        output_state: OutputState::new(&globals, &qh),
        shm,
        data_device_manager,
        data_device: initial_data_device,
        copy_sources: Vec::new(),
        paste_tx,
        paste_rx,
        liquid_glass_mgr,
        surface_glass: None,
        window,
        keyboard: None,
        pointer: None,
        cursor_shape_manager: CursorShapeManager::bind(&globals, &qh).ok(),
        cursor_shape_device: None,
        pointer_enter_serial: 0,
        last_cursor_shape: None,
        active_seat: initial_seat,
        last_input_serial: 0,
        configured: false,
        initial_pty_spawned: false,
        initial_title: opts.title,
        last_window_title: String::new(),
        buffer_scale: 1,
        shm_scratch: Vec::new(),
        frame_pending: false,
        last_frame_sent: Instant::now(),
        exit: false,
        force_shm: opts.force_shm,
        backend: None,
        app: app_state,
        paste_ping: wake_ping,
    };

    run_event_loop(&mut event_loop, &mut wayland_app)?;
    wayland_app.backend = None;
    Ok(())
}

fn run_event_loop(
    event_loop: &mut EventLoop<'static, TerminalWaylandApp>,
    wayland_app: &mut TerminalWaylandApp,
) -> Result<(), WaylandAppError> {
    while !wayland_app.exit {
        let timeout = if wayland_app.app.has_active_animation() {
            Duration::from_millis(8)
        } else {
            Duration::from_millis(120)
        };
        if let Err(err) = event_loop.dispatch(Some(timeout), wayland_app) {
            let msg = err.to_string();
            if wayland_app.conn.protocol_error().is_none()
                && (msg.contains("Broken pipe")
                    || msg.contains("other error during loop operation"))
            {
                tracing::info!("Wayland compositor closed connection; exiting rusty-terminal");
                break;
            }
            return Err(WaylandAppError::EventLoop(msg));
        }

        wayland_app.drain_paste_results();
        let _ = wayland_app.app.drain_pty_events();
        let _ = wayland_app.app.tick(Instant::now());
        wayland_app.process_pending_actions();

        if wayland_app.exit {
            break;
        }

        let frame_ready = !wayland_app.frame_pending
            || wayland_app.last_frame_sent.elapsed() >= Duration::from_millis(50);
        if wayland_app.configured && frame_ready && wayland_app.app.damage != DamageLevel::None {
            wayland_app.draw();
        }
        let _ = wayland_app.conn.flush();
    }
    Ok(())
}

impl TerminalWaylandApp {
    fn ensure_backend(&mut self) {
        if self.backend.is_some() {
            return;
        }
        let w = self.app.width;
        let h = self.app.height;
        if self.force_shm {
            tracing::info!("rusty-terminal running in --no-gpu wl_shm CPU renderer mode");
        } else {
            match GpuRenderer::new(
                self.conn.backend().display_ptr().cast(),
                self.window.wl_surface(),
                w,
                h,
            ) {
                Ok(gpu) => {
                    tracing::info!(
                        width = w,
                        height = h,
                        "rusty-terminal initialized Wayland-EGL GLES2 GPU renderer"
                    );
                    self.backend = Some(RenderBackend::Gpu(Box::new(gpu)));
                    return;
                }
                Err(err) => {
                    tracing::warn!(
                        "Wayland-EGL GPU init unavailable ({err}); falling back to wl_shm CPU renderer"
                    );
                }
            }
        }

        if let Ok(pool) = SlotPool::new((w * h * 4).max(4096) as usize, &self.shm) {
            self.backend = Some(RenderBackend::Shm {
                renderer: ShmFallbackRenderer::new(),
                pool,
            });
        }
    }

    fn sync_liquid_glass(&mut self) {
        let want_glass = !self.app.theme.reduce_transparency;
        if !want_glass {
            if let Some(glass) = self.surface_glass.take() {
                glass.destroy();
            }
            return;
        }
        let Some(mgr) = &self.liquid_glass_mgr else {
            return;
        };
        let glass = self
            .surface_glass
            .get_or_insert_with(|| mgr.get_surface_glass(self.window.wl_surface(), &self.qh, ()));
        glass.set_corner_radius(u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14));
        glass.set_style(GlassStyle::Sidebar);
        glass.commit();
    }

    fn sync_window_title(&mut self) {
        let title = self.app.active_tab().map_or_else(
            || String::from("Terminal"),
            super::state::TerminalTab::display_title,
        );
        if self.last_window_title != title {
            self.window.set_title(&title);
            self.last_window_title = title;
        }
    }

    fn draw(&mut self) {
        self.ensure_backend();
        self.sync_liquid_glass();
        let w = self.app.width;
        let h = self.app.height;
        let scale = self.buffer_scale.max(1);
        let surf = self.window.wl_surface().clone();
        surf.set_buffer_scale(scale);

        let fallback_needed = match self.backend.as_mut() {
            Some(RenderBackend::Gpu(gpu)) => {
                // Request frame callback BEFORE eglSwapBuffers so Mesa swaps & commits frame atomically.
                surf.frame(&self.qh, surf.clone());
                self.frame_pending = true;
                self.last_frame_sent = Instant::now();
                if let Err(err) = gpu.render(&self.app, scale) {
                    tracing::warn!("GPU frame error ({err}); switching to wl_shm fallback");
                    true
                } else {
                    self.app.damage = DamageLevel::None;
                    false
                }
            }
            Some(RenderBackend::Shm { renderer, pool }) => {
                let bw = w.saturating_mul(scale);
                let bh = h.saturating_mul(scale);
                let stride = bw * 4;
                if let Ok((buffer, canvas)) =
                    pool.create_buffer(bw, bh, stride, wl_shm::Format::Argb8888)
                {
                    if scale == 1 {
                        renderer.render(&self.app, canvas, w);
                    } else {
                        let len = (w.max(1) as usize) * (h.max(1) as usize) * 4;
                        if self.shm_scratch.len() != len {
                            self.shm_scratch.resize(len, 0);
                        }
                        renderer.render(&self.app, &mut self.shm_scratch, w);
                        blit_scale_bgra(
                            &self.shm_scratch,
                            w.max(1) as usize,
                            h.max(1) as usize,
                            canvas,
                            scale as usize,
                        );
                    }
                    if let Err(err) = buffer.attach_to(&surf) {
                        tracing::warn!("SHM attach error: {err}");
                    } else {
                        surf.damage_buffer(0, 0, bw, bh);
                        surf.frame(&self.qh, surf.clone());
                        surf.commit();
                        self.frame_pending = true;
                        self.last_frame_sent = Instant::now();
                        self.app.damage = DamageLevel::None;
                    }
                }
                false
            }
            None => false,
        };

        if fallback_needed {
            self.backend = None;
            self.force_shm = true;
            self.frame_pending = false;
            self.draw();
        } else {
            self.sync_window_title();
        }
    }

    fn drain_paste_results(&mut self) {
        while let Ok(text) = self.paste_rx.try_recv() {
            self.app.paste_text(&text);
        }
    }

    fn sync_cursor_shape(&mut self, px: f64, py: f64) {
        let shape = if let Some(edge) = self.resize_edge_at(px, py) {
            match edge {
                ResizeEdge::Left | ResizeEdge::Right => CursorShape::EwResize,
                ResizeEdge::Top | ResizeEdge::Bottom => CursorShape::NsResize,
                ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorShape::NwseResize,
                ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorShape::NeswResize,
                _ => CursorShape::Default,
            }
        } else if py >= f64::from(crate::layout::HEADER_HEIGHT) {
            CursorShape::Text
        } else if self.app.hover.is_some() {
            CursorShape::Pointer
        } else {
            CursorShape::Default
        };
        if self.last_cursor_shape != Some(shape)
            && let Some(dev) = &self.cursor_shape_device
        {
            dev.set_shape(self.pointer_enter_serial, shape);
            self.last_cursor_shape = Some(shape);
        }
    }

    fn process_pending_actions(&mut self) {
        let actions = std::mem::take(&mut self.app.pending_actions);
        for action in actions {
            match action {
                TerminalAction::CloseWindow => {
                    self.exit = true;
                }
                TerminalAction::MinimizeWindow => {
                    self.window.set_minimized();
                }
                TerminalAction::ToggleMaximize => {
                    if self.app.maximized {
                        self.window.unset_maximized();
                    } else {
                        self.window.set_maximized();
                    }
                }
                TerminalAction::StartInteractiveMove => {
                    if let Some(seat) = &self.active_seat {
                        self.window.move_(seat, self.last_input_serial);
                    }
                }
                TerminalAction::CopyClipboard(text) | TerminalAction::CopyPrimary(text) => {
                    if let (Some(ddm), Some(dd)) = (&self.data_device_manager, &self.data_device) {
                        for (old, _) in self.copy_sources.drain(..) {
                            old.inner().destroy();
                        }
                        let source =
                            ddm.create_copy_paste_source(&self.qh, CLIPBOARD_MIMES.iter().copied());
                        source.set_selection(dd, self.last_input_serial);
                        self.copy_sources.push((source, text));
                    }
                }
                TerminalAction::RequestPaste => self.request_clipboard_paste(),
                TerminalAction::UpdateGlassStyle => {
                    self.sync_liquid_glass();
                    self.app.mark_damage(DamageLevel::Full);
                }
            }
        }
    }

    fn request_clipboard_paste(&self) {
        let Some(dd) = &self.data_device else {
            self.paste_from_local_fallback();
            return;
        };
        let Some(offer) = dd.data().selection_offer() else {
            self.paste_from_local_fallback();
            return;
        };
        let mime = offer.with_mime_types(pick_clipboard_mime);
        let Some(mime) = mime else {
            tracing::debug!("Clipboard selection offer contained no plain-text MIME type");
            self.paste_from_local_fallback();
            return;
        };
        let Ok(mut read_pipe) = offer.receive(mime) else {
            tracing::debug!("Failed to create Wayland clipboard receive pipe");
            self.paste_from_local_fallback();
            return;
        };
        let _ = self.conn.flush();
        let tx = self.paste_tx.clone();
        let ping = self.paste_ping.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = (&mut read_pipe)
                .take(4 * 1024 * 1024)
                .read_to_end(&mut bytes);
            if let Ok(text) = String::from_utf8(bytes)
                && !text.is_empty()
            {
                let _ = tx.send(text);
                ping.ping();
            }
        });
    }

    fn paste_from_local_fallback(&self) {
        if let Some((_, text)) = self.copy_sources.last()
            && !text.is_empty()
        {
            let _ = self.paste_tx.send(text.clone());
            self.paste_ping.ping();
        }
    }

    fn resize_edge_at(&self, px: f64, py: f64) -> Option<ResizeEdge> {
        let w = f64::from(self.app.width);
        let h = f64::from(self.app.height);
        let left = px <= RESIZE_BORDER_PX;
        let right = px >= w - RESIZE_BORDER_PX;
        let top = py <= RESIZE_BORDER_PX;
        let bottom = py >= h - RESIZE_BORDER_PX;
        match (left, right, top, bottom) {
            (true, _, true, _) => Some(ResizeEdge::TopLeft),
            (_, true, true, _) => Some(ResizeEdge::TopRight),
            (true, _, _, true) => Some(ResizeEdge::BottomLeft),
            (_, true, _, true) => Some(ResizeEdge::BottomRight),
            (true, _, _, _) => Some(ResizeEdge::Left),
            (_, true, _, _) => Some(ResizeEdge::Right),
            (_, _, true, _) => Some(ResizeEdge::Top),
            (_, _, _, true) => Some(ResizeEdge::Bottom),
            _ => None,
        }
    }
}

impl CompositorHandler for TerminalWaylandApp {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let scale = new_factor.max(1);
        if self.window.wl_surface() == surface && self.buffer_scale != scale {
            self.buffer_scale = scale;
            surface.set_buffer_scale(scale);
            self.app.mark_damage(DamageLevel::Full);
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.frame_pending = false;
        if self.configured
            && (self.app.damage != DamageLevel::None || self.app.has_active_animation())
        {
            self.draw();
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for TerminalWaylandApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for TerminalWaylandApp {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let w = configure
            .new_size
            .0
            .map_or(self.app.width, |v| {
                i32::try_from(v.get()).unwrap_or(DEFAULT_WIDTH)
            })
            .max(MIN_WIDTH);
        let h = configure
            .new_size
            .1
            .map_or(self.app.height, |v| {
                i32::try_from(v.get()).unwrap_or(DEFAULT_HEIGHT)
            })
            .max(MIN_HEIGHT);
        self.configured = true;
        self.app.resize(w, h, configure.is_maximized());
        if !self.initial_pty_spawned {
            self.initial_pty_spawned = true;
            // Replace the placeholder tab with a real PTY sized to the initial configured dimensions
            // so the shell starts with exact cols/rows and zero startup scrollback lines.
            self.app.tabs.clear();
            self.app.spawn_tab(None);
            if let Some(title) = self.initial_title.take()
                && let Some(tab) = self.app.active_tab_mut()
            {
                tab.engine.title = title;
            }
        }
        self.app.mark_damage(DamageLevel::Full);
        if !self.frame_pending {
            self.draw();
        }
    }
}

impl TerminalWaylandApp {
    fn ensure_seat(&mut self, qh: &QueueHandle<Self>, seat: &wl_seat::WlSeat) {
        if self.active_seat.is_none() {
            self.active_seat = Some(seat.clone());
        }
        if self.data_device.is_none()
            && let Some(ddm) = &self.data_device_manager
        {
            self.data_device = Some(ddm.get_data_device(qh, seat));
        }
    }
}

impl SeatHandler for TerminalWaylandApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.ensure_seat(qh, &seat);
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        self.ensure_seat(qh, &seat);
        if capability == Capability::Keyboard
            && self.keyboard.is_none()
            && let Ok(kbd) = self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                Box::new(|state, _kbd, ev| {
                    state.app.handle_key_press(ev.keysym, ev.utf8.as_deref());
                }),
            )
        {
            self.keyboard = Some(kbd);
        }
        if capability == Capability::Pointer
            && self.pointer.is_none()
            && let Ok(ptr) = self.seat_state.get_pointer(qh, &seat)
        {
            self.cursor_shape_device = self
                .cursor_shape_manager
                .as_ref()
                .map(|m| m.get_shape_device(&ptr, qh));
            self.pointer = Some(ptr);
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
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for TerminalWaylandApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        serial: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if self.window.wl_surface() == surface {
            self.last_input_serial = serial;
            self.app.set_focus(true);
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.window.wl_surface() == surface {
            self.app.set_focus(false);
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.last_input_serial = serial;
        self.app
            .handle_key_press(event.keysym, event.utf8.as_deref());
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
        self.app.modifiers = modifiers;
    }
}

impl PointerHandler for TerminalWaylandApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            if self.window.wl_surface() != &ev.surface {
                continue;
            }
            let (px, py) = ev.position;
            match ev.kind {
                PointerEventKind::Enter { serial } => {
                    self.pointer_enter_serial = serial;
                    self.last_cursor_shape = None;
                    self.app.handle_pointer_motion(px, py);
                    self.sync_cursor_shape(px, py);
                }
                PointerEventKind::Motion { .. } => {
                    self.app.handle_pointer_motion(px, py);
                    self.sync_cursor_shape(px, py);
                }
                PointerEventKind::Leave { .. } => {
                    self.last_cursor_shape = None;
                    self.app.hover = None;
                }
                PointerEventKind::Press { button, serial, .. } => {
                    self.last_input_serial = serial;
                    if button == BTN_LEFT
                        && let (Some(edge), Some(seat)) =
                            (self.resize_edge_at(px, py), &self.active_seat)
                    {
                        self.window.resize(seat, serial, edge);
                        let _ = self.conn.flush();
                        continue;
                    }
                    let btn_idx = if button == BTN_LEFT {
                        0
                    } else if button == BTN_MIDDLE {
                        1
                    } else {
                        2
                    };
                    self.app.handle_pointer_button(px, py, btn_idx, true);
                    self.process_pending_actions();
                    let _ = self.conn.flush();
                }
                PointerEventKind::Release { button, .. } => {
                    let btn_idx = if button == BTN_LEFT {
                        0
                    } else if button == BTN_MIDDLE {
                        1
                    } else {
                        2
                    };
                    self.app.handle_pointer_button(px, py, btn_idx, false);
                    self.process_pending_actions();
                }
                PointerEventKind::Axis {
                    vertical:
                        AxisScroll {
                            absolute, discrete, ..
                        },
                    ..
                } => {
                    let steps = if discrete != 0 {
                        f64::from(discrete)
                    } else {
                        absolute / 18.0
                    };
                    if steps.abs() > f64::EPSILON {
                        self.app.handle_scroll_wheel(steps);
                    }
                }
            }
        }
    }
}

impl ShmHandler for TerminalWaylandApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl DataDeviceHandler for TerminalWaylandApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
    ) {
    }
    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
    fn drop_performed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
}

impl DataOfferHandler for TerminalWaylandApp {
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

impl DataSourceHandler for TerminalWaylandApp {
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
        write_pipe: WritePipe,
    ) {
        if !CLIPBOARD_MIMES.contains(&mime.as_str()) {
            return;
        }
        if let Some((_, text)) = self.copy_sources.iter().find(|(s, _)| s.inner() == source) {
            let payload = text.as_bytes().to_vec();
            let fd = OwnedFd::from(write_pipe);
            let _ = std::thread::Builder::new()
                .name(String::from("rusty-term-clip-send"))
                .spawn(move || {
                    let mut file = File::from(fd);
                    let _ = file.write_all(&payload);
                });
        }
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
    ) {
        self.copy_sources.retain(|(s, _)| s.inner() != source);
        source.destroy();
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
        _: &wl_data_source::WlDataSource,
    ) {
    }
    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: DndAction,
    ) {
    }
}

impl Dispatch<RustyosLiquidGlassManagerV1, ()> for TerminalWaylandApp {
    fn event(
        _: &mut Self,
        _: &RustyosLiquidGlassManagerV1,
        _: <RustyosLiquidGlassManagerV1 as wayland_client::Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RustyosSurfaceGlassV1, ()> for TerminalWaylandApp {
    fn event(
        _: &mut Self,
        _: &RustyosSurfaceGlassV1,
        _: <RustyosSurfaceGlassV1 as wayland_client::Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl ProvidesRegistryState for TerminalWaylandApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(TerminalWaylandApp);
delegate_output!(TerminalWaylandApp);
delegate_shm!(TerminalWaylandApp);
delegate_seat!(TerminalWaylandApp);
delegate_keyboard!(TerminalWaylandApp);
delegate_pointer!(TerminalWaylandApp);
delegate_xdg_shell!(TerminalWaylandApp);
delegate_xdg_window!(TerminalWaylandApp);
delegate_data_device!(TerminalWaylandApp);
delegate_registry!(TerminalWaylandApp);

fn pick_clipboard_mime(mimes: &[String]) -> Option<String> {
    for candidate in CLIPBOARD_MIMES {
        if mimes.iter().any(|m| m == candidate) {
            return Some((*candidate).to_owned());
        }
    }
    mimes
        .iter()
        .find(|m| m.starts_with("text/plain"))
        .cloned()
        .or_else(|| {
            mimes
                .iter()
                .find(|m| {
                    m.starts_with("text/")
                        && !m.starts_with("text/html")
                        && !m.starts_with("text/rtf")
                })
                .cloned()
        })
}

fn blit_scale_bgra(src: &[u8], src_w: usize, src_h: usize, dst: &mut [u8], scale: usize) {
    let s = scale.max(1);
    let dw = src_w * s;
    for y in 0..src_h {
        let src_row = &src[y * src_w * 4..(y + 1) * src_w * 4];
        for sy in 0..s {
            let dst_row_off = (y * s + sy) * dw * 4;
            for x in 0..src_w {
                let px = &src_row[x * 4..x * 4 + 4];
                let dst_x = dst_row_off + x * s * 4;
                for sx in 0..s {
                    dst[dst_x + sx * 4..dst_x + sx * 4 + 4].copy_from_slice(px);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pick_clipboard_mime;

    #[test]
    fn picks_utf8_plain_text_over_html_and_supports_spaced_charset() {
        let mimes = vec![
            String::from("text/html"),
            String::from("text/plain; charset=utf-8"),
        ];
        assert_eq!(
            pick_clipboard_mime(&mimes).as_deref(),
            Some("text/plain; charset=utf-8")
        );

        let html_only = vec![String::from("text/html"), String::from("image/png")];
        assert_eq!(pick_clipboard_mime(&html_only), None);
    }
}
