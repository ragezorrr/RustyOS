//! `RustyOS` versioned local IPC protocol (`RUSTYOS-009`, ADR-0005).
//!
//! Provides a bounded, zero-dependency, line-delimited protocol over Unix Domain Sockets (`0600`)
//! connecting `rusty-compositor` and `rusty-shell` with explicit version negotiation (`v1`)
//! and strict input validation (`SECURITY.md`).

use std::env;
use std::error::Error;
use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// Canonical wire protocol version supported by this crate.
pub const IPC_PROTOCOL_VERSION: u16 = 1;

/// Maximum permitted wire frame length in bytes (`8 KiB`) to prevent OOM/DoS via untrusted input.
pub const MAX_IPC_MESSAGE_BYTES: usize = 8192;

/// Notification urgency level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Urgency {
    /// Low priority background notification.
    Low,
    /// Standard desktop notification.
    #[default]
    Normal,
    /// Critical alert requiring user attention.
    Critical,
}

impl Urgency {
    /// Returns the wire identifier for this urgency.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::Critical => "critical",
        }
    }

    /// Parses an [`Urgency`] from its wire representation.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "low" => Some(Self::Low),
            "normal" => Some(Self::Normal),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

/// Summary of a single mapped window exposed to `rusty-shell`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSummary {
    /// Unique window ID in `rusty-wm`.
    pub id: u64,
    /// Workspace index (`1..=9`).
    pub workspace: u8,
    /// Whether this window currently holds keyboard focus.
    pub focused: bool,
    /// Whether this window is maximized.
    pub maximized: bool,
}

/// Complete snapshot of compositor state sent to `rusty-shell` for `TopBar`, `Dock`, `Overview`, and `QuickSettings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopStateSnapshot {
    /// Active workspace index (`1..=9`).
    pub active_workspace: u8,
    /// Active workspace layout mode (`"floating"` or `"master-stack"`).
    pub layout_mode: String,
    /// Whether accessibility `reduced_motion` is enabled.
    pub reduced_motion: bool,
    /// Whether workspace overview mode is active.
    pub overview_active: bool,
    /// Whether the session lock screen is active.
    pub screen_locked: bool,
    /// Number of unread/active notifications held by the compositor/shell bridge.
    pub notification_count: u32,
    /// Median frame interval in microseconds (`p50`).
    pub frame_interval_p50_us: u64,
    /// 95th percentile input-to-present latency in microseconds (`p95`).
    pub input_latency_p95_us: u64,
    /// Currently managed windows across workspaces.
    pub windows: Vec<WindowSummary>,
}

impl Default for DesktopStateSnapshot {
    fn default() -> Self {
        Self {
            active_workspace: 1,
            layout_mode: "floating".to_owned(),
            reduced_motion: false,
            overview_active: false,
            screen_locked: false,
            notification_count: 0,
            frame_interval_p50_us: 16_667,
            input_latency_p95_us: 0,
            windows: Vec::new(),
        }
    }
}

/// Requests sent from `rusty-shell` (client) to `rusty-compositor` (server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellRequest {
    /// Negotiates protocol version and identifies the shell component.
    Handshake {
        /// Client protocol version (`IPC_PROTOCOL_VERSION`).
        protocol_version: u16,
        /// Client identifier (e.g. `"rusty-shell"`).
        client_name: String,
    },
    /// Requests an immediate [`DesktopStateSnapshot`].
    GetStateSnapshot,
    /// Switches the active workspace (`1..=9`).
    SwitchWorkspace {
        /// Target workspace index (`1..=9`).
        workspace: u8,
    },
    /// Focuses and raises a specific window by ID.
    FocusWindow {
        /// Target window ID.
        window_id: u64,
    },
    /// Toggles the active workspace layout mode (`Floating <-> MasterStack`).
    ToggleLayoutMode,
    /// Toggles workspace overview mode.
    ToggleOverview,
    /// Enables or disables `reduced_motion` mode.
    SetReducedMotion {
        /// New `reduced_motion` state.
        enabled: bool,
    },
    /// Locks or unlocks the desktop session screen.
    SetLockScreen {
        /// Whether the session is locked.
        locked: bool,
    },
    /// Pushes a desktop notification through the shell/compositor bus.
    Notify {
        /// Notification urgency.
        urgency: Urgency,
        /// Short notification title (sanitized, no newlines/pipes).
        title: String,
        /// Notification body text.
        body: String,
    },
}

/// Events/responses sent from `rusty-compositor` (server) to `rusty-shell` (client).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositorEvent {
    /// Confirms protocol version handshake.
    HandshakeAck {
        /// Negotiated protocol version.
        protocol_version: u16,
        /// Compositor crate version string.
        server_version: String,
    },
    /// Delivers the current [`DesktopStateSnapshot`].
    StateSnapshot(DesktopStateSnapshot),
    /// Reports a protocol or validation error without dropping the compositor.
    Error {
        /// Machine-readable error code.
        code: String,
        /// Human-readable description.
        message: String,
    },
}

/// Errors returned by IPC framing, validation, or socket transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcError {
    /// Wire message exceeded [`MAX_IPC_MESSAGE_BYTES`].
    MessageTooLarge(usize),
    /// Wire message uses an incompatible protocol version.
    UnsupportedVersion {
        /// Received version.
        received: u16,
        /// Expected version.
        expected: u16,
    },
    /// Malformed wire syntax or invalid field value.
    InvalidFrame(String),
    /// Underlying socket I/O error.
    Io(String),
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageTooLarge(len) => write!(
                f,
                "IPC frame size {len} exceeds maximum {MAX_IPC_MESSAGE_BYTES} bytes"
            ),
            Self::UnsupportedVersion { received, expected } => write!(
                f,
                "unsupported IPC protocol version {received} (expected {expected})"
            ),
            Self::InvalidFrame(msg) => write!(f, "invalid IPC frame: {msg}"),
            Self::Io(msg) => write!(f, "IPC I/O error: {msg}"),
        }
    }
}

impl Error for IpcError {}

impl ShellRequest {
    /// Encodes the request into a single line-delimited wire frame (`v1|<cmd>|...`).
    #[must_use]
    pub fn encode_line(&self) -> String {
        let mut out = String::with_capacity(128);
        let _ = write!(out, "v{IPC_PROTOCOL_VERSION}|");
        match self {
            Self::Handshake {
                protocol_version,
                client_name,
            } => {
                let clean_name = sanitize_token(client_name);
                let _ = writeln!(out, "HANDSHAKE|{protocol_version}|{clean_name}");
            }
            Self::GetStateSnapshot => {
                let _ = writeln!(out, "GET_SNAPSHOT");
            }
            Self::SwitchWorkspace { workspace } => {
                let _ = writeln!(out, "SWITCH_WORKSPACE|{workspace}");
            }
            Self::FocusWindow { window_id } => {
                let _ = writeln!(out, "FOCUS_WINDOW|{window_id}");
            }
            Self::ToggleLayoutMode => {
                let _ = writeln!(out, "TOGGLE_LAYOUT");
            }
            Self::ToggleOverview => {
                let _ = writeln!(out, "TOGGLE_OVERVIEW");
            }
            Self::SetReducedMotion { enabled } => {
                let flag = u8::from(*enabled);
                let _ = writeln!(out, "SET_REDUCED_MOTION|{flag}");
            }
            Self::SetLockScreen { locked } => {
                let flag = u8::from(*locked);
                let _ = writeln!(out, "SET_LOCK_SCREEN|{flag}");
            }
            Self::Notify {
                urgency,
                title,
                body,
            } => {
                let clean_title = sanitize_token(title);
                let clean_body = sanitize_token(body);
                let _ = writeln!(
                    out,
                    "NOTIFY|{}|{clean_title}|{clean_body}",
                    urgency.as_str()
                );
            }
        }
        out
    }

    /// Decodes and validates a [`ShellRequest`] from a single wire line.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] if the frame exceeds [`MAX_IPC_MESSAGE_BYTES`], has an unsupported
    /// protocol version, or contains malformed arguments.
    pub fn decode_line(line: &str) -> Result<Self, IpcError> {
        if line.len() > MAX_IPC_MESSAGE_BYTES {
            return Err(IpcError::MessageTooLarge(line.len()));
        }
        let trimmed = line.trim();
        let mut parts = trimmed.split('|');
        let ver_tag = parts
            .next()
            .ok_or_else(|| IpcError::InvalidFrame("missing version tag".to_owned()))?;
        let ver = parse_version_tag(ver_tag)?;
        if ver != IPC_PROTOCOL_VERSION {
            return Err(IpcError::UnsupportedVersion {
                received: ver,
                expected: IPC_PROTOCOL_VERSION,
            });
        }

        let cmd = parts
            .next()
            .ok_or_else(|| IpcError::InvalidFrame("missing command tag".to_owned()))?;

        match cmd {
            "HANDSHAKE" => {
                let proto_str = parts.next().ok_or_else(|| {
                    IpcError::InvalidFrame("missing handshake version".to_owned())
                })?;
                let protocol_version = proto_str.parse::<u16>().map_err(|_| {
                    IpcError::InvalidFrame(format!("invalid handshake version '{proto_str}'"))
                })?;
                if protocol_version != IPC_PROTOCOL_VERSION {
                    return Err(IpcError::UnsupportedVersion {
                        received: protocol_version,
                        expected: IPC_PROTOCOL_VERSION,
                    });
                }
                let client_name = parts
                    .next()
                    .ok_or_else(|| IpcError::InvalidFrame("missing client_name".to_owned()))?;
                if client_name.is_empty() {
                    return Err(IpcError::InvalidFrame(
                        "client_name cannot be empty".to_owned(),
                    ));
                }
                Ok(Self::Handshake {
                    protocol_version,
                    client_name: client_name.to_owned(),
                })
            }
            "GET_SNAPSHOT" => Ok(Self::GetStateSnapshot),
            "SWITCH_WORKSPACE" => {
                let ws_str = parts
                    .next()
                    .ok_or_else(|| IpcError::InvalidFrame("missing workspace index".to_owned()))?;
                let workspace = ws_str.parse::<u8>().map_err(|_| {
                    IpcError::InvalidFrame(format!("invalid workspace index '{ws_str}'"))
                })?;
                if !(1..=9).contains(&workspace) {
                    return Err(IpcError::InvalidFrame(format!(
                        "workspace index {workspace} out of range 1..=9"
                    )));
                }
                Ok(Self::SwitchWorkspace { workspace })
            }
            "FOCUS_WINDOW" => {
                let id_str = parts
                    .next()
                    .ok_or_else(|| IpcError::InvalidFrame("missing window_id".to_owned()))?;
                let window_id = id_str
                    .parse::<u64>()
                    .map_err(|_| IpcError::InvalidFrame(format!("invalid window_id '{id_str}'")))?;
                Ok(Self::FocusWindow { window_id })
            }
            "TOGGLE_LAYOUT" => Ok(Self::ToggleLayoutMode),
            "TOGGLE_OVERVIEW" => Ok(Self::ToggleOverview),
            "SET_REDUCED_MOTION" => {
                let flag = parse_bool_flag(parts.next(), "SET_REDUCED_MOTION")?;
                Ok(Self::SetReducedMotion { enabled: flag })
            }
            "SET_LOCK_SCREEN" => {
                let flag = parse_bool_flag(parts.next(), "SET_LOCK_SCREEN")?;
                Ok(Self::SetLockScreen { locked: flag })
            }
            "NOTIFY" => {
                let urg_str = parts
                    .next()
                    .ok_or_else(|| IpcError::InvalidFrame("missing urgency".to_owned()))?;
                let urgency = Urgency::parse(urg_str).ok_or_else(|| {
                    IpcError::InvalidFrame(format!("unknown urgency '{urg_str}'"))
                })?;
                let title = parts.next().ok_or_else(|| {
                    IpcError::InvalidFrame("missing notification title".to_owned())
                })?;
                let body = parts.next().unwrap_or("");
                Ok(Self::Notify {
                    urgency,
                    title: title.to_owned(),
                    body: body.to_owned(),
                })
            }
            other => Err(IpcError::InvalidFrame(format!(
                "unknown request command '{other}'"
            ))),
        }
    }
}

impl CompositorEvent {
    /// Encodes the event into a single line-delimited wire frame (`v1|<event>|...`).
    #[must_use]
    pub fn encode_line(&self) -> String {
        let mut out = String::with_capacity(256);
        let _ = write!(out, "v{IPC_PROTOCOL_VERSION}|");
        match self {
            Self::HandshakeAck {
                protocol_version,
                server_version,
            } => {
                let clean_ver = sanitize_token(server_version);
                let _ = writeln!(out, "HANDSHAKE_ACK|{protocol_version}|{clean_ver}");
            }
            Self::StateSnapshot(snap) => {
                let mut wins_buf = String::new();
                for (idx, w) in snap.windows.iter().enumerate() {
                    if idx > 0 {
                        wins_buf.push(',');
                    }
                    let _ = write!(
                        wins_buf,
                        "{}:{}:{}:{}",
                        w.id,
                        w.workspace,
                        u8::from(w.focused),
                        u8::from(w.maximized)
                    );
                }
                if wins_buf.is_empty() {
                    wins_buf.push('-');
                }
                let _ = writeln!(
                    out,
                    "SNAPSHOT|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                    snap.active_workspace,
                    sanitize_token(&snap.layout_mode),
                    u8::from(snap.reduced_motion),
                    u8::from(snap.overview_active),
                    u8::from(snap.screen_locked),
                    snap.notification_count,
                    snap.frame_interval_p50_us,
                    snap.input_latency_p95_us,
                    wins_buf
                );
            }
            Self::Error { code, message } => {
                let _ = writeln!(
                    out,
                    "ERROR|{}|{}",
                    sanitize_token(code),
                    sanitize_token(message)
                );
            }
        }
        out
    }

    /// Decodes a [`CompositorEvent`] from a single wire line.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] if the frame is oversized, uses an unsupported version, or is malformed.
    pub fn decode_line(line: &str) -> Result<Self, IpcError> {
        if line.len() > MAX_IPC_MESSAGE_BYTES {
            return Err(IpcError::MessageTooLarge(line.len()));
        }
        let trimmed = line.trim();
        let mut parts = trimmed.split('|');
        let ver_tag = parts
            .next()
            .ok_or_else(|| IpcError::InvalidFrame("missing version tag".to_owned()))?;
        let ver = parse_version_tag(ver_tag)?;
        if ver != IPC_PROTOCOL_VERSION {
            return Err(IpcError::UnsupportedVersion {
                received: ver,
                expected: IPC_PROTOCOL_VERSION,
            });
        }

        let evt = parts
            .next()
            .ok_or_else(|| IpcError::InvalidFrame("missing event tag".to_owned()))?;

        match evt {
            "HANDSHAKE_ACK" => {
                let proto_str = parts.next().ok_or_else(|| {
                    IpcError::InvalidFrame("missing ack protocol_version".to_owned())
                })?;
                let protocol_version = proto_str.parse::<u16>().map_err(|_| {
                    IpcError::InvalidFrame("invalid ack protocol_version".to_owned())
                })?;
                let server_version = parts.next().unwrap_or("0.1.0").to_owned();
                Ok(Self::HandshakeAck {
                    protocol_version,
                    server_version,
                })
            }
            "SNAPSHOT" => decode_snapshot_fields(&mut parts),
            "ERROR" => {
                let code = parts.next().unwrap_or("ERR").to_owned();
                let message = parts.next().unwrap_or("").to_owned();
                Ok(Self::Error { code, message })
            }
            other => Err(IpcError::InvalidFrame(format!(
                "unknown event tag '{other}'"
            ))),
        }
    }
}

fn decode_snapshot_fields<'a, I>(parts: &mut I) -> Result<CompositorEvent, IpcError>
where
    I: Iterator<Item = &'a str>,
{
    let active_workspace = parts
        .next()
        .ok_or_else(|| IpcError::InvalidFrame("missing active_workspace".to_owned()))?
        .parse::<u8>()
        .map_err(|_| IpcError::InvalidFrame("invalid active_workspace".to_owned()))?;
    let layout_mode = parts
        .next()
        .ok_or_else(|| IpcError::InvalidFrame("missing layout_mode".to_owned()))?
        .to_owned();
    let reduced_motion = parse_bool_flag(parts.next(), "reduced_motion")?;
    let overview_active = parse_bool_flag(parts.next(), "overview_active")?;
    let screen_locked = parse_bool_flag(parts.next(), "screen_locked")?;
    let notification_count = parts
        .next()
        .ok_or_else(|| IpcError::InvalidFrame("missing notification_count".to_owned()))?
        .parse::<u32>()
        .map_err(|_| IpcError::InvalidFrame("invalid notification_count".to_owned()))?;
    let frame_interval_p50_us = parts
        .next()
        .ok_or_else(|| IpcError::InvalidFrame("missing frame_interval_p50_us".to_owned()))?
        .parse::<u64>()
        .map_err(|_| IpcError::InvalidFrame("invalid frame_interval_p50_us".to_owned()))?;
    let input_latency_p95_us = parts
        .next()
        .ok_or_else(|| IpcError::InvalidFrame("missing input_latency_p95_us".to_owned()))?
        .parse::<u64>()
        .map_err(|_| IpcError::InvalidFrame("invalid input_latency_p95_us".to_owned()))?;
    let wins_raw = parts.next().unwrap_or("-");

    let mut windows = Vec::new();
    if wins_raw != "-" && !wins_raw.is_empty() {
        for item in wins_raw.split(',') {
            let mut wparts = item.split(':');
            let id = wparts
                .next()
                .ok_or_else(|| IpcError::InvalidFrame("missing window id".to_owned()))?
                .parse::<u64>()
                .map_err(|_| IpcError::InvalidFrame("invalid window id".to_owned()))?;
            let workspace = wparts
                .next()
                .ok_or_else(|| IpcError::InvalidFrame("missing window workspace".to_owned()))?
                .parse::<u8>()
                .map_err(|_| IpcError::InvalidFrame("invalid window workspace".to_owned()))?;
            let focused = parse_bool_flag(wparts.next(), "window.focused")?;
            let maximized = parse_bool_flag(wparts.next(), "window.maximized")?;
            windows.push(WindowSummary {
                id,
                workspace,
                focused,
                maximized,
            });
        }
    }

    Ok(CompositorEvent::StateSnapshot(DesktopStateSnapshot {
        active_workspace,
        layout_mode,
        reduced_motion,
        overview_active,
        screen_locked,
        notification_count,
        frame_interval_p50_us,
        input_latency_p95_us,
        windows,
    }))
}

/// Computes the default IPC socket path for a given Wayland socket name (e.g. `wayland-2`).
#[must_use]
pub fn default_ipc_socket_path(wayland_socket_name: &str) -> PathBuf {
    let base_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_owned());
    PathBuf::from(base_dir).join(format!("rustyos-ipc-{wayland_socket_name}.sock"))
}

/// Non-blocking Unix Domain Socket IPC server hosted by `rusty-compositor`.
#[derive(Debug)]
pub struct IpcServer {
    listener: UnixListener,
    socket_path: PathBuf,
    clients: Vec<UnixStream>,
}

impl IpcServer {
    /// Binds a new non-blocking IPC socket at `socket_path` with strict `0600` owner-only permissions.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::Io`] if the socket cannot be bound or its permissions cannot be set.
    pub fn bind(socket_path: PathBuf) -> Result<Self, IpcError> {
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }
        let listener =
            UnixListener::bind(&socket_path).map_err(|err| IpcError::Io(err.to_string()))?;
        listener
            .set_nonblocking(true)
            .map_err(|err| IpcError::Io(err.to_string()))?;

        // Enforce 0600 permissions so only the current user can connect (SECURITY.md).
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&socket_path, perms).map_err(|err| IpcError::Io(err.to_string()))?;

        Ok(Self {
            listener,
            socket_path,
            clients: Vec::new(),
        })
    }

    /// Returns the filesystem path of the bound Unix socket.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Polls for new connections and pending client requests without blocking the compositor event loop.
    ///
    /// Disconnected or crashed clients are cleaned up automatically without affecting the server.
    pub fn poll_requests<F>(&mut self, mut handler: F)
    where
        F: FnMut(ShellRequest) -> CompositorEvent,
    {
        while let Ok((stream, _addr)) = self.listener.accept() {
            if stream.set_nonblocking(true).is_ok() {
                self.clients.push(stream);
            }
        }

        let mut alive_clients = Vec::with_capacity(self.clients.len());
        for mut stream in self.clients.drain(..) {
            if let Ok((lines, still_connected)) = read_nonblocking_lines(&mut stream) {
                let mut write_ok = true;
                for line in lines {
                    let response = match ShellRequest::decode_line(&line) {
                        Ok(req) => handler(req),
                        Err(err) => CompositorEvent::Error {
                            code: "INVALID_REQUEST".to_owned(),
                            message: err.to_string(),
                        },
                    };
                    let encoded = response.encode_line();
                    if stream.write_all(encoded.as_bytes()).is_err() || stream.flush().is_err() {
                        write_ok = false;
                        break;
                    }
                }
                if still_connected && write_ok {
                    alive_clients.push(stream);
                }
            }
        }
        self.clients = alive_clients;
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
    }
}

/// Synchronous IPC client used by `rusty-shell` to exchange messages with `rusty-compositor`.
#[derive(Debug)]
pub struct IpcClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl IpcClient {
    /// Connects to the compositor IPC socket at `socket_path` and performs the `v1` handshake.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] if the connection or handshake fails.
    pub fn connect(socket_path: &Path, client_name: &str) -> Result<Self, IpcError> {
        let writer =
            UnixStream::connect(socket_path).map_err(|err| IpcError::Io(err.to_string()))?;
        let reader_stream = writer
            .try_clone()
            .map_err(|err| IpcError::Io(err.to_string()))?;
        let mut client = Self {
            reader: BufReader::new(reader_stream),
            writer,
        };

        let ack = client.send_request(&ShellRequest::Handshake {
            protocol_version: IPC_PROTOCOL_VERSION,
            client_name: client_name.to_owned(),
        })?;

        match ack {
            CompositorEvent::HandshakeAck {
                protocol_version, ..
            } if protocol_version == IPC_PROTOCOL_VERSION => Ok(client),
            CompositorEvent::Error { code, message } => {
                Err(IpcError::InvalidFrame(format!("{code}: {message}")))
            }
            other => Err(IpcError::InvalidFrame(format!(
                "unexpected handshake response: {other:?}"
            ))),
        }
    }

    /// Sends a [`ShellRequest`] and waits for the corresponding [`CompositorEvent`].
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] if socket I/O or response decoding fails.
    pub fn send_request(&mut self, request: &ShellRequest) -> Result<CompositorEvent, IpcError> {
        let encoded = request.encode_line();
        self.writer
            .write_all(encoded.as_bytes())
            .map_err(|err| IpcError::Io(err.to_string()))?;
        self.writer
            .flush()
            .map_err(|err| IpcError::Io(err.to_string()))?;

        let mut line = String::new();
        let bytes = self
            .reader
            .read_line(&mut line)
            .map_err(|err| IpcError::Io(err.to_string()))?;
        if bytes == 0 {
            return Err(IpcError::Io("compositor closed IPC connection".to_owned()));
        }
        CompositorEvent::decode_line(&line)
    }
}

fn read_nonblocking_lines(stream: &mut UnixStream) -> Result<(Vec<String>, bool), IpcError> {
    use io::Read as _;
    let mut buf = [0_u8; MAX_IPC_MESSAGE_BYTES];
    match stream.read(&mut buf) {
        Ok(0) => Ok((Vec::new(), false)),
        Ok(n) => {
            let text = std::str::from_utf8(&buf[..n])
                .map_err(|_| IpcError::InvalidFrame("non-UTF8 IPC payload".to_owned()))?;
            let lines = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            Ok((lines, true))
        }
        Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok((Vec::new(), true)),
        Err(err) => Err(IpcError::Io(err.to_string())),
    }
}

fn parse_version_tag(tag: &str) -> Result<u16, IpcError> {
    let rest = tag
        .strip_prefix('v')
        .ok_or_else(|| IpcError::InvalidFrame(format!("invalid version prefix '{tag}'")))?;
    rest.parse::<u16>()
        .map_err(|_| IpcError::InvalidFrame(format!("invalid version number '{tag}'")))
}

fn parse_bool_flag(raw: Option<&str>, field: &str) -> Result<bool, IpcError> {
    match raw {
        Some("1" | "true") => Ok(true),
        Some("0" | "false") => Ok(false),
        Some(other) => Err(IpcError::InvalidFrame(format!(
            "invalid boolean value '{other}' for {field}"
        ))),
        None => Err(IpcError::InvalidFrame(format!(
            "missing boolean field {field}"
        ))),
    }
}

fn sanitize_token(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c == '|' || c == '\n' || c == '\r' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encoding_and_decoding_for_requests_and_snapshots() -> Result<(), IpcError> {
        let requests = [
            ShellRequest::Handshake {
                protocol_version: IPC_PROTOCOL_VERSION,
                client_name: "rusty-shell".to_owned(),
            },
            ShellRequest::GetStateSnapshot,
            ShellRequest::SwitchWorkspace { workspace: 4 },
            ShellRequest::FocusWindow { window_id: 42 },
            ShellRequest::ToggleLayoutMode,
            ShellRequest::ToggleOverview,
            ShellRequest::SetReducedMotion { enabled: true },
            ShellRequest::SetLockScreen { locked: true },
            ShellRequest::Notify {
                urgency: Urgency::Critical,
                title: "Battery Low".to_owned(),
                body: "10% remaining".to_owned(),
            },
        ];

        for req in requests {
            let line = req.encode_line();
            let decoded = ShellRequest::decode_line(&line)?;
            assert_eq!(decoded, req);
        }

        let snapshot = CompositorEvent::StateSnapshot(DesktopStateSnapshot {
            active_workspace: 2,
            layout_mode: "master-stack".to_owned(),
            reduced_motion: true,
            overview_active: true,
            screen_locked: false,
            notification_count: 3,
            frame_interval_p50_us: 16_680,
            input_latency_p95_us: 6_500,
            windows: vec![
                WindowSummary {
                    id: 1,
                    workspace: 2,
                    focused: true,
                    maximized: false,
                },
                WindowSummary {
                    id: 2,
                    workspace: 2,
                    focused: false,
                    maximized: true,
                },
            ],
        });
        let encoded_snap = snapshot.encode_line();
        let decoded_snap = CompositorEvent::decode_line(&encoded_snap)?;
        assert_eq!(decoded_snap, snapshot);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_versions_oversized_messages_and_invalid_workspaces() {
        let bad_ver = ShellRequest::decode_line("v99|GET_SNAPSHOT");
        assert_eq!(
            bad_ver,
            Err(IpcError::UnsupportedVersion {
                received: 99,
                expected: IPC_PROTOCOL_VERSION,
            })
        );

        let bad_ws = ShellRequest::decode_line("v1|SWITCH_WORKSPACE|0");
        assert!(matches!(bad_ws, Err(IpcError::InvalidFrame(_))));

        let huge = "a".repeat(MAX_IPC_MESSAGE_BYTES + 10);
        assert_eq!(
            ShellRequest::decode_line(&huge),
            Err(IpcError::MessageTooLarge(MAX_IPC_MESSAGE_BYTES + 10))
        );
    }

    #[test]
    fn unix_socket_server_survives_shell_client_disconnect_and_reconnect() -> Result<(), IpcError> {
        let socket_path =
            PathBuf::from(format!("/tmp/rustyos-ipc-test-{}.sock", std::process::id()));
        let mut server = IpcServer::bind(socket_path.clone())?;

        // Verify 0600 permissions.
        let mode = fs::metadata(&socket_path)
            .map_err(|e| IpcError::Io(e.to_string()))?
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        let mut active_ws: u8 = 1;

        // First shell instance connects, switches workspace to 3, and then drops (simulating crash/restart).
        {
            let writer =
                UnixStream::connect(&socket_path).map_err(|e| IpcError::Io(e.to_string()))?;
            let mut client = IpcClient {
                reader: BufReader::new(
                    writer
                        .try_clone()
                        .map_err(|e| IpcError::Io(e.to_string()))?,
                ),
                writer,
            };
            let req_line = ShellRequest::SwitchWorkspace { workspace: 3 }.encode_line();
            client
                .writer
                .write_all(req_line.as_bytes())
                .map_err(|e| IpcError::Io(e.to_string()))?;

            server.poll_requests(|req| {
                if let ShellRequest::SwitchWorkspace { workspace } = req {
                    active_ws = workspace;
                }
                CompositorEvent::StateSnapshot(DesktopStateSnapshot {
                    active_workspace: active_ws,
                    ..DesktopStateSnapshot::default()
                })
            });
            assert_eq!(active_ws, 3);
        }

        // Poll after client 1 disconnected: server must not error or panic.
        server.poll_requests(|_| CompositorEvent::StateSnapshot(DesktopStateSnapshot::default()));

        // Second shell instance connects and observes active_workspace == 3.
        {
            let mut writer =
                UnixStream::connect(&socket_path).map_err(|e| IpcError::Io(e.to_string()))?;
            writer
                .write_all(ShellRequest::GetStateSnapshot.encode_line().as_bytes())
                .map_err(|e| IpcError::Io(e.to_string()))?;
            server.poll_requests(|_| {
                CompositorEvent::StateSnapshot(DesktopStateSnapshot {
                    active_workspace: active_ws,
                    ..DesktopStateSnapshot::default()
                })
            });
            let mut reader = BufReader::new(writer);
            let mut resp = String::new();
            reader
                .read_line(&mut resp)
                .map_err(|e| IpcError::Io(e.to_string()))?;
            let evt = CompositorEvent::decode_line(&resp)?;
            assert_eq!(
                evt,
                CompositorEvent::StateSnapshot(DesktopStateSnapshot {
                    active_workspace: 3,
                    ..DesktopStateSnapshot::default()
                })
            );
        }

        Ok(())
    }
}
