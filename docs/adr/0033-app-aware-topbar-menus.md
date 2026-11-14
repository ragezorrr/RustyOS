# ADR-0033: Динамическое app-aware меню TopBar по профилю активного окна

- **Статус:** accepted
- **Дата:** 2026-11-14
- **Задача:** `RUSTYOS-068`
- **Связанные решения:** `D-041`, `ADR-0023`, `ADR-0028`, `ADR-0032`

## Контекст

После введения аутентичного `Menu Bar` (`RUSTYOS-067`) открытие стороннего приложения (например, `Firefox`) меняло только жирный заголовок активного приложения в `TopBar`, тогда как категории (`File`, `Edit`, `View`, `Go`, `Window`, `Help`) и содержимое всех выпадающих меню оставались жёстко привязаны к `Files` (`About RustyOS`, `New Files Window`, `New Desktop Folder`, `Go -> Desktop/Downloads/Trash`). Для браузеров, терминалов и редакторов меню `TopBar` не соответствовало активному окну.

## Решение

1. **Профильная классификация активного окна (`top_bar_menus.rs`):**
   - Введён `TopBarAppProfile::{Files, Browser, Terminal, Editor, GenericApp}` и классификатор `classify_top_bar_app(app_id, title)`.
   - Композитор (`RustyCompositorState::active_top_bar_app_context`) передаёт в классификатор Wayland `app_id` / XWayland `WM_CLASS` (`self.window_app_id(id)`) и заголовок (`self.window_title(id)`), возвращая каноническое имя (`Firefox`, `Konsole`, `VS Code`, `Files` и др.) и профиль. При фокусе на рабочем столе (`desktop_focused`) или отсутствии видимого сфокусированного окна активным профилем остаётся `("Files", Files)`.
2. **Динамические категории `TopBar` и пересчёт координат (`top_bar.rs`, `shell_render/mod.rs`):**
   - `TopBarTelemetry` дополнен полями `active_app_name` и `app_profile`, а `TopBarLeftLayout` — признаком `has_go: bool` (`TopBarAppProfile::has_go_menu()`).
   - Категория `Go` отображается только для профиля `Files`. Для сторонних приложений (`Browser`, `Terminal`, `Editor`, `GenericApp`) категория `Go` скрывается, `Window`/`Help` и правее расположенные workspace/layout-индикаторы смещаются влево, а hit-testing (`*_for_profile`) и `toggle_top_bar_menu` не допускают открытия или сохранения устаревшего меню `Go`.
3. **Адаптированное содержимое `<App>`, `File`, `Edit`, `View` и синтетические шорткаты (`input/keyboard.rs`):**
   - Пункты меню `<App>` используют реальное имя активного приложения (`About <App>`, `System Settings...`, `Minimize <App>`, `Quit <App>`, `Force Quit <App>`, плюс `Empty Trash...` только в `Files`).
   - `Browser`, `Terminal`, `Editor` и `GenericApp` получают релевантные команды `File`, `Edit`, `View` (включая `New Tab`, `Open Location...`, `Save Page As...`, `Close Tab`, `Copy`/`Paste`/`Find`, `Reload Page`, `Zoom In/Out/Actual Size`, `Toggle Fullscreen` и терминальные `Ctrl+Shift+C/V/F/T/W`).
   - `RustyCompositorState::dispatch_app_shortcut` безопасно (без `unsafe`) ищет keycode целевого keysym и владеющую раскладку через `KeyboardHandle::with_xkb_state` и `Xkb::raw_syms_for_key_in_layout`, временно переключает XKB-раскладку при нелатинской активной раскладке (например, `ru`), отправляет chord через `input_intercept` + `input_forward` с учётом `pressed_keys()` и восстанавливает исходную раскладку.

## Последствия

- Переключение между `Files`, браузерами, терминалами, редакторами и прочими приложениями обновляет и заголовок, и категории/команды `TopBar` без внедрения DBus-протоколов экспорта меню на текущем этапе.
- Классификатор и таблица профилей локализованы в `crates/rusty-compositor/src/shell_render/top_bar_menus.rs`, не требуют новых зависимостей и сохраняют обратную совместимость существующих `Files`-хелперов.
