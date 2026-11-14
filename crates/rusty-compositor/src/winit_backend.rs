//! Nested Winit backend runner (`RUSTYOS-002`).

use std::time::{Duration, Instant};

use rusty_ipc::{IpcServer, default_ipc_socket_path};
use smithay::{
    backend::{
        renderer::{
            ImportDma, damage::OutputDamageTracker, element::AsRenderElements, gles::GlesRenderer,
        },
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    desktop::utils::send_frames_surface_tree,
    input::pointer::{CursorIcon, CursorImageStatus},
    reexports::{
        calloop::{
            EventLoop,
            timer::{TimeoutAction, Timer},
        },
        wayland_protocols::wp::presentation_time::server::wp_presentation_feedback,
        wayland_server::Display,
    },
    utils::{IsAlive, Point, Rectangle},
    wayland::{presentation::Refresh, socket::ListeningSocketSource},
};
use tracing::{info, warn};

use crate::{
    BackendMode, BootstrapReport, CompositorConfig, CompositorError, frame_composer,
    state::{PRIMARY_OUTPUT_ID, RustyCompositorState},
};

/// Runs the nested Smithay + Winit compositor event loop.
///
/// # Errors
///
/// Returns [`CompositorError`] if the event loop, Wayland display, listening socket,
/// or Winit EGL/GLES backend cannot be initialized.
#[allow(clippy::too_many_lines)]
pub fn run_nested_winit(config: &CompositorConfig) -> Result<BootstrapReport, CompositorError> {
    let mut event_loop: EventLoop<'static, RustyCompositorState> =
        EventLoop::try_new().map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let mut display: Display<RustyCompositorState> =
        Display::new().map_err(|err| CompositorError::WaylandDisplay(err.to_string()))?;

    let listening_socket = ListeningSocketSource::new_auto()
        .map_err(|err| CompositorError::WaylandSocket(err.to_string()))?;
    let socket_name = listening_socket
        .socket_name()
        .to_string_lossy()
        .into_owned();
    let (mut backend, winit_evt_loop) = winit::init::<GlesRenderer>()
        .map_err(|err| CompositorError::BackendInit(err.to_string()))?;

    let initial_size = backend.window_size();
    let mut state = RustyCompositorState::new(
        &display,
        socket_name.clone(),
        event_loop.get_signal(),
        event_loop.handle(),
        initial_size,
    )?;
    state.config_path = crate::config::default_user_config_path();
    state.rotation_override = config.rotation_override;
    let winit_rotation = state
        .user_config
        .rotation_for_output("winit-0", config.rotation_override);
    state.set_output_rotation(PRIMARY_OUTPUT_ID, winit_rotation, true);
    let dmabuf_formats = backend.renderer().dmabuf_formats();
    state.init_dmabuf_formats(dmabuf_formats);
    state
        .animations
        .set_reduced_motion(state.user_config.reduced_motion || config.reduced_motion);
    state.start_background_services();
    let mut damage_tracker = OutputDamageTracker::from_output(&state.output);
    let mut text_cache = crate::text::TextCache::new();
    let mut shape_cache = crate::surfaces::UiShapeCache::new();
    let mut portal_capture_cache = crate::screenshot::OffscreenCaptureBuffer::default();
    let loop_handle = event_loop.handle();
    let _xwayland_started = crate::xwayland::spawn_xwayland_source(&display.handle(), &loop_handle);

    // 1. Register Wayland client connection listener.
    loop_handle
        .insert_source(listening_socket, |client_stream, _metadata, app_state| {
            info!("Accepted new Wayland client connection");
            app_state.insert_client_stream(client_stream);
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    // 2. Register Winit window/input event source.
    loop_handle
        .insert_source(winit_evt_loop, move |event, _metadata, app_state| {
            handle_winit_event(event, app_state);
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    // 3. Periodic 60 Hz frame tick (~16.6 ms) compensated for render duration so the
    // end-to-end presentation cadence stays aligned with the 16.67 ms budget.
    let max_frames = config.max_frames;
    let bench_input = config.bench_input;
    let force_full_damage = config.force_full_damage;
    let mut rendered_frames: u64 = 0;
    let mut last_winit_cursor: Option<WinitCursorSync> = None;

    loop_handle
        .insert_source(
            Timer::from_duration(Duration::from_micros(16_667)),
            move |_deadline, _metadata, app_state| {
                if !app_state.running {
                    return TimeoutAction::Drop;
                }

                if bench_input {
                    inject_bench_pointer_step(app_state, rendered_frames);
                }

                sync_winit_window_cursor(
                    &backend,
                    &app_state.cursor_status,
                    &mut last_winit_cursor,
                );

                let tick_start = Instant::now();
                if let Err(err) = render_winit_frame(
                    &mut backend,
                    &mut damage_tracker,
                    &mut text_cache,
                    &mut shape_cache,
                    &mut portal_capture_cache,
                    app_state,
                    force_full_damage,
                    rendered_frames,
                ) {
                    warn!(error = %err, "Frame render error on Winit backend");
                }

                rendered_frames = rendered_frames.saturating_add(1);
                if let Some(limit) = max_frames
                    && rendered_frames >= limit
                {
                    info!(
                        rendered_frames,
                        "Reached configured --max-frames limit; stopping"
                    );
                    app_state.request_shutdown();
                    return TimeoutAction::Drop;
                }

                let elapsed = tick_start.elapsed();
                let next_wait = Duration::from_micros(16_667)
                    .checked_sub(elapsed)
                    .unwrap_or(Duration::from_micros(500));
                TimeoutAction::ToDuration(next_wait)
            },
        )
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let ipc_socket_path = default_ipc_socket_path(&socket_name);
    let mut ipc_server = IpcServer::bind(ipc_socket_path.clone()).ok();

    info!(
        wayland_display = %socket_name,
        ipc_socket = %ipc_socket_path.display(),
        width = initial_size.w,
        height = initial_size.h,
        "RustyOS nested Winit compositor listening"
    );

    let autostart = state.user_config.autostart.clone();
    for cmd in autostart.iter().chain(config.spawn_clients.iter()) {
        frame_composer::spawn_test_client(cmd, &socket_name, &ipc_socket_path);
    }

    event_loop
        .run(Some(Duration::from_millis(4)), &mut state, |app_state| {
            frame_composer::dispatch_and_refresh_clients(
                &mut display,
                ipc_server.as_mut(),
                app_state,
            );
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let timing_summary = frame_composer::finalize_frame_timing(
        &state,
        BackendMode::NestedWinit,
        initial_size,
        config.metrics_out.as_deref(),
    )?;

    Ok(BootstrapReport {
        version: env!("CARGO_PKG_VERSION"),
        backend: BackendMode::NestedWinit,
        socket_name: Some(socket_name),
        frame_timing: Some(timing_summary),
        drm_report: None,
    })
}

fn inject_bench_pointer_step(app_state: &mut RustyCompositorState, rendered_frames: u64) {
    let step = rendered_frames % 240;
    let x = 120.0 + f64::from(u32::try_from(step).unwrap_or(0)) * 4.0;
    let y = 120.0 + f64::from(u32::try_from(step % 120).unwrap_or(0)) * 3.0;
    let time_msec = u32::try_from(app_state.elapsed().as_millis()).unwrap_or(0);
    app_state.inject_synthetic_pointer_motion((x, y).into(), time_msec);
}

fn handle_winit_event(event: WinitEvent, app_state: &mut RustyCompositorState) {
    match event {
        WinitEvent::Resized { size, scale_factor } => {
            let effective_scale = if app_state.user_config.has_explicit_output_scale("winit-0")
                || (app_state.user_config.display_scale - 1.0).abs() > 1e-4
            {
                app_state.user_config.scale_for_output("winit-0")
            } else {
                scale_factor
            };
            app_state.update_output_geometry(PRIMARY_OUTPUT_ID, size, effective_scale, 60_000);
        }
        WinitEvent::CloseRequested => {
            info!("Winit window close requested");
            app_state.request_shutdown();
        }
        WinitEvent::Input(input_event) => {
            app_state.process_input_event(input_event);
        }
        WinitEvent::Focus(_) | WinitEvent::Redraw => {}
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn render_winit_frame(
    backend: &mut WinitGraphicsBackend<GlesRenderer>,
    damage_tracker: &mut OutputDamageTracker,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    portal_capture_cache: &mut crate::screenshot::OffscreenCaptureBuffer,
    state: &mut RustyCompositorState,
    force_full_damage: bool,
    frame_seq: u64,
) -> Result<(), String> {
    let render_start = Instant::now();
    state.tick_animations();
    state.expire_notifications();
    let size = backend.window_size();
    let full_damage = Rectangle::from_size(size);
    let total_output_pixels =
        u64::try_from(size.w.max(0)).unwrap_or(0) * u64::try_from(size.h.max(0)).unwrap_or(0);
    let damage_rects: u32;
    let damaged_pixels: u64;
    let tracked_damage: Option<Vec<Rectangle<i32, smithay::utils::Physical>>>;
    let render_states;

    if state.pending_screenshot.is_some() {
        let renderer = backend.renderer();
        let snap_list = frame_composer::build_frame_render_list(
            renderer,
            text_cache,
            shape_cache,
            state,
            size,
            false,
        );
        frame_composer::process_pending_screenshot(renderer, &snap_list, state, size);
    }

    if state.pending_portal_capture.is_some() || state.has_due_screencast_frame(Instant::now()) {
        let renderer = backend.renderer();
        frame_composer::process_portal_captures_and_streams(
            renderer,
            text_cache,
            shape_cache,
            portal_capture_cache,
            state,
        );
    }

    {
        let buffer_age = if force_full_damage {
            0
        } else {
            backend.buffer_age().unwrap_or(0)
        };
        let (renderer, mut framebuffer) = backend.bind().map_err(|err| err.to_string())?;
        let mut render_list = frame_composer::build_frame_render_list(
            renderer,
            text_cache,
            shape_cache,
            state,
            size,
            false,
        );
        if let CursorImageStatus::Surface(ref cursor_surf) = state.cursor_status
            && cursor_surf.alive()
        {
            #[allow(clippy::cast_possible_truncation)]
            let ptr = Point::from((
                state.pointer_location.x.round() as i32,
                state.pointer_location.y.round() as i32,
            ));
            let primary_scale = state.output_map.primary().scale;
            let mut cursor_elems = Vec::new();
            frame_composer::push_client_surface_cursor(
                &mut cursor_elems,
                renderer,
                cursor_surf,
                ptr,
                primary_scale,
                false,
                None,
            );
            if !cursor_elems.is_empty() {
                cursor_elems.extend(render_list);
                render_list = cursor_elems;
            }
        }

        let render_result = damage_tracker
            .render_output(
                renderer,
                &mut framebuffer,
                buffer_age,
                &render_list,
                frame_composer::BACKGROUND_COLOR,
            )
            .map_err(|err| err.to_string())?;
        render_states = render_result.states;

        if force_full_damage {
            damage_rects = 1;
            damaged_pixels = total_output_pixels;
            tracked_damage = Some(vec![full_damage]);
        } else if let Some(rects) = render_result.damage {
            damage_rects = u32::try_from(rects.len()).unwrap_or(u32::MAX);
            damaged_pixels = rects
                .iter()
                .map(|r| {
                    u64::try_from(r.size.w.max(0)).unwrap_or(0)
                        * u64::try_from(r.size.h.max(0)).unwrap_or(0)
                })
                .sum::<u64>()
                .min(total_output_pixels);
            tracked_damage = Some(rects.clone());
        } else {
            damage_rects = 0;
            damaged_pixels = 0;
            tracked_damage = None;
        }
    }

    let mut presentation_feedback = state.take_presentation_feedback(&state.output, &render_states);
    let elapsed = state.elapsed();
    if state.ext_session_locked {
        if let Some(lock_surf) = state.ext_lock_surface_for_output(&state.output) {
            send_frames_surface_tree(
                lock_surf.wl_surface(),
                &state.output,
                elapsed,
                Some(Duration::ZERO),
                |_, _| Some(state.output.clone()),
            );
        }
    } else {
        for window in state.space.elements() {
            window.send_frame(&state.output, elapsed, Some(Duration::ZERO), |_, _| {
                Some(state.output.clone())
            });
        }
        state.send_ime_popup_frames(&state.output, elapsed);
    }

    state.space.refresh();
    state.popups.cleanup();
    state.cleanup_ime_popups();

    let submit_slice = tracked_damage.as_deref();
    backend
        .submit(submit_slice)
        .map_err(|err| err.to_string())?;
    presentation_feedback.presented(
        state.clock.now(),
        Refresh::fixed(Duration::from_micros(16_667)),
        frame_seq,
        wp_presentation_feedback::Kind::Vsync,
    );
    state.confirm_pending_session_lock();
    let _ = state.display_handle.flush_clients();

    let present_done = Instant::now();
    state.frame_timing.record_frame(
        render_start,
        present_done,
        damage_rects,
        damaged_pixels,
        total_output_pixels,
    );

    // Ensure the Window trait bound is used cleanly.
    let _ = <smithay::desktop::Window as AsRenderElements<GlesRenderer>>::render_elements::<
        smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement<GlesRenderer>,
    >;

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WinitCursorSync {
    Hidden,
    ClientSurface,
    Named(CursorIcon),
}

fn sync_winit_window_cursor(
    backend: &WinitGraphicsBackend<GlesRenderer>,
    status: &CursorImageStatus,
    last: &mut Option<WinitCursorSync>,
) {
    let target = match status {
        CursorImageStatus::Hidden => WinitCursorSync::Hidden,
        CursorImageStatus::Surface(s) if s.alive() => WinitCursorSync::ClientSurface,
        CursorImageStatus::Surface(_) => WinitCursorSync::Named(CursorIcon::Default),
        CursorImageStatus::Named(icon) => WinitCursorSync::Named(*icon),
    };
    if *last == Some(target) {
        return;
    }
    *last = Some(target);
    let win = backend.window();
    match target {
        WinitCursorSync::Hidden | WinitCursorSync::ClientSurface => {
            win.set_cursor_visible(false);
        }
        WinitCursorSync::Named(icon) => {
            win.set_cursor(icon);
            win.set_cursor_visible(true);
        }
    }
}
