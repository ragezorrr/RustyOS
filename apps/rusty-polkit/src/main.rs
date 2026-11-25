//! RustyOS graphical `PolicyKit` authentication agent (`RUSTYOS-172`, ADR-0101).

mod dialog;
#[cfg(test)]
mod tests;

use std::time::Duration;

use dialog::{DialogAction, PolkitDialogState};
use rusty_app::{
    AppConfig, AppContext, BTN_LEFT, ConfigureInfo, KeyEvent, Keysym, LiquidGlassConfig,
    PointerEvent, PointerEventKind, RustyApp, WindowDecorationMode, run_app,
};
use rusty_services::{PolkitAgentHandle, PolkitEvent};
use rusty_ui::{
    InputModifiers, MemoryClipboard, Rect, TextInputKey, ThemeMode, ThemePalette, UiFontEngine,
    UiPainter,
};

const DEFAULT_WIDTH: u32 = 420;
const DEFAULT_HEIGHT: u32 = 240;
const POLKIT_GLASS_RADIUS: u32 = 16;

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

struct PolkitApp {
    cfg: PolkitConfig,
    dialog: Option<PolkitDialogState>,
    agent: PolkitAgentHandle,
    clipboard: MemoryClipboard,
}

impl PolkitApp {
    fn sync_glass(&self, ctx: &mut AppContext<'_>) {
        if self.dialog.is_some() {
            ctx.set_liquid_glass(Some(LiquidGlassConfig::popover(POLKIT_GLASS_RADIUS)));
        } else {
            ctx.set_liquid_glass(None);
        }
    }

    fn submit_action(&mut self, action: DialogAction, ctx: &mut AppContext<'_>) {
        match action {
            DialogAction::Submit(secret) => {
                if let Some(dlg) = self.dialog.as_mut() {
                    dlg.set_checking(true);
                    if self.cfg.demo {
                        dlg.status_msg = Some("Авторизация успешна (демо)".to_owned());
                        ctx.request_redraw();
                        ctx.request_exit();
                        return;
                    }
                    self.agent.submit_password(dlg.cookie.clone(), secret);
                    ctx.request_redraw();
                }
            }
            DialogAction::Cancel => {
                if let Some(dlg) = self.dialog.take() {
                    if !self.cfg.demo {
                        self.agent.cancel(dlg.cookie);
                    }
                    self.sync_glass(ctx);
                    ctx.request_redraw();
                    if self.cfg.demo {
                        ctx.request_exit();
                    }
                }
            }
        }
    }
}

impl RustyApp for PolkitApp {
    fn on_configure(&mut self, ctx: &mut AppContext<'_>, info: ConfigureInfo) {
        if let Some(dlg) = self.dialog.as_mut() {
            dlg.update_layout(info.width, info.height);
        }
        self.sync_glass(ctx);
    }

    fn on_tick(&mut self, ctx: &mut AppContext<'_>) {
        for ev in self.agent.drain_events() {
            match ev {
                PolkitEvent::BeginPrompt(req) => {
                    let mut dlg = PolkitDialogState::new(req);
                    dlg.update_layout(ctx.width(), ctx.height());
                    self.dialog = Some(dlg);
                    self.sync_glass(ctx);
                    ctx.request_redraw();
                }
                PolkitEvent::PromptError { cookie, message } => {
                    if let Some(dlg) = self.dialog.as_mut()
                        && dlg.cookie == cookie
                    {
                        dlg.set_error(message);
                        ctx.request_redraw();
                    }
                }
                PolkitEvent::ClosePrompt { cookie } => {
                    if let Some(dlg) = &self.dialog
                        && dlg.cookie == cookie
                    {
                        self.dialog = None;
                        self.sync_glass(ctx);
                        ctx.request_redraw();
                        if self.cfg.demo {
                            ctx.request_exit();
                        }
                    }
                }
            }
        }
    }

    fn poll_interval(&self) -> Duration {
        if self.dialog.is_some() {
            Duration::from_millis(20)
        } else {
            Duration::from_millis(250)
        }
    }

    fn on_close_requested(&mut self, ctx: &mut AppContext<'_>) -> bool {
        self.submit_action(DialogAction::Cancel, ctx);
        self.cfg.demo
    }

    fn on_key(&mut self, ctx: &mut AppContext<'_>, event: KeyEvent) {
        let Some(dlg) = self.dialog.as_mut() else {
            return;
        };

        let action = if event.keysym == Keysym::Tab || event.keysym == Keysym::ISO_Left_Tab {
            let backwards = event.keysym == Keysym::ISO_Left_Tab || ctx.modifiers().shift;
            dlg.cycle_focus(backwards);
            None
        } else if event.keysym == Keysym::Return
            || event.keysym == Keysym::KP_Enter
            || (event.keysym == Keysym::space && !dlg.focus.is_focused(dialog::focus_id_password()))
        {
            dlg.activate_focused()
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
                let m = ctx.modifiers();
                let mods = InputModifiers {
                    ctrl: m.ctrl,
                    shift: m.shift,
                    alt: m.alt,
                };
                let mut triggered = None;
                for k in keys {
                    if let Some(act) = dlg.on_key(k, mods, ctx.fonts_mut(), &mut self.clipboard) {
                        triggered = Some(act);
                        break;
                    }
                }
                triggered
            }
        };

        if let Some(act) = action {
            self.submit_action(act, ctx);
        } else {
            ctx.request_redraw();
        }
    }

    fn on_pointer(&mut self, ctx: &mut AppContext<'_>, events: &[PointerEvent]) {
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
                    self.submit_action(action, ctx);
                }
            }
        }
    }

    fn paint(
        &mut self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        width: u32,
        height: u32,
    ) {
        let pal = ThemePalette::new(self.cfg.theme_mode == ThemeMode::Dark, 0);
        if let Some(dlg) = self.dialog.as_mut() {
            dlg.update_layout(width, height);
            dlg.paint(painter, fonts, &pal, width, height);
        } else {
            let blank_bg = if pal.is_dark {
                [20, 22, 28, 0]
            } else {
                [245, 246, 250, 0]
            };
            painter.fill_rect(Rect::new(0, 0, width, height), blank_bg);
        }
    }
}

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

    let initial_dialog = if cfg.demo {
        let mut dlg = PolkitDialogState::demo();
        dlg.update_layout(DEFAULT_WIDTH, DEFAULT_HEIGHT);
        Some(dlg)
    } else {
        None
    };

    let mut host_cfg = AppConfig::new(
        "Аутентификация",
        "org.rustyos.polkit",
        (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    )
    .with_min_size(360, 220)
    .with_decorations(WindowDecorationMode::Server);

    if initial_dialog.is_some() {
        host_cfg = host_cfg.with_liquid_glass(LiquidGlassConfig::popover(POLKIT_GLASS_RADIUS));
    }

    let app = PolkitApp {
        cfg,
        dialog: initial_dialog,
        agent: PolkitAgentHandle::start_background(),
        clipboard: MemoryClipboard::default(),
    };

    if let Err(err) = run_app(host_cfg, app) {
        eprintln!("rusty-polkit: {err}");
        std::process::exit(1);
    }
}
