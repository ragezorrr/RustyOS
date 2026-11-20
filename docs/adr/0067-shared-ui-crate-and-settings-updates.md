# 0067. Shared UI Crate and Settings Software Update Integration

- **Status:** accepted
- **Date:** 2026-11-20
- **Task:** RUSTYOS-123

## Context

В ходе развития приложений RustyOS (`rusty-settings`, `rusty-files`, `rusty-terminal`, `rusty-installer`, а также SSD-рендерера композитора) логика геометрии окон, расчёта размеров светофоров (14 px, центры 21/43/65 px), ховер-состояний, палитры тем и векторных микроглифов дублировалась в каждом приложении.

Кроме того, функционал проверки и установки системных обновлений исторически развивался в двух местах:
1. Отдельное приложение `apps/rusty-updater`.
2. Системные настройки `apps/rusty-settings`.

В современных десктопных окружениях Software Update является неотъемлемой частью системных настроек (`Settings -> Software Update`), а наличие отдельного вспомогательного приложения создавало избыточный бинарник, дублирование зависимостей и разрозненный пользовательский опыт.

## Decision

1. **Создание общего крейта `crates/rusty-ui`:**
   - Выделить стандартизированные оконные метрики (`WINDOW_RADIUS = 20`, `TITLEBAR_HEIGHT = 40`, `SSD_TITLEBAR_HEIGHT = 28`, `TRAFFIC_LIGHT_DIAMETER = 14`, `TRAFFIC_LIGHT_CENTERS_X = [21.0, 43.0, 65.0]`).
   - Реализовать общие структуры `TrafficLightButtons`, `TrafficLightKind`, `ThemePalette`, `WindowFrame`, `WindowHitTarget`.
   - Реализовать чистый CPU-холст `UiCanvas` с поддержкой субпиксельного антиалиасинга (`blend_pixel`, `draw_circle_aa`, `stroke_line_aa`, `fast_rounded_rect`, `draw_traffic_lights`).
   - Добавить крейт `rusty-ui` в корень `Cargo.toml` (`[workspace.members]` и `[workspace.dependencies]`).

2. **Интеграция Software Update в `apps/rusty-settings`:**
   - Добавить страницу `SettingsPage::SoftwareUpdate` (категория `System`, заголовок `Software Update`, CLI-параметр `--page updates`).
   - Подключить фоновый опрос обновлений через `rusty_services::updates::check_all_pending_updates` и интерактивный запуск привилегированного обновления через `UpdateTransactionHandle::start_system_upgrade`.
   - Отображать текущий статус, количество ожидающих пакетов, подробный прогресс фаз транзакции (`Synchronizing`, `Downloading`, `CheckingIntegrity`, `Installing`, `Complete`, `Failed`) и список пакетов с пометками о перезапуске.

3. **Полное удаление `apps/rusty-updater`:**
   - Полностью удалить директорию `apps/rusty-updater/` из репозитория и рабочего пространства `Cargo.toml`.
   - Обновить вызовы и лаунчеры в композиторе (`updates_popover.rs`, `shell_actions.rs`, `launcher.rs`, `dock.rs`) на запуск `rusty-settings --page updates`.
   - Обновить `bin/rustyos-session`, `packaging/arch/PKGBUILD`, `scripts/install.sh` и `packaging/applications/org.rustyos.updater.desktop`.

## Consequences

- Достигнута сквозная согласованность геометрии и поведения оконных светофоров через единую кодовую базу `crates/rusty-ui`.
- Устранён избыточный бинарник `rusty-updater`, упрощена упаковка и поддержка Arch-репозитория.
- Настройки системы получили полноценную встроенную страницу обновлений в едином стиле интерфейса.
