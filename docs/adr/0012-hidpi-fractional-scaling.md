# ADR-0012: HiDPI и дробное масштабирование (`wp_fractional_scale_v1` + `wp_viewporter`) (`RUSTYOS-043`)

- **Статус:** accepted
- **Дата:** 2026-11-12
- **Автор:** AI Agent + User

## Контекст

`OutputEntry` уже хранил поле `scale: f64` и вычислял `logical_geometry()`, однако сквозной путь HiDPI в композиторе оставался зафиксированным на `1.0`:
1. В `RustyCompositorState` отсутствовали Wayland-протоколы `wp_fractional_scale_v1` и `wp_viewporter`.
2. `winit_backend.rs` и `drm_backend.rs` передавали `Scale::Integer(1)` / `Scale::from(1.0)` в рендер элементов, damage/offscreen и `add_output`, игнорируя реальный `output.scale`.
3. В `UserConfig` (`[display]`) отсутствовали настройки базового и per-connector масштаба.

## Рассмотренные варианты

- **Вариант A (Выбран):** Зарегистрировать встроенные в Smithay `0.7` `ViewporterState` (`wp_viewporter`) и `FractionalScaleManagerState`/`FractionalScaleHandler` (`wp_fractional_scale_v1`), рассылать `preferred_scale` и integer `preferred_buffer_scale` (`ceil`) всем поверхностям при создании, коммите и перемещении между выходами, добавить `[display]` `scale` и `scales` / `[display.scales]` в `UserConfig`, пересчитывать логическую геометрию и `usable_work_area` через `update_output_geometry`, а также масштабировать shell/SSD-элементы и Wayland-поверхности через `scale_render_elements_for_output` и реальный `output_scale` в Winit, DRM/KMS и скриншотах.
- **Вариант B:** Только Wayland-протоколы с целочисленным `1 | 2` scale в бэкендах (не закрывает 125%/150% ноутбуки и mixed-DPI мониторы).
- **Вариант C:** Авто-DPI только из EDID без `[display]`-конфига и полная перестройка кэшей шрифтов/фигур (ненадёжный EDID на многих экранах и избыточный риск).

## Решение

Утверждён **Вариант A** (`APPROVE:` пользователя от 2026-11-12).

## Последствия

- Wayland-клиенты получают точный дробный масштаб через `wp_fractional_scale_v1` и согласованный `wp_viewporter`, а legacy-клиенты получают integer `ceil(scale)` через `wl_surface::preferred_buffer_scale`.
- Shell UI, SSD-рамки, layer-поверхности, курсор, скриншоты и mixed-DPI раскладка в `OutputMap` синхронизируются с логической геометрией каждого выхода при ресайзе/переносе Winit-окна и в DRM/KMS (`add_output`/`update_output_geometry`).
- Требует последующего визуального и bare-metal подтверждения на реальных HiDPI/mixed-DPI мониторах по `HARDWARE.md`.
