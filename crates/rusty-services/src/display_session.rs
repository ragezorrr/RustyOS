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

/// Formats a compact localized `TopBar` date + time label (`RUSTYOS-141`).
///
/// Supports `"en"`, `"ru"`, and `"uk"` (case-insensitive prefix match, falling back to `"en"`).
#[must_use]
pub fn format_localized_short_datetime(
    wday_sun0: u8,
    month_1_12: u8,
    day: u8,
    hour24: u8,
    minute: u8,
    locale: &str,
    use_24h: bool,
) -> String {
    const EN_DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const EN_MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const RU_DAYS: [&str; 7] = ["Вс", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб"];
    const RU_MONTHS: [&str; 12] = [
        "янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек",
    ];
    const UK_DAYS: [&str; 7] = ["Нд", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб"];
    const UK_MONTHS: [&str; 12] = [
        "січ", "лют", "бер", "кві", "тра", "чер", "лип", "сер", "вер", "жов", "лис", "гру",
    ];

    let lang = locale.trim().to_ascii_lowercase();
    let (days, months) = if lang.starts_with("ru") {
        (&RU_DAYS, &RU_MONTHS)
    } else if lang.starts_with("uk") || lang.starts_with("ua") {
        (&UK_DAYS, &UK_MONTHS)
    } else {
        (&EN_DAYS, &EN_MONTHS)
    };
    let wd = days[usize::from(wday_sun0 % 7)];
    let mo = months[usize::from(month_1_12.clamp(1, 12) - 1)];
    if use_24h {
        format!("{wd} {mo} {day}  {hour24:02}:{minute:02}")
    } else {
        let suffix = if hour24 >= 12 { "PM" } else { "AM" };
        let h12 = match hour24 % 12 {
            0 => 12,
            h => h,
        };
        format!("{wd} {mo} {day}  {h12}:{minute:02} {suffix}")
    }
}

/// Returns compact localized `TopBar` date + time for `locale` (`"en"`, `"ru"`, `"uk"`) (`RUSTYOS-141`).
#[must_use]
pub fn local_clock_short_datetime_localized(locale: &str, use_24h: bool) -> String {
    if let Some(raw) = run_c_locale_date("+%w %m %-d %H %M", 24) {
        let mut parts = raw.split_whitespace();
        if let (Some(w), Some(m), Some(d), Some(h), Some(min)) = (
            parts.next().and_then(|s| s.parse::<u8>().ok()),
            parts.next().and_then(|s| s.parse::<u8>().ok()),
            parts.next().and_then(|s| s.parse::<u8>().ok()),
            parts.next().and_then(|s| s.parse::<u8>().ok()),
            parts.next().and_then(|s| s.parse::<u8>().ok()),
        ) {
            return format_localized_short_datetime(w, m, d, h, min, locale, use_24h);
        }
    }
    local_clock_short_datetime_fmt(use_24h)
}

/// Returns compact macOS-style `TopBar` date + time (`"Wed Apr 1  9:41 AM"`), falling back to `HH:MM` (`RUSTYOS-048`/`049`).
#[must_use]
pub fn local_clock_short_datetime() -> String {
    local_clock_short_datetime_fmt(false)
}

/// Human user account summary displayed on the Lock Screen and Users Settings (`RUSTYOS-054`, `RUSTYOS-131`).
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

/// Read-only detailed account profile for the active desktop user (`RUSTYOS-131`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUserDetails {
    /// Login username (`$USER` or `/etc/passwd` match).
    pub username: String,
    /// Friendly display name.
    pub display_name: String,
    /// Primary UNIX user ID.
    pub uid: u32,
    /// Primary UNIX group ID.
    pub gid: u32,
    /// Configured home directory path.
    pub home_dir: String,
    /// Login shell path.
    pub shell: String,
    /// True if member of `wheel`, `sudo`, `adm`, or `uid == 0`.
    pub is_admin: bool,
    /// Supplementary + primary group names resolved from `/etc/group`.
    pub groups: Vec<String>,
    /// Optional resolved avatar file path.
    pub avatar_path: Option<String>,
    /// Session runtime kind (`Wayland`, `X11`, `TTY`, etc.).
    pub session_type: String,
}

/// Read-only user account discovery service reading `/etc/passwd`, `/etc/group`, and avatar hints (`RUSTYOS-054`, `RUSTYOS-131`).
#[derive(Debug, Default, Clone)]
pub struct UserService {
    passwd_path: String,
    group_path: String,
    accounts_icons_dir: String,
}

impl UserService {
    /// Creates a new user discovery service pointing at standard Linux account databases.
    #[must_use]
    pub fn new() -> Self {
        Self {
            passwd_path: "/etc/passwd".to_owned(),
            group_path: "/etc/group".to_owned(),
            accounts_icons_dir: "/var/lib/AccountsService/icons".to_owned(),
        }
    }

    /// Queries detailed read-only metadata for the currently active user (`RUSTYOS-131`).
    #[must_use]
    pub fn query_current_user_details(&self) -> CurrentUserDetails {
        let username = std::env::var("USER")
            .ok()
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "user".to_owned());
        let passwd_raw = fs::read_to_string(&self.passwd_path).unwrap_or_default();
        let group_raw = fs::read_to_string(&self.group_path).unwrap_or_default();
        let home_env = std::env::var("HOME").ok();
        let session_raw =
            std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "wayland".to_owned());
        Self::parse_current_user_details(
            &username,
            &passwd_raw,
            &group_raw,
            home_env.as_deref(),
            &self.accounts_icons_dir,
            &session_raw,
        )
    }

    /// Pure parser constructing [`CurrentUserDetails`] from `/etc/passwd` + `/etc/group` text (`RUSTYOS-131`).
    #[must_use]
    pub fn parse_current_user_details(
        username: &str,
        passwd_contents: &str,
        group_contents: &str,
        home_env: Option<&str>,
        accounts_icons_dir: &str,
        session_type_raw: &str,
    ) -> CurrentUserDetails {
        let mut display_name = username.to_owned();
        let mut uid = 1000_u32;
        let mut gid = 1000_u32;
        let mut home_dir = home_env.unwrap_or("/home/user").to_owned();
        let mut shell = String::from("/bin/bash");

        for line in passwd_contents.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = trimmed.split(':').collect();
            if parts.len() >= 7 && parts[0].trim() == username {
                let gecos = parts[4].split(',').next().unwrap_or_default().trim();
                if !gecos.is_empty() {
                    gecos.clone_into(&mut display_name);
                }
                uid = parts[2].trim().parse::<u32>().unwrap_or(1000);
                gid = parts[3].trim().parse::<u32>().unwrap_or(1000);
                let raw_home = parts[5].trim();
                if !raw_home.is_empty() {
                    raw_home.clone_into(&mut home_dir);
                }
                let raw_shell = parts[6].trim();
                if !raw_shell.is_empty() {
                    raw_shell.clone_into(&mut shell);
                }
                break;
            }
        }

        let groups = parse_etc_group_for_user(group_contents, username, gid);
        let is_admin = uid == 0
            || groups
                .iter()
                .any(|g| matches!(g.as_str(), "wheel" | "sudo" | "adm"));
        let avatar_path = resolve_user_avatar_path(
            username,
            Some(username),
            Some(home_dir.as_str()),
            accounts_icons_dir,
        );
        let session_type = match session_type_raw.trim().to_ascii_lowercase().as_str() {
            "wayland" => String::from("Wayland"),
            "x11" => String::from("X11"),
            "tty" => String::from("TTY"),
            other if !other.is_empty() => other.to_owned(),
            _ => String::from("Wayland"),
        };

        CurrentUserDetails {
            username: username.to_owned(),
            display_name,
            uid,
            gid,
            home_dir,
            shell,
            is_admin,
            groups,
            avatar_path,
            session_type,
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
            .map_or_default(|raw| parse_passwd_users(&raw, current_user.as_deref()));

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

/// Parses `/etc/group` entries returning all groups matching `username` or `primary_gid` (`RUSTYOS-131`).
#[must_use]
pub fn parse_etc_group_for_user(
    group_contents: &str,
    username: &str,
    primary_gid: u32,
) -> Vec<String> {
    let mut groups = Vec::new();
    for line in group_contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split(':').collect();
        if parts.len() < 4 {
            continue;
        }
        let gname = parts[0].trim();
        if gname.is_empty() {
            continue;
        }
        let gid_match = parts[2].trim().parse::<u32>().ok() == Some(primary_gid);
        let member_match = parts[3]
            .split(',')
            .any(|m| !m.trim().is_empty() && m.trim() == username);
        if (gid_match || member_match) && !groups.iter().any(|g| g == gname) {
            groups.push(gname.to_owned());
        }
    }
    groups
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
