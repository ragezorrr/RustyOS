//! Non-blocking PTY lifecycle, resize, and async reader/writer thread.
//!
//! Uses `rustix::pty` and `rustix::termios` without external terminal crates,
//! waking the compositor-connected Wayland event loop via `calloop::ping::Ping`.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

use std::time::{Duration, Instant};

use rustix::fs::{Mode, OFlags, fcntl_getfl, fcntl_setfl, open};
use rustix::process::{Pid, Signal, ioctl_tiocsctty, kill_process_group, setsid};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::stdio::stdin;
use rustix::termios::{Winsize, tcsetwinsize};
use smithay_client_toolkit::reexports::calloop::ping::Ping;

/// Events emitted from a background PTY thread to the UI loop.
#[derive(Debug)]
pub(crate) enum PtyEvent {
    Output { tab_id: u64, bytes: Vec<u8> },
    Exited { tab_id: u64, code: Option<i32> },
}

/// Configuration for spawning a terminal PTY child process.
#[derive(Debug, Clone)]
pub(crate) struct PtySpawnConfig {
    pub(crate) tab_id: u64,
    pub(crate) program: Option<PathBuf>,
    pub(crate) args: Vec<String>,
    pub(crate) working_dir: Option<PathBuf>,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) pixel_w: u16,
    pub(crate) pixel_h: u16,
}

/// Live PTY session owned by a terminal tab.
#[derive(Debug)]
pub(crate) struct PtySession {
    master: Arc<OwnedFd>,
    write_queue: Arc<Mutex<VecDeque<u8>>>,
    wake_tx: UnixStream,
    shutdown: Arc<AtomicBool>,
    child: Arc<Mutex<Child>>,
    worker: Option<JoinHandle<()>>,
}

impl PtySession {
    /// Spawns a shell (or explicit program) connected to a fresh POSIX PTY.
    pub(crate) fn spawn(
        cfg: &PtySpawnConfig,
        tx: mpsc::Sender<PtyEvent>,
        ping: Option<Ping>,
    ) -> io::Result<Self> {
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)
            .map_err(io::Error::from)?;
        grantpt(&master).map_err(io::Error::from)?;
        unlockpt(&master).map_err(io::Error::from)?;
        let slave_name = ptsname(&master, Vec::new()).map_err(io::Error::from)?;

        set_winsize(&master, cfg.cols, cfg.rows, cfg.pixel_w, cfg.pixel_h)?;

        let slave_fd = open(
            slave_name.as_c_str(),
            OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;

        let slave_in = slave_fd.try_clone()?;
        let slave_out = slave_fd.try_clone()?;
        let slave_err = slave_fd;

        let program = resolve_program(cfg.program.as_deref());
        let mut cmd = Command::new(&program);
        cmd.args(&cfg.args);
        if let Some(dir) = &cfg.working_dir
            && dir.is_dir()
        {
            cmd.current_dir(dir);
        }
        cmd.env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("TERM_PROGRAM", "RustyTerminal")
            .env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"))
            .stdin(Stdio::from(slave_in))
            .stdout(Stdio::from(slave_out))
            .stderr(Stdio::from(slave_err));

        configure_controlling_tty(&mut cmd);
        let child = cmd.spawn()?;

        set_nonblocking(&master)?;

        let (wake_tx, wake_rx) = UnixStream::pair()?;
        wake_tx.set_nonblocking(true)?;
        wake_rx.set_nonblocking(true)?;

        let master = Arc::new(master);
        let write_queue = Arc::new(Mutex::new(VecDeque::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(child));

        let worker = spawn_io_thread(
            cfg.tab_id,
            Arc::clone(&master),
            Arc::clone(&write_queue),
            wake_rx,
            Arc::clone(&shutdown),
            Arc::clone(&child),
            tx,
            ping,
        )?;

        Ok(Self {
            master,
            write_queue,
            wake_tx,
            shutdown,
            child,
            worker: Some(worker),
        })
    }

    /// Queues bytes to be written asynchronously to the PTY master.
    pub(crate) fn write_bytes(&mut self, data: &[u8]) {
        if data.is_empty() || self.shutdown.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut queue) = self.write_queue.lock() {
            queue.extend(data);
        }
        let _ = self.wake_tx.write(b"w");
    }

    /// Updates PTY kernel window dimensions and sends `SIGWINCH` to foreground group.
    pub(crate) fn resize(&self, cols: u16, rows: u16, pixel_w: u16, pixel_h: u16) {
        let _ = set_winsize(&self.master, cols, rows, pixel_w, pixel_h);
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.wake_tx.write(b"q");
        let worker = self.worker.take();
        let child_arc = Arc::clone(&self.child);
        let pgid = child_arc
            .lock()
            .ok()
            .and_then(|c| i32::try_from(c.id()).ok())
            .and_then(Pid::from_raw);
        if let Some(pgid) = pgid {
            // Gracefully notify the foreground shell session group so shells flush history.
            let _ = kill_process_group(pgid, Signal::HUP);
        }
        // Reap worker and child off the UI thread so closing a tab never stalls Wayland dispatch.
        let _ = thread::Builder::new()
            .name(String::from("rusty-term-reaper"))
            .spawn(move || {
                if let Some(handle) = worker {
                    let _ = handle.join();
                }
                let deadline = Instant::now() + Duration::from_millis(250);
                if let Ok(mut child) = child_arc.lock() {
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) | Err(_) => return,
                            Ok(None) if Instant::now() >= deadline => break,
                            Ok(None) => thread::sleep(Duration::from_millis(15)),
                        }
                    }
                    if let Some(pgid) = pgid {
                        let _ = kill_process_group(pgid, Signal::KILL);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                }
            });
    }
}

fn set_winsize<Fd: AsFd>(
    fd: Fd,
    cols: u16,
    rows: u16,
    pixel_w: u16,
    pixel_h: u16,
) -> io::Result<()> {
    tcsetwinsize(
        fd,
        Winsize {
            ws_row: rows.max(1),
            ws_col: cols.max(1),
            ws_xpixel: pixel_w,
            ws_ypixel: pixel_h,
        },
    )
    .map_err(io::Error::from)
}

fn set_nonblocking<Fd: AsFd>(fd: Fd) -> io::Result<()> {
    let flags = fcntl_getfl(&fd).map_err(io::Error::from)?;
    fcntl_setfl(&fd, flags | OFlags::NONBLOCK).map_err(io::Error::from)
}

#[allow(clippy::too_many_arguments)]
fn spawn_io_thread(
    tab_id: u64,
    master: Arc<OwnedFd>,
    write_queue: Arc<Mutex<VecDeque<u8>>>,
    mut wake_rx: UnixStream,
    shutdown: Arc<AtomicBool>,
    child: Arc<Mutex<Child>>,
    tx: mpsc::Sender<PtyEvent>,
    ping: Option<Ping>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name(format!("rusty-pty-{tab_id}"))
        .spawn(move || {
            let Ok(master_dup) = master.try_clone() else {
                return;
            };
            let mut master_file = File::from(master_dup);
            let mut buf = [0_u8; 16384];
            let mut exit_notified = false;

            while !shutdown.load(Ordering::Relaxed) {
                let has_pending_write = write_queue.lock().is_ok_and(|q| !q.is_empty());

                let (master_revents, wake_revents) = poll_two_fds(
                    master_file.as_raw_fd(),
                    has_pending_write,
                    wake_rx.as_raw_fd(),
                );

                if wake_revents & libc::POLLIN != 0 {
                    let mut drain = [0_u8; 64];
                    while let Ok(n) = wake_rx.read(&mut drain) {
                        if n == 0 || drain[..n].contains(&b'q') {
                            break;
                        }
                    }
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                }

                if master_revents & libc::POLLOUT != 0 {
                    drain_write_queue(&mut master_file, &write_queue);
                }

                if master_revents & libc::POLLIN != 0 {
                    if !read_available_pty(&mut master_file, &mut buf, tab_id, &tx, ping.as_ref()) {
                        notify_child_exit(tab_id, &child, &tx, ping.as_ref());
                        exit_notified = true;
                        break;
                    }
                } else if master_revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                    // Drain any final bytes buffered in the line discipline before closing.
                    let _ =
                        read_available_pty(&mut master_file, &mut buf, tab_id, &tx, ping.as_ref());
                    notify_child_exit(tab_id, &child, &tx, ping.as_ref());
                    exit_notified = true;
                    break;
                }
            }

            if !exit_notified && !shutdown.load(Ordering::Relaxed) {
                notify_child_exit(tab_id, &child, &tx, ping.as_ref());
            }
        })
}

fn read_available_pty(
    master: &mut File,
    buf: &mut [u8],
    tab_id: u64,
    tx: &mpsc::Sender<PtyEvent>,
    ping: Option<&Ping>,
) -> bool {
    let mut read_any = false;
    loop {
        match master.read(buf) {
            Ok(0) => {
                if read_any && let Some(p) = ping {
                    p.ping();
                }
                return false;
            }
            Ok(n) => {
                read_any = true;
                if tx
                    .send(PtyEvent::Output {
                        tab_id,
                        bytes: buf[..n].to_vec(),
                    })
                    .is_err()
                {
                    return false;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                if read_any && let Some(p) = ping {
                    p.ping();
                }
                return true;
            }
            Err(_) => {
                if read_any && let Some(p) = ping {
                    p.ping();
                }
                return false;
            }
        }
    }
}

fn drain_write_queue(master: &mut File, queue: &Mutex<VecDeque<u8>>) {
    loop {
        let chunk: Vec<u8> = {
            let Ok(guard) = queue.lock() else {
                return;
            };
            if guard.is_empty() {
                return;
            }
            guard.iter().take(4096).copied().collect()
        };
        match master.write(&chunk) {
            Ok(0) | Err(_) => return,
            Ok(written) => {
                if let Ok(mut guard) = queue.lock() {
                    for _ in 0..written.min(guard.len()) {
                        guard.pop_front();
                    }
                }
            }
        }
    }
}

fn notify_child_exit(
    tab_id: u64,
    child: &Mutex<Child>,
    tx: &mpsc::Sender<PtyEvent>,
    ping: Option<&Ping>,
) {
    let code = child
        .lock()
        .ok()
        .and_then(|mut c| c.try_wait().ok().flatten().or_else(|| c.wait().ok()))
        .and_then(|status| status.code());
    let _ = tx.send(PtyEvent::Exited { tab_id, code });
    if let Some(p) = ping {
        p.ping();
    }
}

/// Safe wrapper invoking `setsid()` + `TIOCSCTTY` in the forked child before `exec`.
#[allow(unsafe_code)]
fn configure_controlling_tty(cmd: &mut Command) {
    // SAFETY: `pre_exec` runs strictly in the forked child before `exec` and invokes
    // only async-signal-safe session/controlling-TTY setup syscalls via `rustix`.
    unsafe {
        cmd.pre_exec(|| {
            setsid().map_err(io::Error::from)?;
            let fd: BorrowedFd<'_> = stdin();
            ioctl_tiocsctty(fd).map_err(io::Error::from)?;
            Ok(())
        });
    }
}

/// Safe wrapper around `libc::poll` over `(pty_master, wake_rx)`.
#[allow(unsafe_code)]
fn poll_two_fds(master_fd: i32, want_write: bool, wake_fd: i32) -> (i16, i16) {
    let master_events = libc::POLLIN | if want_write { libc::POLLOUT } else { 0 };
    let mut fds = [
        libc::pollfd {
            fd: master_fd,
            events: master_events,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // SAFETY: `fds` points to a valid two-element stack array of initialized `pollfd`.
    let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
    if rc < 0 {
        return (0, 0);
    }
    (fds[0].revents, fds[1].revents)
}

fn resolve_program(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Ok(shell) = std::env::var("SHELL") {
        let p = PathBuf::from(&shell);
        if !shell.is_empty() && p.is_file() {
            return p;
        }
    }
    for candidate in [
        "/bin/zsh",
        "/usr/bin/zsh",
        "/bin/bash",
        "/usr/bin/bash",
        "/bin/sh",
    ] {
        let p = Path::new(candidate);
        if p.is_file() {
            return p.to_path_buf();
        }
    }
    PathBuf::from("/bin/sh")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::time::{Duration, Instant};

    #[test]
    fn spawns_pty_reads_output_and_handles_resize() -> Result<(), Box<dyn Error>> {
        let (tx, rx) = mpsc::channel();
        let cfg = PtySpawnConfig {
            tab_id: 1,
            program: Some(PathBuf::from("/bin/sh")),
            args: vec![
                "-c".to_owned(),
                "printf 'rusty-pty-ok\\n'; exit 0".to_owned(),
            ],
            working_dir: None,
            cols: 80,
            rows: 24,
            pixel_w: 800,
            pixel_h: 480,
        };
        let session = PtySession::spawn(&cfg, tx, None)?;
        session.resize(100, 30, 1000, 600);

        let mut collected = String::new();
        let mut exited = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && (!exited || !collected.contains("rusty-pty-ok")) {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(50)) {
                match ev {
                    PtyEvent::Output { bytes, .. } => {
                        collected.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    PtyEvent::Exited { code, .. } => {
                        assert_eq!(code, Some(0));
                        exited = true;
                    }
                }
            }
        }
        assert!(
            collected.contains("rusty-pty-ok"),
            "output was: {collected:?}"
        );
        assert!(exited, "expected clean PTY exit event");
        Ok(())
    }
}
