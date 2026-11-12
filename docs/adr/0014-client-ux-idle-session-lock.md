# ADR-0014: Клиентские протоколы UX, Idle и Session Lock (`wp_cursor_shape_v1`, `xdg_activation_v1`, `ext_idle_notify_v1`, `zwp_idle_inhibit_manager_v1`, `ext_session_lock_v1`) (`RUSTYOS-045`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

Композитор уже предоставлял Wayland/XWayland-окнам дробное масштабирование (`wp_fractional_scale_v1`, `wp_viewporter`), relative pointer, pointer constraints и `wp_presentation`, однако отсутствовали пять стандартных протоколов повседневного клиентского UX и энергосбережения:
1. `wp_cursor_shape_v1` — современные GTK4/Qt6/Firefox/Electron клиенты используют именованные формы курсора вместо ручных XCursor-буферов (`CursorShapeManagerState` + `TabletSeatHandler`).
2. `xdg_activation_v1` — безопасная передача фокуса по временному токену с проверкой TTL (10 с).
3. `ext_idle_notify_v1` + `zwp_idle_inhibit_manager_v1` — уведомление демонов простоя (`swayidle`/`hypridle`) при вводе пользователя и учёт активных surface-ингибиторов (`IdleNotifierState` + `IdleInhibitManagerState`, для чего требуется `LoopHandle` в `RustyCompositorState::new`).
4. `ext_session_lock_v1` — безопасная внешняя блокировка сессии (`swaylock`, `gtklock`) с изолированным рендерингом per-output `LockSurface` поверх чёрного фона и подтверждением `SessionLocker::lock()` после представления кадра.

## Рассмотренные варианты

- **Вариант A (Выбран):** Зарегистрировать все пять встроенных протоколов Smithay `0.7.0` в `state.rs` (`CursorShapeManagerState` + `TabletSeatHandler`, `XdgActivationState`/`XdgActivationHandler`, `IdleNotifierState`/`IdleNotifierHandler`, `IdleInhibitManagerState`/`IdleInhibitHandler`, `SessionLockManagerState`/`SessionLockHandler`), передать `LoopHandle` в `RustyCompositorState::new`, подключить `notify_user_activity()` и изоляцию ввода `ext_session_locked` в `input.rs` и `xwayland.rs`, а также рендер `LockSurface` + чёрный fallback и `confirm_pending_session_lock()` в `winit_backend.rs` / `drm_backend.rs`.
- **Вариант B:** Только `wp_cursor_shape_v1` + `xdg_activation_v1` + `zwp_idle_inhibit_manager_v1` без `ext_idle_notify_v1` и `ext_session_lock_v1` (без изменения `RustyCompositorState::new`, но клиенты простоев и сторонние локеры не работают).
- **Вариант C:** Вариант A + встроенный планировщик DPMS/auto-lock в `[idle]` `config.rs` (избыточный объём и риск конфликта с `swayidle`/`logind`).

## Решение

Утверждён **Вариант A** (`APPROVE:` пользователя от 2026-11-12).

## Последствия

- Клиенты получают стандартные формы курсоров (`wp_cursor_shape_v1`), безопасную активацию окон с ограничением TTL (`10` с) и согласованный цикл `ext_idle_notify_v1` ↔ `zwp_idle_inhibit_manager_v1`.
- При активном `ext_session_lock_v1` пользовательские окна, TopBar, Dock, Launcher, Overview, уведомления, XWayland-буфер обмена и pointer constraints полностью изолируются, а отображаются только программный курсор и `LockSurface` соответствующего выхода с чёрным fallback.
- Живая проверка со сторонними клиентами (`swaylock`, `swayidle`, GTK4/Firefox) и bare-metal `--drm` требует отдельного запуска на хост-сессии.
