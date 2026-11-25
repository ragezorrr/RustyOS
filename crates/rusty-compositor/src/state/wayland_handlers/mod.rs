//! Grouped Smithay Wayland protocol handler implementations for [`RustyCompositorState`].

pub(crate) mod app_menu;
mod core;
pub(super) mod desktop_protocols;
mod dmabuf_sync;
pub(crate) mod liquid_glass;
mod seat_input;
mod selection;
mod session_lock;
