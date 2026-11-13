//! `PolicyKit` authentication modal, XDG Portal consent dialog, and `PickColor` banner.

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::CompositorRenderElement;
use super::tokens::{
    COLOR_ACCENT, RGBA_BORDER, RGBA_SURFACE, TOP_BAR_HEIGHT, WINDOW_RADIUS, make_solid_element,
    premul, push_liquid_glass_panel, push_rounded_glass_box, rect_contains, scale_glass_rgba,
    truncate_ascii,
};
use super::top_bar::TopBarTelemetry;

/// Active native `PolicyKit` authentication modal render state (`RUSTYOS-056`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolkitPromptRenderState<'a> {
    /// Human-readable action prompt description.
    pub message: &'a str,
    /// Privilege action identifier (`org.freedesktop.policykit.exec`, etc.).
    pub action_id: &'a str,
    /// Target UNIX user display name (`"root"`, etc.).
    pub identity_user: &'a str,
    /// Active prompt label (`"Password"` / helper prompt).
    pub prompt_label: &'a str,
    /// Masked dots (`••••`) or visible echo-on text; empty if nothing typed.
    pub input_display: &'a str,
    /// True while submitted credentials are being checked.
    pub checking: bool,
    /// Optional authentication error banner shown above action buttons.
    pub error_message: Option<&'a str>,
    /// Optional PAM informational message.
    pub info_message: Option<&'a str>,
}

/// Interactive click targets inside the centered `PolicyKit` Liquid Glass modal (`RUSTYOS-056`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolkitHit {
    /// Clicked the password / verification field.
    PasswordField,
    /// Clicked `Authenticate` submit button.
    Submit,
    /// Clicked `Cancel` button.
    Cancel,
    /// Clicked elsewhere inside or outside the modal without cancelling.
    Outside,
}

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

#[derive(Debug, Clone, Copy)]
pub(super) struct PolkitModalLayout {
    pub(super) modal: Rectangle<i32, smithay::utils::Physical>,
    pub(super) badge: Rectangle<i32, smithay::utils::Physical>,
    pub(super) input: Rectangle<i32, smithay::utils::Physical>,
    pub(super) status: Rectangle<i32, smithay::utils::Physical>,
    pub(super) cancel: Rectangle<i32, smithay::utils::Physical>,
    pub(super) submit: Rectangle<i32, smithay::utils::Physical>,
}

pub(super) fn polkit_modal_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
) -> PolkitModalLayout {
    let w = screen_size.w.max(280);
    let h = screen_size.h.max(320);
    let mw = 420.min(w - 32).max(248);
    let mh = 278;
    let mx = (w - mw) / 2;
    let my = ((h - mh) / 2 - 12).max(TOP_BAR_HEIGHT + 16);
    let btn_w = ((mw - 60) / 2).clamp(88, 136);
    let btn_y = my + mh - 50;

    PolkitModalLayout {
        modal: Rectangle::new(Point::from((mx, my)), Size::from((mw, mh))),
        badge: Rectangle::new(Point::from((mx + 22, my + 20)), Size::from((38, 38))),
        input: Rectangle::new(Point::from((mx + 22, my + 126)), Size::from((mw - 44, 38))),
        status: Rectangle::new(Point::from((mx + 22, my + 174)), Size::from((mw - 44, 36))),
        cancel: Rectangle::new(Point::from((mx + 22, btn_y)), Size::from((btn_w, 32))),
        submit: Rectangle::new(
            Point::from((mx + mw - 22 - btn_w, btn_y)),
            Size::from((btn_w, 32)),
        ),
    }
}

/// Hit-tests `(x, y)` inside the centered Liquid Glass `PolicyKit` dialog (`RUSTYOS-056`).
#[must_use]
pub fn hit_test_polkit_dialog(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> PolkitHit {
    let layout = polkit_modal_layout(screen_size);
    if rect_contains(layout.submit, x, y) {
        return PolkitHit::Submit;
    }
    if rect_contains(layout.cancel, x, y) {
        return PolkitHit::Cancel;
    }
    if rect_contains(layout.input, x, y) {
        return PolkitHit::PasswordField;
    }
    PolkitHit::Outside
}

pub(super) const PORTAL_MAX_VISIBLE_ROWS: usize = 5;

#[derive(Debug, Clone, Copy)]
pub(super) struct PortalModalLayout {
    pub(super) modal: Rectangle<i32, smithay::utils::Physical>,
    pub(super) badge: Rectangle<i32, smithay::utils::Physical>,
    pub(super) row_start: usize,
    pub(super) row_rects: [Rectangle<i32, smithay::utils::Physical>; PORTAL_MAX_VISIBLE_ROWS],
    pub(super) visible_count: usize,
    pub(super) cursor_toggle: Option<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) cancel: Rectangle<i32, smithay::utils::Physical>,
    pub(super) accept: Rectangle<i32, smithay::utils::Physical>,
}

pub(super) fn portal_visible_range(total: usize, selected: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let count = total.min(PORTAL_MAX_VISIBLE_ROWS);
    let sel = selected.min(total - 1);
    let max_start = total.saturating_sub(count);
    let start = sel.saturating_sub(count / 2).min(max_start);
    (start, start + count)
}

pub(super) fn portal_modal_layout(
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

pub(super) fn pick_color_pill_rect(
    screen_size: Size<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    let w = screen_size.w.max(320);
    let pw = 390.min(w - 32).max(240);
    let ph = 36;
    let px = (w - pw) / 2;
    let py = TOP_BAR_HEIGHT + 12;
    Rectangle::new(Point::from((px, py)), Size::from((pw, ph)))
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

pub(super) fn wrap_polkit_two_lines(message: &str, max_chars: usize) -> (String, Option<String>) {
    let mut line1 = String::new();
    let mut line2 = String::new();
    let mut on_second = false;
    for word in message.split_whitespace() {
        if !on_second {
            let needed = if line1.is_empty() {
                word.chars().count()
            } else {
                line1.chars().count() + 1 + word.chars().count()
            };
            if needed <= max_chars || line1.is_empty() {
                if !line1.is_empty() {
                    line1.push(' ');
                }
                line1.push_str(word);
                continue;
            }
            on_second = true;
        }
        let needed = if line2.is_empty() {
            word.chars().count()
        } else {
            line2.chars().count() + 1 + word.chars().count()
        };
        if needed <= max_chars {
            if !line2.is_empty() {
                line2.push(' ');
            }
            line2.push_str(word);
        } else {
            if !line2.ends_with("...") && line2.len() + 3 <= max_chars + 3 {
                line2.push_str("...");
            }
            break;
        }
    }
    (
        truncate_ascii(&line1, max_chars),
        (!line2.is_empty()).then(|| truncate_ascii(&line2, max_chars)),
    )
}

pub(super) fn render_polkit_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    pk: &PolkitPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = polkit_modal_layout(screen_size);
    render_polkit_header_text(text_cache, renderer, layout.modal, pk, elements);
    render_polkit_controls_text(text_cache, renderer, layout, pk, elements);
}

pub(super) fn render_polkit_header_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    modal: Rectangle<i32, smithay::utils::Physical>,
    pk: &PolkitPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let mx = modal.loc.x;
    let my = modal.loc.y;
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "Authentication Required",
        crate::text::FontWeight::Bold,
        14.5,
        [248, 245, 255, 255],
        Point::from((mx + 70, my + 20)),
    ) {
        elements.push(el);
    }
    let user_line = truncate_ascii(&format!("Authenticating as {}", pk.identity_user), 38);
    if let Some(el) = text_cache.render_label(
        renderer,
        &user_line,
        11.5,
        [188, 178, 212, 255],
        Point::from((mx + 70, my + 40)),
    ) {
        elements.push(el);
    }

    let (line1, line2) = wrap_polkit_two_lines(pk.message, 46);
    if !line1.is_empty()
        && let Some(el) = text_cache.render_label(
            renderer,
            &line1,
            12.0,
            [236, 230, 248, 255],
            Point::from((mx + 22, my + 72)),
        )
    {
        elements.push(el);
    }
    if let Some(ref l2) = line2
        && let Some(el) = text_cache.render_label(
            renderer,
            l2,
            12.0,
            [236, 230, 248, 255],
            Point::from((mx + 22, my + 92)),
        )
    {
        elements.push(el);
    }
}

pub(super) fn render_polkit_controls_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: PolkitModalLayout,
    pk: &PolkitPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let input = layout.input;
    if pk.input_display.is_empty() {
        let placeholder = truncate_ascii(pk.prompt_label.trim_end_matches(':').trim(), 36);
        let label = if placeholder.is_empty() {
            "Password"
        } else {
            placeholder.as_str()
        };
        if let Some(el) = text_cache.render_label(
            renderer,
            label,
            13.0,
            [172, 162, 194, 230],
            Point::from((input.loc.x + 12, input.loc.y + 10)),
        ) {
            elements.push(el);
        }
    } else if let Some(el) = text_cache.render_label(
        renderer,
        pk.input_display,
        13.5,
        [250, 248, 255, 255],
        Point::from((input.loc.x + 12, input.loc.y + 10)),
    ) {
        elements.push(el);
    }

    let (status_text, status_color) = if pk.checking {
        ("Verifying credentials...".to_owned(), [210, 185, 255, 255])
    } else if let Some(err) = pk.error_message {
        (truncate_ascii(err, 48), [255, 138, 142, 255])
    } else if let Some(info) = pk.info_message {
        (truncate_ascii(info, 48), [212, 206, 232, 240])
    } else {
        (truncate_ascii(pk.action_id, 48), [166, 155, 188, 210])
    };
    if !status_text.is_empty()
        && let Some(el) = text_cache.render_label(
            renderer,
            &status_text,
            11.0,
            status_color,
            Point::from((layout.status.loc.x, layout.status.loc.y + 4)),
        )
    {
        elements.push(el);
    }

    let cancel_bounds = Rectangle::new(
        Point::from((layout.cancel.loc.x, layout.cancel.loc.y)),
        Size::from((layout.cancel.size.w, layout.cancel.size.h)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        "Cancel",
        crate::text::FontWeight::Bold,
        12.0,
        [240, 236, 248, 255],
        cancel_bounds,
    ) {
        elements.push(el);
    }
    let submit_label = if pk.checking {
        "Verifying..."
    } else {
        "Authenticate"
    };
    let submit_bounds = Rectangle::new(
        Point::from((layout.submit.loc.x, layout.submit.loc.y)),
        Size::from((layout.submit.size.w, layout.submit.size.h)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        submit_label,
        crate::text::FontWeight::Bold,
        12.0,
        [252, 250, 255, 255],
        submit_bounds,
    ) {
        elements.push(el);
    }
}

pub(super) fn render_polkit_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    pk: &PolkitPromptRenderState<'_>,
) {
    let layout = polkit_modal_layout(screen_size);
    let badge = layout.badge;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Lock,
        18,
        [248, 244, 255, 255],
        Point::from((badge.loc.x + 10, badge.loc.y + 10)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    let input = layout.input;
    let typed_w = crate::text::measure_ui_text_width(
        pk.input_display,
        13.5,
        crate::text::FontWeight::Regular,
    );
    let caret_x = (input.loc.x + 13 + typed_w).min(input.loc.x + input.size.w - 10);
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((caret_x, input.loc.y + 8)), Size::from((2, 22))),
        COLOR_ACCENT,
    )));

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        badge,
        11,
        [165, 110, 255, 205],
        Some([215, 185, 255, 155]),
    );

    let input_border = if pk.error_message.is_some() {
        [255, 130, 136, 220]
    } else {
        [192, 148, 255, 225]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        input,
        11,
        scale_glass_rgba(
            [24, 18, 38, 198],
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
        ),
        Some(input_border),
    );

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.cancel,
        10,
        RGBA_SURFACE,
        Some(RGBA_BORDER),
    );
    let submit_fill = if pk.checking {
        [124, 92, 184, 170]
    } else {
        [165, 110, 255, 224]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.submit,
        10,
        submit_fill,
        Some([215, 185, 255, 185]),
    );

    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        layout.modal,
        WINDOW_RADIUS,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Some((22, 62)),
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((0, 0)), screen_size),
        premul(0.059, 0.039, 0.098, 0.44),
    )));
}

pub(super) fn phys_to_logical_rect(
    rect: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Logical> {
    Rectangle::new(
        Point::from((rect.loc.x, rect.loc.y)),
        Size::from((rect.size.w, rect.size.h)),
    )
}

pub(super) fn render_portal_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    portal: &PortalPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
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
        [248, 245, 255, 255],
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
        [188, 178, 212, 255],
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
        let label = truncate_ascii(row.label, 38);
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            &label,
            crate::text::FontWeight::Bold,
            12.0,
            [248, 244, 255, 255],
            Point::from((row_rect.loc.x + 36, row_rect.loc.y + 5)),
        ) {
            elements.push(el);
        }
        let detail = truncate_ascii(row.detail, 44);
        if let Some(el) = text_cache.render_label(
            renderer,
            &detail,
            10.0,
            [176, 166, 200, 255],
            Point::from((row_rect.loc.x + 36, row_rect.loc.y + 22)),
        ) {
            elements.push(el);
        }
    }

    render_portal_footer_text(text_cache, renderer, screen_size, portal, elements);
}

pub(super) fn render_portal_footer_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    portal: &PortalPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
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
            [232, 224, 248, 255],
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
        [240, 236, 248, 255],
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
        [252, 249, 255, 255],
        phys_to_logical_rect(layout.accept),
    ) {
        elements.push(el);
    }
}

pub(super) fn render_portal_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    portal: &PortalPromptRenderState<'_>,
) {
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
        [248, 244, 255, 255],
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
        [165, 110, 255, 205],
        Some([215, 185, 255, 155]),
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
            [244, 238, 255, 255],
            Point::from((row_rect.loc.x + 12, row_rect.loc.y + 13)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let (fill, border) = if is_selected {
            ([165, 110, 255, 138], [215, 185, 255, 225])
        } else {
            (RGBA_SURFACE, RGBA_BORDER)
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
        RGBA_SURFACE,
        Some(RGBA_BORDER),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.accept,
        10,
        [165, 110, 255, 224],
        Some([215, 185, 255, 185]),
    );

    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        layout.modal,
        WINDOW_RADIUS,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Some((22, 62)),
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((0, 0)), screen_size),
        premul(0.059, 0.039, 0.098, 0.44),
    )));
}

pub(super) fn render_pick_color_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    app_id: &str,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pill = pick_color_pill_rect(screen_size);
    let msg = truncate_ascii(
        &format!(
            "Pick Color ({}) · Click pixel · Esc cancel",
            if app_id.is_empty() { "App" } else { app_id }
        ),
        48,
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        &msg,
        crate::text::FontWeight::Bold,
        12.0,
        [248, 244, 255, 255],
        phys_to_logical_rect(pill),
    ) {
        elements.push(el);
    }
}

pub(super) fn render_pick_color_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
) {
    let pill = pick_color_pill_rect(screen_size);
    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        pill,
        12,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Some((12, 42)),
    );
}
