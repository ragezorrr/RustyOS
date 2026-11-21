//! `TopBar` notification HUD toasts, unread notification badges, and floating OSD pills (`RUSTYOS-090`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Physical, Point, Rectangle, Size};

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{
    DOCK_HEIGHT, DOCK_MARGIN_BOTTOM, TOP_BAR_HEIGHT, ThemePalette,
    push_liquid_glass_panel_elevation, push_rounded_glass_box,
};
#[cfg(test)]
use crate::shell_render::tokens::{TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y};

#[cfg(test)]
use super::panel::compute_top_bar_right_layout;
use super::panel::{TopBarLayoutOptions, compute_top_bar_right_layout_with_options};

/// Shared notification card geometry matching `Notification-Center` (`RUSTYOS-110`).
pub(crate) const NOTIFICATION_CARD_WIDTH: i32 = 344;
pub(crate) const NOTIFICATION_CARD_HEIGHT: i32 = 68;
pub(crate) const NOTIFICATION_CARD_RADIUS: i32 = 22;
pub(crate) const NOTIFICATION_CARD_RIGHT_MARGIN: i32 = 14;
pub(crate) const NOTIFICATION_CARD_TOP_MARGIN: i32 = 12;
const NOTIFICATION_ICON_SIZE: i32 = 38;
const NOTIFICATION_ICON_OFFSET_X: i32 = 16;
const NOTIFICATION_ICON_OFFSET_Y: i32 = 15;
const NOTIFICATION_TEXT_OFFSET_X: i32 = 66;
const NOTIFICATION_TEXT_RIGHT_PAD: i32 = 18;

/// Active notification toast view rendered in the top-right Notification HUD (`RUSTYOS-024`).
#[derive(Debug, Clone)]
pub struct NotificationToastView<'a> {
    /// Unique notification identifier.
    pub id: u64,
    /// Urgency level controlling accent color.
    pub urgency: rusty_ipc::Urgency,
    /// Short notification title.
    pub title: &'a str,
    /// Notification body text.
    pub body: &'a str,
    /// True when the pointer is hovering this notification card (`RUSTYOS-051`).
    pub hovered: bool,
}

/// Active transient On-Screen Display (OSD) indicator pill (`RUSTYOS-031`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsdRenderState<'a> {
    /// Primary label (e.g., `"Volume"`, `"Brightness"`, `"Play / Pause"`, `"Screenshot"`).
    pub label: &'a str,
    /// Optional numeric percentage (`0..=100`) rendered as a progress bar.
    pub value_percent: Option<u8>,
    /// True when audio is muted (grays out the bar fill).
    pub muted: bool,
}

/// Maximum number of floating notification toasts shown simultaneously (`RUSTYOS-024`).
pub const MAX_VISIBLE_NOTIFICATIONS: usize = 4;

/// Computes top-right stacked rectangles for up to [`MAX_VISIBLE_NOTIFICATIONS`] active toasts (`RUSTYOS-024`).
#[must_use]
pub fn notification_toast_rects(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    let visible = count.min(MAX_VISIBLE_NOTIFICATIONS);
    if visible == 0 {
        return Vec::new();
    }
    let width = screen_size.w.max(320);
    let toast_w = NOTIFICATION_CARD_WIDTH.min((width - 28).max(200));
    let toast_h = NOTIFICATION_CARD_HEIGHT;
    let gap = 10;
    let toast_x = (width - toast_w - NOTIFICATION_CARD_RIGHT_MARGIN).max(14);
    let start_y = TOP_BAR_HEIGHT + NOTIFICATION_CARD_TOP_MARGIN;

    let mut rects = Vec::with_capacity(visible);
    for idx in 0..visible {
        let idx_i32 = i32::try_from(idx).unwrap_or(0);
        let y = start_y + idx_i32 * (toast_h + gap);
        rects.push(Rectangle::new(
            Point::from((toast_x, y)),
            Size::from((toast_w, toast_h)),
        ));
    }
    rects
}

#[cfg(test)]
pub(crate) fn notification_badge_rect(screen_width: i32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout(screen_width, false, 1, "12:00");
    (
        layout.notif_x,
        TOP_BAR_ITEM_Y,
        layout.notif_w,
        TOP_BAR_ITEM_H,
    )
}

/// Hit-tests `(x, y)` against visible top-right Notification HUD toasts (`RUSTYOS-024`).
#[must_use]
pub fn check_notification_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    visible_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let rects = notification_toast_rects(screen_size, visible_count);
    for (idx, rect) in rects.into_iter().enumerate() {
        let rx = f64::from(rect.loc.x);
        let ry = f64::from(rect.loc.y);
        let rw = f64::from(rect.size.w);
        let rh = f64::from(rect.size.h);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return Some(idx);
        }
    }
    None
}

/// Checks if `(x, y)` clicked the `TopBar` unread notification badge pill (`RUSTYOS-024`).
#[must_use]
pub fn check_top_bar_notification_click(
    screen_width: i32,
    notification_count: u32,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_notification_click_for_telemetry(
        screen_width,
        false,
        notification_count,
        "12:00",
        x,
        y,
    )
}

/// Telemetry-aware hit-test for the `TopBar` unread notification badge (`RUSTYOS-048`/`049`).
#[must_use]
pub fn check_top_bar_notification_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if opts.notification_count == 0 || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let layout = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    layout.notif_w > 0
        && x >= f64::from(layout.notif_x)
        && x < f64::from(layout.notif_x + layout.notif_w)
}

/// Telemetry-aware hit-test for the `TopBar` unread notification badge (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_notification_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_notification_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

pub(crate) fn notification_app_icon(title: &str) -> crate::icons::BuiltinAppIcon {
    let lower = title.to_ascii_lowercase();
    if lower.contains("term")
        || lower.contains("shell")
        || lower.contains("bash")
        || lower.contains("konsole")
    {
        crate::icons::BuiltinAppIcon::Terminal
    } else if lower.contains("file") || lower.contains("download") || lower.contains("rusty-files")
    {
        crate::icons::BuiltinAppIcon::Files
    } else if lower.contains("browser")
        || lower.contains("web")
        || lower.contains("firefox")
        || lower.contains("chrom")
    {
        crate::icons::BuiltinAppIcon::Browser
    } else if lower.contains("setting") || lower.contains("config") || lower.contains("pref") {
        crate::icons::BuiltinAppIcon::Settings
    } else if lower.contains("update") || lower.contains("upgrade") {
        crate::icons::BuiltinAppIcon::Updater
    } else {
        crate::icons::BuiltinAppIcon::RustySystem
    }
}

pub(crate) fn osd_glyph_kind(
    label: &str,
    value_percent: Option<u8>,
    muted: bool,
) -> crate::surfaces::UiGlyphKind {
    let lower = label.to_ascii_lowercase();
    if lower.contains("bright") || lower.contains("glass") || lower.contains("display") {
        crate::surfaces::UiGlyphKind::Sun
    } else if lower.contains("vol")
        || lower.contains("audio")
        || lower.contains("sound")
        || lower.contains("mute")
        || value_percent.is_some()
    {
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: value_percent.unwrap_or(50),
            muted,
        }
    } else {
        crate::surfaces::UiGlyphKind::Info
    }
}

/// Computes the centered floating OSD pill bounds right above the bottom Dock (`RUSTYOS-031`).
#[must_use]
pub fn osd_pill_rect(
    screen_size: Size<i32, smithay::utils::Physical>,
    has_bar: bool,
) -> Rectangle<i32, smithay::utils::Physical> {
    let w = 256.min((screen_size.w - 40).max(180));
    let h = if has_bar { 56 } else { 40 };
    let x = (screen_size.w - w) / 2;
    let y = (screen_size.h - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - 22 - h).max(TOP_BAR_HEIGHT + 12);
    Rectangle::new(Point::from((x, y)), Size::from((w, h)))
}

/// Generates Liquid Glass OSD indicator elements (`Volume`, `Brightness`, `Media`, `Screenshot`) (`RUSTYOS-031`/`051`).
pub fn generate_osd_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    osd: &OsdRenderState<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let pill = osd_pill_rect(screen_size, osd.value_percent.is_some());
    let px = pill.loc.x;
    let py = pill.loc.y;
    let pw = pill.size.w;

    let pal = ThemePalette::active();
    let text_bright = pal.popover_text_primary;
    let text_muted = pal.popover_text_muted;

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        osd_glyph_kind(osd.label, osd.value_percent, osd.muted),
        22,
        pal.popover_icon,
        Point::from((px + 14, py + (pill.size.h - 22) / 2)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(pct) = osd.value_percent {
        if let Some(el) = text_cache.render_label(
            renderer,
            osd.label,
            12.0,
            text_bright,
            Point::from((px + 44, py + 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let val_text = if osd.muted {
            "Muted".to_owned()
        } else {
            format!("{pct}%")
        };
        let (vw, _) = text_cache.measure_text(&val_text, 12.0);
        if let Some(el) = text_cache.render_label(
            renderer,
            &val_text,
            12.0,
            if osd.muted { text_muted } else { text_bright },
            Point::from((px + pw - 14 - vw, py + 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        push_osd_level_bar(&mut out, shape_cache, renderer, pill, pct, osd.muted, &pal);
    } else if let Some(el) = text_cache.render_label_centered(
        renderer,
        osd.label,
        12.5,
        text_bright,
        Rectangle::new(
            Point::from((px + 38, py)),
            Size::from((pw - 48, pill.size.h)),
        ),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    push_liquid_glass_panel_elevation(
        &mut out,
        shape_cache,
        renderer,
        pill,
        18,
        crate::surfaces::GlassPanelStyle::Popover,
        crate::config::DEFAULT_GLASS_OPACITY,
        false,
        rusty_ui::shadow::ShadowElevation::Popup,
    );
    out
}

fn push_osd_level_bar(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    pill: Rectangle<i32, smithay::utils::Physical>,
    pct: u8,
    muted: bool,
    pal: &ThemePalette,
) {
    let bar_x = pill.loc.x + 44;
    let bar_y = pill.loc.y + 34;
    let bar_w = (pill.size.w - 58).max(24);
    let bar_h = 8;
    let fill_w = (bar_w * i32::from(pct.min(100))) / 100;
    if fill_w > 0 {
        let fill_rgba = if muted {
            pal.popover_text_muted
        } else if pal.is_dark {
            [246, 246, 252, 236]
        } else {
            pal.accent_rgba
        };
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            Rectangle::new(Point::from((bar_x, bar_y)), Size::from((fill_w, bar_h))),
            fill_w.min(4),
            fill_rgba,
            None,
        );
    }
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        Rectangle::new(Point::from((bar_x, bar_y)), Size::from((bar_w, bar_h))),
        4,
        if pal.is_dark {
            [255, 255, 255, 30]
        } else {
            [0, 0, 0, 22]
        },
        Some(pal.border_rgba),
    );
}

/// Appends unified RustyOS chrome (hover close circle, app squircle, `NotificationCard` glass)
/// for both floating HUD toasts and Notification Center stack cards (`RUSTYOS-110`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_notification_card_chrome(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    card_rect: Rectangle<i32, Physical>,
    close_rect: Rectangle<i32, Physical>,
    title: &str,
    close_hovered: bool,
    glass_opacity: f32,
    reduce_transparency: bool,
    pal: &ThemePalette,
) {
    if close_hovered {
        let close_fill = if pal.is_dark {
            [255, 255, 255, 44]
        } else {
            [0, 0, 0, 26]
        };
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            crate::surfaces::UiGlyphKind::Close,
            10,
            pal.popover_icon,
            Point::from((close_rect.loc.x + 5, close_rect.loc.y + 5)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if let Some(el) = shape_cache.circle_element(
            renderer,
            20,
            close_fill,
            Some(pal.border_highlight_rgba),
            Point::from((close_rect.loc.x, close_rect.loc.y)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    let app_icon = notification_app_icon(title);
    if let Some(icon_el) = shape_cache.icons.builtin_squircle_icon_element(
        renderer,
        app_icon,
        NOTIFICATION_ICON_SIZE,
        Point::from((
            card_rect.loc.x + NOTIFICATION_ICON_OFFSET_X,
            card_rect.loc.y + NOTIFICATION_ICON_OFFSET_Y,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(icon_el));
    }

    push_liquid_glass_panel_elevation(
        out,
        shape_cache,
        renderer,
        card_rect,
        NOTIFICATION_CARD_RADIUS,
        crate::surfaces::GlassPanelStyle::NotificationCard,
        glass_opacity,
        reduce_transparency,
        rusty_ui::shadow::ShadowElevation::Popup,
    );
}

fn truncate_text_to_width(
    text_cache: &crate::text::TextCache,
    text: &str,
    weight: crate::text::FontWeight,
    size_px: f32,
    max_w: i32,
) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() || max_w <= 8 {
        return String::new();
    }
    if !text_cache.has_font()
        || text_cache.measure_text_weighted(trimmed, weight, size_px).0 <= max_w
    {
        return trimmed.to_owned();
    }
    let mut best_end = 0usize;
    for (idx, ch) in trimmed.char_indices() {
        let next_end = idx + ch.len_utf8();
        let cand = format!("{}...", &trimmed[..next_end]);
        if text_cache.measure_text_weighted(&cand, weight, size_px).0 <= max_w {
            best_end = next_end;
        } else {
            break;
        }
    }
    if best_end == 0 {
        let mut first = trimmed
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_default();
        first.push_str("...");
        first
    } else {
        format!("{}...", &trimmed[..best_end])
    }
}

#[must_use]
pub(crate) fn wrap_notification_lines(
    text_cache: &crate::text::TextCache,
    text: &str,
    size_px: f32,
    max_w: i32,
    max_lines: usize,
) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || max_lines == 0 || max_w <= 8 {
        return Vec::new();
    }
    let weight = crate::text::FontWeight::Regular;
    let mut lines: Vec<String> = Vec::new();
    let paragraphs: Vec<&str> = trimmed
        .split('\n')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();

    for (p_idx, para) in paragraphs.iter().enumerate() {
        if lines.len() >= max_lines {
            break;
        }
        let mut cur = String::new();
        let words: Vec<&str> = para.split_whitespace().collect();
        for (w_idx, &word) in words.iter().enumerate() {
            let cand = if cur.is_empty() {
                word.to_owned()
            } else {
                format!("{cur} {word}")
            };
            if !text_cache.has_font()
                || text_cache.measure_text_weighted(&cand, weight, size_px).0 <= max_w
            {
                cur = cand;
                continue;
            }
            if cur.is_empty() {
                lines.push(truncate_text_to_width(
                    text_cache, word, weight, size_px, max_w,
                ));
            } else if lines.len() + 1 == max_lines {
                let combined = format!("{cur} {}", words[w_idx..].join(" "));
                lines.push(truncate_text_to_width(
                    text_cache, &combined, weight, size_px, max_w,
                ));
                return lines;
            } else {
                lines.push(cur);
                cur = String::new();
                word.clone_into(&mut cur);
            }
            if lines.len() >= max_lines {
                return lines;
            }
        }
        if !cur.is_empty() {
            if lines.len() + 1 == max_lines && p_idx + 1 < paragraphs.len() {
                let appended = format!("{cur} {}", paragraphs[p_idx + 1]);
                lines.push(truncate_text_to_width(
                    text_cache, &appended, weight, size_px, max_w,
                ));
                return lines;
            }
            lines.push(cur);
        }
    }
    if let Some(last) = lines.last_mut()
        && text_cache.has_font()
        && text_cache.measure_text_weighted(last, weight, size_px).0 > max_w
    {
        *last = truncate_text_to_width(text_cache, last, weight, size_px, max_w);
    }
    lines
}

/// Appends unified RustyOS title and 1-2 line wrapped body labels without `"now"` (`RUSTYOS-110`).
pub(crate) fn push_notification_card_text_lines(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    card_rect: Rectangle<i32, Physical>,
    title: &str,
    body: &str,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let tx = card_rect.loc.x + NOTIFICATION_TEXT_OFFSET_X;
    let ty = card_rect.loc.y;
    let max_w =
        (card_rect.size.w - NOTIFICATION_TEXT_OFFSET_X - NOTIFICATION_TEXT_RIGHT_PAD).max(80);
    let body_lines = wrap_notification_lines(text_cache, body, 11.5, max_w, 2);

    let (title_size, title_y) = match body_lines.len() {
        0 => (13.0, ty + 25),
        1 => (13.0, ty + 16),
        _ => (12.5, ty + 9),
    };
    let title_fit = truncate_text_to_width(
        text_cache,
        title,
        crate::text::FontWeight::Bold,
        title_size,
        max_w,
    );
    if !title_fit.is_empty()
        && let Some(el) = text_cache.render_label_weighted(
            renderer,
            &title_fit,
            crate::text::FontWeight::Bold,
            title_size,
            pal.popover_text_primary,
            Point::from((tx, title_y)),
        )
    {
        elements.push(el);
    }

    let lead_body_fg = if pal.is_dark {
        [228, 228, 236, 255]
    } else {
        [38, 40, 48, 255]
    };
    for (idx, line) in body_lines.iter().enumerate() {
        let line_y = if body_lines.len() >= 2 {
            ty + 26 + i32::try_from(idx).unwrap_or(0) * 16
        } else {
            ty + 35
        };
        let fg = if idx == 0 {
            lead_body_fg
        } else {
            pal.popover_text_secondary
        };
        if let Some(el) =
            text_cache.render_label(renderer, line, 11.5, fg, Point::from((tx, line_y)))
        {
            elements.push(el);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_card_line_wrapping_handles_multiline_and_single_line() {
        let cache = crate::text::TextCache::default();
        let multi = wrap_notification_lines(
            &cache,
            "hey buddy\n—— Stephen Hackett stephen@hackett.fm",
            11.5,
            260,
            2,
        );
        assert_eq!(multi.len(), 2);
        assert_eq!(multi[0], "hey buddy");
        assert_eq!(multi[1], "—— Stephen Hackett stephen@hackett.fm");

        let rects = notification_toast_rects(Size::from((1280, 800)), 1);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].size, Size::from((344, 68)));
        assert_eq!(rects[0].loc.x, 1280 - 344 - 14);
    }
}
