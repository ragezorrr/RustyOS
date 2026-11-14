//! Session D-Bus `org.kde.StatusNotifierWatcher` & `StatusNotifierItem` tray bridge (`RUSTYOS-032`).
//!
//! Runs on a dedicated background thread so desktop tray registrations,
//! D-Bus property queries, and click callbacks (`Activate` / `ContextMenu`)
//! never block the compositor event loop.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};
use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

const WATCHER_BUS_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_FALLBACK_BUS_NAME: &str = "org.rustyos.StatusNotifierWatcher";
const WATCHER_OBJECT_PATH: &str = "/StatusNotifierWatcher";
const SNI_INTERFACE: &str = "org.kde.StatusNotifierItem";

/// `StatusNotifierItem` attention/visibility state (`org.kde.StatusNotifierItem.Status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayStatus {
    /// Item is passive/hidden in low-priority state.
    Passive,
    /// Item is actively running and visible.
    #[default]
    Active,
    /// Item requests user attention (unread messages, warning, alert).
    NeedsAttention,
}

impl TrayStatus {
    /// Parses an SNI `Status` string (`"Passive"`, `"Active"`, `"NeedsAttention"`).
    #[must_use]
    pub fn from_sni_str(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "passive" => Self::Passive,
            "needsattention" | "needs_attention" | "attention" => Self::NeedsAttention,
            _ => Self::Active,
        }
    }
}

/// Snapshot of a single registered tray applet rendered in `TopBar` (`RUSTYOS-032`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItemView {
    /// Canonical `<bus_name><object_path>` key.
    pub key: String,
    /// SNI application identifier.
    pub id: String,
    /// Human-readable title / tooltip summary.
    pub title: String,
    /// Compact 1-2 character badge displayed inside the `TopBar` tray pill.
    pub badge: String,
    /// Optional freedesktop icon theme name (`StatusNotifierItem.IconName`).
    pub icon_name: String,
    /// Optional decoded SNI ARGB32 icon pixmap (`StatusNotifierItem.IconPixmap`).
    pub icon_pixmap: Option<crate::icons::TrayPixmap>,
    /// Current attention / active / passive state.
    pub status: TrayStatus,
}

#[derive(Debug, Clone)]
struct TrackedTrayItem {
    key: String,
    bus_name: String,
    object_path: String,
    id: String,
    title: String,
    badge: String,
    icon_name: String,
    icon_pixmap: Option<crate::icons::TrayPixmap>,
    status: TrayStatus,
    item_is_menu: bool,
}

impl TrackedTrayItem {
    fn to_view(&self) -> TrayItemView {
        TrayItemView {
            key: self.key.clone(),
            id: self.id.clone(),
            title: self.title.clone(),
            badge: self.badge.clone(),
            icon_name: self.icon_name.clone(),
            icon_pixmap: self.icon_pixmap.clone(),
            status: self.status,
        }
    }
}

#[derive(Debug, Clone)]
enum TrayWorkerCmd {
    Register {
        bus_name: String,
        object_path: String,
    },
    LeftClick {
        key: String,
        x: i32,
        y: i32,
    },
    RightClick {
        key: String,
        x: i32,
        y: i32,
    },
    MiddleClick {
        key: String,
        x: i32,
        y: i32,
    },
}

/// Non-blocking compositor handle for D-Bus system tray (`StatusNotifierItem`) applets (`RUSTYOS-032`).
#[derive(Debug)]
pub struct TrayWatcherHandle {
    updates_rx: Mutex<mpsc::Receiver<Vec<TrayItemView>>>,
    cmd_tx: mpsc::Sender<TrayWorkerCmd>,
    stop_flag: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl Default for TrayWatcherHandle {
    fn default() -> Self {
        let (cmd_tx, _cmd_rx) = mpsc::channel();
        let (_updates_tx, updates_rx) = mpsc::channel();
        Self {
            updates_rx: Mutex::new(updates_rx),
            cmd_tx,
            stop_flag: Arc::new(AtomicBool::new(true)),
            _thread: None,
        }
    }
}

impl Drop for TrayWatcherHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

impl TrayWatcherHandle {
    /// Starts the `org.kde.StatusNotifierWatcher` D-Bus service and property worker thread.
    ///
    /// Never fails compositor startup when session D-Bus is absent or sandboxed.
    #[must_use]
    pub fn start_background() -> Self {
        let (updates_tx, updates_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop_flag);
        let thread_cmd_tx = cmd_tx.clone();

        let handle = thread::Builder::new()
            .name("rusty-sni-tray".to_owned())
            .spawn(move || run_tray_loop(&thread_cmd_tx, &cmd_rx, &updates_tx, &thread_stop))
            .ok();

        Self {
            updates_rx: Mutex::new(updates_rx),
            cmd_tx,
            stop_flag,
            _thread: handle,
        }
    }

    /// Returns the newest tray item snapshot if the background thread pushed updates.
    #[must_use]
    pub fn drain_latest(&self) -> Option<Vec<TrayItemView>> {
        let Ok(guard) = self.updates_rx.lock() else {
            return None;
        };
        let mut latest = None;
        while let Ok(snapshot) = guard.try_recv() {
            latest = Some(snapshot);
        }
        latest
    }

    /// Dispatches an asynchronous left-click (`Activate` or `ContextMenu`) to a tray item.
    pub fn activate(&self, key: &str, x: i32, y: i32) {
        let _ = self.cmd_tx.send(TrayWorkerCmd::LeftClick {
            key: key.to_owned(),
            x,
            y,
        });
    }

    /// Dispatches an asynchronous right-click (`ContextMenu`) to a tray item.
    pub fn context_menu(&self, key: &str, x: i32, y: i32) {
        let _ = self.cmd_tx.send(TrayWorkerCmd::RightClick {
            key: key.to_owned(),
            x,
            y,
        });
    }

    /// Dispatches an asynchronous middle-click (`SecondaryActivate`) to a tray item.
    pub fn secondary_activate(&self, key: &str, x: i32, y: i32) {
        let _ = self.cmd_tx.send(TrayWorkerCmd::MiddleClick {
            key: key.to_owned(),
            x,
            y,
        });
    }
}

struct StatusNotifierWatcherInterface {
    cmd_tx: mpsc::Sender<TrayWorkerCmd>,
    registered_keys: Arc<Mutex<Vec<String>>>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl StatusNotifierWatcherInterface {
    #[allow(clippy::needless_pass_by_value)]
    fn register_status_notifier_item(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        service: &str,
    ) {
        let sender = header.sender().map(ToString::to_string);
        if let Some((bus_name, object_path)) =
            parse_sni_service_and_path(service, sender.as_deref())
        {
            let _ = self.cmd_tx.send(TrayWorkerCmd::Register {
                bus_name,
                object_path,
            });
        }
    }

    #[allow(clippy::unused_self)]
    fn register_status_notifier_host(&self, service: &str) {
        let _ = service;
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registered_keys
            .lock()
            .map_or_default(|guard| guard.clone())
    }

    #[zbus(property)]
    #[allow(clippy::unused_self)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    #[allow(clippy::unused_self)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

fn try_bind_watcher_bus(
    cmd_tx: &mpsc::Sender<TrayWorkerCmd>,
    registered_keys: &Arc<Mutex<Vec<String>>>,
    bus_name: &str,
) -> Result<zbus::blocking::Connection, zbus::Error> {
    let iface = StatusNotifierWatcherInterface {
        cmd_tx: cmd_tx.clone(),
        registered_keys: Arc::clone(registered_keys),
    };
    zbus::blocking::connection::Builder::session()?
        .serve_at(WATCHER_OBJECT_PATH, iface)?
        .name(bus_name)?
        .build()
}

fn seed_from_existing_watcher(
    conn: &zbus::blocking::Connection,
    cmd_tx: &mpsc::Sender<TrayWorkerCmd>,
) {
    let Ok(iface_name) = zbus::names::InterfaceName::from_static_str(WATCHER_BUS_NAME) else {
        return;
    };
    let Ok(props_builder) = zbus::blocking::fdo::PropertiesProxy::builder(conn)
        .destination(WATCHER_BUS_NAME)
        .and_then(|b| b.path(WATCHER_OBJECT_PATH))
    else {
        return;
    };
    let Ok(props) = props_builder.build() else {
        return;
    };
    let Ok(val) = props.get(iface_name, "RegisteredStatusNotifierItems") else {
        return;
    };
    let Ok(items) = Vec::<String>::try_from(val) else {
        return;
    };
    for raw in items {
        if let Some((bus_name, object_path)) = parse_sni_service_and_path(&raw, None) {
            let _ = cmd_tx.send(TrayWorkerCmd::Register {
                bus_name,
                object_path,
            });
        }
    }
}

fn run_tray_loop(
    cmd_tx: &mpsc::Sender<TrayWorkerCmd>,
    cmd_rx: &mpsc::Receiver<TrayWorkerCmd>,
    updates_tx: &mpsc::Sender<Vec<TrayItemView>>,
    stop_flag: &AtomicBool,
) {
    let registered_keys = Arc::new(Mutex::new(Vec::new()));
    let (conn, bus_name, owns_primary) = match try_bind_watcher_bus(
        cmd_tx,
        &registered_keys,
        WATCHER_BUS_NAME,
    ) {
        Ok(c) => (c, WATCHER_BUS_NAME, true),
        Err(primary_err) => {
            match try_bind_watcher_bus(cmd_tx, &registered_keys, WATCHER_FALLBACK_BUS_NAME) {
                Ok(c) => {
                    info!(
                        reason = %primary_err,
                        "Host desktop owns org.kde.StatusNotifierWatcher; listening on {WATCHER_FALLBACK_BUS_NAME}"
                    );
                    (c, WATCHER_FALLBACK_BUS_NAME, false)
                }
                Err(fallback_err) => {
                    warn!(
                        error = %fallback_err,
                        "Unable to bind D-Bus StatusNotifierWatcher; system tray disabled"
                    );
                    return;
                }
            }
        }
    };

    info!(bus_name, "Started D-Bus StatusNotifierWatcher tray thread");
    if !owns_primary {
        seed_from_existing_watcher(&conn, cmd_tx);
    }

    let mut items: BTreeMap<String, TrackedTrayItem> = BTreeMap::new();
    let mut last_sweep = Instant::now();

    while !stop_flag.load(Ordering::Relaxed) {
        let mut changed = false;
        match cmd_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(cmd) => {
                changed |= handle_worker_cmd(&conn, &mut items, cmd);
                while let Ok(next_cmd) = cmd_rx.try_recv() {
                    changed |= handle_worker_cmd(&conn, &mut items, next_cmd);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if last_sweep.elapsed() >= Duration::from_secs(2) {
            last_sweep = Instant::now();
            if !owns_primary {
                sync_with_host_watcher(&conn, &mut items, &mut changed);
            }
            changed |= prune_and_refresh_items(&conn, &mut items);
        }

        if changed {
            sync_registered_keys_and_push(&registered_keys, &items, updates_tx);
        }
    }

    drop(conn);
}

fn sync_with_host_watcher(
    conn: &zbus::blocking::Connection,
    items: &mut BTreeMap<String, TrackedTrayItem>,
    changed: &mut bool,
) {
    let Ok(iface_name) = zbus::names::InterfaceName::from_static_str(WATCHER_BUS_NAME) else {
        return;
    };
    let Ok(props_builder) = zbus::blocking::fdo::PropertiesProxy::builder(conn)
        .destination(WATCHER_BUS_NAME)
        .and_then(|b| b.path(WATCHER_OBJECT_PATH))
    else {
        return;
    };
    let Ok(props) = props_builder.build() else {
        return;
    };
    let Ok(val) = props.get(iface_name, "RegisteredStatusNotifierItems") else {
        return;
    };
    let Ok(host_items) = Vec::<String>::try_from(val) else {
        return;
    };
    for raw in host_items {
        if let Some((bus_name, object_path)) = parse_sni_service_and_path(&raw, None) {
            let key = format!("{bus_name}{object_path}");
            if !items.contains_key(&key)
                && let Some(item) = query_sni_item(conn, &bus_name, &object_path)
            {
                items.insert(key, item);
                *changed = true;
            }
        }
    }
}

fn handle_worker_cmd(
    conn: &zbus::blocking::Connection,
    items: &mut BTreeMap<String, TrackedTrayItem>,
    cmd: TrayWorkerCmd,
) -> bool {
    match cmd {
        TrayWorkerCmd::Register {
            bus_name,
            object_path,
        } => {
            let key = format!("{bus_name}{object_path}");
            if let Some(item) = query_sni_item(conn, &bus_name, &object_path) {
                let inserted = items
                    .get(&key)
                    .is_none_or(|prev| sni_item_changed(prev, &item));
                items.insert(key.clone(), item);
                emit_watcher_signal(conn, &key, true);
                debug!(service = %key, "Registered StatusNotifierItem");
                return inserted;
            }
            false
        }
        TrayWorkerCmd::LeftClick { key, x, y } => {
            invoke_item_click(conn, items, &key, ClickKind::Left, x, y)
        }
        TrayWorkerCmd::RightClick { key, x, y } => {
            invoke_item_click(conn, items, &key, ClickKind::Right, x, y)
        }
        TrayWorkerCmd::MiddleClick { key, x, y } => {
            invoke_item_click(conn, items, &key, ClickKind::Middle, x, y)
        }
    }
}

#[derive(Clone, Copy)]
enum ClickKind {
    Left,
    Right,
    Middle,
}

fn invoke_item_click(
    conn: &zbus::blocking::Connection,
    items: &mut BTreeMap<String, TrackedTrayItem>,
    key: &str,
    kind: ClickKind,
    x: i32,
    y: i32,
) -> bool {
    let Some(existing) = items.get(key).cloned() else {
        return false;
    };
    let Ok(proxy) = zbus::blocking::Proxy::new(
        conn,
        existing.bus_name.as_str(),
        existing.object_path.as_str(),
        SNI_INTERFACE,
    ) else {
        return false;
    };

    let methods: [&str; 2] = match kind {
        ClickKind::Left if existing.item_is_menu => ["ContextMenu", "Activate"],
        ClickKind::Left => ["Activate", "ContextMenu"],
        ClickKind::Right => ["ContextMenu", "SecondaryActivate"],
        ClickKind::Middle => ["SecondaryActivate", "Activate"],
    };

    let mut invoked = false;
    for method in methods {
        if proxy.call::<_, _, ()>(method, &(x, y)).is_ok() {
            invoked = true;
            break;
        }
    }

    if !invoked {
        if !bus_name_alive(conn, &existing.bus_name) {
            items.remove(key);
            emit_watcher_signal(conn, key, false);
            return true;
        }
        return false;
    }

    if let Some(refreshed) = query_sni_item(conn, &existing.bus_name, &existing.object_path) {
        let changed = sni_item_changed(&existing, &refreshed);
        items.insert(key.to_owned(), refreshed);
        return changed;
    }
    false
}

fn prune_and_refresh_items(
    conn: &zbus::blocking::Connection,
    items: &mut BTreeMap<String, TrackedTrayItem>,
) -> bool {
    let keys: Vec<(String, String, String)> = items
        .values()
        .map(|it| (it.key.clone(), it.bus_name.clone(), it.object_path.clone()))
        .collect();
    let mut changed = false;

    for (key, bus_name, object_path) in keys {
        if !bus_name_alive(conn, &bus_name) {
            items.remove(&key);
            emit_watcher_signal(conn, &key, false);
            changed = true;
            continue;
        }
        if let Some(updated) = query_sni_item(conn, &bus_name, &object_path)
            && let Some(prev) = items.get(&key)
            && sni_item_changed(prev, &updated)
        {
            items.insert(key, updated);
            changed = true;
        }
    }
    changed
}

fn sni_item_changed(prev: &TrackedTrayItem, next: &TrackedTrayItem) -> bool {
    prev.title != next.title
        || prev.badge != next.badge
        || prev.icon_name != next.icon_name
        || prev.icon_pixmap.as_ref().map(|p| p.fingerprint)
            != next.icon_pixmap.as_ref().map(|p| p.fingerprint)
        || prev.status != next.status
        || prev.item_is_menu != next.item_is_menu
}

fn bus_name_alive(conn: &zbus::blocking::Connection, bus_name: &str) -> bool {
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(conn) else {
        return true;
    };
    let Ok(name) = zbus::names::BusName::try_from(bus_name) else {
        return false;
    };
    dbus.name_has_owner(name).unwrap_or(true)
}

fn query_sni_item(
    conn: &zbus::blocking::Connection,
    bus_name: &str,
    object_path: &str,
) -> Option<TrackedTrayItem> {
    let iface_name = zbus::names::InterfaceName::from_static_str(SNI_INTERFACE).ok()?;
    let props = zbus::blocking::fdo::PropertiesProxy::builder(conn)
        .destination(bus_name)
        .ok()?
        .path(object_path)
        .ok()?
        .build()
        .ok()?;
    let all = props.get_all(iface_name).ok()?;

    let id = prop_string(&all, "Id").unwrap_or_default();
    let title = prop_string(&all, "Title").unwrap_or_default();
    let icon_name = prop_string(&all, "IconName").unwrap_or_default();
    let status_str = prop_string(&all, "Status").unwrap_or_default();
    let item_is_menu = all
        .get("ItemIsMenu")
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false);

    let key = format!("{bus_name}{object_path}");
    let display_title = if !title.is_empty() {
        title
    } else if !id.is_empty() {
        id.clone()
    } else {
        icon_name.clone()
    };
    let badge = make_tray_badge(&id, &display_title, &icon_name);
    let icon_pixmap = parse_sni_icon_pixmap(&all);

    Some(TrackedTrayItem {
        key,
        bus_name: bus_name.to_owned(),
        object_path: object_path.to_owned(),
        id,
        title: display_title,
        badge,
        icon_name,
        icon_pixmap,
        status: TrayStatus::from_sni_str(&status_str),
        item_is_menu,
    })
}

fn parse_sni_icon_pixmap(
    map: &std::collections::HashMap<String, OwnedValue>,
) -> Option<crate::icons::TrayPixmap> {
    let entries = Vec::<(i32, i32, Vec<u8>)>::try_from(map.get("IconPixmap")?.clone()).ok()?;
    let (w, h, bytes) = entries
        .into_iter()
        .filter(|(w, h, _)| *w > 0 && *h > 0)
        .min_by_key(|(w, h, _)| {
            let prefer_ge16 = i32::from(*w < 16 || *h < 16) * 1000;
            prefer_ge16 + (*w - 24).abs() + (*h - 24).abs()
        })?;
    crate::icons::TrayPixmap::from_argb_be(w, h, bytes)
}

fn prop_string(map: &std::collections::HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(|val| String::try_from(val.clone()).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn emit_watcher_signal(conn: &zbus::blocking::Connection, key: &str, registered: bool) {
    let Ok(iface_ref) = conn
        .object_server()
        .interface::<_, StatusNotifierWatcherInterface>(WATCHER_OBJECT_PATH)
    else {
        return;
    };
    let emitter = iface_ref.signal_emitter();
    let res = if registered {
        zbus::block_on(
            StatusNotifierWatcherInterface::status_notifier_item_registered(emitter, key),
        )
    } else {
        zbus::block_on(
            StatusNotifierWatcherInterface::status_notifier_item_unregistered(emitter, key),
        )
    };
    if let Err(err) = res {
        debug!(error = %err, "Failed emitting SNI watcher signal");
    }
}

fn sync_registered_keys_and_push(
    registered_keys: &Arc<Mutex<Vec<String>>>,
    items: &BTreeMap<String, TrackedTrayItem>,
    updates_tx: &mpsc::Sender<Vec<TrayItemView>>,
) {
    let keys: Vec<String> = items.keys().cloned().collect();
    if let Ok(mut guard) = registered_keys.lock() {
        *guard = keys;
    }
    let snapshot: Vec<TrayItemView> = items.values().map(TrackedTrayItem::to_view).collect();
    let _ = updates_tx.send(snapshot);
}

/// Parses the argument passed to `RegisterStatusNotifierItem(service)` into
/// `(bus_name, object_path)` according to the freedesktop/KDE SNI convention.
#[must_use]
pub fn parse_sni_service_and_path(service: &str, sender: Option<&str>) -> Option<(String, String)> {
    let trimmed = service.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('/') {
        let bus = sender?.trim();
        if bus.is_empty() {
            return None;
        }
        return Some((bus.to_owned(), trimmed.to_owned()));
    }
    if let Some((bus, path_tail)) = trimmed.split_once('/') {
        if bus.trim().is_empty() {
            return None;
        }
        let path = if path_tail.is_empty() {
            "/StatusNotifierItem".to_owned()
        } else {
            format!("/{path_tail}")
        };
        return Some((bus.trim().to_owned(), path));
    }
    Some((trimmed.to_owned(), "/StatusNotifierItem".to_owned()))
}

/// Builds a compact 2-character badge label for a system tray item (`"NM"`, `"TG"`, `"OB"`).
#[must_use]
pub fn make_tray_badge(id: &str, title: &str, icon_name: &str) -> String {
    let source = if !id.trim().is_empty() {
        id.trim()
    } else if !title.trim().is_empty() {
        title.trim()
    } else {
        icon_name.trim()
    };
    if source.is_empty() {
        return "Tr".to_owned();
    }

    let words: Vec<&str> = source
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect();
    if words.len() >= 2 {
        let mut out = String::new();
        if let Some(c1) = words[0].chars().next() {
            out.push(c1.to_ascii_uppercase());
        }
        if let Some(c2) = words[1].chars().next() {
            out.push(c2.to_ascii_uppercase());
        }
        if !out.is_empty() {
            return out;
        }
    }

    let chars: Vec<char> = source.chars().filter(|c| c.is_alphanumeric()).collect();
    if chars.len() >= 2 {
        return format!(
            "{}{}",
            chars[0].to_ascii_uppercase(),
            chars[1].to_ascii_lowercase()
        );
    }
    chars
        .first()
        .map_or_else(|| "Tr".to_owned(), |c| c.to_ascii_uppercase().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sni_bus_name_object_path_and_combined_forms() {
        assert_eq!(
            parse_sni_service_and_path("org.kde.StatusNotifierItem-123-1", Some(":1.42")),
            Some((
                "org.kde.StatusNotifierItem-123-1".to_owned(),
                "/StatusNotifierItem".to_owned()
            ))
        );
        assert_eq!(
            parse_sni_service_and_path("/org/ayatana/NotificationItem/nm_applet", Some(":1.77")),
            Some((
                ":1.77".to_owned(),
                "/org/ayatana/NotificationItem/nm_applet".to_owned()
            ))
        );
        assert_eq!(
            parse_sni_service_and_path(":1.88/StatusNotifierItem", None),
            Some((":1.88".to_owned(), "/StatusNotifierItem".to_owned()))
        );
        assert_eq!(
            parse_sni_service_and_path("/StatusNotifierItem", None),
            None
        );
        assert_eq!(parse_sni_service_and_path("", Some(":1.1")), None);
    }

    #[test]
    fn derives_compact_tray_badges_and_status_values() {
        assert_eq!(make_tray_badge("nm-applet", "Network", "nm"), "NA");
        assert_eq!(
            make_tray_badge("TelegramDesktop", "Telegram", "telegram"),
            "Te"
        );
        assert_eq!(make_tray_badge("", "OBS Studio", "obs"), "OS");
        assert_eq!(make_tray_badge("", "", ""), "Tr");

        assert_eq!(TrayStatus::from_sni_str("Passive"), TrayStatus::Passive);
        assert_eq!(
            TrayStatus::from_sni_str("NeedsAttention"),
            TrayStatus::NeedsAttention
        );
        assert_eq!(TrayStatus::from_sni_str("Active"), TrayStatus::Active);
    }
}
