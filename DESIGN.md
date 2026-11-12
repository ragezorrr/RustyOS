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

- No solid background by default: text and icons sit on the wallpaper. A soft top gradient/shadow
  keeps contrast. A solid/tinted fallback is selected by `reduce transparency` or config.
- Height ~24-26 logical px (current 32 is too tall); text 13 px, medium weight.
- Left: active-app menu (name bold) followed by menus; workspace indicators are compact pills.
- Right (monochrome glyphs, 16 px, ~12 px gaps): tray items, network, battery, search,
  control-center toggle (pill highlight when open), keyboard layout, date and time.
- Replace the text status string (`Vol .. • .. • time`) with glyphs and a plain clock.

### 12.2 Dock

- Wide floating capsule (radius = pill-like, >= 20), detached ~8-12 px from the bottom edge,
  width grows with item count; light frosted fill, 1 px top-lit rim, soft outer shadow.
- Icons 48-56 px, consistent squircle shape, ~6-8 px gaps; real app icons first, monogram only
  as fallback.
- Running indicator: 4 px dot below the icon. Separator before trash/recent items.
- Hover: icon magnification or lift with short (120 ms) interruptible motion; reduced motion
  disables magnification.

### 12.3 Windows and menus

- Window radius `large` (20) with wide soft shadow (blur ~40-60, low alpha); active window has a
  stronger shadow, inactive a flatter one.
- Titlebar traffic lights 12-13 px, ~8 px spacing, left aligned; toolbar items grouped in
  rounded capsules.
- Menus/popovers: radius `large`, 4-6 px inner padding, item height ~28 px, monochrome leading
  icons, secondary-colored shortcuts right-aligned, hairline separators, hover fill uses accent.
- Sidebars are lighter/translucent than content area; selected row uses accent fill with
  `medium` radius.

### 12.4 Control center and quick settings

- Tiles use `large` radius glass cards; primary toggles are large round buttons (accent when on).
- Sliders: full-width capsule track, leading icon, accent fill, no tick marks.
- Panel anchors under the menu-bar control-center toggle.

### 12.5 Material and depth

- True lensing/blur is optional and needs a separate ADR (GLES shader cost, hardware matrix).
  Until approved, emulate glass with alpha, top-lit gradient and inner rim; always provide an
  opaque fallback and respect the transparency setting.
- Planned user setting `glass_opacity` (Liquid Glass slider equivalent) is not yet approved or
  implemented; see TASKS.md when scheduled.

### 12.6 Mission Control / Overview

- Workspace strip at the top with captions, windows tiled in a grid below, Dock stays visible.

---

**RustyOS Design Philosophy**

Inspired by the clarity of macOS.  
Enhanced by layered glass surfaces.  
Defined by its own purple identity.

**One system. One visual language. One consistent experience.**
