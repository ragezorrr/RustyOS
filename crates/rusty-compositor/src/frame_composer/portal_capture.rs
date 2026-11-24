//! Screenshots, XDG Portal color-picker/screencast capture, cursor surface, and client launchers.

use std::process::Command;
use std::time::{Duration, Instant};

use smithay::{
    backend::renderer::{
        element::Element,
        element::Kind,
        element::surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        gles::GlesRenderer,
    },
    input::pointer::CursorImageSurfaceData,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Size},
    wayland::compositor,
};
use tracing::{info, warn};

use super::{CursorRenderMode, build_frame_render_list_for_output_with_cursor};
use crate::{shell_render::CompositorRenderElement, state::RustyCompositorState};

pub(crate) fn process_pending_screenshot(
    renderer: &mut GlesRenderer,
    render_list: &[CompositorRenderElement],
    state: &mut RustyCompositorState,
    fallback_size: smithay::utils::Size<i32, smithay::utils::Physical>,
) {
    let Some(req) = state.pending_screenshot.take() else {
        return;
    };
    let primary = state.output_map.primary();
    let output_scale = if primary.scale > 0.1 {
        primary.scale
    } else {
        1.0
    };
    let primary_size = primary.transformed_physical_size();
    let capture_size = if primary_size.w > 0 && primary_size.h > 0 {
        primary_size
    } else {
        fallback_size
    };
    let (crop, copy_only) = match req {
        crate::screenshot::ScreenshotRequest::FullScreen => (None, false),
        crate::screenshot::ScreenshotRequest::FullScreenCopyOnly => (None, true),
        crate::screenshot::ScreenshotRequest::FocusedWindow => (
            state
                .wm
                .focused_window()
                .and_then(|id| state.wm.window(id))
                .map(|w| w.geometry),
            false,
        ),
        crate::screenshot::ScreenshotRequest::Region { rect, copy_only } => (Some(rect), copy_only),
    };
    match crate::screenshot::capture_render_list_rgba(
        renderer,
        render_list,
        capture_size,
        output_scale,
        crop,
    ) {
        Ok((width, height, rgba)) => {
            if let Ok(dest) = std::env::var("RUSTYOS_SCREENSHOT_DEST") {
                if let Ok(bytes) = crate::screenshot::encode_png_rgba(width, height, &rgba) {
                    let _ = std::fs::write(&dest, bytes);
                }
            } else if copy_only {
                crate::screenshot::spawn_copy_screenshot_only(
                    width,
                    height,
                    rgba,
                    state.socket_name.clone(),
                    &state.screenshot_results,
                );
            } else {
                crate::screenshot::spawn_save_screenshot(
                    width,
                    height,
                    rgba,
                    state.socket_name.clone(),
                    &state.screenshot_results,
                );
            }
        }
        Err(err) => {
            warn!(error = %err, "Failed to capture compositor screenshot");
            state.show_osd("Screenshot Failed", None, false);
        }
    }
}

/// Executes pending XDG Portal screenshots, `PickColor` samples, and paced PipeWire `ScreenCast` frames (`RUSTYOS-057`).
pub(crate) fn process_portal_captures_and_streams(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    capture_cache: &mut crate::screenshot::OffscreenCaptureBuffer,
    state: &mut RustyCompositorState,
) {
    if state.is_session_locked() {
        return;
    }

    if let Some(pending) = state.pending_portal_capture.take() {
        match pending {
            crate::state::PendingPortalCapture::Screenshot {
                request_id,
                target,
                output_id,
                window_id,
            } => capture_portal_screenshot(
                renderer,
                text_cache,
                shape_cache,
                capture_cache,
                state,
                request_id,
                target,
                output_id,
                window_id,
            ),
            crate::state::PendingPortalCapture::PickColorSample {
                request_id,
                pointer_global,
            } => capture_portal_pick_color(
                renderer,
                text_cache,
                shape_cache,
                capture_cache,
                state,
                request_id,
                pointer_global,
            ),
        }
    }

    pump_active_screencasts(renderer, text_cache, shape_cache, capture_cache, state);
}

#[allow(clippy::too_many_arguments)]
fn capture_portal_screenshot(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    capture_cache: &mut crate::screenshot::OffscreenCaptureBuffer,
    state: &mut RustyCompositorState,
    request_id: u64,
    target: u32,
    output_id: Option<rusty_wm::OutputId>,
    window_id: Option<rusty_wm::WindowId>,
) {
    let cap_target = if let Some(wid) = window_id
        && state.wm.window(wid).is_some()
    {
        crate::state::ScreencastCaptureTarget::Window(wid)
    } else if let Some(oid) = output_id
        && state.output_map.get(oid).is_some()
    {
        crate::state::ScreencastCaptureTarget::Output(oid)
    } else if (target & rusty_services::SCREENSHOT_TARGET_WINDOW) != 0
        && let Some(fid) = state.wm.focused_window()
    {
        crate::state::ScreencastCaptureTarget::Window(fid)
    } else {
        crate::state::ScreencastCaptureTarget::Output(state.output_map.primary().id)
    };

    let resolved = state.resolve_portal_capture_target(cap_target);
    let snap_list = build_frame_render_list_for_output_with_cursor(
        renderer,
        text_cache,
        shape_cache,
        state,
        resolved.output_id,
        resolved.capture_size,
        CursorRenderMode::None,
    );
    match capture_cache.capture_rgba(
        renderer,
        &snap_list,
        resolved.capture_size,
        resolved.output_scale,
        resolved.crop,
    ) {
        Ok((width, height, rgba)) => {
            crate::screenshot::spawn_save_portal_screenshot(
                request_id,
                width,
                height,
                rgba,
                state.socket_name.clone(),
                state.portal.reply_sender(),
            );
        }
        Err(err) => {
            state
                .portal
                .send_reply(rusty_services::PortalReply::ScreenshotResult {
                    request_id,
                    result: Err(err),
                });
        }
    }
}

fn capture_portal_pick_color(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    capture_cache: &mut crate::screenshot::OffscreenCaptureBuffer,
    state: &mut RustyCompositorState,
    request_id: u64,
    pointer_global: smithay::utils::Point<f64, smithay::utils::Logical>,
) {
    let out_id = state
        .output_under(pointer_global)
        .unwrap_or_else(|| state.output_map.primary().id);
    let entry = state
        .output_map
        .get(out_id)
        .cloned()
        .unwrap_or_else(|| state.output_map.primary().clone());
    let capture_size = entry.transformed_physical_size();
    let snap_list = build_frame_render_list_for_output_with_cursor(
        renderer,
        text_cache,
        shape_cache,
        state,
        entry.id,
        capture_size,
        CursorRenderMode::None,
    );
    let result =
        match capture_cache.capture_rgba(renderer, &snap_list, capture_size, entry.scale, None) {
            Ok((width, height, rgba)) => {
                let local_x = pointer_global.x - f64::from(entry.location.x);
                let local_y = pointer_global.y - f64::from(entry.location.y);
                let (px, py) = crate::screenshot::logical_point_to_buffer_pixel(
                    local_x,
                    local_y,
                    entry.scale,
                    width,
                    height,
                );
                crate::screenshot::sample_rgba_pixel(width, height, &rgba, px, py)
            }
            Err(err) => Err(err),
        };
    state
        .portal
        .send_reply(rusty_services::PortalReply::PickColorResult { request_id, result });
}

fn pump_active_screencasts(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    capture_cache: &mut crate::screenshot::OffscreenCaptureBuffer,
    state: &mut RustyCompositorState,
) {
    let now = Instant::now();
    let due: Vec<(usize, crate::state::ScreencastCaptureTarget, bool)> = state
        .active_screencasts
        .iter()
        .enumerate()
        .filter(|(_, sess)| {
            let interval = Duration::from_secs_f64(1.0 / f64::from(sess.fps.clamp(15, 120)));
            sess.stream.is_alive()
                && (!sess.pushed_initial_frame
                    || (sess.stream.is_consumer_active()
                        && now.duration_since(sess.last_pushed_at) >= interval))
        })
        .map(|(idx, sess)| (idx, sess.target, sess.include_cursor))
        .collect();

    for (idx, target, include_cursor) in due {
        let resolved = state.resolve_portal_capture_target(target);
        let cursor_mode = if include_cursor {
            CursorRenderMode::SoftwareComposited
        } else {
            CursorRenderMode::None
        };
        let render_list = build_frame_render_list_for_output_with_cursor(
            renderer,
            text_cache,
            shape_cache,
            state,
            resolved.output_id,
            resolved.capture_size,
            cursor_mode,
        );
        if let Ok((width, height, rgba)) = capture_cache.capture_rgba(
            renderer,
            &render_list,
            resolved.capture_size,
            resolved.output_scale,
            resolved.crop,
        ) && let Some(sess) = state.active_screencasts.get_mut(idx)
        {
            let _ = sess.stream.push_frame(width, height, rgba);
            sess.last_pushed_at = now;
            sess.pushed_initial_frame = true;
        }
    }
}

pub(crate) fn push_client_surface_cursor(
    render_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    surface: &WlSurface,
    pointer_loc: Point<i32, Logical>,
    output_scale: f64,
    prefer_hw_cursor: bool,
    cursor_plane_limit: Option<smithay::utils::Size<u32, smithay::utils::Buffer>>,
) {
    let hotspot = compositor::with_states(surface, |states| {
        states
            .data_map
            .get::<CursorImageSurfaceData>()
            .and_then(|data| data.lock().ok())
            .map(|attrs| attrs.hotspot)
    })
    .unwrap_or_default();
    let loc = (pointer_loc - hotspot).to_physical_precise_round(output_scale);
    let scale = smithay::utils::Scale::from(output_scale);
    let initial_kind = if prefer_hw_cursor {
        Kind::Cursor
    } else {
        Kind::Unspecified
    };
    let mut elements = render_elements_from_surface_tree::<
        GlesRenderer,
        WaylandSurfaceRenderElement<GlesRenderer>,
    >(renderer, surface, loc, scale, 1.0, initial_kind);

    if prefer_hw_cursor
        && cursor_plane_limit.is_some_and(|limit| {
            elements.len() > 1
                || elements.iter().any(|el| {
                    let sz = el.geometry(scale).size;
                    let w = u32::try_from(sz.w.max(0)).unwrap_or(u32::MAX);
                    let h = u32::try_from(sz.h.max(0)).unwrap_or(u32::MAX);
                    w == 0 || h == 0 || w > limit.w || h > limit.h
                })
        })
    {
        elements = render_elements_from_surface_tree::<
            GlesRenderer,
            WaylandSurfaceRenderElement<GlesRenderer>,
        >(renderer, surface, loc, scale, 1.0, Kind::Unspecified);
    }

    render_list.extend(elements.into_iter().map(CompositorRenderElement::Surface));
}

pub(crate) fn spawn_test_client(
    command: &str,
    socket_name: &str,
    ipc_socket_path: &std::path::Path,
) -> Option<std::process::Child> {
    let parts = split_shell_words(command);
    let (program, args) = parts.split_first()?;

    match Command::new(program)
        .args(args)
        .env("WAYLAND_DISPLAY", socket_name)
        .env("RUSTYOS_IPC_SOCKET", ipc_socket_path)
        .spawn()
    {
        Ok(child) => {
            info!(
                program,
                pid = child.id(),
                wayland_display = socket_name,
                "Spawned child Wayland client"
            );
            Some(child)
        }
        Err(err) => {
            warn!(program, error = %err, "Failed to spawn child Wayland client");
            None
        }
    }
}

/// Splits a simple shell command string supporting single and double quotes (`RUSTYOS-157`).
#[must_use]
pub(crate) fn split_shell_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for ch in input.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            c if c.is_whitespace() && !in_single && !in_double => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Spawns a `greetd` greeter Wayland client and returns its handle so the compositor
/// can exit cleanly once the greeter finishes authentication (`RUSTYOS-157`).
pub(crate) fn spawn_greeter_client(
    command: &str,
    socket_name: &str,
    ipc_socket_path: &std::path::Path,
) -> Option<std::process::Child> {
    let parts = split_shell_words(command);
    let (program, args) = parts.split_first()?;
    match Command::new(program)
        .args(args)
        .env("WAYLAND_DISPLAY", socket_name)
        .env("RUSTYOS_IPC_SOCKET", ipc_socket_path)
        .spawn()
    {
        Ok(child) => {
            info!(
                program,
                pid = child.id(),
                wayland_display = socket_name,
                "Spawned greetd greeter client"
            );
            Some(child)
        }
        Err(err) => {
            warn!(program, error = %err, "Failed to spawn greetd greeter client");
            None
        }
    }
}

/// Best-effort handoff from Plymouth boot splash (`--retain-splash`) once DRM is active (`RUSTYOS-157`).
pub(crate) fn quit_plymouth_retain_splash() -> Option<std::process::Child> {
    if !std::path::Path::new("/run/plymouth/pid").exists() {
        return None;
    }
    Command::new("plymouth")
        .args(["quit", "--retain-splash"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

/// Returns `true` when a bound client window has neither render elements nor a non-zero
/// committed surface size yet, suppressing orphan SSD/shadow chrome before first commit (`RUSTYOS-112`).
#[must_use]
pub(super) const fn should_skip_uncommitted_bound_window(
    has_surface_elems: bool,
    committed_size: Option<Size<i32, Logical>>,
) -> bool {
    !has_surface_elems
        && match committed_size {
            None => true,
            Some(sz) => sz.w <= 0 || sz.h <= 0,
        }
}
