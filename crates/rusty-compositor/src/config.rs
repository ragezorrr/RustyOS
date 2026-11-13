//! User configuration loader (`~/.config/rustyos/config.toml`) and keyboard layout helpers (`RUSTYOS-029`).
//!
//! Parses a lightweight, dependency-free TOML subset with `[keyboard]`, `[ui]`, and `[autostart]`
//! sections and provides safe defaults (`us,ru` + `grp:alt_shift_toggle`) when the config file is
//! absent or partially specified.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use smithay::input::keyboard::XkbConfig;
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
    /// Format Menu Bar clock using 24-hour time (`RUSTYOS-049`, default `false` = macOS 12h AM/PM).
    pub clock_24h: bool,
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
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            keyboard_layout: "us,ru".to_owned(),
            keyboard_variant: String::new(),
            keyboard_options: "grp:alt_shift_toggle".to_owned(),
            repeat_delay: 200,
            repeat_rate: 25,
            reduced_motion: false,
            glass_opacity: DEFAULT_GLASS_OPACITY,
            reduce_transparency: false,
            menubar_show_workspaces: false,
            menubar_show_tray: false,
            menubar_show_input_source: false,
            menubar_show_volume: false,
            clock_24h: false,
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
                section = line[1..line.len() - 1].trim().to_ascii_lowercase();
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
            _ => {
                if let Some(raw_output) = section.strip_prefix("display.outputs.") {
                    self.apply_output_kv(strip_toml_quotes(raw_output), key, val);
                }
            }
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

    fn apply_ui_kv(&mut self, key: &str, val: &str) {
        match key {
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

        let cfg = UserConfig::parse_toml(
            r"
            [ui]
            glass_opacity = 0.62
            reduce_transparency = true
            menubar_show_workspaces = true
            menubar_show_tray = true
            menubar_show_input_source = true
            menubar_show_volume = true
            clock_24h = true
            ",
        );
        assert!((cfg.glass_opacity - 0.62).abs() < 1e-6);
        assert!(cfg.reduce_transparency);
        assert!(cfg.menubar_show_workspaces);
        assert!(cfg.menubar_show_tray);
        assert!(cfg.menubar_show_input_source);
        assert!(cfg.menubar_show_volume);
        assert!(cfg.clock_24h);

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
}
