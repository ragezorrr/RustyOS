# ADR-0058: Унификация контекстных и выпадающих меню под эталон `macOS 27 Golden Gate` (`RUSTYOS-107`)

## Status

accepted

## Context

После приведения `Dock`, `Control Center` и системных CSD-окон к эталону `macOS 27 Golden Gate` (`ADR-0056`, `ADR-0057`) контекстные и выпадающие меню композитора ([top_bar/menus.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/menus.rs), [context_menu.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/context_menu.rs)) и клиента `rusty-files` ([popup.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/render/popup.rs)) расходились с эталоном (`27-Golden-Gate-Go-Menu.png`, `27-Golden-Gate-Finder-Menu.png`, `27-Golden-Gate-About-This-Mac-Menu.png`):

1. Внешний радиус `16 px` и инсет ховер-пилюли (`6 px`, радиус `8 px`, высота строки `28 px`) не образовывали концентрической пары при `5 px` отступе контейнера, из-за чего углы выделения первого и последнего пунктов смотрелись тяжелее.
2. Все пункты меню `TopBar` и `ContextMenu` жёстко требовали `UiGlyphKind` (либо в `rusty-files` резервировали пустое поле слева `24 px`), тогда как в эталоне безыконочные текстовые команды (`Back`, `Forward`, `Enclosing Folder`, `Workspace Overview`, `Sleep`, `Close`, контекстные команды `rusty-files`) выравниваются напрямую по `x = 14 px`, а секции с иконками рисуют `14×14 px` глиф на `x = 14 px` и подпись на `x = 35 px`.
3. Шорткаты в системных меню смешивали `Super+`/`Alt+`/`Ctrl+` со значками `⌘`/`⌥`/`⇧`/`⌃`, а правые модификаторы при наведении отличались по яркости от белой подписи строки.

При этом шейдер и материалы `Liquid Glass` (`liquid_glass.rs`, `sdf.rs`), а также `Dock` (`dock.rs`) и `Control Center` (`quick_settings.rs`) уже соответствуют эталону и не должны были затрагиваться.

## Decision

1. **Концентрическая геометрия контейнера, `Liquid Glass` и клиентский `xdg_popup`-контракт:**
   - Установить `MENU_POPOVER_RADIUS = 13` (для тостов уведомлений в [overlay_shapes.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/overlay_shapes.rs) сохранён радиус `16`), `MENU_POPOVER_PAD_Y = 5`, `MENU_ITEM_HEIGHT = 24`, `MENU_SEPARATOR_HEIGHT = 9` в [tokens.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/tokens.rs), [top_bar/menus.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/top_bar/menus.rs), [layout.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/layout.rs), [popup.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/render/popup.rs) и [wayland.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/wayland.rs) (`xdg_popup`).
   - Ховер-пилюля в [chrome_shapes.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/chrome_shapes.rs), [context_menu.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/context_menu.rs) и [popup.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/render/popup.rs) рисуется с инсетами `HOVER_PILL_INSET_H = 5`, `HOVER_PILL_INSET_V = 1` (`h = 22 px`), радиусом `7 px` и чистой акцентной заливкой `pal.accent_rgba` без рамки (`r_outer - pad = 13 - 5 = 8 ≈ 7`).
   - В [frame_composer.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/frame_composer.rs) клиентские `xdg_popup` исключены из `backdrop_list` и рендерятся через `push_client_popup_elements` / `push_popup_glass_panels` **после** `prepare_liquid_glass_backdrop` и поверх основных окон/`Dock` с помощью `push_liquid_glass_panel(..., GlassPanelStyle::Popover, 13, Some((16, 52)))`. Клиент (`sync_popup_surface` в [wayland.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/wayland.rs)) передаёт `wl_region = spec.layout.rect` (без `POPUP_SHADOW_PAD`), а `render_popup_surface_pixels_with_glass` в [popup.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/render/popup.rs) при активном композиторном стекле оставляет фон SHM-буфера прозрачным (`0x00000000`) без непрозрачного `popover_bg` и CPU-теней.
2. **Двухрежимная левая колонка (`Option<UiGlyphKind>` и разделение `any_non_swatch_checked` / `swatch_rgb`):**
   - В `TopBarMenuEntry::Item`, `ResolvedTopBarMenuItem::Item` и `ContextMenuEntry::Item` тип `icon` переведён на `Option<UiGlyphKind>`; добавлены `ContextMenuEntry::text_item` и `.without_icon()`.
   - Секции без иконок выравниваются по `x = 14 px`, секции с иконками рисуют `14×14 px` глиф на `x = 14 px` и подпись `13.0 px` на `x = 35 px` (в рамках одной секции между разделителями выдерживается единый отступ без смешивания).
   - В `rusty-files` [popup.rs](file:///home/anc/RustyOS_Main/apps/rusty-files/src/render/popup.rs) состояние `any_non_swatch_checked` отделено от `swatch_rgb`: обычные строки (`Open`..`Move to Trash`) начинаются на `x + 14`, строки цветовых тегов рисуют кружок `r = 4.8` с центром `cx = x + 19.0` (левый край `x + 14.2` вровень с текстом `Open`), подписью на `x + 32` и галочкой на `right - 18`, а меню с галочками без тегов (`SortMenu`) — галочку на `x + 14` и текст на `x + 28`.
3. **Правая колонка шорткатов (`12 px`), шевронов и изолированность правки:**
   - Шорткаты всех меню переведены на нотацию `⌘`/`⌥`/`⇧`/`⌃`/`⌫`/`▲`/`⇥`, рендерятся размером `12.0 px` с выравниванием по правому краю (`x + w - 14`) и вместе с шевроном `>` (`12 px` в `x + w - 24`) / галочкой `✓` синхронно становятся чисто-белыми (`#FFFFFF`) на активной плашке ховера.
   - Файлы [liquid_glass.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/liquid_glass.rs), [sdf.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/sdf.rs), [dock.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/dock.rs) и [quick_settings.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/quick_settings.rs) не изменялись.

## Consequences

- Все выпадающие меню верхней панели, системные контекстные меню рабочего стола/дока/окон и клиентские `xdg_popup` меню `rusty-files` разделяют единую концентрическую геометрию и типографику `macOS 27 Golden Gate` в режимах `Light` и `Dark`.
- Подтверждено `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (`256 passed, 0 failed`), smoke-прогонами `--winit --max-frames 35` (Wayland и X11) и живыми `--winit`-снимками меню `Go`, `System`, `Desktop` и клиентских `xdg_popup` в `rusty-files`.
