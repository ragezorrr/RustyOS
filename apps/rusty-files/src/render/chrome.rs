//! Window chrome rendering (CSD traffic lights, toolbar, search scope bar, tab strip, path bar).

use crate::icons::{
    draw_close_x_glyph, draw_ellipsis_glyph, draw_nav_chevron_svg, draw_plus_glyph,
    draw_search_glyph, draw_share_glyph, draw_sort_pill_glyph, draw_tag_outline_glyph,
    draw_view_mode_icon,
};
use crate::layout::{
    FilesHit, FilesLayout, PathCrumbLayout, Rect, SearchScopeBarLayout, TabBarLayout,
};
use crate::render::canvas::{
    Canvas, make_ui_canvas, to_ui_rect, ui_theme_palette, with_font_engine,
};
use crate::render::sidebar::draw_glass_pill;
use crate::render::{Palette, rgba};
use crate::state::{FilesAppState, SearchScope};

/// Renders the RustyOS segmented `Tabs` bar beneath the toolbar.
pub(crate) fn render_tab_bar(c: &mut Canvas<'_>, tb: &TabBarLayout, pal: &Palette) {
    let track_bg = if pal.is_dark {
        rgba(48, 48, 54, 255)
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
        // Mini RustyOS folder glyph before label
        let icon_col = if pal.is_dark {
            rgba(72, 164, 255, 255)
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

fn toolbar_hover_fill(pal: &Palette) -> [u8; 4] {
    if pal.is_dark {
        rgba(255, 255, 255, 22)
    } else {
        rgba(0, 0, 0, 16)
    }
}

fn fill_toolbar_segment_hover(c: &mut Canvas<'_>, rect: Rect, pal: &Palette) {
    let inset = Rect::new(rect.x + 2, rect.y + 2, rect.w - 4, rect.h - 4);
    c.fill_rounded_rect(inset, 14, toolbar_hover_fill(pal));
}

pub(crate) fn render_toolbar(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    draw_glass_pill(c, layout.nav_pill_rect, 16, pal);
    if state.can_go_back() && state.hovered_hit == Some(FilesHit::NavBack) {
        fill_toolbar_segment_hover(c, layout.nav_back_rect, pal);
    }
    if state.can_go_forward() && state.hovered_hit == Some(FilesHit::NavForward) {
        fill_toolbar_segment_hover(c, layout.nav_forward_rect, pal);
    }
    c.fill_rect(
        Rect::new(
            layout.nav_pill_rect.x + layout.nav_back_rect.w,
            layout.nav_pill_rect.y + 7,
            1,
            18,
        ),
        pal.divider,
    );
    let back_col = if state.can_go_back() {
        pal.text_primary
    } else {
        pal.text_muted
    };
    let fwd_col = if state.can_go_forward() {
        pal.text_primary
    } else {
        pal.text_muted
    };
    #[allow(clippy::cast_precision_loss)]
    {
        draw_nav_chevron_svg(
            c,
            layout.nav_back_rect.x as f32 + (layout.nav_back_rect.w as f32) * 0.5,
            layout.nav_back_rect.y as f32 + (layout.nav_back_rect.h as f32) * 0.5,
            true,
            back_col,
        );
        draw_nav_chevron_svg(
            c,
            layout.nav_forward_rect.x as f32 + (layout.nav_forward_rect.w as f32) * 0.5,
            layout.nav_forward_rect.y as f32 + (layout.nav_forward_rect.h as f32) * 0.5,
            false,
            fwd_col,
        );
    }

    if layout.title_max_x - layout.title_x >= 28 {
        let title_clip = Rect::new(layout.title_x, 12, layout.title_max_x - layout.title_x, 38);
        let title = Canvas::fit_text(&state.window_title, title_clip.w, 15.0, true);
        c.draw_text(
            &title,
            layout.title_x,
            21,
            15.0,
            true,
            pal.text_primary,
            title_clip,
        );
    }
    render_toolbar_right_cluster(c, state, layout, pal);
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn render_toolbar_right_cluster(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    // 4-segment View Switcher pill (`Icons` | `List` | `Columns` | `Gallery`)
    draw_glass_pill(c, layout.view_pill_rect, 16, pal);
    for (mode, seg_rect) in layout.view_segments {
        let active = state.view_mode == mode;
        let hovered = state.hovered_hit == Some(FilesHit::ViewMode(mode));
        if active {
            let pill = Rect::new(
                seg_rect.x + 2,
                seg_rect.y + 2,
                seg_rect.w - 4,
                seg_rect.h - 4,
            );
            c.fill_rounded_rect(pill, 14, pal.pill_active);
        } else if hovered {
            fill_toolbar_segment_hover(c, seg_rect, pal);
        }
        let col = if active || hovered {
            pal.text_primary
        } else {
            pal.text_secondary
        };
        draw_view_mode_icon(
            c,
            seg_rect.x as f32 + (seg_rect.w as f32) * 0.5,
            seg_rect.y as f32 + (seg_rect.h as f32) * 0.5,
            mode,
            col,
        );
    }

    // Group By / Sort pill
    draw_glass_pill(c, layout.sort_pill_rect, 16, pal);
    if state.hovered_hit == Some(FilesHit::SortButton) {
        fill_toolbar_segment_hover(c, layout.sort_pill_rect, pal);
    }
    draw_sort_pill_glyph(
        c,
        layout.sort_pill_rect.x as f32 + (layout.sort_pill_rect.w as f32) * 0.5,
        layout.sort_pill_rect.y as f32 + (layout.sort_pill_rect.h as f32) * 0.5,
        pal.text_primary,
    );

    // Actions pill (`Share` | `Tag` | `...`)
    draw_glass_pill(c, layout.action_pill_rect, 16, pal);
    if state.hovered_hit == Some(FilesHit::ActionPrimary) {
        fill_toolbar_segment_hover(c, layout.action_primary_rect, pal);
    }
    if state.hovered_hit == Some(FilesHit::ActionTagPicker) {
        fill_toolbar_segment_hover(c, layout.action_tag_rect, pal);
    }
    if state.hovered_hit == Some(FilesHit::ActionMoreMenu) {
        fill_toolbar_segment_hover(c, layout.action_more_rect, pal);
    }
    draw_share_glyph(
        c,
        layout.action_primary_rect.x as f32 + (layout.action_primary_rect.w as f32) * 0.5,
        layout.action_primary_rect.y as f32 + (layout.action_primary_rect.h as f32) * 0.5,
        pal.text_primary,
    );
    draw_tag_outline_glyph(
        c,
        layout.action_tag_rect.x as f32 + (layout.action_tag_rect.w as f32) * 0.5,
        layout.action_tag_rect.y as f32 + (layout.action_tag_rect.h as f32) * 0.5,
        pal.text_primary,
    );
    draw_ellipsis_glyph(
        c,
        layout.action_more_rect.x as f32 + (layout.action_more_rect.w as f32) * 0.5,
        layout.action_more_rect.y as f32 + (layout.action_more_rect.h as f32) * 0.5,
        pal.text_primary,
    );

    render_toolbar_search_pill(c, state, layout, pal);
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn render_toolbar_search_pill(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    let pill = layout.search_pill_rect;
    let wide = pill.w >= 72;
    let show_text = state.search_active || !state.search_query.is_empty() || wide;
    if !show_text {
        draw_glass_pill(c, pill, 16, pal);
        if state.hovered_hit == Some(FilesHit::SearchToggle) {
            fill_toolbar_segment_hover(c, pill, pal);
        }
        let icon_cx = pill.x as f32 + (pill.w as f32) * 0.5;
        draw_search_glyph(
            c,
            icon_cx,
            pill.y as f32 + (pill.h as f32) * 0.5,
            pal.text_primary,
        );
        return;
    }

    let mut sf = state.search_field.clone();
    sf.rect = to_ui_rect(pill);
    sf.corner_radius = 16.0;
    sf.font_size = 12.5;
    sf.padding_x = 26;
    sf.clearable = true;
    sf.search_style = true;
    if sf.buffer.as_str() != state.search_query {
        sf.buffer.set_text(&state.search_query);
    }
    sf.caret_blink.reset(std::time::Duration::ZERO);
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
        sf.paint(
            &mut painter,
            fe,
            &ui_pal,
            state.search_active,
            1.0,
            blink_ts,
        );
        rusty_ui::render_draw_list_to_canvas(&dl, &mut uc, fe);
    });
}

pub(crate) fn render_search_scope_bar(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    sb: &SearchScopeBarLayout,
    pal: &Palette,
) {
    c.fill_rect(
        Rect::new(
            sb.bar_rect.x + 4,
            sb.bar_rect.bottom() - 1,
            sb.bar_rect.w - 8,
            1,
        ),
        pal.divider,
    );
    c.draw_text(
        "Search:",
        sb.bar_rect.x + 8,
        sb.bar_rect.y + 6,
        12.0,
        false,
        pal.text_secondary,
        sb.bar_rect,
    );
    draw_scope_chip(
        c,
        sb.this_folder_rect,
        &sb.this_folder_label,
        state.search_scope == SearchScope::ThisFolder,
        pal,
    );
    draw_scope_chip(
        c,
        sb.home_rect,
        "Home",
        state.search_scope == SearchScope::Home,
        pal,
    );
    if let Some(clear) = sb.clear_rect {
        c.fill_rounded_rect(clear, 10, pal.pill_bg);
        let tw = Canvas::measure_text("Clear", 11.5, false);
        c.draw_text(
            "Clear",
            clear.x + (clear.w - tw) / 2,
            clear.y + 4,
            11.5,
            false,
            pal.text_secondary,
            clear,
        );
    }
}

pub(crate) fn draw_scope_chip(
    c: &mut Canvas<'_>,
    rect: Rect,
    label: &str,
    selected: bool,
    pal: &Palette,
) {
    if selected {
        c.fill_rounded_rect(rect, 11, pal.pill_active);
    }
    let tw = Canvas::measure_text(label, 12.0, selected);
    let col = if selected {
        pal.text_primary
    } else {
        pal.text_secondary
    };
    c.draw_text(
        label,
        rect.x + (rect.w - tw) / 2,
        rect.y + 4,
        12.0,
        selected,
        col,
        rect,
    );
}
