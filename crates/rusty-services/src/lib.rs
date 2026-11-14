//! Linux system service integrations for RustyOS (`RUSTYOS-011`, ADR-0006).
//!
//! Provides zero-panic, least-privilege adapters for power/battery, network,
//! audio, and session management with graceful fallback if services are absent.

pub mod auth;
pub mod file_manager;
pub mod logind;
pub mod polkit;
pub mod portal;

pub use auth::{
    AsyncAuthHandle, AuthCheckResult, AuthOutcome, SecretText, is_valid_unix_username,
    verify_current_user_password,
};
pub use logind::{LogindEvent, LogindMonitorHandle, notify_session_locked_hint};
pub use polkit::{
    HelperLine, PolkitAgentError, PolkitAgentHandle, PolkitEvent, PolkitPromptRequest,
    PolkitUserReply, parse_helper_line,
};
pub use portal::{
    PIPEWIRE_FRAME_MAGIC, PIPEWIRE_MAX_DIM, PIPEWIRE_MIN_DIM, PORTAL_DBUS_NAME, PORTAL_OBJECT_PATH,
    PORTAL_RESPONSE_CANCELLED, PORTAL_RESPONSE_OK, PORTAL_RESPONSE_OTHER, PipeWireStreamConfig,
    PipeWireStreamHandle, PortalDesktopSnapshot, PortalEvent, PortalHandle, PortalOutputInfo,
    PortalPromptKind, PortalPromptRequest, PortalReply, PortalReplySender, PortalSelection,
    PortalStreamDescriptor, PortalWindowInfo, SCREENCAST_AVAILABLE_CURSOR_MODES,
    SCREENCAST_AVAILABLE_SOURCE_TYPES, SCREENCAST_CURSOR_EMBEDDED, SCREENCAST_CURSOR_HIDDEN,
    SCREENCAST_SOURCE_MONITOR, SCREENCAST_SOURCE_WINDOW, SCREENSHOT_AVAILABLE_TARGETS,
    SCREENSHOT_TARGET_ACTIVE_WINDOW, SCREENSHOT_TARGET_AREA, SCREENSHOT_TARGET_SCREEN,
    SCREENSHOT_TARGET_WINDOW, WorkerStatusLine, encode_pipewire_frame_header,
    encode_screencast_streams_value, parse_worker_line, path_to_file_uri,
    resolve_pipewire_helper_path, rgba_pixel_to_portal_color, sanitize_portal_text,
};

mod connectivity;
mod display_session;
mod media;
mod power;

pub use connectivity::{
    BluetoothService, BluetoothStatus, NetworkService, NetworkStatus, format_bluetooth_label,
    format_nm_network_status, format_nm_network_status_with_signal, parse_proc_net_wireless_signal,
    wifi_signal_bars,
};
pub use display_session::{
    BrightnessService, BrightnessStatus, SessionAction, SessionService, UserAccount, UserService,
    calc_backlight_percent, calc_backlight_raw, local_clock_hhmm, local_clock_lock_date,
    local_clock_lock_hhmm, local_clock_short_datetime, local_clock_short_datetime_fmt,
    parse_passwd_users,
};
pub use media::{
    AudioService, AudioStatus, MediaAction, MediaService, MediaStatus, format_wpctl_volume,
    parse_playerctl_status_line,
};
pub use power::{BatteryState, PowerService, PowerStatus};

#[cfg(test)]
use std::fs;

/// Unified registry providing refreshed system metrics to `rusty-shell`.
#[derive(Debug, Default, Clone)]
pub struct ServiceRegistry {
    /// Power supply service.
    pub power: PowerService,
    /// Network status service.
    pub network: NetworkService,
    /// Bluetooth adapter & device service (`RUSTYOS-027`).
    pub bluetooth: BluetoothService,
    /// Audio volume service.
    pub audio: AudioService,
    /// Display backlight brightness service (`RUSTYOS-031`).
    pub brightness: BrightnessService,
    /// MPRIS2 media control service (`RUSTYOS-031`).
    pub media: MediaService,
    /// User account discovery service (`RUSTYOS-054`).
    pub users: UserService,
    /// Session power/lifecycle service.
    pub session: SessionService,
}

impl ServiceRegistry {
    /// Creates a new service registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            power: PowerService::new(),
            network: NetworkService::new(),
            bluetooth: BluetoothService::new(),
            audio: AudioService::new(),
            brightness: BrightnessService::new(),
            media: MediaService::new(),
            users: UserService::new(),
            session: SessionService::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_service_query_gracefully_handles_empty_or_missing_path() {
        let s = PowerService {
            base_path: "/tmp/non_existent_power_supply_test".to_owned(),
        };
        let status = s.query_status();
        assert_eq!(status.percentage, 100);
        assert!(!status.has_battery);
    }

    #[test]
    fn network_service_query_returns_valid_structure() {
        let net = NetworkService::new();
        let st = net.query_status();
        assert_ne!(st.status_label, "");
    }

    #[test]
    fn audio_service_query_returns_valid_bounds() {
        let audio = AudioService::new();
        let st = audio.query_status();
        assert!(st.volume_percent <= 100);
    }

    #[test]
    fn wpctl_volume_and_clock_helpers_format_cleanly() {
        assert_eq!(format_wpctl_volume(75), "0.75");
        assert_eq!(format_wpctl_volume(150), "1.00");
        let clock = local_clock_hhmm();
        assert_eq!(clock.len(), 5);
        assert_eq!(clock.as_bytes().get(2), Some(&b':'));

        let dt_12h = local_clock_short_datetime_fmt(false);
        assert!(dt_12h.contains("AM") || dt_12h.contains("PM") || dt_12h.len() == 5);
        assert!(dt_12h.is_ascii());

        let dt_24h = local_clock_short_datetime_fmt(true);
        assert!(dt_24h.contains(':') && dt_24h.is_ascii());
    }

    #[test]
    fn networkmanager_and_bluetooth_formatters_handle_all_states() {
        let wifi = format_nm_network_status("802-11-wireless", Some("Studio5G".to_owned()));
        assert!(wifi.connected);
        assert_eq!(wifi.wifi_ssid.as_deref(), Some("Studio5G"));
        assert_eq!(wifi.status_label, "Wi-Fi: Studio5G");
        assert_eq!(wifi.wifi_signal_percent, None);

        let wifi_live = format_nm_network_status_with_signal(
            "802-11-wireless",
            Some("Studio5G".to_owned()),
            Some("wlan0".to_owned()),
            Some(57),
        );
        assert_eq!(wifi_live.wifi_signal_percent, Some(57));
        assert_eq!(wifi_live.interface.as_deref(), Some("wlan0"));

        let proc_wireless = "\
Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
 wlan0: 0000   56.  -54.  -256        0      0      0      0    830        0
";
        assert_eq!(
            parse_proc_net_wireless_signal(proc_wireless, Some("wlan0")),
            Some(80)
        );
        assert_eq!(wifi_signal_bars(false, Some(90)), 0);
        assert_eq!(wifi_signal_bars(true, Some(15)), 1);
        assert_eq!(wifi_signal_bars(true, Some(42)), 2);
        assert_eq!(wifi_signal_bars(true, Some(57)), 3);
        assert_eq!(wifi_signal_bars(true, None), 3);

        let eth = format_nm_network_status("802-3-ethernet", None);
        assert!(eth.connected);
        assert_eq!(eth.wifi_ssid, None);
        assert_eq!(eth.status_label, "Ethernet Connected");
    }

    #[test]
    fn power_service_reads_ac_full_charging_and_discharging_accurately() {
        let base = std::env::temp_dir().join(format!("rustyos_power_test_{}", std::process::id()));
        let ac = base.join("ACAD");
        let bat = base.join("BAT1");
        let usbc = base.join("ucsi-source-psy-USBC000:001");
        let _ = fs::create_dir_all(&ac);
        let _ = fs::create_dir_all(&bat);
        let _ = fs::create_dir_all(&usbc);

        let _ = fs::write(ac.join("type"), "Mains\n");
        let _ = fs::write(ac.join("online"), "1\n");
        let _ = fs::write(usbc.join("type"), "USB\n");
        let _ = fs::write(usbc.join("online"), "0\n");
        let _ = fs::write(bat.join("type"), "Battery\n");
        let _ = fs::write(bat.join("status"), "Full\n");
        let _ = fs::write(bat.join("capacity"), "100\n");

        let svc = PowerService {
            base_path: base.to_string_lossy().into_owned(),
        };
        let plugged_full = svc.query_status();
        assert!(plugged_full.has_battery);
        assert!(plugged_full.ac_online);
        assert_eq!(plugged_full.percentage, 100);
        assert_eq!(plugged_full.state, BatteryState::Full);

        let _ = fs::write(ac.join("online"), "0\n");
        let _ = fs::write(bat.join("status"), "Discharging\n");
        let _ = fs::write(bat.join("capacity"), "62\n");
        let on_battery = svc.query_status();
        assert!(on_battery.has_battery);
        assert!(!on_battery.ac_online);
        assert_eq!(on_battery.percentage, 62);
        assert_eq!(on_battery.state, BatteryState::Discharging);

        let _ = fs::write(bat.join("status"), "Charging\n");
        let charging = svc.query_status();
        assert!(charging.ac_online);
        assert_eq!(charging.state, BatteryState::Charging);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn bluetooth_status_label_formats_gracefully() {
        assert_eq!(
            format_bluetooth_label(false, false, None),
            "BT: Unavailable"
        );
        assert_eq!(format_bluetooth_label(true, false, None), "BT: Off");
        assert_eq!(format_bluetooth_label(true, true, None), "BT: On");
        assert_eq!(
            format_bluetooth_label(true, true, Some("AirPods")),
            "BT: AirPods"
        );

        let bt = BluetoothService::new().query_status();
        assert_ne!(bt.status_label, "");
    }

    #[test]
    fn brightness_service_reads_sysfs_values_and_computes_raw_levels() {
        let base = std::env::temp_dir().join(format!("rustyos_backlight_{}", std::process::id()));
        let dev_dir = base.join("intel_backlight");
        let _ = fs::create_dir_all(&dev_dir);
        let _ = fs::write(dev_dir.join("brightness"), "250\n");
        let _ = fs::write(dev_dir.join("max_brightness"), "500\n");

        let svc = BrightnessService {
            base_path: base.to_string_lossy().into_owned(),
        };
        let status = svc.query_status();
        assert!(status.available);
        assert_eq!(status.percentage, 50);
        assert_eq!(status.device.as_deref(), Some("intel_backlight"));

        assert_eq!(calc_backlight_percent(1, 500), 0);
        assert_eq!(calc_backlight_raw(1, 500), 5);
        assert_eq!(calc_backlight_raw(1, 10), 1);
        assert_eq!(calc_backlight_raw(0, 500), 0);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn playerctl_status_line_parses_playing_paused_and_empty() {
        let playing = parse_playerctl_status_line("Playing\tMidnight City\tM83");
        assert!(playing.active);
        assert!(playing.playing);
        assert_eq!(playing.title, "Midnight City");
        assert_eq!(playing.artist.as_deref(), Some("M83"));

        let paused = parse_playerctl_status_line("Paused\tAmbient Track\t");
        assert!(paused.active);
        assert!(!paused.playing);
        assert_eq!(paused.title, "Ambient Track");
        assert_eq!(paused.artist, None);

        let idle = parse_playerctl_status_line("");
        assert!(!idle.active);
        assert!(!idle.playing);
        assert_eq!(idle.title, "");
    }

    #[test]
    fn passwd_user_parser_and_lock_clock_formatters_work() {
        let passwd = "\
root:x:0:0:root:/root:/bin/bash
nobody:x:65534:65534:Kernel Overflow User:/:/sbin/nologin
merri:x:1001:1001:Merri,,,:/home/merri:/bin/zsh
anc:x:1000:1000:Stephen Hackett,Room 1:/home/anc:/bin/bash
svc:x:1002:1002:Disabled Account:/home/svc:/usr/bin/false
";
        let users = parse_passwd_users(passwd, Some("anc"));
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].username, "anc");
        assert_eq!(users[0].display_name, "Stephen Hackett");
        assert_eq!(users[1].username, "merri");
        assert_eq!(users[1].display_name, "Merri");

        let lock_date = local_clock_lock_date();
        assert!(!lock_date.is_empty() && lock_date.is_ascii());

        let lock_12 = local_clock_lock_hhmm(false);
        assert!(lock_12.contains(':') && !lock_12.contains("AM") && !lock_12.contains("PM"));
        let lock_24 = local_clock_lock_hhmm(true);
        assert!(lock_24.len() == 5 && lock_24.contains(':'));
    }
}
