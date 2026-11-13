//! Keyboard shortcut classification and modal key intercepts (`RUSTYOS-002` / `RUSTYOS-031`).

use rusty_services::MediaAction;
use rusty_wm::{SnapTarget, WmAction, WorkspaceId};
use smithay::{
    backend::input::{Event, InputBackend, KeyState, KeyboardKeyEvent},
    input::keyboard::{FilterResult, Keysym, ModifiersState},
    utils::SERIAL_COUNTER,
};
use tracing::info;

use crate::state::RustyCompositorState;

use super::{KeyIntercept, PortalKeyAction, SecurityKeyAction};

impl RustyCompositorState {
    pub(super) fn handle_keyboard_event<B: InputBackend>(&mut self, event: &B::KeyboardKeyEvent) {
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
    pub(super) fn dispatch_key_intercept(
        &mut self,
        action: KeyIntercept,
        serial: smithay::utils::Serial,
    ) {
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
            KeyIntercept::LockSelectDelta(delta) => {
                let user_count = self.telemetry.snapshot().users.len().max(1);
                self.lock_power_menu_open = false;
                let next = if delta < 0 {
                    if self.lock_selected_user == 0 {
                        user_count.saturating_sub(1)
                    } else {
                        self.lock_selected_user.saturating_sub(1)
                    }
                } else {
                    (self.lock_selected_user + 1) % user_count
                };
                self.select_lock_user(next);
            }
            KeyIntercept::LockScreenConsume
            | KeyIntercept::LauncherConsume
            | KeyIntercept::SwitcherConsume => {}
            KeyIntercept::Security(action) => {
                self.dispatch_security_key_intercept(action, serial);
            }
            KeyIntercept::Portal(action) => {
                self.dispatch_portal_key_intercept(action, serial);
            }
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

    pub(super) fn dispatch_security_key_intercept(
        &mut self,
        action: SecurityKeyAction,
        serial: smithay::utils::Serial,
    ) {
        match action {
            SecurityKeyAction::LockSubmit => self.submit_lock_password(serial),
            SecurityKeyAction::LockBackspace => {
                self.lock_password.pop_char();
                self.lock_auth_error = None;
                self.shell_redraw_needed = true;
            }
            SecurityKeyAction::LockClear => {
                self.lock_password.clear();
                self.lock_auth_error = None;
                self.shell_redraw_needed = true;
            }
            SecurityKeyAction::LockText(text) => {
                self.lock_password.push_str(&text);
                self.lock_auth_error = None;
                self.shell_redraw_needed = true;
            }
            SecurityKeyAction::PolkitSubmit => self.submit_polkit_password(),
            SecurityKeyAction::PolkitCancel => self.cancel_polkit_prompt(serial),
            SecurityKeyAction::PolkitBackspace => {
                self.polkit_password.pop_char();
                if let Some(p) = self.polkit_prompt.as_mut() {
                    p.error_message = None;
                }
                self.shell_redraw_needed = true;
            }
            SecurityKeyAction::PolkitClear => {
                self.polkit_password.clear();
                if let Some(p) = self.polkit_prompt.as_mut() {
                    p.error_message = None;
                }
                self.shell_redraw_needed = true;
            }
            SecurityKeyAction::PolkitText(text) => {
                self.polkit_password.push_str(&text);
                if let Some(p) = self.polkit_prompt.as_mut() {
                    p.error_message = None;
                }
                self.shell_redraw_needed = true;
            }
            SecurityKeyAction::Consume => {}
        }
    }

    pub(super) fn dispatch_portal_key_intercept(
        &mut self,
        action: PortalKeyAction,
        serial: smithay::utils::Serial,
    ) {
        match action {
            PortalKeyAction::PrevSource => self.move_portal_selection(-1),
            PortalKeyAction::NextSource => self.move_portal_selection(1),
            PortalKeyAction::ToggleCursor => self.toggle_portal_include_cursor(),
            PortalKeyAction::Accept => self.accept_portal_prompt(serial),
            PortalKeyAction::Cancel => self.cancel_portal_prompt(serial),
            PortalKeyAction::PickColorCancel => self.cancel_portal_pick_color(),
            PortalKeyAction::Consume => {}
        }
    }
}

pub(super) fn classify_keyboard_input(
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
        return FilterResult::Intercept(match_lock_screen_key(
            state.user_config.require_password_on_lock,
            modifiers,
            sym,
            utf8,
        ));
    }
    if state.polkit_prompt.is_some() {
        if key_state != KeyState::Pressed {
            return FilterResult::Intercept(KeyIntercept::Security(SecurityKeyAction::Consume));
        }
        return FilterResult::Intercept(match_polkit_key(modifiers, sym, utf8));
    }
    if state.portal_prompt.is_some() {
        if key_state != KeyState::Pressed {
            return FilterResult::Intercept(KeyIntercept::Portal(PortalKeyAction::Consume));
        }
        return FilterResult::Intercept(match_portal_key(modifiers, sym));
    }
    if state.portal_pick_color.is_some() {
        if key_state == KeyState::Pressed && sym.raw() == Keysym::Escape.raw() {
            return FilterResult::Intercept(KeyIntercept::Portal(PortalKeyAction::PickColorCancel));
        }
        return FilterResult::Intercept(KeyIntercept::Portal(PortalKeyAction::Consume));
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

pub(super) fn match_switcher_key(modifiers: ModifiersState, sym: Keysym) -> KeyIntercept {
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

pub(super) fn match_launcher_key(
    modifiers: ModifiersState,
    sym: Keysym,
    utf8: Option<&str>,
) -> KeyIntercept {
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
pub(super) fn match_shortcut(modifiers: ModifiersState, sym: Keysym) -> Option<KeyIntercept> {
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

pub(super) const fn match_vt_switch_key(modifiers: ModifiersState, sym: Keysym) -> Option<i32> {
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

pub(super) fn extract_printable_modal_text(
    modifiers: ModifiersState,
    utf8: Option<&str>,
) -> Option<String> {
    if modifiers.ctrl || modifiers.alt || modifiers.logo {
        return None;
    }
    let text = utf8?;
    text.chars()
        .any(|c| !c.is_control() && c != '\0')
        .then(|| text.to_owned())
}

pub(super) fn match_polkit_key(
    modifiers: ModifiersState,
    sym: Keysym,
    utf8: Option<&str>,
) -> KeyIntercept {
    if modifiers.ctrl && modifiers.alt && sym.raw() == Keysym::BackSpace.raw() {
        return KeyIntercept::ExitCompositor;
    }
    if sym.raw() == Keysym::Escape.raw() {
        return KeyIntercept::Security(SecurityKeyAction::PolkitCancel);
    }
    if sym.raw() == Keysym::Return.raw() || sym.raw() == Keysym::KP_Enter.raw() {
        return KeyIntercept::Security(SecurityKeyAction::PolkitSubmit);
    }
    if sym.raw() == Keysym::BackSpace.raw() {
        return KeyIntercept::Security(if modifiers.ctrl {
            SecurityKeyAction::PolkitClear
        } else {
            SecurityKeyAction::PolkitBackspace
        });
    }
    if modifiers.ctrl && (sym == Keysym::u || sym == Keysym::U) {
        return KeyIntercept::Security(SecurityKeyAction::PolkitClear);
    }
    if let Some(text) = extract_printable_modal_text(modifiers, utf8) {
        return KeyIntercept::Security(SecurityKeyAction::PolkitText(text));
    }
    KeyIntercept::Security(SecurityKeyAction::Consume)
}

pub(super) fn match_portal_key(modifiers: ModifiersState, sym: Keysym) -> KeyIntercept {
    if modifiers.ctrl && modifiers.alt && sym.raw() == Keysym::BackSpace.raw() {
        return KeyIntercept::ExitCompositor;
    }
    let action = match sym {
        Keysym::Escape => PortalKeyAction::Cancel,
        Keysym::Return | Keysym::KP_Enter => PortalKeyAction::Accept,
        Keysym::Up | Keysym::KP_Up | Keysym::Left | Keysym::KP_Left => PortalKeyAction::PrevSource,
        Keysym::Down | Keysym::KP_Down | Keysym::Right | Keysym::KP_Right | Keysym::Tab => {
            PortalKeyAction::NextSource
        }
        Keysym::space | Keysym::c | Keysym::C => PortalKeyAction::ToggleCursor,
        _ => PortalKeyAction::Consume,
    };
    KeyIntercept::Portal(action)
}

pub(super) fn match_lock_screen_key(
    require_password: bool,
    modifiers: ModifiersState,
    sym: Keysym,
    utf8: Option<&str>,
) -> KeyIntercept {
    if !require_password {
        return match sym {
            Keysym::Return | Keysym::KP_Enter | Keysym::space | Keysym::Escape => {
                KeyIntercept::UnlockScreen
            }
            Keysym::Up | Keysym::KP_Up | Keysym::Left | Keysym::KP_Left => {
                KeyIntercept::LockSelectDelta(-1)
            }
            Keysym::Down | Keysym::KP_Down | Keysym::Right | Keysym::KP_Right | Keysym::Tab => {
                KeyIntercept::LockSelectDelta(1)
            }
            _ => KeyIntercept::LockScreenConsume,
        };
    }

    if sym.raw() == Keysym::Return.raw() || sym.raw() == Keysym::KP_Enter.raw() {
        return KeyIntercept::Security(SecurityKeyAction::LockSubmit);
    }
    if sym.raw() == Keysym::Escape.raw() {
        return KeyIntercept::Security(SecurityKeyAction::LockClear);
    }
    if sym.raw() == Keysym::BackSpace.raw() {
        return KeyIntercept::Security(if modifiers.ctrl {
            SecurityKeyAction::LockClear
        } else {
            SecurityKeyAction::LockBackspace
        });
    }
    if modifiers.ctrl && (sym == Keysym::u || sym == Keysym::U) {
        return KeyIntercept::Security(SecurityKeyAction::LockClear);
    }
    match sym {
        Keysym::Up | Keysym::KP_Up => return KeyIntercept::LockSelectDelta(-1),
        Keysym::Down | Keysym::KP_Down | Keysym::Tab => return KeyIntercept::LockSelectDelta(1),
        _ => {}
    }
    if let Some(text) = extract_printable_modal_text(modifiers, utf8) {
        return KeyIntercept::Security(SecurityKeyAction::LockText(text));
    }
    KeyIntercept::LockScreenConsume
}
/// Maps dedicated media/volume/brightness and `PrintScreen` keys (`RUSTYOS-031`).
pub(super) const fn match_hardware_shortcut(
    modifiers: ModifiersState,
    sym: Keysym,
) -> Option<KeyIntercept> {
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

pub(super) const fn keysym_to_workspace_index(sym: Keysym) -> Option<u8> {
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
