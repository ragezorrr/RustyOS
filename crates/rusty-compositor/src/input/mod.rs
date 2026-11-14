//! Input routing and keyboard shortcuts for `rusty-compositor` (`RUSTYOS-002` / `RUSTYOS-005`).

use std::time::Instant;

use rusty_services::MediaAction;
use rusty_wm::WmAction;
use smithay::{
    backend::input::{InputBackend, InputEvent},
    input::pointer::MotionEvent,
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::state::RustyCompositorState;

mod gestures;
mod keyboard;
mod pointer;
mod shell_pointer;
#[cfg(test)]
mod tests;

pub use pointer::{clamp_relative_pointer, clamp_relative_pointer_rect, confine_pointer_to_rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PortalKeyAction {
    PrevSource,
    NextSource,
    ToggleCursor,
    Accept,
    Cancel,
    PickColorCancel,
    Consume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SecurityKeyAction {
    LockSubmit,
    LockBackspace,
    LockClear,
    LockText(String),
    PolkitSubmit,
    PolkitCancel,
    PolkitBackspace,
    PolkitClear,
    PolkitText(String),
    Consume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum KeyIntercept {
    ExitCompositor,
    ToggleReducedMotion,
    ToggleLauncher,
    ToggleQuickSettings,
    ToggleOverview,
    MinimizeFocused,
    TriggerNotification,
    ClearNotifications,
    OverviewClose,
    LockScreen,
    UnlockScreen,
    LockSelectDelta(i8),
    LockScreenConsume,
    Security(SecurityKeyAction),
    Portal(PortalKeyAction),
    QuickSettingsClose,
    ToggleSettings,
    SettingsClose,
    SettingsNextTab,
    SettingsPrevTab,
    ReloadConfig,
    TopBarMenuClose,
    ContextMenuAlt(bool),
    ContextMenuKey(smithay::input::keyboard::Keysym),
    LauncherClose,
    LauncherActivate,
    LauncherMove(isize),
    LauncherBackspace,
    LauncherClear,
    LauncherText(String),
    LauncherConsume,
    SwitcherStep(isize),
    CommitSwitcher,
    SwitcherCancel,
    SwitcherConsume,
    DismissPopups,
    SwitchVt(i32),
    ReleaseShortcutInhibitor,
    VolumeStep(i8),
    VolumeMute,
    BrightnessStep(i8),
    Media(MediaAction),
    TakeScreenshot(crate::screenshot::ScreenshotRequest),
    Wm(WmAction),
}

impl RustyCompositorState {
    /// Processes an input event from an [`InputBackend`] (such as Winit).
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        self.frame_timing.mark_input_event(Instant::now());
        if !matches!(
            event,
            InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. }
        ) {
            self.notify_user_activity();
        }
        match event {
            InputEvent::Keyboard { event } => self.handle_keyboard_event::<B>(&event),
            InputEvent::PointerMotion { event } => self.handle_pointer_motion::<B>(&event),
            InputEvent::PointerMotionAbsolute { event } => {
                self.handle_pointer_motion_absolute::<B>(&event);
            }
            InputEvent::PointerButton { event } => self.handle_pointer_button::<B>(&event),
            InputEvent::PointerAxis { event } => self.handle_pointer_axis::<B>(&event),
            InputEvent::GestureSwipeBegin { event } => self.handle_gesture_swipe_begin::<B>(&event),
            InputEvent::GestureSwipeUpdate { event } => {
                self.handle_gesture_swipe_update::<B>(&event);
            }
            InputEvent::GestureSwipeEnd { event } => self.handle_gesture_swipe_end::<B>(&event),
            InputEvent::GesturePinchBegin { event } => self.handle_gesture_pinch_begin::<B>(&event),
            InputEvent::GesturePinchUpdate { event } => {
                self.handle_gesture_pinch_update::<B>(&event);
            }
            InputEvent::GesturePinchEnd { event } => self.handle_gesture_pinch_end::<B>(&event),
            InputEvent::GestureHoldBegin { event } => self.handle_gesture_hold_begin::<B>(&event),
            InputEvent::GestureHoldEnd { event } => self.handle_gesture_hold_end::<B>(&event),
            _ => {}
        }
        if self.has_blocking_shell_overlay() {
            self.deactivate_active_pointer_constraint();
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
}
