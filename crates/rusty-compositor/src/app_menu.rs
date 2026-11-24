//! Global application menu D-Bus registrar (`com.canonical.AppMenu.Registrar`) and
//! `com.canonical.dbusmenu` / `org.gtk.Menus` background client (`RUSTYOS-143`, ADR-0082).
//!
//! Runs strictly on a dedicated background thread so D-Bus I/O never blocks the
//! compositor event loop. When the focused application does not export a D-Bus menu,
//! the `TopBar` falls back cleanly to static `TopBarAppProfile` entries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// Single leaf item or separator inside an application's exported top-level menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMenuItem {
    /// Protocol-assigned item identifier used for `com.canonical.dbusmenu.Event("clicked")`.
    pub id: i32,
    /// Display label with mnemonic underscores stripped.
    pub label: String,
    /// Optional keyboard shortcut hint (e.g. `"Ctrl+S"`).
    pub shortcut: Option<String>,
    /// Whether the item is enabled.
    pub enabled: bool,
    /// True when this entry renders as a visual horizontal separator.
    pub separator: bool,
    /// Optional toggle state (`Some(true)` = checked, `Some(false)` = unchecked).
    pub checked: Option<bool>,
}

/// One top-level menu column in the `TopBar` (e.g. `File`, `Edit`, `View`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMenuSection {
    /// Protocol-assigned root menu item id.
    pub id: i32,
    /// Clean label shown in the `TopBar` (e.g. `"File"`).
    pub title: String,
    /// Child entries displayed in the dropdown popover.
    pub items: Vec<AppMenuItem>,
}

/// Complete exported menu tree for a single registered application window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMenuTree {
    /// D-Bus unique service name owning the menu (e.g. `":1.84"`).
    pub service_name: String,
    /// D-Bus object path implementing `com.canonical.dbusmenu`.
    pub object_path: String,
    /// Top-level menu sections shown in the `TopBar`.
    pub sections: Vec<AppMenuSection>,
}

#[derive(Debug)]
enum AppMenuCommand {
    RefreshWindow {
        window_id: u32,
    },
    ActivateItem {
        service_name: String,
        object_path: String,
        item_id: i32,
    },
}

#[derive(Default)]
struct AppMenuRegistryState {
    /// Maps X11/Wayland registration window IDs to `(service_name, object_path)`.
    registrations: HashMap<u32, (String, String)>,
    /// Cached parsed menu trees keyed by registration window ID.
    trees: HashMap<u32, AppMenuTree>,
}

/// Non-blocking compositor handle to the background `AppMenu` registrar service.
#[derive(Clone)]
pub struct AppMenuHandle {
    state: Arc<Mutex<AppMenuRegistryState>>,
    cmd_tx: Option<mpsc::Sender<AppMenuCommand>>,
}

impl std::fmt::Debug for AppMenuHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppMenuHandle")
            .field("active", &self.cmd_tx.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for AppMenuHandle {
    fn default() -> Self {
        Self::noop()
    }
}

impl AppMenuHandle {
    /// Creates an inert handle for tests or headless sessions without D-Bus.
    #[must_use]
    pub fn noop() -> Self {
        Self {
            state: Arc::new(Mutex::new(AppMenuRegistryState::default())),
            cmd_tx: None,
        }
    }

    /// Spawns the background `com.canonical.AppMenu.Registrar` D-Bus thread.
    ///
    /// Never fails or blocks the compositor: if the session bus is unavailable or
    /// another desktop owns the name, the handle gracefully degrades to a no-op.
    #[must_use]
    pub fn spawn() -> Self {
        let state = Arc::new(Mutex::new(AppMenuRegistryState::default()));
        let (tx, rx) = mpsc::channel::<AppMenuCommand>();
        let worker_state = Arc::clone(&state);
        let worker_tx = tx.clone();

        let _ = thread::Builder::new()
            .name("rusty-appmenu".to_owned())
            .spawn(move || run_appmenu_worker(&worker_state, worker_tx, &rx));

        Self {
            state,
            cmd_tx: Some(tx),
        }
    }

    /// Returns a cloned snapshot of the exported menu tree for `window_id`, if available.
    ///
    /// Uses `try_lock` so the compositor render path never blocks.
    #[must_use]
    pub fn menu_for_window(&self, window_id: u32) -> Option<AppMenuTree> {
        self.state
            .try_lock()
            .ok()
            .and_then(|guard| guard.trees.get(&window_id).cloned())
    }

    /// Requests an asynchronous refresh of the menu tree for `window_id`.
    pub fn request_refresh(&self, window_id: u32) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(AppMenuCommand::RefreshWindow { window_id });
        }
    }

    /// Dispatches a `com.canonical.dbusmenu.Event("clicked")` call asynchronously.
    pub fn activate_item(&self, service_name: String, object_path: String, item_id: i32) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(AppMenuCommand::ActivateItem {
                service_name,
                object_path,
                item_id,
            });
        }
    }

    /// Registers a synthetic menu tree directly (used by unit tests).
    #[cfg(test)]
    pub fn insert_test_menu(&self, window_id: u32, tree: AppMenuTree) {
        if let Ok(mut guard) = self.state.lock() {
            guard.trees.insert(window_id, tree);
        }
    }
}

struct AppMenuRegistrarInterface {
    state: Arc<Mutex<AppMenuRegistryState>>,
    cmd_tx: mpsc::Sender<AppMenuCommand>,
}

#[zbus::interface(name = "com.canonical.AppMenu.Registrar")]
impl AppMenuRegistrarInterface {
    #[allow(clippy::needless_pass_by_value)]
    fn register_window(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        window_id: u32,
        menu_object_path: OwnedObjectPath,
    ) {
        let sender = header.sender().map_or_default(|s| s.as_str().to_owned());
        if sender.is_empty() {
            return;
        }
        let path = menu_object_path.as_str().to_owned();
        if let Ok(mut guard) = self.state.lock() {
            guard.registrations.insert(window_id, (sender, path));
        }
        let _ = self
            .cmd_tx
            .send(AppMenuCommand::RefreshWindow { window_id });
    }

    fn unregister_window(&self, window_id: u32) {
        if let Ok(mut guard) = self.state.lock() {
            guard.registrations.remove(&window_id);
            guard.trees.remove(&window_id);
        }
    }

    fn get_menu_for_window(&self, window_id: u32) -> (String, OwnedObjectPath) {
        if let Ok(guard) = self.state.lock()
            && let Some((svc, path)) = guard.registrations.get(&window_id)
            && let Ok(obj) = OwnedObjectPath::try_from(path.as_str())
        {
            return (svc.clone(), obj);
        }
        (
            String::new(),
            OwnedObjectPath::try_from("/").unwrap_or_else(|_| unreachable!()),
        )
    }
}

fn run_appmenu_worker(
    state: &Arc<Mutex<AppMenuRegistryState>>,
    cmd_tx: mpsc::Sender<AppMenuCommand>,
    rx: &mpsc::Receiver<AppMenuCommand>,
) {
    let Ok(conn) = Connection::session() else {
        tracing::debug!("AppMenu registrar disabled: no session D-Bus");
        return;
    };
    let iface = AppMenuRegistrarInterface {
        state: Arc::clone(state),
        cmd_tx,
    };
    if conn
        .object_server()
        .at("/com/canonical/AppMenu/Registrar", iface)
        .is_err()
    {
        return;
    }
    if conn
        .request_name("com.canonical.AppMenu.Registrar")
        .is_err()
    {
        tracing::info!("Another desktop owns com.canonical.AppMenu.Registrar; using profile menus");
        return;
    }
    tracing::info!("Started D-Bus com.canonical.AppMenu.Registrar background service");

    while let Ok(cmd) = rx.recv() {
        match cmd {
            AppMenuCommand::RefreshWindow { window_id } => {
                let target = state
                    .lock()
                    .ok()
                    .and_then(|guard| guard.registrations.get(&window_id).cloned());
                let Some((service_name, object_path)) = target else {
                    continue;
                };
                if let Some(tree) = fetch_dbusmenu_layout(&conn, &service_name, &object_path)
                    && let Ok(mut guard) = state.lock()
                {
                    guard.trees.insert(window_id, tree);
                }
            }
            AppMenuCommand::ActivateItem {
                service_name,
                object_path,
                item_id,
            } => {
                let _ = conn.call_method(
                    Some(service_name.as_str()),
                    object_path.as_str(),
                    Some("com.canonical.dbusmenu"),
                    "Event",
                    &(item_id, "clicked", Value::from(""), 0_u32),
                );
            }
        }
    }
}

/// Strips GTK/Qt mnemonic underscores (`"_File"` -> `"File"`, `"A__B"` -> `"A_B"`).
#[must_use]
pub fn strip_menu_mnemonic(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                let _ = chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    out.trim().to_owned()
}

type RawDbusMenuNode = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

fn fetch_dbusmenu_layout(
    conn: &Connection,
    service_name: &str,
    object_path: &str,
) -> Option<AppMenuTree> {
    let props: Vec<&str> = vec![
        "label",
        "enabled",
        "visible",
        "type",
        "toggle-type",
        "toggle-state",
        "shortcut",
    ];
    let reply = conn
        .call_method(
            Some(service_name),
            object_path,
            Some("com.canonical.dbusmenu"),
            "GetLayout",
            &(0_i32, 2_i32, props),
        )
        .ok()?;
    let (_rev, root): (u32, RawDbusMenuNode) = reply.body().deserialize().ok()?;
    let sections = parse_dbusmenu_root(&root);
    if sections.is_empty() {
        return None;
    }
    Some(AppMenuTree {
        service_name: service_name.to_owned(),
        object_path: object_path.to_owned(),
        sections,
    })
}

fn parse_dbusmenu_root(root: &RawDbusMenuNode) -> Vec<AppMenuSection> {
    let mut sections = Vec::new();
    for child_val in &root.2 {
        let Ok((sec_id, sec_props, sec_children)) = RawDbusMenuNode::try_from(child_val.clone())
        else {
            continue;
        };
        if !prop_bool(&sec_props, "visible", true) {
            continue;
        }
        let title = prop_str(&sec_props, "label").map_or_default(|s| strip_menu_mnemonic(&s));
        if title.is_empty() {
            continue;
        }
        let mut items = Vec::new();
        for item_val in &sec_children {
            let Ok((item_id, item_props, _)) = RawDbusMenuNode::try_from(item_val.clone()) else {
                continue;
            };
            if !prop_bool(&item_props, "visible", true) {
                continue;
            }
            let is_sep = prop_str(&item_props, "type").as_deref() == Some("separator");
            let label = prop_str(&item_props, "label").map_or_default(|s| strip_menu_mnemonic(&s));
            if !is_sep && label.is_empty() {
                continue;
            }
            let checked = if prop_str(&item_props, "toggle-type").is_some() {
                Some(prop_i32(&item_props, "toggle-state") == Some(1))
            } else {
                None
            };
            items.push(AppMenuItem {
                id: item_id,
                label,
                shortcut: None,
                enabled: prop_bool(&item_props, "enabled", true),
                separator: is_sep,
                checked,
            });
        }
        sections.push(AppMenuSection {
            id: sec_id,
            title,
            items,
        });
    }
    sections
}

fn prop_str(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .map(ToOwned::to_owned)
}

fn prop_bool(props: &HashMap<String, OwnedValue>, key: &str, default: bool) -> bool {
    props
        .get(key)
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(default)
}

fn prop_i32(props: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    props.get(key).and_then(|v| i32::try_from(v).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_menu_mnemonics_and_caches_registered_trees() {
        assert_eq!(strip_menu_mnemonic("_File"), "File");
        assert_eq!(strip_menu_mnemonic("Save _As..."), "Save As...");
        assert_eq!(strip_menu_mnemonic("A__B"), "A_B");

        let handle = AppMenuHandle::noop();
        assert!(handle.menu_for_window(7).is_none());
        handle.insert_test_menu(
            7,
            AppMenuTree {
                service_name: ":1.42".to_owned(),
                object_path: "/MenuBar/1".to_owned(),
                sections: vec![AppMenuSection {
                    id: 1,
                    title: "File".to_owned(),
                    items: vec![AppMenuItem {
                        id: 10,
                        label: "New Window".to_owned(),
                        shortcut: Some("Ctrl+N".to_owned()),
                        enabled: true,
                        separator: false,
                        checked: None,
                    }],
                }],
            },
        );
        let tree = handle.menu_for_window(7);
        assert!(tree.is_some());
        if let Some(tree) = tree {
            assert_eq!(tree.sections.len(), 1);
            assert_eq!(tree.sections[0].title, "File");
            assert_eq!(tree.sections[0].items[0].label, "New Window");
        }
    }
}
