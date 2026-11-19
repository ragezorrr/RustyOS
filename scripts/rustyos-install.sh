#!/usr/bin/env bash
# RustyOS System Installer & Recovery Backend (`RUSTYOS-115`, ADR-0065)
# Safety: never modifies disks when `--dry-run` is active; refuses mounted/root disks.
set -euo pipefail

DRY_RUN=0
CONFIRM_ERASE=0
TARGET_DISK=""
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
  --list-disks-json          Print block devices JSON via lsblk -Jpb
  --rollback-help            Print rollback and recovery instructions
  --dry-run                  Validate inputs and print full plan without touching disks

Install options:
  --target-disk <dev>        Target whole block device (e.g. /dev/vda, /dev/nvme0n1)
  --confirm-erase            Required when --dry-run is NOT set
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

validate_inputs() {
    if [[ -z "${TARGET_DISK}" ]]; then
        echo "Error: --target-disk is required" >&2
        exit 2
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

check_target_disk_safe() {
    local dev="$1"
    if ! [[ "${dev}" =~ ^/dev/(sd[a-z]+|vd[a-z]+|xvd[a-z]+|nvme[0-9]+n[0-9]+|mmcblk[0-9]+|loop[0-9]+)$ ]]; then
        echo "Error: unsupported or unsafe block device path '${dev}'" >&2
        exit 3
    fi

    if [[ "${DRY_RUN}" -eq 0 ]]; then
        if [[ ! -b "${dev}" ]]; then
            echo "Error: block device '${dev}' does not exist" >&2
            exit 3
        fi
        local mounted
        mounted="$(lsblk -nr -o MOUNTPOINTS "${dev}" 2>/dev/null | grep -v '^$' || true)"
        if [[ -n "${mounted}" ]]; then
            echo "Error: target disk '${dev}' has active mountpoints (${mounted}); refusing to erase" >&2
            exit 3
        fi
        local root_src
        root_src="$(findmnt -no SOURCE / 2>/dev/null || true)"
        if [[ -n "${root_src}" && "${root_src}" == "${dev}"* ]]; then
            echo "Error: '${dev}' backs the running root filesystem; refusing to erase" >&2
            exit 3
        fi
    fi
}

run_dry_plan() {
    local efi_part root_part boot_pkg
    efi_part="$(partition_suffix "${TARGET_DISK}" 1)"
    root_part="$(partition_suffix "${TARGET_DISK}" 2)"
    boot_pkg="efibootmgr"
    if [[ "${BOOTLOADER}" == "grub" ]]; then
        boot_pkg="grub efibootmgr"
    fi

    emit_step 5 validate "Validated target=${TARGET_DISK}, user=${USERNAME_VAL}, host=${HOSTNAME_VAL}, bootloader=${BOOTLOADER}"
    emit_step 15 partition "[dry-run] Would wipe GPT and create ${efi_part} (512M EFI FAT32) + ${root_part} (RustyOS ext4)"
    emit_step 30 mount "[dry-run] Would mount ${root_part} -> ${MOUNT_ROOT} and ${efi_part} -> ${MOUNT_ROOT}/boot"
    emit_step 55 pacstrap "[dry-run] Would run pacstrap -K ${MOUNT_ROOT}: base linux linux-firmware networkmanager pipewire wireplumber seatd polkit mesa xorg-xwayland firefox greetd sudo ${boot_pkg} rustyos-git"
    emit_step 75 system "[dry-run] Would set timezone=${TIMEZONE_VAL}, locale=${LOCALE_VAL}, keymap=${KEYMAP_VAL}, hostname=${HOSTNAME_VAL}, user=${USERNAME_VAL}"
    emit_step 88 desktop "[dry-run] Would write /etc/greetd/config.toml (autologin=${AUTOLOGIN}) and ~/.config/rustyos/config.toml (theme=${APPEARANCE}, accent=${ACCENT_INDEX}, kb=${KB_LAYOUT}, oobe=${INSTALL_OOBE})"
    emit_step 96 bootloader "[dry-run] Would install ${BOOTLOADER} onto ${TARGET_DISK} and generate initramfs"
    emit_step 100 done "[dry-run] Installation plan verified cleanly (no disk writes performed)"
}

install_system() {
    if [[ "$(id -u)" -ne 0 ]]; then
        echo "Error: real installation requires root privileges (use pkexec / sudo, or pass --dry-run)" >&2
        exit 4
    fi
    if [[ "${CONFIRM_ERASE}" -ne 1 ]]; then
        echo "Error: pass --confirm-erase to confirm destructive partitioning of ${TARGET_DISK}" >&2
        exit 2
    fi
    for tool in sgdisk mkfs.fat mkfs.ext4 pacstrap genfstab arch-chroot partprobe; do
        if ! command -v "${tool}" >/dev/null 2>&1; then
            echo "Error: required installation tool '${tool}' not found in PATH" >&2
            exit 5
        fi
    done

    local efi_part root_part boot_pkg
    efi_part="$(partition_suffix "${TARGET_DISK}" 1)"
    root_part="$(partition_suffix "${TARGET_DISK}" 2)"
    boot_pkg="efibootmgr"
    if [[ "${BOOTLOADER}" == "grub" ]]; then
        boot_pkg="grub efibootmgr"
    fi

    cleanup_mounts() {
        umount -R "${MOUNT_ROOT}" >/dev/null 2>&1 || true
    }
    trap cleanup_mounts EXIT

    emit_step 5 validate "Checking hardware firmware and disk ${TARGET_DISK}"
    umount -R "${MOUNT_ROOT}" >/dev/null 2>&1 || true
    mkdir -p "${MOUNT_ROOT}"

    emit_step 15 partition "Partitioning ${TARGET_DISK} (GPT: EFI 512 MiB + RustyOS root)"
    sgdisk --zap-all "${TARGET_DISK}"
    sgdisk -n 1:0:+512M -t 1:ef00 -c 1:RUSTYOS_EFI "${TARGET_DISK}"
    sgdisk -n 2:0:0 -t 2:8300 -c 2:RUSTYOS_ROOT "${TARGET_DISK}"
    partprobe "${TARGET_DISK}" || true
    udevadm settle --timeout=10 || sleep 2

    emit_step 28 format "Formatting EFI (${efi_part}) and root (${root_part})"
    mkfs.fat -F32 "${efi_part}"
    mkfs.ext4 -F -L RUSTYOS "${root_part}"

    emit_step 35 mount "Mounting target filesystems"
    mount "${root_part}" "${MOUNT_ROOT}"
    mkdir -p "${MOUNT_ROOT}/boot"
    mount "${efi_part}" "${MOUNT_ROOT}/boot"

    local pacman_conf_arg=()
    if [[ -f /etc/pacman-rustyos-install.conf ]]; then
        pacman_conf_arg=(-C /etc/pacman-rustyos-install.conf)
    fi

    emit_step 45 pacstrap "Installing base system, Wayland desktop, and RustyOS packages"
    # shellcheck disable=SC2086
    pacstrap -K "${pacman_conf_arg[@]}" "${MOUNT_ROOT}" \
        base linux linux-firmware networkmanager pipewire wireplumber \
        seatd polkit mesa xorg-xwayland firefox greetd sudo \
        ${boot_pkg} rustyos-git

    emit_step 75 system "Configuring timezone, locale, hostname, and user ${USERNAME_VAL}"
    genfstab -U "${MOUNT_ROOT}" >> "${MOUNT_ROOT}/etc/fstab"

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

    emit_step 85 desktop "Configuring greetd, NetworkManager, seatd, and RustyOS preferences"
    if [[ -f /etc/pacman.conf ]] && grep -q '^\[rustyos\]' /etc/pacman.conf && ! grep -q '^\[rustyos\]' "${MOUNT_ROOT}/etc/pacman.conf"; then
        awk '/^\[rustyos\]/,/^$/' /etc/pacman.conf >> "${MOUNT_ROOT}/etc/pacman.conf"
    fi
    if [[ -d /opt/rustyos-repo ]]; then
        mkdir -p "${MOUNT_ROOT}/opt/rustyos-repo"
        cp -a /opt/rustyos-repo/. "${MOUNT_ROOT}/opt/rustyos-repo/" || true
    fi

    mkdir -p "${MOUNT_ROOT}/etc/greetd"
    if [[ "${AUTOLOGIN}" -eq 1 ]]; then
        cat <<EOF > "${MOUNT_ROOT}/etc/greetd/config.toml"
[terminal]
vt = 1

[default_session]
command = "/usr/bin/rustyos-session --drm"
user = "${USERNAME_VAL}"
EOF
    else
        cat <<'EOF' > "${MOUNT_ROOT}/etc/greetd/config.toml"
[terminal]
vt = 1

[default_session]
command = "/usr/bin/agreety --cmd /usr/bin/rustyos-session"
user = "greeter"
EOF
    fi

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

    arch-chroot "${MOUNT_ROOT}" systemctl enable NetworkManager seatd greetd

    emit_step 95 bootloader "Installing ${BOOTLOADER}"
    if [[ "${BOOTLOADER}" == "grub" ]]; then
        if [[ -d /sys/firmware/efi ]]; then
            arch-chroot "${MOUNT_ROOT}" grub-install --target=x86_64-efi --efi-directory=/boot --bootloader-id=RustyOS
        else
            arch-chroot "${MOUNT_ROOT}" grub-install --target=i386-pc "${TARGET_DISK}"
        fi
        arch-chroot "${MOUNT_ROOT}" grub-mkconfig -o /boot/grub/grub.cfg
    else
        arch-chroot "${MOUNT_ROOT}" bootctl install
        local root_uuid
        root_uuid="$(blkid -s UUID -o value "${root_part}")"
        cat <<'EOF' > "${MOUNT_ROOT}/boot/loader/loader.conf"
default rustyos.conf
timeout 3
console-mode max
editor no
EOF
        cat <<EOF > "${MOUNT_ROOT}/boot/loader/entries/rustyos.conf"
title   RustyOS
linux   /vmlinuz-linux
initrd  /initramfs-linux.img
options root=UUID=${root_uuid} rw quiet splash
EOF
    fi

    emit_step 100 done "RustyOS installed cleanly on ${TARGET_DISK}; ready to reboot"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --help|-h)
            usage
            exit 0
            ;;
        --list-disks-json)
            lsblk -Jpb -o NAME,KNAME,MODEL,SIZE,TYPE,TRAN,RM,MOUNTPOINTS 2>/dev/null \
                || lsblk -Jpb -o NAME,KNAME,MODEL,SIZE,TYPE,TRAN,RM,MOUNTPOINT
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
        --target-disk)
            TARGET_DISK="${2:-}"
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
check_target_disk_safe "${TARGET_DISK}"

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
