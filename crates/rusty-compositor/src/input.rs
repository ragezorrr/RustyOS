//! Input routing and keyboard shortcuts for `rusty-compositor` (`RUSTYOS-002` / `RUSTYOS-005`).

use rusty_wm::{SnapTarget, WmAction, WorkspaceId};
use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER, Size},
};
use tracing::info;

use crate::state::RustyCompositorState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyIntercept {
    ExitCompositor,
    Wm(WmAction),
}

impl RustyCompositorState {
    /// Processes an input event from an [`InputBackend`] (such as Winit).
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        match event {
            InputEvent::Keyboard { event } => self.handle_keyboard_event::<B>(&event),
            InputEvent::PointerMotionAbsolute { event } => {
                self.handle_pointer_motion_absolute::<B>(&event);
            }
            InputEvent::PointerButton { event } => self.handle_pointer_button::<B>(&event),
            InputEvent::PointerAxis { event } => self.handle_pointer_axis::<B>(&event),
            _ => {}
        }
    }

    fn handle_keyboard_event<B: InputBackend>(&mut self, event: &B::KeyboardKeyEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time_msec(event);
        let key_state = event.state();
        let keyboard = self.keyboard.clone();

        let intercepted = keyboard.input(
            self,
            event.key_code(),
            key_state,
            serial,
            time,
            |_state, modifiers, handle| {
                if key_state != KeyState::Pressed {
                    return FilterResult::Forward;
                }
                let sym = handle.modified_sym();
                if let Some(action) = match_shortcut(*modifiers, sym) {
                    FilterResult::Intercept(action)
                } else {
                    FilterResult::Forward
                }
            },
        );

        match intercepted {
            Some(KeyIntercept::ExitCompositor) => {
                info!("Exit shortcut pressed; shutting down nested compositor");
                self.request_shutdown();
            }
            Some(KeyIntercept::Wm(action)) => {
                self.apply_wm_action(action, serial);
            }
            None => {}
        }
    }

    fn handle_pointer_motion_absolute<B: InputBackend>(
        &mut self,
        event: &B::PointerMotionAbsoluteEvent,
    ) {
        let output_geo = self
            .space
            .output_geometry(&self.output)
            .map_or_else(|| Size::from((1280, 800)), |geo| geo.size);

        let pos: Point<f64, Logical> = event.position_transformed(output_geo);
        self.pointer_location = pos;

        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time_msec(event);
        let under = self
            .surface_under_pointer(pos)
            .map(|(_win, surface, origin)| (surface, origin));

        let pointer = self.pointer.clone();
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pos,
                serial,
                time,
            },
        );
        pointer.frame(self);
    }

    fn handle_pointer_button<B: InputBackend>(&mut self, event: &B::PointerButtonEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let button = event.button_code();
        let button_state = event.state();

        if button_state == ButtonState::Pressed {
            if let Some((window, _surface, _origin)) =
                self.surface_under_pointer(self.pointer_location)
            {
                if let Some(win_id) = self.window_id_for_smithay(&window) {
                    self.wm.focus_and_raise(win_id);
                    self.sync_space_with_wm(serial);
                }
            } else {
                self.space.elements().for_each(|w| {
                    if w.set_activated(false)
                        && let Some(tl) = w.toplevel()
                    {
                        tl.send_configure();
                    }
                });
                let keyboard = self.keyboard.clone();
                keyboard.set_focus(self, None, serial);
            }
        }

        let pointer = self.pointer.clone();
        pointer.button(
            self,
            &ButtonEvent {
                button,
                state: button_state,
                serial,
                time: Event::time_msec(event),
            },
        );
        pointer.frame(self);
    }

    fn handle_pointer_axis<B: InputBackend>(&mut self, event: &B::PointerAxisEvent) {
        let source = event.source();
        let horizontal_amount = event
            .amount(Axis::Horizontal)
            .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.0);
        let vertical_amount = event
            .amount(Axis::Vertical)
            .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.0);
        let horizontal_discrete = event.amount_v120(Axis::Horizontal);
        let vertical_discrete = event.amount_v120(Axis::Vertical);

        let mut frame = AxisFrame::new(Event::time_msec(event)).source(source);

        if horizontal_amount != 0.0 {
            frame = frame.value(Axis::Horizontal, horizontal_amount);
            if let Some(discrete) = horizontal_discrete {
                #[allow(clippy::cast_possible_truncation)]
                {
                    frame = frame.v120(Axis::Horizontal, discrete as i32);
                }
            }
        }
        if vertical_amount != 0.0 {
            frame = frame.value(Axis::Vertical, vertical_amount);
            if let Some(discrete) = vertical_discrete {
                #[allow(clippy::cast_possible_truncation)]
                {
                    frame = frame.v120(Axis::Vertical, discrete as i32);
                }
            }
        }
        if source == AxisSource::Finger {
            if event.amount(Axis::Horizontal) == Some(0.0) {
                frame = frame.stop(Axis::Horizontal);
            }
            if event.amount(Axis::Vertical) == Some(0.0) {
                frame = frame.stop(Axis::Vertical);
            }
        }

        let pointer = self.pointer.clone();
        pointer.axis(self, frame);
        pointer.frame(self);
    }
}

/// Maps keyboard modifiers and keysyms to compositor / window manager actions.
const fn match_shortcut(modifiers: ModifiersState, sym: Keysym) -> Option<KeyIntercept> {
    if sym.raw() == Keysym::Escape.raw()
        || (modifiers.ctrl && modifiers.alt && sym.raw() == Keysym::BackSpace.raw())
    {
        return Some(KeyIntercept::ExitCompositor);
    }

    if !modifiers.alt {
        return None;
    }

    if let Some(ws_idx) = keysym_to_workspace_index(sym) {
        let ws = WorkspaceId::new_clamped(ws_idx);
        return if modifiers.shift {
            Some(KeyIntercept::Wm(WmAction::MoveFocusedToWorkspace(ws)))
        } else {
            Some(KeyIntercept::Wm(WmAction::SwitchWorkspace(ws)))
        };
    }

    match sym {
        Keysym::Tab | Keysym::j | Keysym::J => {
            if modifiers.shift {
                Some(KeyIntercept::Wm(WmAction::FocusPrev))
            } else {
                Some(KeyIntercept::Wm(WmAction::FocusNext))
            }
        }
        Keysym::k | Keysym::K => Some(KeyIntercept::Wm(WmAction::FocusPrev)),
        Keysym::w | Keysym::W | Keysym::q | Keysym::Q => {
            Some(KeyIntercept::Wm(WmAction::CloseFocused))
        }
        Keysym::l | Keysym::L | Keysym::space => Some(KeyIntercept::Wm(WmAction::ToggleLayoutMode)),
        Keysym::m | Keysym::M => Some(KeyIntercept::Wm(WmAction::ToggleMaximizeFocused)),
        Keysym::Left => Some(KeyIntercept::Wm(WmAction::SnapFocused(
            SnapTarget::LeftHalf,
        ))),
        Keysym::Right => Some(KeyIntercept::Wm(WmAction::SnapFocused(
            SnapTarget::RightHalf,
        ))),
        Keysym::c | Keysym::C => Some(KeyIntercept::Wm(WmAction::SnapFocused(SnapTarget::Center))),
        Keysym::o | Keysym::O => Some(KeyIntercept::Wm(WmAction::MoveFocusedToNextOutput)),
        _ => None,
    }
}

const fn keysym_to_workspace_index(sym: Keysym) -> Option<u8> {
    match sym {
        Keysym::_1 | Keysym::exclam => Some(1),
        Keysym::_2 | Keysym::at => Some(2),
        Keysym::_3 | Keysym::numbersign => Some(3),
        Keysym::_4 | Keysym::dollar => Some(4),
        Keysym::_5 | Keysym::percent => Some(5),
        Keysym::_6 | Keysym::asciicircum => Some(6),
        Keysym::_7 | Keysym::ampersand => Some(7),
        Keysym::_8 | Keysym::asterisk => Some(8),
        Keysym::_9 | Keysym::parenleft => Some(9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_mapping_covers_focus_workspaces_and_layouts() {
        let alt = ModifiersState {
            alt: true,
            ..ModifiersState::default()
        };
        let alt_shift = ModifiersState {
            alt: true,
            shift: true,
            ..ModifiersState::default()
        };

        assert_eq!(
            match_shortcut(alt, Keysym::Tab),
            Some(KeyIntercept::Wm(WmAction::FocusNext))
        );
        assert_eq!(
            match_shortcut(alt_shift, Keysym::Tab),
            Some(KeyIntercept::Wm(WmAction::FocusPrev))
        );
        assert_eq!(
            match_shortcut(alt, Keysym::_2),
            Some(KeyIntercept::Wm(WmAction::SwitchWorkspace(WorkspaceId(2))))
        );
        assert_eq!(
            match_shortcut(alt_shift, Keysym::_3),
            Some(KeyIntercept::Wm(WmAction::MoveFocusedToWorkspace(
                WorkspaceId(3)
            )))
        );
        assert_eq!(
            match_shortcut(alt, Keysym::l),
            Some(KeyIntercept::Wm(WmAction::ToggleLayoutMode))
        );
        assert_eq!(
            match_shortcut(alt, Keysym::Left),
            Some(KeyIntercept::Wm(WmAction::SnapFocused(
                SnapTarget::LeftHalf
            )))
        );
        assert_eq!(
            match_shortcut(ModifiersState::default(), Keysym::Escape),
            Some(KeyIntercept::ExitCompositor)
        );
    }
}
