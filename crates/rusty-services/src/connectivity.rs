//! NetworkManager and BlueZ connectivity adapters (`RUSTYOS-011`, `RUSTYOS-027`, `RUSTYOS-052`).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

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

/// Discovered or paired Bluetooth device metadata (`RUSTYOS-131`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BluetoothDevice {
    /// Canonical uppercase MAC address (`AA:BB:CC:DD:EE:FF`).
    pub address: String,
    /// Human-readable device alias or fallback MAC address.
    pub name: String,
    /// `FreeDesktop` icon class hint (e.g. `"audio-headset"`, `"input-keyboard"`).
    pub icon: String,
    /// True if the device is paired/bonded with this adapter.
    pub paired: bool,
    /// True if an active connection exists to this device.
    pub connected: bool,
    /// True if the device is marked trusted by BlueZ.
    pub trusted: bool,
    /// Optional device battery percentage (`org.bluez.Battery1`, `0..=100`).
    pub battery_percent: Option<u8>,
}

/// Extended Bluetooth adapter and device snapshot for System Settings (`RUSTYOS-131`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BluetoothSnapshot {
    /// Adapter summary compatible with Quick Settings / `TopBar` telemetry.
    pub status: BluetoothStatus,
    /// Adapter display name/alias if present.
    pub adapter_name: Option<String>,
    /// Adapter hardware MAC address if present.
    pub adapter_address: Option<String>,
    /// True if the primary adapter is actively scanning for nearby devices.
    pub discovering: bool,
    /// Sorted list of known/discovered devices (connected first, then paired, then name).
    pub devices: Vec<BluetoothDevice>,
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
            && let Ok(out) = crate::run_command_bounded(
                Command::new("nmcli").args(["connection", "up", "id", "--", trimmed]),
                std::time::Duration::from_secs(15),
            )
        {
            if out.status.success() {
                return Ok(());
            }
            let detail = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            );
            if nmcli_requires_secrets(&detail) {
                return Err(format!("Wi-Fi network '{trimmed}' requires a password"));
            }
        }

        let pw_file_guard = if let Some(pw) = clean_pw {
            match TempNmcliPasswdFile::create(pw) {
                Ok(guard) => Some(guard),
                Err(err) => return Err(format!("Unable to prepare Wi-Fi credentials file: {err}")),
            }
        } else {
            None
        };
        let mut cmd = Command::new("nmcli");
        if let Some(ref guard) = pw_file_guard {
            cmd.arg("--passwd-file").arg(&guard.path);
        }
        cmd.args(["device", "wifi", "connect", "--", trimmed]);
        match crate::run_command_bounded(&mut cmd, std::time::Duration::from_secs(20)) {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
                let msg = if err.is_empty() {
                    String::from_utf8_lossy(&out.stdout).trim().to_owned()
                } else {
                    err
                };
                if nmcli_requires_secrets(&msg) {
                    Err(format!("Wi-Fi network '{trimmed}' requires a password"))
                } else {
                    Err(if msg.is_empty() {
                        format!("Failed to connect to Wi-Fi '{trimmed}'")
                    } else {
                        msg
                    })
                }
            }
            Err(err) => Err(format!("Unable to run nmcli: {err}")),
        }
    }

    /// Connects to the requested Wi-Fi network via `nmcli` (`RUSTYOS-108`).
    #[must_use]
    pub fn connect_wifi(&self, ssid: &str) -> bool {
        self.connect_wifi_with_password(ssid, None).is_ok()
    }

    /// Disconnects the active Wi-Fi connection profile for `ssid` via `nmcli` (`RUSTYOS-159`).
    ///
    /// # Errors
    ///
    /// Returns a human-readable diagnostic when `ssid` is empty or `nmcli` fails.
    pub fn disconnect_wifi(&self, ssid: &str) -> Result<(), String> {
        let trimmed = ssid.trim();
        if trimmed.is_empty() {
            return Err(String::from("Wi-Fi network name cannot be empty"));
        }
        run_nmcli_connection_action("down", trimmed)
    }

    /// Forgets (deletes) a saved NetworkManager Wi-Fi connection profile for `ssid` (`RUSTYOS-159`).
    ///
    /// # Errors
    ///
    /// Returns a human-readable diagnostic when `ssid` is empty or `nmcli` fails.
    pub fn forget_wifi(&self, ssid: &str) -> Result<(), String> {
        let trimmed = ssid.trim();
        if trimmed.is_empty() {
            return Err(String::from("Wi-Fi network name cannot be empty"));
        }
        run_nmcli_connection_action("delete", trimmed)
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
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

struct TempNmcliPasswdFile {
    path: std::path::PathBuf,
}

impl TempNmcliPasswdFile {
    fn create(password: &str) -> std::io::Result<Self> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "rustyos-nmcli-pw-{}-{seq}-{nanos}.tmp",
            std::process::id()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        // Format required by `nmcli --passwd-file` (never exposes WPA secret in `/proc/<pid>/cmdline`).
        let clean = password.replace(['\r', '\n'], "");
        writeln!(file, "802-11-wireless-security.psk:{clean}")?;
        file.flush()?;
        Ok(Self { path })
    }
}

impl Drop for TempNmcliPasswdFile {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.path, [0_u8; 64]);
        let _ = std::fs::remove_file(&self.path);
    }
}

fn run_nmcli_connection_action(action: &str, ssid: &str) -> Result<(), String> {
    match crate::run_command_bounded(
        Command::new("nmcli").args(["connection", action, "id", "--", ssid]),
        std::time::Duration::from_secs(12),
    ) {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format_nmcli_action_error(
            action,
            ssid,
            &String::from_utf8_lossy(&out.stderr),
            &String::from_utf8_lossy(&out.stdout),
        )),
        Err(err) => Err(format!("Unable to run nmcli: {err}")),
    }
}

fn format_nmcli_action_error(action: &str, ssid: &str, stderr: &str, stdout: &str) -> String {
    let err = stderr.trim();
    let msg = if err.is_empty() { stdout.trim() } else { err };
    if msg.is_empty() {
        format!("Failed to {action} Wi-Fi '{ssid}'")
    } else {
        msg.to_owned()
    }
}

fn nmcli_requires_secrets(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("secrets were required")
        || lower.contains("802-11-wireless-security.psk")
        || lower.contains("requires a password")
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
        self.query_snapshot().status
    }

    /// Queries detailed Bluetooth adapter state, discovery status, and devices (`RUSTYOS-131`).
    #[must_use]
    pub fn query_snapshot(&self) -> BluetoothSnapshot {
        if let Some(snap) = query_bluez_snapshot_via_dbus() {
            return snap;
        }
        query_bluetooth_snapshot_fallback()
    }

    /// Returns paired and discovered Bluetooth devices sorted by connection state (`RUSTYOS-131`).
    #[must_use]
    pub fn list_devices(&self) -> Vec<BluetoothDevice> {
        self.query_snapshot().devices
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
        run_bluetoothctl(&["--timeout", "2", "power", arg])
    }

    /// Starts or stops nearby Bluetooth discovery (`RUSTYOS-131`).
    #[must_use]
    pub fn set_discovering(&self, discovering: bool) -> bool {
        let method = if discovering {
            "StartDiscovery"
        } else {
            "StopDiscovery"
        };
        if call_bluez_adapter_method(method) {
            return true;
        }
        if matches!(bluez_dbus_has_owner(), Some(false)) {
            return false;
        }
        let arg = if discovering { "on" } else { "off" };
        run_bluetoothctl(&["--timeout", "3", "scan", arg])
    }

    /// Connects to the Bluetooth device identified by `address` (`RUSTYOS-131`).
    #[must_use]
    pub fn connect_device(&self, address: &str) -> bool {
        self.run_device_action(address, "Connect", "connect", "6")
    }

    /// Disconnects the Bluetooth device identified by `address` (`RUSTYOS-131`).
    #[must_use]
    pub fn disconnect_device(&self, address: &str) -> bool {
        self.run_device_action(address, "Disconnect", "disconnect", "4")
    }

    /// Pairs with the Bluetooth device identified by `address` (`RUSTYOS-131`).
    #[must_use]
    pub fn pair_device(&self, address: &str) -> bool {
        self.run_device_action(address, "Pair", "pair", "8")
    }

    /// Removes/forgets the Bluetooth device identified by `address` (`RUSTYOS-131`).
    #[must_use]
    pub fn remove_device(&self, address: &str) -> bool {
        let Some(mac) = normalize_bt_address(address) else {
            return false;
        };
        if remove_bluez_device_via_dbus(&mac) {
            return true;
        }
        if matches!(bluez_dbus_has_owner(), Some(false)) {
            return false;
        }
        run_bluetoothctl(&["--timeout", "4", "remove", &mac])
    }

    fn run_device_action(
        &self,
        address: &str,
        dbus_method: &str,
        cli_verb: &str,
        cli_timeout: &str,
    ) -> bool {
        let _ = self;
        let Some(mac) = normalize_bt_address(address) else {
            return false;
        };
        if call_bluez_device_method(&mac, dbus_method) {
            return true;
        }
        if matches!(bluez_dbus_has_owner(), Some(false)) {
            return false;
        }
        run_bluetoothctl(&["--timeout", cli_timeout, cli_verb, &mac])
    }
}

/// Validates and normalizes a `XX:XX:XX:XX:XX:XX` Bluetooth MAC address (`RUSTYOS-131`).
#[must_use]
pub fn normalize_bt_address(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let mut parts = trimmed.split(':');
    let mut normalized = Vec::with_capacity(6);
    for _ in 0..6 {
        let oct = parts.next()?;
        if oct.len() != 2 || !oct.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        normalized.push(oct.to_ascii_uppercase());
    }
    if parts.next().is_some() {
        return None;
    }
    Some(normalized.join(":"))
}

/// Parses `bluetoothctl devices` output lines (`Device AA:BB:CC:DD:EE:FF Name`) (`RUSTYOS-131`).
#[must_use]
pub fn parse_bluetoothctl_devices(output: &str) -> Vec<BluetoothDevice> {
    let mut devices = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("Device ") else {
            continue;
        };
        let mut split = rest.splitn(2, char::is_whitespace);
        let Some(raw_mac) = split.next() else {
            continue;
        };
        let Some(address) = normalize_bt_address(raw_mac) else {
            continue;
        };
        let name = split
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&address)
            .to_owned();
        if devices
            .iter()
            .any(|d: &BluetoothDevice| d.address == address)
        {
            continue;
        }
        devices.push(BluetoothDevice {
            address,
            name,
            icon: String::new(),
            paired: false,
            connected: false,
            trusted: false,
            battery_percent: None,
        });
    }
    devices
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

type BluezManagedObjects = HashMap<
    zbus::zvariant::OwnedObjectPath,
    HashMap<zbus::names::OwnedInterfaceName, HashMap<String, zbus::zvariant::OwnedValue>>,
>;

fn bluez_managed_objects() -> Option<(zbus::blocking::Connection, BluezManagedObjects)> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let om = zbus::blocking::fdo::ObjectManagerProxy::builder(&conn)
        .destination("org.bluez")
        .ok()?
        .path("/")
        .ok()?
        .build()
        .ok()?;
    let objects = om.get_managed_objects().ok()?;
    Some((conn, objects))
}

fn prop_bool(props: &HashMap<String, zbus::zvariant::OwnedValue>, key: &str) -> bool {
    matches!(props.get(key).map(|v| &**v), Some(Value::Bool(true)))
}

fn prop_str(props: &HashMap<String, zbus::zvariant::OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::Str(s) => Some(s.as_str().trim().to_owned()).filter(|t| !t.is_empty()),
        _ => None,
    })
}

fn prop_u8(props: &HashMap<String, zbus::zvariant::OwnedValue>, key: &str) -> Option<u8> {
    props.get(key).and_then(|v| match &**v {
        Value::U8(b) => Some((*b).min(100)),
        _ => None,
    })
}

fn query_bluez_snapshot_via_dbus() -> Option<BluetoothSnapshot> {
    let (_, objects) = bluez_managed_objects()?;

    let mut available = false;
    let mut powered = false;
    let mut discovering = false;
    let mut adapter_name = None;
    let mut adapter_address = None;
    let mut battery_by_path: HashMap<String, u8> = HashMap::new();
    let mut devices = Vec::new();

    for (path, ifaces) in &objects {
        if let Some(bat_props) = ifaces
            .iter()
            .find_map(|(k, v)| (k.as_str() == "org.bluez.Battery1").then_some(v))
            && let Some(pct) = prop_u8(bat_props, "Percentage")
        {
            battery_by_path.insert(path.as_str().to_owned(), pct);
        }
    }

    for (path, ifaces) in &objects {
        for (iface_name, props) in ifaces {
            if iface_name.as_str() == "org.bluez.Adapter1" {
                available = true;
                if prop_bool(props, "Powered") {
                    powered = true;
                }
                if prop_bool(props, "Discovering") {
                    discovering = true;
                }
                if adapter_name.is_none() {
                    adapter_name = prop_str(props, "Alias").or_else(|| prop_str(props, "Name"));
                }
                if adapter_address.is_none() {
                    adapter_address = prop_str(props, "Address");
                }
            } else if iface_name.as_str() == "org.bluez.Device1" {
                let Some(raw_addr) = prop_str(props, "Address") else {
                    continue;
                };
                let Some(address) = normalize_bt_address(&raw_addr) else {
                    continue;
                };
                let name = prop_str(props, "Alias")
                    .or_else(|| prop_str(props, "Name"))
                    .unwrap_or_else(|| address.clone());
                let icon = prop_str(props, "Icon").unwrap_or_default();
                let paired = prop_bool(props, "Paired");
                let connected = prop_bool(props, "Connected");
                let trusted = prop_bool(props, "Trusted");
                let battery_percent = battery_by_path.get(path.as_str()).copied();
                devices.push(BluetoothDevice {
                    address,
                    name,
                    icon,
                    paired,
                    connected,
                    trusted,
                    battery_percent,
                });
            }
        }
    }

    sort_bluetooth_devices(&mut devices);
    let connected_device = devices.iter().find(|d| d.connected).map(|d| d.name.clone());
    let status_label = format_bluetooth_label(available, powered, connected_device.as_deref());

    Some(BluetoothSnapshot {
        status: BluetoothStatus {
            available,
            powered,
            connected_device,
            status_label,
        },
        adapter_name,
        adapter_address,
        discovering,
        devices,
    })
}

fn sort_bluetooth_devices(devices: &mut [BluetoothDevice]) {
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| b.paired.cmp(&a.paired))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.address.cmp(&b.address))
    });
}

fn find_adapter_path(objects: &BluezManagedObjects) -> Option<zbus::zvariant::OwnedObjectPath> {
    objects.iter().find_map(|(path, ifaces)| {
        ifaces
            .keys()
            .any(|k| k.as_str() == "org.bluez.Adapter1")
            .then(|| path.clone())
    })
}

fn find_device_path(
    objects: &BluezManagedObjects,
    mac: &str,
) -> Option<zbus::zvariant::OwnedObjectPath> {
    objects.iter().find_map(|(path, ifaces)| {
        let dev_props = ifaces
            .iter()
            .find_map(|(k, v)| (k.as_str() == "org.bluez.Device1").then_some(v))?;
        let addr = prop_str(dev_props, "Address")?;
        (normalize_bt_address(&addr).as_deref() == Some(mac)).then(|| path.clone())
    })
}

fn call_bluez_adapter_method(method: &str) -> bool {
    let Some((conn, objects)) = bluez_managed_objects() else {
        return false;
    };
    let Some(adapter_path) = find_adapter_path(&objects) else {
        return false;
    };
    conn.call_method(
        Some("org.bluez"),
        adapter_path.as_str(),
        Some("org.bluez.Adapter1"),
        method,
        &(),
    )
    .is_ok()
}

fn call_bluez_device_method(mac: &str, method: &str) -> bool {
    let Some((conn, objects)) = bluez_managed_objects() else {
        return false;
    };
    let Some(dev_path) = find_device_path(&objects, mac) else {
        return false;
    };
    conn.call_method(
        Some("org.bluez"),
        dev_path.as_str(),
        Some("org.bluez.Device1"),
        method,
        &(),
    )
    .is_ok()
}

fn remove_bluez_device_via_dbus(mac: &str) -> bool {
    let Some((conn, objects)) = bluez_managed_objects() else {
        return false;
    };
    let Some(adapter_path) = find_adapter_path(&objects) else {
        return false;
    };
    let Some(dev_path) = find_device_path(&objects, mac) else {
        return false;
    };
    let obj_path: zbus::zvariant::ObjectPath<'_> = dev_path.as_ref();
    conn.call_method(
        Some("org.bluez"),
        adapter_path.as_str(),
        Some("org.bluez.Adapter1"),
        "RemoveDevice",
        &(obj_path,),
    )
    .is_ok()
}

fn run_bluetoothctl(args: &[&str]) -> bool {
    Command::new("bluetoothctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn set_bluez_powered_via_dbus(powered: bool) -> bool {
    let Some((conn, objects)) = bluez_managed_objects() else {
        return false;
    };
    let Some(adapter_path) = find_adapter_path(&objects) else {
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

fn query_bluetooth_snapshot_fallback() -> BluetoothSnapshot {
    let sysfs_present = fs::read_dir("/sys/class/bluetooth").is_ok_and(|mut d| d.next().is_some());
    let mut snap = BluetoothSnapshot {
        status: BluetoothStatus {
            available: sysfs_present,
            powered: false,
            connected_device: None,
            status_label: format_bluetooth_label(sysfs_present, false, None),
        },
        ..BluetoothSnapshot::default()
    };
    if matches!(bluez_dbus_has_owner(), Some(false)) {
        return snap;
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
                snap.status.available = true;
                snap.status.powered = rest.trim().eq_ignore_ascii_case("yes");
            } else if let Some(rest) = trimmed.strip_prefix("Discovering:") {
                snap.discovering = rest.trim().eq_ignore_ascii_case("yes");
            } else if let Some(rest) = trimmed.strip_prefix("Alias:") {
                let name = rest.trim();
                if !name.is_empty() {
                    snap.adapter_name = Some(name.to_owned());
                }
            }
        }
        if let Ok(devs_out) = Command::new("bluetoothctl")
            .args(["--timeout", "1", "devices"])
            .output()
            && devs_out.status.success()
        {
            snap.devices = parse_bluetoothctl_devices(&String::from_utf8_lossy(&devs_out.stdout));
        }
        snap.status.status_label =
            format_bluetooth_label(snap.status.available, snap.status.powered, None);
    }
    snap
}

#[cfg(test)]
mod tests {
    use super::{NetworkService, format_nmcli_action_error, nmcli_requires_secrets};

    #[test]
    fn detects_nmcli_missing_wifi_psk_messages() {
        assert!(nmcli_requires_secrets(
            "Warning: password for '802-11-wireless-security.psk' not given in 'passwd-file'"
        ));
        assert!(nmcli_requires_secrets(
            "Error: Connection activation failed: Secrets were required, but not provided"
        ));
        assert!(!nmcli_requires_secrets(
            "Error: No network with SSID 'Missing' found."
        ));
    }

    #[test]
    fn wifi_disconnect_and_forget_validate_empty_ssid_and_format_errors() {
        let svc = NetworkService::new();
        assert!(svc.disconnect_wifi("   ").is_err());
        assert!(svc.forget_wifi("").is_err());
        assert_eq!(
            format_nmcli_action_error("delete", "HomeWiFi", "", ""),
            "Failed to delete Wi-Fi 'HomeWiFi'"
        );
        assert_eq!(
            format_nmcli_action_error("down", "HomeWiFi", "Error: unknown connection", ""),
            "Error: unknown connection"
        );
    }
}
