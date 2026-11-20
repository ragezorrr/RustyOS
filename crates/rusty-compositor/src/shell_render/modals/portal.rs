//! XDG Desktop Portal source-selection and consent modal prompt (`RUSTYOS-091`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{
    TOP_BAR_HEIGHT, ThemePalette, WINDOW_RADIUS, make_solid_element, premul,
    push_liquid_glass_panel_elevation, push_rounded_glass_box, rect_contains, truncate_ascii,
};
use crate::shell_render::top_bar::TopBarTelemetry;

use super::phys_to_logical_rect;

/// Single monitor or window candidate row inside the XDG Portal consent modal (`RUSTYOS-057`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalSourceRow<'a> {
    /// Primary label (`"Monitor: eDP-1"` or window title).
    pub label: &'a str,
    /// Secondary subtitle (resolution / refresh or `app_id`).
    pub detail: &'a str,
    /// `true` for a window candidate, `false` for a monitor output.
    pub is_window: bool,
}

/// Active XDG Desktop Portal source-selection / consent modal render state (`RUSTYOS-057`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalPromptRenderState<'a> {
    /// Requesting client application identifier (`app_id`).
    pub app_id: &'a str,
    /// `true` for `ScreenCast`, `false` for `Screenshot`.
    pub is_screencast: bool,
    /// Whether the software cursor should be embedded (`ScreenCast`).
    pub include_cursor: bool,
    /// Whether the `"Include cursor"` toggle row is displayed.
    pub cursor_mode_editable: bool,
    /// Selectable monitors and/or windows.
    pub rows: &'a [PortalSourceRow<'a>],
    /// Highlighted row index inside `rows`.
    pub selected: usize,
}

/// Interactive click targets inside the Liquid Glass XDG Portal modal (`RUSTYOS-057`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalHit {
    /// Clicked candidate row index inside `rows`.
    Row(usize),
    /// Clicked `"Include cursor"` toggle row (`ScreenCast`).
    CursorToggle,
    /// Clicked `Cancel` button.
    Cancel,
    /// Clicked `Capture` or `Share` button.
    Accept,
    /// Clicked outside interactive buttons without dismissing.
    Outside,
}

pub(crate) const PORTAL_MAX_VISIBLE_ROWS: usize = 5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct PortalModalLayout {
    pub(crate) modal: Rectangle<i32, smithay::utils::Physical>,
    pub(crate) badge: Rectangle<i32, smithay::utils::Physical>,
    pub(crate) row_start: usize,
    pub(crate) row_rects: [Rectangle<i32, smithay::utils::Physical>; PORTAL_MAX_VISIBLE_ROWS],
    pub(crate) visible_count: usize,
    pub(crate) cursor_toggle: Option<Rectangle<i32, smithay::utils::Physical>>,
    pub(crate) cancel: Rectangle<i32, smithay::utils::Physical>,
    pub(crate) accept: Rectangle<i32, smithay::utils::Physical>,
}

pub(crate) fn portal_visible_range(total: usize, selected: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let count = total.min(PORTAL_MAX_VISIBLE_ROWS);
    let sel = selected.min(total - 1);
    let max_start = total.saturating_sub(count);
    let start = sel.saturating_sub(count / 2).min(max_start);
    (start, start + count)
}

pub(crate) fn portal_modal_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    total_rows: usize,
    selected: usize,
    show_cursor_toggle: bool,
) -> PortalModalLayout {
    let w = screen_size.w.max(320);
    let h = screen_size.h.max(360);
    let mw = 460.min(w - 32).max(280);
    let (row_start, row_end) = portal_visible_range(total_rows, selected);
    let visible_count = row_end.saturating_sub(row_start);
    let list_rows = i32::try_from(visible_count.max(1)).unwrap_or(1);
    let row_h = 40;
    let row_gap = 6;
    let list_h = list_rows * row_h + (list_rows - 1) * row_gap;
    let toggle_h = if show_cursor_toggle { 34 } else { 0 };
    let mh = 96 + list_h + toggle_h + 62;
    let mx = (w - mw) / 2;
    let my = ((h - mh) / 2 - 10).max(TOP_BAR_HEIGHT + 16);

    let list_y = my + 84;
    let zero = Rectangle::new(Point::from((0, 0)), Size::from((0, 0)));
    let mut row_rects = [zero; PORTAL_MAX_VISIBLE_ROWS];
    for (idx, slot) in row_rects.iter_mut().take(visible_count).enumerate() {
        let off = i32::try_from(idx).unwrap_or(0);
        *slot = Rectangle::new(
            Point::from((mx + 22, list_y + off * (row_h + row_gap))),
            Size::from((mw - 44, row_h)),
        );
    }
    let after_list_y = list_y + list_h + 10;
    let cursor_toggle = show_cursor_toggle.then(|| {
        Rectangle::new(
            Point::from((mx + 22, after_list_y)),
            Size::from((mw - 44, 26)),
        )
    });
    let btn_w = ((mw - 60) / 2).clamp(96, 146);
    let btn_y = my + mh - 50;

    PortalModalLayout {
        modal: Rectangle::new(Point::from((mx, my)), Size::from((mw, mh))),
        badge: Rectangle::new(Point::from((mx + 22, my + 20)), Size::from((38, 38))),
        row_start,
        row_rects,
        visible_count,
        cursor_toggle,
        cancel: Rectangle::new(Point::from((mx + 22, btn_y)), Size::from((btn_w, 32))),
        accept: Rectangle::new(
            Point::from((mx + mw - 22 - btn_w, btn_y)),
            Size::from((btn_w, 32)),
        ),
    }
}

/// Hit-tests `(x, y)` inside the centered Liquid Glass XDG Portal consent dialog (`RUSTYOS-057`).
#[must_use]
pub fn hit_test_portal_dialog(
    screen_size: Size<i32, smithay::utils::Physical>,
    total_rows: usize,
    selected: usize,
    show_cursor_toggle: bool,
    x: f64,
    y: f64,
) -> PortalHit {
    let layout = portal_modal_layout(screen_size, total_rows, selected, show_cursor_toggle);
    if rect_contains(layout.accept, x, y) {
        return PortalHit::Accept;
    }
    if rect_contains(layout.cancel, x, y) {
        return PortalHit::Cancel;
    }
    if let Some(toggle_rect) = layout.cursor_toggle
        && rect_contains(toggle_rect, x, y)
    {
        return PortalHit::CursorToggle;
    }
    for (idx, rect) in layout
        .row_rects
        .iter()
        .take(layout.visible_count)
        .enumerate()
    {
        if rect_contains(*rect, x, y) {
            return PortalHit::Row(layout.row_start + idx);
        }
    }
    PortalHit::Outside
}

pub(crate) fn render_portal_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    portal: &PortalPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
    let layout = portal_modal_layout(
        screen_size,
        portal.rows.len(),
        portal.selected,
        portal.cursor_mode_editable,
    );
    let mx = layout.modal.loc.x;
    let my = layout.modal.loc.y;
    let title = if portal.is_screencast {
        "Share Screen or Window"
    } else {
        "Screenshot Request"
    };
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        title,
        crate::text::FontWeight::Bold,
        14.5,
        pal.popover_text_primary,
        Point::from((mx + 70, my + 20)),
    ) {
        elements.push(el);
    }
    let subtitle = truncate_ascii(
        &format!(
            "Requested by {}",
            if portal.app_id.is_empty() {
                "Application"
            } else {
                portal.app_id
            }
        ),
        40,
    );
    if let Some(el) = text_cache.render_label(
        renderer,
        &subtitle,
        11.5,
        pal.popover_text_muted,
        Point::from((mx + 70, my + 40)),
    ) {
        elements.push(el);
    }

    for (vis_idx, row_rect) in layout
        .row_rects
        .iter()
        .take(layout.visible_count)
        .enumerate()
    {
        let row_idx = layout.row_start + vis_idx;
        let Some(row) = portal.rows.get(row_idx) else {
            continue;
        };
        let is_selected = row_idx == portal.selected;
        let label = truncate_ascii(row.label, 38);
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            &label,
            crate::text::FontWeight::Bold,
            12.0,
            if is_selected {
                [255, 255, 255, 255]
            } else {
                pal.popover_text_primary
            },
            Point::from((row_rect.loc.x + 36, row_rect.loc.y + 5)),
        ) {
            elements.push(el);
        }
        let detail = truncate_ascii(row.detail, 44);
        if let Some(el) = text_cache.render_label(
            renderer,
            &detail,
            10.0,
            if is_selected {
                [236, 242, 255, 245]
            } else {
                pal.popover_text_muted
            },
            Point::from((row_rect.loc.x + 36, row_rect.loc.y + 22)),
        ) {
            elements.push(el);
        }
    }

    render_portal_footer_text(text_cache, renderer, screen_size, portal, elements);
}

pub(crate) fn render_portal_footer_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    portal: &PortalPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
    let layout = portal_modal_layout(
        screen_size,
        portal.rows.len(),
        portal.selected,
        portal.cursor_mode_editable,
    );

    if let Some(toggle_rect) = layout.cursor_toggle {
        let mark = if portal.include_cursor {
            "[x] Include cursor in stream"
        } else {
            "[ ] Include cursor in stream"
        };
        if let Some(el) = text_cache.render_label(
            renderer,
            mark,
            11.5,
            pal.popover_text_secondary,
            Point::from((toggle_rect.loc.x + 6, toggle_rect.loc.y + 5)),
        ) {
            elements.push(el);
        }
    }

    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        "Cancel",
        crate::text::FontWeight::Regular,
        12.5,
        pal.popover_text_primary,
        phys_to_logical_rect(layout.cancel),
    ) {
        elements.push(el);
    }
    let accept_label = if portal.is_screencast {
        "Share"
    } else {
        "Capture"
    };
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        accept_label,
        crate::text::FontWeight::Bold,
        12.5,
        [255, 255, 255, 255],
        phys_to_logical_rect(layout.accept),
    ) {
        elements.push(el);
    }
}

pub(crate) fn render_portal_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    portal: &PortalPromptRenderState<'_>,
) {
    let pal = ThemePalette::active();
    let layout = portal_modal_layout(
        screen_size,
        portal.rows.len(),
        portal.selected,
        portal.cursor_mode_editable,
    );
    let badge = layout.badge;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Grid,
        18,
        [255, 255, 255, 255],
        Point::from((badge.loc.x + 10, badge.loc.y + 10)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        badge,
        11,
        pal.accent_hover_fill,
        Some(pal.accent_hover_border),
    );

    for (vis_idx, row_rect) in layout
        .row_rects
        .iter()
        .take(layout.visible_count)
        .enumerate()
    {
        let row_idx = layout.row_start + vis_idx;
        let is_selected = row_idx == portal.selected;
        let glyph = if portal.rows.get(row_idx).is_some_and(|r| r.is_window) {
            crate::surfaces::UiGlyphKind::WindowMax
        } else {
            crate::surfaces::UiGlyphKind::Grid
        };
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            glyph,
            14,
            if is_selected {
                [255, 255, 255, 255]
            } else {
                pal.popover_icon
            },
            Point::from((row_rect.loc.x + 12, row_rect.loc.y + 13)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let (fill, border) = if is_selected {
            (pal.accent_hover_fill, pal.accent_hover_border)
        } else {
            (pal.control_pill_off_rgba, pal.border_rgba)
        };
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            *row_rect,
            10,
            fill,
            Some(border),
        );
    }

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.cancel,
        10,
        pal.control_pill_off_rgba,
        Some(pal.border_rgba),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.accept,
        10,
        pal.accent_hover_fill,
        Some(pal.accent_hover_border),
    );

    push_liquid_glass_panel_elevation(
        out,
        shape_cache,
        renderer,
        layout.modal,
        WINDOW_RADIUS,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        rusty_ui::shadow::ShadowElevation::ActiveWindow,
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((0, 0)), screen_size),
        premul(0.05, 0.06, 0.08, 0.44),
    )));
}
