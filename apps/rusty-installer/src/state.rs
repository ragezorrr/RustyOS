//! Application state, background install/network workers, and config persistence for
//! `rusty-installer` (`RUSTYOS-115`, ADR-0065).

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use rusty_animation::{AnimatedValue, Easing};
use rusty_services::NetworkService;
use rusty_services::file_manager::{FilesThemeMode, UserFileLocations, clamp_accent_index};

use crate::disks::{
    DualBootKind, DualBootTargetInfo, TargetDiskInfo, collect_dualboot_targets, demo_target_disks,
    discover_target_disks,
};

pub(crate) const LANGUAGE_OPTIONS: &[(&str, &str, &str)] = &[
    ("en_US.UTF-8", "English (United States)", "UTC"),
    ("ru_RU.UTF-8", "Русский (Россия / Европа)", "Europe/Kyiv"),
    ("uk_UA.UTF-8", "Українська (Україна)", "Europe/Kyiv"),
];

pub(crate) const KEYBOARD_PRESETS: &[(&str, &str, &str, &str)] = &[
    (
        "us,ru",
        "English (US) + Russian",
        "grp:alt_shift_toggle",
        "us",
    ),
    ("us", "English (US)", "", "us"),
    (
        "us,ua",
        "English (US) + Ukrainian",
        "grp:alt_shift_toggle",
        "us",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WizardMode {
    #[default]
    Install,
    Oobe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RunEnv {
    #[default]
    Standard,
    Live,
    Demo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum InstallPhase {
    #[default]
    Review,
    Running,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeSync {
    FollowConfig,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallStep {
    Welcome,
    SelectDisk,
    AccountStyle,
    Installing,
}

impl InstallStep {
    #[must_use]
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::SelectDisk => 1,
            Self::AccountStyle => 2,
            Self::Installing => 3,
        }
    }

    #[must_use]
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Install RustyOS",
            Self::SelectDisk => "Select Destination Disk",
            Self::AccountStyle => "Account & Appearance",
            Self::Installing => "Installing RustyOS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OobeStep {
    Hello,
    Keyboard,
    Network,
    Account,
    Appearance,
    Finish,
}

impl OobeStep {
    #[must_use]
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Hello => 0,
            Self::Keyboard => 1,
            Self::Network => 2,
            Self::Account => 3,
            Self::Appearance => 4,
            Self::Finish => 5,
        }
    }

    #[must_use]
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Hello => "Hello",
            Self::Keyboard => "Keyboard Layout",
            Self::Network => "Wi-Fi & Network",
            Self::Account => "Create Account",
            Self::Appearance => "Choose Your Appearance",
            Self::Finish => "Welcome to RustyOS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeChoice {
    Light,
    Dark,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootloaderKind {
    SystemdBoot,
    Grub,
}

impl BootloaderKind {
    #[must_use]
    pub(crate) const fn as_flag(self) -> &'static str {
        match self {
            Self::SystemdBoot => "systemd-boot",
            Self::Grub => "grub",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DiskInstallMode {
    #[default]
    WholeDisk,
    DualBoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextFieldId {
    FullName,
    Username,
    Hostname,
    Password,
    ConfirmPassword,
}

impl TextFieldId {
    pub(crate) const ALL: [Self; 5] = [
        Self::FullName,
        Self::Username,
        Self::Hostname,
        Self::Password,
        Self::ConfirmPassword,
    ];

    #[must_use]
    pub(crate) fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    #[must_use]
    pub(crate) fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StartupOptions {
    pub mode: WizardMode,
    pub env_kind: RunEnv,
    pub dry_run: bool,
    pub no_glass: bool,
    pub theme_override: Option<FilesThemeMode>,
    pub accent_override: Option<usize>,
    pub install_script: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct NetworkSummary {
    pub ethernet_online: bool,
    pub active_ssid: Option<String>,
    pub detail: String,
}

impl Default for NetworkSummary {
    fn default() -> Self {
        Self {
            ethernet_online: false,
            active_ssid: None,
            detail: String::from("Checking network interfaces..."),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WindowFlags {
    pub running: bool,
    pub maximized: bool,
    pub dirty: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AccountDraft {
    pub full_name: String,
    pub username: String,
    pub username_edited: bool,
    pub hostname: String,
    pub password: String,
    pub confirm_password: String,
    pub autologin: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CaretState {
    pub focused_field: Option<TextFieldId>,
    pub visible: bool,
    pub last_blink: Instant,
}

#[derive(Debug)]
pub(crate) enum WorkerEvent {
    DisksLoaded(Vec<TargetDiskInfo>),
    NetworkLoaded(NetworkSummary),
    InstallStep {
        pct: u8,
        stage: String,
        message: String,
    },
    InstallLog(String),
    InstallCompleted {
        success: bool,
        message: String,
    },
    DoctorResult(String),
}

pub(crate) struct InstallerAppState {
    pub win: WindowFlags,
    pub mode: WizardMode,
    pub env_kind: RunEnv,
    pub install_step: InstallStep,
    pub oobe_step: OobeStep,
    pub phase: InstallPhase,
    pub dry_run: bool,
    pub show_logs: bool,
    pub install_script_override: Option<PathBuf>,

    pub theme_choice: ThemeChoice,
    pub resolved_dark: bool,
    pub theme_sync: ThemeSync,
    pub accent_index: usize,
    pub last_config_mtime: Option<SystemTime>,
    pub last_theme_poll: Instant,

    pub language_idx: usize,
    pub keyboard_idx: usize,
    pub bootloader: BootloaderKind,

    pub disk_mode: DiskInstallMode,
    pub disks: Vec<TargetDiskInfo>,
    pub selected_disk_idx: Option<usize>,
    pub dualboot_targets: Vec<DualBootTargetInfo>,
    pub selected_dualboot_idx: Option<usize>,
    pub network: NetworkSummary,
    pub account: AccountDraft,
    pub caret: CaretState,
    pub form_error: Option<String>,

    pub install_error: Option<String>,
    pub started_at: Instant,
    pub progress_anim: AnimatedValue,
    pub target_pct: u8,
    pub stage_id: String,
    pub stage_message: String,
    pub log_lines: Vec<String>,
    pub utility_status: Option<String>,

    event_tx: Sender<WorkerEvent>,
    event_rx: Receiver<WorkerEvent>,
    wake_ping: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl InstallerAppState {
    #[must_use]
    pub(crate) fn new(opts: &StartupOptions) -> Self {
        let loc = UserFileLocations::detect();
        let sys_theme = loc
            .detect_system_theme()
            .unwrap_or(FilesThemeMode::LightGoldenGate);
        let sys_accent = loc.detect_system_accent().unwrap_or(0);
        let effective_theme = opts.theme_override.unwrap_or(sys_theme);
        let resolved_dark = effective_theme == FilesThemeMode::DarkPurpleGlass;
        let theme_choice = if resolved_dark {
            ThemeChoice::Dark
        } else {
            ThemeChoice::Light
        };
        let accent_index = clamp_accent_index(opts.accent_override.unwrap_or(sys_accent));
        let is_demo = opts.env_kind == RunEnv::Demo;
        let default_pw = if is_demo { "rustyos" } else { "" };

        let (event_tx, event_rx) = mpsc::channel();
        let mut st = Self {
            win: WindowFlags {
                running: true,
                maximized: false,
                dirty: true,
            },
            mode: opts.mode,
            env_kind: opts.env_kind,
            install_step: InstallStep::Welcome,
            oobe_step: OobeStep::Hello,
            phase: InstallPhase::Review,
            dry_run: opts.dry_run || is_demo,
            show_logs: false,
            install_script_override: opts.install_script.clone(),
            theme_choice,
            resolved_dark,
            theme_sync: if opts.theme_override.is_some() {
                ThemeSync::Locked
            } else {
                ThemeSync::FollowConfig
            },
            accent_index,
            last_config_mtime: current_config_mtime(),
            last_theme_poll: Instant::now(),
            language_idx: 0,
            keyboard_idx: 0,
            bootloader: BootloaderKind::SystemdBoot,
            disk_mode: DiskInstallMode::WholeDisk,
            disks: if is_demo {
                demo_target_disks()
            } else {
                Vec::new()
            },
            selected_disk_idx: None,
            dualboot_targets: Vec::new(),
            selected_dualboot_idx: None,
            network: NetworkSummary::default(),
            account: AccountDraft {
                full_name: String::from("RustyOS User"),
                username: String::from("rusty"),
                username_edited: false,
                hostname: String::from("rustyos"),
                password: String::from(default_pw),
                confirm_password: String::from(default_pw),
                autologin: true,
            },
            caret: CaretState {
                focused_field: None,
                visible: true,
                last_blink: Instant::now(),
            },
            form_error: None,
            install_error: None,
            started_at: Instant::now(),
            progress_anim: AnimatedValue::new_settled(0.0),
            target_pct: 0,
            stage_id: String::from("ready"),
            stage_message: String::from("Ready to install RustyOS to your destination drive."),
            log_lines: Vec::new(),
            utility_status: None,
            event_tx,
            event_rx,
            wake_ping: None,
        };
        st.refresh_dualboot_and_defaults();
        st
    }

    pub(crate) fn set_wake_ping(&mut self, ping: impl Fn() + Send + Sync + 'static) {
        self.wake_ping = Some(Arc::new(ping));
    }

    pub(crate) fn kick_initial_probes(&mut self) {
        if self.mode == WizardMode::Install && self.env_kind != RunEnv::Demo {
            self.spawn_disk_scan();
        }
        self.spawn_network_probe();
    }

    #[must_use]
    pub(crate) fn selected_disk(&self) -> Option<&TargetDiskInfo> {
        self.selected_disk_idx.and_then(|i| self.disks.get(i))
    }

    #[must_use]
    pub(crate) fn selected_dualboot(&self) -> Option<&DualBootTargetInfo> {
        self.selected_dualboot_idx
            .and_then(|i| self.dualboot_targets.get(i))
    }

    fn refresh_dualboot_and_defaults(&mut self) {
        self.dualboot_targets = collect_dualboot_targets(&self.disks);
        self.selected_disk_idx = self.disks.iter().position(TargetDiskInfo::is_selectable);
        self.selected_dualboot_idx = self.dualboot_targets.iter().position(|t| t.selectable);
    }

    pub(crate) fn spawn_disk_scan(&mut self) {
        self.win.dirty = true;
        let tx = self.event_tx.clone();
        let demo = self.env_kind == RunEnv::Demo;
        let wake = self.wake_ping.clone();
        let _ = thread::Builder::new()
            .name(String::from("installer-disk-scan"))
            .spawn(move || {
                let list = discover_target_disks(demo);
                let _ = tx.send(WorkerEvent::DisksLoaded(list));
                if let Some(cb) = wake {
                    cb();
                }
            });
    }

    pub(crate) fn spawn_network_probe(&mut self) {
        let tx = self.event_tx.clone();
        let wake = self.wake_ping.clone();
        let _ = thread::Builder::new()
            .name(String::from("installer-net-probe"))
            .spawn(move || {
                let status = NetworkService::new().query_status();
                let summary = NetworkSummary {
                    ethernet_online: status.wired_connected,
                    active_ssid: status.wifi_ssid.clone(),
                    detail: if status.wired_connected {
                        String::from("Ethernet link is connected and ready")
                    } else if let Some(ssid) = &status.wifi_ssid {
                        format!("Connected to Wi-Fi '{ssid}'")
                    } else if status.wifi_enabled {
                        String::from("Wi-Fi radio is enabled; select a network in Settings")
                    } else {
                        String::from("No active connection; offline setup available")
                    },
                };
                let _ = tx.send(WorkerEvent::NetworkLoaded(summary));
                if let Some(cb) = wake {
                    cb();
                }
            });
    }

    pub(crate) fn spawn_doctor_check(&mut self) {
        self.utility_status = Some(String::from("Running rustyos-session --doctor..."));
        self.win.dirty = true;
        let tx = self.event_tx.clone();
        let wake = self.wake_ping.clone();
        let _ = thread::Builder::new()
            .name(String::from("installer-doctor"))
            .spawn(move || {
                let msg = match Command::new("rustyos-session").arg("--doctor").output() {
                    Ok(out) => {
                        let txt = String::from_utf8_lossy(&out.stdout);
                        txt.lines()
                            .rfind(|l| !l.trim().is_empty())
                            .map_or_else(|| String::from("Session check completed"), str::to_owned)
                    }
                    Err(_) => String::from(
                        "rustyos-session --doctor available after workspace build / install",
                    ),
                };
                let _ = tx.send(WorkerEvent::DoctorResult(msg));
                if let Some(cb) = wake {
                    cb();
                }
            });
    }

    pub(crate) fn launch_terminal(&mut self) {
        let ok = ["rusty-terminal", "foot", "alacritty"]
            .into_iter()
            .any(|bin| Command::new(bin).spawn().is_ok());
        self.utility_status = Some(if ok {
            String::from("Opened terminal window")
        } else {
            String::from("No terminal binary found in PATH")
        });
        self.win.dirty = true;
    }

    pub(crate) fn poll_workers(&mut self, now: Instant) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.event_rx.try_recv() {
            changed = true;
            match ev {
                WorkerEvent::DisksLoaded(list) => {
                    self.disks = list;
                    self.refresh_dualboot_and_defaults();
                }
                WorkerEvent::NetworkLoaded(summary) => self.network = summary,
                WorkerEvent::InstallStep {
                    pct,
                    stage,
                    message,
                } => {
                    self.target_pct = pct;
                    let log_entry = format!("[{stage}] {message}");
                    self.stage_id = stage;
                    self.stage_message = message;
                    self.progress_anim.retarget(
                        f64::from(pct) / 100.0,
                        now.saturating_duration_since(self.started_at),
                        Duration::from_millis(260),
                        Easing::EaseOutCubic,
                        false,
                    );
                    self.push_log(log_entry);
                }
                WorkerEvent::InstallLog(line) => self.push_log(line),
                WorkerEvent::InstallCompleted { success, message } => {
                    self.stage_message.clone_from(&message);
                    if success {
                        self.phase = InstallPhase::Finished;
                        self.target_pct = 100;
                        self.progress_anim.retarget(
                            1.0,
                            now.saturating_duration_since(self.started_at),
                            Duration::from_millis(220),
                            Easing::EaseOutCubic,
                            false,
                        );
                    } else {
                        self.phase = InstallPhase::Review;
                        self.install_error = Some(message.clone());
                    }
                    self.push_log(message);
                }
                WorkerEvent::DoctorResult(msg) => self.utility_status = Some(msg),
            }
        }
        if self.progress_anim.is_active() {
            let _ = self
                .progress_anim
                .tick(now.saturating_duration_since(self.started_at));
            changed = true;
        }
        if changed {
            self.win.dirty = true;
        }
        changed
    }

    pub(crate) fn poll_system_theme(&mut self, now: Instant) -> bool {
        if self.theme_sync == ThemeSync::Locked
            || now.saturating_duration_since(self.last_theme_poll) < Duration::from_millis(500)
        {
            return false;
        }
        self.last_theme_poll = now;
        let next_mtime = current_config_mtime();
        if next_mtime == self.last_config_mtime {
            return false;
        }
        self.last_config_mtime = next_mtime;
        let loc = UserFileLocations::detect();
        let next_dark = loc
            .detect_system_theme()
            .unwrap_or(FilesThemeMode::LightGoldenGate)
            == FilesThemeMode::DarkPurpleGlass;
        let next_accent = loc.detect_system_accent().unwrap_or(0);
        if next_dark != self.resolved_dark || next_accent != self.accent_index {
            self.resolved_dark = next_dark;
            self.accent_index = next_accent;
            self.win.dirty = true;
            return true;
        }
        false
    }

    pub(crate) fn update_caret_blink(&mut self, now: Instant) -> bool {
        if self.caret.focused_field.is_none() {
            return false;
        }
        if now.saturating_duration_since(self.caret.last_blink) >= Duration::from_millis(530) {
            self.caret.visible = !self.caret.visible;
            self.caret.last_blink = now;
            self.win.dirty = true;
            return true;
        }
        false
    }

    pub(crate) fn choose_theme(&mut self, choice: ThemeChoice, persist_live: bool) {
        self.theme_choice = choice;
        self.theme_sync = ThemeSync::FollowConfig;
        self.resolved_dark = match choice {
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
            ThemeChoice::Auto => system_auto_prefers_dark(),
        };
        if persist_live && self.env_kind != RunEnv::Demo {
            let _ = self.write_ui_config_to_disk(false);
        }
        self.win.dirty = true;
    }

    pub(crate) fn choose_accent(&mut self, idx: usize, persist_live: bool) {
        self.accent_index = clamp_accent_index(idx);
        if persist_live && self.env_kind != RunEnv::Demo {
            let _ = self.write_ui_config_to_disk(false);
        }
        self.win.dirty = true;
    }

    pub(crate) fn go_back(&mut self) {
        self.form_error = None;
        self.caret.focused_field = None;
        match self.mode {
            WizardMode::Install => {
                if self.phase == InstallPhase::Running {
                    return;
                }
                self.install_step = match self.install_step {
                    InstallStep::Welcome | InstallStep::SelectDisk => InstallStep::Welcome,
                    InstallStep::AccountStyle => InstallStep::SelectDisk,
                    InstallStep::Installing => {
                        self.phase = InstallPhase::Review;
                        self.install_error = None;
                        InstallStep::AccountStyle
                    }
                };
            }
            WizardMode::Oobe => {
                self.oobe_step = match self.oobe_step {
                    OobeStep::Hello | OobeStep::Keyboard => OobeStep::Hello,
                    OobeStep::Network => OobeStep::Keyboard,
                    OobeStep::Account => OobeStep::Network,
                    OobeStep::Appearance => OobeStep::Account,
                    OobeStep::Finish => OobeStep::Appearance,
                };
            }
        }
        self.win.dirty = true;
    }

    pub(crate) fn go_next(&mut self, now: Instant) {
        self.form_error = None;
        match self.mode {
            WizardMode::Install => self.advance_install(now),
            WizardMode::Oobe => self.advance_oobe(),
        }
        self.win.dirty = true;
    }

    fn advance_install(&mut self, now: Instant) {
        match self.install_step {
            InstallStep::Welcome => self.install_step = InstallStep::SelectDisk,
            InstallStep::SelectDisk => {
                match self.disk_mode {
                    DiskInstallMode::WholeDisk => {
                        if !self
                            .selected_disk()
                            .is_some_and(TargetDiskInfo::is_selectable)
                        {
                            self.form_error = Some(String::from(
                                "Select an unmounted destination disk (>= 8 GB)",
                            ));
                            return;
                        }
                    }
                    DiskInstallMode::DualBoot => {
                        if !self.selected_dualboot().is_some_and(|t| t.selectable) {
                            self.form_error = Some(String::from(
                                "Select free space or an unmounted partition (>= 12 GB)",
                            ));
                            return;
                        }
                    }
                }
                self.caret.focused_field = Some(TextFieldId::FullName);
                self.install_step = InstallStep::AccountStyle;
            }
            InstallStep::AccountStyle => {
                if let Err(err) = self.validate_account_form() {
                    self.form_error = Some(err);
                    return;
                }
                self.caret.focused_field = None;
                self.phase = InstallPhase::Review;
                self.install_error = None;
                self.target_pct = 0;
                self.progress_anim = AnimatedValue::new_settled(0.0);
                self.stage_id = String::from("review");
                self.stage_message = String::from("Review installation summary and click Install.");
                self.install_step = InstallStep::Installing;
            }
            InstallStep::Installing => match self.phase {
                InstallPhase::Finished => {
                    if self.dry_run || self.env_kind == RunEnv::Demo {
                        self.mode = WizardMode::Oobe;
                        self.oobe_step = OobeStep::Hello;
                    } else {
                        let _ = Command::new("systemctl").arg("reboot").spawn();
                        self.win.running = false;
                    }
                }
                InstallPhase::Review => self.start_install_worker(now),
                InstallPhase::Running => {}
            },
        }
    }

    fn advance_oobe(&mut self) {
        match self.oobe_step {
            OobeStep::Hello => self.oobe_step = OobeStep::Keyboard,
            OobeStep::Keyboard => {
                self.spawn_network_probe();
                self.oobe_step = OobeStep::Network;
            }
            OobeStep::Network => {
                self.caret.focused_field = Some(TextFieldId::FullName);
                self.oobe_step = OobeStep::Account;
            }
            OobeStep::Account => {
                if self.env_kind != RunEnv::Demo
                    && !self.account.password.is_empty()
                    && let Err(err) = self.validate_account_form()
                {
                    self.form_error = Some(err);
                    return;
                }
                self.caret.focused_field = None;
                self.oobe_step = OobeStep::Appearance;
            }
            OobeStep::Appearance => {
                if self.env_kind != RunEnv::Demo {
                    let _ = self.write_ui_config_to_disk(false);
                }
                self.oobe_step = OobeStep::Finish;
            }
            OobeStep::Finish => {
                if self.env_kind != RunEnv::Demo {
                    let _ = self.write_ui_config_to_disk(true);
                }
                self.win.running = false;
            }
        }
    }

    pub(crate) fn validate_account_form(&self) -> Result<(), String> {
        if self.account.full_name.trim().is_empty() {
            return Err(String::from("Please enter your full name"));
        }
        let u = self.account.username.trim();
        let valid_user = !u.is_empty()
            && u.len() <= 32
            && u.bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
            && u.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        if !valid_user || matches!(u, "root" | "liveuser") {
            return Err(String::from(
                "Account name must be lowercase POSIX letters/digits (a-z, 0-9, -, _)",
            ));
        }
        let h = self.account.hostname.trim();
        let valid_host = !h.is_empty()
            && h.len() <= 63
            && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !valid_host {
            return Err(String::from(
                "Computer name may contain only letters, digits, and '-'",
            ));
        }
        if !self.dry_run && self.account.password.len() < 4 {
            return Err(String::from("Password must be at least 4 characters"));
        }
        if self.account.password != self.account.confirm_password {
            return Err(String::from("Passwords do not match"));
        }
        Ok(())
    }

    pub(crate) fn active_field_mut(&mut self) -> Option<&mut String> {
        match self.caret.focused_field? {
            TextFieldId::FullName => Some(&mut self.account.full_name),
            TextFieldId::Username => Some(&mut self.account.username),
            TextFieldId::Hostname => Some(&mut self.account.hostname),
            TextFieldId::Password => Some(&mut self.account.password),
            TextFieldId::ConfirmPassword => Some(&mut self.account.confirm_password),
        }
    }

    pub(crate) fn handle_text_input(&mut self, text: &str) {
        let Some(field) = self.caret.focused_field else {
            return;
        };
        let sanitized: String = text.chars().filter(|c| !c.is_control()).take(64).collect();
        if sanitized.is_empty() {
            return;
        }
        if let Some(buf) = self.active_field_mut()
            && buf.len() + sanitized.len() <= 96
        {
            buf.push_str(&sanitized);
        }
        if field == TextFieldId::Username {
            self.account.username_edited = true;
        } else if field == TextFieldId::FullName && !self.account.username_edited {
            self.account.username = slugify_username(&self.account.full_name);
        }
        self.form_error = None;
        self.caret.visible = true;
        self.caret.last_blink = Instant::now();
        self.win.dirty = true;
    }

    pub(crate) fn handle_backspace(&mut self) {
        let Some(field) = self.caret.focused_field else {
            return;
        };
        if let Some(buf) = self.active_field_mut() {
            buf.pop();
        }
        if field == TextFieldId::Username {
            self.account.username_edited = !self.account.username.is_empty();
        } else if field == TextFieldId::FullName && !self.account.username_edited {
            self.account.username = slugify_username(&self.account.full_name);
        }
        self.form_error = None;
        self.caret.visible = true;
        self.caret.last_blink = Instant::now();
        self.win.dirty = true;
    }

    pub(crate) fn cycle_focus(&mut self, reverse: bool) {
        if !matches!(
            (self.mode, self.install_step, self.oobe_step),
            (WizardMode::Install, InstallStep::AccountStyle, _)
                | (WizardMode::Oobe, _, OobeStep::Account)
        ) {
            return;
        }
        let next = match self.caret.focused_field {
            Some(cur) if reverse => cur.prev(),
            Some(cur) => cur.next(),
            None => TextFieldId::FullName,
        };
        self.caret.focused_field = Some(next);
        self.caret.visible = true;
        self.caret.last_blink = Instant::now();
        self.win.dirty = true;
    }

    #[must_use]
    pub(crate) fn build_install_args(&self) -> Vec<String> {
        let (locale, _, tz) = LANGUAGE_OPTIONS[self.language_idx];
        let (kb_layout, _, _, keymap) = KEYBOARD_PRESETS[self.keyboard_idx];
        let appearance = match self.theme_choice {
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
            ThemeChoice::Auto => "auto",
        };
        let mut args = match self.disk_mode {
            DiskInstallMode::WholeDisk => {
                let disk_path = self
                    .selected_disk()
                    .map_or_else(|| String::from("/dev/vda"), |d| d.path.clone());
                vec![
                    String::from("--install-mode"),
                    String::from("whole-disk"),
                    String::from("--target-disk"),
                    disk_path,
                ]
            }
            DiskInstallMode::DualBoot => self.dualboot_target_args(),
        };
        args.extend([
            String::from("--hostname"),
            self.account.hostname.trim().to_owned(),
            String::from("--username"),
            self.account.username.trim().to_owned(),
            String::from("--locale"),
            locale.to_owned(),
            String::from("--keymap"),
            keymap.to_owned(),
            String::from("--kb-layout"),
            kb_layout.to_owned(),
            String::from("--timezone"),
            tz.to_owned(),
            String::from("--bootloader"),
            self.bootloader.as_flag().to_owned(),
            String::from("--appearance"),
            appearance.to_owned(),
            String::from("--accent"),
            self.accent_index.to_string(),
            String::from("--autologin"),
            String::from(if self.account.autologin { "1" } else { "0" }),
            String::from("--install-oobe"),
            String::from("1"),
        ]);
        if self.dry_run {
            args.push(String::from("--dry-run"));
        } else {
            args.push(String::from(match self.disk_mode {
                DiskInstallMode::WholeDisk => "--confirm-erase",
                DiskInstallMode::DualBoot => "--confirm-dualboot",
            }));
            args.push(String::from("--password-stdin"));
        }
        args
    }

    fn dualboot_target_args(&self) -> Vec<String> {
        let mut out = vec![String::from("--install-mode"), String::from("dual-boot")];
        if let Some(target) = self.selected_dualboot() {
            match &target.kind {
                DualBootKind::UnallocatedGap {
                    start_bytes,
                    size_bytes,
                } => {
                    out.push(String::from("--free-space"));
                    out.push(format!("{}:{start_bytes}:{size_bytes}", target.disk_path));
                }
                DualBootKind::ExistingPartition { part_path, .. } => {
                    out.push(String::from("--root-part"));
                    out.push(part_path.clone());
                }
            }
            if let Some(esp) = &target.efi_part {
                out.push(String::from("--efi-part"));
                out.push(esp.clone());
            }
        }
        out
    }

    pub(crate) fn start_install_worker(&mut self, now: Instant) {
        self.phase = InstallPhase::Running;
        self.install_error = None;
        self.target_pct = 3;
        self.progress_anim.retarget(
            0.03,
            now.saturating_duration_since(self.started_at),
            Duration::from_millis(160),
            Easing::EaseOutCubic,
            false,
        );
        self.stage_id = String::from("starting");
        self.stage_message = String::from("Starting RustyOS installer...");
        self.log_lines.clear();

        let script = resolve_install_script(self.install_script_override.as_deref());
        let args = self.build_install_args();
        let dry_run = self.dry_run;
        let pass = self.account.password.clone();
        let tx = self.event_tx.clone();
        let wake = self.wake_ping.clone();

        let _ = thread::Builder::new()
            .name(String::from("rusty-install-exec"))
            .spawn(move || {
                execute_install_process(
                    script.as_deref(),
                    &args,
                    dry_run,
                    &pass,
                    &tx,
                    wake.as_ref(),
                );
            });
    }

    fn push_log(&mut self, line: String) {
        if self.log_lines.len() >= 120 {
            self.log_lines.remove(0);
        }
        self.log_lines.push(line);
    }

    fn write_ui_config_to_disk(&mut self, clear_oobe_marker: bool) -> std::io::Result<()> {
        let Some(path) = config_toml_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        let (kb_layout, _, kb_opt, _) = KEYBOARD_PRESETS[self.keyboard_idx];
        let theme_val = if self.resolved_dark { "dark" } else { "light" };
        let mode_val = match self.theme_choice {
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
            ThemeChoice::Auto => "auto",
        };
        let accent_str = self.accent_index.to_string();
        for (sec, k, v) in [
            ("keyboard", "layout", kb_layout),
            ("keyboard", "options", kb_opt),
            ("ui", "theme", theme_val),
            ("ui", "theme_mode", mode_val),
            ("ui", "accent_index", accent_str.as_str()),
        ] {
            text = patch_toml_text(&text, sec, k, v);
        }
        write_atomic(&path, text.as_bytes())?;
        self.last_config_mtime = current_config_mtime();
        if clear_oobe_marker && let Some(dir) = path.parent() {
            let _ = fs::remove_file(dir.join(".oobe-pending"));
        }
        notify_compositor_reload();
        Ok(())
    }
}

fn ping_callback(wake: Option<&Arc<dyn Fn() + Send + Sync>>) {
    if let Some(cb) = wake {
        cb();
    }
}

fn run_dry_fallback(
    dry_run: bool,
    tx: &Sender<WorkerEvent>,
    wake: Option<&Arc<dyn Fn() + Send + Sync>>,
) {
    if !dry_run {
        let _ = tx.send(WorkerEvent::InstallCompleted {
            success: false,
            message: String::from("Installer backend /usr/lib/rustyos/rustyos-install not found"),
        });
        ping_callback(wake);
        return;
    }
    for (pct, st, msg) in [
        (20_u8, "partition", "[dry-run] Validating GPT/EFI layout"),
        (
            55_u8,
            "pacstrap",
            "[dry-run] Checking base + rustyos-git packages",
        ),
        (
            85_u8,
            "desktop",
            "[dry-run] Generating greetd & RustyOS config",
        ),
        (
            100_u8,
            "done",
            "[dry-run] Verification complete (no disk modified)",
        ),
    ] {
        let _ = tx.send(WorkerEvent::InstallStep {
            pct,
            stage: st.to_owned(),
            message: msg.to_owned(),
        });
        ping_callback(wake);
        thread::sleep(Duration::from_millis(240));
    }
    let _ = tx.send(WorkerEvent::InstallCompleted {
        success: true,
        message: String::from("Dry-run completed cleanly (no disk modified)."),
    });
    ping_callback(wake);
}

fn execute_install_process(
    script: Option<&Path>,
    args: &[String],
    dry_run: bool,
    password: &str,
    tx: &Sender<WorkerEvent>,
    wake: Option<&Arc<dyn Fn() + Send + Sync>>,
) {
    let Some(script_path) = script else {
        run_dry_fallback(dry_run, tx, wake);
        return;
    };

    let use_pkexec = !dry_run && unsafe_uid_nonzero();
    let mut cmd = if use_pkexec {
        let mut c = Command::new("pkexec");
        c.arg(script_path);
        c
    } else {
        Command::new(script_path)
    };
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let Ok(mut child) = cmd.spawn() else {
        let _ = tx.send(WorkerEvent::InstallCompleted {
            success: false,
            message: format!("Failed to spawn {}", script_path.display()),
        });
        ping_callback(wake);
        return;
    };

    if !dry_run && let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{password}");
    }

    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some((pct, stage, message)) = parse_step_line(&line) {
                let _ = tx.send(WorkerEvent::InstallStep {
                    pct,
                    stage,
                    message,
                });
                if dry_run {
                    thread::sleep(Duration::from_millis(120));
                }
            } else {
                let _ = tx.send(WorkerEvent::InstallLog(line));
            }
            ping_callback(wake);
        }
    }

    let status = child.wait();
    let err_tail = child
        .stderr
        .take()
        .map(|se| {
            BufReader::new(se)
                .lines()
                .map_while(Result::ok)
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .unwrap_or_default();

    let (success, message) = match status {
        Ok(s) if s.success() => (
            true,
            if dry_run {
                String::from("Simulation finished cleanly without disk modifications.")
            } else {
                String::from("RustyOS installation completed successfully!")
            },
        ),
        Ok(s) => (false, format!("Installer exited with {s}: {err_tail}")),
        Err(e) => (false, format!("Failed waiting on installer: {e}")),
    };
    let _ = tx.send(WorkerEvent::InstallCompleted { success, message });
    ping_callback(wake);
}

fn unsafe_uid_nonzero() -> bool {
    std::env::var("EUID")
        .or_else(|_| std::env::var("UID"))
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|uid| uid != 0)
        || Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            })
            .is_some_and(|uid| uid != 0)
}

#[must_use]
pub(crate) fn parse_step_line(line: &str) -> Option<(u8, String, String)> {
    let rest = line.strip_prefix("STEP:")?;
    let mut parts = rest.splitn(3, ':');
    let pct = parts.next()?.parse::<u8>().ok()?.min(100);
    let stage = parts.next()?.to_owned();
    let msg = parts.next()?.to_owned();
    Some((pct, stage, msg))
}

#[must_use]
pub(crate) fn slugify_username(full_name: &str) -> String {
    let mut out = String::new();
    for ch in full_name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || (lower.is_ascii_digit() && !out.is_empty()) {
            out.push(lower);
        } else if (ch == ' ' || ch == '-') && !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 20 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        String::from("rusty")
    } else {
        out
    }
}

fn resolve_install_script(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return p.exists().then(|| p.to_path_buf());
    }
    for candidate in [
        Path::new("/usr/lib/rustyos/rustyos-install"),
        Path::new("scripts/rustyos-install.sh"),
        Path::new("../scripts/rustyos-install.sh"),
        Path::new("../../scripts/rustyos-install.sh"),
    ] {
        if candidate.exists() {
            return Some(candidate.to_path_buf());
        }
    }
    None
}

fn config_toml_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        return Some(PathBuf::from(xdg).join("rustyos").join("config.toml"));
    }
    env::var("HOME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .map(|h| PathBuf::from(h).join(".config/rustyos/config.toml"))
}

fn current_config_mtime() -> Option<SystemTime> {
    fs::metadata(config_toml_path()?).ok()?.modified().ok()
}

fn format_toml_value(val: &str) -> String {
    if matches!(val, "true" | "false") || val.parse::<f64>().is_ok() {
        val.to_owned()
    } else {
        let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
}

pub(crate) fn patch_toml_text(original: &str, section: &str, key: &str, raw_val: &str) -> String {
    let mut lines: Vec<String> = original.lines().map(ToOwned::to_owned).collect();
    let formatted_line = format!("{key} = {}", format_toml_value(raw_val));
    let mut in_section = false;
    let mut section_found = false;
    let mut section_end_idx = lines.len();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let hdr = trimmed[1..trimmed.len() - 1].trim();
            if in_section {
                section_end_idx = i;
                break;
            }
            if hdr == section {
                in_section = true;
                section_found = true;
            }
            continue;
        }
        if in_section
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && let Some((lhs, _)) = trimmed.split_once('=')
            && lhs.trim() == key
        {
            lines[i] = formatted_line;
            return lines.join("\n") + "\n";
        }
    }

    if section_found {
        let mut insert_at = section_end_idx;
        while insert_at > 0 && lines[insert_at - 1].trim().is_empty() {
            insert_at -= 1;
        }
        lines.insert(insert_at, formatted_line);
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.push(formatted_line);
    }
    lines.join("\n") + "\n"
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}

fn notify_compositor_reload() {
    let sock_path = env::var("RUSTYOS_IPC_SOCKET")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let rt = env::var_os("XDG_RUNTIME_DIR")?;
            let wl = env::var("WAYLAND_DISPLAY").ok()?;
            let cand = PathBuf::from(rt).join(format!("rustyos-ipc-{wl}.sock"));
            cand.exists().then_some(cand)
        });
    let Some(path) = sock_path else {
        return;
    };
    if let Ok(mut stream) = UnixStream::connect(path) {
        let timeout = Some(Duration::from_millis(120));
        let _ = stream.set_read_timeout(timeout);
        let _ = stream.set_write_timeout(timeout);
        if stream.write_all(b"v1|RELOAD_CONFIG\n").is_ok() {
            let _ = stream.flush();
            let mut r = BufReader::new(stream);
            let mut line = String::new();
            let _ = r.read_line(&mut line);
        }
    }
}

fn system_auto_prefers_dark() -> bool {
    Command::new("date")
        .arg("+%H")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u8>().ok())
        .is_some_and(|h| !(7..19).contains(&h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_wizard_validates_disk_account_and_dry_run_command() {
        let opts = StartupOptions {
            mode: WizardMode::Install,
            env_kind: RunEnv::Demo,
            dry_run: true,
            ..Default::default()
        };
        let mut st = InstallerAppState::new(&opts);
        assert_eq!(st.install_step, InstallStep::Welcome);
        let now = Instant::now();
        st.go_next(now);
        assert_eq!(st.install_step, InstallStep::SelectDisk);
        assert!(
            st.selected_disk()
                .is_some_and(TargetDiskInfo::is_selectable)
        );

        st.go_next(now);
        assert_eq!(st.install_step, InstallStep::AccountStyle);

        st.account.password = String::from("abc");
        st.account.confirm_password = String::from("xyz");
        assert!(st.validate_account_form().is_err());

        st.account.password = String::from("secret12");
        st.account.confirm_password = String::from("secret12");
        st.choose_theme(ThemeChoice::Dark, false);
        st.choose_accent(1, false);
        assert!(st.validate_account_form().is_ok());

        let args = st.build_install_args();
        assert!(args.contains(&String::from("--dry-run")));
        assert!(args.contains(&String::from("/dev/vda")));
        assert!(args.contains(&String::from("dark")));
        assert!(args.contains(&String::from("1")));

        st.go_next(now);
        assert_eq!(st.install_step, InstallStep::Installing);
        assert_eq!(
            parse_step_line("STEP:55:pacstrap:Installing packages"),
            Some((
                55,
                String::from("pacstrap"),
                String::from("Installing packages")
            ))
        );
    }

    #[test]
    fn oobe_wizard_transitions_and_slugify_work() {
        assert_eq!(slugify_username("Alexey Ivanov"), "alexey-ivanov");
        let opts = StartupOptions {
            mode: WizardMode::Oobe,
            env_kind: RunEnv::Demo,
            ..Default::default()
        };
        let mut st = InstallerAppState::new(&opts);
        let now = Instant::now();
        assert_eq!(st.oobe_step, OobeStep::Hello);
        st.go_next(now);
        assert_eq!(st.oobe_step, OobeStep::Keyboard);
        st.go_next(now);
        assert_eq!(st.oobe_step, OobeStep::Network);
        st.go_next(now);
        assert_eq!(st.oobe_step, OobeStep::Account);
        st.go_next(now);
        assert_eq!(st.oobe_step, OobeStep::Appearance);
        st.choose_accent(2, false);
        assert_eq!(st.accent_index, 2);
        st.go_next(now);
        assert_eq!(st.oobe_step, OobeStep::Finish);
        st.go_back();
        assert_eq!(st.oobe_step, OobeStep::Appearance);
    }

    #[test]
    fn dry_run_worker_executes_backend_script_and_parses_steps() {
        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/rustyos-install.sh");
        assert!(script.is_file());
        let opts = StartupOptions {
            mode: WizardMode::Install,
            env_kind: RunEnv::Demo,
            dry_run: true,
            install_script: Some(script),
            ..Default::default()
        };
        let mut st = InstallerAppState::new(&opts);
        let now = Instant::now();
        st.go_next(now);
        st.go_next(now);
        st.account.password = String::from("secret12");
        st.account.confirm_password = String::from("secret12");
        st.go_next(now);
        assert_eq!(st.install_step, InstallStep::Installing);
        st.go_next(now);
        assert_eq!(st.phase, InstallPhase::Running);

        let deadline = Instant::now() + Duration::from_secs(5);
        while st.phase == InstallPhase::Running && Instant::now() < deadline {
            let _ = st.poll_workers(Instant::now());
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(st.phase, InstallPhase::Finished);
        assert!(st.install_error.is_none());
        assert_eq!(st.target_pct, 100);
        assert!(st.log_lines.iter().any(|l| l.contains("[done]")));
    }

    #[test]
    fn dualboot_mode_builds_safe_args_and_runs_dry_backend() {
        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/rustyos-install.sh");
        let opts = StartupOptions {
            mode: WizardMode::Install,
            env_kind: RunEnv::Demo,
            dry_run: true,
            install_script: Some(script),
            ..Default::default()
        };
        let mut st = InstallerAppState::new(&opts);
        let now = Instant::now();
        st.go_next(now);
        st.disk_mode = DiskInstallMode::DualBoot;
        assert!(st.selected_dualboot().is_some());

        let args = st.build_install_args();
        assert!(
            args.windows(2)
                .any(|w| w == ["--install-mode", "dual-boot"])
        );
        assert!(
            args.iter()
                .any(|a| a == "--free-space" || a == "--root-part")
        );
        assert!(!args.iter().any(|a| a == "--confirm-erase"));

        st.go_next(now);
        assert_eq!(st.install_step, InstallStep::AccountStyle);
        st.account.password = String::from("secret12");
        st.account.confirm_password = String::from("secret12");
        st.go_next(now);
        assert_eq!(st.install_step, InstallStep::Installing);
        st.go_next(now);
        assert_eq!(st.phase, InstallPhase::Running);

        let deadline = Instant::now() + Duration::from_secs(5);
        while st.phase == InstallPhase::Running && Instant::now() < deadline {
            let _ = st.poll_workers(Instant::now());
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(st.phase, InstallPhase::Finished);
        assert!(st.install_error.is_none());
        assert!(
            st.log_lines
                .iter()
                .any(|l| l.contains("Preserves existing"))
        );
    }
}
