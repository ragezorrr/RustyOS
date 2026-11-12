# ADR-0017: macOS 27 Frontend Visual Alignment, SDF Vector Chrome & Glass Surface Controls (`RUSTYOS-048`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

По результатам визуального аудита композитора в сравнении с референсами macOS 27 (`Golden Gate` / `DESIGN.md` §12) и анализа живого скриншота ранней сборки проявились следующие визуальные и UX-расхождения:
1. В `TopBar` символ `"✦"` выводился как `.notdef` прямоугольник из-за отсутствия глифа в системном шрифте; статус-иконки (`Wi-Fi`, `Volume`, `Battery`, `Control Center`) рисовались грубыми прямоугольниками; при `!has_battery` оставалась пустая дыра справа; индикаторы Spaces `2..9` несли тяжёлые серые подложки, а левое меню (`File`/`Edit`/`View`/`Window`/`RustyOS`/App) было частично статичным.
2. Dock имел слишком малый радиус скругления, не выделял запущенные приложения вертикальным разделителем от закреплённых иконок, не анимировал hover-подъём и показывал текстовые фиолетовые квадраты (`>_`, `Fs`, `Ov`) при отсутствии XDG-темы иконок.
3. Fallback-обои Purple Glass 240×135 страдали от цветового бандинга; отсутствовали настраиваемые пользователем `[ui] glass_opacity` / `reduce_transparency` и соответствующие контролы в Control Center; радиус SSD-окон составлял `12` вместо `20`, имелся 1px выступ нижней рамки SSD и расширение Floating `frame_rect` за пределы committed-геометрии.
4. Отдельный слойный клиент `apps/rusty-shell/src/wayland.rs` рисовал угловатые плитки Dock и тяжёлые квадратные подложки Spaces.

## Рассмотренные варианты

- **Вариант A (Выбран):**
  - Ввести чистый CPU SDF-рендеринг с 2×2 суперсэмплингом (`UiGlyphKind` + `UiShapeCache::ui_glyph_element`) в `surfaces.rs` для логотипа `RustyLogo`, статус-иконок (`Wifi`, `Volume`, `Battery`, `ControlCenter`) и лупы `Search`, без добавления новых крейтов и `unsafe`.
  - Поднять разрешение fallback-обоев в `surfaces.rs` до 640×360 с плавными aurora/silk-градиентами и 4×4 Bayer дитерингом против бандинга.
  - В `icons.rs` расширить XDG-поиск PNG (`AdwaitaLegacy`, `Qogir`, `Qogir-Dark`, `breeze-dark`, `Papirus-Dark`, категории `places`/`mimetypes`/`legacy`) и добавить кэшируемые процедурные цветные squircle-иконки `BuiltinAppIcon` (`Terminal`, `Files`, `Overview`, `WindowFallback`).
  - Добавить `[ui] glass_opacity` (`0.35..=1.0`, по умолчанию `0.78`) и `reduce_transparency` в `config.rs`, прокинуть их в `RustyCompositorState`, `shell_render.rs` (`scale_glass_rgba`) и карточку Glass Surface + интерактивные ползунки громкости и прозрачности в Quick Settings / Control Center (`telemetry::SystemMetricsHandle::set_volume_percent`).
  - В `shell_render.rs`, `services/lib.rs`, `telemetry.rs`, `state.rs`, `input.rs` и `winit_backend.rs`: добавить `clock_date_hhmm` (`local_clock_short_datetime`), убрать дыру отсутствующей батареи (`compute_top_bar_right_layout`), расширить ширину активного приложения и пересчёт меню (`compute_top_bar_left_layout`), сделать кликабельные выпадающие меню `TopBar` (`TopBarMenuAction`), обновить Dock (`DOCK_RADIUS = 36`, `DOCK_DIVIDER_GAP = 12`, `dock_hover_anim`), поднять `WINDOW_RADIUS = 20`, убрать 1px перехлёст нижней границы SSD и починить Floating `frame_rect` overshoot в `winit_backend.rs`.
  - Синхронизировать визуальный стиль `apps/rusty-shell/src/wayland.rs` (`fill_rounded_rect`, `fill_capsule_with_rim`, stadium-капсула Dock с разделителем и squircle-плитками).
- **Вариант B:** Косметические правки палитры и констант радиусов в `shell_render.rs` без SDF-глифов, процедурных squircle-иконок, дитеринга обоев, меню `TopBar` и `glass_opacity` (оставило бы `.notdef` и квадратные статус-заглушки).
- **Вариант C:** Добавление пользовательских GLSL/WGSL шейдеров размытия FBO (blur-behind pass) и новых крейтов векторной графики в одном этапе (заметно увеличивает сложность рендерера и выходит за текущий скоуп).

## Решение

Утверждён **Вариант A** (`APPROVE: RUSTYOS-048 Option A` от 2026-11-12).

## Последствия

- Весь chrome интерфейса (`TopBar`, `Dock`, `Control Center`, `Spotlight`, окна `SSD`) переведён на сглаженный macOS 27 Purple Glass вид при нулевых новых зависимостях и без `unsafe`.
- Настройки `[ui] glass_opacity` и `reduce_transparency` теперь конфигурируемы как статически через `config.toml`, так и в реальном времени через Control Center и меню `Edit`.
- Реальный backdrop Gaussian blur-behind окон в GLES-конвейере остаётся отдельной возможной задачей будущего этапа; текущая полупрозрачность использует чистую альфа-композицию (`scale_glass_rgba`). Живая визуальная проверка скриншота на дисплейной сессии (`cargo run -p rusty-compositor`) должна проводиться вне headless sandbox.
