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
use smithay::reexports::calloop::ping::Ping;
use tracing::{info, warn};
use zbus::blocking::{MessageIterator, fdo};
use zbus::interface;
use zbus::object_server::SignalEmitter;
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
        /// Optional canonical Dock application key derived from `desktop-entry` hint or `app_name` (`RUSTYOS-162`).
        app_key: Option<String>,
    },
    /// Close/dismiss an existing notification by ID.
    Close {
        /// Freedesktop notification identifier.
        id: u32,
    },
    /// Explicit Dock unread counter update received via `com.canonical.Unity.LauncherEntry::Update` (`RUSTYOS-162`).
    LauncherBadgeUpdate {
        /// Canonical Dock application key (`"firefox"`, `"org.telegram.desktop"`, ...).
        app_key: String,
        /// Requested unread counter (`0` clears the badge).
        count: u32,
        /// Whether the client requests displaying the counter badge.
        visible: bool,
    },
}

/// Outbound Freedesktop D-Bus signals emitted back to notification clients.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DbusNotificationSignal {
    Closed { id: u32, reason: u32 },
    ActionInvoked { id: u32, action_key: String },
}

/// Handle held by the compositor to drain D-Bus notifications non-blockingly.
#[derive(Debug)]
pub struct DbusNotificationHandle {
    rx: Mutex<mpsc::Receiver<DbusNotificationCommand>>,
    signal_tx: mpsc::Sender<DbusNotificationSignal>,
    stop_flag: Arc<AtomicBool>,
    waker: Arc<Mutex<Option<Ping>>>,
    _thread: Option<JoinHandle<()>>,
}

impl Default for DbusNotificationHandle {
    fn default() -> Self {
        let (_tx, rx) = mpsc::channel();
        let (signal_tx, _signal_rx) = mpsc::channel();
        Self {
            rx: Mutex::new(rx),
            signal_tx,
            stop_flag: Arc::new(AtomicBool::new(true)),
            waker: Arc::new(Mutex::new(None)),
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
        let (signal_tx, signal_rx) = mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop_flag);
        let waker: Arc<Mutex<Option<Ping>>> = Arc::new(Mutex::new(None));
        let thread_waker = Arc::clone(&waker);
        let thread_signal_tx = signal_tx.clone();

        let handle = thread::Builder::new()
            .name("rusty-dbus-notif".to_owned())
            .spawn(move || {
                run_dbus_server_loop(
                    &tx,
                    &thread_signal_tx,
                    &signal_rx,
                    &thread_stop,
                    &thread_waker,
                );
            })
            .ok();

        Self {
            rx: Mutex::new(rx),
            signal_tx,
            stop_flag,
            waker,
            _thread: handle,
        }
    }

    /// Registers the compositor event-loop waker so incoming D-Bus notifications trigger an immediate redraw.
    pub(crate) fn set_waker(&self, ping: Ping) {
        if let Ok(mut guard) = self.waker.lock() {
            *guard = Some(ping);
        }
    }

    /// Emits `org.freedesktop.Notifications.NotificationClosed(id, reason)` asynchronously.
    /// Standard Freedesktop reasons: `1` = expired, `2` = dismissed by user, `3` = closed by call.
    pub fn emit_closed(&self, id: u32, reason: u32) {
        let _ = self
            .signal_tx
            .send(DbusNotificationSignal::Closed { id, reason });
    }

    /// Emits `org.freedesktop.Notifications.ActionInvoked(id, action_key)` asynchronously.
    pub fn emit_action_invoked(&self, id: u32, action_key: &str) {
        let _ = self.signal_tx.send(DbusNotificationSignal::ActionInvoked {
            id,
            action_key: action_key.to_owned(),
        });
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
    signal_tx: mpsc::Sender<DbusNotificationSignal>,
    next_id: AtomicU32,
    waker: Arc<Mutex<Option<Ping>>>,
}

impl NotificationsInterface {
    fn wake_compositor(&self) {
        if let Ok(guard) = self.waker.lock()
            && let Some(ping) = guard.as_ref()
        {
            ping.ping();
        }
    }
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationsInterface {
    #[allow(clippy::unused_self)]
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec![
            "actions",
            "body",
            "body-markup",
            "icon-static",
            "persistence",
        ]
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
        let desktop_entry = hints
            .get("desktop-entry")
            .and_then(|v| <&str>::try_from(v).ok());
        let app_key = resolve_notification_app_key(desktop_entry, app_name);
        let (title, clean_body) = format_dbus_notification(app_name, summary, body);
        let expire_after = parse_expire_timeout(expire_timeout);

        let _ = self.tx.send(DbusNotificationCommand::Notify {
            id,
            replaces_id,
            urgency,
            title,
            body: clean_body,
            expire_after,
            app_key,
        });
        info!(id, app_name, "Received D-Bus notification");
        self.wake_compositor();
        id
    }

    fn close_notification(&self, id: u32) {
        let _ = self.tx.send(DbusNotificationCommand::Close { id });
        let _ = self
            .signal_tx
            .send(DbusNotificationSignal::Closed { id, reason: 3 });
        self.wake_compositor();
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

fn try_bind_notification_bus(
    tx: &mpsc::Sender<DbusNotificationCommand>,
    signal_tx: &mpsc::Sender<DbusNotificationSignal>,
    waker: &Arc<Mutex<Option<Ping>>>,
    bus_name: &str,
) -> Result<zbus::blocking::Connection, zbus::Error> {
    let iface = NotificationsInterface {
        tx: tx.clone(),
        signal_tx: signal_tx.clone(),
        next_id: AtomicU32::new(100),
        waker: Arc::clone(waker),
    };
    let conn = zbus::blocking::connection::Builder::session()?
        .serve_at("/org/freedesktop/Notifications", iface)?
        .build()?;
    let reply =
        conn.request_name_with_flags(bus_name, zbus::fdo::RequestNameFlags::DoNotQueue.into())?;
    if !matches!(
        reply,
        zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner
    ) {
        return Err(zbus::Error::Failure(format!(
            "D-Bus name '{bus_name}' is already owned ({reply:?})"
        )));
    }
    Ok(conn)
}

fn emit_outbound_signal(conn: &zbus::blocking::Connection, sig: DbusNotificationSignal) {
    let Ok(iface_ref) = conn
        .object_server()
        .interface::<_, NotificationsInterface>("/org/freedesktop/Notifications")
    else {
        return;
    };
    let emitter = iface_ref.signal_emitter();
    let _ = zbus::block_on(async move {
        match sig {
            DbusNotificationSignal::Closed { id, reason } => {
                NotificationsInterface::notification_closed(emitter, id, reason).await
            }
            DbusNotificationSignal::ActionInvoked { id, action_key } => {
                NotificationsInterface::action_invoked(emitter, id, &action_key).await
            }
        }
    });
}

fn run_dbus_server_loop(
    tx: &mpsc::Sender<DbusNotificationCommand>,
    signal_tx: &mpsc::Sender<DbusNotificationSignal>,
    signal_rx: &mpsc::Receiver<DbusNotificationSignal>,
    stop_flag: &AtomicBool,
    waker: &Arc<Mutex<Option<Ping>>>,
) {
    // Prefer standard `org.freedesktop.Notifications`; fall back to
    // `org.rustyos.Notifications` in nested sessions where KDE/GNOME already owns it.
    let (conn, bus_name) = match try_bind_notification_bus(
        tx,
        signal_tx,
        waker,
        "org.freedesktop.Notifications",
    ) {
        Ok(c) => (c, "org.freedesktop.Notifications"),
        Err(primary_err) => {
            match try_bind_notification_bus(tx, signal_tx, waker, "org.rustyos.Notifications") {
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
            }
        }
    };

    info!(
        bus_name,
        "Started D-Bus org.freedesktop.Notifications server thread"
    );

    spawn_unity_launcher_badge_listener(&conn, tx.clone(), Arc::clone(waker));

    while !stop_flag.load(Ordering::Relaxed) {
        match signal_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(sig) => {
                emit_outbound_signal(&conn, sig);
                while let Ok(more) = signal_rx.try_recv() {
                    emit_outbound_signal(&conn, more);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(conn);
}

fn spawn_unity_launcher_badge_listener(
    conn: &zbus::blocking::Connection,
    tx: mpsc::Sender<DbusNotificationCommand>,
    waker: Arc<Mutex<Option<Ping>>>,
) {
    let Ok(rule) = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("com.canonical.Unity.LauncherEntry")
        .and_then(|b| b.member("Update"))
        .map(zbus::match_rule::Builder::build)
    else {
        return;
    };
    let Ok(dbus) = fdo::DBusProxy::new(conn) else {
        return;
    };
    if dbus.add_match_rule(rule.clone()).is_err() {
        return;
    }
    let Ok(iter) = MessageIterator::for_match_rule(rule, conn, Some(64)) else {
        return;
    };
    let _ = thread::Builder::new()
        .name("rusty-unity-badges".to_owned())
        .spawn(move || {
            for msg_res in iter {
                let Ok(msg) = msg_res else {
                    break;
                };
                if let Ok((app_uri, props)) = msg
                    .body()
                    .deserialize::<(String, HashMap<String, OwnedValue>)>()
                    && let Some((app_key, count, visible)) =
                        parse_unity_launcher_update(&app_uri, &props)
                {
                    if tx
                        .send(DbusNotificationCommand::LauncherBadgeUpdate {
                            app_key,
                            count,
                            visible,
                        })
                        .is_err()
                    {
                        break;
                    }
                    if let Ok(guard) = waker.lock()
                        && let Some(ping) = guard.as_ref()
                    {
                        ping.ping();
                    }
                }
            }
        });
}

/// Maps a `com.canonical.Unity.LauncherEntry` URI (e.g. `application://org.mozilla.firefox.desktop`)
/// into a canonical RustyOS Dock application key (`RUSTYOS-162`).
#[must_use]
pub fn parse_unity_launcher_app_key(app_uri: &str) -> Option<String> {
    let trimmed = app_uri.trim();
    if trimmed.is_empty() {
        return None;
    }
    let raw = trimmed
        .strip_prefix("application://")
        .unwrap_or(trimmed)
        .trim();
    let stem = raw.strip_suffix(".desktop").unwrap_or(raw).trim();
    if stem.is_empty() {
        return None;
    }
    Some(crate::shell_render::canonical_dock_app_key(
        Some(stem),
        stem,
        rusty_wm::WindowId(0),
    ))
}

/// Parses a `com.canonical.Unity.LauncherEntry::Update(app_uri, properties)` payload into
/// `(app_key, count, visible)` (`RUSTYOS-162`).
#[must_use]
pub fn parse_unity_launcher_update<S: std::hash::BuildHasher>(
    app_uri: &str,
    props: &HashMap<String, OwnedValue, S>,
) -> Option<(String, u32, bool)> {
    let app_key = parse_unity_launcher_app_key(app_uri)?;
    let visible_prop = props
        .get("count-visible")
        .and_then(|v| bool::try_from(v).ok());
    let count_prop = props.get("count").and_then(|v| {
        i64::try_from(v)
            .ok()
            .map(|n| u32::try_from(n.clamp(0, 9999)).unwrap_or(0))
            .or_else(|| {
                u64::try_from(v)
                    .ok()
                    .map(|n| u32::try_from(n.min(9999)).unwrap_or(9999))
            })
            .or_else(|| {
                i32::try_from(v)
                    .ok()
                    .map(|n| u32::try_from(n.clamp(0, 9999)).unwrap_or(0))
            })
            .or_else(|| u32::try_from(v).ok().map(|n| n.min(9999)))
    });
    if visible_prop.is_none() && count_prop.is_none() {
        return None;
    }
    let count = count_prop.unwrap_or(0);
    let visible = match visible_prop {
        Some(false) => false,
        Some(true) | None => count > 0,
    };
    Some((app_key, if visible { count } else { 0 }, visible))
}

/// Resolves a canonical Dock application key from D-Bus notification `desktop-entry` hint or `app_name` (`RUSTYOS-162`).
#[must_use]
pub fn resolve_notification_app_key(desktop_entry: Option<&str>, app_name: &str) -> Option<String> {
    let entry = desktop_entry.map(str::trim).filter(|s| !s.is_empty());
    let clean_name = sanitize_single_line(app_name);
    if let Some(de) = entry {
        let stem = de.strip_suffix(".desktop").unwrap_or(de).trim();
        if !stem.is_empty() {
            return Some(crate::shell_render::canonical_dock_app_key(
                Some(stem),
                if clean_name.is_empty() {
                    stem
                } else {
                    &clean_name
                },
                rusty_wm::WindowId(0),
            ));
        }
    }
    if clean_name.is_empty() {
        return None;
    }
    let lower = clean_name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "notify-send" | "notification" | "system" | "rustyos" | "rusty-compositor"
    ) {
        return None;
    }
    Some(crate::shell_render::canonical_dock_app_key(
        Some(&clean_name),
        &clean_name,
        rusty_wm::WindowId(0),
    ))
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

fn is_unsupported_emoji_char(ch: char) -> bool {
    let cp = ch as u32;
    cp >= 0x1_F000 || matches!(cp, 0x200D | 0x20E3 | 0x2600..=0x27BF | 0xFE0E | 0xFE0F)
}

fn sanitize_single_line(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .filter(|&c| !is_unsupported_emoji_char(c))
        .collect();
    filtered.split_whitespace().collect::<Vec<_>>().join(" ")
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

        let (_, clean_emoji) = format_dbus_notification(
            "Google Chrome",
            "Chat приветствует вас!",
            "chat.google.com\nТак выглядят уведомления на рабочем столе 👍",
        );
        assert_eq!(
            clean_emoji,
            "chat.google.com Так выглядят уведомления на рабочем столе"
        );
    }

    #[test]
    fn unity_launcher_entry_and_notification_app_key_parsers_normalize_badges() {
        let mut props = HashMap::new();
        props.insert("count".to_owned(), OwnedValue::from(7_i64));
        props.insert("count-visible".to_owned(), OwnedValue::from(true));
        assert_eq!(
            parse_unity_launcher_update("application://org.mozilla.firefox.desktop", &props),
            Some(("firefox".to_owned(), 7, true))
        );

        let mut hide_props = HashMap::new();
        hide_props.insert("count".to_owned(), OwnedValue::from(7_i64));
        hide_props.insert("count-visible".to_owned(), OwnedValue::from(false));
        let expected_telegram_key = crate::shell_render::canonical_dock_app_key(
            Some("org.telegram.desktop"),
            "Telegram",
            rusty_wm::WindowId(1),
        );
        assert_eq!(
            parse_unity_launcher_update("application://org.telegram.desktop.desktop", &hide_props),
            Some((expected_telegram_key, 0, false))
        );

        assert_eq!(
            resolve_notification_app_key(Some("org.mozilla.firefox"), "Firefox"),
            Some("firefox".to_owned())
        );
        assert_eq!(resolve_notification_app_key(None, "notify-send"), None);
    }
}
