# ADR-0090: Dock Smart Hide, Multi-Monitor Isolation, Smooth `DockPresentation`, and App Lifecycle Polish (`RUSTYOS-161`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-104`
- **Task:** `RUSTYOS-161`

## Context
RustyOS's floating Liquid Glass Dock previously supported only a binary `dock_auto_hide: bool` toggle, rendered and accepted input solely on the primary output (`is_on_primary`), snapped slot and capsule coordinates instantaneously in 1 frame when apps opened/closed/minimized or pins reordered, relied on Z-order-sensitive window cycling, had no grace window for graceful `Quit`, and lacked metadata caching for pinned apps whose `.desktop` files temporarily disappear during package upgrades (`pacman -Syu`).

## Decision
1. **3-State Dock Visibility (`DockVisibilityMode`) & Settings UI:**
   - Introduce `DockVisibilityMode::{AlwaysShow, SmartHide, AlwaysHide}` in `crates/rusty-compositor/src/config.rs` persisted as `[dock].visibility` (`"always_show"`, `"smart_hide"`, `"always_hide"`).
   - Maintain full backwards compatibility with legacy `[dock].auto_hide = bool` (`effective_dock_visibility()` maps legacy `auto_hide = true` to `AlwaysHide` when `visibility` is unset, and `visibility` always takes precedence over `auto_hide`).
   - Both `SmartHide` and `AlwaysHide` expand `usable_work_area_for_output` across the Dock strip; `SmartHide` computes output-local resting Dock bounds (`dock_obstruction_region`) and hides the Dock only when a visible non-minimized window on that output is maximized, fullscreen, or intersects the Dock region, with a `5 px` edge reveal strip (`DOCK_REVEAL_STRIP_PX`) and `200 ms` pointer-leave grace (`DOCK_AUTO_HIDE_DELAY`).
   - Replace the binary `Auto-hide the Dock` switch in `apps/rusty-settings` (`SettingsPage::DockDesktop`) with a `Dock Visibility` choice control (`RowControlId::DockVisibilityCycle`).
2. **Multi-Monitor Independent Dock (`OutputDockState`):**
   - Track independent `OutputDockState` (`reveal_anim`, `hovered`, `obstructed`, `hide_deadline`, `presentation`) per connected `OutputId` in `crates/rusty-compositor/src/state/dock_state.rs`.
   - Route pointer hover, clicks, drag-reorder, Downloads stack (`DockStackState.output_id`), context menus (`open_dock_context_menu_on_output`), and minimize target rects to the active output.
   - Render Dock-only Liquid Glass chrome (`generate_dock_only_chrome_elements`) and Downloads Stack overlays on secondary outputs with independent fullscreen/occlusion suppression and backdrop blur activation.
3. **Smooth `DockPresentation` Layer (`crates/rusty-compositor/src/shell_render/dock_presentation.rs`):**
   - Smoothly interpolate slot axis centers, section dividers, capsule start/length (`240 ms` `EaseOutCubic`), entering icon scale-in, and departing icon shrink-out (`180 ms` `EaseInOutCubic`), snapping instantaneously when `reduced_motion = true` or output geometry/edge changes.
   - Keep `DockPresentation` main-axis motion orthogonal to cross-axis auto-hide reveal by passing `presented_layout` via `DockViewState`.
4. **Window Cycling, Graceful Quit Grace, and Pin Upgrade Cache:**
   - Select next app window deterministically via `select_dock_app_target_window` (stable ascending `WindowId` cycle when already focused; MRU restore via `dock_app_last_active` when returning from another app) with consistent `browser`/`firefox` grouping.
   - Track graceful Quit requests in `pending_app_quits` (`DOCK_QUIT_GRACE = 3000 ms`), automatically escalating the Dock context menu entry to danger-styled `Force Quit` while windows remain open and surfacing an OSD reminder after 3 s without auto-`SIGKILL`.
   - Cache pinned app metadata in `dock_pin_meta_cache` (`DOCK_PIN_UNINSTALL_GRACE = 3000 ms`) backed by `LauncherState::has_installed_dock_entry` so transient `.desktop` disappearances during upgrades preserve pin icons/labels and never drop pins.

## Consequences
- Users gain Smart Hide (dodge windows), independent multi-monitor Dock interaction, and smooth macOS-grade Dock slot/capsule transitions with zero new Cargo dependencies and zero event-loop blocking.
- `DockPresentation` establishes the foundation for Phase B (`RUSTYOS-162`: live minimized thumbnails, `CountBadge`, `inotify` `TrashMonitor`) and Phase C (`RUSTYOS-163`: Wayland DnD onto Dock).
