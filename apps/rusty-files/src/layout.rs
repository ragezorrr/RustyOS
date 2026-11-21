//! Deterministic RustyOS geometry layout and pointer hit-testing for `rusty-files`
//! (`RUSTYOS-065`, ADR-0030). Both rendering and input handling read the same `FilesLayout`.

use std::path::PathBuf;

use crate::render::Canvas;
use crate::state::{
    GALLERY_BOTTOM_H, GALLERY_INFO_WIDTH, ICON_CELL_H, ICON_CELL_W, TAB_BAR_HEIGHT,
};

use rusty_services::file_manager::{
    FileEntry, FileSortField, FileSortOrder, FileTagColor, FileViewMode, SidebarLocation,
    TransferConflictDecision,
};

use crate::state::{
    FilesAppState, LIST_ROW_HEIGHT, MILLER_COLUMN_WIDTH, OpenPopupMenu, ROW_HEIGHT,
    SIDEBAR_ROW_HEIGHT, SIDEBAR_SECTION_HEIGHT, ScrollTargetArea, SearchScope,
};

/// Unified window corner radius (`14 px`, matching `rusty-settings` and compositor shadows).
#[allow(clippy::cast_possible_wrap)]
pub(crate) const WINDOW_CORNER_RADIUS: i32 = rusty_ui::WINDOW_RADIUS as i32;
/// Edge-to-edge Finder sidebar width (`196 px`).
pub(crate) const SIDEBAR_WIDTH: i32 = 196;
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
    pub rect: Rect,
    pub location: SidebarLocation,
    pub removable_device: Option<String>,
    pub eject_rect: Option<Rect>,
}

/// Clickable collapsible sidebar section header (`Favorites`, `Locations`, `Tags`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidebarSectionHeaderLayout {
    pub title: &'static str,
    pub rect: Rect,
    pub collapsed: bool,
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
    OpenInTerminal(PathBuf),
    ExtractArchive(PathBuf),
    CompressToZip(PathBuf),
    ToggleTabBar,
    TogglePathBar,
    ShareCopyPath(PathBuf),
    ShareCopyUri(PathBuf),
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
    ToggleTag {
        path: PathBuf,
        color: FileTagColor,
    },
    #[allow(dead_code)]
    ZoomWindow,
    #[allow(dead_code)]
    MinimizeWindow,
    #[allow(dead_code)]
    CloseWindow,
}

/// Geometry of the secondary Search Scope strip below the toolbar (`RUSTYOS-071`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchScopeBarLayout {
    pub bar_rect: Rect,
    pub this_folder_rect: Rect,
    pub this_folder_label: String,
    pub home_rect: Rect,
    pub clear_rect: Option<Rect>,
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
    SearchScope(SearchScope),
    SearchScopeClear,
    EmptyTrashBanner,
    SidebarSectionHeader(&'static str),
    SidebarItem(SidebarLocation),
    SidebarRemovableMount(String),
    SidebarRemovableEject(String),
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
    PickerCancel,
    PickerConfirm,
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
    pub sidebar_sections: Vec<SidebarSectionHeaderLayout>,
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
    pub search_scope_bar: Option<SearchScopeBarLayout>,
    pub content_rect: Rect,
    pub columns: Vec<ColumnLayout>,
    pub preview: Option<PreviewLayout>,
    pub list_headers: Vec<(FileSortField, Rect)>,
    pub list_rows: Vec<(usize, Rect)>,
    pub status_bar_rect: Rect,
    pub picker_cancel_rect: Option<Rect>,
    pub picker_confirm_rect: Option<Rect>,
    pub path_crumbs: Vec<PathCrumbLayout>,
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
#[allow(clippy::too_many_lines)]
pub(crate) fn compute_layout(state: &FilesAppState, surface_w: u32, surface_h: u32) -> FilesLayout {
    let width = i32::try_from(surface_w).unwrap_or(1060).max(480);
    let height = i32::try_from(surface_h).unwrap_or(660).max(360);

    let sidebar_rect = Rect::new(0, 0, SIDEBAR_WIDTH, height);
    let sidebar_clip_rect = Rect::new(
        0,
        50,
        SIDEBAR_WIDTH - 1,
        (sidebar_rect.bottom() - 6 - 50).max(40),
    );
    let tl = rusty_ui::TrafficLightButtons::default_csd();
    let btn_d = i32::try_from(tl.close_rect.width).unwrap_or(14);
    let traffic_close = Rect::new(tl.close_rect.x, tl.close_rect.y, btn_d, btn_d);
    let traffic_min = Rect::new(tl.minimize_rect.x, tl.minimize_rect.y, btn_d, btn_d);
    let traffic_max = Rect::new(tl.maximize_rect.x, tl.maximize_rect.y, btn_d, btn_d);

    let (sidebar_rows, sidebar_sections) =
        layout_sidebar_rows(state, sidebar_rect, sidebar_clip_rect);
    let toolbar_x = sidebar_rect.right() + 12;
    let tb = layout_toolbar(state, width, toolbar_x);
    let tab_bar = compute_tab_bar_layout(state, toolbar_x, width);
    let base_top = if tab_bar.is_some() { 88 } else { 58 };
    let search_scope_bar = compute_search_scope_layout(state, toolbar_x, width, base_top);
    let after_search_top = if search_scope_bar.is_some() {
        base_top + 32
    } else {
        base_top
    };

    let (trash_banner_rect, empty_trash_btn_rect, content_top) = if state.is_trash_view() {
        let banner = Rect::new(toolbar_x, after_search_top, width - toolbar_x - 12, 32);
        let btn = Rect::new(banner.right() - 78, after_search_top + 4, 72, 24);
        (Some(banner), Some(btn), after_search_top + 34)
    } else {
        (None, None, after_search_top)
    };

    let (status_bar_rect, picker_cancel_rect, picker_confirm_rect) = if state.picker_mode.is_some()
    {
        let bar = Rect::new(toolbar_x, height - 42, width - toolbar_x - 10, 38);
        let cancel = Rect::new(bar.right() - 172, bar.y + 6, 78, 26);
        let confirm = Rect::new(bar.right() - 86, bar.y + 6, 78, 26);
        (bar, Some(cancel), Some(confirm))
    } else {
        (
            Rect::new(toolbar_x, height - 28, width - toolbar_x - 10, 24),
            None,
            None,
        )
    };
    let status_w = crate::render::Canvas::measure_text(&state.status_summary(), 11.5, false);
    let crumbs_limit_x = (status_bar_rect.right() - status_w - 22).max(status_bar_rect.x + 12);
    let path_crumbs = if state.show_path_bar && state.picker_mode.is_none() {
        compute_path_breadcrumbs_layout(state, status_bar_rect, crumbs_limit_x)
    } else {
        Vec::new()
    };
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
        sidebar_sections,
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
        search_scope_bar,
        content_rect,
        columns: content.columns,
        preview: content.preview,
        list_headers: content.list_headers,
        list_rows: content.list_rows,
        status_bar_rect,
        picker_cancel_rect,
        picker_confirm_rect,
        path_crumbs,
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

fn compute_search_scope_layout(
    state: &FilesAppState,
    toolbar_x: i32,
    width: i32,
    top_y: i32,
) -> Option<SearchScopeBarLayout> {
    if !state.search_active && state.search_query.is_empty() {
        return None;
    }
    let bar_rect = Rect::new(toolbar_x, top_y, (width - toolbar_x - 10).max(220), 28);
    let label_w = crate::render::Canvas::measure_text("Search:", 12.0, false);
    let mut cur_x = bar_rect.x + 8 + label_w + 8;

    let raw_folder = state.search_this_folder_label();
    let folder_label = crate::render::Canvas::fit_text(&raw_folder, 150, 12.0, true);
    let folder_w = (crate::render::Canvas::measure_text(&folder_label, 12.0, true) + 20).max(60);
    let this_folder_rect = Rect::new(cur_x, bar_rect.y + 3, folder_w, 22);
    cur_x += folder_w + 6;

    let home_w = (crate::render::Canvas::measure_text("Home", 12.0, true) + 20).max(56);
    let home_rect = Rect::new(cur_x, bar_rect.y + 3, home_w, 22);

    let clear_rect = if state.search_query.is_empty() {
        None
    } else {
        Some(Rect::new(bar_rect.right() - 60, bar_rect.y + 3, 52, 22))
    };

    Some(SearchScopeBarLayout {
        bar_rect,
        this_folder_rect,
        this_folder_label: folder_label,
        home_rect,
        clear_rect,
    })
}

fn layout_toolbar(state: &FilesAppState, width: i32, toolbar_x: i32) -> ToolbarRects {
    let toolbar_rect = Rect::new(toolbar_x, 10, (width - toolbar_x - 10).max(200), 46);
    let nav_pill_rect = Rect::new(toolbar_x + 4, 15, 60, 32);
    let nav_back_rect = Rect::new(nav_pill_rect.x, nav_pill_rect.y, 30, 32);
    let nav_forward_rect = Rect::new(nav_pill_rect.x + 30, nav_pill_rect.y, 30, 32);
    let title_x = nav_pill_rect.right() + 12;

    let right_margin = width - 16;
    let fixed_cluster_w = 120 + 8 + 42 + 8 + 90 + 8;
    let min_cluster_x = nav_pill_rect.right() + 8;
    let max_search_w = (right_margin - min_cluster_x - fixed_cluster_w).max(32);
    let wide_unfocused_cap = (right_margin - (title_x + 120) - fixed_cluster_w).clamp(32, 164);
    let search_w = if state.search_active || !state.search_query.is_empty() {
        172.min(max_search_w)
    } else if width >= 880 && wide_unfocused_cap >= 120 {
        wide_unfocused_cap
    } else {
        32
    };
    let search_pill_rect = Rect::new(right_margin - search_w, 15, search_w, 32);
    let search_clear_rect = if (state.search_active || !state.search_query.is_empty())
        && !state.search_query.is_empty()
        && search_w >= 72
    {
        Rect::new(search_pill_rect.right() - 24, 21, 18, 20)
    } else {
        Rect::default()
    };

    let action_pill_rect = Rect::new(search_pill_rect.x - 8 - 90, 15, 90, 32);
    let action_primary_rect = Rect::new(action_pill_rect.x, 15, 30, 32);
    let action_tag_rect = Rect::new(action_pill_rect.x + 30, 15, 30, 32);
    let action_more_rect = Rect::new(action_pill_rect.x + 60, 15, 30, 32);

    let sort_pill_rect = Rect::new(action_pill_rect.x - 8 - 42, 15, 42, 32);
    let view_pill_rect = Rect::new(sort_pill_rect.x - 8 - 120, 15, 120, 32);
    let view_segments = [
        (FileViewMode::Icons, Rect::new(view_pill_rect.x, 15, 30, 32)),
        (
            FileViewMode::List,
            Rect::new(view_pill_rect.x + 30, 15, 30, 32),
        ),
        (
            FileViewMode::Columns,
            Rect::new(view_pill_rect.x + 60, 15, 30, 32),
        ),
        (
            FileViewMode::Gallery,
            Rect::new(view_pill_rect.x + 90, 15, 30, 32),
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
) -> (Vec<SidebarRowLayout>, Vec<SidebarSectionHeaderLayout>) {
    let mut rows = Vec::with_capacity(state.sidebar_items.len());
    let mut sections = Vec::with_capacity(4);
    let mut y = 50 - state.sidebar_scroll_y;
    let mut current_section: Option<&'static str> = None;
    for (index, item) in state.sidebar_items.iter().enumerate() {
        if let Some(sec_title) = item.section {
            current_section = Some(sec_title);
            let collapsed = state.collapsed_sidebar_sections.contains(sec_title);
            let hdr_rect = Rect::new(
                sidebar_rect.x + 10,
                y + 4,
                sidebar_rect.w - 20,
                SIDEBAR_SECTION_HEIGHT - 4,
            );
            if hdr_rect.bottom() > clip.y && hdr_rect.y < clip.bottom() {
                sections.push(SidebarSectionHeaderLayout {
                    title: sec_title,
                    rect: hdr_rect,
                    collapsed,
                });
            }
            y += SIDEBAR_SECTION_HEIGHT;
        }
        if current_section.is_some_and(|s| state.collapsed_sidebar_sections.contains(s)) {
            continue;
        }
        let rect = Rect::new(
            sidebar_rect.x + 10,
            y,
            sidebar_rect.w - 20,
            SIDEBAR_ROW_HEIGHT,
        );
        if rect.bottom() > clip.y && rect.y < clip.bottom() {
            let eject_rect = item
                .removable_device
                .as_ref()
                .map(|_| Rect::new(rect.right() - 22, rect.y + 2, 20, rect.h - 4));
            rows.push(SidebarRowLayout {
                index,
                rect,
                location: item.location.clone(),
                removable_device: item.removable_device.clone(),
                eject_rect,
            });
        }
        y += SIDEBAR_ROW_HEIGHT;
    }
    (rows, sections)
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
    let row_h = rusty_ui::menu::ROW_HEIGHT;
    let sep_h = rusty_ui::menu::SEPARATOR_HEIGHT;
    let mut total_h = rusty_ui::menu::VERTICAL_CONTENT_MARGIN * 2;
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
    let row_h = rusty_ui::menu::ROW_HEIGHT;
    let sep_h = rusty_ui::menu::SEPARATOR_HEIGHT;
    let rect = Rect::new(x, y, menu_w, total_h);
    let mut items = Vec::with_capacity(entries.len());
    let mut cy = y + rusty_ui::menu::VERTICAL_CONTENT_MARGIN;
    for (label, checked, swatch_rgb, command) in entries {
        let h = if command.is_some() { row_h } else { sep_h };
        items.push(PopupItemLayout {
            rect: Rect::new(x, cy, menu_w, h),
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
        OpenPopupMenu::ShareMenu => (
            layout.action_primary_rect.x - 16,
            layout.action_primary_rect.bottom() + 6,
            196,
            share_menu_rows(state),
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

fn share_menu_rows(state: &FilesAppState) -> Vec<PopupRowSpec> {
    let mut rows = Vec::new();
    if state.is_trash_view()
        && let Some(entry) = state.selected_entry()
    {
        rows.push((
            String::from("Put Back"),
            false,
            None,
            Some(PopupCommand::RestoreEntry(entry.clone())),
        ));
        rows.push((String::new(), false, None, None));
    }
    if let Some(path) = state.share_target_path() {
        rows.push((
            String::from("Copy Path"),
            false,
            None,
            Some(PopupCommand::ShareCopyPath(path.clone())),
        ));
        rows.push((
            String::from("Copy Link (file://)"),
            false,
            None,
            Some(PopupCommand::ShareCopyUri(path)),
        ));
        if state.selected_entry().is_some() {
            rows.push((
                String::from("Copy Item"),
                false,
                None,
                Some(PopupCommand::CopySelected),
            ));
        }
        rows.push((String::new(), false, None, None));
    }
    rows.push((
        String::from("New Folder"),
        false,
        None,
        Some(PopupCommand::NewFolder),
    ));
    rows
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
        (
            String::from("Show Path Bar"),
            state.show_path_bar,
            None,
            Some(PopupCommand::TogglePathBar),
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
        rusty_services::file_manager::FilesThemeMode::Light => "Dark Theme",
        rusty_services::file_manager::FilesThemeMode::Dark => "Light Theme",
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
        if let Some(dir) = state.active_directory_path() {
            rows.push((
                String::from("Open in Terminal"),
                false,
                None,
                Some(PopupCommand::OpenInTerminal(dir)),
            ));
        }
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
            String::from("Show Path Bar"),
            state.show_path_bar,
            None,
            Some(PopupCommand::TogglePathBar),
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
        rows.push((
            String::from("Open in Terminal"),
            false,
            None,
            Some(PopupCommand::OpenInTerminal(item.path.clone())),
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
        if crate::state::is_extractable_archive(&item.path) {
            rows.push((
                String::from("Extract Here"),
                false,
                None,
                Some(PopupCommand::ExtractArchive(item.path.clone())),
            ));
        }
        rows.push((
            String::from("Compress to .zip"),
            false,
            None,
            Some(PopupCommand::CompressToZip(item.path.clone())),
        ));
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
    if let Some(cancel) = layout.picker_cancel_rect
        && cancel.contains_f64(px, py)
    {
        return Some(FilesHit::PickerCancel);
    }
    if let Some(confirm) = layout.picker_confirm_rect
        && confirm.contains_f64(px, py)
    {
        return Some(FilesHit::PickerConfirm);
    }
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
    if let Some(sb) = &layout.search_scope_bar {
        if sb.clear_rect.is_some_and(|r| r.contains_f64(px, py)) {
            return Some(FilesHit::SearchScopeClear);
        }
        if sb.this_folder_rect.contains_f64(px, py) {
            return Some(FilesHit::SearchScope(SearchScope::ThisFolder));
        }
        if sb.home_rect.contains_f64(px, py) {
            return Some(FilesHit::SearchScope(SearchScope::Home));
        }
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
    if layout.toolbar_rect.contains_f64(px, py)
        || (layout.sidebar_rect.contains_f64(px, py) && py < 50.0)
    {
        return Some(FilesHit::WindowDrag);
    }
    None
}

fn hit_test_body(layout: &FilesLayout, px: f64, py: f64) -> Option<FilesHit> {
    if layout.sidebar_clip_rect.contains_f64(px, py) {
        for sec in &layout.sidebar_sections {
            if sec.rect.contains_f64(px, py) {
                return Some(FilesHit::SidebarSectionHeader(sec.title));
            }
        }
        for row in &layout.sidebar_rows {
            if let Some(eject_r) = row.eject_rect
                && eject_r.contains_f64(px, py)
                && let Some(dev) = &row.removable_device
            {
                return Some(FilesHit::SidebarRemovableEject(dev.clone()));
            }
            if row.rect.contains_f64(px, py) {
                if let Some(dev) = &row.removable_device {
                    return Some(FilesHit::SidebarRemovableMount(dev.clone()));
                }
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

// --- Stage B View Layouts (Tabs, Breadcrumbs, Icons Grid, Gallery) ---

/// Single open tab capsule geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabItemLayout {
    pub index: usize,
    pub rect: Rect,
    pub close_rect: Option<Rect>,
    pub title: String,
    pub active: bool,
}

/// Top tab strip geometry beneath toolbar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabBarLayout {
    pub bar_rect: Rect,
    pub track_rect: Rect,
    pub add_rect: Rect,
    pub tabs: Vec<TabItemLayout>,
}

/// Clickable directory breadcrumb pill in bottom `Path Bar`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathCrumbLayout {
    pub label: String,
    pub path: PathBuf,
    pub rect: Rect,
}

/// Geometry of a single card inside `Icons` grid view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IconCellLayout {
    pub row_idx: usize,
    pub rect: Rect,
    pub art_rect: Rect,
    pub label_rect: Rect,
}

/// Complete computed `Icons` view geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IconsGridLayout {
    pub area_rect: Rect,
    pub cols: usize,
    pub cells: Vec<IconCellLayout>,
}

/// Single filmstrip thumbnail slot in `Gallery` mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GalleryThumbLayout {
    pub row_idx: usize,
    pub rect: Rect,
}

/// Complete computed `Gallery` mode geometry (`stage` + `filmstrip` + `Information` pane).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GalleryLayout {
    pub area_rect: Rect,
    pub stage_rect: Rect,
    pub strip_rect: Rect,
    pub info_rect: Rect,
    pub show_more_rect: Rect,
    pub quick_look_btn: Rect,
    pub get_info_btn: Rect,
    pub open_btn: Rect,
    pub trash_btn: Rect,
    pub thumbs: Vec<GalleryThumbLayout>,
    pub info_tags: Vec<(FileTagColor, Rect)>,
    pub selected_entry: Option<FileEntry>,
}

/// Computes the RustyOS segmented `Tabs` bar beneath the toolbar when visible.
#[must_use]
pub(crate) fn compute_tab_bar_layout(
    state: &FilesAppState,
    toolbar_x: i32,
    width: i32,
) -> Option<TabBarLayout> {
    if !state.tab_bar_visible() {
        return None;
    }
    let items = state.tab_titles();
    if items.is_empty() {
        return None;
    }
    let bar_rect = Rect::new(
        toolbar_x,
        54,
        (width - toolbar_x - 10).max(180),
        TAB_BAR_HEIGHT,
    );
    let add_rect = Rect::new(bar_rect.right() - 26, bar_rect.y + 3, 24, 24);
    let track_rect = Rect::new(
        bar_rect.x + 2,
        bar_rect.y + 3,
        (add_rect.x - bar_rect.x - 10).max(80),
        24,
    );

    let count = i32::try_from(items.len()).unwrap_or(1).max(1);
    let inner_w = (track_rect.w - 4).max(40);
    let base_w = inner_w / count;
    let rem = inner_w % count;

    let mut cur_x = track_rect.x + 2;
    let mut tabs = Vec::with_capacity(items.len());
    let multi = items.len() > 1;
    for (idx, (tab_idx, title, active)) in items.into_iter().enumerate() {
        let extra = i32::from(i32::try_from(idx).unwrap_or(0) < rem);
        let tab_w = (base_w + extra).max(24);
        let rect = Rect::new(cur_x, track_rect.y + 2, tab_w, 20);
        let close_rect = if multi && tab_w >= 46 {
            Some(Rect::new(rect.right() - 18, rect.y + 2, 16, 16))
        } else {
            None
        };
        tabs.push(TabItemLayout {
            index: tab_idx,
            rect,
            close_rect,
            title,
            active,
        });
        cur_x += tab_w;
    }

    Some(TabBarLayout {
        bar_rect,
        track_rect,
        add_rect,
        tabs,
    })
}

/// Computes clickable breadcrumb rectangles inside the bottom status bar.
#[must_use]
pub(crate) fn compute_path_breadcrumbs_layout(
    state: &FilesAppState,
    status_bar: Rect,
    right_limit_x: i32,
) -> Vec<PathCrumbLayout> {
    let crumbs = state.path_breadcrumbs();
    if crumbs.is_empty() {
        return Vec::new();
    }
    let avail_right = right_limit_x.max(status_bar.x + 12);
    let mut cursor_x = status_bar.x + 8;
    let mut out = Vec::new();
    let len = crumbs.len();
    for (i, (label, path)) in crumbs.into_iter().enumerate() {
        let max_seg_w = if i + 1 == len {
            (avail_right - cursor_x).max(0)
        } else {
            130
        };
        if max_seg_w < 26 {
            break;
        }
        let fitted = Canvas::fit_text(&label, max_seg_w - 18, 11.5, i + 1 == len);
        if fitted.is_empty() {
            continue;
        }
        let text_w = Canvas::measure_text(&fitted, 11.5, i + 1 == len);
        let pill_w = (text_w + 16).min(avail_right - cursor_x);
        if pill_w < 22 {
            break;
        }
        out.push(PathCrumbLayout {
            label: fitted,
            path,
            rect: Rect::new(cursor_x, status_bar.y + 3, pill_w, 19),
        });
        cursor_x += pill_w + 14;
        if cursor_x + 24 >= avail_right {
            break;
        }
    }
    out
}

/// Computes `Icons` view grid cell positions inside `content_rect`.
#[must_use]
pub(crate) fn compute_icons_grid_layout(
    state: &FilesAppState,
    content_rect: Rect,
) -> IconsGridLayout {
    let cols = FilesAppState::icon_grid_columns_for_width(content_rect.w);
    let grid_w = i32::try_from(cols).unwrap_or(1) * ICON_CELL_W;
    let start_x = content_rect.x + ((content_rect.w - grid_w) / 2).max(10);
    let entries_len = state.columns.first().map_or(0, |c| c.visible_entries.len());

    let mut cells = Vec::new();
    for idx in 0..entries_len {
        let col = idx % cols;
        let row = idx / cols;
        let cx = start_x + i32::try_from(col).unwrap_or(0) * ICON_CELL_W;
        let cy = content_rect.y + 12 + i32::try_from(row).unwrap_or(0) * ICON_CELL_H
            - state.icons_scroll_y;
        let rect = Rect::new(cx + 4, cy, ICON_CELL_W - 8, ICON_CELL_H - 6);
        if rect.bottom() < content_rect.y || rect.y > content_rect.bottom() {
            continue;
        }
        let art_rect = Rect::new(rect.x + (rect.w - 72) / 2, rect.y + 4, 72, 66);
        let label_rect = Rect::new(rect.x + 2, rect.y + 72, rect.w - 4, 34);
        cells.push(IconCellLayout {
            row_idx: idx,
            rect,
            art_rect,
            label_rect,
        });
    }

    IconsGridLayout {
        area_rect: content_rect,
        cols,
        cells,
    }
}

/// Computes `Gallery` view stage, filmstrip thumbnails, and right `Information` panel geometry.
#[must_use]
pub(crate) fn compute_gallery_layout(state: &FilesAppState, content_rect: Rect) -> GalleryLayout {
    let info_w = GALLERY_INFO_WIDTH.min((content_rect.w / 3).max(200));
    let left_w = (content_rect.w - info_w).max(160);
    let strip_h = GALLERY_BOTTOM_H.min((content_rect.h / 3).max(74));
    let stage_h = (content_rect.h - strip_h).max(80);

    let stage_rect = Rect::new(content_rect.x, content_rect.y, left_w, stage_h);
    let strip_rect = Rect::new(content_rect.x, stage_rect.bottom(), left_w, strip_h);
    let info_rect = Rect::new(
        stage_rect.right(),
        content_rect.y,
        content_rect.right() - stage_rect.right(),
        content_rect.h,
    );

    let entries = state
        .columns
        .first()
        .map_or(&[][..], |c| c.visible_entries.as_slice());
    let mut thumbs = Vec::new();
    let card_sz = 56;
    let card_step = 66;
    let mut tx = strip_rect.x + 14 - state.gallery_strip_scroll_x;
    let ty = strip_rect.y + ((strip_rect.h - card_sz) / 2).max(6);
    for idx in 0..entries.len() {
        let r = Rect::new(tx, ty, card_sz, card_sz);
        if r.right() >= strip_rect.x && r.x <= strip_rect.right() {
            thumbs.push(GalleryThumbLayout {
                row_idx: idx,
                rect: r,
            });
        }
        tx += card_step;
    }

    let selected_entry = state
        .columns
        .first()
        .and_then(|c| c.selected_entry().cloned());

    let show_more_rect = Rect::new(info_rect.right() - 78, info_rect.y + 72, 66, 20);
    let tags_top = if state.gallery_more_info {
        info_rect.y + 236
    } else {
        info_rect.y + 188
    };
    let mut info_tags = Vec::with_capacity(FileTagColor::ALL.len());
    for (i, color) in FileTagColor::ALL.iter().enumerate() {
        let dx = info_rect.x + 16 + i32::try_from(i).unwrap_or(0) * 28;
        info_tags.push((*color, Rect::new(dx, tags_top, 20, 20)));
    }

    let action_y = info_rect.bottom() - 48;
    let btn_w = ((info_rect.w - 32) / 4).max(36);
    let quick_look_btn = Rect::new(info_rect.x + 10, action_y, btn_w, 36);
    let get_info_btn = Rect::new(quick_look_btn.right() + 4, action_y, btn_w, 36);
    let open_btn = Rect::new(get_info_btn.right() + 4, action_y, btn_w, 36);
    let trash_btn = Rect::new(open_btn.right() + 4, action_y, btn_w, 36);

    GalleryLayout {
        area_rect: content_rect,
        stage_rect,
        strip_rect,
        info_rect,
        show_more_rect,
        quick_look_btn,
        get_info_btn,
        open_btn,
        trash_btn,
        thumbs,
        info_tags,
        selected_entry,
    }
}

/// Hit-tests top `Tabs` bar chrome controls.
#[must_use]
pub(crate) fn hit_test_tab_bar(tb: &TabBarLayout, px: f64, py: f64) -> Option<FilesHit> {
    if !tb.bar_rect.contains_f64(px, py) {
        return None;
    }
    if tb.add_rect.contains_f64(px, py) {
        return Some(FilesHit::TabAdd);
    }
    for tab in &tb.tabs {
        if let Some(close) = tab.close_rect
            && close.contains_f64(px, py)
        {
            return Some(FilesHit::TabClose(tab.index));
        }
        if tab.rect.contains_f64(px, py) {
            return Some(FilesHit::TabSelect(tab.index));
        }
    }
    Some(FilesHit::WindowDrag)
}

/// Hit-tests clickable `Path Bar` breadcrumb directories.
#[must_use]
pub(crate) fn hit_test_path_bar(crumbs: &[PathCrumbLayout], px: f64, py: f64) -> Option<FilesHit> {
    crumbs
        .iter()
        .find(|c| c.rect.contains_f64(px, py))
        .map(|c| FilesHit::PathCrumb(c.path.clone()))
}

/// Hit-tests `Icons` view cells.
#[must_use]
pub(crate) fn hit_test_icons_grid(grid: &IconsGridLayout, px: f64, py: f64) -> Option<FilesHit> {
    if !grid.area_rect.contains_f64(px, py) {
        return None;
    }
    for cell in &grid.cells {
        if cell.rect.contains_f64(px, py) {
            return Some(FilesHit::IconCell { idx: cell.row_idx });
        }
    }
    Some(FilesHit::IconsEmpty)
}

/// Hit-tests `Gallery` view stage, filmstrip, inspector `Show More`, tags, and bottom actions.
#[must_use]
pub(crate) fn hit_test_gallery(gal: &GalleryLayout, px: f64, py: f64) -> Option<FilesHit> {
    if !gal.area_rect.contains_f64(px, py) {
        return None;
    }
    for thumb in &gal.thumbs {
        if thumb.rect.contains_f64(px, py) {
            return Some(FilesHit::GalleryThumb { idx: thumb.row_idx });
        }
    }
    if gal.info_rect.contains_f64(px, py) {
        if gal.show_more_rect.contains_f64(px, py) {
            return Some(FilesHit::GalleryShowMoreInfo);
        }
        if gal.quick_look_btn.contains_f64(px, py) {
            return Some(FilesHit::GalleryQuickLookBtn);
        }
        if gal.get_info_btn.contains_f64(px, py) {
            return Some(FilesHit::GalleryInfoBtn);
        }
        if gal.open_btn.contains_f64(px, py) {
            return Some(FilesHit::GalleryOpenBtn);
        }
        if gal.trash_btn.contains_f64(px, py) {
            return Some(FilesHit::GalleryTrashBtn);
        }
        if let Some(entry) = &gal.selected_entry {
            for (color, rect) in &gal.info_tags {
                if rect.contains_f64(px, py) {
                    return Some(FilesHit::GalleryTag {
                        path: entry.path.clone(),
                        color: *color,
                    });
                }
            }
        }
        return None;
    }
    if gal.stage_rect.contains_f64(px, py) && gal.selected_entry.is_some() {
        return Some(FilesHit::GalleryStageOpen);
    }
    None
}

// --- Dialog & Overlay Layouts (Quick Look, Get Info, Go To, Transfer) ---

/// Layout for `Quick Look` preview modal (`Space`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct QuickLookLayout {
    pub modal_rect: Rect,
    pub close_rect: Rect,
    pub prev_rect: Rect,
    pub next_rect: Rect,
    pub open_rect: Rect,
    pub stage_rect: Rect,
}

/// Layout for `Get Info` sheet (`Ctrl+I`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GetInfoLayout {
    pub modal_rect: Rect,
    pub close_rect: Rect,
    pub more_toggle_rect: Rect,
    pub tags: Vec<(FileTagColor, Rect)>,
    pub preview_rect: Rect,
}

/// Layout for `Go to Folder...` sheet (`Ctrl+Shift+G`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoToFolderLayout {
    pub modal_rect: Rect,
    pub input_rect: Rect,
    pub clear_rect: Rect,
    pub suggestions: Vec<(usize, Rect)>,
}

/// Layout for background Copy/Move progress pill card.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct TransferDialogLayout {
    pub card_rect: Rect,
    pub bar_rect: Rect,
    pub stop_rect: Rect,
}

/// Layout for conflict resolution prompt (`Keep Both`, `Skip`, `Stop`, `Replace`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct ConflictDialogLayout {
    pub modal_rect: Rect,
    pub keep_both_rect: Rect,
    pub skip_rect: Rect,
    pub stop_rect: Rect,
    pub replace_rect: Rect,
}

/// Combined active overlay layouts for a frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DialogOverlaysLayout {
    pub quick_look: Option<QuickLookLayout>,
    pub get_info: Option<GetInfoLayout>,
    pub go_to_folder: Option<GoToFolderLayout>,
    pub transfer: Option<TransferDialogLayout>,
    pub conflict: Option<ConflictDialogLayout>,
    pub inline_rename_rect: Option<Rect>,
}

/// Computes active dialog overlay geometries within a `width x height` window.
#[must_use]
pub(crate) fn compute_dialog_overlays_layout(
    state: &FilesAppState,
    width: i32,
    height: i32,
    inline_anchor: Option<Rect>,
) -> DialogOverlaysLayout {
    let mut out = DialogOverlaysLayout {
        inline_rename_rect: inline_anchor,
        ..Default::default()
    };

    if state.quick_look.is_some() {
        let mw = (width - 80).clamp(320, 760);
        let mh = (height - 68).clamp(240, 520);
        let mx = (width - mw) / 2;
        let my = (height - mh) / 2;
        let modal_rect = Rect::new(mx, my, mw, mh);
        out.quick_look = Some(QuickLookLayout {
            close_rect: Rect::new(mx + 12, my + 10, 20, 20),
            prev_rect: Rect::new(mx + 42, my + 10, 22, 20),
            next_rect: Rect::new(mx + 66, my + 10, 22, 20),
            open_rect: Rect::new(modal_rect.right() - 78, my + 8, 66, 24),
            stage_rect: Rect::new(mx + 14, my + 44, mw - 28, mh - 58),
            modal_rect,
        });
    }

    if state.get_info.is_some() {
        let mw = 304.min(width - 40);
        let mh = 436.min(height - 36);
        let mx = (width - mw - 28).max(20);
        let my = ((height - mh) / 2).max(18);
        let modal_rect = Rect::new(mx, my, mw, mh);
        let tags_y = my + 98;
        let mut tags = Vec::with_capacity(FileTagColor::ALL.len());
        for (i, color) in FileTagColor::ALL.iter().enumerate() {
            let tx = mx + 58 + i32::try_from(i).unwrap_or(0) * 30;
            tags.push((*color, Rect::new(tx, tags_y, 22, 22)));
        }
        out.get_info = Some(GetInfoLayout {
            close_rect: Rect::new(mx + 12, my + 10, 18, 18),
            more_toggle_rect: Rect::new(mx + mw - 72, my + 242, 58, 18),
            tags,
            preview_rect: Rect::new(mx + 18, my + mh - 98, mw - 36, 84),
            modal_rect,
        });
    }

    if let Some(gt) = &state.go_to_folder {
        let mw = (width - 96).clamp(280, 440);
        let sugg_count = i32::try_from(gt.suggestions.len()).unwrap_or(0);
        let err_h = if gt.error.is_some() { 22 } else { 0 };
        let mh = 80 + err_h + sugg_count * 28;
        let mx = ((width - mw) / 2).max(16);
        let my = 62;
        let modal_rect = Rect::new(mx, my, mw, mh);
        let input_rect = Rect::new(mx + 12, my + 10, mw - 24, 30);
        let clear_rect = Rect::new(input_rect.right() - 24, input_rect.y + 5, 20, 20);
        let mut suggestions = Vec::with_capacity(gt.suggestions.len());
        let mut sy = my + 68 + err_h;
        for idx in 0..gt.suggestions.len() {
            suggestions.push((idx, Rect::new(mx + 10, sy, mw - 20, 26)));
            sy += 28;
        }
        out.go_to_folder = Some(GoToFolderLayout {
            modal_rect,
            input_rect,
            clear_rect,
            suggestions,
        });
    }

    if let Some(tr) = &state.active_transfer {
        let cw = 360.min(width - 40);
        let cx = (width - cw) / 2;
        let cy = 62;
        let card_rect = Rect::new(cx, cy, cw, 74);
        let bar_rect = Rect::new(cx + 16, cy + 36, cw - 62, 6);
        let stop_rect = Rect::new(bar_rect.right() + 14, cy + 29, 20, 20);
        out.transfer = Some(TransferDialogLayout {
            card_rect,
            bar_rect,
            stop_rect,
        });
        if tr.conflict.is_some() {
            let mw = 420.min(width - 40);
            let mh = 152;
            let mx = (width - mw) / 2;
            let my = ((height - mh) / 2).max(60);
            let modal_rect = Rect::new(mx, my, mw, mh);
            let btn_y = modal_rect.bottom() - 38;
            let replace_rect = Rect::new(modal_rect.right() - 88, btn_y, 74, 26);
            let stop_btn = Rect::new(replace_rect.x - 68, btn_y, 60, 26);
            let skip_rect = Rect::new(stop_btn.x - 66, btn_y, 58, 26);
            let keep_both_rect = Rect::new(skip_rect.x - 96, btn_y, 88, 26);
            out.conflict = Some(ConflictDialogLayout {
                modal_rect,
                keep_both_rect,
                skip_rect,
                stop_rect: stop_btn,
                replace_rect,
            });
        }
    }

    out
}

/// Hit-tests active modal overlays in strict front-to-back stacking order.
#[must_use]
pub(crate) fn hit_test_dialog_overlays(
    ov: &DialogOverlaysLayout,
    px: f64,
    py: f64,
) -> Option<FilesHit> {
    if let Some(cf) = &ov.conflict {
        if cf.replace_rect.contains_f64(px, py) {
            return Some(FilesHit::ConflictDecision(
                TransferConflictDecision::Replace,
            ));
        }
        if cf.keep_both_rect.contains_f64(px, py) {
            return Some(FilesHit::ConflictDecision(
                TransferConflictDecision::KeepBoth,
            ));
        }
        if cf.skip_rect.contains_f64(px, py) {
            return Some(FilesHit::ConflictDecision(TransferConflictDecision::Skip));
        }
        if cf.stop_rect.contains_f64(px, py) {
            return Some(FilesHit::ConflictDecision(TransferConflictDecision::Stop));
        }
        return Some(
            FilesHit::ConflictDecision(TransferConflictDecision::Stop)
                .take_or_block(cf.modal_rect.contains_f64(px, py)),
        );
    }
    if let Some(gt) = &ov.go_to_folder {
        if !gt.modal_rect.contains_f64(px, py) {
            return Some(FilesHit::GoToClose);
        }
        if gt.clear_rect.contains_f64(px, py) {
            return Some(FilesHit::GoToClear);
        }
        for (idx, rect) in &gt.suggestions {
            if rect.contains_f64(px, py) {
                return Some(FilesHit::GoToSuggestion(*idx));
            }
        }
        return Some(FilesHit::GoToModalBody);
    }
    if let Some(gi) = &ov.get_info {
        if gi.close_rect.contains_f64(px, py) {
            return Some(FilesHit::GetInfoClose);
        }
        if gi.modal_rect.contains_f64(px, py) {
            if gi.more_toggle_rect.contains_f64(px, py) {
                return Some(FilesHit::GetInfoMoreToggle);
            }
            for (color, rect) in &gi.tags {
                if rect.contains_f64(px, py) {
                    return Some(FilesHit::GetInfoTag {
                        path: PathBuf::new(),
                        color: *color,
                    });
                }
            }
            return Some(FilesHit::GetInfoModalBody);
        }
    }
    if let Some(ql) = &ov.quick_look {
        if !ql.modal_rect.contains_f64(px, py) || ql.close_rect.contains_f64(px, py) {
            return Some(FilesHit::QuickLookClose);
        }
        if ql.prev_rect.contains_f64(px, py) {
            return Some(FilesHit::QuickLookPrev);
        }
        if ql.next_rect.contains_f64(px, py) {
            return Some(FilesHit::QuickLookNext);
        }
        if ql.open_rect.contains_f64(px, py) {
            return Some(FilesHit::QuickLookOpen);
        }
        return Some(FilesHit::QuickLookModalBody);
    }
    if let Some(tr) = &ov.transfer
        && tr.stop_rect.contains_f64(px, py)
    {
        return Some(FilesHit::TransferStop);
    }
    if let Some(rn_rect) = ov.inline_rename_rect
        && rn_rect.contains_f64(px, py)
    {
        return Some(FilesHit::InlineRenameBox);
    }
    None
}

trait ConflictHitExt {
    fn take_or_block(self, inside: bool) -> FilesHit;
}
impl ConflictHitExt for FilesHit {
    fn take_or_block(self, inside: bool) -> FilesHit {
        if inside {
            FilesHit::GetInfoModalBody
        } else {
            self
        }
    }
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
    fn rustyos_layout_and_hit_testing_work() {
        let (mut state, root) = test_state();
        let layout = compute_layout(&state, 1060, 660);

        assert_eq!(layout.sidebar_rect, Rect::new(0, 0, SIDEBAR_WIDTH, 660));
        assert_eq!(hit_test(&layout, 25.0, 25.0), Some(FilesHit::TrafficClose));
        assert_eq!(
            hit_test(&layout, 45.0, 25.0),
            Some(FilesHit::TrafficMinimize)
        );
        assert_eq!(
            hit_test(&layout, 65.0, 25.0),
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
        let default_layout = compute_layout(&state, 1060, 660);
        assert!(default_layout.icons_grid.is_some());
        assert_eq!(default_layout.path_crumbs.len(), 0);
        assert!(default_layout.search_pill_rect.w >= 120);
        let narrow_unfocused = compute_layout(&state, 768, 461);
        assert_eq!(narrow_unfocused.search_pill_rect.w, 32);

        state.toggle_path_bar();
        let icons_layout = compute_layout(&state, 1060, 660);
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

        state.search_active = true;
        state.set_search_query(String::from("doc"));
        let search_layout = compute_layout(&state, 1060, 660);
        if let Some(sb) = &search_layout.search_scope_bar {
            assert_eq!(
                hit_test(
                    &search_layout,
                    f64::from(sb.home_rect.x + 8),
                    f64::from(sb.home_rect.y + 8)
                ),
                Some(FilesHit::SearchScope(SearchScope::Home))
            );
        }
        state.search_active = false;
        state.set_search_query(String::new());

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

    #[test]
    fn search_scope_clear_and_share_button_hit_test() {
        let (mut state, root) = test_state();
        let idle = compute_layout(&state, 768, 461);
        assert_eq!(
            hit_test(
                &idle,
                f64::from(idle.action_primary_rect.x + 12),
                f64::from(idle.action_primary_rect.y + 16),
            ),
            Some(FilesHit::ActionPrimary)
        );

        state.search_active = true;
        state.set_search_query(String::from("doc"));
        let search_1060 = compute_layout(&state, 1060, 660);
        assert!(search_1060.search_scope_bar.is_some());
        if let Some(sb) = &search_1060.search_scope_bar {
            assert!(sb.clear_rect.is_some());
            if let Some(clear) = sb.clear_rect {
                assert_eq!(
                    hit_test(
                        &search_1060,
                        f64::from(clear.x + 10),
                        f64::from(clear.y + 8)
                    ),
                    Some(FilesHit::SearchScopeClear)
                );
            }
        }
        let search_768 = compute_layout(&state, 768, 461);
        assert_eq!(
            hit_test(&search_768, 768.0 - 36.0, 72.0),
            Some(FilesHit::SearchScopeClear)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
