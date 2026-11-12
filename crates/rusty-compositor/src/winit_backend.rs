//! Nested Winit backend runner (`RUSTYOS-002`).

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use rusty_ipc::{IpcServer, default_ipc_socket_path};
use smithay::{
    backend::{
        renderer::{
            Color32F,
            damage::OutputDamageTracker,
            element::AsRenderElements,
            element::Kind,
            element::surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            gles::GlesRenderer,
        },
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    input::pointer::{CursorIcon, CursorImageStatus, CursorImageSurfaceData},
    output::{Mode, Scale},
    reexports::{
        calloop::{
            EventLoop,
            timer::{TimeoutAction, Timer},
        },
        wayland_server::{Display, protocol::wl_surface::WlSurface},
    },
    utils::{IsAlive, Logical, Point, Rectangle, Size},
    wayland::{compositor, shell::wlr_layer::Layer, socket::ListeningSocketSource},
};
use tracing::{error, info, warn};

use crate::{
    BackendMode, BootstrapReport, CompositorConfig, CompositorError,
    shell_render::CompositorRenderElement,
    state::{PRIMARY_OUTPUT_ID, RustyCompositorState},
};

/// Background clear color matching `background = "#0F0A19"` from `DESIGN.md` (Version 2.0).
pub(crate) const BACKGROUND_COLOR: Color32F = Color32F::new(0.059, 0.039, 0.098, 1.0);

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
    state
        .animations
        .set_reduced_motion(state.user_config.reduced_motion || config.reduced_motion);
    state.telemetry = crate::telemetry::SystemMetricsHandle::start_background();
    state.dbus_notifications =
        crate::dbus_notifications::DbusNotificationHandle::start_background();
    state.tray = crate::tray::TrayWatcherHandle::start_background();
    let mut damage_tracker = OutputDamageTracker::from_output(&state.output);
    let mut text_cache = crate::text::TextCache::new();
    let mut shape_cache = crate::surfaces::UiShapeCache::new();
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

    let autostart = state.user_config.autostart.clone();
    for cmd in autostart.iter().chain(config.spawn_clients.iter()) {
        spawn_test_client(cmd, &socket_name, &ipc_socket_path);
    }

    event_loop
        .run(Some(Duration::from_millis(4)), &mut state, |app_state| {
            dispatch_and_refresh_clients(&mut display, ipc_server.as_mut(), app_state);
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let timing_summary = finalize_frame_timing(
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

pub(crate) fn dispatch_and_refresh_clients(
    display: &mut Display<RustyCompositorState>,
    ipc_server: Option<&mut IpcServer>,
    app_state: &mut RustyCompositorState,
) {
    let dispatch_start = Instant::now();
    app_state.drain_dbus_notifications();
    app_state.drain_tray_updates();
    app_state.drain_screenshot_results();
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

fn push_layer_elements(
    render_list: &mut Vec<CompositorRenderElement>,
    output: &smithay::output::Output,
    renderer: &mut GlesRenderer,
    layers: &[Layer],
) -> bool {
    let layer_map = smithay::desktop::layer_map_for_output(output);
    let has_layers = layer_map.len() > 0;
    if has_layers {
        for &layer in layers {
            for surface in layer_map.layers_on(layer) {
                if let Some(geo) = layer_map.layer_geometry(surface) {
                    let loc = geo.loc.to_physical_precise_round(1.0);
                    render_list.extend(
                        AsRenderElements::<GlesRenderer>::render_elements::<
                            WaylandSurfaceRenderElement<GlesRenderer>,
                        >(
                            surface,
                            renderer,
                            loc,
                            smithay::utils::Scale::from(1.0),
                            1.0,
                        )
                        .into_iter()
                        .map(CompositorRenderElement::Surface),
                    );
                }
            }
        }
    }
    has_layers
}

fn window_title(window: &smithay::desktop::Window, id: rusty_wm::WindowId) -> String {
    if let Some(toplevel) = window.toplevel() {
        let title = smithay::wayland::compositor::with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                .and_then(|data| data.lock().ok())
                .and_then(|guard| guard.title.clone().or_else(|| guard.app_id.clone()))
        });
        if let Some(t) = title
            && !t.trim().is_empty()
        {
            return t;
        }
    } else if let Some(x11) = window.x11_surface() {
        let title = x11.title();
        if !title.trim().is_empty() {
            return title;
        }
    }
    format!("Window #{}", id.0)
}

/// Builds the front-to-back composite render element list shared by nested Winit and native DRM/KMS.
#[allow(clippy::too_many_lines)]
pub(crate) fn build_frame_render_list(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &mut RustyCompositorState,
    size: smithay::utils::Size<i32, smithay::utils::Physical>,
    include_software_cursor: bool,
) -> Vec<CompositorRenderElement> {
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
    let window_count = visible_windows
        .iter()
        .filter(|(id, _)| !state.is_window_minimized(*id))
        .count();
    let focused_win = state
        .wm
        .focused_window()
        .filter(|id| !state.is_window_minimized(*id));

    let mut render_list: Vec<CompositorRenderElement> = Vec::new();
    if include_software_cursor {
        #[allow(clippy::cast_possible_truncation)]
        let ptr = Point::from((
            state.pointer_location.x.round() as i32,
            state.pointer_location.y.round() as i32,
        ));
        match &state.cursor_status {
            CursorImageStatus::Hidden => {}
            CursorImageStatus::Surface(surf) if surf.alive() => {
                push_client_surface_cursor(&mut render_list, renderer, surf, ptr);
            }
            CursorImageStatus::Surface(_) => {
                if let Some(cursor_el) = shape_cache.named_cursor_element(
                    renderer,
                    CursorIcon::Default,
                    state.user_config.cursor_theme.as_deref(),
                    ptr,
                ) {
                    render_list.push(CompositorRenderElement::Memory(cursor_el));
                }
            }
            CursorImageStatus::Named(icon) => {
                if let Some(cursor_el) = shape_cache.named_cursor_element(
                    renderer,
                    *icon,
                    state.user_config.cursor_theme.as_deref(),
                    ptr,
                ) {
                    render_list.push(CompositorRenderElement::Memory(cursor_el));
                }
            }
        }
    }

    if let Some(ref dnd_surface) = state.dnd_icon
        && dnd_surface.alive()
    {
        #[allow(clippy::cast_possible_truncation)]
        let dnd_loc = Point::<i32, Logical>::from((
            state.pointer_location.x.round() as i32 + 10,
            state.pointer_location.y.round() as i32 + 10,
        ))
        .to_physical_precise_round(1.0);
        render_list.extend(
            render_elements_from_surface_tree::<
                GlesRenderer,
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(
                renderer,
                dnd_surface,
                dnd_loc,
                smithay::utils::Scale::from(1.0),
                0.9,
                Kind::Unspecified,
            )
            .into_iter()
            .map(CompositorRenderElement::Surface),
        );
    }

    let tel = state.telemetry.snapshot();
    let timing = state.frame_timing.summary();
    let lock_progress = state.lock_screen_anim.current();
    if state.screen_locked || lock_progress > 0.01 {
        let lock_view = crate::shell_render::LockScreenRenderState {
            clock_hhmm: &tel.clock_hhmm,
            network_label: &tel.network.status_label,
            battery_percent: tel.power.percentage,
            has_battery: tel.power.has_battery,
            window_count,
            progress: lock_progress,
        };
        render_list.extend(crate::shell_render::generate_lock_screen_elements(
            text_cache,
            shape_cache,
            renderer,
            size,
            &lock_view,
        ));
    }

    if !state.screen_locked
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
            size,
            &osd_view,
        ));
    }

    let layer_surfaces_present = push_layer_elements(
        &mut render_list,
        &state.output,
        renderer,
        &[Layer::Overlay, Layer::Top],
    );

    if !layer_surfaces_present {
        let tray_views: Vec<crate::shell_render::TopBarTrayItem<'_>> = state
            .tray_items
            .iter()
            .take(crate::shell_render::MAX_TOP_BAR_TRAY_ITEMS)
            .map(|item| crate::shell_render::TopBarTrayItem {
                key: &item.key,
                label: &item.badge,
                icon_name: &item.icon_name,
                icon_pixmap: item.icon_pixmap.as_ref(),
                attention: item.status == crate::tray::TrayStatus::NeedsAttention,
                passive: item.status == crate::tray::TrayStatus::Passive,
            })
            .collect();
        let top_telemetry = crate::shell_render::TopBarTelemetry {
            clock_hhmm: &tel.clock_hhmm,
            battery_percent: tel.power.percentage,
            has_battery: tel.power.has_battery,
            net_connected: tel.network.connected,
            volume_percent: tel.audio.volume_percent,
            audio_muted: tel.audio.muted,
            quick_settings_open: state.quick_settings_active,
            notification_count: state.notification_count,
            kbd_layout: &state.active_kbd_layout,
            tray_items: &tray_views,
        };

        let toast_views: Vec<crate::shell_render::NotificationToastView<'_>> = state
            .notifications
            .iter()
            .rev()
            .take(crate::shell_render::MAX_VISIBLE_NOTIFICATIONS)
            .map(|n| crate::shell_render::NotificationToastView {
                id: n.id,
                urgency: n.urgency,
                title: &n.title,
                body: &n.body,
            })
            .collect();

        let mut dock_windows: Vec<crate::shell_render::DockWindowEntry> = visible_windows
            .iter()
            .map(|&(win_id, rect)| {
                let minimized = state.is_window_minimized(win_id);
                let title = state.smithay_window_for_id(win_id).map_or_else(
                    || format!("Window #{}", win_id.0),
                    |win| window_title(&win, win_id),
                );
                let badge = crate::shell_render::make_window_badge(&title);
                let app_id = state.window_app_id(win_id);
                let icon_name = state.launcher.icon_for_app_hints(app_id.as_deref(), &title);
                crate::shell_render::DockWindowEntry {
                    id: win_id,
                    title,
                    badge,
                    icon_name,
                    dimensions: format!("{}x{}", rect.width, rect.height),
                    focused: !minimized && focused_win == Some(win_id),
                    minimized,
                }
            })
            .collect();
        dock_windows.sort_by_key(|entry| entry.id.0);

        let terminal_icon = state.launcher.preferred_icon(
            &[
                "foot",
                "kitty",
                "alacritty",
                "wezterm",
                "ghostty",
                "weston-terminal",
                "gnome-terminal",
                "konsole",
                "xfce4-terminal",
            ],
            "utilities-terminal",
        );
        let files_icon = state.launcher.preferred_icon(
            &["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"],
            "system-file-manager",
        );
        let dock_view = crate::shell_render::DockViewState {
            pinned_icons: [Some(terminal_icon), Some(files_icon)],
            windows: &dock_windows,
            hover_slot: state.dock_hover_slot,
            overview_active: state.overview_active,
        };

        let switcher_cards: Vec<crate::shell_render::WindowSwitcherCard<'_>> = state
            .window_switcher
            .as_ref()
            .map(|sw| {
                sw.window_ids
                    .iter()
                    .filter_map(|&win_id| {
                        dock_windows
                            .iter()
                            .find(|entry| entry.id == win_id)
                            .map(|entry| crate::shell_render::WindowSwitcherCard {
                                title: &entry.title,
                                badge: &entry.badge,
                                icon_name: entry.icon_name.as_deref(),
                                minimized: entry.minimized,
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let switcher_view = state.window_switcher.as_ref().and_then(|sw| {
            (!switcher_cards.is_empty()).then_some(crate::shell_render::WindowSwitcherRenderState {
                cards: &switcher_cards,
                selected: sw.selected.min(switcher_cards.len() - 1),
            })
        });

        let launcher_progress = state.launcher_anim.current();
        let show_launcher = state.launcher_active || launcher_progress > 0.01;
        let (win_start, visible_items) = if show_launcher {
            state.launcher.visible_window(3)
        } else {
            (0, Vec::new())
        };
        let cursor_offset_x = if show_launcher && !state.launcher.query.is_empty() {
            text_cache.measure_text(&state.launcher.query, 14.5).0
        } else {
            0
        };
        let spotlight_view = show_launcher.then_some(crate::shell_render::SpotlightRenderState {
            query: &state.launcher.query,
            selected_in_window: state.launcher.selected_index.saturating_sub(win_start),
            cursor_offset_x,
            items: &visible_items,
            progress: launcher_progress,
        });

        let qs_progress = state.quick_settings_anim.current();
        let show_qs = !show_launcher && (state.quick_settings_active || qs_progress > 0.01);
        let qs_view = show_qs.then_some(crate::shell_render::QuickSettingsRenderState {
            layout_mode: layout_str,
            reduced_motion: state.animations.reduced_motion(),
            network_label: &tel.network.status_label,
            net_connected: tel.network.connected,
            bt_available: tel.bluetooth.available,
            bt_powered: tel.bluetooth.powered,
            bt_label: &tel.bluetooth.status_label,
            battery_percent: tel.power.percentage,
            has_battery: tel.power.has_battery,
            ac_online: tel.power.ac_online,
            volume_percent: tel.audio.volume_percent,
            audio_muted: tel.audio.muted,
            frame_p50_us: timing.interval_p50_us,
            window_count,
            progress: qs_progress,
        });

        let ov_progress = state.overview_anim.current();
        let show_ov = !show_launcher && !show_qs && (state.overview_active || ov_progress > 0.01);
        let ov_view = show_ov.then_some(crate::shell_render::OverviewRenderState {
            active_workspace: active_ws,
            layout_mode: layout_str,
            windows: &dock_windows,
            progress: ov_progress,
        });

        let overlay = if let Some(sw) = switcher_view.as_ref() {
            crate::shell_render::ShellOverlayView::Switcher(sw)
        } else if let Some(spot) = spotlight_view.as_ref() {
            crate::shell_render::ShellOverlayView::Spotlight(spot)
        } else if let Some(qs) = qs_view.as_ref() {
            crate::shell_render::ShellOverlayView::QuickSettings(qs)
        } else if let Some(ov) = ov_view.as_ref() {
            crate::shell_render::ShellOverlayView::Overview(ov)
        } else {
            crate::shell_render::ShellOverlayView::None
        };

        render_list.extend(crate::shell_render::generate_shell_composite_elements(
            text_cache,
            shape_cache,
            renderer,
            size,
            active_ws,
            layout_str,
            &dock_view,
            &top_telemetry,
            overlay,
            &toast_views,
        ));
    }

    // Render managed windows and their SSD titlebars (or clean CSD rim) in top-to-bottom stacking order
    for &(win_id, rect) in visible_windows.iter().rev() {
        if state.is_window_minimized(win_id) {
            continue;
        }
        let origin = state
            .animations
            .animated_window_origin(win_id.0)
            .unwrap_or((rect.x, rect.y));
        let is_focused = focused_win == Some(win_id);
        let has_ssd = !state.window_uses_csd(win_id);
        let titlebar_h = if has_ssd {
            crate::shell_render::TITLEBAR_HEIGHT
        } else {
            0
        };
        let actively_resizing = state
            .active_window_resize
            .is_some_and(|resize| resize.window_id == win_id);

        let smithay_win = state.smithay_window_for_id(win_id);
        let committed_size = smithay_win.as_ref().map(|w| w.geometry().size);
        let frame_rect = match (state.wm.active_layout_mode(), committed_size) {
            (rusty_wm::LayoutMode::Floating, Some(sz))
                if !actively_resizing && sz.w >= 64 && sz.h >= 64 =>
            {
                rusty_wm::Rect::new(
                    origin.0,
                    origin.1,
                    rect.width.max(sz.w),
                    rect.height.max(sz.h + titlebar_h),
                )
            }
            _ => rusty_wm::Rect::new(origin.0, origin.1, rect.width, rect.height),
        };

        let has_open_popup = smithay_win
            .as_ref()
            .and_then(smithay::wayland::seat::WaylandFocus::wl_surface)
            .is_some_and(|root| {
                smithay::desktop::PopupManager::popups_for_surface(&root)
                    .next()
                    .is_some()
            });

        if has_ssd && !has_open_popup {
            let title_text = smithay_win.as_ref().map_or_else(
                || format!("Window #{}", win_id.0),
                |win| window_title(win, win_id),
            );
            render_list.extend(crate::shell_render::generate_window_ssd_composite_elements(
                text_cache,
                shape_cache,
                renderer,
                frame_rect,
                &title_text,
                is_focused,
            ));
        }

        if let Some(ref sw) = smithay_win {
            let geo_loc = sw.geometry().loc;
            let client_loc = Point::<i32, Logical>::from((
                origin.0 - geo_loc.x,
                origin.1 + titlebar_h - geo_loc.y,
            ))
            .to_physical_precise_round(1.0);
            render_list.extend(
                <smithay::desktop::Window as AsRenderElements<GlesRenderer>>::render_elements::<
                    WaylandSurfaceRenderElement<GlesRenderer>,
                >(
                    sw,
                    renderer,
                    client_loc,
                    smithay::utils::Scale::from(1.0),
                    1.0,
                )
                .into_iter()
                .map(CompositorRenderElement::Surface),
            );
        }

        if has_ssd && has_open_popup {
            let title_text = smithay_win.as_ref().map_or_else(
                || format!("Window #{}", win_id.0),
                |win| window_title(win, win_id),
            );
            render_list.extend(crate::shell_render::generate_window_ssd_composite_elements(
                text_cache,
                shape_cache,
                renderer,
                frame_rect,
                &title_text,
                is_focused,
            ));
        }

        render_list.extend(crate::shell_render::generate_window_shadow_elements(
            shape_cache,
            renderer,
            frame_rect,
            is_focused,
        ));
    }

    if layer_surfaces_present {
        push_layer_elements(
            &mut render_list,
            &state.output,
            renderer,
            &[Layer::Bottom, Layer::Background],
        );
    }

    if let Some(wallpaper) = shape_cache.wallpaper_element(
        renderer,
        Size::<i32, Logical>::from((size.w, size.h)),
        state.user_config.wallpaper.as_deref(),
    ) {
        render_list.push(CompositorRenderElement::Memory(wallpaper));
    }

    render_list
}

pub(crate) fn process_pending_screenshot(
    renderer: &mut GlesRenderer,
    render_list: &[CompositorRenderElement],
    state: &mut RustyCompositorState,
    size: smithay::utils::Size<i32, smithay::utils::Physical>,
) {
    let Some(req) = state.pending_screenshot.take() else {
        return;
    };
    let crop = match req {
        crate::screenshot::ScreenshotRequest::FullScreen => None,
        crate::screenshot::ScreenshotRequest::FocusedWindow => state
            .wm
            .focused_window()
            .and_then(|id| state.wm.window(id))
            .map(|w| w.geometry),
    };
    match crate::screenshot::capture_render_list_rgba(renderer, render_list, size, crop) {
        Ok((width, height, rgba)) => {
            crate::screenshot::spawn_save_screenshot(
                width,
                height,
                rgba,
                state.socket_name.clone(),
                &state.screenshot_results,
            );
        }
        Err(err) => {
            warn!(error = %err, "Failed to capture compositor screenshot");
            state.show_osd("Screenshot Failed", None, false);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn render_winit_frame(
    backend: &mut WinitGraphicsBackend<GlesRenderer>,
    damage_tracker: &mut OutputDamageTracker,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &mut RustyCompositorState,
    force_full_damage: bool,
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

    if state.pending_screenshot.is_some() {
        let renderer = backend.renderer();
        let snap_list =
            build_frame_render_list(renderer, text_cache, shape_cache, state, size, false);
        process_pending_screenshot(renderer, &snap_list, state, size);
    }

    {
        let buffer_age = if force_full_damage {
            0
        } else {
            backend.buffer_age().unwrap_or(0)
        };
        let (renderer, mut framebuffer) = backend.bind().map_err(|err| err.to_string())?;
        let mut render_list =
            build_frame_render_list(renderer, text_cache, shape_cache, state, size, false);
        if let CursorImageStatus::Surface(ref cursor_surf) = state.cursor_status
            && cursor_surf.alive()
        {
            #[allow(clippy::cast_possible_truncation)]
            let ptr = Point::from((
                state.pointer_location.x.round() as i32,
                state.pointer_location.y.round() as i32,
            ));
            let mut cursor_elems = Vec::new();
            push_client_surface_cursor(&mut cursor_elems, renderer, cursor_surf, ptr);
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

fn push_client_surface_cursor(
    render_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    surface: &WlSurface,
    pointer_loc: Point<i32, Logical>,
) {
    let hotspot = compositor::with_states(surface, |states| {
        states
            .data_map
            .get::<CursorImageSurfaceData>()
            .and_then(|data| data.lock().ok())
            .map(|attrs| attrs.hotspot)
    })
    .unwrap_or_default();
    let loc = (pointer_loc - hotspot).to_physical_precise_round(1.0);
    render_list.extend(
        render_elements_from_surface_tree::<
            GlesRenderer,
            WaylandSurfaceRenderElement<GlesRenderer>,
        >(
            renderer,
            surface,
            loc,
            smithay::utils::Scale::from(1.0),
            1.0,
            Kind::Cursor,
        )
        .into_iter()
        .map(CompositorRenderElement::Surface),
    );
}

pub(crate) fn spawn_test_client(
    command: &str,
    socket_name: &str,
    ipc_socket_path: &std::path::Path,
) {
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
