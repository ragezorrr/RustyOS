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

## Правило добавления задач

Каждая задача должна иметь ID, цель, scope, dependencies, acceptance
criteria, tests, risks и status. Детализировать в первую очередь
ближайшую фазу.
