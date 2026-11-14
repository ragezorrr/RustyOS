---
description: Use for visual components, interaction flows, accessibility
  and design tokens.
name: ui-ux-design
---

# UI and UX Design

- Follow DESIGN.md.
- Prioritize hierarchy, clarity, keyboard navigation and contrast.
- Use consistent spacing, typography, radius and motion tokens.
- Respect reduced motion and scaling.
- Do not copy proprietary assets or exact branded interfaces; achieve 1:1 layout,
  material, and interaction parity with `macOS 27 Golden Gate` using original RustyOS emblems/artwork.
- Check GPU cost of blur, shadows and transparency; respect `reduce_transparency` fallback.
- Enforce `Golden Gate Light & Dark` tokens (`DESIGN.md` §5.1, §12): `#F5F6F8` / `#1E1E20`,
  accent `#007AFF` / `#0A84FF`, Sky Blue folders in BOTH Light and Dark. Zero legacy purple,
  lavender, or plum in surfaces, selections, borders, or folder palettes.
- Before designing or editing UI: open the matching Light and Dark PNGs from
  `docs/design/references/macos-27-golden-gate/INDEX.md` (`read_file`).
- Liquid Glass 1:1 parity rules:
  - Light glass must look clear and luminous (`tint` alpha restrained so blurred folds read through)
    with directional corner caustics/rim highlights, not milky-white or muddy grey.
  - All primary glass overlays (`Dock`, `Popover`, `ControlModule`, `Spotlight` search pill +
    separate results card, `App Switcher`, `LockScreen` password pill) must use GPU
    `LiquidGlassElement` with CPU `UiShapeCache::glass_panel_element` fallback.
  - Transparent `TopBar` with no opaque backing box under right status icons or placeholder block
    behind the logo; non-blocking windows like `Settings` must not dim `TopBar`/`Dock`.
  - Floating macOS 27 window style target: inset rounded glass sidebar (traffic lights inside)
    and separated glass toolbar pills without debug controls in production status bars.
- After UI changes, capture a live `--winit` screenshot in `scratch/` and inspect the PNG with
  `read_file` across Light and Dark modes (without mutating persistent `~/.config/rustyos/config.toml`).
