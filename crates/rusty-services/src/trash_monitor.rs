//! Background `FreeDesktop` Trash monitor using Linux `inotify` and `/proc/self/mountinfo` (`RUSTYOS-162`).
//!
//! Runs all filesystem I/O and directory scanning on a dedicated worker thread with
//! `150 ms` event debouncing so the compositor event loop is never blocked.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Debounce window coalescing rapid Trash create/delete bursts (`RUSTYOS-162`).
pub const TRASH_MONITOR_DEBOUNCE: Duration = Duration::from_millis(150);

/// Result of a background Trash filesystem scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashScanState {
    /// `true` when at least one file exists in home Trash or any mounted external Trash.
    pub has_items: bool,
    /// Ordered set of directories to watch via `inotify` for future changes.
    pub watched_dirs: Vec<PathBuf>,
}

/// Resolves the user's primary `FreeDesktop` Trash root (`$XDG_DATA_HOME/Trash` or `$HOME/.local/share/Trash`).
#[must_use]
pub fn resolve_home_trash_root() -> Option<PathBuf> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
        && !data_home.is_empty()
    {
        return Some(PathBuf::from(data_home).join("Trash"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/Trash"))
}

/// Decodes octal escape sequences used in `/proc/self/mountinfo` mount paths (e.g. `\040` -> `' '`).
#[must_use]
pub fn unescape_mountinfo_path(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && (b'0'..=b'7').contains(&bytes[i + 1])
            && (b'0'..=b'7').contains(&bytes[i + 2])
            && (b'0'..=b'7').contains(&bytes[i + 3])
        {
            let val =
                ((bytes[i + 1] - b'0') << 6) | ((bytes[i + 2] - b'0') << 3) | (bytes[i + 3] - b'0');
            out.push(val);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses `/proc/self/mountinfo` and returns local non-system mount points that may host
/// `FreeDesktop` top-level Trash directories (`.Trash-$UID` / `.Trash/$UID`).
///
/// Virtual, pseudo, and network filesystems (NFS, CIFS, SSHFS, FUSE portals, `tmpfs`, etc.)
/// are excluded so slow or unresponsive remote mounts can never stall Trash checks.
#[must_use]
pub fn parse_mountinfo_local_mounts(mountinfo: &str) -> Vec<PathBuf> {
    const EXCLUDED_MOUNTS: &[&str] = &[
        "/",
        "/boot",
        "/boot/efi",
        "/efi",
        "/home",
        "/usr",
        "/var",
        "/tmp",
        "/dev",
        "/proc",
        "/sys",
        "/run",
        "/etc",
    ];
    let mut mounts = BTreeSet::new();
    for line in mountinfo.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let mut left_fields = left.split_whitespace();
        let Some(raw_mount_point) = left_fields.nth(4) else {
            continue;
        };
        let Some(fs_type) = right.split_whitespace().next() else {
            continue;
        };
        if !is_local_trash_filesystem(fs_type) {
            continue;
        }
        let mount_path = unescape_mountinfo_path(raw_mount_point);
        if mount_path.is_empty()
            || EXCLUDED_MOUNTS.contains(&mount_path.as_str())
            || mount_path.starts_with("/proc/")
            || mount_path.starts_with("/sys/")
            || mount_path.starts_with("/dev/")
            || mount_path.starts_with("/var/")
            || mount_path.starts_with("/snap/")
        {
            continue;
        }
        mounts.insert(PathBuf::from(mount_path));
    }
    mounts.into_iter().collect()
}

fn is_local_trash_filesystem(fs_type: &str) -> bool {
    matches!(
        fs_type,
        "ext2"
            | "ext3"
            | "ext4"
            | "btrfs"
            | "xfs"
            | "f2fs"
            | "vfat"
            | "exfat"
            | "ntfs"
            | "ntfs3"
            | "fuseblk"
            | "zfs"
            | "jfs"
            | "reiserfs"
            | "udf"
    )
}

fn dir_has_any_entry(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut iter| iter.any(|entry| entry.is_ok()))
}

/// Scans home Trash and candidate external mount Trashes without blocking on slow remote mounts.
#[must_use]
pub fn scan_trash_state(
    home_trash: Option<&Path>,
    mount_points: &[PathBuf],
    uid: u32,
) -> TrashScanState {
    let mut has_items = false;
    let mut watched = BTreeSet::new();

    if let Some(root) = home_trash {
        let files_dir = root.join("files");
        if files_dir.is_dir() {
            watched.insert(files_dir.clone());
            if dir_has_any_entry(&files_dir) {
                has_items = true;
            }
        } else if root.is_dir() {
            watched.insert(root.to_path_buf());
        } else if let Some(parent) = root.parent()
            && parent.is_dir()
        {
            watched.insert(parent.to_path_buf());
        }
    }

    for mount in mount_points {
        if !mount.is_dir() {
            continue;
        }
        let user_trash = mount.join(format!(".Trash-{uid}"));
        let shared_trash = mount.join(".Trash").join(uid.to_string());
        let mut found_trash_dir = false;
        for candidate in [user_trash, shared_trash] {
            let files_dir = candidate.join("files");
            if files_dir.is_dir() {
                found_trash_dir = true;
                watched.insert(files_dir.clone());
                if !has_items && dir_has_any_entry(&files_dir) {
                    has_items = true;
                }
            } else if candidate.is_dir() {
                found_trash_dir = true;
                watched.insert(candidate);
            }
        }
        if !found_trash_dir {
            watched.insert(mount.clone());
        }
    }

    TrashScanState {
        has_items,
        watched_dirs: watched.into_iter().collect(),
    }
}

type ChangeCallback = Arc<dyn Fn() + Send + Sync>;

/// Non-blocking handle to the background Linux `inotify` + `/proc/self/mountinfo` Trash monitor.
pub struct TrashMonitorHandle {
    has_items: Arc<AtomicBool>,
    dirty: Arc<AtomicBool>,
    on_change: Arc<Mutex<Option<ChangeCallback>>>,
    ctrl_tx: Option<linux_watch::ControlSender>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for TrashMonitorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrashMonitorHandle")
            .field("has_items", &self.has_items.load(Ordering::Relaxed))
            .field("dirty", &self.dirty.load(Ordering::Relaxed))
            .field("active", &self.thread.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for TrashMonitorHandle {
    fn default() -> Self {
        Self {
            has_items: Arc::new(AtomicBool::new(false)),
            dirty: Arc::new(AtomicBool::new(false)),
            on_change: Arc::new(Mutex::new(None)),
            ctrl_tx: None,
            thread: None,
        }
    }
}

impl Drop for TrashMonitorHandle {
    fn drop(&mut self) {
        if let Some(ref tx) = self.ctrl_tx {
            tx.send_quit();
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl TrashMonitorHandle {
    /// Starts the background Trash monitor watching `$XDG_DATA_HOME/Trash` (or `~/.local/share/Trash`)
    /// and `/proc/self/mountinfo`.
    #[must_use]
    pub fn start_background(on_change: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        Self::start_with_paths(
            resolve_home_trash_root(),
            Some(PathBuf::from("/proc/self/mountinfo")),
            linux_watch::current_uid(),
            on_change,
        )
    }

    /// Starts a Trash monitor targeting explicit paths (useful for isolated unit/integration tests).
    #[must_use]
    pub fn start_with_paths(
        home_trash: Option<PathBuf>,
        mountinfo_path: Option<PathBuf>,
        uid: u32,
        on_change: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let initial_mounts = mountinfo_path
            .as_deref()
            .and_then(|p| fs::read_to_string(p).ok())
            .map_or_default(|s| parse_mountinfo_local_mounts(&s));
        let initial = scan_trash_state(home_trash.as_deref(), &initial_mounts, uid);
        let has_items = Arc::new(AtomicBool::new(initial.has_items));
        let dirty = Arc::new(AtomicBool::new(false));
        let cb_slot = Arc::new(Mutex::new(on_change));

        let Ok((ctrl_tx, ctrl_rx)) = linux_watch::control_pipe() else {
            return Self {
                has_items,
                dirty,
                on_change: cb_slot,
                ctrl_tx: None,
                thread: None,
            };
        };

        let Ok(mut inotify) = linux_watch::InotifyWatcher::new() else {
            return Self {
                has_items,
                dirty,
                on_change: cb_slot,
                ctrl_tx: None,
                thread: None,
            };
        };
        inotify.sync_dirs(&initial.watched_dirs);
        // Re-scan once after registering watches so events between initial scan and watch setup are never missed.
        let initial = scan_trash_state(home_trash.as_deref(), &initial_mounts, uid);
        has_items.store(initial.has_items, Ordering::Release);

        let worker_has_items = Arc::clone(&has_items);
        let worker_dirty = Arc::clone(&dirty);
        let worker_cb = Arc::clone(&cb_slot);
        let thread = thread::Builder::new()
            .name("rusty-trash-mon".to_owned())
            .spawn(move || {
                run_monitor_loop(
                    inotify,
                    home_trash.as_deref(),
                    mountinfo_path.as_deref(),
                    uid,
                    initial,
                    &worker_has_items,
                    &worker_dirty,
                    &ctrl_rx,
                    &worker_cb,
                );
            })
            .ok();

        Self {
            has_items,
            dirty,
            on_change: cb_slot,
            ctrl_tx: Some(ctrl_tx),
            thread,
        }
    }

    /// Registers or replaces the reactive wake callback fired when Trash occupancy changes.
    pub fn set_on_change(&self, cb: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut guard) = self.on_change.lock() {
            *guard = Some(cb);
        }
    }

    /// Returns the latest cached Trash non-empty status without performing any filesystem I/O.
    #[must_use]
    pub fn has_items(&self) -> bool {
        self.has_items.load(Ordering::Acquire)
    }

    /// Returns `Some(has_items)` if Trash occupancy changed since the last call, else `None`.
    #[must_use]
    pub fn take_update(&self) -> Option<bool> {
        self.dirty
            .swap(false, Ordering::AcqRel)
            .then(|| self.has_items.load(Ordering::Acquire))
    }

    /// Wakes the background monitor to immediately re-scan Trash directories.
    pub fn trigger_refresh(&self) {
        if let Some(ref tx) = self.ctrl_tx {
            tx.send_refresh();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_monitor_loop(
    mut inotify: linux_watch::InotifyWatcher,
    home_trash: Option<&Path>,
    mountinfo_path: Option<&Path>,
    uid: u32,
    mut state: TrashScanState,
    has_items: &AtomicBool,
    dirty: &AtomicBool,
    ctrl_rx: &linux_watch::ControlReceiver,
    on_change: &Mutex<Option<ChangeCallback>>,
) {
    let mount_file: Option<File> = mountinfo_path.and_then(|p| File::open(p).ok());
    let mut debounce_deadline: Option<Instant> = None;

    loop {
        let timeout_ms = debounce_deadline.map_or(-1_i32, |deadline| {
            let rem = deadline.saturating_duration_since(Instant::now());
            i32::try_from(rem.as_millis()).unwrap_or(150).max(1)
        });
        let ev = linux_watch::poll_events(&inotify, mount_file.as_ref(), ctrl_rx, timeout_ms);
        if ev.quit() {
            break;
        }
        if ev.inotify_ready() {
            inotify.drain();
            debounce_deadline = Some(Instant::now() + TRASH_MONITOR_DEBOUNCE);
        }
        if ev.mountinfo_changed() {
            debounce_deadline = Some(Instant::now() + TRASH_MONITOR_DEBOUNCE);
        }
        let immediate = ev.refresh_requested();
        let debounce_fired = debounce_deadline.is_some_and(|d| Instant::now() >= d);
        if immediate || debounce_fired {
            debounce_deadline = None;
            let mounts = mountinfo_path
                .and_then(|p| fs::read_to_string(p).ok())
                .map_or_default(|s| parse_mountinfo_local_mounts(&s));
            let next = scan_trash_state(home_trash, &mounts, uid);
            if next.watched_dirs != state.watched_dirs {
                inotify.sync_dirs(&next.watched_dirs);
            }
            let prev = has_items.swap(next.has_items, Ordering::AcqRel);
            if prev != next.has_items {
                dirty.store(true, Ordering::Release);
                if let Ok(guard) = on_change.lock()
                    && let Some(cb) = guard.as_ref()
                {
                    cb();
                }
            }
            state = next;
        }
    }
}

#[allow(unsafe_code)]
mod linux_watch {
    use std::collections::HashMap;
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    const WATCH_MASK: u32 = libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO
        | libc::IN_DELETE_SELF
        | libc::IN_MOVE_SELF
        | libc::IN_ATTRIB;

    pub(super) fn current_uid() -> u32 {
        // SAFETY: `geteuid` has no preconditions and always returns the caller's effective UID.
        unsafe { libc::geteuid() }
    }

    #[derive(Debug)]
    pub(super) struct ControlSender {
        fd: OwnedFd,
    }

    impl ControlSender {
        pub(super) fn send_quit(&self) {
            self.write_byte(b'Q');
        }

        pub(super) fn send_refresh(&self) {
            self.write_byte(b'R');
        }

        fn write_byte(&self, byte: u8) {
            let buf = [byte];
            // SAFETY: `self.fd` is a valid open write end of a non-blocking pipe; `buf` is 1 byte.
            let _ = unsafe { libc::write(self.fd.as_raw_fd(), buf.as_ptr().cast(), 1) };
        }
    }

    #[derive(Debug)]
    pub(super) struct ControlReceiver {
        fd: OwnedFd,
    }

    impl ControlReceiver {
        fn drain_commands(&self) -> (bool, bool) {
            let mut quit = false;
            let mut refresh = false;
            let mut buf = [0_u8; 64];
            loop {
                // SAFETY: `self.fd` is a valid non-blocking pipe read end; `buf` is writable for `buf.len()`.
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

    pub(super) fn control_pipe() -> io::Result<(ControlSender, ControlReceiver)> {
        let mut fds = [-1_i32; 2];
        // SAFETY: `fds` points to two writable `c_int`s; `pipe2` initializes both on success.
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `pipe2` returned 0, so `fds[0]` and `fds[1]` are unique newly-owned descriptors.
        let (rx, tx) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        Ok((ControlSender { fd: tx }, ControlReceiver { fd: rx }))
    }

    #[derive(Debug)]
    pub(super) struct InotifyWatcher {
        fd: OwnedFd,
        watches: HashMap<PathBuf, i32>,
    }

    impl InotifyWatcher {
        pub(super) fn new() -> io::Result<Self> {
            // SAFETY: `inotify_init1` takes only flag bits and returns a fresh fd or -1.
            let raw = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `raw >= 0` is a unique owned inotify descriptor.
            let fd = unsafe { OwnedFd::from_raw_fd(raw) };
            Ok(Self {
                fd,
                watches: HashMap::new(),
            })
        }

        pub(super) fn sync_dirs(&mut self, desired: &[PathBuf]) {
            let stale: Vec<PathBuf> = self
                .watches
                .keys()
                .filter(|p| !desired.iter().any(|d| d == *p))
                .cloned()
                .collect();
            for path in stale {
                if let Some(wd) = self.watches.remove(&path) {
                    // SAFETY: `self.fd` is a valid inotify fd and `wd` was returned by `inotify_add_watch`.
                    let _ = unsafe { libc::inotify_rm_watch(self.fd.as_raw_fd(), wd) };
                }
            }
            for dir in desired {
                if self.watches.contains_key(dir) {
                    continue;
                }
                if let Some(wd) = self.add_watch(dir) {
                    self.watches.insert(dir.clone(), wd);
                }
            }
        }

        fn add_watch(&self, path: &Path) -> Option<i32> {
            let cpath = CString::new(path.as_os_str().as_bytes()).ok()?;
            // SAFETY: `self.fd` is a valid inotify fd and `cpath` is a NUL-terminated path string.
            let wd =
                unsafe { libc::inotify_add_watch(self.fd.as_raw_fd(), cpath.as_ptr(), WATCH_MASK) };
            (wd >= 0).then_some(wd)
        }

        pub(super) fn drain(&self) {
            let mut buf = [0_u8; 4096];
            loop {
                // SAFETY: `self.fd` is a valid non-blocking inotify fd and `buf` is 4096 writable bytes.
                let n =
                    unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n <= 0 {
                    break;
                }
            }
        }
    }

    #[derive(Debug, Default, Clone, Copy)]
    pub(super) struct PollEvents(u8);

    impl PollEvents {
        const INOTIFY_READY: u8 = 1 << 0;
        const MOUNTINFO_CHANGED: u8 = 1 << 1;
        const REFRESH_REQUESTED: u8 = 1 << 2;
        const QUIT: u8 = 1 << 3;

        pub(super) const fn inotify_ready(self) -> bool {
            self.0 & Self::INOTIFY_READY != 0
        }

        pub(super) const fn mountinfo_changed(self) -> bool {
            self.0 & Self::MOUNTINFO_CHANGED != 0
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
        mountinfo: Option<&File>,
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
            libc::pollfd {
                fd: mountinfo.map_or(-1, AsRawFd::as_raw_fd),
                events: libc::POLLPRI | libc::POLLERR,
                revents: 0,
            },
        ];
        let nfds: libc::nfds_t = if mountinfo.is_some() { 3 } else { 2 };
        // SAFETY: `fds` contains `nfds` initialized `pollfd` entries referencing open descriptors.
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), nfds, timeout_ms) };
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
        if nfds == 3 && (fds[2].revents & (libc::POLLPRI | libc::POLLERR) != 0) {
            mask |= PollEvents::MOUNTINFO_CHANGED;
        }
        PollEvents(mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn parses_mountinfo_local_mounts_and_skips_virtual_or_network_fs() {
        let sample = "\
22 1 0:21 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw
28 1 259:2 / / rw,relatime - ext4 /dev/nvme0n1p2 rw
45 28 259:1 / /boot/efi rw,relatime - vfat /dev/nvme0n1p1 rw
72 28 8:17 / /run/media/anc/Backup\\040Drive rw,nosuid,nodev - ext4 /dev/sdb1 rw
73 28 8:33 / /mnt/usb_fat rw,nosuid,nodev - exfat /dev/sdc1 rw
74 28 0:55 / /mnt/nas rw - nfs4 192.168.1.10:/share rw
75 28 0:56 / /run/user/1000/gvfs rw - fuse.gvfsd-fuse gvfsd-fuse rw
76 28 0:57 / /tmp rw - tmpfs tmpfs rw
";
        let mounts = parse_mountinfo_local_mounts(sample);
        assert_eq!(
            mounts,
            vec![
                PathBuf::from("/mnt/usb_fat"),
                PathBuf::from("/run/media/anc/Backup Drive"),
            ]
        );
    }

    #[test]
    fn scan_trash_state_detects_home_and_external_mount_trash()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("rustyos-trash-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home_trash = root.join("home_trash");
        let home_files = home_trash.join("files");
        let ext_mount = root.join("ext_usb");
        let ext_files = ext_mount.join(".Trash-1000").join("files");
        fs::create_dir_all(&home_files)?;
        fs::create_dir_all(&ext_files)?;

        let empty = scan_trash_state(Some(&home_trash), std::slice::from_ref(&ext_mount), 1000);
        assert!(!empty.has_items);
        assert!(empty.watched_dirs.contains(&home_files));
        assert!(empty.watched_dirs.contains(&ext_files));

        fs::write(ext_files.join("photo.png"), b"img")?;
        let ext_full = scan_trash_state(Some(&home_trash), std::slice::from_ref(&ext_mount), 1000);
        assert!(ext_full.has_items);

        fs::remove_file(ext_files.join("photo.png"))?;
        fs::write(home_files.join("doc.txt"), b"txt")?;
        let home_full = scan_trash_state(Some(&home_trash), &[ext_mount], 1000);
        assert!(home_full.has_items);

        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn background_inotify_monitor_reacts_live_to_file_creation_and_removal()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("rustyos-trash-live-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home_trash = root.join("Trash");
        let files_dir = home_trash.join("files");
        fs::create_dir_all(&files_dir)?;

        let wakes = Arc::new(AtomicUsize::new(0));
        let wakes_cb = Arc::clone(&wakes);
        let monitor = TrashMonitorHandle::start_with_paths(
            Some(home_trash),
            None,
            1000,
            Some(Arc::new(move || {
                wakes_cb.fetch_add(1, Ordering::AcqRel);
            })),
        );
        assert!(!monitor.has_items());

        let sample_file = files_dir.join("deleted.txt");
        fs::write(&sample_file, b"hello")?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !monitor.has_items() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            monitor.has_items(),
            "inotify monitor must detect added Trash file"
        );
        assert_eq!(monitor.take_update(), Some(true));
        assert!(wakes.load(Ordering::Acquire) >= 1);

        fs::remove_file(&sample_file)?;
        let deadline2 = Instant::now() + Duration::from_secs(2);
        while monitor.has_items() && Instant::now() < deadline2 {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            !monitor.has_items(),
            "inotify monitor must detect emptied Trash"
        );
        assert_eq!(monitor.take_update(), Some(false));

        drop(monitor);
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }
}
