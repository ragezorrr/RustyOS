//! Canonical client-side bindings and menu model for `rustyos_app_menu_v1`.

#![allow(missing_docs, clippy::all, unreachable_pub)]

#[allow(clippy::wildcard_imports)]
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    #[allow(clippy::wildcard_imports)]
    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../../protocols/rustyos-app-menu-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("../../protocols/rustyos-app-menu-v1.xml");
}

pub use protocol::rustyos_app_menu_manager_v1::RustyosAppMenuManagerV1;
pub use protocol::rustyos_surface_app_menu_v1::{
    Event as SurfaceAppMenuEvent, RustyosSurfaceAppMenuV1,
};

/// Single exported item or separator inside an application's global Menu Bar section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMenuItemSpec {
    /// Application-assigned action id delivered back via `RustyApp::on_menu_item`.
    pub id: i32,
    /// Display label.
    pub label: String,
    /// Optional shortcut hint (e.g. `"Ctrl+N"`).
    pub shortcut: Option<String>,
    /// Whether the item can be clicked.
    pub enabled: bool,
    /// True when rendered as a horizontal separator.
    pub separator: bool,
    /// Optional checkmark (`Some(true)` checked, `Some(false)` unchecked).
    pub checked: Option<bool>,
}

impl AppMenuItemSpec {
    /// Creates an enabled action item.
    #[must_use]
    pub fn action(id: i32, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            shortcut: None,
            enabled: true,
            separator: false,
            checked: None,
        }
    }

    /// Creates a horizontal divider item.
    #[must_use]
    pub const fn separator() -> Self {
        Self {
            id: 0,
            label: String::new(),
            shortcut: None,
            enabled: false,
            separator: true,
            checked: None,
        }
    }

    /// Attaches a shortcut hint label.
    #[must_use]
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Sets whether this item is interactive.
    #[must_use]
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets an optional checkmark indicator.
    #[must_use]
    pub const fn with_checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
}

/// Top-level Menu Bar section exported by a native RustyOS application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMenuSectionSpec {
    /// Section identifier.
    pub id: i32,
    /// Menu header title (`File`, `Edit`, `View`, ...).
    pub title: String,
    /// Rows belonging to this menu.
    pub items: Vec<AppMenuItemSpec>,
}

impl AppMenuSectionSpec {
    /// Creates a named top-level menu section.
    #[must_use]
    pub fn new(id: i32, title: impl Into<String>, items: Vec<AppMenuItemSpec>) -> Self {
        Self {
            id,
            title: title.into(),
            items,
        }
    }
}
