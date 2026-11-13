//! Idle notify, idle inhibit, and ext-session-lock-v1 protocol handlers.

use smithay::{
    delegate_idle_inhibit, delegate_idle_notify, delegate_session_lock,
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    utils::IsAlive,
    wayland::{
        idle_inhibit::IdleInhibitHandler,
        idle_notify::{IdleNotifierHandler, IdleNotifierState},
        session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker},
    },
};

use super::super::{RustyCompositorState, TouchpadSwipeState};

impl IdleNotifierHandler for RustyCompositorState {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.idle_notifier_state
    }
}

impl IdleInhibitHandler for RustyCompositorState {
    fn inhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitor_surfaces.retain(IsAlive::alive);
        if !self.idle_inhibitor_surfaces.iter().any(|s| s == &surface) {
            self.idle_inhibitor_surfaces.push(surface);
        }
        self.idle_notifier_state
            .set_is_inhibited(!self.idle_inhibitor_surfaces.is_empty());
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitor_surfaces
            .retain(|s| s.alive() && s != &surface);
        self.idle_notifier_state
            .set_is_inhibited(!self.idle_inhibitor_surfaces.is_empty());
    }
}

impl SessionLockHandler for RustyCompositorState {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        if self.ext_session_locked {
            drop(confirmation);
            return;
        }
        self.ext_session_locked = true;
        self.abort_portal_activity_for_lock();
        self.window_switcher = None;
        self.dnd_icon = None;
        self.close_launcher();
        self.close_quick_settings();
        self.close_overview();
        self.active_window_drag = None;
        self.active_window_resize = None;
        self.dock_hover_slot = None;
        self.touchpad_swipe = TouchpadSwipeState::default();
        self.deactivate_focused_shortcut_inhibitor();
        self.deactivate_active_pointer_constraint();

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, None, serial);
        let pointer = self.pointer.clone();
        pointer.motion(
            self,
            None,
            &smithay::input::pointer::MotionEvent {
                location: self.pointer_location,
                serial,
                time: u32::try_from(self.elapsed().as_millis()).unwrap_or(0),
            },
        );
        pointer.frame(self);
        self.pending_session_locker = Some(confirmation);
    }

    fn unlock(&mut self) {
        self.ext_session_locked = false;
        self.pending_session_locker = None;
        self.suppressed_inhibitor_surface = None;
        self.ext_lock_surfaces.clear();
        self.sync_portal_snapshot();
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.sync_space_with_wm(serial);
    }

    fn new_surface(&mut self, surface: LockSurface, wl_output: WlOutput) {
        self.ext_lock_surfaces.retain(|(_, s)| s.alive());
        let resolved_output =
            Output::from_resource(&wl_output).unwrap_or_else(|| self.output.clone());
        self.ext_lock_surfaces
            .retain(|(out, _)| out != &resolved_output);

        let logical_size = self.logical_u32_size_for_output(&resolved_output);
        let output_scale = self
            .output_map
            .entries()
            .iter()
            .find(|e| e.output == resolved_output)
            .map_or(1.0, |e| e.scale);
        Self::apply_surface_scale(surface.wl_surface(), output_scale);
        surface.with_pending_state(|state| {
            state.size = Some(logical_size);
        });
        let lock_wl = surface.wl_surface().clone();
        self.ext_lock_surfaces.push((resolved_output, surface));

        if self.ext_session_locked {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let keyboard = self.keyboard.clone();
            keyboard.set_focus(self, Some(lock_wl), serial);
        }
    }
}

delegate_idle_notify!(RustyCompositorState);
delegate_idle_inhibit!(RustyCompositorState);
delegate_session_lock!(RustyCompositorState);
