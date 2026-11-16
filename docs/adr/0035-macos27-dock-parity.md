# ADR-0035: App-centric Dock, локальный hover, Genie/Scale, Edge Placement, Auto-hide и Downloads Stack по macOS 27 Golden Gate

- **Статус:** accepted
- **Дата:** 2026-11-14 (обновлено 2026-11-16)
- **Задача:** `RUSTYOS-073`
- **Связанные решения:** `D-043`, `ADR-0023`, `ADR-0032`, `ADR-0033`, `ADR-0034`

## Контекст

До `RUSTYOS-073` Dock в RustyOS имел статический набор из `6` фиксированных пинов и добавлял отдельный слот на каждое открытое окно (`DockHit::WindowSlot`), ЛКМ по активному окну приводил к повторному сворачиванию, отсутствовали `Launch bounce`, позиции `Left`/`Right`, `Auto-hide`, графический `Downloads Stack`, внутренний drag-reorder/удаление пинов, настройка `[dock]` в `UserConfig` и отдельная вкладка `System Settings -> Desktop & Dock`, а сворачивание выполнялось мгновенно без GPU-анимации. По требованиям `RUSTYOS-073` и уточнению UX отказались от волнового масштабирования соседних значков в пользу спокойного локального hover только на наведённом ярлыке.

## Решение

1. **Конфигурация `[dock]` и вкладка `System Settings -> Desktop & Dock` (`config.rs`, `shell_render/modals.rs`, `state/shell_actions.rs`):**
   - Добавлены поля `dock_icon_size` (`36..=68`), `dock_position` (`Bottom`/`Left`/`Right`), `dock_minimize_effect` (`Genie`/`Scale`), `dock_minimize_into_app`, `dock_auto_hide`, `dock_animate_opening`, `dock_show_indicators`, `dock_show_recent`, `dock_pinned_apps`, `dock_downloads_display` (`Folder`/`Stack`), `dock_downloads_view` (`Automatic`/`Fan`/`Grid`/`List`) и расширен `DownloadsSortMode` до 5 режимов (`DateAdded`, `DateModified`, `Name`, `Size`, `Kind`). Устаревший ключ `[dock].magnification_size` безопасно игнорируется при парсинге для обратной совместимости старых `config.toml`.
   - В `System Settings` добавлена вкладка `SettingsTab::DesktopDock` с 8 интерактивными контролами, синхронизированными с `UserConfig::to_toml_string()` / `parse_toml()`.

2. **App-centric модель Dock и контекстные меню (`shell_render/dock.rs`, `state/shell_actions.rs`, `state/context_menu_actions.rs`):**
   - `build_dock_tiles()` строит единые плитки по `canonical_dock_app_key` в трёх секциях: `Pinned` -> `UnpinnedOrRecent` (открытые + до `4` недавних при `dock_show_recent`) -> `MinimizedWindow` (при `!dock_minimize_into_app`) -> `Utility` (`Downloads`, `Trash`).
   - ЛКМ по иконке приложения фокусирует/поднимает/циклически переключает его окна или запускает с `Launch bounce` без дублирующего сворачивания активного окна; `Alt+Return`, пункт меню нового окна или `TopBar` открывают дополнительный инстанс.
   - ПКМ по приложению выводит список его открытых окон (`DockFocusWindow`), `Keep in Dock` / `Remove from Dock`, `Open at Login`, `Show All Windows` (App Exposé с фильтром `OverviewRenderState::filter_label`) и `Quit` / `Alt+Force Quit`.

3. **Локальный hover, позиция, Auto-hide и Drag-to-Reorder (`shell_render/dock.rs`, `shell_render/tokens.rs`, `input/{shell_pointer,pointer}.rs`):**
   - `compute_dock_layout` рассчитывает стабильную горизонтальную (`Bottom`) или вертикальную (`Left`/`Right`) геометрию с фиксированной шириной капсулы и неизменными соседними слотами: при hover увеличивается (`+8 px` по центру своего слота) и приподнимается (`+6 px`; `+2 px` без масштабирования при `reduced_motion`) только наведённая иконка со стеклянной tooltip-пилюлей, без перерисовки на движение курсора внутри одного слота.
   - Также поддерживаются смещение под `auto_hide_reveal` и превью перетаскивания пина / облачка `"Remove"` при вытягивании за пределы Dock на `>= 52 px`.
   - `usable_work_area_with_dock` адаптирует рабочую область WM при переключении `Bottom`/`Left`/`Right` и `Auto-hide`.

4. **GLES `Genie`/`Scale` сворачивание и графический `Downloads Stack` (`surfaces/window_minimize.rs`, `shell_render/dock_stack.rs`, `frame_composer.rs`):**
   - Безопасный (`unsafe_code = "deny"`) `WindowMinimizePipeline` предкомпилирует шейдер, захватывает offscreen-снапшот сворачиваемого окна через `OutputDamageTracker::render_output` (с переиспользованием текстуры при совпадении размера и `DamageBag` на каждый шаг прогресса `progress`) и деформирует его фрагментным GLES-шейдером `WINDOW_MINIMIZE_FRAG` (`Genie` или `Scale`) в целевой слот Dock для любой кромки экрана (с fallback при отсутствии FBO).
   - Warp-элементы выводятся поверх рабочего стола/окон вне `backdrop_elements`, чтобы исключить повторные проходы Dual-Kawase blur во время 280/250 мс анимаций сворачивания и разворачивания.
   - `ShellOverlayView::DockStack` отображает интерактивный Liquid Glass поповер `Downloads` в режимах `Fan`, `Grid` и `List` (и авто-выбором `Automatic` с учётом края Dock), поддерживая клики, клавиатуру и переход в `rusty-files`.

## Последствия

- Архитектура Dock унифицирована по принципу «одно приложение — один слот», полностью конфигурируется в рантайме и через `config.toml`, сохраняя zero-`unsafe` и поврежденчески-экономный GLES-пайплайн.
- Просмотр свёрнутых окон доступен как отдельными мини-слотами, так и внутри иконки приложения (`minimize_into_app`), а фильтрованный App Exposé переиспользует существующий `Overview`.
