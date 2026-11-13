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

## Правило добавления задач

Каждая задача должна иметь ID, цель, scope, dependencies, acceptance
criteria, tests, risks и status. Детализировать в первую очередь
ближайшую фазу.
