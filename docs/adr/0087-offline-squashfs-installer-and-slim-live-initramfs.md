# ADR-0087: Offline `SquashFS` Copy Installer & Slim Live ISO `initramfs` (`RUSTYOS-158`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-101`
- **Task:** `RUSTYOS-158`

## Context
Testing the Live ISO on real UEFI hardware exposed two systemic issues:
1. Including `kms` in `packaging/archiso/airootfs/etc/mkinitcpio.conf.d/archiso.conf` (which has no `autodetect` hook) caused `mkinitcpio` to embed 143.65 MiB of uncompressed GPU firmware (`usr/lib/firmware`) and 38.12 MiB of GPU kernel modules into the early CPIO, inflating `initramfs-linux.img` to 227 MiB. On strict UEFI firmware (such as HP laptops), this caused `initrd` load/unpack failure before `/init`, triggering `KERNEL PANIC: VFS: Unable to mount root fs on unknown-block(0,0)`.
2. `scripts/rustyos-install.sh` always required an active internet connection and deployed via `pacstrap`, causing slow installs, mirror failures, and recurring package drift between Live ISO and installed systems. Meanwhile, `crates/rusty-services/src/installer_service.rs` probed only `/run/archiso/bootmnt/arch/x86_64/airootfs.sfs`, missing our `install_dir="rustyos"` path.

## Decision
1. **Slim Live ISO `initramfs` (`archiso.conf`):**
   - Match official Arch `releng` by omitting `kms` from Live ISO's `archiso.conf` and enabling `COMPRESSION="zstd"` (`-19`), reducing `initramfs-linux.img` from ~227 MiB to ~40–45 MiB. Built-in `simpledrm` (UEFI GOP) drives Plymouth during early Live boot, and full GPU modules load from `airootfs.sfs` after root switch. Installed systems retain `autodetect` + `kms plymouth` (~15–20 MiB).
2. **Offline `SquashFS` Copy Installer (`scripts/rustyos-install.sh` & `installer_service.rs`):**
   - Add `/run/archiso/bootmnt/rustyos/x86_64/airootfs.sfs` as the primary candidate in `LIVE_SQUASHFS_CANDIDATES` (`installer_service.rs`) and `detect_live_squashfs()` (`rustyos-install.sh`).
   - Add `--install-source <auto|squashfs|pacstrap>` (default `auto`): when a live `airootfs.sfs` is present, skip the mandatory internet preflight, extract `airootfs.sfs` via `unsquashfs -f -d "${MOUNT_ROOT}"`, copy `vmlinuz-linux` from `/usr/lib/modules/*/vmlinuz` (or bootmnt) to `/boot/vmlinuz-linux`, remove live-only artifacts (`liveuser`, `49-rustyos-live.rules`, `archiso.conf`, `mkinitcpio-archiso`, `99-rustyos-liveuser.hook`, `install-rustyos.desktop`, `/opt/rustyos-repo`), reset `/etc/machine-id`, initialize offline `pacman-key`, rebuild `mkinitcpio -P` with `autodetect` + `kms plymouth`, and install `systemd-boot` / `GRUB`.
   - Preserve online `pacstrap` as an automatic fallback when no live SquashFS is available (or when `--install-source pacstrap` is passed).

## Consequences
- Live ISO `initramfs` shrinks ~5× and boots reliably on strict UEFI hardware.
- Offline installations complete in ~1–2 minutes with zero internet dependency and 100% package parity with the tested Live ISO.
- Zero new Cargo dependencies.
