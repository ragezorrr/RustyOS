//! DRM connector discovery, hardware capability probing, GBM/EGL init, and session env sync.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::os::unix::io::OwnedFd;
use std::path::{Path, PathBuf};

use smithay::{
    backend::{
        allocator::{
            Allocator, Fourcc, Modifier,
            dmabuf::{Dmabuf, DmabufAllocator},
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{DrmDevice, DrmDeviceFd, DrmNode, NodeType},
        egl::{EGLContext, EGLDevice, EGLDisplay},
        renderer::{
            Bind, Color32F, Frame, ImportDma, Renderer, gles::GlesRenderer, sync::SyncPoint,
        },
        udev::UdevBackend,
    },
    output::{Mode as OutputMode, Scale},
    reexports::drm::{
        ClientCapability, Device as BasicDevice, DriverCapability,
        control::{Device as ControlDevice, PlaneType, connector, crtc},
    },
    utils::{DeviceFd, Rectangle, Size, Transform},
    wayland::drm_syncobj::supports_syncobj_eventfd,
};
use tracing::{debug, info};

use super::{
    ConnectorReport, DrmHardwareReport, DrmOutputState, PrimaryConnectorSetup,
    collect_candidate_drm_paths, select_best_drm_mode,
};
use crate::{CompositorError, state::RustyCompositorState};

/// Discovers all connected KMS connectors and their candidate CRTC/modes (`RUSTYOS-040`).
#[must_use]
pub fn find_all_connected_connectors(drm_device: &DrmDevice) -> Vec<PrimaryConnectorSetup> {
    find_all_connected_connectors_with_prefs(drm_device, true, None)
}

pub(super) fn find_all_connected_connectors_with_prefs(
    drm_device: &DrmDevice,
    prefer_max_refresh: bool,
    active_outputs: Option<&HashMap<crtc::Handle, DrmOutputState>>,
) -> Vec<PrimaryConnectorSetup> {
    let Ok(resources) = drm_device.resource_handles() else {
        return Vec::new();
    };

    let mut setups = Vec::new();
    let mut claimed_crtcs = HashSet::new();
    if let Some(active) = active_outputs {
        for (&crtc, out_state) in active {
            if drm_device
                .get_connector(out_state.connector, false)
                .is_ok_and(|info| info.state() == connector::State::Connected)
            {
                claimed_crtcs.insert(crtc);
            }
        }
    }

    for &conn_handle in resources.connectors() {
        let Ok(info) = drm_device.get_connector(conn_handle, false) else {
            continue;
        };
        if info.state() != connector::State::Connected {
            continue;
        }
        let Some(mode) = select_best_drm_mode(info.modes(), prefer_max_refresh) else {
            continue;
        };

        let active_crtc = active_outputs.and_then(|active| {
            active
                .iter()
                .find(|(_, out_state)| out_state.connector == conn_handle)
                .map(|(&crtc, _)| crtc)
        });
        let candidate_crtc = active_crtc.or_else(|| {
            info.current_encoder()
                .into_iter()
                .chain(info.encoders().iter().copied())
                .find_map(|enc_handle| {
                    let enc = drm_device.get_encoder(enc_handle).ok()?;
                    enc.crtc()
                        .filter(|c| !claimed_crtcs.contains(c))
                        .or_else(|| {
                            resources
                                .filter_crtcs(enc.possible_crtcs())
                                .into_iter()
                                .find(|c| !claimed_crtcs.contains(c))
                        })
                })
        });

        if let Some(crtc) = candidate_crtc {
            claimed_crtcs.insert(crtc);
            let (w, h) = mode.size();
            let vrefresh = mode.vrefresh().max(1);
            let connector_name = format!("{:?}-{}", info.interface(), info.interface_id());
            setups.push(PrimaryConnectorSetup {
                connector: conn_handle,
                crtc,
                mode,
                connector_name,
                pixel_size: Size::from((i32::from(w), i32::from(h))),
                vrefresh,
            });
        }
    }

    setups
}

/// Dynamically rescans DRM connectors and updates compositor outputs (`RUSTYOS-040`).
///
/// Discovers newly plugged in displays and registers them via [`RustyCompositorState::add_output`],
/// and removes disconnected displays via [`RustyCompositorState::remove_output`] while
/// safely migrating windows to remaining displays.
pub fn rescan_drm_connectors(drm_device: &DrmDevice, state: &mut RustyCompositorState) -> usize {
    let connected = find_all_connected_connectors(drm_device);
    let mut changes = 0;

    // Detect new connectors not yet in state.output_map
    for setup in &connected {
        let already_present = state
            .output_map
            .entries()
            .iter()
            .any(|e| e.name == setup.connector_name);
        if !already_present {
            let next_loc = state.output_map.next_adjacent_location();
            let refresh_milli_hz =
                i32::try_from(setup.vrefresh.saturating_mul(1000)).unwrap_or(60_000);
            let mode = OutputMode {
                size: setup.pixel_size,
                refresh: refresh_milli_hz,
            };
            let props = smithay::output::PhysicalProperties {
                size: (0, 0).into(),
                subpixel: smithay::output::Subpixel::Unknown,
                make: "DRM/KMS".to_owned(),
                model: setup.connector_name.clone(),
            };
            let scale = state.user_config.scale_for_output(&setup.connector_name);
            let new_id = state.add_output(
                setup.connector_name.clone(),
                props,
                mode,
                Scale::Fractional(scale),
                next_loc,
            );
            tracing::info!(
                id = new_id.0,
                name = %setup.connector_name,
                loc = ?next_loc,
                "Hotplug: Attached new DRM display"
            );
            changes += 1;
        }
    }

    // Detect disconnected connectors that are currently mapped
    let to_remove: Vec<rusty_wm::OutputId> = state
        .output_map
        .entries()
        .iter()
        .filter(|e| {
            e.name.starts_with("DP-")
                || e.name.starts_with("HDMI-")
                || e.name.starts_with("eDP-")
                || e.name.starts_with("DVI-")
        })
        .filter(|e| !connected.iter().any(|c| c.connector_name == e.name))
        .map(|e| e.id)
        .collect();

    for id in to_remove {
        if state.remove_output(id) {
            tracing::info!(id = id.0, "Hotplug: Detached disconnected DRM display");
            changes += 1;
        }
    }

    changes
}

pub(super) fn probe_primary_drm_device(
    kms_master_status: &str,
) -> Result<DrmHardwareReport, CompositorError> {
    let primary_path = discover_primary_drm_path()?;
    let primary_node = DrmNode::from_path(&primary_path)
        .map_err(|err| CompositorError::BackendInit(format!("invalid DRM node: {err}")))?;

    let render_path = primary_node
        .node_with_type(NodeType::Render)
        .and_then(Result::ok)
        .and_then(|n| n.dev_path());

    let drm_fd = open_drm_device_fd(&primary_path)?;

    let universal_planes = drm_fd
        .set_client_capability(ClientCapability::UniversalPlanes, true)
        .is_ok();
    let atomic_modesetting = drm_fd
        .set_client_capability(ClientCapability::Atomic, true)
        .is_ok();
    let monotonic_timestamps = drm_fd
        .get_driver_capability(DriverCapability::MonotonicTimestamp)
        .unwrap_or(0)
        == 1;
    let async_page_flip = drm_fd
        .get_driver_capability(DriverCapability::ASyncPageFlip)
        .unwrap_or(0)
        == 1;
    let syncobj_eventfd = supports_syncobj_eventfd(&drm_fd);
    let cursor_w = drm_fd
        .get_driver_capability(DriverCapability::CursorWidth)
        .unwrap_or(64);
    let cursor_h = drm_fd
        .get_driver_capability(DriverCapability::CursorHeight)
        .unwrap_or(64);

    let resources = drm_fd.resource_handles().map_err(|err| {
        CompositorError::BackendInit(format!("DRM resource_handles error: {err}"))
    })?;
    let crtc_count = resources.crtcs().len();
    let plane_count = drm_fd.plane_handles().map_or(0, |planes| planes.len());
    let cursor_plane_count = count_drm_cursor_planes(&drm_fd, universal_planes);

    let mut connectors = Vec::new();
    for &conn_handle in resources.connectors() {
        if let Ok(info) = drm_fd.get_connector(conn_handle, false) {
            let name = format!("{:?}-{}", info.interface(), info.interface_id());
            let status = match info.state() {
                connector::State::Connected => "Connected".to_owned(),
                connector::State::Disconnected => "Disconnected".to_owned(),
                connector::State::Unknown => "Unknown".to_owned(),
            };
            let modes = info.modes();
            let preferred_mode = modes.first().map(|m| {
                let (w, h) = m.size();
                (w, h, m.vrefresh())
            });
            connectors.push(ConnectorReport {
                name,
                status,
                preferred_mode,
                mode_count: modes.len(),
            });
        }
    }

    // Verify GBM + EGL + GLES 3.2 + DMA-BUF allocation, rendering, and texture import.
    let render_target_path = render_path.as_deref().unwrap_or(&primary_path);
    let (dmabuf_render_format_count, dmabuf_texture_format_count, gbm_egl_dmabuf_test_passed) =
        verify_gbm_egl_dmabuf_pipeline(render_target_path)?;

    Ok(DrmHardwareReport {
        primary_node: primary_path.display().to_string(),
        render_node: render_path.map(|p| p.display().to_string()),
        atomic_modesetting,
        universal_planes,
        monotonic_timestamps,
        async_page_flip,
        syncobj_eventfd,
        cursor_size: (cursor_w, cursor_h),
        crtc_count,
        plane_count,
        cursor_plane_count,
        connectors,
        dmabuf_render_format_count,
        dmabuf_texture_format_count,
        gbm_egl_dmabuf_test_passed,
        kms_master_status: kms_master_status.to_owned(),
    })
}

pub(super) fn count_drm_cursor_planes(drm_fd: &DrmDeviceFd, universal_planes: bool) -> usize {
    if !universal_planes {
        return 0;
    }
    let Ok(planes) = drm_fd.plane_handles() else {
        return 0;
    };
    let cursor_val = PlaneType::Cursor as u64;
    planes
        .into_iter()
        .filter(|&plane| {
            let Ok(props) = drm_fd.get_properties(plane) else {
                return false;
            };
            let (ids, vals) = props.as_props_and_values();
            ids.iter().zip(vals.iter()).any(|(&id, &val)| {
                drm_fd
                    .get_property(id)
                    .is_ok_and(|info| info.name().to_str() == Ok("type") && val == cursor_val)
            })
        })
        .count()
}

pub(super) fn discover_primary_drm_path() -> Result<PathBuf, CompositorError> {
    let udev = UdevBackend::new("seat0").ok();
    let candidates = collect_candidate_drm_paths(udev.as_ref());
    if let Some(best) = candidates.into_iter().next() {
        return Ok(best);
    }

    Err(CompositorError::BackendInit(
        "no /dev/dri/card* device found".to_owned(),
    ))
}

pub(super) fn open_drm_device_fd(path: &Path) -> Result<DrmDeviceFd, CompositorError> {
    let file = File::options()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| {
            CompositorError::BackendInit(format!("failed to open {}: {err}", path.display()))
        })?;
    Ok(DrmDeviceFd::new(DeviceFd::from(OwnedFd::from(file))))
}

/// Initializes `GbmDevice`, `EGLDisplay`, and `GlesRenderer` on the target DRM node,
/// allocates a hardware `DMA-BUF` (`Argb8888`), renders a test frame, and imports the `DMA-BUF`.
pub(super) fn verify_gbm_egl_dmabuf_pipeline(
    drm_path: &Path,
) -> Result<(usize, usize, bool), CompositorError> {
    let fd = open_drm_device_fd(drm_path)?;
    let gbm_device = GbmDevice::new(fd)
        .map_err(|err| CompositorError::BackendInit(format!("GbmDevice::new failed: {err}")))?;
    let mut allocator = DmabufAllocator(GbmAllocator::new(
        gbm_device.clone(),
        GbmBufferFlags::RENDERING,
    ));

    let (egl_display, mut renderer) = init_gbm_egl_renderer(&gbm_device, Some(drm_path))?;

    let render_format_count = egl_display.dmabuf_render_formats().iter().count();
    let texture_format_count = egl_display.dmabuf_texture_formats().iter().count();

    let mut dmabuf: Dmabuf = allocator
        .create_buffer(256, 256, Fourcc::Argb8888, &[Modifier::Linear])
        .map_err(|err| {
            CompositorError::BackendInit(format!("GBM DMA-BUF allocation failed: {err}"))
        })?;

    // 1. Bind the GBM-allocated DMA-BUF as an offscreen render target and draw a frame on the GPU.
    {
        let mut target = renderer.bind(&mut dmabuf).map_err(|err| {
            CompositorError::BackendInit(format!("failed to bind DMA-BUF to GLES renderer: {err}"))
        })?;
        let mut frame = renderer
            .render(&mut target, (256, 256).into(), Transform::Normal)
            .map_err(|err| {
                CompositorError::BackendInit(format!("GLES render begin failed: {err}"))
            })?;
        frame
            .clear(
                Color32F::new(0.06, 0.04, 0.10, 1.0),
                &[Rectangle::from_size((256, 256).into())],
            )
            .map_err(|err| CompositorError::BackendInit(format!("GLES clear failed: {err}")))?;
        let sync: SyncPoint = frame
            .finish()
            .map_err(|err| CompositorError::BackendInit(format!("GLES finish failed: {err}")))?;
        let _ = sync.wait();
    }

    // 2. Verify that the rendered DMA-BUF can be imported back as a GPU texture (`ImportDma`).
    let _imported_texture = renderer
        .import_dmabuf(&dmabuf, None)
        .map_err(|err| CompositorError::BackendInit(format!("DMA-BUF import failed: {err}")))?;

    Ok((render_format_count, texture_format_count, true))
}

pub(super) fn drm_path_matches_target(device_path: &Path, target_path: &Path) -> bool {
    if device_path == target_path {
        return true;
    }
    match (fs::canonicalize(device_path), fs::canonicalize(target_path)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Isolated wrapper around Smithay's `unsafe` `EGLDisplay::new` and `GlesRenderer::new` constructors.
#[allow(unsafe_code)]
pub(super) fn init_gbm_egl_renderer(
    gbm_device: &GbmDevice<DrmDeviceFd>,
    preferred_drm_path: Option<&Path>,
) -> Result<(EGLDisplay, GlesRenderer), CompositorError> {
    let target_render_path = preferred_drm_path
        .and_then(|p| DrmNode::from_path(p).ok())
        .and_then(|n| n.node_with_type(NodeType::Render).and_then(Result::ok))
        .and_then(|n| n.dev_path());

    // SAFETY: `GbmDevice<DrmDeviceFd>` is a valid, open GBM device backed by a ref-counted
    // `DrmDeviceFd` that outlives the `EGLDisplay` creation or is retained internally by Smithay.
    // Bind EGL to the opened `GbmDevice` first so multi-GPU/simpledrm setups cannot hijack EGLDevice.
    let egl_display = unsafe {
        match EGLDisplay::new(gbm_device.clone()) {
            Ok(display) => display,
            Err(gbm_err) => {
                let mut matched_display = None;
                if let Ok(devices) = EGLDevice::enumerate() {
                    let all_devices: Vec<_> = devices.collect();
                    let ordered = all_devices.iter().filter(|dev| {
                        let Ok(dev_path) = dev.drm_device_path() else {
                            return false;
                        };
                        preferred_drm_path.is_some_and(|p| drm_path_matches_target(&dev_path, p))
                            || target_render_path
                                .as_deref()
                                .is_some_and(|rp| drm_path_matches_target(&dev_path, rp))
                    });
                    for egl_dev in ordered {
                        if let Ok(display) = EGLDisplay::new(egl_dev.clone()) {
                            matched_display = Some(display);
                            break;
                        }
                    }
                    if matched_display.is_none() && preferred_drm_path.is_none() {
                        for egl_dev in &all_devices {
                            if egl_dev.drm_device_path().is_ok()
                                && let Ok(display) = EGLDisplay::new(egl_dev.clone())
                            {
                                matched_display = Some(display);
                                break;
                            }
                        }
                    }
                }
                matched_display.ok_or_else(|| {
                    CompositorError::BackendInit(format!("EGLDisplay::new failed: {gbm_err}"))
                })?
            }
        }
    };

    let egl_context = EGLContext::new(&egl_display)
        .map_err(|err| CompositorError::BackendInit(format!("EGLContext::new failed: {err}")))?;

    // SAFETY: `egl_context` was freshly created from `egl_display` on this thread and is not
    // current on any other thread.
    let renderer = unsafe {
        GlesRenderer::new(egl_context).map_err(|err| {
            CompositorError::BackendInit(format!("GlesRenderer::new failed: {err}"))
        })?
    };

    Ok((egl_display, renderer))
}

pub(super) fn log_hardware_report(report: &DrmHardwareReport) {
    info!(
        primary_node = %report.primary_node,
        render_node = ?report.render_node,
        atomic = report.atomic_modesetting,
        universal_planes = report.universal_planes,
        monotonic_ts = report.monotonic_timestamps,
        syncobj_eventfd = report.syncobj_eventfd,
        cursor_size = ?report.cursor_size,
        crtcs = report.crtc_count,
        planes = report.plane_count,
        cursor_planes = report.cursor_plane_count,
        dmabuf_render_formats = report.dmabuf_render_format_count,
        dmabuf_texture_formats = report.dmabuf_texture_format_count,
        gbm_egl_dmabuf_ok = report.gbm_egl_dmabuf_test_passed,
        kms_master = %report.kms_master_status,
        "DRM/KMS & GBM/EGL hardware capability discovery complete"
    );
    for conn in &report.connectors {
        info!(
            connector = %conn.name,
            status = %conn.status,
            preferred_mode = ?conn.preferred_mode,
            mode_count = conn.mode_count,
            "Discovered KMS connector"
        );
    }
}

/// Builds `dbus-update-activation-environment --systemd` arguments for native `--drm` sessions
/// (`RUSTYOS-078`, ADR-0038).
#[must_use]
pub(crate) fn build_session_activation_env_args(
    wayland_display: &str,
    x11_display: Option<u32>,
    portal_dir: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        String::from("--systemd"),
        format!("WAYLAND_DISPLAY={}", wayland_display.trim()),
        String::from("XDG_CURRENT_DESKTOP=RustyOS"),
        String::from("XDG_SESSION_DESKTOP=RustyOS"),
        String::from("XDG_SESSION_TYPE=wayland"),
    ];
    if let Some(num) = x11_display {
        args.push(format!("DISPLAY=:{num}"));
    }
    if let Some(dir) = portal_dir.map(str::trim).filter(|d| !d.is_empty()) {
        args.push(format!("XDG_DESKTOP_PORTAL_DIR={dir}"));
    }
    args
}

/// Publishes native `--drm` session variables into `dbus-update-activation-environment` off the
/// compositor event loop so XDG portals and `systemd --user` services resolve `WAYLAND_DISPLAY`
/// and `DISPLAY` without stalling KMS presentation (`RUSTYOS-078`).
pub(crate) fn sync_drm_activation_environment(wayland_display: &str, x11_display: Option<u32>) {
    let clean_wl = wayland_display.trim();
    if clean_wl.is_empty() {
        return;
    }
    let portal_dir = std::env::var("XDG_DESKTOP_PORTAL_DIR").ok();
    let args = build_session_activation_env_args(clean_wl, x11_display, portal_dir.as_deref());
    let _ = std::thread::Builder::new()
        .name(String::from("drm-env-sync"))
        .spawn(move || {
            match std::process::Command::new("dbus-update-activation-environment")
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
            {
                Ok(status) if status.success() => {
                    debug!("Synced RustyOS --drm session environment to D-Bus/systemd");
                }
                Ok(status) => {
                    debug!(code = ?status.code(), "dbus-update-activation-environment exited non-zero");
                }
                Err(err) => {
                    debug!(error = %err, "dbus-update-activation-environment unavailable");
                }
            }
        });
}
