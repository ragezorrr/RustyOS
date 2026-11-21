//! Removable storage discovery, mounting, and safe ejection (`UDisks2` / `lsblk`, `RUSTYOS-134`).

use std::path::PathBuf;
use std::process::Command;

/// Removable/hotplug storage volume discovered for `rusty-files` Locations (`RUSTYOS-134`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovableVolume {
    /// Block device path (e.g. `"/dev/sdb1"`).
    pub device: String,
    /// Human-friendly volume label or device basename.
    pub label: String,
    /// Active mount point if mounted.
    pub mount_point: Option<PathBuf>,
    /// Partition/volume size in bytes.
    pub size_bytes: u64,
    /// True when the device is removable or hotplug.
    pub removable: bool,
    /// True when safe ejection (`unmount` / `power-off`) is supported.
    pub ejectable: bool,
}

/// Validates that `device` is a safe `/dev/...` block device path without shell metacharacters.
#[must_use]
pub fn is_valid_block_device_path(device: &str) -> bool {
    let Some(rest) = device.strip_prefix("/dev/") else {
        return false;
    };
    !rest.is_empty()
        && !rest.contains("..")
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/'))
}

/// Parses `lsblk -P -b -o NAME,LABEL,MOUNTPOINT,SIZE,RM,HOTPLUG,TYPE` key-value lines into removable volumes.
#[must_use]
pub fn parse_lsblk_removable_volumes(raw: &str) -> Vec<RemovableVolume> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let name = extract_lsblk_field(trimmed, "NAME");
        let label = extract_lsblk_field(trimmed, "LABEL");
        let mount = extract_lsblk_field(trimmed, "MOUNTPOINT");
        let size_str = extract_lsblk_field(trimmed, "SIZE");
        let rm = extract_lsblk_field(trimmed, "RM");
        let hotplug = extract_lsblk_field(trimmed, "HOTPLUG");
        let dev_type = extract_lsblk_field(trimmed, "TYPE");

        let is_removable = rm == "1" || hotplug == "1";
        if !is_removable {
            continue;
        }
        if dev_type != "part" && (dev_type != "disk" || mount.is_empty()) {
            continue;
        }
        if mount == "/" || mount.starts_with("/boot") {
            continue;
        }
        let device = if name.starts_with("/dev/") {
            name.clone()
        } else {
            format!("/dev/{name}")
        };
        if !is_valid_block_device_path(&device) {
            continue;
        }
        let display_label = if !label.is_empty() {
            label
        } else if !mount.is_empty() {
            PathBuf::from(&mount)
                .file_name()
                .and_then(|f| f.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(&name)
                .to_owned()
        } else {
            name
        };
        let mount_point = (!mount.is_empty()).then(|| PathBuf::from(&mount));
        let size_bytes = size_str.parse::<u64>().unwrap_or(0);
        out.push(RemovableVolume {
            device,
            label: display_label,
            mount_point,
            size_bytes,
            removable: true,
            ejectable: true,
        });
    }
    out
}

fn extract_lsblk_field(line: &str, key: &str) -> String {
    let prefix = format!("{key}=\"");
    let Some(start) = line.find(&prefix) else {
        return String::new();
    };
    let rest = &line[start + prefix.len()..];
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                value.push(escaped);
            }
        } else if ch == '"' {
            break;
        } else {
            value.push(ch);
        }
    }
    value
}

/// Non-blocking-friendly removable storage adapter (`lsblk` + `udisksctl` / `UDisks2`, `RUSTYOS-134`).
#[derive(Debug, Default, Clone, Copy)]
pub struct StorageService;

impl StorageService {
    /// Lists removable/hotplug volumes available to the user.
    #[must_use]
    pub fn list_removable_volumes() -> Vec<RemovableVolume> {
        let Ok(output) = Command::new("lsblk")
            .args([
                "-P",
                "-b",
                "-o",
                "NAME,LABEL,MOUNTPOINT,SIZE,RM,HOTPLUG,TYPE",
            ])
            .output()
        else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        parse_lsblk_removable_volumes(&String::from_utf8_lossy(&output.stdout))
    }

    /// Mounts a removable block device via `udisksctl mount` (least-privilege `UDisks2`).
    ///
    /// # Errors
    /// Returns an error if `device` fails validation, `udisksctl` is unavailable, or mounting fails.
    pub fn mount_volume(device: &str) -> Result<PathBuf, String> {
        if !is_valid_block_device_path(device) {
            return Err("invalid block device path".to_owned());
        }
        let output = Command::new("udisksctl")
            .args(["mount", "-b", device, "--no-user-interaction"])
            .output()
            .map_err(|err| format!("udisksctl unavailable: {err}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(if stderr.is_empty() {
                format!("failed to mount {device}")
            } else {
                stderr
            });
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(idx) = stdout.find(" at ") {
            let mp = stdout[idx + 4..].trim().trim_end_matches('.');
            if !mp.is_empty() {
                return Ok(PathBuf::from(mp));
            }
        }
        Self::list_removable_volumes()
            .into_iter()
            .find(|v| v.device == device)
            .and_then(|v| v.mount_point)
            .ok_or_else(|| format!("mounted {device}, but mount point could not be resolved"))
    }

    /// Safely unmounts and powers off (best-effort) a removable block device via `udisksctl`.
    ///
    /// # Errors
    /// Returns an error if `device` fails validation, `udisksctl` is unavailable, or unmounting fails.
    pub fn unmount_and_eject(device: &str) -> Result<String, String> {
        if !is_valid_block_device_path(device) {
            return Err("invalid block device path".to_owned());
        }
        let output = Command::new("udisksctl")
            .args(["unmount", "-b", device, "--no-user-interaction"])
            .output()
            .map_err(|err| format!("udisksctl unavailable: {err}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(if stderr.is_empty() {
                format!("failed to unmount {device}")
            } else {
                stderr
            });
        }
        let _ = Command::new("udisksctl")
            .args(["power-off", "-b", device, "--no-user-interaction"])
            .output();
        Ok(format!("Ejected {device}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_service_parses_lsblk_removable_volumes_and_validates_device_paths() {
        assert!(is_valid_block_device_path("/dev/sdb1"));
        assert!(is_valid_block_device_path("/dev/nvme0n1p1"));
        assert!(!is_valid_block_device_path("/dev/../etc/passwd"));
        assert!(!is_valid_block_device_path("/tmp/fake"));
        assert!(!is_valid_block_device_path("/dev/sdb1;rm -rf /"));

        let sample = r#"
NAME="nvme0n1p2" LABEL="Root" MOUNTPOINT="/" SIZE="256000000000" RM="0" HOTPLUG="0" TYPE="part"
NAME="sdb1" LABEL="RUSTY_USB" MOUNTPOINT="/run/media/anc/RUSTY_USB" SIZE="32000000000" RM="1" HOTPLUG="1" TYPE="part"
NAME="sdc1" LABEL="" MOUNTPOINT="" SIZE="16000000000" RM="1" HOTPLUG="1" TYPE="part"
NAME="sdd" LABEL="RAW" MOUNTPOINT="" SIZE="8000000000" RM="1" HOTPLUG="1" TYPE="disk"
"#;
        let vols = parse_lsblk_removable_volumes(sample);
        assert_eq!(vols.len(), 2);
        assert_eq!(vols[0].device, "/dev/sdb1");
        assert_eq!(vols[0].label, "RUSTY_USB");
        assert_eq!(
            vols[0].mount_point,
            Some(PathBuf::from("/run/media/anc/RUSTY_USB"))
        );
        assert_eq!(vols[1].device, "/dev/sdc1");
        assert_eq!(vols[1].label, "sdc1");
        assert_eq!(vols[1].mount_point, None);
    }
}
