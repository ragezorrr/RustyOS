//! Window thumbnail, open/close lifecycle snapshot, and minimize warp composition.

use std::time::Duration;

use smithay::{
    backend::renderer::gles::GlesRenderer,
    utils::{Logical, Point, Rectangle, Size},
};

use super::{client_surfaces::push_window_elements_with_options, scale_render_elements_for_output};
use crate::{shell_render::CompositorRenderElement, state::RustyCompositorState};

pub(super) fn prune_inactive_minimize_snapshots(
    state: &RustyCompositorState,
    visible_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    shape_cache: &mut crate::surfaces::UiShapeCache,
) {
    if shape_cache.has_any_minimize_snapshots() {
        shape_cache.retain_minimize_snapshots(|win_id_u64| {
            let win_id = rusty_wm::WindowId(win_id_u64);
            state.is_window_minimized(win_id)
                || state
                    .minimize_anims
                    .iter()
                    .any(|anim| anim.win_id == win_id)
        });
    }
    shape_cache.retain_lifecycle_snapshots(|win_id_u64| {
        let win_id = rusty_wm::WindowId(win_id_u64);
        visible_windows.iter().any(|(id, _)| *id == win_id)
            || state
                .lifecycle_anims
                .iter()
                .any(|anim| anim.win_id == win_id)
    });
    shape_cache.retain_liquid_glass_slots(|slot_id| {
        if (slot_id & 0x8000_0000_0000_0000) == 0 {
            return true;
        }
        let win_id_part = slot_id & 0x00FF_FFFF_FFFF;
        visible_windows
            .iter()
            .any(|(id, _)| (id.0 & 0x00FF_FFFF_FFFF) == win_id_part)
    });
}

fn window_lifecycle_commit_sig(
    state: &RustyCompositorState,
    win_id: rusty_wm::WindowId,
    frame_rect: rusty_wm::Rect,
    is_focused: bool,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    state.window_commit_revision(win_id).hash(&mut h);
    frame_rect.width.hash(&mut h);
    frame_rect.height.hash(&mut h);
    is_focused.hash(&mut h);
    (!state.window_uses_csd(win_id)).hash(&mut h);
    (state.hovered_traffic_light_window == Some(win_id)).hash(&mut h);
    state.reduce_transparency.hash(&mut h);
    state.user_config.appearance.hash(&mut h);
    state.user_config.ui_accent_index.hash(&mut h);
    h.finish()
}

fn capture_window_local_elements(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    win: (rusty_wm::WindowId, rusty_wm::Rect, bool),
    output_loc: Point<i32, Logical>,
    output_scale: f64,
) -> (rusty_wm::Rect, Vec<CompositorRenderElement>) {
    let (win_id, rect, is_focused) = win;
    let frame_rect = state.resolved_window_global_rect(win_id).unwrap_or(rect);
    let win_origin = (frame_rect.x, frame_rect.y);
    let glass_offset = Point::from((win_origin.0 - output_loc.x, win_origin.1 - output_loc.y));
    let mut local_elements = Vec::new();
    push_window_elements_with_options(
        &mut local_elements,
        renderer,
        text_cache,
        shape_cache,
        state,
        win_id,
        rect,
        Point::from(win_origin),
        output_scale,
        is_focused,
        state.is_window_fullscreen(win_id),
        false,
        glass_offset,
    );
    (
        frame_rect,
        scale_render_elements_for_output(local_elements, output_scale),
    )
}

/// Captures/refreshes live GPU window snapshots for `Workspace Overview` and `Alt+Tab` (`RUSTYOS-132`, `RUSTYOS-167`).
/// Skips re-rendering windows whose `commit_sig` has not changed since the last capture.
#[allow(clippy::too_many_arguments)]
pub(super) fn capture_window_thumbnails(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    output_loc: Point<i32, Logical>,
    output_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    focused_win: Option<rusty_wm::WindowId>,
    output_scale: f64,
    window_ids: &[rusty_wm::WindowId],
) {
    for &win_id in window_ids {
        let Some(rect) = output_windows
            .iter()
            .find(|(id, _)| *id == win_id)
            .map(|(_, r)| *r)
            .or_else(|| state.wm.window(win_id).map(|w| w.geometry))
        else {
            continue;
        };
        if state.is_window_minimized(win_id) {
            continue;
        }
        let is_focused = focused_win == Some(win_id);
        let probe_rect = state.resolved_window_global_rect(win_id).unwrap_or(rect);
        let sig = window_lifecycle_commit_sig(state, win_id, probe_rect, is_focused);
        if shape_cache.lifecycle_snapshot_matches_sig(win_id.0, sig) {
            continue;
        }
        let (actual_rect, scaled) = capture_window_local_elements(
            renderer,
            text_cache,
            shape_cache,
            state,
            (win_id, rect, is_focused),
            output_loc,
            output_scale,
        );
        let _ = shape_cache.capture_lifecycle_snapshot(
            renderer,
            win_id.0,
            Size::from((actual_rect.width.max(1), actual_rect.height.max(1))),
            output_scale,
            sig,
            &scaled,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn capture_pending_open_lifecycle_snapshots(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &mut RustyCompositorState,
    output_loc: Point<i32, Logical>,
    output_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    focused_win: Option<rusty_wm::WindowId>,
    output_scale: f64,
) {
    let now = state.elapsed();
    for idx in 0..state.lifecycle_anims.len() {
        let (win_id, kind, started_after_commit, snapshot_pending) = {
            let anim = &state.lifecycle_anims[idx];
            (
                anim.win_id,
                anim.kind,
                anim.started_after_commit,
                anim.snapshot_pending,
            )
        };
        if kind != crate::state::WindowLifecycleKind::Open {
            continue;
        }
        let Some(&(_, rect)) = output_windows.iter().find(|(id, _)| *id == win_id) else {
            continue;
        };
        let is_focused = focused_win == Some(win_id);
        let probe_rect = state.resolved_window_global_rect(win_id).unwrap_or(rect);
        let sig = window_lifecycle_commit_sig(state, win_id, probe_rect, is_focused);
        if started_after_commit
            && !snapshot_pending
            && shape_cache.lifecycle_snapshot_matches_sig(win_id.0, sig)
        {
            continue;
        }
        let (actual_rect, scaled) = capture_window_local_elements(
            renderer,
            text_cache,
            shape_cache,
            state,
            (win_id, rect, is_focused),
            output_loc,
            output_scale,
        );
        if scaled.is_empty() {
            continue;
        }
        if shape_cache.capture_lifecycle_snapshot(
            renderer,
            win_id.0,
            Size::from((actual_rect.width.max(1), actual_rect.height.max(1))),
            output_scale,
            sig,
            &scaled,
        ) {
            let anim = &mut state.lifecycle_anims[idx];
            anim.window_rect = actual_rect;
            anim.focused = is_focused;
            anim.snapshot_pending = false;
            if !anim.started_after_commit {
                anim.started_after_commit = true;
                let mut progress = rusty_animation::AnimatedValue::new_settled(0.0);
                progress.retarget(
                    1.0,
                    now,
                    Duration::from_millis(180),
                    rusty_animation::Easing::EaseOutQuart,
                    false,
                );
                anim.progress = progress;
                state.shell_redraw_needed = true;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn refresh_idle_close_lifecycle_snapshots(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    output_loc: Point<i32, Logical>,
    output_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    focused_win: Option<rusty_wm::WindowId>,
    output_scale: f64,
) {
    if state.animations.reduced_motion() {
        return;
    }
    for &(win_id, rect) in output_windows {
        if state.is_window_minimized(win_id)
            || state.is_window_fullscreen(win_id)
            || state.minimize_anims.iter().any(|a| a.win_id == win_id)
            || state.lifecycle_anims.iter().any(|a| a.win_id == win_id)
        {
            continue;
        }
        let is_focused = focused_win == Some(win_id);
        let probe_rect = state.resolved_window_global_rect(win_id).unwrap_or(rect);
        let sig = window_lifecycle_commit_sig(state, win_id, probe_rect, is_focused);
        if shape_cache.lifecycle_snapshot_matches_sig(win_id.0, sig) {
            continue;
        }
        let (actual_rect, scaled) = capture_window_local_elements(
            renderer,
            text_cache,
            shape_cache,
            state,
            (win_id, rect, is_focused),
            output_loc,
            output_scale,
        );
        if !scaled.is_empty() {
            let _ = shape_cache.capture_lifecycle_snapshot(
                renderer,
                win_id.0,
                Size::from((actual_rect.width.max(1), actual_rect.height.max(1))),
                output_scale,
                sig,
                &scaled,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_window_lifecycle_snapshots_for_output(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &mut RustyCompositorState,
    output_loc: Point<i32, Logical>,
    output_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    focused_win: Option<rusty_wm::WindowId>,
    fullscreen_win: Option<rusty_wm::WindowId>,
    output_scale: f64,
) {
    if fullscreen_win.is_some() {
        return;
    }
    capture_pending_open_lifecycle_snapshots(
        renderer,
        text_cache,
        shape_cache,
        state,
        output_loc,
        output_windows,
        focused_win,
        output_scale,
    );
    refresh_idle_close_lifecycle_snapshots(
        renderer,
        text_cache,
        shape_cache,
        state,
        output_loc,
        output_windows,
        focused_win,
        output_scale,
    );
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub(super) fn push_lifecycle_warp_elements(
    backdrop_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    output_geo: Rectangle<i32, Logical>,
    output_scale: f64,
) {
    let output_loc = output_geo.loc;
    for anim in &state.lifecycle_anims {
        if !anim.started_after_commit {
            continue;
        }
        let closing = anim.kind == crate::state::WindowLifecycleKind::Close;
        let anim_geo = Rectangle::new(
            Point::from((anim.window_rect.x, anim.window_rect.y)),
            Size::from((
                anim.window_rect.width.max(1),
                anim.window_rect.height.max(1),
            )),
        );
        if anim_geo.intersection(output_geo).is_none() {
            continue;
        }
        let t = anim.progress.current().clamp(0.0, 1.0);
        let (scale_factor, alpha) = if closing {
            (1.0 - 0.07 * t, 1.0 - t)
        } else {
            (0.93 + 0.07 * t, t)
        };
        let win_rect = Rectangle::new(
            Point::from((
                anim.window_rect.x - output_loc.x,
                anim.window_rect.y - output_loc.y,
            )),
            Size::from((
                anim.window_rect.width.max(1),
                anim.window_rect.height.max(1),
            )),
        );
        if let Some(el) = shape_cache.lifecycle_warp_element(
            renderer,
            anim.win_id.0,
            win_rect,
            t,
            closing,
            output_scale,
        ) {
            backdrop_list.push(el);
        }
        let shadow_alpha = (alpha * alpha) as f32;
        if state.user_config.window_shadow != crate::config::WindowShadowMode::None
            && shadow_alpha > 0.002
        {
            let scaled_rect = crate::surfaces::centered_scaled_rect(win_rect, scale_factor);
            let frame_rect = rusty_wm::Rect::new(
                scaled_rect.loc.x,
                scaled_rect.loc.y,
                scaled_rect.size.w,
                scaled_rect.size.h,
            );
            backdrop_list.extend(
                crate::shell_render::generate_window_shadow_elements_with_opacity(
                    shape_cache,
                    renderer,
                    frame_rect,
                    anim.focused,
                    state.user_config.window_shadow,
                    shadow_alpha,
                ),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_minimize_snapshots_for_output(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &mut RustyCompositorState,
    output_loc: Point<i32, Logical>,
    output_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    output_scale: f64,
) {
    if state.minimize_anims.is_empty() && state.minimized_window_ids.is_empty() {
        return;
    }
    for idx in 0..state.minimize_anims.len() {
        let anim = &state.minimize_anims[idx];
        if !output_windows.iter().any(|(id, _)| *id == anim.win_id) {
            continue;
        }
        if !anim.snapshot_pending && shape_cache.has_minimize_snapshot(anim.win_id.0) {
            continue;
        }
        let win_id = anim.win_id;
        let rect = anim.window_rect;
        let win_origin = state
            .animations
            .animated_window_origin(win_id.0)
            .unwrap_or((rect.x, rect.y));
        let glass_offset = Point::from((win_origin.0 - output_loc.x, win_origin.1 - output_loc.y));
        let mut local_elements = Vec::new();
        push_window_elements_with_options(
            &mut local_elements,
            renderer,
            text_cache,
            shape_cache,
            state,
            win_id,
            rect,
            Point::from((win_origin.0, win_origin.1)),
            output_scale,
            true,
            state.is_window_fullscreen(win_id),
            false,
            glass_offset,
        );
        let scaled = scale_render_elements_for_output(local_elements, output_scale);
        let captured = shape_cache.capture_minimize_snapshot(
            renderer,
            win_id.0,
            Size::from((rect.width.max(1), rect.height.max(1))),
            output_scale,
            &scaled,
        );
        state.minimize_anims[idx].snapshot_pending = false;
        if captured && !state.minimize_anims[idx].restoring {
            let now = state.elapsed();
            let mut progress = rusty_animation::AnimatedValue::new_settled(0.0);
            progress.retarget(
                1.0,
                now,
                std::time::Duration::from_millis(280),
                rusty_animation::Easing::EaseInOutCubic,
                false,
            );
            state.minimize_anims[idx].progress = progress;
        }
    }
    // Also capture snapshots for minimized windows lacking a cached texture (e.g. `reduced_motion == true`) (`RUSTYOS-162`).
    for &(win_id, rect) in output_windows {
        if !state.is_window_minimized(win_id)
            || shape_cache.has_minimize_snapshot(win_id.0)
            || state.minimize_anims.iter().any(|a| a.win_id == win_id)
        {
            continue;
        }
        let win_origin = state
            .animations
            .animated_window_origin(win_id.0)
            .unwrap_or((rect.x, rect.y));
        let glass_offset = Point::from((win_origin.0 - output_loc.x, win_origin.1 - output_loc.y));
        let mut local_elements = Vec::new();
        push_window_elements_with_options(
            &mut local_elements,
            renderer,
            text_cache,
            shape_cache,
            state,
            win_id,
            rect,
            Point::from((win_origin.0, win_origin.1)),
            output_scale,
            true,
            state.is_window_fullscreen(win_id),
            false,
            glass_offset,
        );
        if local_elements.is_empty() {
            continue;
        }
        let scaled = scale_render_elements_for_output(local_elements, output_scale);
        let _ = shape_cache.capture_minimize_snapshot(
            renderer,
            win_id.0,
            Size::from((rect.width.max(1), rect.height.max(1))),
            output_scale,
            &scaled,
        );
    }
}

#[allow(clippy::cast_possible_truncation)]
pub(super) fn push_minimize_warp_elements(
    backdrop_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    output_loc: Point<i32, Logical>,
    output_scale: f64,
    output_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
) {
    for anim in &state.minimize_anims {
        if !output_windows.iter().any(|(id, _)| *id == anim.win_id) {
            continue;
        }
        let t = anim.progress.current().clamp(0.0, 1.0);
        let win_rect = Rectangle::new(
            Point::from((
                anim.window_rect.x - output_loc.x,
                anim.window_rect.y - output_loc.y,
            )),
            Size::from((
                anim.window_rect.width.max(1),
                anim.window_rect.height.max(1),
            )),
        );
        let dock_rect = Rectangle::new(
            Point::from((
                anim.dock_rect.x - output_loc.x,
                anim.dock_rect.y - output_loc.y,
            )),
            Size::from((anim.dock_rect.width.max(1), anim.dock_rect.height.max(1))),
        );
        if let Some(el) = shape_cache.minimize_warp_element(
            renderer,
            anim.win_id.0,
            win_rect,
            dock_rect,
            output_scale,
            t,
            state.user_config.dock_minimize_effect,
            state.user_config.dock_position,
        ) {
            backdrop_list.push(el);
        } else {
            let lx = (f64::from(win_rect.loc.x) + (f64::from(dock_rect.loc.x - win_rect.loc.x) * t))
                .round() as i32;
            let ly = (f64::from(win_rect.loc.y) + (f64::from(dock_rect.loc.y - win_rect.loc.y) * t))
                .round() as i32;
            let lw = (f64::from(win_rect.size.w)
                + (f64::from(dock_rect.size.w - win_rect.size.w) * t))
                .round()
                .max(8.0) as i32;
            let lh = (f64::from(win_rect.size.h)
                + (f64::from(dock_rect.size.h - win_rect.size.h) * t))
                .round()
                .max(8.0) as i32;
            if let Some(card) = shape_cache.dock_glass_render_element(
                renderer,
                lw,
                lh,
                10,
                state.glass_opacity,
                state.reduce_transparency,
                Point::from((lx, ly)),
            ) {
                backdrop_list.push(card);
            }
        }
    }
}
