//! Wayland CSD (`xdg_toplevel`) event loop and input dispatch for `rusty-settings` (`RUSTYOS-103`).

use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, ping},
        calloop_wayland_source::WaylandSource,
        protocols::xdg::shell::client::xdg_toplevel::ResizeEdge,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{AxisScroll, BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler},
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
    Connection, Dispatch, QueueHandle, WEnum,
    backend::WaylandError,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::layout::{
    ResizeEdgeZone, SIDEBAR_WIDTH, SettingsHit, SettingsLayout, compute_layout, hit_test,
};
use crate::render::render_window_pixels;
use crate::state::{
    DamageScope, ServiceCommand, ServiceSnapshot, SettingsAppState, StartupOptions,
    execute_coalesced_service_batch,
};

const DEFAULT_WIDTH: u32 = 882;
const DEFAULT_HEIGHT: u32 = 600;

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

#[allow(clippy::struct_excessive_bools)]
struct SettingsClient {
    qh: QueueHandle<Self>,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    window: Option<Window>,
    buffers: Vec<Buffer>,
    buffer_size: (u32, u32),
    frame_sent_at: Option<Instant>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mod_ctrl: bool,
    mod_shift: bool,
    width: u32,
    height: u32,
    configured: bool,
    pointer_inside: bool,
    pointer_pos: (f64, f64),
    hover_hit: Option<SettingsHit>,
    last_click_at: Option<Instant>,
    app: SettingsAppState,
    layout_cache: Option<SettingsLayout>,
    service_tx: Sender<ServiceCommand>,
    service_rx: Receiver<ServiceSnapshot>,
}

impl SettingsClient {
    fn layout(&mut self) -> SettingsLayout {
        let l = compute_layout(&self.app, self.width, self.height);
        self.layout_cache = Some(l.clone());
        l
    }

    fn current_layout(&mut self) -> SettingsLayout {
        if let Some(l) = &self.layout_cache {
            return l.clone();
        }
        self.layout()
    }

    fn flush_pending_service_commands(&mut self) {
        for cmd in self.app.take_pending_service_commands() {
            let _ = self.service_tx.send(cmd);
        }
    }

    fn execute_left_click(&mut self, px: f64, py: f64, serial: u32) {
        let now = Instant::now();
        let double_click = self
            .last_click_at
            .is_some_and(|t| now.saturating_duration_since(t) <= Duration::from_millis(380));
        self.last_click_at = Some(now);

        let layout = self.current_layout();
        let hit = hit_test(&layout, px, py);
        if !matches!(hit, Some(SettingsHit::SearchField)) && self.app.search_focused {
            self.app.search_focused = false;
            self.app.mark_dirty(DamageScope::Sidebar);
        }
        let Some(hit) = hit else {
            return;
        };
        match hit {
            SettingsHit::ResizeEdge(zone) => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.resize(seat, serial, map_resize_edge(zone));
                }
            }
            SettingsHit::WindowDrag => {
                if double_click {
                    if let Some(win) = &self.window {
                        if self.app.maximized {
                            win.unset_maximized();
                        } else {
                            win.set_maximized();
                        }
                    }
                } else if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.move_(seat, serial);
                }
            }
            SettingsHit::TrafficClose => self.app.request_quit(),
            SettingsHit::TrafficMinimize => {
                if let Some(win) = &self.window {
                    win.set_minimized();
                }
            }
            SettingsHit::TrafficMaximize => {
                if let Some(win) = &self.window {
                    if self.app.maximized {
                        win.unset_maximized();
                    } else {
                        win.set_maximized();
                    }
                }
            }
            SettingsHit::NavBack => {
                self.app.go_back();
                self.layout_cache = None;
            }
            SettingsHit::NavForward => {
                self.app.go_forward();
                self.layout_cache = None;
            }
            SettingsHit::SearchField => {
                self.app.search_focused = true;
                self.app.caret_visible = true;
                self.app.mark_dirty(DamageScope::Sidebar);
            }
            SettingsHit::SidebarItem(page) | SettingsHit::PushPage(page) => {
                self.app.navigate_to(page);
                self.layout_cache = None;
            }
            SettingsHit::SelectTheme(choice) => {
                self.app.choose_appearance(choice);
                self.layout_cache = None;
            }
            SettingsHit::SelectAccent(idx) => {
                self.app.choose_accent(idx);
                self.layout_cache = None;
            }
            SettingsHit::DockPosTile(idx) => {
                self.app
                    .apply_row_action(crate::state::RowControlId::DockPositionSet(idx));
                self.layout_cache = None;
            }
            SettingsHit::Control(id)
            | SettingsHit::StepperMinus(id)
            | SettingsHit::StepperPlus(id) => {
                self.app.apply_row_action(id);
                self.layout_cache = None;
            }
        }
        self.flush_pending_service_commands();
    }
}

fn spawn_service_worker(wake: ping::Ping) -> (Sender<ServiceCommand>, Receiver<ServiceSnapshot>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (snap_tx, snap_rx) = mpsc::channel();
    let _ = thread::Builder::new()
        .name(String::from("settings-services"))
        .spawn(move || {
            while let Ok(first) = cmd_rx.recv() {
                let mut batch = vec![first];
                while let Ok(next) = cmd_rx.try_recv() {
                    batch.push(next);
                }
                let snap = execute_coalesced_service_batch(batch);
                if snap_tx.send(snap).is_err() {
                    break;
                }
                wake.ping();
            }
        });
    (cmd_tx, snap_rx)
}

pub(crate) fn run(opts: &StartupOptions) -> Result<(), WaylandAppError> {
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
    window.set_title("Settings");
    window.set_app_id("org.rustyos.Settings");
    window.set_min_size(Some((640, 500)));
    window.commit();

    let (wake_ping, wake_source) =
        ping::make_ping().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let (service_tx, service_rx) = spawn_service_worker(wake_ping);

    let mut client = SettingsClient {
        qh,
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &queue.handle()),
        seat_state: SeatState::new(&globals, &queue.handle()),
        shm,
        pool,
        window: Some(window),
        buffers: Vec::new(),
        buffer_size: (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        frame_sent_at: None,
        seat: None,
        pointer: None,
        keyboard: None,
        mod_ctrl: false,
        mod_shift: false,
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        configured: false,
        pointer_inside: false,
        pointer_pos: (0.0, 0.0),
        hover_hit: None,
        last_click_at: None,
        app: SettingsAppState::new(opts),
        layout_cache: None,
        service_tx,
        service_rx,
    };
    client.flush_pending_service_commands();

    let mut event_loop: EventLoop<'static, SettingsClient> =
        EventLoop::try_new().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;
    event_loop
        .handle()
        .insert_source(wake_source, |(), (), client| {
            while let Ok(snap) = client.service_rx.try_recv() {
                if client.app.apply_service_snapshot(snap) {
                    client.layout_cache = None;
                }
            }
        })
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;

    while client.app.running {
        let now = Instant::now();
        let _ = client.app.update_caret_blink(now);
        if client.app.poll_external_config_if_due(now) {
            client.layout_cache = None;
        }
        if client.app.poll_updates() {
            client.layout_cache = None;
        }
        if let Some(sent_at) = client.frame_sent_at
            && now.saturating_duration_since(sent_at) >= Duration::from_millis(120)
        {
            client.frame_sent_at = None;
        }
        if client.app.step_scroll_animation(now) {
            client.layout_cache = None;
        }
        if client.configured && client.frame_sent_at.is_none() && client.app.dirty {
            draw_frame(&mut client)?;
        }
        conn.flush()?;

        let timeout = if client.app.is_scroll_animating() {
            Some(Duration::from_millis(8))
        } else if client.app.updates_checking || client.app.updates_tx_phase.is_some() {
            Some(Duration::from_millis(50))
        } else if client.app.search_focused {
            Some(Duration::from_millis(250))
        } else {
            Some(Duration::from_millis(400))
        };
        event_loop
            .dispatch(timeout, &mut client)
            .map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    }
    Ok(())
}

fn acquire_reusable_buffer(
    pool: &mut SlotPool,
    buffers: &mut Vec<Buffer>,
    w: u32,
    h: u32,
) -> Result<usize, WaylandAppError> {
    let stride = i32::try_from(w * 4).unwrap_or(i32::MAX);
    let wi = i32::try_from(w).unwrap_or(1);
    let hi = i32::try_from(h).unwrap_or(1);
    if let Some(idx) = buffers
        .iter()
        .position(|b| !b.slot().has_active_buffers() && b.height() == hi && b.stride() == stride)
    {
        return Ok(idx);
    }
    let (buffer, _) = pool
        .create_buffer(wi, hi, stride, wl_shm::Format::Argb8888)
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
    buffers.push(buffer);
    Ok(buffers.len() - 1)
}

fn damage_rect_for_scope(
    scope: DamageScope,
    layout: &SettingsLayout,
    w: u32,
    h: u32,
) -> (i32, i32, i32, i32) {
    let wi = i32::try_from(w).unwrap_or(1).max(1);
    let hi = i32::try_from(h).unwrap_or(1).max(1);
    match scope {
        DamageScope::Caret => {
            let x0 = (layout.search_rect.x - 1).clamp(0, wi);
            let y0 = (layout.search_rect.y - 1).clamp(0, hi);
            let x1 = (layout.search_rect.right() + 1).clamp(x0, wi);
            let y1 = (layout.search_rect.bottom() + 1).clamp(y0, hi);
            if x1 > x0 && y1 > y0 {
                (x0, y0, x1 - x0, y1 - y0)
            } else {
                (0, 0, wi, hi)
            }
        }
        DamageScope::Sidebar => (0, 0, (SIDEBAR_WIDTH + 2).min(wi), hi),
        DamageScope::Page => {
            let px = SIDEBAR_WIDTH.min(wi.saturating_sub(1));
            (px, 0, (wi - px).max(1), hi)
        }
        DamageScope::Full | DamageScope::None => (0, 0, wi, hi),
    }
}

#[allow(clippy::cast_possible_wrap)]
fn draw_frame(client: &mut SettingsClient) -> Result<(), WaylandAppError> {
    let mut layout = client.current_layout();
    let prev_page_scroll = client.app.page_scroll_y;
    let prev_side_scroll = client.app.sidebar_scroll_y;
    client
        .app
        .clamp_scroll_bounds(layout.max_sidebar_scroll, layout.max_page_scroll);
    if client.app.page_scroll_y != prev_page_scroll
        || client.app.sidebar_scroll_y != prev_side_scroll
    {
        layout = client.layout();
    }
    if client.pointer_inside {
        client.hover_hit = hit_test(&layout, client.pointer_pos.0, client.pointer_pos.1);
    }

    let Some(win) = &client.window else {
        return Ok(());
    };
    let w = client.width.max(1);
    let h = client.height.max(1);
    if client.buffer_size != (w, h) {
        client.buffers.clear();
        client.buffer_size = (w, h);
    }
    let idx = acquire_reusable_buffer(&mut client.pool, &mut client.buffers, w, h)?;
    let buf_ref = &client.buffers[idx];
    let canvas = buf_ref
        .canvas(&mut client.pool)
        .ok_or_else(|| WaylandAppError::ShmPool(String::from("Settings SHM slot busy")))?;

    render_window_pixels(
        canvas,
        w,
        h,
        &client.app,
        &layout,
        client.hover_hit.as_ref(),
    );
    let damage_scope = client.app.take_damage_scope();
    let (dx, dy, dw, dh) = damage_rect_for_scope(damage_scope, &layout, w, h);

    win.set_window_geometry(0, 0, w, h);
    buf_ref
        .attach_to(win.wl_surface())
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
    win.wl_surface().damage_buffer(dx, dy, dw, dh);
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

impl ProvidesRegistryState for SettingsClient {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(SettingsClient);
delegate_output!(SettingsClient);
delegate_compositor!(SettingsClient);
delegate_shm!(SettingsClient);
delegate_seat!(SettingsClient);
delegate_pointer!(SettingsClient);
delegate_xdg_shell!(SettingsClient);
delegate_xdg_window!(SettingsClient);

impl OutputHandler for SettingsClient {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl CompositorHandler for SettingsClient {
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

impl ShmHandler for SettingsClient {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl WindowHandler for SettingsClient {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.app.request_quit();
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
            .map_or(DEFAULT_WIDTH, std::num::NonZeroU32::get)
            .max(640);
        let next_h = configure
            .new_size
            .1
            .map_or(DEFAULT_HEIGHT, std::num::NonZeroU32::get)
            .max(500);
        let max_changed = self.app.maximized != configure.is_maximized();
        if next_w != self.width || next_h != self.height || !self.configured || max_changed {
            self.width = next_w;
            self.height = next_h;
            self.layout_cache = None;
            self.app.maximized = configure.is_maximized();
            self.configured = true;
            self.app.mark_dirty(DamageScope::Full);
        }
    }
}

impl SeatHandler for SettingsClient {
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
            if let Ok(ptr) = self.seat_state.get_pointer(qh, &seat) {
                self.pointer = Some(ptr);
            }
        } else if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(seat.get_keyboard(qh, ()));
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            self.pointer = None;
        } else if capability == Capability::Keyboard {
            self.keyboard = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

const fn hit_damage_scope(hit: Option<&SettingsHit>) -> DamageScope {
    match hit {
        None | Some(SettingsHit::ResizeEdge(_) | SettingsHit::WindowDrag) => DamageScope::None,
        Some(
            SettingsHit::TrafficClose
            | SettingsHit::TrafficMinimize
            | SettingsHit::TrafficMaximize
            | SettingsHit::SearchField
            | SettingsHit::SidebarItem(_),
        ) => DamageScope::Sidebar,
        Some(
            SettingsHit::NavBack
            | SettingsHit::NavForward
            | SettingsHit::SelectTheme(_)
            | SettingsHit::SelectAccent(_)
            | SettingsHit::PushPage(_)
            | SettingsHit::DockPosTile(_)
            | SettingsHit::Control(_)
            | SettingsHit::StepperMinus(_)
            | SettingsHit::StepperPlus(_),
        ) => DamageScope::Page,
    }
}

impl PointerHandler for SettingsClient {
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
                    self.pointer_inside = true;
                    let layout = self.current_layout();
                    let next = hit_test(&layout, ev.position.0, ev.position.1);
                    if next != self.hover_hit {
                        let scope = hit_damage_scope(self.hover_hit.as_ref())
                            .merge(hit_damage_scope(next.as_ref()));
                        self.hover_hit = next;
                        if scope != DamageScope::None {
                            self.app.mark_dirty(scope);
                        }
                    }
                }
                PointerEventKind::Leave { .. } => {
                    self.pointer_inside = false;
                    if let Some(prev) = self.hover_hit.take() {
                        let scope = hit_damage_scope(Some(&prev));
                        if scope != DamageScope::None {
                            self.app.mark_dirty(scope);
                        }
                    }
                }
                PointerEventKind::Press {
                    button: BTN_LEFT,
                    serial,
                    ..
                } => {
                    self.execute_left_click(ev.position.0, ev.position.1, serial);
                }
                PointerEventKind::Axis {
                    vertical, source, ..
                } => {
                    let (delta_px, is_discrete) = axis_scroll_delta_px(vertical, source);
                    if delta_px.abs() >= 1e-4 {
                        let layout = self.current_layout();
                        let in_sidebar = self.pointer_pos.0 < f64::from(SIDEBAR_WIDTH);
                        if self.app.scroll_smooth_by(
                            in_sidebar,
                            delta_px,
                            layout.max_sidebar_scroll,
                            layout.max_page_scroll,
                            is_discrete,
                        ) {
                            self.layout_cache = None;
                        }
                    }
                }
                PointerEventKind::Press { .. } | PointerEventKind::Release { .. } => {}
            }
        }
    }
}

fn axis_scroll_delta_px(axis: AxisScroll, source: Option<wl_pointer::AxisSource>) -> (f64, bool) {
    if axis.discrete != 0 {
        return (f64::from(axis.discrete) * 32.0, true);
    }
    if axis.absolute.abs() < 1e-4 {
        return (0.0, false);
    }
    if matches!(
        source,
        Some(wl_pointer::AxisSource::Wheel | wl_pointer::AxisSource::WheelTilt)
    ) {
        (axis.absolute * 2.8, true)
    } else {
        (axis.absolute * 1.25, false)
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for SettingsClient {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Modifiers { mods_depressed, .. } => {
                state.mod_shift = (mods_depressed & 1) != 0;
                state.mod_ctrl = (mods_depressed & 4) != 0;
            }
            wl_keyboard::Event::Leave { .. } => {
                state.mod_shift = false;
                state.mod_ctrl = false;
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                if state.mod_ctrl && key == 33 {
                    // Ctrl+F focuses Search
                    state.app.search_focused = true;
                    state.app.caret_visible = true;
                    state.app.mark_dirty(DamageScope::Sidebar);
                    return;
                }
                if state.mod_ctrl && key == 16 {
                    state.app.request_quit();
                    return;
                }
                let sym = evdev_key_to_sym(key, state.mod_shift);
                if !sym.is_empty() && state.app.handle_search_key(sym) {
                    state.layout_cache = None;
                    state.flush_pending_service_commands();
                    return;
                }
                if !state.app.search_focused {
                    let layout = state.current_layout();
                    let page_step = (layout.page_viewport.h - 40).max(120);
                    let delta = match key {
                        103 => -44,                     // Up
                        108 => 44,                      // Down
                        104 => -page_step,              // PageUp
                        109 => page_step,               // PageDown
                        102 => -layout.max_page_scroll, // Home
                        107 => layout.max_page_scroll,  // End
                        _ => 0,
                    };
                    if delta != 0
                        && state.app.scroll_smooth_by(
                            false,
                            f64::from(delta),
                            layout.max_sidebar_scroll,
                            layout.max_page_scroll,
                            true,
                        )
                    {
                        state.layout_cache = None;
                    }
                }
            }
            _ => {}
        }
    }
}

fn evdev_key_to_sym(code: u32, shift: bool) -> &'static str {
    match code {
        1 => "Escape",
        14 => "BackSpace",
        28 => "Return",
        57 => " ",
        16..=25 => ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"][(code - 16) as usize],
        30..=38 => ["a", "s", "d", "f", "g", "h", "j", "k", "l"][(code - 30) as usize],
        44..=50 => ["z", "x", "c", "v", "b", "n", "m"][(code - 44) as usize],
        2..=11 => {
            if shift {
                ""
            } else {
                ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"][(code - 2) as usize]
            }
        }
        12 => "-",
        _ => "",
    }
}
