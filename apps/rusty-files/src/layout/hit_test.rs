//! Window chrome, body, and scroll-area pointer hit-testing for `rusty-files`.

use crate::state::ScrollTargetArea;

#[allow(clippy::wildcard_imports)]
use super::*;

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
    if let Some(input_r) = layout.picker_filename_rect
        && input_r.contains_f64(px, py)
    {
        return Some(FilesHit::PickerFilenameInput);
    }
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
        for (row_idx, rect) in &layout.list_row_chevrons {
            if rect.contains_f64(px, py) {
                return Some(FilesHit::ListRowChevron { row_idx: *row_idx });
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
