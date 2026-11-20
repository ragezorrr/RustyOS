# 0070. Unified Three-Lobe Gaussian Shadow System

- **Status:** accepted
- **Date:** 2026-11-20
- **Task:** RUSTYOS-126

## Context

В ходе аудита десктоп-окружения RustyOS было выявлено разрозненное определение параметров теней в нескольких компонентах:
1. В `crates/rusty-compositor/src/surfaces/window_shadow.rs` использовались локальные константы вертикального смещения и размытия (`drop_y: 5.0, contact_spread: 6.0, amb_spread: 16.0` для активного окна и `drop_y: 3.0, contact_spread: 4.0, amb_spread: 10.0` для неактивного). При этом внешние границы окна менялись при переключении фокуса, вызывая визуальное смещение сцены;
2. В `crates/rusty-compositor/src/shell_render/tokens.rs`, `chrome_shapes.rs`, `context_menu.rs`, `wifi_popover.rs`, `updates_popover.rs`, `dock_stack.rs` и `notification_center.rs` использовались жестко закодированные пары кортежей `Some((16, 52))`, `Some((18, 54))` и `Some((8, 28))`;
3. В `crates/rusty-ui/src/menu.rs` фон меню отрисовывался через ad-hoc цикл с жестко заданными смещениями и прозрачностями;
4. В `apps/rusty-files/src/render/popup.rs` и `dialogs.rs` использовались локальные массивы `shadow_layers = [...]` и одномерные смещения `Rect::new(rect.x - 2, rect.y + 2, rect.w + 4, rect.h + 5)`.

В эталонной десктопной архитектуре тени базируются на строгой аналитической трёхдольной модели Гаусса (`3-lobe Gaussian shadows`) с фиксированными уровнями возвышения (`ShadowElevation`).

## Decision

1. **Создание модуля `crates/rusty-ui::shadow`:**
   - **Трёхдольная аналитическая модель:**
     - `Lobe 1 (Wide Ambient Lobe)`: диффузный ореол радиуса `wide`, вертикальное смещение `offset`, прозрачность `0.22 * strength`;
     - `Lobe 2 (Narrow Directional Lobe)`: направленная полутень радиуса `narrow`, вертикальное смещение `(offset + 1) / 2`, прозрачность `0.15 * strength`;
     - `Lobe 3 (Contact Lobe)`: тонкая контактная граница радиуса `1.5 px`, вертикальное смещение `1.0 px`, прозрачность `0.16 * strength * contact`.
   - **Стандартизированные уровни возвышения `ShadowElevation`:**
     - `ActiveWindow`: `wide = 24.0, narrow = 9.0, offset = 14.0, strength = 1.00, contact = 1.0`;
     - `InactiveWindow`: `wide = 24.0, narrow = 9.0, offset = 14.0, strength = 0.55, contact = 1.0`;
     - `MenuOrTooltip`: `wide = 14.0, narrow = 6.0, offset = 8.0, strength = 0.85, contact = 1.0`;
     - `Popup`: `wide = 8.0, narrow = 4.0, offset = 3.0, strength = 0.70, contact = 1.0`;
     - `Dock`: `wide = 10.0, narrow = 4.0, offset = 3.0, strength = 0.25, contact = 0.0` (парящий эффект без контактной линии);
     - `None`: нулевые доли.
   - **Отрисовка на CPU-холсте `UiCanvas` (`draw_elevation_shadow`):**
     - Многопроходная аппроксимация кривых Гаусса $e^{-x^2 / 2\sigma^2}$ для каждого активного лепестка без ступенчатых артефактов.

2. **Сквозная интеграция в `crates/rusty-compositor`:**
   - В `surfaces/window_shadow.rs` обновлен `WindowShadowSpec`:
     - Реализован `for_elevation` на базе `rusty_ui::shadow::ShadowStyle`;
     - В `for_window_mode` применены эталонные параметры `wide = 24 px`, `offset = 14 px`. Границы окна (`fb.size == ub.size`) теперь строго неизменны при смене фокуса;
   - В `surfaces/mod.rs` добавлен метод `elevation_shadow_render_element`;
   - В `shell_render/tokens.rs` добавлены `push_elevation_shadow` и `push_liquid_glass_panel_elevation`;
   - В `chrome_shapes.rs`, `context_menu.rs`, `wifi_popover.rs`, `updates_popover.rs`, `dock_stack.rs`, `notification_center.rs` и `frame_composer.rs` все магические кортежи заменены на вызовы с соответствующим `ShadowElevation`.

3. **Сквозная интеграция в приложения (`apps/rusty-files`, `crates/rusty-ui`):**
   - В `crates/rusty-ui/src/menu.rs` метод `draw_menu_background` переведен на `draw_elevation_shadow(..., ShadowElevation::MenuOrTooltip)`;
   - В `apps/rusty-files/src/render/popup.rs` и `dialogs.rs` локальные слои заменены на расчёт долей через `rusty_ui::shadow::ShadowElevation`.

## Consequences

- Полностью устранен локальный хардкод параметров теней во всей кодовой базе десктоп-среды RustyOS.
- При переключении фокуса окон внешние границы сцены остаются стабильными (исчезло геометрическое подергивание).
- Док, контекстные меню, диалоги и окна получили идентичный, физически точный и сглаженный трёхдольный ореол Гаусса.
