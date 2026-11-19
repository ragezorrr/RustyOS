#!/usr/bin/env bash
# shellcheck disable=SC2034
# RustyOS Arch Live ISO profile definition (`RUSTYOS-115`, ADR-0065)

iso_name="rustyos-live"
iso_label="RUSTYOS_$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y%m)"
iso_publisher="RustyOS Contributors <https://github.com/ragezorrr/RustyOS>"
iso_application="RustyOS Live Desktop & Installer"
iso_version="$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y.%m.%d)"
install_dir="rustyos"
buildmodes=('iso')
bootmodes=(
  'bios.syslinux.mbr'
  'bios.syslinux.eltorito'
  'uefi-ia32.grub.esp'
  'uefi-x64.systemd-boot.esp'
  'uefi-x64.systemd-boot.eltorito'
)
arch="x86_64"
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
airootfs_image_tool_options=('-comp' 'xz' '-Xbcj' 'x86' '-b' '1M' '-Xdict-size' '1M')
bootstrap_tarball_compression=('zstd' '-c' '-T0' '--auto-threads=logical' '--long' '-19')
file_permissions=(
  ["/etc/shadow"]="0:0:400"
  ["/etc/gshadow"]="0:0:400"
  ["/etc/sudoers.d/liveuser"]="0:0:440"
  ["/etc/polkit-1/rules.d/49-rustyos-live.rules"]="0:0:644"
)
