---
name: rustyos-design-system
description: >-
  Use this skill when designing, styling, or polishing UI components, apps, shell chrome,
  animations, themes, or Liquid Glass surfaces in RustyOS (`rusty-ui`, `rusty-animation`,
  `rusty-compositor`, and `apps/*`).
---

# RustyOS Design System & Visual Craft

## 1. Visual Identity ("Liquid Glass" + Crisp Precision)
RustyOS combines tactile optical depth (compositor-side frosted glass via `protocols/rustyos-liquid-glass-v1.xml`) with sharp typography, restrained color, and fluid physics. Maintain a **distinct, original RustyOS aesthetic** — never imitate or copy another OS's trademarked assets or terminology.

## 2. Design Tokens (Always Reuse, Never Hardcode)
- **Theme & Accents** (`crates/rusty-ui/src/theme.rs`):
  - Always resolve colors through `ThemePalette::new(is_dark, accent_idx)` and `accent_for(accent_idx, is_dark)`.
  - Support `ThemeMode::{Light, Dark, Auto}` everywhere; test both Light and Dark contrast (WCAG AA+ >= 4.5:1 for body text, >= 3:1 for UI controls/icons).
  - Use the 7 semantic RustyOS accents (`ACCENT_PALETTE` / `ACCENT_DARK_PALETTE`) and `TrafficLightKind` active/inactive/pressed/glyph tokens.
- **Optical Glass Presets** (`protocols/rustyos-liquid-glass-v1.xml`):
  - `popover` (16px radius, high vibrancy) — menus, popovers, HUD sheets
  - `sidebar` (12px radius, high diffusion) — navigation sidebars (`rusty-files`, `rusty-settings`)
  - `control_pill` (capsule) — segmented tabs, compact floating controls
  - `base_panel` — top bar and dock panels
  - Always commit glass region, corner radius, style, and optional tint atomically before `wl_surface.commit`.
- **Geometry & Radii** (`crates/rusty-ui/src/geometry.rs`, `draw.rs`):
  - Prefer continuous squircle corners (`squircle_sdf_dist`) over harsh circular arcs for windows, cards, and popovers.
  - Respect canonical constants: `WINDOW_RADIUS`, `TITLEBAR_HEIGHT`, `CSD_TITLEBAR_HEIGHT` (40px), `DIALOG_HEADER_HEIGHT`, `TRAFFIC_LIGHT_*`.
  - Follow a 4px micro / 8px macro spatial grid (`4, 8, 12, 16, 20, 24`).
- **Elevation Shadows** (`crates/rusty-ui/src/shadow.rs`):
  - Use `ShadowElevation::{ActiveWindow, InactiveWindow, MenuOrTooltip, Popup, Dock}` via `draw_elevation_shadow` instead of ad-hoc alpha rectangles.
- **Typography** (`assets/fonts/`, `crates/rusty-ui/src/text.rs`):
  - UI: `Inter` (`Regular`, `SemiBold`, `Bold`); Code/Terminal: `JetBrains Mono`.
  - Use `UiFontEngine` for glyph rasterization and word-boundary metrics.

## 3. Motion & Interaction Polish
- **Refresh-Rate Independent Motion** (`crates/rusty-animation/src/lib.rs`, `crates/rusty-ui/src/motion.rs`):
  - Use `MotionTokens`: `short` (120ms hover/focus/snap), `standard` (180ms window/panel), `long` (260ms workspace/overview).
  - Default easing: `Easing::EaseOutCubic` or `Easing::EaseOutQuart` (decelerating, responsive); `EaseInOutCubic` for spatial slides.
  - **Mandatory accessibility**: Every animation must honor `reduced_motion: bool` (snap to target immediately with zero frames wasted).
  - Zero heap allocations on the per-frame tick/render path.
- **Interactive States & Keyboard Accessibility**:
  - Every interactive control must visually distinguish: `default`, `hover`, `pressed`, `focused` (focus ring via `FocusScope`), `disabled`, and `inactive window`.
  - Support full keyboard navigation (`Tab`/`Shift+Tab`, arrow keys, `Enter`/`Space`, `Escape` to dismiss popovers/dialogs).
  - Prefer `UiDrawList` + `UiPainter` so widgets render identically on CPU (`UiCanvas`) and GPU (`GlesSurfaceRenderer`) and remain unit-testable headless.

## 4. Design Review Checklist Before Finishing UI Work
1. Both Light and Dark modes verified with `ThemePalette` (no hardcoded RGB).
2. `reduced_motion` path tested in unit tests.
3. Inactive window state and keyboard focus traversal (`FocusScope`) handled.
4. No overlapping text or clipped hit targets at minimum window size.
