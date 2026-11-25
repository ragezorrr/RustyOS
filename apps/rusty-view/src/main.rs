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

use rusty_app::{
    AppConfig, AppContext, AppMenuItemSpec, AppMenuSectionSpec, BTN_LEFT, ConfigureInfo, KeyEvent,
    Keysym, LiquidGlassConfig, PointerEvent, PointerEventKind, RustyApp, WindowDecorationMode,
    run_app,
};
use rusty_ui::{
    ThemeMode, TrafficLightButtons, TrafficLightKind, UiCanvas, UiDrawList, UiFontEngine,
    UiPainter, WINDOW_RADIUS, WindowHitTarget, render_draw_list_to_canvas,
};

use state::{DEFAULT_VIEWER_HEIGHT, DEFAULT_VIEWER_WIDTH, ViewerState};

const MIN_VIEWER_WIDTH: u32 = 400;
const MIN_VIEWER_HEIGHT: u32 = 300;
const VIEWER_GLASS_RADIUS: u32 = WINDOW_RADIUS;

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
    state: ViewerState,
}

impl ViewerApp {
    fn sync_glass(&self, ctx: &mut AppContext<'_>) {
        if self.state.is_fullscreen {
            ctx.set_liquid_glass(None);
        } else {
            ctx.set_liquid_glass(Some(LiquidGlassConfig::popover(VIEWER_GLASS_RADIUS)));
        }
    }

    fn toggle_maximize(&mut self, ctx: &mut AppContext<'_>) {
        let next = !self.state.frame.is_maximized;
        self.state.frame.is_maximized = next;
        ctx.set_maximized(next);
        self.state.dirty = true;
    }

    fn toggle_fullscreen(&mut self, ctx: &mut AppContext<'_>) {
        let next = !self.state.is_fullscreen;
        self.state.is_fullscreen = next;
        ctx.set_fullscreen(next);
        self.sync_glass(ctx);
        self.sync_app_menu(ctx);
        self.state.dirty = true;
    }

    fn sync_app_menu(&self, ctx: &mut AppContext<'_>) {
        let sections = vec![
            AppMenuSectionSpec::new(
                1,
                "File",
                vec![
                    AppMenuItemSpec::action(101, "Previous Image").with_shortcut("Left"),
                    AppMenuItemSpec::action(102, "Next Image").with_shortcut("Right"),
                    AppMenuItemSpec::separator(),
                    AppMenuItemSpec::action(103, "Close Window").with_shortcut("Ctrl+Q"),
                ],
            ),
            AppMenuSectionSpec::new(
                2,
                "View",
                vec![
                    AppMenuItemSpec::action(201, "Zoom In").with_shortcut("+"),
                    AppMenuItemSpec::action(202, "Zoom Out").with_shortcut("-"),
                    AppMenuItemSpec::action(203, "Actual Size").with_shortcut("0"),
                    AppMenuItemSpec::action(204, "Fit to Window")
                        .with_shortcut("F")
                        .with_checked(self.state.viewport.is_fitting),
                    AppMenuItemSpec::separator(),
                    AppMenuItemSpec::action(205, "Show Info").with_shortcut("I"),
                    AppMenuItemSpec::action(206, "Enter Full Screen")
                        .with_shortcut("F11")
                        .with_checked(self.state.is_fullscreen),
                ],
            ),
            AppMenuSectionSpec::new(
                3,
                "Image",
                vec![
                    AppMenuItemSpec::action(301, "Rotate Right").with_shortcut("R"),
                    AppMenuItemSpec::action(302, "Rotate Left").with_shortcut("Shift+R"),
                    AppMenuItemSpec::action(303, "Flip Horizontal").with_shortcut("H"),
                ],
            ),
        ];
        ctx.set_app_menu(sections);
    }
}

impl RustyApp for ViewerApp {
    fn on_configure(&mut self, ctx: &mut AppContext<'_>, info: ConfigureInfo) {
        self.state.frame.is_maximized = info.is_maximized;
        self.state.frame.is_focused = info.is_activated;
        self.state.is_fullscreen = info.is_fullscreen;
        self.state.frame.buttons = TrafficLightButtons::default_csd_for_width(info.width);

        if self.state.viewport.is_fitting {
            self.state.action_fit(info.width, info.height);
        }

        self.sync_glass(ctx);
        self.sync_app_menu(ctx);
        self.state.dirty = true;
    }

    fn on_menu_item(&mut self, ctx: &mut AppContext<'_>, item_id: i32) {
        let w = ctx.width();
        let h = ctx.height();
        match item_id {
            101 => self.state.action_prev(w, h),
            102 => self.state.action_next(w, h),
            103 => {
                ctx.request_exit();
                return;
            }
            201 => self.state.action_zoom_in(w, h),
            202 => self.state.action_zoom_out(w, h),
            203 => self.state.action_actual_size(),
            204 => self.state.action_fit(w, h),
            205 => self.state.action_toggle_info(),
            206 => self.toggle_fullscreen(ctx),
            301 => self.state.action_rotate_cw(w, h),
            302 => self.state.action_rotate_ccw(w, h),
            303 => self.state.action_flip_h(),
            _ => return,
        }
        ctx.set_title(&self.state.frame.title);
        self.sync_app_menu(ctx);
        self.state.dirty = true;
        ctx.request_redraw();
    }

    fn on_tick(&mut self, ctx: &mut AppContext<'_>) {
        if self.state.toolbar.update_visibility() {
            self.state.dirty = true;
        }
        ctx.set_title(&self.state.frame.title);
        if self.state.dirty {
            ctx.request_redraw();
        }
    }

    fn poll_interval(&self) -> Duration {
        if self.state.toolbar.opacity > 0.01 && !self.state.toolbar.is_pointer_inside {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(250)
        }
    }

    fn on_key(&mut self, ctx: &mut AppContext<'_>, event: KeyEvent) {
        let sym = event.keysym;
        let w = ctx.width();
        let h = ctx.height();
        let mods = ctx.modifiers();

        match sym {
            Keysym::Escape | Keysym::q | Keysym::Q => {
                ctx.request_exit();
                return;
            }
            Keysym::F11 => {
                self.toggle_fullscreen(ctx);
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
                if mods.shift {
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
                if mods.ctrl {
                    match sym {
                        Keysym::q => {
                            ctx.request_exit();
                            return;
                        }
                        Keysym::r => self.state.action_rotate_cw(w, h),
                        _ => {}
                    }
                }
            }
        }

        ctx.set_title(&self.state.frame.title);
        self.state.dirty = true;
        ctx.request_redraw();
    }

    fn on_pointer(&mut self, ctx: &mut AppContext<'_>, events: &[PointerEvent]) {
        let w = ctx.width();
        let h = ctx.height();

        for ev in events {
            match ev.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.state
                        .on_pointer_moved(ev.position.0, ev.position.1, w, h);
                }
                PointerEventKind::Press { button, serial, .. } => {
                    if button == BTN_LEFT {
                        let target = self
                            .state
                            .on_pointer_down(ev.position.0, ev.position.1, w, h);
                        if target == WindowHitTarget::TitlebarDrag && !self.state.is_fullscreen {
                            ctx.start_interactive_move(serial);
                        }
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if button == BTN_LEFT {
                        let (px, py) = ctx.pointer_pos();
                        match self.state.on_pointer_up(px, py, w, h) {
                            Some(TrafficLightKind::Close) => {
                                ctx.request_exit();
                                return;
                            }
                            Some(TrafficLightKind::Minimize) => {
                                ctx.set_minimized();
                            }
                            Some(TrafficLightKind::Maximize) => {
                                self.toggle_maximize(ctx);
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
                        let (px, py) = ctx.pointer_pos();
                        self.state.on_wheel(delta, px, py, w, h);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    self.state.frame.on_pointer_leave();
                    self.state.toolbar.is_pointer_inside = false;
                    self.state.dirty = true;
                }
            }
        }

        if self.state.dirty {
            ctx.request_redraw();
        }
    }

    fn paint(
        &mut self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        width: u32,
        height: u32,
    ) {
        let _ = self.state.toolbar.update_visibility();
        self.state.paint(painter, fonts, width, height);
        self.state.dirty = false;
    }
}

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

    let mut state = ViewerState::new(
        cfg.target_path.as_deref(),
        DEFAULT_VIEWER_WIDTH,
        DEFAULT_VIEWER_HEIGHT,
        cfg.theme_mode,
    );
    state.is_fullscreen = cfg.fullscreen;

    let mut host_cfg = AppConfig::new(
        "Image Viewer",
        "org.rustyos.view",
        (DEFAULT_VIEWER_WIDTH, DEFAULT_VIEWER_HEIGHT),
    )
    .with_min_size(MIN_VIEWER_WIDTH, MIN_VIEWER_HEIGHT)
    .with_decorations(WindowDecorationMode::Client)
    .with_fullscreen(cfg.fullscreen);

    if !cfg.fullscreen {
        host_cfg = host_cfg.with_liquid_glass(LiquidGlassConfig::popover(VIEWER_GLASS_RADIUS));
    }

    if let Err(err) = run_app(host_cfg, ViewerApp { state }) {
        eprintln!("rusty-view: {err}");
        eprintln!("Hint: use --test for headless verification without a Wayland compositor");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
