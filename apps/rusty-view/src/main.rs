//! RustyOS first-party native image viewer `rusty-view` (`RUSTYOS-173`, ADR-0102).

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

mod document;
mod hud;
mod playlist;
mod state;
mod toolbar;
mod viewport;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use rusty_ui::{
    ThemeMode, TrafficLightButtons, TrafficLightKind, UiCanvas, UiDrawList, UiFontEngine,
    UiPainter, WindowHitTarget, render_draw_list_to_canvas,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
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
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use state::{DEFAULT_VIEWER_HEIGHT, DEFAULT_VIEWER_WIDTH, ViewerState};

const MIN_VIEWER_WIDTH: u32 = 400;
const MIN_VIEWER_HEIGHT: u32 = 300;

#[derive(Debug, Clone)]
struct ViewerConfig {
    target_path: Option<PathBuf>,
    test_mode: bool,
    theme_mode: ThemeMode,
    fullscreen: bool,
}

fn parse_args() -> Result<ViewerConfig, String> {
    let mut cfg = ViewerConfig {
        target_path: None,
        test_mode: false,
        theme_mode: ThemeMode::Dark,
        fullscreen: false,
    };

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--test" => cfg.test_mode = true,
            "--light" => cfg.theme_mode = ThemeMode::Light,
            "--dark" => cfg.theme_mode = ThemeMode::Dark,
            "--fullscreen" => cfg.fullscreen = true,
            "--help" | "-h" => {
                println!(
                    "RustyOS Native Image Viewer\n\n\
                     Usage: rusty-view [OPTIONS] [IMAGE_OR_DIRECTORY_PATH]\n\n\
                     Options:\n  \
                       --test         Run headless verification and exit\n  \
                       --light        Force light theme\n  \
                       --dark         Force dark theme (default)\n  \
                       --fullscreen   Start in fullscreen mode\n  \
                       --help, -h     Show this help message\n\n\
                     Keyboard Shortcuts:\n  \
                       Right / Down / Space       Next image in folder\n  \
                       Left / Up / Backspace      Previous image in folder\n  \
                       + / =                      Zoom in\n  \
                       - / _                      Zoom out\n  \
                       0                          Actual size (100%)\n  \
                       9 / F                      Fit to window\n  \
                       R                          Rotate clockwise 90°\n  \
                       Shift+R / L                Rotate counter-clockwise 90°\n  \
                       H                          Flip horizontally\n  \
                       I                          Toggle metadata HUD\n  \
                       F11                        Toggle fullscreen\n  \
                       Escape / Q / Ctrl+Q        Quit"
                );
                std::process::exit(0);
            }
            other if !other.starts_with('-') && cfg.target_path.is_none() => {
                cfg.target_path = Some(PathBuf::from(other));
            }
            other => return Err(format!("Unknown argument '{other}'")),
        }
    }

    Ok(cfg)
}

struct ViewerApp {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    width: u32,
    height: u32,
    configured: bool,
    exit: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    seat: Option<wl_seat::WlSeat>,
    modifiers: Modifiers,
    last_serial: u32,
    pointer_pos: (f64, f64),
    fonts: UiFontEngine,
    state: ViewerState,
    is_maximized: bool,
}

impl ViewerApp {
    fn toggle_maximize(&mut self) {
        if self.is_maximized {
            self.window.unset_maximized();
            self.is_maximized = false;
        } else {
            self.window.set_maximized();
            self.is_maximized = true;
        }
        self.state.dirty = true;
    }

    fn toggle_fullscreen(&mut self) {
        if self.state.is_fullscreen {
            self.window.unset_fullscreen();
            self.state.is_fullscreen = false;
        } else {
            self.window.set_fullscreen(None);
            self.state.is_fullscreen = true;
        }
        self.state.dirty = true;
    }

    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured {
            return;
        }

        let w = self.width.max(MIN_VIEWER_WIDTH);
        let h = self.height.max(MIN_VIEWER_HEIGHT);

        let _ = self.state.toolbar.update_visibility();

        let mut list = UiDrawList::new();
        {
            let mut painter = UiPainter::new(&mut list);
            self.state.paint(&mut painter, &mut self.fonts, w, h);
        }

        let stride = i32::try_from(w * 4).unwrap_or(3600);
        let Ok((buffer, canvas_bytes)) = self.pool.create_buffer(
            i32::try_from(w).unwrap_or(900),
            i32::try_from(h).unwrap_or(650),
            stride,
            wl_shm::Format::Argb8888,
        ) else {
            return;
        };

        let mut canvas = UiCanvas::new(
            canvas_bytes,
            w,
            h,
            (w * 4) as usize,
            rusty_ui::PixelFormat::Bgra8888,
        );
        render_draw_list_to_canvas(&list, &mut canvas, &mut self.fonts);

        let surf = self.window.wl_surface();
        surf.damage_buffer(
            0,
            0,
            i32::try_from(w).unwrap_or(900),
            i32::try_from(h).unwrap_or(650),
        );
        let _ = buffer.attach_to(surf);
        surf.commit();
        self.state.dirty = false;
    }
}

impl CompositorHandler for ViewerApp {
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
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if self.state.dirty {
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

impl OutputHandler for ViewerApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for ViewerApp {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        let new_w = w.map_or(DEFAULT_VIEWER_WIDTH, |v| v.get().max(MIN_VIEWER_WIDTH));
        let new_h = h.map_or(DEFAULT_VIEWER_HEIGHT, |v| v.get().max(MIN_VIEWER_HEIGHT));

        self.width = new_w;
        self.height = new_h;
        self.state.frame.buttons = TrafficLightButtons::default_csd_for_width(new_w);

        if self.state.viewport.is_fitting {
            self.state.action_fit(new_w, new_h);
        }

        self.configured = true;
        self.state.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for ViewerApp {
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

impl KeyboardHandler for ViewerApp {
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
        let sym = event.keysym;
        let w = self.width;
        let h = self.height;

        match sym {
            Keysym::Escape | Keysym::q | Keysym::Q => {
                self.exit = true;
                return;
            }
            Keysym::F11 => {
                self.toggle_fullscreen();
            }
            Keysym::plus | Keysym::equal | Keysym::KP_Add => {
                self.state.action_zoom_in(w, h);
            }
            Keysym::minus | Keysym::underscore | Keysym::KP_Subtract => {
                self.state.action_zoom_out(w, h);
            }
            Keysym::_0 | Keysym::KP_0 => {
                self.state.action_actual_size();
            }
            Keysym::_9 | Keysym::KP_9 | Keysym::f | Keysym::F => {
                self.state.action_fit(w, h);
            }
            Keysym::r | Keysym::R => {
                if self.modifiers.shift {
                    self.state.action_rotate_ccw(w, h);
                } else {
                    self.state.action_rotate_cw(w, h);
                }
            }
            Keysym::l | Keysym::L => {
                self.state.action_rotate_ccw(w, h);
            }
            Keysym::h | Keysym::H => {
                self.state.action_flip_h();
            }
            Keysym::Right | Keysym::Down | Keysym::Page_Down | Keysym::space => {
                self.state.action_next(w, h);
            }
            Keysym::Left | Keysym::Up | Keysym::Page_Up | Keysym::BackSpace => {
                self.state.action_prev(w, h);
            }
            Keysym::i | Keysym::I => {
                self.state.action_toggle_info();
            }
            _ => {
                if self.modifiers.ctrl {
                    match sym {
                        Keysym::q => self.exit = true,
                        Keysym::r => self.state.action_rotate_cw(w, h),
                        _ => {}
                    }
                }
            }
        }

        self.state.dirty = true;
        self.draw(qh);
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

impl PointerHandler for ViewerApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let w = self.width;
        let h = self.height;

        for ev in events {
            match ev.kind {
                PointerEventKind::Enter { serial } => {
                    self.last_serial = serial;
                    self.pointer_pos = ev.position;
                    self.state
                        .on_pointer_moved(ev.position.0, ev.position.1, w, h);
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_pos = ev.position;
                    self.state
                        .on_pointer_moved(ev.position.0, ev.position.1, w, h);
                }
                PointerEventKind::Press { button, serial, .. } => {
                    self.last_serial = serial;
                    self.pointer_pos = ev.position;
                    if button == BTN_LEFT {
                        let target = self
                            .state
                            .on_pointer_down(ev.position.0, ev.position.1, w, h);
                        if target == WindowHitTarget::TitlebarDrag
                            && !self.state.is_fullscreen
                            && let Some(seat) = &self.seat
                        {
                            self.window.move_(seat, serial);
                        }
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if button == BTN_LEFT {
                        let traffic =
                            self.state
                                .on_pointer_up(self.pointer_pos.0, self.pointer_pos.1, w, h);
                        match traffic {
                            Some(TrafficLightKind::Close) => {
                                self.exit = true;
                                return;
                            }
                            Some(TrafficLightKind::Minimize) => {
                                self.window.set_minimized();
                            }
                            Some(TrafficLightKind::Maximize) => {
                                self.toggle_maximize();
                            }
                            None => {}
                        }
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let delta = if vertical.discrete != 0 {
                        f64::from(-vertical.discrete) * 15.0
                    } else {
                        -vertical.absolute
                    };
                    if delta.abs() > 0.01 {
                        self.state
                            .on_wheel(delta, self.pointer_pos.0, self.pointer_pos.1, w, h);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    self.state.toolbar.is_pointer_inside = false;
                    self.state.dirty = true;
                }
            }
        }

        if self.state.dirty {
            self.draw(qh);
        }
    }
}

impl ShmHandler for ViewerApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for ViewerApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(ViewerApp);
delegate_compositor!(ViewerApp);
delegate_output!(ViewerApp);
delegate_shm!(ViewerApp);
delegate_seat!(ViewerApp);
delegate_keyboard!(ViewerApp);
delegate_pointer!(ViewerApp);
delegate_xdg_shell!(ViewerApp);
delegate_xdg_window!(ViewerApp);

fn run_headless_tests() -> ExitCode {
    println!("rusty-view: running headless verification mode...");

    let ppm_data = b"P6\n4 4\n255\n\
        \xFF\x00\x00\x00\xFF\x00\x00\x00\xFF\xFF\xFF\x00\
        \x00\xFF\xFF\xFF\x00\xFF\x80\x80\x80\x20\x20\x20\
        \xFF\xFF\x00\x00\xFF\xFF\xFF\x00\xFF\x40\x40\x40\
        \x10\x20\x30\x40\x50\x60\x70\x80\x90\xA0\xB0\xC0";

    let temp_dir = std::env::temp_dir();
    let temp_img_path = temp_dir.join("rusty_view_headless_test.ppm");
    if let Err(e) = std::fs::write(&temp_img_path, ppm_data) {
        eprintln!("rusty-view test: failed to write test PPM: {e}");
        return ExitCode::FAILURE;
    }

    let mut state = ViewerState::new(Some(&temp_img_path), 800, 600, ThemeMode::Dark);
    let Some(doc) = &state.document else {
        eprintln!("rusty-view test: document should be loaded");
        return ExitCode::FAILURE;
    };
    assert_eq!(doc.width, 4);
    assert_eq!(doc.height, 4);
    println!(
        "rusty-view test: loaded sample PPM ({}x{}) successfully",
        doc.width, doc.height
    );

    state.action_rotate_cw(800, 600);
    let rot = state.document.as_ref().map_or(0, |d| d.rotation_deg);
    assert_eq!(rot, 90);
    state.action_zoom_in(800, 600);
    state.action_actual_size();
    state.action_fit(800, 600);
    state.action_toggle_info();
    assert!(state.hud.is_visible);
    println!("rusty-view test: transformations and actions validated");

    let mut fonts = UiFontEngine::new();
    let mut draw_list = UiDrawList::new();
    {
        let mut painter = UiPainter::new(&mut draw_list);
        state.paint(&mut painter, &mut fonts, 800, 600);
    }
    assert!(
        !draw_list.commands.is_empty(),
        "draw list should contain render commands"
    );

    let mut canvas_buf = vec![0u8; 800 * 600 * 4];
    let mut canvas = UiCanvas::new(
        &mut canvas_buf,
        800,
        600,
        800 * 4,
        rusty_ui::PixelFormat::Bgra8888,
    );
    render_draw_list_to_canvas(&draw_list, &mut canvas, &mut fonts);
    println!(
        "rusty-view test: rendered frame to canvas successfully ({} commands)",
        draw_list.commands.len()
    );

    let _ = std::fs::remove_file(temp_img_path);

    println!("rusty-view: headless test mode passed successfully");
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rusty-view: {e}");
            return ExitCode::FAILURE;
        }
    };

    if cfg.test_mode {
        return run_headless_tests();
    }

    let Ok(conn) = Connection::connect_to_env() else {
        eprintln!(
            "rusty-view: failed to connect to Wayland display (WAYLAND_DISPLAY not set or unavailable)"
        );
        eprintln!("Hint: use --test for headless verification without a Wayland compositor");
        return ExitCode::FAILURE;
    };

    let Ok((globals, mut event_queue)) = registry_queue_init(&conn) else {
        eprintln!("rusty-view: failed to initialize Wayland registry");
        return ExitCode::FAILURE;
    };

    let qh = event_queue.handle();
    let Ok(compositor) = CompositorState::bind(&globals, &qh) else {
        eprintln!("rusty-view: wl_compositor unavailable");
        return ExitCode::FAILURE;
    };
    let Ok(xdg_shell) = XdgShell::bind(&globals, &qh) else {
        eprintln!("rusty-view: xdg_wm_base unavailable");
        return ExitCode::FAILURE;
    };
    let Ok(shm) = Shm::bind(&globals, &qh) else {
        eprintln!("rusty-view: wl_shm unavailable");
        return ExitCode::FAILURE;
    };
    let Ok(pool) = SlotPool::new(
        (DEFAULT_VIEWER_WIDTH * DEFAULT_VIEWER_HEIGHT * 4) as usize,
        &shm,
    ) else {
        eprintln!("rusty-view: failed to allocate shm pool");
        return ExitCode::FAILURE;
    };

    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title("Image Viewer");
    window.set_app_id("org.rustyos.view");
    window.set_min_size(Some((MIN_VIEWER_WIDTH, MIN_VIEWER_HEIGHT)));
    if cfg.fullscreen {
        window.set_fullscreen(None);
    }
    window.commit();

    let mut state = ViewerState::new(
        cfg.target_path.as_deref(),
        DEFAULT_VIEWER_WIDTH,
        DEFAULT_VIEWER_HEIGHT,
        cfg.theme_mode,
    );
    state.is_fullscreen = cfg.fullscreen;

    let mut app = ViewerApp {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        width: DEFAULT_VIEWER_WIDTH,
        height: DEFAULT_VIEWER_HEIGHT,
        configured: false,
        exit: false,
        keyboard: None,
        pointer: None,
        seat: None,
        modifiers: Modifiers::default(),
        last_serial: 0,
        pointer_pos: (0.0, 0.0),
        fonts: UiFontEngine::new(),
        state,
        is_maximized: false,
    };

    while !app.exit {
        let _ = event_queue.flush();
        if let Some(guard) = event_queue.prepare_read() {
            let _ = guard.read();
        }
        if event_queue.dispatch_pending(&mut app).is_err() {
            break;
        }

        if app.state.dirty && !app.exit {
            app.draw(&qh);
        }

        std::thread::sleep(Duration::from_millis(4));
    }

    ExitCode::SUCCESS
}
