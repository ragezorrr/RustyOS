//! User configuration loader (`~/.config/rustyos/config.toml`) and keyboard layout helpers (`RUSTYOS-029`).
//!
//! Parses a lightweight, dependency-free TOML subset with `[keyboard]`, `[ui]`, and `[autostart]`
//! sections and provides safe defaults (`us,ru` + `grp:alt_shift_toggle`) when the config file is
//! absent or partially specified.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use smithay::input::keyboard::XkbConfig;
use tracing::{info, warn};

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

/// Persistent user preferences loaded from `~/.config/rustyos/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Prefer the highest supported refresh rate (60/120/144/240 Hz) at the display's native
    /// resolution on DRM/KMS outputs (`RUSTYOS-042`).
    pub prefer_max_refresh: bool,
    /// Variable Refresh Rate (`VRR`) runtime policy on DRM/KMS (`RUSTYOS-042`).
    pub vrr_mode: VrrMode,
    /// Optional custom desktop wallpaper `.png` path (`~/` expanded).
    pub wallpaper: Option<PathBuf>,
    /// Optional `XCursor` theme override (falls back to `XCURSOR_THEME` and system themes).
    pub cursor_theme: Option<String>,
    /// Commands launched automatically once the Wayland socket is ready.
    pub autostart: Vec<String>,
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
            prefer_max_refresh: true,
            vrr_mode: VrrMode::Fullscreen,
            wallpaper: None,
            cursor_theme: None,
            autostart: Vec::new(),
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
            ("ui", "reduced_motion") => {
                if let Ok(b) = val.parse::<bool>() {
                    self.reduced_motion = b;
                }
            }
            ("display", "prefer_max_refresh") => {
                if let Ok(b) = val.parse::<bool>() {
                    self.prefer_max_refresh = b;
                }
            }
            ("display", "vrr" | "vrr_mode") => {
                if let Some(s) = parse_toml_string(val)
                    && let Some(mode) = VrrMode::parse(&s)
                {
                    self.vrr_mode = mode;
                }
            }
            ("ui", "wallpaper") => {
                if let Some(s) = parse_toml_string(val) {
                    self.wallpaper = expand_user_path(&s);
                }
            }
            ("ui", "cursor_theme") => {
                if let Some(s) = parse_toml_string(val) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        self.cursor_theme = Some(trimmed.to_owned());
                    }
                }
            }
            ("autostart", "commands") => {
                self.autostart = parse_toml_string_array(val);
            }
            _ => {}
        }
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
}
