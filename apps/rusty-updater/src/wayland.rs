//! Wayland CSD (`xdg_toplevel`) event loop & input dispatch for `rusty-updater`
//! (`RUSTYOS-077`, ADR-0037).

use std::fmt;
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::EventLoop, calloop_wayland_source::WaylandSource,
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

use crate::layout::{ResizeEdgeZone, UpdaterHit, UpdaterLayout, compute_layout, hit_test};
use crate::render::render_window_pixels;
use crate::state::{StartupOptions, UpdaterAppState};

const DEFAULT_WIDTH: u32 = 740;
const DEFAULT_HEIGHT: u32 = 560;

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

struct UpdaterClient {
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
    width: u32,
    height: u32,
    configured: bool,
    maximized: bool,
    pointer_pos: (f64, f64),
    hover_hit: Option<UpdaterHit>,
    app: UpdaterAppState,
    layout_cache: Option<UpdaterLayout>,
}

impl UpdaterClient {
    fn current_layout(&mut self) -> UpdaterLayout {
        let layout = compute_layout(&self.app, self.width, self.height);
        self.layout_cache = Some(layout.clone());
        layout
    }

    fn execute_left_click(&mut self, px: f64, py: f64, serial: u32) {
        let layout = self.current_layout();
        let Some(hit) = hit_test(&layout, px, py) else {
            return;
        };
        match hit {
            UpdaterHit::ResizeEdge(zone) => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.resize(seat, serial, map_resize_edge(zone));
                }
            }
            UpdaterHit::WindowDrag => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.move_(seat, serial);
                }
            }
            UpdaterHit::TrafficClose => self.app.request_quit(),
            UpdaterHit::TrafficMinimize => {
                if let Some(win) = &self.window {
                    win.set_minimized();
                }
            }
            UpdaterHit::TrafficMaximize => {
                if let Some(win) = &self.window {
                    if self.maximized {
                        win.unset_maximized();
                    } else {
                        win.set_maximized();
                    }
                    self.maximized = !self.maximized;
                }
            }
            UpdaterHit::CheckNow => self.app.start_check_now(),
            UpdaterHit::PrimaryAction => self.activate_primary_action(),
            UpdaterHit::FilterTab(filter) => self.app.set_filter(filter),
            UpdaterHit::ToggleLogs => self.app.toggle_logs(),
            UpdaterHit::RebuildLocalPkg => self.app.start_local_rustyos_upgrade(),
            UpdaterHit::PackageRow(idx) => {
                self.app.selected_idx = Some(idx);
                self.app.dirty = true;
            }
        }
    }

    fn activate_primary_action(&mut self) {
        if self.app.is_upgrading() {
            self.app.cancel_upgrade();
        } else if self.app.snapshot.total_count > 0 {
            self.app.start_system_upgrade();
        } else {
            self.app.start_check_now();
        }
    }
}

/// Runs the Wayland CSD `rusty-updater` application loop (`RUSTYOS-077`).
///
/// # Errors
/// Returns [`WaylandAppError`] when Wayland connection, globals, SHM pool, or loop fails.
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
    window.set_title("Software Update");
    window.set_app_id("org.rustyos.Updater");
    window.set_min_size(Some((600, 440)));
    window.commit();

    let mut client = UpdaterClient {
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
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        configured: false,
        maximized: false,
        pointer_pos: (0.0, 0.0),
        hover_hit: None,
        app: UpdaterAppState::new(opts),
        layout_cache: None,
    };

    let mut event_loop: EventLoop<'static, UpdaterClient> =
        EventLoop::try_new().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;

    while client.app.running {
        let now = Instant::now();
        let _ = client.app.poll_background_jobs();
        let _ = client.app.poll_system_theme();
        if let Some(sent_at) = client.frame_sent_at
            && now.saturating_duration_since(sent_at) >= Duration::from_millis(120)
        {
            client.frame_sent_at = None;
        }
        if client.configured && client.frame_sent_at.is_none() && client.app.dirty {
            draw_frame(&mut client)?;
            client.app.dirty = false;
        }
        conn.flush()?;

        let timeout = if client.app.has_pending_jobs() {
            Some(Duration::from_millis(24))
        } else {
            Some(Duration::from_millis(500))
        };
        event_loop
            .dispatch(timeout, &mut client)
            .map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    }
    Ok(())
}

#[allow(clippy::cast_possible_wrap)]
fn draw_frame(client: &mut UpdaterClient) -> Result<(), WaylandAppError> {
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
        .ok_or_else(|| WaylandAppError::ShmPool(String::from("SHM slot unexpectedly busy")))?;

    render_window_pixels(
        canvas,
        w,
        h,
        &client.app,
        &layout,
        client.hover_hit.as_ref(),
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

impl ProvidesRegistryState for UpdaterClient {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(UpdaterClient);
delegate_output!(UpdaterClient);
delegate_compositor!(UpdaterClient);
delegate_shm!(UpdaterClient);
delegate_seat!(UpdaterClient);
delegate_pointer!(UpdaterClient);
delegate_xdg_shell!(UpdaterClient);
delegate_xdg_window!(UpdaterClient);

impl OutputHandler for UpdaterClient {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl CompositorHandler for UpdaterClient {
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

impl ShmHandler for UpdaterClient {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl WindowHandler for UpdaterClient {
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
            .map_or(self.width, std::num::NonZero::get)
            .max(480);
        let next_h = configure
            .new_size
            .1
            .map_or(self.height, std::num::NonZero::get)
            .max(380);
        if next_w != self.width || next_h != self.height || !self.configured {
            self.width = next_w;
            self.height = next_h;
            self.configured = true;
            self.app.dirty = true;
        }
    }
}

impl SeatHandler for UpdaterClient {
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

impl PointerHandler for UpdaterClient {
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
                        self.app.dirty = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.hover_hit.take().is_some() {
                        self.app.dirty = true;
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
                    let delta = axis_scroll_delta(vertical, source);
                    if delta != 0 {
                        let layout = self.current_layout();
                        self.app.scroll_by(delta, layout.list_viewport.h);
                    }
                }
                _ => {}
            }
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn axis_scroll_delta(axis: AxisScroll, source: Option<wl_pointer::AxisSource>) -> i32 {
    if axis.discrete != 0 {
        return axis.discrete * 28;
    }
    if axis.absolute.abs() < 0.01 {
        return 0;
    }
    let scale = if matches!(
        source,
        Some(wl_pointer::AxisSource::Wheel | wl_pointer::AxisSource::WheelTilt)
    ) {
        2.8
    } else {
        1.25
    };
    (axis.absolute * scale).round() as i32
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for UpdaterClient {
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
                state.mod_ctrl = (mods_depressed & 4) != 0;
            }
            wl_keyboard::Event::Leave { .. } => state.mod_ctrl = false,
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                let key_name = match key {
                    1 => "Escape",
                    16 => "q",
                    19 => "r",
                    38 => "l",
                    103 => "Up",
                    104 => "Page_Up",
                    108 => "Down",
                    109 => "Page_Down",
                    _ => "",
                };
                if !key_name.is_empty() {
                    let vh = state
                        .layout_cache
                        .as_ref()
                        .map_or(280, |l| l.list_viewport.h);
                    state.app.handle_key_input(key_name, state.mod_ctrl, vh);
                }
            }
            _ => {}
        }
    }
}
