# RustyOS

**RustyOS** --- самостоятельная Linux Desktop Environment на Rust с
собственным Wayland-композитором, оконным менеджером, animation engine,
рендерером и оболочкой рабочего стола (визуальное направление вдохновлено
macOS / Liquid Glass при оригинальной реализации).

> **Текущий статус (`RUSTYOS-001` -- `RUSTYOS-045`):** ядро композитора,
> WM, animation engine, shell-оверлеи, multi-monitor DRM/KMS, VRR,
> Direct Scanout (`DMA-BUF`), дробное HiDPI-масштабирование, low-latency
> input/presentation, `cursor-shape`, `xdg-activation`, `idle` и
> `ext_session_lock` реализованы и покрыты автотестами.
> Проверка live bare-metal `--drm` на реальных GPU AMD/NVIDIA и финальная
> упаковка дистрибутива (Фазы 7/9/10) находятся в работе.

## Структура Cargo Workspace (`MSRV 1.99`, `edition = "2024"`)

- `crates/rusty-compositor` --- Wayland-композитор на базе Smithay `0.7`
  (`--winit` nested и `--drm` native DRM/KMS/GBM/EGL/libinput, `XWayland`,
  multi-monitor/VRR/HiDPI, input/shortcuts, frame timing, конфиг и рендер-оверлеи).
- `crates/rusty-wm` --- детерминированная модель окон, рабочих столов
  (`1..=9`), фокуса, стекинга и раскладок (`Floating`, `Snap`, `MasterStack`).
- `crates/rusty-animation` --- refresh-independent движок анимаций
  (`60/120/144/240` Hz, easing, отмена/реверс, `reduced_motion`).
- `crates/rusty-ipc` --- версионируемый Unix socket IPC-протокол между
  композитором и внешним shell.
- `crates/rusty-services` --- неблокирующие системные адаптеры (`BlueZ`,
  `NetworkManager`, аудио/`wpctl`, питание/`UPower`, подсветка `sysfs`).
- `apps/rusty-shell` --- внешний процесс оболочки (`wlr-layer-shell` + IPC).
- `bin/rustyos-session`, `packaging/`, `scripts/install.sh` --- заготовка
  системной сессии и локальной установки.

## Сборка, проверки и запуск

```bash
# Форматирование и статический анализ
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings

# Тесты workspace (флаг --skip полезен в headless/sandbox без BlueZ D-Bus)
cargo test --workspace
cargo test --workspace -- --skip networkmanager_and_bluetooth_formatters_handle_all_states

# Bootstrap-проверка без окна
cargo run -p rusty-compositor -- --check

# Nested Wayland-композитор (Smithay + Winit)
cargo run -p rusty-compositor -- --winit

# Smoke-тест с клиентом и экспортом метрик frame pacing
cargo run -p rusty-compositor -- --winit --max-frames 120 --spawn es2gears_wayland --metrics-out /tmp/rustyos-frame-metrics.json

# Диагностика возможностей DRM/KMS, GBM, EGL и DMA-BUF (без захвата DRM Master)
cargo run -p rusty-compositor -- --drm-probe --metrics-out /tmp/rustyos-drm-report.json

# Нативный запуск на реальном TTY (DRM/KMS + libinput + libseat)
cargo run -p rusty-compositor -- --drm
```

### Основные клавиши и управление

- `Alt + Space` / `Super + Space` --- Spotlight Application Launcher
- `Super + Tab` / `Alt + E` --- Workspace Overview (Mission Control)
- `Alt + Tab` / `Alt + J` / `Alt + K` --- переключение фокуса окон
- `Alt + 1..9` / `Alt + Shift + 1..9` --- переключение рабочего стола / перенос окна (`1..9`)
- `Alt + L` --- переключение раскладки (`Floating` <-> `MasterStack`)
- `Alt + M` / `Alt + F` --- Maximize / Fullscreen (включая Direct Scanout и VRR)
- `Alt + Left` / `Alt + Right` / `Alt + C` --- быстрая привязка (`LeftHalf`, `RightHalf`, `Center`)
- `Alt + O` --- перенос активного окна на следующий монитор
- `Alt + H` --- свернуть активное окно в Dock
- `Alt + W` / `Alt + Q` --- закрыть активное окно
- `Alt + S` / `Alt + R` --- Quick Settings / переключение `reduced_motion`
- `Alt + N` / `Alt + Shift + N` --- тестовое уведомление / очистка уведомлений
- `Super + L` / `Ctrl + Alt + L` --- встроенный Lock Screen (также поддерживается `ext_session_lock_v1`)
- `Print` --- скриншот экрана (c `Shift`/`Alt` для области/буфера)
- `Ctrl + Alt + F1..F12` --- переключение TTY VT
- `Escape` / `Ctrl + Alt + Backspace` --- выход из композитора

### Пользовательский конфиг (`~/.config/rustyos/config.toml`)

Поддерживаются секции `[keyboard]` (`layout`, `variant`, `options`,
`repeat_delay`, `repeat_rate`), `[display]` (`vrr_mode`,
`prefer_max_refresh`, `scale`, `[display.scales]`), `[ui]`
(`reduced_motion`, `wallpaper`, `cursor_theme`) и `[autostart]` (`commands`).

Подробности среды сборки зафиксированы в [docs/DEVELOPMENT_ENVIRONMENT.md](docs/DEVELOPMENT_ENVIRONMENT.md).

## Главные документы

  Файл                         Назначение
  ---------------------------- ------------------------------------------
  `AGENTS.md`                  Постоянные правила разработки и approval gates
  `ROADMAP.md`                 Фазы разработки (0..10) и критерии выхода
  `ARCHITECTURE.md`            Актуальная архитектура и структура workspace
  `PERFORMANCE.md`             Цели по latency, frame pacing и benchmark-протоколы
  `HARDWARE.md`                Требования совместимости AMD, Intel и NVIDIA
  `DESIGN.md`                  Визуальный язык и дизайн-система macOS / Liquid Glass
  `SECURITY.md`                Правила безопасности, IPC и системных изменений
  `DECISIONS.md`               Реестр принятых решений (`D-001` .. `D-022`)
  `TASKS.md`                   Журнал выполненных и текущих задач (`RUSTYOS-001` .. `RUSTYOS-045`)
  `FILE_INVENTORY.md`          Инвентарь файлов репозитория
  `docs/`                      ADR (`0001`..`0014`), hardware matrix и гайды
  `.agents/rules/`             Правила Antigravity
  `.agents/skills/`            14 специализированных навыков
  `.agents/workflows/`         Повторяемые workflows

## Принципы проекта

- Пользователь --- главный архитектор RustyOS; новые фичи `RUSTYOS-NNN` и
  архитектурные изменения внедряются только после предложения и явного
  `APPROVE:`.
- Экономия контекста обязательна, но не за счёт качества, безопасности,
  тестов и замеров производительности.
- Поддержка GPU/VRR/zero-copy заявляется только после реальных проверок и
  фиксации в [docs/HARDWARE_MATRIX.md](docs/HARDWARE_MATRIX.md).
- KDE, GNOME и Hyprland не используются как основа RustyOS.
