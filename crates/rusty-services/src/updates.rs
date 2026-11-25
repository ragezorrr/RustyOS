//! Unprivileged Arch Linux and RustyOS update checker (`RUSTYOS-076`, ADR-0036).
//!
//! Runs `checkupdates --nocolor` (`pacman-contrib`) in a background worker thread so
//! the compositor event loop never blocks on network/disk I/O and `/var/lib/pacman`
//! is never mutated outside explicit privileged `pacman -Syu` execution.

use std::fmt;
use std::io::{BufRead as _, BufReader};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
        if let Ok(guard) = self.snapshot.lock() {
            guard.clone()
        } else {
            UpdateStatusSnapshot::default()
        }
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

fn build_snapshot_from_items_with_limit(
    items: Vec<PackageUpdateItem>,
    max_items: Option<usize>,
) -> UpdateStatusSnapshot {
    let total_count = items.len();
    let rustyos_available = items.iter().any(|item| item.is_rustyos);
    let arch_count = items.iter().filter(|item| !item.is_rustyos).count();
    let restart_recommended = items.iter().any(|item| item.requires_restart);
    let stored: Vec<PackageUpdateItem> = match max_items {
        Some(limit) => items.into_iter().take(limit).collect(),
        None => items,
    };
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

#[cfg(test)]
fn build_snapshot_from_items(items: Vec<PackageUpdateItem>) -> UpdateStatusSnapshot {
    build_snapshot_from_items_with_limit(items, Some(MAX_STORED_ITEMS))
}

/// Runs `checkupdates --nocolor` synchronously and returns all parsed updates without `TopBar` truncation (`RUSTYOS-077`).
#[must_use]
pub fn check_all_pending_updates() -> UpdateStatusSnapshot {
    execute_checkupdates_probe_with_limit(None)
}

fn execute_checkupdates_probe() -> UpdateStatusSnapshot {
    execute_checkupdates_probe_with_limit(Some(MAX_STORED_ITEMS))
}

fn execute_checkupdates_probe_with_limit(max_items: Option<usize>) -> UpdateStatusSnapshot {
    let bin = if Path::new("/usr/bin/checkupdates").is_file() {
        "/usr/bin/checkupdates"
    } else {
        "checkupdates"
    };
    let mut cmd = Command::new(bin);
    cmd.arg("--nocolor");
    match crate::run_command_bounded(&mut cmd, CHECK_TIMEOUT) {
        Ok(output) => {
            let code = output.status.code().unwrap_or(-1);
            if code == 0 || code == 2 {
                let end = output.stdout.len().min(MAX_STDOUT_BYTES);
                let parsed =
                    parse_checkupdates_output(&String::from_utf8_lossy(&output.stdout[..end]));
                build_snapshot_from_items_with_limit(parsed, max_items)
            } else {
                UpdateStatusSnapshot {
                    state: UpdateCheckState::Unavailable,
                    last_checked_label: format!("Today {}", local_clock_hhmm()),
                    error_hint: Some(format!("checkupdates exited with code {code}")),
                    ..UpdateStatusSnapshot::default()
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::TimedOut => UpdateStatusSnapshot {
            state: UpdateCheckState::Unavailable,
            last_checked_label: format!("Today {}", local_clock_hhmm()),
            error_hint: Some("Update check timed out".to_owned()),
            ..UpdateStatusSnapshot::default()
        },
        Err(_) => UpdateStatusSnapshot {
            state: UpdateCheckState::Unavailable,
            last_checked_label: format!("Today {}", local_clock_hhmm()),
            error_hint: Some("Install 'pacman-contrib' (checkupdates)".to_owned()),
            ..UpdateStatusSnapshot::default()
        },
    }
}

/// Host distribution, kernel, and `rustyos-git` packaging summary shown in `rusty-updater` (`RUSTYOS-077`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemReleaseSummary {
    /// Display name from `/etc/os-release` (`"Arch Linux"`, etc.).
    pub distro_name: String,
    /// Running kernel release string (`uname -r`).
    pub kernel_release: String,
    /// Installed `rustyos` or `rustyos-git` package version if managed by `pacman`.
    pub rustyos_installed_version: Option<String>,
    /// Resolved path to `rustyos-update-helper` if installed on the system.
    pub helper_path: Option<PathBuf>,
    /// Local repository `packaging/arch` folder if launched from a git checkout.
    pub repo_pkgbuild_dir: Option<PathBuf>,
    /// Whether `[rustyos]` is configured in `/etc/pacman.conf` (`RUSTYOS-079`).
    pub pacman_repo_configured: bool,
}

/// Returns standard Polkit-authorized installation path for `rustyos-update-helper`.
#[must_use]
pub fn helper_install_path() -> PathBuf {
    PathBuf::from("/usr/lib/rustyos/rustyos-update-helper")
}

/// Resolves an installed `rustyos-update-helper` binary if available.
#[must_use]
pub fn resolve_update_helper_path() -> Option<PathBuf> {
    let candidates = [
        helper_install_path(),
        PathBuf::from("/usr/local/lib/rustyos/rustyos-update-helper"),
    ];
    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// Discovers local `packaging/arch/PKGBUILD` directory if running inside a RustyOS source checkout.
#[must_use]
pub fn detect_local_pkgbuild_dir() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        let dir = cwd.join("packaging/arch");
        if dir.join("PKGBUILD").is_file() {
            return Some(dir);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().skip(1).take(4) {
            let dir = ancestor.join("packaging/arch");
            if dir.join("PKGBUILD").is_file() {
                return Some(dir);
            }
        }
    }
    None
}

fn query_pacman_pkg_version(pkg: &str) -> Option<String> {
    let mut cmd = Command::new("/usr/bin/pacman");
    cmd.args(["-Q", pkg]);
    let out = crate::run_command_bounded(&mut cmd, Duration::from_secs(5)).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (_, ver) = text.trim().split_once(' ')?;
    let clean = sanitize_update_token(ver, 48);
    (!clean.is_empty()).then_some(clean)
}

/// Reads host Arch OS metadata, kernel release, helper presence, and optional local `PKGBUILD` path (`RUSTYOS-077`).
#[must_use]
pub fn read_system_release_summary() -> SystemReleaseSummary {
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let distro_name = parse_os_release_pretty_name(&os_release);
    let kernel_release = match std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        Ok(s) => s.trim().to_owned(),
        Err(_) => String::new(),
    };
    let rustyos_installed_version =
        query_pacman_pkg_version("rustyos-git").or_else(|| query_pacman_pkg_version("rustyos"));
    SystemReleaseSummary {
        distro_name,
        kernel_release,
        rustyos_installed_version,
        helper_path: resolve_update_helper_path(),
        repo_pkgbuild_dir: detect_local_pkgbuild_dir(),
        pacman_repo_configured: is_pacman_repo_configured(),
    }
}

/// Checks whether a pacman configuration string contains a given repository section (`RUSTYOS-079`).
#[must_use]
pub fn parse_pacman_conf_has_repo(contents: &str, repo_name: &str) -> bool {
    contents.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            return false;
        }
        trimmed
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .is_some_and(|name| name.trim().eq_ignore_ascii_case(repo_name))
    })
}

/// Reads host `/etc/pacman.conf` and checks if `[rustyos]` is present (`RUSTYOS-079`).
#[must_use]
pub fn is_pacman_repo_configured() -> bool {
    std::fs::read_to_string("/etc/pacman.conf")
        .is_ok_and(|conf| parse_pacman_conf_has_repo(&conf, "rustyos"))
}

fn parse_os_release_pretty_name(contents: &str) -> String {
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
            let trimmed = rest.trim().trim_matches('"').trim_matches('\'');
            let clean = strip_terminal_sequences(trimmed, 48);
            if !clean.is_empty() {
                return clean;
            }
        }
    }
    "Arch Linux".to_owned()
}

/// Strict validated command accepted by the Polkit-authorized `rustyos-update-helper` (`RUSTYOS-077`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperCommand {
    /// Run full system upgrade (`pacman -Syu --noconfirm --color never`).
    SysUpgrade,
    /// Install a verified local Arch `.pkg.tar.*` package archive via `pacman -U`.
    InstallLocalPackage(PathBuf),
}

/// Validates and parses CLI arguments passed to `rustyos-update-helper` (never invokes a shell).
///
/// # Errors
/// Returns an explanatory error string on unexpected arguments, relative paths, or invalid suffixes.
pub fn validate_helper_args(args: &[String]) -> Result<HelperCommand, String> {
    match args {
        [cmd] if cmd == "sysupgrade" => Ok(HelperCommand::SysUpgrade),
        [cmd, raw_path] if cmd == "install-pkg" => validate_package_archive_path(raw_path),
        _ => Err(
            "usage: rustyos-update-helper <sysupgrade | install-pkg /abs/path.pkg.tar.zst>"
                .to_owned(),
        ),
    }
}

fn validate_package_archive_path(raw_path: &str) -> Result<HelperCommand, String> {
    if raw_path.is_empty() || raw_path.len() > 512 || raw_path.chars().any(char::is_control) {
        return Err("invalid package path characters".to_owned());
    }
    let path = Path::new(raw_path);
    if !path.is_absolute() {
        return Err("package path must be absolute".to_owned());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("package path cannot contain '..'".to_owned());
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let valid_ext = name.ends_with(".pkg.tar.zst")
        || name.ends_with(".pkg.tar.xz")
        || name.ends_with(".pkg.tar.gz")
        || name.ends_with(".pkg.tar");
    if !valid_ext {
        return Err("package archive must end in .pkg.tar.zst/.xz/.gz/.tar".to_owned());
    }
    Ok(HelperCommand::InstallLocalPackage(path.to_path_buf()))
}

/// Executes validated helper command in `rustyos-update-helper` using `/usr/bin/pacman` directly (`RUSTYOS-077`).
///
/// # Errors
/// Returns descriptive error text when validation fails, the file is missing, or `pacman` fails.
pub fn run_helper_cli(args: &[String]) -> Result<(), String> {
    let parsed = validate_helper_args(args)?;
    let pacman_bin = if Path::new("/usr/bin/pacman").is_file() {
        "/usr/bin/pacman"
    } else {
        "pacman"
    };
    let mut cmd = Command::new(pacman_bin);
    cmd.stdin(Stdio::null());
    match &parsed {
        HelperCommand::SysUpgrade => {
            cmd.args(["-Syu", "--noconfirm", "--color", "never"]);
        }
        HelperCommand::InstallLocalPackage(path) => {
            if !path.is_file() {
                return Err(format!("package file not found: {}", path.display()));
            }
            cmd.args(["-U", "--noconfirm", "--color", "never", "--"])
                .arg(path);
        }
    }
    let status = cmd
        .status()
        .map_err(|err| format!("failed to launch pacman: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "pacman exited with status {}",
            status.code().unwrap_or(1)
        ))
    }
}

/// Strips ANSI CSI/OSC sequences and control characters while keeping readable spaces for GUI log viewers.
#[must_use]
pub fn strip_terminal_sequences(raw: &str, max_len: usize) -> String {
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
        if ch == '\t' {
            out.push(' ');
        } else if !ch.is_control() {
            out.push(ch);
        }
        if out.chars().count() >= max_len {
            break;
        }
    }
    out.trim_end().to_owned()
}

/// Live transaction phases reported by [`UpdateTransactionHandle`] (`RUSTYOS-077`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateTxPhase {
    /// Ready, idle.
    Idle,
    /// Waiting for Polkit administrator password prompt (`pkexec`).
    Authenticating,
    /// Building local `rustyos-git` package unprivileged (`makepkg`).
    BuildingPackage,
    /// Synchronizing Arch package databases (`pacman -Syu`).
    Synchronizing,
    /// Downloading updated packages.
    Downloading,
    /// Verifying PGP signatures and package integrity.
    CheckingIntegrity,
    /// Installing/upgrading packages on disk.
    Installing,
    /// Transaction finished cleanly.
    Complete,
    /// Transaction failed with non-zero status.
    Failed,
    /// Cancelled before completion by user request.
    Cancelled,
}

impl UpdateTxPhase {
    /// Human-readable status string rendered above the progress bar.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::Authenticating => "Authorizing administrator access...",
            Self::BuildingPackage => "Building RustyOS package (makepkg)...",
            Self::Synchronizing => "Synchronizing package databases...",
            Self::Downloading => "Downloading updates...",
            Self::CheckingIntegrity => "Verifying packages...",
            Self::Installing => "Installing updates...",
            Self::Complete => "Updates installed",
            Self::Failed => "Update failed",
            Self::Cancelled => "Update cancelled",
        }
    }
}

/// Incremental update event emitted from [`UpdateTransactionHandle`].
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateTxEvent {
    /// Phase transition.
    Phase(UpdateTxPhase),
    /// Normalized overall progress fraction (`0.0..=1.0`).
    Progress(f32),
    /// Sanitized terminal output line.
    LogLine(String),
    /// Final completion marker.
    Finished {
        /// `true` when upgrade exited with code 0.
        success: bool,
        /// `true` when updated packages warrant reboot or session restart.
        restart_recommended: bool,
        /// Short summary displayed in hero card and footer.
        summary: String,
    },
}

/// Extracts phase and normalized `0.0..=1.0` progress from a sanitized `pacman` log line.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn parse_pacman_progress_line(line: &str) -> Option<(UpdateTxPhase, Option<f32>)> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("synchronizing package databases") {
        return Some((UpdateTxPhase::Synchronizing, Some(0.12)));
    }
    if lower.contains("retrieving packages") {
        return Some((UpdateTxPhase::Downloading, Some(0.28)));
    }
    if lower.contains("checking keys")
        || lower.contains("checking package integrity")
        || lower.contains("checking available disk space")
    {
        return Some((UpdateTxPhase::CheckingIntegrity, Some(0.54)));
    }
    if lower.contains("running post-transaction hooks") {
        return Some((UpdateTxPhase::Installing, Some(0.95)));
    }
    if let Some((done, total)) = parse_fraction_tag(line)
        && total > 0
    {
        let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
        if lower.contains("upgrading ")
            || lower.contains("installing ")
            || lower.contains("reinstalling ")
        {
            return Some((UpdateTxPhase::Installing, Some(0.60 + ratio * 0.34)));
        }
        return Some((UpdateTxPhase::Downloading, Some(0.25 + ratio * 0.25)));
    }
    None
}

fn parse_fraction_tag(line: &str) -> Option<(u32, u32)> {
    let trimmed = line.trim_start();
    let after_open = trimmed.strip_prefix('(')?;
    let inside = after_open.split(')').next()?;
    let (lhs, rhs) = inside.split_once('/')?;
    let a = lhs.trim().parse::<u32>().ok()?;
    let b = rhs.trim().parse::<u32>().ok()?;
    Some((a, b))
}

/// Non-blocking streaming update transaction handle for `apps/rusty-updater` (`RUSTYOS-077`).
pub struct UpdateTransactionHandle {
    events: Arc<Mutex<Vec<UpdateTxEvent>>>,
    cancelled: Arc<AtomicBool>,
    child_pid: Arc<Mutex<Option<u32>>>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for UpdateTransactionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateTransactionHandle")
            .field("active", &self.worker.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for UpdateTransactionHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl UpdateTransactionHandle {
    /// Starts `pkexec <helper> sysupgrade` (or clean `--dry-run` simulation) in a background thread.
    #[must_use]
    pub fn start_system_upgrade(
        dry_run: bool,
        helper_override: Option<PathBuf>,
        restart_recommended: bool,
        preview_items: Vec<PackageUpdateItem>,
    ) -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let child_pid = Arc::new(Mutex::new(None));

        let ev_ref = Arc::clone(&events);
        let cancel_ref = Arc::clone(&cancelled);
        let pid_ref = Arc::clone(&child_pid);
        let worker = thread::Builder::new()
            .name("rusty-upgrade-tx".to_owned())
            .spawn(move || {
                if dry_run {
                    run_dry_run_upgrade(&ev_ref, &cancel_ref, restart_recommended, &preview_items);
                } else {
                    run_privileged_helper(
                        helper_override,
                        &["sysupgrade".to_owned()],
                        restart_recommended,
                        &ev_ref,
                        &cancel_ref,
                        &pid_ref,
                    );
                }
            })
            .ok();

        Self {
            events,
            cancelled,
            child_pid,
            worker,
        }
    }

    /// Rebuilds `rustyos-git` unprivileged via `makepkg` and installs the built `.pkg.tar.*` via `pkexec`.
    #[must_use]
    pub fn start_rustyos_local_upgrade(
        dry_run: bool,
        pkgbuild_dir: PathBuf,
        helper_override: Option<PathBuf>,
    ) -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let child_pid = Arc::new(Mutex::new(None));

        let ev_ref = Arc::clone(&events);
        let cancel_ref = Arc::clone(&cancelled);
        let pid_ref = Arc::clone(&child_pid);
        let worker = thread::Builder::new()
            .name("rusty-local-build-tx".to_owned())
            .spawn(move || {
                if dry_run {
                    let preview = vec![PackageUpdateItem {
                        name: "rustyos-git".to_owned(),
                        old_version: "local".to_owned(),
                        new_version: "HEAD".to_owned(),
                        is_rustyos: true,
                        requires_restart: true,
                    }];
                    run_dry_run_upgrade(&ev_ref, &cancel_ref, true, &preview);
                } else {
                    run_local_makepkg_and_install(
                        &pkgbuild_dir,
                        helper_override,
                        &ev_ref,
                        &cancel_ref,
                        &pid_ref,
                    );
                }
            })
            .ok();

        Self {
            events,
            cancelled,
            child_pid,
            worker,
        }
    }

    /// Drains all queued progress/log events without blocking the UI loop.
    #[must_use]
    pub fn drain_events(&self) -> Vec<UpdateTxEvent> {
        if let Ok(mut guard) = self.events.lock() {
            std::mem::take(&mut *guard)
        } else {
            Vec::new()
        }
    }

    /// Signals cancellation and terminates an unprivileged child process if active.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(guard) = self.child_pid.lock()
            && let Some(pid) = *guard
        {
            let mut cmd = Command::new("kill");
            cmd.args(["-TERM", &pid.to_string()]);
            let _ = crate::run_command_bounded(&mut cmd, Duration::from_secs(3));
        }
    }
}

fn push_tx_event(events: &Arc<Mutex<Vec<UpdateTxEvent>>>, ev: UpdateTxEvent) {
    if let Ok(mut guard) = events.lock() {
        guard.push(ev);
    }
}

fn run_dry_run_upgrade(
    events: &Arc<Mutex<Vec<UpdateTxEvent>>>,
    cancelled: &Arc<AtomicBool>,
    restart_recommended: bool,
    items: &[PackageUpdateItem],
) {
    let steps = [
        (
            UpdateTxPhase::Authenticating,
            0.08,
            "[dry-run] Polkit auth ready (pkexec /usr/lib/rustyos/rustyos-update-helper)",
        ),
        (
            UpdateTxPhase::Synchronizing,
            0.20,
            ":: Synchronizing package databases (dry-run simulation)...",
        ),
        (
            UpdateTxPhase::Downloading,
            0.42,
            ":: Retrieving packages (no files written)...",
        ),
        (
            UpdateTxPhase::CheckingIntegrity,
            0.58,
            ":: Checking package integrity...",
        ),
    ];
    for (phase, prog, msg) in steps {
        if cancelled.load(Ordering::Acquire) {
            push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Cancelled));
            push_tx_event(
                events,
                UpdateTxEvent::Finished {
                    success: false,
                    restart_recommended: false,
                    summary: "Update simulation cancelled".to_owned(),
                },
            );
            return;
        }
        push_tx_event(events, UpdateTxEvent::Phase(phase));
        push_tx_event(events, UpdateTxEvent::Progress(prog));
        push_tx_event(events, UpdateTxEvent::LogLine(msg.to_owned()));
        thread::sleep(Duration::from_millis(160));
    }

    push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Installing));
    let sample_len = items.len().clamp(1, 6);
    for (idx, item) in items.iter().take(sample_len).enumerate() {
        if cancelled.load(Ordering::Acquire) {
            push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Cancelled));
            push_tx_event(
                events,
                UpdateTxEvent::Finished {
                    success: false,
                    restart_recommended: false,
                    summary: "Update simulation cancelled".to_owned(),
                },
            );
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let prog = 0.65 + ((idx + 1) as f32 / sample_len as f32) * 0.30;
        push_tx_event(events, UpdateTxEvent::Progress(prog));
        push_tx_event(
            events,
            UpdateTxEvent::LogLine(format!(
                "({}/{sample_len}) [dry-run] upgrading {} ({} -> {})",
                idx + 1,
                item.name,
                item.old_version,
                item.new_version
            )),
        );
        thread::sleep(Duration::from_millis(130));
    }
    push_tx_event(events, UpdateTxEvent::Progress(1.0));
    push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Complete));
    push_tx_event(
        events,
        UpdateTxEvent::Finished {
            success: true,
            restart_recommended,
            summary: format!("Dry-run verified ({} package(s) simulated)", items.len()),
        },
    );
}

fn spawn_and_stream_command(
    program: &Path,
    args: &[String],
    cwd: Option<&Path>,
    envs: &[(&str, &str)],
    events: &Arc<Mutex<Vec<UpdateTxEvent>>>,
    cancelled: &Arc<AtomicBool>,
    child_pid: &Arc<Mutex<Option<u32>>>,
) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Failed to spawn {}: {err}", program.display()))?;
    if let Ok(mut pid_guard) = child_pid.lock() {
        *pid_guard = Some(child.id());
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_handle = stdout.map(|s| spawn_line_reader(s, Arc::clone(events)));
    let err_handle = stderr.map(|s| spawn_line_reader(s, Arc::clone(events)));

    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Ok(st),
            Ok(None) => {
                if cancelled.load(Ordering::Acquire) {
                    let _ = child.kill();
                }
                thread::sleep(Duration::from_millis(40));
            }
            Err(err) => break Err(format!("Wait error: {err}")),
        }
    };
    if let Ok(mut pid_guard) = child_pid.lock() {
        *pid_guard = None;
    }
    if let Some(h) = out_handle {
        let _ = h.join();
    }
    if let Some(h) = err_handle {
        let _ = h.join();
    }

    let st = status?;
    if cancelled.load(Ordering::Acquire) {
        return Err("Cancelled by user".to_owned());
    }
    if st.success() {
        Ok(())
    } else {
        Err(format!(
            "Command exited with status {}",
            st.code().unwrap_or(1)
        ))
    }
}

fn spawn_line_reader<R: std::io::Read + Send + 'static>(
    stream: R,
    events: Arc<Mutex<Vec<UpdateTxEvent>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines().map_while(Result::ok) {
            let clean = strip_terminal_sequences(&line, 240);
            if clean.is_empty() {
                continue;
            }
            if let Some((phase, maybe_prog)) = parse_pacman_progress_line(&clean) {
                push_tx_event(&events, UpdateTxEvent::Phase(phase));
                if let Some(prog) = maybe_prog {
                    push_tx_event(&events, UpdateTxEvent::Progress(prog));
                }
            }
            push_tx_event(&events, UpdateTxEvent::LogLine(clean));
        }
    })
}

fn run_privileged_helper(
    helper_override: Option<PathBuf>,
    helper_args: &[String],
    restart_recommended: bool,
    events: &Arc<Mutex<Vec<UpdateTxEvent>>>,
    cancelled: &Arc<AtomicBool>,
    child_pid: &Arc<Mutex<Option<u32>>>,
) {
    let Some(helper_path) = helper_override.or_else(resolve_update_helper_path) else {
        push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Failed));
        push_tx_event(
            events,
            UpdateTxEvent::LogLine(
                "Missing /usr/lib/rustyos/rustyos-update-helper (install rustyos package or run install.sh --system)".to_owned(),
            ),
        );
        push_tx_event(
            events,
            UpdateTxEvent::Finished {
                success: false,
                restart_recommended: false,
                summary: "Update helper not installed on system".to_owned(),
            },
        );
        return;
    };

    push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Authenticating));
    push_tx_event(events, UpdateTxEvent::Progress(0.05));
    push_tx_event(
        events,
        UpdateTxEvent::LogLine(format!(
            "Requesting Polkit authorization via pkexec {} {}",
            helper_path.display(),
            helper_args.join(" ")
        )),
    );

    let mut full_args = vec![helper_path.to_string_lossy().into_owned()];
    full_args.extend(helper_args.iter().cloned());
    let pkexec = Path::new("/usr/bin/pkexec");
    match spawn_and_stream_command(
        pkexec,
        &full_args,
        None,
        &[("TERM", "dumb"), ("LC_ALL", "C")],
        events,
        cancelled,
        child_pid,
    ) {
        Ok(()) => {
            push_tx_event(events, UpdateTxEvent::Progress(1.0));
            push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Complete));
            push_tx_event(
                events,
                UpdateTxEvent::Finished {
                    success: true,
                    restart_recommended,
                    summary: "System updates applied cleanly".to_owned(),
                },
            );
        }
        Err(msg) => {
            let phase = if cancelled.load(Ordering::Acquire) {
                UpdateTxPhase::Cancelled
            } else {
                UpdateTxPhase::Failed
            };
            push_tx_event(events, UpdateTxEvent::Phase(phase));
            push_tx_event(events, UpdateTxEvent::LogLine(msg.clone()));
            push_tx_event(
                events,
                UpdateTxEvent::Finished {
                    success: false,
                    restart_recommended: false,
                    summary: msg,
                },
            );
        }
    }
}

fn run_local_makepkg_and_install(
    pkgbuild_dir: &Path,
    helper_override: Option<PathBuf>,
    events: &Arc<Mutex<Vec<UpdateTxEvent>>>,
    cancelled: &Arc<AtomicBool>,
    child_pid: &Arc<Mutex<Option<u32>>>,
) {
    let pkgdest = std::env::temp_dir().join(format!("rustyos-makepkg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&pkgdest);
    if let Err(err) = std::fs::create_dir_all(&pkgdest) {
        push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Failed));
        push_tx_event(
            events,
            UpdateTxEvent::Finished {
                success: false,
                restart_recommended: false,
                summary: format!("Failed to create staging dir: {err}"),
            },
        );
        return;
    }

    push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::BuildingPackage));
    push_tx_event(events, UpdateTxEvent::Progress(0.15));
    push_tx_event(
        events,
        UpdateTxEvent::LogLine(format!(
            "Building unprivileged Arch package from {}",
            pkgbuild_dir.display()
        )),
    );

    let pkgdest_str = pkgdest.to_string_lossy().into_owned();
    let makepkg_res = spawn_and_stream_command(
        Path::new("/usr/bin/makepkg"),
        &["-f".to_owned(), "--noconfirm".to_owned()],
        Some(pkgbuild_dir),
        &[("PKGDEST", &pkgdest_str), ("TERM", "dumb"), ("LC_ALL", "C")],
        events,
        cancelled,
        child_pid,
    );
    if let Err(err) = makepkg_res {
        let _ = std::fs::remove_dir_all(&pkgdest);
        push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Failed));
        push_tx_event(events, UpdateTxEvent::LogLine(err.clone()));
        push_tx_event(
            events,
            UpdateTxEvent::Finished {
                success: false,
                restart_recommended: false,
                summary: err,
            },
        );
        return;
    }

    let built_pkg = std::fs::read_dir(&pkgdest).ok().and_then(|entries| {
        entries.flatten().map(|e| e.path()).find(|p| {
            let s = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            s.contains(".pkg.tar") && !s.contains("-debug-")
        })
    });
    let Some(pkg_path) = built_pkg else {
        let _ = std::fs::remove_dir_all(&pkgdest);
        push_tx_event(events, UpdateTxEvent::Phase(UpdateTxPhase::Failed));
        push_tx_event(
            events,
            UpdateTxEvent::Finished {
                success: false,
                restart_recommended: false,
                summary: "makepkg produced no .pkg.tar archive".to_owned(),
            },
        );
        return;
    };

    run_privileged_helper(
        helper_override,
        &[
            "install-pkg".to_owned(),
            pkg_path.to_string_lossy().into_owned(),
        ],
        true,
        events,
        cancelled,
        child_pid,
    );
    let _ = std::fs::remove_dir_all(&pkgdest);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_helper_args_and_blocks_path_traversal() {
        assert_eq!(
            validate_helper_args(&["sysupgrade".to_owned()]),
            Ok(HelperCommand::SysUpgrade)
        );
        assert_eq!(
            validate_helper_args(&[
                "install-pkg".to_owned(),
                "/tmp/rustyos-git-0.1.0-1-x86_64.pkg.tar.zst".to_owned()
            ]),
            Ok(HelperCommand::InstallLocalPackage(PathBuf::from(
                "/tmp/rustyos-git-0.1.0-1-x86_64.pkg.tar.zst"
            )))
        );
        assert!(validate_helper_args(&[]).is_err());
        assert!(validate_helper_args(&["sysupgrade".to_owned(), "--force".to_owned()]).is_err());
        assert!(
            validate_helper_args(&[
                "install-pkg".to_owned(),
                "relative/pkg.pkg.tar.zst".to_owned()
            ])
            .is_err()
        );
        assert!(
            validate_helper_args(&[
                "install-pkg".to_owned(),
                "/tmp/../etc/passwd.pkg.tar.zst".to_owned()
            ])
            .is_err()
        );
        assert!(
            validate_helper_args(&["install-pkg".to_owned(), "/tmp/evil.sh".to_owned()]).is_err()
        );
    }

    #[test]
    fn parses_pacman_progress_and_os_release_lines() {
        assert_eq!(
            parse_pacman_progress_line(":: Synchronizing package databases..."),
            Some((UpdateTxPhase::Synchronizing, Some(0.12)))
        );
        assert_eq!(
            parse_pacman_progress_line("( 12/24) upgrading mesa [####----]"),
            Some((UpdateTxPhase::Installing, Some(0.60 + 0.5 * 0.34)))
        );
        assert_eq!(
            strip_terminal_sequences("\x1b[32m:: Installing\x1b[0m pkg\x07", 80),
            ":: Installing pkg"
        );
        assert_eq!(
            parse_os_release_pretty_name("NAME=\"Arch\"\nPRETTY_NAME=\"Arch Linux\"\n"),
            "Arch Linux"
        );
    }

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

    #[test]
    fn parses_pacman_conf_repo_detection() {
        let conf_without = "\
            [options]\nHoldPkg = pacman glibc\n\
            # [rustyos]\n# Server = https://example.com\n\
            [core]\nInclude = /etc/pacman.d/mirrorlist\n";
        assert!(!parse_pacman_conf_has_repo(conf_without, "rustyos"));

        let conf_with = "\
            [options]\nHoldPkg = pacman glibc\n\
            [rustyos]\nSigLevel = Optional TrustAll\nServer = https://ragezorrr.github.io/RustyOS/repo/$arch\n\
            [extra]\nInclude = /etc/pacman.d/mirrorlist\n";
        assert!(parse_pacman_conf_has_repo(conf_with, "rustyos"));
        assert!(parse_pacman_conf_has_repo(conf_with, "RUSTYOS"));
        assert!(!parse_pacman_conf_has_repo(conf_with, "community"));
    }
}
