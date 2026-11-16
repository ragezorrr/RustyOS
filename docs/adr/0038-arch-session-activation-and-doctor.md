# ADR-0038: Подготовка чистой Arch-сессии, `--drm` D-Bus/systemd activation и pre-flight `--doctor`

- **Статус:** accepted
- **Дата:** 2026-11-16
- **Задача:** `RUSTYOS-078`
- **Связанные решения:** `D-046`, `D-045`, `D-044`, `ADR-0037`, `ADR-0036`, `ADR-0009`

## Контекст

После упаковки (`RUSTYOS-076`) и создания автономного апдейтера с Polkit-хелпером (`RUSTYOS-077`) выявились три практических пробела при развёртывании на минимальном чистом Arch Linux:
1. В `bin/rustyos-session` переменные `systemd --user` / `D-Bus` экспортировались до старта `rusty-compositor`, когда `WAYLAND_DISPLAY` и `DISPLAY` (`XWayland`) ещё отсутствовали.
2. Не хватало `.desktop`-файла `org.rustyos.files.desktop` (`MimeType=inode/directory;`), ряда базовых runtime-зависимостей минимального Arch (`dbus`, `fontconfig`, `adwaita-icon-theme`, `optdepends` системных сервисов) и воспроизводимой проверки `makepkg` в CI.
3. Отсутствовала read-only команда самодиагностики перед первым входом через TTY/SDDM/greetd.

## Решение

1. **Безопасный экспорт окружения только в нативном `--drm` режиме:**
   - В `RustyCompositorState` добавлен флаг `sync_systemd_activation_env` (`false` по умолчанию в тестах и `--winit`, `true` только в `drm_backend.rs`).
   - После создания `WAYLAND_DISPLAY` и при готовности `XWayland` (`set_xwayland_display`) композитор в `--drm` асинхронно (в выделенном фоновом потоке `drm-env-sync`, без блокировки KMS event loop) вызывает `dbus-update-activation-environment --systemd` с `WAYLAND_DISPLAY`, `XDG_CURRENT_DESKTOP=RustyOS`, `XDG_SESSION_DESKTOP=RustyOS`, `XDG_SESSION_TYPE=wayland`, а при наличии — `DISPLAY` и `XDG_DESKTOP_PORTAL_DIR`.

2. **Read-only диагностика `rustyos-session --doctor`:**
   - В `bin/rustyos-session` добавлена команда `--doctor`, которая без root и без изменения системы проверяет наличие бинарников (соседние установленные, свежие из repo `target/`, `PATH`), утилит (`Xwayland`, `dbus-update-activation-environment`, `pkexec`, `pacman`, `checkupdates`), `.desktop`, `polkit` policy, порталов, системных и пользовательских служб (`dbus`, `polkit`, `NetworkManager`, `bluetooth`, `upower`, `pipewire`, `wireplumber`, `xdg-desktop-portal`), `/dev/dri/*` и выполняет `rusty-compositor --drm-probe`.

3. **Arch упаковка и CI:**
   - Добавлен `packaging/applications/org.rustyos.files.desktop` и интегрирован в `packaging/arch/PKGBUILD` и `scripts/install.sh` (включая `--uninstall-*`).
   - В `PKGBUILD` уточнены `depends`/`optdepends` для минимального Arch и добавлен `--remap-path-prefix` в `RUSTFLAGS` для воспроизводимой сборки без `$srcdir`-путей в бинарниках.
   - В `.github/workflows/ci.yml` добавлен job `arch-packaging` (`archlinux:base-devel`) с валидацией скриптов, `.desktop`, Polkit XML и изолированной сборкой `makepkg`.

## Последствия

- Nested `--winit` и unit-тесты не затрагивают пользовательскую `D-Bus`/`systemd` сессию хоста.
- Перед первым входом на чистом Arch пользователь может одной командой `rustyos-session --doctor` убедиться в готовности DRM/GBM/EGL, XWayland, Polkit и порталов.
