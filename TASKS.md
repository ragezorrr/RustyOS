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

## RUSTYOS-018 --- Spotlight Application Launcher Overlay

**Статус:** done **Фаза:** 14 (Shell Components)

- **Цель:** Реализовать модальный Spotlight-лаунчер приложений по центру экрана по горячим клавишам `Super + Space` и `Alt + Space`.
- **Scope:** 
  - Горячие клавиши перехвата в композиторе (`Super+Space` / `Alt+Space`) в `crates/rusty-compositor/src/input.rs`.
  - Команда `ToggleLauncher` в `rusty-ipc`, поле `launcher_active: bool` в `DesktopStateSnapshot`.
  - Модальное окно `Layer::Overlay` в `apps/rusty-shell/src/wayland.rs` с живым поиском приложений и отрисовкой по токенам Purple Glass.
  - Фолбэк модального окна в GLES-пайплайне композитора `crates/rusty-compositor/src/shell_render.rs` для автономного режима без внешнего шелла.
  - Юнит-тесты перехвата хоткеев, сериализации IPC и генерации пикселей лаунчера.
- **Dependencies:** `RUSTYOS-016`, `RUSTYOS-017`.
- **Acceptance criteria:** Хоткеи `Super+Space` / `Alt+Space` переключают `launcher_active`; IPC синхронизирует состояние с `rusty-shell`; лаунчер рендерит окно поиска и результаты; 32 теста и clippy проходят без ошибок.
- **Risks:** Минимальные (гибридный подход обеспечивает работу как с внешним шеллом, так и в автономном композиторе).

## RUSTYOS-019 --- Server-Side Decorations (SSD Titlebar & macOS Traffic Lights)

**Статус:** done **Фаза:** 15 (Window Decorations & Controls)

- **Цель:** Добавить серверные декорации окон (верхняя планка заголовка `TITLEBAR_HEIGHT = 28`, 1px стеклянная рамка по контуру, кнопки управления «светофоры» Close/Minimize/Maximize) и перетаскивание окон мышью за заголовок.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render.rs`: `generate_window_ssd_elements`, `check_window_ssd_hit`, `SsdHit`.
  - `crates/rusty-compositor/src/winit_backend.rs`: единый Z-ordered рендеринг (`CompositorRenderElement`) поверх окон в правильном порядке перекрытия (TopBar/Spotlight -> [Окно + SSD] -> Bottom/Background).
  - `crates/rusty-compositor/src/state.rs` и `crates/rusty-wm/src/lib.rs`: смещение клиентской поверхности на `TITLEBAR_HEIGHT`, метод `WindowManager::move_window` и `AnimationEngine::snap_window_immediate`.
  - `crates/rusty-compositor/src/input.rs`: обработка кликов по «светофорам» и перетаскивание окон за шапку без задержек.
- **Dependencies:** `RUSTYOS-015`, `RUSTYOS-018`.
- **Acceptance criteria:** Каждое окно рендерится с заголовком, рамкой и кнопками управления; нажатие кнопок закрывает/центрирует/разворачивает окно; перетаскивание за заголовок перемещает окно в реальном времени; 34 теста и clippy проходят без ошибок.
- **Risks:** Перекрытие соседних окон решено пооконной сортировкой элементов в `CompositorRenderElement`.

## RUSTYOS-020 --- Font & Text Rendering (`fontdue`)

**Статус:** done **Фаза:** 16 (Typography & Readability)

- **Цель:** Добавить настоящий чёткий рендеринг шрифтов и UTF-8 типографики (включая кириллицу и латиницу) в `TopBar`, `Dock`, `Spotlight` и заголовки SSD-окон.
- **Scope:**
  - `Cargo.toml` / `Cargo.lock`: подключение `fontdue = "0.9.5"`.
  - `crates/rusty-compositor/src/text.rs`: `TextCache` для CPU-растеризации глифов в premultiplied `Argb8888` и кэширования GPU-текстур через `MemoryRenderBuffer`.
  - `crates/rusty-compositor/src/shell_render.rs` и `crates/rusty-compositor/src/winit_backend.rs`: текстовые элементы для рабочих столов `1..9`, бейджа режима (`Float`/`Tile`), статусной пилюли, инициалов Dock, строки поиска/элементов Spotlight и заголовков SSD-окон; скрытие прямоугольных заглушек при активном шрифте.
  - `apps/rusty-shell/src/wayland.rs`: отрисовка реального текста поверх SHM-буферов `TopBar` и `Launcher`.
- **Dependencies:** `RUSTYOS-018`, `RUSTYOS-019`.
- **Acceptance criteria:** При наличии системного шрифта (или `RUSTYOS_FONT_PATH`) отображаются чёткие подписи вместо заглушек; без шрифта происходит безопасный fallback; 35 тестов и clippy проходят без предупреждений.
- **Risks:** Минимальные (текстуры кэшируются по ключу `(text, size_tenths, rgba)` без аллокаций на каждый кадр).

## RUSTYOS-018b --- Interactive Spotlight (Input, Search Filtering & App Launching)

**Статус:** done **Фаза:** 17 (Interactive Shell Workflow)

- **Цель:** Сделать Spotlight полноценным интерактивным лаунчером с вводом UTF-8 текста, фильтрацией `.desktop`-приложений, навигацией и асинхронным запуском внутри Wayland-сессии.
- **Scope:**
  - `crates/rusty-compositor/src/launcher.rs`: `LauncherState`, парсер XDG `.desktop` файлов со снятием `%f`/`%U` кодов, scoring-фильтрация, оконный скролл, fallback на произвольную команду (`Run "..."`), безопасный `Command::spawn` с `WAYLAND_DISPLAY`/`RUSTYOS_IPC_SOCKET` и неблокирующим `reap_children`.
  - `crates/rusty-compositor/src/input.rs` и `state.rs`: изоляция клавиатуры и мыши при активном Spotlight (`Escape` закрывает лаунчер без выхода из композитора, `Up`/`Down`/`Tab`/`Enter`/`BackSpace`/`Ctrl+U`, клик по строке запускает приложение, клик снаружи закрывает).
  - `crates/rusty-compositor/src/shell_render.rs` и `winit_backend.rs`: `SpotlightRenderState`, `SpotlightHit`, динамическое позиционирование каретки курсора и отрисовка до 3 видимых совпадений с бейджами.
  - `crates/rusty-ipc/src/lib.rs` и `apps/rusty-shell/src/lib.rs`: обратная совместимая синхронизация `launcher_query` и команда `ShellRequest::LaunchCommand`.
- **Dependencies:** `RUSTYOS-018`, `RUSTYOS-020`.
- **Acceptance criteria:** Набор текста фильтрует список приложений и двигает каретку; `Up`/`Down`/`Enter` или клик мышью запускают выбранное приложение в композитор; `Escape` закрывает лаунчер; 40 тестов и clippy проходят без предупреждений.
- **Risks:** Минимальные (при отсутствии `WAYLAND_DISPLAY`-совместимого бинарника ошибка запуска логируется без падения композитора; дочерние процессы жнутся неблокирующим `try_wait`).

## RUSTYOS-021 --- Quick Settings Popover & Live System Telemetry

**Статус:** done **Фаза:** 18 (Control Center & Live Telemetry)

- **Цель:** Добавить выпадающий Quick Settings Popover по клику на статусную пилюлю TopBar (или `Alt+S`) и живые системные метрики (`Power`, `Network`, `Audio`, локальные часы, `p50` кадра) без блокировки event loop композитора.
- **Scope:**
  - `crates/rusty-services/src/lib.rs`: `AudioService::set_volume`, `AudioService::toggle_mute` (`wpctl`/`pactl`), `format_wpctl_volume` и `local_clock_hhmm`.
  - `crates/rusty-compositor/src/telemetry.rs`: фоновый поток `SystemMetricsHandle` (`start_background()`), кэширующий опрос сервисов раз в 3 с и асинхронно выполняющий изменения громкости/mute без задержки кадра.
  - `crates/rusty-compositor/src/shell_render.rs` и `winit_backend.rs`: живая статусная пилюля TopBar, кликабельный бейдж раскладки (`Float`/`Tile`), карточка Quick Settings (`QuickSettingsRenderState`, `ShellOverlayView`, `QuickSettingsHit`) с плитками сети/питания, ползунком и кнопками громкости, а также тумблерами `Layout`, `Reduced Motion`, `Open Spotlight` и `Refresh Metrics`.
  - `crates/rusty-compositor/src/input.rs` и `state.rs`: перехват кликов TopBar/QuickSettings и закрытие по `Escape` или клику вне карточки.
- **Dependencies:** `RUSTYOS-011`, `RUSTYOS-018b`, `RUSTYOS-020`.
- **Acceptance criteria:** Клик по правой пилюле TopBar или `Alt+S` открывает Quick Settings; громкость/mute, раскладка и `reduced_motion` работают без блокировки event loop; 43 теста и clippy проходят без предупреждений.
- **Risks:** Минимальные (без PipeWire/PulseAudio UI обновляется оптимистично и не роняет композитор).

## RUSTYOS-022 --- Rounded Glass Aesthetics, Soft Shadows & Interactive Mouse Resize

**Статус:** done **Фаза:** 19 (Visual Polish & Ergonomics)

- **Цель:** Добавить скруглённые углы панелей, оверлеев и SSD-заголовков (`Purple Glass`), мягкие внешние тени, круглые кнопки traffic lights в стиле macOS и интерактивный ресайз окон мышью за края и углы.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces.rs`: кэшируемый на GPU `UiShapeCache` с антиалиасированными SDF-патчами (`circle_element`, `corner_element`, `shadow_corner_element`) в формате premultiplied `Argb8888`.
  - `crates/rusty-compositor/src/shell_render.rs` и `winit_backend.rs`: единый `CompositorRenderElement`, 9-slice рендеринг скруглённого стекла без наложения пикселей (`push_rounded_glass_box`), внешние тени (`push_soft_shadow`, `generate_window_shadow_elements`), круглые traffic lights (`generate_window_ssd_composite_elements`), композитный UI-слой (`generate_shell_composite_elements`) и детекция границ/углов окна (`ResizeEdges`, `SsdHit::Resize`).
  - `crates/rusty-wm/src/lib.rs`, `crates/rusty-compositor/src/state.rs` и `input.rs`: `WindowManager::resize_window`, `WindowResizeState`, `compute_resized_rect` с минимальными габаритами `240x160` и защитой `TOP_BAR_HEIGHT`.
- **Dependencies:** `RUSTYOS-019`, `RUSTYOS-020`, `RUSTYOS-021`.
- **Acceptance criteria:** Все скругления, тени и круглые traffic lights кэшируются на GPU; ресайз за любой край/угол работает без перекрытия TopBar; 46 тестов и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Минимальные (CPU-растеризация выполняется однократно на уникальный размер/радиус/цвет с кэшированием `MemoryRenderBuffer`).

## RUSTYOS-023 --- Interactive Dock & Workspace Overview

**Статус:** done **Фаза:** 20 (Interactive Dock & Expose Overview)

- **Цель:** Сделать нижний Dock интерактивным (запуск терминала/файлов, переключение Workspace Overview, фокусировка открытых окон по плиткам с hover-подсветкой) и реализовать режим Workspace Overview (Expose-сетка карточек окон с выбором мышью и закрытием по `Esc`).
- **Scope:**
  - `crates/rusty-compositor/src/launcher.rs`: `LauncherState::launch_preferred` с проверкой наличия бинарника в `PATH` без блокирующего запуска подпроцессов и fallback в Spotlight.
  - `crates/rusty-compositor/src/shell_render.rs` и `winit_backend.rs`: единая геометрия `dock_slot_rect` / `overview_card_rects`, структуры `DockWindowEntry`, `DockViewState`, `DockHit`, `OverviewRenderState`, `OverviewHit`, `ShellOverlayView::Overview`, рендер Overview-сетки ниже `TopBar`/`Dock` для сохранения их кликабельности и unit-тесты hit-testing.
  - `crates/rusty-compositor/src/state.rs` и `input.rs`: отслеживание `dock_hover_slot`, стабильная сортировка окон `ordered_visible_window_ids`, `handle_dock_hit`, `focus_ordered_window`, горячие клавиши `Super+Tab` / `Alt+E` / `Escape` и обработка кликов по Dock и карточкам Overview.
- **Dependencies:** `RUSTYOS-018b`, `RUSTYOS-020`, `RUSTYOS-022`.
- **Acceptance criteria:** Клик по слотам Dock (`>_`, `Fs`, `Ov`, открытые окна) запускает/фокусирует соответствующие окна или переключает Overview; Overview отображает сетку превью-карточек активного рабочего стола без перекрытия TopBar/Dock; 47 тестов и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Минимальные (при отсутствии установленного терминала/файлового менеджера клик открывает Spotlight).

## RUSTYOS-024 --- Notification HUD & IPC Shell Parity

**Статус:** done **Фаза:** 21 (Notifications & IPC Parity)

- **Цель:** Добавить живой Notification HUD (правый верхний стек тост-баннеров со срочностью, автоистечением по TTL, кликом для скрытия и счётчиком непрочитанных в TopBar) с полной синхронизацией между `rusty-compositor`, `rusty-ipc` и `apps/rusty-shell`.
- **Scope:**
  - `crates/rusty-ipc/src/lib.rs`: `NotificationSummary`, поле `notifications` в `DesktopStateSnapshot` с безопасным `%`-экранированием и обратной совместимостью с 11-польными v1-снимками, запросы `DismissNotification` и `ClearNotifications`.
  - `crates/rusty-compositor/src/state.rs`: `CompositorNotification`, очередь уведомлений (`push_notification`, `expire_notifications`, `dismiss_notification`, `clear_notifications`, `trigger_demo_notification`) и обработка IPC-событий.
  - `crates/rusty-compositor/src/shell_render.rs` и `winit_backend.rs`: `NotificationToastView`, скруглённые стеклянные тосты (`Low`/`Normal`/`Critical` акценты, тень, кнопка `x`), счётчик `N:<count>` в TopBar, скрытие тостов при открытом Quick Settings/Spotlight и unit-тесты hit-testing.
  - `crates/rusty-compositor/src/input.rs`: хоткеи `Alt+N` (демо-уведомление) и `Alt+Shift+N` (очистка), клик по тосту или бейджу TopBar.
  - `apps/rusty-shell/src/lib.rs` и `wayland.rs`: `TopBarState::notification_count`, `NotificationCenter::sync_from_snapshot` / `dismiss` / `clear`, отрисовка бейджа `N:<count>` в SHM-буфере TopBar.
- **Dependencies:** `RUSTYOS-010`, `RUSTYOS-021`, `RUSTYOS-022`.
- **Acceptance criteria:** `Alt+N` и IPC `Notify` создают тост в правом верхнем углу с цветовым акцентом urgency; тосты автоматически истекают по TTL или закрываются кликом / `Alt+Shift+N`; `rusty-shell` синхронизирует очередь по `DesktopStateSnapshot`; 48 тестов и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Минимальные (размер снимка IPC ограничен до `MAX_IPC_MESSAGE_BYTES` с усечением до 8 видимых уведомлений).

## RUSTYOS-025 --- Lock Screen UI, Ambient Aurora Wallpaper & Animated Shell Transitions

**Статус:** done **Фаза:** 22 (Lock Screen, Wallpaper & Shell Motion)

- **Цель:** Добавить интерактивный Lock Screen (`Super+L` / `Ctrl+Alt+L` / IPC `SetLockScreen`), GPU-кэшируемые обои в стиле `Purple Glass` (аврора-градиент вместо плоского фона) и монотонные анимированные переходы shell-оверлеев (`Overview`, `Spotlight`, `Quick Settings`, `Lock Screen`) с поддержкой `reduced_motion`.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces.rs`: кэшируемый `MemoryRenderBuffer` аврора-обоев (`wallpaper_element`, `sample_wallpaper_pixel`, `rasterize_wallpaper_buffer`) и unit-проверка пикселей обоев.
  - `crates/rusty-compositor/src/state.rs` и `lib.rs`: каналы `overview_anim`, `launcher_anim`, `quick_settings_anim`, `lock_screen_anim` на базе `AnimatedValue`, методы `set_screen_locked` и `set_reduced_motion_pref` с изоляцией клавиатуры/указателя при блокировке и обработкой IPC `SetLockScreen`/`SetReducedMotion`.
  - `crates/rusty-compositor/src/shell_render.rs` и `winit_backend.rs`: `LockScreenRenderState`, `generate_lock_screen_elements` поверх всех слоёв, `check_lock_screen_unlock_click`, плавное появление/скрытие `Spotlight`, `QuickSettings`, `Overview` и `LockScreen` с единым смещением и фейдом фона, вывод обоев задним слоем.
  - `crates/rusty-compositor/src/input.rs`: перехват `Super+L`/`Ctrl+Alt+L` для блокировки, разблокировка по `Enter`/`Space`/`Escape` или клику `Unlock Session`, подавление клиентского ввода и ховера в состоянии блокировки, тесты горячих клавиш и hit-testing.
- **Dependencies:** `RUSTYOS-008`, `RUSTYOS-021`, `RUSTYOS-022`, `RUSTYOS-023`.
- **Acceptance criteria:** `Super+L`/`Ctrl+Alt+L` и IPC `SetLockScreen` переводят композитор в экран блокировки с изоляцией ввода клиентских окон; все shell-оверлеи анимируются и мгновенно переключаются при `reduced_motion`; на фоне отображаются кэшируемые аврора-обои; 49 тестов и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Минимальные (CPU-растеризация `256x160` обоев выполняется однократно при старте и масштабируется на GPU).

## RUSTYOS-026 --- XWayland & D-Bus Notifications System Integration

**Статус:** done **Фаза:** 23 (System Integration: XWayland & D-Bus)

- **Цель:** Обеспечить совместимость с X11-приложениями через встроенный `XWayland` + `X11Wm` и принимать системные уведомления (`notify-send` / D-Bus) через сервер `org.freedesktop.Notifications` в наш Notification HUD без блокировки event loop композитора.
- **Scope:**
  - `Cargo.toml`, `Cargo.lock`, `crates/rusty-compositor/Cargo.toml`: включена фича `smithay/xwayland` и добавлен крейт `zbus` (`5.19`, async-io/blocking API).
  - `crates/rusty-compositor/src/xwayland.rs`: асинхронный запуск `XWayland` через `calloop`, реализация `XWaylandShellHandler` и `XwmHandler` (`X11Wm`), маппинг X11-окон в `WindowManager` и `Space` с учётом высоты SSD-заголовка (`compute_x11_client_rect`), обработка `override_redirect` всплывающих окон, заголовков, максимизации, перетаскивания, ресайза и закрытия (`WM_DELETE_WINDOW`).
  - `crates/rusty-compositor/src/dbus_notifications.rs`: фоновый поток `DbusNotificationHandle` с интерфейсом `org.freedesktop.Notifications` (и безопасным fallback на `org.rustyos.Notifications` при занятом имени в nested-сессиях), парсингом `urgency`, `expire_timeout` и `replaces_id`/`CloseNotification`.
  - `crates/rusty-compositor/src/state.rs`, `launcher.rs`, `winit_backend.rs`, `lib.rs`: сквозная передача `DISPLAY=:<N>` дочерним приложениям Spotlight/Dock, синхронизация фокуса через `WaylandFocus`, дренаж D-Bus уведомлений в цикле диспетчеризации и 3 новых unit-теста.
- **Dependencies:** `RUSTYOS-011`, `RUSTYOS-022`, `RUSTYOS-024`.
- **Acceptance criteria:** При наличии `Xwayland` запускается X11 WM и X11-окна управляются `rusty-wm` наравне с Wayland-окнами; `LauncherState` передаёт `DISPLAY` запускаемым приложениям; фоновый D-Bus сервер принимает уведомления в HUD с поддержкой `replaces_id` и graceful fallback без падения при отсутствии шины; 52 теста и `clippy -D warnings` проходят без предупреждений.
- **Risks:** При отсутствии бинарника `Xwayland` или сессионной шины D-Bus соответствующие подсистемы логируют предупреждение и отключаются без нарушения работы композитора.

## RUSTYOS-027 --- System Integration ч. 2: BlueZ, NetworkManager D-Bus & Quick Settings Bluetooth

**Статус:** done **Фаза:** 24 (System Integration: NetworkManager, BlueZ & Control Center)

- **Цель:** Перевести системные сервисы сети и Bluetooth на прямые D-Bus запросы (`org.freedesktop.NetworkManager` и `org.bluez`) с CLI/sysfs fallback и добавить интерактивные плитки `Bluetooth` и `Lock Session` в Quick Settings.
- **Scope:**
  - `crates/rusty-services/Cargo.toml` и `lib.rs`: подключён `zbus`; `NetworkService` читает активное соединение и `SSID`/`Id` напрямую с System D-Bus `NetworkManager` (с fallback на `/sys/class/net` и `nmcli`); добавлены `BluetoothStatus` и `BluetoothService` (опрос `org.bluez.Adapter1`/`Device1` и управление `Powered` по D-Bus или `bluetoothctl`).
  - `crates/rusty-compositor/src/telemetry.rs`: добавлено поле `bluetooth: BluetoothStatus` в `SystemTelemetry` и оптимистичный метод `toggle_bluetooth()` с асинхронной записью в фоне.
  - `crates/rusty-compositor/src/shell_render.rs`, `winit_backend.rs` и `input.rs`: расширена карточка `QuickSettings` (высота `326`), добавлен 3-й ряд кнопок `ToggleBluetooth` (`BT: On`/`Off`/`<device>`) и `LockSession` с hit-testing и тестами.
- **Dependencies:** `RUSTYOS-011`, `RUSTYOS-021`, `RUSTYOS-025`, `RUSTYOS-026`.
- **Acceptance criteria:** Сеть и Bluetooth опрашиваются через System D-Bus без блокировки event loop с безопасным fallback; клик по плитке Bluetooth оптимистично обновляет UI и переключает питание адаптера в фоне; клик `Lock Session` мгновенно включает Lock Screen; 53 теста и `clippy -D warnings` проходят без предупреждений.
- **Risks:** При отсутствии системной шины D-Bus, `NetworkManager` или `bluetoothd` сервисы корректно деградируют на `/sys/class/*` или статус `Unavailable`.

## RUSTYOS-028 --- Native TTY DRM/KMS & `libinput` Render Loop

**Статус:** done **Фаза:** 25 (Native TTY DRM/KMS Scanout & Input)

- **Цель:** Перевести `--drm` / `--kms` из режима однократного hardware-пробинга в полноценный нативный TTY render loop с `LibSeatSession`, `DrmDevice` + `DrmSurface` + `DrmCompositor`, `LibinputInputBackend` (мышь/тачпад/клавиатура), VT-переключением (`Ctrl+Alt+F1..F7`) и общим композитингом shell + Wayland + XWayland, сохранив безопасный probe-fallback внутри активной графической сессии.
- **Scope:**
  - `crates/rusty-compositor/src/input.rs`: подключено относительное движение указателя `InputEvent::PointerMotion` (`libinput` мыши/тачпады) через общую функцию `apply_pointer_position`, чистый кламп `clamp_relative_pointer` и unit-тест.
  - `crates/rusty-compositor/src/winit_backend.rs`: выделен общий `build_frame_render_list` (с опциональным software-курсором `Purple Glass` для универсальной совместимости AMD/Intel/NVIDIA без зависимости от hardware cursor planes) и переиспользуемые `dispatch_and_refresh_clients`, `finalize_frame_timing`, `spawn_test_client`.
  - `crates/rusty-compositor/src/drm_backend.rs`: реализованы `select_preferred_drm_mode`, `mode_refresh_interval_us`, поиск активного коннектора/CRTC (`find_primary_connector_setup`), цикл `DrmCompositor` (`GbmAllocator`, `GbmFramebufferExporter`), `LibinputInputBackend` с мгновенной коалесценцией перерисовки, `DrmEvent::VBlank` pacing, `SessionEvent::PauseSession`/`ActivateSession` (`reset_buffers`), безопасный fallback на `probe_primary_drm_device` при занятом DRM Master и unit-тесты.
- **Dependencies:** `RUSTYOS-006`, `RUSTYOS-019`, `RUSTYOS-025`, `RUSTYOS-026`.
- **Acceptance criteria:** Из TTY (`Ctrl+Alt+F3`) `--drm` захватывает seat/DRM Master, поднимает Wayland/XWayland/IPC, рендерит оболочку с software-курсором на частоте монитора по `VBlank` и корректно переживает VT switch (включая рендер 24x24 стрелки курсора и восстановление DRM/libinput при возврате `Ctrl+Alt+F1` -> `Ctrl+Alt+F3`); внутри вложенной сессии безопасно падает на диагностику `probe_primary_drm_device`; 56 тестов и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Требуется живая проверка на реальном железе из TTY (`Ctrl+Alt+F3`); в nested-окружении выполняется только компиляция, юнит-тесты и fallback-диагностика.

## RUSTYOS-029 --- Конфиг пользователя (`config.toml`), RU/EN раскладка и Clipboard Wayland↔XWayland

**Статус:** done **Фаза:** 26 (Daily Usability: Config, XKB Layout & Clipboard)

- **Цель:** Добавить пользовательский конфиг `~/.config/rustyos/config.toml` без внешних зависимостей, поддержку многораскладочной XKB-клавиатуры (`us,ru`) с живым кликабельным индикатором `EN`/`RU` в `TopBar`, а также двусторонний буфер обмена между Wayland-клиентами и `XWayland` (X11).
- **Scope:**
  - `crates/rusty-compositor/src/config.rs` и `lib.rs`: нулезависимый парсер подмножества TOML `UserConfig` (`[keyboard]` `layout`, `variant`, `options`, `repeat_delay`, `repeat_rate`; `[ui]` `reduced_motion`; `[autostart]` `commands`), поиск `$XDG_CONFIG_HOME/rustyos/config.toml` -> `~/.config/rustyos/config.toml` с безопасными дефолтами (`us,ru`, `grp:alt_shift_toggle`) и маппингом `layout_short_code`.
  - `crates/rusty-compositor/src/state.rs`, `input.rs`, `shell_render.rs`: загрузка `UserConfig` при инициализации `Seat` (с fallback на US XKB при некорректном конфиге), синхронизация `active_kbd_layout` и кликабельный бейдж `EN`/`RU` в `TopBar` (сдвинутый левее бейджа уведомлений), перенос `set_data_device_focus` / `set_primary_focus` в `SeatHandler::focus_changed`, поддержка `zwp_primary_selection_v1` и `zwlr_data_control_manager_v1`, а также реализация `SelectionHandler` для Wayland -> `X11Wm` с немедленным flush X11-сокета.
  - `crates/rusty-compositor/src/xwayland.rs`: сохранение `x11_loop_handle` из `XWaylandEvent::Ready` и полная реализация `XwmHandler::allow_selection_access`/`new_selection`/`send_selection`/`cleared_selection` для `Clipboard` и `Primary` (с защитой чтения при `screen_locked` и немедленным flush Wayland-клиентов).
  - `crates/rusty-compositor/src/winit_backend.rs` и `drm_backend.rs`: применение `user_config.reduced_motion`, запуск команд `user_config.autostart` при старте бэкендов и передача `active_kbd_layout` в `TopBarTelemetry`.
- **Dependencies:** `RUSTYOS-008`, `RUSTYOS-025`, `RUSTYOS-026`, `RUSTYOS-028`.
- **Acceptance criteria:** Отсутствующий или частично заполненный `config.toml` не ломает запуск и применяет безопасные значения по умолчанию; `Alt+Shift` и клик по бейджу `EN`/`RU` в `TopBar` переключают раскладку с синхронным обновлением текста бейджа; копирование/вставка (`Clipboard` и `Primary`) работает в обе стороны между Wayland и X11 (`XWayland`); `autostart` запускает команды без блокировки композитора; 61 тест и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Для сложных многострочных структур за пределами поддерживаемого подмножества TOML (таблицы в одну строку и т. п.) рекомендуется использовать стандартный синтаксис секций `[section]` + `key = value` / `commands = [...]`.

## RUSTYOS-030 --- Убрать двойные заголовки CSD-окон и выровнять геометрию X11/Wayland

**Статус:** done **Фаза:** 27 (CSD/SSD Negotiation & Geometry Alignment)

- **Цель:** Устранить двойные заголовки у CSD-окон (Electron / Cursor / GTK4 / XWayland `_MOTIF_WM_HINTS` и Wayland `zxdg_decoration_manager_v1`), убрать гонку инициализации Qt (`Konsole`) и выровнять внешнюю рамку окна с фактической геометрией клиента.
- **Scope:**
  - `crates/rusty-compositor/src/state.rs` и `winit_backend.rs`: отложенная отправка первичного `xdg_toplevel::configure` в `flush_pending_toplevel_configures` после завершения батча запросов `zxdg_toplevel_decoration_v1` (`new_decoration`/`request_mode`), учёт `DecorationMode::ClientSide` и клиентов без `xdg-decoration` (`csd_window_ids`), динамическая высота заголовка (`window_titlebar_height`), авторасширение плавающих окон под `min_size` (`ensure_floating_min_size`) и обработчики `move_request`, `resize_request`, `maximize_request`, `unmaximize_request`, `toplevel_destroyed`.
  - `crates/rusty-compositor/src/xwayland.rs`: добавлена `compute_x11_client_rect_for_decoration`, учёт `!window.is_decorated()` (`_MOTIF_WM_HINTS` CSD), передача `preferred_size` из геометрии/`min_size` при маппинге и обработка `WmWindowProperty::MotifHints`/`NormalHints` в `property_notify`.
  - `crates/rusty-compositor/src/shell_render.rs`, `input.rs`, `winit_backend.rs`: добавлена `check_window_frame_hit` (не перехватывает заголовок CSD и сохраняет внешний ресайз), CSD-окна рендерятся без навязанной внешней рамки композитора, `frame_rect` в `Floating` выровнен по `committed_size`, добавлено перетаскивание окон через `Alt+LMB` / `Super+LMB`.
- **Dependencies:** `RUSTYOS-019`, `RUSTYOS-022`, `RUSTYOS-026`, `RUSTYOS-028`.
- **Acceptance criteria:** `Konsole` и `Dolphin` стабильно получают `ServerSide` SSD-шапку RustyOS без Breeze `_ □ ×`; `Cursor Agents` / Electron и GTK4 CSD-окна рендерятся с одной шапкой без внешней рамки композитора и с кликабельными кнопками в углах; перетаскивание (`Alt+LMB`/`Super+LMB` и client `move_request`) и ресайз по краям работают на CSD-окнах; 63 теста и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Требуется визуальная проверка в живой сессии на `--winit` / `--drm` с Electron (`Cursor Agents`) и Qt (`Konsole`, `Dolphin`).

## RUSTYOS-031 --- Медиа-клавиши, яркость экрана (Backlight), скриншоты и OSD-индикатор

**Статус:** done **Фаза:** 28 (Hardware Keys, Backlight, Screenshots & OSD)

- **Цель:** Добавить неблокирующую поддержку аппаратных клавиш громкости/mute, яркости экрана (backlight), медиа-клавиш (MPRIS2/`playerctl`), быстрых скриншотов (весь экран и активное окно) с автосохранением в PNG и буфер обмена, а также компактный стеклянный OSD-индикатор над Dock.
- **Scope:**
  - `crates/rusty-services/src/lib.rs`: добавлены `BrightnessStatus`, `BrightnessService` (`/sys/class/backlight` чтение/запись + `systemd-logind` D-Bus `SetBrightness` + `brightnessctl` fallback) и `MediaService` (`org.mpris.MediaPlayer2.Player` over Session D-Bus + `playerctl` fallback).
  - `crates/rusty-compositor/src/telemetry.rs` и `state.rs`: асинхронные команды `step_volume_with_osd`, `toggle_mute_with_osd`, `step_brightness_with_osd`, `dispatch_media`, состояние `OsdBanner` (TTL `1.6s`) и очередь `ScreenshotResultQueue`.
  - `crates/rusty-compositor/src/screenshot.rs`: zero-dependency захват кадра через offscreen `GlesTexture` (`ExportMem::copy_texture`), чистый PNG-кодер RGBA в фоновом потоке (`~/Pictures/Screenshots/RustyOS_*.png`) и копирование в буфер через `wl-copy --type image/png`.
  - `crates/rusty-compositor/src/shell_render.rs`, `input.rs`, `winit_backend.rs`, `drm_backend.rs`: стеклянный OSD-виджет (`generate_osd_elements`), обработка `XF86_Audio*`, `XF86_MonBrightness*` (с `Shift` для точного шага `2%`), `Print` / `Alt+Print` / `Super+Shift+S` и общий рендер/захват для Winit и DRM/KMS.
- **Dependencies:** `RUSTYOS-011`, `RUSTYOS-021`, `RUSTYOS-024`, `RUSTYOS-028`.
- **Acceptance criteria:** Нажатие клавиш громкости/яркости/медиа мгновенно показывает стеклянный OSD над Dock и применяет изменения в фоне без блокировки event loop; `Print` сохраняет PNG всего экрана, `Alt+Print` / `Super+Shift+S` — активного окна, копирует в Wayland clipboard (`wl-copy`) и показывает уведомление; 68 тестов и `clippy -D warnings` проходят без предупреждений.
- **Risks:** На стационарных ПК без `/sys/class/backlight` клавиши яркости безопасно показывают `No Backlight` без ошибок; копирование картинки в clipboard использует опциональный `wl-copy`, если установлен в системе.

## RUSTYOS-032 --- Системный трей (`StatusNotifierItem`) в TopBar и сворачивание/восстановление окон из Dock

**Статус:** done **Фаза:** 29 (System Tray & Window Minimize/Restore)

- **Цель:** Добавить неблокирующий D-Bus сервер системного трея (`org.kde.StatusNotifierWatcher` / `StatusNotifierItem`) с интерактивными значками в `TopBar` (ЛКМ/ПКМ без блокировки event loop) и полноценное сворачивание/восстановление окон через жёлтую кнопку traffic-light, `Alt+H`, запросы клиентов (`xdg_toplevel::minimize_request` / XWayland `XwmHandler::minimize_request`) и переключение через `Dock`.
- **Scope:**
  - `crates/rusty-compositor/src/tray.rs`: фоновый поток `TrayWatcherHandle` с D-Bus-сервером `org.kde.StatusNotifierWatcher` (fallback `org.rustyos.StatusNotifierWatcher` и синхронизация существующих элементов в nested `--winit` режиме), парсер `parse_sni_service_and_path`, чтение `Id`/`Title`/`IconName`/`Status`/`ItemIsMenu`, автоочистка завершившихся апплетов и асинхронный вызов `Activate`/`ContextMenu`/`SecondaryActivate`.
  - `crates/rusty-compositor/src/state.rs` и `xwayland.rs`: отслеживание `minimized_window_ids`, методы `minimize_window`, `restore_window`, `is_window_minimized`, `drain_tray_updates`, обработка `xdg_toplevel::minimize_request` и X11 `minimize_request`/`unminimize_request`, автоматический перевод клавиатурного фокуса на следующее видимое окно и защита `commit`/`Alt+Tab`/`CloseFocused` от работы с уже свёрнутым окном.
  - `crates/rusty-compositor/src/shell_render.rs`, `input.rs`, `winit_backend.rs`, `drm_backend.rs`: компактные кликабельные бейджи `TopBarTrayItem` левее индикатора раскладки `EN`/`RU` (ЛКМ `Activate`, ПКМ `ContextMenu`, СКМ `SecondaryActivate`), переключение свёрнутых плиток в `Dock` (`minimized` индикатор 4px янтарной точкой, клик сворачивает активное окно или разворачивает свёрнутое), хоткей `Alt+H` и обход свёрнутых окон в `check_window_frame_hit`/`build_frame_render_list`.
- **Dependencies:** `RUSTYOS-023`, `RUSTYOS-026`, `RUSTYOS-029`, `RUSTYOS-030`.
- **Acceptance criteria:** Апплеты `StatusNotifierItem` регистрируются на шине D-Bus и отображаются компактными бейджами в `TopBar` с реакцией на ЛКМ/ПКМ без задержки кадров; нажатие жёлтой кнопки заголовка, `Alt+H` или клик по активному окну в `Dock` скрывает окно со сцены и фокусирует следующее видимое окно, а повторный клик по плитке в `Dock` или карточке `Overview` восстанавливает его; 71 тест и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Настройка внешнего D-Bus трея внутри `--winit` сессии с уже активным системным треем хоста опирается на best-effort синхронизацию с хостовым `org.kde.StatusNotifierWatcher` (отдельная изолированная сессия `--drm` или `dbus-run-session` держит первичное имя напрямую).

## RUSTYOS-033 --- Визуальный Alt+Tab HUD, реальные иконки приложений и Drag-and-Drop

**Статус:** done **Фаза:** 30 (Visual Window Switcher, Real App Icons & Wayland DnD)

- **Цель:** Добавить чистый Rust-декодер и GPU-кэш реальных PNG- и SNI-иконок приложений (без новых зависимостей в `Cargo.toml`) для `Dock`, `Spotlight`, `TopBar` трея и переключателя окон, реализовать визуальный `Alt+Tab` Window Switcher HUD в стиле RustyOS Purple Glass, а также отрисовку Wayland Drag-and-Drop поверхности-иконки (`wl_data_device.start_drag`).
- **Scope:**
  - `crates/rusty-compositor/src/icons.rs`, `surfaces.rs`, `lib.rs`: нулезависимый декодер PNG (`IHDR`/`PLTE`/`tRNS`/`IDAT`, zlib/DEFLATE RFC 1951, фильтры `0..=4`), конвертер `TrayPixmap` (`ARGB32` BE -> premultiplied `Argb8888`), билинейный ресайзер, поиск XDG PNG-иконок по темам (`hicolor`/`Papirus`/`Adwaita`/`breeze`/`Yaru`/`pixmaps`) и GPU-кэш `IconCache` внутри `UiShapeCache`.
  - `crates/rusty-compositor/src/launcher.rs` и `tray.rs`: чтение `Icon=` из `.desktop`, сопоставление `icon_for_app_hints` по `app_id`/`WM_CLASS`/заголовку, выбор закреплённых иконок `preferred_icon` для Dock, чтение `IconPixmap` (`a(iiay)`) и `IconName` из D-Bus `StatusNotifierItem`.
  - `crates/rusty-compositor/src/state.rs`, `input.rs`, `xwayland.rs`: состояние `WindowSwitcherState` (`switcher_step`/`switcher_commit`/`switcher_commit_index`/`switcher_cancel`), отпуск `Alt` / `Enter` / клик по карточке подтверждают выбор и разворачивают свёрнутые окна, `Escape` отменяет без смены фокуса (`Alt+J`/`Alt+K` сохранены для быстрого переключения); трекинг `dnd_icon` через `ClientDndGrabHandler::started`/`dropped` с очисткой при закрытии или блокировке экрана.
  - `crates/rusty-compositor/src/shell_render.rs` и `winit_backend.rs`: рендер реальных иконок в `Dock` (`28x28`), `Spotlight` (`24x24`), `TopBar` трее (`16x16`) и `Alt+Tab` Switcher HUD (`36x36`) с сохранением 2-символьных монограмм при отсутствии PNG; рендер `dnd_icon` под курсором для Winit и DRM/KMS.
- **Dependencies:** `RUSTYOS-018b`, `RUSTYOS-023`, `RUSTYOS-026`, `RUSTYOS-032`.
- **Acceptance criteria:** Приложения с PNG-иконками XDG и апплеты `IconPixmap` отображают реальные иконки с GPU-кэшированием и корректно откатываются на бейджи при отсутствии PNG; `Alt+Tab` / `Alt+Shift+Tab` открывают центрированную стеклянную панель окон с MRU-порядком и свёрнутыми окнами, подтверждение по отпусканию `Alt` / `Enter` / клику фокусирует и разворачивает окно, `Escape` отменяет; перетаскивание (`wl_data_device`) рисует `dnd_icon` под курсором на Winit и DRM; 76 тестов и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Для иконок, доступных только в `.svg` без PNG-растров в темах системы, срабатывает чистый текстовый fallback (рендер SVG намеренно не добавлен ради сохранения нулевых внешних зависимостей).

## RUSTYOS-034 --- `xdg_popup` меню, клиентские/`XCursor` курсоры и кастомные обои из `config.toml`

**Статус:** done **Фаза:** 31 (Wayland Popups, Dynamic Cursor Themes & Custom Wallpaper)

- **Цель:** Починить позиционирование, первичную конфигурацию и ввод для `xdg_popup` меню и выпадающих списков, поддержать клиентские (`wl_pointer.set_cursor`), скрытые (`Hidden`) и системные (`XCursor`) курсоры в DRM/KMS и Winit, а также загрузку пользовательских `.png`-обоев из `~/.config/rustyos/config.toml` без добавления внешних зависимостей.
- **Scope:**
  - `crates/rusty-compositor/src/config.rs`: в `[ui]` добавлены опции `wallpaper` (с раскрытием `~/` в домашний каталог) и `cursor_theme` с unit-тестами.
  - `crates/rusty-compositor/src/icons.rs`: обобщён декодер PNG (`decode_png_rgba_limited`) и добавлен `load_png_wallpaper_bgra` с aspect-preserving center-crop (`cover`) билинейным масштабированием до 4K PNG в `Argb8888`.
  - `crates/rusty-compositor/src/surfaces.rs`: добавлен чистый Rust-парсер бинарных курсоров `XCursor` (`parse_xcursor_bytes`, поддержка `Inherits=` и алиасов `CursorIcon`, GPU-кэш `named_cursor_element` со смещением по hotspot и fallback на векторную стрелку), а также кэширование кастомных обоев по `(PathBuf, Size)` в `UiShapeCache::wallpaper_element` с fallback на аврора-градиент.
  - `crates/rusty-compositor/src/state.rs` и `input.rs`: трекинг `cursor_status` через `SeatHandler::cursor_image`, настройка ограничений геометрии `xdg_popup` (`popup_parent_target_rect` + `get_unconstrained_geometry`), отправка первичного `send_configure` и безусловный `popups.commit(surface)` (включая дочерние popup от layer-shell), трекинг `xdg_popup.grab` со сбросом по клику вне меню и по `Escape`, а также обход перехвата SSD-рамок/заголовков при клике по открытому `xdg_popup`.
  - `crates/rusty-compositor/src/winit_backend.rs`: синхронизация `Winit` курсора (`Hidden`, `Named(CursorIcon)`, клиентский `Surface` с hotspot) и полная отрисовка всех состояний `CursorImageStatus` для DRM/KMS (`include_software_cursor`), порядок слоя открытых popup поверх SSD-шапок окон и передача `user_config.wallpaper` в `wallpaper_element`.
- **Dependencies:** `RUSTYOS-016`, `RUSTYOS-025`, `RUSTYOS-028`, `RUSTYOS-029`, `RUSTYOS-033`.
- **Acceptance criteria:** Клиентские `xdg_popup` (контекстные меню, тултипы, выпадающие списки GTK/Qt/Electron) корректно конфигурируются в пределах экрана, перекрывают SSD-заголовки, реагируют на клики и закрываются по клику вне меню или `Escape`; `CursorImageStatus` (`Hidden`, клиентский `Surface`, системные `XCursor` формы) работает в DRM/KMS и Winit; при указании `[ui] wallpaper = "~/.../bg.png"` обои масштабируются по `cover` и кэшируются на GPU, при ошибке/отсутствии срабатывает fallback; 78 тестов и `clippy -D warnings` проходят без предупреждений.
- **Risks:** Для обоев поддерживается формат PNG (`.jpg`/`.webp` намеренно не добавлены для сохранения нулевых внешних зависимостей); для курсоров на хосте без установленных `XCursor`-тем используется встроенная векторная стрелка.

## RUSTYOS-035 --- Dock в стиле macOS 27 и скруглённые окна (DESIGN.md §12)

**Статус:** done **Фаза:** 32 (macOS 27 Visual Alignment)

- **Цель:** Перенести в код первую часть `DESIGN.md` §12: широкая капсула Dock с крупными иконками, круглые индикаторы запущенных окон, ховер без постоянных плиток, увеличенный радиус и мягкие тени окон.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render.rs`: константы `DOCK_HEIGHT = 72`, `DOCK_MARGIN_BOTTOM = 8`, `DOCK_ITEM_SIZE = 56`, `DOCK_ITEM_GAP = 8`, `DOCK_PADDING = 8`, `DOCK_RADIUS = 24`, `DOCK_ICON_SIZE = 48`, `WINDOW_RADIUS = 12`; капсула Dock с радиусом 24, более светлой полупрозрачной заливкой и широкой тенью; иконки `48x48` без постоянной плитки (ховер/активное окно — лёгкая подсветка); индикатор запущенного окна — круглая точка `4 px`; радиус SSD-рамки окна 12 и более широкие тени (`24`/`14`, alpha `110`/`56`).
- **Dependencies:** `RUSTYOS-023`, `RUSTYOS-033`, `RUSTYOS-034`.
- **Acceptance criteria:** Hit-testing Dock совпадает с отрисованными плитками `56x56`; рабочая область окон учитывает новую высоту Dock; `clippy -D warnings` и тесты проходят.
- **Risks:** Нижние углы окон остаются прямыми, так как клиентские поверхности не обрезаются шейдером (нужно решение по GLES, см. `DESIGN.md` §12.5). Разделитель Dock, прозрачная верхняя панель, иконки трея и Quick Settings остаются для следующих задач.

## RUSTYOS-036 --- Прозрачная верхняя панель с иконками статуса (DESIGN.md §12.1)

**Статус:** done **Фаза:** 32 (macOS 27 Visual Alignment)

- **Цель:** Привести `TopBar` к стилю macOS 27: без сплошного фона, компактная высота, вместо текстовой строки статуса — монохромные глифы и часы.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render.rs`: `TOP_BAR_HEIGHT` 32 → 26, константы `TOP_BAR_ITEM_Y`/`TOP_BAR_ITEM_H`/`TOP_BAR_GLYPH_CLUSTER_W`; сплошная заливка и нижняя граница заменены мягким скримом (три полосы с убывающей альфой); плашки раскладки, трея и статуса рисуются только при активном состоянии (текущий рабочий стол, `Tile`, открытые Quick Settings, `attention`); справа вместо `Vol .. • .. • HH:MM` — глифы сигнала сети, громкости/mute, батареи (заливка по проценту) и часы (`push_status_glyphs`); ширина статус-блока 200 → 168.
- **Dependencies:** `RUSTYOS-021`, `RUSTYOS-032`, `RUSTYOS-035`.
- **Acceptance criteria:** Hit-testing верхней панели совпадает с отрисовкой; рабочая область окон учитывает высоту 26; `clippy -D warnings` и тесты проходят.
- **Risks:** Глифы нарисованы прямоугольниками (без векторного шрифта иконок), форма упрощена; требуется визуальная проверка на светлых обоях. Меню активного приложения, иконка Control Center и поиск — отдельные задачи.

## RUSTYOS-037 --- Control Center и Quick Settings в стиле macOS 27 (DESIGN.md §12.4)

**Статус:** done **Фаза:** 32 (macOS 27 Visual Alignment)

- **Цель:** Обновить геометрию, карточки и элементы управления Quick Settings (Control Center) по `DESIGN.md` §12.4: внешний радиус `large` (20 px) с широкой мягкой тенью, скруглённые карточки `16 px`, круглые кнопки переключения `30 px`, капсульный слайдер громкости `18 px` с ведущей иконкой динамика и скруглёнными кнопками mute/step `11 px`, синхронизированный hit-testing.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render.rs`:
    - `quick_settings_rect`: размер `320x336` (вместо `304x326`), внешний радиус `20`, тень `inner_radius: 20`, `spread: 24`, `max_alpha: 110`.
    - Карточки Network & Power: размер `142x58`, радиус `16`, круглые кнопки управления диаметром `30 px` с центрированными индикаторами и монохромными/акцентными статусами.
    - Карточка Sound: размер `292x66`, радиус `16`, капсульный слайдер громкости высотой `18 px` (радиус `9 px`) с ведущим глифом динамика (и зачёркиванием при mute), акцентной заливкой и круглыми кнопками-пилюлями mute (`48x22`, радиус `11`) и `+/-` (`28x22`, радиус `11`).
    - Плитки быстрых действий: 6 скруглённых кнопок `142x34`, радиус `14` (Layout, Motion, Spotlight, Metrics, Bluetooth, Lock) с акцентной заливкой активных состояний (`Tile`, `Reduced Motion`, `Bluetooth On`).
    - Заголовок обновлён на "Control Center" с метриками `p50 ms / windows`.
    - Точный hit-testing в `check_quick_settings_click` синхронизирован со всеми новыми координатами.
- **Dependencies:** `RUSTYOS-021`, `RUSTYOS-027`, `RUSTYOS-036`.
- **Acceptance criteria:** Hit-testing Quick Settings совпадает с новой геометрией (все 55 тестов `rusty-compositor` и 78 тестов воркспейса проходят); `clippy -D warnings` и `cargo fmt` чистые; внешний вид полностью соответствует `DESIGN.md` §12.4.
- **Risks:** Нулевые новые зависимости; переключение Bluetooth и Lock Session сохранены в третьем ряду.

## RUSTYOS-038 --- Active App Menu и глобальный Menubar в TopBar (DESIGN.md §12.1)

**Статус:** done **Фаза:** 32 (macOS 27 Visual Alignment)

- **Цель:** Привести левую часть `TopBar` к архитектуре и стилю macOS 27 согласно `DESIGN.md` §12.1: логотип RustyOS (`✦`), имя текущего сфокусированного приложения полужирным шрифтом (`Terminal`, `Files`, `RustyOS`), глобальные пункты меню (`File`, `Edit`, `View`, `Window`), тонкий вертикальный разделитель и компактные круглые пилюли рабочих столов `18x18 px` с радиусом `9 px`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render.rs`:
    - Константы `TOP_BAR_LOGO_X = 12`, `TOP_BAR_LOGO_W = 18`, `TOP_BAR_APP_X = 34`, `TOP_BAR_APP_MAX_W = 78`, `TOP_BAR_MENUS_START_X = 118`, ширины меню `File` (28), `Edit` (28), `View` (32), `Window` (50), `TOP_BAR_SEPARATOR_X = 296`.
    - Компактные индикаторы воркспейсов: `WS_START_X = 308`, `WS_BTN_WIDTH = 18`, `WS_BTN_GAP = 3`, `WS_BTN_STRIDE = 21`, высота `18 px` с радиусом `9` (круглые пилюли, активный — акцентный фиолетовый `RGBA_ACCENT`, неактивные — полупрозрачные белые `[255, 255, 255, 14]`).
    - Функция `extract_app_name`: извлекает чистое имя приложения из заголовка окна XDG (с разделением по ` — ` и ` - ` и fallback на `RustyOS`).
    - Тип `TopBarMenuHit` и функция `check_top_bar_menu_click` для точного hit-testing меню (`SystemLogo`, `ActiveApp`, `File`, `Edit`, `View`, `Window`).
    - Обновление `check_top_bar_layout_click` для работы с компактным положением индикатора раскладки (`505..557 px`).
    - Отрисовка текста (`generate_shell_text_elements`) и GLES-композиция (`generate_shell_composite_elements`) с жирным именем активного приложения, меню и вертикальным разделителем.
  - `crates/rusty-compositor/src/input.rs`:
    - Интеграция `check_top_bar_menu_click` в обработчик кликов `TopBar`: клик по логотипу `✦` открывает Spotlight-лаунчер, клик по `ActiveApp` переключает Overview, клик по `Window` переключает режим раскладки (`Tile`/`Float`).
- **Dependencies:** `RUSTYOS-036`, `RUSTYOS-037`.
- **Acceptance criteria:** Все 78 тестов воркспейса проходят; `clippy -D warnings` и `cargo fmt` без замечаний; hit-testing меню и рабочих столов точно соответствует визуальной разметке; левая панель полностью соответствует `DESIGN.md` §12.1.
- **Risks:** Нулевые новые зависимости; на экранах любой ширины (от 1024 до 4K) меню и воркспейсы умещаются с достаточным запасом свободного пространства до системного трея.

## RUSTYOS-039 --- Spotlight Launcher в стиле macOS 27 Liquid Glass (DESIGN.md §12.3)

**Статус:** done **Фаза:** 32 (macOS 27 Visual Alignment)

- **Цель:** Обновить модальный лаунчер Spotlight по `DESIGN.md` §12.3: центрированная в верхней трети экрана Liquid Glass карточка `600x350` с радиусом `20` (`large`) и широкой мягкой тенью (`spread: 28`, `max_alpha: 115`), высокая поисковая панель `52 px` со стильной лупой и плейсхолдером, расширенный список результатов до 5 строк со скруглением `12 px`, акцентной подсветкой выбранной строки и тегами категорий (`Application`/`Command`) справа.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render.rs`:
    - `spotlight_modal_rect`: размер `600x350`, центрирование по X и привязка к верхней четверти экрана (`(height - modal_height) / 4`), внешний радиус `20`, тень `inner_radius: 20`, `spread: 28`, `max_alpha: 115`.
    - Поисковая панель: высота `52 px`, радиус `14`, иконка лупы, плейсхолдер `"Spotlight Search"`.
    - Результаты поиска: до 5 видимых строк высотой `46 px` с радиусом `12` (`medium`), выделенная строка с акцентным фоном `RGBA_ACCENT_MUTED` и левым индикатором `COLOR_ACCENT`, иконки `28x28`, правые теги категорий (`Application` / `Command`).
    - `check_spotlight_click`: поддержка hit-testing до 5 строк результатов.
  - `crates/rusty-compositor/src/state.rs` и `winit_backend.rs`:
    - Увеличение окна видимых результатов лаунчера с 3 до 5 (`state.launcher.visible_window(5)`, `self.launcher.exec_at_visible_row(5, row_offset)`).
- **Dependencies:** `RUSTYOS-018b`, `RUSTYOS-037`, `RUSTYOS-038`.
- **Acceptance criteria:** Все 78 тестов воркспейса проходят; `clippy -D warnings` и `cargo fmt` без замечаний; Spotlight отображает до 5 результатов с правильными тегами, корректным hit-testing и плавной анимацией появления.
- **Risks:** Нулевые новые зависимости; сохранена полная клавиатурная навигация (стрелки вверх/вниз, Enter, Escape).

## RUSTYOS-040 --- Multi-Monitor & Dynamic Hotplug Support

**Статус:** done **Фаза:** 33 (Hardware & Multi-Monitor)

- **Цель:** Реализовать полноценную поддержку нескольких мониторов (HDMI, DisplayPort, eDP) и динамического подключения/отключения дисплеев (Hotplug) в композиторе RustyOS: независимые координаты выходов, hit-testing, динамическое объединение границ (bounding bounds) для курсора мыши, перемещение окон между мониторами (`Super+Shift+O` / `Alt+O`) и безопасная миграция окон на оставшийся экран при отключении монитора.
- **Scope:**
  - `crates/rusty-compositor/src/outputs.rs`:
    - Модуль multi-monitor: структура `OutputEntry` (`id`, `name`, `output`, `location`, `physical_size`, `scale`, `refresh_milli_hz`, `is_primary`) с вычислением логической геометрии и `rusty_wm::Rect`.
    - Коллекция `OutputMap` с методами `add`, `remove`, `get`, `primary`, `output_under`, `total_bounds`, `next_adjacent_location` и `smithay_outputs`.
  - `crates/rusty-compositor/src/state.rs`:
    - Поле `output_map: OutputMap` в `RustyCompositorState`.
    - Методы `add_output(...)`, `remove_output(...)`, `total_output_bounds()`, `output_under(...)` и `move_focused_to_next_output(...)`.
  - `crates/rusty-compositor/src/input.rs`:
    - Функция `clamp_relative_pointer_rect` для корректного перемещения курсора мыши между соседними экранами в глобальном пространстве координат.
    - Автоматическая фокусировка активного экрана под указателем мыши (`self.wm.set_focused_output(out_id)`).
    - Ограничение взаимодействия с Dock только областью первичного экрана.
  - `crates/rusty-compositor/src/drm_backend.rs`:
    - Функции `find_all_connected_connectors` и `rescan_drm_connectors` для динамического обнаружения и обработки hotplug-событий в KMS сессии.
  - `crates/rusty-wm/src/lib.rs`:
    - Обновление `remove_output` с автоматической миграцией окон отключённого монитора в рабочую область активного воркспейса оставшегося монитора.
  - Тесты:
    - `outputs::tests::output_entry_geometry_scales_correctly`.
    - `outputs::tests::output_map_placement_and_hit_testing`.
    - `input::tests::clamp_relative_pointer_rect_allows_multi_monitor_crossing`.
    - Интеграционный тест `tests::multi_monitor_hotplug_window_migration_and_layout`.
- **Dependencies:** `RUSTYOS-006`, `RUSTYOS-028`.
- **Acceptance criteria:** Все 82 теста воркспейса проходят; `clippy -D warnings` и `cargo fmt` без замечаний; корректное масштабирование геометрии, hit-testing курсора по экранам, миграция окон при hotplug disconnect.
- **Risks:** Нулевые новые зависимости; сохранена 100% обратная совместимость со всеми существующими одиночными вызовами через `state.output`.

## RUSTYOS-041 --- Direct Scanout & Client `zwp_linux_dmabuf_v1` Support

**Статус:** done **Фаза:** 33 (Hardware & Multi-Monitor)

- **Цель:** Устранить лишнюю промежуточную GLES-копию и включить Zero-Copy прямой вывод на DRM/KMS-плоскости (`Primary` / `Cursor`) для полноэкранных Wayland и XWayland клиентов через протокол `zwp_linux_dmabuf_v1` (`ADR-0010`, `D-018`) с автоматическим безопасным fallback на GLES-композицию.
- **Scope:**
  - `crates/rusty-wm/src/lib.rs`:
    - Состояние полноэкранного окна `ManagedWindow::fullscreen`, действие `WmAction::ToggleFullscreen`, полные границы выхода `ManagedOutput::full_area` (`upsert_output_with_full_area`), методы `set_window_fullscreen` / `toggle_fullscreen_focused`.
  - `crates/rusty-compositor/src/state.rs`:
    - Регистрация `DmabufState` / `DmabufHandler` (`delegate_dmabuf!`) и инициализация `init_dmabuf_formats` / `init_dmabuf_feedback`.
    - Полноэкранный жизненный цикл в `sync_space_with_wm`, обработчики `XdgShellHandler::{fullscreen_request, unfullscreen_request}` и метод `active_unoccluded_fullscreen_window()`.
  - `crates/rusty-compositor/src/xwayland.rs`:
    - Обработчики `XwmHandler::{fullscreen_request, unfullscreen_request}` для XWayland-клиентов.
  - `crates/rusty-compositor/src/input.rs`:
    - Горячая клавиша `Alt+F` (`WmAction::ToggleFullscreen`) и проброс кликов у краёв экрана в активное неперекрытое fullscreen-окно.
  - `crates/rusty-compositor/src/winit_backend.rs`:
    - Инициализация `zwp_linux_dmabuf_v1` (v3) форматами `GlesRenderer` и пропуск chrome/теней/обоев в `build_frame_render_list` при `active_unoccluded_fullscreen_window().is_some()`.
  - `crates/rusty-compositor/src/drm_backend.rs`:
    - Регистрация `zwp_linux_dmabuf_v1` v4 с default `DmabufFeedback` + scanout preference tranche из первичной DRM-плоскости.
    - Подсчёт фактически представленных direct-scanout кадров по `PrimaryPlaneElement::Element(_)` в `handle_vblank`.
  - `crates/rusty-compositor/src/frame_timing.rs`:
    - Поле `direct_scanout_frames: u64` в `FrameTimingCollector` / `FrameTimingSummary` (JSON и лог).
  - `docs/adr/0010-direct-scanout-dmabuf.md`, `DECISIONS.md` (`D-018`).
- **Dependencies:** `RUSTYOS-006`, `RUSTYOS-028`, `RUSTYOS-040`.
- **Acceptance criteria:** Все 86 тестов воркспейса проходят; `clippy -D warnings` и `cargo fmt --check` без замечаний; клиенты получают `zwp_linux_dmabuf_v1`; неперекрытое fullscreen-окно освобождает chrome/обои для direct scanout и корректно откатывается к GLES при появлении оверлеев или окнах поверх; фактическая частота KMS scanout-попаданий подлежит верификации на bare-metal `--drm`.
- **Tests:**
  - `rusty_wm::tests::fullscreen_uses_full_output_bounds_and_restores_prior_state`.
  - `input::tests::alt_f_maps_to_toggle_fullscreen`.
  - `frame_timing::tests::collector_tracks_direct_scanout_frames_in_summary_and_json`.
  - `tests::dmabuf_global_and_unoccluded_fullscreen_fallback_behavior`.
- **Risks:** Нулевые новые внешние зависимости; аппаратный KMS direct scanout (`--drm`) не запускался в headless-среде (по правилам `HARDWARE.md` требуется bare-metal прогон на целевом GPU для подтверждения `direct_scanout_frames > 0`).

## RUSTYOS-042 --- Multi-Monitor DRM/KMS, Mixed-Refresh & VRR

**Статус:** done **Фаза:** 33

**Goal:** Перевести DRM/KMS backend на per-CRTC multi-monitor конвейер с независимым VBlank/page-flip циклом для mixed refresh (`60/120/144/240 Hz`), runtime Udev hotplug, выбором максимального refresh rate на preferred-разрешении, безопасным runtime VRR (`VrrMode`) и per-output изоляцией рендер-листа (ADR-0011, `D-019`).

**Scope (Approved Option A):**
- `crates/rusty-wm/src/lib.rs`: добавлен `WindowManager::visible_windows_for_output(output_id)` для изоляции видимых окон по активному workspace конкретного дисплея.
- `crates/rusty-compositor/src/config.rs`: добавлен enum `VrrMode` (`Off` / `Fullscreen` / `Always`), настройки `[display]` (`prefer_max_refresh`, `vrr` / `vrr_mode`) и helper `UserConfig::apply_toml_kv`.
- `crates/rusty-compositor/src/outputs.rs` & `crates/rusty-compositor/src/state.rs`: хранение и отзыв `wl_output` `GlobalId` при удалении выхода, добавление `active_unoccluded_fullscreen_window_on(output_id)` с изоляцией shell-оверлеев на primary output и дублированием `LockScreen` на все выходы.
- `crates/rusty-compositor/src/winit_backend.rs`: параметризация `build_frame_render_list_for_output` по `OutputEntry` (локальные координаты окон и курсора `pointer_local_if_on_output`, primary-only chrome, per-output wallpaper/layer-shell).
- `crates/rusty-compositor/src/drm_backend.rs`: перевод `DrmRuntime` на per-CRTC `HashMap<crtc::Handle, DrmOutputState>` (`DrmEvent::VBlank(crtc)`), подключение `UdevBackend` hotplug (`sync_hotplug_connectors`, `attach_secondary_connector`), `select_best_drm_mode`, `want_vrr_for_output` и `DrmOutputState::apply_runtime_vrr`.
- `docs/adr/0011-multi-monitor-drm-vrr.md` & `DECISIONS.md` (`D-019`): зафиксировано архитектурное решение Вариант A.

**Verification:**
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` и `cargo test --workspace --offline` пройдены: `91/91` тестов passed (`4` `rusty_animation`, `66` `rusty_compositor`, `3` `rusty_ipc`, `6` `rusty_services`, `5` `rusty_shell`, `7` `rusty_wm`).
- Новые unit/integration тесты: `visible_windows_for_output_isolates_per_display_workspaces`, `parses_display_vrr_and_refresh_preferences`, `pointer_local_maps_coordinates_only_for_matching_output`, `mode_selector_prefers_highest_refresh_at_native_resolution`, `per_output_fullscreen_vrr_and_hotplug_isolation`.
- **Risks:** Нулевые новые внешние зависимости; аппаратный multi-monitor `--drm`, live Udev hotplug и VRR-панель не запускались в headless-среде (по правилам `HARDWARE.md` требуется bare-metal проверка на реальных GPU/мониторах).

## RUSTYOS-043 --- HiDPI & Fractional Scaling (`wp_fractional_scale_v1` + `wp_viewporter`)

**Статус:** done **Фаза:** 34

**Goal:** Реализовать сквозную поддержку HiDPI и дробного масштабирования для Wayland/layer клиентов, shell UI, SSD и смешанных DPI конфигураций (ADR-0012, `D-020`).

**Scope (Approved Option A):**
- `crates/rusty-compositor/src/config.rs`: добавлены `clamp_display_scale`, `UserConfig::display_scale`, `UserConfig::output_scales` (`[display]` `scale` / `scales` / `[display.scales]`), `scale_for_output` и `has_explicit_output_scale`.
- `crates/rusty-compositor/src/outputs.rs`: добавлены `compute_logical_size`, `OutputMap::update_entry_geometry` и `relayout_adjacent` для беззахлёстной перекомпоновки mixed-DPI выходов.
- `crates/rusty-compositor/src/shell_render.rs`: расширен `CompositorRenderElement` вариантами `ScaledSolid` (`RescaleRenderElement`) и `ScaledMemory` (`RelocateRenderElement`).
- `crates/rusty-compositor/src/state.rs`: зарегистрированы `ViewporterState` (`wp_viewporter`) и `FractionalScaleManagerState`/`FractionalScaleHandler` (`wp_fractional_scale_v1`), добавлены `update_output_geometry`, `preferred_scale_for_surface` и `apply_surface_scale` (`preferred_scale` + integer `preferred_buffer_scale` в `new_fractional_scale`, `commit`, `sync_space_with_wm`).
- `crates/rusty-compositor/src/winit_backend.rs`, `drm_backend.rs` & `screenshot.rs`: добавлен `scale_render_elements_for_output`, устранены хардкоды scale `1.0` при рендере слоёв/окон/курсора/DnD/скриптов скриншотов и в Winit `Resized`/DRM `run_drm_kms`/`attach_secondary_connector`.
- `docs/adr/0012-hidpi-fractional-scaling.md` & `DECISIONS.md` (`D-020`): зафиксировано архитектурное решение Вариант A.

**Verification:**
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` и `cargo test --workspace --offline` пройдены: `94/94` тестов passed (`4` `rusty_animation`, `69` `rusty_compositor`, `3` `rusty_ipc`, `6` `rusty_services`, `5` `rusty_shell`, `7` `rusty_wm`).
- Новые unit/integration тесты: `parses_display_fractional_and_per_output_scales`, `update_entry_geometry_recomputes_logical_sizes_and_adjacent_origins`, `fractional_scale_viewporter_and_work_area_rescaling`.
- **Risks:** Нулевые новые внешние зависимости; визуальная проверка дробного HiDPI (125%/150%/200%) на живых Wayland-приложениях и bare-metal `--drm` не проводилась в headless-среде.

## RUSTYOS-044 --- Low-Latency Input & Presentation (`zwp_relative_pointer_manager_v1` + `zwp_pointer_constraints_v1` + `wp_presentation`)

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 6/8
- **ADR / Decision:** ADR-0013 / D-021
- **Acceptance Criteria:**
  - В `state.rs` зарегистрированы `RelativePointerManagerState` (`zwp_relative_pointer_manager_v1`), `PointerConstraintsState` / `PointerConstraintsHandler` (`zwp_pointer_constraints_v1`) и `PresentationState` (`wp_presentation`, monotonic clock), добавлен `Clock<Monotonic>` и методы `has_blocking_shell_overlay`, `deactivate_active_pointer_constraint`, `take_presentation_feedback`.
  - В `input.rs` `handle_pointer_motion` отправляет `PointerHandle::relative_motion` (`delta`, `delta_unaccel`, `utime`) и учитывает `LockedPointer` (с поддержкой `cursor_position_hint`) и `ConfinedPointer` (с проверкой region и helper `confine_pointer_to_rect`), а при открытии Launcher/Overview/LockScreen активный constraint безопасно деактивируется.
  - В `drm_backend.rs` (`DrmOutputState::pending_presentation_feedback`, `vblank_seq`, `compute_drm_presentation_flags`, `compute_presentation_refresh`, `handle_vblank`, сброс на timeout/ошибке) и `winit_backend.rs` собирается и завершается `OutputPresentationFeedback` с флагами `Vsync`/`HwClock`/`ZeroCopy` и `Refresh::Variable`/`Fixed`.
  - `cargo fmt --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершаются без предупреждений; 72/72 тестов `rusty-compositor` и 96/96 запущенных тестов workspace проходят (1 предсуществующий тест `rusty-services::tests::networkmanager_and_bluetooth_formatters_handle_all_states` пропущен из-за зависания `BluetoothService::query_status`/`bluetoothctl` >60s в sandbox-среде без BlueZ/D-Bus).
- **Risks:** Нулевые новые внешние зависимости; проверка `wp_presentation` (`ZeroCopy`/`VRR`) и `PointerConstraint` с реальными играми/XWayland-клиентами на bare-metal `--drm` не проводилась в headless-среде.

## RUSTYOS-045 --- Wayland Client UX, Idle & Session Lock (`wp_cursor_shape_v1`, `xdg_activation_v1`, `ext_idle_notify_v1`, `zwp_idle_inhibit_manager_v1`, `ext_session_lock_v1`)

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 6/7/8
- **ADR / Decision:** ADR-0014 / D-022
- **Acceptance Criteria:**
  - В `state.rs` зарегистрированы `CursorShapeManagerState` (+ `TabletSeatHandler`), `XdgActivationState` / `XdgActivationHandler` (с проверкой TTL 10 с и `activate_window_surface`), `IdleNotifierState` / `IdleNotifierHandler`, `IdleInhibitManagerState` / `IdleInhibitHandler` и `SessionLockManagerState` / `SessionLockHandler`; `RustyCompositorState::new` обновлён для передачи `LoopHandle` (в `winit_backend.rs`, `drm_backend.rs` и тестах `lib.rs`).
  - В `input.rs` и `xwayland.rs` подключены `notify_user_activity()` при вводе и полная изоляция ввода/clipboard/pointer constraints при `ext_session_locked`.
  - В `winit_backend.rs` и `drm_backend.rs` обеспечен изолированный рендер per-output `LockSurface` с чёрным fallback и подтверждением `confirm_pending_session_lock()` после представления кадра.
  - `cargo fmt --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершаются без предупреждений; 73/73 тестов `rusty-compositor` и 97/97 запущенных тестов workspace проходят (1 предсуществующий тест `rusty-services::tests::networkmanager_and_bluetooth_formatters_handle_all_states` пропущен флагом `--skip`).
- **Risks:** Нулевые новые внешние зависимости; живая проверка со сторонними клиентами (`swaylock`, `swayidle`, GTK4/Firefox) на bare-metal `--drm` не проводилась в headless-среде.

## RUSTYOS-046 --- Explicit Sync (`wp_linux_drm_syncobj_v1`), Observable Hardware Cursor Plane & HiDPI Cursors

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 3/4/5
- **ADR / Decision:** ADR-0015 / D-023
- **Acceptance Criteria:**
  - В `state.rs` добавлены хранение `LoopHandle<'static, Self>`, `DrmSyncobjState`, `DrmSyncobjHandler`, `delegate_drm_syncobj!`, `init_drm_syncobj` (runtime-проверка `supports_syncobj_eventfd` и `RUSTYOS_DISABLE_EXPLICIT_SYNC` с fallback на implicit sync), `take_syncobj_unblocked`, `hw_cursor_disabled_by_env` (`RUSTYOS_DISABLE_HW_CURSOR`) и `handle_syncobj_pre_commit` (через `add_pre_commit_hook` в `CompositorHandler::new_surface`, `DrmSyncPoint::generate_blocker` + `DrmSyncPointSource` -> `blocker_cleared`).
  - В `drm_backend.rs` и `frame_timing.rs` добавлены поля `syncobj_eventfd` и `cursor_plane_count` в `DrmHardwareReport`, учёт `drm_surface.planes().cursor.len()`, `RenderFrameResult::needs_sync()` -> `element.sync.wait()`, `hw_cursor_frames` (`result.cursor_element.is_some()`), пробуждение по `take_syncobj_unblocked()`, scoped output dirtying (`output_ids_affected_by_pointer_motion`, `mark_outputs_dirty`) при движении курсора без drag/resize и управление fallback `RUSTYOS_DISABLE_HW_CURSOR`.
  - В `surfaces.rs` и `winit_backend.rs` реализованы `CursorRenderMode`, `build_frame_render_list_for_output_with_cursor`, `push_client_surface_cursor`, `named_cursor_element_for_output`, `cursor_arrow_element_for_output`, HiDPI-масштабирование hotspot и выбор размера XCursor, а также понижение до `Kind::Unspecified` при превышении `cursor_size` или отключении аппаратного курсора.
  - `cargo fmt --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершаются без предупреждений; 76/76 тестов `rusty-compositor` и 100/100 запущенных тестов workspace проходят (1 предсуществующий тест `rusty-services::tests::networkmanager_and_bluetooth_formatters_handle_all_states` пропущен флагом `--skip`).
- **Risks:** Нулевые новые внешние зависимости; реальное срабатывание `wp_linux_drm_syncobj_v1` и DRM cursor plane на физическом оборудовании AMD/Intel/NVIDIA требует отдельного bare-metal `--drm` теста.

## RUSTYOS-047 --- Desktop UX Wayland Protocols (`zwp_pointer_gestures_v1`, `zwp_text_input_v3`, `zwp_input_method_v2`, `zwp_keyboard_shortcuts_inhibit_manager_v1`, `ext_foreign_toplevel_list_v1`, `xdg_toplevel_icon_v1`)

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 6/7/8
- **ADR / Decision:** ADR-0016 / D-024
- **Acceptance Criteria:**
  - В `state.rs` зарегистрированы и интегрированы все 6 Smithay-глобалов (`pointer_gestures_state`, `text_input_manager_state`, `input_method_manager_state`, `keyboard_shortcuts_inhibit_state`, `foreign_toplevel_list_state`, `xdg_toplevel_icon_manager` с размерами `[16, 24, 32, 48, 64]`) без добавления новых внешних зависимостей и без изменений `rusty-ipc` / `apps/rusty-shell`.
  - В `input.rs` и `state.rs` маршрутизированы все 8 вариантов `InputEvent::Gesture*` и реализованы 3-пальцевые свайпы (`TouchpadSwipeState`, `classify_three_finger_swipe`, `switch_workspace_relative`, Overview open/close) с изоляцией при блокировке сессии (`can_handle_compositor_gesture`).
  - В `input.rs` и `state.rs` реализована политика `zwp_keyboard_shortcuts_inhibit_manager_v1` (`KeyboardShortcutsInhibitHandler`, `is_keyboard_shortcuts_inhibited`, `deactivate_focused_shortcut_inhibitor`, `suppressed_inhibitor_surface`): шорткаты пропускаются сфокусированному ингибирующему клиенту, но `Ctrl+Alt+F1..F12` VT-switch, lock-screen изоляция (`ext_session_locked` / `screen_locked`) и аварийный выход `Super+Escape` (`KeyIntercept::ReleaseShortcutInhibitor`) не перехватываются.
  - В `state.rs`, `winit_backend.rs` и `drm_backend.rs` интегрированы `zwp_text_input_v3` + `zwp_input_method_v2` (`InputMethodHandler`, `compute_ime_popup_origin`, `ime_popups_for_output`, `push_ime_popup_elements`, `send_ime_popup_frames`, `cleanup_ime_popups`).
  - В `state.rs`, `xwayland.rs` и `winit_backend.rs` обеспечен единый жизненный цикл `ext_foreign_toplevel_list_v1` (`register_foreign_toplevel`, `sync_foreign_toplevel_metadata`, `close_foreign_toplevel`, `unregister_window_metadata`, унифицированный `window_title`) для Wayland и XWayland окон.
  - В `icons.rs`, `shell_render.rs`, `state.rs` и `winit_backend.rs` поддержан `xdg_toplevel_icon_v1` (`XdgToplevelIconHandler`, `sync_toplevel_icon_from_surface`, безопасное чтение SHM-буферов через `GlesRenderer::import_shm_buffer` -> `TextureBuffer<GlesTexture>` -> `TextureRenderElement`, отображение в Dock и `Alt+Tab` switcher без `unsafe`).
  - `cargo fmt --all --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершаются без предупреждений; 83/83 тестов `rusty-compositor` и 107/107 запущенных тестов workspace проходят (1 предсуществующий тест `rusty-services::tests::networkmanager_and_bluetooth_formatters_handle_all_states` пропущен флагом `--skip`).
- **Risks:** Нулевые новые внешние зависимости; живое E2E-подтверждение физическим тачпадом (3-пальцевые свайпы), IME-демоном (IBus/Fcitx5), клиентом с `zwp_keyboard_shortcuts_inhibit_v1` и Wayland-окнами с SHM `xdg_toplevel_icon_v1` не проводилось в headless sandbox.

## RUSTYOS-048 --- macOS 27 Frontend Visual Alignment, SDF Vector Chrome & Glass Surface Controls

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 7 / UX
- **ADR / Decision:** ADR-0017 / D-025
- **Acceptance Criteria:**
  - В `surfaces.rs` добавлены 2×2-суперсэмплированные SDF-глифы `UiGlyphKind` (`RustyLogo`, `Wifi`, `Volume`, `Battery`, `Search`, `ControlCenter`) и `UiShapeCache::ui_glyph_element`, устранившие `.notdef` прямоугольник логотипа и грубые прямоугольные иконки статуса, а fallback-обои Purple Glass повышены до 640×360 с 4×4 Bayer дитерингом против бандинга.
  - В `icons.rs` расширен XDG-поиск PNG (`AdwaitaLegacy`, `Qogir`, `Qogir-Dark`, `breeze-dark`, `Papirus-Dark`, категории `places`/`mimetypes`/`legacy`) и добавлены процедурные цветные squircle-иконки `BuiltinAppIcon` (`Terminal`, `Files`, `Overview`, `WindowFallback`) через `IconCache::builtin_squircle_icon_element`.
  - В `config.rs` добавлены настройки `[ui]` `glass_opacity` (`0.35..=1.0`, по умолчанию `0.78`) и `reduce_transparency`, подключённые в `state.rs`, `shell_render.rs` (`scale_glass_rgba`) и интерактивную карточку Glass Surface + ползунки громкости/прозрачности в Quick Settings (`telemetry::SystemMetricsHandle::set_volume_percent`, `QuickSettingsHit::{SetVolumePercent, ToggleReduceTransparency, SetGlassOpacityPercent}`).
  - В `rusty-services/src/lib.rs`, `telemetry.rs`, `shell_render.rs`, `state.rs`, `input.rs` и `winit_backend.rs`: добавлен формат даты+времени `clock_date_hhmm` (`local_clock_short_datetime`), убрана пустая дыра при `!has_battery` (`compute_top_bar_right_layout`), сделаны кликабельные выпадающие меню `TopBar` (`TopBarMenuAction` / `compute_top_bar_left_layout`), обновлена капсула Dock (`DOCK_RADIUS = 36`, `DOCK_DIVIDER_GAP = 12`, `dock_hover_anim`), поднят радиус окон (`WINDOW_RADIUS = 20`), убран 1px выступ нижней рамки SSD и исправлен Floating `frame_rect` overshoot в `winit_backend.rs`.
  - Синхронизирован визуальный рендер `apps/rusty-shell/src/wayland.rs` (`fill_rounded_rect`, `fill_capsule_with_rim`, скруглённые Spaces, Dock stadium-капсула с разделителем и squircle-плитками).
  - `cargo fmt --all --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершаются без предупреждений; 90/90 тестов `rusty-compositor`, 5/5 тестов `rusty-shell` и 114/114 запущенных тестов workspace проходят (1 предсуществующий тест `rusty-services::tests::networkmanager_and_bluetooth_formatters_handle_all_states` пропущен флагом `--skip`).
- **Risks:** Нулевые новые внешние зависимости; реальный FBO blur-behind для полупрозрачного стекла остаётся отдельной задачей будущего этапа; живая визуальная сверка скриншота в дисплейной сессии (`cargo run -p rusty-compositor`) в headless sandbox не запускалась.

## RUSTYOS-049 --- 1:1 macOS Menu Bar Visual Parity, Bundled Inter Typography & Minimal Bar Configuration

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 7 / UX
- **ADR / Decision:** ADR-0018 / D-026
- **Acceptance Criteria:**
  - В `assets/fonts/` добавлены встроенные через `include_bytes!` шрифты **Inter** (`Inter-Regular.ttf`, `Inter-SemiBold.ttf`, `LICENSE.txt` OFL-1.1) с поддержкой Latin и Cyrillic без новых внешних Rust-зависимостей и `unsafe`; в `crates/rusty-compositor/src/text.rs` добавлены `FontWeight::{Regular, Bold}`, `measure_ui_text_width`, `measure_text_weighted`, `render_label_weighted`, `render_label_centered_weighted` и выравнивание по единой оптической базовой линии регулярного начертания.
  - В `crates/rusty-compositor/src/shell_render.rs` высота верхней панели переведена на `TOP_BAR_HEIGHT = 24`, единый кегль `TOP_BAR_FONT_SIZE = 13.0` (`FontWeight::Bold` для активного приложения + `FontWeight::Regular` для меню `File`, `Edit`, `View`, `Go`, `Window`, `Help` и часов) с 1 px текстовым/иконочным drop-shadow при прозрачном режиме; убран тёмный трёхуровневый скрайм при `reduce_transparency = false` (сплошная панель только при `reduce_transparency = true`); перекрашена оригинальная монохромная шестиугольная эмблема `RustyLogo` в чистый белый `#FCFAFF`; правый кластер выстроен в точном порядке macOS (`[Battery] · Wi-Fi · Search · Control Center · Clock`).
  - В `crates/rusty-compositor/src/surfaces.rs` качество рендера всех UI SDF-глифов повышено до 4×4 суперсэмплинга (`sample_ui_glyph_pixel`), обновлены `sample_rusty_logo_uv`, `sample_wifi_glyph_uv` (`sd_arc_up`), `sample_battery_glyph_uv`, `sample_search_glyph_uv` и `sample_control_center_glyph_uv`.
  - В `crates/rusty-compositor/src/config.rs`, `state.rs`, `input.rs` и `winit_backend.rs` скрыты по умолчанию полоса воркспейсов `1..=9` + `Float`/`Tile`, SNI tray, `EN` и `Volume` через настраиваемые `[ui]`-параметры (`menubar_show_workspaces`, `menubar_show_tray`, `menubar_show_input_source`, `menubar_show_volume`, `clock_24h` = `false`); в меню `Go`/`Window`/`Help` добавлены `NextWorkspace`, `PrevWorkspace` (`switch_workspace_relative`) и `ShowShortcuts`.
  - В `crates/rusty-services/src/lib.rs` и `telemetry.rs` добавлен нейтральный `LC_ALL=C` формат часов macOS (`local_clock_short_datetime_fmt`: `"Wed Apr 1  9:41 AM"` и 24h `"Wed Apr 1  09:41"`); в `apps/rusty-shell/src/{lib,wayland}.rs` синхронизирован прозрачный рендер Menu Bar `24 px` и встроенный Inter.
  - Обновлены `DESIGN.md` §12.1, `ADR-0018`, запись `D-026` в `DECISIONS.md` и статус задачи в `TASKS.md`.
- **Verification:** `cargo fmt --all --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершились без предупреждений; 117/117 тестов workspace прошли без пропусков (`rusty_animation` 4/4, `rusty_compositor` 92/92, `rusty_ipc` 3/3, `rusty_services` 6/6, `rusty_shell` 5/5, `rusty_wm` 7/7); живая проверка `rusty-compositor --winit` (EGL/OpenGL ES 3.2, Mesa Intel Graphics) и инспекция захваченного скриншота окна подтвердили 1:1 визуальную чистоту Menu Bar.
- **Risks:** Нулевые новые внешние Rust-зависимости; отображение в нативном `--drm` режиме требует отдельного запуска вне nested-сессии.

## RUSTYOS-050 --- 1:1 macOS Dock Visual Parity, Top-Lit Liquid Glass & 4x4 SSAA Squircle Icons

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 7 / UX
- **ADR / Decision:** ADR-0019 / D-027
- **Acceptance Criteria:**
  - В `crates/rusty-compositor/src/surfaces.rs` добавлены `UiShapeCache::dock_glass_panel_element` (`sample_dock_glass_pixel`: полупрозрачное top-lit стекло с 1 px верхним бликом и 4×4 Bayer дитерингом, непрозрачный fallback при `reduce_transparency = true`) и `UiShapeCache::dock_icon_shadow_element` (`sample_dock_icon_shadow_pixel`).
  - В `crates/rusty-compositor/src/icons.rs` добавлены `BuiltinAppIcon::{Browser, Notes, Settings, Downloads, Trash}`, `squircle_sd` переведён на суперэллипс с экспонентой `4.8` и верхним бликом без замкнутой окантовки, суперсэмплинг `sample_builtin_squircle_pixel` повышен до 4×4 (16 выборок на пиксель), а `Downloads` и `Trash` реализованы свободными силуэтами без квадратной подложки.
  - В `crates/rusty-compositor/src/shell_render.rs` геометрические константы выровнены под референс macOS (`DOCK_HEIGHT = 68`, `DOCK_RADIUS = 22`, `DOCK_ITEM_SIZE = 52`, `DOCK_ICON_SIZE = 50`, `DOCK_ITEM_GAP = 6`, `DOCK_DIVIDER_GAP = 14`, `DOCK_PINNED_COUNT = 6`, `DOCK_UTILITY_COUNT = 2`), убраны серые слот-боксы, добавлен постоянный вертикальный разделитель перед `Downloads`/`Trash`, `5x5` SDF-точки запущенных/активных/свёрнутых приложений, квантованное hover-увеличение (`50..=58 px`, +6 px вверх, с уважением `reduced_motion`), тень только под squircle-карточками и стеклянный tooltip `dock_tooltip_rect_and_label`.
  - В `crates/rusty-compositor/src/state.rs` (`handle_dock_hit`, `dock_preferred_candidates`, `launch_dock_app_or_spotlight`) подключён запуск/переключение `Browser`, `Notes`, `Settings` (fallback в Control Center), `Downloads` (`$HOME/Downloads`) и `Trash` (`trash:///`); в `apps/rusty-shell/src/wayland.rs` синхронизированы `render_dock_pixels`, концентрический радиус `20` и размер surface `(488, 68)`.
  - Обновлены `DESIGN.md` §12.2, `ADR-0019`, запись `D-027` в `DECISIONS.md` и статус `RUSTYOS-050` в `TASKS.md`.
- **Verification:** `cargo fmt --all --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершились с кодом 0 без предупреждений; 118/118 тестов workspace прошли без пропусков (`rusty_animation` 4/4, `rusty_compositor` 93/93, `rusty_ipc` 3/3, `rusty_services` 6/6, `rusty_shell` 5/5, `rusty_wm` 7/7); живая проверка `rusty-compositor --winit` и визуальная инспекция скриншота + 200% Dock-кропа подтвердили чистоту иконок, стекла, тени, разделителя и индикаторной точки.
- **Risks:** Нулевые новые внешние Rust-зависимости и ноль `unsafe`; многопроходный GLES FBO blur-behind вынесен за пределы текущей задачи (эмулируется top-lit альфа-градиентом).

## RUSTYOS-051 --- 1:1 macOS Shell Overlays, Modular Control Center, Overview Spaces & Window Chrome

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 7 / UX
- **ADR / Decision:** ADR-0020 / D-028
- **Acceptance Criteria:**
  - В `crates/rusty-compositor/src/surfaces.rs` обобщён рендер стеклянных панелей `UiShapeCache::glass_panel_element` (`GlassPanelStyle::{Dock, Popover, ControlModule}`), добавлен светофор `UiShapeCache::traffic_light_element` (`TrafficLightKind::{Close, Minimize, Maximize}`) с гелевым градиентом/бликом и 17 новых `4×4` SSAA SDF-глифов `UiGlyphKind`; в `crates/rusty-compositor/src/icons.rs` добавлен `BuiltinAppIcon::RustySystem`.
  - В `crates/rusty-compositor/src/shell_render.rs` выпадающие меню Menu Bar обновлены на `MENU_POPOVER_RADIUS = 16`, `pop_w = 256`, ведущие `4×4` SSAA иконки, разделители групп (`TopBarMenuEntry::Separator`), правые реальные шорткаты и hover-пилюлю (`menu_hover_row`), а кнопка Control Center в `TopBar` получила пилюлю-подсветку при открытии (`quick_settings_open`).
  - Control Center переработан на модульный макет `324×346` (`QuickSettingsLayout`): карточка `Wi-Fi` + `Bluetooth`, круглые `46×46` кнопки `Float`/`Tile` и `Motion`, переключатель `Clear Glass`/`Opaque`, полноширинные капсульные слайдеры `Display` и `Sound` с перетаскиванием (`QuickSettingsSliderDrag`), модуль `Power` и круглые действия `Search`/`Sync`/`Lock`.
  - Хром `SSD` (`generate_window_ssd_composite_elements_with_hover`) получил hover-глифы светофора (`check_window_traffic_lights_hover`), верхний specular-обод titlebar, прозрачные дуги нижнего скругления `WINDOW_RADIUS = 20` и многослойные тени (`generate_window_shadow_elements`); Spotlight получил `48 px` поле `ControlModule` и сплошную акцентную пилюлю + `"Open ↵"`; Notification HUD (`340×68`, радиус `16`) заменил полосу срочности на `38×38` squircle-иконку и hover-кнопку закрытия `20×20`; OSD получил ведущий `22×22` `UiGlyphKind` и капсульный трек `h = 8, r = 4`; Overview получил верхнюю ленту миниатюр `Desktop 1..9` с `preview_rects` и `OverviewHit::SwitchWorkspace(u8)` и лёгкий скрайм `0.36`.
  - В `crates/rusty-compositor/src/state.rs`, `input.rs` и `winit_backend.rs` подключены `top_bar_menu_hover_row`, `qs_slider_drag`, `hovered_notification_idx`, `hovered_traffic_light_window`, `overview_workspace_spaces` и сброс временных hover/drag-состояний при закрытии/блокировке.
  - Обновлены `DESIGN.md` §12.3–12.6, `ADR-0020`, запись `D-028` в `DECISIONS.md` и статус задачи в `TASKS.md`.
- **Verification:** `cargo fmt --all --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершились с кодом 0 без предупреждений; 123/123 тестов workspace прошли без пропусков (`rusty_animation` 4/4, `rusty_compositor` 98/98, `rusty_ipc` 3/3, `rusty_services` 6/6, `rusty_shell` 5/5, `rusty_wm` 7/7); `cargo run --package rusty-compositor --offline -- --check` и дымовой запуск `timeout -s INT 3s target/debug/rusty-compositor --winit` инициализировали OpenGL ES 3.2 (Mesa Intel(R) Graphics), шрифты Inter, XWayland и рендер 1280×800 без ошибок.
- **Risks:** Многопроходный GLES FBO blur-behind по Утверждённому Варианту 1 остаётся будущей задачей; интерактивные состояния оверлеев при реальном пользовательском вводе покрыты тестами `input.rs` и `shell_render.rs` (визуальная инспекция живых оверлеев в окне не запускалась автоматически из CLI без интерактивного скрипта ввода).

## RUSTYOS-052 --- 1:1 macOS TopBar Live Wi-Fi Signal (3 Bands) & AC Battery Lightning Cutout

- **Owner:** AI Agent + User
- **Status:** done
- **Priority:** P0
- **Phase:** 7 / UX
- **ADR / Decision:** ADR-0021 / D-029
- **Acceptance Criteria:**
  - В `crates/rusty-services/src/lib.rs` расширен `NetworkStatus` (`wifi_signal_percent: Option<u8>`), добавлены `wifi_signal_bars(connected, signal_percent)`, `parse_proc_net_wireless_signal`, `format_nm_network_status_with_signal`, чтение живого сигнала через NetworkManager D-Bus (`PrimaryConnection -> Devices -> Wireless -> ActiveAccessPoint.Strength`) с fallback на `/proc/net/wireless` и `nmcli`, а в `PowerService::query_status` устранено перетирание активного `Mains`/`ACAD` неактивными USB-PD портами и поддержано `BatteryState::Charging`.
  - В `crates/rusty-compositor/src/surfaces.rs` расширен `UiGlyphKind::Wifi { connected, signal_percent }`, `sample_wifi_glyph_uv` приведён к 1:1 3-полосному вееру macOS (скруглённый сектор-клин `sd_pie_up` + 2 широкие концентрические дуги) с динамической индикацией `0..=3` полос и приглушением неактивных верхних полос; `sample_battery_glyph_uv` приведён к сплошной белой капсуле при `100%` (и стеклянному треку с пропорциональным заполнением `5..=97%`), отделённому зазором `D`-носику и белой наклонной молнии с прозрачным вырезным ореолом; `sample_control_center_glyph_uv` обновлён под верхнюю контурную и нижнюю сплошную пилюлю.
  - В `crates/rusty-compositor/src/shell_render.rs` (`TopBarTelemetry`, `QuickSettingsRenderState`, `compute_top_bar_right_layout_with_options`, `push_top_bar_right_glyphs`, Control Center), `crates/rusty-compositor/src/winit_backend.rs` и `apps/rusty-shell/src/lib.rs` (`TopBarState`, `DesktopShell::tick`) прокинуты `wifi_signal_percent`, `ac_online` и `battery_charging`, убрана отсечка `< 99` для молнии (`t.ac_online || t.battery_charging`) и убран лишний отступ перед часами.
  - Обновлены `DESIGN.md` §12.1, `docs/adr/0021-topbar-live-battery-wifi-parity.md`, запись `D-029` в `DECISIONS.md` и статус задачи в `TASKS.md`.
- **Verification:** `cargo fmt --all --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершились с кодом 0 без предупреждений; 127/127 тестов workspace прошли без пропусков (`rusty_animation` 4/4, `rusty_compositor` 100/100, `rusty_ipc` 3/3, `rusty_services` 8/8, `rusty_shell` 5/5, `rusty_wm` 7/7); живая проверка `rusty-compositor --winit` (Mesa Intel(R) Graphics, EGL/OpenGL ES 3.2) и инспекция финального 500%-кропа подтвердили 1:1 соответствие иконок `Battery`, `Wi-Fi` и `Control Center` с референсом macOS 27.
- **Risks:** Нулевые новые внешние Rust-зависимости и ноль `unsafe`; обновление телеметрии при изменении уровня сигнала выполняется с интервалом опроса `POLL_INTERVAL = 3s`.

### RUSTYOS-053 — 1:1 macOS 27 Floating Liquid Glass Control Center Islands
- **Status:** done
- **Priority:** P1
- **Phase:** Фаза 7 / UX Polish & macOS 27 Control Center Parity
- **Scope:**
  - Убрана внешняя тёмная оболочка `GlassPanelStyle::Popover`; каждый модуль рендерится как парящий островок `GlassPanelStyle::ControlModule` с собственной тенью и мягким верхне-нижним спекулярным бликом (`surfaces.rs`, `shell_render.rs`, `quick_settings_rect` `316x456` на `TOP_BAR_HEIGHT + 10`).
  - В `crates/rusty-services/src/lib.rs` добавлены `NetworkStatus::wifi_enabled`, `NetworkService::set_wifi_enabled` (NetworkManager D-Bus + fallback `nmcli radio wifi`), `MediaStatus` и `MediaService::query_status` (MPRIS2 Session D-Bus + fallback `playerctl`) и модульный тест `playerctl_status_line_parses_playing_paused_and_empty`.
  - В `crates/rusty-compositor/src/telemetry.rs` добавлены `SystemTelemetry::media`, `TelemetryCmd::SetWifiEnabled(bool)`, `SystemMetricsHandle::{set_brightness_percent, toggle_wifi}` и оптимистичное обновление воспроизведения/громкости.
  - В `crates/rusty-compositor/src/surfaces.rs` добавлены 4×4 SSAA SDF-глифы `MediaPrev`, `MediaPlay`, `MediaPause`, `MediaNext`, `MusicNote`, `Moon`, `OverviewSpaces`.
  - В `shell_render.rs`, `state.rs`, `input.rs` и `winit_backend.rs` реализованы отдельные левые капсулы с инвертированными белыми кругами (`Wi-Fi`, `Bluetooth`, `Power`), правая квадратная карточка `Now Playing` (`Not Playing` или трек с рабочими `Prev`/`PlayPause`/`Next`), парные круглые кнопки без подписей (`Tile`/`Float`, `Overview`, `Motion`, `Clear Glass`/`Opaque`), капсула `Focus` (с `State::focus_mode`, подавляющим видимые toast-уведомления), тонкие слайдеры `Display` (`BrightnessService` с fallback на `glass_opacity` `35..=100%`) и `Sound` (с круглой кнопкой `Mute` справа), нижняя пилюля `Lock Screen`, а также заголовки на `Inter SemiBold` (`FontWeight::Bold`).
  - Обновлены `DESIGN.md` §12.4, создан `docs/adr/0022-control-center-floating-islands.md`, добавлена запись `D-030` в `DECISIONS.md` и статус задачи в `TASKS.md`.
- **Verification:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` и `cargo run -p rusty-compositor --offline -- --check` завершились с кодом 0 без предупреждений; 128/128 тестов workspace прошли без пропусков (`rusty_animation` 4/4, `rusty_compositor` 100/100, `rusty_ipc` 3/3, `rusty_services` 9/9, `rusty_shell` 5/5, `rusty_wm` 7/7); живая проверка `rusty-compositor --winit` (Mesa Intel(R) Graphics RPL-U, OpenGL ES 3.2) и визуальная инспекция открытого Control Center (`rustyos_053_cc_crop.png`) подтвердили 1:1 соответствие макету и типографике macOS 27.
- **Risks:** Нулевые новые внешние зависимости и ноль `unsafe`; телеметрия MPRIS2 и Wi-Fi обновляется по циклу опроса `POLL_INTERVAL` (с мгновенным запросом обновления при открытии Control Center).

### RUSTYOS-054 — 1:1 macOS 27 Built-In Lock Screen Parity
- **Status:** done
- **Priority:** P1
- **Phase:** Фаза 7 / UX Polish & macOS 27 Lock Screen Parity
- **Scope:**
  - В `crates/rusty-services/src/lib.rs` добавлены `UserAccount`, `UserService` (`query_users`, `parse_passwd_users`, `resolve_user_avatar_path`), `SessionAction::{Suspend, Reboot, PowerOff}`, `SessionService::dispatch`, `local_clock_lock_date` и `local_clock_lock_hhmm`, а также модульный тест `passwd_user_parser_and_lock_clock_formatters_work`.
  - В `crates/rusty-compositor/src/telemetry.rs` добавлены поля `SystemTelemetry::{users, lock_date, lock_time_hhmm, lock_time_24h}`, команда `TelemetryCmd::Session(SessionAction)` и `SystemMetricsHandle::dispatch_session`.
  - В `crates/rusty-compositor/src/icons.rs` и `surfaces.rs` добавлены `load_circular_avatar_bgra`, `UiShapeCache::wallpaper_element_with_alpha`, `UiShapeCache::user_avatar_element`, 4×4 SSAA SDF-глифы `UiGlyphKind::{Keyboard, PowerSymbol}` и расширены проверки в тестах глифов/аватаров.
  - В `crates/rusty-compositor/src/shell_render.rs`, `state.rs`, `input.rs` и `winit_backend.rs` убраны 92%-ная тёмная завеса и карточка `360×216`: реализованы полноэкранный фон текущих обоев поверх скрытых окон/`TopBar`/`Dock`, верхние дата и крупные полупрозрачные часы, верхне-правый минимальный статус (раскладка + клавиатура, Battery, 3-полосный Wi-Fi, кнопка питания с поповером `Sleep`/`Restart...`/`Shut Down...`), нижний центрированный список пользователей (`38×38` круглые аватары и имена) и разблокировка по клику / `Enter` / `Space` / `Escape`, сохранив `ext_session_lock_v1` без изменений.
  - Обновлены `DESIGN.md` §12.7, создан `docs/adr/0023-macos-lock-screen-parity.md`, добавлена запись `D-031` в `DECISIONS.md` и статус задачи в `TASKS.md`.
- **Verification:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` и `cargo run -p rusty-compositor --offline -- --check` завершились с кодом 0 без предупреждений; 131/131 тестов workspace прошли без пропусков (`rusty_animation` 4/4, `rusty_compositor` 102/102, `rusty_ipc` 3/3, `rusty_services` 10/10, `rusty_shell` 5/5, `rusty_wm` 7/7); живая проверка `rusty-compositor --winit` (Mesa Intel(R) Graphics RPL-U, OpenGL ES 3.2) и визуальная инспекция скриншотов `rustyos_054_lock_full.png`, `rustyos_054_lock_topright.png`, `rustyos_054_lock_users.png` и `rustyos_054_lock_powermenu.png` подтвердили 1:1 соответствие макету и отцентрированную композицию экрана блокировки macOS 27.
- **Risks:** Нулевые новые внешние Rust-зависимости и ноль `unsafe`; встроенный Lock Screen остается без PAM-проверки пароля (внешний PAM-локер может подключаться через сохранённый протокол `ext_session_lock_v1`).

### RUSTYOS-055 — Display Output Rotation (0/90/180/270°) & Pointer Coordinate Mapping
- **Status:** done
- **Priority:** P1
- **Phase:** Фаза 7 / Display & Input
- **Scope:**
  - В `crates/rusty-compositor/src/config.rs` добавлен enum `DisplayRotation` (`Rotate0`/`Rotate90`/`Rotate180`/`Rotate270`), парсинг `[display].rotation`, inline- и секционных таблиц `rotations`, а также `[display.outputs."<NAME>"]` (`rotation` и `scale`), единый helper `lookup_per_output` и `UserConfig::rotation_for_output`.
  - В `crates/rusty-compositor/src/lib.rs` добавлен CLI-флаг `--rotate <0|90|180|270>` (`CompositorConfig::rotation_override`), применено переопределение и в `--check` режиме.
  - В `crates/rusty-compositor/src/outputs.rs`, `state.rs`, `winit_backend.rs` и `drm_backend.rs` добавлены `compute_logical_size_for_rotation`, `OutputEntry::{rotation, with_rotation, unrotated_logical_size, transformed_physical_size}`, `OutputMap::update_entry_rotation`, `RustyCompositorState::set_output_rotation`, `relayout_wm_for_all_outputs`, FBO-компенсированная матрица Winit (`to_winit_transform`) и DRM/KMS-трансформация (`to_wl_transform`), а также корректировка размера захвата скриншота в `process_pending_screenshot`.
  - В `crates/rusty-compositor/src/input.rs` добавлена функция `transform_screen_point_to_output_logical` и обновлена маршрутизация `handle_pointer_motion_absolute` (одиночный и multi-output случай), добавлены модульные тесты парсинга/геометрии/ввода и тесты `parse_args`.
  - Обновлены `docs/adr/0024-display-output-rotation.md`, запись `D-032` в `DECISIONS.md` и статус задачи в `TASKS.md`.
- **Verification:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` и `cargo run -p rusty-compositor --offline -- --check --rotate 180` завершились с кодом 0 без предупреждений; 134/134 тестов workspace прошли без пропусков (`rusty_animation` 4/4, `rusty_compositor` 105/105, `rusty_ipc` 3/3, `rusty_services` 10/10, `rusty_shell` 5/5, `rusty_wm` 7/7); живой рендер `--winit --rotate 180` и `--winit --rotate 90` (скриншоты `rustyos_055_rot180.png`, `rustyos_055_rot90.png` на Mesa Intel(R) Graphics RPL-U) и `--winit --rotate 270 --max-frames 2` подтвердили корректную ориентацию без зеркалирования, пересчёт портретной области `800×1280` и стабильный старт/выход; переустановлены release-бинарники в `~/.local` (`./scripts/install.sh`) и проверен запуск `~/.local/bin/rustyos-session --winit --rotate 180 --max-frames 3`.
- **Risks:** Нулевые новые внешние зависимости и ноль `unsafe`; аппаратная ротация плоскостей DRM/KMS и direct scanout при `90°`/`180°`/`270°` на реальном железе зависят от возможностей конкретного драйвера GPU (при ограничении KMS-плоскости Smithay `DrmCompositor` использует GLES-композитинг; требует отдельной проверки в живой TTY `--drm` сессии).

### RUSTYOS-056 — PAM Lock Screen Authentication, `systemd-logind` Bridge & Native Polkit Agent
- **Status:** done
- **Priority:** P1
- **Phase:** Фаза 7 / Security & Session Integration
- **Scope:**
  - В `crates/rusty-services/src/auth.rs` добавлены `SecretText` (512 Б преаллокация, зануление на `clear`/`Drop`, `[REDACTED]` в `Debug`), `AuthOutcome`, `AsyncAuthHandle` и проверка пароля текущего пользователя через `/usr/bin/unix_chkpwd` (`/sbin/unix_chkpwd`) без блокировки event loop.
  - В `crates/rusty-services/src/logind.rs` добавлены `LogindEvent` (`PrepareForSleep`, `SessionLock`, `SessionUnlock`), `LogindMonitorHandle` (подписка System D-Bus на `org.freedesktop.login1` с `delay` sleep-ингибитором и освобождением через `ack_prepare_for_sleep`) и асинхронный `SetLockedHint`.
  - В `crates/rusty-services/src/polkit.rs` реализован `PolkitAgentHandle` (экспорт `org.freedesktop.PolicyKit1.AuthenticationAgent` через `zbus` 5, клиент `/run/polkit/agent-helper.socket` и fallback `/usr/lib/polkit-1/polkit-agent-helper-1`, отмена и повторные попытки).
  - В `crates/rusty-compositor/src/config.rs` добавлена секция `[security]` (`require_password_on_lock = true`, `lock_on_sleep = true`, `enable_polkit_agent = true`).
  - В `crates/rusty-compositor/src/{state,shell_render,input,winit_backend,drm_backend}.rs` добавлены маскированное Liquid Glass поле пароля (`•`) на Lock Screen, центрированный Liquid Glass модал `ShellOverlayView::Polkit`, эксклюзивный перехват клавиатуры/указателя/жестов и общее дренирование очередей в Winit и DRM бэкендах.
  - Обновлены `docs/adr/0025-pam-logind-polkit-integration.md`, запись `D-033` в `DECISIONS.md` и статус задачи в `TASKS.md`.
- **Verification:** `cargo fmt --all --check` и `cargo clippy --workspace --all-targets --offline -- -D warnings` завершились с кодом 0 без предупреждений; 147/147 тестов workspace прошли без пропусков (`rusty_animation` 4/4, `rusty_compositor` 109/109, `rusty_ipc` 3/3, `rusty_services` 19/19, `rusty_shell` 5/5, `rusty_wm` 7/7); выполнены `cargo run -p rusty-compositor --offline -- --check` и живой smoke-запуск `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` (Mesa Intel(R) Graphics RPL-U, выход с кодом 0).
- **Risks:** Нулевые новые внешние зависимости и ноль `unsafe`; во вложенном `--winit` запуске под хост-сессией (KDE Plasma) регистрация системного Polkit-агента может быть отклонена `polkitd`, так как один PID/сессия уже владеет агентом на D-Bus (сквозная регистрация агента и реальный `pkexec`/sleep с TTY требуют проверки в standalone `--drm` сессии RustyOS).

### RUSTYOS-057 — Native XDG Desktop Portal (`Screenshot` / `PickColor` / `ScreenCast`) & PipeWire Worker
- **Status:** done
- **Priority:** P1
- **Phase:** Фаза 7 / Portals & Media Streaming
- **Scope:**
  - В `crates/rusty-services/c/rusty-pipewire-stream.c` и `crates/rusty-services/build.rs` реализован изолированный C11 PipeWire `Video/Source` воркер (`libpipewire-0.3`, `libspa-0.2`), принимающий `RPW1` кадры по `stdin`, выполняющий `pw_core_sync` до вывода `READY <node_id> <serial>` и отчитывающийся о `STATE streaming`/`paused`.
  - В `crates/rusty-services/src/portal.rs` реализованы `PortalHandle`, `PipeWireStreamHandle`, `org.freedesktop.impl.portal.Screenshot` (v3: `Screenshot`, `PickColor`), `org.freedesktop.impl.portal.ScreenCast` (v6: `CreateSession`, `SelectSources`, `Start`), `Request` и `Session` на `zbus` 5 без блокировки композитора и с `unsafe_code = "deny"`.
  - В `crates/rusty-compositor/src/{screenshot,state,shell_render,input,winit_backend,drm_backend}.rs` добавлены `OffscreenCaptureBuffer`, `unique_screenshot_path`, `sample_rgba_pixel`, Liquid Glass диалог выбора источника `ShellOverlayView::Portal` и пипетка `ShellOverlayView::PickColor`, принудительное отклонение и закрытие сессий при `is_session_locked()`, а также единая подача кадров из Winit и DRM/KMS бэкендов.
  - Добавлены конфиги `packaging/portals/rustyos.portal` и `packaging/portals/rustyos-portals.conf`, интегрированные в `bin/rustyos-session` и `scripts/install.sh`.
  - Оформлены `docs/adr/0026-xdg-desktop-portal-pipewire-screencast.md`, запись `D-034` в `DECISIONS.md` и задача `RUSTYOS-057` в `TASKS.md`.
- **Verification:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo test --workspace --offline` (158 passed, 0 failed: `rusty_animation` 4, `rusty_compositor` 113, `rusty_ipc` 3, `rusty_services` 26 — включая `portal::tests::resolves_and_streams_real_pipewire_worker_when_available`, `rusty_shell` 5, `rusty_wm` 7), `cargo run -p rusty-compositor --offline -- --check`, `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` (rendered_frames=3, wayland-4), а также `sh -n bin/rustyos-session && bash -n scripts/install.sh`.
- **Risks:** Ноль новых внешних Rust-зависимостей и ноль `unsafe`; при работе в PipeWire пока используется offscreen `GlesTexture` readback через канал в воркер вместо zero-copy DMA-BUF, а перехват D-Bus имени во вложенной `--winit` сессии возможен только при отсутствии стороннего активного `rustyos` portal provider; полноэкранная интеграция `xdg-desktop-portal` проверяется в standalone `--drm` сессии.

### RUSTYOS-058 — Модульная декомпозиция `crates/rusty-compositor` (Этап А: `state/` и `shell_render/`)
- **Status:** done
- **Priority:** P2
- **Phase:** Фаза 8 / Architecture & Maintainability
- **Scope:**
  - Монолитный `crates/rusty-compositor/src/state.rs` (6 164 строки) декомпозирован в директорию `crates/rusty-compositor/src/state/` с сохранением публичного API и поведения через `pub use` в `state/mod.rs` (1 833 строки): `wm_windows.rs` (1 252), `system_bridges.rs` (947), `shell_actions.rs` (835), `tests.rs` (171) и сгруппированный `wayland_handlers/` (`mod.rs` 8, `core.rs` 560, `desktop_protocols.rs` 291, `seat_input.rs` 223, `session_lock.rs` 123, `selection.rs` 97, `dmabuf_sync.rs` 41).
  - Монолитный `crates/rusty-compositor/src/shell_render.rs` (9 168 строк) декомпозирован в директорию `crates/rusty-compositor/src/shell_render/` с сохранением публичного API и поведения через `pub use` в `shell_render/mod.rs` (1 521 строка): `tokens.rs` (462), `top_bar.rs` (1 817), `dock.rs` (325), `quick_settings.rs` (1 130), `spotlight_overview.rs` (595), `lock_screen.rs` (984), `modals.rs` (885), `ssd.rs` (508) и `tests.rs` (1 144).
  - Внутренние межмодульные связи ограничены через `pub(in crate::state)` и `pub(super)` с явными импортами (без wildcard-импортов в не-тестовом коде, без новых `Cargo.toml`/крейтов и без нового `unsafe`).
  - Обновлена схема модулей в `ARCHITECTURE.md` и зафиксирован статус `RUSTYOS-058` в `TASKS.md`.
- **Verification:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` (0 предупреждений), `cargo test --workspace --offline` (158 passed, 0 failed, 0 ignored: `rusty_animation` 4, `rusty_compositor` 113, `rusty_ipc` 3, `rusty_services` 26, `rusty_shell` 5, `rusty_wm` 7), `cargo run -p rusty-compositor --offline -- --check` и `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` (`rendered_frames=3`, `wayland-2`) завершились с кодом 0.
- **Risks:** Нулевое изменение публичного API и runtime-логики; остальные крупные модули (`input.rs`, `surfaces.rs`, `crates/rusty-services/src/lib.rs`) вынесены на отдельный опциональный Этап Б.

### RUSTYOS-059 — Модульная декомпозиция `input/`, `surfaces/` и `rusty-services` (Этап Б)
- **Status:** done
- **Priority:** P2
- **Phase:** Фаза 8 / Architecture & Maintainability
- **Scope:**
  - Монолитный `crates/rusty-compositor/src/input.rs` (3 672 строки) декомпозирован в директорию `crates/rusty-compositor/src/input/` с сохранением публичного API и поведения через `pub use` в `input/mod.rs` (149 строк): `keyboard.rs` (735), `pointer.rs` (711), `shell_pointer.rs` (513), `gestures.rs` (182) и `tests.rs` (1 478).
  - Монолитный `crates/rusty-compositor/src/surfaces.rs` (3 234 строки) декомпозирован в директорию `crates/rusty-compositor/src/surfaces/` с сохранением публичного API и поведения в `surfaces/mod.rs` (824 строки): `sdf.rs` (868), `glyphs.rs` (656), `xcursor.rs` (445) и `tests.rs` (504).
  - Из `crates/rusty-services/src/lib.rs` (1 761 → 308 строк с `ServiceRegistry`, реэкспортами и тестами) сервисы перенесены в тематические подмодули с сохранением всего публичного API через `pub use`: `power.rs` (125), `connectivity.rs` (606), `media.rs` (335) и `display_session.rs` (427), при неизменных `auth.rs`, `logind.rs`, `polkit.rs`, `portal.rs`.
  - Внутренние связи ограничены `pub(super)` / `pub(crate)` с явными импортами (без wildcard-импортов в не-тестовом коде, без правок `Cargo.toml` и без нового `unsafe`).
  - Обновлена схема модулей в `ARCHITECTURE.md` и зафиксирован статус `RUSTYOS-059` в `TASKS.md`.
- **Verification:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` (0 предупреждений), `cargo test --workspace --offline` (158 passed, 0 failed, 0 ignored: `rusty_animation` 4, `rusty_compositor` 113, `rusty_ipc` 3, `rusty_services` 26, `rusty_shell` 5, `rusty_wm` 7), `cargo run -p rusty-compositor --offline -- --check` и `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` (`rendered_frames=3`, `wayland-2`) завершились с кодом 0.
- **Risks:** Нулевое изменение публичных интерфейсов и runtime-логики; из крупных монолитов вне рамок Этапа Б осознанно оставлен `crates/rusty-compositor/src/icons.rs` (2 094 строки) со встроенным PNG/deflate-декодером.

### RUSTYOS-060 — Видимость/Z-order системных и интерактивных overlay, `frame_composer.rs`, системные dev-пакеты в CI и жизненный цикл shell сессии
- **Status:** done
- **Priority:** P1
- **Phase:** Фаза 8 / Architecture & Session
- **Scope:**
  - Исправлена видимость и Z-order системных/интерактивных overlay (`Polkit`, `Portal`, `PickColor`, `Switcher`, `Spotlight`, `QuickSettings`, `Overview`, `LockScreen`, `PowerMenu`, уведомления/OSD) относительно полноэкранных окон и `wlr-layer-shell` слоёв: разделены `foreground_overlay` (выше `Layer::Overlay`/`Layer::Top` и `fullscreen`), persistent `chrome` (`TopBar`/`Dock`, скрываемый при `fullscreen` и активных foreground layer-shell поверхностях) и `Overview` (выше управляемых окон, но ниже `TopBar` и `Layer::Top`).
  - Политика bypass полноэкранных окон (`state/mod.rs`) и pointer/hover routing (`input/pointer.rs`, `input/shell_pointer.rs`) синхронизированы с разделением overlay и persistent chrome; в `shell_render/mod.rs` добавлены раздельные композиционные хелперы без ломки существующих публичных функций.
  - Из `winit_backend.rs` в новый backend-независимый модуль `crates/rusty-compositor/src/frame_composer.rs` вынесена общая композиция кадра (сборка render list outputs, HiDPI scale, layer-shell/IME/cursor и post-frame capture/screencast readback), на которую переведены `winit_backend.rs`, `drm_backend.rs` и `screenshot.rs` (без связности `drm_backend -> winit_backend` и без новых зависимостей/`unsafe`).
  - В `.github/workflows/ci.yml` добавлены системные dev-пакеты Ubuntu (`libwayland-dev`, `libxkbcommon-dev`, `libudev-dev`, `libinput-dev`, `libgbm-dev`, `libdrm-dev`, `libseat-dev`, `libegl-dev`, `libgles-dev`, `libpixman-1-dev`, `libpipewire-0.3-dev` и др.) и headless-проверка `cargo run -p rusty-compositor -- --check`.
  - Скрипт `bin/rustyos-session` переведён на встроенный композиторный shell по умолчанию без запуска one-shot `rusty-shell`; внешний `rusty-shell --wayland` запускается только по opt-in `RUSTYOS_EXTERNAL_SHELL=1` с fallback-предупреждением при отсутствии бинарника.
  - Оформлены `docs/adr/0027-session-shell-lifecycle-and-overlay-separation.md` и запись `D-035` в `DECISIONS.md`, обновлены `ARCHITECTURE.md` и `TASKS.md`.
- **Verification:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` (0 предупреждений), `cargo test --workspace --offline` (160 passed, 0 failed, 0 ignored: `rusty_animation` 4, `rusty_compositor` 115, `rusty_ipc` 3, `rusty_services` 26, `rusty_shell` 5, `rusty_wm` 7), `cargo run -p rusty-compositor --offline -- --check`, `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` (`rendered_frames=3`) и `sh -n bin/rustyos-session` завершены с кодом 0.
- **Risks:** Обновлённый workflow `.github/workflows/ci.yml` проверяется на GitHub Actions runner только после `git push`; `frame_composer.rs` консолидирует уже существующую композиционную логику без изменения форматов IPC или Wayland-протоколов.

### RUSTYOS-061 — Сериализация `UserConfig`, live config reload и встроенная панель System Settings
- **Status:** done
- **Priority:** P1
- **Phase:** Фаза 8 / Core UX & Settings
- **Scope:**
  - В `crates/rusty-compositor/src/config.rs` реализованы `VrrMode::as_str`, `escape_toml_string`, `UserConfig::to_toml_string`, атомарное сохранение `UserConfig::save_to_path` через временный файл и `fs::rename`, а также сохранение исходного регистра имени коннектора в `[display.outputs."..."]` при парсинге; добавлен тест `toml_serialization_and_atomic_save_roundtrip_all_sections`.
  - В `RustyCompositorState` (`state/mod.rs`, `state/shell_actions.rs`, `winit_backend.rs`, `drm_backend.rs`) добавлены `config_path: Option<PathBuf>` (`None` в unit-тестах, `default_user_config_path()` в runtime бэкендах), `sync_and_save_user_config`, `apply_user_config` и `reload_user_config` с живым применением UI/Menu Bar/дисплейных (`scale`, `rotation`, `VrrMode`, `prefer_max_refresh`), security-политик и keyboard repeat/XKB параметров.
  - В `crates/rusty-ipc/src/lib.rs` и `state/wm_windows.rs` добавлен запрос `ShellRequest::ReloadConfig` (`RELOAD_CONFIG`), в `shell_render/top_bar.rs` — пункты System Menu `System Settings...` (`Super+,`) и `Reload Config` (`Super+Shift+R`), а в `input/{mod.rs,keyboard.rs,pointer.rs,shell_pointer.rs}` — клавиатурная и pointer-обработка.
  - В `crates/rusty-compositor/src/shell_render/{modals.rs,mod.rs,tests.rs}` и `frame_composer.rs` реализован Liquid Glass модал `ShellOverlayView::Settings` с четырьмя вкладками (`SettingsTab::{Appearance, MenuBar, Displays, Security}`), кнопками `Reload TOML` и `Done`/`×`, изоляцией полноэкранного bypass/поверхностей и unit-тестом `settings_modal_hit_testing_covers_tabs_controls_and_buttons`.
  - Добавлен интеграционный тест `state::tests::persistent_config_save_reload_and_settings_toggles_roundtrip`; оформлены `docs/adr/0028-persistent-user-config-and-system-settings.md`, запись `D-036` в `DECISIONS.md`, обновлены `ARCHITECTURE.md` и `TASKS.md`.
- **Verification:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` (0 предупреждений), `cargo test --workspace --offline` (163 passed, 0 failed, 0 ignored: `rusty_animation` 4, `rusty_compositor` 118, `rusty_ipc` 3, `rusty_services` 26, `rusty_shell` 5, `rusty_wm` 7), `cargo run -p rusty-compositor --offline -- --check` и `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` завершены с кодом 0.
- **Risks:** Новые зависимости и `unsafe` не добавлялись (`unsafe_code = "deny"` сохранён); во вложенном Winit-режиме `--winit` изменение XKB-раскладки и repeat-параметров передаётся Wayland-клиентам, тогда как аппаратный перебор DRM/KMS-режимов (`prefer_max_refresh` / `vrr_mode`) проявляется на нативном `--drm` бэкенде.

### RUSTYOS-062 — Вкладка `Keyboard` в System Settings, кликабельный индикатор раскладки в Menu Bar и live-перезагрузка обоев
- **Status:** done
- **Priority:** P1
- **Phase:** Фаза 8 / Core UX & Settings
- **Scope:**
  - В `crates/rusty-compositor/src/config.rs` добавлены пресеты и хелперы `KEYBOARD_LAYOUT_PRESETS`, `KEYBOARD_OPTION_PRESETS`, `next_keyboard_layout_preset`, `next_keyboard_option_preset`, `keyboard_option_label` и `default_user_wallpaper_path`, а также тест `keyboard_preset_cycling_and_labels_cover_builtin_and_custom_values`.
  - В `crates/rusty-compositor/src/shell_render/{modals.rs,tests.rs}` в `SettingsTab` добавлена пятая вкладка `Keyboard` (`UiGlyphKind::Keyboard`) с настройками `Input Layouts` (`keyboard_layout`), `Switch Shortcut` (`keyboard_options`), `Repeat Rate` (`repeat_rate`), `Repeat Delay` (`repeat_delay`) и `Active Source` (`CycleActiveInputSource`), а во вкладку `Appearance` — строка `Desktop Wallpaper` (`ToggleWallpaper` между custom `.png` и built-in Tahoe), расширена ширина toggle-пилюль до 110 px и обновлены hit-test тесты.
  - В `crates/rusty-compositor/src/shell_render/mod.rs` добавлена полупрозрачная стеклянная капсула за индикатором раскладки (`kbd_x`) в Menu Bar; в `crates/rusty-compositor/src/input/shell_pointer.rs` левый клик по индикатору в Menu Bar переключает активную XKB-раскладку, а правый клик открывает `System Settings` сразу на вкладке `Keyboard`; в `state/mod.rs` (`refresh_keyboard_layout_label`, `cycle_keyboard_layout`) исправлена немедленная перерисовка shell при смене раскладки.
  - В `crates/rusty-compositor/src/surfaces/mod.rs`, `frame_composer.rs`, `state/mod.rs` и `state/shell_actions.rs` добавлена ревизионная синхронизация кэша обоев (`wallpaper_revision` / `sync_wallpaper_revision`, `last_custom_wallpaper`) для live-перезагрузки PNG-обоев при `ReloadConfig` (включая замену файла на диске по тому же пути) и исправлен порядок обновления `user_config` перед пересчётом `active_kbd_layout` в `apply_user_config`; добавлен тест `state::tests::keyboard_settings_wallpaper_reload_and_menubar_click_roundtrip`.
- **Verification:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings` (0 предупреждений), `cargo test --workspace --offline` (165 passed, 0 failed, 0 ignored: `rusty_animation` 4, `rusty_compositor` 120, `rusty_ipc` 3, `rusty_services` 26, `rusty_shell` 5, `rusty_wm` 7), `cargo run -p rusty-compositor --offline -- --check` и `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` (`rendered_frames=3`, `wayland-2`) завершены с кодом 0.
- **Risks:** Новые зависимости и `unsafe` не добавлялись (`unsafe_code = "deny"` сохранён); при замене внешнего PNG-файла без изменения пути перерисовка выполняется по `ReloadConfig` / переключению обоев, без фонового polling/inotify.

### RUSTYOS-063 — Выбор активного монитора и per-output scale/rotation в System Settings -> Displays (Multi-Monitor Displays UX)

- **Status:** Done (`RUSTYOS-063` Вариант 1 — утверждённый объём выполнен)
- **Scope:**
  - В `crates/rusty-compositor/src/shell_render/modals.rs` расширен `SettingsRenderState` (`target_connector`, `target_output_index`, `output_count`, `display_width_px`, `display_height_px`, `display_refresh_hz`), добавлен вариант `SettingsHit::CycleTargetOutput`, а в `settings_display_rows` добавлена строка выбора выхода `Output {idx+1}/{count} · {w}x{h}@{hz}Hz` (`Output · {w}x{h}@{hz}Hz` при одном выходе) с pill `target_connector` и подписью `HiDPI Scale ({conn})`.
  - В `crates/rusty-compositor/src/state/mod.rs` добавлено поле `settings_target_output: Option<OutputId>` и метод `RustyCompositorState::selected_settings_output()` с автоматическим fallback на `output_map.primary()`, если цель не выбрана или монитор отключён.
  - В `crates/rusty-compositor/src/state/shell_actions.rs` обновлены `settings_render_state`, `apply_settings_hit`, `cycle_target_settings_output`, `step_target_display_scale` и `cycle_target_display_rotation`: scale и rotation применяются к выбранному выходу, сохраняются в `[display.outputs."<name>"]` (и в top-level `[display]` только при единственном подключённом выходе), а переключение выхода показывает OSD `Display: <name>`.
  - Обновлены и добавлены модульные тесты в `crates/rusty-compositor/src/shell_render/tests.rs` и `crates/rusty-compositor/src/state/tests.rs` (`multi_output_settings_targets_each_display_independently_with_unplug_fallback`).
- **Verification:**
  - `cargo fmt --all -- --check` — pass;
  - `cargo clippy --workspace --all-targets --offline -- -D warnings` — pass (0 warnings);
  - `cargo test --workspace --offline` — pass (166 passed: `rusty_animation` 4, `rusty_compositor` 121, `rusty_ipc` 3, `rusty_services` 26, `rusty_shell` 5, `rusty_wm` 7);
  - `cargo run -p rusty-compositor --offline -- --check` — pass (`finished cleanly (mode: check-only)`);
  - `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` — pass (`rendered_frames=3`, `finished cleanly (mode: nested-winit)`).
- **Risks:** Новые зависимости и `unsafe` не добавлялись (`unsafe_code = "deny"` сохранён); переименование уже утверждённого scope отсутствует, а `Winit` и `DRM/KMS` используют единый `OutputMap` / `UserConfig` pipeline.

### RUSTYOS-064 — Контекстные меню (ПКМ) на Desktop и Dock (`ShellOverlayView::ContextMenu`, секция `[dock]` в `UserConfig`)

- **Status:** Done (`RUSTYOS-064` Вариант 1 — утверждённый объём выполнен)
- **Scope:**
  - В `crates/rusty-compositor/src/surfaces/{mod.rs,glyphs.rs,tests.rs}` добавлены глифы `UiGlyphKind::{Plus, Check, ChevronRight, Trash}` и их SDF-растеризаторы + покрытие тестами.
  - В `crates/rusty-compositor/src/config.rs` добавлено перечисление `DownloadsSortMode::{DateModified, Name, Size}`, поле `UserConfig::downloads_sort` и опциональная секция `[dock]` (`downloads_sort`) с обратной совместимостью и roundtrip-тестом.
  - В `crates/rusty-compositor/src/shell_render/context_menu.rs`, `tokens.rs`, `dock.rs` и `mod.rs` реализована единая подсистема `ShellOverlayView::ContextMenu` (Liquid Glass `Popover` с радиусом 16 px (`MENU_POPOVER_RADIUS`), ведущими иконками `UiGlyphKind`, 1 px разделителями, галочками, подсказками, каскадными подменю `>` с зазором 4 px и автопереворотом, мультимониторным clamping для primary и secondary выходов, hover/hit-test и клавиатурной навигацией `Up`/`Down`/`Left`/`Right`/`Return`/`Escape`).
  - В `crates/rusty-compositor/src/state/context_menu_actions.rs`, `mod.rs`, `shell_actions.rs`, `launcher.rs`, `frame_composer.rs` и `input/{mod.rs,keyboard.rs,pointer.rs,shell_pointer.rs}` реализованы и подключены:
    - ПКМ (`button == 273`) на пустом Desktop на основном и дополнительных мониторах (`New Folder`, `Open Desktop in Files`, `Change Wallpaper...`, `Display Settings...`, `Open Terminal`, `Mission Control`, `Tile Windows` с проверкой `TopBar`/fullscreen/layer-shell);
    - ПКМ на всех слотах Dock (`PinnedFiles`, `PinnedTerminal`, `PinnedBrowser`, `PinnedNotes`, `PinnedSettings` с `Options > Open at Login` и `Show All Windows`; `ToggleOverview` с `Switch Desktop > Desktop 1..9`; `WindowSlot(idx)` с `Minimize`, `Move to Desktop >`, динамическим `Quit` ↔ `Force Quit` по `Alt`; `PinnedDownloads` с превью до 8 файлов, `Sort By >` и сохранением в `[dock].downloads_sort`; `PinnedTrash` с `Open Trash` и безопасным фоновым `Empty Trash...` через `std::thread::spawn` + `mpsc` + `drain_trash_empty_results()`);
    - взаимное закрытие оверлеев и рендер поверх secondary output в `frame_composer.rs`.
  - Оформлены `docs/adr/0029-context-menus-and-dock-actions.md`, `D-037` в `DECISIONS.md` и обновлена `ARCHITECTURE.md`.
  - Добавлено 7 новых unit-тестов (`shell_render::tests`: 3 теста на clamping/cascading submenu/keyboard nav; `state::tests`: 4 теста на Desktop `New Folder` + multi-monitor + dismiss/reopen, Dock window `Alt -> Force Quit` + `Open at Login`, `Downloads` sort + `config.toml` persistence, и безопасный фоновый `Empty Trash`).
- **Verification:**
  - `cargo fmt --all -- --check` — pass;
  - `cargo clippy --workspace --all-targets --offline -- -D warnings` — pass (0 warnings);
  - `cargo test --workspace --offline` — pass (173 passed, 0 failed, 0 ignored: `rusty_animation` 4, `rusty_compositor` 128, `rusty_ipc` 3, `rusty_services` 26, `rusty_shell` 5, `rusty_wm` 7);
  - `cargo run -p rusty-compositor --offline -- --check` — pass (`finished cleanly (mode: check-only)`);
  - `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` — pass (`rendered_frames=3`, `finished cleanly (mode: nested-winit)`).
- **Risks:** Новые зависимости и `unsafe` не добавлялись (`unsafe_code = "deny"` сохранён); `Force Quit` (`kill -KILL`) защищён проверками `pid > 1` и `pid != self_pid`, а удаление корзины проверяет имя каталога `Trash` и удаляет симлинки без перехода по цели.

### RUSTYOS-065 — Самостоятельный Wayland-файловый менеджер `apps/rusty-files` (Golden Gate, Этап A) и перевод композитора без сторонних ФМ

- **Status:** Done (`RUSTYOS-065` Вариант 1 — утверждённый объём выполнен)
- **Scope:**
  - В `crates/rusty-services/src/file_manager.rs` реализован бэкенд файлового менеджера: XDG-каталоги и `user-dirs.dirs` (`UserFileLocations`), обнаружение томов `/proc/mounts` (`discover_mounts`, `parse_proc_mounts`), `.desktop`-приложения (`scan_applications_in`), недавние файлы (`.xbel` + shallow scan), сетевые/shared точки, `FreeDesktop` Trash (`list_trash_entries`, `move_to_trash`, `restore_from_trash`, `empty_trash`), `create_unique_folder`, фильтрация/сортировка (`filter_and_sort_entries`), форматтеры и атомарное хранение тегов/настроек в `~/.config/rustyos/file_tags.toml` (`FileTagsConfig`).
  - Создано самостоятельное CSD Wayland-приложение `apps/rusty-files` (`src/{main.rs,wayland.rs,state.rs,layout.rs,render.rs}`) поверх уже имеющихся `smithay-client-toolkit`, `wayland-client` (`wl_keyboard` evdev без неблокирующей `xkbcommon 0.7`) и `fontdue` с неблокирующим фоновым чтением каталогов через `mpsc`.
  - Реализована визуальная схема macOS 27 Golden Gate (`LightGoldenGate` по умолчанию + переключатель `DarkPurpleGlass`, `--light`/`--dark`/`--path`/`--trash`/`--recents`): плавающий sidebar с traffic lights и секциями *Recents / Shared / Favorites / Locations / Tags*, единый верхний toolbar (`< | >`, View Switcher с активными **Columns** (Miller Columns + Preview) и **List** и слотами **Icons**/**Gallery** под Этап B (`RUSTYOS-066`), Sort, Actions, Search `Ctrl+F`), баннер Trash (`Empty`), контекстные меню и персистентность тегов.
  - В `crates/rusty-compositor` (`launcher.rs`, `frame_composer.rs`, `state/{mod.rs,shell_actions.rs,context_menu_actions.rs,tests.rs}`, `shell_render/top_bar.rs`) и `apps/rusty-shell/src/lib.rs` удалены ссылки на сторонние ФМ (`nautilus`, `dolphin`, `thunar`, `nemo`, `pcmanfm`), добавлен поиск бинарника рядом с композитором (`resolve_program_path_with`, `launch_program_with_args`), и все маршруты (`Dock::PinnedFiles`, `Dock::PinnedDownloads`, `Dock::PinnedTrash`, `DesktopOpenFiles`, каталоги `DownloadsOpenEntry`) переведены на `rusty-files` (плюс изоляция `UserConfig::default()` в `cfg!(test)` для воспроизводимости unit-тестов).
  - Оформлены `docs/adr/0030-rusty-files-application.md`, `D-038` в `DECISIONS.md` и обновлена `ARCHITECTURE.md`.
- **Verification:**
  - `cargo fmt --all -- --check` — pass;
  - `cargo clippy --workspace --all-targets --offline -- -D warnings` — pass (0 warnings);
  - `cargo test --workspace --offline` — pass (185 passed, 0 failed, 0 ignored: `rusty_animation` 4, `rusty_compositor` 130, `rusty_files` 5, `rusty_ipc` 3, `rusty_services` 31, `rusty_shell` 5, `rusty_wm` 7);
  - `cargo run -p rusty-compositor --offline -- --check` — pass (`finished cleanly (mode: check-only)`);
  - `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` — pass (`rendered_frames=3`, `finished cleanly (mode: nested-winit)`).
- **Risks:** Новые внешние зависимости и `unsafe` не добавлялись (`unsafe_code = "deny"` сохранён); `Icons` grid и `Gallery` cover flow + `Quick Look` оставлены под Этап B (`RUSTYOS-066`) с явным статус-уведомлением при клике; визуальная отрисовка `rusty-files` в интерактивной Wayland-сессии проверялась unit-рендером светлой/тёмной тем (live окно `rusty-files` рекомендуется открыть вручную внутри `--winit`).

### RUSTYOS-066 — Этап B `apps/rusty-files` до паритета Golden Gate и общий декодер `rusty_services::image_decode`

- **Status:** Done (`RUSTYOS-066` Вариант 1 — утверждённый объём выполнен)
- **Scope:**
  - В `crates/rusty-services/src/image_decode.rs` вынесены из `crates/rusty-compositor/src/icons.rs` и дополнены pure-Rust декодер PNG/PPM (`decode_png_rgba`, `decode_png_rgba_limited`, `decode_ppm_rgba_limited`, `load_raster_preview_bgra`, `read_image_dimensions`), билинейный скейлер (`scale_rgba_to_premul_bgra`, `scale_rgba_rect_to_premul_bgra`, `scale_cover_rgba_to_premul_bgra`, `fit_aspect_dimensions`), загрузчики `load_png_wallpaper_bgra` и `load_circular_avatar_bgra`; `crates/rusty-compositor/src/icons.rs` переведён на `rusty_services::image_decode`.
  - В `crates/rusty-services/src/file_manager.rs` включены все 4 режима `FileViewMode::{Icons, List, Columns, Gallery}` (`is_supported() == true`), добавлены `inspect_entry_info` (`FileDetailedInfo`), `read_text_preview` (`TextPreview`), `rename_entry`, `expand_go_to_input`/`resolve_go_to_path`/`suggest_go_to_paths` и chunked перенос `copy_or_move_with_progress` (`TransferKind`, `TransferConflictDecision::{Replace, KeepBoth, Skip, Stop}`, `TransferProgress`, `TransferOutcome`).
  - В `apps/rusty-files` (`src/{views,dialogs,icons,layout,render,state,wayland}.rs`) реализованы:
    - режим **Icons** (`IconsGridLayout`, сетка `100x104 px` с миниатюрами и бейджами тегов) и режим **Gallery** (`GalleryLayout`: крупное превью с фоновым декодированием PNG/PPM/текста, нижняя лента миниатюр, правая панель `Information` и кнопки действий);
    - верхняя полоса вкладок (`TabBarLayout`, `+`, `Ctrl+T`, `Ctrl+W`, `Ctrl+Tab` / `Ctrl+Shift+Tab`), кликабельный нижний `Path Bar`, встроенный `Quick Look` (`Space`), `Get Info` (`Ctrl+I`), `Go to Folder...` (`Ctrl+Shift+G`), inline `Rename` (`F2`), буфер `Copy`/`Cut`/`Paste`/`Duplicate` (`Ctrl+C`/`X`/`V`/`D`) с фоновой пилюлей прогресса/отмены и модальным окном конфликта;
    - чёткие pixel-aligned UI-иконки (`snap_half`/`snap_edge`), doc-иконки с загнутым углом (`draw_dogear_document_icon`) и 4x4 SSAA Golden Gate папки (`draw_entry_icon`, `draw_large_entry_artwork`) с гравировкой категорий и окраской тегами.
  - Оформлены `docs/adr/0031-rusty-files-stage-b.md`, `D-039` в `DECISIONS.md` и обновлена `ARCHITECTURE.md`.
- **Verification:**
  - `cargo fmt --all -- --check` — pass;
  - `cargo clippy --workspace --all-targets --offline -- -D warnings` — pass (0 warnings);
  - `cargo test --workspace --offline` — pass (195 passed, 0 failed, 0 ignored: `rusty_animation` 4, `rusty_compositor` 130, `rusty_files` 9, `rusty_ipc` 3, `rusty_services` 37, `rusty_shell` 5, `rusty_wm` 7);
  - `cargo run -p rusty-compositor --offline -- --check` — pass (`finished cleanly (mode: check-only)`);
  - `cargo run -p rusty-compositor --offline -- --winit --max-frames 3` — pass (`rendered_frames=3`, `finished cleanly (mode: nested-winit)`);
  - `cargo run -p rusty-files --offline -- --help` — pass (`Usage: rusty-files [--path <dir>] [--trash] [--recents] [--light | --dark]`).
- **Risks:** Новые внешние зависимости и `unsafe` не добавлялись (`unsafe_code = "deny"` сохранён); встроенный декодер превью покрывает PNG, PPM (`P3`/`P6`) и UTF-8 текст (дополнительные форматы вроде SVG/PDF/JPEG потребуют отдельных декодеров в следующих задачах); интерактивный запуск живого окна `rusty-files` в Wayland-сессии `--winit` отдельно не снимался скриншотом — отрисовка проверена offscreen unit-тестами `rusty_files` (`stage_b_icons_gallery_tabs_and_overlays_render_within_rounded_clip`, `renders_golden_gate_light_and_dark_themes_cleanly`).

### RUSTYOS-067 — Отказ от темы Purple Glass и реализация светлой и тёмной тем Golden Gate (Light & Dark)

- **Status:** Done (`RUSTYOS-067` Вариант 1 — утверждённый объём выполнен)
- **Scope:**
  - Полностью удалена старая фиолетовая палитра `RustyOS Purple Glass` (`#0F0A19`, `#161023`, `#20182E`, `#292037`, `#A56EFF`, `#7650B5`) из `crates/rusty-compositor`, `apps/rusty-files`, `apps/rusty-shell` и `DESIGN.md`.
  - В `crates/rusty-compositor/src/config.rs` добавлен `AppearanceMode::{Light, Dark}` (по умолчанию `Light`), сохраняемый как `theme = "light" | "dark"` в `[ui]` файла `~/.config/rustyos/config.toml`; во вкладку `System Settings -> Appearance` (`modals.rs`, `state/shell_actions.rs`) добавлен переключатель `Appearance Theme` (`SettingsHit::ToggleAppearance`).
  - В `crates/rusty-compositor/src/shell_render/tokens.rs` реализована палитра `ThemePalette::{current_for, active}` для `Golden Gate Light` (`#F5F6F8` стекло, `#1D1F24` текст, `#007AFF` акцент) и `Golden Gate Dark` (`#262629` графит, `#F2F2F7` текст, `#0A84FF` акцент); в `frame_composer.rs` активная тема синхронизируется с `state.user_config.appearance`.
  - В `crates/rusty-compositor/src/surfaces/{mod,sdf}.rs` ключ `GlassPanelKey` дополнен признаком `is_dark`, реализована генерация матового светлого и тёмного жидкого стекла (`sample_glass_panel_pixel_themed`) и процедурных шёлковых обоев `Golden Gate` (`sample_wallpaper_pixel_themed` по референсу `desktop.jpeg`).
  - Контекстные меню (`context_menu.rs`), выпадающие меню `TopBar` (`shell_render/mod.rs`), `Control Center` (`quick_settings.rs`), `Spotlight`/`Overview` (`spotlight_overview.rs`), `System Settings`/`Polkit`/`Portal` (`modals.rs`) и `SSD` заголовки (`ssd.rs`) переведены на `ThemePalette::active()` в точном соответствии с `context-menu.jpeg` и референсами `macOS 27 Golden Gate`.
  - В `apps/rusty-files` тёмная тема переведена на нейтральный графит `Golden Gate Dark` (`#1E1E20` окно, `#26262A` сайдбар/попапы, `#0A84FF` акцент по референсу `finder-window-dark.jpeg`) с автоопределением `[ui] theme` из `~/.config/rustyos/config.toml` (`UserFileLocations::detect_system_theme`).
  - Оформлены `docs/adr/0032-golden-gate-light-dark-themes.md`, `D-040` в `DECISIONS.md` и обновлён `DESIGN.md` (v3.0).

### RUSTYOS-068 — Динамическое app-aware меню TopBar по профилю активного окна

- **Goal:** Сделать меню `TopBar` по-настоящему контекстным к активному приложению (как в macOS Menu Bar): при фокусе сторонних приложений менять не только заголовок приложения, но и набор категорий и команд выпадающих меню.
- **Status:** Done (`RUSTYOS-068` Вариант 1 — утверждённый объём выполнен)
- **Dependencies:** `RUSTYOS-067`, `ADR-0033`
- **Scope:**
  - В `crates/rusty-compositor/src/shell_render/top_bar_menus.rs` введены `TopBarAppProfile::{Files, Browser, Terminal, Editor, GenericApp}`, `AppShortcutAction`, `ResolvedTopBarMenuItem`, классификатор `classify_top_bar_app(app_id, title)` и резолвер `resolve_top_bar_menu_entries(profile, active_app_name, menu)`.
  - В `crates/rusty-compositor/src/shell_render/top_bar.rs` и `shell_render/mod.rs` добавлены профильные поля `TopBarTelemetry::{active_app_name, app_profile}`, `TopBarLeftLayout::has_go` (скрытие `Go` вне `Files` со сдвигом `Window`/`Help` и workspace/layout-полосы), профильные функции геометрии и hit-testing (`*_for_profile`) и варианты действий `AboutActiveApp`, `ToggleFullscreenFocused`, `QuitActiveApp`, `ForceQuitActiveApp`, `AppShortcut(AppShortcutAction)`.
  - В `crates/rusty-compositor/src/state/{shell_actions,context_menu_actions}.rs`, `frame_composer.rs` и `input/{shell_pointer,keyboard}.rs` подключён единый источник `active_top_bar_app_context`, защита от устаревшего меню `Go` и раскладконезависимая (zero-`unsafe`) отправка `Ctrl+[Shift+]<Key>` через `KeyboardHandle::with_xkb_state` (`raw_syms_for_key_in_layout`) + `input_intercept` / `input_forward`.
  - Оформлены `docs/adr/0033-app-aware-topbar-menus.md` и запись `D-041` в `DECISIONS.md`.
- **Tests:**
  - `shell_render::tests::{app_aware_topbar_classification_and_layout_hides_go_for_non_files, app_aware_topbar_dropdowns_and_popover_hit_testing_match_profile}`
  - `input::tests::app_shortcut_resolves_across_non_latin_xkb_layouts_and_guards_focus`
  - `state::tests::top_bar_active_app_context_and_about_action_default_to_files`
- **Risks:** Сторонние приложения, не поддерживающие стандартные комбинации `Ctrl+...` или имеющие нестандартные XKB-keymap, в профильном режиме опираются на эвристику `app_id`/`title`; при необходимости точной синхронизации произвольных пунктов в будущем может быть добавлен опциональный DBus menu export поверх `TopBarAppProfile`.

### RUSTYOS-069 — GLES Dual-Kawase Backdrop Blur + Liquid Glass шейдер, зачистка остатков purple и обновление документации/скиллов

- **Goal:** Обеспечить аппаратное многопроходное размытие фона (Dual-Kawase) и краевую оптику Liquid Glass для панелей оболочки поверх реальных окон и обоев, завершить зачистку остатков фиолетовой палитры и синхронизировать документацию/скиллы с темой `Golden Gate Light & Dark`.
- **Status:** Done (`RUSTYOS-069` Вариант 1 — утверждённый объём выполнен)
- **Dependencies:** `RUSTYOS-067`, `ADR-0034`
- **Scope:**
  - В `crates/rusty-compositor/src/surfaces/liquid_glass.rs` без `unsafe` (`unsafe_code = "deny"`) реализован `LiquidGlassPipeline`: шейдеры GLSL ES 1.00 `KAWASE_DOWN_FRAG`, `KAWASE_UP_FRAG`, `LIQUID_GLASS_FRAG` (analytical rounded-box SDF, edge lensing/dispersion, vibrance boost, theme-aware Light/Dark tint, directional specular rim), 5-проходный ping-pong FBO (`1 -> 1/2 -> 1/4 -> 1/8 -> 1/4 -> 1/2`), `OutputDamageTracker` для пропуска проходов при отсутствии damage за стеклом, `LiquidGlassElement` со стабильными per-slot `Id` и расширенным `DamageBag`.
  - В `crates/rusty-compositor/src/surfaces/mod.rs`, `shell_render/mod.rs`, `shell_render/tokens.rs` и `frame_composer.rs` подключены `CompositorRenderElement::{ShaderTexture, ScaledShaderTexture}`, helper `build_backdrop_render_elements` и переключение на GLES Liquid Glass для `Dock` (`GlassPanelStyle::Dock`), меню/оверлеев (`Popover`) и плиток `Control Center` (`ControlModule`) с прозрачным runtime CPU fallback при `screen_locked`, `ext_session_locked`, `reduce_transparency = true` или недоступности FBO/шейдеров.
  - В `apps/rusty-files/src/views.rs` удалены оставшиеся фиолетовые RGBA (`rgba(38, 29, 54, 255)` -> нейтральный графит `rgba(48, 48, 54, 255)` и `rgba(148, 134, 248, 255)` -> `rgba(72, 164, 255, 255)`), в `surfaces/tests.rs` обновлён устаревший комментарий про plum-slate.
  - Обновлены `DESIGN.md §12`, скиллы `.agents/skills/{ui-ux-design,desktop-shell,gpu-rendering}/SKILL.md`, оформлены `docs/adr/0034-gles-liquid-glass-backdrop-blur.md` и запись `D-042` в `DECISIONS.md`.
- **Tests:**
  - `surfaces::liquid_glass::tests::{shader_sources_include_smithay_contract_defines_and_uniforms, blur_fbo_ladder_scales_down_and_never_underflows, panel_src_and_uv_respects_hidpi_scale_and_safe_bounds, backdrop_damage_expansion_covers_blur_pad_and_clamps_to_half_fbo, liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive}`
  - Внешняя проверка синтаксиса всех GLSL ES 1.00 вариантов (`NO_ALPHA`, `DEBUG_FLAGS`, `EXTERNAL`) через `/usr/bin/glslangValidator`.
  - `cargo clippy --workspace --all-targets -- -D warnings` и workspace tests (`rusty-compositor`, `rusty-files`).
- **Risks:** Реальный рендеринг GLES FBO требует активного GPU/EGL-контекста (в headless CI проверяется математика FBO/UV/damage, валидность GLSL-вариантов и переключение в CPU fallback; визуальная подстройка интенсивности преломления/тинта поверх живых обоев выполняется на реальном `--winit` / `--drm` дисплее).

### RUSTYOS-070 — Визуальная доводка 1:1 к macOS 27 Golden Gate (`Light & Dark`)

- **Goal:** Довести композитинг Liquid Glass (`Light` и `Dark`), оверлеи оболочки (`Spotlight`, `WindowSwitcher`, `LockScreen`, `Settings`, `TopBar`) и окно `rusty-files` до визуального соответствия референсам `macOS 27 Golden Gate`.
- **Status:** Done (`RUSTYOS-070` Вариант 1 — утверждённый объём выполнен)
- **Dependencies:** `RUSTYOS-067`, `RUSTYOS-069`, `ADR-0027`, `ADR-0032`, `ADR-0034`
- **Scope:**
  - В `crates/rusty-compositor/src/surfaces/liquid_glass.rs` и `sdf.rs` откалиброваны `Light`/`Dark` профили `compute_glass_material` (`Dock`, `Popover`, `ControlModule`) и `LIQUID_GLASS_FRAG` (rim/meniscus caustic и ослабленный CPU fallback без белёсой пелены в Light mode), усилен S-curve контраст обоев (`golden_gate_sky_rgb`, `apply_golden_gate_folds`), а также зачищен legacy purple в fallback-аватаре `sample_fallback_avatar_pixel`.
  - В `crates/rusty-compositor/src/shell_render/spotlight_overview.rs`, `mod.rs` и `tests.rs` реализован двухкапсульный `Spotlight` (`spotlight_search_pill_rect` h=54 r=27 + 10 px gap + `spotlight_results_rect` r=22) на GPU `LiquidGlassElement` с иконками приложений и обновлённым hit-testing; `WindowSwitcher` переведён на единый `LiquidGlassElement` (`24 px`) с theme-aware текстом (`ThemePalette::active()`), без рамочных карточек и с мягкой безрамочной капсулой выделения.
  - В `crates/rusty-compositor/src/frame_composer.rs` и `shell_render/lock_screen.rs` для `LockScreen` подключён изолированный wallpaper-only GLES blur backdrop (`wallpaper_only`) и GPU `LiquidGlassElement` для поля ввода пароля без утечки окон рабочего стола.
  - В `crates/rusty-compositor/src/shell_render/modals.rs`, `frame_composer.rs` и `input/pointer.rs` снят полноэкранный затемняющий scrim для `Settings` и выстроен z-order/hit-testing под `TopBar`, открытыми меню и `Dock`.
  - В `crates/rusty-compositor/src/shell_render/top_bar.rs`, `mod.rs`, `ssd.rs` и `tokens.rs` убраны подложки за `TopBar`-индикаторами, рамки у пунктов меню, purple-tinted белые оттенки, а индикаторы запущенных приложений Dock переведены на нейтральный графит (`Light`) и светло-серый (`Dark`).
  - В `apps/rusty-files/src/{icons,views,layout,render,wayland}.rs` палитры тёмных папок/дисков переведены на `Sky Blue` без legacy purple и удалена отладочная кнопка переключения темы из `render_status_bar`.
- **Tests:**
  - `shell_render::tests::{spotlight_click_detects_rows_inside_and_outside_modal, switcher_layout_and_click_hit_testing_match_bounds, settings_modal_hit_testing_covers_tabs_controls_and_buttons}`
  - `surfaces::liquid_glass::tests::shader_sources_include_smithay_contract_defines_and_uniforms` + `/usr/bin/glslangValidator` проверка всех `#define` вариантов `KAWASE_DOWN_FRAG`, `KAWASE_UP_FRAG`, `LIQUID_GLASS_FRAG`
  - `rusty-files` render tests (`renders_golden_gate_light_and_dark_themes_cleanly`, `stage_b_icons_gallery_tabs_and_overlays_render_within_rounded_clip`)
  - Живой `--winit` прогон на `PLATFORM_WAYLAND_KHR` и `PLATFORM_X11_KHR` в `Light` и `Dark` + визуальная приёмка скриншотов `Desktop`, `TopBar Menu`, `Settings`, `Control Center`, `Spotlight`, `WindowSwitcher` и `LockScreen`.
- **Risks:** На X11 под управлением хостового KWin комбинация `Alt+Tab` перехватывается хостовым WM до XWayland, поэтому в автоматическом X11 скриншот-QA использовался `Ctrl+Alt+Tab`; визуальная приёмка на чистом DRM/KMS (`--drm`) остаётся отдельным этапом.

### RUSTYOS-071 — Приведение `rusty-files` (Finder) и тени окон к macOS 27 Golden Gate (`Light & Dark`)

- **Goal:** Довести хром, стартовые настройки, поиск и скроллбары `rusty-files` + оконную тень композитора до визуального и интерактивного паритета 1:1 с референсами Finder `macOS 27 Golden Gate`.
- **Status:** Done (`RUSTYOS-071` Вариант 1 — утверждённый объём выполнен)
- **Dependencies:** `RUSTYOS-068`, `RUSTYOS-070`, `ADR-0030`, `ADR-0032`
- **Acceptance Criteria:**
  - Левая кнопка правого блока действий заменена на Share (`OpenPopupMenu::ShareMenu`: `Copy Path`, `Copy Link (file://)`, `New Folder`) при сохранении `New Folder` в меню `...`, контекстном меню и шорткатах (`Ctrl+Shift+N` / `Ctrl+N`).
  - Активная капсула View Switcher отрисовывается без внутренней рамки; глиф Sort/Group By переведён на сетку `2×3` + шеврон.
  - Капсула Search адаптивно расширяется с плейсхолдером `"Search"` при ширине окна `>= 880` px даже без фокуса и сворачивается на узких окнах; при активном или непустом поиске открывается `Search Scope Bar` (`ThisFolder` / `Home` + `Clear`) с неблокирующим рекурсивным поиском по Home.
  - Стартовый режим изменён на `FileViewMode::Icons`; добавлена настройка `show_path_bar: bool` (`default = false`) с командой `TogglePathBar` (`"Show Path Bar"`), Path Bar скрыт по умолчанию, Status Bar центрирован и не дублирует заголовок.
  - Скроллбары работают как затухающие overlay-пилюли (`overlay_scrollbar_alpha`) и не перекрывают шевроны `>` в Columns; пилюли toolbar получили 1 px верхний блик, а композиторная тень окна — углублённый градиент без ступенчатого бандинга.
- **Scope:**
  - `crates/rusty-services/src/file_manager.rs`: `FileViewMode` `#[default]` переключён на `Icons`, добавлено поле `show_path_bar` (`default = false`) в `FileTagsConfig` и функция `search_directory_recursive`.
  - `apps/rusty-files/src/icons.rs`: обновлена `draw_sort_pill_glyph` (`2×3` + шеврон), добавлена `draw_share_glyph`, удалена неиспользуемая `draw_folder_plus_glyph`.
  - `apps/rusty-files/src/state.rs`: добавлены `SearchScope` (`ThisFolder`, `Home`), `OpenPopupMenu::ShareMenu`, `HomeSearchResult`, неблокирующий `spawn_home_search_job` / `poll_home_search_jobs`, `toggle_path_bar`, `set_search_scope`, состояние затухания `overlay_scrollbar_alpha` (`note_scroll_activity`, `tick_overlay_scrollbar`).
  - `apps/rusty-files/src/{layout,wayland,views,render}.rs`: адаптивная ширина Search (`width >= 880`), `SearchScopeBarLayout` и hit-test (`SearchScope`, `SearchScopeClear`), `share_menu_rows` + `TogglePathBar`, `overlay_scrollbar_color`, `draw_glass_pill`, безрамочная активная капсула View Switcher, сдвинутые левее шевроны Miller Columns и центрированный Status Bar.
  - `crates/rusty-compositor/src/shell_render/{ssd,tokens}.rs`: усилены многослойные параметры `generate_window_shadow_elements` и адаптивное число полос в `push_soft_shadow` (`(spread / 2).clamp(5, 18)`).
- **Tests:**
  - `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace -- --test-threads=1` (`211 passed; 0 failed`: `rusty_animation` 4, `rusty_compositor` 142, `rusty_files` 13, `rusty_ipc` 3, `rusty_services` 37, `rusty_shell` 5, `rusty_wm` 7)
  - Включая `layout::tests::search_scope_clear_and_share_button_hit_test`, `file_manager::tests::stage_b_view_modes_roundtrip_in_tags_config`, обновлённые тесты `state::tests` и `render::tests`
  - Живые прогоны `rusty-compositor --winit --max-frames 35` на `PLATFORM_WAYLAND_KHR` и `PLATFORM_X11_KHR` без ошибок EGL/шейдеров + визуальная проверка скриншотов `Light` & `Dark` (`Icons`, `Columns`, `Search`, `ShareMenu`, максимизированный wide `Search`).
- **Risks:** При быстрой автоматической X11 XTest-съёмке одиночный кадр без микросдвига курсора иногда считывает предыдущий буфер обмена Mesa/Winit до завершения асинхронного commit клиента; ручное интерактивное взаимодействие и живой Wayland/X11 стек отрабатывают без потерь.

### RUSTYOS-072 — Full-bleed левая панель `rusty-files` 1:1 к Finder `macOS 27 Golden Gate`

- **Goal:** Перевести левую панель (sidebar) `rusty-files` к точному 1:1 виду с Finder-референсами (`27-Golden-Gate-Finder-Home-Folder.png`, `Applications.png`, `Finder-Network.png`), устранить конфликт формулировки в `DESIGN.md §12.3` и довести ритм, палитру, светофоры и векторные глифы сайдбара.
- **Status:** Done (`RUSTYOS-072` Вариант 1 — утверждённый объём выполнен)
- **Dependencies:** `RUSTYOS-071`, `ADR-0030`, `ADR-0032`
- **Acceptance Criteria:**
  - Левая панель переведена в full-bleed edge-to-edge режим (`x=0, y=0..height`, ширина `SIDEBAR_WIDTH = 196`, скругление по внешним левым углам окна `WINDOW_CORNER_RADIUS = 20`) с тонким `1 px` правым разделителем без внешней `10 px` окантовки и рамки.
  - Светофоры выровнены концентрично левому верхнему углу (`(25, 25)`, `(45, 25)`, `(65, 25)`, кликабельная зона `14×14`), увеличены отступ до списка (`y = 50`) и межсекционные интервалы (`SIDEBAR_SECTION_HEIGHT = 24`, `hdr_y = y + 8`, `SIDEBAR_ROW_HEIGHT = 22`) с нейтральной пилюлей и `SemiBold` на активном пункте.
  - Иконка `SidebarIconKind::Network` в секции `Locations` перекрашена из `accent` в нейтрально-графитовый `neutral`, утончены глифы сайдбара (`Applications`, `Desktop`, `Documents`, `Downloads`, `Home`, `Volume`, `Network`, `Trash`, `Tag`).
  - В `DESIGN.md §12.3` зафиксировано различие между full-bleed сайдбаром Finder (`rusty-files`) и плавающей `inset`-карточкой `System Settings` / `Notes`.
- **Scope:**
  - `DESIGN.md` (§12.3): уточнена спецификация full-bleed Finder sidebar по референсам `INDEX.md`.
  - `apps/rusty-files/src/state.rs`: обновлены `SIDEBAR_ROW_HEIGHT = 22` и `SIDEBAR_SECTION_HEIGHT = 24`.
  - `apps/rusty-files/src/layout.rs`: убран `SIDEBAR_CORNER_RADIUS`, обновлены `SIDEBAR_WIDTH = 196`, `sidebar_rect`, `sidebar_clip_rect`, `traffic_close`/`traffic_min`/`traffic_max`, `layout_sidebar_rows`, `hit_test_chrome` и `golden_gate_layout_and_hit_testing_work`.
  - `apps/rusty-files/src/icons.rs`: `SidebarIconKind::Network` переведён на `neutral`, утончены контуры `draw_sidebar_icon` и хелперов (вкл. слим-корпус `draw_drive_volume_icon`).
  - `apps/rusty-files/src/render.rs`: обновлены `palette_for` (`sidebar_bg`, `sidebar_border`, `sidebar_active`, `text_muted` без синего отлива в Light/Dark), `render_sidebar` (edge-to-edge заливка + 1 px правый разделитель + светофоры `(25, 45, 65; 25)`), и добавлены assertions в `renders_golden_gate_light_and_dark_themes_cleanly` на full-bleed край, разделитель и нейтральность `Network`.
- **Tests:**
  - `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins`
  - `cargo test --workspace -- --test-threads=1` (`211 passed; 0 failed`: `rusty_animation` 4, `rusty_compositor` 142, `rusty_files` 13, `rusty_ipc` 3, `rusty_services` 37, `rusty_shell` 5, `rusty_wm` 7)
  - Живые `--winit --max-frames 35` smoke-прогоны на `PLATFORM_WAYLAND_KHR` и `PLATFORM_X11_KHR` (`late=0 missed=0`, без ошибок EGL/шейдеров) и визуальная проверка скриншотов `Light` & `Dark` (`qa-072-light-icons.png`, `qa-072-light-wide.png`, `qa-072-dark-icons.png`).
- **Risks:** На стартовой высоте `768×461` при наличии примонтированного дополнительного тома (`RustyOS HD`) в `Locations` самый нижний тег `Purple` уходит на прокрутку и полностью виден при растяжении/максимизации окна (штатное поведение скролла сайдбара).

### RUSTYOS-073 — App-centric Dock, локальный hover, Stacks и Desktop & Dock в стиле macOS 27 Golden Gate

- **Status:** Done (`RUSTYOS-073 Вариант 1` — утверждённый объём выполнен, волна заменена на локальный hover)
- **Goal:** Привести поведение, физику, визуал и настройки Dock к стандарту `macOS 27 Golden Gate`: единая модель «одно приложение — один слот», спокойный локальный hover только для наведённой иконки, анимации запуска и сворачивания, края экрана + Auto-hide, графический `Downloads Stack`, внутренний drag-реордер/открепление и вкладка `System Settings -> Desktop & Dock`.
- **Dependencies:** `RUSTYOS-069` (`Liquid Glass`), `RUSTYOS-072`, `ADR-0035`, `D-043`.
- **Scope:**
  - App-centric группировка по `canonical_dock_app_key`, динамические `[dock].pinned_apps`, секция открытых + до `4` недавних (`dock_show_recent`), отдельные мини-слоты свёрнутых окон или сворачивание в иконку приложения (`dock_minimize_into_app`), ЛКМ-фокус/восстановление/циклирование без автосворачивания активного окна, ПКМ-меню приложения (список окон `DockFocusWindow`, `Keep in Dock` / `Remove from Dock`, `Open at Login`, `Show All Windows` -> фильтрованный App Exposé, `Quit` / `Alt+Force Quit`).
  - Локальный hover только для наведённой иконки (`+8 px` масштаб по центру своего слота, подъём `+6 px`, или `0 px` масштаб и `+2 px` статический подъём при `reduced_motion`, без волны по соседям и растяжения капсулы Dock) со стеклянной tooltip-пилюлей, настраиваемый размер иконок (`36..=68 px`), `Launch bounce` (`dock_animate_opening`), позиции `Bottom`/`Left`/`Right` (`usable_work_area_with_dock`), `Auto-hide`, безопасный (`unsafe_code = "deny"`) GLES-пайплайн `WindowMinimizePipeline` (`Genie`/`Scale` в целевой слот Dock с инкрементальным `DamageBag`, выносом из `backdrop_elements` без покадрового Dual-Kawase blur и бесшовной S-кривой без рывка при `t = 0`), графический поповер `Downloads Stack` (`Automatic`/`Fan`/`Grid`/`List`, `Display as`, `Sort By`: `Date Added`/`Date Modified`/`Name`/`Size`/`Kind`) и внутренний pointer drag-реордер / открепление при вытягивании (`>= 52 px`, пилюля `"Remove"`).
  - Вкладка `System Settings -> Desktop & Dock` (`8` контролов без `Magnification`, синхронизированных с `UserConfig`; старый `[dock].magnification_size` безопасно игнорируется при парсинге `config.toml`), `ADR-0035`, `D-043`, `DESIGN.md §12.2`.
- **Acceptance Criteria:**
  - Закреплённые, недавние и свёрнутые слоты не создают дублей открытых окон; клик по приложению поднимает/фокусирует окно или запускает с bounce-анимацией;
  - Локальный hover (`+8 px` / `+6 px`), `Position`, `Auto-hide`, `Genie`/`Scale` сворачивание/разворачивание без пропусков кадров, `Downloads Stack` (`Fan`/`Grid`/`List`) и drag-реордер/открепление работают на `Bottom`, `Left` и `Right` при фиксированных слотах и ширине капсулы;
  - `UserConfig` и `System Settings -> Desktop & Dock` сохраняют параметры `[dock]` и безопасно загружают legacy-конфиги с `magnification_size`;
  - Workspace-проверки, живые `--winit` (`Wayland` и `X11`), `--drm-probe` и визуальная проверка `Light` & `Dark` (включая mid-flight сворачивание и разворачивание `Genie`) пройдены без EGL/шейдерных ошибок.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins`
  - `cargo test --workspace -- --test-threads=1` (`218 passed; 0 failed`: `rusty_animation` 4, `rusty_compositor` 149 + 0 bin, `rusty_files` 13, `rusty_ipc` 3, `rusty_services` 37, `rusty_shell` 5 + 0 bin, `rusty_wm` 7)
  - Живые `--winit --max-frames 35` smoke-прогоны на `PLATFORM_WAYLAND_KHR` (`late=0 missed=0`) и `PLATFORM_X11_KHR` (`late=1 missed=0`) + `./target/debug/rusty-compositor --drm-probe` без ошибок шейдеров/FBO/паник
  - Интерактивный `--winit` X11 Light/Dark прогон (`qa_073.py`) и живой 220-кадровый прогон `Genie` minimize & restore (`render_duration_us` p50 `4875 us`, `missed_frames=1`, `dispatch_stalls=0`, скриншоты `qa-073-min-mid.png` и `qa-073-restore-mid.png`, плюс `qa-073-hover-*`).
- **Risks:** На Winit X11-бэкенде в `Xwayland` при полностью статичных простоях `backend.submit(None)` продолжает циклировать swapchain-буферы без нового композитинга (на реальный рендер DRM и интерактивную реакцию при изменении damage не влияет).

### RUSTYOS-074 — Переработка Liquid Glass: удаление 360° белой рамки, диагональный угловой specular и паритет Light/Dark

- **Status:** Done (`RUSTYOS-074 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Устранить замкнутый 360° проволочный контур («белую рамку») и плотный матово-серый оттенок Dark-стекла на капсулах `Dock`, `Control Center`, `Spotlight` и поповеров, приведя физику края (`edge lensing`, угловой диагональный блик, мягкие тени и CPU-fallback) к 1:1 паритету с `macOS 27 Golden Gate`.
- **Dependencies:** `RUSTYOS-069` (`Liquid Glass`, `ADR-0034`), `RUSTYOS-073`.
- **Scope:**
  - `LIQUID_GLASS_FRAG` и `compute_glass_material` (`crates/rusty-compositor/src/surfaces/liquid_glass.rs`): гладкая аналитическая SDF-нормаль без скачков на внутренней диагонали, усиленная краевая рефракция (`6.5..8.5 px` `edge lensing` + дисперсия `0.55 px`), замена замкнутого `0.30 + top/side/bottom` ободка на встречный диагональный `d^7` specular по дугам `top-left` и `bottom-right` (`0.62x`) с затуханием по прямым и `TR`/`BL`-граням, мягкий гауссовский профиль фаски без `shadow_lip`, чистая Light/Dark палитра (прозрачное дымчатое Dark-стекло `0.14..0.24` вместо плотного матового серого).
  - `surfaces/sdf.rs` (`sample_glass_panel_pixel_themed`, `base_glass_layer`, `diagonal_glass_rim`, `sample_shadow_corner_pixel`): CPU-паритет диагонального углового блика и более мягкий кубический спад угловых теней (`(1 - t)^3`).
  - `shell_render/{tokens.rs, mod.rs, quick_settings.rs}`: перевод Dock hover tooltip на `push_liquid_glass_panel` (`GlassPanelStyle::Popover`), смягчение `border_rgba`/`border_highlight_rgba` (`tokens.rs`) и диффузных теней в `Control Center` (`(18, 18)`) и `Spotlight` (`(24, 30)` / `(26, 32)`), кубический спад теневых полос в `push_soft_shadow`.
  - Модульные тесты (`liquid_glass.rs`, `surfaces/tests.rs`) и обновление `DESIGN.md §12.5`.
- **Acceptance Criteria:**
  - На `Dock`, `Control Center`, `Spotlight` и поповерах отсутствует замкнутая 360° белая или тёмная контурная рамка: блик сосредоточен на встречных диагональных дугах `TL`/`BR` и плавно гаснет к `TR`/`BL` и прямым граням;
  - Краевая рефракция физически преломляет фон по кромке без артефактов, Light-капсулы светлые и прозрачные без тёмного ободка тени, Dark-капсулы дымчато-прозрачные;
  - CPU-fallback (`sdf.rs`) воспроизводит ту же диагональную асимметрию без замкнутого контура;
  - GLSL ES 1.00 проверяется во всех препроцессорных комбинациях, пройдены workspace-тесты, `--drm-probe` и живые `--winit` прогоны (`Wayland` + `X11`, `Light` & `Dark`).
- **Tests:**
  - `glslangValidator` по всем 8 комбинациям `#define {NO_ALPHA, DEBUG_FLAGS, EXTERNAL}`
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins`
  - `cargo test --workspace -- --test-threads=1` (`218 passed; 0 failed`)
  - `./target/debug/rusty-compositor --drm-probe`
  - Живые `--winit --max-frames 35` на `Wayland` и `X11` + визуальная сверка Light/Dark PNG (`qa-074-dark-cc-and-dock.png`, `qa-074-dark-dock-hover.png`, `qa-074-dark-spotlight.png`, `qa-074-light-cc-and-dock.png`, `qa-074-light-dock-hover.png`, `qa-074-light-spotlight.png`) без EGL/шейдерных ошибок.
- **Risks:** На однотонно тёмных обоях в Dark-режиме соседние островки `Control Center` разделяются мягкой диффузной тенью и угловыми дугами без яркой обводки; визуальный контраст проверен на дефолтных обоях и в Spotlight.

### RUSTYOS-075 — Бесшовная GLES SDF-тень окон и 9-slice текстурный fallback без ступенек и полос

- **Status:** Done (`RUSTYOS-075 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Устранить видимые горизонтальные ступеньки (`+5/+12 px`), три неконцентрических угла и дискретные `Solid`-полосы под окнами, приведя тени активных и неактивных окон и шелл-оверлеев к единому непрерывному гауссову виду 1:1 с `macOS 27 Golden Gate`.
- **Dependencies:** `RUSTYOS-074`, `ADR-0034`.
- **Scope:**
  - Новый безопасный модуль `crates/rusty-compositor/src/surfaces/window_shadow.rs` (`WindowShadowPipeline`, `WindowShadowSpec`, `WINDOW_SHADOW_FRAG`): единый аналитический GLES SDF-шейдер с непрерывной суммой contact + Y-сдвинутого ambient гауссова ореола (без обрыва под нижней гранью), `GL_FRAGMENT_PRECISION_HIGH`-совместимостью, экрано-пространственным 4×4 упорядоченным дизерингом и кэшированием стабильных `Id` по кадрам через `begin_shadow_frame` (`frame_composer.rs`).
  - `surfaces/sdf.rs` и `surfaces/mod.rs`: единая кривая затухания `shadow_falloff_weight`, новый метод `shadow_edge_element` + `rasterize_shadow_edge_buffer` (растянутые попиксельные 1D `MemoryRenderBuffer`-полосы для 9-slice fallback без `band_count` `Solid`-ступенек), а также `window_shadow_render_element` и `soft_shadow_render_element`.
  - `shell_render/ssd.rs` (`generate_window_shadow_elements`) и `shell_render/tokens.rs` (`push_soft_shadow`): переход с трёх сдвинутых вызовов `push_soft_shadow` (~78 элементов на окно) и дискретных `Solid`-полос на единый `WindowShadowPipeline` элемент (или один непрерывный текстурный 9-slice в CPU-fallback).
  - Тесты (`window_shadow.rs`, `surfaces/tests.rs`) и обновление `DESIGN.md §12.3`.
- **Acceptance Criteria:**
  - Под нижним краем и нижними углами окон (`rusty-files` одиночное и перекрывающееся активное/неактивное) отсутствуют горизонтальные «слои», швы `+5/+12 px`, сдвинутые угловые дуги и полосы бандлинга;
  - Все 8 `#define`-перестановок `WINDOW_SHADOW_FRAG` и 8 перестановок `LIQUID_GLASS_FRAG` проходят `glslangValidator`, пройдены тесты workspace, `--drm-probe` и живые `--winit` прогоны (`Wayland` + `X11`, `Light` & `Dark`).
- **Tests:**
  - `glslangValidator`: `16/16` (`WINDOW_SHADOW_FRAG` + `LIQUID_GLASS_FRAG` по 8 комбинациям `#define {NO_ALPHA, DEBUG_FLAGS, EXTERNAL}`)
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins`
  - `cargo test --workspace -- --test-threads=1` (`220 passed; 0 failed`)
  - `./target/debug/rusty-compositor --drm-probe`
  - Живые `--winit --max-frames 35` (`Wayland` и `X11`) + Light/Dark `rusty-files` скриншоты (`qa-075-dark-window-shadow.png`, `qa-075-dark-overlap-shadow.png`, `qa-075-light-window-shadow.png`, `qa-075-light-overlap-shadow.png`) без EGL/шейдерных ошибок.
- **Risks:** В безголовом окружении без GLES кастомных шейдеров активируется единый 9-slice fallback через `UiShapeCache` (без сдвига `y+5/+12`, полосатость отсутствует за счёт непрерывного 1 px текстурного градиента).

### RUSTYOS-076 — Arch Linux PKGBUILD, актуализация `install.sh` и непривилегированный сервис + индикатор обновлений (Этап A)

- **Status:** Done (`RUSTYOS-076 Вариант 1 (Этап A)` — утверждённый объём выполнен)
- **Goal:** Подготовить штатную упаковку RustyOS для Arch Linux (`makepkg`/`pacman`), актуализировать `scripts/install.sh`, добавить непривилегированную фоновую проверку обновлений Arch/RustyOS и интегрировать индикатор доступных обновлений в `TopBar` + `Liquid Glass` поповер в эстетике `macOS 27 Golden Gate`.
- **Dependencies:** `RUSTYOS-014`, `RUSTYOS-061`, `RUSTYOS-069`, `ADR-0036`, `D-044`.
- **Scope:**
  - `packaging/arch/PKGBUILD`: пакет `rustyos-git` (`provides=('rustyos')`, `conflicts=('rustyos')`, зависимости Wayland/Smithay и `pacman-contrib`) со сборкой `--release --locked --workspace` и установкой `rusty-compositor`, `rusty-shell`, `rusty-files`, `rustyos-session`, `rusty-pipewire-stream`, `rustyos.desktop`, порталов XDG и документации в `/usr`.
  - `scripts/install.sh`: добавлена установка `rusty-files`, непривилегированная сборка через `sudo -u "$SUDO_USER"` при запуске с `sudo`, флаги `--uninstall-user` / `--uninstall-system` и подсказки по `pacman-contrib` / `PKGBUILD`.
  - `crates/rusty-services/src/updates.rs` (`lib.rs`): `UpdateCheckerHandle`, `UpdateCheckState`, `PackageUpdateItem`, `UpdateStatusSnapshot`, `parse_checkupdates_output` (фоновый опрос `/usr/bin/checkupdates --nocolor` без root и без блокировки compositor loop, санитизация ANSI/CSI, классификация `rustyos*` и критичных для перезапуска системных пакетов).
  - `crates/rusty-compositor/src/`: секция `[updates]` в `UserConfig` (`enabled`, `check_interval_minutes` `15..=10080`, `show_in_menu_bar`), значок `UiGlyphKind::Updates { available }` с `#0A84FF` угловой точкой в `surfaces/{mod,glyphs}.rs`, опциональный слот `updates_x` в `shell_render/top_bar.rs` и пункт `Check for Updates...` в `MENU_SYSTEM`, `Liquid Glass` поповер `ShellOverlayView::Updates` (`shell_render/updates_popover.rs`, список приоритетных обновлений, `Check Now`, `Open Updater...` / `Software Update...`), переключатели `Updates Indicator` (`SettingsTab::MenuBar`) и `Auto Update Checks` (`SettingsTab::Security`) в `modals.rs`, интеграция в `state/` (`system_bridges.rs`, `shell_actions.rs`), `input/` и `frame_composer.rs`.
  - Документация: `ADR-0036` (`docs/adr/0036-arch-packaging-and-updates.md`), `D-044` (`DECISIONS.md`), `DESIGN.md §12.1`, `ARCHITECTURE.md`. (Этап B с отдельным GUI-приложением `apps/rusty-updater` и привилегированным установщиком вынесен в отдельную следующую задачу `RUSTYOS-077`.)
- **Acceptance Criteria:**
  - `PKGBUILD` и `scripts/install.sh` проходят `bash -n`, включают `rusty-files`, `pacman-contrib`, XDG portals и `/usr/share/wayland-sessions/rustyos.desktop` для SDDM;
  - `UpdateCheckerHandle` не блокирует compositor loop, безопасно очищает CSI/управляющие символы и не запускает привилегированные `pacman`/`git fetch` процессы;
  - Значок обновления в `TopBar` автоматически появляется при наличии обновлений (`menubar_show_updates && total_count > 0`) или открытом поповере и открывает Golden Gate `Liquid Glass` поповер с корректным остатком (`+N more packages`), кнопкой `Check Now`, запуском `rusty-updater` (либо информативным уведомлением до Этапа B) и сохранением `[updates]` в `config.toml`.
- **Tests:**
  - `bash -n packaging/arch/PKGBUILD scripts/install.sh` (`exit 0`)
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins`
  - `cargo test --workspace -- --test-threads=1` (`227 passed; 0 failed`: `rusty_animation` 4, `rusty_compositor` 155, `rusty_files` 13, `rusty_ipc` 3, `rusty_services` 40, `rusty_shell` 5, `rusty_wm` 7)
  - `./target/debug/rusty-compositor --drm-probe`
  - Живые `--winit --max-frames 35` smoke-прогоны (`Wayland` и `X11`: `late=0 missed=0`, без EGL/шейдерных ошибок) и визуальная проверка `Dark` & `Light` (`qa-076-{dark,light}-system-menu.png`, `qa-076-{dark,light}-updates-popover.png`, `qa-076-{dark,light}-settings-menubar.png`, `qa-076-{dark,light}-settings-security.png`).
- **Risks:** Установка самого пакета через `makepkg -si` и системные действия в `/usr` преднамеренно не выполнялись на хосте до явного решения пользователя; Stage B (`apps/rusty-updater` с интерактивным прогрессом `pacman` через Polkit) остаётся отдельной задачей.

### RUSTYOS-077 — Standalone Wayland апдейтер `rusty-updater` и привилегированный helper через Polkit (Этап B)

- **Status:** Done (`RUSTYOS-077 Вариант 1 (Этап B)` — утверждённый объём выполнен)
- **Goal:** Реализовать самостоятельное Wayland CSD-приложение `apps/rusty-updater` в эстетике `macOS 27 Golden Gate` (`Light & Dark`), потоковый раннер обновлений в `crates/rusty-services`, узкий root-helper `rustyos-update-helper` с политикой Polkit (`org.rustyos.updater.policy`), интеграцию с `PKGBUILD`/`scripts/install.sh` и оболочкой композитора без модификации `/usr` хоста.
- **Dependencies:** `RUSTYOS-014`, `RUSTYOS-032`, `RUSTYOS-069`, `RUSTYOS-076`, `ADR-0037`, `D-045`.
- **Scope:**
  - `apps/rusty-updater/` (`Cargo.toml`, `main.rs`, `state.rs`, `layout.rs`, `render.rs`, `wayland.rs`): `xdg_toplevel` CSD-клиент (`org.rustyos.updater`, `title="Software Update"`) с фильтрами (`All`, `RustyOS`, `Restart`), карточкой статуса с эмблемой и кнопкой `Update Now` / `Check Again` / `Cancel`, полосой прогресса транзакции, просмотром live-лога helper'а (`View Log` / `Packages`), кнопкой локальной сборки RustyOS (`makepkg --noconfirm` непривилегированно + `install-local`), а также безопасным `--dry-run` режимом для визуальной/CLI-проверки.
  - `crates/rusty-services/src/updates.rs` & `bin/rustyos_update_helper.rs`: полный список `check_all_pending_updates()`, `read_system_release_summary()`, неблокирующий `UpdateTransactionHandle` (парсинг прогресса `pacman` и санитизация управляющих последовательностей, локальная сборка `rustyos-git` непривилегированно с последующей установкой через `pkexec /usr/lib/rustyos/rustyos-update-helper`), узкий валидируемый CLI-интерфейс helper'а (`sysupgrade` / `install-local <abs-path-in-trusted-dir>.pkg.tar.{zst,xz,gz}`) без `sh -c` и без root-сборки `makepkg`.
  - Упаковка и права (`packaging/polkit/org.rustyos.updater.policy`, `packaging/applications/org.rustyos.updater.desktop`, `packaging/arch/PKGBUILD`, `scripts/install.sh`): установка бинарника `rusty-updater`, helper'а `/usr/lib/rustyos/rustyos-update-helper`, Polkit policy (`auth_admin_keep`) и `.desktop` файла с чистым `--uninstall-system`.
  - Интеграция с композитором (`crates/rusty-compositor/src/`: `icons.rs`, `launcher.rs`, `shell_render/{dock.rs,top_bar.rs,top_bar_menus.rs}`, `state/shell_actions.rs`): `BuiltinAppIcon::Updater`, элемент лаунчера `UPDATER` (`"Software Update"` / `org.rustyos.updater`), привязка иконки Dock/уведомлений/TopBar (`trigger_app_bounce("updater", false)` и запуск по кнопке `Open Updater...`).
  - Документация: `ADR-0037` (`docs/adr/0037-rusty-updater-and-privileged-upgrade.md`), `D-045` (`DECISIONS.md`), `DESIGN.md §12.3`, `ARCHITECTURE.md` (§2, §3).
- **Acceptance Criteria:**
  - `rusty-updater` запускается на Wayland внутри `rusty-compositor` (в том числе через `TopBar -> Check for Updates... -> Open Updater...`), корректно отображает `Light` и `Dark` палитры `macOS 27 Golden Gate` без legacy purple-токенов, и корректно проводит транзакцию в безопасном `--dry-run` режиме;
  - `rustyos-update-helper` принимает только фиксированные команды `sysupgrade` и `install-local <path>` с жёсткой валидацией пути (запрет `..`, неабсолютных путей и неподходящих суффиксов пакетов) и никогда не запускает `makepkg` от root;
  - `PKGBUILD` и `scripts/install.sh` проходят `bash -n`, включают `rusty-updater`, `rustyos-update-helper`, Polkit policy и `.desktop`, а `--locked` сборка workspace с обновлённым `Cargo.lock` проходит без ошибок.
- **Tests:**
  - `bash -n packaging/arch/PKGBUILD scripts/install.sh` (`exit 0`)
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins --locked`
  - `cargo test --workspace --locked -- --test-threads=1` (`233 passed; 0 failed`: `rusty_animation` 4, `rusty_compositor` 155, `rusty_files` 13, `rusty_ipc` 3, `rusty_services` 42, `rusty_shell` 5, `rusty_updater` 4, `rusty_wm` 7)
  - `./target/debug/rusty-compositor --drm-probe`
  - Живые `--winit --max-frames 35` smoke-прогоны (`Wayland` и `X11`: `late=0 missed=0`, без EGL/шейдерных ошибок) и визуальная QA в `Dark` & `Light` (`qa-077-dark-updater.png`, `qa-077-dark-dryrun-complete.png`, `qa-077-dark-logs.png`, `qa-077-dark-topbar-launch.png`, `qa-077-light-updater.png`, `qa-077-light-dryrun-complete.png`).
- **Risks:** Реальный системный вызов `pkexec /usr/lib/rustyos/rustyos-update-helper` и установка файлов в `/usr` / `/etc/polkit-1` намеренно не выполнялись на хосте (проверено через `--dry-run`, unit-тесты валидации/парсинга и запуск из соседнего каталога `exe_dir`); полноценная runtime-проверка `pkexec` требует установки пакета на хосте или в VM по решению архитектора.

### RUSTYOS-078 — Готовность к чистому Arch Linux: `--drm` activation env, `.desktop`, `rustyos-session --doctor` и проверка `makepkg`

- **Status:** Done (`RUSTYOS-078 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Закрыть практические пробелы развёртывания на минимальном чистом Arch Linux: добавить `org.rustyos.files.desktop`, уточнить `depends`/`optdepends` и воспроизводимость в `PKGBUILD`, реализовать безопасный экспорт `WAYLAND_DISPLAY`/`DISPLAY` в `dbus-update-activation-environment --systemd` строго в `--drm` режиме, добавить read-only диагностику `rustyos-session --doctor`, проверить изолированную сборку `.pkg.tar.zst` через `makepkg` и добавить `arch-packaging` job в CI.
- **Dependencies:** `RUSTYOS-014`, `RUSTYOS-076`, `RUSTYOS-077`, `ADR-0038`, `D-046`.
- **Scope:**
  - `packaging/applications/org.rustyos.files.desktop` (+ правка категории в `org.rustyos.updater.desktop`), `packaging/arch/PKGBUILD` (`depends` для чистого Arch: `dbus`, `fontconfig`, `adwaita-icon-theme`, расширенный `optdepends`, `--remap-path-prefix` в `RUSTFLAGS`), `scripts/install.sh` (установка и `--uninstall-*` для `org.rustyos.files.desktop`, подсказка `--doctor`).
  - `crates/rusty-compositor/src/`: флаг `RustyCompositorState::sync_systemd_activation_env` (`false` в тестах и `--winit`, `true` только в `drm_backend.rs`), чистый конструктор аргументов `build_session_activation_env_args`, неблокирующий фоновый `sync_drm_activation_environment` (в том числе при готовности `XWayland` в `shell_actions::set_xwayland_display`).
  - `bin/rustyos-session`: поддержка `--help` и read-only проверки `--doctor` (резолв бинарников с приоритетом свежего workspace `target/`, проверка утилит `Xwayland`, `dbus-update-activation-environment`, `pkexec`, `pacman`, `checkupdates`, `.desktop`, Polkit policy, порталов, системных/пользовательских служб, `/dev/dri/*` и `--drm-probe`).
  - `.github/workflows/ci.yml`: добавлен job `arch-packaging` (`container: archlinux:base-devel`) с валидацией скриптов, `.desktop`, Polkit XML и изолированным запуском `makepkg`.
  - Документация: `ADR-0038` (`docs/adr/0038-arch-session-activation-and-doctor.md`), `D-046` (`DECISIONS.md`), раздел 5 в `docs/GETTING_STARTED.md`.
- **Acceptance Criteria:**
  - `--winit` и unit-тесты не экспортируют переменные в хостовый `dbus`/`systemd`; `--drm` публикует `WAYLAND_DISPLAY` и `DISPLAY` (после старта `XWayland`) асинхронно без блокировки KMS event loop;
  - `rustyos-session --doctor` выполняется read-only, корректно находит бинарники, проверяет `--drm-probe` и завершается с `exit 0` при готовности ядра;
  - Изолированная сборка `makepkg` без установки на хост успешно создаёт `rustyos-git-*.pkg.tar.zst`, содержащий все 6 исполняемых/сервисных бинарников, `/usr/lib/rustyos/rustyos-update-helper`, оба `.desktop`, `.policy`, `.portal` и `wayland-sessions/rustyos.desktop` (`Exec=/usr/bin/rustyos-session`).
- **Tests:**
  - `python3` YAML-проверка `.github/workflows/ci.yml`, `sh -n bin/rustyos-session`, `bash -n packaging/arch/PKGBUILD scripts/install.sh`, `desktop-file-validate packaging/applications/org.rustyos.{files,updater}.desktop`, `xmllint --noout packaging/polkit/org.rustyos.updater.policy`
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins --locked`
  - `cargo test --workspace --locked -- --test-threads=1` (`234 passed; 0 failed`: `rusty_animation` 4, `rusty_compositor` 156, `rusty_files` 13, `rusty_ipc` 3, `rusty_services` 42, `rusty_shell` 5, `rusty_updater` 4, `rusty_wm` 7)
  - `./bin/rustyos-session --doctor` (`22 ok, 6 warn, 0 fail`, `exit 0`)
  - Изолированный `makepkg -f --holdver --noconfirm --nocheck` -> `rustyos-git-0.1.0.r77.g4702baa-1-x86_64.pkg.tar.zst` (`10M`) + `pacman -Qlp` + `tar -xOf` (`Exec=/usr/bin/rustyos-session`)
  - `./target/debug/rusty-compositor --winit --max-frames 35` (`late=0 missed=0`, без EGL/шейдерных ошибок) и `./target/debug/rusty-compositor --drm-probe`.
- **Risks:** Реальный вход в `--drm` из TTY и интерактивный запрос пароля `pkexec` в установленном системном дереве `/usr` не запускались на рабочей сессии хоста и вынесены в пошаговую проверку пользователя (`docs/GETTING_STARTED.md §5`) перед занесением статуса `working` в `docs/HARDWARE_MATRIX.md`.

### RUSTYOS-079 — Непрерывная доставка: публикация pacman-репозитория в CI, автообновление и сессионный статус

- **Status:** Done (`RUSTYOS-079 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Реализовать автоматическую сборку и публикацию штатного Arch Linux pacman-репозитория (`rustyos-git`, `rustyos.db.tar.zst`, `rustyos.files.tar.zst`) в GitHub Actions при каждом `git push` в `main` (строго после `check` и `arch-packaging`), поддержать опциональную GPG-подпись, отображать версию/перезапуск сессии в `rusty-services` и `rusty-updater`, и документировать подключение `[rustyos]` в `docs/GETTING_STARTED.md` без модификации системы хоста агентом.
- **Dependencies:** `RUSTYOS-014`, `RUSTYOS-076`, `RUSTYOS-077`, `RUSTYOS-078`, `ADR-0039`, `D-047`.
- **Scope:**
  - `.github/workflows/ci.yml`: добавлен job `publish-arch-repo` (`needs: [check, arch-packaging]`, `if: push main`, `container: archlinux:base-devel`), собирающий `rustyos-git` с динамическим `pkgver()`, формирующий базу репозитория `repo-add rustyos.db.tar.zst *.pkg.tar.zst`, поддерживающий откреплённую GPG-подпись (`GPG_SIGNING_KEY` из secrets) с graceful unsigned fallback, публикующий артефакты `actions/upload-artifact@v4` и выгружающий репозиторий в ветку `gh-pages` для раздачи через GitHub Pages (`https://ragezorrr.github.io/RustyOS/repo/$arch`), а также создающий веб-страницу `index.html`.
  - `crates/rusty-services/src/updates.rs`: в `SystemReleaseSummary` добавлен флаг `pacman_repo_configured`, методы `parse_pacman_conf_has_repo` и `is_pacman_repo_configured` для неблокирующей проверки `/etc/pacman.conf` без внешних команд, а также unit-тесты.
  - `apps/rusty-updater/src/render.rs`: в `hero_subtitle` добавлено отображение обновления версии `RustyOS <old> -> <new>`, в подвале списка — подсказка о необходимости перезапуска сессии (`tx_restart_recommended`) либо напоминание о настройке `[rustyos]`, если репозиторий ещё не подключён.
  - Документация: `ADR-0039` (`docs/adr/0039-arch-ci-pacman-repository.md`), `D-047` (`DECISIONS.md`), раздел 6 в `docs/GETTING_STARTED.md` с пошаговой инструкцией подключения репозитория и опциональной GPG-подписью.
- **Acceptance Criteria:**
  - `ci.yml` содержит валидный YAML, где `publish-arch-repo` запускается строго после успешных `check` и `arch-packaging` только при пуше в `main`;
  - `updates.rs` корректно детектирует секцию `[rustyos]` в конфиге pacman без ложных срабатываний на комментарии;
  - `rusty-updater` отображает статус готовности репозитория и рекомендацию выхода из сессии после применения обновления;
  - Агент не трогает `/etc/pacman.conf` и `/usr` хоста; все системные настройки описаны для пользователя.
- **Tests:**
  - `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` (`exit 0`)
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins --locked`
  - `cargo test --workspace --locked -- --test-threads=1` (`235 passed; 0 failed`: `rusty_animation` 4, `rusty_compositor` 157, `rusty_files` 13, `rusty_ipc` 3, `rusty_services` 43, `rusty_shell` 5, `rusty_updater` 4, `rusty_wm` 7)
  - Живой `--winit --max-frames 35` smoke-прогон (`Wayland` и `X11`: `late=0 missed=0`, без EGL/шейдерных ошибок).
- **Risks:** Непосредственная выгрузка в ветку `gh-pages` происходит на стороне GitHub Actions при наличии прав у токена (`contents: write`); включение раздачи в веб-интерфейсе GitHub Pages (`Settings -> Pages`) выполняется владельцем репозитория.

### RUSTYOS-080 — Исправление UX композитора: питание сессии, Chrome PWA, тени окон, папки рабочего стола и автозапуск

- **Status:** Done (`RUSTYOS-080 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Устранить 5 выявленных на живой системе замечаний UX: очистить автозапуск `rusty-updater`, добавить системные действия питания (`Sleep`, `Restart...`, `Shut Down...`, `Log Out...`) в меню Apple/System Logo по спецификации macOS 27 Golden Gate, обеспечить строгий приоритет desktop ID над Chrome PWA ярлыками при поиске иконок, уменьшить радиус/ореол тени окон с возможностью отключения через `[ui].window_shadow` в `config.toml`, и поддержать создание папок в локализованном каталоге рабочего стола пользователя (`user-dirs.dirs`, `~/Рабочий стол`).
- **Dependencies:** `RUSTYOS-033`, `RUSTYOS-048`, `RUSTYOS-054`, `RUSTYOS-073`, `RUSTYOS-075`, `RUSTYOS-079`.
- **Scope:**
  - `~/.config/rustyos/config.toml`: очищен массив `commands = []` в секции `[autostart]`.
  - `crates/rusty-compositor/src/config.rs`: добавлен enum `WindowShadowMode` (`Subtle`, `Normal`, `None`), поле `window_shadow` в `UserConfig`, парсинг строковых и булевых значений из `[ui]` и сериализация.
  - `crates/rusty-compositor/src/surfaces/window_shadow.rs`, `surfaces/mod.rs`, `shell_render/ssd.rs`, `frame_composer.rs`: уменьшен радиус и плотность тени окна до компактного Golden Gate профиля (`amb_spread: 16.0`, `drop_y: 5.0`, `amb_alpha: 0.09` для активного; `amb_spread: 10.0`, `drop_y: 3.0` для неактивного); при `WindowShadowMode::None` тени полностью отключаются.
  - `crates/rusty-compositor/src/shell_render/top_bar.rs` и `state/shell_actions.rs`: в `TopBarMenuAction` и системное меню добавлены `Sleep` (`Moon`), `Restart...` (`Refresh`), `Shut Down...` (`PowerSymbol`), разделитель и `Log Out...` (`Close`, `Super+Shift+Q`), привязанные к `self.telemetry.dispatch_session(...)` и `self.loop_signal.stop()`.
  - `crates/rusty-compositor/src/launcher.rs`: реализован 3-фазный поиск иконок (фаза 1 — точное совпадение ID desktop-файла, фаза 2 — поиск бинарника с исключением веб-приложений PWA `--app-id=`, фаза 3 — fallback); добавлены `google-chrome` и псевдонимы в `canonical_preferred_bins`.
  - `crates/rusty-compositor/src/state/context_menu_actions.rs`: функции `resolve_desktop_dir()` и `resolve_downloads_dir()` интегрированы с `UserFileLocations::detect()` для чтения `~/.config/user-dirs.dirs` и локализованных путей (`Рабочий стол`, `Загрузки`); генерация уникальных имён папок адаптирована для русскоязычной локали («Новая папка»).
- **Acceptance Criteria:**
  - `rusty-updater` не запускается автоматически при старте композитора;
  - Системное меню верхнего бара отображает пункты `Sleep`, `Restart...`, `Shut Down...`, `Log Out...`;
  - Закреплённый и запущенный ярлык `google-chrome` разрешается в официальную иконку Chrome вместо PWA Google Chat;
  - Окна отрисовываются с аккуратной компактной тенью, либо без неё при `window_shadow = "none"` / `false`;
  - Пункт «Новая папка» на рабочем столе создаёт папку в фактическом каталоге пользователя (`~/Рабочий стол`).
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins --locked`
  - `cargo test --workspace --locked -- --test-threads=1` (`236 passed; 0 failed`: `rusty_animation` 4, `rusty_compositor` 158, `rusty_files` 13, `rusty_ipc` 3, `rusty_services` 43, `rusty_shell` 5, `rusty_updater` 4, `rusty_wm` 7)
  - Живой `--winit --max-frames 35` smoke-прогон (`autostart_count=0`, `rendered_frames=35`, без EGL/шейдерных ошибок).
- **Risks:** Нет.

### RUSTYOS-081 — Нативные иконки и сетка рабочего стола (Desktop Canvas)

- **Status:** Done (`RUSTYOS-081 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Реализовать отображение файлов и папок из `~/Рабочий стол` на холсте обоев композитора в стиле macOS Golden Gate: вертикальная сетка в правом верхнем углу, squircle-иконки 48x48, контрастные двухслойные подписи с тенью, выделение кликом, запуск двойным кликом (папки в `rusty-files`, приложения `.desktop` через `launcher`, файлы через `xdg-open`), контекстное меню элемента («Open», «Move to Trash») и автосинхронизация при изменениях.
- **Dependencies:** `RUSTYOS-033`, `RUSTYOS-048`, `RUSTYOS-050`, `RUSTYOS-064`, `RUSTYOS-080`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/desktop_canvas.rs`: модуль нативной отрисовки холста рабочего стола с параметрами macOS 27 Golden Gate (`DESKTOP_ITEM_W: 88`, `DESKTOP_ITEM_H: 92`, `DESKTOP_ICON_SIZE: 48`, отступы `margin_right: 24`, `margin_top: 46`, вертикальный поток с переносом колонок влево); расчет геометрии `compute_desktop_item_rect`, хит-тестирование `hit_test_desktop_item`, плашка выделения `push_rounded_glass_box`, 48x48 squircle-иконки и двухслойная тень шрифта (`[0, 0, 0, 220]` и `[255, 255, 255, 255]`); unit-тесты сетки и хит-тестирования.
  - `crates/rusty-compositor/src/shell_render/mod.rs` & `context_menu.rs`: регистрация модуля `desktop_canvas` и добавление действий контекстного меню `ContextMenuAction::DesktopItemOpen` и `ContextMenuAction::DesktopItemTrash`.
  - `crates/rusty-compositor/src/state/mod.rs`: добавлены поля `desktop_items`, `selected_desktop_item`, `last_desktop_click`, `desktop_last_scan` и метод `resync_desktop_items(&mut self)` для опроса директории рабочего стола.
  - `crates/rusty-compositor/src/state/context_menu_actions.rs`: функции `scan_desktop_canvas_items(&Path)` (парсинг .desktop файлов, сортировка папок и файлов), `open_desktop_canvas_item(&item)` (открытие в `rusty-files` / запуск exec / `xdg-open`), `open_desktop_item_context_menu(point, &item)` и обновление `create_desktop_folder_action` с немедленным вызовом `resync_desktop_items` и фокусом на созданной папке.
  - `crates/rusty-compositor/src/frame_composer.rs`: вызов `render_desktop_canvas_elements` в `build_backdrop_render_elements` перед добавлением обоев на основном мониторе (иконки отображаются поверх обоев и под всеми плавающими окнами и панелями).
  - `crates/rusty-compositor/src/input/pointer.rs`: хит-тестирование иконок рабочего стола при клике на свободное пространство (одиночный клик — выделение, двойной клик <450ms — открытие, правый клик — выбор и контекстное меню элемента; клик в пустое место — сброс выделения и общее меню рабочего стола).
  - `crates/rusty-compositor/src/state/system_bridges.rs`: периодическая автосинхронизация состояния рабочего стола каждые 3 секунды в фоновом цикле системных мостов.
- **Acceptance Criteria:**
  - Файлы и папки из каталога рабочего стола (`~/Рабочий стол` / `~/Desktop`) отображаются прямо на обоях рабочего стола;
  - Сетка иконок выравнивается в правом верхнем углу (macOS Golden Gate);
  - Создание «Новая папка» сразу же отображает новую папку на рабочем столе с автоматическим выделением;
  - Двойной клик по папке открывает её в файловом менеджере `rusty-files`;
  - Правый клик по иконке открывает контекстное меню для данного объекта («Open», «Move to Trash»);
  - Клики по пустому рабочему столу снимают выделение и вызывают общее контекстное меню стола.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins --locked`
  - `cargo test --workspace --locked -- --test-threads=1` (`223 passed; 0 failed`)
  - Интеграционный `--winit --max-frames 35` smoke-прогон (`rendered_frames=35, late=0 missed=0`, чистый выход).
- **Risks:** Нет.

### RUSTYOS-082 — Интерактивный рабочий стол: Drag-and-Drop, Rubber-band и сохранение расположения

- **Status:** Done (`RUSTYOS-082 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Реализовать интерактивное управление иконками на холсте рабочего стола в стиле macOS Golden Gate: перетаскивание иконок мышью с визуальным следованием за курсором и привязкой к ячейкам сетки при отпускании (со сменой слотов при коллизиях), сброс файлов в папки рабочего стола (`fs::rename`), рамка множественного выделения Rubber-band на свободном пространстве холста с акцентной подсветкой macOS (`[0, 122, 255, 30]`), сохранение расположения иконок в `~/.config/rustyos/desktop_grid.toml` и типографика подписей в `Inter Bold/SemiBold` с рассеянной тенью ambient shadow.
- **Dependencies:** `RUSTYOS-081`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/desktop_canvas.rs`: структуры `DesktopRubberBand` (прямоугольник выделения с нормализацией координат) и `DesktopDragState` (отслеживание перетаскивания иконки, порог активации 5px, подсвечивание папки назначения `target_hover_dir`); вычисление координат ячейки `compute_desktop_cell_rect` и обратная проекция курсора в ячейку `cell_at_point`; сохранение и загрузка слотов `desktop_grid.toml` (`load_desktop_grid_positions`, `save_desktop_grid_positions`); отрисовка плавающей иконки при перетаскивании, подсветка целевой папки, отрисовка рамки Rubber-band, чёткие подписи `Inter Bold/SemiBold` с 4-точечным мягким ореолом ambient shadow.
  - `crates/rusty-compositor/src/state/mod.rs`: добавлены поля `selected_desktop_items: HashSet<PathBuf>`, `desktop_drag: Option<DesktopDragState>`, `desktop_rubber_band: Option<DesktopRubberBand>`, хелперы `selected_desktop_item()` и `save_desktop_grid_layout()`.
  - `crates/rusty-compositor/src/state/context_menu_actions.rs`: обновлена функция `scan_desktop_canvas_items` для автоматического назначения слотов с приоритетом сохранённых координат из `desktop_grid.toml` и непрерывного заполнения свободных ячеек для новых файлов; сохранение сетки при создании папки.
  - `crates/rusty-compositor/src/frame_composer.rs`: передача активных состояний `selected_desktop_items`, `desktop_drag` и `desktop_rubber_band` в `render_desktop_canvas_elements`.
  - `crates/rusty-compositor/src/input/pointer.rs`: в `apply_pointer_position` добавлено обновление координат перетаскивания (активация при смещении >=5px, поиск целевой папки) и динамический пересчет пересечений прямоугольника Rubber-band с ячейками иконок; в `handle_pointer_button` обработано отпускание (сброс в папку либо привязка к ячейке с обменом слотов и записью в TOML, завершение Rubber-band) и нажатие (старт drag при клике на иконку, старт Rubber-band при клике на обои).
  - `crates/rusty-compositor/src/state/tests.rs`: unit-тесты для геометрии сетки, функции `cell_at_point`, персистентности `desktop_grid.toml`, пересечений `DesktopRubberBand` и переноса файлов в папку при drop.
- **Acceptance Criteria:**
  - Иконки на рабочем столе можно свободно перетаскивать мышью, при отпускании они привязываются к ближайшей ячейке сетки;
  - Расположение иконок сохраняется между перезапусками в `~/.config/rustyos/desktop_grid.toml`;
  - Перетаскивание файла на папку подсвечивает папку и при отпускании перемещает файл внутрь неё;
  - Зажатие и протяжка ЛКМ на свободном месте рабочего стола рисует рамку Rubber-band и выделяет все охваченные иконки;
  - Подписи иконок отображаются шрифтом Inter SemiBold с мягким рассеянным ореолом, исключающим резкие артефакты дублирования текста.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins --locked`
  - `cargo test --workspace --locked -- --test-threads=1` (`225 passed; 0 failed`)
- **Risks:** Нет.

### RUSTYOS-083 — Полный жизненный цикл файлов рабочего стола: инлайн-переименование, удаление в корзину и Drag-and-Drop в Dock

- **Status:** Done (`RUSTYOS-083 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Реализовать полноценный жизненный цикл файлов и папок рабочего стола в парадигме macOS Golden Gate: инлайн-переименование имени по клавише `Enter` или клику с фокусной плашкой и валидацией `fs::rename`, удаление выделенных файлов в Корзину по клавишам `Delete` или `Super+Backspace` с OSD-уведомлением, интерактивный Drag-and-Drop иконок рабочего стола в иконку Корзины Dock с динамической сменой иконки на `TrashFull` (скомканная бумага) и сохранением контекстной очистки Корзины.
- **Dependencies:** `RUSTYOS-082`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/desktop_canvas.rs`: структура `DesktopInlineRename` (целевой путь, текстовый буфер, позиция курсора); отрисовка фокусного поля ввода вокруг редактируемой подписи (`[20, 24, 34, 230]` с акцентной каймой `#007AFF`); скрытие рамки выделения при активном переименовании; поле `drop_to_trash` в `DesktopDragState` и подавление подсветки ячеек при наведении на Корзину.
  - `crates/rusty-compositor/src/icons.rs`: поддержка динамической иконки `BuiltinAppIcon::TrashFull` (визуализация скомканной бумаги поверх металлической корзины) и выбор между пустой и заполненной корзиной в зависимости от содержимого `~/.local/share/Trash`.
  - `crates/rusty-compositor/src/shell_render/dock.rs`: параметр `trash_has_items: bool` в `append_utility_dock_tiles` для автоматического переключения иконки корзины.
  - `crates/rusty-compositor/src/state/mod.rs` и `state/shell_actions.rs`: поля `desktop_inline_rename: Option<DesktopInlineRename>` и `trash_has_items: bool` в `RustyCompositorState`, метод `update_trash_full_status()`.
  - `crates/rusty-compositor/src/state/context_menu_actions.rs`: методы `commit_desktop_inline_rename()` (валидация спецсимволов `/`, `\0`, проверка коллизий, переименование `fs::rename` и перенос позиции сетки) и `move_selected_desktop_items_to_trash()` (перемещение всех выбранных объектов в `~/.local/share/Trash` с OSD «Moved to Trash»).
  - `crates/rusty-compositor/src/input/keyboard.rs`: перехват клавиш при активном переименовании (`Enter` — коммит, `Escape` — отмена, стрелки `Left`/`Right`, `Backspace`, `Delete`, ввод символов) и горячие клавиши рабочего стола (`Enter` — начало переименования, `Delete` / `Super+Backspace` — перемещение в корзину).
  - `crates/rusty-compositor/src/input/pointer.rs`: детекция наведения перетаскиваемой иконки на слот Корзины в Dock (`drop_to_trash = true`), при отпускании — вызов `move_to_trash`, обновление состояния корзины и удаление из сетки.
  - `crates/rusty-compositor/src/state/tests.rs`: unit-тест `desktop_inline_rename_and_trash_workflow` для полного цикла переименования, горячих клавиш удаления и смены иконки корзины.
- **Acceptance Criteria:**
  - При выделенной иконке нажатие `Enter` активирует инлайн-редактирование имени в фокусной плашке;
  - Повторный `Enter` или клик вне поля сохраняет новое имя с вызовом `fs::rename` и обновлением сетки; `Escape` отменяет ввод;
  - Нажатие `Delete` или `Super+Backspace` перемещает выделенные элементы в `~/.local/share/Trash` с показом OSD;
  - Перетаскивание файла рабочего стола на иконку Корзины в Dock и отпускание перемещает файл в Корзину;
  - Иконка Корзины в Dock динамически переключается на заполненную (`TrashFull`) при наличии файлов.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins --locked`
  - `cargo test --workspace --locked -- --test-threads=1` (`225 passed; 0 failed`)
- **Risks:** Нет.

### RUSTYOS-084 — Нативная Wayland-сессия, системный запуск из Display Manager и окружение клиентов

- **Status:** Done (`RUSTYOS-084 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Обеспечить полноценный запуск RustyOS как самостоятельной десктоп-сессии через системные дисплейные менеджеры (SDDM, GDM, LightDM, greetd): экспорт эталонных переменных окружения клиентских тулкитов (Qt, GTK, Electron, Firefox, SDL, Java), интеграция с `systemd --user` целями `graphical-session.target` / `rustyos-session.target`, гарантированный graceful shutdown сессии с остановкой служб при Log Out, расширение pre-flight самодиагностики `rustyos-session --doctor`, установка юнитов в `PKGBUILD` и `scripts/install.sh`, оформление ADR-0040 и записи D-048.
- **Dependencies:** `RUSTYOS-078`, `ADR-0040`, `D-048`.
- **Scope:**
  - `bin/rustyos-session`: установлены эталонные переменные по умолчанию с сохранением пользовательских переопределений (`QT_QPA_PLATFORM="wayland;xcb"`, `QT_WAYLAND_DISABLE_WINDOWDECORATION="1"`, `GDK_BACKEND="wayland,x11,*"`, `MOZ_ENABLE_WAYLAND="1"`, `ELECTRON_OZONE_PLATFORM_HINT="auto"`, `SDL_VIDEODRIVER="wayland,x11"`, `CLUTTER_BACKEND="wayland"`, `_JAVA_AWT_WM_NONREPARENTING="1"`);
  - `bin/rustyos-session`: в автономном `--drm`/`--kms` режиме добавлен импорт переменных в `systemctl --user import-environment` и старт `rustyos-session.target`; настроен перехват сигналов завершения (`trap cleanup_session EXIT INT TERM HUP`) с вызовом `systemctl --user stop rustyos-session.target graphical-session.target` и запуском `rustyos-session-shutdown.target`;
  - `bin/rustyos-session`: расширена функция `--doctor` проверками доступности `rustyos-session.target`, `rustyos-session-shutdown.target` и ключевых переменных тулкитов;
  - `packaging/systemd/user/rustyos-session.target`: Freedesktop/systemd юнит привязки к `graphical-session.target`;
  - `packaging/systemd/user/rustyos-session-shutdown.target`: юнит бесконфликтной остановки сессионных служб при выходе;
  - `packaging/arch/PKGBUILD`: добавлены каталоги и установка юнитов в `/usr/lib/systemd/user/`;
  - `scripts/install.sh`: добавлена установка и очистка при удалении системных и пользовательских systemd-юнитов;
  - `docs/adr/0040-standalone-wayland-session-and-systemd.md` и запись `D-048` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Все основные приложения (Chromium/Electron, Firefox, Qt, GTK) стартуют нативно через Wayland с четким HiDPI без размытия;
  - `bin/rustyos-session --doctor` выводит статус 33 ok, 0 fail;
  - Завершение сессии чисто останавливает `graphical-session.target` и связанные сессионные демоны;
  - `sh -n bin/rustyos-session` и `bash -n scripts/install.sh` валидны;
  - Пакет Arch Linux собирается с включением systemd user targets.
- **Tests:**
  - `sh -n bin/rustyos-session`, `bash -n scripts/install.sh`;
  - `./bin/rustyos-session --doctor` (`33 ok, 2 warn, 0 fail`);
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace --locked -- --test-threads=1` (`225 passed; 0 failed`).
- **Risks:** Нет.

### RUSTYOS-086 — Унифицированное системное контекстное меню по стандарту macOS 27 и модульная декомпозиция подсистемы меню

- **Status:** Done (`RUSTYOS-086 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Реализовать унифицированное системное контекстное меню по правой кнопке мыши по стандарту macOS 27 Golden Gate для всех оконных поверхностей и приложений: вызов меню управления окном при клике на SSD-заголовок, рамку, по `Alt/Super+Right-Click` или через Wayland CSD `xdg_toplevel.show_window_menu`; тайлинг в левую/правую половину (`⌥←`/`⌥→`), Zoom (`⌥F`), перенос на мониторы (`⌥O`), назначение рабочих столов (`1..9`), сворачивание (`⌘M`), закрытие (`⌘W`) и принудительное завершение (`Alt` -> `Force Quit`); интеграция гибридного меню в `apps/rusty-files`; модульная декомпозиция разросшегося `context_menu_actions.rs` в подмодули `crates/rusty-compositor/src/context_menu/`; оформление ADR-0042 и записи D-050.
- **Dependencies:** `RUSTYOS-064`, `RUSTYOS-081`, `ADR-0042`, `D-050`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/context_menu.rs`: расширены перечисления `ContextMenuAction` (добавлены `WindowTileLeft`, `WindowTileRight`, `WindowToggleMaximize`, `WindowMoveToNextOutput`) и `ContextMenuTarget` (добавлен `Window(WindowId)`).
  - `crates/rusty-compositor/src/context_menu/`: новая модульная подсистема контекстных меню:
    - `window_menu.rs`: функции `build_window_context_entries` (полный набор оконных действий macOS 27) и `build_minimized_window_context_entries`;
    - `desktop_menu.rs`: сканирование канваса Рабочего стола, сетка иконок, создание уникальных папок, переименование и удаление в корзину;
    - `dock_menu.rs`: меню приложений Dock, стек и сортировка Загрузок, управление Корзиной и фоновые каналы очистки;
    - `actions.rs`: диспетчер действий `execute_context_menu_action` и `execute_dock_or_window_context_action`, управление процессами, переключение модификатора `Alt` (`toggle_quit_force_in_entries`);
    - `mod.rs`: центральный координатор контекстных меню, методы `RustyCompositorState` (`open_window_context_menu`, `open_desktop_context_menu`, `open_dock_context_menu`, обработка кликов, ховеров, клавиатурной навигации).
  - `crates/rusty-compositor/src/state/context_menu_actions.rs`: легковесный фасад обратной совместимости, реэкспортирующий `crate::context_menu::*`.
  - `crates/rusty-compositor/src/input/pointer.rs`: перехват правого клика (`button == 273`) на SSD-заголовках, кнопках и рамках с вызовом `open_window_context_menu`; поддержка `Alt+Right-Click` и `Super+Right-Click` на любой области окна для прямого открытия системного меню.
  - `crates/rusty-compositor/src/state/wayland_handlers/core.rs`: реализация обработчика `show_window_menu` в `XdgShellHandler` для вызова системного меню по запросу CSD-приложений.
  - `apps/rusty-files`: добавлены стандартные системные оконные команды (`ZoomWindow`, `MinimizeWindow`, `CloseWindow`) под разделителем в контекстном меню файлов, и трансляция правого клика по панели инструментов композитору через `win.show_window_menu`.
  - `docs/adr/0042-unified-system-context-menu-and-modularization.md` и запись `D-050` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Правый клик по заголовку или рамке любого окна открывает системное меню управления окном;
  - Меню содержит тайлинг (левая/правая половина), Zoom / Restore Size, Move to Desktop 1..9, Move to Next Display (при наличии >1 дисплея), Minimize и Close/Force Quit;
  - При удержании `Alt` пункт «Close» динамически сменяется на красный «Force Quit»;
  - CSD-приложения, вызывающие `xdg_toplevel.show_window_menu`, получают Liquid Glass меню композитора в заданной точке;
  - В `rusty-files` контекстное меню содержит системные действия под разделителем, а клик по тулбару вызывает оконное меню;
  - Монолитный файл `context_menu_actions.rs` декомпозирован на чистые подмодули.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace --locked` (все 226 тестов пройдены успешно).
- **Risks:** Нет.

### RUSTYOS-087 — 1:1 стандарт Liquid Glass Popover для клиентских xdg_popup поверхностей в rusty-files и декомпозиция рендера меню

- **Status:** Done (`RUSTYOS-087 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Привести контекстные меню и выпадающие списки в `apps/rusty-files` к полному 1:1 визуальному и геометрическому соответствию со стандартом Liquid Glass композитора macOS 27 Golden Gate (`DESIGN.md` §10, §12): скругление 16px (`MENU_POPOVER_RADIUS`), многослойная гауссова SDF-тень, зеркальный верхний ободок (`rgba(255, 255, 255, 80)` / `38`), инсетные плавающие пилюли ховера 8px (`HOVER_PILL_RADIUS`) с отступом 6px, правая колонка горячих клавиш (`⇧⌘N`, `⌘T`, `Space`, `⌘I`, `⏎`, `⌘C`, `⌘X`, `⌘V`, `⌘D`, `⌘⌫`, `^⌘F`, `⌘M`, `⌘W`, `⇧⌘.`), тонкие разделители секций с отступом 12px; декомпозировать разросшийся `render.rs`, вынеся логику меню в изолированный модуль `apps/rusty-files/src/render/popup.rs`; оформить ADR-0043 и запись D-051.
- **Dependencies:** `RUSTYOS-086`, `ADR-0043`, `D-051`.
- **Scope:**
  - `apps/rusty-files/src/render/popup.rs`: новый модуль рендеринга выпадающих меню и контекстных списков:
    - Константы `MENU_POPOVER_RADIUS = 16`, `HOVER_PILL_RADIUS = 8`, `HOVER_PILL_INSET_H = 6`, `HOVER_PILL_INSET_V = 1`, `HAIRLINE_DIVIDER_INSET = 12`;
    - Функция `shortcut_for_command` для маппинга стандартных клавиатурных комбинаций macOS 27;
    - Функция `is_danger_command` для цветовой подсветки деструктивных операций (`popover_danger`);
    - Функция `render_popup` с многостадийным SDF-шейдингом тени, полупрозрачной подложкой Liquid Glass, зеркальным верхним кантом, плавающими инсетными пилюлями ховера, монохромными и векторными глифами, цветовыми кружками тегов и правой колонкой шорткатов;
    - Функция `render_popup_surface_pixels` для отрисовки в буфер Wayland-поверхности `xdg_popup`;
    - Unit-тесты на геометрию, шорткаты и цвета.
  - `apps/rusty-files/src/render.rs`:
    - Добавлены токены `popover_bg`, `popover_border`, `popover_rim`, `popover_text_primary`, `popover_text_secondary`, `popover_text_muted`, `popover_danger`, `popover_separator` в структуру `Palette` и функцию `palette_for`;
    - Удалена устаревшая плоская реализация `render_popup` (разгрузка монолитного файла);
    - Подключен модуль `pub(crate) mod popup;` с реэкспортом `render_popup` и `render_popup_surface_pixels`.
  - `apps/rusty-files/src/layout.rs`:
    - Обновлены базовые ширины в `build_popup_entries`: контекстное и actions-меню расширены до 230px для комфортного размещения шорткатов без наложений.
  - `docs/adr/0043-client-liquid-glass-popup-standard.md` и запись `D-051` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Контекстные меню и выпадающие списки в `rusty-files` имеют радиус скругления 16px, мягкую многослойную тень и полупрозрачную подложку Liquid Glass с зеркальным верхним кантом;
  - Выделение пункта при ховере происходит через плавающую инсетную пилюлю со скруглением 8px и боковыми отступами 6px;
  - Пункты меню содержат правую колонку горячих клавиш;
  - Деструктивные действия подсвечиваются системным красным цветом `popover_danger`;
  - Логика отрисовки меню изолирована в `apps/rusty-files/src/render/popup.rs`;
  - Все тесты воркспейса проходят успешно, clippy и cargo fmt чисты.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace` (все 244 теста воркспейса пройдены успешно).
- **Risks:** Нет.

### RUSTYOS-088 — Модульная декомпозиция монолитных подсистем apps/rusty-files/src/state и crates/rusty-services/src/file_manager

- **Status:** Done (`RUSTYOS-088 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Ликвидировать архитектурный долг и code smell двух крупнейших монолитных файлов проекта (`crates/rusty-services/src/file_manager.rs` на 3,240 строк и `apps/rusty-files/src/state.rs` на 3,298 строк), разбив их на чистые, строго сфокусированные субмодули (<700 строк логики) с полным сохранением 100% публичных API, контрактов и всей тестовой базы (244 теста рабочего пространства). Оформить ADR-0044 и запись D-052.
- **Dependencies:** `RUSTYOS-087`, `ADR-0044`, `D-052`.
- **Scope:**
  - `crates/rusty-services/src/file_manager/`:
    - `types.rs`: базовые типы данных (`FileEntry`, `FileSortField`, `FileViewMode`, `FileTagColor`), persisted структуры (`FileTagsConfig`, `UserFileLocations`), модели томов (`VolumeInfo`, `discover_mounts`);
    - `listing.rs`: сканирование директорий, цели навигации `ListingTarget`, поиск, сортировка/фильтрация;
    - `trash.rs`: реализация спецификации `FreeDesktop Trash` (листинг, `.trashinfo`, перемещение, восстановление, очистка);
    - `operations.rs`: файловые операции (копирование, перемещение, переименование, дублирование, прогресс, отмена, разрешение конфликтов);
    - `metadata.rs`: детальная инспекция метаданных `FileDetailedInfo`, текстовые превью `TextPreview`, автодополнение путей `Go to Folder`;
    - `mod.rs`: публичный фасад со 100% re-exports и все 43 теста сервиса.
  - `apps/rusty-files/src/state/`:
    - `types.rs`: геометрические константы, плавная прокрутка `SmoothAxis`, `ColumnPane`, `SidebarRowItem`, `FileTabState`, типы попапов, сообщения очередей `mpsc`;
    - `tabs.rs`: многовкладочный режим (создание, переключение, закрытие, снапшоты);
    - `dialogs.rs`: модальные оверлеи (Quick Look, Get Info, Go to Folder, Rename, Transfer Conflict);
    - `clipboard.rs`: буфер обмена (Copy/Cut/Paste), дублирование, перемещение в корзину, запуск фоновых потоков передачи;
    - `search.rs`: поиск в папке и рекурсивный Home-поиск, кеш превью (LRU), назначение тегов;
    - `input.rs`: обработка горячих клавиш, физика скроллинга, анимационные тики и мигание каретки;
    - `navigation.rs`: навигация по папкам, история Back/Forward, сайдбар, хлебные крошки;
    - `mod.rs`: структура `FilesAppState`, инициализация, фасад состояния и юнит-тесты.
  - `docs/adr/0044-modular-decomposition-of-files-state-and-file-manager.md` и запись `D-052` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Все исходные файлы обеих подсистем декомпозированы на чистые модули (<700 строк логики на файл);
  - Ни один публичный интерфейс, тип или сигнатура функции не сломаны;
  - Все 43 теста `rusty-services` и 15 тестов `rusty-files` (все 244 теста воркспейса) проходят успешно;
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - ADR-0044 и D-052 зафиксированы.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace` (все 244 теста воркспейса пройдены успешно).
- **Risks:** Нет.

### RUSTYOS-089 — Модульная декомпозиция подсистемы рендеринга apps/rusty-files/src/render и консолидация разрозненных корней views.rs/dialogs.rs в layout.rs

- **Status:** Done (`RUSTYOS-089 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Завершить полную модульную реорганизацию `apps/rusty-files`, ликвидировав оставшиеся разрозненные корни `render.rs` (2,614 строк), `views.rs` (1,185 строк) и `dialogs.rs` (1,053 строки). Консолидировать всю геометрию раскладок и хит-тестинга в канонический `layout.rs`, а код отрисовки декомпозировать на чистые сфокусированные слои в пакете `render/` (canvas, chrome, sidebar, views, dialogs, popup, mod) со 100% сохранением всех тестов воркспейса, оформлением ADR-0045 и фиксацией записи D-053.
- **Dependencies:** `RUSTYOS-088`, `ADR-0045`, `D-053`.
- **Scope:**
  - `apps/rusty-files/src/layout.rs`:
    - Консолидация структур геометрии и хит-тестинга из `views.rs` (`TabItemLayout`, `TabBarLayout`, `PathCrumbLayout`, `IconCellLayout`, `IconsGridLayout`, `GalleryLayout`, `compute_*`, `hit_test_*`);
    - Консолидация структур геометрии и хит-тестинга из `dialogs.rs` (`QuickLookLayout`, `GetInfoLayout`, `GoToFolderLayout`, `ConflictDialogLayout`, `TransferDialogLayout`, `DialogOverlaysLayout`, `compute_*`, `hit_test_*`).
  - `apps/rusty-files/src/render/`:
    - `canvas.rs` (1,120 строк): загрузка Inter TTF шрифтов через `Fontdue`, `thread_local` кеш глифов, `Canvas<'a>` буфер пикселей и 2D SDF / RGSS-16 примитивы;
    - `chrome.rs` (432 строки): CSD светофоры окна, пилюли режимов, поле поиска, search scope bar, вкладки `render_tab_bar` и хлебные крошки `render_path_bar`;
    - `sidebar.rs` (128 строк): полупрозрачный Finder сайдбар, скроллбар и баннер корзины `render_trash_banner`;
    - `views.rs` (1,204 строки): Miller Columns, List View, Icons Grid (64px SSAA), Gallery View, инспектор превью и строка состояния;
    - `dialogs.rs` (783 строки): модальные листы и оверлеи (Quick Look, Get Info, Go to Folder, карточки конфликтов, инлайн-переименование);
    - `popup.rs` (353 строки): Liquid Glass спецификация всплывающих окон и поверхностей `xdg_popup`;
    - `mod.rs` (521 строка): токены `Palette`, `palette_for`, генерация кадров `render_window_pixels`, фасад реэкспорта со 100% обратной совместимостью и юнит-тесты рендерера.
  - Упразднение старых монолитных корней `apps/rusty-files/src/render.rs`, `views.rs`, `dialogs.rs` и директив `mod views;` / `mod dialogs;` в `main.rs`.
  - `docs/adr/0045-modular-decomposition-of-files-render-pipeline.md` и запись `D-053` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Корневые монолиты `render.rs`, `views.rs`, `dialogs.rs` полностью удалены;
  - Вся геометрия и хит-тестинг централизованы в `layout.rs`;
  - Подсистема отрисовки разделена на чистые слои в пакете `render/`;
  - 100% сохранение работоспособности: все 244 теста воркспейса проходят успешно;
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - ADR-0045 и D-053 зафиксированы.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace` (все 244 теста воркспейса пройдены успешно).
- **Risks:** Нет.

### RUSTYOS-090 — Модульная декомпозиция подсистемы TopBar композитора crates/rusty-compositor/src/shell_render/top_bar по стандартам CutefishOS и macOS 27

- **Status:** Done (`RUSTYOS-090 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Ликвидировать монолитный файл `crates/rusty-compositor/src/shell_render/top_bar.rs` (2,232 строки), декомпозировав его в чистую модульную структуру `top_bar/` (`panel.rs`, `menus.rs`, `status.rs`, `notifications.rs`, `mod.rs`) по стандартам CutefishOS и macOS 27. Сохранить 100% обратную совместимость всех публичных контрактов и все 244 теста рабочего пространства, оформить ADR-0046 и зафиксировать запись D-054.
- **Dependencies:** `RUSTYOS-089`, `ADR-0046`, `D-054`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/top_bar/`:
    - `panel.rs` (661 строк): структуры настроек и геометрии панели (`TopBarLayoutOptions`, `TopBarLeftLayout`, `TopBarRightLayout`), телеметрия `TopBarTelemetry`, вычисление раскладок `compute_top_bar_left_layout*` и `compute_top_bar_right_layout*`, отрисовка базовой панели и воркспейсов `render_top_bar`, `render_top_bar_workspaces`, хит-тестинг воркспейсов и раскладки окон;
    - `menus.rs` (759 строк): структуры и перечисления меню (`TopBarMenuHit`, `TopBarMenuAction`, `TopBarMenuEntry`, `TopBarMenuPopoverClick`, `MENU_POPOVER_RADIUS`), статические наборы меню, поповеры и их размеры (`top_bar_menu_popover_rect*`), хит-тестинг кликов и ховеров меню;
    - `status.rs` (477 строк): структуры и константы трея (`TopBarTrayItem`, `MAX_TOP_BAR_TRAY_ITEMS`), геометрия и хит-тестинг слотов трея, статусных пилюль, бейджа обновлений, поиска и Control Center, отрисовка монохромных глифов `push_top_bar_right_glyphs`;
    - `notifications.rs` (353 строк): структуры тостов и OSD (`NotificationToastView`, `MAX_VISIBLE_NOTIFICATIONS`, `OsdRenderState`), геометрия и хит-тестинг тостов уведомлений и бейджа, иконки приложений `notification_app_icon`, рендеринг OSD-пилюли громкости/яркости `generate_osd_elements` и `push_osd_level_bar`;
    - `mod.rs` (17 строк): фасад со 100% обратной совместимостью re-exports.
  - Упразднение монолитного файла `crates/rusty-compositor/src/shell_render/top_bar.rs`.
  - `docs/adr/0046-modular-decomposition-of-top-bar.md` и запись `D-054` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Монолит `top_bar.rs` на 2,232 строки разделен на 4 подмодуля (<800 строк каждый);
  - Внешний фасад `shell_render/mod.rs` и потребители не потребовали изменений;
  - Все 244 теста воркспейса проходят успешно;
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - ADR-0046 и D-054 оформлены.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace` (все 244 теста воркспейса пройдены успешно).
- **Risks:** Нет.

### RUSTYOS-091 — Модульная декомпозиция подсистемы модальных диалогов crates/rusty-compositor/src/shell_render/modals по стандартам CutefishOS и macOS 27

- **Status:** Done (`RUSTYOS-091 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Ликвидировать монолитный файл `crates/rusty-compositor/src/shell_render/modals.rs` (1,994 строки), декомпозировав его в чистую модульную структуру `modals/` (`polkit.rs`, `portal.rs`, `magnifier.rs`, `settings.rs`, `mod.rs`) по стандартам CutefishOS и macOS 27. Сохранить 100% обратную совместимость всех публичных контрактов и все 244 теста рабочего пространства, оформить ADR-0047 и зафиксировать запись D-055.
- **Dependencies:** `RUSTYOS-090`, `ADR-0047`, `D-055`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/modals/`:
    - `polkit.rs` (408 строк): структуры (`PolkitPromptRenderState`, `PolkitHit`, `PolkitModalLayout`), геометрия и хит-тестинг `polkit_modal_layout`, `hit_test_polkit_dialog`, обёртка сообщений `wrap_polkit_two_lines`, отрисовка текста и Liquid Glass композита;
    - `portal.rs` (442 строки): структуры (`PortalSourceRow`, `PortalPromptRenderState`, `PortalHit`, `PortalModalLayout`, `PORTAL_MAX_VISIBLE_ROWS`), геометрия списка и хит-тестинг `portal_visible_range`, `portal_modal_layout`, `hit_test_portal_dialog`, отрисовка текста и композита;
    - `magnifier.rs` (73 строки): геометрия плавающей пилюли `pick_color_pill_rect`, отрисовка текста и Liquid Glass оверлея `render_pick_color_text`, `render_pick_color_composite`;
    - `settings.rs` (1,104 строки): перечисления и модели (`SettingsTab`, `SettingsRenderState`, `SettingsHit`, `SettingsRowKind`, `SettingsRowEntry`), геометрия `settings_modal_layout`, `settings_row_pill_rect`, `settings_row_stepper_rects`, спецификация строк вкладок, хит-тестинг `hit_test_settings_modal`, отрисовка текста и интерактивных контролов;
    - `mod.rs` (28 строк): общий хелпер `phys_to_logical_rect` и фасад со 100% обратной совместимостью re-exports.
  - Упразднение монолитного файла `crates/rusty-compositor/src/shell_render/modals.rs`.
  - `docs/adr/0047-modular-decomposition-of-shell-modals.md` и запись `D-055` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Монолит `modals.rs` на 1,994 строки разделен на 4 специализированных подмодуля;
  - Внешний фасад `shell_render/mod.rs` и потребители не потребовали изменений;
  - Все 244 теста воркспейса проходят успешно;
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - ADR-0047 и D-055 оформлены.
- **Tests:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace` (все 244 теста воркспейса пройдены успешно).
- **Risks:** Нет.

### RUSTYOS-092 — Собственный Wayland-протокол Liquid Glass (rustyos_liquid_glass_v1), серверная реализация и интеграция в rusty-files

- **Status:** Done (`RUSTYOS-092 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Реализовать собственный открытый first-party Wayland-протокол `rustyos_liquid_glass_v1` для запроса клиентскими поверхностями (попапы, контекстные меню, сайдбар) аппаратного размытия бэкдропа и эффекта Liquid Glass. Обеспечить серверную обработку и двойную буферизацию состояния стекла в `crates/rusty-compositor`, аппаратный рендеринг Liquid Glass в `frame_composer.rs`, клиентскую интеграцию в `apps/rusty-files`, оформить ADR-0048 и зафиксировать запись D-056.
- **Dependencies:** `RUSTYOS-091`, `ADR-0048`, `D-056`.
- **Scope:**
  - `protocols/rustyos-liquid-glass-v1.xml`:
    - Интерфейс `rustyos_liquid_glass_manager_v1` (фабрика декораторов поверхностей, запрос `get_surface_glass`, деструктор);
    - Интерфейс `rustyos_surface_glass_v1` (запросы `set_region`, `set_corner_radius`, `set_style`, `set_tint`, `commit`, деструктор; пресеты стиля `Popover`, `Sidebar`, `ControlPill`, `BasePanel`, `Custom`).
  - `crates/rusty-compositor/src/state/wayland_handlers/liquid_glass.rs`:
    - Автогенерация интерфейсов и сервера с помощью `wayland-scanner`;
    - Структура `LiquidGlassSurfaceCachedState` с реализацией `smithay::wayland::compositor::Cacheable`;
    - `LiquidGlassManagerState` и регистрация глобала `RustyosLiquidGlassManagerV1` в дисплее;
    - Реализации `GlobalDispatch` и `Dispatch` для менеджера и поверхности стекла;
    - Хелперы `surface_liquid_glass_state` и `has_active_client_liquid_glass`.
  - `crates/rusty-compositor/src/frame_composer.rs`:
    - Активация `needs_backdrop_blur` при наличии активных клиентских поверхностей стекла;
    - Функция `push_window_liquid_glass_elements` в цикле отрисовки окон, генерирующая аппаратные элементы `glass_panel_render_element` под попапами и сайдбаром в строгом Z-порядке front-to-back.
  - `apps/rusty-files/src/liquid_glass.rs` и `apps/rusty-files/src/wayland.rs`:
    - Клиентские биндинги `wayland-scanner` для протокола;
    - Обнаружение глобала в реестре Wayland и привязка `RustyosLiquidGlassManagerV1`;
    - Интеграция `RustyosSurfaceGlassV1` для всех `xdg_popup` (стиль Popover, 16px радиус) и главного окна (стиль Sidebar, 12px радиус);
    - RAII-деструктор `Drop for ActiveWaylandPopup`.
  - `docs/adr/0048-rustyos-liquid-glass-wayland-protocol.md` и запись `D-056` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Полная независимость: протокол `rustyos_liquid_glass_v1` является нативной разработкой RustyOS, свободным от протоколов сторонних DE;
  - Серверная и клиентская части компилируются и взаимодействуют без сбоев;
  - Все 250 тестов воркспейса проходят успешно;
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - ADR-0048 и D-056 оформлены.
- **Tests:**
  - `state::wayland_handlers::liquid_glass::tests::test_liquid_glass_style_conversions`;
  - `state::wayland_handlers::liquid_glass::tests::test_liquid_glass_cached_state_defaults`;
  - `state::wayland_handlers::liquid_glass::tests::test_liquid_glass_cached_state_commit_and_merge`;
  - `state::tests::test_rustyos_liquid_glass_protocol_integration`;
  - Все 250 тестов воркспейса (`cargo test --workspace`).
- **Risks:** Нет.

### RUSTYOS-093 — Приведение Liquid Glass контейнера Дока к эталону 1:1 macOS 27 Golden Gate

- **Status:** Done (`RUSTYOS-093 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Устранить визуальные расхождения между рендерингом Дока RustyOS и эталоном macOS 27 Golden Gate: перевести контейнер на честную геометрию Pill Capsule (`radius = height / 2 = 34px`), внедрить непрерывный top-lit световой шельф в шейдере и SDF, контактное притенение нижней грани и мягкую объемную тень `push_soft_shadow` под доком.
- **Dependencies:** `RUSTYOS-092`, `ADR-0049`, `D-057`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/tokens.rs`:
    - `DOCK_RADIUS = DOCK_HEIGHT / 2` (34px);
    - `DOCK_CAP_PADDING = 14px` для предотвращения наложения крайних иконок на полукруглые торцы;
  - `crates/rusty-compositor/src/shell_render/dock.rs`:
    - Использование `DOCK_CAP_PADDING` при расчете длины капсулы и стартовой позиции иконок;
  - `crates/rusty-compositor/src/shell_render/mod.rs`:
    - Добавление `push_soft_shadow(out, shape_cache, renderer, capsule, DOCK_RADIUS, 20, 72)` под капсулой Дока в `push_dock_chrome_shapes`;
    - Экспорт `DOCK_CAP_PADDING` в публичный интерфейс `shell_render`;
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs` (`LIQUID_GLASS_FRAG`):
    - Верхний источник света `light_top = vec2(-0.35, -0.94)` и расчет `top_shelf` для непрерывного горизонтального фасета;
    - Контактное притенение нижней кромки `bottom_occlusion`;
  - `crates/rusty-compositor/src/surfaces/sdf.rs`:
    - Согласование аналитического CPU-семплера с моделью `top_shelf` и притенения нижней кромки;
  - `apps/rusty-shell/src/wayland.rs`:
    - Перевод радиуса контейнера Дока на `h / 2`;
  - `crates/rusty-compositor/src/shell_render/tests.rs`:
    - Обновление теста констант Дока под `DOCK_RADIUS = 34`;
  - `docs/adr/0049-macos-27-pill-capsule-liquid-glass-dock.md` и запись `D-057` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Док визуально идентичен референсу macOS 27 Golden Gate (Pill Capsule, выразительный верхний шельф, мягкое парение над обоями);
  - Все 250 тестов воркспейса проходят успешно;
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - Живая проверка в `--winit` подтверждена скриншотами.
- **Tests:**
  - `shell_render::tests::macos_dock_divider_gap_tooltip_and_radii_constants`;
  - `surfaces::tests::dock_glass_and_icon_shadow_rasterize_cleanly`;
  - Все 250 тестов воркспейса (`cargo test --workspace`).
- **Risks:** Нет.

### RUSTYOS-094 — Приведение контейнера Дока к точному референсу: радиус 18px, плоское матовое стекло, равномерный 1px кант

- **Status:** Done (`RUSTYOS-094 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Устранить эффект выпуклости стекла и избыточную скруглённость контейнера Дока, приведя его в точное визуальное соответствие с референсом: радиус скругления 18px (прямые вертикальные боковины), удаление выпуклых искажений из шейдера и SDF, плоское матовое акриловое стекло с деликатной 1px окантовкой.
- **Dependencies:** `RUSTYOS-093`, `ADR-0050`, `D-058`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/tokens.rs`:
    - `DOCK_RADIUS = 18px`;
    - удаление искусственного `DOCK_CAP_PADDING`;
  - `crates/rusty-compositor/src/shell_render/dock.rs`:
    - возврат к стандартному `DOCK_PADDING = 8` для оси и длины капсулы;
  - `crates/rusty-compositor/src/shell_render/mod.rs`:
    - удаление `DOCK_CAP_PADDING` из публичных экспортов;
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs` (`LIQUID_GLASS_FRAG`):
    - удаление асимметричных `top_shelf` и `bottom_occlusion`, исключение 3D-выпуклости стекла;
    - деликатный равномерный 1px кант по всему контуру;
  - `crates/rusty-compositor/src/surfaces/sdf.rs`:
    - приведение `diagonal_glass_rim` и `sample_glass_panel_pixel_themed` к плоскому матовому стеклу без выпуклого затемнения;
  - `apps/rusty-shell/src/wayland.rs`:
    - радиус контейнера Дока зафиксирован на `18.min(h / 2).max(1)`;
  - `crates/rusty-compositor/src/shell_render/tests.rs` и `crates/rusty-compositor/src/surfaces/tests.rs`:
    - обновление тестов констант под `DOCK_RADIUS = 18`;
  - `docs/adr/0050-flat-frosted-liquid-glass-dock.md` и запись `D-058` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Контейнер Дока визуально идентичен прикреплённому референсу (радиус 18px, ровные боковины, плоское матовое стекло, тонкий равномерный 1px кант);
  - Все 250 тестов воркспейса проходят успешно;
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - Живой `--winit` снимок подтверждён.
- **Tests:**
  - `shell_render::tests::macos_dock_divider_gap_tooltip_and_radii_constants`;
  - `surfaces::tests::dock_glass_and_icon_shadow_rasterize_cleanly`;
  - Все 250 тестов воркспейса (`cargo test --workspace`).
- **Risks:** Нет.

### RUSTYOS-095 — Калибровка Liquid Glass Дока по стандарту CutefishOS: удаление внешней тени и 3D-бликов

- **Status:** Done (`RUSTYOS-095 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Устранить паразитную тень под контейнером Дока и угловые 3D-блики, создававшие эффект пластиковой кнопки, приведя материал Дока к стандарту CutefishOS (`cutefishos/cutefish-compositor`): плоское матовое стекло, тонкий равномерный 1px hairline кант без вспышек в углах, полное удаление drop shadow.
- **Dependencies:** `RUSTYOS-094`, `ADR-0051`, `D-059`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/mod.rs`:
    - удаление вызова `push_soft_shadow(..., 20, 72)` под Доком в `push_dock_chrome_shapes`;
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`:
    - шейдер `LIQUID_GLASS_FRAG`: удаление угловых бликов (`arc_tl`/`arc_br`) и внутреннего каустика для Дока, формирование чистого равномерного 1px hairline канта;
    - `compute_glass_material`: обнуление вертикальной дымки `sheen = 0.0` для `GlassPanelStyle::Dock` в светлой и тёмной темах, тонкая равномерная окантовка `rim = 0.26` (Light) и `0.18` (Dark);
  - `crates/rusty-compositor/src/surfaces/sdf.rs`:
    - `sample_glass_panel_pixel_themed`: обнуление `sheen_limit = 0.0` для Дока, равномерный фактор канта `spec_dir = 1.0` по всему контуру;
  - `crates/rusty-compositor/src/surfaces/tests.rs`:
    - обновление теста `dock_glass_and_icon_shadow_rasterize_cleanly` под равномерный симметричный 1px кант;
  - `docs/adr/0051-cutefish-calibrated-dock-glass-and-shadow-removal.md` и запись `D-059` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Под Доком отсутствует внешняя темная тень (drop shadow);
  - Стекло Дока плоское и матовое, без пластиковых бликов в углах и вертикальной дымки;
  - Контур оформлен тонким, ровным 1px hairline кантом;
  - Все 250 тестов воркспейса проходят успешно;
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - Живой `--winit` снимок подтверждён.
- **Tests:**
  - `surfaces::tests::dock_glass_and_icon_shadow_rasterize_cleanly`;
  - Все 250 тестов воркспейса (`cargo test --workspace`).
- **Risks:** Нет.

### RUSTYOS-096 — Устранение выпуклости и остаточных 3D-артефактов Liquid Glass Дока (dock_flat path)

- **Status:** Done
- **Goal:** После RUSTYOS-095 Dok-контейнер визуально оставался выпуклым из-за активных `lens_band` (рефракция 8.5px), `light_fresnel` (Fresnel-хайлайт) и `outer_bevel * rim` в GPU-шейдере и CPU-fallback пути. RUSTYOS-096 полностью подавляет эти артефакты для Дока через `dock_flat` маску.
- **Dependencies:** `RUSTYOS-095`.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`:
    - `compute_glass_material` Dock Light/Dark: `sat_refract_rim_sheen = [sat, 0.0, 0.0, 0.0]` (refract=0, rim=0, sheen=0);
    - `LIQUID_GLASS_FRAG`: добавлена `dock_flat = step(refract + rim + sheen, 0.0001)` маска, `lens_band *= (1 - dock_flat)`, `light_fresnel *= (1 - dock_flat)`;
    - тест `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive`: обновлены assertions на `refract==0` и `rim==0` для Дока.
  - `crates/rusty-compositor/src/surfaces/sdf.rs`:
    - `sample_glass_panel_pixel_themed`: `rim_peak` Dock = 0.0, `light_fresnel` Dock = 0.0;
    - `reduce_transparency` ветка: `rim_mix` Dock = 0.0.
- **Acceptance Criteria:**
  - Стекло Дока полностью плоское, матовое, без выпуклости, линзового эффекта и Fresnel-хайлайтов;
  - Нет тени под/вокруг Дока;
  - 170 тестов `rusty-compositor` и все workspace-тесты проходят;
  - `cargo clippy` и `cargo fmt --check` чисты;
  - Живой `--winit` прогон визуально подтверждён.
- **Tests:** `surfaces::tests::dock_glass_and_icon_shadow_rasterize_cleanly`, `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive`.
- **Risks:** Нет.

### RUSTYOS-097 — Устранение вертикального градиента t_v в base_glass_layer для Liquid Glass Дока

- **Status:** Done
- **Goal:** Полностью устранить вертикальный градиент `t_v` в альфе и RGB базового слоя `base_glass_layer` для `GlassPanelStyle::Dock` Light и Dark, сделав тинт стекла 100% однородным и плоским без 3D-перепадов яркости по вертикали (CutefishOS reference).
- **Dependencies:** `RUSTYOS-096`.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces/sdf.rs`:
    - `base_glass_layer`: удаление `- 0.05 * t_v` / `- 12.0 * t_v` (Dark) и `- 0.04 * t_v` / `- 10.0 * t_v` (Light) для `GlassPanelStyle::Dock`;
  - `crates/rusty-compositor/src/surfaces/tests.rs`:
    - обновление проверки `dock_glass_and_icon_shadow_rasterize_cleanly`: утверждение строгой вертикальной симметрии `top_left_arc[3] == bottom_left_arc[3]`.
- **Acceptance Criteria:**
  - Однородный плоский цвет стекла Дока без перепада яркости верх/низ;
  - 100% тестов воркспейса проходят (251 passed);
  - `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - Живой `--winit` снимок подтверждён.
- **Tests:** `surfaces::tests::dock_glass_and_icon_shadow_rasterize_cleanly`, все 251 тестов workspace.
- **Risks:** Нет.

### RUSTYOS-098 — Калибровка контейнера Дока по формулам CutefishOS: динамический радиус `thickness * 28 / 86`, нейтральный тинт 0.22 и 1px screen-blend hairline кант

- **Status:** Done
- **Goal:** Привести активный GPU-шейдер и геометрию Дока к эталону CutefishOS: пропорциональный радиус скругления углов (`thickness * 28 / 86`), нейтральный полупрозрачный серый тинт (alpha 0.22 при полной прозрачности) и равномерный 1px screen-blend hairline кант без выпуклости и угловых 3D-бликов.
- **Dependencies:** `RUSTYOS-097`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/tokens.rs` и `mod.rs`: добавление `compute_dock_radius(thickness)` по формуле `(thickness * 28 + 43) / 86` с клампом `12..=(thickness / 2).max(12)` и передача динамического радиуса в `push_dock_chrome_shapes`;
  - `apps/rusty-shell/src/wayland.rs`: расчёт радиуса контейнера в `render_dock_pixels` по `((h * 28 + 43) / 86).min(h / 2).max(1)`;
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`: калибровка `compute_glass_material` для `GlassPanelStyle::Dock` (Light: `tint_rgba = [0.60, 0.60, 0.62, 0.10 + 0.12 * g]`, `sat_refract_rim_sheen = [1.30, 0.0, 0.35, 0.0]`; Dark: `[0.12, 0.12, 0.14, 0.14 + 0.14 * g]`, `[1.26, 0.0, 0.28, 0.0]`) и маска `dock_flat` в `LIQUID_GLASS_FRAG` с 1px screen-blend hairline кантом;
  - `crates/rusty-compositor/src/surfaces/sdf.rs` и `tests.rs`: синхронизация CPU fallback (`base_glass_layer`, `sample_glass_panel_pixel_themed`) и обновление тестов `dock_glass_and_icon_shadow_rasterize_cleanly`, `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive`.
- **Acceptance Criteria:**
  - Динамический радиус пропорционален высоте Дока (~16px при `icon_size = 36`, высоте 50px);
  - Плоский нейтральный тинт Дока с равномерным 1px screen-blend hairline кантом в GPU и CPU путях;
  - 100% тестов воркспейса проходят (251 passed), `clippy` и `fmt --check` чисты;
  - Живой `--winit` прогон и визуальное сравнение с эталоном подтверждены.
- **Tests:** `surfaces::tests::dock_glass_and_icon_shadow_rasterize_cleanly`, `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive`, все 251 тестов workspace.
- **Risks:** Нет.

### RUSTYOS-099 — Точный перенос шейдера Liquid Glass из CutefishOS (`superellipse r*1.5`, 14px линза, направленный `uRim`), привязка Дока к `half_a` и выравнивание нижней зоны обоев

- **Status:** Done
- **Goal:** Воспроизвести 1:1 математику Liquid Glass из `CutefishOS` (`rounded_clip.glsl` + `glass_fragment_shader.glsl` + `DockWindow::glass()`) для Дока, убрать боковую белую рамку и устранить тёмный диагональный перепад обоев за Доком.
- **Dependencies:** `RUSTYOS-098`.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`:
    - в `LIQUID_GLASS_FRAG`: кубическая суперэллипса `r * 1.5` (`roundedDistance` + 4-точечный `roundedCoverage`), 14px суперэллиптический профиль линзы (`lensHeight`/`lensNormalAt`/`lensOffset`/`refractedDockAt`, `ior = 1.7`, `displacement = 36.0`) и 3-слойный направленный `uRim` (`shade`, `hairline`, `glow`, `lit = 1.0/0.35/0.0`);
    - в `compute_glass_material`: `Light` `[0.60, 0.60, 0.60, 0.13 + 0.12 * g]`, `Dark` `[0.38, 0.38, 0.38, 0.15 + 0.14 * g]`, `[1.30, 36.0, 1.0, 0.0]`;
    - в `make_glass_element`: выбор `buffers.half_a` для `GlassPanelStyle::Dock`;
  - `crates/rusty-compositor/src/surfaces/sdf.rs` и `tests.rs`: сглаживание `sheet3` в `apply_golden_gate_folds`, общий `outward_normal_2d` и `dock_rim_alpha`, обновление тестов `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive` и `dock_glass_and_icon_shadow_rasterize_cleanly`;
  - `docs/adr/0052-cutefish-exact-superellipse-lens-and-rim-shader.md`, `DECISIONS.md` (`D-063`).
- **Acceptance Criteria:**
  - Док рендерится кубической суперэллипсой с 14px преломлением без пластиковых бликов и без белой обводки на боковых торцах;
  - Нет тёмной диагональной складки обоев позади Дока;
  - 100% тестов воркспейса (251 passed), `clippy` и `fmt` чисты;
  - Живой `--winit` прогон (без ошибок GLES/шейдера) и визуальная сверка снимка с эталоном выполнены.
- **Tests:** `surfaces::tests::dock_glass_and_icon_shadow_rasterize_cleanly`, `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive`, все 251 тестов workspace.
- **Risks:** Нет.

### RUSTYOS-100 — Системная унификация Liquid Glass по эталону CutefishOS (`popupGlass` / `fishuiGlass`)

- **Status:** Done
- **Goal:** Распространить единый суперэллиптический пайплайн CutefishOS (`roundedDistance`/`roundedCoverage`, 14px рефракционная линза и 3-слойный направленный `uRim` без `sheen`/угловых вспышек) на все остальные части проекта: `Popover`, `ControlModule`, клиентские поверхности `rustyos_liquid_glass_v1` (`Sidebar`, `ControlPill`), меню, Control Center, Spotlight, уведомления, модальные окна и рендер пилюль/поповеров `rusty-files`.
- **Dependencies:** `RUSTYOS-099`.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`: удаление ветвления `else` в `LIQUID_GLASS_FRAG`, внедрение единого RGSS-сэмплера `refractedAt` с `chroma` (`u_sat_refract.w`) и калибровка `compute_glass_material` (`Popover` = `popupGlass`, `ControlModule` = `fishuiGlass`);
  - `crates/rusty-compositor/src/surfaces/sdf.rs` и `tests.rs`: однородный `base_glass_layer`, общий `cutefish_rim_alpha` без `sheen`/`diagonal_glass_rim`/`t_v`, обновление проверок в `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive` и `dock_glass_and_icon_shadow_rasterize_cleanly`;
  - `apps/rusty-files/src/render/sidebar.rs` и `popup.rs`: обновление `draw_glass_pill` и выделение `render_popup_chrome` с направленным верхним/нижним 1px `hairline` без резкой засветки;
  - `docs/adr/0053-system-wide-cutefish-liquid-glass-unification.md`, `DECISIONS.md` (`D-064`).
- **Acceptance Criteria:**
  - Единый GPU-шейдер без легаси-ветки для всех стеклянных поверхностей композитора и Wayland-клиентов;
  - 100% тестов воркспейса (251 passed), `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты;
  - Живой `--winit` прогон (Spotlight + Уведомление + `rusty-files` + Док) подтверждён визуально без GLES-ошибок.
- **Tests:** Все 251 тестов workspace (`rusty-compositor`, `rusty-files`, `rusty-shell`, `rusty-ipc`, `rusty-services`, `rusty-settings`, `rusty-updater`).
- **Risks:** Нет.

### RUSTYOS-101 — Модульная декомпозиция `crates/rusty-compositor/src/shell_render/mod.rs` на проходы рендера

- **Status:** Done
- **Goal:** Разбить 2014-строчный `crates/rusty-compositor/src/shell_render/mod.rs` на специализированные подмодули проходов рендера (`text_pass.rs`, `overlay_shapes.rs`, `chrome_shapes.rs`) и компактный фасад `mod.rs` без изменения публичного API и с сохранением 100% тестов.
- **Dependencies:** `RUSTYOS-091`, `RUSTYOS-100`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/text_pass.rs` (595 строк): `append_foreground_overlay_text`, `generate_shell_text_elements`, `append_chrome_text`;
  - `crates/rusty-compositor/src/shell_render/overlay_shapes.rs` (398 строк): `generate_shell_foreground_overlay_elements`, `build_foreground_overlay_shapes`;
  - `crates/rusty-compositor/src/shell_render/chrome_shapes.rs` (759 строк): `generate_shell_chrome_composite_elements`, `generate_shell_overview_elements`, `build_chrome_shapes`, Док-функции (`push_dock_chrome_shapes`, `push_dock_slot_shape`, `push_dock_tooltip_and_drag`, `render_dock_tile_icon`) и `build_overview_shapes`;
  - `crates/rusty-compositor/src/shell_render/mod.rs` (329 строк): реэкспорты и корневые точки входа; `tests.rs` (явные импорты `Point`/`Rectangle`/`anim_offset`);
  - `docs/adr/0054-modular-decomposition-of-shell-render-passes.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-065`).
- **Acceptance Criteria:**
  - `shell_render/mod.rs` сокращён с 2014 до 329 строк без изменения публичного API;
  - `cargo fmt --all -- --check` и `cargo clippy --workspace --all-targets -- -D warnings` чисты;
  - 100% тестов воркспейса проходят (251 passed, 0 failed).
- **Tests:** Все 251 тестов workspace.
- **Risks:** Нет.

### RUSTYOS-102 — Редизайн окна System Settings по архитектуре и эталону `cutefishos/settings`

- **Status:** Done
- **Goal:** Перестроить встроенное окно `System Settings` (`crates/rusty-compositor/src/shell_render/modals/settings.rs`) по архитектуре `cutefishos/settings`: разделение `Liquid Glass` сайдбара и непрозрачного правого холста страницы, шапка с traffic lights, полем `Search` и `← → Title`, категоризированные `24×24` (`r=6`) `RowIcon` бейджи без purple/lavender, визуальная карточка превью `Light`/`Dark` + акцентных свотчей и групповые карточки `Card` с `Switch`-тумблерами.
- **Dependencies:** `RUSTYOS-091`, `RUSTYOS-100`, `RUSTYOS-101`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/modals/settings.rs`: геометрия `760×520`, разделение `sidebar_w = 220` (`Liquid Glass`) и непрозрачного правого холста `push_right_rounded_canvas` (`#F4F5F9` Light / `#1C1C1E` Dark, скругление `WINDOW_RADIUS - 1`), traffic lights + `Search` pill, категоризированные `Desktop`/`Devices`/`System` табы с цветными `RowIcon` и нейтральным выделением активного таба, карточка превью `Light`/`Dark` + 6 свотчей, групповые карточки `Card::Style::List` с `1px` разделителями и графические `40×22` `Switch`-тумблеры;
  - `crates/rusty-compositor/src/state/wm_windows.rs`: вызов `self.open_settings()` из `handle_launch_command_ipc` для `"settings" | "rusty-settings" | "system-settings"`;
  - `crates/rusty-compositor/src/shell_render/tests.rs`: обновление `settings_modal_hit_testing_covers_tabs_controls_and_buttons`, `desktop_dock_settings_tab_hit_test_work` и добавление `settings_modal_cutefish_appearance_and_close_targets`;
  - `docs/adr/0055-cutefish-settings-window-architecture.md`, `DECISIONS.md` (`D-066`).
- **Acceptance Criteria:**
  - Живой `--winit` прогон подтверждён визуально в режимах `Dark` и `Light` без GLES-ошибок;
  - `cargo fmt --all -- --check` и `cargo clippy --workspace --all-targets -- -D warnings` чисты;
  - 100% тестов воркспейса проходят (252 passed, 0 failed).
- **Tests:** Все 252 тестов workspace.
- **Risks:** Нет.

### RUSTYOS-103 — Вынос System Settings в отдельное Wayland-окно `apps/rusty-settings`, вертикальный скролл и сквозная смена темы

- **Status:** Done
- **Goal:** Вынести `System Settings` в самостоятельное перемещаемое и изменяемое по размеру Wayland CSD-окно `apps/rusty-settings` (`882×600`, `app_id = "org.rustyos.Settings"`), убрать все пользовательские упоминания сторонних систем (`CutefishOS` / `Golden Gate`), добавить вертикальный скроллинг контента/сайдбара и обеспечить полное сквозное переключение темы (`Light` / `Dark` / `Auto`) across композитор и приложения.
- **Dependencies:** `RUSTYOS-102`.
- **Scope:**
  - `apps/rusty-settings` (`Cargo.toml`, `src/main.rs`, `src/state.rs`, `src/layout.rs`, `src/render.rs`, `src/wayland.rs`): самостоятельный Wayland CSD-клиент (`882×600`, минимум `640×500`), плоский сайдбар (`#ECEDF1`/`#262628`) без Liquid Glass, скроллбар и клиппинг контента (`Canvas::clip`), `PointerEventKind::Axis` и клавиатурная прокрутка, поддержка `ThemeChoice::Auto` (`theme_mode = "auto"`), mtime-синхронизация `config.toml` и надёжное уведомление `v1|RELOAD_CONFIG`.
  - `crates/rusty-ipc/src/lib.rs`: выполнение всех принятых строк запроса в `IpcServer::poll` даже при закрытии сокета клиентом после отправки.
  - `crates/rusty-compositor`: интеграция в `launcher.rs`, `dock.rs`, `wm_windows.rs` (центрированный `882×600` при старте) и `shell_actions.rs` (запуск/фокус `rusty-settings` при живом Wayland-сокете); инвалидация FBO `LiquidGlassPipeline::invalidate_all` (`liquid_glass.rs`, `surfaces/mod.rs`) и сброс `wallpaper_revision` при смене темы; адаптивная Light/Dark раскраска SSD-рамок и заголовков в `shell_render/ssd.rs`; плоский сайдбар и карточки без тёмных рамок в фолбэке `modals/settings.rs`.
  - `apps/rusty-files` и `apps/rusty-updater`: live mtime-синхронизация системной темы из `config.toml` и очистка UI-текста.
  - `docs/adr/0055-cutefish-settings-window-architecture.md`, `DECISIONS.md` (`D-067`).
- **Acceptance Criteria:**
  - `rusty-settings` запускается как независимое Wayland-окно (`882×600`) из Дока/меню/IPC, прокручивается колесом мыши и клавишами, не содержит UI-упоминаний `CutefishOS` и `Golden Gate`;
  - Смена темы `Light`/`Dark` синхронно обновляет фон, `Liquid Glass`, `TopBar`, `Dock`, уведомления/OSD, SSD-заголовки, `rusty-settings`, `rusty-files` и `rusty-updater`;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` и живые `--winit` прогоны Light + Dark пройдены без ошибок.
- **Tests:** Все unit-тесты workspace (`rusty-compositor`, `rusty-settings`, `rusty-files`, `rusty-updater`, `rusty-shell`, `rusty-ipc`, `rusty-services`, `rusty-animation`, `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-104 — Выравнивание `Control Center` под единый `Liquid Glass` (`ControlModule`)

- **Status:** Done
- **Goal:** Откалибровать `GlassPanelStyle::ControlModule` (GLES + CPU fallback) и обновить острова, капсулы, слайдеры, тени и контраст подписей `Control Center` под обновлённый `Liquid Glass` в `Light` и `Dark` темах.
- **Dependencies:** `RUSTYOS-100`, `RUSTYOS-101`, `RUSTYOS-103`.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`: `compute_glass_material` и `make_glass_element` для `ControlModule` переведены на сглаженный `buffers.half_out`, `[1.30, 34.0, 1.0, 0.01]`, Light `[0.72, 0.74, 0.78, 0.11 + 0.11 * g]` и Dark `[0.24, 0.24, 0.27, 0.15 + 0.15 * g]`; расширен `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive`;
  - `crates/rusty-compositor/src/surfaces/sdf.rs`: синхронизация CPU/fallback-подложки (`base_glass_layer`, `reduce_transparency` Light `[238.0, 240.0, 245.0]` и `rim_peak`);
  - `crates/rusty-compositor/src/shell_render/quick_settings.rs`: полные капсулы `Display`/`Sound` (`r = h / 2 = 29`), тень островов `Some((18, 42))`, тонкий `5 px` белый трек слайдера с `12 px` круглым белым thumb-бегунком, `7 px` inset активного круга тумблеров для сохранения `uRim`, очистка лишних рамок у иконок/mute/`Now Playing`, улучшение контраста заголовков и подписей (`popover_text_secondary`);
  - `crates/rusty-compositor/src/state/wm_windows.rs` и `crates/rusty-compositor/src/shell_render/tests.rs`: IPC `"control-center" | "quick-settings"` и проверки геометрии капсул в `macos27_quick_settings_glass_and_volume_slider_hit_testing`;
  - `DESIGN.md` (`§12.4`, `§12.5`), `docs/adr/0056-control-center-liquid-glass-alignment.md`, `DECISIONS.md` (`D-068`).
- **Acceptance Criteria:**
  - Живой `--winit` прогон в `Light` и `Dark` режимах подтверждён визуально без EGL/shader-ошибок и без резкого просвечивания фоновых иконок;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и 256 тестов workspace проходят без ошибок и регрессий.
- **Tests:** Все 256 unit-тестов workspace (`4` `rusty-animation`, `171` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `43` `rusty-services`, `4` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-105 — Унификация `rusty-files`, `rusty-updater` и радиуса окон/теней композитора под стиль `rusty-settings`

- **Status:** Done
- **Goal:** Привести все системные CSD-окна (`rusty-files`, `rusty-updater`, `rusty-settings`) и радиус окон/теней композитора (`WINDOW_RADIUS`) к единому визуальному стандарту `rusty-settings`.
- **Dependencies:** `RUSTYOS-103`, `RUSTYOS-104`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/tokens.rs` и `tests.rs`: перевод `WINDOW_RADIUS` с `20` на `14` для точного совпадения теней `WindowShadowPipeline` и углов CSD-окон;
  - `apps/rusty-settings/src/render.rs`: отрисовка `1 px` контура окна строго до `apply_window_corner_mask` и удаление лишнего квадратного `stroke_rounded_rect` вокруг кружков traffic lights;
  - `apps/rusty-updater/src/{layout,render}.rs`: `WINDOW_CORNER_RADIUS = 14`, `TITLEBAR_HEIGHT = 42`, `14 px` traffic lights `(14/36/58, 13)`, палитра `#F5F6FA`/`#1C1C1D` + карточки `#FFFFFF`/`#2C2C2D` (`r = 12`) и пилюли без серых рамок;
  - `apps/rusty-files/src/{layout.rs,render/{mod.rs,sidebar.rs,chrome.rs}}`: `WINDOW_CORNER_RADIUS = 14`, `14 px` traffic lights `(14/36/58, 13)`, палитра сайдбара `#ECEDF1`/`#262628`, активная строка `#D9DADD`/`#3E3E41` (`r = 10`), бесшовные капсулы тулбара/вкладок без рамок и обводка окна до маски;
  - `DESIGN.md` (`§12.3`), `docs/adr/0057-unified-csd-window-style.md`, `DECISIONS.md` (`D-069`).
- **Acceptance Criteria:**
  - Живой `--winit` прогон в `Light` и `Dark` режимах с открытыми `rusty-files`, `rusty-updater --dry-run` и `rusty-settings` подтверждён визуально без EGL/GLES-ошибок;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и все 256 тестов workspace проходят без ошибок.
- **Tests:** Все 256 unit-тестов workspace (`4` `rusty-animation`, `171` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `43` `rusty-services`, `4` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-106 — Исправление SDF-форм и сетки $D = 70\text{ px}$ в `Control Center` по эталону

- **Status:** Done
- **Goal:** Перевести `LIQUID_GLASS_FRAG` на чистый евклидов $L_2$ circular-arc SDF без сквиркл-искажений и выровнять все модули `Control Center` по точной эталонной сетке $D = 70\text{ px}, G = 12\text{ px}$.
- **Dependencies:** `RUSTYOS-104`.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`: в `LIQUID_GLASS_FRAG` (`cornerExtent`, `cornerNormal`, `roundedDistance`, `outwardAt`, `tangentCoverage`) применён чистый евклидов $L_2$ SDF (`sdRoundBox`) для точных кругов, полукруглых капсул и скруглённых карточек без раздувания углов;
  - `crates/rusty-compositor/src/shell_render/quick_settings.rs`: выровнена сетка `pop_w = 316, pop_h = 516, gap = 12, col_w = 152, btn_d = 70`: капсулы `Wi-Fi`/`Bluetooth`/`Power`/`Focus` `152×70` (`r = 35`) с концентрическим кружком иконки `46 px` (`pad = 12`), 4 круглые кнопки `70×70` (`r = 35`, `6 px` inset при активации), квадратная карточка `Now Playing` `152×152` (`r = 28`, обложка `44×44` `r = 12`), слайдерные карточки `Display`/`Sound` `316×66` (`r = 26`), кнопка `Lock Screen` `118×28` (`r = 14`);
  - `crates/rusty-compositor/src/shell_render/tests.rs` и `src/input/tests.rs`: проверки `display_card.size.h == 66`, `sound_card.size.h == 66`, `layout_btn.size.w == layout_btn.size.h`, `now_playing_card.size.w == now_playing_card.size.h` и обновлённые координаты `track_y` (`qs_y + 373`) и `focus_y` (`qs_y + 281`);
  - `DESIGN.md` (`§12.4`, `§12.5`), `docs/adr/0056-control-center-liquid-glass-alignment.md`, `DECISIONS.md` (`D-070`).
- **Acceptance Criteria:**
  - На живых `--winit`-скриншотах в `Light` и `Dark` кнопки-переключатели являются точными кругами `70×70`, капсулы `152×70` имеют концентрический кружок `46 px`, `Not Playing` — квадрат `152×152`, а `Display`/`Sound` — плашки `316×66` (`r = 26`) в 1:1 пропорциях с эталоном;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и все 256 тестов workspace проходят без ошибок.
- **Tests:** Все 256 unit-тестов workspace (`4` `rusty-animation`, `171` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `43` `rusty-services`, `4` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-107 — Унификация контекстных и выпадающих меню под эталон `macOS 27 Golden Gate`

- **Status:** Done
- **Goal:** Привести выпадающие меню верхней панели, системные контекстные меню и клиентские `xdg_popup` меню `rusty-files` к единой концентрической геометрии (`r = 13` / `r_pill = 7`, `24 px` строки), двухрежимному левому выравниванию и правой колонке шорткатов без изменения `Liquid Glass` шейдера, `Dock` и `Control Center`.
- **Dependencies:** `RUSTYOS-105`, `RUSTYOS-106`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/{tokens.rs, mod.rs, overlay_shapes.rs, top_bar/menus.rs}`: `MENU_POPOVER_RADIUS = 13` (для карточек уведомлений сохранён радиус `16`), `MENU_POPOVER_PAD_Y = 5`, `MENU_ITEM_HEIGHT = 24`, `MENU_SEPARATOR_HEIGHT = 9`, `pop_w = 228`, `pub(crate) fn push_liquid_glass_panel`;
  - `crates/rusty-compositor/src/shell_render/{top_bar_menus.rs, chrome_shapes.rs, text_pass.rs}`: `icon: Option<UiGlyphKind>`, единое выравнивание секций (`x = 14 px` без иконки / `14 px` глиф + `x = 35 px` текст `13 px`), плашка ховера `(mx + 5, row_y + 1, mw - 10, 22)` (`r = 7`) и шорткаты `12 px` (`⌘`/`⌥`/`⇧`/`⌃`/`⌫`/`▲`/`⇥`);
  - `crates/rusty-compositor/src/{shell_render/context_menu.rs, context_menu/desktop_menu.rs, frame_composer.rs}`: `icon: Option<UiGlyphKind>`, `text_item`, `.without_icon()`, `render_panel_row_shapes`, `r = 7` плашка, шеврон/галочка на `mx + mw - 24`, шорткаты `⇧⌘N`, `⌥⏎`, `⌃▲`, `⌥L`, а также `push_client_popup_elements` / `push_popup_glass_panels` для рендера клиентских `xdg_popup` поверх `PrepareBackdrop`, основных окон и `Dock` (исключены из `backdrop_list`);
  - `apps/rusty-files/src/{layout.rs, render/popup.rs, wayland.rs}`: `row_h = 24`, `total_h = 10 + ...`, `MENU_POPOVER_RADIUS = 13`, `HOVER_PILL_RADIUS = 7`, `HOVER_PILL_INSET_H = 5`, `wl_region = spec.layout.rect` в `sync_popup_surface`, прозрачный SHM-фон при активном композиторном стекле (`render_popup_surface_pixels_with_glass`), раздельные `any_non_swatch_checked` / `swatch_rgb` (`x + 14` текст, `cx = x + 19.0` + `x + 32` для тегов, `x + 14` чекмарк + `x + 28` для `SortMenu`) и обновлённые unit-тесты;
  - `DESIGN.md` (`§6 Menus and Popovers`, `§12.3`), `ARCHITECTURE.md` (`§7`), `.agents/skills/ui-ux-design/SKILL.md`, `docs/adr/0058-unified-context-menu-design.md`, `DECISIONS.md` (`D-071`).
- **Acceptance Criteria:**
  - Живые `--winit` прогоны (Wayland/X11 smoke и `Light`/`Dark` снимки `Go`, `System`, `Desktop` и клиентского `xdg_popup` `rusty-files`) подтверждают концентрические углы `13/7`, строки `24 px`, двухрежимное выравнивание и отсутствие каких-либо изменений в `liquid_glass.rs`, `sdf.rs`, `dock.rs` и `quick_settings.rs`;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и все 256 тестов workspace проходят без ошибок.
- **Tests:** Все 256 unit-тестов workspace (`4` `rusty-animation`, `171` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `43` `rusty-services`, `4` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-108 — Разделение статусов `Ethernet` и `Wi-Fi`, всплывающее окно `Wi-Fi` сетей и редизайн страниц `Wi-Fi` и `Network` в `rusty-settings`

- **Status:** Done
- **Goal:** Чётко разделить состояния проводного (`Ethernet`) и беспроводного (`Wi-Fi`) подключений в `rusty-services`, `TopBar` и `Control Center`, добавить всплывающее окно `WifiPopover` (в стиле Golden Gate `r = 13` / `r_pill = 7`) по клику на иконку сети и переработать страницы `Wi-Fi` и `Network` в `rusty-settings` по эталонам `27-Golden-Gate-Settings-Wi-Fi.png` и `27-Golden-Gate-Settings-Network.png`.
- **Dependencies:** `RUSTYOS-106`, `RUSTYOS-107`.
- **Scope:**
  - `crates/rusty-services/src/{connectivity.rs, lib.rs}`: добавлена структура `WifiAccessPoint`, раздельные поля `wired_connected`, `wired_interface`, `wired_label`, `wifi_connected`, `wifi_networks` в `NetworkStatus`, определение обоих интерфейсов в `query_network_via_dbus()` / `query_status()`, парсер `parse_nmcli_wifi_list` и методы `NetworkService::list_wifi_networks`, `NetworkService::connect_wifi`;
  - `crates/rusty-compositor/src/surfaces/{mod.rs, glyphs.rs, tests.rs}`: добавлен SDF-глиф `UiGlyphKind::Ethernet` (`sample_ethernet_glyph_uv`) и тест `ethernet_glyph_rasterizes_cleanly`;
  - `crates/rusty-compositor/src/shell_render/top_bar/{panel.rs, status.rs, chrome_shapes.rs, mod.rs}` и `quick_settings.rs`: отображение `UiGlyphKind::Ethernet` в `TopBar` при `wired_connected && !wifi_connected`, обработка `TopBarHit::Wifi` (`check_top_bar_wifi_click_for_options`), изоляция подписи капсулы `Wi-Fi` в `Control Center` (`wifi_connected`, `wifi_ssid`, `is_wifi_status_label`, `concise_wifi_subtitle_for_state`) без изменений геометрии `Control Center`, `Dock`, `liquid_glass.rs` и `sdf.rs`;
  - `crates/rusty-compositor/src/shell_render/{wifi_popover.rs, overlay_shapes.rs, text_pass.rs, mod.rs, tests.rs}` + `telemetry.rs`, `state/*`, `input/*`, `context_menu/mod.rs`, `frame_composer.rs`: реализован `WifiPopover` (`ShellOverlayView::Wifi`, `MENU_POPOVER_RADIUS = 13`, `HOVER_PILL_RADIUS = 7` при инсете `5 px`), переключатель `Wi-Fi`, строка `Ethernet (<iface>) · Connected`, `Known Networks` / `Other Networks`, подключение (`TelemetryCmd::ConnectWifi`, `SystemMetricsHandle::connect_wifi`), `Wi-Fi Settings...`, `KeyIntercept::WifiPopoverClose` и правый клик `QuickSettingsHit::ToggleWifi`;
  - `apps/rusty-settings/src/{state.rs, layout.rs, render.rs}`: раздельные страницы `SettingsPage::Wifi` (`WifiHeaderSwitch`, текущая сеть, `Known Networks`, `Other Networks`, `Refresh`) и `SettingsPage::Network` (карточки сервисов `Wi-Fi`, `Ethernet`, `Firewall` и `Other Services`);
  - `docs/adr/0059-network-ethernet-wifi-popover-and-settings.md`, `DESIGN.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-072`).
- **Acceptance Criteria:**
  - Живые `--winit` прогоны (Wayland/X11 smoke и `Light`/`Dark` снимки `Control Center`, `WifiPopover`, `Settings > Network` и `Settings > Wi-Fi`) подтверждают раздельное отображение `Ethernet` и `Wi-Fi` (включая одновременную работу `eno1` и `wlan0`) и отсутствие изменений в `liquid_glass.rs`, `sdf.rs`, `dock.rs` и геометрии `Control Center`;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и все 260 тестов workspace проходят без ошибок.
- **Tests:** Все 260 unit-тестов workspace (`4` `rusty-animation`, `174` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `43` `rusty-services`, `5` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-109 — `Notification Center`, разделение триггеров `Control Center` / `Clock` и редизайн вкладки `Notifications` в `rusty-settings`

- **Status:** Done
- **Goal:** Отделить клик по часам от триггера `Control Center` в `TopBar`, реализовать правую панель `Notification Center` (карточки уведомлений из персистентной истории, заголовок с очисткой, виджет `Weather` и интерактивный список `Reminders`, переход `Edit Widgets`) и обновить вкладку `Notifications` в `rusty-settings` по эталонам `27-Golden-Gate-Notification-Center.png` и `27-Golden-Gate-Settings-Notifications.png` без изменений `liquid_glass.rs`, `Dock` и `Control Center`.
- **Dependencies:** `RUSTYOS-107`, `RUSTYOS-108`.
- **Scope:**
  - `crates/rusty-compositor/src/config.rs` и `frame_composer.rs`: добавлены `NotificationPreviewsMode` (`Always`, `WhenUnlocked`, `Never`), поля `notifications_allow`, `notifications_show_previews`, `notifications_when_sleeping`, `notifications_when_locked`, `notifications_mirroring_off` в `UserConfig`, `apply_notifications_kv` / `append_notifications_toml` и применение политик скрытия уведомлений;
  - `crates/rusty-compositor/src/surfaces/{mod.rs, glyphs.rs, tests.rs}`: добавлены SDF-глифы `UiGlyphKind::CloudSun` (`sample_cloud_sun_glyph_uv`) и `UiGlyphKind::Checklist` (`sample_checklist_glyph_uv`);
  - `crates/rusty-compositor/src/shell_render/top_bar/{panel.rs, status.rs, chrome_shapes.rs, notifications.rs, mod.rs}`: разделены зоны `status_hit_x`/`status_hit_w` и `check_top_bar_clock_click_for_options`, добавлен флаг `TopBarTelemetry::notification_center_open` с подсветкой-пилюлей позади часов и убран устаревший синий бейдж `N:k` из `TopBar` (`notif_w = 0`);
  - `crates/rusty-compositor/src/shell_render/{notification_center.rs, overlay_shapes.rs, text_pass.rs, mod.rs, tests.rs}` + `state/*`, `input/*`, `context_menu/mod.rs`: добавлены `ShellOverlayView::NotificationCenter`, `NotificationCenterRenderState`, `NotificationCenterHit`, персистентная история `notification_history` (сохраняющаяся после истечения TTL тоста), ведущая карточка + заголовок с `ClearAll` + стопка карточек, виджет `Weather` (`r = 22`) с почасовым прогнозом, интерактивный `Reminders` (`notification_center_tasks`), `Edit Widgets` / `ClosePanel`, `KeyIntercept::NotificationCenterClose` (`Escape`) и взаимное закрытие оверлеев при нетронутых `liquid_glass.rs`, `sdf.rs`, `dock.rs` и `quick_settings.rs`;
  - `apps/rusty-settings/src/{state.rs, layout.rs, render.rs}`: акцент `#FF3B30`, страница `SettingsPage::Notifications` с карточкой заголовка, политиками `Notification Center`, перечнем `Application Notifications` (5 приложений с векторными скворклами), `Menu Bar & Updates` и записью в `[notifications]` `config.toml`;
  - `docs/adr/0060-notification-center-and-settings.md`, `DESIGN.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-073`).
- **Acceptance Criteria:**
  - Живые `--winit` прогоны (Wayland/X11 smoke и `Light`/`Dark` снимки `Notification Center`, раздельного открытия `Control Center` и `Settings > Notifications`) подтверждают раздельное поведение триггеров `Clock` / `Control Center`, переключение задач в `Reminders`, сохранение уведомлений в истории и отсутствие каких-либо правок в `liquid_glass.rs`, `sdf.rs`, `dock.rs` и `quick_settings.rs`;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и все 265 тестов workspace проходят без ошибок.
- **Tests:** Все 265 unit-тестов workspace (`4` `rusty-animation`, `178` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `43` `rusty-services`, `6` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-110 — Изолированный стиль `GlassPanelStyle::NotificationCard`, унификация тостов и `Notification Center` и палитра Golden Gate в `icons.rs`

- **Status:** Done
- **Goal:** Привести карточки уведомлений (всплывающие тосты и `Notification Center`) к эталону `27-Golden-Gate-Notification-Center.png` без цветовых пятен фона и грубой кромки, унифицировать геометрию и многострочную типографику без `"now"` и убрать legacy purple в системных иконках, не затрагивая `Dock`, `ControlModule` и `Popover`.
- **Dependencies:** `RUSTYOS-109`.
- **Scope:**
  - `crates/rusty-compositor/src/surfaces/{mod.rs, liquid_glass.rs, sdf.rs, tests.rs}`: добавлен вариант `GlassPanelStyle::NotificationCard` (матовое тонирование `0.56 + 0.24*g` Light / `0.62 + 0.22*g` Dark, `refract = 3.0`, `rim = 0.22`, `sat = 1.0`, `chroma = 0.0` и CPU-fallback в `sdf.rs`) при полностью неизменных `Dock`, `ControlModule` и `Popover` и добавлен тест `notification_card_glass_rasterizes_cleanly`;
  - `crates/rusty-compositor/src/icons.rs`: замена фиолетового цвета в `BuiltinAppIcon::RustySystem` (`paint_rusty_system_glyph`), пресете `WindowFallback` и мозаике `Overview` на синюю палитру Golden Gate (`#007AFF`), добавлена проверка синего канала в `resolves_adwaita_legacy_places_and_generates_distinct_squircle_icons`;
  - `crates/rusty-compositor/src/shell_render/{top_bar/notifications.rs, notification_center.rs, overlay_shapes.rs, text_pass.rs, tests.rs}`: вынесены общие константы (`NOTIFICATION_CARD_WIDTH = 344`, `NOTIFICATION_CARD_HEIGHT = 68`, `NOTIFICATION_CARD_RADIUS = 22`, `NOTIFICATION_ICON_OFFSET_X = 16`, `NOTIFICATION_TEXT_OFFSET_X = 66`) и хелперы `push_notification_card_chrome`, `wrap_notification_lines`, `push_notification_card_text_lines` (жирный заголовок, до 2 контрастных строк тела, без `"now"`), добавлен unit-тест `notification_card_line_wrapping_handles_multiline_and_single_line`;
  - `docs/adr/0061-notification-card-material-and-typography.md`, `DESIGN.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-074`).
- **Acceptance Criteria:**
  - Живые `--winit` прогоны (Wayland/X11 smoke и `Light`/`Dark` снимки тостов, `Notification Center`, `Control Center` и `Dock`) подтверждают матовую однородность `NotificationCard`, отсутствие фиолетовой fallback-иконки, единый стиль тостов/карточек и неизменность `Dock`, `Control Center` и меню `Popover`;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и все 267 тестов workspace проходят без ошибок.
- **Tests:** Все 267 unit-тестов workspace (`4` `rusty-animation`, `180` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `43` `rusty-services`, `6` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-111 — Устранение «лесенки» на иконках и глифах (`16-tap R2`, `2x HiDPI`-буферы, `Lanczos-3` PNG и `RGSS` в приложениях)

- **Status:** Done
- **Goal:** Устранить зубчатость краёв (aliasing) на процедурных скворклах и UI-глифах оболочки (включая magnification в `Dock`), внешних `.png`/`SNI`-иконках и векторных иконках `rusty-settings` / `rusty-updater` без добавления новых внешних зависимостей.
- **Dependencies:** `RUSTYOS-110`.
- **Scope:**
  - `crates/rusty-compositor/src/aa.rs`, `lib.rs`, `icons.rs`, `surfaces/{mod.rs, glyphs.rs, sdf.rs}`: добавлены `R2_TAPS_16` и `LinearAccum` (сглаживание в линейном свете без sRGB-каймы), перевод `sample_builtin_squircle_pixel`, `sample_ui_glyph_pixel_rect`, `sample_fallback_avatar_pixel` и `sample_traffic_light_pixel` на `R2_TAPS_16`, `GLYPH_BUFFER_SCALE = 2` (`scale = 2` в `rasterize_builtin_squircle_buffer`, `rasterize_ui_glyph_buffer_rect` и `load_png_as_bgra_scaled`) + unit-тесты `r2_taps_are_distinct_in_both_axes` и `linear_accum_preserves_constant_color_and_coverage`;
  - `crates/rusty-services/src/image_decode.rs`: добавлен сепарабельный `downscale_lanczos_linear` (`lanczos_axis_weights`, sRGB ↔ linear LUT) в `scale_rgba_rect_to_premul_bgra` для `.png` и `argb32_be_to_premul_bgra` + unit-тест `lanczos_linear_downscale_preserves_color_alpha_and_softens_edges`;
  - `apps/rusty-settings/src/render.rs`, `apps/rusty-updater/src/render.rs`: добавлен 4-tap rotated grid SSAA (`aa_cov8`) для скворклов, кругов, сегментов, `draw_wifi_arc`, `draw_updater_emblem` и `draw_refresh_icon`;
  - `docs/adr/0062-icon-antialiasing-and-hidpi-buffers.md`, `DESIGN.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-075`).
- **Acceptance Criteria:**
  - Живой `--winit` прогон (Wayland/X11 smoke и `Light`/`Dark` снимки `TopBar`, `Dock`, увеличенной при наведении иконки `Dock`, уведомлений, `400%` зум-срезов, `rusty-settings` и `rusty-updater`) подтверждает плавные края и отсутствие `EGL_BAD_SURFACE` / shader ошибок;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и все 270 тестов workspace проходят без ошибок.
- **Tests:** Все 270 unit-тестов workspace (`4` `rusty-animation`, `182` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `44` `rusty-services`, `6` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-112 — Ограничение минимального размера при resize без отскока, исправление фона/шейдера минимизации и плавность `rusty-settings`

- **Status:** Done
- **Goal:** Устранить отскок/съезжание при интерактивном уменьшении окна до минимального размера, исправить артефакты фона/тени/стекла и premultiplied-шейдера при минимизации (а также тень до первого `commit`) и повысить отзывчивость и плавность `rusty-settings`.
- **Dependencies:** `RUSTYOS-111`.
- **Scope:**
  - `crates/rusty-wm/src/lib.rs`, `crates/rusty-compositor/src/{state/wm_windows.rs, input/pointer.rs, input/tests.rs}`: добавлены `WindowManager::resize_window_with_min`, `floating_min_bounds`, `effective_window_min_size` и `compute_resized_rect_with_min` для жёсткой фиксации минимальных размеров клиента без смещения противоположной границы и отскока;
  - `crates/rusty-compositor/src/{frame_composer.rs, surfaces/mod.rs, surfaces/liquid_glass.rs, surfaces/window_minimize.rs}`: выделен `push_window_elements_with_options` (минимизационный снапшот с `include_shadow = false`, экранным `glass_backdrop_offset` через `make_glass_element_with_backdrop_loc` / `glass_panel_render_element_at` после `prepare_liquid_glass_backdrop`, пропуск пустого хрома до первого коммита клиента через `should_skip_uncommitted_bound_window`), в `WINDOW_MINIMIZE_FRAG` исправлено premultiplied-умножение (`color * mask`), добавлены 4-tap `sample_filtered`, `u_bounds_px` и плавный `smoothstep(0.76, 0.99, t)`;
  - `apps/rusty-settings/src/{state.rs, wayland.rs, render.rs}`: вынесены `ServiceCommand` / `ServiceSnapshot` / `execute_coalesced_service_batch` в фоновый worker с `calloop::ping`, добавлены субпиксельный/плавный скролл (`scroll_smooth_by`, `step_scroll_animation`), троттлинг `poll_external_config_if_due` (`200 ms`), точечный `wl_surface::damage_buffer` (`DamageScope`), быстрые сканлайны `blend_span_opaque` / `rounded_interior_span`, отсечение невидимых строк по `clip` и кэш premultiplied BGRA-спрайтов `SPRITE_CACHE`;
  - `docs/adr/0063-resize-min-bounds-minimize-cleanup-and-settings-smoothness.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-076`).
- **Acceptance Criteria:**
  - Перетаскивание любой границы resize `rusty-settings` ниже `640×500` жёстко фиксирует размер без смещения противоположного края и баунса;
  - Минимизация и восстановление `Light`/`Dark` окон (включая `rusty-files` с `Liquid Glass`) проходят без тёмных клиньев тени, плоского серого прямоугольника стекла и ложной рамки до первого кадра;
  - Прогон `--winit` (X11 и вложенный Wayland без EGL/shader ошибок), визуальный осмотр снимков, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` и все 275 unit-тестов workspace проходят без ошибок.
- **Tests:** 275 unit-тестов workspace (`4` `rusty-animation`, `183` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-113 — GPU-ускоренный Wayland CSD-терминал `apps/rusty-terminal` (macOS Tahoe + Alacritty GLES2)

- **Status:** Done
- **Goal:** Создать собственный первый Wayland CSD-терминал `apps/rusty-terminal` (`app_id = "org.rustyos.Terminal"`) в стиле macOS Tahoe (`Clear Light` / `Clear Dark` через `rustyos_liquid_glass_v1` + непрозрачный fallback) и архитектуре Alacritty GLES2 с DMA-BUF zero-copy рендерингом и без добавления новых прямых внешних зависимостей.
- **Dependencies:** `RUSTYOS-112`.
- **Scope:**
  - `Cargo.toml`, `apps/rusty-terminal/{Cargo.toml, build.rs, src/main.rs}`: добавлен крейт `rusty-terminal` на уже используемых в графе крейтах (`wayland-egl`, `gl_generator`, `smithay-client-toolkit` с фичей `xkbcommon` -> транзитивные `xkbcommon 0.7.0`/`memmap2 0.8.0` в `Cargo.lock`, `rustix`, `fontdue`, `rusty-animation`, `wayland-scanner`) с CLI (`--no-gpu`, `--working-directory`, `--title`, `-e`/`--`);
  - `apps/rusty-terminal/src/{gpu_egl.rs, renderer_shm.rs, font.rs, theme.rs, liquid_glass.rs}`: Wayland-EGL + OpenGL ES 2.0 2-pass batched GPU-рендерер (`RECT_PROG` SDF скруглённых панелей/рамок/кругов/ячеек + `TEXT_PROG` с динамическим `1024×1024` `ShelfPacker` атласом и `glScissor`-отсечением сетки), автоматический `wl_shm` CPU fallback `ShmFallbackRenderer`, системные моноширинные шрифты + встроенный `Inter` + процедурные бесшовные `Box Drawing` / `Block Elements` / `Powerline` / UI-иконки заголовка, палитры Tahoe `Clear Dark` / `Clear Light` / `Opaque` с живым `ConfigWatcher` и `rustyos_liquid_glass_v1` (`GlassStyle::Sidebar`, `r = 14`);
  - `apps/rusty-terminal/src/{vt.rs, pty.rs, state.rs, layout.rs, wayland.rs}`: потоковый UTF-8/VT100/`xterm-256color`/24-bit `TrueColor` движок (`DECSTBM`, `DECSCUSR` курсор `Bar`/`Block`/`Underline` с `2px` лучом по умолчанию, alternate screen `?47`/`?1047`/`?1049`, `DECCKM`, `?2004` bracketed paste, `?1004` focus reporting, SGR mouse `?1000`/`?1002`/`?1006`, `OSC 0/2/7`, `DSR`/`DA1`, `10 000`-строчный scrollback без ложного скролла при ресайзе/старте), отложенный старт PTY до первого `configure`, неблокирующий `rustix` PTY + фоновый `poll` поток + `calloop::ping`, вкладки (`Ctrl+Shift+T`/`W`), плавный анимированный скролл (`rusty_animation::AnimatedValue`), поиск по истории (`Ctrl+Shift+F`), масштабирование шрифта (`Ctrl++`/`-`/`0`), интерактивное перетаскивание (`xdg_toplevel.move` с корректной инициализацией `wl_seat` + double-click maximize), ресайз по краям, выделение (drag/word/line) и Wayland clipboard (`DataDeviceManagerState`);
  - `crates/rusty-compositor/src/{launcher.rs, shell_render/dock.rs, state/shell_actions.rs, context_menu/actions.rs, input/pointer.rs}`, `apps/rusty-shell/src/lib.rs`: `rusty-terminal` подключён основным терминалом в `Dock`, `Launcher`, контекстном меню рабочего стола (`launch_terminal_in_dir` с `--working-directory`) и `rusty-shell` с сохранением fallback на `foot`/`kitty`/`alacritty`, а в `pointer.rs` выставляется `shell_redraw_needed` при интерактивном drag/resize;
  - `docs/adr/0064-rusty-terminal-architecture.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-077`).
- **Acceptance Criteria:**
  - Живой `--winit` прогон на вложенном Wayland и X11 (`Intel Mesa GLES`, `zwp_linux_dmabuf_v1`) подтверждает инициализацию `Wayland-EGL GLES2 GPU renderer`, отсутствие `EGL_BAD_SURFACE` / shader ошибок, корректное перетаскивание (`xdg_toplevel.move`) и ресайз (`xdg_toplevel.resize`), отсутствие ложного скроллбара на старте, тонкий `2px` bar-курсор, переключение `Clear Dark` / `Clear Light` / `Opaque` / вкладок / панели поиска и визуальный паритет `--no-gpu` `wl_shm` fallback;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins` и все 290 unit-тестов workspace проходят без ошибок.
- **Tests:** 290 unit-тестов workspace (`4` `rusty-animation`, `184` `rusty-compositor`, `15` `rusty-files`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-114 — Переработка иконок macOS 27 (Dock, папок Desktop/Home) с живыми акцентами `[ui].accent_index` и браузер Firefox по умолчанию

- **Status:** Done
- **Goal:** Обновить процедурные иконки `Dock`, рабочего стола (`BuiltinAppIcon::Folder`) и папок `apps/rusty-files` в стиле `macOS 27 Golden Gate` (`Light` и `Dark`) с живой реакцией на системный `[ui].accent_index` (`0..=6`), объединить слот браузера в `Dock` с `Firefox` по умолчанию (`framed_xdg_squircle_icon_element`) и добавить `firefox` в `packaging/arch/PKGBUILD`.
- **Dependencies:** `RUSTYOS-113`.
- **Scope:**
  - `crates/rusty-services/src/file_manager/types.rs`, `apps/rusty-settings/src/state.rs`: вынесены общие константы палитры (`RUSTYOS_ACCENT_COUNT`, `RUSTYOS_ACCENT_SWATCHES`, `clamp_accent_index`, `accent_rgba_for_index`, `detect_system_accent_from_toml`, `UserFileLocations::detect_system_accent`, `find_ui_toml_value`), а `choose_accent` в `rusty-settings` вызывает `notify_compositor_reload()` для мгновенного обновления композитора через IPC `v1|RELOAD_CONFIG`;
  - `crates/rusty-compositor/src/{config.rs, icon_art.rs, icons.rs, frame_composer.rs, shell_render/tokens.rs, shell_render/mod.rs, shell_render/desktop_canvas.rs, shell_render/chrome_shapes.rs, state/shell_actions.rs, state/tests.rs}`: добавлен `[ui].accent_index` (`UserConfig::ui_accent_index`, `set_active_accent_index`/`active_accent_index`, `ThemePalette::current_for_styled`), создан модуль `icon_art.rs` с 16-tap R2 процедурным рендерингом иконок `macOS 27` (`Folder`, `Downloads`, `Trash`, `TrashFull`, `Files`, `Overview`, `Terminal`, `Browser`, `Notes`, `Settings`, `Updater`, `RustySystem`, `WindowFallback`) в `Light`/`Dark` и поддержкой `[ui].accent_index`, добавлены `BuiltinAppIcon::Folder` для папок рабочего стола, `composite_xdg_in_squircle`/`framed_xdg_squircle_icon_element` и приоритет `128x128`/`256x256` в `find_icon_png_in_roots`, а `build_dock_tiles` объединяет `"firefox"` со слотом `"browser"` (`"Firefox"`) без дублирования плиток;
  - `apps/rusty-files/src/{state/mod.rs, icons.rs, render/mod.rs, render/popup.rs, wayland.rs}`: живой парсинг и hot-reload `[ui].accent_index` (`poll_system_theme`, `palette_for_with_accent`, `IconSpriteKey::{SmallFolder, LargeFolder}`), обновление малых и крупных папок (`draw_large_golden_gate_folder` с гладким S-уступом и двойным тисненым глифом категории) и перенос акцента во всплывающие меню (`render_popup_surface_pixels_with_style`, `draw_popup_surface`);
  - `packaging/arch/PKGBUILD`: добавлен пакет `'firefox'` в `depends`;
  - `DECISIONS.md` (`D-078`).
- **Acceptance Criteria:**
  - Живой `--winit` прогон на вложенном Wayland и X11 (`Light Blue`, `rusty-files`, `Dark Blue`, живое переключение на `Coral Red` через клик в `rusty-settings` с `RELOAD_CONFIG` IPC и закрытием настроек) подтверждает одновременное обновление папок Desktop, `rusty-files`, иконок `Dock` (включая `Firefox` в сквиркле, `Finder`, `Apps`, `Terminal`, `Notes`, `Settings`, `Downloads`, `Trash`) без `EGL_BAD_SURFACE` и shader-ошибок;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins` и все 293 unit-теста workspace проходят без ошибок.
- **Tests:** 293 unit-теста workspace (`4` `rusty-animation`, `186` `rusty-compositor`, `16` `rusty-files`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-115 — Инсталлятор `rusty-installer`, OOBE Setup Assistant, профиль `archiso` Live ISO и доупаковка системных приложений

- **Status:** Done
- **Goal:** Доупаковать `rusty-settings` и `rusty-installer` в Arch `PKGBUILD`, `scripts/install.sh` и сессию `bin/rustyos-session`, создать нативное Wayland CSD-приложение `apps/rusty-installer` в стиле `macOS Tahoe / Golden Gate` (мастер установки на диск + первый запуск OOBE Setup Assistant), безопасный root/Polkit backend `scripts/rustyos-install.sh` и воспроизводимый профиль загрузочного Live ISO `packaging/archiso/` с автологином `greetd`.
- **Dependencies:** `RUSTYOS-114`.
- **Scope:**
  - `Cargo.toml`, `apps/rusty-installer/{Cargo.toml, src/{main.rs, disks.rs, state.rs, layout.rs, render.rs, liquid_glass.rs, wayland.rs}}`: добавлен крейт `rusty-installer` (`org.rustyos.Installer`) без новых зависимостей в `Cargo.lock` с режимами `--install`/`--oobe`/`--live`/`--demo`/`--dry-run`/`--light`/`--dark`/`--accent`/`--no-glass`/`--install-script`, обнаружением дисков (`lsblk --json` с блокировкой смонтированных дисков), анимированным прогрессом (`AnimatedValue`), живой сменой `Light`/`Dark`/`Auto` + `[ui].accent_index` (`RELOAD_CONFIG`) и `rustyos_liquid_glass_v1`;
  - `scripts/rustyos-install.sh`, `packaging/polkit/org.rustyos.installer.policy`: безопасный установщик Arch + RustyOS (`--list-disks-json`, `--rollback-help`, `--dry-run`, `--confirm-erase` + `--password-stdin`, потоковый вывод `STEP:<pct>:<stage>:<msg>`, `parted`/`mkfs`/`pacstrap`/`greetd`/`systemd-boot`/`GRUB`/`.oobe-pending`) и Polkit-правило для `/usr/lib/rustyos/rustyos-install`;
  - `packaging/applications/org.rustyos.{settings,installer}.desktop`, `packaging/arch/PKGBUILD`, `scripts/install.sh`, `bin/rustyos-session`, `crates/rusty-compositor/src/launcher.rs`, `.github/workflows/ci.yml`: полная упаковка `rusty-settings` и `rusty-installer`, `--doctor` и `.oobe-pending` автозапуск в `rustyos-session` (отключаемый `RUSTYOS_SKIP_OOBE=1`), приоритет в `Launcher` и ручной job `arch-live-iso` в CI;
  - `packaging/archiso/{profiledef.sh, packages.x86_64, pacman.conf, build-iso.sh, README.md, efiboot/, grub/, syslinux/, airootfs/}`: профиль загрузочного Live ISO (`liveuser`, `greetd` -> `rustyos-session --drm`, Polkit `49-rustyos-live.rules`, ярлык установки на рабочем столе);
  - `docs/adr/0065-archiso-installer-and-oobe.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-079`).
- **Acceptance Criteria:**
  - Живой `--winit` прогон на вложенном Wayland (`--max-frames 90`, ноль `EGL_BAD_SURFACE` / shader-ошибок / паник) и X11 (`Welcome` -> `SelectDisk` -> `Account & Appearance` -> `Installing` 100% симуляция + `OOBE Appearance` живое переключение на `Dark` и `Coral Red` с записью `config.toml` и перекраской композитора по `RELOAD_CONFIG`);
  - Проверка синтаксиса скриптов (`bash -n`, `sh -n`), `--dry-run` `STEP:` вывода, `desktop-file-validate`, `xmllint`, YAML CI, а также `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins` и все 300 unit-тестов workspace проходят без ошибок.
- **Tests:** 300 unit-тестов workspace (`4` `rusty-animation`, `186` `rusty-compositor`, `16` `rusty-files`, `7` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`). Ограничение локальной среды: отсутствие `mkarchiso` и `qemu-system-x86_64` — полная сборка Live ISO проверяется в `workflow_dispatch` job `arch-live-iso` либо локально на Arch через `packaging/archiso/build-iso.sh`.
- **Risks:** Нет.

### RUSTYOS-116 — Безопасный Dual-Boot режим в `rusty-installer`, `scripts/rustyos-install.sh` и `packaging/archiso`

- **Status:** Done
- **Goal:** Обеспечить безопасную установку RustyOS на один физический диск рядом с существующими ОС (EndeavourOS/Linux/Windows) в заранее выделенный root-раздел или неразмеченное свободное место (`Unallocated`) без `sgdisk --zap-all`, с бережным отношением к EFI и поддержкой `os-prober`.
- **Dependencies:** `RUSTYOS-115`.
- **Scope:**
  - `apps/rusty-installer/src/disks.rs`: расширен разбор `lsblk -Jpb` (`children`, `START`, `LOG-SEC`, `FSTYPE`, `PARTTYPE`, `LABEL`, `PTTYPE`), добавлены `DiskPartitionInfo`, `UnallocatedRegionInfo`, `compute_unallocated_gaps` (работает без прав root) и `DualBootTargetInfo::collect_from_disks` (при малом ESP `< 260 MiB` для `UnallocatedGap` планируется отдельный `512 MiB` EFI; при ESP `>= 260 MiB` — переиспользование без форматирования);
  - `apps/rusty-installer/src/{state.rs, layout.rs, render.rs, wayland.rs}`: добавлен `DiskInstallMode::{WholeDisk, DualBoot}` (`Erase Entire Disk` vs `Dual-Boot (Partition / Free)`), клавиатурное переключение `Left`/`Right`, единый `draw_storage_card`, `summary_lines` для Dual-Boot и формирование аргументов `--install-mode dual-boot` / `--root-part` / `--free-space` / `--efi-part` / `--confirm-dualboot`;
  - `scripts/rustyos-install.sh`, `packaging/archiso/packages.x86_64`: реализован `--install-mode dual-boot` (без `sgdisk --zap-all`, точное создание GPT-разделов в свободном диапазоне через `sgdisk` + поиск `/sys/block/.../start`, проверка `>= 120 MiB` на shared EFI перед записью, запрет `mkfs.fat` на существующем ESP, включение `GRUB_DISABLE_OS_PROBER=false`, сохранение существующего `loader.conf` в `systemd-boot`) и добавлен `os-prober`;
  - `docs/adr/0066-installer-dualboot-partitioning.md`, `ARCHITECTURE.md`, `DECISIONS.md` (`D-080`).
- **Acceptance Criteria:**
  - `--dry-run` подтверждает безопасный план как для `--root-part` + `--efi-part`, так и для `--free-space` (без `sgdisk --zap-all` и без форматирования существующего ESP);
  - Живой `--winit` прогон на Nested Wayland (`--max-frames 90`, без EGL/shader/panic ошибок) и X11 (`winit_116_dualboot_demo.png`, `winit_116_dualboot_review.png`, а также read-only скан хостового NVMe `winit_116_dualboot_host_readonly.png` с блокировкой смонтированного `endeavouros` и подхватом `/dev/nvme0n1p1` + `/dev/nvme0n1p3`) подтверждают работу Dual-Boot UI;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins`, `bash -n scripts/rustyos-install.sh` и все 302 unit-теста workspace проходят без ошибок.
- **Tests:** 302 unit-теста workspace (`4` `rusty-animation`, `186` `rusty-compositor`, `16` `rusty-files`, `9` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет.

### RUSTYOS-117 — Проверка интернета, выбор Wi-Fi в `--install` и диагностика ошибок `pacstrap` + Live ISO user/DRM-исправления

- **Status:** Done
- **Goal:** Обеспечить автоопределение интернета и подключение к Wi-Fi (с паролем) прямо внутри `rusty-installer --install`, раннюю preflight-проверку сети до разметки диска, читаемый хвост ошибок с кнопкой `Details` и стабильную автозагрузку Live ISO.
- **Dependencies:** `RUSTYOS-116`.
- **Scope:**
  - `crates/rusty-services/src/connectivity.rs`: добавлены `rescan_wifi_networks` (`nmcli --rescan yes`) и `connect_wifi_with_password` (переиспользование `connect_wifi`);
  - `apps/rusty-installer/src/{state.rs, layout.rs, render.rs, wayland.rs}`: добавлен шаг `InstallStep::Network` (`Internet & Wi-Fi`) с автоопределением Ethernet/Wi-Fi через `NetworkManager`, переключателем `Wi-Fi: On/Off`, сканированием, выбором SSID, полем `TextFieldId::WifiPassword` (включая `Enter`), проверкой `internet_ready` перед продолжением в `--live`, подписью `Install RustyOS` для Dual-Boot и корректным отображением хвоста ошибок (`format_installer_error_tail`, авто-раскрытие `Details`);
  - `scripts/rustyos-install.sh`: preflight-проверка сети (`check_network_preflight`) до разметки диска и запись лога в `/tmp/rustyos-install.log`;
  - `crates/rusty-compositor/src/drm_backend.rs`, `bin/rustyos-session`, `packaging/archiso/`: исправлен порядок хука (`99-rustyos-liveuser.hook` -> `rustyos-liveuser-setup`, создание `liveuser`/`greeter` после `20-systemd-sysusers.hook`, маска `systemd-firstboot.service`, `/etc/locale.conf`, `/etc/vconsole.conf`), перебор и ранжирование DRM GPU до `simpledrm`, привязка `EGLDisplay`, отказ без тихого `exit 0` на TTY.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `sh -n`, `bash -n`, `--dry-run`, `--drm-probe` и живые `--winit` прогоны Wayland + X11 с запуском `rusty-installer --install --demo` пройдены без ошибок.
- **Tests:** 307 unit-тестов workspace (`4` `rusty-animation`, `188` `rusty-compositor`, `16` `rusty-files`, `12` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** На хосте нет `mkarchiso` до установки `archiso`, поэтому финальный прогон новой Live ISO сборки выполняется локально после `sudo pacman -S archiso` или через `workflow_dispatch` `arch-live-iso`.

### RUSTYOS-118 — Оптимизация пакетов ISO/PKGBUILD, моноширинный шрифт терминала, keyring-фикс и опциональный `yay-bin`

- **Status:** Done (`RUSTYOS-118 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Убрать лишние пакеты (`adwaita-icon-theme`/`adwaita-cursors`, `archinstall`, обязательный `firefox`), обеспечить моноширинный шрифт `ttf-jetbrains-mono` в `rusty-terminal`, автоматизировать `pacman-key` + `archlinux-keyring` перед `pacstrap` и установить `yay-bin` из AUR в целевую систему с безопасным fallback.
- **Dependencies:** `RUSTYOS-117`.
- **Scope:**
  - `packaging/arch/PKGBUILD`: удалены `'adwaita-icon-theme'` и обязательный `'firefox'` (перенесён в `optdepends`), добавлен `'ttf-jetbrains-mono'`;
  - `packaging/archiso/packages.x86_64`: удалены `archinstall` и `firefox`, добавлены `ttf-jetbrains-mono`, `git`, `base-devel`;
  - `scripts/rustyos-install.sh`: проверка инструментов `pacman`/`pacman-key`, автоинициализация `pacman-key --init`, `pacman-key --populate archlinux` и `pacman -Sy --noconfirm --needed archlinux-keyring` до `pacstrap`, обновление состава пакетов (`ttf-jetbrains-mono git base-devel` вместо `firefox`), функция `install_optional_yay_bin` (непривилегированный `makepkg` под пользователем + `pacman -U` в `arch-chroot` с мягким fallback) и гарантированная запись секции `[rustyos]` в `/etc/pacman.conf`;
  - `.github/workflows/ci.yml`, `apps/rusty-installer/src/render.rs`: синхронизированы зависимости CI (`ttf-jetbrains-mono` вместо `adwaita-icon-theme` и `firefox`) и текст экрана `Internet & Wi-Fi`;
  - параллельно исправлены два локальных бага: deferred keyboard redraw в `drm_backend.rs`/`keyboard.rs` (уменьшает задержку ввода в Firefox) и общие `send_output_frame_callbacks` / `output_has_pending_frame_callbacks` в `wm_windows.rs` (включая `xdg_popup`, `layer_shell` и IME).
- **Acceptance Criteria:**
  - `bash -n packaging/arch/PKGBUILD scripts/rustyos-install.sh scripts/install.sh packaging/archiso/profiledef.sh packaging/archiso/build-iso.sh`, `sh -n packaging/archiso/airootfs/usr/local/bin/rustyos-liveuser-setup`, YAML-проверка `.github/workflows/ci.yml` и `--dry-run` (`dual-boot` и `whole-disk`) проходят без ошибок;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo build --workspace --bins`, `--drm-probe` и живые `--winit` прогоны (`Wayland` и `X11`) завершаются без ошибок.
- **Tests:** 309 unit-тестов workspace (`4` `rusty-animation`, `190` `rusty-compositor`, `16` `rusty-files`, `12` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Сборка `yay-bin` в целевой системе зависит от доступности `aur.archlinux.org` и GitHub во время установки; при сетевом сбое установка системы продолжается с предупреждением без фатальной ошибки.

### RUSTYOS-119 — Оптимизация release-профиля, hot path композитора и age-eviction кэшей (`RUSTYOS-119 Вариант 1`)

- **Status:** Done (`RUSTYOS-119 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Уменьшить размер и накладные расходы release-бинарника, убрать лишние ежекадровые аллокации/сортировки и FBO-переключения в композиторе при неизменных backdrop/telemetry/UI, а также ограничить рост GPU/RAM-кэшей (`TextCache`, `UiShapeCache`, `IconCache`) через age-based eviction.
- **Dependencies:** `RUSTYOS-118`.
- **Scope:**
  - `Cargo.toml`: добавлен `[profile.release]` (`lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`);
  - `crates/rusty-compositor/src/telemetry.rs`, `frame_timing.rs`, `frame_composer.rs`: перевод `SharedState::latest` на `Arc<SystemTelemetry>` (`snapshot_arc()` с нулевым клонированием на чтении кадра), ленивый `interval_p50_us()` вместо ежекадрового `frame_timing.summary()` (5 сортировок до 4096 элементов) при закрытом Control Center;
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`: `BackdropElemSig` (`Id`, `CommitCounter`, geometry, `src`, `transform`, `alpha`) и `backdrop_signature_matches` для быстрого пропуска `run_backdrop_blur_passes` и offscreen `scene_tex` FBO bind без лишних аллокаций;
  - `crates/rusty-compositor/src/text.rs`, `surfaces/mod.rs`, `icons.rs`: zero-alloc lookup по `&str` в `TextCache::buffers`, объединение `ensure_named_buffer` lookup в `IconCache` и age-based eviction по `frame_counter` для `TextCache`, `UiShapeCache` (`glyphs`, `glass_panels` без полного сброса Dock/TopBar, `avatars`) и `IconCache` (`buffers`, `shm_textures`).
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (включая 5 новых unit-тестов на `snapshot_arc`, `interval_p50_us`, `backdrop_signature_matches` и age-eviction) проходят без ошибок;
  - Живые `--winit` прогоны на `Wayland` и `X11` + `--drm-probe` проходят без `EGL_BAD_SURFACE` и shader-ошибок;
  - Замеры `--release` до/после (размер бинарника `23M -> 15M` и прогоны `120f`/`240f` `--bench-input`) зафиксированы в `PERFORMANCE.md`.
- **Tests:** 314 unit-тестов workspace (`4` `rusty-animation`, `195` `rusty-compositor`, `16` `rusty-files`, `12` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** В `nested-winit` замер `render_duration_us` включает фазовое ожидание `eglSwapBuffers` на хостовом `wl_surface.frame`; прогон на голом железе `--drm` выполняется пользователем отдельно.

### RUSTYOS-120 — Изоляция чистого CPU-времени композитора, оптимизация render list и адаптивная телеметрия

- **Status:** Done
- **Goal:** Изолировать и объективно измерить процессорное время композитора отдельно от блокирующего ожидания vblank хоста (`eglSwapBuffers`), исключить ежекадровые аллокации строк и обходы хэш-таблиц при неизменном рабочем столе, а также перевести опрос фоновой телеметрии на адаптивный интервал (3 с / 12 с).
- **Dependencies:** `RUSTYOS-119`.
- **Scope:**
  - `crates/rusty-compositor/src/frame_timing.rs`, `winit_backend.rs`, `drm_backend.rs`: добавлены `cpu_duration_us` в `FrameSample` и `cpu_p50_us`/`cpu_p95_us`/`cpu_p99_us`/`cpu_max_us` в `FrameTimingSummary`, разделена фиксация `cpu_done` (до `backend.submit()`) и `present_done`, обновлен лог-вывод и экспорт JSON (`--metrics-out`), подготовлен скрипт медианного замера `bench_runner.py` (5–10 прогонов);
  - `crates/rusty-compositor/src/frame_composer.rs`, `shell_render/dock.rs`, `surfaces/window_minimize.rs`, `surfaces/mod.rs`: `collect_dock_windows` форматирует `dimensions` (`format!("{}x{}")`) только при активном Spotlight Overview, `make_window_badge` переведен на zero-alloc итераторы слов, добавлены быстрые проверки `has_any_minimize_snapshots` и ранний выход из `prepare_minimize_snapshots_for_output` при отсутствии сворачивающихся окон;
  - `crates/rusty-compositor/src/telemetry.rs`, `state/shell_actions.rs`: введен адаптивный интервал опроса телеметрии (`POLL_INTERVAL_ACTIVE = 3s` при открытом Control Center / Wi-Fi popover и `POLL_INTERVAL_IDLE = 12s` в фоновом режиме) с атомарным флагом `active_ui`, немедленным пробуждением `cvar.notify_all()` при открытии меню или вызове команд и синхронизацией в `shell_actions.rs`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (включая unit-тесты `collector_tracks_cpu_duration_separately_from_submit` и `telemetry_handle_switches_active_ui_mode`) проходят без ошибок;
  - Живые `--winit` прогоны на Wayland и X11 (`--max-frames 60`, `--max-frames 30`) и `--drm-probe` проходят без `EGL_BAD_SURFACE`, шейдерных ошибок и паник;
  - Результаты многопрогонного медианного бенчмаркинга (чистое CPU-время `p50 ~0.55–0.68 ms`, что подтверждает отсутствие лагов на стороне композитора) зафиксированы в `PERFORMANCE.md`.
- **Tests:** 315 unit-тестов workspace (`4` `rusty-animation`, `196` `rusty-compositor`, `16` `rusty-files`, `12` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет; при закрытом Control Center фоновый поток телеметрии практически не потребляет ресурсы CPU и батареи, а при кликах на ползунки и переключатели немедленно просыпается без задержки.

### RUSTYOS-121 — Реактивный Calloop ping wake-up и keyboard focus pacing для плавного ввода в приложениях (`RUSTYOS-121 Вариант 1`)

- **Status:** Done (`RUSTYOS-121 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Устранить задержку обновления экрана при клавиатурном вводе в текстовых полях приложений (Firefox URL/search bar, редакторы) без движения мыши, возникавшую из-за 50-мс спячки idle-таймера и отсутствия реактивного пробуждения event loop при коммитах буферов клиентом.
- **Dependencies:** `RUSTYOS-120`.
- **Scope:**
  - `crates/rusty-compositor/src/state/mod.rs`, `system_bridges.rs`: добавлено поле `pub(crate) loop_waker: Option<Ping>` в `RustyCompositorState` и метод `pub(crate) fn wake_event_loop(&self)`, вызывающий мгновенный `ping()` для прерывания `epoll_wait`;
  - `crates/rusty-compositor/src/state/wayland_handlers/core.rs`: вызов `self.wake_event_loop()` при `wl_surface.commit()` (`client_surface_committed = true`), гарантирующий немедленный рендеринг обновленного буфера клиента на ближайшем vblank без ожидания таймеров;
  - `crates/rusty-compositor/src/drm_backend.rs`: регистрация источника `make_ping()` (`PingSource`) в `loop_handle`, перехват клавиатурного ввода в окне с фокусом (`is_keyboard && has_focus`) с немедленной пометкой dirty и пробуждением цикла, ограничение `next_timer_duration` частотой обновления дисплея (`fastest_refresh_interval()` ~16.6 мс при 60 Гц / 8.3 мс при 120 Гц вместо 50 мс) при наличии окна в фокусе, а также сброс задержки и пометка dirty при готовности `delayed_ready` в цикле диспетчеризации;
  - `crates/rusty-compositor/src/winit_backend.rs`: регистрация ping waker для единообразного мгновенного пробуждения в nested Winit режиме;
  - Unit-тесты: добавлены тесты `reactive_calloop_ping_waker_wakes_event_loop`, `focused_window_reports_active_focus_for_keyboard_pacing` и `keyboard_focus_pacing_interval_selection`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (317 тестов, включая 3 новых теста на waker и focus pacing) проходят без ошибок;
  - Живые `--winit` прогоны на Wayland и X11 (`--max-frames 60`) и `--drm-probe` проходят штатно с 0 пропущенными/опоздавшими кадрами;
  - Поведение при вводе с клавиатуры в сфокусированном окне (Firefox) непрерывно синхронизируется с vblank дисплея без необходимости шевелить мышью.
- **Tests:** 317 unit-тестов workspace (`4` `rusty-animation`, `199` `rusty-compositor`, `16` `rusty-files`, `12` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `14` `rusty-terminal`, `4` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет; при отсутствии сфокусированных окон и анимаций композитор сохраняет энергоэффективный idle-режим с таймаутом 50 мс.

### RUSTYOS-122 — Сквозная унификация оформления светофоров окон по эталону Settings (`RUSTYOS-122 Вариант 1`)

- **Status:** Done (`RUSTYOS-122 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Привести размеры, координаты, цветовую гамму и hover-эффекты светофоров управления окном (Close, Minimize, Maximize) во всех приложениях и серверных декорациях (SSD) к единому эталону по образцу `apps/rusty-settings`.
- **Dependencies:** `RUSTYOS-121`.
- **Scope:**
  - **Геометрия и цвета эталона:** диаметр 14 px (радиус 7.0 px), вертикальный центр `y = 20 px` (для CSD заголовков высотой 40 px) или `btn_y = y + 7 px` (для SSD высотой 28 px), центры кнопок по горизонтали `21.0`, `43.0`, `65.0` (отступ слева 14 px, зазор 8 px, шаг 22 px), цвета палитры RustyOS (`#EE5A6A` Close, `#F4B63F` Minimize, `#3CC68A` Maximize);
  - `apps/rusty-terminal`: цвета выровнены на фирменную палитру, реализована векторная отрисовка hover-глифов (`x`, `-`, `+`) для CPU SHM (`renderer_shm.rs`) и GPU EGL (`gpu_egl.rs`) путей, добавлен unit-тест `shm_renderer_draws_hover_glyphs_on_traffic_lights`;
  - `apps/rusty-files`: добавлены состояние `traffic_hovered` в `FilesAppState` и отслеживание курсора в `wayland.rs`, в `sidebar.rs` добавлена векторная отрисовка сглаженных глифов `x`, `-`, `+` через `stroke_line_aa`, добавлен unit-тест `traffic_lights_render_hover_glyphs`;
  - `apps/rusty-updater`: в `render.rs` добавлен векторный рендерер линий `Canvas::draw_segment`, устаревший растровый вывод символов заменен на точные векторные отрезки, добавлен unit-тест `traffic_lights_render_vector_hover_glyphs`;
  - `apps/rusty-installer`: в `layout.rs` и `render.rs` координаты кнопок выровнены на `(14, 13)`, `(36, 13)`, `(58, 13)` (размер 14×14 px), цвета приведены к эталону, реализован векторный hover с глифами, добавлен unit-тест `traffic_lights_render_vector_hover_glyphs`;
  - `crates/rusty-compositor/src/shell_render/ssd.rs`: диаметр кнопок SSD увеличен с 12 px до 14 px, позиции выровнены на `x + 14`, `x + 36`, `x + 58` (`btn_y = y + 7`), обновлены диапазоны hit-теста `check_window_frame_hit` (`(5.0..=23.0).contains(&rel_y)`, Close `12.0..=28.0`, Minimize `34.0..=50.0`, Maximize `56.0..=72.0`), обновлены тесты в `shell_render/tests.rs`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (321 unit-тест, включая 4 новых теста на hover-глифы) проходят без ошибок;
  - Живые `--winit` прогоны на Wayland и X11 (`--max-frames 60`) и `--drm-probe` проходят штатно;
  - Светофоры во всех окнах системы (Terminal, Files, Updater, Installer, Settings, SSD-окна) имеют строго одинаковые пропорции 14 px, одинаковый шаг и векторные микроглифы `x`, `-`, `+` при наведении.
- **Tests:** 321 unit-тест workspace (`4` `rusty-animation`, `199` `rusty-compositor`, `17` `rusty-files`, `13` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `10` `rusty-settings`, `5` `rusty-shell`, `15` `rusty-terminal`, `5` `rusty-updater`, `7` `rusty-wm`).
- **Risks:** Нет; векторная отрисовка глифов выполняется субпиксельно без аллокаций памяти и шрифтовых зависимостей.

### RUSTYOS-123 — Создание общего крейта crates/rusty-ui, интеграция Software Update в Settings и удаление rusty-updater (`RUSTYOS-123 Вариант 1`)

- **Status:** Done (`RUSTYOS-123 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Создать общий крейт `crates/rusty-ui` для переиспользования геометрии окон, светофоров, токенов тем и вспомогательного canvas; интегрировать полноценную страницу Software Update в `apps/rusty-settings`; полностью удалить отдельное приложение `apps/rusty-updater` и перенаправить все системные вызовы на Settings.
- **Dependencies:** `RUSTYOS-122`.
- **Scope:**
  - `crates/rusty-ui`: создан новый общий крейт в составе рабочего пространства (`[workspace.members]`, `[workspace.dependencies]`), содержащий модули `geometry` (`WINDOW_RADIUS = 20`, `TITLEBAR_HEIGHT = 40`, `SSD_TITLEBAR_HEIGHT = 28`, `TRAFFIC_LIGHT_DIAMETER = 14`, `TRAFFIC_LIGHT_CENTERS_X = [21.0, 43.0, 65.0]`, `Rect`, `Point`, `Size`), `theme` (`ThemeMode`, `TrafficLightKind`, `ThemePalette`), `traffic_lights` (`TrafficLightButtons`, `glyph_segments`, субпиксельные векторные глифы `x`, `-`, `+`), `canvas` (`UiCanvas` с субпиксельным антиалиасингом `blend_pixel`, `draw_circle_aa`, `stroke_line_aa`, `fast_rounded_rect`), `window_frame` (`WindowFrame`, `WindowHitTarget`, детектирование двойного клика максимизации) и 5 всесторонних unit-тестов;
  - `apps/rusty-settings`: добавлена страница `SettingsPage::SoftwareUpdate` (категория `System`, заголовок `Software Update`, CLI `--page updates`), фоновый чекер обновлений через `rusty_services::updates::check_all_pending_updates`, запуск транзакций через `UpdateTransactionHandle::start_system_upgrade`, карточка текущего статуса, полоса прогресса с лейблами фаз, список ожидающих пакетов и интеграция в sidebar/поиск/иконки;
  - `apps/rusty-updater`: полностью удален каталог `apps/rusty-updater` и его объявление из корневого `Cargo.toml`;
  - Системная интеграция: `bin/rustyos-session`, `scripts/install.sh`, `packaging/arch/PKGBUILD` очищены от устаревшего бинарника `rusty-updater`; `packaging/applications/org.rustyos.updater.desktop` переведён на `Exec=rusty-settings --page updates`;
  - Композитор: `crates/rusty-compositor/src/state/shell_actions.rs` (`open_software_updater_action`), `shell_render/updates_popover.rs` и `launcher.rs` переведены на запуск страницы `rusty-settings --page updates`;
  - Документация: `ADR-0067` (`docs/adr/0067-shared-ui-crate-and-settings-updates.md`), запись `D-081` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (317 unit-тестов) проходят без единой ошибки и предупреждения;
  - `cargo run -p rusty-compositor -- --drm-probe` завершается штатно;
  - Живой `--winit` прогон композитора стартует и работает стабильно;
  - `apps/rusty-updater` полностью отсутствует в исходниках и сборке;
  - `rusty-settings --page updates` открывает страницу Software Update с корректным рендерингом и управлением.
- **Tests:** 317 unit-тестов workspace (`4` `rusty-animation`, `199` `rusty-compositor`, `16` `rusty-files`, `12` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `11` `rusty-settings`, `5` `rusty-shell`, `15` `rusty-terminal`, `5` `rusty-ui`, `7` `rusty-wm`).
- **Risks:** Нет; фоновая проверка обновлений и транзакции используют проверенные низкоуровневые сервисы `rusty-services` без блокировки цикла Wayland.

### RUSTYOS-124 — Сквозное подключение crates/rusty-ui во все приложения и SSD, добавление модуля widgets (`RUSTYOS-124 Вариант 2`)

- **Status:** Done (`RUSTYOS-124 Вариант 2` — утверждённый объём выполнен)
- **Goal:** Реализовать модуль базовых виджетов в `crates/rusty-ui::widgets` (`ToggleSwitch`, `PushButton`, `GroupCard`, `SearchField`) и выполнить сквозную миграцию всех приложений рабочего пространства (`apps/rusty-files`, `apps/rusty-terminal`, `apps/rusty-installer`) и Server-Side Decorations (`crates/rusty-compositor`) на общий крейт `rusty-ui`.
- **Dependencies:** `RUSTYOS-123`.
- **Scope:**
  - `crates/rusty-ui/src/widgets.rs`: реализованы интерактивные структуры `WidgetState` (`Normal`, `Hover`, `Pressed`, `Disabled`), `ToggleSwitch` (38×20 px со сглаженным ползунком и AA-тенями), `PushButton` (`Primary`, `Secondary`, `Destructive`), `GroupCard` (скругление 12 px, фон `pal.card_bg`, разделители строк `draw_row_divider`), `SearchField` (поле поиска 8 px с векторной лупой, кольцом фокуса и кнопкой очистки `SearchFieldHit`);
  - `crates/rusty-ui/src/theme.rs`: добавлен дерайв `Hash` для `TrafficLightKind`;
  - `crates/rusty-ui/src/lib.rs`: экспорт модуля `widgets`, добавлены unit-тесты переключателей, кнопок, карточек и поиска;
  - `apps/rusty-files`: подключен `rusty-ui.workspace = true`, заменены жестко закодированные прямоугольники светофоров на `TrafficLightButtons::default_csd()`, отрисовка микроглифов переведена на `TrafficLightKind` и `glyph_segments()`;
  - `apps/rusty-terminal`: подключен `rusty-ui.workspace = true`, компоновка заголовка переведена на `TrafficLightButtons::default_csd()`, микроглифы в SHM- и EGL-пайплайнах используют `TrafficLightKind` и `glyph_segments()`;
  - `apps/rusty-installer`: подключен `rusty-ui.workspace = true`, компоновка заголовка переведена на `TrafficLightButtons::default_csd()`, микроглифы унифицированы;
  - `crates/rusty-compositor`: подключен `rusty-ui.workspace = true`, в `surfaces/mod.rs` реэкспортирован `TrafficLightKind` из `rusty_ui`, в `shell_render/ssd.rs` хит-тест `check_window_frame_hit` и генерация элементов SSD переведены на `TrafficLightButtons::default_ssd()`;
  - Документация: `ADR-0068` (`docs/adr/0068-unified-ui-widgets-and-app-migration.md`), запись `D-082` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (321 unit-тест) проходят без ошибок и предупреждений;
  - `cargo run -p rusty-compositor -- --drm-probe` завершается штатно;
  - Живой `--winit` прогон композитора стартует и отрисовывает рабочий стол стабильно;
  - Полное устранение дублирования логики геометрии светофоров и микроглифов во всех приложениях и серверных декорациях.
- **Tests:** 321 unit-тест workspace (`4` `rusty-animation`, `199` `rusty-compositor`, `17` `rusty-files`, `13` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `11` `rusty-settings`, `5` `rusty-shell`, `15` `rusty-terminal`, `6` `rusty-ui`, `7` `rusty-wm`).
- **Risks:** Нет; все расчеты координат светофоров строго идентичны эталонным размерам, проверенным в тестах.

### RUSTYOS-125 — Создание модулей menu и window_header в crates/rusty-ui, унификация стиля контекстных меню и оконных заголовков (`RUSTYOS-125 Вариант 2`)

- **Status:** Done (`RUSTYOS-125 Вариант 2` — утверждённый объём выполнен)
- **Goal:** Реализовать модули `crates/rusty-ui::menu` и `crates/rusty-ui::window_header` с эталонными метриками контекстных меню (высота строки 26 px, разделитель 12 px, внешний радиус 14 px, внутренний радиус плашки ховера 9 px, перекрытие каскадных подменю 2 px) и оконных заголовков (высота CSD 40 px, SSD 28 px, диалоги 42 px, радиус скругления 14 px, прозрачность размытия 0.85/0.92, параметры drag & double click), а также сквозное применение обновленного стиля контекстных меню в `rusty-compositor` и `rusty-files`.
- **Dependencies:** `RUSTYOS-123`, `RUSTYOS-124`.
- **Scope:**
  - `crates/rusty-ui/src/menu.rs`: реализованы константы меню (`ROW_HEIGHT = 26`, `SEPARATOR_HEIGHT = 12`, `MIN_MENU_WIDTH = 158`, `CONTENT_MARGIN = 5`, `VERTICAL_CONTENT_MARGIN = 5`, `MENU_RADIUS = 14`, `HOVER_PILL_RADIUS = 9`, `SUBMENU_OVERLAP = 2`), структуры `MenuItem<T>`, `MenuEntry<T>`, `MenuRowLayout`, `MenuLayout` (`compute`, `hit_test`, `clamp_to_screen`, `cascade_submenu_anchor`), а также функции отрисовки `draw_menu_background` и `draw_menu_row` на холсте `UiCanvas`;
  - `crates/rusty-ui/src/window_header.rs`: реализованы константы (`CSD_TITLEBAR_HEIGHT = 40`, `SSD_TITLEBAR_HEIGHT = 28`, `DIALOG_HEADER_HEIGHT = 42`, `WINDOW_RADIUS = 14`, `ACTIVE_WINDOW_BLUR_OPACITY = 0.85`, `INACTIVE_WINDOW_BLUR_OPACITY = 0.92`, `DOUBLE_CLICK_TIMEOUT_MS = 400`, `DRAG_START_DISTANCE = 8.0`), структуры `WindowHeaderConfig` и контроллер `WindowHeader` с поддержкой отступа светофоров `leading_inset` и обработки событий мыши;
  - `crates/rusty-ui/src/canvas.rs`: добавлены методы `fill_rounded_rect` и `stroke_rounded_rect`;
  - `crates/rusty-ui/src/lib.rs`: экспорт модулей `menu` и `window_header`, всесторонний unit-тест `menu_layout_and_window_header`;
  - `crates/rusty-compositor`: обновлены константы контекстных меню, меню верхней панели, поповера Wi-Fi и шейпов хромирования (`MENU_POPOVER_RADIUS = 14`, `MENU_ITEM_HEIGHT = 26`, `MENU_SEPARATOR_HEIGHT = 12`, `HOVER_PILL_RADIUS = 9`), внедрено перекрытие подменю `SUBMENU_OVERLAP = 2 px` для надежного диагонального транзита курсора, обновлен тест `context_menu_cascade_submenu_hover_flip_and_hit_testing`;
  - `apps/rusty-files`: в `layout.rs` и `render/popup.rs` внедрены метрики `rusty_ui::menu::ROW_HEIGHT` (26 px), `SEPARATOR_HEIGHT` (12 px), `VERTICAL_CONTENT_MARGIN` (5 px), `MENU_POPOVER_RADIUS` (14 px), `HOVER_PILL_RADIUS` (9 px);
  - Документация: `ADR-0069` (`docs/adr/0069-unified-window-header-and-context-menu-style.md`), запись `D-083` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (322 unit-теста) проходят без единой ошибки и предупреждения;
  - `cargo run -p rusty-compositor -- --drm-probe` завершается штатно;
  - Живой `--winit` прогон композитора стартует и работает стабильно;
  - Полное согласование метрик контекстных меню и заголовков окон между общим крейтом `rusty-ui`, оболочкой композитора и приложениями.
- **Tests:** 322 unit-теста workspace (`4` `rusty-animation`, `199` `rusty-compositor`, `17` `rusty-files`, `13` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `11` `rusty-settings`, `5` `rusty-shell`, `15` `rusty-terminal`, `7` `rusty-ui`, `7` `rusty-wm`).
- **Risks:** Нет; изменения геометрии меню и оконных заголовков строго концентричны и полностью покрыты автоматическими тестами.

### RUSTYOS-126 — Трёхдольная система теней Гаусса и сквозная ликвидация хардкода (`RUSTYOS-126 Вариант 2`)

- **Status:** Done (`RUSTYOS-126 Вариант 2` — утверждённый объём выполнен)
- **Goal:** Реализовать модуль `crates/rusty-ui::shadow` с эталонной 3-дольной математической моделью Гаусса (`ShadowStyle`, `ShadowElevation`: `ActiveWindow`, `InactiveWindow`, `MenuOrTooltip`, `Popup`, `Dock`) и сквозной заменой всего локального хардкода параметров теней в `crates/rusty-compositor`, `apps/rusty-files` и `crates/rusty-ui`.
- **Dependencies:** `RUSTYOS-125`.
- **Scope:**
  - `crates/rusty-ui/src/shadow.rs`: реализованы структуры `ShadowElevation`, `ShadowLobe`, `ShadowStyle` (`lobes: [ShadowLobe; 3]`), вычисление радиуса охвата `reach()`, эталонные уровни возвышения (Active/Inactive 24/9/14 px, Menu/Tooltip 14/6/8 px, Popup 8/4/3 px, Dock 10/4/3 px без контакта), функция отрисовки на холсте `draw_elevation_shadow`;
  - `crates/rusty-ui/src/lib.rs`: экспорт модуля `shadow`, unit-тест `shadow_elevation_and_rendering`;
  - `crates/rusty-ui/src/menu.rs`: замена ad-hoc цикла в `draw_menu_background` на вызов `draw_elevation_shadow(..., ShadowElevation::MenuOrTooltip)`;
  - `crates/rusty-compositor/src/surfaces/window_shadow.rs`: добавление `for_elevation` на базе `ShadowStyle`, перевод `for_window_mode` на эталонные 24/9/14 px (границы окна `fb.size == ub.size` стабильны при переключении фокуса);
  - `crates/rusty-compositor/src/surfaces/mod.rs`: добавлен метод `elevation_shadow_render_element`;
  - `crates/rusty-compositor/src/shell_render/tokens.rs`: добавлены `push_elevation_shadow` и `push_liquid_glass_panel_elevation`;
  - `crates/rusty-compositor`: устранение магических кортежей `(16, 52)`, `(18, 54)` и `(8, 28)` в `chrome_shapes.rs`, `context_menu.rs`, `wifi_popover.rs`, `updates_popover.rs`, `dock_stack.rs`, `notification_center.rs` и `frame_composer.rs`;
  - `apps/rusty-files`: в `render/popup.rs` и `dialogs.rs` удалены жестко закодированные массивы `shadow_layers = [...]` и смещения, отрисовка переведена на расчёт через `rusty_ui::shadow::ShadowElevation`;
  - `apps/rusty-shell`: очищен устаревший комментарий в `wayland.rs`;
  - Документация: `ADR-0070` (`docs/adr/0070-unified-three-lobe-shadow-system.md`), запись `D-084` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (323 unit-теста) проходят без ошибок и предупреждений;
  - `cargo run -p rusty-compositor -- --drm-probe` завершается штатно;
  - Живой `--winit` прогон композитора стартует и работает стабильно;
  - Полная ликвидация хардкода параметров теней во всех компонентах десктопа.
- **Tests:** 323 unit-теста workspace (`4` `rusty-animation`, `199` `rusty-compositor`, `17` `rusty-files`, `13` `rusty-installer`, `3` `rusty-ipc`, `44` `rusty-services`, `11` `rusty-settings`, `5` `rusty-shell`, `15` `rusty-terminal`, `8` `rusty-ui`, `7` `rusty-wm`).
- **Risks:** Нет; аналитический расчет долей строго эквивалентен эталону и проверен в тестах.

### RUSTYOS-127 — Сквозное приведение меню TopBar и стилей popover к эталонной архитектуре (`RUSTYOS-127 Вариант 1`)

- **Status:** Done (`RUSTYOS-127 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Привести левое меню `TopBar` (`SystemLogo` и `ActiveApp`), состояние рабочего стола по умолчанию (`Desktop`, скрытие `Go`), плашку открытого пункта (`8 px`) и рендер/логику `enabled` пунктов выпадающего меню (`14 px` popover, `9 px` акцентная ховер-пилюля) в точное соответствие эталону.
- **Dependencies:** `RUSTYOS-125`, `RUSTYOS-126`.
- **Scope:**
  - `crates/rusty-compositor/src/shell_render/top_bar_menus.rs`, `top_bar/panel.rs`, `top_bar/menus.rs`: добавлен `TopBarAppProfile::Desktop` по умолчанию, `TopBarMenuEntry::Item::enabled`, жизненный цикл `Hide {App}` / `Hide Others` / `Quit`, очищенное `MENU_SYSTEM`, `default_profile_for_app_name`;
  - `crates/rusty-compositor/src/state/shell_actions.rs`, `input/shell_pointer.rs`: добавлены `HideActiveApp` и `HideOtherApps`, профиль `"Desktop"` при отсутствии сфокусированного окна, фильтрация неактивных пунктов при клике и наведении;
  - `crates/rusty-compositor/src/shell_render/chrome_shapes.rs`, `text_pass.rs`: плашка `8 px` в `TopBar`, отключение ховера и тусклый цвет текста/иконок для `!enabled`, концентрическая акцентная пилюля `9 px` (`alpha = 230` Light / `210` Dark);
  - `crates/rusty-compositor/src/frame_composer.rs`, `screenshot.rs`, `winit_backend.rs`: fallback-размер в `process_pending_screenshot`, `RUSTYOS_SCREENSHOT_DEST` и `RUSTYOS_TEST_CAPTURE_MENU` для живых `--winit` снимков;
  - `crates/rusty-compositor/src/shell_render/tests.rs`, `state/tests.rs`: тест `desktop_active_app_and_system_menu_structure_matches_reference` и адаптация тестов `TopBar`;
  - Документация: `ADR-0071` (`docs/adr/0071-topbar-menu-architecture-and-popover-fidelity.md`), запись `D-085` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (325 пройденных unit-тестов) завершаются чисто;
  - Живая визуальная проверка `--winit` подтверждает совпадение `TopBar` (`Desktop` / `SystemLogo`) и выпадающих меню с эталоном.
- **Tests:** 325 unit-тестов workspace (включая `200` в `rusty-compositor`; `1` предсуществующий тест `rusty-services::tests::networkmanager_and_bluetooth_formatters_handle_all_states` требует BlueZ D-Bus в sandbox).
- **Risks:** Нет.

### RUSTYOS-128 — Консолидация архитектуры, единый суперэллипс, композиторный клиппинг углов, анимации окон и Liquid Glass сайдбаров (`RUSTYOS-128 Вариант 1`)

- **Status:** Done (`RUSTYOS-128 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Устранить дубли (`apps/rusty-shell`, `modals/settings.rs`), синхронизировать `crates/rusty-ui` (`WINDOW_RADIUS = 14`, 7-цветные палитры, premul `UiCanvas`), перевести тени на честный 3-дольный GPU-шейдер без `push_soft_shadow`, внедрить композиторную обрезку клиентских углов и послойный Liquid Glass сайдбаров, добавить анимации открытия/закрытия окон, приглушение светофоров в фоне и обновить навыки/документацию.
- **Dependencies:** `RUSTYOS-125`, `RUSTYOS-126`, `RUSTYOS-127`.
- **Scope:**
  - Удаление дублей: удалены `apps/rusty-shell` (с актуализацией `Cargo.toml`, `bin/rustyos-session`, `scripts/install.sh`, `packaging/arch/PKGBUILD`) и `crates/rusty-compositor/src/shell_render/modals/settings.rs` (`ShellOverlayView::Settings`);
  - `crates/rusty-ui`, `crates/rusty-services/src/file_manager`, `apps/*`: `WINDOW_RADIUS = 14`, нейтральный `FilesThemeMode::{Light, Dark}`, единый `ACCENT_PALETTE` (`#5E5CE6` на индексе 3), premul Porter-Duff Over в `UiCanvas`, `draw_elevation_shadow_with`;
  - `crates/rusty-compositor/src/surfaces/`: новый `window_clip.rs` (`ClippedSurfaceElement`, `SURFACE_CLIP_FRAG`), 3-дольный `WINDOW_SHADOW_FRAG` (`u_lobe1..3`), единый кубический суперэллипс в `LIQUID_GLASS_FRAG`, `WINDOW_SHADOW_FRAG`, `SURFACE_CLIP_FRAG` и `sdf::sample_corner_pixel`, послойный `prepare_backdrop_slot` в `liquid_glass.rs`, open/close lifecycle-снимки в `window_minimize.rs`;
  - `crates/rusty-compositor/src/{frame_composer.rs, state/*, shell_render/*}`: полная ликвидация `push_soft_shadow` и кортежей `(spread, alpha)`, светлая полярность `TopBar` при `reduce_transparency`, послойный `build_backdrop_render_elements` снизу вверх, обёртка неполноэкранных окон в `ClippedSurfaceElement` (с сохранением Direct Scanout), `180 ms`/`160 ms` fade+scale open/close анимации с тенью;
  - `apps/rusty-files`, `apps/rusty-settings`, `apps/rusty-installer`: `WlRegion`-ограниченные Liquid Glass сайдбары (`rustyos_liquid_glass_v1`) с матовым фолбэком и приглушение светофоров без `State::Activated`;
  - Документация и навыки: `ADR-0072`, `D-086` (`DECISIONS.md`), `DESIGN.md`, `ROADMAP.md`, `ARCHITECTURE.md`, `FILE_INVENTORY.md`, `README.md`, `.agents/skills/{ui-ux-design,gpu-rendering,desktop-shell,window-management,wayland-compositor,animation-engine}/SKILL.md`.
- **Acceptance Criteria:**
  - Все 48 вариантов 6 GLSL-шейдеров валидируются `/usr/bin/glslangValidator`;
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, тесты workspace (за вычетом D-Bus теста BlueZ в sandbox) и `--drm-probe` проходят без ошибок и предупреждений;
  - Живые прогоны `--winit` на Wayland (Light) и X11 (Dark) со спауном `rusty-files` и `rusty-settings` проходят без ошибок EGL/shader и подтверждены визуальной инспекцией PNG.
- **Tests:** 323 пройденных теста workspace (`4` `rusty-animation`, `201` `rusty-compositor`, `17` `rusty-files`, `13` `rusty-installer`, `3` `rusty-ipc`, `43` `rusty-services` + `1` пропущенный D-Bus BlueZ тест, `12` `rusty-settings`, `15` `rusty-terminal`, `8` `rusty-ui`, `7` `rusty-wm`).
- **Risks:** Нет; полноэкранные клиенты не оборачиваются в `ClippedSurfaceElement`, сохраняя путь Direct Scanout.

### RUSTYOS-129 — Калибровка `ShadowElevation`, облегчение `GlassPanelStyle::Popover` и контраст токенов меню (`RUSTYOS-129 Вариант 1`)

- **Status:** Done (`RUSTYOS-129 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Убрать жёсткую тёмную контактную обводку и избыточный ореол у меню/поповеров и верхних/боковых границ окон, убрать толстый глянцевый кант и молочную плотность у `GlassPanelStyle::Popover`, повысить читаемость `popover_text_disabled` и смягчить `popover_separator`.
- **Dependencies:** `RUSTYOS-126`, `RUSTYOS-127`, `RUSTYOS-128`.
- **Scope:**
  - `crates/rusty-ui/src/shadow.rs`, `crates/rusty-ui/src/lib.rs`: калибровка `ShadowStyle::for_elevation` (`MenuOrTooltip`: `(14.0, 6.0, 8.0, 0.28, 0.0)`, `Popup`: `(10.0, 4.0, 4.0, 0.24, 0.0)`, `InactiveWindow`: `(24.0, 9.0, 14.0, 0.48, 0.20)`, `ActiveWindow`: `(24.0, 9.0, 14.0, 0.85, 0.28)`) и расширение тестов `shadow_elevation_and_rendering`;
  - `crates/rusty-compositor/src/surfaces/liquid_glass.rs`, `surfaces/sdf.rs`: облегчение `GlassPanelStyle::Popover` (`displacement = 6.0`, `rim = 0.22`, Light `[0.95, 0.96, 0.98, 0.22 + 0.22 * g]`, Dark `[0.17, 0.18, 0.20, 0.28 + 0.24 * g]`), синхронизация CPU-фолбэка `base_glass_layer`/`rim_peak` (`0.16`/`0.14`) и тест `popover_material_keeps_subtle_rim_and_low_displacement`;
  - `crates/rusty-compositor/src/shell_render/tokens.rs`: обновление `popover_text_disabled` (`[72, 78, 90, 155]` Light / `[154, 158, 168, 145]` Dark) и `popover_separator` (`0.055` Light / `0.07` Dark);
  - Документация: обновление `DESIGN.md`, создание `ADR-0073` (`docs/adr/0073-popover-glass-and-soft-shadow-calibration.md`), запись `D-087` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, тесты workspace (за вычетом D-Bus теста BlueZ в sandbox) и `--drm-probe` проходят без ошибок;
  - Живые `--winit` прогоны на Wayland (Light меню + окна) и X11 (Dark окна) проходят без ошибок EGL/shader и подтверждают визуальным осмотром PNG отсутствие жёсткого контура вокруг меню/окон, воздушный матовый поповер и чёткую читаемость `Hide Desktop`/`Quit`.
- **Tests:** 324 пройденных unit-теста workspace (`4` `rusty-animation`, `202` `rusty-compositor`, `17` `rusty-files`, `13` `rusty-installer`, `3` `rusty-ipc`, `43` `rusty-services` + `1` пропущенный D-Bus BlueZ тест, `12` `rusty-settings`, `15` `rusty-terminal`, `8` `rusty-ui`, `7` `rusty-wm`).
- **Risks:** Нет; `bounds_and_offsets` окон не изменились, Direct Scanout не затронут.

### RUSTYOS-130 — Полноценный клиентский UI-тулкит `crates/rusty-ui`, dual GLES 2.0 / CPU рендерер и миграция системных приложений (`RUSTYOS-130 Вариант 1`)

- **Status:** Done (`RUSTYOS-130 Вариант 1` — утверждённый объём выполнен)
- **Goal:** Превратить `crates/rusty-ui` в единый клиентский UI-тулкит с клавиатурным фокусом, выделением/копированием текста (`wl_data_device`), стандартными контролами и анимациями (`rusty-animation` + `reduced_motion`), добавить аппаратный `Wayland-EGL + GLES 2.0` презентер поверх `UiDrawList` с CPU `wl_shm` fallback и перевести `rusty-installer`, `rusty-settings` и `rusty-files` на общий тулкит и `UiFontEngine`.
- **Dependencies:** `RUSTYOS-113`, `RUSTYOS-128`, `RUSTYOS-129`.
- **Scope:**
  - `Cargo.toml`, `crates/rusty-ui/{Cargo.toml,build.rs,src/*}`: добавлены `text.rs` (`UiFontEngine`), `motion.rs` (`ToggleMotion`, `HoverPressMotion`, `ScrollMotion`, `CaretBlink`), `focus.rs` (`WidgetId`, `FocusScope`), `clipboard.rs` (`WaylandClipboardBridge`, `MemoryClipboard`, `ClipboardSink`), `draw.rs` (`UiDrawList`, `UiPainter`, `render_draw_list_to_canvas`), `gpu_gles.rs` (`GlesSurfaceRenderer` на `gl_generator` EGL 1.5 / GLES 2.0 + `wayland-egl`, изолированный `#![allow(unsafe_code)]`), `text_input.rs` (`TextField`/`TextBuffer`), `scroll.rs` (`ScrollView`), `controls.rs` (`DropdownSelect`, `SegmentedTabs`, `Checkbox`, `Slider`), `list.rs` (`SelectableList`), `dialog.rs` (`DialogSheet`), `ContextMenuSession`/`paint_menu_with_painter`, `ToggleSwitch::paint_animated`, `PushButton::paint_labeled`;
  - `apps/rusty-installer`: удалена прямая зависимость `fontdue`, добавлен `--no-gpu`, формы переведены на `TextField` + `FocusScope` + `DataDevice`/`WaylandClipboardBridge` + `UiFontEngine` + `GlesSurfaceRenderer` с `wl_shm` fallback;
  - `apps/rusty-settings`: удалена прямая зависимость `fontdue`, добавлены `--no-gpu`, `xkbcommon` `KeyboardHandler`, поиск на `TextField::search` + `WaylandClipboardBridge`, анимации тумблеров `ToggleSwitch::paint_animated`, перетаскивание ползунка скроллбара, `UiFontEngine` и `GlesSurfaceRenderer` с сохранением прозрачности сайдбара `WlRegion` и `wl_shm` fallback;
  - `apps/rusty-files`: удалена прямая зависимость `fontdue`, добавлены `--no-gpu`, `xkbcommon` `KeyboardHandler`, `TextField` + `WaylandClipboardBridge` для поиска/inline-переименования/Go to Folder, `PushButton::paint_labeled` в диалоге конфликта, единый `UiFontEngine` и `GlesSurfaceRenderer` с сохранением прозрачного `wl_shm` для `xdg_popup`;
  - Документация: обновлены `DESIGN.md`, `ARCHITECTURE.md` (§7), создан `docs/adr/0074-rusty-ui-client-toolkit-and-gles-renderer.md`, добавлена запись `D-088` в `DECISIONS.md`.
- **Acceptance Criteria:**
  - Все 6 GLSL ES 1.00 шейдеров в `crates/rusty-ui/src/gpu_gles.rs` валидируются `/usr/bin/glslangValidator` (`rc=0`);
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `--drm-probe` и unit-тесты workspace (за исключением предсуществующего D-Bus теста BlueZ в sandbox) проходят без ошибок и предупреждений;
  - Живые `--winit` прогоны на Wayland (Light `rusty-files` + `rusty-settings`, Light `rusty-settings`, Light `rusty-installer --oobe --demo`, Light CPU fallback `RUSTYOS_NO_GPU=1`) и X11 (Dark `rusty-files` + `rusty-settings`) проходят без EGL/GLES фолбэков в GPU-режиме и подтверждены визуальным осмотром PNG-скриншотов.
- **Tests:** 332 пройденных unit-теста workspace (`4` `rusty-animation`, `202` `rusty-compositor`, `18` `rusty-files`, `13` `rusty-installer`, `3` `rusty-ipc`, `43` `rusty-services` + `1` пропущенный D-Bus BlueZ тест, `13` `rusty-settings`, `15` `rusty-terminal`, `14` `rusty-ui`, `7` `rusty-wm`).
- **Risks:** Нет; `xdg_popup` меню сохраняют прозрачные `wl_shm`-буферы, весь `unsafe` FFI изолирован в `crates/rusty-ui/src/gpu_gles.rs` с автоматическим CPU `wl_shm` fallback.

## Правило добавления задач

Каждая задача должна иметь ID, цель, scope, dependencies, acceptance
criteria, tests, risks и status. Детализировать в первую очередь
ближайшую фазу.
