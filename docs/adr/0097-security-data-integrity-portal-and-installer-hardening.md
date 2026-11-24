# ADR-0097: Харденинг безопасности, целостности ФС, XDG-порталов, Greeter и установщика (`RUSTYOS-168`)

- **Status:** Accepted
- **Date:** 2026-11-24
- **Decision ID:** `D-111`
- **Related:** `RUSTYOS-168`, `RUSTYOS-154`, `RUSTYOS-136`, `RUSTYOS-137`, `RUSTYOS-115`, `RUSTYOS-116`, `ADR-0083`, `ADR-0080`, `ADR-0081`

## Контекст

Полный аудит системы выявил критические уязвимости, риски потери данных и системные поломки:
1. **Безопасность и изоляция:** IPC `SetLockScreen { locked: false }` снимал блокировку без PAM, интерактивные IPC-команды проходили при заблокированной сессии, `screen_locked` не делал ранний выход из `frame_composer.rs`; `nmcli` передавал WPA-пароль в `argv` (`/proc/<pid>/cmdline`) и SSID без `--`; операции с архивами в `rusty-files` использовали `unzip -o` без защиты от Zip-Slip/Tar-Slip и `zip` без `--`.
2. **Целостность ФС:** `pick_unique_sibling_path` возвращал существующий `base_path` после 999 копий (усекая исходный файл в `0` байт), `trash.rs` не проверял `..` в `.trashinfo`, а Desktop drag-to-trash/rename блокировали event loop композитора и тихо перезаписывали файлы.
3. **Порталы `FileChooser` / `Settings`:** `FileChooserPortalIface` не передавал `WAYLAND_DISPLAY`/`RUSTYOS_IPC_SOCKET` в `rusty-files` (отказ на `--drm`), блокировал поток `zbus` без `Request.Close`; `--save-file` в `rusty-files` не позволял вводить имя файла и возвращал exit code `0` при отмене; `SettingsPortalIface` не эмитил `SettingChanged` и игнорировал `[ui]`/комментарии в `config.toml`.
4. **Установщик, Greeter, скрипты и упаковка:** `apps/rusty-installer` игнорировал `timezone_idx`, допускал pipe-deadlock `stdout`/`stderr`, подставлял `demo_target_disks()` в боевом режиме при сбое `lsblk`, принимал невалидный RFC 1123 `hostname` и показывал фейковый `Create Account` в `--oobe`; `rusty-greeter` блокировал UI-поток без таймаута; `rustyos_installer_service.rs`, `scripts/rustyos-install.sh`, `scripts/install.sh`, `bin/rustyos-session` и `PKGBUILD` содержали рассинхрон флагов, путей порталов и прав пользователя `greeter`.

## Решение

Реализовать **Фазу 1 аудита (Вариант 1)** без новых внешних Cargo-зависимостей и без `unsafe` (только правки файлов репозитория, без вмешательства в хост-ОС):

1. **Композитор:** запретить unauthenticated IPC unlock при `require_password_on_lock = true` и блокировать интерактивные/launch IPC-запросы пока `is_session_locked()`; добавить ранний `return render_list` при `state.screen_locked && lock_progress >= 0.99`; перевести Desktop drag-to-trash на `spawn_background_trash_drop` и защитить Desktop `rename` через `pick_unique_copy_path`.
2. **`rusty-services`:** добавить общий `run_command_bounded(cmd, timeout)` с конкурентным чтением `stdout`/`stderr`; перевести `nmcli` на временный `0600` `--passwd-file` с затиранием в `Drop` и разделитель `--`; убрать возврат занятого `base_path` в `pick_unique_sibling_path` + проверку `(dev, ino)` в `copy_file_chunked` и запрет `..`/относительных путей в `restore_from_trash`; добавить `child.wait()` в `updates.rs` и `conn.close()` в `SystemSignalMonitorHandle::drop`.
3. **Порталы и `rusty-files`:** добавить `PortalHandle::{set_session_env, notify_color_scheme_changed}`, перевести `FileChooserPortalIface` в неблокирующий `async` с `FileChooserRequestIface::close` и пробросом `WAYLAND_DISPLAY`/`RUSTYOS_IPC_SOCKET`, добавить сигнал `SettingsPortalIface::setting_changed` и парсер `parse_portal_color_scheme_from_toml`; в `rusty-files` добавить `validate_archive_member_paths`, распаковку в уникальную подпапку (`unzip -n`), `--` для `zip`/`bsdtar`, редактируемый `picker_filename_field` (`TextField`) и `ExitCode::from(1)` при отмене/закрытии пикера.
4. **Установщик, Greeter, скрипты и `PKGBUILD`:** передавать `TIMEZONE_OPTIONS[self.timezone_idx]`, убрать fallback на `demo_target_disks()` при `!demo_mode`, читать `stderr` в отдельном потоке, унифицировать RFC 1123 `hostname` и пропускать `OobeStep::Account` в `--oobe`; перевести `rusty-greeter` на фоновый поток с 15 с таймаутом и выводом PAM `info`/`error`; синхронизировать `rustyos_installer_service.rs`, `scripts/rustyos-install.sh` (валидация, `sgdisk -E` clamp, `greeter` группы), `scripts/install.sh` (`PORTALS_CONF_DIR`, `--uninstall`, `awk`), `bin/rustyos-session` (`XDG_DESKTOP_PORTAL_DIR`) и `packaging/arch/{rustyos-greeter.sysusers, PKGBUILD}`.

## Последствия

- **Проверки:** `bash -n` (3 скрипта чисто), `cargo fmt --check`, `cargo clippy -D warnings` (0 предупреждений по 5 пакетам), unit-тесты (`240` в `rusty-compositor`, `75` в `rusty-services`, `28` в `rusty-files`, `15` в `rusty-installer`, `1` в `rusty-greeter` — всего `359/359` passed), живые `--winit` Wayland Light/Dark прогоны `rusty-files --save-file report.pdf` с 0 WARN/ERROR.
