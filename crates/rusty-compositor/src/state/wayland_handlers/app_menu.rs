//! First-party Wayland Global Menu Bar protocol handler (`rustyos_app_menu_v1`).

#![allow(missing_docs, clippy::all, unreachable_pub)]

use std::sync::{Arc, Mutex};

use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
    backend::{ClientId, GlobalId, ObjectId},
    protocol::wl_surface::WlSurface,
};

use crate::app_menu::{AppMenuItem, AppMenuSection, AppMenuTree};

use super::super::RustyCompositorState;

#[allow(clippy::wildcard_imports)]
pub mod protocol {
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    #[allow(clippy::wildcard_imports)]
    pub mod __interfaces {
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../../protocols/rustyos-app-menu-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!("../../protocols/rustyos-app-menu-v1.xml");
}

pub use protocol::rustyos_app_menu_manager_v1::RustyosAppMenuManagerV1;
pub use protocol::rustyos_surface_app_menu_v1::RustyosSurfaceAppMenuV1;

#[derive(Debug, Default)]
struct PendingAppMenuState {
    sections: Vec<AppMenuSection>,
}

/// Per-resource user data associating `rustyos_surface_app_menu_v1` with its `WlSurface`.
#[derive(Debug)]
pub struct AppMenuSurfaceUserData {
    surface: Mutex<Option<WlSurface>>,
    pending: Arc<Mutex<PendingAppMenuState>>,
}

impl AppMenuSurfaceUserData {
    fn new(surface: WlSurface) -> Self {
        Self {
            surface: Mutex::new(Some(surface)),
            pending: Arc::new(Mutex::new(PendingAppMenuState::default())),
        }
    }

    fn surface_id(&self) -> Option<ObjectId> {
        self.surface
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(Resource::id))
    }

    fn clear(&self) {
        if let Ok(mut g) = self.surface.lock() {
            *g = None;
        }
    }
}

/// State holder for the `rustyos_app_menu_manager_v1` Wayland global.
#[derive(Debug)]
pub struct AppMenuManagerState {
    pub global: GlobalId,
}

impl AppMenuManagerState {
    /// Registers `rustyos_app_menu_manager_v1` on `display`.
    pub fn new(display: &DisplayHandle) -> Self {
        let global =
            display.create_global::<RustyCompositorState, RustyosAppMenuManagerV1, _>(1, ());
        Self { global }
    }
}

impl GlobalDispatch<RustyosAppMenuManagerV1, ()> for RustyCompositorState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<RustyosAppMenuManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<RustyosAppMenuManagerV1, ()> for RustyCompositorState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &RustyosAppMenuManagerV1,
        request: protocol::rustyos_app_menu_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use protocol::rustyos_app_menu_manager_v1::Request;

        if let Request::GetMenuSurface { id, surface } = request {
            let user_data = AppMenuSurfaceUserData::new(surface);
            let _ = data_init.init(id, user_data);
        }
    }
}

impl Dispatch<RustyosSurfaceAppMenuV1, AppMenuSurfaceUserData> for RustyCompositorState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &RustyosSurfaceAppMenuV1,
        request: protocol::rustyos_surface_app_menu_v1::Request,
        data: &AppMenuSurfaceUserData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use protocol::rustyos_surface_app_menu_v1::Request;

        match request {
            Request::BeginUpdate => {
                if let Ok(mut pending) = data.pending.lock() {
                    pending.sections.clear();
                }
            }
            Request::BeginSection { id, title } => {
                if let Ok(mut pending) = data.pending.lock() {
                    pending.sections.push(AppMenuSection {
                        id,
                        title,
                        items: Vec::new(),
                    });
                }
            }
            Request::AddItem {
                id,
                label,
                shortcut,
                enabled,
                separator,
                checked,
            } => {
                if let Ok(mut pending) = data.pending.lock()
                    && let Some(sec) = pending.sections.last_mut()
                {
                    sec.items.push(AppMenuItem {
                        id,
                        label,
                        shortcut: (!shortcut.trim().is_empty()).then_some(shortcut),
                        enabled: enabled != 0,
                        separator: separator != 0,
                        checked: match checked {
                            0 => Some(false),
                            1 => Some(true),
                            _ => None,
                        },
                    });
                }
            }
            Request::Commit => {
                let Some(surface_id) = data.surface_id() else {
                    return;
                };
                let sections = data.pending.lock().map_or_default(|p| p.sections.clone());
                if sections.is_empty() {
                    state.wayland_app_menus.remove(&surface_id);
                } else {
                    let tree = AppMenuTree {
                        service_name: "__wayland__".to_owned(),
                        object_path: format!("{surface_id:?}"),
                        sections,
                    };
                    state
                        .wayland_app_menus
                        .insert(surface_id, (tree, resource.clone()));
                }
                state.shell_redraw_needed = true;
            }
            Request::Destroy => {
                if let Some(surface_id) = data.surface_id() {
                    state.wayland_app_menus.remove(&surface_id);
                    state.shell_redraw_needed = true;
                }
                data.clear();
            }
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        _resource: &RustyosSurfaceAppMenuV1,
        data: &AppMenuSurfaceUserData,
    ) {
        if let Some(surface_id) = data.surface_id() {
            state.wayland_app_menus.remove(&surface_id);
            state.shell_redraw_needed = true;
        }
        data.clear();
    }
}
