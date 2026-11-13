//! Display backlight, wall-clock formatting, user discovery, and session power controls.

use std::fs;
use std::path::Path;
use std::process::Command;

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

/// Display backlight service using `/sys/class/backlight`, `systemd-logind` D-Bus, and `brightnessctl` (`RUSTYOS-031`).
#[derive(Debug, Clone)]
pub struct BrightnessService {
    pub(crate) base_path: String,
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

fn run_c_locale_date(fmt_arg: &str, max_len: usize) -> Option<String> {
    let output = Command::new("date")
        .env("LC_ALL", "C")
        .arg(fmt_arg)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty() && text.len() <= max_len).then_some(text)
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
    run_c_locale_date(fmt_arg, 28).unwrap_or_else(local_clock_hhmm)
}

/// Returns compact macOS Lock Screen date label in C locale (e.g. `"Thu Sep 10"`) (`RUSTYOS-054`).
#[must_use]
pub fn local_clock_lock_date() -> String {
    run_c_locale_date("+%a %b %-d", 24).unwrap_or_else(|| "Today".to_owned())
}

/// Returns compact macOS Lock Screen clock without AM/PM suffix (e.g. `"8:52"` in 12h or `"08:52"` in 24h) (`RUSTYOS-054`).
#[must_use]
pub fn local_clock_lock_hhmm(use_24h: bool) -> String {
    let fmt_arg = if use_24h { "+%H:%M" } else { "+%-I:%M" };
    run_c_locale_date(fmt_arg, 8).unwrap_or_else(local_clock_hhmm)
}

/// Returns compact macOS-style `TopBar` date + time (`"Wed Apr 1  9:41 AM"`), falling back to `HH:MM` (`RUSTYOS-048`/`049`).
#[must_use]
pub fn local_clock_short_datetime() -> String {
    local_clock_short_datetime_fmt(false)
}

/// Human user account summary displayed on the Lock Screen (`RUSTYOS-054`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAccount {
    /// UNIX login username.
    pub username: String,
    /// Friendly GECOS display name (falls back to `username`).
    pub display_name: String,
    /// UNIX user ID.
    pub uid: u32,
    /// Optional resolved avatar PNG/image file path (`~/.face`, `AccountsService`, etc.).
    pub avatar_path: Option<String>,
}

/// Read-only user account discovery service reading `/etc/passwd` and avatar hints (`RUSTYOS-054`).
#[derive(Debug, Default, Clone)]
pub struct UserService {
    passwd_path: String,
    accounts_icons_dir: String,
}

impl UserService {
    /// Creates a new user discovery service pointing at standard Linux account databases.
    #[must_use]
    pub fn new() -> Self {
        Self {
            passwd_path: "/etc/passwd".to_owned(),
            accounts_icons_dir: "/var/lib/AccountsService/icons".to_owned(),
        }
    }

    /// Queries non-system human users (`UID 1000..=60000`) with the current `$USER` prioritized first.
    #[must_use]
    pub fn query_users(&self) -> Vec<UserAccount> {
        let current_user = std::env::var("USER")
            .ok()
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty());
        let mut users = fs::read_to_string(&self.passwd_path)
            .map(|raw| parse_passwd_users(&raw, current_user.as_deref()))
            .unwrap_or_default();

        if users.is_empty() {
            let fallback_name = current_user.unwrap_or_else(|| "User".to_owned());
            users.push(UserAccount {
                username: fallback_name.clone(),
                display_name: fallback_name,
                uid: 1000,
                avatar_path: None,
            });
        }

        let home_dir = std::env::var("HOME").ok();
        let active_env_user = std::env::var("USER").ok();
        for account in &mut users {
            if account.avatar_path.is_none() {
                account.avatar_path = resolve_user_avatar_path(
                    &account.username,
                    active_env_user.as_deref(),
                    home_dir.as_deref(),
                    &self.accounts_icons_dir,
                );
            }
        }
        users
    }
}

/// Parses `/etc/passwd` entries for interactive human users (`UID 1000..=60000`) (`RUSTYOS-054`).
#[must_use]
pub fn parse_passwd_users(passwd_contents: &str, current_user: Option<&str>) -> Vec<UserAccount> {
    let mut users = Vec::new();
    for line in passwd_contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split(':').collect();
        if parts.len() < 7 {
            continue;
        }
        let username = parts[0].trim();
        let Ok(uid) = parts[2].trim().parse::<u32>() else {
            continue;
        };
        let shell = parts[6].trim();
        if username.is_empty()
            || !(1000..=60000).contains(&uid)
            || shell.ends_with("/nologin")
            || shell.ends_with("/false")
        {
            continue;
        }
        let gecos_name = parts[4].split(',').next().unwrap_or_default().trim();
        let display_name = if gecos_name.is_empty() {
            username.to_owned()
        } else {
            gecos_name.to_owned()
        };
        users.push(UserAccount {
            username: username.to_owned(),
            display_name,
            uid,
            avatar_path: None,
        });
    }

    if let Some(cur) = current_user.map(str::trim).filter(|s| !s.is_empty())
        && let Some(pos) = users.iter().position(|u| u.username == cur)
        && pos > 0
    {
        let active = users.remove(pos);
        users.insert(0, active);
    }
    users.truncate(4);
    users
}

fn resolve_user_avatar_path(
    username: &str,
    active_env_user: Option<&str>,
    home_dir: Option<&str>,
    accounts_icons_dir: &str,
) -> Option<String> {
    let mut candidates = Vec::new();
    if active_env_user == Some(username)
        && let Some(home) = home_dir
    {
        candidates.push(Path::new(home).join(".face"));
        candidates.push(Path::new(home).join(".face.icon"));
    }
    candidates.push(Path::new(accounts_icons_dir).join(username));
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Power/session lifecycle commands triggered from desktop/Lock Screen power controls (`RUSTYOS-054`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    /// Suspend system to RAM via `systemd-logind`.
    Suspend,
    /// Reboot system via `systemd-logind`.
    Reboot,
    /// Shut down system via `systemd-logind`.
    PowerOff,
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

    /// Executes a [`SessionAction`] via `systemd-logind` (`systemctl`).
    #[must_use]
    pub fn dispatch(&self, action: SessionAction) -> bool {
        match action {
            SessionAction::Suspend => self.request_suspend(),
            SessionAction::Reboot => self.request_reboot(),
            SessionAction::PowerOff => self.request_poweroff(),
        }
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
