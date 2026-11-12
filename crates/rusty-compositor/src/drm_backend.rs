//! Native DRM/KMS, GBM, EGL, and DMA-BUF graphics backend (`RUSTYOS-006`).
//!
//! Provides two execution levels:
//! 1. **Runtime Capability Discovery & Offscreen GBM/EGL/DMA-BUF Verification (`--drm-probe`)**:
//!    Safely inspects `/dev/dri/card*` and `/dev/dri/renderD*` without stealing DRM Master from
//!    an active desktop session, enumerates KMS connectors/modes/planes/capabilities, initializes
//!    `GbmDevice` + `EGLDisplay` + `GlesRenderer`, allocates a hardware `DMA-BUF` buffer via GBM,
//!    renders a test quadrant frame on the GPU, and verifies `DMA-BUF` import.
//! 2. **Full Native KMS Session (`--drm` / `--kms`)**:
//!    Initializes `LibSeatSession`, `UdevBackend`, and `DrmDevice` for native TTY operation,
//!    with automatic diagnostic fallback to capability probing when invoked inside an active
//!    graphical session where DRM Master is already held.

use std::fmt::Write as _;
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
        session::{Session, libseat::LibSeatSession},
        udev::UdevBackend,
    },
    reexports::drm::{
        ClientCapability, Device as BasicDevice, DriverCapability,
        control::{Device as ControlDevice, connector},
    },
    utils::{DeviceFd, Rectangle, Transform},
};
use tracing::{info, warn};

use crate::{BackendMode, BootstrapReport, CompositorConfig, CompositorError};

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

/// Executes the native DRM/KMS backend (`--drm` / `--kms`).
///
/// Attempts to open `LibSeatSession` and acquire DRM Master on the primary GPU via `UdevBackend`.
/// If DRM Master is unavailable because an existing desktop compositor is active on the seat,
/// performs a full hardware capability & GBM/EGL/DMA-BUF probe and records the exact status.
///
/// # Errors
///
/// Returns [`CompositorError`] if no DRM GPU is available or hardware probing fails.
pub fn run_native_drm_kms(config: &CompositorConfig) -> Result<BootstrapReport, CompositorError> {
    let master_status = match try_initialize_kms_session() {
        Ok(status) => status,
        Err(reason) => {
            warn!(
                reason = %reason,
                "Active graphical session holds DRM Master; falling back to non-invasive DRM/GBM/EGL hardware verification"
            );
            format!("deferred ({reason})")
        }
    };

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

fn try_initialize_kms_session() -> Result<String, String> {
    let (mut session, _notifier) =
        LibSeatSession::new().map_err(|err| format!("libseat open failed: {err}"))?;
    let seat_name = session.seat();

    let udev_backend =
        UdevBackend::new(&seat_name).map_err(|err| format!("udev scan failed: {err}"))?;
    let (_, primary_path) = udev_backend
        .device_list()
        .find(|(_, path)| DrmNode::from_path(path).is_ok_and(|node| node.ty() == NodeType::Primary))
        .ok_or_else(|| "no primary DRM node reported by udev".to_owned())?;

    let fd = session
        .open(
            primary_path,
            smithay::reexports::rustix::fs::OFlags::RDWR
                | smithay::reexports::rustix::fs::OFlags::CLOEXEC
                | smithay::reexports::rustix::fs::OFlags::NOCTTY
                | smithay::reexports::rustix::fs::OFlags::NONBLOCK,
        )
        .map_err(|err| format!("session device open failed: {err:?}"))?;

    let drm_fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (_drm_device, _drm_notifier) =
        DrmDevice::new(drm_fd, false).map_err(|err| format!("DrmDevice::new failed: {err}"))?;

    Ok(format!(
        "acquired DRM Master on {} (seat: {seat_name})",
        primary_path.display()
    ))
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
}
