//! Smooth per-output Dock slot, divider, and capsule presentation animations (`RUSTYOS-161`).

use std::collections::HashMap;
use std::time::Duration;

use rusty_animation::{AnimatedValue, Easing};
use smithay::utils::{Physical, Point, Rectangle, Size};

use crate::config::DockPosition;

use super::dock::{ComputedDockLayout, ComputedDockSlot, DockAppTile, DockSection};

/// Duration for slot glide, divider glide, capsule resize, and icon pop-in transitions.
pub const DOCK_PRESENTATION_DURATION: Duration = Duration::from_millis(240);
/// Duration for a departing icon to shrink out smoothly.
pub const DOCK_LEAVE_DURATION: Duration = Duration::from_millis(180);

/// Returns a stable presentation identity key for a Dock tile so pinning/unpinning
/// glides the same icon smoothly across sections.
#[must_use]
pub fn dock_tile_presentation_key(tile: &DockAppTile) -> String {
    match tile.section {
        DockSection::Pinned | DockSection::UnpinnedOrRecent => format!("app:{}", tile.app_key),
        DockSection::MinimizedWindow => {
            format!("min:{}", tile.representative_win.map_or(0, |w| w.0))
        }
        DockSection::Utility => format!("util:{}", tile.app_key),
    }
}

#[derive(Debug, Clone)]
struct PresentedSlot {
    key: String,
    axis_center: AnimatedValue,
    appearance: AnimatedValue,
    leaving: bool,
    last_slot: ComputedDockSlot,
}

/// Per-output Dock presentation animator smoothing slot gliding, icon appearance/departure,
/// section dividers, and capsule length changes (`RUSTYOS-161`).
#[derive(Debug)]
pub struct DockPresentation {
    initialized: bool,
    last_position: DockPosition,
    last_screen_size: Size<i32, Physical>,
    capsule_axis_start: AnimatedValue,
    capsule_axis_len: AnimatedValue,
    dividers: Vec<AnimatedValue>,
    slots: Vec<PresentedSlot>,
}

impl Default for DockPresentation {
    fn default() -> Self {
        Self::new()
    }
}

impl DockPresentation {
    /// Creates an uninitialized presentation animator that snaps cleanly on its first layout sync.
    #[must_use]
    pub fn new() -> Self {
        Self {
            initialized: false,
            last_position: DockPosition::Bottom,
            last_screen_size: Size::from((0, 0)),
            capsule_axis_start: AnimatedValue::new_settled(0.0),
            capsule_axis_len: AnimatedValue::new_settled(0.0),
            dividers: Vec::new(),
            slots: Vec::new(),
        }
    }

    /// Returns `true` while any slot glide, appearance/departure, divider, or capsule resize is in flight.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.initialized
            && (self.capsule_axis_start.is_active()
                || self.capsule_axis_len.is_active()
                || self.dividers.iter().any(AnimatedValue::is_active)
                || self.slots.iter().any(|slot| {
                    slot.leaving || slot.axis_center.is_active() || slot.appearance.is_active()
                }))
    }

    /// Resets presentation so the next layout sync snaps immediately without trailing animation.
    pub fn reset(&mut self) {
        self.initialized = false;
        self.dividers.clear();
        self.slots.clear();
    }

    /// Advances all presentation animations to `now` and prunes fully departed icons.
    /// Returns `true` if any animated value changed or a leaving slot was removed.
    pub fn tick(&mut self, now: Duration) -> bool {
        if !self.initialized {
            return false;
        }
        let tick_val = |anim: &mut AnimatedValue| -> bool {
            let was_active = anim.is_active();
            let prev = anim.current();
            let next = anim.tick(now);
            was_active || (next - prev).abs() > 1e-6
        };
        let mut changed = false;
        changed |= tick_val(&mut self.capsule_axis_start);
        changed |= tick_val(&mut self.capsule_axis_len);
        for div in &mut self.dividers {
            changed |= tick_val(div);
        }
        for slot in &mut self.slots {
            changed |= tick_val(&mut slot.axis_center);
            changed |= tick_val(&mut slot.appearance);
        }
        let before = self.slots.len();
        self.slots.retain(|slot| {
            !slot.leaving || (slot.appearance.is_active() && slot.appearance.current() > 0.02)
        });
        if self.slots.len() != before {
            changed = true;
        }
        changed
    }

    /// Synchronizes animation targets with the newly computed resting `target` layout.
    pub fn sync_target(
        &mut self,
        target: &ComputedDockLayout,
        pos: DockPosition,
        screen_size: Size<i32, Physical>,
        now: Duration,
        reduced_motion: bool,
    ) {
        let (cap_start, cap_len) = rect_axis_bounds(pos, target.capsule_rect);
        if !self.initialized
            || reduced_motion
            || self.last_position != pos
            || self.last_screen_size != screen_size
        {
            self.snap_to_target(target, pos, screen_size);
            return;
        }

        retarget_if_changed(&mut self.capsule_axis_start, cap_start, now);
        retarget_if_changed(&mut self.capsule_axis_len, cap_len, now);
        self.sync_dividers(target, pos, now);
        self.sync_slots(target, pos, now);
    }

    fn sync_dividers(&mut self, target: &ComputedDockLayout, pos: DockPosition, now: Duration) {
        let target_div_positions: Vec<f64> = target
            .dividers
            .iter()
            .map(|r| rect_axis_bounds(pos, *r).0)
            .collect();
        if self.dividers.len() == target_div_positions.len() {
            for (anim, &dst) in self.dividers.iter_mut().zip(&target_div_positions) {
                retarget_if_changed(anim, dst, now);
            }
            return;
        }
        let old = std::mem::take(&mut self.dividers);
        self.dividers = target_div_positions
            .into_iter()
            .enumerate()
            .map(|(idx, dst)| {
                if let Some(mut existing) = old.get(idx).copied() {
                    retarget_if_changed(&mut existing, dst, now);
                    existing
                } else {
                    AnimatedValue::new_settled(dst)
                }
            })
            .collect();
    }

    fn sync_slots(&mut self, target: &ComputedDockLayout, pos: DockPosition, now: Duration) {
        let dragged_key = target
            .dragged_icon
            .as_ref()
            .map(|(tile, _)| dock_tile_presentation_key(tile));

        let mut existing_by_key: HashMap<String, PresentedSlot> = self
            .slots
            .drain(..)
            .map(|slot| (slot.key.clone(), slot))
            .collect();
        let mut next_slots = Vec::with_capacity(target.slots.len() + 2);

        for slot in &target.slots {
            let key = dock_tile_presentation_key(&slot.tile);
            let (start, len) = rect_axis_bounds(pos, slot.hit_rect);
            let target_center = start + len * 0.5;
            if let Some(mut entry) = existing_by_key.remove(&key) {
                retarget_if_changed(&mut entry.axis_center, target_center, now);
                if entry.leaving || (entry.appearance.target() - 1.0).abs() > 1e-4 {
                    entry.leaving = false;
                    entry.appearance.retarget(
                        1.0,
                        now,
                        DOCK_PRESENTATION_DURATION,
                        Easing::EaseOutCubic,
                        false,
                    );
                }
                entry.last_slot = slot.clone();
                next_slots.push(entry);
            } else {
                let mut appearance = AnimatedValue::new_settled(0.0);
                appearance.retarget(
                    1.0,
                    now,
                    DOCK_PRESENTATION_DURATION,
                    Easing::EaseOutCubic,
                    false,
                );
                next_slots.push(PresentedSlot {
                    key,
                    axis_center: AnimatedValue::new_settled(target_center),
                    appearance,
                    leaving: false,
                    last_slot: slot.clone(),
                });
            }
        }

        for (_, mut leftover) in existing_by_key {
            if dragged_key.as_deref() == Some(leftover.key.as_str()) {
                continue;
            }
            if !leftover.leaving {
                leftover.leaving = true;
                leftover.appearance.retarget(
                    0.0,
                    now,
                    DOCK_LEAVE_DURATION,
                    Easing::EaseInOutCubic,
                    false,
                );
            }
            if leftover.appearance.is_active() && leftover.appearance.current() > 0.02 {
                next_slots.push(leftover);
            }
        }

        self.slots = next_slots;
    }

    /// Produces the interpolated [`ComputedDockLayout`] matching the current animation state.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn apply(&self, target: &ComputedDockLayout, pos: DockPosition) -> ComputedDockLayout {
        if !self.initialized || !self.is_active() {
            return target.clone();
        }

        let cap_start = self.capsule_axis_start.current().round() as i32;
        let cap_len = (self.capsule_axis_len.current().round() as i32).max(48);
        let capsule_rect = replace_rect_axis(pos, target.capsule_rect, cap_start, cap_len);

        let dividers = target
            .dividers
            .iter()
            .enumerate()
            .map(|(idx, &div)| {
                self.dividers.get(idx).map_or(div, |anim| {
                    let (_, len) = rect_axis_bounds(pos, div);
                    replace_rect_axis(pos, div, anim.current().round() as i32, len.round() as i32)
                })
            })
            .collect();

        let by_key: HashMap<&str, &PresentedSlot> = self
            .slots
            .iter()
            .filter(|s| !s.leaving)
            .map(|s| (s.key.as_str(), s))
            .collect();

        let mut slots = Vec::with_capacity(self.slots.len());
        for slot in &target.slots {
            let key = dock_tile_presentation_key(&slot.tile);
            let mut out_slot = slot.clone();
            if let Some(entry) = by_key.get(key.as_str()) {
                let (start, len) = rect_axis_bounds(pos, slot.hit_rect);
                let target_center = start + len * 0.5;
                let delta = (entry.axis_center.current() - target_center).round() as i32;
                out_slot.hit_rect = shift_rect_axis(pos, out_slot.hit_rect, delta);
                out_slot.icon_rect = shift_rect_axis(pos, out_slot.icon_rect, delta);
                out_slot.dot_rect = shift_rect_axis(pos, out_slot.dot_rect, delta);
                let app = entry.appearance.current().clamp(0.0, 1.0);
                if app < 0.995 {
                    let scale = 0.55 + 0.45 * app;
                    out_slot.icon_rect = scale_rect_centered(out_slot.icon_rect, scale);
                }
            }
            slots.push(out_slot);
        }

        for entry in self.slots.iter().filter(|s| s.leaving) {
            let app = entry.appearance.current().clamp(0.0, 1.0);
            if app <= 0.02 {
                continue;
            }
            let mut ghost = entry.last_slot.clone();
            ghost.slot_idx = usize::MAX;
            ghost.tile.has_indicator = false;
            ghost.tile.badge_count = 0;
            ghost.hit_rect = Rectangle::new(Point::from((0, 0)), Size::from((0, 0)));
            let scale = 0.45 + 0.55 * app;
            ghost.icon_rect = scale_rect_centered(ghost.icon_rect, scale);
            slots.push(ghost);
        }

        ComputedDockLayout {
            capsule_rect,
            dividers,
            slots,
            drag_remove_badge: target.drag_remove_badge,
            dragged_icon: target.dragged_icon.clone(),
        }
    }

    fn snap_to_target(
        &mut self,
        target: &ComputedDockLayout,
        pos: DockPosition,
        screen_size: Size<i32, Physical>,
    ) {
        let (cap_start, cap_len) = rect_axis_bounds(pos, target.capsule_rect);
        self.initialized = true;
        self.last_position = pos;
        self.last_screen_size = screen_size;
        self.capsule_axis_start = AnimatedValue::new_settled(cap_start);
        self.capsule_axis_len = AnimatedValue::new_settled(cap_len);
        self.dividers = target
            .dividers
            .iter()
            .map(|r| AnimatedValue::new_settled(rect_axis_bounds(pos, *r).0))
            .collect();
        self.slots = target
            .slots
            .iter()
            .map(|slot| {
                let (start, len) = rect_axis_bounds(pos, slot.hit_rect);
                PresentedSlot {
                    key: dock_tile_presentation_key(&slot.tile),
                    axis_center: AnimatedValue::new_settled(start + len * 0.5),
                    appearance: AnimatedValue::new_settled(1.0),
                    leaving: false,
                    last_slot: slot.clone(),
                }
            })
            .collect();
    }
}

fn retarget_if_changed(anim: &mut AnimatedValue, target: f64, now: Duration) {
    if (anim.target() - target).abs() > 0.5 {
        anim.retarget(
            target,
            now,
            DOCK_PRESENTATION_DURATION,
            Easing::EaseOutCubic,
            false,
        );
    }
}

fn rect_axis_bounds(pos: DockPosition, rect: Rectangle<i32, Physical>) -> (f64, f64) {
    match pos {
        DockPosition::Bottom => (f64::from(rect.loc.x), f64::from(rect.size.w)),
        DockPosition::Left | DockPosition::Right => (f64::from(rect.loc.y), f64::from(rect.size.h)),
    }
}

fn replace_rect_axis(
    pos: DockPosition,
    rect: Rectangle<i32, Physical>,
    axis_start: i32,
    axis_len: i32,
) -> Rectangle<i32, Physical> {
    match pos {
        DockPosition::Bottom => Rectangle::new(
            Point::from((axis_start, rect.loc.y)),
            Size::from((axis_len, rect.size.h)),
        ),
        DockPosition::Left | DockPosition::Right => Rectangle::new(
            Point::from((rect.loc.x, axis_start)),
            Size::from((rect.size.w, axis_len)),
        ),
    }
}

fn shift_rect_axis(
    pos: DockPosition,
    rect: Rectangle<i32, Physical>,
    delta: i32,
) -> Rectangle<i32, Physical> {
    match pos {
        DockPosition::Bottom => {
            Rectangle::new(Point::from((rect.loc.x + delta, rect.loc.y)), rect.size)
        }
        DockPosition::Left | DockPosition::Right => {
            Rectangle::new(Point::from((rect.loc.x, rect.loc.y + delta)), rect.size)
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn scale_rect_centered(rect: Rectangle<i32, Physical>, scale: f64) -> Rectangle<i32, Physical> {
    let w = (f64::from(rect.size.w) * scale).round().max(8.0) as i32;
    let h = (f64::from(rect.size.h) * scale).round().max(8.0) as i32;
    let dx = (rect.size.w - w) / 2;
    let dy = (rect.size.h - h) / 2;
    Rectangle::new(
        Point::from((rect.loc.x + dx, rect.loc.y + dy)),
        Size::from((w, h)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell_render::{DockHit, DockViewState, compute_dock_layout};

    fn make_tile(section: DockSection, key: &str, hit: DockHit) -> DockAppTile {
        DockAppTile {
            section,
            app_key: key.to_owned(),
            label: key.to_owned(),
            hit,
            builtin_icon: None,
            representative_win: None,
            window_count: 0,
            has_minimized: false,
            is_active: false,
            has_indicator: false,
            icon_name: None,
            toplevel_icon: None,
            fallback_title: key.to_owned(),
            badge_count: 0,
        }
    }

    #[test]
    fn smooth_presentation_glides_slots_and_capsule_and_respects_reduced_motion() {
        let screen = Size::from((1280, 800));
        let tiles_a = vec![
            make_tile(DockSection::Pinned, "files", DockHit::PinnedFiles),
            make_tile(DockSection::Pinned, "terminal", DockHit::PinnedTerminal),
            make_tile(DockSection::Utility, "trash", DockHit::PinnedTrash),
        ];
        let view_a = DockViewState {
            tiles: &tiles_a,
            ..DockViewState::default()
        };
        let layout_a = compute_dock_layout(screen, &view_a);

        let mut pres = DockPresentation::new();
        let t0 = Duration::from_millis(100);
        pres.sync_target(&layout_a, DockPosition::Bottom, screen, t0, false);
        assert!(!pres.is_active(), "first sync should snap cleanly");
        assert_eq!(
            pres.apply(&layout_a, DockPosition::Bottom).capsule_rect,
            layout_a.capsule_rect
        );

        let tiles_b = vec![
            make_tile(DockSection::Pinned, "files", DockHit::PinnedFiles),
            make_tile(DockSection::Pinned, "terminal", DockHit::PinnedTerminal),
            make_tile(
                DockSection::UnpinnedOrRecent,
                "firefox",
                DockHit::WindowSlot(0),
            ),
            make_tile(DockSection::Utility, "trash", DockHit::PinnedTrash),
        ];
        let view_b = DockViewState {
            tiles: &tiles_b,
            ..DockViewState::default()
        };
        let layout_b = compute_dock_layout(screen, &view_b);

        pres.sync_target(&layout_b, DockPosition::Bottom, screen, t0, false);
        assert!(pres.is_active());
        assert!(pres.tick(t0 + Duration::from_millis(80)));
        let mid = pres.apply(&layout_b, DockPosition::Bottom);
        assert!(mid.capsule_rect.size.w > layout_a.capsule_rect.size.w);
        assert!(mid.capsule_rect.size.w < layout_b.capsule_rect.size.w);
        let mid_firefox = mid
            .slots
            .iter()
            .find(|s| s.tile.app_key == "firefox")
            .map(|s| s.icon_rect.size.w)
            .unwrap_or(0);
        let dst_firefox = layout_b
            .slots
            .iter()
            .find(|s| s.tile.app_key == "firefox")
            .map(|s| s.icon_rect.size.w)
            .unwrap_or(0);
        assert!(mid_firefox < dst_firefox);

        let _ = pres.tick(t0 + Duration::from_millis(400));
        assert!(!pres.is_active());
        assert_eq!(
            pres.apply(&layout_b, DockPosition::Bottom).capsule_rect,
            layout_b.capsule_rect
        );

        // Removing firefox creates a brief shrinking ghost and collapses cleanly.
        pres.sync_target(
            &layout_a,
            DockPosition::Bottom,
            screen,
            t0 + Duration::from_millis(500),
            false,
        );
        assert!(pres.is_active());
        let _ = pres.tick(t0 + Duration::from_millis(540));
        let leaving = pres.apply(&layout_a, DockPosition::Bottom);
        assert!(leaving.slots.iter().any(|s| s.slot_idx == usize::MAX));
        let _ = pres.tick(t0 + Duration::from_millis(900));
        assert!(!pres.is_active());

        // Reduced motion snaps immediately.
        pres.sync_target(
            &layout_b,
            DockPosition::Bottom,
            screen,
            t0 + Duration::from_millis(1000),
            true,
        );
        assert!(!pres.is_active());
        assert_eq!(
            pres.apply(&layout_b, DockPosition::Bottom).capsule_rect,
            layout_b.capsule_rect
        );
    }
}
