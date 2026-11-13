# RustyOS — Design System

Version: 2.1 (macOS 27 alignment)  
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

**RustyOS Purple Glass**

The design combines:

- macOS — layout, window composition, navigation and interaction patterns.
- Liquid Glass — layered surfaces, translucency and visual depth.
- RustyOS — deep purple palette, violet accents and custom identity.

Do not directly copy Apple branding, logos, proprietary assets or exact interface implementations.

macOS references are visual inspiration, not implementation specifications.

## 2. Reference Library

Visual references are stored in:

`docs/design/references/`

Before implementing or redesigning a UI component:

1. Inspect the reference directory.
2. Identify images relevant to the current component.
3. Use those images as visual guidance.
4. Apply RustyOS design tokens and interaction rules.
5. Preserve consistency with existing RustyOS components.

Do not load the entire reference collection for every task.

Inspect only relevant screenshots.

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

When references conflict with RustyOS design tokens, RustyOS tokens take priority.

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

### 5.1 Colors

```toml
[colors]
background = "#0F0A19"
surface = "#161023"
surface_elevated = "#20182E"
surface_overlay = "#292037"

accent = "#A56EFF"
accent_hover = "#B98AFF"
accent_muted = "#7650B5"

text = "#F4F0FA"
muted = "#A69BB5"
subtle = "#766B85"

danger = "#E56B75"
success = "#83D6A0"
warning = "#E8C477"

border = "#FFFFFF12"
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

Source: user-provided references in `docs/design/references/` (`desktop`, `context-menu`,
`control-center-overview`, `finder-window-dark`, `window-management`) and public descriptions of
macOS 26/27 Liquid Glass. Pixel values are approximate, measured from those screenshots at ~2x,
and are targets in logical pixels. RustyOS keeps its purple palette (section 5.1); only geometry,
layout and material behavior follow macOS 27. No Apple assets or branding.

### 12.1 Menu bar (top panel)

- Completely transparent by default (`24 px` logical height, zero dark scrim stripe): crisp
  shadowless white text and icons sit directly on the wallpaper. A high-contrast solid fallback
  surface is enabled only by `reduce_transparency = true`.
- Typography: bundled **Inter** (`Inter-SemiBold` for bold active-app name, `Inter-Regular` for
  menus and clock) at unified `13 px` on a shared optical baseline.
- Left: pure monochrome white original RustyOS chamfered hexagon emblem (`18 px`, `#FCFAFF`), bold
  active-app name, and global menus (`File`, `Edit`, `View`, `Go`, `Window`, `Help`).
- Workspace strip (`1..=9` + `Float`/`Tile`), SNI tray icons, standalone `Volume` glyph, and
  keyboard badge (`EN`/`RU`) are hidden by default for 1:1 visual minimalism and enabled opt-in via
  `[ui]` in `config.toml` (`menubar_show_workspaces`, `menubar_show_tray`,
  `menubar_show_input_source`, `menubar_show_volume`); workspaces and window layout remain directly
  accessible via `Go`/`Window` menus, Overview, Control Center, and keyboard shortcuts.
- Right (`4x4` SSAA SDF monochrome glyphs, `10 px` icon gaps / `6 px` before Clock, macOS order):
  `Battery` (`26x20 px` solid monochrome white pill capsule at `100%` or translucent glass track
  with proportional fill `5..=97%`, separated right terminal nub, and sharp white lightning bolt
  surrounded by a transparent cutout halo whenever AC power is connected or charging including
  `99–100% Full`, red `<= 20%` on battery, omitted when absent) -> `Wi-Fi` (`20 px` 1:1 3-band
  fan — rounded pie-wedge anchor + 2 wide concentric arcs — reflecting live `0..=3` signal bars
  from NetworkManager D-Bus / `/proc/net/wireless` / `nmcli` with dimmed inactive upper arcs or
  muted slash when offline) -> `Search` (`18 px`, bright white) -> `Control Center` (`20 px`: top
  hollow outline capsule with left knob ring + bottom solid white capsule with right cutout circle,
  translucent pill highlight when open) -> neutral locale plain Clock (`"Wed Apr 1  9:41 AM"`, or
  24h `"Wed Apr 1  09:41"` with `clock_24h = true`).

### 12.2 Dock

- Concentric rounded floating glass container (`DOCK_HEIGHT = 68 px`, `DOCK_RADIUS = 22 px`,
  `DOCK_MARGIN_BOTTOM = 8 px`), width dynamically fits `6` pinned apps + running windows + `2`
  utility slots; light top-lit frosted fill (`sample_dock_glass_pixel`) with 4×4 Bayer dithering,
  luminous 1 px top specular rim tapering softly along sides/bottom without a heavy outer shadow
  halo, and an opaque high-contrast plum-slate fallback when `reduce_transparency = true`.
- Resting slots have zero grey box outlines or dark backing smudges: crisp `50 px` `4x4` SSAA
  superellipse (exponent `4.8`) app icons sit cleanly on the translucent glass tray over `6 px` gaps
  (`52 px` hit slots).
- Default macOS cadence of original RustyOS artwork (`Files`, `Overview`, `Terminal`, `Browser`,
  `Notes`, `Settings`) + running window slots (preferring real XDG PNG / SHM toplevel icons over
  `WindowFallback`), followed by a centered `1 px` hairline vertical divider and free-standing
  `Downloads` (stacked blue folder) and `Trash` (tapered frosted silver wastebasket) utility icons.
- Running indicator: crisp `5x5` SDF circular dot (`#F8F6FF` active, amber `#E8C477` minimized,
  muted translucent for inactive windows) anchored `7 px` above the Dock bottom edge.
- Hover: quantized icon magnification (`50..=58 px`) and smooth upward lift (`+6 px`, `120 ms`
  interruptible animation; subtle `2 px` static lift when `reduced_motion = true`), accompanied by
  a floating frosted glass tooltip pill naming the hovered application above the Dock.

### 12.3 Windows, menus, Spotlight, Notifications, and OSD

- Window chrome (`SSD`): `WINDOW_RADIUS = 20` rounded corners at both top (`TITLEBAR_HEIGHT`
  titlebar) and bottom (1 px transparent-interior corner rim curves over client content), top
  1 px specular rim, and multi-layer soft ambient + contact shadows (`generate_window_shadow_elements`,
  stronger when focused, flatter when unfocused).
- Titlebar traffic lights: `12 px` gel-lit buttons (`TrafficLightKind::{Close, Minimize, Maximize}`)
  with subtle specular highlight and border, revealing `4x4` SSAA SDF hover glyphs (`×`, `−`, `+`)
  across the button cluster when hovered (`hovered_traffic_light_window`).
- Menu bar dropdowns (`MENU_POPOVER_RADIUS = 16`): `Popover` top-lit liquid glass (`pop_w = 256`),
  leading `4x4` SSAA monochrome `UiGlyphKind` icons, hairline section dividers, right-aligned real
  compositor shortcuts (`Super+Tab`, `Alt+S`, `Super+L`, `Super+Space`, `Alt+L`, `Alt+R`, `Alt+J`,
  `Alt+M`, `Alt+H`, `Alt+W`), and a rounded accent pill highlight on the hovered row
  (`top_bar_menu_hover_row`), anchored under a highlighted `TopBar` button when active.
- Spotlight (`ShellOverlayView::Spotlight`): `Popover` modal with a `48 px` `ControlModule` search
  field, full-width rounded accent pill highlight for the selected match, and a right-aligned
  `"Open ↵"` action hint.
- Notification HUD & OSD: `340x68` `Popover` (`radius = 16`) toast cards lead with a `38x38`
  `BuiltinAppIcon` squircle (`Terminal`, `Files`, `Browser`, `Notes`, `Settings`, `RustySystem`)
  and reveal a circular `20x20` close (`×`) button on hover (`hovered_notification_idx`); centered
  OSD pills (`Popover`, `radius = 18`) pair a `22x22` `Sun` / `Volume` / `Info` SDF glyph with a
  rounded `h = 8, r = 4` capsule level bar.

### 12.4 Control center and quick settings

- Floating Liquid Glass islands anchored top-right (`316x456` bounding box at `TOP_BAR_HEIGHT + 10`
  under the highlighted `TopBar` Control Center pill) with no outer dark container box; each
  island uses individual `GlassPanelStyle::ControlModule` glass and drop shadow (`RUSTYOS-053`).
- Upper grid pairs three full-pill connectivity/status capsules on the left (`Wi-Fi`, `Bluetooth`,
  `Power` with inverted white active circles and live SSID/power state) beside a top-right square
  `Now Playing` card backed by `MPRIS2` / `playerctl` (`Not Playing` or live track title with
  working `Prev`/`PlayPause`/`Next` controls), two captionless `66x66` circular quick toggles
  (`Tile`/`Float` Layout and `Overview`), two captionless circular toggles (`Reduced Motion` and
  `Clear Glass`/`Opaque`), and a right `Focus` (`Do Not Disturb`) capsule suppressing visible toast
  banners when active.
- Lower section stacks two full-width `58px`-tall `ControlModule` sliders (`Display` backed by
  `BrightnessService` when available with `glass_opacity` `35%..=100%` fallback, and `Sound`
  `0%..=100%` with an integrated right-side `Mute` circular toggle, thin `6px` tracks without
  percentage readouts), followed by a bottom centered `118x30` `Lock Screen` floating glass pill.

### 12.5 Material and depth

- Multi-pass GLES FBO blur-behind remains deferred to a dedicated future hardware ADR; until then,
  `UiShapeCache::glass_panel_element` (`GlassPanelStyle::{Dock, Popover, ControlModule}`) emulates
  top-lit liquid glass with `4x4` Bayer dithering, directional alpha tinting, and crisp inner
  specular rims.
- User controls for `glass_opacity` (`0.35..=1.0`, default `0.78`) and `reduce_transparency`
  (`true` switches shell panels to opaque high-contrast surfaces) are configurable via `[ui]` in
  `config.toml` and live-adjustable in Control Center.

### 12.6 Mission Control / Overview

- Top centered `Desktop 1..9` Spaces miniature strip (`overview_space_rects`) displays live
  non-minimized normalized `preview_rects` inside each workspace thumbnail, highlights the active
  workspace with accent fill/border, and switches directly to the clicked workspace
  (`OverviewHit::SwitchWorkspace`).
- A light `0.36` radial-style scrim preserves wallpaper depth while floating `Popover` preview cards
  (and focused outline ring) arrange below the Spaces strip above the visible Dock.

### 12.7 Lock Screen

- Locking switches the topmost compositing layer (`generate_lock_screen_elements`, rendered in front
  of all windows, `TopBar`, and `Dock` across `--winit` and `--drm`) to the full-screen desktop
  wallpaper without a modal card or opaque dark veil (`RUSTYOS-054`).
- Top-center header displays neutral C-locale date (`local_clock_lock_date`, e.g. `"Fri Nov 13"`)
  over large frosted semi-translucent clock numerals (`local_clock_lock_hhmm`, respecting
  `clock_24h`, without AM/PM suffix).
- Top-right minimal status cluster renders on the clean wallpaper: interactive keyboard layout label
  + `UiGlyphKind::Keyboard` (`U.S.`/`RU`), `UiGlyphKind::Battery`, 3-band `UiGlyphKind::Wifi`, and
  `UiGlyphKind::PowerSymbol` opening a compact `Popover` menu (`Sleep`, `Restart...`,
  `Shut Down...`) backed by `SessionService`.
- Bottom centered user list (`UserService` reading `/etc/passwd` UID `1000..=60000` and `~/.face` /
  `AccountsService` avatars or `4x4` SSAA silhouette badges) allows selecting an account
  (`Up`/`Down`/`Tab`/click) and unlocking immediately via single-user click, clicking the active
  selected user, `Enter`, `Space`, or `Escape`.

---

**RustyOS Design Philosophy**

Inspired by the clarity of macOS.  
Enhanced by layered glass surfaces.  
Defined by its own purple identity.

**One system. One visual language. One consistent experience.**
