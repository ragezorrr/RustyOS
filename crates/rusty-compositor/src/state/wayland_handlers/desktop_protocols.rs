//! XDG activation, foreign toplevel list, and xdg-toplevel-icon-v1 handlers.

use std::time::Duration;

use smithay::{
    delegate_foreign_toplevel_list, delegate_xdg_activation,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_protocols::xdg::toplevel_icon::v1::server::{
            xdg_toplevel_icon_manager_v1::{self, XdgToplevelIconManagerV1},
            xdg_toplevel_icon_v1::{self, XdgToplevelIconV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, Resource,
            backend::{ClientId, ObjectId},
            delegate_global_dispatch,
            protocol::{wl_buffer, wl_surface::WlSurface},
        },
    },
    wayland::{
        compositor::{self},
        foreign_toplevel_list::{ForeignToplevelListHandler, ForeignToplevelListState},
        shm::{BufferAccessError, with_buffer_contents},
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
        xdg_toplevel_icon::{
            XdgToplevelIconHandler, XdgToplevelIconManager, XdgToplevelIconManagerUserData,
            XdgToplevelIconUserData,
        },
    },
};

use super::super::RustyCompositorState;

/// Double-buffered surface state for `xdg_toplevel_icon_v1`.
///
/// Snapshots icon metadata on `set_icon` so icon state survives `xdg_toplevel_icon_v1.destroy()`,
/// and bypasses Smithay 0.7.0's `ShmBufferUserData::remove_destruction_hook` (`!=`) bug.
#[derive(Debug, Clone, Default)]
pub(in crate::state) struct ToplevelIconSurfaceState {
    pub(in crate::state) icon_name: Option<String>,
    pub(in crate::state) buffers: Vec<(wl_buffer::WlBuffer, i32)>,
}

impl compositor::Cacheable for ToplevelIconSurfaceState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

#[derive(Debug, Clone)]
pub(in crate::state) struct ToplevelIconBufferEntry {
    buffer: wl_buffer::WlBuffer,
    width: i32,
    height: i32,
    scale: i32,
}

#[derive(Debug)]
pub(in crate::state) struct ToplevelIconEntry {
    pub(in crate::state) resource: XdgToplevelIconV1,
    pub(in crate::state) immutable: bool,
    pub(in crate::state) icon_name: Option<String>,
    pub(in crate::state) buffers: Vec<ToplevelIconBufferEntry>,
}

impl ToplevelIconEntry {
    pub(in crate::state) fn add_buffer_replacing_slot(
        &mut self,
        buffer: wl_buffer::WlBuffer,
        width: i32,
        height: i32,
        scale: i32,
    ) {
        if let Some(existing) = self
            .buffers
            .iter_mut()
            .find(|b| b.width == width && b.height == height && b.scale == scale)
        {
            existing.buffer = buffer;
        } else {
            self.buffers.push(ToplevelIconBufferEntry {
                buffer,
                width,
                height,
                scale,
            });
        }
    }

    pub(in crate::state) fn snapshot_surface_state(&self) -> ToplevelIconSurfaceState {
        ToplevelIconSurfaceState {
            icon_name: self.icon_name.clone(),
            buffers: self
                .buffers
                .iter()
                .map(|b| (b.buffer.clone(), b.scale))
                .collect(),
        }
    }

    pub(super) fn references_buffer(&self, buf_id: &ObjectId) -> bool {
        self.buffers.iter().any(|b| b.buffer.id() == *buf_id)
    }
}

impl XdgActivationHandler for RustyCompositorState {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let within_ttl = token_data.timestamp.elapsed() <= Duration::from_secs(10);
        self.xdg_activation_state.remove_token(&token);
        if !within_ttl || self.is_session_locked() {
            return;
        }
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let _ = self.activate_window_surface(&surface, serial);
    }
}

impl ForeignToplevelListHandler for RustyCompositorState {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.foreign_toplevel_list_state
    }
}

impl XdgToplevelIconHandler for RustyCompositorState {
    fn set_icon(&mut self, toplevel: xdg_toplevel::XdgToplevel, wl_surface: WlSurface) {
        if let Some(win_id) = self.window_id_for_xdg_toplevel_proto(&toplevel) {
            self.sync_toplevel_icon_from_surface(win_id, &wl_surface);
        }
    }
}

delegate_global_dispatch!(
    RustyCompositorState: [
        XdgToplevelIconManagerV1: XdgToplevelIconManagerUserData
    ] => XdgToplevelIconManager
);

impl Dispatch<XdgToplevelIconManagerV1, ()> for RustyCompositorState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &XdgToplevelIconManagerV1,
        request: xdg_toplevel_icon_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use xdg_toplevel_icon_manager_v1::Request;

        match request {
            Request::CreateIcon { id } => {
                let resource = data_init.init(id, XdgToplevelIconUserData::default());
                state.toplevel_icons.insert(
                    resource.id(),
                    ToplevelIconEntry {
                        resource,
                        immutable: false,
                        icon_name: None,
                        buffers: Vec::new(),
                    },
                );
            }
            Request::SetIcon { toplevel, icon } => {
                let Some(wl_surface) = state
                    .xdg_shell_state
                    .toplevel_surfaces()
                    .iter()
                    .find(|tl| tl.xdg_toplevel() == &toplevel)
                    .map(|tl| tl.wl_surface().clone())
                else {
                    return;
                };
                let snapshot = icon
                    .and_then(|icon_res| {
                        let entry = state.toplevel_icons.get_mut(&icon_res.id())?;
                        entry.immutable = true;
                        Some(entry.snapshot_surface_state())
                    })
                    .unwrap_or_default();
                compositor::with_states(&wl_surface, |states| {
                    let mut cached = states.cached_state.get::<ToplevelIconSurfaceState>();
                    *cached.pending() = snapshot;
                });
                state.set_icon(toplevel, wl_surface);
            }
            Request::Destroy | _ => {}
        }
    }
}

impl Dispatch<XdgToplevelIconV1, XdgToplevelIconUserData> for RustyCompositorState {
    fn request(
        state: &mut Self,
        _client: &Client,
        icon: &XdgToplevelIconV1,
        request: xdg_toplevel_icon_v1::Request,
        _data: &XdgToplevelIconUserData,
        dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use xdg_toplevel_icon_v1::Request;

        match request {
            Request::SetName { icon_name } => {
                let Some(entry) = state.toplevel_icons.get_mut(&icon.id()) else {
                    return;
                };
                if entry.immutable {
                    dh.post_error(
                        icon,
                        xdg_toplevel_icon_v1::Error::Immutable as u32,
                        "Request made after the icon has been assigned to a toplevel via 'set_icon'"
                            .to_owned(),
                    );
                    return;
                }
                entry.icon_name = Some(icon_name);
            }
            Request::AddBuffer { buffer, scale } => {
                let is_immutable = state
                    .toplevel_icons
                    .get(&icon.id())
                    .is_some_and(|entry| entry.immutable);
                if is_immutable {
                    dh.post_error(
                        icon,
                        xdg_toplevel_icon_v1::Error::Immutable as u32,
                        "Request made after the icon has been assigned to a toplevel via 'set_icon'"
                            .to_owned(),
                    );
                    return;
                }
                let (width, height) =
                    match with_buffer_contents(&buffer, |_, _, buf| (buf.width, buf.height)) {
                        Ok(dims) => dims,
                        Err(BufferAccessError::NotManaged) => {
                            dh.post_error(
                                icon,
                                xdg_toplevel_icon_v1::Error::InvalidBuffer as u32,
                                "The wl_buffer must be backed by wl_shm".to_owned(),
                            );
                            return;
                        }
                        Err(_) => return,
                    };
                if width <= 0 || width != height {
                    dh.post_error(
                        icon,
                        xdg_toplevel_icon_v1::Error::InvalidBuffer as u32,
                        "The wl_buffer must be a square".to_owned(),
                    );
                    return;
                }
                if let Some(entry) = state.toplevel_icons.get_mut(&icon.id()) {
                    entry.add_buffer_replacing_slot(buffer, width, height, scale);
                }
            }
            Request::Destroy => {
                state.toplevel_icons.remove(&icon.id());
            }
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        resource: &XdgToplevelIconV1,
        _data: &XdgToplevelIconUserData,
    ) {
        state.toplevel_icons.remove(&resource.id());
    }
}

delegate_xdg_activation!(RustyCompositorState);
delegate_foreign_toplevel_list!(RustyCompositorState);
