# ADR-0057: Единый стиль системных CSD-окон и радиуса теней композитора (`RUSTYOS-105`)

## Status

accepted

## Context

После выделения `System Settings` в отдельное Wayland CSD-окно ([apps/rusty-settings](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/render.rs), `ADR-0055`) три основных системных приложения (`rusty-settings`, `rusty-files`, `rusty-updater`) и контур/тень окон композитора визуально расходились:

1. **Разные радиусы углов и теней:** `rusty-settings` использовал `WINDOW_CORNER_RADIUS = 14`, тогда как `rusty-files`, `rusty-updater` и композитор ([tokens.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/tokens.rs), `WINDOW_RADIUS = 20`) применяли `20 px`, из-за чего углы теней композитора не совпадали с углами настроек, а соседние окна имели разное скругление.
2. **Разные traffic lights:** `rusty-settings` использовал `14 px` кружки (`r = 7.0`, шаг `22 px`, `#EE5A6A` / `#F4B63F` / `#3CC68A`, но с лишним квадратным `stroke_rounded_rect`), а `rusty-files` и `rusty-updater` — `12 px` / `12.4 px` кнопки на других координатах и с другой палитрой.
3. **Разнобой карточек и обводок:** в `rusty-updater` карточки и пилюли обводились жёсткой `1 px` рамкой поверх более тусклого фона; в `rusty-files` сайдбар использовал отличные от `rusty-settings` оттенки, радиус активной строки `r = 7` и `1 px` рамку/блики на капсулах тулбара. В `rusty-settings` и `rusty-files` `1 px` контур окна рисовался после маски углов.

## Decision

1. **Единый радиус окон и теней (`14 px`):**
   - Установить `WINDOW_RADIUS = 14` в [crates/rusty-compositor/src/shell_render/tokens.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/tokens.rs) (и обновить проверку в [crates/rusty-compositor/src/shell_render/tests.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/tests.rs)), а также `WINDOW_CORNER_RADIUS = 14` в [apps/rusty-files/src/layout.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/layout.rs) и [apps/rusty-updater/src/layout.rs](file:///home/anc/RustyOS_Main/apps/rusty-updater/src/layout.rs).
   - Во всех трёх клиентах рисовать `1 px` внешний контур окна (`[0, 0, 0, 24]` Light / `[255, 255, 255, 22]` Dark) строго до применения угловой маски ([apps/rusty-settings/src/render.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/render.rs), [apps/rusty-files/src/render/mod.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/render/mod.rs), [apps/rusty-updater/src/render.rs](file:///home/anc/RustyOS_Main/apps/rusty-updater/src/render.rs)).
2. **Единые traffic lights (`14 px`, шаг `22 px`):**
   - Во всех трёх приложениях использовать `Rect(14, 13, 14, 14)`, `Rect(36, 13, 14, 14)`, `Rect(58, 13, 14, 14)` (центры `(21.0, 20.0)`, `(43.0, 20.0)`, `(65.0, 20.0)`, радиус `7.0`, цвета `#EE5A6A`, `#F4B63F`, `#3CC68A`), удалив квадратную рамку ореола в `rusty-settings`.
3. **Палитра и бесшовные карточки/капсулы по эталону `rusty-settings`:**
   - В `rusty-updater` установить `TITLEBAR_HEIGHT = 42`, фон `#F5F6FA`/`#1C1C1D`, карточки `hero_card`/`progress_card`/`list_card` (`r = 12`, `#FFFFFF`/`#2C2C2D`) и пилюли (`refresh_btn`, `filter_tabs`, `logs_toggle_btn`, `rebuild_btn`, `r = 10`/`14`) без серых рамок.
   - В `rusty-files` привести сайдбар к палитре `#ECEDF1`/`#262628`, активный элемент — к `#D9DADD`/`#3E3E41` со скруглением `r = 10`, убрать серые рамки с `draw_glass_pill`, вкладок и кнопок `Clear` / `Empty`.

## Consequences

- Все три системных Wayland CSD-приложения (`rusty-settings`, `rusty-files`, `rusty-updater`) и аналитическая тень `WindowShadowPipeline` композитора образуют единую визуальную систему.
- Подтверждено `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (`256 passed, 0 failed`) и живым `--winit` прогоном в `Light` и `Dark` режимах с открытыми окнами.
