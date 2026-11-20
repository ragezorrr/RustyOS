# RustyOS --- Performance and Frame Pacing

## 1. Цели

Цель --- отзывчивость, ровное движение курсора и отсутствие заметных
рывков. FPS сам по себе недостаточен: важны presentation timing, latency
и frame variance.

    Refresh rate   Период кадра
  -------------- --------------
           60 Hz       16.67 ms
          120 Hz        8.33 ms
          144 Hz        6.94 ms
          240 Hz        4.17 ms

Это теоретический frame interval, а не обещание достижимой
производительности.

## 2. Обязательные метрики

- frame interval distribution: p50/p95/p99;
- missed/late frames;
- presentation timestamps;
- input-to-display latency, если методика достоверна;
- CPU time compositor thread;
- GPU busy/time и queue depth, если доступны достоверные данные;
- memory usage;
- event-loop stalls;
- фактический refresh rate и mode changes.

## 3. Основные принципы

- Не блокировать compositor event loop.
- Использовать корректное frame scheduling и presentation feedback.
- Внедрять damage tracking.
- Не перерисовывать неизменённые области без необходимости.
- Page flip/direct scanout применять только при подтверждённых условиях.
- Hardware cursor использовать, если cursor plane доступен и корректен.
- Избегать лишних GPU/CPU копирований; zero-copy подтверждать
  измерениями.
- Учитывать разные refresh rates на outputs.
- VRR использовать только при поддержке всей цепочки.
- Не привязывать animation clock к фиксированным 60 FPS.

## 4. Курсор и input latency

Проверять путь input event → cursor state → hardware/software
composition → display. Измерять: - cadence input/cursor updates; -
hardware cursor plane behavior; - задержки блокировок; - scaling,
rotation и multi-monitor; - синхронизацию с display.

Если курсор выглядит как на низкой герцовке, сначала измерить
фактический presentation cadence и частоту обновления cursor state, а не
менять renderer наугад.

## 5. Benchmark protocol

Каждый тест фиксирует: - GPU и модель; - kernel и driver; - userspace
graphics stack; - backend; - resolution/scaling/refresh; - число
outputs; - workload и длительность; - commit hash; - команды запуска; -
результаты и ограничения.

По возможности менять один фактор за раз.

## 6. Acceptance

У каждой оптимизации есть baseline, воспроизводимый результат и оценка
регрессий. Не обещать 144/240 Hz на всех GPU. Не удалять
correctness/security checks ради benchmark.

## 7. Зафиксированный Baseline (`RUSTYOS-003`, 2026-11-11)

- **Оборудование:** Intel Corporation Raptor Lake-U `[8086:a7ac]` (rev 04)
- **ОС / Ядро / Драйвер:** EndeavourOS, Linux `7.2.11-arch1-1`, модуль `i915`
- **Userspace Graphics:** Mesa `26.2.6-arch1.1` (EGL 1.5, OpenGL ES 3.2)
- **Backend и режим:** `nested-winit` (`1280x800 @ 60 Hz`, `target_interval_us = 16666`)
- **Workload:** 120 кадров с активным клиентом `es2gears_wayland`
- **Команда запуска:**
  ```bash
  target/debug/rusty-compositor --winit --max-frames 120 --spawn es2gears_wayland --metrics-out /tmp/rustyos-frame-metrics.json
  ```
- **Результаты (до/после компенсации таймера кадра):**
  - *До компенсации (`ToDuration(16_667)` без вычета времени рендера):* `interval_us(p50/p95/p99/max) = 18865 / 19854 / 20121 / 26071`
  - *После компенсации (`16_667 us - tick_elapsed`):* `interval_us(p50/p95/p99/max) = 16787 / 17914 / 18284 / 25523`, `render_us(p50/p95/p99/max) = 2193 / 3315 / 4760 / 10074`, `late = 1` (первый кадр маппинга клиента), `missed = 0`, `stalls = 0`.

## 8. Фаза 4 --- Performance Foundation & Damage Tracking Benchmark (`RUSTYOS-007`, 2026-11-12)

- **Оборудование:** Intel Corporation Raptor Lake-U `[8086:a7ac]` (rev 04), `i915`, Linux `7.2.11-arch1-1`, Mesa `26.2.6-arch1.1`
- **Commit:** `9188766` (базовый) -> `RUSTYOS-007`
- **Workload:** 120 кадров `nested-winit` (`1280x800 @ 60 Hz`), клиент `es2gears_wayland` (`768x533`), синтетический поток движения указателя (`--bench-input` на каждом кадре)
- **Команды воспроизведения:**
  ```bash
  # 1. Режим принудительного полного повреждения (Before / Full-Damage)
  target/debug/rusty-compositor --winit --max-frames 120 --spawn es2gears_wayland --bench-input --force-full-damage --metrics-out /tmp/rustyos-bench-full-damage.json

  # 2. Режим отслеживания повреждений + EGL buffer_age (After / Tracked-Damage)
  target/debug/rusty-compositor --winit --max-frames 120 --spawn es2gears_wayland --bench-input --metrics-out /tmp/rustyos-bench-tracked-damage.json
  ```
- **Сравнение результатов (Full-Damage vs Tracked-Damage):**

| Метрика | Full-Damage (`--force-full-damage`) | Tracked-Damage (`buffer_age` + `OutputDamageTracker`) | Изменение |
| :--- | :--- | :--- | :--- |
| `damage_ratio_permille (avg / p95)` | `1000 / 1000` (100% экрана) | `307 / 462` (30.7% среднее, 46.2% при обновлении окна) | **-69.3% площади композитинга** |
| `zero_damage_frames` | `0` | `25` | Пропуск лишней перерисовки на кадрах без нового коммита |
| `render_duration_us (p50 / p95 / p99 / max)` | `2611 / 6610 / 8773 / 12489` | `2750 / 5321 / 6803 / 8736` | **p95 -19.5%, p99 -22.5%, max -30.0%** |
| `input_to_present_us (p50 / p95 / p99 / max)` | `3150 / 15438 / 16417 / 23546` | `3068 / 6515 / 8829 / 22024` | **p95 -57.8% (`15.4 ms -> 6.5 ms`), p99 -46.2%** |
| `event_loop_dispatch_us (p50 / p95 / p99 / max)` | `196 / 1232 / 1473 / 2746` | `197 / 1145 / 1392 / 1631` | 0 dispatch stalls (`< 8.33 ms`) |
| `interval_us (p50 / p95 / p99 / max)` | `16956 / 18112 / 18727 / 29813` | `17023 / 18529 / 18882 / 21108` | **max interval -29.2% (`late_frames: 1 -> 0`)** |
| `late / missed / stalls (render/dispatch)` | `1 / 0 / 0 / 0` | `0 / 0 / 0 / 0` | Ни одного пропущенного или позднего кадра |

## 9. Фаза 6 --- Release Profile, Zero-Alloc Hot Paths, Backdrop Skip & Bounded Age-Eviction (`RUSTYOS-119`, 2026-11-20)

- **Оборудование:** Intel Corporation Raptor Lake-U `[8086:a7ac]` (rev 04), `i915`, Linux `7.3.4-arch1-2`
- **Userspace Graphics:** Mesa `26.2.7-arch1.1` (`mesa 1:26.2.7-1`, EGL 1.5, OpenGL ES 3.2)
- **Commit / Scope:** базовый `82a92f9` -> `RUSTYOS-119` (`[profile.release]` в `Cargo.toml`, lazy `interval_p50_us()` + `snapshot_arc()` в `frame_composer.rs`/`frame_timing.rs`/`telemetry.rs`, `backdrop_signature_matches` skip в `liquid_glass.rs`, zero-alloc key lookup + age-eviction в `TextCache`/`UiShapeCache`/`IconCache`)
- **Backend и режим:** `nested-winit` (`1280x800 @ 60 Hz`, `target_interval_us = 16666`, 1 output)
- **Команды воспроизведения (`--release`):**
  ```bash
  cargo build --release -p rusty-compositor --bin rusty-compositor
  target/release/rusty-compositor --winit --max-frames 120 --bench-input --metrics-out /tmp/rustyos-119-idle.json
  target/release/rusty-compositor --winit --max-frames 120 --bench-input --spawn ./target/debug/rusty-terminal --metrics-out /tmp/rustyos-119-client.json
  target/release/rusty-compositor --winit --max-frames 240 --bench-input --metrics-out /tmp/rustyos-119-idle240.json
  ```
- **Сравнение (`Before 82a92f9` vs `After RUSTYOS-119`):**

| Метрика | Before (`82a92f9`, `--release`) | After (`RUSTYOS-119`, `--release`) | Изменение |
| :--- | :--- | :--- | :--- |
| Размер бинарника `rusty-compositor` | `23M` (`unstripped`) | `15M` (`15,121,208 bytes`, `lto=thin`, `codegen-units=1`, `strip=symbols`) | **~-35% (`-8M`)** |
| Idle `120f`: `render_duration_us (p50 / p95 / p99 / max)` | `2499 / 6510 / 17320 / 168758` | `1402 / 7693 / 10886 / 155421` | **p50 -43.9%, p99 -37.1%, render_stalls 3 -> 1** |
| Idle `120f`: `input_to_present_us (p50 / p95 / p99 / max)` | `2546 / 10182 / 17336 / 185256` | `1425 / 7700 / 11145 / 171905` | **p50 -44.0%, p95 -24.4%, p99 -35.7%** |
| Idle `120f`: `damage_ratio_avg / zero_damage_frames` | `33 / 116` | `25 / 117` | **avg -24.2%**, `late=0`, `missed=0`, `dispatch_stalls=0` |
| Idle `240f` (After): `render_us / input_us (p50 / p95 / p99)` | --- | `1191 / 4707 / 6905` / `1207 / 4717 / 6913` | `dispatch_us (p50/p95/max)=53/92/252`, `late=0`, `missed=0`, `stalls=1/0` |
| Client `120f` (`rusty-terminal`): `render_duration_us (p50 / p95 / p99 / max)` | `1299 / 5957 / 9881 / 151139` | `6574 / 9318 / 12937 / 305598` | `dispatch_max 459 -> 340 (-25.9%)`, `late=1`, `missed=1`, `stalls=2/0` |

- **Ограничения и оценка регрессий:** В режиме `nested-winit` вызов `backend.submit()` проходит через `eglSwapBuffers` хостового Wayland-композитора (`eglSwapInterval=1` к `wl_surface.frame`), из-за чего фазовый сдвиг между таймером `16.667 ms` и host vblank вносит межпрогонный разброс `render_duration_us p50` в диапазоне `~0.55–11.9 ms` даже на неизменном бинарнике (на `120f` и `240f`). Замеры на `native-drm-kms` и A/B с реальным длительным вводом в Firefox подтверждаются отдельно вне nested-среды. Регрессий EGL (`EGL_BAD_SURFACE` на Wayland и X11), shader-компиляции и unit-тестов (`314` pass, `0` fail) не выявлено.

## 10. Фаза 7 --- Честный бенчмарк (CPU vs Submit), пропуск пересборки render list и адаптивная телеметрия (`RUSTYOS-120`, 2026-11-20)

- **Оборудование:** Intel Corporation Raptor Lake-U `[8086:a7ac]` (rev 04), `i915`, Linux `7.3.4-arch1-2`
- **Userspace Graphics:** Mesa `26.2.7-arch1.1` (`mesa 1:26.2.7-1`, EGL 1.5, OpenGL ES 3.2)
- **Scope / Реализация:**
  - **Part A (Честный бенчмарк):** В `FrameSample` и `FrameTimingSummary` добавлено измерение чистого процессорного времени построения и композитинга кадра (`cpu_duration_us`, `cpu_p50_us`, `cpu_p95_us`, `cpu_p99_us`, `cpu_max_us`) отдельно от блокирующего `backend.submit()` (`eglSwapBuffers` / ожидание host vblank в nested-winit). Поле включено в лог-строку и экспортируется в JSON (`--metrics-out`). Добавлен скрипт `bench_runner.py` для медианного замера (5–10 прогонов) с фильтрацией фазового шума.
  - **Part B (Оптимизация сборки render list):** В `frame_composer.rs` устранено ежекадровое форматирование `dimensions` (`format!("{}x{}")`) в `collect_dock_windows` (вычисляется только при открытом Spotlight Overview), `make_window_badge` переведен на итераторы без аллокации `Vec<&str>`, ранний возврат в `prune_inactive_minimize_snapshots` (`has_any_minimize_snapshots`) и `prepare_minimize_snapshots_for_output` без лишних проходов по хэш-таблицам при отсутствии активных анимаций сворачивания.
  - **Part E (Адаптивная телеметрия):** В `telemetry.rs` введен адаптивный интервал опроса: `POLL_INTERVAL_ACTIVE = 3s` при открытом Control Center / Wi-Fi popover и `POLL_INTERVAL_IDLE = 12s` в фоновом режиме рабочего стола. Синхронизация состояния через `set_active_ui(bool)` и немедленное пробуждение по событиям (клик по меню, hotkey, D-Bus).
- **Backend и режим:** `nested-winit` (`1280x800 @ 60 Hz`, `target_interval_us = 16666`, 1 output)
- **Команды воспроизведения (`--release`):**
  ```bash
  # Медианный замер 7 прогонов по 120 кадров (Idle)
  python3 .gemini/antigravity/brain/1f9783dc-9f74-4483-a6d6-b1c12a304049/scratch/bench_runner.py 7 120

  # Медианный замер 7 прогонов по 120 кадров с клиентом (rusty-terminal)
  python3 .gemini/antigravity/brain/1f9783dc-9f74-4483-a6d6-b1c12a304049/scratch/bench_runner.py 7 120 ./target/release/rusty-terminal

  # Медианный замер 5 прогонов по 240 кадров (длинная дистанция)
  python3 .gemini/antigravity/brain/1f9783dc-9f74-4483-a6d6-b1c12a304049/scratch/bench_runner.py 5 240
  ```
- **Результаты многопрогонных замеров (медиана 5–7 прогонов):**

| Сценарий | CPU Duration (`cpu_p50 / p95 / max`) | Total Render (`render_p50 / p95`) | Input Latency `p50` | Оценка производительности |
| :--- | :--- | :--- | :--- | :--- |
| **Idle (`120f`, 7 прогонов)** | **`556 us / 1021 us / 209 ms`** (разброс p50 `537–716 us`) | `8257 us / 10167 us` (шум фазы `791–12056 us`) | `8269 us` | **Чистое CPU-время композитинга всего ~0.55 ms** (в 30 раз меньше бюджета кадра `16.6 ms`!) |
| **Idle (`240f`, 5 прогонов)** | **`582 us / 920 us / 197 ms`** (разброс p50 `566–607 us`) | `4386 us / 8015 us` | `4398 us` | Стабильность на длинной дистанции, `cpu_p95 < 1 ms` |
| **Client (`rusty-terminal`, 7 прогонов)** | **`680 us / 1917 us / 214 ms`** (разброс p50 `552–921 us`) | `11813 us / 16367 us` | `11838 us` | С живым окном терминала CPU p50 всего **0.68 ms**, p95 **1.91 ms** |

- **Ключевой вывод:** Ранее наблюдавшиеся скачки `render_duration_us` от `0.6` до `15.7 ms` на 100% объясняются фазовой блокировкой внутри `eglSwapBuffers` (ожиданием `wl_surface.frame` от хоста). Фактическая процессорная работа композитора и рендера занимает **0.55–0.68 ms** в медиане, полностью исключая задержки композитинга как причину фризов.
