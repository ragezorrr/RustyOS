//! Linux system service integrations for RustyOS (`RUSTYOS-011`, ADR-0006).
//!
//! Provides zero-panic, least-privilege adapters for power/battery, network,
//! audio, and session management with graceful fallback if services are absent.

use std::fs;
use std::path::Path;
use std::process::Command;

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

    /// Queries the active network status via `/sys/class/net` and `nmcli`.
    #[must_use]
    pub fn query_status(&self) -> NetworkStatus {
        let mut status = NetworkStatus::default();

        // 1. Try reading from /sys/class/net to find operational state
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

        // 2. Try obtaining Wi-Fi SSID via nmcli if wireless
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
}
