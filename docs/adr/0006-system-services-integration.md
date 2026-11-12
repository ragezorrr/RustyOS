# ADR-0006: Linux System Services Integration (`crates/rusty-services`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

Согласно `ROADMAP.md` (Фаза 7 --- System Integration), `ARCHITECTURE.md` (раздел 2, System Services) и `SECURITY.md`:
- RustyOS переиспользует стандартную Linux-инфраструктуру: PipeWire / ALSA, NetworkManager, UPower / sysfs, systemd/logind, BlueZ.
- Принципы:
  1. Не переписывать существующие системные сервисы.
  2. Запуск без прав root (least privilege).
  3. Корректная деградация (graceful fallback): отсутствие сервиса, сбой драйвера или отказ в доступе не должны приводить к падению или зависанию оболочки или композитора.

## Решение

Создать чистый `std`-only крейт `crates/rusty-services` с нативными sysfs/userspace адаптерами и безопасным агрегатором `ServiceRegistry`:
- `PowerService`: мониторинг батареи через `/sys/class/power_supply/` (`capacity`, `status`).
- `NetworkService`: мониторинг сети через `/sys/class/net/` и `nmcli`.
- `AudioService`: опрос громкости и mute через `wpctl` / `pactl`.
- `SessionService`: команды управления сессией (suspend, reboot, poweroff) через `logind`.
- Интеграция с `apps/rusty-shell` для отображения реального статуса оборудования в `TopBar` и `QuickSettings`.

## Последствия

- В workspace добавляется крейт `crates/rusty-services`.
- Оболочка получает реальные системные метрики хоста (аккумулятор, сеть, звук).
