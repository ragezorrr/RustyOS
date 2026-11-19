# RustyOS Arch Live ISO & Installer (`RUSTYOS-115`, ADR-0065)

## Build Live ISO (Arch Linux host with `archiso`)

1. Build `rustyos-git` package first (optional if installing from `gh-pages` `[rustyos]` repo):
   ```bash
   cd packaging/arch && PKGDEST=/tmp/rustyos-pkg makepkg -fs --noconfirm --nocheck
   ```
2. Build the bootable ISO:
   ```bash
   sudo ./packaging/archiso/build-iso.sh --local-repo-dir /tmp/rustyos-pkg --out-dir /tmp/rustyos-iso
   ```

## Test in QEMU (UEFI + VirtIO GPU)

```bash
qemu-system-x86_64 -enable-kvm -m 4096 -smp 4 \
  -bios /usr/share/edk2/x64/OVMF.4m.fd \
  -device virtio-vga-gl -display sdl,gl=on \
  -drive file=/tmp/rustyos-disk.qcow2,if=virtio \
  -cdrom /tmp/rustyos-iso/rustyos-live-*.iso -boot d
```

## Rollback & Recovery

Run `scripts/rustyos-install.sh --rollback-help` (or `/usr/lib/rustyos/rustyos-install --rollback-help`).
