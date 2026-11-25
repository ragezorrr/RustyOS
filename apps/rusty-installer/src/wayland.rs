//! `rusty-app` host integration for `rusty-installer` (`RUSTYOS-115`, ADR-0065).

use std::time::{Duration, Instant};

use rusty_app::{
    AppConfig, AppContext, AppError, BTN_LEFT, ConfigureInfo, KeyEvent, Keysym, LiquidGlassConfig,
    PointerEvent, PointerEventKind, ResizeEdge, RustyApp, WindowDecorationMode, run_app,
};

use crate::disks::TargetDiskInfo;
use crate::layout::{
    InstallerHit, InstallerLayout, MIN_HEIGHT, MIN_WIDTH, ResizeEdgeZone, WINDOW_CORNER_RADIUS,
    compute_layout, hit_test,
};
use crate::render::{render_window_painter, with_font_engine};
use crate::state::{
    DiskInstallMode, InstallPhase, InstallStep, InstallerAppState, StartupOptions, TextFieldId,
    WizardMode,
};

const DEFAULT_WIDTH: u32 = 880;
const DEFAULT_HEIGHT: u32 = 560;

struct InstallerApp {
    app: InstallerAppState,
    clipboard: rusty_ui::MemoryClipboard,
    width: u32,
    height: u32,
    has_glass: bool,
    pointer_pos: (f64, f64),
    pointer_pressed: bool,
    click_streak: u8,
    last_click_at: Option<Instant>,
    hover_hit: Option<InstallerHit>,
    layout_cache: Option<InstallerLayout>,
}

impl InstallerApp {
    fn new(opts: &StartupOptions) -> Self {
        let mut app = InstallerAppState::new(opts);
        app.kick_initial_probes();
        Self {
            app,
            clipboard: rusty_ui::MemoryClipboard::default(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            has_glass: false,
            pointer_pos: (0.0, 0.0),
            pointer_pressed: false,
            click_streak: 0,
            last_click_at: None,
            hover_hit: None,
            layout_cache: None,
        }
    }

    fn current_layout(&mut self) -> InstallerLayout {
        if let Some(cached) = &self.layout_cache {
            return cached.clone();
        }
        let l = compute_layout(&self.app, self.width, self.height);
        self.layout_cache = Some(l.clone());
        l
    }

    fn sync_dirty(&mut self, ctx: &mut AppContext<'_>) {
        if !self.app.win.running {
            ctx.request_exit();
            return;
        }
        if self.app.win.dirty {
            ctx.request_redraw();
        }
    }

    fn handle_key_press(&mut self, ctx: &mut AppContext<'_>, event: &KeyEvent) {
        let mods = ctx.modifiers();
        if mods.ctrl && event.keysym == Keysym::q {
            self.app.request_close();
            ctx.request_exit();
            return;
        }
        if self.app.caret.focused_field.is_some()
            && let Some(ui_key) = map_keysym_to_ui(event.keysym, event.utf8.as_deref())
        {
            let ui_mods = rusty_ui::InputModifiers {
                ctrl: mods.ctrl,
                shift: mods.shift,
                alt: mods.alt,
            };
            let ev = with_font_engine(|fe| {
                self.app
                    .handle_text_key(ui_key, ui_mods, fe, &mut self.clipboard)
            });
            match ev {
                rusty_ui::TextInputEvent::Cancelled => {
                    self.app.set_focused_field(None);
                    return;
                }
                rusty_ui::TextInputEvent::Submitted => {
                    if self.app.caret.focused_field == Some(TextFieldId::WifiPassword) {
                        self.app.connect_selected_wifi();
                    } else {
                        self.app.go_next(Instant::now());
                    }
                    self.layout_cache = None;
                    return;
                }
                rusty_ui::TextInputEvent::Changed | rusty_ui::TextInputEvent::CaretMoved => return,
                rusty_ui::TextInputEvent::Ignored => {}
            }
        }
        match event.keysym {
            Keysym::Escape if self.app.caret.focused_field.is_some() => {
                self.app.set_focused_field(None);
            }
            Keysym::Tab => self.app.cycle_focus(mods.shift),
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
                if self.app.caret.focused_field == Some(TextFieldId::WifiPassword) {
                    self.app.connect_selected_wifi();
                } else {
                    self.app.go_next(Instant::now());
                }
                self.layout_cache = None;
            }
            _ => {
                if !mods.ctrl
                    && !mods.alt
                    && let Some(text) = event.utf8.as_deref()
                {
                    self.app.handle_text_input(text);
                }
            }
        }
    }

    fn execute_left_click(&mut self, ctx: &mut AppContext<'_>, px: f64, py: f64, serial: u32) {
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
        self.dispatch_hit(ctx, hit, (px, py), serial, double_click, now);
    }

    fn dispatch_hit(
        &mut self,
        ctx: &mut AppContext<'_>,
        hit: InstallerHit,
        pos: (f64, f64),
        serial: u32,
        double_click: bool,
        now: Instant,
    ) {
        let (px, py) = pos;
        match hit {
            InstallerHit::ResizeEdge(zone) => {
                ctx.start_interactive_resize(map_resize_edge(zone), serial);
            }
            InstallerHit::WindowDrag => {
                if double_click {
                    ctx.set_maximized(!self.app.win.maximized);
                } else {
                    ctx.start_interactive_move(serial);
                }
            }
            InstallerHit::TrafficClose => self.app.request_close(),
            InstallerHit::TrafficMinimize => ctx.set_minimized(),
            InstallerHit::TrafficMaximize => ctx.set_maximized(!self.app.win.maximized),
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
}

impl RustyApp for InstallerApp {
    fn on_configure(&mut self, ctx: &mut AppContext<'_>, info: ConfigureInfo) {
        if info.width != self.width || info.height != self.height {
            self.width = info.width;
            self.height = info.height;
            self.layout_cache = None;
        }
        self.has_glass = ctx.has_liquid_glass();
        self.app.win.maximized = info.is_maximized;
        self.app.win.focused = info.is_activated;
        self.app.win.dirty = true;
        ctx.request_redraw();
    }

    fn on_key(&mut self, ctx: &mut AppContext<'_>, event: KeyEvent) {
        self.handle_key_press(ctx, &event);
        self.sync_dirty(ctx);
    }

    fn on_pointer(&mut self, ctx: &mut AppContext<'_>, events: &[PointerEvent]) {
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
                    self.execute_left_click(ctx, ev.position.0, ev.position.1, serial);
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
        self.sync_dirty(ctx);
    }

    fn on_tick(&mut self, ctx: &mut AppContext<'_>) {
        let now = Instant::now();
        if self.app.poll_workers(now) {
            self.layout_cache = None;
        }
        let _ = self.app.poll_system_theme(now);
        let _ = self.app.update_caret_blink(now);
        self.sync_dirty(ctx);
    }

    fn poll_interval(&self) -> Duration {
        if self.app.progress_anim.is_active() || self.app.phase == InstallPhase::Running {
            Duration::from_millis(16)
        } else if self.app.caret.focused_field.is_some() {
            Duration::from_millis(250)
        } else {
            Duration::from_millis(450)
        }
    }

    fn on_close_requested(&mut self, _ctx: &mut AppContext<'_>) -> bool {
        self.app.request_close();
        true
    }

    fn paint(
        &mut self,
        painter: &mut rusty_ui::UiPainter<'_>,
        fonts: &mut rusty_ui::UiFontEngine,
        width: u32,
        height: u32,
    ) {
        self.width = width;
        self.height = height;
        let layout = self.current_layout();
        render_window_painter(
            painter,
            fonts,
            width,
            height,
            &self.app,
            &layout,
            self.hover_hit,
            self.has_glass,
        );
        self.app.win.dirty = false;
    }
}

pub(crate) fn run(opts: &StartupOptions) -> Result<(), AppError> {
    let title = if opts.mode == WizardMode::Install {
        "Install RustyOS"
    } else {
        "RustyOS Setup"
    };
    let mut cfg = AppConfig::new(
        title,
        "org.rustyos.Installer",
        (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    )
    .with_min_size(
        u32::try_from(MIN_WIDTH).unwrap_or(720),
        u32::try_from(MIN_HEIGHT).unwrap_or(480),
    )
    .with_decorations(WindowDecorationMode::Client)
    .with_no_gpu(opts.no_gpu);
    if !opts.no_glass {
        cfg = cfg.with_liquid_glass(LiquidGlassConfig::sidebar(
            u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14),
        ));
    }
    run_app(cfg, InstallerApp::new(opts))
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
