//! Privileged installer service contract & Offline `SquashFS` deployment planner (`RUSTYOS-137`, ADR-0081).
//!
//! Separates the unprivileged `rusty-installer` UI from
//! disk partitioning, `unsquashfs` live-image extraction, user/locale setup, and bootloader
//! installation with a typed JSON/line progress protocol and safe `--dry-run` simulation.

use std::path::{Path, PathBuf};

use crate::auth::is_valid_unix_username;
use crate::storage::is_valid_block_device_path;

/// Well-known Archiso / Live ISO `SquashFS` image paths probed for offline installation.
pub const LIVE_SQUASHFS_CANDIDATES: &[&str] = &[
    "/run/archiso/bootmnt/arch/x86_64/airootfs.sfs",
    "/run/archiso/copytoram/airootfs.sfs",
    "/run/archiso/sfs/airootfs/airootfs.img",
];

/// Source medium used by `rustyos-installer-service` to deploy the target OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSource {
    /// Offline extraction of the live `SquashFS` image (`unsquashfs`).
    LiveSquashFs(PathBuf),
    /// Fallback package repository installation (`scripts/rustyos-install.sh` / `pacstrap`).
    PackageRepo,
}

impl InstallSource {
    /// Human-readable label shown in the `rusty-installer` Summary screen.
    #[must_use]
    pub const fn display_label(&self) -> &'static str {
        match self {
            Self::LiveSquashFs(_) => "Live SquashFS (Offline)",
            Self::PackageRepo => "Package Repository (pacstrap)",
        }
    }
}

/// Detects an available live `SquashFS` image on the running medium (with `RUSTYOS_SQUASHFS_PATH` override).
#[must_use]
pub fn detect_live_squashfs_path() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("RUSTYOS_SQUASHFS_PATH").filter(|v| !v.is_empty()) {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Some(p);
        }
    }
    LIVE_SQUASHFS_CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// Resolves the active [`InstallSource`] for the current environment.
#[must_use]
pub fn detect_install_source() -> InstallSource {
    detect_live_squashfs_path().map_or(InstallSource::PackageRepo, InstallSource::LiveSquashFs)
}

/// Structured installation request validated and executed by `rustyos-installer-service`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerPlanRequest {
    /// Target block device path (e.g. `"/dev/vda"` or `"/dev/nvme0n1"`).
    pub target_disk: String,
    /// Primary user account name to create on the target system.
    pub username: String,
    /// Target system hostname.
    pub hostname: String,
    /// IANA timezone identifier (e.g. `"UTC"`, `"Europe/Kyiv"`).
    pub timezone: String,
    /// XKB keyboard layout code (e.g. `"us"`).
    pub keyboard_layout: String,
    /// Installation source (`LiveSquashFs` or `PackageRepo`).
    pub source: InstallSource,
    /// When `true`, simulate all stages without modifying any disk or filesystem.
    pub dry_run: bool,
}

/// Structured progress/completion event emitted by `rustyos-installer-service`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallerStageEvent {
    /// Incremental stage progress (`0..=100`).
    StageProgress {
        /// Percentage complete (`0..=100`).
        pct: u8,
        /// Machine-readable stage identifier (`validate`, `partition`, `unsquashfs`, ...).
        stage: String,
        /// Human-readable status description.
        message: String,
    },
    /// Installation finished successfully.
    Completed {
        /// Completion summary message.
        message: String,
    },
    /// Installation aborted with an error.
    Failed {
        /// Stage where the failure occurred.
        stage: String,
        /// Diagnostic error text.
        error: String,
    },
}

/// Validates an [`InstallerPlanRequest`] before any privileged disk operation begins.
///
/// # Errors
/// Returns a descriptive error string if any field fails strict safety validation.
pub fn validate_installer_request(req: &InstallerPlanRequest) -> Result<(), String> {
    if !is_valid_block_device_path(&req.target_disk) {
        return Err(format!("invalid target disk path: {}", req.target_disk));
    }
    if !is_valid_unix_username(&req.username) {
        return Err(format!("invalid username: {}", req.username));
    }
    if req.hostname.is_empty()
        || req.hostname.len() > 63
        || req.hostname.starts_with('-')
        || req.hostname.ends_with('-')
        || !req
            .hostname
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(format!("invalid hostname: {}", req.hostname));
    }
    if req.timezone.is_empty()
        || req.timezone.contains("..")
        || req.timezone.chars().any(char::is_control)
    {
        return Err(format!("invalid timezone: {}", req.timezone));
    }
    if let InstallSource::LiveSquashFs(path) = &req.source
        && !req.dry_run
        && !Path::new(path).is_file()
    {
        return Err(format!("squashfs image not found: {}", path.display()));
    }
    Ok(())
}

/// Builds the ordered offline `SquashFS` / fallback installation stages (`stage`, `pct`, `message`).
#[must_use]
pub fn build_install_stages(req: &InstallerPlanRequest) -> Vec<(&'static str, u8, String)> {
    let source_desc = match &req.source {
        InstallSource::LiveSquashFs(p) => format!("Extracting SquashFS image {}", p.display()),
        InstallSource::PackageRepo => "Installing base system packages via pacstrap".to_owned(),
    };
    vec![
        (
            "validate",
            5,
            format!("Validated target {} for {}", req.target_disk, req.username),
        ),
        (
            "partition",
            15,
            format!("Preparing GPT partition table on {}", req.target_disk),
        ),
        (
            "format",
            25,
            "Formatting EFI (FAT32) and root (ext4) filesystems".to_owned(),
        ),
        (
            "mount",
            32,
            "Mounting target root and EFI partitions".to_owned(),
        ),
        ("deploy", 68, source_desc),
        (
            "configure",
            84,
            format!(
                "Configuring hostname {}, timezone {}, keyboard {}, user {}",
                req.hostname, req.timezone, req.keyboard_layout, req.username
            ),
        ),
        (
            "bootloader",
            94,
            "Installing UEFI bootloader and generating fstab".to_owned(),
        ),
        ("cleanup", 100, "Unmounting target filesystems".to_owned()),
    ]
}

/// Encodes an [`InstallerStageEvent`] into the `STEP:` / `DONE:` / `FAIL:` line protocol.
#[must_use]
pub fn encode_installer_event(ev: &InstallerStageEvent) -> String {
    match ev {
        InstallerStageEvent::StageProgress {
            pct,
            stage,
            message,
        } => format!("STEP:{pct}:{stage}:{}", sanitize_line(message)),
        InstallerStageEvent::Completed { message } => format!("DONE:{}", sanitize_line(message)),
        InstallerStageEvent::Failed { stage, error } => {
            format!("FAIL:{stage}:{}", sanitize_line(error))
        }
    }
}

/// Parses a line emitted by `rustyos-installer-service` back into an [`InstallerStageEvent`].
#[must_use]
pub fn decode_installer_event(line: &str) -> Option<InstallerStageEvent> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("STEP:") {
        let mut parts = rest.splitn(3, ':');
        let pct = parts.next()?.parse::<u8>().ok()?.min(100);
        let stage = parts.next()?.to_owned();
        let message = parts.next().unwrap_or_default().to_owned();
        return Some(InstallerStageEvent::StageProgress {
            pct,
            stage,
            message,
        });
    }
    if let Some(msg) = trimmed.strip_prefix("DONE:") {
        return Some(InstallerStageEvent::Completed {
            message: msg.to_owned(),
        });
    }
    if let Some(rest) = trimmed.strip_prefix("FAIL:") {
        let mut parts = rest.splitn(2, ':');
        let stage = parts.next()?.to_owned();
        let error = parts.next().unwrap_or_default().to_owned();
        return Some(InstallerStageEvent::Failed { stage, error });
    }
    None
}

fn sanitize_line(raw: &str) -> String {
    raw.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_service_validates_request_encodes_events_and_builds_squashfs_plan() {
        let valid = InstallerPlanRequest {
            target_disk: "/dev/vda".to_owned(),
            username: "rusty".to_owned(),
            hostname: "rustyos-pc".to_owned(),
            timezone: "Europe/Kyiv".to_owned(),
            keyboard_layout: "us".to_owned(),
            source: InstallSource::LiveSquashFs(PathBuf::from("/run/archiso/airootfs.sfs")),
            dry_run: true,
        };
        assert!(validate_installer_request(&valid).is_ok());
        assert_eq!(valid.source.display_label(), "Live SquashFS (Offline)");

        let stages = build_install_stages(&valid);
        assert_eq!(stages.len(), 8);
        assert_eq!(stages.first().map(|s| s.0), Some("validate"));
        assert_eq!(stages.last().map(|s| s.1), Some(100));

        let mut bad_disk = valid.clone();
        bad_disk.target_disk = "/tmp/fake".to_owned();
        assert!(validate_installer_request(&bad_disk).is_err());

        let mut bad_host = valid.clone();
        bad_host.hostname = "-bad!".to_owned();
        assert!(validate_installer_request(&bad_host).is_err());

        let ev = InstallerStageEvent::StageProgress {
            pct: 68,
            stage: "deploy".to_owned(),
            message: "Extracting SquashFS".to_owned(),
        };
        assert_eq!(
            decode_installer_event(&encode_installer_event(&ev)),
            Some(ev)
        );
        let done = InstallerStageEvent::Completed {
            message: "Installed".to_owned(),
        };
        assert_eq!(
            decode_installer_event(&encode_installer_event(&done)),
            Some(done)
        );
    }
}
