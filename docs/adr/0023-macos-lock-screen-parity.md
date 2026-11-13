# ADR-0023: 1:1 macOS 27 Lock Screen Visual & Interactive Parity (`RUSTYOS-054`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

Предыдущий встроенный экран блокировки в `crates/rusty-compositor/src/shell_render.rs` рисовал 92%-ную тёмную сплошную завесу поверх скрытого рабочего стола и центрированную карточку `360×216` с заголовком `"RustyOS Session Locked"` и кнопкой `"Unlock Session"`. При сопоставлении с референсом macOS 27 требуется 1:1 макет без внешней рамки:

1. На экран блокировки должен выводиться полноэкранный фон текущих обоев рабочего стола (`wallpaper_element_with_alpha`), полностью перекрывающий скрытые окна (включая полноэкранные), `TopBar` и `Dock`.
2. В верхней центральной зоне поверх обоев должны располагаться короткая дата (`"Fri Nov 13"`) и крупные полупрозрачные frosted-часы без суффикса `AM`/`PM`.
3. В правом верхнем углу прямо на обоях должен выводиться минималистичный кластер статуса (`U.S.` + иконка клавиатуры, `Battery`, `Wi-Fi` и кнопка питания с поповером `Sleep` / `Restart...` / `Shut Down...`).
4. В нижней центральной части без тёмной карточки должен отображаться список локальных пользователей с круглыми аватарами и именами, с выбором пользователя и разблокировкой по клику / `Enter` / `Space`.

## Рассмотренные варианты

- **Вариант 1 (Выбран):**
  - В `crates/rusty-services/src/lib.rs` добавить `UserAccount`, `UserService` (парсинг `/etc/passwd` UID `1000..=60000` и поиск `~/.face` / `/var/lib/AccountsService/icons/`), `SessionAction::{Suspend, Reboot, PowerOff}`, `SessionService::dispatch` (`systemctl`) и форматтеры `local_clock_lock_date` / `local_clock_lock_hhmm`.
  - В `crates/rusty-compositor/src/telemetry.rs` расширить `SystemTelemetry` (`users`, `lock_date`, `lock_time_hhmm`, `lock_time_24h`) и добавить `SystemMetricsHandle::dispatch_session`.
  - В `crates/rusty-compositor/src/icons.rs` и `surfaces.rs` добавить `load_circular_avatar_bgra`, `UiShapeCache::wallpaper_element_with_alpha`, `UiShapeCache::user_avatar_element` и SDF-глифы `UiGlyphKind::{Keyboard, PowerSymbol}`.
  - В `crates/rusty-compositor/src/shell_render.rs`, `state.rs`, `input.rs` и `winit_backend.rs` заменить модал/завесу в `generate_lock_screen_elements` на макет macOS 27, добавить `LockScreenRenderState`, `LockScreenHit`, `hit_test_lock_screen`, `toggle_lock_power_menu`, `select_lock_user`, `KeyIntercept::LockSelectDelta` и `handle_lock_screen_pointer_click`, сохранив `ext_session_lock_v1` без изменений.
- **Вариант 2:** Встроить полноценную PAM-аутентификацию по паролю непосредственно внутри процесса `rusty-compositor` с полем ввода пароля. Потребует добавления внешних PAM-крейтов или небезопасного FFI (в проекте запрещён `unsafe`) и дублирует инфраструктуру внешнего `ext-session-lock-v1` локера.
- **Вариант 3:** Вынести весь UI экрана блокировки во внешний клиент `rusty-shell` через `ext_session_lock_v1`. Потребует усложнения IPC для рендера обоев/аватаров и меню питания и дублирует уже встроенный в композитор путь отрисовки `--drm`/`--winit`.

## Решение

Утверждён **Вариант 1** (`APPROVE: RUSTYOS-054 Вариант 1` от 2026-11-13).

## Последствия

- Встроенный Lock Screen композитора визуально и композиционно приведён к эталону macOS 27 (обои во весь экран поверх скрытых окон и панелей, крупный полупрозрачный шрифт часов с датой, верхне-правый минимальный статус с рабочим меню питания и переключением раскладки, нижний список пользователей с круглыми аватарами без тёмной обёртки).
- Публичные типы (`UserAccount`, `UserService`, `SessionAction`, `SessionService`, `LockScreenRenderState`, `LockScreenHit`, `UiGlyphKind::{Keyboard, PowerSymbol}`) и протокол `ext_session_lock_v1` расширены без новых внешних Rust-зависимостей и без `unsafe`.
