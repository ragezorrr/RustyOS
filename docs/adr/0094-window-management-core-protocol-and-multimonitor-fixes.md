# ADR-0094: Window Management Core Invariants, Wayland/XDG & XWayland Protocol Fixes, and Multi-Monitor Safety (`RUSTYOS-165`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-108`
- **Task:** `RUSTYOS-165`

## Context
A comprehensive code-level audit of RustyOS window management compared against Cutefish (`KWin` + `cutefishos/kwin-plugins`) uncovered ten high-impact correctness, security, and protocol bugs across `crates/rusty-wm` and `crates/rusty-compositor`:
1. Fullscreen and maximized windows kept invisible SSD titlebar/traffic-light and edge-resize hitboxes active (`visible_ssd_hit_windows` ignored `fullscreen`/`maximized` and used raw `w.geometry` instead of rendered `committed_size`).
2. `CompositorHandler::commit` unconditionally re-acquired `wl_keyboard` focus on background commits (stealing focus from `ext-session-lock-v1`, Spotlight, Polkit/Portal, Overview, and Desktop), failed to match secondary-output workspaces, and `handle_pointer_axis` leaked scroll events behind lock screens and overlays.
3. `minimized` state lived exclusively in `RustyCompositorState`, leaving empty holes in `MasterStack` tiling, allowing dangling focus on minimized windows, jumping focus across monitors on minimize, and causing `restore_window` to replay a `250 ms` Dock unminimize warp on already-visible windows during `Alt+Tab` / Overview clicks.
4. `WindowManager::remove_output` failed to migrate `WindowId`s into `target_ws.stacking` and corrupted `saved_floating_geometry`; `floating_work_area`, `popup_parent_target_rect`, `target_surface_under`, and `commit_layer_surface` hardcoded the primary `&self.output`; dragging across monitors never migrated workspace ownership.
5. `maximize_request` / `unmaximize_request` juggled focus (`focus_and_raise -> ToggleMaximizeFocused -> focus_and_raise(prev)`), mutating z-order and maximizing the wrong window on inactive workspaces; `flush_pending_toplevel_configures` sent configures before the client's initial `wl_surface.commit`, omitting `Fullscreen`; `ack_configure`, `State::{Resizing, Tiled*}`, `max_size`, and pointer-grab `serial` validation were missing; `show_window_menu` missed `TITLEBAR_HEIGHT`.
6. XWayland override-redirect menus/tooltips were mapped into `Space` but never rendered (and leaked on XWayland crash), X11 coordinates drifted after animations, `configure_request` ignored programmatic resizes, X11 server stacking diverged from `rusty-wm`, and `sync_space_with_wm` emitted redundant configure storms; nested Wayland `xdg_popup` submenus rendered behind their parent popup and pushed SSD chrome behind the client surface.

## Decision
1. **Promote `minimized`, `max_size`, Direct State APIs, and Safe Multi-Monitor Migration into `crates/rusty-wm`:**
   - Extend `ManagedWindow` with `minimized: bool` and `max_size: Option<(i32, i32)>`.
   - Add direct, focus-preserving methods: `set_window_minimized`, `set_window_maximized`, `set_window_fullscreen_on_output`, `set_window_max_size`, `migrate_window_to_output`, `output`, `outputs`, `output_for_workspace`, and `output_for_window`.
   - Exclude minimized windows from `MasterStack` tiling, `unmap_window` / `move_focused_to_workspace` focus fallback, `cycle_focus`, and `window_under_point`.
   - Fix `remove_output` to migrate `stacking` order and preserve `saved_floating_geometry` dimensions, and fix `move_focused_to_next_output` to preserve relative coordinates.
2. **Enforce Focus/Input Security Guards and Fullscreen/Maximized Frame Hit-Testing (`crates/rusty-compositor`):**
   - Add `SsdHitWindow::allow_resize: bool` (`!fullscreen && !maximized`), `has_ssd = !fullscreen && !csd`, and match hit rects with `resolved_window_global_rect` so fullscreen windows occlude background frames cleanly without exposing invisible buttons or resize grabs.
   - Add `should_allow_window_keyboard_focus()` and `is_valid_pointer_grab_request(Option<Serial>)` backed by `pressed_pointer_buttons` / `last_pointer_press_serial`; guard `CompositorHandler::commit`, `sync_keyboard_focus_with_wm`, `handle_pointer_axis`, `move_request`, and `resize_request`; stop mutating `wm.focused_output` on bare pointer hover.
3. **Fix Wayland/XDG Protocol Handshake, Multi-Monitor Layer/Popup Geometry, and Teardown:**
   - Gate `flush_pending_toplevel_configures` on `toplevel_initial_committed`, apply `max_size`, `Fullscreen`, `Resizing`, and `Tiled{Left,Right,Top,Bottom}` via `apply_xdg_toplevel_states`, implement `XdgShellHandler::ack_configure`, add `titlebar_h` in `show_window_menu`, and always respond with `configure` to maximize/fullscreen requests without focus juggling.
   - Make `commit_layer_surface`, `layer_destroyed`, `target_surface_under`, `popup_parent_target_rect`, and `floating_work_area_for_window` multi-monitor aware while intersecting `layer_map.non_exclusive_zone()` with RustyOS's built-in TopBar/Dock work area (`effective_work_area_for_output_entry`).
   - Run `output_map.relayout_adjacent()` + `relayout_wm_for_all_outputs()` in `remove_output`, migrate dragged windows across monitors with screen-edge titlebar clamping, cancel active grabs/popups on workspace switch, and clear all window-bound hover/menu/cache references in `finish_window_teardown`.
4. **Fix XWayland Override-Redirect Rendering, Sync, and `xdg_popup` Z-Order:**
   - Render visible X11 override-redirect surfaces in `frame_composer.rs` (`push_xwayland_override_redirect_elements`), disable Direct Scanout occlusion bypass when popups/OR surfaces are active, and prune dead OR/X11 windows on `prune_dead_windows` and `XWaylandEvent::Error`.
   - Reverse `PopupManager::popups_for_surface` order in `push_client_popup_elements` so nested submenus appear above parent popups, and always render SSD chrome in front of the client surface.
   - Deduplicate XWayland `configure`/`set_activated`/`set_maximized`/`set_fullscreen` via `last_x11_configure_state` (including settled post-animation updates in `tick_animations`), sync X11 stacking order via `sync_x11_stacking_order`, honor floating X11 `configure_request` resizes, and propagate `_NET_WM_STATE_HIDDEN` via `x11.set_suspended`.

## Consequences
- Eliminates focus theft during locks/modals, invisible fullscreen hitboxes, spurious `Alt+Tab`/Overview unminimize animations, disappearing windows on monitor disconnect or cross-monitor drag, invisible XWayland menus, and broken nested popup stacking.
- Establishes clean `rusty-wm` invariants ready for Phase B (`RUSTYOS-166`: resize cursors, Edge-Snap, double-click titlebar, traffic-light polish) and Phase C (`RUSTYOS-167`: modal/transient dialogs, thumbnail Alt+Tab, Overview UX).
- Zero new external Cargo dependencies and zero `unsafe` code added.
