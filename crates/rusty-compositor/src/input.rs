//! Input routing and keyboard shortcuts for `rusty-compositor` (`RUSTYOS-002` / `RUSTYOS-005`).

use std::time::Instant;

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyIntercept {
    ExitCompositor,
    ToggleReducedMotion,
    ToggleLauncher,
    ToggleQuickSettings,
    QuickSettingsClose,
    LauncherClose,
    LauncherActivate,
    LauncherMove(isize),
    LauncherBackspace,
    LauncherClear,
    LauncherText(String),
    LauncherConsume,
    Wm(WmAction),
}

impl RustyCompositorState {
    /// Processes an input event from an [`InputBackend`] (such as Winit).
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        self.frame_timing.mark_input_event(Instant::now());
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

    /// Injects a synthetic pointer motion event at the given logical coordinates (`--bench-input`).
    pub fn inject_synthetic_pointer_motion(&mut self, pos: Point<f64, Logical>, time_msec: u32) {
        self.frame_timing.mark_input_event(Instant::now());
        self.pointer_location = pos;

        let serial = SERIAL_COUNTER.next_serial();
        let under = self
            .target_surface_under(pos)
            .map(|(_win, surface, origin)| (surface, origin));

        let pointer = self.pointer.clone();
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pos,
                serial,
                time: time_msec,
            },
        );
        pointer.frame(self);
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
            |state, modifiers, handle| {
                if key_state != KeyState::Pressed {
                    return if state.launcher_active {
                        FilterResult::Intercept(KeyIntercept::LauncherConsume)
                    } else {
                        FilterResult::Forward
                    };
                }
                let sym = handle.modified_sym();
                if state.launcher_active {
                    let utf8 = sym.key_char().map(|ch| ch.to_string());
                    return FilterResult::Intercept(match_launcher_key(
                        *modifiers,
                        sym,
                        utf8.as_deref(),
                    ));
                }
                if state.quick_settings_active && sym.raw() == Keysym::Escape.raw() {
                    return FilterResult::Intercept(KeyIntercept::QuickSettingsClose);
                }
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
            Some(KeyIntercept::ToggleReducedMotion) => {
                let reduced = self.animations.toggle_reduced_motion();
                info!(
                    reduced_motion = reduced,
                    "Toggled reduced-motion preference"
                );
            }
            Some(KeyIntercept::ToggleLauncher) => {
                self.toggle_launcher();
                info!(
                    launcher_active = self.launcher_active,
                    "Toggled Spotlight application launcher overlay"
                );
            }
            Some(KeyIntercept::ToggleQuickSettings) => {
                self.toggle_quick_settings();
                info!(
                    quick_settings_active = self.quick_settings_active,
                    "Toggled Quick Settings popover"
                );
            }
            Some(KeyIntercept::QuickSettingsClose) => {
                self.close_quick_settings();
                info!("Closed Quick Settings popover");
            }
            Some(KeyIntercept::LauncherClose) => {
                self.close_launcher();
                info!("Closed Spotlight application launcher");
            }
            Some(KeyIntercept::LauncherActivate) => {
                self.launch_spotlight_selected();
            }
            Some(KeyIntercept::LauncherMove(delta)) => {
                self.launcher.move_selection(delta);
            }
            Some(KeyIntercept::LauncherBackspace) => {
                self.launcher.pop_char();
            }
            Some(KeyIntercept::LauncherClear) => {
                self.launcher.reset();
            }
            Some(KeyIntercept::LauncherText(text)) => {
                self.launcher.push_text(&text);
            }
            Some(KeyIntercept::LauncherConsume) | None => {}
            Some(KeyIntercept::Wm(action)) => {
                self.apply_wm_action(action, serial);
            }
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

        if let Some(drag) = self.active_window_drag {
            #[allow(clippy::cast_possible_truncation)]
            let dx = (pos.x - drag.start_pointer.x).round() as i32;
            #[allow(clippy::cast_possible_truncation)]
            let dy = (pos.y - drag.start_pointer.y).round() as i32;
            let new_x = drag.start_origin.0 + dx;
            let new_y = (drag.start_origin.1 + dy).max(crate::shell_render::TOP_BAR_HEIGHT);
            if self.wm.move_window(drag.window_id, new_x, new_y)
                && let Some(win) = self.wm.window(drag.window_id)
            {
                let rect = win.geometry;
                self.animations.snap_window_immediate(
                    drag.window_id.0,
                    rusty_animation::LogicalRect::new(rect.x, rect.y, rect.width, rect.height),
                );
                let serial = SERIAL_COUNTER.next_serial();
                self.sync_space_with_wm(serial);
            }
            return;
        }

        if let Some(resize) = self.active_window_resize {
            #[allow(clippy::cast_possible_truncation)]
            let dx = (pos.x - resize.start_pointer.x).round() as i32;
            #[allow(clippy::cast_possible_truncation)]
            let dy = (pos.y - resize.start_pointer.y).round() as i32;
            let target_rect = compute_resized_rect(
                resize.start_rect,
                resize.edges,
                dx,
                dy,
                crate::shell_render::TOP_BAR_HEIGHT,
            );
            if self.wm.resize_window(resize.window_id, target_rect)
                && let Some(win) = self.wm.window(resize.window_id)
            {
                let rect = win.geometry;
                self.animations.snap_window_immediate(
                    resize.window_id.0,
                    rusty_animation::LogicalRect::new(rect.x, rect.y, rect.width, rect.height),
                );
                let serial = SERIAL_COUNTER.next_serial();
                self.sync_space_with_wm(serial);
            }
            return;
        }

        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time_msec(event);
        let under = self
            .target_surface_under(pos)
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

    fn handle_launcher_pointer_click(&mut self) {
        let output_size = self
            .space
            .output_geometry(&self.output)
            .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
            .to_physical(1);
        match crate::shell_render::check_spotlight_click(
            output_size,
            self.pointer_location.x,
            self.pointer_location.y,
        ) {
            crate::shell_render::SpotlightHit::Row(row_idx) => {
                self.launch_spotlight_row(row_idx);
            }
            crate::shell_render::SpotlightHit::InsideModal => {}
            crate::shell_render::SpotlightHit::OutsideModal => {
                self.close_launcher();
            }
        }
    }

    fn handle_quick_settings_pointer_click(&mut self, serial: smithay::utils::Serial) {
        let output_size = self
            .space
            .output_geometry(&self.output)
            .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
            .to_physical(1);
        match crate::shell_render::check_quick_settings_click(
            output_size,
            self.pointer_location.x,
            self.pointer_location.y,
        ) {
            crate::shell_render::QuickSettingsHit::ToggleMute => {
                self.telemetry.toggle_mute();
            }
            crate::shell_render::QuickSettingsHit::VolumeDown => {
                self.telemetry.step_volume(-5);
            }
            crate::shell_render::QuickSettingsHit::VolumeUp => {
                self.telemetry.step_volume(5);
            }
            crate::shell_render::QuickSettingsHit::ToggleLayout => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            crate::shell_render::QuickSettingsHit::ToggleReducedMotion => {
                let _ = self.animations.toggle_reduced_motion();
            }
            crate::shell_render::QuickSettingsHit::OpenSpotlight => {
                self.toggle_launcher();
            }
            crate::shell_render::QuickSettingsHit::RefreshMetrics => {
                self.telemetry.request_refresh();
            }
            crate::shell_render::QuickSettingsHit::InsidePopover => {}
            crate::shell_render::QuickSettingsHit::OutsidePopover => {
                self.close_quick_settings();
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_pointer_button<B: InputBackend>(&mut self, event: &B::PointerButtonEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let button = event.button_code();
        let button_state = event.state();

        if button_state == ButtonState::Released {
            self.active_window_drag = None;
            self.active_window_resize = None;
        }

        if button_state == ButtonState::Pressed {
            let output_width = self
                .space
                .output_geometry(&self.output)
                .map_or(1280, |geo| geo.size.w);

            // Check for TopBar click to switch workspaces, toggle layout, or open Quick Settings
            if let Some(target_ws) = crate::shell_render::check_top_bar_workspace_click(
                self.pointer_location.x,
                self.pointer_location.y,
            ) {
                let ws = WorkspaceId::new_clamped(target_ws);
                self.apply_wm_action(WmAction::SwitchWorkspace(ws), serial);
                return;
            }
            if crate::shell_render::check_top_bar_layout_click(
                self.pointer_location.x,
                self.pointer_location.y,
            ) {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
                return;
            }
            if crate::shell_render::check_top_bar_status_click(
                output_width,
                self.pointer_location.x,
                self.pointer_location.y,
            ) {
                self.toggle_quick_settings();
                return;
            }

            // Handle Spotlight clicks exclusively while launcher overlay is open (RUSTYOS-018b)
            if self.launcher_active {
                self.handle_launcher_pointer_click();
                return;
            }

            // Handle Quick Settings popover clicks exclusively while open (RUSTYOS-021)
            if self.quick_settings_active {
                self.handle_quick_settings_pointer_click(serial);
                return;
            }

            // Check for SSD titlebar & traffic-light button hits (RUSTYOS-019)
            if !self.launcher_active {
                let visible_rects: Vec<(rusty_wm::WindowId, rusty_wm::Rect)> = self
                    .wm
                    .visible_windows()
                    .into_iter()
                    .map(|w| (w.id, w.geometry))
                    .collect();
                if let Some(hit) = crate::shell_render::check_window_ssd_hit(
                    &visible_rects,
                    self.pointer_location.x,
                    self.pointer_location.y,
                ) {
                    match hit {
                        crate::shell_render::SsdHit::Close(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.apply_wm_action(WmAction::CloseFocused, serial);
                        }
                        crate::shell_render::SsdHit::Minimize(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.apply_wm_action(WmAction::SnapFocused(SnapTarget::Center), serial);
                        }
                        crate::shell_render::SsdHit::Maximize(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
                        }
                        crate::shell_render::SsdHit::TitlebarDrag(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.sync_space_with_wm(serial);
                            if let Some(win) = self.wm.window(win_id) {
                                self.active_window_drag = Some(crate::state::WindowDragState {
                                    window_id: win_id,
                                    start_pointer: self.pointer_location,
                                    start_origin: (win.geometry.x, win.geometry.y),
                                });
                            }
                        }
                        crate::shell_render::SsdHit::Resize(win_id, edges) => {
                            self.wm.focus_and_raise(win_id);
                            self.sync_space_with_wm(serial);
                            if let Some(win) = self.wm.window(win_id) {
                                self.active_window_resize = Some(crate::state::WindowResizeState {
                                    window_id: win_id,
                                    edges,
                                    start_pointer: self.pointer_location,
                                    start_rect: win.geometry,
                                });
                            }
                        }
                    }
                    return;
                }
            }

            if let Some((win_opt, _surface, _origin)) =
                self.target_surface_under(self.pointer_location)
            {
                if let Some(window) = win_opt
                    && let Some(win_id) = self.window_id_for_smithay(&window)
                {
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

fn match_launcher_key(modifiers: ModifiersState, sym: Keysym, utf8: Option<&str>) -> KeyIntercept {
    if modifiers.ctrl && modifiers.alt && sym.raw() == Keysym::BackSpace.raw() {
        return KeyIntercept::ExitCompositor;
    }
    if sym.raw() == Keysym::Escape.raw() {
        return KeyIntercept::LauncherClose;
    }
    if (modifiers.logo || modifiers.alt) && sym.raw() == Keysym::space.raw() {
        return KeyIntercept::ToggleLauncher;
    }
    if sym.raw() == Keysym::Return.raw() || sym.raw() == Keysym::KP_Enter.raw() {
        return KeyIntercept::LauncherActivate;
    }
    if sym.raw() == Keysym::Up.raw()
        || sym.raw() == Keysym::KP_Up.raw()
        || (modifiers.ctrl
            && (sym == Keysym::p || sym == Keysym::P || sym == Keysym::k || sym == Keysym::K))
    {
        return KeyIntercept::LauncherMove(-1);
    }
    if sym.raw() == Keysym::Down.raw()
        || sym.raw() == Keysym::KP_Down.raw()
        || sym.raw() == Keysym::Tab.raw()
        || (modifiers.ctrl
            && (sym == Keysym::n || sym == Keysym::N || sym == Keysym::j || sym == Keysym::J))
    {
        return KeyIntercept::LauncherMove(1);
    }
    if sym.raw() == Keysym::BackSpace.raw() {
        return if modifiers.ctrl {
            KeyIntercept::LauncherClear
        } else {
            KeyIntercept::LauncherBackspace
        };
    }
    if modifiers.ctrl && (sym == Keysym::u || sym == Keysym::U) {
        return KeyIntercept::LauncherClear;
    }
    if !modifiers.ctrl
        && !modifiers.alt
        && !modifiers.logo
        && let Some(text) = utf8
        && text.chars().any(|c| !c.is_control() && c != '\0')
    {
        return KeyIntercept::LauncherText(text.to_owned());
    }
    KeyIntercept::LauncherConsume
}

/// Maps keyboard modifiers and keysyms to compositor / window manager actions.
const fn match_shortcut(modifiers: ModifiersState, sym: Keysym) -> Option<KeyIntercept> {
    if sym.raw() == Keysym::Escape.raw()
        || (modifiers.ctrl && modifiers.alt && sym.raw() == Keysym::BackSpace.raw())
    {
        return Some(KeyIntercept::ExitCompositor);
    }

    // Super + Space or Alt + Space activates macOS-style Spotlight application launcher
    if (modifiers.logo || modifiers.alt) && sym.raw() == Keysym::space.raw() {
        return Some(KeyIntercept::ToggleLauncher);
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
        Keysym::l | Keysym::L => Some(KeyIntercept::Wm(WmAction::ToggleLayoutMode)),
        Keysym::m | Keysym::M => Some(KeyIntercept::Wm(WmAction::ToggleMaximizeFocused)),
        Keysym::Left => Some(KeyIntercept::Wm(WmAction::SnapFocused(
            SnapTarget::LeftHalf,
        ))),
        Keysym::Right => Some(KeyIntercept::Wm(WmAction::SnapFocused(
            SnapTarget::RightHalf,
        ))),
        Keysym::c | Keysym::C => Some(KeyIntercept::Wm(WmAction::SnapFocused(SnapTarget::Center))),
        Keysym::o | Keysym::O => Some(KeyIntercept::Wm(WmAction::MoveFocusedToNextOutput)),
        Keysym::r | Keysym::R => Some(KeyIntercept::ToggleReducedMotion),
        Keysym::s | Keysym::S => Some(KeyIntercept::ToggleQuickSettings),
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

fn compute_resized_rect(
    start: rusty_wm::Rect,
    edges: crate::shell_render::ResizeEdges,
    dx: i32,
    dy: i32,
    min_top: i32,
) -> rusty_wm::Rect {
    const MIN_W: i32 = 240;
    const MIN_H: i32 = 160;

    let mut x = start.x;
    let mut y = start.y;
    let mut w = start.width;
    let mut h = start.height;

    if edges.right {
        w = (start.width + dx).max(MIN_W);
    } else if edges.left {
        let right_edge = start.x + start.width;
        let max_left = right_edge - MIN_W;
        x = (start.x + dx).min(max_left);
        w = right_edge - x;
    }

    if edges.bottom {
        h = (start.height + dy).max(MIN_H);
    } else if edges.top {
        let bottom_edge = start.y + start.height;
        let max_top = bottom_edge - MIN_H;
        let bounded_min_top = min_top.min(max_top);
        y = (start.y + dy).clamp(bounded_min_top, max_top);
        h = (bottom_edge - y).max(MIN_H);
    }

    rusty_wm::Rect::new(x, y, w, h)
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
            match_shortcut(alt, Keysym::space),
            Some(KeyIntercept::ToggleLauncher)
        );
        let logo = ModifiersState {
            logo: true,
            ..ModifiersState::default()
        };
        assert_eq!(
            match_shortcut(logo, Keysym::space),
            Some(KeyIntercept::ToggleLauncher)
        );
        assert_eq!(
            match_shortcut(ModifiersState::default(), Keysym::Escape),
            Some(KeyIntercept::ExitCompositor)
        );
    }

    #[test]
    fn launcher_key_intercept_isolates_typing_navigation_and_escape() {
        let none = ModifiersState::default();
        let ctrl = ModifiersState {
            ctrl: true,
            ..ModifiersState::default()
        };
        let logo = ModifiersState {
            logo: true,
            ..ModifiersState::default()
        };

        assert_eq!(
            match_launcher_key(none, Keysym::Escape, None),
            KeyIntercept::LauncherClose
        );
        assert_eq!(
            match_launcher_key(logo, Keysym::space, Some(" ")),
            KeyIntercept::ToggleLauncher
        );
        assert_eq!(
            match_launcher_key(none, Keysym::Return, Some("\r")),
            KeyIntercept::LauncherActivate
        );
        assert_eq!(
            match_launcher_key(none, Keysym::Down, None),
            KeyIntercept::LauncherMove(1)
        );
        assert_eq!(
            match_launcher_key(none, Keysym::Up, None),
            KeyIntercept::LauncherMove(-1)
        );
        assert_eq!(
            match_launcher_key(none, Keysym::BackSpace, None),
            KeyIntercept::LauncherBackspace
        );
        assert_eq!(
            match_launcher_key(ctrl, Keysym::BackSpace, None),
            KeyIntercept::LauncherClear
        );
        assert_eq!(
            match_launcher_key(none, Keysym::a, Some("a")),
            KeyIntercept::LauncherText("a".to_owned())
        );
        assert_eq!(
            match_launcher_key(ctrl, Keysym::a, Some("\x01")),
            KeyIntercept::LauncherConsume
        );
    }

    #[test]
    fn compute_resized_rect_clamps_minimum_bounds_and_top_bar() {
        let start = rusty_wm::Rect::new(200, 100, 400, 300);

        let bottom_right = compute_resized_rect(
            start,
            crate::shell_render::ResizeEdges {
                right: true,
                bottom: true,
                ..Default::default()
            },
            -250,
            -200,
            32,
        );
        assert_eq!(bottom_right, rusty_wm::Rect::new(200, 100, 240, 160));

        let top_left = compute_resized_rect(
            start,
            crate::shell_render::ResizeEdges {
                left: true,
                top: true,
                ..Default::default()
            },
            300,
            -120,
            32,
        );
        assert_eq!(top_left.x, 200 + 400 - 240);
        assert_eq!(top_left.width, 240);
        assert_eq!(top_left.y, 32);
        assert_eq!(top_left.height, (100 + 300) - 32);
    }
}
