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
