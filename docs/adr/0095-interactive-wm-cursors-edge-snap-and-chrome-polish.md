# ADR-0095: Interactive WM Cursor Overrides, Unmaximize-on-Drag, Edge/Corner Snap, Resize Anchor-Lock, and SSD Chrome Polish (`RUSTYOS-166`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-109`
- **Task:** `RUSTYOS-166`

## Context
Following the core WM/protocol stabilization in `RUSTYOS-165` (`ADR-0094`), interactive window ergonomics still trailed Cutefish (`KWin` + `cutefishos/kwin-plugins`) and macOS in five concrete areas:
1. Compositor cursor shape never updated when hovering resize edges/corners or moving off clients onto SSD chrome and shell overlays (`cursor_status` was mutated exclusively by clients).
2. Dragging a maximized window immediately shrunk and detached it from the pointer without a drag threshold or proportional cursor anchoring; top/left interactive resize jittered the opposite bottom-right corner before clients committed resized buffers.
3. No interactive Edge/Corner Snap (drag to TopBar/edges/corners with translucent preview) or double-click SSD titlebar maximize existed.
4. SSD resize borders extended `6 px` inside the window (stealing narrow scrollbars), traffic lights fired on `Pressed` for any non-right mouse button without press-drag-off cancellation, hover flickered across the `4 px` gaps between dots without setting `shell_redraw_needed`, and fullscreen checks inspected only the primary output.
5. Maximized and `MasterStack`-tiled windows retained `14 px` rounded corners and overhanging drop shadows.

## Decision
1. **Compositor Cursor Override Layer (`crates/rusty-compositor/src/state/mod.rs`, `seat_input.rs`, `input/pointer.rs`):**
   - Separate `client_cursor_status` (from `SeatHandler::cursor_image`) and `compositor_cursor_override: Option<CursorIcon>`, resolved into `cursor_status` via `sync_effective_cursor_status()`.
   - Map `ResizeEdges` to standard `CursorIcon::{N,S,E,W,Nw,Ne,Sw,Se}Resize`, `Grabbing` during window drag, `Crosshair` during `ScreenshotHud`, and `Default` over SSD titlebars/buttons, `TopBar`, `Dock`, and shell overlays; clear override over client surfaces so clients retain full cursor control.
2. **Proportional Unmaximize-on-Drag, Double-Click Maximize, and Interactive Edge/Corner Snap (`crates/rusty-wm/src/lib.rs`, `input/pointer.rs`, `shell_render/ssd.rs`, `frame_composer.rs`):**
   - Extend `rusty_wm::SnapTarget` with `TopLeft`, `TopRight`, `BottomLeft`, `BottomRight`, and `Maximize`, plus `compute_snap_rect` and `WindowManager::snap_window`.
   - Hold maximized drags behind a `5 px` movement threshold before unmaximizing and anchoring proportionally under the cursor (`compute_unmaximized_drag_origin`).
   - Toggle maximize on LMB double-click (`<= 400 ms`, `<= 6 px`) on SSD titlebars (`last_titlebar_click`).
   - Detect Edge/Corner Snap zones (`12 px` edge band, `48 px` corner band) per output during drag (`detect_edge_snap_target`), render a translucent accent-tinted rounded Liquid Glass preview directly beneath the dragged window (`generate_edge_snap_preview_elements`), and apply `snap_window` on release.
3. **Top/Left Resize Anchor-Lock & Square Maximized/Tiled Framing (`frame_composer.rs`, `shell_render/ssd.rs`):**
   - During active top/left interactive resize, pin visual `origin` from `resize.start_rect`'s opposite right/bottom edge minus `committed_size`, eliminating opposite-corner wobble while awaiting client commits.
   - Treat fullscreen, maximized, and `MasterStack` windows as `window_is_square_framed(win_id) == true`, skipping `14 px` rounded-corner clipping, SSD corner sprites (`generate_window_ssd_composite_elements_with_options`), and drop shadows.
4. **SSD Outer Resize Zone & Traffic-Light Polish (`shell_render/ssd.rs`, `input/{pointer,shell_pointer}.rs`):**
   - Bias SSD resize borders outward (`6 px` outside, `2 px` inside SSD / `1.5 px` inside CSD) so scrollbars stay unobstructed.
   - Restrict SSD titlebar/button actions to LMB (`272`), track `pressed_ssd_button` on `Pressed` and execute only if released on the same button, switch `check_window_traffic_lights_hover` to the continuous `TrafficLightButtons::is_hovered` cluster box, check fullscreen per pointer output, and set `shell_redraw_needed = true` on hover deltas.

## Consequences
- Window dragging, edge/corner snapping, resizing, cursor feedback, and titlebar controls now match Cutefish/KWin and macOS desktop ergonomics cleanly across single- and multi-monitor setups.
- Zero new external Cargo dependencies and zero `unsafe` code added.
