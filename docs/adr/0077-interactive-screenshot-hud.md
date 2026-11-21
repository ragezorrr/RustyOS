# ADR-0077: Interactive Screenshot HUD (`Region / Window / Full Screen`, Clipboard & File Save, `RUSTYOS-133`)

- **Status:** accepted
- **Date:** 2026-11-21
- **Task:** `RUSTYOS-133`

## Context

`crates/rusty-compositor/src/screenshot.rs` previously supported only immediate `FullScreen` / `FocusedWindow` captures and XDG Portal requests. Users lacked an interactive macOS/CutefishOS-style screenshot workflow with arbitrary rectangular region selection, live window hover highlight, mode switching, and clipboard-only vs. file-save choice.

## Decision

1. **Compositor Shell Overlay `ShellOverlayView::ScreenshotHud`:**
   - Add `crates/rusty-compositor/src/shell_render/modals/screenshot_hud.rs` (`ScreenshotMode::{Region, Window, FullScreen}`, `ScreenshotHudState`, `ScreenshotHudHit`, `normalize_selection_rect`, `hit_test_screenshot_hud`, `render_screenshot_hud_text`, `render_screenshot_hud_composite`).
   - Render a centered bottom Liquid Glass pill (`GlassPanelStyle::Popover`, `ShadowElevation::Popup`) with mode buttons (`Region`, `Window`, `Full Screen`), `Save` / `Copy` toggle, `Capture`, and `Cancel`, plus a 4-part outer dim scrim, white selection border, and `W × H` badge in `Region` mode, or blue hover highlight in `Window` mode.
2. **Capture Pipeline & Input Routing:**
   - Extend `ScreenshotRequest` with `FullScreenCopyOnly` and `Region { rect, copy_only }` and add `spawn_copy_screenshot_only` (`wl-copy --type image/png` with graceful fallback to `~/Pictures/Screenshots/RustyOS_*.png`).
   - Route pointer drag/hover/click (`handle_screenshot_hud_motion`, `handle_screenshot_hud_button`) and keyboard (`Esc`, `Enter`/`Space`, `1`/`2`/`3`, plus `Super+Shift+4` and `TopBar` → `File` → `Screenshot...`) directly in `rusty-compositor`.
   - Dismiss `screenshot_hud` before queueing `pending_screenshot` so the HUD and scrim never appear in the captured image.

## Consequences

- Zero-latency interactive screenshots on both `--winit` (Wayland/X11) and DRM/KMS without IPC roundtrips or blocking the compositor event loop.
