//! Power supply and battery telemetry service (`/sys/class/power_supply`) + power profiles (`RUSTYOS-131`).

use std::fs;
use std::path::Path;
use std::process::Command;

use zbus::zvariant::Value;

/// System power profile (`power-profiles-daemon` / `UPower.PowerProfiles`) (`RUSTYOS-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerProfile {
    /// Reduced power consumption and performance headroom.
    LowPower,
    /// Standard balanced power and responsiveness.
    #[default]
    Balanced,
    /// Maximum sustained CPU/GPU thermal budget.
    Performance,
}

impl PowerProfile {
    /// Parses a `power-profiles-daemon` canonical name.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "power-saver" | "low-power" | "low_power" => Some(Self::LowPower),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            _ => None,
        }
    }

    /// Canonical D-Bus / `powerprofilesctl` profile identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LowPower => "power-saver",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }

    /// Human-readable label displayed in System Settings.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LowPower => "Low Power",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
        }
    }
}

/// Battery health classification derived from full vs design capacity (`RUSTYOS-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatteryCondition {
    #[default]
    Normal,
    ServiceRecommended,
    Unknown,
}

impl BatteryCondition {
    /// Human-readable battery condition string.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::ServiceRecommended => "Service Recommended",
            Self::Unknown => "Unknown",
        }
    }
}

/// Detailed power supply, battery health, and power-profile snapshot (`RUSTYOS-131`).
#[derive(Debug, Clone, PartialEq)]
pub struct PowerDetails {
    /// Base power & charge summary compatible with `TopBar` telemetry.
    pub status: PowerStatus,
    /// Active system power profile if `power-profiles-daemon` is available.
    pub active_profile: Option<PowerProfile>,
    /// Supported system power profiles exposed by firmware/daemon.
    pub available_profiles: Vec<PowerProfile>,
    /// Estimated maximum capacity relative to design capacity (`1..=100%`).
    pub health_percent: Option<u8>,
    /// Battery charge cycle count reported by kernel sysfs, if available.
    pub cycle_count: Option<u32>,
    /// Condition summary (`Normal`, `Service Recommended`, `Unknown`).
    pub condition: BatteryCondition,
    /// Estimated minutes to empty (discharging) or full (charging), if power rate is known.
    pub time_remaining_minutes: Option<u32>,
    /// Instantaneous power draw/charge rate in watts, if reported.
    pub power_watts: Option<f32>,
    /// Hardware battery model string from sysfs `model_name`.
    pub model_name: Option<String>,
    /// Battery chemistry string from sysfs `technology`.
    pub technology: Option<String>,
}

impl Default for PowerDetails {
    fn default() -> Self {
        Self {
            status: PowerStatus::default(),
            active_profile: None,
            available_profiles: Vec::new(),
            health_percent: None,
            cycle_count: None,
            condition: BatteryCondition::Unknown,
            time_remaining_minutes: None,
            power_watts: None,
            model_name: None,
            technology: None,
        }
    }
}

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

/// Power/battery service reading from Linux sysfs (`/sys/class/power_supply`).
#[derive(Debug, Default, Clone)]
pub struct PowerService {
    pub(crate) base_path: String,
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
            let supply_type =
                fs::read_to_string(path.join("type")).map_or_default(|s| s.trim().to_lowercase());

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

            // Check System Battery (BAT0, BAT1, etc.; ignore peripheral scope=Device batteries like hidpp_battery_0)
            if is_system_battery(&path, &name, &supply_type) {
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

    /// Queries detailed battery health, wattage, time remaining, and system power profiles (`RUSTYOS-131`).
    #[must_use]
    pub fn query_details(&self) -> PowerDetails {
        let mut details = self.query_sysfs_details();
        if let Some((active, avail)) = query_power_profiles() {
            details.active_profile = active;
            details.available_profiles = avail;
        }
        details
    }

    /// Sets the active `power-profiles-daemon` power profile if supported (`RUSTYOS-131`).
    #[must_use]
    pub fn set_power_profile(&self, profile: PowerProfile) -> bool {
        let _ = self;
        set_power_profile_impl(profile)
    }

    /// Reads extended battery health and instantaneous telemetry from `self.base_path`.
    #[must_use]
    pub fn query_sysfs_details(&self) -> PowerDetails {
        let status = self.query_status();
        let mut details = PowerDetails {
            status: status.clone(),
            condition: if status.has_battery {
                BatteryCondition::Normal
            } else {
                BatteryCondition::Unknown
            },
            ..PowerDetails::default()
        };
        let p = Path::new(&self.base_path);
        let Ok(entries) = fs::read_dir(p) else {
            return details;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let supply_type =
                fs::read_to_string(path.join("type")).map_or_default(|s| s.trim().to_lowercase());
            if !is_system_battery(&path, &name, &supply_type) {
                continue;
            }
            populate_battery_entry_details(&path, status.state, &mut details);
            break;
        }

        details
    }
}

fn populate_battery_entry_details(
    path: &Path,
    battery_state: BatteryState,
    details: &mut PowerDetails,
) {
    details.cycle_count = read_sysfs_u64(path, "cycle_count")
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&c| c > 0);
    details.model_name = read_sysfs_string(path, "model_name");
    details.technology = read_sysfs_string(path, "technology");

    let voltage_uv = read_sysfs_u64(path, "voltage_now");
    let (now_uwh, full_uwh, design_uwh) = if let (Some(full), Some(design)) = (
        read_sysfs_u64(path, "energy_full"),
        read_sysfs_u64(path, "energy_full_design"),
    ) {
        (read_sysfs_u64(path, "energy_now"), Some(full), Some(design))
    } else if let (Some(c_full), Some(c_design)) = (
        read_sysfs_u64(path, "charge_full"),
        read_sysfs_u64(path, "charge_full_design"),
    ) {
        let scale_uwh =
            |uah: u64| -> u64 { voltage_uv.map_or(uah, |uv| (uah.saturating_mul(uv)) / 1_000_000) };
        (
            read_sysfs_u64(path, "charge_now").map(scale_uwh),
            Some(scale_uwh(c_full)),
            Some(scale_uwh(c_design)),
        )
    } else {
        (None, None, None)
    };

    if let (Some(full), Some(design)) = (full_uwh, design_uwh)
        && design > 0
    {
        let pct = ((full.saturating_mul(100)) / design).clamp(1, 100);
        let pct_u8 = u8::try_from(pct).unwrap_or(100);
        details.health_percent = Some(pct_u8);
        details.condition = if pct_u8 < 80 {
            BatteryCondition::ServiceRecommended
        } else {
            BatteryCondition::Normal
        };
    }

    let power_uw = read_sysfs_u64(path, "power_now").or_else(|| {
        let cur_ua = read_sysfs_u64(path, "current_now")?;
        let v_uv = voltage_uv?;
        Some(cur_ua.saturating_mul(v_uv) / 1_000_000)
    });
    if let Some(uw) = power_uw
        && uw > 0
    {
        #[allow(clippy::cast_precision_loss)]
        {
            details.power_watts = Some(uw as f32 / 1_000_000.0);
        }
    }

    if let (Some(uw), Some(now), Some(full)) = (power_uw, now_uwh, full_uwh)
        && uw >= 200_000
    {
        let target_uwh = match battery_state {
            BatteryState::Discharging => Some(now),
            BatteryState::Charging => Some(full.saturating_sub(now)),
            _ => None,
        };
        if let Some(rem_uwh) = target_uwh {
            let mins = (rem_uwh.saturating_mul(60)) / uw;
            if let Ok(m) = u32::try_from(mins)
                && m > 0
            {
                details.time_remaining_minutes = Some(m);
            }
        }
    }
}

/// Parses `powerprofilesctl list` text output into active and available profiles (`RUSTYOS-131`).
#[must_use]
pub fn parse_powerprofilesctl_list(output: &str) -> (Option<PowerProfile>, Vec<PowerProfile>) {
    let mut active = None;
    let mut available = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        let is_active = trimmed.starts_with('*');
        let rest = trimmed.trim_start_matches('*').trim();
        if let Some((raw_name, _)) = rest.split_once(':')
            && let Some(profile) = PowerProfile::parse(raw_name)
        {
            if is_active {
                active = Some(profile);
            }
            if !available.contains(&profile) {
                available.push(profile);
            }
        }
    }
    (active, available)
}

fn is_system_battery(path: &Path, lower_name: &str, supply_type: &str) -> bool {
    if supply_type != "battery" && !lower_name.starts_with("bat") {
        return false;
    }
    if lower_name.starts_with("hidpp") || lower_name.starts_with("wacom") {
        return false;
    }
    !fs::read_to_string(path.join("scope")).is_ok_and(|s| s.trim().eq_ignore_ascii_case("device"))
}

fn read_sysfs_u64(dir: &Path, file: &str) -> Option<u64> {
    fs::read_to_string(dir.join(file))
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

fn read_sysfs_string(dir: &Path, file: &str) -> Option<String> {
    let s = fs::read_to_string(dir.join(file)).ok()?;
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

const POWER_PROFILE_SERVICES: [(&str, &str, &str); 2] = [
    (
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    ),
    (
        "net.hadess.PowerProfiles",
        "/net/hadess/PowerProfiles",
        "net.hadess.PowerProfiles",
    ),
];

fn query_power_profiles() -> Option<(Option<PowerProfile>, Vec<PowerProfile>)> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let dbus = zbus::blocking::fdo::DBusProxy::new(&conn).ok()?;

    let mut has_service = false;
    for (dest, path, iface) in POWER_PROFILE_SERVICES {
        let Ok(bus_name) = zbus::names::BusName::try_from(dest) else {
            continue;
        };
        if !dbus.name_has_owner(bus_name).unwrap_or(false) {
            continue;
        }
        has_service = true;
        let Ok(iface_name) = zbus::names::InterfaceName::try_from(iface) else {
            continue;
        };
        let Ok(props_builder) = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
            .destination(dest)
            .and_then(|b| b.path(path))
        else {
            continue;
        };
        let Ok(props) = props_builder.build() else {
            continue;
        };
        let Ok(all) = props.get_all(iface_name) else {
            continue;
        };
        let active = all.get("ActiveProfile").and_then(|v| match &**v {
            Value::Str(s) => PowerProfile::parse(s.as_str()),
            _ => None,
        });
        let mut avail = Vec::new();
        if let Some(arr_val) = all.get("Profiles")
            && let Value::Array(arr) = &**arr_val
        {
            for item in arr.iter() {
                if let Value::Dict(dict) = item {
                    for (k, v) in dict.iter() {
                        if let (Value::Str(ks), Value::Str(name)) = (k, v)
                            && ks.as_str() == "Profile"
                            && let Some(profile) = PowerProfile::parse(name.as_str())
                            && !avail.contains(&profile)
                        {
                            avail.push(profile);
                        }
                    }
                }
            }
        }
        return Some((active, avail));
    }

    if !has_service {
        return None;
    }

    let out = Command::new("powerprofilesctl").arg("list").output().ok()?;
    out.status
        .success()
        .then(|| parse_powerprofilesctl_list(&String::from_utf8_lossy(&out.stdout)))
}

fn set_power_profile_impl(profile: PowerProfile) -> bool {
    let Ok(conn) = zbus::blocking::Connection::system() else {
        return false;
    };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return false;
    };
    for (dest, path, iface) in POWER_PROFILE_SERVICES {
        let Ok(bus_name) = zbus::names::BusName::try_from(dest) else {
            continue;
        };
        if !dbus.name_has_owner(bus_name).unwrap_or(false) {
            continue;
        }
        let Ok(iface_name) = zbus::names::InterfaceName::try_from(iface) else {
            continue;
        };
        let Ok(props_builder) = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
            .destination(dest)
            .and_then(|b| b.path(path))
        else {
            continue;
        };
        if let Ok(props) = props_builder.build()
            && props
                .set(iface_name, "ActiveProfile", Value::from(profile.as_str()))
                .is_ok()
        {
            return true;
        }
    }

    Command::new("powerprofilesctl")
        .args(["set", profile.as_str()])
        .status()
        .is_ok_and(|s| s.success())
}
