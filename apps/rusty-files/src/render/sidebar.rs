//! RustyOS translucent sidebar and contextual warning banner rendering.

use crate::icons::{draw_eject_icon, draw_section_chevron_svg, draw_sidebar_icon};
use crate::layout::{FilesHit, FilesLayout, Rect, SidebarSectionHeaderLayout};
use crate::render::canvas::Canvas;
use crate::render::{Palette, overlay_scrollbar_color, rgba};
use crate::state::{FilesAppState, sidebar_total_height};

pub(crate) fn render_sidebar(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
    sidebar_glass: bool,
) {
    // Full-bleed edge-to-edge sidebar (x=0, y=0..height, clipped to window corners).
    let sidebar_fill = if sidebar_glass {
        if pal.is_dark {
            rgba(34, 35, 39, 158)
        } else {
            rgba(242, 243, 247, 152)
        }
    } else {
        pal.sidebar_bg
    };
    c.fill_rect(layout.sidebar_rect, sidebar_fill);
    c.fill_rect(
        Rect::new(
            layout.sidebar_rect.right() - 1,
            layout.sidebar_rect.y,
            1,
            layout.sidebar_rect.h,
        ),
        pal.sidebar_border,
    );

    // Unified 14 px traffic lights matching `rusty-ui` (step 22 px, centers y = 20.0)
    let tl = rusty_ui::TrafficLightButtons::default_csd();
    for kind in [
        rusty_ui::TrafficLightKind::Close,
        rusty_ui::TrafficLightKind::Minimize,
        rusty_ui::TrafficLightKind::Maximize,
    ] {
        let color = if state.focused {
            kind.active_color()
        } else {
            rusty_ui::TrafficLightKind::inactive_color()
        };
        let (cx, cy) = tl.center_of(kind);
        c.fill_circle_f(cx, cy, 7.0, color);
        if state.traffic_hovered {
            draw_traffic_glyph(c, cx, cy, kind);
        }
    }

    let clip = layout.sidebar_clip_rect;
    c.with_clip(clip, |c| {
        for sec in &layout.sidebar_sections {
            render_sidebar_section_header(c, state, sec, pal, clip);
        }
        for row in &layout.sidebar_rows {
            render_sidebar_row(c, state, row, pal, sidebar_glass, clip);
        }
        let total_h = sidebar_total_height(&state.sidebar_items, &state.collapsed_sidebar_sections);
        c.draw_scrollbar(
            clip,
            state.sidebar_scroll_y,
            total_h,
            overlay_scrollbar_color(pal.scrollbar, state.overlay_scrollbar_alpha),
        );
    });
}

fn render_sidebar_section_header(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    sec: &SidebarSectionHeaderLayout,
    pal: &Palette,
    clip: Rect,
) {
    let hovered = state.hovered_hit == Some(FilesHit::SidebarSectionHeader(sec.title));
    if hovered {
        let hover_col = if pal.is_dark {
            rgba(255, 255, 255, 14)
        } else {
            rgba(0, 0, 0, 10)
        };
        c.fill_rounded_rect(sec.rect, 6, hover_col);
    }
    c.draw_text(
        sec.title,
        sec.rect.x + 8,
        sec.rect.y + 4,
        11.0,
        true,
        if hovered {
            pal.text_secondary
        } else {
            pal.text_muted
        },
        clip,
    );
    if sec.collapsed || hovered {
        #[allow(clippy::cast_precision_loss)]
        draw_section_chevron_svg(
            c,
            sec.rect.right() as f32 - 10.0,
            sec.rect.y as f32 + (sec.rect.h as f32) * 0.5,
            sec.collapsed,
            pal.text_muted,
        );
    }
}

fn render_sidebar_row(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    row: &crate::layout::SidebarRowLayout,
    pal: &Palette,
    sidebar_glass: bool,
    clip: Rect,
) {
    let item = &state.sidebar_items[row.index];
    let selected = state.active_sidebar.as_ref() == Some(&item.location);
    let hovered = match &state.hovered_hit {
        Some(FilesHit::SidebarItem(loc)) => loc == &item.location,
        Some(FilesHit::SidebarRemovableMount(dev) | FilesHit::SidebarRemovableEject(dev)) => {
            row.removable_device.as_deref() == Some(dev.as_str())
        }
        _ => false,
    };
    if selected {
        let pill_col = if sidebar_glass {
            if pal.is_dark {
                rgba(255, 255, 255, 34)
            } else {
                rgba(0, 0, 0, 24)
            }
        } else {
            pal.sidebar_active
        };
        c.fill_rounded_rect(row.rect, 10, pill_col);
    } else if hovered {
        let hover_col = if pal.is_dark {
            rgba(255, 255, 255, 15)
        } else {
            rgba(0, 0, 0, 12)
        };
        c.fill_rounded_rect(row.rect, 10, hover_col);
    }
    #[allow(clippy::cast_precision_loss)]
    draw_sidebar_icon(
        c,
        row.rect.x as f32 + 15.0,
        row.rect.y as f32 + (row.rect.h as f32) * 0.5,
        item.icon,
        pal,
    );
    let label_max_w = if row.eject_rect.is_some() {
        row.rect.w - 56
    } else {
        row.rect.w - 36
    };
    let label = Canvas::fit_text(&item.label, label_max_w, 13.0, selected);
    c.draw_text(
        &label,
        row.rect.x + 29,
        row.rect.y + 3,
        13.0,
        selected,
        pal.text_primary,
        clip,
    );
    if let Some(eject_r) = row.eject_rect {
        let eject_hovered = matches!(
            &state.hovered_hit,
            Some(FilesHit::SidebarRemovableEject(dev))
                if row.removable_device.as_deref() == Some(dev.as_str())
        );
        if eject_hovered {
            let bg = if pal.is_dark {
                rgba(255, 255, 255, 28)
            } else {
                rgba(0, 0, 0, 22)
            };
            c.fill_rounded_rect(eject_r, 8, bg);
        }
        #[allow(clippy::cast_precision_loss)]
        draw_eject_icon(
            c,
            eject_r.x as f32 + (eject_r.w as f32) * 0.5,
            eject_r.y as f32 + (eject_r.h as f32) * 0.5,
            if eject_hovered {
                pal.text_primary
            } else {
                pal.text_secondary
            },
        );
    }
}

pub(crate) fn draw_glass_pill(c: &mut Canvas<'_>, rect: Rect, radius: i32, pal: &Palette) {
    c.fill_rounded_rect(rect, radius, pal.pill_bg);
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

fn draw_traffic_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, kind: rusty_ui::TrafficLightKind) {
    let ink = rusty_ui::TrafficLightKind::glyph_ink();
    for seg in rusty_ui::glyph_segments(kind) {
        c.stroke_line_aa(
            cx + seg.dx0,
            cy + seg.dy0,
            cx + seg.dx1,
            cy + seg.dy1,
            0.75,
            ink,
        );
    }
}
