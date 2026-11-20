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

- Use restrained background transparency backed by the unified superellipse lens + directional `uRim` Liquid Glass pipeline (`section 12.5`).
- Apply blur and edge refraction only where it improves visual hierarchy.
- Maintain readable foreground contrast in both Light and Dark themes.
- Avoid excessive blur, diagonal corner glare, and stacked transparency.
- Provide opaque fallbacks when transparency is unavailable or `reduce_transparency = true`.
- Consider GPU and compositor performance (damage-gated Dual-Kawase FBO caching).

Desktop application windows (`apps/rusty-settings`, `apps/rusty-files`, `apps/rusty-terminal`, and future CSD utilities) combine frosted **Liquid Glass** navigation sidebars (`rustyos_liquid_glass_v1` `GlassStyle::Sidebar`, `WlRegion` scoped strictly to the sidebar width + corner overlap, falling back to `#ECEDF1` Light / `#262628` Dark when `reduce_transparency = true`) with clean opaque content canvases (`#F5F6FA` Light / `#1C1C1D` Dark) and borderless cards/pills (`#FFFFFF` / `#2C2C2D`), alongside Liquid Glass shell overlays (`Dock`, `Control Center`, `Spotlight`, `Popovers`, `Notifications`, `OSD`).

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
sidebar_bg = "#ECEDF1"
sidebar_active = "#D9DADD"
sidebar_divider = "#DADCE2"
page_bg = "#F5F6FA"
card_bg = "#FFFFFF"
pill_bg = "#F2F3F7"
window_border = "#00000018"

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
sidebar_bg = "#262628"
sidebar_active = "#3E3E41"
sidebar_divider = "#36363A"
page_bg = "#1C1C1D"
card_bg = "#2C2C2D"
pill_bg = "#2C2C2E"
window_border = "#FFFFFF16"

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
traffic_close = "#EE5A6A"
traffic_minimize = "#F4B63F"
traffic_maximize = "#3CC68A"
```

### 5.2 Radius

```toml
[radius]
small = 8        # controls, list rows
sidebar_row = 10 # active sidebar item pill, filter capsules
medium = 12      # borderless group cards (CARD_CORNER_RADIUS = 12), buttons
window = 14      # CSD & SSD windows, compositor ClippedSurfaceElement & WindowShadowPipeline (WINDOW_CORNER_RADIUS / WINDOW_RADIUS = 14)
popover = 14     # context menus, dropdown popovers (MENU_RADIUS = 14, concentric HOVER_PILL_RADIUS = 9 at 5px inset; TopBar pill = 8)
large = 20       # Control Center tiles, notifications
xlarge = 26      # Spotlight, large sheets
pill = 999       # Dock capsule, Control Center sliders, search capsules, toggles
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

### Windows (Reference Window Style)

Every system application window (`rusty-settings`, `rusty-files`, `rusty-terminal`, `rusty-installer`, and new CSD utilities) follows one unified contract (`crates/rusty-ui`, `ADR-0055`, `ADR-0057`, `ADR-0067`):

- **Corner Radius & Hairline Contour (`14 px`):** `WINDOW_CORNER_RADIUS = 14` (`rusty_ui::WINDOW_RADIUS`) on CSD clients and `WINDOW_RADIUS = 14` in compositor `SSD`, `ClippedSurfaceElement`, `LiquidGlassPipeline`, and `WindowShadowPipeline` (unified continuous cubic squircle contour). Draw the `1 px` outer border (`[0, 0, 0, 24]` Light / `[255, 255, 255, 22]` Dark) **before** applying the anti-aliased corner mask (`apply_window_corner_mask` / `clip_rounded_corners`) so rounded corners never darken unevenly.
- **Traffic Lights (`14 px`, step `22 px`):** `Rect(14, 13, 14, 14)`, `Rect(36, 13, 14, 14)`, `Rect(58, 13, 14, 14)` (centers `(21.0, 20.0)`, `(43.0, 20.0)`, `(65.0, 20.0)`, `r = 7.0`) in `#EE5A6A` / `#F4B63F` / `#3CC68A`, revealing `×`, `−`, `+` (`[0, 0, 0, 150]`) on group hover and dimming to `TrafficLightKind::inactive_color()` when unfocused (`!ACTIVATED`).
- **Sidebar & Navigation:** translucent frosted Liquid Glass sidebar via `rustyos_liquid_glass_v1` (`GlassStyle::Sidebar`, `WlRegion` restricted to `(0, 0, SIDEBAR_WIDTH + WINDOW_CORNER_RADIUS, height)`, opaque fallback `#ECEDF1` Light / `#262628` Dark when `reduce_transparency = true`), separated by a `1 px` right divider (`#DADCE2` / `#36363A`), neutral active item pill (`r = 10`, `#D9DADD` / `#3E3E41`), and borderless `Search` capsule (`r = 14`, accent focus ring only when active).
- **Page Canvas, Borderless Cards & Capsules:** `#F5F6FA` (Light) / `#1C1C1D` (Dark) page canvas, borderless group cards (`CARD_CORNER_RADIUS = 12`, `#FFFFFF` / `#2C2C2D`, zero card border strokes) with `1 px` internal row dividers, and borderless toolbar/filter/action capsules (`#F2F3F7` / `#2C2C2E`).
- **Scrolling & Live Theme Sync:** content and sidebar clip strictly below the header (`y >= HEADER_HEIGHT`) with proportional `4 px` (`r = 2`) scrollbars and live `config.toml` mtime + `v1|RELOAD_CONFIG` theme synchronization across compositor and clients.

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

### Menus and Popovers (Reference Context & Dropdown Menu Style)

All compositor dropdowns, desktop context menus, and client application `xdg_popup` surfaces (`top_bar/menus.rs`, `shell_render/context_menu.rs`, `apps/rusty-files/src/render/popup.rs`, `ADR-0058`, `ADR-0069`–`0071`) share this 1:1 contract:

- **Outer Geometry & Liquid Glass:** `MENU_RADIUS / MENU_POPOVER_RADIUS = 14`, `MENU_POPOVER_PAD_Y = 5`, `ROW_HEIGHT / MENU_ITEM_HEIGHT = 26`, `SEPARATOR_HEIGHT / MENU_SEPARATOR_HEIGHT = 12`, calibrated lightweight `GlassPanelStyle::Popover` (`displacement = 6.0`, `rim = 0.22`, subtle cool translucent veil without heavy bezel lip), and soft contact-free analytical shadow (`ShadowElevation::MenuOrTooltip` / `ShadowElevation::Popup`, `contact = 0.0`) (`RUSTYOS-129`, `ADR-0073`).
- **Client `xdg_popup` Compositor Contract:** client applications attach `rustyos_liquid_glass_v1` with `GlassStyle::Popover`, `corner_radius = 14`, and a `wl_region` matching the exact logical menu card `spec.layout.rect` (excluding any CPU shadow padding). When compositor glass is active (`has_compositor_glass == true`), the client SHM buffer stays transparent (`0x00000000` background, zero opaque `popover_bg` fill and zero duplicate CPU shadow), while `frame_composer.rs` (`push_client_popup_elements`) renders the popup **above** the main window and `PrepareBackdrop` pass (never inside `backdrop_list`) over `push_liquid_glass_panel_elevation(..., GlassPanelStyle::Popover, 14, ShadowElevation::MenuOrTooltip)`.
- **Concentric Hover Pill:** `HOVER_PILL_RADIUS = 9` (`14 - 5 = 9`), `HOVER_PILL_INSET_H = 5`, `HOVER_PILL_INSET_V = 1` (`24 px` height inside `26 px` row), filled with calibrated `pal.accent_rgba` (`alpha = 230` Light / `210` Dark for `#007AFF` / `#0A84FF`, or `#FF3B30` for destructive rows) and pure `#FFFFFF` text, icon, and shortcut on hover (`enabled: false` items stay crisply legible in `popover_text_disabled`: `[72, 78, 90, 155]` Light / `[154, 158, 168, 145]` Dark).
- **Two-Mode Left Alignment & Section Consistency:**
  - Shell menus (`top_bar/menus.rs`, `shell_render/context_menu.rs`): `x = 14 px` directly for iconless rows; `14×14 px` `UiGlyphKind` icon at `(mx + 14, row_y + 6)` + `13 px` label at `mx + 35` when an icon is present, aligned uniformly within each section.
  - Client popups (`apps/rusty-files/src/render/popup.rs`): plain action rows (`!any_non_swatch_checked && swatch_rgb.is_none()`) start at `x + 14`; tag swatch rows (`swatch_rgb.is_some()`) center their `r = 4.8` circle at `cx = x + 19.0` (left edge `x + 14.2`, flush with `x + 14` text) with label at `x + 32` and active checkmark at `right - 18`; non-swatch checked menus (`SortMenu`) place the checkmark at `x + 14` and label at `x + 28`.
- **Right Shortcut & Indicator Column:** `12 px` right-aligned macOS symbols (`⌘`, `⌥`, `⌃`, `⇧`, `⌫`, `▲`, `⇥`, `⏎`) at `right - 14`, or `12×12 px` `ChevronRight` / `Check` at `(mx + mw - 24, row_y + 7)`.
- **Dividers:** `12 px` separator slot height with a `1 px` subtle hairline centered horizontally (`popover_separator`: `5.5%` black Light / `7%` white Dark) and `12 px` horizontal inset (`x + 12 .. x + w - 12`).
- **Behavior:** Clear hierarchy, keyboard accessibility, and proper positioning and dismissal behavior.

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
  `Updates` badge with `#0A84FF` corner dot (`RUSTYOS-076`, shown only when updates are pending or
  its popover is open; also accessible via `System -> Check for Updates...`) -> `Battery` ->
  `Wi-Fi` -> `Search` -> `Control Center` (two horizontal toggle capsules) -> plain Clock
  (`"Wed Apr 1  9:41 AM"` or 24h with `clock_24h = true`).

### 12.2 Dock & Wallpaper

- Built-in Golden Gate procedural wallpaper targets layered 3D S-curve silk folds with crisp luminous
  crest highlights (matching `27-Golden-Gate-Desktop.png` Light/Dark contrast) so Liquid Glass edge
  refraction reads clearly behind Dock and popovers.
- App-centric concentric rounded floating glass container (dynamic corner radius `compute_dock_radius(thickness)` = `(thickness * 28 + 43) / 86`, clamped `>= 12 px`; `GlassPanelStyle::Dock` sampling crisp `half_a` backdrop with `14 px` superellipse edge refraction and 3-layer directional `uRim` — top `1.0`, bottom `0.35`, vertical sides `0.0`; opaque fallback on `reduce_transparency = true`), supporting `Bottom`, `Left`, and `Right` screen edges (`usable_work_area_with_dock`) and `Auto-hide` edge reveal. Slots follow three sections separated by `1 px` hairline dividers:
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

- Window chrome (`SSD` & unified `CSD`): `WINDOW_RADIUS = 14` continuous cubic squircle corners at both top and bottom (`ClippedSurfaceElement` clipping all non-fullscreen client surfaces in the compositor while keeping fullscreen clients unwrapped for Direct Scanout), theme-adaptive Light / Dark titlebars (`ssd_frame_colors`), top 1 px specular rim, centered fade+scale open/close animations (`180 ms` `EaseOutQuart` open / `160 ms` `EaseOutCubic` close, `0.93 <-> 1.0`), and a single-pass seamless analytical 3-lobe GLES Gaussian drop shadow (`WindowShadowPipeline` using wide, narrow, and soft attenuated `1.5 px` contact lobes — `contact = 0.28` `ActiveWindow` / `0.20` `InactiveWindow`, eliminating dark top/side perimeter outlines while preserving downward depth — with unified `squircleDistance` and 4x4 ordered dithering; CPU 9-slice & `UiCanvas::draw_elevation_shadow_with` fallbacks) (`generate_window_shadow_elements`, `RUSTYOS-075`, `RUSTYOS-126`, `RUSTYOS-128`, `RUSTYOS-129`).
  System apps (`apps/rusty-settings`, `apps/rusty-files`, `apps/rusty-terminal`, `apps/rusty-installer`) share unified `crates/rusty-ui` tokens: `WINDOW_CORNER_RADIUS = 14`, `1 px` outer border clipped by the corner mask (`[0, 0, 0, 24]` Light / `[255, 255, 255, 22]` Dark), and unified `14 px` traffic lights (`#EE5A6A` / `#F4B63F` / `#3CC68A`, centers `(21, 20)`, `(43, 20)`, `(65, 20)`, dimmed via `inactive_color()` when unfocused):
  - `rusty-settings` (`882×600`, `min_size = 640×500`): frosted Liquid Glass sidebar via `rustyos_liquid_glass_v1` (opaque `#ECEDF1`/`#262628` when `reduce_transparency = true`, `r = 10` active pill `#D9DADD`/`#3E3E41`), borderless cards (`#FFFFFF` / `#2C2C2D`, `r = 12`) on `#F5F6FA` / `#1C1C1D`, `Light`/`Dark`/`Auto` preview tiles, and vertical scrolling without dimming `TopBar` or `Dock`.
- Titlebar traffic lights (`SSD`): `14 px` buttons (`TrafficLightKind::{Close, Minimize, Maximize}`)
  revealing `×`, `−`, `+` hover glyphs (`hovered_traffic_light_window`).
- System apps & visual parity:
  - `rusty-files`: Sky Blue folder icons in BOTH Light and Dark appearances (never purple/lavender),
    full-bleed frosted Liquid Glass sidebar via `rustyos_liquid_glass_v1` (`GlassStyle::Sidebar`, `WlRegion` sidebar width + radius, opaque `#ECEDF1`/`#262628` fallback when `reduce_transparency = true`, `r = 10` active row) containing the unified `14 px` traffic lights and a subtle `1 px` right divider, borderless toolbar/tab capsules (`< | >`, view switcher, sort, actions, search), and clean status bar without debug theme-toggle buttons.
  - `rusty-settings` (*Software Update*, `RUSTYOS-123`, `ADR-0067`): встроенная страница Software Update (`--page updates`) с карточками текущего статуса, полосой прогресса с лейблами фаз, списком пакетов и кнопками обновления через Polkit.
  - `rusty-settings` (*Wi-Fi* и *Network*, `RUSTYOS-108`, `ADR-0059`): разделение страниц `SettingsPage::Wifi` (карточка заголовка с тумблером, текущая активная сеть, `Known Networks` с отметкой активного SSID и замочком защищённых точек, `Other Networks` и `Refresh`) и `SettingsPage::Network` (карточки сервисов `Wi-Fi`, `Ethernet`, `Firewall` с подписью статуса подключения и секция `Other Services`) по референсам `27-Golden-Gate-Settings-Wi-Fi.png` и `27-Golden-Gate-Settings-Network.png`.
  - `rusty-settings` (*Notifications*, `RUSTYOS-109`, `ADR-0060`): иконка с красным фоном `#FF3B30`, верхняя карточка с основным переключателем `Notifications`, секция `Notification Center` (`Show previews`, `when display is sleeping`, `when screen is locked`, `when mirroring or sharing the display`), список `Application Notifications` (`Files`, `Terminal`, `Browser`, `Software Update`, `System Settings` с векторными скворклами) и политики `Menu Bar & Updates` с синхронизацией в TOML `[notifications]` по референсу `27-Golden-Gate-Settings-Notifications.png`.
- Menu bar dropdowns, desktop context menus, and client `xdg_popup` surfaces (`TopBarAppProfile::Desktop` default hiding `Go` on empty desktop, `Hide`/`Hide Others`/`Quit` lifecycle with `enabled` rows, `8 px` active `TopBar` pill, `MENU_POPOVER_RADIUS = 14`, `MENU_POPOVER_PAD_Y = 5`, `MENU_ITEM_HEIGHT = 26`, `MENU_SEPARATOR_HEIGHT = 12`, `§6`, `ADR-0043`, `ADR-0058`, `ADR-0069`–`0071`): clear `GlassPanelStyle::Popover` Liquid Glass (`push_client_popup_elements` / `push_liquid_glass_panel_elevation` over `PrepareBackdrop`, scoped via `wl_region = spec.layout.rect` with transparent client SHM buffers when compositor glass is active), two-mode left alignment (`x = 14 px` for iconless rows; `14×14 px` `UiGlyphKind` icon at `x = 14 px` + `13 px` label at `x = 35 px`, or `rusty-files` `14/28/32 px` with swatch dot at `cx = x + 19.0`), `12 px` hairline dividers (`12 px` inset), right-aligned `12 px` shortcut column (`⌘`/`⌥`/`⌃`/`⇧`/`⌫`/`▲`/`⇥`/`⏎`), submenu chevrons (`>`), and concentric accent pill highlight on hover (`HOVER_PILL_RADIUS = 9`, `HOVER_PILL_INSET_H = 5`, `HOVER_PILL_INSET_V = 1`, pure `#FFFFFF` text/icon/shortcut on accent pill).
- TopBar Ethernet/Wi-Fi, `WifiPopover` и изоляция статуса Wi-Fi в Control Center (`RUSTYOS-108`, `ADR-0059`):
  - `TopBar` показывает `UiGlyphKind::Ethernet` при подключённом проводном соединении без активного Wi-Fi (`wired_connected && !wifi_connected`) и индикатор сигнала Wi-Fi при активном Wi-Fi; клик по иконке (`TopBarHit::Wifi`) открывает `WifiPopover`.
  - `WifiPopover` (`MENU_POPOVER_RADIUS = 14`, ховер-пилюля `HOVER_PILL_RADIUS = 9` с отступом `5 px` поверх `GlassPanelStyle::Popover`): заголовок `Wi-Fi` с тумблером, строка `Ethernet (<iface>) Connected` при активном проводном интерфейсе, секции `Known Networks` / `Other Networks` и переход `Wi-Fi Settings...`.
  - Капсула `Wi-Fi` в Control Center отображает только состояние Wi-Fi (активный `SSID`, `On` или `Off`) и не подменяется проводным статусом; геометрия и радиусы островов Control Center и `Dock` не изменяются.
- Notification Center (`ShellOverlayView::NotificationCenter`, `RUSTYOS-109`, `ADR-0060`):
  - Клик по блоку часов в `TopBar` (`check_top_bar_clock_click_for_options`) открывает правую панель `Notification Center` независимо от иконки `Control Center` (`status_hit_x .. clock_x - 2`) с подсветкой-пилюлей позади часов; устаревший синий счётчик `N:k` в `TopBar` не отображается (как в `27-Golden-Gate-Notification-Center.png`).
  - Компоновка по референсу `27-Golden-Gate-Notification-Center.png`: ведущая карточка уведомления, заголовок `Notification Center` с кнопкой `ClearAll`, стопка персистентных карточек из `notification_history` (сохраняемых после истечения TTL баннера), карточка `Weather` (`r = 22`, небесно-голубой градиент и почасовая полоса с `UiGlyphKind::{Sun, CloudSun}`), интерактивный виджет `Reminders` (`UiGlyphKind::Checklist`, переключение задач) и панель с кнопками `Edit Widgets` / `ClosePanel`, ограниченная сверху над `Dock` без изменения `Dock` и `Control Center`.
  - Материал и типографика карточек уведомлений (`RUSTYOS-110`, `ADR-0061`): единый изолированный стиль `GlassPanelStyle::NotificationCard` (`r = 22`, `344×68`, спокойное матовое тонирование без цветовых просветов фона и широкой линзы `Popover`), концентричная `38×38` иконка на `x + 16` (в палитре Golden Gate `#007AFF` без legacy purple в `BuiltinAppIcon::RustySystem` / `WindowFallback`), жирный заголовок (`FontWeight::Bold`), перенос до 2 контрастных строк тела через `wrap_notification_lines` без метки `"now"`.
- Spotlight (`ShellOverlayView::Spotlight`): dual-capsule layout matching `27-Golden-Gate-Spotlight-Search-v2.png`
  — a floating rounded search pill (`ControlModule` / dark gloss) separated cleanly above the results
  and category-filter `Popover` card.
- App Switcher (`Alt/Super+Tab`): clear `LiquidGlassElement` bar with subtle rounded highlight on the
  selected app icon (no opaque square boxes per slot).
- Notification HUD & OSD: `NotificationCard` toasts (`344×68, r = 22`) sharing chrome and multi-line typography
  with `Notification Center`, `38x38` squircle app icons (без legacy purple) and theme-aware hover close (`×`) circle;
  centered OSD pills (`Popover`) pairing `22x22` glyphs with rounded `h = 8, r = 4` level bars.
- Icon & Glyph Rendering Quality (`RUSTYOS-111`, `ADR-0062`): все процедурные скворклы, UI-глифы (`Dock`, `TopBar`, меню, `Notification Center`), traffic lights и системные окна используют `16-tap R2` (`LinearAccum` в линейном свете) или `4-tap RGSS` (`aa_cov8`) без ортогональной лесенки, `2x` `HiDPI` буферы (`GLYPH_BUFFER_SCALE = 2`) и сепарабельное `Lanczos-3` сжатие для `.png`/`SNI`.

### 12.4 Control center and quick settings

- Floating Liquid Glass islands anchored top-right (`TOP_BAR_HEIGHT + 10`) with no outer dark container box;
  each island uses individual `GlassPanelStyle::ControlModule` glass and soft 3-lobe drop shadow (`ShadowElevation::Popup`, `RUSTYOS-053`, `RUSTYOS-104`, `RUSTYOS-106`, `RUSTYOS-126`).
- Matches `27-Golden-Gate-Finder-Control-Center.png` hierarchy on a unified `D = 70 px`, `gap = 12 px` grid (`316 × 516`, `col_w = 152`): three stacked connectivity/status capsules on the left (`Wi-Fi`, `Bluetooth`, `Power` at `152 × 70, r = 35` with concentric borderless `46 px` icon circles at `pad = 12`), square `Now Playing` card (`152 × 152, r = 28`, `44 × 44` `r = 12` artwork tile) backed by `MPRIS2` / `playerctl` on top-right, circular quick toggles (`Layout`, `Overview`, `Reduced Motion`, `Transparency` at `70 × 70, r = 35`, active state rendered as `6 px` inset circle preserving outer `uRim`), right `Focus` capsule (`152 × 70, r = 35`), rounded capsule-card `Display` and `Sound` sliders (`316 × 66, r = 26`, `5 px` bright white active track + `12 px` circular thumb knob + borderless `30 px` mute circle), legible `popover_text_secondary`/`muted` capsule subtitles, and centered bottom `Lock Screen` pill (`118 × 28, r = 14`).

### 12.5 Material and depth (Unified Liquid Glass)

- **Backdrop Blur Pyramid (`LiquidGlassPipeline`, `surfaces/liquid_glass.rs`):**
  - 5-pass GLES Dual-Kawase down/upsample ladder (`scene 1/1 -> half_a 1/2 -> quarter_a 1/4 -> eighth_a 1/8 -> quarter_b 1/4 -> half_out 1/2`), gated by `OutputDamageTracker` with `BLUR_DAMAGE_PAD_PHYS = 32 px` padding so idle frames perform zero extra GPU passes.
  - On theme switch (`Light` <-> `Dark`) or wallpaper update (`wallpaper_revision`), `UiShapeCache::sync_wallpaper_revision` calls `LiquidGlassPipeline::invalidate_all()` so offscreen backdrops rebuild immediately without stale tinting.
- **Single Unified Composite Shader (`LIQUID_GLASS_FRAG`):**
  - All GPU glass surfaces — compositor overlays and Wayland clients via `rustyos_liquid_glass_v1` — share one branch-free GLSL ES 1.00 fragment shader (`ADR-0052`, `ADR-0053`, `RUSTYOS-106`):
  - **Euclidean $L_2$ Circular-Arc SDF (`RUSTYOS-106`):** `sdRoundBox` (`rc = clamp(r, 0.0, min(halfSize.x, halfSize.y))`) with isotropic circular arcs and Euclidean normal `normalize(max(q * r, vec2(0.0001)))`, producing true circular toggles, $180^\circ$ capsule ends, and clean 4-point tangent-averaged anti-aliased edge coverage (`roundedCoverage`) aligned with CPU `sdf.rs`.
  - **14 px superellipse edge lens:** `lensHeight` over `band = 14.0 px` (`ior = 1.7`), 4-sample `refractedAt` sampling clamped to `u_uv_bounds` with optional chromatic channel shift (`u_sat_refract.w = chroma`), vibrance saturation boost (`u_sat_refract.x`), and target Light/Dark frosted tint mix (`u_tint_rgba`).
  - **3-layer directional `uRim` (`lit = abs(outward.y) * mix(1.0, 0.35, 0.5 + 0.5 * outward.y)`):** top edge weight `1.0`, bottom edge `0.35`, left/right vertical sides `0.0` (no side glare, no diagonal `d^7` corner flares, no vertical `sheen`):
    1. Outer `1 px` edge shade darkening (`shade`);
    2. Inner `1 px` sharp `hairline` (`1.0 - (1.0 - glass) * (1.0 - rim_x * lit * 0.43 * hairline)`);
    3. Exponential inner `glow` (`exp(-d / 1.7)`) modulated by luminance `headroom` to avoid clipping bright backgrounds.
- **Three Material Profiles (`compute_glass_material`):**
  - `GlassPanelStyle::Dock` (compositor Dock & client `BasePanel`): samples crisper `half_a`, `sat = 1.30`, `displacement = 36.0`, `rim = 1.0`, `chroma = 0.0`, Light `[0.60, 0.60, 0.60, 0.13 + 0.12 * g]`, Dark `[0.38, 0.38, 0.38, 0.15 + 0.14 * g]`.
  - `GlassPanelStyle::Popover` (menus, popovers, Spotlight, Notifications, App Switcher, Overview cards, `DockStack`, Dock tooltip, `Polkit`/`Portal`/`Magnifier` sheets, `LockScreen` card, client `Popover`/`Custom`): calibrated light frost with minimal lens/rim (`sat = 1.18`, `displacement = 6.0`, `rim = 0.22`, `chroma = 0.01`, Light `[0.95, 0.96, 0.98, 0.22 + 0.22 * g]`, Dark `[0.17, 0.18, 0.20, 0.28 + 0.24 * g]`, CPU fallback `0.56` Light / `0.68` Dark with subtle `rim_peak = 0.16` Light / `0.14` Dark) (`RUSTYOS-129`, `ADR-0073`).
  - `GlassPanelStyle::ControlModule` (Control Center islands, Lock Screen password pill, client `Sidebar`/`ControlPill`): samples smooth `half_out` (`RUSTYOS-104`), `sat = 1.30`, `displacement = 34.0`, `rim = 1.0`, `chroma = 0.01`, Light `[0.72, 0.74, 0.78, 0.11 + 0.11 * g]`, Dark `[0.24, 0.24, 0.27, 0.15 + 0.15 * g]`.
- **CPU Fallback & Client Surfaces:**
  - `UiShapeCache::glass_panel_element` (`surfaces/sdf.rs`, `sample_glass_panel_pixel_themed`) uses flat themed `base_glass_layer` + `4×4` Bayer dithering + matching directional top/bottom rim (`directional_rim_alpha`, no side glare or diagonal flares), automatically taking over on `reduce_transparency = true` (high-contrast opaque surfaces), Lock Screen, or GLES unavailability.
  - Client-rendered Chrome (`apps/rusty-files/src/render/{sidebar.rs, popup.rs}`: `draw_glass_pill`, `render_popup_chrome`) mirrors the directional `100%` top / `35%` bottom `1 px` inner hairline cleanly.
- **Runtime Configuration:**
  - `glass_opacity` (`0.35..=1.0`, `DEFAULT_GLASS_OPACITY = 0.78`) and `reduce_transparency` live under `[ui]` in `config.toml` and update compositor + Wayland apps (`rusty-settings`, `rusty-files`, `rusty-terminal`, `rusty-installer`) live via `RELOAD_CONFIG` / mtime sync.

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
