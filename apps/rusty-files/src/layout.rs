//! Deterministic Golden Gate geometry layout and pointer hit-testing for `rusty-files`
//! (`RUSTYOS-065`, ADR-0030). Both rendering and input handling read the same `FilesLayout`.

use std::path::PathBuf;

use rusty_services::file_manager::{
    FileEntry, FileSortField, FileSortOrder, FileTagColor, FileViewMode, SidebarLocation,
    TransferConflictDecision,
};

use crate::dialogs::{
    DialogOverlaysLayout, compute_dialog_overlays_layout, hit_test_dialog_overlays,
};
use crate::state::{
    FilesAppState, LIST_ROW_HEIGHT, MILLER_COLUMN_WIDTH, OpenPopupMenu, ROW_HEIGHT,
    SIDEBAR_ROW_HEIGHT, SIDEBAR_SECTION_HEIGHT, ScrollTargetArea,
};
use crate::views::{
    GalleryLayout, IconsGridLayout, PathCrumbLayout, TabBarLayout, compute_gallery_layout,
    compute_icons_grid_layout, compute_path_breadcrumbs_layout, compute_tab_bar_layout,
    hit_test_gallery, hit_test_icons_grid, hit_test_path_bar, hit_test_tab_bar,
};

/// Window corner radius matching macOS 27 Golden Gate (`20 px`).
pub(crate) const WINDOW_CORNER_RADIUS: i32 = 20;
/// Floating sidebar corner radius (`14 px`).
pub(crate) const SIDEBAR_CORNER_RADIUS: i32 = 14;
/// Floating sidebar width (`186 px`).
pub(crate) const SIDEBAR_WIDTH: i32 = 186;
/// Resize border hotspot thickness around CSD window edges.
const RESIZE_BORDER_PX: i32 = 5;

/// Axis-aligned integer rectangle in surface-local logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// Constructs a rectangle.
    #[must_use]
    pub(crate) const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Right edge (`x + w`).
    #[must_use]
    pub(crate) const fn right(self) -> i32 {
        self.x + self.w
    }

    /// Bottom edge (`y + h`).
    #[must_use]
    pub(crate) const fn bottom(self) -> i32 {
        self.y + self.h
    }

    /// True if `(px, py)` falls inside `[x, right) x [y, bottom)`.
    #[must_use]
    pub(crate) fn contains_f64(self, px: f64, py: f64) -> bool {
        self.w > 0
            && self.h > 0
            && px >= f64::from(self.x)
            && px < f64::from(self.right())
            && py >= f64::from(self.y)
            && py < f64::from(self.bottom())
    }
}

/// CSD resize edge zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResizeEdgeZone {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Sidebar row layout box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidebarRowLayout {
    pub index: usize,
    pub section_header_y: Option<i32>,
    pub rect: Rect,
    pub location: SidebarLocation,
}

/// Column pane layout in Miller Columns mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnLayout {
    pub col_idx: usize,
    pub rect: Rect,
    pub rows: Vec<(usize, Rect)>,
}

/// File inspector / preview card shown to the right of selected file in Columns mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewLayout {
    pub rect: Rect,
    pub entry: FileEntry,
    pub open_btn: Rect,
    pub trash_btn: Rect,
    pub tag_dots: Vec<(FileTagColor, Rect)>,
}

/// Command executed when clicking a popover / context menu row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PopupCommand {
    SortBy(FileSortField),
    SetSortOrder(FileSortOrder),
    NewFolder,
    NewTab,
    NewTabIn(PathBuf),
    ToggleTabBar,
    QuickLook,
    GetInfoSelected,
    GetInfoPath(PathBuf),
    GoToFolder,
    CopySelected,
    CutSelected,
    PasteIntoActive,
    DuplicateSelected,
    RenameSelected,
    ToggleShowHidden,
    ToggleTheme,
    OpenEntry(FileEntry),
    MoveToTrash(FileEntry),
    RestoreEntry(FileEntry),
    EmptyTrash,
    ToggleTag { path: PathBuf, color: FileTagColor },
}

/// Visual separator or actionable row inside an open popover menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PopupItemLayout {
    pub rect: Rect,
    pub label: String,
    pub checked: bool,
    pub swatch_rgb: Option<[u8; 3]>,
    pub command: Option<PopupCommand>,
}

/// Computed popover menu geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PopupLayout {
    pub rect: Rect,
    pub items: Vec<PopupItemLayout>,
}

/// Unclamped native Wayland `xdg_popup` geometry relative to the parent window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PopupSurfaceSpec {
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub surface_width: i32,
    pub surface_height: i32,
    pub shadow_pad: i32,
    pub layout: PopupLayout,
}

/// Semantic hit result for any left/right click inside `rusty-files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FilesHit {
    ResizeEdge(ResizeEdgeZone),
    WindowDrag,
    TrafficClose,
    TrafficMinimize,
    TrafficMaximize,
    NavBack,
    NavForward,
    ViewMode(FileViewMode),
    SortButton,
    ActionPrimary,
    ActionTagPicker,
    ActionMoreMenu,
    SearchToggle,
    SearchClear,
    EmptyTrashBanner,
    SidebarItem(SidebarLocation),
    ColumnRow { col_idx: usize, row_idx: usize },
    ColumnEmpty { col_idx: usize },
    ListHeaderSort(FileSortField),
    ListRow { row_idx: usize },
    ListEmpty,
    IconCell { idx: usize },
    IconsEmpty,
    GalleryThumb { idx: usize },
    GalleryStageOpen,
    GalleryShowMoreInfo,
    GalleryQuickLookBtn,
    GalleryInfoBtn,
    GalleryOpenBtn,
    GalleryTrashBtn,
    GalleryTag { path: PathBuf, color: FileTagColor },
    TabAdd,
    TabSelect(usize),
    TabClose(usize),
    PathCrumb(PathBuf),
    QuickLookClose,
    QuickLookPrev,
    QuickLookNext,
    QuickLookOpen,
    QuickLookModalBody,
    GetInfoClose,
    GetInfoMoreToggle,
    GetInfoTag { path: PathBuf, color: FileTagColor },
    GetInfoModalBody,
    GoToClose,
    GoToClear,
    GoToSuggestion(usize),
    GoToModalBody,
    TransferStop,
    ConflictDecision(TransferConflictDecision),
    InlineRenameBox,
    PreviewOpen,
    PreviewTrashOrRestore,
    PreviewTag { path: PathBuf, color: FileTagColor },
    PopupItem(PopupCommand),
    ThemeFooterToggle,
}

/// Complete computed UI geometry for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilesLayout {
    pub width: i32,
    pub height: i32,
    pub sidebar_rect: Rect,
    pub sidebar_clip_rect: Rect,
    pub traffic_close: Rect,
    pub traffic_min: Rect,
    pub traffic_max: Rect,
    pub sidebar_rows: Vec<SidebarRowLayout>,
    pub toolbar_rect: Rect,
    pub nav_pill_rect: Rect,
    pub nav_back_rect: Rect,
    pub nav_forward_rect: Rect,
    pub title_x: i32,
    pub title_max_x: i32,
    pub view_pill_rect: Rect,
    pub view_segments: [(FileViewMode, Rect); 4],
    pub sort_pill_rect: Rect,
    pub action_pill_rect: Rect,
    pub action_primary_rect: Rect,
    pub action_tag_rect: Rect,
    pub action_more_rect: Rect,
    pub search_pill_rect: Rect,
    pub search_clear_rect: Rect,
    pub trash_banner_rect: Option<Rect>,
    pub empty_trash_btn_rect: Option<Rect>,
    pub content_rect: Rect,
    pub columns: Vec<ColumnLayout>,
    pub preview: Option<PreviewLayout>,
    pub list_headers: Vec<(FileSortField, Rect)>,
    pub list_rows: Vec<(usize, Rect)>,
    pub status_bar_rect: Rect,
    pub path_crumbs: Vec<PathCrumbLayout>,
    pub theme_toggle_rect: Rect,
    pub tab_bar: Option<TabBarLayout>,
    pub icons_grid: Option<IconsGridLayout>,
    pub gallery: Option<GalleryLayout>,
    pub popup: Option<PopupLayout>,
    pub overlays: DialogOverlaysLayout,
}

struct ToolbarRects {
    toolbar_rect: Rect,
    nav_pill_rect: Rect,
    nav_back_rect: Rect,
    nav_forward_rect: Rect,
    title_x: i32,
    title_max_x: i32,
    view_pill_rect: Rect,
    view_segments: [(FileViewMode, Rect); 4],
    sort_pill_rect: Rect,
    action_pill_rect: Rect,
    action_primary_rect: Rect,
    action_tag_rect: Rect,
    action_more_rect: Rect,
    search_pill_rect: Rect,
    search_clear_rect: Rect,
}

struct ContentAreaLayout {
    columns: Vec<ColumnLayout>,
    preview: Option<PreviewLayout>,
    list_headers: Vec<(FileSortField, Rect)>,
    list_rows: Vec<(usize, Rect)>,
    icons_grid: Option<IconsGridLayout>,
    gallery: Option<GalleryLayout>,
}

struct ListAreaLayout {
    headers: Vec<(FileSortField, Rect)>,
    rows: Vec<(usize, Rect)>,
}

/// Computes pixel-accurate layout for `state` at `(surface_w, surface_h)`.
#[must_use]
pub(crate) fn compute_layout(state: &FilesAppState, surface_w: u32, surface_h: u32) -> FilesLayout {
    let width = i32::try_from(surface_w).unwrap_or(1060).max(480);
    let height = i32::try_from(surface_h).unwrap_or(660).max(360);

    let sidebar_rect = Rect::new(10, 10, SIDEBAR_WIDTH, (height - 20).max(120));
    let sidebar_clip_rect = Rect::new(
        sidebar_rect.x + 2,
        46,
        sidebar_rect.w - 4,
        (sidebar_rect.bottom() - 8 - 46).max(40),
    );
    let traffic_close = Rect::new(22, 23, 14, 14);
    let traffic_min = Rect::new(42, 23, 14, 14);
    let traffic_max = Rect::new(62, 23, 14, 14);

    let sidebar_rows = layout_sidebar_rows(state, sidebar_rect, sidebar_clip_rect);
    let toolbar_x = sidebar_rect.right() + 12;
    let tb = layout_toolbar(state, width, toolbar_x);
    let tab_bar = compute_tab_bar_layout(state, toolbar_x, width);
    let base_top = if tab_bar.is_some() { 88 } else { 58 };

    let (trash_banner_rect, empty_trash_btn_rect, content_top) = if state.is_trash_view() {
        let banner = Rect::new(toolbar_x, base_top, width - toolbar_x - 12, 32);
        let btn = Rect::new(banner.right() - 78, base_top + 4, 72, 24);
        (Some(banner), Some(btn), base_top + 34)
    } else {
        (None, None, base_top)
    };

    let status_bar_rect = Rect::new(toolbar_x, height - 28, width - toolbar_x - 10, 24);
    let theme_toggle_rect = Rect::new(status_bar_rect.right() - 104, status_bar_rect.y + 3, 98, 19);
    let status_w = crate::render::Canvas::measure_text(&state.status_summary(), 11.5, false);
    let crumbs_limit_x = (theme_toggle_rect.x - status_w - 22).max(status_bar_rect.x + 12);
    let path_crumbs = compute_path_breadcrumbs_layout(state, status_bar_rect, crumbs_limit_x);
    let content_rect = Rect::new(
        toolbar_x,
        content_top,
        width - toolbar_x - 8,
        (status_bar_rect.y - 4 - content_top).max(60),
    );

    let content = layout_content_area(state, content_rect);
    let inline_anchor = compute_inline_rename_anchor(state, content_rect, &content);
    let overlays = compute_dialog_overlays_layout(state, width, height, inline_anchor);

    let mut base = FilesLayout {
        width,
        height,
        sidebar_rect,
        sidebar_clip_rect,
        traffic_close,
        traffic_min,
        traffic_max,
        sidebar_rows,
        toolbar_rect: tb.toolbar_rect,
        nav_pill_rect: tb.nav_pill_rect,
        nav_back_rect: tb.nav_back_rect,
        nav_forward_rect: tb.nav_forward_rect,
        title_x: tb.title_x,
        title_max_x: tb.title_max_x,
        view_pill_rect: tb.view_pill_rect,
        view_segments: tb.view_segments,
        sort_pill_rect: tb.sort_pill_rect,
        action_pill_rect: tb.action_pill_rect,
        action_primary_rect: tb.action_primary_rect,
        action_tag_rect: tb.action_tag_rect,
        action_more_rect: tb.action_more_rect,
        search_pill_rect: tb.search_pill_rect,
        search_clear_rect: tb.search_clear_rect,
        trash_banner_rect,
        empty_trash_btn_rect,
        content_rect,
        columns: content.columns,
        preview: content.preview,
        list_headers: content.list_headers,
        list_rows: content.list_rows,
        status_bar_rect,
        path_crumbs,
        theme_toggle_rect,
        tab_bar,
        icons_grid: content.icons_grid,
        gallery: content.gallery,
        popup: None,
        overlays,
    };

    base.popup = state
        .popup
        .as_ref()
        .map(|popup| compute_popup_layout(state, &base, popup));
    base
}

fn compute_inline_rename_anchor(
    state: &FilesAppState,
    content_rect: Rect,
    content: &ContentAreaLayout,
) -> Option<Rect> {
    let rn = state.inline_rename.as_ref()?;
    if let Some(grid) = &content.icons_grid
        && let Some(cell) = grid.cells.iter().find(|c| c.row_idx == rn.row_idx)
    {
        return Some(Rect::new(
            (cell.rect.x - 10).max(content_rect.x + 4),
            cell.label_rect.y,
            (cell.rect.w + 20).clamp(132, 220),
            24,
        ));
    }
    if let Some(gal) = &content.gallery {
        return Some(Rect::new(
            gal.info_rect.x + 12,
            gal.info_rect.y + 10,
            (gal.info_rect.w - 24).max(120),
            26,
        ));
    }
    if !content.list_rows.is_empty()
        && let Some((_, row)) = content.list_rows.iter().find(|(idx, _)| *idx == rn.row_idx)
    {
        return Some(Rect::new(
            row.x + 28,
            row.y + 2,
            (row.w - 220).clamp(160, 320),
            (row.h - 4).max(20),
        ));
    }
    if let Some(col) = content.columns.iter().find(|c| c.col_idx == rn.col_idx)
        && let Some((_, row)) = col.rows.iter().find(|(idx, _)| *idx == rn.row_idx)
    {
        return Some(Rect::new(
            row.x + 24,
            row.y + 2,
            (row.w - 38).max(110),
            (row.h - 4).max(20),
        ));
    }
    None
}

fn layout_toolbar(state: &FilesAppState, width: i32, toolbar_x: i32) -> ToolbarRects {
    let toolbar_rect = Rect::new(toolbar_x, 10, (width - toolbar_x - 10).max(200), 46);
    let nav_pill_rect = Rect::new(toolbar_x + 4, 15, 68, 32);
    let nav_back_rect = Rect::new(nav_pill_rect.x, nav_pill_rect.y, 34, 32);
    let nav_forward_rect = Rect::new(nav_pill_rect.x + 34, nav_pill_rect.y, 34, 32);
    let title_x = nav_pill_rect.right() + 14;

    let right_margin = width - 16;
    let fixed_cluster_w = 136 + 8 + 54 + 8 + 102 + 8;
    let min_cluster_x = nav_pill_rect.right() + 8;
    let max_search_w = (right_margin - min_cluster_x - fixed_cluster_w).max(32);
    let search_w = if state.search_active {
        172.min(max_search_w)
    } else {
        32
    };
    let search_pill_rect = Rect::new(right_margin - search_w, 15, search_w, 32);
    let search_clear_rect = if state.search_active && !state.search_query.is_empty() {
        Rect::new(search_pill_rect.right() - 24, 21, 18, 20)
    } else {
        Rect::default()
    };

    let action_pill_rect = Rect::new(search_pill_rect.x - 8 - 102, 15, 102, 32);
    let action_primary_rect = Rect::new(action_pill_rect.x, 15, 34, 32);
    let action_tag_rect = Rect::new(action_pill_rect.x + 34, 15, 34, 32);
    let action_more_rect = Rect::new(action_pill_rect.x + 68, 15, 34, 32);

    let sort_pill_rect = Rect::new(action_pill_rect.x - 8 - 54, 15, 54, 32);
    let view_pill_rect = Rect::new(sort_pill_rect.x - 8 - 136, 15, 136, 32);
    let view_segments = [
        (FileViewMode::Icons, Rect::new(view_pill_rect.x, 15, 34, 32)),
        (
            FileViewMode::List,
            Rect::new(view_pill_rect.x + 34, 15, 34, 32),
        ),
        (
            FileViewMode::Columns,
            Rect::new(view_pill_rect.x + 68, 15, 34, 32),
        ),
        (
            FileViewMode::Gallery,
            Rect::new(view_pill_rect.x + 102, 15, 34, 32),
        ),
    ];
    let title_max_x = (view_pill_rect.x - 10).max(title_x);

    ToolbarRects {
        toolbar_rect,
        nav_pill_rect,
        nav_back_rect,
        nav_forward_rect,
        title_x,
        title_max_x,
        view_pill_rect,
        view_segments,
        sort_pill_rect,
        action_pill_rect,
        action_primary_rect,
        action_tag_rect,
        action_more_rect,
        search_pill_rect,
        search_clear_rect,
    }
}

fn layout_sidebar_rows(
    state: &FilesAppState,
    sidebar_rect: Rect,
    clip: Rect,
) -> Vec<SidebarRowLayout> {
    let mut rows = Vec::with_capacity(state.sidebar_items.len());
    let mut y = 48 - state.sidebar_scroll_y;
    for (index, item) in state.sidebar_items.iter().enumerate() {
        let top_y = y;
        let section_header_y = if item.section.is_some() {
            let hdr_y = y + 3;
            y += SIDEBAR_SECTION_HEIGHT;
            Some(hdr_y)
        } else {
            None
        };
        let rect = Rect::new(
            sidebar_rect.x + 8,
            y,
            sidebar_rect.w - 16,
            SIDEBAR_ROW_HEIGHT,
        );
        if rect.bottom() > clip.y && top_y < clip.bottom() {
            rows.push(SidebarRowLayout {
                index,
                section_header_y,
                rect,
                location: item.location.clone(),
            });
        }
        y += SIDEBAR_ROW_HEIGHT;
    }
    rows
}

fn layout_content_area(state: &FilesAppState, content_rect: Rect) -> ContentAreaLayout {
    if state.view_mode == FileViewMode::Icons {
        return ContentAreaLayout {
            columns: Vec::new(),
            preview: None,
            list_headers: Vec::new(),
            list_rows: Vec::new(),
            icons_grid: Some(compute_icons_grid_layout(state, content_rect)),
            gallery: None,
        };
    }
    if state.view_mode == FileViewMode::Gallery {
        return ContentAreaLayout {
            columns: Vec::new(),
            preview: None,
            list_headers: Vec::new(),
            list_rows: Vec::new(),
            icons_grid: None,
            gallery: Some(compute_gallery_layout(state, content_rect)),
        };
    }
    if state.view_mode == FileViewMode::List {
        let list = layout_list_view(state, content_rect);
        return ContentAreaLayout {
            columns: Vec::new(),
            preview: None,
            list_headers: list.headers,
            list_rows: list.rows,
            icons_grid: None,
            gallery: None,
        };
    }

    let mut cols = Vec::with_capacity(state.columns.len());
    let total_cols_w = i32::try_from(state.columns.len()).unwrap_or(1) * MILLER_COLUMN_WIDTH;
    let preview_reserve = if state
        .selected_entry()
        .is_some_and(|e| !e.is_navigable_dir())
    {
        240
    } else {
        0
    };
    let max_overflow = (total_cols_w + preview_reserve - content_rect.w).max(0);
    let scroll_x = if max_overflow > 0 {
        if state.follow_right_column {
            max_overflow
        } else {
            state.column_scroll_x.clamp(0, max_overflow)
        }
    } else {
        0
    };

    let mut cur_x = content_rect.x - scroll_x;
    for (col_idx, col) in state.columns.iter().enumerate() {
        let col_rect = Rect::new(cur_x, content_rect.y, MILLER_COLUMN_WIDTH, content_rect.h);
        let mut rows = Vec::new();
        let mut ry = content_rect.y + 6 - col.scroll_y;
        for row_idx in 0..col.visible_entries.len() {
            let r = Rect::new(cur_x + 8, ry, MILLER_COLUMN_WIDTH - 16, ROW_HEIGHT);
            if r.bottom() > content_rect.y && r.y < content_rect.bottom() {
                rows.push((row_idx, r));
            }
            ry += ROW_HEIGHT;
        }
        cols.push(ColumnLayout {
            col_idx,
            rect: col_rect,
            rows,
        });
        cur_x += MILLER_COLUMN_WIDTH;
    }

    let preview = compute_preview_layout(state, content_rect, cur_x);
    ContentAreaLayout {
        columns: cols,
        preview,
        list_headers: Vec::new(),
        list_rows: Vec::new(),
        icons_grid: None,
        gallery: None,
    }
}

fn compute_preview_layout(
    state: &FilesAppState,
    content_rect: Rect,
    start_x: i32,
) -> Option<PreviewLayout> {
    let entry = state.selected_entry()?.clone();
    if entry.is_navigable_dir() && !state.is_trash_view() {
        return None;
    }
    let avail_w = content_rect.right() - start_x;
    if avail_w < 200 {
        return None;
    }
    let rect = Rect::new(start_x, content_rect.y, avail_w, content_rect.h);
    let btn_y = rect.y + 214;
    let btn_w = ((avail_w - 48) / 2).clamp(76, 128);
    let center_x = rect.x + avail_w / 2;
    let open_btn = Rect::new(center_x - btn_w - 6, btn_y, btn_w, 28);
    let trash_btn = Rect::new(center_x + 6, btn_y, btn_w, 28);

    let mut tag_dots = Vec::with_capacity(FileTagColor::ALL.len());
    let start_dots_x = center_x - 66;
    for (i, color) in FileTagColor::ALL.iter().enumerate() {
        let dx = start_dots_x + i32::try_from(i).unwrap_or(0) * 24;
        tag_dots.push((*color, Rect::new(dx, btn_y + 42, 16, 16)));
    }
    Some(PreviewLayout {
        rect,
        entry,
        open_btn,
        trash_btn,
        tag_dots,
    })
}

fn layout_list_view(state: &FilesAppState, content_rect: Rect) -> ListAreaLayout {
    let hdr_y = content_rect.y;
    let hdr_h = 26;
    let name_w = (content_rect.w - 310).max(160);
    let date_x = content_rect.x + name_w;
    let size_x = date_x + 140;
    let kind_x = size_x + 78;

    let headers = vec![
        (
            FileSortField::Name,
            Rect::new(content_rect.x, hdr_y, name_w, hdr_h),
        ),
        (
            FileSortField::DateModified,
            Rect::new(date_x, hdr_y, 140, hdr_h),
        ),
        (FileSortField::Size, Rect::new(size_x, hdr_y, 78, hdr_h)),
        (
            FileSortField::Kind,
            Rect::new(
                kind_x,
                hdr_y,
                (content_rect.right() - kind_x).max(60),
                hdr_h,
            ),
        ),
    ];

    let mut rows = Vec::new();
    if let Some(col) = state.columns.first() {
        let body_top = hdr_y + hdr_h + 2;
        let mut ry = body_top - state.list_scroll_y;
        for row_idx in 0..col.visible_entries.len() {
            let r = Rect::new(content_rect.x + 6, ry, content_rect.w - 18, LIST_ROW_HEIGHT);
            if r.bottom() > body_top && r.y < content_rect.bottom() {
                rows.push((row_idx, r));
            }
            ry += LIST_ROW_HEIGHT;
        }
    }
    ListAreaLayout { headers, rows }
}

fn measure_popup_height(entries: &[PopupRowSpec]) -> i32 {
    let row_h = 28_i32;
    let sep_h = 9_i32;
    let mut total_h = 12_i32;
    for (_, _, _, cmd) in entries {
        total_h += if cmd.is_some() { row_h } else { sep_h };
    }
    total_h
}

fn build_popup_items(
    x: i32,
    y: i32,
    menu_w: i32,
    total_h: i32,
    entries: Vec<PopupRowSpec>,
) -> PopupLayout {
    let row_h = 28_i32;
    let sep_h = 9_i32;
    let rect = Rect::new(x, y, menu_w, total_h);
    let mut items = Vec::with_capacity(entries.len());
    let mut cy = y + 6;
    for (label, checked, swatch_rgb, command) in entries {
        let h = if command.is_some() { row_h } else { sep_h };
        items.push(PopupItemLayout {
            rect: Rect::new(x + 5, cy, menu_w - 10, h),
            label,
            checked,
            swatch_rgb,
            command,
        });
        cy += h;
    }
    PopupLayout { rect, items }
}

fn compute_popup_layout(
    state: &FilesAppState,
    layout: &FilesLayout,
    popup: &OpenPopupMenu,
) -> PopupLayout {
    let (anchor_x, anchor_y, menu_w, entries) = build_popup_entries(state, layout, popup);
    let total_h = measure_popup_height(&entries);
    let x = anchor_x.clamp(8, (layout.width - menu_w - 8).max(8));
    let y = anchor_y.clamp(8, (layout.height - total_h - 8).max(8));
    build_popup_items(x, y, menu_w, total_h, entries)
}

/// Computes an unconstrained `xdg_popup` surface spec so menus can extend outside the parent window.
pub(crate) fn compute_popup_surface_spec(
    state: &FilesAppState,
    layout: &FilesLayout,
    popup: &OpenPopupMenu,
    shadow_pad: i32,
) -> PopupSurfaceSpec {
    let (anchor_x, anchor_y, menu_w, entries) = build_popup_entries(state, layout, popup);
    let total_h = measure_popup_height(&entries);
    let pad = shadow_pad.max(0);
    let popup_layout = build_popup_items(pad, pad, menu_w, total_h, entries);
    PopupSurfaceSpec {
        anchor_x,
        anchor_y,
        surface_width: menu_w + pad * 2,
        surface_height: total_h + pad * 2,
        shadow_pad: pad,
        layout: popup_layout,
    }
}

type PopupRowSpec = (String, bool, Option<[u8; 3]>, Option<PopupCommand>);

fn build_popup_entries(
    state: &FilesAppState,
    layout: &FilesLayout,
    popup: &OpenPopupMenu,
) -> (i32, i32, i32, Vec<PopupRowSpec>) {
    match popup {
        OpenPopupMenu::SortMenu => (
            layout.sort_pill_rect.x,
            layout.sort_pill_rect.bottom() + 6,
            176,
            sort_menu_rows(state),
        ),
        OpenPopupMenu::ActionsMenu => (
            layout.action_pill_rect.right() - 210,
            layout.action_pill_rect.bottom() + 6,
            210,
            actions_menu_rows(state),
        ),
        OpenPopupMenu::TagPicker { target_path } => (
            layout.action_tag_rect.x - 40,
            layout.action_tag_rect.bottom() + 6,
            164,
            tag_menu_rows(state, target_path),
        ),
        OpenPopupMenu::ContextMenu {
            x, y, target_entry, ..
        } => (*x, *y, 200, context_menu_rows(state, target_entry.as_ref())),
    }
}

fn sort_menu_rows(state: &FilesAppState) -> Vec<PopupRowSpec> {
    let fields = [
        FileSortField::Name,
        FileSortField::DateModified,
        FileSortField::Size,
        FileSortField::Kind,
    ];
    let mut rows: Vec<PopupRowSpec> = fields
        .iter()
        .map(|f| {
            (
                String::from(f.label()),
                state.sort_field == *f,
                None,
                Some(PopupCommand::SortBy(*f)),
            )
        })
        .collect();
    rows.push((String::new(), false, None, None));
    rows.push((
        String::from("Ascending"),
        state.sort_order == FileSortOrder::Ascending,
        None,
        Some(PopupCommand::SetSortOrder(FileSortOrder::Ascending)),
    ));
    rows.push((
        String::from("Descending"),
        state.sort_order == FileSortOrder::Descending,
        None,
        Some(PopupCommand::SetSortOrder(FileSortOrder::Descending)),
    ));
    rows
}

fn actions_menu_rows(state: &FilesAppState) -> Vec<PopupRowSpec> {
    let mut rows = vec![
        (
            String::from("New Folder"),
            false,
            None,
            Some(PopupCommand::NewFolder),
        ),
        (
            String::from("New Tab"),
            false,
            None,
            Some(PopupCommand::NewTab),
        ),
        (
            String::from("Go to Folder..."),
            false,
            None,
            Some(PopupCommand::GoToFolder),
        ),
        (
            String::from("Show Tab Bar"),
            state.tab_bar_visible(),
            None,
            Some(PopupCommand::ToggleTabBar),
        ),
    ];
    append_selection_action_rows(state, &mut rows);
    if !state.is_trash_view() && state.clipboard.is_some() {
        rows.push((
            String::from("Paste Item"),
            false,
            None,
            Some(PopupCommand::PasteIntoActive),
        ));
    }
    rows.push((
        String::from("Empty Trash"),
        false,
        None,
        Some(PopupCommand::EmptyTrash),
    ));
    rows.push((String::new(), false, None, None));
    rows.push((
        String::from("Show Hidden Files"),
        state.show_hidden,
        None,
        Some(PopupCommand::ToggleShowHidden),
    ));
    let theme_label = match state.theme {
        rusty_services::file_manager::FilesThemeMode::LightGoldenGate => "Dark Purple Glass Theme",
        rusty_services::file_manager::FilesThemeMode::DarkPurpleGlass => "Light Golden Gate Theme",
    };
    rows.push((
        String::from(theme_label),
        false,
        None,
        Some(PopupCommand::ToggleTheme),
    ));
    rows
}

fn append_selection_action_rows(state: &FilesAppState, rows: &mut Vec<PopupRowSpec>) {
    let Some(entry) = state.selected_entry() else {
        return;
    };
    rows.push((String::new(), false, None, None));
    rows.push((
        String::from("Quick Look"),
        false,
        None,
        Some(PopupCommand::QuickLook),
    ));
    rows.push((
        String::from("Get Info"),
        false,
        None,
        Some(PopupCommand::GetInfoSelected),
    ));
    if state.is_trash_view() {
        rows.push((
            String::from("Put Back"),
            false,
            None,
            Some(PopupCommand::RestoreEntry(entry.clone())),
        ));
    } else {
        rows.push((
            String::from("Rename..."),
            false,
            None,
            Some(PopupCommand::RenameSelected),
        ));
        rows.push((
            String::from("Copy"),
            false,
            None,
            Some(PopupCommand::CopySelected),
        ));
        rows.push((
            String::from("Cut"),
            false,
            None,
            Some(PopupCommand::CutSelected),
        ));
        rows.push((
            String::from("Duplicate"),
            false,
            None,
            Some(PopupCommand::DuplicateSelected),
        ));
        rows.push((
            String::from("Move to Trash"),
            false,
            None,
            Some(PopupCommand::MoveToTrash(entry.clone())),
        ));
    }
}

fn tag_menu_rows(state: &FilesAppState, target_path: &std::path::Path) -> Vec<PopupRowSpec> {
    let assigned = state.tags_for(target_path);
    FileTagColor::ALL
        .iter()
        .map(|c| {
            (
                String::from(c.label()),
                assigned.contains(c),
                Some(c.rgb()),
                Some(PopupCommand::ToggleTag {
                    path: target_path.to_path_buf(),
                    color: *c,
                }),
            )
        })
        .collect()
}

fn context_menu_rows(state: &FilesAppState, entry: Option<&FileEntry>) -> Vec<PopupRowSpec> {
    let mut rows = Vec::new();
    if let Some(item) = entry {
        append_context_item_rows(state, item, &mut rows);
    } else {
        rows.push((
            String::from("New Folder"),
            false,
            None,
            Some(PopupCommand::NewFolder),
        ));
        rows.push((
            String::from("New Tab"),
            false,
            None,
            Some(PopupCommand::NewTab),
        ));
        if !state.is_trash_view() && state.clipboard.is_some() {
            rows.push((
                String::from("Paste Item"),
                false,
                None,
                Some(PopupCommand::PasteIntoActive),
            ));
        }
        rows.push((
            String::from("Get Info"),
            false,
            None,
            Some(PopupCommand::GetInfoSelected),
        ));
        rows.push((
            String::from("Show Hidden Files"),
            state.show_hidden,
            None,
            Some(PopupCommand::ToggleShowHidden),
        ));
    }
    if state.is_trash_view() {
        rows.push((String::new(), false, None, None));
        rows.push((
            String::from("Empty Trash"),
            false,
            None,
            Some(PopupCommand::EmptyTrash),
        ));
    }
    rows
}

fn append_context_item_rows(state: &FilesAppState, item: &FileEntry, rows: &mut Vec<PopupRowSpec>) {
    rows.push((
        String::from("Open"),
        false,
        None,
        Some(PopupCommand::OpenEntry(item.clone())),
    ));
    if item.is_navigable_dir() {
        rows.push((
            String::from("Open in New Tab"),
            false,
            None,
            Some(PopupCommand::NewTabIn(item.path.clone())),
        ));
    }
    rows.push((
        String::from("Quick Look"),
        false,
        None,
        Some(PopupCommand::QuickLook),
    ));
    rows.push((
        String::from("Get Info"),
        false,
        None,
        Some(PopupCommand::GetInfoPath(item.path.clone())),
    ));
    if state.is_trash_view() {
        rows.push((
            String::from("Put Back"),
            false,
            None,
            Some(PopupCommand::RestoreEntry(item.clone())),
        ));
    } else {
        rows.push((String::new(), false, None, None));
        rows.push((
            String::from("Rename..."),
            false,
            None,
            Some(PopupCommand::RenameSelected),
        ));
        rows.push((
            String::from("Copy"),
            false,
            None,
            Some(PopupCommand::CopySelected),
        ));
        rows.push((
            String::from("Cut"),
            false,
            None,
            Some(PopupCommand::CutSelected),
        ));
        rows.push((
            String::from("Duplicate"),
            false,
            None,
            Some(PopupCommand::DuplicateSelected),
        ));
        rows.push((
            String::from("Move to Trash"),
            false,
            None,
            Some(PopupCommand::MoveToTrash(item.clone())),
        ));
        rows.push((String::new(), false, None, None));
        rows.extend(tag_menu_rows(state, &item.path));
    }
}

/// Resolves which resize edge (if any) covers `(px, py)`.
#[must_use]
pub(crate) fn detect_resize_edge(layout: &FilesLayout, px: f64, py: f64) -> Option<ResizeEdgeZone> {
    let b = f64::from(RESIZE_BORDER_PX);
    let w = f64::from(layout.width);
    let h = f64::from(layout.height);
    let left = px < b;
    let right = px >= w - b;
    let top = py < b;
    let bottom = py >= h - b;

    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeEdgeZone::TopLeft),
        (_, true, true, _) => Some(ResizeEdgeZone::TopRight),
        (true, _, _, true) => Some(ResizeEdgeZone::BottomLeft),
        (_, true, _, true) => Some(ResizeEdgeZone::BottomRight),
        (true, _, _, _) => Some(ResizeEdgeZone::Left),
        (_, true, _, _) => Some(ResizeEdgeZone::Right),
        (_, _, true, _) => Some(ResizeEdgeZone::Top),
        (_, _, _, true) => Some(ResizeEdgeZone::Bottom),
        _ => None,
    }
}

/// Maps a pointer position `(px, py)` to a semantic `FilesHit`.
#[must_use]
pub(crate) fn hit_test(layout: &FilesLayout, px: f64, py: f64) -> Option<FilesHit> {
    if let Some(edge) = detect_resize_edge(layout, px, py) {
        return Some(FilesHit::ResizeEdge(edge));
    }
    if let Some(popup) = &layout.popup
        && popup.rect.contains_f64(px, py)
    {
        for item in &popup.items {
            if item.rect.contains_f64(px, py)
                && let Some(cmd) = &item.command
            {
                return Some(FilesHit::PopupItem(cmd.clone()));
            }
        }
        return None;
    }
    if let Some(hit) = hit_test_dialog_overlays(&layout.overlays, px, py) {
        return Some(hit);
    }

    if let Some(hit) = hit_test_chrome(layout, px, py) {
        return Some(hit);
    }
    hit_test_body(layout, px, py)
}

fn hit_test_chrome(layout: &FilesLayout, px: f64, py: f64) -> Option<FilesHit> {
    if layout.traffic_close.contains_f64(px, py) {
        return Some(FilesHit::TrafficClose);
    }
    if layout.traffic_min.contains_f64(px, py) {
        return Some(FilesHit::TrafficMinimize);
    }
    if layout.traffic_max.contains_f64(px, py) {
        return Some(FilesHit::TrafficMaximize);
    }
    if layout.nav_back_rect.contains_f64(px, py) {
        return Some(FilesHit::NavBack);
    }
    if layout.nav_forward_rect.contains_f64(px, py) {
        return Some(FilesHit::NavForward);
    }
    for (mode, rect) in layout.view_segments {
        if rect.contains_f64(px, py) {
            return Some(FilesHit::ViewMode(mode));
        }
    }
    if layout.sort_pill_rect.contains_f64(px, py) {
        return Some(FilesHit::SortButton);
    }
    if layout.action_primary_rect.contains_f64(px, py) {
        return Some(FilesHit::ActionPrimary);
    }
    if layout.action_tag_rect.contains_f64(px, py) {
        return Some(FilesHit::ActionTagPicker);
    }
    if layout.action_more_rect.contains_f64(px, py) {
        return Some(FilesHit::ActionMoreMenu);
    }
    if layout.search_clear_rect.contains_f64(px, py) {
        return Some(FilesHit::SearchClear);
    }
    if layout.search_pill_rect.contains_f64(px, py) {
        return Some(FilesHit::SearchToggle);
    }
    if let Some(tb) = &layout.tab_bar
        && let Some(hit) = hit_test_tab_bar(tb, px, py)
    {
        return Some(hit);
    }
    if let Some(btn) = layout.empty_trash_btn_rect
        && btn.contains_f64(px, py)
    {
        return Some(FilesHit::EmptyTrashBanner);
    }
    if let Some(hit) = hit_test_path_bar(&layout.path_crumbs, px, py) {
        return Some(hit);
    }
    if layout.theme_toggle_rect.contains_f64(px, py) {
        return Some(FilesHit::ThemeFooterToggle);
    }
    if layout.toolbar_rect.contains_f64(px, py)
        || (layout.sidebar_rect.contains_f64(px, py) && py < 48.0)
    {
        return Some(FilesHit::WindowDrag);
    }
    None
}

fn hit_test_body(layout: &FilesLayout, px: f64, py: f64) -> Option<FilesHit> {
    if layout.sidebar_clip_rect.contains_f64(px, py) {
        for row in &layout.sidebar_rows {
            if row.rect.contains_f64(px, py) {
                return Some(FilesHit::SidebarItem(row.location.clone()));
            }
        }
        return None;
    }

    if !layout.content_rect.contains_f64(px, py) {
        return None;
    }

    if let Some(grid) = &layout.icons_grid {
        return hit_test_icons_grid(grid, px, py);
    }

    if let Some(gal) = &layout.gallery {
        return hit_test_gallery(gal, px, py);
    }

    if let Some(preview) = &layout.preview
        && preview.rect.contains_f64(px, py)
    {
        if preview.open_btn.contains_f64(px, py) {
            return Some(FilesHit::PreviewOpen);
        }
        if preview.trash_btn.contains_f64(px, py) {
            return Some(FilesHit::PreviewTrashOrRestore);
        }
        for (color, rect) in &preview.tag_dots {
            if rect.contains_f64(px, py) {
                return Some(FilesHit::PreviewTag {
                    path: preview.entry.path.clone(),
                    color: *color,
                });
            }
        }
        return None;
    }

    if !layout.list_headers.is_empty() {
        for (field, rect) in &layout.list_headers {
            if rect.contains_f64(px, py) {
                return Some(FilesHit::ListHeaderSort(*field));
            }
        }
        for (row_idx, rect) in &layout.list_rows {
            if rect.contains_f64(px, py) {
                return Some(FilesHit::ListRow { row_idx: *row_idx });
            }
        }
        return Some(FilesHit::ListEmpty);
    }

    for col in &layout.columns {
        if col.rect.contains_f64(px, py) {
            for (row_idx, rect) in &col.rows {
                if rect.contains_f64(px, py) {
                    return Some(FilesHit::ColumnRow {
                        col_idx: col.col_idx,
                        row_idx: *row_idx,
                    });
                }
            }
            return Some(FilesHit::ColumnEmpty {
                col_idx: col.col_idx,
            });
        }
    }
    None
}

/// Resolves which vertical scroll region lies under `(px, py)`.
#[must_use]
pub(crate) fn scroll_area_at(layout: &FilesLayout, px: f64, py: f64) -> Option<ScrollTargetArea> {
    if layout.sidebar_clip_rect.contains_f64(px, py) {
        return Some(ScrollTargetArea::Sidebar);
    }
    if !layout.content_rect.contains_f64(px, py) {
        return None;
    }
    if layout.icons_grid.is_some() {
        return Some(ScrollTargetArea::Icons);
    }
    if let Some(gal) = &layout.gallery
        && !gal.info_rect.contains_f64(px, py)
    {
        return Some(ScrollTargetArea::GalleryStrip);
    }
    if !layout.list_headers.is_empty() {
        return Some(ScrollTargetArea::List);
    }
    for col in &layout.columns {
        if col.rect.contains_f64(px, py) {
            return Some(ScrollTargetArea::Column(col.col_idx));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StartupOptions;
    use rusty_services::file_manager::UserFileLocations;

    fn test_state() -> (FilesAppState, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("rustyos-files-layout-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        (FilesAppState::new(locs, &StartupOptions::default()), root)
    }

    #[test]
    fn golden_gate_layout_and_hit_testing_work() {
        let (mut state, root) = test_state();
        let layout = compute_layout(&state, 1060, 660);

        assert_eq!(layout.sidebar_rect, Rect::new(10, 10, 186, 640));
        assert_eq!(hit_test(&layout, 28.0, 29.0), Some(FilesHit::TrafficClose));
        assert_eq!(
            hit_test(&layout, 48.0, 29.0),
            Some(FilesHit::TrafficMinimize)
        );
        assert_eq!(
            hit_test(&layout, 68.0, 29.0),
            Some(FilesHit::TrafficMaximize)
        );
        assert_eq!(hit_test(&layout, 224.0, 30.0), Some(FilesHit::NavBack));
        assert_eq!(hit_test(&layout, 260.0, 30.0), Some(FilesHit::NavForward));

        let cols_seg = layout.view_segments[2].1;
        assert_eq!(
            hit_test(
                &layout,
                f64::from(cols_seg.x + 12),
                f64::from(cols_seg.y + 12)
            ),
            Some(FilesHit::ViewMode(FileViewMode::Columns))
        );

        let search = layout.search_pill_rect;
        assert_eq!(
            hit_test(&layout, f64::from(search.x + 12), f64::from(search.y + 12)),
            Some(FilesHit::SearchToggle)
        );

        // Popup clamping keeps popover inside window bounds even near bottom-right corner
        state.popup = Some(OpenPopupMenu::ContextMenu {
            x: 1050,
            y: 650,
            target_entry: None,
            target_dir: None,
        });
        let popup_layout = compute_layout(&state, 1060, 660);
        if let Some(p) = &popup_layout.popup {
            assert!(p.rect.right() <= 1052);
            assert!(p.rect.bottom() <= 652);
        }
        state.popup = None;
        state.search_active = true;
        let narrow = compute_layout(&state, 768, 461);
        assert!(narrow.nav_pill_rect.right() < narrow.view_pill_rect.x);
        assert!(narrow.view_pill_rect.right() < narrow.sort_pill_rect.x);
        assert!(narrow.sort_pill_rect.right() < narrow.action_pill_rect.x);
        assert!(narrow.action_pill_rect.right() < narrow.search_pill_rect.x);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stage_b_views_tabs_crumbs_and_overlays_hit_test() {
        let (mut state, root) = test_state();
        let docs = state.locations.home.join("Documents");
        let _ = std::fs::create_dir_all(&docs);
        state.navigate_to_directory(docs.clone());
        state.complete_pending_loads_sync();

        state.set_view_mode(FileViewMode::Icons);
        let icons_layout = compute_layout(&state, 1060, 660);
        assert!(icons_layout.icons_grid.is_some());
        if let Some(crumb) = icons_layout.path_crumbs.last() {
            assert_eq!(
                hit_test(
                    &icons_layout,
                    f64::from(crumb.rect.x + 6),
                    f64::from(crumb.rect.y + 6)
                ),
                Some(FilesHit::PathCrumb(crumb.path.clone()))
            );
        }

        state.new_tab(None);
        state.set_view_mode(FileViewMode::Gallery);
        let gal_layout = compute_layout(&state, 1060, 660);
        assert!(gal_layout.tab_bar.is_some());
        if let Some(tb) = &gal_layout.tab_bar {
            assert_eq!(
                hit_test(
                    &gal_layout,
                    f64::from(tb.add_rect.x + 8),
                    f64::from(tb.add_rect.y + 8)
                ),
                Some(FilesHit::TabAdd)
            );
        }
        assert!(gal_layout.gallery.is_some());
        if let Some(gal) = &gal_layout.gallery {
            assert_eq!(
                hit_test(
                    &gal_layout,
                    f64::from(gal.quick_look_btn.x + 8),
                    f64::from(gal.quick_look_btn.y + 8)
                ),
                Some(FilesHit::GalleryQuickLookBtn)
            );
            assert_eq!(
                scroll_area_at(
                    &gal_layout,
                    f64::from(gal.strip_rect.x + 12),
                    f64::from(gal.strip_rect.y + 12)
                ),
                Some(ScrollTargetArea::GalleryStrip)
            );
        }

        state.open_go_to_folder();
        let goto_layout = compute_layout(&state, 1060, 660);
        assert!(goto_layout.overlays.go_to_folder.is_some());
        assert_eq!(
            hit_test(&goto_layout, 80.0, 200.0),
            Some(FilesHit::GoToClose)
        );
        if let Some(gt) = &goto_layout.overlays.go_to_folder {
            assert_eq!(
                hit_test(
                    &goto_layout,
                    f64::from(gt.clear_rect.x + 8),
                    f64::from(gt.clear_rect.y + 8)
                ),
                Some(FilesHit::GoToClear)
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn xdg_popup_surface_spec_unclamped_for_outside_window_overflow() {
        let (state, root) = test_state();
        let layout = compute_layout(&state, 1060, 660);
        let menu = OpenPopupMenu::ContextMenu {
            x: 1020,
            y: 620,
            target_entry: None,
            target_dir: Some(root.clone()),
        };
        let spec = compute_popup_surface_spec(&state, &layout, &menu, 8);
        assert_eq!(spec.anchor_x, 1020);
        assert_eq!(spec.anchor_y, 620);
        assert_eq!(spec.shadow_pad, 8);
        assert!(spec.anchor_x + spec.layout.rect.w > layout.width);
        assert!(spec.anchor_y + spec.layout.rect.h > layout.height);
        assert_eq!(spec.surface_width, spec.layout.rect.w + 16);
        assert_eq!(spec.surface_height, spec.layout.rect.h + 16);
        let _ = std::fs::remove_dir_all(&root);
    }
}
