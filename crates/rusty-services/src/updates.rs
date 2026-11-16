//! Unprivileged Arch Linux and RustyOS update checker (`RUSTYOS-076`, ADR-0036).
//!
//! Runs `checkupdates --nocolor` (`pacman-contrib`) in a background worker thread so
//! the compositor event loop never blocks on network/disk I/O and `/var/lib/pacman`
//! is never mutated outside explicit privileged `pacman -Syu` execution.

use std::fmt;
use std::io::Read as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::display_session::local_clock_hhmm;

const MAX_STDOUT_BYTES: usize = 512 * 1024;
const MAX_STORED_ITEMS: usize = 64;
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const INITIAL_BOOT_DELAY: Duration = Duration::from_secs(3);

/// Current lifecycle state of the background update check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateCheckState {
    /// Checker disabled or awaiting first run.
    #[default]
    Idle,
    /// `checkupdates` scan currently running in the background.
    Checking,
    /// Repositories checked cleanly; zero updates pending.
    UpToDate,
    /// At least one Arch Linux or RustyOS update is available.
    Available,
    /// `checkupdates` binary missing or transient check failure.
    Unavailable,
}

/// Sanitized description of a single pending package update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageUpdateItem {
    /// Package name (`linux`, `mesa`, `rustyos-git`, etc.).
    pub name: String,
    /// Installed package version string.
    pub old_version: String,
    /// Candidate target version string.
    pub new_version: String,
    /// `true` when this package belongs to RustyOS itself.
    pub is_rustyos: bool,
    /// `true` when updating this package warrants a session restart or reboot.
    pub requires_restart: bool,
}

/// Immutable snapshot consumed by `TopBar` and the updates popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatusSnapshot {
    /// Current state of the background checker.
    pub state: UpdateCheckState,
    /// Total pending updates across Arch and RustyOS.
    pub total_count: usize,
    /// Pending Arch Linux system packages (`!is_rustyos`).
    pub arch_count: usize,
    /// Whether an update to `rustyos` / `rustyos-*` is pending.
    pub rustyos_available: bool,
    /// Whether any pending update touches kernel, GPU drivers, systemd, or RustyOS.
    pub restart_recommended: bool,
    /// Human-readable timestamp or `"Never"` / `"Checking..."`.
    pub last_checked_label: String,
    /// Short diagnostic message when `state == Unavailable`.
    pub error_hint: Option<String>,
    /// Prioritized sample of pending updates (RustyOS and restart-critical first).
    pub items: Vec<PackageUpdateItem>,
}

impl Default for UpdateStatusSnapshot {
    fn default() -> Self {
        Self {
            state: UpdateCheckState::Idle,
            total_count: 0,
            arch_count: 0,
            rustyos_available: false,
            restart_recommended: false,
            last_checked_label: "Never".to_owned(),
            error_hint: None,
            items: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct SharedWorkerConfig {
    enabled: bool,
    interval: Duration,
    force_check: bool,
    stop: bool,
}

/// Non-blocking handle controlling the background Arch/RustyOS update checker thread.
pub struct UpdateCheckerHandle {
    snapshot: Arc<Mutex<UpdateStatusSnapshot>>,
    dirty: Arc<AtomicBool>,
    signal: Arc<(Mutex<SharedWorkerConfig>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for UpdateCheckerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateCheckerHandle")
            .field("active", &self.worker.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for UpdateCheckerHandle {
    fn default() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(UpdateStatusSnapshot::default())),
            dirty: Arc::new(AtomicBool::new(false)),
            signal: Arc::new((
                Mutex::new(SharedWorkerConfig {
                    enabled: false,
                    interval: Duration::from_hours(3),
                    force_check: false,
                    stop: true,
                }),
                Condvar::new(),
            )),
            worker: None,
        }
    }
}

impl Drop for UpdateCheckerHandle {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.signal.0.lock() {
            guard.stop = true;
            self.signal.1.notify_all();
        }
    }
}

impl UpdateCheckerHandle {
    /// Spawns the background update checker thread unless disabled cleanly.
    #[must_use]
    pub fn start_background(enabled: bool, interval_minutes: u32) -> Self {
        let clamped_mins = interval_minutes.clamp(15, 10_080);
        let snapshot = Arc::new(Mutex::new(UpdateStatusSnapshot::default()));
        let dirty = Arc::new(AtomicBool::new(false));
        let signal = Arc::new((
            Mutex::new(SharedWorkerConfig {
                enabled,
                interval: Duration::from_secs(u64::from(clamped_mins) * 60),
                force_check: false,
                stop: false,
            }),
            Condvar::new(),
        ));

        let snap_ref = Arc::clone(&snapshot);
        let dirty_ref = Arc::clone(&dirty);
        let sig_ref = Arc::clone(&signal);
        let worker = thread::Builder::new()
            .name("rusty-updates".to_owned())
            .spawn(move || run_worker_loop(&snap_ref, &dirty_ref, &sig_ref))
            .ok();

        Self {
            snapshot,
            dirty,
            signal,
            worker,
        }
    }

    /// Returns a cloned snapshot of the latest update status.
    #[must_use]
    pub fn snapshot(&self) -> UpdateStatusSnapshot {
        self.snapshot
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Returns `true` and clears the pending UI notification flag when state changed.
    #[must_use]
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// Requests an immediate background `checkupdates` scan without blocking the caller.
    pub fn trigger_check_now(&self) {
        if let Ok(mut guard) = self.signal.0.lock() {
            guard.force_check = true;
            self.signal.1.notify_all();
        }
    }

    /// Updates polling policy dynamically when `[updates]` changes in `config.toml`.
    pub fn reconfigure(&self, enabled: bool, interval_minutes: u32) {
        let clamped_mins = interval_minutes.clamp(15, 10_080);
        if let Ok(mut guard) = self.signal.0.lock() {
            guard.enabled = enabled;
            guard.interval = Duration::from_secs(u64::from(clamped_mins) * 60);
            self.signal.1.notify_all();
        }
        if !enabled && let Ok(mut snap) = self.snapshot.lock() {
            *snap = UpdateStatusSnapshot::default();
            self.dirty.store(true, Ordering::Release);
        }
    }
}

/// Sanitizes package/version tokens from external command output (`ASCII` printable only).
#[must_use]
pub fn sanitize_update_token(raw: &str, max_len: usize) -> String {
    let mut out = String::new();
    let mut in_csi = false;
    for ch in raw.chars() {
        if in_csi {
            if ch.is_ascii_alphabetic() {
                in_csi = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_csi = true;
            continue;
        }
        if ch.is_ascii() && !ch.is_ascii_control() && !ch.is_whitespace() {
            out.push(ch);
            if out.chars().count() >= max_len {
                break;
            }
        }
    }
    out
}

/// Returns `true` when `pkg_name` is a core RustyOS distribution package.
#[must_use]
pub fn is_rustyos_package(pkg_name: &str) -> bool {
    pkg_name == "rustyos" || pkg_name == "rustyos-git" || pkg_name.starts_with("rustyos-")
}

/// Returns `true` when updating `pkg_name` should prompt a reboot or session restart.
#[must_use]
pub fn package_requires_restart(pkg_name: &str) -> bool {
    pkg_name == "linux"
        || pkg_name.starts_with("linux-")
        || pkg_name == "systemd"
        || pkg_name.starts_with("systemd-")
        || pkg_name == "mesa"
        || pkg_name.starts_with("nvidia")
        || pkg_name == "amd-ucode"
        || pkg_name == "intel-ucode"
        || pkg_name == "wayland"
        || is_rustyos_package(pkg_name)
}

/// Parses `checkupdates --nocolor` output lines (`<pkg> <old> -> <new>`) deterministically.
#[must_use]
pub fn parse_checkupdates_output(stdout: &str) -> Vec<PackageUpdateItem> {
    let mut items = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((lhs, rhs)) = trimmed.split_once("->") else {
            continue;
        };
        let mut lhs_parts = lhs.split_whitespace();
        let Some(raw_name) = lhs_parts.next() else {
            continue;
        };
        let Some(raw_old) = lhs_parts.next() else {
            continue;
        };
        if lhs_parts.next().is_some() {
            continue;
        }
        let mut rhs_parts = rhs.split_whitespace();
        let Some(raw_new) = rhs_parts.next() else {
            continue;
        };
        if rhs_parts.next().is_some() {
            continue;
        }
        let name = sanitize_update_token(raw_name, 64);
        let old_version = sanitize_update_token(raw_old, 48);
        let new_version = sanitize_update_token(raw_new, 48);
        if name.is_empty() || old_version.is_empty() || new_version.is_empty() {
            continue;
        }
        let is_rustyos = is_rustyos_package(&name);
        let requires_restart = package_requires_restart(&name);
        items.push(PackageUpdateItem {
            name,
            old_version,
            new_version,
            is_rustyos,
            requires_restart,
        });
    }
    items.sort_by(|a, b| {
        b.is_rustyos
            .cmp(&a.is_rustyos)
            .then_with(|| b.requires_restart.cmp(&a.requires_restart))
            .then_with(|| a.name.cmp(&b.name))
    });
    items
}

fn run_worker_loop(
    snapshot: &Arc<Mutex<UpdateStatusSnapshot>>,
    dirty: &Arc<AtomicBool>,
    signal: &Arc<(Mutex<SharedWorkerConfig>, Condvar)>,
) {
    let mut next_wait = INITIAL_BOOT_DELAY;
    loop {
        let should_run = {
            let Ok(mut guard) = signal.0.lock() else {
                return;
            };
            if !guard.stop && !guard.force_check {
                let wait_dur = if guard.enabled {
                    next_wait
                } else {
                    Duration::from_secs(3600)
                };
                if let Ok((res, _)) = signal.1.wait_timeout(guard, wait_dur) {
                    guard = res;
                } else {
                    return;
                }
            }
            if guard.stop {
                return;
            }
            let forced = std::mem::take(&mut guard.force_check);
            let run = forced || guard.enabled;
            next_wait = guard.interval;
            run
        };

        if !should_run {
            continue;
        }

        if let Ok(mut snap) = snapshot.lock() {
            snap.state = UpdateCheckState::Checking;
            snap.error_hint = None;
            dirty.store(true, Ordering::Release);
        }

        let next_snap = execute_checkupdates_probe();
        if let Ok(mut snap) = snapshot.lock() {
            *snap = next_snap;
            dirty.store(true, Ordering::Release);
        }
    }
}

fn build_snapshot_from_items(items: Vec<PackageUpdateItem>) -> UpdateStatusSnapshot {
    let total_count = items.len();
    let rustyos_available = items.iter().any(|item| item.is_rustyos);
    let arch_count = items.iter().filter(|item| !item.is_rustyos).count();
    let restart_recommended = items.iter().any(|item| item.requires_restart);
    let stored: Vec<PackageUpdateItem> = items.into_iter().take(MAX_STORED_ITEMS).collect();
    let clock = local_clock_hhmm();
    let state = if total_count > 0 {
        UpdateCheckState::Available
    } else {
        UpdateCheckState::UpToDate
    };
    UpdateStatusSnapshot {
        state,
        total_count,
        arch_count,
        rustyos_available,
        restart_recommended,
        last_checked_label: format!("Today {clock}"),
        error_hint: None,
        items: stored,
    }
}

fn execute_checkupdates_probe() -> UpdateStatusSnapshot {
    let bin = if std::path::Path::new("/usr/bin/checkupdates").is_file() {
        "/usr/bin/checkupdates"
    } else {
        "checkupdates"
    };
    let Ok(mut child) = Command::new(bin)
        .arg("--nocolor")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return UpdateStatusSnapshot {
            state: UpdateCheckState::Unavailable,
            last_checked_label: format!("Today {}", local_clock_hhmm()),
            error_hint: Some("Install 'pacman-contrib' (checkupdates)".to_owned()),
            ..UpdateStatusSnapshot::default()
        };
    };

    let deadline = Instant::now() + CHECK_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut buf = Vec::new();
                if let Some(out) = child.stdout.take() {
                    let _ = out.take(MAX_STDOUT_BYTES as u64).read_to_end(&mut buf);
                }
                let code = status.code().unwrap_or(-1);
                if code == 0 || code == 2 {
                    let parsed = parse_checkupdates_output(&String::from_utf8_lossy(&buf));
                    return build_snapshot_from_items(parsed);
                }
                return UpdateStatusSnapshot {
                    state: UpdateCheckState::Unavailable,
                    last_checked_label: format!("Today {}", local_clock_hhmm()),
                    error_hint: Some(format!("checkupdates exited with code {code}")),
                    ..UpdateStatusSnapshot::default()
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return UpdateStatusSnapshot {
                        state: UpdateCheckState::Unavailable,
                        last_checked_label: format!("Today {}", local_clock_hhmm()),
                        error_hint: Some("Update check timed out".to_owned()),
                        ..UpdateStatusSnapshot::default()
                    };
                }
                thread::sleep(Duration::from_millis(80));
            }
            Err(err) => {
                let _ = child.kill();
                return UpdateStatusSnapshot {
                    state: UpdateCheckState::Unavailable,
                    last_checked_label: format!("Today {}", local_clock_hhmm()),
                    error_hint: Some(format!("checkupdates wait error: {err}")),
                    ..UpdateStatusSnapshot::default()
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arch_and_rustyos_updates_and_prioritizes_critical_packages() {
        let sample = "\
            git 2.47.0-1 -> 2.47.1-1\n\
            \x1b[31mlinux\x1b[0m 6.12.1.arch1-1 -> 6.12.2.arch1-1\n\
            mesa 1:24.2.7-1 -> 1:24.3.0-1\n\
            rustyos-git 0.1.0.r74-1 -> 0.1.0.r76-1\n\
            malformed line without arrow\n\
            bad pkg extra 1.0 -> 2.0\n";
        let items = parse_checkupdates_output(sample);
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].name, "rustyos-git");
        assert!(items[0].is_rustyos && items[0].requires_restart);
        assert_eq!(items[1].name, "linux");
        assert!(items[1].requires_restart && !items[1].is_rustyos);
        assert_eq!(items[2].name, "mesa");
        assert!(items[2].requires_restart && !items[2].is_rustyos);
        assert_eq!(items[3].name, "git");
        assert!(!items[3].requires_restart);

        let snap = build_snapshot_from_items(items);
        assert_eq!(snap.state, UpdateCheckState::Available);
        assert_eq!(snap.total_count, 4);
        assert_eq!(snap.arch_count, 3);
        assert!(snap.rustyos_available);
        assert!(snap.restart_recommended);
    }

    #[test]
    fn empty_checkupdates_output_marks_up_to_date() {
        let items = parse_checkupdates_output("");
        let snap = build_snapshot_from_items(items);
        assert_eq!(snap.state, UpdateCheckState::UpToDate);
        assert_eq!(snap.total_count, 0);
        assert!(!snap.rustyos_available);
        assert!(!snap.restart_recommended);
    }

    #[test]
    fn noop_handle_never_panics_when_reconfigured_or_triggered() {
        let handle = UpdateCheckerHandle::default();
        assert_eq!(handle.snapshot().state, UpdateCheckState::Idle);
        handle.trigger_check_now();
        handle.reconfigure(false, 60);
        assert!(handle.take_dirty());
        assert!(!handle.take_dirty());
    }
}
