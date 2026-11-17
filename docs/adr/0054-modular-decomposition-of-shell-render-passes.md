# ADR-0054: Modular Decomposition of `crates/rusty-compositor/src/shell_render` Render Passes

## Status

accepted

## Context

После выноса подмодулей `top_bar/` (`RUSTYOS-090`) и `modals/` (`RUSTYOS-091`) корневой файл `crates/rusty-compositor/src/shell_render/mod.rs` оставался монолитом на `2014` строк, объединяя фасад публичных реэкспортов, единый тип `CompositorRenderElement`, `ShellOverlayView` и четыре крупных конвейера генерации элементов кадра:
1. Текстовый проход (`append_foreground_overlay_text`, `generate_shell_text_elements`, `append_chrome_text`).
2. Проход геометрии переднего плана (`generate_shell_foreground_overlay_elements`, `build_foreground_overlay_shapes`).
3. Проход базового хрома (`TopBar`, `Dock`) и `Workspace Overview` (`generate_shell_chrome_composite_elements`, `generate_shell_overview_elements`, `build_chrome_shapes`, `render_dock_tile_icon`, `push_dock_tooltip_and_drag`, `push_dock_chrome_shapes`, `push_dock_slot_shape`, `build_overview_shapes`).

## Decision

Разбить `crates/rusty-compositor/src/shell_render/mod.rs` без изменения публичных сигнатур и поведения:
1. `crates/rusty-compositor/src/shell_render/text_pass.rs` (`595` строк) — `append_foreground_overlay_text`, `generate_shell_text_elements`, `append_chrome_text`.
2. `crates/rusty-compositor/src/shell_render/overlay_shapes.rs` (`398` строк) — `generate_shell_foreground_overlay_elements`, `build_foreground_overlay_shapes`.
3. `crates/rusty-compositor/src/shell_render/chrome_shapes.rs` (`759` строк) — `generate_shell_chrome_composite_elements`, `generate_shell_overview_elements`, `build_chrome_shapes`, `push_dock_chrome_shapes`, `push_dock_slot_shape`, `push_dock_tooltip_and_drag`, `render_dock_tile_icon`, `build_overview_shapes`.
4. `crates/rusty-compositor/src/shell_render/mod.rs` (`329` строк) — чистый фасад реэкспортов, `CompositorRenderElement`, `ShellOverlayView`, `generate_shell_render_elements`, `generate_shell_composite_elements` и `#[cfg(test)] mod tests;`.

## Consequences

- `shell_render/mod.rs` сокращён с `2014` до `329` строк (`-83.7%`), каждый проход рендера изолирован в отдельном файле с явными импортами (без wildcard-импортов согласно `clippy::pedantic`).
- 100% публичного API `rusty_compositor::shell_render` и все `251` тестов воркспейса сохранены без изменений.
