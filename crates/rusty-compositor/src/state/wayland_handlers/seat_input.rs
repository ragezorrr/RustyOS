//! Seat, pointer constraints, tablet, IME/text-input, and keyboard shortcuts inhibit handlers.

use smithay::{
    delegate_cursor_shape, delegate_input_method_manager, delegate_keyboard_shortcuts_inhibit,
    delegate_pointer_constraints, delegate_pointer_gestures, delegate_relative_pointer,
    delegate_seat, delegate_text_input_manager,
    desktop::layer_map_for_output,
    input::{
        Seat, SeatHandler, SeatState,
        pointer::{CursorImageStatus, PointerHandle},
    },
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle},
    wayland::{
        input_method::{InputMethodHandler, PopupSurface as ImePopupSurface},
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState,
            KeyboardShortcutsInhibitor, KeyboardShortcutsInhibitorSeat as _,
        },
        pointer_constraints::{
            PointerConstraint, PointerConstraintsHandler, with_pointer_constraint,
        },
        seat::WaylandFocus,
        selection::{data_device::set_data_device_focus, primary_selection::set_primary_focus},
        tablet_manager::TabletSeatHandler,
    },
};
use tracing::debug;

use super::super::RustyCompositorState;

impl SeatHandler for RustyCompositorState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        if let Some(prev) = self.active_shortcut_inhibitor.as_ref()
            && focused.is_none_or(|surf| prev.wl_surface() != surf)
        {
            prev.inactivate();
            self.active_shortcut_inhibitor = None;
        }
        if self.suppressed_inhibitor_surface.as_ref() != focused {
            self.suppressed_inhibitor_surface = None;
        }
        if !self.is_session_locked()
            && !self.has_blocking_shell_overlay()
            && self.suppressed_inhibitor_surface.as_ref() != focused
            && let Some(surf) = focused
            && let Some(inhibitor) = seat.keyboard_shortcuts_inhibitor_for_surface(surf)
        {
            inhibitor.activate();
            self.active_shortcut_inhibitor = Some(inhibitor);
        }
        let client = focused.and_then(|s| self.display_handle.get_client(s.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client.clone());
        set_primary_focus(&self.display_handle, seat, client);
        let _ = self.display_handle.flush_clients();
        debug!(has_focus = focused.is_some(), "Keyboard focus changed");
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}

impl PointerConstraintsHandler for RustyCompositorState {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        if !self.has_blocking_shell_overlay() && pointer.current_focus().as_ref() == Some(surface) {
            with_pointer_constraint(surface, pointer, |constraint| {
                if let Some(c) = constraint
                    && !c.is_active()
                {
                    c.activate();
                }
            });
        }
    }

    fn cursor_position_hint(
        &mut self,
        surface: &WlSurface,
        pointer: &PointerHandle<Self>,
        location: Point<f64, Logical>,
    ) {
        let mut active_locked = false;
        with_pointer_constraint(surface, pointer, |constraint| {
            if let Some(c) = constraint
                && c.is_active()
                && matches!(&*c, PointerConstraint::Locked(_))
            {
                active_locked = true;
            }
        });
        if active_locked && let Some(bounds) = self.surface_logical_bounds(surface) {
            let total = self.total_output_bounds();
            let warped = Point::from((
                (bounds.loc.x + location.x).clamp(
                    f64::from(total.loc.x),
                    f64::from(total.loc.x + total.size.w - 1).max(f64::from(total.loc.x)),
                ),
                (bounds.loc.y + location.y).clamp(
                    f64::from(total.loc.y),
                    f64::from(total.loc.y + total.size.h - 1).max(f64::from(total.loc.y)),
                ),
            ));
            self.pointer_location = warped;
        }
    }
}

impl TabletSeatHandler for RustyCompositorState {}

impl InputMethodHandler for RustyCompositorState {
    fn new_popup(&mut self, surface: ImePopupSurface) {
        self.ime_popups.retain(ImePopupSurface::alive);
        if !self
            .ime_popups
            .iter()
            .any(|existing| existing.wl_surface() == surface.wl_surface())
        {
            self.ime_popups.push(surface);
        }
    }

    fn dismiss_popup(&mut self, surface: ImePopupSurface) {
        self.ime_popups
            .retain(|existing| existing.wl_surface() != surface.wl_surface() && existing.alive());
    }

    fn popup_repositioned(&mut self, surface: ImePopupSurface) {
        self.ime_popups.retain(ImePopupSurface::alive);
        if !self
            .ime_popups
            .iter()
            .any(|existing| existing.wl_surface() == surface.wl_surface())
            && surface.alive()
        {
            self.ime_popups.push(surface);
        }
    }

    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        for (_, win) in &self.window_bindings {
            if win
                .wl_surface()
                .is_some_and(|root| Self::surface_in_tree(&root, parent))
                && let Some(loc) = self.space.element_location(win)
            {
                return Rectangle::new(loc + win.geometry().loc, win.geometry().size);
            }
        }
        for entry in self.output_map.entries() {
            let map = layer_map_for_output(&entry.output);
            for layer in map.layers() {
                if Self::surface_in_tree(layer.wl_surface(), parent)
                    && let Some(geo) = map.layer_geometry(layer)
                {
                    return Rectangle::new(entry.logical_geometry().loc + geo.loc, geo.size);
                }
            }
        }
        self.output_map.primary().logical_geometry()
    }
}

impl KeyboardShortcutsInhibitHandler for RustyCompositorState {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.keyboard_shortcuts_inhibit_state
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        if self
            .suppressed_inhibitor_surface
            .as_ref()
            .is_some_and(|surf| surf == inhibitor.wl_surface())
        {
            self.suppressed_inhibitor_surface = None;
        }
        if !self.is_session_locked()
            && !self.has_blocking_shell_overlay()
            && self
                .keyboard
                .current_focus()
                .as_ref()
                .is_some_and(|focused| focused == inhibitor.wl_surface())
        {
            inhibitor.activate();
            self.active_shortcut_inhibitor = Some(inhibitor);
        }
    }

    fn inhibitor_destroyed(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        if self
            .suppressed_inhibitor_surface
            .as_ref()
            .is_some_and(|surf| surf == inhibitor.wl_surface())
        {
            self.suppressed_inhibitor_surface = None;
        }
        if self
            .active_shortcut_inhibitor
            .as_ref()
            .is_some_and(|cur| cur.wl_surface() == inhibitor.wl_surface())
        {
            self.active_shortcut_inhibitor = None;
        }
    }
}

delegate_relative_pointer!(RustyCompositorState);
delegate_pointer_constraints!(RustyCompositorState);
delegate_pointer_gestures!(RustyCompositorState);
delegate_cursor_shape!(RustyCompositorState);
delegate_seat!(RustyCompositorState);
delegate_text_input_manager!(RustyCompositorState);
delegate_input_method_manager!(RustyCompositorState);
delegate_keyboard_shortcuts_inhibit!(RustyCompositorState);
