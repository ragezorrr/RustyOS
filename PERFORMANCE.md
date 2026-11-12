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
