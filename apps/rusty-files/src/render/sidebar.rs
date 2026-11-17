//! Golden Gate translucent sidebar and contextual warning banner rendering.

use crate::icons::draw_sidebar_icon;
use crate::layout::{FilesLayout, Rect};
use crate::render::canvas::Canvas;
use crate::render::{Palette, overlay_scrollbar_color, rgba};
use crate::state::{FilesAppState, sidebar_total_height};

pub(crate) fn render_sidebar(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    // Full-bleed edge-to-edge Finder sidebar (`x=0, y=0..height`, clipped to window corners).
    c.fill_rect(layout.sidebar_rect, pal.sidebar_bg);
    c.fill_rect(
        Rect::new(
            layout.sidebar_rect.right() - 1,
            layout.sidebar_rect.y,
            1,
            layout.sidebar_rect.h,
        ),
        pal.sidebar_border,
    );

    // Golden Gate traffic lights concentric with top-left window corner radius (strictly above clip)
    c.fill_circle_f(25.0, 25.0, 6.0, rgba(255, 95, 87, 255));
    c.fill_circle_f(45.0, 25.0, 6.0, rgba(254, 188, 46, 255));
    c.fill_circle_f(65.0, 25.0, 6.0, rgba(40, 200, 64, 255));

    let clip = layout.sidebar_clip_rect;
    c.with_clip(clip, |c| {
        for row in &layout.sidebar_rows {
            let item = &state.sidebar_items[row.index];
            if let Some(hdr_y) = row.section_header_y
                && let Some(section) = item.section
            {
                c.draw_text(
                    section,
                    row.rect.x + 8,
                    hdr_y,
                    11.0,
                    true,
                    pal.text_muted,
                    clip,
                );
            }

            let selected = state.active_sidebar.as_ref() == Some(&item.location);
            if selected {
                c.fill_rounded_rect(row.rect, 7, pal.sidebar_active);
            }
            #[allow(clippy::cast_precision_loss)]
            draw_sidebar_icon(
                c,
                row.rect.x as f32 + 15.0,
                row.rect.y as f32 + (row.rect.h as f32) * 0.5,
                item.icon,
                pal,
            );
            let label = Canvas::fit_text(&item.label, row.rect.w - 36, 13.0, selected);
            c.draw_text(
                &label,
                row.rect.x + 29,
                row.rect.y + 3,
                13.0,
                selected,
                pal.text_primary,
                clip,
            );
        }
        let total_h = sidebar_total_height(&state.sidebar_items);
        c.draw_scrollbar(
            clip,
            state.sidebar_scroll_y,
            total_h,
            overlay_scrollbar_color(pal.scrollbar, state.overlay_scrollbar_alpha),
        );
    });
}

pub(crate) fn draw_glass_pill(c: &mut Canvas<'_>, rect: Rect, radius: i32, pal: &Palette) {
    c.fill_rounded_rect(rect, radius, pal.pill_bg);
    if rect.w > radius * 2 && rect.h > 4 {
        let (top_alpha, bot_alpha) = if pal.is_dark { (38, 14) } else { (110, 38) };
        let span = rect.w - 2 * radius + 6;
        c.fill_rect(
            Rect::new(rect.x + radius - 3, rect.y + 1, span, 1),
            rgba(255, 255, 255, top_alpha),
        );
        c.fill_rect(
            Rect::new(rect.x + radius - 3, rect.y + rect.h - 2, span, 1),
            rgba(255, 255, 255, bot_alpha),
        );
    }
    c.stroke_rounded_rect(rect, radius, pal.pill_border);
}

pub(crate) fn render_trash_banner(
    c: &mut Canvas<'_>,
    layout: &FilesLayout,
    banner: Rect,
    pal: &Palette,
) {
    c.fill_rounded_rect(banner, 8, pal.row_alt);
    c.draw_text(
        "Trash — deleted files remain here until you empty Trash",
        banner.x + 12,
        banner.y + 8,
        12.5,
        false,
        pal.text_secondary,
        banner,
    );
    if let Some(btn) = layout.empty_trash_btn_rect {
        c.fill_rounded_rect(btn, 12, pal.pill_bg);
        c.stroke_rounded_rect(btn, 12, pal.pill_border);
        c.draw_text(
            "Empty",
            btn.x + 16,
            btn.y + 4,
            12.0,
            true,
            pal.text_primary,
            btn,
        );
    }
}
