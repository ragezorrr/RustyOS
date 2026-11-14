# ADR-0030: Самостоятельный Wayland-файловый менеджер `apps/rusty-files` (macOS 27 Golden Gate, Этап A)

- **Статус:** accepted
- **Дата:** 2026-11-14
- **Автор:** AI Agent + User
- **Связанные задачи:** RUSTYOS-065 (развивает ADR-0028, ADR-0029)

## Контекст

1. В RustyOS принципиально не используются сторонние файловые менеджеры (`nautilus`, `dolphin`, `thunar`, `nemo`, `pcmanfm`), однако маршрутизация в `crates/rusty-compositor` (`launcher.rs`, `frame_composer.rs`, `state/shell_actions.rs`, `state/context_menu_actions.rs`) и `apps/rusty-shell` по-прежнему содержала кандидатов на эти внешние программы и вызовы `xdg-open` для папок и `trash:///`.
2. Реализация файлового менеджера внутри процесса композитора нарушила бы изоляцию event loop и отказоустойчивость (`frame pacing` и ввод не должны блокироваться при чтении медленных каталогов или сетевых/съёмных монтирований).
3. Пользователю требуется собственный файловый менеджер в эстетике macOS 27 (Golden Gate) на базе уже имеющихся в `Cargo.lock` крейтов без добавления новых зависимостей и без `unsafe`.

## Решение

1. **Общий бэкенд файловой подсистемы (`crates/rusty-services/src/file_manager.rs`):**
   - Реализовано обнаружение XDG-каталогов и `user-dirs.dirs` (`UserFileLocations`), томов/монтирований из `/proc/mounts` с декодированием `\040` и фильтрацией псевдо-ФС (`discover_mounts`, `parse_proc_mounts`), `.desktop`-приложений (`scan_applications_in`), недавних файлов (`.local/share/recently-used.xbel` и shallow scan) и сетевых/shared источников.
   - Реализованы операции `FreeDesktop` Trash (`list_trash_entries`, `move_to_trash`, `restore_from_trash`, `empty_trash`), безопасное создание каталогов (`create_unique_folder`, `validate_single_component_name`), фильтрация/сортировка (`filter_and_sort_entries`, `FileSortField`, `FileSortOrder`) и атомарное хранение локальных тегов/предпочтений UI в `~/.config/rustyos/file_tags.toml` (`FileTagsConfig`, `FileTagColor`, `FilesThemeMode`, `FileViewMode`).

2. **Самостоятельное Wayland-приложение (`apps/rusty-files`, Этап A):**
   - CSD Wayland-клиент поверх `smithay-client-toolkit` (`wl_shm` буферизация), `wayland-client` (`wl_keyboard` evdev-трансляция без неблокирующей `xkbcommon 0.7`) и `fontdue` (`Inter`). Неблокирующее чтение каталогов выполняется в фоновых потоках с доставкой результатов в `FilesAppState` через `mpsc`.
   - Визуальная компоновка `1:1` Golden Gate (`layout.rs`, `render.rs`, `state.rs`, `wayland.rs`): плавающий скруглённый sidebar с macOS traffic lights, секциями *Recents / Shared / Favorites / Locations / Tags* и скроллом; единый верхний toolbar с пилюлями `< | >`, View Switcher (активные **Columns** (Miller Columns + Preview/Inspector) и **List** в Этапе A; слоты **Icons** и **Gallery** зарезервированы под Этап B с явным статус-сообщением), Sort, Actions (`New Folder` / Tags / More `...`) и Search (`Ctrl+F`); баннер корзины `Empty`; нижний переключатель темы `Light Golden Gate` (по умолчанию) ↔ `Dark Purple Glass` (`--light` / `--dark`, `--path <dir>`, `--trash`, `--recents`).

3. **Маршрутизация в композиторе и лаунчере (`crates/rusty-compositor`, `apps/rusty-shell`):**
   - Удалены кандидаты `nautilus`, `dolphin`, `thunar`, `nemo`, `pcmanfm` из `launcher.rs`, `frame_composer.rs`, `state/shell_actions.rs`, `shell_render/top_bar.rs` и `apps/rusty-shell/src/lib.rs`.
   - Добавлено разрешение бинарника рядом с текущим исполняемым файлом композитора (`resolve_program_path_with`, `LauncherState::launch_program_with_args`) и маршрутизация `DockHit::PinnedFiles` → `rusty-files`, `DockHit::PinnedDownloads` → `rusty-files --path <Downloads>`, `DockHit::PinnedTrash` → `rusty-files --trash`, `DesktopOpenFiles` → `rusty-files --path <Desktop>`, `DownloadsOpenEntry` (для каталогов) → `rusty-files --path <dir>`. `xdg-open` оставлен только для открытия обычных файлов-документов.

## Последствия

- Операции ФС и UI файлового менеджера полностью вынесены в отдельный Wayland-процесс `rusty-files`, сохраняя `unsafe_code = "deny"` и нулевой простой event loop композитора.
- Все системные точки входа в работу с папками, `Downloads` и `Trash` единообразно запускают `rusty-files` даже при запуске из `target/debug` без установки в системный `$PATH`.
- Этап B (`RUSTYOS-066`: `Icons` grid + `Gallery` cover flow + `Quick Look` предпросмотр) опирается на готовые слоты toolbar и общую модель `FilesAppState` / `file_manager.rs`.
