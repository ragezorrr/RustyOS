//! Per-output Dock visibility (`AlwaysShow`/`SmartHide`/`AlwaysHide`), multi-monitor layout,
//! smooth [`crate::shell_render::DockPresentation`] synchronization, graceful Quit tracking,
//! and package-upgrade pin metadata grace (`RUSTYOS-161`).

use std::time::Duration;

use rusty_wm::{OutputId, Rect, WindowId};
use smithay::utils::{Logical, Physical, Point, Size};

use crate::config::DockVisibilityMode;
use crate::shell_render::{
    ComputedDockLayout, ContextMenuTarget, DockAppTile, DockViewState, DockWindowEntry,
    TOP_BAR_HEIGHT, compute_dock_layout, dock_label_for_key, dock_obstruction_region,
};

use super::{
    CachedDockPinMeta, DOCK_AUTO_HIDE_DELAY, DOCK_PIN_UNINSTALL_GRACE, DOCK_QUIT_GRACE,
    OutputDockState, PendingAppQuit, RustyCompositorState,
};

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.width > 0
        && a.height > 0
        && b.width > 0
        && b.height > 0
        && a.x < b.x + b.width
        && a.x + a.width > b.x
        && a.y < b.y + b.height
        && a.y + a.height > b.y
}

impl RustyCompositorState {
    /// Resolves the display output under `pos` (falling back to primary) together with
    /// `(output_id, is_primary, logical_size_phys, local_x, local_y)` (`RUSTYOS-161`).
    #[must_use]
    pub fn output_local_coords(
        &self,
        pos: Point<f64, Logical>,
    ) -> (OutputId, bool, Size<i32, Physical>, f64, f64) {
        let entry = self
            .output_map
            .output_under(pos)
            .and_then(|id| self.output_map.get(id))
            .unwrap_or_else(|| self.output_map.primary());
        let geo = entry.logical_geometry();
        let size = Size::<i32, Physical>::from((geo.size.w.max(320), geo.size.h.max(240)));
        let local_x = pos.x - f64::from(geo.loc.x);
        let local_y = pos.y - f64::from(geo.loc.y);
        (entry.id, entry.is_primary, size, local_x, local_y)
    }

    /// Logical size of `output_id` typed as [`Physical`] for shell layout functions (`RUSTYOS-161`).
    #[must_use]
    pub fn output_logical_size(&self, output_id: OutputId) -> Size<i32, Physical> {
        let geo = self
            .output_map
            .get(output_id)
            .unwrap_or_else(|| self.output_map.primary())
            .logical_geometry();
        Size::from((geo.size.w.max(320), geo.size.h.max(240)))
    }

    /// Returns `true` when `output_id` hosts a blocking top/overlay layer-shell surface (`RUSTYOS-161`).
    #[must_use]
    pub fn has_foreground_layer_surfaces_on_id(&self, output_id: OutputId) -> bool {
        self.output_map
            .get(output_id)
            .is_some_and(|entry| self.has_foreground_layer_surfaces_on(&entry.output))
    }

    /// Current auto-hide reveal factor (`0.0..=1.0`) for the Dock on `output_id` (`RUSTYOS-161`).
    #[must_use]
    pub fn dock_reveal_for_output(&self, output_id: OutputId) -> f64 {
        let vis = self.user_config.effective_dock_visibility();
        if !vis.uses_auto_hide_reveal() {
            return 1.0;
        }
        self.output_docks.get(&output_id).map_or_else(
            || self.dock_auto_hide_anim.current(),
            |d| d.reveal_anim.current(),
        )
    }

    /// Returns `true` when a visible, non-minimized window on `output_id` is maximized,
    /// fullscreen, or overlaps the resting Dock region on `output_id` (`RUSTYOS-161`).
    #[must_use]
    pub fn is_dock_obstructed_on_output(&self, output_id: OutputId) -> bool {
        let Some(entry) = self.output_map.get(output_id) else {
            return false;
        };
        let geo = entry.logical_geometry();
        let logical_size = Size::<i32, Physical>::from((geo.size.w.max(320), geo.size.h.max(240)));
        let tiles = self.build_dock_tiles();
        let rest_view = DockViewState {
            tiles: &tiles,
            icon_size: self.user_config.dock_icon_size,
            position: self.user_config.dock_position,
            show_indicators: self.user_config.dock_show_indicators,
            top_inset: if entry.is_primary { TOP_BAR_HEIGHT } else { 0 },
            ..DockViewState::default()
        };
        let local_region = dock_obstruction_region(logical_size, &rest_view);
        let dock_global = Rect::new(
            local_region.x + geo.loc.x,
            local_region.y + geo.loc.y,
            local_region.width,
            local_region.height,
        );
        self.wm
            .visible_windows_for_output(output_id)
            .into_iter()
            .any(|win| {
                if self.is_window_minimized(win.id) {
                    return false;
                }
                win.maximized || win.fullscreen || rects_intersect(win.geometry, dock_global)
            })
    }

    /// Updates pointer hover / reveal-zone state for `output_id` and syncs Dock visibility (`RUSTYOS-161`).
    pub fn set_dock_hovered_on_output(&mut self, output_id: OutputId, hovered: bool) {
        let vis = self.user_config.effective_dock_visibility();
        let initial = if matches!(vis, DockVisibilityMode::AlwaysHide) {
            0.0
        } else {
            1.0
        };
        let mut changed_outputs = Vec::new();
        if hovered {
            let other_ids: Vec<OutputId> = self
                .output_docks
                .iter()
                .filter_map(|(&id, st)| (id != output_id && st.hovered).then_some(id))
                .collect();
            for other in other_ids {
                if let Some(st) = self.output_docks.get_mut(&other) {
                    st.hovered = false;
                }
                changed_outputs.push(other);
            }
        }
        let entry = self
            .output_docks
            .entry(output_id)
            .or_insert_with(|| OutputDockState::new(initial));
        if entry.hovered != hovered {
            entry.hovered = hovered;
            changed_outputs.push(output_id);
        }
        for out_id in changed_outputs {
            self.sync_dock_visibility_for_output(out_id, true);
        }
        self.mirror_primary_dock_state();
    }

    /// Recomputes window obstruction and updates Dock reveal animations across all outputs (`RUSTYOS-161`).
    pub fn sync_dock_visibility_for_all_outputs(&mut self, allow_hide_delay: bool) {
        let output_ids: Vec<OutputId> = self
            .output_map
            .entries()
            .iter()
            .map(|entry| entry.id)
            .collect();
        for output_id in output_ids {
            self.sync_dock_visibility_for_output(output_id, allow_hide_delay);
        }
        self.mirror_primary_dock_state();
    }

    /// Recomputes window obstruction and updates Dock reveal state for a single output (`RUSTYOS-161`).
    pub fn sync_dock_visibility_for_output(&mut self, output_id: OutputId, allow_hide_delay: bool) {
        let vis = self.user_config.effective_dock_visibility();
        let obstructed = matches!(vis, DockVisibilityMode::SmartHide)
            && self.is_dock_obstructed_on_output(output_id);
        let primary_id = self.output_map.primary().id;
        let has_stack = self
            .dock_stack
            .as_ref()
            .is_some_and(|s| s.output_id == output_id);
        let has_drag = self
            .dock_drag
            .as_ref()
            .is_some_and(|d| d.output_id == output_id)
            || self.dock_drop_highlight_for_output(output_id).is_some();
        let has_menu = self.context_menu.as_ref().is_some_and(|ctx| {
            ctx.output_id == output_id && matches!(ctx.target, ContextMenuTarget::DockSlot(_))
        });
        let active_out = self.active_dock_output.unwrap_or(primary_id);
        let has_kdb_focus = self.dock_focused_slot.is_some() && active_out == output_id;
        let overview_visible = self.overview_active && output_id == primary_id;
        let now = self.elapsed();
        let reduced_motion = self.animations.reduced_motion();
        let anim_speed = self.user_config.dock_animation_speed;

        let initial = if matches!(vis, DockVisibilityMode::AlwaysHide) {
            0.0
        } else {
            1.0
        };
        let state = self
            .output_docks
            .entry(output_id)
            .or_insert_with(|| OutputDockState::new(initial));
        state.obstructed = obstructed;
        state.reveal_anim.set_speed_multiplier(anim_speed);

        let interactive = state.hovered || has_stack || has_drag || has_menu || has_kdb_focus;
        let should_show = match vis {
            DockVisibilityMode::AlwaysShow => true,
            DockVisibilityMode::SmartHide => !obstructed || interactive || overview_visible,
            DockVisibilityMode::AlwaysHide => interactive || overview_visible,
        };

        if should_show {
            state.hide_deadline = None;
            if (state.reveal_anim.target() - 1.0).abs() > 1e-4 {
                state.reveal_anim.retarget(1.0, now, reduced_motion);
                self.shell_redraw_needed = true;
            }
        } else if allow_hide_delay && !reduced_motion && state.reveal_anim.target() > 0.5 {
            if state.hide_deadline.is_none() {
                state.hide_deadline = Some(now + DOCK_AUTO_HIDE_DELAY);
                self.shell_redraw_needed = true;
            }
        } else {
            state.hide_deadline = None;
            if state.reveal_anim.target().abs() > 1e-4 {
                state.reveal_anim.retarget(0.0, now, reduced_motion);
                self.shell_redraw_needed = true;
            }
        }
    }

    fn mirror_primary_dock_state(&mut self) {
        let primary_id = self.output_map.primary().id;
        if let Some(primary_dock) = self.output_docks.get(&primary_id) {
            self.dock_auto_hide_anim = primary_dock.reveal_anim;
            self.dock_auto_hide_hovered = primary_dock.hovered;
        }
    }

    /// Returns `true` if any per-output Dock reveal, hide-delay timer, or slot presentation is active (`RUSTYOS-161`).
    #[must_use]
    pub fn has_active_dock_animations(&self) -> bool {
        self.output_docks.values().any(|dock| {
            dock.reveal_anim.is_active()
                || dock.hide_deadline.is_some()
                || dock.presentation.is_active()
        }) || !self.pending_app_quits.is_empty()
    }

    /// Advances all per-output Dock reveal animations, 200 ms hide-delay timers, slot presentations,
    /// and graceful app Quit grace timers (`RUSTYOS-161`).
    pub(crate) fn tick_dock_states(&mut self, now: Duration) {
        let reduced_motion = self.animations.reduced_motion();
        let anim_speed = self.user_config.dock_animation_speed;
        self.dock_hover_anim.set_speed_multiplier(anim_speed);
        let mut redraw = false;
        for dock in self.output_docks.values_mut() {
            dock.reveal_anim.set_speed_multiplier(anim_speed);
            dock.presentation.set_speed_multiplier(anim_speed);
            if let Some(deadline) = dock.hide_deadline
                && now >= deadline
            {
                dock.hide_deadline = None;
                if dock.reveal_anim.target().abs() > 1e-4 {
                    dock.reveal_anim.retarget(0.0, now, reduced_motion);
                    redraw = true;
                }
            }
            let reveal_was_active = dock.reveal_anim.is_active();
            let prev_reveal = dock.reveal_anim.current();
            let next_reveal = dock.reveal_anim.tick(now);
            if reveal_was_active || (next_reveal - prev_reveal).abs() > 1e-6 {
                redraw = true;
            }
            if dock.presentation.tick(now) {
                redraw = true;
            }
        }
        if !self.dock_hover_anim.is_active()
            && self.dock_hover_anim.current() <= 0.002
            && self.dock_hover_slot.is_none()
        {
            self.dock_pointer_axis_pos = None;
        }
        self.mirror_primary_dock_state();
        if redraw {
            self.shell_redraw_needed = true;
        }

        if !self.pending_app_quits.is_empty() {
            let keys: Vec<String> = self.pending_app_quits.keys().cloned().collect();
            let mut timed_out_labels = Vec::new();
            for key in keys {
                let still_running = !self.matching_windows_for_dock_key(&key).is_empty();
                if !still_running {
                    self.pending_app_quits.remove(&key);
                    continue;
                }
                if let Some(pending) = self.pending_app_quits.get_mut(&key)
                    && !pending.notified_timeout
                    && now >= pending.started_at + pending.grace
                {
                    pending.notified_timeout = true;
                    timed_out_labels.push(pending.label.clone());
                }
            }
            for label in timed_out_labels {
                self.show_osd(
                    format!("{label} still running — right-click to Force Quit"),
                    None,
                    false,
                );
            }
        }
    }

    /// Resets all per-output [`crate::shell_render::DockPresentation`] states so layout snaps cleanly.
    pub fn reset_all_dock_presentations(&mut self) {
        for dock in self.output_docks.values_mut() {
            dock.presentation.reset();
        }
    }

    /// Builds a [`DockViewState`] scoped to `output_id` with optional pre-animated layout (`RUSTYOS-161`).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn dock_view_state_for_output<'a>(
        &'a self,
        output_id: OutputId,
        tiles: &'a [DockAppTile],
        windows: &'a [DockWindowEntry],
        bounces: &'a [(&'a str, f64)],
        presented_layout: Option<&'a ComputedDockLayout>,
    ) -> DockViewState<'a> {
        let is_primary = self
            .output_map
            .get(output_id)
            .is_none_or(|entry| entry.is_primary);
        let hover_slot = if self.active_dock_output == Some(output_id) {
            self.dock_hover_slot
        } else {
            None
        };
        let drag_preview = self
            .dock_drag
            .as_ref()
            .filter(|d| d.output_id == output_id)
            .and_then(|_| self.dock_drag_preview());
        DockViewState {
            pinned_icons: [None, None],
            windows,
            tiles,
            hover_slot,
            overview_active: self.overview_active && is_primary,
            hover_progress: self.dock_hover_anim.current(),
            reduced_motion: self.animations.reduced_motion(),
            icon_size: self.user_config.dock_icon_size,
            position: self.user_config.dock_position,
            auto_hide_reveal: self.dock_reveal_for_output(output_id),
            show_indicators: self.user_config.dock_show_indicators,
            bounces,
            drag_preview,
            top_inset: if is_primary { TOP_BAR_HEIGHT } else { 0 },
            presented_layout,
            drop_highlight: self.dock_drop_highlight_for_output(output_id),
            pointer_axis_pos: if self.active_dock_output == Some(output_id) {
                self.dock_pointer_axis_pos
            } else {
                None
            },
            focused_slot: if self.active_dock_output == Some(output_id) {
                self.dock_focused_slot
            } else {
                None
            },
            magnification_enabled: self.user_config.dock_magnification_enabled,
            magnification_max: self.user_config.dock_magnification_max,
            show_tooltips: self.user_config.dock_show_tooltips,
        }
    }

    /// Computes the presented Dock layout for `output_id` without mutating presentation targets (`RUSTYOS-161`).
    #[must_use]
    pub fn dock_layout_for_output(
        &self,
        output_id: OutputId,
        tiles: &[DockAppTile],
    ) -> ComputedDockLayout {
        let screen_size = self.output_logical_size(output_id);
        let bounces = self.dock_bounce_offsets();
        let view = self.dock_view_state_for_output(output_id, tiles, &[], &bounces, None);
        let target = compute_dock_layout(screen_size, &view);
        self.output_docks.get(&output_id).map_or_else(
            || target.clone(),
            |dock| {
                dock.presentation
                    .apply(&target, self.user_config.dock_position)
            },
        )
    }

    /// Synchronizes `output_id`'s [`crate::shell_render::DockPresentation`] with the latest `tiles`
    /// and returns the interpolated [`ComputedDockLayout`] for this frame (`RUSTYOS-161`).
    pub fn sync_dock_presentation_for_output(
        &mut self,
        output_id: OutputId,
        tiles: &[DockAppTile],
    ) -> ComputedDockLayout {
        let screen_size = self.output_logical_size(output_id);
        let bounces = self.dock_bounce_offsets();
        let view = self.dock_view_state_for_output(output_id, tiles, &[], &bounces, None);
        let target = compute_dock_layout(screen_size, &view);
        let pos = self.user_config.dock_position;
        let now = self.elapsed();
        let reduced_motion = self.animations.reduced_motion();
        let initial = if matches!(
            self.user_config.effective_dock_visibility(),
            DockVisibilityMode::AlwaysHide
        ) {
            0.0
        } else {
            1.0
        };
        let dock = self
            .output_docks
            .entry(output_id)
            .or_insert_with(|| OutputDockState::new(initial));
        dock.presentation
            .set_speed_multiplier(self.user_config.dock_animation_speed);
        dock.presentation
            .sync_target(&target, pos, screen_size, now, reduced_motion);
        dock.presentation.apply(&target, pos)
    }

    /// Records the most recently focused window for its canonical Dock application key (`RUSTYOS-161`).
    pub(crate) fn record_focused_app_window(&mut self, focused_id: Option<WindowId>) {
        let Some(win_id) = focused_id else {
            return;
        };
        if self.is_window_minimized(win_id) {
            return;
        }
        let title = self.window_title(win_id);
        let app_id = self.window_app_id(win_id);
        let key = crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, win_id);
        self.dock_app_last_active.insert(key.clone(), win_id);
        self.clear_notification_badge(&key);
        if key == "firefox" && self.is_browser_merged_with_firefox() {
            self.dock_app_last_active
                .insert("browser".to_owned(), win_id);
            self.clear_notification_badge("browser");
        }
    }

    /// Updates the explicit `com.canonical.Unity.LauncherEntry` badge counter for `app_key` (`RUSTYOS-162`).
    pub fn set_unity_launcher_badge(&mut self, app_key: &str, count: u32, visible: bool) {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let clamped = count.min(9999);
        let changed = if visible && clamped > 0 {
            self.dock_unity_badges.insert(key, clamped) != Some(clamped)
        } else {
            self.dock_unity_badges.remove(&key).is_some()
        };
        if changed {
            self.shell_redraw_needed = true;
        }
    }

    /// Increments the notification-derived unread badge count for `app_key` when not currently focused (`RUSTYOS-162`).
    pub fn increment_notification_badge(&mut self, app_key: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let currently_focused = self.wm.focused_window().is_some_and(|win_id| {
            !self.is_window_minimized(win_id) && {
                let title = self.window_title(win_id);
                let app_id = self.window_app_id(win_id);
                let canon =
                    crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, win_id);
                canon == key
                    || (key == "browser"
                        && canon == "firefox"
                        && self.is_browser_merged_with_firefox())
            }
        });
        if currently_focused {
            return;
        }
        let entry = self.dock_unread_notif_badges.entry(key).or_insert(0);
        *entry = entry.saturating_add(1).min(999);
        self.shell_redraw_needed = true;
    }

    /// Decrements the notification-derived unread badge count for `app_key` (`RUSTYOS-162`).
    pub fn decrement_notification_badge(&mut self, app_key: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        if let Some(count) = self.dock_unread_notif_badges.get_mut(&key) {
            if *count <= 1 {
                self.dock_unread_notif_badges.remove(&key);
            } else {
                *count -= 1;
            }
            self.shell_redraw_needed = true;
        }
    }

    /// Clears the notification-derived unread badge for `app_key` (`RUSTYOS-162`).
    pub fn clear_notification_badge(&mut self, app_key: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        if self.dock_unread_notif_badges.remove(&key).is_some() {
            self.shell_redraw_needed = true;
        }
    }

    /// Resolves the effective Dock unread count badge for `app_key` (`RUSTYOS-162`).
    ///
    /// Explicit `com.canonical.Unity.LauncherEntry` counts take precedence over notification-derived
    /// unread counters; `"browser"` merges `"firefox"` when Firefox owns the Browser slot.
    #[must_use]
    pub fn dock_badge_count_for_key(&self, app_key: &str) -> u32 {
        let key = app_key.trim().to_ascii_lowercase();
        let merge_firefox = key == "browser" && self.is_browser_merged_with_firefox();
        let unity = self.dock_unity_badges.get(&key).copied().or_else(|| {
            merge_firefox
                .then(|| self.dock_unity_badges.get("firefox").copied())
                .flatten()
        });
        if let Some(count) = unity
            && count > 0
        {
            return count;
        }
        let notif_direct = self
            .dock_unread_notif_badges
            .get(&key)
            .copied()
            .unwrap_or(0);
        let notif_ff = if merge_firefox {
            self.dock_unread_notif_badges
                .get("firefox")
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        notif_direct.saturating_add(notif_ff)
    }

    /// Returns `true` when the `"browser"` Dock slot merges `"firefox"` windows (default Firefox pin behavior).
    #[must_use]
    pub(crate) fn is_browser_merged_with_firefox(&self) -> bool {
        let explicit_firefox_pin = self
            .user_config
            .dock_pinned_apps
            .iter()
            .any(|k| k.trim().eq_ignore_ascii_case("firefox"));
        if explicit_firefox_pin {
            return false;
        }
        self.launcher
            .find_command_for_dock_key("browser")
            .is_some_and(|cmd| cmd.to_ascii_lowercase().contains("firefox"))
            || self
                .recent_unpinned_apps
                .iter()
                .any(|rec| rec.app_key == "firefox")
            || self.ordered_visible_window_ids().into_iter().any(|id| {
                let title = self.window_title(id);
                let app_id = self.window_app_id(id);
                crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, id)
                    == "firefox"
            })
    }

    /// Begins a `3000 ms` graceful Quit grace window for `app_key` (`RUSTYOS-161`).
    pub fn begin_graceful_app_quit(&mut self, app_key: &str, label: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        let now = self.elapsed();
        self.pending_app_quits.insert(
            key.clone(),
            PendingAppQuit {
                app_key: key,
                label: label.to_owned(),
                started_at: now,
                grace: DOCK_QUIT_GRACE,
                notified_timeout: false,
            },
        );
    }

    /// Clears any pending graceful Quit tracker for `app_key` (e.g. after Force Quit) (`RUSTYOS-161`).
    pub fn clear_pending_app_quit(&mut self, app_key: &str) {
        let key = app_key.trim().to_ascii_lowercase();
        self.pending_app_quits.remove(&key);
    }

    /// Returns `true` if a graceful Quit was already requested for `app_key` and windows remain open (`RUSTYOS-161`).
    #[must_use]
    pub fn is_app_quit_pending(&self, app_key: &str) -> bool {
        let key = app_key.trim().to_ascii_lowercase();
        self.pending_app_quits.contains_key(&key)
    }

    /// Refreshes cached metadata for pinned Dock apps, retaining cached icons/labels for a `3000 ms`
    /// grace period when a `.desktop` file vanishes temporarily during package upgrades (`RUSTYOS-161`).
    pub fn refresh_dock_pin_metadata_cache(&mut self) {
        let now = self.elapsed();
        let pinned: Vec<String> = self
            .user_config
            .dock_pinned_apps
            .iter()
            .map(|k| k.trim().to_ascii_lowercase())
            .filter(|k| !k.is_empty())
            .collect();
        for key in &pinned {
            let builtin = crate::shell_render::dock_builtin_for_key(key).is_some();
            let available = builtin || self.launcher.has_installed_dock_entry(key);
            if available {
                let icon_name = self.launcher.icon_for_dock_key(key);
                let command = self.launcher.find_command_for_dock_key(key);
                let label = dock_label_for_key(key);
                self.dock_pin_meta_cache.insert(
                    key.clone(),
                    CachedDockPinMeta {
                        icon_name,
                        label,
                        command,
                        missing_since: None,
                    },
                );
            } else if let Some(cached) = self.dock_pin_meta_cache.get_mut(key) {
                cached.missing_since.get_or_insert(now);
            } else {
                self.dock_pin_meta_cache.insert(
                    key.clone(),
                    CachedDockPinMeta {
                        icon_name: None,
                        label: dock_label_for_key(key),
                        command: None,
                        missing_since: Some(now),
                    },
                );
            }
        }
        self.dock_pin_meta_cache
            .retain(|k, _| pinned.iter().any(|p| p == k));
    }

    /// Returns `true` while a pinned app's desktop entry is missing within the `3000 ms` upgrade grace (`RUSTYOS-161`).
    #[must_use]
    pub fn dock_pin_in_upgrade_grace(&self, app_key: &str) -> bool {
        let key = app_key.trim().to_ascii_lowercase();
        self.dock_pin_meta_cache
            .get(&key)
            .and_then(|meta| meta.missing_since)
            .is_some_and(|since| self.elapsed().saturating_sub(since) < DOCK_PIN_UNINSTALL_GRACE)
    }

    /// Returns cached pin metadata for fallback icon/label resolution during package upgrades (`RUSTYOS-161`).
    #[must_use]
    pub fn cached_dock_pin_meta(&self, app_key: &str) -> Option<&CachedDockPinMeta> {
        let key = app_key.trim().to_ascii_lowercase();
        self.dock_pin_meta_cache.get(&key)
    }
}
