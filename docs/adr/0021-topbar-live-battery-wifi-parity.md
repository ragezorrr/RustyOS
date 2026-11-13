# ADR-0021: 1:1 macOS TopBar Live Wi-Fi Signal (3 Bands) & AC Battery Lightning Cutout (`RUSTYOS-052`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

По результатам визуального сравнения `TopBar` с референсом macOS 27 ("Golden Gate"):
1. Предыдущая иконка Wi-Fi (`sample_wifi_glyph_uv`) содержала 4 элемента (якорная точка + 3 дуги) и принимала только флаг `connected: bool`, тогда как на macOS иконка Wi-Fi состоит ровно из **3 полос** (нижний якорный элемент + средняя дуга + верхняя дуга) и динамически отображает силу сигнала.
2. При подключённом питании на `99–100%` (`ACAD online=1`, `BAT1 Full 100%`) прорезная молния на батарее скрывалась условием `t.ac_online && t.battery_percent < 99`, а в `PowerService` порядок обхода sysfs мог допустить перезапись активного `Mains`-адаптера неактивным USB-PD портом; сама 3-сегментная молния была чрезмерно тонкой на фоне заполненной белой капсулы.

## Рассмотренные варианты

- **Вариант A (Выбран):**
  - Расширить публичные структуры и API: `NetworkStatus` (`wifi_signal_percent: Option<u8>`, `wifi_signal_bars`, `parse_proc_net_wireless_signal`, `format_nm_network_status_with_signal` в `crates/rusty-services`), `TopBarTelemetry` / `QuickSettingsRenderState` (`wifi_signal_percent`, `battery_charging` в `crates/rusty-compositor/src/shell_render.rs`), `UiGlyphKind::Wifi { connected, signal_percent }` (`surfaces.rs`) и `TopBarState` (`ac_online`, `battery_charging`, `wifi_signal_percent` в `apps/rusty-shell/src/lib.rs`).
  - Реализовать чтение живого сигнала Wi-Fi через NetworkManager D-Bus (`PrimaryConnection -> Devices -> Wireless -> ActiveAccessPoint.Strength`) с fallback на `/proc/net/wireless` и `nmcli`, а в `PowerService` агрегировать `ACAD`/`Mains`/USB-PD и `BatteryState::Charging` без перетирания `Mains`.
  - Привести `sample_wifi_glyph_uv` к 1:1 3-полосному глифу (нижняя капля + 2 концентрические дуги с приглушёнными неактивными верхними полосами по уровню сигнала и диагональным перечёркиванием при отключении).
  - Снять ограничение `< 99` (`charging: t.ac_online || t.battery_charging`) и обновить `sample_battery_glyph_uv` прорезной 2-клиновой молнией macOS и живым пропорциональным заполнением `5..=100%`.
- **Вариант B:** Ограничиться статичной геометрией без чтения уровня сигнала Wi-Fi и исправления sysfs AC-детекции. Не обеспечивает живую индикацию.
- **Вариант C:** Добавить внешние клиентские крейты `upower`/`networkmanager` вместо прямых D-Bus/sysfs адаптеров. Излишне увеличивает дерево зависимостей при наличии `zbus` и `/sys/class/power_supply`.

## Решение

Утверждён **Вариант A** (`APPROVE: RUSTYOS-052 Вариант A` от 2026-11-13).

## Последствия

- Иконки `Battery` и `Wi-Fi` в `TopBar` и `Control Center` визуально совпадают с эталоном macOS 27 (3 полосы Wi-Fi, чёткая прорезная молния зарядки при подключённом питании включая `99–100%`, живой уровень сигнала и заряда).
- Расширены публичные структуры `NetworkStatus`, `TopBarTelemetry`, `QuickSettingsRenderState`, `UiGlyphKind::Wifi` и `TopBarState` без новых внешних зависимостей и `unsafe`.
