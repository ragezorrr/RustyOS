# ADR-0024: Display Output Rotation (`0°`/`90°`/`180°`/`270°`) & Pointer Coordinate Mapping (`RUSTYOS-055`)

- **Статус:** accepted
- **Дата:** 2026-11-13
- **Автор:** AI Agent + User

## Контекст

В `rusty-compositor` выходы ранее создавались с фиксированными ориентациями: вложенный `winit-0` использовал `Transform::Flipped180` исключительно для компенсации Y-оси OpenGL FBO (чтобы изображение в окне оставалось прямым `0°`), а DRM/KMS-коннекторы всегда получали `Transform::Normal`. Отсутствовали:
- настройки ориентации в [`UserConfig`](../../crates/rusty-compositor/src/config.rs) (`[display]`), включая per-output переопределения;
- CLI-переопределение `--rotate <0|90|180|270>` в [`CompositorConfig`](../../crates/rusty-compositor/src/lib.rs);
- учёт поворота в [`OutputEntry`](../../crates/rusty-compositor/src/outputs.rs) (пересчёт логических размеров при `90°`/`270°`, размер захвата скриншота и перекладка `WM`/multi-monitor);
- обратное преобразование абсолютных координат указателя (`Winit` / тачскрин) в [`handle_pointer_motion_absolute`](../../crates/rusty-compositor/src/input.rs).

## Рассмотренные варианты

- **Вариант 1 (Выбран):**
  - Ввести [`DisplayRotation`](../../crates/rusty-compositor/src/config.rs) (`Rotate0`, `Rotate90`, `Rotate180`, `Rotate270`, дефолт `0°`) с парсингом чисел и алиасов (`0`/`normal`, `90`, `180`/`inverted`, `270`), общим ключом `[display].rotation`, таблицами `[display].rotations` / `[display.rotations]` и секциями `[display.outputs."<NAME>"]`.
  - Добавить CLI `--rotate <DEG>` в `CompositorConfig` (`parse_args`) и прокинуть `rotation_override` в `RustyCompositorState`, `winit_backend` и `drm_backend`.
  - Для DRM/KMS применять `DisplayRotation::to_wl_transform()` (`Normal`, `_90`, `_180`, `_270`); для Winit GL FBO применять `DisplayRotation::to_winit_transform()` (`Flipped180`, `Flipped90`, `Flipped`, `Flipped270`), чтобы итоговая матрица `flip180 * output.invert()` в `GlesRenderer` и `OutputDamageTracker` совпадала с DRM.
  - В `outputs.rs` и `state.rs` добавить `compute_logical_size_for_rotation`, `OutputEntry::with_rotation`, `unrotated_logical_size`, `transformed_physical_size`, `set_output_rotation` и общий helper перекладки `relayout_wm_for_all_outputs`.
  - В `input.rs` добавить `transform_screen_point_to_output_logical` и использовать его в `handle_pointer_motion_absolute`.
- **Вариант 2:** Ограничиться временным флагом `--rotate-180` только в CLI без поддержки `config.toml` и `90°`/`270°`. Отклонён как одноразовое решение, которое придётся переделывать для постоянной конфигурации и портретных экранов.
- **Вариант 3:** Изменить дефолтную трансформацию `winit-0` напрямую в `state.rs`. Отклонён, так как ломает нормальную ориентацию по умолчанию, не покрывает DRM и не решает преобразование указателя.

## Решение

Утверждён **Вариант 1** (`APPROVE: RUSTYOS-055 Вариант 1` от 2026-11-13).

## Последствия

- `rusty-compositor` и `rustyos-session` поддерживают как постоянную конфигурацию ориентации в `~/.config/rustyos/config.toml` (глобально и per-connector), так и быстрый флаг `--rotate 0|90|180|270` при запуске.
- Все 4 ориентации работают без новых внешних Rust-зависимостей и без `unsafe`; при несовместимости аппаратных плоскостей DRM с поворотом на конкретном GPU Smithay `DrmCompositor` автоматически использует GLES-композитинг.
