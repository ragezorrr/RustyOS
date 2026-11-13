# ADR-0020: 1:1 macOS Shell Overlays, Modular Control Center, Overview Spaces & Window Chrome (`RUSTYOS-051`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

После приведения Menu Bar (`RUSTYOS-049`) и Dock (`RUSTYOS-050`) к визуальному стандарту macOS 27 ("Golden Gate") оставшиеся оверлеи шелла и оконный хром в `crates/rusty-compositor/` сохраняли упрощённый прототипный вид:
1. Выпадающие меню Menu Bar не имели скругления `16 px`, ведущих монохромных иконок, разделителей смысловых групп, правых шорткатов и hover-выделения строки; кнопка Control Center в `TopBar` не подсвечивалась пилюлей при открытии.
2. Control Center (`QuickSettings`) рендерил текстовую телеметрическую шапку и компактные кнопки `Mute`/`-`/`+` вместо модульных стеклянных карт, крупных круглых тогглов и полноширинных капсульных слайдеров с плавным перетаскиванием.
3. Оконный хром `SSD` ограничивался 1-слойной тенью, плоскими светофорными кругами без hover-глифов (`×`, `−`, `+`) и квадратными нижними углами клиентской рамки.
4. Spotlight выделял строку узкой полоской `3 px` сбоку вместо сплошной акцентной пилюли; Notification HUD использовал полосу срочности слева вместо squircle-иконок; OSD не имел ведущего SDF-глифа и капсульного трека; Workspace Overview затемнял фон тяжёлым тёмным скраймом без верхней ленты миниатюр Spaces (`Desktop 1..9`).

## Рассмотренные варианты

- **Вариант 1 (Выбран):**
  - Привести все оверлеи и SSD-хром в `crates/rusty-compositor/src/` (`surfaces.rs`, `icons.rs`, `shell_render.rs`, `state.rs`, `input.rs`, `winit_backend.rs`) к 1:1 визуальному и интерактивному соответствию macOS 27 в существующем 2D SDF / GLES memory-texture пайплайне без изменений GLES FBO blur и без новых внешних крейтов/`unsafe`.
  - Обобщить `UiShapeCache::glass_panel_element` (`GlassPanelStyle::{Dock, Popover, ControlModule}`), добавить `UiShapeCache::traffic_light_element` (`TrafficLightKind::{Close, Minimize, Maximize}`) и 17 новых `4×4` SSAA SDF-глифов `UiGlyphKind`; добавить `BuiltinAppIcon::RustySystem` в `icons.rs`.
  - Обновить `shell_render.rs` (меню с разделителями/иконками/шорткатами/hover-строкой, модульный Control Center `324×346` с `QuickSettingsLayout` и слайдерами `Display`/`Sound`, `generate_window_ssd_composite_elements_with_hover` с прозрачной дугой нижних скруглений `WINDOW_RADIUS = 20` и многослойной тенью, Spotlight, Notification HUD `340×68`, OSD с `UiGlyphKind` + капсулой, Overview с лентой `Desktop 1..9` и лёгким скраймом) и связать live hover/drag/Spaces-клики в `state.rs`, `input.rs` и `winit_backend.rs`.
- **Вариант 2:** Реализовать полноценный многопроходный GLES Kawase blur-behind в отдельной render target FBO перед выравниванием оверлеев: требует отдельного GPU/FBO бюджета и выходит за рамки текущей UX-задачи.
- **Вариант 3:** Ограничиться косметической правкой отступов/цветов в текущих оверлеях без hover-глифов, слайдеров и ленты Spaces в Overview: не закрывает ключевые расхождения с референсом macOS 27.

## Решение

Утверждён **Вариант 1** (`APPROVE: RUSTYOS-051 Option 1` от 2026-11-13).

## Последствия

- Menu Bar, Control Center, Spotlight, Notification HUD, OSD, Workspace Overview Spaces и SSD-окна приведены к единому визуальному языку macOS 27 при сохранении палитры Purple Glass, оригинальной графики `RustyOS`, `glass_opacity` (`0.35..=1.0`) и режима `reduce_transparency`.
- Ввод мыши в `input.rs` и рендер в `winit_backend.rs` поддерживают hover-подсветку строк меню, hover-крестик уведомлений, hover-глифы светофора окон, непрерывный drag слайдеров `Display` / `Sound` и переключение рабочих столов `1..=9` кликом по миниатюре Spaces в Overview.
- Настоящий многопроходный GPU blur-behind остаётся отдельной будущей архитектурной задачей.
