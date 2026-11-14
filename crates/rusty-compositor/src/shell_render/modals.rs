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

/// Active sidebar category inside the built-in System Settings modal (`RUSTYOS-061`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    /// Frosted glass opacity, reduce transparency, and reduced motion.
    #[default]
    Appearance,
    /// Menu Bar indicators and clock format.
    MenuBar,
    /// `HiDPI` scale presets, output rotation, VRR policy, and max refresh rate.
    Displays,
    /// Lock screen authentication, lock-on-sleep, and `PolicyKit` agent.
    Security,
}

impl SettingsTab {
    /// Ordered sidebar tabs displayed in System Settings.
    pub const ALL: [Self; 4] = [
        Self::Appearance,
        Self::MenuBar,
        Self::Displays,
        Self::Security,
    ];

    /// Human-readable sidebar & header title.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::MenuBar => "Menu Bar",
            Self::Displays => "Displays",
            Self::Security => "Security",
        }
    }

    const fn icon(self) -> crate::surfaces::UiGlyphKind {
        match self {
            Self::Appearance => crate::surfaces::UiGlyphKind::Transparency,
            Self::MenuBar => crate::surfaces::UiGlyphKind::ControlCenter,
            Self::Displays => crate::surfaces::UiGlyphKind::WindowMax,
            Self::Security => crate::surfaces::UiGlyphKind::Lock,
        }
    }

    /// Next category in wrap-around order.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Appearance => Self::MenuBar,
            Self::MenuBar => Self::Displays,
            Self::Displays => Self::Security,
            Self::Security => Self::Appearance,
        }
    }

    /// Previous category in wrap-around order.
    #[must_use]
    pub const fn prev(self) -> Self {
        match self {
            Self::Appearance => Self::Security,
            Self::MenuBar => Self::Appearance,
            Self::Displays => Self::MenuBar,
            Self::Security => Self::Displays,
        }
    }
}

/// Snapshot passed to the System Settings renderer (`RUSTYOS-061`).
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct SettingsRenderState<'a> {
    /// Active left-sidebar category tab.
    pub active_tab: SettingsTab,
    /// Shell glass opacity factor (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels toggle.
    pub reduce_transparency: bool,
    /// Reduced motion / instant animations toggle.
    pub reduced_motion: bool,
    /// Show Menu Bar workspace strip.
    pub menubar_show_workspaces: bool,
    /// Show Menu Bar `StatusNotifier` tray icons.
    pub menubar_show_tray: bool,
    /// Show Menu Bar input source indicator (`EN`/`RU`).
    pub menubar_show_input_source: bool,
    /// Show Menu Bar standalone volume glyph.
    pub menubar_show_volume: bool,
    /// Use 24-hour Menu Bar clock format.
    pub clock_24h: bool,
    /// Primary connected display name (e.g. `"Winit-1"` or `"eDP-1"`).
    pub primary_connector: &'a str,
    /// Active `HiDPI` scale for the primary output.
    pub display_scale: f64,
    /// Active orientation for the primary output.
    pub display_rotation: crate::config::DisplayRotation,
    /// Global Adaptive Sync / VRR policy.
    pub vrr_mode: crate::config::VrrMode,
    /// Prefer max supported mode refresh rate on DRM/KMS.
    pub prefer_max_refresh: bool,
    /// Require PAM password when unlocking the Lock Screen.
    pub require_password_on_lock: bool,
    /// Automatically lock session on logind sleep.
    pub lock_on_sleep: bool,
    /// Enable native `PolicyKit` authentication agent.
    pub enable_polkit_agent: bool,
    /// Footer persistence badge (e.g. `"Saved"`, `"Reloaded"`, or `""`).
    pub config_status_label: &'a str,
}

/// Interactive click targets inside the System Settings modal (`RUSTYOS-061`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsHit {
    /// Switch active sidebar category.
    SelectTab(SettingsTab),
    /// Decrease frosted glass opacity.
    GlassOpacityDown,
    /// Increase frosted glass opacity.
    GlassOpacityUp,
    /// Toggle `reduce_transparency`.
    ToggleReduceTransparency,
    /// Toggle `reduced_motion`.
    ToggleReducedMotion,
    /// Toggle `menubar_show_workspaces`.
    ToggleMenuWorkspaces,
    /// Toggle `menubar_show_tray`.
    ToggleMenuTray,
    /// Toggle `menubar_show_input_source`.
    ToggleMenuInputSource,
    /// Toggle `menubar_show_volume`.
    ToggleMenuVolume,
    /// Toggle `clock_24h`.
    ToggleClock24h,
    /// Step display `HiDPI` scale down.
    ScaleDown,
    /// Step display `HiDPI` scale up.
    ScaleUp,
    /// Cycle display rotation (`0° -> 90° -> 180° -> 270°`).
    CycleRotation,
    /// Cycle Adaptive Sync / VRR mode (`Off -> Fullscreen -> Always`).
    CycleVrrMode,
    /// Toggle `prefer_max_refresh`.
    TogglePreferMaxRefresh,
    /// Toggle `require_password_on_lock`.
    ToggleRequirePassword,
    /// Toggle `lock_on_sleep`.
    ToggleLockOnSleep,
    /// Toggle `enable_polkit_agent`.
    TogglePolkitAgent,
    /// Reload `~/.config/rustyos/config.toml` from disk.
    ReloadConfig,
    /// Close System Settings overlay (`×` or `Done`).
    Close,
    /// Empty space inside the settings window (does not dismiss).
    ModalBody,
    /// Click outside the modal bounds (dismisses overlay).
    Outside,
}

#[derive(Debug, Clone, Copy)]
struct SettingsModalLayout {
    modal: Rectangle<i32, smithay::utils::Physical>,
    badge: Rectangle<i32, smithay::utils::Physical>,
    close_btn: Rectangle<i32, smithay::utils::Physical>,
    tabs: [Rectangle<i32, smithay::utils::Physical>; 4],
    divider_x: i32,
    content_x: i32,
    rows: [Rectangle<i32, smithay::utils::Physical>; 5],
    reload_btn: Rectangle<i32, smithay::utils::Physical>,
    done_btn: Rectangle<i32, smithay::utils::Physical>,
}

fn settings_modal_layout(screen_size: Size<i32, smithay::utils::Physical>) -> SettingsModalLayout {
    let w = 580.min((screen_size.w - 32).max(380));
    let h = 432.min((screen_size.h - TOP_BAR_HEIGHT - 24).max(320));
    let x = (screen_size.w - w) / 2;
    let y = ((screen_size.h - h) / 2).max(TOP_BAR_HEIGHT + 10);

    let modal = Rectangle::new(Point::from((x, y)), Size::from((w, h)));
    let badge = Rectangle::new(Point::from((x + 20, y + 16)), Size::from((34, 34)));
    let close_btn = Rectangle::new(Point::from((x + w - 44, y + 20)), Size::from((26, 26)));

    let tabs = std::array::from_fn(|idx| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let ry = y + 70 + (idx as i32) * 44;
        Rectangle::new(Point::from((x + 18, ry)), Size::from((152, 36)))
    });

    let divider_x = x + 184;
    let content_x = x + 198;
    let content_w = (w - 218).max(160);
    let rows = std::array::from_fn(|idx| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let ry = y + 98 + (idx as i32) * 52;
        Rectangle::new(Point::from((content_x, ry)), Size::from((content_w, 44)))
    });

    let done_btn = Rectangle::new(Point::from((x + w - 98, y + h - 50)), Size::from((78, 32)));
    let reload_btn = Rectangle::new(
        Point::from((done_btn.loc.x - 122, y + h - 50)),
        Size::from((112, 32)),
    );

    SettingsModalLayout {
        modal,
        badge,
        close_btn,
        tabs,
        divider_x,
        content_x,
        rows,
        reload_btn,
        done_btn,
    }
}

fn settings_row_pill_rect(
    row: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    Rectangle::new(
        Point::from((row.loc.x + row.size.w - 110, row.loc.y + 8)),
        Size::from((100, 28)),
    )
}

fn settings_row_stepper_rects(
    row: Rectangle<i32, smithay::utils::Physical>,
) -> (
    Rectangle<i32, smithay::utils::Physical>,
    Rectangle<i32, smithay::utils::Physical>,
    Rectangle<i32, smithay::utils::Physical>,
) {
    let plus = Rectangle::new(
        Point::from((row.loc.x + row.size.w - 38, row.loc.y + 8)),
        Size::from((28, 28)),
    );
    let value_box = Rectangle::new(
        Point::from((plus.loc.x - 62, row.loc.y + 8)),
        Size::from((56, 28)),
    );
    let minus = Rectangle::new(
        Point::from((value_box.loc.x - 34, row.loc.y + 8)),
        Size::from((28, 28)),
    );
    (minus, value_box, plus)
}

enum SettingsRowKind {
    Stepper {
        down: SettingsHit,
        up: SettingsHit,
        value: String,
    },
    Toggle {
        hit: SettingsHit,
        value: &'static str,
        active: bool,
    },
}

struct SettingsRowEntry {
    label: String,
    kind: SettingsRowKind,
}

fn toggle_row(label: impl Into<String>, hit: SettingsHit, active: bool) -> SettingsRowEntry {
    SettingsRowEntry {
        label: label.into(),
        kind: SettingsRowKind::Toggle {
            hit,
            value: if active { "On" } else { "Off" },
            active,
        },
    }
}

fn settings_tab_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    match st.active_tab {
        SettingsTab::Appearance => settings_appearance_rows(st),
        SettingsTab::MenuBar => vec![
            toggle_row(
                "Workspace Strip",
                SettingsHit::ToggleMenuWorkspaces,
                st.menubar_show_workspaces,
            ),
            toggle_row(
                "Tray Applets",
                SettingsHit::ToggleMenuTray,
                st.menubar_show_tray,
            ),
            toggle_row(
                "Input Source Badge",
                SettingsHit::ToggleMenuInputSource,
                st.menubar_show_input_source,
            ),
            toggle_row(
                "Volume Indicator",
                SettingsHit::ToggleMenuVolume,
                st.menubar_show_volume,
            ),
            toggle_row("24-Hour Clock", SettingsHit::ToggleClock24h, st.clock_24h),
        ],
        SettingsTab::Displays => settings_display_rows(st),
        SettingsTab::Security => vec![
            toggle_row(
                "Require Password on Lock",
                SettingsHit::ToggleRequirePassword,
                st.require_password_on_lock,
            ),
            toggle_row(
                "Lock Screen on Sleep",
                SettingsHit::ToggleLockOnSleep,
                st.lock_on_sleep,
            ),
            toggle_row(
                "Built-in Polkit Agent",
                SettingsHit::TogglePolkitAgent,
                st.enable_polkit_agent,
            ),
        ],
    }
}

fn settings_appearance_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pct = (crate::config::clamp_glass_opacity(st.glass_opacity) * 100.0).round() as u32;
    vec![
        SettingsRowEntry {
            label: "Glass Opacity".to_owned(),
            kind: SettingsRowKind::Stepper {
                down: SettingsHit::GlassOpacityDown,
                up: SettingsHit::GlassOpacityUp,
                value: format!("{pct}%"),
            },
        },
        toggle_row(
            "Reduce Transparency",
            SettingsHit::ToggleReduceTransparency,
            st.reduce_transparency,
        ),
        toggle_row(
            "Reduce Motion",
            SettingsHit::ToggleReducedMotion,
            st.reduced_motion,
        ),
    ]
}

fn settings_display_rows(st: &SettingsRenderState<'_>) -> Vec<SettingsRowEntry> {
    let conn = truncate_ascii(st.primary_connector, 12);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scale_pct = (crate::config::clamp_display_scale(st.display_scale) * 100.0).round() as u32;
    let vrr_label = match st.vrr_mode {
        crate::config::VrrMode::Off => "Off",
        crate::config::VrrMode::Fullscreen => "Fullscreen",
        crate::config::VrrMode::Always => "Always",
    };
    let rot_label = match st.display_rotation {
        crate::config::DisplayRotation::Rotate0 => "0°",
        crate::config::DisplayRotation::Rotate90 => "90°",
        crate::config::DisplayRotation::Rotate180 => "180°",
        crate::config::DisplayRotation::Rotate270 => "270°",
    };
    vec![
        SettingsRowEntry {
            label: format!("HiDPI Scale ({conn})"),
            kind: SettingsRowKind::Stepper {
                down: SettingsHit::ScaleDown,
                up: SettingsHit::ScaleUp,
                value: format!("{scale_pct}%"),
            },
        },
        SettingsRowEntry {
            label: "Rotation".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::CycleRotation,
                value: rot_label,
                active: st.display_rotation != crate::config::DisplayRotation::Rotate0,
            },
        },
        SettingsRowEntry {
            label: "Adaptive Sync (VRR)".to_owned(),
            kind: SettingsRowKind::Toggle {
                hit: SettingsHit::CycleVrrMode,
                value: vrr_label,
                active: st.vrr_mode != crate::config::VrrMode::Off,
            },
        },
        toggle_row(
            "Prefer Max Refresh",
            SettingsHit::TogglePreferMaxRefresh,
            st.prefer_max_refresh,
        ),
    ]
}

/// Hit-tests pointer coordinates against the System Settings modal (`RUSTYOS-061`).
#[must_use]
pub fn hit_test_settings_modal(
    screen_size: Size<i32, smithay::utils::Physical>,
    state: &SettingsRenderState<'_>,
    x: f64,
    y: f64,
) -> SettingsHit {
    let layout = settings_modal_layout(screen_size);
    if !rect_contains(layout.modal, x, y) {
        return SettingsHit::Outside;
    }
    if rect_contains(layout.close_btn, x, y) || rect_contains(layout.done_btn, x, y) {
        return SettingsHit::Close;
    }
    if rect_contains(layout.reload_btn, x, y) {
        return SettingsHit::ReloadConfig;
    }
    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        if rect_contains(rect, x, y) {
            return SettingsHit::SelectTab(*tab);
        }
    }
    for (entry, row_rect) in settings_tab_rows(state).iter().zip(layout.rows) {
        match &entry.kind {
            SettingsRowKind::Stepper { down, up, .. } => {
                let (minus, _, plus) = settings_row_stepper_rects(row_rect);
                if rect_contains(minus, x, y) {
                    return *down;
                }
                if rect_contains(plus, x, y) {
                    return *up;
                }
            }
            SettingsRowKind::Toggle { hit, .. } => {
                if rect_contains(settings_row_pill_rect(row_rect), x, y) {
                    return *hit;
                }
            }
        }
    }
    SettingsHit::ModalBody
}

pub(super) fn render_settings_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    st: &SettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = settings_modal_layout(screen_size);
    let modal = layout.modal;

    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "System Settings",
        crate::text::FontWeight::Bold,
        15.0,
        [248, 244, 255, 255],
        Point::from((layout.badge.loc.x + 44, layout.badge.loc.y + 8)),
    ) {
        elements.push(el);
    }

    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        let selected = *tab == st.active_tab;
        let col = if selected {
            [250, 248, 255, 255]
        } else {
            [202, 194, 224, 235]
        };
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            tab.label(),
            if selected {
                crate::text::FontWeight::Bold
            } else {
                crate::text::FontWeight::Regular
            },
            12.5,
            col,
            Point::from((rect.loc.x + 34, rect.loc.y + 10)),
        ) {
            elements.push(el);
        }
    }

    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        st.active_tab.label(),
        crate::text::FontWeight::Bold,
        14.0,
        [248, 244, 255, 255],
        Point::from((layout.content_x + 2, modal.loc.y + 68)),
    ) {
        elements.push(el);
    }

    render_settings_rows_text(text_cache, renderer, &layout, st, elements);
    render_settings_footer_text(text_cache, renderer, &layout, st, elements);
}

fn render_settings_rows_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    st: &SettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    for (entry, row) in settings_tab_rows(st).iter().zip(layout.rows) {
        if let Some(el) = text_cache.render_label(
            renderer,
            &entry.label,
            12.5,
            [236, 230, 248, 255],
            Point::from((row.loc.x + 14, row.loc.y + 14)),
        ) {
            elements.push(el);
        }
        match &entry.kind {
            SettingsRowKind::Stepper { value, .. } => {
                let (minus, value_box, plus) = settings_row_stepper_rects(row);
                for (rect, label) in [(minus, "-"), (value_box, value.as_str()), (plus, "+")] {
                    if let Some(el) = text_cache.render_label_centered_weighted(
                        renderer,
                        label,
                        crate::text::FontWeight::Bold,
                        12.0,
                        [248, 244, 255, 255],
                        phys_to_logical_rect(rect),
                    ) {
                        elements.push(el);
                    }
                }
            }
            SettingsRowKind::Toggle { value, active, .. } => {
                let pill = settings_row_pill_rect(row);
                let fg = if *active {
                    [252, 250, 255, 255]
                } else {
                    [214, 206, 232, 240]
                };
                if let Some(el) = text_cache.render_label_centered_weighted(
                    renderer,
                    value,
                    crate::text::FontWeight::Bold,
                    11.5,
                    fg,
                    phys_to_logical_rect(pill),
                ) {
                    elements.push(el);
                }
            }
        }
    }
}

fn render_settings_footer_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    st: &SettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let status = if st.config_status_label.is_empty() {
        "~/.config/rustyos/config.toml".to_owned()
    } else {
        truncate_ascii(&format!("config.toml · {}", st.config_status_label), 38)
    };
    if let Some(el) = text_cache.render_label(
        renderer,
        &status,
        11.0,
        [176, 166, 204, 225],
        Point::from((
            layout.modal.loc.x + 22,
            layout.modal.loc.y + layout.modal.size.h - 40,
        )),
    ) {
        elements.push(el);
    }

    for (rect, label, bold_fg) in [
        (layout.reload_btn, "Reload TOML", [238, 232, 248, 255]),
        (layout.done_btn, "Done", [252, 250, 255, 255]),
    ] {
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            label,
            crate::text::FontWeight::Bold,
            12.0,
            bold_fg,
            phys_to_logical_rect(rect),
        ) {
            elements.push(el);
        }
    }
}

pub(super) fn render_settings_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    st: &SettingsRenderState<'_>,
) {
    let layout = settings_modal_layout(screen_size);
    let modal = layout.modal;

    // Icons pushed before panel boxes so glyphs composite above backgrounds.
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::ControlCenter,
        18,
        [248, 244, 255, 255],
        Point::from((layout.badge.loc.x + 8, layout.badge.loc.y + 8)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Close,
        12,
        [240, 234, 248, 240],
        Point::from((layout.close_btn.loc.x + 7, layout.close_btn.loc.y + 7)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        let fg = if *tab == st.active_tab {
            [252, 250, 255, 255]
        } else {
            [196, 188, 220, 230]
        };
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            tab.icon(),
            16,
            fg,
            Point::from((rect.loc.x + 10, rect.loc.y + 10)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.badge,
        10,
        [165, 110, 255, 210],
        Some([215, 185, 255, 160]),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.close_btn,
        13,
        RGBA_SURFACE,
        Some(RGBA_BORDER),
    );

    render_settings_controls_composite(out, shape_cache, renderer, &layout, top_telemetry, st);

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.reload_btn,
        10,
        RGBA_SURFACE,
        Some(RGBA_BORDER),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.done_btn,
        10,
        [165, 110, 255, 224],
        Some([215, 185, 255, 185]),
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(
            Point::from((layout.divider_x, modal.loc.y + 66)),
            Size::from((1, (modal.size.h - 130).max(40))),
        ),
        premul(1.0, 1.0, 1.0, 0.12),
    )));

    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        modal,
        WINDOW_RADIUS,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Some((22, 62)),
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((0, 0)), screen_size),
        premul(0.059, 0.039, 0.098, 0.42),
    )));
}

fn render_settings_controls_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &SettingsModalLayout,
    top_telemetry: &TopBarTelemetry<'_>,
    st: &SettingsRenderState<'_>,
) {
    for (entry, row) in settings_tab_rows(st).iter().zip(layout.rows) {
        match &entry.kind {
            SettingsRowKind::Stepper { .. } => {
                let (minus, value_box, plus) = settings_row_stepper_rects(row);
                for (btn, fill) in [
                    (minus, [56, 46, 82, 220]),
                    (value_box, [34, 26, 52, 210]),
                    (plus, [56, 46, 82, 220]),
                ] {
                    push_rounded_glass_box(
                        out,
                        shape_cache,
                        renderer,
                        btn,
                        8,
                        fill,
                        Some(RGBA_BORDER),
                    );
                }
            }
            SettingsRowKind::Toggle { active, .. } => {
                let pill = settings_row_pill_rect(row);
                let (fill, border) = if *active {
                    ([165, 110, 255, 220], [215, 185, 255, 185])
                } else {
                    ([46, 38, 66, 205], RGBA_BORDER)
                };
                push_rounded_glass_box(out, shape_cache, renderer, pill, 10, fill, Some(border));
            }
        }
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            row,
            11,
            scale_glass_rgba(
                [30, 23, 46, 175],
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
            ),
            Some(RGBA_BORDER),
        );
    }

    for (tab, rect) in SettingsTab::ALL.iter().zip(layout.tabs) {
        let selected = *tab == st.active_tab;
        let fill = if selected {
            [165, 110, 255, 195]
        } else {
            scale_glass_rgba(
                [34, 26, 52, 120],
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
            )
        };
        let border = if selected {
            Some([215, 185, 255, 175])
        } else {
            None
        };
        push_rounded_glass_box(out, shape_cache, renderer, rect, 10, fill, border);
    }
}
