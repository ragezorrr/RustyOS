//! Non-blocking PAM / shadow password verification helpers (`RUSTYOS-056`, `SECURITY.md`).
//!
//! Uses Linux-PAM's privileged helper (`/usr/bin/unix_chkpwd` / `/sbin/unix_chkpwd`)
//! so neither the compositor nor the shell requires elevated privileges or `unsafe` FFI.

use std::fmt;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_SECRET_BYTES: usize = 512;
const HELPER_TIMEOUT: Duration = Duration::from_secs(5);
const UNIX_CHKPWD_PATHS: [&str; 2] = ["/usr/bin/unix_chkpwd", "/sbin/unix_chkpwd"];

/// Fixed-capacity UTF-8 secret buffer that zeros its storage in-place on clear/drop (`SECURITY.md`).
///
/// Pre-reserves [`MAX_SECRET_BYTES`] on construction so ordinary passphrase entry
/// avoids heap reallocations that might leave plaintext copies behind.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretText {
    bytes: Vec<u8>,
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText([REDACTED])")
    }
}

impl Default for SecretText {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        self.clear();
    }
}

impl SecretText {
    /// Creates an empty secret buffer pre-allocated to [`MAX_SECRET_BYTES`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(MAX_SECRET_BYTES),
        }
    }

    /// Creates a secret buffer seeded from `raw` (primarily for tests / mock calls).
    #[must_use]
    pub fn from_slice(raw: &str) -> Self {
        let mut s = Self::new();
        s.push_str(raw);
        s
    }

    /// Returns `true` when no bytes are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns stored byte length.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns Unicode scalar count for masked dot rendering (`•`).
    #[must_use]
    pub fn char_len(&self) -> usize {
        std::str::from_utf8(&self.bytes).map_or_else(|_| self.bytes.len(), |s| s.chars().count())
    }

    /// Returns `true` if appending `text` fits safely within [`MAX_SECRET_BYTES`].
    pub fn push_str(&mut self, text: &str) -> bool {
        let filtered_len: usize = text
            .chars()
            .filter(|c| !c.is_control() && *c != '\0')
            .map(char::len_utf8)
            .sum();
        if self.bytes.len().saturating_add(filtered_len) > self.bytes.capacity() {
            return false;
        }
        for ch in text.chars().filter(|c| !c.is_control() && *c != '\0') {
            let mut buf = [0_u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            self.bytes.extend_from_slice(encoded.as_bytes());
            buf.fill(0);
        }
        true
    }

    /// Removes the trailing UTF-8 character and overwrites its vacated bytes with zeros.
    pub fn pop_char(&mut self) -> bool {
        let Ok(as_str) = std::str::from_utf8(&self.bytes) else {
            self.clear();
            return false;
        };
        let Some(last) = as_str.chars().next_back() else {
            return false;
        };
        let new_len = self.bytes.len().saturating_sub(last.len_utf8());
        self.bytes[new_len..].fill(0);
        self.bytes.truncate(new_len);
        true
    }

    /// Overwrites active bytes with zeros and clears the buffer.
    pub fn clear(&mut self) {
        self.bytes.fill(0);
        self.bytes.clear();
    }

    /// Moves all secret bytes into a fresh [`SecretText`] and wipes `self`.
    #[must_use]
    pub fn take(&mut self) -> Self {
        let mut out = Self::new();
        out.bytes.extend_from_slice(&self.bytes);
        self.clear();
        out
    }

    /// Exposes stored bytes strictly for immediate child stdin/socket handoff.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Formats visible text when an authentication prompt asks for echo (`PAM_PROMPT_ECHO_ON`).
    #[must_use]
    pub fn visible_or_masked(&self, echo_on: bool, max_dots: usize) -> String {
        if echo_on && let Ok(s) = std::str::from_utf8(&self.bytes) {
            return s.chars().take(max_dots).collect();
        }
        "•".repeat(self.char_len().min(max_dots))
    }
}

/// Result of verifying a user's password via PAM helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    /// Credentials verified cleanly (`PAM_SUCCESS`).
    Verified,
    /// Invalid username/password (`PAM_AUTH_ERR`).
    InvalidCredentials,
    /// Verification helper is missing, timed out, or refused verification for another user.
    Unavailable(String),
}

/// Asynchronous password verification response delivered back to the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCheckResult {
    /// Caller-assigned monotonic request token to discard stale responses.
    pub request_id: u64,
    /// Target username that was checked.
    pub username: String,
    /// Verification outcome.
    pub outcome: AuthOutcome,
}

/// Validates that `username` is a safe single POSIX login name before passing to a helper binary.
#[must_use]
pub fn is_valid_unix_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 64
        && !username.starts_with('-')
        && username
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'$'))
}

/// Locates the Linux-PAM `unix_chkpwd` setuid helper if installed on the host.
#[must_use]
pub fn find_unix_chkpwd_binary() -> Option<&'static Path> {
    UNIX_CHKPWD_PATHS
        .iter()
        .map(Path::new)
        .find(|p| p.is_file())
}

/// Verifies `password` for the current effective user via `/usr/bin/unix_chkpwd` (`/sbin/unix_chkpwd`).
#[must_use]
pub fn verify_current_user_password(username: &str, password: &SecretText) -> AuthOutcome {
    if !is_valid_unix_username(username) {
        return AuthOutcome::InvalidCredentials;
    }
    if let Ok(active_user) = std::env::var("USER")
        && !active_user.is_empty()
        && active_user != username
    {
        return AuthOutcome::Unavailable(format!(
            "Unlocking '{username}' requires switching user via display manager"
        ));
    }
    let Some(helper) = find_unix_chkpwd_binary() else {
        return AuthOutcome::Unavailable("PAM helper unix_chkpwd is not installed".to_owned());
    };
    verify_with_helper_binary(helper, username, password)
}

/// Executes a `unix_chkpwd`-compatible helper binary at `helper` with `<password>\0` on stdin.
#[must_use]
pub fn verify_with_helper_binary(
    helper: &Path,
    username: &str,
    password: &SecretText,
) -> AuthOutcome {
    if !is_valid_unix_username(username) || password.as_bytes().contains(&0) {
        return AuthOutcome::InvalidCredentials;
    }

    let mut child = match Command::new(helper)
        .args([username, "nullok"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(err) => {
            return AuthOutcome::Unavailable(format!("Failed to launch unix_chkpwd: {err}"));
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let mut payload = Vec::with_capacity(password.byte_len() + 1);
        payload.extend_from_slice(password.as_bytes());
        payload.push(0);
        let write_res = stdin.write_all(&payload).and_then(|()| stdin.flush());
        payload.fill(0);
        drop(stdin);
        if let Err(err) = write_res {
            let _ = child.kill();
            let _ = child.wait();
            return AuthOutcome::Unavailable(format!("Failed to write to PAM helper: {err}"));
        }
    }

    let deadline = Instant::now() + HELPER_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => AuthOutcome::Verified,
                    Some(7 | 10) => AuthOutcome::InvalidCredentials,
                    Some(other) => {
                        AuthOutcome::Unavailable(format!("PAM verification returned code {other}"))
                    }
                    None => AuthOutcome::Unavailable("PAM helper terminated by signal".to_owned()),
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return AuthOutcome::Unavailable("PAM verification timed out".to_owned());
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => {
                return AuthOutcome::Unavailable(format!("Failed waiting on PAM helper: {err}"));
            }
        }
    }
}

type VerifierCallback = Arc<dyn Fn(&str, &SecretText) -> AuthOutcome + Send + Sync>;

struct AuthCmd {
    request_id: u64,
    username: String,
    password: SecretText,
}

/// Non-blocking handle running password checks on a dedicated background worker thread.
pub struct AsyncAuthHandle {
    cmd_tx: Option<Sender<AuthCmd>>,
    results_rx: Mutex<Receiver<AuthCheckResult>>,
    _worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for AsyncAuthHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncAuthHandle")
            .field("active", &self.cmd_tx.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for AsyncAuthHandle {
    fn default() -> Self {
        let (_rx_tx, results_rx) = mpsc::channel();
        Self {
            cmd_tx: None,
            results_rx: Mutex::new(results_rx),
            _worker: None,
        }
    }
}

impl AsyncAuthHandle {
    /// Starts the real PAM (`unix_chkpwd`) verification worker thread.
    #[must_use]
    pub fn start_background() -> Self {
        Self::with_verifier(Arc::new(verify_current_user_password))
    }

    /// Starts a verification worker backed by a custom callback (primarily for isolated tests).
    #[must_use]
    pub fn with_verifier(verifier: VerifierCallback) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AuthCmd>();
        let (results_tx, results_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("rusty-auth".to_owned())
            .spawn(move || {
                while let Ok(mut cmd) = cmd_rx.recv() {
                    let outcome = verifier(&cmd.username, &cmd.password);
                    cmd.password.clear();
                    let _ = results_tx.send(AuthCheckResult {
                        request_id: cmd.request_id,
                        username: cmd.username,
                        outcome,
                    });
                }
            })
            .ok();

        Self {
            cmd_tx: worker.as_ref().map(|_| cmd_tx),
            results_rx: Mutex::new(results_rx),
            _worker: worker,
        }
    }

    /// Returns `true` if a background worker is running.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.cmd_tx.is_some()
    }

    /// Queues a non-blocking verification check; returns `false` if no worker is attached.
    #[must_use]
    pub fn submit_check(
        &self,
        request_id: u64,
        username: String,
        mut password: SecretText,
    ) -> bool {
        let Some(ref tx) = self.cmd_tx else {
            password.clear();
            return false;
        };
        if tx
            .send(AuthCmd {
                request_id,
                username,
                password,
            })
            .is_err()
        {
            return false;
        }
        true
    }

    /// Non-blockingly drains completed authentication checks.
    #[must_use]
    pub fn drain_results(&self) -> Vec<AuthCheckResult> {
        let Ok(guard) = self.results_rx.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(res) = guard.try_recv() {
            out.push(res);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_text_handles_utf8_editing_redacts_debug_and_zeros_on_drop() {
        let mut sec = SecretText::new();
        assert!(sec.is_empty());
        assert!(sec.push_str("пароль"));
        assert_eq!(sec.char_len(), 6);
        assert_eq!(sec.visible_or_masked(false, 16), "••••••");
        assert_eq!(sec.visible_or_masked(true, 16), "пароль");

        let dbg = format!("{sec:?}");
        assert_eq!(dbg, "SecretText([REDACTED])");
        assert!(!dbg.contains("пароль"));

        assert!(sec.pop_char());
        assert_eq!(sec.char_len(), 5);
        assert!(sec.push_str("!\n\0?"));
        // Control chars & NUL are filtered out cleanly
        assert_eq!(sec.visible_or_masked(true, 16), "парол!?");

        let taken = sec.take();
        assert!(sec.is_empty());
        assert_eq!(taken.char_len(), 7);
    }

    #[test]
    fn validates_posix_usernames_safely() {
        assert!(is_valid_unix_username("anc"));
        assert!(is_valid_unix_username("john.doe_1"));
        assert!(!is_valid_unix_username(""));
        assert!(!is_valid_unix_username("-root"));
        assert!(!is_valid_unix_username("root/etc"));
        assert!(!is_valid_unix_username("a b"));
    }

    #[test]
    fn async_auth_handle_verifies_via_injected_verifier() {
        let handle = AsyncAuthHandle::with_verifier(Arc::new(|user, pass| {
            if user == "anc" && pass.as_bytes() == b"secret" {
                AuthOutcome::Verified
            } else {
                AuthOutcome::InvalidCredentials
            }
        }));
        assert!(handle.is_active());
        assert!(handle.submit_check(1, "anc".to_owned(), SecretText::from_slice("wrong")));
        assert!(handle.submit_check(2, "anc".to_owned(), SecretText::from_slice("secret")));

        let mut results = Vec::new();
        for _ in 0..50 {
            results.extend(handle.drain_results());
            if results.len() == 2 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].request_id, 1);
        assert_eq!(results[0].outcome, AuthOutcome::InvalidCredentials);
        assert_eq!(results[1].request_id, 2);
        assert_eq!(results[1].outcome, AuthOutcome::Verified);
    }
}
