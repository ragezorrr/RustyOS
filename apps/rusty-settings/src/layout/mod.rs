//! Geometry layout and pointer hit-testing for `rusty-settings` (`RUSTYOS-103`).

mod hit_test;
mod pages;
#[cfg(test)]
mod tests;

pub(crate) use hit_test::{
    about_more_info_rect, battery_tabs_inner_rect, bt_device_action_rects, choice_rect,
    dropdown_trigger_rect, hit_test, shortcut_pill_rect, stepper_rects, switch_rect,
};
use pages::{
    RowSpec, build_standard_page_sections, place_card_section, row_choice, row_stepper, row_switch,
};

use crate::state::{RowControlId, SettingsAppState, SettingsPage, ThemeChoice};

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
        info_action: RowControlId,
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

    let tl = rusty_ui::TrafficLightButtons::default_csd();
    let btn_d = i32::try_from(tl.close_rect.width).unwrap_or(14);
    let traffic_close = Rect::new(tl.close_rect.x, tl.close_rect.y, btn_d, btn_d);
    let traffic_min = Rect::new(tl.minimize_rect.x, tl.minimize_rect.y, btn_d, btn_d);
    let traffic_max = Rect::new(tl.maximize_rect.x, tl.maximize_rect.y, btn_d, btn_d);
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
    let sec = place_card_section(None, content_x, &mut cur_y, content_w, build_dock_rows(app));
    (dock_positions, vec![sec])
}

#[must_use]
#[allow(clippy::cast_possible_truncation)]
fn build_dock_rows(app: &SettingsAppState) -> Vec<RowSpec> {
    vec![
        row_stepper(
            "Dock Size",
            RowControlId::DockSizeDown,
            RowControlId::DockSizeUp,
            format!("{} px", app.dock_icon_size),
        ),
        row_switch(
            "Magnification",
            RowControlId::DockMagnificationToggle,
            app.dock_magnification_enabled,
        ),
        row_stepper(
            "Peak Magnification",
            RowControlId::DockMagnificationDown,
            RowControlId::DockMagnificationUp,
            format!("{:.1}x", app.dock_magnification_max),
        ),
        row_stepper(
            "Dock Animation Speed",
            RowControlId::DockAnimationSpeedDown,
            RowControlId::DockAnimationSpeedUp,
            format!("{:.2}x", app.dock_animation_speed),
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
        row_choice(
            "Dock Visibility",
            RowControlId::DockVisibilityCycle,
            crate::state::dock_visibility_label(&app.dock_visibility).to_owned(),
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
        row_switch(
            "Show icon labels on hover",
            RowControlId::DockTooltipsToggle,
            app.dock_show_tooltips,
        ),
    ]
}
