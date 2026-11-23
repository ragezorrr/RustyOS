# ADR-0089: System Date & Time Service (`org.freedesktop.timedate1`) & Settings UI (`RUSTYOS-160`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-103`
- **Task:** `RUSTYOS-160`

## Context
On a freshly installed RustyOS system, `timedatectl status` reported `Time zone: UTC` and `NTP service: inactive` (`systemd-timesyncd` was not enabled by `scripts/rustyos-install.sh`), while `apps/rusty-settings` (`SettingsPage::DateTime`) only exposed read-only `Current Time`, `Use 24-Hour Clock`, and `Clock Language`. Users had no graphical way to change their timezone, enable NTP synchronization, or set the date/time manually.

## Decision
1. **`TimeDateService` (`crates/rusty-services/src/timedate.rs`):**
   - Expose `TimeDateStatus` (`timezone`, `ntp_enabled`, `ntp_synchronized`, `can_ntp`) and `TimeDateService::{query_status, list_timezones, set_ntp, set_timezone, set_manual_datetime}`.
   - Query state via `org.freedesktop.timedate1` D-Bus `Properties.GetAll` with clean fallbacks to `timedatectl show` (`parse_timedatectl_show`) and `/etc/localtime`.
   - Mutate system clock and timezone via `org.freedesktop.timedate1.{SetNTP, SetTimezone, SetTime}` with `interactive = true` so the compositor's built-in graphical Polkit agent authorizes changes natively without custom privileged helpers or TTY `pkttyagent` issues.
   - Validate all inputs strictly (`is_valid_iana_timezone`, `/usr/share/zoneinfo` presence check, `parse_manual_datetime_input` including leap-year bounds, and `match_timezone_suggestions`).
2. **`SettingsPage::DateTime` (`apps/rusty-settings`):**
   - Expand `Date & Time` with two sections: **Date & Time** (`Current Time`, `Set Time and Date Automatically` NTP switch, `Synchronization` status/error readout, `Date & Time` -> `SettingsModal::ManualDateTime`, `Time Zone` -> `SettingsModal::TimezoneEdit` with live suggestion matching, `Quick Time Zone` preset cycler) and **Clock & Time Format** (`Use 24-Hour Clock`, `Clock Language`).
   - Dispatch `ServiceCommand::{SetNtpEnabled, SetTimezone, SetManualDateTime}` asynchronously on the worker thread, report `timedate_error` cleanly in UI, and trigger `notify_compositor_reload()` when timezone changes so the `TopBar` clock refreshes.
3. **Installer (`scripts/rustyos-install.sh`):**
   - Enable `systemd-timesyncd` alongside `NetworkManager`, `greetd`, and `bluetooth` for new installations.

## Consequences
- Users can configure NTP, IANA timezone, and manual date/time directly from `rusty-settings` with native Polkit authentication.
- New installations boot with `systemd-timesyncd` enabled by default.
- Zero new Cargo dependencies and zero custom privileged binaries.
