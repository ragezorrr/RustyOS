//! Server-side Wayland Drag-and-Drop (`text/uri-list`) drop target for the Dock,
//! non-blocking pipe/Trash/Downloads execution, `.desktop` pinning, and Spotlight drag-to-Dock (`RUSTYOS-163`).

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use smithay::{
    backend::input::ButtonState,
    input::pointer::ButtonEvent,
    reexports::wayland_server::{
        Resource,
        protocol::{
            wl_data_device_manager::DndAction, wl_data_source::WlDataSource, wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point, Serial},
    wayland::selection::data_device::with_source_metadata,
};
use tracing::{info, warn};

use crate::config::DockPosition;
use crate::shell_render::{
    ComputedDockLayout, DockHit, DockSection, TOP_BAR_HEIGHT, compute_dock_layout,
    dock_label_for_key,
};
use rusty_wm::OutputId;

use super::RustyCompositorState;

/// Hard upper bound for incoming Wayland `text/uri-list` payloads (`256 KiB`).
pub const DND_URI_MAX_BYTES: usize = 256 * 1024;
/// Hard deadline for reading an external client's `text/uri-list` pipe (`1500 ms`).
pub const DND_URI_READ_TIMEOUT: Duration = Duration::from_millis(1500);
/// Maximum number of local paths accepted from a single Dock drop.
pub const MAX_DND_DROP_PATHS: usize = 64;

const URI_LIST_MIME: &str = "text/uri-list";
const EVT_SOURCE_ACTION_SINCE: u32 = 3;

/// Active Dock drop target resolved under the pointer during an external, desktop, or Spotlight drag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockDropTarget {
    /// Drop files onto a running or pinned application tile to open them in that app.
    App {
        output_id: OutputId,
        slot_idx: usize,
        app_key: String,
        label: String,
        pinned_insert_idx: Option<usize>,
    },
    /// Drop files onto the `Trash` utility slot to move them into `FreeDesktop` Trash.
    Trash {
        output_id: OutputId,
        slot_idx: usize,
    },
    /// Drop files onto the `Downloads` utility slot to copy them safely into `~/Downloads`.
    Downloads {
        output_id: OutputId,
        slot_idx: usize,
    },
    /// Insert a `.desktop` file or Spotlight application into `[dock].pinned_apps` at `insert_idx`.
    PinInsert {
        output_id: OutputId,
        insert_idx: usize,
    },
}

impl DockDropTarget {
    /// Output on which this Dock drop target lives.
    #[must_use]
    pub const fn output_id(&self) -> OutputId {
        match self {
            Self::App { output_id, .. }
            | Self::Trash { output_id, .. }
            | Self::Downloads { output_id, .. }
            | Self::PinInsert { output_id, .. } => *output_id,
        }
    }

    /// Preferred Wayland `DndAction` negotiated with the source client.
    #[must_use]
    pub fn preferred_action(&self, offered: DndAction) -> DndAction {
        match self {
            Self::Trash { .. } => {
                if offered.contains(DndAction::Move) {
                    DndAction::Move
                } else if offered.contains(DndAction::Copy) {
                    DndAction::Copy
                } else {
                    DndAction::Move
                }
            }
            Self::App { .. } | Self::Downloads { .. } | Self::PinInsert { .. } => {
                if offered.contains(DndAction::Copy) {
                    DndAction::Copy
                } else if offered.contains(DndAction::Move) {
                    DndAction::Move
                } else {
                    DndAction::Copy
                }
            }
        }
    }

    /// Human-readable Dock hover action pill label.
    #[must_use]
    pub fn action_label(&self) -> String {
        match self {
            Self::App { label, .. } => format!("Open in {label}"),
            Self::Trash { .. } => "Move to Trash".to_owned(),
            Self::Downloads { .. } => "Copy to Downloads".to_owned(),
            Self::PinInsert { .. } => "Pin to Dock".to_owned(),
        }
    }
}

/// Active external client Wayland `wl_data_device` drag offering `text/uri-list`.
#[derive(Debug, Clone)]
pub struct ExternalDndState {
    pub source: WlDataSource,
    pub offered_actions: DndAction,
    pub active_target: Option<DockDropTarget>,
    pub last_sent_dock_target: bool,
    pub last_sent_action: DndAction,
}

/// Deferred pointer button release held briefly while an external `text/uri-list` pipe drains.
#[derive(Debug, Clone, Copy)]
pub struct DeferredPointerRelease {
    pub button: u32,
    pub serial: Serial,
    pub time: u32,
}

type PipeResultSlot = Arc<Mutex<Option<Result<Vec<u8>, String>>>>;
type WakerCallback = Arc<dyn Fn() + Send + Sync>;
type SharedWaker = Arc<Mutex<Option<WakerCallback>>>;

/// In-flight non-blocking pipe transfer from an external `WlDataSource` after dropping on the Dock.
#[derive(Debug)]
pub struct PendingExternalDndDrop {
    pub source: WlDataSource,
    pub target: DockDropTarget,
    pub deferred_release: Option<DeferredPointerRelease>,
    pub started_at: Instant,
    pub result_slot: PipeResultSlot,
}

/// Completion notification from a background Trash or Downloads file operation.
#[derive(Debug, Clone)]
pub struct DockDndBgOutcome {
    pub osd_message: String,
    pub refresh_trash: bool,
    pub refresh_desktop: bool,
}

/// Active drag-to-pin gesture initiated from a Spotlight application row (`RUSTYOS-163`).
#[derive(Debug, Clone)]
pub struct SpotlightDragState {
    pub row_idx: usize,
    pub app_key: String,
    pub label: String,
    pub icon_name: Option<String>,
    pub start_pointer: Point<f64, Logical>,
    pub current_pointer: Point<f64, Logical>,
    pub active_drag: bool,
    pub target: Option<DockDropTarget>,
}

/// Shared wakeup + background-result mailbox for non-blocking Dock `DnD` operations.
#[derive(Default)]
pub struct DockDndMailbox {
    waker: SharedWaker,
    bg_outcomes: Arc<Mutex<Vec<DockDndBgOutcome>>>,
}

impl std::fmt::Debug for DockDndMailbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockDndMailbox").finish_non_exhaustive()
    }
}

impl DockDndMailbox {
    pub fn set_waker(&self, waker: WakerCallback) {
        if let Ok(mut guard) = self.waker.lock() {
            *guard = Some(waker);
        }
    }

    pub(crate) fn clone_waker(&self) -> SharedWaker {
        Arc::clone(&self.waker)
    }

    pub(crate) fn drain_bg_outcomes(&self) -> Vec<DockDndBgOutcome> {
        self.bg_outcomes
            .lock()
            .map_or_else(|_| Vec::new(), |mut guard| std::mem::take(&mut *guard))
    }

    fn wake_shared(waker: &SharedWaker) {
        let cb = waker.lock().ok().and_then(|g| g.clone());
        if let Some(wake) = cb {
            wake();
        }
    }
}

/// Parses an RFC 2483 `text/uri-list` payload into validated, deduplicated local filesystem paths.
#[must_use]
pub fn parse_uri_list(bytes: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(bytes);
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for raw_line in text.lines() {
        if paths.len() >= MAX_DND_DROP_PATHS {
            break;
        }
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(path) = parse_local_file_uri(line)
            && seen.insert(path.clone())
        {
            paths.push(path);
        }
    }
    paths
}

fn parse_local_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("FILE://"))?;
    let path_part = if let Some(abs) = rest.strip_prefix('/') {
        format!("/{abs}")
    } else {
        // Reject remote hosts (`file://remotehost/...`) and malformed URIs.
        let local = rest.strip_prefix("localhost/")?;
        format!("/{local}")
    };
    let decoded = percent_decode_path(&path_part)?;
    if decoded.is_empty() || decoded.contains(&0) {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(decoded)))
}

fn percent_decode_path(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_nibble(bytes[i + 1])?;
            let lo = hex_nibble(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Returns `true` when `paths` is non-empty and every entry is a `.desktop` file.
#[must_use]
pub fn is_desktop_file_drop(paths: &[PathBuf]) -> bool {
    !paths.is_empty()
        && paths.iter().all(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("desktop"))
        })
}

/// Resolves which Dock drop target (if any) sits beneath `(local_x, local_y)` on `output_id`.
#[must_use]
pub fn resolve_dock_drop_target_at(
    layout: &ComputedDockLayout,
    position: DockPosition,
    output_id: OutputId,
    local_x: f64,
    local_y: f64,
    prefer_pin_insert: bool,
) -> Option<DockDropTarget> {
    let cap = layout.capsule_rect;
    let pad = 12.0;
    if local_x < f64::from(cap.loc.x) - pad
        || local_x > f64::from(cap.loc.x + cap.size.w) + pad
        || local_y < f64::from(cap.loc.y) - pad
        || local_y > f64::from(cap.loc.y + cap.size.h) + pad
    {
        return None;
    }

    let pinned_slots: Vec<&crate::shell_render::ComputedDockSlot> = layout
        .slots
        .iter()
        .filter(|s| s.slot_idx != usize::MAX && s.tile.section == DockSection::Pinned)
        .collect();

    if prefer_pin_insert {
        let insert_idx = compute_pinned_insert_index(&pinned_slots, position, local_x, local_y);
        return Some(DockDropTarget::PinInsert {
            output_id,
            insert_idx,
        });
    }

    for slot in &layout.slots {
        if slot.slot_idx == usize::MAX {
            continue;
        }
        let r = slot.hit_rect;
        if local_x < f64::from(r.loc.x) - 3.0
            || local_x > f64::from(r.loc.x + r.size.w) + 3.0
            || local_y < f64::from(r.loc.y) - 10.0
            || local_y > f64::from(r.loc.y + r.size.h) + 10.0
        {
            continue;
        }
        return match slot.tile.hit {
            DockHit::PinnedTrash => Some(DockDropTarget::Trash {
                output_id,
                slot_idx: slot.slot_idx,
            }),
            DockHit::PinnedDownloads => Some(DockDropTarget::Downloads {
                output_id,
                slot_idx: slot.slot_idx,
            }),
            DockHit::ToggleOverview | DockHit::PinnedSettings | DockHit::MinimizedWindow(_) => None,
            _ if slot.tile.app_key == "updater" => None,
            _ => {
                let pinned_insert_idx = (slot.tile.section == DockSection::Pinned).then(|| {
                    compute_pinned_insert_index(&pinned_slots, position, local_x, local_y)
                });
                Some(DockDropTarget::App {
                    output_id,
                    slot_idx: slot.slot_idx,
                    app_key: slot.tile.app_key.clone(),
                    label: slot.tile.label.clone(),
                    pinned_insert_idx,
                })
            }
        };
    }

    // Hovering a gap inside the pinned section offers a clean `.desktop` pin-insertion target.
    if let (Some(first), Some(last)) = (pinned_slots.first(), pinned_slots.last()) {
        let cur = axis_coord(position, local_x, local_y);
        let min_axis = f64::from(rect_axis_start(position, first.hit_rect)) - 10.0;
        let max_axis = f64::from(rect_axis_end(position, last.hit_rect)) + 10.0;
        if cur >= min_axis && cur <= max_axis {
            let insert_idx = compute_pinned_insert_index(&pinned_slots, position, local_x, local_y);
            return Some(DockDropTarget::PinInsert {
                output_id,
                insert_idx,
            });
        }
    }

    None
}

fn compute_pinned_insert_index(
    pinned_slots: &[&crate::shell_render::ComputedDockSlot],
    position: DockPosition,
    local_x: f64,
    local_y: f64,
) -> usize {
    let cur = axis_coord(position, local_x, local_y);
    let mut insert_idx = pinned_slots.len();
    for (idx, slot) in pinned_slots.iter().enumerate() {
        let center = f64::midpoint(
            f64::from(rect_axis_start(position, slot.hit_rect)),
            f64::from(rect_axis_end(position, slot.hit_rect)),
        );
        if cur < center {
            insert_idx = idx;
            break;
        }
    }
    insert_idx
}

const fn axis_coord(pos: DockPosition, x: f64, y: f64) -> f64 {
    match pos {
        DockPosition::Bottom => x,
        DockPosition::Left | DockPosition::Right => y,
    }
}

const fn rect_axis_start(
    pos: DockPosition,
    rect: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
) -> i32 {
    match pos {
        DockPosition::Bottom => rect.loc.x,
        DockPosition::Left | DockPosition::Right => rect.loc.y,
    }
}

const fn rect_axis_end(
    pos: DockPosition,
    rect: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
) -> i32 {
    match pos {
        DockPosition::Bottom => rect.loc.x + rect.size.w,
        DockPosition::Left | DockPosition::Right => rect.loc.y + rect.size.h,
    }
}

impl RustyCompositorState {
    /// Registers an external event-loop waker (`calloop::ping`) for async Dock `DnD` completions.
    pub fn set_dock_dnd_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.dock_dnd_mailbox.set_waker(waker);
    }

    /// Returns `true` when any external, Desktop, or Spotlight drag targeting the Dock is active.
    #[must_use]
    pub fn is_any_dock_drop_drag_active(&self) -> bool {
        self.external_dnd.is_some()
            || self.spotlight_drag.as_ref().is_some_and(|d| d.active_drag)
            || self.desktop_drag.as_ref().is_some_and(|d| d.is_active)
    }

    /// Records the start of a client Wayland `wl_data_device.start_drag` session (`RUSTYOS-163`).
    pub fn on_external_dnd_started(
        &mut self,
        source: Option<WlDataSource>,
        icon: Option<WlSurface>,
    ) {
        self.dnd_icon = icon;
        self.external_dnd = None;
        if let Some(src) = source
            && let Ok((offers_uri_list, offered_actions)) = with_source_metadata(&src, |meta| {
                let has_uri = meta
                    .mime_types
                    .iter()
                    .any(|m| m.eq_ignore_ascii_case(URI_LIST_MIME));
                (has_uri, meta.dnd_action)
            })
            && offers_uri_list
        {
            self.external_dnd = Some(ExternalDndState {
                source: src,
                offered_actions,
                active_target: None,
                last_sent_dock_target: false,
                last_sent_action: DndAction::None,
            });
        }
    }

    /// Clears active external `DnD` tracking when Smithay finishes or cancels `DnDGrab`.
    pub fn on_external_dnd_dropped(&mut self) {
        self.dnd_icon = None;
        if self.external_dnd.take().is_some() {
            self.sync_dock_auto_hide_target();
            self.shell_redraw_needed = true;
        }
    }

    /// Updates Dock hover/drop-target state during an active external Wayland `text/uri-list` drag.
    ///
    /// Returns `true` when the pointer is over a Dock target (caller should pass `under = None` to
    /// Smithay's `DnDGrab` so underlying windows receive `wl_data_device.leave()`).
    pub fn update_external_dnd_dock_hover(&mut self, pos: Point<f64, Logical>) -> bool {
        if self.external_dnd.is_none() {
            return false;
        }
        let (output_id, is_primary, output_size, local_x, local_y) = self.output_local_coords(pos);
        let reveal = self.dock_reveal_for_output(output_id);
        let new_target = if reveal > 0.15 {
            let tiles = self.build_dock_tiles();
            let rest_view = crate::shell_render::DockViewState {
                tiles: &tiles,
                icon_size: self.user_config.dock_icon_size,
                position: self.user_config.dock_position,
                auto_hide_reveal: reveal,
                show_indicators: self.user_config.dock_show_indicators,
                top_inset: if is_primary { TOP_BAR_HEIGHT } else { 0 },
                ..crate::shell_render::DockViewState::default()
            };
            let layout = compute_dock_layout(output_size, &rest_view);
            resolve_dock_drop_target_at(
                &layout,
                self.user_config.dock_position,
                output_id,
                local_x,
                local_y,
                false,
            )
        } else {
            None
        };

        let mut target_changed = false;
        let over_dock = new_target.is_some();
        if let Some(ref mut dnd) = self.external_dnd {
            if dnd.active_target != new_target {
                dnd.active_target.clone_from(&new_target);
                target_changed = true;
            }
            if let Some(ref tgt) = new_target {
                let desired_action = tgt.preferred_action(dnd.offered_actions);
                if !dnd.last_sent_dock_target {
                    dnd.source.target(Some(URI_LIST_MIME.to_owned()));
                    dnd.last_sent_dock_target = true;
                }
                if dnd.last_sent_action != desired_action {
                    if dnd.source.version() >= EVT_SOURCE_ACTION_SINCE {
                        dnd.source.action(desired_action);
                    }
                    dnd.last_sent_action = desired_action;
                }
            } else if dnd.last_sent_dock_target {
                dnd.source.target(None);
                if dnd.source.version() >= EVT_SOURCE_ACTION_SINCE {
                    dnd.source.action(DndAction::None);
                }
                dnd.last_sent_dock_target = false;
                dnd.last_sent_action = DndAction::None;
            }
        }
        if target_changed {
            self.shell_redraw_needed = true;
        }
        over_dock
    }

    /// Intercepts a pointer button release over a Dock drop target during an external Wayland drag,
    /// requesting `text/uri-list` non-blockingly before releasing Smithay's `DnDGrab`.
    ///
    /// Returns `true` when the button release was deferred pending the async pipe read.
    pub fn begin_external_dnd_dock_drop(&mut self, button: u32, serial: Serial, time: u32) -> bool {
        let Some(dnd) = self.external_dnd.as_ref() else {
            return false;
        };
        let Some(target) = dnd.active_target.clone() else {
            return false;
        };
        let source = dnd.source.clone();
        let chosen_action = target.preferred_action(dnd.offered_actions);

        let Ok((read_pipe, write_pipe)) = std::io::pipe() else {
            warn!("Failed to allocate pipe for Wayland Dock DnD transfer");
            return false;
        };

        source.target(Some(URI_LIST_MIME.to_owned()));
        if source.version() >= EVT_SOURCE_ACTION_SINCE {
            source.action(chosen_action);
            source.dnd_drop_performed();
        }
        source.send(URI_LIST_MIME.to_owned(), write_pipe.as_fd());
        drop(write_pipe);
        let _ = self.display_handle.flush_clients();

        let result_slot: PipeResultSlot = Arc::new(Mutex::new(None));
        let slot_bg = Arc::clone(&result_slot);
        let waker_bg = self.dock_dnd_mailbox.clone_waker();
        let read_fd = OwnedFd::from(read_pipe);
        let _ = std::thread::Builder::new()
            .name("rustyos-dnd-read".to_owned())
            .spawn(move || {
                let res = rusty_services::read_fd_bounded_with_timeout(
                    read_fd,
                    DND_URI_MAX_BYTES,
                    DND_URI_READ_TIMEOUT,
                );
                if let Ok(mut guard) = slot_bg.lock() {
                    *guard = Some(res);
                }
                DockDndMailbox::wake_shared(&waker_bg);
            });

        self.external_dnd = None;
        self.dnd_icon = None;
        self.pending_external_dnd_drop = Some(PendingExternalDndDrop {
            source,
            target,
            deferred_release: Some(DeferredPointerRelease {
                button,
                serial,
                time,
            }),
            started_at: Instant::now(),
            result_slot,
        });
        self.shell_redraw_needed = true;
        true
    }

    /// Cancels any stalled in-flight external `DnD` transfer and releases its deferred button event.
    pub fn cancel_pending_external_dnd_drop(&mut self) {
        if let Some(pending) = self.pending_external_dnd_drop.take() {
            if pending.source.is_alive() {
                pending.source.cancelled();
                let _ = self.display_handle.flush_clients();
            }
            if let Some(rel) = pending.deferred_release {
                self.replay_deferred_pointer_release(rel);
            }
            self.sync_dock_auto_hide_target();
            self.shell_redraw_needed = true;
        }
    }

    fn replay_deferred_pointer_release(&mut self, rel: DeferredPointerRelease) {
        let pointer = self.pointer.clone();
        pointer.button(
            self,
            &ButtonEvent {
                button: rel.button,
                state: ButtonState::Released,
                serial: rel.serial,
                time: rel.time,
            },
        );
        pointer.frame(self);
    }

    /// Drains completed external Wayland `DnD` pipe reads and background Trash/Downloads jobs.
    pub fn drain_dock_dnd_events(&mut self) {
        let ready = self.pending_external_dnd_drop.as_ref().is_some_and(|p| {
            p.result_slot.lock().is_ok_and(|g| g.is_some())
                || p.started_at.elapsed() >= DND_URI_READ_TIMEOUT + Duration::from_millis(200)
        });
        if ready && let Some(pending) = self.pending_external_dnd_drop.take() {
            let payload = pending
                .result_slot
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_else(|| Err("timed out waiting for data transfer".to_owned()));

            let ok = payload.is_ok();
            if pending.source.is_alive() {
                if ok && pending.source.version() >= EVT_SOURCE_ACTION_SINCE {
                    pending.source.dnd_finished();
                } else if !ok {
                    pending.source.cancelled();
                }
                let _ = self.display_handle.flush_clients();
            }
            if let Some(rel) = pending.deferred_release {
                self.replay_deferred_pointer_release(rel);
            }
            match payload {
                Ok(bytes) => {
                    let paths = parse_uri_list(&bytes);
                    if !paths.is_empty() {
                        self.execute_dock_drop(&pending.target, paths, false);
                    }
                }
                Err(err) => {
                    warn!(error = %err, "External Wayland Dock drop transfer failed");
                }
            }
            self.sync_dock_auto_hide_target();
            self.shell_redraw_needed = true;
        }

        for outcome in self.dock_dnd_mailbox.drain_bg_outcomes() {
            self.show_osd(&outcome.osd_message, None, false);
            if outcome.refresh_trash {
                self.update_trash_full_status();
            }
            if outcome.refresh_desktop {
                self.resync_desktop_items();
                self.save_desktop_grid_layout();
            }
            self.shell_redraw_needed = true;
        }
    }

    /// Inserts or moves `app_key` into `[dock].pinned_apps` at `insert_idx` and persists config.
    pub fn pin_app_at_index(&mut self, app_key: &str, insert_idx: usize) -> bool {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() || matches!(key.as_str(), "downloads" | "trash") {
            return false;
        }
        let pins = &mut self.user_config.dock_pinned_apps;
        if let Some(existing_idx) = pins.iter().position(|k| k.eq_ignore_ascii_case(&key)) {
            let target = insert_idx.min(pins.len().saturating_sub(1));
            if existing_idx != target {
                let item = pins.remove(existing_idx);
                pins.insert(target, item);
            }
        } else {
            let target = insert_idx.min(pins.len());
            pins.insert(target, key.clone());
        }
        self.refresh_dock_pin_metadata_cache();
        self.sync_and_save_user_config();
        let label = dock_label_for_key(&key);
        self.show_osd(format!("Pinned {label} to Dock"), None, false);
        self.shell_redraw_needed = true;
        true
    }

    /// Executes a validated file or `.desktop` drop onto a resolved [`DockDropTarget`].
    pub fn execute_dock_drop(
        &mut self,
        target: &DockDropTarget,
        paths: Vec<PathBuf>,
        from_desktop: bool,
    ) {
        if paths.is_empty() {
            return;
        }
        match target {
            DockDropTarget::Trash { .. } => {
                self.spawn_background_trash_drop(paths, from_desktop);
            }
            DockDropTarget::Downloads { .. } => {
                self.spawn_background_downloads_copy(paths);
            }
            DockDropTarget::PinInsert { insert_idx, .. } => {
                self.pin_dropped_desktop_files(&paths, *insert_idx);
            }
            DockDropTarget::App {
                app_key,
                label,
                pinned_insert_idx,
                ..
            } => {
                if let Some(insert_idx) = pinned_insert_idx
                    && is_desktop_file_drop(&paths)
                {
                    self.pin_dropped_desktop_files(&paths, *insert_idx);
                    return;
                }
                if !self.launcher.app_accepts_dropped_paths(app_key, &paths) {
                    self.show_osd(format!("{label} does not accept these files"), None, false);
                    return;
                }
                let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
                if self.launcher.launch_by_dock_key_with_paths(
                    app_key,
                    &paths,
                    &self.socket_name,
                    &ipc_path,
                ) {
                    self.trigger_app_bounce(app_key, false);
                    self.show_osd(format!("Opened in {label}"), None, false);
                    info!(app_key = %app_key, count = paths.len(), "Opened dropped files via Dock");
                }
            }
        }
    }

    fn pin_dropped_desktop_files(&mut self, paths: &[PathBuf], mut insert_idx: usize) {
        let mut pinned_any = false;
        for path in paths {
            if !path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("desktop"))
            {
                continue;
            }
            if let Some((key, _app)) = self.launcher.resolve_desktop_file_to_dock_key(path)
                && self.pin_app_at_index(&key, insert_idx)
            {
                insert_idx = insert_idx.saturating_add(1);
                pinned_any = true;
            }
        }
        if !pinned_any {
            self.show_osd("Drop a .desktop application to pin", None, false);
        }
    }

    pub(crate) fn spawn_background_trash_drop(&mut self, paths: Vec<PathBuf>, from_desktop: bool) {
        let Some(trash_root) = super::context_menu_actions::resolve_trash_root() else {
            self.show_osd("Trash unavailable", None, false);
            return;
        };
        if from_desktop {
            for p in &paths {
                self.selected_desktop_items.remove(p);
            }
        }
        let waker = self.dock_dnd_mailbox.clone_waker();
        let outcomes = Arc::clone(&self.dock_dnd_mailbox.bg_outcomes);
        let _ = std::thread::Builder::new()
            .name("rustyos-dock-trash".to_owned())
            .spawn(move || {
                let mut moved = 0_usize;
                for path in &paths {
                    if rusty_services::file_manager::move_to_trash(path, &trash_root).is_ok() {
                        moved += 1;
                    }
                }
                let msg = if moved == 0 {
                    "Could not move to Trash".to_owned()
                } else if moved == 1 {
                    "Moved to Trash".to_owned()
                } else {
                    format!("Moved {moved} items to Trash")
                };
                if let Ok(mut guard) = outcomes.lock() {
                    guard.push(DockDndBgOutcome {
                        osd_message: msg,
                        refresh_trash: moved > 0,
                        refresh_desktop: from_desktop && moved > 0,
                    });
                }
                DockDndMailbox::wake_shared(&waker);
            });
    }

    fn spawn_background_downloads_copy(&self, paths: Vec<PathBuf>) {
        let Some(downloads_dir) = resolve_downloads_dir() else {
            return;
        };
        let waker = self.dock_dnd_mailbox.clone_waker();
        let outcomes = Arc::clone(&self.dock_dnd_mailbox.bg_outcomes);
        let _ = std::thread::Builder::new()
            .name("rustyos-dock-dl-copy".to_owned())
            .spawn(move || {
                let _ = fs::create_dir_all(&downloads_dir);
                let mut copied = 0_usize;
                for src in &paths {
                    if copy_entry_unique(src, &downloads_dir).is_ok() {
                        copied += 1;
                    }
                }
                let msg = if copied == 0 {
                    "Could not copy to Downloads".to_owned()
                } else if copied == 1 {
                    "Copied to Downloads".to_owned()
                } else {
                    format!("Copied {copied} items to Downloads")
                };
                if let Ok(mut guard) = outcomes.lock() {
                    guard.push(DockDndBgOutcome {
                        osd_message: msg,
                        refresh_trash: false,
                        refresh_desktop: false,
                    });
                }
                DockDndMailbox::wake_shared(&waker);
            });
    }

    /// Resolves the active Dock drop visual highlight for `output_id` across Wayland, Desktop, and Spotlight drags.
    #[must_use]
    pub fn dock_drop_highlight_for_output(
        &self,
        output_id: OutputId,
    ) -> Option<crate::shell_render::DockDropHighlight<'_>> {
        if let Some(ref spot) = self.spotlight_drag
            && spot.active_drag
            && let Some(DockDropTarget::PinInsert {
                output_id: tgt_out,
                insert_idx,
            }) = spot.target
            && tgt_out == output_id
        {
            return Some(crate::shell_render::DockDropHighlight::PinInsert {
                insert_idx,
                label: &spot.label,
            });
        }
        if let Some(ref dnd) = self.external_dnd
            && let Some(ref target) = dnd.active_target
            && target.output_id() == output_id
        {
            return Some(target_to_highlight(target));
        }
        if let Some(ref desk) = self.desktop_drag
            && desk.is_active
            && let Some(ref target) = desk.dock_drop_target
            && target.output_id() == output_id
        {
            return Some(target_to_highlight(target));
        }
        None
    }
}

fn target_to_highlight(target: &DockDropTarget) -> crate::shell_render::DockDropHighlight<'_> {
    match target {
        DockDropTarget::App {
            slot_idx, label, ..
        } => crate::shell_render::DockDropHighlight::Slot {
            slot_idx: *slot_idx,
            kind: crate::shell_render::DockDropKind::OpenInApp,
            label,
        },
        DockDropTarget::Trash { slot_idx, .. } => crate::shell_render::DockDropHighlight::Slot {
            slot_idx: *slot_idx,
            kind: crate::shell_render::DockDropKind::MoveToTrash,
            label: "Trash",
        },
        DockDropTarget::Downloads { slot_idx, .. } => {
            crate::shell_render::DockDropHighlight::Slot {
                slot_idx: *slot_idx,
                kind: crate::shell_render::DockDropKind::CopyToDownloads,
                label: "Downloads",
            }
        }
        DockDropTarget::PinInsert { insert_idx, .. } => {
            crate::shell_render::DockDropHighlight::PinInsert {
                insert_idx: *insert_idx,
                label: "",
            }
        }
    }
}

fn resolve_downloads_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads"))
}

fn copy_entry_unique(src: &Path, dest_dir: &Path) -> std::io::Result<()> {
    let file_name = src.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing file name")
    })?;
    let mut dest = dest_dir.join(file_name);
    if dest == src {
        return Ok(());
    }
    if dest.exists() {
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("item");
        let ext = src.extension().and_then(|e| e.to_str());
        for idx in 2..=999_u32 {
            let candidate = ext.map_or_else(
                || format!("{stem} ({idx})"),
                |e| format!("{stem} ({idx}).{e}"),
            );
            let path = dest_dir.join(candidate);
            if !path.exists() {
                dest = path;
                break;
            }
        }
    }
    if src.is_dir() {
        copy_dir_recursive(src, &dest)
    } else {
        fs::copy(src, &dest).map(|_| ())
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    if dest.starts_with(src) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cannot copy directory into itself",
        ));
    }
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let child_src = entry.path();
        let child_dest = dest.join(entry.file_name());
        if child_src.is_dir() {
            copy_dir_recursive(&child_src, &child_dest)?;
        } else {
            fs::copy(&child_src, &child_dest)?;
        }
    }
    Ok(())
}
