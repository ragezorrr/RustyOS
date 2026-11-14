//! `TopBar` layout/hit-testing/rendering, global menus, Notification HUD, and OSD pill.

use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::CompositorRenderElement;
use super::tokens::{
    COLOR_ACCENT, COLOR_ACCENT_MUTED, COLOR_BORDER, COLOR_MUTED, COLOR_SUBTLE, COLOR_SUCCESS,
    COLOR_SURFACE_ELEVATED, COLOR_SURFACE_OVERLAY, COLOR_WARNING, DOCK_HEIGHT, DOCK_MARGIN_BOTTOM,
    TOP_BAR_APP_X, TOP_BAR_FONT_SIZE, TOP_BAR_HEIGHT, TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y,
    TOP_BAR_LOGO_W, TOP_BAR_LOGO_X, WS_BTN_STRIDE, WS_BTN_WIDTH, make_solid_element,
    push_liquid_glass_panel, push_rounded_glass_box,
};

/// Compact system tray item metadata rendered inside `TopBar` (`RUSTYOS-032`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarTrayItem<'a> {
    /// Unique identifier (`<bus><path>`).
    pub key: &'a str,
    /// Short 1-2 char monogram (`"NM"`, `"TG"`, etc.).
    pub label: &'a str,
    /// Optional XDG / filesystem icon name (`RUSTYOS-033`).
    pub icon_name: &'a str,
    /// Optional decoded SNI ARGB32 icon pixmap (`RUSTYOS-033`).
    pub icon_pixmap: Option<&'a crate::icons::TrayPixmap>,
    /// Attention alert requested by applet (`NeedsAttention`).
    pub attention: bool,
    /// Passive/idle state (`Passive`).
    pub passive: bool,
}

/// Visibility and telemetry flags controlling optional items in the `TopBar` (`RUSTYOS-049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct TopBarLayoutOptions {
    /// Whether a hardware battery is present.
    pub has_battery: bool,
    /// Active unread desktop notification count.
    pub notification_count: u32,
    /// Show standalone volume glyph left of Battery/Wi-Fi.
    pub show_volume: bool,
    /// Show keyboard input source indicator (`EN`/`RU`).
    pub show_input_source: bool,
    /// Show workspace numbers `1..=9` and layout badge.
    pub show_workspaces: bool,
    /// Show `StatusNotifierItem` tray applets.
    pub show_tray: bool,
}

/// Live hardware/session telemetry displayed inside `TopBar` (`RUSTYOS-021`, `RUSTYOS-048`, `RUSTYOS-049`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct TopBarTelemetry<'a> {
    /// Local wall-clock date/time label shown in `TopBar` (`"Wed Apr 1  9:41 AM"`).
    pub clock_hhmm: &'a str,
    /// Battery percentage (`0..=100`).
    pub battery_percent: u8,
    /// Whether a hardware battery is present.
    pub has_battery: bool,
    /// Whether AC adapter is plugged in (`RUSTYOS-049`).
    pub ac_online: bool,
    /// Whether battery is actively charging (`RUSTYOS-052`).
    pub battery_charging: bool,
    /// Whether an active network interface is up.
    pub net_connected: bool,
    /// Live Wi-Fi signal strength percentage (`0..=100`, `RUSTYOS-052`).
    pub wifi_signal_percent: Option<u8>,
    /// Master output volume (`0..=100`).
    pub volume_percent: u8,
    /// Master audio output is muted.
    pub audio_muted: bool,
    /// Whether the Quick Settings popover is currently open.
    pub quick_settings_open: bool,
    /// Number of active/unread desktop notifications (`RUSTYOS-024`).
    pub notification_count: u32,
    /// Active keyboard layout badge code (`"EN"`, `"RU"`, etc.) (`RUSTYOS-029`).
    pub kbd_layout: &'a str,
    /// Registered `StatusNotifierItem` tray badges rendered when `show_tray == true` (`RUSTYOS-032`).
    pub tray_items: &'a [TopBarTrayItem<'a>],
    /// Currently open `TopBar` left menu dropdown if any (`RUSTYOS-048`).
    pub active_menu: Option<TopBarMenuHit>,
    /// Currently hovered item index (`0..`) inside the open `TopBar` dropdown (`RUSTYOS-051`).
    pub menu_hover_row: Option<usize>,
    /// Global shell glass opacity multiplier (`0.35..=1.0`, default `0.78`) (`RUSTYOS-048`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels mode (`RUSTYOS-048`).
    pub reduce_transparency: bool,
    /// Show workspace digits `1..=9` and `Float`/`Tile` pill (`RUSTYOS-049`, default `false`).
    pub show_workspaces: bool,
    /// Show host SNI system tray icons in Menu Bar (`RUSTYOS-049`, default `false`).
    pub show_tray: bool,
    /// Show keyboard layout badge in Menu Bar (`RUSTYOS-049`, default `false`).
    pub show_input_source: bool,
    /// Show standalone speaker volume icon in Menu Bar (`RUSTYOS-049`, default `false`).
    pub show_volume: bool,
}

impl TopBarTelemetry<'_> {
    /// Extracts [`TopBarLayoutOptions`] matching the active telemetry and UI toggles.
    #[must_use]
    pub const fn layout_options(&self) -> TopBarLayoutOptions {
        TopBarLayoutOptions {
            has_battery: self.has_battery,
            notification_count: self.notification_count,
            show_volume: self.show_volume,
            show_input_source: self.show_input_source,
            show_workspaces: self.show_workspaces,
            show_tray: self.show_tray,
        }
    }
}

impl Default for TopBarTelemetry<'static> {
    fn default() -> Self {
        Self {
            clock_hhmm: "Wed Apr 1  9:41 AM",
            battery_percent: 100,
            has_battery: false,
            ac_online: false,
            battery_charging: false,
            net_connected: true,
            wifi_signal_percent: Some(100),
            volume_percent: 75,
            audio_muted: false,
            quick_settings_open: false,
            notification_count: 0,
            kbd_layout: "EN",
            tray_items: &[],
            active_menu: None,
            menu_hover_row: None,
            glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
            reduce_transparency: false,
            show_workspaces: false,
            show_tray: false,
            show_input_source: false,
            show_volume: false,
        }
    }
}

/// Active notification toast view rendered in the top-right Notification HUD (`RUSTYOS-024`).
#[derive(Debug, Clone)]
pub struct NotificationToastView<'a> {
    /// Unique notification identifier.
    pub id: u64,
    /// Urgency level controlling accent color.
    pub urgency: rusty_ipc::Urgency,
    /// Short notification title.
    pub title: &'a str,
    /// Notification body text.
    pub body: &'a str,
    /// True when the pointer is hovering this notification card (`RUSTYOS-051`).
    pub hovered: bool,
}

/// Interactive click target inside the `TopBar` Left Menu area (`RUSTYOS-038`/`049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuHit {
    /// System emblem (`UiGlyphKind::RustyLogo`).
    SystemLogo,
    /// Active application name menu.
    ActiveApp,
    /// Global File menu.
    File,
    /// Global Edit menu.
    Edit,
    /// Global View menu.
    View,
    /// Global Go menu (`RUSTYOS-049`).
    Go,
    /// Global Window menu.
    Window,
    /// Global Help menu (`RUSTYOS-049`).
    Help,
}

/// Action triggered from an open `TopBar` menu dropdown popover (`RUSTYOS-048`/`049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuAction {
    /// Show concise RustyOS notification card.
    AboutRustyOs,
    /// Open built-in System Settings overlay (`Super+,`) (`RUSTYOS-061`).
    OpenSettings,
    /// Reload `~/.config/rustyos/config.toml` immediately (`Super+Shift+R`) (`RUSTYOS-061`).
    ReloadConfig,
    /// Show concise keyboard shortcuts reference card (`RUSTYOS-049`).
    ShowShortcuts,
    /// Toggle Workspace Overview.
    OpenOverview,
    /// Toggle Control Center (`QuickSettings`).
    OpenControlCenter,
    /// Toggle Spotlight Search.
    OpenSpotlight,
    /// Launch preferred terminal emulator.
    NewTerminal,
    /// Launch preferred file manager.
    OpenFiles,
    /// Switch to next workspace (`RUSTYOS-049`).
    NextWorkspace,
    /// Switch to previous workspace (`RUSTYOS-049`).
    PrevWorkspace,
    /// Switch between Floating and Tiling layout modes.
    ToggleLayout,
    /// Toggle `reduced_motion`.
    ToggleReducedMotion,
    /// Toggle `reduce_transparency`.
    ToggleReduceTransparency,
    /// Request non-blocking telemetry refresh.
    RefreshMetrics,
    /// Engage session Lock Screen.
    LockSession,
    /// Cycle focus to next window.
    FocusNextWindow,
    /// Maximize / restore focused window.
    ToggleMaximizeFocused,
    /// Minimize focused window.
    MinimizeFocused,
    /// Close focused window.
    CloseFocused,
}

/// Corner radius for floating Menu Bar dropdowns and Notification cards (`DESIGN.md` §12.3–12.6).
pub const MENU_POPOVER_RADIUS: i32 = 16;

/// Rich menu item entry with leading SDF glyph, optional shortcut label, and separator (`RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuEntry {
    /// Clickable action row.
    Item {
        /// Visible label.
        label: &'static str,
        /// Leading `4x4` SSAA SDF glyph.
        icon: crate::surfaces::UiGlyphKind,
        /// Optional right-aligned real keybinding hint.
        shortcut: Option<&'static str>,
        /// Target compositor / WM action.
        action: TopBarMenuAction,
    },
    /// Horizontal hairline separator between logical groups.
    Separator,
}

/// Hit-test outcome inside an open `TopBar` menu dropdown popover (`RUSTYOS-048`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuPopoverClick {
    /// Specific menu action row clicked.
    Action(TopBarMenuAction),
    /// Click inside popover frame (absorb).
    Inside,
    /// Click outside popover frame (dismiss).
    Outside,
}

/// Active transient On-Screen Display (OSD) indicator pill (`RUSTYOS-031`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsdRenderState<'a> {
    /// Primary label (e.g., `"Volume"`, `"Brightness"`, `"Play / Pause"`, `"Screenshot"`).
    pub label: &'a str,
    /// Optional numeric percentage (`0..=100`) rendered as a progress bar.
    pub value_percent: Option<u8>,
    /// True when audio is muted (grays out the bar fill).
    pub muted: bool,
}

/// Dynamic left-side geometry for `TopBar` app name, global menus, optional workspace strip, and layout badge (`RUSTYOS-048`/`049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarLeftLayout {
    pub app_x: i32,
    pub app_w: i32,
    pub file_x: i32,
    pub file_w: i32,
    pub edit_x: i32,
    pub edit_w: i32,
    pub view_x: i32,
    pub view_w: i32,
    pub go_x: i32,
    pub go_w: i32,
    pub win_x: i32,
    pub win_w: i32,
    pub help_x: i32,
    pub help_w: i32,
    pub show_workspaces: bool,
    pub separator_x: i32,
    pub ws_start_x: i32,
    pub layout_x: i32,
    pub layout_w: i32,
}

pub(super) fn measure_menu_box_width(label: &str) -> i32 {
    (crate::text::measure_ui_text_width(label, TOP_BAR_FONT_SIZE, crate::text::FontWeight::Regular)
        + 14)
        .max(26)
}

/// Computes `TopBar` left-side x coordinates (`macOS` Menu Bar rhythm: `Logo  App  File  Edit  View  Go  Window  Help`) (`RUSTYOS-048`/`049`).
#[must_use]
pub fn compute_top_bar_left_layout(active_app_name: &str) -> TopBarLeftLayout {
    compute_top_bar_left_layout_with_options(active_app_name, false)
}

/// Computes `TopBar` left-side layout with explicit workspace-strip visibility (`RUSTYOS-049`).
#[must_use]
pub fn compute_top_bar_left_layout_with_options(
    active_app_name: &str,
    show_workspaces: bool,
) -> TopBarLeftLayout {
    let app_w = (crate::text::measure_ui_text_width(
        active_app_name,
        TOP_BAR_FONT_SIZE,
        crate::text::FontWeight::Bold,
    ) + 12)
        .clamp(44, 168);
    let file_x = TOP_BAR_APP_X + app_w;
    let file_w = measure_menu_box_width("File");
    let edit_x = file_x + file_w;
    let edit_w = measure_menu_box_width("Edit");
    let view_x = edit_x + edit_w;
    let view_w = measure_menu_box_width("View");
    let go_x = view_x + view_w;
    let go_w = measure_menu_box_width("Go");
    let win_x = go_x + go_w;
    let win_w = measure_menu_box_width("Window");
    let help_x = win_x + win_w;
    let help_w = measure_menu_box_width("Help");
    let separator_x = help_x + help_w + 10;
    let ws_start_x = separator_x + 12;
    let layout_x = ws_start_x + 9 * WS_BTN_STRIDE + 8;
    TopBarLeftLayout {
        app_x: TOP_BAR_APP_X,
        app_w,
        file_x,
        file_w,
        edit_x,
        edit_w,
        view_x,
        view_w,
        go_x,
        go_w,
        win_x,
        win_w,
        help_x,
        help_w,
        show_workspaces,
        separator_x,
        ws_start_x,
        layout_x,
        layout_w: 50,
    }
}

/// Dynamic right-side geometry for `TopBar` matching macOS order (`Battery · Wi-Fi · Search · Control Center · Clock`) (`RUSTYOS-048`/`049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarRightLayout {
    pub search_x: i32,
    pub search_w: i32,
    pub wifi_x: i32,
    pub volume_x: Option<i32>,
    pub battery_x: Option<i32>,
    pub control_center_x: i32,
    pub control_center_w: i32,
    pub clock_x: i32,
    pub clock_w: i32,
    pub status_pill_x: i32,
    pub status_pill_w: i32,
    pub status_hit_x: i32,
    pub status_hit_w: i32,
    pub notif_x: i32,
    pub notif_w: i32,
    pub kbd_x: Option<i32>,
    pub kbd_w: i32,
    pub tray_anchor_x: i32,
}

/// Computes compact right-aligned `TopBar` positions for `RUSTYOS-049` defaults (`show_* = false`).
#[must_use]
pub fn compute_top_bar_right_layout(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
) -> TopBarRightLayout {
    compute_top_bar_right_layout_with_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            ..TopBarLayoutOptions::default()
        },
    )
}

/// Computes right-aligned `TopBar` geometry from exact `Inter` text width and visibility options (`RUSTYOS-049`).
#[must_use]
pub fn compute_top_bar_right_layout_with_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
) -> TopBarRightLayout {
    let clock_w = crate::text::measure_ui_text_width(
        clock_text,
        TOP_BAR_FONT_SIZE,
        crate::text::FontWeight::Regular,
    )
    .clamp(42, 220);
    let right_edge = screen_width - 10;
    let clock_x = (right_edge - clock_w).max(screen_width - 236);

    let control_center_w = 20;
    let control_center_x = clock_x - 6 - control_center_w;

    let search_w = 18;
    let search_x = control_center_x - 10 - search_w;

    let wifi_w = 20;
    let wifi_x = search_x - 10 - wifi_w;
    let mut left_cursor = wifi_x;

    let battery_w = 26;
    let battery_x = if opts.has_battery {
        let bx = left_cursor - 10 - battery_w;
        left_cursor = bx;
        Some(bx)
    } else {
        None
    };

    let volume_w = 18;
    let volume_x = if opts.show_volume {
        let vx = left_cursor - 10 - volume_w;
        left_cursor = vx;
        Some(vx)
    } else {
        None
    };

    let status_pill_x = control_center_x - 3;
    let status_pill_w = control_center_w + 6;
    let status_hit_x = left_cursor - 4;
    let status_hit_w = (right_edge - status_hit_x).max(88);

    let (notif_x, notif_w) = if opts.notification_count > 0 {
        let nw = 38;
        let nx = left_cursor - 8 - nw;
        left_cursor = nx;
        (nx, nw)
    } else {
        (left_cursor, 0)
    };

    let kbd_w = 28;
    let kbd_x = if opts.show_input_source {
        let kx = left_cursor - 8 - kbd_w;
        left_cursor = kx;
        Some(kx)
    } else {
        None
    };

    TopBarRightLayout {
        search_x,
        search_w,
        wifi_x,
        volume_x,
        battery_x,
        control_center_x,
        control_center_w,
        clock_x,
        clock_w,
        status_pill_x,
        status_pill_w,
        status_hit_x,
        status_hit_w,
        notif_x,
        notif_w,
        kbd_x,
        kbd_w,
        tray_anchor_x: left_cursor,
    }
}

pub(super) fn status_pill_rect(screen_width: i32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout(screen_width, false, 0, "12:00");
    (
        layout.status_pill_x,
        TOP_BAR_ITEM_Y,
        layout.status_pill_w,
        TOP_BAR_ITEM_H,
    )
}

#[cfg(test)]
pub(super) fn notification_badge_rect(screen_width: i32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout(screen_width, false, 1, "12:00");
    (
        layout.notif_x,
        TOP_BAR_ITEM_Y,
        layout.notif_w,
        TOP_BAR_ITEM_H,
    )
}

#[cfg(test)]
pub(super) fn kbd_badge_rect(screen_width: i32, notification_count: u32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout_with_options(
        screen_width,
        "12:00",
        TopBarLayoutOptions {
            notification_count,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
    );
    (
        layout.kbd_x.unwrap_or(layout.tray_anchor_x),
        TOP_BAR_ITEM_Y,
        layout.kbd_w,
        TOP_BAR_ITEM_H,
    )
}

/// Maximum number of system tray applets displayed simultaneously in `TopBar` (`RUSTYOS-032`).
pub const MAX_TOP_BAR_TRAY_ITEMS: usize = 6;

/// Computes `TopBar` tray pill rectangles when tray visibility is enabled (`RUSTYOS-032`/`049`).
#[must_use]
pub fn tray_slot_rects(
    screen_width: i32,
    notification_count: u32,
    tray_count: usize,
) -> Vec<(i32, i32, i32, i32)> {
    tray_slot_rects_with_options(
        screen_width,
        "12:00",
        TopBarLayoutOptions {
            notification_count,
            show_tray: true,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
        tray_count,
    )
}

/// Computes `TopBar` tray pill rectangles using exact telemetry and visibility flags (`RUSTYOS-048`/`049`).
#[must_use]
pub fn tray_slot_rects_with_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    tray_count: usize,
) -> Vec<(i32, i32, i32, i32)> {
    if tray_count == 0 || !opts.show_tray {
        return Vec::new();
    }
    let left = compute_top_bar_left_layout_with_options("RustyOS", opts.show_workspaces);
    let min_left_x = if opts.show_workspaces {
        left.layout_x + left.layout_w + 10
    } else {
        left.help_x + left.help_w + 12
    };
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    let anchor_x = right.tray_anchor_x;
    let avail = (anchor_x - 8 - min_left_x).max(0);
    let slot_w = 24;
    let gap = 6;
    let stride = slot_w + gap;
    let max_fit = usize::try_from((avail + gap) / stride)
        .unwrap_or(0)
        .min(MAX_TOP_BAR_TRAY_ITEMS);
    let count = tray_count.min(max_fit);
    if count == 0 {
        return Vec::new();
    }
    let count_i32 = i32::try_from(count).unwrap_or(0);
    let first_x = anchor_x - 8 - (count_i32 * stride - gap);
    (0..count)
        .map(|idx| {
            let idx_i32 = i32::try_from(idx).unwrap_or(0);
            (
                first_x + idx_i32 * stride,
                TOP_BAR_ITEM_Y,
                slot_w,
                TOP_BAR_ITEM_H,
            )
        })
        .collect()
}

/// Backwards-compatible wrapper for telemetry-driven tray rectangles (`RUSTYOS-048`).
#[must_use]
pub fn tray_slot_rects_with_layout(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    tray_count: usize,
) -> Vec<(i32, i32, i32, i32)> {
    tray_slot_rects_with_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            show_tray: true,
            ..TopBarLayoutOptions::default()
        },
        tray_count,
    )
}

/// Maximum number of floating notification toasts shown simultaneously (`RUSTYOS-024`).
pub const MAX_VISIBLE_NOTIFICATIONS: usize = 4;

/// Computes top-right stacked rectangles for up to [`MAX_VISIBLE_NOTIFICATIONS`] active toasts (`RUSTYOS-024`).
#[must_use]
pub fn notification_toast_rects(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    let visible = count.min(MAX_VISIBLE_NOTIFICATIONS);
    if visible == 0 {
        return Vec::new();
    }
    let width = screen_size.w.max(320);
    let toast_w = 340.min((width - 32).max(200));
    let toast_h = 68;
    let gap = 10;
    let toast_x = width - toast_w - 16;
    let start_y = TOP_BAR_HEIGHT + 12;

    let mut rects = Vec::with_capacity(visible);
    for idx in 0..visible {
        let idx_i32 = i32::try_from(idx).unwrap_or(0);
        let y = start_y + idx_i32 * (toast_h + gap);
        rects.push(Rectangle::new(
            Point::from((toast_x, y)),
            Size::from((toast_w, toast_h)),
        ));
    }
    rects
}

/// Hit-tests `(x, y)` against visible top-right Notification HUD toasts (`RUSTYOS-024`).
#[must_use]
pub fn check_notification_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    visible_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let rects = notification_toast_rects(screen_size, visible_count);
    for (idx, rect) in rects.into_iter().enumerate() {
        let rx = f64::from(rect.loc.x);
        let ry = f64::from(rect.loc.y);
        let rw = f64::from(rect.size.w);
        let rh = f64::from(rect.size.h);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return Some(idx);
        }
    }
    None
}

/// Checks if `(x, y)` clicked the `TopBar` unread notification badge pill (`RUSTYOS-024`).
#[must_use]
pub fn check_top_bar_notification_click(
    screen_width: i32,
    notification_count: u32,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_notification_click_for_telemetry(
        screen_width,
        false,
        notification_count,
        "12:00",
        x,
        y,
    )
}

/// Telemetry-aware hit-test for the `TopBar` unread notification badge (`RUSTYOS-048`/`049`).
#[must_use]
pub fn check_top_bar_notification_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if opts.notification_count == 0 || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let layout = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    x >= f64::from(layout.notif_x) && x < f64::from(layout.notif_x + layout.notif_w)
}

/// Telemetry-aware hit-test for the `TopBar` unread notification badge (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_notification_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_notification_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

/// Checks if `(x, y)` clicked the `TopBar` keyboard layout pill (`"EN"` / `"RU"`) when visible (`RUSTYOS-029`).
#[must_use]
pub fn check_top_bar_kbd_click(screen_width: i32, notification_count: u32, x: f64, y: f64) -> bool {
    check_top_bar_kbd_click_for_options(
        screen_width,
        "12:00",
        TopBarLayoutOptions {
            notification_count,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

/// Option-aware hit-test for the `TopBar` keyboard layout indicator (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_kbd_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if !opts.show_input_source || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let layout = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    let Some(kbd_x) = layout.kbd_x else {
        return false;
    };
    x >= f64::from(kbd_x) && x < f64::from(kbd_x + layout.kbd_w)
}

/// Telemetry-aware hit-test for the `TopBar` keyboard layout indicator (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_kbd_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_kbd_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

/// Checks if `(x, y)` clicked one of the `TopBar` system tray pills and returns its visible index (`RUSTYOS-032`).
#[must_use]
pub fn check_top_bar_tray_click(
    screen_width: i32,
    notification_count: u32,
    tray_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    check_top_bar_tray_click_for_options(
        screen_width,
        "12:00",
        TopBarLayoutOptions {
            notification_count,
            show_tray: true,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
        tray_count,
        x,
        y,
    )
}

/// Option-aware hit-test for `TopBar` system tray icons (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_tray_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    tray_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    if !opts.show_tray || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    for (idx, (bx, _by, bw, _bh)) in
        tray_slot_rects_with_options(screen_width, clock_text, opts, tray_count)
            .into_iter()
            .enumerate()
    {
        if x >= f64::from(bx) && x < f64::from(bx + bw) {
            return Some(idx);
        }
    }
    None
}

/// Telemetry-aware hit-test for `TopBar` system tray icons (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_tray_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    tray_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    check_top_bar_tray_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            show_tray: true,
            ..TopBarLayoutOptions::default()
        },
        tray_count,
        x,
        y,
    )
}

pub(super) const MENU_SYSTEM: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "About RustyOS",
        icon: crate::surfaces::UiGlyphKind::Info,
        shortcut: None,
        action: TopBarMenuAction::AboutRustyOs,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "System Settings...",
        icon: crate::surfaces::UiGlyphKind::ControlCenter,
        shortcut: Some("Super+,"),
        action: TopBarMenuAction::OpenSettings,
    },
    TopBarMenuEntry::Item {
        label: "Reload Config",
        icon: crate::surfaces::UiGlyphKind::Refresh,
        shortcut: Some("Super+Shift+R"),
        action: TopBarMenuAction::ReloadConfig,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: crate::surfaces::UiGlyphKind::Grid,
        shortcut: Some("Super+Tab"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: crate::surfaces::UiGlyphKind::ControlCenter,
        shortcut: Some("Alt+S"),
        action: TopBarMenuAction::OpenControlCenter,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Lock Screen",
        icon: crate::surfaces::UiGlyphKind::Lock,
        shortcut: Some("Super+L"),
        action: TopBarMenuAction::LockSession,
    },
];

pub(super) const MENU_ACTIVE_APP: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "New Terminal",
        icon: crate::surfaces::UiGlyphKind::TerminalPrompt,
        shortcut: None,
        action: TopBarMenuAction::NewTerminal,
    },
    TopBarMenuEntry::Item {
        label: "Open Files",
        icon: crate::surfaces::UiGlyphKind::Folder,
        shortcut: None,
        action: TopBarMenuAction::OpenFiles,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: crate::surfaces::UiGlyphKind::ControlCenter,
        shortcut: Some("Alt+S"),
        action: TopBarMenuAction::OpenControlCenter,
    },
];

pub(super) const MENU_FILE: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "New Terminal",
        icon: crate::surfaces::UiGlyphKind::TerminalPrompt,
        shortcut: None,
        action: TopBarMenuAction::NewTerminal,
    },
    TopBarMenuEntry::Item {
        label: "Open Files",
        icon: crate::surfaces::UiGlyphKind::Folder,
        shortcut: None,
        action: TopBarMenuAction::OpenFiles,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Spotlight Search",
        icon: crate::surfaces::UiGlyphKind::Search,
        shortcut: Some("Super+Space"),
        action: TopBarMenuAction::OpenSpotlight,
    },
];

pub(super) const MENU_EDIT: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Toggle Float / Tile",
        icon: crate::surfaces::UiGlyphKind::LayoutSplit,
        shortcut: Some("Alt+L"),
        action: TopBarMenuAction::ToggleLayout,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Toggle Reduced Motion",
        icon: crate::surfaces::UiGlyphKind::Motion,
        shortcut: Some("Alt+R"),
        action: TopBarMenuAction::ToggleReducedMotion,
    },
    TopBarMenuEntry::Item {
        label: "Toggle Opaque Panels",
        icon: crate::surfaces::UiGlyphKind::Transparency,
        shortcut: None,
        action: TopBarMenuAction::ToggleReduceTransparency,
    },
];

pub(super) const MENU_VIEW: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: crate::surfaces::UiGlyphKind::Grid,
        shortcut: Some("Super+Tab"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: crate::surfaces::UiGlyphKind::ControlCenter,
        shortcut: Some("Alt+S"),
        action: TopBarMenuAction::OpenControlCenter,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Refresh Metrics",
        icon: crate::surfaces::UiGlyphKind::Refresh,
        shortcut: None,
        action: TopBarMenuAction::RefreshMetrics,
    },
];

pub(super) const MENU_GO: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: crate::surfaces::UiGlyphKind::Grid,
        shortcut: Some("Super+Tab"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Next Workspace",
        icon: crate::surfaces::UiGlyphKind::ArrowNext,
        shortcut: None,
        action: TopBarMenuAction::NextWorkspace,
    },
    TopBarMenuEntry::Item {
        label: "Previous Workspace",
        icon: crate::surfaces::UiGlyphKind::ArrowPrev,
        shortcut: None,
        action: TopBarMenuAction::PrevWorkspace,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Spotlight Search",
        icon: crate::surfaces::UiGlyphKind::Search,
        shortcut: Some("Super+Space"),
        action: TopBarMenuAction::OpenSpotlight,
    },
];

pub(super) const MENU_WINDOW: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Focus Next Window",
        icon: crate::surfaces::UiGlyphKind::ArrowNext,
        shortcut: Some("Alt+J"),
        action: TopBarMenuAction::FocusNextWindow,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Maximize / Restore",
        icon: crate::surfaces::UiGlyphKind::WindowMax,
        shortcut: Some("Alt+M"),
        action: TopBarMenuAction::ToggleMaximizeFocused,
    },
    TopBarMenuEntry::Item {
        label: "Minimize Window",
        icon: crate::surfaces::UiGlyphKind::WindowMin,
        shortcut: Some("Alt+H"),
        action: TopBarMenuAction::MinimizeFocused,
    },
    TopBarMenuEntry::Item {
        label: "Toggle Float / Tile",
        icon: crate::surfaces::UiGlyphKind::LayoutSplit,
        shortcut: Some("Alt+L"),
        action: TopBarMenuAction::ToggleLayout,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Close Window",
        icon: crate::surfaces::UiGlyphKind::Close,
        shortcut: Some("Alt+W"),
        action: TopBarMenuAction::CloseFocused,
    },
];

pub(super) const MENU_HELP: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "About RustyOS",
        icon: crate::surfaces::UiGlyphKind::Info,
        shortcut: None,
        action: TopBarMenuAction::AboutRustyOs,
    },
    TopBarMenuEntry::Item {
        label: "Keyboard Shortcuts",
        icon: crate::surfaces::UiGlyphKind::Shortcuts,
        shortcut: None,
        action: TopBarMenuAction::ShowShortcuts,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Refresh Metrics",
        icon: crate::surfaces::UiGlyphKind::Refresh,
        shortcut: None,
        action: TopBarMenuAction::RefreshMetrics,
    },
];

/// Returns menu dropdown entries with leading SDF icons, group separators, and real keybindings (`RUSTYOS-051`).
#[must_use]
pub const fn top_bar_menu_entries(menu: TopBarMenuHit) -> &'static [TopBarMenuEntry] {
    match menu {
        TopBarMenuHit::SystemLogo => MENU_SYSTEM,
        TopBarMenuHit::ActiveApp => MENU_ACTIVE_APP,
        TopBarMenuHit::File => MENU_FILE,
        TopBarMenuHit::Edit => MENU_EDIT,
        TopBarMenuHit::View => MENU_VIEW,
        TopBarMenuHit::Go => MENU_GO,
        TopBarMenuHit::Window => MENU_WINDOW,
        TopBarMenuHit::Help => MENU_HELP,
    }
}

/// Returns compact `(label, action)` menu tuples for an open `TopBar` dropdown (`RUSTYOS-048`).
#[must_use]
pub const fn top_bar_menu_items(
    menu: TopBarMenuHit,
) -> &'static [(&'static str, TopBarMenuAction)] {
    match menu {
        TopBarMenuHit::SystemLogo => &[
            ("About RustyOS", TopBarMenuAction::AboutRustyOs),
            ("System Settings...", TopBarMenuAction::OpenSettings),
            ("Reload Config", TopBarMenuAction::ReloadConfig),
            ("Workspace Overview", TopBarMenuAction::OpenOverview),
            ("Control Center", TopBarMenuAction::OpenControlCenter),
            ("Lock Screen", TopBarMenuAction::LockSession),
        ],
        TopBarMenuHit::ActiveApp => &[
            ("New Terminal", TopBarMenuAction::NewTerminal),
            ("Open Files", TopBarMenuAction::OpenFiles),
            ("Control Center", TopBarMenuAction::OpenControlCenter),
        ],
        TopBarMenuHit::File => &[
            ("New Terminal", TopBarMenuAction::NewTerminal),
            ("Open Files", TopBarMenuAction::OpenFiles),
            ("Spotlight Search", TopBarMenuAction::OpenSpotlight),
        ],
        TopBarMenuHit::Edit => &[
            ("Toggle Float / Tile", TopBarMenuAction::ToggleLayout),
            (
                "Toggle Reduced Motion",
                TopBarMenuAction::ToggleReducedMotion,
            ),
            (
                "Toggle Opaque Panels",
                TopBarMenuAction::ToggleReduceTransparency,
            ),
        ],
        TopBarMenuHit::View => &[
            ("Workspace Overview", TopBarMenuAction::OpenOverview),
            ("Control Center", TopBarMenuAction::OpenControlCenter),
            ("Refresh Metrics", TopBarMenuAction::RefreshMetrics),
        ],
        TopBarMenuHit::Go => &[
            ("Workspace Overview", TopBarMenuAction::OpenOverview),
            ("Next Workspace", TopBarMenuAction::NextWorkspace),
            ("Previous Workspace", TopBarMenuAction::PrevWorkspace),
            ("Spotlight Search", TopBarMenuAction::OpenSpotlight),
        ],
        TopBarMenuHit::Window => &[
            ("Focus Next Window", TopBarMenuAction::FocusNextWindow),
            (
                "Maximize / Restore",
                TopBarMenuAction::ToggleMaximizeFocused,
            ),
            ("Minimize Window", TopBarMenuAction::MinimizeFocused),
            ("Toggle Float / Tile", TopBarMenuAction::ToggleLayout),
            ("Close Window", TopBarMenuAction::CloseFocused),
        ],
        TopBarMenuHit::Help => &[
            ("About RustyOS", TopBarMenuAction::AboutRustyOs),
            ("Keyboard Shortcuts", TopBarMenuAction::ShowShortcuts),
            ("Refresh Metrics", TopBarMenuAction::RefreshMetrics),
        ],
    }
}

pub(super) const MENU_POPOVER_PAD_Y: i32 = 6;
pub(super) const MENU_ITEM_HEIGHT: i32 = 28;
pub(super) const MENU_SEPARATOR_HEIGHT: i32 = 9;

/// Computes the glass dropdown popover rectangle anchored beneath `menu` (`RUSTYOS-048`/`051`).
#[must_use]
pub fn top_bar_menu_popover_rect(
    active_app_name: &str,
    menu: TopBarMenuHit,
) -> (i32, i32, i32, i32) {
    let left = compute_top_bar_left_layout(active_app_name);
    let anchor_x = match menu {
        TopBarMenuHit::SystemLogo => TOP_BAR_LOGO_X,
        TopBarMenuHit::ActiveApp => left.app_x,
        TopBarMenuHit::File => left.file_x,
        TopBarMenuHit::Edit => left.edit_x,
        TopBarMenuHit::View => left.view_x,
        TopBarMenuHit::Go => left.go_x,
        TopBarMenuHit::Window => left.win_x,
        TopBarMenuHit::Help => left.help_x,
    };
    let mut content_h = 0;
    for entry in top_bar_menu_entries(menu) {
        content_h += match entry {
            TopBarMenuEntry::Item { .. } => MENU_ITEM_HEIGHT,
            TopBarMenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
        };
    }
    let pop_w = 256;
    let pop_h = MENU_POPOVER_PAD_Y * 2 + content_h;
    (anchor_x, TOP_BAR_HEIGHT + 6, pop_w, pop_h)
}

pub(super) fn top_bar_menu_entry_at(
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> Option<(usize, TopBarMenuAction)> {
    let (px, py, pw, ph) = top_bar_menu_popover_rect(active_app_name, menu);
    let fx = f64::from(px);
    let fy = f64::from(py);
    if x < fx || x >= fx + f64::from(pw) || y < fy || y >= fy + f64::from(ph) {
        return None;
    }
    let mut cursor_y = py + MENU_POPOVER_PAD_Y;
    for (entry_idx, entry) in top_bar_menu_entries(menu).iter().enumerate() {
        match entry {
            TopBarMenuEntry::Item { action, .. } => {
                let top = f64::from(cursor_y);
                let bot = f64::from(cursor_y + MENU_ITEM_HEIGHT);
                if y >= top && y < bot {
                    return Some((entry_idx, *action));
                }
                cursor_y += MENU_ITEM_HEIGHT;
            }
            TopBarMenuEntry::Separator => {
                cursor_y += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    None
}

/// Hit-tests `(x, y)` against an open `TopBar` menu dropdown popover (`RUSTYOS-048`/`051`).
#[must_use]
pub fn check_top_bar_menu_popover_click(
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> TopBarMenuPopoverClick {
    let (px, py, pw, ph) = top_bar_menu_popover_rect(active_app_name, menu);
    let fx = f64::from(px);
    let fy = f64::from(py);
    if x < fx || x >= fx + f64::from(pw) || y < fy || y >= fy + f64::from(ph) {
        return TopBarMenuPopoverClick::Outside;
    }
    if let Some((_, action)) = top_bar_menu_entry_at(active_app_name, menu, x, y) {
        TopBarMenuPopoverClick::Action(action)
    } else {
        TopBarMenuPopoverClick::Inside
    }
}

/// Returns the hovered entry index (`0..top_bar_menu_entries(menu).len()`) inside an open menu popover (`RUSTYOS-051`).
#[must_use]
pub fn check_top_bar_menu_popover_hover_row(
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> Option<usize> {
    top_bar_menu_entry_at(active_app_name, menu, x, y).map(|(idx, _)| idx)
}

/// Hit-tests `(x, y)` against the `TopBar` Left Menu (`Logo`, `App`, `File`, `Edit`, `View`, `Go`, `Window`, `Help`) (`RUSTYOS-038`/`049`).
#[must_use]
pub fn check_top_bar_menu_click(x: f64, y: f64) -> Option<TopBarMenuHit> {
    check_top_bar_menu_click_for_app("RustyOS", x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` Left Menu (`RUSTYOS-048`/`049`).
#[must_use]
pub fn check_top_bar_menu_click_for_app(
    active_app_name: &str,
    x: f64,
    y: f64,
) -> Option<TopBarMenuHit> {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let left = compute_top_bar_left_layout(active_app_name);
    if x >= f64::from(TOP_BAR_LOGO_X) && x < f64::from(TOP_BAR_LOGO_X + TOP_BAR_LOGO_W) {
        return Some(TopBarMenuHit::SystemLogo);
    }
    for (hit, bx, bw) in [
        (TopBarMenuHit::ActiveApp, left.app_x, left.app_w),
        (TopBarMenuHit::File, left.file_x, left.file_w),
        (TopBarMenuHit::Edit, left.edit_x, left.edit_w),
        (TopBarMenuHit::View, left.view_x, left.view_w),
        (TopBarMenuHit::Go, left.go_x, left.go_w),
        (TopBarMenuHit::Window, left.win_x, left.win_w),
        (TopBarMenuHit::Help, left.help_x, left.help_w),
    ] {
        if x >= f64::from(bx) && x < f64::from(bx + bw) {
            return Some(hit);
        }
    }
    None
}

/// Checks if `(x, y)` clicked the `TopBar` Layout badge when visible (`Float` / `Tile`).
#[must_use]
pub fn check_top_bar_layout_click(x: f64, y: f64) -> bool {
    check_top_bar_layout_click_with_options("RustyOS", true, x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` Layout badge (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_layout_click_for_app(active_app_name: &str, x: f64, y: f64) -> bool {
    check_top_bar_layout_click_with_options(active_app_name, true, x, y)
}

/// Option-aware hit-test for the optional `TopBar` Layout badge (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_layout_click_with_options(
    active_app_name: &str,
    show_workspaces: bool,
    x: f64,
    y: f64,
) -> bool {
    if !show_workspaces || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let left = compute_top_bar_left_layout_with_options(active_app_name, show_workspaces);
    x >= f64::from(left.layout_x) && x < f64::from(left.layout_x + left.layout_w)
}

/// Option-aware hit-test for the right `TopBar` Spotlight Search loupe (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_search_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    x >= f64::from(right.search_x - 3) && x < f64::from(right.search_x + right.search_w + 3)
}

/// Checks if `(x, y)` clicked the right `TopBar` Spotlight Search loupe (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_search_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_search_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

/// Checks if `(x, y)` clicked the right `TopBar` Control Center / status cluster.
#[must_use]
pub fn check_top_bar_status_click(screen_width: i32, x: f64, y: f64) -> bool {
    check_top_bar_status_click_for_telemetry(screen_width, false, 0, "12:00", x, y)
}

/// Option-aware hit-test for the `TopBar` status / Control Center cluster (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_status_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    x >= f64::from(right.status_hit_x) && x < f64::from(right.status_hit_x + right.status_hit_w)
}

/// Telemetry-aware hit-test for the `TopBar` status / Control Center cluster (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_status_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_status_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

pub(super) fn render_top_bar(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    active_workspace: u8,
    layout_mode: &str,
    telemetry: &TopBarTelemetry<'_>,
    has_text: bool,
) {
    let left_layout =
        compute_top_bar_left_layout_with_options("RustyOS", telemetry.show_workspaces);

    // 1. TopBar Background (opaque only when reduce_transparency is requested)
    if telemetry.reduce_transparency {
        let top_bar_geo = Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT)));
        elements.push(make_solid_element(top_bar_geo, COLOR_SURFACE_ELEVATED));
        let top_bar_border =
            Rectangle::new(Point::from((0, TOP_BAR_HEIGHT - 1)), Size::from((width, 1)));
        elements.push(make_solid_element(top_bar_border, COLOR_BORDER));
    }

    // 2. Active App Menu placeholder (when no font renderer is active)
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((TOP_BAR_LOGO_X, TOP_BAR_ITEM_Y)),
            Size::from((TOP_BAR_LOGO_W, TOP_BAR_ITEM_H)),
        ),
        COLOR_ACCENT_MUTED,
    ));
    if !has_text {
        elements.push(make_solid_element(
            Rectangle::new(
                Point::from((left_layout.app_x, TOP_BAR_ITEM_Y)),
                Size::from((left_layout.app_w, TOP_BAR_ITEM_H)),
            ),
            COLOR_SURFACE_OVERLAY,
        ));
    }

    // 3. Optional Workspace indicators and layout badge (`menubar_show_workspaces = true`)
    if telemetry.show_workspaces {
        render_top_bar_workspaces(
            elements,
            &left_layout,
            active_workspace,
            layout_mode,
            has_text,
        );
    }

    let ws_y = TOP_BAR_ITEM_Y;

    // 4. Status area on the right side of TopBar (battery, network, audio, clock)
    let (status_x, status_y, status_pill_width, status_h) = status_pill_rect(width);
    let status_pill_geo = Rectangle::new(
        Point::from((status_x, status_y)),
        Size::from((status_pill_width, status_h)),
    );
    let pill_bg = if telemetry.quick_settings_open {
        COLOR_ACCENT_MUTED
    } else {
        COLOR_SURFACE_OVERLAY
    };
    elements.push(make_solid_element(status_pill_geo, pill_bg));

    // Top subtle highlight on status pill
    let status_highlight = Rectangle::new(
        Point::from((status_x, status_y)),
        Size::from((status_pill_width, 1)),
    );
    let highlight_col = if telemetry.quick_settings_open {
        COLOR_ACCENT
    } else {
        COLOR_BORDER
    };
    elements.push(make_solid_element(status_highlight, highlight_col));

    // Status dot (green if online, amber if offline)
    let dot_geo = Rectangle::new(Point::from((status_x + 8, ws_y + 6)), Size::from((8, 8)));
    let dot_color = if telemetry.net_connected {
        COLOR_SUCCESS
    } else {
        COLOR_WARNING
    };
    elements.push(make_solid_element(dot_geo, dot_color));

    if !has_text {
        // Accent status bars fallback when no font is available
        let bar1_geo = Rectangle::new(Point::from((status_x + 24, ws_y + 8)), Size::from((40, 4)));
        elements.push(make_solid_element(bar1_geo, COLOR_MUTED));
        let bar2_geo = Rectangle::new(Point::from((status_x + 72, ws_y + 6)), Size::from((46, 8)));
        elements.push(make_solid_element(bar2_geo, COLOR_ACCENT_MUTED));
    }
}

pub(super) fn render_top_bar_workspaces(
    elements: &mut Vec<SolidColorRenderElement>,
    left_layout: &TopBarLeftLayout,
    active_workspace: u8,
    layout_mode: &str,
    has_text: bool,
) {
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((left_layout.separator_x, 6)),
            Size::from((1, 12)),
        ),
        COLOR_BORDER,
    ));

    let ws_btn_height = TOP_BAR_ITEM_H;
    let ws_y = TOP_BAR_ITEM_Y;
    let mut start_x = left_layout.ws_start_x;

    for ws in 1..=9 {
        let is_active = ws == active_workspace;
        let ws_geo = Rectangle::new(
            Point::from((start_x, ws_y)),
            Size::from((WS_BTN_WIDTH, ws_btn_height)),
        );

        if is_active {
            elements.push(make_solid_element(ws_geo, COLOR_ACCENT));
        } else {
            elements.push(make_solid_element(ws_geo, COLOR_SURFACE_OVERLAY));
            if !has_text {
                let pip_geo = Rectangle::new(
                    Point::from((
                        start_x + (WS_BTN_WIDTH - 6) / 2,
                        ws_y + (ws_btn_height - 2) / 2,
                    )),
                    Size::from((6, 2)),
                );
                elements.push(make_solid_element(pip_geo, COLOR_SUBTLE));
            }
        }
        start_x += WS_BTN_STRIDE;
    }

    let layout_bg = Rectangle::new(
        Point::from((left_layout.layout_x, ws_y)),
        Size::from((left_layout.layout_w, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_bg, COLOR_SURFACE_OVERLAY));

    let layout_accent = if layout_mode == "master-stack" {
        COLOR_ACCENT
    } else {
        COLOR_SUBTLE
    };
    let layout_strip = Rectangle::new(
        Point::from((left_layout.layout_x, ws_y)),
        Size::from((3, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_strip, layout_accent));
}

/// Checks if a mouse click at `(x, y)` was inside the `TopBar` workspaces area when visible and returns the target workspace (1..=9).
#[must_use]
pub fn check_top_bar_workspace_click(x: f64, y: f64) -> Option<u8> {
    check_top_bar_workspace_click_with_options("RustyOS", true, x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` workspaces area (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_workspace_click_for_app(active_app_name: &str, x: f64, y: f64) -> Option<u8> {
    check_top_bar_workspace_click_with_options(active_app_name, true, x, y)
}

/// Option-aware hit-test for the optional `TopBar` workspaces area (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_workspace_click_with_options(
    active_app_name: &str,
    show_workspaces: bool,
    x: f64,
    y: f64,
) -> Option<u8> {
    if !show_workspaces || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let left = compute_top_bar_left_layout_with_options(active_app_name, show_workspaces);
    let start_x = f64::from(left.ws_start_x);
    let stride = f64::from(WS_BTN_STRIDE);
    let total_width = stride * 9.0;

    if x < start_x || x >= start_x + total_width {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let idx = ((x - start_x) / stride).floor() as u8 + 1;
    if (1..=9).contains(&idx) {
        Some(idx)
    } else {
        None
    }
}

pub(super) fn notification_app_icon(title: &str) -> crate::icons::BuiltinAppIcon {
    let lower = title.to_ascii_lowercase();
    if lower.contains("term")
        || lower.contains("shell")
        || lower.contains("bash")
        || lower.contains("konsole")
    {
        crate::icons::BuiltinAppIcon::Terminal
    } else if lower.contains("file") || lower.contains("download") || lower.contains("dolphin") {
        crate::icons::BuiltinAppIcon::Files
    } else if lower.contains("browser")
        || lower.contains("web")
        || lower.contains("firefox")
        || lower.contains("chrom")
    {
        crate::icons::BuiltinAppIcon::Browser
    } else if lower.contains("setting") || lower.contains("config") || lower.contains("pref") {
        crate::icons::BuiltinAppIcon::Settings
    } else {
        crate::icons::BuiltinAppIcon::RustySystem
    }
}

pub(super) fn osd_glyph_kind(
    label: &str,
    value_percent: Option<u8>,
    muted: bool,
) -> crate::surfaces::UiGlyphKind {
    let lower = label.to_ascii_lowercase();
    if lower.contains("bright") || lower.contains("glass") || lower.contains("display") {
        crate::surfaces::UiGlyphKind::Sun
    } else if lower.contains("vol")
        || lower.contains("audio")
        || lower.contains("sound")
        || lower.contains("mute")
        || value_percent.is_some()
    {
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: value_percent.unwrap_or(50),
            muted,
        }
    } else {
        crate::surfaces::UiGlyphKind::Info
    }
}

/// Computes the centered floating OSD pill bounds right above the bottom Dock (`RUSTYOS-031`).
#[must_use]
pub fn osd_pill_rect(
    screen_size: Size<i32, smithay::utils::Physical>,
    has_bar: bool,
) -> Rectangle<i32, smithay::utils::Physical> {
    let w = 256.min((screen_size.w - 40).max(180));
    let h = if has_bar { 56 } else { 40 };
    let x = (screen_size.w - w) / 2;
    let y = (screen_size.h - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - 22 - h).max(TOP_BAR_HEIGHT + 12);
    Rectangle::new(Point::from((x, y)), Size::from((w, h)))
}

/// Generates Liquid Glass OSD indicator elements (`Volume`, `Brightness`, `Media`, `Screenshot`) (`RUSTYOS-031`/`051`).
pub fn generate_osd_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    osd: &OsdRenderState<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let pill = osd_pill_rect(screen_size, osd.value_percent.is_some());
    let px = pill.loc.x;
    let py = pill.loc.y;
    let pw = pill.size.w;

    let text_bright = [248, 244, 255, 255];
    let text_muted = [182, 172, 204, 255];

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        osd_glyph_kind(osd.label, osd.value_percent, osd.muted),
        22,
        [250, 246, 255, 255],
        Point::from((px + 14, py + (pill.size.h - 22) / 2)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(pct) = osd.value_percent {
        if let Some(el) = text_cache.render_label(
            renderer,
            osd.label,
            12.0,
            text_bright,
            Point::from((px + 44, py + 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let val_text = if osd.muted {
            "Muted".to_owned()
        } else {
            format!("{pct}%")
        };
        let (vw, _) = text_cache.measure_text(&val_text, 12.0);
        if let Some(el) = text_cache.render_label(
            renderer,
            &val_text,
            12.0,
            if osd.muted { text_muted } else { text_bright },
            Point::from((px + pw - 14 - vw, py + 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }

        let bar_x = px + 44;
        let bar_y = py + 34;
        let bar_w = (pw - 58).max(24);
        let bar_h = 8;
        let fill_w = (bar_w * i32::from(pct.min(100))) / 100;
        if fill_w > 0 {
            let fill_rgba = if osd.muted {
                [152, 140, 174, 180]
            } else {
                [246, 240, 255, 236]
            };
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((bar_x, bar_y)), Size::from((fill_w, bar_h))),
                fill_w.min(4),
                fill_rgba,
                None,
            );
        }
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(Point::from((bar_x, bar_y)), Size::from((bar_w, bar_h))),
            4,
            [255, 255, 255, 30],
            Some([255, 255, 255, 48]),
        );
    } else if let Some(el) = text_cache.render_label_centered(
        renderer,
        osd.label,
        12.5,
        text_bright,
        Rectangle::new(
            Point::from((px + 38, py)),
            Size::from((pw - 48, pill.size.h)),
        ),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    push_liquid_glass_panel(
        &mut out,
        shape_cache,
        renderer,
        pill,
        18,
        crate::surfaces::GlassPanelStyle::Popover,
        crate::config::DEFAULT_GLASS_OPACITY,
        false,
        Some((14, 46)),
    );
    out
}

/// Draws monochrome macOS Menu Bar glyphs (`[Volume] · [Battery] · Wi-Fi · Search · Control Center`) (`RUSTYOS-036`, `RUSTYOS-049`).
pub(super) fn push_top_bar_right_glyphs(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    right: &TopBarRightLayout,
    t: &TopBarTelemetry<'_>,
) {
    use crate::surfaces::UiGlyphKind;

    let white = [252, 250, 255, 255];

    if let Some(vx) = right.volume_x
        && let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Volume {
                level_percent: t.volume_percent,
                muted: t.audio_muted,
            },
            18,
            white,
            Point::from((vx, TOP_BAR_ITEM_Y + 1)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(bx) = right.battery_x
        && let Some(el) = shape_cache.ui_glyph_rect_element(
            renderer,
            UiGlyphKind::Battery {
                percent: t.battery_percent,
                charging: t.ac_online || t.battery_charging,
            },
            26,
            20,
            white,
            Point::from((bx, TOP_BAR_ITEM_Y)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Wifi {
            connected: t.net_connected,
            signal_percent: t.wifi_signal_percent,
        },
        20,
        white,
        Point::from((right.wifi_x, TOP_BAR_ITEM_Y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Search,
        18,
        white,
        Point::from((right.search_x, TOP_BAR_ITEM_Y + 1)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let cc_color = if t.quick_settings_open {
        [255, 255, 255, 255]
    } else {
        white
    };
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::ControlCenter,
        20,
        cc_color,
        Point::from((right.control_center_x, TOP_BAR_ITEM_Y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
}
