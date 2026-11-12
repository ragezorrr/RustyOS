# ADR-0009: Session Launcher, Desktop Entry & User-Space Installer (`RUSTYOS-014`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

Согласно `ROADMAP.md` (Фаза 10 --- Distribution and Installer):
- Цель: превратить RustyOS в полноценно запускаемую сессию рабочего стола Linux.
- Требования:
  1. Корректная регистрация в Display Manager через стандарт Wayland-сессий (`/usr/share/wayland-sessions` или `~/.local/share/wayland-sessions`).
  2. Экспорт переменных окружения (`XDG_CURRENT_DESKTOP=RustyOS`, `XDG_SESSION_TYPE=wayland`, `XDG_SESSION_DESKTOP=RustyOS`).
  3. Координированный запуск сессии: композитор `rusty-compositor` поднимает Wayland-сокет и IPC-сокет, после чего запускается независимая оболочка `rusty-shell`.
  4. Безопасный скрипт установки `scripts/install.sh`, работающий в пространстве пользователя (без необходимости прав root) и поддерживающий режим `--system`.

## Решение

Выбран **Вариант A**:
1. Лаунчер сессии `bin/rustyos-session`:
   - Настраивает переменные окружения XDG.
   - Запускает `rusty-compositor` (с флагами `--winit` или `--drm` по выбору) со спавном `rusty-shell`.
2. Файл сессии `packaging/wayland-sessions/rustyos.desktop`:
   - Содержит `Type=Application`, `DesktopNames=RustyOS`, `Exec=rustyos-session`.
3. Скрипт сборки и установки `scripts/install.sh`:
   - Собирает release-бинарники (`cargo build --release --workspace`).
   - Копирует бинарники в `~/.local/bin` (или `/usr/local/bin` с sudo), регистрирует десктоп-файл сессии.

## Последствия

- Завершается Фаза 10 базового плана `ROADMAP.md`.
- Пользователь может выбрать RustyOS в экране входа (GDM, SDDM, greetd, LightDM) или запустить команду `rustyos-session`.
