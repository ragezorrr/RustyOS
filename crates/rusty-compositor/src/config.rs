//! User configuration loader (`~/.config/rustyos/config.toml`) and keyboard layout helpers (`RUSTYOS-029`).
//!
//! Parses a lightweight, dependency-free TOML subset with `[keyboard]`, `[ui]`, and `[autostart]`
//! sections and provides safe defaults (`us,ru` + `grp:alt_shift_toggle`) when the config file is
//! absent or partially specified.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use rusty_services::PowerProfile;
use smithay::input::keyboard::{Keysym, ModifiersState, XkbConfig};
use smithay::utils::Transform;
use tracing::{info, warn};

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
    fn parse(raw: &str) -> Option<Self> {
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

/// Parsed keyboard modifier/keysym combination for configurable compositor shortcuts (`RUSTYOS-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ShortcutCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
    pub keysym: Keysym,
}

impl ShortcutCombo {
    /// Checks whether live modifiers + keysym match this configured binding (ASCII letters matched case-insensitively).
    #[must_use]
    pub fn matches(self, mods: ModifiersState, sym: Keysym) -> bool {
        mods.ctrl == self.ctrl
            && mods.alt == self.alt
            && mods.shift == self.shift
            && mods.logo == self.logo
            && normalize_match_keysym(sym) == normalize_match_keysym(self.keysym)
    }

    /// Formats a shortcut back into canonical `Super+Shift+R` notation.
    #[must_use]
    pub fn canonical_string(self) -> String {
        let mut parts = Vec::with_capacity(5);
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.logo {
            parts.push("Super".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(keysym_canonical_name(self.keysym));
        parts.join("+")
    }
}

const fn normalize_match_keysym(sym: Keysym) -> u32 {
    let raw = sym.raw();
    if raw >= Keysym::A.raw() && raw <= Keysym::Z.raw() {
        raw + 0x20
    } else if raw == Keysym::ISO_Left_Tab.raw() {
        Keysym::Tab.raw()
    } else {
        raw
    }
}

fn keysym_canonical_name(sym: Keysym) -> String {
    match sym {
        Keysym::space => "Space".to_owned(),
        Keysym::Tab | Keysym::ISO_Left_Tab => "Tab".to_owned(),
        Keysym::comma => "Comma".to_owned(),
        Keysym::period => "Period".to_owned(),
        Keysym::slash => "Slash".to_owned(),
        Keysym::Left => "Left".to_owned(),
        Keysym::Right => "Right".to_owned(),
        Keysym::Up => "Up".to_owned(),
        Keysym::Down => "Down".to_owned(),
        Keysym::Return | Keysym::KP_Enter => "Return".to_owned(),
        Keysym::BackSpace => "BackSpace".to_owned(),
        Keysym::Delete => "Delete".to_owned(),
        Keysym::Escape => "Escape".to_owned(),
        Keysym::Print => "Print".to_owned(),
        _ => {
            let raw = normalize_match_keysym(sym);
            if let Some(ch) = Keysym::new(raw).key_char()
                && ch.is_ascii_alphanumeric()
            {
                return ch.to_ascii_uppercase().to_string();
            }
            if raw >= Keysym::F1.raw() && raw <= Keysym::F12.raw() {
                return format!("F{}", raw - Keysym::F1.raw() + 1);
            }
            format!("0x{raw:x}")
        }
    }
}

/// Parses `"Super+Space"`, `"Alt+W"`, `"Super+Shift+R"`, `"Alt+Left"` into [`ShortcutCombo`] (`RUSTYOS-131`).
#[must_use]
pub fn parse_shortcut_combo(raw: &str) -> Option<ShortcutCombo> {
    let stripped = strip_toml_quotes(raw);
    if stripped.is_empty() {
        return None;
    }
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut logo = false;
    let mut keysym = None;

    for token in stripped.split('+').map(str::trim).filter(|t| !t.is_empty()) {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "opt" | "option" => alt = true,
            "shift" => shift = true,
            "super" | "logo" | "meta" | "win" | "cmd" => logo = true,
            key => {
                if keysym.is_some() {
                    return None;
                }
                keysym = parse_key_token(key);
                keysym?;
            }
        }
    }

    let ks = keysym?;
    let is_fn_or_print =
        ks == Keysym::Print || (ks.raw() >= Keysym::F1.raw() && ks.raw() <= Keysym::F12.raw());
    if !ctrl && !alt && !logo && !is_fn_or_print {
        return None;
    }
    Some(ShortcutCombo {
        ctrl,
        alt,
        shift,
        logo,
        keysym: ks,
    })
}

fn parse_key_token(lower: &str) -> Option<Keysym> {
    let sym = match lower {
        "space" => Keysym::space,
        "tab" => Keysym::Tab,
        "comma" | "," => Keysym::comma,
        "period" | "." => Keysym::period,
        "slash" | "/" => Keysym::slash,
        "left" => Keysym::Left,
        "right" => Keysym::Right,
        "up" => Keysym::Up,
        "down" => Keysym::Down,
        "return" | "enter" => Keysym::Return,
        "backspace" => Keysym::BackSpace,
        "delete" | "del" => Keysym::Delete,
        "escape" | "esc" => Keysym::Escape,
        "print" => Keysym::Print,
        "f1" => Keysym::F1,
        "f2" => Keysym::F2,
        "f3" => Keysym::F3,
        "f4" => Keysym::F4,
        "f5" => Keysym::F5,
        "f6" => Keysym::F6,
        "f7" => Keysym::F7,
        "f8" => Keysym::F8,
        "f9" => Keysym::F9,
        "f10" => Keysym::F10,
        "f11" => Keysym::F11,
        "f12" => Keysym::F12,
        single if single.len() == 1 => {
            let b = single.as_bytes()[0];
            if b.is_ascii_lowercase() || b.is_ascii_digit() {
                Keysym::new(u32::from(b))
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(sym)
}

/// User-customizable compositor shortcuts stored under `[shortcuts]` (`RUSTYOS-131`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutBindings {
    pub launcher: String,
    pub overview: String,
    pub lock_screen: String,
    pub settings: String,
    pub reload_config: String,
    pub quick_settings: String,
    pub close_window: String,
    pub toggle_maximize: String,
    pub toggle_fullscreen: String,
    pub minimize_window: String,
    pub snap_left: String,
    pub snap_right: String,
    pub snap_center: String,
    pub switch_layout: String,
    pub reduced_motion: String,
    pub toggle_notifications: String,
    pub clear_notifications: String,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        Self {
            launcher: String::from("Super+Space"),
            overview: String::from("Super+Tab"),
            lock_screen: String::from("Super+L"),
            settings: String::from("Super+Comma"),
            reload_config: String::from("Super+Shift+R"),
            quick_settings: String::from("Alt+S"),
            close_window: String::from("Alt+W"),
            toggle_maximize: String::from("Alt+M"),
            toggle_fullscreen: String::from("Alt+F"),
            minimize_window: String::from("Alt+H"),
            snap_left: String::from("Alt+Left"),
            snap_right: String::from("Alt+Right"),
            snap_center: String::from("Alt+C"),
            switch_layout: String::from("Alt+L"),
            reduced_motion: String::from("Alt+R"),
            toggle_notifications: String::from("Alt+N"),
            clear_notifications: String::from("Alt+Shift+N"),
        }
    }
}

impl ShortcutBindings {
    fn apply_kv(&mut self, key: &str, val: &str) {
        let Some(combo) = parse_shortcut_combo(val) else {
            return;
        };
        let canonical = combo.canonical_string();
        let target = match key {
            "launcher" => &mut self.launcher,
            "overview" => &mut self.overview,
            "lock_screen" => &mut self.lock_screen,
            "settings" => &mut self.settings,
            "reload_config" => &mut self.reload_config,
            "quick_settings" => &mut self.quick_settings,
            "close_window" => &mut self.close_window,
            "toggle_maximize" => &mut self.toggle_maximize,
            "toggle_fullscreen" => &mut self.toggle_fullscreen,
            "minimize_window" => &mut self.minimize_window,
            "snap_left" => &mut self.snap_left,
            "snap_right" => &mut self.snap_right,
            "snap_center" => &mut self.snap_center,
            "switch_layout" => &mut self.switch_layout,
            "reduced_motion" => &mut self.reduced_motion,
            "toggle_notifications" => &mut self.toggle_notifications,
            "clear_notifications" => &mut self.clear_notifications,
            _ => return,
        };
        *target = canonical;
    }

    fn append_toml(&self, out: &mut String) {
        use std::fmt::Write as _;

        let _ = writeln!(out, "\n[shortcuts]");
        for (k, v) in [
            ("launcher", &self.launcher),
            ("overview", &self.overview),
            ("lock_screen", &self.lock_screen),
            ("settings", &self.settings),
            ("reload_config", &self.reload_config),
            ("quick_settings", &self.quick_settings),
            ("close_window", &self.close_window),
            ("toggle_maximize", &self.toggle_maximize),
            ("toggle_fullscreen", &self.toggle_fullscreen),
            ("minimize_window", &self.minimize_window),
            ("snap_left", &self.snap_left),
            ("snap_right", &self.snap_right),
            ("snap_center", &self.snap_center),
            ("switch_layout", &self.switch_layout),
            ("reduced_motion", &self.reduced_motion),
            ("toggle_notifications", &self.toggle_notifications),
            ("clear_notifications", &self.clear_notifications),
        ] {
            let _ = writeln!(out, "{k} = \"{}\"", escape_toml_string(v));
        }
    }
}

/// Persistent user preferences loaded from `~/.config/rustyos/config.toml`.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct UserConfig {
    /// Comma-separated XKB layout list (e.g. `"us,ru"`).
    pub keyboard_layout: String,
    /// Optional comma-separated XKB layout variants.
    pub keyboard_variant: String,
    /// XKB options (e.g. `"grp:alt_shift_toggle"`).
    pub keyboard_options: String,
    /// Key repeat delay in milliseconds (`50..=2000`).
    pub repeat_delay: i32,
    /// Key repeat rate in keys/second (`1..=120`).
    pub repeat_rate: i32,
    /// System UI theme (`Light` or `Dark`, default `Light`) (`RUSTYOS-067`).
    pub appearance: AppearanceMode,
    /// System UI accent color index (`0..=6`, default `0` = RustyOS Blue) (`RUSTYOS-114`).
    pub ui_accent_index: u8,
    /// Prefer instant transitions (`reduced_motion`).
    pub reduced_motion: bool,
    /// Shell frosted glass opacity scalar (`0.35..=1.0`, default `0.78`) (`RUSTYOS-048`).
    pub glass_opacity: f32,
    /// Render nearly opaque shell panels for legibility/accessibility (`RUSTYOS-048`).
    pub reduce_transparency: bool,
    /// Show compact workspace numbers (`1..=9`) and layout badge in Menu Bar (`RUSTYOS-049`, default `false`).
    pub menubar_show_workspaces: bool,
    /// Show `StatusNotifierItem` tray applets in Menu Bar (`RUSTYOS-049`, default `false`).
    pub menubar_show_tray: bool,
    /// Show keyboard input source indicator (`EN`/`RU`) in Menu Bar (`RUSTYOS-049`, default `false`).
    pub menubar_show_input_source: bool,
    /// Show standalone volume speaker glyph next to Wi-Fi in Menu Bar (`RUSTYOS-049`, default `false`).
    pub menubar_show_volume: bool,
    /// Show Wi-Fi / Ethernet indicator in Menu Bar (`RUSTYOS-141`, default `true`).
    pub menubar_show_wifi: bool,
    /// Show Battery indicator in Menu Bar (`RUSTYOS-141`, default `true`).
    pub menubar_show_battery: bool,
    /// Show standalone Bluetooth indicator in Menu Bar (`RUSTYOS-141`, default `false`).
    pub menubar_show_bluetooth: bool,
    /// Show numeric battery percentage beside the Battery glyph (`RUSTYOS-141`, default `false`).
    pub menubar_show_battery_percentage: bool,
    /// Automatically adapt Menu Bar foreground contrast to the top wallpaper strip (`RUSTYOS-141`, default `true`).
    pub menubar_adaptive_contrast: bool,
    /// Format Menu Bar clock using 24-hour time (`RUSTYOS-049`, default `false` = 12h AM/PM).
    pub clock_24h: bool,
    /// Session clock locale (`"en"`, `"ru"`, `"uk"`) (`RUSTYOS-141`, default `"en"`).
    pub clock_locale: String,
    /// Window drop shadow profile (`Subtle` [default], `Normal`, `None`) (`RUSTYOS-080`).
    pub window_shadow: WindowShadowMode,
    /// Prefer the highest supported refresh rate (60/120/144/240 Hz) at the display's native
    /// resolution on DRM/KMS outputs (`RUSTYOS-042`).
    pub prefer_max_refresh: bool,
    /// Variable Refresh Rate (`VRR`) runtime policy on DRM/KMS (`RUSTYOS-042`).
    pub vrr_mode: VrrMode,
    /// Default `HiDPI` scale factor applied when no per-output override exists (`RUSTYOS-043`).
    pub display_scale: f64,
    /// Optional per-connector `HiDPI` scale overrides (`"eDP-1" -> 1.5`, `"DP-1" -> 2.0`) (`RUSTYOS-043`).
    pub output_scales: BTreeMap<String, f64>,
    /// Default display output orientation when no per-output or CLI override exists (`RUSTYOS-055`).
    pub display_rotation: DisplayRotation,
    /// Optional per-connector display rotation overrides (`"eDP-1" -> Rotate180`) (`RUSTYOS-055`).
    pub output_rotations: BTreeMap<String, DisplayRotation>,
    /// Optional custom desktop wallpaper `.png` path (`~/` expanded).
    pub wallpaper: Option<PathBuf>,
    /// Optional `XCursor` theme override (falls back to `XCURSOR_THEME` and system themes).
    pub cursor_theme: Option<String>,
    /// Commands launched automatically once the Wayland socket is ready.
    pub autostart: Vec<String>,
    /// Require PAM password verification (`unix_chkpwd`) to unlock the Lock Screen (`RUSTYOS-056`).
    pub require_password_on_lock: bool,
    /// Automatically lock the session on `systemd-logind` `PrepareForSleep(true)` / `Session.Lock` (`RUSTYOS-056`).
    pub lock_on_sleep: bool,
    /// Enable the native compositor `org.freedesktop.PolicyKit1.AuthenticationAgent` (`RUSTYOS-056`).
    pub enable_polkit_agent: bool,
    /// Resting Dock app icon edge size in logical pixels (`36..=68`, default `50`) (`RUSTYOS-073`).
    pub dock_icon_size: i32,
    /// Screen edge anchoring the Dock (`Bottom`, `Left`, `Right`) (`RUSTYOS-073`).
    pub dock_position: DockPosition,
    /// Animation used when minimizing/restoring windows (`Genie` or `Scale`) (`RUSTYOS-073`).
    pub dock_minimize_effect: DockMinimizeEffect,
    /// Minimize windows directly into their owning application's Dock icon (`RUSTYOS-073`).
    pub dock_minimize_into_app: bool,
    /// Automatically hide Dock when idle and expand `usable_work_area` (`RUSTYOS-073`).
    pub dock_auto_hide: bool,
    /// Bounce app icon while launching (`RUSTYOS-073`).
    pub dock_animate_opening: bool,
    /// Render running-application dot indicators beneath active Dock apps (`RUSTYOS-073`).
    pub dock_show_indicators: bool,
    /// Show recently closed unpinned apps section before the right divider (`RUSTYOS-073`).
    pub dock_show_recent: bool,
    /// Ordered list of pinned Dock application keys (`RUSTYOS-073`).
    pub dock_pinned_apps: Vec<String>,
    /// Left-click presentation mode (`Stack` or `Folder`) for `Downloads` (`RUSTYOS-073`).
    pub dock_downloads_display: DownloadsDisplayMode,
    /// Visual layout mode (`Automatic`, `Fan`, `Grid`, `List`) for `Downloads` stack (`RUSTYOS-073`).
    pub dock_downloads_view: DownloadsViewMode,
    /// Sort order used in the Dock `Downloads` stack & context menu (`RUSTYOS-064` / `RUSTYOS-073`).
    pub downloads_sort: DownloadsSortMode,
    /// Enable automatic background Arch/RustyOS update checks (`RUSTYOS-076`).
    pub updates_enabled: bool,
    /// Interval in minutes between automatic background update checks (`15..=10080`) (`RUSTYOS-076`).
    pub updates_interval_minutes: u32,
    /// Show the updates indicator in the `TopBar` Menu Bar when updates are available (`RUSTYOS-076`).
    pub updates_show_in_menu_bar: bool,
    /// Master switch allowing notification banners and Notification Center entries (`RUSTYOS-109`).
    pub notifications_allow: bool,
    /// Body preview visibility policy (`Always`, `WhenUnlocked`, `Never`) (`RUSTYOS-109`).
    pub notifications_show_previews: NotificationPreviewsMode,
    /// Allow notifications when display is sleeping (`RUSTYOS-109`).
    pub notifications_when_sleeping: bool,
    /// Allow notifications when screen is locked (`RUSTYOS-109`).
    pub notifications_when_locked: bool,
    /// Suppress notifications while mirroring or screen sharing (`RUSTYOS-109`).
    pub notifications_mirroring_off: bool,
    /// `[power].low_power_mode` power-profile policy (`RUSTYOS-131`).
    pub power_low_power_mode: LowPowerMode,
    /// `[power].display_sleep_minutes` inactivity display sleep hint (`0` = Never, `0..=180`) (`RUSTYOS-131`).
    pub power_display_sleep_minutes: u32,
    /// `[power].auto_lock_minutes` automatic idle lock timeout (`0` = Off, `0..=120`) (`RUSTYOS-131`).
    pub power_auto_lock_minutes: u32,
    /// `[power].dim_on_battery` display dim hint when running on battery (`RUSTYOS-131`).
    pub power_dim_on_battery: bool,
    /// `[shortcuts]` customizable compositor keyboard bindings (`RUSTYOS-131`).
    pub shortcuts: ShortcutBindings,
}

/// Default interval between background update checks (`180` minutes / 3 hours) (`RUSTYOS-076`).
pub const DEFAULT_UPDATES_INTERVAL_MINUTES: u32 = 180;
/// Minimum allowed interval between background update checks (`15` minutes) (`RUSTYOS-076`).
pub const MIN_UPDATES_INTERVAL_MINUTES: u32 = 15;
/// Maximum allowed interval between background update checks (`7` days) (`RUSTYOS-076`).
pub const MAX_UPDATES_INTERVAL_MINUTES: u32 = 10_080;

/// Clamps `[updates] check_interval_minutes` to `15..=10080` (`RUSTYOS-076`).
#[must_use]
pub fn clamp_updates_interval_minutes(minutes: u32) -> u32 {
    minutes.clamp(MIN_UPDATES_INTERVAL_MINUTES, MAX_UPDATES_INTERVAL_MINUTES)
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            keyboard_layout: "us,ru".to_owned(),
            keyboard_variant: String::new(),
            keyboard_options: "grp:alt_shift_toggle".to_owned(),
            repeat_delay: 200,
            repeat_rate: 25,
            appearance: AppearanceMode::Light,
            ui_accent_index: 0,
            reduced_motion: false,
            glass_opacity: DEFAULT_GLASS_OPACITY,
            reduce_transparency: false,
            menubar_show_workspaces: false,
            menubar_show_tray: false,
            menubar_show_input_source: false,
            menubar_show_volume: false,
            menubar_show_wifi: true,
            menubar_show_battery: true,
            menubar_show_bluetooth: false,
            menubar_show_battery_percentage: false,
            menubar_adaptive_contrast: true,
            clock_24h: false,
            clock_locale: "en".to_owned(),
            window_shadow: WindowShadowMode::Subtle,
            prefer_max_refresh: true,
            vrr_mode: VrrMode::Fullscreen,
            display_scale: 1.0,
            output_scales: BTreeMap::new(),
            display_rotation: DisplayRotation::Rotate0,
            output_rotations: BTreeMap::new(),
            wallpaper: None,
            cursor_theme: None,
            autostart: Vec::new(),
            require_password_on_lock: true,
            lock_on_sleep: true,
            enable_polkit_agent: true,
            dock_icon_size: DEFAULT_DOCK_ICON_SIZE,
            dock_position: DockPosition::Bottom,
            dock_minimize_effect: DockMinimizeEffect::Genie,
            dock_minimize_into_app: false,
            dock_auto_hide: false,
            dock_animate_opening: true,
            dock_show_indicators: true,
            dock_show_recent: true,
            dock_pinned_apps: default_dock_pinned_apps(),
            dock_downloads_display: DownloadsDisplayMode::Stack,
            dock_downloads_view: DownloadsViewMode::Automatic,
            downloads_sort: DownloadsSortMode::DateModified,
            updates_enabled: true,
            updates_interval_minutes: DEFAULT_UPDATES_INTERVAL_MINUTES,
            updates_show_in_menu_bar: true,
            notifications_allow: true,
            notifications_show_previews: NotificationPreviewsMode::WhenUnlocked,
            notifications_when_sleeping: false,
            notifications_when_locked: true,
            notifications_mirroring_off: true,
            power_low_power_mode: LowPowerMode::Never,
            power_display_sleep_minutes: 10,
            power_auto_lock_minutes: 0,
            power_dim_on_battery: true,
            shortcuts: ShortcutBindings::default(),
        }
    }
}

impl UserConfig {
    /// Loads [`UserConfig`] from `$XDG_CONFIG_HOME/rustyos/config.toml` or `~/.config/rustyos/config.toml`.
    ///
    /// Falls back cleanly to [`UserConfig::default`] if no file exists or cannot be read.
    #[must_use]
    pub fn load_default() -> Self {
        let Some(path) = default_user_config_path() else {
            return Self::default();
        };
        Self::load_from_path(&path)
    }

    /// Loads [`UserConfig`] from an explicit file path, logging diagnostics and falling back on I/O errors.
    #[must_use]
    pub fn load_from_path(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(contents) => {
                let parsed = Self::parse_toml(&contents);
                info!(
                    path = %path.display(),
                    layout = %parsed.keyboard_layout,
                    options = %parsed.keyboard_options,
                    reduced_motion = parsed.reduced_motion,
                    autostart_count = parsed.autostart.len(),
                    "Loaded RustyOS user configuration"
                );
                parsed
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "Failed to read user configuration; using built-in defaults"
                );
                Self::default()
            }
        }
    }

    /// Parses a TOML configuration string (`[keyboard]`, `[ui]`, `[autostart]`) without external crates.
    #[must_use]
    pub fn parse_toml(src: &str) -> Self {
        let mut cfg = Self::default();
        let mut section = String::new();
        let mut pending_array_key: Option<String> = None;
        let mut pending_array_buf = String::new();

        for raw_line in src.lines() {
            let line = strip_toml_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }

            if let Some(array_key) = pending_array_key.take() {
                pending_array_buf.push(' ');
                pending_array_buf.push_str(line);
                if line.ends_with(']') {
                    if section == "autostart" && array_key == "commands" {
                        cfg.autostart = parse_toml_string_array(&pending_array_buf);
                    }
                    pending_array_buf.clear();
                } else {
                    pending_array_key = Some(array_key);
                }
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
                const OUTPUTS_PREFIX: &str = "display.outputs.";
                let raw_section = line[1..line.len() - 1].trim();
                section = if raw_section.is_char_boundary(OUTPUTS_PREFIX.len())
                    && raw_section[..OUTPUTS_PREFIX.len()].eq_ignore_ascii_case(OUTPUTS_PREFIX)
                {
                    format!("{OUTPUTS_PREFIX}{}", &raw_section[OUTPUTS_PREFIX.len()..])
                } else {
                    raw_section.to_ascii_lowercase()
                };
                continue;
            }

            let Some((raw_key, raw_val)) = line.split_once('=') else {
                continue;
            };
            let key = raw_key.trim();
            let val = raw_val.trim();

            if val.starts_with('[') && !val.ends_with(']') {
                pending_array_key = Some(key.to_owned());
                val.clone_into(&mut pending_array_buf);
                continue;
            }

            cfg.apply_toml_kv(&section, key, val);
        }

        cfg
    }

    fn apply_toml_kv(&mut self, section: &str, key: &str, val: &str) {
        match (section, key) {
            ("keyboard", "layout") => {
                if let Some(s) = parse_toml_string(val)
                    && !s.trim().is_empty()
                {
                    self.keyboard_layout = s;
                }
            }
            ("keyboard", "variant") => {
                if let Some(s) = parse_toml_string(val) {
                    self.keyboard_variant = s;
                }
            }
            ("keyboard", "options") => {
                if let Some(s) = parse_toml_string(val) {
                    self.keyboard_options = s;
                }
            }
            ("keyboard", "repeat_delay") => {
                if let Ok(n) = val.parse::<i32>() {
                    self.repeat_delay = n.clamp(50, 2000);
                }
            }
            ("keyboard", "repeat_rate") => {
                if let Ok(n) = val.parse::<i32>() {
                    self.repeat_rate = n.clamp(1, 120);
                }
            }
            ("ui", _) => self.apply_ui_kv(key, val),
            ("display", _) => self.apply_display_kv(key, val),
            ("display.scales", _) => {
                self.apply_output_kv(strip_toml_quotes(key), "scale", val);
            }
            ("display.rotations", _) => {
                self.apply_output_kv(strip_toml_quotes(key), "rotation", val);
            }
            ("autostart", "commands") => {
                self.autostart = parse_toml_string_array(val);
            }
            ("security", _) => self.apply_security_kv(key, val),
            ("dock", _) => self.apply_dock_kv(key, val),
            ("top_bar", _) => self.apply_top_bar_kv(key, val),
            ("updates", _) => self.apply_updates_kv(key, val),
            ("notifications", _) => self.apply_notifications_kv(key, val),
            ("power", _) => self.apply_power_kv(key, val),
            ("shortcuts", _) => self.shortcuts.apply_kv(key, val),
            _ => {
                if let Some(raw_output) = section.strip_prefix("display.outputs.") {
                    self.apply_output_kv(strip_toml_quotes(raw_output), key, val);
                }
            }
        }
    }

    fn apply_top_bar_kv(&mut self, key: &str, val: &str) {
        match key {
            "show_wifi" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.menubar_show_wifi = b;
                }
            }
            "show_battery" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.menubar_show_battery = b;
                }
            }
            "show_sound" | "show_volume" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.menubar_show_volume = b;
                }
            }
            "show_bluetooth" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.menubar_show_bluetooth = b;
                }
            }
            "show_battery_percentage" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.menubar_show_battery_percentage = b;
                }
            }
            "adaptive_contrast" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.menubar_adaptive_contrast = b;
                }
            }
            "clock_24h" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.clock_24h = b;
                }
            }
            "clock_locale" => {
                if let Some(s) = parse_toml_string(val) {
                    let norm = s.trim().to_ascii_lowercase();
                    if !norm.is_empty() && norm.len() <= 16 {
                        self.clock_locale = norm;
                    }
                }
            }
            _ => {}
        }
    }

    fn apply_dock_kv(&mut self, key: &str, val: &str) {
        match key {
            "icon_size" => {
                if let Ok(px) = val.parse::<i32>() {
                    self.dock_icon_size = clamp_dock_icon_size(px);
                }
            }
            "position" => {
                if let Some(pos) = DockPosition::parse(val) {
                    self.dock_position = pos;
                }
            }
            "minimize_effect" => {
                if let Some(effect) = DockMinimizeEffect::parse(val) {
                    self.dock_minimize_effect = effect;
                }
            }
            "minimize_into_app" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.dock_minimize_into_app = b;
                }
            }
            "auto_hide" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.dock_auto_hide = b;
                }
            }
            "animate_opening" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.dock_animate_opening = b;
                }
            }
            "show_indicators" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.dock_show_indicators = b;
                }
            }
            "show_recent" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.dock_show_recent = b;
                }
            }
            "pinned_apps" => {
                let parsed: Vec<String> = parse_toml_string_array(val)
                    .into_iter()
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !parsed.is_empty() {
                    self.dock_pinned_apps = parsed;
                }
            }
            "downloads_display" => {
                if let Some(mode) = DownloadsDisplayMode::parse(val) {
                    self.dock_downloads_display = mode;
                }
            }
            "downloads_view" => {
                if let Some(mode) = DownloadsViewMode::parse(val) {
                    self.dock_downloads_view = mode;
                }
            }
            "downloads_sort" => {
                if let Some(mode) = DownloadsSortMode::parse(val) {
                    self.downloads_sort = mode;
                }
            }
            // Also accepts deprecated `[dock].magnification_size` silently for backwards compatibility.
            _ => {}
        }
    }

    fn apply_display_kv(&mut self, key: &str, val: &str) {
        match key {
            "prefer_max_refresh" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.prefer_max_refresh = b;
                }
            }
            "vrr" | "vrr_mode" => {
                if let Some(s) = parse_toml_string(val)
                    && let Some(mode) = VrrMode::parse(&s)
                {
                    self.vrr_mode = mode;
                }
            }
            "scale" => {
                if let Ok(scale) = val.parse::<f64>() {
                    self.display_scale = clamp_display_scale(scale);
                }
            }
            "scales" => {
                for (name, scale) in parse_toml_inline_table(val, |v| v.parse::<f64>().ok()) {
                    self.output_scales.insert(name, clamp_display_scale(scale));
                }
            }
            "rotation" | "transform" => {
                if let Some(rot) = DisplayRotation::parse(val) {
                    self.display_rotation = rot;
                }
            }
            "rotations" => {
                for (name, rot) in parse_toml_inline_table(val, DisplayRotation::parse) {
                    self.output_rotations.insert(name, rot);
                }
            }
            _ => {}
        }
    }

    fn apply_output_kv(&mut self, output_name: &str, key: &str, val: &str) {
        if output_name.is_empty() {
            return;
        }
        match key {
            "rotation" | "transform" => {
                if let Some(rot) = DisplayRotation::parse(val) {
                    self.output_rotations.insert(output_name.to_owned(), rot);
                }
            }
            "scale" => {
                if let Ok(scale) = val.parse::<f64>() {
                    self.output_scales
                        .insert(output_name.to_owned(), clamp_display_scale(scale));
                }
            }
            _ => {}
        }
    }

    fn apply_security_kv(&mut self, key: &str, val: &str) {
        match key {
            "require_password_on_lock" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.require_password_on_lock = b;
                }
            }
            "lock_on_sleep" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.lock_on_sleep = b;
                }
            }
            "enable_polkit_agent" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.enable_polkit_agent = b;
                }
            }
            _ => {}
        }
    }

    fn apply_updates_kv(&mut self, key: &str, val: &str) {
        match key {
            "enabled" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.updates_enabled = b;
                }
            }
            "check_interval_minutes" => {
                if let Ok(mins) = val.parse::<u32>() {
                    self.updates_interval_minutes = clamp_updates_interval_minutes(mins);
                }
            }
            "show_in_menu_bar" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.updates_show_in_menu_bar = b;
                }
            }
            _ => {}
        }
    }

    fn apply_notifications_kv(&mut self, key: &str, val: &str) {
        match key {
            "allow" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.notifications_allow = b;
                }
            }
            "show_previews" => {
                if let Some(mode) = NotificationPreviewsMode::parse(val) {
                    self.notifications_show_previews = mode;
                }
            }
            "when_sleeping" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.notifications_when_sleeping = b;
                }
            }
            "when_locked" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.notifications_when_locked = b;
                }
            }
            "mirroring_off" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.notifications_mirroring_off = b;
                }
            }
            _ => {}
        }
    }

    fn apply_power_kv(&mut self, key: &str, val: &str) {
        match key {
            "low_power_mode" => {
                if let Some(mode) = LowPowerMode::parse(val) {
                    self.power_low_power_mode = mode;
                }
            }
            "display_sleep_minutes" => {
                if let Ok(m) = val.parse::<u32>() {
                    self.power_display_sleep_minutes = m.min(180);
                }
            }
            "auto_lock_minutes" => {
                if let Ok(m) = val.parse::<u32>() {
                    self.power_auto_lock_minutes = m.min(120);
                }
            }
            "dim_on_battery" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.power_dim_on_battery = b;
                }
            }
            _ => {}
        }
    }

    fn apply_ui_kv(&mut self, key: &str, val: &str) {
        match key {
            "theme" | "appearance" => {
                if let Some(mode) = AppearanceMode::parse(val) {
                    self.appearance = mode;
                }
            }
            "accent_index" => {
                if let Ok(idx) = val.parse::<usize>() {
                    let clamped = rusty_services::file_manager::clamp_accent_index(idx);
                    self.ui_accent_index = u8::try_from(clamped).unwrap_or(0);
                }
            }
            "wallpaper" => {
                if let Some(s) = parse_toml_string(val) {
                    self.wallpaper = expand_user_path(&s);
                }
            }
            "cursor_theme" => {
                if let Some(s) = parse_toml_string(val) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        self.cursor_theme = Some(trimmed.to_owned());
                    }
                }
            }
            "glass_opacity" => {
                if let Ok(v) = val.parse::<f32>() {
                    self.glass_opacity = clamp_glass_opacity(v);
                }
            }
            "reduced_motion"
            | "reduce_transparency"
            | "menubar_show_workspaces"
            | "menubar_show_tray"
            | "menubar_show_input_source"
            | "menubar_show_volume"
            | "clock_24h" => {
                if let Ok(b) = val.parse::<bool>() {
                    let target = match key {
                        "reduced_motion" => &mut self.reduced_motion,
                        "reduce_transparency" => &mut self.reduce_transparency,
                        "menubar_show_workspaces" => &mut self.menubar_show_workspaces,
                        "menubar_show_tray" => &mut self.menubar_show_tray,
                        "menubar_show_input_source" => &mut self.menubar_show_input_source,
                        "menubar_show_volume" => &mut self.menubar_show_volume,
                        "clock_24h" => &mut self.clock_24h,
                        _ => return,
                    };
                    *target = b;
                }
            }
            "window_shadow" | "shadow" => {
                if let Some(mode) = WindowShadowMode::parse(val) {
                    self.window_shadow = mode;
                } else if let Some(s) = parse_toml_string(val)
                    && let Some(mode) = WindowShadowMode::parse(&s)
                {
                    self.window_shadow = mode;
                }
            }
            _ => {}
        }
    }

    /// Returns the configured `HiDPI` scale factor for `connector` (or `self.display_scale` fallback).
    #[must_use]
    pub fn scale_for_output(&self, connector: &str) -> f64 {
        lookup_per_output(&self.output_scales, connector).unwrap_or(self.display_scale)
    }

    /// Returns whether an explicit per-output scale override exists for `connector`.
    #[must_use]
    pub fn has_explicit_output_scale(&self, connector: &str) -> bool {
        lookup_per_output(&self.output_scales, connector).is_some()
    }

    /// Resolves the effective display rotation for `connector`, giving `cli_override` priority
    /// over per-connector overrides and `[display].rotation` (`RUSTYOS-055`).
    #[must_use]
    pub fn rotation_for_output(
        &self,
        connector: &str,
        cli_override: Option<DisplayRotation>,
    ) -> DisplayRotation {
        cli_override
            .or_else(|| lookup_per_output(&self.output_rotations, connector))
            .unwrap_or(self.display_rotation)
    }

    /// Builds a Smithay [`XkbConfig`] referencing the configured layout, variant, and options.
    #[must_use]
    pub fn xkb_config(&self) -> XkbConfig<'_> {
        let options =
            (!self.keyboard_options.trim().is_empty()).then(|| self.keyboard_options.clone());
        XkbConfig {
            rules: "",
            model: "",
            layout: &self.keyboard_layout,
            variant: &self.keyboard_variant,
            options,
        }
    }

    /// Serializes [`UserConfig`] to canonical TOML matching [`Self::parse_toml`] (`RUSTYOS-061`).
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn to_toml_string(&self) -> String {
        use std::collections::BTreeSet;
        use std::fmt::Write as _;

        let mut out = String::from("# RustyOS compositor configuration\n\n");
        let _ = writeln!(out, "[keyboard]");
        let _ = writeln!(
            out,
            "layout = \"{}\"",
            escape_toml_string(&self.keyboard_layout)
        );
        let _ = writeln!(
            out,
            "variant = \"{}\"",
            escape_toml_string(&self.keyboard_variant)
        );
        let _ = writeln!(
            out,
            "options = \"{}\"",
            escape_toml_string(&self.keyboard_options)
        );
        let _ = writeln!(out, "repeat_delay = {}", self.repeat_delay.clamp(50, 2000));
        let _ = writeln!(out, "repeat_rate = {}", self.repeat_rate.clamp(1, 120));

        let _ = writeln!(out, "\n[ui]");
        let _ = writeln!(out, "theme = \"{}\"", self.appearance.as_str());
        let _ = writeln!(out, "accent_index = {}", self.ui_accent_index);
        let _ = writeln!(out, "reduced_motion = {}", self.reduced_motion);
        let _ = writeln!(
            out,
            "glass_opacity = {}",
            clamp_glass_opacity(self.glass_opacity)
        );
        let _ = writeln!(out, "reduce_transparency = {}", self.reduce_transparency);
        let _ = writeln!(
            out,
            "menubar_show_workspaces = {}",
            self.menubar_show_workspaces
        );
        let _ = writeln!(out, "menubar_show_tray = {}", self.menubar_show_tray);
        let _ = writeln!(
            out,
            "menubar_show_input_source = {}",
            self.menubar_show_input_source
        );
        let _ = writeln!(out, "menubar_show_volume = {}", self.menubar_show_volume);
        let _ = writeln!(out, "clock_24h = {}", self.clock_24h);
        let _ = writeln!(out, "window_shadow = \"{}\"", self.window_shadow.as_str());
        if let Some(wallpaper) = self.wallpaper.as_ref() {
            let _ = writeln!(
                out,
                "wallpaper = \"{}\"",
                escape_toml_string(&wallpaper.to_string_lossy())
            );
        }
        if let Some(cursor_theme) = self.cursor_theme.as_deref() {
            let trimmed = cursor_theme.trim();
            if !trimmed.is_empty() {
                let _ = writeln!(out, "cursor_theme = \"{}\"", escape_toml_string(trimmed));
            }
        }

        let _ = writeln!(out, "\n[top_bar]");
        let _ = writeln!(out, "show_wifi = {}", self.menubar_show_wifi);
        let _ = writeln!(out, "show_battery = {}", self.menubar_show_battery);
        let _ = writeln!(out, "show_sound = {}", self.menubar_show_volume);
        let _ = writeln!(out, "show_bluetooth = {}", self.menubar_show_bluetooth);
        let _ = writeln!(
            out,
            "show_battery_percentage = {}",
            self.menubar_show_battery_percentage
        );
        let _ = writeln!(
            out,
            "adaptive_contrast = {}",
            self.menubar_adaptive_contrast
        );
        let _ = writeln!(out, "clock_24h = {}", self.clock_24h);
        let _ = writeln!(
            out,
            "clock_locale = \"{}\"",
            escape_toml_string(&self.clock_locale)
        );

        let _ = writeln!(out, "\n[display]");
        let _ = writeln!(out, "prefer_max_refresh = {}", self.prefer_max_refresh);
        let _ = writeln!(out, "vrr_mode = \"{}\"", self.vrr_mode.as_str());
        let _ = writeln!(out, "scale = {}", clamp_display_scale(self.display_scale));
        let _ = writeln!(out, "rotation = \"{}\"", self.display_rotation.as_str());

        let connectors: BTreeSet<&str> = self
            .output_scales
            .keys()
            .chain(self.output_rotations.keys())
            .map(String::as_str)
            .collect();
        for connector in connectors {
            let _ = writeln!(
                out,
                "\n[display.outputs.\"{}\"]",
                escape_toml_string(connector)
            );
            if let Some(scale) = self.output_scales.get(connector) {
                let _ = writeln!(out, "scale = {}", clamp_display_scale(*scale));
            }
            if let Some(rot) = self.output_rotations.get(connector) {
                let _ = writeln!(out, "rotation = \"{}\"", rot.as_str());
            }
        }

        self.append_security_toml(&mut out);
        self.append_dock_toml(&mut out);
        self.append_updates_toml(&mut out);
        self.append_notifications_toml(&mut out);
        self.append_power_toml(&mut out);
        self.shortcuts.append_toml(&mut out);

        let _ = writeln!(out, "\n[autostart]");
        let escaped_cmds: Vec<String> = self
            .autostart
            .iter()
            .map(|cmd| format!("\"{}\"", escape_toml_string(cmd)))
            .collect();
        let _ = writeln!(out, "commands = [{}]", escaped_cmds.join(", "));

        out
    }

    fn append_security_toml(&self, out: &mut String) {
        use std::fmt::Write as _;

        let _ = writeln!(out, "\n[security]");
        let _ = writeln!(
            out,
            "require_password_on_lock = {}",
            self.require_password_on_lock
        );
        let _ = writeln!(out, "lock_on_sleep = {}", self.lock_on_sleep);
        let _ = writeln!(out, "enable_polkit_agent = {}", self.enable_polkit_agent);
    }

    fn append_updates_toml(&self, out: &mut String) {
        use std::fmt::Write as _;

        let _ = writeln!(out, "\n[updates]");
        let _ = writeln!(out, "enabled = {}", self.updates_enabled);
        let _ = writeln!(
            out,
            "check_interval_minutes = {}",
            clamp_updates_interval_minutes(self.updates_interval_minutes)
        );
        let _ = writeln!(out, "show_in_menu_bar = {}", self.updates_show_in_menu_bar);
    }

    fn append_notifications_toml(&self, out: &mut String) {
        use std::fmt::Write as _;

        let _ = writeln!(out, "\n[notifications]");
        let _ = writeln!(out, "allow = {}", self.notifications_allow);
        let _ = writeln!(
            out,
            "show_previews = \"{}\"",
            self.notifications_show_previews.as_str()
        );
        let _ = writeln!(out, "when_sleeping = {}", self.notifications_when_sleeping);
        let _ = writeln!(out, "when_locked = {}", self.notifications_when_locked);
        let _ = writeln!(out, "mirroring_off = {}", self.notifications_mirroring_off);
    }

    fn append_power_toml(&self, out: &mut String) {
        use std::fmt::Write as _;

        let _ = writeln!(out, "\n[power]");
        let _ = writeln!(
            out,
            "low_power_mode = \"{}\"",
            self.power_low_power_mode.as_str()
        );
        let _ = writeln!(
            out,
            "display_sleep_minutes = {}",
            self.power_display_sleep_minutes.min(180)
        );
        let _ = writeln!(
            out,
            "auto_lock_minutes = {}",
            self.power_auto_lock_minutes.min(120)
        );
        let _ = writeln!(out, "dim_on_battery = {}", self.power_dim_on_battery);
    }

    fn append_dock_toml(&self, out: &mut String) {
        use std::fmt::Write as _;

        let _ = writeln!(out, "\n[dock]");
        let _ = writeln!(
            out,
            "icon_size = {}",
            clamp_dock_icon_size(self.dock_icon_size)
        );
        let _ = writeln!(out, "position = \"{}\"", self.dock_position.as_str());
        let _ = writeln!(
            out,
            "minimize_effect = \"{}\"",
            self.dock_minimize_effect.as_str()
        );
        let _ = writeln!(out, "minimize_into_app = {}", self.dock_minimize_into_app);
        let _ = writeln!(out, "auto_hide = {}", self.dock_auto_hide);
        let _ = writeln!(out, "animate_opening = {}", self.dock_animate_opening);
        let _ = writeln!(out, "show_indicators = {}", self.dock_show_indicators);
        let _ = writeln!(out, "show_recent = {}", self.dock_show_recent);
        let escaped_pins: Vec<String> = self
            .dock_pinned_apps
            .iter()
            .map(|pin| format!("\"{}\"", escape_toml_string(pin)))
            .collect();
        let _ = writeln!(out, "pinned_apps = [{}]", escaped_pins.join(", "));
        let _ = writeln!(
            out,
            "downloads_display = \"{}\"",
            self.dock_downloads_display.as_str()
        );
        let _ = writeln!(
            out,
            "downloads_view = \"{}\"",
            self.dock_downloads_view.as_str()
        );
        let _ = writeln!(out, "downloads_sort = \"{}\"", self.downloads_sort.as_str());
    }

    /// Atomically persists [`UserConfig`] to `path` via a sibling temp file + rename (`RUSTYOS-061`).
    ///
    /// # Errors
    ///
    /// Returns an I/O error if creating the parent directory, writing the temporary file,
    /// or renaming it into `path` fails.
    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        let dir = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                fs::create_dir_all(parent)?;
                parent
            }
            _ => Path::new("."),
        };
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.toml");
        let tmp_path = dir.join(format!(".{file_name}.tmp.{}", std::process::id()));
        fs::write(&tmp_path, self.to_toml_string())?;
        if let Err(err) = fs::rename(&tmp_path, path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(err);
        }
        Ok(())
    }
}

fn escape_toml_string(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' | '\r' => escaped.push(' '),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Resolves the canonical path `~/.config/rustyos/config.toml` (honoring `XDG_CONFIG_HOME`).
#[must_use]
pub fn default_user_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        return Some(PathBuf::from(xdg).join("rustyos").join("config.toml"));
    }
    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("rustyos")
            .join("config.toml")
    })
}

/// Resolves the default user custom wallpaper `.png` path (`~/.config/rustyos/wallpaper.png`).
#[must_use]
pub fn default_user_wallpaper_path() -> Option<PathBuf> {
    default_user_config_path().and_then(|cfg| cfg.parent().map(|dir| dir.join("wallpaper.png")))
}

fn expand_user_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return Some(PathBuf::from(home).join(rest));
        }
    } else if trimmed == "~"
        && let Some(home) = env::var_os("HOME")
    {
        return Some(PathBuf::from(home));
    }
    Some(PathBuf::from(trimmed))
}

/// Formats a compact 2-letter uppercase `TopBar` indicator (`"EN"`, `"RU"`, etc.) from XKB layout info.
#[must_use]
pub fn layout_short_code(
    xkb_layout_name: &str,
    active_index: usize,
    configured_layouts: &str,
) -> String {
    let lower = xkb_layout_name.to_ascii_lowercase();
    if lower.contains("english") || lower.starts_with("us") {
        return "EN".to_owned();
    }
    if lower.contains("russian") || lower.starts_with("ru") {
        return "RU".to_owned();
    }
    if lower.contains("ukrain") || lower.starts_with("ua") {
        return "UA".to_owned();
    }
    if lower.contains("german") || lower.starts_with("de") {
        return "DE".to_owned();
    }
    if lower.contains("french") || lower.starts_with("fr") {
        return "FR".to_owned();
    }

    if let Some(code) = configured_layouts.split(',').nth(active_index) {
        let cleaned: String = code
            .trim()
            .chars()
            .filter(char::is_ascii_alphabetic)
            .take(2)
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if cleaned == "US" {
            return "EN".to_owned();
        }
        if !cleaned.is_empty() {
            return cleaned;
        }
    }

    let fallback: String = xkb_layout_name
        .chars()
        .filter(char::is_ascii_alphabetic)
        .take(2)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if fallback.is_empty() {
        "EN".to_owned()
    } else {
        fallback
    }
}

/// Curated multi-layout XKB presets exposed in `System Settings -> Keyboard` (`RUSTYOS-062`).
pub const KEYBOARD_LAYOUT_PRESETS: [&str; 6] = ["us", "us,ru", "us,ua", "us,de", "us,fr", "us,es"];

/// Curated XKB group-switch shortcuts `(xkb_option, human_label)` for `System Settings -> Keyboard` (`RUSTYOS-062`).
pub const KEYBOARD_OPTION_PRESETS: [(&str, &str); 5] = [
    ("grp:alt_shift_toggle", "Alt+Shift"),
    ("grp:caps_toggle", "Caps Lock"),
    ("grp:ctrl_shift_toggle", "Ctrl+Shift"),
    ("grp:win_space_toggle", "Win+Space"),
    ("", "Off"),
];

/// Returns the next curated `[keyboard] layout` preset after `current` (`RUSTYOS-062`).
#[must_use]
pub fn next_keyboard_layout_preset(current: &str) -> &'static str {
    let normalized = current.trim();
    match KEYBOARD_LAYOUT_PRESETS
        .iter()
        .position(|preset| preset.eq_ignore_ascii_case(normalized))
    {
        Some(idx) => KEYBOARD_LAYOUT_PRESETS[(idx + 1) % KEYBOARD_LAYOUT_PRESETS.len()],
        None => KEYBOARD_LAYOUT_PRESETS[1],
    }
}

/// Returns the next curated `[keyboard] options` group-switch preset after `current` (`RUSTYOS-062`).
#[must_use]
pub fn next_keyboard_option_preset(current: &str) -> &'static str {
    let normalized = current.trim();
    match KEYBOARD_OPTION_PRESETS
        .iter()
        .position(|(opt, _)| opt.eq_ignore_ascii_case(normalized))
    {
        Some(idx) => KEYBOARD_OPTION_PRESETS[(idx + 1) % KEYBOARD_OPTION_PRESETS.len()].0,
        None => KEYBOARD_OPTION_PRESETS[0].0,
    }
}

/// Maps an XKB `options` string to a concise button label for `System Settings` (`RUSTYOS-062`).
#[must_use]
pub fn keyboard_option_label(options: &str) -> &'static str {
    let normalized = options.trim();
    KEYBOARD_OPTION_PRESETS
        .iter()
        .find(|(opt, _)| opt.eq_ignore_ascii_case(normalized))
        .map_or("Custom", |(_, label)| *label)
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_double && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '#' && !in_double && !in_single {
            return &line[..idx];
        }
    }
    line
}

fn parse_toml_string(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len() - 1];
        return Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return Some(trimmed[1..trimmed.len() - 1].to_owned());
    }
    None
}

fn parse_toml_string_array(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or("");
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for ch in inner.chars() {
        if let Some(q) = quote {
            if escaped {
                current.push(ch);
                escaped = false;
            } else if q == '"' && ch == '\\' {
                escaped = true;
            } else if ch == q {
                if !current.trim().is_empty() {
                    out.push(current.clone());
                }
                current.clear();
                quote = None;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
            current.clear();
        }
    }

    out
}

fn strip_toml_quotes(raw: &str) -> &str {
    raw.trim().trim_matches('"').trim_matches('\'').trim()
}

fn lookup_per_output<T: Copy>(map: &BTreeMap<String, T>, connector: &str) -> Option<T> {
    map.get(connector).copied().or_else(|| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(connector))
            .map(|(_, v)| *v)
    })
}

fn parse_toml_inline_table<T>(
    raw: &str,
    mut parse_val: impl FnMut(&str) -> Option<T>,
) -> Vec<(String, T)> {
    let trimmed = raw.trim();
    let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for pair in inner.split(',') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let key = strip_toml_quotes(k);
        if !key.is_empty()
            && let Some(parsed) = parse_val(v.trim())
        {
            entries.push((key.to_owned(), parsed));
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_toml_with_comments_and_multiline_autostart() {
        let raw = r#"
            # RustyOS desktop preferences
            [keyboard]
            layout = "us,ru,de" # multi-layout
            variant = ",winkeys,"
            options = "grp:win_space_toggle"
            repeat_delay = 280
            repeat_rate = 40

            [ui]
            reduced_motion = true
            wallpaper = "/usr/share/backgrounds/rustyos.png"
            cursor_theme = "Adwaita"

            [autostart]
            commands = [
                "konsole",
                "nm-applet --indicator",
            ]
        "#;

        let cfg = UserConfig::parse_toml(raw);
        assert_eq!(cfg.keyboard_layout, "us,ru,de");
        assert_eq!(cfg.keyboard_variant, ",winkeys,");
        assert_eq!(cfg.keyboard_options, "grp:win_space_toggle");
        assert_eq!(cfg.repeat_delay, 280);
        assert_eq!(cfg.repeat_rate, 40);
        assert!(cfg.reduced_motion);
        assert_eq!(
            cfg.wallpaper,
            Some(PathBuf::from("/usr/share/backgrounds/rustyos.png"))
        );
        assert_eq!(cfg.cursor_theme.as_deref(), Some("Adwaita"));
        assert_eq!(cfg.autostart, vec!["konsole", "nm-applet --indicator"]);
    }

    #[test]
    fn missing_or_out_of_range_values_fall_back_safely() {
        let raw = r"
            [keyboard]
            repeat_delay = 5
            repeat_rate = 999
        ";
        let cfg = UserConfig::parse_toml(raw);
        assert_eq!(cfg.keyboard_layout, "us,ru");
        assert_eq!(cfg.keyboard_options, "grp:alt_shift_toggle");
        assert_eq!(cfg.repeat_delay, 50);
        assert_eq!(cfg.repeat_rate, 120);
        assert!(!cfg.reduced_motion);
        assert_eq!(cfg.wallpaper, None);
        assert_eq!(cfg.cursor_theme, None);
        assert_eq!(cfg.autostart, Vec::<String>::new());

        let legacy_dock = UserConfig::parse_toml(
            r"
            [dock]
            icon_size = 56
            magnification_size = 76
            ",
        );
        assert_eq!(legacy_dock.dock_icon_size, 56);
    }

    #[test]
    fn layout_short_code_maps_xkb_names_and_configured_codes() {
        assert_eq!(layout_short_code("English (US)", 0, "us,ru"), "EN");
        assert_eq!(layout_short_code("Russian", 1, "us,ru"), "RU");
        assert_eq!(layout_short_code("Unknown Layout", 1, "us,pl"), "PL");
    }

    #[test]
    fn parses_display_vrr_and_refresh_preferences() {
        let defaults = UserConfig::default();
        assert!(defaults.prefer_max_refresh);
        assert_eq!(defaults.vrr_mode, VrrMode::Fullscreen);

        let cfg = UserConfig::parse_toml(
            r#"
            [display]
            prefer_max_refresh = false
            vrr = "always"
            "#,
        );
        assert!(!cfg.prefer_max_refresh);
        assert_eq!(cfg.vrr_mode, VrrMode::Always);

        let cfg_off = UserConfig::parse_toml(
            r#"
            [display]
            vrr_mode = "off"
            "#,
        );
        assert_eq!(cfg_off.vrr_mode, VrrMode::Off);
    }

    #[test]
    fn parses_display_fractional_and_per_output_scales() {
        let cfg = UserConfig::parse_toml(
            r#"
            [display]
            scale = 1.25
            scales = { "DP-1" = 2.0, "HDMI-A-1" = 8.0 }

            [display.scales]
            "eDP-1" = 1.5
            dp-2 = 0.25
            "#,
        );
        assert!((cfg.display_scale - 1.25).abs() < 1e-6);
        assert!((cfg.scale_for_output("DP-1") - 2.0).abs() < 1e-6);
        assert!((cfg.scale_for_output("HDMI-A-1") - 4.0).abs() < 1e-6);
        assert!((cfg.scale_for_output("eDP-1") - 1.5).abs() < 1e-6);
        assert!((cfg.scale_for_output("DP-2") - 0.5).abs() < 1e-6);
        assert!((cfg.scale_for_output("DP-9") - 1.25).abs() < 1e-6);
        assert!(cfg.has_explicit_output_scale("eDP-1"));
        assert!(!cfg.has_explicit_output_scale("DP-9"));
    }

    #[test]
    fn parses_ui_glass_opacity_and_reduce_transparency() {
        let defaults = UserConfig::default();
        assert!((defaults.glass_opacity - DEFAULT_GLASS_OPACITY).abs() < 1e-6);
        assert!(!defaults.reduce_transparency);
        assert!(!defaults.menubar_show_workspaces);
        assert!(!defaults.menubar_show_tray);
        assert!(!defaults.menubar_show_input_source);
        assert!(!defaults.menubar_show_volume);
        assert!(!defaults.clock_24h);
        assert_eq!(defaults.window_shadow, WindowShadowMode::Subtle);

        let cfg = UserConfig::parse_toml(
            r#"
            [ui]
            glass_opacity = 0.62
            reduce_transparency = true
            menubar_show_workspaces = true
            menubar_show_tray = true
            menubar_show_input_source = true
            menubar_show_volume = true
            clock_24h = true
            window_shadow = "none"
            "#,
        );
        assert!((cfg.glass_opacity - 0.62).abs() < 1e-6);
        assert!(cfg.reduce_transparency);
        assert!(cfg.menubar_show_workspaces);
        assert!(cfg.menubar_show_tray);
        assert!(cfg.menubar_show_input_source);
        assert!(cfg.menubar_show_volume);
        assert!(cfg.clock_24h);
        assert_eq!(cfg.window_shadow, WindowShadowMode::None);

        let normal_cfg = UserConfig::parse_toml("[ui]\nwindow_shadow = \"normal\"\n");
        assert_eq!(normal_cfg.window_shadow, WindowShadowMode::Normal);
        let bool_off = UserConfig::parse_toml("[ui]\nwindow_shadow = false\n");
        assert_eq!(bool_off.window_shadow, WindowShadowMode::None);
        let bool_on = UserConfig::parse_toml("[ui]\nwindow_shadow = true\n");
        assert_eq!(bool_on.window_shadow, WindowShadowMode::Subtle);

        let clamped_low = UserConfig::parse_toml("[ui]\nglass_opacity = 0.05\n");
        assert!((clamped_low.glass_opacity - MIN_GLASS_OPACITY).abs() < 1e-6);

        let clamped_high = UserConfig::parse_toml("[ui]\nglass_opacity = 4.5\n");
        assert!((clamped_high.glass_opacity - 1.0).abs() < 1e-6);
        assert!((clamp_glass_opacity(f32::NAN) - DEFAULT_GLASS_OPACITY).abs() < 1e-6);
    }

    #[test]
    fn parses_display_rotations_and_per_output_overrides() {
        assert_eq!(DisplayRotation::parse("0"), Some(DisplayRotation::Rotate0));
        assert_eq!(
            DisplayRotation::parse("normal"),
            Some(DisplayRotation::Rotate0)
        );
        assert_eq!(
            DisplayRotation::parse("90deg"),
            Some(DisplayRotation::Rotate90)
        );
        assert_eq!(
            DisplayRotation::parse("inverted"),
            Some(DisplayRotation::Rotate180)
        );
        assert_eq!(
            DisplayRotation::parse("\"270\""),
            Some(DisplayRotation::Rotate270)
        );
        assert_eq!(DisplayRotation::parse("45"), None);

        let cfg = UserConfig::parse_toml(
            r#"
            [display]
            rotation = 90
            rotations = { "HDMI-A-1" = 270 }

            [display.rotations]
            "DP-1" = "inverted"

            [display.outputs."eDP-1"]
            rotation = 180
            scale = 1.5
            "#,
        );
        assert_eq!(cfg.display_rotation, DisplayRotation::Rotate90);
        assert_eq!(
            cfg.rotation_for_output("HDMI-A-1", None),
            DisplayRotation::Rotate270
        );
        assert_eq!(
            cfg.rotation_for_output("dp-1", None),
            DisplayRotation::Rotate180
        );
        assert_eq!(
            cfg.rotation_for_output("eDP-1", None),
            DisplayRotation::Rotate180
        );
        assert!((cfg.scale_for_output("eDP-1") - 1.5).abs() < 1e-6);
        assert_eq!(
            cfg.rotation_for_output("DP-9", None),
            DisplayRotation::Rotate90
        );
        assert_eq!(
            cfg.rotation_for_output("eDP-1", Some(DisplayRotation::Rotate0)),
            DisplayRotation::Rotate0
        );
        assert_eq!(
            DisplayRotation::Rotate180.to_wl_transform(),
            Transform::_180
        );
        assert_eq!(
            DisplayRotation::Rotate180.to_winit_transform(),
            Transform::Flipped
        );
        assert_eq!(
            DisplayRotation::Rotate0.to_winit_transform(),
            Transform::Flipped180
        );
    }

    #[test]
    fn parses_security_section_preferences() {
        let defaults = UserConfig::default();
        assert!(defaults.require_password_on_lock);
        assert!(defaults.lock_on_sleep);
        assert!(defaults.enable_polkit_agent);

        let cfg = UserConfig::parse_toml(
            r"
            [security]
            require_password_on_lock = false
            lock_on_sleep = false
            enable_polkit_agent = false
            ",
        );
        assert!(!cfg.require_password_on_lock);
        assert!(!cfg.lock_on_sleep);
        assert!(!cfg.enable_polkit_agent);
    }

    #[test]
    fn toml_serialization_and_atomic_save_roundtrip_all_sections()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut cfg = UserConfig {
            keyboard_layout: "us,ua".to_owned(),
            keyboard_variant: ",phonetic".to_owned(),
            keyboard_options: "grp:caps_toggle".to_owned(),
            repeat_delay: 280,
            repeat_rate: 42,
            appearance: AppearanceMode::Dark,
            ui_accent_index: 3,
            reduced_motion: true,
            glass_opacity: 0.62,
            reduce_transparency: true,
            menubar_show_workspaces: true,
            menubar_show_tray: true,
            menubar_show_input_source: true,
            menubar_show_volume: true,
            clock_24h: true,
            prefer_max_refresh: false,
            vrr_mode: VrrMode::Always,
            display_scale: 1.5,
            display_rotation: DisplayRotation::Rotate90,
            wallpaper: Some(PathBuf::from("/usr/share/wallpapers/rustyos.png")),
            cursor_theme: Some("Adwaita".to_owned()),
            autostart: vec![
                "foot --server".to_owned(),
                "nm-applet \"quoted\"".to_owned(),
            ],
            require_password_on_lock: false,
            lock_on_sleep: false,
            enable_polkit_agent: false,
            dock_icon_size: 56,
            dock_position: DockPosition::Left,
            dock_minimize_effect: DockMinimizeEffect::Scale,
            dock_minimize_into_app: true,
            dock_auto_hide: true,
            dock_animate_opening: false,
            dock_show_indicators: false,
            dock_show_recent: false,
            dock_pinned_apps: vec![
                "files".to_owned(),
                "overview".to_owned(),
                "firefox".to_owned(),
                "terminal".to_owned(),
            ],
            dock_downloads_display: DownloadsDisplayMode::Folder,
            dock_downloads_view: DownloadsViewMode::Grid,
            downloads_sort: DownloadsSortMode::Kind,
            updates_enabled: false,
            updates_interval_minutes: 360,
            updates_show_in_menu_bar: false,
            ..UserConfig::default()
        };
        cfg.output_scales.insert("eDP-1".to_owned(), 1.75);
        cfg.output_scales.insert("DP-1".to_owned(), 2.0);
        cfg.output_rotations
            .insert("DP-1".to_owned(), DisplayRotation::Rotate270);

        let serialized = cfg.to_toml_string();
        let reparsed = UserConfig::parse_toml(&serialized);
        assert_eq!(reparsed, cfg);

        let temp_dir = std::env::temp_dir().join(format!(
            "rustyos-config-test-{}-{}",
            std::process::id(),
            reparsed.repeat_delay
        ));
        let path = temp_dir.join("nested/config.toml");
        cfg.save_to_path(&path)?;
        let reloaded = UserConfig::load_from_path(&path);
        assert_eq!(reloaded, cfg);
        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn keyboard_preset_cycling_and_labels_cover_builtin_and_custom_values() {
        assert_eq!(next_keyboard_layout_preset("us"), "us,ru");
        assert_eq!(next_keyboard_layout_preset("us,es"), "us");
        assert_eq!(next_keyboard_layout_preset("us,jp"), "us,ru");

        assert_eq!(
            next_keyboard_option_preset("grp:alt_shift_toggle"),
            "grp:caps_toggle"
        );
        assert_eq!(next_keyboard_option_preset(""), "grp:alt_shift_toggle");
        assert_eq!(keyboard_option_label("grp:ctrl_shift_toggle"), "Ctrl+Shift");
        assert_eq!(keyboard_option_label(""), "Off");
        assert_eq!(keyboard_option_label("lv3:ralt_switch"), "Custom");
        assert!(default_user_wallpaper_path().is_none_or(|p| p.ends_with("rustyos/wallpaper.png")));
    }

    #[test]
    fn parses_updates_section_and_clamps_interval() {
        let defaults = UserConfig::default();
        assert!(defaults.updates_enabled);
        assert_eq!(
            defaults.updates_interval_minutes,
            DEFAULT_UPDATES_INTERVAL_MINUTES
        );
        assert!(defaults.updates_show_in_menu_bar);

        let low = UserConfig::parse_toml(
            "[updates]\nenabled = false\ncheck_interval_minutes = 2\nshow_in_menu_bar = false\n",
        );
        assert!(!low.updates_enabled);
        assert_eq!(low.updates_interval_minutes, MIN_UPDATES_INTERVAL_MINUTES);
        assert!(!low.updates_show_in_menu_bar);

        let high = UserConfig::parse_toml("[updates]\ncheck_interval_minutes = 999999\n");
        assert_eq!(high.updates_interval_minutes, MAX_UPDATES_INTERVAL_MINUTES);
    }

    #[test]
    fn parses_notifications_section_and_roundtrips() {
        let defaults = UserConfig::default();
        assert!(defaults.notifications_allow);
        assert_eq!(
            defaults.notifications_show_previews,
            NotificationPreviewsMode::WhenUnlocked
        );
        assert!(!defaults.notifications_when_sleeping);
        assert!(defaults.notifications_when_locked);
        assert!(defaults.notifications_mirroring_off);

        let parsed = UserConfig::parse_toml(
            "[notifications]\nallow = false\nshow_previews = \"never\"\nwhen_sleeping = true\nwhen_locked = false\nmirroring_off = false\n",
        );
        assert!(!parsed.notifications_allow);
        assert_eq!(
            parsed.notifications_show_previews,
            NotificationPreviewsMode::Never
        );
        assert!(parsed.notifications_when_sleeping);
        assert!(!parsed.notifications_when_locked);
        assert!(!parsed.notifications_mirroring_off);

        let reparsed = UserConfig::parse_toml(&parsed.to_toml_string());
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn parses_power_and_shortcuts_sections_and_roundtrips() {
        let raw = r#"
            [power]
            low_power_mode = "battery"
            display_sleep_minutes = 15
            auto_lock_minutes = 5
            dim_on_battery = false

            [shortcuts]
            launcher = "Ctrl+Space"
            close_window = "Super+Q"
            overview = "Alt+Tab"
        "#;
        let parsed = UserConfig::parse_toml(raw);
        assert_eq!(parsed.power_low_power_mode, LowPowerMode::BatteryOnly);
        assert_eq!(parsed.power_display_sleep_minutes, 15);
        assert_eq!(parsed.power_auto_lock_minutes, 5);
        assert!(!parsed.power_dim_on_battery);
        assert_eq!(parsed.shortcuts.launcher, "Ctrl+Space");
        assert_eq!(parsed.shortcuts.close_window, "Super+Q");
        assert_eq!(parsed.shortcuts.overview, "Alt+Tab");

        let reparsed = UserConfig::parse_toml(&parsed.to_toml_string());
        assert_eq!(reparsed, parsed);

        assert_eq!(
            LowPowerMode::BatteryOnly.desired_profile(false, true),
            PowerProfile::LowPower
        );
        assert_eq!(
            LowPowerMode::BatteryOnly.desired_profile(true, true),
            PowerProfile::Balanced
        );
        assert_eq!(
            LowPowerMode::Never.desired_profile(false, true),
            PowerProfile::Balanced
        );
    }
}
