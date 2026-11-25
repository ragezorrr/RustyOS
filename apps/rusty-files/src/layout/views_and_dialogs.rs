//! Tab bar, breadcrumbs, icon grid, gallery, and modal dialog overlay layouts for `rusty-files`.

use std::path::PathBuf;

use rusty_services::file_manager::TransferConflictDecision;

use crate::render::Canvas;
use crate::state::{
    BatchRenameMode, FilesAppState, GALLERY_BOTTOM_H, GALLERY_INFO_WIDTH, GROUP_HEADER_HEIGHT,
    ICON_CELL_H, ICON_CELL_W, TAB_BAR_HEIGHT,
};

#[allow(clippy::wildcard_imports)]
use super::*;

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
    pub group_headers: Vec<(String, Rect)>,
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

/// Computes `Icons` view grid cell positions inside `content_rect` (respecting `Group By` headers).
#[must_use]
pub(crate) fn compute_icons_grid_layout(
    state: &FilesAppState,
    content_rect: Rect,
) -> IconsGridLayout {
    let cols = FilesAppState::icon_grid_columns_for_width(content_rect.w);
    let grid_w = i32::try_from(cols).unwrap_or(1) * ICON_CELL_W;
    let start_x = content_rect.x + ((content_rect.w - grid_w) / 2).max(10);
    let col_opt = state.columns.first();
    let entries_len = col_opt.map_or(0, |c| c.visible_entries.len());
    let group_sections = col_opt.map_or(&[][..], |c| c.group_sections.as_slice());

    let mut group_headers = Vec::new();
    let mut cells = Vec::new();
    let mut cur_y = content_rect.y + 12 - state.icons_scroll_y;
    if group_sections.is_empty() {
        for idx in 0..entries_len {
            let col = idx % cols;
            let row = idx / cols;
            let cx = start_x + i32::try_from(col).unwrap_or(0) * ICON_CELL_W;
            let cy = cur_y + i32::try_from(row).unwrap_or(0) * ICON_CELL_H;
            let rect = Rect::new(cx + 4, cy, ICON_CELL_W - 8, ICON_CELL_H - 6);
            if rect.bottom() >= content_rect.y && rect.y <= content_rect.bottom() {
                let art_rect = Rect::new(rect.x + (rect.w - 72) / 2, rect.y + 4, 72, 66);
                let label_rect = Rect::new(rect.x + 2, rect.y + 72, rect.w - 4, 34);
                cells.push(IconCellLayout {
                    row_idx: idx,
                    rect,
                    art_rect,
                    label_rect,
                });
            }
        }
    } else {
        for (s_i, (start_idx, title)) in group_sections.iter().enumerate() {
            let end_idx = group_sections.get(s_i + 1).map_or(entries_len, |n| n.0);
            let hdr_rect = Rect::new(
                content_rect.x + 12,
                cur_y,
                (content_rect.w - 28).max(120),
                GROUP_HEADER_HEIGHT,
            );
            if hdr_rect.bottom() >= content_rect.y && hdr_rect.y <= content_rect.bottom() {
                group_headers.push((title.clone(), hdr_rect));
            }
            cur_y += GROUP_HEADER_HEIGHT;
            for (local_i, idx) in (*start_idx..end_idx).enumerate() {
                let col = local_i % cols;
                let row = local_i / cols;
                let cx = start_x + i32::try_from(col).unwrap_or(0) * ICON_CELL_W;
                let cy = cur_y + i32::try_from(row).unwrap_or(0) * ICON_CELL_H;
                let rect = Rect::new(cx + 4, cy, ICON_CELL_W - 8, ICON_CELL_H - 6);
                if rect.bottom() >= content_rect.y && rect.y <= content_rect.bottom() {
                    let art_rect = Rect::new(rect.x + (rect.w - 72) / 2, rect.y + 4, 72, 66);
                    let label_rect = Rect::new(rect.x + 2, rect.y + 72, rect.w - 4, 34);
                    cells.push(IconCellLayout {
                        row_idx: idx,
                        rect,
                        art_rect,
                        label_rect,
                    });
                }
            }
            let group_rows =
                i32::try_from(end_idx.saturating_sub(*start_idx).div_ceil(cols)).unwrap_or(0);
            cur_y += group_rows * ICON_CELL_H;
        }
    }

    IconsGridLayout {
        area_rect: content_rect,
        cols,
        group_headers,
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

/// Layout for multi-file `Batch Rename` modal sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameLayout {
    pub modal_rect: Rect,
    pub mode_tabs: [(BatchRenameMode, Rect); 3],
    pub field0_rect: Rect,
    pub field1_rect: Option<Rect>,
    pub prefix_toggle_rect: Option<Rect>,
    pub cancel_rect: Rect,
    pub confirm_rect: Rect,
}

/// Combined active overlay layouts for a frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DialogOverlaysLayout {
    pub quick_look: Option<QuickLookLayout>,
    pub get_info: Option<GetInfoLayout>,
    pub go_to_folder: Option<GoToFolderLayout>,
    pub batch_rename: Option<BatchRenameLayout>,
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

    if let Some(br) = &state.batch_rename {
        out.batch_rename = Some(compute_batch_rename_layout(br, width, height));
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
    if let Some(br) = &ov.batch_rename {
        return Some(hit_test_batch_rename(br, px, py));
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

fn compute_batch_rename_layout(
    br: &crate::state::BatchRenameState,
    width: i32,
    height: i32,
) -> BatchRenameLayout {
    let mw = (width - 80).clamp(320, 440);
    let mh = 228;
    let mx = ((width - mw) / 2).max(16);
    let my = ((height - mh) / 2).max(60);
    let modal_rect = Rect::new(mx, my, mw, mh);
    let tab_w = (mw - 32) / 3;
    let tab_y = my + 40;
    let mode_tabs = [
        (
            BatchRenameMode::ReplaceText,
            Rect::new(mx + 16, tab_y, tab_w - 4, 24),
        ),
        (
            BatchRenameMode::AddText,
            Rect::new(mx + 16 + tab_w, tab_y, tab_w - 4, 24),
        ),
        (
            BatchRenameMode::FormatIndex,
            Rect::new(mx + 16 + tab_w * 2, tab_y, tab_w, 24),
        ),
    ];
    let field0_rect = Rect::new(mx + 16, my + 78, mw - 32, 28);
    let field1_rect =
        (br.mode != BatchRenameMode::AddText).then(|| Rect::new(mx + 16, my + 114, mw - 32, 28));
    let prefix_toggle_rect =
        (br.mode == BatchRenameMode::AddText).then(|| Rect::new(mx + 16, my + 116, 176, 24));
    let btn_y = modal_rect.bottom() - 38;
    let confirm_rect = Rect::new(modal_rect.right() - 96, btn_y, 80, 26);
    let cancel_rect = Rect::new(confirm_rect.x - 82, btn_y, 72, 26);
    BatchRenameLayout {
        modal_rect,
        mode_tabs,
        field0_rect,
        field1_rect,
        prefix_toggle_rect,
        cancel_rect,
        confirm_rect,
    }
}

fn hit_test_batch_rename(br: &BatchRenameLayout, px: f64, py: f64) -> FilesHit {
    if br.cancel_rect.contains_f64(px, py) {
        return FilesHit::BatchRenameCancel;
    }
    if br.confirm_rect.contains_f64(px, py) {
        return FilesHit::BatchRenameConfirm;
    }
    for (mode, rect) in br.mode_tabs {
        if rect.contains_f64(px, py) {
            return FilesHit::BatchRenameMode(mode);
        }
    }
    if br.field0_rect.contains_f64(px, py) {
        return FilesHit::BatchRenameField(0);
    }
    if br.field1_rect.is_some_and(|r| r.contains_f64(px, py)) {
        return FilesHit::BatchRenameField(1);
    }
    if br
        .prefix_toggle_rect
        .is_some_and(|r| r.contains_f64(px, py))
    {
        return FilesHit::BatchRenamePrefixToggle;
    }
    if br.modal_rect.contains_f64(px, py) {
        return FilesHit::BatchRenameModalBody;
    }
    FilesHit::BatchRenameCancel
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
