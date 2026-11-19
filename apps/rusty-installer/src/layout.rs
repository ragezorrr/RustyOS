//! Layout geometry and hit-testing for `rusty-installer` (`RUSTYOS-115`).

use crate::state::{
    BootloaderKind, InstallPhase, InstallStep, InstallerAppState, KEYBOARD_PRESETS,
    LANGUAGE_OPTIONS, OobeStep, RunEnv, TextFieldId, ThemeChoice, WizardMode,
};
use rusty_services::file_manager::RUSTYOS_ACCENT_SWATCHES;

pub(crate) const WINDOW_CORNER_RADIUS: i32 = 18;
pub(crate) const CARD_CORNER_RADIUS: i32 = 12;
pub(crate) const PILL_CORNER_RADIUS: i32 = 16;
pub(crate) const HEADER_HEIGHT: i32 = 44;
pub(crate) const FOOTER_HEIGHT: i32 = 58;
pub(crate) const MIN_WIDTH: i32 = 700;
pub(crate) const MIN_HEIGHT: i32 = 500;
const RESIZE_EDGE_PX: i32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    #[must_use]
    pub(crate) const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub(crate) const fn right(self) -> i32 {
        self.x + self.w
    }

    #[must_use]
    pub(crate) const fn bottom(self) -> i32 {
        self.y + self.h
    }

    #[must_use]
    pub(crate) fn contains_f64(self, px: f64, py: f64) -> bool {
        let fx = f64::from(self.x);
        let fy = f64::from(self.y);
        px >= fx && px < fx + f64::from(self.w) && py >= fy && py < fy + f64::from(self.h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResizeEdgeZone {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WelcomeAction {
    InstallToDisk,
    RunDoctor,
    OpenTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallerHit {
    ResizeEdge(ResizeEdgeZone),
    WindowDrag,
    TrafficClose,
    TrafficMinimize,
    TrafficMaximize,
    BackButton,
    PrimaryButton,
    WelcomeCard(WelcomeAction),
    SelectDisk(usize),
    RefreshDisks,
    SetBootloader(BootloaderKind),
    SelectLanguage(usize),
    SelectKeyboard(usize),
    RefreshNetwork,
    FocusField(TextFieldId),
    ToggleAutologin,
    SelectTheme(ThemeChoice),
    SelectAccent(usize),
    ToggleLogs,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionCardRect {
    pub action: WelcomeAction,
    pub rect: Rect,
    pub title: &'static str,
    pub subtitle: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct DiskCardRect {
    pub idx: usize,
    pub rect: Rect,
    pub selected: bool,
    pub selectable: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct OptionRowRect {
    pub idx: usize,
    pub rect: Rect,
    pub title: String,
    pub subtitle: String,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TextFieldRect {
    pub id: TextFieldId,
    pub label: &'static str,
    pub rect: Rect,
    pub masked: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ThemeTileRect {
    pub choice: ThemeChoice,
    pub label: &'static str,
    pub ring_rect: Rect,
    pub preview_rect: Rect,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum StepContentLayout {
    Welcome {
        hero_emblem: Rect,
        cards: Vec<ActionCardRect>,
        status_rect: Rect,
    },
    SelectDisk {
        cards: Vec<DiskCardRect>,
        refresh_btn: Rect,
        boot_systemd: Rect,
        boot_grub: Rect,
    },
    AccountAndStyle {
        fields: Vec<TextFieldRect>,
        kb_chips: Vec<(usize, Rect, &'static str)>,
        theme_chips: Vec<(ThemeChoice, Rect, &'static str)>,
        swatches: Vec<(usize, Rect, [u8; 4], bool)>,
    },
    Installing {
        emblem_rect: Rect,
        progress_rect: Rect,
        toggle_logs_btn: Rect,
        log_box: Option<Rect>,
        summary_lines: Vec<String>,
    },
    Hello {
        hero_emblem: Rect,
        rows: Vec<OptionRowRect>,
    },
    Keyboard {
        rows: Vec<OptionRowRect>,
    },
    Network {
        card_rect: Rect,
        refresh_btn: Rect,
    },
    OobeAccount {
        fields: Vec<TextFieldRect>,
        autologin_row: Rect,
    },
    Appearance {
        tiles: [ThemeTileRect; 3],
        accent_card: Rect,
        swatches: Vec<(usize, Rect, [u8; 4], bool)>,
    },
    Finish {
        hero_emblem: Rect,
        summary_card: Rect,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct InstallerLayout {
    pub win_w: i32,
    pub win_h: i32,
    pub traffic_close: Rect,
    pub traffic_min: Rect,
    pub traffic_max: Rect,
    pub header_rect: Rect,
    pub step_dots_center_x: i32,
    pub step_count: usize,
    pub active_step_idx: usize,
    pub content_rect: Rect,
    pub footer_rect: Rect,
    pub back_btn: Option<Rect>,
    pub back_label: &'static str,
    pub primary_btn: Rect,
    pub primary_label: &'static str,
    pub content: StepContentLayout,
}

#[must_use]
#[allow(clippy::cast_possible_wrap)]
pub(crate) fn compute_layout(app: &InstallerAppState, width: u32, height: u32) -> InstallerLayout {
    let win_w = i32::try_from(width).unwrap_or(800).max(MIN_WIDTH);
    let win_h = i32::try_from(height).unwrap_or(580).max(MIN_HEIGHT);
    let traffic_close = Rect::new(16, 15, 14, 14);
    let traffic_min = Rect::new(38, 15, 14, 14);
    let traffic_max = Rect::new(60, 15, 14, 14);
    let header_rect = Rect::new(0, 0, win_w, HEADER_HEIGHT);
    let content_rect = Rect::new(
        36,
        HEADER_HEIGHT + 6,
        win_w - 72,
        win_h - HEADER_HEIGHT - FOOTER_HEIGHT - 12,
    );
    let footer_rect = Rect::new(0, win_h - FOOTER_HEIGHT, win_w, FOOTER_HEIGHT);

    let (step_count, active_step_idx, show_back, back_label, primary_label) = footer_meta(app);
    let back_btn = show_back.then(|| Rect::new(36, footer_rect.y + 13, 98, 32));
    let primary_w = if primary_label.len() > 14 { 176 } else { 132 };
    let primary_btn = Rect::new(win_w - 36 - primary_w, footer_rect.y + 13, primary_w, 32);

    let content = match app.mode {
        WizardMode::Install => compute_install_content(app, content_rect),
        WizardMode::Oobe => compute_oobe_content(app, content_rect),
    };

    InstallerLayout {
        win_w,
        win_h,
        traffic_close,
        traffic_min,
        traffic_max,
        header_rect,
        step_dots_center_x: win_w / 2,
        step_count,
        active_step_idx,
        content_rect,
        footer_rect,
        back_btn,
        back_label,
        primary_btn,
        primary_label,
        content,
    }
}

fn idx_i32(idx: usize) -> i32 {
    i32::try_from(idx).unwrap_or(0)
}

fn footer_meta(app: &InstallerAppState) -> (usize, usize, bool, &'static str, &'static str) {
    match app.mode {
        WizardMode::Install => {
            let show_back =
                app.install_step != InstallStep::Welcome && app.phase != InstallPhase::Running;
            let primary = match app.install_step {
                InstallStep::Welcome | InstallStep::SelectDisk | InstallStep::AccountStyle => {
                    "Continue"
                }
                InstallStep::Installing if app.phase == InstallPhase::Finished => {
                    if app.dry_run || app.env_kind == RunEnv::Demo {
                        "Preview Setup"
                    } else {
                        "Restart Now"
                    }
                }
                InstallStep::Installing if app.phase == InstallPhase::Running => "Installing...",
                InstallStep::Installing if app.dry_run => "Run Simulation",
                InstallStep::Installing => "Erase & Install",
            };
            (4, app.install_step.index(), show_back, "Back", primary)
        }
        WizardMode::Oobe => {
            let show_back = app.oobe_step != OobeStep::Hello;
            let primary = match app.oobe_step {
                OobeStep::Finish => "Get Started",
                _ => "Continue",
            };
            (6, app.oobe_step.index(), show_back, "Back", primary)
        }
    }
}

fn compute_install_content(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    match app.install_step {
        InstallStep::Welcome => compute_install_welcome(c),
        InstallStep::SelectDisk => compute_install_select_disk(app, c),
        InstallStep::AccountStyle => compute_install_account_style(app, c),
        InstallStep::Installing => compute_install_progress(app, c),
    }
}

fn compute_install_welcome(c: Rect) -> StepContentLayout {
    let emblem = Rect::new(c.x + (c.w - 56) / 2, c.y + 4, 56, 56);
    let card_specs = [
        (
            WelcomeAction::InstallToDisk,
            "Install RustyOS to Disk",
            "Guided macOS-style installation on NVMe, SSD, or VM drive",
        ),
        (
            WelcomeAction::RunDoctor,
            "System Readiness Check (--doctor)",
            "Verify DRM/KMS, PipeWire, Polkit, Seatd, and Wayland session tools",
        ),
        (
            WelcomeAction::OpenTerminal,
            "Open RustyOS Terminal",
            "Inspect disks, partitions, or run recovery utilities in shell",
        ),
    ];
    let cards = card_specs
        .into_iter()
        .enumerate()
        .map(|(i, (action, title, subtitle))| ActionCardRect {
            action,
            rect: Rect::new(c.x + 28, c.y + 124 + idx_i32(i) * 66, c.w - 56, 58),
            title,
            subtitle,
        })
        .collect();
    StepContentLayout::Welcome {
        hero_emblem: emblem,
        cards,
        status_rect: Rect::new(c.x + 28, c.y + 322, c.w - 56, 30),
    }
}

fn compute_install_select_disk(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let cards = app
        .disks
        .iter()
        .enumerate()
        .take(4)
        .map(|(i, d)| DiskCardRect {
            idx: i,
            rect: Rect::new(c.x + 12, c.y + 52 + idx_i32(i) * 68, c.w - 24, 60),
            selected: app.selected_disk_idx == Some(i),
            selectable: d.is_selectable(),
        })
        .collect();
    let row_y = c.bottom() - 40;
    StepContentLayout::SelectDisk {
        cards,
        refresh_btn: Rect::new(c.right() - 106, c.y + 14, 94, 28),
        boot_systemd: Rect::new(c.x + 12, row_y, 162, 32),
        boot_grub: Rect::new(c.x + 184, row_y, 124, 32),
    }
}

fn compute_install_account_style(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let col_w = (c.w - 28) / 2;
    let f = |idx: i32, id, label, masked| TextFieldRect {
        id,
        label,
        rect: Rect::new(c.x + 6, c.y + 52 + idx * 52, col_w, 30),
        masked,
    };
    let fields = vec![
        f(0, TextFieldId::FullName, "Full Name", false),
        f(1, TextFieldId::Username, "Account Name", false),
        f(2, TextFieldId::Hostname, "Computer Name", false),
        f(3, TextFieldId::Password, "Password", true),
        f(4, TextFieldId::ConfirmPassword, "Verify Password", true),
    ];
    let rx = c.x + col_w + 26;
    let kb_chips = KEYBOARD_PRESETS
        .iter()
        .enumerate()
        .map(|(i, (code, _, _, _))| (i, Rect::new(rx + idx_i32(i) * 84, c.y + 70, 78, 30), *code))
        .collect();
    let theme_chips = [
        (ThemeChoice::Light, "Light"),
        (ThemeChoice::Dark, "Dark"),
        (ThemeChoice::Auto, "Auto"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (choice, lbl))| {
        (
            choice,
            Rect::new(rx + idx_i32(i) * 84, c.y + 144, 78, 30),
            lbl,
        )
    })
    .collect();
    let swatches = RUSTYOS_ACCENT_SWATCHES
        .iter()
        .enumerate()
        .map(|(i, &rgba)| {
            (
                i,
                Rect::new(rx + idx_i32(i) * 38, c.y + 220, 30, 30),
                rgba,
                app.accent_index == i,
            )
        })
        .collect();
    StepContentLayout::AccountAndStyle {
        fields,
        kb_chips,
        theme_chips,
        swatches,
    }
}

fn compute_install_progress(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let emblem = Rect::new(c.x + (c.w - 56) / 2, c.y + 8, 56, 56);
    let progress_rect = Rect::new(c.x + 48, c.y + 134, c.w - 96, 12);
    let toggle_logs_btn = Rect::new(c.x + (c.w - 132) / 2, c.y + 160, 132, 26);
    let log_box = app
        .show_logs
        .then(|| Rect::new(c.x + 24, c.y + 196, c.w - 48, (c.h - 204).max(110)));
    let disk_label = app.selected_disk().map_or_else(
        || String::from("/dev/vda"),
        |d| format!("{} ({})", d.model, d.path),
    );
    let theme_lbl = match app.theme_choice {
        ThemeChoice::Light => "Golden Gate Light",
        ThemeChoice::Dark => "Golden Gate Dark",
        ThemeChoice::Auto => "Golden Gate Auto",
    };
    let summary_lines = vec![
        format!("Destination Drive: {disk_label}"),
        format!(
            "Primary Account: {} ({}) on {}.local",
            app.account.full_name.trim(),
            app.account.username.trim(),
            app.account.hostname.trim()
        ),
        format!(
            "Bootloader & Desktop: {}  •  {}",
            app.bootloader.as_flag(),
            theme_lbl
        ),
    ];
    StepContentLayout::Installing {
        emblem_rect: emblem,
        progress_rect,
        toggle_logs_btn,
        log_box,
        summary_lines,
    }
}

fn compute_oobe_content(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    match app.oobe_step {
        OobeStep::Hello => StepContentLayout::Hello {
            hero_emblem: Rect::new(c.x + (c.w - 56) / 2, c.y + 6, 56, 56),
            rows: LANGUAGE_OPTIONS
                .iter()
                .enumerate()
                .map(|(i, &(loc, name, tz))| OptionRowRect {
                    idx: i,
                    rect: Rect::new(c.x + 48, c.y + 132 + idx_i32(i) * 58, c.w - 96, 50),
                    title: name.to_owned(),
                    subtitle: format!("Locale: {loc}  •  Timezone: {tz}"),
                    selected: app.language_idx == i,
                })
                .collect(),
        },
        OobeStep::Keyboard => StepContentLayout::Keyboard {
            rows: KEYBOARD_PRESETS
                .iter()
                .enumerate()
                .map(|(i, &(layout, label, opt, _))| OptionRowRect {
                    idx: i,
                    rect: Rect::new(c.x + 48, c.y + 84 + idx_i32(i) * 62, c.w - 96, 52),
                    title: label.to_owned(),
                    subtitle: if opt.is_empty() {
                        format!("XKB layout: {layout}")
                    } else {
                        format!("XKB layout: {layout}  •  Switch: Alt+Shift")
                    },
                    selected: app.keyboard_idx == i,
                })
                .collect(),
        },
        OobeStep::Network => StepContentLayout::Network {
            card_rect: Rect::new(c.x + 48, c.y + 88, c.w - 96, 150),
            refresh_btn: Rect::new(c.right() - 158, c.y + 104, 94, 30),
        },
        OobeStep::Account => compute_oobe_account(c),
        OobeStep::Appearance => compute_oobe_appearance(app, c),
        OobeStep::Finish => StepContentLayout::Finish {
            hero_emblem: Rect::new(c.x + (c.w - 64) / 2, c.y + 12, 64, 64),
            summary_card: Rect::new(c.x + 64, c.y + 146, c.w - 128, 166),
        },
    }
}

fn compute_oobe_account(c: Rect) -> StepContentLayout {
    let fields = [
        (TextFieldId::FullName, "Full Name", false),
        (TextFieldId::Username, "Account Name", false),
        (TextFieldId::Hostname, "Computer Name", false),
        (
            TextFieldId::Password,
            "Password (optional in OOBE preview)",
            true,
        ),
        (TextFieldId::ConfirmPassword, "Verify Password", true),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (id, label, masked))| TextFieldRect {
        id,
        label,
        rect: Rect::new(c.x + 64, c.y + 72 + idx_i32(i) * 50, c.w - 128, 28),
        masked,
    })
    .collect();
    StepContentLayout::OobeAccount {
        fields,
        autologin_row: Rect::new(c.x + 64, c.y + 324, c.w - 128, 34),
    }
}

fn compute_oobe_appearance(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let tile_specs = [
        (ThemeChoice::Light, "Light"),
        (ThemeChoice::Dark, "Dark"),
        (ThemeChoice::Auto, "Auto"),
    ];
    let tile_w = 134;
    let stride = 154;
    let start_x = c.x + (c.w - (stride * 2 + tile_w)) / 2;
    let tiles = std::array::from_fn(|i| {
        let rx = start_x + idx_i32(i) * stride;
        ThemeTileRect {
            choice: tile_specs[i].0,
            label: tile_specs[i].1,
            ring_rect: Rect::new(rx, c.y + 72, tile_w, 92),
            preview_rect: Rect::new(rx + 6, c.y + 78, tile_w - 12, 80),
            selected: app.theme_choice == tile_specs[i].0,
        }
    });
    let accent_card = Rect::new(c.x + 64, c.y + 204, c.w - 128, 84);
    let swatches_span = 6 * 52 + 38;
    let swatches_x0 = accent_card.x + (accent_card.w - swatches_span) / 2;
    let swatches = RUSTYOS_ACCENT_SWATCHES
        .iter()
        .enumerate()
        .map(|(i, &rgba)| {
            let sx = swatches_x0 + idx_i32(i) * 52;
            (
                i,
                Rect::new(sx, accent_card.y + 36, 38, 38),
                rgba,
                app.accent_index == i,
            )
        })
        .collect();
    StepContentLayout::Appearance {
        tiles,
        accent_card,
        swatches,
    }
}

#[must_use]
pub(crate) fn hit_test(layout: &InstallerLayout, px: f64, py: f64) -> Option<InstallerHit> {
    if let Some(edge) = hit_resize_edge(layout.win_w, layout.win_h, px, py) {
        return Some(InstallerHit::ResizeEdge(edge));
    }
    let chrome = [
        (layout.traffic_close, InstallerHit::TrafficClose),
        (layout.traffic_min, InstallerHit::TrafficMinimize),
        (layout.traffic_max, InstallerHit::TrafficMaximize),
        (layout.header_rect, InstallerHit::WindowDrag),
        (layout.primary_btn, InstallerHit::PrimaryButton),
    ];
    for (rect, hit) in chrome {
        if rect.contains_f64(px, py) {
            return Some(hit);
        }
    }
    if let Some(back) = layout.back_btn
        && back.contains_f64(px, py)
    {
        return Some(InstallerHit::BackButton);
    }
    hit_content(&layout.content, px, py)
}

fn hit_content(content: &StepContentLayout, px: f64, py: f64) -> Option<InstallerHit> {
    match content {
        StepContentLayout::Welcome { cards, .. } => cards
            .iter()
            .find(|c| c.rect.contains_f64(px, py))
            .map(|c| InstallerHit::WelcomeCard(c.action)),
        StepContentLayout::SelectDisk {
            cards,
            refresh_btn,
            boot_systemd,
            boot_grub,
        } => {
            if refresh_btn.contains_f64(px, py) {
                return Some(InstallerHit::RefreshDisks);
            }
            if boot_systemd.contains_f64(px, py) {
                return Some(InstallerHit::SetBootloader(BootloaderKind::SystemdBoot));
            }
            if boot_grub.contains_f64(px, py) {
                return Some(InstallerHit::SetBootloader(BootloaderKind::Grub));
            }
            cards
                .iter()
                .find(|c| c.selectable && c.rect.contains_f64(px, py))
                .map(|c| InstallerHit::SelectDisk(c.idx))
        }
        StepContentLayout::AccountAndStyle {
            fields,
            kb_chips,
            theme_chips,
            swatches,
        } => fields
            .iter()
            .find(|f| f.rect.contains_f64(px, py))
            .map(|f| InstallerHit::FocusField(f.id))
            .or_else(|| {
                kb_chips
                    .iter()
                    .find(|(_, r, _)| r.contains_f64(px, py))
                    .map(|(i, _, _)| InstallerHit::SelectKeyboard(*i))
            })
            .or_else(|| {
                theme_chips
                    .iter()
                    .find(|(_, r, _)| r.contains_f64(px, py))
                    .map(|(c, _, _)| InstallerHit::SelectTheme(*c))
            })
            .or_else(|| hit_swatches(swatches, px, py)),
        StepContentLayout::Installing {
            toggle_logs_btn, ..
        } => toggle_logs_btn
            .contains_f64(px, py)
            .then_some(InstallerHit::ToggleLogs),
        StepContentLayout::Hello { rows, .. } => rows
            .iter()
            .find(|r| r.rect.contains_f64(px, py))
            .map(|r| InstallerHit::SelectLanguage(r.idx)),
        StepContentLayout::Keyboard { rows } => rows
            .iter()
            .find(|r| r.rect.contains_f64(px, py))
            .map(|r| InstallerHit::SelectKeyboard(r.idx)),
        StepContentLayout::Network { refresh_btn, .. } => refresh_btn
            .contains_f64(px, py)
            .then_some(InstallerHit::RefreshNetwork),
        StepContentLayout::OobeAccount {
            fields,
            autologin_row,
        } => {
            if autologin_row.contains_f64(px, py) {
                return Some(InstallerHit::ToggleAutologin);
            }
            fields
                .iter()
                .find(|f| f.rect.contains_f64(px, py))
                .map(|f| InstallerHit::FocusField(f.id))
        }
        StepContentLayout::Appearance {
            tiles, swatches, ..
        } => tiles
            .iter()
            .find(|t| t.ring_rect.contains_f64(px, py))
            .map(|t| InstallerHit::SelectTheme(t.choice))
            .or_else(|| hit_swatches(swatches, px, py)),
        StepContentLayout::Finish { .. } => None,
    }
}

fn hit_swatches(
    swatches: &[(usize, Rect, [u8; 4], bool)],
    px: f64,
    py: f64,
) -> Option<InstallerHit> {
    swatches
        .iter()
        .find(|(_, r, _, _)| r.contains_f64(px, py))
        .map(|(i, _, _, _)| InstallerHit::SelectAccent(*i))
}

fn hit_resize_edge(w: i32, h: i32, px: f64, py: f64) -> Option<ResizeEdgeZone> {
    let e = f64::from(RESIZE_EDGE_PX);
    let fw = f64::from(w);
    let fh = f64::from(h);
    let left = px < e;
    let right = px >= fw - e;
    let top = py < e;
    let bottom = py >= fh - e;
    match (top, bottom, left, right) {
        (true, _, true, _) => Some(ResizeEdgeZone::TopLeft),
        (true, _, _, true) => Some(ResizeEdgeZone::TopRight),
        (_, true, true, _) => Some(ResizeEdgeZone::BottomLeft),
        (_, true, _, true) => Some(ResizeEdgeZone::BottomRight),
        (true, _, _, _) => Some(ResizeEdgeZone::Top),
        (_, true, _, _) => Some(ResizeEdgeZone::Bottom),
        (_, _, true, _) => Some(ResizeEdgeZone::Left),
        (_, _, _, true) => Some(ResizeEdgeZone::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StartupOptions;

    #[test]
    fn layout_and_hit_testing_cover_install_and_oobe_steps() {
        let mut app = InstallerAppState::new(&StartupOptions {
            mode: WizardMode::Install,
            env_kind: RunEnv::Demo,
            ..Default::default()
        });
        let l0 = compute_layout(&app, 800, 580);
        let p = &l0.primary_btn;
        assert_eq!(
            hit_test(&l0, f64::from(p.x + 10), f64::from(p.y + 10)),
            Some(InstallerHit::PrimaryButton)
        );
        assert_eq!(hit_test(&l0, 22.0, 22.0), Some(InstallerHit::TrafficClose));

        app.mode = WizardMode::Oobe;
        app.oobe_step = OobeStep::Appearance;
        let l_app = compute_layout(&app, 800, 580);
        assert!(matches!(
            l_app.content,
            StepContentLayout::Appearance { .. }
        ));
        if let StepContentLayout::Appearance {
            tiles, swatches, ..
        } = &l_app.content
        {
            assert_eq!(
                hit_test(
                    &l_app,
                    f64::from(tiles[1].ring_rect.x + 10),
                    f64::from(tiles[1].ring_rect.y + 10)
                ),
                Some(InstallerHit::SelectTheme(ThemeChoice::Dark))
            );
            assert_eq!(
                hit_test(
                    &l_app,
                    f64::from(swatches[1].1.x + 12),
                    f64::from(swatches[1].1.y + 12)
                ),
                Some(InstallerHit::SelectAccent(1))
            );
        }
    }
}
