# 0073. Popover Liquid Glass Lightening and Soft Contact-Free Shadow Calibration

- **Status:** accepted
- **Date:** 2026-11-20
- **Task:** RUSTYOS-129

## Context

Живое визуальное сравнение выпадающих меню и окон RustyOS (`RUSTYOS-128`) со скриншотом-ориентиром выявило три взаимосвязанных источника излишне жёстких контуров и «пластиковой» тяжести:
1. В `crates/rusty-ui/src/shadow.rs` профили `MenuOrTooltip` (`strength = 0.85`, `contact = 1.0`) и `Popup` (`strength = 0.70`, `contact = 1.0`), а также `ActiveWindow` / `InactiveWindow` (`contact = 1.0`) добавляли `1.5 px` контактную долю с минимальным смещением, формируя тёмный ореол по левому, правому и верхнему краям меню и избыточную обводку у окон;
2. В `crates/rusty-compositor/src/surfaces/liquid_glass.rs` (`compute_glass_material`) профиль `GlassPanelStyle::Popover` использовал `displacement = 20.0`, `rim = 0.85` и плотный светлый тинт `0.32 + 0.28 * g` (в CPU-фолбэке `surfaces/sdf.rs` `0.62`, `rim_peak = 0.36`), из-за чего шейдер `LIQUID_GLASS_FRAG` рисовал толстый 3D-бортик линзы и 1 px затемнённый периметр `shade`;
3. В `crates/rusty-compositor/src/shell_render/tokens.rs` светлый `popover_text_disabled` (`[165, 170, 182, 185]`) блёк на лёгком стекле, а разделитель `popover_separator` (`10%` чёрного / `12%` белого) выглядел грубее воздушного волосяного штриха.

## Decision

1. **Калибровка `ShadowElevation` (`crates/rusty-ui/src/shadow.rs`):**
   - Для `MenuOrTooltip` (`Self::new(14.0, 6.0, 8.0, 0.28, 0.0)`) и `Popup` (`Self::new(10.0, 4.0, 4.0, 0.24, 0.0)`) полностью отключена `1.5 px` контактная доля (`contact = 0.0`) и ослаблены боковой/верхний диффузные ореолы, сохраняя лишь мягкое смещённое вниз отделение карточки от фона;
   - Для `ActiveWindow` (`Self::new(24.0, 9.0, 14.0, 0.85, 0.28)`) и `InactiveWindow` (`Self::new(24.0, 9.0, 14.0, 0.48, 0.20)`) сохранены геометрия `bounds_and_offsets` (`sigma = 24, dy = 14`, `fpx = 42, fpt = 28`) и монотонность между активным и неактивным состояниями при кратном ослаблении боковой и верхней контактной обводки;
   - Модульный тест `rusty_ui::tests::shadow_elevation_and_rendering` расширен проверками отсутствия контактной доли у `MenuOrTooltip` / `Popup` и ослабленного контактного уровня окон.

2. **Облегчение материала `GlassPanelStyle::Popover` (`surfaces/liquid_glass.rs`, `surfaces/sdf.rs`):**
   - В `compute_glass_material`:
     - Светлый `Popover`: `tint_rgba = [0.95, 0.96, 0.98, 0.22 + 0.22 * g]`, `sat_refract_rim_sheen = [1.18, 6.0, 0.22, 0.01]` (мягкая прохладная матовость, минимальная `6 px` краевая линза и лёгкий `0.22` ободок без жёсткого тёмного `shade`-контура);
     - Тёмный `Popover`: `tint_rgba = [0.17, 0.18, 0.20, 0.28 + 0.24 * g]`, `sat_refract_rim_sheen = [1.18, 6.0, 0.22, 0.01]`.
   - CPU-фолбэк `surfaces/sdf.rs`: `base_glass_layer` откалиброван на `0.56` (`[243, 246, 250]`) Light / `0.68` (`[42, 44, 48]`) Dark и `rim_peak` `0.16` Light / `0.14` Dark;
   - Добавлен модульный тест `popover_material_keeps_subtle_rim_and_low_displacement`.

3. **Токены контраста меню и разделителей (`shell_render/tokens.rs`):**
   - `popover_text_disabled`: `[72, 78, 90, 155]` (Light) и `[154, 158, 168, 145]` (Dark) для чёткой читаемости неактивных пунктов поверх полупрозрачного размытого стекла;
   - `popover_separator`: `premul(0.0, 0.0, 0.0, 0.055)` (Light) и `premul(1.0, 1.0, 1.0, 0.07)` (Dark).

4. **Документация:**
   - Обновлён `DESIGN.md` (разделы меню/поповеров, теней окон и профиля `GlassPanelStyle::Popover`).

## Consequences

- Выпадающие меню и поповеры избавлены от резкой тёмной рамки и пластикового 3D-кантика, естественно растворяясь в мягком матовом стекле при сохранении чёткого контраста отключённых пунктов (`Hide Desktop`, `Quit`) и тонких разделителей.
- Окна сохраняют чёткую иерархию глубины (`ActiveWindow` > `InactiveWindow`) и `WINDOW_RADIUS = 14` без грубого тёмного канта по верхнему и боковым периметрам.
- Изменения носят локальный характер параметров материалов и токенов, не затрагивают Direct Scanout и публичные IPC-протоколы.
