# 0075. System Settings (`Bluetooth`, `Battery & Power`, `About`, `Users`, `Shortcuts`), расширение `rusty-services` и секции `[power]`/`[shortcuts]`

- **Status:** accepted
- **Date:** 2026-11-20
- **Task:** RUSTYOS-131

## Context

После перевода клиентского UI на унифицированный тулкит `crates/rusty-ui` (`RUSTYOS-130`) в `apps/rusty-settings` оставались непокрытыми ключевые разделы `Core UX`: управление устройствами **Bluetooth**, **Battery & Power** (профили питания, здоровье батареи, энергосбережение и авто-блокировка), сводка **About**, учётная запись **Users** и пользовательская настройка системных сочетаний **Shortcuts**. При этом `BluetoothService` умел только читать/переключать питание адаптера (`query_status`/`set_powered`), `PowerService` возвращал только базовый `PowerStatus`, а `UserConfig` (`config.toml`) не содержал секций `[power]` и `[shortcuts]`.

## Decision

1. **Расширение публичных контрактов `crates/rusty-services` (обратно совместимо):**
   - **`connectivity.rs` (`BluetoothService`):** добавлены структуры `BluetoothDevice` (адрес, имя, `paired`/`connected`/`trusted`, `battery_percent` из `org.bluez.Battery1`) и `BluetoothSnapshot` (`status`, `adapter_name`, `adapter_address`, `discovering`, `devices`), а также методы `query_snapshot`, `list_devices`, `set_discovering`, `connect_device`, `disconnect_device`, `pair_device`, `remove_device` поверх `org.bluez` System D-Bus с fallback на `bluetoothctl`. Все MAC-адреса валидируются через `normalize_bt_address` до вызова D-Bus/CLI.
   - **`power.rs` (`PowerService`):** добавлены `PowerProfile` (`LowPower`, `Balanced`, `Performance`), `BatteryCondition` (`Normal`, `ServiceRecommended`, `Unknown`), `PowerDetails` и методы `query_details`, `query_sysfs_details`, `set_power_profile` (`org.freedesktop.UPower.PowerProfiles` / `net.hadess.PowerProfiles` + fallback `powerprofilesctl` с честной деградацией при отсутствии демона). При чтении `/sys/class/power_supply` фильтр `is_system_battery` исключает периферийные батареи (`scope=Device`, `hidpp_battery_*`), чтобы беспроводные аксессуары не искажали системный статус зарядки.
   - **`display_session.rs` (`UserService`):** добавлены read-only структура `CurrentUserDetails`, парсер `parse_etc_group_for_user` и метод `query_current_user_details` (имя, UID/GID, home, shell, роль `Administrator`/`Standard`, группы, тип сессии) без модификации `/etc/passwd`/`/etc/shadow` и без запуска привилегированных операций.

2. **Обратно-совместимые секции `[power]` и `[shortcuts]` в `crates/rusty-compositor`:**
   - В `UserConfig` (`config.rs`) добавлены `[power]` (`low_power_mode: LowPowerMode`, `display_sleep_minutes`, `auto_lock_minutes`, `dim_on_battery`) и `[shortcuts]` (`ShortcutBindings`, 17 настраиваемых глобальных и WM-сочетаний с парсером `parse_shortcut_combo`/`ShortcutCombo`) с сохранением прежних значений по умолчанию и всех safety/VT-инвариантов (`Ctrl+Alt+BackSpace`, `Escape`, `VT switch`, hardware media/screenshot, `Super+Escape`).
   - В `TelemetryCmd::SetPowerProfile` (`telemetry.rs`) и `RustyCompositorState` (`apply_user_config`, `last_user_activity`, `check_idle_auto_lock`) изменения `[power]` применяются асинхронно без блокировки event loop; в `input/keyboard.rs` функция `match_shortcut_with_config` применяет пользовательские биндинги после `RELOAD_CONFIG`.

3. **Новые разделы и контролы `crates/rusty-ui` в `apps/rusty-settings`:**
   - В `SettingsPage` добавлены `Bluetooth`, `About`, `Battery`, `Users` и `Shortcuts` (также доступный ссылкой `Keyboard Shortcuts...` со страницы `Keyboard`), общим итогом 17 пунктов сайдбара.
   - Интерактивные виджеты `crates/rusty-ui`: `DropdownSelect` (`Low Power Mode`), `SegmentedTabs` (`Usage & Supply` / `Health & Power`), `ToggleSwitch::paint_animated`, `PushButton::paint_labeled` и модальный слой `SettingsModal` (`DialogSheet::confirm`/`DialogSheet::prompt` + `TextField` с `WaylandClipboardBridge`) для `(i)` сведений/удаления Bluetooth-устройств, деталей `Battery & Power Options`, `More Info...` в `About` и редактирования хоткеев с проверкой конфликтов/зарезервированных комбинаций.
   - Граница безопасности для `Users`: страница `Users` намеренно ограничена непривилегированным read-only обзором; создание/удаление пользователей и смена паролей через `org.freedesktop.Accounts` D-Bus и Polkit вынесены в отдельную последующую задачу.

## Consequences

- `rusty-settings` закрывает базовые системные сценарии Bluetooth, батареи/энергопрофилей, сведений об оборудовании, обзора учётной записи и переназначения шорткатов композитора с живым применением через `v1|RELOAD_CONFIG`.
- Расширения `rusty-services` и `UserConfig` обратно совместимы со всеми существующими потребителями и тестами (`BluetoothStatus`, `PowerStatus`, `config.toml` без новых секций).
- Все операции D-Bus/CLI выполняются в фоновом потоке (`ServiceCommand`/`TelemetryCmd`), а системные батареи корректно изолированы от периферийных HID-устройств (`scope=Device`).
