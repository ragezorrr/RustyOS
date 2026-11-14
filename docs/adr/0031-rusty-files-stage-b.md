# ADR-0031: Этап B файлового менеджера `apps/rusty-files` до паритета с Golden Gate и общий декодер `rusty_services::image_decode`

- **Статус:** accepted
- **Дата:** 2026-11-14
- **Автор:** AI Agent + User
- **Связанные задачи:** RUSTYOS-066 (развивает ADR-0030)

## Контекст

1. В Этапе A (`RUSTYOS-065`, ADR-0030) были заложены каркас `apps/rusty-files` (режимы **Columns** и **List**) и `crates/rusty-services/src/file_manager.rs`, но слоты **Icons** и **Gallery** в View Switcher оставались зарезервированы под Этап B, отсутствовали `Quick Look`, панель вкладок (`Tabs`), `Path Bar`, диалоги `Get Info` / `Go to Folder`, переименование, копирование/перемещение с разрешением конфликтов и предпросмотр растровых изображений/текста.
2. Чистый Rust PNG-декодер и билинейный скейлер уже существовали в `crates/rusty-compositor/src/icons.rs`, однако их переиспользование из `apps/rusty-files` требовало вынесения в общий крейт рабочей области без добавления новых внешних зависимостей (`--offline`) и при сохранении `#![deny(unsafe_code)]`.
3. По визуальным референсам macOS 27 Golden Gate (`Icons`, `List`, `Columns`, `Gallery`, `Quick Look`, `Get Info`) требовалась также доработка чёткости UI-иконок и 4x4 SSAA папок.

## Решение

1. **Общий декодер изображений без зависимостей (`crates/rusty-services/src/image_decode.rs`):**
   - Pure-Rust PNG (zlib/deflate + фильтры + цветовые пространства) и PPM декодер плюс билинейный скейлер вынесены из `crates/rusty-compositor/src/icons.rs` в `rusty_services::image_decode` (`decode_png_rgba`, `decode_png_rgba_limited`, `decode_ppm_rgba_limited`, `load_raster_preview_bgra`, `read_image_dimensions`, `scale_rgba_to_premul_bgra`, `load_png_wallpaper_bgra`, `load_circular_avatar_bgra`).
   - `crates/rusty-compositor/src/icons.rs` переведён на реэкспорт/вызов `rusty_services::image_decode` без дублирования логики.

2. **Расширение бэкенда файлового менеджера (`crates/rusty-services/src/file_manager.rs`):**
   - Включены все четыре режима `FileViewMode::{Icons, List, Columns, Gallery}` (`is_supported() == true`).
   - Добавлены: инспекция метаданных `inspect_entry_info` → `FileDetailedInfo` (с ограниченным подсчётом размера каталогов и размерами изображений), `read_text_preview` → `TextPreview`, `rename_entry` с проверкой `validate_single_component_name`, `expand_go_to_input` / `resolve_go_to_path` / `suggest_go_to_paths`.
   - Реализован chunked перенос/копирование `copy_or_move_with_progress` (`TransferKind::{Copy, Move}`, `TransferConflictDecision::{Replace, KeepBoth, Skip, Stop}`, `TransferProgress`, `TransferOutcome`) с защитой от циклических копий и атомарной отменой через `AtomicBool`.

3. **Доведение UI `apps/rusty-files` до паритета Golden Gate (`src/{views,dialogs,icons,layout,render,state,wayland}.rs`):**
   - **Режимы просмотра:** реализованы **Icons** (`compute_icons_grid_layout`, `render_icons_grid` — сетка 100x104 px с превью и бейджами тегов) и **Gallery** (`compute_gallery_layout`, `render_gallery_view` — крупная сцена превью с фоновой загрузкой PNG/PPM/текста, нижняя горизонтальная лента миниатюр `GalleryThumbLayout` и правая панель `Information` со свойствами и быстрыми кнопками `Quick Look` / `Get Info` / `Show in Columns`).
   - **Навигация и вкладки:** верхняя полоса вкладок (`compute_tab_bar_layout`, `FileTabState`, `+`, `Ctrl+T`, `Ctrl+W`, `Ctrl+Tab` / `Ctrl+Shift+Tab`), кликабельный нижний `Path Bar` (`compute_path_breadcrumbs_layout`) и переключатель темы `Light <-> Dark`.
   - **Операции и оверлеи (`dialogs.rs`, `state.rs`):** встроенный `Quick Look` (`Space`), `Get Info` (`Ctrl+I`), `Go to Folder...` (`Ctrl+Shift+G`) с подсказками автодополнения, inline `Rename` (`F2` / контекстное меню), буфер `Copy`/`Cut`/`Paste`/`Duplicate` (`Ctrl+C`/`X`/`V`/`D`) с фоновым потоком переноса, пилюлей прогресса в toolbar, кнопкой отмены и интерактивным модальным диалогом конфликта (`Replace` / `Keep Both` / `Skip` / `Stop`).
   - **Качество иконок (`icons.rs`):** pixel-aligned `14–18 px` контуры для sidebar/toolbar (`snap_half`, `snap_edge`), doc-иконки с загнутым углом (`draw_dogear_document_icon`) и 4x4 SSAA Golden Gate папки (`draw_entry_icon`, `draw_large_entry_artwork`) с гравировкой категорий (`Desktop`, `Documents`, `Downloads`, `Pictures`, `Music`, `Applications`, `Developer`, `Trash`) и окраской по тегам.

## Последствия

- `rusty-compositor` и `rusty-files` используют единый pure-Rust конвейер декодирования изображений без новых внешних крейтов и без `unsafe`.
- Все 4 режима просмотра Golden Gate (`Icons`, `List`, `Columns`, `Gallery`), превью, вкладки и базовые операции с ФС (`Copy`/`Move`/`Rename`/`Trash`/`Get Info`) выполняются асинхронно относительно Wayland event loop `rusty-files`.
- Дальнейшие форматы медиа-превью (SVG, видеокадры через PipeWire/GStreamer, PDF) при необходимости подключаются к существующему кэшу `CachedPreview` без изменения компоновки.
