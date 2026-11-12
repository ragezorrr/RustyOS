# RustyOS --- Initial Task Queue

Статусы: `todo`, `in_progress`, `blocked`, `review`, `done`.

## RUSTYOS-001 --- Project Bootstrap

**Статус:** done **Фаза:** 0

### Перед началом

Агент выполняет read-only аудит: - ОС и kernel; -
Rust/Cargo/toolchain; - Git status; - доступные graphics libraries и dev
packages; - текущее состояние workspace.

Затем предлагает план и точный список файлов/команд/проверок. Установки
пакетов и системные изменения --- только после approval.

### Объём

- Cargo workspace;
- минимальный placeholder compositor crate;
- rustfmt/clippy config;
- `.gitignore`;
- минимальный CI workflow;
- README и build instructions;
- краткое описание dev environment.

### Acceptance criteria

- workspace собирается;
- `cargo fmt --check` проходит;
- `cargo check --workspace` проходит;
- `cargo clippy --workspace --all-targets -- -D warnings` проходит либо
  ограничения объяснены;
- `cargo test --workspace` проходит;
- системных изменений без approval нет;
- diff просмотрен.

## RUSTYOS-002 --- Smithay Winit Prototype

**Статус:** done **Фаза:** 1

Nested compositor, тестовый Wayland client, отображение surface, базовый
input, корректный shutdown, documented dependencies and limitations.

## RUSTYOS-003 --- Frame Timing Instrumentation

**Статус:** done **Фаза:** 1--4

Измерение frame/presentation intervals, воспроизводимый сценарий,
hardware/software context и низкие накладные расходы.

## RUSTYOS-004 --- Core Window Management Model (`crates/rusty-wm`)

**Статус:** done **Фаза:** 2

- **Цель:** Детерминированная, тестируемая без GPU/Wayland модель управления окнами.
- **Scope:** Outputs, Workspaces (`1..=9`), Windows, Focus, Stacking (`z-order`), Layout/Placement (macOS-style Floating + Snap Left/Right/Maximize/Center + Master-Stack Tiling), `WmAction`.
- **Dependencies:** `RUSTYOS-002`.
- **Acceptance criteria:** `crates/rusty-wm` собирается без `unsafe` и внешних зависимостей; покрыты unit-тестами multi-window, multi-workspace и multi-output сценарии.
- **Risks:** Расхождение состояния между чистой моделью WM и Wayland-поверхностями при закрытии/ресайзе (минимизируется явным контрактом синхронизации в `RUSTYOS-005`).

## RUSTYOS-005 --- Compositor WM Integration & Shortcuts

**Статус:** done **Фаза:** 2

- **Цель:** Интеграция `rusty-wm` в `rusty-compositor`, синхронизация геометрии/видимости с `Space<Window>` и `xdg_toplevel`, клавиатурные сочетания.
- **Scope:** Связка `WindowId <-> smithay::desktop::Window`, отправка `xdg_toplevel::configure` с размерами и `Activated` состоянием, фильтрация видимых окон активного workspace при рендере и вводе, хоткеи (`Alt+Tab`/`Alt+J/K`, `Alt+1..9`, `Alt+Shift+1..9`, `Alt+L`, `Alt+M`, `Alt+Left/Right`, `Alt+C`, `Alt+W`), проверка multi-window сценария.
- **Dependencies:** `RUSTYOS-004`.
- **Acceptance criteria:** Хоткеи и клики мыши предсказуемо переключают фокус, раскладку и рабочие столы; проверен сценарий с несколькими окнами; все проверки `fmt`, `check`, `clippy`, `test` проходят.
- **Risks:** Некорректная обработка `commit` уже закрытого окна (минимизируется очисткой неживых окон в `refresh`).

## RUSTYOS-006 --- Native DRM/KMS & GBM/EGL Graphics Backend

**Статус:** done **Фаза:** 3

- **Цель:** Реализация нативного графического бэкенда DRM/KMS + GBM/EGL + DMA-BUF с runtime capability discovery и безопасной верификацией на реальном GPU.
- **Scope:** Подключение `backend_drm`, `backend_gbm`, `backend_udev`, `backend_session_libseat`, `backend_libinput`; модуль `drm_backend` с обнаружением DRM-нод, коннекторов, режимов (`1920x1080`), плоскостей (`primary`/`cursor`/`overlay`), проверкой `GBM` + `EGLDisplay` + `GlesRenderer` + `DMA-BUF` рендера/импорта (`--drm-probe`) и инициализацией `LibSeatSession` + `DrmDevice` (`--drm`); запись результатов в `docs/HARDWARE_MATRIX.md`.
- **Dependencies:** `RUSTYOS-002`, `RUSTYOS-005`.
- **Acceptance criteria:** Нативный бэкенд собирается без предупреждений, проходит реальную аппаратную проверку DRM/GBM/EGL/DMA-BUF на Intel Raptor Lake-U (`i915`), nested Winit backend остаётся полностью рабочим, `docs/HARDWARE_MATRIX.md` обновлён.
- **Risks:** Занятость `DRM Master` при запуске внутри активной графической сессии (минимизируется безопасным режимом `--drm-probe` и чистым fallback-отчётом в `--drm`).

## RUSTYOS-007 --- Performance Foundation & Damage Tracking Benchmark Harness

**Статус:** done **Фаза:** 4

- **Цель:** Измерение input-to-present latency, задержек диспетчеризации event loop, эффективности damage tracking и воспроизводимое сравнение «до / после».
- **Scope:** Расширение `FrameTimingCollector` и `FrameTimingSummary` (`input_to_present_us`, `event_loop_dispatch_us`, `dispatch_stalls`, `damage_ratio_permille`, `zero_damage_frames`); подача реального `buffer_age` и среза повреждённых прямоугольников в `backend.submit(...)`; CLI-флаги `--bench-input` и `--force-full-damage`; фиксация результатов сравнения в `PERFORMANCE.md`.
- **Dependencies:** `RUSTYOS-003`, `RUSTYOS-005`.
- **Acceptance criteria:** Нулевые аллокации в горячем цикле сбора метрик, проходят unit-тесты и реальный A/B-бенчмарк на оборудовании Intel Raptor Lake-U (`i915`), зафиксировано снижение площади повреждений и `p95` input-to-present latency в `PERFORMANCE.md`.
- **Risks:** Артефакты частичного обновления буфера при неверном `buffer_age` (минимизируется запросом `backend.buffer_age().unwrap_or(0)` после `bind()`).

## Правило добавления задач

Каждая задача должна иметь ID, цель, scope, dependencies, acceptance
criteria, tests, risks и status. Детализировать в первую очередь
ближайшую фазу.
