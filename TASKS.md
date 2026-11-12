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

## RUSTYOS-008 --- Refresh-Rate Independent Animation Engine (`crates/rusty-animation`)

**Статус:** done **Фаза:** 5

- **Цель:** Плавные, прерываемые переходы окон и рабочих столов на базе монотонного времени (`Duration`), независимые от частоты обновления монитора (`60/120/144/240 Hz`), с поддержкой `reduced-motion`.
- **Scope:** Создание `std`-only крейта `crates/rusty-animation` (`MotionTokens`, `Easing`, `AnimatedValue`, `AnimatedRect`, `AnimationEngine`); интеграция в `crates/rusty-compositor` (`state.rs`, `input.rs`, `winit_backend.rs`, флаг `--reduced-motion`, хоткей `Alt+R`); unit-тесты кривых easing, независимости от герцовки (`60/120/144/240 Hz`), пропусков кадров, `retarget`/`reverse` и `reduced_motion`.
- **Dependencies:** `RUSTYOS-004`, `RUSTYOS-005`, `RUSTYOS-007`.
- **Acceptance criteria:** Анимации не привязаны к фиксированным 60 FPS, корректно прерываются/разворачиваются без скачков, режим `reduced_motion` мгновенно фиксирует целевую геометрию, все тесты и линтеры проходят.
- **Risks:** Лишние `xdg_toplevel::configure` при анимации (минимизируется отправкой клиенту сразу целевого размера `rect.width x rect.height` и интерполяцией позиции в `Space<Window>`).

## RUSTYOS-009 --- Versioned Local IPC Protocol & Server (`crates/rusty-ipc`)

**Статус:** done **Фаза:** 6

- **Цель:** Безопасный версионированный локальный IPC между `rusty-compositor` и `rusty-shell` по Unix Domain Socket (`0600`).
- **Scope:** Крейт `crates/rusty-ipc` (`IPC_PROTOCOL_VERSION = 1`, `MAX_IPC_MESSAGE_BYTES = 8192`, `ShellRequest`, `CompositorEvent`, `DesktopStateSnapshot`, `IpcServer`, `IpcClient`); интеграция неблокирующего опроса `IpcServer` в `rusty-compositor` (`state.rs`, `winit_backend.rs`).
- **Dependencies:** `RUSTYOS-005`, `RUSTYOS-008`.
- **Acceptance criteria:** Сокет создаётся с правами `0600`, отклоняются неверные версии и сообщения `> 8 KiB`, отключение/падение IPC-клиента не роняет и не блокирует композитор.
- **Risks:** Блокировка event loop на медленном клиенте (исключена переводом `UnixListener` и `UnixStream` в `set_nonblocking(true)`).

## RUSTYOS-010 --- Independent Desktop Shell (`apps/rusty-shell`)

**Статус:** done **Фаза:** 6

- **Цель:** Реализация самостоятельного процесса оболочки рабочего стола с независимым lifecycle.
- **Scope:** Приложение `apps/rusty-shell` (`DesignTokens`, `TopBarState`, `DockItem`, `AppLauncher`, `NotificationCenter`, `QuickSettingsState`, `WallpaperConfig`, `DesktopShell`); синхронизация состояния с `rusty-compositor` по `rusty-ipc`, проверка сценария перезапуска `rusty-shell` без перезапуска композитора.
- **Dependencies:** `RUSTYOS-008`, `RUSTYOS-009`.
- **Acceptance criteria:** `rusty-shell` работает в отдельном процессе, переживает собственный перезапуск и восстанавливает состояние (`active_workspace`, `overview_active`, окна в Dock) из `StateSnapshot` композитора.
- **Risks:** Потеря синхронизации после перезапуска `rusty-shell` (решается немедленным запросом `GetStateSnapshot` при новом `Handshake`).

## RUSTYOS-011 --- Linux System Services Integration (`crates/rusty-services`)

**Статус:** done **Фаза:** 7

- **Цель:** Безопасная интеграция с базовыми сервисами Linux (батарея, сеть, аудио, сессия) с минимальными привилегиями и гарантией мягкой деградации.
- **Scope:** Крейт `crates/rusty-services` (`PowerService`, `NetworkService`, `AudioService`, `SessionService`, `ServiceRegistry`); интеграция с `apps/rusty-shell` для отображения реального состояния батареи, сети и громкости в `TopBar` и `QuickSettings`.
- **Dependencies:** `RUSTYOS-009`, `RUSTYOS-010`.
- **Acceptance criteria:** Сборка без внешних сетевых зависимостей, нулевые паники при отсутствии сервисов, тесты проходят.
- **Risks:** Блокировка процесса на зависших внешних вызовах (минимизируется чтением sysfs и быстрыми status-вызовами).

## RUSTYOS-012 --- Visual Shell Overlays & Core UX Rendering

**Статус:** done **Фаза:** 8

- **Цель:** Нативная визуальная отрисовка элементов оболочки (TopBar, Dock) на экране и резервирование рабочей области окон (Work Area).
- **Scope:** Модуль `shell_render.rs` (`generate_shell_render_elements`, `usable_work_area`, `check_top_bar_workspace_click`); интеграция в GLES-пайплайн композитора `render_output`; интерактивное переключение воркспейсов мышью по клику на TopBar; индикация фокуса окон в Dock; эксклюзивная рабочая зона окна между TopBar и Dock.
- **Dependencies:** `RUSTYOS-010`, `RUSTYOS-011`.
- **Acceptance criteria:** На экране отображается TopBar и Dock с дизайн-токенами из `DESIGN.md`; окна позиционируются внутри рабочей зоны и не перекрывают панели; клик мыши по индикаторам `1..9` переключает воркспейс; тесты и линтеры проходят.
- **Risks:** Дополнительная нагрузка на рендер (минимизирована использованием статических solid color элементов).

## RUSTYOS-013 --- Hardware Compatibility & Capability Matrix Verification

**Статус:** done **Фаза:** 9

- **Цель:** Верификация цепочки совместимости GPU (Intel, AMD, NVIDIA), Atomic KMS, DMA-BUF форматов, многомониторных коннекторов и аппаратного курсора с документацией в матрице.
- **Scope:** Запуск и валидация аппаратного пробника `--drm-probe` на Intel Raptor Lake-U (`i915`), проверка обнаружения CRTCs (4), Planes (24), DMA-BUF форматов рендера (133) и текстур (236), коннекторов eDP/HDMI/DP, фиксация результатов и fallback-поведения в `docs/HARDWARE_MATRIX.md`.
- **Dependencies:** `RUSTYOS-006`, `RUSTYOS-012`.
- **Acceptance criteria:** Пробник успешно исполняется без прав root и без падений, обнаружены аппаратные курсоры и плоскости, `docs/HARDWARE_MATRIX.md` содержит полные проверенные данные.
- **Risks:** Несовместимость проприетарных драйверов (минимизируется разделением обнаружения возможностей от политики рендеринга).

## RUSTYOS-014 --- Distribution & Session Installation Infrastructure

**Статус:** done **Фаза:** 10

- **Цель:** Превратить RustyOS в устанавливаемую десктопную сессию с возможностью выбора из Display Manager.
- **Scope:** Точка входа сессии `bin/rustyos-session` (`XDG_CURRENT_DESKTOP=RustyOS`, запуск композитора и оболочки); файл сессии `packaging/wayland-sessions/rustyos.desktop`; инсталлятор `scripts/install.sh` с поддержкой пользовательской установки (`~/.local/bin`) без прав root; документация в `docs/adr/0009-distribution-and-session.md`.
- **Dependencies:** `RUSTYOS-010`, `RUSTYOS-012`.
- **Acceptance criteria:** `rustyos-session` находит и запускает композитор и оболочку; `rustyos.desktop` валиден по стандарту Freedesktop; сборка и тесты проходят без ошибок.
- **Risks:** Отсутствие прав при установке (решено дефолтным префиксом `~/.local`).

## RUSTYOS-015 --- High-Contrast Visual Styling & Design System 2.0 (RustyOS Purple Glass)

**Статус:** done **Фаза:** 11 (Product Evolution)

- **Цель:** Приведение оверлеев оболочки (`TopBar`, `Dock`, фон) в полное соответствие с дизайн-системой Version 2.0 ("RustyOS Purple Glass") с высоким контрастом и читаемостью.
- **Scope:** Обновление дизайн-токенов в `crates/rusty-compositor/src/shell_render.rs` (`COLOR_SURFACE_ELEVATED`, `COLOR_SURFACE_OVERLAY`, `COLOR_ACCENT`, `COLOR_BORDER`, `COLOR_SUCCESS`); 1px внешняя полупрозрачная рамка (glass rim) для Dock и нижняя разделительная полоса для TopBar; яркие контрастные индикаторы воркспейсов и статусный апплет; обновление `BACKGROUND_COLOR` в `winit_backend.rs` на `#0F0A19`; модульное разделение на `render_top_bar` и `render_dock`; фиксация спецификации в `DESIGN.md` и референсов в `docs/design/references/`.
- **Dependencies:** `RUSTYOS-012`, `RUSTYOS-014`.
- **Acceptance criteria:** Элементы интерфейса не сливаются с темным фоном; TopBar и Dock четко выделены контрастными рамками и подложками; активные воркспейсы и окна имеют яркую неоновую индикацию; все 28 юнит-тестов проходят; clippy и rustfmt проходят без ошибок.
- **Risks:** Нулевые (локальное обновление визуальных констант и геометрии элементов оверлея).

## RUSTYOS-016 --- Wayland Layer-Shell Protocol (`zwlr_layer_shell_v1`)

**Статус:** done **Фаза:** 12 (Product Evolution)

- **Цель:** Реализовать поддержку протокола `zwlr_layer_shell_v1` в композиторе для независимых панелей, дока, оверлеев и автоматического расчёта эксклюзивной рабочей зоны.
- **Scope:** Крейт `crates/rusty-compositor`: глобал `WlrLayerShellState`, реализация трейта `WlrLayerShellHandler`, макрос `delegate_layer_shell!`, маппинг `LayerMap` на выход (`Output`), обработка коммитов и начального конфигурирования layer surface, динамический пересчёт `non_exclusive_zone` и синхронизация рабочего пространства окон с `WindowManager`, поддержка правильного порядка слоёв (`Overlay`, `Top`, `Bottom`, `Background`) в `target_surface_under` для фокуса мыши, fallback на встроенные оверлеи при отсутствии внешних клиентов layer-shell.
- **Dependencies:** `RUSTYOS-012`, `RUSTYOS-015`.
- **Acceptance criteria:** Глобал `zwlr_layer_shell_v1` регистрируется на Wayland display; клиенты layer-shell успешно маппятся на `LayerMap`; рабочая область окон динамически адаптируется под exclusive zone; юнит-тесты и clippy проходят.
- **Risks:** Коллизия событий ввода между окнами и панелями (решено строгой иерархией слоёв в `target_surface_under`).

## RUSTYOS-017 --- Native Layer-Shell Client in `rusty-shell`

**Статус:** done **Фаза:** 13 (Product Evolution)

- **Цель:** Реализовать клиентскую привязку к протоколу `zwlr_layer_shell_v1` в `apps/rusty-shell`, превратив процесс шелла в полноценное графическое Wayland-приложение.
- **Scope:** Модуль `apps/rusty-shell/src/wayland.rs` (`ShellWaylandClient`, `ShellClientData`, `render_top_bar_pixels`, `render_dock_pixels`); создание поверхностей слоёв `Top` (TopBar) и `Bottom` (Dock); выделение SHM буферов (`SlotPool`, `wl_shm::Format::Argb8888`); высококонтрастный рендеринг пикселей по токенам `DESIGN.md` v2.0 Purple Glass; флаги `--wayland` и `--daemon` в `main.rs` для запуска постоянного цикла синхронизации с композитором по IPC; юнит-тесты рендеринга буферов.
- **Dependencies:** `RUSTYOS-010`, `RUSTYOS-016`.
- **Acceptance criteria:** `rusty-shell` успешно соединяется с Wayland сервером по протоколу `zwlr_layer_shell_v1`; выделяет и коммитит ARGB8888 буферы; корректно рисует TopBar и Dock с активными воркспейсами и иконками; clippy и все 31 юнит-тест проходят без ошибок.
- **Risks:** Нулевые (клиент мягко деградирует в CLI/HUD режим, если Wayland-дисплей недоступен).

## Правило добавления задач

Каждая задача должна иметь ID, цель, scope, dependencies, acceptance
criteria, tests, risks и status. Детализировать в первую очередь
ближайшую фазу.
