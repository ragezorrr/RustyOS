# ADR-0037: Автономная утилита обновления `apps/rusty-updater` и Polkit-хелпер `rustyos-update-helper` (Этап B)

- **Статус:** accepted
- **Дата:** 2026-11-16
- **Задача:** `RUSTYOS-077`
- **Связанные решения:** `D-045`, `D-044`, `ADR-0036`, `ADR-0030`, `ADR-0032`

## Контекст

После `RUSTYOS-076` (Этап A) в композиторе появились непривилегированный сервис `UpdateCheckerHandle` (`checkupdates --nocolor`), индикатор в `TopBar` и `Liquid Glass` поповер `ShellOverlayView::Updates`, запускающий бинарник `rusty-updater`, однако самого приложения установки и просмотра полного списка обновлений не было.

Необходимо предоставить пользователю цельное desktop-приложение обновления Arch Linux и RustyOS в эстетике `macOS 27 Golden Gate` без нарушения принципов `SECURITY.md` (минимальные привилегии, запрет root для GUI/композитора, авторизация через Polkit и отсутствие `sh -c`).

## Решение

1. **Отдельное Wayland CSD-приложение `apps/rusty-updater`:**
   - Реализовано на том же лёгком стеке без сторонних GUI-фреймворков (`smithay-client-toolkit` + `fontdue` + `rusty-services`, без добавления новых внешних крейтов в `Cargo.lock`).
   - Поддерживает темы `Golden Gate Light & Dark` (автоопределение из `[ui]` в `config.toml` + `--light`/`--dark`), полный прокручиваемый список обновлений (`check_all_pending_updates()`), фильтры `All` / `RustyOS` / `Restart`, прогресс-бар по фазам транзакции, раскрываемую панель живого лога `pacman`, кнопку локальной пересборки `makepkg` при запуске из checkout-дерева и безопасный режим симуляции `--dry-run`.

2. **Разделение привилегий (`rustyos-update-helper` + `org.rustyos.updater.policy`):**
   - В `crates/rusty-services` добавлены потоковый `UpdateTransactionHandle`, парсер прогресса `parse_pacman_progress_line`, строгий валидатор аргументов `validate_helper_args` и бинарник `rustyos-update-helper` (устанавливается в `/usr/lib/rustyos/rustyos-update-helper`).
   - Polkit policy `packaging/polkit/org.rustyos.updater.policy` (`org.rustyos.updater.apply`) требует обязательной административной аутентификации (`auth_admin` / `auth_admin_keep`, `allow_gui=true`) для `pkexec`, интегрируясь со встроенным `PolkitAgentHandle` композитора.
   - `rustyos-update-helper` никогда не вызывает shell (`sh -c`) и допускает только две команды: `sysupgrade` (`pacman -Syu --noconfirm --color never`) и `install-pkg <abs-path.pkg.tar.*>` (с проверкой абсолютного пути без `..`). Локальная пересборка `makepkg` всегда выполняется без `root`.

3. **Упаковка и интеграция с оболочкой:**
   - Обновлены `Cargo.toml`, `packaging/arch/PKGBUILD`, `scripts/install.sh` (включая очистку в `--uninstall-*`), добавлена запись `packaging/applications/org.rustyos.updater.desktop`.
   - В `rusty-compositor` добавлены процедурный значок `BuiltinAppIcon::Updater`, алиасы в `launcher.rs` и `dock.rs`, классификация `TopBarAppProfile` (`"Software Update"`).

## Последствия

- Кнопка `Software Update...` / `Open Updater...` в `TopBar`-поповере и запись в Spotlight бесшовно запускают `rusty-updater`.
- Повышение привилегий ограничено узким валидируемым интерфейсом через `pkexec` и легко откатывается через `pacman -Rns rustyos-git` или `scripts/install.sh --uninstall-system`.
