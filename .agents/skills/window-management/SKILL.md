---
description: Use for focus, workspaces, placement, stacking, outputs and
  shortcuts.
name: window-management
---

# Window Management

- Keep WM state deterministic and testable.
- Separate policy from low-level rendering.
- Define focus and stacking rules explicitly.
- Test multi-window, multi-workspace and multi-output scenarios.
- Preserve keyboard accessibility.
- Standard desktop windows (including `rusty-settings` and utilities) participate in normal WM
  focus/drag/stacking/Overview without full-screen modal dimming over `TopBar`/`Dock`.
- Client windows (`!is_fullscreen`) receive compositor-enforced `WINDOW_RADIUS = 14` continuous
  cubic squircle corner clipping (`ClippedSurfaceElement`) while fullscreen windows keep raw
  `WaylandSurfaceRenderElement`s for unoccluded Direct Scanout.
- Respect `xdg_toplevel::State::Activated` focus state in client chrome (`inactive_color()` muted
  traffic lights when unfocused) and drive open (`180 ms` `EaseOutQuart`, `0.93 -> 1.0` + fade) and
  close (`160 ms` `EaseOutCubic`, `1.0 -> 0.93` + fade) transitions via `lifecycle_anims`.
- Prefer live window/workspace thumbnail composition in Overview/Switcher over static placeholders.
- Public IPC or WM architecture changes require approval.
