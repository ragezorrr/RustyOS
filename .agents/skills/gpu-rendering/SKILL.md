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
  `Popover`/`ControlModule` through the unified continuous cubic squircle lens shader
  (`LIQUID_GLASS_FRAG`, `14 px` edge lens `ior = 1.7`, directional top/bottom `uRim`, zero
  corner flares/sheen). Call `LiquidGlassPipeline::invalidate_all()` (via
  `UiShapeCache::sync_wallpaper_revision`) whenever `appearance` or wallpaper changes, suppress
  backdrop capture while locked, and fall back cleanly to CPU `UiShapeCache::glass_panel_element`.
- Analytical 3-Lobe Drop Shadows (`surfaces/window_shadow.rs`, `ADR-0070`/`0072`) evaluate wide ambient,
  narrow directional, and `1.5 px` contact Gaussian lobes (`u_lobe1..3`, matching `ShadowStyle`)
  with unified `squircleDistance` (`r * 1.42` for corners, L2 for capsules) and 4x4 ordered dithering.
- Compositor Client Corner Clipping (`surfaces/window_clip.rs`, `ClippedSurfaceElement`, `ADR-0072`):
  wraps all non-fullscreen `WaylandSurfaceRenderElement`s (`SURFACE_CLIP_FRAG`) using inverse 2x2
  `frame.projection()` (`gl_FragCoord` -> physical coords, transform- & FBO-invariant) + 1x1 white
  carrier texture for `SolidColor` subsurfaces, trims corner `opaque_regions` so `GL_BLEND` stays
  active on corners, returns `UnderlyingStorage::None` when clipped, and leaves `is_fullscreen` raw
  for Direct Scanout.
- Offscreen Snapshot Lifecycle (`surfaces/window_minimize.rs`, `WINDOW_MINIMIZE_FRAG`): serves both
  Dock `Genie`/`Scale` minimize/restore warps and centered `0.93 <-> 1.0` fade+scale open/close
  transitions from pooled FBO snapshots keyed by surface commit revisions.
- **Smithay/EGL Surface Invariant (critical):** any offscreen `renderer.bind(&mut texture)` pass
  (`LiquidGlassPipeline`, `capture_into_slot`, screenshot/portal capture) calls `egl.make_current()`,
  leaving the thread on `EGL_NO_SURFACE`. `WinitGraphicsBackend::bind()` does not restore the window
  surface; only `renderer.render(...)` calls `make_current_with_surface()`. When
  `OutputDamageTracker` returns `damage = None` on idle frames, explicitly call
  `renderer.render(&mut framebuffer, size, Transform::Normal)` + `frame.finish()` before
  `backend.submit()` so `submit()` and next-frame `buffer_age()` never hit `EGL_BAD_SURFACE` (`0x300d`).
- Validate every preprocessor permutation of all GLSL ES 1.00 shaders (`KAWASE_DOWN_FRAG`,
  `KAWASE_UP_FRAG`, `LIQUID_GLASS_FRAG`, `WINDOW_SHADOW_FRAG`, `WINDOW_MINIMIZE_FRAG`,
  `SURFACE_CLIP_FRAG`) across `NO_ALPHA`, `DEBUG_FLAGS`, and `EXTERNAL` with `/usr/bin/glslangValidator`.
