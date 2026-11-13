use rusty_services::MediaAction;
use rusty_wm::{SnapTarget, WmAction, WorkspaceId};
use smithay::{
    backend::input::{
        Event, GestureBeginEvent, GestureEndEvent, GesturePinchUpdateEvent,
        GestureSwipeUpdateEvent, InputBackend, InputEvent, KeyState,
    },
    input::keyboard::{FilterResult, Keysym, ModifiersState},
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size},
};

use crate::state::RustyCompositorState;

use super::keyboard::*;
use super::pointer::*;
use super::*;

#[test]
#[allow(clippy::too_many_lines)]
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
        Some(KeyIntercept::SwitcherStep(1))
    );
    assert_eq!(
        match_shortcut(alt_shift, Keysym::Tab),
        Some(KeyIntercept::SwitcherStep(-1))
    );
    assert_eq!(
        match_shortcut(alt, Keysym::ISO_Left_Tab),
        Some(KeyIntercept::SwitcherStep(-1))
    );
    assert_eq!(
        match_shortcut(alt, Keysym::j),
        Some(KeyIntercept::Wm(WmAction::FocusNext))
    );
    assert_eq!(
        match_shortcut(alt_shift, Keysym::j),
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
    assert_eq!(
        match_shortcut(logo, Keysym::Tab),
        Some(KeyIntercept::ToggleOverview)
    );
    assert_eq!(
        match_shortcut(alt, Keysym::e),
        Some(KeyIntercept::ToggleOverview)
    );
    assert_eq!(
        match_shortcut(alt, Keysym::h),
        Some(KeyIntercept::MinimizeFocused)
    );
    assert_eq!(
        match_shortcut(alt, Keysym::n),
        Some(KeyIntercept::TriggerNotification)
    );
    assert_eq!(
        match_shortcut(alt_shift, Keysym::n),
        Some(KeyIntercept::ClearNotifications)
    );
    assert_eq!(
        match_shortcut(logo, Keysym::l),
        Some(KeyIntercept::LockScreen)
    );
    let ctrl_alt = ModifiersState {
        ctrl: true,
        alt: true,
        ..ModifiersState::default()
    };
    assert_eq!(
        match_shortcut(ctrl_alt, Keysym::l),
        Some(KeyIntercept::LockScreen)
    );
    let none = ModifiersState::default();
    assert_eq!(
        match_lock_screen_key(false, none, Keysym::Return, None),
        KeyIntercept::UnlockScreen
    );
    assert_eq!(
        match_lock_screen_key(false, none, Keysym::Down, None),
        KeyIntercept::LockSelectDelta(1)
    );
    assert_eq!(
        match_lock_screen_key(false, none, Keysym::Up, None),
        KeyIntercept::LockSelectDelta(-1)
    );
    assert_eq!(
        match_lock_screen_key(false, none, Keysym::a, Some("a")),
        KeyIntercept::LockScreenConsume
    );
    assert_eq!(
        match_lock_screen_key(true, none, Keysym::a, Some("a")),
        KeyIntercept::Security(SecurityKeyAction::LockText("a".to_owned()))
    );
    assert_eq!(
        match_lock_screen_key(true, none, Keysym::space, Some(" ")),
        KeyIntercept::Security(SecurityKeyAction::LockText(" ".to_owned()))
    );
    assert_eq!(
        match_lock_screen_key(true, none, Keysym::Return, None),
        KeyIntercept::Security(SecurityKeyAction::LockSubmit)
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

#[test]
fn clamp_relative_pointer_keeps_cursor_inside_output_bounds() {
    let bounds = Size::from((1920, 1080));
    let p = clamp_relative_pointer((10.0, 20.0).into(), (-50.0, -40.0).into(), bounds);
    assert!((p.x - 0.0).abs() < f64::EPSILON);
    assert!((p.y - 0.0).abs() < f64::EPSILON);

    let p2 = clamp_relative_pointer((1900.0, 1060.0).into(), (100.0, 80.0).into(), bounds);
    assert!((p2.x - 1919.0).abs() < f64::EPSILON);
    assert!((p2.y - 1079.0).abs() < f64::EPSILON);
}

#[test]
fn vt_switch_matching_covers_xf86_and_ctrl_alt_fn() {
    let none = ModifiersState::default();
    let ctrl_alt = ModifiersState {
        ctrl: true,
        alt: true,
        ..ModifiersState::default()
    };
    assert_eq!(match_vt_switch_key(none, Keysym::XF86_Switch_VT_1), Some(1));
    assert_eq!(
        match_vt_switch_key(ctrl_alt, Keysym::XF86_Switch_VT_3),
        Some(3)
    );
    assert_eq!(match_vt_switch_key(ctrl_alt, Keysym::F1), Some(1));
    assert_eq!(match_vt_switch_key(ctrl_alt, Keysym::F12), Some(12));
    assert_eq!(match_vt_switch_key(none, Keysym::F1), None);
}

#[test]
fn hardware_media_brightness_and_screenshot_shortcuts() {
    let none = ModifiersState::default();
    let shift = ModifiersState {
        shift: true,
        ..ModifiersState::default()
    };
    let alt = ModifiersState {
        alt: true,
        ..ModifiersState::default()
    };
    let super_shift = ModifiersState {
        logo: true,
        shift: true,
        ..ModifiersState::default()
    };

    assert_eq!(
        match_hardware_shortcut(none, Keysym::XF86_AudioRaiseVolume),
        Some(KeyIntercept::VolumeStep(5))
    );
    assert_eq!(
        match_hardware_shortcut(shift, Keysym::XF86_AudioLowerVolume),
        Some(KeyIntercept::VolumeStep(-2))
    );
    assert_eq!(
        match_hardware_shortcut(none, Keysym::XF86_AudioMute),
        Some(KeyIntercept::VolumeMute)
    );
    assert_eq!(
        match_hardware_shortcut(none, Keysym::XF86_MonBrightnessUp),
        Some(KeyIntercept::BrightnessStep(5))
    );
    assert_eq!(
        match_hardware_shortcut(shift, Keysym::XF86_MonBrightnessDown),
        Some(KeyIntercept::BrightnessStep(-2))
    );
    assert_eq!(
        match_hardware_shortcut(none, Keysym::XF86_AudioPlay),
        Some(KeyIntercept::Media(MediaAction::PlayPause))
    );
    assert_eq!(
        match_hardware_shortcut(none, Keysym::XF86_AudioNext),
        Some(KeyIntercept::Media(MediaAction::Next))
    );
    assert_eq!(
        match_hardware_shortcut(none, Keysym::Print),
        Some(KeyIntercept::TakeScreenshot(
            crate::screenshot::ScreenshotRequest::FullScreen
        ))
    );
    assert_eq!(
        match_hardware_shortcut(alt, Keysym::Print),
        Some(KeyIntercept::TakeScreenshot(
            crate::screenshot::ScreenshotRequest::FocusedWindow
        ))
    );
    assert_eq!(
        match_hardware_shortcut(super_shift, Keysym::S),
        Some(KeyIntercept::TakeScreenshot(
            crate::screenshot::ScreenshotRequest::FocusedWindow
        ))
    );
}

#[test]
fn clamp_relative_pointer_rect_allows_multi_monitor_crossing() {
    // Dual monitor layout: Monitor 1 (0, 0, 1920, 1080) + Monitor 2 (1920, 0, 1920, 1080)
    let bounds = smithay::utils::Rectangle::new((0, 0).into(), (3840, 1080).into());

    // Moving right from monitor 1 into monitor 2
    let p = clamp_relative_pointer_rect((1900.0, 500.0).into(), (100.0, 0.0).into(), bounds);
    assert_eq!(p.x, 2000.0);
    assert_eq!(p.y, 500.0);

    // Moving left back into monitor 1
    let p2 = clamp_relative_pointer_rect((2000.0, 500.0).into(), (-200.0, 0.0).into(), bounds);
    assert_eq!(p2.x, 1800.0);
    assert_eq!(p2.y, 500.0);

    // Clamping at rightmost edge of monitor 2
    let p3 = clamp_relative_pointer_rect((3800.0, 500.0).into(), (200.0, 0.0).into(), bounds);
    assert_eq!(p3.x, 3839.0);
    assert_eq!(p3.y, 500.0);
}

#[test]
fn alt_f_maps_to_toggle_fullscreen() {
    let alt = ModifiersState {
        alt: true,
        ..ModifiersState::default()
    };
    assert_eq!(
        match_shortcut(alt, Keysym::f),
        Some(KeyIntercept::Wm(WmAction::ToggleFullscreen))
    );
    assert_eq!(
        match_shortcut(alt, Keysym::F),
        Some(KeyIntercept::Wm(WmAction::ToggleFullscreen))
    );
}

#[test]
fn confine_pointer_to_rect_clamps_within_surface_bounds() {
    let surface_rect = Rectangle::new((100.0, 50.0).into(), (800.0, 600.0).into());
    let inside = confine_pointer_to_rect((500.0, 300.0).into(), (20.0, -30.0).into(), surface_rect);
    assert_eq!(inside, (520.0, 270.0).into());

    let clamp_tl =
        confine_pointer_to_rect((110.0, 60.0).into(), (-100.0, -100.0).into(), surface_rect);
    assert_eq!(clamp_tl, (100.0, 50.0).into());

    let clamp_br =
        confine_pointer_to_rect((890.0, 640.0).into(), (100.0, 100.0).into(), surface_rect);
    assert_eq!(clamp_br, (899.0, 649.0).into());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MockTouchpadDevice;

impl smithay::backend::input::Device for MockTouchpadDevice {
    fn id(&self) -> String {
        "mock-touchpad".to_owned()
    }

    fn name(&self) -> String {
        "Mock Touchpad".to_owned()
    }

    fn has_capability(&self, capability: smithay::backend::input::DeviceCapability) -> bool {
        capability == smithay::backend::input::DeviceCapability::Gesture
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }

    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

struct MockGestureBegin {
    time: u64,
    fingers: u32,
}

struct MockGestureSwipeUpdate {
    time: u64,
    dx: f64,
    dy: f64,
}

struct MockGesturePinchUpdate {
    time: u64,
    dx: f64,
    dy: f64,
    scale: f64,
    rotation: f64,
}

struct MockGestureEnd {
    time: u64,
    cancelled: bool,
}

struct MockPointerAbs {
    time: u64,
    norm_x: f64,
    norm_y: f64,
}

struct MockGestureBackend;

impl InputBackend for MockGestureBackend {
    type Device = MockTouchpadDevice;
    type KeyboardKeyEvent = smithay::backend::input::UnusedEvent;
    type PointerAxisEvent = smithay::backend::input::UnusedEvent;
    type PointerButtonEvent = smithay::backend::input::UnusedEvent;
    type PointerMotionEvent = smithay::backend::input::UnusedEvent;
    type PointerMotionAbsoluteEvent = MockPointerAbs;
    type GestureSwipeBeginEvent = MockGestureBegin;
    type GestureSwipeUpdateEvent = MockGestureSwipeUpdate;
    type GestureSwipeEndEvent = MockGestureEnd;
    type GesturePinchBeginEvent = MockGestureBegin;
    type GesturePinchUpdateEvent = MockGesturePinchUpdate;
    type GesturePinchEndEvent = MockGestureEnd;
    type GestureHoldBeginEvent = MockGestureBegin;
    type GestureHoldEndEvent = MockGestureEnd;
    type TouchDownEvent = smithay::backend::input::UnusedEvent;
    type TouchUpEvent = smithay::backend::input::UnusedEvent;
    type TouchMotionEvent = smithay::backend::input::UnusedEvent;
    type TouchCancelEvent = smithay::backend::input::UnusedEvent;
    type TouchFrameEvent = smithay::backend::input::UnusedEvent;
    type TabletToolAxisEvent = smithay::backend::input::UnusedEvent;
    type TabletToolProximityEvent = smithay::backend::input::UnusedEvent;
    type TabletToolTipEvent = smithay::backend::input::UnusedEvent;
    type TabletToolButtonEvent = smithay::backend::input::UnusedEvent;
    type SwitchToggleEvent = smithay::backend::input::UnusedEvent;
    type SpecialEvent = std::convert::Infallible;
}

impl Event<MockGestureBackend> for MockGestureBegin {
    fn time(&self) -> u64 {
        self.time
    }
    fn device(&self) -> MockTouchpadDevice {
        MockTouchpadDevice
    }
}
impl GestureBeginEvent<MockGestureBackend> for MockGestureBegin {
    fn fingers(&self) -> u32 {
        self.fingers
    }
}
impl smithay::backend::input::GestureSwipeBeginEvent<MockGestureBackend> for MockGestureBegin {}
impl smithay::backend::input::GesturePinchBeginEvent<MockGestureBackend> for MockGestureBegin {}
impl smithay::backend::input::GestureHoldBeginEvent<MockGestureBackend> for MockGestureBegin {}

impl Event<MockGestureBackend> for MockPointerAbs {
    fn time(&self) -> u64 {
        self.time
    }
    fn device(&self) -> MockTouchpadDevice {
        MockTouchpadDevice
    }
}
impl smithay::backend::input::AbsolutePositionEvent<MockGestureBackend> for MockPointerAbs {
    fn x(&self) -> f64 {
        self.norm_x
    }
    fn y(&self) -> f64 {
        self.norm_y
    }
    fn x_transformed(&self, width: i32) -> f64 {
        (self.norm_x * f64::from(width)).max(0.0)
    }
    fn y_transformed(&self, height: i32) -> f64 {
        (self.norm_y * f64::from(height)).max(0.0)
    }
}
impl smithay::backend::input::PointerMotionAbsoluteEvent<MockGestureBackend> for MockPointerAbs {}

impl Event<MockGestureBackend> for MockGestureSwipeUpdate {
    fn time(&self) -> u64 {
        self.time
    }
    fn device(&self) -> MockTouchpadDevice {
        MockTouchpadDevice
    }
}
impl GestureSwipeUpdateEvent<MockGestureBackend> for MockGestureSwipeUpdate {
    fn delta_x(&self) -> f64 {
        self.dx
    }
    fn delta_y(&self) -> f64 {
        self.dy
    }
}

impl Event<MockGestureBackend> for MockGesturePinchUpdate {
    fn time(&self) -> u64 {
        self.time
    }
    fn device(&self) -> MockTouchpadDevice {
        MockTouchpadDevice
    }
}
impl GesturePinchUpdateEvent<MockGestureBackend> for MockGesturePinchUpdate {
    fn delta_x(&self) -> f64 {
        self.dx
    }
    fn delta_y(&self) -> f64 {
        self.dy
    }
    fn scale(&self) -> f64 {
        self.scale
    }
    fn rotation(&self) -> f64 {
        self.rotation
    }
}

impl Event<MockGestureBackend> for MockGestureEnd {
    fn time(&self) -> u64 {
        self.time
    }
    fn device(&self) -> MockTouchpadDevice {
        MockTouchpadDevice
    }
}
impl GestureEndEvent<MockGestureBackend> for MockGestureEnd {
    fn cancelled(&self) -> bool {
        self.cancelled
    }
}
impl smithay::backend::input::GestureSwipeEndEvent<MockGestureBackend> for MockGestureEnd {}
impl smithay::backend::input::GesturePinchEndEvent<MockGestureBackend> for MockGestureEnd {}
impl smithay::backend::input::GestureHoldEndEvent<MockGestureBackend> for MockGestureEnd {}

type TestCompositorSetup = (
    smithay::reexports::wayland_server::Display<RustyCompositorState>,
    smithay::reexports::calloop::EventLoop<'static, RustyCompositorState>,
    RustyCompositorState,
);

fn test_compositor_state(
    socket_name: &str,
) -> Result<TestCompositorSetup, Box<dyn std::error::Error>> {
    let display = smithay::reexports::wayland_server::Display::new()?;
    let event_loop = smithay::reexports::calloop::EventLoop::try_new()?;
    let compositor = RustyCompositorState::new(
        &display,
        socket_name.to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1920, 1080).into(),
    )?;
    Ok((display, event_loop, compositor))
}

fn send_three_finger_swipe(
    compositor: &mut RustyCompositorState,
    dx: f64,
    dy: f64,
    base_time: u64,
) {
    compositor.process_input_event(InputEvent::<MockGestureBackend>::GestureSwipeBegin {
        event: MockGestureBegin {
            time: base_time,
            fingers: 3,
        },
    });
    compositor.process_input_event(InputEvent::<MockGestureBackend>::GestureSwipeUpdate {
        event: MockGestureSwipeUpdate {
            time: base_time + 10_000,
            dx,
            dy,
        },
    });
    compositor.process_input_event(InputEvent::<MockGestureBackend>::GestureSwipeEnd {
        event: MockGestureEnd {
            time: base_time + 20_000,
            cancelled: false,
        },
    });
}

#[test]
fn process_input_event_routes_all_gesture_events_and_three_finger_swipes()
-> Result<(), Box<dyn std::error::Error>> {
    let (_display, _event_loop, mut compositor) = test_compositor_state("rusty-test-gestures")?;
    assert_eq!(compositor.wm.active_workspace_id(), WorkspaceId(1));

    // 3-finger swipe left -> next workspace (Workspace 2)
    send_three_finger_swipe(&mut compositor, -120.0, 10.0, 10_000);
    assert_eq!(compositor.wm.active_workspace_id(), WorkspaceId(2));

    // 3-finger swipe up -> opens Overview; 3-finger swipe down -> closes Overview
    send_three_finger_swipe(&mut compositor, 4.0, -110.0, 40_000);
    assert!(compositor.overview_active);
    send_three_finger_swipe(&mut compositor, 0.0, 100.0, 70_000);
    assert!(!compositor.overview_active);

    // Lock-screen isolation: 3-finger swipe must NOT alter workspace/Overview while locked
    compositor.ext_session_locked = true;
    send_three_finger_swipe(&mut compositor, 140.0, -140.0, 100_000);
    assert_eq!(compositor.wm.active_workspace_id(), WorkspaceId(2));
    assert!(!compositor.overview_active);
    compositor.ext_session_locked = false;

    // Route 4-finger swipe, 2-finger pinch, and 3-finger hold cleanly to PointerHandle
    for ev in [
        InputEvent::<MockGestureBackend>::GestureSwipeBegin {
            event: MockGestureBegin {
                time: 130_000,
                fingers: 4,
            },
        },
        InputEvent::GestureSwipeUpdate {
            event: MockGestureSwipeUpdate {
                time: 140_000,
                dx: -200.0,
                dy: 0.0,
            },
        },
        InputEvent::GestureSwipeEnd {
            event: MockGestureEnd {
                time: 150_000,
                cancelled: false,
            },
        },
        InputEvent::GesturePinchBegin {
            event: MockGestureBegin {
                time: 160_000,
                fingers: 2,
            },
        },
        InputEvent::GesturePinchUpdate {
            event: MockGesturePinchUpdate {
                time: 170_000,
                dx: 1.0,
                dy: -2.0,
                scale: 1.25,
                rotation: 12.0,
            },
        },
        InputEvent::GesturePinchEnd {
            event: MockGestureEnd {
                time: 180_000,
                cancelled: false,
            },
        },
        InputEvent::GestureHoldBegin {
            event: MockGestureBegin {
                time: 190_000,
                fingers: 3,
            },
        },
        InputEvent::GestureHoldEnd {
            event: MockGestureEnd {
                time: 200_000,
                cancelled: true,
            },
        },
    ] {
        compositor.process_input_event(ev);
    }
    assert_eq!(compositor.wm.active_workspace_id(), WorkspaceId(2));
    Ok(())
}

#[test]
fn keyboard_shortcuts_inhibit_respects_lock_isolation() -> Result<(), Box<dyn std::error::Error>> {
    let (_display, _event_loop, mut compositor) = test_compositor_state("rusty-test-inhibit-lock")?;
    let logo = ModifiersState {
        logo: true,
        ..ModifiersState::default()
    };
    let ctrl_alt = ModifiersState {
        ctrl: true,
        alt: true,
        ..ModifiersState::default()
    };

    // VT switching is never bypassed by lock states or shortcut inhibitors.
    compositor.screen_locked = true;
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            ctrl_alt,
            Keysym::F2,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::SwitchVt(2))
    ));
    // With default require_password_on_lock = true, Escape stays isolated in Lock Screen
    // (clears password buffer rather than unlocking or triggering ReleaseShortcutInhibitor).
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            logo,
            Keysym::Escape,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::Security(SecurityKeyAction::LockClear))
    ));
    compositor.user_config.require_password_on_lock = false;
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            logo,
            Keysym::Escape,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::UnlockScreen)
    ));
    compositor.user_config.require_password_on_lock = true;

    compositor.screen_locked = false;
    compositor.ext_session_locked = true;
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            ctrl_alt,
            Keysym::F3,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::SwitchVt(3))
    ));
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            logo,
            Keysym::Tab,
            KeyState::Pressed,
            None
        ),
        FilterResult::Forward
    ));
    Ok(())
}

#[test]
fn keyboard_shortcuts_inhibit_preserves_vt_switch_and_super_escape()
-> Result<(), Box<dyn std::error::Error>> {
    let (_display, _event_loop, compositor) = test_compositor_state("rusty-test-inhibit-keys")?;
    let logo = ModifiersState {
        logo: true,
        ..ModifiersState::default()
    };
    let ctrl_alt = ModifiersState {
        ctrl: true,
        alt: true,
        ..ModifiersState::default()
    };

    // When inhibited, compositor shortcuts (Super+Space, bare Escape) forward to client,
    // while Ctrl+Alt+F4 VT switch and Super+Escape emergency release stay non-inhibitable.
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            logo,
            Keysym::space,
            KeyState::Pressed,
            None
        ),
        FilterResult::Forward
    ));
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            ModifiersState::default(),
            Keysym::Escape,
            KeyState::Pressed,
            None,
        ),
        FilterResult::Forward
    ));
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            logo,
            Keysym::Escape,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::ReleaseShortcutInhibitor)
    ));
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            true,
            ctrl_alt,
            Keysym::F4,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::SwitchVt(4))
    ));

    // Normal compositor shortcuts are intercepted when inhibitor is inactive.
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            false,
            logo,
            Keysym::space,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::ToggleLauncher)
    ));
    assert!(!compositor.is_keyboard_shortcuts_inhibited());

    Ok(())
}

#[test]
fn macos27_pointer_hover_slider_drag_and_overview_space_switching()
-> Result<(), Box<dyn std::error::Error>> {
    let (_display, _event_loop, mut compositor) =
        test_compositor_state("rusty-test-macos27-hover")?;
    let serial = SERIAL_COUNTER.next_serial();
    let out_size = compositor.output_map.primary().physical_size;

    // 1. Traffic-light hover tracks top-most visible SSD window and clears when pointer leaves.
    let win_id = rusty_wm::WindowId(77);
    let rect = compositor.wm.map_window(win_id, Some((640, 480)));
    let tl_pos: Point<f64, Logical> = (f64::from(rect.x) + 18.0, f64::from(rect.y) + 14.0).into();
    compositor.apply_pointer_position(tl_pos, 1);
    assert_eq!(compositor.hovered_traffic_light_window, Some(win_id));
    compositor.apply_pointer_position((10.0, 400.0).into(), 2);
    assert_eq!(compositor.hovered_traffic_light_window, None);

    // 2. Notification toast hover sets and clears `hovered_notification_idx`.
    let _ = compositor.push_notification(
        rusty_ipc::Urgency::Normal,
        "Build Complete".to_owned(),
        "All checks finished".to_owned(),
    );
    let toast_rect = crate::shell_render::notification_toast_rects(out_size, 1)[0];
    let toast_center: Point<f64, Logical> = (
        f64::from(toast_rect.loc.x + toast_rect.size.w / 2),
        f64::from(toast_rect.loc.y + toast_rect.size.h / 2),
    )
        .into();
    compositor.apply_pointer_position(toast_center, 3);
    assert_eq!(compositor.hovered_notification_idx, Some(0));

    // 3. Open menu row hover tracks active row and resets on `close_top_bar_menu`.
    compositor.toggle_top_bar_menu(crate::shell_render::TopBarMenuHit::File);
    let (pop_x, pop_y, _, _) = crate::shell_render::top_bar_menu_popover_rect(
        "RustyOS",
        crate::shell_render::TopBarMenuHit::File,
    );
    compositor.apply_pointer_position((f64::from(pop_x + 24), f64::from(pop_y + 18)).into(), 4);
    assert_eq!(compositor.top_bar_menu_hover_row, Some(0));
    compositor.close_top_bar_menu();
    assert_eq!(compositor.top_bar_menu_hover_row, None);

    // 4. Control Center Display slider drag adjusts `glass_opacity` (fallback when no backlight),
    //    and Focus capsule toggles DND cleanly.
    let qs_w = 316.min((out_size.w - 24).max(240));
    let qs_x = out_size.w - qs_w - 12;
    let qs_y = crate::shell_render::TOP_BAR_HEIGHT + 10;
    let track_left = f64::from(qs_x + 34);
    let track_right = f64::from(qs_x + qs_w - 34);
    let track_mid_x = f64::midpoint(track_left, track_right);
    let track_y = f64::from(qs_y + 327);

    compositor.toggle_quick_settings();
    compositor.pointer_location = (track_mid_x, track_y).into();
    compositor.handle_quick_settings_pointer_click(serial, 272);
    assert_eq!(
        compositor.qs_slider_drag,
        Some(crate::shell_render::QuickSettingsSliderDrag::Display)
    );
    compositor.apply_pointer_position((track_left, track_y).into(), 5);
    assert!(compositor.glass_opacity <= 0.36);
    compositor.apply_pointer_position((track_right, track_y).into(), 6);
    assert!(compositor.glass_opacity >= 0.99);

    let col_w = (qs_w - 12) / 2;
    let focus_x = f64::from(qs_x + col_w + 12 + col_w / 2);
    let focus_y = f64::from(qs_y + 240);
    compositor.pointer_location = (focus_x, focus_y).into();
    compositor.handle_quick_settings_pointer_click(serial, 272);
    assert!(compositor.focus_mode);

    compositor.close_quick_settings();
    assert_eq!(compositor.qs_slider_drag, None);

    // 5. Overview Space miniature strip reflects mapped window & switches workspace on click.
    let spaces = compositor.overview_workspace_spaces();
    assert_eq!(spaces.len(), 9);
    assert!(spaces[0].active);
    assert_eq!(spaces[0].window_count, 1);
    assert!(spaces[0].preview_rects[0].2 > 0.0);
    compositor.toggle_overview();
    assert!(compositor.overview_active);
    let space3_rect = crate::shell_render::overview_space_rects(out_size, 9)[2];
    compositor.pointer_location = (
        f64::from(space3_rect.loc.x + space3_rect.size.w / 2),
        f64::from(space3_rect.loc.y + space3_rect.size.h / 2),
    )
        .into();
    compositor.handle_overview_pointer_click(out_size, 1, serial);
    assert_eq!(compositor.wm.active_workspace_id(), WorkspaceId(3));
    assert!(!compositor.overview_active);

    // 6. Locking session clears all transient hover and drag states.
    compositor.apply_wm_action(WmAction::SwitchWorkspace(WorkspaceId(1)), serial);
    compositor.apply_pointer_position(tl_pos, 7);
    assert_eq!(compositor.hovered_traffic_light_window, Some(win_id));
    compositor.set_screen_locked(true, serial);
    assert_eq!(compositor.hovered_traffic_light_window, None);
    assert_eq!(compositor.hovered_notification_idx, None);
    assert_eq!(compositor.top_bar_menu_hover_row, None);
    assert_eq!(compositor.qs_slider_drag, None);
    assert!(!compositor.lock_power_menu_open);

    Ok(())
}

#[test]
fn lock_screen_pointer_power_menu_and_unlock() -> Result<(), Box<dyn std::error::Error>> {
    let (_display, _event_loop, mut compositor) = test_compositor_state("rusty-test-lock-pointer")?;
    let serial = SERIAL_COUNTER.next_serial();
    let out_size = compositor.output_map.primary().physical_size;
    compositor.set_screen_locked(true, serial);

    // Toggle power menu via top-right power symbol and dismiss by clicking outside.
    let pwr_pos: Point<f64, Logical> = (f64::from(out_size.w - 20), 18.0).into();
    compositor.apply_pointer_position(pwr_pos, 1);
    compositor.handle_lock_screen_pointer_click(out_size, serial);
    assert!(compositor.lock_power_menu_open);
    compositor.apply_pointer_position(Point::from((40.0, 40.0)), 2);
    compositor.handle_lock_screen_pointer_click(out_size, serial);
    assert!(!compositor.lock_power_menu_open);

    // With default require_password_on_lock = true, clicking the empty row must stay locked.
    let users_snap = compositor.telemetry.snapshot().users;
    let user_names: Vec<&str> = users_snap.iter().map(|u| u.display_name.as_str()).collect();
    let (ux, uy) = crate::shell_render::lock_screen_primary_row_center(
        out_size,
        if user_names.is_empty() {
            &["RustyOS User"]
        } else {
            &user_names
        },
    );
    compositor.apply_pointer_position((ux, uy).into(), 3);
    compositor.handle_lock_screen_pointer_click(out_size, serial);
    assert!(compositor.screen_locked);

    // When password-on-lock is disabled via [security], clicking row 0 unlocks immediately.
    compositor.user_config.require_password_on_lock = false;
    compositor.handle_lock_screen_pointer_click(out_size, serial);
    assert!(!compositor.screen_locked);

    Ok(())
}

#[test]
fn lock_screen_password_auth_and_logind_sleep_flow() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, mpsc};

    let (_display, _event_loop, mut compositor) =
        test_compositor_state("rusty-test-lock-auth-logind")?;
    let serial = SERIAL_COUNTER.next_serial();

    compositor.auth =
        rusty_services::AsyncAuthHandle::with_verifier(Arc::new(|_username, secret| {
            if secret.as_bytes() == b"correct-horse" {
                rusty_services::AuthOutcome::Verified
            } else {
                rusty_services::AuthOutcome::InvalidCredentials
            }
        }));
    let (logind_tx, logind_rx) = mpsc::channel();
    compositor.logind = rusty_services::LogindMonitorHandle::from_receiver(logind_rx);

    // 2. systemd-logind PrepareForSleep(true) locks session automatically when lock_on_sleep = true.
    logind_tx.send(rusty_services::LogindEvent::PrepareForSleep {
        sleep_starting: true,
    })?;
    assert!(compositor.drain_system_integration_events(serial));
    assert!(compositor.screen_locked);

    // 3. Typing wrong password stays locked and clears secret buffer after async rejection.
    let mods = ModifiersState::default();
    assert!(matches!(
        classify_keyboard_input(&compositor, false, mods, Keysym::b, KeyState::Pressed, Some("b")),
        FilterResult::Intercept(KeyIntercept::Security(SecurityKeyAction::LockText(ref s))) if s == "b"
    ));
    compositor.dispatch_key_intercept(
        KeyIntercept::Security(SecurityKeyAction::LockText("wrong".to_owned())),
        serial,
    );
    assert_eq!(compositor.lock_password.char_len(), 5);
    compositor.dispatch_key_intercept(
        KeyIntercept::Security(SecurityKeyAction::LockSubmit),
        serial,
    );
    for _ in 0..40 {
        if compositor.drain_system_integration_events(serial) && !compositor.lock_auth_checking {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(compositor.screen_locked);
    assert!(!compositor.lock_auth_checking);
    assert!(compositor.lock_password.is_empty());
    assert_eq!(
        compositor.lock_auth_error.as_deref(),
        Some("Incorrect password")
    );

    // 4. Typing correct password unlocks session asynchronously and clears error/secret.
    compositor.dispatch_key_intercept(
        KeyIntercept::Security(SecurityKeyAction::LockText("correct-horse".to_owned())),
        serial,
    );
    compositor.dispatch_key_intercept(
        KeyIntercept::Security(SecurityKeyAction::LockSubmit),
        serial,
    );
    for _ in 0..40 {
        if compositor.drain_system_integration_events(serial) && !compositor.screen_locked {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!compositor.screen_locked);
    assert!(compositor.lock_password.is_empty());
    assert_eq!(compositor.lock_auth_error, None);

    // 5. logind SessionLock / SessionUnlock transitions work deterministically.
    logind_tx.send(rusty_services::LogindEvent::SessionLock)?;
    compositor.drain_system_integration_events(serial);
    assert!(compositor.screen_locked);
    logind_tx.send(rusty_services::LogindEvent::SessionUnlock)?;
    compositor.drain_system_integration_events(serial);
    assert!(!compositor.screen_locked);

    Ok(())
}

#[test]
fn polkit_dialog_captures_input_submits_and_cancels() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::mpsc;

    let (_display, _event_loop, mut compositor) = test_compositor_state("rusty-test-polkit-modal")?;
    let serial = SERIAL_COUNTER.next_serial();
    let (polkit_ev_tx, polkit_ev_rx) = mpsc::channel();
    let (polkit_reply_tx, polkit_reply_rx) = mpsc::channel();
    compositor.polkit =
        rusty_services::PolkitAgentHandle::with_test_channels(polkit_ev_rx, polkit_reply_tx);

    // 6. Polkit dialog intercepts keys exclusively, submits password, and supports cancellation.
    polkit_ev_tx.send(rusty_services::PolkitEvent::BeginPrompt(
        rusty_services::PolkitPromptRequest {
            cookie: "cookie-42".to_owned(),
            action_id: "org.freedesktop.policykit.exec".to_owned(),
            message: "Authentication is needed to run root shell".to_owned(),
            icon_name: String::new(),
            username: "root".to_owned(),
            uid: 0,
            caller_pid: Some(1234),
            prompt_label: "Password:".to_owned(),
            echo_on: false,
        },
    ))?;
    compositor.drain_system_integration_events(serial);
    assert!(compositor.polkit_prompt.is_some());
    assert!(compositor.has_blocking_shell_overlay());

    compositor.dispatch_key_intercept(
        KeyIntercept::Security(SecurityKeyAction::PolkitText("sudo-pass".to_owned())),
        serial,
    );
    compositor.dispatch_key_intercept(
        KeyIntercept::Security(SecurityKeyAction::PolkitSubmit),
        serial,
    );
    let reply = polkit_reply_rx.try_recv()?;
    match reply {
        rusty_services::PolkitUserReply::Submit { cookie, secret } => {
            assert_eq!(cookie, "cookie-42");
            assert_eq!(secret.as_bytes(), b"sudo-pass");
        }
        rusty_services::PolkitUserReply::Cancel { .. } => {
            return Err("expected Submit reply, got Cancel".into());
        }
    }
    assert!(compositor.polkit_password.is_empty());

    polkit_ev_tx.send(rusty_services::PolkitEvent::PromptError {
        cookie: "cookie-42".to_owned(),
        message: "Authentication failure".to_owned(),
    })?;
    compositor.drain_system_integration_events(serial);
    assert_eq!(
        compositor
            .polkit_prompt
            .as_ref()
            .and_then(|p| p.error_message.as_deref()),
        Some("Authentication failure")
    );

    let mods = ModifiersState::default();
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            false,
            mods,
            Keysym::Escape,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::Security(SecurityKeyAction::PolkitCancel))
    ));
    compositor.dispatch_key_intercept(
        KeyIntercept::Security(SecurityKeyAction::PolkitCancel),
        serial,
    );
    assert!(compositor.polkit_prompt.is_none());
    match polkit_reply_rx.try_recv()? {
        rusty_services::PolkitUserReply::Cancel { cookie } => {
            assert_eq!(cookie, "cookie-42");
        }
        rusty_services::PolkitUserReply::Submit { .. } => {
            return Err("expected Cancel reply, got Submit".into());
        }
    }

    Ok(())
}

fn sample_screencast_prompt_event(
    request_id: u64,
    app_id: &str,
    allowed_types: u32,
    persist_mode: u32,
) -> rusty_services::PortalEvent {
    rusty_services::PortalEvent::ShowPrompt(rusty_services::PortalPromptRequest {
        request_id,
        request_handle: format!("/org/freedesktop/portal/desktop/request/{request_id}"),
        app_id: app_id.to_owned(),
        parent_window: String::new(),
        kind: rusty_services::PortalPromptKind::ScreenCast {
            session_handle: format!("/org/freedesktop/portal/desktop/session/{request_id}"),
            allowed_types,
            multiple: false,
            cursor_mode: rusty_services::SCREENCAST_CURSOR_HIDDEN,
            persist_mode,
            restored_target: None,
        },
    })
}

#[test]
fn portal_prompt_screencast_flow_and_lock_isolation() -> Result<(), Box<dyn std::error::Error>> {
    let mut display: smithay::reexports::wayland_server::Display<RustyCompositorState> =
        smithay::reexports::wayland_server::Display::new()?;
    let event_loop: smithay::reexports::calloop::EventLoop<RustyCompositorState> =
        smithay::reexports::calloop::EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-portal-ui".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let (ev_tx, ev_rx) = std::sync::mpsc::channel();
    let (rep_tx, rep_rx) = std::sync::mpsc::channel();
    compositor.portal = rusty_services::PortalHandle::with_test_channels_and_replies(
        ev_tx.clone(),
        ev_rx,
        Some(rep_tx),
    );
    let serial = SERIAL_COUNTER.next_serial();

    // 1. Lock Screen immediately denies portal prompts.
    compositor.set_screen_locked(true, serial);
    ev_tx.send(sample_screencast_prompt_event(
        10,
        "obs",
        rusty_services::SCREENCAST_SOURCE_MONITOR,
        0,
    ))?;
    compositor.drain_system_integration_events(serial);
    assert!(compositor.portal_prompt.is_none());
    assert!(matches!(
        rep_rx.try_recv()?,
        rusty_services::PortalReply::PromptDecision {
            request_id: 10,
            accepted: false,
            selection: None
        }
    ));

    // 2. Unlocked: ScreenCast prompt accepts keyboard cursor toggle + Return approval.
    compositor.set_screen_locked(false, serial);
    ev_tx.send(sample_screencast_prompt_event(
        12,
        "firefox",
        rusty_services::SCREENCAST_SOURCE_MONITOR | rusty_services::SCREENCAST_SOURCE_WINDOW,
        1,
    ))?;
    compositor.drain_system_integration_events(serial);
    assert!(compositor.portal_prompt.is_some());
    assert!(compositor.has_blocking_shell_overlay());

    let mods = ModifiersState::default();
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            false,
            mods,
            Keysym::space,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::Portal(PortalKeyAction::ToggleCursor))
    ));
    compositor.dispatch_key_intercept(KeyIntercept::Portal(PortalKeyAction::ToggleCursor), serial);
    assert!(
        compositor
            .portal_prompt
            .as_ref()
            .is_some_and(|p| p.include_cursor)
    );

    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            false,
            mods,
            Keysym::Return,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::Portal(PortalKeyAction::Accept))
    ));
    compositor.dispatch_key_intercept(KeyIntercept::Portal(PortalKeyAction::Accept), serial);
    assert!(compositor.portal_prompt.is_none());
    match rep_rx.try_recv()? {
        rusty_services::PortalReply::PromptDecision {
            request_id: 12,
            accepted: true,
            selection:
                Some(rusty_services::PortalSelection::ScreenCast {
                    source_type: rusty_services::SCREENCAST_SOURCE_MONITOR,
                    cursor_mode: rusty_services::SCREENCAST_CURSOR_EMBEDDED,
                    ..
                }),
        } => {}
        other => return Err(format!("unexpected portal reply: {other:?}").into()),
    }

    let _ = display.flush_clients();
    Ok(())
}

#[test]
fn portal_pick_color_lock_isolation_and_escape_cancel() -> Result<(), Box<dyn std::error::Error>> {
    let mut display: smithay::reexports::wayland_server::Display<RustyCompositorState> =
        smithay::reexports::wayland_server::Display::new()?;
    let event_loop: smithay::reexports::calloop::EventLoop<RustyCompositorState> =
        smithay::reexports::calloop::EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-portal-pick".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let (ev_tx, ev_rx) = std::sync::mpsc::channel();
    let (rep_tx, rep_rx) = std::sync::mpsc::channel();
    compositor.portal = rusty_services::PortalHandle::with_test_channels_and_replies(
        ev_tx.clone(),
        ev_rx,
        Some(rep_tx),
    );
    let serial = SERIAL_COUNTER.next_serial();

    // 1. Lock Screen immediately rejects PickColor.
    compositor.set_screen_locked(true, serial);
    ev_tx.send(rusty_services::PortalEvent::PickColor {
        request_id: 11,
        app_id: "gimp".to_owned(),
    })?;
    compositor.drain_system_integration_events(serial);
    assert!(compositor.portal_pick_color.is_none());
    assert!(matches!(
        rep_rx.try_recv()?,
        rusty_services::PortalReply::PickColorResult {
            request_id: 11,
            result: Err(_)
        }
    ));

    // 2. Unlocked PickColor intercepts Escape cleanly.
    compositor.set_screen_locked(false, serial);
    ev_tx.send(rusty_services::PortalEvent::PickColor {
        request_id: 13,
        app_id: "kcolorchooser".to_owned(),
    })?;
    compositor.drain_system_integration_events(serial);
    assert!(compositor.portal_pick_color.is_some());
    let mods = ModifiersState::default();
    assert!(matches!(
        classify_keyboard_input(
            &compositor,
            false,
            mods,
            Keysym::Escape,
            KeyState::Pressed,
            None
        ),
        FilterResult::Intercept(KeyIntercept::Portal(PortalKeyAction::PickColorCancel))
    ));
    compositor.dispatch_key_intercept(
        KeyIntercept::Portal(PortalKeyAction::PickColorCancel),
        serial,
    );
    assert!(compositor.portal_pick_color.is_none());
    assert!(matches!(
        rep_rx.try_recv()?,
        rusty_services::PortalReply::PickColorResult {
            request_id: 13,
            result: Err(_)
        }
    ));

    // 3. Locking mid-PickColor revokes the active picker immediately.
    ev_tx.send(rusty_services::PortalEvent::PickColor {
        request_id: 14,
        app_id: "kcolorchooser".to_owned(),
    })?;
    compositor.drain_system_integration_events(serial);
    assert!(compositor.portal_pick_color.is_some());
    compositor.set_screen_locked(true, serial);
    assert!(compositor.portal_pick_color.is_none());
    assert!(matches!(
        rep_rx.try_recv()?,
        rusty_services::PortalReply::PickColorResult {
            request_id: 14,
            result: Err(_)
        }
    ));

    let _ = display.flush_clients();
    Ok(())
}

#[test]
fn transform_screen_point_handles_all_rotations_and_abs_events()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::config::DisplayRotation;
    let sz = Size::<i32, Logical>::from((1280, 800));

    assert_eq!(
        transform_screen_point_to_output_logical(
            (100.0, 50.0).into(),
            sz,
            DisplayRotation::Rotate0
        ),
        (100.0, 50.0).into()
    );
    assert_eq!(
        transform_screen_point_to_output_logical(
            (100.0, 50.0).into(),
            sz,
            DisplayRotation::Rotate180
        ),
        (1180.0, 750.0).into()
    );
    assert_eq!(
        transform_screen_point_to_output_logical(
            (100.0, 50.0).into(),
            sz,
            DisplayRotation::Rotate90
        ),
        (750.0, 100.0).into()
    );
    assert_eq!(
        transform_screen_point_to_output_logical(
            (100.0, 50.0).into(),
            sz,
            DisplayRotation::Rotate270
        ),
        (50.0, 1180.0).into()
    );

    let (_display, _event_loop, mut compositor) =
        test_compositor_state("rusty-test-rotate-pointer")?;
    assert!(compositor.set_output_rotation(
        crate::state::PRIMARY_OUTPUT_ID,
        DisplayRotation::Rotate180,
        true,
    ));
    assert_eq!(
        compositor.output.current_transform(),
        smithay::utils::Transform::Flipped
    );

    // Moving to (90%, 90%) of rotated 180° 1920x1080 screen hits (10%, 10%) => (192, 108).
    compositor.process_input_event(InputEvent::<MockGestureBackend>::PointerMotionAbsolute {
        event: MockPointerAbs {
            time: 10,
            norm_x: 0.9,
            norm_y: 0.9,
        },
    });
    assert!((compositor.pointer_location.x - 192.0).abs() < 1e-4);
    assert!((compositor.pointer_location.y - 108.0).abs() < 1e-4);

    // Switching to 90° swaps logical dimensions to 1080x1920 and maps normalized raw screen input.
    assert!(compositor.set_output_rotation(
        crate::state::PRIMARY_OUTPUT_ID,
        DisplayRotation::Rotate90,
        true,
    ));
    assert_eq!(
        compositor.output_map.primary().logical_geometry().size,
        (1080, 1920).into()
    );
    compositor.process_input_event(InputEvent::<MockGestureBackend>::PointerMotionAbsolute {
        event: MockPointerAbs {
            time: 11,
            norm_x: 0.25,
            norm_y: 0.75,
        },
    });
    assert!((compositor.pointer_location.x - 270.0).abs() < 1e-4);
    assert!((compositor.pointer_location.y - 480.0).abs() < 1e-4);

    Ok(())
}
