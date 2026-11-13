//! `systemd-logind` System D-Bus integration (`RUSTYOS-056`, `ROADMAP.md` Phase 7).
//!
//! Subscribes to `org.freedesktop.login1.Manager.PrepareForSleep` (with a `delay`
//! sleep inhibitor FD held briefly until the Lock Screen engages) and session
//! `Lock` / `Unlock` signals on a dedicated background thread so the compositor
//! event loop never blocks.

use std::fmt;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, fdo};
use zbus::zvariant::{OwnedFd, OwnedObjectPath};

const LOGIN1_DEST: &str = "org.freedesktop.login1";
const LOGIN1_MANAGER_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_MANAGER_IFACE: &str = "org.freedesktop.login1.Manager";
const LOGIN1_SESSION_IFACE: &str = "org.freedesktop.login1.Session";
const SLEEP_ACK_TIMEOUT: Duration = Duration::from_millis(1200);

/// Session lifecycle events emitted by `systemd-logind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogindEvent {
    /// Emitted before suspend (`true`) and after resume (`false`).
    PrepareForSleep {
        /// `true` right before system sleep starts; `false` right after resume.
        sleep_starting: bool,
    },
    /// External request to lock this graphical session (`loginctl lock-session`).
    SessionLock,
    /// External request to unlock this graphical session (`loginctl unlock-session`).
    SessionUnlock,
}

/// Resolves the `org.freedesktop.login1.Session` D-Bus object path for the current process.
#[must_use]
pub fn resolve_current_session_path(conn: &Connection) -> Option<OwnedObjectPath> {
    if let Ok(reply) = conn.call_method(
        Some(LOGIN1_DEST),
        LOGIN1_MANAGER_PATH,
        Some(LOGIN1_MANAGER_IFACE),
        "GetSessionByPID",
        &(std::process::id(),),
    ) && let Ok(path) = reply.body().deserialize::<OwnedObjectPath>()
    {
        return Some(path);
    }

    if let Ok(session_id) = std::env::var("XDG_SESSION_ID") {
        let trimmed = session_id.trim();
        if !trimmed.is_empty()
            && let Ok(reply) = conn.call_method(
                Some(LOGIN1_DEST),
                LOGIN1_MANAGER_PATH,
                Some(LOGIN1_MANAGER_IFACE),
                "GetSession",
                &(trimmed,),
            )
            && let Ok(path) = reply.body().deserialize::<OwnedObjectPath>()
        {
            return Some(path);
        }
    }

    conn.call_method(
        Some(LOGIN1_DEST),
        LOGIN1_MANAGER_PATH,
        Some(LOGIN1_MANAGER_IFACE),
        "GetSession",
        &("auto",),
    )
    .ok()
    .and_then(|reply| reply.body().deserialize::<OwnedObjectPath>().ok())
}

/// Resolves the active `systemd-logind` session ID string (e.g. `"2"`) via D-Bus or `$XDG_SESSION_ID`.
#[must_use]
pub fn resolve_current_session_id(conn: &Connection) -> Option<String> {
    if let Some(session_path) = resolve_current_session_path(conn)
        && let Ok(iface) = zbus::names::InterfaceName::from_static_str(LOGIN1_SESSION_IFACE)
        && let Ok(props_builder) = zbus::blocking::fdo::PropertiesProxy::builder(conn)
            .destination(LOGIN1_DEST)
            .and_then(|b| b.path(session_path.as_ref()))
        && let Ok(props) = props_builder.build()
        && let Ok(val) = props.get(iface, "Id")
        && let Ok(id) = String::try_from(val)
    {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    std::env::var("XDG_SESSION_ID")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Pure signal classifier mapping `login1` D-Bus header fields + payload to [`LogindEvent`].
#[must_use]
pub fn classify_logind_signal(
    iface: Option<&str>,
    member: Option<&str>,
    path: Option<&str>,
    expected_session_path: Option<&str>,
    prepare_for_sleep_arg: Option<bool>,
) -> Option<LogindEvent> {
    match (iface, member) {
        (Some(LOGIN1_MANAGER_IFACE), Some("PrepareForSleep")) => prepare_for_sleep_arg
            .map(|sleep_starting| LogindEvent::PrepareForSleep { sleep_starting }),
        (Some(LOGIN1_SESSION_IFACE), Some("Lock"))
            if expected_session_path.is_some_and(|expected| path == Some(expected)) =>
        {
            Some(LogindEvent::SessionLock)
        }
        (Some(LOGIN1_SESSION_IFACE), Some("Unlock"))
            if expected_session_path.is_some_and(|expected| path == Some(expected)) =>
        {
            Some(LogindEvent::SessionUnlock)
        }
        _ => None,
    }
}

/// Notifies `systemd-logind` of this session's current screen lock state when supported.
pub fn notify_session_locked_hint(locked: bool) {
    let Ok(conn) = Connection::system() else {
        return;
    };
    notify_locked_hint_with_conn(&conn, locked);
}

fn notify_locked_hint_with_conn(conn: &Connection, locked: bool) {
    let Some(session_path) = resolve_current_session_path(conn) else {
        return;
    };
    let _ = conn.call_method(
        Some(LOGIN1_DEST),
        session_path.as_ref(),
        Some(LOGIN1_SESSION_IFACE),
        "SetLockedHint",
        &(locked,),
    );
}

fn acquire_sleep_delay_inhibitor(conn: &Connection) -> Option<OwnedFd> {
    conn.call_method(
        Some(LOGIN1_DEST),
        LOGIN1_MANAGER_PATH,
        Some(LOGIN1_MANAGER_IFACE),
        "Inhibit",
        &(
            "sleep",
            "RustyOS",
            "Lock screen cleanly before system sleep",
            "delay",
        ),
    )
    .ok()
    .and_then(|reply| reply.body().deserialize::<OwnedFd>().ok())
}

/// Non-blocking handle receiving `systemd-logind` sleep and lock signals.
pub struct LogindMonitorHandle {
    events_rx: Mutex<Receiver<LogindEvent>>,
    sleep_ack: Arc<(Mutex<bool>, Condvar)>,
    conn_slot: Arc<Mutex<Option<Connection>>>,
    stop_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for LogindMonitorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogindMonitorHandle")
            .field("active", &self.worker.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for LogindMonitorHandle {
    fn default() -> Self {
        let (_tx, rx) = mpsc::channel();
        Self {
            events_rx: Mutex::new(rx),
            sleep_ack: Arc::new((Mutex::new(false), Condvar::new())),
            conn_slot: Arc::new(Mutex::new(None)),
            stop_flag: Arc::new(AtomicBool::new(true)),
            worker: None,
        }
    }
}

impl Drop for LogindMonitorHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        self.ack_prepare_for_sleep();
        if let Ok(mut guard) = self.conn_slot.lock()
            && let Some(conn) = guard.take()
        {
            let _ = conn.close();
        }
    }
}

impl LogindMonitorHandle {
    /// Starts the background System D-Bus listener for `org.freedesktop.login1`.
    ///
    /// Never fails compositor startup if `systemd-logind` or System D-Bus is absent.
    #[must_use]
    pub fn start_background() -> Self {
        let (tx, rx) = mpsc::channel();
        let sleep_ack = Arc::new((Mutex::new(false), Condvar::new()));
        let conn_slot = Arc::new(Mutex::new(None));
        let stop_flag = Arc::new(AtomicBool::new(false));

        let worker_ack = Arc::clone(&sleep_ack);
        let worker_conn = Arc::clone(&conn_slot);
        let worker_stop = Arc::clone(&stop_flag);

        let worker = thread::Builder::new()
            .name("rusty-logind".to_owned())
            .spawn(move || {
                run_logind_monitor_loop(&tx, &worker_ack, &worker_conn, &worker_stop);
            })
            .ok();

        Self {
            events_rx: Mutex::new(rx),
            sleep_ack,
            conn_slot,
            stop_flag,
            worker,
        }
    }

    /// Creates a deterministic handle seeded with a custom channel (for compositor tests).
    #[must_use]
    pub fn from_receiver(events_rx: Receiver<LogindEvent>) -> Self {
        Self {
            events_rx: Mutex::new(events_rx),
            sleep_ack: Arc::new((Mutex::new(false), Condvar::new())),
            conn_slot: Arc::new(Mutex::new(None)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            worker: None,
        }
    }

    /// Non-blockingly drains queued `systemd-logind` events.
    #[must_use]
    pub fn drain_events(&self) -> Vec<LogindEvent> {
        let Ok(guard) = self.events_rx.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(ev) = guard.try_recv() {
            out.push(ev);
        }
        out
    }

    /// Releases the `delay` sleep inhibitor FD after the Lock Screen has been engaged.
    pub fn ack_prepare_for_sleep(&self) {
        let (lock, cvar) = &*self.sleep_ack;
        if let Ok(mut pending) = lock.lock() {
            *pending = false;
            cvar.notify_all();
        }
    }

    /// Asynchronously updates `org.freedesktop.login1.Session.SetLockedHint` off the event loop.
    pub fn set_locked_hint(&self, locked: bool) {
        if self.worker.is_none() || self.stop_flag.load(Ordering::Acquire) {
            return;
        }
        let conn = self.conn_slot.lock().ok().and_then(|g| g.as_ref().cloned());
        let _ = thread::Builder::new()
            .name("rusty-logind-hint".to_owned())
            .spawn(move || {
                if let Some(c) = conn {
                    notify_locked_hint_with_conn(&c, locked);
                } else {
                    notify_session_locked_hint(locked);
                }
            });
    }
}

fn add_logind_match_rules(conn: &Connection, session_path: Option<&OwnedObjectPath>) -> bool {
    let Ok(dbus) = fdo::DBusProxy::new(conn) else {
        return false;
    };

    let Ok(sleep_builder) = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(LOGIN1_DEST)
        .and_then(|b| b.interface(LOGIN1_MANAGER_IFACE))
        .and_then(|b| b.member("PrepareForSleep"))
    else {
        return false;
    };
    if dbus.add_match_rule(sleep_builder.build()).is_err() {
        return false;
    }

    if let Some(spath) = session_path {
        for member in ["Lock", "Unlock"] {
            if let Ok(builder) = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender(LOGIN1_DEST)
                .and_then(|b| b.interface(LOGIN1_SESSION_IFACE))
                .and_then(|b| b.path(spath.as_ref()))
                .and_then(|b| b.member(member))
            {
                let _ = dbus.add_match_rule(builder.build());
            }
        }
    }
    true
}

fn run_logind_monitor_loop(
    events_tx: &Sender<LogindEvent>,
    sleep_ack: &Arc<(Mutex<bool>, Condvar)>,
    conn_slot: &Arc<Mutex<Option<Connection>>>,
    stop_flag: &AtomicBool,
) {
    let Ok(conn) = Connection::system() else {
        return;
    };
    if let Ok(mut guard) = conn_slot.lock() {
        *guard = Some(conn.clone());
    }

    let session_path = resolve_current_session_path(&conn);
    if !add_logind_match_rules(&conn, session_path.as_ref()) {
        return;
    }

    let mut inhibitor_fd = acquire_sleep_delay_inhibitor(&conn);
    let expected_session = session_path
        .as_deref()
        .map(zbus::zvariant::ObjectPath::as_str);

    for msg_res in MessageIterator::from(&conn) {
        if stop_flag.load(Ordering::Acquire) {
            break;
        }
        let Ok(msg) = msg_res else {
            break;
        };
        let hdr = msg.header();
        let iface = hdr.interface().map(zbus::names::InterfaceName::as_str);
        let member = hdr.member().map(zbus::names::MemberName::as_str);
        let path = hdr.path().map(zbus::zvariant::ObjectPath::as_str);
        let sleep_arg = if member == Some("PrepareForSleep") {
            msg.body().deserialize::<bool>().ok()
        } else {
            None
        };

        let Some(event) = classify_logind_signal(iface, member, path, expected_session, sleep_arg)
        else {
            continue;
        };

        match event {
            LogindEvent::PrepareForSleep {
                sleep_starting: true,
            } => {
                let (lock, cvar) = &**sleep_ack;
                if let Ok(mut pending) = lock.lock() {
                    *pending = true;
                }
                let _ = events_tx.send(event);
                if let Ok(guard) = lock.lock() {
                    let _ = cvar.wait_timeout_while(guard, SLEEP_ACK_TIMEOUT, |pending| {
                        *pending && !stop_flag.load(Ordering::Acquire)
                    });
                }
                drop(inhibitor_fd.take());
            }
            LogindEvent::PrepareForSleep {
                sleep_starting: false,
            } => {
                if inhibitor_fd.is_none() {
                    inhibitor_fd = acquire_sleep_delay_inhibitor(&conn);
                }
                let _ = events_tx.send(event);
            }
            LogindEvent::SessionLock | LogindEvent::SessionUnlock => {
                let _ = events_tx.send(event);
            }
        }
    }

    if let Ok(mut guard) = conn_slot.lock() {
        let _ = guard.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_prepare_for_sleep_and_matching_session_lock_signals() {
        let sess = Some("/org/freedesktop/login1/session/_32");
        assert_eq!(
            classify_logind_signal(
                Some(LOGIN1_MANAGER_IFACE),
                Some("PrepareForSleep"),
                Some(LOGIN1_MANAGER_PATH),
                sess,
                Some(true),
            ),
            Some(LogindEvent::PrepareForSleep {
                sleep_starting: true
            })
        );
        assert_eq!(
            classify_logind_signal(
                Some(LOGIN1_MANAGER_IFACE),
                Some("PrepareForSleep"),
                Some(LOGIN1_MANAGER_PATH),
                sess,
                Some(false),
            ),
            Some(LogindEvent::PrepareForSleep {
                sleep_starting: false
            })
        );
        assert_eq!(
            classify_logind_signal(Some(LOGIN1_SESSION_IFACE), Some("Lock"), sess, sess, None,),
            Some(LogindEvent::SessionLock)
        );
        assert_eq!(
            classify_logind_signal(Some(LOGIN1_SESSION_IFACE), Some("Unlock"), sess, sess, None,),
            Some(LogindEvent::SessionUnlock)
        );
        // Signals for another user's session path must be ignored.
        assert_eq!(
            classify_logind_signal(
                Some(LOGIN1_SESSION_IFACE),
                Some("Lock"),
                Some("/org/freedesktop/login1/session/_99"),
                sess,
                None,
            ),
            None
        );
    }

    #[test]
    fn drain_events_and_sleep_ack_are_non_blocking() {
        let (tx, rx) = mpsc::channel();
        let handle = LogindMonitorHandle::from_receiver(rx);
        tx.send(LogindEvent::PrepareForSleep {
            sleep_starting: true,
        })
        .ok();
        tx.send(LogindEvent::SessionLock).ok();
        assert_eq!(
            handle.drain_events(),
            vec![
                LogindEvent::PrepareForSleep {
                    sleep_starting: true
                },
                LogindEvent::SessionLock,
            ]
        );
        assert_eq!(handle.drain_events(), []);
        handle.ack_prepare_for_sleep();
    }
}
