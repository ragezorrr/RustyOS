# ADR-0093: Reactive TopBar System D-Bus Telemetry and Live `.desktop` Application Catalog (`RUSTYOS-164`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-107`
- **Task:** `RUSTYOS-164`

## Context
Two real-world shell latency bugs degraded daily usability compared to Cutefish/macOS desktops:
1. **Delayed `TopBar` Battery & Wi-Fi state updates:** `crates/rusty-compositor/src/telemetry.rs` slept `12 s` (`POLL_INTERVAL_IDLE`) whenever no popover was open, polled all hardware/CLI services sequentially, and had no `revision` counter or `calloop::ping` wakeup callback. Unplugging AC power or switching Wi-Fi left stale charging/network icons in `TopBar` for up to 12+ seconds (and on idle DRM/KMS screens until an unrelated input or client commit triggered a redraw). Additionally, the compact Battery popover omitted time-to-empty / time-to-full estimates and no low-battery alerts were emitted.
2. **Stale Spotlight & Dock `.desktop` catalog:** `LauncherState::new()` (`crates/rusty-compositor/src/launcher.rs`) scanned only three hardcoded directories once at compositor startup, missing `$XDG_DATA_DIRS` and Flatpak `exports/share/applications` paths and never picking up newly installed or removed applications until a full session restart.

## Decision
1. **Reactive System D-Bus Signal Subscriptions & Targeted Fast Refresh (`crates/rusty-services/src/system_signals.rs`, `crates/rusty-compositor/src/telemetry.rs`):**
   - Add `SystemSignalMask` (`POWER`, `NETWORK`, `BLUETOOTH`) and `SystemSignalMonitorHandle` on a single lightweight background thread subscribing to `zbus::blocking::Connection::system()` signals (`org.freedesktop.DBus.Properties::PropertiesChanged` under `/org/freedesktop/UPower` and `/org/bluez`, and all signals under `/org/freedesktop/NetworkManager`), with graceful no-op fallback when the system bus is absent.
   - Extend `SystemMetricsHandle` with `revision: AtomicU64`, `pending_signals: AtomicU8`, `set_on_change(TelemetryChangeCallback)`, `notify_system_signal`, and `replace_snapshot`. When a system D-Bus signal arrives, the worker coalesces burst events over `80 ms` (`SIGNAL_COALESCE_DEBOUNCE`) and runs `poll_targeted` (refreshing only the signalled domain + local clock, skipping slow audio/media/user queries). Only when the new `SystemTelemetry` differs from the cached snapshot does it increment `revision` and invoke `calloop::ping`.
   - Keep `3 s` active / `12 s` idle polling solely as a safety net for wall-clock minutes and services without D-Bus push signals.
2. **Battery Time Remaining & One-Shot Low-Battery Notifications (`crates/rusty-services/src/power.rs`, `crates/rusty-compositor/src/shell_render/topbar_popovers.rs`, `state/mod.rs`):**
   - Populate `PowerStatus::time_remaining_minutes: Option<u32>` directly during sysfs `PowerService::query_status()` and export `format_battery_time_remaining(minutes, charging)` (`"2h 15m left"`, `"45m to full"`).
   - Display time remaining in `TopBarQuickPopoverKind::Battery` (`format_battery_popover_status`).
   - Evaluate low-battery alerts inside `RustyCompositorState::drain_telemetry_updates()` (`<= 20%` Normal `"Low Battery"`, `<= 10%` Critical `"Critically Low Battery"` while discharging; one-shot per tier with `>= 25%` hysteresis and instant reset on AC/Charging/Full).
3. **Live `inotify` `.desktop` Application Catalog Monitor (`crates/rusty-services/src/app_catalog_monitor.rs`, `crates/rusty-compositor/src/launcher.rs`):**
   - Export `discover_application_dirs()` / `discover_application_dirs_from()` covering `$XDG_DATA_HOME` / `~/.local/share`, user & system Flatpak `exports/share/applications`, `$XDG_DATA_DIRS`, `/usr/local/share`, and `/usr/share` in XDG priority order with deduplication.
   - Add `AppCatalogMonitorHandle` (isolated `#[allow(unsafe_code)] mod linux_watch` using workspace `libc` `inotify` + `poll` + `250 ms` debounce, watching existing parent dirs when an `applications/` subdir does not exist yet so newly created user/Flatpak dirs are picked up live).
   - Add non-blocking background rescanning to `LauncherState` (`request_async_reload`, `request_async_reload_dirs`, `take_reloaded_catalog`, `replace_catalog`, `set_reload_waker`) and wire `drain_app_catalog_updates()` + unified `attach_event_loop_waker(ping)` in `drm_backend.rs`, `winit_backend.rs`, and `frame_composer.rs` so Spotlight and `refresh_dock_pin_metadata_cache()` update live without blocking the event loop. When `inotify` is unavailable, opening Spotlight triggers an async fallback rescan.

## Consequences
- Plugging/unplugging AC power, connecting/disconnecting Wi-Fi, or toggling Bluetooth updates `TopBar` within ~100–250 ms on both Winit and idle DRM/KMS outputs without extra idle wakeups when nothing changes.
- Installing or uninstalling native packages or Flatpak apps updates Spotlight search results and Dock pin metadata within ~250–300 ms without restarting `rusty-compositor`.
- Zero new external Cargo dependencies and zero `unsafe` code added to `crates/rusty-compositor`.
