//! Deterministic RustyOS geometry layout and pointer hit-testing for `rusty-files`
//! (`RUSTYOS-065`, ADR-0030). Both rendering and input handling read the same `FilesLayout`.

mod hit_test;
mod popups;
#[cfg(test)]
mod tests;
mod views_and_dialogs;

pub(crate) use hit_test::{hit_test, scroll_area_at};
use popups::compute_popup_layout;
pub(crate) use popups::compute_popup_surface_spec;
#[allow(clippy::wildcard_imports)]
pub(crate) use views_and_dialogs::*;

use std::path::PathBuf;

use rusty_services::file_manager::{
    FileEntry, FileSortField, FileSortOrder, FileTagColor, FileViewMode, SidebarLocation,
    TransferConflictDecision,
};

use crate::state::{
    FilesAppState, LIST_ROW_HEIGHT, MILLER_COLUMN_WIDTH, ROW_HEIGHT, SIDEBAR_ROW_HEIGHT,
    SIDEBAR_SECTION_HEIGHT, SearchScope,
};

/// Unified window corner radius (`14 px`, matching `rusty-settings` and compositor shadows).
#[allow(clippy::cast_possible_wrap)]
pub(crate) const WINDOW_CORNER_RADIUS: i32 = rusty_ui::WINDOW_RADIUS as i32;
/// Edge-to-edge sidebar width (`196 px`).
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
    RunExecutable(PathBuf),
    RunInTerminal(PathBuf),
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
    PickerFilenameInput,
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
    pub picker_filename_rect: Option<Rect>,
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

    let (status_bar_rect, picker_filename_rect, picker_cancel_rect, picker_confirm_rect) =
        if let Some(ref mode) = state.picker_mode {
            let bar = Rect::new(toolbar_x, height - 42, width - toolbar_x - 10, 38);
            let cancel = Rect::new(bar.right() - 172, bar.y + 6, 78, 26);
            let confirm = Rect::new(bar.right() - 86, bar.y + 6, 78, 26);
            let filename_rect =
                matches!(mode, crate::state::FilePickerMode::SaveFile { .. }).then(|| {
                    let box_x = bar.x + 72;
                    let left_limit = cancel.x - 12;
                    let box_w = (left_limit - box_x).clamp(80, 280);
                    Rect::new(box_x, bar.y + 6, box_w, 26)
                });
            (bar, filename_rect, Some(cancel), Some(confirm))
        } else {
            (
                Rect::new(toolbar_x, height - 28, width - toolbar_x - 10, 24),
                None,
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
        picker_filename_rect,
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
