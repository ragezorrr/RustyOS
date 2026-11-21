# ADR-0082: TopBar Adaptive Backdrop, Compact Popovers, Privacy Telemetry & Global AppMenu (`RUSTYOS-141`..`143`)

- **Status:** accepted
- **Date:** 2026-11-21
- **Tasks:** `RUSTYOS-141`, `RUSTYOS-142`, `RUSTYOS-143`

## Context

The RustyOS `TopBar` (`crates/rusty-compositor/src/shell_render/top_bar/`) renders inside the compositor (`ADR-0027`, `ADR-0072`), but previously:
1. Foreground text and glyph contrast depended solely on Light/Dark theme mode rather than the actual wallpaper pixels behind the bar.
2. Only Wi-Fi and Software Updates had standalone `TopBar` popovers; Volume, Battery, and Bluetooth lacked dedicated popovers and right-click visibility toggles persisted in `config.toml`.
3. There were no live PipeWire microphone/camera privacy indicators, active screen-recording stop chip, or localized (`en`/`ru`/`uk`) `12h`/`24h` clock formatting.
4. Global application menus on the left used only static `TopBarAppProfile` templates without a D-Bus `com.canonical.AppMenu.Registrar` backend.

## Decision

Implement all three stages cleanly inside `rusty-compositor` and `rusty-services` without adding new heavy dependencies or blocking the compositor event loop:

1. **Stage A (`RUSTYOS-141`):**
   - Add `shell_render/top_bar/backdrop.rs` (`BarBackdropStyle`, `analyze_top_bar_backdrop_bgra`, `relative_luminance`, `prefers_light_foreground`) invoked once on wallpaper decode in `surfaces/mod.rs` to compute light/dark foreground ink and optional scrim alpha on busy strips.
   - Add `[top_bar]` section in `config.rs` (`show_wifi`, `show_battery`, `show_sound`, `show_bluetooth`, `show_battery_percentage`, `adaptive_contrast`, `clock_24h`, `clock_locale`) and right-click visibility toggles on `TopBar` status icons.
   - Add `format_localized_short_datetime` / `local_clock_short_datetime_localized` (`en`/`ru`/`uk`, `12h`/`24h`) in `rusty-services`.
   - Add `shell_render/topbar_popovers.rs` (`TopBarQuickPopoverKind::{Volume, Battery, Bluetooth}`, `ShellOverlayView::TopBarQuickPopover`) with Liquid Glass rendering, hover pills, and Escape/outside-click dismissal.

2. **Stage B (`RUSTYOS-142`):**
   - Add `crates/rusty-services/src/privacy.rs` (`DeviceUsageSnapshot`, `parse_pipewire_dump_usage`, `DeviceUsageService::query_snapshot`) polled on the background `SystemMetricsHandle` worker thread.
   - Render orange (mic) and green (camera) privacy indicator dots and a clickable red `REC` stop pill when `active_screencasts` is non-empty.

3. **Stage C (`RUSTYOS-143`):**
   - Add `crates/rusty-compositor/src/app_menu.rs` (`AppMenuHandle`, `AppMenuTree`, `AppMenuSection`, `AppMenuItem`, `strip_menu_mnemonic`) running `com.canonical.AppMenu.Registrar` + `com.canonical.dbusmenu` calls on a dedicated `zbus::blocking` background thread, with non-blocking `try_lock` reads on the compositor thread and graceful fallback to `TopBarAppProfile`.

## Consequences

- Zero event-loop blocking: PipeWire privacy polling, clock formatting, and D-Bus `AppMenu` registration all run off the main compositor loop.
- Existing `TopBar` users keep backwards-compatible defaults while gaining adaptive wallpaper contrast, customizable status icons, compact popovers, privacy indicators, and D-Bus global menu registration.
