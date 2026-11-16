# ADR-0040: Нативная Wayland-сессия, системный запуск из Display Manager и окружение клиентов

- **Статус:** accepted
- **Дата:** 2026-11-16
- **Задача:** `RUSTYOS-084`
- **Связанные решения:** `D-048`, `D-046`, `D-045`, `ADR-0038`, `ADR-0009`

## Контекст

При развёртывании сессии RustyOS через системные менеджеры входа (SDDM, GDM, LightDM, greetd) выявились три ключевых пробела:
1. **Отсутствие переменных окружения для GUI-тулкитов**: приложения на базе Qt, GTK, Electron (Chromium, VS Code, Slack, Obsidian), Mozilla (Firefox) и SDL по умолчанию не получали информацию о нативном бэкенде Wayland и стартовали через XWayland. Это приводило к размытию шрифтов и интерфейсов при HiDPI-масштабировании, отсутствию тачпад-жестов и дополнительному потреблению памяти.
2. **Отсутствие интеграции с целями `systemd --user`**: при входе и последующем завершении сессии (`Log Out...` в меню) не активировались и не останавливались стандартные цели `graphical-session.target` и специфичные для сессии цели `rustyos-session.target`. Из-за этого сессионные демоны пользователя (порталы, треи, медиа-сервисы) могли оставаться активными между входами разных пользователей.
3. **Отсутствие безопасного перехвата сигналов при завершении сессии**: прямой запуск через `exec` в shell-скрипте не позволял выполнить graceful shutdown и остановку фоновых сервисов сессии при остановке композитора.

## Решение

1. **Экспорт эталонного окружения тулкитов для Wayland (`bin/rustyos-session`):**
   - Установлены переменные по умолчанию с сохранением пользовательских переопределений:
     - `QT_QPA_PLATFORM="wayland;xcb"` и `QT_WAYLAND_DISABLE_WINDOWDECORATION="1"`;
     - `GDK_BACKEND="wayland,x11,*"`;
     - `MOZ_ENABLE_WAYLAND="1"`;
     - `ELECTRON_OZONE_PLATFORM_HINT="auto"`;
     - `SDL_VIDEODRIVER="wayland,x11"`;
     - `CLUTTER_BACKEND="wayland"`;
     - `_JAVA_AWT_WM_NONREPARENTING="1"`.
   - В автономном режиме (`--drm` / `--kms`) эти переменные синхронизируются в `dbus-update-activation-environment --systemd` и `systemctl --user import-environment`.

2. **Интеграция с `systemd --user` (Systemd Graphical Session Targets):**
   - Добавлены файлы целей:
     - `packaging/systemd/user/rustyos-session.target`: `BindsTo=graphical-session.target`, `Wants=graphical-session-pre.target`, `After=graphical-session-pre.target`;
     - `packaging/systemd/user/rustyos-session-shutdown.target`: `Conflicts=graphical-session.target ...`, `After=graphical-session.target ...`.
   - При старте автономной сессии активируется `rustyos-session.target`.
   - Настроен перехват сигналов завершения (`trap cleanup_session EXIT INT TERM HUP`), который при выходе из композитора останавливает `rustyos-session.target` и `graphical-session.target`, переводя сервисы в `rustyos-session-shutdown.target`.

3. **Обновление упаковки и скрипта установки:**
   - В `packaging/arch/PKGBUILD` добавлена установка юнитов в `/usr/lib/systemd/user/`;
   - В `scripts/install.sh` добавлена установка в `/usr/lib/systemd/user/` (root) или `${XDG_DATA_HOME:-$HOME/.local/share}/systemd/user/` (user mode);
   - В `bin/rustyos-session --doctor` добавлены проверки наличия systemd session targets и переменных окружения тулкитов.

## Последствия

- Все поддерживающие Wayland приложения автоматически запускаются в нативном Wayland-режиме с чётким HiDPI-рендерингом без необходимости ручной настройки профиля пользователя.
- При выходе из сессии (`Log Out...` или сигнал display manager) фоновые сервисы пользователя чисто останавливаются.
- Тестовые вложенные запуски (`--winit`) остаются изолированными и не переопределяют сессионные цели хостовой системы.
