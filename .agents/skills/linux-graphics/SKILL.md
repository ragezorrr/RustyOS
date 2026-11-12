---
description: Use for DRM/KMS, GBM, EGL, DMA-BUF, synchronization and
  display modes.
name: linux-graphics
---

# Linux Graphics

- Trace the complete kernel/userspace/driver/backend path.
- Discover capabilities at runtime.
- Handle device loss, hotplug, modeset failure and unsupported formats.
- Treat DMA-BUF modifiers and synchronization as compatibility concerns.
- Never assume all GPUs expose identical behavior.
- Keep nested backend available where practical.
- Record exact GPU, kernel, driver and userspace versions.
- Ask before changing graphics stack or core backend.
