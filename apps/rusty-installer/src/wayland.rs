//! Wayland CSD (`xdg_toplevel`) event loop, Liquid Glass binding, and input dispatch
//! for `rusty-installer` (`RUSTYOS-115`, ADR-0065).

use std::fmt;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
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
    Connection, Dispatch, EventQueue, QueueHandle,
    backend::WaylandError,
    globals::registry_queue_init,
    protocol::{
        wl_data_device, wl_data_device_manager::DndAction, wl_data_source, wl_keyboard, wl_output,
        wl_pointer, wl_seat, wl_shm, wl_surface,
    },
};

use crate::disks::TargetDiskInfo;
use crate::layout::{
    InstallerHit, InstallerLayout, MIN_HEIGHT, MIN_WIDTH, ResizeEdgeZone, WINDOW_CORNER_RADIUS,
    compute_layout, hit_test,
};
use crate::liquid_glass::{GlassStyle, RustyosLiquidGlassManagerV1, RustyosSurfaceGlassV1};
use crate::render::{render_window_pixels, with_font_engine};
use crate::state::{
    DiskInstallMode, InstallPhase, InstallStep, InstallerAppState, StartupOptions, TextFieldId,
    WizardMode,
};

const DEFAULT_WIDTH: u32 = 880;
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
    conn: Connection,
    qh: QueueHandle<Self>,
    loop_handle: LoopHandle<'static, Self>,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    data_device_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    clipboard: rusty_ui::WaylandClipboardBridge,
    paste_ping: ping::Ping,
    last_input_serial: u32,
    window: Option<Window>,
    gles: Option<rusty_ui::GlesSurfaceRenderer>,
    use_gpu: bool,
    cpu_backbuf: Vec<u8>,
    frame_rev: u64,
    _glass_mgr: Option<RustyosLiquidGlassManagerV1>,
    window_glass: Option<RustyosSurfaceGlassV1>,
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
    pointer_pressed: bool,
    click_streak: u8,
    last_click_at: Option<Instant>,
    hover_hit: Option<InstallerHit>,
    app: InstallerAppState,
    layout_cache: Option<InstallerLayout>,
}

impl InstallerClient {
    const fn has_glass(&self) -> bool {
        self.window_glass.is_some()
    }

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
        if self.app.caret.focused_field.is_some()
            && let Some(ui_key) = map_keysym_to_ui(keysym, utf8)
        {
            let mods = rusty_ui::InputModifiers {
                ctrl: self.modifiers.ctrl,
                shift: self.modifiers.shift,
                alt: self.modifiers.alt,
            };
            let ev = with_font_engine(|fe| {
                self.app
                    .handle_text_key(ui_key, mods, fe, &mut self.clipboard)
            });
            self.sync_clipboard_io();
            match ev {
                rusty_ui::TextInputEvent::Cancelled => {
                    self.app.set_focused_field(None);
                    return;
                }
                rusty_ui::TextInputEvent::Submitted => {
                    if self.app.caret.focused_field == Some(crate::state::TextFieldId::WifiPassword)
                    {
                        self.app.connect_selected_wifi();
                    } else {
                        self.app.go_next(Instant::now());
                    }
                    self.layout_cache = None;
                    return;
                }
                rusty_ui::TextInputEvent::Changed | rusty_ui::TextInputEvent::CaretMoved => {
                    return;
                }
                rusty_ui::TextInputEvent::Ignored => {}
            }
        }
        match keysym {
            Keysym::Escape if self.app.caret.focused_field.is_some() => {
                self.app.set_focused_field(None);
            }
            Keysym::Tab => self.app.cycle_focus(self.modifiers.shift),
            Keysym::ISO_Left_Tab => self.app.cycle_focus(true),
            Keysym::BackSpace => self.app.handle_backspace(),
            Keysym::Left
                if self.app.mode == WizardMode::Install
                    && matches!(
                        self.app.install_step,
                        InstallStep::Disk | InstallStep::Partition
                    ) =>
            {
                self.app.disk_mode = DiskInstallMode::WholeDisk;
                self.app.win.dirty = true;
                self.layout_cache = None;
            }
            Keysym::Right
                if self.app.mode == WizardMode::Install
                    && matches!(
                        self.app.install_step,
                        InstallStep::Disk | InstallStep::Partition
                    ) =>
            {
                self.app.disk_mode = DiskInstallMode::DualBoot;
                self.app.win.dirty = true;
                self.layout_cache = None;
            }
            Keysym::Return | Keysym::KP_Enter => {
                if self.app.caret.focused_field == Some(crate::state::TextFieldId::WifiPassword) {
                    self.app.connect_selected_wifi();
                } else {
                    self.app.go_next(Instant::now());
                }
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
        let Some(hit) = hit_test(&layout, px, py) else {
            if self.app.caret.focused_field.is_some() {
                self.app.set_focused_field(None);
            }
            return;
        };

        if !matches!(hit, InstallerHit::FocusField(_)) && self.app.caret.focused_field.is_some() {
            self.app.set_focused_field(None);
        }
        self.dispatch_hit(hit, px, py, serial, double_click, now);
    }

    fn dispatch_hit(
        &mut self,
        hit: InstallerHit,
        px: f64,
        py: f64,
        serial: u32,
        double_click: bool,
        now: Instant,
    ) {
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
            InstallerHit::OtherButton => {
                self.app.trigger_other_action();
                self.layout_cache = None;
            }
            InstallerHit::TimezoneRow(idx) => {
                self.app.timezone_idx = idx;
                self.app.win.dirty = true;
            }
            InstallerHit::PrimaryButton => {
                self.app.go_next(now);
                self.layout_cache = None;
            }
            other => self.dispatch_content_hit(other, px, py, now),
        }
    }

    fn dispatch_content_hit(&mut self, hit: InstallerHit, px: f64, py: f64, _now: Instant) {
        match hit {
            InstallerHit::SelectDiskMode(mode) => {
                self.app.disk_mode = mode;
                self.app.form_error = None;
                self.app.win.dirty = true;
                self.layout_cache = None;
            }
            InstallerHit::SelectDisk(idx) => self.select_storage_target(idx),
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
            InstallerHit::RefreshNetwork => {
                self.app.spawn_network_probe_with_scan(true);
                self.layout_cache = None;
            }
            InstallerHit::ToggleWifi => {
                self.app.toggle_wifi_radio();
                self.layout_cache = None;
            }
            InstallerHit::SelectWifi(idx) => {
                self.app.select_wifi_row(idx);
                self.layout_cache = None;
            }
            InstallerHit::ConnectWifi => {
                self.app.connect_selected_wifi();
                self.layout_cache = None;
            }
            InstallerHit::FocusField(field) => self.focus_and_press_field(field, px, py),
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
            InstallerHit::ResizeEdge(_)
            | InstallerHit::WindowDrag
            | InstallerHit::TrafficClose
            | InstallerHit::TrafficMinimize
            | InstallerHit::TrafficMaximize
            | InstallerHit::BackButton
            | InstallerHit::OtherButton
            | InstallerHit::TimezoneRow(_)
            | InstallerHit::PrimaryButton => {}
        }
    }

    fn focus_and_press_field(&mut self, field: TextFieldId, px: f64, py: f64) {
        self.app.set_focused_field(Some(field));
        let layout = self.current_layout();
        let rect = match &layout.content {
            crate::layout::StepContentLayout::AccountAndStyle { fields, .. }
            | crate::layout::StepContentLayout::OobeAccount { fields, .. } => {
                fields.iter().find(|f| f.id == field).map(|f| f.rect)
            }
            crate::layout::StepContentLayout::Network { password_field, .. } => password_field
                .as_ref()
                .filter(|f| f.id == field)
                .map(|f| f.rect),
            _ => None,
        };
        if let Some(fr_rect) = rect {
            let ui_rect = rusty_ui::Rect::new(
                fr_rect.x,
                fr_rect.y,
                u32::try_from(fr_rect.w.max(1)).unwrap_or(1),
                u32::try_from(fr_rect.h.max(1)).unwrap_or(1),
            );
            let click_n = self.click_streak;
            let elapsed = self.app.epoch.elapsed();
            let tf = &mut self.app.fields[field.idx()];
            tf.rect = ui_rect;
            with_font_engine(|fe| {
                let _ = tf.on_pointer_down(px, py, click_n, elapsed, fe);
            });
        }
    }

    fn select_storage_target(&mut self, idx: usize) {
        let selectable = match self.app.disk_mode {
            DiskInstallMode::WholeDisk => self
                .app
                .disks
                .get(idx)
                .is_some_and(TargetDiskInfo::is_selectable),
            DiskInstallMode::DualBoot => self
                .app
                .dualboot_targets
                .get(idx)
                .is_some_and(|t| t.selectable),
        };
        if selectable {
            match self.app.disk_mode {
                DiskInstallMode::WholeDisk => self.app.selected_disk_idx = Some(idx),
                DiskInstallMode::DualBoot => self.app.selected_dualboot_idx = Some(idx),
            }
            self.app.form_error = None;
            self.app.win.dirty = true;
            self.layout_cache = None;
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
    window.commit();

    let (wake_ping, wake_source) =
        ping::make_ping().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let (paste_ping, paste_source) =
        ping::make_ping().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let mut app = InstallerAppState::new(opts);
    app.set_wake_ping(move || wake_ping.ping());
    app.kick_initial_probes();

    let event_loop: EventLoop<'static, InstallerClient> =
        EventLoop::try_new().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let loop_handle = event_loop.handle();
    insert_event_sources(&loop_handle, &conn, queue, wake_source, paste_source)?;

    let no_gpu_env = std::env::var_os("RUSTYOS_NO_GPU").is_some_and(|v| v == "1");
    let use_gpu = !opts.no_gpu && !no_gpu_env;
    let data_device_manager = DataDeviceManagerState::bind(&globals, &qh).ok();

    let client = InstallerClient {
        conn: conn.clone(),
        qh: qh.clone(),
        loop_handle: loop_handle.clone(),
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        data_device_manager,
        data_device: None,
        clipboard: rusty_ui::WaylandClipboardBridge::new(),
        paste_ping,
        last_input_serial: 0,
        window: Some(window),
        gles: None,
        use_gpu,
        cpu_backbuf: Vec::new(),
        frame_rev: 0,
        _glass_mgr: glass_mgr,
        window_glass,
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
        pointer_pressed: false,
        click_streak: 1,
        last_click_at: None,
        hover_hit: None,
        app,
        layout_cache: None,
    };
    Ok((conn, client, event_loop))
}

fn insert_event_sources(
    loop_handle: &LoopHandle<'static, InstallerClient>,
    conn: &Connection,
    queue: EventQueue<InstallerClient>,
    wake_source: ping::PingSource,
    paste_source: ping::PingSource,
) -> Result<(), WaylandAppError> {
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
    loop_handle
        .insert_source(paste_source, |(), (), client| {
            if let Some(pasted) = client.clipboard.poll_received_paste() {
                client.app.handle_text_input(&pasted);
            }
        })
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;
    Ok(())
}

fn try_draw_frame_gles(
    client: &mut InstallerClient,
    layout: &InstallerLayout,
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
    let has_glass = client.has_glass();
    render_window_pixels(
        &mut client.cpu_backbuf,
        w,
        h,
        &client.app,
        layout,
        client.hover_hit,
        has_glass,
    );
    client.frame_rev = client.frame_rev.wrapping_add(1);
    let rev = client.frame_rev;
    let pixels: Arc<[u8]> = Arc::from(client.cpu_backbuf.as_slice());
    let mut dl = rusty_ui::UiDrawList::new();
    {
        let mut painter = rusty_ui::UiPainter::new(&mut dl);
        painter.draw_image(
            rusty_ui::Rect::new(0, 0, w, h),
            rev,
            w,
            h,
            pixels,
            rusty_ui::PixelFormat::Bgra8888,
        );
    }
    if let Some(win) = &client.window {
        win.set_window_geometry(0, 0, w, h);
        win.wl_surface().frame(&client.qh, win.wl_surface().clone());
    }
    let wi = i32::try_from(w).unwrap_or(1);
    let hi = i32::try_from(h).unwrap_or(1);
    let gles_res = if let Some(gles) = &mut client.gles {
        with_font_engine(|fe| gles.render_draw_list(&dl, fe, wi, hi, [0, 0, 0, 0]))
    } else {
        Ok(())
    };
    match gles_res {
        Ok(()) => {
            client.frame_sent_at = Some(Instant::now());
            true
        }
        Err(err) => {
            tracing::warn!(%err, "rusty-installer GLES2 swap failed; falling back to wl_shm");
            client.gles = None;
            client.use_gpu = false;
            false
        }
    }
}

fn draw_frame(client: &mut InstallerClient) -> Result<(), WaylandAppError> {
    let layout = client.current_layout();
    let w = client.width.max(1);
    let h = client.height.max(1);
    if client.window.is_none() {
        return Ok(());
    }
    if try_draw_frame_gles(client, &layout, w, h) {
        return Ok(());
    }

    let has_glass = client.has_glass();
    let Some(win) = &client.window else {
        return Ok(());
    };
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
        has_glass,
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
delegate_data_device!(InstallerClient);
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
        self.app.win.focused = configure.state.contains(WindowState::ACTIVATED);
        if self.use_gpu
            && self.gles.is_none()
            && let Some(win) = &self.window
        {
            match rusty_ui::GlesSurfaceRenderer::new(
                self.conn.backend().display_ptr().cast(),
                win.wl_surface(),
                clamped_w.cast_signed(),
                clamped_h.cast_signed(),
            ) {
                Ok(renderer) => {
                    self.gles = Some(renderer);
                }
                Err(err) => {
                    tracing::warn!(%err, "rusty-installer GLES2 unavailable; using CPU wl_shm fallback");
                    self.use_gpu = false;
                }
            }
        }
        self.configured = true;
        self.app.win.dirty = true;
    }
}

impl SeatHandler for InstallerClient {
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
                    if self.pointer_pressed
                        && let Some(field) = self.app.caret.focused_field
                    {
                        let changed = with_font_engine(|fe| {
                            self.app.fields[field.idx()].on_pointer_move(ev.position.0, fe)
                        });
                        if changed {
                            self.app.win.dirty = true;
                        }
                    }
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
                    self.pointer_pressed = true;
                    self.execute_left_click(ev.position.0, ev.position.1, serial);
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => {
                    self.pointer_pressed = false;
                    if let Some(field) = self.app.caret.focused_field {
                        self.app.fields[field.idx()].on_pointer_up();
                    }
                }
                _ => {}
            }
        }
    }
}

impl DataDeviceHandler for InstallerClient {
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

impl DataOfferHandler for InstallerClient {
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

impl DataSourceHandler for InstallerClient {
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
