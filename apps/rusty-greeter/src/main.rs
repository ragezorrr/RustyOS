//! RustyOS graphical `greetd` display manager greeter (`RUSTYOS-136`, ADR-0080).

mod greetd;

use std::path::PathBuf;
use std::time::Duration;

use rusty_ui::{
    ButtonStyle, InputModifiers, PushButton, Rect, TextField, TextInputKey, ThemeMode,
    ThemePalette, UiCanvas, UiDrawList, UiFontEngine, UiPainter, render_draw_list_to_canvas,
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

struct GreeterConfig {
    demo: bool,
    theme_mode: ThemeMode,
    default_user: String,
    session_cmd: Vec<String>,
}

fn parse_args() -> GreeterConfig {
    let mut cfg = GreeterConfig {
        demo: false,
        theme_mode: ThemeMode::Dark,
        default_user: std::env::var("USER").unwrap_or_else(|_| "rusty".to_owned()),
        session_cmd: vec!["rustyos-session".to_owned()],
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => cfg.demo = true,
            "--light" => cfg.theme_mode = ThemeMode::Light,
            "--dark" => cfg.theme_mode = ThemeMode::Dark,
            "--user" => {
                if let Some(u) = args.next() {
                    cfg.default_user = u;
                }
            }
            "--session-cmd" => {
                if let Some(cmd) = args.next() {
                    cfg.session_cmd = cmd.split_whitespace().map(ToOwned::to_owned).collect();
                }
            }
            _ => {}
        }
    }
    cfg
}

#[allow(clippy::struct_excessive_bools)]
struct GreeterApp {
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
    modifiers: Modifiers,
    cfg: GreeterConfig,
    fonts: UiFontEngine,
    user_field: TextField,
    pass_field: TextField,
    focus_password: bool,
    status_msg: String,
    auth_rx: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    dirty: bool,
}

impl GreeterApp {
    fn layout_rects(&self) -> (Rect, Rect, Rect) {
        let w = i32::try_from(self.width).unwrap_or(960);
        let h = i32::try_from(self.height).unwrap_or(600);
        let cx = w / 2;
        let cy = h / 2;
        let field_w = 280_u32;
        let user_rect = Rect::new(cx - 140, cy - 28, field_w, 34);
        let pass_rect = Rect::new(cx - 140, cy + 16, field_w, 34);
        let btn_rect = Rect::new(cx - 72, cy + 68, 144, 34);
        (user_rect, pass_rect, btn_rect)
    }

    fn submit_login(&mut self) {
        let user = self.user_field.buffer.as_str().trim().to_owned();
        let pass = self.pass_field.buffer.as_str().to_owned();
        if user.is_empty() {
            "Enter a username".clone_into(&mut self.status_msg);
            self.dirty = true;
            return;
        }
        if self.cfg.demo {
            if pass.is_empty() {
                "Demo mode: enter any password to sign in".clone_into(&mut self.status_msg);
            } else {
                self.status_msg = format!("Signed in as {user} (demo)");
                self.exit = true;
            }
            self.dirty = true;
            return;
        }
        if self.auth_rx.is_some() {
            return;
        }
        let Ok(sock) = std::env::var("GREETD_SOCK") else {
            "GREETD_SOCK not set (use --demo for preview)".clone_into(&mut self.status_msg);
            self.dirty = true;
            return;
        };
        "Signing in...".clone_into(&mut self.status_msg);
        self.dirty = true;
        let session_cmd = self.cfg.session_cmd.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_rx = Some(rx);
        let _ = std::thread::Builder::new()
            .name("rusty-greeter-auth".to_owned())
            .spawn(move || {
                let res = greetd::GreetdClient::connect(&PathBuf::from(sock))
                    .and_then(|mut c| c.authenticate_and_start(&user, &pass, session_cmd));
                let _ = tx.send(res);
            });
    }

    fn poll_auth_result(&mut self) {
        let outcome = match self
            .auth_rx
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv)
        {
            Some(Ok(res)) => Some(res),
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => Some(Err(
                "Authentication worker terminated unexpectedly".to_owned(),
            )),
            _ => None,
        };
        if let Some(res) = outcome {
            self.auth_rx = None;
            match res {
                Ok(()) => {
                    self.exit = true;
                }
                Err(err) => {
                    self.status_msg = err;
                    self.pass_field.buffer.set_text("");
                    self.dirty = true;
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured || !self.dirty {
            return;
        }
        let w = self.width.max(320);
        let h = self.height.max(240);
        let pal = ThemePalette::new(self.cfg.theme_mode == ThemeMode::Dark, 0);
        let bg = if self.cfg.theme_mode == ThemeMode::Dark {
            [24, 28, 38, 255]
        } else {
            [236, 240, 248, 255]
        };

        let (user_rect, pass_rect, btn_rect) = self.layout_rects();
        self.user_field.rect = user_rect;
        self.pass_field.rect = pass_rect;

        let card_rect = Rect::new(
            user_rect.x - 32,
            user_rect.y - 108,
            user_rect.width + 64,
            262,
        );
        let avatar_rect = Rect::new(
            card_rect.x + i32::try_from(card_rect.width.saturating_sub(56) / 2).unwrap_or(0),
            card_rect.y + 22,
            56,
            56,
        );
        let title_rect = Rect::new(card_rect.x, avatar_rect.y + 62, card_rect.width, 24);
        let initial = self
            .user_field
            .buffer
            .as_str()
            .chars()
            .next()
            .unwrap_or('R')
            .to_ascii_uppercase()
            .to_string();

        let mut list = UiDrawList::new();
        {
            let mut painter = UiPainter::new(&mut list);
            painter.fill_rect(Rect::new(0, 0, w, h), bg);
            painter.fill_rounded_rect(card_rect, 18.0, pal.card_bg);
            let card_border = if pal.is_dark {
                [255, 255, 255, 28]
            } else {
                [0, 0, 0, 24]
            };
            painter.stroke_rounded_rect(card_rect, 18.0, 1.0, card_border);
            painter.fill_rounded_rect(avatar_rect, 28.0, pal.accent);
            painter.draw_text_centered(
                &mut self.fonts,
                initial,
                avatar_rect,
                22.0,
                true,
                [255, 255, 255, 255],
            );
            painter.draw_text_centered(
                &mut self.fonts,
                "RustyOS",
                title_rect,
                15.0,
                true,
                pal.text_primary,
            );
            self.user_field.paint(
                &mut painter,
                &mut self.fonts,
                &pal,
                !self.focus_password,
                if self.focus_password { 0.0 } else { 1.0 },
                Duration::ZERO,
            );
            self.pass_field.paint(
                &mut painter,
                &mut self.fonts,
                &pal,
                self.focus_password,
                if self.focus_password { 1.0 } else { 0.0 },
                Duration::ZERO,
            );
            let btn = PushButton::new(btn_rect, ButtonStyle::Primary);
            btn.paint_labeled(&mut painter, &mut self.fonts, "Sign In", &pal, false);
            if !self.status_msg.is_empty() {
                let status_rect = Rect::new(
                    card_rect.x + 12,
                    btn_rect.y + 42,
                    card_rect.width.saturating_sub(24),
                    22,
                );
                painter.draw_text_centered(
                    &mut self.fonts,
                    self.status_msg.clone(),
                    status_rect,
                    11.5,
                    false,
                    pal.text_secondary,
                );
            }
        }

        let stride = i32::try_from(w * 4).unwrap_or(1280);
        let Ok((buffer, canvas_bytes)) = self.pool.create_buffer(
            i32::try_from(w).unwrap_or(960),
            i32::try_from(h).unwrap_or(600),
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
            i32::try_from(w).unwrap_or(960),
            i32::try_from(h).unwrap_or(600),
        );
        let _ = buffer.attach_to(surf);
        surf.commit();
        self.dirty = false;
    }
}

impl CompositorHandler for GreeterApp {
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
        self.draw(qh);
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

impl OutputHandler for GreeterApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for GreeterApp {
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
        self.width = configure.new_size.0.map_or(960, std::num::NonZeroU32::get);
        self.height = configure.new_size.1.map_or(600, std::num::NonZeroU32::get);
        self.configured = true;
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for GreeterApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
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
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for GreeterApp {
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
        if event.keysym == Keysym::Tab {
            self.focus_password = !self.focus_password;
            self.dirty = true;
            self.draw(qh);
            return;
        }
        if event.keysym == Keysym::Return || event.keysym == Keysym::KP_Enter {
            self.submit_login();
            self.draw(qh);
            return;
        }
        if event.keysym == Keysym::Escape && self.cfg.demo {
            self.exit = true;
            return;
        }
        let mut keys = Vec::new();
        if event.keysym == Keysym::BackSpace {
            keys.push(TextInputKey::Backspace);
        } else if event.keysym == Keysym::Delete {
            keys.push(TextInputKey::Delete);
        } else if event.keysym == Keysym::Left {
            keys.push(TextInputKey::Left);
        } else if event.keysym == Keysym::Right {
            keys.push(TextInputKey::Right);
        } else if event.keysym == Keysym::Home {
            keys.push(TextInputKey::Home);
        } else if event.keysym == Keysym::End {
            keys.push(TextInputKey::End);
        } else if let Some(text) = event.utf8.as_deref() {
            for ch in text.chars().filter(|c| !c.is_control()) {
                keys.push(TextInputKey::Char(ch));
            }
        }
        if !keys.is_empty() {
            let mods = InputModifiers {
                ctrl: self.modifiers.ctrl,
                shift: self.modifiers.shift,
                alt: self.modifiers.alt,
            };
            let mut clip = rusty_ui::MemoryClipboard::default();
            let field = if self.focus_password {
                &mut self.pass_field
            } else {
                &mut self.user_field
            };
            for k in keys {
                let _ = field.handle_key(k, mods, Duration::ZERO, &mut self.fonts, &mut clip);
            }
            self.dirty = true;
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

impl PointerHandler for GreeterApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            if let PointerEventKind::Press {
                button: BTN_LEFT, ..
            } = ev.kind
            {
                #[allow(clippy::cast_possible_truncation)]
                let (px, py) = (ev.position.0.round() as i32, ev.position.1.round() as i32);
                let (user_rect, pass_rect, btn_rect) = self.layout_rects();
                if user_rect.contains(px, py) {
                    self.focus_password = false;
                    self.dirty = true;
                } else if pass_rect.contains(px, py) {
                    self.focus_password = true;
                    self.dirty = true;
                } else if btn_rect.contains(px, py) {
                    self.submit_login();
                }
                self.draw(qh);
            }
        }
    }
}

impl ShmHandler for GreeterApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for GreeterApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(GreeterApp);
delegate_compositor!(GreeterApp);
delegate_output!(GreeterApp);
delegate_shm!(GreeterApp);
delegate_seat!(GreeterApp);
delegate_keyboard!(GreeterApp);
delegate_pointer!(GreeterApp);
delegate_xdg_shell!(GreeterApp);
delegate_xdg_window!(GreeterApp);

fn main() {
    let cfg = parse_args();
    let Ok(conn) = Connection::connect_to_env() else {
        eprintln!("rusty-greeter: failed to connect to Wayland display");
        std::process::exit(1);
    };
    let Ok((globals, mut event_queue)) = registry_queue_init(&conn) else {
        eprintln!("rusty-greeter: failed to initialize Wayland registry");
        std::process::exit(1);
    };
    let qh = event_queue.handle();
    let Ok(compositor) = CompositorState::bind(&globals, &qh) else {
        eprintln!("rusty-greeter: wl_compositor unavailable");
        std::process::exit(1);
    };
    let Ok(xdg_shell) = XdgShell::bind(&globals, &qh) else {
        eprintln!("rusty-greeter: xdg_wm_base unavailable");
        std::process::exit(1);
    };
    let Ok(shm) = Shm::bind(&globals, &qh) else {
        eprintln!("rusty-greeter: wl_shm unavailable");
        std::process::exit(1);
    };
    let Ok(pool) = SlotPool::new(960 * 600 * 4, &shm) else {
        eprintln!("rusty-greeter: failed to allocate shm pool");
        std::process::exit(1);
    };
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title("RustyOS Sign In");
    window.set_app_id("org.rustyos.greeter");
    window.commit();

    let mut user_field = TextField::new(Rect::new(0, 0, 280, 34), "Username");
    user_field.buffer.set_text(&cfg.default_user);
    let pass_field = TextField::password(Rect::new(0, 0, 280, 34), "Enter Password");

    let mut app = GreeterApp {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        width: 960,
        height: 600,
        configured: false,
        exit: false,
        keyboard: None,
        pointer: None,
        modifiers: Modifiers::default(),
        cfg,
        fonts: UiFontEngine::new(),
        user_field,
        pass_field,
        focus_password: true,
        status_msg: String::new(),
        auth_rx: None,
        dirty: true,
    };

    while !app.exit {
        if app.auth_rx.is_some() {
            let _ = event_queue.flush();
            if let Some(guard) = event_queue.prepare_read() {
                let _ = guard.read();
            }
            if event_queue.dispatch_pending(&mut app).is_err() {
                break;
            }
            app.poll_auth_result();
            if app.dirty && !app.exit {
                app.draw(&qh);
            }
            std::thread::sleep(Duration::from_millis(25));
        } else if event_queue.blocking_dispatch(&mut app).is_err() {
            break;
        }
    }
}
