# ADR-0019: 1:1 macOS Dock Visual Parity, Top-Lit Liquid Glass & 4x4 SSAA Squircle Icons (`RUSTYOS-050`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

После `RUSTYOS-048` и `RUSTYOS-049` живая инспекция нижнего `Dock` в сравнении с референсом macOS Dock выявила существенные визуальные расхождения:
1. Панель Dock имела чрезмерное скругление полной пилюли (`DOCK_HEIGHT = 72`, `DOCK_RADIUS = 36`), слишком тёмную непрозрачную заливку (`[42, 32, 64, 212]`) и серые подложки/боксы вокруг активных и наведённых слотов, визуально «топившие» иконки.
2. По умолчанию отображались только 3 закреплённые иконки (`Terminal`, `Files`, `Overview`) без macOS-каденции (`Files`, `Overview`, `Terminal`, `Browser`, `Notes`, `Settings`), постоянного разделителя и правой утилитарной пары (`Downloads`, `Trash`).
3. Процедурные иконки в `crates/rusty-compositor/src/icons.rs` использовали экспоненту суперэллипса `3.8`, `2x2` SSAA и яркий замкнутый обод по всему периметру, не имея мягкой тени под карточками, hover-увеличения, стеклянного tooltip-подписи над наведённой иконкой и свободных силуэтов для `Downloads`/`Trash`.
4. Отдельный Wayland layer-shell клиент `apps/rusty-shell/src/wayland.rs` продолжал рисовать узкую капсулу `(240, 56)` с серыми подложками `SURFACE_OVERLAY`.

## Рассмотренные варианты

- **Вариант A (Выбран):**
  - В `crates/rusty-compositor/src/surfaces.rs` добавить GPU-кэшируемый рендер полупрозрачного top-lit жидкого стекла `UiShapeCache::dock_glass_panel_element` (`sample_dock_glass_pixel` с анти-бандинговым 4×4 Bayer дитерингом, верхним зеркальным ободом 1 px и непрозрачным fallback `[31, 25, 45]` при `reduce_transparency = true`) и мягкой тени `UiShapeCache::dock_icon_shadow_element` (`sample_dock_icon_shadow_pixel`), ограниченной по кардинальности ключей (`dock_glass_panels` <= 16, `dock_icon_shadows` на квантованный размер иконки).
  - В `crates/rusty-compositor/src/icons.rs` расширить `BuiltinAppIcon` вариантами `Browser`, `Notes`, `Settings`, `Downloads` (свободная стопка полупрозрачных синих папок со стрелкой вниз) и `Trash` (свободная матово-металлическая конусная корзина без внешней рамки), перевести `squircle_sd` на экспоненту `4.8` с верхним бликом вместо жёсткой замкнутой обводки и повысить суперсэмплинг `sample_builtin_squircle_pixel` до `4x4` (16 выборок на пиксель).
  - В `crates/rusty-compositor/src/shell_render.rs` перевести геометрию на `DOCK_HEIGHT = 68`, `DOCK_RADIUS = 22`, `DOCK_ITEM_SIZE = 52`, `DOCK_ICON_SIZE = 50`, `DOCK_ITEM_GAP = 6`, `DOCK_DIVIDER_GAP = 14`, убрать серые слот-боксы, закрепить `Files` / `Overview` / `Terminal` / `Browser` / `Notes` / `Settings` + постоянный вертикальный разделитель `1x38` + `Downloads` / `Trash`, добавить мягкие `5x5` SDF-точки индикации (`pip`), квантованное hover-увеличение (`50..=58 px`, +6 px вверх; отключено при `reduced_motion`) и стеклянный tooltip `dock_tooltip_rect_and_label`.
  - В `crates/rusty-compositor/src/state.rs` поддержать клики по новым `DockHit::{PinnedBrowser, PinnedNotes, PinnedSettings, PinnedDownloads, PinnedTrash}` (с fallback `Settings -> Control Center`, `Downloads -> $HOME/Downloads`, `Trash -> trash:///`), а в `apps/rusty-shell/src/wayland.rs` синхронизировать `render_dock_pixels` и размеры layer-surface `(488, 68)` с концентрическим радиусом `20`.
- **Вариант B:** Оставить полную пилюлю `DOCK_RADIUS = 36` и добавить лишь новые иконки: не давало концентрической гармонии с 50 px squircle-иконками и сохраняло тёмный тяжёлый контейнер.
- **Вариант C:** Ввести реальный многопроходный Kawase/Gaussian blur-behind в GLES-шейдерах: не укладывается в текущий этап без отдельного FBO blur-бюджета на GPU и требует отдельного hardware ADR.

## Решение

Утверждён **Вариант A** (`APPROVE: RUSTYOS-050 Option A` от 2026-11-13).

## Последствия

- `Dock` визуально выровнен с референсом macOS при сохранении оригинальной графики RustyOS (без сторонних брендовых ресурсов Apple) и фирменной палитры Purple Glass.
- Свободные силуэты `Downloads` и `Trash` отделены постоянным 1 px разделителем и рендерятся непосредственно поверх матового стекла без квадратных теней или подложек.
- Кэширование тени и стеклянной панели ограничено дискретными ключами в `UiShapeCache` и квантованными размерами при hover-анимации, исключая раздувание текстурного кэша GLES.
