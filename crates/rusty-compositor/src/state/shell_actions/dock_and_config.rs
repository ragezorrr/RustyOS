//! Dock tiles, stacks, bounce/drag actions, Spotlight/Settings launchers, and user config sync.

use rusty_wm::WindowId;
use smithay::utils::Serial;
use tracing::info;

use super::super::RustyCompositorState;
use super::DockWindowRecord;

impl RustyCompositorState {
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

    /// Returns the open Downloads Stack render state only when it is anchored on `output_id` (`RUSTYOS-161`).
    #[must_use]
    pub fn dock_stack_render_state_for_output(
        &self,
        output_id: rusty_wm::OutputId,
    ) -> Option<crate::shell_render::DockStackRenderState<'_>> {
        self.dock_stack
            .as_ref()
            .filter(|stack| stack.output_id == output_id)?;
        self.dock_stack_render_state()
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
            self.app_bounces.push(super::super::DockBounceState {
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

    /// Updates pointer edge/hover state for Dock Auto-hide reveal animations on the primary output (`RUSTYOS-073` / `RUSTYOS-161`).
    pub fn set_dock_auto_hide_hovered(&mut self, hovered: bool) {
        let primary_id = self.output_map.primary().id;
        self.set_dock_hovered_on_output(primary_id, hovered);
    }

    pub(crate) fn sync_dock_auto_hide_target(&mut self) {
        self.sync_dock_visibility_for_all_outputs(false);
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
        if let Some(source_idx) = drag.source_pinned_idx {
            return Some(crate::shell_render::DockDragPreview {
                section: crate::shell_render::DockSection::Pinned,
                app_key: &drag.app_key,
                minimized_win: None,
                source_idx,
                target_idx: drag.target_pinned_idx,
                pointer_xy: (drag.current_pointer.x, drag.current_pointer.y),
                pulled_outside: drag.pulled_outside,
            });
        }
        let source_idx = drag.source_minimized_idx?;
        let win_id = drag.source_minimized_win?;
        Some(crate::shell_render::DockDragPreview {
            section: crate::shell_render::DockSection::MinimizedWindow,
            app_key: &drag.app_key,
            minimized_win: Some(win_id),
            source_idx,
            target_idx: drag.target_minimized_idx,
            pointer_xy: (drag.current_pointer.x, drag.current_pointer.y),
            pulled_outside: false,
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
        let primary_id = self.output_map.primary().id;
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
            auto_hide_reveal: self.dock_reveal_for_output(primary_id),
            show_indicators: self.user_config.dock_show_indicators,
            bounces,
            drag_preview: self.dock_drag_preview(),
            top_inset: crate::shell_render::TOP_BAR_HEIGHT,
            presented_layout: None,
            drop_highlight: self.dock_drop_highlight_for_output(primary_id),
            pointer_axis_pos: if self.active_dock_output == Some(primary_id) {
                self.dock_pointer_axis_pos
            } else {
                None
            },
            focused_slot: if self.active_dock_output == Some(primary_id) {
                self.dock_focused_slot
            } else {
                None
            },
            magnification_enabled: self.user_config.dock_magnification_enabled,
            magnification_max: self.user_config.dock_magnification_max,
            show_tooltips: self.user_config.dock_show_tooltips,
        }
    }

    #[must_use]
    pub fn active_dock_layout(
        &self,
        tiles: &[crate::shell_render::DockAppTile],
    ) -> crate::shell_render::ComputedDockLayout {
        let output_id = self
            .active_dock_output
            .unwrap_or_else(|| self.output_map.primary().id);
        self.dock_layout_for_output(output_id, tiles)
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
            let cached_meta = self.cached_dock_pin_meta(&key);
            let missing_desktop = cached_meta.is_some_and(|meta| meta.missing_since.is_some());
            let icon_name = matching
                .iter()
                .find_map(|r| r.icon_name.clone())
                .or_else(|| {
                    if missing_desktop {
                        cached_meta.and_then(|meta| meta.icon_name.clone())
                    } else {
                        self.launcher.icon_for_dock_key(&key)
                    }
                })
                .or_else(|| cached_meta.and_then(|meta| meta.icon_name.clone()))
                .or_else(|| merge_firefox.then(|| "firefox".to_owned()));
            let toplevel_icon = matching.iter().find_map(|r| r.toplevel_icon.clone());
            let label = if merge_firefox {
                "Firefox".to_owned()
            } else if let Some(meta) = cached_meta.filter(|m| !m.label.is_empty()) {
                meta.label.clone()
            } else {
                crate::shell_render::dock_label_for_key(&key)
            };
            let badge_count = self.dock_badge_count_for_key(&key);
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
                badge_count,
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
            let badge_count = self.dock_badge_count_for_key(&rec.app_key);
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
                badge_count,
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
                let badge_count = self.dock_badge_count_for_key(&recent.app_key);
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
                    badge_count,
                    icon_name: recent.icon_name.clone(),
                    toplevel_icon: None,
                    fallback_title: recent.label.clone(),
                });
            }
        }

        if !self.user_config.dock_minimize_into_app {
            let mut emitted_min = std::collections::HashSet::new();
            let ordered_records: Vec<&DockWindowRecord> = self
                .minimized_dock_order
                .iter()
                .filter_map(|win_id| records.iter().find(|r| r.minimized && r.id == *win_id))
                .inspect(|r| {
                    emitted_min.insert(r.id);
                })
                .collect();
            for rec in ordered_records.into_iter().chain(
                records
                    .iter()
                    .filter(|r| r.minimized && !emitted_min.contains(&r.id)),
            ) {
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
                    badge_count: 0,
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

    pub(super) fn wayland_socket_exists(&self) -> bool {
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
        let target_output = self
            .active_dock_output
            .unwrap_or_else(|| self.output_map.primary().id);
        let tiles = self.build_dock_tiles();
        let layout = self.dock_layout_for_output(target_output, &tiles);
        let anchor_rect = layout
            .slots
            .iter()
            .find(|slot| slot.tile.hit == crate::shell_render::DockHit::PinnedDownloads)
            .map_or_else(
                || smithay::utils::Rectangle::new((0, 0).into(), (50, 50).into()),
                |slot| slot.icon_rect,
            );
        let entries =
            super::super::context_menu_actions::resolve_downloads_dir().map_or_default(|dir| {
                super::super::context_menu_actions::read_downloads_stack_entries(
                    &dir,
                    self.user_config.downloads_sort,
                    24,
                )
            });
        self.dock_stack = Some(super::super::DockStackState {
            output_id: target_output,
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
            self.refresh_dock_pin_metadata_cache();
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
            self.refresh_dock_pin_metadata_cache();
            self.sync_and_save_user_config();
            self.shell_redraw_needed = true;
        }
    }

    /// Commits a finished Dock drag to reorder minimized window tiles (`RUSTYOS-162`).
    pub fn commit_dock_minimized_reorder(&mut self, dragged_win: WindowId, target_idx: usize) {
        if !self.is_window_minimized(dragged_win) {
            return;
        }
        let records = self.collect_dock_window_records();
        let mut visible_order: Vec<WindowId> = self
            .minimized_dock_order
            .iter()
            .copied()
            .filter(|id| records.iter().any(|r| r.minimized && r.id == *id))
            .collect();
        for rec in records.iter().filter(|r| r.minimized) {
            if !visible_order.contains(&rec.id) {
                visible_order.push(rec.id);
            }
        }
        let Some(src_idx) = visible_order.iter().position(|id| *id == dragged_win) else {
            return;
        };
        let clamped_target = target_idx.min(visible_order.len().saturating_sub(1));
        if clamped_target == src_idx {
            return;
        }
        let moved = visible_order.remove(src_idx);
        visible_order.insert(clamped_target, moved);

        let visible_set: std::collections::HashSet<WindowId> =
            visible_order.iter().copied().collect();
        let mut next_order: Vec<WindowId> = self
            .minimized_dock_order
            .iter()
            .copied()
            .filter(|id| self.is_window_minimized(*id) && !visible_set.contains(id))
            .collect();
        next_order.extend(visible_order);
        self.minimized_dock_order = next_order;
        self.shell_redraw_needed = true;
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
                self.refresh_dock_pin_metadata_cache();
                self.sync_and_save_user_config();
                self.shell_redraw_needed = true;
            }
        } else {
            self.user_config.dock_pinned_apps.push(key);
            self.recent_unpinned_apps
                .retain(|rec| !rec.app_key.eq_ignore_ascii_case(app_key));
            self.refresh_dock_pin_metadata_cache();
            self.sync_and_save_user_config();
            self.shell_redraw_needed = true;
        }
    }

    /// Selects the next deterministic target window for an app Dock click (`RUSTYOS-161`).
    ///
    /// When `focused` is already one of `unmin_sorted`, cycles forward across `unmin_sorted`
    /// in stable ascending `WindowId` order. Otherwise returns `last_active` if still present,
    /// falling back to the last window in `unmin_sorted`.
    #[must_use]
    pub(crate) fn select_dock_app_target_window(
        unmin_sorted: &[WindowId],
        focused: Option<WindowId>,
        last_active: Option<WindowId>,
    ) -> Option<WindowId> {
        if unmin_sorted.is_empty() {
            return None;
        }
        if let Some(pos) = focused.and_then(|f| unmin_sorted.iter().position(|id| *id == f)) {
            if unmin_sorted.len() > 1 {
                return Some(unmin_sorted[(pos + 1) % unmin_sorted.len()]);
            }
            return Some(unmin_sorted[pos]);
        }
        last_active
            .filter(|id| unmin_sorted.contains(id))
            .or_else(|| unmin_sorted.last().copied())
    }

    fn focus_or_launch_dock_app(&mut self, app_key: &str, serial: Serial) {
        let key = app_key.trim().to_ascii_lowercase();
        let merge_firefox = key == "browser" && self.is_browser_merged_with_firefox();
        let records = self.collect_dock_window_records();
        let mut matching: Vec<&DockWindowRecord> = records
            .iter()
            .filter(|r| r.app_key == key || (merge_firefox && r.app_key == "firefox"))
            .collect();
        matching.sort_by_key(|r| r.id.0);
        if matching.is_empty() {
            self.launch_dock_app_new_instance(&key, true);
            return;
        }
        let unmin: Vec<WindowId> = matching
            .iter()
            .filter(|r| !r.minimized)
            .map(|r| r.id)
            .collect();
        if unmin.is_empty() {
            let first_min = self
                .dock_app_last_active
                .get(&key)
                .copied()
                .filter(|id| matching.iter().any(|r| r.id == *id))
                .unwrap_or(matching[0].id);
            self.restore_window(first_min, serial);
            self.dock_app_last_active.insert(key, first_min);
            self.close_overview();
            return;
        }
        let focused = self.wm.focused_window();
        let last_active = self.dock_app_last_active.get(&key).copied();
        let already_focused = focused.is_some_and(|f| unmin.contains(&f));
        if let Some(target) = Self::select_dock_app_target_window(&unmin, focused, last_active) {
            if !already_focused {
                for &id in &unmin {
                    if id != target {
                        self.wm.focus_and_raise(id);
                    }
                }
            }
            if !already_focused || unmin.len() > 1 {
                self.wm.focus_and_raise(target);
                self.dock_app_last_active.insert(key, target);
                self.sync_space_with_wm(serial);
            }
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
                    let downloads = super::super::context_menu_actions::resolve_downloads_dir();
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

    /// Focuses the Dock for keyboard navigation on the active output and reveals auto-hide if active.
    pub fn focus_dock(&mut self) {
        let tiles = self.build_dock_tiles();
        if tiles.is_empty() {
            return;
        }
        let primary_id = self.output_map.primary().id;
        if self.active_dock_output.is_none() {
            self.active_dock_output = Some(primary_id);
        }
        let default_idx = self
            .dock_hover_slot
            .or(self.dock_focused_slot)
            .unwrap_or(0)
            .min(tiles.len() - 1);
        self.dock_focused_slot = Some(default_idx);
        self.sync_dock_visibility_for_all_outputs(false);
        self.shell_redraw_needed = true;
    }

    /// Moves Dock keyboard selection by `delta` (`-1` / `+1`, or `isize::MIN`/`isize::MAX` for Home/End).
    pub fn step_dock_focus(&mut self, delta: isize) {
        let tiles = self.build_dock_tiles();
        let count = tiles.len();
        if count == 0 {
            self.dock_focused_slot = None;
            return;
        }
        let next_idx = if delta == isize::MIN {
            0
        } else if delta == isize::MAX {
            count - 1
        } else {
            let cur = self
                .dock_focused_slot
                .unwrap_or(if delta > 0 { count - 1 } else { 0 });
            let wrapped = (isize::try_from(cur).unwrap_or(0) + delta)
                .rem_euclid(isize::try_from(count).unwrap_or(1));
            usize::try_from(wrapped).unwrap_or(0)
        };
        self.dock_focused_slot = Some(next_idx);
        self.shell_redraw_needed = true;
    }

    /// Dismisses Dock keyboard focus and allows auto-hide to re-engage if appropriate.
    pub fn clear_dock_focus(&mut self) {
        if self.dock_focused_slot.take().is_some() {
            self.sync_dock_visibility_for_all_outputs(true);
            self.shell_redraw_needed = true;
        }
    }

    /// Activates the currently keyboard-focused Dock slot.
    pub fn activate_focused_dock_slot(&mut self, serial: Serial) {
        let Some(slot_idx) = self.dock_focused_slot else {
            return;
        };
        let tiles = self.build_dock_tiles();
        let Some(tile) = tiles.get(slot_idx) else {
            self.clear_dock_focus();
            return;
        };
        let hit = tile.hit;
        self.clear_dock_focus();
        self.handle_dock_hit(hit, serial);
    }

    /// Opens the context menu for the currently keyboard-focused Dock slot.
    pub fn open_focused_dock_context_menu(&mut self) {
        let Some(slot_idx) = self.dock_focused_slot else {
            return;
        };
        let output_id = self
            .active_dock_output
            .unwrap_or_else(|| self.output_map.primary().id);
        let logical_size = self.output_logical_size(output_id);
        let tiles = self.build_dock_tiles();
        let layout = self.dock_layout_for_output(output_id, &tiles);
        let Some(slot) = layout.slots.iter().find(|s| s.slot_idx == slot_idx) else {
            return;
        };
        let geo = self
            .output_map
            .get(output_id)
            .unwrap_or_else(|| self.output_map.primary())
            .logical_geometry();
        let anchor_local_x = f64::from(slot.icon_rect.loc.x + slot.icon_rect.size.w / 2);
        let anchor_local_y = f64::from(slot.icon_rect.loc.y + slot.icon_rect.size.h / 2);
        let anchor = smithay::utils::Point::from((
            f64::from(geo.loc.x) + anchor_local_x,
            f64::from(geo.loc.y) + anchor_local_y,
        ));
        let alt_held = self.keyboard.modifier_state().alt;
        self.open_dock_context_menu_on_output(
            output_id,
            slot.tile.hit,
            logical_size,
            anchor,
            alt_held,
        );
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
        let effective_vis = self.user_config.effective_dock_visibility();
        self.user_config.dock_visibility = effective_vis;
        self.user_config.dock_auto_hide =
            matches!(effective_vis, crate::config::DockVisibilityMode::AlwaysHide);
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
        let mut cfg = cfg;
        let next_dock_vis = cfg.effective_dock_visibility();
        cfg.dock_visibility = next_dock_vis;
        cfg.dock_auto_hide = matches!(next_dock_vis, crate::config::DockVisibilityMode::AlwaysHide);
        let dock_layout_changed = cfg.dock_position != self.user_config.dock_position
            || cfg.dock_icon_size != self.user_config.dock_icon_size
            || next_dock_vis != self.user_config.effective_dock_visibility()
            || cfg.dock_auto_hide != self.user_config.dock_auto_hide;
        let dock_pins_changed = cfg.dock_pinned_apps != self.user_config.dock_pinned_apps;
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
            if appearance_changed {
                self.portal.notify_color_scheme_changed(is_dark);
            }
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
            self.reset_all_dock_presentations();
            self.sync_dock_auto_hide_target();
            self.relayout_wm_for_all_outputs();
        }
        if dock_pins_changed {
            self.refresh_dock_pin_metadata_cache();
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
