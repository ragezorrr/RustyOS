---
description: Use for panel, dock, launcher, notifications, overview and
  quick settings.
name: desktop-shell
---

# Desktop Shell

- Shell should have lifecycle independent from compositor.
- Prefer resilient IPC and clear failure handling.
- Keep UI responsive; avoid blocking calls in rendering/event paths.
- Use shared design tokens and accessible interaction states.
- Shell crash/restart must not unnecessarily terminate compositor.
- Do not add a UI toolkit or process boundary without approval.
- Panel, Dock, quick settings, app-aware TopBar (`TopBarAppProfile`, `Files` default), and
  overview follow `DESIGN.md` §12 (`RustyOS Light/Dark + Liquid Glass`) with
  CPU/opaque fallbacks when `reduce_transparency = true` or screen-locked.
- Verify front-to-back ordering in `frame_composer.rs`: modal scrims must only appear for true
  blocking modals (`Polkit`, `Portal`), never darkening `TopBar` or `Dock` behind non-blocking
  settings windows.
- Render all shell glass overlays (`Dock`, `TopBar` menus, `Spotlight` dual-capsule layout,
  `ControlModule` tiles, `App Switcher`, `Notification` cards, `LockScreen` input pill) via GPU
  `LiquidGlassElement`, leaving CPU `push_rounded_glass_box` only for inner badges/highlights.
- Keep `TopBar` dropdowns complete (`>` submenu chevrons, macOS-readable shortcuts) and avoid
  debug chrome (e.g. theme toggles in app status bars or backing rectangles behind status glyphs).
