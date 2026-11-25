//! Common data types, enums, and component states for `rusty-files` state machine.

use std::collections::BTreeSet;
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
/// Row height for RustyOS sidebar rows.
pub(crate) const SIDEBAR_ROW_HEIGHT: i32 = 22;
/// Section header + top inter-section gap height in the RustyOS sidebar.
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

/// Icon classification for RustyOS sidebar entries.
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
    RemovableDrive,
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
    /// Block device path (e.g. `"/dev/sdb1"`) for removable volumes that show an `Eject` button.
    pub removable_device: Option<String>,
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
    /// Optional tree nesting depth per visible row (`List` view folder expansion).
    pub visible_depths: Vec<u16>,
    /// Group header boundaries `(first_row_idx, section_title)` when `GroupByMode != None`.
    pub group_sections: Vec<(usize, String)>,
    /// Currently selected primary/anchor row index in `visible_entries`.
    pub selected_index: Option<usize>,
    /// All selected row indices in `visible_entries` (`RUSTYOS-150` Multi-Select).
    pub selected_indices: BTreeSet<usize>,
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
            visible_depths: Vec::new(),
            group_sections: Vec::new(),
            selected_index: None,
            selected_indices: BTreeSet::new(),
            scroll_y: 0,
            scroll_anim: SmoothAxis::default(),
            loading: true,
            error: None,
        }
    }

    /// Returns currently selected primary entry if valid.
    #[must_use]
    pub(crate) fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_index
            .and_then(|idx| self.visible_entries.get(idx))
    }

    /// Returns true when `idx` is part of the current selection.
    #[must_use]
    pub(crate) fn is_selected(&self, idx: usize) -> bool {
        self.selected_index == Some(idx) || self.selected_indices.contains(&idx)
    }

    /// Returns all currently selected entries in visible order.
    #[must_use]
    pub(crate) fn selected_entries(&self) -> Vec<&FileEntry> {
        if self.selected_indices.is_empty() {
            return self.selected_entry().into_iter().collect();
        }
        self.selected_indices
            .iter()
            .filter_map(|&idx| self.visible_entries.get(idx))
            .collect()
    }

    /// Returns total count of selected items in this column.
    #[must_use]
    pub(crate) fn selected_count(&self) -> usize {
        if self.selected_indices.is_empty() {
            usize::from(self.selected_index.is_some())
        } else {
            self.selected_indices.len()
        }
    }

    /// Replaces selection with a single item `idx`.
    pub(crate) fn set_single_selection(&mut self, idx: usize) {
        self.selected_indices.clear();
        if idx < self.visible_entries.len() {
            self.selected_index = Some(idx);
            self.selected_indices.insert(idx);
        } else {
            self.selected_index = None;
        }
    }

    /// Toggles `idx` in the multi-selection (`Ctrl+Click`).
    pub(crate) fn toggle_selection(&mut self, idx: usize) {
        if idx >= self.visible_entries.len() {
            return;
        }
        if self.selected_indices.is_empty()
            && let Some(prev) = self.selected_index
        {
            self.selected_indices.insert(prev);
        }
        if !self.selected_indices.remove(&idx) {
            self.selected_indices.insert(idx);
            self.selected_index = Some(idx);
        } else if self.selected_index == Some(idx) {
            self.selected_index = self.selected_indices.iter().next_back().copied();
        }
    }

    /// Selects a contiguous range from the anchor `selected_index` to `idx` (`Shift+Click`).
    pub(crate) fn select_range_to(&mut self, idx: usize) {
        if idx >= self.visible_entries.len() {
            return;
        }
        let anchor = self.selected_index.unwrap_or(idx);
        let (lo, hi) = if anchor <= idx {
            (anchor, idx)
        } else {
            (idx, anchor)
        };
        self.selected_indices.clear();
        for i in lo..=hi {
            self.selected_indices.insert(i);
        }
        self.selected_index = Some(idx);
    }

    /// Selects all visible entries (`Ctrl+A`).
    pub(crate) fn select_all(&mut self) {
        self.selected_indices.clear();
        for i in 0..self.visible_entries.len() {
            self.selected_indices.insert(i);
        }
        self.selected_index = (!self.visible_entries.is_empty()).then_some(0);
    }

    /// Clears all selection in this column.
    pub(crate) fn clear_selection(&mut self) {
        self.selected_index = None;
        self.selected_indices.clear();
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
        let selected_paths: BTreeSet<PathBuf> = self
            .selected_entries()
            .into_iter()
            .map(|e| e.path.clone())
            .collect();
        self.visible_entries = filter_and_sort_entries(
            self.raw_entries.clone(),
            show_hidden,
            query,
            sort_field,
            sort_order,
        );
        self.visible_depths = vec![0; self.visible_entries.len()];
        self.group_sections.clear();
        self.reconcile_selection_paths(selected_path.as_ref(), &selected_paths);
    }

    pub(crate) fn reconcile_selection_paths(
        &mut self,
        selected_path: Option<&PathBuf>,
        selected_paths: &BTreeSet<PathBuf>,
    ) {
        self.selected_index =
            selected_path.and_then(|p| self.visible_entries.iter().position(|e| &e.path == p));
        self.selected_indices.clear();
        for (i, e) in self.visible_entries.iter().enumerate() {
            if selected_paths.contains(&e.path) {
                self.selected_indices.insert(i);
            }
        }
        if let Some(primary) = self.selected_index {
            self.selected_indices.insert(primary);
        } else {
            self.selected_index = self.selected_indices.iter().next().copied();
        }
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
    pub available_bytes: Option<u64>,
}

/// Completed background archive extract/compress operation delivered over `mpsc`.
#[derive(Debug)]
pub(crate) struct ArchiveJobResult {
    pub target_dir: PathBuf,
    pub outcome: Result<String, String>,
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

/// Active internal file drag-and-drop state (`RUSTYOS-134`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FileDragState {
    /// Paths being dragged.
    pub sources: Vec<PathBuf>,
    /// Pointer press origin `(x, y)`.
    pub start_pos: (f64, f64),
    /// Current pointer `(x, y)`.
    pub current_pos: (f64, f64),
    /// Becomes `true` once pointer moves at least `6 px` from `start_pos`.
    pub active: bool,
    /// Optional hovered destination directory.
    pub target_dir: Option<PathBuf>,
    /// True when hovered over `SidebarLocation::Trash`.
    pub target_is_trash: bool,
    /// When `true`, copy instead of move.
    pub copy_mode: bool,
}

impl FileDragState {
    /// Creates a pending drag state from `sources` at `start_pos`.
    #[must_use]
    pub(crate) fn start(sources: Vec<PathBuf>, start_pos: (f64, f64)) -> Self {
        Self {
            sources,
            start_pos,
            current_pos: start_pos,
            active: false,
            target_dir: None,
            target_is_trash: false,
            copy_mode: false,
        }
    }

    /// Updates pointer coordinates and activates drag once threshold (`6 px`) is crossed.
    pub(crate) fn update_pos(&mut self, pos: (f64, f64)) -> bool {
        self.current_pos = pos;
        if !self.active {
            let dx = pos.0 - self.start_pos.0;
            let dy = pos.1 - self.start_pos.1;
            if dx * dx + dy * dy >= 36.0 {
                self.active = true;
            }
        }
        self.active
    }

    /// Returns `true` if dropping `self.sources` onto `dest` is valid (not onto itself or a child).
    #[must_use]
    pub(crate) fn can_drop_on(&self, dest: &std::path::Path) -> bool {
        !self.sources.is_empty()
            && self
                .sources
                .iter()
                .all(|src| src != dest && !dest.starts_with(src))
    }
}

/// Reversible Finder filesystem action stored on `FilesAppState::undo_stack`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UndoAction {
    /// Undoing a single inline rename restores `new_path` -> `old_path`.
    Rename {
        new_path: PathBuf,
        old_path: PathBuf,
    },
    /// Undoing a batch rename restores every `(new_path, old_path)`.
    BatchRename { pairs: Vec<(PathBuf, PathBuf)> },
    /// Undoing `New Folder`, `Duplicate`, or `Copy` moves created items to Trash.
    RemoveCreated { created_paths: Vec<PathBuf> },
    /// Undoing `Move` transfers `(current_path, original_path)` back to original locations.
    MoveBack { pairs: Vec<(PathBuf, PathBuf)> },
    /// Undoing `Move to Trash` restores `trash_names` via `.trashinfo`.
    RestoreTrashed { trash_names: Vec<String> },
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

/// File chooser portal mode when `rusty-files` is launched as an XDG `FileChooser` dialog (`RUSTYOS-154`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FilePickerMode {
    OpenFile,
    OpenFiles,
    OpenFolder,
    SaveFile { default_name: String },
}

/// Startup options parsed from CLI flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct StartupOptions {
    pub initial_path: Option<PathBuf>,
    pub open_trash: bool,
    pub open_recents: bool,
    pub theme_override: Option<FilesThemeMode>,
    pub open_test_popup: bool,
    pub no_gpu: bool,
    pub picker_mode: Option<FilePickerMode>,
    pub picker_title: Option<String>,
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

/// Grouping dimension applied across `Icons`, `List`, and `Columns` views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum GroupByMode {
    #[default]
    None,
    Kind,
    DateModified,
    Size,
    Tags,
}

impl GroupByMode {
    pub(crate) const ALL: [Self; 5] = [
        Self::None,
        Self::Kind,
        Self::DateModified,
        Self::Size,
        Self::Tags,
    ];

    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Kind => "Kind",
            Self::DateModified => "Date Modified",
            Self::Size => "Size",
            Self::Tags => "Tags",
        }
    }
}

/// Active rubber-band drag-selection box in empty canvas regions of `Icons` or `List` views.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MarqueeSelectionState {
    pub start_pos: (f64, f64),
    pub current_pos: (f64, f64),
    pub active: bool,
    pub additive: bool,
    pub baseline_paths: BTreeSet<PathBuf>,
}

impl MarqueeSelectionState {
    #[must_use]
    pub(crate) fn start(
        start_pos: (f64, f64),
        additive: bool,
        baseline_paths: BTreeSet<PathBuf>,
    ) -> Self {
        Self {
            start_pos,
            current_pos: start_pos,
            active: false,
            additive,
            baseline_paths,
        }
    }

    pub(crate) fn update_pos(&mut self, pos: (f64, f64)) -> bool {
        self.current_pos = pos;
        if !self.active {
            let dx = pos.0 - self.start_pos.0;
            let dy = pos.1 - self.start_pos.1;
            if dx * dx + dy * dy >= 36.0 {
                self.active = true;
            }
        }
        self.active
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn rect(&self) -> Option<crate::layout::Rect> {
        if !self.active {
            return None;
        }
        let x0 = self.start_pos.0.min(self.current_pos.0).round() as i32;
        let y0 = self.start_pos.1.min(self.current_pos.1).round() as i32;
        let x1 = self.start_pos.0.max(self.current_pos.0).round() as i32;
        let y1 = self.start_pos.1.max(self.current_pos.1).round() as i32;
        Some(crate::layout::Rect::new(
            x0,
            y0,
            (x1 - x0).max(2),
            (y1 - y0).max(2),
        ))
    }
}

/// Strategy used by the multi-item `Batch Rename` sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum BatchRenameMode {
    #[default]
    ReplaceText,
    AddText,
    FormatIndex,
}

impl BatchRenameMode {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ReplaceText => "Replace Text",
            Self::AddText => "Add Text",
            Self::FormatIndex => "Format",
        }
    }
}

/// Interactive multi-file Batch Rename modal state (`F2` when 2+ entries are selected).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameState {
    pub targets: Vec<PathBuf>,
    pub mode: BatchRenameMode,
    pub find_text: String,
    pub replace_text: String,
    pub add_text: String,
    pub add_as_prefix: bool,
    pub format_base: String,
    pub start_index: usize,
    pub active_field: usize,
    pub error: Option<String>,
}

impl BatchRenameState {
    #[must_use]
    pub(crate) fn new(targets: Vec<PathBuf>) -> Self {
        Self {
            targets,
            mode: BatchRenameMode::ReplaceText,
            find_text: String::new(),
            replace_text: String::new(),
            add_text: String::new(),
            add_as_prefix: true,
            format_base: String::from("Item "),
            start_index: 1,
            active_field: 0,
            error: None,
        }
    }

    #[must_use]
    pub(crate) fn compute_target_name(&self, old_name: &str, idx: usize) -> String {
        match self.mode {
            BatchRenameMode::ReplaceText => {
                if self.find_text.is_empty() {
                    old_name.to_owned()
                } else {
                    old_name.replace(&self.find_text, &self.replace_text)
                }
            }
            BatchRenameMode::AddText => {
                if self.add_as_prefix {
                    format!("{}{old_name}", self.add_text)
                } else if let Some((stem, ext)) = old_name.rsplit_once('.') {
                    if !stem.is_empty() {
                        return format!("{stem}{}.{ext}", self.add_text);
                    }
                    format!("{old_name}{}", self.add_text)
                } else {
                    format!("{old_name}{}", self.add_text)
                }
            }
            BatchRenameMode::FormatIndex => {
                let num = self.start_index.saturating_add(idx);
                if let Some((stem, ext)) = old_name.rsplit_once('.')
                    && !stem.is_empty()
                {
                    return format!("{}{num}.{ext}", self.format_base);
                }
                format!("{}{num}", self.format_base)
            }
        }
    }

    #[must_use]
    pub(crate) fn preview_pairs(&self) -> Vec<(PathBuf, String)> {
        self.targets
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let old_name = p
                    .file_name()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let next = self.compute_target_name(&old_name, i);
                (p.clone(), next)
            })
            .collect()
    }
}

/// Section header height inserted above each group when `GroupByMode != None`.
pub(crate) const GROUP_HEADER_HEIGHT: i32 = 24;

/// Computes group ordering & `(first_row_idx, title)` section boundaries for a column's entries.
#[must_use]
pub(crate) fn apply_grouping_to_entries(
    entries: Vec<FileEntry>,
    depths: Vec<u16>,
    mode: GroupByMode,
    tags_config: &rusty_services::file_manager::FileTagsConfig,
    now_secs: i64,
) -> (Vec<FileEntry>, Vec<u16>, Vec<(usize, String)>) {
    if mode == GroupByMode::None || entries.is_empty() {
        return (entries, depths, Vec::new());
    }
    let mut pairs: Vec<(u8, String, usize, FileEntry, u16)> = entries
        .into_iter()
        .zip(depths.into_iter().chain(std::iter::repeat(0)))
        .enumerate()
        .map(|(orig_idx, (entry, depth))| {
            let (rank, label) = group_bucket_for_entry(&entry, mode, tags_config, now_secs);
            (rank, label, orig_idx, entry, depth)
        })
        .collect();
    pairs.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    let mut out_entries = Vec::with_capacity(pairs.len());
    let mut out_depths = Vec::with_capacity(pairs.len());
    let mut sections = Vec::new();
    let mut prev_label: Option<String> = None;
    for (row_idx, (_, label, _, entry, depth)) in pairs.into_iter().enumerate() {
        if prev_label.as_deref() != Some(label.as_str()) {
            sections.push((row_idx, label.clone()));
            prev_label = Some(label);
        }
        out_entries.push(entry);
        out_depths.push(depth);
    }
    (out_entries, out_depths, sections)
}

fn group_bucket_for_entry(
    entry: &FileEntry,
    mode: GroupByMode,
    tags_config: &rusty_services::file_manager::FileTagsConfig,
    now_secs: i64,
) -> (u8, String) {
    match mode {
        GroupByMode::None => (0, String::new()),
        GroupByMode::Kind => {
            let rank = u8::from(!entry.is_navigable_dir());
            (rank, entry.kind_label().to_owned())
        }
        GroupByMode::DateModified => {
            let Some(mtime) = entry.modified_unix_secs else {
                return (5, String::from("Earlier"));
            };
            let age = now_secs.saturating_sub(mtime);
            if age < 86_400 {
                (0, String::from("Today"))
            } else if age < 172_800 {
                (1, String::from("Yesterday"))
            } else if age < 7 * 86_400 {
                (2, String::from("Previous 7 Days"))
            } else if age < 30 * 86_400 {
                (3, String::from("Previous 30 Days"))
            } else {
                (4, String::from("Older"))
            }
        }
        GroupByMode::Size => {
            if entry.is_navigable_dir() {
                (3, String::from("Folders"))
            } else if entry.size_bytes >= 100 * 1024 * 1024 {
                (0, String::from("Large (> 100 MB)"))
            } else if entry.size_bytes >= 1024 * 1024 {
                (1, String::from("Medium (1 - 100 MB)"))
            } else {
                (2, String::from("Small (< 1 MB)"))
            }
        }
        GroupByMode::Tags => {
            let tags = tags_config.tags_for(&entry.path);
            if let Some(first) = tags.first() {
                (0, first.label().to_owned())
            } else {
                (1, String::from("No Tags"))
            }
        }
    }
}

/// Computes total pixel height consumed by visible sidebar items + section headers + bottom padding.
#[must_use]
pub(crate) fn sidebar_total_height(
    items: &[SidebarRowItem],
    collapsed_sections: &BTreeSet<&'static str>,
) -> i32 {
    let mut h = 0_i32;
    let mut current_section: Option<&'static str> = None;
    for item in items {
        if let Some(sec) = item.section {
            current_section = Some(sec);
            h += SIDEBAR_SECTION_HEIGHT;
        }
        if current_section.is_some_and(|s| collapsed_sections.contains(s)) {
            continue;
        }
        h += SIDEBAR_ROW_HEIGHT;
    }
    h + 14
}
