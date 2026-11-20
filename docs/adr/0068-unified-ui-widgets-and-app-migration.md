# 0068. Unified UI Widgets and Cross-App UI Library Integration

- **Status:** accepted
- **Date:** 2026-11-20
- **Task:** RUSTYOS-124

## Context

После создания базового крейта `crates/rusty-ui` в рамках `RUSTYOS-123` возникла необходимость расширить его возможности и выполнить сквозную интеграцию во все графические приложения окружения RustyOS и Server-Side Decorations (SSD) композитора.

До текущего момента:
1. `apps/rusty-files`, `apps/rusty-terminal`, `apps/rusty-installer` и `crates/rusty-compositor` (SSD) сохраняли локальные реализации геометрии светофоров, расчёта координат кнопок закрытия/сворачивания/разворачивания, хит-тестинга и микроглифов (`×`, `−`, `+`).
2. Базовые интерактивные элементы управления (переключатели `ToggleSwitch`, кнопки `PushButton`, сгруппированные карточки настроек `GroupCard`, поисковые строки с иконкой лупы и кнопкой очистки `SearchField`) либо реализовывались локально в `rusty-settings`, либо отсутствовали в унифицированном виде.

## Decision

1. **Модуль базовых виджетов в `crates/rusty-ui::widgets`:**
   - Реализовать состояния интерактивных элементов `WidgetState` (`Normal`, `Hover`, `Pressed`, `Disabled`).
   - `ToggleSwitch`: стандартизированный переключатель 38×20 px со сглаженным круговым ползунком, активным акцентным цветом палитры, неактивным нейтральным серым фоном и антиалиасингом кругов.
   - `PushButton`: универсальная кнопка с поддержкой стилей `ButtonStyle` (`Primary`, `Secondary`, `Destructive`), состояний ховера/нажатия и системных скруглений (8 px).
   - `GroupCard`: карточка сгруппированных строк настроек со скруглением 12 px, системным фоном `pal.card_bg` и разделителями строк с настраиваемым отступом `draw_row_divider`.
   - `SearchField`: капсульное поле ввода со скруглением 8 px, векторной иконкой лупы, опциональным кольцом фокуса и интерактивной кнопкой очистки (14×14 px) с хит-тестингом `SearchFieldHit`.

2. **Сквозная интеграция `crates/rusty-ui` во все приложения:**
   - **`apps/rusty-files`:** подключить `rusty-ui.workspace = true`, заменить локальные константы на `TrafficLightButtons::default_csd()`, отрисовку микроглифов перевести на `TrafficLightKind` и `glyph_segments()`.
   - **`apps/rusty-terminal`:** подключить `rusty-ui.workspace = true`, использовать `TrafficLightButtons::default_csd()` в вычислении геометрии заголовка, унифицировать рендеринг глифов в SHM- и EGL-пайплайнах через `TrafficLightKind` и `glyph_segments()`.
   - **`apps/rusty-installer`:** подключить `rusty-ui.workspace = true`, использовать `TrafficLightButtons::default_csd()` в компоновке окна установщика, унифицировать векторную отрисовку микроглифов.

3. **Интеграция с композитором (Server-Side Decorations):**
   - В `crates/rusty-compositor/Cargo.toml` добавить зависимость `rusty-ui.workspace = true`.
   - В `crates/rusty-compositor/src/surfaces/mod.rs` реэкспортировать `TrafficLightKind` из `rusty_ui`.
   - В `crates/rusty-compositor/src/shell_render/ssd.rs` перевести хит-тестинг заголовка SSD и генерацию элементов светофоров на `TrafficLightButtons::default_ssd()`, устранив дублирование магических чисел координат.

## Consequences

- Все приложения (`rusty-settings`, `rusty-files`, `rusty-terminal`, `rusty-installer`) и серверные декорации композитора теперь используют единый источник истины для оконных элементов управления и цветовой палитры.
- Любое изменение геометрии или стиля светофоров автоматически отражается во всей десктоп-системе.
- Появилась модульная библиотека переиспользуемых виджетов (`ToggleSwitch`, `PushButton`, `GroupCard`, `SearchField`), готовая для построения новых системных окон и диалогов.
