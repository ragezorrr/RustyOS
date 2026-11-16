# ADR-0036: Упаковка для Arch Linux (`PKGBUILD`), `install.sh` и непривилегированный сервис обновлений (Этап A)

- **Статус:** accepted
- **Дата:** 2026-11-16
- **Задача:** `RUSTYOS-076`
- **Связанные решения:** `D-044`, `ADR-0028`, `ADR-0030`, `ADR-0032`, `ADR-0034`

## Контекст

По мере перехода к установке RustyOS на чистую систему Arch Linux (включая обнаружение сессии через SDDM/дисплей-менеджеры из `/usr/share/wayland-sessions/`) возникли два архитектурных разрыва:

1. **Упаковка и установка:** `scripts/install.sh` не устанавливал файловый менеджер `rusty-files`, при запуске через `sudo` собирал Cargo workspace от `root`, не имел режима удаления (`--uninstall-user` / `--uninstall-system`), а в репозитории отсутствовал `PKGBUILD` для штатного владения файлами в `/usr` через `pacman`.
2. **Обновления в оболочке:** Пользователю необходим наглядный индикатор доступных обновлений Arch Linux и самого RustyOS в `TopBar` (появляющийся при наличии обновлений и вызываемый вручную через меню системы) без блокировки compositor event loop и без автоматического запуска привилегированных транзакций `pacman` или интерактивных `git fetch`/SSH-команд.

## Решение

Реализация разделена на два независимых этапа; настоящий ADR фиксирует **Этап A (`RUSTYOS-076`)**, оставляя отдельное GUI-приложение и привилегированную установку пакетов на **Этап B (`RUSTYOS-077`, `apps/rusty-updater`)**:

1. **Arch `PKGBUILD` (`packaging/arch/PKGBUILD`) и актуализация `scripts/install.sh`:**
   - Создан пакет `rustyos-git` (`provides=('rustyos')`, `conflicts=('rustyos')`) с зависимостями среды Wayland/Smithay и `pacman-contrib` (`checkupdates`), сборкой `--release --locked --workspace` и установкой `rusty-compositor`, `rusty-shell`, `rusty-files`, `rustyos-session`, `rusty-pipewire-stream`, `rustyos.desktop`, порталов XDG (`rustyos.portal`, `rustyos-portals.conf`) и документации.
   - `scripts/install.sh` обновлён для установки `rusty-files`, выполнения сборки от имени `$SUDO_USER` при запуске под `sudo` (без создания root-артефактов в `target/`), проверки наличия `checkupdates` и добавления флагов `--uninstall-user` / `--uninstall-system`.

2. **Непривилегированный фоновый сервис обновлений (`crates/rusty-services/src/updates.rs`):**
   - Добавлен `UpdateCheckerHandle` (`UpdateCheckState`, `PackageUpdateItem`, `UpdateStatusSnapshot`, `parse_checkupdates_output`), выполняющий чтение обновлений в отдельном фоновом потоке строго через непривилегированный `checkupdates --nocolor` (изолированная временная БД без `sudo`/`pkexec` и без `pacman -Sy` в системную БД, с 30-секундным таймаутом, ограничением stdout и очисткой управляющих/CSI ANSI-последовательностей).
   - Выходные строки парсятся и приоритизируются (сначала пакеты `rustyos*` и критичные для перезапуска системные пакеты: `linux*`, `mesa`, `nvidia*`, `systemd`, драйверы), с отслеживанием флага `restart_recommended`.

3. **Настройки `[updates]`, `TopBar`-индикатор и Liquid Glass поповер (`crates/rusty-compositor`):**
   - В `UserConfig` (`config.rs`) добавлена секция `[updates]` (`enabled = true`, `check_interval_minutes = 180` с зажимом `15..=10080`, `show_in_menu_bar = true`), а в `System Settings` добавлены переключатели `Menu Bar -> Updates Indicator` и `Security -> Auto Update Checks`.
   - `UiGlyphKind::Updates { available }` (`surfaces/{mod,glyphs}.rs`) рендерит монохромный 4×4 SSAA SDF-значок обновления и `#0A84FF` угловой индикатор при доступных обновлениях.
   - Значок появляется в правом кластере `TopBar` при наличии обновлений (`menubar_show_updates && total_count > 0`) либо пока открыт поповер, а также доступен в любое время через `System -> Check for Updates...`. Одноразовый HUD-тост уведомляет пользователя при росте числа доступных обновлений.
   - `ShellOverlayView::Updates` (`shell_render/updates_popover.rs`) отображает `Liquid Glass` поповер в стиле `macOS 27 Golden Gate` с заголовком статуса, сводкой по Arch/RustyOS и пометкой о перезапуске, списком приоритетных пакетов (`name`, `old -> new`), кнопками `Check Now` и `Open Updater...` / `Software Update...` (запускает бинарник `rusty-updater` при его наличии в `PATH` или показывает информативное уведомление до завершения Этапа B `RUSTYOS-077`).

## Последствия

- Установка через `makepkg` / `pacman` корректно регистрирует `/usr/share/wayland-sessions/rustyos.desktop` для SDDM и обеспечивает чистое удаление/обновление.
- Проверка обновлений в композиторе полностью изолирована от compositor event loop и не требует прав `root`.
- Этап B (`RUSTYOS-077`) сможет подключаться к `rusty-updater` без изменения контракта композитора: кнопка поповера автоматически переключится на запуск бинарника при появлении его в `PATH`.
