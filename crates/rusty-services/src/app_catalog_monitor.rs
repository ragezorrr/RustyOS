//! Reactive Linux `inotify` monitor for XDG `.desktop` application directories (`RUSTYOS-164`).
//!
//! Watches user, system, `$XDG_DATA_DIRS`, and Flatpak `applications/` directories (plus existing
//! parent directories when an `applications/` folder has not been created yet), debounces bursts
//! from package managers (`250 ms`), and wakes consumers non-blockingly so Spotlight and Dock stay
//! synchronized without restarting the compositor.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Debounce window coalescing package-manager bursts (`pacman`/`yay`/Flatpak) into one rescan.
pub const APP_CATALOG_DEBOUNCE: Duration = Duration::from_millis(250);

type ChangeCallback = Arc<dyn Fn() + Send + Sync>;

/// Returns deduplicated XDG + Flatpak `.desktop` application directories in priority order
/// (user overrides system) (`RUSTYOS-164`).
#[must_use]
pub fn discover_application_dirs() -> Vec<PathBuf> {
    discover_application_dirs_from(
        env::var("XDG_DATA_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_DATA_DIRS").ok().as_deref(),
    )
}

/// Deterministic helper building the prioritized list of `.desktop` application directories.
#[must_use]
pub fn discover_application_dirs_from(
    xdg_data_home: Option<&str>,
    home: Option<&str>,
    xdg_data_dirs: Option<&str>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let mut push_unique = |path: PathBuf| {
        if seen.insert(path.clone()) {
            dirs.push(path);
        }
    };

    if let Some(data_home) = xdg_data_home.map(str::trim).filter(|s| !s.is_empty()) {
        push_unique(PathBuf::from(data_home).join("applications"));
    } else if let Some(home_dir) = home.map(str::trim).filter(|s| !s.is_empty()) {
        push_unique(PathBuf::from(home_dir).join(".local/share/applications"));
    }

    if let Some(home_dir) = home.map(str::trim).filter(|s| !s.is_empty()) {
        push_unique(
            PathBuf::from(home_dir).join(".local/share/flatpak/exports/share/applications"),
        );
    }

    if let Some(raw_dirs) = xdg_data_dirs {
        for base in raw_dirs.split(':').map(str::trim).filter(|s| !s.is_empty()) {
            push_unique(PathBuf::from(base).join("applications"));
        }
    }

    push_unique(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    push_unique(PathBuf::from("/usr/local/share/applications"));
    push_unique(PathBuf::from("/usr/share/applications"));

    dirs
}

/// Computes the set of currently existing directories to watch with `inotify` (the app dirs
/// themselves, or their nearest existing ancestor so newly created `applications/` folders are caught).
fn resolve_watch_targets(app_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for dir in app_dirs {
        let mut cursor: Option<&Path> = Some(dir.as_path());
        while let Some(p) = cursor {
            if p.is_dir() {
                let pb = p.to_path_buf();
                if seen.insert(pb.clone()) {
                    targets.push(pb);
                }
                break;
            }
            cursor = p.parent();
        }
    }
    targets
}

/// Non-blocking handle to the background Linux `inotify` `.desktop` catalog monitor.
pub struct AppCatalogMonitorHandle {
    dirty: Arc<AtomicBool>,
    revision: Arc<AtomicU64>,
    on_change: Arc<Mutex<Option<ChangeCallback>>>,
    ctrl_tx: Option<linux_watch::ControlSender>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for AppCatalogMonitorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppCatalogMonitorHandle")
            .field("dirty", &self.dirty.load(Ordering::Relaxed))
            .field("revision", &self.revision.load(Ordering::Relaxed))
            .field("active", &self.thread.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for AppCatalogMonitorHandle {
    fn default() -> Self {
        Self {
            dirty: Arc::new(AtomicBool::new(false)),
            revision: Arc::new(AtomicU64::new(0)),
            on_change: Arc::new(Mutex::new(None)),
            ctrl_tx: None,
            thread: None,
        }
    }
}

impl Drop for AppCatalogMonitorHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.ctrl_tx.take() {
            tx.signal_quit();
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl AppCatalogMonitorHandle {
    /// Starts monitoring all standard XDG and Flatpak `.desktop` application directories.
    #[must_use]
    pub fn start_background(on_change: Option<ChangeCallback>) -> Self {
        Self::start_with_dirs(discover_application_dirs(), on_change)
    }

    /// Starts monitoring explicit `.desktop` application directories (useful for isolated tests).
    #[must_use]
    pub fn start_with_dirs(app_dirs: Vec<PathBuf>, on_change: Option<ChangeCallback>) -> Self {
        let dirty = Arc::new(AtomicBool::new(false));
        let revision = Arc::new(AtomicU64::new(0));
        let cb_slot = Arc::new(Mutex::new(on_change));

        let Ok((ctrl_tx, ctrl_rx)) = linux_watch::control_pipe() else {
            return Self {
                dirty,
                revision,
                on_change: cb_slot,
                ctrl_tx: None,
                thread: None,
            };
        };

        let Ok(mut watcher) = linux_watch::InotifyWatcher::new() else {
            return Self {
                dirty,
                revision,
                on_change: cb_slot,
                ctrl_tx: None,
                thread: None,
            };
        };
        // Register all inotify watches before spawning so immediate FS writes are never missed.
        watcher.sync_dirs(&resolve_watch_targets(&app_dirs));

        let dirty_bg = Arc::clone(&dirty);
        let rev_bg = Arc::clone(&revision);
        let cb_bg = Arc::clone(&cb_slot);

        let thread = thread::Builder::new()
            .name("rustyos-app-catalog-mon".to_owned())
            .spawn(move || {
                monitor_thread_main(watcher, &app_dirs, &ctrl_rx, &dirty_bg, &rev_bg, &cb_bg);
            })
            .ok();

        Self {
            dirty,
            revision,
            on_change: cb_slot,
            ctrl_tx: Some(ctrl_tx),
            thread,
        }
    }

    /// Returns `true` if the background `inotify` monitor thread is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.thread.is_some()
    }

    /// Installs or replaces the wakeup callback invoked whenever monitored `.desktop` dirs change.
    pub fn set_on_change(&self, on_change: ChangeCallback) {
        if let Ok(mut guard) = self.on_change.lock() {
            *guard = Some(on_change);
        }
    }

    /// Atomically consumes a pending catalog-changed flag.
    #[must_use]
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// Returns the monotonic count of observed debounced catalog changes.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Requests an immediate catalog refresh signal on the background monitor.
    pub fn trigger_refresh(&self) {
        if let Some(ref tx) = self.ctrl_tx {
            tx.signal_refresh();
        }
    }
}

fn monitor_thread_main(
    mut watcher: linux_watch::InotifyWatcher,
    app_dirs: &[PathBuf],
    ctrl_rx: &linux_watch::ControlReceiver,
    dirty: &AtomicBool,
    revision: &AtomicU64,
    on_change: &Mutex<Option<ChangeCallback>>,
) {
    let mut debounce_deadline: Option<Instant> = None;

    loop {
        let timeout_ms = debounce_deadline.map_or(-1, |deadline| {
            let rem = deadline.saturating_duration_since(Instant::now());
            i32::try_from(rem.as_millis()).unwrap_or(i32::MAX).max(1)
        });

        let events = linux_watch::poll_events(&watcher, ctrl_rx, timeout_ms);
        if events.quit() {
            break;
        }
        if events.inotify_ready() {
            watcher.drain_events();
            debounce_deadline = Some(Instant::now() + APP_CATALOG_DEBOUNCE);
        }
        if events.refresh_requested() {
            debounce_deadline = Some(Instant::now());
        }

        if let Some(deadline) = debounce_deadline
            && Instant::now() >= deadline
        {
            debounce_deadline = None;
            watcher.sync_dirs(&resolve_watch_targets(app_dirs));
            dirty.store(true, Ordering::Release);
            revision.fetch_add(1, Ordering::AcqRel);
            let cb = on_change.lock().ok().and_then(|g| g.clone());
            if let Some(wake) = cb {
                wake();
            }
        }
    }
}

#[allow(unsafe_code)]
mod linux_watch {
    use super::{HashMap, HashSet, PathBuf};
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    const WATCH_MASK: u32 = libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO
        | libc::IN_CLOSE_WRITE
        | libc::IN_ATTRIB
        | libc::IN_DELETE_SELF
        | libc::IN_MOVE_SELF;

    pub(super) struct ControlSender {
        fd: OwnedFd,
    }

    impl ControlSender {
        pub(super) fn signal_quit(&self) {
            self.write_byte(b'Q');
        }

        pub(super) fn signal_refresh(&self) {
            self.write_byte(b'R');
        }

        fn write_byte(&self, b: u8) {
            // SAFETY: `self.fd` is a valid open write end of a non-blocking pipe.
            let _ = unsafe { libc::write(self.fd.as_raw_fd(), (&raw const b).cast(), 1) };
        }
    }

    pub(super) struct ControlReceiver {
        fd: OwnedFd,
    }

    impl ControlReceiver {
        pub(super) fn drain_commands(&self) -> (bool, bool) {
            let mut buf = [0_u8; 64];
            let mut quit = false;
            let mut refresh = false;
            loop {
                // SAFETY: `self.fd` is an open non-blocking pipe read fd; `buf` is valid writable memory.
                let n =
                    unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n <= 0 {
                    break;
                }
                let count = usize::try_from(n).unwrap_or(0);
                for &b in &buf[..count] {
                    if b == b'Q' {
                        quit = true;
                    } else if b == b'R' {
                        refresh = true;
                    }
                }
            }
            (quit, refresh)
        }
    }

    pub(super) fn control_pipe() -> Result<(ControlSender, ControlReceiver), ()> {
        let mut fds = [-1_i32; 2];
        // SAFETY: `fds` is a valid 2-element array for `pipe2`.
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if rc != 0 {
            return Err(());
        }
        // SAFETY: `pipe2` succeeded and returned two newly owned file descriptors.
        let rx = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        // SAFETY: `pipe2` succeeded and returned two newly owned file descriptors.
        let tx = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        Ok((ControlSender { fd: tx }, ControlReceiver { fd: rx }))
    }

    pub(super) struct InotifyWatcher {
        fd: OwnedFd,
        watches: HashMap<PathBuf, i32>,
    }

    impl InotifyWatcher {
        pub(super) fn new() -> Result<Self, ()> {
            // SAFETY: `inotify_init1` requires no pointer arguments and returns a new fd or -1.
            let raw = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            if raw < 0 {
                return Err(());
            }
            // SAFETY: `raw >= 0` is a newly created owned inotify file descriptor.
            let fd = unsafe { OwnedFd::from_raw_fd(raw) };
            Ok(Self {
                fd,
                watches: HashMap::new(),
            })
        }

        pub(super) fn sync_dirs(&mut self, desired: &[PathBuf]) {
            let wanted: HashSet<PathBuf> = desired.iter().cloned().collect();
            let stale: Vec<(PathBuf, i32)> = self
                .watches
                .iter()
                .filter(|(p, _)| !wanted.contains(*p) || !p.is_dir())
                .map(|(p, &wd)| (p.clone(), wd))
                .collect();
            for (path, wd) in stale {
                // SAFETY: `self.fd` is an open inotify fd and `wd` was previously returned by `inotify_add_watch`.
                let _ = unsafe { libc::inotify_rm_watch(self.fd.as_raw_fd(), wd) };
                self.watches.remove(&path);
            }

            for dir in desired {
                if self.watches.contains_key(dir) || !dir.is_dir() {
                    continue;
                }
                let Ok(cpath) = CString::new(dir.as_os_str().as_bytes()) else {
                    continue;
                };
                // SAFETY: `self.fd` is open and `cpath` is a valid NUL-terminated path string.
                let wd = unsafe {
                    libc::inotify_add_watch(self.fd.as_raw_fd(), cpath.as_ptr(), WATCH_MASK)
                };
                if wd >= 0 {
                    self.watches.insert(dir.clone(), wd);
                }
            }
        }

        pub(super) fn drain_events(&self) {
            let mut buf = [0_u8; 4096];
            loop {
                // SAFETY: `self.fd` is an open non-blocking inotify fd; `buf` is valid writable memory.
                let n =
                    unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n <= 0 {
                    break;
                }
            }
        }
    }

    #[derive(Default, Clone, Copy)]
    pub(super) struct PollEvents(u8);

    impl PollEvents {
        const INOTIFY_READY: u8 = 1 << 0;
        const REFRESH_REQUESTED: u8 = 1 << 1;
        const QUIT: u8 = 1 << 2;

        pub(super) const fn inotify_ready(self) -> bool {
            self.0 & Self::INOTIFY_READY != 0
        }

        pub(super) const fn refresh_requested(self) -> bool {
            self.0 & Self::REFRESH_REQUESTED != 0
        }

        pub(super) const fn quit(self) -> bool {
            self.0 & Self::QUIT != 0
        }
    }

    pub(super) fn poll_events(
        inotify: &InotifyWatcher,
        ctrl: &ControlReceiver,
        timeout_ms: i32,
    ) -> PollEvents {
        let mut fds = [
            libc::pollfd {
                fd: ctrl.fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: inotify.fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `fds` contains 2 initialized `pollfd` entries referencing open descriptors.
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };
        if rc <= 0 {
            return PollEvents::default();
        }
        let mut mask = 0_u8;
        if fds[0].revents & libc::POLLIN != 0 {
            let (quit, refresh) = ctrl.drain_commands();
            if quit {
                mask |= PollEvents::QUIT;
            }
            if refresh {
                mask |= PollEvents::REFRESH_REQUESTED;
            }
        }
        if fds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            mask |= PollEvents::QUIT;
        }
        if fds[1].revents & libc::POLLIN != 0 {
            mask |= PollEvents::INOTIFY_READY;
        }
        PollEvents(mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn discover_application_dirs_includes_xdg_data_dirs_and_flatpak_deduplicated() {
        let dirs = discover_application_dirs_from(
            Some("/home/alice/.local/share"),
            Some("/home/alice"),
            Some("/usr/local/share:/opt/custom/share:/usr/share"),
        );
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/home/alice/.local/share/applications"),
                PathBuf::from("/home/alice/.local/share/flatpak/exports/share/applications"),
                PathBuf::from("/usr/local/share/applications"),
                PathBuf::from("/opt/custom/share/applications"),
                PathBuf::from("/usr/share/applications"),
                PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            ]
        );
    }

    #[test]
    fn background_inotify_app_catalog_monitor_detects_installed_desktop_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("rustyos-app-mon-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let apps_dir = root.join("applications");
        fs::create_dir_all(&apps_dir)?;

        let wakes = Arc::new(AtomicUsize::new(0));
        let wakes_cb = Arc::clone(&wakes);
        let monitor = AppCatalogMonitorHandle::start_with_dirs(
            vec![apps_dir.clone()],
            Some(Arc::new(move || {
                wakes_cb.fetch_add(1, Ordering::AcqRel);
            })),
        );
        assert!(monitor.is_active());
        assert!(!monitor.take_dirty());

        fs::write(
            apps_dir.join("new-app.desktop"),
            "[Desktop Entry]\nType=Application\nName=NewApp\nExec=/bin/true\n",
        )?;

        let deadline = Instant::now() + Duration::from_secs(2);
        while monitor.revision() == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            monitor.revision() >= 1,
            "inotify monitor must detect newly installed .desktop file"
        );
        assert!(monitor.take_dirty());
        assert!(wakes.load(Ordering::Acquire) >= 1);

        drop(monitor);
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }
}
