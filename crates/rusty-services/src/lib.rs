//! Linux system service integrations for RustyOS (`RUSTYOS-011`, ADR-0006).
//!
//! Provides zero-panic, least-privilege adapters for power/battery, network,
//! audio, and session management with graceful fallback if services are absent.

pub mod auth;
pub mod file_manager;
pub mod image_decode;
pub mod installer_service;
pub mod logind;
pub mod polkit;
pub mod portal;
pub mod privacy;
pub mod storage;
pub mod theme_sync;
pub mod updates;

pub use auth::{
    AsyncAuthHandle, AuthCheckResult, AuthOutcome, SecretText, is_valid_unix_username,
    verify_current_user_password,
};
pub use installer_service::{
    InstallSource, InstallerPlanRequest, InstallerStageEvent, LIVE_SQUASHFS_CANDIDATES,
    build_install_stages, decode_installer_event, detect_install_source, detect_live_squashfs_path,
    encode_installer_event, validate_installer_request,
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
    BluetoothDevice, BluetoothService, BluetoothSnapshot, BluetoothStatus, NetworkService,
    NetworkStatus, WifiAccessPoint, format_bluetooth_label, format_nm_network_status,
    format_nm_network_status_with_signal, normalize_bt_address, parse_bluetoothctl_devices,
    parse_nmcli_wifi_list, parse_proc_net_wireless_signal, wifi_signal_bars,
};
pub use display_session::{
    BrightnessService, BrightnessStatus, CurrentUserDetails, SessionAction, SessionService,
    UserAccount, UserService, calc_backlight_percent, calc_backlight_raw,
    format_localized_short_datetime, local_clock_hhmm, local_clock_lock_date,
    local_clock_lock_hhmm, local_clock_short_datetime, local_clock_short_datetime_fmt,
    local_clock_short_datetime_localized, parse_etc_group_for_user, parse_passwd_users,
};
pub use media::{
    AudioService, AudioStatus, MediaAction, MediaService, MediaStatus, format_wpctl_volume,
    parse_playerctl_status_line,
};
pub use power::{
    BatteryCondition, BatteryState, PowerDetails, PowerProfile, PowerService, PowerStatus,
    parse_powerprofilesctl_list,
};
pub use privacy::{DeviceUsageService, DeviceUsageSnapshot, parse_pipewire_dump_usage};
pub use storage::{
    RemovableVolume, StorageService, is_valid_block_device_path, parse_lsblk_removable_volumes,
};
pub use theme_sync::{ThemeSyncConfig, ThemeSyncService, merge_gtk_settings_ini};
pub use updates::{
    PackageUpdateItem, UpdateCheckState, UpdateCheckerHandle, UpdateStatusSnapshot,
    is_rustyos_package, package_requires_restart, parse_checkupdates_output,
};

#[cfg(test)]
use std::fs;

/// Unified registry providing refreshed system metrics to the compositor shell and system apps.
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
        assert!(wifi.wifi_connected);
        assert!(!wifi.wired_connected);
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

        let eth = format_nm_network_status_with_signal(
            "802-3-ethernet",
            Some("Проводное соединение 1".to_owned()),
            Some("enp3s0".to_owned()),
            None,
        );
        assert!(eth.connected);
        assert!(eth.wired_connected);
        assert!(!eth.wifi_connected);
        assert_eq!(eth.wired_interface.as_deref(), Some("enp3s0"));
        assert_eq!(eth.wired_label.as_deref(), Some("Проводное соединение 1"));
        assert_eq!(eth.wifi_ssid, None);
        assert_eq!(eth.status_label, "Проводное соединение 1");

        let parsed = parse_nmcli_wifi_list(
            " :HomeNet:65:WPA2\n*:Office\\:5G:82:WPA2 WPA3\n :HomeNet:91:WPA2\n :FreeCafe:40:--\n ::55:WPA2\n",
            &[String::from("HomeNet")],
            None,
        );
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].ssid, "Office:5G");
        assert!(parsed[0].connected && parsed[0].known && parsed[0].secure);
        assert_eq!(parsed[1].ssid, "HomeNet");
        assert_eq!(parsed[1].signal_percent, 91);
        assert!(parsed[1].known && !parsed[1].connected);
        assert_eq!(parsed[2].ssid, "FreeCafe");
        assert!(!parsed[2].secure && !parsed[2].known);
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

    #[test]
    fn bluetooth_mac_validation_and_device_parser_reject_invalid_input() {
        assert_eq!(
            normalize_bt_address(" aa:bb:cc:11:22:ff "),
            Some("AA:BB:CC:11:22:FF".to_owned())
        );
        assert_eq!(normalize_bt_address("invalid;rm -rf"), None);
        assert_eq!(normalize_bt_address("AA:BB:CC:DD:EE"), None);

        let raw = "\
Device 11:22:33:44:55:66 AirPods Pro
Device bad-mac Broken Device
Device aa:bb:cc:dd:ee:ff MX Master 3S
Device 11:22:33:44:55:66 Duplicate Name
";
        let list = parse_bluetoothctl_devices(raw);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].address, "11:22:33:44:55:66");
        assert_eq!(list[0].name, "AirPods Pro");
        assert_eq!(list[1].address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(list[1].name, "MX Master 3S");

        let bt = BluetoothService::new();
        assert!(!bt.connect_device("not-a-mac"));
        assert!(!bt.remove_device("not-a-mac"));
    }

    #[test]
    fn power_service_parses_health_cycles_and_profile_output() {
        let base = std::env::temp_dir().join(format!("rustyos_power_det_{}", std::process::id()));
        let bat = base.join("BAT0");
        let ac = base.join("AC");
        let _ = fs::create_dir_all(&bat);
        let _ = fs::create_dir_all(&ac);
        let _ = fs::write(ac.join("type"), "Mains\n");
        let _ = fs::write(ac.join("online"), "0\n");
        let _ = fs::write(bat.join("type"), "Battery\n");
        let _ = fs::write(bat.join("status"), "Discharging\n");
        let _ = fs::write(bat.join("capacity"), "62\n");
        let _ = fs::write(bat.join("energy_full_design"), "50000000\n");
        let _ = fs::write(bat.join("energy_full"), "44000000\n");
        let _ = fs::write(bat.join("energy_now"), "22000000\n");
        let _ = fs::write(bat.join("power_now"), "11000000\n");
        let _ = fs::write(bat.join("cycle_count"), "142\n");
        let _ = fs::write(bat.join("model_name"), "RustyCell-50\n");
        let _ = fs::write(bat.join("technology"), "Li-poly\n");
        let hid = base.join("hidpp_battery_0");
        let _ = fs::create_dir_all(&hid);
        let _ = fs::write(hid.join("type"), "Battery\n");
        let _ = fs::write(hid.join("scope"), "Device\n");
        let _ = fs::write(hid.join("status"), "Full\n");
        let _ = fs::write(hid.join("capacity"), "95\n");

        let svc = PowerService {
            base_path: base.to_string_lossy().into_owned(),
        };
        let det = svc.query_sysfs_details();
        assert!(det.status.has_battery);
        assert_eq!(det.status.percentage, 62);
        assert_eq!(det.health_percent, Some(88));
        assert_eq!(det.cycle_count, Some(142));
        assert_eq!(det.condition, BatteryCondition::Normal);
        assert_eq!(det.time_remaining_minutes, Some(120));
        assert!(det.power_watts.is_some_and(|w| (w - 11.0).abs() < 0.01));
        assert_eq!(det.model_name.as_deref(), Some("RustyCell-50"));
        assert_eq!(det.technology.as_deref(), Some("Li-poly"));

        let prof_out = "\
  performance:
* balanced:
  power-saver:
";
        let (active, avail) = parse_powerprofilesctl_list(prof_out);
        assert_eq!(active, Some(PowerProfile::Balanced));
        assert_eq!(
            avail,
            vec![
                PowerProfile::Performance,
                PowerProfile::Balanced,
                PowerProfile::LowPower
            ]
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn current_user_details_parses_passwd_groups_and_admin_role() {
        let passwd = "alice:x:1000:1000:Alice Example,:/home/alice:/bin/zsh\n";
        let groups = "\
wheel:x:998:alice
alice:x:1000:
video:x:986:alice,bob
";
        let det = UserService::parse_current_user_details(
            "alice",
            passwd,
            groups,
            None,
            "/nonexistent",
            "wayland",
        );
        assert_eq!(det.username, "alice");
        assert_eq!(det.display_name, "Alice Example");
        assert_eq!(det.uid, 1000);
        assert_eq!(det.gid, 1000);
        assert_eq!(det.shell, "/bin/zsh");
        assert!(det.is_admin);
        assert_eq!(det.groups, vec!["wheel", "alice", "video"]);
        assert_eq!(det.session_type, "Wayland");
    }
}
