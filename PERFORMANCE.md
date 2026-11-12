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
