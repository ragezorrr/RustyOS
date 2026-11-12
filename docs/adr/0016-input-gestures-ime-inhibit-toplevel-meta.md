# ADR-0016: Desktop UX Wayland Protocols — Gestures, IME, Shortcut Inhibit, Foreign Toplevels & Toplevel Icons (`RUSTYOS-047`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

После закрытия инфраструктурных графических и клиентских протоколов (`RUSTYOS-044`--`RUSTYOS-046`) композитору не хватало пяти прикладных Wayland-протоколов повседневного desktop UX:
1. Навигационных touchpad-жестов (`zwp_pointer_gestures_v1`): swipe/pinch/hold события `smithay::backend::input::InputEvent::Gesture*` не маршрутизировались в `PointerHandle`, и отсутствовали 3-пальцевые свайпы для переключения рабочих столов и открытия Overview.
2. Полноценного ввода текста через IME (`zwp_text_input_v3` + `zwp_input_method_v2`): Wayland-клиенты и виртуальные клавиатуры/IME-демоны не могли обмениваться preedit/commit состоянием и позиционировать всплывающие окна кандидатов.
3. Управляемого перехвата шорткатов (`zwp_keyboard_shortcuts_inhibit_manager_v1`): полноэкранные VM/RDP/игры и IDE не могли временно перехватывать комбинации клавиш у композитора при сохранении аварийных системных гарантий.
4. Метаданных окон для shell/панелей (`ext_foreign_toplevel_list_v1`): внешние клиенты и инфраструктура панели не получали единый поток создания, обновления заголовка/app_id и закрытия Wayland и XWayland окон.
5. Иконок верхнеуровневых окон (`xdg_toplevel_icon_v1`): Dock и `Alt+Tab` switcher опирались только на тему иконок / текстовые бейджи и не отображали SHM-буферы иконок, переданные клиентами напрямую.

## Рассмотренные варианты

- **Вариант A (Выбран):**
  - В `state.rs` зарегистрировать все 6 Smithay-глобалов в `RustyCompositorState` (`pointer_gestures_state`, `text_input_manager_state`, `input_method_manager_state`, `keyboard_shortcuts_inhibit_state`, `foreign_toplevel_list_state`, `xdg_toplevel_icon_manager` с размерами `[16, 24, 32, 48, 64]`) без добавления новых внешних крейтов и без изменений `rusty-ipc` / `apps/rusty-shell`.
  - В `input.rs` и `state.rs` маршрутизировать все 8 вариантов `InputEvent::Gesture*`, реализовать `TouchpadSwipeState` и 3-пальцевые свайпы (рабочие столы `WorkspaceId - 1 / + 1` и Overview) с изоляцией на экране блокировки; для `zwp_keyboard_shortcuts_inhibit_manager_v1` пропускать шорткаты сфокусированному ингибирующему клиенту, оставляя неперехватываемыми `Ctrl+Alt+F1..F12`, lock-screen изоляцию и аварийный выход `Super+Escape` (`KeyIntercept::ReleaseShortcutInhibitor` с отслеживанием `suppressed_inhibitor_surface`).
  - Для `zwp_text_input_v3` + `zwp_input_method_v2` реализовать `InputMethodHandler`, вычисление позиции `compute_ime_popup_origin`, выбор видимых `ime_popups_for_output` и рендер `PopupSurface` в `winit_backend.rs` (`push_ime_popup_elements`) и отправку frame-callbacks/очистку в `winit_backend.rs` и `drm_backend.rs`.
  - Для `ext_foreign_toplevel_list_v1` поддерживать единый жизненный цикл `ForeignToplevelHandle` для Wayland (`state.rs`) и XWayland (`xwayland.rs`), синхронизируя `title`/`app_id` и закрывая handle при снятии окна.
  - Для `xdg_toplevel_icon_v1` в `icons.rs` без `unsafe` читать `ToplevelIconCachedState` (`icon_name` и SHM-буферы через `GlesRenderer::import_shm_buffer` -> `TextureBuffer<GlesTexture>` -> `TextureRenderElement`) и отображать их в Dock и `Alt+Tab` switcher (`shell_render.rs`, `winit_backend.rs`).
- **Вариант B:** Ограничиться регистрацией глобалов и текстовых метаданных (`icon_name`), отложив SHM-рендер иконок, IME-попапы и touchpad-жесты на последующие этапы.
- **Вариант C:** Вариант A + одновременное расширение `rusty-ipc` / `apps/rusty-shell` для проброса иконок и списка окон во внешний shell-процесс в одном PR (превышает скоуп чисто композиторного этапа).

## Решение

Утверждён **Вариант A** (`APPROVE:` пользователя от 2026-11-12).

## Последствия

- Композитор экспортирует `zwp_pointer_gestures_v1`, `zwp_text_input_v3`, `zwp_input_method_v2`, `zwp_keyboard_shortcuts_inhibit_manager_v1`, `ext_foreign_toplevel_list_v1` и `xdg_toplevel_icon_v1` полностью в пределах существующих зависимостей workspace и без единого блока `unsafe`.
- Все системные инварианты ввода (`Ctrl+Alt+F1..F12`, `ext_session_lock_v1` / `screen_locked`, аварийное снятие ингибитора `Super+Escape`) гарантируются в `classify_keyboard_input` и при свайпах.
- Живое E2E-тестирование с реальным физическим тачпадом libinput, IME-демоном (IBus/Fcitx5), клиентами с `zwp_keyboard_shortcuts_inhibit` и Wayland-приложениями с `xdg_toplevel_icon_v1` SHM-иконками требует запуска на живой Wayland/DRM сессии.
