# ADR-0061: Материал карточек `GlassPanelStyle::NotificationCard`, унифицированная многострочная типографика и палитра системных иконок (`RUSTYOS-110`)

## Status

accepted

## Context

При визуальном сопоставлении карточек уведомлений RustyOS с эталоном `macOS 27 Golden Gate` ([27-Golden-Gate-Notification-Center.png](file:///home/anc/RustyOS_Main/docs/design/references/macos-27-golden-gate/desktop-and-shell/27-Golden-Gate-Notification-Center.png)) выявлены 5 проблем:

1. Карточки в [notification_center.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/notification_center.rs) и всплывающие тосты в [overlay_shapes.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/overlay_shapes.rs) использовали `GlassPanelStyle::Popover` ([liquid_glass.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/liquid_glass.rs): прозрачность `0.32–0.60`, преломление `20.0`, `rim = 0.85`), из-за чего яркие участки обоев и окон просвечивали пятнами на правом крае карточки, а по периметру возникала широкая стеклянная линза.
2. Fallback-иконка `BuiltinAppIcon::RustySystem` и первый пресет `WindowFallback` + плитка миниатюры в [icons.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/icons.rs) оставались фиолетовыми, что противоречит `DESIGN.md` / `AGENTS.md` §5.
3. Всплывающие тосты и карточки `Notification Center` расходились по геометрии и шрифтам: `16 px` против `20 px` радиуса, `340 px` против `344 px` ширины, `Regular 12.5 px` + `popover_text_muted` против `Bold 13.0 px` + `popover_text_secondary`, жёсткая обрезка символов (`truncate_ascii`), метка `"now"` и прижатый отступ иконки `x + 14`.

## Decision

1. **Изолированный стиль `GlassPanelStyle::NotificationCard` без изменения `Dock`, `ControlModule` и `Popover`:**
   - В [surfaces/mod.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/mod.rs), [surfaces/liquid_glass.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/liquid_glass.rs) и [surfaces/sdf.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/sdf.rs) добавлен вариант `GlassPanelStyle::NotificationCard` с матовым тонированием (`alpha = 0.56 + 0.24 * g` Light / `0.62 + 0.22 * g` Dark), мягким преломлением `3.0`, деликатным краем `rim = 0.22`, насыщенностью `1.0` и нулевой хроматической аберрацией поверх `buffers.half_out` (и соответствующим CPU fallback в `sdf.rs`). Параметры `Dock`, `ControlModule` и `Popover` не менялись.

2. **Унификация геометрии, хром-слоя и многострочной типографики карточек:**
   - В [top_bar/notifications.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/notifications.rs) введены единые константы (`NOTIFICATION_CARD_WIDTH = 344`, `NOTIFICATION_CARD_HEIGHT = 68`, `NOTIFICATION_CARD_RADIUS = 22`, `NOTIFICATION_CARD_RIGHT_MARGIN = 14`, `NOTIFICATION_ICON_OFFSET_X = 16`, `NOTIFICATION_TEXT_OFFSET_X = 66`) и общие функции `push_notification_card_chrome`, `wrap_notification_lines` (перенос до 2 строк тела с измерением ширины через `TextCache` и поддержкой `\n`) и `push_notification_card_text_lines`.
   - И [overlay_shapes.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/overlay_shapes.rs) / [text_pass.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/text_pass.rs), и [notification_center.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/notification_center.rs) переведены на эти общие функции без метки `"now"` и с контрастным жирным заголовком (`FontWeight::Bold`).

3. **Удаление legacy purple в системных скворклах:**
   - В [icons.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/icons.rs) цвета `BuiltinAppIcon::RustySystem` (`paint_rusty_system_glyph`), первый пресет `WindowFallback` и плитка `Overview` переведены на синюю палитру Golden Gate (`#007AFF`).

## Consequences

- Всплывающие уведомления и карточки в `Notification Center` имеют одинаковую матово-однородную фактуру `r = 22` без цветных пятен фона, концентричную посадку иконки `x + 16`, читаемую многострочную типографику и нейтрально-синюю системную иконку при полной сохранности `Dock`, `Control Center` и контекстных меню `Popover`.
