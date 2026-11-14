//! Stage B Golden Gate views (`Icons` grid, `Gallery` + `Information` inspector,
//! top `Tabs` bar, and clickable bottom `Path Bar`) (`RUSTYOS-066`).

use std::path::PathBuf;

use rusty_services::file_manager::{
    FileDetailedInfo, FileEntry, FileTagColor, format_byte_size, format_unix_timestamp,
};

use crate::icons::{
    draw_close_x_glyph, draw_entry_icon, draw_large_entry_artwork, draw_plus_glyph,
};
use crate::layout::{FilesHit, Rect};
use crate::render::{Canvas, Palette, rgba};
use crate::state::{
    CachedPreview, FilesAppState, GALLERY_BOTTOM_H, GALLERY_INFO_WIDTH, ICON_CELL_H, ICON_CELL_W,
    TAB_BAR_HEIGHT,
};

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

/// Computes the Golden Gate segmented `Tabs` bar beneath the toolbar when visible.
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

/// Renders the Golden Gate segmented `Tabs` bar beneath the toolbar.
pub(crate) fn render_tab_bar(c: &mut Canvas<'_>, tb: &TabBarLayout, pal: &Palette) {
    let track_bg = if pal.is_dark {
        rgba(38, 29, 54, 255)
    } else {
        rgba(232, 235, 241, 255)
    };
    c.fill_rounded_rect(tb.track_rect, 12, track_bg);

    for (idx, tab) in tb.tabs.iter().enumerate() {
        if idx > 0 && !tab.active && !tb.tabs[idx - 1].active {
            c.fill_rect(
                Rect::new(tab.rect.x, tab.rect.y + 4, 1, tab.rect.h - 8),
                pal.divider,
            );
        }
        if tab.active {
            let active_fill = if pal.is_dark {
                pal.pill_active
            } else {
                rgba(255, 255, 255, 255)
            };
            c.fill_rounded_rect(tab.rect, 10, active_fill);
            c.stroke_rounded_rect(tab.rect, 10, pal.pill_border);
        }
        let max_label_w = if tab.close_rect.is_some() {
            (tab.rect.w - 28).max(16)
        } else {
            (tab.rect.w - 16).max(16)
        };
        let label = Canvas::fit_text(&tab.title, max_label_w, 12.0, tab.active);
        let tw = Canvas::measure_text(&label, 12.0, tab.active);
        let tx = tab.rect.x + (tab.rect.w - tw) / 2;
        c.draw_text(
            &label,
            tx,
            tab.rect.y + 4,
            12.0,
            tab.active,
            if tab.active {
                pal.text_primary
            } else {
                pal.text_secondary
            },
            tab.rect,
        );
        if let Some(close) = tab.close_rect {
            #[allow(clippy::cast_precision_loss)]
            draw_close_x_glyph(
                c,
                close.x as f32 + 8.0,
                close.y as f32 + 8.0,
                3.2,
                pal.text_muted,
            );
        }
    }

    c.fill_rounded_rect(tb.add_rect, 12, pal.pill_bg);
    c.stroke_rounded_rect(tb.add_rect, 12, pal.pill_border);
    #[allow(clippy::cast_precision_loss)]
    draw_plus_glyph(
        c,
        tb.add_rect.x as f32 + 12.0,
        tb.add_rect.y as f32 + 12.0,
        4.2,
        pal.text_primary,
    );
}

/// Renders clickable bottom `Path Bar` breadcrumb directories inside the status strip.
pub(crate) fn render_path_bar(c: &mut Canvas<'_>, crumbs: &[PathCrumbLayout], pal: &Palette) {
    let last_idx = crumbs.len().saturating_sub(1);
    for (idx, crumb) in crumbs.iter().enumerate() {
        let is_last = idx == last_idx;
        if is_last {
            c.fill_rounded_rect(crumb.rect, 6, pal.row_alt);
        }
        // Mini Golden Gate folder glyph before label
        let icon_col = if pal.is_dark {
            rgba(148, 134, 248, 255)
        } else {
            rgba(56, 152, 246, 255)
        };
        #[allow(clippy::cast_precision_loss)]
        {
            c.fill_rounded_rect_f(
                crumb.rect.x as f32 + 4.0,
                crumb.rect.y as f32 + 5.0,
                9.0,
                8.0,
                1.5,
                icon_col,
            );
        }
        c.draw_text(
            &crumb.label,
            crumb.rect.x + 16,
            crumb.rect.y + 4,
            11.0,
            is_last,
            if is_last {
                pal.text_primary
            } else {
                pal.text_secondary
            },
            crumb.rect,
        );
        if !is_last {
            #[allow(clippy::cast_precision_loss)]
            let cx = crumb.rect.right() as f32 + 6.5;
            #[allow(clippy::cast_precision_loss)]
            let cy = crumb.rect.y as f32 + 9.5;
            c.stroke_polyline_aa(
                &[(cx - 2.0, cy - 3.0), (cx + 1.5, cy), (cx - 2.0, cy + 3.0)],
                false,
                1.15,
                pal.text_muted,
            );
        }
    }
}

/// Renders the Stage B `Icons` grid view with 64 px SSAA folders, thumbnails, tag dots, and wrap labels.
pub(crate) fn render_icons_grid(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    grid: &IconsGridLayout,
    pal: &Palette,
) {
    let clip = grid.area_rect;
    let Some(col) = state.columns.first() else {
        return;
    };
    c.with_clip(clip, |c| {
        if col.loading {
            c.draw_text(
                "Loading...",
                clip.x + 24,
                clip.y + 24,
                13.0,
                false,
                pal.text_muted,
                clip,
            );
            return;
        }
        if col.visible_entries.is_empty() {
            c.draw_text(
                "This folder is empty",
                clip.x + (clip.w - 130) / 2,
                clip.y + 64,
                13.0,
                false,
                pal.text_muted,
                clip,
            );
            return;
        }
        for cell in &grid.cells {
            let Some(entry) = col.visible_entries.get(cell.row_idx) else {
                continue;
            };
            let selected = col.selected_index == Some(cell.row_idx);
            let tags = state.tags_for(&entry.path);
            render_single_icon_cell(c, state, cell, entry, tags, selected, pal);
        }
        let rows = i32::try_from(col.visible_entries.len().div_ceil(grid.cols.max(1))).unwrap_or(0);
        c.draw_scrollbar(
            clip,
            state.icons_scroll_y,
            rows * ICON_CELL_H + 16,
            pal.scrollbar,
        );
    });
}

fn render_single_icon_cell(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    cell: &IconCellLayout,
    entry: &FileEntry,
    tags: &[FileTagColor],
    selected: bool,
    pal: &Palette,
) {
    if selected {
        let art_hl = Rect::new(
            cell.art_rect.x - 4,
            cell.art_rect.y - 2,
            cell.art_rect.w + 8,
            cell.art_rect.h + 2,
        );
        c.fill_rounded_rect(art_hl, 8, pal.row_alt);
        c.stroke_rounded_rect(art_hl, 8, pal.divider);
    }

    if let Some(CachedPreview::Image(img)) = state.cached_preview_for(entry, 96, 96) {
        draw_framed_thumbnail(c, img, cell.art_rect, pal);
    } else {
        #[allow(clippy::cast_precision_loss)]
        draw_large_entry_artwork(
            c,
            cell.art_rect.x as f32 + cell.art_rect.w as f32 * 0.5,
            cell.art_rect.y as f32 + cell.art_rect.h as f32 * 0.52,
            58.0,
            entry,
            tags.first().copied(),
            pal,
        );
    }

    // Tag dot + centered 2-line wrapped label
    let lines = wrap_two_lines(
        &entry.name,
        cell.label_rect.w - if tags.is_empty() { 8 } else { 22 },
        12.0,
        selected,
    );
    let first_line = &lines.0;
    let l1_w = Canvas::measure_text(first_line, 12.0, selected);
    let dot_space = if tags.is_empty() { 0 } else { 12 };
    let total_l1_w = (l1_w + dot_space).min(cell.label_rect.w - 4);

    if selected {
        let pill_w = (total_l1_w + 12).min(cell.label_rect.w);
        let pill = Rect::new(
            cell.label_rect.x + (cell.label_rect.w - pill_w) / 2,
            cell.label_rect.y,
            pill_w,
            18,
        );
        c.fill_rounded_rect(pill, 5, pal.accent);
    }

    let start_x = cell.label_rect.x + (cell.label_rect.w - total_l1_w) / 2;
    if let Some(tag) = tags.first() {
        let [tr, tg, tb] = tag.rgb();
        #[allow(clippy::cast_precision_loss)]
        c.fill_circle_f(
            start_x as f32 + 4.5,
            cell.label_rect.y as f32 + 9.0,
            4.0,
            rgba(tr, tg, tb, 255),
        );
    }
    let text_col = if selected {
        pal.accent_text
    } else {
        pal.text_primary
    };
    c.draw_text(
        first_line,
        start_x + dot_space,
        cell.label_rect.y + 3,
        12.0,
        selected,
        text_col,
        cell.label_rect,
    );
    if !lines.1.is_empty() {
        let l2_w = Canvas::measure_text(&lines.1, 12.0, false);
        c.draw_text(
            &lines.1,
            cell.label_rect.x + (cell.label_rect.w - l2_w) / 2,
            cell.label_rect.y + 18,
            12.0,
            false,
            pal.text_primary,
            cell.label_rect,
        );
    }
}

/// Renders the Stage B `Gallery` view (`stage` + horizontal `filmstrip` + right `Information` panel).
pub(crate) fn render_gallery_view(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gal: &GalleryLayout,
    pal: &Palette,
) {
    render_gallery_stage(c, state, gal, pal);
    render_gallery_filmstrip(c, state, gal, pal);
    render_gallery_inspector(c, state, gal, pal);
}

fn render_gallery_stage(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gal: &GalleryLayout,
    pal: &Palette,
) {
    c.with_clip(gal.stage_rect, |c| {
        let Some(entry) = &gal.selected_entry else {
            c.draw_text(
                "No item selected",
                gal.stage_rect.x + (gal.stage_rect.w - 110) / 2,
                gal.stage_rect.y + gal.stage_rect.h / 2 - 8,
                13.0,
                false,
                pal.text_muted,
                gal.stage_rect,
            );
            return;
        };

        let inner = Rect::new(
            gal.stage_rect.x + 24,
            gal.stage_rect.y + 18,
            (gal.stage_rect.w - 48).max(40),
            (gal.stage_rect.h - 36).max(40),
        );
        match state.cached_preview_for(entry, 760, 480) {
            Some(CachedPreview::Image(img)) => {
                draw_aspect_fitted_preview(c, img, inner, 8, pal);
            }
            Some(CachedPreview::Text(preview)) => {
                render_text_preview_card(c, inner, &entry.name, &preview.lines, pal);
            }
            _ => {
                let tags = state.tags_for(&entry.path);
                #[allow(clippy::cast_precision_loss)]
                draw_large_entry_artwork(
                    c,
                    inner.x as f32 + inner.w as f32 * 0.5,
                    inner.y as f32 + inner.h as f32 * 0.48,
                    96.0,
                    entry,
                    tags.first().copied(),
                    pal,
                );
            }
        }
    });
}

fn render_gallery_filmstrip(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gal: &GalleryLayout,
    pal: &Palette,
) {
    c.fill_rect(
        Rect::new(
            gal.strip_rect.x + 8,
            gal.strip_rect.y,
            gal.strip_rect.w - 16,
            1,
        ),
        pal.divider,
    );
    let entries = state.columns.first().map_or((&[][..], None), |col| {
        (col.visible_entries.as_slice(), col.selected_index)
    });

    c.with_clip(gal.strip_rect, |c| {
        for thumb in &gal.thumbs {
            let Some(entry) = entries.0.get(thumb.row_idx) else {
                continue;
            };
            let active = entries.1 == Some(thumb.row_idx);
            let outer = Rect::new(
                thumb.rect.x - 3,
                thumb.rect.y - 3,
                thumb.rect.w + 6,
                thumb.rect.h + 6,
            );
            if active {
                c.fill_rounded_rect(outer, 8, pal.sidebar_active);
                c.stroke_rounded_rect(outer, 8, pal.accent);
            } else {
                c.fill_rounded_rect(thumb.rect, 6, pal.row_alt);
                c.stroke_rounded_rect(thumb.rect, 6, pal.divider);
            }

            if let Some(CachedPreview::Image(img)) = state.cached_preview_for(entry, 96, 96) {
                draw_framed_thumbnail(c, img, thumb.rect, pal);
            } else {
                let tags = state.tags_for(&entry.path);
                #[allow(clippy::cast_precision_loss)]
                draw_large_entry_artwork(
                    c,
                    thumb.rect.x as f32 + thumb.rect.w as f32 * 0.5,
                    thumb.rect.y as f32 + thumb.rect.h as f32 * 0.52,
                    38.0,
                    entry,
                    tags.first().copied(),
                    pal,
                );
            }
        }
    });
}

fn render_gallery_inspector(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gal: &GalleryLayout,
    pal: &Palette,
) {
    let info_bg = if pal.is_dark {
        rgba(35, 27, 51, 255)
    } else {
        rgba(247, 248, 251, 255)
    };
    c.fill_rect(gal.info_rect, info_bg);
    c.fill_rect(
        Rect::new(gal.info_rect.x, gal.info_rect.y, 1, gal.info_rect.h),
        pal.divider,
    );

    c.with_clip(gal.info_rect, |c| {
        let Some(entry) = &gal.selected_entry else {
            return;
        };
        let detailed = state.cached_info_for(&entry.path);
        render_inspector_header(c, state, gal, entry, detailed, pal);

        c.fill_rect(
            Rect::new(
                gal.info_rect.x + 14,
                gal.info_rect.y + 68,
                gal.info_rect.w - 28,
                1,
            ),
            pal.divider,
        );
        c.draw_text(
            "Information",
            gal.info_rect.x + 14,
            gal.info_rect.y + 76,
            13.0,
            true,
            pal.text_primary,
            gal.info_rect,
        );
        let toggle_lbl = if state.gallery_more_info {
            "Show Less"
        } else {
            "Show More"
        };
        c.draw_text(
            toggle_lbl,
            gal.show_more_rect.x,
            gal.show_more_rect.y + 4,
            11.5,
            true,
            pal.accent,
            gal.info_rect,
        );

        render_inspector_kv_rows(c, state, gal, entry, detailed, pal);
        render_inspector_tags_and_actions(c, state, gal, entry, pal);
    });
}

fn render_inspector_header(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gal: &GalleryLayout,
    entry: &FileEntry,
    detailed: Option<&FileDetailedInfo>,
    pal: &Palette,
) {
    let mini_rect = Rect::new(gal.info_rect.x + 14, gal.info_rect.y + 14, 42, 42);
    if let Some(CachedPreview::Image(img)) = state.cached_preview_for(entry, 96, 96) {
        draw_framed_thumbnail(c, img, mini_rect, pal);
    } else {
        #[allow(clippy::cast_precision_loss)]
        draw_entry_icon(
            c,
            mini_rect.x as f32 + 21.0,
            mini_rect.y as f32 + 21.0,
            entry,
            false,
            pal,
        );
    }
    let tx = mini_rect.right() + 10;
    let max_tw = (gal.info_rect.right() - tx - 10).max(40);
    let title = Canvas::fit_text(&entry.name, max_tw, 13.0, true);
    c.draw_text(
        &title,
        tx,
        gal.info_rect.y + 16,
        13.0,
        true,
        pal.text_primary,
        gal.info_rect,
    );
    let bytes = detailed.map_or(entry.size_bytes, |d| d.size_bytes);
    let sub = format!("{}  -  {}", entry.kind_label(), format_byte_size(bytes));
    let fitted_sub = Canvas::fit_text(&sub, max_tw, 11.0, false);
    c.draw_text(
        &fitted_sub,
        tx,
        gal.info_rect.y + 36,
        11.0,
        false,
        pal.text_muted,
        gal.info_rect,
    );
}

fn render_inspector_kv_rows(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gal: &GalleryLayout,
    entry: &FileEntry,
    detailed: Option<&FileDetailedInfo>,
    pal: &Palette,
) {
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    let created = detailed.and_then(|d| d.created_unix_secs);
    rows.push((
        "Created",
        created.map_or_else(|| String::from("--"), format_unix_timestamp),
    ));
    let modified = detailed
        .and_then(|d| d.modified_unix_secs)
        .or(entry.modified_unix_secs);
    rows.push((
        "Modified",
        modified.map_or_else(|| String::from("--"), format_unix_timestamp),
    ));
    if let Some((w, h)) = detailed.and_then(|d| d.dimensions) {
        rows.push(("Dimensions", format!("{w} x {h}")));
    } else if let Some(count) = detailed.and_then(|d| d.item_count) {
        rows.push(("Contents", format!("{count} items")));
    }
    if state.gallery_more_info
        && let Some(d) = detailed
    {
        rows.push(("Where", d.where_parent.clone()));
        rows.push(("Access", d.permissions.clone()));
    }

    let mut ry = gal.info_rect.y + 104;
    let val_x = gal.info_rect.x + 88;
    let max_val_w = (gal.info_rect.right() - val_x - 12).max(40);
    for (label, value) in rows {
        c.draw_text(
            label,
            gal.info_rect.x + 14,
            ry,
            11.5,
            false,
            pal.text_muted,
            gal.info_rect,
        );
        let fitted = Canvas::fit_text(&value, max_val_w, 11.5, false);
        c.draw_text(
            &fitted,
            val_x,
            ry,
            11.5,
            false,
            pal.text_primary,
            gal.info_rect,
        );
        ry += 22;
    }
}

fn render_inspector_tags_and_actions(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gal: &GalleryLayout,
    entry: &FileEntry,
    pal: &Palette,
) {
    let tags_hdr_y = if state.gallery_more_info {
        gal.info_rect.y + 214
    } else {
        gal.info_rect.y + 166
    };
    c.draw_text(
        "Tags",
        gal.info_rect.x + 14,
        tags_hdr_y,
        12.5,
        true,
        pal.text_primary,
        gal.info_rect,
    );
    let assigned = state.tags_for(&entry.path);
    for (color, rect) in &gal.info_tags {
        let active = assigned.contains(color);
        let [tr, tg, tb] = color.rgb();
        if active {
            c.fill_rounded_rect(*rect, 10, pal.sidebar_active);
            c.stroke_rounded_rect(*rect, 10, pal.accent);
        }
        #[allow(clippy::cast_precision_loss)]
        c.fill_circle_f(
            rect.x as f32 + 10.0,
            rect.y as f32 + 10.0,
            if active { 5.8 } else { 5.0 },
            rgba(tr, tg, tb, 255),
        );
    }

    // Bottom horizontal divider & compact Finder quick actions (`Quick Look`, `Info`, `Open`, `Trash`)
    let bar_top = gal.info_rect.bottom() - 56;
    c.fill_rect(
        Rect::new(gal.info_rect.x + 12, bar_top, gal.info_rect.w - 24, 1),
        pal.divider,
    );
    let buttons = [
        (gal.quick_look_btn, "Quick"),
        (gal.get_info_btn, "Info"),
        (gal.open_btn, "Open"),
        (
            gal.trash_btn,
            if state.is_trash_view() {
                "Restore"
            } else {
                "Trash"
            },
        ),
    ];
    for (rect, label) in buttons {
        c.fill_rounded_rect(rect, 8, pal.pill_bg);
        c.stroke_rounded_rect(rect, 8, pal.pill_border);
        let lw = Canvas::measure_text(label, 11.0, true);
        c.draw_text(
            label,
            rect.x + (rect.w - lw) / 2,
            rect.y + 11,
            11.0,
            true,
            pal.text_primary,
            rect,
        );
    }
}

/// Blits an aspect-fitted thumbnail centered inside `bounds` with crisp frame border.
pub(crate) fn draw_framed_thumbnail(
    c: &mut Canvas<'_>,
    img: &rusty_services::file_manager::DecodedPreviewImage,
    bounds: Rect,
    pal: &Palette,
) {
    let w = i32::try_from(img.width).unwrap_or(1).max(1);
    let h = i32::try_from(img.height).unwrap_or(1).max(1);
    let dst_w = w.min(bounds.w - 4);
    let dst_h = h.min(bounds.h - 4);
    let dx = bounds.x + (bounds.w - dst_w) / 2;
    let dy = bounds.y + (bounds.h - dst_h) / 2;
    let frame = Rect::new(dx - 2, dy - 2, dst_w + 4, dst_h + 4);
    c.fill_rounded_rect(
        frame,
        4,
        rgba(255, 255, 255, if pal.is_dark { 30 } else { 255 }),
    );
    c.blit_bgra_premul_fit(dx, dy, dst_w, dst_h, img, bounds);
    c.stroke_rounded_rect(frame, 4, pal.pill_border);
}

/// Blits a large centered aspect-fitted image with rounded corners inside `bounds`.
pub(crate) fn draw_aspect_fitted_preview(
    c: &mut Canvas<'_>,
    img: &rusty_services::file_manager::DecodedPreviewImage,
    bounds: Rect,
    radius: i32,
    pal: &Palette,
) {
    let iw = i32::try_from(img.width).unwrap_or(1).max(1);
    let ih = i32::try_from(img.height).unwrap_or(1).max(1);
    let max_w = bounds.w.max(20);
    let max_h = bounds.h.max(20);
    let (fit_w, fit_h) = if iw * max_h >= ih * max_w {
        let tw = iw.min(max_w);
        let th = ((ih * tw) / iw).max(1);
        (tw, th)
    } else {
        let th = ih.min(max_h);
        let tw = ((iw * th) / ih).max(1);
        (tw, th)
    };
    let dx = bounds.x + (bounds.w - fit_w) / 2;
    let dy = bounds.y + (bounds.h - fit_h) / 2;
    let frame = Rect::new(dx - 1, dy - 1, fit_w + 2, fit_h + 2);
    c.blit_bgra_premul_fit(dx, dy, fit_w, fit_h, img, bounds);
    c.stroke_rounded_rect(frame, radius, pal.pill_border);
}

/// Renders a crisp monospaced-feel card for plain-text/code previews in Gallery & Quick Look.
pub(crate) fn render_text_preview_card(
    c: &mut Canvas<'_>,
    bounds: Rect,
    title: &str,
    lines: &[String],
    pal: &Palette,
) {
    let card_w = bounds.w.min(620);
    let card_h = bounds.h.min(440);
    let card = Rect::new(
        bounds.x + (bounds.w - card_w) / 2,
        bounds.y + (bounds.h - card_h) / 2,
        card_w,
        card_h,
    );
    let paper = if pal.is_dark {
        rgba(38, 30, 56, 255)
    } else {
        rgba(253, 254, 255, 255)
    };
    c.fill_rounded_rect(card, 10, paper);
    c.stroke_rounded_rect(card, 10, pal.pill_border);
    c.with_clip(card, |c| {
        let title_fit = Canvas::fit_text(title, card.w - 28, 12.5, true);
        c.draw_text(
            &title_fit,
            card.x + 14,
            card.y + 10,
            12.5,
            true,
            pal.text_secondary,
            card,
        );
        c.fill_rect(
            Rect::new(card.x + 12, card.y + 30, card.w - 24, 1),
            pal.divider,
        );
        let mut ly = card.y + 38;
        for line in lines {
            if ly + 16 > card.bottom() - 8 {
                break;
            }
            let expanded = line.replace('\t', "    ");
            let fitted = Canvas::fit_text(&expanded, card.w - 28, 11.5, false);
            c.draw_text(
                &fitted,
                card.x + 14,
                ly,
                11.5,
                false,
                pal.text_primary,
                card,
            );
            ly += 17;
        }
        if lines.is_empty() {
            c.draw_text(
                "(empty file)",
                card.x + 14,
                ly,
                12.0,
                false,
                pal.text_muted,
                card,
            );
        }
    });
}

fn wrap_two_lines(name: &str, max_w: i32, size_px: f32, bold_line1: bool) -> (String, String) {
    if Canvas::measure_text(name, size_px, bold_line1) <= max_w {
        return (name.to_owned(), String::new());
    }
    let mut split_idx = 0_usize;
    let mut acc_w = 0_i32;
    for (byte_idx, ch) in name.char_indices() {
        let mut buf = [0_u8; 4];
        let s = ch.encode_utf8(&mut buf);
        let cw = Canvas::measure_text(s, size_px, bold_line1);
        if acc_w + cw > max_w && byte_idx > 0 {
            break;
        }
        acc_w += cw;
        split_idx = byte_idx + ch.len_utf8();
    }
    let line1 = &name[..split_idx];
    let rest = name[split_idx..].trim_start();
    let line2 = if rest.is_empty() {
        String::new()
    } else {
        Canvas::fit_text(rest, max_w, size_px, false)
    };
    (line1.to_owned(), line2)
}
