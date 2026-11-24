//! Block device, GPT partition, and unallocated gap discovery (`RUSTYOS-115` / `RUSTYOS-116`).

use std::process::Command;

const MIN_WHOLE_DISK_BYTES: u64 = 8_000_000_000;
const MIN_DUALBOOT_ROOT_BYTES: u64 = 12_000_000_000;
const MIN_SHARED_EFI_BYTES: u64 = 272_629_760; // 260 MiB
const ONE_MIB_BYTES: u64 = 1_048_576;
const GPT_EFI_PARTTYPE: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskPartitionInfo {
    pub path: String,
    pub size_bytes: u64,
    pub size_label: String,
    pub start_bytes: Option<u64>,
    pub fstype: Option<String>,
    pub parttype: Option<String>,
    pub label: Option<String>,
    pub mounted: bool,
    pub mount_hint: Option<String>,
}

impl DiskPartitionInfo {
    #[must_use]
    pub(crate) fn is_efi(&self) -> bool {
        if let Some(pt) = &self.parttype {
            let low = pt.trim().to_ascii_lowercase();
            if low == GPT_EFI_PARTTYPE || low == "0xef" {
                return true;
            }
        }
        let is_vfat = self
            .fstype
            .as_deref()
            .is_some_and(|fs| fs.eq_ignore_ascii_case("vfat"));
        if !is_vfat || self.size_bytes > 4_294_967_296 {
            return false;
        }
        let hint_efi = self.mount_hint.as_deref().is_some_and(|m| {
            let l = m.to_ascii_lowercase();
            l.starts_with("/efi") || l.starts_with("/boot")
        });
        let label_efi = self.label.as_deref().is_some_and(|lbl| {
            let l = lbl.to_ascii_lowercase();
            l.contains("efi") || l.contains("esp")
        });
        hint_efi || label_efi
    }

    #[must_use]
    pub(crate) fn is_efi_large_enough(&self) -> bool {
        self.is_efi() && self.size_bytes >= MIN_SHARED_EFI_BYTES
    }

    #[must_use]
    pub(crate) fn is_dualboot_root_candidate(&self) -> bool {
        !self.is_efi() && self.size_bytes >= MIN_DUALBOOT_ROOT_BYTES
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnallocatedRegionInfo {
    pub disk_path: String,
    pub start_bytes: u64,
    pub size_bytes: u64,
    pub size_label: String,
    pub gpt_disk: bool,
}

impl UnallocatedRegionInfo {
    #[must_use]
    pub(crate) fn is_selectable(&self) -> bool {
        self.gpt_disk && self.size_bytes >= MIN_DUALBOOT_ROOT_BYTES
    }
}

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
    pub pttype: Option<String>,
    pub partitions: Vec<DiskPartitionInfo>,
    pub free_regions: Vec<UnallocatedRegionInfo>,
}

impl TargetDiskInfo {
    #[must_use]
    pub(crate) fn is_selectable(&self) -> bool {
        !self.mounted && self.size_bytes >= MIN_WHOLE_DISK_BYTES
    }

    #[must_use]
    pub(crate) fn is_live_media(&self) -> bool {
        self.mount_hint
            .as_deref()
            .is_some_and(|m| m.contains("/run/archiso"))
            || self
                .partitions
                .iter()
                .filter_map(|p| p.mount_hint.as_deref())
                .any(|m| m.contains("/run/archiso"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DualBootKind {
    UnallocatedGap {
        start_bytes: u64,
        size_bytes: u64,
    },
    ExistingPartition {
        part_path: String,
        fstype: Option<String>,
        label: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DualBootTargetInfo {
    pub disk_path: String,
    pub disk_model: String,
    pub title: String,
    pub subtitle: String,
    pub size_bytes: u64,
    pub size_label: String,
    pub efi_part: Option<String>,
    pub efi_reused: bool,
    pub selectable: bool,
    pub badge_label: &'static str,
    pub kind: DualBootKind,
}

#[must_use]
#[allow(clippy::cast_precision_loss)]
pub(crate) fn format_bytes_human(bytes: u64) -> String {
    const TB: f64 = 1_000_000_000_000.0;
    const GB: f64 = 1_000_000_000.0;
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
            pttype: Some(String::from("gpt")),
            partitions: Vec::new(),
            free_regions: Vec::new(),
        },
        TargetDiskInfo {
            path: String::from("/dev/sda"),
            model: String::from("Primary Dual-Boot SSD"),
            size_bytes: 1_000_204_886_016,
            size_label: format_bytes_human(1_000_204_886_016),
            transport: String::from("sata"),
            removable: false,
            mounted: true,
            mount_hint: Some(String::from("/efi")),
            pttype: Some(String::from("gpt")),
            partitions: vec![
                DiskPartitionInfo {
                    path: String::from("/dev/sda1"),
                    size_bytes: 536_870_912,
                    size_label: format_bytes_human(536_870_912),
                    start_bytes: Some(ONE_MIB_BYTES),
                    fstype: Some(String::from("vfat")),
                    parttype: Some(String::from(GPT_EFI_PARTTYPE)),
                    label: Some(String::from("EFI")),
                    mounted: true,
                    mount_hint: Some(String::from("/efi")),
                },
                DiskPartitionInfo {
                    path: String::from("/dev/sda2"),
                    size_bytes: 400_000_000_000,
                    size_label: format_bytes_human(400_000_000_000),
                    start_bytes: Some(537_919_488),
                    fstype: Some(String::from("btrfs")),
                    parttype: Some(String::from("0fc63daf-8483-4772-8e79-3d69d8477de4")),
                    label: Some(String::from("endeavouros")),
                    mounted: true,
                    mount_hint: Some(String::from("/")),
                },
                DiskPartitionInfo {
                    path: String::from("/dev/sda3"),
                    size_bytes: 120_000_000_000,
                    size_label: format_bytes_human(120_000_000_000),
                    start_bytes: Some(400_537_919_488),
                    fstype: Some(String::from("ext4")),
                    parttype: Some(String::from("0fc63daf-8483-4772-8e79-3d69d8477de4")),
                    label: Some(String::from("RustyOS_Target")),
                    mounted: false,
                    mount_hint: None,
                },
            ],
            free_regions: vec![UnallocatedRegionInfo {
                disk_path: String::from("/dev/sda"),
                start_bytes: 520_537_919_488,
                size_bytes: 180_000_000_000,
                size_label: format_bytes_human(180_000_000_000),
                gpt_disk: true,
            }],
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
            pttype: Some(String::from("gpt")),
            partitions: Vec::new(),
            free_regions: Vec::new(),
        },
    ]
}

#[must_use]
pub(crate) fn discover_target_disks(demo_mode: bool) -> Vec<TargetDiskInfo> {
    if demo_mode {
        return demo_target_disks();
    }
    // Try rich column list first, then a legacy-compatible column list for older `util-linux`.
    // Never fall back to fabricated `demo_target_disks()` in live mode (`RUSTYOS-168`).
    let column_sets = [
        "NAME,KNAME,MODEL,SIZE,START,LOG-SEC,TYPE,TRAN,RM,FSTYPE,PARTTYPE,LABEL,PTTYPE,MOUNTPOINT,MOUNTPOINTS",
        "NAME,KNAME,MODEL,SIZE,TYPE,TRAN,RM,FSTYPE,LABEL,MOUNTPOINT",
    ];
    for cols in column_sets {
        let mut cmd = Command::new("lsblk");
        cmd.args(["-Jpb", "-o", cols]);
        if let Ok(o) =
            rusty_services::run_command_bounded(&mut cmd, std::time::Duration::from_secs(8))
            && o.status.success()
        {
            return parse_lsblk_json(&String::from_utf8_lossy(&o.stdout));
        }
    }
    Vec::new()
}

/// Builds candidate Dual-Boot targets (unallocated regions first, then reusable partitions).
#[must_use]
pub(crate) fn collect_dualboot_targets(disks: &[TargetDiskInfo]) -> Vec<DualBootTargetInfo> {
    let global_large_efi = disks
        .iter()
        .filter(|d| !d.is_live_media())
        .flat_map(|d| d.partitions.iter())
        .find(|p| p.is_efi_large_enough())
        .map(|p| p.path.clone());

    let mut targets = Vec::new();
    for disk in disks {
        if disk.is_live_media() {
            continue;
        }
        let disk_large_efi = disk
            .partitions
            .iter()
            .find(|p| p.is_efi_large_enough())
            .map(|p| p.path.clone())
            .or_else(|| global_large_efi.clone());
        let has_small_esp = disk
            .partitions
            .iter()
            .any(|p| p.is_efi() && !p.is_efi_large_enough());

        for region in &disk.free_regions {
            targets.push(build_free_region_target(
                disk,
                region,
                disk_large_efi.as_deref(),
                has_small_esp,
            ));
        }

        for part in &disk.partitions {
            if !part.is_dualboot_root_candidate() {
                continue;
            }
            targets.push(build_partition_target(
                disk,
                part,
                disk_large_efi.as_deref(),
                has_small_esp,
            ));
        }
    }
    targets
}

fn build_free_region_target(
    disk: &TargetDiskInfo,
    region: &UnallocatedRegionInfo,
    large_efi: Option<&str>,
    has_small_esp: bool,
) -> DualBootTargetInfo {
    let (efi_part, efi_reused, subtitle) = match (large_efi, has_small_esp) {
        (Some(esp), false) => (
            Some(esp.to_owned()),
            true,
            format!(
                "{} • Free space on {} • Reuses EFI {} untouched",
                region.size_label, disk.path, esp
            ),
        ),
        (_, true) => (
            None,
            false,
            format!(
                "{} • Free space on {} • Adds 512M EFI (keeps small ESP safe)",
                region.size_label, disk.path
            ),
        ),
        (None, false) => (
            None,
            false,
            format!(
                "{} • Free space on {} • Creates 512M EFI + RustyOS root",
                region.size_label, disk.path
            ),
        ),
    };
    let selectable = region.is_selectable();
    DualBootTargetInfo {
        disk_path: disk.path.clone(),
        disk_model: disk.model.clone(),
        title: format!("Unallocated Space ({})", disk.path),
        subtitle,
        size_bytes: region.size_bytes,
        size_label: region.size_label.clone(),
        efi_part,
        efi_reused,
        selectable,
        badge_label: if selectable { "Free Gap" } else { "GPT / <12G" },
        kind: DualBootKind::UnallocatedGap {
            start_bytes: region.start_bytes,
            size_bytes: region.size_bytes,
        },
    }
}

fn build_partition_target(
    disk: &TargetDiskInfo,
    part: &DiskPartitionInfo,
    large_efi: Option<&str>,
    has_small_esp: bool,
) -> DualBootTargetInfo {
    let fs = part.fstype.as_deref().unwrap_or("raw");
    let lbl = part
        .label
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("Unlabeled");
    let (selectable, badge, efi_part, efi_reused, subtitle) = if part.mounted {
        (
            false,
            "Mounted OS",
            large_efi.map(str::to_owned),
            false,
            format!(
                "{} • {} '{}' mounted at {}",
                part.size_label,
                fs,
                lbl,
                part.mount_hint.as_deref().unwrap_or("/")
            ),
        )
    } else if let Some(esp) = large_efi {
        (
            true,
            "Available",
            Some(esp.to_owned()),
            true,
            format!(
                "{} • {} '{}' • Reuses EFI {} untouched",
                part.size_label, fs, lbl, esp
            ),
        )
    } else if has_small_esp {
        (
            false,
            "ESP < 260M",
            None,
            false,
            format!(
                "{} • Existing ESP < 260 MB; shrink space instead to add 512M EFI",
                part.size_label
            ),
        )
    } else {
        (
            false,
            "No EFI",
            None,
            false,
            format!(
                "{} • No usable EFI >= 260 MB found on {}",
                part.size_label, disk.path
            ),
        )
    };

    DualBootTargetInfo {
        disk_path: disk.path.clone(),
        disk_model: disk.model.clone(),
        title: format!("{} ({lbl})", part.path),
        subtitle,
        size_bytes: part.size_bytes,
        size_label: part.size_label.clone(),
        efi_part,
        efi_reused,
        selectable,
        badge_label: badge,
        kind: DualBootKind::ExistingPartition {
            part_path: part.path.clone(),
            fstype: part.fstype.clone(),
            label: part.label.clone(),
        },
    }
}

/// Parses `lsblk -Jpb` JSON without adding external JSON dependencies.
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
        if let Some(disk) = parse_single_disk_object(obj) {
            out.push(disk);
        }
    }
    out
}

fn parse_single_disk_object(obj: &str) -> Option<TargetDiskInfo> {
    if extract_json_str(obj, "type").as_deref() != Some("disk") {
        return None;
    }
    let name_or_kname = extract_json_str(obj, "kname").or_else(|| extract_json_str(obj, "name"))?;
    let path = normalize_dev_path(&name_or_kname);
    let size_bytes = extract_json_u64(obj, "size").unwrap_or(0);
    let sec_size = extract_json_u64(obj, "log-sec").unwrap_or(512).max(512);
    let pttype = extract_json_str(obj, "pttype");
    let tran = extract_json_str(obj, "tran").unwrap_or_else(|| infer_transport(&path));
    let model = extract_json_str(obj, "model")
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| format!("{} Drive", tran.to_ascii_uppercase()));
    let removable = extract_json_bool(obj, "rm").unwrap_or(false);
    let mount_hint = collect_nonempty_mountpoints(obj);
    let mounted = mount_hint.is_some();

    let partitions = parse_disk_children(obj, sec_size);
    let free_regions = compute_unallocated_gaps(
        &path,
        size_bytes,
        pttype.as_deref() == Some("gpt"),
        &partitions,
    );

    Some(TargetDiskInfo {
        path,
        model,
        size_bytes,
        size_label: format_bytes_human(size_bytes),
        transport: tran,
        removable,
        mounted,
        mount_hint,
        pttype,
        partitions,
        free_regions,
    })
}

fn normalize_dev_path(name_or_kname: &str) -> String {
    if name_or_kname.starts_with('/') {
        name_or_kname.to_owned()
    } else {
        format!("/dev/{name_or_kname}")
    }
}

fn infer_transport(path: &str) -> String {
    if path.contains("nvme") {
        String::from("nvme")
    } else if path.contains("vd") {
        String::from("virtio")
    } else {
        String::from("disk")
    }
}

fn parse_disk_children(disk_obj: &str, default_sec_size: u64) -> Vec<DiskPartitionInfo> {
    let Some(children_val) = find_top_level_value(disk_obj, "children") else {
        return Vec::new();
    };
    let Some(arr) = extract_balanced_slice(children_val, 0, b'[', b']') else {
        return Vec::new();
    };

    let mut parts = Vec::new();
    for obj in split_top_level_objects(arr) {
        if extract_json_str(obj, "type").as_deref() != Some("part") {
            continue;
        }
        let Some(name) = extract_json_str(obj, "kname").or_else(|| extract_json_str(obj, "name"))
        else {
            continue;
        };
        let path = normalize_dev_path(&name);
        let size_bytes = extract_json_u64(obj, "size").unwrap_or(0);
        let sec = extract_json_u64(obj, "log-sec")
            .unwrap_or(default_sec_size)
            .max(512);
        let start_bytes = extract_json_u64(obj, "start").map(|s| s.saturating_mul(sec));
        let fstype = extract_json_str(obj, "fstype");
        let parttype = extract_json_str(obj, "parttype");
        let label = extract_json_str(obj, "label");
        let mount_hint = collect_nonempty_mountpoints(obj);
        let mounted = mount_hint.is_some();

        parts.push(DiskPartitionInfo {
            path,
            size_bytes,
            size_label: format_bytes_human(size_bytes),
            start_bytes,
            fstype,
            parttype,
            label,
            mounted,
            mount_hint,
        });
    }
    parts
}

fn compute_unallocated_gaps(
    disk_path: &str,
    disk_size: u64,
    gpt_disk: bool,
    partitions: &[DiskPartitionInfo],
) -> Vec<UnallocatedRegionInfo> {
    if !gpt_disk || disk_size <= ONE_MIB_BYTES * 2 || partitions.is_empty() {
        return Vec::new();
    }
    let mut intervals: Vec<(u64, u64)> = partitions
        .iter()
        .filter_map(|p| {
            let s = p.start_bytes?;
            Some((s, s.saturating_add(p.size_bytes)))
        })
        .collect();
    if intervals.len() != partitions.len() {
        return Vec::new();
    }
    intervals.sort_unstable_by_key(|(s, _)| *s);

    let end_limit = disk_size.saturating_sub(ONE_MIB_BYTES);
    let mut cursor = ONE_MIB_BYTES;
    let mut gaps = Vec::new();
    for (p_start, p_end) in intervals {
        let clamped_start = p_start.min(end_limit);
        if clamped_start > cursor && clamped_start - cursor >= MIN_DUALBOOT_ROOT_BYTES {
            let sz = clamped_start - cursor;
            gaps.push(UnallocatedRegionInfo {
                disk_path: disk_path.to_owned(),
                start_bytes: cursor,
                size_bytes: sz,
                size_label: format_bytes_human(sz),
                gpt_disk,
            });
        }
        cursor = cursor.max(p_end);
    }
    if end_limit > cursor && end_limit - cursor >= MIN_DUALBOOT_ROOT_BYTES {
        let sz = end_limit - cursor;
        gaps.push(UnallocatedRegionInfo {
            disk_path: disk_path.to_owned(),
            start_bytes: cursor,
            size_bytes: sz,
            size_label: format_bytes_human(sz),
            gpt_disk,
        });
    }
    gaps
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
    if let Some(mp_val) = find_top_level_value(obj, "mountpoints")
        && let Some(arr_slice) = extract_balanced_slice(mp_val, 0, b'[', b']')
    {
        for part in arr_slice.split('"') {
            if part.starts_with('/') {
                return Some(part.to_owned());
            }
        }
    }
    if let Some(mp) = extract_json_str(obj, "mountpoint")
        && !mp.is_empty()
    {
        return Some(mp);
    }
    let mut idx = 0;
    while let Some(pos) = obj[idx..].find("\"mountpoint") {
        let rest = &obj[idx + pos..];
        let Some(colon) = rest.find(':') else {
            break;
        };
        let after = rest[colon + 1..].trim_start();
        if let Some(arr_slice) = extract_balanced_slice(after, 0, b'[', b']') {
            for part in arr_slice.split('"') {
                if part.starts_with('/') {
                    return Some(part.to_owned());
                }
            }
        } else if let Some(after_q) = after.strip_prefix('"')
            && let Some(end_q) = after_q.find('"')
        {
            let mp = &after_q[..end_q];
            if !mp.is_empty() {
                return Some(mp.to_owned());
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
              "log-sec": 512,
              "pttype": "gpt",
              "type": "disk",
              "tran": "nvme",
              "rm": false,
              "mountpoints": [null],
              "children": [
                {"name": "nvme0n1p1", "size": 536870912, "start": 2048, "type": "part", "fstype": "vfat", "parttype": "c12a7328-f81f-11d2-ba4b-00a0c93ec93b", "label": "EFI", "mountpoints": ["/boot"]},
                {"name": "nvme0n1p2", "size": 500000000000, "start": 1050624, "type": "part", "fstype": "btrfs", "label": "endeavouros", "mountpoints": ["/"]}
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
        assert_eq!(disks[0].partitions.len(), 2);
        assert!(disks[0].partitions[0].is_efi_large_enough());
        assert_eq!(disks[0].free_regions.len(), 1);
        assert!(disks[0].free_regions[0].is_selectable());

        let db_targets = collect_dualboot_targets(&disks);
        assert!(db_targets.iter().any(|t| {
            t.selectable
                && matches!(t.kind, DualBootKind::UnallocatedGap { .. })
                && t.efi_part.as_deref() == Some("/dev/nvme0n1p1")
                && t.efi_reused
        }));
        assert!(db_targets.iter().any(|t| !t.selectable
            && matches!(t.kind, DualBootKind::ExistingPartition { ref part_path, .. } if part_path == "/dev/nvme0n1p2")));

        assert_eq!(disks[1].path, "/dev/vda");
        assert_eq!(disks[1].size_label, "69 GB");
        assert!(!disks[1].mounted);
        assert!(disks[1].is_selectable());
    }

    #[test]
    fn small_windows_esp_triggers_dedicated_efi_for_unallocated_gap() {
        let win_disk = TargetDiskInfo {
            path: String::from("/dev/nvme0n1"),
            model: String::from("Win11 NVMe"),
            size_bytes: 512_000_000_000,
            size_label: String::from("512 GB"),
            transport: String::from("nvme"),
            removable: false,
            mounted: false,
            mount_hint: None,
            pttype: Some(String::from("gpt")),
            partitions: vec![
                DiskPartitionInfo {
                    path: String::from("/dev/nvme0n1p1"),
                    size_bytes: 104_857_600,
                    size_label: String::from("105 MB"),
                    start_bytes: Some(ONE_MIB_BYTES),
                    fstype: Some(String::from("vfat")),
                    parttype: Some(String::from(GPT_EFI_PARTTYPE)),
                    label: Some(String::from("SYSTEM")),
                    mounted: false,
                    mount_hint: None,
                },
                DiskPartitionInfo {
                    path: String::from("/dev/nvme0n1p2"),
                    size_bytes: 250_000_000_000,
                    size_label: String::from("250 GB"),
                    start_bytes: Some(120_000_000),
                    fstype: Some(String::from("ntfs")),
                    parttype: None,
                    label: Some(String::from("Windows")),
                    mounted: false,
                    mount_hint: None,
                },
            ],
            free_regions: vec![UnallocatedRegionInfo {
                disk_path: String::from("/dev/nvme0n1"),
                start_bytes: 250_120_000_000,
                size_bytes: 260_000_000_000,
                size_label: String::from("260 GB"),
                gpt_disk: true,
            }],
        };

        let db = collect_dualboot_targets(&[win_disk]);
        let gap = db
            .iter()
            .find(|t| matches!(t.kind, DualBootKind::UnallocatedGap { .. }));
        assert!(gap.is_some());
        if let Some(g) = gap {
            assert!(g.selectable);
            assert!(g.efi_part.is_none());
            assert!(!g.efi_reused);
        }
        let ntfs = db.iter().find(|t| {
            matches!(t.kind, DualBootKind::ExistingPartition { ref part_path, .. } if part_path == "/dev/nvme0n1p2")
        });
        assert!(ntfs.is_some_and(|n| !n.selectable && n.badge_label == "ESP < 260M"));
    }
}
