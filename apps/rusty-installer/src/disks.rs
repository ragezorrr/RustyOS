//! Target block device discovery and `lsblk -J` parsing for `rusty-installer` (`RUSTYOS-115`).

use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TargetDiskInfo {
    pub path: String,
    pub model: String,
    pub size_bytes: u64,
    pub size_label: String,
    pub transport: String,
    pub removable: bool,
    pub mounted: bool,
    pub mount_hint: Option<String>,
}

impl TargetDiskInfo {
    #[must_use]
    pub(crate) fn is_selectable(&self) -> bool {
        !self.mounted && self.size_bytes >= 8_000_000_000
    }
}

#[must_use]
#[allow(clippy::cast_precision_loss)]
pub(crate) fn format_bytes_human(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    const TB: f64 = 1_000_000_000_000.0;
    const MB: f64 = 1_000_000.0;
    let f = bytes as f64;
    if f >= TB {
        format!("{:.1} TB", f / TB)
    } else if f >= GB {
        format!("{:.0} GB", f / GB)
    } else if f >= MB {
        format!("{:.0} MB", f / MB)
    } else {
        format!("{bytes} B")
    }
}

#[must_use]
pub(crate) fn demo_target_disks() -> Vec<TargetDiskInfo> {
    vec![
        TargetDiskInfo {
            path: String::from("/dev/vda"),
            model: String::from("RustyOS NVMe SSD"),
            size_bytes: 512_110_190_592,
            size_label: format_bytes_human(512_110_190_592),
            transport: String::from("nvme"),
            removable: false,
            mounted: false,
            mount_hint: None,
        },
        TargetDiskInfo {
            path: String::from("/dev/sda"),
            model: String::from("Secondary SATA SSD"),
            size_bytes: 1_000_204_886_016,
            size_label: format_bytes_human(1_000_204_886_016),
            transport: String::from("sata"),
            removable: false,
            mounted: false,
            mount_hint: None,
        },
        TargetDiskInfo {
            path: String::from("/dev/sdb"),
            model: String::from("RustyOS Live USB"),
            size_bytes: 15_600_000_000,
            size_label: format_bytes_human(15_600_000_000),
            transport: String::from("usb"),
            removable: true,
            mounted: true,
            mount_hint: Some(String::from("/run/archiso/bootmnt")),
        },
    ]
}

#[must_use]
pub(crate) fn discover_target_disks(demo_mode: bool) -> Vec<TargetDiskInfo> {
    if demo_mode {
        return demo_target_disks();
    }
    let out = Command::new("lsblk")
        .args([
            "-Jpb",
            "-o",
            "NAME,KNAME,MODEL,SIZE,TYPE,TRAN,RM,MOUNTPOINT,MOUNTPOINTS",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let parsed = parse_lsblk_json(&String::from_utf8_lossy(&o.stdout));
            if parsed.is_empty() {
                demo_target_disks()
            } else {
                parsed
            }
        }
        _ => demo_target_disks(),
    }
}

/// Parses `lsblk -Jpb` JSON without pulling any external JSON crate.
#[must_use]
pub(crate) fn parse_lsblk_json(raw: &str) -> Vec<TargetDiskInfo> {
    let Some(arr_pos) = raw.find("\"blockdevices\"") else {
        return Vec::new();
    };
    let Some(open_bracket) = raw[arr_pos..].find('[').map(|rel| arr_pos + rel) else {
        return Vec::new();
    };
    let Some(arr_slice) = extract_balanced_slice(raw, open_bracket, b'[', b']') else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for obj in split_top_level_objects(arr_slice) {
        if extract_json_str(obj, "type").as_deref() != Some("disk") {
            continue;
        }
        let Some(name_or_kname) =
            extract_json_str(obj, "kname").or_else(|| extract_json_str(obj, "name"))
        else {
            continue;
        };
        let path = if name_or_kname.starts_with('/') {
            name_or_kname
        } else {
            format!("/dev/{name_or_kname}")
        };
        let size_bytes = extract_json_u64(obj, "size").unwrap_or(0);
        let tran = extract_json_str(obj, "tran").unwrap_or_else(|| {
            if path.contains("nvme") {
                String::from("nvme")
            } else if path.contains("vd") {
                String::from("virtio")
            } else {
                String::from("disk")
            }
        });
        let model = extract_json_str(obj, "model")
            .map(|m| m.trim().to_owned())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| format!("{} Drive", tran.to_ascii_uppercase()));
        let removable = extract_json_bool(obj, "rm").unwrap_or(false);
        let mount_hint = collect_nonempty_mountpoints(obj);
        let mounted = mount_hint.is_some();

        out.push(TargetDiskInfo {
            path,
            model,
            size_bytes,
            size_label: format_bytes_human(size_bytes),
            transport: tran,
            removable,
            mounted,
            mount_hint,
        });
    }
    out
}

fn extract_balanced_slice(text: &str, start: usize, open: u8, close: u8) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.get(start).copied() != Some(open) {
        return None;
    }
    let mut depth = 0_usize;
    let mut in_str = false;
    let mut escaped = false;
    for (idx, &b) in bytes.iter().enumerate().skip(start) {
        if escaped {
            escaped = false;
            continue;
        }
        if b == b'\\' && in_str {
            escaped = true;
            continue;
        }
        if b == b'"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(&text[start..=idx]);
            }
        }
    }
    None
}

fn split_top_level_objects(array_slice: &str) -> Vec<&str> {
    let bytes = array_slice.as_bytes();
    let mut objects = Vec::new();
    let mut obj_start: Option<usize> = None;
    let mut depth = 0_usize;
    let mut in_str = false;
    let mut escaped = false;

    for (idx, &b) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if b == b'\\' && in_str {
            escaped = true;
            continue;
        }
        if b == b'"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if b == b'{' {
            if depth == 1 && obj_start.is_none() {
                obj_start = Some(idx);
            }
            depth += 1;
        } else if b == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 1
                && let Some(start) = obj_start.take()
            {
                objects.push(&array_slice[start..=idx]);
            }
        } else if b == b'[' && depth == 0 {
            depth = 1;
        }
    }
    objects
}

fn find_top_level_value<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let bytes = obj.as_bytes();
    let needle = format!("\"{key}\"");
    let mut depth = 0_usize;
    let mut in_str = false;
    let mut escaped = false;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' && in_str {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            if depth == 1 && !in_str && obj[i..].starts_with(&needle) {
                let after = obj[i + needle.len()..].trim_start();
                if let Some(rest) = after.strip_prefix(':') {
                    return Some(rest.trim_start());
                }
            }
            in_str = !in_str;
            i += 1;
            continue;
        }
        if !in_str {
            match b {
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn extract_json_str(obj: &str, key: &str) -> Option<String> {
    let val = find_top_level_value(obj, key)?;
    let inner = val.strip_prefix('"')?;
    let mut out = String::new();
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            out.push(match ch {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn extract_json_u64(obj: &str, key: &str) -> Option<u64> {
    let val = find_top_level_value(obj, key)?;
    if let Some(s) = val.strip_prefix('"') {
        let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
        return digits.parse().ok();
    }
    let digits: String = val.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn extract_json_bool(obj: &str, key: &str) -> Option<bool> {
    let val = find_top_level_value(obj, key)?;
    if val.starts_with("true") {
        Some(true)
    } else if val.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn collect_nonempty_mountpoints(obj: &str) -> Option<String> {
    let mut idx = 0;
    while let Some(pos) = obj[idx..].find("\"mountpoint") {
        let rest = &obj[idx + pos..];
        let Some(colon) = rest.find(':') else {
            break;
        };
        let after = rest[colon + 1..].trim_start();
        if let Some(after_q) = after.strip_prefix('"')
            && let Some(end_q) = after_q.find('"')
        {
            let mp = &after_q[..end_q];
            if !mp.is_empty() {
                return Some(mp.to_owned());
            }
        } else if let Some(arr_slice) = extract_balanced_slice(after, 0, b'[', b']') {
            for part in arr_slice.split('"') {
                if part.starts_with('/') {
                    return Some(part.to_owned());
                }
            }
        }
        idx += pos + 11;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsblk_json_and_detects_mounted_host_disk() {
        let sample = r#"{
          "blockdevices": [
            {
              "name": "loop0",
              "kname": "loop0",
              "model": null,
              "size": 780000000,
              "type": "loop",
              "tran": null,
              "rm": false,
              "mountpoint": "/run/archiso/airootfs"
            },
            {
              "name": "nvme0n1",
              "kname": "nvme0n1",
              "model": "Samsung SSD 990 PRO 1TB ",
              "size": 1000204886016,
              "type": "disk",
              "tran": "nvme",
              "rm": false,
              "mountpoints": [null],
              "children": [
                {"name": "nvme0n1p1", "size": 536870912, "type": "part", "mountpoints": ["/boot"]},
                {"name": "nvme0n1p2", "size": 999660000000, "type": "part", "mountpoints": ["/"]}
              ]
            },
            {
              "name": "vda",
              "kname": "vda",
              "model": "QEMU HARDDISK",
              "size": 68719476736,
              "type": "disk",
              "tran": null,
              "rm": false,
              "mountpoints": [null]
            }
          ]
        }"#;

        let disks = parse_lsblk_json(sample);
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[0].path, "/dev/nvme0n1");
        assert_eq!(disks[0].model, "Samsung SSD 990 PRO 1TB");
        assert!(disks[0].mounted);
        assert!(!disks[0].is_selectable());

        assert_eq!(disks[1].path, "/dev/vda");
        assert_eq!(disks[1].size_label, "69 GB");
        assert!(!disks[1].mounted);
        assert!(disks[1].is_selectable());
    }
}
