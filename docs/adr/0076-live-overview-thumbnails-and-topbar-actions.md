# ADR-0076: Live GPU Window Thumbnails in Workspace Overview and Expanded TopBar Window Actions (`RUSTYOS-132`)

- **Status:** accepted
- **Date:** 2026-11-21
- **Task:** `RUSTYOS-132`

## Context

`Workspace Overview` (`Exposé` / `Mission Control`) previously rendered semi-transparent glass placeholder cards with centered `"badge • dimensions"` text instead of real window contents. Meanwhile, `TopBar`'s `Window` menu lacked tiling and workspace/display migration actions already supported by `rusty-wm`, and `About RustyOS` / `Keyboard Shortcuts` only emitted toast notifications instead of opening the dedicated `rusty-settings` pages added in `RUSTYOS-131`.

## Decision

1. **Live GPU Window Thumbnails in `Workspace Overview`:**
   - Reuse `WindowMinimizePipeline`'s `commit_sig`-cached offscreen `lifecycle_snapshots` (`crates/rusty-compositor/src/surfaces/window_minimize.rs`) so windows are captured once per commit signature change (zero per-frame FBO overhead when windows are idle, live updates when clients commit new frames).
   - Add `compute_aspect_fit_rect`, `has_lifecycle_snapshot`, and `render_overview_thumbnail_element` on `WindowMinimizePipeline` / `UiShapeCache`, rendering each thumbnail via `WINDOW_MINIMIZE_FRAG` (`sample_filtered` 4-tap RGSS downsampling) centered with aspect-fit inside the Overview card.
   - Skip the `"badge • dimensions"` placeholder text in `render_overview_text` whenever a live GPU thumbnail is present, retaining it as fallback for minimized or uncommitted windows.
2. **Expanded `TopBar` Actions:**
   - Add `TileLeftFocused`, `TileRightFocused`, `MoveToNextWorkspace`, `MoveToPrevWorkspace`, and `MoveToNextDisplay` to `TopBarMenuAction` / `MENU_WINDOW` and dispatch them via `WmAction` in `state/shell_actions.rs`.
   - Route `TopBarMenuAction::AboutRustyOs` and `TopBarMenuAction::ShowShortcuts` directly to `open_settings_page(Some("about"))` and `open_settings_page(Some("shortcuts"))`.

## Consequences

- `Workspace Overview` displays crisp, live-updating window previews on both `--winit` (Wayland/X11) and DRM/KMS without CPU readback or extra `unsafe` code.
- `TopBar` provides complete window management and direct navigation to `About` and `Shortcuts` in `rusty-settings`.
