//! Strongly-typed configuration enums and clamp helpers for RustyOS compositor.

use rusty_services::PowerProfile;
use smithay::utils::Transform;

use super::strip_toml_quotes;

/// Output orientation / rotation in 90-degree `wl_output` steps (`RUSTYOS-055`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayRotation {
    /// Upright `0°` (`wl_output::Transform::Normal`).
    #[default]
    Rotate0,
    /// Rotated `90°` counter-clockwise (`wl_output::Transform::_90`).
    Rotate90,
    /// Upside-down `180°` (`wl_output::Transform::_180`).
    Rotate180,
    /// Rotated `270°` counter-clockwise / `90°` clockwise (`wl_output::Transform::_270`).
    Rotate270,
}

impl DisplayRotation {
    /// Parses a display rotation angle or alias (`0`/`normal`, `90`, `180`/`inverted`, `270`).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let stripped = raw
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_ascii_lowercase();
        let cleaned = stripped
            .strip_suffix("deg")
            .or_else(|| stripped.strip_suffix('°'))
            .unwrap_or(&stripped)
            .trim();
        match cleaned {
            "0" | "normal" => Some(Self::Rotate0),
            "90" => Some(Self::Rotate90),
            "180" | "inverted" | "upside-down" => Some(Self::Rotate180),
            "270" => Some(Self::Rotate270),
            _ => None,
        }
    }

    /// Returns the rotation angle in degrees (`0`, `90`, `180`, `270`).
    #[must_use]
    pub const fn degrees(self) -> u16 {
        match self {
            Self::Rotate0 => 0,
            Self::Rotate90 => 90,
            Self::Rotate180 => 180,
            Self::Rotate270 => 270,
        }
    }

    /// Canonical string representation (`"0"`, `"90"`, `"180"`, `"270"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rotate0 => "0",
            Self::Rotate90 => "90",
            Self::Rotate180 => "180",
            Self::Rotate270 => "270",
        }
    }

    /// Returns `true` when width and height swap (`90°` or `270°`).
    #[must_use]
    pub const fn swaps_dimensions(self) -> bool {
        matches!(self, Self::Rotate90 | Self::Rotate270)
    }

    /// Converts to the Wayland `wl_output` / DRM surface transform.
    #[must_use]
    pub const fn to_wl_transform(self) -> Transform {
        match self {
            Self::Rotate0 => Transform::Normal,
            Self::Rotate90 => Transform::_90,
            Self::Rotate180 => Transform::_180,
            Self::Rotate270 => Transform::_270,
        }
    }

    /// Converts to the Winit OpenGL FBO-compensated `Output` transform so rendered orientation
    /// matches [`Self::to_wl_transform`] after Winit's default backbuffer Y-flip.
    #[must_use]
    pub const fn to_winit_transform(self) -> Transform {
        match self {
            Self::Rotate0 => Transform::Flipped180,
            Self::Rotate90 => Transform::Flipped90,
            Self::Rotate180 => Transform::Flipped,
            Self::Rotate270 => Transform::Flipped270,
        }
    }
}

/// Variable Refresh Rate (`Adaptive Sync` / `VRR`) runtime policy (`RUSTYOS-042`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VrrMode {
    /// Disable runtime VRR toggling.
    Off,
    /// Automatically enable VRR only when an unoccluded fullscreen window is displayed on a
    /// supported CRTC (`VrrSupport::Supported`, without forcing a modeset).
    #[default]
    Fullscreen,
    /// Enable VRR continuously on outputs where the kernel reports `VrrSupport::Supported`.
    Always,
}

impl VrrMode {
    pub(super) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "disabled" | "never" => Some(Self::Off),
            "fullscreen" | "auto" => Some(Self::Fullscreen),
            "always" | "on" | "true" | "enabled" => Some(Self::Always),
            _ => None,
        }
    }

    /// Canonical lowercase string representation (`"off"`, `"fullscreen"`, `"always"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Fullscreen => "fullscreen",
            Self::Always => "always",
        }
    }
}

/// Window drop shadow style (`Subtle` [default], `Normal`, `None`) (`RUSTYOS-080`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowShadowMode {
    /// Crisp, compact RustyOS elevation (default).
    #[default]
    Subtle,
    /// Traditional deeper elevation blur.
    Normal,
    /// Disabled (no drop shadows around windows).
    None,
}

impl WindowShadowMode {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "subtle" | "compact" | "small" | "low" | "true" | "on" | "enabled" => {
                Some(Self::Subtle)
            }
            "normal" | "default" | "large" | "high" => Some(Self::Normal),
            "none" | "off" | "false" | "disabled" => Some(Self::None),
            _ => None,
        }
    }

    /// Canonical string representation (`"subtle"`, `"normal"`, `"none"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Subtle => "subtle",
            Self::Normal => "normal",
            Self::None => "none",
        }
    }
}

/// Notification Center content preview visibility policy (`RUSTYOS-109`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationPreviewsMode {
    /// Always show full notification body text.
    Always,
    /// Show notification body text only while the session is unlocked.
    #[default]
    WhenUnlocked,
    /// Hide body preview text in Notification Center and banners.
    Never,
}

impl NotificationPreviewsMode {
    /// Parses `[notifications].show_previews` from TOML.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match strip_toml_quotes(raw).trim().to_ascii_lowercase().as_str() {
            "always" => Some(Self::Always),
            "when_unlocked" | "unlocked" => Some(Self::WhenUnlocked),
            "never" | "off" => Some(Self::Never),
            _ => None,
        }
    }

    /// Canonical TOML value (`"always"`, `"when_unlocked"`, `"never"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::WhenUnlocked => "when_unlocked",
            Self::Never => "never",
        }
    }
}

/// Default frosted glass opacity factor across macOS 27 shell surfaces (`RUSTYOS-048`).
pub const DEFAULT_GLASS_OPACITY: f32 = 0.78;
/// Minimum allowed glass opacity factor (`0.35..=1.0`).
pub const MIN_GLASS_OPACITY: f32 = 0.35;

/// Clamps configured or runtime shell glass opacity into `0.35..=1.0` (`DEFAULT_GLASS_OPACITY` if NaN/inf).
#[must_use]
pub fn clamp_glass_opacity(raw: f32) -> f32 {
    if raw.is_finite() {
        raw.clamp(MIN_GLASS_OPACITY, 1.0)
    } else {
        DEFAULT_GLASS_OPACITY
    }
}

/// Clamps a configured or detected display scale factor into a safe range (`0.5..=4.0`).
#[must_use]
pub fn clamp_display_scale(scale: f64) -> f64 {
    if scale.is_finite() {
        scale.clamp(0.5, 4.0)
    } else {
        1.0
    }
}

/// Default resting Dock app icon size in logical pixels (`36..=68`).
pub const DEFAULT_DOCK_ICON_SIZE: i32 = 50;

/// Clamps Dock resting icon size into `36..=68` logical pixels.
#[must_use]
pub fn clamp_dock_icon_size(px: i32) -> i32 {
    px.clamp(36, 68)
}

/// Default pinned Dock application keys (`RUSTYOS-073`).
#[must_use]
pub fn default_dock_pinned_apps() -> Vec<String> {
    vec![
        "files".to_owned(),
        "overview".to_owned(),
        "terminal".to_owned(),
        "browser".to_owned(),
        "notes".to_owned(),
        "settings".to_owned(),
    ]
}

/// Screen edge where the floating Dock is anchored (`RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockPosition {
    /// Centered along bottom screen edge (macOS default).
    #[default]
    Bottom,
    /// Vertically centered along left screen edge.
    Left,
    /// Vertically centered along right screen edge.
    Right,
}

impl DockPosition {
    /// Canonical TOML key string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    /// Human-readable UI label shown in `System Settings -> Desktop & Dock`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bottom => "Bottom",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }

    /// Cycles `Bottom -> Left -> Right -> Bottom`.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Bottom => Self::Left,
            Self::Left => Self::Right,
            Self::Right => Self::Bottom,
        }
    }

    /// Parses a persisted string from `[dock].position`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match strip_toml_quotes(raw).trim().to_ascii_lowercase().as_str() {
            "bottom" => Some(Self::Bottom),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// Dock visibility and auto-hide policy (`RUSTYOS-161`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockVisibilityMode {
    /// Keep the Dock permanently visible and reserve `usable_work_area`.
    #[default]
    AlwaysShow,
    /// Hide the Dock only when obstructed by a visible or maximized window on the same output.
    SmartHide,
    /// Always hide the Dock when idle and reveal on screen-edge hover.
    AlwaysHide,
}

impl DockVisibilityMode {
    /// Canonical TOML key string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlwaysShow => "always_show",
            Self::SmartHide => "smart_hide",
            Self::AlwaysHide => "always_hide",
        }
    }

    /// Human-readable UI label shown in `System Settings -> Dock & Desktop`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AlwaysShow => "Always Show",
            Self::SmartHide => "Smart Hide",
            Self::AlwaysHide => "Always Hide",
        }
    }

    /// Cycles `AlwaysShow -> SmartHide -> AlwaysHide -> AlwaysShow`.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::AlwaysShow => Self::SmartHide,
            Self::SmartHide => Self::AlwaysHide,
            Self::AlwaysHide => Self::AlwaysShow,
        }
    }

    /// Returns `true` when windows may expand across the Dock strip (`SmartHide` or `AlwaysHide`).
    #[must_use]
    pub const fn expands_work_area(self) -> bool {
        !matches!(self, Self::AlwaysShow)
    }

    /// Returns `true` when the Dock participates in edge-reveal auto-hide transitions.
    #[must_use]
    pub const fn uses_auto_hide_reveal(self) -> bool {
        !matches!(self, Self::AlwaysShow)
    }

    /// Parses a persisted string from `[dock].visibility`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match strip_toml_quotes(raw).trim().to_ascii_lowercase().as_str() {
            "always_show" | "always-show" | "show" | "never" => Some(Self::AlwaysShow),
            "smart_hide" | "smart-hide" | "smart" | "intellihide" | "intell_hide"
            | "when_obstructed" | "when-obstructed" | "dodge" => Some(Self::SmartHide),
            "always_hide" | "always-hide" | "hide" | "always" => Some(Self::AlwaysHide),
            _ => None,
        }
    }
}

/// Window minimize/restore animation effect into the Dock (`RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockMinimizeEffect {
    /// Curvilinear liquid funnel pulled into the target Dock icon (`macOS` default).
    #[default]
    Genie,
    /// Uniform smooth geometric scaling toward/from the target Dock icon.
    Scale,
}

impl DockMinimizeEffect {
    /// Canonical TOML key string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Genie => "genie",
            Self::Scale => "scale",
        }
    }

    /// Human-readable UI label shown in `System Settings -> Desktop & Dock`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Genie => "Genie",
            Self::Scale => "Scale",
        }
    }

    /// Toggles `Genie <-> Scale`.
    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::Genie => Self::Scale,
            Self::Scale => Self::Genie,
        }
    }

    /// Parses a persisted string from `[dock].minimize_effect`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match strip_toml_quotes(raw).trim().to_ascii_lowercase().as_str() {
            "genie" => Some(Self::Genie),
            "scale" => Some(Self::Scale),
            _ => None,
        }
    }
}

/// Left-click presentation style for the `Downloads` Dock utility (`RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DownloadsDisplayMode {
    /// Opens an interactive Liquid Glass Stack popover (`macOS` default).
    #[default]
    Stack,
    /// Launches `rusty-files --path ~/Downloads` directly.
    Folder,
}

impl DownloadsDisplayMode {
    /// Canonical TOML key string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stack => "stack",
            Self::Folder => "folder",
        }
    }

    /// Human-readable menu label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stack => "Stack",
            Self::Folder => "Folder",
        }
    }

    /// Parses a persisted string from `[dock].downloads_display`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match strip_toml_quotes(raw).trim().to_ascii_lowercase().as_str() {
            "stack" => Some(Self::Stack),
            "folder" => Some(Self::Folder),
            _ => None,
        }
    }
}

/// Popover geometry style when viewing `Downloads` as a Stack (`RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DownloadsViewMode {
    /// Chooses `Fan` for compact lists (`<= 8` items) and `Grid` for larger directories.
    #[default]
    Automatic,
    /// Arc-stacked floating pills radiating from the `Downloads` Dock slot.
    Fan,
    /// Multi-column translucent Liquid Glass popover grid.
    Grid,
    /// Compact scrollable/dense vertical Liquid Glass file list.
    List,
}

impl DownloadsViewMode {
    /// Canonical TOML key string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Fan => "fan",
            Self::Grid => "grid",
            Self::List => "list",
        }
    }

    /// Human-readable menu label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Fan => "Fan",
            Self::Grid => "Grid",
            Self::List => "List",
        }
    }

    /// Resolves `Automatic` into concrete `Fan` / `Grid` given `item_count`.
    #[must_use]
    pub const fn resolve_for_count(self, item_count: usize) -> Self {
        match self {
            Self::Automatic => {
                if item_count <= 8 {
                    Self::Fan
                } else {
                    Self::Grid
                }
            }
            other => other,
        }
    }

    /// Parses a persisted string from `[dock].downloads_view`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match strip_toml_quotes(raw).trim().to_ascii_lowercase().as_str() {
            "auto" | "automatic" => Some(Self::Automatic),
            "fan" => Some(Self::Fan),
            "grid" => Some(Self::Grid),
            "list" => Some(Self::List),
            _ => None,
        }
    }
}

/// Sort order for the Dock `Downloads` stack & context menu (`RUSTYOS-064` / `RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DownloadsSortMode {
    /// Sort newest added/created items first.
    DateAdded,
    /// Sort newest modified items first.
    #[default]
    DateModified,
    /// Sort case-insensitively by entry name (`A..Z`).
    Name,
    /// Sort largest files first.
    Size,
    /// Group directories first, then sort by extension/kind and name.
    Kind,
}

impl DownloadsSortMode {
    /// Canonical TOML key string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DateAdded => "date_added",
            Self::DateModified => "date_modified",
            Self::Name => "name",
            Self::Size => "size",
            Self::Kind => "kind",
        }
    }

    /// Human-readable label shown inside the `Downloads` context menu.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DateAdded => "Date Added",
            Self::DateModified => "Date Modified",
            Self::Name => "Name",
            Self::Size => "Size",
            Self::Kind => "Kind",
        }
    }

    /// Parses a persisted string from `[dock].downloads_sort`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let norm = strip_toml_quotes(raw).trim().to_ascii_lowercase();
        match norm.as_str() {
            "added" | "date_added" => Some(Self::DateAdded),
            "date" | "date_modified" | "modified" => Some(Self::DateModified),
            "name" | "alpha" => Some(Self::Name),
            "size" => Some(Self::Size),
            "kind" | "type" => Some(Self::Kind),
            _ => None,
        }
    }
}

/// RustyOS system appearance mode (`RUSTYOS-067`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppearanceMode {
    /// RustyOS Light frosted milk-glass (`#F5F6F8` popovers, `#007AFF` blue accent).
    #[default]
    Light,
    /// RustyOS Dark neutral graphite (`#262629` popovers, `#0A84FF` blue accent).
    Dark,
}

impl AppearanceMode {
    /// Canonical TOML string (`"light"` or `"dark"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Human-readable UI label (`"Light"` or `"Dark"`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Returns `true` when in Dark mode.
    #[must_use]
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    /// Toggles between `Light` and `Dark`.
    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    /// Parses a theme string from `[ui].theme` or `[ui].appearance`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let norm = strip_toml_quotes(raw).trim().to_ascii_lowercase();
        match norm.as_str() {
            "light" | "light_rustyos" | "rustyos_light" => Some(Self::Light),
            "dark" | "dark_rustyos" | "rustyos_dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// Low Power Mode policy stored under `[power].low_power_mode` (`RUSTYOS-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LowPowerMode {
    /// Use Balanced power profile regardless of power source.
    #[default]
    Never,
    /// Enable Low Power profile automatically while discharging on battery.
    BatteryOnly,
    /// Keep Low Power profile active on both battery and AC power.
    Always,
    /// Enable Low Power profile only while plugged into AC power.
    AcOnly,
}

impl LowPowerMode {
    /// Canonical lowercase TOML value (`"never"`, `"battery"`, `"always"`, `"ac"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::BatteryOnly => "battery",
            Self::Always => "always",
            Self::AcOnly => "ac",
        }
    }

    /// Human-readable label matching RustyOS Battery settings.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Never => "Never",
            Self::BatteryOnly => "Only on Battery",
            Self::Always => "Always",
            Self::AcOnly => "Only on Power Adapter",
        }
    }

    /// Parses a `[power].low_power_mode` TOML value.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let norm = strip_toml_quotes(raw).trim().to_ascii_lowercase();
        match norm.as_str() {
            "never" | "off" => Some(Self::Never),
            "battery" | "only_on_battery" | "on_battery" => Some(Self::BatteryOnly),
            "always" | "on" => Some(Self::Always),
            "ac" | "adapter" | "only_on_ac" | "only_on_adapter" => Some(Self::AcOnly),
            _ => None,
        }
    }

    /// Resolves desired [`PowerProfile`] given AC online & hardware battery state.
    #[must_use]
    pub const fn desired_profile(self, ac_online: bool, has_battery: bool) -> PowerProfile {
        match self {
            Self::Never => PowerProfile::Balanced,
            Self::Always => PowerProfile::LowPower,
            Self::BatteryOnly => {
                if has_battery && !ac_online {
                    PowerProfile::LowPower
                } else {
                    PowerProfile::Balanced
                }
            }
            Self::AcOnly => {
                if ac_online || !has_battery {
                    PowerProfile::LowPower
                } else {
                    PowerProfile::Balanced
                }
            }
        }
    }
}
