# ADR-0091: Dock Live Minimized Thumbnails, Unread Count Badges, and Reactive `inotify` `TrashMonitor` (`RUSTYOS-162`)

- **Status:** Accepted
- **Date:** 2026-11-23
- **Decision ID:** `D-105`
- **Task:** `RUSTYOS-162`

## Context
After Phase A (`RUSTYOS-161`) introduced Smart Hide, multi-monitor Dock independence, and smooth `DockPresentation`, three user-visible Dock capabilities still lagged behind macOS/CutefishOS parity:
1. Minimized windows (`DockSection::MinimizedWindow`) rendered only a framed application icon instead of a live window preview, and their slot order in the Dock could not be rearranged via drag-and-drop.
2. Applications had no numeric unread badge (`CountBadge`) on Dock icons, ignoring both `com.canonical.Unity.LauncherEntry::Update` signals (used by Telegram, Discord, Firefox, Thunderbird, Chromium, Electron) and unread D-Bus desktop notifications.
3. The Dock Trash icon (`Trash` / `TrashFull`) only refreshed when triggered by internal compositor actions, missing files deleted from external file managers, terminals, or removable mounts.

## Decision
1. **Live Minimized Window GPU Thumbnails & Drag-to-Reorder (`minimized_dock_order`):**
   - Retain offscreen `GlesTexture` snapshots inside `WindowMinimizePipeline` (`crates/rusty-compositor/src/surfaces/window_minimize.rs`) for the full lifetime of minimized windows (`state.is_window_minimized(win_id) || active_minimize_anim`), freeing GPU memory immediately once a window finishes restoring or closes.
   - Capture snapshots even when `reduced_motion = true` in `prepare_minimize_snapshots_for_output` so live thumbnails remain available without running warp animations.
   - Give each `MinimizedWindowSnapshot` a dedicated `dock_thumb_id: Id` and `dock_thumb_damage` bag in `render_minimized_dock_thumbnail_element` so Dock thumbnails reuse the existing 4-tap RGSS shader without `OutputDamageTracker` `Id` collisions against active minimize/restore warp quads and with zero `glReadPixels` CPU stalls.
   - Render `DockSection::MinimizedWindow` slots (`push_minimized_dock_slot`) as aspect-fit framed previews with a compact (`42%`, `16..=24 px`) app-icon badge at the bottom-right corner, falling back gracefully to the framed app icon if no snapshot exists.
   - Track user-defined minimized window ordering in `RustyCompositorState::minimized_dock_order`, extend `DockDragState` / `DockDragPreview` (`section: DockSection`, `minimized_win: Option<WindowId>`) and pointer drag handling to reorder minimized Dock slots via `commit_dock_minimized_reorder` (never removable by pulling outside), smoothly animated by `DockPresentation`.
2. **Dock Unread `CountBadge` (Unity `LauncherEntry` + Notification Fallback):**
   - Subscribe to session D-Bus signal `com.canonical.Unity.LauncherEntry::Update` on the existing `zbus::blocking::Connection` in `crates/rusty-compositor/src/dbus_notifications.rs` (`spawn_unity_launcher_badge_listener`, `parse_unity_launcher_app_key`, `parse_unity_launcher_update`), emitting `DbusNotificationCommand::LauncherBadgeUpdate` and waking the compositor via the existing `calloop::ping` waker.
   - Maintain `dock_unity_badges` (authoritative per canonical `app_key`) and `dock_unread_notif_badges` (fallback counter incremented by unread D-Bus notifications resolved via `desktop-entry` / `app_name` when the target app is not focused, decremented on dismiss/close, and cleared automatically on focus in `record_focused_app_window`), keeping `browser`/`firefox` merged consistently.
   - Render macOS-styled red pill badges (`#FF3B30` Light / `#FF453A` Dark, `1..=99` and `"99+"`, bold white label, crisp rim + elevation shadow) in `dock_count_badge_rect`, `push_dock_slot_shape`, and `append_dock_text` across all outputs (suppressed on leaving ghost slots).
3. **Reactive Background `TrashMonitor` (`crates/rusty-services/src/trash_monitor.rs`):**
   - Implement a non-blocking `TrashMonitorHandle` using workspace `libc` (`inotify_init1`, `inotify_add_watch`, `pipe2`, `poll`, `geteuid`) isolated inside `#[allow(unsafe_code)] mod linux_watch` with RAII `OwnedFd` handles (`crates/rusty-compositor` remains 100% `unsafe`-free; zero new external Cargo dependencies).
   - Watch both `$XDG_DATA_HOME/Trash/files` (and parent dirs when `Trash/` does not exist yet) and per-mount `.Trash-$uid/files` / `.Trash/$uid/files` discovered via `/proc/self/mountinfo` (`POLLPRI | POLLERR` instant mount/unmount re-sync, filtering virtual/network filesystems).
   - Debounce filesystem bursts (`TRASH_MONITOR_DEBOUNCE = 150 ms`), expose lock-free `AtomicBool` state (`has_items`, `take_update`), wake the compositor event loop via `calloop::ping::Ping` (`set_on_change`), and drain updates in `frame_composer.rs::dispatch_and_refresh_clients`.

## Consequences
- Minimized Dock previews update directly from GPU memory with zero CPU readbacks and reorder smoothly alongside pinned icons.
- Third-party apps supporting Unity `LauncherEntry` as well as standard `org.freedesktop.Notifications` apps surface live macOS-style unread badges on Dock icons on all monitors.
- External Trash changes (including removable drives) update the Dock Trash icon within ~150 ms without blocking or polling the compositor event loop.
- Zero new Cargo dependencies added; `crates/rusty-compositor` stays free of `unsafe` code.
