//! NetworkManager and BlueZ connectivity adapters (`RUSTYOS-011`, `RUSTYOS-027`, `RUSTYOS-052`).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use zbus::zvariant::Value;

/// Discovered or saved wireless access point entry (`RUSTYOS-108`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiAccessPoint {
    /// Wireless network SSID.
    pub ssid: String,
    /// Signal strength percentage (`0..=100`).
    pub signal_percent: u8,
    /// True if the network requires WPA/WPA2/WPA3/WEP authentication.
    pub secure: bool,
    /// True if this access point is currently connected.
    pub connected: bool,
    /// True if a saved NetworkManager connection profile exists for this SSID (or it is connected).
    pub known: bool,
}

/// Network connectivity status.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct NetworkStatus {
    /// True if an active network interface has an IP and is connected.
    pub connected: bool,
    /// True if a wired Ethernet interface is connected (`RUSTYOS-108`).
    pub wired_connected: bool,
    /// Name of the active wired Ethernet interface, if connected (`RUSTYOS-108`).
    pub wired_interface: Option<String>,
    /// Human-readable label for the wired Ethernet connection (`RUSTYOS-108`).
    pub wired_label: Option<String>,
    /// True if a wireless interface is actively connected to an SSID (`RUSTYOS-108`).
    pub wifi_connected: bool,
    /// True if the system Wi-Fi radio is enabled (`RUSTYOS-053`).
    pub wifi_enabled: bool,
    /// Name of the primary active interface (e.g., "wlan0", "eth0").
    pub interface: Option<String>,
    /// Wi-Fi SSID if connected via wireless.
    pub wifi_ssid: Option<String>,
    /// Live Wi-Fi signal strength percentage (`0..=100`) when connected over wireless (`RUSTYOS-052`).
    pub wifi_signal_percent: Option<u8>,
    /// Available and known Wi-Fi networks sorted by connection/known/signal (`RUSTYOS-108`).
    pub wifi_networks: Vec<WifiAccessPoint>,
    /// Human-readable status label for `TopBar`.
    pub status_label: String,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            connected: false,
            wired_connected: false,
            wired_interface: None,
            wired_label: None,
            wifi_connected: false,
            wifi_enabled: false,
            interface: None,
            wifi_ssid: None,
            wifi_signal_percent: None,
            wifi_networks: Vec::new(),
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
        let mut wifi_iface = None;

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
                    if dev_name.starts_with('w') {
                        status.wifi_connected = true;
                        status.wifi_enabled = true;
                        if wifi_iface.is_none() {
                            wifi_iface = Some(dev_name.clone());
                        }
                    } else if !status.wired_connected {
                        status.wired_connected = true;
                        status.wired_interface = Some(dev_name.clone());
                        status.wired_label = Some(format!("Ethernet ({dev_name})"));
                    }
                    if status.interface.is_none() {
                        status.interface = Some(dev_name);
                    }
                    status.connected = true;
                }
            }
        }

        if let Some(iface) = wifi_iface.as_deref()
            && let Ok(wireless) = fs::read_to_string("/proc/net/wireless")
        {
            status.wifi_signal_percent = parse_proc_net_wireless_signal(&wireless, Some(iface));
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
                    status.connected = true;
                    status.wifi_connected = true;
                    status.wifi_enabled = true;
                    if let Ok(sig) = sig_raw.trim().parse::<u8>() {
                        status.wifi_signal_percent = Some(sig.min(100));
                    }
                    let trimmed = ssid_raw.trim();
                    if !trimmed.is_empty() {
                        status.wifi_ssid = Some(trimmed.to_owned());
                    }
                    break;
                }
            }
        }

        if status.wifi_enabled {
            status.wifi_networks = list_wifi_networks_cached(status.wifi_ssid.as_deref());
        }

        status.status_label = if status.wifi_connected {
            status.wifi_ssid.as_ref().map_or_else(
                || "Wi-Fi Connected".to_owned(),
                |ssid| format!("Wi-Fi: {ssid}"),
            )
        } else if status.wired_connected {
            status
                .wired_label
                .clone()
                .unwrap_or_else(|| "Ethernet Connected".to_owned())
        } else if status.wifi_enabled {
            "Wi-Fi: On".to_owned()
        } else {
            "Disconnected".to_owned()
        };

        status
    }

    /// Lists nearby Wi-Fi networks using cached scan data and marks saved profiles (`RUSTYOS-108`).
    #[must_use]
    pub fn list_wifi_networks(&self) -> Vec<WifiAccessPoint> {
        let status = self.query_status();
        list_wifi_networks_cached(status.wifi_ssid.as_deref())
    }

    /// Triggers a fresh Wi-Fi scan via `nmcli` and returns sorted access points (`RUSTYOS-117`).
    #[must_use]
    pub fn rescan_wifi_networks(&self) -> Vec<WifiAccessPoint> {
        let status = self.query_status();
        let active = status.wifi_ssid.as_deref();
        let fresh = list_wifi_networks_with_rescan(active, true);
        if fresh.is_empty() {
            list_wifi_networks_with_rescan(active, false)
        } else {
            fresh
        }
    }

    /// Connects to a Wi-Fi SSID (with optional passphrase) via `nmcli`, returning clear diagnostics (`RUSTYOS-117`).
    ///
    /// # Errors
    ///
    /// Returns a human-readable `nmcli` error message when the SSID is empty or connection fails.
    pub fn connect_wifi_with_password(
        &self,
        ssid: &str,
        password: Option<&str>,
    ) -> Result<(), String> {
        let trimmed = ssid.trim();
        if trimmed.is_empty() {
            return Err(String::from("Wi-Fi network name cannot be empty"));
        }
        let clean_pw = password.filter(|pw| !pw.is_empty());
        if clean_pw.is_none()
            && Command::new("nmcli")
                .args(["connection", "up", "id", trimmed])
                .status()
                .is_ok_and(|s| s.success())
        {
            return Ok(());
        }

        let mut cmd = Command::new("nmcli");
        cmd.args(["device", "wifi", "connect", trimmed]);
        if let Some(pw) = clean_pw {
            cmd.args(["password", pw]);
        }
        match cmd.output() {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
                let msg = if err.is_empty() {
                    String::from_utf8_lossy(&out.stdout).trim().to_owned()
                } else {
                    err
                };
                Err(if msg.is_empty() {
                    format!("Failed to connect to Wi-Fi '{trimmed}'")
                } else {
                    msg
                })
            }
            Err(err) => Err(format!("Unable to run nmcli: {err}")),
        }
    }

    /// Connects to the requested Wi-Fi network via `nmcli` (`RUSTYOS-108`).
    #[must_use]
    pub fn connect_wifi(&self, ssid: &str) -> bool {
        self.connect_wifi_with_password(ssid, None).is_ok()
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

/// Parses `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY dev wifi list` output into sorted [`WifiAccessPoint`] items (`RUSTYOS-108`).
#[must_use]
pub fn parse_nmcli_wifi_list(
    raw: &str,
    saved_ssids: &[String],
    active_ssid: Option<&str>,
) -> Vec<WifiAccessPoint> {
    let mut by_ssid: HashMap<String, WifiAccessPoint> = HashMap::new();
    for line in raw.lines() {
        let Some((in_use_raw, rest)) = line.split_once(':') else {
            continue;
        };
        let Some((rest_ssid_sig, security_raw)) = rest.rsplit_once(':') else {
            continue;
        };
        let Some((ssid_raw, signal_raw)) = rest_ssid_sig.rsplit_once(':') else {
            continue;
        };
        let ssid = ssid_raw.replace("\\:", ":").trim().to_owned();
        if ssid.is_empty() {
            continue;
        }
        let signal_percent = signal_raw.trim().parse::<u8>().unwrap_or(0).min(100);
        let sec_trim = security_raw.trim();
        let secure = !sec_trim.is_empty() && sec_trim != "--";
        let connected =
            in_use_raw.trim() == "*" || active_ssid.is_some_and(|active| active == ssid);
        let known = connected || saved_ssids.iter().any(|s| s == &ssid);

        by_ssid
            .entry(ssid.clone())
            .and_modify(|existing| {
                existing.connected |= connected;
                existing.known |= known;
                existing.secure |= secure;
                if signal_percent > existing.signal_percent {
                    existing.signal_percent = signal_percent;
                }
            })
            .or_insert(WifiAccessPoint {
                ssid,
                signal_percent,
                secure,
                connected,
                known,
            });
    }

    let mut list: Vec<WifiAccessPoint> = by_ssid.into_values().collect();
    list.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| b.known.cmp(&a.known))
            .then_with(|| b.signal_percent.cmp(&a.signal_percent))
            .then_with(|| {
                a.ssid
                    .to_ascii_lowercase()
                    .cmp(&b.ssid.to_ascii_lowercase())
            })
    });
    list
}

fn query_saved_wifi_ssids_nmcli() -> Vec<String> {
    let Ok(output) = Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE", "connection", "show"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| {
            let (name, conn_type) = line.rsplit_once(':')?;
            if conn_type.trim() == "802-11-wireless" {
                let clean = name.replace("\\:", ":").trim().to_owned();
                (!clean.is_empty()).then_some(clean)
            } else {
                None
            }
        })
        .collect()
}

fn list_wifi_networks_with_rescan(active_ssid: Option<&str>, rescan: bool) -> Vec<WifiAccessPoint> {
    let rescan_arg = if rescan { "yes" } else { "no" };
    let Ok(output) = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "dev",
            "wifi",
            "list",
            "--rescan",
            rescan_arg,
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let saved = query_saved_wifi_ssids_nmcli();
    let raw = String::from_utf8_lossy(&output.stdout);
    parse_nmcli_wifi_list(&raw, &saved, active_ssid)
}

fn list_wifi_networks_cached(active_ssid: Option<&str>) -> Vec<WifiAccessPoint> {
    list_wifi_networks_with_rescan(active_ssid, false)
}

fn query_active_device_signal_dbus(
    conn: &zbus::blocking::Connection,
    ac_props: &zbus::blocking::fdo::PropertiesProxy<'_>,
    ac_iface: &zbus::names::InterfaceName<'_>,
    is_wifi: bool,
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
    if !is_wifi {
        return (iface_name, None);
    }
    let Ok(wifi_iface) = zbus::names::InterfaceName::from_static_str(
        "org.freedesktop.NetworkManager.Device.Wireless",
    ) else {
        return (iface_name, None);
    };
    let Ok(access_point_iface) =
        zbus::names::InterfaceName::from_static_str("org.freedesktop.NetworkManager.AccessPoint")
    else {
        return (iface_name, None);
    };
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
        let wifi_networks = if wireless_enabled {
            list_wifi_networks_cached(None)
        } else {
            Vec::new()
        };
        return Some(NetworkStatus {
            wifi_enabled: wireless_enabled,
            wifi_networks,
            status_label: if wireless_enabled {
                "Wi-Fi: On".to_owned()
            } else {
                "Disconnected".to_owned()
            },
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
        (active_iface, wifi_signal_percent) =
            query_active_device_signal_dbus(&conn, &ac_props, &ac_iface, is_wifi);
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
    if status.wifi_enabled {
        status.wifi_networks = list_wifi_networks_cached(status.wifi_ssid.as_deref());
        // If Ethernet is primary while a secondary Wi-Fi interface is also connected, record it too.
        if !status.wifi_connected
            && let Some(active_ap) = status.wifi_networks.iter().find(|ap| ap.connected)
        {
            status.wifi_connected = true;
            status.wifi_ssid = Some(active_ap.ssid.clone());
            status.wifi_signal_percent = Some(active_ap.signal_percent);
        }
    }
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
    let is_wired = !is_wifi && !conn_type.trim().is_empty();
    let status_label = match (&clean_id, is_wifi) {
        (Some(name), true) => format!("Wi-Fi: {name}"),
        (Some(name), false) => name.clone(),
        (None, true) => "Wi-Fi Connected".to_owned(),
        (None, false) => "Ethernet Connected".to_owned(),
    };
    NetworkStatus {
        connected: true,
        wired_connected: is_wired,
        wired_interface: if is_wired { interface.clone() } else { None },
        wired_label: if is_wired {
            Some(
                clean_id
                    .clone()
                    .unwrap_or_else(|| "Ethernet Connected".to_owned()),
            )
        } else {
            None
        },
        wifi_connected: is_wifi,
        wifi_enabled: is_wifi,
        interface,
        wifi_ssid: if is_wifi { clean_id } else { None },
        wifi_signal_percent: if is_wifi {
            wifi_signal_percent.map(|p| p.min(100))
        } else {
            None
        },
        wifi_networks: Vec::new(),
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
