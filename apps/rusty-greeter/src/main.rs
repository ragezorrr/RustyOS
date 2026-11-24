//! RustyOS graphical `greetd` display manager greeter (`RUSTYOS-136`, ADR-0080).

mod greetd;

use std::path::PathBuf;
use std::time::Duration;

use rusty_app::{
    AppConfig, AppContext, BTN_LEFT, ConfigureInfo, GlassRegion, KeyEvent, Keysym,
    LiquidGlassConfig, PointerEvent, PointerEventKind, RustyApp, WindowDecorationMode, run_app,
};
use rusty_ui::{
    ButtonStyle, InputModifiers, MemoryClipboard, PushButton, Rect, TextField, TextInputKey,
    ThemeMode, ThemePalette, UiFontEngine, UiPainter,
};

const DEFAULT_WIDTH: u32 = 960;
const DEFAULT_HEIGHT: u32 = 600;
const GREETER_CARD_RADIUS: u32 = 18;

#[derive(Debug, Clone, PartialEq, Eq)]
struct GreeterConfig {
    demo: bool,
    theme_mode: ThemeMode,
    default_user: String,
    session_cmd: Vec<String>,
}

fn parse_args_from<I>(args: I) -> GreeterConfig
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = GreeterConfig {
        demo: false,
        theme_mode: ThemeMode::Dark,
        default_user: std::env::var("USER").unwrap_or_else(|_| "rusty".to_owned()),
        session_cmd: vec!["rustyos-session".to_owned()],
    };
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--demo" => cfg.demo = true,
            "--light" => cfg.theme_mode = ThemeMode::Light,
            "--dark" => cfg.theme_mode = ThemeMode::Dark,
            "--user" => {
                if let Some(u) = iter.next() {
                    cfg.default_user = u;
                }
            }
            "--session-cmd" => {
                if let Some(cmd) = iter.next() {
                    let parsed: Vec<String> =
                        cmd.split_whitespace().map(ToOwned::to_owned).collect();
                    if !parsed.is_empty() {
                        cfg.session_cmd = parsed;
                    }
                }
            }
            _ => {}
        }
    }
    cfg
}

fn layout_rects(width: u32, height: u32) -> (Rect, Rect, Rect, Rect) {
    let w = i32::try_from(width).unwrap_or(960);
    let h = i32::try_from(height).unwrap_or(600);
    let cx = w / 2;
    let cy = h / 2;
    let field_w = 280_u32;
    let user_rect = Rect::new(cx - 140, cy - 28, field_w, 34);
    let pass_rect = Rect::new(cx - 140, cy + 16, field_w, 34);
    let btn_rect = Rect::new(cx - 72, cy + 68, 144, 34);
    let card_rect = Rect::new(
        user_rect.x - 32,
        user_rect.y - 108,
        user_rect.width + 64,
        262,
    );
    (user_rect, pass_rect, btn_rect, card_rect)
}

fn glass_for_size(width: u32, height: u32) -> LiquidGlassConfig {
    let (_, _, _, card) = layout_rects(width, height);
    LiquidGlassConfig::popover(GREETER_CARD_RADIUS).with_region(GlassRegion::Rect {
        x: card.x,
        y: card.y,
        width: i32::try_from(card.width).unwrap_or(344),
        height: i32::try_from(card.height).unwrap_or(262),
    })
}

struct GreeterApp {
    cfg: GreeterConfig,
    user_field: TextField,
    pass_field: TextField,
    focus_password: bool,
    status_msg: String,
    auth_rx: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    clipboard: MemoryClipboard,
}

impl GreeterApp {
    fn new(cfg: GreeterConfig) -> Self {
        let mut user_field = TextField::new(Rect::new(0, 0, 280, 34), "Username");
        user_field.buffer.set_text(&cfg.default_user);
        let pass_field = TextField::password(Rect::new(0, 0, 280, 34), "Enter Password");
        Self {
            cfg,
            user_field,
            pass_field,
            focus_password: true,
            status_msg: String::new(),
            auth_rx: None,
            clipboard: MemoryClipboard::default(),
        }
    }

    fn submit_login(&mut self) -> bool {
        let user = self.user_field.buffer.as_str().trim().to_owned();
        let pass = self.pass_field.buffer.as_str().to_owned();
        if user.is_empty() {
            "Enter a username".clone_into(&mut self.status_msg);
            return false;
        }
        if self.cfg.demo {
            if pass.is_empty() {
                "Demo mode: enter any password to sign in".clone_into(&mut self.status_msg);
                return false;
            }
            self.status_msg = format!("Signed in as {user} (demo)");
            return true;
        }
        if self.auth_rx.is_some() {
            return false;
        }
        let Ok(sock) = std::env::var("GREETD_SOCK") else {
            "GREETD_SOCK not set (use --demo for preview)".clone_into(&mut self.status_msg);
            return false;
        };
        "Signing in...".clone_into(&mut self.status_msg);
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
        false
    }

    fn poll_auth_result(&mut self, ctx: &mut AppContext<'_>) {
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
                Ok(()) => ctx.request_exit(),
                Err(err) => {
                    self.status_msg = err;
                    self.pass_field.buffer.set_text("");
                    ctx.request_redraw();
                }
            }
        }
    }
}

impl RustyApp for GreeterApp {
    fn on_configure(&mut self, ctx: &mut AppContext<'_>, info: ConfigureInfo) {
        ctx.set_liquid_glass(Some(glass_for_size(info.width, info.height)));
    }

    fn on_tick(&mut self, ctx: &mut AppContext<'_>) {
        if self.auth_rx.is_some() {
            self.poll_auth_result(ctx);
        }
    }

    fn poll_interval(&self) -> Duration {
        if self.auth_rx.is_some() {
            Duration::from_millis(25)
        } else {
            Duration::from_millis(250)
        }
    }

    fn on_key(&mut self, ctx: &mut AppContext<'_>, event: KeyEvent) {
        if event.keysym == Keysym::Tab {
            self.focus_password = !self.focus_password;
            ctx.request_redraw();
            return;
        }
        if event.keysym == Keysym::Return || event.keysym == Keysym::KP_Enter {
            if self.submit_login() {
                ctx.request_exit();
            } else {
                ctx.request_redraw();
            }
            return;
        }
        if event.keysym == Keysym::Escape && self.cfg.demo {
            ctx.request_exit();
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
            let m = ctx.modifiers();
            let mods = InputModifiers {
                ctrl: m.ctrl,
                shift: m.shift,
                alt: m.alt,
            };
            let field = if self.focus_password {
                &mut self.pass_field
            } else {
                &mut self.user_field
            };
            for k in keys {
                let _ = field.handle_key(
                    k,
                    mods,
                    Duration::ZERO,
                    ctx.fonts_mut(),
                    &mut self.clipboard,
                );
            }
            ctx.request_redraw();
        }
    }

    fn on_pointer(&mut self, ctx: &mut AppContext<'_>, events: &[PointerEvent]) {
        let (user_rect, pass_rect, btn_rect, _) = layout_rects(ctx.width(), ctx.height());
        for ev in events {
            if let PointerEventKind::Press {
                button: BTN_LEFT, ..
            } = ev.kind
            {
                #[allow(clippy::cast_possible_truncation)]
                let (px, py) = (ev.position.0.round() as i32, ev.position.1.round() as i32);
                if user_rect.contains(px, py) {
                    self.focus_password = false;
                    ctx.request_redraw();
                } else if pass_rect.contains(px, py) {
                    self.focus_password = true;
                    ctx.request_redraw();
                } else if btn_rect.contains(px, py) {
                    if self.submit_login() {
                        ctx.request_exit();
                        return;
                    }
                    ctx.request_redraw();
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
        let w = width.max(320);
        let h = height.max(240);
        let pal = ThemePalette::new(self.cfg.theme_mode == ThemeMode::Dark, 0);
        let bg = if pal.is_dark {
            [24, 28, 38, 255]
        } else {
            [236, 240, 248, 255]
        };

        let (user_rect, pass_rect, btn_rect, card_rect) = layout_rects(w, h);
        self.user_field.rect = user_rect;
        self.pass_field.rect = pass_rect;

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
            fonts,
            initial,
            avatar_rect,
            22.0,
            true,
            [255, 255, 255, 255],
        );
        painter.draw_text_centered(fonts, "RustyOS", title_rect, 15.0, true, pal.text_primary);
        self.user_field.paint(
            painter,
            fonts,
            &pal,
            !self.focus_password,
            if self.focus_password { 0.0 } else { 1.0 },
            Duration::ZERO,
        );
        self.pass_field.paint(
            painter,
            fonts,
            &pal,
            self.focus_password,
            if self.focus_password { 1.0 } else { 0.0 },
            Duration::ZERO,
        );
        let btn = PushButton::new(btn_rect, ButtonStyle::Primary);
        btn.paint_labeled(painter, fonts, "Sign In", &pal, false);
        if !self.status_msg.is_empty() {
            let status_rect = Rect::new(
                card_rect.x + 12,
                btn_rect.y + 42,
                card_rect.width.saturating_sub(24),
                22,
            );
            painter.draw_text_centered(
                fonts,
                self.status_msg.clone(),
                status_rect,
                11.5,
                false,
                pal.text_secondary,
            );
        }
    }
}

fn main() {
    let cfg = parse_args_from(std::env::args().skip(1));
    let host_cfg = AppConfig::new(
        "RustyOS Sign In",
        "org.rustyos.greeter",
        (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    )
    .with_min_size(320, 240)
    .with_decorations(WindowDecorationMode::Server)
    .with_liquid_glass(glass_for_size(DEFAULT_WIDTH, DEFAULT_HEIGHT));

    if let Err(err) = run_app(host_cfg, GreeterApp::new(cfg)) {
        eprintln!("rusty-greeter: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_ui::UiDrawList;

    #[test]
    fn parse_args_overrides_demo_theme_user_and_session_cmd() {
        let cfg = parse_args_from([
            "--demo".to_owned(),
            "--light".to_owned(),
            "--user".to_owned(),
            "alice".to_owned(),
            "--session-cmd".to_owned(),
            "rustyos-session --drm".to_owned(),
        ]);
        assert!(cfg.demo);
        assert_eq!(cfg.theme_mode, ThemeMode::Light);
        assert_eq!(cfg.default_user, "alice");
        assert_eq!(cfg.session_cmd, vec!["rustyos-session", "--drm"]);
    }

    #[test]
    fn demo_submit_validates_username_and_password() {
        let cfg = GreeterConfig {
            demo: true,
            theme_mode: ThemeMode::Dark,
            default_user: String::new(),
            session_cmd: vec!["rustyos-session".to_owned()],
        };
        let mut app = GreeterApp::new(cfg);
        assert!(!app.submit_login());
        assert_eq!(app.status_msg, "Enter a username");

        app.user_field.buffer.set_text("rusty");
        assert!(!app.submit_login());
        assert!(app.status_msg.contains("enter any password"));

        app.pass_field.buffer.set_text("secret");
        assert!(app.submit_login());
        assert!(app.status_msg.contains("Signed in as rusty"));
    }

    #[test]
    fn greeter_paint_produces_draw_commands_and_glass_region_matches_card() {
        let cfg = GreeterConfig {
            demo: true,
            theme_mode: ThemeMode::Dark,
            default_user: "rusty".to_owned(),
            session_cmd: vec!["rustyos-session".to_owned()],
        };
        let mut app = GreeterApp::new(cfg);
        app.status_msg = "Ready".to_owned();
        let mut fonts = UiFontEngine::new();
        let mut list = UiDrawList::new();
        {
            let mut painter = UiPainter::new(&mut list);
            app.paint(&mut painter, &mut fonts, DEFAULT_WIDTH, DEFAULT_HEIGHT);
        }
        assert!(list.commands.len() >= 5);

        let (_, _, _, card) = layout_rects(DEFAULT_WIDTH, DEFAULT_HEIGHT);
        let glass = glass_for_size(DEFAULT_WIDTH, DEFAULT_HEIGHT);
        assert_eq!(glass.corner_radius, GREETER_CARD_RADIUS);
        assert_eq!(
            glass.region,
            GlassRegion::Rect {
                x: card.x,
                y: card.y,
                width: i32::try_from(card.width).unwrap_or(0),
                height: i32::try_from(card.height).unwrap_or(0),
            }
        );
    }
}
