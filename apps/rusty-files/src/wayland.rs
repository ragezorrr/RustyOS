//! Wayland CSD (`xdg_toplevel`) client event loop and SHM presentation for `rusty-files`
//! (`RUSTYOS-065`, ADR-0030).

use std::fmt;
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell, XdgSurface,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    backend::WaylandError,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::layout::{
    FilesHit, FilesLayout, PopupCommand, ResizeEdgeZone, compute_layout, hit_test, scroll_area_at,
};
use crate::render::render_window_pixels;
use crate::state::OpenPopupMenu;
use crate::state::{FilesAppState, ScrollTargetArea, StartupOptions};
use rusty_services::file_manager::UserFileLocations;

const DEFAULT_WIDTH: u32 = 1060;
const DEFAULT_HEIGHT: u32 = 660;

/// Wayland client error returned by `run`.
#[derive(Debug)]
pub(crate) enum WaylandAppError {
    Connect(wayland_client::ConnectError),
    Global(wayland_client::globals::GlobalError),
    Bind(wayland_client::globals::BindError),
    Dispatch(wayland_client::DispatchError),
    Backend(WaylandError),
    ShmPool(String),
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

struct WaylandClientData {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    _compositor_state: CompositorState,
    shm: Shm,
    _xdg_shell: XdgShell,
    pool: SlotPool,
    window: Option<Window>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mod_shift: bool,
    mod_ctrl: bool,
    width: u32,
    height: u32,
    configured: bool,
    pointer_pos: (f64, f64),
    last_click: Option<(Instant, FilesHit)>,
    last_title: String,
    app: FilesAppState,
    layout_cache: Option<FilesLayout>,
}

impl WaylandClientData {
    fn layout(&mut self) -> FilesLayout {
        let l = compute_layout(&self.app, self.width, self.height);
        self.layout_cache = Some(l.clone());
        l
    }

    fn execute_left_click(&mut self, px: f64, py: f64, serial: u32) {
        let layout = self.layout();
        let Some(hit) = hit_test(&layout, px, py) else {
            if self.app.popup.is_some() {
                self.app.popup = None;
                self.app.dirty = true;
            }
            return;
        };

        let now = Instant::now();
        let double_click = self.last_click.as_ref().is_some_and(|(t, prev)| {
            now.duration_since(*t) <= Duration::from_millis(400) && prev == &hit
        });
        self.last_click = if double_click {
            None
        } else {
            Some((now, hit.clone()))
        };

        match hit {
            FilesHit::ResizeEdge(zone) => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.resize(seat, serial, map_resize_edge(zone));
                }
            }
            FilesHit::WindowDrag => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.move_(seat, serial);
                }
            }
            FilesHit::TrafficClose => self.app.request_quit(),
            FilesHit::TrafficMinimize => {
                if let Some(win) = &self.window {
                    win.set_minimized();
                }
            }
            FilesHit::TrafficMaximize => self.toggle_maximize(),
            other => self.handle_app_hit(other, double_click),
        }
    }

    fn toggle_maximize(&mut self) {
        let Some(win) = &self.window else {
            return;
        };
        let next = !self.app.is_maximized();
        if next {
            win.set_maximized();
        } else {
            win.unset_maximized();
        }
        self.app.set_maximized(next);
    }

    fn handle_app_hit(&mut self, hit: FilesHit, double_click: bool) {
        match hit {
            FilesHit::NavBack => self.app.go_back(),
            FilesHit::NavForward => self.app.go_forward(),
            FilesHit::ViewMode(mode) => self.app.set_view_mode(mode),
            FilesHit::SortButton => self.toggle_popup(OpenPopupMenu::SortMenu),
            FilesHit::ActionMoreMenu => self.toggle_popup(OpenPopupMenu::ActionsMenu),
            FilesHit::ActionPrimary => {
                if self.app.is_trash_view() {
                    if let Some(entry) = self.app.selected_entry().cloned() {
                        self.app.restore_entry(&entry);
                    }
                } else {
                    self.app.action_new_folder();
                }
            }
            FilesHit::ActionTagPicker => {
                let target_path = self
                    .app
                    .selected_entry()
                    .map(|e| e.path.clone())
                    .or_else(|| self.app.active_writable_directory());
                if let Some(path) = target_path {
                    self.toggle_popup(OpenPopupMenu::TagPicker { target_path: path });
                }
            }
            FilesHit::SearchToggle => {
                self.app.search_active =
                    !(self.app.search_active && self.app.search_query.is_empty());
                self.app.dirty = true;
            }
            FilesHit::SearchClear => self.app.set_search_query(String::new()),
            FilesHit::EmptyTrashBanner => self.app.action_empty_trash(),
            FilesHit::ThemeFooterToggle => self.app.toggle_theme(),
            FilesHit::SidebarItem(loc) => self.app.navigate_sidebar(loc, true),
            FilesHit::ColumnRow { col_idx, row_idx } => {
                self.app.select_entry(col_idx, row_idx, double_click);
            }
            FilesHit::ColumnEmpty { col_idx } => {
                self.app.popup = None;
                self.app.active_column = col_idx;
                if let Some(col) = self.app.columns.get_mut(col_idx) {
                    col.selected_index = None;
                }
                self.app.columns.truncate(col_idx + 1);
                self.app.dirty = true;
            }
            FilesHit::ListHeaderSort(field) => self.app.set_sort(field),
            FilesHit::ListRow { row_idx } => self.app.select_entry(0, row_idx, double_click),
            FilesHit::ListEmpty => {
                self.app.popup = None;
                if let Some(col) = self.app.columns.first_mut() {
                    col.selected_index = None;
                }
                self.app.dirty = true;
            }
            FilesHit::PreviewOpen => self.app.activate_selected(),
            FilesHit::PreviewTrashOrRestore => self.app.action_move_selected_to_trash(),
            FilesHit::PreviewTag { path, color } => self.app.toggle_tag_on_path(&path, color),
            FilesHit::PopupItem(cmd) => self.execute_popup_command(cmd),
            FilesHit::ResizeEdge(_)
            | FilesHit::WindowDrag
            | FilesHit::TrafficClose
            | FilesHit::TrafficMinimize
            | FilesHit::TrafficMaximize => {}
        }
    }

    fn execute_right_click(&mut self, px: f64, py: f64) {
        let layout = self.layout();
        let Some(hit) = hit_test(&layout, px, py) else {
            self.app.popup = None;
            self.app.dirty = true;
            return;
        };

        let (entry, dir) = match hit {
            FilesHit::ColumnRow { col_idx, row_idx } => {
                self.app.select_entry(col_idx, row_idx, false);
                let e = self
                    .app
                    .columns
                    .get(col_idx)
                    .and_then(|c| c.visible_entries.get(row_idx))
                    .cloned();
                (e, self.app.active_writable_directory())
            }
            FilesHit::ListRow { row_idx } => {
                self.app.select_entry(0, row_idx, false);
                let e = self
                    .app
                    .columns
                    .first()
                    .and_then(|c| c.visible_entries.get(row_idx))
                    .cloned();
                (e, self.app.active_writable_directory())
            }
            FilesHit::ColumnEmpty { col_idx } => {
                self.app.active_column = col_idx;
                (None, self.app.active_writable_directory())
            }
            FilesHit::ListEmpty => (None, self.app.active_writable_directory()),
            _ => {
                self.app.popup = None;
                self.app.dirty = true;
                return;
            }
        };

        #[allow(clippy::cast_possible_truncation)]
        let (x, y) = (px.round() as i32, py.round() as i32);
        self.app.popup = Some(OpenPopupMenu::ContextMenu {
            x,
            y,
            target_entry: entry,
            target_dir: dir,
        });
        self.app.dirty = true;
    }

    fn toggle_popup(&mut self, menu: OpenPopupMenu) {
        if self.app.popup.as_ref() == Some(&menu) {
            self.app.popup = None;
        } else {
            self.app.popup = Some(menu);
        }
        self.app.dirty = true;
    }

    fn execute_popup_command(&mut self, cmd: PopupCommand) {
        self.app.popup = None;
        match cmd {
            PopupCommand::SortBy(field) => self.app.set_sort(field),
            PopupCommand::SetSortOrder(order) => self.app.set_sort_order(order),
            PopupCommand::NewFolder => self.app.action_new_folder(),
            PopupCommand::ToggleShowHidden => self.app.toggle_show_hidden(),
            PopupCommand::ToggleTheme => self.app.toggle_theme(),
            PopupCommand::OpenEntry(entry) => {
                if entry.is_navigable_dir() && !self.app.is_trash_view() {
                    self.app.activate_selected();
                } else {
                    self.app.launch_entry(&entry);
                }
            }
            PopupCommand::MoveToTrash(entry) => self.app.trash_entry(&entry),
            PopupCommand::RestoreEntry(entry) => self.app.restore_entry(&entry),
            PopupCommand::EmptyTrash => self.app.action_empty_trash(),
            PopupCommand::ToggleTag { path, color } => self.app.toggle_tag_on_path(&path, color),
        }
    }

    fn handle_axis_scroll(&mut self, px: f64, py: f64, vertical_delta: f64) {
        if vertical_delta.abs() < 0.01 {
            return;
        }
        let layout = self.layout();
        let Some(area) = scroll_area_at(&layout, px, py) else {
            return;
        };
        let viewport_h = match area {
            ScrollTargetArea::Sidebar => layout.sidebar_clip_rect.h,
            ScrollTargetArea::List => layout.content_rect.h - 28,
            ScrollTargetArea::Column(_) => layout.content_rect.h,
        };
        #[allow(clippy::cast_possible_truncation)]
        let step = (vertical_delta * 2.2).round() as i32;
        if step != 0 {
            self.app.scroll_vertical_at(area, step, viewport_h.max(60));
        }
    }
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

/// Connects to Wayland compositor, maps the Golden Gate CSD window, and runs the event loop.
///
/// # Errors
/// Returns `WaylandAppError` on Wayland connection, protocol binding, or rendering failure.
pub(crate) fn run(options: &StartupOptions) -> Result<(), WaylandAppError> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let compositor_state = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let pool = SlotPool::new((DEFAULT_WIDTH * DEFAULT_HEIGHT * 4) as usize, &shm)
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;

    let locations = UserFileLocations::detect();
    let app = FilesAppState::new(locations, options);
    let initial_title = app.window_title.clone();

    let surface = compositor_state.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestClient, &qh);
    window.set_title(&initial_title);
    window.set_app_id("org.rustyos.Files");
    window.set_min_size(Some((720, 440)));
    window.commit();

    let mut data = WaylandClientData {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        _compositor_state: compositor_state,
        shm,
        _xdg_shell: xdg_shell,
        pool,
        window: Some(window),
        seat: None,
        pointer: None,
        keyboard: None,
        mod_shift: false,
        mod_ctrl: false,
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        configured: false,
        pointer_pos: (0.0, 0.0),
        last_click: None,
        last_title: initial_title,
        app,
        layout_cache: None,
    };

    while data.app.is_running() {
        let _ = queue.dispatch_pending(&mut data)?;
        let _ = data.app.poll_background_loads();

        if data.app.window_title != data.last_title
            && let Some(win) = &data.window
        {
            let title = format!("{} - RustyOS Files", data.app.window_title);
            win.set_title(&title);
            data.last_title.clone_from(&data.app.window_title);
        }

        if data.configured && data.app.dirty {
            draw_frame(&mut data)?;
            data.app.dirty = false;
        }
        conn.flush()?;

        if let Some(guard) = queue.prepare_read() {
            match guard.read() {
                Ok(_) => {
                    let _ = queue.dispatch_pending(&mut data)?;
                }
                Err(WaylandError::Io(ref io_err))
                    if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    std::thread::sleep(Duration::from_millis(12));
                }
                Err(err) => return Err(err.into()),
            }
        } else {
            std::thread::sleep(Duration::from_millis(8));
        }
    }
    Ok(())
}

#[allow(clippy::cast_possible_wrap)]
fn draw_frame(data: &mut WaylandClientData) -> Result<(), WaylandAppError> {
    let layout = data.layout();
    let Some(win) = &data.window else {
        return Ok(());
    };
    let w = data.width;
    let h = data.height;
    let stride = (w * 4) as i32;
    let (buffer, canvas) = data
        .pool
        .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;

    render_window_pixels(canvas, w, h, &data.app, &layout);
    win.set_window_geometry(0, 0, w, h);
    buffer
        .attach_to(win.wl_surface())
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
    win.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
    win.commit();
    Ok(())
}

impl ProvidesRegistryState for WaylandClientData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(WaylandClientData);
delegate_output!(WaylandClientData);
delegate_compositor!(WaylandClientData);
delegate_shm!(WaylandClientData);
delegate_seat!(WaylandClientData);
delegate_pointer!(WaylandClientData);
delegate_xdg_shell!(WaylandClientData);
delegate_xdg_window!(WaylandClientData);

impl OutputHandler for WaylandClientData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl CompositorHandler for WaylandClientData {
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
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
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

impl ShmHandler for WaylandClientData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for WaylandClientData {
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
        cap: Capability,
    ) {
        if self.seat.is_none() {
            self.seat = Some(seat.clone());
        }
        if cap == Capability::Pointer
            && self.pointer.is_none()
            && let Ok(ptr) = self.seat_state.get_pointer(qh, &seat)
        {
            self.pointer = Some(ptr);
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(seat.get_keyboard(qh, ()));
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Pointer
            && let Some(ptr) = self.pointer.take()
        {
            ptr.release();
        }
        if cap == Capability::Keyboard
            && let Some(kbd) = self.keyboard.take()
        {
            kbd.release();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl WindowHandler for WaylandClientData {
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
        let new_w = configure
            .new_size
            .0
            .map_or(DEFAULT_WIDTH, std::num::NonZeroU32::get)
            .max(480);
        let new_h = configure
            .new_size
            .1
            .map_or(DEFAULT_HEIGHT, std::num::NonZeroU32::get)
            .max(360);
        self.width = new_w;
        self.height = new_h;
        self.app.set_maximized(configure.is_maximized());
        self.configured = true;
        self.app.dirty = true;
    }
}

impl PointerHandler for WaylandClientData {
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
                PointerEventKind::Press { button, serial, .. } => {
                    let (px, py) = ev.position;
                    if button == BTN_LEFT {
                        self.execute_left_click(px, py, serial);
                    } else if button == BTN_RIGHT {
                        self.execute_right_click(px, py);
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let delta = if vertical.discrete != 0 {
                        f64::from(vertical.discrete) * 14.0
                    } else {
                        vertical.absolute
                    };
                    self.handle_axis_scroll(ev.position.0, ev.position.1, delta);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for WaylandClientData {
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
                let (key_name, utf8_opt) = decode_evdev_key(key, state.mod_shift);
                let mut utf8_buf = [0_u8; 4];
                let text = utf8_opt.map(|ch| ch.encode_utf8(&mut utf8_buf) as &str);
                state.app.handle_key_input(key_name, text, state.mod_ctrl);
            }
            _ => {}
        }
    }
}

const fn decode_evdev_key(evdev_code: u32, shift: bool) -> (&'static str, Option<char>) {
    match evdev_code {
        1 => ("Escape", None),
        14 => ("BackSpace", None),
        28 | 96 => ("Return", None),
        103 => ("Up", None),
        105 => ("Left", None),
        106 => ("Right", None),
        108 => ("Down", None),
        111 => ("Delete", None),
        33 => ("f", Some(if shift { 'F' } else { 'f' })),
        49 => ("n", Some(if shift { 'N' } else { 'n' })),
        2..=11 => {
            let digits = if shift {
                ['!', '@', '#', '$', '%', '^', '&', '*', '(', ')']
            } else {
                ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0']
            };
            ("", Some(digits[(evdev_code - 2) as usize]))
        }
        12 => ("", Some(if shift { '_' } else { '-' })),
        13 => ("", Some(if shift { '+' } else { '=' })),
        16..=25 => {
            let row = b"qwertyuiop";
            let ch = row[(evdev_code - 16) as usize] as char;
            ("", Some(if shift { ch.to_ascii_uppercase() } else { ch }))
        }
        30..=38 => {
            let row = b"asdfghjkl";
            let ch = row[(evdev_code - 30) as usize] as char;
            ("", Some(if shift { ch.to_ascii_uppercase() } else { ch }))
        }
        44..=50 => {
            let row = b"zxcvbnm";
            let ch = row[(evdev_code - 44) as usize] as char;
            ("", Some(if shift { ch.to_ascii_uppercase() } else { ch }))
        }
        51 => ("", Some(if shift { '<' } else { ',' })),
        52 => ("", Some(if shift { '>' } else { '.' })),
        53 => ("", Some(if shift { '?' } else { '/' })),
        57 => ("", Some(' ')),
        _ => ("", None),
    }
}
