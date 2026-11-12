//! Session D-Bus `org.freedesktop.Notifications` bridge (`RUSTYOS-026`).
//!
//! Owns `org.freedesktop.Notifications` on the user D-Bus session (falling back
//! to `org.rustyos.Notifications` if a host desktop notification daemon is
//! already active in nested `--winit` sessions) on a dedicated background
//! thread so the compositor event loop is never blocked.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusty_ipc::Urgency;
use tracing::{info, warn};
use zbus::interface;
use zbus::zvariant::OwnedValue;

/// Commands emitted asynchronously from the D-Bus thread into the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbusNotificationCommand {
    /// Enqueue a new desktop notification received over D-Bus.
    Notify {
        /// Freedesktop notification identifier returned to the caller.
        id: u32,
        /// Optional ID of an existing notification to replace.
        replaces_id: u32,
        /// Normalized urgency level.
        urgency: Urgency,
        /// Formatted notification title.
        title: String,
        /// Sanitized single-line notification body.
        body: String,
        /// Optional explicit expiration timeout (`None` uses compositor default).
        expire_after: Option<Duration>,
    },
    /// Close/dismiss an existing notification by ID.
    Close {
        /// Freedesktop notification identifier.
        id: u32,
    },
}

/// Handle held by the compositor to drain D-Bus notifications non-blockingly.
#[derive(Debug)]
pub struct DbusNotificationHandle {
    rx: Mutex<mpsc::Receiver<DbusNotificationCommand>>,
    stop_flag: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl Default for DbusNotificationHandle {
    fn default() -> Self {
        let (_tx, rx) = mpsc::channel();
        Self {
            rx: Mutex::new(rx),
            stop_flag: Arc::new(AtomicBool::new(true)),
            _thread: None,
        }
    }
}

impl Drop for DbusNotificationHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

impl DbusNotificationHandle {
    /// Starts the D-Bus `org.freedesktop.Notifications` listener on a background thread.
    ///
    /// Never fails compositor startup: if the session bus is absent or restricted,
    /// the background thread logs a warning and exits cleanly.
    #[must_use]
    pub fn start_background() -> Self {
        let (tx, rx) = mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop_flag);

        let handle = thread::Builder::new()
            .name("rusty-dbus-notif".to_owned())
            .spawn(move || run_dbus_server_loop(&tx, &thread_stop))
            .ok();

        Self {
            rx: Mutex::new(rx),
            stop_flag,
            _thread: handle,
        }
    }

    /// Non-blockingly drains all pending D-Bus notification commands.
    #[must_use]
    pub fn drain_commands(&self) -> Vec<DbusNotificationCommand> {
        let Ok(guard) = self.rx.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(cmd) = guard.try_recv() {
            out.push(cmd);
        }
        out
    }
}

struct NotificationsInterface {
    tx: mpsc::Sender<DbusNotificationCommand>,
    next_id: AtomicU32,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationsInterface {
    #[allow(clippy::unused_self)]
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["body", "persistence"]
    }

    #[allow(clippy::unused_self)]
    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        (
            "RustyOS Notification HUD",
            "RustyOS",
            env!("CARGO_PKG_VERSION"),
            "1.2",
        )
    }

    #[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let _ = (app_icon, actions);
        let id = if replaces_id > 0 {
            replaces_id
        } else {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        };
        let urgency = parse_dbus_urgency(hints.get("urgency"));
        let (title, clean_body) = format_dbus_notification(app_name, summary, body);
        let expire_after = parse_expire_timeout(expire_timeout);

        let _ = self.tx.send(DbusNotificationCommand::Notify {
            id,
            replaces_id,
            urgency,
            title,
            body: clean_body,
            expire_after,
        });
        id
    }

    fn close_notification(&self, id: u32) {
        let _ = self.tx.send(DbusNotificationCommand::Close { id });
    }
}

fn try_bind_notification_bus(
    tx: &mpsc::Sender<DbusNotificationCommand>,
    bus_name: &str,
) -> Result<zbus::blocking::Connection, zbus::Error> {
    let iface = NotificationsInterface {
        tx: tx.clone(),
        next_id: AtomicU32::new(100),
    };
    zbus::blocking::connection::Builder::session()?
        .serve_at("/org/freedesktop/Notifications", iface)?
        .name(bus_name)?
        .build()
}

fn run_dbus_server_loop(tx: &mpsc::Sender<DbusNotificationCommand>, stop_flag: &AtomicBool) {
    // Prefer standard `org.freedesktop.Notifications`; fall back to
    // `org.rustyos.Notifications` in nested sessions where KDE/GNOME already owns it.
    let (conn, bus_name) = match try_bind_notification_bus(tx, "org.freedesktop.Notifications") {
        Ok(c) => (c, "org.freedesktop.Notifications"),
        Err(primary_err) => match try_bind_notification_bus(tx, "org.rustyos.Notifications") {
            Ok(c) => {
                info!(
                    reason = %primary_err,
                    "Host notification daemon owns org.freedesktop.Notifications; listening on org.rustyos.Notifications"
                );
                (c, "org.rustyos.Notifications")
            }
            Err(fallback_err) => {
                warn!(
                    error = %fallback_err,
                    "Unable to bind D-Bus notification service; falling back to IPC-only HUD"
                );
                return;
            }
        },
    };

    info!(
        bus_name,
        "Started D-Bus org.freedesktop.Notifications server thread"
    );

    while !stop_flag.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(200));
    }
    drop(conn);
}

/// Maps freedesktop D-Bus `"urgency"` hint values (`0=Low`, `1=Normal`, `2=Critical`).
#[must_use]
pub fn parse_dbus_urgency(hint: Option<&OwnedValue>) -> Urgency {
    let Some(val) = hint else {
        return Urgency::Normal;
    };
    let level = u8::try_from(val)
        .ok()
        .or_else(|| u32::try_from(val).ok().and_then(|v| u8::try_from(v).ok()));
    match level {
        Some(0) => Urgency::Low,
        Some(2) => Urgency::Critical,
        _ => Urgency::Normal,
    }
}

/// Converts freedesktop `expire_timeout` milliseconds (`-1` => default TTL, `0` => persistent/long TTL).
#[must_use]
pub fn parse_expire_timeout(expire_timeout_ms: i32) -> Option<Duration> {
    match expire_timeout_ms.cmp(&0) {
        std::cmp::Ordering::Greater => {
            let clamped_ms = u64::try_from(expire_timeout_ms)
                .unwrap_or(10_000)
                .clamp(1_000, 60_000);
            Some(Duration::from_millis(clamped_ms))
        }
        std::cmp::Ordering::Equal => {
            // Explicit 0 means "do not auto-expire quickly"; cap cleanly at 5 minutes.
            Some(Duration::from_secs(300))
        }
        std::cmp::Ordering::Less => None,
    }
}

/// Normalizes D-Bus `(app_name, summary, body)` into concise single-line strings for the HUD.
#[must_use]
pub fn format_dbus_notification(app_name: &str, summary: &str, body: &str) -> (String, String) {
    let app = sanitize_single_line(app_name);
    let sum = sanitize_single_line(summary);
    let clean_body = sanitize_single_line(body);

    let title = match (app.is_empty(), sum.is_empty()) {
        (true, true) => "Notification".to_owned(),
        (false, true) => app,
        (true, false) => sum,
        (false, false) => {
            if sum
                .to_ascii_lowercase()
                .starts_with(&app.to_ascii_lowercase())
            {
                sum
            } else {
                format!("{app}: {sum}")
            }
        }
    };
    (title, clean_body)
}

fn sanitize_single_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbus_urgency_and_timeout_mapping_handle_valid_and_missing_hints() {
        let low = OwnedValue::from(0_u8);
        let critical = OwnedValue::from(2_u8);
        let invalid = OwnedValue::from(1.5_f64);

        assert_eq!(parse_dbus_urgency(None), Urgency::Normal);
        assert_eq!(parse_dbus_urgency(Some(&low)), Urgency::Low);
        assert_eq!(parse_dbus_urgency(Some(&critical)), Urgency::Critical);
        assert_eq!(parse_dbus_urgency(Some(&invalid)), Urgency::Normal);

        assert_eq!(parse_expire_timeout(-1), None);
        assert_eq!(parse_expire_timeout(0), Some(Duration::from_secs(300)));
        assert_eq!(parse_expire_timeout(250), Some(Duration::from_millis(1000)));
        assert_eq!(
            parse_expire_timeout(4500),
            Some(Duration::from_millis(4500))
        );
    }

    #[test]
    fn dbus_notification_formatting_combines_app_summary_and_sanitizes_newlines() {
        let (title, body) =
            format_dbus_notification("Spotify", "Now Playing", "Track\nOne\t\tArtist");
        assert_eq!(title, "Spotify: Now Playing");
        assert_eq!(body, "Track One Artist");

        let (dedup_title, _) = format_dbus_notification("Steam", "Steam Update", "Ready");
        assert_eq!(dedup_title, "Steam Update");

        let (fallback_title, _) = format_dbus_notification("", "", "Empty");
        assert_eq!(fallback_title, "Notification");
    }
}
