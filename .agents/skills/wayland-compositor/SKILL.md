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
- Test with known Wayland clients (`rusty-files`, `rusty-settings`, `rusty-terminal`, XWayland)
  and record environment.
- Wrap non-fullscreen `WaylandSurfaceRenderElement`s in `ClippedSurfaceElement` (`WINDOW_RADIUS = 14`,
  `UnderlyingStorage::None`) while leaving fullscreen windows unwrapped for Direct Scanout; keep
  `CompositorHandler::commit` surface revision tracking accurate for lifecycle snapshots.
- Support `rustyos_liquid_glass_v1` (`GlassStyle::Sidebar` + `WlRegion` sidebar scoping on client
  apps and `xdg_popup`s) and enforce `wl_surface` damage/opaque region trimming around clipped corners.
- When using offscreen FBO passes (`LiquidGlassPipeline`, snapshot capture), preserve the Winit EGL
  window surface invariant on idle frames (`Damage == None`) by calling `renderer.render(...)` +
  `frame.finish()` before `backend.submit()`.
- Do not invent protocol behavior; consult protocol definitions.
- Architecture and API changes require approval.
