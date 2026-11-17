//! `TopBar` notification HUD toasts, unread notification badges, and floating OSD pills (`RUSTYOS-090`).

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{
    DOCK_HEIGHT, DOCK_MARGIN_BOTTOM, TOP_BAR_HEIGHT, ThemePalette, push_liquid_glass_panel,
    push_rounded_glass_box,
};
#[cfg(test)]
use crate::shell_render::tokens::{TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y};

#[cfg(test)]
use super::panel::compute_top_bar_right_layout;
use super::panel::{TopBarLayoutOptions, compute_top_bar_right_layout_with_options};

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
    let toast_w = 340.min((width - 32).max(200));
    let toast_h = 68;
    let gap = 10;
    let toast_x = width - toast_w - 16;
    let start_y = TOP_BAR_HEIGHT + 12;

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
    x >= f64::from(layout.notif_x) && x < f64::from(layout.notif_x + layout.notif_w)
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

    push_liquid_glass_panel(
        &mut out,
        shape_cache,
        renderer,
        pill,
        18,
        crate::surfaces::GlassPanelStyle::Popover,
        crate::config::DEFAULT_GLASS_OPACITY,
        false,
        Some((14, 46)),
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
