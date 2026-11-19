//! Wayland CSD (`xdg_toplevel`) event loop, Liquid Glass binding, and input dispatch
//! for `rusty-installer` (`RUSTYOS-115`, ADR-0065).

use std::fmt;
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle, ping},
        calloop_wayland_source::WaylandSource,
        csd_frame::WindowState,
        protocols::xdg::shell::client::xdg_toplevel::ResizeEdge,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell, XdgSurface,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    backend::WaylandError,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::disks::TargetDiskInfo;
use crate::layout::{
    InstallerHit, InstallerLayout, MIN_HEIGHT, MIN_WIDTH, ResizeEdgeZone, WINDOW_CORNER_RADIUS,
    WelcomeAction, compute_layout, hit_test,
};
use crate::liquid_glass::{GlassStyle, RustyosLiquidGlassManagerV1, RustyosSurfaceGlassV1};
use crate::render::render_window_pixels;
use crate::state::{InstallPhase, InstallerAppState, StartupOptions, WizardMode};

const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 580;

#[derive(Debug)]
pub(crate) enum WaylandAppError {
    Connect(wayland_client::ConnectError),
    Global(wayland_client::globals::GlobalError),
    Bind(wayland_client::globals::BindError),
    Dispatch(wayland_client::DispatchError),
    Backend(WaylandError),
    ShmPool(String),
    EventLoop(String),
}

impl fmt::Display for WaylandAppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "Wayland connect error: {e}"),
            Self::Global(e) => write!(f, "Wayland global error: {e}"),
            Self::Bind(e) => write!(f, "Wayland bind error: {e}"),
            Self::Dispatch(e) => write!(f, "Wayland dispatch error: {e}"),
            Self::Backend(e) => write!(f, "Wayland protocol error: {e}"),
            Self::ShmPool(e) => write!(f, "SHM pool error: {e}"),
            Self::EventLoop(e) => write!(f, "Event loop error: {e}"),
        }
    }
}

impl std::error::Error for WaylandAppError {}

impl From<wayland_client::ConnectError> for WaylandAppError {
    fn from(v: wayland_client::ConnectError) -> Self {
        Self::Connect(v)
    }
}
impl From<wayland_client::globals::GlobalError> for WaylandAppError {
    fn from(v: wayland_client::globals::GlobalError) -> Self {
        Self::Global(v)
    }
}
impl From<wayland_client::globals::BindError> for WaylandAppError {
    fn from(v: wayland_client::globals::BindError) -> Self {
        Self::Bind(v)
    }
}
impl From<wayland_client::DispatchError> for WaylandAppError {
    fn from(v: wayland_client::DispatchError) -> Self {
        Self::Dispatch(v)
    }
}
impl From<WaylandError> for WaylandAppError {
    fn from(v: WaylandError) -> Self {
        Self::Backend(v)
    }
}

struct InstallerClient {
    qh: QueueHandle<Self>,
    loop_handle: LoopHandle<'static, Self>,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    window: Option<Window>,
    _glass_mgr: Option<RustyosLiquidGlassManagerV1>,
    _window_glass: Option<RustyosSurfaceGlassV1>,
    has_glass: bool,
    buffers: Vec<Buffer>,
    buffer_size: (u32, u32),
    frame_sent_at: Option<Instant>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,
    width: u32,
    height: u32,
    configured: bool,
    pointer_pos: (f64, f64),
    last_click_at: Option<Instant>,
    hover_hit: Option<InstallerHit>,
    app: InstallerAppState,
    layout_cache: Option<InstallerLayout>,
}

impl InstallerClient {
    fn layout(&mut self) -> InstallerLayout {
        let l = compute_layout(&self.app, self.width, self.height);
        self.layout_cache = Some(l.clone());
        l
    }

    fn current_layout(&mut self) -> InstallerLayout {
        if let Some(cached) = &self.layout_cache {
            return cached.clone();
        }
        self.layout()
    }

    fn handle_key(&mut self, keysym: Keysym, utf8: Option<&str>) {
        if self.modifiers.ctrl && keysym == Keysym::q {
            self.app.win.running = false;
            return;
        }
        match keysym {
            Keysym::Escape if self.app.caret.focused_field.is_some() => {
                self.app.caret.focused_field = None;
                self.app.win.dirty = true;
            }
            Keysym::Tab => self.app.cycle_focus(self.modifiers.shift),
            Keysym::ISO_Left_Tab => self.app.cycle_focus(true),
            Keysym::BackSpace => self.app.handle_backspace(),
            Keysym::Return | Keysym::KP_Enter => {
                self.app.go_next(Instant::now());
                self.layout_cache = None;
            }
            _ => {
                if !self.modifiers.ctrl
                    && !self.modifiers.alt
                    && let Some(text) = utf8
                {
                    self.app.handle_text_input(text);
                }
            }
        }
    }

    fn execute_left_click(&mut self, px: f64, py: f64, serial: u32) {
        let now = Instant::now();
        let double_click = self
            .last_click_at
            .is_some_and(|t| now.saturating_duration_since(t) <= Duration::from_millis(380));
        self.last_click_at = Some(now);

        let layout = self.current_layout();
        let Some(hit) = hit_test(&layout, px, py) else {
            if self.app.caret.focused_field.take().is_some() {
                self.app.win.dirty = true;
            }
            return;
        };

        if !matches!(hit, InstallerHit::FocusField(_)) && self.app.caret.focused_field.is_some() {
            self.app.caret.focused_field = None;
            self.app.win.dirty = true;
        }
        self.dispatch_hit(hit, serial, double_click, now);
    }

    fn dispatch_hit(&mut self, hit: InstallerHit, serial: u32, double_click: bool, now: Instant) {
        match hit {
            InstallerHit::ResizeEdge(zone) => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.resize(seat, serial, map_resize_edge(zone));
                }
            }
            InstallerHit::WindowDrag => {
                if double_click {
                    self.toggle_maximized();
                } else if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.move_(seat, serial);
                }
            }
            InstallerHit::TrafficClose => self.app.win.running = false,
            InstallerHit::TrafficMinimize => {
                if let Some(win) = &self.window {
                    win.set_minimized();
                }
            }
            InstallerHit::TrafficMaximize => self.toggle_maximized(),
            InstallerHit::BackButton => {
                self.app.go_back();
                self.layout_cache = None;
            }
            InstallerHit::PrimaryButton => {
                self.app.go_next(now);
                self.layout_cache = None;
            }
            InstallerHit::WelcomeCard(action) => match action {
                WelcomeAction::InstallToDisk => {
                    self.app.go_next(now);
                    self.layout_cache = None;
                }
                WelcomeAction::RunDoctor => self.app.spawn_doctor_check(),
                WelcomeAction::OpenTerminal => self.app.launch_terminal(),
            },
            InstallerHit::SelectDisk(idx) => {
                if self
                    .app
                    .disks
                    .get(idx)
                    .is_some_and(TargetDiskInfo::is_selectable)
                {
                    self.app.selected_disk_idx = Some(idx);
                    self.app.form_error = None;
                    self.app.win.dirty = true;
                    self.layout_cache = None;
                }
            }
            InstallerHit::RefreshDisks => self.app.spawn_disk_scan(),
            InstallerHit::SetBootloader(kind) => {
                self.app.bootloader = kind;
                self.app.win.dirty = true;
            }
            InstallerHit::SelectLanguage(idx) => {
                if idx < crate::state::LANGUAGE_OPTIONS.len() {
                    self.app.language_idx = idx;
                    self.app.win.dirty = true;
                    self.layout_cache = None;
                }
            }
            InstallerHit::SelectKeyboard(idx) => {
                if idx < crate::state::KEYBOARD_PRESETS.len() {
                    self.app.keyboard_idx = idx;
                    self.app.win.dirty = true;
                    self.layout_cache = None;
                }
            }
            InstallerHit::RefreshNetwork => self.app.spawn_network_probe(),
            InstallerHit::FocusField(field) => {
                self.app.caret.focused_field = Some(field);
                self.app.caret.visible = true;
                self.app.caret.last_blink = now;
                self.app.win.dirty = true;
            }
            InstallerHit::ToggleAutologin => {
                self.app.account.autologin = !self.app.account.autologin;
                self.app.win.dirty = true;
            }
            InstallerHit::SelectTheme(choice) => {
                let persist = self.app.mode == WizardMode::Oobe;
                self.app.choose_theme(choice, persist);
                self.layout_cache = None;
            }
            InstallerHit::SelectAccent(idx) => {
                let persist = self.app.mode == WizardMode::Oobe;
                self.app.choose_accent(idx, persist);
                self.layout_cache = None;
            }
            InstallerHit::ToggleLogs => {
                self.app.show_logs = !self.app.show_logs;
                self.app.win.dirty = true;
                self.layout_cache = None;
            }
        }
    }

    fn toggle_maximized(&self) {
        if let Some(win) = &self.window {
            if self.app.win.maximized {
                win.unset_maximized();
            } else {
                win.set_maximized();
            }
        }
    }
}

pub(crate) fn run(opts: &StartupOptions) -> Result<(), WaylandAppError> {
    let (conn, mut client, mut event_loop) = setup_client(opts)?;
    while client.app.win.running {
        let now = Instant::now();
        if client.app.poll_workers(now) {
            client.layout_cache = None;
        }
        let _ = client.app.poll_system_theme(now);
        let _ = client.app.update_caret_blink(now);
        if let Some(sent_at) = client.frame_sent_at
            && now.saturating_duration_since(sent_at) >= Duration::from_millis(120)
        {
            client.frame_sent_at = None;
        }
        if client.configured && client.frame_sent_at.is_none() && client.app.win.dirty {
            draw_frame(&mut client)?;
            client.app.win.dirty = false;
        }
        conn.flush()?;

        let timeout =
            if client.app.progress_anim.is_active() || client.app.phase == InstallPhase::Running {
                Some(Duration::from_millis(16))
            } else if client.app.caret.focused_field.is_some() {
                Some(Duration::from_millis(250))
            } else {
                Some(Duration::from_millis(450))
            };
        event_loop
            .dispatch(timeout, &mut client)
            .map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    }
    Ok(())
}

fn setup_client(
    opts: &StartupOptions,
) -> Result<
    (
        Connection,
        InstallerClient,
        EventLoop<'static, InstallerClient>,
    ),
    WaylandAppError,
> {
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let compositor_state = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let pool = SlotPool::new((DEFAULT_WIDTH * DEFAULT_HEIGHT * 4 * 3) as usize, &shm)
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;

    let surface = compositor_state.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestClient, &qh);
    window.set_title(match opts.mode {
        WizardMode::Install => "Install RustyOS",
        WizardMode::Oobe => "RustyOS Setup Assistant",
    });
    window.set_app_id("org.rustyos.Installer");
    window.set_min_size(Some((
        u32::try_from(MIN_WIDTH).unwrap_or(680),
        u32::try_from(MIN_HEIGHT).unwrap_or(500),
    )));

    let glass_mgr: Option<RustyosLiquidGlassManagerV1> = if opts.no_glass {
        None
    } else {
        globals.bind(&qh, 1..=1, ()).ok()
    };
    let window_glass = glass_mgr.as_ref().map(|mgr| {
        let g = mgr.get_surface_glass(window.wl_surface(), &qh, ());
        g.set_corner_radius(u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(18));
        g.set_style(GlassStyle::Sidebar);
        g.commit();
        g
    });
    let has_glass = window_glass.is_some();
    window.commit();

    let (wake_ping, wake_source) =
        ping::make_ping().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let mut app = InstallerAppState::new(opts);
    app.set_wake_ping(move || wake_ping.ping());
    app.kick_initial_probes();

    let event_loop: EventLoop<'static, InstallerClient> =
        EventLoop::try_new().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let loop_handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue)
        .insert(loop_handle.clone())
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;
    loop_handle
        .insert_source(wake_source, |(), (), client| {
            if client.app.poll_workers(Instant::now()) {
                client.layout_cache = None;
            }
        })
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;

    let client = InstallerClient {
        qh: qh.clone(),
        loop_handle: loop_handle.clone(),
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        window: Some(window),
        _glass_mgr: glass_mgr,
        _window_glass: window_glass,
        has_glass,
        buffers: Vec::new(),
        buffer_size: (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        frame_sent_at: None,
        seat: None,
        pointer: None,
        keyboard: None,
        modifiers: Modifiers::default(),
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        configured: false,
        pointer_pos: (0.0, 0.0),
        last_click_at: None,
        hover_hit: None,
        app,
        layout_cache: None,
    };
    Ok((conn, client, event_loop))
}

#[allow(clippy::cast_possible_wrap)]
fn draw_frame(client: &mut InstallerClient) -> Result<(), WaylandAppError> {
    let layout = client.current_layout();
    let Some(win) = &client.window else {
        return Ok(());
    };
    let w = client.width.max(1);
    let h = client.height.max(1);
    if client.buffer_size != (w, h) {
        client.buffers.clear();
        client.buffer_size = (w, h);
    }
    let stride = i32::try_from(w * 4).unwrap_or(i32::MAX);
    let wi = i32::try_from(w).unwrap_or(1);
    let hi = i32::try_from(h).unwrap_or(1);
    let idx =
        if let Some(pos) = client.buffers.iter().position(|b| {
            !b.slot().has_active_buffers() && b.height() == hi && b.stride() == stride
        }) {
            pos
        } else {
            let (buf, _) = client
                .pool
                .create_buffer(wi, hi, stride, wl_shm::Format::Argb8888)
                .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
            client.buffers.push(buf);
            client.buffers.len() - 1
        };

    let buf_ref = &client.buffers[idx];
    let canvas = buf_ref
        .canvas(&mut client.pool)
        .ok_or_else(|| WaylandAppError::ShmPool(String::from("SHM slot busy")))?;

    render_window_pixels(
        canvas,
        w,
        h,
        &client.app,
        &layout,
        client.hover_hit,
        client.has_glass,
    );
    win.set_window_geometry(0, 0, w, h);
    buf_ref
        .attach_to(win.wl_surface())
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
    win.wl_surface().damage_buffer(0, 0, wi, hi);
    win.wl_surface().frame(&client.qh, win.wl_surface().clone());
    win.commit();
    client.frame_sent_at = Some(Instant::now());
    Ok(())
}

const fn map_resize_edge(zone: ResizeEdgeZone) -> ResizeEdge {
    match zone {
        ResizeEdgeZone::Top => ResizeEdge::Top,
        ResizeEdgeZone::Bottom => ResizeEdge::Bottom,
        ResizeEdgeZone::Left => ResizeEdge::Left,
        ResizeEdgeZone::Right => ResizeEdge::Right,
        ResizeEdgeZone::TopLeft => ResizeEdge::TopLeft,
        ResizeEdgeZone::TopRight => ResizeEdge::TopRight,
        ResizeEdgeZone::BottomLeft => ResizeEdge::BottomLeft,
        ResizeEdgeZone::BottomRight => ResizeEdge::BottomRight,
    }
}

impl ProvidesRegistryState for InstallerClient {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(InstallerClient);
delegate_output!(InstallerClient);
delegate_compositor!(InstallerClient);
delegate_shm!(InstallerClient);
delegate_seat!(InstallerClient);
delegate_pointer!(InstallerClient);
delegate_keyboard!(InstallerClient);
delegate_xdg_shell!(InstallerClient);
delegate_xdg_window!(InstallerClient);

impl Dispatch<RustyosLiquidGlassManagerV1, ()> for InstallerClient {
    fn event(
        _: &mut Self,
        _: &RustyosLiquidGlassManagerV1,
        _: crate::liquid_glass::protocol::rustyos_liquid_glass_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RustyosSurfaceGlassV1, ()> for InstallerClient {
    fn event(
        _: &mut Self,
        _: &RustyosSurfaceGlassV1,
        _: crate::liquid_glass::protocol::rustyos_surface_glass_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl OutputHandler for InstallerClient {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl CompositorHandler for InstallerClient {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self
            .window
            .as_ref()
            .is_some_and(|w| w.wl_surface() == surface)
        {
            self.frame_sent_at = None;
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

impl ShmHandler for InstallerClient {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl WindowHandler for InstallerClient {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.app.win.running = false;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let next_w = configure
            .new_size
            .0
            .map_or(DEFAULT_WIDTH, std::num::NonZeroU32::get);
        let next_h = configure
            .new_size
            .1
            .map_or(DEFAULT_HEIGHT, std::num::NonZeroU32::get);
        let clamped_w = next_w.max(u32::try_from(MIN_WIDTH).unwrap_or(680));
        let clamped_h = next_h.max(u32::try_from(MIN_HEIGHT).unwrap_or(500));
        if (clamped_w, clamped_h) != (self.width, self.height) {
            self.width = clamped_w;
            self.height = clamped_h;
            self.layout_cache = None;
        }
        self.app.win.maximized = configure.state.contains(WindowState::MAXIMIZED);
        self.configured = true;
        self.app.win.dirty = true;
    }
}

impl SeatHandler for InstallerClient {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if self.seat.is_none() {
            self.seat = Some(seat);
        }
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            if self.seat.is_none() {
                self.seat = Some(seat.clone());
            }
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        } else if capability == Capability::Keyboard && self.keyboard.is_none() {
            if self.seat.is_none() {
                self.seat = Some(seat.clone());
            }
            if let Ok(kbd) = self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                Box::new(|state, _kbd, ev| {
                    state.handle_key(ev.keysym, ev.utf8.as_deref());
                }),
            ) {
                self.keyboard = Some(kbd);
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        } else if capability == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for InstallerClient {
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
        self.modifiers = Modifiers::default();
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.handle_key(event.keysym, event.utf8.as_deref());
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

impl PointerHandler for InstallerClient {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            self.pointer_pos = ev.position;
            match ev.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let layout = self.current_layout();
                    let next = hit_test(&layout, ev.position.0, ev.position.1);
                    if next != self.hover_hit {
                        self.hover_hit = next;
                        self.app.win.dirty = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.hover_hit.take().is_some() {
                        self.app.win.dirty = true;
                    }
                }
                PointerEventKind::Press {
                    button: BTN_LEFT,
                    serial,
                    ..
                } => {
                    self.execute_left_click(ev.position.0, ev.position.1, serial);
                }
                _ => {}
            }
        }
    }
}
