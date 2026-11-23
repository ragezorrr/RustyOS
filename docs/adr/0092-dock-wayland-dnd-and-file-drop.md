# ADR-0092: Dock Server-Side Wayland DnD Drop Target, File-to-App Launch, and Spotlight/Desktop Drag-to-Dock (`RUSTYOS-163`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-106`
- **Task:** `RUSTYOS-163`

## Context
RustyOS renders the floating Liquid Glass Dock directly inside `rusty-compositor` rather than as an external layer-shell Wayland client. Consequently, during a client-initiated Wayland `wl_data_device.start_drag` (`text/uri-list`), Smithay's `DnDGrab` saw no client `WlSurface` under the Dock (`target == None`), ignored drops, sent `wl_data_source.cancelled()`, and kept `SmartHide`/`AlwaysHide` Docks concealed. In addition, `apps/rusty-files` only handled intra-window `FileDragState` drags without starting a Wayland `wl_data_device` drag when leaving the window, `LauncherApp` did not track `MimeType=` or expand `%f`/`%F`/`%u`/`%U` arguments, and Desktop/Spotlight drags could not drop files onto Dock apps or drag Spotlight apps directly into `[dock].pinned_apps`.

## Decision
1. **Server-Side Wayland `wl_data_device` Dock Drop Target (`crates/rusty-compositor/src/state/dock_dnd.rs`):**
   - Intercept `ClientDndGrabHandler::started` (`wayland_handlers/selection.rs`) to track `WlDataSource` instances offering `text/uri-list` (`ExternalDndState`).
   - During pointer motion over any output's Dock (`update_external_dnd_dock_hover`), auto-reveal `SmartHide`/`AlwaysHide` Docks, resolve `DockDropTarget::{App, Trash, Downloads, PinInsert}`, suppress underlying window focus (`under = None` so background clients receive `wl_data_device.leave()`), and negotiate `wl_data_source.target("text/uri-list")` + `wl_data_source.action(Copy | Move)`.
   - On pointer release over a Dock target (`begin_external_dnd_dock_drop`), request `text/uri-list` via `std::io::pipe()` + `source.dnd_drop_performed()` + `source.send(...)`, read asynchronously on a bounded background thread (`read_fd_bounded_with_timeout` in `crates/rusty-services/src/trash_monitor.rs`, `256 KiB` cap, `1500 ms` timeout) waking the compositor via `calloop::ping`, emit `source.dnd_finished()`, and replay the deferred release to end `DnDGrab` cleanly without sending premature `cancelled()`.
   - Parse RFC 2483 `text/uri-list` strictly (`parse_uri_list`: local `file:///` and `file://localhost/` only, percent-decoding, NUL rejection, deduplication, `64`-path cap).
2. **MIME/Exec File Dispatch, Background Trash/Downloads, and `.desktop` Pinning:**
   - Extend `LauncherApp` ([launcher.rs](file:///home/anc/RustyOS/crates/rusty-compositor/src/launcher.rs)) with `mime_types: Vec<String>`, `expand_exec_argv_with_paths` (`%f`/`%F`/`%u`/`%U` + positional fallback), `mime_for_path`, `desktop_mime_matches`, `app_accepts_dropped_paths`, `launch_by_dock_key_with_paths` (including `current_dir` for `terminal` and parent-dir resolution for `files`), and `resolve_desktop_file_to_dock_key`.
   - Execute `Trash` (`move_to_trash`) and `Downloads` (unique non-overwriting copy) drops on background threads via `DockDndMailbox` + `calloop::ping` so the compositor event loop never blocks.
   - Dropping `.desktop` files onto `PinInsert` (or onto `Pinned` slots) parses and pins them at the target index via `pin_app_at_index`.
3. **Unified Dock Drop Visuals, Desktop Canvas & Spotlight Drag-to-Dock, and `rusty-files` Wayland DnD Export:**
   - Add `DockDropHighlight::{Slot, PinInsert}` (`Copy`-preserving on `DockViewState`) rendering `#007AFF`/`#0A84FF` (or `#FF3B30`/`#FF453A` for `Trash`) target halos, smooth `DockPresentation` insertion gaps (`drop_insert_rect`), and `"Open in <App>"` / `"Move to Trash"` / `"Copy to Downloads"` / `"Pin <App>"` glass pills.
   - Extend `DesktopDragState` (`dock_drop_target`) to drop desktop files onto any Dock app, Downloads, Trash, or `.desktop` pin position, and add `SpotlightDragState` to drag Spotlight app rows directly onto the Dock `Pinned` strip with a floating icon preview.
   - Wire `apps/rusty-files/src/wayland.rs` to hand off active `FileDragState` drags crossing the window edge to `wl_data_device.start_drag` offering `text/uri-list` and `text/plain;charset=utf-8` (`Copy | Move`), reloading the active column when `dnd_finished` completes a `Move` action.

## Consequences
- Users can drag files from `rusty-files`, Desktop Canvas, or any third-party Wayland/XWayland app onto Dock icons, `Downloads`, or `Trash`, and drag `.desktop` files or Spotlight apps directly into the Dock's pinned strip.
- Zero new external Cargo dependencies and zero `unsafe` code added to `crates/rusty-compositor`; client-to-client Wayland DnD remains unaffected.
