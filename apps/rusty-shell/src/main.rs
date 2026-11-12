//! Binary entry point for `rusty-shell` (`RUSTYOS-010`).

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use rusty_ipc::{ShellRequest, Urgency};
use rusty_shell::DesktopShell;

fn main() -> ExitCode {
    let mut ipc_socket: Option<PathBuf> = env::var("RUSTYOS_IPC_SOCKET").ok().map(PathBuf::from);
    let mut commands: Vec<ShellRequest> = Vec::new();
    let mut run_wayland_daemon = false;
    let mut shell = DesktopShell::default();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ipc-socket" => {
                let Some(path) = args.next() else {
                    eprintln!("rusty-shell error: --ipc-socket requires a path");
                    return ExitCode::from(2);
                };
                ipc_socket = Some(PathBuf::from(path));
            }
            "--switch-workspace" => {
                let Some(ws_str) = args.next() else {
                    eprintln!("rusty-shell error: --switch-workspace requires 1..=9");
                    return ExitCode::from(2);
                };
                let Ok(workspace) = ws_str.parse::<u8>() else {
                    eprintln!("rusty-shell error: invalid workspace '{ws_str}'");
                    return ExitCode::from(2);
                };
                commands.push(ShellRequest::SwitchWorkspace { workspace });
            }
            "--toggle-overview" => commands.push(ShellRequest::ToggleOverview),
            "--toggle-layout" => commands.push(ShellRequest::ToggleLayoutMode),
            "--reduced-motion" => commands.push(ShellRequest::SetReducedMotion { enabled: true }),
            "--lock" => commands.push(ShellRequest::SetLockScreen { locked: true }),
            "--unlock" => commands.push(ShellRequest::SetLockScreen { locked: false }),
            "--notify" => {
                let title = args.next().unwrap_or_else(|| "RustyOS".to_owned());
                let body = args
                    .next()
                    .unwrap_or_else(|| "Shell notification".to_owned());
                let _ = shell.notifications.push(
                    Urgency::Normal,
                    title.clone(),
                    body.clone(),
                    Duration::ZERO,
                );
                commands.push(ShellRequest::Notify {
                    urgency: Urgency::Normal,
                    title,
                    body,
                });
            }
            "--status-hud" => {}
            "--wayland" | "--daemon" => run_wayland_daemon = true,
            other => {
                eprintln!("rusty-shell error: unknown argument '{other}'");
                return ExitCode::from(2);
            }
        }
    }

    if let Some(socket_path) = &ipc_socket {
        match shell.sync_with_compositor(socket_path, &commands, Duration::from_millis(16)) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("rusty-shell IPC error: {err}");
                return ExitCode::FAILURE;
            }
        }
    }

    if run_wayland_daemon {
        match rusty_shell::wayland::ShellWaylandClient::connect(shell.clone()) {
            Ok(mut client) => {
                let _ = client.draw_surfaces();
                println!("rusty-shell: connected to Wayland display, Layer-Shell surfaces mapped");
                while client.data.running {
                    if let Some(socket_path) = &ipc_socket {
                        let _ =
                            shell.sync_with_compositor(socket_path, &[], Duration::from_millis(50));
                        client.update_state(shell.clone());
                    }
                    let _ = client.dispatch_pending();
                    let _ = client.draw_surfaces();
                    std::thread::sleep(Duration::from_millis(16));
                }
                return ExitCode::SUCCESS;
            }
            Err(err) => {
                eprintln!("rusty-shell error: failed to connect to Wayland: {err}");
                return ExitCode::FAILURE;
            }
        }
    }

    print!("{}", shell.render_hud_summary());
    ExitCode::SUCCESS
}
