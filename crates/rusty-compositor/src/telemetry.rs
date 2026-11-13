//! Asynchronous system telemetry poller & quick audio controls for `rusty-compositor` (`RUSTYOS-021`).
//!
//! Runs `rusty-services` hardware/CLI queries (`PowerService`, `NetworkService`, `AudioService`,
//! and local clock) on a background worker thread so the compositor event loop is never blocked.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusty_services::{
    AudioStatus, BluetoothStatus, BrightnessStatus, MediaAction, MediaStatus, NetworkStatus,
    PowerStatus, ServiceRegistry, SessionAction, UserAccount, local_clock_hhmm,
    local_clock_lock_date, local_clock_lock_hhmm, local_clock_short_datetime_fmt,
};

const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Snapshot of system hardware and service status rendered in `TopBar` and `QuickSettings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemTelemetry {
    /// Power / battery charge status.
    pub power: PowerStatus,
    /// Network connectivity and SSID / interface status.
    pub network: NetworkStatus,
    /// Bluetooth adapter & connected-device status (`RUSTYOS-027`).
    pub bluetooth: BluetoothStatus,
    /// Output volume percentage and mute flag.
    pub audio: AudioStatus,
    /// Display backlight brightness status (`RUSTYOS-031`).
    pub brightness: BrightnessStatus,
    /// MPRIS2 media playback status and active track metadata (`RUSTYOS-053`).
    pub media: MediaStatus,
    /// Local user accounts for Lock Screen user switcher (`RUSTYOS-054`).
    pub users: Vec<UserAccount>,
    /// Local wall-clock time formatted as `HH:MM`.
    pub clock_hhmm: String,
    /// Compact macOS-style `TopBar` date + 12h time label (`"Wed Apr 1  9:41 AM"`).
    pub clock_date_hhmm: String,
    /// Compact macOS-style `TopBar` date + 24h time label (`"Wed Apr 1  09:41"`).
    pub clock_date_24h: String,
    /// Lock Screen date header (`"Fri Nov 13"`) (`RUSTYOS-054`).
    pub lock_date: String,
    /// Lock Screen 12h clock without AM/PM suffix (`"8:52"`) (`RUSTYOS-054`).
    pub lock_time_hhmm: String,
    /// Lock Screen 24h clock (`"08:52"`) (`RUSTYOS-054`).
    pub lock_time_24h: String,
}

impl Default for SystemTelemetry {
    fn default() -> Self {
        Self {
            power: PowerStatus::default(),
            network: NetworkStatus::default(),
            bluetooth: BluetoothStatus::default(),
            audio: AudioStatus::default(),
            brightness: BrightnessStatus::default(),
            media: MediaStatus::default(),
            users: Vec::new(),
            clock_hhmm: "12:00".to_owned(),
            clock_date_hhmm: "Wed Apr 1  9:41 AM".to_owned(),
            clock_date_24h: "Wed Apr 1  09:41".to_owned(),
            lock_date: "Thu Sep 10".to_owned(),
            lock_time_hhmm: "12:00".to_owned(),
            lock_time_24h: "12:00".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelemetryCmd {
    RefreshNow,
    SetVolume(u8),
    ToggleMute,
    SetBluetoothPowered(bool),
    SetWifiEnabled(bool),
    SetBrightness(u8),
    MediaControl(MediaAction),
    Session(SessionAction),
}

struct SharedState {
    latest: Mutex<SystemTelemetry>,
    queue: Mutex<VecDeque<TelemetryCmd>>,
    cvar: Condvar,
    stop: AtomicBool,
}

/// Non-blocking handle to live system telemetry and async audio controls.
pub struct SystemMetricsHandle {
    shared: Arc<SharedState>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for SystemMetricsHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemMetricsHandle")
            .field("has_worker", &self.worker.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for SystemMetricsHandle {
    fn default() -> Self {
        Self::static_default()
    }
}

impl Drop for SystemMetricsHandle {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.cvar.notify_all();
    }
}

impl SystemMetricsHandle {
    /// Creates a non-spawning telemetry handle (used in headless unit tests).
    #[must_use]
    pub fn static_default() -> Self {
        Self::with_initial(SystemTelemetry::default())
    }

    /// Creates a deterministic non-spawning handle seeded with `initial` telemetry.
    #[must_use]
    pub fn with_initial(initial: SystemTelemetry) -> Self {
        Self {
            shared: Arc::new(SharedState {
                latest: Mutex::new(initial),
                queue: Mutex::new(VecDeque::new()),
                cvar: Condvar::new(),
                stop: AtomicBool::new(false),
            }),
            worker: None,
        }
    }

    /// Spawns a background poller thread refreshing `Power`, `Network`, `Audio`, and clock every 3s.
    #[must_use]
    pub fn start_background() -> Self {
        let shared = Arc::new(SharedState {
            latest: Mutex::new(SystemTelemetry::default()),
            queue: Mutex::new(VecDeque::new()),
            cvar: Condvar::new(),
            stop: AtomicBool::new(false),
        });

        let thread_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("rustyos-telemetry".to_owned())
            .spawn(move || telemetry_worker_loop(&thread_shared))
            .ok();

        Self { shared, worker }
    }

    /// Returns the latest cached telemetry without blocking on hardware or CLI I/O.
    #[must_use]
    pub fn snapshot(&self) -> SystemTelemetry {
        self.shared
            .latest
            .try_lock()
            .map_or_else(|_| SystemTelemetry::default(), |guard| guard.clone())
    }

    /// Schedules an immediate non-blocking background refresh of all services.
    pub fn request_refresh(&self) {
        self.push_cmd(TelemetryCmd::RefreshNow);
    }

    /// Adjusts master output volume by `delta` (-100..=100) optimistically and dispatches asynchronously.
    pub fn step_volume(&self, delta: i8) {
        let target = if let Ok(mut guard) = self.shared.latest.lock() {
            let cur = i16::from(guard.audio.volume_percent);
            let next = u8::try_from((cur + i16::from(delta)).clamp(0, 100)).unwrap_or(100);
            guard.audio.volume_percent = next;
            if delta > 0 {
                guard.audio.muted = false;
            }
            Some(next)
        } else {
            None
        };
        if let Some(pct) = target {
            self.push_cmd(TelemetryCmd::SetVolume(pct));
        }
    }

    /// Toggles master mute optimistically and dispatches asynchronously.
    pub fn toggle_mute(&self) {
        if let Ok(mut guard) = self.shared.latest.lock() {
            guard.audio.muted = !guard.audio.muted;
        }
        self.push_cmd(TelemetryCmd::ToggleMute);
    }

    /// Sets master output volume to `percent` (`0..=100`) optimistically and dispatches asynchronously (`RUSTYOS-048`).
    pub fn set_volume_percent(&self, percent: u8) {
        let clamped = percent.min(100);
        if let Ok(mut guard) = self.shared.latest.lock() {
            guard.audio.volume_percent = clamped;
            if clamped > 0 {
                guard.audio.muted = false;
            }
        }
        self.push_cmd(TelemetryCmd::SetVolume(clamped));
    }

    /// Adjusts master output volume by `delta` and returns `(volume_percent, muted)` for OSD display.
    #[must_use]
    pub fn step_volume_with_osd(&self, delta: i8) -> (u8, bool) {
        self.step_volume(delta);
        let snap = self.snapshot();
        (snap.audio.volume_percent, snap.audio.muted)
    }

    /// Toggles master mute and returns `(volume_percent, muted)` for OSD display.
    #[must_use]
    pub fn toggle_mute_with_osd(&self) -> (u8, bool) {
        self.toggle_mute();
        let snap = self.snapshot();
        (snap.audio.volume_percent, snap.audio.muted)
    }

    /// Adjusts display backlight brightness by `delta` (`1..=100`) optimistically and returns `(available, percentage)`.
    #[must_use]
    pub fn step_brightness_with_osd(&self, delta: i8) -> (bool, u8) {
        let mut target_to_apply = None;
        let result = if let Ok(mut guard) = self.shared.latest.lock() {
            if guard.brightness.available {
                let cur = i16::from(guard.brightness.percentage);
                let next = u8::try_from((cur + i16::from(delta)).clamp(1, 100)).unwrap_or(100);
                guard.brightness.percentage = next;
                target_to_apply = Some(next);
                (true, next)
            } else {
                (false, guard.brightness.percentage)
            }
        } else {
            (false, 100)
        };
        if let Some(pct) = target_to_apply {
            self.push_cmd(TelemetryCmd::SetBrightness(pct));
        }
        result
    }

    /// Sets display backlight brightness directly to `percent` (`5..=100`) when backlight hardware exists (`RUSTYOS-053`).
    #[must_use]
    pub fn set_brightness_percent(&self, percent: u8) -> bool {
        let clamped = percent.clamp(5, 100);
        let applied = if let Ok(mut guard) = self.shared.latest.lock()
            && guard.brightness.available
        {
            guard.brightness.percentage = clamped;
            true
        } else {
            false
        };
        if applied {
            self.push_cmd(TelemetryCmd::SetBrightness(clamped));
        }
        applied
    }

    /// Dispatches an MPRIS2 / `playerctl` media control action asynchronously (`RUSTYOS-031`).
    pub fn dispatch_media(&self, action: MediaAction) {
        if matches!(action, MediaAction::PlayPause)
            && let Ok(mut guard) = self.shared.latest.lock()
            && guard.media.active
        {
            guard.media.playing = !guard.media.playing;
        }
        self.push_cmd(TelemetryCmd::MediaControl(action));
    }

    /// Toggles Wi-Fi radio state optimistically and applies via `NetworkManager` / `nmcli` (`RUSTYOS-053`).
    #[must_use]
    pub fn toggle_wifi(&self) -> bool {
        let next_enabled = if let Ok(mut guard) = self.shared.latest.lock() {
            let currently_on = guard.network.wifi_enabled || guard.network.wifi_ssid.is_some();
            let next = !currently_on;
            guard.network.wifi_enabled = next;
            if next {
                if !guard.network.connected {
                    "Wi-Fi: On".clone_into(&mut guard.network.status_label);
                }
            } else {
                guard.network.wifi_ssid = None;
                guard.network.wifi_signal_percent = None;
                if guard
                    .network
                    .interface
                    .as_deref()
                    .is_some_and(|iface| iface.starts_with('w'))
                {
                    guard.network.connected = false;
                    "Wi-Fi Off".clone_into(&mut guard.network.status_label);
                }
            }
            next
        } else {
            return false;
        };
        self.push_cmd(TelemetryCmd::SetWifiEnabled(next_enabled));
        next_enabled
    }

    /// Toggles Bluetooth adapter power optimistically and applies via BlueZ asynchronously (`RUSTYOS-027`).
    pub fn toggle_bluetooth(&self) {
        let next_powered = if let Ok(mut guard) = self.shared.latest.lock() {
            let next = !guard.bluetooth.powered;
            if guard.bluetooth.available {
                guard.bluetooth.powered = next;
                guard.bluetooth.status_label = rusty_services::format_bluetooth_label(
                    true,
                    next,
                    guard.bluetooth.connected_device.as_deref(),
                );
                Some(next)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(enabled) = next_powered {
            self.push_cmd(TelemetryCmd::SetBluetoothPowered(enabled));
        }
    }

    /// Dispatches a session power action (`Suspend` / `Reboot` / `PowerOff`) asynchronously (`RUSTYOS-054`).
    pub fn dispatch_session(&self, action: SessionAction) {
        self.push_cmd(TelemetryCmd::Session(action));
    }

    fn push_cmd(&self, cmd: TelemetryCmd) {
        if let Ok(mut q) = self.shared.queue.lock() {
            q.push_back(cmd);
        }
        self.shared.cvar.notify_all();
    }
}

fn poll_once(services: &ServiceRegistry) -> SystemTelemetry {
    SystemTelemetry {
        power: services.power.query_status(),
        network: services.network.query_status(),
        bluetooth: services.bluetooth.query_status(),
        audio: services.audio.query_status(),
        brightness: services.brightness.query_status(),
        media: services.media.query_status(),
        users: services.users.query_users(),
        clock_hhmm: local_clock_hhmm(),
        clock_date_hhmm: local_clock_short_datetime_fmt(false),
        clock_date_24h: local_clock_short_datetime_fmt(true),
        lock_date: local_clock_lock_date(),
        lock_time_hhmm: local_clock_lock_hhmm(false),
        lock_time_24h: local_clock_lock_hhmm(true),
    }
}

fn telemetry_worker_loop(shared: &SharedState) {
    let services = ServiceRegistry::new();

    // Perform initial snapshot outside lock
    let initial = poll_once(&services);
    if let Ok(mut latest) = shared.latest.lock() {
        *latest = initial;
    }

    while !shared.stop.load(Ordering::Acquire) {
        let mut cmds = Vec::new();
        if let Ok(mut guard) = shared.queue.lock() {
            if guard.is_empty() {
                if let Ok((new_guard, _)) = shared.cvar.wait_timeout(guard, POLL_INTERVAL) {
                    guard = new_guard;
                } else {
                    continue;
                }
            }
            while let Some(c) = guard.pop_front() {
                cmds.push(c);
            }
        }

        if shared.stop.load(Ordering::Acquire) {
            break;
        }

        for cmd in cmds {
            match cmd {
                TelemetryCmd::RefreshNow => {}
                TelemetryCmd::SetVolume(pct) => {
                    let _ = services.audio.set_volume(pct);
                }
                TelemetryCmd::ToggleMute => {
                    let _ = services.audio.toggle_mute();
                }
                TelemetryCmd::SetBluetoothPowered(enabled) => {
                    let _ = services.bluetooth.set_powered(enabled);
                }
                TelemetryCmd::SetWifiEnabled(enabled) => {
                    let _ = services.network.set_wifi_enabled(enabled);
                }
                TelemetryCmd::SetBrightness(pct) => {
                    let _ = services.brightness.set_percent(pct);
                }
                TelemetryCmd::MediaControl(action) => {
                    let _ = services.media.dispatch(action);
                }
                TelemetryCmd::Session(action) => {
                    let _ = services.session.dispatch(action);
                }
            }
        }

        let snap = poll_once(&services);
        if let Ok(mut latest) = shared.latest.lock() {
            *latest = snap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_telemetry_steps_volume_and_mute_optimistically_within_bounds() {
        let handle = SystemMetricsHandle::static_default();
        assert_eq!(handle.snapshot().audio.volume_percent, 75);

        handle.step_volume(10);
        assert_eq!(handle.snapshot().audio.volume_percent, 85);

        handle.step_volume(50);
        assert_eq!(handle.snapshot().audio.volume_percent, 100);

        handle.toggle_mute();
        assert!(handle.snapshot().audio.muted);

        handle.step_volume(-30);
        assert_eq!(handle.snapshot().audio.volume_percent, 70);
        assert!(handle.snapshot().audio.muted);

        let initial_bt = SystemTelemetry {
            bluetooth: BluetoothStatus {
                available: true,
                powered: false,
                connected_device: None,
                status_label: "BT: Off".to_owned(),
            },
            ..SystemTelemetry::default()
        };
        let bt_handle = SystemMetricsHandle::with_initial(initial_bt);
        bt_handle.toggle_bluetooth();
        assert!(bt_handle.snapshot().bluetooth.powered);
        assert_eq!(bt_handle.snapshot().bluetooth.status_label, "BT: On");

        let bright_handle = SystemMetricsHandle::with_initial(SystemTelemetry {
            brightness: BrightnessStatus {
                available: true,
                percentage: 40,
                device: Some("intel_backlight".to_owned()),
            },
            ..SystemTelemetry::default()
        });
        assert_eq!(bright_handle.step_brightness_with_osd(10), (true, 50));
        assert_eq!(bright_handle.step_brightness_with_osd(-90), (true, 1));
        assert!(bright_handle.set_brightness_percent(2));
        assert_eq!(bright_handle.snapshot().brightness.percentage, 5);
        assert!(bright_handle.set_brightness_percent(82));
        assert_eq!(bright_handle.snapshot().brightness.percentage, 82);

        let wifi_handle = SystemMetricsHandle::with_initial(SystemTelemetry {
            network: NetworkStatus {
                connected: true,
                wifi_enabled: true,
                interface: Some("wlan0".to_owned()),
                wifi_ssid: Some("LabNet".to_owned()),
                wifi_signal_percent: Some(74),
                status_label: "Wi-Fi: LabNet".to_owned(),
            },
            media: MediaStatus {
                active: true,
                playing: false,
                title: "Test Track".to_owned(),
                artist: Some("Artist".to_owned()),
            },
            ..SystemTelemetry::default()
        });
        assert!(!wifi_handle.toggle_wifi());
        assert!(!wifi_handle.snapshot().network.wifi_enabled);
        assert!(!wifi_handle.snapshot().network.connected);
        assert_eq!(wifi_handle.snapshot().network.status_label, "Wi-Fi Off");
        wifi_handle.dispatch_media(MediaAction::PlayPause);
        assert!(wifi_handle.snapshot().media.playing);

        handle.set_volume_percent(42);
        assert_eq!(handle.snapshot().audio.volume_percent, 42);
        assert!(!handle.snapshot().audio.muted);
    }
}
