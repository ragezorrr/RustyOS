# ADR-0078: Removable Storage Service (`UDisks2`/`lsblk`) and Internal File Drag-and-Drop in `rusty-files` (`RUSTYOS-134`)

- **Status:** accepted
- **Date:** 2026-11-21
- **Task:** `RUSTYOS-134`

## Context

`apps/rusty-files` already implemented Quick Look (`Space`), background chunked copy/move with progress card and cancel, and `/proc/mounts` volumes, but lacked a dedicated least-privilege removable storage adapter (`UDisks2` / `lsblk` mount & safe eject) and internal file drag-and-drop onto directories.

## Decision

1. **`StorageService` in `crates/rusty-services/src/storage.rs`:**
   - Add `RemovableVolume`, `is_valid_block_device_path` (strict `/dev/...` path guard rejecting traversal and shell metacharacters), `parse_lsblk_removable_volumes`, and `StorageService::{list_removable_volumes, mount_volume, unmount_and_eject}` (`lsblk -P -b` + `udisksctl --no-user-interaction`).
2. **`apps/rusty-files` Integration:**
   - Store `removable_volumes` and expose `refresh_removable_volumes`, `eject_removable_volume`, and `mount_or_open_removable_volume` on `FilesAppState`.
   - Add `FileDragState` (6 px drag threshold + self/descendant drop guard) wired into `apps/rusty-files/src/wayland.rs` pointer press/motion/release and `action_drop_paths` reusing the existing background `start_background_transfer` progress worker.

## Consequences

- Least-privilege removable drive discovery/mount/eject works across Linux setups with graceful fallback when `udisksctl` is absent, and internal drag-and-drop reuses the non-blocking transfer pipeline.
