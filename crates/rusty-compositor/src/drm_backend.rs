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
use std::collections::{HashMap, HashSet};
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
            Allocator, Format, Fourcc, Modifier,
            dmabuf::{Dmabuf, DmabufAllocator},
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, DrmSurface, NodeType, VrrSupport,
            compositor::{DrmCompositor, FrameFlags, PrimaryPlaneElement},
            exporter::gbm::GbmFramebufferExporter,
        },
        egl::{EGLContext, EGLDevice, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            Bind, Color32F, Frame, ImportDma, Renderer, gles::GlesRenderer, sync::SyncPoint,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent},
    },
    desktop::utils::{OutputPresentationFeedback, send_frames_surface_tree},
    output::{Mode as OutputMode, Scale},
    reexports::{
        calloop::{
            EventLoop,
            timer::{TimeoutAction, Timer},
        },
        drm::{
            ClientCapability, Device as BasicDevice, DriverCapability,
            control::{
                Device as ControlDevice, Mode as ControlMode, ModeTypeFlags, PlaneType, connector,
                crtc,
            },
        },
        input::Libinput,
        rustix::fs::OFlags,
        wayland_protocols::wp::{
            linux_dmabuf::zv1::server::zwp_linux_dmabuf_feedback_v1,
            presentation_time::server::wp_presentation_feedback,
        },
        wayland_server::Display,
    },
    utils::{DeviceFd, Logical, Physical, Point, Rectangle, Size, Transform},
    wayland::{
        dmabuf::DmabufFeedbackBuilder, drm_syncobj::supports_syncobj_eventfd,
        presentation::Refresh, socket::ListeningSocketSource,
    },
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
    /// Whether DRM timeline syncobj eventfd signaling is supported (`wp_linux_drm_syncobj_v1`, `RUSTYOS-046`).
    pub syncobj_eventfd: bool,
    /// Recommended hardware cursor dimensions `(width, height)`.
    pub cursor_size: (u64, u64),
    /// Number of CRTC scanout engines on the GPU.
    pub crtc_count: usize,
    /// Total number of DRM planes exposed by the device.
    pub plane_count: usize,
    /// Number of DRM `PlaneType::Cursor` hardware planes discovered under universal planes (`RUSTYOS-046`).
    pub cursor_plane_count: usize,
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
        let _ = writeln!(out, "  \"syncobj_eventfd\": {},", self.syncobj_eventfd);
        let _ = writeln!(
            out,
            "  \"cursor_size\": [{}, {}],",
            self.cursor_size.0, self.cursor_size.1
        );
        let _ = writeln!(out, "  \"crtc_count\": {},", self.crtc_count);
        let _ = writeln!(out, "  \"plane_count\": {},", self.plane_count);
        let _ = writeln!(
            out,
            "  \"cursor_plane_count\": {},",
            self.cursor_plane_count
        );
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
pub struct PrimaryConnectorSetup {
    pub connector: connector::Handle,
    pub crtc: crtc::Handle,
    pub mode: ControlMode,
    pub connector_name: String,
    pub pixel_size: Size<i32, Physical>,
    pub vrefresh: u32,
}

enum SessionStartOutcome {
    Completed(Box<BootstrapReport>),
    DeferToProbe(String),
}

#[allow(clippy::struct_excessive_bools)]
struct DrmOutputState {
    output_id: rusty_wm::OutputId,
    connector: connector::Handle,
    connector_name: String,
    drm_compositor: RustyDrmCompositor,
    pixel_size: Size<i32, Physical>,
    total_output_pixels: u64,
    vrefresh: u32,
    vrr_support: VrrSupport,
    vrr_enabled: bool,
    vblank_pending: bool,
    vblank_queued_at: Option<Instant>,
    needs_redraw: bool,
    last_frame_direct_scanout: bool,
    cursor_plane_count: usize,
    hw_cursor_enabled: bool,
    last_frame_hw_cursor: bool,
    frame_start: Option<Instant>,
    next_frame_at: Instant,
    pending_presentation_feedback: Option<OutputPresentationFeedback>,
    vblank_seq: u64,
}

struct DrmRuntime {
    drm_device: DrmDevice,
    primary_node: DrmNode,
    gbm_device: GbmDevice<DrmDeviceFd>,
    libinput: Libinput,
    renderer: GlesRenderer,
    render_formats: Vec<Format>,
    cursor_size: Size<u32, smithay::utils::Buffer>,
    hw_cursor_disabled: bool,
    text_cache: crate::text::TextCache,
    shape_cache: crate::surfaces::UiShapeCache,
    portal_capture_cache: crate::screenshot::OffscreenCaptureBuffer,
    outputs: HashMap<crtc::Handle, DrmOutputState>,
    output_order: Vec<crtc::Handle>,
    paused: bool,
    pending_hotplug_rescan: bool,
    rendered_frames: u64,
    max_frames: Option<u64>,
}

/// Evaluates whether runtime VRR should be active for `output_id` given `vrr_mode` (`RUSTYOS-042`).
#[must_use]
pub(crate) fn want_vrr_for_output(
    vrr_mode: crate::config::VrrMode,
    state: &RustyCompositorState,
    output_id: rusty_wm::OutputId,
) -> bool {
    match vrr_mode {
        crate::config::VrrMode::Off => false,
        crate::config::VrrMode::Fullscreen => state
            .active_unoccluded_fullscreen_window_on(output_id)
            .is_some(),
        crate::config::VrrMode::Always => true,
    }
}

/// Computes `wp_presentation_feedback::Kind` flags for DRM `VBlank` completion (`RUSTYOS-044`).
#[must_use]
pub(crate) fn compute_drm_presentation_flags(
    direct_scanout: bool,
) -> wp_presentation_feedback::Kind {
    let mut flags = wp_presentation_feedback::Kind::Vsync | wp_presentation_feedback::Kind::HwClock;
    if direct_scanout {
        flags |= wp_presentation_feedback::Kind::ZeroCopy;
    }
    flags
}

/// Computes `wp_presentation` [`Refresh`] metadata given runtime `VRR` state and panel refresh rate (`RUSTYOS-044`).
#[must_use]
pub(crate) fn compute_presentation_refresh(vrr_enabled: bool, vrefresh: u32) -> Refresh {
    let interval = Duration::from_micros(mode_refresh_interval_us(vrefresh));
    if vrr_enabled {
        Refresh::variable(interval)
    } else {
        Refresh::fixed(interval)
    }
}

struct DrmOutputContext<'a> {
    primary_node: DrmNode,
    gbm_device: &'a GbmDevice<DrmDeviceFd>,
    render_formats: &'a [Format],
    cursor_size: Size<u32, smithay::utils::Buffer>,
    hw_cursor_disabled: bool,
}

fn build_drm_output_state(
    ctx: &DrmOutputContext<'_>,
    setup: &PrimaryConnectorSetup,
    smithay_output: &smithay::output::Output,
    output_id: rusty_wm::OutputId,
    drm_surface: DrmSurface,
) -> Result<DrmOutputState, CompositorError> {
    let cursor_plane_count = drm_surface.planes().cursor.len();
    let hw_cursor_enabled = cursor_plane_count > 0 && !ctx.hw_cursor_disabled;
    let gbm_allocator = GbmAllocator::new(
        ctx.gbm_device.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let gbm_exporter = GbmFramebufferExporter::new(ctx.gbm_device.clone(), Some(ctx.primary_node));
    let drm_compositor = DrmCompositor::new(
        smithay_output,
        drm_surface,
        None,
        gbm_allocator,
        gbm_exporter,
        [Fourcc::Argb8888, Fourcc::Xrgb8888],
        ctx.render_formats.iter().copied(),
        ctx.cursor_size,
        None,
    )
    .map_err(|err| CompositorError::BackendInit(format!("DrmCompositor::new failed: {err:?}")))?;

    let vrr_support = drm_compositor
        .vrr_supported(setup.connector)
        .unwrap_or(VrrSupport::NotSupported);
    let total_output_pixels = u64::try_from(setup.pixel_size.w.max(0)).unwrap_or(0)
        * u64::try_from(setup.pixel_size.h.max(0)).unwrap_or(0);
    info!(
        connector = %setup.connector_name,
        output_id = output_id.0,
        width = setup.pixel_size.w,
        height = setup.pixel_size.h,
        refresh_hz = setup.vrefresh,
        vrr_support = ?vrr_support,
        cursor_planes = cursor_plane_count,
        hw_cursor = hw_cursor_enabled,
        "Initialized KMS DrmCompositor output"
    );

    Ok(DrmOutputState {
        output_id,
        connector: setup.connector,
        connector_name: setup.connector_name.clone(),
        drm_compositor,
        pixel_size: setup.pixel_size,
        total_output_pixels,
        vrefresh: setup.vrefresh,
        vrr_support,
        vrr_enabled: false,
        vblank_pending: false,
        vblank_queued_at: None,
        needs_redraw: true,
        last_frame_direct_scanout: false,
        cursor_plane_count,
        hw_cursor_enabled,
        last_frame_hw_cursor: false,
        frame_start: None,
        next_frame_at: Instant::now(),
        pending_presentation_feedback: None,
        vblank_seq: 0,
    })
}

impl DrmOutputState {
    fn reset_if_vblank_timed_out(&mut self) -> bool {
        if !self.vblank_pending {
            return true;
        }
        let timed_out = self
            .vblank_queued_at
            .is_some_and(|queued| queued.elapsed() >= Duration::from_millis(500));
        if timed_out {
            warn!(
                connector = %self.connector_name,
                "DRM VBlank watchdog (>500ms); resetting output compositor pipeline"
            );
            self.vblank_pending = false;
            self.vblank_queued_at = None;
            if let Some(mut stale) = self.pending_presentation_feedback.take() {
                stale.discarded();
            }
            let _ = self.drm_compositor.clear();
            let _ = self.drm_compositor.reset_state();
            self.drm_compositor.reset_buffers();
            self.drm_compositor.reset_buffer_ages();
            true
        } else {
            self.needs_redraw = true;
            false
        }
    }

    fn apply_runtime_vrr(&mut self, want_vrr: bool) {
        if self.vrr_support != VrrSupport::Supported || self.vrr_enabled == want_vrr {
            return;
        }
        match self.drm_compositor.use_vrr(want_vrr) {
            Ok(()) => {
                info!(
                    connector = %self.connector_name,
                    vrr = want_vrr,
                    "Updated runtime VRR state"
                );
                self.vrr_enabled = want_vrr;
            }
            Err(err) => {
                warn!(
                    connector = %self.connector_name,
                    error = ?err,
                    "Failed to toggle runtime VRR; falling back to fixed refresh"
                );
                self.vrr_support = VrrSupport::NotSupported;
                self.vrr_enabled = false;
            }
        }
    }

    fn submit_rendered_frame(
        &mut self,
        renderer: &mut GlesRenderer,
        render_list: &[crate::shell_render::CompositorRenderElement],
        state: &mut RustyCompositorState,
        target_output: &smithay::output::Output,
        render_start: Instant,
    ) {
        match self.drm_compositor.render_frame(
            renderer,
            render_list,
            crate::frame_composer::BACKGROUND_COLOR,
            FrameFlags::DEFAULT,
        ) {
            Ok(result) => {
                if result.is_empty {
                    self.needs_redraw = false;
                    self.last_frame_direct_scanout = false;
                    self.last_frame_hw_cursor = false;
                    if state.ext_session_locked {
                        state.confirm_pending_session_lock();
                    }
                    return;
                }
                if result.needs_sync()
                    && let PrimaryPlaneElement::Swapchain(ref element) = result.primary_element
                {
                    let _ = element.sync.wait();
                }
                let direct_scanout =
                    matches!(result.primary_element, PrimaryPlaneElement::Element(_));
                let hw_cursor = result.cursor_element.is_some();
                let render_states = result.states;
                if let Err(err) = self.drm_compositor.queue_frame(()) {
                    warn!(
                        connector = %self.connector_name,
                        error = ?err,
                        "Failed to queue DRM/KMS page flip"
                    );
                    self.needs_redraw = true;
                    self.last_frame_direct_scanout = false;
                    self.last_frame_hw_cursor = false;
                } else {
                    self.vblank_pending = true;
                    self.vblank_queued_at = Some(Instant::now());
                    self.needs_redraw = false;
                    self.frame_start = Some(render_start);
                    self.last_frame_direct_scanout = direct_scanout;
                    self.last_frame_hw_cursor = hw_cursor;
                    if let Some(mut stale) = self.pending_presentation_feedback.take() {
                        stale.discarded();
                    }
                    self.pending_presentation_feedback =
                        Some(state.take_presentation_feedback(target_output, &render_states));
                }
            }
            Err(err) => {
                warn!(
                    connector = %self.connector_name,
                    error = ?err,
                    "DRM/KMS render_frame error"
                );
                self.needs_redraw = true;
                self.last_frame_direct_scanout = false;
                self.last_frame_hw_cursor = false;
            }
        }
    }
}

/// Computes output IDs whose cursor overlay region overlaps `prev_ptr` or `next_ptr` (`RUSTYOS-046`).
#[must_use]
pub(crate) fn output_ids_affected_by_pointer_motion(
    output_map: &crate::outputs::OutputMap,
    prev_ptr: Point<f64, Logical>,
    next_ptr: Point<f64, Logical>,
) -> Vec<rusty_wm::OutputId> {
    let mut ids = Vec::new();
    for entry in output_map.entries() {
        let geo = entry.logical_geometry();
        if crate::frame_composer::pointer_local_if_on_output(prev_ptr, geo).is_some()
            || crate::frame_composer::pointer_local_if_on_output(next_ptr, geo).is_some()
        {
            ids.push(entry.id);
        }
    }
    ids
}

impl DrmRuntime {
    fn fastest_refresh_interval(&self) -> Duration {
        let min_us = self
            .outputs
            .values()
            .map(|o| mode_refresh_interval_us(o.vrefresh))
            .min()
            .unwrap_or(16_667);
        Duration::from_micros(min_us)
    }

    fn mark_all_dirty(&mut self) {
        for out in self.outputs.values_mut() {
            out.needs_redraw = true;
        }
    }

    fn process_portal(&mut self, state: &mut RustyCompositorState) {
        if !state.running || self.paused || state.is_session_locked() {
            return;
        }
        crate::frame_composer::process_portal_captures_and_streams(
            &mut self.renderer,
            &mut self.text_cache,
            &mut self.shape_cache,
            &mut self.portal_capture_cache,
            state,
        );
    }

    fn mark_outputs_dirty(&mut self, output_ids: &[rusty_wm::OutputId]) {
        if output_ids.is_empty() {
            self.mark_all_dirty();
            return;
        }
        let mut marked_any = false;
        for out in self.outputs.values_mut() {
            if output_ids.contains(&out.output_id) {
                out.needs_redraw = true;
                marked_any = true;
            }
        }
        if !marked_any {
            self.mark_all_dirty();
        }
    }

    fn render_ready_outputs(&mut self, state: &mut RustyCompositorState, force_timer: bool) {
        if !state.running || self.paused {
            return;
        }
        let now = Instant::now();
        let crtcs = self.output_order.clone();
        for crtc in crtcs {
            let should_draw = self
                .outputs
                .get(&crtc)
                .is_some_and(|out| out.needs_redraw || (force_timer && now >= out.next_frame_at));
            if should_draw {
                self.render_and_queue_output(crtc, state);
            }
        }
    }

    fn capture_primary_screenshot(
        &mut self,
        state: &mut RustyCompositorState,
        output_id: rusty_wm::OutputId,
        pixel_size: smithay::utils::Size<i32, smithay::utils::Physical>,
    ) {
        let snap_list = crate::frame_composer::build_frame_render_list_for_output(
            &mut self.renderer,
            &mut self.text_cache,
            &mut self.shape_cache,
            state,
            output_id,
            pixel_size,
            false,
        );
        crate::frame_composer::process_pending_screenshot(
            &mut self.renderer,
            &snap_list,
            state,
            pixel_size,
        );
    }

    fn render_and_queue_output(&mut self, crtc: crtc::Handle, state: &mut RustyCompositorState) {
        if !state.running || self.paused {
            return;
        }
        let want_vrr;
        let output_id;
        let pixel_size;
        let interval;
        let hw_cursor_allowed;
        {
            let Some(out) = self.outputs.get_mut(&crtc) else {
                return;
            };
            if !out.reset_if_vblank_timed_out() {
                return;
            }
            output_id = out.output_id;
            pixel_size = out.pixel_size;
            interval = Duration::from_micros(mode_refresh_interval_us(out.vrefresh));
            hw_cursor_allowed = out.hw_cursor_enabled && !self.hw_cursor_disabled;
            want_vrr = want_vrr_for_output(state.user_config.vrr_mode, state, output_id);
            out.apply_runtime_vrr(want_vrr);
        }

        let render_start = Instant::now();
        state.tick_animations();
        state.expire_notifications();

        let is_primary = state.output_map.primary().id == output_id;
        if is_primary && state.pending_screenshot.is_some() {
            self.capture_primary_screenshot(state, output_id, pixel_size);
        }

        let cursor_mode = if hw_cursor_allowed {
            crate::frame_composer::CursorRenderMode::HardwarePlane {
                max_size: self.cursor_size,
            }
        } else {
            crate::frame_composer::CursorRenderMode::SoftwareComposited
        };
        let render_list = crate::frame_composer::build_frame_render_list_for_output_with_cursor(
            &mut self.renderer,
            &mut self.text_cache,
            &mut self.shape_cache,
            state,
            output_id,
            pixel_size,
            cursor_mode,
        );

        let target_output = state
            .output_map
            .get(output_id)
            .map_or_else(|| state.output.clone(), |entry| entry.output.clone());

        let Some(out) = self.outputs.get_mut(&crtc) else {
            return;
        };
        out.next_frame_at = Instant::now() + interval;
        out.submit_rendered_frame(
            &mut self.renderer,
            &render_list,
            state,
            &target_output,
            render_start,
        );
    }

    fn handle_vblank(&mut self, crtc: crtc::Handle, state: &mut RustyCompositorState) {
        let Some(out) = self.outputs.get_mut(&crtc) else {
            return;
        };
        if let Err(err) = out.drm_compositor.frame_submitted() {
            warn!(
                connector = %out.connector_name,
                error = ?err,
                "Failed to mark DRM/KMS frame as submitted"
            );
        }
        out.vblank_pending = false;
        out.vblank_queued_at = None;

        let present_done = Instant::now();
        let frame_start = out.frame_start.take().unwrap_or(present_done);
        state.frame_timing.record_frame(
            frame_start,
            present_done,
            1,
            out.total_output_pixels,
            out.total_output_pixels,
        );
        state
            .frame_timing
            .record_direct_scanout(out.last_frame_direct_scanout);
        state
            .frame_timing
            .record_hw_cursor(out.last_frame_hw_cursor);

        let output_id = out.output_id;
        let should_requeue = out.needs_redraw;
        out.vblank_seq = out.vblank_seq.saturating_add(1);
        let vblank_seq = out.vblank_seq;
        let refresh = compute_presentation_refresh(out.vrr_enabled, out.vrefresh);
        let flags = compute_drm_presentation_flags(out.last_frame_direct_scanout);
        if let Some(mut feedback) = out.pending_presentation_feedback.take() {
            feedback.presented(state.clock.now(), refresh, vblank_seq, flags);
        }
        let target_output = state
            .output_map
            .get(output_id)
            .map_or_else(|| state.output.clone(), |entry| entry.output.clone());

        let elapsed = state.elapsed();
        if state.ext_session_locked {
            state.confirm_pending_session_lock();
            if let Some(lock_surf) = state.ext_lock_surface_for_output(&target_output) {
                send_frames_surface_tree(
                    lock_surf.wl_surface(),
                    &target_output,
                    elapsed,
                    Some(Duration::ZERO),
                    |_, _| Some(target_output.clone()),
                );
            }
        } else {
            for window in state.space.elements() {
                window.send_frame(&target_output, elapsed, Some(Duration::ZERO), |_, _| {
                    Some(target_output.clone())
                });
            }
            state.send_ime_popup_frames(&target_output, elapsed);
        }
        state.space.refresh();
        state.popups.cleanup();
        state.cleanup_ime_popups();
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

        if should_requeue && !self.paused && state.running {
            self.render_and_queue_output(crtc, state);
        }
    }

    fn attach_secondary_connector(
        &mut self,
        setup: &PrimaryConnectorSetup,
        state: &mut RustyCompositorState,
    ) -> bool {
        if self
            .outputs
            .values()
            .any(|o| o.connector == setup.connector)
            || self.outputs.contains_key(&setup.crtc)
        {
            return false;
        }
        let drm_surface =
            match self
                .drm_device
                .create_surface(setup.crtc, setup.mode, &[setup.connector])
            {
                Ok(surf) => surf,
                Err(err) => {
                    warn!(
                        connector = %setup.connector_name,
                        error = ?err,
                        "Failed to create DrmSurface for secondary display"
                    );
                    return false;
                }
            };
        let next_loc = state.output_map.next_adjacent_location();
        let refresh_milli_hz = i32::try_from(setup.vrefresh.saturating_mul(1000)).unwrap_or(60_000);
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
        let Some(smithay_output) = state.output_map.get(new_id).map(|e| e.output.clone()) else {
            state.remove_output(new_id);
            return false;
        };
        let ctx = DrmOutputContext {
            primary_node: self.primary_node,
            gbm_device: &self.gbm_device,
            render_formats: &self.render_formats,
            cursor_size: self.cursor_size,
            hw_cursor_disabled: self.hw_cursor_disabled,
        };
        match build_drm_output_state(&ctx, setup, &smithay_output, new_id, drm_surface) {
            Ok(out_state) => {
                self.output_order.push(setup.crtc);
                self.outputs.insert(setup.crtc, out_state);
                true
            }
            Err(err) => {
                warn!(
                    connector = %setup.connector_name,
                    error = %err,
                    "Failed to initialize secondary DrmOutputState; rolling back"
                );
                state.remove_output(new_id);
                false
            }
        }
    }

    fn sync_hotplug_connectors(&mut self, state: &mut RustyCompositorState) {
        if self.paused {
            self.pending_hotplug_rescan = true;
            return;
        }
        self.pending_hotplug_rescan = false;

        let connected = find_all_connected_connectors_with_prefs(
            &self.drm_device,
            state.user_config.prefer_max_refresh,
            Some(&self.outputs),
        );

        let current_crtcs = self.output_order.clone();
        for crtc in current_crtcs {
            if self.outputs.len() <= 1 {
                break;
            }
            let still_connected = self.outputs.get(&crtc).is_some_and(|out| {
                connected
                    .iter()
                    .any(|c| c.connector == out.connector && c.connector_name == out.connector_name)
            });
            if !still_connected && let Some(removed) = self.outputs.remove(&crtc) {
                self.output_order.retain(|c| *c != crtc);
                state.remove_output(removed.output_id);
                info!(
                    connector = %removed.connector_name,
                    output_id = removed.output_id.0,
                    "Hotplug: Removed disconnected DRM output"
                );
            }
        }

        for setup in &connected {
            let crtc = setup.crtc;
            if self.attach_secondary_connector(setup, state) {
                self.render_and_queue_output(crtc, state);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModeDescriptor {
    size: (u16, u16),
    vrefresh: u32,
    preferred: bool,
    clock: u32,
}

fn select_best_mode_index(modes: &[ModeDescriptor], prefer_max_refresh: bool) -> Option<usize> {
    let base_idx = modes
        .iter()
        .position(|m| m.preferred)
        .or_else(|| (!modes.is_empty()).then_some(0))?;
    if !prefer_max_refresh {
        return Some(base_idx);
    }

    let target_size = modes[base_idx].size;
    modes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.size == target_size)
        .max_by_key(|(_, m)| (m.vrefresh, m.preferred, m.clock))
        .map(|(idx, _)| idx)
        .or(Some(base_idx))
}

/// Selects the preferred DRM display mode (falling back to the first reported mode).
#[must_use]
pub fn select_preferred_drm_mode(modes: &[ControlMode]) -> Option<ControlMode> {
    select_best_drm_mode(modes, false)
}

/// Selects the optimal DRM mode for the panel's preferred resolution, optionally prioritizing the
/// highest supported vertical refresh rate (`60/120/144/240 Hz`) (`RUSTYOS-042`).
#[must_use]
pub fn select_best_drm_mode(
    modes: &[ControlMode],
    prefer_max_refresh: bool,
) -> Option<ControlMode> {
    let descriptors: Vec<ModeDescriptor> = modes
        .iter()
        .map(|m| ModeDescriptor {
            size: m.size(),
            vrefresh: m.vrefresh(),
            preferred: m.mode_type().contains(ModeTypeFlags::PREFERRED),
            clock: m.clock(),
        })
        .collect();
    select_best_mode_index(&descriptors, prefer_max_refresh).map(|idx| modes[idx])
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

    let user_cfg = crate::config::UserConfig::load_default();
    let setups =
        find_all_connected_connectors_with_prefs(&drm_device, user_cfg.prefer_max_refresh, None);
    let Some(setup) = setups.first().cloned() else {
        return Ok(SessionStartOutcome::DeferToProbe(
            "no connected DRM connector with a compatible CRTC and mode".to_owned(),
        ));
    };

    let primary_surface =
        match drm_device.create_surface(setup.crtc, setup.mode, &[setup.connector]) {
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
        event_loop.handle(),
        setup.pixel_size,
    )?;
    state.config_path = crate::config::default_user_config_path();

    let refresh_milli_hz = i32::try_from(setup.vrefresh.saturating_mul(1000)).unwrap_or(60_000);
    let primary_scale = state.user_config.scale_for_output(&setup.connector_name);
    state.rotation_override = config.rotation_override;
    let primary_rotation = state
        .user_config
        .rotation_for_output(&setup.connector_name, config.rotation_override);
    state
        .output_map
        .primary_mut()
        .name
        .clone_from(&setup.connector_name);
    state.update_output_geometry(
        PRIMARY_OUTPUT_ID,
        setup.pixel_size,
        primary_scale,
        refresh_milli_hz,
    );
    state.set_output_rotation(PRIMARY_OUTPUT_ID, primary_rotation, false);
    let logical_size = state.output_map.primary().logical_geometry().size;
    state.pointer_location = Point::<f64, Logical>::from((
        f64::from(logical_size.w) / 2.0,
        f64::from(logical_size.h) / 2.0,
    ));

    state
        .animations
        .set_reduced_motion(state.user_config.reduced_motion || config.reduced_motion);
    state.start_background_services();

    let client_dmabuf_formats = renderer.dmabuf_formats();
    let scanout_formats: Vec<_> = primary_surface
        .plane_info()
        .formats
        .iter()
        .filter(|fmt| client_dmabuf_formats.contains(fmt))
        .copied()
        .collect();
    let main_device = primary_node
        .node_with_type(NodeType::Render)
        .and_then(Result::ok)
        .unwrap_or(primary_node)
        .dev_id();
    let feedback_builder = DmabufFeedbackBuilder::new(main_device, client_dmabuf_formats.clone());
    let feedback_builder = if scanout_formats.is_empty() {
        feedback_builder
    } else {
        feedback_builder.add_preference_tranche(
            primary_node.dev_id(),
            Some(zwp_linux_dmabuf_feedback_v1::TrancheFlags::Scanout),
            scanout_formats,
        )
    };
    match feedback_builder.build() {
        Ok(feedback) => state.init_dmabuf_feedback(&feedback),
        Err(err) => {
            warn!(error = %err, "Failed to build DmabufFeedback; falling back to v3 format list");
            state.init_dmabuf_formats(client_dmabuf_formats);
        }
    }
    let explicit_sync_active = state.init_drm_syncobj(drm_device.device_fd());

    let render_formats: Vec<_> = renderer
        .egl_context()
        .dmabuf_render_formats()
        .iter()
        .copied()
        .collect();
    let cursor_size = drm_device.cursor_size();
    let hw_cursor_disabled = RustyCompositorState::hw_cursor_disabled_by_env();
    let primary_ctx = DrmOutputContext {
        primary_node,
        gbm_device: &gbm_device,
        render_formats: &render_formats,
        cursor_size,
        hw_cursor_disabled,
    };
    let primary_out_state = build_drm_output_state(
        &primary_ctx,
        &setup,
        &state.output,
        PRIMARY_OUTPUT_ID,
        primary_surface,
    )?;

    let mut outputs = HashMap::new();
    let output_order = vec![setup.crtc];
    outputs.insert(setup.crtc, primary_out_state);

    let runtime = Rc::new(RefCell::new(DrmRuntime {
        drm_device,
        primary_node,
        gbm_device,
        libinput,
        renderer,
        render_formats,
        cursor_size,
        hw_cursor_disabled,
        text_cache: crate::text::TextCache::new(),
        shape_cache: crate::surfaces::UiShapeCache::new(),
        portal_capture_cache: crate::screenshot::OffscreenCaptureBuffer::default(),
        outputs,
        output_order,
        paused: false,
        pending_hotplug_rescan: false,
        rendered_frames: 0,
        max_frames: config.max_frames,
    }));

    {
        let mut rt = runtime.borrow_mut();
        for extra in setups.iter().skip(1) {
            rt.attach_secondary_connector(extra, &mut state);
        }
    }

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
            let is_pointer_motion = matches!(
                event,
                InputEvent::PointerMotion { .. } | InputEvent::PointerMotionAbsolute { .. }
            );
            let prev_ptr = app_state.pointer_location;
            app_state.process_input_event(event);
            if let Some(vt) = app_state.pending_vt_switch.take() {
                info!(vt, "Switching virtual terminal via libseat");
                if let Err(err) = session.change_vt(vt) {
                    warn!(vt, error = ?err, "libseat change_vt failed");
                }
                return;
            }
            let mut rt = rt_input.borrow_mut();
            if is_pointer_motion
                && app_state.active_window_drag.is_none()
                && app_state.active_window_resize.is_none()
            {
                let dirty_ids = output_ids_affected_by_pointer_motion(
                    &app_state.output_map,
                    prev_ptr,
                    app_state.pointer_location,
                );
                rt.mark_outputs_dirty(&dirty_ids);
            } else {
                rt.mark_all_dirty();
            }
            rt.render_ready_outputs(app_state, false);
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let rt_udev = Rc::clone(&runtime);
    let primary_dev_id = primary_node.dev_id();
    loop_handle
        .insert_source(
            udev_backend,
            move |event, _metadata, app_state| match event {
                UdevEvent::Added { device_id, .. }
                | UdevEvent::Changed { device_id }
                | UdevEvent::Removed { device_id }
                    if device_id == primary_dev_id =>
                {
                    info!(
                        device_id,
                        "Udev DRM hotplug event received; rescanning connectors"
                    );
                    rt_udev.borrow_mut().sync_hotplug_connectors(app_state);
                }
                _ => {}
            },
        )
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let rt_session = Rc::clone(&runtime);
    loop_handle
        .insert_source(session_notifier, move |event, _metadata, app_state| {
            let mut rt = rt_session.borrow_mut();
            match event {
                SessionEvent::PauseSession => {
                    info!("VT switched away; pausing DRM device and libinput");
                    rt.paused = true;
                    for out in rt.outputs.values_mut() {
                        out.vblank_pending = false;
                        out.vblank_queued_at = None;
                        out.frame_start = None;
                    }
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
                    for out in rt.outputs.values_mut() {
                        if let Err(err) = out.drm_compositor.clear() {
                            warn!(connector = %out.connector_name, error = ?err, "DrmCompositor::clear on VT activate failed");
                        }
                        if let Err(err) = out.drm_compositor.reset_state() {
                            warn!(connector = %out.connector_name, error = ?err, "DrmCompositor::reset_state on VT activate failed");
                        }
                        out.drm_compositor.reset_buffers();
                        out.drm_compositor.reset_buffer_ages();
                        out.vblank_pending = false;
                        out.vblank_queued_at = None;
                        out.needs_redraw = true;
                    }
                    rt.paused = false;
                    if rt.pending_hotplug_rescan {
                        rt.sync_hotplug_connectors(app_state);
                    }
                    rt.render_ready_outputs(app_state, true);
                }
            }
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let rt_drm = Rc::clone(&runtime);
    loop_handle
        .insert_source(
            drm_notifier,
            move |event, _metadata, app_state| match event {
                DrmEvent::VBlank(crtc) => rt_drm.borrow_mut().handle_vblank(crtc, app_state),
                DrmEvent::Error(err) => warn!(error = ?err, "DRM device event error"),
            },
        )
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let initial_interval = runtime.borrow().fastest_refresh_interval();
    let rt_timer = Rc::clone(&runtime);
    loop_handle
        .insert_source(
            Timer::from_duration(initial_interval),
            move |_deadline, _metadata, app_state| {
                if !app_state.running {
                    return TimeoutAction::Drop;
                }
                let mut rt = rt_timer.borrow_mut();
                rt.render_ready_outputs(app_state, true);
                TimeoutAction::ToDuration(rt.fastest_refresh_interval())
            },
        )
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let ipc_socket_path = default_ipc_socket_path(&socket_name);
    let mut ipc_server = IpcServer::bind(ipc_socket_path.clone()).ok();

    let total_cursor_planes: usize = runtime
        .borrow()
        .outputs
        .values()
        .map(|o| o.cursor_plane_count)
        .sum();
    info!(
        connector = %setup.connector_name,
        width = setup.pixel_size.w,
        height = setup.pixel_size.h,
        refresh_hz = setup.vrefresh,
        active_outputs = runtime.borrow().outputs.len(),
        explicit_sync = explicit_sync_active,
        cursor_planes = total_cursor_planes,
        hw_cursor_disabled,
        cursor_size = ?cursor_size,
        seat = %seat_name,
        wayland_display = %socket_name,
        ipc_socket = %ipc_socket_path.display(),
        "RustyOS Native DRM/KMS compositor active"
    );

    let autostart = state.user_config.autostart.clone();
    for cmd in autostart.iter().chain(config.spawn_clients.iter()) {
        crate::frame_composer::spawn_test_client(cmd, &socket_name, &ipc_socket_path);
    }

    runtime.borrow_mut().render_ready_outputs(&mut state, true);

    let rt_dispatch = Rc::clone(&runtime);
    event_loop
        .run(Some(Duration::from_millis(4)), &mut state, |app_state| {
            crate::frame_composer::dispatch_and_refresh_clients(
                &mut display,
                ipc_server.as_mut(),
                app_state,
            );
            if app_state.take_syncobj_unblocked() || app_state.take_shell_redraw_needed() {
                let mut rt = rt_dispatch.borrow_mut();
                rt.mark_all_dirty();
                rt.render_ready_outputs(app_state, false);
            }
            if app_state.pending_portal_capture.is_some()
                || app_state.has_due_screencast_frame(Instant::now())
            {
                rt_dispatch.borrow_mut().process_portal(app_state);
            }
        })
        .map_err(|err| CompositorError::EventLoop(err.to_string()))?;

    let timing_summary = crate::frame_composer::finalize_frame_timing(
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

/// Discovers all connected KMS connectors and their candidate CRTC/modes (`RUSTYOS-040`).
#[must_use]
pub fn find_all_connected_connectors(drm_device: &DrmDevice) -> Vec<PrimaryConnectorSetup> {
    find_all_connected_connectors_with_prefs(drm_device, true, None)
}

fn find_all_connected_connectors_with_prefs(
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

fn count_drm_cursor_planes(drm_fd: &DrmDeviceFd, universal_planes: bool) -> usize {
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
            syncobj_eventfd: true,
            cursor_size: (256, 256),
            crtc_count: 4,
            plane_count: 31,
            cursor_plane_count: 4,
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
        assert!(json.contains("\"syncobj_eventfd\": true"));
        assert!(json.contains("\"cursor_plane_count\": 4"));
        assert!(json.contains("\"preferred_mode\": \"1920x1080@60Hz\""));
        assert!(json.contains("\"gbm_egl_dmabuf_test_passed\": true"));
    }

    #[test]
    fn scoped_pointer_dirtying_limits_redraw_to_crossed_outputs() {
        use smithay::output::{Output, PhysicalProperties, Subpixel};

        let out1 = Output::new(
            "eDP-1".to_owned(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Test".to_owned(),
                model: "Primary".to_owned(),
            },
        );
        let mut map = crate::outputs::OutputMap::new(crate::outputs::OutputEntry::new(
            rusty_wm::OutputId(1),
            "eDP-1",
            out1,
            (0, 0).into(),
            (1920, 1080).into(),
            1.0,
            60_000,
            true,
        ));
        let out2 = Output::new(
            "DP-1".to_owned(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Test".to_owned(),
                model: "Secondary".to_owned(),
            },
        );
        map.add(crate::outputs::OutputEntry::new(
            rusty_wm::OutputId(2),
            "DP-1",
            out2,
            (1920, 0).into(),
            (1920, 1080).into(),
            1.0,
            144_000,
            false,
        ));

        // Motion completely inside display 1 should only dirty display 1.
        let inside_primary = output_ids_affected_by_pointer_motion(
            &map,
            (200.0, 200.0).into(),
            (400.0, 300.0).into(),
        );
        assert_eq!(inside_primary, vec![rusty_wm::OutputId(1)]);

        // Motion completely inside display 2 should only dirty display 2.
        let inside_secondary = output_ids_affected_by_pointer_motion(
            &map,
            (2500.0, 200.0).into(),
            (2700.0, 350.0).into(),
        );
        assert_eq!(inside_secondary, vec![rusty_wm::OutputId(2)]);

        // Motion crossing from display 1 into display 2 must dirty both so the old cursor disappears cleanly.
        let crossing = output_ids_affected_by_pointer_motion(
            &map,
            (1800.0, 500.0).into(),
            (2100.0, 500.0).into(),
        );
        assert_eq!(crossing, vec![rusty_wm::OutputId(1), rusty_wm::OutputId(2)]);
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

    #[test]
    fn mode_selector_prefers_highest_refresh_at_native_resolution() {
        let modes = [
            ModeDescriptor {
                size: (2560, 1440),
                vrefresh: 60,
                preferred: true,
                clock: 241_500,
            },
            ModeDescriptor {
                size: (2560, 1440),
                vrefresh: 144,
                preferred: false,
                clock: 586_000,
            },
            ModeDescriptor {
                size: (2560, 1440),
                vrefresh: 240,
                preferred: false,
                clock: 980_000,
            },
            // Lower resolution at higher refresh must not override preferred native size.
            ModeDescriptor {
                size: (1920, 1080),
                vrefresh: 360,
                preferred: false,
                clock: 700_000,
            },
        ];

        assert_eq!(select_best_mode_index(&modes, false), Some(0));
        assert_eq!(select_best_mode_index(&modes, true), Some(2));
        assert_eq!(select_best_mode_index(&[], true), None);
    }

    #[test]
    fn presentation_flags_and_vrr_refresh_metadata() {
        use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
        use smithay::wayland::presentation::Refresh;

        let regular = compute_drm_presentation_flags(false);
        assert!(regular.contains(wp_presentation_feedback::Kind::Vsync));
        assert!(regular.contains(wp_presentation_feedback::Kind::HwClock));
        assert!(!regular.contains(wp_presentation_feedback::Kind::ZeroCopy));

        let direct = compute_drm_presentation_flags(true);
        assert!(direct.contains(wp_presentation_feedback::Kind::Vsync));
        assert!(direct.contains(wp_presentation_feedback::Kind::HwClock));
        assert!(direct.contains(wp_presentation_feedback::Kind::ZeroCopy));

        assert!(matches!(
            compute_presentation_refresh(true, 144),
            Refresh::Variable(_)
        ));
        assert!(matches!(
            compute_presentation_refresh(false, 144),
            Refresh::Fixed(_)
        ));
    }
}
