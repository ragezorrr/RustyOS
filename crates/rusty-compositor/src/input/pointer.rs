//! Pointer motion/button/axis routing, resize math, and pointer constraint clamping.

use rusty_wm::WmAction;
use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend,
        PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent},
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size},
    wayland::pointer_constraints::{PointerConstraint, with_pointer_constraint},
};

use crate::state::RustyCompositorState;

impl RustyCompositorState {
    pub(super) fn handle_pointer_motion<B: InputBackend>(&mut self, event: &B::PointerMotionEvent) {
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

    pub(super) fn handle_pointer_motion_absolute<B: InputBackend>(
        &mut self,
        event: &B::PointerMotionAbsoluteEvent,
    ) {
        let pos = if self.output_map.count() <= 1 {
            let primary = self.output_map.primary();
            let unrotated = primary.unrotated_logical_size();
            let raw: Point<f64, Logical> = event.position_transformed(unrotated);
            let local = transform_screen_point_to_output_logical(raw, unrotated, primary.rotation);
            Point::from((
                local.x + f64::from(primary.location.x),
                local.y + f64::from(primary.location.y),
            ))
        } else {
            let bounds = self.total_output_bounds();
            let normalized: Point<f64, Logical> = event.position_transformed(bounds.size);
            let global = Point::from((
                normalized.x + f64::from(bounds.loc.x),
                normalized.y + f64::from(bounds.loc.y),
            ));
            if let Some(entry) = self
                .output_map
                .output_under(global)
                .and_then(|id| self.output_map.get(id))
                && entry.rotation != crate::config::DisplayRotation::Rotate0
            {
                let geo = entry.logical_geometry();
                let unrotated = entry.unrotated_logical_size();
                let local_raw = Point::from((
                    (global.x - f64::from(geo.loc.x))
                        * (f64::from(unrotated.w) / f64::from(geo.size.w.max(1))),
                    (global.y - f64::from(geo.loc.y))
                        * (f64::from(unrotated.h) / f64::from(geo.size.h.max(1))),
                ));
                let local =
                    transform_screen_point_to_output_logical(local_raw, unrotated, entry.rotation);
                Point::from((
                    local.x + f64::from(geo.loc.x),
                    local.y + f64::from(geo.loc.y),
                ))
            } else {
                global
            }
        };
        let time = Event::time_msec(event);
        self.apply_pointer_position(pos, time);
    }

    pub(super) fn dispatch_pointer_motion_to_surface(
        &mut self,
        pos: Point<f64, Logical>,
        under: Option<(
            smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
            Point<f64, Logical>,
        )>,
        time: u32,
    ) {
        let serial = SERIAL_COUNTER.next_serial();
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

    #[allow(clippy::too_many_lines)]
    pub(super) fn apply_pointer_position(&mut self, pos: Point<f64, Logical>, time: u32) {
        self.pointer_location = pos;

        if self.screenshot_hud.is_some() {
            self.set_compositor_cursor_override(Some(
                smithay::input::pointer::CursorIcon::Crosshair,
            ));
            self.handle_screenshot_hud_motion(pos);
            self.dispatch_pointer_motion_to_surface(pos, None, time);
            return;
        }

        if self.ext_session_locked
            || self.screen_locked
            || self.polkit_prompt.is_some()
            || self.portal_prompt.is_some()
        {
            self.clear_shell_hover_states();
            let under = if self.ext_session_locked {
                self.ext_lock_surface_under(pos)
            } else {
                None
            };
            self.set_compositor_cursor_override(
                under
                    .is_none()
                    .then_some(smithay::input::pointer::CursorIcon::Default),
            );
            self.dispatch_pointer_motion_to_surface(pos, under, time);
            return;
        }

        self.update_dock_hover_slot(pos);
        if self.quick_settings_active
            && let Some(drag) = self.qs_slider_drag
        {
            self.apply_quick_settings_slider_drag(drag, pos);
            return;
        }
        self.update_shell_hover_states(pos);

        if self.active_window_drag.is_some() {
            self.handle_active_window_drag_motion(pos);
            return;
        }

        if let Some(resize) = self.active_window_resize {
            self.set_compositor_cursor_override(Some(cursor_icon_for_resize_edges(resize.edges)));
            #[allow(clippy::cast_possible_truncation)]
            let dx = (pos.x - resize.start_pointer.x).round() as i32;
            #[allow(clippy::cast_possible_truncation)]
            let dy = (pos.y - resize.start_pointer.y).round() as i32;
            let (min_w, min_h) = self.effective_window_min_size(resize.window_id);
            let max_size = self.wm.window(resize.window_id).and_then(|w| w.max_size);
            let min_top = self
                .wm
                .output_for_window(resize.window_id)
                .map_or(crate::shell_render::TOP_BAR_HEIGHT, |o| o.work_area.y);
            let target_rect = compute_resized_rect_with_bounds(
                resize.start_rect,
                resize.edges,
                dx,
                dy,
                min_top,
                min_w,
                min_h,
                max_size,
            );
            if self
                .wm
                .resize_window_with_min(resize.window_id, target_rect, min_w, min_h)
                && let Some(win) = self.wm.window(resize.window_id)
            {
                let rect = win.geometry;
                self.animations.snap_window_immediate(
                    resize.window_id.0,
                    rusty_animation::LogicalRect::new(rect.x, rect.y, rect.width, rect.height),
                );
                let serial = SERIAL_COUNTER.next_serial();
                self.sync_space_with_wm(serial);
                self.shell_redraw_needed = true;
            }
            return;
        }

        if self.spotlight_drag.is_some() {
            let mut activated_now = false;
            let mut active = false;
            if let Some(ref mut drag) = self.spotlight_drag {
                drag.current_pointer = pos;
                let dist = (pos.x - drag.start_pointer.x).hypot(pos.y - drag.start_pointer.y);
                if !drag.active_drag && dist > 6.0 {
                    drag.active_drag = true;
                    activated_now = true;
                }
                active = drag.active_drag;
            }
            if activated_now && self.launcher_active {
                self.close_launcher();
            }
            if active {
                let (out_id, is_primary, out_size, lx, ly) = self.output_local_coords(pos);
                let reveal = self.dock_reveal_for_output(out_id);
                let target = if reveal > 0.15 {
                    let tiles = self.build_dock_tiles();
                    let rest_view = crate::shell_render::DockViewState {
                        tiles: &tiles,
                        icon_size: self.user_config.dock_icon_size,
                        position: self.user_config.dock_position,
                        auto_hide_reveal: reveal,
                        show_indicators: self.user_config.dock_show_indicators,
                        top_inset: if is_primary {
                            crate::shell_render::TOP_BAR_HEIGHT
                        } else {
                            0
                        },
                        ..crate::shell_render::DockViewState::default()
                    };
                    let layout = crate::shell_render::compute_dock_layout(out_size, &rest_view);
                    crate::state::resolve_dock_drop_target_at(
                        &layout,
                        self.user_config.dock_position,
                        out_id,
                        lx,
                        ly,
                        true,
                    )
                } else {
                    None
                };
                if let Some(ref mut drag) = self.spotlight_drag {
                    drag.target = target;
                }
                self.shell_redraw_needed = true;
            }
            return;
        }

        if self.desktop_drag.is_some() {
            let (out_id, is_primary, out_size, dock_x, dock_y) = self.output_local_coords(pos);
            let primary_entry = self.output_map.primary();
            let primary_geo = primary_entry.logical_geometry();
            let local_pos = Point::from((
                pos.x - f64::from(primary_geo.loc.x),
                pos.y - f64::from(primary_geo.loc.y),
            ));
            let screen_size = primary_geo.size;
            let dragged_is_desktop = self.desktop_drag.as_ref().is_some_and(|d| {
                crate::state::is_desktop_file_drop(std::slice::from_ref(&d.dragged_path))
            });
            let dock_reveal = self.dock_reveal_for_output(out_id);
            let dock_drop_target = if dock_reveal > 0.15 {
                let tiles = self.build_dock_tiles();
                let rest_view = crate::shell_render::DockViewState {
                    tiles: &tiles,
                    icon_size: self.user_config.dock_icon_size,
                    position: self.user_config.dock_position,
                    auto_hide_reveal: dock_reveal,
                    show_indicators: self.user_config.dock_show_indicators,
                    top_inset: if is_primary {
                        crate::shell_render::TOP_BAR_HEIGHT
                    } else {
                        0
                    },
                    ..crate::shell_render::DockViewState::default()
                };
                let layout = crate::shell_render::compute_dock_layout(out_size, &rest_view);
                crate::state::resolve_dock_drop_target_at(
                    &layout,
                    self.user_config.dock_position,
                    out_id,
                    dock_x,
                    dock_y,
                    dragged_is_desktop,
                )
            } else {
                None
            };
            let drop_to_trash = matches!(
                dock_drop_target,
                Some(crate::state::DockDropTarget::Trash { .. })
            );

            if let Some(ref mut drag) = self.desktop_drag {
                drag.current_pointer = pos;
                let dx = pos.x - drag.start_pointer.x;
                let dy = pos.y - drag.start_pointer.y;
                if (dx * dx + dy * dy) >= 25.0 {
                    drag.is_active = true;
                }
                if drag.is_active {
                    let hit_folder = if dock_drop_target.is_none() {
                        crate::shell_render::hit_test_desktop_item(
                            &self.desktop_items,
                            screen_size,
                            crate::shell_render::DOCK_HEIGHT,
                            local_pos,
                        )
                        .and_then(|idx| self.desktop_items.get(idx))
                        .filter(|it| it.is_dir && it.path != drag.dragged_path)
                        .map(|it| it.path.clone())
                    } else {
                        None
                    };

                    drag.target_hover_dir = hit_folder;
                    drag.drop_to_trash = drop_to_trash;
                    drag.dock_drop_target = dock_drop_target;
                }
            }
            self.shell_redraw_needed = true;
            return;
        }

        if let Some(ref mut rb) = self.desktop_rubber_band {
            rb.current = pos;
            let rb_rect = rb.rect();
            let primary_entry = self.output_map.primary();
            let primary_geo = primary_entry.logical_geometry();
            let screen_size = primary_geo.size;

            self.selected_desktop_items.clear();
            for item in &self.desktop_items {
                let cell_rect = crate::shell_render::compute_desktop_cell_rect(
                    item.slot.0,
                    item.slot.1,
                    screen_size,
                );
                let item_screen_rect = Rectangle::new(
                    Point::from((
                        cell_rect.loc.x + primary_geo.loc.x,
                        cell_rect.loc.y + primary_geo.loc.y,
                    )),
                    cell_rect.size,
                );
                if rb_rect.overlaps(item_screen_rect) {
                    self.selected_desktop_items.insert(item.path.clone());
                }
            }
            self.shell_redraw_needed = true;
            return;
        }

        let over_dock_dnd = self.update_external_dnd_dock_hover(pos);
        let under = if over_dock_dnd {
            None
        } else {
            self.target_surface_under(pos)
                .map(|(_win, surface, origin)| (surface, origin))
        };
        self.update_compositor_cursor_for_pointer(pos, under.is_some());
        self.dispatch_pointer_motion_to_surface(pos, under, time);
    }

    fn handle_active_window_drag_motion(&mut self, pos: Point<f64, Logical>) {
        let Some(mut drag) = self.active_window_drag else {
            return;
        };
        self.set_compositor_cursor_override(Some(smithay::input::pointer::CursorIcon::Grabbing));

        let dest_out = self.output_under(pos);
        let dest_work = dest_out
            .and_then(|out_id| self.wm.output(out_id))
            .map(|o| (o.id, o.work_area));
        let min_y = dest_work.map_or(crate::shell_render::TOP_BAR_HEIGHT, |(_, wa)| wa.y);

        // Hold maximized windows across a 5 px drag threshold so clicks/double-clicks never
        // accidentally unmaximize, then anchor proportionally under the cursor (`RUSTYOS-166`).
        if drag.started_maximized && !drag.unmaximized_during_drag {
            let dist = (pos.x - drag.start_pointer.x).hypot(pos.y - drag.start_pointer.y);
            if dist < 5.0 {
                return;
            }
            let saved_w = self
                .wm
                .window(drag.window_id)
                .map_or(640, |w| w.saved_floating_geometry.width);
            let anchored = compute_unmaximized_drag_origin(
                drag.start_pointer,
                drag.start_origin,
                drag.start_width,
                saved_w,
                pos,
                min_y,
            );
            drag.start_origin = anchored;
            drag.start_pointer = pos;
            drag.unmaximized_during_drag = true;
        }

        if let Some(out_id) = dest_out {
            let _ = self.wm.migrate_window_to_output(drag.window_id, out_id);
        }

        #[allow(clippy::cast_possible_truncation)]
        let dx = (pos.x - drag.start_pointer.x).round() as i32;
        #[allow(clippy::cast_possible_truncation)]
        let dy = (pos.y - drag.start_pointer.y).round() as i32;
        let total = self.total_output_bounds();
        let win_w = self
            .wm
            .window(drag.window_id)
            .map_or(320, |w| w.geometry.width);
        let min_x = total.loc.x - win_w + 64;
        let max_x = total.loc.x + total.size.w - 64;
        let max_y = (total.loc.y + total.size.h - crate::shell_render::TITLEBAR_HEIGHT).max(min_y);
        let new_x = (drag.start_origin.0 + dx).clamp(min_x, max_x.max(min_x));
        let new_y = (drag.start_origin.1 + dy).clamp(min_y, max_y);

        drag.snap_preview = dest_work.and_then(|(out_id, work_area)| {
            detect_edge_snap_target(work_area, pos).map(|target| {
                let preview = rusty_wm::compute_snap_rect(work_area, self.wm.gap_px, target);
                (out_id, target, preview)
            })
        });
        self.active_window_drag = Some(drag);

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
            self.shell_redraw_needed = true;
        }
    }

    pub(crate) fn update_compositor_cursor_for_pointer(
        &mut self,
        pos: Point<f64, Logical>,
        has_client_surface: bool,
    ) {
        use smithay::input::pointer::CursorIcon;

        if self.overview_drag.as_ref().is_some_and(|d| d.dragging) {
            self.set_compositor_cursor_override(Some(CursorIcon::Grabbing));
            return;
        }
        if self.has_blocking_shell_overlay()
            || self.context_menu.is_some()
            || self.window_switcher.is_some()
            || self.active_top_bar_menu.is_some()
            || self.dock_hover_slot.is_some()
            || self.dock_stack.is_some()
            || self.dock_drag.is_some()
        {
            self.set_compositor_cursor_override(Some(CursorIcon::Default));
            return;
        }

        let (_, is_on_primary, _, primary_local_y) = self.primary_local_coords(pos);
        if is_on_primary
            && primary_local_y < f64::from(crate::shell_render::TOP_BAR_HEIGHT)
            && !self.should_hide_shell_chrome_for_fullscreen()
        {
            self.set_compositor_cursor_override(Some(CursorIcon::Default));
            return;
        }

        let hit_windows = self.visible_ssd_hit_windows();
        if let Some(hit) = crate::shell_render::check_window_frame_hit(&hit_windows, pos.x, pos.y) {
            let icon = match hit {
                crate::shell_render::SsdHit::Resize(_, edges) => {
                    cursor_icon_for_resize_edges(edges)
                }
                crate::shell_render::SsdHit::Close(_)
                | crate::shell_render::SsdHit::Minimize(_)
                | crate::shell_render::SsdHit::Maximize(_)
                | crate::shell_render::SsdHit::TitlebarDrag(_) => CursorIcon::Default,
            };
            self.set_compositor_cursor_override(Some(icon));
            return;
        }

        if has_client_surface {
            self.set_compositor_cursor_override(None);
        } else {
            self.set_compositor_cursor_override(Some(CursorIcon::Default));
        }
    }

    pub(super) fn primary_local_coords(
        &self,
        pos: Point<f64, Logical>,
    ) -> (Size<i32, smithay::utils::Physical>, bool, f64, f64) {
        let primary_entry = self.output_map.primary();
        let primary_geo = primary_entry.logical_geometry();
        #[allow(clippy::cast_possible_truncation)]
        let px = pos.x.round() as i32;
        #[allow(clippy::cast_possible_truncation)]
        let py = pos.y.round() as i32;
        let is_on_primary = primary_geo.contains((px, py));
        let local_x = pos.x - f64::from(primary_geo.loc.x);
        let local_y = pos.y - f64::from(primary_geo.loc.y);
        (primary_entry.physical_size, is_on_primary, local_x, local_y)
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn handle_pointer_button<B: InputBackend>(&mut self, event: &B::PointerButtonEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let button = event.button_code();
        let button_state = event.state();

        if button_state == ButtonState::Pressed {
            self.pressed_pointer_buttons.insert(button);
            self.last_pointer_press_serial = Some(serial);
            if self.pending_external_dnd_drop.is_some() {
                self.cancel_pending_external_dnd_drop();
            }
        } else {
            self.pressed_pointer_buttons.remove(&button);
        }

        if button_state == ButtonState::Released {
            if button == 272
                && self.begin_external_dnd_dock_drop(button, serial, Event::time_msec(event))
            {
                return;
            }
            if button == 272
                && let Some(spot) = self.spotlight_drag.take()
            {
                if spot.active_drag {
                    if let Some(crate::state::DockDropTarget::PinInsert { insert_idx, .. }) =
                        spot.target
                    {
                        self.pin_app_at_index(&spot.app_key, insert_idx);
                    }
                    self.sync_dock_auto_hide_target();
                    self.shell_redraw_needed = true;
                } else {
                    self.launch_spotlight_row(spot.row_idx);
                }
                return;
            }
            if self.screenshot_hud.is_some() {
                self.handle_screenshot_hud_button(false);
                return;
            }
            if button == 272
                && let Some(drag) = self.overview_drag.take()
            {
                if drag.dragging {
                    if let Some(ws_num) = drag.target_workspace {
                        let target_ws = rusty_wm::WorkspaceId::new_clamped(ws_num);
                        if self.wm.move_window_to_workspace(drag.window_id, target_ws) {
                            self.sync_space_with_wm(serial);
                            tracing::info!(
                                window_id = drag.window_id.0,
                                workspace = target_ws.0,
                                "Moved window via Overview drag-to-workspace"
                            );
                        }
                    }
                } else {
                    self.restore_window(drag.window_id, serial);
                    self.close_overview();
                }
                self.update_compositor_cursor_for_pointer(self.pointer_location, false);
                self.shell_redraw_needed = true;
                return;
            }
            if button == 272
                && let Some((pressed_win, pressed_kind)) = self.pressed_ssd_button.take()
            {
                let hit_windows = self.visible_ssd_hit_windows();
                let still_on_same = match crate::shell_render::check_window_frame_hit(
                    &hit_windows,
                    self.pointer_location.x,
                    self.pointer_location.y,
                ) {
                    Some(crate::shell_render::SsdHit::Close(id)) => {
                        id == pressed_win && pressed_kind == rusty_ui::TrafficLightKind::Close
                    }
                    Some(crate::shell_render::SsdHit::Minimize(id)) => {
                        id == pressed_win && pressed_kind == rusty_ui::TrafficLightKind::Minimize
                    }
                    Some(crate::shell_render::SsdHit::Maximize(id)) => {
                        id == pressed_win && pressed_kind == rusty_ui::TrafficLightKind::Maximize
                    }
                    _ => false,
                };
                if still_on_same {
                    match pressed_kind {
                        rusty_ui::TrafficLightKind::Close => {
                            self.wm.focus_and_raise(pressed_win);
                            self.apply_wm_action(WmAction::CloseFocused, serial);
                        }
                        rusty_ui::TrafficLightKind::Minimize => {
                            self.minimize_window(pressed_win, serial);
                        }
                        rusty_ui::TrafficLightKind::Maximize => {
                            self.wm.focus_and_raise(pressed_win);
                            self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
                        }
                    }
                }
                self.shell_redraw_needed = true;
                return;
            }
            let ended_drag = if button == 272 || self.pressed_pointer_buttons.is_empty() {
                self.active_window_drag.take()
            } else {
                None
            };
            let ended_resize = if button == 272 || self.pressed_pointer_buttons.is_empty() {
                self.active_window_resize.take().is_some()
            } else {
                false
            };
            self.qs_slider_drag = None;
            if let Some(drag) = ended_drag {
                if let Some((out_id, target, _)) = drag.snap_preview {
                    let _ = self.wm.migrate_window_to_output(drag.window_id, out_id);
                    let _ = self.wm.snap_window(drag.window_id, target);
                }
                self.sync_space_with_wm(serial);
                let has_surface = self.target_surface_under(self.pointer_location).is_some();
                self.update_compositor_cursor_for_pointer(self.pointer_location, has_surface);
                self.shell_redraw_needed = true;
            } else if ended_resize {
                self.sync_space_with_wm(serial);
                let has_surface = self.target_surface_under(self.pointer_location).is_some();
                self.update_compositor_cursor_for_pointer(self.pointer_location, has_surface);
                self.shell_redraw_needed = true;
            }
            if button == 272
                && let Some(drag) = self.dock_drag.take()
            {
                if drag.active_drag {
                    if let Some(src_idx) = drag.source_pinned_idx {
                        self.commit_dock_pin_reorder_or_remove(
                            src_idx,
                            drag.target_pinned_idx,
                            drag.pulled_outside,
                        );
                    } else if let Some(win_id) = drag.source_minimized_win {
                        self.commit_dock_minimized_reorder(win_id, drag.target_minimized_idx);
                    }
                } else {
                    self.handle_dock_hit(drag.hit, serial);
                }
                self.sync_dock_auto_hide_target();
                self.shell_redraw_needed = true;
                return;
            }

            if button == 272
                && let Some(drag) = self.desktop_drag.take()
                && drag.is_active
            {
                if let Some(target) = drag.dock_drop_target {
                    self.execute_dock_drop(&target, vec![drag.dragged_path], true);
                    self.sync_dock_auto_hide_target();
                    self.shell_redraw_needed = true;
                    return;
                }
                if drag.drop_to_trash {
                    self.spawn_background_trash_drop(vec![drag.dragged_path], true);
                    self.shell_redraw_needed = true;
                    return;
                }
                if let Some(dest_dir) = drag.target_hover_dir {
                    if let Some(file_name) = drag.dragged_path.file_name() {
                        let candidate = dest_dir.join(file_name);
                        let dest_path = if candidate.exists() {
                            rusty_services::file_manager::pick_unique_copy_path(&candidate)
                        } else {
                            candidate
                        };
                        if dest_path == drag.dragged_path || dest_path.exists() {
                            tracing::warn!(
                                ?dest_path,
                                "Skipped desktop drop into folder to prevent silent overwrite"
                            );
                        } else if let Err(err) = std::fs::rename(&drag.dragged_path, &dest_path) {
                            tracing::warn!(?err, "Failed to move desktop item into folder");
                        } else {
                            self.resync_desktop_items();
                            self.save_desktop_grid_layout();
                        }
                    }
                } else {
                    let primary_entry = self.output_map.primary();
                    let primary_geo = primary_entry.logical_geometry();
                    let local_pos = Point::from((
                        drag.current_pointer.x - f64::from(primary_geo.loc.x),
                        drag.current_pointer.y - f64::from(primary_geo.loc.y),
                    ));
                    let screen_size = primary_geo.size;
                    let target_slot = crate::shell_render::cell_at_point(
                        local_pos,
                        screen_size,
                        crate::shell_render::DOCK_HEIGHT,
                    );
                    if let Some(item_idx) = self
                        .desktop_items
                        .iter()
                        .position(|it| it.path == drag.dragged_path)
                    {
                        let old_slot = self.desktop_items[item_idx].slot;
                        if let Some(other_idx) = self
                            .desktop_items
                            .iter()
                            .position(|it| it.slot == target_slot)
                        {
                            self.desktop_items[other_idx].slot = old_slot;
                        }
                        self.desktop_items[item_idx].slot = target_slot;
                        self.save_desktop_grid_layout();
                    }
                }
                self.shell_redraw_needed = true;
                return;
            }

            if button == 272 && self.desktop_rubber_band.take().is_some() {
                self.shell_redraw_needed = true;
                return;
            }
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
                self.handle_lock_screen_pointer_click(output_phys, serial);
                return;
            }

            if self.polkit_prompt.is_some() {
                let output_phys = self
                    .space
                    .output_geometry(&self.output)
                    .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
                    .to_physical(1);
                self.handle_polkit_pointer_click(output_phys, serial);
                return;
            }

            if self.portal_prompt.is_some() {
                let output_phys = self
                    .space
                    .output_geometry(&self.output)
                    .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
                    .to_physical(1);
                self.handle_portal_pointer_click(output_phys, serial);
                return;
            }

            if self.portal_pick_color.is_some() {
                self.trigger_pick_color_sample(self.pointer_location);
                return;
            }

            if self.screenshot_hud.is_some() {
                self.handle_screenshot_hud_button(true);
                return;
            }

            if self.context_menu.is_some()
                && self.handle_context_menu_pointer_click(button, self.pointer_location, serial)
                    == Some(true)
            {
                return;
            }

            let output_width = self
                .space
                .output_geometry(&self.output)
                .map_or(1280, |geo| geo.size.w);

            let shell_chrome_visible = !self.has_foreground_layer_surfaces()
                && !self.should_hide_shell_chrome_for_fullscreen();

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

            // Handle Alt+Tab window switcher HUD clicks (`RUSTYOS-033`, `RUSTYOS-167`)
            if let Some(ref sw) = self.window_switcher {
                let output_phys = self
                    .space
                    .output_geometry(&self.output)
                    .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
                    .to_physical(1);
                let hit = crate::shell_render::hit_test_switcher(
                    output_phys,
                    sw.window_ids.len(),
                    self.pointer_location.x,
                    self.pointer_location.y,
                );
                match hit {
                    crate::shell_render::SwitcherHit::CloseCard(idx) => {
                        self.switcher_close_index(idx);
                    }
                    crate::shell_render::SwitcherHit::Card(idx) if button == 274 => {
                        self.switcher_close_index(idx);
                    }
                    crate::shell_render::SwitcherHit::Card(idx) => {
                        self.switcher_commit_index(idx, serial);
                    }
                    crate::shell_render::SwitcherHit::Outside => {
                        self.switcher_cancel();
                    }
                }
                return;
            }

            // Handle Quick Settings popover clicks exclusively while open (RUSTYOS-021)
            if self.quick_settings_active {
                self.handle_quick_settings_pointer_click(serial, button);
                return;
            }
            if self.updates_popover_open {
                self.handle_updates_popover_pointer_click(button);
                return;
            }
            if self.wifi_popover_open {
                self.handle_wifi_popover_pointer_click(button);
                return;
            }
            if self.top_bar_quick_popover.is_some() {
                self.handle_top_bar_quick_popover_pointer_click(button);
                return;
            }
            if self.notification_center_open {
                self.handle_notification_center_pointer_click(button);
                return;
            }

            if self.dock_stack.is_some() && self.handle_dock_stack_pointer_press(button) {
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
                let rects = crate::shell_render::notification_toast_rects(
                    output_phys,
                    visible_toast_ids.len(),
                );
                let hit_close = rects.get(toast_idx).is_some_and(|card| {
                    let cx = f64::from(card.loc.x + card.size.w - 28);
                    let cy = f64::from(card.loc.y + 10);
                    self.pointer_location.x >= cx
                        && self.pointer_location.x < cx + 20.0
                        && self.pointer_location.y >= cy
                        && self.pointer_location.y < cy + 20.0
                });
                if hit_close {
                    self.dismiss_notification(toast_id);
                } else {
                    self.activate_notification(toast_id);
                }
                self.hovered_notification_idx = None;
                self.shell_redraw_needed = true;
                return;
            }

            // Handle Dock clicks & internal drag-reorder (`RUSTYOS-023` / `RUSTYOS-064` / `RUSTYOS-073` / `RUSTYOS-161`) across all outputs
            let (cursor_output_id, _, _, dock_local_x, dock_local_y) =
                self.output_local_coords(self.pointer_location);
            let dock_chrome_visible_here = !self
                .has_foreground_layer_surfaces_on_id(cursor_output_id)
                && !self.should_hide_shell_chrome_for_fullscreen_on(cursor_output_id);
            let (_, is_on_primary, primary_local_x, primary_local_y) =
                self.primary_local_coords(self.pointer_location);
            let ordered_ids = self.ordered_visible_window_ids();
            if dock_chrome_visible_here
                && self.handle_dock_pointer_press(
                    cursor_output_id,
                    dock_local_x,
                    dock_local_y,
                    button,
                )
            {
                return;
            }

            // Handle Workspace Overview card & Spaces strip clicks or backdrop dismissal (`RUSTYOS-023` / `RUSTYOS-051` / `RUSTYOS-167`)
            if self.overview_active {
                self.handle_overview_pointer_click(output_phys, ordered_ids.len(), serial, button);
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
                let hit_windows = self.visible_ssd_hit_windows();
                if let Some(hit) = crate::shell_render::check_window_frame_hit(
                    &hit_windows,
                    self.pointer_location.x,
                    self.pointer_location.y,
                ) {
                    if button == 273 {
                        let win_id = match hit {
                            crate::shell_render::SsdHit::TitlebarDrag(id)
                            | crate::shell_render::SsdHit::Resize(id, _)
                            | crate::shell_render::SsdHit::Close(id)
                            | crate::shell_render::SsdHit::Minimize(id)
                            | crate::shell_render::SsdHit::Maximize(id) => id,
                        };
                        self.wm.focus_and_raise(win_id);
                        self.open_window_context_menu(self.pointer_location, win_id);
                        return;
                    }
                    if button != 272 {
                        return;
                    }
                    match hit {
                        crate::shell_render::SsdHit::Close(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.sync_space_with_wm(serial);
                            self.pressed_ssd_button =
                                Some((win_id, rusty_ui::TrafficLightKind::Close));
                            self.shell_redraw_needed = true;
                        }
                        crate::shell_render::SsdHit::Minimize(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.sync_space_with_wm(serial);
                            self.pressed_ssd_button =
                                Some((win_id, rusty_ui::TrafficLightKind::Minimize));
                            self.shell_redraw_needed = true;
                        }
                        crate::shell_render::SsdHit::Maximize(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.sync_space_with_wm(serial);
                            self.pressed_ssd_button =
                                Some((win_id, rusty_ui::TrafficLightKind::Maximize));
                            self.shell_redraw_needed = true;
                        }
                        crate::shell_render::SsdHit::TitlebarDrag(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.sync_space_with_wm(serial);
                            let now = std::time::Instant::now();
                            let is_double_click = self.last_titlebar_click.is_some_and(
                                |(prev_id, prev_pos, prev_at)| {
                                    prev_id == win_id
                                        && now.duration_since(prev_at)
                                            <= std::time::Duration::from_millis(400)
                                        && (self.pointer_location.x - prev_pos.x)
                                            .hypot(self.pointer_location.y - prev_pos.y)
                                            <= 6.0
                                },
                            );
                            if is_double_click {
                                self.last_titlebar_click = None;
                                self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
                                return;
                            }
                            self.last_titlebar_click = Some((win_id, self.pointer_location, now));
                            if let Some(win) = self.wm.window(win_id) {
                                self.active_window_drag = Some(crate::state::WindowDragState::new(
                                    win_id,
                                    self.pointer_location,
                                    win,
                                ));
                                self.set_compositor_cursor_override(Some(
                                    smithay::input::pointer::CursorIcon::Grabbing,
                                ));
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
                                self.set_compositor_cursor_override(Some(
                                    cursor_icon_for_resize_edges(edges),
                                ));
                                self.sync_space_with_wm(serial);
                            }
                        }
                    }
                    return;
                }

                // Allow Alt+Left or Super+Left anywhere on a non-fullscreen window to drag it
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
                    if let Some(win_id) = target_id
                        && self.wm.window(win_id).is_some_and(|w| !w.fullscreen)
                    {
                        self.wm.focus_and_raise(win_id);
                        self.sync_space_with_wm(serial);
                        if let Some(win) = self.wm.window(win_id) {
                            self.active_window_resize = None;
                            self.active_window_drag = Some(crate::state::WindowDragState::new(
                                win_id,
                                self.pointer_location,
                                win,
                            ));
                            self.set_compositor_cursor_override(Some(
                                smithay::input::pointer::CursorIcon::Grabbing,
                            ));
                            return;
                        }
                    }
                }

                // Allow Alt+Right or Super+Right anywhere on a window (CSD/SSD) to open the system window menu
                if button == 273 && (mods.alt || mods.logo) {
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
                        self.open_window_context_menu(self.pointer_location, win_id);
                        return;
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
                    if let Some(x11) = w.x11_surface() {
                        let _ = x11.set_activated(false);
                    }
                    if w.set_activated(false)
                        && let Some(tl) = w.toplevel()
                    {
                        tl.send_configure();
                    }
                });
                let keyboard = self.keyboard.clone();
                keyboard.set_focus(self, None, serial);
                self.desktop_focused = true;
                self.shell_redraw_needed = true;

                // Desktop canvas icon interaction (RUSTYOS-081)
                let hit_item = if is_on_primary && !self.has_foreground_layer_surfaces() {
                    let screen_size = self.output_map.primary().logical_geometry().size;
                    let local_pos = Point::from((primary_local_x, primary_local_y));
                    crate::shell_render::hit_test_desktop_item(
                        &self.desktop_items,
                        screen_size,
                        crate::shell_render::DOCK_HEIGHT,
                        local_pos,
                    )
                    .and_then(|idx| self.desktop_items.get(idx).cloned())
                } else {
                    None
                };

                if self.desktop_inline_rename.is_some() {
                    self.commit_desktop_inline_rename();
                }

                if let Some(item) = hit_item {
                    if button == 272 {
                        let now = std::time::Instant::now();
                        let is_double_click =
                            if let Some((ref last_path, last_time)) = self.last_desktop_click {
                                last_path == &item.path
                                    && now.duration_since(last_time).as_millis() <= 450
                            } else {
                                false
                            };
                        if is_double_click {
                            self.last_desktop_click = None;
                            self.desktop_drag = None;
                            self.open_desktop_canvas_item(&item);
                        } else {
                            let was_already_selected = self.selected_desktop_items.len() == 1
                                && self.selected_desktop_items.contains(&item.path);
                            let slow_click_rename = was_already_selected
                                && self.last_desktop_click.as_ref().is_some_and(|(p, t)| {
                                    p == &item.path
                                        && (500..2500).contains(&now.duration_since(*t).as_millis())
                                });

                            if slow_click_rename
                                && let Some(name) = item.path.file_name().and_then(|f| f.to_str())
                            {
                                let len = name.chars().count();
                                self.desktop_inline_rename =
                                    Some(crate::shell_render::DesktopInlineRename {
                                        target_path: item.path.clone(),
                                        buffer: name.to_owned(),
                                        cursor_pos: len,
                                    });
                                self.last_desktop_click = None;
                                self.shell_redraw_needed = true;
                                return;
                            }

                            if !self.selected_desktop_items.contains(&item.path) {
                                self.selected_desktop_items.clear();
                                self.selected_desktop_items.insert(item.path.clone());
                            }
                            self.desktop_drag = Some(crate::shell_render::DesktopDragState {
                                start_pointer: self.pointer_location,
                                current_pointer: self.pointer_location,
                                dragged_path: item.path.clone(),
                                is_active: false,
                                target_hover_dir: None,
                                drop_to_trash: false,
                                dock_drop_target: None,
                            });
                            self.last_desktop_click = Some((item.path, now));
                            self.shell_redraw_needed = true;
                        }
                        return;
                    } else if button == 273 {
                        self.selected_desktop_items.clear();
                        self.selected_desktop_items.insert(item.path.clone());
                        self.open_desktop_item_context_menu(self.pointer_location, &item);
                        return;
                    }
                } else {
                    if !self.selected_desktop_items.is_empty() {
                        self.selected_desktop_items.clear();
                        self.shell_redraw_needed = true;
                    }
                    if button == 272 && is_on_primary && !self.has_foreground_layer_surfaces() {
                        self.desktop_rubber_band = Some(crate::shell_render::DesktopRubberBand {
                            start: self.pointer_location,
                            current: self.pointer_location,
                        });
                        self.shell_redraw_needed = true;
                        return;
                    }
                    if button == 273
                        && !self.has_foreground_layer_surfaces()
                        && (!is_on_primary
                            || primary_local_y >= f64::from(crate::shell_render::TOP_BAR_HEIGHT))
                    {
                        let hide_chrome = self
                            .output_map
                            .output_under(self.pointer_location)
                            .is_some_and(|oid| {
                                self.should_hide_shell_chrome_for_fullscreen_on(oid)
                            });
                        if !hide_chrome {
                            self.open_desktop_context_menu(self.pointer_location);
                            return;
                        }
                    }
                }
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

    pub(super) fn handle_pointer_axis<B: InputBackend>(&mut self, event: &B::PointerAxisEvent) {
        if self.screen_locked
            || self.polkit_prompt.is_some()
            || self.portal_prompt.is_some()
            || self.has_blocking_shell_overlay()
            || self.window_switcher.is_some()
            || self.screenshot_hud.is_some()
            || self.context_menu.is_some()
            || self.active_top_bar_menu.is_some()
        {
            return;
        }
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
}

/// Transforms an unrotated screen-space pointer point inside `unrotated_size` into output-local
/// logical coordinates under `rotation` (`RUSTYOS-055`).
#[must_use]
pub(crate) fn transform_screen_point_to_output_logical(
    point: Point<f64, Logical>,
    unrotated_size: Size<i32, Logical>,
    rotation: crate::config::DisplayRotation,
) -> Point<f64, Logical> {
    let w = f64::from(unrotated_size.w.max(1));
    let h = f64::from(unrotated_size.h.max(1));
    let x = point.x.clamp(0.0, w);
    let y = point.y.clamp(0.0, h);
    let mapped = match rotation {
        crate::config::DisplayRotation::Rotate0 => (x, y),
        crate::config::DisplayRotation::Rotate90 => ((h - y).clamp(0.0, h), x.clamp(0.0, w)),
        crate::config::DisplayRotation::Rotate180 => (w - x, h - y),
        crate::config::DisplayRotation::Rotate270 => (y.clamp(0.0, h), (w - x).clamp(0.0, w)),
    };
    Point::from(mapped)
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

#[cfg(test)]
pub(super) fn compute_resized_rect(
    start: rusty_wm::Rect,
    edges: crate::shell_render::ResizeEdges,
    dx: i32,
    dy: i32,
    min_top: i32,
) -> rusty_wm::Rect {
    compute_resized_rect_with_min(start, edges, dx, dy, min_top, 240, 160)
}

#[cfg(test)]
pub(super) fn compute_resized_rect_with_min(
    start: rusty_wm::Rect,
    edges: crate::shell_render::ResizeEdges,
    dx: i32,
    dy: i32,
    min_top: i32,
    min_w: i32,
    min_h: i32,
) -> rusty_wm::Rect {
    compute_resized_rect_with_bounds(start, edges, dx, dy, min_top, min_w, min_h, None)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compute_resized_rect_with_bounds(
    start: rusty_wm::Rect,
    edges: crate::shell_render::ResizeEdges,
    dx: i32,
    dy: i32,
    min_top: i32,
    min_w: i32,
    min_h: i32,
    max_size: Option<(i32, i32)>,
) -> rusty_wm::Rect {
    let min_w = min_w.max(64);
    let min_h = min_h.max(64);
    let max_w = max_size
        .map(|(mw, _)| mw)
        .filter(|&mw| mw > 0)
        .map(|mw| mw.max(min_w));
    let max_h = max_size
        .map(|(_, mh)| mh)
        .filter(|&mh| mh > 0)
        .map(|mh| mh.max(min_h));

    let mut x = start.x;
    let mut y = start.y;
    let mut w = start.width;
    let mut h = start.height;

    if edges.right {
        w = (start.width + dx).max(min_w);
        if let Some(cap) = max_w {
            w = w.min(cap);
        }
    } else if edges.left {
        let right_edge = start.x + start.width;
        let max_left = right_edge - min_w;
        let min_left = max_w.map_or(i32::MIN, |cap| right_edge - cap);
        x = (start.x + dx).clamp(min_left, max_left);
        w = right_edge - x;
    }

    if edges.bottom {
        h = (start.height + dy).max(min_h);
        if let Some(cap) = max_h {
            h = h.min(cap);
        }
    } else if edges.top {
        let bottom_edge = start.y + start.height;
        let max_top = bottom_edge - min_h;
        let min_top_from_max = max_h.map_or(i32::MIN, |cap| bottom_edge - cap);
        let bounded_min_top = min_top.max(min_top_from_max).min(max_top);
        y = (start.y + dy).clamp(bounded_min_top, max_top);
        h = (bottom_edge - y).max(min_h);
    }

    rusty_wm::Rect::new(x, y, w, h)
}

/// Maps window [`ResizeEdges`] to the matching standard Wayland [`CursorIcon`] (`RUSTYOS-166`).
#[must_use]
pub(crate) const fn cursor_icon_for_resize_edges(
    edges: crate::shell_render::ResizeEdges,
) -> smithay::input::pointer::CursorIcon {
    use smithay::input::pointer::CursorIcon;

    match (edges.left, edges.right, edges.top, edges.bottom) {
        (true, false, true, false) => CursorIcon::NwResize,
        (false, true, true, false) => CursorIcon::NeResize,
        (true, false, false, true) => CursorIcon::SwResize,
        (false, true, false, true) => CursorIcon::SeResize,
        (true, false, false, false) => CursorIcon::WResize,
        (false, true, false, false) => CursorIcon::EResize,
        (false, false, true, false) => CursorIcon::NResize,
        (false, false, false, true) => CursorIcon::SResize,
        _ => CursorIcon::Default,
    }
}

/// Detects an Edge/Corner Snap target when dragging a window near the bounds of `work_area` (`RUSTYOS-166`).
#[must_use]
pub(crate) fn detect_edge_snap_target(
    work_area: rusty_wm::Rect,
    pos: Point<f64, Logical>,
) -> Option<rusty_wm::SnapTarget> {
    const EDGE_BAND: f64 = 12.0;
    const CORNER_BAND: f64 = 48.0;

    let x0 = f64::from(work_area.x);
    let y0 = f64::from(work_area.y);
    let x1 = f64::from(work_area.x + work_area.width);
    let y1 = f64::from(work_area.y + work_area.height);

    let near_left = pos.x <= x0 + EDGE_BAND;
    let near_right = pos.x >= x1 - EDGE_BAND;
    let near_top = pos.y <= y0 + EDGE_BAND;
    let in_top_corner = pos.y <= y0 + CORNER_BAND;
    let in_bottom_corner = pos.y >= y1 - CORNER_BAND;

    if near_left {
        if in_top_corner {
            Some(rusty_wm::SnapTarget::TopLeft)
        } else if in_bottom_corner {
            Some(rusty_wm::SnapTarget::BottomLeft)
        } else {
            Some(rusty_wm::SnapTarget::LeftHalf)
        }
    } else if near_right {
        if in_top_corner {
            Some(rusty_wm::SnapTarget::TopRight)
        } else if in_bottom_corner {
            Some(rusty_wm::SnapTarget::BottomRight)
        } else {
            Some(rusty_wm::SnapTarget::RightHalf)
        }
    } else if near_top {
        Some(rusty_wm::SnapTarget::Maximize)
    } else {
        None
    }
}

/// Computes the unmaximized floating window origin anchored proportionally beneath the pointer (`RUSTYOS-166`).
#[must_use]
pub(crate) fn compute_unmaximized_drag_origin(
    start_pointer: Point<f64, Logical>,
    start_origin: (i32, i32),
    start_width: i32,
    saved_width: i32,
    pos: Point<f64, Logical>,
    min_y: i32,
) -> (i32, i32) {
    let ratio = ((start_pointer.x - f64::from(start_origin.0)) / f64::from(start_width.max(1)))
        .clamp(0.1, 0.9);
    #[allow(clippy::cast_possible_truncation)]
    let anchor_x = (pos.x - ratio * f64::from(saved_width.max(120))).round() as i32;
    #[allow(clippy::cast_possible_truncation)]
    let anchor_y =
        ((pos.y - f64::from(crate::shell_render::TITLEBAR_HEIGHT) * 0.5).round() as i32).max(min_y);
    (anchor_x, anchor_y)
}
