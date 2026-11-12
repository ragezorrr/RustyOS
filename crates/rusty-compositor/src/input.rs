//! Input routing and keyboard shortcuts for `rusty-compositor` (`RUSTYOS-002` / `RUSTYOS-005`).

use std::time::Instant;

use rusty_services::MediaAction;
use rusty_wm::{SnapTarget, WmAction, WorkspaceId};
use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, GestureBeginEvent,
        GestureEndEvent, GesturePinchUpdateEvent, GestureSwipeUpdateEvent, InputBackend,
        InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
        PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState},
        pointer::{
            AxisFrame, ButtonEvent, GestureHoldBeginEvent as PointerGestureHoldBeginEvent,
            GestureHoldEndEvent as PointerGestureHoldEndEvent,
            GesturePinchBeginEvent as PointerGesturePinchBeginEvent,
            GesturePinchEndEvent as PointerGesturePinchEndEvent,
            GesturePinchUpdateEvent as PointerGesturePinchUpdateEvent,
            GestureSwipeBeginEvent as PointerGestureSwipeBeginEvent,
            GestureSwipeEndEvent as PointerGestureSwipeEndEvent,
            GestureSwipeUpdateEvent as PointerGestureSwipeUpdateEvent, MotionEvent,
            RelativeMotionEvent,
        },
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size},
    wayland::pointer_constraints::{PointerConstraint, with_pointer_constraint},
};
use tracing::info;

use crate::state::RustyCompositorState;

#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyIntercept {
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
    LockScreenConsume,
    QuickSettingsClose,
    TopBarMenuClose,
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
                let sym = handle.modified_sym();
                let utf8 = sym.key_char().map(|ch| ch.to_string());
                classify_keyboard_input(
                    state,
                    state.is_keyboard_shortcuts_inhibited(),
                    *modifiers,
                    sym,
                    key_state,
                    utf8.as_deref(),
                )
            },
        );

        if let Some(action) = intercepted {
            self.dispatch_key_intercept(action, serial);
        }
        self.refresh_keyboard_layout_label();
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch_key_intercept(&mut self, action: KeyIntercept, serial: smithay::utils::Serial) {
        match action {
            KeyIntercept::ExitCompositor => {
                info!("Exit shortcut pressed; shutting down nested compositor");
                self.request_shutdown();
            }
            KeyIntercept::ToggleReducedMotion => {
                let reduced = !self.animations.reduced_motion();
                self.set_reduced_motion_pref(reduced);
                info!(
                    reduced_motion = reduced,
                    "Toggled reduced-motion preference"
                );
            }
            KeyIntercept::ToggleLauncher => {
                self.toggle_launcher();
                info!(
                    launcher_active = self.launcher_active,
                    "Toggled Spotlight application launcher overlay"
                );
            }
            KeyIntercept::ToggleQuickSettings => {
                self.toggle_quick_settings();
                info!(
                    quick_settings_active = self.quick_settings_active,
                    "Toggled Quick Settings popover"
                );
            }
            KeyIntercept::ToggleOverview => {
                self.toggle_overview();
                info!(
                    overview_active = self.overview_active,
                    "Toggled Workspace Overview"
                );
            }
            KeyIntercept::MinimizeFocused => {
                if let Some(win_id) = self
                    .wm
                    .focused_window()
                    .filter(|id| !self.is_window_minimized(*id))
                {
                    self.minimize_window(win_id, serial);
                }
            }
            KeyIntercept::TriggerNotification => {
                let id = self.trigger_demo_notification();
                info!(
                    id,
                    count = self.notification_count,
                    "Triggered Notification HUD toast"
                );
            }
            KeyIntercept::ClearNotifications => {
                self.clear_notifications();
                info!("Cleared all Notification HUD toasts");
            }
            KeyIntercept::OverviewClose => {
                self.close_overview();
                info!("Closed Workspace Overview");
            }
            KeyIntercept::DismissPopups => {
                self.dismiss_active_popup_grabs();
            }
            KeyIntercept::LockScreen => {
                self.set_screen_locked(true, serial);
                info!("Engaged desktop session Lock Screen");
            }
            KeyIntercept::UnlockScreen => {
                self.set_screen_locked(false, serial);
                info!("Unlocked desktop session Lock Screen");
            }
            KeyIntercept::LockScreenConsume
            | KeyIntercept::LauncherConsume
            | KeyIntercept::SwitcherConsume => {}
            KeyIntercept::QuickSettingsClose => {
                self.close_quick_settings();
                info!("Closed Quick Settings popover");
            }
            KeyIntercept::TopBarMenuClose => {
                self.close_top_bar_menu();
                info!("Closed TopBar menu popover");
            }
            KeyIntercept::LauncherClose => {
                self.close_launcher();
                info!("Closed Spotlight application launcher");
            }
            KeyIntercept::LauncherActivate => {
                self.launch_spotlight_selected();
            }
            KeyIntercept::LauncherMove(delta) => {
                self.launcher.move_selection(delta);
            }
            KeyIntercept::LauncherBackspace => {
                self.launcher.pop_char();
            }
            KeyIntercept::LauncherClear => {
                self.launcher.reset();
            }
            KeyIntercept::LauncherText(text) => {
                self.launcher.push_text(&text);
            }
            KeyIntercept::SwitcherStep(delta) => {
                self.switcher_step(delta);
            }
            KeyIntercept::CommitSwitcher => {
                self.switcher_commit(serial);
            }
            KeyIntercept::SwitcherCancel => {
                self.switcher_cancel();
            }
            KeyIntercept::SwitchVt(vt) => {
                self.pending_vt_switch = Some(vt);
            }
            KeyIntercept::ReleaseShortcutInhibitor => {
                self.deactivate_focused_shortcut_inhibitor();
                info!("Released active keyboard shortcut inhibitor via Super+Escape");
            }
            KeyIntercept::VolumeStep(delta) => {
                let (pct, muted) = self.telemetry.step_volume_with_osd(delta);
                self.show_osd("Volume", Some(pct), muted);
            }
            KeyIntercept::VolumeMute => {
                let (pct, muted) = self.telemetry.toggle_mute_with_osd();
                self.show_osd(
                    if muted { "Audio Muted" } else { "Volume" },
                    Some(pct),
                    muted,
                );
            }
            KeyIntercept::BrightnessStep(delta) => {
                let (available, pct) = self.telemetry.step_brightness_with_osd(delta);
                if available {
                    self.show_osd("Brightness", Some(pct), false);
                } else {
                    self.show_osd("No Backlight", None, false);
                }
            }
            KeyIntercept::Media(media_action) => {
                self.telemetry.dispatch_media(media_action);
                let label = match media_action {
                    MediaAction::PlayPause => "Play / Pause",
                    MediaAction::Next => "Next Track",
                    MediaAction::Previous => "Previous Track",
                    MediaAction::Stop => "Stop Media",
                };
                self.show_osd(label, None, false);
            }
            KeyIntercept::TakeScreenshot(req) => {
                self.pending_screenshot = Some(req);
            }
            KeyIntercept::Wm(wm_action) => {
                self.apply_wm_action(wm_action, serial);
            }
        }
    }

    fn handle_pointer_motion<B: InputBackend>(&mut self, event: &B::PointerMotionEvent) {
        let delta = event.delta();
        let delta_unaccel = event.delta_unaccel();
        let utime = Event::time(event);
        let time = Event::time_msec(event);

        if self.has_blocking_shell_overlay() {
            self.deactivate_active_pointer_constraint();
            let bounds = self.total_output_bounds();
            let pos = clamp_relative_pointer_rect(self.pointer_location, delta, bounds);
            self.apply_pointer_position(pos, time);
            return;
        }

        let under = self.target_surface_under(self.pointer_location);
        let rel_focus = under
            .as_ref()
            .map(|(_, surface, origin)| (surface.clone(), *origin));
        let pointer = self.pointer.clone();
        pointer.relative_motion(
            self,
            rel_focus,
            &RelativeMotionEvent {
                delta,
                delta_unaccel,
                utime,
            },
        );

        if let Some((_, ref surface, origin)) = under {
            let local_pt = (self.pointer_location - origin).to_i32_round();
            let mut action = None;
            with_pointer_constraint(surface, &self.pointer, |constraint| {
                let Some(c) = constraint else {
                    return;
                };
                if !c.is_active() {
                    if c.region().is_none_or(|r| r.contains(local_pt)) {
                        c.activate();
                    } else {
                        return;
                    }
                }
                action = Some(match &*c {
                    PointerConstraint::Locked(_) => true,
                    PointerConstraint::Confined(_) => false,
                });
            });

            match action {
                Some(true) => {
                    pointer.frame(self);
                    return;
                }
                Some(false) => {
                    let surface_bounds =
                        self.surface_logical_bounds(surface).unwrap_or_else(|| {
                            let total = self.total_output_bounds();
                            Rectangle::new(
                                total.loc.to_f64(),
                                Size::from((
                                    f64::from(total.size.w.max(1)),
                                    f64::from(total.size.h.max(1)),
                                )),
                            )
                        });
                    let pos = confine_pointer_to_rect(self.pointer_location, delta, surface_bounds);
                    self.apply_pointer_position(pos, time);
                    return;
                }
                None => {}
            }
        }

        let bounds = self.total_output_bounds();
        let pos = clamp_relative_pointer_rect(self.pointer_location, delta, bounds);
        self.apply_pointer_position(pos, time);
    }

    fn handle_pointer_motion_absolute<B: InputBackend>(
        &mut self,
        event: &B::PointerMotionAbsoluteEvent,
    ) {
        let bounds = self.total_output_bounds();
        let normalized: Point<f64, Logical> = event.position_transformed(bounds.size);
        let pos = Point::from((
            normalized.x + f64::from(bounds.loc.x),
            normalized.y + f64::from(bounds.loc.y),
        ));
        let time = Event::time_msec(event);
        self.apply_pointer_position(pos, time);
    }

    fn apply_pointer_position(&mut self, pos: Point<f64, Logical>, time: u32) {
        self.pointer_location = pos;

        if let Some(out_id) = self.output_under(pos) {
            self.wm.set_focused_output(out_id);
        }

        if self.ext_session_locked {
            self.set_dock_hover_slot(None);
            let serial = SERIAL_COUNTER.next_serial();
            let under = self.ext_lock_surface_under(pos);
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
            return;
        }

        if self.screen_locked {
            self.set_dock_hover_slot(None);
            let serial = SERIAL_COUNTER.next_serial();
            let pointer = self.pointer.clone();
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location: pos,
                    serial,
                    time,
                },
            );
            pointer.frame(self);
            return;
        }

        self.update_dock_hover_slot(pos);

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

    fn update_dock_hover_slot(&mut self, pos: Point<f64, Logical>) {
        let primary_entry = self.output_map.primary();
        let primary_geo = primary_entry.logical_geometry();
        #[allow(clippy::cast_possible_truncation)]
        let px = pos.x.round() as i32;
        #[allow(clippy::cast_possible_truncation)]
        let py = pos.y.round() as i32;
        let is_on_primary = primary_geo.contains((px, py));

        let next_slot = if is_on_primary
            && !self.launcher_active
            && !self.quick_settings_active
            && self.active_unoccluded_fullscreen_window().is_none()
        {
            crate::shell_render::check_dock_slot(
                primary_entry.physical_size,
                self.wm.visible_windows().len(),
                pos.x,
                pos.y,
            )
        } else {
            None
        };
        self.set_dock_hover_slot(next_slot);
    }

    fn active_top_bar_app_name(&self) -> String {
        self.wm
            .focused_window()
            .filter(|id| self.wm.visible_windows().iter().any(|w| w.id == *id))
            .map_or_else(
                || "RustyOS".to_owned(),
                |id| crate::shell_render::extract_app_name(&self.window_title(id)),
            )
    }

    fn handle_top_bar_pointer_click(
        &mut self,
        output_width: i32,
        serial: smithay::utils::Serial,
        button: u32,
    ) -> bool {
        let active_app_name = self.active_top_bar_app_name();
        let x = self.pointer_location.x;
        let y = self.pointer_location.y;
        let was_menu_open = self.active_top_bar_menu.is_some();

        if let Some(open_menu) = self.active_top_bar_menu {
            match crate::shell_render::check_top_bar_menu_popover_click(
                &active_app_name,
                open_menu,
                x,
                y,
            ) {
                crate::shell_render::TopBarMenuPopoverClick::Action(action) => {
                    self.dispatch_top_bar_menu_action(action, serial);
                    return true;
                }
                crate::shell_render::TopBarMenuPopoverClick::Inside => {
                    return true;
                }
                crate::shell_render::TopBarMenuPopoverClick::Outside => {
                    if y > f64::from(crate::shell_render::TOP_BAR_HEIGHT) {
                        self.close_top_bar_menu();
                        return true;
                    }
                }
            }
        }

        if let Some(menu_hit) =
            crate::shell_render::check_top_bar_menu_click_for_app(&active_app_name, x, y)
        {
            self.toggle_top_bar_menu(menu_hit);
            return true;
        }

        if y <= f64::from(crate::shell_render::TOP_BAR_HEIGHT) {
            self.close_top_bar_menu();
        }

        if let Some(target_ws) = crate::shell_render::check_top_bar_workspace_click_with_options(
            &active_app_name,
            self.menubar_show_workspaces,
            x,
            y,
        ) {
            let ws = WorkspaceId::new_clamped(target_ws);
            self.apply_wm_action(WmAction::SwitchWorkspace(ws), serial);
            return true;
        }
        if crate::shell_render::check_top_bar_layout_click_with_options(
            &active_app_name,
            self.menubar_show_workspaces,
            x,
            y,
        ) {
            self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            return true;
        }

        if self.handle_top_bar_right_cluster_click(output_width, button, x, y) {
            return true;
        }

        was_menu_open
    }

    fn handle_top_bar_right_cluster_click(
        &mut self,
        output_width: i32,
        button: u32,
        x: f64,
        y: f64,
    ) -> bool {
        let tel = self.telemetry.snapshot();
        let clock_text = if self.clock_24h {
            tel.clock_date_24h.as_str()
        } else {
            tel.clock_date_hhmm.as_str()
        };
        let opts = crate::shell_render::TopBarLayoutOptions {
            has_battery: tel.power.has_battery,
            notification_count: self.notification_count,
            show_volume: self.menubar_show_volume,
            show_input_source: self.menubar_show_input_source,
            show_workspaces: self.menubar_show_workspaces,
            show_tray: self.menubar_show_tray,
        };

        if crate::shell_render::check_top_bar_search_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            self.toggle_launcher();
            return true;
        }
        if crate::shell_render::check_top_bar_status_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            self.toggle_quick_settings();
            return true;
        }
        if crate::shell_render::check_top_bar_notification_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            self.clear_notifications();
            return true;
        }
        if crate::shell_render::check_top_bar_kbd_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            self.cycle_keyboard_layout();
            return true;
        }
        if let Some(tray_idx) = crate::shell_render::check_top_bar_tray_click_for_options(
            output_width,
            clock_text,
            opts,
            self.tray_items.len(),
            x,
            y,
        ) && let Some(item) = self.tray_items.get(tray_idx)
        {
            #[allow(clippy::cast_possible_truncation)]
            let click_x = x.round() as i32;
            #[allow(clippy::cast_possible_truncation)]
            let click_y = y.round() as i32;
            match button {
                272 => self.tray.activate(&item.key, click_x, click_y),
                273 => self.tray.context_menu(&item.key, click_x, click_y),
                274 => self.tray.secondary_activate(&item.key, click_x, click_y),
                _ => {}
            }
            return true;
        }

        false
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
            crate::shell_render::QuickSettingsHit::SetVolumePercent(pct) => {
                self.telemetry.set_volume_percent(pct);
            }
            crate::shell_render::QuickSettingsHit::ToggleReduceTransparency => {
                self.toggle_reduce_transparency();
            }
            crate::shell_render::QuickSettingsHit::SetGlassOpacityPercent(pct) => {
                self.set_glass_opacity(f32::from(pct) / 100.0);
            }
            crate::shell_render::QuickSettingsHit::ToggleLayout => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            crate::shell_render::QuickSettingsHit::ToggleReducedMotion => {
                let reduced = !self.animations.reduced_motion();
                self.set_reduced_motion_pref(reduced);
            }
            crate::shell_render::QuickSettingsHit::OpenSpotlight => {
                self.toggle_launcher();
            }
            crate::shell_render::QuickSettingsHit::RefreshMetrics => {
                self.telemetry.request_refresh();
            }
            crate::shell_render::QuickSettingsHit::ToggleBluetooth => {
                self.telemetry.toggle_bluetooth();
            }
            crate::shell_render::QuickSettingsHit::LockSession => {
                self.set_screen_locked(true, serial);
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

        if self.ext_session_locked {
            if button_state == ButtonState::Pressed
                && let Some((lock_surf, _)) = self.ext_lock_surface_under(self.pointer_location)
            {
                let keyboard = self.keyboard.clone();
                keyboard.set_focus(self, Some(lock_surf), serial);
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
            return;
        }

        if button_state == ButtonState::Pressed {
            if self.screen_locked {
                let output_phys = self
                    .space
                    .output_geometry(&self.output)
                    .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
                    .to_physical(1);
                if crate::shell_render::check_lock_screen_unlock_click(
                    output_phys,
                    self.pointer_location.x,
                    self.pointer_location.y,
                ) {
                    self.set_screen_locked(false, serial);
                }
                return;
            }

            let output_width = self
                .space
                .output_geometry(&self.output)
                .map_or(1280, |geo| geo.size.w);

            let shell_chrome_visible = self.active_unoccluded_fullscreen_window().is_none();

            // Check for TopBar click to open/execute menus, switch workspaces, toggle layout, or open Search / Control Center
            if shell_chrome_visible
                && self.handle_top_bar_pointer_click(output_width, serial, button)
            {
                return;
            }

            // Handle Spotlight clicks exclusively while launcher overlay is open (RUSTYOS-018b)
            if self.launcher_active {
                self.handle_launcher_pointer_click();
                return;
            }

            // Handle Alt+Tab window switcher HUD clicks (`RUSTYOS-033`)
            if let Some(ref sw) = self.window_switcher {
                let output_phys = self
                    .space
                    .output_geometry(&self.output)
                    .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
                    .to_physical(1);
                if let Some(idx) = crate::shell_render::check_switcher_click(
                    output_phys,
                    sw.window_ids.len(),
                    self.pointer_location.x,
                    self.pointer_location.y,
                ) {
                    self.switcher_commit_index(idx, serial);
                } else {
                    self.switcher_cancel();
                }
                return;
            }

            // Handle Quick Settings popover clicks exclusively while open (RUSTYOS-021)
            if self.quick_settings_active {
                self.handle_quick_settings_pointer_click(serial);
                return;
            }

            // Handle floating Notification HUD toast clicks to dismiss (`RUSTYOS-024`)
            let output_phys = self
                .space
                .output_geometry(&self.output)
                .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
                .to_physical(1);
            let visible_toast_ids: Vec<u64> = self
                .notifications
                .iter()
                .rev()
                .take(crate::shell_render::MAX_VISIBLE_NOTIFICATIONS)
                .map(|n| n.id)
                .collect();
            if let Some(toast_idx) = crate::shell_render::check_notification_click(
                output_phys,
                visible_toast_ids.len(),
                self.pointer_location.x,
                self.pointer_location.y,
            ) && let Some(&toast_id) = visible_toast_ids.get(toast_idx)
            {
                self.dismiss_notification(toast_id);
                return;
            }

            // Handle bottom Dock clicks (`RUSTYOS-023`) before Overview/windows when visible
            let ordered_ids = self.ordered_visible_window_ids();
            if shell_chrome_visible
                && let Some(dock_hit) = crate::shell_render::check_dock_click(
                    output_phys,
                    ordered_ids.len(),
                    self.pointer_location.x,
                    self.pointer_location.y,
                )
            {
                self.handle_dock_hit(dock_hit, serial);
                return;
            }

            // Handle Workspace Overview card clicks & backdrop dismissal (`RUSTYOS-023`)
            if self.overview_active {
                match crate::shell_render::check_overview_click(
                    output_phys,
                    ordered_ids.len(),
                    self.pointer_location.x,
                    self.pointer_location.y,
                ) {
                    crate::shell_render::OverviewHit::Card(idx) => {
                        self.focus_ordered_window(idx, serial);
                    }
                    crate::shell_render::OverviewHit::Dismiss => {
                        self.close_overview();
                    }
                }
                return;
            }

            // Dismiss grabbed `xdg_popup` menus on outside clicks and route popup clicks directly (`RUSTYOS-034`).
            let target_hit = self.target_surface_under(self.pointer_location);
            if self
                .active_popup_grabs
                .iter()
                .any(smithay::wayland::shell::xdg::PopupSurface::alive)
            {
                let inside_grab = target_hit
                    .as_ref()
                    .is_some_and(|(_, surf, _)| self.is_surface_in_active_popup_grab(surf));
                if !inside_grab {
                    self.dismiss_active_popup_grabs();
                }
            }
            let hits_popup = target_hit
                .as_ref()
                .is_some_and(|(_, surf, _)| self.is_surface_in_popup(surf));

            // Check for SSD titlebar, traffic-light buttons, or outer frame resize hits (RUSTYOS-019 / RUSTYOS-030)
            if !self.launcher_active && !hits_popup {
                let hit_windows: Vec<crate::shell_render::SsdHitWindow> = self
                    .wm
                    .visible_windows()
                    .into_iter()
                    .filter(|w| !self.is_window_minimized(w.id))
                    .map(|w| crate::shell_render::SsdHitWindow {
                        id: w.id,
                        rect: w.geometry,
                        has_ssd: !self.window_uses_csd(w.id),
                    })
                    .collect();
                if let Some(hit) = crate::shell_render::check_window_frame_hit(
                    &hit_windows,
                    self.pointer_location.x,
                    self.pointer_location.y,
                ) {
                    match hit {
                        crate::shell_render::SsdHit::Close(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.apply_wm_action(WmAction::CloseFocused, serial);
                        }
                        crate::shell_render::SsdHit::Minimize(win_id) => {
                            self.minimize_window(win_id, serial);
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

                // Allow Alt+Left or Super+Left anywhere on a window (especially CSD windows) to drag it
                let mods = self.keyboard.modifier_state();
                if button == 272 && (mods.alt || mods.logo) {
                    let target_id = self
                        .target_surface_under(self.pointer_location)
                        .and_then(|(win_opt, _, _)| {
                            win_opt.and_then(|w| self.window_id_for_smithay(&w))
                        })
                        .or_else(|| {
                            #[allow(clippy::cast_possible_truncation)]
                            self.wm
                                .window_under_point(
                                    self.pointer_location.x.round() as i32,
                                    self.pointer_location.y.round() as i32,
                                )
                                .filter(|id| !self.is_window_minimized(*id))
                        });
                    if let Some(win_id) = target_id {
                        self.wm.focus_and_raise(win_id);
                        self.sync_space_with_wm(serial);
                        if let Some(win) = self.wm.window(win_id) {
                            self.active_window_resize = None;
                            self.active_window_drag = Some(crate::state::WindowDragState {
                                window_id: win_id,
                                start_pointer: self.pointer_location,
                                start_origin: (win.geometry.x, win.geometry.y),
                            });
                            return;
                        }
                    }
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

    fn handle_gesture_swipe_begin<B: InputBackend>(&mut self, event: &B::GestureSwipeBeginEvent) {
        if self.screen_locked {
            self.touchpad_swipe = crate::state::TouchpadSwipeState::default();
            return;
        }
        let fingers = GestureBeginEvent::fingers(event);
        if self.begin_touchpad_swipe(fingers) {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_begin(
            self,
            &PointerGestureSwipeBeginEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                fingers,
            },
        );
    }

    fn handle_gesture_swipe_update<B: InputBackend>(&mut self, event: &B::GestureSwipeUpdateEvent) {
        if self.screen_locked {
            return;
        }
        let delta = GestureSwipeUpdateEvent::delta(event);
        if self.update_touchpad_swipe(delta) {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_update(
            self,
            &PointerGestureSwipeUpdateEvent {
                time: Event::time_msec(event),
                delta,
            },
        );
    }

    fn handle_gesture_swipe_end<B: InputBackend>(&mut self, event: &B::GestureSwipeEndEvent) {
        if self.screen_locked {
            self.touchpad_swipe = crate::state::TouchpadSwipeState::default();
            return;
        }
        let cancelled = GestureEndEvent::cancelled(event);
        let serial = SERIAL_COUNTER.next_serial();
        if self.end_touchpad_swipe(cancelled, serial) {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_end(
            self,
            &PointerGestureSwipeEndEvent {
                serial,
                time: Event::time_msec(event),
                cancelled,
            },
        );
    }

    fn handle_gesture_pinch_begin<B: InputBackend>(&mut self, event: &B::GesturePinchBeginEvent) {
        if self.screen_locked {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_begin(
            self,
            &PointerGesturePinchBeginEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                fingers: GestureBeginEvent::fingers(event),
            },
        );
    }

    fn handle_gesture_pinch_update<B: InputBackend>(&mut self, event: &B::GesturePinchUpdateEvent) {
        if self.screen_locked {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_update(
            self,
            &PointerGesturePinchUpdateEvent {
                time: Event::time_msec(event),
                delta: GesturePinchUpdateEvent::delta(event),
                scale: GesturePinchUpdateEvent::scale(event),
                rotation: GesturePinchUpdateEvent::rotation(event),
            },
        );
    }

    fn handle_gesture_pinch_end<B: InputBackend>(&mut self, event: &B::GesturePinchEndEvent) {
        if self.screen_locked {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_end(
            self,
            &PointerGesturePinchEndEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                cancelled: GestureEndEvent::cancelled(event),
            },
        );
    }

    fn handle_gesture_hold_begin<B: InputBackend>(&mut self, event: &B::GestureHoldBeginEvent) {
        if self.screen_locked {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_hold_begin(
            self,
            &PointerGestureHoldBeginEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                fingers: GestureBeginEvent::fingers(event),
            },
        );
    }

    fn handle_gesture_hold_end<B: InputBackend>(&mut self, event: &B::GestureHoldEndEvent) {
        if self.screen_locked {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_hold_end(
            self,
            &PointerGestureHoldEndEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                cancelled: GestureEndEvent::cancelled(event),
            },
        );
    }
}

fn classify_keyboard_input(
    state: &RustyCompositorState,
    shortcuts_inhibited: bool,
    modifiers: ModifiersState,
    sym: Keysym,
    key_state: KeyState,
    utf8: Option<&str>,
) -> FilterResult<KeyIntercept> {
    if key_state == KeyState::Pressed
        && let Some(vt) = match_vt_switch_key(modifiers, sym)
    {
        return FilterResult::Intercept(KeyIntercept::SwitchVt(vt));
    }
    if state.ext_session_locked {
        return FilterResult::Forward;
    }
    if state.screen_locked {
        if key_state != KeyState::Pressed {
            return FilterResult::Intercept(KeyIntercept::LockScreenConsume);
        }
        return FilterResult::Intercept(match_lock_screen_key(sym));
    }
    if shortcuts_inhibited && !state.has_blocking_shell_overlay() {
        if key_state == KeyState::Pressed && modifiers.logo && sym.raw() == Keysym::Escape.raw() {
            return FilterResult::Intercept(KeyIntercept::ReleaseShortcutInhibitor);
        }
        return FilterResult::Forward;
    }
    if key_state == KeyState::Pressed
        && let Some(hw) = match_hardware_shortcut(modifiers, sym)
    {
        return FilterResult::Intercept(hw);
    }
    if key_state != KeyState::Pressed {
        return if state.window_switcher.is_some() {
            if modifiers.alt {
                FilterResult::Intercept(KeyIntercept::SwitcherConsume)
            } else {
                FilterResult::Intercept(KeyIntercept::CommitSwitcher)
            }
        } else if state.launcher_active {
            FilterResult::Intercept(KeyIntercept::LauncherConsume)
        } else {
            FilterResult::Forward
        };
    }
    if state.window_switcher.is_some() {
        return FilterResult::Intercept(match_switcher_key(modifiers, sym));
    }
    if state.launcher_active {
        return FilterResult::Intercept(match_launcher_key(modifiers, sym, utf8));
    }
    if state.quick_settings_active && sym.raw() == Keysym::Escape.raw() {
        return FilterResult::Intercept(KeyIntercept::QuickSettingsClose);
    }
    if state.active_top_bar_menu.is_some() && sym.raw() == Keysym::Escape.raw() {
        return FilterResult::Intercept(KeyIntercept::TopBarMenuClose);
    }
    if state.overview_active && sym.raw() == Keysym::Escape.raw() {
        return FilterResult::Intercept(KeyIntercept::OverviewClose);
    }
    if !state.active_popup_grabs.is_empty() && sym.raw() == Keysym::Escape.raw() {
        return FilterResult::Intercept(KeyIntercept::DismissPopups);
    }
    if let Some(action) = match_shortcut(modifiers, sym) {
        FilterResult::Intercept(action)
    } else {
        FilterResult::Forward
    }
}

fn match_switcher_key(modifiers: ModifiersState, sym: Keysym) -> KeyIntercept {
    if modifiers.ctrl && modifiers.alt && sym.raw() == Keysym::BackSpace.raw() {
        return KeyIntercept::ExitCompositor;
    }
    match sym {
        Keysym::Escape => KeyIntercept::SwitcherCancel,
        Keysym::Return | Keysym::KP_Enter | Keysym::space => KeyIntercept::CommitSwitcher,
        Keysym::ISO_Left_Tab | Keysym::Left | Keysym::k | Keysym::K => {
            KeyIntercept::SwitcherStep(-1)
        }
        Keysym::Tab => KeyIntercept::SwitcherStep(if modifiers.shift { -1 } else { 1 }),
        Keysym::Right | Keysym::j | Keysym::J => KeyIntercept::SwitcherStep(1),
        _ => KeyIntercept::SwitcherConsume,
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
fn match_shortcut(modifiers: ModifiersState, sym: Keysym) -> Option<KeyIntercept> {
    if sym.raw() == Keysym::Escape.raw()
        || (modifiers.ctrl && modifiers.alt && sym.raw() == Keysym::BackSpace.raw())
    {
        return Some(KeyIntercept::ExitCompositor);
    }

    // Super + Space or Alt + Space activates macOS-style Spotlight application launcher
    if (modifiers.logo || modifiers.alt) && sym.raw() == Keysym::space.raw() {
        return Some(KeyIntercept::ToggleLauncher);
    }

    // Super + Tab toggles Workspace Overview (`RUSTYOS-023`)
    if modifiers.logo && sym.raw() == Keysym::Tab.raw() {
        return Some(KeyIntercept::ToggleOverview);
    }

    // Super + L or Ctrl + Alt + L engages session Lock Screen (`RUSTYOS-025`)
    if (modifiers.logo || (modifiers.ctrl && modifiers.alt)) && matches!(sym, Keysym::l | Keysym::L)
    {
        return Some(KeyIntercept::LockScreen);
    }

    if let Some(hw) = match_hardware_shortcut(modifiers, sym) {
        return Some(hw);
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
        Keysym::Tab => {
            if modifiers.shift {
                Some(KeyIntercept::SwitcherStep(-1))
            } else {
                Some(KeyIntercept::SwitcherStep(1))
            }
        }
        Keysym::ISO_Left_Tab => Some(KeyIntercept::SwitcherStep(-1)),
        Keysym::j | Keysym::J => {
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
        Keysym::f | Keysym::F => Some(KeyIntercept::Wm(WmAction::ToggleFullscreen)),
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
        Keysym::e | Keysym::E => Some(KeyIntercept::ToggleOverview),
        Keysym::h | Keysym::H => Some(KeyIntercept::MinimizeFocused),
        Keysym::n | Keysym::N => {
            if modifiers.shift {
                Some(KeyIntercept::ClearNotifications)
            } else {
                Some(KeyIntercept::TriggerNotification)
            }
        }
        _ => None,
    }
}

const fn match_vt_switch_key(modifiers: ModifiersState, sym: Keysym) -> Option<i32> {
    match sym {
        Keysym::XF86_Switch_VT_1 => Some(1),
        Keysym::XF86_Switch_VT_2 => Some(2),
        Keysym::XF86_Switch_VT_3 => Some(3),
        Keysym::XF86_Switch_VT_4 => Some(4),
        Keysym::XF86_Switch_VT_5 => Some(5),
        Keysym::XF86_Switch_VT_6 => Some(6),
        Keysym::XF86_Switch_VT_7 => Some(7),
        Keysym::XF86_Switch_VT_8 => Some(8),
        Keysym::XF86_Switch_VT_9 => Some(9),
        Keysym::XF86_Switch_VT_10 => Some(10),
        Keysym::XF86_Switch_VT_11 => Some(11),
        Keysym::XF86_Switch_VT_12 => Some(12),
        Keysym::F1 if modifiers.ctrl && modifiers.alt => Some(1),
        Keysym::F2 if modifiers.ctrl && modifiers.alt => Some(2),
        Keysym::F3 if modifiers.ctrl && modifiers.alt => Some(3),
        Keysym::F4 if modifiers.ctrl && modifiers.alt => Some(4),
        Keysym::F5 if modifiers.ctrl && modifiers.alt => Some(5),
        Keysym::F6 if modifiers.ctrl && modifiers.alt => Some(6),
        Keysym::F7 if modifiers.ctrl && modifiers.alt => Some(7),
        Keysym::F8 if modifiers.ctrl && modifiers.alt => Some(8),
        Keysym::F9 if modifiers.ctrl && modifiers.alt => Some(9),
        Keysym::F10 if modifiers.ctrl && modifiers.alt => Some(10),
        Keysym::F11 if modifiers.ctrl && modifiers.alt => Some(11),
        Keysym::F12 if modifiers.ctrl && modifiers.alt => Some(12),
        _ => None,
    }
}

const fn match_lock_screen_key(sym: Keysym) -> KeyIntercept {
    match sym {
        Keysym::Return | Keysym::KP_Enter | Keysym::space | Keysym::Escape => {
            KeyIntercept::UnlockScreen
        }
        _ => KeyIntercept::LockScreenConsume,
    }
}

/// Maps dedicated media/volume/brightness and `PrintScreen` keys (`RUSTYOS-031`).
const fn match_hardware_shortcut(modifiers: ModifiersState, sym: Keysym) -> Option<KeyIntercept> {
    let step = if modifiers.shift { 2 } else { 5 };
    match sym {
        Keysym::XF86_AudioRaiseVolume => Some(KeyIntercept::VolumeStep(step)),
        Keysym::XF86_AudioLowerVolume => Some(KeyIntercept::VolumeStep(-step)),
        Keysym::XF86_AudioMute => Some(KeyIntercept::VolumeMute),
        Keysym::XF86_MonBrightnessUp => Some(KeyIntercept::BrightnessStep(step)),
        Keysym::XF86_MonBrightnessDown => Some(KeyIntercept::BrightnessStep(-step)),
        Keysym::XF86_AudioPlay | Keysym::XF86_AudioPause => {
            Some(KeyIntercept::Media(MediaAction::PlayPause))
        }
        Keysym::XF86_AudioNext => Some(KeyIntercept::Media(MediaAction::Next)),
        Keysym::XF86_AudioPrev => Some(KeyIntercept::Media(MediaAction::Previous)),
        Keysym::XF86_AudioStop => Some(KeyIntercept::Media(MediaAction::Stop)),
        Keysym::Print => {
            if modifiers.alt {
                Some(KeyIntercept::TakeScreenshot(
                    crate::screenshot::ScreenshotRequest::FocusedWindow,
                ))
            } else {
                Some(KeyIntercept::TakeScreenshot(
                    crate::screenshot::ScreenshotRequest::FullScreen,
                ))
            }
        }
        Keysym::Sys_Req => Some(KeyIntercept::TakeScreenshot(
            crate::screenshot::ScreenshotRequest::FocusedWindow,
        )),
        Keysym::s | Keysym::S if modifiers.logo && modifiers.shift => Some(
            KeyIntercept::TakeScreenshot(crate::screenshot::ScreenshotRequest::FocusedWindow),
        ),
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

/// Clamps a relative pointer delta (`libinput` mice/trackpads) within a multi-monitor bounding rectangle (`RUSTYOS-040`).
#[must_use]
pub fn clamp_relative_pointer_rect(
    current: Point<f64, Logical>,
    delta: Point<f64, Logical>,
    bounds: smithay::utils::Rectangle<i32, Logical>,
) -> Point<f64, Logical> {
    let min_x = f64::from(bounds.loc.x);
    let max_x = f64::from(bounds.loc.x + (bounds.size.w - 1).max(0));
    let min_y = f64::from(bounds.loc.y);
    let max_y = f64::from(bounds.loc.y + (bounds.size.h - 1).max(0));
    (
        (current.x + delta.x).clamp(min_x, max_x),
        (current.y + delta.y).clamp(min_y, max_y),
    )
        .into()
}

/// Clamps relative pointer delta within a client surface's confined region (`RUSTYOS-044`).
#[must_use]
pub fn confine_pointer_to_rect(
    current: Point<f64, Logical>,
    delta: Point<f64, Logical>,
    bounds: Rectangle<f64, Logical>,
) -> Point<f64, Logical> {
    let min_x = bounds.loc.x;
    let max_x = bounds.loc.x + (bounds.size.w - 1.0).max(0.0);
    let min_y = bounds.loc.y;
    let max_y = bounds.loc.y + (bounds.size.h - 1.0).max(0.0);
    Point::from((
        (current.x + delta.x).clamp(min_x, max_x),
        (current.y + delta.y).clamp(min_y, max_y),
    ))
}

/// Clamps a relative pointer delta (`libinput` mice/trackpads) to `[0, output_geo - 1]`.
#[must_use]
pub fn clamp_relative_pointer(
    current: Point<f64, Logical>,
    delta: Point<f64, Logical>,
    output_geo: Size<i32, Logical>,
) -> Point<f64, Logical> {
    clamp_relative_pointer_rect(
        current,
        delta,
        smithay::utils::Rectangle::new((0, 0).into(), output_geo),
    )
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
        assert_eq!(
            match_lock_screen_key(Keysym::Return),
            KeyIntercept::UnlockScreen
        );
        assert_eq!(
            match_lock_screen_key(Keysym::a),
            KeyIntercept::LockScreenConsume
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
        let inside =
            confine_pointer_to_rect((500.0, 300.0).into(), (20.0, -30.0).into(), surface_rect);
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

    struct MockGestureBackend;

    impl InputBackend for MockGestureBackend {
        type Device = MockTouchpadDevice;
        type KeyboardKeyEvent = smithay::backend::input::UnusedEvent;
        type PointerAxisEvent = smithay::backend::input::UnusedEvent;
        type PointerButtonEvent = smithay::backend::input::UnusedEvent;
        type PointerMotionEvent = smithay::backend::input::UnusedEvent;
        type PointerMotionAbsoluteEvent = smithay::backend::input::UnusedEvent;
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
    fn keyboard_shortcuts_inhibit_respects_lock_isolation() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_display, _event_loop, mut compositor) =
            test_compositor_state("rusty-test-inhibit-lock")?;
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
}
