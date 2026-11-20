//! `PolicyKit` authentication modal prompt layout, hit-testing, and Liquid Glass rendering (`RUSTYOS-091`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{
    COLOR_ACCENT, TOP_BAR_HEIGHT, ThemePalette, WINDOW_RADIUS, make_solid_element, premul,
    push_liquid_glass_panel_elevation, push_rounded_glass_box, rect_contains, scale_glass_rgba,
    truncate_ascii,
};
use crate::shell_render::top_bar::TopBarTelemetry;

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

#[derive(Debug, Clone, Copy)]
pub(crate) struct PolkitModalLayout {
    pub(crate) modal: Rectangle<i32, smithay::utils::Physical>,
    pub(crate) badge: Rectangle<i32, smithay::utils::Physical>,
    pub(crate) input: Rectangle<i32, smithay::utils::Physical>,
    pub(crate) status: Rectangle<i32, smithay::utils::Physical>,
    pub(crate) cancel: Rectangle<i32, smithay::utils::Physical>,
    pub(crate) submit: Rectangle<i32, smithay::utils::Physical>,
}

pub(crate) fn polkit_modal_layout(
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

pub(crate) fn wrap_polkit_two_lines(message: &str, max_chars: usize) -> (String, Option<String>) {
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

pub(crate) fn render_polkit_text(
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

pub(crate) fn render_polkit_header_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    modal: Rectangle<i32, smithay::utils::Physical>,
    pk: &PolkitPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
    let mx = modal.loc.x;
    let my = modal.loc.y;
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "Authentication Required",
        crate::text::FontWeight::Bold,
        14.5,
        pal.popover_text_primary,
        Point::from((mx + 70, my + 20)),
    ) {
        elements.push(el);
    }
    let user_line = truncate_ascii(&format!("Authenticating as {}", pk.identity_user), 38);
    if let Some(el) = text_cache.render_label(
        renderer,
        &user_line,
        11.5,
        pal.popover_text_muted,
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
            pal.popover_text_secondary,
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
            pal.popover_text_secondary,
            Point::from((mx + 22, my + 92)),
        )
    {
        elements.push(el);
    }
}

pub(crate) fn render_polkit_controls_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: PolkitModalLayout,
    pk: &PolkitPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
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
            pal.popover_text_muted,
            Point::from((input.loc.x + 12, input.loc.y + 10)),
        ) {
            elements.push(el);
        }
    } else if let Some(el) = text_cache.render_label(
        renderer,
        pk.input_display,
        13.5,
        pal.popover_text_primary,
        Point::from((input.loc.x + 12, input.loc.y + 10)),
    ) {
        elements.push(el);
    }

    let (status_text, status_color) = if pk.checking {
        ("Verifying credentials...".to_owned(), pal.accent_rgba)
    } else if let Some(err) = pk.error_message {
        (truncate_ascii(err, 48), pal.popover_danger)
    } else if let Some(info) = pk.info_message {
        (truncate_ascii(info, 48), pal.popover_text_secondary)
    } else {
        (truncate_ascii(pk.action_id, 48), pal.popover_text_muted)
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
        pal.popover_text_primary,
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
        [255, 255, 255, 255],
        submit_bounds,
    ) {
        elements.push(el);
    }
}

pub(crate) fn render_polkit_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    pk: &PolkitPromptRenderState<'_>,
) {
    let pal = ThemePalette::active();
    let layout = polkit_modal_layout(screen_size);
    let badge = layout.badge;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Lock,
        18,
        [255, 255, 255, 255],
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
        pal.accent_hover_fill,
        Some(pal.accent_hover_border),
    );

    let input_border = if pk.error_message.is_some() {
        pal.popover_danger
    } else {
        pal.accent_hover_border
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        input,
        11,
        scale_glass_rgba(
            pal.control_pill_off_rgba,
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
        pal.control_pill_off_rgba,
        Some(pal.border_rgba),
    );
    let submit_fill = if pk.checking {
        [68, 128, 210, 180]
    } else {
        pal.accent_hover_fill
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.submit,
        10,
        submit_fill,
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
