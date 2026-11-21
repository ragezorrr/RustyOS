# ADR-0081: Privileged `rustyos-installer-service` Backend and Offline SquashFS Support (`RUSTYOS-137`)

- **Status:** accepted
- **Date:** 2026-11-21
- **Task:** `RUSTYOS-137`

## Context

Following the architecture of `cutefishos/cutefish-installer` (`cutefish-installer-service` + `squashfs-tools`), RustyOS needed a typed, testable Rust installer backend binary that separates privileged disk/image operations from the unprivileged `apps/rusty-installer` Wayland UI and supports offline Live SquashFS (`airootfs.sfs`) deployment alongside the existing `pacstrap` fallback.

## Decision

1. **Add `crates/rusty-services/src/installer_service.rs` and binary `rustyos-installer-service`:**
   - `InstallSource::{LiveSquashFs(PathBuf), PackageRepo}` with automatic probing (`detect_live_squashfs_path`, `detect_install_source`).
   - `InstallerPlanRequest`, `validate_installer_request` (strict block device, POSIX username, hostname, and timezone validation), `build_install_stages` (8 ordered stages), and `InstallerStageEvent` (`STEP:` / `DONE:` / `FAIL:` line protocol).
   - `--detect-source` and `--dry-run` CLI modes for safe unprivileged verification without touching disks; live mode delegates to `/usr/lib/rustyos/rustyos-install`.
2. **Packaging & CI:**
   - Install `/usr/lib/rustyos/rustyos-installer-service` in `packaging/arch/PKGBUILD` and assert its presence in `.github/workflows/ci.yml`.

## Consequences

- `rustyos-installer-service` provides a clean, unit-tested Rust boundary between the installer UI and privileged OS deployment, matching `cutefish-installer-service` while preserving full compatibility with `scripts/rustyos-install.sh` and `Dual-Boot`.
