//! Layout geometry and hit-testing for `rusty-installer` (`RUSTYOS-115`).

use crate::state::{
    BootloaderKind, DiskInstallMode, InstallPhase, InstallStep, InstallerAppState,
    KEYBOARD_PRESETS, LANGUAGE_OPTIONS, OobeStep, RunEnv, TIMEZONE_OPTIONS, TextFieldId,
    ThemeChoice, WizardMode,
};
use rusty_services::file_manager::RUSTYOS_ACCENT_SWATCHES;

#[allow(clippy::cast_possible_wrap)]
pub(crate) const WINDOW_CORNER_RADIUS: i32 = rusty_ui::WINDOW_RADIUS as i32;
pub(crate) const CARD_CORNER_RADIUS: i32 = 12;
pub(crate) const PILL_CORNER_RADIUS: i32 = 16;
pub(crate) const HEADER_HEIGHT: i32 = 40;
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
pub(crate) enum InstallerHit {
    ResizeEdge(ResizeEdgeZone),
    WindowDrag,
    TrafficClose,
    TrafficMinimize,
    TrafficMaximize,
    BackButton,
    OtherButton,
    PrimaryButton,
    TimezoneRow(usize),
    SelectDiskMode(DiskInstallMode),
    SelectDisk(usize),
    RefreshDisks,
    SetBootloader(BootloaderKind),
    SelectLanguage(usize),
    SelectKeyboard(usize),
    RefreshNetwork,
    ToggleWifi,
    SelectWifi(usize),
    ConnectWifi,
    FocusField(TextFieldId),
    ToggleAutologin,
    SelectTheme(ThemeChoice),
    SelectAccent(usize),
    ToggleLogs,
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
pub(crate) struct WifiRowRect {
    pub idx: usize,
    pub rect: Rect,
    pub ap: rusty_services::WifiAccessPoint,
    pub selected: bool,
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
pub(crate) struct DiskTileRect {
    pub idx: usize,
    pub rect: Rect,
    pub title: String,
    pub subtitle: String,
    pub badge: &'static str,
    pub selected: bool,
    pub selectable: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum StepContentLayout {
    SelectDisk {
        mode_whole: Rect,
        mode_dualboot: Rect,
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
        wifi_toggle_btn: Rect,
        refresh_btn: Rect,
        wifi_rows: Vec<WifiRowRect>,
        password_field: Option<TextFieldRect>,
        connect_btn: Option<Rect>,
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
    WizardWelcome {
        logo_rect: Rect,
    },
    WizardDiskTiles {
        tiles: Vec<DiskTileRect>,
    },
    WizardTimezone {
        rows: Vec<OptionRowRect>,
    },
    WizardSummary {
        card_rect: Rect,
        lines: Vec<String>,
        warning_rect: Rect,
    },
    WizardResult {
        badge_rect: Rect,
        success: bool,
        message: String,
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
    pub other_btn: Option<Rect>,
    pub other_label: &'static str,
    pub back_btn: Option<Rect>,
    pub back_label: &'static str,
    pub primary_btn: Rect,
    pub primary_label: &'static str,
    pub content: StepContentLayout,
}

#[must_use]
#[allow(clippy::cast_possible_wrap)]
pub(crate) fn compute_layout(app: &InstallerAppState, width: u32, height: u32) -> InstallerLayout {
    let win_w = i32::try_from(width).unwrap_or(880).max(MIN_WIDTH);
    let win_h = i32::try_from(height).unwrap_or(560).max(MIN_HEIGHT);
    let is_install = app.mode == WizardMode::Install;
    let tl = rusty_ui::TrafficLightButtons::default_csd();
    let btn_d = i32::try_from(tl.close_rect.width).unwrap_or(14);
    let (traffic_close, traffic_min, traffic_max) = (
        Rect::new(tl.close_rect.x, tl.close_rect.y, btn_d, btn_d),
        Rect::new(tl.minimize_rect.x, tl.minimize_rect.y, btn_d, btn_d),
        Rect::new(tl.maximize_rect.x, tl.maximize_rect.y, btn_d, btn_d),
    );
    let header_rect = Rect::new(0, 0, win_w, HEADER_HEIGHT);
    let content_rect = Rect::new(
        36,
        HEADER_HEIGHT + 6,
        win_w - 72,
        win_h - HEADER_HEIGHT - FOOTER_HEIGHT - 12,
    );
    let footer_rect = Rect::new(0, win_h - FOOTER_HEIGHT, win_w, FOOTER_HEIGHT);

    let (step_count, active_step_idx, show_back, back_label, primary_label) = footer_meta(app);
    let primary_w = if primary_label.len() > 14 {
        164
    } else if is_install {
        112
    } else {
        132
    };
    let (primary_btn, back_btn) = if is_install {
        let btn_y = win_h - 32 - 36;
        let p_btn = Rect::new(win_w - 32 - primary_w, btn_y, primary_w, 36);
        let b_btn = show_back.then(|| Rect::new(p_btn.x - 12 - 112, btn_y, 112, 36));
        (p_btn, b_btn)
    } else {
        let p_btn = Rect::new(win_w - 36 - primary_w, footer_rect.y + 13, primary_w, 32);
        let b_btn = show_back.then(|| Rect::new(36, footer_rect.y + 13, 98, 32));
        (p_btn, b_btn)
    };

    let (other_btn, other_label) = if is_install {
        let btn_y = win_h - 32 - 36;
        match app.install_step {
            InstallStep::Welcome => (Some(Rect::new(32, btn_y, 132, 36)), "Try RustyOS"),
            InstallStep::Disk => (Some(Rect::new(32, btn_y, 176, 36)), "Manual Partitioning"),
            InstallStep::Result => {
                let label = if app.install_error.is_some() {
                    "Close"
                } else {
                    "Keep Trying"
                };
                (Some(Rect::new(32, btn_y, 128, 36)), label)
            }
            _ => (None, ""),
        }
    } else {
        (None, "")
    };

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
        other_btn,
        other_label,
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
            let show_back = !matches!(
                app.install_step,
                InstallStep::Language | InstallStep::Progress | InstallStep::Result
            );
            let primary = match app.install_step {
                InstallStep::Language
                | InstallStep::Disk
                | InstallStep::Partition
                | InstallStep::Account
                | InstallStep::TimeZone => "Next",
                InstallStep::Welcome | InstallStep::Summary => "Install",
                InstallStep::Progress if app.phase == InstallPhase::Finished => "Continue",
                InstallStep::Progress => "Installing...",
                InstallStep::Result => {
                    if app.dry_run || app.env_kind == RunEnv::Demo {
                        "Close"
                    } else {
                        "Restart Now"
                    }
                }
            };
            (9, app.install_step.index(), show_back, "Back", primary)
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
        InstallStep::Language => compute_hello_content(app, c),
        InstallStep::Welcome => StepContentLayout::WizardWelcome {
            logo_rect: Rect::new(c.x + (c.w - 168) / 2, c.y + 72, 168, 78),
        },
        InstallStep::Disk => compute_wizard_disk_tiles(app, c),
        InstallStep::Partition => compute_install_select_disk(app, c),
        InstallStep::Account => compute_install_account_style(app, c),
        InstallStep::TimeZone => compute_wizard_timezone(app, c),
        InstallStep::Summary => compute_wizard_summary(app, c),
        InstallStep::Progress => compute_install_progress(app, c),
        InstallStep::Result => StepContentLayout::WizardResult {
            badge_rect: Rect::new(c.x + (c.w - 72) / 2, c.y + 116, 72, 72),
            success: app.install_error.is_none(),
            message: app.install_error.clone().unwrap_or_else(|| {
                "RustyOS has been installed. Restart your computer to begin.".to_owned()
            }),
        },
    }
}

fn compute_wizard_disk_tiles(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let count = app.disks.len().clamp(1, 4);
    let tile_w = 160;
    let tile_h = 176;
    let gap = 12;
    let total_w = idx_i32(count) * tile_w + idx_i32(count.saturating_sub(1)) * gap;
    let start_x = c.x + (c.w - total_w) / 2;
    let y = c.y + 132;
    let tiles = app
        .disks
        .iter()
        .enumerate()
        .take(4)
        .map(|(i, d)| DiskTileRect {
            idx: i,
            rect: Rect::new(start_x + idx_i32(i) * (tile_w + gap), y, tile_w, tile_h),
            title: if d.model.is_empty() {
                d.path.clone()
            } else {
                d.model.clone()
            },
            subtitle: format!("{}  •  {}", d.size_label, d.path),
            badge: if d.removable { "USB" } else { "DISK" },
            selected: app.selected_disk_idx == Some(i),
            selectable: d.is_selectable(),
        })
        .collect();
    StepContentLayout::WizardDiskTiles { tiles }
}

fn compute_wizard_timezone(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let card_w = 480.min(c.w);
    let card_x = c.x + (c.w - card_w) / 2;
    let rows = TIMEZONE_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, &(tz, desc))| OptionRowRect {
            idx: i,
            rect: Rect::new(card_x + 4, c.y + 116 + idx_i32(i) * 48, card_w - 8, 42),
            title: tz.to_owned(),
            subtitle: desc.to_owned(),
            selected: app.timezone_idx == i,
        })
        .collect();
    StepContentLayout::WizardTimezone { rows }
}

fn compute_wizard_summary(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let card_w = 480.min(c.w);
    let card_x = c.x + (c.w - card_w) / 2;
    let card_rect = Rect::new(card_x, c.y + 108, card_w, 168);
    let warning_rect = Rect::new(card_x, card_rect.bottom() + 14, card_w, 56);
    let disk_lbl = app.selected_disk().map_or_else(
        || "None".to_owned(),
        |d| format!("{} ({})", d.path, d.size_label),
    );
    let tz = TIMEZONE_OPTIONS
        .get(app.timezone_idx)
        .map_or("UTC", |&(t, _)| t);
    let lines = vec![
        format!("Target Disk:  {disk_lbl}"),
        format!(
            "Install Source:  {}",
            rusty_services::detect_install_source().display_label()
        ),
        format!(
            "User Account:  {} ({})",
            app.account.full_name, app.account.username
        ),
        format!(
            "Computer Name:  {}  •  Time Zone: {tz}",
            app.account.hostname
        ),
    ];
    StepContentLayout::WizardSummary {
        card_rect,
        lines,
        warning_rect,
    }
}

fn compute_hello_content(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let card_w = 480.min(c.w);
    let card_x = c.x + (c.w - card_w) / 2;
    StepContentLayout::Hello {
        hero_emblem: Rect::new(c.x + (c.w - 64) / 2, c.y + 4, 64, 64),
        rows: LANGUAGE_OPTIONS
            .iter()
            .enumerate()
            .map(|(i, &(loc, name, tz))| OptionRowRect {
                idx: i,
                rect: Rect::new(card_x + 4, c.y + 150 + idx_i32(i) * 52, card_w - 8, 46),
                title: name.to_owned(),
                subtitle: format!("Locale: {loc}  •  Timezone: {tz}"),
                selected: app.language_idx == i,
            })
            .collect(),
    }
}

fn compute_keyboard_content(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    StepContentLayout::Keyboard {
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
    }
}

fn compute_network_content(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let card_rect = Rect::new(c.x + 36, c.y + 72, c.w - 72, 74);
    let refresh_btn = Rect::new(card_rect.right() - 102, card_rect.y + 22, 86, 30);
    let wifi_toggle_btn = Rect::new(refresh_btn.x - 110, card_rect.y + 22, 98, 30);

    let list_y = card_rect.bottom() + 12;
    let max_rows = usize::try_from(((c.bottom() - list_y - 54) / 42).clamp(1, 4)).unwrap_or(1);
    let wifi_rows: Vec<WifiRowRect> = app
        .network
        .wifi_networks
        .iter()
        .take(max_rows)
        .enumerate()
        .map(|(i, ap)| WifiRowRect {
            idx: i,
            rect: Rect::new(c.x + 36, list_y + idx_i32(i) * 42, c.w - 72, 36),
            ap: ap.clone(),
            selected: app.selected_wifi_idx == Some(i),
        })
        .collect();

    let selected_ap = app.selected_wifi();
    let row_end_y = list_y + idx_i32(wifi_rows.len()) * 42;
    let action_y = (row_end_y + 12).min(c.bottom() - 40);

    let password_field = selected_ap
        .filter(|ap| ap.secure && !ap.connected)
        .map(|_| TextFieldRect {
            id: TextFieldId::WifiPassword,
            label: "Wi-Fi Password",
            rect: Rect::new(c.x + 36, action_y, c.w - 196, 30),
            masked: true,
        });
    let connect_btn = selected_ap
        .filter(|ap| !ap.connected)
        .map(|_| Rect::new(c.right() - 148, action_y, 112, 30));

    StepContentLayout::Network {
        card_rect,
        wifi_toggle_btn,
        refresh_btn,
        wifi_rows,
        password_field,
        connect_btn,
    }
}

fn compute_install_select_disk(app: &InstallerAppState, c: Rect) -> StepContentLayout {
    let cards: Vec<DiskCardRect> = match app.disk_mode {
        DiskInstallMode::WholeDisk => app
            .disks
            .iter()
            .enumerate()
            .take(4)
            .map(|(i, d)| DiskCardRect {
                idx: i,
                rect: Rect::new(c.x + 12, c.y + 82 + idx_i32(i) * 64, c.w - 24, 58),
                selected: app.selected_disk_idx == Some(i),
                selectable: d.is_selectable(),
            })
            .collect(),
        DiskInstallMode::DualBoot => app
            .dualboot_targets
            .iter()
            .enumerate()
            .take(4)
            .map(|(i, t)| DiskCardRect {
                idx: i,
                rect: Rect::new(c.x + 12, c.y + 82 + idx_i32(i) * 64, c.w - 24, 58),
                selected: app.selected_dualboot_idx == Some(i),
                selectable: t.selectable,
            })
            .collect(),
    };
    let row_y = c.bottom() - 40;
    StepContentLayout::SelectDisk {
        mode_whole: Rect::new(c.x + 12, c.y + 42, 166, 30),
        mode_dualboot: Rect::new(c.x + 188, c.y + 42, 206, 30),
        cards,
        refresh_btn: Rect::new(c.right() - 106, c.y + 10, 94, 28),
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
    let target_line = match app.disk_mode {
        DiskInstallMode::WholeDisk => app.selected_disk().map_or_else(
            || String::from("Destination Drive: /dev/vda (Erase Entire Disk)"),
            |d| format!("Erase Whole Disk:   {} ({})", d.model, d.path),
        ),
        DiskInstallMode::DualBoot => app.selected_dualboot().map_or_else(
            || String::from("Dual-Boot Target:   (none)"),
            |t| {
                let efi_note = if let Some(esp) = &t.efi_part {
                    format!("reuses EFI {esp}")
                } else {
                    String::from("dedicated 512M EFI")
                };
                format!("Dual-Boot Target:   {} ({efi_note})", t.title)
            },
        ),
    };
    let theme_lbl = match app.theme_choice {
        ThemeChoice::Light => "RustyOS Light",
        ThemeChoice::Dark => "RustyOS Dark",
        ThemeChoice::Auto => "RustyOS Auto",
    };
    let summary_lines = vec![
        target_line,
        format!(
            "Primary Account: {} ({}) on {}.local",
            app.account.full_name.trim(),
            app.account.username.trim(),
            app.account.hostname.trim()
        ),
        format!(
            "Bootloader & Source: {}  •  {}  •  {}",
            app.bootloader.as_flag(),
            rusty_services::detect_install_source().display_label(),
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
        OobeStep::Hello => compute_hello_content(app, c),
        OobeStep::Keyboard => compute_keyboard_content(app, c),
        OobeStep::Network => compute_network_content(app, c),
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
    if let Some(other) = layout.other_btn
        && other.contains_f64(px, py)
    {
        return Some(InstallerHit::OtherButton);
    }
    if let Some(back) = layout.back_btn
        && back.contains_f64(px, py)
    {
        return Some(InstallerHit::BackButton);
    }
    if let Some(hit) = hit_content(&layout.content, px, py) {
        return Some(hit);
    }
    if layout.traffic_close.w == 0 {
        return Some(InstallerHit::WindowDrag);
    }
    None
}

#[allow(clippy::too_many_lines)]
fn hit_content(content: &StepContentLayout, px: f64, py: f64) -> Option<InstallerHit> {
    match content {
        StepContentLayout::SelectDisk {
            mode_whole,
            mode_dualboot,
            cards,
            refresh_btn,
            boot_systemd,
            boot_grub,
        } => hit_select_disk_content(
            [*mode_whole, *mode_dualboot],
            cards,
            *refresh_btn,
            [*boot_systemd, *boot_grub],
            px,
            py,
        ),
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
        StepContentLayout::Network {
            wifi_toggle_btn,
            refresh_btn,
            wifi_rows,
            password_field,
            connect_btn,
            ..
        } => hit_network_content(
            *wifi_toggle_btn,
            *refresh_btn,
            wifi_rows,
            password_field.as_ref(),
            *connect_btn,
            px,
            py,
        ),
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
        StepContentLayout::Finish { .. }
        | StepContentLayout::WizardWelcome { .. }
        | StepContentLayout::WizardSummary { .. }
        | StepContentLayout::WizardResult { .. } => None,
        StepContentLayout::WizardDiskTiles { tiles } => tiles
            .iter()
            .find(|t| t.selectable && t.rect.contains_f64(px, py))
            .map(|t| InstallerHit::SelectDisk(t.idx)),
        StepContentLayout::WizardTimezone { rows } => rows
            .iter()
            .find(|r| r.rect.contains_f64(px, py))
            .map(|r| InstallerHit::TimezoneRow(r.idx)),
    }
}

fn hit_select_disk_content(
    modes: [Rect; 2],
    cards: &[DiskCardRect],
    refresh_btn: Rect,
    bootloaders: [Rect; 2],
    px: f64,
    py: f64,
) -> Option<InstallerHit> {
    if modes[0].contains_f64(px, py) {
        return Some(InstallerHit::SelectDiskMode(DiskInstallMode::WholeDisk));
    }
    if modes[1].contains_f64(px, py) {
        return Some(InstallerHit::SelectDiskMode(DiskInstallMode::DualBoot));
    }
    if refresh_btn.contains_f64(px, py) {
        return Some(InstallerHit::RefreshDisks);
    }
    if bootloaders[0].contains_f64(px, py) {
        return Some(InstallerHit::SetBootloader(BootloaderKind::SystemdBoot));
    }
    if bootloaders[1].contains_f64(px, py) {
        return Some(InstallerHit::SetBootloader(BootloaderKind::Grub));
    }
    cards
        .iter()
        .find(|c| c.selectable && c.rect.contains_f64(px, py))
        .map(|c| InstallerHit::SelectDisk(c.idx))
}

fn hit_network_content(
    wifi_toggle_btn: Rect,
    refresh_btn: Rect,
    wifi_rows: &[WifiRowRect],
    password_field: Option<&TextFieldRect>,
    connect_btn: Option<Rect>,
    px: f64,
    py: f64,
) -> Option<InstallerHit> {
    if wifi_toggle_btn.contains_f64(px, py) {
        return Some(InstallerHit::ToggleWifi);
    }
    if refresh_btn.contains_f64(px, py) {
        return Some(InstallerHit::RefreshNetwork);
    }
    if let Some(btn) = connect_btn
        && btn.contains_f64(px, py)
    {
        return Some(InstallerHit::ConnectWifi);
    }
    if let Some(pf) = password_field
        && pf.rect.contains_f64(px, py)
    {
        return Some(InstallerHit::FocusField(pf.id));
    }
    wifi_rows
        .iter()
        .find(|r| r.rect.contains_f64(px, py))
        .map(|r| InstallerHit::SelectWifi(r.idx))
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

        app.install_step = InstallStep::Partition;
        app.disk_mode = DiskInstallMode::DualBoot;
        let l_disk = compute_layout(&app, 768, 500);
        if let StepContentLayout::SelectDisk {
            mode_dualboot,
            cards,
            ..
        } = &l_disk.content
        {
            assert_eq!(
                hit_test(
                    &l_disk,
                    f64::from(mode_dualboot.x + 10),
                    f64::from(mode_dualboot.y + 10)
                ),
                Some(InstallerHit::SelectDiskMode(DiskInstallMode::DualBoot))
            );
            assert_ne!(cards.len(), 0);
        }

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
