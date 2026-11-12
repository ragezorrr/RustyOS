# ADR-0004: Refresh-Rate Independent Animation Engine (`crates/rusty-animation`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

В соответствии с `ROADMAP.md` (Фаза 5 --- Animation Engine), `ARCHITECTURE.md` (раздел 2, Animation Engine) и `DESIGN.md` (раздел 3–4, Motion tokens и accessibility) проекту RustyOS необходим движок анимаций, обеспечивающий плавные переходы в духе macOS без привязки к фиксированным 60 FPS:
- единая монотонная шкала времени (`Duration` от старта композитора или `Instant`), независимая от частоты обновления монитора (60 / 120 / 144 / 240 Hz) и устойчивая к пропускам кадров;
- кривые сглаживания (`Easing`: `Linear`, `EaseOutCubic`, `EaseInOutCubic`, `SpringMacOs`);
- поддержка прерывания (`interrupt` / `retarget`), разворота (`reverse`) и отмены (`cancel`) переходов в полёте без скачков координат;
- первоклассная поддержка режима `reduced_motion` (мгновенное или сокращённое применение целевого состояния без поломки UX);
- отсутствие блокирующих операций и аллокаций на горячем пути кадра.

Добавление нового крейта `crates/rusty-animation` в Cargo workspace меняет границы модулей и требует явного согласования (Approval Gate).

## Рассмотренные варианты

### Вариант A: Чистый `std`-only крейт `crates/rusty-animation` + интеграция с `rusty-compositor` (Рекомендуемый)

1. **Крейт `crates/rusty-animation` (`std`-only, `unsafe_code = "deny"`, 0 внешних зависимостей):**
   - `MotionTokens`: стандартные длительности из `DESIGN.md` (`short_ms = 120`, `standard_ms = 180`, `long_ms = 260`, `reduced_motion: bool`).
   - `Easing`: математические кривые (`Linear`, `EaseOutCubic`, `EaseInOutCubic`, `EaseOutQuart`) с гарантией монотонности на `[0.0, 1.0]`.
   - `AnimatedValue` (`f64`) и `AnimatedRect` (`x, y, width, height`):
     - хранят `start_val`, `current_val`, `target_val`, `start_time: Duration`, `duration: Duration`, `easing: Easing`;
     - метод `retarget(new_target, now, duration, easing)` плавно перенаправляет активную анимацию из текущей промежуточной точки (`current_val`) к новой цели без рывка;
     - метод `reverse(now)` разворачивает активный переход обратно к исходной точке с сохранением пройденной доли времени;
     - метод `cancel_at_current()` / `snap_to_target()` немедленно завершает анимацию;
     - при `reduced_motion = true` любой `retarget` мгновенно устанавливает `current_val = target_val` (`completed`).
   - `AnimationEngine`: управляет переходами геометрии окон (`WindowId -> AnimatedRect`) и смещением при переключении рабочих столов (`workspace_slide_offset_x: AnimatedValue`), принимает монотонный `now: Duration` на каждом кадре любого refresh rate (`60/120/144/240 Hz`).
2. **Интеграция в `crates/rusty-compositor` (`RUSTYOS-008`):**
   - В `RustyCompositorState` добавляется `animations: AnimationEngine`.
   - При `sync_space_with_wm` (snap, maximize, tiling, смена workspace) `xdg_toplevel::configure` сразу получает целевой размер `(rect.width, rect.height)` (чтобы клиент не пересоздавал буферы на каждом промежуточном кадре), а позиция окна в `Space<Window>` плавно интерполируется через `AnimationEngine::tick(elapsed)` на каждом кадре.
   - Хоткей `Alt+R` переключает режим `reduced_motion` в рантайме, а CLI-флаг `--reduced-motion` позволяет запускать композитор сразу в режиме доступности.

- **Плюсы:**
  - Полная детерминированность и тестируемость на любых частотах (`60/120/144/240 Hz`) и при пропусках кадров без GPU/Wayland.
  - Отсутствие `configure`-шторма для Wayland-клиентов (клиент рендерит в целевой размер, а композитор анимирует координаты и переходы).
  - Нулевые внешние зависимости.
- **Минусы:**
  - На этапе 5 анимируются координаты/переходы окон и рабочих столов (`Space::map_element`), тогда как нелинейное масштабирование текстур на GPU (`scale matrix`) остаётся для этапа кастомного рендерера/эффектов.

### Вариант B: Встроить код анимаций прямо внутрь `crates/rusty-compositor`

- **Плюсы:** Меньше крейтов в workspace.
- **Минусы:** Нарушает целевую структуру `ARCHITECTURE.md` (`crates/rusty-animation`), где движок анимаций должен переиспользоваться также будущим `rusty-shell` (Фаза 6: панель, док, лаунчер, уведомления).

### Вариант C: Использовать стороннюю библиотеку анимаций/пружин из crates.io

- **Плюсы:** Готовые примитивы.
- **Минусы:** Лишняя внешняя зависимость ради ~250 строк чистой математики; отсутствие работы в `--offline` без предварительного кэширования.

## Решение

Выбран **Вариант A**: создать `std`-only крейт `crates/rusty-animation` и интегрировать его в `crates/rusty-compositor` (задача `RUSTYOS-008`).

## Последствия

- Анимации окон и рабочих столов работают одинаково по времени на 60, 120, 144 и 240 Hz.
- Будущий `rusty-shell` (Фаза 6) сможет напрямую использовать `crates/rusty-animation` и `MotionTokens`.
- В `TASKS.md` добавляется задача `RUSTYOS-008`.
