//! System date, time, NTP, and IANA timezone management via `org.freedesktop.timedate1`
//! (`RUSTYOS-160`, ADR-0089).
//!
//! Uses standard `systemd-timedated` D-Bus APIs with `interactive = true` so the compositor's
//! Polkit agent handles authorization cleanly without custom setuid helpers or shell escalation.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use zbus::zvariant::OwnedValue;

const TIMEDATE_DEST: &str = "org.freedesktop.timedate1";
const TIMEDATE_PATH: &str = "/org/freedesktop/timedate1";
const TIMEDATE_IFACE: &str = "org.freedesktop.timedate1";

/// Curated list of common IANA timezones used for quick selection and offline fallbacks.
pub const COMMON_TIMEZONES: &[&str] = &[
    "UTC",
    "Europe/Kyiv",
    "Europe/Warsaw",
    "Europe/Berlin",
    "Europe/London",
    "Europe/Paris",
    "America/New_York",
    "America/Chicago",
    "America/Los_Angeles",
    "Asia/Dubai",
    "Asia/Tokyo",
    "Asia/Seoul",
    "Australia/Sydney",
];

/// Current system date/time synchronization and timezone state (`RUSTYOS-160`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeDateStatus {
    /// Active IANA timezone identifier (e.g. `"Europe/Kyiv"`, `"UTC"`).
    pub timezone: String,
    /// Whether automatic NTP time synchronization is enabled (`systemd-timesyncd` / `chronyd`).
    pub ntp_enabled: bool,
    /// Whether the system clock is currently synchronized via NTP.
    pub ntp_synchronized: bool,
    /// Whether an NTP client unit is installed and available on this host.
    pub can_ntp: bool,
}

impl Default for TimeDateStatus {
    fn default() -> Self {
        Self {
            timezone: String::from("UTC"),
            ntp_enabled: false,
            ntp_synchronized: false,
            can_ntp: true,
        }
    }
}

/// System date/time and timezone adapter backed by `org.freedesktop.timedate1` (`RUSTYOS-160`).
#[derive(Debug, Default, Clone, Copy)]
pub struct TimeDateService;

impl TimeDateService {
    /// Creates a new [`TimeDateService`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Queries the active timezone and NTP synchronization state via `systemd-timedated` D-Bus,
    /// falling back to `timedatectl show` and `/etc/localtime`.
    #[must_use]
    pub fn query_status(&self) -> TimeDateStatus {
        if let Some(status) = query_timedate_via_dbus() {
            return status;
        }
        if let Ok(out) = Command::new("timedatectl")
            .arg("show")
            .stdin(Stdio::null())
            .output()
            && out.status.success()
        {
            return parse_timedatectl_show(&String::from_utf8_lossy(&out.stdout));
        }
        TimeDateStatus {
            timezone: read_etc_localtime_timezone().unwrap_or_else(|| String::from("UTC")),
            ..TimeDateStatus::default()
        }
    }

    /// Lists available IANA timezones via `timedatectl list-timezones` with a curated fallback.
    #[must_use]
    pub fn list_timezones(&self) -> Vec<String> {
        if let Ok(out) = Command::new("timedatectl")
            .arg("list-timezones")
            .stdin(Stdio::null())
            .output()
            && out.status.success()
        {
            let zones: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim)
                .filter(|z| is_valid_iana_timezone(z))
                .map(ToOwned::to_owned)
                .collect();
            if !zones.is_empty() {
                return zones;
            }
        }
        COMMON_TIMEZONES.iter().map(|&z| String::from(z)).collect()
    }

    /// Enables or disables automatic NTP synchronization via `org.freedesktop.timedate1.SetNTP`
    /// with interactive Polkit authorization.
    ///
    /// # Errors
    ///
    /// Returns a human-readable diagnostic when D-Bus or Polkit authorization fails.
    pub fn set_ntp(&self, enabled: bool) -> Result<(), String> {
        let conn = open_system_dbus()?;
        conn.call_method(
            Some(TIMEDATE_DEST),
            TIMEDATE_PATH,
            Some(TIMEDATE_IFACE),
            "SetNTP",
            &(enabled, true),
        )
        .map(|_| ())
        .map_err(|err| format_dbus_timedate_error("SetNTP", &err.to_string()))
    }

    /// Sets the system IANA timezone via `org.freedesktop.timedate1.SetTimezone`
    /// with interactive Polkit authorization.
    ///
    /// # Errors
    ///
    /// Returns an error if `timezone` is not a valid installed IANA zone or if D-Bus/Polkit fails.
    pub fn set_timezone(&self, timezone: &str) -> Result<(), String> {
        let trimmed = timezone.trim();
        if !is_valid_iana_timezone(trimmed) {
            return Err(format!(
                "Invalid timezone '{trimmed}' (example: Europe/Kyiv)"
            ));
        }
        let zoneinfo_root = Path::new("/usr/share/zoneinfo");
        if zoneinfo_root.is_dir() && !zoneinfo_root.join(trimmed).is_file() {
            return Err(format!("Unknown timezone '{trimmed}'"));
        }
        let conn = open_system_dbus()?;
        conn.call_method(
            Some(TIMEDATE_DEST),
            TIMEDATE_PATH,
            Some(TIMEDATE_IFACE),
            "SetTimezone",
            &(trimmed, true),
        )
        .map(|_| ())
        .map_err(|err| format_dbus_timedate_error("SetTimezone", &err.to_string()))
    }

    /// Sets the system clock manually from `"YYYY-MM-DD HH:MM"` or `"YYYY-MM-DD HH:MM:SS"`
    /// in the active local timezone via `org.freedesktop.timedate1.SetTime` (requires NTP off).
    ///
    /// # Errors
    ///
    /// Returns a diagnostic if the format is invalid, conversion fails, NTP is active, or Polkit denies.
    pub fn set_manual_datetime(&self, input: &str) -> Result<(), String> {
        let canonical = parse_manual_datetime_input(input)?;
        let out = Command::new("date")
            .args(["-d", &canonical, "+%s"])
            .stdin(Stdio::null())
            .output()
            .map_err(|err| format!("Failed to convert local date/time: {err}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            return Err(if err.is_empty() {
                format!("Invalid date/time '{canonical}'")
            } else {
                err
            });
        }
        let secs = String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("Invalid timestamp produced for '{canonical}'"))?;
        let usec = secs
            .checked_mul(1_000_000)
            .ok_or_else(|| String::from("Date/time is out of supported range"))?;
        let conn = open_system_dbus()?;
        conn.call_method(
            Some(TIMEDATE_DEST),
            TIMEDATE_PATH,
            Some(TIMEDATE_IFACE),
            "SetTime",
            &(usec, false, true),
        )
        .map(|_| ())
        .map_err(|err| format_dbus_timedate_error("SetTime", &err.to_string()))
    }
}

fn open_system_dbus() -> Result<zbus::blocking::Connection, String> {
    zbus::blocking::Connection::system()
        .map_err(|err| format!("Unable to connect to system D-Bus: {err}"))
}

fn format_dbus_timedate_error(method: &str, raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("interactive authentication required")
        || lower.contains("not authorized")
        || lower.contains("accessdenied")
    {
        return String::from("Authentication cancelled or not authorized");
    }
    if lower.contains("automatic time synchronization is enabled") {
        return String::from("Turn off Automatic Date & Time (NTP) before setting time manually");
    }
    if raw.trim().is_empty() {
        format!("timedated {method} failed")
    } else {
        raw.trim().to_owned()
    }
}

fn query_timedate_via_dbus() -> Option<TimeDateStatus> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let reply = conn
        .call_method(
            Some(TIMEDATE_DEST),
            TIMEDATE_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(TIMEDATE_IFACE,),
        )
        .ok()?;
    let props: HashMap<String, OwnedValue> = reply.body().deserialize().ok()?;
    let timezone = props
        .get("Timezone")
        .and_then(|v| String::try_from(v.clone()).ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| String::from("UTC"));
    let ntp_enabled = props
        .get("NTP")
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(false);
    let ntp_synchronized = props
        .get("NTPSynchronized")
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(false);
    let can_ntp = props
        .get("CanNTP")
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(true);
    Some(TimeDateStatus {
        timezone,
        ntp_enabled,
        ntp_synchronized,
        can_ntp,
    })
}

fn read_etc_localtime_timezone() -> Option<String> {
    let target = fs::read_link("/etc/localtime").ok()?;
    let s = target.to_string_lossy();
    let (_, tz) = s.split_once("zoneinfo/")?;
    let trimmed = tz.trim();
    is_valid_iana_timezone(trimmed).then(|| trimmed.to_owned())
}

/// Parses `timedatectl show` key-value lines into a [`TimeDateStatus`] (`RUSTYOS-160`).
#[must_use]
pub fn parse_timedatectl_show(output: &str) -> TimeDateStatus {
    let mut status = TimeDateStatus::default();
    for line in output.lines() {
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let v = val.trim();
        match key.trim() {
            "Timezone" if is_valid_iana_timezone(v) => v.clone_into(&mut status.timezone),
            "NTP" => status.ntp_enabled = v.eq_ignore_ascii_case("yes"),
            "NTPSynchronized" => status.ntp_synchronized = v.eq_ignore_ascii_case("yes"),
            "CanNTP" => status.can_ntp = v.eq_ignore_ascii_case("yes"),
            _ => {}
        }
    }
    status
}

/// Validates an IANA timezone identifier against traversal and control characters (`RUSTYOS-160`).
#[must_use]
pub fn is_valid_iana_timezone(tz: &str) -> bool {
    let trimmed = tz.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || trimmed.starts_with('/')
        || trimmed.ends_with('/')
        || trimmed.contains("..")
        || trimmed.contains("//")
    {
        return false;
    }
    trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+'))
}

/// Validates and normalizes manual local date/time input (`"YYYY-MM-DD HH:MM"` or `"YYYY-MM-DD HH:MM:SS"`).
///
/// # Errors
///
/// Returns a concise user-facing validation error if the date or time is malformed or out of range.
pub fn parse_manual_datetime_input(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let Some((date_part, time_part)) = trimmed.split_once(' ') else {
        return Err(String::from(
            "Use format YYYY-MM-DD HH:MM (e.g. 2026-11-23 15:30)",
        ));
    };
    let date_bits: Vec<&str> = date_part.trim().split('-').collect();
    if date_bits.len() != 3 {
        return Err(String::from("Invalid date — expected YYYY-MM-DD"));
    }
    let year = date_bits[0]
        .parse::<u32>()
        .map_err(|_| String::from("Invalid year"))?;
    let month = date_bits[1]
        .parse::<u32>()
        .map_err(|_| String::from("Invalid month (01..12)"))?;
    let day = date_bits[2]
        .parse::<u32>()
        .map_err(|_| String::from("Invalid day"))?;
    if !(1970..=2100).contains(&year) || !(1..=12).contains(&month) {
        return Err(String::from("Year must be 1970..2100 and month 01..12"));
    }
    let max_day = days_in_month(year, month);
    if day == 0 || day > max_day {
        return Err(format!(
            "Day must be 01..{max_day:02} for {year:04}-{month:02}"
        ));
    }

    let time_bits: Vec<&str> = time_part.trim().split(':').collect();
    if time_bits.len() != 2 && time_bits.len() != 3 {
        return Err(String::from("Invalid time — expected HH:MM or HH:MM:SS"));
    }
    let hour = time_bits[0]
        .parse::<u32>()
        .map_err(|_| String::from("Invalid hour (00..23)"))?;
    let minute = time_bits[1]
        .parse::<u32>()
        .map_err(|_| String::from("Invalid minute (00..59)"))?;
    let second = if time_bits.len() == 3 {
        time_bits[2]
            .parse::<u32>()
            .map_err(|_| String::from("Invalid second (00..59)"))?
    } else {
        0
    };
    if hour > 23 || minute > 59 || second > 59 {
        return Err(String::from("Time out of range (00:00..23:59)"));
    }
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
    ))
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        2 => {
            let leap =
                (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
            if leap { 29 } else { 28 }
        }
        _ => 30,
    }
}

/// Returns up to `limit` IANA timezone suggestions matching `query` case-insensitively (`RUSTYOS-160`).
#[must_use]
pub fn match_timezone_suggestions(query: &str, all: &[String], limit: usize) -> Vec<String> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return COMMON_TIMEZONES
            .iter()
            .take(limit)
            .map(|&z| String::from(z))
            .collect();
    }
    let mut exact_prefix = Vec::new();
    let mut contains = Vec::new();
    for tz in all {
        let lower = tz.to_ascii_lowercase();
        if lower == needle || lower.starts_with(&needle) {
            exact_prefix.push(tz.clone());
        } else if lower.contains(&needle) {
            contains.push(tz.clone());
        }
    }
    exact_prefix.extend(contains);
    exact_prefix.truncate(limit);
    exact_prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timedatectl_show_fields_and_validates_iana_timezones() {
        let raw = "\
Timezone=Europe/Kyiv
LocalRTC=no
CanNTP=yes
NTP=yes
NTPSynchronized=yes
";
        let st = parse_timedatectl_show(raw);
        assert_eq!(st.timezone, "Europe/Kyiv");
        assert!(st.can_ntp);
        assert!(st.ntp_enabled);
        assert!(st.ntp_synchronized);

        assert!(is_valid_iana_timezone("UTC"));
        assert!(is_valid_iana_timezone("Europe/Kyiv"));
        assert!(is_valid_iana_timezone("Etc/GMT+2"));
        assert!(!is_valid_iana_timezone(""));
        assert!(!is_valid_iana_timezone("../etc/passwd"));
        assert!(!is_valid_iana_timezone("/UTC"));
        assert!(!is_valid_iana_timezone("Europe//Kyiv"));
    }

    #[test]
    fn validates_manual_datetime_input_including_leap_years() {
        assert_eq!(
            parse_manual_datetime_input("2026-11-23 15:45"),
            Ok(String::from("2026-11-23 15:45:00"))
        );
        assert_eq!(
            parse_manual_datetime_input("2024-02-29 08:05:09"),
            Ok(String::from("2024-02-29 08:05:09"))
        );
        assert!(parse_manual_datetime_input("2025-02-29 08:05").is_err());
        assert!(parse_manual_datetime_input("2026-13-01 10:00").is_err());
        assert!(parse_manual_datetime_input("2026-11-23 25:00").is_err());
        assert!(parse_manual_datetime_input("not-a-date").is_err());
    }

    #[test]
    fn formats_dbus_errors_and_matches_timezone_suggestions() {
        assert!(
            format_dbus_timedate_error("SetTimezone", "Interactive authentication required.")
                .contains("Authentication")
        );
        assert!(
            format_dbus_timedate_error("SetTime", "Automatic time synchronization is enabled")
                .contains("NTP")
        );
        let all = vec![
            String::from("UTC"),
            String::from("Europe/Kyiv"),
            String::from("Europe/Berlin"),
            String::from("America/New_York"),
        ];
        assert_eq!(
            match_timezone_suggestions("kyiv", &all, 3),
            vec![String::from("Europe/Kyiv")]
        );
    }
}
