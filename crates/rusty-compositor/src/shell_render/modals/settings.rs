//! System Settings sheet modal layout, tab models, hit-testing, and Liquid Glass rendering (`RUSTYOS-091`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{
    TOP_BAR_HEIGHT, ThemePalette, WINDOW_RADIUS, active_appearance, make_solid_element,
    push_liquid_glass_panel, push_rounded_glass_box, rect_contains, scale_glass_rgba,
    truncate_ascii,
};
use crate::shell_render::top_bar::TopBarTelemetry;

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

    const fn icon(self) -> crate::surfaces::UiGlyphKind {
        match self {
            Self::Appearance => crate::surfaces::UiGlyphKind::Transparency,
            Self::MenuBar => crate::surfaces::UiGlyphKind::ControlCenter,
            Self::Displays => crate::surfaces::UiGlyphKind::WindowMax,
            Self::Keyboard => crate::surfaces::UiGlyphKind::Keyboard,
            Self::Security => crate::surfaces::UiGlyphKind::Lock,
            Self::DesktopDock => crate::surfaces::UiGlyphKind::OverviewSpaces,
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
    /// Toggle between custom `.png` wallpaper and built-in Golden Gate gradient.
    ToggleWallpaper,
    /// Toggle between Golden Gate `Light` and `Dark` appearance (`RUSTYOS-067`).
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
    /// Close System Settings overlay (`×` or `Done`).
    Close,
    /// Empty space inside the settings window (does not dismiss).
    ModalBody,
    /// Click outside the modal bounds (dismisses overlay).
    Outside,
}

#[derive(Debug, Clone)]
struct SettingsModalLayout {
    modal: Rectangle<i32, smithay::utils::Physical>,
    badge: Rectangle<i32, smithay::utils::Physical>,
    close_btn: Rectangle<i32, smithay::utils::Physical>,
    tabs: [Rectangle<i32, smithay::utils::Physical>; 6],
    divider_x: i32,
    content_x: i32,
    rows: Vec<Rectangle<i32, smithay::utils::Physical>>,
    reload_btn: Rectangle<i32, smithay::utils::Physical>,
    done_btn: Rectangle<i32, smithay::utils::Physical>,
}

fn settings_modal_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    active_tab: SettingsTab,
) -> SettingsModalLayout {
    let w = 580.min((screen_size.w - 32).max(380));
    let h = 432.min((screen_size.h - TOP_BAR_HEIGHT - 24).max(320));
    let x = (screen_size.w - w) / 2;
    let y = ((screen_size.h - h) / 2).max(TOP_BAR_HEIGHT + 10);

    let modal = Rectangle::new(Point::from((x, y)), Size::from((w, h)));
    let badge = Rectangle::new(Point::from((x + 20, y + 16)), Size::from((34, 34)));
    let close_btn = Rectangle::new(Point::from((x + w - 44, y + 20)), Size::from((26, 26)));

    let tabs = std::array::from_fn(|idx| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let ry = y + 68 + (idx as i32) * 40;
        Rectangle::new(Point::from((x + 18, ry)), Size::from((152, 34)))
    });

    let divider_x = x + 184;
    let content_x = x + 198;
    let content_w = (w - 218).max(160);
    let rows = if active_tab == SettingsTab::DesktopDock {
        (0_i32..8)
            .map(|idx| {
                let ry = y + 90 + idx * 31;
                Rectangle::new(Point::from((content_x, ry)), Size::from((content_w, 27)))
            })
            .collect()
    } else if active_tab == SettingsTab::MenuBar {
        (0_i32..6)
            .map(|idx| {
                let ry = y + 94 + idx * 45;
                Rectangle::new(Point::from((content_x, ry)), Size::from((content_w, 38)))
            })
            .collect()
    } else {
        (0_i32..5)
            .map(|idx| {
                let ry = y + 98 + idx * 52;
                Rectangle::new(Point::from((content_x, ry)), Size::from((content_w, 44)))
            })
            .collect()
    };

    let done_btn = Rectangle::new(Point::from((x + w - 98, y + h - 50)), Size::from((78, 32)));
    let reload_btn = Rectangle::new(
        Point::from((done_btn.loc.x - 122, y + h - 50)),
        Size::from((112, 32)),
    );

    SettingsModalLayout {
        modal,
        badge,
        close_btn,
        tabs,
        divider_x,
        content_x,
        rows,
        reload_btn,
        done_btn,
    }
}

fn settings_row_pill_rect(
    row: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    let pill_h = if row.size.h < 34 { 21 } else { 28 };
    let offset_y = (row.size.h - pill_h) / 2;
    Rectangle::new(
        Point::from((row.loc.x + row.size.w - 120, row.loc.y + offset_y)),
        Size::from((110, pill_h)),
    )
}

fn settings_row_stepper_rects(
    row: Rectangle<i32, smithay::utils::Physical>,
) -> (
    Rectangle<i32, smithay::utils::Physical>,
    Rectangle<i32, smithay::utils::Physical>,
    Rectangle<i32, smithay::utils::Physical>,
) {
    let ctrl_h = if row.size.h < 34 { 21 } else { 28 };
    let offset_y = (row.size.h - ctrl_h) / 2;
    let plus = Rectangle::new(
        Point::from((row.loc.x + row.size.w - 38, row.loc.y + offset_y)),
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
    Toggle {
        hit: SettingsHit,
        value: std::borrow::Cow<'static, str>,
        active: bool,
    },
}

struct SettingsRowEntry {
    label: String,
    kind: SettingsRowKind,
}

fn toggle_row(label: impl Into<String>, hit: SettingsHit, active: bool) -> SettingsRowEntry {
    SettingsRowEntry {
        label: label.into(),
        kind: SettingsRowKind::Toggle {
            hit,
            value: std::borrow::Cow::Borrowed(if active { "On" } else { "Off" }),
            active,
        },
    }
}

fn settings_tab_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    match st.active_tab {
        SettingsTab::Appearance => settings_appearance_rows(st),
        SettingsTab::MenuBar => vec![
            toggle_row(
                "Workspace Strip",
                SettingsHit::ToggleMenuWorkspaces,
                st.menubar_show_workspaces,
            ),
            toggle_row(
                "Tray Applets",
                SettingsHit::ToggleMenuTray,
                st.menubar_show_tray,
            ),
            toggle_row(
                "Input Source Badge",
                SettingsHit::ToggleMenuInputSource,
                st.menubar_show_input_source,
            ),
            toggle_row(
                "Volume Indicator",
                SettingsHit::ToggleMenuVolume,
                st.menubar_show_volume,
            ),
            toggle_row(
                "Updates Indicator",
                SettingsHit::ToggleMenuUpdates,
                st.menubar_show_updates,
            ),
            toggle_row("24-Hour Clock", SettingsHit::ToggleClock24h, st.clock_24h),
        ],
        SettingsTab::Displays => settings_display_rows(st),
        SettingsTab::Keyboard => settings_keyboard_rows(st),
        SettingsTab::Security => vec![
            toggle_row(
                "Require Password on Lock",
                SettingsHit::ToggleRequirePassword,
                st.require_password_on_lock,
            ),
            toggle_row(
                "Lock Screen on Sleep",
                SettingsHit::ToggleLockOnSleep,
                st.lock_on_sleep,
            ),
            toggle_row(
                "Built-in Polkit Agent",
                SettingsHit::TogglePolkitAgent,
                st.enable_polkit_agent,
            ),
            toggle_row(
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
        SettingsRowEntry {
            label: "Position on screen".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::DockCyclePosition,
                value: std::borrow::Cow::Borrowed(st.dock_position.label()),
                active: true,
            },
        },
        SettingsRowEntry {
            label: "Minimize using".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::DockToggleMinimizeEffect,
                value: std::borrow::Cow::Borrowed(st.dock_minimize_effect.label()),
                active: true,
            },
        },
        toggle_row(
            "Minimize into app icon",
            SettingsHit::DockToggleMinimizeIntoApp,
            st.dock_minimize_into_app,
        ),
        toggle_row(
            "Auto-hide the Dock",
            SettingsHit::DockToggleAutoHide,
            st.dock_auto_hide,
        ),
        toggle_row(
            "Animate opening apps",
            SettingsHit::DockToggleAnimateOpening,
            st.dock_animate_opening,
        ),
        toggle_row(
            "Show open indicators",
            SettingsHit::DockToggleShowIndicators,
            st.dock_show_indicators,
        ),
        toggle_row(
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
        toggle_row(
            "Reduce Transparency",
            SettingsHit::ToggleReduceTransparency,
            st.reduce_transparency,
        ),
        toggle_row(
            "Reduce Motion",
            SettingsHit::ToggleReducedMotion,
            st.reduced_motion,
        ),
        SettingsRowEntry {
            label: "Desktop Wallpaper".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::ToggleWallpaper,
                value: std::borrow::Cow::Borrowed(if st.has_custom_wallpaper {
                    "Custom PNG"
                } else {
                    "Golden Gate"
                }),
                active: st.has_custom_wallpaper,
            },
        },
        SettingsRowEntry {
            label: "Appearance Theme".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::ToggleAppearance,
                value: std::borrow::Cow::Borrowed(mode.label()),
                active: mode.is_dark(),
            },
        },
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
        SettingsRowEntry {
            label: output_label,
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::CycleTargetOutput,
                value: std::borrow::Cow::Owned(conn.clone()),
                active: st.output_count > 1,
            },
        },
        SettingsRowEntry {
            label: format!("HiDPI Scale ({conn})"),
            kind: SettingsRowKind::Stepper {
                down: SettingsHit::ScaleDown,
                up: SettingsHit::ScaleUp,
                value: format!("{scale_pct}%"),
            },
        },
        SettingsRowEntry {
            label: "Rotation".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::CycleRotation,
                value: std::borrow::Cow::Borrowed(rot_label),
                active: st.display_rotation != crate::config::DisplayRotation::Rotate0,
            },
        },
        SettingsRowEntry {
            label: "Adaptive Sync (VRR)".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::CycleVrrMode,
                value: std::borrow::Cow::Borrowed(vrr_label),
                active: st.vrr_mode != crate::config::VrrMode::Off,
            },
        },
        toggle_row(
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
        SettingsRowEntry {
            label: "Input Layouts".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::CycleKeyboardLayout,
                value: std::borrow::Cow::Owned(layouts_value),
                active: st.keyboard_layout.contains(','),
            },
        },
        SettingsRowEntry {
            label: "Switch Shortcut".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::CycleKeyboardOptions,
                value: std::borrow::Cow::Borrowed(options_label),
                active: !st.keyboard_options.trim().is_empty(),
            },
        },
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
        SettingsRowEntry {
            label: "Active Source".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::CycleActiveInputSource,
                value: std::borrow::Cow::Owned(truncate_ascii(st.active_kbd_layout, 4)),
                active: true,
            },
        },
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
    if rect_contains(layout.close_btn, x, y) || rect_contains(layout.done_btn, x, y) {
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
            SettingsRowKind::Toggle { hit, .. } => {
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
    let modal = layout.modal;

    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "System Settings",
        crate::text::FontWeight::Bold,
        15.0,
        pal.popover_text_primary,
        Point::from((layout.badge.loc.x + 44, layout.badge.loc.y + 8)),
    ) {
        elements.push(el);
    }

    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        let selected = *tab == st.active_tab;
        let col = if selected {
            [255, 255, 255, 255]
        } else {
            pal.popover_text_secondary
        };
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            tab.label(),
            if selected {
                crate::text::FontWeight::Bold
            } else {
                crate::text::FontWeight::Regular
            },
            12.5,
            col,
            Point::from((rect.loc.x + 34, rect.loc.y + 10)),
        ) {
            elements.push(el);
        }
    }

    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        st.active_tab.label(),
        crate::text::FontWeight::Bold,
        14.0,
        pal.popover_text_primary,
        Point::from((layout.content_x + 2, modal.loc.y + 68)),
    ) {
        elements.push(el);
    }

    render_settings_rows_text(text_cache, renderer, &layout, st, elements);
    render_settings_footer_text(text_cache, renderer, &layout, st, elements);
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
        let compact = row.size.h < 34;
        let label_size = if compact { 11.5 } else { 12.5 };
        let label_y = if compact {
            row.loc.y + 7
        } else {
            row.loc.y + 14
        };
        if let Some(el) = text_cache.render_label(
            renderer,
            &entry.label,
            label_size,
            pal.popover_text_primary,
            Point::from((row.loc.x + 14, label_y)),
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
            SettingsRowKind::Toggle { value, active, .. } => {
                let pill = settings_row_pill_rect(row);
                let fg = if *active {
                    [255, 255, 255, 255]
                } else {
                    pal.popover_text_secondary
                };
                if let Some(el) = text_cache.render_label_centered_weighted(
                    renderer,
                    value,
                    crate::text::FontWeight::Bold,
                    11.5,
                    fg,
                    phys_to_logical_rect(pill),
                ) {
                    elements.push(el);
                }
            }
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
            layout.modal.loc.x + 22,
            layout.modal.loc.y + layout.modal.size.h - 40,
        )),
    ) {
        elements.push(el);
    }

    for (rect, label, bold_fg) in [
        (layout.reload_btn, "Reload TOML", pal.popover_text_primary),
        (layout.done_btn, "Done", [255, 255, 255, 255]),
    ] {
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            label,
            crate::text::FontWeight::Bold,
            12.0,
            bold_fg,
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
    top_telemetry: &TopBarTelemetry<'_>,
    st: &SettingsRenderState<'_>,
) {
    let pal = ThemePalette::active();
    let layout = settings_modal_layout(screen_size, st.active_tab);
    let modal = layout.modal;

    // Icons pushed before panel boxes so glyphs composite above backgrounds.
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::ControlCenter,
        18,
        [255, 255, 255, 255],
        Point::from((layout.badge.loc.x + 8, layout.badge.loc.y + 8)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Close,
        12,
        pal.popover_icon,
        Point::from((layout.close_btn.loc.x + 7, layout.close_btn.loc.y + 7)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        let fg = if *tab == st.active_tab {
            [255, 255, 255, 255]
        } else {
            pal.popover_icon
        };
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            tab.icon(),
            16,
            fg,
            Point::from((rect.loc.x + 10, rect.loc.y + 10)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.badge,
        10,
        pal.accent_hover_fill,
        Some(pal.accent_hover_border),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.close_btn,
        13,
        pal.control_pill_off_rgba,
        Some(pal.border_rgba),
    );

    render_settings_controls_composite(out, shape_cache, renderer, &layout, top_telemetry, st);

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.reload_btn,
        10,
        pal.control_pill_off_rgba,
        Some(pal.border_rgba),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.done_btn,
        10,
        pal.accent_hover_fill,
        Some(pal.accent_hover_border),
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(
            Point::from((layout.divider_x, modal.loc.y + 66)),
            Size::from((1, (modal.size.h - 130).max(40))),
        ),
        pal.popover_separator,
    )));

    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        modal,
        WINDOW_RADIUS,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Some((24, 68)),
    );
}

fn render_settings_controls_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    top_telemetry: &TopBarTelemetry<'_>,
    st: &SettingsRenderState<'_>,
) {
    let pal = ThemePalette::active();
    let stepper_btn_fill = if pal.is_dark {
        [62, 62, 70, 224]
    } else {
        [214, 218, 228, 224]
    };
    let stepper_val_fill = if pal.is_dark {
        [44, 44, 50, 214]
    } else {
        [232, 235, 242, 224]
    };
    let row_card_fill = if pal.is_dark {
        [32, 32, 36, 178]
    } else {
        [236, 239, 245, 196]
    };
    let inactive_tab_fill = if pal.is_dark {
        [36, 36, 40, 120]
    } else {
        [228, 232, 240, 145]
    };
    for (entry, &row) in settings_tab_rows(st).iter().zip(&layout.rows) {
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
                        Some(pal.border_rgba),
                    );
                }
            }
            SettingsRowKind::Toggle { active, .. } => {
                let pill = settings_row_pill_rect(row);
                let (fill, border) = if *active {
                    (pal.accent_hover_fill, pal.accent_hover_border)
                } else {
                    (pal.control_pill_off_rgba, pal.border_rgba)
                };
                push_rounded_glass_box(out, shape_cache, renderer, pill, 10, fill, Some(border));
            }
        }
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            row,
            11,
            scale_glass_rgba(
                row_card_fill,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
            ),
            Some(pal.border_rgba),
        );
    }

    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        let selected = *tab == st.active_tab;
        let fill = if selected {
            pal.accent_hover_fill
        } else {
            scale_glass_rgba(
                inactive_tab_fill,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
            )
        };
        let border = if selected {
            Some(pal.accent_hover_border)
        } else {
            None
        };
        push_rounded_glass_box(out, shape_cache, renderer, rect, 10, fill, border);
    }
}
