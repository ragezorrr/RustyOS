# ADR-0001: Фаза 1 — Стек зависимостей и модель цикла событий для Smithay Winit Prototype (`RUSTYOS-002`)

- Status: accepted
- Date: 2026-11-11
- Decision owner: RustyOS user

## Context

После завершения `RUSTYOS-001` (Фаза 0) следующим этапом является `RUSTYOS-002` (Фаза 1 — Nested Compositor Prototype).
Цель задачи: реализовать минимальный Wayland compositor с бэкендом Smithay + Winit, который запускается в окне текущей графической сессии, принимает подключение тестового Wayland-клиента, отображает его поверхность (`wl_surface` / `xdg_toplevel`), обрабатывает базовый ввод (клавиатура/указатель), корректно обрабатывает отключение клиента и штатное завершение работы без блокировки цикла событий.

В локальном кэше Cargo и в системе уже присутствуют:
- `smithay = "0.7.0"`
- `calloop = "0.14.5"` (`smithay` реэкспортирует `calloop` и `wayland-server`)
- `tracing = "0.1.44"`, `tracing-subscriber = "0.3.23"`
- системные библиотеки Wayland, EGL, GLESv2, XKB, DRM/GBM
- системные тестовые клиенты `/usr/bin/es2gears_wayland`, `/usr/bin/wayland-info`, `/usr/bin/konsole`

Требуется согласовать:
1. Набор внешних зависимостей и feature-флагов `smithay` для Фазы 1 (чтобы не тянуть лишние бэкенды вроде `backend_vulkan` или `xwayland` раньше времени).
2. Архитектуру главного цикла событий (`calloop::EventLoop` vs ручной `pump_events` цикл) и способ проверки с тестовым Wayland-клиентом.

## Options

### Option A (Рекомендуемый): Минимальные фичи `smithay 0.7` (`default-features = false`) + `calloop::EventLoop` + встроенный smoke-тест клиента

1. **Зависимости в `[workspace.dependencies]`:**
   - `smithay = { version = "0.7.0", default-features = false, features = ["backend_winit", "renderer_gl", "wayland_frontend", "desktop"] }`
   - `tracing = "0.1.44"`
   - `tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }`
   *(Клиентские и серверные крейты `calloop`, `wayland-server`, `wayland-protocols` используются через `smithay::reexports`, чтобы гарантировать 100% совпадение версий).*
2. **Архитектура цикла событий:**
   - Использование неблокирующего `calloop::EventLoop` (`WinitEventLoop` + `ListeningSocketSource` + `Generic` source для `Display`), а также `smithay::desktop::Space` для отслеживания `Window` / `ToplevelSurface`.
   - Поддержка аргументов CLI в `rusty-compositor`:
     - `--check` (быстрая проверка без открытия окна, для headless CI);
     - `--nested` / `--winit` (запуск nested-композитора);
     - `--max-frames <N>` или `--timeout-ms <MS>` (опциональный флаг для автоматизированных проверок и воспроизводимого завершения).
3. **Тестовый Wayland-клиент и верификация:**
   - Интеграционный/модульный тест жизненного цикла сервера и подключения клиента (через `wayland-client` из реэкспорта/кэша или тестовый harness) + реальный прогон с системным `/usr/bin/es2gears_wayland` / `wayland-info` в текущей сессии (`WAYLAND_DISPLAY=wayland-1`).

- **Плюсы:**
  - Полностью собирается офлайн из уже закэшированных крейтов (`smithay 0.7.0`).
  - Не компилирует `backend_vulkan`, `backend_drm`, `backend_udev`, `xwayland` до соответствующих фаз (экономит время сборки и избегает лишнего `unsafe` в зависимостях).
  - Сразу закладывает правильную архитектуру на базе `calloop::EventLoop` и `smithay::desktop::Space`, которая потребуется в Фазе 2 (`Window Management`) и Фазе 3 (`Native DRM/KMS`).
- **Минусы:**
  - Чуть больше начального связующего кода (`delegate_*` макросы Smithay), чем в примитивном `loop { pump_events }`.
- **Риски:** Низкие.

### Option B: `smithay 0.7` с `default` features + упрощённый цикл `pump_events` (как в `examples/minimal.rs`)

- **Плюсы:** Чуть короче код инициализации Winit-цикла.
- **Минусы:**
  - `default` фичи `smithay` сразу компилируют Vulkan (`ash`), X11, DRM/GBM, libseat, udev и XWayland, что нарушает принцип минимального объёма Фазы 1 и правило `AGENTS.md` (не вводить Vulkan без обоснования).
  - Цикл на `pump_events` без `calloop` и `desktop::Space` придётся полностью переписывать в `RUSTYOS-003` и Фазе 2.
- **Риски:** Лишние зависимости и технический долг при переходе к Фазе 2.

### Option C: Выделение отдельного крейта/бинаря `crates/rusty-test-client` уже в `RUSTYOS-002`

- **Плюсы:** Собственный минимальный SHM-клиент в репозитории.
- **Минусы:** Добавляет лишний крейт в workspace (противоречит `Token Efficiency Protocol`: «Не создавай лишние crates, абстракции, файлы и зависимости»), тогда как тестовый клиент можно держать как пример/интеграционный тест внутри `crates/rusty-compositor` и дополнять проверкой системным `es2gears_wayland`.

## Recommendation

Рекомендуется **Option A**:
- Подключить `smithay = "0.7.0"` с `default-features = false` и только четырьмя фичами (`backend_winit`, `renderer_gl`, `wayland_frontend`, `desktop`), плюс `tracing` и `tracing-subscriber`.
- Построить прототип на `calloop::EventLoop` и `smithay::desktop::Space`, обеспечив чистый non-blocking цикл, отображение `xdg_toplevel` поверхности, обработку клавиатуры/мыши (включая хоткей выхода `Escape` / `Ctrl+Q` и закрытие окна Winit) и корректный shutdown.
- Сохранить `#![deny(unsafe_code)]` в нашем коде (`crates/rusty-compositor`).

## Decision

Утверждён **Option A** (APPROVE: Option A, 2026-11-11): использование `smithay 0.7` с минимальными фичами (`backend_winit`, `renderer_gl`, `wayland_frontend`, `desktop`), `calloop::EventLoop`, `smithay::desktop::Space<Window>` и `tracing`.

## Consequences

- В `Cargo.toml` добавляются `smithay`, `tracing`, `tracing-subscriber`.
- `crates/rusty-compositor` получает модульную структуру (`state`, `handlers`, `winit_backend`, `input`) и рабочий режим `--nested` / `--winit`.
- В `docs/HARDWARE_MATRIX.md` записывается результат реального запуска nested-прототипа на Intel Raptor Lake-U (`i915`).
