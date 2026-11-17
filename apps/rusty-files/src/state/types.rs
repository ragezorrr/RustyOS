//! Common data types, enums, and component states for `rusty-files` state machine.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use rusty_services::file_manager::{
    DecodedPreviewImage, FileDetailedInfo, FileEntry, FileSortField, FileSortOrder, FileTagColor,
    FilesThemeMode, ListingTarget, SidebarLocation, TextPreview, TransferConflictDecision,
    TransferKind, TransferOutcome, TransferProgress, filter_and_sort_entries,
};

/// Fixed column width in Miller Columns mode.
pub(crate) const MILLER_COLUMN_WIDTH: i32 = 216;
/// Row height for Miller column rows.
pub(crate) const ROW_HEIGHT: i32 = 26;
/// Row height for Golden Gate sidebar rows.
pub(crate) const SIDEBAR_ROW_HEIGHT: i32 = 22;
/// Section header + top inter-section gap height in the Golden Gate sidebar.
pub(crate) const SIDEBAR_SECTION_HEIGHT: i32 = 24;
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
pub(crate) const MAX_PREVIEW_CACHE_ITEMS: usize = 96;

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
    pub(crate) fn new_loading(target: ListingTarget, title: String) -> Self {
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

/// Search scope selector shown in the Search Scope bar (`RUSTYOS-071`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SearchScope {
    /// Filter entries within the active folder / column.
    #[default]
    ThisFolder,
    /// Recursive filename search across user Home directory.
    Home,
}

/// Active floating popover menu inside `rusty-files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenPopupMenu {
    /// Sort criterion & order menu anchored below Sort pill.
    SortMenu,
    /// Share menu anchored below left Share action button (`RUSTYOS-071`).
    ShareMenu,
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

/// Completed recursive Home search payload delivered over `mpsc` (`RUSTYOS-071`).
#[derive(Debug)]
pub(crate) struct HomeSearchResult {
    pub(crate) generation: u64,
    pub(crate) query: String,
    pub(crate) entries: Vec<FileEntry>,
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

pub(crate) type PreviewCacheKey = (PathBuf, Option<i64>, usize, usize);

#[derive(Debug, Clone)]
pub(crate) struct PreviewCacheEntry {
    pub(crate) key: PreviewCacheKey,
    pub(crate) content: CachedPreview,
}

#[derive(Debug)]
pub(crate) struct PreviewJobResult {
    pub(crate) key: PreviewCacheKey,
    pub(crate) content: CachedPreview,
}

#[derive(Debug)]
pub(crate) struct InfoJobResult {
    pub(crate) path: PathBuf,
    pub(crate) info: Result<FileDetailedInfo, String>,
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
    pub(crate) reply_tx: Option<mpsc::SyncSender<TransferConflictDecision>>,
}

/// Active background Copy/Move UI state + cancellation handle.
#[derive(Debug)]
pub(crate) struct ActiveTransferState {
    pub progress: TransferProgress,
    pub cancel_flag: Arc<AtomicBool>,
    pub conflict: Option<ConflictPrompt>,
}

#[derive(Debug)]
pub(crate) enum TransferWorkerMsg {
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
    pub open_test_popup: bool,
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
