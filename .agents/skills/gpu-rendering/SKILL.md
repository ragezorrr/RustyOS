---
description: Use for renderer, GPU resources, compositing, textures,
  damage and presentation.
name: gpu-rendering
---

# GPU Rendering

- Define resource ownership and lifetime.
- Avoid unnecessary copies and CPU readbacks.
- Track damage and redraw only what is needed.
- Respect GPU synchronization and buffer lifecycle.
- Separate capability detection from rendering policy.
- Add fallbacks for unsupported formats/features.
- Benchmark on target hardware; do not assume zero-copy or direct
  scanout.
