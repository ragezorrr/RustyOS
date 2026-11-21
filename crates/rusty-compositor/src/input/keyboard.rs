//! Keyboard shortcut classification and modal key intercepts (`RUSTYOS-002` / `RUSTYOS-031`).

use rusty_services::MediaAction;
use rusty_wm::{SnapTarget, WmAction, WorkspaceId};
use smithay::{
    backend::input::{Event, InputBackend, KeyState, KeyboardKeyEvent},
    input::keyboard::{FilterResult, Keysym, ModifiersState},
    utils::SERIAL_COUNTER,
};
use tracing::info;

use crate::config::{ShortcutBindings, parse_shortcut_combo};
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
            let needs_redraw = !matches!(
                action,
                KeyIntercept::LockScreenConsume
                    | KeyIntercept::LauncherConsume
                    | KeyIntercept::SwitcherConsume
                    | KeyIntercept::DesktopInlineRenameConsume
                    | KeyIntercept::SwitchVt(_)
                    | KeyIntercept::Security(SecurityKeyAction::Consume)
                    | KeyIntercept::Portal(PortalKeyAction::Consume)
            );
            self.dispatch_key_intercept(action, serial);
            if needs_redraw {
                self.shell_redraw_needed = true;
            }
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
            KeyIntercept::ToggleSettings => {
                self.open_settings();
                info!("Opened or focused System Settings");
            }
            KeyIntercept::ReloadConfig => {
                let reloaded = self.reload_user_config();
                info!(reloaded, "Processed ReloadConfig keyboard shortcut");
            }
            KeyIntercept::ContextMenuAlt(held) => {
                self.sync_context_menu_alt_modifier(held);
            }
            KeyIntercept::ContextMenuKey(sym) => {
                self.handle_context_menu_key_event(sym, serial);
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
            | KeyIntercept::SwitcherConsume
            | KeyIntercept::DesktopInlineRenameConsume => {}
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
            KeyIntercept::UpdatesPopoverClose => {
                self.close_updates_popover();
                info!("Closed Software Updates popover");
            }
            KeyIntercept::WifiPopoverClose => {
                self.close_wifi_popover();
                self.close_top_bar_quick_popover();
                info!("Closed TopBar status popover");
            }
            KeyIntercept::NotificationCenterClose => {
                self.close_notification_center();
                info!("Closed Notification Center");
            }
            KeyIntercept::TopBarMenuClose => {
                self.close_top_bar_menu();
                info!("Closed TopBar menu popover");
            }
            KeyIntercept::DockStackClose => self.close_dock_stack(),
            KeyIntercept::DockStackStep(delta) => {
                if let Some(ref mut stack) = self.dock_stack {
                    let count = stack.entries.len();
                    if count > 0 {
                        let cur = stack
                            .selected_idx
                            .or(stack.hovered_item)
                            .unwrap_or(if delta > 0 { count - 1 } else { 0 });
                        let next = (isize::try_from(cur).unwrap_or(0) + delta)
                            .rem_euclid(isize::try_from(count).unwrap_or(1));
                        let next_idx = usize::try_from(next).unwrap_or(0);
                        stack.selected_idx = Some(next_idx);
                        stack.hovered_item = Some(next_idx);
                        self.shell_redraw_needed = true;
                    }
                }
            }
            KeyIntercept::DockStackActivate => {
                let target_path = self.dock_stack.as_ref().and_then(|stack| {
                    let idx = stack.selected_idx.or(stack.hovered_item).unwrap_or(0);
                    stack.entries.get(idx).map(|e| e.path.clone())
                });
                self.close_dock_stack();
                if let Some(path) = target_path {
                    self.open_downloads_entry(&path);
                } else {
                    let downloads = crate::state::context_menu_actions::resolve_downloads_dir();
                    if !self.launch_rusty_files_dir(downloads.as_deref()) {
                        self.toggle_launcher();
                    }
                }
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
            KeyIntercept::OpenScreenshotHud => {
                self.open_screenshot_hud();
            }
            KeyIntercept::ScreenshotHudKey(raw_sym) => {
                self.handle_screenshot_hud_key(raw_sym);
            }
            KeyIntercept::Wm(wm_action) => {
                self.apply_wm_action(wm_action, serial);
            }
            KeyIntercept::DesktopStartInlineRename => {
                if self.selected_desktop_items.len() == 1
                    && let Some(path) = self.selected_desktop_items.iter().next()
                    && let Some(name) = path.file_name().and_then(|f| f.to_str())
                {
                    let len = name.chars().count();
                    self.desktop_inline_rename = Some(crate::shell_render::DesktopInlineRename {
                        target_path: path.clone(),
                        buffer: name.to_owned(),
                        cursor_pos: len,
                    });
                    self.shell_redraw_needed = true;
                }
            }
            KeyIntercept::DesktopInlineRenameCancel => {
                self.desktop_inline_rename = None;
                self.shell_redraw_needed = true;
            }
            KeyIntercept::DesktopInlineRenameCommit => {
                self.commit_desktop_inline_rename();
            }
            KeyIntercept::DesktopInlineRenameBackspace => {
                if let Some(ref mut rename) = self.desktop_inline_rename
                    && rename.cursor_pos > 0
                {
                    let mut chars: Vec<char> = rename.buffer.chars().collect();
                    rename.cursor_pos -= 1;
                    if rename.cursor_pos < chars.len() {
                        chars.remove(rename.cursor_pos);
                        rename.buffer = chars.into_iter().collect();
                        self.shell_redraw_needed = true;
                    }
                }
            }
            KeyIntercept::DesktopInlineRenameDelete => {
                if let Some(ref mut rename) = self.desktop_inline_rename {
                    let mut chars: Vec<char> = rename.buffer.chars().collect();
                    if rename.cursor_pos < chars.len() {
                        chars.remove(rename.cursor_pos);
                        rename.buffer = chars.into_iter().collect();
                        self.shell_redraw_needed = true;
                    }
                }
            }
            KeyIntercept::DesktopInlineRenameMoveLeft => {
                if let Some(ref mut rename) = self.desktop_inline_rename {
                    rename.cursor_pos = rename.cursor_pos.saturating_sub(1);
                    self.shell_redraw_needed = true;
                }
            }
            KeyIntercept::DesktopInlineRenameMoveRight => {
                if let Some(ref mut rename) = self.desktop_inline_rename {
                    let max_pos = rename.buffer.chars().count();
                    rename.cursor_pos = (rename.cursor_pos + 1).min(max_pos);
                    self.shell_redraw_needed = true;
                }
            }
            KeyIntercept::DesktopInlineRenameInsert(text) => {
                if let Some(ref mut rename) = self.desktop_inline_rename {
                    let mut chars: Vec<char> = rename.buffer.chars().collect();
                    let ins_chars: Vec<char> = text.chars().collect();
                    let ins_len = ins_chars.len();
                    chars.splice(rename.cursor_pos..rename.cursor_pos, ins_chars);
                    rename.cursor_pos += ins_len;
                    rename.buffer = chars.into_iter().collect();
                    self.shell_redraw_needed = true;
                }
            }
            KeyIntercept::DesktopMoveSelectedToTrash => {
                self.move_selected_desktop_items_to_trash();
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

#[allow(clippy::too_many_lines)]
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
    if state.screenshot_hud.is_some() {
        if key_state == KeyState::Pressed {
            return FilterResult::Intercept(KeyIntercept::ScreenshotHudKey(sym.raw()));
        }
        return FilterResult::Intercept(KeyIntercept::ScreenshotHudKey(0));
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
    if state.desktop_inline_rename.is_some() {
        if key_state != KeyState::Pressed {
            return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameConsume);
        }
        if sym.raw() == Keysym::Escape.raw() {
            return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameCancel);
        }
        if sym.raw() == Keysym::Return.raw() || sym.raw() == Keysym::KP_Enter.raw() {
            return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameCommit);
        }
        if sym.raw() == Keysym::BackSpace.raw() {
            return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameBackspace);
        }
        if sym.raw() == Keysym::Delete.raw() {
            return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameDelete);
        }
        if sym.raw() == Keysym::Left.raw() {
            return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameMoveLeft);
        }
        if sym.raw() == Keysym::Right.raw() {
            return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameMoveRight);
        }
        if let Some(text) = utf8 {
            let filtered: String = text
                .chars()
                .filter(|c| !c.is_control() && *c != '/' && *c != '\0')
                .collect();
            if !filtered.is_empty() {
                return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameInsert(filtered));
            }
        }
        return FilterResult::Intercept(KeyIntercept::DesktopInlineRenameConsume);
    }
    if state.context_menu.is_some() {
        return classify_context_menu_key(modifiers, sym, key_state);
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
    if state.updates_popover_open && sym.raw() == Keysym::Escape.raw() {
        return FilterResult::Intercept(KeyIntercept::UpdatesPopoverClose);
    }
    if (state.wifi_popover_open || state.top_bar_quick_popover.is_some())
        && sym.raw() == Keysym::Escape.raw()
    {
        return FilterResult::Intercept(KeyIntercept::WifiPopoverClose);
    }
    if state.notification_center_open && sym.raw() == Keysym::Escape.raw() {
        return FilterResult::Intercept(KeyIntercept::NotificationCenterClose);
    }
    if state.dock_stack.is_some()
        && let Some(intercept) = classify_dock_stack_key(modifiers, sym)
    {
        return FilterResult::Intercept(intercept);
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
    if state.desktop_focused
        || (state.keyboard.current_focus().is_none() && !state.has_blocking_shell_overlay())
    {
        if (sym.raw() == Keysym::Return.raw() || sym.raw() == Keysym::KP_Enter.raw())
            && !modifiers.ctrl
            && !modifiers.alt
            && !modifiers.logo
            && state.selected_desktop_items.len() == 1
        {
            return FilterResult::Intercept(KeyIntercept::DesktopStartInlineRename);
        }
        if (sym.raw() == Keysym::Delete.raw()
            || (modifiers.logo
                && (sym.raw() == Keysym::BackSpace.raw() || sym.raw() == Keysym::Delete.raw())))
            && !state.selected_desktop_items.is_empty()
        {
            return FilterResult::Intercept(KeyIntercept::DesktopMoveSelectedToTrash);
        }
    }
    if let Some(action) = match_shortcut_with_config(modifiers, sym, &state.user_config.shortcuts) {
        FilterResult::Intercept(action)
    } else {
        FilterResult::Forward
    }
}

fn classify_dock_stack_key(modifiers: ModifiersState, sym: Keysym) -> Option<KeyIntercept> {
    if sym.raw() == Keysym::Escape.raw() {
        return Some(KeyIntercept::DockStackClose);
    }
    if modifiers.ctrl || modifiers.alt || modifiers.logo {
        return None;
    }
    match sym {
        Keysym::Up | Keysym::KP_Up | Keysym::Left | Keysym::KP_Left => {
            Some(KeyIntercept::DockStackStep(-1))
        }
        Keysym::Down | Keysym::KP_Down | Keysym::Right | Keysym::KP_Right | Keysym::Tab => {
            Some(KeyIntercept::DockStackStep(1))
        }
        Keysym::Return | Keysym::KP_Enter => Some(KeyIntercept::DockStackActivate),
        _ => None,
    }
}

fn classify_context_menu_key(
    modifiers: ModifiersState,
    sym: Keysym,
    key_state: KeyState,
) -> FilterResult<KeyIntercept> {
    if matches!(
        sym,
        Keysym::Alt_L | Keysym::Alt_R | Keysym::Meta_L | Keysym::Meta_R
    ) {
        return FilterResult::Intercept(KeyIntercept::ContextMenuAlt(modifiers.alt));
    }
    if key_state == KeyState::Pressed {
        FilterResult::Intercept(KeyIntercept::ContextMenuKey(sym))
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

/// Maps keyboard modifiers and keysyms using default [`ShortcutBindings`].
#[cfg(test)]
pub(super) fn match_shortcut(modifiers: ModifiersState, sym: Keysym) -> Option<KeyIntercept> {
    match_shortcut_with_config(modifiers, sym, &ShortcutBindings::default())
}

/// Maps keyboard modifiers and keysyms against configurable `[shortcuts]` and built-in WM bindings (`RUSTYOS-131`).
pub(super) fn match_shortcut_with_config(
    modifiers: ModifiersState,
    sym: Keysym,
    shortcuts: &ShortcutBindings,
) -> Option<KeyIntercept> {
    if sym.raw() == Keysym::Escape.raw()
        || (modifiers.ctrl && modifiers.alt && sym.raw() == Keysym::BackSpace.raw())
    {
        return Some(KeyIntercept::ExitCompositor);
    }
    if let Some(hw) = match_hardware_shortcut(modifiers, sym) {
        return Some(hw);
    }
    if let Some(configured) = match_configured_shortcut(modifiers, sym, shortcuts) {
        return Some(configured);
    }
    match_builtin_wm_shortcut(modifiers, sym)
}

fn match_configured_shortcut(
    modifiers: ModifiersState,
    sym: Keysym,
    shortcuts: &ShortcutBindings,
) -> Option<KeyIntercept> {
    let bindings: [(&str, KeyIntercept); 17] = [
        (&shortcuts.launcher, KeyIntercept::ToggleLauncher),
        (&shortcuts.overview, KeyIntercept::ToggleOverview),
        (&shortcuts.lock_screen, KeyIntercept::LockScreen),
        (&shortcuts.settings, KeyIntercept::ToggleSettings),
        (&shortcuts.reload_config, KeyIntercept::ReloadConfig),
        (&shortcuts.quick_settings, KeyIntercept::ToggleQuickSettings),
        (
            &shortcuts.close_window,
            KeyIntercept::Wm(WmAction::CloseFocused),
        ),
        (
            &shortcuts.toggle_maximize,
            KeyIntercept::Wm(WmAction::ToggleMaximizeFocused),
        ),
        (
            &shortcuts.toggle_fullscreen,
            KeyIntercept::Wm(WmAction::ToggleFullscreen),
        ),
        (&shortcuts.minimize_window, KeyIntercept::MinimizeFocused),
        (
            &shortcuts.snap_left,
            KeyIntercept::Wm(WmAction::SnapFocused(SnapTarget::LeftHalf)),
        ),
        (
            &shortcuts.snap_right,
            KeyIntercept::Wm(WmAction::SnapFocused(SnapTarget::RightHalf)),
        ),
        (
            &shortcuts.snap_center,
            KeyIntercept::Wm(WmAction::SnapFocused(SnapTarget::Center)),
        ),
        (
            &shortcuts.switch_layout,
            KeyIntercept::Wm(WmAction::ToggleLayoutMode),
        ),
        (&shortcuts.reduced_motion, KeyIntercept::ToggleReducedMotion),
        (
            &shortcuts.toggle_notifications,
            KeyIntercept::TriggerNotification,
        ),
        (
            &shortcuts.clear_notifications,
            KeyIntercept::ClearNotifications,
        ),
    ];

    for (raw, intercept) in bindings {
        if let Some(combo) = parse_shortcut_combo(raw)
            && combo.matches(modifiers, sym)
        {
            return Some(intercept);
        }
    }

    let defaults = ShortcutBindings::default();
    if shortcuts.launcher == defaults.launcher
        && modifiers.alt
        && !modifiers.ctrl
        && !modifiers.logo
        && sym.raw() == Keysym::space.raw()
    {
        return Some(KeyIntercept::ToggleLauncher);
    }
    if shortcuts.overview == defaults.overview
        && modifiers.alt
        && !modifiers.ctrl
        && !modifiers.logo
        && matches!(sym, Keysym::e | Keysym::E)
    {
        return Some(KeyIntercept::ToggleOverview);
    }
    if shortcuts.lock_screen == defaults.lock_screen
        && modifiers.ctrl
        && modifiers.alt
        && !modifiers.logo
        && matches!(sym, Keysym::l | Keysym::L)
    {
        return Some(KeyIntercept::LockScreen);
    }
    if shortcuts.close_window == defaults.close_window
        && modifiers.alt
        && !modifiers.ctrl
        && !modifiers.logo
        && matches!(sym, Keysym::q | Keysym::Q)
    {
        return Some(KeyIntercept::Wm(WmAction::CloseFocused));
    }
    None
}

fn match_builtin_wm_shortcut(modifiers: ModifiersState, sym: Keysym) -> Option<KeyIntercept> {
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
        Keysym::Tab => Some(KeyIntercept::SwitcherStep(if modifiers.shift {
            -1
        } else {
            1
        })),
        Keysym::ISO_Left_Tab => Some(KeyIntercept::SwitcherStep(-1)),
        Keysym::j | Keysym::J => Some(KeyIntercept::Wm(if modifiers.shift {
            WmAction::FocusPrev
        } else {
            WmAction::FocusNext
        })),
        Keysym::k | Keysym::K => Some(KeyIntercept::Wm(WmAction::FocusPrev)),
        Keysym::o | Keysym::O => Some(KeyIntercept::Wm(WmAction::MoveFocusedToNextOutput)),
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
        Keysym::_4 | Keysym::dollar if modifiers.logo && modifiers.shift && !modifiers.ctrl => {
            Some(KeyIntercept::OpenScreenshotHud)
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedAppShortcut {
    pub(crate) ctrl_keycode: smithay::input::keyboard::Keycode,
    pub(crate) shift_keycode: Option<smithay::input::keyboard::Keycode>,
    pub(crate) target_keycode: smithay::input::keyboard::Keycode,
    pub(crate) target_layout: smithay::input::keyboard::Layout,
    pub(crate) original_layout: smithay::input::keyboard::Layout,
}

const fn app_shortcut_key_spec(
    action: crate::shell_render::AppShortcutAction,
) -> (bool, &'static [Keysym]) {
    use crate::shell_render::AppShortcutAction;
    match action {
        AppShortcutAction::NewTab => (false, &[Keysym::t, Keysym::T]),
        AppShortcutAction::NewTabShift => (true, &[Keysym::t, Keysym::T]),
        AppShortcutAction::NewWindow => (false, &[Keysym::n, Keysym::N]),
        AppShortcutAction::OpenLocation => (false, &[Keysym::l, Keysym::L]),
        AppShortcutAction::OpenFile => (false, &[Keysym::o, Keysym::O]),
        AppShortcutAction::SaveFile => (false, &[Keysym::s, Keysym::S]),
        AppShortcutAction::CloseTab => (false, &[Keysym::w, Keysym::W]),
        AppShortcutAction::CloseTabShift => (true, &[Keysym::w, Keysym::W]),
        AppShortcutAction::Undo => (false, &[Keysym::z, Keysym::Z]),
        AppShortcutAction::Redo => (true, &[Keysym::z, Keysym::Z]),
        AppShortcutAction::Cut => (false, &[Keysym::x, Keysym::X]),
        AppShortcutAction::Copy => (false, &[Keysym::c, Keysym::C]),
        AppShortcutAction::CopyShift => (true, &[Keysym::c, Keysym::C]),
        AppShortcutAction::Paste => (false, &[Keysym::v, Keysym::V]),
        AppShortcutAction::PasteShift => (true, &[Keysym::v, Keysym::V]),
        AppShortcutAction::SelectAll => (false, &[Keysym::a, Keysym::A]),
        AppShortcutAction::Find => (false, &[Keysym::f, Keysym::F]),
        AppShortcutAction::FindShift => (true, &[Keysym::f, Keysym::F]),
        AppShortcutAction::ReloadPage => (false, &[Keysym::r, Keysym::R]),
        AppShortcutAction::ZoomIn => (false, &[Keysym::equal, Keysym::plus, Keysym::KP_Add]),
        AppShortcutAction::ZoomOut => (false, &[Keysym::minus, Keysym::KP_Subtract]),
        AppShortcutAction::ZoomReset => (false, &[Keysym::_0, Keysym::KP_0]),
    }
}

fn find_key_in_layout(
    xkb: &smithay::input::keyboard::Xkb,
    layout: smithay::input::keyboard::Layout,
    targets: &[Keysym],
) -> Option<smithay::input::keyboard::Keycode> {
    for raw in 8_u32..=255_u32 {
        let kc = smithay::input::keyboard::Keycode::new(raw);
        let syms = xkb.raw_syms_for_key_in_layout(kc, layout);
        if targets.iter().any(|t| syms.contains(t)) {
            return Some(kc);
        }
    }
    None
}

fn find_key_across_layouts(
    xkb: &smithay::input::keyboard::Xkb,
    preferred: smithay::input::keyboard::Layout,
    all_layouts: &[smithay::input::keyboard::Layout],
    targets: &[Keysym],
) -> Option<(
    smithay::input::keyboard::Keycode,
    smithay::input::keyboard::Layout,
)> {
    if let Some(kc) = find_key_in_layout(xkb, preferred, targets) {
        return Some((kc, preferred));
    }
    for &layout in all_layouts {
        if layout != preferred
            && let Some(kc) = find_key_in_layout(xkb, layout, targets)
        {
            return Some((kc, layout));
        }
    }
    None
}

impl RustyCompositorState {
    pub(crate) fn resolve_app_shortcut(
        &mut self,
        action: crate::shell_render::AppShortcutAction,
    ) -> Option<ResolvedAppShortcut> {
        let (needs_shift, target_syms) = app_shortcut_key_spec(action);
        let keyboard = self.keyboard.clone();
        keyboard.with_xkb_state(self, |ctx| {
            let xkb = ctx.xkb().lock().ok()?;
            let original_layout = xkb.active_layout();
            let layouts: Vec<_> = xkb.layouts().collect();
            let (ctrl_keycode, _) = find_key_across_layouts(
                &xkb,
                original_layout,
                &layouts,
                &[Keysym::Control_L, Keysym::Control_R],
            )?;
            let shift_keycode = if needs_shift {
                Some(
                    find_key_across_layouts(
                        &xkb,
                        original_layout,
                        &layouts,
                        &[Keysym::Shift_L, Keysym::Shift_R],
                    )?
                    .0,
                )
            } else {
                None
            };
            let (target_keycode, target_layout) =
                find_key_across_layouts(&xkb, original_layout, &layouts, target_syms)?;
            Some(ResolvedAppShortcut {
                ctrl_keycode,
                shift_keycode,
                target_keycode,
                target_layout,
                original_layout,
            })
        })
    }

    fn send_synthetic_key_edge(
        &mut self,
        keyboard: &smithay::input::keyboard::KeyboardHandle<Self>,
        keycode: smithay::input::keyboard::Keycode,
        key_state: KeyState,
        time_ms: u32,
    ) {
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let (_, mods_changed) = keyboard.input_intercept(self, keycode, key_state, |_, _, _| {
            FilterResult::<()>::Forward
        });
        keyboard.input_forward(self, keycode, key_state, serial, time_ms, mods_changed);
    }

    /// Synthesizes a layout-independent `Ctrl+[Shift+]<Key>` chord to the focused client (`RUSTYOS-068`).
    pub(crate) fn dispatch_app_shortcut(
        &mut self,
        action: crate::shell_render::AppShortcutAction,
    ) -> bool {
        if self.desktop_focused || self.keyboard.current_focus().is_none() {
            return false;
        }
        let Some(seq) = self.resolve_app_shortcut(action) else {
            return false;
        };
        let keyboard = self.keyboard.clone();
        let pressed = keyboard.pressed_keys();
        let press_ctrl = !pressed.contains(&seq.ctrl_keycode);
        let press_shift = seq.shift_keycode.filter(|kc| !pressed.contains(kc));
        let press_target = !pressed.contains(&seq.target_keycode);

        #[allow(clippy::cast_possible_truncation)]
        let time_ms = self.start_time.elapsed().as_millis() as u32;

        let switched_layout = seq.target_layout != seq.original_layout;
        if switched_layout {
            keyboard.with_xkb_state(self, |mut ctx| {
                ctx.set_layout(seq.target_layout);
            });
        }

        if press_ctrl {
            self.send_synthetic_key_edge(&keyboard, seq.ctrl_keycode, KeyState::Pressed, time_ms);
        }
        if let Some(shift_kc) = press_shift {
            self.send_synthetic_key_edge(&keyboard, shift_kc, KeyState::Pressed, time_ms);
        }
        if press_target {
            self.send_synthetic_key_edge(&keyboard, seq.target_keycode, KeyState::Pressed, time_ms);
            self.send_synthetic_key_edge(
                &keyboard,
                seq.target_keycode,
                KeyState::Released,
                time_ms.saturating_add(1),
            );
        }
        if let Some(shift_kc) = press_shift {
            self.send_synthetic_key_edge(
                &keyboard,
                shift_kc,
                KeyState::Released,
                time_ms.saturating_add(1),
            );
        }
        if press_ctrl {
            self.send_synthetic_key_edge(
                &keyboard,
                seq.ctrl_keycode,
                KeyState::Released,
                time_ms.saturating_add(1),
            );
        }

        if switched_layout {
            keyboard.with_xkb_state(self, |mut ctx| {
                ctx.set_layout(seq.original_layout);
            });
        }
        true
    }
}
