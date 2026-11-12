# ADR-0003: Фаза 3 — Native Graphics Backend (DRM/KMS, GBM/EGL, Capability Discovery и Безопасный Fallback)

- Status: accepted
- Date: 2026-11-12
- Decision owner: RustyOS user

## Context

После завершения Фазы 2 (`Window Management` — `RUSTYOS-004` и `RUSTYOS-005`) следующим этапом по `ROADMAP.md` является **Фаза 3 — Native Graphics Backend** (`RUSTYOS-006`).
Цель Фазы 3:
- обнаружение возможностей оборудования во время выполнения (`capability discovery`: DRM nodes, коннекторы, режимы, atomic modesetting, cursor/overlay planes, GBM, EGL/GLES форматы и DMA-BUF модификаторы);
- жизненный цикл `DRM/KMS` + `GBM/EGL` + `libseat` + `udev` + `libinput` для нативного запуска;
- проверка реального оборудования без поломки текущей графической сессии разработчика, запись точных версий kernel/driver/userspace и ограничений в `docs/HARDWARE_MATRIX.md`, а также сохранение nested Winit backend для повседневной разработки.

Read-only аудит хост-системы показал:
- В `/dev/dri/` доступны `card1` (`226:1`, активный модуль ядра `i915`, Intel Raptor Lake-U `[8086:a7ac]`) и `renderD128` (`226:128`).
- Подключён дисплей `card1-eDP-1` (`1920x1080`), коннекторы `DP-1` и `HDMI-A-1` в статусе `disconnected`.
- В локальном кэше Cargo уже присутствуют все необходимые крейты (`drm 0.14.1`, `gbm 0.18.0`, `udev 0.9.3`, `libseat 0.2.4`, `input 0.9.1`).
- Поскольку на `card1` сейчас запущена активная графическая сессия пользователя (Wayland `wayland-1` и сама IDE Antigravity), захват DRM master на `card1` изнутри текущей сессии без переключения VT либо вернёт `EACCES`/`EBUSY` (безопасный отказ `libseat`/DRM), либо при принудительном переключении VT прервёт работу самой IDE.

Требуется согласовать:
1. Включение фич `smithay` для Фазы 3 (`backend_drm`, `backend_gbm`, `backend_udev`, `backend_session_libseat`, `backend_libinput`).
2. Архитектуру модуля нативного бэкенда (`drm_backend.rs`) с безопасным `Capability Discovery & Offscreen GBM/EGL/DMA-BUF Probe` (которое можно запускать прямо из текущей сессии на `/dev/dri/card1` и `/dev/dri/renderD128` без захвата DRM master) + полноценным путём `LibSeatSession` + `DrmDevice` для запуска из чистого TTY.
3. Локальное разрешение `#[allow(unsafe_code)]` исключительно для двух вызовов `EGLDisplay::new` и `GlesRenderer::new`, которые в API `smithay 0.7.0` объявлены как `unsafe fn` (с обязательным `// SAFETY:` комментарием и безопасной обёрткой, как требует п. 5 `AGENTS.md`).

## Options

### Option A (Рекомендуемый): Двухуровневый Native Backend (`--drm-probe` + `--drm`) в `rusty-compositor`

1. **Зависимости и фичи `smithay` в `Cargo.toml`:**
   Расширяем фичи `smithay` в `[workspace.dependencies]`:
   - `backend_drm`, `backend_gbm`, `backend_udev`, `backend_session_libseat`, `backend_libinput` (при сохранении `backend_winit`, `renderer_gl`, `wayland_frontend`, `desktop`; без `backend_vulkan` и `xwayland`).
2. **Режим 1 — Неинвазивный Hardware Capability & GBM/EGL/DMA-BUF Probe (`--drm-probe` или автоматический fallback при занятом DRM Master):**
   - Сканирует устройства через `udev` (`UdevBackend` / `/dev/dri/card*` и `renderD*`), определяет primary и render ноды (`DrmNode`).
   - Читает через DRM ioctl / sysfs список коннекторов (`eDP-1`, `DP-1`, `HDMI-A-1`), их статус, физические размеры и режимы (`1920x1080 @ 60 Hz`), проверяет наличие `ClientCapability::Atomic`, `UniversalPlanes`, cursor/overlay planes.
   - Инициализирует `GbmDevice` и `GbmAllocator`, поднимает нативный `EGLDisplay` и `GlesRenderer` на GPU (`Intel Raptor Lake-U`), извлекает поддерживаемые `dmabuf_render_formats` и `dmabuf_texture_formats` (Fourcc + Modifiers, включая линейные и Intel tiled модификаторы).
   - Выполняет реальный аппаратный тест аллокации GBM DMA-BUF буфера (`Argb8888`), рендеринга кадра через `GlesRenderer` на GPU и импорта DMA-BUF текстуры, после чего формирует полный отчёт `DrmHardwareReport` (и опционально сохраняет JSON).
   - **Главное преимущество:** 100% безопасно выполняется прямо внутри текущей сессии Antigravity, реально тестирует весь стек `Kernel i915 -> libdrm -> GBM -> EGL -> GLES 3.2 -> DMA-BUF` на физическом GPU без риска погасить экран разработчика.
3. **Режим 2 — Полный KMS Session Mode (`--drm` / `--kms`):**
   - Пытается открыть сессию через `LibSeatSession::new()`, получить DRM master через сессию, инициализировать `DrmDevice` + `GbmDevice` + `UdevBackend` + `LibinputInputBackend`.
   - Если запущен изнутри уже активной графической сессии, где DRM master занят хостовым композитором, не падает с паникой, а выдаёт чёткую диагностику, выполняет capability probe и (при флаге `--fallback-winit`) переключается на nested Winit backend.
4. **Изоляция `unsafe`:**
   - В `smithay 0.7.0` конструкторы `EGLDisplay::new` и `GlesRenderer::new` помечены `unsafe`. Мы изолируем их в одной приватной функции-обёртке `init_gbm_egl_renderer` с явным `#[allow(unsafe_code)]` и документацией `// SAFETY:`, сохраняя `#![deny(unsafe_code)]` на уровне всего workspace.

- **Плюсы:**
  - Строго соблюдает `HARDWARE.md`, `system-safety.md` и `graphics-performance.md`.
  - Даёт воспроизводимую аппаратную проверку DRM/GBM/EGL/DMA-BUF на реальном Intel Raptor Lake-U прямо сейчас, не убивая текущую Wayland-сессию пользователя.
  - Закладывает полноценный KMS/libseat код для запуска из TTY.
- **Минусы:**
  - Полный захват KMS page-flip (`DRM Master`) может быть выполнен только при запуске из отдельного TTY (вне активного хостового композитора), что честно фиксируется в `docs/HARDWARE_MATRIX.md`.
- **Риски:** Низкие.

### Option B: Только монолитный `--drm` с принудительным переключением VT / захватом сессии

- **Плюсы:** Чуть меньше кода (нет отдельного режима `--drm-probe`).
- **Минусы:** Попытка запустить такой тест во время работы в Antigravity либо завершится ошибкой доступа к seat, либо переключит VT и прервёт сессию пользователя.

### Option C: Выделение отдельного крейта `crates/rusty-renderer` уже сейчас

- **Плюсы:** Соответствует будущей карте крейтов.
- **Минусы:** На этапе Фазы 3 создаст циклические или дублирующие обёртки вокруг `smithay::backend::drm` и `smithay::backend::renderer::gles`, пока рендерер используется только внутри `rusty-compositor`.

## Recommendation

Рекомендуется **Option A**:
- Включить фичи `backend_drm`, `backend_gbm`, `backend_udev`, `backend_session_libseat`, `backend_libinput` в `smithay`.
- Реализовать модуль `crates/rusty-compositor/src/drm_backend.rs` с поддержкой как полного `LibSeatSession` KMS-пути (`--drm`), так и безопасного аппаратного профилирования `DRM/GBM/EGL/DMA-BUF` (`--drm-probe` и автоматическая диагностика при занятом DRM Master).
- Прогнать реальный аппаратный тест на `/dev/dri/card1` / `/dev/dri/renderD128` (`Intel Raptor Lake-U`, `i915`) и занести точные данные в `docs/HARDWARE_MATRIX.md`.

## Decision

Утверждён **Option A** (APPROVE: Option A, 2026-11-12): включение фич `backend_drm`, `backend_gbm`, `backend_udev`, `backend_session_libseat`, `backend_libinput` в `smithay` и реализация двухуровневого нативного бэкенда (`--drm-probe` для безопасной аппаратной проверки DRM/GBM/EGL/DMA-BUF и `--drm` для KMS/libseat сессии).

## Consequences

- `Cargo.toml` обновляется фичами `smithay` для DRM/GBM/udev/libseat/libinput.
- В `crates/rusty-compositor` появляется модуль `drm_backend.rs` с runtime capability discovery, проверкой GBM/EGL/DMA-BUF и KMS/libseat инициализацией.
- В `docs/HARDWARE_MATRIX.md` обновляется строка `DRM/KMS` результатами реальной проверки на Intel Raptor Lake-U.
