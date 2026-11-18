# ADR-0062: Антиалиасинг `16-tap R2` в линейном свете, `2x HiDPI`-буферы и `Lanczos-3` сжатие PNG для иконок и глифов (`RUSTYOS-111`)

## Status

accepted

## Context

При проверке чёткости иконок оболочки и системных приложений обнаружены 4 причины «лесенки» (aliasing):

1. Процедурные скворклы ([icons.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/icons.rs)), UI-глифы ([glyphs.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/glyphs.rs)) и аватарки / traffic lights ([sdf.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/sdf.rs)) использовали регулярную ортогональную решётку `4×4` (`[0.125, 0.375, 0.625, 0.875]`) с усреднением в гамма-кодированном пространстве, из-за чего близкие к горизонтали/вертикали границы и диагонали распадались на 4 ступеньки с серой каймой.
2. Буферы иконок и UI-глифов создавались со scale `1` (`upload_bgra_buffer`, `rasterize_ui_glyph_buffer_rect`), теряя резкость при magnification в `Dock` и на `HiDPI`/дробных позициях.
3. В [image_decode.rs](file:///home/anc/RustyOS_Main/crates/rusty-services/src/image_decode.rs) `scale_rgba_rect_to_premul_bgra` использовал одиночный билинейный сэмпл при уменьшении крупных `.png`-иконок приложений и `SNI`-трея.
4. Клиентские окна [apps/rusty-settings/src/render.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/render.rs) и [apps/rusty-updater/src/render.rs](file:///home/anc/RustyOS_Main/apps/rusty-updater/src/render.rs) сэмплировали скворклы, кольца, сегменты и эмблемы в 1 точке `(px + 0.5, py + 0.5)` без субпиксельного SSAA.

## Decision

1. **Единый модуль субпиксельной выборки в линейном свете (`crates/rusty-compositor/src/aa.rs`):**
   - Добавлена 16-точечная последовательность `R2_TAPS_16` (без повторяющихся строк/столбцов) и аккумулятор `LinearAccum` (`srgb_to_linear` / `linear_to_srgb` с premultiplied `BGRA` выводом).
   - На `R2_TAPS_16` и `LinearAccum` переведены `sample_builtin_squircle_pixel` ([icons.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/icons.rs)), `sample_ui_glyph_pixel_rect` ([glyphs.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/glyphs.rs)) и `sample_fallback_avatar_pixel` / `sample_traffic_light_pixel` ([sdf.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/surfaces/sdf.rs)).

2. **`2x HiDPI`-буферы для иконок и UI-глифов композитора:**
   - Введены `GLYPH_BUFFER_SCALE = 2`, растеризация `rasterize_ui_glyph_buffer_rect` и `rasterize_builtin_squircle_buffer` в физическом разрешении `2x` (`scale = 2`) при сохранении прежнего логического размера (`size_px`).
   - В `ensure_named_buffer` / `load_png_as_bgra_scaled` ([icons.rs](file:///home/anc/RustyOS_Main/crates/rusty-compositor/src/icons.rs)) внешние `.png`-иконки с достаточным разрешением (`>= 2 * logical_size`) также декодируются в `2x` буфер через `upload_bgra_buffer_scaled`.

3. **Сепарабельный `Lanczos-3` downsampling в линейном свете (`crates/rusty-services/src/image_decode.rs`):**
   - При уменьшении (`dst_w < src_w || dst_h < src_h`) `scale_rgba_rect_to_premul_bgra` использует `downscale_lanczos_linear` (`lanczos_axis_weights`, sRGB ↔ linear LUT с premultiplied цветами), обслуживающий и внешние `.png`, и `argb32_be_to_premul_bgra` для `SNI` tray.

4. **4-tap `RGSS` (`aa_cov8`) в `apps/rusty-settings` и `apps/rusty-updater`:**
   - В [apps/rusty-settings/src/render.rs](file:///home/anc/RustyOS_Main/apps/rusty-settings/src/render.rs) и [apps/rusty-updater/src/render.rs](file:///home/anc/RustyOS_Main/apps/rusty-updater/src/render.rs) добавлена функция `aa_cov8` (4-tap rotated grid с быстрым отсечением полностью внутренних/внешних пикселей) для `fill_rounded_rect`, `stroke_rounded_rect`, `fill_circle`, `fill_gradient_squircle`, `draw_segment`, `draw_wifi_arc`, `draw_updater_emblem` и `draw_refresh_icon`.

## Consequences

- Иконки `Dock` (включая magnification при наведении), `TopBar`, уведомлений, сторонних `.png`-приложений, `SNI`-трея, а также эмблемы и скворклы в `rusty-settings` и `rusty-updater` рендерятся плавно без лесенки и без новых внешних зависимостей (`Cargo.toml`).
- После однократной растеризации в `IconCache` / `UiShapeCache` нагрузка в render-цикле нулевая.
