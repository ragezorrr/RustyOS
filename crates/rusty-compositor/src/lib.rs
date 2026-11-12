//! `RustyOS` Wayland compositor core library.
//!
//! Provides the foundational crate boundary, CLI configuration, Wayland state
//! (`wl_compositor`, `wl_shm`, `xdg_wm_base`, `wl_seat`, `wl_output`), and the Phase 1
//! nested Smithay + Winit prototype (`RUSTYOS-002`).

pub mod drm_backend;
pub mod frame_timing;
pub mod input;
pub mod shell_render;
pub mod state;
pub mod winit_backend;

use std::error::Error;
use std::fmt;

use drm_backend::DrmHardwareReport;
use frame_timing::FrameTimingSummary;

/// Execution backend mode for the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendMode {
    /// Dry-run initialization check used during bootstrap and headless CI validation.
    #[default]
    CheckOnly,
    /// Nested windowed prototype backend (`RUSTYOS-002` via Smithay + Winit).
    NestedWinit,
    /// Non-invasive DRM/GBM/EGL/DMA-BUF capability discovery & offscreen GPU probe (`RUSTYOS-006`).
    DrmProbe,
    /// Native DRM/KMS backend (`RUSTYOS-006` via `libseat`, `udev`, `DrmDevice`, and `GBM`).
    NativeDrmKms,
}

impl BackendMode {
    /// Returns the canonical identifier for the backend mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CheckOnly => "check-only",
            Self::NestedWinit => "nested-winit",
            Self::DrmProbe => "drm-probe",
            Self::NativeDrmKms => "native-drm-kms",
        }
    }
}

/// Startup configuration for `rusty-compositor`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompositorConfig {
    /// Selected backend execution mode.
    pub backend: BackendMode,
    /// Optional frame limit for automated smoke tests in nested mode.
    pub max_frames: Option<u64>,
    /// Optional Wayland client commands to spawn once the socket is ready.
    pub spawn_clients: Vec<String>,
    /// Optional file path to export frame timing JSON metrics on exit (`RUSTYOS-003`).
    pub metrics_out: Option<String>,
    /// Inject deterministic synthetic pointer input before each frame (`RUSTYOS-007`).
    pub bench_input: bool,
    /// Force full-output damage submission instead of tracked damage rectangles (`RUSTYOS-007`).
    pub force_full_damage: bool,
    /// Enable accessibility reduced-motion mode (`RUSTYOS-008`).
    pub reduced_motion: bool,
}

impl CompositorConfig {
    /// Creates a new configuration with the specified [`BackendMode`].
    #[must_use]
    pub const fn new(backend: BackendMode) -> Self {
        Self {
            backend,
            max_frames: None,
            spawn_clients: Vec::new(),
            metrics_out: None,
            bench_input: false,
            force_full_damage: false,
            reduced_motion: false,
        }
    }
}

/// Errors returned during compositor initialization or execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositorError {
    /// The requested backend is not yet implemented in the current phase.
    BackendNotImplemented(BackendMode),
    /// An unrecognized or malformed command-line argument was provided.
    InvalidArgument(String),
    /// Failed to initialize or run the `calloop` event loop.
    EventLoop(String),
    /// Failed to initialize the Wayland `Display`.
    WaylandDisplay(String),
    /// Failed to bind the Wayland listening socket.
    WaylandSocket(String),
    /// Failed to initialize the graphics/window backend.
    BackendInit(String),
    /// Failed to initialize the input seat (keyboard/pointer).
    SeatInitialization(String),
    /// Failed to write the frame timing metrics report.
    MetricsExport(String),
}

impl fmt::Display for CompositorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendNotImplemented(mode) => {
                write!(
                    f,
                    "backend '{}' is not implemented yet in the current phase",
                    mode.as_str()
                )
            }
            Self::InvalidArgument(arg) => {
                write!(f, "invalid command-line argument: '{arg}'")
            }
            Self::EventLoop(msg) => write!(f, "event loop error: {msg}"),
            Self::WaylandDisplay(msg) => write!(f, "wayland display error: {msg}"),
            Self::WaylandSocket(msg) => write!(f, "wayland socket error: {msg}"),
            Self::BackendInit(msg) => write!(f, "graphics backend initialization error: {msg}"),
            Self::SeatInitialization(msg) => write!(f, "seat initialization error: {msg}"),
            Self::MetricsExport(msg) => write!(f, "failed to export frame timing metrics: {msg}"),
        }
    }
}

impl Error for CompositorError {}

/// Outcome of a compositor run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapReport {
    /// Crate version string.
    pub version: &'static str,
    /// Validated backend mode.
    pub backend: BackendMode,
    /// Bound Wayland socket name if a Wayland server was started.
    pub socket_name: Option<String>,
    /// Collected frame timing summary if frames were rendered (`RUSTYOS-003`).
    pub frame_timing: Option<FrameTimingSummary>,
    /// Collected DRM/GBM/EGL hardware capability report (`RUSTYOS-006`).
    pub drm_report: Option<DrmHardwareReport>,
}

/// Parses command-line arguments (excluding program name) into [`CompositorConfig`].
///
/// Supported flags:
/// - `--check` / `--dry-run`: validate headless initialization without opening a window.
/// - `--nested` / `--winit`: run the nested Smithay + Winit compositor.
/// - `--drm` / `--kms`: select the native DRM/KMS backend (Phase 3).
/// - `--max-frames <N>`: stop automatically after rendering `<N>` frames in nested mode.
/// - `--spawn <CMD>`: spawn `<CMD>` with `WAYLAND_DISPLAY` set to the compositor socket.
///
/// # Errors
///
/// Returns [`CompositorError::InvalidArgument`] if an unknown flag or invalid value is passed.
pub fn parse_args<I, S>(args: I) -> Result<CompositorConfig, CompositorError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut config = CompositorConfig::default();
    let mut iter = args.into_iter();

    while let Some(raw) = iter.next() {
        let arg = raw.as_ref();
        match arg {
            "--check" | "--dry-run" => config.backend = BackendMode::CheckOnly,
            "--nested" | "--winit" => config.backend = BackendMode::NestedWinit,
            "--drm-probe" => config.backend = BackendMode::DrmProbe,
            "--drm" | "--kms" => config.backend = BackendMode::NativeDrmKms,
            "--max-frames" => {
                let Some(val) = iter.next() else {
                    return Err(CompositorError::InvalidArgument(
                        "--max-frames requires a positive integer".to_owned(),
                    ));
                };
                let parsed = val.as_ref().parse::<u64>().map_err(|_| {
                    CompositorError::InvalidArgument(format!(
                        "invalid --max-frames value: '{}'",
                        val.as_ref()
                    ))
                })?;
                if parsed == 0 {
                    return Err(CompositorError::InvalidArgument(
                        "--max-frames must be greater than 0".to_owned(),
                    ));
                }
                config.max_frames = Some(parsed);
            }
            "--spawn" => {
                let Some(cmd) = iter.next() else {
                    return Err(CompositorError::InvalidArgument(
                        "--spawn requires a command string".to_owned(),
                    ));
                };
                let trimmed = cmd.as_ref().trim();
                if trimmed.is_empty() {
                    return Err(CompositorError::InvalidArgument(
                        "--spawn command cannot be empty".to_owned(),
                    ));
                }
                config.spawn_clients.push(trimmed.to_owned());
            }
            "--metrics-out" => {
                let Some(path) = iter.next() else {
                    return Err(CompositorError::InvalidArgument(
                        "--metrics-out requires a file path".to_owned(),
                    ));
                };
                let trimmed = path.as_ref().trim();
                if trimmed.is_empty() {
                    return Err(CompositorError::InvalidArgument(
                        "--metrics-out path cannot be empty".to_owned(),
                    ));
                }
                config.metrics_out = Some(trimmed.to_owned());
            }
            "--bench-input" => config.bench_input = true,
            "--force-full-damage" => config.force_full_damage = true,
            "--reduced-motion" => config.reduced_motion = true,
            other => return Err(CompositorError::InvalidArgument(other.to_owned())),
        }
    }

    Ok(config)
}

/// Executes the compositor with the provided [`CompositorConfig`].
///
/// # Errors
///
/// Returns [`CompositorError`] if backend initialization fails or if an unimplemented backend
/// is requested.
pub fn run_bootstrap(config: &CompositorConfig) -> Result<BootstrapReport, CompositorError> {
    match config.backend {
        BackendMode::CheckOnly => {
            // Also verify that headless Wayland Display & state can be constructed cleanly.
            let event_loop =
                smithay::reexports::calloop::EventLoop::<state::RustyCompositorState>::try_new()
                    .map_err(|err| CompositorError::EventLoop(err.to_string()))?;
            let display = smithay::reexports::wayland_server::Display::new()
                .map_err(|err| CompositorError::WaylandDisplay(err.to_string()))?;
            let _state = state::RustyCompositorState::new(
                &display,
                "wayland-check".to_owned(),
                event_loop.get_signal(),
                (1280, 800).into(),
            )?;

            Ok(BootstrapReport {
                version: env!("CARGO_PKG_VERSION"),
                backend: config.backend,
                socket_name: None,
                frame_timing: None,
                drm_report: None,
            })
        }
        BackendMode::NestedWinit => winit_backend::run_nested_winit(config),
        BackendMode::DrmProbe => drm_backend::run_drm_probe(config),
        BackendMode::NativeDrmKms => drm_backend::run_native_drm_kms(config),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

    #[test]
    fn default_config_runs_check_only_bootstrap() {
        let config = CompositorConfig::default();
        assert_eq!(config.backend, BackendMode::CheckOnly);

        let report = run_bootstrap(&config);
        assert_eq!(
            report,
            Ok(BootstrapReport {
                version: env!("CARGO_PKG_VERSION"),
                backend: BackendMode::CheckOnly,
                socket_name: None,
                frame_timing: None,
                drm_report: None,
            })
        );
    }

    #[test]
    fn parse_args_recognizes_drm_and_probe_modes() {
        let probe = parse_args(["--drm-probe"]);
        assert_eq!(probe, Ok(CompositorConfig::new(BackendMode::DrmProbe)));

        let drm = parse_args(["--drm"]);
        assert_eq!(drm, Ok(CompositorConfig::new(BackendMode::NativeDrmKms)));
    }

    #[test]
    fn parse_args_handles_known_and_unknown_flags() {
        let cfg = parse_args(["--check"]);
        assert_eq!(cfg, Ok(CompositorConfig::new(BackendMode::CheckOnly)));

        let nested = parse_args([
            "--winit",
            "--max-frames",
            "10",
            "--spawn",
            "wayland-info",
            "--spawn",
            "es2gears_wayland",
            "--metrics-out",
            "/tmp/metrics.json",
            "--bench-input",
            "--force-full-damage",
            "--reduced-motion",
        ]);
        assert_eq!(
            nested,
            Ok(CompositorConfig {
                backend: BackendMode::NestedWinit,
                max_frames: Some(10),
                spawn_clients: vec!["wayland-info".to_owned(), "es2gears_wayland".to_owned()],
                metrics_out: Some("/tmp/metrics.json".to_owned()),
                bench_input: true,
                force_full_damage: true,
                reduced_motion: true,
            })
        );

        let invalid = parse_args(["--unknown-flag"]);
        assert_eq!(
            invalid,
            Err(CompositorError::InvalidArgument(
                "--unknown-flag".to_owned()
            ))
        );
    }

    #[test]
    fn headless_wayland_socket_and_state_lifecycle() -> Result<(), Box<dyn Error>> {
        use std::os::unix::net::UnixStream;

        let event_loop: EventLoop<'static, state::RustyCompositorState> = EventLoop::try_new()?;
        let mut display: Display<state::RustyCompositorState> = Display::new()?;

        let mut compositor = state::RustyCompositorState::new(
            &display,
            "wayland-test-0".to_owned(),
            event_loop.get_signal(),
            (1280, 800).into(),
        )?;

        // Connect an in-memory UnixStream pair as a Wayland client and dispatch protocol state.
        let (server_stream, _client_stream) = UnixStream::pair()?;
        compositor.insert_client_stream(server_stream);
        display.dispatch_clients(&mut compositor)?;
        display.flush_clients()?;

        assert!(compositor.running);
        assert!(!compositor.launcher_active);

        // Test Spotlight launcher IPC toggle cycle
        compositor.handle_ipc_request(rusty_ipc::ShellRequest::ToggleLauncher);
        assert!(compositor.launcher_active);
        let snap_open = compositor.build_ipc_snapshot();
        assert!(snap_open.launcher_active);

        compositor.handle_ipc_request(rusty_ipc::ShellRequest::ToggleLauncher);
        assert!(!compositor.launcher_active);
        let snap_closed = compositor.build_ipc_snapshot();
        assert!(!snap_closed.launcher_active);

        compositor.request_shutdown();
        assert!(!compositor.running);
        Ok(())
    }

    #[test]
    fn layer_shell_global_is_registered_and_layer_map_configured() -> Result<(), Box<dyn Error>> {
        let event_loop: EventLoop<'static, state::RustyCompositorState> = EventLoop::try_new()?;
        let display: Display<state::RustyCompositorState> = Display::new()?;

        let compositor = state::RustyCompositorState::new(
            &display,
            "wayland-layer-test-0".to_owned(),
            event_loop.get_signal(),
            (1920, 1080).into(),
        )?;

        // Verify layer shell global exists and layer map on output is initialized
        let _layer_global = compositor.wlr_layer_shell_state.shell_global();
        let layer_map = smithay::desktop::layer_map_for_output(&compositor.output);
        assert_eq!(layer_map.len(), 0);

        Ok(())
    }
}
