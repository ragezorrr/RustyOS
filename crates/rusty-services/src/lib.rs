//! Linux system service integrations for RustyOS (`RUSTYOS-011`, ADR-0006).
//!
//! Provides zero-panic, least-privilege adapters for power/battery, network,
//! audio, and session management with graceful fallback if services are absent.

use std::fs;
use std::path::Path;
use std::process::Command;

use zbus::zvariant::Value;

/// Battery charging and health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatteryState {
    /// Battery is charging.
    Charging,
    /// Battery is discharging on battery power.
    Discharging,
    /// Battery is full and on AC power.
    Full,
    /// Battery status is unknown or not connected.
    #[default]
    Unknown,
}

/// Power and battery metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerStatus {
    /// Battery charge percentage (0..=100).
    pub percentage: u8,
    /// Current charging status.
    pub state: BatteryState,
    /// True if system is plugged into AC power.
    pub ac_online: bool,
    /// True if a hardware battery was detected.
    pub has_battery: bool,
}

impl Default for PowerStatus {
    fn default() -> Self {
        Self {
            percentage: 100,
            state: BatteryState::Full,
            ac_online: true,
            has_battery: false,
        }
    }
}

/// Network connectivity status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkStatus {
    /// True if an active network interface has an IP and is connected.
    pub connected: bool,
    /// Name of the primary active interface (e.g., "wlan0", "eth0").
    pub interface: Option<String>,
    /// Wi-Fi SSID if connected via wireless.
    pub wifi_ssid: Option<String>,
    /// Human-readable status label for `TopBar`.
    pub status_label: String,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            connected: false,
            interface: None,
            wifi_ssid: None,
            status_label: "Disconnected".to_owned(),
        }
    }
}

/// Bluetooth adapter & connected-device status (`RUSTYOS-027`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BluetoothStatus {
    /// True if at least one BlueZ or sysfs Bluetooth adapter is present.
    pub available: bool,
    /// True if the primary adapter is powered on.
    pub powered: bool,
    /// Name/alias of the first connected Bluetooth device, if any.
    pub connected_device: Option<String>,
    /// Concise status label formatted for Quick Settings (`BT: On`, `BT: Off`, `BT: <dev>`).
    pub status_label: String,
}

impl Default for BluetoothStatus {
    fn default() -> Self {
        Self {
            available: false,
            powered: false,
            connected_device: None,
            status_label: "BT: Unavailable".to_owned(),
        }
    }
}

/// Audio volume and mute status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStatus {
    /// Output master volume percentage (0..=100).
    pub volume_percent: u8,
    /// Master mute state.
    pub muted: bool,
    /// Backend service identified (e.g., "pipewire", "wireplumber", "alsa").
    pub backend: String,
}

impl Default for AudioStatus {
    fn default() -> Self {
        Self {
            volume_percent: 75,
            muted: false,
            backend: "default".to_owned(),
        }
    }
}

/// Power/battery service reading from Linux sysfs (`/sys/class/power_supply`).
#[derive(Debug, Default, Clone)]
pub struct PowerService {
    base_path: String,
}

impl PowerService {
    /// Creates a new power service targeting `/sys/class/power_supply`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base_path: "/sys/class/power_supply".to_owned(),
        }
    }

    /// Queries the current power supply status from sysfs.
    #[must_use]
    pub fn query_status(&self) -> PowerStatus {
        let p = Path::new(&self.base_path);
        if !p.exists() {
            return PowerStatus::default();
        }

        let mut status = PowerStatus::default();
        let Ok(entries) = fs::read_dir(p) else {
            return status;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();

            // Check AC adapter (AC, ACAD, ADP1, etc.)
            if (name.starts_with("ac") || name.starts_with("adp"))
                && let Ok(online_str) = fs::read_to_string(path.join("online"))
            {
                status.ac_online = online_str.trim() == "1";
            }

            // Check Battery (BAT0, BAT1, etc.)
            if name.starts_with("bat") {
                status.has_battery = true;
                if let Ok(cap_str) = fs::read_to_string(path.join("capacity"))
                    && let Ok(val) = cap_str.trim().parse::<u8>()
                {
                    status.percentage = val.min(100);
                }
                if let Ok(st_str) = fs::read_to_string(path.join("status")) {
                    status.state = match st_str.trim().to_lowercase().as_str() {
                        "charging" => BatteryState::Charging,
                        "discharging" => BatteryState::Discharging,
                        "full" => BatteryState::Full,
                        _ => BatteryState::Unknown,
                    };
                }
            }
        }

        status
    }
}

/// Network service inspecting Linux network devices and NetworkManager.
#[derive(Debug, Default, Clone)]
pub struct NetworkService;

impl NetworkService {
    /// Creates a new network service.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Queries the active network status via `NetworkManager` D-Bus, falling back to `/sys/class/net` and `nmcli`.
    #[must_use]
    pub fn query_status(&self) -> NetworkStatus {
        if let Some(dbus_status) = query_network_via_dbus() {
            return dbus_status;
        }

        let mut status = NetworkStatus::default();

        // Fallback: /sys/class/net operstate + nmcli
        let net_path = Path::new("/sys/class/net");
        if let Ok(entries) = fs::read_dir(net_path) {
            for entry in entries.flatten() {
                let dev_name = entry.file_name().to_string_lossy().to_string();
                if dev_name == "lo" {
                    continue;
                }
                let operstate_file = entry.path().join("operstate");
                if let Ok(st) = fs::read_to_string(operstate_file)
                    && st.trim() == "up"
                {
                    status.connected = true;
                    status.interface = Some(dev_name.clone());
                    if dev_name.starts_with('w') {
                        "Wi-Fi Connected".clone_into(&mut status.status_label);
                    } else {
                        "Ethernet Connected".clone_into(&mut status.status_label);
                    }
                    break;
                }
            }
        }

        if status.connected
            && let Ok(output) = Command::new("nmcli")
                .args(["-t", "-f", "active,ssid", "dev", "wifi"])
                .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if let Some(ssid) = line.strip_prefix("yes:") {
                    let trimmed = ssid.trim();
                    if !trimmed.is_empty() {
                        status.wifi_ssid = Some(trimmed.to_owned());
                        status.status_label = format!("Wi-Fi: {trimmed}");
                        break;
                    }
                }
            }
        }

        status
    }
}

fn query_network_via_dbus() -> Option<NetworkStatus> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let nm_iface =
        zbus::names::InterfaceName::from_static_str("org.freedesktop.NetworkManager").ok()?;
    let props = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
        .destination("org.freedesktop.NetworkManager")
        .ok()?
        .path("/org/freedesktop/NetworkManager")
        .ok()?
        .build()
        .ok()?;
    let all = props.get_all(nm_iface).ok()?;

    let nm_state = match all.get("State").map(|v| &**v) {
        Some(Value::U32(n)) => *n,
        _ => 0,
    };
    // NetworkManager states >= 50 correspond to ConnectedLocal / ConnectedSite / ConnectedGlobal.
    if nm_state < 50 {
        return Some(NetworkStatus::default());
    }

    let conn_type = match all.get("PrimaryConnectionType").map(|v| &**v) {
        Some(Value::Str(s)) => s.as_str().to_owned(),
        _ => String::new(),
    };

    let mut active_id = None;
    if let Some(Value::ObjectPath(primary_path)) = all.get("PrimaryConnection").map(|v| &**v)
        && primary_path.as_str() != "/"
        && let Ok(ac_iface) = zbus::names::InterfaceName::from_static_str(
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        && let Ok(path_builder) = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
            .destination("org.freedesktop.NetworkManager")
            .and_then(|b| b.path(primary_path.as_str().to_owned()))
        && let Ok(ac_props) = path_builder.build()
        && let Ok(id_val) = ac_props.get(ac_iface, "Id")
        && let Value::Str(id_str) = &*id_val
        && !id_str.as_str().trim().is_empty()
    {
        active_id = Some(id_str.as_str().to_owned());
    }

    Some(format_nm_network_status(&conn_type, active_id))
}

/// Formats NetworkManager connection type & profile ID into [`NetworkStatus`].
#[must_use]
pub fn format_nm_network_status(conn_type: &str, active_id: Option<String>) -> NetworkStatus {
    let clean_id = active_id.filter(|s| !s.trim().is_empty());
    let is_wifi = conn_type.contains("wireless") || conn_type.contains("wifi");
    let status_label = match (&clean_id, is_wifi) {
        (Some(name), true) => format!("Wi-Fi: {name}"),
        (Some(name), false) => name.clone(),
        (None, true) => "Wi-Fi Connected".to_owned(),
        (None, false) => "Ethernet Connected".to_owned(),
    };
    NetworkStatus {
        connected: true,
        interface: None,
        wifi_ssid: if is_wifi { clean_id } else { None },
        status_label,
    }
}

/// Bluetooth service interacting with `BlueZ` over System D-Bus (with CLI/sysfs fallback) (`RUSTYOS-027`).
#[derive(Debug, Default, Clone)]
pub struct BluetoothService;

impl BluetoothService {
    /// Creates a new Bluetooth adapter service.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Queries the primary Bluetooth adapter and connected device status.
    #[must_use]
    pub fn query_status(&self) -> BluetoothStatus {
        if let Some(dbus_bt) = query_bluez_via_dbus() {
            return dbus_bt;
        }
        query_bluetooth_fallback()
    }

    /// Enables or disables Bluetooth adapter power via `BlueZ` D-Bus or `bluetoothctl`.
    #[must_use]
    pub fn set_powered(&self, powered: bool) -> bool {
        if set_bluez_powered_via_dbus(powered) {
            return true;
        }
        let arg = if powered { "on" } else { "off" };
        Command::new("bluetoothctl")
            .args(["power", arg])
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// Formats a [`BluetoothStatus`] summary from adapter/device metadata (`RUSTYOS-027`).
#[must_use]
pub fn format_bluetooth_label(
    available: bool,
    powered: bool,
    connected_device: Option<&str>,
) -> String {
    if !available {
        return "BT: Unavailable".to_owned();
    }
    if !powered {
        return "BT: Off".to_owned();
    }
    match connected_device.map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => format!("BT: {name}"),
        None => "BT: On".to_owned(),
    }
}

fn query_bluez_via_dbus() -> Option<BluetoothStatus> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let om = zbus::blocking::fdo::ObjectManagerProxy::builder(&conn)
        .destination("org.bluez")
        .ok()?
        .path("/")
        .ok()?
        .build()
        .ok()?;
    let objects = om.get_managed_objects().ok()?;

    let mut available = false;
    let mut powered = false;
    let mut connected_device: Option<String> = None;

    for ifaces in objects.values() {
        for (iface_name, props) in ifaces {
            if iface_name.as_str() == "org.bluez.Adapter1" {
                available = true;
                if matches!(props.get("Powered").map(|v| &**v), Some(Value::Bool(true))) {
                    powered = true;
                }
            } else if iface_name.as_str() == "org.bluez.Device1"
                && connected_device.is_none()
                && matches!(
                    props.get("Connected").map(|v| &**v),
                    Some(Value::Bool(true))
                )
            {
                connected_device = props
                    .get("Alias")
                    .or_else(|| props.get("Name"))
                    .and_then(|v| match &**v {
                        Value::Str(s) => Some(s.as_str().to_owned()),
                        _ => None,
                    })
                    .filter(|s| !s.trim().is_empty());
            }
        }
    }

    let status_label = format_bluetooth_label(available, powered, connected_device.as_deref());
    Some(BluetoothStatus {
        available,
        powered,
        connected_device,
        status_label,
    })
}

fn set_bluez_powered_via_dbus(powered: bool) -> bool {
    let Ok(conn) = zbus::blocking::Connection::system() else {
        return false;
    };
    let Ok(om_builder) = zbus::blocking::fdo::ObjectManagerProxy::builder(&conn)
        .destination("org.bluez")
        .and_then(|b| b.path("/"))
    else {
        return false;
    };
    let Ok(om) = om_builder.build() else {
        return false;
    };
    let Ok(objects) = om.get_managed_objects() else {
        return false;
    };
    let Some(adapter_path) = objects.into_iter().find_map(|(path, ifaces)| {
        ifaces
            .keys()
            .any(|k| k.as_str() == "org.bluez.Adapter1")
            .then_some(path)
    }) else {
        return false;
    };

    let Ok(iface_name) = zbus::names::InterfaceName::from_static_str("org.bluez.Adapter1") else {
        return false;
    };
    let Ok(props_builder) = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
        .destination("org.bluez")
        .and_then(|b| b.path(adapter_path))
    else {
        return false;
    };
    let Ok(props) = props_builder.build() else {
        return false;
    };
    props
        .set(iface_name, "Powered", Value::Bool(powered))
        .is_ok()
}

fn query_bluetooth_fallback() -> BluetoothStatus {
    let sysfs_present = fs::read_dir("/sys/class/bluetooth").is_ok_and(|mut d| d.next().is_some());
    let mut status = BluetoothStatus {
        available: sysfs_present,
        powered: false,
        connected_device: None,
        status_label: format_bluetooth_label(sysfs_present, false, None),
    };
    if let Ok(output) = Command::new("bluetoothctl").arg("show").output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Powered:") {
                status.available = true;
                status.powered = rest.trim().eq_ignore_ascii_case("yes");
            }
        }
        status.status_label = format_bluetooth_label(status.available, status.powered, None);
    }
    status
}

/// Audio service interacting with PipeWire/WirePlumber or PulseAudio/ALSA.
#[derive(Debug, Default, Clone)]
pub struct AudioService;

impl AudioService {
    /// Creates a new audio service adapter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Queries the current audio volume and mute state via `wpctl` or `pactl`.
    #[must_use]
    pub fn query_status(&self) -> AudioStatus {
        let mut status = AudioStatus::default();

        // 1. Try PipeWire's WirePlumber CLI: `wpctl get-volume @DEFAULT_AUDIO_SINK@`
        if let Ok(output) = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(rest) = text.strip_prefix("Volume:") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if let Some(vol_str) = parts.first()
                    && let Ok(vol_f) = vol_str.parse::<f32>()
                {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        status.volume_percent = ((vol_f * 100.0).round() as u8).min(100);
                    }
                }
                status.muted = text.contains("[MUTED]");
                "wireplumber".clone_into(&mut status.backend);
                return status;
            }
        }

        // 2. Fallback to `pactl get-sink-volume @DEFAULT_SINK@`
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(idx) = text.find('%') {
                let prefix = &text[..idx];
                if let Some(last_space) = prefix.rfind(' ')
                    && let Ok(val) = prefix[last_space + 1..].parse::<u8>()
                {
                    status.volume_percent = val.min(100);
                    "pulseaudio".clone_into(&mut status.backend);
                }
            }
        }

        status
    }

    /// Sets master output volume (`0..=100`) via `wpctl` or `pactl`.
    #[must_use]
    pub fn set_volume(&self, percent: u8) -> bool {
        let clamped = percent.min(100);
        let wpctl_arg = format_wpctl_volume(clamped);
        if Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &wpctl_arg])
            .status()
            .is_ok_and(|s| s.success())
        {
            return true;
        }
        let pactl_arg = format!("{clamped}%");
        Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &pactl_arg])
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Toggles master output mute via `wpctl` or `pactl`.
    #[must_use]
    pub fn toggle_mute(&self) -> bool {
        if Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .status()
            .is_ok_and(|s| s.success())
        {
            return true;
        }
        Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// Formats `percent` (`0..=100`) into `wpctl` scalar volume string (e.g. `"0.75"`).
#[must_use]
pub fn format_wpctl_volume(percent: u8) -> String {
    let clamped = percent.min(100);
    format!("{:.2}", f32::from(clamped) / 100.0)
}

/// Returns current local wall-clock time as `"HH:MM"` using system `date` (with UTC fallback).
#[must_use]
pub fn local_clock_hhmm() -> String {
    if let Ok(output) = Command::new("date").arg("+%H:%M").output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if text.len() == 5 && text.as_bytes().get(2) == Some(&b':') {
            return text;
        }
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mins_day = (secs / 60) % (24 * 60);
    let hh = mins_day / 60;
    let mm = mins_day % 60;
    format!("{hh:02}:{mm:02}")
}

/// Session management service interacting with `systemd-logind`.
#[derive(Debug, Default, Clone)]
pub struct SessionService;

impl SessionService {
    /// Creates a new session service adapter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Requests system suspension (sleep) via logind.
    #[must_use]
    pub fn request_suspend(&self) -> bool {
        Command::new("systemctl")
            .arg("suspend")
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Requests system reboot via logind.
    #[must_use]
    pub fn request_reboot(&self) -> bool {
        Command::new("systemctl")
            .arg("reboot")
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Requests system poweroff via logind.
    #[must_use]
    pub fn request_poweroff(&self) -> bool {
        Command::new("systemctl")
            .arg("poweroff")
            .status()
            .is_ok_and(|s| s.success())
    }
}

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
    }

    #[test]
    fn networkmanager_and_bluetooth_formatters_handle_all_states() {
        let wifi = format_nm_network_status("802-11-wireless", Some("Studio5G".to_owned()));
        assert!(wifi.connected);
        assert_eq!(wifi.wifi_ssid.as_deref(), Some("Studio5G"));
        assert_eq!(wifi.status_label, "Wi-Fi: Studio5G");

        let eth = format_nm_network_status("802-3-ethernet", None);
        assert!(eth.connected);
        assert_eq!(eth.wifi_ssid, None);
        assert_eq!(eth.status_label, "Ethernet Connected");

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
}
