//! `TopBar` base panel layout, workspace strip indicators, telemetry state, and hit-testing (`RUSTYOS-090`).

use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::utils::{Point, Rectangle, Size};

use crate::shell_render::tokens::{
    COLOR_ACCENT, COLOR_ACCENT_MUTED, COLOR_BORDER, COLOR_MUTED, COLOR_SUBTLE, COLOR_SUCCESS,
    COLOR_SURFACE_ELEVATED, COLOR_SURFACE_OVERLAY, COLOR_WARNING, TOP_BAR_APP_X, TOP_BAR_FONT_SIZE,
    TOP_BAR_HEIGHT, TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y, TOP_BAR_LOGO_W, TOP_BAR_LOGO_X, WS_BTN_STRIDE,
    WS_BTN_WIDTH, make_solid_element,
};
use crate::shell_render::top_bar_menus::TopBarAppProfile;

use super::menus::TopBarMenuHit;
use super::status::{TopBarTrayItem, status_pill_rect};

/// Visibility and telemetry flags controlling optional items in the `TopBar` (`RUSTYOS-049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct TopBarLayoutOptions {
    /// Whether a hardware battery is present.
    pub has_battery: bool,
    /// Active unread desktop notification count.
    pub notification_count: u32,
    /// Show Wi-Fi / Ethernet glyph (`RUSTYOS-141`, default `true`).
    pub show_wifi: bool,
    /// Show Battery glyph when a battery is present (`RUSTYOS-141`, default `true`).
    pub show_battery: bool,
    /// Show numeric battery percentage next to the Battery glyph (`RUSTYOS-141`).
    pub show_battery_percentage: bool,
    /// Show standalone Bluetooth glyph (`RUSTYOS-141`).
    pub show_bluetooth: bool,
    /// Show standalone volume glyph left of Battery/Wi-Fi.
    pub show_volume: bool,
    /// Show keyboard input source indicator (`EN`/`RU`).
    pub show_input_source: bool,
    /// Show workspace numbers `1..=9` and layout badge.
    pub show_workspaces: bool,
    /// Show `StatusNotifierItem` tray applets.
    pub show_tray: bool,
    /// Show Software Updates indicator icon (`RUSTYOS-076`).
    pub show_updates: bool,
    /// True when microphone capture is active (`RUSTYOS-142`).
    pub mic_active: bool,
    /// True when camera capture is active (`RUSTYOS-142`).
    pub camera_active: bool,
    /// True when screen recording / screencast is active (`RUSTYOS-142`).
    pub recording_active: bool,
}

impl Default for TopBarLayoutOptions {
    fn default() -> Self {
        Self {
            has_battery: false,
            notification_count: 0,
            show_wifi: true,
            show_battery: true,
            show_battery_percentage: false,
            show_bluetooth: false,
            show_volume: false,
            show_input_source: false,
            show_workspaces: false,
            show_tray: false,
            show_updates: false,
            mic_active: false,
            camera_active: false,
            recording_active: false,
        }
    }
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
    /// Whether a wired Ethernet interface is connected (`RUSTYOS-108`).
    pub wired_connected: bool,
    /// Whether a wireless Wi-Fi interface is actively connected (`RUSTYOS-108`).
    pub wifi_connected: bool,
    /// Whether the Wi-Fi radio is enabled (`RUSTYOS-108`).
    pub wifi_enabled: bool,
    /// Live Wi-Fi signal strength percentage (`0..=100`, `RUSTYOS-052`).
    pub wifi_signal_percent: Option<u8>,
    /// Master output volume (`0..=100`).
    pub volume_percent: u8,
    /// Master audio output is muted.
    pub audio_muted: bool,
    /// Whether the Quick Settings popover is currently open.
    pub quick_settings_open: bool,
    /// Whether the Notification Center drawer is currently open (`RUSTYOS-109`).
    pub notification_center_open: bool,
    /// Whether the Wi-Fi popover is currently open (`RUSTYOS-108`).
    pub wifi_open: bool,
    /// Whether the Software Updates popover is currently open (`RUSTYOS-076`).
    pub updates_open: bool,
    /// Show Software Updates indicator in the Menu Bar right cluster (`RUSTYOS-076`).
    pub show_updates: bool,
    /// Number of pending Arch/RustyOS updates (`RUSTYOS-076`).
    pub update_count: u32,
    /// True while background `checkupdates` refresh is running (`RUSTYOS-076`).
    pub update_checking: bool,
    /// True when a `rustyos*` package update is pending (`RUSTYOS-076`).
    pub rustyos_update_available: bool,
    /// Number of active/unread desktop notifications (`RUSTYOS-024`).
    pub notification_count: u32,
    /// Active keyboard layout badge code (`"EN"`, `"RU"`, etc.) (`RUSTYOS-029`).
    pub kbd_layout: &'a str,
    /// Registered `StatusNotifierItem` tray badges rendered when `show_tray == true` (`RUSTYOS-032`).
    pub tray_items: &'a [TopBarTrayItem<'a>],
    /// Canonical active application title in `TopBar` (`RUSTYOS-068`).
    pub active_app_name: &'a str,
    /// Functional profile driving dynamic `TopBar` categories and shortcuts (`RUSTYOS-068`).
    pub app_profile: TopBarAppProfile,
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
    /// Show Wi-Fi / Ethernet indicator in Menu Bar (`RUSTYOS-141`, default `true`).
    pub show_wifi: bool,
    /// Show Battery indicator in Menu Bar (`RUSTYOS-141`, default `true`).
    pub show_battery: bool,
    /// Show numeric battery percentage beside the Battery glyph (`RUSTYOS-141`).
    pub show_battery_percentage: bool,
    /// Show standalone Bluetooth indicator in Menu Bar (`RUSTYOS-141`).
    pub show_bluetooth: bool,
    /// Whether Bluetooth adapter is powered on (`RUSTYOS-141`).
    pub bluetooth_powered: bool,
    /// True when at least one application is actively capturing from a microphone (`RUSTYOS-142`).
    pub mic_active: bool,
    /// True when at least one application is actively capturing from a camera (`RUSTYOS-142`).
    pub camera_active: bool,
    /// True when screen recording / screencast is active (`RUSTYOS-142`).
    pub recording_active: bool,
    /// Adaptive `TopBar` contrast style computed from the wallpaper strip (`RUSTYOS-141`).
    pub backdrop: Option<super::BarBackdropStyle>,
    /// Active compact status popover (`Volume`, `Battery`, `Bluetooth`) if open (`RUSTYOS-141`).
    pub quick_popover: Option<crate::shell_render::TopBarQuickPopoverKind>,
    /// Live D-Bus `AppMenuTree` exported by the focused window if any (`RUSTYOS-145`).
    pub app_menu_tree: Option<&'a crate::app_menu::AppMenuTree>,
}

impl TopBarTelemetry<'_> {
    /// Extracts [`TopBarLayoutOptions`] matching the active telemetry and UI toggles.
    #[must_use]
    pub const fn layout_options(&self) -> TopBarLayoutOptions {
        TopBarLayoutOptions {
            has_battery: self.has_battery,
            notification_count: self.notification_count,
            show_wifi: self.show_wifi,
            show_battery: self.show_battery,
            show_battery_percentage: self.show_battery_percentage,
            show_bluetooth: self.show_bluetooth,
            show_volume: self.show_volume,
            show_input_source: self.show_input_source,
            show_workspaces: self.show_workspaces,
            show_tray: self.show_tray,
            show_updates: self.show_updates,
            mic_active: self.mic_active,
            camera_active: self.camera_active,
            recording_active: self.recording_active,
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
            wired_connected: false,
            wifi_connected: true,
            wifi_enabled: true,
            wifi_signal_percent: Some(100),
            volume_percent: 75,
            audio_muted: false,
            quick_settings_open: false,
            notification_center_open: false,
            wifi_open: false,
            updates_open: false,
            show_updates: false,
            update_count: 0,
            update_checking: false,
            rustyos_update_available: false,
            notification_count: 0,
            kbd_layout: "EN",
            tray_items: &[],
            active_app_name: "Desktop",
            app_profile: TopBarAppProfile::Desktop,
            active_menu: None,
            menu_hover_row: None,
            glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
            reduce_transparency: false,
            show_workspaces: false,
            show_tray: false,
            show_input_source: false,
            show_volume: false,
            show_wifi: true,
            show_battery: true,
            show_battery_percentage: false,
            show_bluetooth: false,
            bluetooth_powered: false,
            mic_active: false,
            camera_active: false,
            recording_active: false,
            backdrop: None,
            quick_popover: None,
            app_menu_tree: None,
        }
    }
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
    pub has_go: bool,
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

pub(crate) fn measure_menu_box_width(label: &str) -> i32 {
    (crate::text::measure_ui_text_width(label, TOP_BAR_FONT_SIZE, crate::text::FontWeight::Regular)
        + 14)
        .max(26)
}

fn default_profile_for_app_name(active_app_name: &str) -> TopBarAppProfile {
    if active_app_name.eq_ignore_ascii_case("desktop") {
        TopBarAppProfile::Desktop
    } else {
        TopBarAppProfile::Files
    }
}

/// Computes `TopBar` left-side x coordinates (`macOS` Menu Bar rhythm: `Logo  App  File  Edit  View  Go  Window  Help`) (`RUSTYOS-048`/`049`).
#[must_use]
pub fn compute_top_bar_left_layout(active_app_name: &str) -> TopBarLeftLayout {
    let profile = default_profile_for_app_name(active_app_name);
    compute_top_bar_left_layout_for_profile(profile, active_app_name, false)
}

/// Computes `TopBar` left-side layout with explicit workspace-strip visibility (`RUSTYOS-049`, `RUSTYOS-067`).
#[must_use]
pub fn compute_top_bar_left_layout_with_options(
    active_app_name: &str,
    show_workspaces: bool,
) -> TopBarLeftLayout {
    let profile = default_profile_for_app_name(active_app_name);
    compute_top_bar_left_layout_for_profile(profile, active_app_name, show_workspaces)
}

/// Computes `TopBar` left-side layout adapted to `profile` (e.g., hiding `Go` outside `Files`) (`RUSTYOS-068`).
#[must_use]
pub fn compute_top_bar_left_layout_for_profile(
    profile: TopBarAppProfile,
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
    let has_go = profile.has_go_menu();
    let go_x = view_x + view_w;
    let go_w = if has_go {
        measure_menu_box_width("Go")
    } else {
        0
    };
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
        has_go,
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
    pub wifi_visible: bool,
    pub volume_x: Option<i32>,
    pub battery_x: Option<i32>,
    pub bluetooth_x: Option<i32>,
    pub privacy_x: Option<i32>,
    pub recording_x: Option<i32>,
    pub recording_w: i32,
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
    pub updates_x: Option<i32>,
    pub updates_w: i32,
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
#[allow(clippy::too_many_lines)]
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
    let (wifi_x, mut left_cursor) = if opts.show_wifi {
        let wx = search_x - 10 - wifi_w;
        (wx, wx)
    } else {
        (search_x, search_x)
    };

    let battery_w = if opts.show_battery_percentage { 56 } else { 26 };
    let battery_x = if opts.has_battery && opts.show_battery {
        let bx = left_cursor - 10 - battery_w;
        left_cursor = bx;
        Some(bx)
    } else {
        None
    };

    let bluetooth_w = 18;
    let bluetooth_x = if opts.show_bluetooth {
        let btx = left_cursor - 10 - bluetooth_w;
        left_cursor = btx;
        Some(btx)
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

    let privacy_w = match (opts.mic_active, opts.camera_active) {
        (true, true) => 22,
        (true, false) | (false, true) => 12,
        (false, false) => 0,
    };
    let privacy_x = if privacy_w > 0 {
        let px = left_cursor - 8 - privacy_w;
        left_cursor = px;
        Some(px)
    } else {
        None
    };

    let recording_w = 46;
    let recording_x = if opts.recording_active {
        let rx = left_cursor - 8 - recording_w;
        left_cursor = rx;
        Some(rx)
    } else {
        None
    };

    let status_pill_x = control_center_x - 3;
    let status_pill_w = control_center_w + 6;
    let status_hit_x = control_center_x - 5;
    let status_hit_w = (clock_x - 2 - status_hit_x).max(28);

    // macOS 27 Golden Gate (`27-Golden-Gate-Notification-Center.png`) never draws an `N:k` pill in
    // the Menu Bar; Notification Center opens directly from the Clock. Keep layout slot empty.
    let (notif_x, notif_w) = (left_cursor, 0);

    let updates_w = 22;
    let updates_x = if opts.show_updates {
        let ux = left_cursor - 8 - updates_w;
        left_cursor = ux;
        Some(ux)
    } else {
        None
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
        wifi_visible: opts.show_wifi,
        volume_x,
        battery_x,
        bluetooth_x,
        privacy_x,
        recording_x,
        recording_w,
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
        updates_x,
        updates_w,
        kbd_x,
        kbd_w,
        tray_anchor_x: left_cursor,
    }
}

pub(crate) fn render_top_bar(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    active_workspace: u8,
    layout_mode: &str,
    telemetry: &TopBarTelemetry<'_>,
    has_text: bool,
) {
    let left_layout = compute_top_bar_left_layout_for_profile(
        telemetry.app_profile,
        telemetry.active_app_name,
        telemetry.show_workspaces,
    );

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

pub(crate) fn render_top_bar_workspaces(
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
    let profile = default_profile_for_app_name(active_app_name);
    check_top_bar_layout_click_for_profile(profile, active_app_name, show_workspaces, x, y)
}

/// Profile-aware hit-test for the optional `TopBar` Layout badge (`RUSTYOS-068`).
#[must_use]
pub fn check_top_bar_layout_click_for_profile(
    profile: TopBarAppProfile,
    active_app_name: &str,
    show_workspaces: bool,
    x: f64,
    y: f64,
) -> bool {
    if !show_workspaces || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, show_workspaces);
    x >= f64::from(left.layout_x) && x < f64::from(left.layout_x + left.layout_w)
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
    let profile = default_profile_for_app_name(active_app_name);
    check_top_bar_workspace_click_for_profile(profile, active_app_name, show_workspaces, x, y)
}

/// Profile-aware hit-test for the optional `TopBar` workspaces area (`RUSTYOS-068`).
#[must_use]
pub fn check_top_bar_workspace_click_for_profile(
    profile: TopBarAppProfile,
    active_app_name: &str,
    show_workspaces: bool,
    x: f64,
    y: f64,
) -> Option<u8> {
    if !show_workspaces || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, show_workspaces);
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
