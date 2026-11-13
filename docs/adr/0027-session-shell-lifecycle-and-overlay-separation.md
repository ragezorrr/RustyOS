# ADR-0027: Разделение overlay/chrome в Z-order, backend-независимый `frame_composer` и встроенный shell сессии по умолчанию

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User
- **Связанные задачи:** RUSTYOS-060 (уточняет ADR-0009, ADR-0017)

## Контекст

1. При отображении полноэкранного окна (`fullscreen`) или `wlr-layer-shell` поверхностей (`Layer::Overlay` / `Layer::Top`) композитор подавлял отрисовку всех элементов `shell_render` целиком. В результате системные и интерактивные overlay — модальный запрос `Polkit`, экран выбора цели `Portal`, режим выбора цвета `PickColor`, `Switcher` (`Alt+Tab`), `Spotlight`, `QuickSettings`, `Overview` (а также toast-уведомления и OSD при отсутствии активного fullscreen/overlay-слоя) — могли оказаться скрытыми или перекрытыми под layer-shell/fullscreen-поверхностями.
2. Общая backend-независимая композиция кадра (сборка render list для outputs, масштабирование HiDPI, слои `Layer::Background`..`Layer::Overlay`, IME-popup, отложенные screenshot/screencast/portal readback и курсор) исторически жила в `winit_backend.rs`, что создавало искусственную зависимость `drm_backend.rs` и `screenshot.rs` от модуля Winit.
3. Скрипт `bin/rustyos-session` по умолчанию запускал внешний бинарник `rusty-shell` без флага `--wayland`/`--daemon` (one-shot HUD-режим), тогда как фактическим shell-слоем сессии является встроенный композиторный `shell_render`, а внешний shell-клиент предназначен для опционального запуска с `--wayland`.
4. В `.github/workflows/ci.yml` отсутствовала установка системных dev-пакетов Linux/Wayland/GLES/PipeWire, необходимых для сборки `-sys`-зависимостей (`libwayland-dev`, `libxkbcommon-dev`, `libudev-dev`, `libinput-dev`, `libgbm-dev`, `libdrm-dev`, `libseat-dev`, `libegl-dev`, `libgles-dev`, `libpixman-1-dev`, `libpipewire-0.3-dev` и др.) и headless-проверки композитора.

## Решение

1. **Разделение системных/интерактивных overlay и persistent shell chrome в Z-order и политике скрытия:**
   - Подавление полноэкранного bypass (`active_unoccluded_fullscreen_window_on`) теперь учитывает все системные и интерактивные overlay (`Polkit`, `Portal`, `PickColor`, `Switcher`, `Spotlight`, `QuickSettings`, `Overview`, `LockScreen`, `PowerMenu` и др.) и отделено от политики скрытия persistent chrome (`TopBar`, `Dock`).
   - Оверлеи разделены на два явных слоя композиции: `chrome` (persistent `TopBar`/`Dock`, скрываемый при `fullscreen` или активных поверхностях `Layer::Overlay`/`Layer::Top`) и `foreground_overlay` (`Polkit`, `Portal`, `PickColor`, `Switcher`, `Spotlight`, `QuickSettings`, `PowerMenu`, `LockScreen`, а также неблокирующие уведомления/OSD). `foreground_overlay` рендерится выше полноэкранных окон и `wlr-layer-shell` (`Layer::Overlay` / `Layer::Top`), тогда как `Overview` располагается над управляемыми окнами, но ниже `TopBar` и `Layer::Top`.
   - Обработка pointer-ввода (`input/pointer.rs`, `input/shell_pointer.rs`) синхронизирована с этим разделением: hover и интерактивность persistent chrome и SSD скрываются при `fullscreen` или foreground layer-shell, не нарушая перехват ввода активными модалками и overlay.
2. **Вынесение `frame_composer.rs`:**
   - Backend-независимая логика сборки render list кадра, масштабирования элементов под output scale, слоёв layer-shell/IME/cursor и post-frame capture/screencast вынесена из `winit_backend.rs` в `crates/rusty-compositor/src/frame_composer.rs`.
   - `winit_backend.rs`, `drm_backend.rs` и `screenshot.rs` используют единый модуль `frame_composer` без перекрёстных зависимостей `drm_backend -> winit_backend`.
3. **Жизненный цикл shell в `bin/rustyos-session` (уточнение ADR-0009 и ADR-0017):**
   - Встроенный композиторный shell (`shell_render`) зафиксирован как канонический shell сессии `rustyos-session` по умолчанию.
   - Запуск внешнего клиента `apps/rusty-shell` переведён в явный opt-in через `RUSTYOS_EXTERNAL_SHELL=1` (с передачей `--wayland` и безопасным fallback на встроенный shell при отсутствии бинарника).
4. **CI-окружение (`.github/workflows/ci.yml`):**
   - Добавлен шаг установки системных dev-пакетов Ubuntu для Wayland/DRM/GBM/EGL/GLES/PipeWire и headless smoke-check `cargo run -p rusty-compositor -- --check`.

## Последствия

- Системные диалоги безопасности (`Polkit`, `Portal`, `PickColor`), `Switcher`, `Spotlight`, `QuickSettings` и `Overview` гарантированно видимы и не перекрываются полноэкранными окнами или `wlr-layer-shell` слоями.
- Устранена архитектурная связность `drm_backend` -> `winit_backend`; Winit и DRM/KMS используют общий композитор кадра `frame_composer.rs`.
- `rustyos-session` больше не порождает лишний короткоживущий процесс `rusty-shell` при стандартном запуске; при этом сохранена возможность тестирования/использования внешнего клиента через `RUSTYOS_EXTERNAL_SHELL=1`.
- Локальная конфигурация CI синхронизирована с реальными нативными `-sys` зависимостями workspace (фактическое выполнение job на GitHub Actions runner происходит после push в удалённый репозиторий).
