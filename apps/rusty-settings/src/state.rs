//! State model, TOML configuration synchronization, and IPC live-reload for `rusty-settings` (`RUSTYOS-103`).

use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use rusty_services::file_manager::RUSTYOS_ACCENT_SWATCHES;
use rusty_services::updates::{
    UpdateCheckState, UpdateStatusSnapshot, UpdateTransactionHandle, UpdateTxEvent, UpdateTxPhase,
    check_all_pending_updates,
};
use rusty_services::{
    AudioService, BluetoothService, BluetoothSnapshot, COMMON_TIMEZONES, CurrentUserDetails,
    NetworkService, NetworkStatus, PowerDetails, PowerProfile, PowerService, TimeDateService,
    TimeDateStatus, UserAccount, UserService, is_valid_iana_timezone, match_timezone_suggestions,
    parse_manual_datetime_input,
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

    fn try_apply_wifi_service_action(&mut self, action: RowControlId) -> bool {
        match action {
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
                if ap.connected {
                    self.network_status = format!("Disconnecting from {}...", ap.ssid);
                    self.pending_service_cmds
                        .push(ServiceCommand::DisconnectWifi(ap.ssid));
                } else if ap.secure && !ap.known {
                    self.open_wifi_password_modal(ap.ssid, None);
                } else {
                    self.network_status = format!("Connecting to {}...", ap.ssid);
                    self.pending_service_cmds
                        .push(ServiceCommand::ConnectWifi(ap.ssid));
                }
            }
            RowControlId::WifiInfoIndex(idx) => {
                let Some(ap) = self.network.wifi_networks.get(idx).cloned() else {
                    return true;
                };
                if ap.known || ap.connected {
                    self.open_wifi_info_modal(&ap);
                } else if ap.secure {
                    self.open_wifi_password_modal(ap.ssid, None);
                } else {
                    self.network_status = format!("Connecting to {}...", ap.ssid);
                    self.pending_service_cmds
                        .push(ServiceCommand::ConnectWifi(ap.ssid));
                }
            }
            RowControlId::WifiDisconnectActive => {
                if let Some(ssid) = self.network.wifi_ssid.clone() {
                    self.network_status = format!("Disconnecting from {ssid}...");
                    self.pending_service_cmds
                        .push(ServiceCommand::DisconnectWifi(ssid));
                }
            }
            RowControlId::NetworkRefresh => {
                self.network_status = String::from("Scanning nearby Wi-Fi networks...");
                self.pending_service_cmds.push(ServiceCommand::RescanWifi);
            }
            _ => return false,
        }
        true
    }

    fn try_apply_timedate_service_action(&mut self, action: RowControlId) -> bool {
        match action {
            RowControlId::DateTimeNtpToggle => {
                let next = !self.timedate.ntp_enabled;
                self.timedate.ntp_enabled = next;
                self.timedate_hint = String::from(if next {
                    "Enabling NTP synchronization..."
                } else {
                    "Disabling NTP synchronization..."
                });
                self.pending_service_cmds
                    .push(ServiceCommand::SetNtpEnabled(next));
            }
            RowControlId::DateTimeTimezoneOpen => self.open_timezone_modal(),
            RowControlId::DateTimeTimezoneCycle => {
                let idx = COMMON_TIMEZONES
                    .iter()
                    .position(|z| z.eq_ignore_ascii_case(&self.timedate.timezone))
                    .map_or(0, |i| (i + 1) % COMMON_TIMEZONES.len());
                let next = String::from(COMMON_TIMEZONES[idx]);
                self.timedate.timezone.clone_from(&next);
                self.timedate_hint = format!("Setting timezone to {next}...");
                self.pending_service_cmds
                    .push(ServiceCommand::SetTimezone(next));
            }
            RowControlId::DateTimeManualOpen => {
                if self.timedate.ntp_enabled {
                    self.timedate_hint =
                        String::from("Turn off Automatic Date & Time before setting time manually");
                } else {
                    self.open_manual_datetime_modal();
                }
            }
            _ => return false,
        }
        true
    }

    fn try_apply_service_action(&mut self, action: RowControlId) -> bool {
        if self.try_apply_wifi_service_action(action)
            || self.try_apply_timedate_service_action(action)
        {
            return true;
        }
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
            RowControlId::BluetoothPoweredToggle => {
                let next = !self.bluetooth.status.powered;
                self.bluetooth.status.powered = next;
                if !next {
                    self.bluetooth.discovering = false;
                }
                self.bluetooth_status_hint = String::from(if next {
                    "Turning on..."
                } else {
                    "Turning off..."
                });
                self.pending_service_cmds
                    .push(ServiceCommand::SetBluetoothPowered(next));
            }
            RowControlId::BluetoothScanToggle => {
                let next = !self.bluetooth.discovering;
                self.bluetooth.discovering = next;
                self.bluetooth_status_hint = String::from(if next {
                    "Scanning for devices..."
                } else {
                    "Stopping scan..."
                });
                self.pending_service_cmds
                    .push(ServiceCommand::SetBluetoothDiscovering(next));
            }
            RowControlId::BluetoothActionIndex(idx) => {
                self.trigger_bluetooth_device_action(idx);
            }
            RowControlId::PowerProfileCycle => {
                let next = match self.power.active_profile {
                    Some(PowerProfile::Balanced) | None => PowerProfile::Performance,
                    Some(PowerProfile::Performance) => PowerProfile::LowPower,
                    Some(PowerProfile::LowPower) => PowerProfile::Balanced,
                };
                self.power.active_profile = Some(next);
                self.pending_service_cmds
                    .push(ServiceCommand::SetPowerProfile(next));
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
            RowControlId::ToggleWallpaper => match self.custom_wallpaper.as_deref() {
                None | Some("rustyos-dynamic") => {
                    let next = String::from("rustyos-light");
                    self.custom_wallpaper = Some(next.clone());
                    let _ = update_config_entries(&[("ui", "wallpaper", &next)]);
                }
                Some("rustyos-light") => {
                    let next = String::from("rustyos-dark");
                    self.custom_wallpaper = Some(next.clone());
                    let _ = update_config_entries(&[("ui", "wallpaper", &next)]);
                }
                _ => {
                    self.custom_wallpaper = None;
                    let _ = update_config_entries(&[("ui", "wallpaper", "")]);
                }
            },
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
            RowControlId::DockVisibilityCycle => {
                let next = match self.dock_visibility.as_str() {
                    "always_show" => "smart_hide",
                    "smart_hide" => "always_hide",
                    _ => "always_show",
                };
                next.clone_into(&mut self.dock_visibility);
                self.dock_auto_hide = next == "always_hide";
                let auto_v = bool_str(self.dock_auto_hide);
                let _ = update_config_entries(&[
                    ("dock", "visibility", &self.dock_visibility),
                    ("dock", "auto_hide", auto_v),
                ]);
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

    #[allow(clippy::too_many_lines)]
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
                let _ = update_config_entries(&[
                    ("ui", "menubar_show_volume", v),
                    ("top_bar", "show_sound", v),
                ]);
            }
            RowControlId::MenubarWifiToggle => {
                self.menubar_wifi = !self.menubar_wifi;
                let v = bool_str(self.menubar_wifi);
                let _ = update_config_entries(&[("top_bar", "show_wifi", v)]);
            }
            RowControlId::MenubarBatteryToggle => {
                self.menubar_battery = !self.menubar_battery;
                let v = bool_str(self.menubar_battery);
                let _ = update_config_entries(&[("top_bar", "show_battery", v)]);
            }
            RowControlId::MenubarBatteryPercentageToggle => {
                self.menubar_battery_percentage = !self.menubar_battery_percentage;
                let v = bool_str(self.menubar_battery_percentage);
                let _ = update_config_entries(&[("top_bar", "show_battery_percentage", v)]);
            }
            RowControlId::MenubarBluetoothToggle => {
                self.menubar_bluetooth = !self.menubar_bluetooth;
                let v = bool_str(self.menubar_bluetooth);
                let _ = update_config_entries(&[("top_bar", "show_bluetooth", v)]);
            }
            RowControlId::MenubarAdaptiveContrastToggle => {
                self.menubar_adaptive_contrast = !self.menubar_adaptive_contrast;
                let v = bool_str(self.menubar_adaptive_contrast);
                let _ = update_config_entries(&[("top_bar", "adaptive_contrast", v)]);
            }
            RowControlId::Clock24hToggle => {
                self.clock_24h = !self.clock_24h;
                let v = bool_str(self.clock_24h);
                let _ =
                    update_config_entries(&[("ui", "clock_24h", v), ("top_bar", "clock_24h", v)]);
            }
            RowControlId::ClockLocaleCycle => {
                let next = match self.clock_locale.as_str() {
                    "en" => "ru",
                    "ru" => "uk",
                    _ => "en",
                };
                next.clone_into(&mut self.clock_locale);
                let _ = update_config_entries(&[("top_bar", "clock_locale", next)]);
            }
            RowControlId::UpdatesEnabledToggle => {
                self.updates_enabled = !self.updates_enabled;
                let v = bool_str(self.updates_enabled);
                let _ = update_config_entries(&[("updates", "enabled", v)]);
            }
            RowControlId::UpdatesCheckNow => {
                self.check_updates_now();
            }
            RowControlId::UpdatesInstallNow => {
                self.start_update_transaction();
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
                match self.cursor_theme.as_str() {
                    "RustyOS" => "Adwaita",
                    "Adwaita" => "Bibata-Modern-Classic",
                    _ => "RustyOS",
                }
                .clone_into(&mut self.cursor_theme);
                let _ = update_config_entries(&[("ui", "cursor_theme", &self.cursor_theme)]);
            }
            RowControlId::CursorSizeDown => {
                self.cursor_size = self.cursor_size.saturating_sub(4).clamp(16, 64);
                let v = self.cursor_size.to_string();
                let _ = update_config_entries(&[("ui", "cursor_size", &v)]);
            }
            RowControlId::CursorSizeUp => {
                self.cursor_size = (self.cursor_size + 4).clamp(16, 64);
                let v = self.cursor_size.to_string();
                let _ = update_config_entries(&[("ui", "cursor_size", &v)]);
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
            RowControlId::PowerDisplaySleepCycle => {
                self.power_display_sleep_minutes = match self.power_display_sleep_minutes {
                    2 => 5,
                    5 => 10,
                    10 => 15,
                    15 => 30,
                    30 => 60,
                    60 => 0,
                    _ => 2,
                };
                let v = self.power_display_sleep_minutes.to_string();
                let _ = update_config_entries(&[("power", "display_sleep_minutes", &v)]);
            }
            RowControlId::PowerAutoLockCycle => {
                self.power_auto_lock_minutes = match self.power_auto_lock_minutes {
                    0 => 1,
                    1 => 5,
                    5 => 15,
                    15 => 30,
                    _ => 0,
                };
                let v = self.power_auto_lock_minutes.to_string();
                let _ = update_config_entries(&[("power", "auto_lock_minutes", &v)]);
            }
            RowControlId::PowerDimOnBatteryToggle => {
                self.power_dim_on_battery = !self.power_dim_on_battery;
                let v = bool_str(self.power_dim_on_battery);
                let _ = update_config_entries(&[("power", "dim_on_battery", v)]);
            }
            RowControlId::ShortcutsResetDefaults => {
                self.shortcuts = std::array::from_fn(|i| SHORTCUT_DEFS[i].default_combo.to_owned());
                let pairs: Vec<(&str, &str, &str)> = SHORTCUT_DEFS
                    .iter()
                    .map(|d| ("shortcuts", d.key, d.default_combo))
                    .collect();
                let _ = update_config_entries(&pairs);
            }
            RowControlId::BluetoothInfoIndex(idx) => self.open_bluetooth_info_modal(idx),
            RowControlId::PowerOptionsOpen | RowControlId::BatteryDetailsOpen => {
                self.open_battery_details_modal();
            }
            RowControlId::AboutMoreInfoOpen => self.open_about_more_info_modal(),
            RowControlId::ShortcutEditIndex(idx) => self.open_shortcut_edit_modal(idx),
            _ => {}
        }
    }

    pub(crate) fn select_battery_tab(&mut self, index: usize) {
        let elapsed = self.epoch.elapsed();
        if self.battery_tab.select(index, elapsed, self.reduced_motion) {
            self.battery_tab_animating = !self.reduced_motion;
            self.mark_dirty(DamageScope::Page);
        }
    }

    pub(crate) fn select_low_power_mode(&mut self, index: usize) {
        let mode_str = match index {
            1 => "battery",
            2 => "always",
            3 => "ac",
            _ => "never",
        };
        self.low_power_dropdown.selected = index.min(3);
        self.low_power_dropdown.open = false;
        self.power_low_power_mode = String::from(mode_str);
        let _ = update_config_entries(&[("power", "low_power_mode", mode_str)]);
        self.last_config_mtime = current_config_mtime();
        notify_compositor_reload();

        let desired = match mode_str {
            "always" => PowerProfile::LowPower,
            "battery" if self.power.status.has_battery && !self.power.status.ac_online => {
                PowerProfile::LowPower
            }
            "ac" if self.power.status.ac_online || !self.power.status.has_battery => {
                PowerProfile::LowPower
            }
            _ => PowerProfile::Balanced,
        };
        if !self.power.available_profiles.is_empty() {
            self.power.active_profile = Some(desired);
            self.pending_service_cmds
                .push(ServiceCommand::SetPowerProfile(desired));
        }
        self.mark_dirty(DamageScope::Page);
    }

    fn trigger_bluetooth_device_action(&mut self, idx: usize) {
        let Some(dev) = self.bluetooth.devices.get(idx).cloned() else {
            return;
        };
        if dev.connected {
            self.bluetooth_status_hint = format!("Disconnecting {}...", dev.name);
            self.pending_service_cmds
                .push(ServiceCommand::DisconnectBluetooth(dev.address));
        } else if dev.paired {
            self.bluetooth_status_hint = format!("Connecting {}...", dev.name);
            self.pending_service_cmds
                .push(ServiceCommand::ConnectBluetooth(dev.address));
        } else {
            self.bluetooth_status_hint = format!("Pairing {}...", dev.name);
            self.pending_service_cmds
                .push(ServiceCommand::PairBluetooth(dev.address));
        }
    }

    fn open_bluetooth_info_modal(&mut self, idx: usize) {
        let Some(dev) = self.bluetooth.devices.get(idx).cloned() else {
            return;
        };
        let mut msg = format!("MAC: {}", dev.address);
        if dev.paired {
            msg.push_str(" · Paired");
        }
        if dev.connected {
            msg.push_str(" · Connected");
        }
        if let Some(pct) = dev.battery_percent {
            use std::fmt::Write as _;
            let _ = write!(msg, " · Battery {pct}%");
        }
        let mut sheet = rusty_ui::DialogSheet::confirm(
            dev.name,
            msg,
            if dev.paired { "Forget" } else { "Pair" },
        )
        .with_destructive(dev.paired);
        sheet.cancel_label = Some(String::from("Close"));
        sheet.width = 400;
        self.active_modal = Some(SettingsModal::BluetoothDeviceInfo {
            address: dev.address,
            paired: dev.paired,
            connected: dev.connected,
            sheet,
        });
        self.mark_dirty(DamageScope::Full);
    }

    fn open_battery_details_modal(&mut self) {
        let cond = self.power.condition.label();
        let cap = self
            .power
            .health_percent
            .map_or_else(|| String::from("N/A"), |p| format!("{p}%"));
        let cycles = self
            .power
            .cycle_count
            .map_or_else(|| String::from("N/A"), |c| c.to_string());
        let msg = format!(
            "Condition: {cond} · Max Capacity: {cap} · Cycles: {cycles} · Sleep {} min",
            self.power_display_sleep_minutes
        );
        let mut sheet = rusty_ui::DialogSheet::confirm("Battery & Power Options", msg, "Done");
        sheet.cancel_label = None;
        sheet.width = 440;
        self.active_modal = Some(SettingsModal::InfoSheet { sheet });
        self.mark_dirty(DamageScope::Full);
    }

    fn open_about_more_info_modal(&mut self) {
        let msg = format!(
            "{} · {} ({}) · {}",
            self.about_info.os_name,
            self.about_info.kernel,
            self.about_info.architecture,
            self.about_info.cpu
        );
        let mut sheet = rusty_ui::DialogSheet::confirm("About This System", msg, "Done");
        sheet.cancel_label = None;
        sheet.width = 460;
        self.active_modal = Some(SettingsModal::InfoSheet { sheet });
        self.mark_dirty(DamageScope::Full);
    }

    fn open_shortcut_edit_modal(&mut self, index: usize) {
        let Some(def) = SHORTCUT_DEFS.get(index) else {
            return;
        };
        let current = self.shortcuts[index].clone();
        let mut sheet = rusty_ui::DialogSheet::prompt(
            format!("Shortcut: {}", def.label),
            format!(
                "Default: {} · Type Super/Alt/Ctrl/Shift + Key",
                def.default_combo
            ),
            "Save",
            "e.g. Super+Space",
            current.clone(),
        );
        sheet.width = 420;
        let mut modal = SettingsModal::ShortcutEdit {
            index,
            conflict_index: None,
            validated_combo: Some(current),
            sheet,
        };
        self.refresh_shortcut_modal_validation(&mut modal);
        self.active_modal = Some(modal);
        self.mark_dirty(DamageScope::Full);
    }

    fn refresh_shortcut_modal_validation(&self, modal: &mut SettingsModal) {
        let SettingsModal::ShortcutEdit {
            index,
            conflict_index,
            validated_combo,
            sheet,
        } = modal
        else {
            return;
        };
        let raw = sheet.input.as_ref().map_or("", |tf| tf.buffer.as_str());
        match validate_shortcut_input(raw) {
            Ok(canon) => {
                let conflict = self
                    .shortcuts
                    .iter()
                    .enumerate()
                    .find(|(i, s)| *i != *index && s.eq_ignore_ascii_case(&canon))
                    .map(|(i, _)| i);
                *conflict_index = conflict;
                *validated_combo = Some(canon.clone());
                if let Some(ci) = conflict {
                    sheet.message = format!(
                        "Conflicts with \"{}\" · Save swaps both",
                        SHORTCUT_DEFS[ci].label
                    );
                } else {
                    sheet.message = format!(
                        "Valid: {canon} · Default: {}",
                        SHORTCUT_DEFS[*index].default_combo
                    );
                }
            }
            Err(err) => {
                *conflict_index = None;
                *validated_combo = None;
                sheet.message = err.to_owned();
            }
        }
    }

    pub(crate) fn open_wifi_password_modal(&mut self, ssid: String, error: Option<String>) {
        let message =
            error.unwrap_or_else(|| format!("Enter the WPA2/WPA3 password for \"{ssid}\"."));
        let mut sheet = rusty_ui::DialogSheet::prompt(
            format!("Join \"{ssid}\""),
            message,
            "Join",
            "Password",
            "",
        );
        sheet.width = 430;
        if let Some(tf) = sheet.input.as_mut() {
            tf.masked = true;
            tf.buffer.clear_selection();
        }
        self.active_modal = Some(SettingsModal::WifiPassword {
            ssid,
            show_password: false,
            sheet,
        });
        self.mark_dirty(DamageScope::Full);
    }

    fn open_wifi_info_modal(&mut self, ap: &rusty_services::WifiAccessPoint) {
        let state_label = if ap.connected { "Connected" } else { "Saved" };
        let sec_label = if ap.secure { "WPA2/WPA3" } else { "Open" };
        let msg = format!(
            "{state_label} · {sec_label} · Signal {}%",
            ap.signal_percent
        );
        let mut sheet =
            rusty_ui::DialogSheet::confirm(ap.ssid.clone(), msg, "Forget").with_destructive(true);
        sheet.cancel_label = Some(String::from("Close"));
        sheet.width = 400;
        self.active_modal = Some(SettingsModal::WifiNetworkInfo {
            ssid: ap.ssid.clone(),
            connected: ap.connected,
            sheet,
        });
        self.mark_dirty(DamageScope::Full);
    }

    pub(crate) fn open_timezone_modal(&mut self) {
        if self.available_timezones.is_empty() {
            self.available_timezones = TimeDateService::new().list_timezones();
        }
        let current = self.timedate.timezone.clone();
        let mut sheet = rusty_ui::DialogSheet::prompt(
            "Set Time Zone",
            format!("Current: {current} · Enter IANA zone (e.g. Europe/Kyiv)"),
            "Apply",
            "Europe/Kyiv",
            current,
        );
        sheet.width = 450;
        let mut modal = SettingsModal::TimezoneEdit { sheet };
        self.refresh_modal_validation(&mut modal);
        self.active_modal = Some(modal);
        self.mark_dirty(DamageScope::Full);
    }

    pub(crate) fn open_manual_datetime_modal(&mut self) {
        let now_seed = current_local_iso_datetime_seed();
        let mut sheet = rusty_ui::DialogSheet::prompt(
            "Set Date & Time",
            "Format: YYYY-MM-DD HH:MM (local time)",
            "Set",
            "2026-11-23 15:30",
            now_seed,
        );
        sheet.width = 430;
        let mut modal = SettingsModal::ManualDateTime { sheet };
        self.refresh_modal_validation(&mut modal);
        self.active_modal = Some(modal);
        self.mark_dirty(DamageScope::Full);
    }

    fn refresh_modal_validation(&self, modal: &mut SettingsModal) {
        match modal {
            SettingsModal::ShortcutEdit { .. } => self.refresh_shortcut_modal_validation(modal),
            SettingsModal::TimezoneEdit { sheet } => {
                let raw = sheet.input.as_ref().map_or("", |tf| tf.buffer.as_str());
                let trimmed = raw.trim();
                let suggestions = match_timezone_suggestions(trimmed, &self.available_timezones, 3);
                if trimmed.is_empty() {
                    sheet.message = format!("Presets: {}", suggestions.join(", "));
                } else if is_valid_iana_timezone(trimmed)
                    && self.available_timezones.iter().any(|z| z == trimmed)
                {
                    sheet.message = format!("Valid IANA timezone: {trimmed}");
                } else if !suggestions.is_empty() {
                    sheet.message = format!("Matches: {}", suggestions.join(", "));
                } else {
                    sheet.message = String::from("No matching IANA timezone (e.g. Europe/Kyiv)");
                }
            }
            SettingsModal::ManualDateTime { sheet } => {
                let raw = sheet.input.as_ref().map_or("", |tf| tf.buffer.as_str());
                sheet.message = match parse_manual_datetime_input(raw) {
                    Ok(canon) => format!("Valid local time: {canon}"),
                    Err(err) => err,
                };
            }
            SettingsModal::BluetoothDeviceInfo { .. }
            | SettingsModal::WifiPassword { .. }
            | SettingsModal::WifiNetworkInfo { .. }
            | SettingsModal::InfoSheet { .. } => {}
        }
    }

    pub(crate) fn toggle_wifi_password_visibility(&mut self) {
        if let Some(SettingsModal::WifiPassword {
            show_password,
            sheet,
            ..
        }) = &mut self.active_modal
        {
            *show_password = !*show_password;
            if let Some(tf) = sheet.input.as_mut() {
                tf.masked = !*show_password;
            }
            self.mark_dirty(DamageScope::Full);
        }
    }

    pub(crate) fn dismiss_active_modal(&mut self) {
        if self.active_modal.take().is_some() {
            self.mark_dirty(DamageScope::Full);
        }
    }

    pub(crate) fn confirm_active_modal(&mut self) {
        let Some(modal) = self.active_modal.take() else {
            return;
        };
        match modal {
            SettingsModal::BluetoothDeviceInfo {
                address,
                paired,
                connected,
                ..
            } => {
                let _ = connected;
                let cmd = if paired {
                    ServiceCommand::RemoveBluetooth(address)
                } else {
                    ServiceCommand::PairBluetooth(address)
                };
                self.pending_service_cmds.push(cmd);
            }
            SettingsModal::WifiPassword {
                ssid,
                show_password,
                mut sheet,
            } => {
                let password = sheet
                    .input
                    .as_ref()
                    .map_or_default(|tf| tf.buffer.as_str().to_owned());
                if password.is_empty() {
                    sheet.message = String::from("Please enter the Wi-Fi password.");
                    self.active_modal = Some(SettingsModal::WifiPassword {
                        ssid,
                        show_password,
                        sheet,
                    });
                    self.mark_dirty(DamageScope::Full);
                    return;
                }
                self.network_status = format!("Connecting to {ssid}...");
                self.pending_service_cmds
                    .push(ServiceCommand::ConnectWifiWithPassword { ssid, password });
            }
            SettingsModal::WifiNetworkInfo {
                ssid, connected, ..
            } => {
                let _ = connected;
                self.network_status = format!("Forgetting {ssid}...");
                self.pending_service_cmds
                    .push(ServiceCommand::ForgetWifi(ssid));
            }
            SettingsModal::InfoSheet { .. } => {}
            SettingsModal::ShortcutEdit {
                index,
                conflict_index,
                validated_combo,
                sheet,
            } => {
                let Some(canon) = validated_combo else {
                    let mut restored = SettingsModal::ShortcutEdit {
                        index,
                        conflict_index,
                        validated_combo: None,
                        sheet,
                    };
                    self.refresh_shortcut_modal_validation(&mut restored);
                    self.active_modal = Some(restored);
                    self.mark_dirty(DamageScope::Full);
                    return;
                };
                let old_combo = self.shortcuts[index].clone();
                self.shortcuts[index].clone_from(&canon);
                if let Some(ci) = conflict_index {
                    self.shortcuts[ci].clone_from(&old_combo);
                    let _ = update_config_entries(&[
                        ("shortcuts", SHORTCUT_DEFS[index].key, &canon),
                        ("shortcuts", SHORTCUT_DEFS[ci].key, &old_combo),
                    ]);
                } else {
                    let _ =
                        update_config_entries(&[("shortcuts", SHORTCUT_DEFS[index].key, &canon)]);
                }
                self.last_config_mtime = current_config_mtime();
                notify_compositor_reload();
            }
            SettingsModal::TimezoneEdit { sheet } => self.confirm_timezone_modal(sheet),
            SettingsModal::ManualDateTime { sheet } => self.confirm_manual_datetime_modal(sheet),
        }
        self.mark_dirty(DamageScope::Full);
    }

    fn confirm_timezone_modal(&mut self, mut sheet: rusty_ui::DialogSheet) {
        let raw = sheet
            .input
            .as_ref()
            .map_or_default(|tf| tf.buffer.as_str().trim().to_owned());
        let resolved = if is_valid_iana_timezone(&raw)
            && (self.available_timezones.is_empty()
                || self.available_timezones.iter().any(|z| z == &raw))
        {
            Some(raw)
        } else {
            let matches = match_timezone_suggestions(&raw, &self.available_timezones, 2);
            (matches.len() == 1).then(|| matches[0].clone())
        };
        let Some(tz) = resolved else {
            sheet.message = String::from("Enter an exact IANA timezone (e.g. Europe/Kyiv)");
            self.active_modal = Some(SettingsModal::TimezoneEdit { sheet });
            return;
        };
        self.timedate.timezone.clone_from(&tz);
        self.timedate_hint = format!("Setting timezone to {tz}...");
        self.pending_service_cmds
            .push(ServiceCommand::SetTimezone(tz));
    }

    fn confirm_manual_datetime_modal(&mut self, mut sheet: rusty_ui::DialogSheet) {
        let raw = sheet
            .input
            .as_ref()
            .map_or_default(|tf| tf.buffer.as_str().trim().to_owned());
        match parse_manual_datetime_input(&raw) {
            Ok(canonical) => {
                self.timedate_hint = format!("Setting time to {canonical}...");
                self.pending_service_cmds
                    .push(ServiceCommand::SetManualDateTime(canonical));
            }
            Err(err) => {
                sheet.message = err;
                self.active_modal = Some(SettingsModal::ManualDateTime { sheet });
            }
        }
    }

    pub(crate) fn handle_modal_text_key(
        &mut self,
        key: rusty_ui::TextInputKey,
        mods: rusty_ui::InputModifiers,
        fonts: &mut rusty_ui::UiFontEngine,
        clipboard: &mut dyn rusty_ui::ClipboardSink,
    ) -> bool {
        if key == rusty_ui::TextInputKey::Escape {
            self.dismiss_active_modal();
            return true;
        }
        let Some(mut modal) = self.active_modal.take() else {
            return false;
        };
        let elapsed = self.epoch.elapsed();
        let ev = if let Some(tf) = modal.sheet_mut().input.as_mut() {
            tf.handle_key(key, mods, elapsed, fonts, clipboard)
        } else if key == rusty_ui::TextInputKey::Enter {
            rusty_ui::TextInputEvent::Submitted
        } else {
            rusty_ui::TextInputEvent::Ignored
        };

        match ev {
            rusty_ui::TextInputEvent::Submitted => {
                self.active_modal = Some(modal);
                self.confirm_active_modal();
                return true;
            }
            rusty_ui::TextInputEvent::Cancelled => {
                self.mark_dirty(DamageScope::Full);
                return true;
            }
            rusty_ui::TextInputEvent::Changed => {
                self.refresh_modal_validation(&mut modal);
            }
            rusty_ui::TextInputEvent::CaretMoved | rusty_ui::TextInputEvent::Ignored => {}
        }
        self.active_modal = Some(modal);
        self.mark_dirty(DamageScope::Full);
        true
    }

    pub(crate) fn handle_modal_paste(
        &mut self,
        text: &str,
        fonts: &mut rusty_ui::UiFontEngine,
    ) -> bool {
        if text.is_empty() {
            return false;
        }
        let Some(mut modal) = self.active_modal.take() else {
            return false;
        };
        if let Some(tf) = modal.sheet_mut().input.as_mut() {
            let mut mem = rusty_ui::MemoryClipboard::new();
            rusty_ui::ClipboardSink::set_text(&mut mem, text);
            let _ = tf.handle_key(
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
            self.refresh_modal_validation(&mut modal);
            self.active_modal = Some(modal);
            self.mark_dirty(DamageScope::Full);
            return true;
        }
        self.active_modal = Some(modal);
        false
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
        let mut explicit_dock_visibility: Option<String> = None;
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
            if sec == "dock"
                && key == "visibility"
                && let Some(norm) = normalize_dock_visibility(val)
            {
                explicit_dock_visibility = Some(norm.to_owned());
            }
            self.apply_toml_kv(sec, key, val);
        }
        if auto_mode_seen {
            self.theme_choice = ThemeChoice::Auto;
        }
        if let Some(vis) = explicit_dock_visibility {
            self.dock_auto_hide = vis == "always_hide";
            self.dock_visibility = vis;
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
            ("ui", "menubar_show_volume") | ("top_bar", "show_sound") => {
                self.menubar_volume = parse_bool(val);
            }
            ("top_bar", "show_wifi") => self.menubar_wifi = parse_bool(val),
            ("top_bar", "show_battery") => self.menubar_battery = parse_bool(val),
            ("top_bar", "show_battery_percentage") => {
                self.menubar_battery_percentage = parse_bool(val);
            }
            ("top_bar", "show_bluetooth") => self.menubar_bluetooth = parse_bool(val),
            ("top_bar", "adaptive_contrast") => self.menubar_adaptive_contrast = parse_bool(val),
            ("ui" | "top_bar", "clock_24h") => self.clock_24h = parse_bool(val),
            ("top_bar", "clock_locale") => {
                let norm = val.trim().trim_matches('"').to_ascii_lowercase();
                if matches!(norm.as_str(), "en" | "ru" | "uk") {
                    self.clock_locale = norm;
                }
            }
            ("ui", "cursor_theme") if !val.is_empty() => {
                val.trim()
                    .trim_matches('"')
                    .clone_into(&mut self.cursor_theme);
            }
            ("ui", "cursor_size") => {
                if let Ok(v) = val.trim().parse::<u32>() {
                    self.cursor_size = v.clamp(16, 64);
                }
            }
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
            ("dock", "visibility") => {
                if let Some(norm) = normalize_dock_visibility(val) {
                    norm.clone_into(&mut self.dock_visibility);
                    self.dock_auto_hide = norm == "always_hide";
                }
            }
            ("dock", "auto_hide") => {
                self.dock_auto_hide = parse_bool(val);
                let vis = if self.dock_auto_hide {
                    "always_hide"
                } else {
                    "always_show"
                };
                vis.clone_into(&mut self.dock_visibility);
            }
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
            ("power", "low_power_mode") => {
                let (idx, norm) = match val.trim().to_ascii_lowercase().as_str() {
                    "battery" | "only_on_battery" | "on_battery" => (1, "battery"),
                    "always" | "on" => (2, "always"),
                    "ac" | "adapter" | "only_on_ac" | "only_on_adapter" => (3, "ac"),
                    _ => (0, "never"),
                };
                self.power_low_power_mode = String::from(norm);
                self.low_power_dropdown.selected = idx;
            }
            ("power", "display_sleep_minutes") => {
                if let Ok(v) = val.parse::<u32>() {
                    self.power_display_sleep_minutes = v.min(180);
                }
            }
            ("power", "auto_lock_minutes") => {
                if let Ok(v) = val.parse::<u32>() {
                    self.power_auto_lock_minutes = v.min(120);
                }
            }
            ("power", "dim_on_battery") => self.power_dim_on_battery = parse_bool(val),
            ("shortcuts", key) => {
                if let Some(idx) = SHORTCUT_DEFS.iter().position(|d| d.key == key)
                    && let Ok(canon) = validate_shortcut_input(val)
                {
                    self.shortcuts[idx] = canon;
                }
            }
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

fn normalize_dock_visibility(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "always_show" | "always-show" | "show" | "never" => Some("always_show"),
        "smart_hide" | "smart-hide" | "smart" | "intellihide" | "intell_hide"
        | "when_obstructed" | "when-obstructed" | "dodge" => Some("smart_hide"),
        "always_hide" | "always-hide" | "hide" | "always" => Some("always_hide"),
        _ => None,
    }
}

pub(crate) fn dock_visibility_label(raw: &str) -> &'static str {
    match normalize_dock_visibility(raw).unwrap_or("always_show") {
        "smart_hide" => "Smart Hide",
        "always_hide" => "Always Hide",
        _ => "Always Show",
    }
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
    std::thread::spawn(move || {
        let Ok(mut stream) = UnixStream::connect(sock_path) else {
            return;
        };
        let timeout = Some(Duration::from_millis(120));
        let _ = stream.set_read_timeout(timeout);
        let _ = stream.set_write_timeout(timeout);
        let _ = stream.write_all(b"v1|RELOAD_CONFIG\n");
        let _ = stream.flush();
    });
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

fn current_local_iso_datetime_seed() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_default(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// Queries live NetworkManager, BlueZ, power, timedated, and audio status (`RUSTYOS-112`, `RUSTYOS-131`, `RUSTYOS-160`).
#[must_use]
pub(crate) fn query_service_snapshot() -> ServiceSnapshot {
    let network = NetworkService::new().query_status();
    let bluetooth = BluetoothService::new().query_snapshot();
    let power = PowerService::new().query_details();
    let timedate = TimeDateService::new().query_status();
    let audio = AudioService::new().query_status();
    ServiceSnapshot {
        network,
        bluetooth,
        power,
        timedate,
        timedate_error: None,
        volume_percent: audio.volume_percent.clamp(0, 100),
        muted: audio.muted,
        wifi_action_result: None,
    }
}

/// Collapses redundant queued service commands before worker execution (`RUSTYOS-112`, `RUSTYOS-159`, `RUSTYOS-160`).
#[must_use]
pub(crate) fn coalesce_service_commands(cmds: Vec<ServiceCommand>) -> Vec<ServiceCommand> {
    let mut had_refresh = false;
    let mut rescan_wifi = false;
    let mut set_volume = None;
    let mut toggle_mute = false;
    let mut set_wifi = None;
    let mut wifi_op: Option<ServiceCommand> = None;
    let mut bt_power = None;
    let mut bt_discover = None;
    let mut bt_device_op: Option<ServiceCommand> = None;
    let mut power_profile = None;
    let mut set_ntp = None;
    let mut set_tz = None;
    let mut set_manual_time = None;

    for cmd in cmds {
        match cmd {
            ServiceCommand::Refresh => had_refresh = true,
            ServiceCommand::RescanWifi => rescan_wifi = true,
            ServiceCommand::SetVolume(v) => set_volume = Some(v),
            ServiceCommand::ToggleMute => toggle_mute = !toggle_mute,
            ServiceCommand::SetWifiEnabled(en) => {
                set_wifi = Some(en);
                if !en {
                    wifi_op = None;
                    rescan_wifi = false;
                }
            }
            op @ (ServiceCommand::ConnectWifi(_)
            | ServiceCommand::ConnectWifiWithPassword { .. }
            | ServiceCommand::DisconnectWifi(_)
            | ServiceCommand::ForgetWifi(_)) => wifi_op = Some(op),
            ServiceCommand::SetBluetoothPowered(en) => {
                bt_power = Some(en);
                if !en {
                    bt_discover = None;
                    bt_device_op = None;
                }
            }
            ServiceCommand::SetBluetoothDiscovering(en) => bt_discover = Some(en),
            op @ (ServiceCommand::ConnectBluetooth(_)
            | ServiceCommand::DisconnectBluetooth(_)
            | ServiceCommand::PairBluetooth(_)
            | ServiceCommand::RemoveBluetooth(_)) => bt_device_op = Some(op),
            ServiceCommand::SetPowerProfile(p) => power_profile = Some(p),
            ServiceCommand::SetNtpEnabled(en) => {
                set_ntp = Some(en);
                if en {
                    set_manual_time = None;
                }
            }
            ServiceCommand::SetTimezone(tz) => set_tz = Some(tz),
            ServiceCommand::SetManualDateTime(dt) => set_manual_time = Some(dt),
        }
    }

    let mut out = Vec::new();
    if let Some(en) = set_wifi {
        out.push(ServiceCommand::SetWifiEnabled(en));
    }
    if let Some(op) = wifi_op {
        out.push(op);
    }
    if rescan_wifi {
        out.push(ServiceCommand::RescanWifi);
    }
    if let Some(en) = bt_power {
        out.push(ServiceCommand::SetBluetoothPowered(en));
    }
    if let Some(op) = bt_device_op {
        out.push(op);
    }
    if let Some(en) = bt_discover {
        out.push(ServiceCommand::SetBluetoothDiscovering(en));
    }
    if let Some(p) = power_profile {
        out.push(ServiceCommand::SetPowerProfile(p));
    }
    if let Some(en) = set_ntp {
        out.push(ServiceCommand::SetNtpEnabled(en));
    }
    if let Some(tz) = set_tz {
        out.push(ServiceCommand::SetTimezone(tz));
    }
    if let Some(dt) = set_manual_time {
        out.push(ServiceCommand::SetManualDateTime(dt));
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

/// Executes a coalesced batch of service commands and queries fresh snapshot (`RUSTYOS-112`, `RUSTYOS-131`, `RUSTYOS-159`, `RUSTYOS-160`).
#[must_use]
pub(crate) fn execute_coalesced_service_batch(cmds: Vec<ServiceCommand>) -> ServiceSnapshot {
    let bt = BluetoothService::new();
    let net = NetworkService::new();
    let td = TimeDateService::new();
    let mut wifi_action_result: Option<WifiActionOutcome> = None;
    let mut timedate_error: Option<String> = None;
    let mut rescanned_networks: Option<Vec<rusty_services::WifiAccessPoint>> = None;
    for cmd in coalesce_service_commands(cmds) {
        match cmd {
            ServiceCommand::Refresh => {}
            ServiceCommand::RescanWifi => {
                rescanned_networks = Some(net.rescan_wifi_networks());
            }
            ServiceCommand::SetVolume(v) => {
                let _ = AudioService::new().set_volume(v);
            }
            ServiceCommand::ToggleMute => {
                let _ = AudioService::new().toggle_mute();
            }
            ServiceCommand::SetWifiEnabled(en) => {
                let _ = net.set_wifi_enabled(en);
            }
            ServiceCommand::ConnectWifi(ssid) => {
                let result = net.connect_wifi_with_password(&ssid, None);
                wifi_action_result = Some(WifiActionOutcome {
                    ssid,
                    kind: WifiActionKind::Connect,
                    result,
                });
            }
            ServiceCommand::ConnectWifiWithPassword { ssid, password } => {
                let result = net.connect_wifi_with_password(&ssid, Some(&password));
                wifi_action_result = Some(WifiActionOutcome {
                    ssid,
                    kind: WifiActionKind::Connect,
                    result,
                });
            }
            ServiceCommand::DisconnectWifi(ssid) => {
                let result = net.disconnect_wifi(&ssid);
                wifi_action_result = Some(WifiActionOutcome {
                    ssid,
                    kind: WifiActionKind::Disconnect,
                    result,
                });
            }
            ServiceCommand::ForgetWifi(ssid) => {
                let result = net.forget_wifi(&ssid);
                wifi_action_result = Some(WifiActionOutcome {
                    ssid,
                    kind: WifiActionKind::Forget,
                    result,
                });
            }
            ServiceCommand::SetBluetoothPowered(en) => {
                let _ = bt.set_powered(en);
            }
            ServiceCommand::SetBluetoothDiscovering(en) => {
                let _ = bt.set_discovering(en);
            }
            ServiceCommand::ConnectBluetooth(addr) => {
                let _ = bt.connect_device(&addr);
            }
            ServiceCommand::DisconnectBluetooth(addr) => {
                let _ = bt.disconnect_device(&addr);
            }
            ServiceCommand::PairBluetooth(addr) => {
                let _ = bt.pair_device(&addr);
            }
            ServiceCommand::RemoveBluetooth(addr) => {
                let _ = bt.remove_device(&addr);
            }
            ServiceCommand::SetPowerProfile(profile) => {
                let _ = PowerService::new().set_power_profile(profile);
            }
            ServiceCommand::SetNtpEnabled(en) => {
                if let Err(err) = td.set_ntp(en) {
                    timedate_error = Some(err);
                }
            }
            ServiceCommand::SetTimezone(tz) => {
                if let Err(err) = td.set_timezone(&tz) {
                    timedate_error = Some(err);
                }
            }
            ServiceCommand::SetManualDateTime(dt) => {
                if let Err(err) = td.set_manual_datetime(&dt) {
                    timedate_error = Some(err);
                }
            }
        }
    }
    let mut snap = query_service_snapshot();
    if let Some(fresh) = rescanned_networks
        && !fresh.is_empty()
    {
        snap.network.wifi_networks = fresh;
    }
    snap.wifi_action_result = wifi_action_result;
    snap.timedate_error = timedate_error;
    snap
}

fn default_search_controls() -> (
    rusty_ui::FocusScope,
    rusty_ui::WidgetId,
    rusty_ui::TextField,
) {
    let focus_search_id = rusty_ui::WidgetId::from_tag("settings.search");
    let mut focus = rusty_ui::FocusScope::new();
    focus.begin_frame();
    focus.register(focus_search_id);
    let search_field = rusty_ui::TextField::search(rusty_ui::Rect::new(12, 48, 204, 28), "Search");
    (focus, focus_search_id, search_field)
}

fn default_battery_tab() -> rusty_ui::SegmentedTabs {
    rusty_ui::SegmentedTabs::new(
        rusty_ui::Rect::new(0, 0, 260, 28),
        vec![
            String::from("Usage & Supply"),
            String::from("Health & Power"),
        ],
        0,
    )
}

fn default_low_power_dropdown() -> rusty_ui::DropdownSelect {
    rusty_ui::DropdownSelect::new(
        rusty_ui::Rect::new(0, 0, 176, 26),
        vec![
            String::from("Never"),
            String::from("Only on Battery"),
            String::from("Always"),
            String::from("Only on Power Adapter"),
        ],
        0,
    )
}

fn validate_shortcut_input(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Enter modifiers and key (e.g. Super+Space)");
    }
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut logo = false;
    let mut key_name: Option<String> = None;

    for token in trimmed.split('+').map(str::trim).filter(|t| !t.is_empty()) {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "opt" | "option" => alt = true,
            "shift" => shift = true,
            "super" | "logo" | "meta" | "win" | "cmd" => logo = true,
            other => {
                if key_name.is_some() {
                    return Err("Specify only one primary key after modifiers");
                }
                key_name = Some(normalize_shortcut_key_token(other)?);
            }
        }
    }

    let key = key_name.ok_or("Missing primary key (e.g. Space, W, Left)")?;
    let is_fn_or_print = key == "Print"
        || (key.starts_with('F') && key[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)));
    if !ctrl && !alt && !logo && !is_fn_or_print {
        return Err("Include Ctrl, Alt, or Super modifier");
    }
    if (ctrl && alt && key == "BackSpace")
        || (ctrl && alt && key.starts_with('F') && key[1..].parse::<u8>().is_ok_and(|n| n <= 12))
        || key == "Escape"
    {
        return Err("Reserved compositor/VT safety shortcut");
    }

    let mut parts = Vec::with_capacity(5);
    if ctrl {
        parts.push("Ctrl".to_owned());
    }
    if logo {
        parts.push("Super".to_owned());
    }
    if alt {
        parts.push("Alt".to_owned());
    }
    if shift {
        parts.push("Shift".to_owned());
    }
    parts.push(key);
    Ok(parts.join("+"))
}

fn normalize_shortcut_key_token(lower: &str) -> Result<String, &'static str> {
    let out = match lower {
        "space" => String::from("Space"),
        "tab" => String::from("Tab"),
        "comma" | "," => String::from("Comma"),
        "period" | "." => String::from("Period"),
        "slash" | "/" => String::from("Slash"),
        "left" => String::from("Left"),
        "right" => String::from("Right"),
        "up" => String::from("Up"),
        "down" => String::from("Down"),
        "return" | "enter" => String::from("Return"),
        "backspace" => String::from("BackSpace"),
        "delete" | "del" => String::from("Delete"),
        "escape" | "esc" => String::from("Escape"),
        "print" => String::from("Print"),
        fn_key
            if fn_key.starts_with('f')
                && fn_key[1..]
                    .parse::<u8>()
                    .is_ok_and(|n| (1..=12).contains(&n)) =>
        {
            fn_key.to_ascii_uppercase()
        }
        single if single.len() == 1 => {
            let b = single.as_bytes()[0];
            if b.is_ascii_alphanumeric() {
                (b as char).to_ascii_uppercase().to_string()
            } else {
                return Err("Unsupported key token; use A-Z, 0-9, Space, Tab, Left/Right, F1-F12");
            }
        }
        _ => return Err("Unsupported key token; use A-Z, 0-9, Space, Tab, Comma, Left/Right"),
    };
    Ok(out)
}

#[must_use]
pub(crate) fn gather_system_about_info() -> SystemAboutInfo {
    let mut info = SystemAboutInfo::default();
    if let Ok(h) = fs::read_to_string("/proc/sys/kernel/hostname")
        && !h.trim().is_empty()
    {
        h.trim().clone_into(&mut info.hostname);
    }
    if let Ok(os_rel) = fs::read_to_string("/etc/os-release") {
        for line in os_rel.lines() {
            if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                rest.trim().trim_matches('"').clone_into(&mut info.os_name);
                break;
            }
        }
    }
    let ostype =
        fs::read_to_string("/proc/sys/kernel/ostype").unwrap_or_else(|_| String::from("Linux"));
    let osrel = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let k = format!("{} {}", ostype.trim(), osrel.trim())
        .trim()
        .to_owned();
    if !k.is_empty() {
        info.kernel = k;
    }

    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if let Some((_, val)) = line.split_once(':')
                && (line.starts_with("model name") || line.starts_with("Hardware"))
                && !val.trim().is_empty()
            {
                val.trim().clone_into(&mut info.cpu);
                break;
            }
        }
    }

    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kib = rest
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse::<u64>().ok())
                    .unwrap_or(0);
                if kib > 0 {
                    #[allow(clippy::cast_precision_loss)]
                    let gib = kib as f64 / (1024.0 * 1024.0);
                    info.memory = format!("{gib:.1} GiB RAM");
                }
                break;
            }
        }
    }

    if let Some(gpu) = detect_drm_gpu_label() {
        info.graphics = gpu;
    }
    if let Some(disk) = detect_root_disk_summary() {
        info.startup_disk = disk;
    }
    info
}

fn detect_drm_gpu_label() -> Option<String> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let dev_dir = entry.path().join("device");
        let vendor = fs::read_to_string(dev_dir.join("vendor")).unwrap_or_default();
        let vendor_trimmed = vendor.trim().to_ascii_lowercase();
        let label = match vendor_trimmed.as_str() {
            "0x1002" => "AMD Radeon Graphics",
            "0x8086" => "Intel Integrated Graphics",
            "0x10de" => "NVIDIA Graphics",
            "0x1af4" => "VirtIO GPU",
            "0x1234" => "QEMU Standard VGA",
            "0x15ad" => "VMware SVGA",
            _ => "",
        };
        if !label.is_empty() {
            return Some(label.to_owned());
        }
        if let Ok(uevent) = fs::read_to_string(dev_dir.join("uevent")) {
            for line in uevent.lines() {
                if let Some(drv) = line.strip_prefix("DRIVER=")
                    && !drv.trim().is_empty()
                {
                    return Some(drv.trim().to_owned());
                }
            }
        }
    }
    None
}

fn detect_root_disk_summary() -> Option<String> {
    let out = Command::new("df").args(["-Pk", "/"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 4 {
        return None;
    }
    let total_kib = cols[1].parse::<u64>().ok()?;
    let avail_kib = cols[3].parse::<u64>().ok()?;
    #[allow(clippy::cast_precision_loss)]
    let total_gb = total_kib as f64 / (1024.0 * 1024.0);
    #[allow(clippy::cast_precision_loss)]
    let avail_gb = avail_kib as f64 / (1024.0 * 1024.0);
    Some(format!("{avail_gb:.1} GiB free of {total_gb:.1} GiB"))
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
            vec![
                SettingsPage::Wifi,
                SettingsPage::Bluetooth,
                SettingsPage::Network
            ]
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

    #[test]
    fn search_field_editing_clipboard_and_toggle_animations_work() {
        let mut st = SettingsAppState::defaults_with_page(SettingsPage::Appearance);
        let mut fe = rusty_ui::UiFontEngine::new();
        let mut mem = rusty_ui::MemoryClipboard::new();
        st.set_search_focused(true);
        assert!(st.apply_search_paste("display", &mut fe));
        assert_eq!(st.search_query, "display");
        let ctrl = rusty_ui::InputModifiers {
            ctrl: true,
            shift: false,
            alt: false,
        };
        assert!(st.handle_search_text_key(
            rusty_ui::TextInputKey::Char('a'),
            ctrl,
            &mut fe,
            &mut mem
        ));
        assert!(st.handle_search_text_key(
            rusty_ui::TextInputKey::Char('c'),
            ctrl,
            &mut fe,
            &mut mem
        ));
        assert_eq!(
            rusty_ui::ClipboardSink::get_text(&mut mem).as_deref(),
            Some("display")
        );

        st.navigate_to(SettingsPage::DockDesktop);
        let l1 = crate::layout::compute_layout(&st, 882, 600);
        st.sync_toggles_from_layout(&l1);
        let p0 = st.switch_progress(RowControlId::DockIndicatorsToggle, st.dock_show_indicators);
        st.dock_show_indicators = !st.dock_show_indicators;
        let l2 = crate::layout::compute_layout(&st, 882, 600);
        st.sync_toggles_from_layout(&l2);
        std::thread::sleep(Duration::from_millis(20));
        let _ = st.step_ui_animations(Instant::now());
        let p1 = st.switch_progress(RowControlId::DockIndicatorsToggle, st.dock_show_indicators);
        assert!((p1 - p0).abs() > 0.01);

        assert_eq!(dock_visibility_label(&st.dock_visibility), "Always Show");
        st.apply_toml_device_kv("dock", "visibility", "smart_hide");
        assert_eq!(st.dock_visibility, "smart_hide");
        assert!(!st.dock_auto_hide);
        assert_eq!(dock_visibility_label(&st.dock_visibility), "Smart Hide");
        st.apply_toml_device_kv("dock", "auto_hide", "true");
        assert_eq!(st.dock_visibility, "always_hide");
        assert!(st.dock_auto_hide);
        assert_eq!(dock_visibility_label(&st.dock_visibility), "Always Hide");
    }

    #[test]
    fn shortcut_validation_and_power_config_parsing_work() {
        assert_eq!(
            validate_shortcut_input(" super + shift + r "),
            Ok(String::from("Super+Shift+R"))
        );
        assert_eq!(validate_shortcut_input("alt+w"), Ok(String::from("Alt+W")));
        assert!(validate_shortcut_input("w").is_err());
        assert!(validate_shortcut_input("ctrl+alt+backspace").is_err());

        let mut st = SettingsAppState::defaults_with_page(SettingsPage::Battery);
        st.apply_toml_kv("power", "low_power_mode", "battery");
        st.apply_toml_kv("power", "display_sleep_minutes", "15");
        st.apply_toml_kv("power", "auto_lock_minutes", "5");
        st.apply_toml_kv("power", "dim_on_battery", "false");
        assert_eq!(st.power_low_power_mode, "battery");
        assert_eq!(st.low_power_dropdown.selected, 1);
        assert_eq!(st.power_display_sleep_minutes, 15);
        assert_eq!(st.power_auto_lock_minutes, 5);
        assert!(!st.power_dim_on_battery);

        st.apply_toml_kv("shortcuts", "launcher", "ctrl+space");
        assert_eq!(st.shortcuts[0], "Ctrl+Space");
    }

    #[test]
    fn wifi_password_modal_disconnect_forget_and_error_retry_work() {
        let mut st = SettingsAppState::new(&StartupOptions {
            initial_page: SettingsPage::Wifi,
            wifi_prompt_ssid: Some(String::from("Cafe-5G")),
            ..Default::default()
        });
        assert!(matches!(
            st.active_modal,
            Some(SettingsModal::WifiPassword { ref ssid, show_password: false, .. }) if ssid == "Cafe-5G"
        ));
        assert!(
            st.active_modal
                .as_ref()
                .and_then(|m| m.sheet().input.as_ref())
                .is_some_and(|tf| tf.masked)
        );
        st.toggle_wifi_password_visibility();
        assert!(
            st.active_modal
                .as_ref()
                .and_then(|m| m.sheet().input.as_ref())
                .is_some_and(|tf| !tf.masked)
        );
        st.pending_service_cmds.clear();

        // Empty password stays open with validation message
        st.confirm_active_modal();
        assert!(matches!(
            st.active_modal,
            Some(SettingsModal::WifiPassword { .. })
        ));
        assert_eq!(st.pending_service_cmds.len(), 0);

        // Non-empty password enqueues ConnectWifiWithPassword
        if let Some(modal) = &mut st.active_modal
            && let Some(tf) = modal.sheet_mut().input.as_mut()
        {
            tf.buffer.set_text("super-secret-psk");
        }
        st.confirm_active_modal();
        assert!(st.active_modal.is_none());
        assert_eq!(
            st.pending_service_cmds,
            vec![ServiceCommand::ConnectWifiWithPassword {
                ssid: String::from("Cafe-5G"),
                password: String::from("super-secret-psk"),
            }]
        );
    }

    #[test]
    fn wifi_error_reopens_modal_and_forget_disconnect_rescan_work() {
        let mut st = SettingsAppState::defaults_with_page(SettingsPage::Wifi);

        // Failed connect snapshot re-opens password dialog with nmcli error
        let err_snap = ServiceSnapshot {
            network: NetworkStatus {
                wifi_enabled: true,
                wifi_networks: vec![
                    rusty_services::WifiAccessPoint {
                        ssid: String::from("Home-WiFi"),
                        signal_percent: 90,
                        secure: true,
                        connected: true,
                        known: true,
                    },
                    rusty_services::WifiAccessPoint {
                        ssid: String::from("Cafe-5G"),
                        signal_percent: 65,
                        secure: true,
                        connected: false,
                        known: false,
                    },
                ],
                wifi_connected: true,
                wifi_ssid: Some(String::from("Home-WiFi")),
                ..NetworkStatus::default()
            },
            bluetooth: BluetoothSnapshot::default(),
            power: PowerDetails::default(),
            timedate: TimeDateStatus::default(),
            timedate_error: None,
            volume_percent: 50,
            muted: false,
            wifi_action_result: Some(WifiActionOutcome {
                ssid: String::from("Cafe-5G"),
                kind: WifiActionKind::Connect,
                result: Err(String::from("Wi-Fi network 'Cafe-5G' requires a password")),
            }),
        };
        assert!(st.apply_service_snapshot(err_snap));
        assert!(matches!(
            st.active_modal,
            Some(SettingsModal::WifiPassword { ref ssid, ref sheet, .. })
                if ssid == "Cafe-5G" && sheet.message.contains("requires a password")
        ));
        st.dismiss_active_modal();

        // Info button on saved network opens Forget modal; confirm enqueues ForgetWifi
        st.pending_service_cmds.clear();
        assert!(st.try_apply_service_action(RowControlId::WifiInfoIndex(0)));
        assert!(matches!(
            st.active_modal,
            Some(SettingsModal::WifiNetworkInfo { ref ssid, .. }) if ssid == "Home-WiFi"
        ));
        st.confirm_active_modal();
        assert_eq!(
            st.pending_service_cmds,
            vec![ServiceCommand::ForgetWifi(String::from("Home-WiFi"))]
        );

        // Disconnect active enqueues DisconnectWifi; Refresh enqueues RescanWifi
        st.pending_service_cmds.clear();
        assert!(st.try_apply_service_action(RowControlId::WifiDisconnectActive));
        assert!(st.try_apply_service_action(RowControlId::NetworkRefresh));
        assert_eq!(
            coalesce_service_commands(st.pending_service_cmds.clone()),
            vec![
                ServiceCommand::DisconnectWifi(String::from("Home-WiFi")),
                ServiceCommand::RescanWifi,
            ]
        );
    }

    #[test]
    fn datetime_ntp_timezone_and_manual_time_modals_work() {
        let mut st = SettingsAppState::defaults_with_page(SettingsPage::DateTime);
        st.available_timezones = vec![
            String::from("UTC"),
            String::from("Europe/Kyiv"),
            String::from("Europe/Berlin"),
        ];

        // Toggle NTP on
        assert!(st.try_apply_service_action(RowControlId::DateTimeNtpToggle));
        assert!(st.timedate.ntp_enabled);
        assert_eq!(
            st.pending_service_cmds,
            vec![ServiceCommand::SetNtpEnabled(true)]
        );
        // Manual time blocked while NTP enabled
        assert!(st.try_apply_service_action(RowControlId::DateTimeManualOpen));
        assert!(st.active_modal.is_none());
        assert!(st.timedate_hint.contains("Automatic"));

        // Disable NTP and open manual date/time modal
        st.pending_service_cmds.clear();
        assert!(st.try_apply_service_action(RowControlId::DateTimeNtpToggle));
        assert!(st.try_apply_service_action(RowControlId::DateTimeManualOpen));
        assert!(matches!(
            st.active_modal,
            Some(SettingsModal::ManualDateTime { .. })
        ));
        if let Some(modal) = &mut st.active_modal
            && let Some(tf) = modal.sheet_mut().input.as_mut()
        {
            tf.buffer.set_text("2026-11-23 18:45");
        }
        st.confirm_active_modal();
        assert!(st.active_modal.is_none());

        // Timezone modal resolves unique partial match ("kyiv" -> "Europe/Kyiv")
        assert!(st.try_apply_service_action(RowControlId::DateTimeTimezoneOpen));
        if let Some(modal) = &mut st.active_modal
            && let Some(tf) = modal.sheet_mut().input.as_mut()
        {
            tf.buffer.set_text("kyiv");
        }
        st.confirm_active_modal();
        assert_eq!(st.timedate.timezone, "Europe/Kyiv");
        assert_eq!(
            coalesce_service_commands(st.pending_service_cmds.clone()),
            vec![
                ServiceCommand::SetNtpEnabled(false),
                ServiceCommand::SetTimezone(String::from("Europe/Kyiv")),
                ServiceCommand::SetManualDateTime(String::from("2026-11-23 18:45:00")),
            ]
        );
    }
}
