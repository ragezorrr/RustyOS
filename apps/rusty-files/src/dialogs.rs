//! Stage B Golden Gate modal sheets & inline dialogs (`Quick Look`, `Get Info`,
//! `Go to Folder`, `Transfer Progress`, conflict `Replace / Keep Both / Stop`,
//! and inline `Rename`) (`RUSTYOS-066`).

use std::path::PathBuf;

use rusty_services::file_manager::{
    FileTagColor, TransferConflictDecision, TransferKind, format_byte_size, format_unix_timestamp,
};

use crate::icons::{draw_chevron_aa, draw_close_x_glyph, draw_large_entry_artwork};
use crate::layout::{FilesHit, FilesLayout, Rect};
use crate::render::{Canvas, Palette, rgba};
use crate::state::{CachedPreview, FilesAppState, InlineRenameState};
use crate::views::{draw_aspect_fitted_preview, draw_framed_thumbnail, render_text_preview_card};

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

/// Renders active Stage B overlays cleanly in back-to-front order.
pub(crate) fn render_dialog_overlays(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    if let Some(rn) = &state.inline_rename {
        render_inline_rename_editor(c, state, rn, layout, pal);
    }
    if let Some(ql) = &layout.overlays.quick_look {
        render_quick_look_overlay(c, state, ql, pal);
    }
    if let Some(gi) = &layout.overlays.get_info {
        render_get_info_sheet(c, state, gi, pal);
    }
    if let Some(gt) = &layout.overlays.go_to_folder {
        render_go_to_folder_sheet(c, state, gt, pal);
    }
    if let Some(tr) = &layout.overlays.transfer {
        render_transfer_progress_card(c, state, tr, pal);
    }
    if let Some(cf) = &layout.overlays.conflict {
        render_conflict_dialog(c, state, cf, pal);
    }
}

fn render_inline_rename_editor(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    rn: &InlineRenameState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    let box_rect = layout.overlays.inline_rename_rect.unwrap_or_else(|| {
        Rect::new(
            layout.content_rect.x + 20,
            layout.content_rect.y + 12,
            220,
            26,
        )
    });
    c.fill_rounded_rect(box_rect, 6, pal.window_bg);
    let border = if rn.error.is_some() {
        rgba(235, 55, 48, 255)
    } else {
        pal.accent
    };
    c.stroke_rounded_rect(box_rect, 6, border);
    let fitted = Canvas::fit_text(&rn.draft, box_rect.w - 14, 12.5, false);
    c.draw_text(
        &fitted,
        box_rect.x + 6,
        box_rect.y + 5,
        12.5,
        false,
        pal.text_primary,
        box_rect,
    );
    if state.caret_visible {
        let caret_x = box_rect.x + 6 + Canvas::measure_text(&fitted, 12.5, false);
        if caret_x < box_rect.right() - 4 {
            c.fill_rect(
                Rect::new(caret_x, box_rect.y + 4, 2, box_rect.h - 8),
                pal.accent,
            );
        }
    }
}

fn render_quick_look_overlay(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    ql: &QuickLookLayout,
    pal: &Palette,
) {
    let Some(qs) = &state.quick_look else {
        return;
    };
    let win = Rect::new(0, 0, c.w, c.h);
    c.fill_rect(win, rgba(16, 18, 26, if pal.is_dark { 130 } else { 78 }));
    draw_modal_shadow_and_frame(c, ql.modal_rect, 14, pal);

    // Top bar: Close (x), prev <, next >, Title, Open button
    c.fill_rounded_rect(ql.close_rect, 10, pal.pill_bg);
    c.stroke_rounded_rect(ql.close_rect, 10, pal.pill_border);
    #[allow(clippy::cast_precision_loss)]
    draw_close_x_glyph(
        c,
        ql.close_rect.x as f32 + 10.0,
        ql.close_rect.y as f32 + 10.0,
        3.5,
        pal.text_primary,
    );
    #[allow(clippy::cast_precision_loss)]
    {
        draw_chevron_aa(
            c,
            ql.prev_rect.x as f32 + 11.0,
            ql.prev_rect.y as f32 + 10.0,
            true,
            2.4,
            4.2,
            1.4,
            pal.text_secondary,
        );
        draw_chevron_aa(
            c,
            ql.next_rect.x as f32 + 11.0,
            ql.next_rect.y as f32 + 10.0,
            false,
            2.4,
            4.2,
            1.4,
            pal.text_secondary,
        );
    }

    let title_x = ql.next_rect.right() + 12;
    let title_w = (ql.open_rect.x - title_x - 10).max(40);
    let title = Canvas::fit_text(&qs.entry.name, title_w, 13.5, true);
    c.draw_text(
        &title,
        title_x,
        ql.modal_rect.y + 12,
        13.5,
        true,
        pal.text_primary,
        ql.modal_rect,
    );

    c.fill_rounded_rect(ql.open_rect, 12, pal.pill_bg);
    c.stroke_rounded_rect(ql.open_rect, 12, pal.pill_border);
    let ow = Canvas::measure_text("Open", 12.0, true);
    c.draw_text(
        "Open",
        ql.open_rect.x + (ql.open_rect.w - ow) / 2,
        ql.open_rect.y + 5,
        12.0,
        true,
        pal.text_primary,
        ql.open_rect,
    );

    c.fill_rect(
        Rect::new(
            ql.modal_rect.x + 1,
            ql.modal_rect.y + 38,
            ql.modal_rect.w - 2,
            1,
        ),
        pal.divider,
    );

    render_quick_look_stage(c, state, ql, &qs.entry, pal);
}

fn render_quick_look_stage(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    ql: &QuickLookLayout,
    entry: &rusty_services::file_manager::FileEntry,
    pal: &Palette,
) {
    c.with_clip(ql.stage_rect, |c| {
        match state.cached_preview_for(entry, 760, 500) {
            Some(CachedPreview::Image(img)) => {
                draw_aspect_fitted_preview(c, img, ql.stage_rect, 8, pal);
            }
            Some(CachedPreview::Text(preview)) => {
                render_text_preview_card(c, ql.stage_rect, &entry.name, &preview.lines, pal);
            }
            _ => {
                let tags = state.tags_for(&entry.path);
                #[allow(clippy::cast_precision_loss)]
                draw_large_entry_artwork(
                    c,
                    ql.stage_rect.x as f32 + ql.stage_rect.w as f32 * 0.5,
                    ql.stage_rect.y as f32 + ql.stage_rect.h as f32 * 0.42,
                    104.0,
                    entry,
                    tags.first().copied(),
                    pal,
                );
                let sub = format!(
                    "{}  -  {}",
                    entry.kind_label(),
                    format_byte_size(entry.size_bytes)
                );
                let sw = Canvas::measure_text(&sub, 12.5, false);
                c.draw_text(
                    &sub,
                    ql.stage_rect.x + (ql.stage_rect.w - sw) / 2,
                    ql.stage_rect.bottom() - 42,
                    12.5,
                    false,
                    pal.text_secondary,
                    ql.stage_rect,
                );
            }
        }
    });
}

fn render_get_info_sheet(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gi: &GetInfoLayout,
    pal: &Palette,
) {
    let Some(gs) = &state.get_info else {
        return;
    };
    draw_modal_shadow_and_frame(c, gi.modal_rect, 12, pal);

    c.with_clip(gi.modal_rect, |c| {
        c.fill_rounded_rect(gi.close_rect, 9, pal.pill_bg);
        c.stroke_rounded_rect(gi.close_rect, 9, pal.pill_border);
        #[allow(clippy::cast_precision_loss)]
        draw_close_x_glyph(
            c,
            gi.close_rect.x as f32 + 9.0,
            gi.close_rect.y as f32 + 9.0,
            3.0,
            pal.text_primary,
        );

        let name = gs
            .info
            .as_ref()
            .map(|i| i.name.as_str())
            .or_else(|| gs.target_path.file_name().and_then(|s| s.to_str()))
            .unwrap_or("Info");
        let hdr = Canvas::fit_text(&format!("{name} Info"), gi.modal_rect.w - 48, 13.0, true);
        c.draw_text(
            &hdr,
            gi.close_rect.right() + 10,
            gi.modal_rect.y + 11,
            13.0,
            true,
            pal.text_primary,
            gi.modal_rect,
        );
        c.fill_rect(
            Rect::new(
                gi.modal_rect.x + 1,
                gi.modal_rect.y + 34,
                gi.modal_rect.w - 2,
                1,
            ),
            pal.divider,
        );

        let size_str = gs.info.as_ref().map_or_else(
            || String::from("Loading..."),
            |d| format_byte_size(d.size_bytes),
        );
        let item_name = Canvas::fit_text(name, gi.modal_rect.w - 86, 13.5, true);
        c.draw_text(
            &item_name,
            gi.modal_rect.x + 68,
            gi.modal_rect.y + 46,
            13.5,
            true,
            pal.text_primary,
            gi.modal_rect,
        );
        c.draw_text(
            &size_str,
            gi.modal_rect.x + 68,
            gi.modal_rect.y + 66,
            12.0,
            false,
            pal.text_secondary,
            gi.modal_rect,
        );

        render_get_info_tags_and_general(c, state, gi, gs, pal);
        render_get_info_more_and_preview(c, state, gi, gs, pal);
    });
}

fn render_get_info_tags_and_general(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gi: &GetInfoLayout,
    gs: &crate::state::GetInfoState,
    pal: &Palette,
) {
    c.draw_text(
        "Tags:",
        gi.modal_rect.x + 16,
        gi.modal_rect.y + 102,
        11.5,
        true,
        pal.text_secondary,
        gi.modal_rect,
    );
    let assigned = state.tags_for(&gs.target_path);
    for (color, rect) in &gi.tags {
        let active = assigned.contains(color);
        if active {
            c.fill_rounded_rect(*rect, 11, pal.sidebar_active);
            c.stroke_rounded_rect(*rect, 11, pal.accent);
        }
        let [tr, tg, tb] = color.rgb();
        #[allow(clippy::cast_precision_loss)]
        c.fill_circle_f(
            rect.x as f32 + 11.0,
            rect.y as f32 + 11.0,
            if active { 6.0 } else { 5.0 },
            rgba(tr, tg, tb, 255),
        );
    }

    c.fill_rect(
        Rect::new(
            gi.modal_rect.x + 14,
            gi.modal_rect.y + 128,
            gi.modal_rect.w - 28,
            1,
        ),
        pal.divider,
    );
    c.draw_text(
        "General:",
        gi.modal_rect.x + 16,
        gi.modal_rect.y + 136,
        12.0,
        true,
        pal.text_primary,
        gi.modal_rect,
    );

    let mut rows: Vec<(&'static str, String)> = Vec::new();
    if let Some(info) = &gs.info {
        rows.push(("Kind", info.kind_label.clone()));
        rows.push(("Where", info.where_parent.clone()));
        if let Some(cnt) = info.item_count {
            rows.push(("Items", cnt.to_string()));
        }
        rows.push((
            "Modified",
            info.modified_unix_secs
                .map_or_else(|| String::from("--"), format_unix_timestamp),
        ));
    } else if let Some(err) = &gs.error {
        rows.push(("Error", err.clone()));
    }
    let mut ry = gi.modal_rect.y + 158;
    let val_x = gi.modal_rect.x + 92;
    let max_vw = (gi.modal_rect.right() - val_x - 14).max(40);
    for (lbl, val) in rows {
        c.draw_text(
            lbl,
            gi.modal_rect.x + 18,
            ry,
            11.5,
            false,
            pal.text_muted,
            gi.modal_rect,
        );
        let fv = Canvas::fit_text(&val, max_vw, 11.5, false);
        c.draw_text(&fv, val_x, ry, 11.5, false, pal.text_primary, gi.modal_rect);
        ry += 20;
    }
}

fn render_get_info_more_and_preview(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gi: &GetInfoLayout,
    gs: &crate::state::GetInfoState,
    pal: &Palette,
) {
    let assigned = state.tags_for(&gs.target_path);
    let val_x = gi.modal_rect.x + 92;

    c.fill_rect(
        Rect::new(
            gi.modal_rect.x + 14,
            gi.more_toggle_rect.y - 6,
            gi.modal_rect.w - 28,
            1,
        ),
        pal.divider,
    );
    c.draw_text(
        "More Info:",
        gi.modal_rect.x + 16,
        gi.more_toggle_rect.y,
        12.0,
        true,
        pal.text_primary,
        gi.modal_rect,
    );
    c.draw_text(
        if gs.more_expanded { "Hide" } else { "Show" },
        gi.more_toggle_rect.x + 14,
        gi.more_toggle_rect.y + 1,
        11.0,
        true,
        pal.accent,
        gi.more_toggle_rect,
    );

    if gs.more_expanded
        && let Some(info) = &gs.info
    {
        let mut my = gi.more_toggle_rect.y + 22;
        if let Some((dw, dh)) = info.dimensions {
            c.draw_text(
                "Dimensions",
                gi.modal_rect.x + 18,
                my,
                11.5,
                false,
                pal.text_muted,
                gi.modal_rect,
            );
            c.draw_text(
                &format!("{dw} x {dh}"),
                val_x,
                my,
                11.5,
                false,
                pal.text_primary,
                gi.modal_rect,
            );
            my += 19;
        }
        c.draw_text(
            "Access",
            gi.modal_rect.x + 18,
            my,
            11.5,
            false,
            pal.text_muted,
            gi.modal_rect,
        );
        c.draw_text(
            &info.permissions,
            val_x,
            my,
            11.5,
            false,
            pal.text_primary,
            gi.modal_rect,
        );
    }

    c.fill_rounded_rect(gi.preview_rect, 8, pal.row_alt);
    c.stroke_rounded_rect(gi.preview_rect, 8, pal.divider);
    if let Some(entry) = state.selected_entry()
        && entry.path == gs.target_path
    {
        if let Some(CachedPreview::Image(img)) = state.cached_preview_for(entry, 96, 96) {
            draw_framed_thumbnail(c, img, gi.preview_rect, pal);
        } else {
            #[allow(clippy::cast_precision_loss)]
            draw_large_entry_artwork(
                c,
                gi.preview_rect.x as f32 + gi.preview_rect.w as f32 * 0.5,
                gi.preview_rect.y as f32 + gi.preview_rect.h as f32 * 0.52,
                54.0,
                entry,
                assigned.first().copied(),
                pal,
            );
        }
    }
}

fn render_go_to_folder_sheet(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gt_layout: &GoToFolderLayout,
    pal: &Palette,
) {
    let Some(gt) = &state.go_to_folder else {
        return;
    };
    draw_modal_shadow_and_frame(c, gt_layout.modal_rect, 12, pal);

    c.with_clip(gt_layout.modal_rect, |c| {
        c.fill_rounded_rect(gt_layout.input_rect, 8, pal.row_alt);
        c.stroke_rounded_rect(gt_layout.input_rect, 8, pal.accent);
        let fitted = Canvas::fit_text(&gt.input, gt_layout.input_rect.w - 36, 13.0, false);
        c.draw_text(
            &fitted,
            gt_layout.input_rect.x + 10,
            gt_layout.input_rect.y + 8,
            13.0,
            false,
            pal.text_primary,
            gt_layout.input_rect,
        );
        if state.caret_visible {
            let caret_x = gt_layout.input_rect.x + 10 + Canvas::measure_text(&fitted, 13.0, false);
            c.fill_rect(
                Rect::new(caret_x, gt_layout.input_rect.y + 6, 2, 18),
                pal.accent,
            );
        }

        #[allow(clippy::cast_precision_loss)]
        {
            c.fill_circle_f(
                gt_layout.clear_rect.x as f32 + 10.0,
                gt_layout.clear_rect.y as f32 + 10.0,
                7.5,
                pal.text_muted,
            );
            draw_close_x_glyph(
                c,
                gt_layout.clear_rect.x as f32 + 10.0,
                gt_layout.clear_rect.y as f32 + 10.0,
                3.0,
                pal.window_bg,
            );
        }

        let mut hdr_y = gt_layout.input_rect.bottom() + 8;
        if let Some(err) = &gt.error {
            c.draw_text(
                err,
                gt_layout.modal_rect.x + 14,
                hdr_y,
                11.5,
                true,
                rgba(220, 52, 46, 255),
                gt_layout.modal_rect,
            );
            hdr_y += 22;
        }
        c.draw_text(
            "Go to:",
            gt_layout.modal_rect.x + 14,
            hdr_y,
            11.0,
            true,
            pal.text_muted,
            gt_layout.modal_rect,
        );

        for (idx, rect) in &gt_layout.suggestions {
            let Some(path) = gt.suggestions.get(*idx) else {
                continue;
            };
            let selected = *idx == gt.selected_suggestion;
            if selected {
                c.fill_rounded_rect(*rect, 6, pal.accent);
            }
            let text_col = if selected {
                pal.accent_text
            } else {
                pal.text_primary
            };
            let crumb_str = path.display().to_string().replace('/', " > ");
            let clean = crumb_str.trim_start_matches(" > ").to_owned();
            let fitted_sugg = Canvas::fit_text(&clean, rect.w - 16, 12.0, selected);
            c.draw_text(
                &fitted_sugg,
                rect.x + 8,
                rect.y + 6,
                12.0,
                selected,
                text_col,
                *rect,
            );
        }
    });
}

fn render_transfer_progress_card(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    tr_layout: &TransferDialogLayout,
    pal: &Palette,
) {
    let Some(tr) = &state.active_transfer else {
        return;
    };
    draw_modal_shadow_and_frame(c, tr_layout.card_rect, 12, pal);

    c.with_clip(tr_layout.card_rect, |c| {
        let p = &tr.progress;
        let verb = match p.kind {
            TransferKind::Copy => "Copying",
            TransferKind::Move => "Moving",
        };
        let dest_name = p
            .destination_dir
            .file_name()
            .map_or_else(|| String::from("/"), |s| s.to_string_lossy().into_owned());
        let title = format!("{verb} \"{}\" to \"{dest_name}\"", p.current_item_name);
        let fitted = Canvas::fit_text(&title, tr_layout.card_rect.w - 32, 12.5, true);
        c.draw_text(
            &fitted,
            tr_layout.card_rect.x + 16,
            tr_layout.card_rect.y + 12,
            12.5,
            true,
            pal.text_primary,
            tr_layout.card_rect,
        );

        c.fill_rounded_rect(tr_layout.bar_rect, 3, pal.row_alt);
        let frac_u64 = p
            .completed_bytes
            .min(p.total_bytes)
            .saturating_mul(1000)
            .checked_div(p.total_bytes)
            .or_else(|| {
                p.completed_items
                    .min(p.total_items)
                    .saturating_mul(1000)
                    .checked_div(p.total_items)
                    .and_then(|v| u64::try_from(v).ok())
            })
            .unwrap_or(0);
        let frac = i32::try_from(frac_u64).unwrap_or(1000);
        let fill_w = ((tr_layout.bar_rect.w * frac) / 1000).clamp(4, tr_layout.bar_rect.w);
        if fill_w > 0 {
            c.fill_rounded_rect(
                Rect::new(
                    tr_layout.bar_rect.x,
                    tr_layout.bar_rect.y,
                    fill_w,
                    tr_layout.bar_rect.h,
                ),
                3,
                pal.accent,
            );
        }

        c.fill_rounded_rect(tr_layout.stop_rect, 10, pal.pill_bg);
        c.stroke_rounded_rect(tr_layout.stop_rect, 10, pal.pill_border);
        #[allow(clippy::cast_precision_loss)]
        draw_close_x_glyph(
            c,
            tr_layout.stop_rect.x as f32 + 10.0,
            tr_layout.stop_rect.y as f32 + 10.0,
            3.2,
            pal.text_secondary,
        );

        let sub = format!(
            "{} of {}  -  {} of {} items",
            format_byte_size(p.completed_bytes),
            format_byte_size(p.total_bytes),
            p.completed_items,
            p.total_items
        );
        let sub_fit = Canvas::fit_text(&sub, tr_layout.card_rect.w - 32, 11.0, false);
        c.draw_text(
            &sub_fit,
            tr_layout.card_rect.x + 16,
            tr_layout.card_rect.y + 50,
            11.0,
            false,
            pal.text_muted,
            tr_layout.card_rect,
        );
    });
}

fn render_conflict_dialog(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    cf_layout: &ConflictDialogLayout,
    pal: &Palette,
) {
    let Some(tr) = &state.active_transfer else {
        return;
    };
    let Some(conflict) = &tr.conflict else {
        return;
    };
    let win = Rect::new(0, 0, c.w, c.h);
    c.fill_rect(win, rgba(16, 18, 26, if pal.is_dark { 130 } else { 78 }));
    draw_modal_shadow_and_frame(c, cf_layout.modal_rect, 14, pal);

    c.with_clip(cf_layout.modal_rect, |c| {
        let name = conflict
            .dest
            .file_name()
            .map_or("item", |s| s.to_str().unwrap_or("item"));
        let src_name = conflict
            .source
            .file_name()
            .map_or("source", |s| s.to_str().unwrap_or("source"));
        let msg = Canvas::fit_text(
            &format!("An item named \"{name}\" already exists here."),
            cf_layout.modal_rect.w - 36,
            13.0,
            true,
        );
        c.draw_text(
            &msg,
            cf_layout.modal_rect.x + 18,
            cf_layout.modal_rect.y + 22,
            13.0,
            true,
            pal.text_primary,
            cf_layout.modal_rect,
        );
        let sub = Canvas::fit_text(
            &format!("Do you want to replace it with \"{src_name}\"?"),
            cf_layout.modal_rect.w - 36,
            11.5,
            false,
        );
        c.draw_text(
            &sub,
            cf_layout.modal_rect.x + 18,
            cf_layout.modal_rect.y + 48,
            11.5,
            false,
            pal.text_secondary,
            cf_layout.modal_rect,
        );

        let btns = [
            (cf_layout.keep_both_rect, "Keep Both", false),
            (cf_layout.skip_rect, "Skip", false),
            (cf_layout.stop_rect, "Stop", false),
            (cf_layout.replace_rect, "Replace", true),
        ];
        for (rect, label, primary) in btns {
            c.fill_rounded_rect(rect, 7, if primary { pal.accent } else { pal.pill_bg });
            if !primary {
                c.stroke_rounded_rect(rect, 7, pal.pill_border);
            }
            let tw = Canvas::measure_text(label, 12.0, primary);
            c.draw_text(
                label,
                rect.x + (rect.w - tw) / 2,
                rect.y + 6,
                12.0,
                primary,
                if primary {
                    pal.accent_text
                } else {
                    pal.text_primary
                },
                rect,
            );
        }
    });
}

fn draw_modal_shadow_and_frame(c: &mut Canvas<'_>, rect: Rect, radius: i32, pal: &Palette) {
    let shadow = Rect::new(rect.x - 2, rect.y + 2, rect.w + 4, rect.h + 5);
    c.fill_rounded_rect(
        shadow,
        radius + 1,
        rgba(0, 0, 0, if pal.is_dark { 96 } else { 42 }),
    );
    c.fill_rounded_rect(rect, radius, pal.popup_bg);
    c.stroke_rounded_rect(rect, radius, pal.popup_border);
}
