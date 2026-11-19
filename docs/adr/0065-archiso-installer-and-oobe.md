# ADR-0065: Инсталлятор `rusty-installer`, OOBE Setup Assistant и загрузочный Live ISO (`RUSTYOS-115`)

## Status

accepted

## Context

После завершения базового набора приложений рабочего стола (`rusty-files`, `rusty-settings`, `rusty-updater`, `rusty-terminal`) и перехода на `macOS 27 Golden Gate` (`RUSTYOS-114`) для превращения `RustyOS` в устанавливаемый дистрибутив и воспроизводимый образ потребовалось закрыть четыре системных пробела:

1. **Неполная упаковка системных приложений:** в [PKGBUILD](file:///home/anc/RustyOS_Main/packaging/arch/PKGBUILD) и [install.sh](file:///home/anc/RustyOS_Main/scripts/install.sh) не были доупакованы `rusty-settings`, `.desktop`-ярлыки настроек и инсталлятора, а [bin/rustyos-session](file:///home/anc/RustyOS_Main/bin/rustyos-session) не умел проверять готовность зависимостей (`--doctor`) и запускать Setup Assistant при первом входе.
2. **Отсутствие собственного GUI-инсталлятора и OOBE в стиле macOS Tahoe:** нужен нативный Wayland CSD-клиент без тяжелых сторонних тулкитов и без добавления новых внешних крейтов в `Cargo.lock`, единый по типографике (`Inter`), скруглениям, палитре `Golden Gate Light & Dark`, живому `[ui].accent_index` и `rustyos_liquid_glass_v1`.
3. **Безопасный привилегированный backend разметки и установки Arch + RustyOS:** GUI не должен напрямую запускать деструктивные команды; необходим отделимый root/Polkit helper (`/usr/lib/rustyos/rustyos-install`), защищающий смонтированные хостовые диски, поддерживающий `--dry-run`, `--list-disks-json` и отдающий машиночитаемый поток `STEP:<pct>:<stage>:<msg>`.
4. **Загрузочный Live ISO профиль:** нужен воспроизводимый профиль `archiso` с `liveuser`, автовходом через `greetd` в `rustyos-session --drm`, базовой конфигурацией и ярлыком установки на рабочем столе.

## Decision

Реализовать полный конвейер установки и первого запуска (`RUSTYOS-115`):

1. **Новое приложение [apps/rusty-installer](file:///home/anc/RustyOS_Main/apps/rusty-installer/Cargo.toml) (`org.rustyos.Installer`) без новых зависимостей:**
   - Построено на существующих крейтах графа (`smithay-client-toolkit` + `calloop` + `xkbcommon`, `wayland-client`, `wayland-scanner` для `rustyos_liquid_glass_v1`, `fontdue`, `rusty-animation`, `rusty-services`).
   - Объединяет два режима:
     - **Installer (`--install`, `--live`, `--demo`, `--dry-run`):** `Welcome` (установка на диск, `--doctor`, `rusty-terminal`) -> `SelectDisk` (`lsblk --json` в `disks.rs`, блокировка смонтированных/`< 8 GiB` дисков, выбор `systemd-boot`/`GRUB`) -> `AccountStyle` (профиль, XKB, `Light`/`Dark`/`Auto`, живые свотчи `[ui].accent_index`) -> `Installing` (summary, плавный прогресс `AnimatedValue`, потоковый разбор `STEP:` и сворачиваемый лог).
     - **OOBE Setup Assistant (`--oobe`):** 6 шагов `Hello` -> `Keyboard` -> `Network` (`NetworkService::query_status`) -> `Account` -> `Appearance` (мини-карточки `Light`/`Dark`/`Auto` + 7 свотчей с записью `config.toml` и мгновенным IPC `v1|RELOAD_CONFIG`) -> `Finish` (очистка маркера `.oobe-pending`).

2. **Безопасный backend-скрипт [rustyos-install.sh](file:///home/anc/RustyOS_Main/scripts/rustyos-install.sh) и Polkit:**
   - Поддерживает `--list-disks-json`, `--rollback-help`, `--dry-run` и реальный режим (`--confirm-erase` + `--password-stdin`), блокирует целевые диски со смонтированными разделами или без GPT/UEFI-требований, выполняет `parted`/`mkfs`/`pacstrap`/`arch-chroot`, настраивает `greetd` (`rustyos-session --drm`), локали, раскладку, загрузчик, `config.toml` и маркер `~/.config/rustyos/.oobe-pending`.
   - Привилегированный запуск из GUI идёт через [org.rustyos.installer.policy](file:///home/anc/RustyOS_Main/packaging/polkit/org.rustyos.installer.policy) и `/usr/lib/rustyos/rustyos-install` (`pkexec`).

3. **Упаковка, сессия и профиль Live ISO [packaging/archiso/](file:///home/anc/RustyOS_Main/packaging/archiso/README.md):**
   - Добавлены [org.rustyos.settings.desktop](file:///home/anc/RustyOS_Main/packaging/applications/org.rustyos.settings.desktop) и [org.rustyos.installer.desktop](file:///home/anc/RustyOS_Main/packaging/applications/org.rustyos.installer.desktop); обновлены [PKGBUILD](file:///home/anc/RustyOS_Main/packaging/arch/PKGBUILD), [install.sh](file:///home/anc/RustyOS_Main/scripts/install.sh), [launcher.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/launcher.rs), [rustyos-session](file:///home/anc/RustyOS_Main/bin/rustyos-session) (`--doctor` и автозапуск `--oobe` при наличии `.oobe-pending`, отключаемый `RUSTYOS_SKIP_OOBE=1`) и [.github/workflows/ci.yml](file:///home/anc/RustyOS_Main/.github/workflows/ci.yml) (включая ручной job `arch-live-iso`).
   - В `packaging/archiso/` добавлены `profiledef.sh`, `packages.x86_64`, `pacman.conf`, конфиги `efiboot`/`grub`/`syslinux`, `airootfs` (`greetd` autologin `liveuser -> rustyos-session --drm`, Polkit rules, systemd preset, стартовый `config.toml`, ярлык `install-rustyos.desktop`) и скрипт `build-iso.sh` (`--local-repo-dir`).

## Consequences

- В `Cargo.lock` добавлена только запись локального workspace-пакета `rusty-installer` (14 строк) без новых внешних крейтов; установка и первый запуск `RustyOS` оформлены в едином визуальном языке `Golden Gate`.
- Локальная dry-run симуляция и 7 unit/integration тестов проверяются без root и без риска для дисков хоста.
- Полная сборка загрузочного ISO (`mkarchiso`) воспроизводима локально через `packaging/archiso/build-iso.sh` на Arch Linux и через `workflow_dispatch` job `arch-live-iso` в CI.
