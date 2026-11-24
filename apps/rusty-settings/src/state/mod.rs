//! State model, TOML configuration synchronization, and IPC live-reload for `rusty-settings` (`RUSTYOS-103`).

mod actions;
mod config_io;
mod modals;
#[cfg(test)]
mod tests;

#[allow(clippy::wildcard_imports)]
use config_io::*;
pub(crate) use config_io::{dock_visibility_label, execute_coalesced_service_batch};

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use rusty_services::file_manager::RUSTYOS_ACCENT_SWATCHES;
use rusty_services::updates::{
    UpdateCheckState, UpdateStatusSnapshot, UpdateTransactionHandle, UpdateTxEvent, UpdateTxPhase,
    check_all_pending_updates,
};
use rusty_services::{
    BluetoothSnapshot, CurrentUserDetails, NetworkStatus, PowerDetails, PowerProfile,
    TimeDateStatus, UserAccount, UserService,
};

/// Predefined accent color palette shared across RustyOS (`RUSTYOS-114`).
pub(crate) const ACCENT_COLORS: [[u8; 4]; RUSTYOS_ACCENT_SWATCHES.len()] = RUSTYOS_ACCENT_SWATCHES;

/// CLI startup overrides parsed by `main`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StartupOptions {
    pub(crate) theme_override: Option<ThemeChoice>,
    pub(crate) initial_page: SettingsPage,
    pub(crate) no_gpu: bool,
    pub(crate) wifi_prompt_ssid: Option<String>,
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
    Bluetooth,
    Network,
    About,
    Battery,
    Users,
    #[default]
    Appearance,
    Wallpaper,
    DockDesktop,
    MenuBar,
    Notifications,
    DateTime,
    Language,
    SoftwareUpdate,
    Display,
    Sound,
    Keyboard,
    Shortcuts,
    Mouse,
    Touchpad,
    Fonts,
    IconTheme,
    CursorStyle,
}

impl SettingsPage {
    pub(crate) const SIDEBAR: [Self; 20] = [
        Self::Wifi,
        Self::Bluetooth,
        Self::Network,
        Self::About,
        Self::Battery,
        Self::Users,
        Self::DateTime,
        Self::Language,
        Self::SoftwareUpdate,
        Self::Appearance,
        Self::Wallpaper,
        Self::DockDesktop,
        Self::MenuBar,
        Self::Notifications,
        Self::Display,
        Self::Sound,
        Self::Keyboard,
        Self::Shortcuts,
        Self::Mouse,
        Self::Touchpad,
    ];

    #[must_use]
    pub(crate) fn from_cli_id(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "wifi" | "wlan" => Self::Wifi,
            "bluetooth" | "bt" => Self::Bluetooth,
            "network" => Self::Network,
            "about" | "general" | "info" | "system" => Self::About,
            "battery" | "power" | "energy" => Self::Battery,
            "users" | "user" | "accounts" => Self::Users,
            "updates" | "update" | "softwareupdate" | "software-update" => Self::SoftwareUpdate,
            "wallpaper" | "background" => Self::Wallpaper,
            "dock" | "desktop" => Self::DockDesktop,
            "menubar" | "menu-bar" | "topbar" | "top-bar" => Self::MenuBar,
            "notifications" => Self::Notifications,
            "datetime" | "date" | "time" | "clock" => Self::DateTime,
            "language" | "region" | "locale" => Self::Language,
            "display" | "displays" => Self::Display,
            "sound" | "audio" => Self::Sound,
            "keyboard" => Self::Keyboard,
            "shortcuts" | "shortcut" | "hotkeys" | "keybindings" => Self::Shortcuts,
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
            Self::Bluetooth => "Bluetooth",
            Self::Network => "Network",
            Self::About => "About",
            Self::Battery => "Battery",
            Self::Users => "Users",
            Self::Appearance => "Appearance",
            Self::Wallpaper => "Wallpaper",
            Self::DockDesktop => "Dock & Desktop",
            Self::MenuBar => "Menu Bar",
            Self::Notifications => "Notifications",
            Self::DateTime => "Date & Time",
            Self::Language => "Language & Region",
            Self::SoftwareUpdate => "Software Update",
            Self::Display => "Display",
            Self::Sound => "Sound",
            Self::Keyboard => "Keyboard",
            Self::Shortcuts => "Shortcuts",
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
            Self::Wifi | Self::Bluetooth | Self::Network => Some("Network"),
            Self::About
            | Self::Battery
            | Self::Users
            | Self::DateTime
            | Self::Language
            | Self::SoftwareUpdate => Some("System"),
            Self::Appearance
            | Self::Wallpaper
            | Self::DockDesktop
            | Self::MenuBar
            | Self::Notifications => Some("Desktop"),
            Self::Display
            | Self::Sound
            | Self::Keyboard
            | Self::Shortcuts
            | Self::Mouse
            | Self::Touchpad => Some("Devices"),
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
            Self::Wifi | Self::Network | Self::Bluetooth | Self::Users => [0x00, 0x7A, 0xFF, 0xFF],
            Self::About => [0x8E, 0x8E, 0x93, 0xFF],
            Self::Battery | Self::IconTheme => [0x34, 0xC7, 0x59, 0xFF],
            Self::Appearance => [0x03, 0xB4, 0xCB, 0xFF],
            Self::Wallpaper => [0x34, 0xB4, 0xA7, 0xFF],
            Self::DockDesktop => [0x58, 0x56, 0xD6, 0xFF],
            Self::Notifications => [0xFF, 0x3B, 0x30, 0xFF],
            Self::SoftwareUpdate | Self::Display | Self::MenuBar => [0x00, 0x87, 0xED, 0xFF],
            Self::DateTime => [0x41, 0x8C, 0xFF, 0xFF],
            Self::Language => [0x20, 0xA7, 0xFF, 0xFF],
            Self::Sound => [0xF1, 0x68, 0x84, 0xFF],
            Self::Keyboard => [0x5E, 0x6A, 0xD2, 0xFF],
            Self::Shortcuts => [0x63, 0x63, 0x66, 0xFF],
            Self::Mouse | Self::CursorStyle => [0x33, 0x85, 0xFF, 0xFF],
            Self::Touchpad => [0x99, 0x99, 0x99, 0xFF],
            Self::Fonts => [0xFE, 0x84, 0x33, 0xFF],
        }
    }

    #[must_use]
    pub(crate) const fn keywords(self) -> &'static str {
        match self {
            Self::Wifi => "wireless wlan network internet password",
            Self::Bluetooth => "bluetooth wireless headset audio mouse keyboard pair devices",
            Self::Network => "vpn proxy ethernet tethering internet",
            Self::About => "about hardware system cpu memory gpu disk kernel os version host",
            Self::Battery => "battery power charging low power energy sleep health cycles",
            Self::Users => "user account profile groups uid admin home shell login",
            Self::Appearance => "theme dark light auto accent color",
            Self::Wallpaper => "background picture desktop image wallpaper",
            Self::DockDesktop => {
                "dock position size visibility smart hide auto hide glass effect transparency motion"
            }
            Self::MenuBar => "menubar topbar wifi battery volume bluetooth clock contrast tray",
            Self::Notifications => "alerts banners do not disturb previews",
            Self::DateTime => "date time clock timezone 24-hour ntp sync",
            Self::Language => "language region locale english russian ukrainian formats",
            Self::SoftwareUpdate => "updates packages upgrade software check version pacman",
            Self::Display => "monitor screen scale hidpi refresh rate vrr rotation",
            Self::Sound => "audio volume speakers mute microphone",
            Self::Keyboard => "input layout shortcut repeat rate delay",
            Self::Shortcuts => "shortcuts hotkeys keybindings launcher overview lock snap window",
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
    DockVisibilityCycle,
    DockIndicatorsToggle,
    DockRecentToggle,
    DockMinimizeIntoAppToggle,
    DockMinimizeEffectCycle,
    MenubarWorkspacesToggle,
    MenubarTrayToggle,
    MenubarInputToggle,
    MenubarVolumeToggle,
    MenubarWifiToggle,
    MenubarBatteryToggle,
    MenubarBatteryPercentageToggle,
    MenubarBluetoothToggle,
    MenubarAdaptiveContrastToggle,
    Clock24hToggle,
    ClockLocaleCycle,
    UpdatesEnabledToggle,
    ScaleDown,
    ScaleUp,
    DisplayOutputCycle,
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
    CursorSizeDown,
    CursorSizeUp,
    WifiEnabledToggle,
    WifiConnectIndex(usize),
    WifiInfoIndex(usize),
    WifiDisconnectActive,
    NetworkRefresh,
    NotifAllowToggle,
    NotifShowPreviewsCycle,
    NotifWhenSleepingToggle,
    NotifWhenLockedToggle,
    NotifMirroringCycle,
    NotifAppToggle(usize),
    UpdatesCheckNow,
    UpdatesInstallNow,
    BluetoothPoweredToggle,
    BluetoothScanToggle,
    BluetoothActionIndex(usize),
    BluetoothInfoIndex(usize),
    PowerProfileCycle,
    PowerDisplaySleepCycle,
    PowerAutoLockCycle,
    PowerDimOnBatteryToggle,
    PowerOptionsOpen,
    BatteryDetailsOpen,
    AboutMoreInfoOpen,
    ShortcutEditIndex(usize),
    ShortcutsResetDefaults,
    DateTimeNtpToggle,
    DateTimeTimezoneOpen,
    DateTimeTimezoneCycle,
    DateTimeManualOpen,
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

/// Read-only hardware and operating-system info displayed in `About` (`RUSTYOS-131`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SystemAboutInfo {
    pub(crate) hostname: String,
    pub(crate) os_name: String,
    pub(crate) kernel: String,
    pub(crate) architecture: String,
    pub(crate) cpu: String,
    pub(crate) memory: String,
    pub(crate) graphics: String,
    pub(crate) startup_disk: String,
}

impl Default for SystemAboutInfo {
    fn default() -> Self {
        Self {
            hostname: String::from("rustyos"),
            os_name: String::from("RustyOS Linux"),
            kernel: String::from("Linux"),
            architecture: String::from(std::env::consts::ARCH),
            cpu: String::from("Processor"),
            memory: String::from("Unknown"),
            graphics: String::from("GPU"),
            startup_disk: String::from("/"),
        }
    }
}

/// Canonical notification app keys synced with `[notifications].muted_apps` (`RUSTYOS-170`).
pub(crate) const NOTIF_APP_KEYS: [&str; 5] =
    ["files", "terminal", "browser", "updater", "settings"];

/// Customizable `[shortcuts]` metadata catalog matching compositor `ShortcutBindings` (`RUSTYOS-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShortcutDef {
    pub(crate) key: &'static str,
    pub(crate) label: &'static str,
    pub(crate) category: &'static str,
    pub(crate) default_combo: &'static str,
}

pub(crate) const SHORTCUT_DEFS: [ShortcutDef; 17] = [
    ShortcutDef {
        key: "launcher",
        label: "Open Spotlight Launcher",
        category: "System & Shell",
        default_combo: "Super+Space",
    },
    ShortcutDef {
        key: "overview",
        label: "Workspace Overview",
        category: "System & Shell",
        default_combo: "Super+Tab",
    },
    ShortcutDef {
        key: "lock_screen",
        label: "Lock Screen",
        category: "System & Shell",
        default_combo: "Super+L",
    },
    ShortcutDef {
        key: "settings",
        label: "System Settings",
        category: "System & Shell",
        default_combo: "Super+Comma",
    },
    ShortcutDef {
        key: "quick_settings",
        label: "Control Center",
        category: "System & Shell",
        default_combo: "Alt+S",
    },
    ShortcutDef {
        key: "reload_config",
        label: "Reload Configuration",
        category: "System & Shell",
        default_combo: "Super+Shift+R",
    },
    ShortcutDef {
        key: "reduced_motion",
        label: "Toggle Reduced Motion",
        category: "System & Shell",
        default_combo: "Alt+R",
    },
    ShortcutDef {
        key: "toggle_notifications",
        label: "Send Test Notification",
        category: "System & Shell",
        default_combo: "Alt+N",
    },
    ShortcutDef {
        key: "clear_notifications",
        label: "Clear Notifications",
        category: "System & Shell",
        default_combo: "Alt+Shift+N",
    },
    ShortcutDef {
        key: "close_window",
        label: "Close Focused Window",
        category: "Windows & Tiling",
        default_combo: "Alt+W",
    },
    ShortcutDef {
        key: "toggle_maximize",
        label: "Maximize / Restore Window",
        category: "Windows & Tiling",
        default_combo: "Alt+M",
    },
    ShortcutDef {
        key: "toggle_fullscreen",
        label: "Toggle Fullscreen",
        category: "Windows & Tiling",
        default_combo: "Alt+F",
    },
    ShortcutDef {
        key: "minimize_window",
        label: "Minimize Window to Dock",
        category: "Windows & Tiling",
        default_combo: "Alt+H",
    },
    ShortcutDef {
        key: "snap_left",
        label: "Tile Window Left Half",
        category: "Windows & Tiling",
        default_combo: "Alt+Left",
    },
    ShortcutDef {
        key: "snap_right",
        label: "Tile Window Right Half",
        category: "Windows & Tiling",
        default_combo: "Alt+Right",
    },
    ShortcutDef {
        key: "snap_center",
        label: "Center Window",
        category: "Windows & Tiling",
        default_combo: "Alt+C",
    },
    ShortcutDef {
        key: "switch_layout",
        label: "Switch Floating / Tiling Layout",
        category: "Windows & Tiling",
        default_combo: "Alt+L",
    },
];

/// Active modal dialog sheet hosted inside `rusty-settings` (`RUSTYOS-131`, `RUSTYOS-159`).
#[derive(Debug, Clone)]
pub(crate) enum SettingsModal {
    BluetoothDeviceInfo {
        address: String,
        paired: bool,
        connected: bool,
        sheet: rusty_ui::DialogSheet,
    },
    WifiPassword {
        ssid: String,
        show_password: bool,
        sheet: rusty_ui::DialogSheet,
    },
    WifiNetworkInfo {
        ssid: String,
        connected: bool,
        sheet: rusty_ui::DialogSheet,
    },
    InfoSheet {
        sheet: rusty_ui::DialogSheet,
    },
    ShortcutEdit {
        index: usize,
        conflict_index: Option<usize>,
        validated_combo: Option<String>,
        sheet: rusty_ui::DialogSheet,
    },
    TimezoneEdit {
        sheet: rusty_ui::DialogSheet,
    },
    ManualDateTime {
        sheet: rusty_ui::DialogSheet,
    },
}

impl SettingsModal {
    #[must_use]
    pub(crate) const fn sheet(&self) -> &rusty_ui::DialogSheet {
        match self {
            Self::BluetoothDeviceInfo { sheet, .. }
            | Self::WifiPassword { sheet, .. }
            | Self::WifiNetworkInfo { sheet, .. }
            | Self::InfoSheet { sheet }
            | Self::ShortcutEdit { sheet, .. }
            | Self::TimezoneEdit { sheet }
            | Self::ManualDateTime { sheet } => sheet,
        }
    }

    pub(crate) fn sheet_mut(&mut self) -> &mut rusty_ui::DialogSheet {
        match self {
            Self::BluetoothDeviceInfo { sheet, .. }
            | Self::WifiPassword { sheet, .. }
            | Self::WifiNetworkInfo { sheet, .. }
            | Self::InfoSheet { sheet }
            | Self::ShortcutEdit { sheet, .. }
            | Self::TimezoneEdit { sheet }
            | Self::ManualDateTime { sheet } => sheet,
        }
    }

    /// Computes the "Show password" checkbox origin and hit rectangle inside [`Self::WifiPassword`].
    #[must_use]
    pub(crate) fn wifi_show_password_checkbox(
        &self,
        win_w: u32,
        win_h: u32,
    ) -> Option<rusty_ui::Checkbox> {
        let Self::WifiPassword {
            show_password,
            sheet,
            ..
        } = self
        else {
            return None;
        };
        let layout = sheet.layout(win_w, win_h);
        Some(rusty_ui::Checkbox::new(
            layout.card_rect.x + 20,
            layout.primary_rect.y + 7,
            "Show password",
            *show_password,
        ))
    }
}

/// Type of Wi-Fi operation reported back from the background service worker (`RUSTYOS-159`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WifiActionKind {
    Connect,
    Disconnect,
    Forget,
}

/// Result of a Wi-Fi connect/disconnect/forget command executed off the UI thread (`RUSTYOS-159`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WifiActionOutcome {
    pub(crate) ssid: String,
    pub(crate) kind: WifiActionKind,
    pub(crate) result: Result<(), String>,
}

/// Snapshot of live NetworkManager, BlueZ, power, timedated, and audio service status (`RUSTYOS-112`, `RUSTYOS-131`, `RUSTYOS-160`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ServiceSnapshot {
    pub(crate) network: NetworkStatus,
    pub(crate) bluetooth: BluetoothSnapshot,
    pub(crate) power: PowerDetails,
    pub(crate) timedate: TimeDateStatus,
    pub(crate) timedate_error: Option<String>,
    pub(crate) volume_percent: u8,
    pub(crate) muted: bool,
    pub(crate) wifi_action_result: Option<WifiActionOutcome>,
}

/// Asynchronous service command executed off the Wayland GUI thread (`RUSTYOS-112`, `RUSTYOS-131`, `RUSTYOS-160`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServiceCommand {
    Refresh,
    RescanWifi,
    SetVolume(u8),
    ToggleMute,
    SetWifiEnabled(bool),
    ConnectWifi(String),
    ConnectWifiWithPassword { ssid: String, password: String },
    DisconnectWifi(String),
    ForgetWifi(String),
    SetBluetoothPowered(bool),
    SetBluetoothDiscovering(bool),
    ConnectBluetooth(String),
    DisconnectBluetooth(String),
    PairBluetooth(String),
    RemoveBluetooth(String),
    SetPowerProfile(PowerProfile),
    SetNtpEnabled(bool),
    SetTimezone(String),
    SetManualDateTime(String),
}

/// Mutable state backing the System Settings window.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct SettingsAppState {
    pub(crate) running: bool,
    pub(crate) dirty: bool,
    damage_scope: DamageScope,
    pub(crate) maximized: bool,
    pub(crate) focused: bool,
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
    pub(crate) epoch: Instant,
    pub(crate) search_field: rusty_ui::TextField,
    pub(crate) focus: rusty_ui::FocusScope,
    pub(crate) focus_search_id: rusty_ui::WidgetId,
    toggle_anims: Vec<(RowControlId, rusty_ui::ToggleMotion)>,

    // Persistent config.toml preferences
    pub(crate) reduce_transparency: bool,
    pub(crate) reduced_motion: bool,
    pub(crate) glass_opacity: f32,
    pub(crate) custom_wallpaper: Option<String>,
    pub(crate) dock_icon_size: i32,
    pub(crate) dock_position: String,
    pub(crate) dock_minimize_effect: String,
    pub(crate) dock_minimize_into_app: bool,
    pub(crate) dock_visibility: String,
    pub(crate) dock_auto_hide: bool,
    pub(crate) dock_show_indicators: bool,
    pub(crate) dock_show_recent: bool,
    pub(crate) menubar_workspaces: bool,
    pub(crate) menubar_tray: bool,
    pub(crate) menubar_input: bool,
    pub(crate) menubar_volume: bool,
    pub(crate) menubar_wifi: bool,
    pub(crate) menubar_battery: bool,
    pub(crate) menubar_battery_percentage: bool,
    pub(crate) menubar_bluetooth: bool,
    pub(crate) menubar_adaptive_contrast: bool,
    pub(crate) clock_24h: bool,
    pub(crate) clock_locale: String,
    pub(crate) updates_enabled: bool,
    pub(crate) notif_allow: bool,
    pub(crate) notif_show_previews: String,
    pub(crate) notif_when_sleeping: bool,
    pub(crate) notif_when_locked: bool,
    pub(crate) notif_mirroring_off: bool,
    pub(crate) notif_app_enabled: [bool; 5],
    pub(crate) display_scale: f64,
    pub(crate) display_outputs: Vec<String>,
    pub(crate) selected_display_idx: usize,
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
    pub(crate) cursor_size: u32,
    pub(crate) power_low_power_mode: String,
    pub(crate) power_display_sleep_minutes: u32,
    pub(crate) power_auto_lock_minutes: u32,
    pub(crate) power_dim_on_battery: bool,
    pub(crate) shortcuts: [String; SHORTCUT_DEFS.len()],

    // Interactive rusty-ui stateful widgets & modals (`RUSTYOS-131`)
    pub(crate) battery_tab: rusty_ui::SegmentedTabs,
    pub(crate) battery_tab_animating: bool,
    pub(crate) low_power_dropdown: rusty_ui::DropdownSelect,
    pub(crate) active_modal: Option<SettingsModal>,

    // Live hardware/service & system readouts
    pub(crate) network_status: String,
    pub(crate) network: NetworkStatus,
    pub(crate) bluetooth: BluetoothSnapshot,
    pub(crate) bluetooth_status_hint: String,
    pub(crate) power: PowerDetails,
    pub(crate) timedate: TimeDateStatus,
    pub(crate) timedate_hint: String,
    pub(crate) available_timezones: Vec<String>,
    pub(crate) about_info: SystemAboutInfo,
    pub(crate) user_details: Option<CurrentUserDetails>,
    pub(crate) all_users: Vec<UserAccount>,
    pub(crate) volume_percent: u8,
    pub(crate) muted: bool,
    pending_service_cmds: Vec<ServiceCommand>,

    // Software update state
    pub(crate) updates_snapshot: UpdateStatusSnapshot,
    pub(crate) updates_checking: bool,
    pub(crate) updates_tx_phase: Option<UpdateTxPhase>,
    pub(crate) updates_tx_progress: f32,
    pub(crate) updates_tx_summary: Option<String>,
    pub(crate) updates_tx_restart: bool,
    updates_rx: Arc<Mutex<Option<Receiver<UpdateStatusSnapshot>>>>,
    active_tx: Arc<Mutex<Option<UpdateTransactionHandle>>>,
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
        if matches!(opts.initial_page, SettingsPage::About | SettingsPage::Users) {
            state.refresh_static_system_info();
        }
        if opts.initial_page == SettingsPage::SoftwareUpdate {
            state.check_updates_now();
        }
        if let Some(ssid) = &opts.wifi_prompt_ssid {
            state.open_wifi_password_modal(ssid.clone(), None);
        }
        state
    }

    #[allow(clippy::too_many_lines)]
    fn defaults_with_page(initial_page: SettingsPage) -> Self {
        let epoch = Instant::now();
        let (focus, focus_search_id, search_field) = default_search_controls();
        let shortcuts = std::array::from_fn(|i| SHORTCUT_DEFS[i].default_combo.to_owned());
        Self {
            running: true,
            dirty: true,
            damage_scope: DamageScope::Full,
            maximized: false,
            focused: true,
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
            last_caret_blink: epoch,
            epoch,
            search_field,
            focus,
            focus_search_id,
            toggle_anims: Vec::new(),
            reduce_transparency: false,
            reduced_motion: false,
            glass_opacity: 1.0,
            custom_wallpaper: None,
            dock_icon_size: 48,
            dock_position: String::from("bottom"),
            dock_minimize_effect: String::from("genie"),
            dock_minimize_into_app: true,
            dock_visibility: String::from("always_show"),
            dock_auto_hide: false,
            dock_show_indicators: true,
            dock_show_recent: true,
            menubar_workspaces: false,
            menubar_tray: false,
            menubar_input: true,
            menubar_volume: false,
            menubar_wifi: true,
            menubar_battery: true,
            menubar_battery_percentage: false,
            menubar_bluetooth: false,
            menubar_adaptive_contrast: true,
            clock_24h: true,
            clock_locale: String::from("en"),
            updates_enabled: true,
            notif_allow: true,
            notif_show_previews: String::from("When Unlocked"),
            notif_when_sleeping: false,
            notif_when_locked: true,
            notif_mirroring_off: true,
            notif_app_enabled: [true; 5],
            display_scale: 1.0,
            display_outputs: vec![String::from("All Displays")],
            selected_display_idx: 0,
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
            cursor_theme: String::from("RustyOS"),
            cursor_size: 24,
            power_low_power_mode: String::from("never"),
            power_display_sleep_minutes: 10,
            power_auto_lock_minutes: 0,
            power_dim_on_battery: true,
            shortcuts,
            battery_tab: default_battery_tab(),
            battery_tab_animating: false,
            low_power_dropdown: default_low_power_dropdown(),
            active_modal: None,
            network_status: String::from("Checking..."),
            network: NetworkStatus::default(),
            bluetooth: BluetoothSnapshot::default(),
            bluetooth_status_hint: String::new(),
            power: PowerDetails::default(),
            timedate: TimeDateStatus::default(),
            timedate_hint: String::new(),
            available_timezones: Vec::new(),
            about_info: SystemAboutInfo::default(),
            user_details: None,
            all_users: Vec::new(),
            volume_percent: 65,
            muted: false,
            pending_service_cmds: Vec::new(),
            updates_snapshot: UpdateStatusSnapshot::default(),
            updates_checking: false,
            updates_tx_phase: None,
            updates_tx_progress: 0.0,
            updates_tx_summary: None,
            updates_tx_restart: false,
            updates_rx: Arc::new(Mutex::new(None)),
            active_tx: Arc::new(Mutex::new(None)),
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
        self.low_power_dropdown.open = false;
        if matches!(
            page,
            SettingsPage::Wifi
                | SettingsPage::Network
                | SettingsPage::Sound
                | SettingsPage::Bluetooth
                | SettingsPage::Battery
        ) {
            self.pending_service_cmds.push(ServiceCommand::Refresh);
        } else if matches!(page, SettingsPage::About | SettingsPage::Users) {
            self.refresh_static_system_info();
        } else if page == SettingsPage::SoftwareUpdate
            && self.updates_snapshot.state == UpdateCheckState::Idle
        {
            self.check_updates_now();
        }
    }

    pub(crate) fn refresh_static_system_info(&mut self) {
        self.about_info = gather_system_about_info();
        let svc = UserService::new();
        self.user_details = Some(svc.query_current_user_details());
        self.all_users = svc.query_users();
    }

    pub(crate) fn check_updates_now(&mut self) {
        if self.updates_checking {
            return;
        }
        self.updates_checking = true;
        self.updates_snapshot.state = UpdateCheckState::Checking;
        self.mark_dirty(DamageScope::Page);

        let (tx, rx) = mpsc::channel();
        if let Ok(mut lock) = self.updates_rx.lock() {
            *lock = Some(rx);
        }
        std::thread::spawn(move || {
            let snap = check_all_pending_updates();
            let _ = tx.send(snap);
        });
    }

    pub(crate) fn start_update_transaction(&mut self) {
        if self.updates_tx_phase.is_some() {
            return;
        }
        self.updates_tx_phase = Some(UpdateTxPhase::Synchronizing);
        self.updates_tx_progress = 0.0;
        self.mark_dirty(DamageScope::Page);

        let restart = self.updates_snapshot.restart_recommended;
        let preview = self.updates_snapshot.items.clone();
        let handle = UpdateTransactionHandle::start_system_upgrade(false, None, restart, preview);
        if let Ok(mut lock) = self.active_tx.lock() {
            *lock = Some(handle);
        }
    }

    pub(crate) fn poll_updates(&mut self) -> bool {
        let mut changed = false;
        let snap_res = if let Ok(mut lock) = self.updates_rx.lock() {
            if let Some(rx) = lock.as_mut() {
                match rx.try_recv() {
                    Ok(snap) => {
                        *lock = None;
                        Some(Some(snap))
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        *lock = None;
                        Some(None)
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(res) = snap_res {
            if let Some(snap) = res {
                self.updates_snapshot = snap;
            }
            self.updates_checking = false;
            self.mark_dirty(DamageScope::Page);
            changed = true;
        }

        let events = if let Ok(mut lock) = self.active_tx.lock() {
            if let Some(tx) = lock.as_mut() {
                tx.drain_events()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        if !events.is_empty() {
            changed = true;
            self.mark_dirty(DamageScope::Page);
            for event in events {
                match event {
                    UpdateTxEvent::Phase(phase) => {
                        self.updates_tx_phase = Some(phase);
                    }
                    UpdateTxEvent::Progress(prog) => {
                        self.updates_tx_progress = prog;
                    }
                    UpdateTxEvent::Finished {
                        success,
                        summary,
                        restart_recommended,
                    } => {
                        self.updates_tx_phase = if success {
                            Some(UpdateTxPhase::Complete)
                        } else {
                            Some(UpdateTxPhase::Failed)
                        };
                        self.updates_tx_summary = Some(summary);
                        self.updates_tx_restart = restart_recommended;
                    }
                    UpdateTxEvent::LogLine(_) => {}
                }
            }
        }

        if self.updates_tx_phase.is_some_and(|p| {
            matches!(
                p,
                UpdateTxPhase::Complete | UpdateTxPhase::Failed | UpdateTxPhase::Cancelled
            )
        }) && let Ok(mut lock) = self.active_tx.lock()
        {
            *lock = None;
        }

        changed
    }

    fn reset_page_scroll(&mut self) {
        self.page_scroll_y = 0;
        self.page_scroll_f = 0.0;
        self.page_scroll_target = 0.0;
        if (self.sidebar_scroll_target - self.sidebar_scroll_f).abs() < 0.25 {
            self.last_scroll_tick = None;
        }
    }

    pub(crate) fn reset_sidebar_scroll(&mut self) {
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

    pub(crate) fn set_page_scroll_immediate(&mut self, y: i32, max_page_scroll: i32) {
        let clamped = y.clamp(0, max_page_scroll.max(0));
        self.page_scroll_y = clamped;
        self.page_scroll_f = f64::from(clamped);
        self.page_scroll_target = f64::from(clamped);
        self.mark_dirty(DamageScope::Page);
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
        let has_wifi_outcome = snap.wifi_action_result.is_some();
        let tz_changed = self.timedate.timezone != snap.timedate.timezone;
        let next_td_hint = snap.timedate_error.unwrap_or_default();
        let changed = has_wifi_outcome
            || self.network_status != snap.network.status_label
            || self.network != snap.network
            || self.bluetooth != snap.bluetooth
            || self.power != snap.power
            || self.timedate != snap.timedate
            || self.timedate_hint != next_td_hint
            || self.volume_percent != clamped_vol
            || self.muted != snap.muted;
        if !changed {
            return false;
        }
        self.network_status.clone_from(&snap.network.status_label);
        self.network = snap.network;
        self.bluetooth = snap.bluetooth;
        self.bluetooth_status_hint.clear();
        self.power = snap.power;
        self.timedate = snap.timedate;
        self.timedate_hint = next_td_hint;
        self.volume_percent = clamped_vol;
        self.muted = snap.muted;
        if tz_changed {
            notify_compositor_reload();
        }
        if let Some(outcome) = snap.wifi_action_result
            && let Err(err) = outcome.result
        {
            self.network_status.clone_from(&err);
            if outcome.kind == WifiActionKind::Connect {
                self.open_wifi_password_modal(outcome.ssid, Some(err));
            }
            self.mark_dirty(DamageScope::Full);
            return true;
        }
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
        notify_compositor_reload();
        self.mark_dirty(DamageScope::Full);
    }

    pub(crate) fn set_search_focused(&mut self, focused: bool) {
        self.sync_search_field_from_query();
        let elapsed = self.epoch.elapsed();
        self.search_focused = focused;
        self.caret_visible = focused;
        self.search_field.caret_blink.reset(elapsed);
        if focused {
            self.focus
                .set_focus(self.focus_search_id, elapsed, self.reduced_motion);
        } else {
            self.focus.clear_focus(elapsed, self.reduced_motion);
        }
        self.mark_dirty(DamageScope::Sidebar);
    }

    pub(crate) fn sync_search_field_from_query(&mut self) {
        if self.search_field.buffer.as_str() != self.search_query {
            self.search_field.buffer.set_text(&self.search_query);
        }
    }

    pub(crate) fn update_caret_blink(&mut self, _now: Instant) -> bool {
        if !self.search_focused {
            self.caret_visible = false;
            return false;
        }
        let next = self
            .search_field
            .caret_blink
            .is_visible(self.epoch.elapsed());
        if next != self.caret_visible {
            self.caret_visible = next;
            self.last_caret_blink = Instant::now();
            self.mark_dirty(DamageScope::Caret);
            return true;
        }
        false
    }

    pub(crate) fn apply_search_paste(
        &mut self,
        text: &str,
        fonts: &mut rusty_ui::UiFontEngine,
    ) -> bool {
        if !self.search_focused || text.is_empty() {
            return false;
        }
        self.sync_search_field_from_query();
        let mut mem = rusty_ui::MemoryClipboard::new();
        rusty_ui::ClipboardSink::set_text(&mut mem, text);
        let handled = self.search_field.handle_key(
            rusty_ui::TextInputKey::Char('v'),
            rusty_ui::InputModifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
            self.epoch.elapsed(),
            fonts,
            &mut mem,
        );
        self.finish_search_input_event(handled)
    }

    pub(crate) fn handle_search_text_key(
        &mut self,
        key: rusty_ui::TextInputKey,
        mods: rusty_ui::InputModifiers,
        fonts: &mut rusty_ui::UiFontEngine,
        clipboard: &mut dyn rusty_ui::ClipboardSink,
    ) -> bool {
        if !self.search_focused {
            return false;
        }
        self.sync_search_field_from_query();
        let ev = self
            .search_field
            .handle_key(key, mods, self.epoch.elapsed(), fonts, clipboard);
        self.finish_search_input_event(ev)
    }

    fn finish_search_input_event(&mut self, ev: rusty_ui::TextInputEvent) -> bool {
        let mut navigated = false;
        match ev {
            rusty_ui::TextInputEvent::Ignored => return false,
            rusty_ui::TextInputEvent::Cancelled => {
                if self.search_query.is_empty() {
                    self.set_search_focused(false);
                    return true;
                }
                self.search_field.buffer.set_text("");
                self.search_query.clear();
            }
            rusty_ui::TextInputEvent::Submitted => {
                if let Some(first) = self.visible_sidebar_pages().first().copied() {
                    self.navigate_to(first);
                    navigated = true;
                }
            }
            rusty_ui::TextInputEvent::Changed => {
                let s = self.search_field.buffer.as_str();
                let mut trimmed = String::new();
                for ch in s.chars().take(32) {
                    trimmed.push(ch);
                }
                if trimmed.len() != s.len() {
                    self.search_field.buffer.set_text(&trimmed);
                }
                self.search_query = trimmed;
            }
            rusty_ui::TextInputEvent::CaretMoved => {}
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

    pub(crate) fn sync_toggles_from_layout(&mut self, layout: &crate::layout::SettingsLayout) {
        let elapsed = self.epoch.elapsed();
        for sec in &layout.sections {
            for row in &sec.rows {
                let (action, active) = match &row.control {
                    crate::layout::CardRowControl::Switch { action, active }
                    | crate::layout::CardRowControl::WifiHeaderSwitch { action, active, .. }
                    | crate::layout::CardRowControl::NotificationHeader {
                        action, active, ..
                    }
                    | crate::layout::CardRowControl::NotificationAppRow {
                        action, active, ..
                    } => (*action, *active),
                    _ => continue,
                };
                if let Some((_, m)) = self.toggle_anims.iter_mut().find(|(id, _)| *id == action) {
                    m.set_active(active, elapsed, self.reduced_motion);
                } else {
                    self.toggle_anims
                        .push((action, rusty_ui::ToggleMotion::new(active)));
                }
            }
        }
    }

    #[must_use]
    pub(crate) fn switch_progress(&self, action: RowControlId, active: bool) -> f32 {
        self.toggle_anims
            .iter()
            .find(|(id, _)| *id == action)
            .map_or(if active { 1.0 } else { 0.0 }, |(_, m)| m.progress())
    }

    pub(crate) fn step_ui_animations(&mut self, _now: Instant) -> bool {
        let elapsed = self.epoch.elapsed();
        let mut changed = DamageScope::None;
        if self.focus.tick(elapsed) {
            changed = changed.merge(DamageScope::Sidebar);
        }
        let tab_active = self.battery_tab.tick(elapsed);
        self.battery_tab_animating = tab_active;
        let mut page_anim = tab_active;
        for (_, m) in &mut self.toggle_anims {
            if m.tick(elapsed) {
                page_anim = true;
            }
        }
        if page_anim {
            changed = changed.merge(DamageScope::Page);
        }
        if changed != DamageScope::None {
            self.mark_dirty(changed);
            return true;
        }
        false
    }

    #[must_use]
    pub(crate) fn is_ui_animating(&self) -> bool {
        let a = self.focus.ring_alpha();
        self.is_scroll_animating()
            || self.battery_tab_animating
            || (a > 0.01 && a < 0.99)
            || self.toggle_anims.iter().any(|(_, m)| m.is_active())
    }
}
