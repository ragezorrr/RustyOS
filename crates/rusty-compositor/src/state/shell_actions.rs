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
            self.close_wifi_popover();
            self.close_notification_center();
            self.close_overview();
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
            self.close_wifi_popover();
            self.close_notification_center();
            self.window_switcher = None;
            self.close_launcher();
            self.close_overview();
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
        self.close_wifi_popover();
        self.close_notification_center();
        self.close_launcher();
        self.close_overview();
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

    /// Launches System Settings on the Software Update page (`RUSTYOS-123`).
    pub fn open_software_updater_action(&mut self) {
        self.close_updates_popover();
        if crate::launcher::resolve_program_path("rusty-settings").is_some() {
            let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
            if self
                .launcher
                .launch_program_with_args(
                    "rusty-settings",
                    &["--page", "updates"],
                    &self.socket_name,
                    &ipc_path,
                )
                .is_ok()
            {
                self.trigger_app_bounce("settings", false);
                return;
            }
        }
        let body = if self.updates_status.total_count > 0 {
            format!(
                "{} updates ready · run 'sudo pacman -Syu' or open System Settings",
                self.updates_status.total_count
            )
        } else {
            "System is up to date".to_owned()
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
            self.top_bar_layout_options_for(&tel),
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
            updater_installed: crate::launcher::resolve_program_path("rusty-settings").is_some(),
        }
    }

    /// Toggles the `TopBar` Wi-Fi & Ethernet popover (`RUSTYOS-108`).
    pub fn toggle_wifi_popover(&mut self) {
        if self.wifi_popover_open {
            self.close_wifi_popover();
        } else {
            self.open_wifi_popover();
        }
    }

    /// Opens the `TopBar` Wi-Fi & Ethernet popover and refreshes network telemetry (`RUSTYOS-108`).
    pub fn open_wifi_popover(&mut self) {
        if self.is_session_locked() {
            return;
        }
        self.wifi_popover_open = true;
        self.wifi_popover_hover = None;
        self.close_dock_stack();
        self.close_context_menu();
        self.close_top_bar_menu();
        self.close_updates_popover();
        self.window_switcher = None;
        self.close_quick_settings();
        self.close_notification_center();
        self.close_launcher();
        self.close_overview();
        self.sync_telemetry_active_ui();
        self.telemetry.request_refresh();
        self.shell_redraw_needed = true;
    }

    /// Closes the `TopBar` Wi-Fi & Ethernet popover (`RUSTYOS-108`).
    pub fn close_wifi_popover(&mut self) {
        if self.wifi_popover_open || self.wifi_popover_hover.is_some() {
            self.wifi_popover_open = false;
            self.wifi_popover_hover = None;
            self.sync_telemetry_active_ui();
            self.shell_redraw_needed = true;
        }
    }

    /// Opens `rusty-settings --page wifi` (`RUSTYOS-108`, `RUSTYOS-128`).
    pub fn open_wifi_settings_action(&mut self) {
        self.close_wifi_popover();
        self.open_settings_page(Some("wifi"));
    }

    /// Opens `rusty-settings --page wifi --wifi-ssid <ssid>` to prompt for a Wi-Fi password (`RUSTYOS-159`).
    pub fn open_wifi_password_settings_action(&mut self, ssid: &str) {
        self.close_wifi_popover();
        let trimmed = ssid.trim();
        if trimmed.is_empty() {
            self.open_settings_page(Some("wifi"));
            return;
        }
        if self.wayland_socket_exists()
            && self.launch_rusty_settings_args(&["--page", "wifi", "--wifi-ssid", trimmed])
        {
            self.shell_redraw_needed = true;
        }
    }

    /// Builds [`crate::shell_render::TopBarLayoutOptions`] reflecting active config and telemetry (`RUSTYOS-141`, `RUSTYOS-142`).
    #[must_use]
    pub fn top_bar_layout_options_for(
        &self,
        tel: &crate::telemetry::SystemTelemetry,
    ) -> crate::shell_render::TopBarLayoutOptions {
        crate::shell_render::TopBarLayoutOptions {
            has_battery: tel.power.has_battery,
            notification_count: self.notification_count,
            show_wifi: self.user_config.menubar_show_wifi,
            show_battery: self.user_config.menubar_show_battery,
            show_battery_percentage: self.user_config.menubar_show_battery_percentage,
            show_bluetooth: self.user_config.menubar_show_bluetooth,
            show_volume: self.menubar_show_volume,
            show_input_source: self.menubar_show_input_source,
            show_workspaces: self.menubar_show_workspaces,
            show_tray: self.menubar_show_tray,
            show_updates: self.should_show_top_bar_updates(),
            mic_active: tel.device_usage.mic_active,
            camera_active: tel.device_usage.camera_active,
            recording_active: !self.active_screencasts.is_empty(),
        }
    }

    /// Toggles a compact `TopBar` popover (`Volume`, `Battery`, `Bluetooth`) (`RUSTYOS-141`).
    pub fn toggle_top_bar_quick_popover(
        &mut self,
        kind: crate::shell_render::TopBarQuickPopoverKind,
    ) {
        if self.top_bar_quick_popover == Some(kind) {
            self.close_top_bar_quick_popover();
            return;
        }
        if self.is_session_locked() {
            return;
        }
        self.close_wifi_popover();
        self.close_updates_popover();
        self.close_notification_center();
        self.close_dock_stack();
        self.close_context_menu();
        self.active_top_bar_menu = None;
        self.top_bar_menu_hover_row = None;
        self.quick_settings_active = false;
        self.top_bar_quick_popover = Some(kind);
        self.top_bar_quick_popover_hover = None;
        self.sync_telemetry_active_ui();
        self.shell_redraw_needed = true;
    }

    /// Closes the active compact `TopBar` quick popover (`RUSTYOS-141`).
    pub fn close_top_bar_quick_popover(&mut self) {
        if self.top_bar_quick_popover.is_some() || self.top_bar_quick_popover_hover.is_some() {
            self.top_bar_quick_popover = None;
            self.top_bar_quick_popover_hover = None;
            self.sync_telemetry_active_ui();
            self.shell_redraw_needed = true;
        }
    }

    /// Toggles visibility of a right-side `TopBar` indicator and persists `[top_bar]` to `config.toml` (`RUSTYOS-141`).
    pub fn toggle_top_bar_control_visibility(&mut self, control: &str) {
        match control {
            "wifi" => self.user_config.menubar_show_wifi = !self.user_config.menubar_show_wifi,
            "battery" => {
                self.user_config.menubar_show_battery = !self.user_config.menubar_show_battery;
            }
            "sound" | "volume" => {
                self.menubar_show_volume = !self.menubar_show_volume;
                self.user_config.menubar_show_volume = self.menubar_show_volume;
            }
            "bluetooth" => {
                self.user_config.menubar_show_bluetooth = !self.user_config.menubar_show_bluetooth;
            }
            "battery_percentage" => {
                self.user_config.menubar_show_battery_percentage =
                    !self.user_config.menubar_show_battery_percentage;
            }
            _ => return,
        }
        if let Some(path) = self.config_path.as_ref() {
            let _ = self.user_config.save_to_path(path);
        }
        self.shell_redraw_needed = true;
    }

    /// Returns the live D-Bus `AppMenuTree` for the currently focused window, if registered (`RUSTYOS-145`).
    #[must_use]
    pub fn active_app_menu_tree(&self) -> Option<crate::app_menu::AppMenuTree> {
        let wid = self.wm.focused_window()?;
        let id = u32::try_from(wid.0).ok()?;
        self.app_menu.menu_for_window(id)
    }

    /// Stops all active portal screencast / screen-recording sessions (`RUSTYOS-142`).
    pub fn stop_active_screen_recording(&mut self) {
        if !self.active_screencasts.is_empty() {
            self.active_screencasts.clear();
            self.shell_redraw_needed = true;
        }
    }

    /// Builds [`crate::shell_render::TopBarQuickPopoverState`] anchored below the clicked `TopBar` icon (`RUSTYOS-141`).
    #[must_use]
    pub fn top_bar_quick_popover_render_state<'a>(
        &self,
        output_width: i32,
        kind: crate::shell_render::TopBarQuickPopoverKind,
        tel: &'a crate::telemetry::SystemTelemetry,
    ) -> crate::shell_render::TopBarQuickPopoverState<'a> {
        let clock_text = if self.clock_24h {
            tel.clock_date_24h.as_str()
        } else {
            tel.clock_date_hhmm.as_str()
        };
        let right = crate::shell_render::compute_top_bar_right_layout_with_options(
            output_width,
            clock_text,
            self.top_bar_layout_options_for(tel),
        );
        let anchor_right_x = match kind {
            crate::shell_render::TopBarQuickPopoverKind::Volume => {
                right.volume_x.map_or(right.wifi_x, |x| x + 18)
            }
            crate::shell_render::TopBarQuickPopoverKind::Battery => {
                right.battery_x.map_or(right.wifi_x, |x| x + 26)
            }
            crate::shell_render::TopBarQuickPopoverKind::Bluetooth => {
                right.bluetooth_x.map_or(right.wifi_x, |x| x + 18)
            }
        };
        crate::shell_render::TopBarQuickPopoverState {
            kind,
            anchor_right_x,
            glass_opacity: self.glass_opacity,
            reduce_transparency: self.reduce_transparency,
            audio: &tel.audio,
            power: &tel.power,
            bluetooth: &tel.bluetooth,
            show_battery_percentage: self.user_config.menubar_show_battery_percentage,
            hovered_hit: self.top_bar_quick_popover_hover,
        }
    }

    /// Builds [`crate::shell_render::WifiPopoverRenderState`] anchored below the `TopBar` network glyph (`RUSTYOS-108`).
    #[must_use]
    pub fn wifi_render_state<'a>(
        &self,
        output_width: i32,
        network: &'a rusty_services::NetworkStatus,
    ) -> crate::shell_render::WifiPopoverRenderState<'a> {
        let tel = self.telemetry.snapshot();
        let clock_text = if self.clock_24h {
            tel.clock_date_24h.as_str()
        } else {
            tel.clock_date_hhmm.as_str()
        };
        let right = crate::shell_render::compute_top_bar_right_layout_with_options(
            output_width,
            clock_text,
            self.top_bar_layout_options_for(&tel),
        );
        crate::shell_render::WifiPopoverRenderState {
            glass_opacity: self.glass_opacity,
            reduce_transparency: self.reduce_transparency,
            network,
            anchor_right_x: right.wifi_x + 20,
            hovered_hit: self.wifi_popover_hover,
        }
    }

    /// Toggles the right-edge `Notification Center` drawer (`RUSTYOS-109`).
    pub fn toggle_notification_center(&mut self) {
        if self.notification_center_open {
            self.close_notification_center();
        } else {
            self.open_notification_center();
        }
    }

    /// Opens `Notification Center` and closes competing shell overlays (`RUSTYOS-109`).
    pub fn open_notification_center(&mut self) {
        if self.is_session_locked() {
            return;
        }
        self.notification_center_open = true;
        self.notification_center_hover = None;
        self.close_dock_stack();
        self.close_context_menu();
        self.close_top_bar_menu();
        self.close_updates_popover();
        self.close_wifi_popover();
        self.window_switcher = None;
        self.close_quick_settings();
        self.close_launcher();
        self.close_overview();
        self.telemetry.request_refresh();
        self.shell_redraw_needed = true;
    }

    /// Closes `Notification Center` (`RUSTYOS-109`).
    pub fn close_notification_center(&mut self) {
        if self.notification_center_open || self.notification_center_hover.is_some() {
            self.notification_center_open = false;
            self.notification_center_hover = None;
            self.shell_redraw_needed = true;
        }
    }

    /// Toggles completion state of task `idx` inside the Reminders widget (`RUSTYOS-109`, `RUSTYOS-156`).
    pub fn toggle_notification_center_task(&mut self, idx: usize) {
        if let Some(done) = self.notification_center_tasks.get_mut(idx) {
            *done = !*done;
            self.user_config.notifications_reminders_done = self.notification_center_tasks;
            if let Some(path) = self.config_path.as_ref() {
                let _ = self.user_config.save_to_path(path);
            }
            self.shell_redraw_needed = true;
        }
    }

    /// Cycles the visible Notification Center widgets (`All` -> `Reminders Only` -> `Weather Only`)
    /// and saves the layout to `config.toml` (`RUSTYOS-156`).
    pub fn cycle_notification_center_widgets(&mut self) {
        let (next_weather, next_reminders) = match (
            self.user_config.notifications_show_weather_widget,
            self.user_config.notifications_show_reminders_widget,
        ) {
            (true, true) => (false, true),
            (false, true) => (true, false),
            _ => (true, true),
        };
        self.user_config.notifications_show_weather_widget = next_weather;
        self.user_config.notifications_show_reminders_widget = next_reminders;
        if let Some(path) = self.config_path.as_ref() {
            let _ = self.user_config.save_to_path(path);
        }
        self.shell_redraw_needed = true;
    }

    /// Opens `rusty-settings --page notifications` from `Edit Widgets` (`RUSTYOS-109`, `RUSTYOS-128`).
    pub fn open_notification_settings_action(&mut self) {
        self.close_notification_center();
        self.open_settings_page(Some("notifications"));
    }

    /// Builds newest-first items backed by [`Self::notification_history`] (`RUSTYOS-109`).
    #[must_use]
    pub fn build_notification_center_items(
        &self,
    ) -> Vec<crate::shell_render::NotificationCenterItemView<'_>> {
        let now = self.start_time.elapsed();
        self.notification_history
            .iter()
            .rev()
            .map(|n| {
                let age_secs = now.saturating_sub(n.created_at).as_secs();
                let age_label = if age_secs < 60 {
                    "now".to_owned()
                } else if age_secs < 3600 {
                    format!("{}m ago", age_secs / 60)
                } else {
                    format!("{}h ago", age_secs / 3600)
                };
                crate::shell_render::NotificationCenterItemView {
                    id: n.id,
                    urgency: n.urgency,
                    title: &n.title,
                    body: &n.body,
                    age_label,
                }
            })
            .collect()
    }

    /// Builds [`crate::shell_render::NotificationCenterRenderState`] for rendering and hit-testing (`RUSTYOS-109`).
    #[must_use]
    pub fn notification_center_render_state<'a>(
        &'a self,
        items: &'a [crate::shell_render::NotificationCenterItemView<'a>],
        hide_previews: bool,
    ) -> crate::shell_render::NotificationCenterRenderState<'a> {
        let tel = self.telemetry.snapshot_arc();
        let system_summary = format!(
            "{}% Vol • {}% Bat",
            tel.audio.volume_percent, tel.power.percentage
        );
        crate::shell_render::NotificationCenterRenderState {
            glass_opacity: self.glass_opacity,
            reduce_transparency: self.reduce_transparency,
            hide_previews,
            notifications: items,
            task_states: self.notification_center_tasks,
            task_labels: crate::shell_render::DEFAULT_NOTIFICATION_TASKS,
            hovered_hit: self.notification_center_hover,
            location_name: "Local Station",
            system_summary,
            show_weather_widget: self.user_config.notifications_show_weather_widget,
            show_reminders_widget: self.user_config.notifications_show_reminders_widget,
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
            self.close_wifi_popover();
            self.close_notification_center();
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
        self.wifi_popover_hover = None;
        self.notification_center_hover = None;
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
            TopBarMenuAction::AboutRustyOs => self.open_settings_page(Some("about")),
            TopBarMenuAction::AboutActiveApp => self.dispatch_about_active_app(),
            TopBarMenuAction::OpenOverview => self.toggle_overview(),
            TopBarMenuAction::OpenControlCenter => self.toggle_quick_settings(),
            TopBarMenuAction::OpenSettings => self.open_settings(),
            TopBarMenuAction::CheckForUpdates => self.open_updates_popover(true),
            TopBarMenuAction::ReloadConfig => {
                let _ = self.reload_user_config();
            }
            TopBarMenuAction::OpenSpotlight => self.toggle_launcher(),
            TopBarMenuAction::OpenScreenshotHud => self.open_screenshot_hud(),
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
            TopBarMenuAction::ShowShortcuts => self.open_settings_page(Some("shortcuts")),
            TopBarMenuAction::DbusMenuItem(item_id) => {
                if let Some(tree) = self.active_app_menu_tree() {
                    self.app_menu
                        .activate_item(tree.service_name, tree.object_path, item_id);
                }
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
            TopBarMenuAction::TileLeftFocused => {
                self.apply_wm_action(
                    WmAction::SnapFocused(rusty_wm::SnapTarget::LeftHalf),
                    serial,
                );
            }
            TopBarMenuAction::TileRightFocused => {
                self.apply_wm_action(
                    WmAction::SnapFocused(rusty_wm::SnapTarget::RightHalf),
                    serial,
                );
            }
            TopBarMenuAction::MoveToNextWorkspace => {
                let cur = self.wm.active_workspace_id().0;
                let next = if cur >= 9 { 1 } else { cur + 1 };
                self.apply_wm_action(
                    WmAction::MoveFocusedToWorkspace(rusty_wm::WorkspaceId::new_clamped(next)),
                    serial,
                );
            }
            TopBarMenuAction::MoveToPrevWorkspace => {
                let cur = self.wm.active_workspace_id().0;
                let prev = if cur <= 1 { 9 } else { cur - 1 };
                self.apply_wm_action(
                    WmAction::MoveFocusedToWorkspace(rusty_wm::WorkspaceId::new_clamped(prev)),
                    serial,
                );
            }
            TopBarMenuAction::MoveToNextDisplay => {
                self.apply_wm_action(WmAction::MoveFocusedToNextOutput, serial);
            }
            TopBarMenuAction::MinimizeFocused | TopBarMenuAction::HideActiveApp => {
                if let Some(focused) = self
                    .focused_top_bar_window_id()
                    .or_else(|| self.wm.focused_window())
                {
                    self.minimize_window(focused, serial);
                }
            }
            TopBarMenuAction::HideOtherApps => {
                let focused = self
                    .focused_top_bar_window_id()
                    .or_else(|| self.wm.focused_window());
                let to_minimize: Vec<_> = self
                    .wm
                    .visible_windows()
                    .into_iter()
                    .filter(|w| Some(w.id) != focused && !self.is_window_minimized(w.id))
                    .map(|w| w.id)
                    .collect();
                for win_id in to_minimize {
                    self.minimize_window(win_id, serial);
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
        } else if profile == crate::shell_render::TopBarAppProfile::Desktop {
            (
                "About Desktop".to_owned(),
                "RustyOS Desktop Workspace".to_owned(),
            )
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
            self.close_context_menu();
            self.window_switcher = None;
            self.dnd_icon = None;
            self.close_launcher();
            self.close_quick_settings();
            self.close_updates_popover();
            self.close_wifi_popover();
            self.close_notification_center();
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

    /// Toggles system appearance (`Light <-> Dark`) immediately, updating wallpapers, open XDG windows,
    /// GTK/Qt theme sync, and `config.toml` (`RUSTYOS-156`).
    pub fn toggle_appearance_mode(&mut self) {
        let mut next = self.user_config.clone();
        next.appearance = match next.appearance {
            crate::config::AppearanceMode::Light => crate::config::AppearanceMode::Dark,
            crate::config::AppearanceMode::Dark => crate::config::AppearanceMode::Light,
        };
        if let Some(path) = self.config_path.as_ref() {
            let _ = next.save_to_path(path);
        }
        self.apply_user_config(next);
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

    pub(in crate::state) fn sync_telemetry_active_ui(&self) {
        let active = self.quick_settings_active || self.wifi_popover_open;
        self.telemetry.set_active_ui(active);
    }

    pub(in crate::state) fn retarget_quick_settings_anim(&mut self) {
        self.sync_telemetry_active_ui();
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
        const MAX_HISTORY_LEN: usize = 32;
        self.next_notification_id = self.next_notification_id.saturating_add(1);
        let ttl = match urgency {
            Urgency::Low => Duration::from_secs(4),
            Urgency::Normal => Duration::from_secs(6),
            Urgency::Critical => Duration::from_secs(12),
        };
        let id = self.next_notification_id;
        let item = CompositorNotification {
            id,
            dbus_id: None,
            urgency,
            title,
            body,
            created_at: now,
            ttl,
        };
        self.notifications.push(item.clone());
        self.notification_history.push(item);
        if self.notifications.len() > MAX_QUEUE_LEN {
            let excess = self.notifications.len() - MAX_QUEUE_LEN;
            self.notifications.drain(0..excess);
        }
        if self.notification_history.len() > MAX_HISTORY_LEN {
            let excess = self.notification_history.len() - MAX_HISTORY_LEN;
            self.notification_history.drain(0..excess);
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
        const MAX_HISTORY_LEN: usize = 32;
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
            existing.title.clone_from(&title);
            existing.body.clone_from(&body);
            existing.created_at = now;
            existing.ttl = ttl;
            if let Some(hist) = self
                .notification_history
                .iter_mut()
                .find(|n| n.dbus_id == Some(replaces_id))
            {
                hist.dbus_id = Some(dbus_id);
                hist.urgency = urgency;
                hist.title = title;
                hist.body = body;
                hist.created_at = now;
                hist.ttl = ttl;
            }
            self.sync_notification_count();
            return;
        }

        self.next_notification_id = self.next_notification_id.saturating_add(1);
        let id = self.next_notification_id;
        let item = CompositorNotification {
            id,
            dbus_id: Some(dbus_id),
            urgency,
            title,
            body,
            created_at: now,
            ttl,
        };
        self.notifications.push(item.clone());
        self.notification_history.push(item);
        if self.notifications.len() > MAX_QUEUE_LEN {
            let excess = self.notifications.len() - MAX_QUEUE_LEN;
            self.notifications.drain(0..excess);
        }
        if self.notification_history.len() > MAX_HISTORY_LEN {
            let excess = self.notification_history.len() - MAX_HISTORY_LEN;
            self.notification_history.drain(0..excess);
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
                    self.shell_redraw_needed = true;
                }
                crate::dbus_notifications::DbusNotificationCommand::Close { id } => {
                    self.notifications.retain(|n| n.dbus_id != Some(id));
                    self.notification_history.retain(|n| n.dbus_id != Some(id));
                    self.sync_notification_count();
                    self.shell_redraw_needed = true;
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

    /// Dismisses a single active notification by ID (`RUSTYOS-024`, `RUSTYOS-109`).
    pub fn dismiss_notification(&mut self, id: u64) -> bool {
        let prev_len = self.notifications.len();
        let prev_hist = self.notification_history.len();
        self.notifications.retain(|n| n.id != id);
        self.notification_history.retain(|n| n.id != id);
        self.sync_notification_count();
        self.notifications.len() < prev_len || self.notification_history.len() < prev_hist
    }

    /// Clears all active notifications from the HUD queue and Notification Center (`RUSTYOS-024`, `RUSTYOS-109`).
    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
        self.notification_history.clear();
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

    /// Opens the interactive `ScreenshotHud` overlay (`RUSTYOS-133`).
    pub fn open_screenshot_hud(&mut self) {
        self.close_overview();
        self.close_dock_stack();
        self.close_context_menu();
        self.close_launcher();
        self.close_quick_settings();
        self.close_updates_popover();
        self.close_wifi_popover();
        self.close_notification_center();
        self.close_top_bar_menu();
        self.screenshot_hud = Some(crate::shell_render::ScreenshotHudState::default());
        self.shell_redraw_needed = true;
    }

    /// Closes the interactive `ScreenshotHud` overlay (`RUSTYOS-133`).
    pub fn close_screenshot_hud(&mut self) {
        if self.screenshot_hud.take().is_some() {
            self.shell_redraw_needed = true;
        }
    }

    /// Executes the active `ScreenshotHud` capture and closes the HUD so it is excluded from the PNG (`RUSTYOS-133`).
    pub fn trigger_screenshot_hud_capture(&mut self) {
        let Some(hud) = self.screenshot_hud.take() else {
            return;
        };
        let req = match hud.mode {
            crate::shell_render::ScreenshotMode::FullScreen => {
                if hud.copy_only {
                    crate::screenshot::ScreenshotRequest::FullScreenCopyOnly
                } else {
                    crate::screenshot::ScreenshotRequest::FullScreen
                }
            }
            crate::shell_render::ScreenshotMode::Window => {
                if let Some((_, rect)) = hud.hovered_window {
                    crate::screenshot::ScreenshotRequest::Region {
                        rect,
                        copy_only: hud.copy_only,
                    }
                } else {
                    crate::screenshot::ScreenshotRequest::FocusedWindow
                }
            }
            crate::shell_render::ScreenshotMode::Region => {
                if let Some(rect) = hud.active_region_rect() {
                    crate::screenshot::ScreenshotRequest::Region {
                        rect,
                        copy_only: hud.copy_only,
                    }
                } else if hud.copy_only {
                    crate::screenshot::ScreenshotRequest::FullScreenCopyOnly
                } else {
                    crate::screenshot::ScreenshotRequest::FullScreen
                }
            }
        };
        self.pending_screenshot = Some(req);
        self.shell_redraw_needed = true;
    }

    /// Updates hover/drag state while `ScreenshotHud` is active (`RUSTYOS-133`).
    #[allow(clippy::cast_possible_truncation)]
    pub fn handle_screenshot_hud_motion(
        &mut self,
        loc: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) {
        let Some(hud) = self.screenshot_hud.as_mut() else {
            return;
        };
        let output_phys = self
            .space
            .output_geometry(&self.output)
            .map_or_else(|| smithay::utils::Size::from((1280, 800)), |geo| geo.size)
            .to_physical(1);
        let px = loc.x.round() as i32;
        let py = loc.y.round() as i32;
        let next_hover = crate::shell_render::hit_test_screenshot_hud(
            output_phys,
            smithay::utils::Point::from((px, py)),
        );
        if hud.hover_hit != next_hover {
            hud.hover_hit = next_hover;
            self.shell_redraw_needed = true;
        }
        if hud.mode == crate::shell_render::ScreenshotMode::Region && hud.drag_start.is_some() {
            hud.drag_current = Some((px, py));
            self.shell_redraw_needed = true;
        } else if hud.mode == crate::shell_render::ScreenshotMode::Window {
            let hovered = self
                .wm
                .visible_windows()
                .into_iter()
                .rev()
                .find(|w| {
                    !self.is_window_minimized(w.id)
                        && px >= w.geometry.x
                        && px < w.geometry.x + w.geometry.width
                        && py >= w.geometry.y
                        && py < w.geometry.y + w.geometry.height
                })
                .map(|w| (w.id, w.geometry));
            if let Some(hud) = self.screenshot_hud.as_mut()
                && hud.hovered_window != hovered
            {
                hud.hovered_window = hovered;
                self.shell_redraw_needed = true;
            }
        }
    }

    /// Handles pointer press/release while `ScreenshotHud` is active (`RUSTYOS-133`).
    #[allow(clippy::cast_possible_truncation)]
    pub fn handle_screenshot_hud_button(&mut self, pressed: bool) {
        let output_phys = self
            .space
            .output_geometry(&self.output)
            .map_or_else(|| smithay::utils::Size::from((1280, 800)), |geo| geo.size)
            .to_physical(1);
        let px = self.pointer_location.x.round() as i32;
        let py = self.pointer_location.y.round() as i32;
        if pressed {
            if let Some(hit) = crate::shell_render::hit_test_screenshot_hud(
                output_phys,
                smithay::utils::Point::from((px, py)),
            ) {
                match hit {
                    crate::shell_render::ScreenshotHudHit::ModeRegion => {
                        if let Some(hud) = self.screenshot_hud.as_mut() {
                            hud.mode = crate::shell_render::ScreenshotMode::Region;
                            self.shell_redraw_needed = true;
                        }
                    }
                    crate::shell_render::ScreenshotHudHit::ModeWindow => {
                        if let Some(hud) = self.screenshot_hud.as_mut() {
                            hud.mode = crate::shell_render::ScreenshotMode::Window;
                            self.shell_redraw_needed = true;
                        }
                    }
                    crate::shell_render::ScreenshotHudHit::ModeFullScreen => {
                        if let Some(hud) = self.screenshot_hud.as_mut() {
                            hud.mode = crate::shell_render::ScreenshotMode::FullScreen;
                            self.shell_redraw_needed = true;
                        }
                    }
                    crate::shell_render::ScreenshotHudHit::ToggleCopyOnly => {
                        if let Some(hud) = self.screenshot_hud.as_mut() {
                            hud.copy_only = !hud.copy_only;
                            self.shell_redraw_needed = true;
                        }
                    }
                    crate::shell_render::ScreenshotHudHit::Capture => {
                        self.trigger_screenshot_hud_capture();
                    }
                    crate::shell_render::ScreenshotHudHit::Cancel => {
                        self.close_screenshot_hud();
                    }
                }
                return;
            }
            let mode = self.screenshot_hud.as_ref().map(|h| h.mode);
            match mode {
                Some(crate::shell_render::ScreenshotMode::Region) => {
                    if let Some(hud) = self.screenshot_hud.as_mut() {
                        hud.drag_start = Some((px, py));
                        hud.drag_current = Some((px, py));
                        hud.selection = None;
                        self.shell_redraw_needed = true;
                    }
                }
                Some(
                    crate::shell_render::ScreenshotMode::Window
                    | crate::shell_render::ScreenshotMode::FullScreen,
                ) => {
                    self.trigger_screenshot_hud_capture();
                }
                None => {}
            }
        } else if let Some(hud) = self.screenshot_hud.as_mut()
            && hud.mode == crate::shell_render::ScreenshotMode::Region
            && let Some(start) = hud.drag_start.take()
        {
            hud.drag_current = None;
            hud.selection = crate::shell_render::normalize_selection_rect(start, (px, py));
            self.shell_redraw_needed = true;
        }
    }

    /// Handles keyboard input while `ScreenshotHud` is active (`RUSTYOS-133`).
    pub fn handle_screenshot_hud_key(&mut self, raw_sym: u32) {
        use smithay::input::keyboard::Keysym;
        if raw_sym == Keysym::Escape.raw() {
            self.close_screenshot_hud();
        } else if raw_sym == Keysym::Return.raw()
            || raw_sym == Keysym::KP_Enter.raw()
            || raw_sym == Keysym::space.raw()
        {
            self.trigger_screenshot_hud_capture();
        } else if let Some(hud) = self.screenshot_hud.as_mut() {
            if raw_sym == Keysym::_1.raw() {
                hud.mode = crate::shell_render::ScreenshotMode::Region;
                self.shell_redraw_needed = true;
            } else if raw_sym == Keysym::_2.raw() {
                hud.mode = crate::shell_render::ScreenshotMode::Window;
                self.shell_redraw_needed = true;
            } else if raw_sym == Keysym::_3.raw() {
                hud.mode = crate::shell_render::ScreenshotMode::FullScreen;
                self.shell_redraw_needed = true;
            }
        }
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
            self.close_wifi_popover();
            self.close_notification_center();
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
                "Desktop".to_owned(),
                crate::shell_render::TopBarAppProfile::Desktop,
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

    pub(crate) fn dock_key_for_hit(&self, hit: crate::shell_render::DockHit) -> Option<String> {
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

    /// Builds ordered RustyOS Dock tiles with app grouping (`RUSTYOS-073`).
    #[must_use]
    pub fn build_dock_tiles(&self) -> Vec<crate::shell_render::DockAppTile> {
        let records = self.collect_dock_window_records();
        let mut tiles =
            Vec::with_capacity(self.user_config.dock_pinned_apps.len() + records.len() + 4);
        let mut pinned_keys = std::collections::HashSet::new();

        let explicit_firefox_pin = self
            .user_config
            .dock_pinned_apps
            .iter()
            .any(|k| k.trim().eq_ignore_ascii_case("firefox"));
        let firefox_default = !explicit_firefox_pin
            && (self
                .launcher
                .find_command_for_dock_key("browser")
                .is_some_and(|cmd| cmd.to_ascii_lowercase().contains("firefox"))
                || records.iter().any(|rec| rec.app_key == "firefox")
                || self
                    .recent_unpinned_apps
                    .iter()
                    .any(|rec| rec.app_key == "firefox"));

        for (pin_idx, raw_key) in self.user_config.dock_pinned_apps.iter().enumerate() {
            let key = raw_key.trim().to_ascii_lowercase();
            if key.is_empty() || !pinned_keys.insert(key.clone()) {
                continue;
            }
            let merge_firefox = key == "browser" && firefox_default;
            if merge_firefox {
                pinned_keys.insert("firefox".to_owned());
            }
            let matching: Vec<&DockWindowRecord> = records
                .iter()
                .filter(|rec| rec.app_key == key || (merge_firefox && rec.app_key == "firefox"))
                .collect();
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
                .or_else(|| self.launcher.icon_for_dock_key(&key))
                .or_else(|| merge_firefox.then(|| "firefox".to_owned()));
            let toplevel_icon = matching.iter().find_map(|r| r.toplevel_icon.clone());
            let label = if merge_firefox {
                "Firefox".to_owned()
            } else {
                crate::shell_render::dock_label_for_key(&key)
            };
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
        crate::shell_render::append_utility_dock_tiles(&mut tiles, self.trash_has_items);
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

    pub fn launch_terminal_in_dir(&mut self, dir: Option<&std::path::Path>) -> bool {
        self.close_dock_stack();
        self.close_overview();
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        if let Some(cmd) = self.launcher.find_command_for_dock_key("terminal") {
            let launched = if let Some(cwd) = dir.and_then(|d| d.to_str())
                && matches!(
                    std::path::Path::new(&cmd)
                        .file_name()
                        .and_then(|n| n.to_str()),
                    Some("rusty-terminal" | "foot" | "alacritty" | "kitty")
                ) {
                self.launcher
                    .launch_program_with_args(
                        &cmd,
                        &["--working-directory", cwd],
                        &self.socket_name,
                        &ipc_path,
                    )
                    .is_ok()
            } else {
                self.launcher
                    .launch_command(&cmd, &self.socket_name, &ipc_path)
                    .is_ok()
            };
            if launched {
                self.trigger_app_bounce("terminal", false);
                return true;
            }
        }
        self.toggle_launcher();
        false
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

    pub(in crate::state) fn launch_rusty_settings_args(&mut self, args: &[&str]) -> bool {
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        let launched = self
            .launcher
            .launch_program_with_args("rusty-settings", args, &self.socket_name, &ipc_path)
            .is_ok();
        if launched {
            self.trigger_app_bounce("settings", false);
        }
        launched
    }

    pub(in crate::state) fn launch_rusty_settings(&mut self) -> bool {
        self.launch_rusty_settings_args(&[])
    }

    fn wayland_socket_exists(&self) -> bool {
        !self.socket_name.is_empty()
            && std::env::var_os("XDG_RUNTIME_DIR")
                .map(|dir| std::path::PathBuf::from(dir).join(&self.socket_name))
                .is_some_and(|path| path.exists())
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
        self.close_wifi_popover();
        self.close_notification_center();
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
            crate::shell_render::DockHit::PinnedSettings => self.open_settings(),
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

    /// Opens or focuses the standalone `rusty-settings` Wayland window, optionally on `page` (`RUSTYOS-103`, `RUSTYOS-128`).
    pub fn open_settings_page(&mut self, page: Option<&str>) {
        if self.is_session_locked() {
            return;
        }
        self.close_dock_stack();
        self.close_context_menu();
        self.close_launcher();
        self.close_quick_settings();
        self.close_updates_popover();
        self.close_wifi_popover();
        self.close_notification_center();
        self.close_overview();
        self.close_top_bar_menu();

        let existing = self
            .collect_dock_window_records()
            .into_iter()
            .find(|r| r.app_key == "settings")
            .map(|r| r.id);
        if let Some(win_id) = existing {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            if self.is_window_minimized(win_id) {
                self.restore_window(win_id, serial);
            } else {
                self.wm.focus_and_raise(win_id);
                self.sync_space_with_wm(serial);
            }
            self.shell_redraw_needed = true;
            return;
        }

        if self.wayland_socket_exists() {
            let launched = match page {
                Some(p) => self.launch_rusty_settings_args(&["--page", p]),
                None => self.launch_rusty_settings(),
            };
            if launched {
                self.shell_redraw_needed = true;
            }
        }
    }

    /// Opens or focuses the standalone `rusty-settings` Wayland window (`RUSTYOS-103`, `RUSTYOS-128`).
    pub fn open_settings(&mut self) {
        self.open_settings_page(None);
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
                    info!(path = %path.display(), "Saved RustyOS user configuration");
                }
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        path = %path.display(),
                        "Failed to persist RustyOS user configuration"
                    );
                }
            }
        }
        self.shell_redraw_needed = true;
    }

    /// Applies an updated [`crate::config::UserConfig`] to live compositor subsystems without writing disk (`RUSTYOS-061`/`062`).
    #[allow(clippy::too_many_lines)]
    pub fn apply_user_config(&mut self, cfg: crate::config::UserConfig) {
        let repeat_changed = cfg.repeat_delay != self.user_config.repeat_delay
            || cfg.repeat_rate != self.user_config.repeat_rate;
        let xkb_changed = cfg.keyboard_layout != self.user_config.keyboard_layout
            || cfg.keyboard_variant != self.user_config.keyboard_variant
            || cfg.keyboard_options != self.user_config.keyboard_options;
        let wallpaper_changed =
            cfg.wallpaper != self.user_config.wallpaper || cfg.wallpaper.is_some();
        let appearance_changed = cfg.appearance != self.user_config.appearance
            || cfg.ui_accent_index != self.user_config.ui_accent_index;
        let dock_layout_changed = cfg.dock_position != self.user_config.dock_position
            || cfg.dock_icon_size != self.user_config.dock_icon_size
            || cfg.dock_auto_hide != self.user_config.dock_auto_hide;
        let updates_changed = cfg.updates_enabled != self.updates_enabled
            || cfg.updates_interval_minutes != self.user_config.updates_interval_minutes;
        let power_mode_changed = cfg.power_low_power_mode != self.user_config.power_low_power_mode;

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
        self.notification_center_tasks = cfg.notifications_reminders_done;

        self.sync_output_configs(&cfg);

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

        if wallpaper_changed || appearance_changed {
            let is_dark = !matches!(cfg.appearance, crate::config::AppearanceMode::Light);
            if let Some(ref wp) = cfg.wallpaper
                && crate::surfaces::resolve_effective_wallpaper_path(Some(wp.as_path()), is_dark)
                    .is_none_or(|p| !p.exists())
            {
                tracing::warn!(
                    path = %wp.display(),
                    "Configured wallpaper file does not exist; falling back to built-in gradient"
                );
            }
            self.wallpaper_revision = self.wallpaper_revision.wrapping_add(1);
        }

        self.user_config = cfg;
        self.telemetry
            .set_clock_locale(&self.user_config.clock_locale);
        if power_mode_changed {
            let pwr = self.telemetry.snapshot().power;
            let profile = self
                .user_config
                .power_low_power_mode
                .desired_profile(pwr.ac_online, pwr.has_battery);
            self.telemetry.set_power_profile(profile);
        }
        crate::shell_render::set_active_appearance(self.user_config.appearance);
        crate::shell_render::set_active_accent_index(self.user_config.ui_accent_index);
        if appearance_changed {
            for toplevel in self
                .window_bindings
                .iter()
                .filter_map(|(_, sw)| sw.toplevel())
            {
                if toplevel.alive() && toplevel.is_initial_configure_sent() {
                    toplevel.send_configure();
                }
            }
        }
        if appearance_changed && self.config_path.is_some() {
            let prefer_dark = !matches!(
                self.user_config.appearance,
                crate::config::AppearanceMode::Light
            );
            rusty_services::ThemeSyncService::apply_async(rusty_services::ThemeSyncConfig {
                prefer_dark,
                icon_theme: None,
                cursor_theme: self.user_config.cursor_theme.clone(),
                cursor_size: self.user_config.cursor_size,
                font_name: None,
            });
        }
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

    fn sync_output_configs(&mut self, cfg: &crate::config::UserConfig) {
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
                self.show_osd("Config Reloaded", None, false);
                info!(path = %path.display(), "Reloaded RustyOS configuration from disk");
                true
            }
            Err(err) => {
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
}
