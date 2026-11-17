# ADR-0052: CutefishOS Exact Superellipse Lens, Directional Rim, and Backdrop Calibration

## Status

accepted

## Context

При визуальном сопоставлении живого Дока (`RUSTYOS-098`) с эталоном CutefishOS выявлены четыре ключевых отличия в пайплайне Liquid Glass:
1. **Круговая дуга против кубической суперэллипсы:** в `LIQUID_GLASS_FRAG` использовался круговой SDF (`length(max(q, 0.0)) - radius`), тогда как в CutefishOS (`rounded_clip.glsl`, `rounded_shape.h`) используется кубическая суперэллипса `x^3 + y^3 <= 1` с протяжённостью `r_ext = clamp(r * 1.5, 0.0, halfSize)` и 4-точечным субпиксельным покрытием.
2. **Всенаправленный белый кант 360° против 3-слойного направленного `uRim`:** равномерный `hairline` давал заметную белую обводку на левом и правом торцах. В CutefishOS (`glass_fragment_shader.glsl`) внешний 1px контур слегка затемняется (`shade`), а внутренняя 1px нить (`hairline`) и мягкий `glow` модулируются по вертикальной нормали `lit = abs(outward.y) * mix(1.0, 0.35, 0.5 + 0.5 * outward.y)` (верх `1.0`, низ `0.35`, бока `0.0`).
3. **Пять проходов Dual-Kawase против мягкого преломления `blurSigma = 1.0px`:** использование глубоко размытого `buffers.half_out` и `refract = 0` превращало Док в матовую плашку вместо преломляющей суперэллиптической 14px линзы (`displacement = 36`, `ior = 1.7`, `profile = 2.5`, `chroma = 0`, `specular = 0`, `sheen = 0`).
4. **Паразитный темный перепад в процедурных обоях:** третья диагональная складка `sheet3` в `apply_golden_gate_folds` (`sdf.rs`) пересекала область Дока и создавала ложное ощущение тени на фоне.

## Decision

1. **`LIQUID_GLASS_FRAG` (`crates/rusty-compositor/src/surfaces/liquid_glass.rs`):**
   - Внедрены `cornerExtent`, `cornerNormal`, `roundedDistance` (без деноминаторного андерфлоу в `mediump`), `tangentCoverage`, 4-точечный `roundedCoverage`, `outwardAt`, `insideLens`, суперэллиптический профиль `lensHeight` (`bevel = 14`, `depth = 16`, `profile = 2.5`), `lensNormalAt`, `lensOffset` (`ior = 1.7`) и 4-точечный RGSS-сэмплер `refractedDockAt`.
   - Для режима Дока (`dock_mode = step(u_sat_refract.w, 0.0001)`) применён точный 3-слойный `uRim` из `glass_fragment_shader.glsl` (внешний 1px `shade`, сдвинутый внутрь `hairline` + `glow`, `lit = 1.0 / 0.35 / 0.0`).
   - В `compute_glass_material` установлены калиброванные параметры CutefishOS (`Light`: `[0.60, 0.60, 0.60, 0.13 + 0.12 * g]`, `Dark`: `[0.38, 0.38, 0.38, 0.15 + 0.14 * g]`, `sat_refract_rim_sheen = [1.30, 36.0, 1.0, 0.0]`).
   - В `make_glass_element` Док переключён на буфер первого прохода `buffers.half_a` (`sigma ~ 1.0px`), сохраняя 5-проходный `buffers.half_out` для `Popover` и `ControlModule`.
2. **Процедурные обои и CPU fallback (`crates/rusty-compositor/src/surfaces/sdf.rs`, `tests.rs`):**
   - Переход `sheet3` в `apply_golden_gate_folds` расширен и осветлён, устраняя тёмную диагональную полосу за Доком.
   - Синхронизирован CPU fallback в `base_glass_layer` и `dock_rim_alpha` (через общий `outward_normal_2d`), обновлены тесты `dock_glass_and_icon_shadow_rasterize_cleanly` и `liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive`.

## Consequences

- Док в RustyOS воспроизводит геометрию кубической суперэллипсы, 14px краевую рефракцию без белых пластиковых бликов и чистый направленный кант CutefishOS без боковой белой обводки и темной полосы на фоне.
- Zero extra FBO allocations: используется уже существующий буфер `half_a`.
