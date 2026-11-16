//! Shell UI overlay actions, launcher, overview, notifications, and OSD helpers.

use std::time::Duration;

use rusty_animation::{Easing, MotionTokens};
use rusty_ipc::Urgency;
use rusty_wm::{WindowId, WmAction};
use smithay::utils::Serial;
use tracing::{info, warn};

use super::{CompositorNotification, OsdBanner, RustyCompositorState, TouchpadSwipeState};

pub(in crate::state) struct DockWindowRecord {
    pub(in crate::state) id: WindowId,
    pub(in crate::state) title: String,
    pub(in crate::state) app_key: String,
    pub(in crate::state) minimized: bool,
    pub(in crate::state) focused: bool,
    pub(in crate::state) icon_name: Option<String>,
    pub(in crate::state) toplevel_icon: Option<crate::icons::ShmToplevelIcon>,
}

impl RustyCompositorState {
    /// Requests a clean shutdown of the compositor event loop.
    pub fn request_shutdown(&mut self) {
        if self.running {
            info!("Shutdown requested; stopping compositor loop");
            self.running = false;
            self.loop_signal.stop();
        }
    }

    /// Toggles the Spotlight application launcher overlay and resets query state.
    pub fn toggle_launcher(&mut self) {
        self.launcher_active = !self.launcher_active;
        if self.launcher_active {
            self.close_dock_stack();
            self.close_context_menu();
            self.close_top_bar_menu();
            self.window_switcher = None;
            self.close_quick_settings();
            self.close_updates_popover();
            self.close_overview();
            self.close_settings();
        }
        self.launcher.reset();
        self.retarget_launcher_anim();
    }

    /// Closes the Spotlight overlay and clears its active search query.
    pub fn close_launcher(&mut self) {
        self.launcher_active = false;
        self.launcher.reset();
        self.retarget_launcher_anim();
    }

    /// Toggles the `TopBar` Quick Settings popover and triggers an async telemetry refresh when opened.
    pub fn toggle_quick_settings(&mut self) {
        self.quick_settings_active = !self.quick_settings_active;
        self.qs_slider_drag = None;
        if self.quick_settings_active {
            self.close_dock_stack();
            self.close_context_menu();
            self.close_top_bar_menu();
            self.close_updates_popover();
            self.window_switcher = None;
            self.close_launcher();
            self.close_overview();
            self.close_settings();
            self.telemetry.request_refresh();
        }
        self.retarget_quick_settings_anim();
    }

    /// Closes the Quick Settings popover.
    pub fn close_quick_settings(&mut self) {
        self.quick_settings_active = false;
        self.qs_slider_drag = None;
        self.retarget_quick_settings_anim();
    }

    /// Returns whether the Software Updates indicator is currently shown in the Menu Bar (`RUSTYOS-076`).
    #[must_use]
    pub const fn should_show_top_bar_updates(&self) -> bool {
        self.updates_popover_open
            || (self.menubar_show_updates && self.updates_status.total_count > 0)
    }

    /// Toggles the `TopBar` Software Updates popover (`RUSTYOS-076`).
    pub fn toggle_updates_popover(&mut self) {
        if self.updates_popover_open {
            self.close_updates_popover();
        } else {
            let trigger_refresh =
                self.updates_status.state == rusty_services::UpdateCheckState::Idle;
            self.open_updates_popover(trigger_refresh);
        }
    }

    /// Opens the `TopBar` Software Updates popover and optionally starts a fresh check (`RUSTYOS-076`).
    pub fn open_updates_popover(&mut self, trigger_refresh: bool) {
        if self.is_session_locked() {
            return;
        }
        self.updates_popover_open = true;
        self.updates_popover_hover = None;
        self.close_dock_stack();
        self.close_context_menu();
        self.close_top_bar_menu();
        self.window_switcher = None;
        self.close_quick_settings();
        self.close_launcher();
        self.close_overview();
        self.close_settings();
        if trigger_refresh {
            self.trigger_updates_check();
        }
        self.shell_redraw_needed = true;
    }

    /// Closes the `TopBar` Software Updates popover (`RUSTYOS-076`).
    pub fn close_updates_popover(&mut self) {
        if self.updates_popover_open || self.updates_popover_hover.is_some() {
            self.updates_popover_open = false;
            self.updates_popover_hover = None;
            self.shell_redraw_needed = true;
        }
    }

    /// Requests an immediate asynchronous `checkupdates` refresh (`RUSTYOS-076`).
    pub fn trigger_updates_check(&mut self) {
        self.updates_status.state = rusty_services::UpdateCheckState::Checking;
        self.updates_checker.trigger_check_now();
        self.shell_redraw_needed = true;
    }

    /// Launches `rusty-updater` when installed, or shows a clean guidance notice (`RUSTYOS-076`/`RUSTYOS-077`).
    pub fn open_software_updater_action(&mut self) {
        self.close_updates_popover();
        if crate::launcher::resolve_program_path("rusty-updater").is_some() {
            let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
            if self
                .launcher
                .launch_program_with_args("rusty-updater", &[], &self.socket_name, &ipc_path)
                .is_ok()
            {
                self.trigger_app_bounce("updater", false);
                return;
            }
        }
        let body = if self.updates_status.total_count > 0 {
            format!(
                "{} updates ready · run 'sudo pacman -Syu' or install rusty-updater",
                self.updates_status.total_count
            )
        } else {
            "rusty-updater binary is not installed; system is current".to_owned()
        };
        let _ = self.push_notification(
            rusty_ipc::Urgency::Normal,
            "Software Update".to_owned(),
            body,
        );
        self.show_osd("Software Update", None, false);
    }

    /// Builds [`crate::shell_render::UpdatesRenderState`] anchored below `TopBar` (`RUSTYOS-076`).
    #[must_use]
    pub fn updates_render_state(
        &self,
        output_width: i32,
    ) -> crate::shell_render::UpdatesRenderState<'_> {
        let tel = self.telemetry.snapshot();
        let clock_text = if self.clock_24h {
            tel.clock_date_24h.as_str()
        } else {
            tel.clock_date_hhmm.as_str()
        };
        let right = crate::shell_render::compute_top_bar_right_layout_with_options(
            output_width,
            clock_text,
            crate::shell_render::TopBarLayoutOptions {
                has_battery: tel.power.has_battery,
                notification_count: self.notification_count,
                show_volume: self.menubar_show_volume,
                show_input_source: self.menubar_show_input_source,
                show_workspaces: self.menubar_show_workspaces,
                show_tray: self.menubar_show_tray,
                show_updates: self.should_show_top_bar_updates(),
            },
        );
        let anchor_right_x = right
            .updates_x
            .map_or(output_width - 14, |ux| ux + right.updates_w);
        crate::shell_render::UpdatesRenderState {
            glass_opacity: self.glass_opacity,
            reduce_transparency: self.reduce_transparency,
            snapshot: &self.updates_status,
            anchor_right_x,
            hovered_hit: self.updates_popover_hover,
            updater_installed: crate::launcher::resolve_program_path("rusty-updater").is_some(),
        }
    }

    /// Toggles a `TopBar` left menu dropdown popover (`RUSTYOS-048`, `RUSTYOS-068`).
    pub fn toggle_top_bar_menu(&mut self, menu: crate::shell_render::TopBarMenuHit) {
        if menu == crate::shell_render::TopBarMenuHit::Go
            && !self.active_top_bar_app_context().1.has_go_menu()
        {
            self.close_top_bar_menu();
            return;
        }
        if self.active_top_bar_menu == Some(menu) {
            self.close_top_bar_menu();
        } else {
            self.close_context_menu();
            self.close_quick_settings();
            self.close_updates_popover();
            self.close_launcher();
            self.active_top_bar_menu = Some(menu);
            self.top_bar_menu_hover_row = None;
        }
    }

    /// Closes any open `TopBar` menu dropdown popover (`RUSTYOS-048`).
    pub fn close_top_bar_menu(&mut self) {
        self.active_top_bar_menu = None;
        self.top_bar_menu_hover_row = None;
    }

    /// Clears transient shell hover and active slider drag states (`RUSTYOS-051`).
    pub fn clear_shell_hover_states(&mut self) {
        self.set_dock_hover_slot(None);
        self.set_dock_auto_hide_hovered(false);
        self.dock_drag = None;
        self.close_dock_stack();
        self.updates_popover_hover = None;
        self.top_bar_menu_hover_row = None;
        self.qs_slider_drag = None;
        self.hovered_notification_idx = None;
        self.hovered_traffic_light_window = None;
    }

    /// Updates hovered Dock slot and triggers a 120 ms lift transition (`RUSTYOS-048`).
    pub fn set_dock_hover_slot(&mut self, next: Option<usize>) {
        if self.dock_hover_slot != next {
            self.dock_hover_slot = next;
            let now = self.elapsed();
            self.dock_hover_anim.retarget(
                if next.is_some() { 1.0 } else { 0.0 },
                now,
                Duration::from_millis(120),
                Easing::EaseOutCubic,
                self.animations.reduced_motion(),
            );
            self.shell_redraw_needed = true;
        }
    }

    /// Updates shell glass opacity clamped to `0.35..=1.0` (`RUSTYOS-048`).
    pub fn set_glass_opacity(&mut self, opacity: f32) {
        self.glass_opacity = crate::config::clamp_glass_opacity(opacity);
        self.sync_and_save_user_config();
    }

    /// Toggles Focus (Do Not Disturb) notification popup suppression (`RUSTYOS-053`).
    pub fn toggle_focus_mode(&mut self) -> bool {
        self.focus_mode = !self.focus_mode;
        if self.focus_mode {
            self.hovered_notification_idx = None;
        }
        self.focus_mode
    }

    /// Toggles opaque high-contrast shell panels (`reduce_transparency`) (`RUSTYOS-048`).
    pub fn toggle_reduce_transparency(&mut self) {
        self.reduce_transparency = !self.reduce_transparency;
        self.sync_and_save_user_config();
    }

    /// Executes an action chosen from an open `TopBar` menu dropdown (`RUSTYOS-048`, `RUSTYOS-068`).
    pub fn dispatch_top_bar_menu_action(
        &mut self,
        action: crate::shell_render::TopBarMenuAction,
        serial: Serial,
    ) {
        use crate::shell_render::TopBarMenuAction;

        self.close_top_bar_menu();
        match action {
            TopBarMenuAction::AboutRustyOs => {
                let _ = self.push_notification(
                    Urgency::Low,
                    "RustyOS Desktop".to_owned(),
                    "Wayland compositor • Golden Gate UI".to_owned(),
                );
            }
            TopBarMenuAction::AboutActiveApp => self.dispatch_about_active_app(),
            TopBarMenuAction::OpenOverview => self.toggle_overview(),
            TopBarMenuAction::OpenControlCenter => self.toggle_quick_settings(),
            TopBarMenuAction::OpenSettings => self.open_settings(),
            TopBarMenuAction::CheckForUpdates => self.open_updates_popover(true),
            TopBarMenuAction::ReloadConfig => {
                let _ = self.reload_user_config();
            }
            TopBarMenuAction::OpenSpotlight => self.toggle_launcher(),
            TopBarMenuAction::NewTerminal => {
                self.launch_dock_app_new_instance("terminal", true);
            }
            TopBarMenuAction::OpenFiles => {
                self.launch_dock_app_new_instance("files", true);
            }
            TopBarMenuAction::NewDesktopFolder => self.create_desktop_folder_action(),
            TopBarMenuAction::GoRecents
            | TopBarMenuAction::GoDesktop
            | TopBarMenuAction::GoDocuments
            | TopBarMenuAction::GoDownloads
            | TopBarMenuAction::GoHome
            | TopBarMenuAction::GoTrash => self.dispatch_top_bar_go_action(action),
            TopBarMenuAction::EmptyTrash => self.spawn_empty_trash_worker(),
            TopBarMenuAction::ToggleReducedMotion => {
                let next = !self.animations.reduced_motion();
                self.set_reduced_motion_pref(next);
            }
            TopBarMenuAction::ToggleReduceTransparency => self.toggle_reduce_transparency(),
            TopBarMenuAction::RefreshMetrics => self.telemetry.request_refresh(),
            TopBarMenuAction::LockSession => self.set_screen_locked(true, serial),
            TopBarMenuAction::Sleep => {
                self.telemetry
                    .dispatch_session(rusty_services::SessionAction::Suspend);
            }
            TopBarMenuAction::Restart => {
                self.telemetry
                    .dispatch_session(rusty_services::SessionAction::Reboot);
            }
            TopBarMenuAction::ShutDown => {
                self.telemetry
                    .dispatch_session(rusty_services::SessionAction::PowerOff);
            }
            TopBarMenuAction::LogOut => {
                self.loop_signal.stop();
            }
            TopBarMenuAction::ShowShortcuts => {
                let _ = self.push_notification(
                    Urgency::Low,
                    "RustyOS Shortcuts".to_owned(),
                    "Super+Space: Search • Super+Tab: Switcher • Super+1..9: Workspaces".to_owned(),
                );
            }
            other => self.dispatch_top_bar_window_action(other, serial),
        }
    }

    fn dispatch_top_bar_window_action(
        &mut self,
        action: crate::shell_render::TopBarMenuAction,
        serial: Serial,
    ) {
        use crate::shell_render::TopBarMenuAction;

        match action {
            TopBarMenuAction::ToggleLayout => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            TopBarMenuAction::FocusNextWindow => {
                self.apply_wm_action(WmAction::FocusNext, serial);
            }
            TopBarMenuAction::NextWorkspace => self.switch_workspace_relative(1, serial),
            TopBarMenuAction::PrevWorkspace => self.switch_workspace_relative(-1, serial),
            TopBarMenuAction::ToggleMaximizeFocused => {
                self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
            }
            TopBarMenuAction::ToggleFullscreenFocused => {
                self.apply_wm_action(WmAction::ToggleFullscreen, serial);
            }
            TopBarMenuAction::MinimizeFocused => {
                if let Some(focused) = self.wm.focused_window() {
                    self.minimize_window(focused, serial);
                }
            }
            TopBarMenuAction::CloseFocused => {
                self.apply_wm_action(WmAction::CloseFocused, serial);
            }
            TopBarMenuAction::QuitActiveApp => {
                if let Some(focused) = self.focused_top_bar_window_id() {
                    self.close_window_by_id(focused);
                }
            }
            TopBarMenuAction::ForceQuitActiveApp => {
                if let Some(focused) = self.focused_top_bar_window_id() {
                    self.force_quit_window_by_id(focused);
                }
            }
            TopBarMenuAction::AppShortcut(shortcut) => {
                let _ = self.dispatch_app_shortcut(shortcut);
            }
            _ => {}
        }
    }

    fn focused_top_bar_window_id(&self) -> Option<rusty_wm::WindowId> {
        if self.desktop_focused {
            return None;
        }
        self.wm.focused_window().filter(|id| {
            !self.is_window_minimized(*id) && self.wm.visible_windows().iter().any(|w| w.id == *id)
        })
    }

    fn dispatch_about_active_app(&mut self) {
        let (app_name, profile) = self.active_top_bar_app_context();
        let (title, body) = if let Some(win_id) = self.focused_top_bar_window_id() {
            let app_id = self.window_app_id(win_id);
            let win_title = self.window_title(win_id);
            let detail = match app_id.as_deref() {
                Some(id) if !id.trim().is_empty() && !win_title.trim().is_empty() => {
                    format!("{id} • {win_title}")
                }
                Some(id) if !id.trim().is_empty() => id.to_owned(),
                _ if !win_title.trim().is_empty() => win_title,
                _ => "Running in RustyOS Wayland".to_owned(),
            };
            (format!("About {app_name}"), detail)
        } else if profile == crate::shell_render::TopBarAppProfile::Files {
            (
                "About Files".to_owned(),
                "RustyOS Desktop & File Manager".to_owned(),
            )
        } else {
            (
                format!("About {app_name}"),
                "Running in RustyOS Wayland".to_owned(),
            )
        };
        let _ = self.push_notification(Urgency::Low, title, body);
    }

    fn dispatch_top_bar_go_action(&mut self, action: crate::shell_render::TopBarMenuAction) {
        let launched = match action {
            crate::shell_render::TopBarMenuAction::GoRecents => {
                self.launch_rusty_files(&["--recents"])
            }
            crate::shell_render::TopBarMenuAction::GoDesktop => {
                let desktop = super::context_menu_actions::resolve_desktop_dir();
                self.launch_rusty_files_dir(desktop.as_deref())
            }
            crate::shell_render::TopBarMenuAction::GoDocuments => {
                let locs = rusty_services::file_manager::UserFileLocations::detect();
                self.launch_rusty_files_dir(Some(&locs.documents))
            }
            crate::shell_render::TopBarMenuAction::GoDownloads => {
                let downloads = super::context_menu_actions::resolve_downloads_dir();
                self.launch_rusty_files_dir(downloads.as_deref())
            }
            crate::shell_render::TopBarMenuAction::GoHome => {
                let locs = rusty_services::file_manager::UserFileLocations::detect();
                self.launch_rusty_files_dir(Some(&locs.home))
            }
            crate::shell_render::TopBarMenuAction::GoTrash => self.launch_rusty_files(&["--trash"]),
            _ => false,
        };
        if !launched {
            self.toggle_launcher();
        }
    }

    /// Engages or disengages the interactive session Lock Screen with smooth transition (`RUSTYOS-025`).
    pub fn set_screen_locked(&mut self, locked: bool, serial: Serial) {
        self.screen_locked = locked;
        self.lock_power_menu_open = false;
        self.lock_selected_user = 0;
        self.lock_password.clear();
        self.lock_auth_checking = false;
        self.lock_pending_request_id = None;
        self.lock_auth_error = None;
        if locked {
            self.polkit_password.clear();
            self.abort_portal_activity_for_lock();
            self.close_settings();
            self.close_context_menu();
            self.window_switcher = None;
            self.dnd_icon = None;
            self.close_launcher();
            self.close_quick_settings();
            self.close_updates_popover();
            self.close_top_bar_menu();
            self.close_overview();
            self.active_window_drag = None;
            self.active_window_resize = None;
            self.clear_shell_hover_states();
            self.touchpad_swipe = TouchpadSwipeState::default();
            self.deactivate_focused_shortcut_inhibitor();
            self.telemetry.request_refresh();
            let keyboard = self.keyboard.clone();
            keyboard.set_focus(self, None, serial);
        } else {
            self.suppressed_inhibitor_surface = None;
            self.sync_portal_snapshot();
            self.sync_space_with_wm(serial);
            if self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
                let keyboard = self.keyboard.clone();
                keyboard.set_focus(self, None, serial);
            }
        }
        self.logind.set_locked_hint(locked);
        self.shell_redraw_needed = true;
        let now = self.elapsed();
        let tokens = MotionTokens::default();
        self.lock_screen_anim.retarget(
            if locked { 1.0 } else { 0.0 },
            now,
            tokens.standard_duration,
            Easing::EaseOutQuart,
            self.animations.reduced_motion(),
        );
    }

    /// Builds a [`LockScreenRenderState`] borrowing the current telemetry snapshot and password length (`RUSTYOS-056`).
    #[must_use]
    pub fn lock_screen_render_state<'a>(
        &'a self,
        tel: &'a crate::telemetry::SystemTelemetry,
        progress: f64,
    ) -> crate::shell_render::LockScreenRenderState<'a> {
        let lock_clock = if self.clock_24h {
            &tel.lock_time_24h
        } else {
            &tel.lock_time_hhmm
        };
        crate::shell_render::LockScreenRenderState {
            date_label: &tel.lock_date,
            clock_hhmm: lock_clock,
            keyboard_layout: &self.active_kbd_layout,
            has_battery: tel.power.has_battery,
            battery_percent: tel.power.percentage,
            battery_charging: tel.power.ac_online
                || tel.power.state == rusty_services::BatteryState::Charging,
            wifi_connected: tel.network.connected,
            wifi_signal_percent: tel.network.wifi_signal_percent,
            wallpaper_path: self.user_config.wallpaper.as_deref(),
            users: &tel.users,
            selected_user: self.lock_selected_user,
            power_menu_open: self.lock_power_menu_open,
            require_password: self.user_config.require_password_on_lock,
            password_len: self.lock_password.char_len(),
            auth_checking: self.lock_auth_checking,
            auth_error: self.lock_auth_error.as_deref(),
            glass_opacity: self.glass_opacity,
            reduce_transparency: self.reduce_transparency,
            progress,
        }
    }

    /// Toggles the Lock Screen top-right power popover (`RUSTYOS-054`).
    pub fn toggle_lock_power_menu(&mut self) {
        self.lock_power_menu_open = !self.lock_power_menu_open;
    }

    /// Selects an interactive user row index on the Lock Screen (`RUSTYOS-054`).
    pub fn select_lock_user(&mut self, idx: usize) {
        let user_count = self.telemetry.snapshot().users.len().max(1);
        let next = idx.min(user_count - 1);
        if next != self.lock_selected_user {
            self.lock_selected_user = next;
            self.lock_password.clear();
            self.lock_auth_checking = false;
            self.lock_pending_request_id = None;
            self.lock_auth_error = None;
        }
    }

    /// Updates accessibility `reduced_motion` and snaps in-flight shell transitions if enabled (`RUSTYOS-025`).
    pub fn set_reduced_motion_pref(&mut self, enabled: bool) {
        self.animations.set_reduced_motion(enabled);
        self.sync_and_save_user_config();
        if enabled {
            self.overview_anim.snap_to_target();
            self.launcher_anim.snap_to_target();
            self.quick_settings_anim.snap_to_target();
            self.lock_screen_anim.snap_to_target();
            self.dock_hover_anim.snap_to_target();
        }
    }

    pub(in crate::state) fn retarget_launcher_anim(&mut self) {
        let now = self.elapsed();
        let tokens = MotionTokens::default();
        self.launcher_anim.retarget(
            if self.launcher_active { 1.0 } else { 0.0 },
            now,
            tokens.short_duration,
            Easing::EaseOutQuart,
            self.animations.reduced_motion(),
        );
    }

    pub(in crate::state) fn retarget_quick_settings_anim(&mut self) {
        let now = self.elapsed();
        let tokens = MotionTokens::default();
        self.quick_settings_anim.retarget(
            if self.quick_settings_active { 1.0 } else { 0.0 },
            now,
            tokens.short_duration,
            Easing::EaseOutCubic,
            self.animations.reduced_motion(),
        );
    }

    pub(in crate::state) fn retarget_overview_anim(&mut self) {
        let now = self.elapsed();
        let tokens = MotionTokens::default();
        self.overview_anim.retarget(
            if self.overview_active { 1.0 } else { 0.0 },
            now,
            tokens.long_duration,
            Easing::EaseOutCubic,
            self.animations.reduced_motion(),
        );
    }

    /// Pushes a notification into the compositor HUD queue using current monotonic uptime (`RUSTYOS-024`).
    pub fn push_notification(&mut self, urgency: Urgency, title: String, body: String) -> u64 {
        let now = self.start_time.elapsed();
        self.push_notification_at(urgency, title, body, now)
    }

    /// Pushes a notification at explicit monotonic timestamp `now` (`RUSTYOS-024`).
    pub fn push_notification_at(
        &mut self,
        urgency: Urgency,
        title: String,
        body: String,
        now: Duration,
    ) -> u64 {
        const MAX_QUEUE_LEN: usize = 16;
        self.next_notification_id = self.next_notification_id.saturating_add(1);
        let ttl = match urgency {
            Urgency::Low => Duration::from_secs(4),
            Urgency::Normal => Duration::from_secs(6),
            Urgency::Critical => Duration::from_secs(12),
        };
        let id = self.next_notification_id;
        self.notifications.push(CompositorNotification {
            id,
            dbus_id: None,
            urgency,
            title,
            body,
            created_at: now,
            ttl,
        });
        if self.notifications.len() > MAX_QUEUE_LEN {
            let excess = self.notifications.len() - MAX_QUEUE_LEN;
            self.notifications.drain(0..excess);
        }
        self.sync_notification_count();
        id
    }

    /// Updates the active `XWayland` `DISPLAY` number (`:N`) on state and launcher (`RUSTYOS-026`).
    pub fn set_xwayland_display(&mut self, display_number: Option<u32>) {
        self.xwayland_display = display_number;
        self.launcher.x11_display = display_number.map(|num| format!(":{num}"));
        if self.sync_systemd_activation_env && display_number.is_some() {
            crate::drm_backend::sync_drm_activation_environment(&self.socket_name, display_number);
        }
    }

    /// Inserts or replaces a notification received from the D-Bus session server (`RUSTYOS-026`).
    pub fn push_dbus_notification(
        &mut self,
        dbus_id: u32,
        replaces_id: u32,
        urgency: Urgency,
        title: String,
        body: String,
        expire_after: Option<Duration>,
    ) {
        const MAX_QUEUE_LEN: usize = 12;
        let now = self.start_time.elapsed();
        let default_ttl = match urgency {
            Urgency::Low => Duration::from_secs(4),
            Urgency::Normal => Duration::from_secs(6),
            Urgency::Critical => Duration::from_secs(12),
        };
        let ttl = expire_after.unwrap_or(default_ttl);

        if replaces_id > 0
            && let Some(existing) = self
                .notifications
                .iter_mut()
                .find(|n| n.dbus_id == Some(replaces_id))
        {
            existing.dbus_id = Some(dbus_id);
            existing.urgency = urgency;
            existing.title = title;
            existing.body = body;
            existing.created_at = now;
            existing.ttl = ttl;
            self.sync_notification_count();
            return;
        }

        self.next_notification_id = self.next_notification_id.saturating_add(1);
        let id = self.next_notification_id;
        self.notifications.push(CompositorNotification {
            id,
            dbus_id: Some(dbus_id),
            urgency,
            title,
            body,
            created_at: now,
            ttl,
        });
        if self.notifications.len() > MAX_QUEUE_LEN {
            let excess = self.notifications.len() - MAX_QUEUE_LEN;
            self.notifications.drain(0..excess);
        }
        self.sync_notification_count();
    }

    /// Non-blockingly drains queued notifications from the D-Bus background thread (`RUSTYOS-026`).
    pub fn drain_dbus_notifications(&mut self) {
        for cmd in self.dbus_notifications.drain_commands() {
            match cmd {
                crate::dbus_notifications::DbusNotificationCommand::Notify {
                    id,
                    replaces_id,
                    urgency,
                    title,
                    body,
                    expire_after,
                } => {
                    self.push_dbus_notification(
                        id,
                        replaces_id,
                        urgency,
                        title,
                        body,
                        expire_after,
                    );
                }
                crate::dbus_notifications::DbusNotificationCommand::Close { id } => {
                    self.notifications.retain(|n| n.dbus_id != Some(id));
                    self.sync_notification_count();
                }
            }
        }
    }

    /// Non-blockingly drains system tray updates from the background SNI watcher (`RUSTYOS-032`).
    pub fn drain_tray_updates(&mut self) {
        if let Some(items) = self.tray.drain_latest() {
            self.tray_items = items;
        }
    }

    /// Removes notifications whose TTL has elapsed as of current uptime (`RUSTYOS-024`).
    pub fn expire_notifications(&mut self) {
        let now = self.start_time.elapsed();
        self.expire_notifications_at(now);
    }

    /// Removes notifications and transient OSD banners whose TTL has elapsed at `now` (`RUSTYOS-024`/`031`).
    pub fn expire_notifications_at(&mut self, now: Duration) {
        self.notifications
            .retain(|n| now.saturating_sub(n.created_at) < n.ttl);
        self.sync_notification_count();
        if self.osd.as_ref().is_some_and(|osd| now >= osd.expires_at) {
            self.osd = None;
        }
    }

    /// Displays a transient glass OSD indicator pill above the Dock (`RUSTYOS-031`).
    pub fn show_osd(&mut self, label: impl Into<String>, value_percent: Option<u8>, muted: bool) {
        let expires_at = self.start_time.elapsed() + Duration::from_millis(1600);
        self.osd = Some(OsdBanner {
            label: label.into(),
            value_percent,
            muted,
            expires_at,
        });
    }

    /// Polls background screenshot encoding/saving results without blocking (`RUSTYOS-031`).
    pub fn drain_screenshot_results(&mut self) {
        while let Some(res) = self.screenshot_results.try_pop() {
            match res {
                Ok(path) => {
                    let filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("screenshot.png")
                        .to_owned();
                    let _ = self.push_notification(
                        Urgency::Low,
                        "Screenshot Saved".to_owned(),
                        format!("{filename} (copied to clipboard)"),
                    );
                    self.show_osd("Screenshot Saved", None, false);
                }
                Err(err) => {
                    warn!(error = %err, "Failed to save screenshot");
                    let _ = self.push_notification(
                        Urgency::Critical,
                        "Screenshot Failed".to_owned(),
                        err,
                    );
                    self.show_osd("Screenshot Failed", None, false);
                }
            }
        }
    }

    /// Dismisses a single active notification by ID (`RUSTYOS-024`).
    pub fn dismiss_notification(&mut self, id: u64) -> bool {
        let prev_len = self.notifications.len();
        self.notifications.retain(|n| n.id != id);
        self.sync_notification_count();
        self.notifications.len() < prev_len
    }

    /// Clears all active notifications from the HUD queue (`RUSTYOS-024`).
    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
        self.sync_notification_count();
    }

    /// Pushes a live workspace/telemetry status notification (`Alt+N`, `RUSTYOS-024`).
    pub fn trigger_demo_notification(&mut self) -> u64 {
        let next_idx = self.next_notification_id % 3;
        let urgency = match next_idx {
            0 => Urgency::Normal,
            1 => Urgency::Low,
            _ => Urgency::Critical,
        };
        let ws = self.wm.active_workspace_id().0;
        let win_count = self.wm.visible_windows().len();
        let tel = self.telemetry.snapshot();
        let (title, body) = match urgency {
            Urgency::Normal => (
                format!("Workspace {ws} Status"),
                format!("{win_count} windows • {}", tel.network.status_label),
            ),
            Urgency::Low => (
                "System Telemetry".to_owned(),
                format!(
                    "Vol {}%{} • {}",
                    tel.audio.volume_percent,
                    if tel.audio.muted { " (Muted)" } else { "" },
                    tel.clock_hhmm
                ),
            ),
            Urgency::Critical => (
                "Attention Required".to_owned(),
                "Click toast or Alt+Shift+N to dismiss".to_owned(),
            ),
        };
        self.push_notification(urgency, title, body)
    }

    pub(in crate::state) fn sync_notification_count(&mut self) {
        self.notification_count = u32::try_from(self.notifications.len()).unwrap_or(u32::MAX);
    }

    /// Toggles Workspace Overview / Expose mode (`RUSTYOS-023`).
    pub fn toggle_overview(&mut self) {
        self.overview_active = !self.overview_active;
        if self.overview_active {
            self.close_dock_stack();
            self.close_context_menu();
            self.close_launcher();
            self.close_quick_settings();
            self.close_updates_popover();
            self.close_settings();
            self.active_window_drag = None;
            self.active_window_resize = None;
        } else {
            self.overview_filter_app = None;
        }
        self.retarget_overview_anim();
    }

    /// Exits Workspace Overview mode (`RUSTYOS-023`).
    pub fn close_overview(&mut self) {
        self.overview_active = false;
        self.overview_filter_app = None;
        self.retarget_overview_anim();
    }

    /// Toggles Workspace Overview filtered to windows of a single Dock app (App Exposé, `RUSTYOS-073`).
    pub fn toggle_overview_for_app(&mut self, app_key: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        if self.overview_active && self.overview_filter_app.as_deref() == Some(key.as_str()) {
            self.close_overview();
            return;
        }
        self.overview_filter_app = Some(key);
        if self.overview_active {
            self.shell_redraw_needed = true;
        } else {
            self.toggle_overview();
        }
    }

    /// Ordered window IDs shown in Workspace Overview (filtered when App Exposé is active).
    #[must_use]
    pub fn overview_window_ids(&self) -> Vec<WindowId> {
        let ids = self.ordered_visible_window_ids();
        let Some(ref filter) = self.overview_filter_app else {
            return ids;
        };
        ids.into_iter()
            .filter(|&id| {
                let title = self.window_title(id);
                let app_id = self.window_app_id(id);
                crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, id)
                    == *filter
            })
            .collect()
    }

    #[must_use]
    pub fn dock_stack_render_state(&self) -> Option<crate::shell_render::DockStackRenderState<'_>> {
        let stack = self.dock_stack.as_ref()?;
        Some(crate::shell_render::DockStackRenderState {
            entries: &stack.entries,
            view_mode: self.user_config.dock_downloads_view,
            dock_position: self.user_config.dock_position,
            anchor_rect: stack.anchor_rect,
            hovered_item: stack.hovered_item,
            hovered_open_in_files: stack.hovered_open_in_files,
            title: &stack.title,
        })
    }

    #[must_use]
    pub fn open_dock_stack_layout(
        &self,
        primary_size: smithay::utils::Size<i32, smithay::utils::Physical>,
    ) -> Option<crate::shell_render::DockStackLayout> {
        let state = self.dock_stack_render_state()?;
        Some(crate::shell_render::compute_dock_stack_layout(
            primary_size,
            &state,
        ))
    }

    /// Returns visible window IDs on the active workspace in stable ascending `WindowId` order (`RUSTYOS-023`).
    #[must_use]
    pub fn ordered_visible_window_ids(&self) -> Vec<WindowId> {
        let mut ids: Vec<WindowId> = self
            .wm
            .visible_windows()
            .into_iter()
            .map(|w| w.id)
            .collect();
        ids.sort_by_key(|id| id.0);
        ids
    }

    /// Returns the canonical active application name and profile for `TopBar` (`RUSTYOS-068`).
    #[must_use]
    pub fn active_top_bar_app_context(&self) -> (String, crate::shell_render::TopBarAppProfile) {
        let Some(win_id) = self.focused_top_bar_window_id() else {
            return (
                "Files".to_owned(),
                crate::shell_render::TopBarAppProfile::Files,
            );
        };
        let app_id = self.window_app_id(win_id);
        let title = self.window_title(win_id);
        crate::shell_render::classify_top_bar_app(app_id.as_deref(), &title)
    }

    /// Returns the active frontmost application label shown beside the `RustyOS` emblem (`RUSTYOS-049`/`051`/`067`/`068`).
    #[must_use]
    pub fn active_top_bar_app_name(&self) -> String {
        self.active_top_bar_app_context().0
    }

    /// Builds miniature Space entries (`Desktop 1..=9`) for Workspace Overview (`RUSTYOS-051`).
    #[must_use]
    pub fn overview_workspace_spaces(&self) -> Vec<crate::shell_render::OverviewWorkspaceSpace> {
        let primary_geo = self.output_map.primary().logical_geometry();
        let active_ws = self.wm.active_workspace_id().0;
        let top_bar = f32::from(i16::try_from(crate::shell_render::TOP_BAR_HEIGHT).unwrap_or(28));
        #[allow(clippy::cast_precision_loss)]
        let avail_w = primary_geo.size.w.max(320) as f32;
        #[allow(clippy::cast_precision_loss)]
        let avail_h = (primary_geo.size.h.max(240) as f32 - top_bar).max(120.0);

        (rusty_wm::WorkspaceId::MIN..=rusty_wm::WorkspaceId::MAX)
            .map(|ws_id| {
                let mut preview_rects = [(0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32); 4];
                let mut count = 0_usize;
                if let Some(ws) = self.wm.workspace(rusty_wm::WorkspaceId(ws_id)) {
                    for &win_id in &ws.stacking {
                        if self.is_window_minimized(win_id) {
                            continue;
                        }
                        if let Some(win) = self.wm.window(win_id) {
                            if count < preview_rects.len() {
                                #[allow(clippy::cast_precision_loss)]
                                let (rx, ry, rw, rh) = (
                                    (win.geometry.x - primary_geo.loc.x) as f32 / avail_w,
                                    ((win.geometry.y - primary_geo.loc.y) as f32 - top_bar)
                                        / avail_h,
                                    win.geometry.width as f32 / avail_w,
                                    win.geometry.height as f32 / avail_h,
                                );
                                let nx = rx.clamp(0.02, 0.85);
                                let ny = ry.clamp(0.02, 0.85);
                                let nw = rw.clamp(0.10, 0.96 - nx);
                                let nh = rh.clamp(0.10, 0.96 - ny);
                                preview_rects[count] = (nx, ny, nw, nh);
                            }
                            count += 1;
                        }
                    }
                }
                crate::shell_render::OverviewWorkspaceSpace {
                    id: ws_id,
                    window_count: count,
                    active: ws_id == active_ws,
                    preview_rects,
                }
            })
            .collect()
    }

    /// Focuses and raises the `idx`-th window in [`Self::ordered_visible_window_ids`],
    /// restoring it if minimized (`RUSTYOS-023` / `RUSTYOS-032`).
    pub fn focus_ordered_window(&mut self, idx: usize, serial: Serial) {
        let ids = self.ordered_visible_window_ids();
        if let Some(&win_id) = ids.get(idx) {
            self.restore_window(win_id, serial);
            self.close_overview();
        }
    }

    #[cfg(test)]
    pub(in crate::state) fn dock_preferred_candidates(
        hit: crate::shell_render::DockHit,
    ) -> Option<&'static [&'static str]> {
        match hit {
            crate::shell_render::DockHit::PinnedFiles => {
                crate::launcher::LauncherState::canonical_preferred_bins("files")
            }
            crate::shell_render::DockHit::PinnedTerminal => {
                crate::launcher::LauncherState::canonical_preferred_bins("terminal")
            }
            crate::shell_render::DockHit::PinnedBrowser => {
                crate::launcher::LauncherState::canonical_preferred_bins("browser")
            }
            crate::shell_render::DockHit::PinnedNotes => {
                crate::launcher::LauncherState::canonical_preferred_bins("notes")
            }
            _ => None,
        }
    }

    pub(in crate::state) fn dock_key_for_hit(
        &self,
        hit: crate::shell_render::DockHit,
    ) -> Option<String> {
        match hit {
            crate::shell_render::DockHit::PinnedFiles => Some("files".to_owned()),
            crate::shell_render::DockHit::ToggleOverview => Some("overview".to_owned()),
            crate::shell_render::DockHit::PinnedTerminal => Some("terminal".to_owned()),
            crate::shell_render::DockHit::PinnedBrowser => Some("browser".to_owned()),
            crate::shell_render::DockHit::PinnedNotes => Some("notes".to_owned()),
            crate::shell_render::DockHit::PinnedSettings => Some("settings".to_owned()),
            crate::shell_render::DockHit::PinnedApp(idx) => {
                self.user_config.dock_pinned_apps.get(idx).cloned()
            }
            crate::shell_render::DockHit::PinnedDownloads => Some("downloads".to_owned()),
            crate::shell_render::DockHit::PinnedTrash => Some("trash".to_owned()),
            _ => self
                .build_dock_tiles()
                .into_iter()
                .find(|tile| tile.hit == hit)
                .map(|tile| tile.app_key),
        }
    }

    /// Triggers a single or continuous launch bounce animation on `app_key` (`RUSTYOS-073`).
    pub fn trigger_app_bounce(&mut self, app_key: &str, continuous: bool) {
        if !self.user_config.dock_animate_opening || self.animations.reduced_motion() {
            return;
        }
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let now = self.elapsed();
        if let Some(existing) = self.app_bounces.iter_mut().find(|b| b.app_key == key) {
            existing.started_at = now;
            existing.elapsed_ms = 0.0;
            existing.continuous = existing.continuous || continuous;
        } else {
            self.app_bounces.push(super::DockBounceState {
                app_key: key,
                started_at: now,
                elapsed_ms: 0.0,
                continuous,
            });
        }
        self.shell_redraw_needed = true;
    }

    /// Allows a continuous launch bounce to finish cleanly once the app maps its window (`RUSTYOS-073`).
    pub fn settle_app_bounce(&mut self, app_key: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        for b in &mut self.app_bounces {
            if b.app_key == key {
                b.continuous = false;
            }
        }
    }

    /// Computes vertical/inward bounce displacements in logical pixels (`0.0..=14.0`).
    #[must_use]
    pub fn dock_bounce_offsets(&self) -> Vec<(&str, f64)> {
        const PERIOD_MS: f64 = 520.0;
        const AMP_PX: f64 = 14.0;
        self.app_bounces
            .iter()
            .map(|b| {
                let phase = (b.elapsed_ms % PERIOD_MS) / PERIOD_MS;
                let decay = if b.continuous {
                    1.0
                } else {
                    (1.0 - b.elapsed_ms / (PERIOD_MS * 2.0)).clamp(0.25, 1.0)
                };
                let offset = (phase * std::f64::consts::PI).sin().abs() * AMP_PX * decay;
                (b.app_key.as_str(), offset)
            })
            .collect()
    }

    /// Updates pointer edge/hover state for Dock Auto-hide reveal animations (`RUSTYOS-073`).
    pub fn set_dock_auto_hide_hovered(&mut self, hovered: bool) {
        if self.dock_auto_hide_hovered != hovered {
            self.dock_auto_hide_hovered = hovered;
            self.sync_dock_auto_hide_target();
        }
    }

    pub(crate) fn sync_dock_auto_hide_target(&mut self) {
        let keep_open = !self.user_config.dock_auto_hide
            || self.dock_auto_hide_hovered
            || self.dock_stack.is_some()
            || self.dock_drag.is_some()
            || matches!(
                self.context_menu.as_ref().map(|c| c.target),
                Some(crate::shell_render::ContextMenuTarget::DockSlot(_))
            );
        let target = if keep_open { 1.0 } else { 0.0 };
        if (self.dock_auto_hide_anim.target() - target).abs() > 1e-4 {
            let now = self.elapsed();
            let easing = if keep_open {
                Easing::EaseOutCubic
            } else {
                Easing::EaseInOutCubic
            };
            self.dock_auto_hide_anim.retarget(
                target,
                now,
                Duration::from_millis(180),
                easing,
                self.animations.reduced_motion(),
            );
            self.shell_redraw_needed = true;
        }
    }

    /// Closes an open Dock `Downloads` Stack popover if present (`RUSTYOS-073`).
    pub fn close_dock_stack(&mut self) {
        if self.dock_stack.take().is_some() {
            self.sync_dock_auto_hide_target();
            self.shell_redraw_needed = true;
        }
    }

    #[must_use]
    pub fn primary_output_logical_size(
        &self,
    ) -> smithay::utils::Size<i32, smithay::utils::Physical> {
        let lsize = self.output_map.primary().logical_geometry().size;
        smithay::utils::Size::from((lsize.w.max(320), lsize.h.max(240)))
    }

    #[must_use]
    pub fn dock_drag_preview(&self) -> Option<crate::shell_render::DockDragPreview<'_>> {
        let drag = self.dock_drag.as_ref()?;
        if !drag.active_drag {
            return None;
        }
        let source_pinned_idx = drag.source_pinned_idx?;
        Some(crate::shell_render::DockDragPreview {
            app_key: &drag.app_key,
            source_pinned_idx,
            target_pinned_idx: drag.target_pinned_idx,
            pointer_xy: (drag.current_pointer.x, drag.current_pointer.y),
            pulled_outside: drag.pulled_outside,
        })
    }

    #[must_use]
    pub fn dock_view_state<'a>(
        &'a self,
        tiles: &'a [crate::shell_render::DockAppTile],
        windows: &'a [crate::shell_render::DockWindowEntry],
        bounces: &'a [(&'a str, f64)],
        hover_slot: Option<usize>,
    ) -> crate::shell_render::DockViewState<'a> {
        crate::shell_render::DockViewState {
            pinned_icons: [None, None],
            windows,
            tiles,
            hover_slot,
            overview_active: self.overview_active,
            hover_progress: self.dock_hover_anim.current(),
            reduced_motion: self.animations.reduced_motion(),
            icon_size: self.user_config.dock_icon_size,
            position: self.user_config.dock_position,
            auto_hide_reveal: if self.user_config.dock_auto_hide {
                self.dock_auto_hide_anim.current()
            } else {
                1.0
            },
            show_indicators: self.user_config.dock_show_indicators,
            bounces,
            drag_preview: self.dock_drag_preview(),
        }
    }

    #[must_use]
    pub fn active_dock_layout(
        &self,
        tiles: &[crate::shell_render::DockAppTile],
    ) -> crate::shell_render::ComputedDockLayout {
        let screen_size = self.primary_output_logical_size();
        let bounces = self.dock_bounce_offsets();
        let view = self.dock_view_state(tiles, &[], &bounces, self.dock_hover_slot);
        crate::shell_render::compute_dock_layout(screen_size, &view)
    }

    pub(in crate::state) fn collect_dock_window_records(&self) -> Vec<DockWindowRecord> {
        let focused = self.wm.focused_window();
        self.ordered_visible_window_ids()
            .into_iter()
            .map(|id| {
                let title = self.window_title(id);
                let app_id = self.window_app_id(id);
                let minimized = self.is_window_minimized(id);
                let app_key =
                    crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, id);
                let icon_name = self.launcher.icon_for_app_hints(app_id.as_deref(), &title);
                let toplevel_icon = self.window_toplevel_icons.get(&id).cloned();
                DockWindowRecord {
                    id,
                    title,
                    app_key,
                    minimized,
                    focused: !minimized && focused == Some(id),
                    icon_name,
                    toplevel_icon,
                }
            })
            .collect()
    }

    fn label_for_dock_record(key: &str, fallback_title: &str) -> String {
        if key.starts_with("win-") {
            return crate::shell_render::extract_app_name(fallback_title);
        }
        crate::shell_render::dock_label_for_key(key)
    }

    /// Builds ordered macOS 27 Golden Gate Dock tiles with app grouping (`RUSTYOS-073`).
    #[must_use]
    pub fn build_dock_tiles(&self) -> Vec<crate::shell_render::DockAppTile> {
        let records = self.collect_dock_window_records();
        let mut tiles =
            Vec::with_capacity(self.user_config.dock_pinned_apps.len() + records.len() + 4);
        let mut pinned_keys = std::collections::HashSet::new();

        for (pin_idx, raw_key) in self.user_config.dock_pinned_apps.iter().enumerate() {
            let key = raw_key.trim().to_ascii_lowercase();
            if key.is_empty() || !pinned_keys.insert(key.clone()) {
                continue;
            }
            let matching: Vec<&DockWindowRecord> =
                records.iter().filter(|rec| rec.app_key == key).collect();
            let representative_win = matching
                .iter()
                .find(|r| r.focused)
                .or_else(|| matching.iter().find(|r| !r.minimized))
                .or_else(|| matching.first())
                .map(|r| r.id);
            let is_active = if key == "overview" {
                self.overview_active
            } else {
                (key == "files" && self.desktop_focused) || matching.iter().any(|r| r.focused)
            };
            let has_indicator = if key == "overview" {
                self.overview_active
            } else {
                !matching.is_empty() || (key == "files" && self.desktop_focused)
            };
            let icon_name = matching
                .iter()
                .find_map(|r| r.icon_name.clone())
                .or_else(|| self.launcher.icon_for_dock_key(&key));
            let toplevel_icon = matching.iter().find_map(|r| r.toplevel_icon.clone());
            let label = crate::shell_render::dock_label_for_key(&key);
            tiles.push(crate::shell_render::DockAppTile {
                section: crate::shell_render::DockSection::Pinned,
                hit: crate::shell_render::dock_hit_for_pinned_key(&key, pin_idx),
                builtin_icon: crate::shell_render::dock_builtin_for_key(&key),
                app_key: key,
                label: label.clone(),
                representative_win,
                window_count: matching.len(),
                has_minimized: matching.iter().any(|r| r.minimized),
                is_active,
                has_indicator,
                icon_name,
                toplevel_icon,
                fallback_title: label,
            });
        }

        self.push_unpinned_and_minimized_tiles(&records, &pinned_keys, &mut tiles);
        crate::shell_render::append_utility_dock_tiles(&mut tiles);
        tiles
    }

    fn push_unpinned_and_minimized_tiles(
        &self,
        records: &[DockWindowRecord],
        pinned_keys: &std::collections::HashSet<String>,
        tiles: &mut Vec<crate::shell_render::DockAppTile>,
    ) {
        let mut seen_unpinned = std::collections::HashSet::new();
        let mut unpinned_ordinal = 0_usize;
        for rec in records {
            if pinned_keys.contains(&rec.app_key) || !seen_unpinned.insert(rec.app_key.clone()) {
                continue;
            }
            let group: Vec<&DockWindowRecord> = records
                .iter()
                .filter(|other| other.app_key == rec.app_key)
                .collect();
            let rep = group
                .iter()
                .find(|r| r.focused)
                .or_else(|| group.iter().find(|r| !r.minimized))
                .or_else(|| group.first())
                .map(|r| r.id);
            let label = Self::label_for_dock_record(&rec.app_key, &rec.title);
            tiles.push(crate::shell_render::DockAppTile {
                section: crate::shell_render::DockSection::UnpinnedOrRecent,
                app_key: rec.app_key.clone(),
                label: label.clone(),
                hit: crate::shell_render::DockHit::WindowSlot(unpinned_ordinal),
                builtin_icon: crate::shell_render::dock_builtin_for_key(&rec.app_key),
                representative_win: rep,
                window_count: group.len(),
                has_minimized: group.iter().any(|r| r.minimized),
                is_active: group.iter().any(|r| r.focused),
                has_indicator: true,
                icon_name: group.iter().find_map(|r| r.icon_name.clone()),
                toplevel_icon: group.iter().find_map(|r| r.toplevel_icon.clone()),
                fallback_title: label,
            });
            unpinned_ordinal += 1;
        }

        if self.user_config.dock_show_recent {
            for (idx, recent) in self.recent_unpinned_apps.iter().enumerate() {
                if pinned_keys.contains(&recent.app_key) || seen_unpinned.contains(&recent.app_key)
                {
                    continue;
                }
                tiles.push(crate::shell_render::DockAppTile {
                    section: crate::shell_render::DockSection::UnpinnedOrRecent,
                    app_key: recent.app_key.clone(),
                    label: recent.label.clone(),
                    hit: crate::shell_render::DockHit::RecentApp(idx),
                    builtin_icon: crate::shell_render::dock_builtin_for_key(&recent.app_key),
                    representative_win: None,
                    window_count: 0,
                    has_minimized: false,
                    is_active: false,
                    has_indicator: false,
                    icon_name: recent.icon_name.clone(),
                    toplevel_icon: None,
                    fallback_title: recent.label.clone(),
                });
            }
        }

        if !self.user_config.dock_minimize_into_app {
            for rec in records.iter().filter(|r| r.minimized) {
                let short = if rec.title.len() > 24 {
                    crate::shell_render::extract_app_name(&rec.title)
                } else {
                    rec.title.clone()
                };
                tiles.push(crate::shell_render::DockAppTile {
                    section: crate::shell_render::DockSection::MinimizedWindow,
                    app_key: rec.app_key.clone(),
                    label: short,
                    hit: crate::shell_render::DockHit::MinimizedWindow(rec.id),
                    builtin_icon: crate::shell_render::dock_builtin_for_key(&rec.app_key),
                    representative_win: Some(rec.id),
                    window_count: 1,
                    has_minimized: true,
                    is_active: false,
                    has_indicator: false,
                    icon_name: rec.icon_name.clone(),
                    toplevel_icon: rec.toplevel_icon.clone(),
                    fallback_title: rec.title.clone(),
                });
            }
        }
    }

    /// Launches a fresh instance of a Dock app key with opening bounce (`RUSTYOS-073`).
    pub fn launch_dock_app_new_instance(&mut self, app_key: &str, continuous_bounce: bool) {
        self.close_dock_stack();
        self.close_overview();
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        if self
            .launcher
            .launch_by_dock_key(app_key, &self.socket_name, &ipc_path)
        {
            self.trigger_app_bounce(app_key, continuous_bounce);
        } else {
            self.toggle_launcher();
        }
    }

    pub(in crate::state) fn launch_rusty_files(&mut self, args: &[&str]) -> bool {
        self.close_dock_stack();
        self.close_overview();
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        let launched = self
            .launcher
            .launch_program_with_args("rusty-files", args, &self.socket_name, &ipc_path)
            .is_ok();
        if launched {
            self.trigger_app_bounce("files", false);
        }
        launched
    }

    pub(crate) fn launch_rusty_files_dir(&mut self, dir: Option<&std::path::Path>) -> bool {
        match dir.and_then(|p| p.to_str()) {
            Some(path_str) => self.launch_rusty_files(&["--path", path_str]),
            None => self.launch_rusty_files(&[]),
        }
    }

    pub(in crate::state) fn open_downloads_stack(&mut self) {
        self.close_context_menu();
        self.close_top_bar_menu();
        self.close_quick_settings();
        self.close_updates_popover();
        self.close_settings();
        self.close_launcher();
        self.close_overview();
        let tiles = self.build_dock_tiles();
        let layout = self.active_dock_layout(&tiles);
        let anchor_rect = layout
            .slots
            .iter()
            .find(|slot| slot.tile.hit == crate::shell_render::DockHit::PinnedDownloads)
            .map_or_else(
                || smithay::utils::Rectangle::new((0, 0).into(), (50, 50).into()),
                |slot| slot.icon_rect,
            );
        let entries = super::context_menu_actions::resolve_downloads_dir().map_or_default(|dir| {
            super::context_menu_actions::read_downloads_stack_entries(
                &dir,
                self.user_config.downloads_sort,
                24,
            )
        });
        self.dock_stack = Some(super::DockStackState {
            title: "Downloads".to_owned(),
            anchor_rect,
            entries,
            hovered_item: None,
            hovered_open_in_files: false,
            selected_idx: None,
        });
        self.sync_dock_auto_hide_target();
        self.shell_redraw_needed = true;
    }

    /// Commits a finished internal Dock drag to reorder or unpin `[dock].pinned_apps` (`RUSTYOS-073`).
    pub fn commit_dock_pin_reorder_or_remove(
        &mut self,
        source_idx: usize,
        target_idx: usize,
        pulled_outside: bool,
    ) {
        let len = self.user_config.dock_pinned_apps.len();
        if source_idx >= len {
            return;
        }
        let key = self.user_config.dock_pinned_apps[source_idx].clone();
        if pulled_outside && crate::shell_render::is_dock_pin_removable(&key) {
            self.user_config.dock_pinned_apps.remove(source_idx);
            self.set_dock_hover_slot(None);
            self.sync_and_save_user_config();
            self.shell_redraw_needed = true;
            return;
        }
        let clamped_target = target_idx.min(len.saturating_sub(1));
        if clamped_target != source_idx {
            let removed = self.user_config.dock_pinned_apps.remove(source_idx);
            self.user_config
                .dock_pinned_apps
                .insert(clamped_target, removed);
            self.sync_and_save_user_config();
            self.shell_redraw_needed = true;
        }
    }

    /// Pins or unpins an application key inside `[dock].pinned_apps` (`RUSTYOS-073`).
    pub fn toggle_app_keep_in_dock(&mut self, app_key: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        if let Some(pos) = self
            .user_config
            .dock_pinned_apps
            .iter()
            .position(|item| item.eq_ignore_ascii_case(&key))
        {
            if crate::shell_render::is_dock_pin_removable(&key) {
                self.user_config.dock_pinned_apps.remove(pos);
                self.sync_and_save_user_config();
                self.shell_redraw_needed = true;
            }
        } else {
            self.user_config.dock_pinned_apps.push(key);
            self.recent_unpinned_apps
                .retain(|rec| !rec.app_key.eq_ignore_ascii_case(app_key));
            self.sync_and_save_user_config();
            self.shell_redraw_needed = true;
        }
    }

    fn focus_or_launch_dock_app(&mut self, app_key: &str, serial: Serial) {
        let records = self.collect_dock_window_records();
        let matching: Vec<&DockWindowRecord> =
            records.iter().filter(|r| r.app_key == app_key).collect();
        if matching.is_empty() {
            self.launch_dock_app_new_instance(app_key, true);
            return;
        }
        let unmin: Vec<WindowId> = matching
            .iter()
            .filter(|r| !r.minimized)
            .map(|r| r.id)
            .collect();
        if unmin.is_empty() {
            let first_min = matching[0].id;
            self.restore_window(first_min, serial);
            self.close_overview();
            return;
        }
        let focused = self.wm.focused_window();
        if let Some(pos) = focused.and_then(|f| unmin.iter().position(|id| *id == f)) {
            if unmin.len() > 1 {
                let next = unmin[(pos + 1) % unmin.len()];
                self.wm.focus_and_raise(next);
                self.sync_space_with_wm(serial);
            }
        } else {
            for &id in &unmin {
                self.wm.focus_and_raise(id);
            }
            self.sync_space_with_wm(serial);
        }
        self.close_overview();
    }

    /// Executes an interactive Dock click action (`RUSTYOS-023` / `RUSTYOS-032` / `RUSTYOS-073`).
    pub fn handle_dock_hit(&mut self, hit: crate::shell_render::DockHit, serial: Serial) {
        if hit == crate::shell_render::DockHit::ToggleOverview {
            self.close_dock_stack();
            self.toggle_overview();
            return;
        }
        if hit == crate::shell_render::DockHit::PinnedDownloads {
            self.close_overview();
            match self.user_config.dock_downloads_display {
                crate::config::DownloadsDisplayMode::Stack => {
                    if self.dock_stack.is_some() {
                        self.close_dock_stack();
                    } else {
                        self.open_downloads_stack();
                    }
                }
                crate::config::DownloadsDisplayMode::Folder => {
                    let downloads = super::context_menu_actions::resolve_downloads_dir();
                    if !self.launch_rusty_files_dir(downloads.as_deref()) {
                        self.toggle_launcher();
                    }
                }
            }
            return;
        }
        self.close_dock_stack();
        match hit {
            crate::shell_render::DockHit::PinnedSettings => {
                self.close_overview();
                let open_settings_win = self
                    .collect_dock_window_records()
                    .into_iter()
                    .find(|r| r.app_key == "settings")
                    .map(|r| r.id);
                if let Some(win_id) = open_settings_win {
                    if self.is_window_minimized(win_id) {
                        self.restore_window(win_id, serial);
                    } else {
                        self.wm.focus_and_raise(win_id);
                        self.sync_space_with_wm(serial);
                    }
                } else {
                    self.toggle_settings();
                }
            }
            crate::shell_render::DockHit::PinnedTrash => {
                if !self.launch_rusty_files(&["--trash"]) {
                    self.toggle_launcher();
                }
            }
            crate::shell_render::DockHit::MinimizedWindow(win_id) => {
                self.restore_window(win_id, serial);
                self.close_overview();
            }
            other => {
                if let Some(app_key) = self.dock_key_for_hit(other) {
                    self.focus_or_launch_dock_app(&app_key, serial);
                }
            }
        }
    }

    /// Launches the currently highlighted candidate from Spotlight and closes the overlay.
    pub fn launch_spotlight_selected(&mut self) {
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        let _ = self.launcher.launch_selected(&self.socket_name, &ipc_path);
        self.close_launcher();
    }

    /// Launches the Spotlight candidate at visible row offset `row_offset` and closes the overlay.
    pub fn launch_spotlight_row(&mut self, row_offset: usize) {
        if let Some(cmd) = self.launcher.exec_at_visible_row(5, row_offset) {
            let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
            let _ = self
                .launcher
                .launch_command(&cmd, &self.socket_name, &ipc_path);
        }
        self.close_launcher();
    }

    /// Opens the built-in System Settings modal overlay (`RUSTYOS-061`).
    pub fn open_settings(&mut self) {
        if self.is_session_locked() {
            return;
        }
        self.settings_active = true;
        self.close_dock_stack();
        self.close_context_menu();
        self.close_launcher();
        self.close_quick_settings();
        self.close_updates_popover();
        self.close_overview();
        self.close_top_bar_menu();
        self.shell_redraw_needed = true;
    }

    /// Closes the built-in System Settings modal overlay (`RUSTYOS-061`).
    pub fn close_settings(&mut self) {
        if self.settings_active {
            self.settings_active = false;
            self.shell_redraw_needed = true;
        }
    }

    /// Toggles the built-in System Settings modal overlay (`Super+,`) (`RUSTYOS-061`).
    pub fn toggle_settings(&mut self) {
        if self.settings_active {
            self.close_settings();
        } else {
            self.open_settings();
        }
    }

    /// Builds a snapshot of live settings & config state for [`crate::shell_render::ShellOverlayView::Settings`].
    #[must_use]
    pub fn settings_render_state(&self) -> crate::shell_render::SettingsRenderState<'_> {
        let entries = self.output_map.entries();
        let target = self.selected_settings_output();
        let target_idx = entries
            .iter()
            .position(|entry| entry.id == target.id)
            .unwrap_or(0);
        crate::shell_render::SettingsRenderState {
            active_tab: self.settings_tab,
            glass_opacity: self.glass_opacity,
            reduce_transparency: self.reduce_transparency,
            reduced_motion: self.animations.reduced_motion(),
            has_custom_wallpaper: self.user_config.wallpaper.is_some(),
            menubar_show_workspaces: self.menubar_show_workspaces,
            menubar_show_tray: self.menubar_show_tray,
            menubar_show_input_source: self.menubar_show_input_source,
            menubar_show_volume: self.menubar_show_volume,
            menubar_show_updates: self.menubar_show_updates,
            clock_24h: self.clock_24h,
            target_connector: &target.name,
            target_output_index: target_idx,
            output_count: entries.len().max(1),
            display_width_px: target.physical_size.w,
            display_height_px: target.physical_size.h,
            display_refresh_hz: target.refresh_hz(),
            display_scale: target.scale,
            display_rotation: target.rotation,
            vrr_mode: self.user_config.vrr_mode,
            prefer_max_refresh: self.user_config.prefer_max_refresh,
            keyboard_layout: &self.user_config.keyboard_layout,
            keyboard_options: &self.user_config.keyboard_options,
            repeat_rate: self.user_config.repeat_rate,
            repeat_delay: self.user_config.repeat_delay,
            active_kbd_layout: &self.active_kbd_layout,
            require_password_on_lock: self.user_config.require_password_on_lock,
            lock_on_sleep: self.user_config.lock_on_sleep,
            enable_polkit_agent: self.user_config.enable_polkit_agent,
            updates_enabled: self.updates_enabled,
            dock_icon_size: self.user_config.dock_icon_size,
            dock_position: self.user_config.dock_position,
            dock_minimize_effect: self.user_config.dock_minimize_effect,
            dock_minimize_into_app: self.user_config.dock_minimize_into_app,
            dock_auto_hide: self.user_config.dock_auto_hide,
            dock_animate_opening: self.user_config.dock_animate_opening,
            dock_show_indicators: self.user_config.dock_show_indicators,
            dock_show_recent: self.user_config.dock_show_recent,
            config_status_label: self.config_status_label,
        }
    }

    /// Synchronizes runtime UI/menu preferences back into [`Self::user_config`] and persists
    /// atomically when [`Self::config_path`] is configured (`RUSTYOS-061`).
    pub fn sync_and_save_user_config(&mut self) {
        self.user_config.reduced_motion = self.animations.reduced_motion();
        self.user_config.glass_opacity = crate::config::clamp_glass_opacity(self.glass_opacity);
        self.user_config.reduce_transparency = self.reduce_transparency;
        self.user_config.menubar_show_workspaces = self.menubar_show_workspaces;
        self.user_config.menubar_show_tray = self.menubar_show_tray;
        self.user_config.menubar_show_input_source = self.menubar_show_input_source;
        self.user_config.menubar_show_volume = self.menubar_show_volume;
        self.user_config.updates_show_in_menu_bar = self.menubar_show_updates;
        self.user_config.updates_enabled = self.updates_enabled;
        self.user_config.clock_24h = self.clock_24h;
        self.updates_checker.reconfigure(
            self.updates_enabled,
            self.user_config.updates_interval_minutes,
        );

        if let Some(path) = self.config_path.as_deref() {
            match self.user_config.save_to_path(path) {
                Ok(()) => {
                    self.config_status_label = "Saved";
                    info!(path = %path.display(), "Saved RustyOS user configuration");
                }
                Err(err) => {
                    self.config_status_label = "Save Err";
                    tracing::warn!(
                        ?err,
                        path = %path.display(),
                        "Failed to persist RustyOS user configuration"
                    );
                }
            }
        } else {
            self.config_status_label = "Saved";
        }
        self.shell_redraw_needed = true;
    }

    /// Applies an updated [`crate::config::UserConfig`] to live compositor subsystems without writing disk (`RUSTYOS-061`/`062`).
    pub fn apply_user_config(&mut self, cfg: crate::config::UserConfig) {
        let repeat_changed = cfg.repeat_delay != self.user_config.repeat_delay
            || cfg.repeat_rate != self.user_config.repeat_rate;
        let xkb_changed = cfg.keyboard_layout != self.user_config.keyboard_layout
            || cfg.keyboard_variant != self.user_config.keyboard_variant
            || cfg.keyboard_options != self.user_config.keyboard_options;
        let wallpaper_changed =
            cfg.wallpaper != self.user_config.wallpaper || cfg.wallpaper.is_some();
        let dock_layout_changed = cfg.dock_position != self.user_config.dock_position
            || cfg.dock_icon_size != self.user_config.dock_icon_size
            || cfg.dock_auto_hide != self.user_config.dock_auto_hide;
        let updates_changed = cfg.updates_enabled != self.updates_enabled
            || cfg.updates_interval_minutes != self.user_config.updates_interval_minutes;

        self.animations.set_reduced_motion(cfg.reduced_motion);
        self.glass_opacity = crate::config::clamp_glass_opacity(cfg.glass_opacity);
        self.reduce_transparency = cfg.reduce_transparency;
        self.menubar_show_workspaces = cfg.menubar_show_workspaces;
        self.menubar_show_tray = cfg.menubar_show_tray;
        self.menubar_show_input_source = cfg.menubar_show_input_source;
        self.menubar_show_volume = cfg.menubar_show_volume;
        self.menubar_show_updates = cfg.updates_show_in_menu_bar;
        self.updates_enabled = cfg.updates_enabled;
        self.clock_24h = cfg.clock_24h;

        let output_snapshots: Vec<_> = self
            .output_map
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.id,
                    entry.name.clone(),
                    entry.physical_size,
                    entry.refresh_milli_hz,
                    entry.scale,
                    entry.rotation,
                )
            })
            .collect();
        for (id, name, phys_size, refresh_milli_hz, cur_scale, cur_rot) in output_snapshots {
            let target_scale = cfg.scale_for_output(&name);
            if (target_scale - cur_scale).abs() > 1e-4 {
                self.update_output_geometry(id, phys_size, target_scale, refresh_milli_hz);
            }
            let target_rot = cfg.rotation_for_output(&name, self.rotation_override);
            if target_rot != cur_rot {
                let use_winit_flip = name.eq_ignore_ascii_case("winit-0");
                self.set_output_rotation(id, target_rot, use_winit_flip);
            }
        }

        let keyboard = self.keyboard.clone();
        if repeat_changed {
            keyboard.change_repeat_info(cfg.repeat_rate, cfg.repeat_delay);
        }
        let mut xkb_applied = false;
        if xkb_changed {
            if let Err(err) = keyboard.set_xkb_config(self, cfg.xkb_config()) {
                tracing::warn!(
                    layout = %cfg.keyboard_layout,
                    ?err,
                    "Failed to apply reloaded XKB config; keeping existing keymap"
                );
            } else {
                xkb_applied = true;
            }
        }

        if wallpaper_changed {
            if let Some(ref wp) = cfg.wallpaper {
                self.last_custom_wallpaper = Some(wp.clone());
                if !wp.exists() {
                    tracing::warn!(
                        path = %wp.display(),
                        "Configured wallpaper file does not exist; falling back to built-in gradient"
                    );
                }
            }
            self.wallpaper_revision = self.wallpaper_revision.wrapping_add(1);
        }

        self.user_config = cfg;
        crate::shell_render::set_active_appearance(self.user_config.appearance);
        if updates_changed {
            self.updates_checker.reconfigure(
                self.updates_enabled,
                self.user_config.updates_interval_minutes,
            );
        }
        if dock_layout_changed {
            self.close_dock_stack();
            self.sync_dock_auto_hide_target();
            self.relayout_wm_for_all_outputs();
        }
        if xkb_applied {
            self.refresh_keyboard_layout_label();
        }
        self.shell_redraw_needed = true;
    }

    /// Reloads [`Self::config_path`] from disk and applies it live (`RUSTYOS-061`).
    pub fn reload_user_config(&mut self) -> bool {
        let Some(path) = self.config_path.clone() else {
            return false;
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                let parsed = crate::config::UserConfig::parse_toml(&contents);
                self.apply_user_config(parsed);
                self.config_status_label = "Reloaded";
                self.show_osd("Config Reloaded", None, false);
                info!(path = %path.display(), "Reloaded RustyOS configuration from disk");
                true
            }
            Err(err) => {
                self.config_status_label = "Reload Err";
                self.shell_redraw_needed = true;
                tracing::warn!(
                    ?err,
                    path = %path.display(),
                    "Failed to reload RustyOS configuration"
                );
                false
            }
        }
    }

    /// Executes an interactive control hit inside the System Settings modal (`RUSTYOS-061`/`062`).
    pub fn apply_settings_hit(&mut self, hit: crate::shell_render::SettingsHit) {
        use crate::shell_render::SettingsHit;

        match hit {
            SettingsHit::SelectTab(tab) => {
                self.settings_tab = tab;
                self.shell_redraw_needed = true;
            }
            SettingsHit::GlassOpacityDown => self.set_glass_opacity(self.glass_opacity - 0.05),
            SettingsHit::GlassOpacityUp => self.set_glass_opacity(self.glass_opacity + 0.05),
            SettingsHit::ToggleReduceTransparency => self.toggle_reduce_transparency(),
            SettingsHit::ToggleReducedMotion => {
                let next = !self.animations.reduced_motion();
                self.set_reduced_motion_pref(next);
            }
            SettingsHit::ToggleWallpaper => self.toggle_wallpaper_setting(),
            SettingsHit::ToggleAppearance => self.toggle_appearance_setting(),
            SettingsHit::ToggleMenuWorkspaces => {
                self.menubar_show_workspaces = !self.menubar_show_workspaces;
                self.sync_and_save_user_config();
            }
            SettingsHit::ToggleMenuTray => {
                self.menubar_show_tray = !self.menubar_show_tray;
                self.sync_and_save_user_config();
            }
            SettingsHit::ToggleMenuInputSource => {
                self.menubar_show_input_source = !self.menubar_show_input_source;
                self.sync_and_save_user_config();
            }
            SettingsHit::ToggleMenuVolume => {
                self.menubar_show_volume = !self.menubar_show_volume;
                self.sync_and_save_user_config();
            }
            SettingsHit::ToggleMenuUpdates => {
                self.menubar_show_updates = !self.menubar_show_updates;
                self.sync_and_save_user_config();
            }
            SettingsHit::ToggleClock24h => {
                self.clock_24h = !self.clock_24h;
                self.sync_and_save_user_config();
            }
            dock_hit @ (SettingsHit::DockSizeDown
            | SettingsHit::DockSizeUp
            | SettingsHit::DockCyclePosition
            | SettingsHit::DockToggleMinimizeEffect
            | SettingsHit::DockToggleMinimizeIntoApp
            | SettingsHit::DockToggleAutoHide
            | SettingsHit::DockToggleAnimateOpening
            | SettingsHit::DockToggleShowIndicators
            | SettingsHit::DockToggleShowRecent) => self.apply_dock_settings_hit(dock_hit),
            SettingsHit::ReloadConfig => {
                let _ = self.reload_user_config();
            }
            SettingsHit::Close | SettingsHit::Outside => self.close_settings(),
            SettingsHit::ModalBody => {}
            other => self.apply_display_keyboard_security_settings_hit(other),
        }
    }

    fn apply_display_keyboard_security_settings_hit(
        &mut self,
        hit: crate::shell_render::SettingsHit,
    ) {
        use crate::config::{VrrMode, next_keyboard_layout_preset, next_keyboard_option_preset};
        use crate::shell_render::SettingsHit;

        match hit {
            SettingsHit::CycleTargetOutput => self.cycle_target_settings_output(),
            SettingsHit::ScaleDown | SettingsHit::ScaleUp => {
                self.step_target_display_scale(matches!(hit, SettingsHit::ScaleUp));
            }
            SettingsHit::CycleRotation => self.cycle_target_display_rotation(),
            SettingsHit::CycleVrrMode => {
                self.user_config.vrr_mode = match self.user_config.vrr_mode {
                    VrrMode::Off => VrrMode::Fullscreen,
                    VrrMode::Fullscreen => VrrMode::Always,
                    VrrMode::Always => VrrMode::Off,
                };
                self.sync_and_save_user_config();
            }
            SettingsHit::TogglePreferMaxRefresh => {
                self.user_config.prefer_max_refresh = !self.user_config.prefer_max_refresh;
                self.sync_and_save_user_config();
            }
            SettingsHit::CycleKeyboardLayout => {
                let next = next_keyboard_layout_preset(&self.user_config.keyboard_layout);
                self.apply_keyboard_settings_and_save(Some(next.to_owned()), None, None, None);
            }
            SettingsHit::CycleKeyboardOptions => {
                let next = next_keyboard_option_preset(&self.user_config.keyboard_options);
                self.apply_keyboard_settings_and_save(None, Some(next.to_owned()), None, None);
            }
            SettingsHit::RepeatRateDown | SettingsHit::RepeatRateUp => {
                let delta = if matches!(hit, SettingsHit::RepeatRateUp) {
                    5
                } else {
                    -5
                };
                let next_rate = (self.user_config.repeat_rate + delta).clamp(5, 120);
                self.apply_keyboard_settings_and_save(None, None, None, Some(next_rate));
            }
            SettingsHit::RepeatDelayDown | SettingsHit::RepeatDelayUp => {
                let delta = if matches!(hit, SettingsHit::RepeatDelayUp) {
                    50
                } else {
                    -50
                };
                let next_delay = (self.user_config.repeat_delay + delta).clamp(100, 1000);
                self.apply_keyboard_settings_and_save(None, None, Some(next_delay), None);
            }
            SettingsHit::CycleActiveInputSource => self.cycle_keyboard_layout(),
            SettingsHit::ToggleRequirePassword => {
                self.user_config.require_password_on_lock =
                    !self.user_config.require_password_on_lock;
                self.sync_and_save_user_config();
            }
            SettingsHit::ToggleLockOnSleep => {
                self.user_config.lock_on_sleep = !self.user_config.lock_on_sleep;
                self.sync_and_save_user_config();
            }
            SettingsHit::TogglePolkitAgent => {
                self.user_config.enable_polkit_agent = !self.user_config.enable_polkit_agent;
                self.sync_and_save_user_config();
            }
            SettingsHit::ToggleUpdatesEnabled => {
                self.updates_enabled = !self.updates_enabled;
                self.sync_and_save_user_config();
            }
            _ => {}
        }
    }

    fn apply_dock_settings_hit(&mut self, hit: crate::shell_render::SettingsHit) {
        use crate::shell_render::SettingsHit;

        let mut layout_changed = false;
        match hit {
            SettingsHit::DockSizeDown => {
                self.user_config.dock_icon_size =
                    crate::config::clamp_dock_icon_size(self.user_config.dock_icon_size - 4);
                layout_changed = true;
            }
            SettingsHit::DockSizeUp => {
                self.user_config.dock_icon_size =
                    crate::config::clamp_dock_icon_size(self.user_config.dock_icon_size + 4);
                layout_changed = true;
            }
            SettingsHit::DockCyclePosition => {
                self.user_config.dock_position = self.user_config.dock_position.next();
                layout_changed = true;
            }
            SettingsHit::DockToggleMinimizeEffect => {
                self.user_config.dock_minimize_effect =
                    self.user_config.dock_minimize_effect.toggle();
            }
            SettingsHit::DockToggleMinimizeIntoApp => {
                self.user_config.dock_minimize_into_app = !self.user_config.dock_minimize_into_app;
            }
            SettingsHit::DockToggleAutoHide => {
                self.user_config.dock_auto_hide = !self.user_config.dock_auto_hide;
                self.sync_dock_auto_hide_target();
                layout_changed = true;
            }
            SettingsHit::DockToggleAnimateOpening => {
                self.user_config.dock_animate_opening = !self.user_config.dock_animate_opening;
            }
            SettingsHit::DockToggleShowIndicators => {
                self.user_config.dock_show_indicators = !self.user_config.dock_show_indicators;
            }
            SettingsHit::DockToggleShowRecent => {
                self.user_config.dock_show_recent = !self.user_config.dock_show_recent;
            }
            _ => return,
        }
        if layout_changed {
            self.close_dock_stack();
            self.relayout_wm_for_all_outputs();
        }
        self.sync_and_save_user_config();
        self.shell_redraw_needed = true;
    }

    fn toggle_appearance_setting(&mut self) {
        self.user_config.appearance = self.user_config.appearance.toggle();
        crate::shell_render::set_active_appearance(self.user_config.appearance);
        let label = self.user_config.appearance.label();
        self.sync_and_save_user_config();
        self.show_osd(format!("Appearance: {label}"), None, false);
    }

    fn cycle_target_settings_output(&mut self) {
        let entries = self.output_map.entries();
        if entries.len() <= 1 {
            return;
        }
        let cur_id = self.selected_settings_output().id;
        let cur_idx = entries
            .iter()
            .position(|entry| entry.id == cur_id)
            .unwrap_or(0);
        let next = &entries[(cur_idx + 1) % entries.len()];
        let next_id = next.id;
        let next_name = next.name.clone();
        self.settings_target_output = Some(next_id);
        self.shell_redraw_needed = true;
        self.show_osd(format!("Display: {next_name}"), None, false);
    }

    fn cycle_target_display_rotation(&mut self) {
        use crate::config::DisplayRotation;

        let target = self.selected_settings_output();
        let id = target.id;
        let name = target.name.clone();
        let next_rot = match target.rotation {
            DisplayRotation::Rotate0 => DisplayRotation::Rotate90,
            DisplayRotation::Rotate90 => DisplayRotation::Rotate180,
            DisplayRotation::Rotate180 => DisplayRotation::Rotate270,
            DisplayRotation::Rotate270 => DisplayRotation::Rotate0,
        };
        self.rotation_override = None;
        let use_winit_flip = name.eq_ignore_ascii_case("winit-0");
        self.user_config.output_rotations.insert(name, next_rot);
        if self.output_map.entries().len() == 1 {
            self.user_config.display_rotation = next_rot;
        }
        self.set_output_rotation(id, next_rot, use_winit_flip);
        self.sync_and_save_user_config();
    }

    fn toggle_wallpaper_setting(&mut self) {
        if self.user_config.wallpaper.is_some() {
            self.last_custom_wallpaper = self.user_config.wallpaper.take();
            self.wallpaper_revision = self.wallpaper_revision.wrapping_add(1);
            self.sync_and_save_user_config();
            return;
        }

        let candidate = self
            .last_custom_wallpaper
            .clone()
            .or_else(|| crate::config::default_user_wallpaper_path().filter(|p| p.is_file()));
        if let Some(path) = candidate {
            self.last_custom_wallpaper = Some(path.clone());
            self.user_config.wallpaper = Some(path);
            self.wallpaper_revision = self.wallpaper_revision.wrapping_add(1);
            self.sync_and_save_user_config();
        } else {
            self.config_status_label = "No PNG";
            self.show_osd("Set wallpaper in config.toml", None, false);
            self.shell_redraw_needed = true;
        }
    }

    fn apply_keyboard_settings_and_save(
        &mut self,
        layout: Option<String>,
        options: Option<String>,
        repeat_delay: Option<i32>,
        repeat_rate: Option<i32>,
    ) {
        let mut next = self.user_config.clone();
        if let Some(l) = layout {
            next.keyboard_layout = l;
        }
        if let Some(o) = options {
            next.keyboard_options = o;
        }
        if let Some(delay) = repeat_delay {
            next.repeat_delay = delay.clamp(50, 2000);
        }
        if let Some(rate) = repeat_rate {
            next.repeat_rate = rate.clamp(1, 120);
        }
        self.apply_user_config(next);
        self.sync_and_save_user_config();
    }

    fn step_target_display_scale(&mut self, increase: bool) {
        const SCALE_PRESETS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
        let target = self.selected_settings_output();
        let id = target.id;
        let name = target.name.clone();
        let phys = target.physical_size;
        let refresh = target.refresh_milli_hz;
        let cur = target.scale;

        let nearest = SCALE_PRESETS
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (*a - cur)
                    .abs()
                    .partial_cmp(&(*b - cur).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or(0, |(idx, _)| idx);
        let next_idx = if increase {
            (nearest + 1).min(SCALE_PRESETS.len() - 1)
        } else {
            nearest.saturating_sub(1)
        };
        let next_scale = SCALE_PRESETS[next_idx];
        self.user_config.output_scales.insert(name, next_scale);
        if self.output_map.entries().len() == 1 {
            self.user_config.display_scale = next_scale;
        }
        self.update_output_geometry(id, phys, next_scale, refresh);
        self.sync_and_save_user_config();
    }
}
