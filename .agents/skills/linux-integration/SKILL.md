---
description: Use for PipeWire, NetworkManager, BlueZ, Polkit, portals,
  XWayland and logind.
name: linux-integration
---

# Linux Integration

- Prefer existing system services over reimplementing them.
- Use documented APIs and authorization mechanisms.
- Handle service absence, restart and permission errors.
- Do not elevate privileges in the shell.
- Keep optional integrations isolated.
- System-level changes require approval.
