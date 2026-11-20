# 0072. Architecture Consolidation, Unified Cubic Squircle Clipping, 3-Lobe Shadow Shader, Window Lifecycle Animations, and Frosted Liquid Glass Sidebars

- **Status:** accepted
- **Date:** 2026-11-20
- **Task:** RUSTYOS-128

## Context

После приведения меню и теней (`RUSTYOS-125`–`RUSTYOS-127`) аудит архитектуры и визуальной согласованности RustyOS выявил остаточные структурные расхождения между композитором, `rusty-ui`, системными приложениями и документацией:
1. В репозитории сохранялись дублирующие компоненты: устаревший внешний shell-процесс `apps/rusty-shell` (при канонической однопроцессной оболочке в композиторе по `ADR-0027`) и дублирующая модаль `crates/rusty-compositor/src/shell_render/modals/settings.rs` (`ShellOverlayView::Settings`) при наличии самостоятельного приложения `apps/rusty-settings`;
2. `crates/rusty-ui` и приложения использовали расходящиеся константы радиусов (`WINDOW_RADIUS = 20` vs `14`), устаревшие фиолетовые токены акцентов (`#AF52DE`, `GOLDEN_GATE_ACCENTS`, `DarkPurpleGlass`) и упрощённое смешивание в `UiCanvas::blend_pixel`;
3. Клиентские окна без собственной обрезки или сторонние поверхности могли сохранять прямые углы, а геометрический контур `LIQUID_GLASS_FRAG` (L2-окружность) и `surfaces/sdf.rs::sample_corner_pixel` не совпадал с единым кубическим суперэллипсом `WINDOW_SHADOW_FRAG`;
4. Шейдер теней не использовал явные 3-дольные uniform-ы (`u_lobe1..3`), а в `shell_render` оставались вызовы `push_soft_shadow` и `Some((spread, alpha))`, равно как инвертированная полярность светлого `TopBar` при `reduce_transparency = true`;
5. В навигационных панелях `rusty-files` и `rusty-settings` отсутствовал реальный композиторный Liquid Glass (с корректной послойной подготовкой фона строго позади каждого окна), а `rusty-files`, `rusty-settings` и `rusty-installer` не приглушали индикаторы светофоров при потере фокуса (`State::Activated`);
6. Окна открывались и закрывались скачкообразно без плавных анимаций масштабирования и затухания, а часть `.agents/skills/*` и проектной документации содержала устаревшие метрики.

## Decision

1. **Консолидация границ shell и удаление дублей:**
   - Удалён `apps/rusty-shell`, опция `RUSTYOS_EXTERNAL_SHELL` из сессионных скриптов (`bin/rustyos-session`, `scripts/install.sh`, `packaging/arch/PKGBUILD`) и модаль `crates/rusty-compositor/src/shell_render/modals/settings.rs` (`ShellOverlayView::Settings`); переходы к настройкам направляются напрямую в `rusty-settings`.
   - `crates/rusty-ipc` сохранён для версионируемого IPC между композитором и системными клиентами; `ROADMAP.md` (Фаза 6) и `ARCHITECTURE.md` синхронизированы с однопроцессной моделью (`ADR-0027`).

2. **Единый источник правды `crates/rusty-ui`, 3-дольный GPU-шейдер теней и полярность `TopBar`:**
   - `WINDOW_RADIUS` унифицирован в `14 px` во всех крейтах и приложениях; палитры переведены на нейтральные `Light` / `Dark` и 7-цветный `ACCENT_PALETTE` (`#5E5CE6` Indigo на индексе 3 вместо `#AF52DE`), `FilesThemeMode::{LightGoldenGate, DarkPurpleGlass}` переименованы в `{Light, Dark}` (с обратной совместимостью чтения конфига).
   - `UiCanvas::blend_pixel` / `fill_rect` переведены на корректное premultiplied Porter-Duff Over смешивание; добавлен `draw_elevation_shadow_with`.
   - `WINDOW_SHADOW_FRAG` в `crates/rusty-compositor/src/surfaces/window_shadow.rs` рассчитывает 3 доли Гаусса (`u_lobe1..3`: wide + narrow + `1.5 px` contact) в едином проходе с непрерывным сглаживанием; полностью ликвидированы `push_soft_shadow` и `Some((spread, alpha))` в `shell_render/`.
   - Для `reduce_transparency = true` в светлой теме `TopBar` использует светлую основу `[242, 243, 247, 248]` и тёмный текст/иконки `[28, 29, 34, 255]`.

3. **Единый суперэллипс, композиторная обрезка углов и сохранность Direct Scanout:**
   - В `LIQUID_GLASS_FRAG`, `WINDOW_SHADOW_FRAG`, `SURFACE_CLIP_FRAG` (`crates/rusty-compositor/src/surfaces/window_clip.rs`) и `surfaces/sdf.rs::sample_corner_pixel` унифицирован единый непрерывный кубический суперэллипс (`rExt = clamp(r * 1.42, 0.0, minHalf)` для углов при сохранении L2 для капсул).
   - Неполноэкранные клиентские `WaylandSurfaceRenderElement` оборачиваются в `ClippedSurfaceElement` (`WINDOW_RADIUS = 14`, обратная 2x2 матрица проекции `frame.projection()`, обрезка `SolidColor`-поверхностей через 1x1 носитель, усечение углов в `opaque_regions`, `UnderlyingStorage::None`), тогда как полноэкранные окна (`is_fullscreen`) остаются нетронутыми для Direct Scanout.

4. **Реальный послойный Liquid Glass сайдбаров, фокусные светофоры и open/close анимации:**
   - В `frame_composer.rs::build_backdrop_render_elements` и `surfaces/liquid_glass.rs` окна обрабатываются снизу вверх с послойным расчётом Dual-Kawase размытия по ключу `(output_id, win_id)` (`prepare_liquid_glass_backdrop_slot`), чтобы каждый стеклянный сайдбар (`rusty-files`, `rusty-settings`) честно размывал обои и нижележащие окна без самозамыливания; клиентский `WlRegion` ограничен `(0, 0, SIDEBAR_WIDTH + WINDOW_RADIUS, height)` с матовым фолбэком при `reduce_transparency`.
   - В `rusty-files`, `rusty-settings` и `rusty-installer` добавлено отслеживание `xdg_toplevel::State::Activated` и приглушение неактивных светофоров через `TrafficLightKind::inactive_color()`.
   - В `state/wm_windows.rs`, `surfaces/window_minimize.rs` и `frame_composer.rs` добавлены snapshot-анимации открытия (`180 ms` `EaseOutQuart`, `0.93 -> 1.0` + fade) и закрытия (`160 ms` `EaseOutCubic`, `1.0 -> 0.93` + fade) с синхронным масштабированием и затуханием 3-дольной тени и мгновенным пропуском при `reduced_motion`.

5. **Синхронизация документации и навыков:**
   - Актуализированы `DESIGN.md`, `ROADMAP.md`, `ARCHITECTURE.md`, `FILE_INVENTORY.md`, `README.md` и `.agents/skills/{ui-ux-design,gpu-rendering,desktop-shell,window-management,wayland-compositor,animation-engine}/SKILL.md`.

## Consequences

- Устранены архитектурные дубли (`apps/rusty-shell`, `modals/settings.rs`) и визуальный хардкод теней/цветов во всех крейтах.
- Все неполноэкранные окна, поповеры и стеклянные сайдбары разделяют единый суперэллиптический контур (`14 px`), честное послойное размытие Dual-Kawase и единые реакции фокуса/жизненного цикла без регрессии для Direct Scanout.
- Все 48 препроцессорных вариантов 6 GLSL-шейдеров валидируются `glslangValidator`, пройдены строгий Clippy, тесты workspace, `--drm-probe` и живые Wayland (Light) и X11 (Dark) прогоны `--winit`.
