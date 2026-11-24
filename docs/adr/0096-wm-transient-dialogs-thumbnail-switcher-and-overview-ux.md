# ADR-0096: Модальные и transient-диалоги, `Alt+Tab` с живыми GPU-миниатюрами и интерактивный `Workspace Overview` (`RUSTYOS-167`)

- **Status:** Accepted
- **Date:** 2026-11-24
- **Decision ID:** `D-110`
- **Related:** `RUSTYOS-167`, `RUSTYOS-166`, `RUSTYOS-165`, `RUSTYOS-132`, `ADR-0095`, `ADR-0094`, `ADR-0076`

## Контекст

После стабилизации ядра WM (`RUSTYOS-165`) и интерактивной эргономики (`RUSTYOS-166`) оставались три системных пробела пользовательского уровня по сравнению с Cutefish (`KWin`) и `DESIGN.md` (§12.3, §12.6):

1. В `rusty-wm::ManagedWindow` отсутствовали связи `parent`/`transient`/`modal`, из-за чего диалоги Wayland (`xdg_toplevel.set_parent`) и XWayland (`WM_TRANSIENT_FOR`, `WmWindowType::{Dialog, Utility, Splash}`) открывались в каскаде вместо центра родителя, уходили под родителя при клике, тайлились в `MasterStack` и отрывались при смене workspace/монитора.
2. `Alt+Tab` (`WindowSwitcher`) не переиспользовал уже существующий GPU-кэш миниатюр (`RUSTYOS-132`) и показывал только иконки приложений без превью окон, hover-подсветки, закрытия (`×` / СКМ / `Q`) и возврата исходного фокуса при `Esc`.
3. `Workspace Overview` не имел hover-подсветки карточек и пространств `Desktop 1..9`, кнопки закрытия `×`, перетаскивания карточки окна на другое пространство (`drag-to-workspace`) и клавиатурной навигации по сетке.

## Решение

Реализовать Фазу C по **Варианту 1** без новых внешних Cargo-зависимостей и без `unsafe`:

1. **Иерархия `parent`/`transient`/`modal` в `crates/rusty-wm`:**
   - Расширить `ManagedWindow` полями `parent: Option<WindowId>`, `is_dialog: bool`, `is_modal: bool` и предикатом `is_floating_transient()`.
   - Добавить `WindowManager::{set_window_parent, set_window_dialog_flags, center_window_on_parent, transient_descendants, active_modal_descendant, move_window_to_workspace}` с защитой от циклов.
   - Обеспечить инвариант Z-order (`enforce_transient_stacking_order`: дети всегда выше родителя), перенаправление фокуса на активного модального потомка в `focus_and_raise` / `cycle_focus`, сохранение floating-геометрии и скруглённых углов (`window_is_square_framed == false`) в `MasterStack`, совместный перенос при `move_window_to_workspace` / `migrate_window_to_output` и приоритетный возврат фокуса родителю в `unmap_window`.
2. **Wayland / XWayland интеграция:**
   - Реализовать `XdgShellHandler::parent_changed` и `sync_xdg_toplevel_parent_and_modal` (`ToplevelSurface::parent()` + `XdgToplevelSurfaceData::modal`).
   - Подключить `sync_x11_transient_and_dialog` (`X11Surface::is_transient_for()`, `WmWindowType::{Dialog, Utility, Splash}`) в `map_window_request` и `property_notify` (`WmWindowProperty::{TransientFor, WindowType}`).
3. **`Alt+Tab` Window Switcher с живыми GPU-миниатюрами:**
   - Обобщить `capture_overview_window_snapshots` -> `capture_window_thumbnails` и расширить `WindowMinimizePipeline::render_overview_thumbnail_element` / `has_lifecycle_snapshot` fallback-ом на `MinimizedWindowSnapshot` для свёрнутых окон.
   - Перевести `WindowSwitcher` на `16:10` карточки с живыми миниатюрами, `20×20` бейджем приложения, кнопкой `×` (`SwitcherHit::{Card, CloseCard, Outside}`), СКМ/`Q`/`Delete` закрытием (`KeyIntercept::SwitcherCloseSelected`), hover-подсветкой и восстановлением `previous_focus` при `Esc`.
4. **Интерактивный `Workspace Overview`:**
   - Добавить `OverviewHit::CloseCard(usize)`, `overview_card_close_rect`, `overview_grid_cols`, `OverviewCardDragState` и поля hover/selection/drag в `OverviewRenderState`.
   - Реализовать hover-подсветку карточек и `Desktop 1..9`, кнопку `×` / СКМ закрытия карточки, drag-to-workspace (перетаскивание карточки на пилюлю `Desktop N` с курсором `Grabbing` и совместным переносом дочерних диалогов) и клавиатурную навигацию (`Left`/`Right`/`Up`/`Down`/`Tab`/`Enter`/`Delete`/`Q`, `KeyIntercept::{OverviewMove, OverviewActivate, OverviewCloseSelected}`).

## Последствия

- **Плюсы:** полное соответствие ожиданиям desktop WM для диалогов, мгновенный `Alt+Tab` с живыми превью окон и полноценная интерактивность `Overview` без новых зависимостей, новых GLES-шейдеров и `unsafe`.
- **Проверки:** `12/12` тестов `rusty-wm`, `240/240` тестов `rusty-compositor`, `cargo fmt --check`, `cargo clippy -D warnings` и живые `--winit` Wayland Light/Dark прогоны `switcher` и `overview` с двумя реальными клиентами (`rusty-settings`, `rusty-files`) и 0 WARN/ERROR.
