//! Modal dialogs and overlay sheets rendering (Quick Look, Get Info, Go to Folder, Rename, Conflict).

use rusty_services::file_manager::{TransferKind, format_byte_size, format_unix_timestamp};

use crate::icons::{draw_chevron_aa, draw_close_x_glyph, draw_large_entry_artwork};
use crate::layout::{
    BatchRenameLayout, ConflictDialogLayout, FilesLayout, GetInfoLayout, GoToFolderLayout,
    QuickLookLayout, Rect, TransferDialogLayout,
};
use crate::render::canvas::{
    Canvas, make_ui_canvas, to_ui_rect, ui_theme_palette, with_font_engine,
};
use crate::render::views::{
    draw_aspect_fitted_preview, draw_framed_thumbnail, render_text_preview_card,
};
use crate::render::{Palette, palette_for_with_accent, rgba};
use crate::state::{BatchRenameMode, CachedPreview, FilesAppState, InlineRenameState};

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
    if let Some(br) = &layout.overlays.batch_rename {
        render_batch_rename_sheet(c, state, br, pal);
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

pub(crate) fn render_inline_rename_editor(
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
    let mut rf = state.rename_field.clone();
    rf.rect = to_ui_rect(box_rect);
    rf.corner_radius = 6.0;
    rf.font_size = 12.5;
    rf.padding_x = 6;
    if rf.buffer.as_str() != rn.draft {
        rf.buffer.set_text(&rn.draft);
    }
    rf.caret_blink.reset(std::time::Duration::ZERO);
    let blink_ts = if state.caret_visible {
        std::time::Duration::ZERO
    } else {
        std::time::Duration::from_millis(600)
    };
    let mut ui_pal = ui_theme_palette(pal, state.accent_index);
    if rn.error.is_some() {
        ui_pal.accent = rgba(235, 55, 48, 255);
    }
    let mut dl = rusty_ui::UiDrawList::new();
    let mut uc = make_ui_canvas(c.buf, c.w, c.h);
    with_font_engine(|fe| {
        let mut painter = rusty_ui::UiPainter::new(&mut dl);
        rf.paint(&mut painter, fe, &ui_pal, true, 1.0, blink_ts);
        rusty_ui::render_draw_list_to_canvas(&dl, &mut uc, fe);
    });
}

pub(crate) fn render_quick_look_overlay(
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

pub(crate) fn render_quick_look_stage(
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

pub(crate) fn render_get_info_window(
    buf: &mut [u8],
    width: u32,
    height: u32,
    state: &FilesAppState,
    gi: &GetInfoLayout,
    has_glass: bool,
) {
    let pal = palette_for_with_accent(state.theme, state.accent_index);
    let mut c = Canvas::new(buf, width, height);
    c.clear();
    let r = gi.modal_rect;
    let bg = if has_glass && !state.reduce_transparency {
        let a = u8::try_from((u16::from(pal.popup_bg[3]) * 78) / 100).unwrap_or(200);
        rgba(pal.popup_bg[0], pal.popup_bg[1], pal.popup_bg[2], a)
    } else {
        rgba(pal.popup_bg[0], pal.popup_bg[1], pal.popup_bg[2], 248)
    };
    c.fill_rounded_rect(r, 12, bg);
    c.stroke_rounded_rect(r, 12, pal.popup_border);
    render_get_info_body(&mut c, state, gi, &pal);
}

pub(crate) fn render_get_info_sheet(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gi: &GetInfoLayout,
    pal: &Palette,
) {
    if state.get_info.is_none() {
        return;
    }
    draw_modal_shadow_and_frame(c, gi.modal_rect, 12, pal);
    render_get_info_body(c, state, gi, pal);
}

fn render_get_info_body(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    gi: &GetInfoLayout,
    pal: &Palette,
) {
    let Some(gs) = &state.get_info else {
        return;
    };

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

pub(crate) fn render_get_info_tags_and_general(
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
        let kind_label = match crate::state::classify_executable_file(&gs.target_path) {
            crate::state::ExecutableFileState::Runnable
            | crate::state::ExecutableFileState::BinaryMissingExecBit => {
                String::from("Unix Executable")
            }
            _ => info.kind_label.clone(),
        };
        rows.push(("Kind", kind_label));
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

pub(crate) fn render_get_info_more_and_preview(
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

    if gi.preview_rect.h >= 32 {
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
}

pub(crate) fn render_go_to_folder_sheet(
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
        let mut gf = state.goto_field.clone();
        gf.rect = to_ui_rect(gt_layout.input_rect);
        gf.corner_radius = 8.0;
        gf.font_size = 13.0;
        gf.padding_x = 10;
        gf.clearable = false;
        if gf.buffer.as_str() != gt.input {
            gf.buffer.set_text(&gt.input);
        }
        gf.caret_blink.reset(std::time::Duration::ZERO);
        let blink_ts = if state.caret_visible {
            std::time::Duration::ZERO
        } else {
            std::time::Duration::from_millis(600)
        };
        let ui_pal = ui_theme_palette(pal, state.accent_index);
        let mut dl = rusty_ui::UiDrawList::new();
        let mut uc = make_ui_canvas(c.buf, c.w, c.h);
        with_font_engine(|fe| {
            let mut painter = rusty_ui::UiPainter::new(&mut dl);
            painter.push_clip(to_ui_rect(gt_layout.modal_rect));
            gf.paint(&mut painter, fe, &ui_pal, true, 1.0, blink_ts);
            painter.pop_clip();
            rusty_ui::render_draw_list_to_canvas(&dl, &mut uc, fe);
        });

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

pub(crate) fn render_transfer_progress_card(
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

pub(crate) fn render_conflict_dialog(
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

        let ui_pal = ui_theme_palette(pal, state.accent_index);
        let btns = [
            (
                cf_layout.keep_both_rect,
                "Keep Both",
                rusty_ui::ButtonStyle::Secondary,
            ),
            (
                cf_layout.skip_rect,
                "Skip",
                rusty_ui::ButtonStyle::Secondary,
            ),
            (
                cf_layout.stop_rect,
                "Stop",
                rusty_ui::ButtonStyle::Secondary,
            ),
            (
                cf_layout.replace_rect,
                "Replace",
                rusty_ui::ButtonStyle::Primary,
            ),
        ];
        let mut dl = rusty_ui::UiDrawList::new();
        let mut uc = make_ui_canvas(c.buf, c.w, c.h);
        with_font_engine(|fe| {
            let mut painter = rusty_ui::UiPainter::new(&mut dl);
            painter.push_clip(to_ui_rect(cf_layout.modal_rect));
            for (rect, label, style) in btns {
                let btn =
                    rusty_ui::PushButton::new(to_ui_rect(rect), style).with_corner_radius(7.0);
                btn.paint_labeled(
                    &mut painter,
                    fe,
                    label,
                    &ui_pal,
                    style == rusty_ui::ButtonStyle::Primary,
                );
            }
            painter.pop_clip();
            rusty_ui::render_draw_list_to_canvas(&dl, &mut uc, fe);
        });
    });
}

#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
pub(crate) fn draw_modal_shadow_and_frame(
    c: &mut Canvas<'_>,
    rect: Rect,
    radius: i32,
    pal: &Palette,
) {
    let shadow_rect =
        rusty_ui::Rect::new(rect.x, rect.y, rect.w.max(0) as u32, rect.h.max(0) as u32);
    rusty_ui::draw_elevation_shadow_with(
        shadow_rect,
        u32::try_from(radius).unwrap_or(0),
        rusty_ui::ShadowElevation::Popup,
        pal.is_dark,
        |sr, rad, col| {
            c.fill_rounded_rect(
                Rect::new(sr.x, sr.y, sr.width as i32, sr.height as i32),
                rad,
                col,
            );
        },
    );
    c.fill_rounded_rect(rect, radius, pal.popup_bg);
    c.stroke_rounded_rect(rect, radius, pal.popup_border);
}

#[allow(clippy::too_many_lines)]
pub(crate) fn render_batch_rename_sheet(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    br_layout: &BatchRenameLayout,
    pal: &Palette,
) {
    let Some(br) = &state.batch_rename else {
        return;
    };
    draw_modal_shadow_and_frame(c, br_layout.modal_rect, 12, pal);
    let m = br_layout.modal_rect;
    c.with_clip(m, |c| {
        let title = format!("Rename {} Finder Items", br.targets.len());
        c.draw_text(&title, m.x + 16, m.y + 12, 13.5, true, pal.text_primary, m);

        for (mode, rect) in br_layout.mode_tabs {
            let active = br.mode == mode;
            c.fill_rounded_rect(rect, 6, if active { pal.accent } else { pal.pill_bg });
            let tw = Canvas::measure_text(mode.label(), 11.5, active);
            c.draw_text(
                mode.label(),
                rect.x + (rect.w - tw) / 2,
                rect.y + 5,
                11.5,
                active,
                if active {
                    pal.accent_text
                } else {
                    pal.text_primary
                },
                rect,
            );
        }

        let (label0, val0) = match br.mode {
            BatchRenameMode::ReplaceText => ("Find:", br.find_text.as_str()),
            BatchRenameMode::AddText => ("Add text:", br.add_text.as_str()),
            BatchRenameMode::FormatIndex => ("Name format:", br.format_base.as_str()),
        };
        render_batch_rename_input(
            c,
            br_layout.field0_rect,
            label0,
            val0,
            br.active_field == 0,
            pal,
        );

        if let Some(f1) = br_layout.field1_rect {
            let start_str = br.start_index.to_string();
            let (label1, val1) = match br.mode {
                BatchRenameMode::ReplaceText => ("Replace:", br.replace_text.as_str()),
                BatchRenameMode::FormatIndex => ("Start at:", start_str.as_str()),
                BatchRenameMode::AddText => ("", ""),
            };
            render_batch_rename_input(c, f1, label1, val1, br.active_field == 1, pal);
        }

        if let Some(pr) = br_layout.prefix_toggle_rect {
            c.fill_rounded_rect(pr, 6, pal.pill_bg);
            let pos_label = if br.add_as_prefix {
                "Placement: Before Name"
            } else {
                "Placement: After Name"
            };
            c.draw_text(
                pos_label,
                pr.x + 8,
                pr.y + 5,
                11.5,
                false,
                pal.text_primary,
                pr,
            );
        }

        let sample_old = br
            .targets
            .first()
            .and_then(|p| p.file_name())
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let sample_new = br.compute_target_name(&sample_old, 0);
        let msg = br
            .error
            .as_ref()
            .map_or_else(|| format!("Example: \"{sample_new}\""), Clone::clone);
        let msg_col = if br.error.is_some() {
            rgba(230, 62, 56, 255)
        } else {
            pal.text_secondary
        };
        let fitted = Canvas::fit_text(&msg, m.w - 32, 11.5, false);
        c.draw_text(&fitted, m.x + 16, m.y + 150, 11.5, false, msg_col, m);

        let ui_pal = ui_theme_palette(pal, state.accent_index);
        let mut dl = rusty_ui::UiDrawList::new();
        let mut uc = make_ui_canvas(c.buf, c.w, c.h);
        with_font_engine(|fe| {
            let mut painter = rusty_ui::UiPainter::new(&mut dl);
            painter.push_clip(to_ui_rect(m));
            let cancel_btn = rusty_ui::PushButton::new(
                to_ui_rect(br_layout.cancel_rect),
                rusty_ui::ButtonStyle::Secondary,
            )
            .with_corner_radius(7.0);
            cancel_btn.paint_labeled(&mut painter, fe, "Cancel", &ui_pal, false);
            let ok_btn = rusty_ui::PushButton::new(
                to_ui_rect(br_layout.confirm_rect),
                rusty_ui::ButtonStyle::Primary,
            )
            .with_corner_radius(7.0);
            ok_btn.paint_labeled(&mut painter, fe, "Rename", &ui_pal, true);
            painter.pop_clip();
            rusty_ui::render_draw_list_to_canvas(&dl, &mut uc, fe);
        });
    });
}

fn render_batch_rename_input(
    c: &mut Canvas<'_>,
    rect: Rect,
    label: &str,
    value: &str,
    focused: bool,
    pal: &Palette,
) {
    c.fill_rounded_rect(rect, 7, pal.pill_bg);
    c.stroke_rounded_rect(rect, 7, if focused { pal.accent } else { pal.divider });
    c.draw_text(
        label,
        rect.x + 8,
        rect.y + 6,
        11.5,
        true,
        pal.text_secondary,
        rect,
    );
    let label_w = Canvas::measure_text(label, 11.5, true);
    let val_x = rect.x + 16 + label_w;
    let fitted = Canvas::fit_text(value, (rect.right() - val_x - 8).max(40), 12.0, false);
    c.draw_text(
        &fitted,
        val_x,
        rect.y + 6,
        12.0,
        false,
        pal.text_primary,
        rect,
    );
}
