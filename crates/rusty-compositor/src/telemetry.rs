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
    AudioStatus, BluetoothStatus, NetworkStatus, PowerStatus, ServiceRegistry, local_clock_hhmm,
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
    /// Local wall-clock time formatted as `HH:MM`.
    pub clock_hhmm: String,
}

impl Default for SystemTelemetry {
    fn default() -> Self {
        Self {
            power: PowerStatus::default(),
            network: NetworkStatus::default(),
            bluetooth: BluetoothStatus::default(),
            audio: AudioStatus::default(),
            clock_hhmm: "12:00".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelemetryCmd {
    RefreshNow,
    SetVolume(u8),
    ToggleMute,
    SetBluetoothPowered(bool),
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
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
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
        clock_hhmm: local_clock_hhmm(),
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
    }
}
