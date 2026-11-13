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

    pub(super) fn apply_pointer_position(&mut self, pos: Point<f64, Logical>, time: u32) {
        self.pointer_location = pos;

        if let Some(out_id) = self.output_under(pos) {
            self.wm.set_focused_output(out_id);
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

        if let Some(drag) = self.active_window_drag {
            #[allow(clippy::cast_possible_truncation)]
            let dx = (pos.x - drag.start_pointer.x).round() as i32;
            #[allow(clippy::cast_possible_truncation)]
            let dy = (pos.y - drag.start_pointer.y).round() as i32;
            let new_x = drag.start_origin.0 + dx;
            let new_y = (drag.start_origin.1 + dy).max(crate::shell_render::TOP_BAR_HEIGHT);
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
            }
            return;
        }

        if let Some(resize) = self.active_window_resize {
            #[allow(clippy::cast_possible_truncation)]
            let dx = (pos.x - resize.start_pointer.x).round() as i32;
            #[allow(clippy::cast_possible_truncation)]
            let dy = (pos.y - resize.start_pointer.y).round() as i32;
            let target_rect = compute_resized_rect(
                resize.start_rect,
                resize.edges,
                dx,
                dy,
                crate::shell_render::TOP_BAR_HEIGHT,
            );
            if self.wm.resize_window(resize.window_id, target_rect)
                && let Some(win) = self.wm.window(resize.window_id)
            {
                let rect = win.geometry;
                self.animations.snap_window_immediate(
                    resize.window_id.0,
                    rusty_animation::LogicalRect::new(rect.x, rect.y, rect.width, rect.height),
                );
                let serial = SERIAL_COUNTER.next_serial();
                self.sync_space_with_wm(serial);
            }
            return;
        }

        let under = self
            .target_surface_under(pos)
            .map(|(_win, surface, origin)| (surface, origin));
        self.dispatch_pointer_motion_to_surface(pos, under, time);
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

        if button_state == ButtonState::Released {
            self.active_window_drag = None;
            self.active_window_resize = None;
            self.qs_slider_drag = None;
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

            // Handle Alt+Tab window switcher HUD clicks (`RUSTYOS-033`)
            if let Some(ref sw) = self.window_switcher {
                let output_phys = self
                    .space
                    .output_geometry(&self.output)
                    .map_or_else(|| Size::from((1280, 800)), |geo| geo.size)
                    .to_physical(1);
                if let Some(idx) = crate::shell_render::check_switcher_click(
                    output_phys,
                    sw.window_ids.len(),
                    self.pointer_location.x,
                    self.pointer_location.y,
                ) {
                    self.switcher_commit_index(idx, serial);
                } else {
                    self.switcher_cancel();
                }
                return;
            }

            // Handle Quick Settings popover clicks exclusively while open (RUSTYOS-021)
            if self.quick_settings_active {
                self.handle_quick_settings_pointer_click(serial, button);
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
                self.dismiss_notification(toast_id);
                self.hovered_notification_idx = None;
                return;
            }

            // Handle bottom Dock clicks (`RUSTYOS-023`) before Overview/windows when visible
            let ordered_ids = self.ordered_visible_window_ids();
            if shell_chrome_visible
                && let Some(dock_hit) = crate::shell_render::check_dock_click(
                    output_phys,
                    ordered_ids.len(),
                    self.pointer_location.x,
                    self.pointer_location.y,
                )
            {
                self.handle_dock_hit(dock_hit, serial);
                return;
            }

            // Handle Workspace Overview card & Spaces strip clicks or backdrop dismissal (`RUSTYOS-023` / `RUSTYOS-051`)
            if self.overview_active {
                self.handle_overview_pointer_click(output_phys, ordered_ids.len(), serial);
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
                    match hit {
                        crate::shell_render::SsdHit::Close(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.apply_wm_action(WmAction::CloseFocused, serial);
                        }
                        crate::shell_render::SsdHit::Minimize(win_id) => {
                            self.minimize_window(win_id, serial);
                        }
                        crate::shell_render::SsdHit::Maximize(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
                        }
                        crate::shell_render::SsdHit::TitlebarDrag(win_id) => {
                            self.wm.focus_and_raise(win_id);
                            self.sync_space_with_wm(serial);
                            if let Some(win) = self.wm.window(win_id) {
                                self.active_window_drag = Some(crate::state::WindowDragState {
                                    window_id: win_id,
                                    start_pointer: self.pointer_location,
                                    start_origin: (win.geometry.x, win.geometry.y),
                                });
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
                            }
                        }
                    }
                    return;
                }

                // Allow Alt+Left or Super+Left anywhere on a window (especially CSD windows) to drag it
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
                    if let Some(win_id) = target_id {
                        self.wm.focus_and_raise(win_id);
                        self.sync_space_with_wm(serial);
                        if let Some(win) = self.wm.window(win_id) {
                            self.active_window_resize = None;
                            self.active_window_drag = Some(crate::state::WindowDragState {
                                window_id: win_id,
                                start_pointer: self.pointer_location,
                                start_origin: (win.geometry.x, win.geometry.y),
                            });
                            return;
                        }
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
                    if w.set_activated(false)
                        && let Some(tl) = w.toplevel()
                    {
                        tl.send_configure();
                    }
                });
                let keyboard = self.keyboard.clone();
                keyboard.set_focus(self, None, serial);
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

pub(super) fn compute_resized_rect(
    start: rusty_wm::Rect,
    edges: crate::shell_render::ResizeEdges,
    dx: i32,
    dy: i32,
    min_top: i32,
) -> rusty_wm::Rect {
    const MIN_W: i32 = 240;
    const MIN_H: i32 = 160;

    let mut x = start.x;
    let mut y = start.y;
    let mut w = start.width;
    let mut h = start.height;

    if edges.right {
        w = (start.width + dx).max(MIN_W);
    } else if edges.left {
        let right_edge = start.x + start.width;
        let max_left = right_edge - MIN_W;
        x = (start.x + dx).min(max_left);
        w = right_edge - x;
    }

    if edges.bottom {
        h = (start.height + dy).max(MIN_H);
    } else if edges.top {
        let bottom_edge = start.y + start.height;
        let max_top = bottom_edge - MIN_H;
        let bounded_min_top = min_top.min(max_top);
        y = (start.y + dy).clamp(bounded_min_top, max_top);
        h = (bottom_edge - y).max(MIN_H);
    }

    rusty_wm::Rect::new(x, y, w, h)
}
