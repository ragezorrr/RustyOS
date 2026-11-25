//! Backend-agnostic frame composition, `HiDPI` element scaling, screenshots, and portal captures (`RUSTYOS-060`).

mod client_surfaces;
mod portal_capture;
mod snapshots;

use client_surfaces::{
    push_client_popup_elements, push_window_backdrop_elements,
    push_xwayland_override_redirect_elements, window_glass_backdrop_slot,
};
pub(crate) use portal_capture::{
    process_pending_screenshot, process_portal_captures_and_streams, push_client_surface_cursor,
    quit_plymouth_retain_splash, spawn_greeter_client, spawn_test_client,
};
#[cfg(test)]
use portal_capture::{should_skip_uncommitted_bound_window, split_shell_words};
use snapshots::{
    capture_window_thumbnails, prepare_minimize_snapshots_for_output,
    prepare_window_lifecycle_snapshots_for_output, prune_inactive_minimize_snapshots,
    push_lifecycle_warp_elements, push_minimize_warp_elements,
};

use std::fs;
use std::time::Instant;

use rusty_ipc::IpcServer;
use smithay::{
    backend::renderer::{
        Color32F,
        element::AsRenderElements,
        element::Element,
        element::Kind,
        element::surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        element::utils::{Relocate, RelocateRenderElement, RescaleRenderElement},
        gles::GlesRenderer,
    },
    input::pointer::{CursorIcon, CursorImageStatus},
    reexports::wayland_server::Display,
    utils::{IsAlive, Logical, Point, Rectangle, Size},
    wayland::shell::wlr_layer::Layer,
};
use tracing::{error, info, warn};

use crate::{
    BackendMode, CompositorError,
    shell_render::{
        CompositorRenderElement, DockWindowEntry, MAX_TOP_BAR_TRAY_ITEMS,
        MAX_VISIBLE_NOTIFICATIONS, NotificationToastView, OverviewRenderState,
        PolkitPromptRenderState, PortalPromptRenderState, PortalSourceRow,
        QuickSettingsRenderState, ShellOverlayView, SpotlightRenderState, TopBarTelemetry,
        TopBarTrayItem, WindowSwitcherCard, WindowSwitcherRenderState,
    },
    state::RustyCompositorState,
};

/// Background clear color matching `background = "#141518"` from `DESIGN.md` (RustyOS).
pub(crate) const BACKGROUND_COLOR: Color32F = Color32F::new(0.078, 0.082, 0.094, 1.0);

/// Controls cursor inclusion and hardware cursor plane candidacy per output frame (`RUSTYOS-046`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorRenderMode {
    /// Do not render any compositor cursor element (e.g. compositor screenshot capture).
    None,
    /// Composite the cursor on the primary scene (`Kind::Unspecified`) without hardware plane promotion.
    SoftwareComposited,
    /// Promote cursor elements with `Kind::Cursor` when fitting inside `max_size`, else fall back to `Kind::Unspecified`.
    HardwarePlane {
        max_size: smithay::utils::Size<u32, smithay::utils::Buffer>,
    },
}

pub(crate) fn dispatch_and_refresh_clients(
    display: &mut Display<RustyCompositorState>,
    ipc_server: Option<&mut IpcServer>,
    app_state: &mut RustyCompositorState,
) {
    let dispatch_start = Instant::now();
    app_state.drain_dbus_notifications();
    if app_state.expire_notifications() {
        app_state.shell_redraw_needed = true;
    }
    app_state.drain_tray_updates();
    app_state.drain_screenshot_results();
    app_state.drain_trash_empty_results();
    app_state.drain_trash_monitor_updates();
    app_state.drain_telemetry_updates();
    app_state.drain_app_catalog_updates();
    app_state.drain_dock_dnd_events();
    let _ = app_state.drain_system_integration_events(smithay::utils::SERIAL_COUNTER.next_serial());
    if let Some(ipc) = ipc_server {
        ipc.poll_requests(|req| app_state.handle_ipc_request(req));
    }
    if let Err(err) = display.dispatch_clients(app_state) {
        error!(error = %err, "Failed to dispatch Wayland client events");
    }
    app_state.flush_pending_toplevel_configures();
    app_state.prune_dead_windows();
    app_state.launcher.reap_children();
    app_state.space.refresh();
    app_state.popups.cleanup();
    app_state.cleanup_ime_popups();
    if let Err(err) = display.flush_clients() {
        warn!(error = %err, "Failed to flush Wayland clients");
    }
    app_state
        .frame_timing
        .record_dispatch_duration(dispatch_start.elapsed());
}

pub(crate) fn finalize_frame_timing(
    state: &RustyCompositorState,
    backend_mode: BackendMode,
    initial_size: smithay::utils::Size<i32, smithay::utils::Physical>,
    metrics_out: Option<&str>,
) -> Result<crate::frame_timing::FrameTimingSummary, CompositorError> {
    let timing_summary = state.frame_timing.summary();
    info!(
        summary = %timing_summary.to_log_line(),
        "Frame timing instrumentation summary"
    );

    if let Some(path) = metrics_out {
        let mode_size = state
            .output
            .current_mode()
            .map_or((initial_size.w, initial_size.h), |m| (m.size.w, m.size.h));
        let json = timing_summary.to_json(backend_mode.as_str(), mode_size);
        fs::write(path, json).map_err(|err| CompositorError::MetricsExport(err.to_string()))?;
        info!(path = %path, "Exported frame timing JSON report");
    }

    Ok(timing_summary)
}

/// Renders all mapped `wlr-layer-shell` surfaces belonging to `layers` onto `render_list`,
/// returning `true` iff at least one surface on the requested layers was emitted (`RUSTYOS-060`).
fn push_layer_elements(
    render_list: &mut Vec<CompositorRenderElement>,
    output: &smithay::output::Output,
    renderer: &mut GlesRenderer,
    layers: &[Layer],
    output_scale: f64,
) -> bool {
    let layer_map = smithay::desktop::layer_map_for_output(output);
    let mut rendered_any = false;
    for &layer in layers {
        for surface in layer_map.layers_on(layer) {
            if let Some(geo) = layer_map.layer_geometry(surface) {
                rendered_any = true;
                let loc = geo.loc.to_physical_precise_round(output_scale);
                render_list.extend(
                    AsRenderElements::<GlesRenderer>::render_elements::<
                        WaylandSurfaceRenderElement<GlesRenderer>,
                    >(
                        surface,
                        renderer,
                        loc,
                        smithay::utils::Scale::from(output_scale),
                        1.0,
                    )
                    .into_iter()
                    .map(CompositorRenderElement::Surface),
                );
            }
        }
    }
    rendered_any
}

fn push_ime_popup_elements(
    render_list: &mut Vec<CompositorRenderElement>,
    output: &smithay::output::Output,
    renderer: &mut GlesRenderer,
    state: &RustyCompositorState,
    output_scale: f64,
) {
    let scale = smithay::utils::Scale::from(output_scale);
    for (popup, local_loc) in state.ime_popups_for_output(output) {
        let phys_loc = local_loc.to_physical_precise_round(output_scale);
        let elements = render_elements_from_surface_tree::<
            GlesRenderer,
            WaylandSurfaceRenderElement<GlesRenderer>,
        >(
            renderer,
            popup.wl_surface(),
            phys_loc,
            scale,
            1.0,
            Kind::Unspecified,
        );
        render_list.extend(elements.into_iter().map(CompositorRenderElement::Surface));
    }
}

fn relocate_hi_dpi_element<E>(
    element: E,
    scale: f64,
    wrap_scaled: impl FnOnce(RelocateRenderElement<E>) -> CompositorRenderElement,
    wrap_plain: impl FnOnce(E) -> CompositorRenderElement,
) -> CompositorRenderElement
where
    E: Element,
{
    let logical_loc = element.geometry(smithay::utils::Scale::from(1.0)).loc;
    let target_loc: Point<i32, smithay::utils::Physical> =
        Point::<i32, Logical>::from((logical_loc.x, logical_loc.y))
            .to_physical_precise_round(scale);
    let delta = Point::from((target_loc.x - logical_loc.x, target_loc.y - logical_loc.y));
    if delta == Point::from((0, 0)) {
        wrap_plain(element)
    } else {
        wrap_scaled(RelocateRenderElement::from_element(
            element,
            delta,
            Relocate::Relative,
        ))
    }
}

/// Scales logical shell/SSD elements (`Solid`, `Memory`, `Texture`) into physical `HiDPI` coordinates for `scale`.
pub(crate) fn scale_render_elements_for_output(
    elements: Vec<CompositorRenderElement>,
    scale: f64,
) -> Vec<CompositorRenderElement> {
    let clamped = crate::config::clamp_display_scale(scale);
    if (clamped - 1.0).abs() <= 1e-4 {
        return elements;
    }
    elements
        .into_iter()
        .map(|el| match el {
            CompositorRenderElement::Solid(solid) => CompositorRenderElement::ScaledSolid(
                RescaleRenderElement::from_element(solid, Point::from((0, 0)), clamped),
            ),
            CompositorRenderElement::Memory(mem) => relocate_hi_dpi_element(
                mem,
                clamped,
                CompositorRenderElement::ScaledMemory,
                CompositorRenderElement::Memory,
            ),
            CompositorRenderElement::Texture(tex) => relocate_hi_dpi_element(
                tex,
                clamped,
                CompositorRenderElement::ScaledTexture,
                CompositorRenderElement::Texture,
            ),
            CompositorRenderElement::ShaderTexture(glass) => relocate_hi_dpi_element(
                glass,
                clamped,
                CompositorRenderElement::ScaledShaderTexture,
                CompositorRenderElement::ShaderTexture,
            ),
            other => other,
        })
        .collect()
}

/// Returns output-local pointer coordinates if `pointer` falls within (or near) `output_geo` (`RUSTYOS-042`).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn pointer_local_if_on_output(
    pointer: Point<f64, Logical>,
    output_geo: Rectangle<i32, Logical>,
) -> Option<Point<i32, Logical>> {
    let gx = pointer.x.round() as i32;
    let gy = pointer.y.round() as i32;
    let margin = 64;
    let in_bounds = gx >= output_geo.loc.x - margin
        && gx < output_geo.loc.x + output_geo.size.w + margin
        && gy >= output_geo.loc.y - margin
        && gy < output_geo.loc.y + output_geo.size.h + margin;
    in_bounds.then(|| Point::from((gx - output_geo.loc.x, gy - output_geo.loc.y)))
}

/// Builds the front-to-back composite render element list for the primary output.
pub(crate) fn build_frame_render_list(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &mut RustyCompositorState,
    size: smithay::utils::Size<i32, smithay::utils::Physical>,
    include_software_cursor: bool,
) -> Vec<CompositorRenderElement> {
    let primary_id = state.output_map.primary().id;
    build_frame_render_list_for_output(
        renderer,
        text_cache,
        shape_cache,
        state,
        primary_id,
        size,
        include_software_cursor,
    )
}

fn push_ext_session_lock_elements(
    render_list: &mut Vec<CompositorRenderElement>,
    state: &RustyCompositorState,
    renderer: &mut GlesRenderer,
    output: &smithay::output::Output,
    logical_size: Size<i32, smithay::utils::Physical>,
    output_scale: f64,
) {
    if let Some(lock_surf) = state.ext_lock_surface_for_output(output) {
        render_list.extend(
            render_elements_from_surface_tree::<
                GlesRenderer,
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(
                renderer,
                lock_surf.wl_surface(),
                Point::from((0, 0)),
                smithay::utils::Scale::from(output_scale),
                1.0,
                Kind::Unspecified,
            )
            .into_iter()
            .map(CompositorRenderElement::Surface),
        );
    }
    let backdrop = crate::shell_render::make_solid_element(
        Rectangle::new(
            (0, 0).into(),
            (logical_size.w.max(1), logical_size.h.max(1)).into(),
        ),
        Color32F::BLACK,
    );
    render_list.extend(scale_render_elements_for_output(
        vec![CompositorRenderElement::Solid(backdrop)],
        output_scale,
    ));
}

/// Builds the front-to-back composite render element list scoped to a specific output (`RUSTYOS-042`).
pub(crate) fn build_frame_render_list_for_output(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &mut RustyCompositorState,
    output_id: rusty_wm::OutputId,
    size: smithay::utils::Size<i32, smithay::utils::Physical>,
    include_software_cursor: bool,
) -> Vec<CompositorRenderElement> {
    let cursor_mode = if include_software_cursor {
        CursorRenderMode::SoftwareComposited
    } else {
        CursorRenderMode::None
    };
    build_frame_render_list_for_output_with_cursor(
        renderer,
        text_cache,
        shape_cache,
        state,
        output_id,
        size,
        cursor_mode,
    )
}

fn push_cursor_elements(
    render_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    local_ptr: Point<i32, Logical>,
    output_scale: f64,
    cursor_mode: CursorRenderMode,
) {
    let (render_cursor, prefer_hw_cursor, cursor_plane_limit) = match cursor_mode {
        CursorRenderMode::None => (false, false, None),
        CursorRenderMode::SoftwareComposited => (true, false, None),
        CursorRenderMode::HardwarePlane { max_size } => (true, true, Some(max_size)),
    };
    if !render_cursor {
        return;
    }

    let named_cursor_kind = if prefer_hw_cursor {
        Kind::Cursor
    } else {
        Kind::Unspecified
    };
    match &state.cursor_status {
        CursorImageStatus::Hidden => {}
        CursorImageStatus::Surface(surf) if surf.alive() => {
            push_client_surface_cursor(
                render_list,
                renderer,
                surf,
                local_ptr,
                output_scale,
                prefer_hw_cursor,
                cursor_plane_limit,
            );
        }
        CursorImageStatus::Surface(_) => {
            if let Some(cursor_el) = shape_cache.named_cursor_element_for_output(
                renderer,
                CursorIcon::Default,
                state.user_config.cursor_theme.as_deref(),
                state.user_config.cursor_size,
                local_ptr,
                output_scale,
                cursor_plane_limit,
                named_cursor_kind,
            ) {
                render_list.push(CompositorRenderElement::Memory(cursor_el));
            }
        }
        CursorImageStatus::Named(icon) => {
            if let Some(cursor_el) = shape_cache.named_cursor_element_for_output(
                renderer,
                *icon,
                state.user_config.cursor_theme.as_deref(),
                state.user_config.cursor_size,
                local_ptr,
                output_scale,
                cursor_plane_limit,
                named_cursor_kind,
            ) {
                render_list.push(CompositorRenderElement::Memory(cursor_el));
            }
        }
    }
}

fn collect_dock_windows(
    state: &RustyCompositorState,
    visible_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    focused_win: Option<rusty_wm::WindowId>,
    include_dimensions: bool,
) -> Vec<DockWindowEntry> {
    let mut dock_windows: Vec<DockWindowEntry> = visible_windows
        .iter()
        .map(|&(win_id, rect)| {
            let minimized = state.is_window_minimized(win_id);
            let title = state.window_title(win_id);
            let badge = crate::shell_render::make_window_badge(&title);
            let app_id = state.window_app_id(win_id);
            let icon_name = state.launcher.icon_for_app_hints(app_id.as_deref(), &title);
            let toplevel_icon = state.window_toplevel_icons.get(&win_id).cloned();
            DockWindowEntry {
                id: win_id,
                title,
                badge,
                icon_name,
                toplevel_icon,
                dimensions: if include_dimensions {
                    format!("{}x{}", rect.width, rect.height)
                } else {
                    String::new()
                },
                focused: !minimized && focused_win == Some(win_id),
                minimized,
            }
        })
        .collect();
    dock_windows.sort_by_key(|entry| entry.id.0);
    dock_windows
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn build_backdrop_render_elements(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    smithay_output: &smithay::output::Output,
    output_id: u32,
    output_geo: Rectangle<i32, Logical>,
    output_loc: Point<i32, Logical>,
    output_scale: f64,
    output_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    focused_win: Option<rusty_wm::WindowId>,
    fullscreen_win: Option<rusty_wm::WindowId>,
    is_primary: bool,
) -> Vec<CompositorRenderElement> {
    let mut bg_raw: Vec<CompositorRenderElement> = Vec::new();
    let _ = push_layer_elements(
        &mut bg_raw,
        smithay_output,
        renderer,
        &[Layer::Bottom, Layer::Background],
        output_scale,
    );

    if fullscreen_win.is_none() {
        if is_primary && (!state.desktop_items.is_empty() || state.desktop_rubber_band.is_some()) {
            crate::shell_render::render_desktop_canvas_elements(
                renderer,
                text_cache,
                shape_cache,
                &state.desktop_items,
                &state.selected_desktop_items,
                state.desktop_drag.as_ref(),
                state.desktop_rubber_band.as_ref(),
                state.desktop_inline_rename.as_ref(),
                Size::<i32, Logical>::from((output_geo.size.w.max(1), output_geo.size.h.max(1))),
                crate::shell_render::DOCK_HEIGHT,
                &mut bg_raw,
            );
        }
        if let Some(wallpaper) = shape_cache.wallpaper_element(
            renderer,
            Size::<i32, Logical>::from((output_geo.size.w.max(1), output_geo.size.h.max(1))),
            state.user_config.wallpaper.as_deref(),
        ) {
            bg_raw.push(CompositorRenderElement::Memory(wallpaper));
        }
    }

    let mut accumulated = scale_render_elements_for_output(bg_raw, output_scale);
    let backdrop_logical =
        Size::<i32, Logical>::from((output_geo.size.w.max(1), output_geo.size.h.max(1)));

    let drag_snap = state.active_window_drag.and_then(|d| {
        d.snap_preview
            .filter(|(out_id, _, _)| out_id.0 == output_id)
            .map(|(_, _, preview_rect)| (d.window_id, preview_rect))
    });
    let mut drag_snap_drawn = false;

    // Walk bottom-to-top so every Liquid Glass window samples the true Dual-Kawase blur
    // of wallpaper, desktop items, and all windows strictly beneath it (zero self-blur).
    for &(win_id, rect) in output_windows {
        if state.is_window_minimized(win_id)
            || state
                .minimize_anims
                .iter()
                .any(|anim| anim.win_id == win_id)
            || state.lifecycle_anims.iter().any(|anim| {
                anim.win_id == win_id && anim.kind == crate::state::WindowLifecycleKind::Open
            })
        {
            continue;
        }
        if let Some(fs_id) = fullscreen_win
            && win_id != fs_id
        {
            continue;
        }
        if !drag_snap_drawn
            && let Some((drag_win_id, preview_rect)) = drag_snap
            && drag_win_id == win_id
        {
            accumulated = prepend_edge_snap_preview(
                accumulated,
                shape_cache,
                renderer,
                preview_rect,
                output_loc,
                output_scale,
            );
            drag_snap_drawn = true;
        }
        let is_focused = focused_win == Some(win_id);
        let is_fullscreen = fullscreen_win == Some(win_id) || state.is_window_fullscreen(win_id);
        let has_glass = !is_fullscreen
            && !state.reduce_transparency
            && !state.screen_locked
            && state
                .smithay_window_for_id(win_id)
                .and_then(|sw| {
                    smithay::wayland::seat::WaylandFocus::wl_surface(&sw).map(|s| {
                        crate::state::liquid_glass::surface_liquid_glass_state(&s)
                            .is_some_and(|g| g.has_effective_area())
                    })
                })
                .unwrap_or(false);
        if has_glass {
            let slot = window_glass_backdrop_slot(output_id, win_id.0);
            let _ = shape_cache.prepare_liquid_glass_backdrop_slot(
                renderer,
                slot,
                backdrop_logical,
                output_scale,
                &accumulated,
            );
        } else {
            shape_cache.clear_active_liquid_glass_backdrop();
        }

        let mut win_raw: Vec<CompositorRenderElement> = Vec::new();
        push_window_backdrop_elements(
            &mut win_raw,
            renderer,
            text_cache,
            shape_cache,
            state,
            win_id,
            rect,
            output_loc,
            output_scale,
            is_focused,
            is_fullscreen,
        );
        shape_cache.clear_active_liquid_glass_backdrop();
        let mut win_scaled = scale_render_elements_for_output(win_raw, output_scale);
        win_scaled.extend(accumulated);
        accumulated = win_scaled;
    }

    if !drag_snap_drawn && let Some((_, preview_rect)) = drag_snap {
        accumulated = prepend_edge_snap_preview(
            accumulated,
            shape_cache,
            renderer,
            preview_rect,
            output_loc,
            output_scale,
        );
    }

    accumulated
}

fn prepend_edge_snap_preview(
    accumulated: Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    preview_rect: rusty_wm::Rect,
    output_loc: Point<i32, Logical>,
    output_scale: f64,
) -> Vec<CompositorRenderElement> {
    let local_preview = rusty_wm::Rect::new(
        preview_rect.x - output_loc.x,
        preview_rect.y - output_loc.y,
        preview_rect.width,
        preview_rect.height,
    );
    let snap_raw = crate::shell_render::generate_edge_snap_preview_elements(
        shape_cache,
        renderer,
        local_preview,
    );
    let mut snap_scaled = scale_render_elements_for_output(snap_raw, output_scale);
    snap_scaled.extend(accumulated);
    snap_scaled
}

/// Builds the front-to-back composite render element list for `output_id` using `cursor_mode` (`RUSTYOS-046`, `RUSTYOS-060`).
#[allow(clippy::too_many_lines)]
pub(crate) fn build_frame_render_list_for_output_with_cursor(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &mut RustyCompositorState,
    output_id: rusty_wm::OutputId,
    size: smithay::utils::Size<i32, smithay::utils::Physical>,
    cursor_mode: CursorRenderMode,
) -> Vec<CompositorRenderElement> {
    let output_entry = state
        .output_map
        .get(output_id)
        .cloned()
        .unwrap_or_else(|| state.output_map.primary().clone());
    crate::shell_render::set_active_appearance(state.user_config.appearance);
    crate::shell_render::set_active_accent_index(state.user_config.ui_accent_index);
    crate::shell_render::begin_solid_element_frame(output_entry.id.0);
    text_cache.begin_frame();
    text_cache.set_active_scale(output_entry.scale);
    shape_cache.begin_shadow_frame(output_entry.id.0);
    shape_cache.clear_active_liquid_glass_backdrop();
    let output_loc = output_entry.location;
    let output_geo = output_entry.logical_geometry();
    let output_scale = output_entry.scale;
    let logical_size = Size::<i32, smithay::utils::Physical>::from((
        output_geo.size.w.max(1),
        output_geo.size.h.max(1),
    ));
    let _ = size;
    let smithay_output = output_entry.output;
    let is_primary = output_entry.is_primary;

    let active_ws = state.wm.active_workspace_id().0;
    let layout_str = match state.wm.active_layout_mode() {
        rusty_wm::LayoutMode::Floating => "floating",
        rusty_wm::LayoutMode::MasterStack => "master-stack",
    };
    let visible_windows: Vec<(rusty_wm::WindowId, rusty_wm::Rect)> = state
        .wm
        .visible_windows()
        .into_iter()
        .map(|w| (w.id, w.geometry))
        .collect();
    let output_windows: Vec<(rusty_wm::WindowId, rusty_wm::Rect)> = state
        .wm
        .visible_windows_for_output(output_entry.id)
        .into_iter()
        .map(|w| (w.id, w.geometry))
        .collect();
    let window_count = visible_windows
        .iter()
        .filter(|(id, _)| !state.is_window_minimized(*id))
        .count();
    let focused_win = (!state.desktop_focused)
        .then(|| {
            state
                .wm
                .focused_window()
                .filter(|id| !state.is_window_minimized(*id))
        })
        .flatten();

    let mut render_list: Vec<CompositorRenderElement> = Vec::new();
    let local_ptr = pointer_local_if_on_output(state.pointer_location, output_geo);
    if let Some(ptr) = local_ptr {
        push_cursor_elements(
            &mut render_list,
            renderer,
            shape_cache,
            state,
            ptr,
            output_scale,
            cursor_mode,
        );
    }

    if state.ext_session_locked {
        push_ext_session_lock_elements(
            &mut render_list,
            state,
            renderer,
            &smithay_output,
            logical_size,
            output_scale,
        );
        return render_list;
    }

    if let Some(ref dnd_surface) = state.dnd_icon
        && dnd_surface.alive()
        && let Some(ptr) = local_ptr
    {
        let dnd_loc = Point::<i32, Logical>::from((ptr.x + 10, ptr.y + 10))
            .to_physical_precise_round(output_scale);
        render_list.extend(
            render_elements_from_surface_tree::<
                GlesRenderer,
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(
                renderer,
                dnd_surface,
                dnd_loc,
                smithay::utils::Scale::from(output_scale),
                0.9,
                Kind::Unspecified,
            )
            .into_iter()
            .map(CompositorRenderElement::Surface),
        );
    }

    if let Some(ref spot_drag) = state.spotlight_drag
        && spot_drag.active_drag
        && let Some(ptr) = local_ptr
    {
        let sz = (state.user_config.dock_icon_size + 4).clamp(40, 68);
        let icon_loc = Point::<i32, Logical>::from((ptr.x - sz / 2, ptr.y - sz / 2));
        let builtin = crate::shell_render::dock_builtin_for_key(&spot_drag.app_key);
        let resolved = builtin
            .and_then(|b| {
                shape_cache
                    .icons
                    .builtin_squircle_icon_element(renderer, b, sz, icon_loc)
            })
            .or_else(|| {
                spot_drag.icon_name.as_deref().and_then(|name| {
                    shape_cache
                        .icons
                        .icon_element_by_name(renderer, name, sz, icon_loc)
                })
            })
            .or_else(|| {
                shape_cache.icons.builtin_squircle_icon_element(
                    renderer,
                    crate::icons::BuiltinAppIcon::WindowFallback {
                        hue_seed: 0x5A,
                        active: true,
                    },
                    sz,
                    icon_loc,
                )
            });
        if let Some(el) = resolved {
            render_list.push(CompositorRenderElement::Memory(el));
        }
    }

    shape_cache.sync_wallpaper_revision(state.wallpaper_revision);

    let tel = state.telemetry.snapshot_arc();
    let lock_progress = state.lock_screen_anim.current();
    if state.screen_locked || lock_progress > 0.01 {
        let lock_view = state.lock_screen_render_state(&tel, lock_progress);
        if lock_progress >= 0.96
            && !state.reduce_transparency
            && (lock_view.require_password || lock_view.power_menu_open)
            && let Some(wp) = shape_cache.wallpaper_element(
                renderer,
                Size::<i32, Logical>::from((output_geo.size.w.max(1), output_geo.size.h.max(1))),
                state.user_config.wallpaper.as_deref(),
            )
        {
            // Lock Screen blur backdrop MUST contain ONLY the wallpaper — never desktop windows.
            let wallpaper_only = scale_render_elements_for_output(
                vec![CompositorRenderElement::Memory(wp)],
                output_scale,
            );
            let backdrop_logical = Size::<i32, Logical>::from((logical_size.w, logical_size.h));
            let _ = shape_cache.prepare_liquid_glass_backdrop(
                renderer,
                output_entry.id.0,
                backdrop_logical,
                output_scale,
                &wallpaper_only,
            );
        }
        render_list.extend(crate::shell_render::generate_lock_screen_elements(
            text_cache,
            shape_cache,
            renderer,
            logical_size,
            &lock_view,
        ));
        // Once the opaque lock screen veil is fully settled, never composite hidden desktop
        // windows, layer-shell surfaces, popups, or IME overlays underneath it (`RUSTYOS-168`).
        if state.screen_locked && lock_progress >= 0.99 {
            return render_list;
        }
    }

    push_ime_popup_elements(
        &mut render_list,
        &smithay_output,
        renderer,
        state,
        output_scale,
    );

    let mut top_layer_elements: Vec<CompositorRenderElement> = Vec::new();
    let top_layer_surfaces_present = push_layer_elements(
        &mut top_layer_elements,
        &smithay_output,
        renderer,
        &[Layer::Overlay, Layer::Top],
        output_scale,
    );
    let fullscreen_win = if top_layer_surfaces_present {
        None
    } else {
        state.active_unoccluded_fullscreen_window_on(output_entry.id)
    };
    prune_inactive_minimize_snapshots(state, &visible_windows, shape_cache);
    let backdrop_elements = build_backdrop_render_elements(
        renderer,
        text_cache,
        shape_cache,
        state,
        &smithay_output,
        output_entry.id.0,
        output_geo,
        output_loc,
        output_scale,
        &output_windows,
        focused_win,
        fullscreen_win,
        is_primary,
    );
    let hide_chrome_for_fullscreen = fullscreen_win.is_some()
        || state.greeter_mode
        || state.should_hide_shell_chrome_for_fullscreen_on(output_entry.id);
    let context_menu_here = state
        .context_menu
        .as_ref()
        .is_some_and(|ctx| ctx.output_id == output_entry.id);
    let dock_visible_here = !top_layer_surfaces_present
        && !hide_chrome_for_fullscreen
        && !state.greeter_mode
        && !state.is_session_locked()
        && state.dock_reveal_for_output(output_entry.id) > 0.01;
    let dock_stack_here = state
        .dock_stack_render_state_for_output(output_entry.id)
        .is_some();
    let needs_backdrop_blur = !state.screen_locked
        && !state.ext_session_locked
        && !state.reduce_transparency
        && lock_progress <= 0.01
        && (state.has_active_client_liquid_glass()
            || if is_primary {
                !hide_chrome_for_fullscreen
                    || state.osd.is_some()
                    || context_menu_here
                    || state.polkit_prompt.is_some()
                    || state.portal_prompt.is_some()
            } else {
                dock_visible_here
                    || dock_stack_here
                    || (context_menu_here
                        && !top_layer_surfaces_present
                        && fullscreen_win.is_none())
            });
    if needs_backdrop_blur {
        let backdrop_logical = Size::<i32, Logical>::from((logical_size.w, logical_size.h));
        let _ = shape_cache.prepare_liquid_glass_backdrop(
            renderer,
            output_entry.id.0,
            backdrop_logical,
            output_scale,
            &backdrop_elements,
        );
    }
    prepare_minimize_snapshots_for_output(
        renderer,
        text_cache,
        shape_cache,
        state,
        output_loc,
        &output_windows,
        output_scale,
    );
    prepare_window_lifecycle_snapshots_for_output(
        renderer,
        text_cache,
        shape_cache,
        state,
        output_loc,
        &output_windows,
        focused_win,
        fullscreen_win,
        output_scale,
    );

    if is_primary
        && !state.screen_locked
        && let Some(osd) = state.osd.as_ref()
    {
        let osd_view = crate::shell_render::OsdRenderState {
            label: &osd.label,
            value_percent: osd.value_percent,
            muted: osd.muted,
        };
        render_list.extend(crate::shell_render::generate_osd_elements(
            text_cache,
            shape_cache,
            renderer,
            logical_size,
            &osd_view,
        ));
    }

    let dock_prepared = (is_primary || dock_visible_here).then(|| {
        let dock_tiles = state.build_dock_tiles();
        let presented_dock = state.sync_dock_presentation_for_output(output_entry.id, &dock_tiles);
        (dock_tiles, presented_dock)
    });

    let tray_views: Vec<TopBarTrayItem<'_>> = state
        .tray_items
        .iter()
        .take(MAX_TOP_BAR_TRAY_ITEMS)
        .map(|item| TopBarTrayItem {
            key: &item.key,
            label: &item.badge,
            icon_name: &item.icon_name,
            icon_pixmap: item.icon_pixmap.as_ref(),
            attention: item.status == crate::tray::TrayStatus::NeedsAttention,
            passive: item.status == crate::tray::TrayStatus::Passive,
        })
        .collect();
    let clock_label = if state.clock_24h {
        &tel.clock_date_24h
    } else {
        &tel.clock_date_hhmm
    };
    let (active_app_name, app_profile) = state.active_top_bar_app_context();
    let live_app_menu = state.active_app_menu_tree();
    let menu_on_this_output = state.top_bar_output.is_none_or(|id| id == output_entry.id);
    let active_top_bar_menu = if menu_on_this_output {
        state
            .active_top_bar_menu
            .filter(|m| *m != crate::shell_render::TopBarMenuHit::Go || app_profile.has_go_menu())
    } else {
        None
    };
    let bar_backdrop = state
        .user_config
        .menubar_adaptive_contrast
        .then(|| shape_cache.wallpaper_top_bar_backdrop())
        .flatten();
    let base_top_telemetry = TopBarTelemetry {
        clock_hhmm: clock_label,
        battery_percent: tel.power.percentage,
        has_battery: tel.power.has_battery,
        ac_online: tel.power.ac_online,
        battery_charging: tel.power.state == rusty_services::BatteryState::Charging,
        net_connected: tel.network.connected,
        wired_connected: tel.network.wired_connected,
        wifi_connected: tel.network.wifi_connected,
        wifi_enabled: tel.network.wifi_enabled,
        wifi_signal_percent: tel.network.wifi_signal_percent,
        volume_percent: tel.audio.volume_percent,
        audio_muted: tel.audio.muted,
        quick_settings_open: state.quick_settings_active && menu_on_this_output,
        notification_center_open: state.notification_center_open && menu_on_this_output,
        wifi_open: state.wifi_popover_open && menu_on_this_output,
        updates_open: state.updates_popover_open && menu_on_this_output,
        show_updates: state.should_show_top_bar_updates(),
        update_count: u32::try_from(state.updates_status.total_count).unwrap_or(u32::MAX),
        update_checking: state.updates_status.state == rusty_services::UpdateCheckState::Checking,
        rustyos_update_available: state.updates_status.rustyos_available,
        notification_count: state.notification_count,
        kbd_layout: &state.active_kbd_layout,
        tray_items: &tray_views,
        active_app_name: &active_app_name,
        app_profile,
        active_menu: active_top_bar_menu,
        menu_hover_row: active_top_bar_menu.and(state.top_bar_menu_hover_row),
        glass_opacity: state.glass_opacity,
        reduce_transparency: state.reduce_transparency,
        show_workspaces: state.menubar_show_workspaces,
        show_tray: state.menubar_show_tray,
        show_input_source: state.menubar_show_input_source,
        show_volume: state.menubar_show_volume,
        show_wifi: state.user_config.menubar_show_wifi,
        show_battery: state.user_config.menubar_show_battery,
        show_battery_percentage: state.user_config.menubar_show_battery_percentage,
        show_bluetooth: state.user_config.menubar_show_bluetooth,
        bluetooth_powered: tel.bluetooth.powered,
        mic_active: tel.device_usage.mic_active,
        camera_active: tel.device_usage.camera_active,
        recording_active: !state.active_screencasts.is_empty(),
        backdrop: bar_backdrop,
        quick_popover: menu_on_this_output
            .then_some(state.top_bar_quick_popover)
            .flatten(),
        app_menu_tree: live_app_menu.as_ref(),
    };

    if is_primary {
        if let Some((dock_tiles, presented_dock)) = dock_prepared {
            let top_telemetry = base_top_telemetry;

            let lock_hidden_preview = matches!(
                state.user_config.notifications_show_previews,
                crate::config::NotificationPreviewsMode::Never
            ) || (state.is_session_locked()
                && matches!(
                    state.user_config.notifications_show_previews,
                    crate::config::NotificationPreviewsMode::WhenUnlocked
                ));
            let suppress_toasts = state.focus_mode
                || !state.user_config.notifications_allow
                || (state.is_session_locked() && !state.user_config.notifications_when_locked);
            let toast_views: Vec<NotificationToastView<'_>> = if suppress_toasts {
                Vec::new()
            } else {
                state
                    .notifications
                    .iter()
                    .rev()
                    .take(MAX_VISIBLE_NOTIFICATIONS)
                    .enumerate()
                    .map(|(idx, n)| NotificationToastView {
                        id: n.id,
                        urgency: n.urgency,
                        app_key: n.app_key.as_deref(),
                        app_name: n.app_display_name.as_deref(),
                        title: &n.title,
                        body: if lock_hidden_preview {
                            "Notification preview hidden"
                        } else {
                            &n.body
                        },
                        hovered: state.hovered_notification_idx == Some(idx),
                        avatar: n.avatar.as_ref(),
                    })
                    .collect()
            };

            let launcher_progress = state.launcher_anim.current();
            let show_launcher = state.launcher_active || launcher_progress > 0.01;
            let qs_progress = state.quick_settings_anim.current();
            let show_qs = !show_launcher && (state.quick_settings_active || qs_progress > 0.01);
            let ov_progress = state.overview_anim.current();
            let show_ov =
                !show_launcher && !show_qs && (state.overview_active || ov_progress > 0.01);

            let dock_windows = collect_dock_windows(state, &visible_windows, focused_win, show_ov);
            let dock_bounces = state.dock_bounce_offsets();
            let dock_view = state.dock_view_state_for_output(
                output_entry.id,
                &dock_tiles,
                &dock_windows,
                &dock_bounces,
                Some(&presented_dock),
            );

            let switcher_cards: Vec<WindowSwitcherCard<'_>> =
                state.window_switcher.as_ref().map_or_default(|sw| {
                    sw.window_ids
                        .iter()
                        .filter_map(|&win_id| {
                            dock_windows
                                .iter()
                                .find(|entry| entry.id == win_id)
                                .map(|entry| WindowSwitcherCard {
                                    title: &entry.title,
                                    badge: &entry.badge,
                                    window_id: entry.id,
                                    icon_name: entry.icon_name.as_deref(),
                                    toplevel_icon: entry.toplevel_icon.as_ref(),
                                    minimized: entry.minimized,
                                })
                        })
                        .collect()
                });
            let switcher_view = state.window_switcher.as_ref().and_then(|sw| {
                (!switcher_cards.is_empty()).then_some(WindowSwitcherRenderState {
                    cards: &switcher_cards,
                    selected: sw.selected.min(switcher_cards.len() - 1),
                    hovered: sw.hovered.filter(|&i| i < switcher_cards.len()),
                    hovered_close: sw.hovered_close.filter(|&i| i < switcher_cards.len()),
                })
            });

            let (win_start, visible_items) = if show_launcher {
                state.launcher.visible_window(5)
            } else {
                (0, Vec::new())
            };
            let cursor_offset_x = if show_launcher && !state.launcher.query.is_empty() {
                text_cache.measure_text(&state.launcher.query, 14.5).0
            } else {
                0
            };
            let spotlight_view = show_launcher.then_some(SpotlightRenderState {
                query: &state.launcher.query,
                selected_in_window: state.launcher.selected_index.saturating_sub(win_start),
                cursor_offset_x,
                items: &visible_items,
                progress: launcher_progress,
            });

            let qs_view = show_qs.then(|| QuickSettingsRenderState {
                layout_mode: layout_str,
                reduced_motion: state.animations.reduced_motion(),
                network_label: &tel.network.status_label,
                net_connected: tel.network.connected,
                wifi_enabled: tel.network.wifi_enabled,
                wifi_connected: tel.network.wifi_connected,
                wifi_ssid: tel.network.wifi_ssid.as_deref(),
                wifi_signal_percent: tel.network.wifi_signal_percent,
                bt_available: tel.bluetooth.available,
                bt_powered: tel.bluetooth.powered,
                bt_label: &tel.bluetooth.status_label,
                battery_percent: tel.power.percentage,
                has_battery: tel.power.has_battery,
                ac_online: tel.power.ac_online,
                battery_charging: tel.power.state == rusty_services::BatteryState::Charging,
                brightness_available: tel.brightness.available,
                brightness_percent: tel.brightness.percentage,
                volume_percent: tel.audio.volume_percent,
                audio_muted: tel.audio.muted,
                audio_output_name: &tel.audio.active_output_name,
                media_active: tel.media.active,
                media_playing: tel.media.playing,
                media_title: &tel.media.title,
                focus_mode: state.focus_mode,
                frame_p50_us: state.frame_timing.interval_p50_us(),
                window_count,
                progress: qs_progress,
                glass_opacity: state.glass_opacity,
                reduce_transparency: state.reduce_transparency,
            });

            let overview_spaces = if show_ov {
                state.overview_workspace_spaces()
            } else {
                Vec::new()
            };
            let overview_filtered_windows: Option<Vec<DockWindowEntry>> =
                if show_ov && state.overview_filter_app.is_some() {
                    let allowed: std::collections::HashSet<_> =
                        state.overview_window_ids().into_iter().collect();
                    Some(
                        dock_windows
                            .iter()
                            .filter(|w| allowed.contains(&w.id))
                            .cloned()
                            .collect(),
                    )
                } else {
                    None
                };
            let overview_filter_label = show_ov
                .then_some(state.overview_filter_app.as_deref())
                .flatten()
                .map(|key| {
                    format!(
                        "App Expose: {}",
                        crate::shell_render::dock_label_for_key(key)
                    )
                });
            let ov_windows = overview_filtered_windows
                .as_deref()
                .unwrap_or(&dock_windows);
            #[allow(clippy::cast_possible_truncation)]
            let ov_dragging_card = state.overview_drag.as_ref().and_then(|drag| {
                drag.dragging.then(|| {
                    let dx = (drag.current_pos.x - drag.start_pos.x).round() as i32;
                    let dy = (drag.current_pos.y - drag.start_pos.y).round() as i32;
                    (drag.card_index, Point::from((dx, dy)))
                })
            });
            let ov_view = show_ov.then_some(OverviewRenderState {
                active_workspace: active_ws,
                layout_mode: layout_str,
                windows: ov_windows,
                spaces: &overview_spaces,
                progress: ov_progress,
                filter_label: overview_filter_label.as_deref(),
                hovered_card: state
                    .overview_hovered_card
                    .filter(|&i| i < ov_windows.len()),
                hovered_close: state
                    .overview_hovered_close
                    .filter(|&i| i < ov_windows.len()),
                hovered_space: state.overview_hovered_space,
                selected_card: state
                    .overview_selected_card
                    .filter(|&i| i < ov_windows.len()),
                drag_target_workspace: state
                    .overview_drag
                    .as_ref()
                    .filter(|d| d.dragging)
                    .and_then(|d| d.target_workspace),
                dragging_card: ov_dragging_card.filter(|(idx, _)| *idx < ov_windows.len()),
            });

            let polkit_display = state
                .polkit_prompt
                .as_ref()
                .map(|pk| state.polkit_password.visible_or_masked(pk.echo_on, 48));
            let polkit_view = state
                .polkit_prompt
                .as_ref()
                .zip(polkit_display.as_deref())
                .map(|(pk, display)| PolkitPromptRenderState {
                    message: &pk.message,
                    action_id: &pk.action_id,
                    identity_user: &pk.identity_user,
                    prompt_label: &pk.prompt_label,
                    input_display: display,
                    checking: pk.checking,
                    error_message: pk.error_message.as_deref(),
                    info_message: pk.info_message.as_deref(),
                });

            let portal_rows: Vec<PortalSourceRow<'_>> =
                state.portal_prompt.as_ref().map_or_default(|p| {
                    p.candidates
                        .iter()
                        .map(|c| PortalSourceRow {
                            label: &c.label,
                            detail: &c.detail,
                            is_window: c.is_window,
                        })
                        .collect()
                });
            let portal_view = state
                .portal_prompt
                .as_ref()
                .map(|p| PortalPromptRenderState {
                    app_id: &p.app_id,
                    is_screencast: p.is_screencast,
                    include_cursor: p.include_cursor,
                    cursor_mode_editable: p.cursor_mode_editable,
                    rows: &portal_rows,
                    selected: p.selected,
                });

            let context_menu_view = state
                .context_menu
                .as_ref()
                .filter(|ctx| ctx.output_id == output_entry.id);
            let dock_stack_view = state.dock_stack_render_state_for_output(output_entry.id);
            let updates_view = state
                .updates_popover_open
                .then(|| state.updates_render_state(logical_size.w));
            let wifi_view = state
                .wifi_popover_open
                .then(|| state.wifi_render_state(logical_size.w, &tel.network));
            let quick_pop_view = state
                .top_bar_quick_popover
                .map(|kind| state.top_bar_quick_popover_render_state(logical_size.w, kind, &tel));
            let nc_items = if state.notification_center_open {
                state.build_notification_center_items()
            } else {
                Vec::new()
            };
            let nc_view = state
                .notification_center_open
                .then(|| state.notification_center_render_state(&nc_items, lock_hidden_preview));

            let overlay = if let Some(pk) = polkit_view.as_ref() {
                ShellOverlayView::Polkit(pk)
            } else if let Some(pt) = portal_view.as_ref() {
                ShellOverlayView::Portal(pt)
            } else if let Some(sw) = switcher_view.as_ref() {
                ShellOverlayView::Switcher(sw)
            } else if let Some(spot) = spotlight_view.as_ref() {
                ShellOverlayView::Spotlight(spot)
            } else if let Some(qs) = qs_view.as_ref() {
                ShellOverlayView::QuickSettings(qs)
            } else if let Some(up) = updates_view.as_ref() {
                ShellOverlayView::Updates(up)
            } else if let Some(wifi) = wifi_view.as_ref() {
                ShellOverlayView::Wifi(wifi)
            } else if let Some(qpop) = quick_pop_view.as_ref() {
                ShellOverlayView::TopBarQuickPopover(qpop)
            } else if let Some(nc) = nc_view.as_ref() {
                ShellOverlayView::NotificationCenter(nc)
            } else if let Some(ctx) = context_menu_view {
                ShellOverlayView::ContextMenu(ctx)
            } else if let Some(stack) = dock_stack_view.as_ref() {
                ShellOverlayView::DockStack(stack)
            } else if let Some(ov) = ov_view.as_ref() {
                ShellOverlayView::Overview(ov)
            } else if let Some(pick) = state.portal_pick_color.as_ref() {
                ShellOverlayView::PickColor(&pick.app_id)
            } else if let Some(hud) = state.screenshot_hud.as_ref() {
                ShellOverlayView::ScreenshotHud(hud)
            } else {
                ShellOverlayView::None
            };

            if let ShellOverlayView::Switcher(sw) = overlay {
                let sw_ids: Vec<rusty_wm::WindowId> =
                    sw.cards.iter().map(|c| c.window_id).collect();
                capture_window_thumbnails(
                    renderer,
                    text_cache,
                    shape_cache,
                    state,
                    output_loc,
                    &output_windows,
                    focused_win,
                    output_scale,
                    &sw_ids,
                );
            }
            let overlay_elements = crate::shell_render::generate_shell_foreground_overlay_elements(
                text_cache,
                shape_cache,
                renderer,
                logical_size,
                &top_telemetry,
                overlay,
                &toast_views,
            );
            let chrome_elements = if !top_layer_surfaces_present && !hide_chrome_for_fullscreen {
                crate::shell_render::generate_shell_chrome_composite_elements(
                    text_cache,
                    shape_cache,
                    renderer,
                    logical_size,
                    active_ws,
                    layout_str,
                    &dock_view,
                    &top_telemetry,
                    overlay,
                )
            } else {
                Vec::new()
            };

            let mut popup_elements = Vec::new();
            push_xwayland_override_redirect_elements(
                &mut popup_elements,
                renderer,
                state,
                output_geo,
                output_scale,
            );
            push_client_popup_elements(
                &mut popup_elements,
                renderer,
                shape_cache,
                state,
                output_loc,
                output_scale,
                &output_windows,
                fullscreen_win,
            );
            render_list.extend(overlay_elements);
            render_list.extend(top_layer_elements);
            render_list.extend(popup_elements);
            render_list.extend(chrome_elements);

            let overview_overlay = if matches!(overlay, ShellOverlayView::ContextMenu(_))
                && let Some(ov) = ov_view.as_ref()
            {
                ShellOverlayView::Overview(ov)
            } else {
                overlay
            };
            if let ShellOverlayView::Overview(ov) = overview_overlay {
                let ov_ids: Vec<rusty_wm::WindowId> = ov.windows.iter().map(|w| w.id).collect();
                capture_window_thumbnails(
                    renderer,
                    text_cache,
                    shape_cache,
                    state,
                    output_loc,
                    &output_windows,
                    focused_win,
                    output_scale,
                    &ov_ids,
                );
            }
            render_list.extend(crate::shell_render::generate_shell_overview_elements(
                text_cache,
                shape_cache,
                renderer,
                logical_size,
                &top_telemetry,
                overview_overlay,
            ));
        }
    } else {
        let secondary_dock_prepared = dock_prepared;
        let secondary_context_menu = (!top_layer_surfaces_present
            && fullscreen_win.is_none()
            && !state.greeter_mode
            && !state.should_hide_shell_chrome_for_fullscreen_on(output_entry.id)
            && !state.is_session_locked()
            && state.polkit_prompt.is_none()
            && state.portal_prompt.is_none())
        .then(|| {
            state
                .context_menu
                .as_ref()
                .filter(|ctx| ctx.output_id == output_entry.id)
        })
        .flatten();
        let secondary_stack = (!top_layer_surfaces_present
            && fullscreen_win.is_none()
            && !state.greeter_mode
            && !state.should_hide_shell_chrome_for_fullscreen_on(output_entry.id)
            && !state.is_session_locked()
            && state.polkit_prompt.is_none()
            && state.portal_prompt.is_none())
        .then(|| state.dock_stack_render_state_for_output(output_entry.id))
        .flatten();
        if let Some(ctx) = secondary_context_menu {
            render_list.extend(crate::shell_render::generate_context_menu_elements(
                text_cache,
                shape_cache,
                renderer,
                logical_size,
                state.glass_opacity,
                state.reduce_transparency,
                ctx,
                false,
            ));
        } else if let Some(ref stack) = secondary_stack {
            let secondary_telemetry = TopBarTelemetry {
                glass_opacity: state.glass_opacity,
                reduce_transparency: state.reduce_transparency,
                ..TopBarTelemetry::default()
            };
            render_list.extend(
                crate::shell_render::generate_shell_foreground_overlay_elements(
                    text_cache,
                    shape_cache,
                    renderer,
                    logical_size,
                    &secondary_telemetry,
                    ShellOverlayView::DockStack(stack),
                    &[],
                ),
            );
        }
        render_list.extend(top_layer_elements);
        push_xwayland_override_redirect_elements(
            &mut render_list,
            renderer,
            state,
            output_geo,
            output_scale,
        );
        push_client_popup_elements(
            &mut render_list,
            renderer,
            shape_cache,
            state,
            output_loc,
            output_scale,
            &output_windows,
            fullscreen_win,
        );
        let secondary_chrome_allowed = !top_layer_surfaces_present
            && fullscreen_win.is_none()
            && !state.greeter_mode
            && !state.should_hide_shell_chrome_for_fullscreen_on(output_entry.id)
            && !state.is_session_locked()
            && state.polkit_prompt.is_none()
            && state.portal_prompt.is_none();
        if secondary_chrome_allowed {
            let dock_bounces = state.dock_bounce_offsets();
            let dock_view_owned =
                secondary_dock_prepared
                    .as_ref()
                    .map(|(dock_tiles, presented_dock)| {
                        state.dock_view_state_for_output(
                            output_entry.id,
                            dock_tiles,
                            &[],
                            &dock_bounces,
                            Some(presented_dock),
                        )
                    });
            let secondary_overlay = if let Some(ctx) = secondary_context_menu {
                ShellOverlayView::ContextMenu(ctx)
            } else if let Some(ref stack) = secondary_stack {
                ShellOverlayView::DockStack(stack)
            } else {
                ShellOverlayView::None
            };
            render_list.extend(
                crate::shell_render::generate_secondary_shell_chrome_elements(
                    text_cache,
                    shape_cache,
                    renderer,
                    logical_size,
                    active_ws,
                    layout_str,
                    dock_view_owned.as_ref(),
                    &base_top_telemetry,
                    secondary_overlay,
                ),
            );
        }
    }

    shape_cache.clear_active_liquid_glass_backdrop();
    let mut final_list = scale_render_elements_for_output(render_list, output_scale);
    if fullscreen_win.is_none()
        && (!state.minimize_anims.is_empty() || !state.lifecycle_anims.is_empty())
    {
        let mut warp_elements = Vec::new();
        if !state.lifecycle_anims.is_empty() {
            push_lifecycle_warp_elements(
                &mut warp_elements,
                renderer,
                shape_cache,
                state,
                output_geo,
                output_scale,
            );
        }
        if !state.minimize_anims.is_empty() {
            push_minimize_warp_elements(
                &mut warp_elements,
                renderer,
                shape_cache,
                state,
                output_loc,
                output_scale,
                &output_windows,
            );
        }
        final_list.extend(scale_render_elements_for_output(
            warp_elements,
            output_scale,
        ));
    }
    final_list.extend(backdrop_elements);
    final_list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_chrome_for_uncommitted_bound_client_until_buffer_commit() {
        assert!(should_skip_uncommitted_bound_window(false, None));
        assert!(should_skip_uncommitted_bound_window(
            false,
            Some(Size::from((0, 0)))
        ));
        assert!(should_skip_uncommitted_bound_window(
            false,
            Some(Size::from((640, 0)))
        ));
        assert!(!should_skip_uncommitted_bound_window(
            false,
            Some(Size::from((640, 500)))
        ));
        assert!(!should_skip_uncommitted_bound_window(
            true,
            Some(Size::from((0, 0)))
        ));
    }

    #[test]
    fn pointer_local_maps_coordinates_only_for_matching_output() {
        let out1 = Rectangle::new((0, 0).into(), (1920, 1080).into());
        let out2 = Rectangle::new((1920, 0).into(), (2560, 1440).into());

        // Pointer inside monitor 2 maps to monitor-local origin and stays out of monitor 1 beyond margin.
        assert_eq!(
            pointer_local_if_on_output((2500.0, 720.0).into(), out2),
            Some(Point::from((580, 720)))
        );
        assert_eq!(
            pointer_local_if_on_output((2500.0, 720.0).into(), out1),
            None
        );

        // Near monitor boundary within 64px cursor margin maps cleanly into both during transitions.
        assert_eq!(
            pointer_local_if_on_output((1930.0, 400.0).into(), out1),
            Some(Point::from((1930, 400)))
        );
        assert_eq!(
            pointer_local_if_on_output((1930.0, 400.0).into(), out2),
            Some(Point::from((10, 400)))
        );
    }

    #[test]
    fn split_shell_words_preserves_quoted_greeter_session_cmd() {
        let parts = split_shell_words(
            "/usr/bin/rusty-greeter --session-cmd \"/usr/bin/rustyos-session --drm\"",
        );
        assert_eq!(
            parts,
            vec![
                "/usr/bin/rusty-greeter",
                "--session-cmd",
                "/usr/bin/rustyos-session --drm"
            ]
        );
    }
}
