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
- Standard desktop windows (including `System Settings` and utilities) should participate in
  normal WM focus/drag/stacking/Overview without full-screen modal dimming over `TopBar`/`Dock`.
- Prefer live window/workspace thumbnail composition in Overview/Switcher over static placeholders.
- Public IPC or WM architecture changes require approval.
