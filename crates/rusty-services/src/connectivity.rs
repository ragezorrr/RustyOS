//! NetworkManager and BlueZ connectivity adapters (`RUSTYOS-011`, `RUSTYOS-027`, `RUSTYOS-052`).

use std::fs;
use std::path::Path;
use std::process::Command;

use zbus::zvariant::Value;

/// Network connectivity status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkStatus {
    /// True if an active network interface has an IP and is connected.
    pub connected: bool,
    /// True if the system Wi-Fi radio is enabled (`RUSTYOS-053`).
    pub wifi_enabled: bool,
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
            wifi_enabled: false,
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
            if is_wifi_iface {
                status.wifi_enabled = true;
                if let Ok(wireless) = fs::read_to_string("/proc/net/wireless") {
                    status.wifi_signal_percent =
                        parse_proc_net_wireless_signal(&wireless, status.interface.as_deref());
                }
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
                        status.wifi_enabled = true;
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

    /// Enables or disables the system Wi-Fi radio via `NetworkManager` D-Bus or `nmcli` (`RUSTYOS-053`).
    #[must_use]
    pub fn set_wifi_enabled(&self, enabled: bool) -> bool {
        if set_nm_wifi_enabled_via_dbus(enabled) {
            return true;
        }
        let arg = if enabled { "on" } else { "off" };
        Command::new("nmcli")
            .args(["radio", "wifi", arg])
            .status()
            .is_ok_and(|s| s.success())
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
    let wireless_enabled = matches!(
        all.get("WirelessEnabled").map(|v| &**v),
        Some(Value::Bool(true))
    );
    // NetworkManager states >= 50 correspond to ConnectedLocal / ConnectedSite / ConnectedGlobal.
    if nm_state < 50 {
        return Some(NetworkStatus {
            wifi_enabled: wireless_enabled,
            ..NetworkStatus::default()
        });
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

    let mut status = format_nm_network_status_with_signal(
        &conn_type,
        active_id,
        active_iface,
        wifi_signal_percent,
    );
    status.wifi_enabled = wireless_enabled || status.wifi_ssid.is_some();
    Some(status)
}

fn set_nm_wifi_enabled_via_dbus(enabled: bool) -> bool {
    let Ok(conn) = zbus::blocking::Connection::system() else {
        return false;
    };
    let Ok(iface_name) =
        zbus::names::InterfaceName::from_static_str("org.freedesktop.NetworkManager")
    else {
        return false;
    };
    let Ok(props_builder) = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
        .destination("org.freedesktop.NetworkManager")
        .and_then(|b| b.path("/org/freedesktop/NetworkManager"))
    else {
        return false;
    };
    let Ok(props) = props_builder.build() else {
        return false;
    };
    props
        .set(iface_name, "WirelessEnabled", Value::Bool(enabled))
        .is_ok()
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
        wifi_enabled: is_wifi,
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
