//! State model, TOML configuration synchronization, and IPC live-reload for `rusty-settings` (`RUSTYOS-103`).

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use rusty_services::{AudioService, NetworkService, NetworkStatus};

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
            Self::Notifications => [0xFF, 0x3B, 0x30, 0xFF],
            Self::Sound => [0xF1, 0x68, 0x84, 0xFF],
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
    WifiEnabledToggle,
    WifiConnectIndex(usize),
    NetworkRefresh,
    NotifAllowToggle,
    NotifShowPreviewsCycle,
    NotifWhenSleepingToggle,
    NotifWhenLockedToggle,
    NotifMirroringCycle,
    NotifAppToggle(usize),
}

/// Coalesced UI damage scope for partial `wl_surface::damage_buffer` updates (`RUSTYOS-112`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DamageScope {
    #[default]
    None,
    Caret,
    Sidebar,
    Page,
    Full,
}

impl DamageScope {
    #[must_use]
    pub(crate) const fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, x) | (x, Self::None) => x,
            (Self::Full, _)
            | (_, Self::Full)
            | (Self::Sidebar, Self::Page)
            | (Self::Page, Self::Sidebar) => Self::Full,
            (Self::Caret, Self::Caret) => Self::Caret,
            (Self::Caret | Self::Sidebar, Self::Sidebar) | (Self::Sidebar, Self::Caret) => {
                Self::Sidebar
            }
            (Self::Caret | Self::Page, Self::Page) | (Self::Page, Self::Caret) => Self::Page,
        }
    }
}

/// Snapshot of live NetworkManager and audio service status (`RUSTYOS-112`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceSnapshot {
    pub(crate) network: NetworkStatus,
    pub(crate) volume_percent: u8,
    pub(crate) muted: bool,
}

/// Asynchronous service command executed off the Wayland GUI thread (`RUSTYOS-112`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServiceCommand {
    Refresh,
    SetVolume(u8),
    ToggleMute,
    SetWifiEnabled(bool),
    ConnectWifi(String),
}

/// Mutable state backing the System Settings window.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct SettingsAppState {
    pub(crate) running: bool,
    pub(crate) dirty: bool,
    damage_scope: DamageScope,
    pub(crate) maximized: bool,
    pub(crate) theme_choice: ThemeChoice,
    pub(crate) resolved_dark: bool,
    theme_locked: bool,
    last_config_mtime: Option<SystemTime>,
    last_config_poll: Option<Instant>,
    pub(crate) accent_index: usize,
    pub(crate) current_page: SettingsPage,
    pub(crate) history: Vec<SettingsPage>,
    pub(crate) history_idx: usize,
    pub(crate) page_scroll_y: i32,
    pub(crate) sidebar_scroll_y: i32,
    page_scroll_f: f64,
    page_scroll_target: f64,
    sidebar_scroll_f: f64,
    sidebar_scroll_target: f64,
    last_scroll_tick: Option<Instant>,
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
    pub(crate) notif_allow: bool,
    pub(crate) notif_show_previews: String,
    pub(crate) notif_when_sleeping: bool,
    pub(crate) notif_when_locked: bool,
    pub(crate) notif_mirroring_off: bool,
    pub(crate) notif_app_enabled: [bool; 5],
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
    pub(crate) network: NetworkStatus,
    pub(crate) volume_percent: u8,
    pub(crate) muted: bool,
    pending_service_cmds: Vec<ServiceCommand>,
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
        state.pending_service_cmds.push(ServiceCommand::Refresh);
        state
    }

    fn defaults_with_page(initial_page: SettingsPage) -> Self {
        Self {
            running: true,
            dirty: true,
            damage_scope: DamageScope::Full,
            maximized: false,
            theme_choice: ThemeChoice::Light,
            resolved_dark: false,
            theme_locked: false,
            last_config_mtime: None,
            last_config_poll: None,
            accent_index: 0,
            current_page: initial_page,
            history: vec![initial_page],
            history_idx: 0,
            page_scroll_y: 0,
            sidebar_scroll_y: 0,
            page_scroll_f: 0.0,
            page_scroll_target: 0.0,
            sidebar_scroll_f: 0.0,
            sidebar_scroll_target: 0.0,
            last_scroll_tick: None,
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
            notif_allow: true,
            notif_show_previews: String::from("When Unlocked"),
            notif_when_sleeping: false,
            notif_when_locked: true,
            notif_mirroring_off: true,
            notif_app_enabled: [true; 5],
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
            network: NetworkStatus::default(),
            volume_percent: 65,
            muted: false,
            pending_service_cmds: Vec::new(),
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

    pub(crate) fn mark_dirty(&mut self, scope: DamageScope) {
        self.dirty = true;
        self.damage_scope = self.damage_scope.merge(scope);
    }

    #[must_use]
    pub(crate) fn take_damage_scope(&mut self) -> DamageScope {
        let scope = if self.dirty && self.damage_scope == DamageScope::None {
            DamageScope::Full
        } else {
            self.damage_scope
        };
        self.dirty = false;
        self.damage_scope = DamageScope::None;
        scope
    }

    #[must_use]
    pub(crate) fn take_pending_service_commands(&mut self) -> Vec<ServiceCommand> {
        std::mem::take(&mut self.pending_service_cmds)
    }

    fn queue_service_refresh_for_page(&mut self, page: SettingsPage) {
        if matches!(
            page,
            SettingsPage::Wifi | SettingsPage::Network | SettingsPage::Sound
        ) {
            self.pending_service_cmds.push(ServiceCommand::Refresh);
        }
    }

    fn reset_page_scroll(&mut self) {
        self.page_scroll_y = 0;
        self.page_scroll_f = 0.0;
        self.page_scroll_target = 0.0;
        if (self.sidebar_scroll_target - self.sidebar_scroll_f).abs() < 0.25 {
            self.last_scroll_tick = None;
        }
    }

    fn reset_sidebar_scroll(&mut self) {
        self.sidebar_scroll_y = 0;
        self.sidebar_scroll_f = 0.0;
        self.sidebar_scroll_target = 0.0;
        if (self.page_scroll_target - self.page_scroll_f).abs() < 0.25 {
            self.last_scroll_tick = None;
        }
    }

    pub(crate) fn go_back(&mut self) {
        if self.can_go_back() {
            self.history_idx -= 1;
            self.current_page = self.history[self.history_idx];
            self.reset_page_scroll();
            self.queue_service_refresh_for_page(self.current_page);
            self.mark_dirty(DamageScope::Full);
        }
    }

    pub(crate) fn go_forward(&mut self) {
        if self.can_go_forward() {
            self.history_idx += 1;
            self.current_page = self.history[self.history_idx];
            self.reset_page_scroll();
            self.queue_service_refresh_for_page(self.current_page);
            self.mark_dirty(DamageScope::Full);
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
        self.reset_page_scroll();
        self.queue_service_refresh_for_page(page);
        self.mark_dirty(DamageScope::Full);
    }

    #[cfg(test)]
    pub(crate) fn scroll_by(
        &mut self,
        in_sidebar: bool,
        delta_y: i32,
        max_sidebar_scroll: i32,
        max_page_scroll: i32,
    ) -> bool {
        self.scroll_smooth_by(
            in_sidebar,
            f64::from(delta_y),
            max_sidebar_scroll,
            max_page_scroll,
            false,
        )
    }

    /// Accumulates fractional or animated scroll deltas for sidebar or content pane (`RUSTYOS-112`).
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn scroll_smooth_by(
        &mut self,
        in_sidebar: bool,
        delta_px: f64,
        max_sidebar_scroll: i32,
        max_page_scroll: i32,
        animate: bool,
    ) -> bool {
        if delta_px.abs() < 1e-4 {
            return false;
        }
        let max_scroll = if in_sidebar {
            max_sidebar_scroll
        } else {
            max_page_scroll
        };
        if max_scroll <= 0 {
            return false;
        }
        let max_f = f64::from(max_scroll);
        let (pos_i, pos_f, target) = if in_sidebar {
            (
                &mut self.sidebar_scroll_y,
                &mut self.sidebar_scroll_f,
                &mut self.sidebar_scroll_target,
            )
        } else {
            (
                &mut self.page_scroll_y,
                &mut self.page_scroll_f,
                &mut self.page_scroll_target,
            )
        };
        let next_target = (*target + delta_px).clamp(0.0, max_f);
        *target = next_target;
        if !animate || self.reduced_motion {
            *pos_f = next_target;
            let next_i = next_target.round() as i32;
            if next_i != *pos_i {
                *pos_i = next_i;
                self.mark_dirty(if in_sidebar {
                    DamageScope::Sidebar
                } else {
                    DamageScope::Page
                });
                return true;
            }
            return false;
        }
        if (*target - *pos_f).abs() < 0.05 {
            return false;
        }
        if self.last_scroll_tick.is_none() {
            self.last_scroll_tick = Instant::now().checked_sub(Duration::from_millis(8));
        }
        self.step_scroll_animation(Instant::now())
    }

    #[must_use]
    pub(crate) fn is_scroll_animating(&self) -> bool {
        self.last_scroll_tick.is_some()
            && ((self.page_scroll_target - self.page_scroll_f).abs() >= 0.25
                || (self.sidebar_scroll_target - self.sidebar_scroll_f).abs() >= 0.25)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn step_scroll_animation(&mut self, now: Instant) -> bool {
        let Some(prev) = self.last_scroll_tick else {
            return false;
        };
        let dt = now
            .saturating_duration_since(prev)
            .as_secs_f64()
            .clamp(0.001, 0.05);
        self.last_scroll_tick = Some(now);
        let alpha = 1.0 - (-22.0 * dt).exp();

        let mut changed_scope = DamageScope::None;
        let sidebar_done = step_scroll_axis(
            &mut self.sidebar_scroll_y,
            &mut self.sidebar_scroll_f,
            self.sidebar_scroll_target,
            alpha,
            DamageScope::Sidebar,
            &mut changed_scope,
        );
        let page_done = step_scroll_axis(
            &mut self.page_scroll_y,
            &mut self.page_scroll_f,
            self.page_scroll_target,
            alpha,
            DamageScope::Page,
            &mut changed_scope,
        );
        if sidebar_done && page_done {
            self.last_scroll_tick = None;
        }
        if changed_scope != DamageScope::None {
            self.mark_dirty(changed_scope);
            return true;
        }
        false
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn clamp_scroll_bounds(&mut self, max_sidebar_scroll: i32, max_page_scroll: i32) {
        let max_s = f64::from(max_sidebar_scroll.max(0));
        self.sidebar_scroll_target = self.sidebar_scroll_target.clamp(0.0, max_s);
        self.sidebar_scroll_f = self.sidebar_scroll_f.clamp(0.0, max_s);
        self.sidebar_scroll_y = self.sidebar_scroll_y.clamp(0, max_sidebar_scroll.max(0));

        let max_p = f64::from(max_page_scroll.max(0));
        self.page_scroll_target = self.page_scroll_target.clamp(0.0, max_p);
        self.page_scroll_f = self.page_scroll_f.clamp(0.0, max_p);
        self.page_scroll_y = self.page_scroll_y.clamp(0, max_page_scroll.max(0));
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

    pub(crate) fn apply_service_snapshot(&mut self, snap: ServiceSnapshot) -> bool {
        let clamped_vol = snap.volume_percent.clamp(0, 100);
        let changed = self.network_status != snap.network.status_label
            || self.network != snap.network
            || self.volume_percent != clamped_vol
            || self.muted != snap.muted;
        if !changed {
            return false;
        }
        self.network_status.clone_from(&snap.network.status_label);
        self.network = snap.network;
        self.volume_percent = clamped_vol;
        self.muted = snap.muted;
        self.mark_dirty(DamageScope::Page);
        true
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
        self.mark_dirty(DamageScope::Full);
    }

    pub(crate) fn choose_accent(&mut self, idx: usize) {
        let clamped = idx.min(ACCENT_COLORS.len() - 1);
        self.accent_index = clamped;
        let val = clamped.to_string();
        let _ = update_config_entries(&[("ui", "accent_index", &val)]);
        self.last_config_mtime = current_config_mtime();
        self.mark_dirty(DamageScope::Full);
    }

    pub(crate) fn update_caret_blink(&mut self, now: Instant) -> bool {
        if !self.search_focused {
            self.caret_visible = false;
            return false;
        }
        if now.saturating_duration_since(self.last_caret_blink) >= Duration::from_millis(530) {
            self.caret_visible = !self.caret_visible;
            self.last_caret_blink = now;
            self.mark_dirty(DamageScope::Caret);
            return true;
        }
        false
    }

    #[must_use]
    pub(crate) fn handle_search_key(&mut self, sym: &str) -> bool {
        if !self.search_focused {
            return false;
        }
        let mut navigated = false;
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
                    navigated = true;
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
        self.reset_sidebar_scroll();
        self.last_caret_blink = Instant::now();
        self.mark_dirty(if navigated {
            DamageScope::Full
        } else {
            DamageScope::Sidebar
        });
        true
    }

    fn try_apply_service_action(&mut self, action: RowControlId) -> bool {
        match action {
            RowControlId::VolumeDown => {
                let target = self.volume_percent.saturating_sub(5);
                self.volume_percent = target;
                self.pending_service_cmds
                    .push(ServiceCommand::SetVolume(target));
            }
            RowControlId::VolumeUp => {
                let target = self.volume_percent.saturating_add(5).min(100);
                self.volume_percent = target;
                self.pending_service_cmds
                    .push(ServiceCommand::SetVolume(target));
            }
            RowControlId::ToggleMute => {
                self.muted = !self.muted;
                self.pending_service_cmds.push(ServiceCommand::ToggleMute);
            }
            RowControlId::WifiEnabledToggle => {
                let next = !self.network.wifi_enabled;
                self.network.wifi_enabled = next;
                self.pending_service_cmds
                    .push(ServiceCommand::SetWifiEnabled(next));
            }
            RowControlId::WifiConnectIndex(idx) => {
                let Some(ap) = self.network.wifi_networks.get(idx).cloned() else {
                    return true;
                };
                self.network_status = format!("Connecting to {}...", ap.ssid);
                self.pending_service_cmds
                    .push(ServiceCommand::ConnectWifi(ap.ssid));
            }
            RowControlId::NetworkRefresh => {
                self.network_status = String::from("Refreshing...");
                self.pending_service_cmds.push(ServiceCommand::Refresh);
            }
            _ => return false,
        }
        self.mark_dirty(DamageScope::Page);
        true
    }

    pub(crate) fn apply_row_action(&mut self, action: RowControlId) {
        if self.try_apply_service_action(action) {
            return;
        }
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
        self.mark_dirty(DamageScope::Page);
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
            RowControlId::CursorThemeCycle => {
                if self.cursor_theme == "Adwaita" {
                    "breeze_cursors"
                } else {
                    "Adwaita"
                }
                .clone_into(&mut self.cursor_theme);
                let _ = update_config_entries(&[("ui", "cursor_theme", &self.cursor_theme)]);
            }
            other => self.apply_notification_action(other),
        }
    }

    fn apply_notification_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::NotifAllowToggle => {
                self.notif_allow = !self.notif_allow;
                let v = bool_str(self.notif_allow);
                let _ = update_config_entries(&[("notifications", "allow", v)]);
            }
            RowControlId::NotifShowPreviewsCycle => {
                let (next_label, toml_val) = match self.notif_show_previews.as_str() {
                    "When Unlocked" => ("Never", "never"),
                    "Never" => ("Always", "always"),
                    _ => ("When Unlocked", "when_unlocked"),
                };
                next_label.clone_into(&mut self.notif_show_previews);
                let _ = update_config_entries(&[("notifications", "show_previews", toml_val)]);
            }
            RowControlId::NotifWhenSleepingToggle => {
                self.notif_when_sleeping = !self.notif_when_sleeping;
                let v = bool_str(self.notif_when_sleeping);
                let _ = update_config_entries(&[("notifications", "when_sleeping", v)]);
            }
            RowControlId::NotifWhenLockedToggle => {
                self.notif_when_locked = !self.notif_when_locked;
                let v = bool_str(self.notif_when_locked);
                let _ = update_config_entries(&[("notifications", "when_locked", v)]);
            }
            RowControlId::NotifMirroringCycle => {
                self.notif_mirroring_off = !self.notif_mirroring_off;
                let v = bool_str(self.notif_mirroring_off);
                let _ = update_config_entries(&[("notifications", "mirroring_off", v)]);
            }
            RowControlId::NotifAppToggle(idx) => {
                if let Some(enabled) = self.notif_app_enabled.get_mut(idx) {
                    *enabled = !*enabled;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn poll_external_config_if_due(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_config_poll
            && now.saturating_duration_since(last) < Duration::from_millis(200)
        {
            return false;
        }
        self.last_config_poll = Some(now);
        self.poll_external_config()
    }

    pub(crate) fn poll_external_config(&mut self) -> bool {
        if self.theme_locked {
            return false;
        }
        let mtime = current_config_mtime();
        if mtime.is_some() && mtime != self.last_config_mtime {
            self.last_config_mtime = mtime;
            self.load_from_disk();
            self.mark_dirty(DamageScope::Full);
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
            ("notifications", "allow") => self.notif_allow = parse_bool(val),
            ("notifications", "show_previews") => {
                let norm = match val.trim().to_ascii_lowercase().as_str() {
                    "always" => "Always",
                    "never" | "off" => "Never",
                    _ => "When Unlocked",
                };
                norm.clone_into(&mut self.notif_show_previews);
            }
            ("notifications", "when_sleeping") => self.notif_when_sleeping = parse_bool(val),
            ("notifications", "when_locked") => self.notif_when_locked = parse_bool(val),
            ("notifications", "mirroring_off") => self.notif_mirroring_off = parse_bool(val),
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

#[allow(clippy::cast_possible_truncation)]
fn step_scroll_axis(
    pos_i: &mut i32,
    pos_f: &mut f64,
    target: f64,
    alpha: f64,
    scope: DamageScope,
    changed_scope: &mut DamageScope,
) -> bool {
    let diff = target - *pos_f;
    if diff.abs() < 0.25 {
        *pos_f = target;
        let next_i = target.round() as i32;
        if next_i != *pos_i {
            *pos_i = next_i;
            *changed_scope = changed_scope.merge(scope);
        }
        return true;
    }
    *pos_f += diff * alpha;
    if (target - *pos_f).abs() < 0.25 {
        *pos_f = target;
        let next_i = target.round() as i32;
        if next_i != *pos_i {
            *pos_i = next_i;
            *changed_scope = changed_scope.merge(scope);
        }
        return true;
    }
    let next_i = pos_f.round() as i32;
    if next_i != *pos_i {
        *pos_i = next_i;
        *changed_scope = changed_scope.merge(scope);
    }
    false
}

/// Queries live NetworkManager and audio status (`RUSTYOS-112`).
#[must_use]
pub(crate) fn query_service_snapshot() -> ServiceSnapshot {
    let network = NetworkService::new().query_status();
    let audio = AudioService::new().query_status();
    ServiceSnapshot {
        network,
        volume_percent: audio.volume_percent.clamp(0, 100),
        muted: audio.muted,
    }
}

/// Collapses redundant queued service commands before worker execution (`RUSTYOS-112`).
#[must_use]
pub(crate) fn coalesce_service_commands(cmds: Vec<ServiceCommand>) -> Vec<ServiceCommand> {
    let mut had_refresh = false;
    let mut set_volume = None;
    let mut toggle_mute = false;
    let mut set_wifi = None;
    let mut connect_ssid = None;

    for cmd in cmds {
        match cmd {
            ServiceCommand::Refresh => had_refresh = true,
            ServiceCommand::SetVolume(v) => set_volume = Some(v),
            ServiceCommand::ToggleMute => toggle_mute = !toggle_mute,
            ServiceCommand::SetWifiEnabled(en) => {
                set_wifi = Some(en);
                if !en {
                    connect_ssid = None;
                }
            }
            ServiceCommand::ConnectWifi(ssid) => connect_ssid = Some(ssid),
        }
    }

    let mut out = Vec::new();
    if let Some(en) = set_wifi {
        out.push(ServiceCommand::SetWifiEnabled(en));
    }
    if let Some(ssid) = connect_ssid {
        out.push(ServiceCommand::ConnectWifi(ssid));
    }
    if let Some(v) = set_volume {
        out.push(ServiceCommand::SetVolume(v));
    }
    if toggle_mute {
        out.push(ServiceCommand::ToggleMute);
    }
    if out.is_empty() && had_refresh {
        out.push(ServiceCommand::Refresh);
    }
    out
}

/// Executes a coalesced batch of service commands and queries fresh snapshot (`RUSTYOS-112`).
#[must_use]
pub(crate) fn execute_coalesced_service_batch(cmds: Vec<ServiceCommand>) -> ServiceSnapshot {
    for cmd in coalesce_service_commands(cmds) {
        match cmd {
            ServiceCommand::Refresh => {}
            ServiceCommand::SetVolume(v) => {
                let _ = AudioService::new().set_volume(v);
            }
            ServiceCommand::ToggleMute => {
                let _ = AudioService::new().toggle_mute();
            }
            ServiceCommand::SetWifiEnabled(en) => {
                let _ = NetworkService::new().set_wifi_enabled(en);
            }
            ServiceCommand::ConnectWifi(ssid) => {
                let _ = NetworkService::new().connect_wifi(&ssid);
            }
        }
    }
    query_service_snapshot()
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

    #[test]
    fn smooth_scroll_accumulates_subpixel_deltas_and_eases_to_target() {
        let mut st = SettingsAppState::defaults_with_page(SettingsPage::Keyboard);
        assert!(!st.scroll_smooth_by(false, 0.4, 100, 400, false));
        assert!(st.scroll_smooth_by(false, 0.4, 100, 400, false));
        assert_eq!(st.page_scroll_y, 1);
        assert!(!st.scroll_smooth_by(false, 0.6, 100, 400, false));
        assert_eq!(st.page_scroll_y, 1);
        assert!(st.scroll_smooth_by(false, 0.4, 100, 400, false));
        assert_eq!(st.page_scroll_y, 2);

        // Discrete wheel impulse animates smoothly when !reduced_motion
        let mut anim = SettingsAppState::defaults_with_page(SettingsPage::Keyboard);
        assert!(anim.scroll_smooth_by(false, 36.0, 0, 400, true));
        assert!(anim.page_scroll_y > 0 && anim.page_scroll_y < 36);
        assert!(anim.is_scroll_animating());
        let mut t = Instant::now();
        for _ in 0..40 {
            t += Duration::from_millis(16);
            let _ = anim.step_scroll_animation(t);
        }
        assert_eq!(anim.page_scroll_y, 36);
        assert!(!anim.is_scroll_animating());

        // Reduced motion snaps immediately without trailing ticks
        let mut rm = SettingsAppState::defaults_with_page(SettingsPage::Keyboard);
        rm.reduced_motion = true;
        assert!(rm.scroll_smooth_by(false, 36.0, 0, 400, true));
        assert_eq!(rm.page_scroll_y, 36);
        assert!(!rm.is_scroll_animating());
    }

    #[test]
    fn damage_scope_merges_and_coalesces_service_commands() {
        assert_eq!(
            DamageScope::Caret.merge(DamageScope::Sidebar),
            DamageScope::Sidebar
        );
        assert_eq!(
            DamageScope::Sidebar.merge(DamageScope::Page),
            DamageScope::Full
        );

        let coalesced = coalesce_service_commands(vec![
            ServiceCommand::Refresh,
            ServiceCommand::SetVolume(45),
            ServiceCommand::SetVolume(55),
            ServiceCommand::ToggleMute,
            ServiceCommand::ToggleMute,
            ServiceCommand::Refresh,
        ]);
        assert_eq!(coalesced, vec![ServiceCommand::SetVolume(55)]);

        let refresh_only =
            coalesce_service_commands(vec![ServiceCommand::Refresh, ServiceCommand::Refresh]);
        assert_eq!(refresh_only, vec![ServiceCommand::Refresh]);
    }
}
