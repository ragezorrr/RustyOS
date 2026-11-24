//! Wayland CSD (`xdg_toplevel`) event loop and input dispatch for `rusty-settings` (`RUSTYOS-103`).

use std::fmt;
use std::os::fd::OwnedFd;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::DataSourceHandler,
    },
    delegate_compositor, delegate_data_device, delegate_keyboard, delegate_output,
    delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle, ping},
        calloop_wayland_source::WaylandSource,
        csd_frame::WindowState,
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
            AxisScroll, BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler,
            cursor_shape::CursorShapeManager,
        },
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
    Connection, Dispatch, EventQueue, QueueHandle,
    backend::WaylandError,
    globals::registry_queue_init,
    protocol::{
        wl_data_device, wl_data_device_manager::DndAction, wl_data_source, wl_keyboard, wl_output,
        wl_pointer, wl_seat, wl_shm, wl_surface,
    },
};

use crate::layout::{
    HEADER_HEIGHT, ResizeEdgeZone, SIDEBAR_WIDTH, SettingsHit, SettingsLayout,
    WINDOW_CORNER_RADIUS, compute_layout, hit_test,
};
use crate::liquid_glass::{GlassStyle, RustyosLiquidGlassManagerV1, RustyosSurfaceGlassV1};
use crate::render::{render_window_pixels_with_glass, with_font_engine};
use crate::state::{
    DamageScope, ServiceCommand, ServiceSnapshot, SettingsAppState, SettingsPage, StartupOptions,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderBackend {
    Gpu,
    Cpu,
}

#[allow(clippy::struct_excessive_bools)]
struct SettingsClient {
    conn: Connection,
    qh: QueueHandle<Self>,
    loop_handle: LoopHandle<'static, Self>,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor_state: CompositorState,
    shm: Shm,
    data_device_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    clipboard: rusty_ui::WaylandClipboardBridge,
    paste_ping: ping::Ping,
    last_input_serial: u32,
    _glass_mgr: Option<RustyosLiquidGlassManagerV1>,
    window_glass: Option<RustyosSurfaceGlassV1>,
    last_glass_state: Option<(u32, u32, bool)>,
    pool: SlotPool,
    window: Option<Window>,
    gles: Option<rusty_ui::GlesSurfaceRenderer>,
    render_backend: RenderBackend,
    cpu_backbuf: Vec<u8>,
    buffer_scale: i32,
    buffers: Vec<Buffer>,
    buffer_size: (u32, u32),
    frame_sent_at: Option<Instant>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shape_manager: Option<CursorShapeManager>,
    cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    pointer_enter_serial: u32,
    last_cursor_shape: Option<CursorShape>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,
    width: u32,
    height: u32,
    configured: bool,
    pointer_inside: bool,
    pointer_pressed: bool,
    scrollbar_drag: Option<(f64, i32)>,
    click_streak: u8,
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
        self.app.sync_toggles_from_layout(&l);
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

    fn sync_clipboard_io(&mut self) {
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
            let mime = offer.with_mime_types(rusty_ui::WaylandClipboardBridge::pick_utf8_mime);
            if let Some(mime) = mime
                && let Ok(read_pipe) = offer.receive(mime)
            {
                let _ = self.conn.flush();
                let wake = self.paste_ping.clone();
                self.clipboard
                    .spawn_receive_from_fd(OwnedFd::from(read_pipe), move || wake.ping());
            }
        }
    }

    fn handle_key(&mut self, keysym: Keysym, utf8: Option<&str>) {
        if self.modifiers.ctrl && matches!(keysym, Keysym::q | Keysym::Q) {
            self.app.request_quit();
            return;
        }
        if self.app.active_modal.is_some() {
            if let Some(ui_key) = map_keysym_to_ui(keysym, utf8) {
                let mods = rusty_ui::InputModifiers {
                    ctrl: self.modifiers.ctrl,
                    shift: self.modifiers.shift,
                    alt: self.modifiers.alt,
                };
                if with_font_engine(|fe| {
                    self.app
                        .handle_modal_text_key(ui_key, mods, fe, &mut self.clipboard)
                }) {
                    self.sync_clipboard_io();
                    self.layout_cache = None;
                    self.flush_pending_service_commands();
                }
            }
            return;
        }
        if self.app.current_page == SettingsPage::Battery && self.app.low_power_dropdown.open {
            let ctrl_key = match keysym {
                Keysym::Up => Some(rusty_ui::ControlKey::Up),
                Keysym::Down => Some(rusty_ui::ControlKey::Down),
                Keysym::Home => Some(rusty_ui::ControlKey::Home),
                Keysym::End => Some(rusty_ui::ControlKey::End),
                Keysym::Return | Keysym::KP_Enter => Some(rusty_ui::ControlKey::Enter),
                Keysym::Escape => Some(rusty_ui::ControlKey::Escape),
                _ => None,
            };
            if let Some(ck) = ctrl_key {
                if let Some(idx) = self.app.low_power_dropdown.handle_key(ck) {
                    self.app.select_low_power_mode(idx);
                    self.flush_pending_service_commands();
                } else {
                    self.app.mark_dirty(DamageScope::Page);
                }
                self.layout_cache = None;
                return;
            }
        }
        if self.modifiers.ctrl && matches!(keysym, Keysym::f | Keysym::F) {
            self.app.set_search_focused(true);
            return;
        }
        if self.app.search_focused
            && let Some(ui_key) = map_keysym_to_ui(keysym, utf8)
        {
            let mods = rusty_ui::InputModifiers {
                ctrl: self.modifiers.ctrl,
                shift: self.modifiers.shift,
                alt: self.modifiers.alt,
            };
            let handled = with_font_engine(|fe| {
                self.app
                    .handle_search_text_key(ui_key, mods, fe, &mut self.clipboard)
            });
            self.sync_clipboard_io();
            if handled {
                self.layout_cache = None;
                self.flush_pending_service_commands();
                return;
            }
        }
        if !self.app.search_focused {
            let layout = self.current_layout();
            let page_step = (layout.page_viewport.h - 40).max(120);
            let delta = match keysym {
                Keysym::Up => -44,
                Keysym::Down => 44,
                Keysym::Page_Up => -page_step,
                Keysym::Page_Down => page_step,
                Keysym::Home => -layout.max_page_scroll,
                Keysym::End => layout.max_page_scroll,
                _ => 0,
            };
            if delta != 0
                && self.app.scroll_smooth_by(
                    false,
                    f64::from(delta),
                    layout.max_sidebar_scroll,
                    layout.max_page_scroll,
                    true,
                )
            {
                self.layout_cache = None;
            }
        }
    }

    fn sync_cursor_shape(&mut self) {
        let shape = match self.hover_hit {
            Some(SettingsHit::ResizeEdge(ResizeEdgeZone::Left | ResizeEdgeZone::Right)) => {
                CursorShape::EwResize
            }
            Some(SettingsHit::ResizeEdge(ResizeEdgeZone::Top | ResizeEdgeZone::Bottom)) => {
                CursorShape::NsResize
            }
            Some(SettingsHit::ResizeEdge(
                ResizeEdgeZone::TopLeft | ResizeEdgeZone::BottomRight,
            )) => CursorShape::NwseResize,
            Some(SettingsHit::ResizeEdge(
                ResizeEdgeZone::TopRight | ResizeEdgeZone::BottomLeft,
            )) => CursorShape::NeswResize,
            Some(SettingsHit::SearchField) => CursorShape::Text,
            Some(
                SettingsHit::TrafficClose
                | SettingsHit::TrafficMinimize
                | SettingsHit::TrafficMaximize
                | SettingsHit::NavBack
                | SettingsHit::NavForward
                | SettingsHit::SidebarItem(_)
                | SettingsHit::SelectTheme(_)
                | SettingsHit::SelectAccent(_)
                | SettingsHit::Control(_)
                | SettingsHit::StepperMinus(_)
                | SettingsHit::StepperPlus(_)
                | SettingsHit::PushPage(_)
                | SettingsHit::DockPosTile(_)
                | SettingsHit::BatteryTab(_)
                | SettingsHit::LowPowerDropdownTrigger,
            ) => CursorShape::Pointer,
            _ => CursorShape::Default,
        };
        if self.last_cursor_shape != Some(shape)
            && let Some(dev) = &self.cursor_shape_device
        {
            dev.set_shape(self.pointer_enter_serial, shape);
            self.last_cursor_shape = Some(shape);
        }
    }

    fn execute_left_click(&mut self, px: f64, py: f64, serial: u32) {
        self.last_input_serial = serial;
        let now = Instant::now();
        let multi = self
            .last_click_at
            .is_some_and(|t| now.saturating_duration_since(t) <= Duration::from_millis(380));
        self.click_streak = if multi {
            self.click_streak.saturating_add(1).min(3)
        } else {
            1
        };
        let double_click = self.click_streak == 2;
        self.last_click_at = Some(now);

        let layout = self.current_layout();
        if self.app.active_modal.is_some() {
            self.handle_modal_left_press(px, py, double_click);
            return;
        }
        if self.app.current_page == SettingsPage::Battery
            && self.app.low_power_dropdown.open
            && let Some(dd_rect) = layout.low_power_dropdown_rect
        {
            self.app.low_power_dropdown.rect = rusty_ui::Rect::new(
                dd_rect.x,
                dd_rect.y,
                u32::try_from(dd_rect.w.max(1)).unwrap_or(1),
                u32::try_from(dd_rect.h.max(1)).unwrap_or(1),
            );
            let picked = self.app.low_power_dropdown.on_pointer_down(px, py);
            if let Some(idx) = picked {
                self.app.select_low_power_mode(idx);
                self.flush_pending_service_commands();
                self.layout_cache = None;
                return;
            }
            if self.app.low_power_dropdown.open {
                self.app.mark_dirty(DamageScope::Page);
                return;
            }
            self.app.mark_dirty(DamageScope::Page);
            self.layout_cache = None;
            if dd_rect.contains_f64(px, py) {
                return;
            }
        }
        let hit = hit_test(&layout, px, py);
        if !matches!(hit, Some(SettingsHit::SearchField)) && self.app.search_focused {
            self.app.set_search_focused(false);
        }
        if hit.is_none() && self.handle_scrollbar_track_jump(&layout, px, py) {
            return;
        }
        let Some(hit) = hit else {
            return;
        };
        self.dispatch_hit(hit, &layout, px, py, serial, double_click);
        self.flush_pending_service_commands();
    }

    #[allow(clippy::cast_possible_truncation)]
    fn handle_scrollbar_track_jump(&mut self, layout: &SettingsLayout, px: f64, py: f64) -> bool {
        if layout.max_page_scroll <= 0
            || px < f64::from(layout.win_w - 11)
            || py < f64::from(HEADER_HEIGHT)
        {
            return false;
        }
        let thumb_h = layout.page_scrollbar_thumb.map_or(28, |t| t.h);
        let track_top = f64::from(HEADER_HEIGHT + 6);
        let travel = f64::from((layout.win_h - HEADER_HEIGHT - 14 - thumb_h).max(1));
        let rel = ((py - track_top - f64::from(thumb_h) * 0.5) / travel).clamp(0.0, 1.0);
        let target_y = (rel * f64::from(layout.max_page_scroll)).round() as i32;
        self.app
            .set_page_scroll_immediate(target_y, layout.max_page_scroll);
        self.layout_cache = None;
        true
    }

    #[allow(clippy::cast_possible_truncation)]
    fn update_scrollbar_drag(&mut self, py: f64) -> bool {
        let Some((start_py, start_scroll)) = self.scrollbar_drag else {
            return false;
        };
        let layout = self.current_layout();
        let thumb_h = layout.page_scrollbar_thumb.map_or(28, |t| t.h);
        let travel = (layout.win_h - HEADER_HEIGHT - 14 - thumb_h).max(1);
        let delta_scroll = ((py - start_py) * f64::from(layout.max_page_scroll) / f64::from(travel))
            .round() as i32;
        self.app
            .set_page_scroll_immediate(start_scroll + delta_scroll, layout.max_page_scroll);
        self.layout_cache = None;
        true
    }

    fn dispatch_hit(
        &mut self,
        hit: SettingsHit,
        layout: &SettingsLayout,
        px: f64,
        py: f64,
        serial: u32,
        double_click: bool,
    ) {
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
            other => self.dispatch_content_hit(other, layout, px, py),
        }
    }

    fn dispatch_content_hit(
        &mut self,
        hit: SettingsHit,
        layout: &SettingsLayout,
        px: f64,
        py: f64,
    ) {
        match hit {
            SettingsHit::NavBack => {
                self.app.go_back();
                self.layout_cache = None;
            }
            SettingsHit::NavForward => {
                self.app.go_forward();
                self.layout_cache = None;
            }
            SettingsHit::SearchField => {
                self.app.set_search_focused(true);
                let sr = layout.search_rect;
                self.app.search_field.rect = rusty_ui::Rect::new(
                    sr.x,
                    sr.y,
                    u32::try_from(sr.w.max(1)).unwrap_or(1),
                    u32::try_from(sr.h.max(1)).unwrap_or(1),
                );
                let click_n = self.click_streak;
                let elapsed = self.app.epoch.elapsed();
                let hit = with_font_engine(|fe| {
                    self.app
                        .search_field
                        .on_pointer_down(px, py, click_n, elapsed, fe)
                });
                if matches!(hit, Some(rusty_ui::TextFieldHit::ClearButton)) {
                    self.app.search_query.clear();
                    self.app.reset_sidebar_scroll();
                    self.layout_cache = None;
                    self.app.mark_dirty(DamageScope::Full);
                }
            }
            SettingsHit::ScrollbarThumb => {
                self.scrollbar_drag = Some((py, self.app.page_scroll_y));
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
            SettingsHit::BatteryTab(idx) => {
                self.app.select_battery_tab(idx);
                self.layout_cache = None;
            }
            SettingsHit::LowPowerDropdownTrigger => {
                if let Some(dd_rect) = layout.low_power_dropdown_rect {
                    self.app.low_power_dropdown.rect = rusty_ui::Rect::new(
                        dd_rect.x,
                        dd_rect.y,
                        u32::try_from(dd_rect.w.max(1)).unwrap_or(1),
                        u32::try_from(dd_rect.h.max(1)).unwrap_or(1),
                    );
                }
                let _ = self.app.low_power_dropdown.on_pointer_down(px, py);
                self.app.mark_dirty(DamageScope::Page);
            }
            SettingsHit::Control(id)
            | SettingsHit::StepperMinus(id)
            | SettingsHit::StepperPlus(id) => {
                self.app.apply_row_action(id);
                self.layout_cache = None;
                let next_layout = self.current_layout();
                self.app.sync_toggles_from_layout(&next_layout);
            }
            SettingsHit::ResizeEdge(_)
            | SettingsHit::WindowDrag
            | SettingsHit::TrafficClose
            | SettingsHit::TrafficMinimize
            | SettingsHit::TrafficMaximize => {}
        }
    }

    fn handle_modal_left_press(&mut self, px: f64, py: f64, _double_click: bool) {
        let w = self.width.max(1);
        let h = self.height.max(1);
        let streak = self.click_streak;
        let elapsed = self.app.epoch.elapsed();
        if let Some(modal) = &mut self.app.active_modal {
            if let Some(cb) = modal.wifi_show_password_checkbox(w, h)
                && cb.hit_rect.contains_f64(px, py)
            {
                self.app.toggle_wifi_password_visibility();
                return;
            }
            let sheet = modal.sheet_mut();
            let layout = sheet.sync_input_rect(w, h);
            if let (Some(tf), Some(ir)) = (&mut sheet.input, layout.input_rect)
                && ir.contains_f64(px, py)
            {
                let _ = with_font_engine(|fe| tf.on_pointer_down(px, py, streak, elapsed, fe));
            } else {
                let _ = sheet.on_pointer_down(px, py, w, h);
            }
            self.app.mark_dirty(DamageScope::Full);
        }
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

fn tick_settings_frame(client: &mut SettingsClient, now: Instant) -> Result<(), WaylandAppError> {
    let _ = client.app.update_caret_blink(now);
    if client.app.step_ui_animations(now) {
        client.layout_cache = None;
    }
    if client.app.poll_external_config_if_due(now) {
        client.layout_cache = None;
    }
    sync_settings_glass_region(client);
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
        draw_frame(client)?;
    }
    Ok(())
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

    let (glass_mgr, window_glass) = bind_settings_glass(&globals, &qh, &window);

    let (wake_ping, wake_source) =
        ping::make_ping().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let (paste_ping, paste_source) =
        ping::make_ping().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let (service_tx, service_rx) = spawn_service_worker(wake_ping);

    let mut event_loop: EventLoop<'static, SettingsClient> =
        EventLoop::try_new().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let loop_handle = event_loop.handle();
    insert_settings_sources(&loop_handle, &conn, queue, wake_source, paste_source)?;

    let no_gpu_env = std::env::var_os("RUSTYOS_NO_GPU").is_some_and(|v| v == "1");
    let render_backend = if opts.no_gpu || no_gpu_env {
        RenderBackend::Cpu
    } else {
        RenderBackend::Gpu
    };

    let mut client = SettingsClient {
        conn: conn.clone(),
        qh: qh.clone(),
        loop_handle,
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        compositor_state,
        shm,
        data_device_manager: DataDeviceManagerState::bind(&globals, &qh).ok(),
        data_device: None,
        clipboard: rusty_ui::WaylandClipboardBridge::new(),
        paste_ping,
        last_input_serial: 0,
        _glass_mgr: glass_mgr,
        window_glass,
        last_glass_state: None,
        pool,
        window: Some(window),
        gles: None,
        render_backend,
        cpu_backbuf: Vec::new(),
        buffer_scale: 1,
        buffers: Vec::new(),
        buffer_size: (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        frame_sent_at: None,
        seat: None,
        pointer: None,
        cursor_shape_manager: CursorShapeManager::bind(&globals, &qh).ok(),
        cursor_shape_device: None,
        pointer_enter_serial: 0,
        last_cursor_shape: None,
        keyboard: None,
        modifiers: Modifiers::default(),
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        configured: false,
        pointer_inside: false,
        pointer_pressed: false,
        scrollbar_drag: None,
        click_streak: 1,
        pointer_pos: (0.0, 0.0),
        hover_hit: None,
        last_click_at: None,
        app: SettingsAppState::new(opts),
        layout_cache: None,
        service_tx,
        service_rx,
    };
    client.flush_pending_service_commands();

    while client.app.running {
        let now = Instant::now();
        tick_settings_frame(&mut client, now)?;
        conn.flush()?;

        let timeout = if client.app.is_ui_animating() {
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

fn bind_settings_glass(
    globals: &wayland_client::globals::GlobalList,
    qh: &QueueHandle<SettingsClient>,
    window: &Window,
) -> (
    Option<RustyosLiquidGlassManagerV1>,
    Option<RustyosSurfaceGlassV1>,
) {
    let glass_mgr: Option<RustyosLiquidGlassManagerV1> = globals.bind(qh, 1..=1, ()).ok();
    let window_glass = glass_mgr.as_ref().map(|mgr| {
        let g = mgr.get_surface_glass(window.wl_surface(), qh, ());
        g.set_corner_radius(u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14));
        g.set_style(GlassStyle::Sidebar);
        g.commit();
        g
    });
    window.commit();
    (glass_mgr, window_glass)
}

fn insert_settings_sources(
    loop_handle: &LoopHandle<'static, SettingsClient>,
    conn: &Connection,
    queue: EventQueue<SettingsClient>,
    wake_source: ping::PingSource,
    paste_source: ping::PingSource,
) -> Result<(), WaylandAppError> {
    WaylandSource::new(conn.clone(), queue)
        .insert(loop_handle.clone())
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;
    loop_handle
        .insert_source(wake_source, |(), (), client| {
            while let Ok(snap) = client.service_rx.try_recv() {
                if client.app.apply_service_snapshot(snap) {
                    client.layout_cache = None;
                }
            }
        })
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;
    loop_handle
        .insert_source(paste_source, |(), (), client| {
            if let Some(pasted) = client.clipboard.poll_received_paste() {
                let changed = with_font_engine(|fe| {
                    if client.app.active_modal.is_some() {
                        client.app.handle_modal_paste(&pasted, fe)
                    } else {
                        client.app.apply_search_paste(&pasted, fe)
                    }
                });
                if changed {
                    client.layout_cache = None;
                }
            }
        })
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;
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

fn blit_scale_bgra(src: &[u8], src_w: u32, src_h: u32, dst: &mut [u8], scale: u32) {
    let sw = src_w as usize;
    let sh = src_h as usize;
    let s = scale.max(1) as usize;
    let dw = sw * s;
    for y in 0..sh {
        let src_row = &src[y * sw * 4..(y + 1) * sw * 4];
        for sy in 0..s {
            let dst_row_off = (y * s + sy) * dw * 4;
            for x in 0..sw {
                let px = &src_row[x * 4..x * 4 + 4];
                let dst_x = dst_row_off + x * s * 4;
                for sx in 0..s {
                    dst[dst_x + sx * 4..dst_x + sx * 4 + 4].copy_from_slice(px);
                }
            }
        }
    }
}

#[allow(clippy::cast_possible_wrap)]
fn try_draw_settings_gles(
    client: &mut SettingsClient,
    layout: &SettingsLayout,
    w: u32,
    h: u32,
) -> bool {
    if client.gles.is_none() {
        return false;
    }
    let len = (w as usize) * (h as usize) * 4;
    if client.cpu_backbuf.len() != len {
        client.cpu_backbuf.resize(len, 0);
    }
    let sidebar_glass = client.window_glass.is_some() && !client.app.reduce_transparency;
    render_window_pixels_with_glass(
        &mut client.cpu_backbuf,
        w,
        h,
        &client.app,
        layout,
        client.hover_hit.as_ref(),
        sidebar_glass,
    );
    let damage_scope = client.app.take_damage_scope();
    let (dx, dy, dw, dh) = damage_rect_for_scope(damage_scope, layout, w, h);
    let damage_rect = Some(rusty_ui::Rect::new(
        dx.max(0),
        dy.max(0),
        u32::try_from(dw.max(0)).unwrap_or(0),
        u32::try_from(dh.max(0)).unwrap_or(0),
    ));
    let scale = client.buffer_scale.max(1);
    if let Some(win) = &client.window {
        win.wl_surface().set_buffer_scale(scale);
        win.set_window_geometry(0, 0, w, h);
        win.wl_surface().frame(&client.qh, win.wl_surface().clone());
    }
    let res = if let Some(gles) = &mut client.gles {
        gles.present_bgra_frame(&client.cpu_backbuf, w, h, scale, damage_rect, [0, 0, 0, 0])
    } else {
        Ok(())
    };
    match res {
        Ok(()) => {
            client.frame_sent_at = Some(Instant::now());
            true
        }
        Err(err) => {
            tracing::warn!(%err, "rusty-settings GLES2 swap failed; falling back to wl_shm");
            client.gles = None;
            client.render_backend = RenderBackend::Cpu;
            false
        }
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

    if client.window.is_none() {
        return Ok(());
    }
    let w = client.width.max(1);
    let h = client.height.max(1);
    if try_draw_settings_gles(client, &layout, w, h) {
        return Ok(());
    }
    let Some(win) = &client.window else {
        return Ok(());
    };
    let scale = client.buffer_scale.max(1);
    let scale_u = u32::try_from(scale).unwrap_or(1);
    let bw = w.saturating_mul(scale_u);
    let bh = h.saturating_mul(scale_u);
    if client.buffer_size != (bw, bh) {
        client.buffers.clear();
        client.buffer_size = (bw, bh);
    }
    let idx = acquire_reusable_buffer(&mut client.pool, &mut client.buffers, bw, bh)?;
    let buf_ref = &client.buffers[idx];
    let canvas = buf_ref
        .canvas(&mut client.pool)
        .ok_or_else(|| WaylandAppError::ShmPool(String::from("Settings SHM slot busy")))?;

    let sidebar_glass = client.window_glass.is_some() && !client.app.reduce_transparency;
    if scale_u == 1 {
        render_window_pixels_with_glass(
            canvas,
            w,
            h,
            &client.app,
            &layout,
            client.hover_hit.as_ref(),
            sidebar_glass,
        );
    } else {
        let len = (w as usize) * (h as usize) * 4;
        if client.cpu_backbuf.len() != len {
            client.cpu_backbuf.resize(len, 0);
        }
        render_window_pixels_with_glass(
            &mut client.cpu_backbuf,
            w,
            h,
            &client.app,
            &layout,
            client.hover_hit.as_ref(),
            sidebar_glass,
        );
        blit_scale_bgra(&client.cpu_backbuf, w, h, canvas, scale_u);
    }
    let damage_scope = client.app.take_damage_scope();
    let (dx, dy, dw, dh) = damage_rect_for_scope(damage_scope, &layout, w, h);

    win.wl_surface().set_buffer_scale(scale);
    win.set_window_geometry(0, 0, w, h);
    buf_ref
        .attach_to(win.wl_surface())
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
    win.wl_surface().damage_buffer(
        dx.saturating_mul(scale),
        dy.saturating_mul(scale),
        dw.saturating_mul(scale),
        dh.saturating_mul(scale),
    );
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
delegate_keyboard!(SettingsClient);
delegate_pointer!(SettingsClient);
delegate_data_device!(SettingsClient);
delegate_xdg_shell!(SettingsClient);
delegate_xdg_window!(SettingsClient);

impl Dispatch<RustyosLiquidGlassManagerV1, ()> for SettingsClient {
    fn event(
        _state: &mut Self,
        _proxy: &RustyosLiquidGlassManagerV1,
        _event: crate::liquid_glass::protocol::rustyos_liquid_glass_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RustyosSurfaceGlassV1, ()> for SettingsClient {
    fn event(
        _state: &mut Self,
        _proxy: &RustyosSurfaceGlassV1,
        _event: crate::liquid_glass::protocol::rustyos_surface_glass_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

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
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let scale = new_factor.max(1);
        if self
            .window
            .as_ref()
            .is_some_and(|w| w.wl_surface() == surface)
            && self.buffer_scale != scale
        {
            self.buffer_scale = scale;
            surface.set_buffer_scale(scale);
            self.app.mark_dirty(DamageScope::Full);
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
        let next_focused = configure.state.contains(WindowState::ACTIVATED);
        let focus_changed = self.app.focused != next_focused;
        let max_changed = self.app.maximized != configure.is_maximized();
        let size_changed = next_w != self.width || next_h != self.height || !self.configured;
        if size_changed || max_changed || focus_changed {
            self.width = next_w;
            self.height = next_h;
            self.layout_cache = None;
            self.app.maximized = configure.is_maximized();
            self.app.focused = next_focused;
            self.configured = true;
            self.app.mark_dirty(if size_changed || max_changed {
                DamageScope::Full
            } else {
                DamageScope::Sidebar
            });
        }
        if self.render_backend == RenderBackend::Gpu
            && self.gles.is_none()
            && let Some(win) = &self.window
        {
            match rusty_ui::GlesSurfaceRenderer::new(
                self.conn.backend().display_ptr().cast(),
                win.wl_surface(),
                next_w.cast_signed(),
                next_h.cast_signed(),
            ) {
                Ok(renderer) => {
                    self.gles = Some(renderer);
                }
                Err(err) => {
                    tracing::warn!(%err, "rusty-settings GLES2 unavailable; using CPU wl_shm fallback");
                    self.render_backend = RenderBackend::Cpu;
                }
            }
        }
        sync_settings_glass_region(self);
    }
}

fn sync_settings_glass_region(client: &mut SettingsClient) {
    let Some(ref g) = client.window_glass else {
        return;
    };
    let next = (client.width, client.height, !client.app.reduce_transparency);
    if client.last_glass_state == Some(next) {
        return;
    }
    client.last_glass_state = Some(next);
    if let Ok(region) = Region::new(&client.compositor_state) {
        if !client.app.reduce_transparency {
            let wi = i32::try_from(client.width).unwrap_or(1);
            let hi = i32::try_from(client.height).unwrap_or(1);
            let glass_w = (SIDEBAR_WIDTH + WINDOW_CORNER_RADIUS).min(wi).max(1);
            region.add(0, 0, glass_w, hi.max(1));
        }
        g.set_region(Some(region.wl_region()));
    }
    g.set_corner_radius(u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14));
    g.set_style(GlassStyle::Sidebar);
    g.commit();
    client.app.mark_dirty(DamageScope::Full);
}

impl SeatHandler for SettingsClient {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if self.data_device.is_none()
            && let Some(ddm) = &self.data_device_manager
        {
            self.data_device = Some(ddm.get_data_device(qh, &seat));
        }
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
                self.cursor_shape_device = self
                    .cursor_shape_manager
                    .as_ref()
                    .map(|m| m.get_shape_device(&ptr, qh));
                self.pointer = Some(ptr);
            }
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
            | SettingsHit::BatteryTab(_)
            | SettingsHit::LowPowerDropdownTrigger
            | SettingsHit::Control(_)
            | SettingsHit::StepperMinus(_)
            | SettingsHit::StepperPlus(_)
            | SettingsHit::ScrollbarThumb,
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
                PointerEventKind::Enter { serial } => {
                    self.pointer_enter_serial = serial;
                    self.last_cursor_shape = None;
                    self.pointer_inside = true;
                    if !self.handle_modal_pointer_motion(ev.position.0, ev.position.1)
                        && !self.update_scrollbar_drag(ev.position.1)
                    {
                        self.handle_page_pointer_motion(ev.position.0, ev.position.1);
                    }
                    self.sync_cursor_shape();
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_inside = true;
                    if self.handle_modal_pointer_motion(ev.position.0, ev.position.1)
                        || self.update_scrollbar_drag(ev.position.1)
                    {
                        continue;
                    }
                    self.handle_page_pointer_motion(ev.position.0, ev.position.1);
                    self.sync_cursor_shape();
                }
                PointerEventKind::Leave { .. } => {
                    self.pointer_inside = false;
                    self.last_cursor_shape = None;
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
                    self.pointer_pressed = true;
                    self.execute_left_click(ev.position.0, ev.position.1, serial);
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => {
                    self.pointer_pressed = false;
                    self.scrollbar_drag = None;
                    self.app.search_field.on_pointer_up();
                    self.handle_modal_left_release(ev.position.0, ev.position.1);
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

impl SettingsClient {
    fn handle_modal_pointer_motion(&mut self, px: f64, py: f64) -> bool {
        if self.app.active_modal.is_none() {
            return false;
        }
        let w = self.width.max(1);
        let h = self.height.max(1);
        let pressed = self.pointer_pressed;
        if let Some(modal) = &mut self.app.active_modal {
            let sheet = modal.sheet_mut();
            let mut changed = sheet.on_pointer_move(px, py, w, h);
            if pressed && let Some(tf) = &mut sheet.input {
                changed |= with_font_engine(|fe| tf.on_pointer_move(px, fe));
            }
            if changed {
                self.app.mark_dirty(DamageScope::Full);
            }
        }
        true
    }

    fn handle_page_pointer_motion(&mut self, px: f64, py: f64) {
        if self.pointer_pressed && self.app.search_focused {
            let changed = with_font_engine(|fe| self.app.search_field.on_pointer_move(px, fe));
            if changed {
                self.app.mark_dirty(DamageScope::Sidebar);
            }
        }
        let layout = self.current_layout();
        if self.app.current_page == SettingsPage::Battery
            && self.app.low_power_dropdown.open
            && let Some(dd_rect) = layout.low_power_dropdown_rect
        {
            self.app.low_power_dropdown.rect = rusty_ui::Rect::new(
                dd_rect.x,
                dd_rect.y,
                u32::try_from(dd_rect.w.max(1)).unwrap_or(1),
                u32::try_from(dd_rect.h.max(1)).unwrap_or(1),
            );
            if self.app.low_power_dropdown.on_pointer_move(px, py) {
                self.app.mark_dirty(DamageScope::Page);
            }
        }
        let next = hit_test(&layout, px, py);
        if next != self.hover_hit {
            let scope =
                hit_damage_scope(self.hover_hit.as_ref()).merge(hit_damage_scope(next.as_ref()));
            self.hover_hit = next;
            if scope != DamageScope::None {
                self.app.mark_dirty(scope);
            }
        }
    }

    fn handle_modal_left_release(&mut self, px: f64, py: f64) {
        if self.app.active_modal.is_none() {
            return;
        }
        let w = self.width.max(1);
        let h = self.height.max(1);
        let act = if let Some(modal) = &mut self.app.active_modal {
            let sheet = modal.sheet_mut();
            if let Some(tf) = &mut sheet.input {
                tf.on_pointer_up();
            }
            sheet.on_pointer_up(px, py, w, h)
        } else {
            None
        };
        match act {
            Some(rusty_ui::DialogAction::Confirm) => {
                self.app.confirm_active_modal();
                self.layout_cache = None;
                self.flush_pending_service_commands();
            }
            Some(rusty_ui::DialogAction::Cancel) => {
                self.app.dismiss_active_modal();
                self.layout_cache = None;
            }
            None => self.app.mark_dirty(DamageScope::Full),
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

impl KeyboardHandler for SettingsClient {
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

impl DataDeviceHandler for SettingsClient {
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

impl DataOfferHandler for SettingsClient {
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

impl DataSourceHandler for SettingsClient {
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
        self.clipboard
            .serve_send_request(source, &mime, OwnedFd::from(write_pipe));
    }
    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
    ) {
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

fn map_keysym_to_ui(keysym: Keysym, utf8: Option<&str>) -> Option<rusty_ui::TextInputKey> {
    match keysym {
        Keysym::Left => Some(rusty_ui::TextInputKey::Left),
        Keysym::Right => Some(rusty_ui::TextInputKey::Right),
        Keysym::Home => Some(rusty_ui::TextInputKey::Home),
        Keysym::End => Some(rusty_ui::TextInputKey::End),
        Keysym::BackSpace => Some(rusty_ui::TextInputKey::Backspace),
        Keysym::Delete => Some(rusty_ui::TextInputKey::Delete),
        Keysym::Return | Keysym::KP_Enter => Some(rusty_ui::TextInputKey::Enter),
        Keysym::Escape => Some(rusty_ui::TextInputKey::Escape),
        _ => utf8
            .and_then(|s| s.chars().next())
            .filter(|ch| !ch.is_control())
            .map(rusty_ui::TextInputKey::Char)
            .or_else(|| {
                keysym
                    .key_char()
                    .filter(|ch| !ch.is_control())
                    .map(rusty_ui::TextInputKey::Char)
            }),
    }
}
