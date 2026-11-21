# ADR-0085: Installed Audio Pipeline, Output Device Switching & Control/Notification Center Upgrade (`RUSTYOS-156`)

- **Status:** Accepted
- **Date:** 2026-11-21
- **Decision ID:** `D-099`

## Context
Freshly installed RustyOS systems lacked `pipewire-alsa`, `pipewire-pulse`, `alsa-utils`, `alsa-ucm-conf`, and `sof-firmware` in `pacstrap` / `PKGBUILD` `depends`, and did not auto-start `pipewire.service`, `pipewire-pulse.service`, and `wireplumber.service` or unmute initial ALSA mixer channels. In addition, `AudioService` could not enumerate or switch output sinks (e.g., Speakers vs. HDMI vs. Headphones), and `Notification Center` did not persist `Reminders` checklist state or widget visibility across restarts.

## Decision
1. **Installed System & Live ISO Audio Stack:**
   - Added `pipewire-alsa`, `pipewire-pulse`, `wireplumber`, `alsa-utils`, `alsa-ucm-conf`, and `sof-firmware` to `packaging/arch/PKGBUILD` `depends`, `packaging/archiso/packages.x86_64`, `scripts/rustyos-install.sh`, and `.github/workflows/ci.yml`.
   - Added `Wants=pipewire.service pipewire-pulse.service wireplumber.service` to `packaging/systemd/user/rustyos-session.target`, explicit service startup in `bin/rustyos-session`, and `AudioService::ensure_session_audio_ready()` (`amixer -q sset Master/Speaker/Headphone/PCM unmute`) on telemetry worker startup.
2. **Audio Output Device Enumeration & Switching (`AudioService`, `TopBar` Volume Popover, `Control Center`):**
   - Added `AudioOutputDevice`, `parse_wpctl_status_sinks`, `parse_pactl_short_sinks`, `AudioService::list_outputs()`, `AudioService::set_default_output()`, and `AudioService::cycle_next_output()` in `crates/rusty-services/src/media.rs`.
   - Displayed discovered audio outputs with active checkmarks in `TopBarQuickPopoverKind::Volume` (`TopBarQuickPopoverHit::SelectAudioOutput`) and added the active output badge + device switcher (`QuickSettingsHit::CycleAudioOutput`), `Dark Mode` toggle (`QuickSettingsHit::ToggleDarkMode`), and `Settings` button (`QuickSettingsHit::OpenSettings`) to `Control Center`.
3. **Notification Center Persistence & Live Telemetry:**
   - Added `reminders_done`, `show_weather_widget`, and `show_reminders_widget` under `[notifications]` in `crates/rusty-compositor/src/config.rs`, relative notification age labels (`"now"`, `"2m ago"`, `"1h ago"`), and live volume/battery summary inside `Notification Center`.

## Consequences
- Installed systems and Live ISOs have out-of-the-box audio across Intel/AMD SOF and standard HDA/HDMI/Bluetooth hardware.
- Users can switch audio output devices directly from `Control Center` or the `TopBar` Volume popover and persist `Notification Center` tasks and widget layout.
