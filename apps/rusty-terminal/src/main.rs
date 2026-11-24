//! RustyOS first-party GPU-accelerated Wayland CSD terminal emulator (`RUSTYOS-113`).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

mod font;
mod gpu_egl;
mod layout;
pub(crate) use rusty_app::liquid_glass;
mod pty;
mod renderer_shm;
mod state;
mod theme;
mod vt;
mod wayland;

use std::path::PathBuf;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

use crate::wayland::StartupOptions;

fn main() -> ExitCode {
    init_logging();

    let Some(opts) = parse_cli(std::env::args().skip(1)) else {
        return ExitCode::SUCCESS;
    };

    if let Err(err) = wayland::run(opts) {
        tracing::error!("rusty-terminal exited with error: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn parse_cli(args: impl Iterator<Item = String>) -> Option<StartupOptions> {
    let mut opts = StartupOptions {
        force_shm: false,
        program: None,
        program_args: Vec::new(),
        working_dir: None,
        title: None,
    };

    let mut it = args.peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "Usage: rusty-terminal [--no-gpu] [--working-directory DIR] [--title TITLE] [-e CMD [ARGS...]]"
                );
                return None;
            }
            "--no-gpu" => opts.force_shm = true,
            "--working-directory" | "--cwd" => {
                if let Some(dir) = it.next() {
                    opts.working_dir = Some(PathBuf::from(dir));
                }
            }
            "--title" | "-T" => {
                if let Some(t) = it.next() {
                    opts.title = Some(t);
                }
            }
            "-e" | "--" => {
                if let Some(cmd) = it.next() {
                    opts.program = Some(PathBuf::from(cmd));
                    opts.program_args = it.collect();
                }
                break;
            }
            _ if !arg.starts_with('-') && opts.working_dir.is_none() => {
                let path = PathBuf::from(&arg);
                if path.is_dir() {
                    opts.working_dir = Some(path);
                }
            }
            _ => {}
        }
    }

    Some(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_flags_and_exec_command() {
        let opts = parse_cli(
            [
                "--no-gpu",
                "--working-directory",
                "/tmp",
                "--title",
                "Dev",
                "-e",
                "/bin/sh",
                "-c",
                "echo hi",
            ]
            .into_iter()
            .map(str::to_owned),
        );
        assert!(opts.is_some());
        if let Some(o) = opts {
            assert!(o.force_shm);
            assert_eq!(o.working_dir, Some(PathBuf::from("/tmp")));
            assert_eq!(o.title.as_deref(), Some("Dev"));
            assert_eq!(o.program, Some(PathBuf::from("/bin/sh")));
            assert_eq!(o.program_args, vec!["-c", "echo hi"]);
        }
    }
}
