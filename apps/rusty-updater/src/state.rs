//! Application state, background check/transaction polling, and actions for `rusty-updater`
//! (`RUSTYOS-077`, ADR-0037).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::SystemTime;

use rusty_services::file_manager::{FilesThemeMode, detect_system_theme_from_toml};
use rusty_services::updates::{
    PackageUpdateItem, SystemReleaseSummary, UpdateCheckState, UpdateStatusSnapshot,
    UpdateTransactionHandle, UpdateTxEvent, UpdateTxPhase, check_all_pending_updates,
    read_system_release_summary,
};

pub(crate) const ROW_HEIGHT: i32 = 38;
pub(crate) const LOG_LINE_HEIGHT: i32 = 22;
const MAX_LOG_LINES: usize = 300;

/// Startup configuration parsed from CLI flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StartupOptions {
    pub theme_override: Option<FilesThemeMode>,
    pub dry_run: bool,
    pub check_immediately: bool,
    pub pkgbuild_dir: Option<PathBuf>,
}

/// Filter tabs narrowing the displayed package list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum UpdaterFilter {
    #[default]
    All,
    RustyOs,
    RestartRequired,
}

impl UpdaterFilter {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::RustyOs => "RustyOS",
            Self::RestartRequired => "Restart",
        }
    }
}

/// Self-contained UI & transaction state for `rusty-updater`.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct UpdaterAppState {
    pub running: bool,
    pub dirty: bool,
    pub theme: FilesThemeMode,
    theme_locked: bool,
    last_config_mtime: Option<SystemTime>,
    pub dry_run: bool,
    pub summary: SystemReleaseSummary,
    pub snapshot: UpdateStatusSnapshot,
    pub filter: UpdaterFilter,
    pub show_logs: bool,
    pub scroll_y: i32,
    pub log_scroll_y: i32,
    pub selected_idx: Option<usize>,
    pub tx_phase: UpdateTxPhase,
    pub tx_progress: f32,
    pub tx_logs: Vec<String>,
    pub tx_summary: Option<String>,
    pub tx_restart_recommended: bool,
    check_rx: Option<Receiver<UpdateStatusSnapshot>>,
    active_tx: Option<UpdateTransactionHandle>,
}

impl UpdaterAppState {
    #[must_use]
    pub(crate) fn new(opts: &StartupOptions) -> Self {
        let mut summary = read_system_release_summary();
        if let Some(dir) = opts.pkgbuild_dir.clone() {
            summary.repo_pkgbuild_dir = Some(dir);
        }
        let theme = opts.theme_override.unwrap_or_else(detect_config_theme);

        let mut app = Self {
            running: true,
            dirty: true,
            theme,
            theme_locked: opts.theme_override.is_some(),
            last_config_mtime: updater_config_mtime(),
            dry_run: opts.dry_run,
            summary,
            snapshot: UpdateStatusSnapshot::default(),
            filter: UpdaterFilter::All,
            show_logs: false,
            scroll_y: 0,
            log_scroll_y: 0,
            selected_idx: None,
            tx_phase: UpdateTxPhase::Idle,
            tx_progress: 0.0,
            tx_logs: Vec::new(),
            tx_summary: None,
            tx_restart_recommended: false,
            check_rx: None,
            active_tx: None,
        };
        if opts.check_immediately || !opts.dry_run {
            app.start_check_now();
        } else {
            app.snapshot = sample_dry_run_snapshot();
        }
        app
    }

    #[must_use]
    pub(crate) fn is_checking(&self) -> bool {
        self.check_rx.is_some() || self.snapshot.state == UpdateCheckState::Checking
    }

    #[must_use]
    pub(crate) fn is_upgrading(&self) -> bool {
        self.active_tx.is_some()
    }

    #[must_use]
    pub(crate) fn has_pending_jobs(&self) -> bool {
        self.is_checking() || self.is_upgrading()
    }

    pub(crate) fn request_quit(&mut self) {
        if let Some(tx) = self.active_tx.take() {
            tx.cancel();
        }
        self.running = false;
    }

    pub(crate) fn poll_system_theme(&mut self) -> bool {
        if self.theme_locked {
            return false;
        }
        let mtime = updater_config_mtime();
        if mtime.is_some() && mtime != self.last_config_mtime {
            self.last_config_mtime = mtime;
            let next = detect_config_theme();
            if next != self.theme {
                self.theme = next;
                self.dirty = true;
                return true;
            }
        }
        false
    }

    pub(crate) fn start_check_now(&mut self) {
        if self.is_checking() || self.is_upgrading() {
            return;
        }
        if self.dry_run {
            self.snapshot = sample_dry_run_snapshot();
            self.scroll_y = 0;
            self.selected_idx = None;
            self.dirty = true;
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.snapshot.state = UpdateCheckState::Checking;
        self.check_rx = Some(rx);
        self.dirty = true;
        let _ = thread::Builder::new()
            .name("updater-check".to_owned())
            .spawn(move || {
                let snap = check_all_pending_updates();
                let _ = tx.send(snap);
            });
    }

    pub(crate) fn start_system_upgrade(&mut self) {
        if self.is_upgrading() || self.is_checking() {
            return;
        }
        self.tx_logs.clear();
        self.tx_progress = 0.04;
        self.tx_phase = UpdateTxPhase::Authenticating;
        self.tx_summary = None;
        self.tx_restart_recommended = false;
        self.log_scroll_y = 0;
        let tx = UpdateTransactionHandle::start_system_upgrade(
            self.dry_run,
            self.summary.helper_path.clone(),
            self.snapshot.restart_recommended,
            self.snapshot.items.clone(),
        );
        self.active_tx = Some(tx);
        self.dirty = true;
    }

    pub(crate) fn start_local_rustyos_upgrade(&mut self) {
        if self.is_upgrading() || self.is_checking() {
            return;
        }
        let Some(pkg_dir) = self.summary.repo_pkgbuild_dir.clone() else {
            return;
        };
        self.tx_logs.clear();
        self.tx_progress = 0.04;
        self.tx_phase = UpdateTxPhase::BuildingPackage;
        self.tx_summary = None;
        self.tx_restart_recommended = false;
        self.log_scroll_y = 0;
        self.show_logs = true;
        let tx = UpdateTransactionHandle::start_rustyos_local_upgrade(
            self.dry_run,
            pkg_dir,
            self.summary.helper_path.clone(),
        );
        self.active_tx = Some(tx);
        self.dirty = true;
    }

    pub(crate) fn cancel_upgrade(&mut self) {
        if let Some(tx) = &self.active_tx {
            tx.cancel();
            self.tx_phase = UpdateTxPhase::Cancelled;
            self.dirty = true;
        }
    }

    pub(crate) fn set_filter(&mut self, filter: UpdaterFilter) {
        if self.filter != filter {
            self.filter = filter;
            self.scroll_y = 0;
            self.selected_idx = None;
            self.dirty = true;
        }
    }

    pub(crate) fn toggle_logs(&mut self) {
        self.show_logs = !self.show_logs;
        self.dirty = true;
    }

    #[must_use]
    pub(crate) fn filtered_items(&self) -> Vec<&PackageUpdateItem> {
        self.snapshot
            .items
            .iter()
            .filter(|item| match self.filter {
                UpdaterFilter::All => true,
                UpdaterFilter::RustyOs => item.is_rustyos,
                UpdaterFilter::RestartRequired => item.requires_restart,
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn filter_count(&self, filter: UpdaterFilter) -> usize {
        match filter {
            UpdaterFilter::All => self.snapshot.total_count.max(self.snapshot.items.len()),
            UpdaterFilter::RustyOs => self
                .snapshot
                .items
                .iter()
                .filter(|item| item.is_rustyos)
                .count(),
            UpdaterFilter::RestartRequired => self
                .snapshot
                .items
                .iter()
                .filter(|item| item.requires_restart)
                .count(),
        }
    }

    pub(crate) fn scroll_by(&mut self, delta_px: i32, list_viewport_h: i32) {
        if self.show_logs {
            let content_h = i32::try_from(self.tx_logs.len()).unwrap_or(0) * LOG_LINE_HEIGHT + 12;
            let max_scroll = (content_h - list_viewport_h).max(0);
            let next = (self.log_scroll_y + delta_px).clamp(0, max_scroll);
            if next != self.log_scroll_y {
                self.log_scroll_y = next;
                self.dirty = true;
            }
        } else {
            let content_h =
                i32::try_from(self.filtered_items().len()).unwrap_or(0) * ROW_HEIGHT + 8;
            let max_scroll = (content_h - list_viewport_h).max(0);
            let next = (self.scroll_y + delta_px).clamp(0, max_scroll);
            if next != self.scroll_y {
                self.scroll_y = next;
                self.dirty = true;
            }
        }
    }

    pub(crate) fn poll_background_jobs(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.check_rx {
            match rx.try_recv() {
                Ok(snap) => {
                    self.snapshot = snap;
                    self.check_rx = None;
                    self.scroll_y = 0;
                    changed = true;
                }
                Err(TryRecvError::Disconnected) => {
                    self.check_rx = None;
                    changed = true;
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        let events = self
            .active_tx
            .as_ref()
            .map(UpdateTransactionHandle::drain_events)
            .unwrap_or_default();
        if !events.is_empty() {
            changed = true;
            self.apply_tx_events(events);
        }

        if changed {
            self.dirty = true;
        }
        changed
    }

    fn apply_tx_events(&mut self, events: Vec<UpdateTxEvent>) {
        let mut trigger_recheck = false;
        for ev in events {
            match ev {
                UpdateTxEvent::Phase(phase) => self.tx_phase = phase,
                UpdateTxEvent::Progress(prog) => self.tx_progress = prog.clamp(0.0, 1.0),
                UpdateTxEvent::LogLine(line) => {
                    self.tx_logs.push(line);
                    if self.tx_logs.len() > MAX_LOG_LINES {
                        let overflow = self.tx_logs.len() - MAX_LOG_LINES;
                        self.tx_logs.drain(0..overflow);
                    }
                }
                UpdateTxEvent::Finished {
                    success,
                    restart_recommended,
                    summary,
                } => {
                    self.active_tx = None;
                    self.tx_summary = Some(summary);
                    self.tx_restart_recommended = success && restart_recommended;
                    if success {
                        self.tx_phase = UpdateTxPhase::Complete;
                        self.tx_progress = 1.0;
                        if self.dry_run {
                            self.snapshot.state = UpdateCheckState::UpToDate;
                            self.snapshot.total_count = 0;
                            self.snapshot.arch_count = 0;
                            self.snapshot.rustyos_available = false;
                            self.snapshot.items.clear();
                        } else {
                            trigger_recheck = true;
                        }
                    } else if self.tx_phase != UpdateTxPhase::Cancelled {
                        self.tx_phase = UpdateTxPhase::Failed;
                    }
                }
            }
        }
        if trigger_recheck {
            self.start_check_now();
        }
    }

    pub(crate) fn handle_key_input(&mut self, key: &str, ctrl: bool, viewport_h: i32) {
        if ctrl && (key == "q" || key == "Q") {
            self.request_quit();
            return;
        }
        match key {
            "Escape" => {
                if self.show_logs {
                    self.show_logs = false;
                    self.dirty = true;
                }
            }
            "r" | "R" => self.start_check_now(),
            "l" | "L" => self.toggle_logs(),
            "Down" => self.scroll_by(ROW_HEIGHT, viewport_h),
            "Up" => self.scroll_by(-ROW_HEIGHT, viewport_h),
            "Page_Down" => self.scroll_by(viewport_h.max(ROW_HEIGHT), viewport_h),
            "Page_Up" => self.scroll_by(-(viewport_h.max(ROW_HEIGHT)), viewport_h),
            _ => {}
        }
    }
}

fn updater_config_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|base| base.join("rustyos/config.toml"))
}

fn updater_config_mtime() -> Option<SystemTime> {
    std::fs::metadata(updater_config_path()?)
        .ok()?
        .modified()
        .ok()
}

fn detect_config_theme() -> FilesThemeMode {
    if let Some(path) = updater_config_path()
        && let Ok(raw) = std::fs::read_to_string(path)
        && let Some(mode) = detect_system_theme_from_toml(&raw)
    {
        return mode;
    }
    FilesThemeMode::LightGoldenGate
}

#[must_use]
pub(crate) fn sample_dry_run_snapshot() -> UpdateStatusSnapshot {
    let items = vec![
        PackageUpdateItem {
            name: "rustyos-git".to_owned(),
            old_version: "0.1.0.r76-1".to_owned(),
            new_version: "0.1.0.r77-1".to_owned(),
            is_rustyos: true,
            requires_restart: true,
        },
        PackageUpdateItem {
            name: "linux".to_owned(),
            old_version: "7.2.11.arch1-1".to_owned(),
            new_version: "7.3.4.arch1-1".to_owned(),
            is_rustyos: false,
            requires_restart: true,
        },
        PackageUpdateItem {
            name: "mesa".to_owned(),
            old_version: "1:26.2.6-1".to_owned(),
            new_version: "1:26.3.0-1".to_owned(),
            is_rustyos: false,
            requires_restart: true,
        },
        PackageUpdateItem {
            name: "wayland".to_owned(),
            old_version: "1.23.1-1".to_owned(),
            new_version: "1.24.0-1".to_owned(),
            is_rustyos: false,
            requires_restart: true,
        },
        PackageUpdateItem {
            name: "firefox".to_owned(),
            old_version: "136.0-1".to_owned(),
            new_version: "136.0.1-1".to_owned(),
            is_rustyos: false,
            requires_restart: false,
        },
    ];
    UpdateStatusSnapshot {
        state: UpdateCheckState::Available,
        total_count: items.len(),
        arch_count: 4,
        rustyos_available: true,
        restart_recommended: true,
        last_checked_label: "Dry-run preview".to_owned(),
        error_hint: None,
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_counts_and_dry_run_simulation_update_state_cleanly() {
        let opts = StartupOptions {
            theme_override: Some(FilesThemeMode::LightGoldenGate),
            dry_run: true,
            check_immediately: false,
            pkgbuild_dir: None,
        };
        let mut app = UpdaterAppState::new(&opts);
        assert_eq!(app.filter_count(UpdaterFilter::All), 5);
        assert_eq!(app.filter_count(UpdaterFilter::RustyOs), 1);
        assert_eq!(app.filter_count(UpdaterFilter::RestartRequired), 4);

        app.set_filter(UpdaterFilter::RustyOs);
        let filtered = app.filtered_items();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "rustyos-git");

        app.start_system_upgrade();
        assert!(app.is_upgrading());
        for _ in 0..40 {
            app.poll_background_jobs();
            if !app.is_upgrading() {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(60));
        }
        assert!(!app.is_upgrading());
        assert_eq!(app.tx_phase, UpdateTxPhase::Complete);
        assert!(app.tx_restart_recommended);
        assert_eq!(app.snapshot.state, UpdateCheckState::UpToDate);
    }
}
