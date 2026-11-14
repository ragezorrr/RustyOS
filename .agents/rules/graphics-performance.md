---
description: Apply to compositor rendering, GPU, DRM/KMS, cursor, frame
  scheduling and animations.
trigger: model_decision
---

# Graphics and Performance

- Measure frame pacing and presentation timing; FPS alone is
  insufficient.
- Never block the compositor event loop with slow IO or expensive CPU
  work.
- Detect capabilities at runtime.
- Never assume hardware cursor, VRR, DMA-BUF, modifiers, explicit sync
  or direct scanout.
- Provide safe fallback when an optional capability is unavailable.
- Record GPU, kernel, driver, display mode and backend in benchmark
  reports.
- Do not claim support without reproducible hardware tests.
- When changing EGL/GLES, damage tracking, or frame loop paths, verify
  live `--winit` runs on both Wayland and X11 with zero `EGL_BAD_SURFACE`
  or fallback warnings, plus `--drm-probe` when DRM capabilities are touched.
