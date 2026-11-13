//! Linux DMA-BUF, DRM explicit syncobj, and presentation-time handlers.

use smithay::{
    backend::allocator::{Buffer as AllocatorBuffer, dmabuf::Dmabuf},
    delegate_dmabuf, delegate_drm_syncobj, delegate_presentation,
    wayland::{
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        drm_syncobj::{DrmSyncobjHandler, DrmSyncobjState},
    },
};

use super::super::RustyCompositorState;

impl DmabufHandler for RustyCompositorState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        if dmabuf.num_planes() > 0 && dmabuf.width() > 0 && dmabuf.height() > 0 {
            let _ = notifier.successful::<Self>();
        } else {
            notifier.failed();
        }
    }
}

impl DrmSyncobjHandler for RustyCompositorState {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.syncobj_state.as_mut()
    }
}

delegate_presentation!(RustyCompositorState);
delegate_dmabuf!(RustyCompositorState);
delegate_drm_syncobj!(RustyCompositorState);
