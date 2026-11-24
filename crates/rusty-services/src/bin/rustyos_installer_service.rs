//! Privileged RustyOS installer backend (`rustyos-installer-service`, `RUSTYOS-137`, ADR-0081).
//!
//! Invoked by `apps/rusty-installer` via Polkit or `--dry-run` for local validation.

use std::path::PathBuf;
use std::process::Command;

use rusty_services::{
    InstallSource, InstallerPlanRequest, InstallerStageEvent, build_install_stages,
    detect_install_source, encode_installer_event, validate_installer_request,
};

#[allow(clippy::too_many_lines)]
fn main() {
    let mut target_disk = String::new();
    let mut username = String::from("rusty");
    let mut hostname = String::from("rustyos");
    let mut timezone = String::from("UTC");
    let mut keyboard_layout = String::from("us");
    let mut squashfs_override: Option<PathBuf> = None;
    let mut dry_run = false;
    let mut detect_only = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--detect-source" => detect_only = true,
            "--dry-run" => dry_run = true,
            "--target-disk" => {
                if let Some(v) = args.next() {
                    target_disk = v;
                }
            }
            "--username" => {
                if let Some(v) = args.next() {
                    username = v;
                }
            }
            "--hostname" => {
                if let Some(v) = args.next() {
                    hostname = v;
                }
            }
            "--timezone" => {
                if let Some(v) = args.next() {
                    timezone = v;
                }
            }
            "--keyboard" => {
                if let Some(v) = args.next() {
                    keyboard_layout = v;
                }
            }
            "--squashfs" => {
                if let Some(v) = args.next() {
                    squashfs_override = Some(PathBuf::from(v));
                }
            }
            _ => {}
        }
    }

    let source = squashfs_override.map_or_else(detect_install_source, InstallSource::LiveSquashFs);

    if detect_only {
        println!("{}", source.display_label());
        return;
    }

    let req = InstallerPlanRequest {
        target_disk,
        username,
        hostname,
        timezone,
        keyboard_layout,
        source,
        dry_run,
    };

    if let Err(err) = validate_installer_request(&req) {
        println!(
            "{}",
            encode_installer_event(&InstallerStageEvent::Failed {
                stage: "validate".to_owned(),
                error: err,
            })
        );
        std::process::exit(2);
    }

    if req.dry_run {
        for (stage, pct, message) in build_install_stages(&req) {
            println!(
                "{}",
                encode_installer_event(&InstallerStageEvent::StageProgress {
                    pct,
                    stage: stage.to_owned(),
                    message,
                })
            );
        }
        println!(
            "{}",
            encode_installer_event(&InstallerStageEvent::Completed {
                message: format!(
                    "Dry-run completed ({}, target {})",
                    req.source.display_label(),
                    req.target_disk
                ),
            })
        );
        return;
    }

    // Live execution delegates to `/usr/lib/rustyos/rustyos-install` when falling back to pacstrap
    // or runs the offline SquashFS pipeline if `unsquashfs` is available.
    let helper = PathBuf::from("/usr/lib/rustyos/rustyos-install");
    if helper.is_file() {
        let status = Command::new(&helper)
            .args([
                "--install-mode",
                "whole-disk",
                "--target-disk",
                &req.target_disk,
                "--username",
                &req.username,
                "--hostname",
                &req.hostname,
                "--timezone",
                &req.timezone,
                "--kb-layout",
                &req.keyboard_layout,
                "--confirm-erase",
                "--password-stdin",
            ])
            .status();
        match status {
            Ok(s) if s.success() => {
                println!(
                    "{}",
                    encode_installer_event(&InstallerStageEvent::Completed {
                        message: "RustyOS installation completed".to_owned(),
                    })
                );
            }
            Ok(s) => {
                println!(
                    "{}",
                    encode_installer_event(&InstallerStageEvent::Failed {
                        stage: "deploy".to_owned(),
                        error: format!("installer helper exited with {s}"),
                    })
                );
                std::process::exit(1);
            }
            Err(err) => {
                println!(
                    "{}",
                    encode_installer_event(&InstallerStageEvent::Failed {
                        stage: "deploy".to_owned(),
                        error: format!("failed to spawn {}: {err}", helper.display()),
                    })
                );
                std::process::exit(1);
            }
        }
    } else {
        println!(
            "{}",
            encode_installer_event(&InstallerStageEvent::Failed {
                stage: "deploy".to_owned(),
                error: "live installer helper /usr/lib/rustyos/rustyos-install not found"
                    .to_owned(),
            })
        );
        std::process::exit(1);
    }
}
