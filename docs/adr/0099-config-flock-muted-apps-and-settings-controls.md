# ADR-0099: Межпроцессная блокировка `config.toml` (`flock`), сквозной `muted_apps` и интерактивные контролы `rusty-settings` (`RUSTYOS-170`)

- **Статус:** Accepted
- **Дата:** 2026-11-24
- **Решение:** `D-113`
- **Задача:** `RUSTYOS-170` (Фаза 3 полного аудита системы)

## Контекст

Заключительная фаза аудита закрывает три группы дефектов конфигурации и системных настроек:

1. **Конкурентная запись `config.toml`:** `rusty-compositor` и `rusty-settings` записывали `~/.config/rustyos/config.toml` без межпроцессной блокировки, а строковый патчер `format_toml_value` оборачивал TOML-массивы в кавычки.
2. **Недоделанный `[notifications].muted_apps`:** переключение приложений в `rusty-settings` (`RowControlId::NotifAppToggle`) меняло только состояние в памяти и не сохранялось в `config.toml`, а композитор не фильтровал toast-баннеры по `muted_apps`.
3. **Интерактивные контролы `rusty-settings`:** слайдеры (`Volume`, `Scale`, `GlassOpacity`) не поддерживали плавное перетаскивание (`pointer_drag`), модалка `ShortcutEdit` требовала ручного ввода текста вместо захвата клавиш, а страница `Display` не позволяла выбирать конкретный монитор (`[display.<name>]`).

## Решение

1. **Атомарная запись `config.toml` с `flock` (`crates/rusty-services/src/lib.rs`):**
   - Добавлены безопасные (0 `unsafe`, без новых крейтов) функции `with_config_file_lock` (использует `std::fs::File::lock()` на `.config.toml.lock`) и `write_file_atomic` (`.tmp.<pid>` + `sync_all()` + `rename()`).
   - `UserConfig::save_to_path` (`crates/rusty-compositor/src/config.rs`) и `update_config_entries` (`apps/rusty-settings/src/state.rs`) переведены на `with_config_file_lock` + `write_file_atomic`.
   - В `format_toml_value` добавлена поддержка TOML-массивов (`[...]`) и корректное экранирование управляющих символов; добавлен `format_toml_string_array`.
2. **Сквозной `[notifications].muted_apps`:**
   - В `UserConfig` добавлено поле `notifications_muted_apps: Vec<String>`, парсинг/сериализация `muted_apps` и метод `is_notification_app_muted(&str)`.
   - В `CompositorState::push_dbus_notification_for_app` (`crates/rusty-compositor/src/state/shell_actions.rs`) при `!notifications_allow` или совпадении `app_key` с `muted_apps` подавляется всплывающий toast-баннер (запись сохраняется в `notification_history`).
   - В `apps/rusty-settings/src/state.rs` `RowControlId::NotifAppToggle` сохраняет `[notifications].muted_apps`, а `apply_toml_kv` синхронизирует `notif_app_enabled` из `config.toml`.
3. **Интерактивные контролы `rusty-settings`:**
   - В `apps/rusty-settings/src/wayland.rs` добавлен `StepperDragKind::{Volume, Scale, GlassOpacity}` с `update_stepper_drag(px)` для перетаскивания слайдеров.
   - В `handle_key` для `SettingsModal::ShortcutEdit` добавлен прямой захват комбинаций клавиш (`format_captured_shortcut` + `apply_captured_shortcut`) с сохранением ручного ввода в `TextField` как fallback.
   - В `SettingsPage::Display` добавлен селектор `Active Monitor` (`RowControlId::DisplayOutputCycle`), синхронизируемый из Wayland `OutputState` (`sync_wayland_outputs`), с записью per-output настроек в `[display.<name>]`.

## Последствия

- **Плюсы:** устранены гонки/lost-update при записи `config.toml`, работает сквозное отключение баннеров по приложениям, улучшена эргономика `rusty-settings` при нулевых новых зависимостях и `0 unsafe`.
- **Риски:** низкие; формат `config.toml` полностью обратно совместим.
