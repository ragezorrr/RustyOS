//! Native `xdg-desktop-portal` backend (`org.freedesktop.impl.portal.desktop.rustyos`)
//! and isolated PipeWire `Video/Source` stream management (`RUSTYOS-057`, ADR-0026).
//!
//! Exposes `org.freedesktop.impl.portal.Screenshot` (v3) and
//! `org.freedesktop.impl.portal.ScreenCast` (v6) on the user Session D-Bus via `zbus`,
//! forwarding interactive approval and frame capture to `rusty-compositor`, while delegating
//! PipeWire `pw_stream` protocol handling to the isolated `rusty-pipewire-stream` C worker.

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::future::Future;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, Receiver, Sender},
};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// Session D-Bus well-known name registered for the RustyOS portal backend.
pub const PORTAL_DBUS_NAME: &str = "org.freedesktop.impl.portal.desktop.rustyos";
/// Object path where RustyOS portal interfaces are exported.
pub const PORTAL_OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

/// Portal response code: user accepted / request succeeded.
pub const PORTAL_RESPONSE_OK: u32 = 0;
/// Portal response code: user cancelled or `Request.Close` fired.
pub const PORTAL_RESPONSE_CANCELLED: u32 = 1;
/// Portal response code: request failed or rejected by policy (e.g. locked screen).
pub const PORTAL_RESPONSE_OTHER: u32 = 2;

/// `org.freedesktop.impl.portal.Screenshot` target bit: full output/screen.
pub const SCREENSHOT_TARGET_SCREEN: u32 = 1;
/// `org.freedesktop.impl.portal.Screenshot` target bit: selected window.
pub const SCREENSHOT_TARGET_WINDOW: u32 = 2;
/// `org.freedesktop.impl.portal.Screenshot` target bit: area.
pub const SCREENSHOT_TARGET_AREA: u32 = 4;
/// `org.freedesktop.impl.portal.Screenshot` target bit: active window.
pub const SCREENSHOT_TARGET_ACTIVE_WINDOW: u32 = 8;
/// Supported screenshot targets advertised by `AvailableTargets`.
pub const SCREENSHOT_AVAILABLE_TARGETS: u32 =
    SCREENSHOT_TARGET_SCREEN | SCREENSHOT_TARGET_WINDOW | SCREENSHOT_TARGET_ACTIVE_WINDOW;

/// `org.freedesktop.impl.portal.ScreenCast` source type bit: physical/virtual monitor.
pub const SCREENCAST_SOURCE_MONITOR: u32 = 1;
/// `org.freedesktop.impl.portal.ScreenCast` source type bit: application window.
pub const SCREENCAST_SOURCE_WINDOW: u32 = 2;
/// Supported source types advertised by `AvailableSourceTypes`.
pub const SCREENCAST_AVAILABLE_SOURCE_TYPES: u32 =
    SCREENCAST_SOURCE_MONITOR | SCREENCAST_SOURCE_WINDOW;

/// `org.freedesktop.impl.portal.ScreenCast` cursor mode: cursor excluded.
pub const SCREENCAST_CURSOR_HIDDEN: u32 = 1;
/// `org.freedesktop.impl.portal.ScreenCast` cursor mode: cursor composited into video frames.
pub const SCREENCAST_CURSOR_EMBEDDED: u32 = 2;
/// Supported cursor modes advertised by `AvailableCursorModes`.
pub const SCREENCAST_AVAILABLE_CURSOR_MODES: u32 =
    SCREENCAST_CURSOR_HIDDEN | SCREENCAST_CURSOR_EMBEDDED;

/// Magic header word (`'RPW1'`) framing RGBA buffers sent to `rusty-pipewire-stream`.
pub const PIPEWIRE_FRAME_MAGIC: u32 = 0x5250_5731;
/// Minimum video frame dimension accepted by `rusty-pipewire-stream`.
pub const PIPEWIRE_MIN_DIM: u32 = 16;
/// Maximum video frame dimension accepted by `rusty-pipewire-stream`.
pub const PIPEWIRE_MAX_DIM: u32 = 8192;

const HELPER_READY_TIMEOUT: Duration = Duration::from_secs(5);

/// Active output metadata exposed to the portal picker UI (`RUSTYOS-057`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalOutputInfo {
    /// Internal `OutputId` raw numeric value.
    pub id: u64,
    /// Output connector/virtual name (e.g., `"eDP-1"`).
    pub name: String,
    /// Global logical X coordinate.
    pub x: i32,
    /// Global logical Y coordinate.
    pub y: i32,
    /// Logical/pixel capture width.
    pub width: i32,
    /// Logical/pixel capture height.
    pub height: i32,
    /// Rounded refresh rate in Hz.
    pub refresh_hz: u32,
    /// Whether this output is primary.
    pub is_primary: bool,
}

/// Active top-level window metadata exposed to the portal picker UI (`RUSTYOS-057`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalWindowInfo {
    /// Internal `WindowId` raw numeric value.
    pub id: u64,
    /// Sanitized visible window title.
    pub title: String,
    /// Sanitized application ID.
    pub app_id: String,
    /// Window X coordinate in global logical coordinates.
    pub x: i32,
    /// Window Y coordinate in global logical coordinates.
    pub y: i32,
    /// Window width in logical pixels.
    pub width: i32,
    /// Window height in logical pixels.
    pub height: i32,
    /// Whether the window is currently focused.
    pub focused: bool,
}

/// Snapshot of compositor outputs, windows, and lock state shared with the portal backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortalDesktopSnapshot {
    /// Connected outputs ordered primary-first.
    pub outputs: Vec<PortalOutputInfo>,
    /// Open windows in stacking/workspace order.
    pub windows: Vec<PortalWindowInfo>,
    /// Whether the compositor session lock is currently engaged (`SECURITY.md`).
    pub screen_locked: bool,
}

/// Discriminator for interactive portal approval prompts shown inside the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalPromptKind {
    /// Interactive `org.freedesktop.impl.portal.Screenshot` request.
    Screenshot {
        /// Initial target bit (`SCREENSHOT_TARGET_*`).
        requested_target: u32,
    },
    /// Interactive `org.freedesktop.impl.portal.ScreenCast.Start` approval & source selection.
    ScreenCast {
        /// D-Bus `session_handle` object path.
        session_handle: String,
        /// Allowed `SCREENCAST_SOURCE_*` bitmask.
        allowed_types: u32,
        /// Whether client allows multi-stream selection.
        multiple: bool,
        /// Initial cursor mode (`SCREENCAST_CURSOR_*`).
        cursor_mode: u32,
        /// Requested `persist_mode`.
        persist_mode: u32,
        /// Optional previously persisted target mapping (`output:...` / `window:...`).
        restored_target: Option<String>,
    },
}

/// Interactive prompt forwarded from the portal D-Bus worker to `rusty-compositor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalPromptRequest {
    /// Monotonic backend request identifier.
    pub request_id: u64,
    /// Caller D-Bus `org.freedesktop.impl.portal.Request` path.
    pub request_handle: String,
    /// Calling desktop application ID (or empty if unsandboxed).
    pub app_id: String,
    /// Optional caller window identifier string.
    pub parent_window: String,
    /// Prompt kind and parameters.
    pub kind: PortalPromptKind,
}

/// User-selected capture target returned from the compositor approval modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalSelection {
    /// Capture target chosen for `Screenshot`.
    Screenshot {
        /// Target bit (`1 = Screen`, `2 = Window`, `8 = ActiveWindow`).
        target: u32,
        /// Selected output ID when capturing a screen.
        output_id: Option<u64>,
        /// Selected window ID when capturing a window.
        window_id: Option<u64>,
    },
    /// Capture target chosen for `ScreenCast`.
    ScreenCast {
        /// Single chosen `SCREENCAST_SOURCE_*` value (`1` or `2`).
        source_type: u32,
        /// Selected output ID for `Monitor` capture.
        output_id: Option<u64>,
        /// Selected window ID for `Window` capture.
        window_id: Option<u64>,
        /// Chosen cursor mode (`1 = Hidden`, `2 = Embedded`).
        cursor_mode: u32,
        /// Effective persist mode.
        persist_mode: u32,
    },
}

/// Metadata for an active PipeWire video stream returned by `ScreenCast.Start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalStreamDescriptor {
    /// Legacy PipeWire node ID.
    pub node_id: u32,
    /// Monotonic 64-bit PipeWire `object.serial` (`PW_KEY_TARGET_OBJECT`).
    pub pipewire_serial: u64,
    /// Position X in compositor coordinate space.
    pub x: i32,
    /// Position Y in compositor coordinate space.
    pub y: i32,
    /// Width in compositor coordinate space.
    pub width: i32,
    /// Height in compositor coordinate space.
    pub height: i32,
    /// `SCREENCAST_SOURCE_MONITOR` or `SCREENCAST_SOURCE_WINDOW`.
    pub source_type: u32,
    /// Stable mapping ID identifying the stream source.
    pub mapping_id: String,
}

/// Asynchronous events sent from the portal D-Bus worker to `rusty-compositor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalEvent {
    /// Show the native Liquid Glass source-picker / consent dialog.
    ShowPrompt(PortalPromptRequest),
    /// Perform an immediate non-interactive screenshot capture.
    CaptureScreenshot {
        /// Backend request identifier.
        request_id: u64,
        /// Sanitized caller `app_id`.
        app_id: String,
        /// Target bit (`SCREENSHOT_TARGET_*`).
        target: u32,
        /// Optional preselected output ID.
        output_id: Option<u64>,
        /// Optional preselected window ID.
        window_id: Option<u64>,
    },
    /// Enter interactive pixel color picking mode (`Screenshot.PickColor`).
    PickColor {
        /// Backend request identifier.
        request_id: u64,
        /// Sanitized caller `app_id`.
        app_id: String,
    },
    /// Start PipeWire frame streaming for an approved `ScreenCast` session.
    StartScreenCast {
        /// Backend request identifier awaiting `ScreenCastResult`.
        request_id: u64,
        /// Session handle path owning the PipeWire stream(s).
        session_handle: String,
        /// Sanitized caller `app_id`.
        app_id: String,
        /// Approved capture selection.
        selection: PortalSelection,
    },
    /// Stop all PipeWire streams associated with `session_handle`.
    StopSession {
        /// Closed D-Bus session object path.
        session_handle: String,
    },
    /// Dismiss any open modal prompt/color-pick for `request_id` (`Request.Close`).
    CancelRequest {
        /// Cancelled backend request identifier.
        request_id: u64,
    },
}

/// Completion messages sent from `rusty-compositor` back to the portal D-Bus worker.
#[derive(Debug, Clone, PartialEq)]
pub enum PortalReply {
    /// Result of an interactive approval dialog (`ShowPrompt`).
    PromptDecision {
        /// Request identifier from [`PortalPromptRequest`].
        request_id: u64,
        /// `true` if user approved the request.
        accepted: bool,
        /// Selected capture target when `accepted` is `true`.
        selection: Option<PortalSelection>,
    },
    /// Saved PNG file URI (`file:///...`) from `Screenshot` or `CaptureScreenshot`.
    ScreenshotResult {
        /// Request identifier awaiting the screenshot URI.
        request_id: u64,
        /// Resulting `file://` URI or error string.
        result: Result<String, String>,
    },
    /// Normalized sRGB `(r, g, b)` color (`0.0..=1.0`) from `PickColor`.
    PickColorResult {
        /// Request identifier awaiting the color sample.
        request_id: u64,
        /// Resulting RGB tuple or cancel/error reason.
        result: Result<(f64, f64, f64), String>,
    },
    /// Active PipeWire stream descriptor(s) from `StartScreenCast`.
    ScreenCastResult {
        /// Request identifier awaiting stream startup.
        request_id: u64,
        /// Session handle owning the stream.
        session_handle: String,
        /// Active PipeWire stream metadata or startup failure reason.
        result: Result<Vec<PortalStreamDescriptor>, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScreenCastSessionConfig {
    app_id: String,
    source_types: u32,
    multiple: bool,
    cursor_mode: u32,
    persist_mode: u32,
    restored_target: Option<String>,
}

/// Configuration for launching an isolated `rusty-pipewire-stream` child worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeWireStreamConfig {
    /// Initial frame width (`16..=8192`).
    pub width: u32,
    /// Initial frame height (`16..=8192`).
    pub height: u32,
    /// Target frame cadence in FPS (`1..=240`).
    pub fps: u32,
    /// Client application label attached to `PW_KEY_APP_NAME`.
    pub app_name: String,
    /// Stream description attached to `PW_KEY_NODE_DESCRIPTION`.
    pub node_description: String,
}

struct EncodedFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct FrameSlot {
    pending: Option<EncodedFrame>,
    shutdown: bool,
}

/// Live handle to an isolated `rusty-pipewire-stream` child publishing a `Video/Source` node.
pub struct PipeWireStreamHandle {
    node_id: u32,
    pipewire_serial: u64,
    consumer_active: Arc<AtomicBool>,
    stream_alive: Arc<AtomicBool>,
    slot: Arc<(Mutex<FrameSlot>, Condvar)>,
    child: Arc<Mutex<Option<Child>>>,
    writer_thread: Option<JoinHandle<()>>,
    watcher_thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for PipeWireStreamHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipeWireStreamHandle")
            .field("node_id", &self.node_id)
            .field("pipewire_serial", &self.pipewire_serial)
            .field("consumer_active", &self.is_consumer_active())
            .field("alive", &self.is_alive())
            .finish_non_exhaustive()
    }
}

impl Drop for PipeWireStreamHandle {
    fn drop(&mut self) {
        self.shutdown_internal();
    }
}

impl PipeWireStreamHandle {
    /// Resolves `rusty-pipewire-stream` and spawns an isolated `Video/Source` worker.
    ///
    /// # Errors
    ///
    /// Returns an error if the helper binary cannot be located, fails to connect to PipeWire,
    /// or times out before advertising a valid `node_id`.
    pub fn spawn(config: &PipeWireStreamConfig) -> Result<Self, String> {
        let helper = resolve_pipewire_helper_path()?;
        Self::spawn_with_binary(&helper, config)
    }

    /// Spawns `binary_path` with `config`, waiting until `READY <node_id> <serial>` is received.
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails, `READY` is invalid/timed out, or PipeWire errors out.
    pub fn spawn_with_binary(
        binary_path: &Path,
        config: &PipeWireStreamConfig,
    ) -> Result<Self, String> {
        let mut child = spawn_worker_child(binary_path, config)?;
        let Some(stdin) = child.stdin.take() else {
            terminate_child(&mut child);
            return Err("pipewire worker missing stdin pipe".to_owned());
        };
        let Some(stdout) = child.stdout.take() else {
            terminate_child(&mut child);
            return Err("pipewire worker missing stdout pipe".to_owned());
        };

        let consumer_active = Arc::new(AtomicBool::new(false));
        let stream_alive = Arc::new(AtomicBool::new(true));
        let (ready_rx, watcher_thread) = match spawn_worker_watcher(
            stdout,
            Arc::clone(&consumer_active),
            Arc::clone(&stream_alive),
        ) {
            Ok(res) => res,
            Err(err) => {
                terminate_child(&mut child);
                return Err(err);
            }
        };

        let (node_id, pipewire_serial) = match ready_rx.recv_timeout(HELPER_READY_TIMEOUT) {
            Ok(Ok(ids)) => ids,
            Ok(Err(err)) => {
                terminate_child(&mut child);
                let _ = watcher_thread.join();
                return Err(err);
            }
            Err(_) => {
                terminate_child(&mut child);
                let _ = watcher_thread.join();
                return Err("timed out waiting for PipeWire Video/Source stream".to_owned());
            }
        };

        let slot = Arc::new((
            Mutex::new(FrameSlot {
                pending: None,
                shutdown: false,
            }),
            Condvar::new(),
        ));
        let writer_slot = Arc::clone(&slot);
        let writer_alive = Arc::clone(&stream_alive);

        let writer_thread = match thread::Builder::new()
            .name("rusty-pw-write".to_owned())
            .spawn(move || {
                run_frame_writer_loop(stdin, &writer_slot, &writer_alive);
            }) {
            Ok(t) => t,
            Err(err) => {
                terminate_child(&mut child);
                let _ = watcher_thread.join();
                return Err(format!("failed to start pipewire writer thread: {err}"));
            }
        };

        Ok(Self {
            node_id,
            pipewire_serial,
            consumer_active,
            stream_alive,
            slot,
            child: Arc::new(Mutex::new(Some(child))),
            writer_thread: Some(writer_thread),
            watcher_thread: Some(watcher_thread),
        })
    }

    /// Returns the PipeWire node ID assigned to this stream.
    #[must_use]
    pub const fn node_id(&self) -> u32 {
        self.node_id
    }

    /// Returns the monotonic PipeWire `object.serial` assigned to this stream.
    #[must_use]
    pub const fn pipewire_serial(&self) -> u64 {
        self.pipewire_serial
    }

    /// Returns `true` while the PipeWire worker process has not exited or errored.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.stream_alive.load(Ordering::Acquire)
    }

    /// Returns `true` when at least one PipeWire consumer is linked and streaming.
    #[must_use]
    pub fn is_consumer_active(&self) -> bool {
        self.consumer_active.load(Ordering::Acquire)
    }

    /// Non-blockingly queues the latest RGBA frame (`width * height * 4` bytes) for transmission.
    ///
    /// Replaces any unconsumed buffered frame so slow consumers never stall compositor frame pacing.
    #[must_use]
    pub fn push_frame(&self, width: u32, height: u32, rgba: Vec<u8>) -> bool {
        if !self.is_alive() {
            return false;
        }
        let Some(expected) = expected_rgba_len(width, height) else {
            return false;
        };
        if rgba.len() != expected {
            return false;
        }
        let (lock, cvar) = &*self.slot;
        let Ok(mut guard) = lock.lock() else {
            return false;
        };
        if guard.shutdown {
            return false;
        }
        guard.pending = Some(EncodedFrame {
            width,
            height,
            rgba,
        });
        cvar.notify_one();
        true
    }

    fn shutdown_internal(&mut self) {
        self.stream_alive.store(false, Ordering::Release);
        {
            let (lock, cvar) = &*self.slot;
            if let Ok(mut guard) = lock.lock() {
                guard.shutdown = true;
                guard.pending = None;
                cvar.notify_all();
            }
        }
        if let Some(handle) = self.writer_thread.take() {
            let _ = handle.join();
        }
        if let Ok(mut guard) = self.child.lock()
            && let Some(mut child) = guard.take()
            && !matches!(child.try_wait(), Ok(Some(_)))
        {
            terminate_child(&mut child);
        }
        if let Some(handle) = self.watcher_thread.take() {
            let _ = handle.join();
        }
    }
}

fn spawn_worker_child(binary_path: &Path, config: &PipeWireStreamConfig) -> Result<Child, String> {
    let width = config.width.clamp(PIPEWIRE_MIN_DIM, PIPEWIRE_MAX_DIM);
    let height = config.height.clamp(PIPEWIRE_MIN_DIM, PIPEWIRE_MAX_DIM);
    let fps = config.fps.clamp(1, 240);
    let app_name = sanitize_portal_text(&config.app_name, "RustyOS Client", 64);
    let node_desc = sanitize_portal_text(&config.node_description, "RustyOS ScreenCast", 80);

    Command::new(binary_path)
        .args(["--width", &width.to_string()])
        .args(["--height", &height.to_string()])
        .args(["--fps", &fps.to_string()])
        .args(["--app-name", &app_name])
        .args(["--node-desc", &node_desc])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to spawn {}: {err}", binary_path.display()))
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

type ReadyRx = Receiver<Result<(u32, u64), String>>;

fn spawn_worker_watcher(
    stdout: std::process::ChildStdout,
    consumer_active: Arc<AtomicBool>,
    stream_alive: Arc<AtomicBool>,
) -> Result<(ReadyRx, JoinHandle<()>), String> {
    let mut reader = BufReader::new(stdout);
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(u32, u64), String>>();
    let thread = thread::Builder::new()
        .name("rusty-pw-watch".to_owned())
        .spawn(move || {
            let mut sent_ready = false;
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        if !sent_ready {
                            let _ = ready_tx.send(Err(
                                "pipewire worker closed before reporting READY".to_owned(),
                            ));
                        }
                        stream_alive.store(false, Ordering::Release);
                        break;
                    }
                    Ok(_) => match parse_worker_line(&line) {
                        Some(WorkerStatusLine::Ready { node_id, serial }) => {
                            if !sent_ready {
                                sent_ready = true;
                                let _ = ready_tx.send(Ok((node_id, serial)));
                            }
                        }
                        Some(WorkerStatusLine::StateStreaming) => {
                            consumer_active.store(true, Ordering::Release);
                        }
                        Some(WorkerStatusLine::StatePaused) => {
                            consumer_active.store(false, Ordering::Release);
                        }
                        Some(WorkerStatusLine::Error(msg)) => {
                            if !sent_ready {
                                let _ = ready_tx.send(Err(msg));
                            }
                            stream_alive.store(false, Ordering::Release);
                            break;
                        }
                        None => {}
                    },
                }
            }
        })
        .map_err(|err| format!("failed to start pipewire watcher thread: {err}"))?;
    Ok((ready_rx, thread))
}

fn expected_rgba_len(width: u32, height: u32) -> Option<usize> {
    if !(PIPEWIRE_MIN_DIM..=PIPEWIRE_MAX_DIM).contains(&width)
        || !(PIPEWIRE_MIN_DIM..=PIPEWIRE_MAX_DIM).contains(&height)
    {
        return None;
    }
    let w = usize::try_from(width).ok()?;
    let h = usize::try_from(height).ok()?;
    w.checked_mul(h)?.checked_mul(4)
}

/// Serializes the 20-byte `RPW1` binary frame packet header (`magic, width, height, stride, flags`).
#[must_use]
pub fn encode_pipewire_frame_header(width: u32, height: u32, stride: u32) -> [u8; 20] {
    let mut hdr = [0u8; 20];
    hdr[0..4].copy_from_slice(&PIPEWIRE_FRAME_MAGIC.to_le_bytes());
    hdr[4..8].copy_from_slice(&width.to_le_bytes());
    hdr[8..12].copy_from_slice(&height.to_le_bytes());
    hdr[12..16].copy_from_slice(&stride.to_le_bytes());
    hdr
}

fn run_frame_writer_loop(
    stdin: std::process::ChildStdin,
    slot: &Arc<(Mutex<FrameSlot>, Condvar)>,
    alive: &Arc<AtomicBool>,
) {
    let mut writer = BufWriter::with_capacity(64 * 1024, stdin);
    let (lock, cvar) = &**slot;

    loop {
        let frame = {
            let mut guard = match lock.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            while guard.pending.is_none() && !guard.shutdown {
                guard = match cvar.wait(guard) {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
            }
            if guard.shutdown {
                break;
            }
            guard.pending.take()
        };

        let Some(frame) = frame else {
            continue;
        };
        let Some(stride) = frame.width.checked_mul(4) else {
            continue;
        };
        let hdr = encode_pipewire_frame_header(frame.width, frame.height, stride);
        if writer.write_all(&hdr).is_err()
            || writer.write_all(&frame.rgba).is_err()
            || writer.flush().is_err()
        {
            alive.store(false, Ordering::Release);
            break;
        }
    }
    let _ = writer.flush();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStatusLine {
    Ready { node_id: u32, serial: u64 },
    StateStreaming,
    StatePaused,
    Error(String),
}

/// Parses one stdout line from `rusty-pipewire-stream`.
#[must_use]
pub fn parse_worker_line(raw: &str) -> Option<WorkerStatusLine> {
    let trimmed = raw.trim();
    if trimmed == "STATE streaming" {
        return Some(WorkerStatusLine::StateStreaming);
    }
    if trimmed == "STATE paused" {
        return Some(WorkerStatusLine::StatePaused);
    }
    if let Some(rest) = trimmed.strip_prefix("READY ") {
        let mut parts = rest.split_whitespace();
        let node_id = parts.next()?.parse::<u32>().ok()?;
        let serial = parts.next()?.parse::<u64>().ok()?;
        return Some(WorkerStatusLine::Ready { node_id, serial });
    }
    if let Some(err) = trimmed.strip_prefix("ERROR ") {
        return Some(WorkerStatusLine::Error(err.trim().to_owned()));
    }
    None
}

/// Resolves the `rusty-pipewire-stream` helper binary location cleanly across dev/install paths.
///
/// Search order:
/// 1. Sibling binary beside the running executable (`${BIN_DIR}/rusty-pipewire-stream`),
/// 2. `PATH` lookup,
/// 3. Compile-time `RUSTY_PIPEWIRE_STREAM_BIN` from `build.rs`.
///
/// # Errors
///
/// Returns an error if no executable `rusty-pipewire-stream` binary exists in those locations.
pub fn resolve_pipewire_helper_path() -> Result<PathBuf, String> {
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join("rusty-pipewire-stream");
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("rusty-pipewire-stream");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    if let Some(built) = option_env!("RUSTY_PIPEWIRE_STREAM_BIN") {
        let path = PathBuf::from(built);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(
        "rusty-pipewire-stream helper unavailable (build/install libpipewire-0.3 worker)"
            .to_owned(),
    )
}

/// Sanitizes portal strings (`app_id`, window titles, output names) by stripping control chars.
#[must_use]
pub fn sanitize_portal_text(raw: &str, fallback: &str, max_chars: usize) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Converts a filesystem path into a canonical percent-encoded `file://` URI for `Screenshot`.
#[must_use]
pub fn path_to_file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut out = String::with_capacity(raw.len() + 16);
    out.push_str("file://");
    for byte in raw.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(*byte));
            }
            other => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
}

/// Converts an opaque RGBA `[u8; 4]` pixel into normalized sRGB `(f64, f64, f64)` in `0.0..=1.0`.
#[must_use]
pub fn rgba_pixel_to_portal_color(pixel: [u8; 4]) -> (f64, f64, f64) {
    (
        f64::from(pixel[0]) / 255.0,
        f64::from(pixel[1]) / 255.0,
        f64::from(pixel[2]) / 255.0,
    )
}

/// Converts a [`PortalStreamDescriptor`] slice into the D-Bus `a(ua{sv})` value expected by `Start`.
#[must_use]
pub fn encode_screencast_streams_value(
    streams: &[PortalStreamDescriptor],
) -> Vec<(u32, HashMap<String, OwnedValue>)> {
    let mut out = Vec::with_capacity(streams.len());
    for stream in streams {
        let mut props: HashMap<String, OwnedValue> = HashMap::new();
        if stream.source_type == SCREENCAST_SOURCE_MONITOR
            && let Ok(v) = OwnedValue::try_from(Value::from((stream.x, stream.y)))
        {
            props.insert("position".to_owned(), v);
        }
        if let Ok(v) =
            OwnedValue::try_from(Value::from((stream.width.max(1), stream.height.max(1))))
        {
            props.insert("size".to_owned(), v);
        }
        if let Ok(v) = OwnedValue::try_from(Value::from(stream.source_type)) {
            props.insert("source_type".to_owned(), v);
        }
        if !stream.mapping_id.is_empty()
            && let Ok(v) = OwnedValue::try_from(Value::from(stream.mapping_id.clone()))
        {
            props.insert("mapping_id".to_owned(), v);
        }
        if let Ok(v) = OwnedValue::try_from(Value::from(stream.pipewire_serial)) {
            props.insert("pipewire-serial".to_owned(), v);
        }
        out.push((stream.node_id, props));
    }
    out
}

/// Minimal `std` single-shot future so portal D-Bus calls wait without blocking `zbus`.
struct AsyncSignal<T> {
    state: Arc<Mutex<SignalState<T>>>,
}

struct SignalState<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

struct AsyncSignalTx<T> {
    state: Arc<Mutex<SignalState<T>>>,
}

impl<T> AsyncSignal<T> {
    fn channel() -> (AsyncSignalTx<T>, Self) {
        let state = Arc::new(Mutex::new(SignalState {
            value: None,
            waker: None,
        }));
        (
            AsyncSignalTx {
                state: Arc::clone(&state),
            },
            Self { state },
        )
    }
}

impl<T> AsyncSignalTx<T> {
    fn send(self, val: T) {
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.value.is_none() {
            guard.value = Some(val);
        }
        if let Some(waker) = guard.waker.take() {
            waker.wake();
        }
    }
}

impl<T> Future for AsyncSignal<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(val) = guard.value.take() {
            Poll::Ready(val)
        } else {
            guard.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

enum PendingAwaiter {
    Prompt(AsyncSignalTx<(bool, Option<PortalSelection>)>),
    Screenshot(AsyncSignalTx<Result<String, String>>),
    PickColor(AsyncSignalTx<Result<(f64, f64, f64), String>>),
    ScreenCast(AsyncSignalTx<Result<Vec<PortalStreamDescriptor>, String>>),
}

struct PendingEntry {
    handle_path: Option<String>,
    session_handle: Option<String>,
    awaiter: PendingAwaiter,
}

struct PortalSharedState {
    snapshot: Mutex<PortalDesktopSnapshot>,
    sessions: Mutex<HashMap<String, ScreenCastSessionConfig>>,
    pending: Mutex<HashMap<u64, PendingEntry>>,
    next_request_id: AtomicU64,
    events_tx: Sender<PortalEvent>,
    replies_tap: Mutex<Option<Sender<PortalReply>>>,
}

impl PortalSharedState {
    fn alloc_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    fn is_screen_locked(&self) -> bool {
        self.snapshot.lock().is_ok_and(|s| s.screen_locked)
    }

    fn cancel_request(&self, request_id: u64) {
        let removed = self
            .pending
            .lock()
            .ok()
            .and_then(|mut map| map.remove(&request_id));
        if let Some(entry) = removed {
            match entry.awaiter {
                PendingAwaiter::Prompt(tx) => tx.send((false, None)),
                PendingAwaiter::Screenshot(tx) => tx.send(Err("cancelled".to_owned())),
                PendingAwaiter::PickColor(tx) => tx.send(Err("cancelled".to_owned())),
                PendingAwaiter::ScreenCast(tx) => tx.send(Err("cancelled".to_owned())),
            }
            let _ = self
                .events_tx
                .send(PortalEvent::CancelRequest { request_id });
        }
    }

    fn cancel_session_pending(&self, session_handle: &str) {
        let matching_ids: Vec<u64> = self
            .pending
            .lock()
            .map(|map| {
                map.iter()
                    .filter(|(_, e)| e.session_handle.as_deref() == Some(session_handle))
                    .map(|(id, _)| *id)
                    .collect()
            })
            .unwrap_or_default();
        for id in matching_ids {
            self.cancel_request(id);
        }
    }

    fn deliver_reply(&self, reply: PortalReply) {
        if let Ok(guard) = self.replies_tap.lock()
            && let Some(tap) = guard.as_ref()
        {
            let _ = tap.send(reply.clone());
        }
        match reply {
            PortalReply::PromptDecision {
                request_id,
                accepted,
                selection,
            } => {
                if let Some(entry) = self
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut map| map.remove(&request_id))
                    && let PendingAwaiter::Prompt(tx) = entry.awaiter
                {
                    tx.send((accepted, selection));
                }
            }
            PortalReply::ScreenshotResult { request_id, result } => {
                if let Some(entry) = self
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut map| map.remove(&request_id))
                    && let PendingAwaiter::Screenshot(tx) = entry.awaiter
                {
                    tx.send(result);
                }
            }
            PortalReply::PickColorResult { request_id, result } => {
                if let Some(entry) = self
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut map| map.remove(&request_id))
                    && let PendingAwaiter::PickColor(tx) = entry.awaiter
                {
                    tx.send(result);
                }
            }
            PortalReply::ScreenCastResult {
                request_id,
                session_handle: _,
                result,
            } => {
                if let Some(entry) = self
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut map| map.remove(&request_id))
                    && let PendingAwaiter::ScreenCast(tx) = entry.awaiter
                {
                    tx.send(result);
                }
            }
        }
    }
}

struct PortalRequestIface {
    request_id: u64,
    shared: Arc<PortalSharedState>,
}

#[interface(name = "org.freedesktop.impl.portal.Request")]
impl PortalRequestIface {
    async fn close(&self, #[zbus(connection)] conn: &zbus::Connection) -> zbus::fdo::Result<()> {
        let handle = self.shared.pending.lock().ok().and_then(|map| {
            map.get(&self.request_id)
                .and_then(|e| e.handle_path.clone())
        });
        self.shared.cancel_request(self.request_id);
        if let Some(path) = handle {
            let _ = conn.object_server().remove::<Self, _>(path.as_str()).await;
        }
        Ok(())
    }
}

struct PortalSessionIface {
    session_handle: String,
    shared: Arc<PortalSharedState>,
}

#[interface(name = "org.freedesktop.impl.portal.Session")]
impl PortalSessionIface {
    async fn close(&self, #[zbus(connection)] conn: &zbus::Connection) -> zbus::fdo::Result<()> {
        if let Ok(mut guard) = self.shared.sessions.lock() {
            guard.remove(&self.session_handle);
        }
        self.shared.cancel_session_pending(&self.session_handle);
        let _ = self.shared.events_tx.send(PortalEvent::StopSession {
            session_handle: self.session_handle.clone(),
        });
        let _ = conn
            .object_server()
            .remove::<Self, _>(self.session_handle.as_str())
            .await;
        Ok(())
    }

    #[zbus(signal)]
    async fn closed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        let _ = self;
        1
    }
}

struct ScreenshotPortalIface {
    shared: Arc<PortalSharedState>,
}

#[interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl ScreenshotPortalIface {
    #[allow(clippy::needless_pass_by_value)]
    async fn screenshot(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        if self.shared.is_screen_locked() {
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        }

        let interactive = opt_bool(&options, "interactive").unwrap_or(false);
        let requested_target = opt_u32(&options, "target")
            .filter(|t| {
                *t == SCREENSHOT_TARGET_SCREEN
                    || *t == SCREENSHOT_TARGET_WINDOW
                    || *t == SCREENSHOT_TARGET_ACTIVE_WINDOW
            })
            .unwrap_or(SCREENSHOT_TARGET_SCREEN);
        let clean_app = sanitize_portal_text(&app_id, "Desktop App", 64);
        let handle_str = handle.as_str().to_owned();

        let (selected_target, output_id, window_id) = if interactive {
            let prompt_req_id = self.shared.alloc_request_id();
            register_request_object(conn, &self.shared, prompt_req_id, &handle_str).await;

            let (tx, rx) = AsyncSignal::channel();
            insert_pending(
                &self.shared,
                prompt_req_id,
                Some(handle_str.clone()),
                None,
                PendingAwaiter::Prompt(tx),
            );
            let _ = self
                .shared
                .events_tx
                .send(PortalEvent::ShowPrompt(PortalPromptRequest {
                    request_id: prompt_req_id,
                    request_handle: handle_str.clone(),
                    app_id: clean_app.clone(),
                    parent_window: sanitize_portal_text(&parent_window, "", 128),
                    kind: PortalPromptKind::Screenshot { requested_target },
                }));

            let (accepted, selection) = rx.await;
            let _ = conn
                .object_server()
                .remove::<PortalRequestIface, _>(handle_str.as_str())
                .await;
            if !accepted {
                return Ok((PORTAL_RESPONSE_CANCELLED, HashMap::new()));
            }
            match selection {
                Some(PortalSelection::Screenshot {
                    target,
                    output_id,
                    window_id,
                }) => (target, output_id, window_id),
                _ => (requested_target, None, None),
            }
        } else {
            (requested_target, None, None)
        };

        if self.shared.is_screen_locked() {
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        }

        let cap_req_id = self.shared.alloc_request_id();
        let (cap_tx, cap_rx) = AsyncSignal::channel();
        insert_pending(
            &self.shared,
            cap_req_id,
            None,
            None,
            PendingAwaiter::Screenshot(cap_tx),
        );
        let _ = self.shared.events_tx.send(PortalEvent::CaptureScreenshot {
            request_id: cap_req_id,
            app_id: clean_app,
            target: selected_target,
            output_id,
            window_id,
        });

        match cap_rx.await {
            Ok(uri) => {
                let mut res = HashMap::new();
                if let Ok(val) = OwnedValue::try_from(Value::from(uri)) {
                    res.insert("uri".to_owned(), val);
                }
                Ok((PORTAL_RESPONSE_OK, res))
            }
            Err(_) => Ok((PORTAL_RESPONSE_OTHER, HashMap::new())),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    async fn pick_color(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let _ = (&parent_window, &options);
        if self.shared.is_screen_locked() {
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        }
        let req_id = self.shared.alloc_request_id();
        let handle_str = handle.as_str().to_owned();
        register_request_object(conn, &self.shared, req_id, &handle_str).await;

        let (tx, rx) = AsyncSignal::channel();
        insert_pending(
            &self.shared,
            req_id,
            Some(handle_str.clone()),
            None,
            PendingAwaiter::PickColor(tx),
        );
        let _ = self.shared.events_tx.send(PortalEvent::PickColor {
            request_id: req_id,
            app_id: sanitize_portal_text(&app_id, "Desktop App", 64),
        });

        let outcome = rx.await;
        let _ = conn
            .object_server()
            .remove::<PortalRequestIface, _>(handle_str.as_str())
            .await;

        match outcome {
            Ok((r, g, b)) => {
                let mut res = HashMap::new();
                if let Ok(val) = OwnedValue::try_from(Value::from((r, g, b))) {
                    res.insert("color".to_owned(), val);
                }
                Ok((PORTAL_RESPONSE_OK, res))
            }
            Err(msg) if msg == "cancelled" => Ok((PORTAL_RESPONSE_CANCELLED, HashMap::new())),
            Err(_) => Ok((PORTAL_RESPONSE_OTHER, HashMap::new())),
        }
    }

    #[zbus(property)]
    fn available_targets(&self) -> u32 {
        let _ = self;
        SCREENSHOT_AVAILABLE_TARGETS
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        let _ = self;
        3
    }
}

struct ScreenCastPortalIface {
    shared: Arc<PortalSharedState>,
}

#[interface(name = "org.freedesktop.impl.portal.ScreenCast")]
impl ScreenCastPortalIface {
    #[allow(clippy::needless_pass_by_value)]
    async fn create_session(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let _ = (&handle, &options);
        let session_path = session_handle.as_str().to_owned();
        let session_iface = PortalSessionIface {
            session_handle: session_path.clone(),
            shared: Arc::clone(&self.shared),
        };
        if conn
            .object_server()
            .at(session_path.as_str(), session_iface)
            .await
            .is_err()
        {
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        }

        if let Ok(mut sessions) = self.shared.sessions.lock() {
            sessions.insert(
                session_path.clone(),
                ScreenCastSessionConfig {
                    app_id: sanitize_portal_text(&app_id, "Desktop App", 64),
                    source_types: SCREENCAST_SOURCE_MONITOR,
                    multiple: false,
                    cursor_mode: SCREENCAST_CURSOR_HIDDEN,
                    persist_mode: 0,
                    restored_target: None,
                },
            );
        }

        let session_id = session_path
            .rsplit('/')
            .next()
            .unwrap_or("rustyos_session")
            .to_owned();
        let mut results = HashMap::new();
        if let Ok(v) = OwnedValue::try_from(Value::from(session_id)) {
            results.insert("session_id".to_owned(), v);
        }
        Ok((PORTAL_RESPONSE_OK, results))
    }

    #[allow(clippy::needless_pass_by_value)]
    async fn select_sources(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let _ = (&handle, &app_id);
        let session_path = session_handle.as_str().to_owned();
        let requested_cursor = opt_u32(&options, "cursor_mode").unwrap_or(SCREENCAST_CURSOR_HIDDEN);
        if !is_valid_screencast_cursor_mode(requested_cursor) {
            // ScreenCast portal spec: unadvertised cursor_mode closes the session.
            if let Ok(mut sessions) = self.shared.sessions.lock() {
                sessions.remove(&session_path);
            }
            let _ = conn
                .object_server()
                .remove::<PortalSessionIface, _>(session_path.as_str())
                .await;
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        }

        let raw_types = opt_u32(&options, "types").unwrap_or(SCREENCAST_SOURCE_MONITOR);
        let types = (raw_types & SCREENCAST_AVAILABLE_SOURCE_TYPES).max(SCREENCAST_SOURCE_MONITOR);
        let filtered_types = if (raw_types & SCREENCAST_AVAILABLE_SOURCE_TYPES) != 0 {
            raw_types & SCREENCAST_AVAILABLE_SOURCE_TYPES
        } else {
            types
        };
        let multiple = opt_bool(&options, "multiple").unwrap_or(false);
        let persist_mode = opt_u32(&options, "persist_mode").unwrap_or(0).min(2);
        let restored = parse_restore_data(options.get("restore_data"));

        let updated = if let Ok(mut sessions) = self.shared.sessions.lock()
            && let Some(cfg) = sessions.get_mut(&session_path)
        {
            cfg.source_types = filtered_types;
            cfg.multiple = multiple;
            cfg.cursor_mode = requested_cursor;
            cfg.persist_mode = persist_mode;
            cfg.restored_target = restored;
            true
        } else {
            false
        };

        if !updated {
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        }
        Ok((PORTAL_RESPONSE_OK, HashMap::new()))
    }

    #[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
    async fn start(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<(u32, HashMap<String, OwnedValue>)> {
        let _ = &options;
        if self.shared.is_screen_locked() {
            return Ok((PORTAL_RESPONSE_CANCELLED, HashMap::new()));
        }

        let session_path = session_handle.as_str().to_owned();
        let Some(cfg) = self
            .shared
            .sessions
            .lock()
            .ok()
            .and_then(|s| s.get(&session_path).cloned())
        else {
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        };

        let clean_app = sanitize_portal_text(&app_id, &cfg.app_id, 64);
        let handle_str = handle.as_str().to_owned();
        let prompt_req_id = self.shared.alloc_request_id();
        register_request_object(conn, &self.shared, prompt_req_id, &handle_str).await;

        let (prompt_tx, prompt_rx) = AsyncSignal::channel();
        insert_pending(
            &self.shared,
            prompt_req_id,
            Some(handle_str.clone()),
            Some(session_path.clone()),
            PendingAwaiter::Prompt(prompt_tx),
        );
        let _ = self
            .shared
            .events_tx
            .send(PortalEvent::ShowPrompt(PortalPromptRequest {
                request_id: prompt_req_id,
                request_handle: handle_str.clone(),
                app_id: clean_app.clone(),
                parent_window: sanitize_portal_text(&parent_window, "", 128),
                kind: PortalPromptKind::ScreenCast {
                    session_handle: session_path.clone(),
                    allowed_types: cfg.source_types,
                    multiple: cfg.multiple,
                    cursor_mode: cfg.cursor_mode,
                    persist_mode: cfg.persist_mode,
                    restored_target: cfg.restored_target.clone(),
                },
            }));

        let (accepted, selection) = prompt_rx.await;
        let _ = conn
            .object_server()
            .remove::<PortalRequestIface, _>(handle_str.as_str())
            .await;

        if !accepted || self.shared.is_screen_locked() {
            return Ok((PORTAL_RESPONSE_CANCELLED, HashMap::new()));
        }

        let Some(selection) = selection else {
            return Ok((PORTAL_RESPONSE_CANCELLED, HashMap::new()));
        };

        let effective_persist = match &selection {
            PortalSelection::ScreenCast { persist_mode, .. } => *persist_mode,
            PortalSelection::Screenshot { .. } => cfg.persist_mode,
        };

        let stream_req_id = self.shared.alloc_request_id();
        let (stream_tx, stream_rx) = AsyncSignal::channel();
        insert_pending(
            &self.shared,
            stream_req_id,
            None,
            Some(session_path.clone()),
            PendingAwaiter::ScreenCast(stream_tx),
        );
        let _ = self.shared.events_tx.send(PortalEvent::StartScreenCast {
            request_id: stream_req_id,
            session_handle: session_path.clone(),
            app_id: clean_app,
            selection,
        });

        let Ok(streams) = stream_rx.await else {
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        };
        if streams.is_empty() {
            return Ok((PORTAL_RESPONSE_OTHER, HashMap::new()));
        }

        let encoded_streams = encode_screencast_streams_value(&streams);
        let mut results = HashMap::new();
        if let Ok(v) = OwnedValue::try_from(Value::from(encoded_streams)) {
            results.insert("streams".to_owned(), v);
        }
        if let Ok(v) = OwnedValue::try_from(Value::from(effective_persist)) {
            results.insert("persist_mode".to_owned(), v);
        }
        if effective_persist > 0
            && let Some(first) = streams.first()
            && let Ok(token) = OwnedValue::try_from(Value::from(first.mapping_id.clone()))
            && let Ok(restore_val) =
                OwnedValue::try_from(Value::from(("RustyOS".to_owned(), 1u32, token)))
        {
            results.insert("restore_data".to_owned(), restore_val);
        }

        Ok((PORTAL_RESPONSE_OK, results))
    }

    #[zbus(property)]
    fn available_source_types(&self) -> u32 {
        let _ = self;
        SCREENCAST_AVAILABLE_SOURCE_TYPES
    }

    #[zbus(property)]
    fn available_cursor_modes(&self) -> u32 {
        let _ = self;
        SCREENCAST_AVAILABLE_CURSOR_MODES
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        let _ = self;
        6
    }
}

async fn register_request_object(
    conn: &zbus::Connection,
    shared: &Arc<PortalSharedState>,
    request_id: u64,
    handle_path: &str,
) {
    let iface = PortalRequestIface {
        request_id,
        shared: Arc::clone(shared),
    };
    let _ = conn.object_server().at(handle_path, iface).await;
}

fn insert_pending(
    shared: &Arc<PortalSharedState>,
    request_id: u64,
    handle_path: Option<String>,
    session_handle: Option<String>,
    awaiter: PendingAwaiter,
) {
    if let Ok(mut pending) = shared.pending.lock() {
        pending.insert(
            request_id,
            PendingEntry {
                handle_path,
                session_handle,
                awaiter,
            },
        );
    }
}

fn is_valid_screencast_cursor_mode(cursor_mode: u32) -> bool {
    (cursor_mode & SCREENCAST_AVAILABLE_CURSOR_MODES) != 0 && cursor_mode.is_power_of_two()
}

fn opt_bool(options: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    options.get(key).and_then(|v| bool::try_from(&**v).ok())
}

fn opt_u32(options: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    options.get(key).and_then(|v| u32::try_from(&**v).ok())
}

fn parse_restore_data(raw: Option<&OwnedValue>) -> Option<String> {
    let value = raw?;
    let structure = zbus::zvariant::Structure::try_from(&**value).ok()?;
    let fields = structure.fields();
    if fields.len() != 3 {
        return None;
    }
    let vendor = <&str>::try_from(&fields[0]).ok()?;
    let version = u32::try_from(&fields[1]).ok()?;
    if vendor != "RustyOS" || version != 1 {
        return None;
    }
    let token = <&str>::try_from(&fields[2]).ok()?;
    Some(token.to_owned())
}

/// Thread-safe handle for replying to asynchronous portal requests off-thread.
#[derive(Clone)]
pub struct PortalReplySender {
    shared: Arc<PortalSharedState>,
}

impl fmt::Debug for PortalReplySender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortalReplySender").finish_non_exhaustive()
    }
}

impl PortalReplySender {
    /// Resolves a pending request initiated by the portal D-Bus worker.
    pub fn send_reply(&self, reply: PortalReply) {
        self.shared.deliver_reply(reply);
    }

    /// Returns a cloneable sender for resolving portal replies from background worker threads.
    #[must_use]
    pub fn reply_sender(&self) -> PortalReplySender {
        PortalReplySender {
            shared: Arc::clone(&self.shared),
        }
    }
}

/// Non-blocking compositor handle owning `org.freedesktop.impl.portal.desktop.rustyos`.
pub struct PortalHandle {
    events_rx: Mutex<Receiver<PortalEvent>>,
    shared: Arc<PortalSharedState>,
    stop_flag: Arc<AtomicBool>,
    conn_slot: Arc<Mutex<Option<zbus::Connection>>>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for PortalHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortalHandle")
            .field("active", &self.worker.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for PortalHandle {
    fn default() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let shared = Arc::new(PortalSharedState {
            snapshot: Mutex::new(PortalDesktopSnapshot::default()),
            sessions: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            next_request_id: AtomicU64::new(1),
            events_tx,
            replies_tap: Mutex::new(None),
        });
        Self {
            events_rx: Mutex::new(events_rx),
            shared,
            stop_flag: Arc::new(AtomicBool::new(true)),
            conn_slot: Arc::new(Mutex::new(None)),
            worker: None,
        }
    }
}

impl Drop for PortalHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        let ids: Vec<u64> = self
            .shared
            .pending
            .lock()
            .map(|p| p.keys().copied().collect())
            .unwrap_or_default();
        for id in ids {
            self.shared.cancel_request(id);
        }
        if let Ok(mut guard) = self.conn_slot.lock()
            && let Some(conn) = guard.take()
        {
            drop(conn);
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

impl PortalHandle {
    /// Starts the non-blocking Session D-Bus portal backend thread.
    ///
    /// Gracefully stays inactive if Session D-Bus is unavailable in test or minimal environments.
    #[must_use]
    pub fn start_background() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let shared = Arc::new(PortalSharedState {
            snapshot: Mutex::new(PortalDesktopSnapshot::default()),
            sessions: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            next_request_id: AtomicU64::new(1),
            events_tx,
            replies_tap: Mutex::new(None),
        });
        let stop_flag = Arc::new(AtomicBool::new(false));
        let conn_slot = Arc::new(Mutex::new(None));

        let worker_shared = Arc::clone(&shared);
        let worker_stop = Arc::clone(&stop_flag);
        let worker_conn = Arc::clone(&conn_slot);
        let worker = thread::Builder::new()
            .name("rusty-portal".to_owned())
            .spawn(move || {
                run_portal_worker(&worker_shared, &worker_conn, &worker_stop);
            })
            .ok();

        Self {
            events_rx: Mutex::new(events_rx),
            shared,
            stop_flag,
            conn_slot,
            worker,
        }
    }

    /// Creates a deterministic offline handle wired to custom event/reply test channels.
    #[must_use]
    pub fn with_test_channels(
        events_tx: Sender<PortalEvent>,
        events_rx: Receiver<PortalEvent>,
    ) -> Self {
        Self::with_test_channels_and_replies(events_tx, events_rx, None)
    }

    /// Creates a deterministic offline test handle that mirrors outgoing replies to `replies_tx`.
    #[must_use]
    pub fn with_test_channels_and_replies(
        events_tx: Sender<PortalEvent>,
        events_rx: Receiver<PortalEvent>,
        replies_tx: Option<Sender<PortalReply>>,
    ) -> Self {
        let shared = Arc::new(PortalSharedState {
            snapshot: Mutex::new(PortalDesktopSnapshot::default()),
            sessions: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            next_request_id: AtomicU64::new(1),
            events_tx,
            replies_tap: Mutex::new(replies_tx),
        });
        Self {
            events_rx: Mutex::new(events_rx),
            shared,
            stop_flag: Arc::new(AtomicBool::new(false)),
            conn_slot: Arc::new(Mutex::new(None)),
            worker: None,
        }
    }

    /// Updates the desktop snapshot (outputs, windows, lock status) read by incoming D-Bus calls.
    pub fn update_snapshot(&self, snapshot: PortalDesktopSnapshot) {
        if let Ok(mut guard) = self.shared.snapshot.lock() {
            *guard = snapshot;
        }
    }

    /// Returns a copy of the last desktop snapshot synchronized into the portal backend.
    #[must_use]
    pub fn snapshot(&self) -> PortalDesktopSnapshot {
        self.shared
            .snapshot
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Non-blockingly drains pending portal events into the compositor event loop.
    #[must_use]
    pub fn drain_events(&self) -> Vec<PortalEvent> {
        let Ok(guard) = self.events_rx.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(ev) = guard.try_recv() {
            out.push(ev);
        }
        out
    }

    /// Resolves a pending request initiated by the portal D-Bus worker.
    pub fn send_reply(&self, reply: PortalReply) {
        self.shared.deliver_reply(reply);
    }

    /// Returns a cloneable sender for resolving portal replies from background worker threads.
    #[must_use]
    pub fn reply_sender(&self) -> PortalReplySender {
        PortalReplySender {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Emits `org.freedesktop.impl.portal.Session.Closed` and forgets an invalidated session.
    pub fn notify_session_closed(&self, session_handle: &str) {
        if let Ok(mut sessions) = self.shared.sessions.lock() {
            sessions.remove(session_handle);
        }
        self.shared.cancel_session_pending(session_handle);
        let conn = self.conn_slot.lock().ok().and_then(|g| g.clone());
        let Some(conn) = conn else {
            return;
        };
        let path = session_handle.to_owned();
        zbus::block_on(async move {
            if let Ok(iface_ref) = conn
                .object_server()
                .interface::<_, PortalSessionIface>(path.as_str())
                .await
            {
                let _ = PortalSessionIface::closed(iface_ref.signal_emitter()).await;
            }
            let _ = conn
                .object_server()
                .remove::<PortalSessionIface, _>(path.as_str())
                .await;
        });
    }
}

fn run_portal_worker(
    shared: &Arc<PortalSharedState>,
    conn_slot: &Arc<Mutex<Option<zbus::Connection>>>,
    stop_flag: &Arc<AtomicBool>,
) {
    let Ok(conn) = zbus::block_on(async {
        let screenshot_iface = ScreenshotPortalIface {
            shared: Arc::clone(shared),
        };
        let screencast_iface = ScreenCastPortalIface {
            shared: Arc::clone(shared),
        };
        zbus::connection::Builder::session()?
            .serve_at(PORTAL_OBJECT_PATH, screenshot_iface)?
            .serve_at(PORTAL_OBJECT_PATH, screencast_iface)?
            .name(PORTAL_DBUS_NAME)?
            .build()
            .await
    }) else {
        return;
    };

    if let Ok(mut slot) = conn_slot.lock() {
        *slot = Some(conn.clone());
    }

    while !stop_flag.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(50));
    }

    if let Ok(mut slot) = conn_slot.lock() {
        *slot = None;
    }
    drop(conn);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_rpw1_frame_header_little_endian() {
        let hdr = encode_pipewire_frame_header(1920, 1080, 7680);
        assert_eq!(&hdr[0..4], &PIPEWIRE_FRAME_MAGIC.to_le_bytes());
        assert_eq!(u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]), 1920);
        assert_eq!(u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]), 1080);
        assert_eq!(
            u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]),
            7680
        );
        assert_eq!(u32::from_le_bytes([hdr[16], hdr[17], hdr[18], hdr[19]]), 0);
    }

    #[test]
    fn parses_worker_status_lines() {
        assert_eq!(
            parse_worker_line("READY 90 8757\n"),
            Some(WorkerStatusLine::Ready {
                node_id: 90,
                serial: 8757,
            })
        );
        assert_eq!(
            parse_worker_line("STATE streaming"),
            Some(WorkerStatusLine::StateStreaming)
        );
        assert_eq!(
            parse_worker_line("STATE paused\n"),
            Some(WorkerStatusLine::StatePaused)
        );
        assert_eq!(
            parse_worker_line("ERROR pipewire daemon unavailable"),
            Some(WorkerStatusLine::Error(
                "pipewire daemon unavailable".to_owned()
            ))
        );
        assert_eq!(parse_worker_line("UNKNOWN"), None);
    }

    #[test]
    fn converts_paths_to_percent_encoded_file_uris() {
        assert_eq!(
            path_to_file_uri(Path::new("/home/anc/Pictures/Screenshots/My Shot #1.png")),
            "file:///home/anc/Pictures/Screenshots/My%20Shot%20%231.png"
        );
    }

    #[test]
    fn converts_rgba_pixel_to_unit_rgb_tuple() {
        assert_eq!(
            rgba_pixel_to_portal_color([0, 255, 128, 255]),
            (0.0, 1.0, 128.0 / 255.0)
        );
    }

    #[test]
    fn encodes_screencast_streams_to_dbus_variant_value() -> Result<(), Box<dyn std::error::Error>>
    {
        let streams = vec![PortalStreamDescriptor {
            node_id: 101,
            pipewire_serial: 9901,
            x: 10,
            y: 20,
            width: 1920,
            height: 1080,
            source_type: SCREENCAST_SOURCE_MONITOR,
            mapping_id: "output:eDP-1".to_owned(),
        }];
        let encoded = encode_screencast_streams_value(&streams);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].0, 101);
        let props = &encoded[0].1;
        assert_eq!(
            u32::try_from(&**props.get("source_type").ok_or("missing")?)?,
            1
        );
        assert_eq!(
            u64::try_from(&**props.get("pipewire-serial").ok_or("missing")?)?,
            9901
        );
        assert_eq!(
            <&str>::try_from(&**props.get("mapping_id").ok_or("missing")?)?,
            "output:eDP-1"
        );
        let _wrapped = OwnedValue::try_from(Value::from(encoded))?;
        Ok(())
    }

    #[test]
    fn resolves_and_streams_real_pipewire_worker_when_available() {
        let Ok(helper) = resolve_pipewire_helper_path() else {
            return;
        };
        let cfg = PipeWireStreamConfig {
            width: 64,
            height: 64,
            fps: 30,
            app_name: "rusty-services-test".to_owned(),
            node_description: "RustyOS Unit Test Stream".to_owned(),
        };
        let Ok(stream) = PipeWireStreamHandle::spawn_with_binary(&helper, &cfg) else {
            // PipeWire user daemon may be absent inside isolated build sandboxes.
            return;
        };
        assert!(stream.node_id() > 0);
        assert!(stream.pipewire_serial() > 0);
        assert!(stream.push_frame(64, 64, vec![200u8; 64 * 64 * 4]));
        assert!(!stream.push_frame(64, 64, vec![0u8; 16]));
    }

    #[test]
    fn validates_screencast_cursor_modes_and_restore_data() -> Result<(), Box<dyn std::error::Error>>
    {
        assert!(is_valid_screencast_cursor_mode(SCREENCAST_CURSOR_HIDDEN));
        assert!(is_valid_screencast_cursor_mode(SCREENCAST_CURSOR_EMBEDDED));
        assert!(!is_valid_screencast_cursor_mode(0));
        assert!(!is_valid_screencast_cursor_mode(
            SCREENCAST_CURSOR_HIDDEN | SCREENCAST_CURSOR_EMBEDDED
        ));
        assert!(!is_valid_screencast_cursor_mode(4));

        let valid = OwnedValue::try_from(Value::from(("RustyOS", 1u32, "output:eDP-1")))?;
        assert_eq!(
            parse_restore_data(Some(&valid)).as_deref(),
            Some("output:eDP-1")
        );

        let wrong_vendor = OwnedValue::try_from(Value::from(("Other", 1u32, "output:eDP-1")))?;
        assert_eq!(parse_restore_data(Some(&wrong_vendor)), None);

        let wrong_ver = OwnedValue::try_from(Value::from(("RustyOS", 2u32, "output:eDP-1")))?;
        assert_eq!(parse_restore_data(Some(&wrong_ver)), None);
        Ok(())
    }
}
