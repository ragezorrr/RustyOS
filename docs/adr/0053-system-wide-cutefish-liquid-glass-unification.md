# ADR-0053: System-Wide CutefishOS Liquid Glass Unification (`popupGlass` / `fishuiGlass`)

## Status

accepted

## Context

После переноса точного суперэллиптического шейдера CutefishOS для Дока (`RUSTYOS-099`) в композиторе сохранялась легаси-ветка `else` для всех остальных стеклянных поверхностей (`GlassPanelStyle::Popover` и `GlassPanelStyle::ControlModule`):
- верхние меню/поповеры, Control Center (Quick Settings), Spotlight, уведомления, тултипы/стеки Дока, OSD, App Switcher и системные модальные окна (`Settings`, `Polkit`, `Portal`, `Magnifier`, `LockScreen`);
- клиентские поверхности по Wayland-протоколу `rustyos_liquid_glass_v1` (`Popover`, `Sidebar`, `ControlPill`, `Custom` в `frame_composer.rs`, включая `rusty-files`).

Эта ветка по-прежнему содержала диагональные угловые вспышки (`arc_tl`/`arc_br`), белёсую вертикальную дымку (`sheen`), `light_fresnel = 0.04` и экспоненциальное линзирование `lens_band` вместо единого пайплайна `glass_fragment_shader.glsl` + `rounded_clip.glsl`. В CPU-fallback (`sdf.rs`) также оставались вертикальный градиент `t_v` и `diagonal_glass_rim`, а в клиентском рендере `apps/rusty-files` (`sidebar.rs`, `popup.rs`) — яркий верхний `sheen`.

## Decision

1. **Единый GPU-шейдер `LIQUID_GLASS_FRAG` (`crates/rusty-compositor/src/surfaces/liquid_glass.rs`):**
   - Удалено ветвление `if (dock_mode) ... else ...` и старые легаси-формулы (`lens_band`, `sheen`, `light_fresnel`, `corner_flares`, `spec_dir`).
   - Все стили (`Dock`, `Popover`, `ControlModule`) проходят через единый конвейер CutefishOS: кубическая суперэллипса (`cornerExtent = r * 1.5`, 4-точечный `roundedCoverage`), 14px суперэллиптическая рефракция (`refractedAt` с поддержкой хроматического расщепления `u_sat_refract.w = chroma`) и 3-слойный направленный кант `uRim` (`lit = 1.0 / 0.35 / 0.0`, внешний 1px `shade`, внутренний `hairline` + `glow`).
   - В `compute_glass_material` заданы параметры CutefishOS:
     - `Dock` (`DockWindow::glass`): `half_a`, `saturation = 1.30`, `displacement = 36.0`, `rim = 1.0`, `chroma = 0.0`.
     - `Popover` (`popupGlass`): `half_out`, `saturation = 1.20`, `displacement = 20.0`, `rim = 0.85`, `chroma = 0.03`, `Light` `[1.0, 1.0, 1.0, 0.32 + 0.28 * g]`, `Dark` `[0.17, 0.17, 0.19, 0.34 + 0.26 * g]`.
     - `ControlModule` (`fishuiGlass` / `Sidebar` / `ControlPill`): `half_out`, `saturation = 1.22`, `displacement = 24.0`, `rim = 0.90`, `chroma = 0.03`, `Light` `[1.0, 1.0, 1.0, 0.10 + 0.14 * g]`, `Dark` `[0.14, 0.14, 0.16, 0.14 + 0.16 * g]`.
2. **Унификация CPU-fallback (`crates/rusty-compositor/src/surfaces/sdf.rs`, `tests.rs`):**
   - Убран вертикальный градиент `t_v`, `sheen_limit`, `light_fresnel` и `diagonal_glass_rim` для `Popover` и `ControlModule`; функция `cutefish_rim_alpha` стала общей для всех стилей.
3. **Клиентский рендер `rusty-files` (`apps/rusty-files/src/render/sidebar.rs`, `popup.rs`):**
   - В `draw_glass_pill` и `render_popup_chrome` применён направленный внутренний 1px `hairline` (верх 100%, низ 35% без белой боковой засветки) с внешним 1px контуром.

## Consequences

- Вся визуальная экосистема RustyOS (композиторные оверлеи, меню, Control Center, уведомления, Spotlight, модальные окна и клиентские окна по `rustyos_liquid_glass_v1`) использует единый быстрый GPU-шейдер без ветвлений `if/else` и единый стандарт материалов `CutefishOS`.
