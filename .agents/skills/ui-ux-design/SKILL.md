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
  material, and interaction parity with modern macOS-inspired desktop standards using original RustyOS emblems/artwork.
- Check GPU cost of blur, shadows and transparency; respect `reduce_transparency` fallback.
- Enforce `RustyOS Light & Dark` tokens (`DESIGN.md` §5.1, §12): `#F5F6F8` / `#1E1E20`,
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
    `UiShapeCache::glass_panel_element` fallback (`directional_rim_alpha`).
  - Transparent `TopBar` with no opaque backing box under right status icons or placeholder block
    behind the logo.
  - **Shared UI Crate & Reference Window Style (`crates/rusty-ui`, `DESIGN.md` §6, `§12.3`, `ADR-0057`, `ADR-0067`):**
    all system CSD/SSD windows (`rusty-settings`, `rusty-files`, `rusty-terminal`, `rusty-installer`,
    and compositor `ssd.rs`) use `crates/rusty-ui` as the single source of truth for window geometry,
    traffic light buttons (`TrafficLightButtons`, `TRAFFIC_LIGHT_DIAMETER = 14`, centers `21.0, 43.0, 65.0`,
    `#EE5A6A`/`#F4B63F`/`#3CC68A` with vector microglyphs `x`, `-`, `+`), outer `1 px` border
    (`[0,0,0,24]` Light / `[255,255,255,22]` Dark) drawn **before** the corner mask, and `WindowFrame`.
    Software Update is integrated directly into `rusty-settings` (`--page updates`). Sidebars
    use flat `#ECEDF1` Light / `#262628` Dark (zero backdrop blur) with `r = 10` `#D9DADD`/`#3E3E41`
    active pills; content canvases use `#F5F6FA` / `#1C1C1D` with borderless `r = 12` `#FFFFFF`/`#2C2C2D`
    cards and borderless toolbar/filter capsules, clean status bars, and never dim `TopBar`/`Dock`.
  - **Reference Context & Dropdown Menu Style (`DESIGN.md` §6, `§12.3`, `ADR-0058`):** all shell
    menus (`top_bar/menus.rs`, `shell_render/context_menu.rs`) and client `xdg_popup` menus
    (`apps/rusty-files/src/render/popup.rs`, future Wayland apps) share `MENU_POPOVER_RADIUS = 13`,
    `MENU_POPOVER_PAD_Y = 5`, `MENU_ITEM_HEIGHT = 24`, `MENU_SEPARATOR_HEIGHT = 9`, concentric hover
    pill (`HOVER_PILL_RADIUS = 7`, `HOVER_PILL_INSET_H = 5`, `HOVER_PILL_INSET_V = 1`, pure `#FFFFFF`
    hover text/icons/shortcuts), two-mode left alignment (`14 px` without icon, `14 px` icon + `35 px`
    label in shell; `x + 14` text / `cx = x + 19.0` + `x + 32` swatches / `x + 14` check + `x + 28` text
    in `rusty-files`), `12 px` right-aligned macOS shortcuts, and `GlassPanelStyle::Popover` (`(16, 52)`
    shadow). Client `xdg_popup`s must attach `rustyos_liquid_glass_v1` with `wl_region = spec.layout.rect`,
    keep transparent SHM backgrounds when compositor glass is active, and render in
    `frame_composer.rs::push_client_popup_elements` above windows and `PrepareBackdrop`.
- After UI changes, capture a live `--winit` screenshot in `scratch/` and inspect the PNG with
  `read_file` across Light and Dark modes (without mutating persistent `~/.config/rustyos/config.toml`).
