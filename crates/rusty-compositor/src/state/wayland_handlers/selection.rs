//! Clipboard, primary selection, data-control, and drag-and-drop handlers.

use std::os::unix::io::OwnedFd;

use smithay::{
    delegate_data_control, delegate_data_device, delegate_primary_selection,
    input::Seat,
    reexports::wayland_server::protocol::{wl_data_source::WlDataSource, wl_surface::WlSurface},
    wayland::selection::{
        SelectionHandler, SelectionSource, SelectionTarget,
        data_device::{
            ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
        },
        primary_selection::{PrimarySelectionHandler, PrimarySelectionState},
        wlr_data_control::{DataControlHandler, DataControlState},
    },
};
use tracing::warn;

use super::super::RustyCompositorState;

impl SelectionHandler for RustyCompositorState {
    type SelectionUserData = ();

    fn new_selection(
        &mut self,
        ty: SelectionTarget,
        source: Option<SelectionSource>,
        _seat: Seat<Self>,
    ) {
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(err) = xwm.new_selection(ty, source.map(|s| s.mime_types())) {
                warn!(?ty, error = ?err, "Failed to mirror Wayland selection to XWayland");
            } else {
                // `X11Wm::new_selection` buffers `SetSelectionOwner` in `x11rb` without flushing;
                // perform a lightweight RandR query so `Xwayland` observes ownership immediately.
                let _ = xwm.get_randr_primary_output();
            }
        }
    }

    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        _user_data: &Self::SelectionUserData,
    ) {
        if let (Some(xwm), Some(loop_handle)) = (self.xwm.as_mut(), self.x11_loop_handle.clone())
            && let Err(err) = xwm.send_selection(ty, mime_type, fd, loop_handle)
        {
            warn!(?ty, error = ?err, "Failed to transfer XWayland selection to Wayland");
        }
    }
}

impl DataDeviceHandler for RustyCompositorState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl PrimarySelectionHandler for RustyCompositorState {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}

impl DataControlHandler for RustyCompositorState {
    fn data_control_state(&self) -> &DataControlState {
        &self.data_control_state
    }
}

impl ClientDndGrabHandler for RustyCompositorState {
    fn started(
        &mut self,
        _source: Option<WlDataSource>,
        icon: Option<WlSurface>,
        _seat: Seat<Self>,
    ) {
        self.dnd_icon = icon;
    }

    fn dropped(&mut self, _target: Option<WlSurface>, _validated: bool, _seat: Seat<Self>) {
        self.dnd_icon = None;
    }
}

impl ServerDndGrabHandler for RustyCompositorState {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

delegate_data_device!(RustyCompositorState);
delegate_primary_selection!(RustyCompositorState);
delegate_data_control!(RustyCompositorState);
