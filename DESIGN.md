# RustyOS — Design System

Version: 2.0  
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
small = 6
medium = 10
large = 16
xlarge = 22
pill = 999
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

---

**RustyOS Design Philosophy**

Inspired by the clarity of macOS.  
Enhanced by layered glass surfaces.  
Defined by its own purple identity.

**One system. One visual language. One consistent experience.**
