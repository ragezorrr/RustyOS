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
            .map(|w| {
                let has_ssd = !w.fullscreen && !self.window_uses_csd(w.id);
                let rect = self
                    .resolved_window_global_rect(w.id)
                    .map_or(w.geometry, |r| {
                        rusty_wm::Rect::new(w.geometry.x, w.geometry.y, r.width, r.height)
                    });
                crate::shell_render::SsdHitWindow {
                    id: w.id,
                    rect,
                    has_ssd,
                    allow_resize: !w.fullscreen && !w.maximized,
                }
            })
            .collect()
    }

    pub(super) fn update_dock_hover_slot(&mut self, pos: Point<f64, Logical>) {
        let (output_id, is_primary, logical_size, local_x, local_y) = self.output_local_coords(pos);
        let drag_reveal_active = self.is_any_dock_drop_drag_active();
        let chrome_allowed = (!self.launcher_active || drag_reveal_active)
            && !self.quick_settings_active
            && !self.updates_popover_open
            && !self.wifi_popover_open
            && !self.notification_center_open
            && !self.has_foreground_layer_surfaces_on_id(output_id)
            && !self.should_hide_shell_chrome_for_fullscreen_on(output_id);

        if chrome_allowed && let Some(drag_output) = self.dock_drag.as_ref().map(|d| d.output_id) {
            let drag_geo = self
                .output_map
                .get(drag_output)
                .unwrap_or_else(|| self.output_map.primary())
                .logical_geometry();
            let drag_size = Size::from((drag_geo.size.w.max(320), drag_geo.size.h.max(240)));
            let drag_local_x = pos.x - f64::from(drag_geo.loc.x);
            let drag_local_y = pos.y - f64::from(drag_geo.loc.y);
            self.active_dock_output = Some(drag_output);
            self.update_dock_drag_motion(drag_output, drag_size, drag_local_x, drag_local_y);
            self.set_dock_hovered_on_output(drag_output, true);
            return;
        }
        if !chrome_allowed {
            self.set_dock_hovered_on_output(output_id, false);
            self.set_dock_hover_slot(None);
            return;
        }

        let tiles = self.build_dock_tiles();
        let layout = self.dock_layout_for_output(output_id, &tiles);
        let next_slot = crate::shell_render::check_dock_layout_slot(&layout, local_x, local_y);
        let bounces = self.dock_bounce_offsets();
        let dock_view =
            self.dock_view_state_for_output(output_id, &tiles, &[], &bounces, Some(&layout));

        let mut hovered_this_dock = next_slot.is_some();
        if self
            .user_config
            .effective_dock_visibility()
            .uses_auto_hide_reveal()
        {
            let reveal = self.dock_reveal_for_output(output_id);
            let in_edge = crate::shell_render::is_pointer_in_dock_auto_hide_zone(
                logical_size,
                &dock_view,
                reveal > 0.25,
                local_x,
                local_y,
            );
            let c = layout.capsule_rect;
            let pad = 14;
            let near_capsule = reveal > 0.05
                && local_x >= f64::from(c.loc.x - pad)
                && local_x < f64::from(c.loc.x + c.size.w + pad)
                && local_y >= f64::from(c.loc.y - pad)
                && local_y < f64::from(c.loc.y + c.size.h + pad);
            let in_stack = self
                .dock_stack_render_state_for_output(output_id)
                .and_then(|st| {
                    crate::shell_render::hit_test_dock_stack(logical_size, &st, local_x, local_y)
                })
                .is_some();
            hovered_this_dock = in_edge || near_capsule || in_stack;
            self.set_dock_hovered_on_output(output_id, hovered_this_dock);
        }

        if hovered_this_dock || (is_primary && next_slot.is_some()) {
            self.active_dock_output = Some(output_id);
        }
        if self.active_dock_output == Some(output_id) {
            self.set_dock_hover_slot(next_slot);
        } else {
            self.set_dock_hover_slot(None);
        }
    }

    fn nearest_dock_section_index(
        slots: &[crate::shell_render::ComputedDockSlot],
        section: crate::shell_render::DockSection,
        position: crate::config::DockPosition,
        local_x: f64,
        local_y: f64,
        fallback: usize,
    ) -> usize {
        let mut best_idx = fallback;
        let mut best_dist = f64::MAX;
        let mut ordinal = 0_usize;
        let cur = match position {
            crate::config::DockPosition::Bottom => local_x,
            crate::config::DockPosition::Left | crate::config::DockPosition::Right => local_y,
        };
        for slot in slots {
            if slot.tile.section != section {
                continue;
            }
            let center = match position {
                crate::config::DockPosition::Bottom => {
                    f64::from(slot.hit_rect.loc.x) + f64::from(slot.hit_rect.size.w) * 0.5
                }
                crate::config::DockPosition::Left | crate::config::DockPosition::Right => {
                    f64::from(slot.hit_rect.loc.y) + f64::from(slot.hit_rect.size.h) * 0.5
                }
            };
            let d = (cur - center).abs();
            if d < best_dist {
                best_dist = d;
                best_idx = ordinal;
            }
            ordinal += 1;
        }
        best_idx
    }

    fn update_dock_drag_motion(
        &mut self,
        output_id: rusty_wm::OutputId,
        output_size: Size<i32, smithay::utils::Physical>,
        local_x: f64,
        local_y: f64,
    ) {
        let Some(ref drag_snapshot) = self.dock_drag else {
            return;
        };
        let dist = (local_x - drag_snapshot.start_pointer.x)
            .hypot(local_y - drag_snapshot.start_pointer.y);
        let can_drag = drag_snapshot.source_pinned_idx.is_some()
            || drag_snapshot.source_minimized_idx.is_some();
        let active_drag = can_drag && (drag_snapshot.active_drag || dist > 6.0);
        let app_key = drag_snapshot.app_key.clone();
        let source_pinned = drag_snapshot.source_pinned_idx;
        let source_minimized = drag_snapshot.source_minimized_idx;
        let mut target_pinned_idx = drag_snapshot.target_pinned_idx;
        let mut target_minimized_idx = drag_snapshot.target_minimized_idx;
        let mut pulled_outside = false;

        if active_drag {
            let is_primary = self
                .output_map
                .get(output_id)
                .is_none_or(|entry| entry.is_primary);
            let tiles = self.build_dock_tiles();
            let rest_view = crate::shell_render::DockViewState {
                tiles: &tiles,
                icon_size: self.user_config.dock_icon_size,
                position: self.user_config.dock_position,
                show_indicators: self.user_config.dock_show_indicators,
                top_inset: if is_primary {
                    crate::shell_render::TOP_BAR_HEIGHT
                } else {
                    0
                },
                ..crate::shell_render::DockViewState::default()
            };
            let rest_layout = crate::shell_render::compute_dock_layout(output_size, &rest_view);
            if source_pinned.is_some() {
                let cap = rest_layout.capsule_rect;
                let outside_dist = match self.user_config.dock_position {
                    crate::config::DockPosition::Bottom => f64::from(cap.loc.y) - local_y,
                    crate::config::DockPosition::Left => {
                        local_x - f64::from(cap.loc.x + cap.size.w)
                    }
                    crate::config::DockPosition::Right => f64::from(cap.loc.x) - local_x,
                };
                pulled_outside =
                    outside_dist > 52.0 && crate::shell_render::is_dock_pin_removable(&app_key);

                if !pulled_outside {
                    target_pinned_idx = Self::nearest_dock_section_index(
                        &rest_layout.slots,
                        crate::shell_render::DockSection::Pinned,
                        self.user_config.dock_position,
                        local_x,
                        local_y,
                        target_pinned_idx,
                    );
                }
            } else if source_minimized.is_some() {
                target_minimized_idx = Self::nearest_dock_section_index(
                    &rest_layout.slots,
                    crate::shell_render::DockSection::MinimizedWindow,
                    self.user_config.dock_position,
                    local_x,
                    local_y,
                    target_minimized_idx,
                );
            }
        }

        if let Some(ref mut drag) = self.dock_drag {
            drag.current_pointer = Point::from((local_x, local_y));
            drag.active_drag = active_drag;
            drag.target_pinned_idx = target_pinned_idx;
            drag.target_minimized_idx = target_minimized_idx;
            drag.pulled_outside = pulled_outside;
        }
        if active_drag {
            self.set_dock_hover_slot(None);
            self.shell_redraw_needed = true;
        }
    }

    pub(super) fn handle_dock_stack_pointer_press(&mut self, button: u32) -> bool {
        let Some(stack_output_id) = self.dock_stack.as_ref().map(|s| s.output_id) else {
            return false;
        };
        let (output_id, _, logical_size, local_x, local_y) =
            self.output_local_coords(self.pointer_location);
        if output_id == stack_output_id
            && let Some(hit) = self
                .dock_stack_render_state_for_output(output_id)
                .and_then(|st| {
                    crate::shell_render::hit_test_dock_stack(logical_size, &st, local_x, local_y)
                })
        {
            match hit {
                crate::shell_render::DockStackHit::Entry(idx) => {
                    let path = self
                        .dock_stack
                        .as_ref()
                        .and_then(|s| s.entries.get(idx).map(|e| e.path.clone()));
                    self.close_dock_stack();
                    if let Some(target_path) = path {
                        self.open_downloads_entry(&target_path);
                    }
                    return true;
                }
                crate::shell_render::DockStackHit::OpenInFiles => {
                    self.close_dock_stack();
                    let downloads = crate::state::context_menu_actions::resolve_downloads_dir();
                    if !self.launch_rusty_files_dir(downloads.as_deref()) {
                        self.toggle_launcher();
                    }
                    return true;
                }
                crate::shell_render::DockStackHit::InsideBackground => {
                    return true;
                }
            }
        }
        let clicked_downloads_toggle = output_id == stack_output_id && button == 272 && {
            let tiles = self.build_dock_tiles();
            let layout = self.dock_layout_for_output(output_id, &tiles);
            crate::shell_render::check_dock_layout_slot(&layout, local_x, local_y).is_some_and(
                |idx| {
                    layout
                        .slots
                        .iter()
                        .find(|s| s.slot_idx == idx)
                        .is_some_and(|s| {
                            s.tile.hit == crate::shell_render::DockHit::PinnedDownloads
                        })
                },
            )
        };
        self.close_dock_stack();
        clicked_downloads_toggle
    }

    pub(super) fn handle_dock_pointer_press(
        &mut self,
        output_id: rusty_wm::OutputId,
        local_x: f64,
        local_y: f64,
        button: u32,
    ) -> bool {
        if self
            .user_config
            .effective_dock_visibility()
            .uses_auto_hide_reveal()
            && self.dock_reveal_for_output(output_id) <= 0.25
        {
            return false;
        }
        let logical_size = self.output_logical_size(output_id);
        let tiles = self.build_dock_tiles();
        let layout = self.dock_layout_for_output(output_id, &tiles);
        let Some(slot_idx) = crate::shell_render::check_dock_layout_slot(&layout, local_x, local_y)
        else {
            return false;
        };
        let Some(slot) = layout.slots.iter().find(|s| s.slot_idx == slot_idx) else {
            return false;
        };
        self.active_dock_output = Some(output_id);
        if button == 273 {
            let alt_held = self.keyboard.modifier_state().alt;
            self.open_dock_context_menu_on_output(
                output_id,
                slot.tile.hit,
                logical_size,
                self.pointer_location,
                alt_held,
            );
            return true;
        }
        if button == 272 {
            let source_pinned_idx = (slot.tile.section == crate::shell_render::DockSection::Pinned)
                .then(|| {
                    self.user_config
                        .dock_pinned_apps
                        .iter()
                        .position(|k| k.eq_ignore_ascii_case(&slot.tile.app_key))
                })
                .flatten();
            let (source_minimized_win, source_minimized_idx) = if slot.tile.section
                == crate::shell_render::DockSection::MinimizedWindow
            {
                let win_id = slot.tile.representative_win;
                let idx = layout
                    .slots
                    .iter()
                    .filter(|s| s.tile.section == crate::shell_render::DockSection::MinimizedWindow)
                    .position(|s| s.tile.representative_win == win_id);
                (win_id, idx)
            } else {
                (None, None)
            };
            self.dock_drag = Some(crate::state::DockDragState {
                output_id,
                slot_idx,
                app_key: slot.tile.app_key.clone(),
                source_pinned_idx,
                source_minimized_win,
                source_minimized_idx,
                start_pointer: Point::from((local_x, local_y)),
                current_pointer: Point::from((local_x, local_y)),
                active_drag: false,
                target_pinned_idx: source_pinned_idx.unwrap_or(0),
                target_minimized_idx: source_minimized_idx.unwrap_or(0),
                pulled_outside: false,
                hit: slot.tile.hit,
            });
            self.sync_dock_auto_hide_target();
            return true;
        }
        false
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
        if self.context_menu.is_some() {
            self.update_context_menu_hover_at(pos);
            return;
        }
        let (primary_size, is_on_primary, local_x, local_y) = self.primary_local_coords(pos);
        let chrome_hidden =
            self.has_foreground_layer_surfaces() || self.should_hide_shell_chrome_for_fullscreen();
        self.update_dock_stack_hover(pos);
        self.update_updates_popover_hover(is_on_primary, chrome_hidden, local_x, local_y);
        self.update_wifi_popover_hover(is_on_primary, chrome_hidden, local_x, local_y);
        self.update_top_bar_quick_popover_hover(is_on_primary, chrome_hidden, local_x, local_y);
        self.update_notification_center_hover(is_on_primary, chrome_hidden, local_x, local_y);
        let fullscreen_active = self
            .topmost_fullscreen_window_on(self.output_map.primary().id)
            .is_some();

        self.top_bar_menu_hover_row = if is_on_primary && !chrome_hidden {
            if let Some(menu) = self.active_top_bar_menu {
                let (active_app, app_profile) = self.active_top_bar_app_context();
                crate::shell_render::check_top_bar_menu_popover_hover_row_for_profile(
                    app_profile,
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
            && !self.updates_popover_open
            && !self.wifi_popover_open
            && !self.notification_center_open
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
            && !self.updates_popover_open
            && !self.wifi_popover_open
            && !self.notification_center_open
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

    fn update_dock_stack_hover(&mut self, pos: Point<f64, Logical>) {
        let Some(stack_output_id) = self.dock_stack.as_ref().map(|s| s.output_id) else {
            return;
        };
        let (output_id, _, logical_size, local_x, local_y) = self.output_local_coords(pos);
        if output_id != stack_output_id
            || self.has_foreground_layer_surfaces_on_id(output_id)
            || self.should_hide_shell_chrome_for_fullscreen_on(output_id)
        {
            return;
        }
        let stack_hit = self
            .dock_stack_render_state_for_output(output_id)
            .and_then(|st| {
                crate::shell_render::hit_test_dock_stack(logical_size, &st, local_x, local_y)
            });
        let (new_item, new_footer) = match stack_hit {
            Some(crate::shell_render::DockStackHit::Entry(idx)) => (Some(idx), false),
            Some(crate::shell_render::DockStackHit::OpenInFiles) => (None, true),
            _ => (None, false),
        };
        if let Some(ref mut stack) = self.dock_stack
            && (stack.hovered_item != new_item || stack.hovered_open_in_files != new_footer)
        {
            stack.hovered_item = new_item;
            stack.hovered_open_in_files = new_footer;
            self.shell_redraw_needed = true;
        }
    }

    fn update_updates_popover_hover(
        &mut self,
        is_on_primary: bool,
        chrome_hidden: bool,
        local_x: f64,
        local_y: f64,
    ) {
        if !(self.updates_popover_open && is_on_primary && !chrome_hidden) {
            return;
        }
        let logical_size = self.primary_output_logical_size();
        let updates_view = self.updates_render_state(logical_size.w);
        let next_hover = match crate::shell_render::hit_test_updates_popover(
            logical_size,
            &updates_view,
            local_x,
            local_y,
        ) {
            hit @ (crate::shell_render::UpdatesPopoverHit::CheckNow
            | crate::shell_render::UpdatesPopoverHit::OpenUpdater) => Some(hit),
            crate::shell_render::UpdatesPopoverHit::InsidePopover
            | crate::shell_render::UpdatesPopoverHit::OutsidePopover => None,
        };
        if self.updates_popover_hover != next_hover {
            self.updates_popover_hover = next_hover;
            self.shell_redraw_needed = true;
        }
    }

    fn update_wifi_popover_hover(
        &mut self,
        is_on_primary: bool,
        chrome_hidden: bool,
        local_x: f64,
        local_y: f64,
    ) {
        if !(self.wifi_popover_open && is_on_primary && !chrome_hidden) {
            return;
        }
        let logical_size = self.primary_output_logical_size();
        let tel = self.telemetry.snapshot();
        let wifi_view = self.wifi_render_state(logical_size.w, &tel.network);
        let next_hover = match crate::shell_render::hit_test_wifi_popover(
            logical_size,
            &wifi_view,
            local_x,
            local_y,
        ) {
            hit @ (crate::shell_render::WifiPopoverHit::ToggleWifi
            | crate::shell_render::WifiPopoverHit::SelectNetwork(_)
            | crate::shell_render::WifiPopoverHit::OpenWifiSettings) => Some(hit),
            crate::shell_render::WifiPopoverHit::InsidePopover
            | crate::shell_render::WifiPopoverHit::OutsidePopover => None,
        };
        if self.wifi_popover_hover != next_hover {
            self.wifi_popover_hover = next_hover;
            self.shell_redraw_needed = true;
        }
    }

    fn update_top_bar_quick_popover_hover(
        &mut self,
        is_on_primary: bool,
        chrome_hidden: bool,
        local_x: f64,
        local_y: f64,
    ) {
        let Some(kind) = self.top_bar_quick_popover else {
            return;
        };
        if !is_on_primary || chrome_hidden {
            return;
        }
        let logical_size = self.primary_output_logical_size();
        let tel = self.telemetry.snapshot();
        let state = self.top_bar_quick_popover_render_state(logical_size.w, kind, &tel);
        let next_hover = match crate::shell_render::hit_test_topbar_quick_popover(
            logical_size.w,
            &state,
            local_x,
            local_y,
        ) {
            crate::shell_render::TopBarQuickPopoverHit::InsidePopover
            | crate::shell_render::TopBarQuickPopoverHit::OutsidePopover => None,
            hit => Some(hit),
        };
        if self.top_bar_quick_popover_hover != next_hover {
            self.top_bar_quick_popover_hover = next_hover;
            self.shell_redraw_needed = true;
        }
    }

    pub(crate) fn handle_top_bar_quick_popover_pointer_click(&mut self, _button: u32) {
        let Some(kind) = self.top_bar_quick_popover else {
            return;
        };
        let logical_size = self.primary_output_logical_size();
        let tel = self.telemetry.snapshot();
        let state = self.top_bar_quick_popover_render_state(logical_size.w, kind, &tel);
        let hit = crate::shell_render::hit_test_topbar_quick_popover(
            logical_size.w,
            &state,
            self.pointer_location.x,
            self.pointer_location.y,
        );
        match hit {
            crate::shell_render::TopBarQuickPopoverHit::ToggleMute => {
                self.telemetry.toggle_mute();
                self.shell_redraw_needed = true;
            }
            crate::shell_render::TopBarQuickPopoverHit::VolumeDown => {
                self.telemetry.step_volume(-10);
                self.shell_redraw_needed = true;
            }
            crate::shell_render::TopBarQuickPopoverHit::VolumeUp => {
                self.telemetry.step_volume(10);
                self.shell_redraw_needed = true;
            }
            crate::shell_render::TopBarQuickPopoverHit::SelectAudioOutput(idx) => {
                self.telemetry.select_audio_output_index(idx);
                self.shell_redraw_needed = true;
            }
            crate::shell_render::TopBarQuickPopoverHit::ToggleBatteryPercentage => {
                self.toggle_top_bar_control_visibility("battery_percentage");
            }
            crate::shell_render::TopBarQuickPopoverHit::SetPowerProfile(profile) => {
                self.telemetry.set_power_profile(profile);
                self.shell_redraw_needed = true;
            }
            crate::shell_render::TopBarQuickPopoverHit::ToggleBluetooth => {
                self.telemetry.toggle_bluetooth();
                self.shell_redraw_needed = true;
            }
            crate::shell_render::TopBarQuickPopoverHit::OpenSettings => {
                self.close_top_bar_quick_popover();
                let page = match kind {
                    crate::shell_render::TopBarQuickPopoverKind::Volume => Some("sound"),
                    crate::shell_render::TopBarQuickPopoverKind::Battery => Some("battery"),
                    crate::shell_render::TopBarQuickPopoverKind::Bluetooth => Some("bluetooth"),
                };
                self.open_settings_page(page);
            }
            crate::shell_render::TopBarQuickPopoverHit::InsidePopover => {}
            crate::shell_render::TopBarQuickPopoverHit::OutsidePopover => {
                self.close_top_bar_quick_popover();
            }
        }
    }

    fn update_notification_center_hover(
        &mut self,
        is_on_primary: bool,
        chrome_hidden: bool,
        local_x: f64,
        local_y: f64,
    ) {
        if !(self.notification_center_open && is_on_primary && !chrome_hidden) {
            return;
        }
        let logical_size = self.primary_output_logical_size();
        let items = self.build_notification_center_items();
        let nc_view = self.notification_center_render_state(&items, false);
        let next_hover = match crate::shell_render::hit_test_notification_center(
            logical_size,
            &nc_view,
            local_x,
            local_y,
        ) {
            hit @ (crate::shell_render::NotificationCenterHit::DismissNotification(_)
            | crate::shell_render::NotificationCenterHit::ClearAll
            | crate::shell_render::NotificationCenterHit::ToggleTask(_)
            | crate::shell_render::NotificationCenterHit::EditWidgets
            | crate::shell_render::NotificationCenterHit::ClosePanel) => Some(hit),
            crate::shell_render::NotificationCenterHit::InsidePanel
            | crate::shell_render::NotificationCenterHit::OutsidePanel => None,
        };
        if self.notification_center_hover != next_hover {
            self.notification_center_hover = next_hover;
            self.shell_redraw_needed = true;
        }
    }

    pub(crate) fn handle_top_bar_pointer_click(
        &mut self,
        output_width: i32,
        serial: smithay::utils::Serial,
        button: u32,
    ) -> bool {
        let (active_app_name, app_profile) = self.active_top_bar_app_context();
        let live_app_menu = self.active_app_menu_tree();
        let x = self.pointer_location.x;
        let y = self.pointer_location.y;
        let was_menu_open = self.active_top_bar_menu.is_some();

        if let Some(open_menu) = self.active_top_bar_menu {
            match crate::shell_render::check_top_bar_menu_popover_click_with_app_menu(
                app_profile,
                &active_app_name,
                open_menu,
                live_app_menu.as_ref(),
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

        if let Some(menu_hit) = crate::shell_render::check_top_bar_menu_click_with_app_menu(
            app_profile,
            &active_app_name,
            live_app_menu.as_ref(),
            x,
            y,
        ) {
            self.toggle_top_bar_menu(menu_hit);
            return true;
        }

        if y <= f64::from(crate::shell_render::TOP_BAR_HEIGHT) {
            self.close_top_bar_menu();
        }

        if let Some(target_ws) = crate::shell_render::check_top_bar_workspace_click_for_profile(
            app_profile,
            &active_app_name,
            self.menubar_show_workspaces,
            x,
            y,
        ) {
            let ws = WorkspaceId::new_clamped(target_ws);
            self.apply_wm_action(WmAction::SwitchWorkspace(ws), serial);
            return true;
        }
        if crate::shell_render::check_top_bar_layout_click_for_profile(
            app_profile,
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

    #[allow(clippy::too_many_lines)]
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
        let opts = self.top_bar_layout_options_for(&tel);

        if crate::shell_render::check_top_bar_recording_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            self.stop_active_screen_recording();
            return true;
        }
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
        if crate::shell_render::check_top_bar_wifi_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            if button == 0x111 {
                self.toggle_top_bar_control_visibility("wifi");
            } else {
                self.toggle_wifi_popover();
            }
            return true;
        }
        if crate::shell_render::check_top_bar_volume_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            if button == 0x111 {
                self.toggle_top_bar_control_visibility("sound");
            } else {
                self.toggle_top_bar_quick_popover(
                    crate::shell_render::TopBarQuickPopoverKind::Volume,
                );
            }
            return true;
        }
        if crate::shell_render::check_top_bar_battery_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            if button == 0x111 {
                self.toggle_top_bar_control_visibility("battery");
            } else {
                self.toggle_top_bar_quick_popover(
                    crate::shell_render::TopBarQuickPopoverKind::Battery,
                );
            }
            return true;
        }
        if crate::shell_render::check_top_bar_bluetooth_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            if button == 0x111 {
                self.toggle_top_bar_control_visibility("bluetooth");
            } else {
                self.toggle_top_bar_quick_popover(
                    crate::shell_render::TopBarQuickPopoverKind::Bluetooth,
                );
            }
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
        if crate::shell_render::check_top_bar_clock_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            self.toggle_notification_center();
            return true;
        }
        if crate::shell_render::check_top_bar_updates_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            self.toggle_updates_popover();
            return true;
        }
        self.handle_top_bar_right_accessory_click(output_width, clock_text, opts, button, x, y)
    }

    fn handle_top_bar_right_accessory_click(
        &mut self,
        output_width: i32,
        clock_text: &str,
        opts: crate::shell_render::TopBarLayoutOptions,
        button: u32,
        x: f64,
        y: f64,
    ) -> bool {
        if crate::shell_render::check_top_bar_kbd_click_for_options(
            output_width,
            clock_text,
            opts,
            x,
            y,
        ) {
            if button == 273 {
                self.open_settings_page(Some("keyboard"));
            } else {
                self.cycle_keyboard_layout();
            }
            return true;
        }
        if let Some(tray_idx) = crate::shell_render::check_top_bar_tray_click_for_options(
            output_width,
            clock_text,
            opts,
            self.tray_items.len(),
            x,
            y,
        ) {
            return self.handle_top_bar_tray_item_click(tray_idx, button, x, y);
        }

        false
    }

    fn handle_top_bar_tray_item_click(
        &mut self,
        tray_idx: usize,
        button: u32,
        x: f64,
        y: f64,
    ) -> bool {
        let Some(item) = self.tray_items.get(tray_idx) else {
            return false;
        };
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
        true
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
                let (_, visible) = self.launcher.visible_window(5);
                if let Some(cand) = visible.get(row_idx)
                    && let Some(app_key) = cand.app_key.clone()
                {
                    self.spotlight_drag = Some(crate::state::SpotlightDragState {
                        row_idx,
                        app_key,
                        label: cand.name.clone(),
                        icon_name: cand.icon.clone(),
                        start_pointer: self.pointer_location,
                        current_pointer: self.pointer_location,
                        active_drag: false,
                        target: None,
                    });
                } else {
                    self.launch_spotlight_row(row_idx);
                }
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
                if button == 273 {
                    self.open_wifi_popover();
                } else {
                    let _ = self.telemetry.toggle_wifi();
                }
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
            crate::shell_render::QuickSettingsHit::CycleAudioOutput => {
                if button == 273 {
                    self.close_quick_settings();
                    self.toggle_top_bar_quick_popover(
                        crate::shell_render::TopBarQuickPopoverKind::Volume,
                    );
                } else {
                    self.telemetry.cycle_audio_output();
                    self.shell_redraw_needed = true;
                }
            }
            crate::shell_render::QuickSettingsHit::ToggleDarkMode => {
                self.toggle_appearance_mode();
            }
            crate::shell_render::QuickSettingsHit::OpenSettings => {
                self.close_quick_settings();
                self.open_settings_page(None);
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

    pub(super) fn handle_updates_popover_pointer_click(&mut self, button: u32) {
        let (output_size, _on_primary, local_x, local_y) =
            self.primary_local_coords(self.pointer_location);
        let view = self.updates_render_state(output_size.w);
        let hit =
            crate::shell_render::hit_test_updates_popover(output_size, &view, local_x, local_y);
        match hit {
            crate::shell_render::UpdatesPopoverHit::CheckNow if button == 272 => {
                self.trigger_updates_check();
            }
            crate::shell_render::UpdatesPopoverHit::OpenUpdater if button == 272 => {
                self.open_software_updater_action();
            }
            crate::shell_render::UpdatesPopoverHit::InsidePopover
            | crate::shell_render::UpdatesPopoverHit::CheckNow
            | crate::shell_render::UpdatesPopoverHit::OpenUpdater => {}
            crate::shell_render::UpdatesPopoverHit::OutsidePopover => {
                self.close_updates_popover();
            }
        }
    }

    pub(super) fn handle_wifi_popover_pointer_click(&mut self, button: u32) {
        let (output_size, _on_primary, local_x, local_y) =
            self.primary_local_coords(self.pointer_location);
        let tel = self.telemetry.snapshot();
        let view = self.wifi_render_state(output_size.w, &tel.network);
        let hit = crate::shell_render::hit_test_wifi_popover(output_size, &view, local_x, local_y);
        match hit {
            crate::shell_render::WifiPopoverHit::ToggleWifi if button == 272 => {
                let _ = self.telemetry.toggle_wifi();
                self.shell_redraw_needed = true;
            }
            crate::shell_render::WifiPopoverHit::SelectNetwork(idx) if button == 272 => {
                if let Some(ap) = tel.network.wifi_networks.get(idx) {
                    if ap.secure && !ap.known {
                        self.open_wifi_password_settings_action(&ap.ssid);
                    } else {
                        self.telemetry.connect_wifi(&ap.ssid);
                        self.shell_redraw_needed = true;
                    }
                }
            }
            crate::shell_render::WifiPopoverHit::OpenWifiSettings if button == 272 => {
                self.open_wifi_settings_action();
            }
            crate::shell_render::WifiPopoverHit::InsidePopover
            | crate::shell_render::WifiPopoverHit::ToggleWifi
            | crate::shell_render::WifiPopoverHit::SelectNetwork(_)
            | crate::shell_render::WifiPopoverHit::OpenWifiSettings => {}
            crate::shell_render::WifiPopoverHit::OutsidePopover => {
                self.close_wifi_popover();
            }
        }
    }

    pub(super) fn handle_notification_center_pointer_click(&mut self, button: u32) {
        let (output_size, _on_primary, local_x, local_y) =
            self.primary_local_coords(self.pointer_location);
        let items = self.build_notification_center_items();
        let view = self.notification_center_render_state(&items, false);
        let hit =
            crate::shell_render::hit_test_notification_center(output_size, &view, local_x, local_y);
        match hit {
            crate::shell_render::NotificationCenterHit::DismissNotification(id)
                if button == 272 =>
            {
                self.dismiss_notification(id);
                self.notification_center_hover = None;
                self.shell_redraw_needed = true;
            }
            crate::shell_render::NotificationCenterHit::ClearAll if button == 272 => {
                self.clear_notifications();
                self.notification_center_hover = None;
                self.shell_redraw_needed = true;
            }
            crate::shell_render::NotificationCenterHit::ToggleTask(idx) if button == 272 => {
                self.toggle_notification_center_task(idx);
            }
            crate::shell_render::NotificationCenterHit::EditWidgets if button == 272 => {
                self.open_notification_settings_action();
            }
            crate::shell_render::NotificationCenterHit::EditWidgets if button == 273 => {
                self.cycle_notification_center_widgets();
            }
            crate::shell_render::NotificationCenterHit::ClosePanel if button == 272 => {
                self.close_notification_center();
            }
            crate::shell_render::NotificationCenterHit::OutsidePanel => {
                self.close_notification_center();
            }
            crate::shell_render::NotificationCenterHit::InsidePanel
            | crate::shell_render::NotificationCenterHit::DismissNotification(_)
            | crate::shell_render::NotificationCenterHit::ClearAll
            | crate::shell_render::NotificationCenterHit::ToggleTask(_)
            | crate::shell_render::NotificationCenterHit::EditWidgets
            | crate::shell_render::NotificationCenterHit::ClosePanel => {}
        }
    }

    pub(super) fn handle_overview_pointer_click(
        &mut self,
        output_phys: Size<i32, smithay::utils::Physical>,
        window_count: usize,
        serial: smithay::utils::Serial,
    ) {
        let overview_ids = self.overview_window_ids();
        let card_count = if self.overview_filter_app.is_some() {
            overview_ids.len()
        } else {
            window_count
        };
        let spaces = self.overview_workspace_spaces();
        match crate::shell_render::check_overview_click_with_spaces(
            output_phys,
            &spaces,
            card_count,
            self.pointer_location.x,
            self.pointer_location.y,
        ) {
            crate::shell_render::OverviewHit::Card(idx) => {
                if let Some(&win_id) = overview_ids.get(idx) {
                    self.restore_window(win_id, serial);
                    self.close_overview();
                } else {
                    self.focus_ordered_window(idx, serial);
                }
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
