//! Asynchronous system telemetry poller & quick audio controls for `rusty-compositor` (`RUSTYOS-021`).
//!
//! Runs `rusty-services` hardware/CLI queries (`PowerService`, `NetworkService`, `AudioService`,
//! and local clock) on a background worker thread so the compositor event loop is never blocked.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusty_services::{
    AudioStatus, BluetoothStatus, BrightnessStatus, DeviceUsageService, DeviceUsageSnapshot,
    MediaAction, MediaStatus, NetworkStatus, PowerProfile, PowerStatus, ServiceRegistry,
    SessionAction, SystemSignalMask, SystemSignalMonitorHandle, UserAccount, local_clock_hhmm,
    local_clock_lock_date, local_clock_lock_hhmm, local_clock_short_datetime_localized,
};

const POLL_INTERVAL_ACTIVE: Duration = Duration::from_secs(3);
const POLL_INTERVAL_IDLE: Duration = Duration::from_secs(12);
const SIGNAL_COALESCE_DEBOUNCE: Duration = Duration::from_millis(80);

/// Callback invoked from the background telemetry thread when a live snapshot changes (`RUSTYOS-164`).
pub type TelemetryChangeCallback = Arc<dyn Fn() + Send + Sync>;

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
    /// Active microphone and camera privacy telemetry (`RUSTYOS-142`).
    pub device_usage: DeviceUsageSnapshot,
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
            device_usage: DeviceUsageSnapshot::default(),
            clock_hhmm: "12:00".to_owned(),
            clock_date_hhmm: "Wed Apr 1  9:41 AM".to_owned(),
            clock_date_24h: "Wed Apr 1  09:41".to_owned(),
            lock_date: "Thu Sep 10".to_owned(),
            lock_time_hhmm: "12:00".to_owned(),
            lock_time_24h: "12:00".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TelemetryCmd {
    RefreshNow,
    SetVolume(u8),
    ToggleMute,
    SetAudioOutput(String),
    CycleAudioOutput,
    SetBluetoothPowered(bool),
    SetPowerProfile(PowerProfile),
    SetWifiEnabled(bool),
    ConnectWifi(String),
    SetBrightness(u8),
    MediaControl(MediaAction),
    Session(SessionAction),
}

struct SharedState {
    latest: Mutex<Arc<SystemTelemetry>>,
    queue: Mutex<VecDeque<TelemetryCmd>>,
    clock_locale: Mutex<String>,
    cvar: Condvar,
    stop: AtomicBool,
    active_ui: AtomicBool,
    revision: AtomicU64,
    pending_signals: AtomicU8,
    on_change: Mutex<Option<TelemetryChangeCallback>>,
}

impl SharedState {
    fn new(initial: SystemTelemetry) -> Self {
        Self {
            latest: Mutex::new(Arc::new(initial)),
            queue: Mutex::new(VecDeque::new()),
            clock_locale: Mutex::new("en".to_owned()),
            cvar: Condvar::new(),
            stop: AtomicBool::new(false),
            active_ui: AtomicBool::new(false),
            revision: AtomicU64::new(0),
            pending_signals: AtomicU8::new(0),
            on_change: Mutex::new(None),
        }
    }

    fn notify_change(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        let cb = self.on_change.lock().ok().and_then(|g| g.clone());
        if let Some(wake) = cb {
            wake();
        }
    }

    fn update_snapshot_if_changed(&self, snap: SystemTelemetry) -> bool {
        let changed = if let Ok(mut latest) = self.latest.lock() {
            if **latest == snap {
                false
            } else {
                *latest = Arc::new(snap);
                true
            }
        } else {
            false
        };
        if changed {
            self.notify_change();
        }
        changed
    }

    fn enqueue_signal(&self, mask: SystemSignalMask) {
        if mask.is_empty() {
            return;
        }
        self.pending_signals.fetch_or(mask.bits(), Ordering::AcqRel);
        if let Ok(_guard) = self.queue.lock() {
            self.cvar.notify_all();
        }
    }
}

/// Non-blocking handle to live system telemetry and async audio controls.
pub struct SystemMetricsHandle {
    shared: Arc<SharedState>,
    worker: Option<JoinHandle<()>>,
    _signal_monitor: Option<SystemSignalMonitorHandle>,
}

impl fmt::Debug for SystemMetricsHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemMetricsHandle")
            .field("has_worker", &self.worker.is_some())
            .field("active_ui", &self.is_active_ui())
            .field("revision", &self.revision())
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
        if let Ok(_guard) = self.shared.queue.lock() {
            self.shared.cvar.notify_all();
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
            shared: Arc::new(SharedState::new(initial)),
            worker: None,
            _signal_monitor: None,
        }
    }

    /// Spawns a background telemetry worker plus reactive system D-Bus signal subscriptions
    /// (`UPower`, `NetworkManager`, `BlueZ`) with 3s active / 12s idle fallback polling (`RUSTYOS-164`).
    #[must_use]
    pub fn start_background() -> Self {
        let shared = Arc::new(SharedState::new(SystemTelemetry::default()));

        let signal_shared = Arc::clone(&shared);
        let signal_monitor = SystemSignalMonitorHandle::start_background(Arc::new(move |mask| {
            signal_shared.enqueue_signal(mask);
        }));

        let thread_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("rustyos-telemetry".to_owned())
            .spawn(move || telemetry_worker_loop(&thread_shared))
            .ok();

        Self {
            shared,
            worker,
            _signal_monitor: Some(signal_monitor),
        }
    }

    /// Installs or replaces the wakeup callback invoked whenever cached telemetry changes (`RUSTYOS-164`).
    pub fn set_on_change(&self, cb: TelemetryChangeCallback) {
        if let Ok(mut guard) = self.shared.on_change.lock() {
            *guard = Some(cb);
        }
    }

    /// Returns the monotonic revision incremented whenever the telemetry snapshot changes (`RUSTYOS-164`).
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.shared.revision.load(Ordering::Acquire)
    }

    /// Notifies the background telemetry worker that one or more system D-Bus domains changed (`RUSTYOS-164`).
    pub fn notify_system_signal(&self, mask: SystemSignalMask) {
        self.shared.enqueue_signal(mask);
    }

    /// Replaces the cached telemetry snapshot and fires the change callback if it differs (`RUSTYOS-164`).
    #[must_use]
    pub fn replace_snapshot(&self, snap: SystemTelemetry) -> bool {
        self.shared.update_snapshot_if_changed(snap)
    }

    /// Returns a shared read-only handle to the latest cached telemetry without cloning strings or lists (`RUSTYOS-119`).
    #[must_use]
    pub fn snapshot_arc(&self) -> Arc<SystemTelemetry> {
        self.shared.latest.try_lock().map_or_else(
            |_| Arc::new(SystemTelemetry::default()),
            |guard| Arc::clone(&guard),
        )
    }

    /// Returns an owned copy of the latest cached telemetry without blocking on hardware or CLI I/O.
    #[must_use]
    pub fn snapshot(&self) -> SystemTelemetry {
        (*self.snapshot_arc()).clone()
    }

    /// Schedules an immediate non-blocking background refresh of all services.
    pub fn request_refresh(&self) {
        self.push_cmd(TelemetryCmd::RefreshNow);
    }

    /// Marks whether live telemetry popovers (Control Center, Wi-Fi popover) are open (`RUSTYOS-120`).
    /// When active, polling runs every 3s. When closed, polling relaxes to 12s to save CPU and battery.
    pub fn set_active_ui(&self, active: bool) {
        let was_active = self.shared.active_ui.swap(active, Ordering::Relaxed);
        if active && !was_active {
            self.shared.cvar.notify_all();
        }
    }

    /// Returns whether the UI poller is in active (frequent) mode.
    #[must_use]
    pub fn is_active_ui(&self) -> bool {
        self.shared.active_ui.load(Ordering::Relaxed)
    }

    /// Adjusts master output volume by `delta` (-100..=100) optimistically and dispatches asynchronously.
    pub fn step_volume(&self, delta: i8) {
        let target = if let Ok(mut guard) = self.shared.latest.lock() {
            let state = Arc::make_mut(&mut guard);
            let cur = i16::from(state.audio.volume_percent);
            let next = u8::try_from((cur + i16::from(delta)).clamp(0, 100)).unwrap_or(100);
            state.audio.volume_percent = next;
            if delta > 0 {
                state.audio.muted = false;
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
            let state = Arc::make_mut(&mut guard);
            state.audio.muted = !state.audio.muted;
        }
        self.push_cmd(TelemetryCmd::ToggleMute);
    }

    /// Sets master output volume to `percent` (`0..=100`) optimistically and dispatches asynchronously (`RUSTYOS-048`).
    pub fn set_volume_percent(&self, percent: u8) {
        let clamped = percent.min(100);
        if let Ok(mut guard) = self.shared.latest.lock() {
            let state = Arc::make_mut(&mut guard);
            state.audio.volume_percent = clamped;
            if clamped > 0 {
                state.audio.muted = false;
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
            let cur_state = Arc::make_mut(&mut guard);
            if cur_state.brightness.available {
                let cur = i16::from(cur_state.brightness.percentage);
                let next = u8::try_from((cur + i16::from(delta)).clamp(1, 100)).unwrap_or(100);
                cur_state.brightness.percentage = next;
                target_to_apply = Some(next);
                (true, next)
            } else {
                (false, cur_state.brightness.percentage)
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
            Arc::make_mut(&mut guard).brightness.percentage = clamped;
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
            let state = Arc::make_mut(&mut guard);
            state.media.playing = !state.media.playing;
        }
        self.push_cmd(TelemetryCmd::MediaControl(action));
    }

    /// Toggles Wi-Fi radio state optimistically and applies via `NetworkManager` / `nmcli` (`RUSTYOS-053`).
    #[must_use]
    pub fn toggle_wifi(&self) -> bool {
        let next_enabled = if let Ok(mut guard) = self.shared.latest.lock() {
            let state = Arc::make_mut(&mut guard);
            let currently_on = state.network.wifi_enabled || state.network.wifi_ssid.is_some();
            let next = !currently_on;
            state.network.wifi_enabled = next;
            if next {
                if !state.network.connected {
                    "Wi-Fi: On".clone_into(&mut state.network.status_label);
                }
            } else {
                state.network.wifi_connected = false;
                state.network.wifi_ssid = None;
                state.network.wifi_signal_percent = None;
                for ap in &mut state.network.wifi_networks {
                    ap.connected = false;
                }
                if state.network.wired_connected {
                    state.network.connected = true;
                    state.network.status_label = state
                        .network
                        .wired_label
                        .clone()
                        .unwrap_or_else(|| "Ethernet Connected".to_owned());
                } else if state
                    .network
                    .interface
                    .as_deref()
                    .is_some_and(|iface| iface.starts_with('w'))
                {
                    state.network.connected = false;
                    "Wi-Fi Off".clone_into(&mut state.network.status_label);
                }
            }
            next
        } else {
            return false;
        };
        self.push_cmd(TelemetryCmd::SetWifiEnabled(next_enabled));
        next_enabled
    }

    /// Connects to the given Wi-Fi `ssid` optimistically and dispatches `nmcli` asynchronously (`RUSTYOS-108`).
    pub fn connect_wifi(&self, ssid: &str) {
        let trimmed = ssid.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.shared.latest.lock() {
            let state = Arc::make_mut(&mut guard);
            state.network.connected = true;
            state.network.wifi_enabled = true;
            state.network.wifi_connected = true;
            state.network.wifi_ssid = Some(trimmed.to_owned());
            let mut matched_sig = None;
            for ap in &mut state.network.wifi_networks {
                ap.connected = ap.ssid == trimmed;
                if ap.connected {
                    ap.known = true;
                    matched_sig = Some(ap.signal_percent);
                }
            }
            if let Some(sig) = matched_sig {
                state.network.wifi_signal_percent = Some(sig);
            }
            if !state.network.wired_connected {
                state.network.status_label = format!("Wi-Fi: {trimmed}");
            }
        }
        self.push_cmd(TelemetryCmd::ConnectWifi(trimmed.to_owned()));
    }

    /// Toggles Bluetooth adapter power optimistically and applies via BlueZ asynchronously (`RUSTYOS-027`).
    pub fn toggle_bluetooth(&self) {
        let next_powered = if let Ok(mut guard) = self.shared.latest.lock() {
            if guard.bluetooth.available {
                let state = Arc::make_mut(&mut guard);
                let next = !state.bluetooth.powered;
                state.bluetooth.powered = next;
                state.bluetooth.status_label = rusty_services::format_bluetooth_label(
                    true,
                    next,
                    state.bluetooth.connected_device.as_deref(),
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

    /// Requests switching the active `power-profiles-daemon` power profile asynchronously (`RUSTYOS-131`).
    pub fn set_power_profile(&self, profile: PowerProfile) {
        self.push_cmd(TelemetryCmd::SetPowerProfile(profile));
    }

    /// Switches the default audio output sink by index into `snapshot().audio.outputs` (`RUSTYOS-156`).
    pub fn select_audio_output_index(&self, idx: usize) {
        let mut target_id = None;
        if let Ok(mut guard) = self.shared.latest.lock()
            && idx < guard.audio.outputs.len()
        {
            let state = Arc::make_mut(&mut guard);
            for (i, dev) in state.audio.outputs.iter_mut().enumerate() {
                dev.is_default = i == idx;
                if dev.is_default {
                    dev.name.clone_into(&mut state.audio.active_output_name);
                    target_id = Some(dev.id.clone());
                }
            }
        }
        if let Some(id) = target_id {
            self.push_cmd(TelemetryCmd::SetAudioOutput(id));
        }
    }

    /// Cycles to the next available audio output sink optimistically (`RUSTYOS-156`).
    pub fn cycle_audio_output(&self) {
        if let Ok(mut guard) = self.shared.latest.lock()
            && guard.audio.outputs.len() >= 2
        {
            let state = Arc::make_mut(&mut guard);
            let cur = state
                .audio
                .outputs
                .iter()
                .position(|o| o.is_default)
                .unwrap_or(0);
            let next = (cur + 1) % state.audio.outputs.len();
            for (i, dev) in state.audio.outputs.iter_mut().enumerate() {
                dev.is_default = i == next;
                if dev.is_default {
                    dev.name.clone_into(&mut state.audio.active_output_name);
                }
            }
        }
        self.push_cmd(TelemetryCmd::CycleAudioOutput);
    }

    fn push_cmd(&self, cmd: TelemetryCmd) {
        if let Ok(mut q) = self.shared.queue.lock() {
            q.push_back(cmd);
        }
        self.shared.cvar.notify_all();
    }
    /// Updates the session clock locale (`"en"`, `"ru"`, `"uk"`) and triggers refresh (`RUSTYOS-141`).
    pub fn set_clock_locale(&self, locale: &str) {
        if let Ok(mut guard) = self.shared.clock_locale.lock()
            && *guard != locale
        {
            locale.clone_into(&mut guard);
            drop(guard);
            self.request_refresh();
        }
    }
}

fn refresh_clock_fields(snap: &mut SystemTelemetry, clock_locale: &str) {
    snap.clock_hhmm = local_clock_hhmm();
    snap.clock_date_hhmm = local_clock_short_datetime_localized(clock_locale, false);
    snap.clock_date_24h = local_clock_short_datetime_localized(clock_locale, true);
    snap.lock_date = local_clock_lock_date();
    snap.lock_time_hhmm = local_clock_lock_hhmm(false);
    snap.lock_time_24h = local_clock_lock_hhmm(true);
}

fn poll_once(services: &ServiceRegistry, clock_locale: &str) -> SystemTelemetry {
    let mut snap = SystemTelemetry {
        power: services.power.query_status(),
        network: services.network.query_status(),
        bluetooth: services.bluetooth.query_status(),
        audio: services.audio.query_status(),
        brightness: services.brightness.query_status(),
        media: services.media.query_status(),
        users: services.users.query_users(),
        device_usage: DeviceUsageService.query_snapshot(),
        ..SystemTelemetry::default()
    };
    refresh_clock_fields(&mut snap, clock_locale);
    snap
}

fn poll_targeted(
    services: &ServiceRegistry,
    shared: &SharedState,
    mask: SystemSignalMask,
    clock_locale: &str,
) -> SystemTelemetry {
    let mut snap = shared
        .latest
        .lock()
        .map_or_else(|_| SystemTelemetry::default(), |g| (**g).clone());
    if mask.has_power() {
        snap.power = services.power.query_status();
    }
    if mask.has_network() {
        snap.network = services.network.query_status();
    }
    if mask.has_bluetooth() {
        snap.bluetooth = services.bluetooth.query_status();
    }
    if mask.has_audio() {
        snap.audio = services.audio.query_status();
    }
    refresh_clock_fields(&mut snap, clock_locale);
    snap
}

fn apply_telemetry_cmd(services: &ServiceRegistry, cmd: TelemetryCmd) {
    match cmd {
        TelemetryCmd::RefreshNow => {}
        TelemetryCmd::SetVolume(pct) => {
            let _ = services.audio.set_volume(pct);
        }
        TelemetryCmd::ToggleMute => {
            let _ = services.audio.toggle_mute();
        }
        TelemetryCmd::SetAudioOutput(id) => {
            let _ = services.audio.set_default_output(&id);
        }
        TelemetryCmd::CycleAudioOutput => {
            let _ = services.audio.cycle_next_output();
        }
        TelemetryCmd::SetBluetoothPowered(enabled) => {
            let _ = services.bluetooth.set_powered(enabled);
        }
        TelemetryCmd::SetPowerProfile(profile) => {
            let _ = services.power.set_power_profile(profile);
        }
        TelemetryCmd::SetWifiEnabled(enabled) => {
            let _ = services.network.set_wifi_enabled(enabled);
        }
        TelemetryCmd::ConnectWifi(ssid) => {
            if let Err(err) = services.network.connect_wifi_with_password(&ssid, None) {
                tracing::warn!(ssid = %ssid, error = %err, "Wi-Fi connection failed");
            }
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

fn wait_coalesced_signals(
    shared: &SharedState,
    initial_bits: u8,
    cmds: &mut Vec<TelemetryCmd>,
) -> SystemSignalMask {
    let mut combined = initial_bits;
    let deadline = Instant::now() + SIGNAL_COALESCE_DEBOUNCE;
    if let Ok(mut guard) = shared.queue.lock() {
        loop {
            if shared.stop.load(Ordering::Acquire) || !guard.is_empty() {
                break;
            }
            let Some(rem) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            if rem.is_zero() {
                break;
            }
            let Ok((new_guard, _)) = shared.cvar.wait_timeout(guard, rem) else {
                combined |= shared.pending_signals.swap(0, Ordering::AcqRel);
                return SystemSignalMask::from_bits(combined);
            };
            guard = new_guard;
            combined |= shared.pending_signals.swap(0, Ordering::AcqRel);
        }
        while let Some(c) = guard.pop_front() {
            cmds.push(c);
        }
    }
    combined |= shared.pending_signals.swap(0, Ordering::AcqRel);
    SystemSignalMask::from_bits(combined)
}

fn read_clock_locale(shared: &SharedState) -> String {
    shared
        .clock_locale
        .lock()
        .map_or_else(|_| "en".to_owned(), |g| g.clone())
}

fn telemetry_worker_loop(shared: &SharedState) {
    let services = ServiceRegistry::new();
    services.audio.ensure_session_audio_ready();

    // Perform initial snapshot outside lock
    let locale = read_clock_locale(shared);
    let initial = poll_once(&services, &locale);
    let _ = shared.update_snapshot_if_changed(initial);

    while !shared.stop.load(Ordering::Acquire) {
        let mut cmds = Vec::new();
        if let Ok(mut guard) = shared.queue.lock() {
            if guard.is_empty() && shared.pending_signals.load(Ordering::Acquire) == 0 {
                let interval = if shared.active_ui.load(Ordering::Relaxed) {
                    POLL_INTERVAL_ACTIVE
                } else {
                    POLL_INTERVAL_IDLE
                };
                if let Ok((new_guard, _)) = shared.cvar.wait_timeout(guard, interval) {
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

        let signal_bits = shared.pending_signals.swap(0, Ordering::AcqRel);
        if cmds.is_empty() && signal_bits != 0 {
            let mask = wait_coalesced_signals(shared, signal_bits, &mut cmds);
            if shared.stop.load(Ordering::Acquire) {
                break;
            }
            if cmds.is_empty() {
                let locale = read_clock_locale(shared);
                let snap = poll_targeted(&services, shared, mask, &locale);
                let _ = shared.update_snapshot_if_changed(snap);
                continue;
            }
        }

        for cmd in cmds {
            apply_telemetry_cmd(&services, cmd);
        }

        let locale = read_clock_locale(shared);
        let snap = poll_once(&services, &locale);
        let _ = shared.update_snapshot_if_changed(snap);
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
                wired_connected: false,
                wired_interface: None,
                wired_label: None,
                wifi_connected: true,
                wifi_enabled: true,
                interface: Some("wlan0".to_owned()),
                wifi_ssid: Some("LabNet".to_owned()),
                wifi_signal_percent: Some(74),
                wifi_networks: Vec::new(),
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

        let arc1 = handle.snapshot_arc();
        let arc2 = handle.snapshot_arc();
        assert!(Arc::ptr_eq(&arc1, &arc2));
        handle.set_volume_percent(60);
        let arc3 = handle.snapshot_arc();
        assert!(!Arc::ptr_eq(&arc1, &arc3));
        assert_eq!(arc3.audio.volume_percent, 60);

        assert!(!handle.is_active_ui());
        handle.set_active_ui(true);
        assert!(handle.is_active_ui());
        handle.set_active_ui(false);
        assert!(!handle.is_active_ui());
    }

    #[test]
    fn replace_snapshot_bumps_revision_and_fires_on_change_only_on_delta() {
        let handle = SystemMetricsHandle::static_default();
        let wakes = Arc::new(AtomicU64::new(0));
        let wakes_cb = Arc::clone(&wakes);
        handle.set_on_change(Arc::new(move || {
            wakes_cb.fetch_add(1, Ordering::AcqRel);
        }));
        assert_eq!(handle.revision(), 0);

        // Identical snapshot must not bump revision or wake the compositor.
        assert!(!handle.replace_snapshot(SystemTelemetry::default()));
        assert_eq!(handle.revision(), 0);
        assert_eq!(wakes.load(Ordering::Acquire), 0);

        // Changed power state bumps revision and invokes the wakeup callback.
        let mut unplugged = SystemTelemetry::default();
        unplugged.power.has_battery = true;
        unplugged.power.ac_online = false;
        unplugged.power.percentage = 78;
        unplugged.power.state = rusty_services::BatteryState::Discharging;
        unplugged.power.time_remaining_minutes = Some(135);
        assert!(handle.replace_snapshot(unplugged.clone()));
        assert_eq!(handle.revision(), 1);
        assert_eq!(wakes.load(Ordering::Acquire), 1);

        // Repeating the same unplugged snapshot stays silent.
        assert!(!handle.replace_snapshot(unplugged));
        assert_eq!(handle.revision(), 1);
        assert_eq!(wakes.load(Ordering::Acquire), 1);
    }
}
