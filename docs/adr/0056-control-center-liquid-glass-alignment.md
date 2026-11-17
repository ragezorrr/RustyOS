# ADR-0056: Выравнивание Control Center под единый Liquid Glass (`ControlModule`)

## Status

accepted

## Context

После калибровки единого шейдера `LIQUID_GLASS_FRAG` (`ADR-0052`, `ADR-0053`) острова `Control Center` ([crates/rusty-compositor/src/shell_render/quick_settings.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/quick_settings.rs)) заметно отставали от `Dock` и целевого визуального ориентира:

1. Материал `GlassPanelStyle::ControlModule` имел заниженное краевое преломление и блик (`sat = 1.22`, `displacement = 24.0`, `rim = 0.90`) с более тяжёлой/меловой подложкой в Light-теме, из-за чего белые активные треки слайдеров и кружки иконок теряли контраст, а островки читались тусклее `Dock`. При тестировании более резкого `half_a` ярлыки рабочего стола за `Not Playing` / `Overview` начинали просвечивать слишком резко, мешая переднему плану.
2. Карточки `Display` и `Sound` использовали прямоугольное скругление вместо полных капсул `r = h / 2`, бегунки слайдеров не имели круглого белого thumb-бегунка, кружки иконок/мута и обложка `Now Playing` имели лишние рамки, а активные круглые тумблеры перекрывали внешний `uRim` стекла.
3. Подписи капсул (`Wi-Fi`, `Bluetooth`, `Power`, `Focus`) на `pal.popover_text_muted` проваливались по контрасту поверх светлого матового стекла, а IPC `LaunchCommand` не умел открывать `Control Center` для автоматизированной `--winit`-проверки.

## Decision

1. **Калибровка `GlassPanelStyle::ControlModule` (GPU + CPU fallback):**
   - В [crates/rusty-compositor/src/surfaces/liquid_glass.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/liquid_glass.rs) (`compute_glass_material`, `make_glass_element`): использовать сглаженный `buffers.half_out` (исключая проступание резкого текста иконок рабочего стола сквозь модули) при чёткой оптике `[1.30, 34.0, 1.0, 0.01]` (`sat = 1.30`, `displacement = 34.0`, `rim = 1.0`, `chroma = 0.01`), светлой нейтрально-серебристой тонировке `[0.72, 0.74, 0.78, 0.11 + 0.11 * g]` и тёмной `[0.24, 0.24, 0.27, 0.15 + 0.15 * g]`.
   - В [crates/rusty-compositor/src/surfaces/sdf.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/sdf.rs) (`base_glass_layer`, `sample_glass_panel_pixel_themed`): согласовать CPU/fallback-подложку Light `[184.0, 188.0, 198.0]` (`0.22 * (0.48 + 0.52 * glass_scale)`, `reduce_transparency` `[238.0, 240.0, 245.0]`), Dark `[60.0, 60.0, 68.0]` (`0.28 * ...`) и `rim_peak` `0.42` / `0.35`.

2. **Обновление контролов и типографики Control Center ([crates/rusty-compositor/src/shell_render/quick_settings.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/shell_render/quick_settings.rs)):**
   - Слайдерные карточки `Display` и `Sound` переведены на полную капсулу (`r = card.size.h / 2 = 29`) и единую тень островов `Some((18, 42))`.
   - Слайдер `push_cc_thin_slider` рендерит тонкий `5 px` белый активный трек с круглым `12 px` белым thumb-бегунком (`[0, 0, 0, 34]` микроободок), кнопка mute в `push_cc_sound_slider` и кружки капсул (`push_cc_capsule_icon`), а также `art_rect` (`r = 10`) в `push_cc_now_playing_contents` очищены от грубых рамок.
   - Активный индикатор круглых тумблеров (`push_cc_round_toggle_buttons`) смещён на `7 px` внутрь, чтобы не перекрывать стеклянный `uRim`.
   - В `render_control_center_capsule_text` подписи активных капсул используют `pal.popover_text_secondary`, а заголовок неактивных — `pal.popover_text_secondary`, повышая читаемость в Light-режиме.

3. **IPC и документация:**
   - В [crates/rusty-compositor/src/state/wm_windows.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/state/wm_windows.rs) (`handle_launch_command_ipc`) добавлена обработка `"control-center" | "quick-settings"` -> `self.toggle_quick_settings()`.
   - Обновлены разделы `§12.4` и `§12.5` в [DESIGN.md](file:///home/anc/RustyOS_Main/DESIGN.md).

## Consequences

- `Control Center` визуально и оптически согласован с `Dock` в `Light` и `Dark` темах: белые слайдеры, thumb-бегунки, иконки и подписи отчётливо читаются поверх преломляющего стекла, без резкого просвечивания фоновых ярлыков и без артефактов границ.
- Изменения покрыты unit-тестами (`liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive`, `macos27_quick_settings_glass_and_volume_slider_hit_testing`; 256 тестов workspace пройдены, `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --all -- --check` чисты) и живыми `--winit`-прогонами в Light и Dark режимах без GLES/shader-ошибок.
