# ADR-0045: Модульная декомпозиция подсистемы рендеринга apps/rusty-files/src/render и консолидация разрозненных корней views.rs/dialogs.rs в layout.rs

- **Статус:** accepted
- **Дата:** 2026-11-17
- **Задача:** `RUSTYOS-089`
- **Связанные решения:** `D-053`, `D-052`, `D-051`, `D-050`, `D-030`, `ADR-0044`, `ADR-0043`, `ADR-0030`

## Контекст

1. **Разрастание монолитных корней рендеринга:**
   В ходе полного архитектурного аудита проекта было зафиксировано, что подсистема отрисовки файлового менеджера `rusty-files` была разбита исторически некорректно:
   - `apps/rusty-files/src/render.rs` разросся до 2,614 строк, объединяя в себе 2D Canvas растровые примитивы, кеш шрифтов Inter, сайдбар, тулбар, Miller Columns, списочный режим, инспектор превью и устаревшие функции попапов;
   - `apps/rusty-files/src/views.rs` содержал 1,185 строк и сочетал в себе геометрию вкладок/хлебных крошек/сетки иконок/галереи с отрисовкой этих представлений;
   - `apps/rusty-files/src/dialogs.rs` содержал 1,053 строки и совмещал геометрию модальных окон/оверлеев с их отрисовкой.
2. **Нарушение принципа единой ответственности:**
   Наличие отдельных корневых модулей `views.rs` и `dialogs.rs`, которые наполовину состояли из разметки (`layout`), наполовину из рисования (`render`), размывало границы подсистем и затрудняло рефакторинг.
3. **Требование:**
   Провести чистую модульную реорганизацию:
   - Перенести все структуры геометрической раскладки и расчёты хит-тестинга из `views.rs` и `dialogs.rs` в единый канонический модуль `apps/rusty-files/src/layout.rs`;
   - Преобразовать монолитный `render.rs` и оставшийся код отрисовки в единый модульный пакет `apps/rusty-files/src/render/`;
   - Упразднить разрозненные файлы `render.rs`, `views.rs` и `dialogs.rs`;
   - Сохранить 100% совместимость публичных интерфейсов и прохождение всех тестов рабочего пространства.

## Решение

### 1. Консолидация геометрии и хит-тестинга в `apps/rusty-files/src/layout.rs`

Вся логика вычисления геометрии и хит-тестинга из `views.rs` (строки 19–401) и `dialogs.rs` (строки 16–290) перемещена в `apps/rusty-files/src/layout.rs`:
- Структуры геометрии представлений: `TabItemLayout`, `TabBarLayout`, `PathCrumbLayout`, `IconCellLayout`, `IconsGridLayout`, `GalleryLayout`;
- Функции расчёта раскладки: `compute_tab_bar_layout`, `compute_path_breadcrumbs_layout`, `compute_icons_grid_layout`, `compute_gallery_layout`;
- Хит-тестинг представлений: `hit_test_tab_bar`, `hit_test_path_bar`, `hit_test_icons_grid`, `hit_test_gallery`;
- Структуры модальных окон и оверлеев: `QuickLookLayout`, `GetInfoLayout`, `GoToFolderLayout`, `ConflictDialogLayout`, `TransferDialogLayout`, `DialogOverlaysLayout`;
- Функции расчёта раскладки оверлеев: `compute_quick_look_layout`, `compute_get_info_layout`, `compute_goto_folder_layout`, `compute_conflict_dialog_layout`, `compute_dialog_overlays_layout`;
- Хит-тестинг оверлеев: `hit_test_dialog_overlays`.

### 2. Модульный пакет `apps/rusty-files/src/render/`

Подсистема рендеринга разделена на 6 сфокусированных слоёв:

1. **`canvas.rs` (1,120 строк):**
   - Загрузка шрифтов Inter (`Regular`, `SemiBold`) через `Fontdue`;
   - Потокобезопасный `thread_local` кеш растрированных глифов (`CachedGlyph`, `GLYPH_CACHE`);
   - Базовая структура `Canvas<'a>` для прямого рисования в буфер пикселей `ARGB8888`;
   - 2D SDF и RGSS-16 сглаженные примитивы: `fill_rect`, `fill_rounded_rect`, `stroke_rounded_rect`, `fill_circle_f`, `stroke_ellipse_aa`, `stroke_line_aa`, `stroke_polyline_aa`, `fill_poly_aa`, отрисовка текста и скроллбаров.
2. **`chrome.rs` (432 строки):**
   - Элементы оконного хрома macOS Golden Gate: CSD светофоры управления окном (красный, жёлтый, зелёный), сегментированные пилюли режимов, поле быстрого поиска, вторичная полоса области поиска (`SearchScopeBar`), сегментированная панель вкладок (`render_tab_bar`) и хлебные крошки пути (`render_path_bar`).
3. **`sidebar.rs` (128 строк):**
   - Полупрозрачный Finder сайдбар с разделителем, подсветкой активного пункта и плавающими скроллбарами;
   - Контекстный баннер корзины с кнопкой «Empty» (`render_trash_banner`).
4. **`views.rs` (1,204 строки):**
   - Отрисовка основных файловых представлений: Miller Columns (`render_miller_columns`), List View (`render_list_view`), сетка иконок 64px с SSAA сглаживанием (`render_icons_grid`), галерея с предпросмотром (`render_gallery_view`), плавающий инспектор метаданных (`render_preview_inspector`) и строка состояния (`render_status_bar`).
   - Вспомогательные функции отрисовки эскизов и превью: `draw_aspect_fitted_preview`, `draw_framed_thumbnail`, `render_text_preview_card`.
5. **`dialogs.rs` (783 строки):**
   - Модальные листы и оверлеи: быстрое превью Quick Look (`Space`), окно свойств Get Info (`Ctrl+I`), лист перехода Go to Folder, карточки подтверждения конфликтов переноса файлов и инлайн-редактор переименования.
6. **`popup.rs` (353 строки):**
   - Стандарт Liquid Glass всплывающих меню и поверхностей `xdg_popup` (`RUSTYOS-087`, ADR-0043).
7. **`mod.rs` (521 строка):**
   - Определения `Palette`, функции сопоставления палитр `palette_for`, хелпер `rgba`, утилита затухания скроллбара `overlay_scrollbar_color`;
   - Публичные входные точки генерации кадров: `render_window_pixels`, `render_window_pixels_with_popup`;
   - Реэкспорт всех подмодулей для 100% обратной совместимости;
   - Полный комплект unit-тестов рендеринга (проверка тем Light/Dark, отсечения клиппинга, отрисовки всех видов).

### 3. Упразднение монолитов

Удалены устаревшие файлы:
- `apps/rusty-files/src/render.rs`
- `apps/rusty-files/src/views.rs`
- `apps/rusty-files/src/dialogs.rs`
В `apps/rusty-files/src/main.rs` удалены более не существующие директивы `mod views;` и `mod dialogs;`.

## Последствия

- **Чистая модульность:** Ни один файл подсистемы рендеринга теперь не является монолитом: размеры файлов варьируются от 128 до 1,204 строк с чётко очерченными границами ответственности.
- **Логическая целостность геометрии:** Вся геометрия и хит-тестинг приложения `rusty-files` теперь централизованно располагаются в `layout.rs`.
- **100% сохранение тестов:** Все 244 unit-теста воркспейса (включая 15 тестов `rusty-files` и 43 теста `rusty-services`) компилируются и проходят успешно.
- **Чистота кода:** `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` проходят с нулевым количеством предупреждений.
