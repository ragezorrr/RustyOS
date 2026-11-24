//! File listing view renderers: Miller Columns, List View, Icon Grid, Gallery View, and Preview Inspector.

use rusty_services::file_manager::{
    DecodedPreviewImage, FileDetailedInfo, FileEntry, FileSortField, FileSortOrder, FileTagColor,
    format_byte_size, format_unix_timestamp,
};

use crate::icons::{draw_chevron_aa, draw_entry_icon, draw_large_entry_artwork};
use crate::layout::{
    ColumnLayout, FilesLayout, GalleryLayout, IconCellLayout, IconsGridLayout, PreviewLayout, Rect,
};
use crate::render::canvas::Canvas;
use crate::render::chrome::render_path_bar;
use crate::render::{Palette, overlay_scrollbar_color, rgba};
use crate::state::{CachedPreview, FilesAppState, ICON_CELL_H, LIST_ROW_HEIGHT, ROW_HEIGHT};

pub(crate) fn render_miller_columns(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    c.with_clip(layout.content_rect, |c| {
        for col_layout in &layout.columns {
            render_miller_column_pane(c, state, layout, col_layout, pal);
        }
    });
}

pub(crate) fn render_miller_column_pane(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    col_layout: &ColumnLayout,
    pal: &Palette,
) {
    let Some(col) = state.columns.get(col_layout.col_idx) else {
        return;
    };
    let is_focused_col =
        col_layout.col_idx == state.active_column || col_layout.col_idx + 1 == state.columns.len();

    // Column divider stops strictly at content_rect.bottom(), above the status bar margin
    c.fill_rect(
        Rect::new(
            col_layout.rect.right() - 1,
            layout.content_rect.y,
            1,
            layout.content_rect.h,
        ),
        pal.divider,
    );

    let col_clip = Rect::new(
        col_layout.rect.x,
        layout.content_rect.y,
        col_layout.rect.w - 1,
        layout.content_rect.h,
    );

    c.with_clip(col_clip, |c| {
        if col.loading {
            c.draw_text(
                "Loading...",
                col_layout.rect.x + 14,
                col_layout.rect.y + 12,
                12.5,
                false,
                pal.text_muted,
                col_clip,
            );
            return;
        }
        if let Some(err) = &col.error {
            let msg = Canvas::fit_text(err, col_layout.rect.w - 24, 12.0, false);
            c.draw_text(
                &msg,
                col_layout.rect.x + 12,
                col_layout.rect.y + 12,
                12.0,
                false,
                rgba(220, 72, 72, 255),
                col_clip,
            );
            return;
        }
        if col.visible_entries.is_empty() {
            c.draw_text(
                "Empty",
                col_layout.rect.x + 16,
                col_layout.rect.y + 14,
                12.5,
                false,
                pal.text_muted,
                col_clip,
            );
            return;
        }

        for &(row_idx, row_rect) in &col_layout.rows {
            let Some(entry) = col.visible_entries.get(row_idx) else {
                continue;
            };
            let selected = col.is_selected(row_idx);
            let hovered = state.hovered_hit
                == Some(crate::layout::FilesHit::ColumnRow {
                    col_idx: col_layout.col_idx,
                    row_idx,
                });
            if !selected && hovered {
                let hover_bg = if pal.is_dark {
                    rgba(255, 255, 255, 12)
                } else {
                    rgba(0, 0, 0, 10)
                };
                c.fill_rounded_rect(row_rect, 6, hover_bg);
            }
            let primary_active = selected && is_focused_col;
            render_miller_column_row(
                c,
                state,
                entry,
                row_rect,
                col_clip,
                selected,
                primary_active,
                pal,
            );
        }

        let total_h = i32::try_from(col.visible_entries.len()).unwrap_or(0) * ROW_HEIGHT + 12;
        c.draw_scrollbar(
            col_clip,
            col.scroll_y,
            total_h,
            overlay_scrollbar_color(pal.scrollbar, state.overlay_scrollbar_alpha),
        );
    });
}

#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
pub(crate) fn render_miller_column_row(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    entry: &FileEntry,
    row_rect: Rect,
    col_clip: Rect,
    selected: bool,
    primary_active: bool,
    pal: &Palette,
) {
    if selected {
        let bg = if primary_active {
            pal.accent
        } else {
            pal.secondary_select
        };
        c.fill_rounded_rect(row_rect, 7, bg);
    }
    let fg = if primary_active {
        pal.accent_text
    } else {
        pal.text_primary
    };
    draw_entry_icon(
        c,
        row_rect.x as f32 + 13.0,
        row_rect.y as f32 + (row_rect.h as f32) * 0.5,
        entry,
        primary_active,
        pal,
    );

    let tags = state.tags_for(&entry.path);
    let tags_w = i32::try_from(tags.len().min(3)).unwrap_or(0) * 9;
    let chev_w = if entry.is_navigable_dir() { 20 } else { 6 };
    let text_w = (row_rect.w - 30 - tags_w - chev_w).max(20);
    let label = Canvas::fit_text(&entry.name, text_w, 13.0, selected);
    c.draw_text(
        &label,
        row_rect.x + 26,
        row_rect.y + 5,
        13.0,
        selected,
        fg,
        col_clip,
    );

    let mut dot_x = row_rect.right() - chev_w - 6;
    for tag in tags.iter().take(3).rev() {
        let [r, g, b] = tag.rgb();
        c.fill_circle(dot_x, row_rect.y + 13, 3.5, rgba(r, g, b, 255));
        dot_x -= 9;
    }
    if entry.is_navigable_dir() {
        let chev_col = if primary_active {
            pal.accent_text
        } else {
            pal.text_muted
        };
        draw_chevron_aa(
            c,
            row_rect.right() as f32 - 13.5,
            row_rect.y as f32 + 13.0,
            false,
            1.9,
            3.5,
            1.35,
            chev_col,
        );
    }
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn render_preview_inspector(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    preview: &PreviewLayout,
    pal: &Palette,
) {
    let entry = &preview.entry;
    let cx = preview.rect.x + preview.rect.w / 2;
    let card_clip = preview.rect;

    c.with_clip(card_clip, |c| {
        let badge = Rect::new(cx - 32, preview.rect.y + 34, 64, 72);
        c.fill_rounded_rect(badge, 12, pal.row_alt);
        c.stroke_rounded_rect(badge, 12, pal.divider);
        draw_entry_icon(
            c,
            badge.x as f32 + 32.0,
            badge.y as f32 + 24.0,
            entry,
            false,
            pal,
        );
        let kind_w = Canvas::measure_text(entry.kind_label(), 10.5, true);
        c.draw_text(
            entry.kind_label(),
            cx - kind_w / 2,
            badge.y + 44,
            10.5,
            true,
            pal.text_secondary,
            badge,
        );

        let name = Canvas::fit_text(&entry.name, preview.rect.w - 40, 15.0, true);
        let name_w = Canvas::measure_text(&name, 15.0, true);
        c.draw_text(
            &name,
            cx - name_w / 2,
            preview.rect.y + 122,
            15.0,
            true,
            pal.text_primary,
            card_clip,
        );

        let meta_sub = format!(
            "{}  -  {}",
            entry.kind_label(),
            format_byte_size(entry.size_bytes)
        );
        let meta_w = Canvas::measure_text(&meta_sub, 12.0, false);
        c.draw_text(
            &meta_sub,
            cx - meta_w / 2,
            preview.rect.y + 146,
            12.0,
            false,
            pal.text_secondary,
            card_clip,
        );

        if let Some(secs) = entry.modified_unix_secs {
            let mod_text = format!("Modified: {}", format_unix_timestamp(secs));
            let mw = Canvas::measure_text(&mod_text, 11.5, false);
            c.draw_text(
                &mod_text,
                cx - mw / 2,
                preview.rect.y + 174,
                11.5,
                false,
                pal.text_muted,
                card_clip,
            );
        }

        render_preview_actions_and_tags(c, state, preview, pal);
    });
}

pub(crate) fn render_preview_actions_and_tags(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    preview: &PreviewLayout,
    pal: &Palette,
) {
    c.fill_rounded_rect(preview.open_btn, 14, pal.pill_bg);
    c.stroke_rounded_rect(preview.open_btn, 14, pal.pill_border);
    let open_w = Canvas::measure_text("Open", 12.5, true);
    c.draw_text(
        "Open",
        preview.open_btn.x + (preview.open_btn.w - open_w) / 2,
        preview.open_btn.y + 7,
        12.5,
        true,
        pal.text_primary,
        preview.open_btn,
    );

    let trash_label = if state.is_trash_view() {
        "Put Back"
    } else {
        "Move to Trash"
    };
    c.fill_rounded_rect(preview.trash_btn, 14, pal.pill_bg);
    c.stroke_rounded_rect(preview.trash_btn, 14, pal.pill_border);
    let tl_w = Canvas::measure_text(trash_label, 12.0, false);
    c.draw_text(
        trash_label,
        preview.trash_btn.x + (preview.trash_btn.w - tl_w) / 2,
        preview.trash_btn.y + 7,
        12.0,
        false,
        pal.text_primary,
        preview.trash_btn,
    );

    let active_tags = state.tags_for(&preview.entry.path);
    for (color, rect) in &preview.tag_dots {
        let [r, g, b] = color.rgb();
        c.fill_circle(rect.x + 8, rect.y + 8, 6.0, rgba(r, g, b, 255));
        if active_tags.contains(color) {
            c.fill_circle(rect.x + 8, rect.y + 8, 2.2, rgba(255, 255, 255, 255));
        }
    }
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn render_list_view(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    let hdr_y = layout.content_rect.y;
    for (field, rect) in &layout.list_headers {
        c.draw_text(
            field.label(),
            rect.x + 12,
            rect.y + 5,
            11.5,
            true,
            pal.text_secondary,
            *rect,
        );
        if state.sort_field == *field {
            let label_w = Canvas::measure_text(field.label(), 11.5, true);
            let cx = (rect.x + 18 + label_w) as f32;
            let cy = rect.y as f32 + 12.5;
            let pts = match state.sort_order {
                FileSortOrder::Ascending => {
                    [(cx - 3.0, cy + 1.5), (cx, cy - 1.5), (cx + 3.0, cy + 1.5)]
                }
                FileSortOrder::Descending => {
                    [(cx - 3.0, cy - 1.5), (cx, cy + 1.5), (cx + 3.0, cy - 1.5)]
                }
            };
            c.stroke_polyline_aa(&pts, false, 1.3, pal.text_secondary);
        }
    }
    c.fill_rect(
        Rect::new(
            layout.content_rect.x + 8,
            hdr_y + 25,
            layout.content_rect.w - 16,
            1,
        ),
        pal.divider,
    );

    let Some(col) = state.columns.first() else {
        return;
    };
    let clip = Rect::new(
        layout.content_rect.x,
        hdr_y + 27,
        layout.content_rect.w,
        (layout.content_rect.h - 27).max(20),
    );
    let cols_x = list_column_offsets(layout);

    c.with_clip(clip, |c| {
        for &(row_idx, rect) in &layout.list_rows {
            if let Some(entry) = col.visible_entries.get(row_idx) {
                let selected = col.is_selected(row_idx);
                let hovered =
                    state.hovered_hit == Some(crate::layout::FilesHit::ListRow { row_idx });
                if !selected && hovered {
                    let hover_bg = if pal.is_dark {
                        rgba(255, 255, 255, 12)
                    } else {
                        rgba(0, 0, 0, 10)
                    };
                    c.fill_rounded_rect(rect, 6, hover_bg);
                }
                render_list_row(c, entry, rect, row_idx, selected, cols_x, clip, pal);
            }
        }
        let total_h = i32::try_from(col.visible_entries.len()).unwrap_or(0) * LIST_ROW_HEIGHT + 8;
        c.draw_scrollbar(
            clip,
            state.list_scroll_y,
            total_h,
            overlay_scrollbar_color(pal.scrollbar, state.overlay_scrollbar_alpha),
        );
    });
}

pub(crate) fn list_column_offsets(layout: &FilesLayout) -> (i32, i32, i32) {
    let date_x = layout
        .list_headers
        .iter()
        .find(|(f, _)| *f == FileSortField::DateModified)
        .map_or(layout.content_rect.x + 280, |(_, r)| r.x);
    let size_x = layout
        .list_headers
        .iter()
        .find(|(f, _)| *f == FileSortField::Size)
        .map_or(date_x + 140, |(_, r)| r.x);
    let kind_x = layout
        .list_headers
        .iter()
        .find(|(f, _)| *f == FileSortField::Kind)
        .map_or(size_x + 78, |(_, r)| r.x);
    (date_x, size_x, kind_x)
}

#[allow(clippy::cast_precision_loss, clippy::too_many_arguments)]
pub(crate) fn render_list_row(
    c: &mut Canvas<'_>,
    entry: &FileEntry,
    rect: Rect,
    row_idx: usize,
    selected: bool,
    cols_x: (i32, i32, i32),
    clip: Rect,
    pal: &Palette,
) {
    let (date_x, size_x, kind_x) = cols_x;
    if selected {
        c.fill_rounded_rect(rect, 7, pal.accent);
    } else if row_idx % 2 == 1 {
        c.fill_rounded_rect(rect, 7, pal.row_alt);
    }
    let fg = if selected {
        pal.accent_text
    } else {
        pal.text_primary
    };
    let sub_fg = if selected {
        pal.accent_text
    } else {
        pal.text_secondary
    };

    draw_entry_icon(
        c,
        rect.x as f32 + 15.0,
        rect.y as f32 + (rect.h as f32) * 0.5,
        entry,
        selected,
        pal,
    );
    let name = Canvas::fit_text(&entry.name, (date_x - rect.x - 36).max(60), 13.0, selected);
    c.draw_text(&name, rect.x + 30, rect.y + 7, 13.0, selected, fg, clip);

    let date_str = entry
        .modified_unix_secs
        .map_or_else(|| String::from("--"), format_unix_timestamp);
    c.draw_text(&date_str, date_x + 8, rect.y + 7, 12.0, false, sub_fg, clip);

    let size_str = if entry.is_navigable_dir() {
        String::from("--")
    } else {
        format_byte_size(entry.size_bytes)
    };
    c.draw_text(&size_str, size_x + 8, rect.y + 7, 12.0, false, sub_fg, clip);
    c.draw_text(
        entry.kind_label(),
        kind_x + 8,
        rect.y + 7,
        12.0,
        false,
        sub_fg,
        clip,
    );
}

pub(crate) fn render_status_bar(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    c.fill_rect(
        Rect::new(
            layout.status_bar_rect.x + 4,
            layout.status_bar_rect.y,
            layout.status_bar_rect.w - 8,
            1,
        ),
        pal.divider,
    );
    if let Some(mode) = &state.picker_mode {
        render_file_picker_bar(c, state, layout, mode, pal);
        return;
    }
    let summary = state.status_summary();
    let status_right = layout.status_bar_rect.right() - 8;
    if layout.path_crumbs.is_empty() {
        let max_text_w = (status_right - layout.status_bar_rect.x - 12).max(40);
        let fitted = Canvas::fit_text(&summary, max_text_w, 11.5, false);
        let sum_w = Canvas::measure_text(&fitted, 11.5, false);
        let text_x = layout.status_bar_rect.x + ((layout.status_bar_rect.w - sum_w) / 2).max(8);
        let text_col = if state.status_message.is_some() {
            pal.text_secondary
        } else {
            pal.text_muted
        };
        c.draw_text(
            &fitted,
            text_x,
            layout.status_bar_rect.y + 6,
            11.5,
            false,
            text_col,
            layout.status_bar_rect,
        );
    } else {
        render_path_bar(c, &layout.path_crumbs, pal);
        let right_edge = layout
            .path_crumbs
            .last()
            .map_or(layout.status_bar_rect.x + 12, |crumb| {
                crumb.rect.right() + 12
            });
        let avail_summary = (status_right - right_edge - 8).max(0);
        if avail_summary >= 36 {
            let fitted = Canvas::fit_text(&summary, avail_summary, 11.5, false);
            let sum_w = Canvas::measure_text(&fitted, 11.5, false);
            c.draw_text(
                &fitted,
                status_right - sum_w,
                layout.status_bar_rect.y + 6,
                11.5,
                false,
                pal.text_secondary,
                layout.status_bar_rect,
            );
        }
    }
}

fn render_file_picker_bar(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    mode: &crate::state::FilePickerMode,
    pal: &Palette,
) {
    let bar = layout.status_bar_rect;
    let confirm_label = match mode {
        crate::state::FilePickerMode::OpenFile | crate::state::FilePickerMode::OpenFiles => "Open",
        crate::state::FilePickerMode::OpenFolder => "Select",
        crate::state::FilePickerMode::SaveFile { .. } => "Save",
    };
    let left_limit = layout
        .picker_cancel_rect
        .map_or(bar.right() - 180, |r| r.x - 12);
    if let crate::state::FilePickerMode::SaveFile { .. } = mode {
        c.draw_text(
            "Save As:",
            bar.x + 10,
            bar.y + 11,
            12.5,
            true,
            pal.text_secondary,
            bar,
        );
        let box_x = bar.x + 72;
        let box_w = (left_limit - box_x).clamp(80, 280);
        let input_rect = layout
            .picker_filename_rect
            .unwrap_or_else(|| Rect::new(box_x, bar.y + 6, box_w, 26));
        c.fill_rounded_rect(input_rect, 8, pal.pill_bg);
        if state.picker_filename_focused {
            c.stroke_rounded_rect(input_rect, 8, pal.accent);
        }
        let shown = Canvas::fit_text(&state.picker_filename, input_rect.w - 16, 12.5, false);
        c.draw_text(
            &shown,
            input_rect.x + 8,
            input_rect.y + 5,
            12.5,
            false,
            pal.text_primary,
            input_rect,
        );
        if state.picker_filename_focused && state.caret_visible {
            let prefix = &state.picker_filename[..state
                .picker_filename_field
                .buffer
                .cursor()
                .min(state.picker_filename.len())];
            let caret_dx = Canvas::measure_text(prefix, 12.5, false).min(input_rect.w - 14);
            c.fill_rect(
                Rect::new(input_rect.x + 8 + caret_dx, input_rect.y + 5, 1, 16),
                pal.accent,
            );
        }
    } else {
        let prompt = state
            .status_message
            .as_deref()
            .unwrap_or_else(|| state.picker_title.as_deref().unwrap_or("Choose an item"));
        let fitted = Canvas::fit_text(prompt, (left_limit - bar.x - 12).max(40), 12.0, false);
        c.draw_text(
            &fitted,
            bar.x + 10,
            bar.y + 11,
            12.0,
            false,
            pal.text_secondary,
            bar,
        );
    }
    if let Some(cancel_r) = layout.picker_cancel_rect {
        c.fill_rounded_rect(cancel_r, 8, pal.pill_bg);
        let tw = Canvas::measure_text("Cancel", 12.5, true);
        c.draw_text(
            "Cancel",
            cancel_r.x + (cancel_r.w - tw) / 2,
            cancel_r.y + 5,
            12.5,
            true,
            pal.text_primary,
            cancel_r,
        );
    }
    if let Some(confirm_r) = layout.picker_confirm_rect {
        c.fill_rounded_rect(confirm_r, 8, pal.accent);
        let tw = Canvas::measure_text(confirm_label, 12.5, true);
        c.draw_text(
            confirm_label,
            confirm_r.x + (confirm_r.w - tw) / 2,
            confirm_r.y + 5,
            12.5,
            true,
            pal.accent_text,
            confirm_r,
        );
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
            let selected = col.is_selected(cell.row_idx);
            let hovered =
                state.hovered_hit == Some(crate::layout::FilesHit::IconCell { idx: cell.row_idx });
            if !selected && hovered {
                let hover_bg = if pal.is_dark {
                    rgba(255, 255, 255, 14)
                } else {
                    rgba(0, 0, 0, 10)
                };
                let art_hl = Rect::new(
                    cell.art_rect.x - 4,
                    cell.art_rect.y - 4,
                    cell.art_rect.w + 8,
                    cell.art_rect.h + 8,
                );
                c.fill_rounded_rect(art_hl, 10, hover_bg);
            }
            let tags = state.tags_for(&entry.path);
            render_single_icon_cell(c, state, cell, entry, tags, selected, pal);
        }
        let rows = i32::try_from(col.visible_entries.len().div_ceil(grid.cols.max(1))).unwrap_or(0);
        c.draw_scrollbar(
            clip,
            state.icons_scroll_y,
            rows * ICON_CELL_H + 16,
            overlay_scrollbar_color(pal.scrollbar, state.overlay_scrollbar_alpha),
        );
    });
}

pub(crate) fn render_single_icon_cell(
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

pub(crate) fn render_gallery_stage(
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

pub(crate) fn render_gallery_filmstrip(
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

pub(crate) fn render_gallery_inspector(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gal: &GalleryLayout,
    pal: &Palette,
) {
    let info_bg = if pal.is_dark {
        rgba(36, 36, 40, 255)
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

pub(crate) fn render_inspector_header(
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

pub(crate) fn render_inspector_kv_rows(
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

pub(crate) fn render_inspector_tags_and_actions(
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
    img: &DecodedPreviewImage,
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
    img: &DecodedPreviewImage,
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
        rgba(38, 38, 44, 255)
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

pub(crate) fn wrap_two_lines(
    name: &str,
    max_w: i32,
    size_px: f32,
    bold_line1: bool,
) -> (String, String) {
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
