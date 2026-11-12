//! Nested Winit backend runner (`RUSTYOS-002`).

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use rusty_ipc::{IpcServer, default_ipc_socket_path};
use smithay::{
    backend::{
        renderer::{
            Color32F, damage::OutputDamageTracker, element::AsRenderElements,
            element::solid::SolidColorRenderElement, gles::GlesRenderer,
        },
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    desktop::space::render_output,
    output::{Mode, Scale},
    reexports::{
        calloop::{
            EventLoop,
            timer::{TimeoutAction, Timer},
        },
        wayland_server::Display,
    },
    utils::Rectangle,
    wayland::socket::ListeningSocketSource,
};
use tracing::{error, info, warn};

use crate::{
    BackendMode, BootstrapReport, CompositorConfig, CompositorError,
    state::{PRIMARY_OUTPUT_ID, RustyCompositorState},
};

/// Clear color for the nested compositor background (dark slate inspired by macOS dark desktop).
/// Background clear color matching `background = "#0F0A19"` from `DESIGN.md` (Version 2.0).
const BACKGROUND_COLOR: Color32F = Color32F::new(0.059, 0.039, 0.098, 1.0);

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
        initial_size,
    )?;
    state.animations.set_reduced_motion(config.reduced_motion);
    let mut damage_tracker = OutputDamageTracker::from_output(&state.output);
    let loop_handle = event_loop.handle();

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

                let tick_start = Instant::now();
                if let Err(err) = render_winit_frame(
                    &mut backend,
                    &mut damage_tracker,
                    app_state,
                    force_full_damage,
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

    for cmd in &config.spawn_clients {
        spawn_test_client(cmd, &socket_name, &ipc_socket_path);
    }

    event_loop
        .run(Some(Duration::from_millis(4)), &mut state, |app_state| {
            dispatch_and_refresh_clients(&mut display, ipc_server.as_mut(), app_state);
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let timing_summary =
        finalize_frame_timing(&state, initial_size, config.metrics_out.as_deref())?;

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

fn dispatch_and_refresh_clients(
    display: &mut Display<RustyCompositorState>,
    ipc_server: Option<&mut IpcServer>,
    app_state: &mut RustyCompositorState,
) {
    let dispatch_start = Instant::now();
    if let Some(ipc) = ipc_server {
        ipc.poll_requests(|req| app_state.handle_ipc_request(req));
    }
    if let Err(err) = display.dispatch_clients(app_state) {
        error!(error = %err, "Failed to dispatch Wayland client events");
    }
    app_state.prune_dead_windows();
    app_state.space.refresh();
    app_state.popups.cleanup();
    if let Err(err) = display.flush_clients() {
        warn!(error = %err, "Failed to flush Wayland clients");
    }
    app_state
        .frame_timing
        .record_dispatch_duration(dispatch_start.elapsed());
}

fn finalize_frame_timing(
    state: &RustyCompositorState,
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
        let json = timing_summary.to_json(BackendMode::NestedWinit.as_str(), mode_size);
        fs::write(path, json).map_err(|err| CompositorError::MetricsExport(err.to_string()))?;
        info!(path = %path, "Exported frame timing JSON report");
    }

    Ok(timing_summary)
}

fn handle_winit_event(event: WinitEvent, app_state: &mut RustyCompositorState) {
    match event {
        WinitEvent::Resized { size, scale_factor } => {
            update_output_size(&app_state.output, size, scale_factor);
            let work_area = crate::shell_render::usable_work_area(size);
            app_state.wm.upsert_output(PRIMARY_OUTPUT_ID, work_area);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            app_state.sync_space_with_wm(serial);
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

fn update_output_size(
    output: &smithay::output::Output,
    size: smithay::utils::Size<i32, smithay::utils::Physical>,
    scale_factor: f64,
) {
    let mode = Mode {
        size,
        refresh: 60_000,
    };
    output.change_current_state(
        Some(mode),
        None,
        Some(Scale::Fractional(scale_factor)),
        None,
    );
}

fn render_winit_frame(
    backend: &mut WinitGraphicsBackend<GlesRenderer>,
    damage_tracker: &mut OutputDamageTracker,
    state: &mut RustyCompositorState,
    force_full_damage: bool,
) -> Result<(), String> {
    let render_start = Instant::now();
    state.tick_animations();
    let size = backend.window_size();
    let full_damage = Rectangle::from_size(size);
    let total_output_pixels =
        u64::try_from(size.w.max(0)).unwrap_or(0) * u64::try_from(size.h.max(0)).unwrap_or(0);
    let damage_rects: u32;
    let damaged_pixels: u64;
    let tracked_damage: Option<Vec<Rectangle<i32, smithay::utils::Physical>>>;

    {
        let buffer_age = if force_full_damage {
            0
        } else {
            backend.buffer_age().unwrap_or(0)
        };
        let (renderer, mut framebuffer) = backend.bind().map_err(|err| err.to_string())?;

        let active_ws = state.wm.active_workspace_id().0;
        let layout_str = match state.wm.active_layout_mode() {
            rusty_wm::LayoutMode::Floating => "floating",
            rusty_wm::LayoutMode::MasterStack => "master-stack",
        };
        let window_count = state.wm.visible_windows().len();
        let has_focused = state.wm.focused_window().is_some();
        let has_layer_surfaces = smithay::desktop::layer_map_for_output(&state.output).len() > 0;
        let shell_elements = if has_layer_surfaces {
            Vec::new()
        } else {
            crate::shell_render::generate_shell_render_elements(
                size,
                active_ws,
                layout_str,
                window_count,
                has_focused,
                state.launcher_active,
            )
        };

        let render_result =
            render_output::<GlesRenderer, SolidColorRenderElement, smithay::desktop::Window, _>(
                &state.output,
                renderer,
                &mut framebuffer,
                1.0,
                buffer_age,
                [&state.space],
                &shell_elements,
                damage_tracker,
                BACKGROUND_COLOR,
            )
            .map_err(|err| err.to_string())?;

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

    let elapsed = state.elapsed();
    for window in state.space.elements() {
        window.send_frame(&state.output, elapsed, Some(Duration::ZERO), |_, _| {
            Some(state.output.clone())
        });
    }

    state.space.refresh();
    state.popups.cleanup();
    let _ = state.display_handle.flush_clients();

    let submit_slice = tracked_damage.as_deref();
    backend
        .submit(submit_slice)
        .map_err(|err| err.to_string())?;

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

fn spawn_test_client(command: &str, socket_name: &str, ipc_socket_path: &std::path::Path) {
    let mut parts = command.split_whitespace();
    let Some(program) = parts.next() else {
        return;
    };
    let args: Vec<&str> = parts.collect();

    match Command::new(program)
        .args(&args)
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
        }
        Err(err) => {
            warn!(program, error = %err, "Failed to spawn child Wayland client");
        }
    }
}
