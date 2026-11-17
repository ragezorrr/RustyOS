//! State model, TOML configuration synchronization, and IPC live-reload for `rusty-settings` (`RUSTYOS-103`).

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use rusty_services::{AudioService, NetworkService};

/// Predefined accent color palette.
pub(crate) const ACCENT_COLORS: [[u8; 4]; 7] = [
    [0x33, 0x85, 0xFF, 0xFF], // Blue
    [0xFF, 0x5C, 0x6D, 0xFF], // Coral Red
    [0x35, 0xBF, 0x56, 0xFF], // Green
    [0x82, 0x66, 0xFF, 0xFF], // Indigo
    [0xCA, 0x64, 0xAC, 0xFF], // Pink
    [0xFE, 0xA0, 0x42, 0xFF], // Orange
    [0x4F, 0x59, 0x6B, 0xFF], // Graphite
];

/// CLI startup overrides parsed by `main`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StartupOptions {
    pub(crate) theme_override: Option<ThemeChoice>,
    pub(crate) initial_page: SettingsPage,
}

/// Appearance mode tiles (`Light`, `Dark`, `Auto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ThemeChoice {
    #[default]
    Light,
    Dark,
    Auto,
}

/// Sidebar and pushed sub-pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsPage {
    Wifi,
    Network,
    #[default]
    Appearance,
    Wallpaper,
    DockDesktop,
    Notifications,
    Display,
    Sound,
    Keyboard,
    Mouse,
    Touchpad,
    Fonts,
    IconTheme,
    CursorStyle,
}

impl SettingsPage {
    pub(crate) const SIDEBAR: [Self; 11] = [
        Self::Wifi,
        Self::Network,
        Self::Appearance,
        Self::Wallpaper,
        Self::DockDesktop,
        Self::Notifications,
        Self::Display,
        Self::Sound,
        Self::Keyboard,
        Self::Mouse,
        Self::Touchpad,
    ];

    #[must_use]
    pub(crate) fn from_cli_id(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "wifi" | "wlan" => Self::Wifi,
            "network" => Self::Network,
            "wallpaper" | "background" => Self::Wallpaper,
            "dock" | "desktop" => Self::DockDesktop,
            "notifications" => Self::Notifications,
            "display" | "displays" => Self::Display,
            "sound" | "audio" => Self::Sound,
            "keyboard" => Self::Keyboard,
            "mouse" => Self::Mouse,
            "touchpad" | "trackpad" => Self::Touchpad,
            "fonts" => Self::Fonts,
            "icontheme" | "icons" => Self::IconTheme,
            "cursortheme" | "cursor" => Self::CursorStyle,
            _ => Self::Appearance,
        }
    }

    #[must_use]
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Network => "Network",
            Self::Appearance => "Appearance",
            Self::Wallpaper => "Wallpaper",
            Self::DockDesktop => "Dock & Desktop",
            Self::Notifications => "Notifications",
            Self::Display => "Display",
            Self::Sound => "Sound",
            Self::Keyboard => "Keyboard",
            Self::Mouse => "Mouse",
            Self::Touchpad => "Touchpad",
            Self::Fonts => "Fonts",
            Self::IconTheme => "Icon Theme",
            Self::CursorStyle => "Cursor Style",
        }
    }

    #[must_use]
    pub(crate) const fn category(self) -> Option<&'static str> {
        match self {
            Self::Wifi | Self::Network => Some("Network"),
            Self::Appearance | Self::Wallpaper | Self::DockDesktop | Self::Notifications => {
                Some("Desktop")
            }
            Self::Display | Self::Sound | Self::Keyboard | Self::Mouse | Self::Touchpad => {
                Some("Devices")
            }
            Self::Fonts | Self::IconTheme | Self::CursorStyle => None,
        }
    }

    #[must_use]
    pub(crate) const fn sidebar_root(self) -> Self {
        match self {
            Self::Fonts | Self::IconTheme | Self::CursorStyle => Self::Appearance,
            other => other,
        }
    }

    #[must_use]
    pub(crate) const fn icon_tint(self) -> [u8; 4] {
        match self {
            Self::Wifi | Self::Network => [0x00, 0x67, 0xFF, 0xFF],
            Self::Appearance => [0x03, 0xB4, 0xCB, 0xFF],
            Self::Wallpaper => [0x34, 0xB4, 0xA7, 0xFF],
            Self::DockDesktop => [0x85, 0x85, 0xFC, 0xFF],
            Self::Notifications | Self::Sound => [0xF1, 0x68, 0x84, 0xFF],
            Self::Display => [0x00, 0x87, 0xED, 0xFF],
            Self::Keyboard => [0x5E, 0x6A, 0xD2, 0xFF],
            Self::Mouse | Self::CursorStyle => [0x33, 0x85, 0xFF, 0xFF],
            Self::Touchpad => [0x99, 0x99, 0x99, 0xFF],
            Self::Fonts => [0xFE, 0x84, 0x33, 0xFF],
            Self::IconTheme => [0x2E, 0xC3, 0x47, 0xFF],
        }
    }

    #[must_use]
    pub(crate) const fn keywords(self) -> &'static str {
        match self {
            Self::Wifi => "wireless wlan network internet password",
            Self::Network => "vpn proxy ethernet tethering internet",
            Self::Appearance => "theme dark light auto accent color",
            Self::Wallpaper => "background picture desktop image wallpaper",
            Self::DockDesktop => "dock position size auto hide glass effect transparency motion",
            Self::Notifications => "alerts banners menubar clock workspaces tray",
            Self::Display => "monitor screen scale hidpi refresh rate vrr rotation",
            Self::Sound => "audio volume speakers mute microphone",
            Self::Keyboard => "input layout shortcut repeat rate delay",
            Self::Mouse => "pointer speed acceleration cursor",
            Self::Touchpad => "trackpad tap click natural scrolling",
            Self::Fonts => "font inter ui monospace size",
            Self::IconTheme => "icons system theme",
            Self::CursorStyle => "cursor pointer size theme",
        }
    }
}

/// Interactive setting action triggered inside a page card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowControlId {
    ToggleReduceTransparency,
    ToggleReducedMotion,
    GlassOpacityDown,
    GlassOpacityUp,
    ToggleWallpaper,
    DockPositionSet(u8),
    DockSizeDown,
    DockSizeUp,
    DockAutoHideToggle,
    DockIndicatorsToggle,
    DockRecentToggle,
    DockMinimizeIntoAppToggle,
    DockMinimizeEffectCycle,
    MenubarWorkspacesToggle,
    MenubarTrayToggle,
    MenubarInputToggle,
    MenubarVolumeToggle,
    Clock24hToggle,
    UpdatesEnabledToggle,
    ScaleDown,
    ScaleUp,
    PreferMaxRefreshToggle,
    VrrModeCycle,
    RotationCycle,
    KeyboardLayoutCycle,
    KeyboardOptionCycle,
    RepeatRateDown,
    RepeatRateUp,
    RepeatDelayDown,
    RepeatDelayUp,
    LockPasswordToggle,
    LockSleepToggle,
    PolkitToggle,
    VolumeDown,
    VolumeUp,
    ToggleMute,
    CursorThemeCycle,
}

/// Mutable state backing the System Settings window.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct SettingsAppState {
    pub(crate) running: bool,
    pub(crate) dirty: bool,
    pub(crate) maximized: bool,
    pub(crate) theme_choice: ThemeChoice,
    pub(crate) resolved_dark: bool,
    theme_locked: bool,
    last_config_mtime: Option<SystemTime>,
    pub(crate) accent_index: usize,
    pub(crate) current_page: SettingsPage,
    pub(crate) history: Vec<SettingsPage>,
    pub(crate) history_idx: usize,
    pub(crate) page_scroll_y: i32,
    pub(crate) sidebar_scroll_y: i32,
    pub(crate) search_focused: bool,
    pub(crate) search_query: String,
    pub(crate) caret_visible: bool,
    last_caret_blink: Instant,

    // Persistent config.toml preferences
    pub(crate) reduce_transparency: bool,
    pub(crate) reduced_motion: bool,
    pub(crate) glass_opacity: f32,
    pub(crate) custom_wallpaper: Option<String>,
    pub(crate) dock_icon_size: i32,
    pub(crate) dock_position: String,
    pub(crate) dock_minimize_effect: String,
    pub(crate) dock_minimize_into_app: bool,
    pub(crate) dock_auto_hide: bool,
    pub(crate) dock_show_indicators: bool,
    pub(crate) dock_show_recent: bool,
    pub(crate) menubar_workspaces: bool,
    pub(crate) menubar_tray: bool,
    pub(crate) menubar_input: bool,
    pub(crate) menubar_volume: bool,
    pub(crate) clock_24h: bool,
    pub(crate) updates_enabled: bool,
    pub(crate) display_scale: f64,
    pub(crate) prefer_max_refresh: bool,
    pub(crate) vrr_mode: String,
    pub(crate) display_rotation: String,
    pub(crate) keyboard_layout: String,
    pub(crate) keyboard_options: String,
    pub(crate) repeat_rate: i32,
    pub(crate) repeat_delay: i32,
    pub(crate) require_password_on_lock: bool,
    pub(crate) lock_on_sleep: bool,
    pub(crate) enable_polkit_agent: bool,
    pub(crate) cursor_theme: String,

    // Live hardware/service readouts
    pub(crate) network_status: String,
    pub(crate) volume_percent: u8,
    pub(crate) muted: bool,
}

impl SettingsAppState {
    #[must_use]
    pub(crate) fn new(opts: &StartupOptions) -> Self {
        let mut state = Self::defaults_with_page(opts.initial_page);
        state.load_from_disk();
        state.last_config_mtime = current_config_mtime();
        if let Some(theme_override) = opts.theme_override {
            state.theme_choice = theme_override;
            state.resolved_dark = matches!(theme_override, ThemeChoice::Dark);
            state.theme_locked = true;
        }
        state.refresh_services();
        state
    }

    fn defaults_with_page(initial_page: SettingsPage) -> Self {
        Self {
            running: true,
            dirty: true,
            maximized: false,
            theme_choice: ThemeChoice::Light,
            resolved_dark: false,
            theme_locked: false,
            last_config_mtime: None,
            accent_index: 0,
            current_page: initial_page,
            history: vec![initial_page],
            history_idx: 0,
            page_scroll_y: 0,
            sidebar_scroll_y: 0,
            search_focused: false,
            search_query: String::new(),
            caret_visible: true,
            last_caret_blink: Instant::now(),
            reduce_transparency: false,
            reduced_motion: false,
            glass_opacity: 1.0,
            custom_wallpaper: None,
            dock_icon_size: 48,
            dock_position: String::from("bottom"),
            dock_minimize_effect: String::from("genie"),
            dock_minimize_into_app: true,
            dock_auto_hide: false,
            dock_show_indicators: true,
            dock_show_recent: true,
            menubar_workspaces: false,
            menubar_tray: false,
            menubar_input: true,
            menubar_volume: false,
            clock_24h: true,
            updates_enabled: true,
            display_scale: 1.0,
            prefer_max_refresh: true,
            vrr_mode: String::from("fullscreen"),
            display_rotation: String::from("0"),
            keyboard_layout: String::from("us,ru"),
            keyboard_options: String::from("grp:alt_shift_toggle"),
            repeat_rate: 25,
            repeat_delay: 200,
            require_password_on_lock: true,
            lock_on_sleep: true,
            enable_polkit_agent: true,
            cursor_theme: String::from("Adwaita"),
            network_status: String::from("Checking..."),
            volume_percent: 65,
            muted: false,
        }
    }

    pub(crate) fn request_quit(&mut self) {
        self.running = false;
    }

    #[must_use]
    pub(crate) const fn can_go_back(&self) -> bool {
        self.history_idx > 0
    }

    #[must_use]
    pub(crate) fn can_go_forward(&self) -> bool {
        self.history_idx + 1 < self.history.len()
    }

    #[must_use]
    pub(crate) const fn is_dark_ui(&self) -> bool {
        self.resolved_dark
    }

    pub(crate) fn go_back(&mut self) {
        if self.can_go_back() {
            self.history_idx -= 1;
            self.current_page = self.history[self.history_idx];
            self.page_scroll_y = 0;
            self.dirty = true;
        }
    }

    pub(crate) fn go_forward(&mut self) {
        if self.can_go_forward() {
            self.history_idx += 1;
            self.current_page = self.history[self.history_idx];
            self.page_scroll_y = 0;
            self.dirty = true;
        }
    }

    pub(crate) fn navigate_to(&mut self, page: SettingsPage) {
        if self.current_page == page {
            return;
        }
        self.history.truncate(self.history_idx + 1);
        self.history.push(page);
        if self.history.len() > 50 {
            self.history.remove(0);
        }
        self.history_idx = self.history.len() - 1;
        self.current_page = page;
        self.page_scroll_y = 0;
        self.refresh_services();
        self.dirty = true;
    }

    pub(crate) fn scroll_by(
        &mut self,
        in_sidebar: bool,
        delta_y: i32,
        max_sidebar_scroll: i32,
        max_page_scroll: i32,
    ) -> bool {
        if delta_y == 0 {
            return false;
        }
        if in_sidebar && max_sidebar_scroll > 0 {
            let next = (self.sidebar_scroll_y + delta_y).clamp(0, max_sidebar_scroll);
            if next != self.sidebar_scroll_y {
                self.sidebar_scroll_y = next;
                self.dirty = true;
                return true;
            }
            return false;
        }
        if !in_sidebar && max_page_scroll > 0 {
            let next = (self.page_scroll_y + delta_y).clamp(0, max_page_scroll);
            if next != self.page_scroll_y {
                self.page_scroll_y = next;
                self.dirty = true;
                return true;
            }
        }
        false
    }

    #[must_use]
    pub(crate) fn visible_sidebar_pages(&self) -> Vec<SettingsPage> {
        let needle = self.search_query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return SettingsPage::SIDEBAR.to_vec();
        }
        SettingsPage::SIDEBAR
            .iter()
            .copied()
            .filter(|p| {
                p.title().to_ascii_lowercase().contains(&needle)
                    || p.category()
                        .is_some_and(|c| c.to_ascii_lowercase().contains(&needle))
                    || p.keywords().contains(&needle)
            })
            .collect()
    }

    pub(crate) fn refresh_services(&mut self) {
        let net = NetworkService::new().query_status();
        self.network_status = net.status_label;
        let audio = AudioService::new().query_status();
        self.volume_percent = audio.volume_percent.clamp(0, 100);
        self.muted = audio.muted;
    }

    pub(crate) fn choose_appearance(&mut self, choice: ThemeChoice) {
        self.theme_choice = choice;
        self.theme_locked = false;
        self.resolved_dark = match choice {
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
            ThemeChoice::Auto => system_auto_prefers_dark(),
        };
        let theme_val = if self.resolved_dark { "dark" } else { "light" };
        let mode_val = match choice {
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
            ThemeChoice::Auto => "auto",
        };
        let _ =
            update_config_entries(&[("ui", "theme", theme_val), ("ui", "theme_mode", mode_val)]);
        self.last_config_mtime = current_config_mtime();
        notify_compositor_reload();
        self.dirty = true;
    }

    pub(crate) fn choose_accent(&mut self, idx: usize) {
        let clamped = idx.min(ACCENT_COLORS.len() - 1);
        self.accent_index = clamped;
        let val = clamped.to_string();
        let _ = update_config_entries(&[("ui", "accent_index", &val)]);
        self.last_config_mtime = current_config_mtime();
        self.dirty = true;
    }

    pub(crate) fn update_caret_blink(&mut self, now: Instant) -> bool {
        if !self.search_focused {
            self.caret_visible = false;
            return false;
        }
        if now.saturating_duration_since(self.last_caret_blink) >= Duration::from_millis(530) {
            self.caret_visible = !self.caret_visible;
            self.last_caret_blink = now;
            self.dirty = true;
            return true;
        }
        false
    }

    #[must_use]
    pub(crate) fn handle_search_key(&mut self, sym: &str) -> bool {
        if !self.search_focused {
            return false;
        }
        match sym {
            "Escape" => {
                if self.search_query.is_empty() {
                    self.search_focused = false;
                } else {
                    self.search_query.clear();
                }
            }
            "BackSpace" => {
                self.search_query.pop();
            }
            "Return" => {
                if let Some(first) = self.visible_sidebar_pages().first().copied() {
                    self.navigate_to(first);
                }
            }
            other if other.len() == 1 => {
                if self.search_query.len() < 32 {
                    self.search_query.push_str(other);
                }
            }
            _ => return false,
        }
        self.caret_visible = true;
        self.sidebar_scroll_y = 0;
        self.last_caret_blink = Instant::now();
        self.dirty = true;
        true
    }

    pub(crate) fn apply_row_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::ToggleReduceTransparency => {
                self.reduce_transparency = !self.reduce_transparency;
                let v = bool_str(self.reduce_transparency);
                let _ = update_config_entries(&[("ui", "reduce_transparency", v)]);
            }
            RowControlId::ToggleReducedMotion => {
                self.reduced_motion = !self.reduced_motion;
                let v = bool_str(self.reduced_motion);
                let _ = update_config_entries(&[("ui", "reduced_motion", v)]);
            }
            RowControlId::GlassOpacityDown => {
                self.glass_opacity = (self.glass_opacity - 0.1).clamp(0.35, 1.0);
                let v = format!("{:.2}", self.glass_opacity);
                let _ = update_config_entries(&[("ui", "glass_opacity", &v)]);
            }
            RowControlId::GlassOpacityUp => {
                self.glass_opacity = (self.glass_opacity + 0.1).clamp(0.35, 1.0);
                let v = format!("{:.2}", self.glass_opacity);
                let _ = update_config_entries(&[("ui", "glass_opacity", &v)]);
            }
            RowControlId::ToggleWallpaper => {
                if self.custom_wallpaper.is_some() {
                    self.custom_wallpaper = None;
                    let _ = update_config_entries(&[("ui", "wallpaper", "")]);
                } else {
                    let fallback = String::from("/usr/share/backgrounds/rustyos-default.png");
                    self.custom_wallpaper = Some(fallback.clone());
                    let _ = update_config_entries(&[("ui", "wallpaper", &fallback)]);
                }
            }
            other => self.apply_device_action(other),
        }
        self.last_config_mtime = current_config_mtime();
        notify_compositor_reload();
        self.dirty = true;
    }

    fn apply_device_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::DockPositionSet(idx) => {
                match idx {
                    0 => "left",
                    2 => "right",
                    _ => "bottom",
                }
                .clone_into(&mut self.dock_position);
                let _ = update_config_entries(&[("dock", "position", &self.dock_position)]);
            }
            RowControlId::DockSizeDown => {
                self.dock_icon_size = (self.dock_icon_size - 4).clamp(36, 68);
                let v = self.dock_icon_size.to_string();
                let _ = update_config_entries(&[("dock", "icon_size", &v)]);
            }
            RowControlId::DockSizeUp => {
                self.dock_icon_size = (self.dock_icon_size + 4).clamp(36, 68);
                let v = self.dock_icon_size.to_string();
                let _ = update_config_entries(&[("dock", "icon_size", &v)]);
            }
            RowControlId::DockAutoHideToggle => {
                self.dock_auto_hide = !self.dock_auto_hide;
                let v = bool_str(self.dock_auto_hide);
                let _ = update_config_entries(&[("dock", "auto_hide", v)]);
            }
            RowControlId::DockIndicatorsToggle => {
                self.dock_show_indicators = !self.dock_show_indicators;
                let v = bool_str(self.dock_show_indicators);
                let _ = update_config_entries(&[("dock", "show_indicators", v)]);
            }
            RowControlId::DockRecentToggle => {
                self.dock_show_recent = !self.dock_show_recent;
                let v = bool_str(self.dock_show_recent);
                let _ = update_config_entries(&[("dock", "show_recent", v)]);
            }
            RowControlId::DockMinimizeIntoAppToggle => {
                self.dock_minimize_into_app = !self.dock_minimize_into_app;
                let v = bool_str(self.dock_minimize_into_app);
                let _ = update_config_entries(&[("dock", "minimize_into_app", v)]);
            }
            RowControlId::DockMinimizeEffectCycle => {
                if self.dock_minimize_effect == "genie" {
                    "scale"
                } else {
                    "genie"
                }
                .clone_into(&mut self.dock_minimize_effect);
                let _ = update_config_entries(&[(
                    "dock",
                    "minimize_effect",
                    &self.dock_minimize_effect,
                )]);
            }
            other => self.apply_system_action(other),
        }
    }

    fn apply_system_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::MenubarWorkspacesToggle => {
                self.menubar_workspaces = !self.menubar_workspaces;
                let v = bool_str(self.menubar_workspaces);
                let _ = update_config_entries(&[("ui", "menubar_show_workspaces", v)]);
            }
            RowControlId::MenubarTrayToggle => {
                self.menubar_tray = !self.menubar_tray;
                let v = bool_str(self.menubar_tray);
                let _ = update_config_entries(&[("ui", "menubar_show_tray", v)]);
            }
            RowControlId::MenubarInputToggle => {
                self.menubar_input = !self.menubar_input;
                let v = bool_str(self.menubar_input);
                let _ = update_config_entries(&[("ui", "menubar_show_input_source", v)]);
            }
            RowControlId::MenubarVolumeToggle => {
                self.menubar_volume = !self.menubar_volume;
                let v = bool_str(self.menubar_volume);
                let _ = update_config_entries(&[("ui", "menubar_show_volume", v)]);
            }
            RowControlId::Clock24hToggle => {
                self.clock_24h = !self.clock_24h;
                let v = bool_str(self.clock_24h);
                let _ = update_config_entries(&[("ui", "clock_24h", v)]);
            }
            RowControlId::UpdatesEnabledToggle => {
                self.updates_enabled = !self.updates_enabled;
                let v = bool_str(self.updates_enabled);
                let _ = update_config_entries(&[("updates", "enabled", v)]);
            }
            RowControlId::ScaleDown => {
                self.display_scale = (self.display_scale - 0.25).clamp(1.0, 3.0);
                let v = format!("{:.2}", self.display_scale);
                let _ = update_config_entries(&[("display", "scale", &v)]);
            }
            RowControlId::ScaleUp => {
                self.display_scale = (self.display_scale + 0.25).clamp(1.0, 3.0);
                let v = format!("{:.2}", self.display_scale);
                let _ = update_config_entries(&[("display", "scale", &v)]);
            }
            RowControlId::PreferMaxRefreshToggle => {
                self.prefer_max_refresh = !self.prefer_max_refresh;
                let v = bool_str(self.prefer_max_refresh);
                let _ = update_config_entries(&[("display", "prefer_max_refresh", v)]);
            }
            RowControlId::VrrModeCycle => {
                match self.vrr_mode.as_str() {
                    "off" => "fullscreen",
                    "fullscreen" => "always",
                    _ => "off",
                }
                .clone_into(&mut self.vrr_mode);
                let _ = update_config_entries(&[("display", "vrr_mode", &self.vrr_mode)]);
            }
            RowControlId::RotationCycle => {
                match self.display_rotation.as_str() {
                    "0" => "90",
                    "90" => "180",
                    "180" => "270",
                    _ => "0",
                }
                .clone_into(&mut self.display_rotation);
                let _ = update_config_entries(&[("display", "rotation", &self.display_rotation)]);
            }
            other => self.apply_input_action(other),
        }
    }

    fn apply_input_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::KeyboardLayoutCycle => {
                match self.keyboard_layout.as_str() {
                    "us" => "us,ru",
                    "us,ru" => "us,ua",
                    _ => "us",
                }
                .clone_into(&mut self.keyboard_layout);
                let _ = update_config_entries(&[("keyboard", "layout", &self.keyboard_layout)]);
            }
            RowControlId::KeyboardOptionCycle => {
                match self.keyboard_options.as_str() {
                    "grp:alt_shift_toggle" => "grp:win_space_toggle",
                    "grp:win_space_toggle" => "grp:caps_toggle",
                    _ => "grp:alt_shift_toggle",
                }
                .clone_into(&mut self.keyboard_options);
                let _ = update_config_entries(&[("keyboard", "options", &self.keyboard_options)]);
            }
            RowControlId::RepeatRateDown => {
                self.repeat_rate = (self.repeat_rate - 5).clamp(1, 120);
                let v = self.repeat_rate.to_string();
                let _ = update_config_entries(&[("keyboard", "repeat_rate", &v)]);
            }
            RowControlId::RepeatRateUp => {
                self.repeat_rate = (self.repeat_rate + 5).clamp(1, 120);
                let v = self.repeat_rate.to_string();
                let _ = update_config_entries(&[("keyboard", "repeat_rate", &v)]);
            }
            RowControlId::RepeatDelayDown => {
                self.repeat_delay = (self.repeat_delay - 25).clamp(50, 2000);
                let v = self.repeat_delay.to_string();
                let _ = update_config_entries(&[("keyboard", "repeat_delay", &v)]);
            }
            RowControlId::RepeatDelayUp => {
                self.repeat_delay = (self.repeat_delay + 25).clamp(50, 2000);
                let v = self.repeat_delay.to_string();
                let _ = update_config_entries(&[("keyboard", "repeat_delay", &v)]);
            }
            RowControlId::LockPasswordToggle => {
                self.require_password_on_lock = !self.require_password_on_lock;
                let v = bool_str(self.require_password_on_lock);
                let _ = update_config_entries(&[("security", "require_password_on_lock", v)]);
            }
            RowControlId::LockSleepToggle => {
                self.lock_on_sleep = !self.lock_on_sleep;
                let v = bool_str(self.lock_on_sleep);
                let _ = update_config_entries(&[("security", "lock_on_sleep", v)]);
            }
            RowControlId::PolkitToggle => {
                self.enable_polkit_agent = !self.enable_polkit_agent;
                let v = bool_str(self.enable_polkit_agent);
                let _ = update_config_entries(&[("security", "enable_polkit_agent", v)]);
            }
            RowControlId::VolumeDown => {
                let target = self.volume_percent.saturating_sub(5);
                let _ = AudioService::new().set_volume(target);
                self.refresh_services();
            }
            RowControlId::VolumeUp => {
                let target = self.volume_percent.saturating_add(5).min(100);
                let _ = AudioService::new().set_volume(target);
                self.refresh_services();
            }
            RowControlId::ToggleMute => {
                let _ = AudioService::new().toggle_mute();
                self.refresh_services();
            }
            RowControlId::CursorThemeCycle => {
                if self.cursor_theme == "Adwaita" {
                    "breeze_cursors"
                } else {
                    "Adwaita"
                }
                .clone_into(&mut self.cursor_theme);
                let _ = update_config_entries(&[("ui", "cursor_theme", &self.cursor_theme)]);
            }
            _ => {}
        }
    }

    pub(crate) fn poll_external_config(&mut self) -> bool {
        if self.theme_locked {
            return false;
        }
        let mtime = current_config_mtime();
        if mtime.is_some() && mtime != self.last_config_mtime {
            self.last_config_mtime = mtime;
            self.load_from_disk();
            self.dirty = true;
            return true;
        }
        false
    }

    pub(crate) fn load_from_disk(&mut self) {
        let Some(path) = config_toml_path() else {
            return;
        };
        let Ok(content) = fs::read_to_string(&path) else {
            return;
        };
        let mut section = String::new();
        let mut auto_mode_seen = false;
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                line[1..line.len() - 1].trim().clone_into(&mut section);
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let sec = section.trim();
            let key = k.trim();
            let val = unquote_toml(v.trim());
            if sec == "ui" && key == "theme_mode" && val.eq_ignore_ascii_case("auto") {
                auto_mode_seen = true;
            }
            self.apply_toml_kv(sec, key, val);
        }
        if auto_mode_seen {
            self.theme_choice = ThemeChoice::Auto;
        }
    }

    fn apply_toml_kv(&mut self, section: &str, key: &str, val: &str) {
        match (section, key) {
            ("ui", "theme") => {
                let dark = val.eq_ignore_ascii_case("dark");
                self.resolved_dark = dark;
                self.theme_choice = if dark {
                    ThemeChoice::Dark
                } else {
                    ThemeChoice::Light
                };
            }
            ("ui", "accent_index") => {
                if let Ok(idx) = val.parse::<usize>() {
                    self.accent_index = idx.min(ACCENT_COLORS.len() - 1);
                }
            }
            ("ui", "reduce_transparency") => self.reduce_transparency = parse_bool(val),
            ("ui", "reduced_motion") => self.reduced_motion = parse_bool(val),
            ("ui", "glass_opacity") => {
                if let Ok(v) = val.parse::<f32>() {
                    self.glass_opacity = v.clamp(0.35, 1.0);
                }
            }
            ("ui", "wallpaper") => {
                self.custom_wallpaper = (!val.is_empty()).then(|| val.to_owned());
            }
            ("ui", "menubar_show_workspaces") => self.menubar_workspaces = parse_bool(val),
            ("ui", "menubar_show_tray") => self.menubar_tray = parse_bool(val),
            ("ui", "menubar_show_input_source") => self.menubar_input = parse_bool(val),
            ("ui", "menubar_show_volume") => self.menubar_volume = parse_bool(val),
            ("ui", "clock_24h") => self.clock_24h = parse_bool(val),
            ("ui", "cursor_theme") if !val.is_empty() => val.clone_into(&mut self.cursor_theme),
            ("updates", "enabled") => self.updates_enabled = parse_bool(val),
            _ => self.apply_toml_device_kv(section, key, val),
        }
    }

    fn apply_toml_device_kv(&mut self, section: &str, key: &str, val: &str) {
        match (section, key) {
            ("dock", "icon_size") => {
                if let Ok(v) = val.parse::<i32>() {
                    self.dock_icon_size = v.clamp(36, 68);
                }
            }
            ("dock", "position") => self.dock_position = val.to_ascii_lowercase(),
            ("dock", "minimize_effect") => self.dock_minimize_effect = val.to_ascii_lowercase(),
            ("dock", "minimize_into_app") => self.dock_minimize_into_app = parse_bool(val),
            ("dock", "auto_hide") => self.dock_auto_hide = parse_bool(val),
            ("dock", "show_indicators") => self.dock_show_indicators = parse_bool(val),
            ("dock", "show_recent") => self.dock_show_recent = parse_bool(val),
            ("display", "scale") => {
                if let Ok(v) = val.parse::<f64>() {
                    self.display_scale = v.clamp(1.0, 3.0);
                }
            }
            ("display", "prefer_max_refresh") => self.prefer_max_refresh = parse_bool(val),
            ("display", "vrr_mode") => self.vrr_mode = val.to_ascii_lowercase(),
            ("display", "rotation") => val.clone_into(&mut self.display_rotation),
            ("keyboard", "layout") => val.clone_into(&mut self.keyboard_layout),
            ("keyboard", "options") => val.clone_into(&mut self.keyboard_options),
            ("keyboard", "repeat_rate") => {
                if let Ok(v) = val.parse::<i32>() {
                    self.repeat_rate = v.clamp(1, 120);
                }
            }
            ("keyboard", "repeat_delay") => {
                if let Ok(v) = val.parse::<i32>() {
                    self.repeat_delay = v.clamp(50, 2000);
                }
            }
            ("security", "require_password_on_lock") => {
                self.require_password_on_lock = parse_bool(val);
            }
            ("security", "lock_on_sleep") => self.lock_on_sleep = parse_bool(val),
            ("security", "enable_polkit_agent") => self.enable_polkit_agent = parse_bool(val),
            _ => {}
        }
    }
}

const fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn parse_bool(s: &str) -> bool {
    s.eq_ignore_ascii_case("true") || s == "1"
}

fn unquote_toml(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

fn config_toml_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        return Some(PathBuf::from(xdg).join("rustyos").join("config.toml"));
    }
    env::var("HOME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .map(|h| PathBuf::from(h).join(".config/rustyos/config.toml"))
}

fn format_toml_value(val: &str) -> String {
    if matches!(val, "true" | "false") || val.parse::<f64>().is_ok() {
        val.to_owned()
    } else {
        let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
}

fn patch_toml_text(original: &str, section: &str, key: &str, raw_val: &str) -> String {
    let mut lines: Vec<String> = original.lines().map(ToOwned::to_owned).collect();
    let formatted_line = format!("{key} = {}", format_toml_value(raw_val));
    let mut in_section = false;
    let mut section_found = false;
    let mut section_end_idx = lines.len();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let hdr = trimmed[1..trimmed.len() - 1].trim();
            if in_section {
                section_end_idx = i;
                break;
            }
            if hdr == section {
                in_section = true;
                section_found = true;
            }
            continue;
        }
        if in_section
            && let Some((lhs, _)) = trimmed.split_once('=')
            && lhs.trim() == key
        {
            if raw_val.is_empty() && key == "wallpaper" {
                lines.remove(i);
            } else {
                lines[i] = formatted_line;
            }
            return lines.join("\n") + "\n";
        }
    }

    if raw_val.is_empty() && key == "wallpaper" {
        return lines.join("\n") + "\n";
    }
    if section_found {
        lines.insert(section_end_idx, formatted_line);
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.push(formatted_line);
    }
    lines.join("\n") + "\n"
}

fn update_config_entries(updates: &[(&str, &str, &str)]) -> std::io::Result<()> {
    let Some(path) = config_toml_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = fs::read_to_string(&path)
        .unwrap_or_else(|_| String::from("# RustyOS compositor configuration\n\n[ui]\n"));
    for &(section, key, val) in updates {
        text = patch_toml_text(&text, section, key, val);
    }
    write_atomic(&path, text.as_bytes())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

fn current_config_mtime() -> Option<SystemTime> {
    let path = config_toml_path()?;
    fs::metadata(path).ok()?.modified().ok()
}

fn system_auto_prefers_dark() -> bool {
    let Ok(out) = Command::new("date").arg("+%H").output() else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let hour = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u8>()
        .unwrap_or(12);
    !(7..19).contains(&hour)
}

fn resolve_ipc_socket_path() -> Option<PathBuf> {
    if let Ok(sock_path) = env::var("RUSTYOS_IPC_SOCKET")
        && !sock_path.trim().is_empty()
    {
        return Some(PathBuf::from(sock_path));
    }
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")?;
    let wayland_display = env::var("WAYLAND_DISPLAY").ok()?;
    let candidate = PathBuf::from(runtime_dir).join(format!("rustyos-ipc-{wayland_display}.sock"));
    candidate.exists().then_some(candidate)
}

fn notify_compositor_reload() {
    let Some(sock_path) = resolve_ipc_socket_path() else {
        return;
    };
    let Ok(mut stream) = UnixStream::connect(sock_path) else {
        return;
    };
    let timeout = Some(Duration::from_millis(150));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    if stream.write_all(b"v1|RELOAD_CONFIG\n").is_ok() && stream.flush().is_ok() {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_patch_preserves_existing_sections_and_updates_keys() {
        let input = "[ui]\ntheme = \"dark\"\n[autostart]\ncommands = [\"foot\"]\n";
        let out = patch_toml_text(input, "ui", "theme", "light");
        assert!(out.contains("theme = \"light\""));
        assert!(out.contains("commands = [\"foot\"]"));
        let out2 = patch_toml_text(&out, "ui", "accent_index", "2");
        assert!(out2.contains("accent_index = 2"));
    }

    #[test]
    fn search_filters_sidebar_pages_by_keyword_and_category() {
        let mut st = SettingsAppState::defaults_with_page(SettingsPage::Appearance);
        st.search_query = String::from("dock");
        assert_eq!(st.visible_sidebar_pages(), vec![SettingsPage::DockDesktop]);
        st.search_query = String::from("network");
        assert_eq!(
            st.visible_sidebar_pages(),
            vec![SettingsPage::Wifi, SettingsPage::Network]
        );
    }
}
