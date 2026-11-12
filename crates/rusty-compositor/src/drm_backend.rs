//! Native DRM/KMS, GBM, EGL, DMA-BUF, and `libinput` backend (`RUSTYOS-006`, `RUSTYOS-028`).
//!
//! Provides two execution levels:
//! 1. **Runtime Capability Discovery & Offscreen GBM/EGL/DMA-BUF Verification (`--drm-probe`)**:
//!    Safely inspects `/dev/dri/card*` and `/dev/dri/renderD*` without stealing DRM Master from
//!    an active desktop session, enumerates KMS connectors/modes/planes/capabilities, initializes
//!    `GbmDevice` + `EGLDisplay` + `GlesRenderer`, allocates a hardware `DMA-BUF` buffer via GBM,
//!    renders a test frame on the GPU, and verifies `DMA-BUF` import.
//! 2. **Full Native TTY KMS Session (`--drm` / `--kms`)**:
//!    Initializes `LibSeatSession`, `UdevBackend`, `DrmDevice` + `DrmSurface` + `DrmCompositor`,
//!    and `LibinputInputBackend` (relative & absolute pointer + keyboard + VT switch handling)
//!    for bare-metal TTY Wayland + XWayland rendering, with automatic diagnostic fallback to
//!    capability probing when invoked inside an active graphical session where DRM Master is held.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::os::unix::io::OwnedFd;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rusty_ipc::{IpcServer, default_ipc_socket_path};
use smithay::{
    backend::{
        allocator::{
            Allocator, Fourcc, Modifier,
            dmabuf::{Dmabuf, DmabufAllocator},
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType,
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
        },
        egl::{EGLContext, EGLDevice, EGLDisplay},
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            Bind, Color32F, Frame, ImportDma, Renderer, gles::GlesRenderer, sync::SyncPoint,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::UdevBackend,
    },
    output::{Mode as OutputMode, Scale},
    reexports::{
        calloop::{
            EventLoop,
            timer::{TimeoutAction, Timer},
        },
        drm::{
            ClientCapability, Device as BasicDevice, DriverCapability,
            control::{
                Device as ControlDevice, Mode as ControlMode, ModeTypeFlags, connector, crtc,
            },
        },
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::Display,
    },
    utils::{DeviceFd, Logical, Physical, Point, Rectangle, Size, Transform},
    wayland::socket::ListeningSocketSource,
};
use tracing::{info, warn};

use crate::{
    BackendMode, BootstrapReport, CompositorConfig, CompositorError,
    state::{PRIMARY_OUTPUT_ID, RustyCompositorState},
};

/// Summary of a discovered KMS display connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorReport {
    /// Connector name (e.g. `eDP-1`, `DP-1`, `HDMI-A-1`).
    pub name: String,
    /// Connection state (`Connected`, `Disconnected`, `Unknown`).
    pub status: String,
    /// Preferred or first advertised mode (`width x height @ refresh_hz`).
    pub preferred_mode: Option<(u16, u16, u32)>,
    /// Total number of modes advertised by the connector.
    pub mode_count: usize,
}

/// Comprehensive hardware capability report for a DRM/GBM/EGL device (`RUSTYOS-006`).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrmHardwareReport {
    /// Primary DRM node path (e.g. `/dev/dri/card1`).
    pub primary_node: String,
    /// Render DRM node path if available (e.g. `/dev/dri/renderD128`).
    pub render_node: Option<String>,
    /// Whether the kernel driver supports `ClientCapability::Atomic`.
    pub atomic_modesetting: bool,
    /// Whether the kernel driver supports `ClientCapability::UniversalPlanes`.
    pub universal_planes: bool,
    /// Whether the driver reports monotonic vblank/page-flip timestamps.
    pub monotonic_timestamps: bool,
    /// Whether async page-flip is supported by the driver.
    pub async_page_flip: bool,
    /// Recommended hardware cursor dimensions `(width, height)`.
    pub cursor_size: (u64, u64),
    /// Number of CRTC scanout engines on the GPU.
    pub crtc_count: usize,
    /// Total number of DRM planes exposed by the device.
    pub plane_count: usize,
    /// Discovered display connectors and their preferred modes.
    pub connectors: Vec<ConnectorReport>,
    /// Number of `DMA-BUF` render formats advertised by `EGLDisplay`.
    pub dmabuf_render_format_count: usize,
    /// Number of `DMA-BUF` texture sampling formats advertised by `EGLDisplay`.
    pub dmabuf_texture_format_count: usize,
    /// Whether an offscreen GBM `DMA-BUF` allocation + GLES 3.2 render + `DMA-BUF` import succeeded.
    pub gbm_egl_dmabuf_test_passed: bool,
    /// Note regarding KMS DRM Master acquisition status.
    pub kms_master_status: String,
}

impl DrmHardwareReport {
    /// Serializes the hardware capability report to formatted JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(1024);
        let _ = writeln!(out, "{{");
        let _ = writeln!(out, "  \"primary_node\": \"{}\",", self.primary_node);
        match &self.render_node {
            Some(rn) => {
                let _ = writeln!(out, "  \"render_node\": \"{rn}\",");
            }
            None => {
                let _ = writeln!(out, "  \"render_node\": null,");
            }
        }
        let _ = writeln!(
            out,
            "  \"atomic_modesetting\": {},",
            self.atomic_modesetting
        );
        let _ = writeln!(out, "  \"universal_planes\": {},", self.universal_planes);
        let _ = writeln!(
            out,
            "  \"monotonic_timestamps\": {},",
            self.monotonic_timestamps
        );
        let _ = writeln!(out, "  \"async_page_flip\": {},", self.async_page_flip);
        let _ = writeln!(
            out,
            "  \"cursor_size\": [{}, {}],",
            self.cursor_size.0, self.cursor_size.1
        );
        let _ = writeln!(out, "  \"crtc_count\": {},", self.crtc_count);
        let _ = writeln!(out, "  \"plane_count\": {},", self.plane_count);
        let _ = writeln!(
            out,
            "  \"dmabuf_render_format_count\": {},",
            self.dmabuf_render_format_count
        );
        let _ = writeln!(
            out,
            "  \"dmabuf_texture_format_count\": {},",
            self.dmabuf_texture_format_count
        );
        let _ = writeln!(
            out,
            "  \"gbm_egl_dmabuf_test_passed\": {},",
            self.gbm_egl_dmabuf_test_passed
        );
        let _ = writeln!(
            out,
            "  \"kms_master_status\": \"{}\",",
            self.kms_master_status
        );
        let _ = writeln!(out, "  \"connectors\": [");
        for (idx, conn) in self.connectors.iter().enumerate() {
            let comma = if idx + 1 < self.connectors.len() {
                ","
            } else {
                ""
            };
            let mode_str = conn.preferred_mode.map_or_else(
                || "null".to_owned(),
                |(w, h, hz)| format!("\"{w}x{h}@{hz}Hz\""),
            );
            let _ = writeln!(
                out,
                "    {{ \"name\": \"{}\", \"status\": \"{}\", \"preferred_mode\": {}, \"mode_count\": {} }}{comma}",
                conn.name, conn.status, mode_str, conn.mode_count
            );
        }
        let _ = writeln!(out, "  ]");
        let _ = write!(out, "}}");
        out
    }
}

/// Executes the non-invasive hardware capability discovery and offscreen GBM/EGL/DMA-BUF probe.
///
/// # Errors
///
/// Returns [`CompositorError`] if no accessible DRM device is found or if GBM/EGL initialization
/// fails on the hardware.
pub fn run_drm_probe(config: &CompositorConfig) -> Result<BootstrapReport, CompositorError> {
    let report = probe_primary_drm_device("probe-only (DRM Master not requested)")?;
    log_hardware_report(&report);

    if let Some(path) = &config.metrics_out {
        fs::write(path, report.to_json())
            .map_err(|err| CompositorError::MetricsExport(err.to_string()))?;
        info!(path = %path, "Exported DRM/GBM/EGL capability JSON report");
    }

    Ok(BootstrapReport {
        version: env!("CARGO_PKG_VERSION"),
        backend: BackendMode::DrmProbe,
        socket_name: None,
        frame_timing: None,
        drm_report: Some(report),
    })
}

type RustyDrmCompositor =
    DrmCompositor<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

#[derive(Debug, Clone)]
struct PrimaryConnectorSetup {
    connector: connector::Handle,
    crtc: crtc::Handle,
    mode: ControlMode,
    connector_name: String,
    pixel_size: Size<i32, Physical>,
    vrefresh: u32,
}

enum SessionStartOutcome {
    Completed(Box<BootstrapReport>),
    DeferToProbe(String),
}

struct DrmRuntime {
    drm_device: DrmDevice,
    libinput: Libinput,
    renderer: GlesRenderer,
    drm_compositor: RustyDrmCompositor,
    text_cache: crate::text::TextCache,
    shape_cache: crate::surfaces::UiShapeCache,
    pixel_size: Size<i32, Physical>,
    total_output_pixels: u64,
    paused: bool,
    vblank_pending: bool,
    vblank_queued_at: Option<Instant>,
    needs_redraw: bool,
    frame_start: Option<Instant>,
    rendered_frames: u64,
    max_frames: Option<u64>,
}

impl DrmRuntime {
    fn render_and_queue(&mut self, state: &mut RustyCompositorState) {
        if !state.running || self.paused {
            return;
        }
        if self.vblank_pending {
            let timed_out = self
                .vblank_queued_at
                .is_some_and(|queued| queued.elapsed() >= Duration::from_millis(500));
            if timed_out {
                warn!("DRM VBlank watchdog (>500ms); resetting compositor pipeline");
                self.vblank_pending = false;
                self.vblank_queued_at = None;
                let _ = self.drm_compositor.clear();
                let _ = self.drm_compositor.reset_state();
                self.drm_compositor.reset_buffers();
                self.drm_compositor.reset_buffer_ages();
            } else {
                self.needs_redraw = true;
                return;
            }
        }

        let render_start = Instant::now();
        state.tick_animations();
        state.expire_notifications();

        if state.pending_screenshot.is_some() {
            let snap_list = crate::winit_backend::build_frame_render_list(
                &mut self.renderer,
                &mut self.text_cache,
                &mut self.shape_cache,
                state,
                self.pixel_size,
                false,
            );
            crate::winit_backend::process_pending_screenshot(
                &mut self.renderer,
                &snap_list,
                state,
                self.pixel_size,
            );
        }

        let render_list = crate::winit_backend::build_frame_render_list(
            &mut self.renderer,
            &mut self.text_cache,
            &mut self.shape_cache,
            state,
            self.pixel_size,
            true,
        );

        match self.drm_compositor.render_frame(
            &mut self.renderer,
            &render_list,
            crate::winit_backend::BACKGROUND_COLOR,
            FrameFlags::DEFAULT,
        ) {
            Ok(result) => {
                if result.is_empty {
                    self.needs_redraw = false;
                } else if let Err(err) = self.drm_compositor.queue_frame(()) {
                    warn!(error = ?err, "Failed to queue DRM/KMS page flip");
                    self.needs_redraw = true;
                } else {
                    self.vblank_pending = true;
                    self.vblank_queued_at = Some(Instant::now());
                    self.needs_redraw = false;
                    self.frame_start = Some(render_start);
                }
            }
            Err(err) => {
                warn!(error = ?err, "DRM/KMS render_frame error");
                self.needs_redraw = true;
            }
        }
    }

    fn handle_vblank(&mut self, state: &mut RustyCompositorState) {
        if let Err(err) = self.drm_compositor.frame_submitted() {
            warn!(error = ?err, "Failed to mark DRM/KMS frame as submitted");
        }
        self.vblank_pending = false;
        self.vblank_queued_at = None;

        let present_done = Instant::now();
        let frame_start = self.frame_start.take().unwrap_or(present_done);
        state.frame_timing.record_frame(
            frame_start,
            present_done,
            1,
            self.total_output_pixels,
            self.total_output_pixels,
        );

        let elapsed = state.elapsed();
        for window in state.space.elements() {
            window.send_frame(&state.output, elapsed, Some(Duration::ZERO), |_, _| {
                Some(state.output.clone())
            });
        }
        state.space.refresh();
        state.popups.cleanup();
        let _ = state.display_handle.flush_clients();

        self.rendered_frames = self.rendered_frames.saturating_add(1);
        if let Some(limit) = self.max_frames
            && self.rendered_frames >= limit
        {
            info!(
                rendered_frames = self.rendered_frames,
                "Reached configured --max-frames limit on DRM/KMS; stopping"
            );
            state.request_shutdown();
            return;
        }

        if self.needs_redraw && !self.paused && state.running {
            self.render_and_queue(state);
        }
    }
}

/// Selects the preferred DRM display mode (falling back to the first reported mode).
#[must_use]
pub fn select_preferred_drm_mode(modes: &[ControlMode]) -> Option<ControlMode> {
    modes
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| modes.first())
        .copied()
}

/// Converts a display vertical refresh rate (`Hz`) into a clamped frame pacing interval (`us`).
#[must_use]
pub fn mode_refresh_interval_us(vrefresh: u32) -> u64 {
    let hz = u64::from(vrefresh.max(1));
    (1_000_000 / hz).max(1_000)
}

/// Executes the native DRM/KMS backend (`--drm` / `--kms`).
///
/// Attempts to acquire `LibSeatSession`, DRM Master, `DrmCompositor`, and `LibinputInputBackend`
/// for full bare-metal TTY rendering. If DRM Master is unavailable because an existing desktop
/// compositor holds the seat, falls back non-invasively to full capability & GBM/EGL/DMA-BUF
/// verification.
///
/// # Errors
///
/// Returns [`CompositorError`] if running natively fails fatally or hardware probing fails.
pub fn run_native_drm_kms(config: &CompositorConfig) -> Result<BootstrapReport, CompositorError> {
    match try_run_native_drm_session(config)? {
        SessionStartOutcome::Completed(report) => Ok(*report),
        SessionStartOutcome::DeferToProbe(reason) => {
            warn!(
                reason = %reason,
                "Active graphical session holds DRM Master; falling back to non-invasive DRM/GBM/EGL hardware verification"
            );
            let master_status = format!("deferred ({reason})");
            let report = probe_primary_drm_device(&master_status)?;
            log_hardware_report(&report);

            if let Some(path) = &config.metrics_out {
                fs::write(path, report.to_json())
                    .map_err(|err| CompositorError::MetricsExport(err.to_string()))?;
                info!(path = %path, "Exported DRM/GBM/EGL capability JSON report");
            }

            Ok(BootstrapReport {
                version: env!("CARGO_PKG_VERSION"),
                backend: BackendMode::NativeDrmKms,
                socket_name: None,
                frame_timing: None,
                drm_report: Some(report),
            })
        }
    }
}

#[allow(clippy::too_many_lines)]
fn try_run_native_drm_session(
    config: &CompositorConfig,
) -> Result<SessionStartOutcome, CompositorError> {
    let Ok((mut session, session_notifier)) = LibSeatSession::new() else {
        return Ok(SessionStartOutcome::DeferToProbe(
            "libseat session open failed".to_owned(),
        ));
    };
    let seat_name = session.seat();

    let Ok(udev_backend) = UdevBackend::new(&seat_name) else {
        return Ok(SessionStartOutcome::DeferToProbe(
            "udev backend scan failed".to_owned(),
        ));
    };
    let Some((_, primary_path)) = udev_backend.device_list().find(|(_, path)| {
        DrmNode::from_path(path).is_ok_and(|node| node.ty() == NodeType::Primary)
    }) else {
        return Ok(SessionStartOutcome::DeferToProbe(
            "no primary DRM node reported by udev".to_owned(),
        ));
    };

    let primary_node = match DrmNode::from_path(primary_path) {
        Ok(node) => node,
        Err(err) => {
            return Ok(SessionStartOutcome::DeferToProbe(format!(
                "invalid primary DRM node: {err}"
            )));
        }
    };

    let fd = match session.open(
        primary_path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    ) {
        Ok(fd) => fd,
        Err(err) => {
            return Ok(SessionStartOutcome::DeferToProbe(format!(
                "session device open failed: {err:?}"
            )));
        }
    };

    let drm_fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm_device, drm_notifier) = match DrmDevice::new(drm_fd, false) {
        Ok(pair) => pair,
        Err(err) => {
            return Ok(SessionStartOutcome::DeferToProbe(format!(
                "DrmDevice::new failed: {err}"
            )));
        }
    };

    let gbm_device = match GbmDevice::new(drm_device.device_fd().clone()) {
        Ok(gbm) => gbm,
        Err(err) => {
            return Ok(SessionStartOutcome::DeferToProbe(format!(
                "GbmDevice::new failed: {err}"
            )));
        }
    };

    let (_egl_display, renderer) = match init_gbm_egl_renderer(&gbm_device) {
        Ok(pair) => pair,
        Err(err) => {
            return Ok(SessionStartOutcome::DeferToProbe(format!(
                "GBM EGL renderer init failed: {err}"
            )));
        }
    };

    let setup = match find_primary_connector_setup(&drm_device) {
        Ok(setup) => setup,
        Err(reason) => return Ok(SessionStartOutcome::DeferToProbe(reason)),
    };

    let drm_surface = match drm_device.create_surface(setup.crtc, setup.mode, &[setup.connector]) {
        Ok(surface) => surface,
        Err(err) => {
            return Ok(SessionStartOutcome::DeferToProbe(format!(
                "DRM surface creation failed: {err}"
            )));
        }
    };

    let mut libinput = Libinput::new_with_udev(LibinputSessionInterface::from(session.clone()));
    if libinput.udev_assign_seat(&seat_name).is_err() {
        return Ok(SessionStartOutcome::DeferToProbe(format!(
            "libinput failed to assign seat {seat_name}"
        )));
    }
    let libinput_backend = LibinputInputBackend::new(libinput.clone());

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

    let mut state = RustyCompositorState::new(
        &display,
        socket_name.clone(),
        event_loop.get_signal(),
        setup.pixel_size,
    )?;

    let refresh_milli_hz = i32::try_from(setup.vrefresh.saturating_mul(1000)).unwrap_or(60_000);
    let output_mode = OutputMode {
        size: setup.pixel_size,
        refresh: refresh_milli_hz,
    };
    state.output.change_current_state(
        Some(output_mode),
        Some(Transform::Normal),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    state.output.set_preferred(output_mode);
    let work_area = crate::shell_render::usable_work_area(setup.pixel_size);
    state.wm.upsert_output(PRIMARY_OUTPUT_ID, work_area);
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    state.sync_space_with_wm(serial);
    state.pointer_location = Point::<f64, Logical>::from((
        f64::from(setup.pixel_size.w) / 2.0,
        f64::from(setup.pixel_size.h) / 2.0,
    ));

    state
        .animations
        .set_reduced_motion(state.user_config.reduced_motion || config.reduced_motion);
    state.telemetry = crate::telemetry::SystemMetricsHandle::start_background();
    state.dbus_notifications =
        crate::dbus_notifications::DbusNotificationHandle::start_background();
    state.tray = crate::tray::TrayWatcherHandle::start_background();

    let gbm_allocator = GbmAllocator::new(
        gbm_device.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let gbm_exporter = GbmFramebufferExporter::new(gbm_device, Some(primary_node));
    let render_formats: Vec<_> = renderer
        .egl_context()
        .dmabuf_render_formats()
        .iter()
        .copied()
        .collect();
    let cursor_size = drm_device.cursor_size();
    let drm_compositor = DrmCompositor::new(
        &state.output,
        drm_surface,
        None,
        gbm_allocator,
        gbm_exporter,
        [Fourcc::Argb8888, Fourcc::Xrgb8888],
        render_formats,
        cursor_size,
        None,
    )
    .map_err(|err| CompositorError::BackendInit(format!("DrmCompositor::new failed: {err:?}")))?;

    let total_output_pixels = u64::try_from(setup.pixel_size.w.max(0)).unwrap_or(0)
        * u64::try_from(setup.pixel_size.h.max(0)).unwrap_or(0);
    let runtime = Rc::new(RefCell::new(DrmRuntime {
        drm_device,
        libinput,
        renderer,
        drm_compositor,
        text_cache: crate::text::TextCache::new(),
        shape_cache: crate::surfaces::UiShapeCache::new(),
        pixel_size: setup.pixel_size,
        total_output_pixels,
        paused: false,
        vblank_pending: false,
        vblank_queued_at: None,
        needs_redraw: true,
        frame_start: None,
        rendered_frames: 0,
        max_frames: config.max_frames,
    }));

    let loop_handle = event_loop.handle();
    let _xwayland_started = crate::xwayland::spawn_xwayland_source(&display.handle(), &loop_handle);

    loop_handle
        .insert_source(listening_socket, |client_stream, _metadata, app_state| {
            info!("Accepted new Wayland client connection (DRM/KMS)");
            app_state.insert_client_stream(client_stream);
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let rt_input = Rc::clone(&runtime);
    loop_handle
        .insert_source(libinput_backend, move |event, _metadata, app_state| {
            app_state.process_input_event(event);
            if let Some(vt) = app_state.pending_vt_switch.take() {
                info!(vt, "Switching virtual terminal via libseat");
                if let Err(err) = session.change_vt(vt) {
                    warn!(vt, error = ?err, "libseat change_vt failed");
                }
                return;
            }
            let mut rt = rt_input.borrow_mut();
            rt.needs_redraw = true;
            if !rt.vblank_pending && !rt.paused {
                rt.render_and_queue(app_state);
            }
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let rt_session = Rc::clone(&runtime);
    loop_handle
        .insert_source(session_notifier, move |event, _metadata, app_state| {
            let mut rt = rt_session.borrow_mut();
            match event {
                SessionEvent::PauseSession => {
                    info!("VT switched away; pausing DRM device and libinput");
                    rt.paused = true;
                    rt.vblank_pending = false;
                    rt.vblank_queued_at = None;
                    rt.frame_start = None;
                    app_state.active_window_drag = None;
                    app_state.active_window_resize = None;
                    rt.libinput.suspend();
                    let _ = rt.libinput.dispatch();
                    rt.drm_device.pause();
                }
                SessionEvent::ActivateSession => {
                    info!("VT activated; resuming DRM device, compositor, and libinput");
                    if let Err(err) = rt.drm_device.activate(true) {
                        warn!(error = ?err, "DrmDevice::activate(true) failed; retrying soft activate");
                        if let Err(err2) = rt.drm_device.activate(false) {
                            warn!(error = ?err2, "Failed to re-activate DRM device");
                        }
                    }
                    if rt.libinput.resume().is_err() {
                        warn!("Failed to resume libinput context");
                    } else {
                        let _ = rt.libinput.dispatch();
                    }
                    if let Err(err) = rt.drm_compositor.clear() {
                        warn!(error = ?err, "DrmCompositor::clear on VT activate failed");
                    }
                    if let Err(err) = rt.drm_compositor.reset_state() {
                        warn!(error = ?err, "DrmCompositor::reset_state on VT activate failed");
                    }
                    rt.drm_compositor.reset_buffers();
                    rt.drm_compositor.reset_buffer_ages();
                    rt.paused = false;
                    rt.vblank_pending = false;
                    rt.vblank_queued_at = None;
                    rt.needs_redraw = true;
                    rt.render_and_queue(app_state);
                }
            }
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let rt_drm = Rc::clone(&runtime);
    loop_handle
        .insert_source(
            drm_notifier,
            move |event, _metadata, app_state| match event {
                DrmEvent::VBlank(_) => rt_drm.borrow_mut().handle_vblank(app_state),
                DrmEvent::Error(err) => warn!(error = ?err, "DRM device event error"),
            },
        )
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let frame_interval = Duration::from_micros(mode_refresh_interval_us(setup.vrefresh));
    let rt_timer = Rc::clone(&runtime);
    loop_handle
        .insert_source(
            Timer::from_duration(frame_interval),
            move |_deadline, _metadata, app_state| {
                if !app_state.running {
                    return TimeoutAction::Drop;
                }
                rt_timer.borrow_mut().render_and_queue(app_state);
                TimeoutAction::ToDuration(frame_interval)
            },
        )
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let ipc_socket_path = default_ipc_socket_path(&socket_name);
    let mut ipc_server = IpcServer::bind(ipc_socket_path.clone()).ok();

    info!(
        connector = %setup.connector_name,
        width = setup.pixel_size.w,
        height = setup.pixel_size.h,
        refresh_hz = setup.vrefresh,
        seat = %seat_name,
        wayland_display = %socket_name,
        ipc_socket = %ipc_socket_path.display(),
        "RustyOS Native DRM/KMS compositor active"
    );

    let autostart = state.user_config.autostart.clone();
    for cmd in autostart.iter().chain(config.spawn_clients.iter()) {
        crate::winit_backend::spawn_test_client(cmd, &socket_name, &ipc_socket_path);
    }

    runtime.borrow_mut().render_and_queue(&mut state);

    event_loop
        .run(Some(Duration::from_millis(4)), &mut state, |app_state| {
            crate::winit_backend::dispatch_and_refresh_clients(
                &mut display,
                ipc_server.as_mut(),
                app_state,
            );
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let timing_summary = crate::winit_backend::finalize_frame_timing(
        &state,
        BackendMode::NativeDrmKms,
        setup.pixel_size,
        config.metrics_out.as_deref(),
    )?;

    Ok(SessionStartOutcome::Completed(Box::new(BootstrapReport {
        version: env!("CARGO_PKG_VERSION"),
        backend: BackendMode::NativeDrmKms,
        socket_name: Some(socket_name),
        frame_timing: Some(timing_summary),
        drm_report: None,
    })))
}

fn find_primary_connector_setup(drm_device: &DrmDevice) -> Result<PrimaryConnectorSetup, String> {
    let resources = drm_device
        .resource_handles()
        .map_err(|err| format!("DRM resource_handles error: {err}"))?;

    for &conn_handle in resources.connectors() {
        let Ok(info) = drm_device.get_connector(conn_handle, false) else {
            continue;
        };
        if info.state() != connector::State::Connected {
            continue;
        }
        let Some(mode) = select_preferred_drm_mode(info.modes()) else {
            continue;
        };

        let candidate_crtc = info
            .current_encoder()
            .into_iter()
            .chain(info.encoders().iter().copied())
            .find_map(|enc_handle| {
                let enc = drm_device.get_encoder(enc_handle).ok()?;
                enc.crtc().or_else(|| {
                    resources
                        .filter_crtcs(enc.possible_crtcs())
                        .into_iter()
                        .next()
                })
            });

        if let Some(crtc) = candidate_crtc {
            let (w, h) = mode.size();
            let vrefresh = mode.vrefresh().max(1);
            let connector_name = format!("{:?}-{}", info.interface(), info.interface_id());
            return Ok(PrimaryConnectorSetup {
                connector: conn_handle,
                crtc,
                mode,
                connector_name,
                pixel_size: Size::from((i32::from(w), i32::from(h))),
                vrefresh,
            });
        }
    }

    Err("no connected DRM connector with a compatible CRTC and mode".to_owned())
}

fn probe_primary_drm_device(kms_master_status: &str) -> Result<DrmHardwareReport, CompositorError> {
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
        cursor_size: (cursor_w, cursor_h),
        crtc_count,
        plane_count,
        connectors,
        dmabuf_render_format_count,
        dmabuf_texture_format_count,
        gbm_egl_dmabuf_test_passed,
        kms_master_status: kms_master_status.to_owned(),
    })
}

fn discover_primary_drm_path() -> Result<PathBuf, CompositorError> {
    if let Ok(udev) = UdevBackend::new("seat0") {
        for (_, path) in udev.device_list() {
            if DrmNode::from_path(path).is_ok_and(|n| n.ty() == NodeType::Primary) {
                return Ok(path.to_path_buf());
            }
        }
    }

    for candidate in ["/dev/dri/card0", "/dev/dri/card1", "/dev/dri/card2"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    Err(CompositorError::BackendInit(
        "no /dev/dri/card* device found".to_owned(),
    ))
}

fn open_drm_device_fd(path: &Path) -> Result<DrmDeviceFd, CompositorError> {
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
fn verify_gbm_egl_dmabuf_pipeline(
    drm_path: &Path,
) -> Result<(usize, usize, bool), CompositorError> {
    let fd = open_drm_device_fd(drm_path)?;
    let gbm_device = GbmDevice::new(fd)
        .map_err(|err| CompositorError::BackendInit(format!("GbmDevice::new failed: {err}")))?;
    let mut allocator = DmabufAllocator(GbmAllocator::new(
        gbm_device.clone(),
        GbmBufferFlags::RENDERING,
    ));

    let (egl_display, mut renderer) = init_gbm_egl_renderer(&gbm_device)?;

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

/// Isolated wrapper around Smithay's `unsafe` `EGLDisplay::new` and `GlesRenderer::new` constructors.
#[allow(unsafe_code)]
fn init_gbm_egl_renderer(
    gbm_device: &GbmDevice<DrmDeviceFd>,
) -> Result<(EGLDisplay, GlesRenderer), CompositorError> {
    // SAFETY: `GbmDevice<DrmDeviceFd>` is a valid, open GBM device backed by a ref-counted
    // `DrmDeviceFd` that outlives the `EGLDisplay` creation or is retained internally by Smithay.
    let egl_display = unsafe {
        if let Ok(mut devices) = EGLDevice::enumerate()
            && let Some(egl_dev) = devices.find(|d| d.drm_device_path().is_ok())
            && let Ok(display) = EGLDisplay::new(egl_dev)
        {
            display
        } else {
            EGLDisplay::new(gbm_device.clone()).map_err(|err| {
                CompositorError::BackendInit(format!("EGLDisplay::new failed: {err}"))
            })?
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

fn log_hardware_report(report: &DrmHardwareReport) {
    info!(
        primary_node = %report.primary_node,
        render_node = ?report.render_node,
        atomic = report.atomic_modesetting,
        universal_planes = report.universal_planes,
        monotonic_ts = report.monotonic_timestamps,
        cursor_size = ?report.cursor_size,
        crtcs = report.crtc_count,
        planes = report.plane_count,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_report_serializes_valid_json() {
        let sample = DrmHardwareReport {
            primary_node: "/dev/dri/card1".to_owned(),
            render_node: Some("/dev/dri/renderD128".to_owned()),
            atomic_modesetting: true,
            universal_planes: true,
            monotonic_timestamps: true,
            async_page_flip: true,
            cursor_size: (256, 256),
            crtc_count: 4,
            plane_count: 31,
            connectors: vec![ConnectorReport {
                name: "eDP-1".to_owned(),
                status: "Connected".to_owned(),
                preferred_mode: Some((1920, 1080, 60)),
                mode_count: 3,
            }],
            dmabuf_render_format_count: 42,
            dmabuf_texture_format_count: 68,
            gbm_egl_dmabuf_test_passed: true,
            kms_master_status: "probe-only".to_owned(),
        };

        let json = sample.to_json();
        assert!(json.contains("\"primary_node\": \"/dev/dri/card1\""));
        assert!(json.contains("\"atomic_modesetting\": true"));
        assert!(json.contains("\"preferred_mode\": \"1920x1080@60Hz\""));
        assert!(json.contains("\"gbm_egl_dmabuf_test_passed\": true"));
    }

    #[test]
    fn mode_refresh_interval_covers_high_refresh_panels() {
        assert_eq!(mode_refresh_interval_us(60), 16_666);
        assert_eq!(mode_refresh_interval_us(120), 8_333);
        assert_eq!(mode_refresh_interval_us(144), 6_944);
        assert_eq!(mode_refresh_interval_us(240), 4_166);
        assert_eq!(mode_refresh_interval_us(0), 1_000_000);
        assert!(select_preferred_drm_mode(&[]).is_none());
    }
}
