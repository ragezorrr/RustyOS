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
    /// Live Wi-Fi signal strength percentage (`0..=100`) when connected over wireless (`RUSTYOS-052`).
    pub wifi_signal_percent: Option<u8>,
    /// Human-readable status label for `TopBar`.
    pub status_label: String,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            connected: false,
            interface: None,
            wifi_ssid: None,
            wifi_signal_percent: None,
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

/// Display backlight brightness status (`RUSTYOS-031`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrightnessStatus {
    /// True if at least one sysfs backlight device was detected.
    pub available: bool,
    /// Current backlight brightness percentage (0..=100).
    pub percentage: u8,
    /// Sysfs device name (e.g. `"intel_backlight"`, `"amdgpu_bl1"`).
    pub device: Option<String>,
}

impl Default for BrightnessStatus {
    fn default() -> Self {
        Self {
            available: false,
            percentage: 100,
            device: None,
        }
    }
}

/// Media playback control command dispatched via MPRIS2 / `playerctl` (`RUSTYOS-031`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    /// Toggle play/pause on the active player.
    PlayPause,
    /// Skip to next track.
    Next,
    /// Return to previous track.
    Previous,
    /// Stop playback.
    Stop,
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
        let mut saw_adapter = false;
        let mut any_ac_online = false;

        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let supply_type = fs::read_to_string(path.join("type"))
                .map(|s| s.trim().to_lowercase())
                .unwrap_or_default();

            let is_mains =
                supply_type == "mains" || name.starts_with("ac") || name.starts_with("adp");
            let is_usb_pd = name.starts_with("ucsi") || supply_type.starts_with("usb");
            if (is_mains || is_usb_pd)
                && let Ok(online_str) = fs::read_to_string(path.join("online"))
            {
                saw_adapter = true;
                if online_str.trim() == "1" {
                    any_ac_online = true;
                }
            }

            // Check Battery (BAT0, BAT1, etc.)
            if supply_type == "battery" || name.starts_with("bat") {
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

        if status.state == BatteryState::Charging {
            any_ac_online = true;
        }
        if saw_adapter || status.has_battery {
            status.ac_online = any_ac_online;
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

        if status.connected {
            let is_wifi_iface = status
                .interface
                .as_deref()
                .is_some_and(|iface| iface.starts_with('w'));
            if is_wifi_iface && let Ok(wireless) = fs::read_to_string("/proc/net/wireless") {
                status.wifi_signal_percent =
                    parse_proc_net_wireless_signal(&wireless, status.interface.as_deref());
            }
            if let Ok(output) = Command::new("nmcli")
                .args(["-t", "-f", "active,signal,ssid", "dev", "wifi"])
                .output()
                && output.status.success()
            {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    if let Some(rest) = line.strip_prefix("yes:")
                        && let Some((sig_raw, ssid_raw)) = rest.split_once(':')
                    {
                        if let Ok(sig) = sig_raw.trim().parse::<u8>() {
                            status.wifi_signal_percent = Some(sig.min(100));
                        }
                        let trimmed = ssid_raw.trim();
                        if !trimmed.is_empty() {
                            status.wifi_ssid = Some(trimmed.to_owned());
                            status.status_label = format!("Wi-Fi: {trimmed}");
                        }
                        break;
                    }
                }
            }
        }

        status
    }
}

/// Parses Linux `/proc/net/wireless` link quality (`0..=70` or percentage) into `0..=100` (`RUSTYOS-052`).
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn parse_proc_net_wireless_signal(contents: &str, iface_hint: Option<&str>) -> Option<u8> {
    for line in contents.lines().skip(2) {
        let Some((iface_part, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = iface_part.trim();
        if let Some(hint) = iface_hint
            && iface != hint
        {
            continue;
        }
        let mut fields = rest.split_whitespace();
        let _status_hex = fields.next()?;
        let Some(link_raw) = fields.next() else {
            continue;
        };
        let cleaned = link_raw.trim_end_matches('.');
        if let Ok(link_val) = cleaned.parse::<f32>() {
            let pct = if link_val > 70.0 {
                link_val.clamp(0.0, 100.0)
            } else {
                (link_val / 70.0 * 100.0).clamp(0.0, 100.0)
            };
            return Some(pct.round() as u8);
        }
    }
    None
}

/// Maps live Wi-Fi connectivity and signal percentage (`0..=100`) to `0..=3` macOS Wi-Fi bars (`RUSTYOS-052`).
#[must_use]
pub const fn wifi_signal_bars(connected: bool, signal_percent: Option<u8>) -> u8 {
    if !connected {
        return 0;
    }
    let Some(pct) = signal_percent else {
        return 3;
    };
    if pct == 0 {
        0
    } else if pct <= 28 {
        1
    } else if pct <= 54 {
        2
    } else {
        3
    }
}

fn query_wireless_signal_dbus(
    conn: &zbus::blocking::Connection,
    ac_props: &zbus::blocking::fdo::PropertiesProxy<'_>,
    ac_iface: &zbus::names::InterfaceName<'_>,
) -> (Option<String>, Option<u8>) {
    let Ok(devs_val) = ac_props.get(ac_iface.clone(), "Devices") else {
        return (None, None);
    };
    let Value::Array(arr) = &*devs_val else {
        return (None, None);
    };
    let Some(Value::ObjectPath(dev_path)) = arr.first() else {
        return (None, None);
    };
    let Ok(dev_iface) =
        zbus::names::InterfaceName::from_static_str("org.freedesktop.NetworkManager.Device")
    else {
        return (None, None);
    };
    let Ok(wifi_iface) = zbus::names::InterfaceName::from_static_str(
        "org.freedesktop.NetworkManager.Device.Wireless",
    ) else {
        return (None, None);
    };
    let Ok(access_point_iface) =
        zbus::names::InterfaceName::from_static_str("org.freedesktop.NetworkManager.AccessPoint")
    else {
        return (None, None);
    };
    let Ok(dev_builder) = zbus::blocking::fdo::PropertiesProxy::builder(conn)
        .destination("org.freedesktop.NetworkManager")
        .and_then(|b| b.path(dev_path.as_str().to_owned()))
    else {
        return (None, None);
    };
    let Ok(dev_props) = dev_builder.build() else {
        return (None, None);
    };
    let iface_name = dev_props.get(dev_iface, "Interface").ok().and_then(|v| {
        if let Value::Str(s) = &*v {
            Some(s.as_str().to_owned())
        } else {
            None
        }
    });
    let Ok(access_point_val) = dev_props.get(wifi_iface, "ActiveAccessPoint") else {
        return (iface_name, None);
    };
    let Value::ObjectPath(access_point_path) = &*access_point_val else {
        return (iface_name, None);
    };
    if access_point_path.as_str() == "/" {
        return (iface_name, None);
    }
    let Ok(access_point_builder) = zbus::blocking::fdo::PropertiesProxy::builder(conn)
        .destination("org.freedesktop.NetworkManager")
        .and_then(|b| b.path(access_point_path.as_str().to_owned()))
    else {
        return (iface_name, None);
    };
    let Ok(access_point_props) = access_point_builder.build() else {
        return (iface_name, None);
    };
    let strength = access_point_props
        .get(access_point_iface, "Strength")
        .ok()
        .and_then(|v| {
            if let Value::U8(s) = &*v {
                Some((*s).min(100))
            } else {
                None
            }
        });
    (iface_name, strength)
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
    let is_wifi = conn_type.contains("wireless") || conn_type.contains("wifi");

    let mut active_id = None;
    let mut active_iface = None;
    let mut wifi_signal_percent = None;
    if let Some(Value::ObjectPath(primary_path)) = all.get("PrimaryConnection").map(|v| &**v)
        && primary_path.as_str() != "/"
        && let Ok(ac_iface) = zbus::names::InterfaceName::from_static_str(
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        && let Ok(path_builder) = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
            .destination("org.freedesktop.NetworkManager")
            .and_then(|b| b.path(primary_path.as_str().to_owned()))
        && let Ok(ac_props) = path_builder.build()
    {
        if let Ok(id_val) = ac_props.get(ac_iface.clone(), "Id")
            && let Value::Str(id_str) = &*id_val
            && !id_str.as_str().trim().is_empty()
        {
            active_id = Some(id_str.as_str().to_owned());
        }
        if is_wifi {
            (active_iface, wifi_signal_percent) =
                query_wireless_signal_dbus(&conn, &ac_props, &ac_iface);
        }
    }
    if is_wifi
        && wifi_signal_percent.is_none()
        && let Ok(wireless) = fs::read_to_string("/proc/net/wireless")
    {
        wifi_signal_percent = parse_proc_net_wireless_signal(&wireless, active_iface.as_deref());
    }

    Some(format_nm_network_status_with_signal(
        &conn_type,
        active_id,
        active_iface,
        wifi_signal_percent,
    ))
}

/// Formats NetworkManager connection type & profile ID into [`NetworkStatus`].
#[must_use]
pub fn format_nm_network_status(conn_type: &str, active_id: Option<String>) -> NetworkStatus {
    format_nm_network_status_with_signal(conn_type, active_id, None, None)
}

/// Formats NetworkManager connection metadata and live Wi-Fi signal strength into [`NetworkStatus`] (`RUSTYOS-052`).
#[must_use]
pub fn format_nm_network_status_with_signal(
    conn_type: &str,
    active_id: Option<String>,
    interface: Option<String>,
    wifi_signal_percent: Option<u8>,
) -> NetworkStatus {
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
        interface,
        wifi_ssid: if is_wifi { clean_id } else { None },
        wifi_signal_percent: if is_wifi {
            wifi_signal_percent.map(|p| p.min(100))
        } else {
            None
        },
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
        if matches!(bluez_dbus_has_owner(), Some(false)) {
            return false;
        }
        let arg = if powered { "on" } else { "off" };
        Command::new("bluetoothctl")
            .args(["--timeout", "2", "power", arg])
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

fn bluez_dbus_has_owner() -> Option<bool> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let dbus = zbus::blocking::fdo::DBusProxy::new(&conn).ok()?;
    let bus_name = zbus::names::BusName::try_from("org.bluez").ok()?;
    dbus.name_has_owner(bus_name).ok()
}

fn query_bluetooth_fallback() -> BluetoothStatus {
    let sysfs_present = fs::read_dir("/sys/class/bluetooth").is_ok_and(|mut d| d.next().is_some());
    let mut status = BluetoothStatus {
        available: sysfs_present,
        powered: false,
        connected_device: None,
        status_label: format_bluetooth_label(sysfs_present, false, None),
    };
    if matches!(bluez_dbus_has_owner(), Some(false)) {
        return status;
    }
    if let Ok(output) = Command::new("bluetoothctl")
        .args(["--timeout", "1", "show"])
        .output()
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

/// Display backlight service using `/sys/class/backlight`, `systemd-logind` D-Bus, and `brightnessctl` (`RUSTYOS-031`).
#[derive(Debug, Clone)]
pub struct BrightnessService {
    base_path: String,
}

impl Default for BrightnessService {
    fn default() -> Self {
        Self::new()
    }
}

impl BrightnessService {
    /// Creates a new backlight service targeting `/sys/class/backlight`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base_path: "/sys/class/backlight".to_owned(),
        }
    }

    fn select_device(&self) -> Option<(String, std::path::PathBuf, u64, u64)> {
        let p = Path::new(&self.base_path);
        let entries = fs::read_dir(p).ok()?;
        let mut candidates: Vec<(String, std::path::PathBuf, u64, u64)> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_string_lossy().into_owned();
                let cur: u64 = fs::read_to_string(path.join("brightness"))
                    .ok()?
                    .trim()
                    .parse()
                    .ok()?;
                let max: u64 = fs::read_to_string(path.join("max_brightness"))
                    .ok()?
                    .trim()
                    .parse()
                    .ok()?;
                (max > 0).then_some((name, path, cur, max))
            })
            .collect();
        if candidates.is_empty() {
            return None;
        }
        candidates.sort_by_key(|(name, _, _, _)| {
            let prio = if name.contains("intel") || name.contains("amdgpu") {
                0
            } else if name.contains("nvidia") || name.contains("gmux") {
                1
            } else {
                2
            };
            (prio, name.clone())
        });
        candidates.into_iter().next()
    }

    /// Queries the current primary backlight percentage.
    #[must_use]
    pub fn query_status(&self) -> BrightnessStatus {
        let Some((name, _path, cur, max)) = self.select_device() else {
            return BrightnessStatus::default();
        };
        let pct = calc_backlight_percent(cur, max);
        BrightnessStatus {
            available: true,
            percentage: pct,
            device: Some(name),
        }
    }

    /// Sets primary backlight brightness (`1..=100`% if positive, clamped to `0..=100`).
    #[must_use]
    pub fn set_percent(&self, percent: u8) -> bool {
        let clamped = percent.min(100);
        let Some((name, path, _cur, max)) = self.select_device() else {
            return false;
        };
        let raw = calc_backlight_raw(clamped, max);

        // 1. Direct sysfs write (when permissions allow).
        if fs::write(path.join("brightness"), format!("{raw}\n")).is_ok() {
            return true;
        }

        // 2. systemd-logind Session.SetBrightness over System D-Bus (no root needed).
        if set_backlight_via_logind_dbus(&name, raw) {
            return true;
        }

        // 3. Fallback to `brightnessctl`.
        let pct_arg = format!("{clamped}%");
        Command::new("brightnessctl")
            .args(["-d", &name, "set", &pct_arg])
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// Converts raw `(current, max)` sysfs brightness values to a `0..=100` percentage.
#[must_use]
pub fn calc_backlight_percent(current: u64, max: u64) -> u8 {
    if max == 0 {
        return 100;
    }
    let rounded = (current.saturating_mul(100).saturating_add(max / 2)) / max;
    u8::try_from(rounded.min(100)).unwrap_or(100)
}

/// Converts a `0..=100` percentage to a raw hardware brightness value in `0..=max`.
#[must_use]
pub fn calc_backlight_raw(percent: u8, max: u64) -> u64 {
    if max == 0 {
        return 0;
    }
    let p = u64::from(percent.min(100));
    let raw = (max.saturating_mul(p).saturating_add(50)) / 100;
    if p > 0 { raw.max(1).min(max) } else { 0 }
}

fn set_backlight_via_logind_dbus(device: &str, raw_value: u64) -> bool {
    let Ok(raw_u32) = u32::try_from(raw_value) else {
        return false;
    };
    let Ok(conn) = zbus::blocking::Connection::system() else {
        return false;
    };
    conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1/session/auto",
        Some("org.freedesktop.login1.Session"),
        "SetBrightness",
        &("backlight", device, raw_u32),
    )
    .is_ok()
}

/// Media player control service interacting with MPRIS2 over Session D-Bus and `playerctl` (`RUSTYOS-031`).
#[derive(Debug, Default, Clone)]
pub struct MediaService;

impl MediaService {
    /// Creates a new MPRIS2/playerctl media service.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Sends a media playback control action to the first active MPRIS2 player (or `playerctl`).
    #[must_use]
    pub fn dispatch(&self, action: MediaAction) -> bool {
        if dispatch_mpris_via_dbus(action) {
            return true;
        }
        let arg = match action {
            MediaAction::PlayPause => "play-pause",
            MediaAction::Next => "next",
            MediaAction::Previous => "previous",
            MediaAction::Stop => "stop",
        };
        Command::new("playerctl")
            .arg(arg)
            .status()
            .is_ok_and(|s| s.success())
    }
}

fn dispatch_mpris_via_dbus(action: MediaAction) -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return false;
    };
    let Ok(names) = dbus.list_names() else {
        return false;
    };
    let Some(player_bus) = names
        .into_iter()
        .find(|n| n.as_str().starts_with("org.mpris.MediaPlayer2."))
    else {
        return false;
    };
    let method = match action {
        MediaAction::PlayPause => "PlayPause",
        MediaAction::Next => "Next",
        MediaAction::Previous => "Previous",
        MediaAction::Stop => "Stop",
    };
    conn.call_method(
        Some(player_bus.as_str()),
        "/org/mpris/MediaPlayer2",
        Some("org.mpris.MediaPlayer2.Player"),
        method,
        &(),
    )
    .is_ok()
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

/// Returns compact macOS-style `TopBar` date + time in neutral C locale (`RUSTYOS-049`), falling back to `HH:MM`.
///
/// - When `use_24h == false` (default): `"Wed Apr 1  9:41 AM"`
/// - When `use_24h == true`: `"Wed Apr 1  09:41"`
#[must_use]
pub fn local_clock_short_datetime_fmt(use_24h: bool) -> String {
    let fmt_arg = if use_24h {
        "+%a %b %-d  %H:%M"
    } else {
        "+%a %b %-d  %-I:%M %p"
    };
    if let Ok(output) = Command::new("date")
        .env("LC_ALL", "C")
        .arg(fmt_arg)
        .output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !text.is_empty() && text.len() <= 28 {
            return text;
        }
    }
    local_clock_hhmm()
}

/// Returns compact macOS-style `TopBar` date + time (`"Wed Apr 1  9:41 AM"`), falling back to `HH:MM` (`RUSTYOS-048`/`049`).
#[must_use]
pub fn local_clock_short_datetime() -> String {
    local_clock_short_datetime_fmt(false)
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
    /// Display backlight brightness service (`RUSTYOS-031`).
    pub brightness: BrightnessService,
    /// MPRIS2 media control service (`RUSTYOS-031`).
    pub media: MediaService,
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
}
