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
  material, and interaction parity with modern desktop standards using original RustyOS emblems/artwork.
- Check GPU cost of blur, shadows and transparency; respect `reduce_transparency` fallback.
- Enforce `RustyOS Light & Dark` tokens (`DESIGN.md` §5.1, §12): `#F5F6F8` / `#1E1E20`,
  accent `#007AFF` / `#0A84FF`, Sky Blue folders in BOTH Light and Dark. Zero legacy purple,
  lavender, or plum in surfaces, selections, borders, or folder palettes.
- Do NOT use third-party branding or codenames in code, comments, or documentation.
- Liquid Glass parity rules:
  - Use the unified continuous cubic squircle lens + directional 3-layer `uRim` shader (`LIQUID_GLASS_FRAG`:
    top `1.0`, bottom `0.35`, vertical sides `0.0`, zero diagonal corner flares or milky `sheen`)
    so Light and Dark glass stay clear and balanced against underlying wallpaper folds.
  - All primary glass overlays (`Dock`, `Popover`, `ControlModule`, `Spotlight` search pill +
    separate results card, `App Switcher`, `LockScreen` password pill, `rustyos_liquid_glass_v1`
    client surfaces & window sidebars) use GPU `LiquidGlassElement` (`compute_glass_material`) with
    matching CPU `UiShapeCache::glass_panel_element` fallback (`directional_rim_alpha`).
  - Transparent `TopBar` with no opaque backing box under right status icons or placeholder block
    behind the logo (adapting cleanly to dark ink on light bar when `reduce_transparency = true` in Light mode).
  - **Shared UI Crate & Reference Window Style (`crates/rusty-ui`, `DESIGN.md` §6, `§12.3`, `ADR-0057`, `ADR-0067`, `ADR-0069`, `ADR-0070`, `ADR-0072`):**
    all system CSD/SSD windows (`rusty-settings`, `rusty-files`, `rusty-terminal`, `rusty-installer`,
    and compositor `ssd.rs`) use `crates/rusty-ui` as the single source of truth for window geometry
    (`WINDOW_RADIUS = 14`), 7-color neutral Light/Dark accents (`#007AFF`/`#0A84FF`, Indigo `#5E5CE6`
    at index 3, zero purple glass tokens), `TrafficLightButtons` (`TRAFFIC_LIGHT_DIAMETER = 14`,
    centers `21.0, 43.0, 65.0`, `#EE5A6A`/`#F4B63F`/`#3CC68A`, dimming to `inactive_color()` when
    unfocused / `!ACTIVATED`), outer `1 px` border (`[0,0,0,24]` Light / `[255,255,255,22]` Dark)
    drawn **before** the corner mask, `WindowHeader`, and `WindowFrame`.
    Software Update lives inside standalone `rusty-settings` (`--page updates`). `rusty-files`,
    `rusty-settings`, and `rusty-terminal` attach `rustyos_liquid_glass_v1` (`GlassStyle::Sidebar`,
    `r = 14`, `WlRegion` scoped to sidebar width + corner overlap) for frosted Liquid Glass sidebars
    (falling back to flat `#ECEDF1` Light / `#262628` Dark when `reduce_transparency = true`) with
    `r = 10` active pills; content canvases use `#F5F6FA` / `#1C1C1D` with borderless `r = 12`
    `#FFFFFF`/`#2C2C2D` cards and borderless toolbar/filter capsules, clean status bars, and never dim `TopBar`/`Dock`.
  - **Unified Drop Shadow & Menu Contract (`crates/rusty-ui/src/{menu,shadow}.rs`, `DESIGN.md` §6, `ADR-0058`, `ADR-0069`–`0071`):**
    all shadows use `ShadowElevation::{ActiveWindow, InactiveWindow, MenuOrTooltip, Popup, Dock}`
    3-lobe Gaussian math (`draw_elevation_shadow_with` on CPU `UiCanvas`; `WINDOW_SHADOW_FRAG` on GPU)
    without hardcoded offset tuples or legacy `push_soft_shadow`. All `TopBar` dropdowns (`Desktop`
    default profile, `Hide`/`Hide Others`/`Quit` lifecycle with `enabled` dim rows, clean `SystemLogo`,
    `8 px` open TopBar pill), desktop context menus, and client `xdg_popup`s share `MENU_RADIUS = 14`,
    `MENU_POPOVER_PAD_Y = 5`, `ROW_HEIGHT = 26`, `SEPARATOR_HEIGHT = 12`, concentric hover pill
    (`HOVER_PILL_RADIUS = 9`, `5 px` inset, pure `#FFFFFF` text/icon/shortcut on accent pill),
    two-mode left alignment (`14 px` plain / `14 px` icon + `35 px` label in shell; swatch/check
    alignment in `rusty-files`), and `12 px` right-aligned shortcut column. Client `xdg_popup`s
    attach `rustyos_liquid_glass_v1` (`wl_region = spec.layout.rect`) with transparent SHM buffers and
    render in `frame_composer.rs::push_client_popup_elements` above windows and `PrepareBackdrop`.
- After UI changes, capture a live `--winit` screenshot in `scratch/` and inspect the PNG with
  `read_file` across Light and Dark modes (without mutating persistent `~/.config/rustyos/config.toml`).
