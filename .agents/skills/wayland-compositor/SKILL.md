---
description: Use for Wayland server, Smithay, surfaces, outputs, input
  and lifecycle.
name: wayland-compositor
---

# Wayland Compositor

- Verify current Smithay version and backend APIs before coding.
- Start with nested Winit prototype; do not skip ahead to DRM/KMS.
- Keep client lifecycle, surfaces, outputs, input and presentation
  responsibilities clear.
- Validate protocol state and handle client disconnects.
- Keep event-loop callbacks short and non-blocking.
- Test with known Wayland clients and record environment.
- Do not invent protocol behavior; consult protocol definitions.
- Architecture and API changes require approval.
