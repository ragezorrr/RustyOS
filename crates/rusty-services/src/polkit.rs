//! `Polkit` Authentication Agent (`org.freedesktop.PolicyKit1.AuthenticationAgent`) (`RUSTYOS-056`).
//!
//! Registers the RustyOS session agent with `polkitd` on System D-Bus and delegates
//! actual PAM verification to the privileged system helper (`/run/polkit/agent-helper.socket`
//! or `/usr/lib/polkit-1/polkit-agent-helper-1`), keeping compositor/shell unprivileged.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::pin::Pin;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zbus::blocking::Connection;
use zbus::interface;
use zbus::zvariant::{OwnedValue, Value};

use crate::auth::{SecretText, is_valid_unix_username};
use crate::logind::resolve_current_session_id;

const POLKIT_DEST: &str = "org.freedesktop.PolicyKit1";
const POLKIT_AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const POLKIT_AUTHORITY_IFACE: &str = "org.freedesktop.PolicyKit1.Authority";
const POLKIT_AGENT_PATH: &str = "/org/rustyos/PolicyKit1/AuthenticationAgent";
const POLKIT_HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";
const POLKIT_HELPER_BIN: &str = "/usr/lib/polkit-1/polkit-agent-helper-1";
const HELPER_IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Custom `org.freedesktop.PolicyKit1.Error.*` replies returned from `BeginAuthentication`.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
pub enum PolkitAgentError {
    /// Fallthrough `zbus` transport error.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// User dismissed the dialog or `CancelAuthentication` fired.
    Cancelled(String),
    /// Helper binary/socket or protocol failure.
    Failed(String),
}

/// Interactive prompt metadata forwarded from `polkitd` to the compositor dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolkitPromptRequest {
    /// Opaque Polkit request cookie matching `CancelAuthentication` and replies.
    pub cookie: String,
    /// Action identifier (e.g. `"org.freedesktop.policykit.exec"`).
    pub action_id: String,
    /// Localized prompt explanation from Polkit policy.
    pub message: String,
    /// Optional freedesktop icon name advertised by the policy.
    pub icon_name: String,
    /// Selected UNIX username being authenticated.
    pub username: String,
    /// Selected UNIX UID.
    pub uid: u32,
    /// Optional PID of the process requesting authorization (`polkit.subject-pid`).
    pub caller_pid: Option<u32>,
    /// Prompt text emitted by PAM helper (e.g. `"Password:"`).
    pub prompt_label: String,
    /// Whether entered characters should be echoed instead of masked (`PAM_PROMPT_ECHO_ON`).
    pub echo_on: bool,
}

/// Events emitted asynchronously from the Polkit D-Bus worker to the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolkitEvent {
    /// Open (or replace) the modal Polkit password dialog for a request.
    BeginPrompt(PolkitPromptRequest),
    /// Password verification failed; dialog stays open for another attempt.
    PromptError {
        /// Active cookie that failed authentication.
        cookie: String,
        /// Human-readable reason to show inline.
        message: String,
    },
    /// Close the dialog because authorization succeeded or was cancelled.
    ClosePrompt {
        /// Cookie whose dialog should close.
        cookie: String,
    },
}

/// User actions sent from the compositor UI back to the Polkit worker.
#[derive(Debug)]
pub enum PolkitUserReply {
    /// Submit entered secret for `cookie`.
    Submit {
        /// Active request cookie.
        cookie: String,
        /// Zeroizing passphrase buffer.
        secret: SecretText,
    },
    /// Cancel/dismiss the prompt cleanly (`org.freedesktop.PolicyKit1.Error.Cancelled`).
    Cancel {
        /// Active request cookie.
        cookie: String,
    },
}

/// Parsed single line from `polkit-agent-helper-1` stdout / Unix socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperLine {
    /// `PAM_PROMPT_ECHO_OFF <prompt>` (hidden passphrase input).
    PamPromptEchoOff(String),
    /// `PAM_PROMPT_ECHO_ON <prompt>` (visible prompt input).
    PamPromptEchoOn(String),
    /// `PAM_ERROR_MSG <msg>` from PAM stack.
    PamErrorMsg(String),
    /// `PAM_TEXT_INFO <msg>` from PAM stack.
    PamTextInfo(String),
    /// Helper verified credentials and reported `AuthenticationAgentResponse2`.
    Success,
    /// Helper rejected credentials.
    Failure,
    /// Unrecognized auxiliary line.
    Other(String),
}

/// Parses one line emitted by `polkit-agent-helper-1` / `polkit-agent-helper.socket`.
#[must_use]
pub fn parse_helper_line(raw: &str) -> HelperLine {
    let trimmed = raw.trim_end_matches(['\r', '\n']);
    if trimmed == "SUCCESS" {
        return HelperLine::Success;
    }
    if trimmed == "FAILURE" {
        return HelperLine::Failure;
    }
    if let Some(rest) = trimmed.strip_prefix("PAM_PROMPT_ECHO_OFF") {
        return HelperLine::PamPromptEchoOff(sanitize_prompt_fragment(rest));
    }
    if let Some(rest) = trimmed.strip_prefix("PAM_PROMPT_ECHO_ON") {
        return HelperLine::PamPromptEchoOn(sanitize_prompt_fragment(rest));
    }
    if let Some(rest) = trimmed.strip_prefix("PAM_ERROR_MSG") {
        return HelperLine::PamErrorMsg(sanitize_prompt_fragment(rest));
    }
    if let Some(rest) = trimmed.strip_prefix("PAM_TEXT_INFO") {
        return HelperLine::PamTextInfo(sanitize_prompt_fragment(rest));
    }
    HelperLine::Other(sanitize_prompt_fragment(trimmed))
}

fn sanitize_prompt_fragment(raw: &str) -> String {
    raw.replace("\\n", " ")
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Resolves a UNIX UID to its login name using `/etc/passwd` contents.
#[must_use]
pub fn resolve_uid_username_from_passwd(target_uid: u32, passwd_contents: &str) -> Option<String> {
    for line in passwd_contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut fields = trimmed.split(':');
        let Some(name) = fields.next().map(str::trim) else {
            continue;
        };
        let _pw = fields.next();
        let Some(uid) = fields.next().and_then(|s| s.trim().parse::<u32>().ok()) else {
            continue;
        };
        if uid == target_uid && is_valid_unix_username(name) {
            return Some(name.to_owned());
        }
    }
    None
}

/// Picks the preferred `(uid, username)` from `PolicyKit` `a(sa{sv})` identities.
#[must_use]
pub fn pick_preferred_identity<S: ::std::hash::BuildHasher>(
    identities: &[(String, HashMap<String, OwnedValue, S>)],
    preferred_uid: Option<u32>,
    passwd_contents: &str,
) -> Option<(u32, String)> {
    let mut parsed = Vec::new();
    for (kind, props) in identities {
        if kind != "unix-user" {
            continue;
        }
        let Some(uid) = props.get("uid").and_then(|v| {
            u32::try_from(&**v)
                .ok()
                .or_else(|| i32::try_from(&**v).ok().and_then(|i| u32::try_from(i).ok()))
        }) else {
            continue;
        };
        if let Some(name) = resolve_uid_username_from_passwd(uid, passwd_contents) {
            parsed.push((uid, name));
        }
    }

    if let Some(target_uid) = preferred_uid
        && let Some(matched) = parsed.iter().find(|(uid, _)| *uid == target_uid)
    {
        return Some(matched.clone());
    }
    parsed.into_iter().next()
}

fn current_process_uid() -> Option<u32> {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("Uid:")
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|uid| uid.parse::<u32>().ok())
            })
        })
}

/// Minimal pure-`std` single-shot future so `BeginAuthentication` suspends without blocking `zbus`.
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
        if let Ok(mut guard) = self.state.lock() {
            if guard.value.is_none() {
                guard.value = Some(val);
            }
            if let Some(waker) = guard.waker.take() {
                waker.wake();
            }
        }
    }
}

impl<T> Future for AsyncSignal<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state.lock() {
            Ok(mut guard) => {
                if let Some(val) = guard.value.take() {
                    Poll::Ready(val)
                } else {
                    guard.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                if let Some(val) = guard.value.take() {
                    Poll::Ready(val)
                } else {
                    guard.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        }
    }
}

enum HelperConnection {
    Socket {
        reader: BufReader<UnixStream>,
        writer: UnixStream,
    },
    Process {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
    },
}

impl HelperConnection {
    fn from_unix_stream(stream: UnixStream, username: &str, cookie: &str) -> Result<Self, String> {
        let _ = stream.set_read_timeout(Some(HELPER_IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(HELPER_IO_TIMEOUT));
        let mut writer = stream
            .try_clone()
            .map_err(|err| format!("Failed to clone helper stream: {err}"))?;
        let preamble = format!("{username}\n{cookie}\n");
        writer
            .write_all(preamble.as_bytes())
            .and_then(|()| writer.flush())
            .map_err(|err| format!("Failed writing preamble to polkit socket: {err}"))?;
        Ok(Self::Socket {
            reader: BufReader::new(stream),
            writer,
        })
    }

    fn open_system(username: &str, cookie: &str) -> Result<Self, String> {
        if !is_valid_unix_username(username) || cookie.contains(['\n', '\r', '\0']) {
            return Err("Invalid Polkit identity or cookie".to_owned());
        }

        if Path::new(POLKIT_HELPER_SOCKET).exists() {
            let stream = UnixStream::connect(POLKIT_HELPER_SOCKET)
                .map_err(|err| format!("Failed connecting to {POLKIT_HELPER_SOCKET}: {err}"))?;
            return Self::from_unix_stream(stream, username, cookie);
        }

        if Path::new(POLKIT_HELPER_BIN).is_file() {
            let mut child = Command::new(POLKIT_HELPER_BIN)
                .arg(username)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|err| format!("Failed launching {POLKIT_HELPER_BIN}: {err}"))?;
            let Some(mut stdin) = child.stdin.take() else {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Missing stdin pipe on polkit-agent-helper-1".to_owned());
            };
            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Missing stdout pipe on polkit-agent-helper-1".to_owned());
            };
            if let Err(err) = stdin
                .write_all(format!("{cookie}\n").as_bytes())
                .and_then(|()| stdin.flush())
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Failed writing cookie to polkit helper: {err}"));
            }
            return Ok(Self::Process {
                child,
                stdin,
                stdout: BufReader::new(stdout),
            });
        }

        Err("No polkit-agent-helper socket or binary available".to_owned())
    }

    fn read_line(&mut self, buf: &mut String) -> Result<usize, String> {
        buf.clear();
        match self {
            Self::Socket { reader, .. } => reader
                .read_line(buf)
                .map_err(|err| format!("Polkit helper read error: {err}")),
            Self::Process { stdout, .. } => stdout
                .read_line(buf)
                .map_err(|err| format!("Polkit helper read error: {err}")),
        }
    }

    fn write_secret_line(&mut self, secret: &SecretText) -> Result<(), String> {
        let mut payload = Vec::with_capacity(secret.byte_len() + 1);
        payload.extend_from_slice(secret.as_bytes());
        payload.push(b'\n');
        let res = match self {
            Self::Socket { writer, .. } => writer.write_all(&payload).and_then(|()| writer.flush()),
            Self::Process { stdin, .. } => stdin.write_all(&payload).and_then(|()| stdin.flush()),
        };
        payload.fill(0);
        res.map_err(|err| format!("Polkit helper write error: {err}"))
    }
}

impl Drop for HelperConnection {
    fn drop(&mut self) {
        if let Self::Process { child, .. } = self {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

type HelperOpener = Arc<dyn Fn(&str, &str) -> Result<HelperConnection, String> + Send + Sync>;

enum SessionControl {
    Reply(PolkitUserReply),
    ExternalCancel,
}

#[derive(Clone)]
struct ActiveSession {
    cookie: String,
    ctrl_tx: Sender<SessionControl>,
}

fn read_until_prompt_or_outcome(
    helper: &mut HelperConnection,
    last_error: &mut Option<String>,
) -> Result<PromptWaitOutcome, String> {
    let mut line = String::new();
    loop {
        let n = helper.read_line(&mut line)?;
        if n == 0 {
            return Ok(PromptWaitOutcome::Finished {
                success: false,
                error: last_error.take(),
            });
        }
        match parse_helper_line(&line) {
            HelperLine::PamPromptEchoOff(prompt) => {
                return Ok(PromptWaitOutcome::NeedInput {
                    prompt_label: if prompt.is_empty() {
                        "Password:".to_owned()
                    } else {
                        prompt
                    },
                    echo_on: false,
                });
            }
            HelperLine::PamPromptEchoOn(prompt) => {
                return Ok(PromptWaitOutcome::NeedInput {
                    prompt_label: if prompt.is_empty() {
                        "Input:".to_owned()
                    } else {
                        prompt
                    },
                    echo_on: true,
                });
            }
            HelperLine::PamErrorMsg(msg) | HelperLine::PamTextInfo(msg) => {
                if !msg.is_empty() {
                    *last_error = Some(msg);
                }
            }
            HelperLine::Success => {
                return Ok(PromptWaitOutcome::Finished {
                    success: true,
                    error: None,
                });
            }
            HelperLine::Failure => {
                return Ok(PromptWaitOutcome::Finished {
                    success: false,
                    error: last_error.take(),
                });
            }
            HelperLine::Other(_) => {}
        }
    }
}

enum PromptWaitOutcome {
    NeedInput {
        prompt_label: String,
        echo_on: bool,
    },
    Finished {
        success: bool,
        error: Option<String>,
    },
}

enum AttemptOutcome {
    Success,
    RetryNeeded,
    Terminate(Result<(), PolkitAgentError>),
}

fn drive_helper_attempt(
    helper: &mut HelperConnection,
    cookie: &str,
    ctrl_rx: &Receiver<SessionControl>,
    events_tx: &Sender<PolkitEvent>,
    last_error: &mut Option<String>,
) -> AttemptOutcome {
    loop {
        match ctrl_rx.recv() {
            Ok(SessionControl::Reply(PolkitUserReply::Submit {
                cookie: reply_cookie,
                mut secret,
            })) => {
                if reply_cookie != cookie {
                    secret.clear();
                    continue;
                }
                let write_res = helper.write_secret_line(&secret);
                secret.clear();
                if let Err(err) = write_res {
                    let _ = events_tx.send(PolkitEvent::PromptError {
                        cookie: cookie.to_owned(),
                        message: err,
                    });
                    return AttemptOutcome::RetryNeeded;
                }
                match read_until_prompt_or_outcome(helper, last_error) {
                    Ok(PromptWaitOutcome::NeedInput { .. }) => {
                        let _ = events_tx.send(PolkitEvent::PromptError {
                            cookie: cookie.to_owned(),
                            message: last_error
                                .take()
                                .unwrap_or_else(|| "Additional input required".to_owned()),
                        });
                    }
                    Ok(PromptWaitOutcome::Finished { success: true, .. }) => {
                        return AttemptOutcome::Success;
                    }
                    Ok(PromptWaitOutcome::Finished {
                        success: false,
                        error,
                    }) => {
                        let _ = events_tx.send(PolkitEvent::PromptError {
                            cookie: cookie.to_owned(),
                            message: error
                                .unwrap_or_else(|| "Authentication failed — try again".to_owned()),
                        });
                        return AttemptOutcome::RetryNeeded;
                    }
                    Err(err) => {
                        let _ = events_tx.send(PolkitEvent::PromptError {
                            cookie: cookie.to_owned(),
                            message: err,
                        });
                        return AttemptOutcome::RetryNeeded;
                    }
                }
            }
            Ok(SessionControl::Reply(PolkitUserReply::Cancel {
                cookie: reply_cookie,
            })) => {
                if reply_cookie == cookie {
                    return AttemptOutcome::Terminate(Err(PolkitAgentError::Cancelled(
                        "Authentication cancelled by user".to_owned(),
                    )));
                }
            }
            Ok(SessionControl::ExternalCancel) | Err(_) => {
                return AttemptOutcome::Terminate(Err(PolkitAgentError::Cancelled(
                    "Authentication cancelled".to_owned(),
                )));
            }
        }
    }
}

fn run_session_coordinator(
    request: &PolkitPromptRequest,
    helper_opener: &HelperOpener,
    events_tx: &Sender<PolkitEvent>,
    ctrl_rx: &Receiver<SessionControl>,
    active_slot: &Arc<Mutex<Option<ActiveSession>>>,
    done_tx: AsyncSignalTx<Result<(), PolkitAgentError>>,
) {
    let cookie = request.cookie.clone();
    let username = request.username.clone();
    let mut prompt_sent = false;
    let mut last_error: Option<String> = None;

    let result = loop {
        let mut helper = match helper_opener(&username, &cookie) {
            Ok(h) => h,
            Err(err) => break Err(PolkitAgentError::Failed(err)),
        };

        let (prompt_label, echo_on) =
            match read_until_prompt_or_outcome(&mut helper, &mut last_error) {
                Ok(PromptWaitOutcome::NeedInput {
                    prompt_label,
                    echo_on,
                }) => (prompt_label, echo_on),
                Ok(PromptWaitOutcome::Finished { success: true, .. }) => break Ok(()),
                Ok(PromptWaitOutcome::Finished {
                    success: false,
                    error,
                }) => {
                    break Err(PolkitAgentError::Failed(error.unwrap_or_else(|| {
                        "Polkit helper failed before prompt".to_owned()
                    })));
                }
                Err(err) => break Err(PolkitAgentError::Failed(err)),
            };

        if !prompt_sent {
            let mut initial_req = request.clone();
            initial_req.prompt_label = prompt_label;
            initial_req.echo_on = echo_on;
            let _ = events_tx.send(PolkitEvent::BeginPrompt(initial_req));
            prompt_sent = true;
        }

        match drive_helper_attempt(&mut helper, &cookie, ctrl_rx, events_tx, &mut last_error) {
            AttemptOutcome::Success => break Ok(()),
            AttemptOutcome::RetryNeeded => {}
            AttemptOutcome::Terminate(res) => break res,
        }
    };

    if let Ok(mut guard) = active_slot.lock()
        && guard.as_ref().is_some_and(|s| s.cookie == cookie)
    {
        *guard = None;
    }
    if prompt_sent || matches!(result, Err(PolkitAgentError::Cancelled(_))) {
        let _ = events_tx.send(PolkitEvent::ClosePrompt { cookie });
    }
    done_tx.send(result);
}

struct PolkitAgentInterface {
    active: Arc<Mutex<Option<ActiveSession>>>,
    events_tx: Sender<PolkitEvent>,
    helper_opener: HelperOpener,
}

#[interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl PolkitAgentInterface {
    #[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> Result<(), PolkitAgentError> {
        let passwd = fs::read_to_string("/etc/passwd").unwrap_or_default();
        let preferred_uid = current_process_uid();
        let Some((uid, username)) = pick_preferred_identity(&identities, preferred_uid, &passwd)
        else {
            return Err(PolkitAgentError::Failed(
                "No valid unix-user identity provided by PolicyKit".to_owned(),
            ));
        };

        let caller_pid = details
            .get("polkit.subject-pid")
            .or_else(|| details.get("polkit.caller-pid"))
            .and_then(|s| s.trim().parse::<u32>().ok());

        let (done_tx, done_rx) = AsyncSignal::channel();
        let (ctrl_tx, ctrl_rx) = mpsc::channel();

        if let Ok(mut guard) = self.active.lock() {
            if let Some(prev) = guard.take() {
                let _ = prev.ctrl_tx.send(SessionControl::ExternalCancel);
            }
            *guard = Some(ActiveSession {
                cookie: cookie.clone(),
                ctrl_tx,
            });
        }

        let req = PolkitPromptRequest {
            cookie,
            action_id,
            message,
            icon_name,
            username,
            uid,
            caller_pid,
            prompt_label: "Password:".to_owned(),
            echo_on: false,
        };

        let opener = Arc::clone(&self.helper_opener);
        let events_tx = self.events_tx.clone();
        let active_slot = Arc::clone(&self.active);

        if thread::Builder::new()
            .name("rusty-polkit-helper".to_owned())
            .spawn(move || {
                run_session_coordinator(&req, &opener, &events_tx, &ctrl_rx, &active_slot, done_tx);
            })
            .is_err()
        {
            return Err(PolkitAgentError::Failed(
                "Failed spawning Polkit helper coordinator thread".to_owned(),
            ));
        }

        done_rx.await
    }

    fn cancel_authentication(&self, cookie: &str) {
        if let Ok(mut guard) = self.active.lock()
            && guard.as_ref().is_some_and(|s| s.cookie == cookie)
            && let Some(session) = guard.take()
        {
            let _ = session.ctrl_tx.send(SessionControl::ExternalCancel);
        }
    }
}

/// Non-blocking compositor handle owning the native Polkit Authentication Agent lifecycle.
pub struct PolkitAgentHandle {
    events_rx: Mutex<Receiver<PolkitEvent>>,
    reply_tx: Option<Sender<PolkitUserReply>>,
    active: Arc<Mutex<Option<ActiveSession>>>,
    stop_flag: Arc<AtomicBool>,
    conn_slot: Arc<Mutex<Option<Connection>>>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for PolkitAgentHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolkitAgentHandle")
            .field("active", &self.worker.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for PolkitAgentHandle {
    fn default() -> Self {
        let (_tx, rx) = mpsc::channel();
        Self {
            events_rx: Mutex::new(rx),
            reply_tx: None,
            active: Arc::new(Mutex::new(None)),
            stop_flag: Arc::new(AtomicBool::new(true)),
            conn_slot: Arc::new(Mutex::new(None)),
            worker: None,
        }
    }
}

impl Drop for PolkitAgentHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Ok(mut guard) = self.active.lock()
            && let Some(session) = guard.take()
        {
            let _ = session.ctrl_tx.send(SessionControl::ExternalCancel);
        }
        if let Ok(mut guard) = self.conn_slot.lock()
            && let Some(conn) = guard.take()
        {
            let _ = conn.close();
        }
    }
}

impl PolkitAgentHandle {
    /// Registers the session's `org.freedesktop.PolicyKit1.AuthenticationAgent` on System D-Bus.
    ///
    /// Gracefully stays inactive if System D-Bus is absent or another host Polkit agent already
    /// owns the current graphical session (for instance in nested `--winit` sessions).
    #[must_use]
    pub fn start_background() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel::<PolkitUserReply>();
        let active: Arc<Mutex<Option<ActiveSession>>> = Arc::new(Mutex::new(None));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let conn_slot = Arc::new(Mutex::new(None));

        let worker_active = Arc::clone(&active);
        let worker_stop = Arc::clone(&stop_flag);
        let worker_conn = Arc::clone(&conn_slot);

        let worker = thread::Builder::new()
            .name("rusty-polkit".to_owned())
            .spawn(move || {
                run_polkit_agent_loop(
                    &events_tx,
                    &reply_rx,
                    &worker_active,
                    &worker_conn,
                    &worker_stop,
                );
            })
            .ok();

        Self {
            events_rx: Mutex::new(events_rx),
            reply_tx: worker.as_ref().map(|_| reply_tx),
            active,
            stop_flag,
            conn_slot,
            worker,
        }
    }

    /// Creates a deterministic test handle wired to custom event/reply channels.
    #[must_use]
    pub fn with_test_channels(
        events_rx: Receiver<PolkitEvent>,
        reply_tx: Sender<PolkitUserReply>,
    ) -> Self {
        Self {
            events_rx: Mutex::new(events_rx),
            reply_tx: Some(reply_tx),
            active: Arc::new(Mutex::new(None)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            conn_slot: Arc::new(Mutex::new(None)),
            worker: None,
        }
    }

    /// Non-blockingly drains queued Polkit authentication events.
    #[must_use]
    pub fn drain_events(&self) -> Vec<PolkitEvent> {
        let Ok(guard) = self.events_rx.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(ev) = guard.try_recv() {
            out.push(ev);
        }
        out
    }

    /// Submits a user passphrase attempt for `cookie`.
    pub fn submit_password(&self, cookie: String, mut secret: SecretText) {
        if let Some(ref tx) = self.reply_tx {
            let _ = tx.send(PolkitUserReply::Submit { cookie, secret });
        } else {
            secret.clear();
        }
    }

    /// Cancels the active Polkit dialog for `cookie`.
    pub fn cancel(&self, cookie: String) {
        if let Some(ref tx) = self.reply_tx {
            let _ = tx.send(PolkitUserReply::Cancel { cookie });
        }
    }
}

fn build_session_subject(session_id: &str) -> (&'static str, HashMap<&'static str, Value<'_>>) {
    let mut map = HashMap::new();
    map.insert("session-id", Value::from(session_id));
    ("unix-session", map)
}

fn polkit_locale() -> String {
    std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "en_US.UTF-8".to_owned())
}

fn run_polkit_agent_loop(
    events_tx: &Sender<PolkitEvent>,
    reply_rx: &Receiver<PolkitUserReply>,
    active: &Arc<Mutex<Option<ActiveSession>>>,
    conn_slot: &Arc<Mutex<Option<Connection>>>,
    stop_flag: &AtomicBool,
) {
    let iface = PolkitAgentInterface {
        active: Arc::clone(active),
        events_tx: events_tx.clone(),
        helper_opener: Arc::new(HelperConnection::open_system),
    };

    let Ok(conn) = zbus::blocking::connection::Builder::system()
        .and_then(|b| b.serve_at(POLKIT_AGENT_PATH, iface))
        .and_then(zbus::blocking::connection::Builder::build)
    else {
        return;
    };

    if let Ok(mut guard) = conn_slot.lock() {
        *guard = Some(conn.clone());
    }

    let Some(session_id) = resolve_current_session_id(&conn) else {
        return;
    };
    let subject = build_session_subject(&session_id);
    let locale = polkit_locale();

    let registered = conn
        .call_method(
            Some(POLKIT_DEST),
            POLKIT_AUTHORITY_PATH,
            Some(POLKIT_AUTHORITY_IFACE),
            "RegisterAuthenticationAgent",
            &(subject.clone(), locale.as_str(), POLKIT_AGENT_PATH),
        )
        .is_ok();

    if !registered {
        if let Ok(mut guard) = conn_slot.lock() {
            let _ = guard.take();
        }
        return;
    }

    while !stop_flag.load(Ordering::Acquire) {
        match reply_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(reply) => {
                let target_cookie = match &reply {
                    PolkitUserReply::Submit { cookie, .. } | PolkitUserReply::Cancel { cookie } => {
                        cookie
                    }
                };
                if let Ok(guard) = active.lock()
                    && let Some(session) = guard.as_ref()
                    && session.cookie == *target_cookie
                {
                    let _ = session.ctrl_tx.send(SessionControl::Reply(reply));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = conn.call_method(
        Some(POLKIT_DEST),
        POLKIT_AUTHORITY_PATH,
        Some(POLKIT_AUTHORITY_IFACE),
        "UnregisterAuthenticationAgent",
        &(subject, POLKIT_AGENT_PATH),
    );

    if let Ok(mut guard) = conn_slot.lock() {
        let _ = guard.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_polkit_helper_line_variants() {
        assert_eq!(
            parse_helper_line("PAM_PROMPT_ECHO_OFF Password: \n"),
            HelperLine::PamPromptEchoOff("Password:".to_owned())
        );
        assert_eq!(
            parse_helper_line("PAM_PROMPT_ECHO_ON Verification code: "),
            HelperLine::PamPromptEchoOn("Verification code:".to_owned())
        );
        assert_eq!(
            parse_helper_line("PAM_ERROR_MSG Authentication failure\\n\r\n"),
            HelperLine::PamErrorMsg("Authentication failure".to_owned())
        );
        assert_eq!(
            parse_helper_line("PAM_TEXT_INFO Insert security key"),
            HelperLine::PamTextInfo("Insert security key".to_owned())
        );
        assert_eq!(parse_helper_line("SUCCESS\n"), HelperLine::Success);
        assert_eq!(parse_helper_line("FAILURE\r\n"), HelperLine::Failure);
    }

    #[test]
    fn picks_preferred_identity_from_polkit_subjects() {
        let passwd = "\
root:x:0:0:root:/root:/bin/bash
anc:x:1000:1000:Anc:/home/anc:/bin/bash
";
        let mut root_props = HashMap::new();
        root_props.insert("uid".to_owned(), OwnedValue::from(0_u32));
        let mut user_props = HashMap::new();
        user_props.insert("uid".to_owned(), OwnedValue::from(1000_u32));
        let identities = vec![
            ("unix-user".to_owned(), root_props),
            ("unix-user".to_owned(), user_props),
        ];

        assert_eq!(
            pick_preferred_identity(&identities, Some(1000), passwd),
            Some((1000, "anc".to_owned()))
        );
        assert_eq!(
            pick_preferred_identity(&identities, Some(2000), passwd),
            Some((0, "root".to_owned()))
        );
    }

    fn sample_req(cookie: &str) -> PolkitPromptRequest {
        PolkitPromptRequest {
            cookie: cookie.to_owned(),
            action_id: "org.freedesktop.policykit.exec".to_owned(),
            message: "Authentication required".to_owned(),
            icon_name: "dialog-password".to_owned(),
            username: "anc".to_owned(),
            uid: 1000,
            caller_pid: Some(1234),
            prompt_label: "Password:".to_owned(),
            echo_on: false,
        }
    }

    fn poll_signal(
        sig: &mut AsyncSignal<Result<(), PolkitAgentError>>,
    ) -> Option<Result<(), PolkitAgentError>> {
        for _ in 0..50 {
            let mut cx = Context::from_waker(Waker::noop());
            if let Poll::Ready(res) = Pin::new(&mut *sig).poll(&mut cx) {
                return Some(res);
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn session_coordinator_handles_retry_and_success_over_unix_sockets()
    -> Result<(), Box<dyn std::error::Error>> {
        let attempt = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempt_clone = Arc::clone(&attempt);
        let opener: HelperOpener = Arc::new(move |username, cookie| {
            assert_eq!(username, "anc");
            assert_eq!(cookie, "cookie-1");
            let (client_sock, mut helper_sock) = UnixStream::pair().map_err(|e| e.to_string())?;
            let n = attempt_clone.fetch_add(1, Ordering::Relaxed);
            thread::spawn(move || {
                let Ok(cloned) = helper_sock.try_clone() else {
                    return;
                };
                let mut br = BufReader::new(cloned);
                let mut l1 = String::new();
                let mut l2 = String::new();
                let mut pass = String::new();
                let _ = br.read_line(&mut l1);
                let _ = br.read_line(&mut l2);
                let _ = helper_sock.write_all(b"PAM_PROMPT_ECHO_OFF Password: \n");
                let _ = helper_sock.flush();
                let _ = br.read_line(&mut pass);
                let reply = if n == 0 {
                    b"PAM_ERROR_MSG Bad password\nFAILURE\n".as_slice()
                } else {
                    b"SUCCESS\n".as_slice()
                };
                let _ = helper_sock.write_all(reply);
                let _ = helper_sock.flush();
            });
            HelperConnection::from_unix_stream(client_sock, username, cookie)
        });

        let (events_tx, events_rx) = mpsc::channel();
        let (ctrl_tx, ctrl_rx) = mpsc::channel();
        let active = Arc::new(Mutex::new(None));
        let (done_tx, mut done_rx) = AsyncSignal::channel();
        let req = sample_req("cookie-1");

        thread::spawn(move || {
            run_session_coordinator(&req, &opener, &events_tx, &ctrl_rx, &active, done_tx);
        });

        assert!(matches!(
            events_rx.recv_timeout(Duration::from_secs(2))?,
            PolkitEvent::BeginPrompt(_)
        ));
        ctrl_tx.send(SessionControl::Reply(PolkitUserReply::Submit {
            cookie: "cookie-1".to_owned(),
            secret: SecretText::from_slice("wrong"),
        }))?;
        assert!(matches!(
            events_rx.recv_timeout(Duration::from_secs(2))?,
            PolkitEvent::PromptError { .. }
        ));
        ctrl_tx.send(SessionControl::Reply(PolkitUserReply::Submit {
            cookie: "cookie-1".to_owned(),
            secret: SecretText::from_slice("correct"),
        }))?;
        assert!(matches!(
            events_rx.recv_timeout(Duration::from_secs(2))?,
            PolkitEvent::ClosePrompt { .. }
        ));
        assert!(matches!(poll_signal(&mut done_rx), Some(Ok(()))));
        Ok(())
    }

    #[test]
    fn session_coordinator_returns_cancelled_on_external_abort()
    -> Result<(), Box<dyn std::error::Error>> {
        let cancel_opener: HelperOpener = Arc::new(|username, cookie| {
            let (client_sock, mut helper_sock) = UnixStream::pair().map_err(|e| e.to_string())?;
            thread::spawn(move || {
                let _ = helper_sock.write_all(b"PAM_PROMPT_ECHO_OFF Password: \n");
                let _ = helper_sock.flush();
                thread::sleep(Duration::from_millis(200));
            });
            HelperConnection::from_unix_stream(client_sock, username, cookie)
        });
        let (cancel_events_tx, cancel_events_rx) = mpsc::channel();
        let (cancel_ctrl_tx, cancel_ctrl_rx) = mpsc::channel();
        let active2 = Arc::new(Mutex::new(None));
        let (cancel_done_tx, mut cancel_done_rx) = AsyncSignal::channel();
        let req2 = sample_req("cookie-2");

        thread::spawn(move || {
            run_session_coordinator(
                &req2,
                &cancel_opener,
                &cancel_events_tx,
                &cancel_ctrl_rx,
                &active2,
                cancel_done_tx,
            );
        });
        assert!(matches!(
            cancel_events_rx.recv_timeout(Duration::from_secs(2))?,
            PolkitEvent::BeginPrompt(_)
        ));
        cancel_ctrl_tx.send(SessionControl::ExternalCancel)?;
        assert!(matches!(
            cancel_events_rx.recv_timeout(Duration::from_secs(2))?,
            PolkitEvent::ClosePrompt { .. }
        ));
        assert!(matches!(
            poll_signal(&mut cancel_done_rx),
            Some(Err(PolkitAgentError::Cancelled(_)))
        ));
        Ok(())
    }
}
