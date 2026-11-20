---
description: Use for panel, dock, launcher, notifications, overview and
  quick settings.
name: desktop-shell
---

# Desktop Shell

- Shell overlays (`TopBar`, `Dock`, `Launcher`, `QuickSettings`, `NotificationCenter`, `Overview`,
  `ContextMenu`, `LockScreen`, `Polkit`, `Portal`) run in the single-process compositor shell
  (`crates/rusty-compositor/src/shell_render/`, `ADR-0027`, `ADR-0072`) with resilient `rusty-ipc`
  control paths and standalone `rusty-settings` (do not recreate duplicate shell processes or modal
  Settings clones).
- Keep UI responsive; avoid blocking calls in rendering/event paths.
- Use shared design tokens (`crates/rusty-ui`, `DESIGN.md`) and accessible interaction states.
- Do not add a UI toolkit or process boundary without approval.
- Panel, Dock, quick settings, app-aware TopBar (`TopBarAppProfile::Desktop` default hiding `Go` on
  empty desktop; `Hide {App}` / `Hide Others` / `Quit` with `enabled` dim rows; clean `SystemLogo`
  menu; `8 px` active pill; Light/Dark `reduce_transparency` polarity), and overview follow
  `DESIGN.md` §12 (`RustyOS Light & Dark + Liquid Glass`) with CPU/opaque fallbacks when
  `reduce_transparency = true` or screen-locked.
- Verify front-to-back ordering in `frame_composer.rs`: modal scrims must only appear for true
  blocking modals (`Polkit`, `Portal`), never darkening `TopBar` or `Dock` behind non-blocking
  windows (`rusty-settings` etc.).
- Render all shell glass overlays (`Dock`, `TopBar` menus, `Spotlight` dual-capsule layout,
  `ControlModule` tiles, `App Switcher`, `Notification` cards, `LockScreen` input pill) via GPU
  `LiquidGlassElement` + 3-lobe `ShadowElevation`, leaving CPU `push_rounded_glass_box` only for
  inner badges/highlights.
- Keep `TopBar` dropdowns complete (`>` submenu chevrons, readable shortcuts) and avoid debug
  chrome (e.g. theme toggles in app status bars or backing rectangles behind status glyphs).
