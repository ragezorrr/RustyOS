# ADR-0034: GLES Dual-Kawase Backdrop Blur и Liquid Glass шейдеры

- **Статус:** accepted
- **Дата:** 2026-11-14
- **Задача:** `RUSTYOS-069`
- **Связанные решения:** `D-042`, `ADR-0023`, `ADR-0032`

## Контекст

После перехода на темы `Golden Gate Light & Dark` (`RUSTYOS-067`) аудит против референсов `docs/design/references/macos-27-golden-gate/` показал ключевое визуальное расхождение: контейнеры оболочки (`Dock`, `Popover` меню, `Control Center`, `ControlModule`, `Spotlight`, `Overview`, `Window Switcher`, `Notification` и `OSD`) оставались CPU-растеризованной полупрозрачной заливкой с Байеровским дитерингом без реального размытия фона (backdrop blur) и краевого преломления Liquid Glass. Дополнительно в `apps/rusty-files/src/views.rs` и `DESIGN.md §12` оставались точечные следы прежней фиолетовой темы.

## Решение

1. **Безопасный zero-`unsafe` GLES Dual-Kawase пайплайн (`crates/rusty-compositor/src/surfaces/liquid_glass.rs`):**
   - Используются исключительно публичные безопасные API Smithay `0.7.0` (`GlesRenderer::compile_custom_texture_shader`, `Offscreen::create_buffer`, `Bind::bind`, `OutputDamageTracker`, `TextureRenderElement::from_texture_with_damage`, `TextureShaderElement`, `DamageBag`) при сохранении workspace-политики `unsafe_code = "deny"`.
   - Реализованы три GLSL ES 1.00 (`#version 100`) фрагментных шейдера с поддержкой маркеров Smithay (`//_DEFINES_`, `EXTERNAL`, `NO_ALPHA`, `DEBUG_FLAGS`):
     - `KAWASE_DOWN_FRAG` (5-tap downsample с безопасным UV-clamp),
     - `KAWASE_UP_FRAG` (8-tap upsample),
     - `LIQUID_GLASS_FRAG` (аналитический rounded-box SDF, градиентное краевое оптическое преломление/линзирование и мягкая микродисперсия, boost насыщенности фона, Golden Gate Light/Dark tint для `Dock`/`Popover`/`ControlModule`, верхний блик sheen и направленный specular bevel rim).
   - Выполнение размытия строится по 5-проходной пирамиде (`scene -> 1/2 -> 1/4 -> 1/8 -> 1/4 -> 1/2`).
2. **Damage-gating и HiDPI-интеграция (`frame_composer.rs`, `shell_render/mod.rs`, `shell_render/tokens.rs`):**
   - В `build_frame_render_list_for_output_with_cursor` сначала собираются и масштабируются элементы заднего плана (`build_backdrop_render_elements`: окна + SSD + тени + `Layer::Bottom/Background` + обои).
   - `OutputDamageTracker` рендерит этот фон в `scene_tex` с учётом age/damage: при отсутствии изменений за стеклом (`damage == None`) все 5 Dual-Kawase проходов пропускаются (`0` GPU-проходов на idle-кадре).
   - `LiquidGlassElement` оборачивает `TextureShaderElement` со стабильным per-slot `Id` (обновляемым при изменении параметров панели/материала) и расширенным `DamageBag` (`+32 px` padding в `half_out`).
   - В `CompositorRenderElement` добавлены варианты `ShaderTexture` / `ScaledShaderTexture`, масштабируемые в `scale_render_elements_for_output`.
3. **Изоляция блокировки и runtime CPU fallback:**
   - При `screen_locked`, `ext_session_locked`, `lock_progress > 0.01`, `reduce_transparency = true`, скрытом хроме полноэкранного приложения или ошибке компиляции/выделения FBO подготовка GL-бэкдропа не выполняется, и `UiShapeCache::glass_panel_render_element` / `dock_glass_render_element` прозрачно возвращают прежние CPU-буферы `MemoryRenderBufferRenderElement`.
4. **Зачистка остатков purple и обновление документации/скиллов:**
   - В `apps/rusty-files/src/views.rs` удалены два оставшихся фиолетовых RGBA (`rgba(38, 29, 54, 255)` и `rgba(148, 134, 248, 255)`).
   - Обновлены `DESIGN.md §12`, комментарий в `surfaces/tests.rs` и скиллы `.agents/skills/{ui-ux-design,desktop-shell,gpu-rendering}/SKILL.md`.

## Последствия

- `Dock`, меню, `Control Center`, `ControlModule`, `Spotlight`, `Notifications`, `Window Switcher` и `OSD` получают реальный GPU backdrop blur с краевой оптикой Liquid Glass на `Light` и `Dark` темах Golden Gate.
- Композитор сохраняет 100% работоспособность без GLES FBO (headless/тесты/`reduce_transparency`) и не смешивает открытые окна в стекло экрана блокировки.
- Ценой являются 6 offscreen-текстур на активный output (`1x`, `2×1/2`, `2×1/4`, `1×1/8` ABGR8888), обновляемых только при наличии backdrop damage.
