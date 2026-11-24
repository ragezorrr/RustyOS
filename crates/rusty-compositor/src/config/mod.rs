//! User configuration loader (`~/.config/rustyos/config.toml`) and keyboard layout helpers (`RUSTYOS-029`).
//!
//! Parses a lightweight, dependency-free TOML subset with `[keyboard]`, `[ui]`, and `[autostart]`
//! sections and provides safe defaults (`us,ru` + `grp:alt_shift_toggle`) when the config file is
//! absent or partially specified.

mod shortcuts;
#[cfg(test)]
mod tests;
mod types;

pub use shortcuts::{ShortcutBindings, ShortcutCombo, parse_shortcut_combo};
pub use types::*;

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use smithay::input::keyboard::XkbConfig;
use tracing::{info, warn};

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
    /// Optional `XCursor` theme override (`"RustyOS"` uses built-in `@2x` SVG cursors).
    pub cursor_theme: Option<String>,
    /// Logical cursor size in pixels (`16..=64`, default `24`) (`RUSTYOS-151`).
    pub cursor_size: u32,
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
    /// Dock visibility policy (`AlwaysShow`, `SmartHide`, or `AlwaysHide`) (`RUSTYOS-161`).
    pub dock_visibility: DockVisibilityMode,
    /// Legacy alias for `dock_visibility == DockVisibilityMode::AlwaysHide` (`RUSTYOS-073`).
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
    /// Persisted completion state of the 4 Reminders checklist items (`RUSTYOS-156`).
    pub notifications_reminders_done: [bool; 4],
    /// Show the top Weather & System widget inside Notification Center (`RUSTYOS-156`).
    pub notifications_show_weather_widget: bool,
    /// Show the bottom Reminders checklist widget inside Notification Center (`RUSTYOS-156`).
    pub notifications_show_reminders_widget: bool,
    /// Lowercase canonical app keys whose popup toast banners are muted (`[notifications].muted_apps`, `RUSTYOS-170`).
    pub notifications_muted_apps: Vec<String>,
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
            cursor_size: 24,
            autostart: Vec::new(),
            require_password_on_lock: true,
            lock_on_sleep: true,
            enable_polkit_agent: true,
            dock_icon_size: DEFAULT_DOCK_ICON_SIZE,
            dock_position: DockPosition::Bottom,
            dock_minimize_effect: DockMinimizeEffect::Genie,
            dock_minimize_into_app: false,
            dock_visibility: DockVisibilityMode::AlwaysShow,
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
            notifications_reminders_done: [false; 4],
            notifications_show_weather_widget: true,
            notifications_show_reminders_widget: true,
            notifications_muted_apps: Vec::new(),
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
        let mut explicit_dock_visibility: Option<DockVisibilityMode> = None;

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

            if section == "dock"
                && key == "visibility"
                && let Some(vis) = DockVisibilityMode::parse(val)
            {
                explicit_dock_visibility = Some(vis);
            }
            cfg.apply_toml_kv(&section, key, val);
        }

        if let Some(vis) = explicit_dock_visibility {
            cfg.dock_visibility = vis;
            cfg.dock_auto_hide = matches!(vis, DockVisibilityMode::AlwaysHide);
        }

        cfg
    }

    /// Resolves effective Dock visibility, honoring legacy `dock_auto_hide = true` when
    /// `dock_visibility` was left at its default (`RUSTYOS-161`).
    #[must_use]
    pub const fn effective_dock_visibility(&self) -> DockVisibilityMode {
        match self.dock_visibility {
            DockVisibilityMode::AlwaysShow if self.dock_auto_hide => DockVisibilityMode::AlwaysHide,
            other => other,
        }
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
            "visibility" => {
                if let Some(vis) = DockVisibilityMode::parse(val) {
                    self.dock_visibility = vis;
                    self.dock_auto_hide = matches!(vis, DockVisibilityMode::AlwaysHide);
                }
            }
            "auto_hide" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.dock_auto_hide = b;
                    self.dock_visibility = if b {
                        DockVisibilityMode::AlwaysHide
                    } else {
                        DockVisibilityMode::AlwaysShow
                    };
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
            "reminders_done" => {
                let inner = val.trim().trim_start_matches('[').trim_end_matches(']');
                for (idx, part) in inner.split(',').take(4).enumerate() {
                    if let Ok(b) = part.trim().parse::<bool>() {
                        self.notifications_reminders_done[idx] = b;
                    }
                }
            }
            "show_weather_widget" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.notifications_show_weather_widget = b;
                }
            }
            "show_reminders_widget" => {
                if let Ok(b) = val.parse::<bool>() {
                    self.notifications_show_reminders_widget = b;
                }
            }
            "muted_apps" => {
                self.notifications_muted_apps = parse_toml_string_array(val)
                    .into_iter()
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            _ => {}
        }
    }

    /// Returns `true` when `app_key` is listed in `[notifications].muted_apps` (`RUSTYOS-170`).
    #[must_use]
    pub fn is_notification_app_muted(&self, app_key: &str) -> bool {
        let needle = app_key.trim();
        !needle.is_empty()
            && self
                .notifications_muted_apps
                .iter()
                .any(|m| m.eq_ignore_ascii_case(needle))
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
            "cursor_size" => {
                if let Ok(v) = val.parse::<u32>() {
                    self.cursor_size = v.clamp(16, 64);
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
        let _ = writeln!(out, "cursor_size = {}", self.cursor_size.clamp(16, 64));

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
        let _ = writeln!(
            out,
            "reminders_done = [{}, {}, {}, {}]",
            self.notifications_reminders_done[0],
            self.notifications_reminders_done[1],
            self.notifications_reminders_done[2],
            self.notifications_reminders_done[3]
        );
        let _ = writeln!(
            out,
            "show_weather_widget = {}",
            self.notifications_show_weather_widget
        );
        let _ = writeln!(
            out,
            "show_reminders_widget = {}",
            self.notifications_show_reminders_widget
        );
        let muted_list: Vec<String> = self
            .notifications_muted_apps
            .iter()
            .map(|s| format!("\"{}\"", escape_toml_string(s)))
            .collect();
        let _ = writeln!(out, "muted_apps = [{}]", muted_list.join(", "));
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
        let vis = self.effective_dock_visibility();
        let _ = writeln!(out, "minimize_into_app = {}", self.dock_minimize_into_app);
        let _ = writeln!(out, "visibility = \"{}\"", vis.as_str());
        let _ = writeln!(
            out,
            "auto_hide = {}",
            matches!(vis, DockVisibilityMode::AlwaysHide)
        );
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

    /// Atomically persists [`UserConfig`] to `path` under an advisory `flock` via a sibling temp file + `sync_all` + `rename` (`RUSTYOS-061`, `RUSTYOS-170`).
    ///
    /// # Errors
    ///
    /// Returns an I/O error if creating the parent directory, acquiring the lock, writing the
    /// temporary file, or renaming it into `path` fails.
    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        let payload = self.to_toml_string();
        rusty_services::with_config_file_lock(path, || {
            rusty_services::write_file_atomic(path, payload.as_bytes())
        })
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
