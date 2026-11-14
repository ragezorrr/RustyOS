//! Application state, tabs, navigation history, background loaders/transfers,
//! and Stage B interactive flows for `rusty-files` (`RUSTYOS-065`, `RUSTYOS-066`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use rusty_services::file_manager::{
    DecodedPreviewImage, FileDetailedInfo, FileEntry, FileEntryKind, FileSortField, FileSortOrder,
    FileTagColor, FileTagsConfig, FileViewMode, FilesThemeMode, ListingTarget, SidebarLocation,
    TextPreview, TransferConflictDecision, TransferKind, TransferOutcome, TransferProgress,
    UserFileLocations, VolumeInfo, copy_or_move_with_progress, create_unique_folder,
    discover_mounts, empty_trash, filter_and_sort_entries, inspect_entry_info,
    list_entries_for_target, load_image_preview_bgra, move_to_trash, read_text_preview,
    rename_entry, resolve_go_to_path, restore_from_trash, suggest_go_to_paths,
};

/// Fixed column width in Miller Columns mode.
pub(crate) const MILLER_COLUMN_WIDTH: i32 = 216;
/// Row height for Miller column rows.
pub(crate) const ROW_HEIGHT: i32 = 26;
/// Row height for Golden Gate sidebar rows.
pub(crate) const SIDEBAR_ROW_HEIGHT: i32 = 23;
/// Section header height in the Golden Gate sidebar.
pub(crate) const SIDEBAR_SECTION_HEIGHT: i32 = 18;
/// List view row height.
pub(crate) const LIST_ROW_HEIGHT: i32 = 28;
/// Icon grid cell width.
pub(crate) const ICON_CELL_W: i32 = 122;
/// Icon grid cell height.
pub(crate) const ICON_CELL_H: i32 = 114;
/// Top tab strip bar height when visible.
pub(crate) const TAB_BAR_HEIGHT: i32 = 30;
/// Width of the right `Information` inspector pane in Gallery mode.
pub(crate) const GALLERY_INFO_WIDTH: i32 = 246;
/// Height of the bottom thumbnail filmstrip + action row in Gallery mode.
pub(crate) const GALLERY_BOTTOM_H: i32 = 104;
/// Maximum number of cached image/text previews kept in memory.
const MAX_PREVIEW_CACHE_ITEMS: usize = 96;

/// Exponential smooth-scroll & subpixel accumulator for responsive 60/120/144/240 Hz scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct SmoothAxis {
    pub pos: f64,
    pub target: f64,
    pub max_scroll: i32,
}

impl SmoothAxis {
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn rounded(self) -> i32 {
        self.pos.round() as i32
    }

    pub(crate) fn set_immediate(&mut self, value: i32) {
        let clamped = f64::from(value.max(0));
        self.pos = clamped;
        self.target = clamped;
    }

    pub(crate) fn reset(&mut self) {
        self.pos = 0.0;
        self.target = 0.0;
        self.max_scroll = 0;
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn impulse(&mut self, delta: f64, max_scroll: i32, smooth: bool) -> i32 {
        self.max_scroll = max_scroll.max(0);
        let ceiling = f64::from(self.max_scroll);
        if smooth {
            self.target = (self.target + delta).clamp(0.0, ceiling);
        } else {
            let next = (self.pos + delta).clamp(0.0, ceiling);
            self.pos = next;
            self.target = next;
        }
        self.pos.round() as i32
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn tick(&mut self, dt_secs: f64) -> (i32, bool, bool) {
        let ceiling = f64::from(self.max_scroll.max(0));
        self.target = self.target.clamp(0.0, ceiling);
        let diff = self.target - self.pos;
        if diff.abs() < 0.35 {
            let changed = (self.pos - self.target).abs() > 0.01;
            self.pos = self.target;
            return (self.pos.round() as i32, changed, false);
        }
        let prev_px = self.pos.round() as i32;
        let alpha = 1.0 - (-dt_secs * 24.0).exp();
        self.pos = (self.pos + diff * alpha).clamp(0.0, ceiling);
        let next_px = self.pos.round() as i32;
        (next_px, next_px != prev_px, true)
    }

    pub(crate) fn is_active(self) -> bool {
        (self.target - self.pos).abs() >= 0.35
    }
}

/// Icon classification for Golden Gate sidebar entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarIconKind {
    Recents,
    Shared,
    Applications,
    Desktop,
    Documents,
    Downloads,
    Home,
    Volume,
    Network,
    Trash,
    Tag(FileTagColor),
}

/// One row rendered inside the floating translucent sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidebarRowItem {
    /// Optional uppercase-style section caption displayed immediately above this row.
    pub section: Option<&'static str>,
    /// Display label.
    pub label: String,
    /// Target navigation location.
    pub location: SidebarLocation,
    /// Icon to draw left of the label.
    pub icon: SidebarIconKind,
}

/// Single Miller column (or root listing in List view).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ColumnPane {
    /// Backing target directory or virtual folder.
    pub target: ListingTarget,
    /// Header/breadcrumb title.
    pub title: String,
    /// Unfiltered raw entries returned by background loader.
    pub raw_entries: Vec<FileEntry>,
    /// Filtered & sorted entries currently visible.
    pub visible_entries: Vec<FileEntry>,
    /// Currently selected row index in `visible_entries`.
    pub selected_index: Option<usize>,
    /// Vertical scroll offset in pixels (`>= 0`).
    pub scroll_y: i32,
    /// Floating-point smooth scroll state paired with `scroll_y`.
    pub scroll_anim: SmoothAxis,
    /// True while background thread is still loading this column.
    pub loading: bool,
    /// Last load error if directory failed to read.
    pub error: Option<String>,
}

impl ColumnPane {
    fn new_loading(target: ListingTarget, title: String) -> Self {
        Self {
            target,
            title,
            raw_entries: Vec::new(),
            visible_entries: Vec::new(),
            selected_index: None,
            scroll_y: 0,
            scroll_anim: SmoothAxis::default(),
            loading: true,
            error: None,
        }
    }

    /// Returns currently selected entry if valid.
    #[must_use]
    pub(crate) fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_index
            .and_then(|idx| self.visible_entries.get(idx))
    }

    /// Re-applies active search filter and sort settings, preserving selection by path.
    pub(crate) fn refresh_view(
        &mut self,
        show_hidden: bool,
        query: &str,
        sort_field: FileSortField,
        sort_order: FileSortOrder,
    ) {
        let selected_path = self.selected_entry().map(|entry| entry.path.clone());
        self.visible_entries = filter_and_sort_entries(
            self.raw_entries.clone(),
            show_hidden,
            query,
            sort_field,
            sort_order,
        );
        self.selected_index = selected_path
            .as_ref()
            .and_then(|p| self.visible_entries.iter().position(|e| &e.path == p));
    }
}

/// Recorded history snapshot for `<` Back and `>` Forward pills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavigationHistoryEntry {
    pub target: ListingTarget,
    pub title: String,
    pub sidebar: Option<SidebarLocation>,
}

/// Active floating popover menu inside `rusty-files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenPopupMenu {
    /// Sort criterion & order menu anchored below Sort pill.
    SortMenu,
    /// Actions menu anchored below `...` More pill.
    ActionsMenu,
    /// Quick tag color assignment menu anchored below Tag pill.
    TagPicker { target_path: PathBuf },
    /// Right-click context menu anchored at cursor `(x, y)`.
    ContextMenu {
        x: i32,
        y: i32,
        target_entry: Option<FileEntry>,
        target_dir: Option<PathBuf>,
    },
}

/// Completed background listing payload delivered over `mpsc`.
#[derive(Debug)]
pub(crate) struct LoadJobResult {
    pub generation: u64,
    pub col_idx: usize,
    pub target: ListingTarget,
    pub entries: Result<Vec<FileEntry>, String>,
}

/// Snapshot of an open Finder tab.
#[derive(Debug, Clone)]
pub(crate) struct FileTabState {
    pub columns: Vec<ColumnPane>,
    pub active_column: usize,
    pub column_scroll_x: i32,
    pub list_scroll_y: i32,
    pub icons_scroll_y: i32,
    pub gallery_strip_scroll_x: i32,
    pub active_sidebar: Option<SidebarLocation>,
    pub back_stack: Vec<NavigationHistoryEntry>,
    pub forward_stack: Vec<NavigationHistoryEntry>,
    pub search_query: String,
    pub window_title: String,
    pub loader_gen: u64,
}

/// Cached visual/text preview payload used in `Icons`, `Gallery`, `Quick Look`, and `Get Info`.
#[derive(Debug, Clone)]
pub(crate) enum CachedPreview {
    Image(DecodedPreviewImage),
    Text(TextPreview),
    Unavailable,
}

type PreviewCacheKey = (PathBuf, Option<i64>, usize, usize);

#[derive(Debug, Clone)]
struct PreviewCacheEntry {
    key: PreviewCacheKey,
    content: CachedPreview,
}

#[derive(Debug)]
struct PreviewJobResult {
    key: PreviewCacheKey,
    content: CachedPreview,
}

#[derive(Debug)]
struct InfoJobResult {
    path: PathBuf,
    info: Result<FileDetailedInfo, String>,
}

/// Staged file clipboard paths for `Copy` / `Cut` / `Paste`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClipboardItemSet {
    pub kind: TransferKind,
    pub paths: Vec<PathBuf>,
}

/// Active `Quick Look` overlay state (`Space`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuickLookState {
    pub entry: FileEntry,
}

/// Active `Get Info` sheet state (`Ctrl+I`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GetInfoState {
    pub target_path: PathBuf,
    pub loading: bool,
    pub info: Option<FileDetailedInfo>,
    pub error: Option<String>,
    pub more_expanded: bool,
}

/// Active `Go to Folder...` sheet state (`Ctrl+Shift+G`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoToFolderState {
    pub input: String,
    pub suggestions: Vec<PathBuf>,
    pub selected_suggestion: usize,
    pub error: Option<String>,
}

/// Active inline `Rename` editor state (`F2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineRenameState {
    pub target_path: PathBuf,
    pub col_idx: usize,
    pub row_idx: usize,
    pub draft: String,
    pub error: Option<String>,
}

/// Interactive conflict prompt paused on background transfer worker.
#[derive(Debug)]
pub(crate) struct ConflictPrompt {
    pub source: PathBuf,
    pub dest: PathBuf,
    reply_tx: Option<mpsc::SyncSender<TransferConflictDecision>>,
}

/// Active background Copy/Move UI state + cancellation handle.
#[derive(Debug)]
pub(crate) struct ActiveTransferState {
    pub progress: TransferProgress,
    pub cancel_flag: Arc<AtomicBool>,
    pub conflict: Option<ConflictPrompt>,
}

#[derive(Debug)]
enum TransferWorkerMsg {
    Progress(TransferProgress),
    Conflict {
        source: PathBuf,
        dest: PathBuf,
        reply_tx: mpsc::SyncSender<TransferConflictDecision>,
    },
    Finished {
        kind: TransferKind,
        dest_dir: PathBuf,
        result: Result<TransferOutcome, String>,
    },
}

/// Startup options parsed from CLI flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StartupOptions {
    pub initial_path: Option<PathBuf>,
    pub open_trash: bool,
    pub open_recents: bool,
    pub theme_override: Option<FilesThemeMode>,
}

/// Window maximize state requested by compositor or traffic-light toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WindowSizeState {
    #[default]
    Normal,
    Maximized,
}

/// Application loop run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AppRunState {
    #[default]
    Running,
    Exiting,
}

/// Complete application model for `rusty-files`.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct FilesAppState {
    pub locations: UserFileLocations,
    pub tags_config: FileTagsConfig,
    pub volumes: Vec<VolumeInfo>,
    pub sidebar_items: Vec<SidebarRowItem>,
    pub active_sidebar: Option<SidebarLocation>,
    pub view_mode: FileViewMode,
    pub theme: FilesThemeMode,
    pub sort_field: FileSortField,
    pub sort_order: FileSortOrder,
    pub show_hidden: bool,
    pub search_active: bool,
    pub search_query: String,
    pub columns: Vec<ColumnPane>,
    pub active_column: usize,
    pub column_scroll_x: i32,
    pub list_scroll_y: i32,
    pub icons_scroll_y: i32,
    pub gallery_strip_scroll_x: i32,
    pub last_icon_grid_cols: usize,
    pub last_content_w: i32,
    pub follow_right_column: bool,
    pub sidebar_scroll_y: i32,
    column_scroll_x_anim: SmoothAxis,
    list_scroll_anim: SmoothAxis,
    icons_scroll_anim: SmoothAxis,
    gallery_strip_scroll_anim: SmoothAxis,
    sidebar_scroll_anim: SmoothAxis,
    last_anim_tick: Option<Instant>,
    pub caret_visible: bool,
    last_caret_tick: Instant,
    pub back_stack: Vec<NavigationHistoryEntry>,
    pub forward_stack: Vec<NavigationHistoryEntry>,
    pub tabs: Vec<FileTabState>,
    pub active_tab: usize,
    pub show_tab_bar: bool,
    pub gallery_more_info: bool,
    pub popup: Option<OpenPopupMenu>,
    pub clipboard: Option<ClipboardItemSet>,
    pub quick_look: Option<QuickLookState>,
    pub get_info: Option<GetInfoState>,
    pub go_to_folder: Option<GoToFolderState>,
    pub inline_rename: Option<InlineRenameState>,
    pub active_transfer: Option<ActiveTransferState>,
    pub status_message: Option<String>,
    pub window_title: String,
    pub window_size: WindowSizeState,
    pub dirty: bool,
    pub run_state: AppRunState,
    pub loader_gen: u64,
    result_tx: mpsc::Sender<LoadJobResult>,
    result_rx: mpsc::Receiver<LoadJobResult>,
    preview_cache: Vec<PreviewCacheEntry>,
    pending_previews: BTreeSet<PreviewCacheKey>,
    preview_tx: mpsc::Sender<PreviewJobResult>,
    preview_rx: mpsc::Receiver<PreviewJobResult>,
    cached_info: Option<(PathBuf, Result<FileDetailedInfo, String>)>,
    pending_info_path: Option<PathBuf>,
    info_tx: mpsc::Sender<InfoJobResult>,
    info_rx: mpsc::Receiver<InfoJobResult>,
    transfer_tx: mpsc::Sender<TransferWorkerMsg>,
    transfer_rx: mpsc::Receiver<TransferWorkerMsg>,
}

impl FilesAppState {
    /// Creates application state using XDG locations, persisted `file_tags.toml`, and CLI options.
    #[must_use]
    pub(crate) fn new(locations: UserFileLocations, opts: &StartupOptions) -> Self {
        let tags_config = FileTagsConfig::load_from_path(&locations.tags_config_path());
        let volumes = discover_mounts();
        let (result_tx, result_rx) = mpsc::channel();
        let (preview_tx, preview_rx) = mpsc::channel();
        let (info_tx, info_rx) = mpsc::channel();
        let (transfer_tx, transfer_rx) = mpsc::channel();
        let sidebar_items = build_sidebar_items(&locations, &volumes);

        let theme = opts.theme_override.unwrap_or(tags_config.theme);
        let view_mode = tags_config.view_mode;

        let mut app = Self {
            sort_field: tags_config.sort_field,
            sort_order: tags_config.sort_order,
            show_hidden: tags_config.show_hidden,
            locations,
            tags_config,
            volumes,
            sidebar_items,
            active_sidebar: None,
            view_mode,
            theme,
            search_active: false,
            search_query: String::new(),
            columns: Vec::new(),
            active_column: 0,
            column_scroll_x: 0,
            list_scroll_y: 0,
            icons_scroll_y: 0,
            gallery_strip_scroll_x: 0,
            last_icon_grid_cols: 5,
            last_content_w: 836,
            follow_right_column: true,
            sidebar_scroll_y: 0,
            column_scroll_x_anim: SmoothAxis::default(),
            list_scroll_anim: SmoothAxis::default(),
            icons_scroll_anim: SmoothAxis::default(),
            gallery_strip_scroll_anim: SmoothAxis::default(),
            sidebar_scroll_anim: SmoothAxis::default(),
            last_anim_tick: None,
            caret_visible: true,
            last_caret_tick: Instant::now(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            tabs: Vec::new(),
            active_tab: 0,
            show_tab_bar: false,
            gallery_more_info: false,
            popup: None,
            clipboard: None,
            quick_look: None,
            get_info: None,
            go_to_folder: None,
            inline_rename: None,
            active_transfer: None,
            status_message: None,
            window_title: String::from("Files"),
            window_size: WindowSizeState::Normal,
            dirty: true,
            run_state: AppRunState::Running,
            loader_gen: 1,
            result_tx,
            result_rx,
            preview_cache: Vec::new(),
            pending_previews: BTreeSet::new(),
            preview_tx,
            preview_rx,
            cached_info: None,
            pending_info_path: None,
            info_tx,
            info_rx,
            transfer_tx,
            transfer_rx,
        };

        if opts.open_trash {
            app.navigate_sidebar(SidebarLocation::Trash, false);
        } else if opts.open_recents {
            app.navigate_sidebar(SidebarLocation::Recents, false);
        } else if let Some(path) = &opts.initial_path {
            app.open_initial_path(path);
        } else {
            app.navigate_sidebar(SidebarLocation::Home, false);
        }
        app.tabs.push(app.snapshot_active_tab());
        app
    }

    /// True while event loop should stay alive.
    #[must_use]
    pub(crate) const fn is_running(&self) -> bool {
        matches!(self.run_state, AppRunState::Running)
    }

    /// Marks the window to exit cleanly on the next event loop tick.
    pub(crate) fn request_quit(&mut self) {
        self.run_state = AppRunState::Exiting;
    }

    /// True if compositor or traffic-light toggle has the window maximized.
    #[must_use]
    pub(crate) const fn is_maximized(&self) -> bool {
        matches!(self.window_size, WindowSizeState::Maximized)
    }

    /// Updates maximized state flag.
    pub(crate) fn set_maximized(&mut self, maximized: bool) {
        self.window_size = if maximized {
            WindowSizeState::Maximized
        } else {
            WindowSizeState::Normal
        };
    }

    fn open_initial_path(&mut self, path: &Path) {
        let matched = match_path_to_sidebar(&self.locations, &self.volumes, path);
        self.active_sidebar.clone_from(&matched);
        let title = path_display_title(path);
        self.open_root_target(
            ListingTarget::Directory(path.to_path_buf()),
            title,
            matched,
            false,
        );
    }

    /// Navigates to a sidebar destination cleanly.
    pub(crate) fn navigate_sidebar(&mut self, location: SidebarLocation, push_history: bool) {
        let target = self.locations.resolve_target(&location);
        let title = sidebar_label_for(&location, &self.locations, &self.volumes);
        self.active_sidebar = Some(location.clone());
        self.open_root_target(target, title, Some(location), push_history);
    }

    /// Replaces the root view with `target` and schedules non-blocking background loading.
    pub(crate) fn open_root_target(
        &mut self,
        target: ListingTarget,
        title: String,
        sidebar: Option<SidebarLocation>,
        push_history: bool,
    ) {
        if push_history && let Some(first) = self.columns.first() {
            if first.target == target {
                return;
            }
            self.back_stack.push(NavigationHistoryEntry {
                target: first.target.clone(),
                title: first.title.clone(),
                sidebar: self.active_sidebar.clone(),
            });
            self.forward_stack.clear();
        }
        self.popup = None;
        self.inline_rename = None;
        self.loader_gen = self.loader_gen.wrapping_add(1);
        self.active_sidebar = sidebar;
        self.active_column = 0;
        self.column_scroll_x = 0;
        self.list_scroll_y = 0;
        self.icons_scroll_y = 0;
        self.gallery_strip_scroll_x = 0;
        self.follow_right_column = true;
        self.column_scroll_x_anim.reset();
        self.list_scroll_anim.reset();
        self.icons_scroll_anim.reset();
        self.gallery_strip_scroll_anim.reset();
        self.window_title.clone_from(&title);
        self.columns = vec![ColumnPane::new_loading(target.clone(), title)];
        self.spawn_load_job(0, target);
        self.dirty = true;
    }

    /// True if history Back stack is non-empty.
    #[must_use]
    pub(crate) const fn can_go_back(&self) -> bool {
        !self.back_stack.is_empty()
    }

    /// True if history Forward stack is non-empty.
    #[must_use]
    pub(crate) const fn can_go_forward(&self) -> bool {
        !self.forward_stack.is_empty()
    }

    /// Navigates backward in history (`<`).
    pub(crate) fn go_back(&mut self) {
        let Some(prev) = self.back_stack.pop() else {
            return;
        };
        if let Some(cur) = self.columns.first() {
            self.forward_stack.push(NavigationHistoryEntry {
                target: cur.target.clone(),
                title: cur.title.clone(),
                sidebar: self.active_sidebar.clone(),
            });
        }
        self.open_root_target(prev.target, prev.title, prev.sidebar, false);
    }

    /// Navigates forward in history (`>`).
    pub(crate) fn go_forward(&mut self) {
        let Some(next) = self.forward_stack.pop() else {
            return;
        };
        if let Some(cur) = self.columns.first() {
            self.back_stack.push(NavigationHistoryEntry {
                target: cur.target.clone(),
                title: cur.title.clone(),
                sidebar: self.active_sidebar.clone(),
            });
        }
        self.open_root_target(next.target, next.title, next.sidebar, false);
    }

    /// Polls completed background directory reads, previews, info inspection, and transfers.
    pub(crate) fn poll_background_loads(&mut self) -> bool {
        let mut changed = self.poll_directory_jobs();
        if self.poll_preview_jobs() {
            changed = true;
        }
        if self.poll_info_jobs() {
            changed = true;
        }
        if self.poll_transfer_jobs() {
            changed = true;
        }
        if changed {
            self.dirty = true;
        }
        changed
    }

    fn poll_directory_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.result_rx.try_recv() {
            if msg.generation != self.loader_gen {
                continue;
            }
            let Some(col) = self.columns.get_mut(msg.col_idx) else {
                continue;
            };
            if col.target != msg.target {
                continue;
            }
            col.loading = false;
            match msg.entries {
                Ok(entries) => {
                    col.raw_entries = entries;
                    col.error = None;
                    col.refresh_view(
                        self.show_hidden,
                        &self.search_query,
                        self.sort_field,
                        self.sort_order,
                    );
                }
                Err(err) => {
                    col.raw_entries.clear();
                    col.visible_entries.clear();
                    col.error = Some(err);
                }
            }
            changed = true;
        }
        if changed {
            self.ensure_stage_b_assets_requested();
        }
        changed
    }

    fn poll_preview_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(job) = self.preview_rx.try_recv() {
            self.pending_previews.remove(&job.key);
            self.insert_preview_cache(job.key, job.content);
            changed = true;
        }
        changed
    }

    fn poll_info_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(job) = self.info_rx.try_recv() {
            if self.pending_info_path.as_ref() == Some(&job.path) {
                self.pending_info_path = None;
            }
            if let Some(gi) = &mut self.get_info
                && gi.target_path == job.path
            {
                gi.loading = false;
                match &job.info {
                    Ok(info) => {
                        gi.info = Some(info.clone());
                        gi.error = None;
                    }
                    Err(err) => {
                        gi.error = Some(err.clone());
                    }
                }
            }
            self.cached_info = Some((job.path, job.info));
            changed = true;
        }
        changed
    }

    fn poll_transfer_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.transfer_rx.try_recv() {
            changed = true;
            match msg {
                TransferWorkerMsg::Progress(progress) => {
                    if let Some(tr) = &mut self.active_transfer {
                        tr.progress = progress;
                    }
                }
                TransferWorkerMsg::Conflict {
                    source,
                    dest,
                    reply_tx,
                } => {
                    if let Some(tr) = &mut self.active_transfer {
                        tr.conflict = Some(ConflictPrompt {
                            source,
                            dest,
                            reply_tx: Some(reply_tx),
                        });
                    } else {
                        let _ = reply_tx.send(TransferConflictDecision::Stop);
                    }
                }
                TransferWorkerMsg::Finished {
                    kind,
                    dest_dir,
                    result,
                } => {
                    self.active_transfer = None;
                    self.handle_transfer_finished(kind, &dest_dir, result);
                }
            }
        }
        changed
    }

    fn handle_transfer_finished(
        &mut self,
        kind: TransferKind,
        dest_dir: &Path,
        result: Result<TransferOutcome, String>,
    ) {
        match result {
            Ok(outcome) => {
                if kind == TransferKind::Move && !outcome.created_paths.is_empty() {
                    self.clipboard = None;
                }
                let verb = match kind {
                    TransferKind::Copy => "Copied",
                    TransferKind::Move => "Moved",
                };
                if outcome.cancelled {
                    self.status_message = Some(format!(
                        "{verb} stopped ({} completed)",
                        outcome.created_paths.len()
                    ));
                } else {
                    self.status_message = Some(format!(
                        "{verb} {} item{}",
                        outcome.created_paths.len(),
                        if outcome.created_paths.len() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    ));
                }
                self.reload_directory(dest_dir);
                self.reload_active_column();
            }
            Err(err) => {
                self.status_message = Some(format!("Transfer failed: {err}"));
            }
        }
    }

    fn spawn_load_job(&self, col_idx: usize, target: ListingTarget) {
        let tx = self.result_tx.clone();
        let gen_id = self.loader_gen;
        let locations = self.locations.clone();
        let tags = self.tags_config.clone();
        thread::spawn(move || {
            let res =
                list_entries_for_target(&target, &locations, &tags).map_err(|e| e.to_string());
            let _ = tx.send(LoadJobResult {
                generation: gen_id,
                col_idx,
                target,
                entries: res,
            });
        });
    }

    /// Synchronous helper for deterministic tests without thread timing jitter.
    #[cfg(test)]
    pub(crate) fn complete_pending_loads_sync(&mut self) {
        for idx in 0..self.columns.len() {
            if self.columns[idx].loading {
                let target = self.columns[idx].target.clone();
                let res = list_entries_for_target(&target, &self.locations, &self.tags_config);
                let col = &mut self.columns[idx];
                col.loading = false;
                match res {
                    Ok(entries) => {
                        col.raw_entries = entries;
                        col.error = None;
                        col.refresh_view(
                            self.show_hidden,
                            &self.search_query,
                            self.sort_field,
                            self.sort_order,
                        );
                    }
                    Err(e) => {
                        col.error = Some(e.to_string());
                    }
                }
            }
        }
        while self.result_rx.try_recv().is_ok() {}
        self.ensure_stage_b_assets_requested();
    }

    /// Handles row selection inside `col_idx` (expands Miller child column if directory).
    pub(crate) fn select_entry(&mut self, col_idx: usize, row_idx: usize, double_click: bool) {
        self.popup = None;
        self.inline_rename = None;
        let Some(entry) = self
            .columns
            .get(col_idx)
            .and_then(|c| c.visible_entries.get(row_idx))
            .cloned()
        else {
            return;
        };
        if let Some(col) = self.columns.get_mut(col_idx) {
            col.selected_index = Some(row_idx);
        }
        self.active_column = col_idx;

        if self.view_mode == FileViewMode::Columns {
            self.columns.truncate(col_idx + 1);
            if entry.is_navigable_dir() && !self.is_trash_view() {
                let child_target = ListingTarget::Directory(entry.path.clone());
                let child_col = ColumnPane::new_loading(child_target.clone(), entry.name.clone());
                self.columns.push(child_col);
                self.window_title.clone_from(&self.columns[0].title);
                self.spawn_load_job(col_idx + 1, child_target);
            } else if double_click {
                self.launch_entry(&entry);
            }
        } else if double_click {
            if entry.is_navigable_dir() && !self.is_trash_view() {
                self.open_root_target(
                    ListingTarget::Directory(entry.path.clone()),
                    entry.name.clone(),
                    None,
                    true,
                );
            } else {
                self.launch_entry(&entry);
            }
        }

        if let Some(ql) = &mut self.quick_look {
            ql.entry = entry.clone();
        }
        self.ensure_stage_b_assets_for_selection(&entry);
        self.follow_right_column = true;
        self.dirty = true;
    }

    /// Activates the currently selected item (opens folder into next column or launches file).
    pub(crate) fn activate_selected(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if entry.is_navigable_dir() && !self.is_trash_view() {
            if self.view_mode == FileViewMode::Columns {
                if self.columns.get(self.active_column + 1).is_some() {
                    self.active_column += 1;
                    self.follow_right_column = true;
                    if let Some(next_col) = self.columns.get_mut(self.active_column)
                        && next_col.selected_index.is_none()
                        && !next_col.visible_entries.is_empty()
                    {
                        next_col.selected_index = Some(0);
                    }
                    self.dirty = true;
                }
            } else {
                self.open_root_target(ListingTarget::Directory(entry.path), entry.name, None, true);
            }
        } else {
            self.launch_entry(&entry);
        }
    }

    /// Launches a `.desktop` application or opens a regular document with `xdg-open`.
    pub(crate) fn launch_entry(&mut self, entry: &FileEntry) {
        if entry.kind == FileEntryKind::Application
            && let Some(exec_line) = &entry.desktop_exec
        {
            if let Some(parts) = parse_desktop_exec_tokens(exec_line)
                && let Some((program, args)) = parts.split_first()
            {
                let res = Command::new(program)
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                self.status_message = Some(match res {
                    Ok(_) => format!("Opened {}", entry.name),
                    Err(e) => format!("Failed to open {}: {e}", entry.name),
                });
                self.dirty = true;
            }
            return;
        }
        if !entry.is_navigable_dir() {
            let res = Command::new("xdg-open")
                .arg(&entry.path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            self.status_message = Some(match res {
                Ok(_) => format!("Opening {}", entry.name),
                Err(e) => format!("Failed to open {}: {e}", entry.name),
            });
            self.dirty = true;
        }
    }

    /// Changes view mode (`Icons`, `List`, `Columns`, `Gallery`) and persists preference.
    pub(crate) fn set_view_mode(&mut self, mode: FileViewMode) {
        self.popup = None;
        self.inline_rename = None;
        if self.view_mode != mode {
            self.view_mode = mode;
            if mode != FileViewMode::Columns && self.columns.len() > 1 {
                let keep_idx = self.active_column.min(self.columns.len() - 1);
                let chosen = self.columns.swap_remove(keep_idx);
                self.columns = vec![chosen];
                self.active_column = 0;
                self.window_title.clone_from(&self.columns[0].title);
            }
            if mode == FileViewMode::Gallery
                && let Some(col) = self.columns.first_mut()
                && col.selected_index.is_none()
                && !col.visible_entries.is_empty()
            {
                col.selected_index = Some(0);
            }
            self.tags_config.view_mode = mode;
            self.persist_preferences();
            self.status_message = Some(format!("Switched to {} view", mode.label()));
            self.ensure_stage_b_assets_requested();
            self.dirty = true;
        }
    }

    /// Toggles Light Golden Gate <-> Dark Purple Glass theme and saves `file_tags.toml`.
    pub(crate) fn toggle_theme(&mut self) {
        self.theme = self.theme.toggle();
        self.tags_config.theme = self.theme;
        self.persist_preferences();
        self.status_message = Some(match self.theme {
            FilesThemeMode::LightGoldenGate => String::from("Light Golden Gate appearance"),
            FilesThemeMode::DarkPurpleGlass => String::from("Dark Purple Glass appearance"),
        });
        self.dirty = true;
    }

    /// Changes active sort criterion (flipping order if same field is selected again).
    pub(crate) fn set_sort(&mut self, field: FileSortField) {
        if self.sort_field == field {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_field = field;
            self.sort_order = FileSortOrder::Ascending;
        }
        self.tags_config.sort_field = self.sort_field;
        self.tags_config.sort_order = self.sort_order;
        self.persist_preferences();
        self.reapply_filters_all_columns();
        self.popup = None;
        self.dirty = true;
    }

    /// Explicitly sets sort direction (`Ascending` or `Descending`).
    pub(crate) fn set_sort_order(&mut self, order: FileSortOrder) {
        self.sort_order = order;
        self.tags_config.sort_order = order;
        self.persist_preferences();
        self.reapply_filters_all_columns();
        self.popup = None;
        self.dirty = true;
    }

    /// Toggles dot-file visibility and persists preference.
    pub(crate) fn toggle_show_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.tags_config.show_hidden = self.show_hidden;
        self.persist_preferences();
        self.reapply_filters_all_columns();
        self.popup = None;
        self.dirty = true;
    }

    /// Updates search filter query and refreshes visible column entries.
    pub(crate) fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.reapply_filters_all_columns();
        self.reset_caret_blink();
        self.dirty = true;
    }

    fn reapply_filters_all_columns(&mut self) {
        for col in &mut self.columns {
            col.refresh_view(
                self.show_hidden,
                &self.search_query,
                self.sort_field,
                self.sort_order,
            );
        }
    }

    /// Creates a unique folder in the active directory and refreshes its listing.
    pub(crate) fn action_new_folder(&mut self) {
        self.popup = None;
        let Some(dir) = self.active_writable_directory() else {
            self.status_message = Some(String::from("Cannot create folder in virtual view"));
            self.dirty = true;
            return;
        };
        match create_unique_folder(&dir, "untitled folder") {
            Ok(created) => {
                let name = created
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("untitled folder");
                self.status_message = Some(format!("Created {name}"));
                self.reload_directory(&dir);
            }
            Err(err) => {
                self.status_message = Some(format!("New Folder failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Moves the currently selected entry to Trash, or prompts reload if inside Trash.
    pub(crate) fn action_move_selected_to_trash(&mut self) {
        self.popup = None;
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = Some(String::from("Select an item first"));
            self.dirty = true;
            return;
        };
        self.trash_entry(&entry);
    }

    /// Moves specific `entry` to `FreeDesktop` Trash (updating tags & reloading).
    pub(crate) fn trash_entry(&mut self, entry: &FileEntry) {
        if self.is_trash_view() {
            self.restore_entry(entry);
            return;
        }
        match move_to_trash(&entry.path, &self.locations.trash_dir) {
            Ok(_) => {
                self.tags_config.remove_tags_for(&entry.path);
                self.persist_preferences();
                self.status_message = Some(format!("Moved \"{}\" to Trash", entry.name));
                self.reload_active_column();
            }
            Err(err) => {
                self.status_message = Some(format!("Move to Trash failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Restores a trashed entry back to its original path.
    pub(crate) fn restore_entry(&mut self, entry: &FileEntry) {
        match restore_from_trash(&entry.name, &self.locations.trash_dir) {
            Ok(dest) => {
                self.status_message = Some(format!("Restored to {}", dest.display()));
                self.reload_active_column();
            }
            Err(err) => {
                self.status_message = Some(format!("Restore failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Empties the Trash cleanly.
    pub(crate) fn action_empty_trash(&mut self) {
        self.popup = None;
        match empty_trash(&self.locations.trash_dir) {
            Ok(count) => {
                self.status_message = Some(format!("Emptied Trash ({count} items removed)"));
                if self.is_trash_view() {
                    self.reload_active_column();
                }
            }
            Err(err) => {
                self.status_message = Some(format!("Empty Trash failed: {err}"));
            }
        }
        self.dirty = true;
    }

    /// Toggles `tag` color on `path` and persists `file_tags.toml`.
    pub(crate) fn toggle_tag_on_path(&mut self, path: &Path, tag: FileTagColor) {
        let added = self.tags_config.toggle_tag(path, tag);
        self.persist_preferences();
        let verb = if added { "Tagged" } else { "Removed tag" };
        self.status_message = Some(format!("{verb} {}", tag.label()));
        if matches!(
            self.columns.first().map(|c| &c.target),
            Some(ListingTarget::Tag(_))
        ) {
            self.reload_active_column();
        }
        self.dirty = true;
    }

    fn reload_directory(&mut self, dir: &Path) {
        let target = ListingTarget::Directory(dir.to_path_buf());
        let indices: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (c.target == target).then_some(i))
            .collect();
        for idx in indices {
            self.columns[idx].loading = true;
            self.spawn_load_job(idx, target.clone());
        }
    }

    fn reload_active_column(&mut self) {
        if let Some(col) = self.columns.get_mut(self.active_column) {
            let target = col.target.clone();
            col.loading = true;
            self.spawn_load_job(self.active_column, target);
        }
    }

    fn persist_preferences(&self) {
        let _ = self
            .tags_config
            .save_to_path(&self.locations.tags_config_path());
    }

    /// Currently focused entry across active column/list.
    #[must_use]
    pub(crate) fn selected_entry(&self) -> Option<&FileEntry> {
        self.columns
            .get(self.active_column)
            .and_then(ColumnPane::selected_entry)
    }

    /// True if root location is `ListingTarget::Trash`.
    #[must_use]
    pub(crate) fn is_trash_view(&self) -> bool {
        matches!(
            self.columns.first().map(|c| &c.target),
            Some(ListingTarget::Trash)
        )
    }

    /// Directory where `New Folder` can be created (active column target or parent directory).
    #[must_use]
    pub(crate) fn active_writable_directory(&self) -> Option<PathBuf> {
        if let Some(col) = self.columns.get(self.active_column)
            && let ListingTarget::Directory(path) = &col.target
        {
            return Some(path.clone());
        }
        self.columns.iter().rev().find_map(|c| match &c.target {
            ListingTarget::Directory(p) => Some(p.clone()),
            _ => None,
        })
    }

    /// Returns assigned tag colors for `path`.
    #[must_use]
    pub(crate) fn tags_for(&self, path: &Path) -> &[FileTagColor] {
        self.tags_config.tags_for(path)
    }

    /// Handles keyboard navigation, overlays, tabs, file operations, and search text input.
    pub(crate) fn handle_key_input(
        &mut self,
        key_name: &str,
        utf8: Option<&str>,
        ctrl: bool,
        shift: bool,
    ) {
        if self.handle_modal_key_input(key_name, utf8, ctrl) {
            return;
        }
        if self.handle_shortcut_key_input(key_name, utf8, ctrl, shift) {
            return;
        }
        if key_name == "Escape" {
            if self.popup.is_some() {
                self.popup = None;
            } else if self.search_active {
                if self.search_query.is_empty() {
                    self.search_active = false;
                } else {
                    self.set_search_query(String::new());
                }
            }
            self.dirty = true;
            return;
        }
        if self.search_active {
            if key_name == "BackSpace" {
                self.search_query.pop();
                self.reapply_filters_all_columns();
                self.reset_caret_blink();
                self.dirty = true;
                return;
            }
            if let Some(text) = utf8
                && !ctrl
                && text.chars().all(|c| !c.is_control())
                && !text.is_empty()
            {
                self.search_query.push_str(text);
                self.reapply_filters_all_columns();
                self.reset_caret_blink();
                self.dirty = true;
                return;
            }
        }
        if key_name == "Space" {
            self.toggle_quick_look_selected();
            return;
        }
        self.handle_navigation_key(key_name);
    }

    fn handle_modal_key_input(&mut self, key_name: &str, utf8: Option<&str>, ctrl: bool) -> bool {
        if let Some(tr) = &self.active_transfer
            && tr.conflict.is_some()
        {
            if key_name == "Escape" {
                self.resolve_transfer_conflict(TransferConflictDecision::Stop);
                return true;
            }
            return true;
        }
        if self.inline_rename.is_some() {
            self.handle_inline_rename_key(key_name, utf8, ctrl);
            return true;
        }
        if self.go_to_folder.is_some() {
            self.handle_go_to_folder_key(key_name, utf8, ctrl);
            return true;
        }
        if self.get_info.is_some() && key_name == "Escape" {
            self.get_info = None;
            self.dirty = true;
            return true;
        }
        if self.quick_look.is_some() {
            match key_name {
                "Escape" | "Space" => {
                    self.quick_look = None;
                    self.dirty = true;
                }
                "Left" | "Up" => self.step_quick_look(-1),
                "Right" | "Down" => self.step_quick_look(1),
                "Return" | "KP_Enter" => {
                    self.quick_look = None;
                    self.activate_selected();
                }
                _ => {}
            }
            return true;
        }
        false
    }

    fn handle_shortcut_key_input(
        &mut self,
        key_name: &str,
        utf8: Option<&str>,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        if key_name == "F2" {
            self.start_inline_rename_selected();
            return true;
        }
        if !ctrl {
            return false;
        }
        if key_name == "Tab" {
            self.cycle_next_tab(shift);
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'f') {
            self.search_active = true;
            self.popup = None;
            self.reset_caret_blink();
            self.dirty = true;
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'n') {
            self.action_new_folder();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 't') {
            self.new_tab(None);
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'w') {
            self.close_active_tab();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'i') {
            if self.get_info.is_some() {
                self.get_info = None;
                self.dirty = true;
            } else {
                self.open_get_info_selected();
            }
            return true;
        }
        if shift && key_matches_ascii(key_name, utf8, 'g') {
            self.open_go_to_folder();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'c') {
            self.action_copy_selected();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'x') {
            self.action_cut_selected();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'v') {
            self.action_paste_into_active();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'd') {
            self.action_duplicate_selected();
            return true;
        }
        false
    }

    fn handle_inline_rename_key(&mut self, key_name: &str, utf8: Option<&str>, ctrl: bool) {
        match key_name {
            "Escape" => {
                self.inline_rename = None;
                self.dirty = true;
            }
            "Return" | "KP_Enter" => self.commit_inline_rename(),
            "BackSpace" => {
                if let Some(rn) = &mut self.inline_rename {
                    rn.draft.pop();
                    rn.error = None;
                    self.reset_caret_blink();
                    self.dirty = true;
                }
            }
            _ => {
                if let Some(text) = utf8
                    && !ctrl
                    && text.chars().all(|c| !c.is_control())
                    && !text.is_empty()
                    && let Some(rn) = &mut self.inline_rename
                {
                    rn.draft.push_str(text);
                    rn.error = None;
                    self.reset_caret_blink();
                    self.dirty = true;
                }
            }
        }
    }

    fn handle_go_to_folder_key(&mut self, key_name: &str, utf8: Option<&str>, ctrl: bool) {
        match key_name {
            "Escape" => {
                self.go_to_folder = None;
                self.dirty = true;
            }
            "Return" | "KP_Enter" => self.submit_go_to_folder(),
            "Up" => {
                if let Some(gt) = &mut self.go_to_folder
                    && !gt.suggestions.is_empty()
                {
                    gt.selected_suggestion = gt.selected_suggestion.saturating_sub(1);
                    self.dirty = true;
                }
            }
            "Down" => {
                if let Some(gt) = &mut self.go_to_folder
                    && !gt.suggestions.is_empty()
                {
                    gt.selected_suggestion =
                        (gt.selected_suggestion + 1).min(gt.suggestions.len() - 1);
                    self.dirty = true;
                }
            }
            "Tab" => {
                let current_dir = self.active_writable_directory();
                if let Some(gt) = &mut self.go_to_folder
                    && let Some(chosen) = gt.suggestions.get(gt.selected_suggestion).cloned()
                {
                    gt.input = format_go_to_display(&chosen, &self.locations);
                    gt.error = None;
                    gt.suggestions =
                        suggest_go_to_paths(&gt.input, current_dir.as_deref(), &self.locations, 6);
                    gt.selected_suggestion = 0;
                    self.reset_caret_blink();
                    self.dirty = true;
                }
            }
            "BackSpace" => {
                let current_dir = self.active_writable_directory();
                if let Some(gt) = &mut self.go_to_folder {
                    gt.input.pop();
                    gt.error = None;
                    gt.suggestions =
                        suggest_go_to_paths(&gt.input, current_dir.as_deref(), &self.locations, 6);
                    gt.selected_suggestion = 0;
                    self.reset_caret_blink();
                    self.dirty = true;
                }
            }
            _ => {
                if let Some(text) = utf8
                    && !ctrl
                    && text.chars().all(|c| !c.is_control())
                    && !text.is_empty()
                {
                    let current_dir = self.active_writable_directory();
                    if let Some(gt) = &mut self.go_to_folder {
                        gt.input.push_str(text);
                        gt.error = None;
                        gt.suggestions = suggest_go_to_paths(
                            &gt.input,
                            current_dir.as_deref(),
                            &self.locations,
                            6,
                        );
                        gt.selected_suggestion = 0;
                        self.reset_caret_blink();
                        self.dirty = true;
                    }
                }
            }
        }
    }

    fn handle_navigation_key(&mut self, key_name: &str) {
        match self.view_mode {
            FileViewMode::Icons => {
                let step = isize::try_from(self.last_icon_grid_cols.max(1)).unwrap_or(5);
                match key_name {
                    "Right" => self.move_selection_delta(1),
                    "Left" => self.move_selection_delta(-1),
                    "Down" => self.move_selection_delta(step),
                    "Up" => self.move_selection_delta(-step),
                    "Return" | "KP_Enter" => self.activate_selected(),
                    "BackSpace" => self.go_back(),
                    "Delete" => self.action_move_selected_to_trash(),
                    _ => {}
                }
            }
            FileViewMode::Gallery => match key_name {
                "Right" | "Down" => self.move_selection_delta(1),
                "Left" | "Up" => self.move_selection_delta(-1),
                "Return" | "KP_Enter" => self.activate_selected(),
                "BackSpace" => self.go_back(),
                "Delete" => self.action_move_selected_to_trash(),
                _ => {}
            },
            FileViewMode::Columns | FileViewMode::List => match key_name {
                "Down" => self.move_selection_delta(1),
                "Up" => self.move_selection_delta(-1),
                "Right" | "Return" | "KP_Enter" => self.activate_selected(),
                "Left" => {
                    if self.view_mode == FileViewMode::Columns && self.active_column > 0 {
                        self.active_column -= 1;
                        self.columns.truncate(self.active_column + 1);
                        self.follow_right_column = true;
                        self.dirty = true;
                    }
                }
                "BackSpace" => self.go_back(),
                "Delete" => self.action_move_selected_to_trash(),
                _ => {}
            },
        }
    }

    fn move_selection_delta(&mut self, delta: isize) {
        let Some(col_idx) = Some(self.active_column).filter(|&i| i < self.columns.len()) else {
            return;
        };
        let len = self.columns[col_idx].visible_entries.len();
        if len == 0 {
            return;
        }
        let next_idx = match self.columns[col_idx].selected_index {
            None => 0,
            Some(cur) => {
                let raw = cur.saturating_add_signed(delta);
                raw.min(len - 1)
            }
        };
        self.select_entry(col_idx, next_idx, false);
    }

    /// Computes horizontal column count for `Icons` grid given content viewport width.
    #[must_use]
    pub(crate) fn icon_grid_columns_for_width(content_w: i32) -> usize {
        let usable = (content_w - 28).max(ICON_CELL_W);
        usize::try_from((usable / ICON_CELL_W).max(1)).unwrap_or(1)
    }

    /// Applies continuous (`smooth = false`) or eased (`smooth = true`) scroll impulse to `area`.
    pub(crate) fn scroll_axis(
        &mut self,
        area: ScrollTargetArea,
        delta_px: f64,
        smooth: bool,
        viewport_extent: i32,
    ) {
        match area {
            ScrollTargetArea::Sidebar => {
                let content_h = sidebar_total_height(&self.sidebar_items);
                let max_scroll = (content_h - viewport_extent + 6).max(0);
                if self.sidebar_scroll_anim.rounded() != self.sidebar_scroll_y {
                    self.sidebar_scroll_anim
                        .set_immediate(self.sidebar_scroll_y);
                }
                self.sidebar_scroll_y = self
                    .sidebar_scroll_anim
                    .impulse(delta_px, max_scroll, smooth);
            }
            ScrollTargetArea::List => {
                let count = self
                    .columns
                    .first()
                    .map_or(0, |c| i32::try_from(c.visible_entries.len()).unwrap_or(0));
                let content_h = count * LIST_ROW_HEIGHT;
                let max_scroll = (content_h - viewport_extent + 8).max(0);
                if self.list_scroll_anim.rounded() != self.list_scroll_y {
                    self.list_scroll_anim.set_immediate(self.list_scroll_y);
                }
                self.list_scroll_y = self.list_scroll_anim.impulse(delta_px, max_scroll, smooth);
            }
            ScrollTargetArea::Icons => {
                let count = self.columns.first().map_or(0, |c| c.visible_entries.len());
                let cols = self.last_icon_grid_cols.max(1);
                let rows = i32::try_from(count.div_ceil(cols)).unwrap_or(0);
                let content_h = rows * ICON_CELL_H + 16;
                let max_scroll = (content_h - viewport_extent + 12).max(0);
                if self.icons_scroll_anim.rounded() != self.icons_scroll_y {
                    self.icons_scroll_anim.set_immediate(self.icons_scroll_y);
                }
                self.icons_scroll_y = self.icons_scroll_anim.impulse(delta_px, max_scroll, smooth);
                self.ensure_stage_b_assets_requested();
            }
            ScrollTargetArea::GalleryStrip => {
                let count = self
                    .columns
                    .first()
                    .map_or(0, |c| i32::try_from(c.visible_entries.len()).unwrap_or(0));
                let content_w = count * 70 + 24;
                let max_scroll = (content_w - viewport_extent + 12).max(0);
                if self.gallery_strip_scroll_anim.rounded() != self.gallery_strip_scroll_x {
                    self.gallery_strip_scroll_anim
                        .set_immediate(self.gallery_strip_scroll_x);
                }
                self.gallery_strip_scroll_x = self
                    .gallery_strip_scroll_anim
                    .impulse(delta_px, max_scroll, smooth);
                self.ensure_stage_b_assets_requested();
            }
            ScrollTargetArea::Column(col_idx) => {
                if let Some(col) = self.columns.get_mut(col_idx) {
                    let count = i32::try_from(col.visible_entries.len()).unwrap_or(0);
                    let content_h = count * ROW_HEIGHT;
                    let max_scroll = (content_h - viewport_extent + 12).max(0);
                    if col.scroll_anim.rounded() != col.scroll_y {
                        col.scroll_anim.set_immediate(col.scroll_y);
                    }
                    col.scroll_y = col.scroll_anim.impulse(delta_px, max_scroll, smooth);
                }
            }
        }
        if smooth {
            self.last_anim_tick.get_or_insert_with(Instant::now);
        }
        self.dirty = true;
    }

    /// Scrolls Miller columns horizontally (`Columns` view) or gallery strip horizontally.
    pub(crate) fn scroll_horizontal(&mut self, delta_px: f64, smooth: bool, content_w: i32) {
        if self.view_mode == FileViewMode::Gallery {
            self.scroll_axis(ScrollTargetArea::GalleryStrip, delta_px, smooth, content_w);
            return;
        }
        if self.view_mode != FileViewMode::Columns {
            return;
        }
        let total_cols_w = i32::try_from(self.columns.len()).unwrap_or(1) * MILLER_COLUMN_WIDTH;
        let preview_reserve = if self.selected_entry().is_some_and(|e| !e.is_navigable_dir()) {
            240
        } else {
            0
        };
        let max_overflow = (total_cols_w + preview_reserve - content_w).max(0);
        if max_overflow == 0 {
            return;
        }
        if self.follow_right_column {
            self.column_scroll_x = max_overflow;
            self.column_scroll_x_anim.set_immediate(max_overflow);
            self.follow_right_column = false;
        }
        self.column_scroll_x = self
            .column_scroll_x_anim
            .impulse(delta_px, max_overflow, smooth);
        if smooth {
            self.last_anim_tick.get_or_insert_with(Instant::now);
        }
        self.dirty = true;
    }

    /// Advances smooth scroll animations aligned with compositor presentation cadence.
    pub(crate) fn tick_animations(&mut self, now: Instant) -> bool {
        let Some(prev) = self.last_anim_tick else {
            self.last_anim_tick = Some(now);
            return false;
        };
        let dt = now
            .saturating_duration_since(prev)
            .as_secs_f64()
            .clamp(0.001, 0.033);
        self.last_anim_tick = Some(now);

        let mut moved = false;
        let mut active = false;

        let (v, m, a) = self.sidebar_scroll_anim.tick(dt);
        if m {
            self.sidebar_scroll_y = v;
            moved = true;
        }
        active |= a;

        let (v, m, a) = self.list_scroll_anim.tick(dt);
        if m {
            self.list_scroll_y = v;
            moved = true;
        }
        active |= a;

        let (v, m, a) = self.icons_scroll_anim.tick(dt);
        if m {
            self.icons_scroll_y = v;
            self.ensure_stage_b_assets_requested();
            moved = true;
        }
        active |= a;

        let (v, m, a) = self.gallery_strip_scroll_anim.tick(dt);
        if m {
            self.gallery_strip_scroll_x = v;
            self.ensure_stage_b_assets_requested();
            moved = true;
        }
        active |= a;

        let (v, m, a) = self.column_scroll_x_anim.tick(dt);
        if m {
            self.column_scroll_x = v;
            moved = true;
        }
        active |= a;

        for col in &mut self.columns {
            let (v, m, a) = col.scroll_anim.tick(dt);
            if m {
                col.scroll_y = v;
                moved = true;
            }
            active |= a;
        }

        if !active {
            self.last_anim_tick = None;
        }
        if moved {
            self.dirty = true;
        }
        active
    }

    #[must_use]
    pub(crate) fn has_active_animations(&self) -> bool {
        self.sidebar_scroll_anim.is_active()
            || self.list_scroll_anim.is_active()
            || self.icons_scroll_anim.is_active()
            || self.gallery_strip_scroll_anim.is_active()
            || self.column_scroll_x_anim.is_active()
            || self.columns.iter().any(|c| c.scroll_anim.is_active())
    }

    #[must_use]
    pub(crate) fn has_pending_jobs(&self) -> bool {
        self.columns.iter().any(|c| c.loading)
            || !self.pending_previews.is_empty()
            || self.pending_info_path.is_some()
            || self.active_transfer.is_some()
    }

    #[must_use]
    pub(crate) const fn has_focused_text_input(&self) -> bool {
        self.search_active || self.inline_rename.is_some() || self.go_to_folder.is_some()
    }

    pub(crate) fn reset_caret_blink(&mut self) {
        self.caret_visible = true;
        self.last_caret_tick = Instant::now();
        self.dirty = true;
    }

    #[must_use]
    pub(crate) fn time_until_caret_blink(&self, now: Instant) -> Duration {
        let elapsed = now.saturating_duration_since(self.last_caret_tick);
        Duration::from_millis(530)
            .saturating_sub(elapsed)
            .max(Duration::from_millis(10))
    }

    pub(crate) fn update_caret_blink(&mut self, now: Instant) -> bool {
        if !self.has_focused_text_input() {
            if !self.caret_visible {
                self.caret_visible = true;
            }
            return false;
        }
        if now.saturating_duration_since(self.last_caret_tick) >= Duration::from_millis(530) {
            self.caret_visible = !self.caret_visible;
            self.last_caret_tick = now;
            self.dirty = true;
            return true;
        }
        false
    }

    // --- Tabs ---

    fn snapshot_active_tab(&self) -> FileTabState {
        FileTabState {
            columns: self.columns.clone(),
            active_column: self.active_column,
            column_scroll_x: self.column_scroll_x,
            list_scroll_y: self.list_scroll_y,
            icons_scroll_y: self.icons_scroll_y,
            gallery_strip_scroll_x: self.gallery_strip_scroll_x,
            active_sidebar: self.active_sidebar.clone(),
            back_stack: self.back_stack.clone(),
            forward_stack: self.forward_stack.clone(),
            search_query: self.search_query.clone(),
            window_title: self.window_title.clone(),
            loader_gen: self.loader_gen,
        }
    }

    fn apply_tab_snapshot(&mut self, tab: FileTabState) {
        self.columns = tab.columns;
        self.active_column = tab.active_column;
        self.column_scroll_x = tab.column_scroll_x;
        self.list_scroll_y = tab.list_scroll_y;
        self.icons_scroll_y = tab.icons_scroll_y;
        self.gallery_strip_scroll_x = tab.gallery_strip_scroll_x;
        self.column_scroll_x_anim.set_immediate(tab.column_scroll_x);
        self.list_scroll_anim.set_immediate(tab.list_scroll_y);
        self.icons_scroll_anim.set_immediate(tab.icons_scroll_y);
        self.gallery_strip_scroll_anim
            .set_immediate(tab.gallery_strip_scroll_x);
        self.follow_right_column = true;
        self.active_sidebar = tab.active_sidebar;
        self.back_stack = tab.back_stack;
        self.forward_stack = tab.forward_stack;
        self.search_query = tab.search_query;
        self.window_title = tab.window_title;
        self.loader_gen = tab.loader_gen;
        self.popup = None;
        self.inline_rename = None;
        self.reapply_filters_all_columns();
        self.ensure_stage_b_assets_requested();
    }

    fn sync_current_tab(&mut self) {
        let snapshot = self.snapshot_active_tab();
        if let Some(slot) = self.tabs.get_mut(self.active_tab) {
            *slot = snapshot;
        }
    }

    /// Opens a new Finder tab (cloning active root or opening `target_override`) and switches to it.
    pub(crate) fn new_tab(&mut self, target_override: Option<ListingTarget>) {
        self.sync_current_tab();
        let target = target_override.unwrap_or_else(|| {
            self.active_writable_directory().map_or_else(
                || {
                    self.columns
                        .first()
                        .map_or(ListingTarget::Directory(self.locations.home.clone()), |c| {
                            c.target.clone()
                        })
                },
                ListingTarget::Directory,
            )
        });
        let (title, sidebar) = match &target {
            ListingTarget::Directory(p) => (
                path_display_title(p),
                match_path_to_sidebar(&self.locations, &self.volumes, p),
            ),
            _ => (self.window_title.clone(), self.active_sidebar.clone()),
        };
        let placeholder = self.snapshot_active_tab();
        self.tabs.push(placeholder);
        self.active_tab = self.tabs.len() - 1;
        self.back_stack.clear();
        self.forward_stack.clear();
        self.open_root_target(target, title, sidebar, false);
        self.sync_current_tab();
        self.dirty = true;
    }

    /// Switches to open tab `idx`.
    pub(crate) fn switch_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active_tab {
            return;
        }
        self.sync_current_tab();
        self.active_tab = idx;
        if let Some(tab) = self.tabs.get(idx).cloned() {
            self.apply_tab_snapshot(tab);
        }
        self.dirty = true;
    }

    /// Closes tab `idx` (keeps at least 1 open tab).
    pub(crate) fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return;
        }
        self.sync_current_tab();
        self.tabs.remove(idx);
        if idx < self.active_tab {
            self.active_tab -= 1;
        } else if idx == self.active_tab {
            self.active_tab = self.active_tab.min(self.tabs.len() - 1);
            if let Some(tab) = self.tabs.get(self.active_tab).cloned() {
                self.apply_tab_snapshot(tab);
            }
        }
        self.dirty = true;
    }

    /// Closes the currently active tab if multiple tabs are open.
    pub(crate) fn close_active_tab(&mut self) {
        self.close_tab(self.active_tab);
    }

    /// Cycles active tab forward or backward (`Ctrl+Tab` / `Ctrl+Shift+Tab`).
    pub(crate) fn cycle_next_tab(&mut self, reverse: bool) {
        if self.tabs.len() <= 1 {
            return;
        }
        let next = if reverse {
            if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            }
        } else {
            (self.active_tab + 1) % self.tabs.len()
        };
        self.switch_tab(next);
    }

    /// Toggles persistent visibility of the top Tabs bar.
    pub(crate) fn toggle_tab_bar(&mut self) {
        self.show_tab_bar = !self.show_tab_bar;
        self.popup = None;
        self.dirty = true;
    }

    /// True if top Tabs strip should currently be laid out and drawn.
    #[must_use]
    pub(crate) fn tab_bar_visible(&self) -> bool {
        self.tabs.len() > 1 || self.show_tab_bar
    }

    /// Ordered `(tab_index, title, is_active)` list reflecting live active tab title.
    #[must_use]
    pub(crate) fn tab_titles(&self) -> Vec<(usize, String, bool)> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let active = i == self.active_tab;
                let title = if active {
                    self.window_title.clone()
                } else {
                    tab.window_title.clone()
                };
                (i, title, active)
            })
            .collect()
    }

    // --- Previews, Quick Look & Get Info ---

    /// Returns cached image/text preview for `entry` within `[max_w x max_h]` if already decoded.
    #[must_use]
    pub(crate) fn cached_preview_for(
        &self,
        entry: &FileEntry,
        max_w: usize,
        max_h: usize,
    ) -> Option<&CachedPreview> {
        let key = (entry.path.clone(), entry.modified_unix_secs, max_w, max_h);
        self.preview_cache
            .iter()
            .rev()
            .find(|item| item.key == key)
            .map(|item| &item.content)
    }

    /// Returns cached [`FileDetailedInfo`] for `path` if inspected.
    #[must_use]
    pub(crate) fn cached_info_for(&self, path: &Path) -> Option<&FileDetailedInfo> {
        self.cached_info
            .as_ref()
            .filter(|(p, _)| p == path)
            .and_then(|(_, res)| res.as_ref().ok())
    }

    fn insert_preview_cache(&mut self, key: PreviewCacheKey, content: CachedPreview) {
        if let Some(pos) = self.preview_cache.iter().position(|it| it.key == key) {
            self.preview_cache.remove(pos);
        } else if self.preview_cache.len() >= MAX_PREVIEW_CACHE_ITEMS {
            self.preview_cache.remove(0);
        }
        self.preview_cache.push(PreviewCacheEntry { key, content });
    }

    fn request_preview_if_needed(&mut self, entry: &FileEntry, max_w: usize, max_h: usize) {
        if entry.kind != FileEntryKind::RegularFile && entry.kind != FileEntryKind::Symlink {
            return;
        }
        let key = (entry.path.clone(), entry.modified_unix_secs, max_w, max_h);
        if self.preview_cache.iter().any(|it| it.key == key) || self.pending_previews.contains(&key)
        {
            return;
        }
        let is_raster = is_supported_raster_name(&entry.name);
        self.pending_previews.insert(key.clone());
        let tx = self.preview_tx.clone();
        let path = entry.path.clone();
        thread::spawn(move || {
            let content = if is_raster {
                load_image_preview_bgra(&path, max_w, max_h)
                    .map_or(CachedPreview::Unavailable, CachedPreview::Image)
            } else {
                read_text_preview(&path, 24 * 1024, 48)
                    .map_or(CachedPreview::Unavailable, CachedPreview::Text)
            };
            let _ = tx.send(PreviewJobResult { key, content });
        });
    }

    fn request_info_if_needed(&mut self, path: &Path) {
        if self.cached_info.as_ref().is_some_and(|(p, _)| p == path)
            || self.pending_info_path.as_deref() == Some(path)
        {
            return;
        }
        self.pending_info_path = Some(path.to_path_buf());
        let tx = self.info_tx.clone();
        let target = path.to_path_buf();
        let locs = self.locations.clone();
        thread::spawn(move || {
            let info = inspect_entry_info(&target, &locs).map_err(|e| e.to_string());
            let _ = tx.send(InfoJobResult { path: target, info });
        });
    }

    fn ensure_stage_b_assets_for_selection(&mut self, entry: &FileEntry) {
        if self.view_mode == FileViewMode::Gallery {
            self.request_preview_if_needed(entry, 760, 480);
            self.request_info_if_needed(&entry.path);
        }
        if self.quick_look.is_some() {
            self.request_preview_if_needed(entry, 760, 500);
            self.request_info_if_needed(&entry.path);
        }
    }

    fn ensure_stage_b_assets_requested(&mut self) {
        if self.view_mode == FileViewMode::Icons || self.view_mode == FileViewMode::Gallery {
            let entries: Vec<FileEntry> = self
                .columns
                .first()
                .map(|c| c.visible_entries.iter().take(48).cloned().collect())
                .unwrap_or_default();
            for entry in &entries {
                if is_supported_raster_name(&entry.name) {
                    self.request_preview_if_needed(entry, 96, 96);
                }
            }
        }
        if let Some(entry) = self.selected_entry().cloned() {
            self.ensure_stage_b_assets_for_selection(&entry);
        }
    }

    /// Toggles `Quick Look` overlay (`Space`) for the selected item.
    pub(crate) fn toggle_quick_look_selected(&mut self) {
        if self.quick_look.is_some() {
            self.quick_look = None;
            self.dirty = true;
            return;
        }
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = Some(String::from("Select an item for Quick Look"));
            self.dirty = true;
            return;
        };
        self.request_preview_if_needed(&entry, 760, 500);
        self.request_info_if_needed(&entry.path);
        self.quick_look = Some(QuickLookState { entry });
        self.popup = None;
        self.dirty = true;
    }

    /// Cycles active `Quick Look` preview backward (`-1`) or forward (`+1`).
    pub(crate) fn step_quick_look(&mut self, delta: isize) {
        let Some(col) = self.columns.get(self.active_column) else {
            return;
        };
        if col.visible_entries.is_empty() {
            return;
        }
        let len = col.visible_entries.len();
        let next = match col.selected_index {
            None => 0,
            Some(cur) => cur.saturating_add_signed(delta).min(len - 1),
        };
        self.select_entry(self.active_column, next, false);
    }

    /// Opens `Get Info` sheet for currently selected item (`Ctrl+I`) or active folder.
    pub(crate) fn open_get_info_selected(&mut self) {
        let target = self
            .selected_entry()
            .map(|e| e.path.clone())
            .or_else(|| self.active_writable_directory());
        if let Some(path) = target {
            self.open_get_info_for_path(&path);
        }
    }

    /// Opens `Get Info` sheet for `path` and schedules non-blocking metadata/preview loading.
    pub(crate) fn open_get_info_for_path(&mut self, path: &Path) {
        self.popup = None;
        if let Some(entry) = self.selected_entry().cloned()
            && entry.path == path
        {
            self.request_preview_if_needed(&entry, 96, 96);
        }
        let cached = self.cached_info_for(path).cloned();
        self.get_info = Some(GetInfoState {
            loading: cached.is_none(),
            target_path: path.to_path_buf(),
            info: cached,
            error: None,
            more_expanded: true,
        });
        self.pending_info_path = None;
        self.request_info_if_needed(path);
        self.dirty = true;
    }

    /// Opens `Go to Folder...` sheet (`Ctrl+Shift+G`).
    pub(crate) fn open_go_to_folder(&mut self) {
        self.popup = None;
        self.inline_rename = None;
        let current_dir = self.active_writable_directory();
        let seed = current_dir.as_deref().map_or_else(
            || String::from("~/"),
            |p| format_go_to_display(p, &self.locations),
        );
        let suggestions = suggest_go_to_paths(&seed, current_dir.as_deref(), &self.locations, 6);
        self.go_to_folder = Some(GoToFolderState {
            input: seed,
            suggestions,
            selected_suggestion: 0,
            error: None,
        });
        self.reset_caret_blink();
        self.dirty = true;
    }

    /// Confirms `Go to Folder...` input and navigates to folder (or file's parent).
    pub(crate) fn submit_go_to_folder(&mut self) {
        let Some(gt) = &self.go_to_folder else {
            return;
        };
        let raw_input = gt.input.clone();
        let current_dir = self.active_writable_directory();
        let Some(resolved) =
            resolve_go_to_path(&raw_input, current_dir.as_deref(), &self.locations)
        else {
            if let Some(gt_mut) = &mut self.go_to_folder {
                gt_mut.error = Some(String::from("The folder can't be found."));
            }
            self.dirty = true;
            return;
        };
        self.go_to_folder = None;
        if resolved.is_dir() {
            let title = path_display_title(&resolved);
            let sidebar = match_path_to_sidebar(&self.locations, &self.volumes, &resolved);
            self.open_root_target(ListingTarget::Directory(resolved), title, sidebar, true);
        } else if let Some(parent) = resolved.parent() {
            let parent_buf = parent.to_path_buf();
            let title = path_display_title(&parent_buf);
            let sidebar = match_path_to_sidebar(&self.locations, &self.volumes, &parent_buf);
            self.open_root_target(ListingTarget::Directory(parent_buf), title, sidebar, true);
            self.status_message = Some(format!("Navigated near {}", resolved.display()));
        }
        self.dirty = true;
    }

    /// Picks a specific `Go to Folder...` autocomplete row and opens it immediately.
    pub(crate) fn pick_go_to_suggestion(&mut self, idx: usize) {
        let Some(path) = self
            .go_to_folder
            .as_ref()
            .and_then(|gt| gt.suggestions.get(idx))
            .cloned()
        else {
            return;
        };
        if let Some(gt) = &mut self.go_to_folder {
            gt.input = path.display().to_string();
        }
        self.submit_go_to_folder();
    }

    // --- Inline Rename ---

    /// Begins inline rename (`F2`) for currently selected entry.
    pub(crate) fn start_inline_rename_selected(&mut self) {
        let Some(col) = self.columns.get(self.active_column) else {
            return;
        };
        let Some(row_idx) = col.selected_index else {
            return;
        };
        let Some(entry) = col.visible_entries.get(row_idx).cloned() else {
            return;
        };
        self.start_inline_rename_entry(self.active_column, row_idx, &entry);
    }

    /// Begins inline rename for a specific entry.
    pub(crate) fn start_inline_rename_entry(
        &mut self,
        col_idx: usize,
        row_idx: usize,
        entry: &FileEntry,
    ) {
        if self.is_trash_view()
            || matches!(
                entry.kind,
                FileEntryKind::MountPoint | FileEntryKind::Application
            )
        {
            self.status_message = Some(String::from("Cannot rename this item here"));
            self.dirty = true;
            return;
        }
        self.popup = None;
        self.inline_rename = Some(InlineRenameState {
            target_path: entry.path.clone(),
            col_idx,
            row_idx,
            draft: entry.name.clone(),
            error: None,
        });
        self.reset_caret_blink();
        self.dirty = true;
    }

    /// Commits active inline rename via [`rename_entry`], migrating tags & reloading view.
    pub(crate) fn commit_inline_rename(&mut self) {
        let Some(rn) = self.inline_rename.clone() else {
            return;
        };
        match rename_entry(&rn.target_path, &rn.draft) {
            Ok(new_path) => {
                let existing_tags: Vec<FileTagColor> =
                    self.tags_config.tags_for(&rn.target_path).to_vec();
                if !existing_tags.is_empty() {
                    self.tags_config.remove_tags_for(&rn.target_path);
                    for tag in existing_tags {
                        self.tags_config.toggle_tag(&new_path, tag);
                    }
                    self.persist_preferences();
                }
                if let Some(parent) = new_path.parent() {
                    self.reload_directory(parent);
                }
                self.reload_active_column();
                self.inline_rename = None;
                let new_name = new_path
                    .file_name()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                self.status_message = Some(format!("Renamed to {new_name}"));
            }
            Err(err) => {
                if let Some(edit) = &mut self.inline_rename {
                    edit.error = Some(err.to_string());
                }
                self.status_message = Some(format!("Rename failed: {err}"));
            }
        }
        self.dirty = true;
    }

    // --- Clipboard & Background Copy/Move ---

    /// Copies selected path onto in-app file clipboard (`Ctrl+C`).
    pub(crate) fn action_copy_selected(&mut self) {
        self.stage_clipboard(TransferKind::Copy);
    }

    /// Stages selected path for Move (`Ctrl+X`).
    pub(crate) fn action_cut_selected(&mut self) {
        self.stage_clipboard(TransferKind::Move);
    }

    fn stage_clipboard(&mut self, kind: TransferKind) {
        self.popup = None;
        let Some(entry) = self.selected_entry().cloned() else {
            self.status_message = Some(String::from("Select an item first"));
            self.dirty = true;
            return;
        };
        if self.is_trash_view() {
            return;
        }
        self.clipboard = Some(ClipboardItemSet {
            kind,
            paths: vec![entry.path],
        });
        let label = match kind {
            TransferKind::Copy => "Copied",
            TransferKind::Move => "Cut",
        };
        self.status_message = Some(format!("{label} \"{}\"", entry.name));
        self.dirty = true;
    }

    /// Pastes clipboard items into active directory (`Ctrl+V`) in background thread.
    pub(crate) fn action_paste_into_active(&mut self) {
        self.popup = None;
        if self.active_transfer.is_some() {
            self.status_message = Some(String::from("Transfer already in progress"));
            self.dirty = true;
            return;
        }
        let Some(clip) = self.clipboard.clone() else {
            self.status_message = Some(String::from("Clipboard is empty"));
            self.dirty = true;
            return;
        };
        let Some(dest_dir) = self.active_writable_directory() else {
            self.status_message = Some(String::from("Cannot paste into virtual location"));
            self.dirty = true;
            return;
        };
        self.start_background_transfer(clip.paths, dest_dir, clip.kind);
    }

    /// Duplicates selected item in its parent folder (`Ctrl+D`).
    pub(crate) fn action_duplicate_selected(&mut self) {
        self.popup = None;
        if self.active_transfer.is_some() {
            return;
        }
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if self.is_trash_view() {
            return;
        }
        let Some(dest_dir) = entry.path.parent().map(Path::to_path_buf) else {
            return;
        };
        self.start_background_transfer(vec![entry.path], dest_dir, TransferKind::Copy);
    }

    fn start_background_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        dest_dir: PathBuf,
        kind: TransferKind,
    ) {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let initial_name = sources
            .first()
            .and_then(|p| p.file_name())
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let initial_progress = TransferProgress {
            kind,
            current_item_name: initial_name,
            destination_dir: dest_dir.clone(),
            total_bytes: 0,
            completed_bytes: 0,
            total_items: sources.len(),
            completed_items: 0,
        };
        self.active_transfer = Some(ActiveTransferState {
            progress: initial_progress,
            cancel_flag: Arc::clone(&cancel_flag),
            conflict: None,
        });
        let tx = self.transfer_tx.clone();
        thread::spawn(move || {
            let res = copy_or_move_with_progress(
                &sources,
                &dest_dir,
                kind,
                &cancel_flag,
                |progress| {
                    let _ = tx.send(TransferWorkerMsg::Progress(progress.clone()));
                },
                |src, dst| {
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    if tx
                        .send(TransferWorkerMsg::Conflict {
                            source: src.to_path_buf(),
                            dest: dst.to_path_buf(),
                            reply_tx,
                        })
                        .is_err()
                    {
                        return TransferConflictDecision::Stop;
                    }
                    reply_rx.recv().unwrap_or(TransferConflictDecision::Stop)
                },
            )
            .map_err(|e| e.to_string());
            let _ = tx.send(TransferWorkerMsg::Finished {
                kind,
                dest_dir,
                result: res,
            });
        });
        self.dirty = true;
    }

    /// Answers an active transfer collision prompt (`Replace`, `KeepBoth`, `Skip`, `Stop`).
    pub(crate) fn resolve_transfer_conflict(&mut self, decision: TransferConflictDecision) {
        if let Some(tr) = &mut self.active_transfer
            && let Some(mut conflict) = tr.conflict.take()
        {
            if decision == TransferConflictDecision::Stop {
                tr.cancel_flag.store(true, Ordering::Relaxed);
            }
            if let Some(reply) = conflict.reply_tx.take() {
                let _ = reply.send(decision);
            }
            self.dirty = true;
        }
    }

    /// Cancels an active background Copy/Move transfer.
    pub(crate) fn cancel_active_transfer(&mut self) {
        if let Some(tr) = &mut self.active_transfer {
            tr.cancel_flag.store(true, Ordering::Relaxed);
            if let Some(mut conflict) = tr.conflict.take()
                && let Some(reply) = conflict.reply_tx.take()
            {
                let _ = reply.send(TransferConflictDecision::Stop);
            }
            self.dirty = true;
        }
    }

    // --- Clickable Path Bar ---

    /// Decomposes the focused directory target into clickable `(label, path)` breadcrumb segments.
    #[must_use]
    pub(crate) fn path_breadcrumbs(&self) -> Vec<(String, PathBuf)> {
        let dir = self
            .columns
            .get(self.active_column)
            .and_then(|c| match &c.target {
                ListingTarget::Directory(p) => Some(p.as_path()),
                _ => None,
            })
            .or_else(|| match self.columns.first().map(|c| &c.target) {
                Some(ListingTarget::Directory(p)) => Some(p.as_path()),
                _ => None,
            });
        let Some(target_dir) = dir else {
            return Vec::new();
        };

        let mut crumbs = Vec::new();
        if let Ok(rel) = target_dir.strip_prefix(&self.locations.home) {
            crumbs.push((
                self.locations.user_label.clone(),
                self.locations.home.clone(),
            ));
            let mut acc = self.locations.home.clone();
            for comp in rel.components() {
                acc.push(comp.as_os_str());
                crumbs.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
            }
            return crumbs;
        }
        let mut acc = PathBuf::new();
        for comp in target_dir.components() {
            acc.push(comp.as_os_str());
            let label = if acc == Path::new("/") {
                String::from("RustyOS HD")
            } else {
                comp.as_os_str().to_string_lossy().into_owned()
            };
            crumbs.push((label, acc.clone()));
        }
        crumbs
    }

    /// Navigates cleanly when clicking a bottom `Path Bar` breadcrumb directory.
    pub(crate) fn navigate_to_directory(&mut self, path: PathBuf) {
        if path.is_dir() {
            let title = path_display_title(&path);
            let sidebar = match_path_to_sidebar(&self.locations, &self.volumes, &path);
            self.open_root_target(ListingTarget::Directory(path), title, sidebar, true);
        }
    }

    /// Formats right-edge status summary string for the bottom bar.
    #[must_use]
    pub(crate) fn status_summary(&self) -> String {
        if let Some(msg) = &self.status_message {
            return msg.clone();
        }
        if let Some(col) = self.columns.get(self.active_column) {
            if col.loading {
                return format!("Loading {}...", col.title);
            }
            if let Some(err) = &col.error {
                return err.clone();
            }
            let n = col.visible_entries.len();
            return if n == 1 {
                String::from("1 item")
            } else {
                format!("{n} items")
            };
        }
        String::new()
    }
}

fn key_matches_ascii(key_name: &str, utf8: Option<&str>, ch: char) -> bool {
    let mut tmp = [0_u8; 4];
    let s = ch.encode_utf8(&mut tmp);
    key_name.eq_ignore_ascii_case(s) || utf8.is_some_and(|u| u.eq_ignore_ascii_case(s))
}

fn is_supported_raster_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("png") || ext.eq_ignore_ascii_case("ppm"))
}

fn format_go_to_display(path: &Path, locations: &UserFileLocations) -> String {
    if let Ok(rel) = path.strip_prefix(&locations.home) {
        if rel.as_os_str().is_empty() {
            return String::from("~/");
        }
        return format!("~/{}", rel.display());
    }
    path.display().to_string()
}

/// Target scrollable region under pointer for wheel events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollTargetArea {
    Sidebar,
    List,
    Icons,
    GalleryStrip,
    Column(usize),
}

/// Computes total pixel height consumed by sidebar items + section headers.
#[must_use]
pub(crate) fn sidebar_total_height(items: &[SidebarRowItem]) -> i32 {
    let mut h = 0_i32;
    for item in items {
        if item.section.is_some() {
            h += SIDEBAR_SECTION_HEIGHT;
        }
        h += SIDEBAR_ROW_HEIGHT;
    }
    h
}

fn build_sidebar_items(
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
) -> Vec<SidebarRowItem> {
    let mut items = vec![
        SidebarRowItem {
            section: None,
            label: String::from("Recents"),
            location: SidebarLocation::Recents,
            icon: SidebarIconKind::Recents,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Shared"),
            location: SidebarLocation::Shared,
            icon: SidebarIconKind::Shared,
        },
        SidebarRowItem {
            section: Some("Favorites"),
            label: String::from("Applications"),
            location: SidebarLocation::Applications,
            icon: SidebarIconKind::Applications,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Desktop"),
            location: SidebarLocation::Desktop,
            icon: SidebarIconKind::Desktop,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Documents"),
            location: SidebarLocation::Documents,
            icon: SidebarIconKind::Documents,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Downloads"),
            location: SidebarLocation::Downloads,
            icon: SidebarIconKind::Downloads,
        },
        SidebarRowItem {
            section: Some("Locations"),
            label: locations.user_label.clone(),
            location: SidebarLocation::Home,
            icon: SidebarIconKind::Home,
        },
    ];

    for vol in volumes {
        if !vol.is_network {
            items.push(SidebarRowItem {
                section: None,
                label: vol.name.clone(),
                location: SidebarLocation::Volume(vol.mount_point.clone()),
                icon: SidebarIconKind::Volume,
            });
        }
    }

    items.push(SidebarRowItem {
        section: None,
        label: String::from("Network"),
        location: SidebarLocation::Network,
        icon: SidebarIconKind::Network,
    });
    items.push(SidebarRowItem {
        section: None,
        label: String::from("Trash"),
        location: SidebarLocation::Trash,
        icon: SidebarIconKind::Trash,
    });

    for (idx, tag) in FileTagColor::ALL.iter().enumerate() {
        items.push(SidebarRowItem {
            section: (idx == 0).then_some("Tags"),
            label: String::from(tag.label()),
            location: SidebarLocation::Tag(*tag),
            icon: SidebarIconKind::Tag(*tag),
        });
    }
    items
}

fn match_path_to_sidebar(
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
    path: &Path,
) -> Option<SidebarLocation> {
    if path == locations.home {
        return Some(SidebarLocation::Home);
    }
    if path == locations.desktop {
        return Some(SidebarLocation::Desktop);
    }
    if path == locations.documents {
        return Some(SidebarLocation::Documents);
    }
    if path == locations.downloads {
        return Some(SidebarLocation::Downloads);
    }
    if path == locations.trash_dir || path == locations.trash_dir.join("files") {
        return Some(SidebarLocation::Trash);
    }
    for vol in volumes {
        if path == vol.mount_point {
            return Some(SidebarLocation::Volume(vol.mount_point.clone()));
        }
    }
    None
}

fn sidebar_label_for(
    loc: &SidebarLocation,
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
) -> String {
    match loc {
        SidebarLocation::Recents => String::from("Recents"),
        SidebarLocation::Shared => String::from("Shared"),
        SidebarLocation::Applications => String::from("Applications"),
        SidebarLocation::Desktop => String::from("Desktop"),
        SidebarLocation::Documents => String::from("Documents"),
        SidebarLocation::Downloads => String::from("Downloads"),
        SidebarLocation::Home => locations.user_label.clone(),
        SidebarLocation::Volume(path) => volumes
            .iter()
            .find(|v| &v.mount_point == path)
            .map_or_else(|| path_display_title(path), |v| v.name.clone()),
        SidebarLocation::Network => String::from("Network"),
        SidebarLocation::Trash => String::from("Trash"),
        SidebarLocation::Tag(tag) => format!("{} Tag", tag.label()),
    }
}

fn path_display_title(path: &Path) -> String {
    if path == Path::new("/") {
        return String::from("RustyOS HD");
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .map_or_else(|| path.to_string_lossy().into_owned(), ToOwned::to_owned)
}

fn parse_desktop_exec_tokens(exec_line: &str) -> Option<Vec<String>> {
    let tokens: Vec<String> = exec_line
        .split_whitespace()
        .filter(|tok| !matches!(*tok, "%u" | "%U" | "%f" | "%F" | "%i" | "%c" | "%k"))
        .map(ToOwned::to_owned)
        .collect();
    (!tokens.is_empty()).then_some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestHomeGuard {
        root: PathBuf,
    }

    impl TestHomeGuard {
        fn new(label: &str) -> std::io::Result<Self> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let root = std::env::temp_dir().join(format!(
                "rustyos-files-state-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("home/Documents/SubDir"))?;
            fs::create_dir_all(root.join("home/Downloads"))?;
            fs::create_dir_all(root.join("home/Desktop"))?;
            fs::write(root.join("home/Documents/notes.txt"), b"hello")?;
            fs::write(root.join("home/Documents/SubDir/deep.rs"), b"fn main() {}")?;
            fs::write(root.join("home/Documents/.hidden"), b"secret")?;
            Ok(Self { root })
        }

        fn locations(&self) -> UserFileLocations {
            UserFileLocations::with_roots(
                &self.root.join("home"),
                &self.root.join("config"),
                &self.root.join("data"),
            )
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn miller_columns_history_and_stage_b_guard_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("miller")?;
        let mut state = FilesAppState::new(guard.locations(), &StartupOptions::default());
        state.complete_pending_loads_sync();
        assert_eq!(state.theme, FilesThemeMode::LightGoldenGate);
        assert_eq!(state.view_mode, FileViewMode::Columns);

        state.navigate_sidebar(SidebarLocation::Documents, true);
        state.complete_pending_loads_sync();
        assert!(state.can_go_back());
        assert_eq!(state.columns.len(), 1);
        assert_eq!(state.columns[0].visible_entries.len(), 2);

        // Select SubDir (sorted first since directories lead) -> opens column 1
        state.select_entry(0, 0, false);
        state.complete_pending_loads_sync();
        assert_eq!(state.columns.len(), 2);
        assert_eq!(state.columns[1].visible_entries[0].name, "deep.rs");

        // History Back & Forward
        state.go_back();
        state.complete_pending_loads_sync();
        assert_eq!(state.window_title, "home");
        assert!(state.can_go_forward());

        state.go_forward();
        state.complete_pending_loads_sync();
        assert_eq!(state.window_title, "Documents");

        // Stage B views (`Icons`, `Gallery`) switch & persist cleanly
        state.set_view_mode(FileViewMode::Icons);
        assert_eq!(state.view_mode, FileViewMode::Icons);
        state.set_view_mode(FileViewMode::Gallery);
        assert_eq!(state.view_mode, FileViewMode::Gallery);
        state.set_view_mode(FileViewMode::Columns);
        assert_eq!(state.view_mode, FileViewMode::Columns);
        Ok(())
    }

    #[test]
    fn theme_toggle_tags_hidden_and_trash_actions_persist() -> Result<(), Box<dyn std::error::Error>>
    {
        let guard = TestHomeGuard::new("persist")?;
        let locs = guard.locations();
        let mut state = FilesAppState::new(
            locs.clone(),
            &StartupOptions {
                initial_path: Some(locs.documents.clone()),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();

        state.toggle_theme();
        assert_eq!(state.theme, FilesThemeMode::DarkPurpleGlass);
        state.toggle_show_hidden();
        assert!(state.show_hidden);
        state.set_view_mode(FileViewMode::List);
        assert_eq!(state.view_mode, FileViewMode::List);

        let notes_path = locs.documents.join("notes.txt");
        state.toggle_tag_on_path(&notes_path, FileTagColor::Green);
        assert_eq!(state.tags_for(&notes_path), &[FileTagColor::Green]);

        // Reload fresh app state -> persisted preferences & tags are restored
        let reloaded = FilesAppState::new(locs.clone(), &StartupOptions::default());
        assert_eq!(reloaded.theme, FilesThemeMode::DarkPurpleGlass);
        assert_eq!(reloaded.view_mode, FileViewMode::List);
        assert!(reloaded.show_hidden);
        assert_eq!(reloaded.tags_for(&notes_path), &[FileTagColor::Green]);

        // Test New Folder & Move to Trash + Empty Trash
        state.action_new_folder();
        state.complete_pending_loads_sync();
        assert!(locs.documents.join("untitled folder").is_dir());

        let idx = state.columns[0]
            .visible_entries
            .iter()
            .position(|e| e.name == "notes.txt")
            .ok_or("notes.txt missing")?;
        state.select_entry(0, idx, false);
        state.action_move_selected_to_trash();
        state.complete_pending_loads_sync();
        assert!(!notes_path.exists());
        assert_eq!(state.tags_for(&notes_path), []);

        state.navigate_sidebar(SidebarLocation::Trash, false);
        state.complete_pending_loads_sync();
        assert_eq!(state.columns[0].visible_entries.len(), 1);
        state.action_empty_trash();
        state.complete_pending_loads_sync();
        assert_eq!(state.columns[0].visible_entries, []);
        Ok(())
    }

    #[test]
    fn stage_b_tabs_rename_goto_quicklook_getinfo_and_clipboard_work()
    -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("stage-b-flows")?;
        let locs = guard.locations();
        let mut state = FilesAppState::new(locs.clone(), &StartupOptions::default());
        state.complete_pending_loads_sync();

        // 1. Multi-tab isolation & cycling
        assert!(!state.tab_bar_visible());
        state.new_tab(Some(ListingTarget::Directory(locs.documents.clone())));
        state.complete_pending_loads_sync();
        assert!(state.tab_bar_visible());
        assert_eq!(state.tab_titles().len(), 2);
        assert_eq!(state.window_title, "Documents");

        state.handle_key_input("Tab", None, true, false);
        assert_eq!(state.active_tab, 0);
        assert_eq!(state.window_title, "home");

        state.handle_key_input("Tab", None, true, true);
        assert_eq!(state.active_tab, 1);
        assert_eq!(state.window_title, "Documents");

        // 2. Path Bar breadcrumbs inside Documents/SubDir
        state.set_view_mode(FileViewMode::Icons);
        state.last_icon_grid_cols = 4;
        state.select_entry(0, 0, true); // opens SubDir
        state.complete_pending_loads_sync();
        let crumbs = state.path_breadcrumbs();
        let crumb_labels: Vec<&str> = crumbs.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(crumb_labels, vec!["home", "Documents", "SubDir"]);

        // 3. Inline Rename with tag preservation
        let deep_rs = locs.documents.join("SubDir/deep.rs");
        state.toggle_tag_on_path(&deep_rs, FileTagColor::Blue);
        state.select_entry(0, 0, false);
        state.handle_key_input("F2", None, false, false);
        assert!(state.inline_rename.is_some());
        state.handle_key_input("BackSpace", None, false, false); // remove 's' -> deep.r
        state.handle_key_input("", Some("s"), false, false);
        if let Some(rn) = &mut state.inline_rename {
            rn.draft = String::from("renamed.rs");
        }
        state.handle_key_input("Return", None, false, false);
        state.complete_pending_loads_sync();
        let renamed = locs.documents.join("SubDir/renamed.rs");
        assert!(renamed.exists());
        assert_eq!(state.tags_for(&renamed), &[FileTagColor::Blue]);

        // 4. Quick Look toggle (`Space`) & close (`Escape`)
        let target_entry = state.columns[0]
            .visible_entries
            .iter()
            .position(|e| e.name == "renamed.rs")
            .ok_or("renamed.rs missing")?;
        state.select_entry(0, target_entry, false);
        state.handle_key_input("Space", Some(" "), false, false);
        assert!(state.quick_look.is_some());
        state.handle_key_input("Escape", None, false, false);
        assert!(state.quick_look.is_none());

        // 5. Get Info & Go to Folder
        state.handle_key_input("i", Some("i"), true, false);
        assert!(state.get_info.is_some());
        state.handle_key_input("Escape", None, false, false);
        assert!(state.get_info.is_none());

        state.open_go_to_folder();
        if let Some(gt) = &mut state.go_to_folder {
            gt.input = String::from("~/Downloads");
        }
        state.submit_go_to_folder();
        state.complete_pending_loads_sync();
        assert!(state.go_to_folder.is_none());
        assert_eq!(state.window_title, "Downloads");

        // 6. Close second tab -> collapses back to 1 tab
        state.close_active_tab();
        assert_eq!(state.tab_titles().len(), 1);
        assert!(!state.tab_bar_visible());
        Ok(())
    }

    #[test]
    fn smooth_scroll_axes_and_caret_blink_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("smooth-scroll-caret")?;
        let locs = guard.locations();
        for idx in 0..30 {
            let _ = std::fs::File::create(locs.documents.join(format!("item_{idx:02}.txt")));
        }
        let mut state = FilesAppState::new(locs.clone(), &StartupOptions::default());
        state.complete_pending_loads_sync();

        let col = state
            .columns
            .first()
            .and_then(|c| c.visible_entries.iter().position(|e| e.name == "Documents"))
            .ok_or("Documents missing")?;
        state.select_entry(0, col, false);
        state.complete_pending_loads_sync();

        state.scroll_axis(ScrollTargetArea::Column(1), 88.0, true, 160);
        assert!(state.has_active_animations());
        let t0 = state.last_anim_tick.unwrap_or_else(Instant::now);
        let _ = state.tick_animations(t0 + Duration::from_millis(16));
        assert!(state.columns[1].scroll_y > 0);
        assert!(state.columns[1].scroll_y < 88);
        for step in 2..=30 {
            let _ = state.tick_animations(t0 + Duration::from_millis(step * 16));
        }
        assert_eq!(state.columns[1].scroll_y, 88);
        assert!(!state.has_active_animations());

        state.search_active = true;
        state.set_search_query(String::from("item"));
        assert!(state.caret_visible);
        let flip_at = state.last_caret_tick + Duration::from_millis(550);
        assert!(state.update_caret_blink(flip_at));
        assert!(!state.caret_visible);
        state.reset_caret_blink();
        assert!(state.caret_visible);
        Ok(())
    }
}
