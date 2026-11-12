//! `RustyOS` Wayland compositor core library.
//!
//! Provides the foundational crate boundary, CLI configuration, Wayland state
//! (`wl_compositor`, `wl_shm`, `xdg_wm_base`, `wl_seat`, `wl_output`), and the Phase 1
//! nested Smithay + Winit prototype (`RUSTYOS-002`).

pub mod config;
pub mod dbus_notifications;
pub mod drm_backend;
pub mod frame_timing;
pub mod icons;
pub mod input;
pub mod launcher;
pub mod outputs;
pub mod screenshot;
pub mod shell_render;
pub mod state;
pub mod surfaces;
pub mod telemetry;
pub mod text;
pub mod tray;
pub mod winit_backend;
pub mod xwayland;

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
    use crate::state::RustyCompositorState;
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

        // Test Notification HUD lifecycle & IPC commands (RUSTYOS-024)
        compositor.handle_ipc_request(rusty_ipc::ShellRequest::Notify {
            urgency: rusty_ipc::Urgency::Low,
            title: "Build".to_owned(),
            body: "Compiled".to_owned(),
        });
        let id2 = compositor.push_notification_at(
            rusty_ipc::Urgency::Critical,
            "Alert".to_owned(),
            "Low battery".to_owned(),
            std::time::Duration::from_secs(1),
        );
        let notif_snap = compositor.build_ipc_snapshot();
        assert_eq!(notif_snap.notification_count, 2);
        assert_eq!(notif_snap.notifications.len(), 2);

        compositor.handle_ipc_request(rusty_ipc::ShellRequest::DismissNotification { id: id2 });
        assert_eq!(compositor.notification_count, 1);

        compositor.expire_notifications_at(std::time::Duration::from_secs(100));
        assert_eq!(compositor.notification_count, 0);
        assert_eq!(compositor.notifications, Vec::new());

        // Test Lock Screen IPC transitions, overlay isolation & reduced-motion snap (`RUSTYOS-025`)
        compositor.handle_ipc_request(rusty_ipc::ShellRequest::ToggleLauncher);
        assert!(compositor.launcher_active);
        compositor.handle_ipc_request(rusty_ipc::ShellRequest::SetLockScreen { locked: true });
        assert!(compositor.screen_locked);
        assert!(!compositor.launcher_active);
        assert!((compositor.lock_screen_anim.target() - 1.0).abs() < f64::EPSILON);

        compositor.handle_ipc_request(rusty_ipc::ShellRequest::SetReducedMotion { enabled: true });
        assert!((compositor.lock_screen_anim.current() - 1.0).abs() < f64::EPSILON);

        compositor.handle_ipc_request(rusty_ipc::ShellRequest::SetLockScreen { locked: false });
        assert!(!compositor.screen_locked);
        assert!((compositor.lock_screen_anim.current() - 0.0).abs() < f64::EPSILON);

        // Test XWayland display propagation and D-Bus notification replacement (`RUSTYOS-026`)
        compositor.set_xwayland_display(Some(7));
        assert_eq!(compositor.xwayland_display, Some(7));
        assert_eq!(compositor.launcher.x11_display.as_deref(), Some(":7"));
        compositor.push_dbus_notification(
            42,
            0,
            rusty_ipc::Urgency::Normal,
            "App: Hello".to_owned(),
            "First".to_owned(),
            None,
        );
        assert_eq!(compositor.notification_count, 1);
        compositor.push_dbus_notification(
            43,
            42,
            rusty_ipc::Urgency::Critical,
            "App: Replaced".to_owned(),
            "Updated".to_owned(),
            None,
        );
        assert_eq!(compositor.notification_count, 1);
        assert_eq!(compositor.notifications[0].dbus_id, Some(43));
        assert_eq!(compositor.notifications[0].title, "App: Replaced");
        compositor.clear_notifications();

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

    #[test]
    fn keyboard_layout_badge_cycles_configured_layouts() -> Result<(), Box<dyn Error>> {
        let event_loop: EventLoop<'static, state::RustyCompositorState> = EventLoop::try_new()?;
        let display: Display<state::RustyCompositorState> = Display::new()?;

        let mut compositor = state::RustyCompositorState::new(
            &display,
            "wayland-kbd-test-0".to_owned(),
            event_loop.get_signal(),
            (1280, 800).into(),
        )?;

        let initial = compositor.active_kbd_layout.clone();
        assert_ne!(initial, "");

        let layout_count = compositor
            .user_config
            .keyboard_layout
            .split(',')
            .filter(|part| !part.trim().is_empty())
            .count();
        if layout_count >= 2 {
            compositor.cycle_keyboard_layout();
            if compositor.user_config.keyboard_layout == "us,ru" {
                assert_eq!(initial, "EN");
                assert_eq!(compositor.active_kbd_layout, "RU");
            }
            compositor.cycle_keyboard_layout();
            if compositor.user_config.keyboard_layout == "us,ru" {
                assert_eq!(compositor.active_kbd_layout, "EN");
            }
        }

        Ok(())
    }

    #[test]
    fn selection_protocols_register_and_update_clipboard_and_primary_state()
    -> Result<(), Box<dyn Error>> {
        use smithay::wayland::selection::{
            data_device::{
                clear_data_device_selection, current_data_device_selection_userdata,
                set_data_device_focus, set_data_device_selection,
            },
            primary_selection::{
                clear_primary_selection, current_primary_selection_userdata, set_primary_focus,
                set_primary_selection,
            },
        };

        let event_loop: EventLoop<'static, state::RustyCompositorState> = EventLoop::try_new()?;
        let display: Display<state::RustyCompositorState> = Display::new()?;

        let compositor = state::RustyCompositorState::new(
            &display,
            "wayland-sel-test-0".to_owned(),
            event_loop.get_signal(),
            (1280, 800).into(),
        )?;

        set_data_device_focus(&compositor.display_handle, &compositor.seat, None);
        set_primary_focus(&compositor.display_handle, &compositor.seat, None);

        set_data_device_selection(
            &compositor.display_handle,
            &compositor.seat,
            vec!["text/plain;charset=utf-8".to_owned()],
            (),
        );
        assert!(current_data_device_selection_userdata(&compositor.seat).is_some());
        clear_data_device_selection(&compositor.display_handle, &compositor.seat);
        assert!(current_data_device_selection_userdata(&compositor.seat).is_none());

        set_primary_selection(
            &compositor.display_handle,
            &compositor.seat,
            vec!["text/plain".to_owned()],
            (),
        );
        assert!(current_primary_selection_userdata(&compositor.seat).is_some());
        clear_primary_selection(&compositor.display_handle, &compositor.seat);
        assert!(current_primary_selection_userdata(&compositor.seat).is_none());

        Ok(())
    }

    #[test]
    fn csd_decoration_state_adjusts_titlebar_height_and_client_size() -> Result<(), Box<dyn Error>>
    {
        let event_loop: EventLoop<'static, state::RustyCompositorState> = EventLoop::try_new()?;
        let display: Display<state::RustyCompositorState> = Display::new()?;

        let mut compositor = state::RustyCompositorState::new(
            &display,
            "wayland-csd-test-0".to_owned(),
            event_loop.get_signal(),
            (1280, 800).into(),
        )?;

        let win_id = rusty_wm::WindowId(42);
        let rect = rusty_wm::Rect::new(100, 80, 800, 600);

        // Default Wayland window without CSD marker uses SSD titlebar.
        assert!(!compositor.window_uses_csd(win_id));
        assert_eq!(
            compositor.window_titlebar_height(win_id),
            shell_render::TITLEBAR_HEIGHT
        );
        assert_eq!(
            compositor.effective_client_size(win_id, rect),
            (800, 600 - shell_render::TITLEBAR_HEIGHT)
        );

        // Once negotiated as CSD, titlebar reserve drops to 0 and full height goes to client.
        compositor.xdg_decoration_requested.insert(win_id);
        compositor.csd_window_ids.insert(win_id);
        assert!(compositor.window_uses_csd(win_id));
        assert_eq!(compositor.window_titlebar_height(win_id), 0);
        assert_eq!(compositor.effective_client_size(win_id, rect), (800, 600));

        // Cleanup removes tracking state cleanly.
        compositor.unregister_window_decoration(win_id);
        assert!(!compositor.window_uses_csd(win_id));
        assert!(!compositor.xdg_decoration_requested.contains(&win_id));

        Ok(())
    }

    #[test]
    fn minimize_and_dock_restore_manage_focus_and_visibility() -> Result<(), Box<dyn Error>> {
        let event_loop: EventLoop<'static, state::RustyCompositorState> = EventLoop::try_new()?;
        let display: Display<state::RustyCompositorState> = Display::new()?;

        let mut compositor = state::RustyCompositorState::new(
            &display,
            "wayland-min-test-0".to_owned(),
            event_loop.get_signal(),
            (1280, 800).into(),
        )?;

        let w1 = compositor.wm.next_window_id();
        let _ = compositor.wm.map_window(w1, Some((600, 400)));
        let w2 = compositor.wm.next_window_id();
        let _ = compositor.wm.map_window(w2, Some((600, 400)));
        assert_eq!(compositor.wm.focused_window(), Some(w2));

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        compositor.minimize_window(w2, serial);
        assert!(compositor.is_window_minimized(w2));
        assert!(!compositor.is_window_minimized(w1));
        // Focus automatically falls back to the remaining visible window (w1).
        assert_eq!(compositor.wm.focused_window(), Some(w1));

        // Both windows remain in Dock order (`ordered_visible_window_ids`), with w2 at slot 1.
        assert_eq!(compositor.ordered_visible_window_ids(), vec![w1, w2]);

        // Clicking Dock slot for minimized w2 restores and refocuses it.
        compositor.handle_dock_hit(shell_render::DockHit::WindowSlot(1), serial);
        assert!(!compositor.is_window_minimized(w2));
        assert_eq!(compositor.wm.focused_window(), Some(w2));

        // Clicking Dock slot for already-focused w2 minimizes it again.
        compositor.handle_dock_hit(shell_render::DockHit::WindowSlot(1), serial);
        assert!(compositor.is_window_minimized(w2));
        assert_eq!(compositor.wm.focused_window(), Some(w1));

        Ok(())
    }

    #[test]
    fn alt_tab_switcher_steps_wraps_restores_minimized_and_cancels() -> Result<(), Box<dyn Error>> {
        let event_loop: EventLoop<'static, state::RustyCompositorState> = EventLoop::try_new()?;
        let display: Display<state::RustyCompositorState> = Display::new()?;

        let mut compositor = state::RustyCompositorState::new(
            &display,
            "wayland-switcher-test-0".to_owned(),
            event_loop.get_signal(),
            (1280, 800).into(),
        )?;

        let w1 = compositor.wm.next_window_id();
        let _ = compositor.wm.map_window(w1, Some((600, 400)));
        let w2 = compositor.wm.next_window_id();
        let _ = compositor.wm.map_window(w2, Some((600, 400)));
        let w3 = compositor.wm.next_window_id();
        let _ = compositor.wm.map_window(w3, Some((600, 400)));

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        compositor.minimize_window(w1, serial);
        // Active unminimized order (top-to-bottom): [w3, w2], then minimized [w1]
        assert_eq!(compositor.wm.focused_window(), Some(w3));

        // First Alt+Tab opens switcher highlighting second item (w2)
        compositor.switcher_step(1);
        let sw = compositor
            .window_switcher
            .as_ref()
            .ok_or("switcher should be open")?;
        assert_eq!(sw.window_ids, vec![w3, w2, w1]);
        assert_eq!(sw.selected, 1);

        // Escape cancels without changing focus or restoring minimized window
        compositor.switcher_cancel();
        assert!(compositor.window_switcher.is_none());
        assert_eq!(compositor.wm.focused_window(), Some(w3));
        assert!(compositor.is_window_minimized(w1));

        // Re-open and step forward twice to reach minimized w1, then commit to restore & focus
        compositor.switcher_step(1);
        compositor.switcher_step(1);
        assert_eq!(
            compositor.window_switcher.as_ref().map(|s| s.selected),
            Some(2)
        );
        compositor.switcher_commit(serial);
        assert!(compositor.window_switcher.is_none());
        assert!(!compositor.is_window_minimized(w1));
        assert_eq!(compositor.wm.focused_window(), Some(w1));

        Ok(())
    }

    #[test]
    fn multi_monitor_hotplug_window_migration_and_layout() -> Result<(), Box<dyn Error>> {
        let display: Display<RustyCompositorState> = Display::new()?;
        let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
        let mut compositor = RustyCompositorState::new(
            &display,
            "rusty-test-multi-output".to_owned(),
            event_loop.get_signal(),
            (1920, 1080).into(),
        )?;

        assert_eq!(compositor.output_map.count(), 1);
        assert_eq!(compositor.output_map.primary().id, rusty_wm::OutputId(1));

        // Add a secondary 144 Hz display via hotplug
        let next_loc = compositor.output_map.next_adjacent_location();
        assert_eq!(next_loc, (1920, 0).into());

        let out2_mode = smithay::output::Mode {
            size: (2560, 1440).into(),
            refresh: 144_000,
        };
        let out2_props = smithay::output::PhysicalProperties {
            size: (0, 0).into(),
            subpixel: smithay::output::Subpixel::Unknown,
            make: "RustyOS".to_owned(),
            model: "HDMI-A-1 Display".to_owned(),
        };
        let out2_id = compositor.add_output(
            "HDMI-A-1",
            out2_props,
            out2_mode,
            smithay::output::Scale::Integer(1),
            next_loc,
        );

        assert_eq!(compositor.output_map.count(), 2);
        assert_eq!(out2_id, rusty_wm::OutputId(2));

        // Total bounding geometry covers both monitors: 1920 + 2560 = 4480 width
        let total = compositor.total_output_bounds();
        assert_eq!(total.loc, (0, 0).into());
        assert_eq!(total.size, (4480, 1440).into());

        // Hit testing identifies monitor correctly
        assert_eq!(
            compositor.output_under((500.0, 500.0).into()),
            Some(rusty_wm::OutputId(1))
        );
        assert_eq!(
            compositor.output_under((2500.0, 500.0).into()),
            Some(rusty_wm::OutputId(2))
        );

        // Map window w1 on primary monitor (workspace 1)
        let w1 = compositor.wm.next_window_id();
        let _ = compositor.wm.map_window(w1, Some((800, 600)));
        assert_eq!(
            compositor.wm.window(w1).map(|w| w.workspace),
            Some(rusty_wm::WorkspaceId(1))
        );

        // Move window to next output (monitor 2)
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let moved = compositor.move_focused_to_next_output(serial);
        assert_eq!(moved, Some(w1));
        assert_eq!(compositor.wm.focused_output(), rusty_wm::OutputId(2));
        let win_geom = compositor
            .wm
            .window(w1)
            .map(|w| w.geometry)
            .unwrap_or_default();
        assert!(win_geom.x >= 1920);

        // Hotplug unplug of monitor 2: window migrates back to remaining monitor 1
        assert!(compositor.remove_output(out2_id));
        assert_eq!(compositor.output_map.count(), 1);
        assert_eq!(compositor.output_map.primary().id, rusty_wm::OutputId(1));
        assert_eq!(
            compositor.wm.window(w1).map(|w| w.workspace),
            Some(rusty_wm::WorkspaceId(1))
        );

        Ok(())
    }
}
