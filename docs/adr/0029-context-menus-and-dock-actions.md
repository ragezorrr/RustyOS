# ADR-0029: Контекстные меню (ПКМ) на Desktop и Dock (`ShellOverlayView::ContextMenu`) и опция `[dock]`

- **Статус:** accepted
- **Дата:** 2026-11-14
- **Автор:** AI Agent + User
- **Связанные задачи:** RUSTYOS-064 (развивает ADR-0019, ADR-0020, ADR-0027, ADR-0028)

## Контекст

1. Правый клик (ПКМ, `button == 273`) по пустому рабочему столу (на основном или дополнительном мониторе) и по слотам Dock не имел встроенной обработки контекстного меню в стиле macOS, что оставляло пользователя без привычных быстрых действий (создание папок на Desktop, смена обоев/настроек экрана, управление автозапуском приложений в Dock, `Minimize` / `Move to Desktop >` / `Quit` / `Alt → Force Quit`, просмотр и сортировка `Downloads`, а также `Open Trash` / `Empty Trash...`).
2. Открытие всплывающих меню на разных мониторах, вложенных каскадных подменю (`>`), а также потенциально длительная очистка корзины (`~/.local/share/Trash`) не должны блокировать compositor event loop и не должны требовать новых внешних зависимостей или `unsafe`-кода.

## Решение

1. **Единая подсистема `ShellOverlayView::ContextMenu` (`crates/rusty-compositor/src/shell_render/context_menu.rs`, `mod.rs`, `surfaces/glyphs.rs`):**
   - Расширен набор монохромных SDF-иконок (`UiGlyphKind::{Plus, Check, ChevronRight, Trash}`).
   - Реализована модель `ContextMenuState` (`ContextMenuTarget::{Desktop, DockSlot(DockHit)}`, `ContextMenuEntry::{Item, Separator}`, `ContextMenuAction`) и мультимониторный расчёт геометрии `compute_context_menu_layout` с clamping внутри границ целевого монитора (на primary сохраняется резерв под 24 px `TopBar`, на secondary меню свободно располагается в пределах `[8..w-8, 8..h-8]`) и каскадными подменю (`>`) с положительным зазором 4 px и автопереворотом влево у правого края экрана.
   - Отрисовка выполнена на базе Liquid Glass `Popover` с радиусом 16 px (`MENU_POPOVER_RADIUS`), разделителями 1 px, галочками активных пунктов, подсказками справа, dangerously-красной подсветкой `Force Quit` / `Empty Trash`, полным управлением мышью и клавиатурой (`Up`/`Down`/`Left`/`Right`/`Return`/`Escape`) и корректной интеграцией в `frame_composer.rs` (для primary и secondary выходов).

2. **Контекстные меню Desktop и Dock (`crates/rusty-compositor/src/state/context_menu_actions.rs`, `input/{mod.rs,keyboard.rs,pointer.rs,shell_pointer.rs}`):**
   - **Desktop ПКМ (`button == 273` вне окон, TopBar, слоёв и fullscreen-окон):** меню с пунктами `New Folder` (создание уникальной `New Folder` / `New Folder N` в `$XDG_DESKTOP_DIR` или `~/Desktop` с OSD-уведомлением), `Open Desktop in Files`, `Change Wallpaper...` (открывает System Settings → Appearance), `Display Settings...` (Displays), `Open Terminal`, `Mission Control`, `Tile Windows` (`Alt+L` с галочкой текущего режима `MasterStack`/`Floating`).
   - **Dock ПКМ (`is_on_primary` guard, все слоты):**
     - `PinnedFiles` / `PinnedTerminal` / `PinnedBrowser` / `PinnedNotes` / `PinnedSettings`: `New ... Window` / `Open System Settings`, `Options > Open at Login` (синхронизируется с `UserConfig::autostart` и сохраняется атомарно в `config.toml`), при наличии запущенных окон — `Show All Windows` и динамический `Quit` ↔ `Force Quit` при зажатом `Alt`.
     - `ToggleOverview`: `Mission Control` / `Exit Mission Control` и `Switch Desktop > Desktop 1..9`.
     - `WindowSlot(idx)`: `Focus`/`Show`/`Restore Window`, `Minimize`, `Move to Desktop > Desktop 1..9`, и динамический `Quit` ↔ `Force Quit` (`Client::get_credentials` / `X11Surface::pid` с защитой от `pid <= 1` и собственного PID композитора).
     - `PinnedDownloads`: до 8 файлов/папок из `$XDG_DOWNLOAD_DIR` с размером, `Sort By > {Date Modified, Name, Size}` (перестраивает открытое меню на лету и сохраняется в `[dock].downloads_sort`) и `Open "Downloads"`.
     - `PinnedTrash`: `Open Trash` и `Empty Trash... (<count>)` с проверкой имени корневого каталога (`Trash`), `symlink_metadata` (удаление симлинков без следования по цели) и вынесением удаления на фоновый `std::thread::spawn` + `mpsc` канал, опрашиваемый неблокирующим `drain_trash_empty_results()` в `frame_composer.rs`.

3. **Опциональная секция `[dock]` в `UserConfig` (`crates/rusty-compositor/src/config.rs`):**
   - Добавлено перечисление `DownloadsSortMode::{DateModified, Name, Size}` и поле `UserConfig::downloads_sort` (секция `[dock]`, ключ `downloads_sort`, значение по умолчанию `"date_modified"`). Парсинг и сериализация сохраняют полную обратную совместимость с существующими `config.toml`.

## Последствия

- Десктоп и все слоты Dock получают единообразное macOS-совместимое контекстное меню поверх единого Liquid Glass renderer, работающее на всех мониторах и при любом масштабе/ориентации без внешних GTK/Qt-зависимостей и без `unsafe`.
- Очистка корзины и операции файловой системы выполняются безопасно и не замедляют презентацию кадров.
- Появилась документированная опциональная секция `[dock]` в `config.toml` с покрытием roundtrip-тестами.
