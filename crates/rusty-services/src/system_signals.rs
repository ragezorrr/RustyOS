//! Event-driven system D-Bus signal monitor for `UPower`, `NetworkManager`, and `BlueZ` (`RUSTYOS-164`).
//!
//! Eliminates multi-second `TopBar` lag when plugging/unplugging AC power, switching Wi-Fi/Ethernet,
//! or toggling Bluetooth by subscribing directly to D-Bus `PropertiesChanged` and state-change signals
//! on a single lightweight background thread with graceful no-op fallback when system D-Bus is unavailable.

use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zbus::MatchRule;

/// Bitmask identifying which hardware telemetry domains received a live D-Bus change signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemSignalMask(u8);

impl SystemSignalMask {
    /// Empty mask (no pending change).
    pub const EMPTY: Self = Self(0);
    /// Battery / AC power (`org.freedesktop.UPower`).
    pub const POWER: Self = Self(1 << 0);
    /// Wi-Fi / Ethernet (`org.freedesktop.NetworkManager`).
    pub const NETWORK: Self = Self(1 << 1);
    /// Bluetooth adapter / devices (`org.bluez`).
    pub const BLUETOOTH: Self = Self(1 << 2);
    /// Audio volume / sink changes (`PipeWire` / `PulseAudio`).
    pub const AUDIO: Self = Self(1 << 3);
    /// All reactive system signal domains combined.
    pub const ALL: Self = Self(Self::POWER.0 | Self::NETWORK.0 | Self::BLUETOOTH.0 | Self::AUDIO.0);

    /// Returns raw bit representation for atomic storage.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Reconstructs a mask from raw bits.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// Returns `true` when no bits are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns `true` when `UPower` signalled a power/battery change.
    #[must_use]
    pub const fn has_power(self) -> bool {
        self.0 & Self::POWER.0 != 0
    }

    /// Returns `true` when `NetworkManager` signalled a network change.
    #[must_use]
    pub const fn has_network(self) -> bool {
        self.0 & Self::NETWORK.0 != 0
    }

    /// Returns `true` when `BlueZ` signalled a Bluetooth change.
    #[must_use]
    pub const fn has_bluetooth(self) -> bool {
        self.0 & Self::BLUETOOTH.0 != 0
    }

    /// Returns `true` when `PipeWire` or `PulseAudio` signalled an audio change.
    #[must_use]
    pub const fn has_audio(self) -> bool {
        self.0 & Self::AUDIO.0 != 0
    }

    /// Combines two masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Classifies a system D-Bus signal object path into a [`SystemSignalMask`].
#[must_use]
pub fn classify_system_signal_path(object_path: &str) -> SystemSignalMask {
    if object_path == "/org/freedesktop/UPower"
        || object_path.starts_with("/org/freedesktop/UPower/")
    {
        SystemSignalMask::POWER
    } else if object_path == "/org/freedesktop/NetworkManager"
        || object_path.starts_with("/org/freedesktop/NetworkManager/")
    {
        SystemSignalMask::NETWORK
    } else if object_path == "/org/bluez" || object_path.starts_with("/org/bluez/") {
        SystemSignalMask::BLUETOOTH
    } else {
        SystemSignalMask::EMPTY
    }
}

/// Non-blocking handle to the background system signal listener (`UPower`, `NetworkManager`, `BlueZ`, `PipeWire`).
pub struct SystemSignalMonitorHandle {
    stop: Arc<AtomicBool>,
    conn_slot: Arc<std::sync::Mutex<Option<zbus::blocking::Connection>>>,
    worker: Option<JoinHandle<()>>,
    pulse_worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for SystemSignalMonitorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemSignalMonitorHandle")
            .field("active", &self.worker.is_some())
            .field("pulse_active", &self.pulse_worker.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for SystemSignalMonitorHandle {
    fn default() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(true)),
            conn_slot: Arc::new(std::sync::Mutex::new(None)),
            worker: None,
            pulse_worker: None,
        }
    }
}

impl Drop for SystemSignalMonitorHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(mut guard) = self.conn_slot.lock()
            && let Some(conn) = guard.take()
        {
            let _ = conn.close();
        }
    }
}

impl SystemSignalMonitorHandle {
    /// Spawns a background system signal listener invoking `on_signal(mask)` whenever
    /// `UPower`, `NetworkManager`, `BlueZ`, or `PipeWire` emits a relevant change signal (`RUSTYOS-171`).
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn start_background(on_signal: Arc<dyn Fn(SystemSignalMask) + Send + Sync>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = Arc::clone(&stop);
        let conn_slot = Arc::new(std::sync::Mutex::new(None));
        let conn_bg = Arc::clone(&conn_slot);
        let dbus_cb = Arc::clone(&on_signal);
        let worker = thread::Builder::new()
            .name("rustyos-sys-signals".to_owned())
            .spawn(move || signal_listener_loop(&stop_bg, &conn_bg, &dbus_cb))
            .ok();

        let pulse_stop = Arc::clone(&stop);
        let pulse_cb = Arc::clone(&on_signal);
        let pulse_worker = thread::Builder::new()
            .name("rustyos-pulse-signals".to_owned())
            .spawn(move || pulse_signal_listener_loop(&pulse_stop, &pulse_cb))
            .ok();

        Self {
            stop,
            conn_slot,
            worker,
            pulse_worker,
        }
    }
}

fn build_match_rules() -> Vec<MatchRule<'static>> {
    let mut rules = Vec::with_capacity(3);
    if let Ok(r) = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus.Properties")
        .and_then(|b| b.member("PropertiesChanged"))
        .and_then(|b| b.path_namespace("/org/freedesktop/UPower"))
    {
        rules.push(r.build());
    }
    if let Ok(r) = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path_namespace("/org/freedesktop/NetworkManager")
    {
        rules.push(r.build());
    }
    if let Ok(r) = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus.Properties")
        .and_then(|b| b.member("PropertiesChanged"))
        .and_then(|b| b.path_namespace("/org/bluez"))
    {
        rules.push(r.build());
    }
    rules
}

fn signal_listener_loop(
    stop: &AtomicBool,
    conn_slot: &std::sync::Mutex<Option<zbus::blocking::Connection>>,
    on_signal: &Arc<dyn Fn(SystemSignalMask) + Send + Sync>,
) {
    let Ok(conn) = zbus::blocking::Connection::system() else {
        return;
    };
    if let Ok(mut guard) = conn_slot.lock() {
        *guard = Some(conn.clone());
    }
    if stop.load(Ordering::Acquire) {
        let _ = conn.close();
        return;
    }
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return;
    };
    let mut registered_any = false;
    for rule in build_match_rules() {
        if dbus.add_match_rule(rule).is_ok() {
            registered_any = true;
        }
    }
    if !registered_any {
        return;
    }

    let iter = zbus::blocking::MessageIterator::from(&conn);
    for msg in iter {
        if stop.load(Ordering::Acquire) {
            break;
        }
        let Ok(message) = msg else {
            break;
        };
        if message.message_type() != zbus::message::Type::Signal {
            continue;
        }
        let header = message.header();
        let Some(path) = header.path() else {
            continue;
        };
        let mask = classify_system_signal_path(path.as_str());
        if !mask.is_empty() {
            on_signal(mask);
        }
    }
}

fn pulse_signal_listener_loop(
    stop: &AtomicBool,
    on_signal: &Arc<dyn Fn(SystemSignalMask) + Send + Sync>,
) {
    while !stop.load(Ordering::Acquire) {
        if let Ok(mut client) = crate::audio_pulse::PulseClient::connect()
            && client
                .subscribe(
                    crate::audio_pulse::PA_SUBSCRIPTION_MASK_SINK
                        | crate::audio_pulse::PA_SUBSCRIPTION_MASK_SERVER,
                )
                .is_ok()
        {
            while !stop.load(Ordering::Acquire) {
                match client.read_packet_with_timeout(Duration::from_millis(500)) {
                    Ok(body) => {
                        if body.len() >= 5 {
                            let mut r = crate::audio_pulse::TagReader::new(&body);
                            if let Ok(cmd) = r.get_u32()
                                && cmd == crate::audio_pulse::PA_COMMAND_SUBSCRIBE_EVENT
                            {
                                on_signal(SystemSignalMask::AUDIO);
                            }
                        }
                    }
                    Err(crate::audio_pulse::PulseError::Io(e))
                        if e.kind() == std::io::ErrorKind::TimedOut
                            || e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => {
                        break;
                    }
                }
            }
        }
        for _ in 0..20 {
            if stop.load(Ordering::Acquire) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_upower_networkmanager_and_bluez_signal_paths() {
        assert_eq!(
            classify_system_signal_path("/org/freedesktop/UPower"),
            SystemSignalMask::POWER
        );
        assert_eq!(
            classify_system_signal_path("/org/freedesktop/UPower/devices/DisplayDevice"),
            SystemSignalMask::POWER
        );
        assert_eq!(
            classify_system_signal_path("/org/freedesktop/NetworkManager/ActiveConnection/3"),
            SystemSignalMask::NETWORK
        );
        assert_eq!(
            classify_system_signal_path("/org/freedesktop/NetworkManager/Devices/2"),
            SystemSignalMask::NETWORK
        );
        assert_eq!(
            classify_system_signal_path("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"),
            SystemSignalMask::BLUETOOTH
        );
        assert_eq!(
            classify_system_signal_path("/org/freedesktop/DBus"),
            SystemSignalMask::EMPTY
        );

        let combined = SystemSignalMask::POWER
            .union(SystemSignalMask::NETWORK)
            .union(SystemSignalMask::AUDIO);
        assert!(combined.has_power());
        assert!(combined.has_network());
        assert!(!combined.has_bluetooth());
        assert!(combined.has_audio());
        assert_eq!(SystemSignalMask::from_bits(combined.bits()), combined);
    }
}
