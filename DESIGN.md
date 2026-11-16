# RustyOS — Design System

Version: 3.0 (macOS 27 Golden Gate Light & Dark)  
Status: Active  
Scope: Entire desktop environment

## 1. Design Direction

RustyOS follows a modern desktop design philosophy inspired by macOS, contemporary Liquid Glass interfaces, and refined desktop UX patterns.

The goal is to create an independent Linux desktop environment with:

- Consistent visual language.
- Refined typography and clear hierarchy.
- Layered surfaces and subtle transparency.
- Smooth, responsive interactions.
- Consistent window and panel geometry.
- Distinct RustyOS visual identity.

### Visual Identity

**RustyOS Golden Gate (Light & Dark)**

The design combines:

- macOS 27 Golden Gate — layout, window composition, navigation and interaction patterns.
- Liquid Glass — layered frosted light (`#F5F6F8`) and neutral dark graphite (`#262629`) surfaces, translucency and visual depth.
- RustyOS — dual Light (`#007AFF` accent) and Dark (`#0A84FF` accent) appearances with Golden Gate warm champagne/sand and coastal slate-blue procedural wallpapers.

Do not directly copy Apple branding, logos, proprietary assets or exact interface implementations.

macOS references are visual inspiration, not implementation specifications.

## 2. Reference Library

Visual references are stored in:

`docs/design/references/`

Full-resolution `macOS 27 Golden Gate` reference packs downloaded locally are stored under `docs/design/references/macos-27-golden-gate/` (gitignored to avoid binary repository bloat) and cataloged in `INDEX.md` inside that directory.

Before implementing or redesigning a UI component:

1. Inspect the reference directory.
2. Identify images relevant to the current component.
3. Use those images as visual guidance.
4. Apply RustyOS Golden Gate tokens and interaction rules.
5. Preserve consistency across Light and Dark modes (zero legacy purple/lavender/plum tokens, including folder/selection icons).
6. Capture and visually inspect a live `--winit` screenshot against the chosen Light and Dark reference PNGs.

Do not load the entire reference collection for every task.

Inspect only the relevant Light and Dark screenshots listed in `docs/design/references/macos-27-golden-gate/INDEX.md`.

### Reference Priority

Use references to understand:

- Desktop composition.
- Window geometry and titlebars.
- Dock and application launchers.
- System panels and control center.
- Settings interfaces.
- Dialogs, popovers and context menus.
- Visual depth, transparency and shadows.
- Motion and interaction patterns.

`macOS 27 Golden Gate` (`INDEX.md`) is the 1:1 visual parity target for layout, Liquid Glass clarity, shadows, and color polarity (using original RustyOS emblems/assets). Do not preserve legacy tokens when they visibly diverge from Golden Gate references; propose token/layout alignment cleanly through `APPROVE:`.

## 3. UX Principles

- Predictability is more important than decoration.
- Primary actions must remain quickly accessible.
- Consistency across applications and system components is mandatory.
- Animations must be short, meaningful and interruptible.
- Support reduced-motion preferences.
- Keyboard navigation and visible focus indicators are mandatory.
- Maintain readability at different display scaling factors.
- Transparency and blur must never compromise contrast or performance.
- Interactive elements must provide clear visual feedback.
- Avoid unnecessary visual complexity.

## 4. Visual Language

### 4.1 Surfaces

Use a layered surface hierarchy:

1. Desktop background.
2. Main application surfaces.
3. Elevated windows and panels.
4. Floating popovers and menus.
5. Modal dialogs and critical overlays.

Each layer should have a clear visual distinction.

Use subtle borders, shadows and transparency to communicate depth.

### 4.2 Liquid Glass

Liquid Glass-inspired styling may be used for:

- Dock.
- Top panel.
- Control center.
- Floating system panels.
- Popovers.
- Selected overlays.

Rules:

- Use restrained background transparency.
- Apply blur only where it improves visual hierarchy.
- Maintain readable foreground contrast.
- Avoid excessive blur and stacked transparency.
- Provide opaque fallbacks when transparency is unavailable.
- Consider GPU and compositor performance.

Not every component needs glass effects.

### 4.3 Geometry

Use consistent corner radii across the desktop environment.

Windows, cards, panels, menus and controls should follow a coherent geometry system.

Avoid mixing unrelated shapes or arbitrary radii.

### 4.4 Typography

Typography must provide clear information hierarchy.

Priorities:

- Readability.
- Consistent font weights.
- Predictable sizing.
- Proper alignment.
- Support for display scaling.

Use the project's established font configuration unless a change is explicitly justified.

## 5. Design Tokens

Colors, spacing, radius, typography and motion must have a single source of truth.

The following values are initial design tokens and may evolve as the visual system matures.

### 5.1 Colors (`Golden Gate Light & Dark`, `RUSTYOS-067`)

```toml
[colors.light]
background = "#F5F6F8"
surface = "#FFFFFF"
surface_elevated = "#F5F6F8"
surface_overlay = "#EBECEF"

accent = "#007AFF"
accent_hover = "#1A88FF"
accent_muted = "#0062CC"

text = "#1D1F24"
muted = "#5A606E"
subtle = "#868D9C"

border = "#00000018"

[colors.dark]
background = "#141518"
surface = "#1E1E21"
surface_elevated = "#26262A"
surface_overlay = "#303036"

accent = "#0A84FF"
accent_hover = "#409CFF"
accent_muted = "#0062CC"

text = "#F2F2F7"
muted = "#AEAEB6"
subtle = "#84848C"

border = "#FFFFFF1F"

[colors.semantic]
danger = "#E56B75"
success = "#30D158"
warning = "#E8C477"
```

### 5.2 Radius

```toml
[radius]
small = 8      # controls, list rows
medium = 12     # cards, buttons
large = 20      # windows, menus, popovers, control-center tiles
xlarge = 26     # Spotlight, large sheets
pill = 999      # Dock capsule, toggles, menu-bar pills
```

### 5.3 Spacing

```toml
[spacing]
unit = 4
small = 8
medium = 12
large = 20
xlarge = 28
xxlarge = 36
```

### 5.4 Motion

```toml
[motion]
reduced_motion = false
short_ms = 120
standard_ms = 180
long_ms = 260
```

All tokens should be centralized and reused.

Do not introduce arbitrary colors, spacing values or radii when an existing token is suitable.

## 6. Component Design

All system components must follow the same visual language.

### Windows

- Consistent titlebar geometry.
- Clear active and inactive states.
- Predictable window controls.
- Subtle elevation.
- Consistent rounded corners.
- Smooth but responsive transitions.

### Dock

- Floating appearance.
- Clear application icons.
- Consistent spacing.
- Subtle hover and active feedback.
- Visible running application indicators.
- See section 12 for the macOS 27 target geometry.

### Panels

- Clear grouping of controls.
- Consistent internal spacing.
- Restrained transparency.
- Readable labels and icons.
- Predictable opening and closing behavior.

### Menus and Popovers

- Clear hierarchy.
- Consistent item heights.
- Subtle hover states.
- Keyboard accessibility.
- Proper positioning and dismissal behavior.

### Controls

Buttons, switches, sliders, inputs and checkboxes must share consistent:

- Geometry.
- Typography.
- Colors.
- Interaction states.
- Focus behavior.

## 7. Motion and Accessibility

Use monotonic time for animations.

Animations must:

- Be interruptible.
- Support cancellation.
- Avoid delaying critical actions.
- Avoid unnecessary continuous rendering.
- Respect reduced-motion preferences.

Verify:

- Color contrast.
- Focus visibility.
- Keyboard navigation.
- Display scaling.
- Screen-reader integration.
- Animation behavior.
- Performance under compositor load.

When reduced motion is enabled, replace nonessential transitions with immediate or minimal changes.

## 8. Performance Principles

RustyOS is a native desktop environment. Visual quality must not compromise responsiveness.

- Avoid unnecessary blur layers.
- Avoid excessive transparency.
- Minimize redundant redraws.
- Avoid expensive effects on frequently updated elements.
- Keep animations lightweight.
- Reuse rendered resources when possible.
- Avoid unnecessary allocations in rendering paths.

Performance is part of the design system, not a separate concern.

## 9. Implementation Rules

Before creating or modifying a component:

1. Inspect existing implementations.
2. Identify reusable components and tokens.
3. Check relevant visual references.
4. Follow established architecture.
5. Implement only the necessary changes.
6. Verify visual consistency and functionality.

Do not:

- Rewrite unrelated components.
- Introduce duplicate design tokens.
- Hardcode styles that belong in the design system.
- Copy proprietary Apple assets.
- Add dependencies without justification.
- Sacrifice accessibility for visual effects.

## 10. Design Consistency

Every RustyOS component must feel like part of one operating system.

Consistency must be maintained across:

- Applications.
- Window decorations.
- Desktop panels.
- Dock.
- System settings.
- Notifications.
- Context menus.
- Dialogs.
- System controls.

New components must extend the existing design language rather than establish independent visual styles.

## 11. Design Evolution

This document is the central visual contract for RustyOS.

When the design evolves:

- Update the relevant tokens.
- Preserve backward consistency.
- Update component specifications when necessary.
- Use the reference library to validate visual decisions.
- Avoid introducing competing design systems.

The design system should evolve incrementally, not through unrelated redesigns.

## 12. macOS 27 Alignment (Liquid Glass)

Source: local `macOS 27 Golden Gate` reference library in `docs/design/references/macos-27-golden-gate/`
(`INDEX.md`, including `27-Golden-Gate-Desktop.png`, `27-Golden-Gate-Finder-Control-Center.png`,
`27-Golden-Gate-About-This-Mac-Menu.png`, `27-Golden-Gate-Spotlight-Search-v2.png`,
`27-Golden-Gate-Finder-Home-Folder.png`, `27-Golden-Gate-Settings-Appearance.png`,
`27-Golden-Gate-Finder-App-Switcher.png`, `27-Golden-Gate-Mission-Control.png`,
`27-Golden-Gate-Login-Window.png`, `27-Golden-Gate-Notification-Center.png`, and `appearance/*`).
Pixel values are approximate targets in logical pixels. RustyOS follows neutral `Golden Gate Light & Dark`
palettes (section 5.1); geometry, Liquid Glass clarity, and interactions target 1:1 parity with
`macOS 27 Golden Gate` using original RustyOS emblems and artwork (no Apple branding).

### 12.1 Menu bar (top panel)

- Completely transparent by default (`24 px` logical height, zero dark scrim stripe and zero opaque
  backing pill behind the right status icons or square backing block behind the logo): crisp text
  and icons sit directly on the desktop/wallpaper with subtle shadow where needed for legibility.
  A high-contrast solid fallback surface is enabled only by `reduce_transparency = true`.
- Typography: bundled **Inter** (`Inter-SemiBold` for bold active-app name, `Inter-Regular` for
  menus and clock) at unified `13 px` on a shared optical baseline.
- Left: pure monochrome original RustyOS chamfered hexagon emblem (`18 px`), bold
  active-app name (`Files` default when no app window owns focus, switching dynamically via
  `TopBarAppProfile`), and app-aware menus (`File`, `Edit`, `View`, conditional `Go`, `Window`, `Help`).
- Workspace strip (`1..=9` + `Float`/`Tile`), SNI tray icons, standalone `Volume` glyph, and
  keyboard badge (`EN`/`RU`) are hidden by default for 1:1 visual minimalism and enabled opt-in via
  `[ui]` in `config.toml` (`menubar_show_workspaces`, `menubar_show_tray`,
  `menubar_show_input_source`, `menubar_show_volume`); workspaces and window layout remain directly
  accessible via `Go`/`Window` menus, Overview, Control Center, and keyboard shortcuts.
- Right (`4x4` SSAA SDF monochrome glyphs, `10 px` icon gaps / `6 px` before Clock, macOS order):
  `Battery` -> `Wi-Fi` -> `Search` -> `Control Center` (two horizontal toggle capsules) -> plain
  Clock (`"Wed Apr 1  9:41 AM"` or 24h with `clock_24h = true`).

### 12.2 Dock & Wallpaper

- Built-in Golden Gate procedural wallpaper targets layered 3D S-curve silk folds with crisp luminous
  crest highlights (matching `27-Golden-Gate-Desktop.png` Light/Dark contrast) so Liquid Glass edge
  refraction reads clearly behind Dock and popovers.
- App-centric concentric rounded floating glass container (`DOCK_RADIUS = 22 px`, clear top-lit
  `LiquidGlassElement` frosted fill, tapering specular rim, opaque fallback on `reduce_transparency = true`),
  supporting `Bottom`, `Left`, and `Right` screen edges (`usable_work_area_with_dock`) and `Auto-hide`
  edge reveal. Slots follow three sections separated by `1 px` hairline dividers:
  1. `Pinned` apps (`[dock].pinned_apps`, reorderable and pull-out removable via `>= 52 px` `"Remove"` pill);
  2. `Unpinned` running apps + up to `4` recent apps (`show_recent`);
  3. Minimized window mini-slots (when `minimize_into_app = false`) + utility slots (`Downloads` Stack + `Trash`).
- Resting slots have zero grey box outlines or dark backing smudges: crisp `4x4` SSAA superellipse app icons
  (configurable `dock_icon_size` `36..=68 px`) sit cleanly on the translucent glass tray.
- Running indicator (`dock_show_indicators`): neutral circular `5x5` dot (dark/charcoal in Light mode,
  luminous white in Dark mode; amber when an app has minimized windows). Clicking an app icon focuses,
  raises, restores, or cycles its windows without re-minimizing active windows; launching triggers upward
  `Launch bounce` when `dock_animate_opening = true`. Right-click opens an app-centric menu (open windows,
  `Keep in Dock` / `Remove from Dock`, `Open at Login`, `Show All Windows` -> filtered App Exposé,
  `Quit` / `Alt+Force Quit`).
- Hover & Minimize: single-icon local hover without wave rippling or capsule stretching (hovered icon
  scales `+8 px` centered in its slot and lifts `+6 px`, or `+2 px` static lift with `0 px` scaling
  when `reduced_motion = true`) with a floating frosted glass tooltip pill; window minimize uses safe
  GLES `WindowMinimizePipeline` (`Genie` or `Scale` deformation into the target Dock slot).
- `Downloads` utility supports `Display as` (`Folder` / `Stack`), interactive `Liquid Glass` popover in
  `Fan`, `Grid`, or `List` (`Automatic` adapts to Dock edge), `Sort By` (`Date Added`, `Date Modified`,
  `Name`, `Size`, `Kind`), and `"Open in Files"`.
- Runtime Dock configuration lives in `[dock]` of `config.toml` and `System Settings -> Desktop & Dock`
  (`SettingsTab::DesktopDock`).

### 12.3 Windows, Finder (`rusty-files`), menus, Spotlight, Notifications, and OSD

- Window chrome (`SSD`): `WINDOW_RADIUS = 20` rounded corners at both top (`TITLEBAR_HEIGHT`) and
  bottom, top 1 px specular rim, and a single-pass seamless analytical GLES SDF Gaussian drop shadow
  (`WindowShadowPipeline`: continuous contact + Y-shifted ambient glow with 4x4 ordered dithering
  replacing multi-offset strips and discrete `Solid` bands; CPU 9-slice fallback via continuous
  corner + stretched edge buffers in `UiShapeCache`) (`generate_window_shadow_elements`, `RUSTYOS-075`).
  Standard system windows (e.g., `System Settings`) should behave as movable desktop windows without a
  full-screen modal scrim that darkens `TopBar` and `Dock`.
- Titlebar traffic lights: `12 px` gel-lit buttons (`TrafficLightKind::{Close, Minimize, Maximize}`)
  revealing `×`, `−`, `+` hover glyphs (`hovered_traffic_light_window`).
- Finder / `rusty-files` 1:1 parity targets (`27-Golden-Gate-Finder-Home-Folder.png`, `Applications.png`,
  `Finder-Network.png`): Sky Blue folder icons in BOTH Light and Dark appearances (never purple/lavender),
  full-bleed edge-to-edge sidebar (`x=0, y=0..height`, matching the left window corners) containing the
  traffic lights and a subtle `1 px` right divider (distinct from the floating inset sidebar card used in
  `System Settings` / `Notes`), separated glass toolbar pills (`< | >`, view switcher, actions, search) above
  the tab bar, and clean status bar without debug theme-toggle buttons.
- Menu bar dropdowns (`MENU_POPOVER_RADIUS = 16–20`): clear `Popover` liquid glass with leading
  `UiGlyphKind` icons, hairline dividers, right-aligned shortcut column (`⌘`/`⌥`/`⌃`/`⇧` or compositor
  equivalents), submenu chevrons (`>`), and a rounded accent pill highlight on hover.
- Spotlight (`ShellOverlayView::Spotlight`): dual-capsule layout matching `27-Golden-Gate-Spotlight-Search-v2.png`
  — a floating rounded search pill (`ControlModule` / dark gloss) separated cleanly above the results
  and category-filter `Popover` card.
- App Switcher (`Alt/Super+Tab`): clear `LiquidGlassElement` bar with subtle rounded highlight on the
  selected app icon (no opaque square boxes per slot).
- Notification HUD & OSD: `Popover` toast cards (plus long-term right-edge `Notification Center` stack)
  with `38x38` squircle app icons and hover close (`×`) circle; centered OSD pills pairing `22x22`
  glyphs with rounded `h = 8, r = 4` level bars.

### 12.4 Control center and quick settings

- Floating Liquid Glass islands anchored top-right (`TOP_BAR_HEIGHT + 10`) with no outer dark container box;
  each island uses individual `GlassPanelStyle::ControlModule` glass and drop shadow (`RUSTYOS-053`).
- Matches `27-Golden-Gate-Finder-Control-Center.png` hierarchy: three stacked connectivity/status capsules
  on the left (`Wi-Fi`, `Bluetooth`, `Power` / `AirDrop` equivalent), square `Now Playing` card backed by
  `MPRIS2` / `playerctl` on top-right, circular quick toggles (`Layout`, `Overview`, `Reduced Motion`,
  `Transparency`), right `Focus` capsule, full-width `Display` and `Sound` slider capsules (bright
  active track over tinted glass), and bottom pill (`Lock Screen` / `Edit Controls` parity).

### 12.5 Material and depth

- Glass containers (`Dock`, `Popover`, `ControlModule`, `Switcher`, `LockScreen` input pill, `Dock` tooltip)
  render through the 5-pass GLES Dual-Kawase backdrop blur FBO pipeline (`LiquidGlassPipeline`:
  `1 -> 1/2 -> 1/4 -> 1/8 -> 1/4 -> 1/2`) using `OutputDamageTracker` on the backdrop scene so idle
  frames incur zero extra GPU passes, composited via `LIQUID_GLASS_FRAG` (`TextureShaderElement`) with
  continuous analytical rounded-box SDF normals, pronounced edge optical lensing/dispersion (`6.5..8.5 px`),
  vibrance boost, opposing diagonal (`top-left` / `bottom-right`) `d^7` corner-arc specular bevel without
  a 360° wireframe contour, cubic-decay ambient drop shadows, and restrained translucent Light/Dark
  tinting (`ADR-0034`, `RUSTYOS-074`).
- Automatic runtime CPU fallback (`UiShapeCache::glass_panel_element` with `4x4` Bayer dithering and
  matching diagonal corner-arc specular math in `sdf.rs`) activates cleanly whenever
  `reduce_transparency = true`, the session/screen is locked, no glass UI is visible, or GLES
  shader/FBO init fails.
- User controls for `glass_opacity` (`0.35..=1.0`, default `0.78`) and `reduce_transparency`
  (`true` switches shell panels to opaque high-contrast surfaces) are configurable via `[ui]` in
  `config.toml` and live-adjustable in Control Center.

### 12.6 Mission Control / Overview

- Top centered `Desktop 1..9` Spaces strip + arranged window cards below; long-term 1:1 parity target
  replaces static placeholder rectangles with live scaled desktop/window thumbnails (`27-Golden-Gate-Mission-Control.png`).

### 12.7 Lock Screen

- Locking renders the full-screen wallpaper in front of all windows, `TopBar`, and `Dock` without a modal
  card or dark veil (`RUSTYOS-054`).
- Top-center neutral C-locale date over large embossed/semi-translucent frosted clock numerals;
  minimal top-right status cluster (`U.S.`/`RU` + keyboard, `Battery`, `Wifi`, `PowerSymbol` popover);
  bottom centered user avatars and `LiquidGlassElement`-backed password input pill (`27-Golden-Gate-Login-Window.png`).

---

**RustyOS Design Philosophy**

Inspired by the clarity of macOS.  
Enhanced by layered glass surfaces.  
Unified across Golden Gate Light and Dark appearances.

**One system. One visual language. One consistent experience.**
