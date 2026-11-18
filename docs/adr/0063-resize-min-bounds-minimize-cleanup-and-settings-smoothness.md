# ADR-0063: Жёсткие минимальные границы resize окна, очистка минимизации/стекла и плавность `rusty-settings` (`RUSTYOS-112`)

## Status

accepted

## Context

При пользовательском тестировании были выявлены три взаимосвязанных UX- и рендер-дефекта:

1. **«Баунс» и проскальзывание окна при интерактивном уменьшении ниже минимального размера:** при перетаскивании левой или верхней границы resize (`ResizeDragEdges` через `compute_resized_rect`) минимальный размер ограничивался константой `(240, 160)`, а реальные ограничения клиента (`xdg_toplevel` `min_size`, например `(640, 500)` у `rusty-settings`, или `(320, 220)`) применялись только постфактум в `ensure_floating_min_size`, привязанной к `anchor = rect.(x, y)`, что вызывало рывки и съезжание противоположной границы окна.
2. **Визуальные артефакты минимизации и начального кадра:**
   - Снимок минимизации (`prepare_minimize_snapshots_for_output`) захватывал внешнюю мягкую тень окна (`WindowShadow`), обрезая её по границе `bounds` в тёмные клинья на углах и искажая альфа-канал.
   - `prepare_liquid_glass_backdrop` выполнялся уже после захвата minimize-снапшота, а полупрозрачный регион `Liquid Glass` (например сайдбар `rusty-files`) рендерился в координатном пространстве окна без смещения экрана, превращаясь во время минимизации в плоский серый квадрат.
   - Фрагментный шейдер [window_minimize.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/window_minimize.rs) выполнял двойное premultiplied-домножение `color.rgb * out_alpha` при одиночной выборке без префильтрации (вызывая тёмную кайму и лесенку на «хвосте»).
   - Связанные `wl_surface` с нулевым/отсутствующим буфером до первого `commit` всё равно рисовали пустую тень/хром окна.
3. **Задержки и рывки в `rusty-settings`:** переключение страниц и управляющие элементы синхронно вызывали `refresh_services()` (`nmcli`, `wpctl`, `pactl`) на главном потоке Wayland, скроллинг округлял дробные дельты с потерей субпиксельных шагов, `PointerEventKind::Axis` пересчитывал `layout()` дважды за событие, `poll_external_config()` вызывал `stat()` на каждом пробуждении цикла, а `fill_rect`/`fill_rounded_rect`/`stroke_rounded_rect` и иконки/превью пересчитывались попиксельно в полный кадр с полной площадью `damage_buffer`.

## Decision

1. **Единые пер-оконные минимальные границы resize без отскока (`rusty-wm`, `rusty-compositor`):**
   - В [crates/rusty-wm/src/lib.rs](file:///home/anc/RustyOS_Main/crates/rusty-wm/src/lib.rs) добавлен метод `WindowManager::resize_window_with_min(&mut self, id, rect, min_w, min_h) -> bool` (с делегированием `resize_window` к `(240, 160)`).
   - В [crates/rusty-compositor/src/state/wm_windows.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/state/wm_windows.rs) добавлены `floating_min_bounds` и `effective_window_min_size`, учитывающие клиентский `min_size` / плавающий минимум до применения размера.
   - В [crates/rusty-compositor/src/input/pointer.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/input/pointer.rs) добавлена функция `compute_resized_rect_with_min`, закрепляющая противоположную кромку при упирании в `(min_w, min_h)`.

2. **Корректный захват minimize-снапшота, `Liquid Glass` и premultiplied-шейдер (`crates/rusty-compositor`):**
   - В [crates/rusty-compositor/src/frame_composer.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/frame_composer.rs) выделена функция `push_window_elements_with_options`: снимок минимизации строится с `include_shadow = false` и экранным `glass_backdrop_offset` после `build_backdrop_render_elements` и `prepare_liquid_glass_backdrop`; для ещё не закоммиченных клиентов (`should_skip_uncommitted_bound_window`) пустой хром и тень не рисуются.
   - В [crates/rusty-compositor/src/surfaces/liquid_glass.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/liquid_glass.rs) и [crates/rusty-compositor/src/surfaces/mod.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/mod.rs) добавлены `make_glass_element_with_backdrop_loc` и `glass_panel_render_element_at` для раздельной привязки экранного backdrop при рендере в offscreen FBO.
   - В [crates/rusty-compositor/src/surfaces/window_minimize.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/window_minimize.rs) шейдер `WINDOW_MINIMIZE_FRAG` переведён на premultiplied `color * mask`, 4-tap rotated-grid префильтрацию `sample_filtered`, пиксельное сглаживание краёв через `u_bounds_px` (`fw_a`/`fw_c`) и плавное затухание `smoothstep(0.76, 0.99, t)`.

3. **Асинхронный worker сервисов, субпиксельный/плавный скролл и оптимизация CPU-рендера (`apps/rusty-settings`):**
   - В [apps/rusty-settings/src/state.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/state.rs) и [apps/rusty-settings/src/wayland.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/wayland.rs) опрос и команды `NetworkService` / `AudioService` вынесены в фоновый коалесцирующий поток (`execute_coalesced_service_batch` + `calloop::ping::make_ping`), добавлены субпиксельный накопитель и плавная 60/120 Hz анимация скролла `scroll_smooth_by` / `step_scroll_animation` (с мгновенным переходом при `reduced_motion`), троттлинг `poll_external_config_if_due` (`200 ms`) и точечный `wl_surface::damage_buffer` через `DamageScope`.
   - В [apps/rusty-settings/src/render.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/render.rs) добавлены быстрые сканлайн-заливки `blend_span_opaque` + `rounded_interior_span`, отсечение невидимых строк по `self.clip` и кэш premultiplied BGRA-спрайтов `SPRITE_CACHE` для иконок сайдбара, сервисных бэджей и карточек превью тем (`blit_cached_sprite`).

## Consequences

- Интерактивное уменьшение окна останавливается жёстко на его реальном минимальном размере без смещения противоположной грани и баунса.
- Минимизация и восстановление окон протекают без тёмных углов тени, плоского серого прямоугольника на местах `Liquid Glass`, артефактов на краях и фантомной рамки до первого кадра клиента.
- `rusty-settings` сохраняет нулевую блокировку Wayland-цикла при навигации и переключении сервисов, плавно скроллится и снижает нагрузку на CPU и композитор без добавления новых зависимостей в `Cargo.toml`.
