//! System Settings sheet modal layout, tab models, hit-testing, and Liquid Glass rendering (`RUSTYOS-091`, `RUSTYOS-102`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{
    TOP_BAR_HEIGHT, ThemePalette, WINDOW_RADIUS, active_appearance, make_solid_element,
    push_rounded_glass_box, rect_contains, rgba_to_premul_color32f, truncate_ascii,
};
use crate::shell_render::top_bar::TopBarTelemetry;
use crate::surfaces::{CornerQuadrant, TrafficLightKind, UiGlyphKind};

use super::phys_to_logical_rect;

/// Active sidebar category inside the built-in System Settings modal (`RUSTYOS-061`/`062`/`073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    /// Frosted glass opacity, reduce transparency, reduced motion, and wallpaper.
    #[default]
    Appearance,
    /// Menu Bar indicators and clock format.
    MenuBar,
    /// `HiDPI` scale presets, output rotation, VRR policy, and max refresh rate.
    Displays,
    /// `XKB` keyboard layouts, switch shortcut, repeat rate, and repeat delay.
    Keyboard,
    /// Lock screen authentication, lock-on-sleep, and `PolicyKit` agent.
    Security,
    /// Dock size, screen edge position, minimize effect, auto-hide, indicators & recent apps.
    DesktopDock,
}

impl SettingsTab {
    /// Ordered sidebar tabs displayed in System Settings.
    pub const ALL: [Self; 6] = [
        Self::Appearance,
        Self::MenuBar,
        Self::Displays,
        Self::Keyboard,
        Self::Security,
        Self::DesktopDock,
    ];

    /// Human-readable sidebar & header title.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::MenuBar => "Menu Bar",
            Self::Displays => "Displays",
            Self::Keyboard => "Keyboard",
            Self::Security => "Security",
            Self::DesktopDock => "Desktop & Dock",
        }
    }

    const fn section_title(self) -> &'static str {
        match self {
            Self::Appearance => "Desktop & Glass",
            Self::MenuBar => "Menu Bar Controls",
            Self::Displays => "Display Configuration",
            Self::Keyboard => "Input & Repeat",
            Self::Security => "Privacy, Lock & Updates",
            Self::DesktopDock => "Dock Preferences",
        }
    }

    const fn category_header(self) -> Option<&'static str> {
        match self {
            Self::Appearance => Some("Desktop"),
            Self::Displays => Some("Devices"),
            Self::Security => Some("System"),
            Self::MenuBar | Self::Keyboard | Self::DesktopDock => None,
        }
    }

    const fn icon(self) -> UiGlyphKind {
        match self {
            Self::Appearance => UiGlyphKind::Transparency,
            Self::MenuBar => UiGlyphKind::ControlCenter,
            Self::Displays => UiGlyphKind::WindowMax,
            Self::Keyboard => UiGlyphKind::Keyboard,
            Self::Security => UiGlyphKind::Lock,
            Self::DesktopDock => UiGlyphKind::OverviewSpaces,
        }
    }

    const fn icon_color(self) -> [u8; 4] {
        // RustyOS category squircle palette (zero purple/lavender).
        match self {
            Self::Appearance => [3, 180, 203, 255],
            Self::MenuBar => [0, 135, 237, 255],
            Self::Displays => [0, 122, 255, 255],
            Self::Keyboard => [52, 120, 214, 255],
            Self::Security => [46, 178, 92, 255],
            Self::DesktopDock => [42, 162, 236, 255],
        }
    }

    /// Next category in wrap-around order.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Appearance => Self::MenuBar,
            Self::MenuBar => Self::Displays,
            Self::Displays => Self::Keyboard,
            Self::Keyboard => Self::Security,
            Self::Security => Self::DesktopDock,
            Self::DesktopDock => Self::Appearance,
        }
    }

    /// Previous category in wrap-around order.
    #[must_use]
    pub const fn prev(self) -> Self {
        match self {
            Self::Appearance => Self::DesktopDock,
            Self::MenuBar => Self::Appearance,
            Self::Displays => Self::MenuBar,
            Self::Keyboard => Self::Displays,
            Self::Security => Self::Keyboard,
            Self::DesktopDock => Self::Security,
        }
    }
}

/// Snapshot passed to the System Settings renderer (`RUSTYOS-061`/`062`).
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct SettingsRenderState<'a> {
    /// Active left-sidebar category tab.
    pub active_tab: SettingsTab,
    /// Shell glass opacity factor (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels toggle.
    pub reduce_transparency: bool,
    /// Reduced motion / instant animations toggle.
    pub reduced_motion: bool,
    /// Whether `[ui] wallpaper` currently uses a custom `.png` file instead of built-in Tahoe.
    pub has_custom_wallpaper: bool,
    /// Show Menu Bar workspace strip.
    pub menubar_show_workspaces: bool,
    /// Show Menu Bar `StatusNotifier` tray icons.
    pub menubar_show_tray: bool,
    /// Show Menu Bar input source indicator (`EN`/`RU`).
    pub menubar_show_input_source: bool,
    /// Show Menu Bar standalone volume glyph.
    pub menubar_show_volume: bool,
    /// Show Menu Bar Software Updates indicator when updates are pending (`RUSTYOS-076`).
    pub menubar_show_updates: bool,
    /// Use 24-hour Menu Bar clock format.
    pub clock_24h: bool,
    /// Active target display connector name (e.g. `"Winit-1"`, `"eDP-1"`, `"DP-1"`).
    pub target_connector: &'a str,
    /// Zero-based index of the selected display inside [`OutputMap::entries`].
    pub target_output_index: usize,
    /// Total number of currently connected displays (`>= 1`).
    pub output_count: usize,
    /// Physical pixel width of the selected display.
    pub display_width_px: i32,
    /// Physical pixel height of the selected display.
    pub display_height_px: i32,
    /// Rounded refresh rate of the selected display in Hertz (`60`, `144`, ...).
    pub display_refresh_hz: u32,
    /// Active `HiDPI` scale for the selected display.
    pub display_scale: f64,
    /// Active orientation for the selected display.
    pub display_rotation: crate::config::DisplayRotation,
    /// Global Adaptive Sync / VRR policy.
    pub vrr_mode: crate::config::VrrMode,
    /// Prefer max supported mode refresh rate on DRM/KMS.
    pub prefer_max_refresh: bool,
    /// Comma-separated `XKB` layout list (e.g. `"us,ru"`).
    pub keyboard_layout: &'a str,
    /// `XKB` options string (e.g. `"grp:alt_shift_toggle"`).
    pub keyboard_options: &'a str,
    /// Key repeat rate in keys/second (`1..=120`).
    pub repeat_rate: i32,
    /// Key repeat delay in milliseconds (`50..=2000`).
    pub repeat_delay: i32,
    /// Currently active short input layout badge (e.g. `"EN"`, `"RU"`).
    pub active_kbd_layout: &'a str,
    /// Require PAM password when unlocking the Lock Screen.
    pub require_password_on_lock: bool,
    /// Automatically lock session on logind sleep.
    pub lock_on_sleep: bool,
    /// Enable native `PolicyKit` authentication agent.
    pub enable_polkit_agent: bool,
    /// Enable periodic background Arch/RustyOS update checks (`RUSTYOS-076`).
    pub updates_enabled: bool,
    /// Dock icon base size in px (`36..=68`).
    pub dock_icon_size: i32,
    /// Dock edge placement (`Bottom`, `Left`, `Right`).
    pub dock_position: crate::config::DockPosition,
    /// Window minimize transition (`Genie` or `Scale`).
    pub dock_minimize_effect: crate::config::DockMinimizeEffect,
    /// Minimize windows directly into their owning app icon instead of trailing slots.
    pub dock_minimize_into_app: bool,
    /// Automatically hide and show the Dock when pointer is away from the screen edge.
    pub dock_auto_hide: bool,
    /// Animate opening applications with a launch bounce.
    pub dock_animate_opening: bool,
    /// Show open application indicator dots under running Dock items.
    pub dock_show_indicators: bool,
    /// Show suggested/recent unpinned applications in the Dock middle section.
    pub dock_show_recent: bool,
    /// Footer persistence badge (e.g. `"Saved"`, `"Reloaded"`, or `""`).
    pub config_status_label: &'a str,
}

/// Interactive click targets inside the System Settings modal (`RUSTYOS-061`/`062`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsHit {
    /// Switch active sidebar category.
    SelectTab(SettingsTab),
    /// Decrease frosted glass opacity.
    GlassOpacityDown,
    /// Increase frosted glass opacity.
    GlassOpacityUp,
    /// Toggle `reduce_transparency`.
    ToggleReduceTransparency,
    /// Toggle `reduced_motion`.
    ToggleReducedMotion,
    /// Toggle between custom `.png` wallpaper and built-in RustyOS gradient.
    ToggleWallpaper,
    /// Toggle between `Light` and `Dark` appearance (`RUSTYOS-067`).
    ToggleAppearance,
    /// Toggle `menubar_show_workspaces`.
    ToggleMenuWorkspaces,
    /// Toggle `menubar_show_tray`.
    ToggleMenuTray,
    /// Toggle `menubar_show_input_source`.
    ToggleMenuInputSource,
    /// Toggle `menubar_show_volume`.
    ToggleMenuVolume,
    /// Toggle `updates_show_in_menu_bar` (`RUSTYOS-076`).
    ToggleMenuUpdates,
    /// Toggle `clock_24h`.
    ToggleClock24h,
    /// Cycle which connected output is being configured in `Displays` (`RUSTYOS-063`).
    CycleTargetOutput,
    /// Step display `HiDPI` scale down.
    ScaleDown,
    /// Step display `HiDPI` scale up.
    ScaleUp,
    /// Cycle display rotation (`0° -> 90° -> 180° -> 270°`).
    CycleRotation,
    /// Cycle Adaptive Sync / VRR mode (`Off -> Fullscreen -> Always`).
    CycleVrrMode,
    /// Toggle `prefer_max_refresh`.
    TogglePreferMaxRefresh,
    /// Cycle curated `[keyboard] layout` presets (`us`, `us,ru`, `us,ua`, ...).
    CycleKeyboardLayout,
    /// Cycle curated `[keyboard] options` layout-switch shortcuts.
    CycleKeyboardOptions,
    /// Decrease key repeat rate (`keys/s`).
    RepeatRateDown,
    /// Increase key repeat rate (`keys/s`).
    RepeatRateUp,
    /// Decrease key repeat delay (`ms`).
    RepeatDelayDown,
    /// Increase key repeat delay (`ms`).
    RepeatDelayUp,
    /// Cycle active `XKB` keyboard layout group (`EN` <-> `RU`, etc.).
    CycleActiveInputSource,
    /// Toggle `require_password_on_lock`.
    ToggleRequirePassword,
    /// Toggle `lock_on_sleep`.
    ToggleLockOnSleep,
    /// Toggle `enable_polkit_agent`.
    TogglePolkitAgent,
    /// Toggle automatic background update checks (`[updates] enabled`) (`RUSTYOS-076`).
    ToggleUpdatesEnabled,
    /// Decrease `[dock].icon_size`.
    DockSizeDown,
    /// Increase `[dock].icon_size`.
    DockSizeUp,
    /// Cycle `[dock].position` (`Bottom` -> `Left` -> `Right`).
    DockCyclePosition,
    /// Toggle `[dock].minimize_effect` (`Genie` <-> `Scale`).
    DockToggleMinimizeEffect,
    /// Toggle `[dock].minimize_into_app`.
    DockToggleMinimizeIntoApp,
    /// Toggle `[dock].auto_hide`.
    DockToggleAutoHide,
    /// Toggle `[dock].animate_opening`.
    DockToggleAnimateOpening,
    /// Toggle `[dock].show_indicators`.
    DockToggleShowIndicators,
    /// Toggle `[dock].show_recent`.
    DockToggleShowRecent,
    /// Reload `~/.config/rustyos/config.toml` from disk.
    ReloadConfig,
    /// Close System Settings overlay (`×`, red traffic light, or `Done`).
    Close,
    /// Empty space inside the settings window (does not dismiss).
    ModalBody,
    /// Click outside the modal bounds (dismisses overlay).
    Outside,
}

#[derive(Debug, Clone)]
struct SettingsModalLayout {
    modal: Rectangle<i32, smithay::utils::Physical>,
    close_tl: Rectangle<i32, smithay::utils::Physical>,
    close_btn: Rectangle<i32, smithay::utils::Physical>,
    search_rect: Rectangle<i32, smithay::utils::Physical>,
    tabs: [Rectangle<i32, smithay::utils::Physical>; 6],
    divider_x: i32,
    content_x: i32,
    theme_card: Option<Rectangle<i32, smithay::utils::Physical>>,
    list_card: Rectangle<i32, smithay::utils::Physical>,
    rows: Vec<Rectangle<i32, smithay::utils::Physical>>,
    reload_btn: Rectangle<i32, smithay::utils::Physical>,
    done_btn: Rectangle<i32, smithay::utils::Physical>,
}

struct AppearanceCardGeometry {
    light_tile: Rectangle<i32, smithay::utils::Physical>,
    dark_tile: Rectangle<i32, smithay::utils::Physical>,
    swatches: [(Rectangle<i32, smithay::utils::Physical>, [u8; 4]); 6],
}

fn settings_modal_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    active_tab: SettingsTab,
) -> SettingsModalLayout {
    let w = 760.min((screen_size.w - 32).max(480));
    let h = 520.min((screen_size.h - TOP_BAR_HEIGHT - 24).max(380));
    let x = (screen_size.w - w) / 2;
    let y = ((screen_size.h - h) / 2).max(TOP_BAR_HEIGHT + 10);

    let sidebar_w = 220;
    let divider_x = x + sidebar_w;
    let content_x = divider_x + 24;
    let content_w = (w - sidebar_w - 48).max(200);

    let modal = Rectangle::new(Point::from((x, y)), Size::from((w, h)));
    let close_tl = Rectangle::new(Point::from((x + 12, y + 10)), Size::from((20, 20)));
    let close_btn = Rectangle::new(Point::from((x + w - 42, y + 10)), Size::from((26, 26)));
    let search_rect = Rectangle::new(
        Point::from((x + 12, y + 46)),
        Size::from((sidebar_w - 24, 28)),
    );

    let tab_offsets = [102, 138, 200, 236, 298, 334];
    let tabs = std::array::from_fn(|idx| {
        Rectangle::new(
            Point::from((x + 12, y + tab_offsets[idx])),
            Size::from((sidebar_w - 24, 34)),
        )
    });

    let theme_card = (active_tab == SettingsTab::Appearance).then(|| {
        Rectangle::new(
            Point::from((content_x, y + 62)),
            Size::from((content_w, 92)),
        )
    });

    let (list_y, row_h, count) = match active_tab {
        SettingsTab::Appearance => (y + 178, 46, 5),
        SettingsTab::DesktopDock => (y + 74, 46, 8),
        SettingsTab::MenuBar => (y + 74, 52, 6),
        SettingsTab::Security => (y + 74, 56, 4),
        SettingsTab::Displays | SettingsTab::Keyboard => (y + 74, 56, 5),
    };
    let rows = (0..count)
        .map(|idx| {
            Rectangle::new(
                Point::from((content_x, list_y + idx * row_h)),
                Size::from((content_w, row_h)),
            )
        })
        .collect();
    let list_card = Rectangle::new(
        Point::from((content_x, list_y)),
        Size::from((content_w, count * row_h)),
    );

    let done_btn = Rectangle::new(Point::from((x + w - 102, y + h - 42)), Size::from((78, 28)));
    let reload_btn = Rectangle::new(
        Point::from((done_btn.loc.x - 120, y + h - 42)),
        Size::from((110, 28)),
    );

    SettingsModalLayout {
        modal,
        close_tl,
        close_btn,
        search_rect,
        tabs,
        divider_x,
        content_x,
        theme_card,
        list_card,
        rows,
        reload_btn,
        done_btn,
    }
}

fn appearance_card_geometry(
    card: Rectangle<i32, smithay::utils::Physical>,
) -> AppearanceCardGeometry {
    let light_tile = Rectangle::new(
        Point::from((card.loc.x + 16, card.loc.y + 10)),
        Size::from((96, 52)),
    );
    let dark_tile = Rectangle::new(
        Point::from((card.loc.x + 124, card.loc.y + 10)),
        Size::from((96, 52)),
    );
    let colors: [[u8; 4]; 6] = [
        [0, 122, 255, 255],
        [48, 176, 199, 255],
        [52, 199, 89, 255],
        [255, 196, 0, 255],
        [255, 149, 0, 255],
        [255, 82, 87, 255],
    ];
    let swatch_x0 = card.loc.x + 258;
    let swatch_y = card.loc.y + 38;
    let swatches = std::array::from_fn(|idx| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let sx = swatch_x0 + (idx as i32) * 34;
        (
            Rectangle::new(Point::from((sx, swatch_y)), Size::from((22, 22))),
            colors[idx],
        )
    });
    AppearanceCardGeometry {
        light_tile,
        dark_tile,
        swatches,
    }
}

fn settings_row_pill_rect(
    row: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    let pill_h = 26;
    let offset_y = (row.size.h - pill_h) / 2;
    Rectangle::new(
        Point::from((row.loc.x + row.size.w - 126, row.loc.y + offset_y)),
        Size::from((110, pill_h)),
    )
}

fn switch_track_rect(
    row: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    let track_w = 40;
    let track_h = 22;
    let offset_y = (row.size.h - track_h) / 2;
    Rectangle::new(
        Point::from((row.loc.x + row.size.w - 16 - track_w, row.loc.y + offset_y)),
        Size::from((track_w, track_h)),
    )
}

fn settings_row_stepper_rects(
    row: Rectangle<i32, smithay::utils::Physical>,
) -> (
    Rectangle<i32, smithay::utils::Physical>,
    Rectangle<i32, smithay::utils::Physical>,
    Rectangle<i32, smithay::utils::Physical>,
) {
    let ctrl_h = 26;
    let offset_y = (row.size.h - ctrl_h) / 2;
    let plus = Rectangle::new(
        Point::from((row.loc.x + row.size.w - 44, row.loc.y + offset_y)),
        Size::from((28, ctrl_h)),
    );
    let value_box = Rectangle::new(
        Point::from((plus.loc.x - 62, row.loc.y + offset_y)),
        Size::from((56, ctrl_h)),
    );
    let minus = Rectangle::new(
        Point::from((value_box.loc.x - 34, row.loc.y + offset_y)),
        Size::from((28, ctrl_h)),
    );
    (minus, value_box, plus)
}

enum SettingsRowKind {
    Stepper {
        down: SettingsHit,
        up: SettingsHit,
        value: String,
    },
    Switch {
        hit: SettingsHit,
        active: bool,
    },
    Choice {
        hit: SettingsHit,
        value: std::borrow::Cow<'static, str>,
    },
}

struct SettingsRowEntry {
    label: String,
    kind: SettingsRowKind,
}

fn switch_row(label: impl Into<String>, hit: SettingsHit, active: bool) -> SettingsRowEntry {
    SettingsRowEntry {
        label: label.into(),
        kind: SettingsRowKind::Switch { hit, active },
    }
}

fn choice_row(
    label: impl Into<String>,
    hit: SettingsHit,
    value: std::borrow::Cow<'static, str>,
) -> SettingsRowEntry {
    SettingsRowEntry {
        label: label.into(),
        kind: SettingsRowKind::Choice { hit, value },
    }
}

fn settings_tab_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    match st.active_tab {
        SettingsTab::Appearance => settings_appearance_rows(st),
        SettingsTab::MenuBar => vec![
            switch_row(
                "Workspace Strip",
                SettingsHit::ToggleMenuWorkspaces,
                st.menubar_show_workspaces,
            ),
            switch_row(
                "Tray Applets",
                SettingsHit::ToggleMenuTray,
                st.menubar_show_tray,
            ),
            switch_row(
                "Input Source Badge",
                SettingsHit::ToggleMenuInputSource,
                st.menubar_show_input_source,
            ),
            switch_row(
                "Volume Indicator",
                SettingsHit::ToggleMenuVolume,
                st.menubar_show_volume,
            ),
            switch_row(
                "Updates Indicator",
                SettingsHit::ToggleMenuUpdates,
                st.menubar_show_updates,
            ),
            switch_row("24-Hour Clock", SettingsHit::ToggleClock24h, st.clock_24h),
        ],
        SettingsTab::Displays => settings_display_rows(st),
        SettingsTab::Keyboard => settings_keyboard_rows(st),
        SettingsTab::Security => vec![
            switch_row(
                "Require Password on Lock",
                SettingsHit::ToggleRequirePassword,
                st.require_password_on_lock,
            ),
            switch_row(
                "Lock Screen on Sleep",
                SettingsHit::ToggleLockOnSleep,
                st.lock_on_sleep,
            ),
            switch_row(
                "Built-in Polkit Agent",
                SettingsHit::TogglePolkitAgent,
                st.enable_polkit_agent,
            ),
            switch_row(
                "Auto Update Checks",
                SettingsHit::ToggleUpdatesEnabled,
                st.updates_enabled,
            ),
        ],
        SettingsTab::DesktopDock => settings_dock_rows(st),
    }
}

fn settings_dock_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    let icon_sz = crate::config::clamp_dock_icon_size(st.dock_icon_size);
    vec![
        SettingsRowEntry {
            label: "Dock Size".to_owned(),
            kind: SettingsRowKind::Stepper {
                down: SettingsHit::DockSizeDown,
                up: SettingsHit::DockSizeUp,
                value: format!("{icon_sz} px"),
            },
        },
        choice_row(
            "Position on screen",
            SettingsHit::DockCyclePosition,
            std::borrow::Cow::Borrowed(st.dock_position.label()),
        ),
        choice_row(
            "Minimize using",
            SettingsHit::DockToggleMinimizeEffect,
            std::borrow::Cow::Borrowed(st.dock_minimize_effect.label()),
        ),
        switch_row(
            "Minimize into app icon",
            SettingsHit::DockToggleMinimizeIntoApp,
            st.dock_minimize_into_app,
        ),
        switch_row(
            "Auto-hide the Dock",
            SettingsHit::DockToggleAutoHide,
            st.dock_auto_hide,
        ),
        switch_row(
            "Animate opening apps",
            SettingsHit::DockToggleAnimateOpening,
            st.dock_animate_opening,
        ),
        switch_row(
            "Show open indicators",
            SettingsHit::DockToggleShowIndicators,
            st.dock_show_indicators,
        ),
        switch_row(
            "Show recent apps",
            SettingsHit::DockToggleShowRecent,
            st.dock_show_recent,
        ),
    ]
}

fn settings_appearance_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pct = (crate::config::clamp_glass_opacity(st.glass_opacity) * 100.0).round() as u32;
    let mode = active_appearance();
    vec![
        SettingsRowEntry {
            label: "Glass Opacity".to_owned(),
            kind: SettingsRowKind::Stepper {
                down: SettingsHit::GlassOpacityDown,
                up: SettingsHit::GlassOpacityUp,
                value: format!("{pct}%"),
            },
        },
        switch_row(
            "Reduce Transparency",
            SettingsHit::ToggleReduceTransparency,
            st.reduce_transparency,
        ),
        switch_row(
            "Reduce Motion",
            SettingsHit::ToggleReducedMotion,
            st.reduced_motion,
        ),
        choice_row(
            "Desktop Wallpaper",
            SettingsHit::ToggleWallpaper,
            std::borrow::Cow::Borrowed(if st.has_custom_wallpaper {
                "Custom PNG"
            } else {
                "RustyOS Default"
            }),
        ),
        choice_row(
            "Appearance Theme",
            SettingsHit::ToggleAppearance,
            std::borrow::Cow::Borrowed(mode.label()),
        ),
    ]
}

fn settings_display_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    let conn = truncate_ascii(st.target_connector, 11);
    let output_label = if st.output_count > 1 {
        format!(
            "Output {}/{} · {}x{}@{}Hz",
            st.target_output_index + 1,
            st.output_count,
            st.display_width_px,
            st.display_height_px,
            st.display_refresh_hz,
        )
    } else {
        format!(
            "Output · {}x{}@{}Hz",
            st.display_width_px, st.display_height_px, st.display_refresh_hz,
        )
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scale_pct = (crate::config::clamp_display_scale(st.display_scale) * 100.0).round() as u32;
    let vrr_label = match st.vrr_mode {
        crate::config::VrrMode::Off => "Off",
        crate::config::VrrMode::Fullscreen => "Fullscreen",
        crate::config::VrrMode::Always => "Always",
    };
    let rot_label = match st.display_rotation {
        crate::config::DisplayRotation::Rotate0 => "0°",
        crate::config::DisplayRotation::Rotate90 => "90°",
        crate::config::DisplayRotation::Rotate180 => "180°",
        crate::config::DisplayRotation::Rotate270 => "270°",
    };
    vec![
        choice_row(
            output_label,
            SettingsHit::CycleTargetOutput,
            std::borrow::Cow::Owned(conn.clone()),
        ),
        SettingsRowEntry {
            label: format!("HiDPI Scale ({conn})"),
            kind: SettingsRowKind::Stepper {
                down: SettingsHit::ScaleDown,
                up: SettingsHit::ScaleUp,
                value: format!("{scale_pct}%"),
            },
        },
        choice_row(
            "Rotation",
            SettingsHit::CycleRotation,
            std::borrow::Cow::Borrowed(rot_label),
        ),
        choice_row(
            "Adaptive Sync (VRR)",
            SettingsHit::CycleVrrMode,
            std::borrow::Cow::Borrowed(vrr_label),
        ),
        switch_row(
            "Prefer Max Refresh",
            SettingsHit::TogglePreferMaxRefresh,
            st.prefer_max_refresh,
        ),
    ]
}

fn settings_keyboard_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    let layouts_value = truncate_ascii(st.keyboard_layout, 11);
    let options_label = crate::config::keyboard_option_label(st.keyboard_options);
    let rate = st.repeat_rate.clamp(1, 120);
    let delay = st.repeat_delay.clamp(50, 2000);
    vec![
        choice_row(
            "Input Layouts",
            SettingsHit::CycleKeyboardLayout,
            std::borrow::Cow::Owned(layouts_value),
        ),
        choice_row(
            "Switch Shortcut",
            SettingsHit::CycleKeyboardOptions,
            std::borrow::Cow::Borrowed(options_label),
        ),
        SettingsRowEntry {
            label: "Repeat Rate".to_owned(),
            kind: SettingsRowKind::Stepper {
                down: SettingsHit::RepeatRateDown,
                up: SettingsHit::RepeatRateUp,
                value: format!("{rate}/s"),
            },
        },
        SettingsRowEntry {
            label: "Repeat Delay".to_owned(),
            kind: SettingsRowKind::Stepper {
                down: SettingsHit::RepeatDelayDown,
                up: SettingsHit::RepeatDelayUp,
                value: format!("{delay}ms"),
            },
        },
        choice_row(
            "Active Source",
            SettingsHit::CycleActiveInputSource,
            std::borrow::Cow::Owned(truncate_ascii(st.active_kbd_layout, 4)),
        ),
    ]
}

/// Hit-tests pointer coordinates against the System Settings modal (`RUSTYOS-061`).
#[must_use]
pub fn hit_test_settings_modal(
    screen_size: Size<i32, smithay::utils::Physical>,
    state: &SettingsRenderState<'_>,
    x: f64,
    y: f64,
) -> SettingsHit {
    let layout = settings_modal_layout(screen_size, state.active_tab);
    if !rect_contains(layout.modal, x, y) {
        return SettingsHit::Outside;
    }
    if rect_contains(layout.close_tl, x, y)
        || rect_contains(layout.close_btn, x, y)
        || rect_contains(layout.done_btn, x, y)
    {
        return SettingsHit::Close;
    }
    if rect_contains(layout.reload_btn, x, y) {
        return SettingsHit::ReloadConfig;
    }
    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        if rect_contains(rect, x, y) {
            return SettingsHit::SelectTab(*tab);
        }
    }
    if let Some(card) = layout.theme_card
        && rect_contains(card, x, y)
        && x < f64::from(card.loc.x + 240)
    {
        return SettingsHit::ToggleAppearance;
    }
    for (entry, &row_rect) in settings_tab_rows(state).iter().zip(&layout.rows) {
        match &entry.kind {
            SettingsRowKind::Stepper { down, up, .. } => {
                let (minus, _, plus) = settings_row_stepper_rects(row_rect);
                if rect_contains(minus, x, y) {
                    return *down;
                }
                if rect_contains(plus, x, y) {
                    return *up;
                }
            }
            SettingsRowKind::Switch { hit, .. } | SettingsRowKind::Choice { hit, .. } => {
                if rect_contains(settings_row_pill_rect(row_rect), x, y) {
                    return *hit;
                }
            }
        }
    }
    SettingsHit::ModalBody
}

pub(crate) fn render_settings_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    st: &SettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
    let layout = settings_modal_layout(screen_size, st.active_tab);

    render_settings_sidebar_text(text_cache, renderer, &layout, pal, elements);

    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        st.active_tab.label(),
        crate::text::FontWeight::Bold,
        14.0,
        pal.popover_text_primary,
        Point::from((layout.content_x + 48, layout.modal.loc.y + 12)),
    ) {
        elements.push(el);
    }

    if let Some(theme_card) = layout.theme_card {
        render_appearance_card_text(text_cache, renderer, &layout, theme_card, pal, elements);
    } else if let Some(el) = text_cache.render_label_weighted(
        renderer,
        st.active_tab.section_title(),
        crate::text::FontWeight::Bold,
        12.0,
        pal.popover_text_secondary,
        Point::from((layout.content_x + 16, layout.list_card.loc.y - 20)),
    ) {
        elements.push(el);
    }

    render_settings_rows_text(text_cache, renderer, &layout, st, elements);
    render_settings_footer_text(text_cache, renderer, &layout, st, elements);
}

fn render_settings_sidebar_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    pal: ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(el) = text_cache.render_label(
        renderer,
        "Search",
        12.0,
        pal.popover_text_muted,
        Point::from((layout.search_rect.loc.x + 30, layout.search_rect.loc.y + 7)),
    ) {
        elements.push(el);
    }

    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        if let Some(cat) = tab.category_header()
            && let Some(el) = text_cache.render_label_weighted(
                renderer,
                cat,
                crate::text::FontWeight::Regular,
                11.0,
                pal.popover_text_muted,
                Point::from((rect.loc.x + 6, rect.loc.y - 18)),
            )
        {
            elements.push(el);
        }
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            tab.label(),
            crate::text::FontWeight::Regular,
            12.5,
            pal.popover_text_primary,
            Point::from((rect.loc.x + 38, rect.loc.y + 9)),
        ) {
            elements.push(el);
        }
    }
}

fn render_appearance_card_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    theme_card: Rectangle<i32, smithay::utils::Physical>,
    pal: ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "Appearance",
        crate::text::FontWeight::Bold,
        12.0,
        pal.popover_text_secondary,
        Point::from((layout.content_x + 16, theme_card.loc.y - 20)),
    ) {
        elements.push(el);
    }

    let geom = appearance_card_geometry(theme_card);
    for (tile, label, selected) in [
        (geom.light_tile, "Light", !pal.is_dark),
        (geom.dark_tile, "Dark", pal.is_dark),
    ] {
        let caption_bounds = Rectangle::new(
            Point::from((tile.loc.x, tile.loc.y + tile.size.h + 5)),
            Size::from((tile.size.w, 18)),
        );
        let fg = if selected {
            pal.accent_rgba
        } else {
            pal.popover_text_primary
        };
        let weight = if selected {
            crate::text::FontWeight::Bold
        } else {
            crate::text::FontWeight::Regular
        };
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            label,
            weight,
            11.5,
            fg,
            phys_to_logical_rect(caption_bounds),
        ) {
            elements.push(el);
        }
    }

    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "Accent Color",
        crate::text::FontWeight::Regular,
        11.5,
        pal.popover_text_secondary,
        Point::from((theme_card.loc.x + 258, theme_card.loc.y + 14)),
    ) {
        elements.push(el);
    }

    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "Desktop & Glass",
        crate::text::FontWeight::Bold,
        12.0,
        pal.popover_text_secondary,
        Point::from((layout.content_x + 16, layout.list_card.loc.y - 20)),
    ) {
        elements.push(el);
    }
}

fn render_settings_rows_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    st: &SettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
    for (entry, &row) in settings_tab_rows(st).iter().zip(&layout.rows) {
        let label_y = row.loc.y + (row.size.h - 16) / 2;
        if let Some(el) = text_cache.render_label(
            renderer,
            &entry.label,
            12.5,
            pal.popover_text_primary,
            Point::from((row.loc.x + 16, label_y)),
        ) {
            elements.push(el);
        }
        match &entry.kind {
            SettingsRowKind::Stepper { value, .. } => {
                let (minus, value_box, plus) = settings_row_stepper_rects(row);
                for (rect, label) in [(minus, "-"), (value_box, value.as_str()), (plus, "+")] {
                    if let Some(el) = text_cache.render_label_centered_weighted(
                        renderer,
                        label,
                        crate::text::FontWeight::Bold,
                        12.0,
                        pal.popover_text_primary,
                        phys_to_logical_rect(rect),
                    ) {
                        elements.push(el);
                    }
                }
            }
            SettingsRowKind::Choice { value, .. } => {
                let pill = settings_row_pill_rect(row);
                if let Some(el) = text_cache.render_label_centered_weighted(
                    renderer,
                    value,
                    crate::text::FontWeight::Regular,
                    11.5,
                    pal.popover_text_primary,
                    phys_to_logical_rect(pill),
                ) {
                    elements.push(el);
                }
            }
            SettingsRowKind::Switch { .. } => {}
        }
    }
}

fn render_settings_footer_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    st: &SettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
    let status = if st.config_status_label.is_empty() {
        "~/.config/rustyos/config.toml".to_owned()
    } else {
        truncate_ascii(&format!("config.toml · {}", st.config_status_label), 38)
    };
    if let Some(el) = text_cache.render_label(
        renderer,
        &status,
        11.0,
        pal.popover_text_muted,
        Point::from((
            layout.content_x + 4,
            layout.modal.loc.y + layout.modal.size.h - 35,
        )),
    ) {
        elements.push(el);
    }

    for (rect, label, weight, fg) in [
        (
            layout.reload_btn,
            "Reload TOML",
            crate::text::FontWeight::Regular,
            pal.popover_text_primary,
        ),
        (
            layout.done_btn,
            "Done",
            crate::text::FontWeight::Bold,
            [255, 255, 255, 255],
        ),
    ] {
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            label,
            weight,
            11.5,
            fg,
            phys_to_logical_rect(rect),
        ) {
            elements.push(el);
        }
    }
}

pub(crate) fn render_settings_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    _top_telemetry: &TopBarTelemetry<'_>,
    st: &SettingsRenderState<'_>,
) {
    let pal = ThemePalette::active();
    let layout = settings_modal_layout(screen_size, st.active_tab);

    let card_fill = if pal.is_dark {
        [44, 44, 45, 255]
    } else {
        [255, 255, 255, 255]
    };
    let card_border = if pal.is_dark {
        [255, 255, 255, 22]
    } else {
        [0, 0, 0, 18]
    };
    let page_canvas_fill = if pal.is_dark {
        [28, 28, 29, 255]
    } else {
        [245, 246, 250, 255]
    };

    // 1. Frontmost chrome icons and sidebar white glyphs.
    render_settings_chrome_icons(out, shape_cache, renderer, &layout, pal);
    render_settings_sidebar_composite(out, shape_cache, renderer, &layout, pal, st);

    // 2. Appearance visual theme preview + accent color swatches card.
    if let Some(theme_card) = layout.theme_card {
        render_appearance_card_composite(out, shape_cache, renderer, theme_card, pal, card_fill);
    }

    // 3. Row controls, 1px inset separators, and grouped Card::Style::List container.
    render_settings_controls_composite(
        out,
        shape_cache,
        renderer,
        &layout,
        st,
        pal,
        card_fill,
        card_border,
    );

    // 4-7. Top-right close pill, footer buttons, vertical divider, opaque right canvas, and flat sidebar frame.
    render_settings_panel_chrome(
        out,
        shape_cache,
        renderer,
        &layout,
        pal,
        card_fill,
        card_border,
        page_canvas_fill,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_settings_panel_chrome(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    pal: ThemePalette,
    card_fill: [u8; 4],
    card_border: [u8; 4],
    page_canvas_fill: [u8; 4],
) {
    let modal = layout.modal;

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.close_btn,
        13,
        pal.control_pill_off_rgba,
        Some(pal.border_rgba),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.reload_btn,
        8,
        card_fill,
        Some(card_border),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.done_btn,
        8,
        pal.accent_hover_fill,
        Some(pal.accent_hover_border),
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(
            Point::from((layout.divider_x, modal.loc.y + 1)),
            Size::from((1, (modal.size.h - 2).max(20))),
        ),
        pal.popover_separator,
    )));

    // Inset 1px on top/right/bottom so the outer rim frames all outer edges uniformly.
    let canvas_rect = Rectangle::new(
        Point::from((layout.divider_x + 1, modal.loc.y + 1)),
        Size::from((
            (modal.loc.x + modal.size.w - 1 - (layout.divider_x + 1)).max(16),
            (modal.size.h - 2).max(16),
        )),
    );
    push_right_rounded_canvas(
        out,
        shape_cache,
        renderer,
        canvas_rect,
        (WINDOW_RADIUS - 1).max(2),
        page_canvas_fill,
    );

    // Flat cool-grey sidebar background (no Liquid Glass blur/refraction).
    let sidebar_fill = if pal.is_dark {
        [38, 38, 40, 255]
    } else {
        [236, 237, 241, 255]
    };
    let modal_border = if pal.is_dark {
        [255, 255, 255, 28]
    } else {
        [0, 0, 0, 24]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        modal,
        WINDOW_RADIUS,
        sidebar_fill,
        Some(modal_border),
    );
}

fn render_settings_chrome_icons(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    pal: ThemePalette,
) {
    let modal = layout.modal;
    for (idx, kind) in [
        TrafficLightKind::Close,
        TrafficLightKind::Minimize,
        TrafficLightKind::Maximize,
    ]
    .into_iter()
    .enumerate()
    {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let tl_x = modal.loc.x + 16 + (idx as i32) * 20;
        if let Some(el) = shape_cache.traffic_light_element(
            renderer,
            kind,
            12,
            true,
            false,
            Point::from((tl_x, modal.loc.y + 14)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    for (kind, size_px, tint, loc) in [
        (
            UiGlyphKind::Search,
            13,
            pal.popover_text_muted,
            Point::from((layout.search_rect.loc.x + 10, layout.search_rect.loc.y + 7)),
        ),
        (
            UiGlyphKind::ArrowPrev,
            13,
            pal.popover_text_secondary,
            Point::from((layout.content_x, modal.loc.y + 13)),
        ),
        (
            UiGlyphKind::ChevronRight,
            13,
            pal.popover_text_disabled,
            Point::from((layout.content_x + 22, modal.loc.y + 13)),
        ),
        (
            UiGlyphKind::Close,
            12,
            pal.popover_icon,
            Point::from((layout.close_btn.loc.x + 7, layout.close_btn.loc.y + 7)),
        ),
    ] {
        if let Some(el) = shape_cache.ui_glyph_element(renderer, kind, size_px, tint, loc) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
}

fn render_settings_sidebar_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    pal: ThemePalette,
    st: &SettingsRenderState<'_>,
) {
    // Glyphs first (frontmost), then squircle badges, then active tab highlight and search box.
    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            tab.icon(),
            16,
            [255, 255, 255, 255],
            Point::from((rect.loc.x + 10, rect.loc.y + 9)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        let icon_box = Rectangle::new(
            Point::from((rect.loc.x + 6, rect.loc.y + 5)),
            Size::from((24, 24)),
        );
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            icon_box,
            6,
            tab.icon_color(),
            None,
        );

        if *tab == st.active_tab {
            let active_fill = if pal.is_dark {
                [58, 58, 61, 255]
            } else {
                [217, 218, 221, 255]
            };
            push_rounded_glass_box(out, shape_cache, renderer, rect, 10, active_fill, None);
        }
    }

    let search_fill = if pal.is_dark {
        [58, 58, 60, 255]
    } else {
        [255, 255, 255, 255]
    };
    let search_radius = (layout.search_rect.size.h / 2).max(8);
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.search_rect,
        search_radius,
        search_fill,
        None,
    );
}

fn render_appearance_card_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    theme_card: Rectangle<i32, smithay::utils::Physical>,
    pal: ThemePalette,
    card_fill: [u8; 4],
) {
    let geom = appearance_card_geometry(theme_card);

    // Active blue accent swatch carries white check and soft outer ring.
    if let Some((swatch_rect, _)) = geom.swatches.first()
        && let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Check,
            12,
            [255, 255, 255, 255],
            Point::from((swatch_rect.loc.x + 5, swatch_rect.loc.y + 5)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
    for (idx, (swatch_rect, rgba)) in geom.swatches.iter().enumerate() {
        if let Some(el) = shape_cache.circle_element(
            renderer,
            22,
            *rgba,
            Some([0, 0, 0, 24]),
            Point::from((swatch_rect.loc.x, swatch_rect.loc.y)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if idx == 0
            && let Some(el) = shape_cache.circle_element(
                renderer,
                28,
                [0, 122, 255, 86],
                None,
                Point::from((swatch_rect.loc.x - 3, swatch_rect.loc.y - 3)),
            )
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    render_theme_preview_tile(
        out,
        shape_cache,
        renderer,
        geom.light_tile,
        false,
        !pal.is_dark,
        pal,
        card_fill,
    );
    render_theme_preview_tile(
        out,
        shape_cache,
        renderer,
        geom.dark_tile,
        true,
        pal.is_dark,
        pal,
        card_fill,
    );

    push_rounded_glass_box(out, shape_cache, renderer, theme_card, 12, card_fill, None);
}

#[allow(clippy::too_many_arguments)]
fn render_theme_preview_tile(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    tile: Rectangle<i32, smithay::utils::Physical>,
    preview_dark: bool,
    selected: bool,
    pal: ThemePalette,
    card_fill: [u8; 4],
) {
    // Front-to-back: tiny window dots -> inner window pane -> outer desktop sky strip -> ring.
    let dot_rgba = if preview_dark {
        [170, 175, 185, 255]
    } else {
        [96, 102, 114, 255]
    };
    for dot_idx in 0..3 {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((tile.loc.x + tile.size.w - 36 + dot_idx * 7, tile.loc.y + 18)),
                Size::from((3, 3)),
            ),
            rgba_to_premul_color32f(dot_rgba),
        )));
    }

    let inner_win = Rectangle::new(
        Point::from((tile.loc.x + 2, tile.loc.y + 12)),
        Size::from((tile.size.w - 14, tile.size.h - 14)),
    );
    let inner_fill = if preview_dark {
        [24, 25, 28, 255]
    } else {
        [250, 251, 253, 255]
    };
    push_rounded_glass_box(out, shape_cache, renderer, inner_win, 5, inner_fill, None);

    let bg_fill = if preview_dark {
        [46, 62, 74, 255]
    } else {
        [152, 182, 200, 255]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        tile,
        7,
        bg_fill,
        Some([0, 0, 0, 22]),
    );

    if selected {
        let gap_rect = Rectangle::new(
            Point::from((tile.loc.x - 1, tile.loc.y - 1)),
            Size::from((tile.size.w + 2, tile.size.h + 2)),
        );
        push_rounded_glass_box(out, shape_cache, renderer, gap_rect, 8, card_fill, None);

        let ring_rect = Rectangle::new(
            Point::from((tile.loc.x - 3, tile.loc.y - 3)),
            Size::from((tile.size.w + 6, tile.size.h + 6)),
        );
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            ring_rect,
            10,
            pal.accent_rgba,
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_settings_controls_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    st: &SettingsRenderState<'_>,
    pal: ThemePalette,
    card_fill: [u8; 4],
    card_border: [u8; 4],
) {
    let stepper_btn_fill = if pal.is_dark {
        [62, 62, 70, 255]
    } else {
        [232, 235, 242, 255]
    };
    let stepper_val_fill = if pal.is_dark {
        [52, 52, 58, 255]
    } else {
        [244, 246, 250, 255]
    };
    let choice_fill = if pal.is_dark {
        [62, 62, 68, 255]
    } else {
        [236, 238, 244, 255]
    };
    let switch_off_fill = if pal.is_dark {
        [72, 72, 78, 255]
    } else {
        [209, 213, 222, 255]
    };

    let rows_data = settings_tab_rows(st);
    for (entry, &row) in rows_data.iter().zip(&layout.rows) {
        match &entry.kind {
            SettingsRowKind::Stepper { .. } => {
                let (minus, value_box, plus) = settings_row_stepper_rects(row);
                for (btn, fill) in [
                    (minus, stepper_btn_fill),
                    (value_box, stepper_val_fill),
                    (plus, stepper_btn_fill),
                ] {
                    push_rounded_glass_box(
                        out,
                        shape_cache,
                        renderer,
                        btn,
                        8,
                        fill,
                        Some(card_border),
                    );
                }
            }
            SettingsRowKind::Switch { active, .. } => {
                let track = switch_track_rect(row);
                let thumb_x = if *active {
                    track.loc.x + 20
                } else {
                    track.loc.x + 2
                };
                if let Some(el) = shape_cache.circle_element(
                    renderer,
                    18,
                    [255, 255, 255, 255],
                    Some([0, 0, 0, 24]),
                    Point::from((thumb_x, track.loc.y + 2)),
                ) {
                    out.push(CompositorRenderElement::Memory(el));
                }
                let (fill, border) = if *active {
                    (pal.accent_rgba, None)
                } else {
                    (switch_off_fill, Some(card_border))
                };
                push_rounded_glass_box(out, shape_cache, renderer, track, 11, fill, border);
            }
            SettingsRowKind::Choice { .. } => {
                let pill = settings_row_pill_rect(row);
                push_rounded_glass_box(
                    out,
                    shape_cache,
                    renderer,
                    pill,
                    8,
                    choice_fill,
                    Some(card_border),
                );
            }
        }
    }

    // 1px inset row separators inside the Card (pushed before list_card so they appear on top).
    let sep_color = rgba_to_premul_color32f(card_border);
    let last_idx = layout.rows.len().saturating_sub(1);
    for &row in layout.rows.iter().take(last_idx) {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((layout.list_card.loc.x + 16, row.loc.y + row.size.h - 1)),
                Size::from(((layout.list_card.size.w - 32).max(16), 1)),
            ),
            sep_color,
        )));
    }

    // Single grouped borderless Card::Style::List background wrapping all rows.
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.list_card,
        12,
        card_fill,
        None,
    );
}

#[allow(clippy::many_single_char_names)]
fn push_right_rounded_canvas(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, smithay::utils::Physical>,
    radius: i32,
    fill_rgba: [u8; 4],
) {
    let x = rect.loc.x;
    let y = rect.loc.y;
    let w = rect.size.w;
    let h = rect.size.h;
    let r = radius.min(w / 2).min(h / 2).max(0);
    let fill_col = rgba_to_premul_color32f(fill_rgba);

    if r <= 1 {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            rect, fill_col,
        )));
        return;
    }

    // Rounded TopRight & BottomRight corners; left edge meets divider straight.
    for (quad, loc) in [
        (CornerQuadrant::TopRight, Point::from((x + w - r, y))),
        (
            CornerQuadrant::BottomRight,
            Point::from((x + w - r, y + h - r)),
        ),
    ] {
        if let Some(el) = shape_cache.corner_element(renderer, r, quad, fill_rgba, None, loc) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    // Top band, bottom band, and full-width middle band without overdraw.
    if w > r {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x, y)), Size::from((w - r, r))),
            fill_col,
        )));
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x, y + h - r)), Size::from((w - r, r))),
            fill_col,
        )));
    }
    if h > 2 * r {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x, y + r)), Size::from((w, h - 2 * r))),
            fill_col,
        )));
    }
}
