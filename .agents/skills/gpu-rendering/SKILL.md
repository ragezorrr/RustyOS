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
- Liquid Glass (`surfaces/liquid_glass.rs`, ADR-0052/0053) runs a damage-gated Dual-Kawase FBO
  ladder (`1 -> half_a -> quarter_a -> eighth_a -> quarter_b -> half_out`) via safe Smithay
  `GlesTexProgram` / `Offscreen`, sampling `half_a` for `Dock` and `half_out` for
  `Popover`/`ControlModule` through the single unified superellipse lens shader
  (`LIQUID_GLASS_FRAG`, `14 px` edge lens `ior = 1.7`, directional top/bottom `uRim`, zero
  corner flares/sheen). Call `LiquidGlassPipeline::invalidate_all()` (via
  `UiShapeCache::sync_wallpaper_revision`) whenever `appearance` or wallpaper changes, suppress
  backdrop capture while locked, and fall back cleanly to CPU `UiShapeCache::glass_panel_element`.
- **Smithay/EGL Surface Invariant (critical):** any offscreen `renderer.bind(&mut texture)` pass
  (`LiquidGlassPipeline`, screenshot/portal capture) calls `egl.make_current()`, leaving the
  thread on `EGL_NO_SURFACE`. `WinitGraphicsBackend::bind()` does not restore the window surface;
  only `renderer.render(...)` calls `make_current_with_surface()`. When `OutputDamageTracker`
  returns `damage = None` on idle frames, explicitly call `renderer.render(&mut framebuffer, size,
  Transform::Normal)` + `frame.finish()` before `backend.submit()` so `submit()` and next-frame
  `buffer_age()` never hit `EGL_BAD_SURFACE` (`0x300d`).
- Validate every preprocessor permutation of GLSL ES 1.00 shaders (`NO_ALPHA`, `DEBUG_FLAGS`,
  `EXTERNAL`) with `/usr/bin/glslangValidator` after shader edits.
