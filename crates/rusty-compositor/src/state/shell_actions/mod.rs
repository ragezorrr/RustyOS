//! Shell UI overlay actions, launcher, overview, notifications, and OSD helpers.

mod dock_and_config;
mod notifications;
mod overview;

use std::time::Duration;

use rusty_animation::{Easing, MotionTokens};
use rusty_ipc::Urgency;
use rusty_wm::{WindowId, WmAction};
use smithay::utils::Serial;
use tracing::info;

use super::{RustyCompositorState, TouchpadSwipeState};

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
            if !self.app_catalog_monitor.is_active() {
                self.launcher.request_async_reload();
            }
        }
        self.launcher.reset();
        self.retarget_launcher_anim();
    }

    /// Closes the Spotlight overlay and clears its active search query.
    pub fn close_launcher(&mut self) {
        self.launcher_active = false;
        if self.spotlight_drag.as_ref().is_some_and(|d| !d.active_drag) {
            self.spotlight_drag = None;
        }
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
                    app_key: n.app_key.as_deref(),
                    app_name: n.app_display_name.as_deref(),
                    title: &n.title,
                    body: &n.body,
                    age_label,
                    avatar: n.avatar.as_ref(),
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
            expanded_stacks: &self.expanded_notification_stacks,
            task_states: self.notification_center_tasks,
            task_labels: crate::shell_render::DEFAULT_NOTIFICATION_TASKS,
            hovered_hit: self.notification_center_hover,
            location_name: "Local Station",
            system_summary,
            show_weather_widget: self.user_config.notifications_show_weather_widget,
            show_reminders_widget: self.user_config.notifications_show_reminders_widget,
        }
    }

    /// Toggles expand/collapse for a notification card stack group (`RUSTYOS-173`).
    pub fn toggle_notification_center_stack(&mut self, root_id: u64) {
        let key = root_id.to_string();
        if self.expanded_notification_stacks.contains(&key) {
            self.expanded_notification_stacks.remove(&key);
        } else {
            self.expanded_notification_stacks.insert(key);
        }
        self.shell_redraw_needed = true;
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
        self.pressed_ssd_button = None;
        self.overview_hovered_card = None;
        self.overview_hovered_close = None;
        self.overview_hovered_space = None;
        self.overview_drag = None;
        self.set_compositor_cursor_override(None);
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
}
