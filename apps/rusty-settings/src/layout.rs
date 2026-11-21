//! Geometry layout and pointer hit-testing for `rusty-settings` (`RUSTYOS-103`).

use crate::state::{RowControlId, SHORTCUT_DEFS, SettingsAppState, SettingsPage, ThemeChoice};

#[allow(clippy::cast_possible_wrap)]
pub(crate) const WINDOW_CORNER_RADIUS: i32 = rusty_ui::WINDOW_RADIUS as i32;
pub(crate) const CARD_CORNER_RADIUS: i32 = 12;
pub(crate) const SIDEBAR_WIDTH: i32 = 230;
pub(crate) const HEADER_HEIGHT: i32 = 40;
pub(crate) const PAGE_MARGIN: i32 = 24;
const RESIZE_BORDER_PX: i32 = 5;

/// Axis-aligned rectangle in surface-local logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
        self.w > 0
            && self.h > 0
            && px >= f64::from(self.x)
            && px < f64::from(self.right())
            && py >= f64::from(self.y)
            && py < f64::from(self.bottom())
    }
}

/// Edge/corner hotspots for interactive CSD window resizing.
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
pub(crate) struct SidebarCategoryEntry {
    pub label: &'static str,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SidebarItemLayout {
    pub page: SettingsPage,
    pub rect: Rect,
    pub icon_rect: Rect,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceBadgeKind {
    Wifi,
    Ethernet,
    Firewall,
    Bluetooth,
    Battery,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationAppKind {
    Bell,
    Files,
    Terminal,
    Browser,
    Updater,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CardRowControl {
    Switch {
        action: RowControlId,
        active: bool,
    },
    Stepper {
        down: RowControlId,
        up: RowControlId,
        value: String,
    },
    Choice {
        action: RowControlId,
        value: String,
    },
    Navigation {
        target: SettingsPage,
        leading: Option<SettingsPage>,
    },
    ServiceStatus {
        badge_tint: [u8; 4],
        badge_kind: ServiceBadgeKind,
        subtitle: String,
        dot_rgba: [u8; 4],
        target: Option<SettingsPage>,
    },
    WifiHeaderSwitch {
        subtitle: String,
        action: RowControlId,
        active: bool,
    },
    ServiceHeaderSwitch {
        badge_tint: [u8; 4],
        badge_kind: ServiceBadgeKind,
        subtitle: String,
        action: RowControlId,
        active: bool,
    },
    BluetoothDeviceRow {
        subtitle: String,
        connected: bool,
        paired: bool,
        action: RowControlId,
        info_action: RowControlId,
    },
    DropdownTrigger {
        value: String,
    },
    BatterySegmentedTabs,
    AboutHeroCard {
        subtitle: String,
        action: RowControlId,
    },
    UserProfileCard {
        initials: String,
        subtitle: String,
        badge_tint: [u8; 4],
    },
    ShortcutBinding {
        combo: String,
        action: RowControlId,
    },
    WifiNetwork {
        connected: bool,
        secure: bool,
        signal_percent: u8,
        action: RowControlId,
    },
    NotificationHeader {
        subtitle: String,
        action: RowControlId,
        active: bool,
    },
    NotificationAppRow {
        app_kind: NotificationAppKind,
        badge_tint: [u8; 4],
        subtitle: String,
        active: bool,
        action: RowControlId,
    },
    Info {
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardRowLayout {
    pub rect: Rect,
    pub label: String,
    pub control: CardRowControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardSectionLayout {
    pub section_label: Option<(String, Rect)>,
    pub card_rect: Rect,
    pub rows: Vec<CardRowLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AppearanceTileLayout {
    pub choice: ThemeChoice,
    pub label: &'static str,
    pub ring_rect: Rect,
    pub artwork_rect: Rect,
    pub label_rect: Rect,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AppearanceCardsLayout {
    pub mode_label_rect: Rect,
    pub mode_card_rect: Rect,
    pub tiles: [AppearanceTileLayout; 3],
    pub accent_label_rect: Rect,
    pub accent_card_rect: Rect,
    pub swatches: [(Rect, [u8; 4], bool); 7],
    pub links_card_rect: Rect,
    pub links: [CardRowLayout; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DockPositionCardLayout {
    pub section_rect: Rect,
    pub card_rect: Rect,
    pub tiles: [(u8, &'static str, Rect, Rect, Rect, bool); 3],
}

/// Interactive click regions inside `rusty-settings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsHit {
    ResizeEdge(ResizeEdgeZone),
    WindowDrag,
    TrafficClose,
    TrafficMinimize,
    TrafficMaximize,
    NavBack,
    NavForward,
    SearchField,
    SidebarItem(SettingsPage),
    SelectTheme(ThemeChoice),
    SelectAccent(usize),
    Control(RowControlId),
    StepperMinus(RowControlId),
    StepperPlus(RowControlId),
    PushPage(SettingsPage),
    DockPosTile(u8),
    BatteryTab(usize),
    LowPowerDropdownTrigger,
    ScrollbarThumb,
}

#[derive(Debug, Clone)]
pub(crate) struct SettingsLayout {
    pub win_w: i32,
    pub win_h: i32,
    pub sidebar_rect: Rect,
    pub sidebar_viewport: Rect,
    pub divider_x: i32,
    pub page_rect: Rect,
    pub page_viewport: Rect,
    pub traffic_close: Rect,
    pub traffic_min: Rect,
    pub traffic_max: Rect,
    pub search_rect: Rect,
    pub nav_back: Rect,
    pub nav_forward: Rect,
    pub title_rect: Rect,
    pub categories: Vec<SidebarCategoryEntry>,
    pub sidebar_items: Vec<SidebarItemLayout>,
    pub max_sidebar_scroll: i32,
    pub appearance: Option<AppearanceCardsLayout>,
    pub dock_positions: Option<DockPositionCardLayout>,
    pub sections: Vec<CardSectionLayout>,
    pub low_power_dropdown_rect: Option<Rect>,
    pub max_page_scroll: i32,
    pub page_scrollbar_thumb: Option<Rect>,
}

#[must_use]
pub(crate) fn compute_layout(app: &SettingsAppState, width: u32, height: u32) -> SettingsLayout {
    let win_w = i32::try_from(width.max(640)).unwrap_or(882);
    let win_h = i32::try_from(height.max(500)).unwrap_or(600);
    let sidebar_rect = Rect::new(0, 0, SIDEBAR_WIDTH, win_h);
    let divider_x = SIDEBAR_WIDTH;
    let page_rect = Rect::new(divider_x + 1, 0, (win_w - divider_x - 1).max(100), win_h);
    let sidebar_viewport = Rect::new(0, 82, SIDEBAR_WIDTH, (win_h - 82).max(40));
    let page_viewport = Rect::new(
        page_rect.x,
        HEADER_HEIGHT,
        page_rect.w,
        (win_h - HEADER_HEIGHT).max(40),
    );

    let traffic_close = Rect::new(14, 13, 14, 14);
    let traffic_min = Rect::new(36, 13, 14, 14);
    let traffic_max = Rect::new(58, 13, 14, 14);
    let search_rect = Rect::new(12, 48, SIDEBAR_WIDTH - 24, 28);

    let nav_back = Rect::new(divider_x + 16, 11, 22, 18);
    let nav_forward = Rect::new(divider_x + 42, 11, 22, 18);
    let title_rect = Rect::new(divider_x + 70, 12, (page_rect.w - 90).max(80), 20);

    let (mut categories, mut sidebar_items, sidebar_bottom) = build_sidebar_items(app);
    let max_sidebar_scroll = (sidebar_bottom - win_h).max(0);
    let s_scroll = app.sidebar_scroll_y.clamp(0, max_sidebar_scroll);
    if s_scroll > 0 {
        for cat in &mut categories {
            cat.rect.y -= s_scroll;
        }
        for item in &mut sidebar_items {
            item.rect.y -= s_scroll;
            item.icon_rect.y -= s_scroll;
        }
    }

    let content_x = page_rect.x + PAGE_MARGIN;
    let content_w = (page_rect.w - PAGE_MARGIN * 2).max(200);

    let (mut appearance, mut dock_positions, mut sections) = match app.current_page {
        SettingsPage::Appearance => {
            let app_cards = build_appearance_layout(app, content_x, content_w);
            let glass_sec = build_appearance_glass_section(
                app,
                content_x,
                content_w,
                app_cards.links_card_rect.bottom() + 20,
            );
            (Some(app_cards), None, vec![glass_sec])
        }
        SettingsPage::DockDesktop => {
            let (dp, secs) = build_dock_layout(app, content_x, content_w);
            (None, Some(dp), secs)
        }
        other => (
            None,
            None,
            build_standard_page_sections(app, other, content_x, content_w),
        ),
    };

    let content_bottom =
        page_content_bottom(appearance.as_ref(), dock_positions.as_ref(), &sections);
    let max_page_scroll = (content_bottom - win_h).max(0);
    let p_scroll = app.page_scroll_y.clamp(0, max_page_scroll);
    if p_scroll > 0 {
        apply_page_scroll_offset(
            appearance.as_mut(),
            dock_positions.as_mut(),
            &mut sections,
            p_scroll,
        );
    }
    let page_scrollbar_thumb = compute_scrollbar_thumb(win_w, win_h, p_scroll, max_page_scroll);
    let low_power_dropdown_rect = extract_dropdown_rect(&sections);

    SettingsLayout {
        win_w,
        win_h,
        sidebar_rect,
        sidebar_viewport,
        divider_x,
        page_rect,
        page_viewport,
        traffic_close,
        traffic_min,
        traffic_max,
        search_rect,
        nav_back,
        nav_forward,
        title_rect,
        categories,
        sidebar_items,
        max_sidebar_scroll,
        appearance,
        dock_positions,
        sections,
        low_power_dropdown_rect,
        max_page_scroll,
        page_scrollbar_thumb,
    }
}

fn extract_dropdown_rect(sections: &[CardSectionLayout]) -> Option<Rect> {
    for sec in sections {
        for row in &sec.rows {
            if matches!(row.control, CardRowControl::DropdownTrigger { .. }) {
                return Some(dropdown_trigger_rect(row.rect));
            }
        }
    }
    None
}

#[allow(clippy::cast_possible_truncation)]
fn build_appearance_glass_section(
    app: &SettingsAppState,
    content_x: i32,
    content_w: i32,
    start_y: i32,
) -> CardSectionLayout {
    let mut cur_y = start_y;
    place_card_section(
        Some("Desktop & Glass"),
        content_x,
        &mut cur_y,
        content_w,
        vec![
            row_stepper(
                "Glass Opacity",
                RowControlId::GlassOpacityDown,
                RowControlId::GlassOpacityUp,
                format!("{}%", (app.glass_opacity * 100.0).round() as i32),
            ),
            row_switch(
                "Reduce Transparency",
                RowControlId::ToggleReduceTransparency,
                app.reduce_transparency,
            ),
            row_switch(
                "Reduced Motion",
                RowControlId::ToggleReducedMotion,
                app.reduced_motion,
            ),
        ],
    )
}

fn page_content_bottom(
    appearance: Option<&AppearanceCardsLayout>,
    dock_positions: Option<&DockPositionCardLayout>,
    sections: &[CardSectionLayout],
) -> i32 {
    let mut bottom = HEADER_HEIGHT + 24;
    if let Some(app) = appearance {
        bottom = bottom.max(app.links_card_rect.bottom() + 24);
    }
    if let Some(dp) = dock_positions {
        bottom = bottom.max(dp.card_rect.bottom() + 24);
    }
    for sec in sections {
        bottom = bottom.max(sec.card_rect.bottom() + 24);
    }
    bottom
}

fn apply_page_scroll_offset(
    appearance: Option<&mut AppearanceCardsLayout>,
    dock_positions: Option<&mut DockPositionCardLayout>,
    sections: &mut [CardSectionLayout],
    scroll_y: i32,
) {
    if let Some(app) = appearance {
        app.mode_label_rect.y -= scroll_y;
        app.mode_card_rect.y -= scroll_y;
        for tile in &mut app.tiles {
            tile.ring_rect.y -= scroll_y;
            tile.artwork_rect.y -= scroll_y;
            tile.label_rect.y -= scroll_y;
        }
        app.accent_label_rect.y -= scroll_y;
        app.accent_card_rect.y -= scroll_y;
        for (rect, _, _) in &mut app.swatches {
            rect.y -= scroll_y;
        }
        app.links_card_rect.y -= scroll_y;
        for row in &mut app.links {
            row.rect.y -= scroll_y;
        }
    }
    if let Some(dp) = dock_positions {
        dp.section_rect.y -= scroll_y;
        dp.card_rect.y -= scroll_y;
        for (_, _, ring, art, lbl, _) in &mut dp.tiles {
            ring.y -= scroll_y;
            art.y -= scroll_y;
            lbl.y -= scroll_y;
        }
    }
    for sec in sections {
        if let Some((_, rect)) = &mut sec.section_label {
            rect.y -= scroll_y;
        }
        sec.card_rect.y -= scroll_y;
        for row in &mut sec.rows {
            row.rect.y -= scroll_y;
        }
    }
}

fn compute_scrollbar_thumb(win_w: i32, win_h: i32, scroll_y: i32, max_scroll: i32) -> Option<Rect> {
    if max_scroll <= 0 {
        return None;
    }
    let view_h = (win_h - HEADER_HEIGHT).max(40);
    let track_top = HEADER_HEIGHT + 6;
    let track_h = (win_h - track_top - 8).max(24);
    let content_h = view_h + max_scroll;
    let thumb_h = ((track_h * view_h) / content_h.max(1)).clamp(28, track_h);
    let max_offset = (track_h - thumb_h).max(0);
    let thumb_y = track_top + (scroll_y.clamp(0, max_scroll) * max_offset) / max_scroll;
    Some(Rect::new(win_w - 7, thumb_y, 4, thumb_h))
}

fn build_sidebar_items(
    app: &SettingsAppState,
) -> (Vec<SidebarCategoryEntry>, Vec<SidebarItemLayout>, i32) {
    let mut categories = Vec::new();
    let mut items = Vec::new();
    let visible = app.visible_sidebar_pages();
    let active_root = app.current_page.sidebar_root();

    let mut cur_y = 76;
    let mut last_cat: Option<&'static str> = None;
    for page in visible {
        let cat = page.category();
        if cat == last_cat {
            cur_y += 2;
        } else {
            cur_y += if last_cat.is_none() { 16 } else { 14 };
            if let Some(label) = cat {
                categories.push(SidebarCategoryEntry {
                    label,
                    rect: Rect::new(17, cur_y, SIDEBAR_WIDTH - 34, 16),
                });
                cur_y += 20;
            }
            last_cat = cat;
        }
        let item_rect = Rect::new(12, cur_y, SIDEBAR_WIDTH - 24, 34);
        let icon_rect = Rect::new(item_rect.x + 5, item_rect.y + 5, 24, 24);
        items.push(SidebarItemLayout {
            page,
            rect: item_rect,
            icon_rect,
            selected: page == active_root,
        });
        cur_y += 34;
    }

    (categories, items, cur_y + 16)
}

fn build_appearance_layout(
    app: &SettingsAppState,
    content_x: i32,
    content_w: i32,
) -> AppearanceCardsLayout {
    let mode_label_rect = Rect::new(content_x + 16, 56, 180, 18);
    let mode_card_rect = Rect::new(content_x, 78, content_w, 120);
    let tile_specs = [
        (ThemeChoice::Light, "Light"),
        (ThemeChoice::Dark, "Dark"),
        (ThemeChoice::Auto, "Auto"),
    ];
    let tile_x0 = content_x + 16;
    let tile_stride = 132.min(((content_w - 32 - 108) / 2).max(116));
    let tiles = std::array::from_fn(|i| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let rx = tile_x0 + (i as i32) * tile_stride;
        let ring_rect = Rect::new(rx, 90, 108, 76);
        let artwork_rect = Rect::new(rx + 4, 94, 100, 68);
        let label_rect = Rect::new(rx, 171, 108, 18);
        AppearanceTileLayout {
            choice: tile_specs[i].0,
            label: tile_specs[i].1,
            ring_rect,
            artwork_rect,
            label_rect,
            selected: app.theme_choice == tile_specs[i].0,
        }
    });

    let accent_label_rect = Rect::new(content_x + 16, 218, 180, 18);
    let accent_card_rect = Rect::new(content_x, 240, content_w, 64);
    let swatch_x0 = content_x + 14;
    let swatch_stride = 52;
    let swatches = std::array::from_fn(|i| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let sx = swatch_x0 + (i as i32) * swatch_stride;
        (
            Rect::new(sx, 253, 38, 38),
            crate::state::ACCENT_COLORS[i],
            app.accent_index == i,
        )
    });

    let links_card_rect = Rect::new(content_x, 326, content_w, 132);
    let link_specs = [
        SettingsPage::Fonts,
        SettingsPage::IconTheme,
        SettingsPage::CursorStyle,
    ];
    let links = std::array::from_fn(|i| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let ry = links_card_rect.y + (i as i32) * 44;
        let target = link_specs[i];
        CardRowLayout {
            rect: Rect::new(content_x, ry, content_w, 44),
            label: target.title().to_owned(),
            control: CardRowControl::Navigation {
                target,
                leading: Some(target),
            },
        }
    });

    AppearanceCardsLayout {
        mode_label_rect,
        mode_card_rect,
        tiles,
        accent_label_rect,
        accent_card_rect,
        swatches,
        links_card_rect,
        links,
    }
}

#[must_use]
#[allow(clippy::cast_possible_truncation)]
fn build_dock_layout(
    app: &SettingsAppState,
    content_x: i32,
    content_w: i32,
) -> (DockPositionCardLayout, Vec<CardSectionLayout>) {
    let section_rect = Rect::new(content_x + 16, 56, 180, 18);
    let card_rect = Rect::new(content_x, 78, content_w, 122);
    let labels = [(0_u8, "Left"), (1_u8, "Bottom"), (2_u8, "Right")];
    let active_idx = match app.dock_position.as_str() {
        "left" => 0,
        "right" => 2,
        _ => 1,
    };
    let tile_x0 = content_x + 16;
    let tiles = std::array::from_fn(|i| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let rx = tile_x0 + (i as i32) * 114;
        let ring = Rect::new(rx, 88, 88, 84);
        let art = Rect::new(rx + 4, 92, 80, 76);
        let lbl = Rect::new(rx, 174, 88, 18);
        (
            labels[i].0,
            labels[i].1,
            ring,
            art,
            lbl,
            active_idx == labels[i].0,
        )
    });

    let dock_positions = DockPositionCardLayout {
        section_rect,
        card_rect,
        tiles,
    };

    let mut cur_y = card_rect.bottom() + 18;
    let rows = vec![
        row_stepper(
            "Dock Size",
            RowControlId::DockSizeDown,
            RowControlId::DockSizeUp,
            format!("{} px", app.dock_icon_size),
        ),
        row_switch(
            "Reduce Transparency",
            RowControlId::ToggleReduceTransparency,
            app.reduce_transparency,
        ),
        row_switch(
            "Reduced Motion",
            RowControlId::ToggleReducedMotion,
            app.reduced_motion,
        ),
        row_stepper(
            "Glass Opacity",
            RowControlId::GlassOpacityDown,
            RowControlId::GlassOpacityUp,
            format!("{}%", (app.glass_opacity * 100.0).round() as i32),
        ),
        row_switch(
            "Auto-hide the Dock",
            RowControlId::DockAutoHideToggle,
            app.dock_auto_hide,
        ),
        row_choice(
            "Minimize using",
            RowControlId::DockMinimizeEffectCycle,
            app.dock_minimize_effect.clone(),
        ),
        row_switch(
            "Minimize into app icon",
            RowControlId::DockMinimizeIntoAppToggle,
            app.dock_minimize_into_app,
        ),
        row_switch(
            "Show open indicators",
            RowControlId::DockIndicatorsToggle,
            app.dock_show_indicators,
        ),
        row_switch(
            "Show recent apps",
            RowControlId::DockRecentToggle,
            app.dock_show_recent,
        ),
    ];
    let sec = place_card_section(None, content_x, &mut cur_y, content_w, rows);
    (dock_positions, vec![sec])
}

fn build_standard_page_sections(
    app: &SettingsAppState,
    page: SettingsPage,
    content_x: i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut cur_y = 56;
    match page {
        SettingsPage::Wallpaper
        | SettingsPage::MenuBar
        | SettingsPage::Notifications
        | SettingsPage::DateTime
        | SettingsPage::Language
        | SettingsPage::SoftwareUpdate
        | SettingsPage::Wifi
        | SettingsPage::Bluetooth
        | SettingsPage::Network
        | SettingsPage::About
        | SettingsPage::Battery
        | SettingsPage::Users => {
            build_system_page_sections(app, page, content_x, &mut cur_y, content_w)
        }
        SettingsPage::Display | SettingsPage::Sound | SettingsPage::Keyboard => {
            build_hardware_page_sections(app, page, content_x, &mut cur_y, content_w)
        }
        SettingsPage::Shortcuts => {
            build_shortcuts_page_sections(app, content_x, &mut cur_y, content_w)
        }
        SettingsPage::Mouse
        | SettingsPage::Touchpad
        | SettingsPage::Fonts
        | SettingsPage::IconTheme
        | SettingsPage::CursorStyle => {
            build_style_page_sections(app, page, content_x, &mut cur_y, content_w)
        }
        SettingsPage::Appearance | SettingsPage::DockDesktop => Vec::new(),
    }
}

#[allow(clippy::too_many_lines)]
fn build_system_page_sections(
    app: &SettingsAppState,
    page: SettingsPage,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    match page {
        SettingsPage::Wallpaper => {
            let mode = if app.custom_wallpaper.is_some() {
                "Custom PNG"
            } else {
                "RustyOS Default"
            };
            vec![place_card_section(
                Some("Desktop Picture"),
                content_x,
                cur_y,
                content_w,
                vec![
                    row_choice("Wallpaper", RowControlId::ToggleWallpaper, mode.to_owned()),
                    row_info(
                        "Source",
                        app.custom_wallpaper
                            .as_deref()
                            .unwrap_or("Built-in RustyOS Wallpaper")
                            .to_owned(),
                    ),
                ],
            )]
        }
        SettingsPage::MenuBar => {
            vec![
                place_card_section(
                    Some("Status Icons"),
                    content_x,
                    cur_y,
                    content_w,
                    vec![
                        row_switch(
                            "Show Wi-Fi Indicator",
                            RowControlId::MenubarWifiToggle,
                            app.menubar_wifi,
                        ),
                        row_switch(
                            "Show Battery Indicator",
                            RowControlId::MenubarBatteryToggle,
                            app.menubar_battery,
                        ),
                        row_switch(
                            "Show Battery Percentage",
                            RowControlId::MenubarBatteryPercentageToggle,
                            app.menubar_battery_percentage,
                        ),
                        row_switch(
                            "Show Volume Indicator",
                            RowControlId::MenubarVolumeToggle,
                            app.menubar_volume,
                        ),
                        row_switch(
                            "Show Bluetooth Indicator",
                            RowControlId::MenubarBluetoothToggle,
                            app.menubar_bluetooth,
                        ),
                        row_switch(
                            "Show Input Source Badge",
                            RowControlId::MenubarInputToggle,
                            app.menubar_input,
                        ),
                        row_switch(
                            "Show Workspace Strip",
                            RowControlId::MenubarWorkspacesToggle,
                            app.menubar_workspaces,
                        ),
                        row_switch(
                            "Show System Tray Applets",
                            RowControlId::MenubarTrayToggle,
                            app.menubar_tray,
                        ),
                    ],
                ),
                place_card_section(
                    Some("Appearance & Clock"),
                    content_x,
                    cur_y,
                    content_w,
                    vec![
                        row_switch(
                            "Adaptive Wallpaper Contrast",
                            RowControlId::MenubarAdaptiveContrastToggle,
                            app.menubar_adaptive_contrast,
                        ),
                        row_switch(
                            "Use 24-Hour Clock",
                            RowControlId::Clock24hToggle,
                            app.clock_24h,
                        ),
                        row_choice(
                            "Clock Language",
                            RowControlId::ClockLocaleCycle,
                            match app.clock_locale.as_str() {
                                "ru" => "Русский",
                                "uk" => "Українська",
                                _ => "English",
                            }
                            .to_owned(),
                        ),
                    ],
                ),
            ]
        }
        SettingsPage::DateTime => {
            let now_label = rusty_services::local_clock_short_datetime_localized(
                &app.clock_locale,
                app.clock_24h,
            );
            vec![place_card_section(
                Some("Clock & Time Format"),
                content_x,
                cur_y,
                content_w,
                vec![
                    row_info("Current Time", now_label),
                    row_switch(
                        "Use 24-Hour Clock",
                        RowControlId::Clock24hToggle,
                        app.clock_24h,
                    ),
                    row_choice(
                        "Clock Language",
                        RowControlId::ClockLocaleCycle,
                        match app.clock_locale.as_str() {
                            "ru" => "Русский",
                            "uk" => "Українська",
                            _ => "English",
                        }
                        .to_owned(),
                    ),
                ],
            )]
        }
        SettingsPage::Language => {
            vec![place_card_section(
                Some("Preferred Language"),
                content_x,
                cur_y,
                content_w,
                vec![
                    row_choice(
                        "System & Clock Language",
                        RowControlId::ClockLocaleCycle,
                        match app.clock_locale.as_str() {
                            "ru" => "Русский",
                            "uk" => "Українська",
                            _ => "English",
                        }
                        .to_owned(),
                    ),
                    row_info(
                        "Supported Languages",
                        "English • Русский • Українська".to_owned(),
                    ),
                ],
            )]
        }
        SettingsPage::Notifications => {
            build_notifications_page_sections(app, content_x, cur_y, content_w)
        }
        SettingsPage::SoftwareUpdate => {
            build_software_update_sections(app, content_x, cur_y, content_w)
        }
        SettingsPage::Wifi => build_wifi_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::Bluetooth => build_bluetooth_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::Network => build_network_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::About => build_about_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::Battery => build_battery_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::Users => build_users_page_sections(app, content_x, cur_y, content_w),
        _ => Vec::new(),
    }
}

fn build_bluetooth_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let subtitle = if !app.bluetooth_status_hint.is_empty() {
        app.bluetooth_status_hint.clone()
    } else if !app.bluetooth.status.available {
        String::from("No Bluetooth hardware adapter detected.")
    } else if app.bluetooth.status.powered {
        let host = app
            .bluetooth
            .adapter_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&app.about_info.hostname);
        format!("Discoverable as \"{host}\"")
    } else {
        String::from("Turn on Bluetooth to connect headphones, mice, and keyboards.")
    };

    let mut header_rows = vec![RowSpec {
        label: String::from("Bluetooth"),
        height: 62,
        control: CardRowControl::ServiceHeaderSwitch {
            badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
            badge_kind: ServiceBadgeKind::Bluetooth,
            subtitle,
            action: RowControlId::BluetoothPoweredToggle,
            active: app.bluetooth.status.powered,
        },
    }];
    if app.bluetooth.status.available && app.bluetooth.status.powered {
        header_rows.push(row_choice(
            if app.bluetooth.discovering {
                "Scanning for Nearby Devices..."
            } else {
                "Nearby Discovery"
            },
            RowControlId::BluetoothScanToggle,
            String::from(if app.bluetooth.discovering {
                "Stop Scan"
            } else {
                "Scan"
            }),
        ));
    }
    sections.push(place_card_section(
        None,
        content_x,
        cur_y,
        content_w,
        header_rows,
    ));

    append_bluetooth_device_sections(app, content_x, cur_y, content_w, &mut sections);
    sections
}

fn append_bluetooth_device_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    sections: &mut Vec<CardSectionLayout>,
) {
    let mut my_rows = Vec::new();
    let mut nearby_rows = Vec::new();
    for (idx, dev) in app.bluetooth.devices.iter().enumerate() {
        let sub = if dev.connected {
            dev.battery_percent.map_or_else(
                || String::from("Connected"),
                |pct| format!("Connected · {pct}%"),
            )
        } else if dev.paired {
            String::from("Not Connected")
        } else {
            dev.address.clone()
        };
        let row = RowSpec {
            label: dev.name.clone(),
            height: 50,
            control: CardRowControl::BluetoothDeviceRow {
                subtitle: sub,
                connected: dev.connected,
                paired: dev.paired,
                action: RowControlId::BluetoothActionIndex(idx),
                info_action: RowControlId::BluetoothInfoIndex(idx),
            },
        };
        if dev.paired || dev.connected {
            my_rows.push(row);
        } else {
            nearby_rows.push(row);
        }
    }

    if my_rows.is_empty() {
        my_rows.push(row_info(
            "Paired Devices",
            String::from(if app.bluetooth.status.available {
                "None"
            } else {
                "Adapter Unavailable"
            }),
        ));
    }
    sections.push(place_card_section(
        Some("My Devices"),
        content_x,
        cur_y,
        content_w,
        my_rows,
    ));

    if app.bluetooth.status.powered {
        if nearby_rows.is_empty() {
            nearby_rows.push(row_info(
                "Nearby Devices",
                String::from(if app.bluetooth.discovering {
                    "Searching..."
                } else {
                    "Click Scan to discover devices"
                }),
            ));
        }
        sections.push(place_card_section(
            Some("Nearby Devices"),
            content_x,
            cur_y,
            content_w,
            nearby_rows,
        ));
    }
}

fn build_battery_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let st = &app.power.status;
    let power_sub = if st.has_battery {
        let verb = match st.state {
            rusty_services::BatteryState::Charging => "Charging",
            rusty_services::BatteryState::Discharging => "On Battery",
            rusty_services::BatteryState::Full => "Fully Charged",
            rusty_services::BatteryState::Unknown => "Battery Detected",
        };
        format!("{}% · {verb}", st.percentage)
    } else {
        String::from("AC Power · No Hardware Battery Detected")
    };

    let mut top_rows = vec![
        RowSpec {
            label: String::from("Battery & Power"),
            height: 58,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0x34, 0xC7, 0x59, 0xFF],
                badge_kind: ServiceBadgeKind::Battery,
                subtitle: power_sub,
                dot_rgba: if st.ac_online {
                    [40, 205, 65, 255]
                } else {
                    [245, 166, 35, 255]
                },
                target: None,
            },
        },
        RowSpec {
            label: String::from("Low Power Mode"),
            height: 44,
            control: CardRowControl::DropdownTrigger {
                value: app.low_power_dropdown.selected_label().to_owned(),
            },
        },
        row_choice(
            "Battery Health",
            RowControlId::BatteryDetailsOpen,
            format!("{} (i)", app.power.condition.label()),
        ),
    ];
    if !app.power.available_profiles.is_empty()
        && let Some(active) = app.power.active_profile
    {
        top_rows.push(row_choice(
            "System Power Profile",
            RowControlId::PowerProfileCycle,
            active.label().to_owned(),
        ));
    }
    sections.push(place_card_section(
        None, content_x, cur_y, content_w, top_rows,
    ));
    sections.push(build_battery_details_section(
        app, content_x, cur_y, content_w,
    ));
    sections
}

fn build_battery_details_section(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> CardSectionLayout {
    let st = &app.power.status;
    let mut detail_rows = vec![RowSpec {
        label: String::new(),
        height: 44,
        control: CardRowControl::BatterySegmentedTabs,
    }];
    if app.battery_tab.selected == 0 {
        detail_rows.push(row_info(
            "Power Source",
            String::from(if st.ac_online {
                "Power Adapter (AC)"
            } else {
                "Battery"
            }),
        ));
        if st.has_battery {
            detail_rows.push(row_info("Charge Level", format!("{}%", st.percentage)));
        }
        if let Some(mins) = app.power.time_remaining_minutes {
            let h = mins / 60;
            let m = mins % 60;
            detail_rows.push(row_info("Time Estimate", format!("{h}h {m:02}m")));
        }
        if let Some(watts) = app.power.power_watts {
            detail_rows.push(row_info("Power Rate", format!("{watts:.1} W")));
        }
        if let Some(model) = &app.power.model_name {
            detail_rows.push(row_info("Battery Model", model.clone()));
        }
    } else {
        let cap_str = app
            .power
            .health_percent
            .map_or_else(|| String::from("N/A"), |p| format!("{p}%"));
        detail_rows.push(row_info("Maximum Capacity", cap_str));
        if let Some(cycles) = app.power.cycle_count {
            detail_rows.push(row_info("Cycle Count", cycles.to_string()));
        }
        let sleep_lbl = if app.power_display_sleep_minutes == 0 {
            String::from("Never")
        } else {
            format!("{} min", app.power_display_sleep_minutes)
        };
        detail_rows.push(row_choice(
            "Turn Display Off on Inactivity",
            RowControlId::PowerDisplaySleepCycle,
            sleep_lbl,
        ));
        let lock_lbl = if app.power_auto_lock_minutes == 0 {
            String::from("Off")
        } else {
            format!("{} min", app.power_auto_lock_minutes)
        };
        detail_rows.push(row_choice(
            "Auto-Lock Screen After",
            RowControlId::PowerAutoLockCycle,
            lock_lbl,
        ));
        detail_rows.push(row_switch(
            "Slightly Dim Display on Battery",
            RowControlId::PowerDimOnBatteryToggle,
            app.power_dim_on_battery,
        ));
        detail_rows.push(row_switch(
            "Lock Screen on Sleep",
            RowControlId::LockSleepToggle,
            app.lock_on_sleep,
        ));
        detail_rows.push(row_choice(
            "Additional Power Settings",
            RowControlId::PowerOptionsOpen,
            String::from("Options..."),
        ));
    }

    place_card_section(
        Some("Energy & Health"),
        content_x,
        cur_y,
        content_w,
        detail_rows,
    )
}

fn build_about_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let about = &app.about_info;
    let mut sections = Vec::new();
    sections.push(place_card_section(
        None,
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: about.hostname.clone(),
            height: 82,
            control: CardRowControl::AboutHeroCard {
                subtitle: format!("{} ({})", about.os_name, about.architecture),
                action: RowControlId::AboutMoreInfoOpen,
            },
        }],
    ));
    sections.push(place_card_section(
        Some("Hardware & Software"),
        content_x,
        cur_y,
        content_w,
        vec![
            row_info("Processor", about.cpu.clone()),
            row_info("Memory", about.memory.clone()),
            row_info("Graphics", about.graphics.clone()),
            row_info("Startup Disk", about.startup_disk.clone()),
            row_info("Kernel", about.kernel.clone()),
            row_info("Compositor", String::from("RustyOS Wayland (Smithay)")),
        ],
    ));
    sections.push(place_card_section(
        Some("Related"),
        content_x,
        cur_y,
        content_w,
        vec![
            RowSpec {
                label: String::from("Software Update"),
                height: 44,
                control: CardRowControl::Navigation {
                    target: SettingsPage::SoftwareUpdate,
                    leading: Some(SettingsPage::SoftwareUpdate),
                },
            },
            RowSpec {
                label: String::from("Displays"),
                height: 44,
                control: CardRowControl::Navigation {
                    target: SettingsPage::Display,
                    leading: Some(SettingsPage::Display),
                },
            },
        ],
    ));
    sections
}

fn build_users_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let details = app.user_details.as_ref();
    let username = details.map_or("user", |d| d.username.as_str());
    let display_name = details.map_or(username, |d| d.display_name.as_str());
    let role = if details.is_some_and(|d| d.is_admin) {
        "Administrator"
    } else {
        "Standard User"
    };
    let initials: String = display_name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    let initials = if initials.is_empty() {
        String::from("U")
    } else {
        initials
    };

    sections.push(place_card_section(
        Some("Current Account"),
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: display_name.to_owned(),
            height: 62,
            control: CardRowControl::UserProfileCard {
                initials,
                subtitle: format!("@{username} · {role}"),
                badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
            },
        }],
    ));

    if let Some(det) = details {
        let groups_str = if det.groups.is_empty() {
            String::from("None")
        } else {
            let joined = det.groups.join(", ");
            if joined.len() > 38 {
                format!("{} groups ({})", det.groups.len(), det.groups[0])
            } else {
                joined
            }
        };
        sections.push(place_card_section(
            Some("Account Details"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_info("Username", det.username.clone()),
                row_info("Account Role", String::from(role)),
                row_info("User / Group ID", format!("{} : {}", det.uid, det.gid)),
                row_info("Home Directory", det.home_dir.clone()),
                row_info("Login Shell", det.shell.clone()),
                row_info("Session", det.session_type.clone()),
                row_info("Groups", groups_str),
            ],
        ));
    }

    let mut local_rows: Vec<RowSpec> = app
        .all_users
        .iter()
        .map(|u| {
            row_info_string(
                u.display_name.clone(),
                format!("@{} (UID {})", u.username, u.uid),
            )
        })
        .collect();
    local_rows.push(row_info(
        "Manage Users",
        String::from("Read-only (AccountsService & Polkit separate)"),
    ));
    sections.push(place_card_section(
        Some("Local Users"),
        content_x,
        cur_y,
        content_w,
        local_rows,
    ));
    sections
}

fn build_shortcuts_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut system_rows = Vec::new();
    let mut wm_rows = Vec::new();
    for (idx, def) in SHORTCUT_DEFS.iter().enumerate() {
        let row = RowSpec {
            label: def.label.to_owned(),
            height: 44,
            control: CardRowControl::ShortcutBinding {
                combo: app.shortcuts[idx].clone(),
                action: RowControlId::ShortcutEditIndex(idx),
            },
        };
        if def.category == "System & Shell" {
            system_rows.push(row);
        } else {
            wm_rows.push(row);
        }
    }
    vec![
        place_card_section(
            Some("System & Shell"),
            content_x,
            cur_y,
            content_w,
            system_rows,
        ),
        place_card_section(
            Some("Windows & Tiling"),
            content_x,
            cur_y,
            content_w,
            wm_rows,
        ),
        place_card_section(
            Some("Defaults"),
            content_x,
            cur_y,
            content_w,
            vec![row_choice(
                "Restore Default Keybindings",
                RowControlId::ShortcutsResetDefaults,
                String::from("Reset Defaults"),
            )],
        ),
    ]
}

#[allow(clippy::too_many_lines)]
fn build_notifications_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = vec![
        place_card_section_with_gap(
            None,
            content_x,
            cur_y,
            content_w,
            vec![RowSpec {
                label: String::from("Notifications"),
                height: 64,
                control: CardRowControl::NotificationHeader {
                    subtitle: String::from(
                        "Customize banners, sounds, and Notification Center behavior.",
                    ),
                    action: RowControlId::NotifAllowToggle,
                    active: app.notif_allow,
                },
            }],
            18,
        ),
        place_card_section(
            Some("Notification Center"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Show previews",
                    RowControlId::NotifShowPreviewsCycle,
                    app.notif_show_previews.clone(),
                ),
                row_switch(
                    "when display is sleeping",
                    RowControlId::NotifWhenSleepingToggle,
                    app.notif_when_sleeping,
                ),
                row_switch(
                    "when screen is locked",
                    RowControlId::NotifWhenLockedToggle,
                    app.notif_when_locked,
                ),
                row_choice(
                    "when mirroring or sharing the display",
                    RowControlId::NotifMirroringCycle,
                    String::from(if app.notif_mirroring_off {
                        "Notifications Off"
                    } else {
                        "Allow"
                    }),
                ),
            ],
        ),
    ];

    append_notification_app_rows_section(app, content_x, cur_y, content_w, &mut sections);

    sections.push(place_card_section(
        Some("Menu Bar & Updates"),
        content_x,
        cur_y,
        content_w,
        vec![
            row_switch("24-Hour Clock", RowControlId::Clock24hToggle, app.clock_24h),
            row_switch(
                "Automatic Update Checks",
                RowControlId::UpdatesEnabledToggle,
                app.updates_enabled,
            ),
            row_switch(
                "Show Workspace Strip",
                RowControlId::MenubarWorkspacesToggle,
                app.menubar_workspaces,
            ),
            row_switch(
                "Show System Tray Applets",
                RowControlId::MenubarTrayToggle,
                app.menubar_tray,
            ),
            row_switch(
                "Show Input Source Badge",
                RowControlId::MenubarInputToggle,
                app.menubar_input,
            ),
            row_switch(
                "Show Wi-Fi Indicator",
                RowControlId::MenubarWifiToggle,
                app.menubar_wifi,
            ),
            row_switch(
                "Show Battery Indicator",
                RowControlId::MenubarBatteryToggle,
                app.menubar_battery,
            ),
            row_switch(
                "Show Battery Percentage",
                RowControlId::MenubarBatteryPercentageToggle,
                app.menubar_battery_percentage,
            ),
            row_switch(
                "Show Volume Indicator",
                RowControlId::MenubarVolumeToggle,
                app.menubar_volume,
            ),
            row_switch(
                "Show Bluetooth Indicator",
                RowControlId::MenubarBluetoothToggle,
                app.menubar_bluetooth,
            ),
            row_switch(
                "Adaptive Wallpaper Contrast",
                RowControlId::MenubarAdaptiveContrastToggle,
                app.menubar_adaptive_contrast,
            ),
            row_switch(
                "Use 24-Hour Clock",
                RowControlId::Clock24hToggle,
                app.clock_24h,
            ),
            row_choice(
                "Clock Language",
                RowControlId::ClockLocaleCycle,
                match app.clock_locale.as_str() {
                    "ru" => "Русский",
                    "uk" => "Українська",
                    _ => "English",
                }
                .to_owned(),
            ),
        ],
    ));
    sections
}

fn append_notification_app_rows_section(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    sections: &mut Vec<CardSectionLayout>,
) {
    let app_specs: [(&str, NotificationAppKind, [u8; 4]); 5] = [
        (
            "Files",
            NotificationAppKind::Files,
            [0x0A, 0x84, 0xFF, 0xFF],
        ),
        (
            "Terminal",
            NotificationAppKind::Terminal,
            [0x36, 0x3A, 0x45, 0xFF],
        ),
        (
            "Browser",
            NotificationAppKind::Browser,
            [0x24, 0x91, 0xFF, 0xFF],
        ),
        (
            "Software Update",
            NotificationAppKind::Updater,
            [0x34, 0xC7, 0x59, 0xFF],
        ),
        (
            "System Settings",
            NotificationAppKind::Settings,
            [0x76, 0x7A, 0x84, 0xFF],
        ),
    ];
    let app_rows = app_specs
        .into_iter()
        .enumerate()
        .map(|(idx, (name, kind, tint))| {
            let active = app.notif_app_enabled.get(idx).copied().unwrap_or(true);
            RowSpec {
                label: name.to_owned(),
                height: 52,
                control: CardRowControl::NotificationAppRow {
                    app_kind: kind,
                    badge_tint: tint,
                    subtitle: String::from(if active {
                        "Banners, Badges, Sounds"
                    } else {
                        "Off"
                    }),
                    active,
                    action: RowControlId::NotifAppToggle(idx),
                },
            }
        })
        .collect();
    sections.push(place_card_section(
        Some("Application Notifications"),
        content_x,
        cur_y,
        content_w,
        app_rows,
    ));
}

fn build_wifi_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let mut header_rows = vec![RowSpec {
        label: String::from("Wi-Fi"),
        height: 62,
        control: CardRowControl::WifiHeaderSwitch {
            subtitle: String::from("Set up Wi-Fi to wirelessly connect to the internet."),
            action: RowControlId::WifiEnabledToggle,
            active: app.network.wifi_enabled,
        },
    }];
    if app.network.wifi_connected || app.network.wifi_ssid.is_some() {
        let active_name = app
            .network
            .wifi_ssid
            .clone()
            .unwrap_or_else(|| String::from("Connected Network"));
        let sig_str = app.network.wifi_signal_percent.map_or_else(
            || String::from("Connected"),
            |s| format!("Connected · {s}%"),
        );
        header_rows.push(RowSpec {
            label: active_name,
            height: 54,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
                badge_kind: ServiceBadgeKind::Wifi,
                subtitle: sig_str,
                dot_rgba: [40, 205, 65, 255],
                target: None,
            },
        });
    }
    sections.push(place_card_section(
        None,
        content_x,
        cur_y,
        content_w,
        header_rows,
    ));

    if app.network.wifi_enabled {
        append_wifi_network_sections(app, content_x, cur_y, content_w, &mut sections);
    } else {
        sections.push(place_card_section(
            Some("Known Networks"),
            content_x,
            cur_y,
            content_w,
            vec![row_info("Wi-Fi Radio", String::from("Off"))],
        ));
    }

    sections.push(place_card_section(
        Some("Security & Lock Screen"),
        content_x,
        cur_y,
        content_w,
        vec![
            row_switch(
                "Require Password on Lock",
                RowControlId::LockPasswordToggle,
                app.require_password_on_lock,
            ),
            row_switch(
                "Lock Screen on Sleep",
                RowControlId::LockSleepToggle,
                app.lock_on_sleep,
            ),
            row_switch(
                "Built-in Polkit Agent",
                RowControlId::PolkitToggle,
                app.enable_polkit_agent,
            ),
        ],
    ));
    sections
}

fn append_wifi_network_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    sections: &mut Vec<CardSectionLayout>,
) {
    let mut known_rows = Vec::new();
    let mut other_rows = Vec::new();
    for (idx, ap) in app.network.wifi_networks.iter().enumerate() {
        let row = RowSpec {
            label: ap.ssid.clone(),
            height: 40,
            control: CardRowControl::WifiNetwork {
                connected: ap.connected,
                secure: ap.secure,
                signal_percent: ap.signal_percent,
                action: RowControlId::WifiConnectIndex(idx),
            },
        };
        if ap.connected || ap.known {
            known_rows.push(row);
        } else {
            other_rows.push(row);
        }
    }
    if !known_rows.is_empty() {
        sections.push(place_card_section(
            Some("Known Networks"),
            content_x,
            cur_y,
            content_w,
            known_rows,
        ));
    }
    other_rows.push(RowSpec {
        label: String::from("Scan Nearby Networks"),
        height: 44,
        control: CardRowControl::Choice {
            action: RowControlId::NetworkRefresh,
            value: String::from("Refresh"),
        },
    });
    sections.push(place_card_section(
        Some("Other Networks"),
        content_x,
        cur_y,
        content_w,
        other_rows,
    ));
}

fn build_network_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let green_dot = [40, 205, 65, 255];
    let amber_dot = [245, 166, 35, 255];
    let gray_dot = [152, 152, 160, 255];

    let (wifi_sub, wifi_dot) = if app.network.wifi_connected {
        let name = app.network.wifi_ssid.as_deref().unwrap_or("Connected");
        (format!("Connected · {name}"), green_dot)
    } else if app.network.wifi_enabled {
        (String::from("On · Not Connected"), amber_dot)
    } else {
        (String::from("Off"), gray_dot)
    };
    sections.push(place_card_section_with_gap(
        None,
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: String::from("Wi-Fi"),
            height: 54,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
                badge_kind: ServiceBadgeKind::Wifi,
                subtitle: wifi_sub,
                dot_rgba: wifi_dot,
                target: Some(SettingsPage::Wifi),
            },
        }],
        12,
    ));

    let (eth_sub, eth_dot) = if app.network.wired_connected {
        let label = app.network.wired_interface.as_deref().map_or_else(
            || String::from("Connected"),
            |iface| format!("Connected · {iface}"),
        );
        (label, green_dot)
    } else {
        (String::from("Cable Unplugged"), gray_dot)
    };
    sections.push(place_card_section_with_gap(
        None,
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: String::from("Ethernet"),
            height: 54,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
                badge_kind: ServiceBadgeKind::Ethernet,
                subtitle: eth_sub,
                dot_rgba: eth_dot,
                target: None,
            },
        }],
        12,
    ));

    sections.push(place_card_section_with_gap(
        None,
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: String::from("Firewall"),
            height: 54,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0xFF, 0x8D, 0x28, 0xFF],
                badge_kind: ServiceBadgeKind::Firewall,
                subtitle: String::from("Inactive"),
                dot_rgba: gray_dot,
                target: None,
            },
        }],
        20,
    ));

    sections.push(place_card_section(
        Some("Other Services"),
        content_x,
        cur_y,
        content_w,
        vec![
            row_info(
                "Primary Interface",
                app.network
                    .interface
                    .clone()
                    .unwrap_or_else(|| String::from("None")),
            ),
            row_info("Backend", String::from("NetworkManager / D-Bus")),
            row_choice(
                "Network Status",
                RowControlId::NetworkRefresh,
                String::from("Refresh"),
            ),
        ],
    ));
    sections
}

#[allow(clippy::cast_possible_truncation)]
fn build_hardware_page_sections(
    app: &SettingsAppState,
    page: SettingsPage,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    match page {
        SettingsPage::Display => vec![place_card_section(
            Some("Display Configuration"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_stepper(
                    "HiDPI Scale",
                    RowControlId::ScaleDown,
                    RowControlId::ScaleUp,
                    format!("{}%", (app.display_scale * 100.0).round() as i32),
                ),
                row_switch(
                    "Prefer Maximum Refresh Rate",
                    RowControlId::PreferMaxRefreshToggle,
                    app.prefer_max_refresh,
                ),
                row_choice(
                    "Adaptive Sync (VRR)",
                    RowControlId::VrrModeCycle,
                    app.vrr_mode.clone(),
                ),
                row_choice(
                    "Rotation",
                    RowControlId::RotationCycle,
                    format!("{} deg", app.display_rotation),
                ),
            ],
        )],
        SettingsPage::Sound => vec![place_card_section(
            Some("Output Volume"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_stepper(
                    "Master Volume",
                    RowControlId::VolumeDown,
                    RowControlId::VolumeUp,
                    format!("{}%", app.volume_percent),
                ),
                row_switch("Mute Output", RowControlId::ToggleMute, app.muted),
            ],
        )],
        SettingsPage::Keyboard => vec![place_card_section(
            Some("Keyboard & Input Sources"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Input Layouts",
                    RowControlId::KeyboardLayoutCycle,
                    app.keyboard_layout.clone(),
                ),
                row_choice(
                    "Switch Shortcut",
                    RowControlId::KeyboardOptionCycle,
                    app.keyboard_options.clone(),
                ),
                row_stepper(
                    "Repeat Rate",
                    RowControlId::RepeatRateDown,
                    RowControlId::RepeatRateUp,
                    format!("{}/s", app.repeat_rate),
                ),
                row_stepper(
                    "Repeat Delay",
                    RowControlId::RepeatDelayDown,
                    RowControlId::RepeatDelayUp,
                    format!("{} ms", app.repeat_delay),
                ),
                RowSpec {
                    label: String::from("Keyboard Shortcuts..."),
                    height: 44,
                    control: CardRowControl::Navigation {
                        target: SettingsPage::Shortcuts,
                        leading: Some(SettingsPage::Shortcuts),
                    },
                },
            ],
        )],
        _ => Vec::new(),
    }
}

fn build_style_page_sections(
    app: &SettingsAppState,
    page: SettingsPage,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    match page {
        SettingsPage::Mouse => vec![place_card_section(
            Some("Pointer & Mouse"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Cursor Style",
                    RowControlId::CursorThemeCycle,
                    app.cursor_theme.clone(),
                ),
                row_info("Acceleration Profile", String::from("Adaptive (libinput)")),
            ],
        )],
        SettingsPage::Touchpad => vec![place_card_section(
            Some("Touchpad"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_info("Tap to Click", String::from("Enabled")),
                row_info("Two-Finger Scroll", String::from("Natural")),
            ],
        )],
        SettingsPage::Fonts => vec![place_card_section(
            Some("System Typography"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_info("UI Font", String::from("Inter Regular / SemiBold")),
                row_info("Monospace", String::from("JetBrains Mono")),
                row_info("Default Size", String::from("9 pt")),
            ],
        )],
        SettingsPage::IconTheme => vec![place_card_section(
            Some("Icon Theme"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_info("Active Theme", String::from("RustyOS System")),
                row_info("Dock Icons", String::from("Built-in Vector Squircles")),
            ],
        )],
        SettingsPage::CursorStyle => vec![place_card_section(
            Some("Cursor Style"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Cursor Theme",
                    RowControlId::CursorThemeCycle,
                    app.cursor_theme.clone(),
                ),
                row_info("Cursor Size", String::from("24 px (HiDPI-aware)")),
            ],
        )],
        _ => Vec::new(),
    }
}

struct RowSpec {
    label: String,
    height: i32,
    control: CardRowControl,
}

fn row_switch(label: &'static str, action: RowControlId, active: bool) -> RowSpec {
    RowSpec {
        label: label.to_owned(),
        height: 44,
        control: CardRowControl::Switch { action, active },
    }
}

fn row_stepper(
    label: &'static str,
    down: RowControlId,
    up: RowControlId,
    value: String,
) -> RowSpec {
    RowSpec {
        label: label.to_owned(),
        height: 44,
        control: CardRowControl::Stepper { down, up, value },
    }
}

fn row_choice(label: &'static str, action: RowControlId, value: String) -> RowSpec {
    RowSpec {
        label: label.to_owned(),
        height: 44,
        control: CardRowControl::Choice { action, value },
    }
}

fn row_info(label: &'static str, value: String) -> RowSpec {
    RowSpec {
        label: label.to_owned(),
        height: 44,
        control: CardRowControl::Info { value },
    }
}

fn row_info_string(label: String, value: String) -> RowSpec {
    RowSpec {
        label,
        height: 44,
        control: CardRowControl::Info { value },
    }
}

fn build_software_update_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();

    // Section 1: Update Status
    let mut status_rows = Vec::new();
    if app.updates_checking {
        status_rows.push(row_info("Status", "Checking for updates...".to_owned()));
    } else if let Some(phase) = app.updates_tx_phase {
        let phase_str = phase.label();
        status_rows.push(row_info(
            "Progress",
            format!("{} ({:.0}%)", phase_str, app.updates_tx_progress * 100.0),
        ));
        if let Some(summary) = &app.updates_tx_summary {
            status_rows.push(row_info_string("Summary".to_owned(), summary.clone()));
        }
    } else if app.updates_snapshot.total_count > 0 {
        status_rows.push(row_info_string(
            "Available Updates".to_owned(),
            format!("{} pending packages", app.updates_snapshot.total_count),
        ));
        status_rows.push(row_choice(
            "Install",
            RowControlId::UpdatesInstallNow,
            "Update Now".to_owned(),
        ));
        status_rows.push(row_choice(
            "Refresh",
            RowControlId::UpdatesCheckNow,
            "Check Again".to_owned(),
        ));
    } else {
        status_rows.push(row_info(
            "System Status",
            "RustyOS is up to date".to_owned(),
        ));
        status_rows.push(row_info_string(
            "Last Checked".to_owned(),
            app.updates_snapshot.last_checked_label.clone(),
        ));
        status_rows.push(row_choice(
            "Check",
            RowControlId::UpdatesCheckNow,
            "Check for Updates".to_owned(),
        ));
    }

    sections.push(place_card_section(
        Some("Software Update"),
        content_x,
        cur_y,
        content_w,
        status_rows,
    ));

    // Section 2: Pending Packages List
    if app.updates_snapshot.total_count > 0 && app.updates_tx_phase.is_none() {
        let mut pkg_rows = Vec::new();
        for item in &app.updates_snapshot.items {
            let label = if item.is_rustyos {
                format!("{} [RustyOS]", item.name)
            } else if item.requires_restart {
                format!("{} [Core]", item.name)
            } else {
                item.name.clone()
            };
            pkg_rows.push(row_info_string(
                label,
                format!("{} -> {}", item.old_version, item.new_version),
            ));
        }
        if !pkg_rows.is_empty() {
            sections.push(place_card_section(
                Some("Pending Packages"),
                content_x,
                cur_y,
                content_w,
                pkg_rows,
            ));
        }
    }

    // Section 3: Preferences
    let auto_rows = vec![row_switch(
        "Check Automatically",
        RowControlId::UpdatesEnabledToggle,
        app.updates_enabled,
    )];
    sections.push(place_card_section(
        Some("Preferences"),
        content_x,
        cur_y,
        content_w,
        auto_rows,
    ));

    sections
}

fn place_card_section(
    title: Option<&str>,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    specs: Vec<RowSpec>,
) -> CardSectionLayout {
    place_card_section_with_gap(title, content_x, cur_y, content_w, specs, 20)
}

fn place_card_section_with_gap(
    title: Option<&str>,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    specs: Vec<RowSpec>,
    bottom_gap: i32,
) -> CardSectionLayout {
    let section_label = title.map(|t| {
        let lbl_rect = Rect::new(content_x + 16, *cur_y, 260, 18);
        *cur_y += 22;
        (t.to_owned(), lbl_rect)
    });
    let card_h: i32 = specs.iter().map(|s| s.height).sum::<i32>().max(44);
    let card_rect = Rect::new(content_x, *cur_y, content_w, card_h);
    let mut rows = Vec::with_capacity(specs.len());
    let mut ry = card_rect.y;
    for spec in specs {
        let rh = spec.height;
        rows.push(CardRowLayout {
            rect: Rect::new(content_x, ry, content_w, rh),
            label: spec.label,
            control: spec.control,
        });
        ry += rh;
    }
    *cur_y = card_rect.bottom() + bottom_gap;
    CardSectionLayout {
        section_label,
        card_rect,
        rows,
    }
}

#[must_use]
pub(crate) fn switch_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 38, row.y + (row.h - 20) / 2, 38, 20)
}

#[must_use]
pub(crate) fn choice_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 124, row.y + (row.h - 26) / 2, 124, 26)
}

#[must_use]
pub(crate) fn stepper_rects(row: Rect) -> (Rect, Rect, Rect) {
    let plus = Rect::new(row.right() - 16 - 26, row.y + (row.h - 26) / 2, 26, 26);
    let val = Rect::new(plus.x - 62, plus.y, 58, 26);
    let minus = Rect::new(val.x - 30, plus.y, 26, 26);
    (minus, val, plus)
}

#[must_use]
pub(crate) fn dropdown_trigger_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 176, row.y + (row.h - 26) / 2, 176, 26)
}

#[must_use]
pub(crate) fn battery_tabs_inner_rect(row: Rect) -> Rect {
    let w = (row.w - 32).clamp(180, 320);
    Rect::new(row.x + (row.w - w) / 2, row.y + (row.h - 28) / 2, w, 28)
}

#[must_use]
pub(crate) fn bt_device_action_rects(row: Rect) -> (Rect, Rect) {
    let info_r = Rect::new(row.right() - 16 - 22, row.y + (row.h - 22) / 2, 22, 22);
    let action_r = Rect::new(info_r.x - 10 - 92, row.y + (row.h - 24) / 2, 92, 24);
    (action_r, info_r)
}

#[must_use]
pub(crate) fn about_more_info_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 104, row.y + (row.h - 28) / 2, 104, 28)
}

#[must_use]
pub(crate) fn shortcut_pill_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 144, row.y + (row.h - 26) / 2, 144, 26)
}

#[must_use]
pub(crate) fn hit_test(layout: &SettingsLayout, px: f64, py: f64) -> Option<SettingsHit> {
    if let Some(edge) = hit_test_resize_edge(layout.win_w, layout.win_h, px, py) {
        return Some(SettingsHit::ResizeEdge(edge));
    }
    if layout.traffic_close.contains_f64(px, py) {
        return Some(SettingsHit::TrafficClose);
    }
    if layout.traffic_min.contains_f64(px, py) {
        return Some(SettingsHit::TrafficMinimize);
    }
    if layout.traffic_max.contains_f64(px, py) {
        return Some(SettingsHit::TrafficMaximize);
    }
    if layout.search_rect.contains_f64(px, py) {
        return Some(SettingsHit::SearchField);
    }
    if layout.nav_back.contains_f64(px, py) {
        return Some(SettingsHit::NavBack);
    }
    if layout.nav_forward.contains_f64(px, py) {
        return Some(SettingsHit::NavForward);
    }
    if py >= f64::from(layout.sidebar_viewport.y) {
        for item in &layout.sidebar_items {
            if item.rect.contains_f64(px, py) {
                return Some(SettingsHit::SidebarItem(item.page));
            }
        }
    }
    if py < f64::from(HEADER_HEIGHT) {
        return Some(SettingsHit::WindowDrag);
    }
    if let Some(thumb) = layout.page_scrollbar_thumb {
        let expanded = Rect::new(thumb.x - 4, thumb.y, thumb.w + 8, thumb.h);
        if expanded.contains_f64(px, py) {
            return Some(SettingsHit::ScrollbarThumb);
        }
    }
    if let Some(app_layout) = &layout.appearance
        && let Some(hit) = hit_test_appearance(app_layout, px, py)
    {
        return Some(hit);
    }
    if let Some(dp) = &layout.dock_positions {
        for &(idx, _, ring, _, _, _) in &dp.tiles {
            if ring.contains_f64(px, py) {
                return Some(SettingsHit::DockPosTile(idx));
            }
        }
    }
    for sec in &layout.sections {
        for row in &sec.rows {
            if let Some(hit) = hit_test_row(row, px, py) {
                return Some(hit);
            }
        }
    }
    None
}

fn hit_test_appearance(app: &AppearanceCardsLayout, px: f64, py: f64) -> Option<SettingsHit> {
    for tile in &app.tiles {
        if tile.ring_rect.contains_f64(px, py) || tile.label_rect.contains_f64(px, py) {
            return Some(SettingsHit::SelectTheme(tile.choice));
        }
    }
    for (idx, (rect, _, _)) in app.swatches.iter().enumerate() {
        if rect.contains_f64(px, py) {
            return Some(SettingsHit::SelectAccent(idx));
        }
    }
    for row in &app.links {
        if row.rect.contains_f64(px, py)
            && let CardRowControl::Navigation { target, .. } = row.control
        {
            return Some(SettingsHit::PushPage(target));
        }
    }
    None
}

fn hit_test_row(row: &CardRowLayout, px: f64, py: f64) -> Option<SettingsHit> {
    match &row.control {
        CardRowControl::WifiNetwork { action, .. }
        | CardRowControl::NotificationAppRow { action, .. } => {
            if row.rect.contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::Switch { action, .. }
        | CardRowControl::WifiHeaderSwitch { action, .. }
        | CardRowControl::ServiceHeaderSwitch { action, .. }
        | CardRowControl::NotificationHeader { action, .. } => {
            if switch_rect(row.rect).contains_f64(px, py) || row.rect.contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::BluetoothDeviceRow {
            action,
            info_action,
            ..
        } => {
            let (act_r, info_r) = bt_device_action_rects(row.rect);
            if info_r.contains_f64(px, py) {
                return Some(SettingsHit::Control(*info_action));
            }
            if act_r.contains_f64(px, py) || row.rect.contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::DropdownTrigger { .. } => {
            if dropdown_trigger_rect(row.rect).contains_f64(px, py) {
                return Some(SettingsHit::LowPowerDropdownTrigger);
            }
        }
        CardRowControl::BatterySegmentedTabs => {
            let tabs_r = battery_tabs_inner_rect(row.rect);
            if tabs_r.contains_f64(px, py) {
                let idx = usize::from(px >= f64::from(tabs_r.x + tabs_r.w / 2));
                return Some(SettingsHit::BatteryTab(idx));
            }
        }
        CardRowControl::AboutHeroCard { action, .. } => {
            if about_more_info_rect(row.rect).contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::ShortcutBinding { action, .. } => {
            if shortcut_pill_rect(row.rect).contains_f64(px, py) || row.rect.contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::Choice { action, .. } => {
            if choice_rect(row.rect).contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::Stepper { down, up, .. } => {
            let (minus, _, plus) = stepper_rects(row.rect);
            if minus.contains_f64(px, py) {
                return Some(SettingsHit::StepperMinus(*down));
            }
            if plus.contains_f64(px, py) {
                return Some(SettingsHit::StepperPlus(*up));
            }
        }
        CardRowControl::Navigation { target, .. }
        | CardRowControl::ServiceStatus {
            target: Some(target),
            ..
        } => {
            if row.rect.contains_f64(px, py) {
                return Some(SettingsHit::PushPage(*target));
            }
        }
        CardRowControl::ServiceStatus { target: None, .. }
        | CardRowControl::Info { .. }
        | CardRowControl::UserProfileCard { .. } => {}
    }
    None
}

fn hit_test_resize_edge(win_w: i32, win_h: i32, px: f64, py: f64) -> Option<ResizeEdgeZone> {
    let b = f64::from(RESIZE_BORDER_PX);
    let w = f64::from(win_w);
    let h = f64::from(win_h);
    let left = px >= 0.0 && px < b;
    let right = px >= w - b && px < w;
    let top = py >= 0.0 && py < b;
    let bottom = py >= h - b && py < h;
    match (left, right, top, bottom) {
        (true, false, true, false) => Some(ResizeEdgeZone::TopLeft),
        (false, true, true, false) => Some(ResizeEdgeZone::TopRight),
        (true, false, false, true) => Some(ResizeEdgeZone::BottomLeft),
        (false, true, false, true) => Some(ResizeEdgeZone::BottomRight),
        (true, false, false, false) => Some(ResizeEdgeZone::Left),
        (false, true, false, false) => Some(ResizeEdgeZone::Right),
        (false, false, true, false) => Some(ResizeEdgeZone::Top),
        (false, false, false, true) => Some(ResizeEdgeZone::Bottom),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StartupOptions;

    #[test]
    fn default_appearance_layout_matches_882x600_metrics() {
        let mut app = SettingsAppState::new(&StartupOptions::default());
        let l = compute_layout(&app, 882, 600);
        assert_eq!(l.sidebar_rect.w, 230);
        assert_eq!(l.sidebar_items.len(), 20);
        assert!(l.max_page_scroll > 0);
        assert!(l.page_scrollbar_thumb.is_some());
        assert_eq!(hit_test(&l, 20.0, 20.0), Some(SettingsHit::TrafficClose));
        assert_eq!(hit_test(&l, 400.0, 20.0), Some(SettingsHit::WindowDrag));
        assert!(l.appearance.is_some());
        if let Some(app_cards) = &l.appearance {
            assert_eq!(app_cards.tiles.len(), 3);
            assert_eq!(app_cards.swatches.len(), 7);
            assert_eq!(app_cards.links.len(), 3);
            // Dark tile at x=403..511, y=90..166
            assert_eq!(
                hit_test(&l, 450.0, 120.0),
                Some(SettingsHit::SelectTheme(ThemeChoice::Dark))
            );
            // Auto tile at x=535..643
            assert_eq!(
                hit_test(&l, 580.0, 120.0),
                Some(SettingsHit::SelectTheme(ThemeChoice::Auto))
            );
            // Accent swatch #2 (Green: divider 231 + margin 24 + pad 14 + 2*52 + 19 = 392)
            assert_eq!(
                hit_test(&l, 231.0 + 24.0 + 14.0 + 2.0 * 52.0 + 19.0, 272.0),
                Some(SettingsHit::SelectAccent(2))
            );
            // Fonts navigation row
            assert_eq!(
                hit_test(&l, 400.0, 345.0),
                Some(SettingsHit::PushPage(SettingsPage::Fonts))
            );
        }
        assert!(app.scroll_by(false, 36, l.max_sidebar_scroll, l.max_page_scroll));
        let scrolled = compute_layout(&app, 882, 600);
        if let Some(app_cards) = &scrolled.appearance {
            assert_eq!(app_cards.mode_card_rect.y, 78 - 36);
        }
    }

    #[test]
    fn wifi_and_network_pages_expose_service_cards_and_ap_rows() {
        let mut app = SettingsAppState::new(&StartupOptions::default());
        let sample_net = rusty_services::NetworkStatus {
            connected: true,
            wired_connected: true,
            wired_interface: Some(String::from("enp3s0")),
            wired_label: Some(String::from("Wired")),
            wifi_connected: true,
            wifi_enabled: true,
            interface: Some(String::from("enp3s0")),
            wifi_ssid: Some(String::from("Lab5G")),
            wifi_signal_percent: Some(82),
            wifi_networks: vec![
                rusty_services::WifiAccessPoint {
                    ssid: String::from("Lab5G"),
                    signal_percent: 82,
                    secure: true,
                    connected: true,
                    known: true,
                },
                rusty_services::WifiAccessPoint {
                    ssid: String::from("Lobby"),
                    signal_percent: 45,
                    secure: false,
                    connected: false,
                    known: false,
                },
            ],
            status_label: String::from("Wi-Fi: Lab5G"),
        };

        app.current_page = SettingsPage::Network;
        app.network = sample_net.clone();
        let net_layout = compute_layout(&app, 882, 600);
        assert!(net_layout.sections.len() >= 4);
        let wifi_card_row = &net_layout.sections[0].rows[0];
        assert_eq!(
            hit_test(
                &net_layout,
                f64::from(wifi_card_row.rect.x + 80),
                f64::from(wifi_card_row.rect.y + 20)
            ),
            Some(SettingsHit::PushPage(SettingsPage::Wifi))
        );

        app.current_page = SettingsPage::Wifi;
        app.network = sample_net;
        let wifi_layout = compute_layout(&app, 882, 600);
        assert!(wifi_layout.sections.len() >= 3);
        let header_row = &wifi_layout.sections[0].rows[0];
        assert_eq!(
            hit_test(
                &wifi_layout,
                f64::from(header_row.rect.right() - 24),
                f64::from(header_row.rect.y + 24)
            ),
            Some(SettingsHit::Control(RowControlId::WifiEnabledToggle))
        );
        let ap0 = &wifi_layout.sections[1].rows[0];
        assert_eq!(
            hit_test(
                &wifi_layout,
                f64::from(ap0.rect.x + 60),
                f64::from(ap0.rect.y + 18)
            ),
            Some(SettingsHit::Control(RowControlId::WifiConnectIndex(0)))
        );
        let ap1 = &wifi_layout.sections[2].rows[0];
        assert_eq!(
            hit_test(
                &wifi_layout,
                f64::from(ap1.rect.x + 60),
                f64::from(ap1.rect.y + 18)
            ),
            Some(SettingsHit::Control(RowControlId::WifiConnectIndex(1)))
        );
    }

    #[test]
    fn notifications_page_exposes_header_policies_and_app_rows() {
        let mut app = SettingsAppState::new(&StartupOptions::default());
        app.current_page = SettingsPage::Notifications;
        let layout = compute_layout(&app, 882, 600);
        assert!(layout.sections.len() >= 4);

        let hero_row = &layout.sections[0].rows[0];
        assert_eq!(
            hit_test(
                &layout,
                f64::from(hero_row.rect.x + 80),
                f64::from(hero_row.rect.y + 24)
            ),
            Some(SettingsHit::Control(RowControlId::NotifAllowToggle))
        );

        let app0 = &layout.sections[2].rows[0];
        assert_eq!(
            hit_test(
                &layout,
                f64::from(app0.rect.x + 80),
                f64::from(app0.rect.y + 20)
            ),
            Some(SettingsHit::Control(RowControlId::NotifAppToggle(0)))
        );
        app.apply_row_action(RowControlId::NotifAppToggle(0));
        assert!(!app.notif_app_enabled[0]);
    }

    #[test]
    fn software_update_page_exposes_status_and_actions() {
        let app = SettingsAppState::new(&StartupOptions {
            initial_page: SettingsPage::SoftwareUpdate,
            theme_override: None,
            ..StartupOptions::default()
        });
        assert_eq!(app.current_page, SettingsPage::SoftwareUpdate);
        let layout = compute_layout(&app, 882, 600);
        assert!(layout.sections.len() >= 2);
        assert_eq!(
            layout.sections[0]
                .section_label
                .as_ref()
                .map(|(s, _)| s.as_str()),
            Some("Software Update")
        );
    }

    #[test]
    fn new_settings_pages_build_sections_and_hit_targets() {
        let mut app = SettingsAppState::new(&StartupOptions::default());

        app.current_page = SettingsPage::Bluetooth;
        app.bluetooth = rusty_services::BluetoothSnapshot {
            status: rusty_services::BluetoothStatus {
                available: true,
                powered: true,
                connected_device: Some(String::from("AirPods")),
                status_label: String::from("BT: AirPods"),
            },
            discovering: false,
            adapter_name: Some(String::from("rustyos-host")),
            adapter_address: None,
            devices: vec![rusty_services::BluetoothDevice {
                address: String::from("11:22:33:44:55:66"),
                name: String::from("AirPods"),
                icon: String::from("audio-headset"),
                paired: true,
                connected: true,
                trusted: true,
                battery_percent: Some(80),
            }],
        };
        let bluetooth_layout = compute_layout(&app, 882, 600);
        assert!(bluetooth_layout.sections.len() >= 3);
        let dev_row = &bluetooth_layout.sections[1].rows[0];
        let (act_r, info_r) = bt_device_action_rects(dev_row.rect);
        assert_eq!(
            hit_test(
                &bluetooth_layout,
                f64::from(act_r.x + 12),
                f64::from(act_r.y + 8)
            ),
            Some(SettingsHit::Control(RowControlId::BluetoothActionIndex(0)))
        );
        assert_eq!(
            hit_test(
                &bluetooth_layout,
                f64::from(info_r.x + 8),
                f64::from(info_r.y + 8)
            ),
            Some(SettingsHit::Control(RowControlId::BluetoothInfoIndex(0)))
        );

        app.current_page = SettingsPage::Battery;
        let power_layout = compute_layout(&app, 882, 600);
        assert!(power_layout.low_power_dropdown_rect.is_some());
        if let Some(dd) = power_layout.low_power_dropdown_rect {
            assert_eq!(
                hit_test(&power_layout, f64::from(dd.x + 10), f64::from(dd.y + 10)),
                Some(SettingsHit::LowPowerDropdownTrigger)
            );
        }

        for pg in [
            SettingsPage::About,
            SettingsPage::Users,
            SettingsPage::Shortcuts,
        ] {
            app.current_page = pg;
            let l = compute_layout(&app, 882, 600);
            assert_ne!(l.sections.len(), 0);
        }
    }
}
