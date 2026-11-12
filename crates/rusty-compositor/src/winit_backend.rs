//! Nested Winit backend runner (`RUSTYOS-002`).

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use smithay::{
    backend::{
        renderer::{
            Color32F, damage::OutputDamageTracker, element::AsRenderElements, gles::GlesRenderer,
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
const BACKGROUND_COLOR: Color32F = Color32F::new(0.11, 0.13, 0.17, 1.0);

/// Runs the nested Smithay + Winit compositor event loop.
///
/// # Errors
///
/// Returns [`CompositorError`] if the event loop, Wayland display, listening socket,
/// or Winit EGL/GLES backend cannot be initialized.
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
    let mut rendered_frames: u64 = 0;

    loop_handle
        .insert_source(
            Timer::from_duration(Duration::from_micros(16_667)),
            move |_deadline, _metadata, app_state| {
                if !app_state.running {
                    return TimeoutAction::Drop;
                }

                let tick_start = Instant::now();
                if let Err(err) = render_winit_frame(&mut backend, &mut damage_tracker, app_state) {
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

    info!(
        wayland_display = %socket_name,
        width = initial_size.w,
        height = initial_size.h,
        "RustyOS nested Winit compositor listening"
    );

    for cmd in &config.spawn_clients {
        spawn_test_client(cmd, &socket_name);
    }

    event_loop
        .run(Some(Duration::from_millis(4)), &mut state, |app_state| {
            if let Err(err) = display.dispatch_clients(app_state) {
                error!(error = %err, "Failed to dispatch Wayland client events");
            }
            app_state.prune_dead_windows();
            app_state.space.refresh();
            app_state.popups.cleanup();
            if let Err(err) = display.flush_clients() {
                warn!(error = %err, "Failed to flush Wayland clients");
            }
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
            app_state
                .wm
                .upsert_output(PRIMARY_OUTPUT_ID, rusty_wm::Rect::new(0, 0, size.w, size.h));
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
) -> Result<(), String> {
    let render_start = Instant::now();
    let size = backend.window_size();
    let full_damage = Rectangle::from_size(size);
    let damage_rects: u32;

    {
        let (renderer, mut framebuffer) = backend.bind().map_err(|err| err.to_string())?;
        let custom_elements: [&smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement<GlesRenderer>; 0] = [];

        let render_result = render_output::<
            GlesRenderer,
            &smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement<
                GlesRenderer,
            >,
            smithay::desktop::Window,
            _,
        >(
            &state.output,
            renderer,
            &mut framebuffer,
            1.0,
            0,
            [&state.space],
            &custom_elements,
            damage_tracker,
            BACKGROUND_COLOR,
        )
        .map_err(|err| err.to_string())?;

        damage_rects = render_result
            .damage
            .map_or(0, |rects| u32::try_from(rects.len()).unwrap_or(u32::MAX));
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

    backend
        .submit(Some(&[full_damage]))
        .map_err(|err| err.to_string())?;

    let present_done = Instant::now();
    state
        .frame_timing
        .record_frame(render_start, present_done, damage_rects);

    // Ensure the Window trait bound is used cleanly.
    let _ = <smithay::desktop::Window as AsRenderElements<GlesRenderer>>::render_elements::<
        smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement<GlesRenderer>,
    >;

    Ok(())
}

fn spawn_test_client(command: &str, socket_name: &str) {
    let mut parts = command.split_whitespace();
    let Some(program) = parts.next() else {
        return;
    };
    let args: Vec<&str> = parts.collect();

    match Command::new(program)
        .args(&args)
        .env("WAYLAND_DISPLAY", socket_name)
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
