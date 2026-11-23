#!/usr/bin/env bash
# RustyOS System Installer & Recovery Backend (`RUSTYOS-115` / `RUSTYOS-116`, ADR-0065 / ADR-0066)
# Safety: never modifies disks when `--dry-run` is active; dual-boot mode never calls `sgdisk --zap-all`.
set -euo pipefail

DRY_RUN=0
CONFIRM_ERASE=0
CONFIRM_DUALBOOT=0
INSTALL_MODE="whole-disk"
INSTALL_SOURCE="auto"
TARGET_DISK=""
ROOT_PART=""
FREE_SPACE_SPEC=""
EFI_PART=""
HOSTNAME_VAL="rustyos"
USERNAME_VAL="rusty"
PASSWORD_STDIN=0
PASSWORD_VAL=""
LOCALE_VAL="en_US.UTF-8"
KEYMAP_VAL="us"
KB_LAYOUT="us,ru"
TIMEZONE_VAL="UTC"
BOOTLOADER="systemd-boot"
APPEARANCE="light"
ACCENT_INDEX=0
AUTOLOGIN=1
INSTALL_OOBE=1
MOUNT_ROOT="/mnt/rustyos-install"

usage() {
    cat <<'EOF'
Usage: rustyos-install [OPTIONS]

Modes:
  --list-disks-json          Print block devices & partitions JSON via lsblk -Jpb
  --rollback-help            Print rollback and recovery instructions
  --dry-run                  Validate inputs and print full plan without touching disks

Install options:
  --install-mode <mode>      whole-disk | dual-boot (default: whole-disk)
  --install-source <src>     auto | squashfs | pacstrap (default: auto; prefers offline Live SquashFS)
  --target-disk <dev>        Target whole block device (for whole-disk mode)
  --root-part <dev>          Target existing root partition (for dual-boot mode)
  --free-space <disk:start:size>
                             Carve partition(s) strictly inside unallocated bytes range
  --efi-part <dev>           Existing EFI System Partition to reuse WITHOUT formatting
  --confirm-erase            Required in real whole-disk mode (wipes entire disk)
  --confirm-dualboot         Required in real dual-boot mode (preserves other partitions)
  --hostname <name>          Target hostname (default: rustyos)
  --username <user>          Primary sudo user name (default: rusty)
  --password-stdin           Read user password line from stdin (non-interactive)
  --locale <locale>          System locale (default: en_US.UTF-8)
  --keymap <vconsole>        Console keymap (default: us)
  --kb-layout <xkb>          RustyOS [keyboard].layout (default: us,ru)
  --timezone <Region/City>   Timezone (default: UTC)
  --bootloader <type>        systemd-boot | grub (default: systemd-boot)
  --appearance <mode>        light | dark | auto (default: light)
  --accent <0..6>            Accent swatch index 0..6 (default: 0)
  --autologin <0|1>          Enable greetd initial session into rustyos-session (default: 1)
  --install-oobe <0|1>       Mark first boot for OOBE Setup Assistant (default: 1)
EOF
}

print_rollback_help() {
    cat <<'EOF'
=== RustyOS Rollback & Recovery Guide ===
1. Package removal / rollback on an existing Arch system:
     sudo pacman -Rns rustyos-git
     sudo pacman -U /var/cache/pacman/pkg/rustyos-git-<prev>.pkg.tar.zst
2. Live ISO chroot recovery if target system fails to boot:
     mount /dev/<root-part> /mnt
     mount /dev/<efi-part> /mnt/boot
     arch-chroot /mnt
     journalctl -b -1
     systemctl restart greetd   # or switch /etc/greetd/config.toml to bash
3. Session-only fallback without removing RustyOS:
     Disable greetd autologin in /etc/greetd/config.toml or run `rustyos-session --doctor`.
EOF
}

emit_step() {
    printf 'STEP:%s:%s:%s\n' "$1" "$2" "$3"
}

is_valid_disk_path() {
    [[ "$1" =~ ^/dev/(sd[a-z]+|vd[a-z]+|xvd[a-z]+|nvme[0-9]+n[0-9]+|mmcblk[0-9]+|loop[0-9]+)$ ]]
}

is_valid_part_path() {
    [[ "$1" =~ ^/dev/(sd[a-z]+[0-9]+|vd[a-z]+[0-9]+|xvd[a-z]+[0-9]+|nvme[0-9]+n[0-9]+p[0-9]+|mmcblk[0-9]+p[0-9]+|loop[0-9]+p[0-9]+)$ ]]
}

FREE_DISK=""
FREE_START_BYTES=0
FREE_SIZE_BYTES=0

parse_free_space_spec() {
    local spec="$1"
    IFS=':' read -r FREE_DISK FREE_START_BYTES FREE_SIZE_BYTES extra <<<"${spec}"
    if [[ -n "${extra:-}" ]] || [[ -z "${FREE_DISK}" ]]; then
        echo "Error: invalid --free-space '${spec}' (expected <disk>:<start_bytes>:<size_bytes>)" >&2
        exit 2
    fi
    if ! is_valid_disk_path "${FREE_DISK}"; then
        echo "Error: unsafe disk path '${FREE_DISK}' in --free-space" >&2
        exit 3
    fi
    if ! [[ "${FREE_START_BYTES}" =~ ^[0-9]+$ && "${FREE_SIZE_BYTES}" =~ ^[0-9]+$ ]]; then
        echo "Error: --free-space byte offsets must be integers" >&2
        exit 2
    fi
    if (( FREE_SIZE_BYTES < 12000000000 )); then
        echo "Error: --free-space region must be at least 12 GB (got ${FREE_SIZE_BYTES} bytes)" >&2
        exit 2
    fi
}

validate_inputs() {
    if ! [[ "${INSTALL_MODE}" =~ ^(whole-disk|dual-boot)$ ]]; then
        echo "Error: invalid --install-mode '${INSTALL_MODE}' (expected whole-disk or dual-boot)" >&2
        exit 2
    fi
    if ! [[ "${INSTALL_SOURCE}" =~ ^(auto|squashfs|pacstrap)$ ]]; then
        echo "Error: invalid --install-source '${INSTALL_SOURCE}' (expected auto, squashfs, or pacstrap)" >&2
        exit 2
    fi
    if [[ "${INSTALL_MODE}" == "whole-disk" ]]; then
        if [[ -z "${TARGET_DISK}" ]]; then
            echo "Error: --target-disk is required in whole-disk mode" >&2
            exit 2
        fi
    else
        if [[ -n "${ROOT_PART}" && -n "${FREE_SPACE_SPEC}" ]]; then
            echo "Error: pass either --root-part OR --free-space in dual-boot mode, not both" >&2
            exit 2
        fi
        if [[ -z "${ROOT_PART}" && -z "${FREE_SPACE_SPEC}" ]]; then
            echo "Error: dual-boot mode requires --root-part or --free-space" >&2
            exit 2
        fi
        if [[ -n "${FREE_SPACE_SPEC}" ]]; then
            parse_free_space_spec "${FREE_SPACE_SPEC}"
        fi
    fi
    if ! [[ "${HOSTNAME_VAL}" =~ ^[a-zA-Z0-9][a-zA-Z0-9-]{0,62}$ ]]; then
        echo "Error: invalid --hostname '${HOSTNAME_VAL}'" >&2
        exit 2
    fi
    if ! [[ "${USERNAME_VAL}" =~ ^[a-z_][a-z0-9_-]{0,31}$ ]]; then
        echo "Error: invalid POSIX --username '${USERNAME_VAL}'" >&2
        exit 2
    fi
    if [[ "${USERNAME_VAL}" == "root" || "${USERNAME_VAL}" == "liveuser" ]]; then
        echo "Error: reserved username '${USERNAME_VAL}'" >&2
        exit 2
    fi
    if ! [[ "${BOOTLOADER}" =~ ^(systemd-boot|grub)$ ]]; then
        echo "Error: invalid --bootloader '${BOOTLOADER}' (expected systemd-boot or grub)" >&2
        exit 2
    fi
    if ! [[ "${APPEARANCE}" =~ ^(light|dark|auto)$ ]]; then
        echo "Error: invalid --appearance '${APPEARANCE}'" >&2
        exit 2
    fi
    if ! [[ "${ACCENT_INDEX}" =~ ^[0-6]$ ]]; then
        echo "Error: --accent must be 0..6 (got '${ACCENT_INDEX}')" >&2
        exit 2
    fi
}

partition_suffix() {
    local dev="$1"
    local num="$2"
    if [[ "${dev}" =~ [0-9]$ ]]; then
        printf '%sp%s\n' "${dev}" "${num}"
    else
        printf '%s%s\n' "${dev}" "${num}"
    fi
}

cleanup_install_mounts() {
    if [[ -d "${MOUNT_ROOT}/etc/pacman.d/gnupg" ]] && command -v gpgconf >/dev/null 2>&1; then
        gpgconf --homedir "${MOUNT_ROOT}/etc/pacman.d/gnupg" --kill all >/dev/null 2>&1 || true
    fi
    if command -v fuser >/dev/null 2>&1; then
        fuser -km "${MOUNT_ROOT}" >/dev/null 2>&1 || true
    fi
    umount -R "${MOUNT_ROOT}" >/dev/null 2>&1 || umount -lR "${MOUNT_ROOT}" >/dev/null 2>&1 || true
}

assert_unmounted_non_root() {
    local dev="$1"
    local mounted
    mounted="$(lsblk -nr -o MOUNTPOINTS "${dev}" 2>/dev/null | grep -v '^$' || true)"
    if [[ -n "${mounted}" ]]; then
        local non_installer_mnts=""
        local m
        while IFS= read -r m; do
            [[ -z "${m}" ]] && continue
            if [[ "${m}" != "${MOUNT_ROOT}" && "${m}" != "${MOUNT_ROOT}/"* ]]; then
                non_installer_mnts+="${m} "
            fi
        done <<<"${mounted}"
        if [[ -z "${non_installer_mnts}" ]]; then
            cleanup_install_mounts
            mounted="$(lsblk -nr -o MOUNTPOINTS "${dev}" 2>/dev/null | grep -v '^$' || true)"
        fi
    fi
    if [[ -n "${mounted}" ]]; then
        echo "Error: target '${dev}' has active mountpoints (${mounted}); refusing" >&2
        exit 3
    fi
    local root_src
    root_src="$(findmnt -no SOURCE / 2>/dev/null || true)"
    if [[ -n "${root_src}" && "${root_src}" == "${dev}"* ]]; then
        echo "Error: '${dev}' backs the running root filesystem; refusing" >&2
        exit 3
    fi
}

check_targets_safe() {
    if [[ "${INSTALL_MODE}" == "whole-disk" ]]; then
        if ! is_valid_disk_path "${TARGET_DISK}"; then
            echo "Error: unsupported or unsafe block device path '${TARGET_DISK}'" >&2
            exit 3
        fi
        if [[ "${DRY_RUN}" -eq 0 ]]; then
            if [[ ! -b "${TARGET_DISK}" ]]; then
                echo "Error: block device '${TARGET_DISK}' does not exist" >&2
                exit 3
            fi
            assert_unmounted_non_root "${TARGET_DISK}"
        fi
        return 0
    fi

    if [[ -n "${ROOT_PART}" ]]; then
        if ! is_valid_part_path "${ROOT_PART}"; then
            echo "Error: invalid partition device path '${ROOT_PART}'" >&2
            exit 3
        fi
        if [[ "${DRY_RUN}" -eq 0 ]]; then
            if [[ ! -b "${ROOT_PART}" ]]; then
                echo "Error: partition '${ROOT_PART}' does not exist" >&2
                exit 3
            fi
            assert_unmounted_non_root "${ROOT_PART}"
        fi
    fi

    if [[ -n "${FREE_DISK}" && "${DRY_RUN}" -eq 0 ]]; then
        if [[ ! -b "${FREE_DISK}" ]]; then
            echo "Error: disk '${FREE_DISK}' does not exist" >&2
            exit 3
        fi
    fi

    if [[ -n "${EFI_PART}" ]]; then
        if ! is_valid_part_path "${EFI_PART}"; then
            echo "Error: invalid EFI partition path '${EFI_PART}'" >&2
            exit 3
        fi
        if [[ "${DRY_RUN}" -eq 0 ]]; then
            if [[ ! -b "${EFI_PART}" ]]; then
                echo "Error: EFI partition '${EFI_PART}' does not exist" >&2
                exit 3
            fi
            local efi_fs
            efi_fs="$(lsblk -no FSTYPE "${EFI_PART}" 2>/dev/null || true)"
            if [[ "${efi_fs}" != "vfat" ]]; then
                echo "Error: EFI partition '${EFI_PART}' must be FAT32/vfat (found '${efi_fs}')" >&2
                exit 3
            fi
        fi
    fi
}

find_part_by_start_sector() {
    local disk="$1"
    local wanted_sec="$2"
    local base_disk part_name start_file s
    base_disk="$(basename "${disk}")"
    for start_file in /sys/block/"${base_disk}"/*/start; do
        [[ -r "${start_file}" ]] || continue
        read -r s <"${start_file}"
        if [[ "${s}" == "${wanted_sec}" ]]; then
            part_name="$(basename "$(dirname "${start_file}")")"
            printf '/dev/%s\n' "${part_name}"
            return 0
        fi
    done
    return 1
}

detect_live_squashfs() {
    local cand
    if [[ -n "${RUSTYOS_SQUASHFS_PATH:-}" && -f "${RUSTYOS_SQUASHFS_PATH}" ]]; then
        printf '%s\n' "${RUSTYOS_SQUASHFS_PATH}"
        return 0
    fi
    for cand in \
        /run/archiso/bootmnt/rustyos/x86_64/airootfs.sfs \
        /run/archiso/copytoram/airootfs.sfs \
        /run/archiso/bootmnt/arch/x86_64/airootfs.sfs \
        /run/archiso/sfs/airootfs/airootfs.img; do
        if [[ -f "${cand}" ]]; then
            printf '%s\n' "${cand}"
            return 0
        fi
    done
    return 1
}

resolve_install_source() {
    if [[ "${INSTALL_SOURCE}" == "pacstrap" ]]; then
        printf 'pacstrap\n'
        return 0
    fi
    if detect_live_squashfs >/dev/null 2>&1; then
        printf 'squashfs\n'
        return 0
    fi
    if [[ "${INSTALL_SOURCE}" == "squashfs" ]]; then
        echo "Error: --install-source squashfs requested but no live airootfs.sfs was found" >&2
        exit 2
    fi
    printf 'pacstrap\n'
}

run_dry_plan() {
    local boot_pkg="efibootmgr"
    if [[ "${BOOTLOADER}" == "grub" ]]; then
        boot_pkg="grub efibootmgr os-prober"
    fi

    if [[ "${INSTALL_MODE}" == "whole-disk" ]]; then
        local efi_part root_part
        efi_part="$(partition_suffix "${TARGET_DISK}" 1)"
        root_part="$(partition_suffix "${TARGET_DISK}" 2)"
        emit_step 5 validate "Validated whole-disk target=${TARGET_DISK}, user=${USERNAME_VAL}, host=${HOSTNAME_VAL}, bootloader=${BOOTLOADER}"
        emit_step 15 partition "[dry-run] Would wipe GPT and create ${efi_part} (512M EFI FAT32) + ${root_part} (RustyOS ext4)"
        emit_step 30 mount "[dry-run] Would mount ${root_part} -> ${MOUNT_ROOT} and ${efi_part} -> ${MOUNT_ROOT}/boot"
    else
        if [[ -n "${ROOT_PART}" ]]; then
            local efi_msg="${EFI_PART:-required EFI partition} (reused WITHOUT formatting)"
            emit_step 5 validate "Validated dual-boot partition mode: root=${ROOT_PART}, efi=${efi_msg}, bootloader=${BOOTLOADER}"
            emit_step 15 partition "[dry-run] Preserves partition table (no sgdisk --zap-all); formats ONLY ${ROOT_PART} as ext4"
            emit_step 30 mount "[dry-run] Would mount ${ROOT_PART} -> ${MOUNT_ROOT} and reuse ${EFI_PART:-<efi>} -> ${MOUNT_ROOT}/boot"
        else
            local efi_note="reuses existing EFI ${EFI_PART} without formatting"
            if [[ -z "${EFI_PART}" ]]; then
                efi_note="creates dedicated 512M RUSTYOS_EFI inside free gap (existing ESP untouched)"
            fi
            emit_step 5 validate "Validated dual-boot free-space mode on ${FREE_DISK} (offset=${FREE_START_BYTES}, size=${FREE_SIZE_BYTES}), bootloader=${BOOTLOADER}"
            emit_step 15 partition "[dry-run] Preserves existing partitions; carves RustyOS inside free gap (${efi_note})"
            emit_step 30 mount "[dry-run] Would mount new RustyOS root -> ${MOUNT_ROOT} and EFI -> ${MOUNT_ROOT}/boot"
        fi
    fi

    local resolved_source sfs_path=""
    resolved_source="$(resolve_install_source)"
    if [[ "${resolved_source}" == "squashfs" ]]; then
        sfs_path="$(detect_live_squashfs)"
        emit_step 40 extract "[dry-run] Would extract offline Live SquashFS ${sfs_path} -> ${MOUNT_ROOT} via unsquashfs"
        emit_step 55 cleanup "[dry-run] Would copy vmlinuz-linux -> /boot, remove liveuser/archiso artifacts, and initialize offline pacman keyring"
    else
        emit_step 40 keyring "[dry-run] Would initialize pacman-key and refresh archlinux-keyring"
        emit_step 55 pacstrap "[dry-run] Would run pacstrap -K ${MOUNT_ROOT}: base linux linux-firmware intel-ucode amd-ucode dosfstools networkmanager bluez bluez-utils upower pipewire wireplumber seatd polkit mesa xorg-xwayland xdg-utils xdg-desktop-portal-gtk unzip ttf-jetbrains-mono git base-devel greetd plymouth sudo ${boot_pkg} rustyos-git"
    fi
    emit_step 75 system "[dry-run] Would set timezone=${TIMEZONE_VAL}, locale=${LOCALE_VAL}, keymap=${KEYMAP_VAL}, hostname=${HOSTNAME_VAL}, user=${USERNAME_VAL}"
    emit_step 88 desktop "[dry-run] Would write /etc/greetd/config.toml (autologin=${AUTOLOGIN}) and ~/.config/rustyos/config.toml (theme=${APPEARANCE}, accent=${ACCENT_INDEX}, kb=${KB_LAYOUT}, oobe=${INSTALL_OOBE})"
    emit_step 90 yay "[dry-run] Would build and install optional yay-bin AUR helper for ${USERNAME_VAL} (with safe fallback)"
    if [[ "${INSTALL_MODE}" == "dual-boot" ]]; then
        emit_step 96 bootloader "[dry-run] Would install ${BOOTLOADER} with dual-boot preservation (entries/rustyos.conf or os-prober) and initramfs"
    else
        emit_step 96 bootloader "[dry-run] Would install ${BOOTLOADER} onto ${TARGET_DISK} and generate initramfs"
    fi
    emit_step 100 done "[dry-run] Installation plan verified cleanly (no disk writes performed)"
}

check_network_preflight() {
    emit_step 3 netcheck "Verifying internet access for package installation"
    if command -v curl >/dev/null 2>&1; then
        if curl -fsIL --connect-timeout 5 --max-time 8 https://geo.mirror.pkgbuild.com/ >/dev/null 2>&1 \
            || curl -fsIL --connect-timeout 5 --max-time 8 https://archlinux.org/ >/dev/null 2>&1; then
            return 0
        fi
    elif ip route show default 2>/dev/null | grep -q . || ip -6 route show default 2>/dev/null | grep -q .; then
        return 0
    fi
    echo "Error: no active internet connection detected. Connect to Ethernet or Wi-Fi first (required by pacstrap). Full log: /tmp/rustyos-install.log" >&2
    exit 6
}

install_optional_yay_bin() {
    if ! ip route show default 2>/dev/null | grep -q . && ! ip -6 route show default 2>/dev/null | grep -q .; then
        emit_step 90 yay "Offline install detected; skipping optional yay-bin AUR helper"
        return 0
    fi
    local build_dir="/tmp/rustyos-yay-build"
    emit_step 90 yay "Installing yay AUR helper (yay-bin) for ${USERNAME_VAL}"
    rm -rf "${MOUNT_ROOT}${build_dir}"
    mkdir -p "${MOUNT_ROOT}${build_dir}"
    if ! arch-chroot "${MOUNT_ROOT}" chown -R "${USERNAME_VAL}:${USERNAME_VAL}" "${build_dir}"; then
        rm -rf "${MOUNT_ROOT}${build_dir}"
        echo "Warning: failed to prepare ${build_dir} for yay-bin build; continuing without yay." >&2
        return 0
    fi
    if arch-chroot "${MOUNT_ROOT}" su - "${USERNAME_VAL}" -c \
        "git clone --depth=1 https://aur.archlinux.org/yay-bin.git '${build_dir}/src' && cd '${build_dir}/src' && makepkg -f --noconfirm --noprogressbar"
    then
        local pkg_list=()
        local pkg_file
        for pkg_file in "${MOUNT_ROOT}${build_dir}"/src/yay-bin-*.pkg.tar.*; do
            [[ -e "${pkg_file}" ]] || continue
            case "${pkg_file}" in
                *-debug-*.pkg.tar.*|*.sig) continue ;;
            esac
            pkg_list+=("${pkg_file#${MOUNT_ROOT}}")
        done
        if [[ "${#pkg_list[@]}" -gt 0 ]] && arch-chroot "${MOUNT_ROOT}" pacman -U --noconfirm "${pkg_list[@]}"; then
            rm -rf "${MOUNT_ROOT}${build_dir}"
            return 0
        fi
    fi
    rm -rf "${MOUNT_ROOT}${build_dir}"
    echo "Warning: optional yay-bin AUR installation failed (network/AUR unreachable); system installation continues without yay." >&2
    return 0
}

install_system() {
    if [[ "$(id -u)" -ne 0 ]]; then
        echo "Error: real installation requires root privileges (use pkexec / sudo, or pass --dry-run)" >&2
        exit 4
    fi
    : > /tmp/rustyos-install.log
    chmod 0644 /tmp/rustyos-install.log 2>/dev/null || true
    exec > >(tee -a /tmp/rustyos-install.log) 2> >(tee -a /tmp/rustyos-install.log >&2)

    if [[ "${INSTALL_MODE}" == "whole-disk" && "${CONFIRM_ERASE}" -ne 1 ]]; then
        echo "Error: pass --confirm-erase to confirm destructive partitioning of ${TARGET_DISK}" >&2
        exit 2
    fi
    if [[ "${INSTALL_MODE}" == "dual-boot" && "${CONFIRM_DUALBOOT}" -ne 1 ]]; then
        echo "Error: pass --confirm-dualboot to confirm dual-boot partition installation" >&2
        exit 2
    fi
    local resolved_source
    resolved_source="$(resolve_install_source)"

    local required_tools=(sgdisk blockdev mkfs.fat mkfs.ext4 genfstab arch-chroot partprobe blkid)
    if [[ "${resolved_source}" == "squashfs" ]]; then
        required_tools+=(unsquashfs)
    else
        required_tools+=(pacman pacman-key pacstrap)
    fi
    for tool in "${required_tools[@]}"; do
        if ! command -v "${tool}" >/dev/null 2>&1; then
            echo "Error: required installation tool '${tool}' not found in PATH" >&2
            exit 5
        fi
    done

    if [[ "${resolved_source}" == "pacstrap" ]]; then
        check_network_preflight
    else
        emit_step 3 netcheck "Using offline Live SquashFS image (internet connection not required)"
    fi

    local efi_part root_part format_efi=0 boot_pkg="efibootmgr" boot_disk=""
    if [[ "${BOOTLOADER}" == "grub" ]]; then
        boot_pkg="grub efibootmgr os-prober"
    fi

    trap cleanup_install_mounts EXIT
    cleanup_install_mounts
    mkdir -p "${MOUNT_ROOT}"

    if [[ "${INSTALL_MODE}" == "whole-disk" ]]; then
        boot_disk="${TARGET_DISK}"
        efi_part="$(partition_suffix "${TARGET_DISK}" 1)"
        root_part="$(partition_suffix "${TARGET_DISK}" 2)"
        format_efi=1

        emit_step 5 validate "Checking hardware firmware and disk ${TARGET_DISK}"
        emit_step 15 partition "Partitioning ${TARGET_DISK} (GPT: EFI 512 MiB + RustyOS root)"
        sgdisk --zap-all "${TARGET_DISK}"
        sgdisk -n 1:0:+512M -t 1:ef00 -c 1:RUSTYOS_EFI "${TARGET_DISK}"
        sgdisk -n 2:0:0 -t 2:8300 -c 2:RUSTYOS_ROOT "${TARGET_DISK}"
        partprobe "${TARGET_DISK}" || true
        udevadm settle --timeout=10 || sleep 2

        emit_step 28 format "Formatting EFI (${efi_part}) and root (${root_part})"
        mkfs.fat -F32 "${efi_part}"
        mkfs.ext4 -F -L RUSTYOS "${root_part}"
    else
        emit_step 5 validate "Validating dual-boot layout (preserving existing OS partitions)"
        if [[ -n "${ROOT_PART}" ]]; then
            root_part="${ROOT_PART}"
            boot_disk="$(lsblk -no PKNAME "${ROOT_PART}" | head -n1)"
            [[ -n "${boot_disk}" ]] && boot_disk="/dev/${boot_disk}"
            if [[ -z "${EFI_PART}" ]]; then
                echo "Error: --efi-part is required when installing onto an existing --root-part in dual-boot mode" >&2
                exit 2
            fi
            efi_part="${EFI_PART}"
            format_efi=0
            emit_step 28 format "Formatting ONLY target root partition ${root_part} (EFI ${efi_part} untouched)"
            mkfs.ext4 -F -L RUSTYOS "${root_part}"
        else
            boot_disk="${FREE_DISK}"
            local pttype sec_size align start_sec end_max_sec efi_sectors=0 cur_sec need_new_efi=0
            pttype="$(blkid -s PTTYPE -o value "${FREE_DISK}" 2>/dev/null || true)"
            if [[ "${pttype}" != "gpt" ]]; then
                echo "Error: dual-boot free-space partitioning requires a GPT disk (found '${pttype:-unknown}')" >&2
                exit 3
            fi
            sec_size="$(blockdev --getss "${FREE_DISK}")"
            align="$((1048576 / sec_size))"
            (( align < 1 )) && align=1
            start_sec="$(( ((FREE_START_BYTES + sec_size - 1) / sec_size + align - 1) / align * align ))"
            end_max_sec="$(( (FREE_START_BYTES + FREE_SIZE_BYTES) / sec_size - 1 ))"

            if [[ -z "${EFI_PART}" ]]; then
                need_new_efi=1
            else
                local esp_bytes
                esp_bytes="$(blockdev --getsize64 "${EFI_PART}")"
                if (( esp_bytes < 272629760 )); then
                    need_new_efi=1
                fi
            fi

            cur_sec="${start_sec}"
            local efi_start_sec=0 root_start_sec=0
            if [[ "${need_new_efi}" -eq 1 ]]; then
                efi_sectors="$(( 536870912 / sec_size ))"
                efi_start_sec="${cur_sec}"
                local efi_end_sec="$(( cur_sec + efi_sectors - 1 ))"
                cur_sec="$(( efi_end_sec + 1 ))"
                (( cur_sec + align >= end_max_sec )) && {
                    echo "Error: free space too small for 512M EFI + RustyOS root" >&2
                    exit 3
                }
                root_start_sec="${cur_sec}"
                emit_step 15 partition "Creating 512 MiB RUSTYOS_EFI + RUSTYOS_ROOT strictly inside unallocated gap"
                sgdisk -n "0:${efi_start_sec}:${efi_end_sec}" -t 0:ef00 -c 0:RUSTYOS_EFI "${FREE_DISK}"
                sgdisk -n "0:${root_start_sec}:${end_max_sec}" -t 0:8300 -c 0:RUSTYOS_ROOT "${FREE_DISK}"
                format_efi=1
            else
                efi_part="${EFI_PART}"
                root_start_sec="${cur_sec}"
                format_efi=0
                emit_step 15 partition "Creating RUSTYOS_ROOT inside unallocated gap (reusing EFI ${efi_part})"
                sgdisk -n "0:${root_start_sec}:${end_max_sec}" -t 0:8300 -c 0:RUSTYOS_ROOT "${FREE_DISK}"
            fi

            partprobe "${FREE_DISK}" || true
            udevadm settle --timeout=10 || sleep 2

            if [[ "${need_new_efi}" -eq 1 ]]; then
                efi_part="$(find_part_by_start_sector "${FREE_DISK}" "${efi_start_sec}")" || {
                    echo "Error: unable to locate newly created EFI partition at sector ${efi_start_sec}" >&2
                    exit 5
                }
            fi
            root_part="$(find_part_by_start_sector "${FREE_DISK}" "${root_start_sec}")" || {
                echo "Error: unable to locate newly created root partition at sector ${root_start_sec}" >&2
                exit 5
            }

            emit_step 28 format "Formatting new RustyOS filesystem(s)"
            if [[ "${format_efi}" -eq 1 ]]; then
                mkfs.fat -F32 "${efi_part}"
            fi
            mkfs.ext4 -F -L RUSTYOS "${root_part}"
        fi
    fi

    emit_step 35 mount "Mounting target filesystems"
    mount "${root_part}" "${MOUNT_ROOT}"
    mkdir -p "${MOUNT_ROOT}/boot"
    mount "${efi_part}" "${MOUNT_ROOT}/boot"

    if [[ "${INSTALL_MODE}" == "dual-boot" && "${format_efi}" -eq 0 ]]; then
        local avail_esp
        avail_esp="$(df -B1 --output=avail "${MOUNT_ROOT}/boot" | tail -n1 | tr -d ' ')"
        if [[ -n "${avail_esp}" ]] && (( avail_esp < 125829120 )); then
            echo "Error: shared EFI ${efi_part} has only ${avail_esp} bytes free (< 120 MiB needed for kernel/initramfs); use free-space mode to create a separate 512M EFI" >&2
            exit 3
        fi
    fi

    if [[ "${resolved_source}" == "squashfs" ]]; then
        local sfs_path
        sfs_path="$(detect_live_squashfs)"
        emit_step 40 extract "Extracting offline RustyOS system image (${sfs_path})"
        unsquashfs -f -d "${MOUNT_ROOT}" "${sfs_path}"

        emit_step 60 cleanup "Preparing kernel, cleaning Live ISO artifacts, and initializing keyring"
        local kimg=""
        for cand in \
            "${MOUNT_ROOT}"/usr/lib/modules/*/vmlinuz \
            /run/archiso/bootmnt/rustyos/boot/x86_64/vmlinuz-linux \
            /run/archiso/bootmnt/arch/boot/x86_64/vmlinuz-linux; do
            if [[ -f "${cand}" ]]; then
                kimg="${cand}"
                break
            fi
        done
        if [[ -z "${kimg}" ]]; then
            echo "Error: unable to locate vmlinuz kernel image after SquashFS extraction" >&2
            exit 5
        fi
        cp -f "${kimg}" "${MOUNT_ROOT}/boot/vmlinuz-linux"

        # Clean Live ISO-only users, hooks, presets, and desktop shortcuts from target root.
        arch-chroot "${MOUNT_ROOT}" userdel -rf liveuser 2>/dev/null || true
        rm -rf \
            "${MOUNT_ROOT}/home/liveuser" \
            "${MOUNT_ROOT}/etc/sudoers.d/liveuser" \
            "${MOUNT_ROOT}/etc/polkit-1/rules.d/49-rustyos-live.rules" \
            "${MOUNT_ROOT}/etc/systemd/system-preset/10-rustyos-live.preset" \
            "${MOUNT_ROOT}/etc/skel/Desktop/install-rustyos.desktop" \
            "${MOUNT_ROOT}/etc/mkinitcpio.conf.d/archiso.conf" \
            "${MOUNT_ROOT}/etc/pacman.d/hooks/99-rustyos-liveuser.hook" \
            "${MOUNT_ROOT}/usr/local/bin/rustyos-liveuser-setup" \
            "${MOUNT_ROOT}/etc/pacman-rustyos-install.conf" \
            "${MOUNT_ROOT}/opt/rustyos-repo" \
            "${MOUNT_ROOT}/var/lib/dbus/machine-id"
        : > "${MOUNT_ROOT}/etc/machine-id"
        arch-chroot "${MOUNT_ROOT}" pacman -Rns --noconfirm mkinitcpio-archiso 2>/dev/null || true
        arch-chroot "${MOUNT_ROOT}" pacman-key --init
        arch-chroot "${MOUNT_ROOT}" pacman-key --populate archlinux
    else
        local pacstrap_conf_arg=() host_pacman_conf_arg=()
        if [[ -f /etc/pacman-rustyos-install.conf ]]; then
            pacstrap_conf_arg=(-C /etc/pacman-rustyos-install.conf)
            host_pacman_conf_arg=(--config /etc/pacman-rustyos-install.conf)
        fi
        if ! grep -qE '^[[:space:]]*Server[[:space:]]*=' /etc/pacman.d/mirrorlist 2>/dev/null; then
            mkdir -p /etc/pacman.d
            cat <<'EOF' > /etc/pacman.d/mirrorlist
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
Server = https://fastly.mirror.pkgbuild.com/$repo/os/$arch
Server = https://berlin.mirror.pkgbuild.com/$repo/os/$arch
Server = https://mirror.rackspace.com/archlinux/$repo/os/$arch
EOF
        fi

        emit_step 40 keyring "Initializing pacman keyring and updating archlinux-keyring"
        pacman-key --init
        pacman-key --populate archlinux
        pacman -Sy --noconfirm --needed "${host_pacman_conf_arg[@]}" archlinux-keyring

        emit_step 45 pacstrap "Installing base system, Wayland desktop, and RustyOS packages"
        # shellcheck disable=SC2086
        pacstrap -K "${pacstrap_conf_arg[@]}" "${MOUNT_ROOT}" \
            base linux linux-firmware intel-ucode amd-ucode dosfstools sof-firmware alsa-ucm-conf alsa-utils \
            networkmanager bluez bluez-utils upower pipewire pipewire-alsa pipewire-pulse wireplumber \
            seatd polkit mesa xorg-xwayland xdg-utils xdg-desktop-portal-gtk unzip \
            ttf-jetbrains-mono git base-devel greetd plymouth sudo \
            ${boot_pkg} rustyos-git
    fi

    emit_step 75 system "Configuring timezone, locale, hostname, and user ${USERNAME_VAL}"
    genfstab -U "${MOUNT_ROOT}" > "${MOUNT_ROOT}/etc/fstab"

    cat <<EOF > "${MOUNT_ROOT}/etc/vconsole.conf"
KEYMAP=${KEYMAP_VAL}
EOF
    printf '%s\n' "${HOSTNAME_VAL}" > "${MOUNT_ROOT}/etc/hostname"
    sed -i "s/^#${LOCALE_VAL}/${LOCALE_VAL}/" "${MOUNT_ROOT}/etc/locale.gen" || printf '%s UTF-8\n' "${LOCALE_VAL}" >> "${MOUNT_ROOT}/etc/locale.gen"
    printf 'LANG=%s\n' "${LOCALE_VAL}" > "${MOUNT_ROOT}/etc/locale.conf"

    arch-chroot "${MOUNT_ROOT}" ln -sf "/usr/share/zoneinfo/${TIMEZONE_VAL}" /etc/localtime
    arch-chroot "${MOUNT_ROOT}" hwclock --systohc
    arch-chroot "${MOUNT_ROOT}" locale-gen
    arch-chroot "${MOUNT_ROOT}" useradd -m -G wheel,seat,video,input,audio -s /bin/bash "${USERNAME_VAL}"
    printf '%s:%s\n' "${USERNAME_VAL}" "${PASSWORD_VAL}" | arch-chroot "${MOUNT_ROOT}" chpasswd
    printf '%%wheel ALL=(ALL:ALL) ALL\n' > "${MOUNT_ROOT}/etc/sudoers.d/10-wheel"
    chmod 0440 "${MOUNT_ROOT}/etc/sudoers.d/10-wheel"

    emit_step 85 desktop "Configuring greetd, NetworkManager, and RustyOS preferences"
    if [[ -f "${MOUNT_ROOT}/etc/pacman.conf" ]] && ! grep -q '^\[rustyos\]$' "${MOUNT_ROOT}/etc/pacman.conf"; then
        if [[ -f /etc/pacman.conf ]] && grep -q '^\[rustyos\]$' /etc/pacman.conf; then
            printf '\n' >> "${MOUNT_ROOT}/etc/pacman.conf"
            awk '/^\[rustyos\]$/,/^$/' /etc/pacman.conf >> "${MOUNT_ROOT}/etc/pacman.conf"
        elif [[ -f /etc/pacman-rustyos-install.conf ]] && grep -q '^\[rustyos\]$' /etc/pacman-rustyos-install.conf; then
            printf '\n' >> "${MOUNT_ROOT}/etc/pacman.conf"
            awk '/^\[rustyos\]$/,/^$/' /etc/pacman-rustyos-install.conf >> "${MOUNT_ROOT}/etc/pacman.conf"
        else
            cat <<'EOF' >> "${MOUNT_ROOT}/etc/pacman.conf"

[rustyos]
SigLevel = Optional TrustAll
Server = https://ragezorrr.github.io/RustyOS/repo/$arch
EOF
        fi
    fi
    if [[ "${resolved_source}" == "pacstrap" && -d /opt/rustyos-repo ]]; then
        mkdir -p "${MOUNT_ROOT}/opt/rustyos-repo"
        cp -a /opt/rustyos-repo/. "${MOUNT_ROOT}/opt/rustyos-repo/" || true
    fi

    mkdir -p "${MOUNT_ROOT}/etc/greetd"
    if [[ "${AUTOLOGIN}" -eq 1 ]]; then
        cat <<EOF > "${MOUNT_ROOT}/etc/greetd/config.toml"
[terminal]
vt = 1

[initial_session]
command = "/usr/bin/rustyos-session --drm"
user = "${USERNAME_VAL}"

[default_session]
command = "/usr/bin/rustyos-session --greeter"
user = "greeter"
EOF
    else
        cat <<'EOF' > "${MOUNT_ROOT}/etc/greetd/config.toml"
[terminal]
vt = 1

[default_session]
command = "/usr/bin/rustyos-session --greeter"
user = "greeter"
EOF
    fi

    # Configure Plymouth boot splash (`RUSTYOS-157`) and rebuild initramfs with `plymouth` hook.
    mkdir -p "${MOUNT_ROOT}/etc/plymouth"
    cat <<'EOF' > "${MOUNT_ROOT}/etc/plymouth/plymouthd.conf"
[Daemon]
Theme=rustyos
ShowDelay=0
DeviceTimeout=5
EOF
    if [[ -f "${MOUNT_ROOT}/etc/mkinitcpio.conf" ]] && ! grep -q '\bplymouth\b' "${MOUNT_ROOT}/etc/mkinitcpio.conf"; then
        if grep -q 'HOOKS=.*\bkms\b' "${MOUNT_ROOT}/etc/mkinitcpio.conf"; then
            sed -i 's/\(HOOKS=(.*kms\)/\1 plymouth/' "${MOUNT_ROOT}/etc/mkinitcpio.conf"
        else
            sed -i 's/\(HOOKS=(.*udev\)/\1 plymouth/' "${MOUNT_ROOT}/etc/mkinitcpio.conf"
        fi
    fi
    arch-chroot "${MOUNT_ROOT}" mkinitcpio -P

    local cfg_dir="${MOUNT_ROOT}/home/${USERNAME_VAL}/.config/rustyos"
    mkdir -p "${cfg_dir}"
    cat <<EOF > "${cfg_dir}/config.toml"
[keyboard]
layout = "${KB_LAYOUT}"
variant = ""
options = "grp:alt_shift_toggle"

[ui]
theme = "${APPEARANCE}"
theme_mode = "${APPEARANCE}"
accent_index = ${ACCENT_INDEX}
glass_opacity = 0.78
reduced_motion = false
reduce_transparency = false
EOF
    if [[ "${INSTALL_OOBE}" -eq 1 ]]; then
        : > "${cfg_dir}/.oobe-pending"
    fi
    arch-chroot "${MOUNT_ROOT}" chown -R "${USERNAME_VAL}:${USERNAME_VAL}" "/home/${USERNAME_VAL}/.config"

    arch-chroot "${MOUNT_ROOT}" systemctl enable NetworkManager greetd bluetooth
    install_optional_yay_bin

    emit_step 95 bootloader "Installing ${BOOTLOADER}"
    local quiet_opts="quiet splash loglevel=3 rd.udev.log_level=3 vt.global_cursor_default=0"
    if [[ "${BOOTLOADER}" == "grub" ]]; then
        if grep -q '^#GRUB_DISABLE_OS_PROBER=' "${MOUNT_ROOT}/etc/default/grub" 2>/dev/null; then
            sed -i 's/^#GRUB_DISABLE_OS_PROBER=.*/GRUB_DISABLE_OS_PROBER=false/' "${MOUNT_ROOT}/etc/default/grub"
        elif ! grep -q '^GRUB_DISABLE_OS_PROBER=' "${MOUNT_ROOT}/etc/default/grub" 2>/dev/null; then
            printf '\nGRUB_DISABLE_OS_PROBER=false\n' >> "${MOUNT_ROOT}/etc/default/grub"
        fi
        if grep -q '^GRUB_CMDLINE_LINUX_DEFAULT=' "${MOUNT_ROOT}/etc/default/grub" 2>/dev/null; then
            sed -i "s|^GRUB_CMDLINE_LINUX_DEFAULT=.*|GRUB_CMDLINE_LINUX_DEFAULT=\"${quiet_opts}\"|" "${MOUNT_ROOT}/etc/default/grub"
        else
            printf 'GRUB_CMDLINE_LINUX_DEFAULT="%s"\n' "${quiet_opts}" >> "${MOUNT_ROOT}/etc/default/grub"
        fi
        if grep -q '^#\?GRUB_THEME=' "${MOUNT_ROOT}/etc/default/grub" 2>/dev/null; then
            sed -i 's|^#\?GRUB_THEME=.*|GRUB_THEME="/usr/share/grub/themes/rustyos/theme.txt"|' "${MOUNT_ROOT}/etc/default/grub"
        else
            printf 'GRUB_THEME="/usr/share/grub/themes/rustyos/theme.txt"\n' >> "${MOUNT_ROOT}/etc/default/grub"
        fi
        if [[ -d /sys/firmware/efi ]]; then
            arch-chroot "${MOUNT_ROOT}" grub-install --target=x86_64-efi --efi-directory=/boot --bootloader-id=RustyOS
        else
            arch-chroot "${MOUNT_ROOT}" grub-install --target=i386-pc "${boot_disk}"
        fi
        arch-chroot "${MOUNT_ROOT}" grub-mkconfig -o /boot/grub/grub.cfg
    else
        arch-chroot "${MOUNT_ROOT}" bootctl install
        local root_uuid
        root_uuid="$(blkid -s UUID -o value "${root_part}")"
        local boot_timeout="0"
        if [[ "${INSTALL_MODE}" == "dual-boot" ]]; then
            boot_timeout="4"
        fi
        if [[ "${INSTALL_MODE}" == "whole-disk" || ! -f "${MOUNT_ROOT}/boot/loader/loader.conf" ]]; then
            cat <<EOF > "${MOUNT_ROOT}/boot/loader/loader.conf"
default rustyos.conf
timeout ${boot_timeout}
console-mode max
editor no
EOF
        fi
        mkdir -p "${MOUNT_ROOT}/boot/loader/entries"
        cat <<EOF > "${MOUNT_ROOT}/boot/loader/entries/rustyos.conf"
title   RustyOS
linux   /vmlinuz-linux
initrd  /initramfs-linux.img
options root=UUID=${root_uuid} rw ${quiet_opts}
EOF
        cat <<EOF > "${MOUNT_ROOT}/boot/loader/entries/rustyos-console.conf"
title   RustyOS (Console / Verbose)
linux   /vmlinuz-linux
initrd  /initramfs-linux.img
options root=UUID=${root_uuid} rw
EOF
    fi

    emit_step 100 done "RustyOS installation completed successfully"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --help|-h)
            usage
            exit 0
            ;;
        --list-disks-json)
            lsblk -Jpb -o NAME,KNAME,MODEL,SIZE,START,LOG-SEC,TYPE,TRAN,RM,FSTYPE,PARTTYPE,LABEL,PTTYPE,MOUNTPOINT,MOUNTPOINTS 2>/dev/null \
                || lsblk -Jpb -o NAME,KNAME,MODEL,SIZE,TYPE,TRAN,RM,FSTYPE,LABEL,MOUNTPOINT
            exit 0
            ;;
        --rollback-help)
            print_rollback_help
            exit 0
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --confirm-erase)
            CONFIRM_ERASE=1
            shift
            ;;
        --confirm-dualboot)
            CONFIRM_DUALBOOT=1
            shift
            ;;
        --install-mode)
            INSTALL_MODE="${2:-}"
            shift 2
            ;;
        --install-source)
            INSTALL_SOURCE="${2:-}"
            shift 2
            ;;
        --target-disk)
            TARGET_DISK="${2:-}"
            shift 2
            ;;
        --root-part)
            ROOT_PART="${2:-}"
            shift 2
            ;;
        --free-space)
            FREE_SPACE_SPEC="${2:-}"
            shift 2
            ;;
        --efi-part)
            EFI_PART="${2:-}"
            shift 2
            ;;
        --hostname)
            HOSTNAME_VAL="${2:-}"
            shift 2
            ;;
        --username)
            USERNAME_VAL="${2:-}"
            shift 2
            ;;
        --password-stdin)
            PASSWORD_STDIN=1
            shift
            ;;
        --locale)
            LOCALE_VAL="${2:-}"
            shift 2
            ;;
        --keymap)
            KEYMAP_VAL="${2:-}"
            shift 2
            ;;
        --kb-layout)
            KB_LAYOUT="${2:-}"
            shift 2
            ;;
        --timezone)
            TIMEZONE_VAL="${2:-}"
            shift 2
            ;;
        --bootloader)
            BOOTLOADER="${2:-}"
            shift 2
            ;;
        --appearance)
            APPEARANCE="${2:-}"
            shift 2
            ;;
        --accent)
            ACCENT_INDEX="${2:-}"
            shift 2
            ;;
        --autologin)
            AUTOLOGIN="${2:-}"
            shift 2
            ;;
        --install-oobe)
            INSTALL_OOBE="${2:-}"
            shift 2
            ;;
        *)
            echo "Error: unknown argument '$1'" >&2
            usage >&2
            exit 2
            ;;
    esac
done

validate_inputs
check_targets_safe

if [[ "${PASSWORD_STDIN}" -eq 1 ]]; then
    IFS= read -r PASSWORD_VAL || PASSWORD_VAL=""
fi
if [[ "${DRY_RUN}" -eq 0 && -z "${PASSWORD_VAL}" ]]; then
    echo "Error: non-empty user password via --password-stdin is required for real installation" >&2
    exit 2
fi

if [[ "${DRY_RUN}" -eq 1 ]]; then
    run_dry_plan
else
    install_system
fi
