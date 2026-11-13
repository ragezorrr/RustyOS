//! Shell overlay, `TopBar`, `Dock`, Quick Settings, Overview, Polkit, Portal, and Lock Screen pointer hits.

use rusty_wm::{WmAction, WorkspaceId};
use smithay::utils::{Logical, Point, Size};
use tracing::info;

use crate::state::RustyCompositorState;

impl RustyCompositorState {
    pub(super) fn visible_ssd_hit_windows(&self) -> Vec<crate::shell_render::SsdHitWindow> {
        self.wm
            .visible_windows()
            .into_iter()
            .filter(|w| !self.is_window_minimized(w.id))
            .map(|w| crate::shell_render::SsdHitWindow {
                id: w.id,
                rect: w.geometry,
                has_ssd: !self.window_uses_csd(w.id),
            })
            .collect()
    }

    pub(super) fn update_dock_hover_slot(&mut self, pos: Point<f64, Logical>) {
        let (primary_size, is_on_primary, local_x, local_y) = self.primary_local_coords(pos);

        let next_slot = if is_on_primary
            && !self.launcher_active
            && !self.quick_settings_active
            && !self.has_foreground_layer_surfaces()
            && !self.should_hide_shell_chrome_for_fullscreen()
        {
            crate::shell_render::check_dock_slot(
                primary_size,
                self.wm.visible_windows().len(),
                local_x,
                local_y,
            )
        } else {
            None
        };
        self.set_dock_hover_slot(next_slot);
    }

    pub(super) fn apply_display_slider_percent(&mut self, track_pct: u8) {
        if self.telemetry.snapshot().brightness.available {
            let hw_pct = track_pct.clamp(5, 100);
            let _ = self.telemetry.set_brightness_percent(hw_pct);
        } else {
            let glass_pct = 35_u16 + (u16::from(track_pct.min(100)) * 65 + 50) / 100;
            self.set_glass_opacity(f32::from(glass_pct.clamp(35, 100)) / 100.0);
        }
    }

    pub(super) fn apply_quick_settings_slider_drag(
        &mut self,
        drag: crate::shell_render::QuickSettingsSliderDrag,
        pos: Point<f64, Logical>,
    ) {
        let (primary_size, _on_primary, local_x, _local_y) = self.primary_local_coords(pos);
        let pct = crate::shell_render::quick_settings_slider_percent(primary_size, drag, local_x);
        match drag {
            crate::shell_render::QuickSettingsSliderDrag::Sound => {
                self.telemetry.set_volume_percent(pct);
            }
            crate::shell_render::QuickSettingsSliderDrag::Display => {
                self.apply_display_slider_percent(pct);
            }
        }
    }

    pub(super) fn update_shell_hover_states(&mut self, pos: Point<f64, Logical>) {
        let (primary_size, is_on_primary, local_x, local_y) = self.primary_local_coords(pos);
        let chrome_hidden =
            self.has_foreground_layer_surfaces() || self.should_hide_shell_chrome_for_fullscreen();
        let fullscreen_active = self
            .topmost_fullscreen_window_on(self.output_map.primary().id)
            .is_some();

        self.top_bar_menu_hover_row = if is_on_primary && !chrome_hidden {
            if let Some(menu) = self.active_top_bar_menu {
                let active_app = self.active_top_bar_app_name();
                crate::shell_render::check_top_bar_menu_popover_hover_row(
                    &active_app,
                    menu,
                    local_x,
                    local_y,
                )
            } else {
                None
            }
        } else {
            None
        };

        let visible_toast_count = if self.focus_mode {
            0
        } else {
            self.notifications
                .len()
                .min(crate::shell_render::MAX_VISIBLE_NOTIFICATIONS)
        };
        self.hovered_notification_idx = if is_on_primary
            && visible_toast_count > 0
            && !self.launcher_active
            && !self.quick_settings_active
            && self.window_switcher.is_none()
        {
            crate::shell_render::check_notification_click(
                primary_size,
                visible_toast_count,
                local_x,
                local_y,
            )
        } else {
            None
        };

        self.hovered_traffic_light_window = if !fullscreen_active
            && !self.launcher_active
            && !self.quick_settings_active
            && !self.overview_active
            && self.active_top_bar_menu.is_none()
            && self.window_switcher.is_none()
            && self.active_window_drag.is_none()
            && self.active_window_resize.is_none()
        {
            let hit_windows = self.visible_ssd_hit_windows();
            crate::shell_render::check_window_traffic_lights_hover(&hit_windows, pos.x, pos.y)
        } else {
            None
        };
    }

    pub(super) fn handle_top_bar_pointer_click(
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

    pub(super) fn handle_top_bar_right_cluster_click(
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

    pub(super) fn handle_launcher_pointer_click(&mut self) {
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

    pub(super) fn handle_quick_settings_pointer_click(
        &mut self,
        serial: smithay::utils::Serial,
        button: u32,
    ) {
        let (output_size, _on_primary, local_x, local_y) =
            self.primary_local_coords(self.pointer_location);
        match crate::shell_render::check_quick_settings_click(output_size, local_x, local_y) {
            crate::shell_render::QuickSettingsHit::ToggleWifi => {
                let _ = self.telemetry.toggle_wifi();
            }
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
            crate::shell_render::QuickSettingsHit::SetDisplayPercent(pct) => {
                self.apply_display_slider_percent(pct);
            }
            crate::shell_render::QuickSettingsHit::MediaPrev => {
                self.telemetry
                    .dispatch_media(rusty_services::MediaAction::Previous);
            }
            crate::shell_render::QuickSettingsHit::MediaPlayPause => {
                self.telemetry
                    .dispatch_media(rusty_services::MediaAction::PlayPause);
            }
            crate::shell_render::QuickSettingsHit::MediaNext => {
                self.telemetry
                    .dispatch_media(rusty_services::MediaAction::Next);
            }
            crate::shell_render::QuickSettingsHit::ToggleLayout => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            crate::shell_render::QuickSettingsHit::ToggleOverview => {
                self.toggle_overview();
            }
            crate::shell_render::QuickSettingsHit::ToggleReducedMotion => {
                let reduced = !self.animations.reduced_motion();
                self.set_reduced_motion_pref(reduced);
            }
            crate::shell_render::QuickSettingsHit::ToggleFocus => {
                let _ = self.toggle_focus_mode();
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
                return;
            }
        }

        self.qs_slider_drag = if button == 272 && self.quick_settings_active {
            crate::shell_render::check_quick_settings_slider_drag(output_size, local_x, local_y)
        } else {
            None
        };
    }

    pub(super) fn handle_overview_pointer_click(
        &mut self,
        output_phys: Size<i32, smithay::utils::Physical>,
        window_count: usize,
        serial: smithay::utils::Serial,
    ) {
        let spaces = self.overview_workspace_spaces();
        match crate::shell_render::check_overview_click_with_spaces(
            output_phys,
            &spaces,
            window_count,
            self.pointer_location.x,
            self.pointer_location.y,
        ) {
            crate::shell_render::OverviewHit::Card(idx) => {
                self.focus_ordered_window(idx, serial);
            }
            crate::shell_render::OverviewHit::SwitchWorkspace(ws) => {
                let target_ws = WorkspaceId::new_clamped(ws);
                if target_ws != self.wm.active_workspace_id() {
                    self.apply_wm_action(WmAction::SwitchWorkspace(target_ws), serial);
                }
                self.close_overview();
            }
            crate::shell_render::OverviewHit::Dismiss => {
                self.close_overview();
            }
        }
    }

    pub(crate) fn handle_polkit_pointer_click(
        &mut self,
        output_phys: Size<i32, smithay::utils::Physical>,
        serial: smithay::utils::Serial,
    ) {
        match crate::shell_render::hit_test_polkit_dialog(
            output_phys,
            self.pointer_location.x,
            self.pointer_location.y,
        ) {
            crate::shell_render::PolkitHit::Submit => self.submit_polkit_password(),
            crate::shell_render::PolkitHit::Cancel => self.cancel_polkit_prompt(serial),
            crate::shell_render::PolkitHit::PasswordField
            | crate::shell_render::PolkitHit::Outside => {}
        }
    }

    pub(crate) fn handle_portal_pointer_click(
        &mut self,
        output_phys: Size<i32, smithay::utils::Physical>,
        serial: smithay::utils::Serial,
    ) {
        let Some(prompt) = self.portal_prompt.as_ref() else {
            return;
        };
        let hit = crate::shell_render::hit_test_portal_dialog(
            output_phys,
            prompt.candidates.len(),
            prompt.selected,
            prompt.is_screencast && prompt.cursor_mode_editable,
            self.pointer_location.x,
            self.pointer_location.y,
        );
        match hit {
            crate::shell_render::PortalHit::Row(idx) => {
                if let Some(p) = self.portal_prompt.as_mut()
                    && idx < p.candidates.len()
                {
                    p.selected = idx;
                    self.shell_redraw_needed = true;
                }
            }
            crate::shell_render::PortalHit::CursorToggle => self.toggle_portal_include_cursor(),
            crate::shell_render::PortalHit::Accept => self.accept_portal_prompt(serial),
            crate::shell_render::PortalHit::Cancel => self.cancel_portal_prompt(serial),
            crate::shell_render::PortalHit::Outside => {}
        }
    }

    pub(crate) fn handle_lock_screen_pointer_click(
        &mut self,
        output_phys: Size<i32, smithay::utils::Physical>,
        serial: smithay::utils::Serial,
    ) {
        let tel = self.telemetry.snapshot();
        let lock_view = self.lock_screen_render_state(&tel, 1.0);
        match crate::shell_render::hit_test_lock_screen(
            output_phys,
            &lock_view,
            self.pointer_location.x,
            self.pointer_location.y,
        ) {
            crate::shell_render::LockScreenHit::UnlockUser(_) => {
                self.lock_power_menu_open = false;
                if !self.user_config.require_password_on_lock {
                    self.set_screen_locked(false, serial);
                    info!("Unlocked desktop session Lock Screen via user click");
                } else if !self.lock_password.is_empty() {
                    self.submit_lock_password(serial);
                }
            }
            crate::shell_render::LockScreenHit::SubmitPassword => {
                self.lock_power_menu_open = false;
                self.submit_lock_password(serial);
            }
            crate::shell_render::LockScreenHit::SelectUser(idx) => {
                self.lock_power_menu_open = false;
                self.select_lock_user(idx);
            }
            crate::shell_render::LockScreenHit::ToggleKeyboardLayout => {
                self.lock_power_menu_open = false;
                self.cycle_keyboard_layout();
            }
            crate::shell_render::LockScreenHit::TogglePowerMenu => {
                self.toggle_lock_power_menu();
            }
            crate::shell_render::LockScreenHit::DismissPowerMenu => {
                self.lock_power_menu_open = false;
            }
            crate::shell_render::LockScreenHit::PowerAction(action) => {
                self.lock_power_menu_open = false;
                self.telemetry.dispatch_session(action);
                info!(?action, "Dispatched Lock Screen power menu action");
            }
            crate::shell_render::LockScreenHit::None => {}
        }
    }
}
