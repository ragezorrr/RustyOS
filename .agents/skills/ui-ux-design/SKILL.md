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
  - Use the unified superellipse lens + directional 3-layer `uRim` shader (`LIQUID_GLASS_FRAG`:
    top `1.0`, bottom `0.35`, vertical sides `0.0`, zero diagonal corner flares or milky `sheen`)
    so Light and Dark glass stay clear and balanced against underlying wallpaper folds.
  - All primary glass overlays (`Dock`, `Popover`, `ControlModule`, `Spotlight` search pill +
    separate results card, `App Switcher`, `LockScreen` password pill, `rustyos_liquid_glass_v1`
    client surfaces) use GPU `LiquidGlassElement` (`compute_glass_material`) with matching CPU
    `UiShapeCache::glass_panel_element` fallback (`cutefish_rim_alpha`).
  - Transparent `TopBar` with no opaque backing box under right status icons or placeholder block
    behind the logo; standalone `System Settings` (`apps/rusty-settings`) uses a flat cool-grey
    sidebar (`#ECEDF1` Light / `#262628` Dark) without Liquid Glass blur and never dims `TopBar`/`Dock`.
  - Floating window chrome uses rounded top/bottom corners (`WINDOW_RADIUS = 20`) and Light/Dark
    adaptive SSD frames (`ssd_frame_colors`), with clean toolbars and zero debug controls in status bars.
- After UI changes, capture a live `--winit` screenshot in `scratch/` and inspect the PNG with
  `read_file` across Light and Dark modes (without mutating persistent `~/.config/rustyos/config.toml`).
