//! RustyOS graphical `PolicyKit` authentication agent (`RUSTYOS-172`, ADR-0101).

mod dialog;
#[cfg(test)]
mod tests;

use std::time::Duration;

use dialog::{DialogAction, PolkitDialogState};
use rusty_services::{PolkitAgentHandle, PolkitEvent, SecretText};
use rusty_ui::{
    InputModifiers, MemoryClipboard, Rect, TextInputKey, ThemeMode, ThemePalette, UiCanvas,
    UiDrawList, UiFontEngine, UiPainter, render_draw_list_to_canvas,
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

const DEFAULT_WIDTH: u32 = 420;
const DEFAULT_HEIGHT: u32 = 240;

struct PolkitConfig {
    demo: bool,
    test_mode: bool,
    theme_mode: ThemeMode,
}

fn parse_args() -> PolkitConfig {
    let mut cfg = PolkitConfig {
        demo: false,
        test_mode: false,
        theme_mode: ThemeMode::Dark,
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--demo" => cfg.demo = true,
            "--test" => cfg.test_mode = true,
            "--light" => cfg.theme_mode = ThemeMode::Light,
            "--dark" => cfg.theme_mode = ThemeMode::Dark,
            "--help" | "-h" => {
                println!(
                    "RustyOS PolicyKit Authentication Agent\n\n\
                     Usage: rusty-polkit [OPTIONS]\n\n\
                     Options:\n  \
                       --demo       Launch interactive preview dialog immediately\n  \
                       --test       Run sanity checks and exit\n  \
                       --light      Force light theme\n  \
                       --dark       Force dark theme (default)\n  \
                       --help, -h   Show this help message"
                );
                std::process::exit(0);
            }
            _ => {}
        }
    }
    cfg
}

#[allow(clippy::struct_excessive_bools)]
struct PolkitApp {
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
    cfg: PolkitConfig,
    fonts: UiFontEngine,
    dialog: Option<PolkitDialogState>,
    agent: PolkitAgentHandle,
    clipboard: MemoryClipboard,
    dirty: bool,
}

impl PolkitApp {
    fn poll_agent_events(&mut self, qh: &QueueHandle<Self>) {
        for ev in self.agent.drain_events() {
            match ev {
                PolkitEvent::BeginPrompt(req) => {
                    let mut dlg = PolkitDialogState::new(req);
                    dlg.update_layout(self.width, self.height);
                    self.dialog = Some(dlg);
                    self.dirty = true;
                    self.draw(qh);
                }
                PolkitEvent::PromptError { cookie, message } => {
                    if let Some(dlg) = self.dialog.as_mut()
                        && dlg.cookie == cookie
                    {
                        dlg.set_error(message);
                        self.dirty = true;
                        self.draw(qh);
                    }
                }
                PolkitEvent::ClosePrompt { cookie } => {
                    if let Some(dlg) = &self.dialog
                        && dlg.cookie == cookie
                    {
                        self.dialog = None;
                        self.dirty = true;
                        self.draw(qh);
                        if self.cfg.demo {
                            self.exit = true;
                        }
                    }
                }
            }
        }
    }

    fn submit_action(&mut self, action: DialogAction, qh: &QueueHandle<Self>) {
        match action {
            DialogAction::Submit(secret) => {
                if let Some(dlg) = self.dialog.as_mut() {
                    dlg.set_checking(true);
                    if self.cfg.demo {
                        dlg.status_msg = Some("Авторизация успешна (демо)".to_owned());
                        self.dirty = true;
                        self.draw(qh);
                        self.exit = true;
                        return;
                    }
                    self.agent.submit_password(dlg.cookie.clone(), secret);
                    self.dirty = true;
                    self.draw(qh);
                }
            }
            DialogAction::Cancel => {
                if let Some(dlg) = self.dialog.take() {
                    if !self.cfg.demo {
                        self.agent.cancel(dlg.cookie);
                    }
                    self.dirty = true;
                    self.draw(qh);
                    if self.cfg.demo {
                        self.exit = true;
                    }
                }
            }
        }
    }

    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured {
            return;
        }

        let w = self.width.max(360);
        let h = self.height.max(220);
        let pal = ThemePalette::new(self.cfg.theme_mode == ThemeMode::Dark, 0);

        let mut list = UiDrawList::new();
        {
            let mut painter = UiPainter::new(&mut list);
            if let Some(dlg) = self.dialog.as_mut() {
                dlg.update_layout(w, h);
                dlg.paint(&mut painter, &mut self.fonts, &pal, w, h);
            } else {
                // Window is hidden / blank when no authentication prompt is active
                let blank_bg = if pal.is_dark {
                    [20, 22, 28, 0]
                } else {
                    [245, 246, 250, 0]
                };
                painter.fill_rect(Rect::new(0, 0, w, h), blank_bg);
            }
        }

        let stride = i32::try_from(w * 4).unwrap_or(1680);
        let Ok((buffer, canvas_bytes)) = self.pool.create_buffer(
            i32::try_from(w).unwrap_or(420),
            i32::try_from(h).unwrap_or(240),
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
            i32::try_from(w).unwrap_or(420),
            i32::try_from(h).unwrap_or(240),
        );
        let _ = buffer.attach_to(surf);
        surf.commit();
        self.dirty = false;
    }
}

impl CompositorHandler for PolkitApp {
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

impl OutputHandler for PolkitApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for PolkitApp {
    fn request_close(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &Window) {
        self.submit_action(DialogAction::Cancel, qh);
        if self.cfg.demo {
            self.exit = true;
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
        let (w, h) = configure.new_size;
        self.width = w.map_or(DEFAULT_WIDTH, |v| v.get().max(360));
        self.height = h.map_or(DEFAULT_HEIGHT, |v| v.get().max(220));
        self.configured = true;
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for PolkitApp {
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

impl KeyboardHandler for PolkitApp {
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
        let Some(dlg) = self.dialog.as_mut() else {
            return;
        };

        let action = if event.keysym == Keysym::Return || event.keysym == Keysym::KP_Enter {
            let pass = dlg.pass_field.buffer.as_str().trim();
            if pass.is_empty() {
                None
            } else {
                Some(DialogAction::Submit(SecretText::from_slice(pass)))
            }
        } else if event.keysym == Keysym::Escape {
            Some(DialogAction::Cancel)
        } else {
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

            if keys.is_empty() {
                None
            } else {
                let mods = InputModifiers {
                    ctrl: self.modifiers.ctrl,
                    shift: self.modifiers.shift,
                    alt: self.modifiers.alt,
                };
                let mut triggered = None;
                for k in keys {
                    if let Some(act) = dlg.on_key(k, mods, &mut self.fonts, &mut self.clipboard) {
                        triggered = Some(act);
                        break;
                    }
                }
                triggered
            }
        };

        if let Some(act) = action {
            self.submit_action(act, qh);
        } else {
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

impl PointerHandler for PolkitApp {
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
                if let Some(dlg) = self.dialog.as_mut()
                    && let Some(action) = dlg.on_pointer_click(px, py)
                {
                    self.submit_action(action, qh);
                }
            }
        }
    }
}

impl ShmHandler for PolkitApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for PolkitApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(PolkitApp);
delegate_compositor!(PolkitApp);
delegate_output!(PolkitApp);
delegate_shm!(PolkitApp);
delegate_seat!(PolkitApp);
delegate_keyboard!(PolkitApp);
delegate_pointer!(PolkitApp);
delegate_xdg_shell!(PolkitApp);
delegate_xdg_window!(PolkitApp);

fn main() {
    let cfg = parse_args();

    if cfg.test_mode {
        println!("rusty-polkit: running in test mode...");
        let dlg = PolkitDialogState::demo();
        assert_eq!(
            dlg.username,
            std::env::var("USER").unwrap_or_else(|_| "admin".to_owned())
        );
        println!("rusty-polkit: test mode checks passed successfully");
        return;
    }

    let Ok(conn) = Connection::connect_to_env() else {
        eprintln!("rusty-polkit: failed to connect to Wayland display");
        std::process::exit(1);
    };

    let Ok((globals, mut event_queue)) = registry_queue_init(&conn) else {
        eprintln!("rusty-polkit: failed to initialize Wayland registry");
        std::process::exit(1);
    };

    let qh = event_queue.handle();
    let Ok(compositor) = CompositorState::bind(&globals, &qh) else {
        eprintln!("rusty-polkit: wl_compositor unavailable");
        std::process::exit(1);
    };
    let Ok(xdg_shell) = XdgShell::bind(&globals, &qh) else {
        eprintln!("rusty-polkit: xdg_wm_base unavailable");
        std::process::exit(1);
    };
    let Ok(shm) = Shm::bind(&globals, &qh) else {
        eprintln!("rusty-polkit: wl_shm unavailable");
        std::process::exit(1);
    };
    let Ok(pool) = SlotPool::new((DEFAULT_WIDTH * DEFAULT_HEIGHT * 4) as usize, &shm) else {
        eprintln!("rusty-polkit: failed to allocate shm pool");
        std::process::exit(1);
    };

    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title("Аутентификация");
    window.set_app_id("org.rustyos.polkit");
    window.set_min_size(Some((360, 220)));
    window.commit();

    let initial_dialog = if cfg.demo {
        let mut dlg = PolkitDialogState::demo();
        dlg.update_layout(DEFAULT_WIDTH, DEFAULT_HEIGHT);
        Some(dlg)
    } else {
        None
    };

    let agent = PolkitAgentHandle::start_background();

    let mut app = PolkitApp {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        configured: false,
        exit: false,
        keyboard: None,
        pointer: None,
        modifiers: Modifiers::default(),
        cfg,
        fonts: UiFontEngine::new(),
        dialog: initial_dialog,
        agent,
        clipboard: MemoryClipboard::default(),
        dirty: true,
    };

    while !app.exit {
        let _ = event_queue.flush();
        if let Some(guard) = event_queue.prepare_read() {
            let _ = guard.read();
        }
        if event_queue.dispatch_pending(&mut app).is_err() {
            break;
        }

        app.poll_agent_events(&qh);

        if app.dirty && !app.exit {
            app.draw(&qh);
        }

        std::thread::sleep(Duration::from_millis(20));
    }
}
