//! Backend-agnostic frame composition, `HiDPI` element scaling, screenshots, and portal captures (`RUSTYOS-060`).

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use rusty_ipc::IpcServer;
use smithay::{
    backend::renderer::{
        Color32F,
        element::AsRenderElements,
        element::Element,
        element::Id,
        element::Kind,
        element::solid::SolidColorRenderElement,
        element::surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        element::utils::{Relocate, RelocateRenderElement, RescaleRenderElement},
        gles::GlesRenderer,
        utils::CommitCounter,
    },
    input::pointer::{CursorIcon, CursorImageStatus, CursorImageSurfaceData},
    reexports::wayland_server::{Display, protocol::wl_surface::WlSurface},
    utils::{IsAlive, Logical, Point, Rectangle, Size},
    wayland::{compositor, shell::wlr_layer::Layer},
};
use tracing::{error, info, warn};

use crate::{
    BackendMode, CompositorError,
    shell_render::{
        CompositorRenderElement, DockViewState, DockWindowEntry, MAX_TOP_BAR_TRAY_ITEMS,
        MAX_VISIBLE_NOTIFICATIONS, NotificationToastView, OverviewRenderState,
        PolkitPromptRenderState, PortalPromptRenderState, PortalSourceRow,
        QuickSettingsRenderState, ShellOverlayView, SpotlightRenderState, TopBarTelemetry,
        TopBarTrayItem, WindowSwitcherCard, WindowSwitcherRenderState,
    },
    state::RustyCompositorState,
};

/// Background clear color matching `background = "#0F0A19"` from `DESIGN.md` (Version 2.0).
pub(crate) const BACKGROUND_COLOR: Color32F = Color32F::new(0.059, 0.039, 0.098, 1.0);

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
    app_state.drain_tray_updates();
    app_state.drain_screenshot_results();
    app_state.drain_trash_empty_results();
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
    let backdrop = SolidColorRenderElement::new(
        Id::new(),
        Rectangle::new(
            (0, 0).into(),
            (logical_size.w.max(1), logical_size.h.max(1)).into(),
        ),
        CommitCounter::default(),
        Color32F::BLACK,
        Kind::Unspecified,
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
                dimensions: format!("{}x{}", rect.width, rect.height),
                focused: !minimized && focused_win == Some(win_id),
                minimized,
            }
        })
        .collect();
    dock_windows.sort_by_key(|entry| entry.id.0);
    dock_windows
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
    let focused_win = state
        .wm
        .focused_window()
        .filter(|id| !state.is_window_minimized(*id));

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

    shape_cache.sync_wallpaper_revision(state.wallpaper_revision);

    let tel = state.telemetry.snapshot();
    let timing = state.frame_timing.summary();
    let lock_progress = state.lock_screen_anim.current();
    if state.screen_locked || lock_progress > 0.01 {
        let lock_view = state.lock_screen_render_state(&tel, lock_progress);
        render_list.extend(crate::shell_render::generate_lock_screen_elements(
            text_cache,
            shape_cache,
            renderer,
            logical_size,
            &lock_view,
        ));
    }

    push_ime_popup_elements(
        &mut render_list,
        &smithay_output,
        renderer,
        state,
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

    let fullscreen_win = if is_primary {
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
        let top_telemetry = TopBarTelemetry {
            clock_hhmm: clock_label,
            battery_percent: tel.power.percentage,
            has_battery: tel.power.has_battery,
            ac_online: tel.power.ac_online,
            battery_charging: tel.power.state == rusty_services::BatteryState::Charging,
            net_connected: tel.network.connected,
            wifi_signal_percent: tel.network.wifi_signal_percent,
            volume_percent: tel.audio.volume_percent,
            audio_muted: tel.audio.muted,
            quick_settings_open: state.quick_settings_active,
            notification_count: state.notification_count,
            kbd_layout: &state.active_kbd_layout,
            tray_items: &tray_views,
            active_menu: state.active_top_bar_menu,
            menu_hover_row: state.top_bar_menu_hover_row,
            glass_opacity: state.glass_opacity,
            reduce_transparency: state.reduce_transparency,
            show_workspaces: state.menubar_show_workspaces,
            show_tray: state.menubar_show_tray,
            show_input_source: state.menubar_show_input_source,
            show_volume: state.menubar_show_volume,
        };

        let toast_views: Vec<NotificationToastView<'_>> = if state.focus_mode {
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
                    title: &n.title,
                    body: &n.body,
                    hovered: state.hovered_notification_idx == Some(idx),
                })
                .collect()
        };

        let dock_windows = collect_dock_windows(state, &visible_windows, focused_win);
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
        let files_icon = state
            .launcher
            .preferred_icon(&["rusty-files"], "system-file-manager");
        let dock_view = DockViewState {
            pinned_icons: [Some(terminal_icon), Some(files_icon)],
            windows: &dock_windows,
            hover_slot: state.dock_hover_slot,
            hover_progress: state.dock_hover_anim.current(),
            reduced_motion: state.animations.reduced_motion(),
            overview_active: state.overview_active,
        };

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
            })
        });

        let launcher_progress = state.launcher_anim.current();
        let show_launcher = state.launcher_active || launcher_progress > 0.01;
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

        let qs_progress = state.quick_settings_anim.current();
        let show_qs = !show_launcher && (state.quick_settings_active || qs_progress > 0.01);
        let qs_view = show_qs.then_some(QuickSettingsRenderState {
            layout_mode: layout_str,
            reduced_motion: state.animations.reduced_motion(),
            network_label: &tel.network.status_label,
            net_connected: tel.network.connected,
            wifi_enabled: tel.network.wifi_enabled,
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
            media_active: tel.media.active,
            media_playing: tel.media.playing,
            media_title: &tel.media.title,
            focus_mode: state.focus_mode,
            frame_p50_us: timing.interval_p50_us,
            window_count,
            progress: qs_progress,
            glass_opacity: state.glass_opacity,
            reduce_transparency: state.reduce_transparency,
        });

        let ov_progress = state.overview_anim.current();
        let show_ov = !show_launcher && !show_qs && (state.overview_active || ov_progress > 0.01);
        let overview_spaces = if show_ov {
            state.overview_workspace_spaces()
        } else {
            Vec::new()
        };
        let ov_view = show_ov.then_some(OverviewRenderState {
            active_workspace: active_ws,
            layout_mode: layout_str,
            windows: &dock_windows,
            spaces: &overview_spaces,
            progress: ov_progress,
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

        let settings_view = state.settings_active.then(|| state.settings_render_state());
        let context_menu_view = state
            .context_menu
            .as_ref()
            .filter(|ctx| ctx.output_id == output_entry.id);

        let overlay = if let Some(pk) = polkit_view.as_ref() {
            ShellOverlayView::Polkit(pk)
        } else if let Some(pt) = portal_view.as_ref() {
            ShellOverlayView::Portal(pt)
        } else if let Some(sw) = switcher_view.as_ref() {
            ShellOverlayView::Switcher(sw)
        } else if let Some(spot) = spotlight_view.as_ref() {
            ShellOverlayView::Spotlight(spot)
        } else if let Some(settings) = settings_view.as_ref() {
            ShellOverlayView::Settings(settings)
        } else if let Some(qs) = qs_view.as_ref() {
            ShellOverlayView::QuickSettings(qs)
        } else if let Some(ctx) = context_menu_view {
            ShellOverlayView::ContextMenu(ctx)
        } else if let Some(ov) = ov_view.as_ref() {
            ShellOverlayView::Overview(ov)
        } else if let Some(pick) = state.portal_pick_color.as_ref() {
            ShellOverlayView::PickColor(&pick.app_id)
        } else {
            ShellOverlayView::None
        };

        // Render priority modals, interactive overlays, and notification toasts above Layer::Overlay/Top and fullscreen windows.
        render_list.extend(
            crate::shell_render::generate_shell_foreground_overlay_elements(
                text_cache,
                shape_cache,
                renderer,
                logical_size,
                &top_telemetry,
                overlay,
                &toast_views,
            ),
        );

        let top_layer_surfaces_present = push_layer_elements(
            &mut render_list,
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
        let hide_chrome_for_fullscreen = fullscreen_win.is_some()
            || state.should_hide_shell_chrome_for_fullscreen_on(output_entry.id);

        if !top_layer_surfaces_present && !hide_chrome_for_fullscreen {
            render_list.extend(
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
                ),
            );
        }

        let overview_overlay = if matches!(overlay, ShellOverlayView::ContextMenu(_))
            && let Some(ov) = ov_view.as_ref()
        {
            ShellOverlayView::Overview(ov)
        } else {
            overlay
        };
        render_list.extend(crate::shell_render::generate_shell_overview_elements(
            text_cache,
            shape_cache,
            renderer,
            logical_size,
            &top_telemetry,
            overview_overlay,
        ));

        fullscreen_win
    } else {
        let top_layer_surfaces_present = push_layer_elements(
            &mut render_list,
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
        if !top_layer_surfaces_present
            && fullscreen_win.is_none()
            && !state.should_hide_shell_chrome_for_fullscreen_on(output_entry.id)
            && !state.is_session_locked()
            && state.polkit_prompt.is_none()
            && state.portal_prompt.is_none()
            && let Some(ref ctx) = state.context_menu
            && ctx.output_id == output_entry.id
        {
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
        }
        fullscreen_win
    };

    // Render managed windows belonging to this output and their SSD titlebars in top-to-bottom order
    for &(win_id, rect) in output_windows.iter().rev() {
        if state.is_window_minimized(win_id) {
            continue;
        }
        if let Some(fs_id) = fullscreen_win
            && win_id != fs_id
        {
            continue;
        }
        let global_origin = state
            .animations
            .animated_window_origin(win_id.0)
            .unwrap_or((rect.x, rect.y));
        let origin = (
            global_origin.0 - output_loc.x,
            global_origin.1 - output_loc.y,
        );
        let is_focused = focused_win == Some(win_id);
        let is_fullscreen = fullscreen_win == Some(win_id) || state.is_window_fullscreen(win_id);
        let has_ssd = !is_fullscreen && !state.window_uses_csd(win_id);
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
                rusty_wm::Rect::new(origin.0, origin.1, sz.w, sz.h + titlebar_h)
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

        let ssd_hovered = state.hovered_traffic_light_window == Some(win_id);
        if has_ssd && !has_open_popup {
            let title_text = state.window_title(win_id);
            render_list.extend(
                crate::shell_render::generate_window_ssd_composite_elements_with_hover(
                    text_cache,
                    shape_cache,
                    renderer,
                    frame_rect,
                    &title_text,
                    is_focused,
                    ssd_hovered,
                ),
            );
        }

        if let Some(ref sw) = smithay_win {
            let geo_loc = sw.geometry().loc;
            let client_loc = Point::<i32, Logical>::from((
                origin.0 - geo_loc.x,
                origin.1 + titlebar_h - geo_loc.y,
            ))
            .to_physical_precise_round(output_scale);
            render_list.extend(
                <smithay::desktop::Window as AsRenderElements<GlesRenderer>>::render_elements::<
                    WaylandSurfaceRenderElement<GlesRenderer>,
                >(
                    sw,
                    renderer,
                    client_loc,
                    smithay::utils::Scale::from(output_scale),
                    1.0,
                )
                .into_iter()
                .map(CompositorRenderElement::Surface),
            );
        }

        if has_ssd && has_open_popup {
            let title_text = state.window_title(win_id);
            render_list.extend(
                crate::shell_render::generate_window_ssd_composite_elements_with_hover(
                    text_cache,
                    shape_cache,
                    renderer,
                    frame_rect,
                    &title_text,
                    is_focused,
                    ssd_hovered,
                ),
            );
        }

        if !is_fullscreen {
            render_list.extend(crate::shell_render::generate_window_shadow_elements(
                shape_cache,
                renderer,
                frame_rect,
                is_focused,
            ));
        }
    }

    let _ = push_layer_elements(
        &mut render_list,
        &smithay_output,
        renderer,
        &[Layer::Bottom, Layer::Background],
        output_scale,
    );

    if fullscreen_win.is_none()
        && let Some(wallpaper) = shape_cache.wallpaper_element(
            renderer,
            Size::<i32, Logical>::from((output_geo.size.w.max(1), output_geo.size.h.max(1))),
            state.user_config.wallpaper.as_deref(),
        )
    {
        render_list.push(CompositorRenderElement::Memory(wallpaper));
    }

    scale_render_elements_for_output(render_list, output_scale)
}

pub(crate) fn process_pending_screenshot(
    renderer: &mut GlesRenderer,
    render_list: &[CompositorRenderElement],
    state: &mut RustyCompositorState,
    _size: smithay::utils::Size<i32, smithay::utils::Physical>,
) {
    let Some(req) = state.pending_screenshot.take() else {
        return;
    };
    let primary = state.output_map.primary();
    let output_scale = primary.scale;
    let capture_size = primary.transformed_physical_size();
    let crop = match req {
        crate::screenshot::ScreenshotRequest::FullScreen => None,
        crate::screenshot::ScreenshotRequest::FocusedWindow => state
            .wm
            .focused_window()
            .and_then(|id| state.wm.window(id))
            .map(|w| w.geometry),
    };
    match crate::screenshot::capture_render_list_rgba(
        renderer,
        render_list,
        capture_size,
        output_scale,
        crop,
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
