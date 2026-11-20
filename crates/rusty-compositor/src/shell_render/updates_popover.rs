//! `TopBar` Software Updates `Liquid Glass` popover layout, hit-testing, and rendering (`RUSTYOS-076`).

use rusty_services::{PackageUpdateItem, UpdateCheckState, UpdateStatusSnapshot};
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Physical, Point, Rectangle, Size};

use crate::surfaces::{GlassPanelStyle, UiGlyphKind};

use super::CompositorRenderElement;
use super::tokens::{
    TOP_BAR_HEIGHT, ThemePalette, make_solid_element, push_liquid_glass_panel_elevation,
    push_rounded_glass_box, scale_glass_rgba, truncate_ascii,
};

const UPDATES_POPOVER_WIDTH: i32 = 340;
const UPDATES_HEADER_H: i32 = 58;
/// Maximum number of package rows shown simultaneously inside the popover.
pub const MAX_POPOVER_UPDATE_ITEMS: usize = 6;

/// Interactive hit-test regions inside [`ShellOverlayView::Updates`] (`RUSTYOS-076`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatesPopoverHit {
    /// Trigger an immediate background `checkupdates` refresh.
    CheckNow,
    /// Launch System Settings on the Software Update page (`RUSTYOS-123`).
    OpenUpdater,
    /// Click fell inside the popover panel background.
    InsidePopover,
    /// Click fell outside the popover (dismisses popover).
    OutsidePopover,
}

/// Live view state driving [`ShellOverlayView::Updates`] (`RUSTYOS-076`).
#[derive(Debug, Clone, Copy)]
pub struct UpdatesRenderState<'a> {
    /// Global glass opacity factor (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell mode.
    pub reduce_transparency: bool,
    /// Latest update checker snapshot from `rusty_services`.
    pub snapshot: &'a UpdateStatusSnapshot,
    /// Right anchor X coordinate in `TopBar` coordinates (typically near the Updates glyph).
    pub anchor_right_x: i32,
    /// Currently hovered action button inside the popover.
    pub hovered_hit: Option<UpdatesPopoverHit>,
    /// Whether the `rusty-updater` Stage B companion binary is present in `PATH`.
    pub updater_installed: bool,
}

/// Precomputed physical/logical rectangles for the Software Updates popover (`RUSTYOS-076`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatesPopoverLayout {
    /// Outer popover frame.
    pub panel_rect: Rectangle<i32, Physical>,
    /// Leading blue update badge in header.
    pub badge_rect: Rectangle<i32, Physical>,
    /// Circular top-right refresh button inside header.
    pub refresh_btn_rect: Rectangle<i32, Physical>,
    /// Summary strip rect when pending updates are listed.
    pub summary_rect: Option<Rectangle<i32, Physical>>,
    /// Per-package row rectangles (`<= MAX_POPOVER_UPDATE_ITEMS`).
    pub item_rects: Vec<Rectangle<i32, Physical>>,
    /// Empty / unavailable status card when no package rows are shown.
    pub empty_card_rect: Option<Rectangle<i32, Physical>>,
    /// Overflow line (`"+ N more packages"`) when list exceeds [`MAX_POPOVER_UPDATE_ITEMS`].
    pub overflow_rect: Option<Rectangle<i32, Physical>>,
    /// Secondary footer action button (`Check Now`).
    pub check_btn_rect: Rectangle<i32, Physical>,
    /// Primary footer accent action button (`Open Updater...`).
    pub open_btn_rect: Rectangle<i32, Physical>,
    /// Separator Y coordinate beneath the header.
    pub header_sep_y: i32,
    /// Separator Y coordinate above the footer actions.
    pub footer_sep_y: i32,
}

fn point_in_rect(rect: Rectangle<i32, Physical>, x: f64, y: f64) -> bool {
    x >= f64::from(rect.loc.x)
        && x < f64::from(rect.loc.x + rect.size.w)
        && y >= f64::from(rect.loc.y)
        && y < f64::from(rect.loc.y + rect.size.h)
}

fn to_logical_rect(rect: Rectangle<i32, Physical>) -> Rectangle<i32, Logical> {
    Rectangle::new(
        Point::from((rect.loc.x, rect.loc.y)),
        Size::from((rect.size.w, rect.size.h)),
    )
}

/// Computes the Golden Gate `Liquid Glass` popover geometry anchored below `TopBar` (`RUSTYOS-076`).
#[must_use]
pub fn compute_updates_popover_layout(
    screen_size: Size<i32, Physical>,
    state: &UpdatesRenderState<'_>,
) -> UpdatesPopoverLayout {
    let w = UPDATES_POPOVER_WIDTH.min((screen_size.w - 24).max(268));
    let px = (state.anchor_right_x - w + 12).clamp(12, (screen_size.w - w - 12).max(12));
    let py = TOP_BAR_HEIGHT + 8;

    let badge_rect = Rectangle::new(Point::from((px + 14, py + 12)), Size::from((34, 34)));
    let refresh_btn_rect =
        Rectangle::new(Point::from((px + w - 42, py + 15)), Size::from((28, 28)));
    let header_sep_y = py + UPDATES_HEADER_H;

    let visible_items = state.snapshot.items.len().min(MAX_POPOVER_UPDATE_ITEMS);
    let overflow_items = state
        .snapshot
        .total_count
        .max(state.snapshot.items.len())
        .saturating_sub(MAX_POPOVER_UPDATE_ITEMS);

    let mut cur_y = header_sep_y + 8;
    let mut summary_rect = None;
    let mut item_rects = Vec::with_capacity(visible_items);
    let mut empty_card_rect = None;
    let mut overflow_rect = None;

    if visible_items > 0 {
        summary_rect = Some(Rectangle::new(
            Point::from((px + 14, cur_y)),
            Size::from((w - 28, 24)),
        ));
        cur_y += 30;
        for _ in 0..visible_items {
            item_rects.push(Rectangle::new(
                Point::from((px + 12, cur_y)),
                Size::from((w - 24, 26)),
            ));
            cur_y += 28;
        }
        if overflow_items > 0 {
            overflow_rect = Some(Rectangle::new(
                Point::from((px + 14, cur_y)),
                Size::from((w - 28, 20)),
            ));
            cur_y += 24;
        }
    } else {
        empty_card_rect = Some(Rectangle::new(
            Point::from((px + 14, cur_y)),
            Size::from((w - 28, 56)),
        ));
        cur_y += 62;
    }

    let footer_sep_y = cur_y + 2;
    let footer_y = footer_sep_y + 10;
    let check_btn_rect = Rectangle::new(Point::from((px + 14, footer_y)), Size::from((98, 28)));
    let open_btn_rect =
        Rectangle::new(Point::from((px + 120, footer_y)), Size::from((w - 134, 28)));
    let total_h = (footer_y + 28 + 12) - py;
    let panel_rect = Rectangle::new(Point::from((px, py)), Size::from((w, total_h)));

    UpdatesPopoverLayout {
        panel_rect,
        badge_rect,
        refresh_btn_rect,
        summary_rect,
        item_rects,
        empty_card_rect,
        overflow_rect,
        check_btn_rect,
        open_btn_rect,
        header_sep_y,
        footer_sep_y,
    }
}

/// Hit-tests pointer coordinates against [`ShellOverlayView::Updates`] (`RUSTYOS-076`).
#[must_use]
pub fn hit_test_updates_popover(
    screen_size: Size<i32, Physical>,
    state: &UpdatesRenderState<'_>,
    x: f64,
    y: f64,
) -> UpdatesPopoverHit {
    let layout = compute_updates_popover_layout(screen_size, state);
    if point_in_rect(layout.refresh_btn_rect, x, y) || point_in_rect(layout.check_btn_rect, x, y) {
        return UpdatesPopoverHit::CheckNow;
    }
    if point_in_rect(layout.open_btn_rect, x, y) {
        return UpdatesPopoverHit::OpenUpdater;
    }
    if point_in_rect(layout.panel_rect, x, y) {
        return UpdatesPopoverHit::InsidePopover;
    }
    UpdatesPopoverHit::OutsidePopover
}

fn updates_header_copy(snapshot: &UpdateStatusSnapshot) -> (String, String) {
    let title = match snapshot.state {
        UpdateCheckState::Checking => "Checking for Updates...".to_owned(),
        UpdateCheckState::Available => {
            if snapshot.total_count == 1 {
                "1 Update Available".to_owned()
            } else {
                format!("{} Updates Available", snapshot.total_count)
            }
        }
        UpdateCheckState::UpToDate => "RustyOS is Up to Date".to_owned(),
        UpdateCheckState::Unavailable => "Update Check Unavailable".to_owned(),
        UpdateCheckState::Idle => "Software Updates".to_owned(),
    };
    let subtitle = match snapshot.state {
        UpdateCheckState::Checking => "Querying pacman checkupdates...".to_owned(),
        UpdateCheckState::Unavailable => snapshot
            .error_hint
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "checkupdates not found".to_owned()),
        _ => {
            if snapshot.last_checked_label.is_empty() || snapshot.last_checked_label == "Never" {
                "Arch + RustyOS packages".to_owned()
            } else {
                format!("Checked {}", snapshot.last_checked_label)
            }
        }
    };
    (truncate_ascii(&title, 28), truncate_ascii(&subtitle, 34))
}

fn push_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    text: &str,
    size: f32,
    rgba: [u8; 4],
    pos: (i32, i32),
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(el) = text_cache.render_label(renderer, text, size, rgba, Point::from(pos)) {
        elements.push(el);
    }
}

fn push_bold_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    text: &str,
    size: f32,
    rgba: [u8; 4],
    pos: (i32, i32),
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        text,
        crate::text::FontWeight::Bold,
        size,
        rgba,
        Point::from(pos),
    ) {
        elements.push(el);
    }
}

fn render_updates_item_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    row_rect: Rectangle<i32, Physical>,
    item: &PackageUpdateItem,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let left_pad = if item.is_rustyos || item.requires_restart {
        22
    } else {
        10
    };
    let name_text = truncate_ascii(&item.name, 21);
    let name_color = if item.is_rustyos {
        pal.accent_rgba
    } else {
        pal.popover_text_primary
    };
    let (nw, _) = text_cache.measure_text(&name_text, 11.5);
    if item.is_rustyos {
        push_bold_text(
            text_cache,
            renderer,
            &name_text,
            11.5,
            name_color,
            (row_rect.loc.x + left_pad, row_rect.loc.y + 6),
            elements,
        );
    } else {
        push_text(
            text_cache,
            renderer,
            &name_text,
            11.5,
            name_color,
            (row_rect.loc.x + left_pad, row_rect.loc.y + 6),
            elements,
        );
    }

    let ver_str = truncate_ascii(&format!("{} -> {}", item.old_version, item.new_version), 27);
    let (vw, _) = text_cache.measure_text(&ver_str, 10.5);
    let max_ver_x = row_rect.loc.x + row_rect.size.w - 10 - vw;
    if max_ver_x > row_rect.loc.x + left_pad + nw + 10 {
        push_text(
            text_cache,
            renderer,
            &ver_str,
            10.5,
            pal.popover_text_muted,
            (max_ver_x, row_rect.loc.y + 7),
            elements,
        );
    }
}

fn render_updates_body_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &UpdatesPopoverLayout,
    snapshot: &UpdateStatusSnapshot,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(summary) = layout.summary_rect {
        let rusty_state = if snapshot.rustyos_available {
            "RustyOS: Update ready"
        } else {
            "RustyOS: Current"
        };
        let mut summary_text = format!("Arch: {}  ·  {rusty_state}", snapshot.arch_count);
        if snapshot.restart_recommended {
            summary_text.push_str("  ·  Restart");
        }
        let clipped = truncate_ascii(&summary_text, 44);
        push_text(
            text_cache,
            renderer,
            &clipped,
            11.0,
            pal.popover_text_muted,
            (summary.loc.x + 8, summary.loc.y + 5),
            elements,
        );
    }

    for (rect, item) in layout.item_rects.iter().zip(&snapshot.items) {
        render_updates_item_text(text_cache, renderer, *rect, item, pal, elements);
    }

    if let Some(ov_rect) = layout.overflow_rect {
        let extra = snapshot
            .total_count
            .max(snapshot.items.len())
            .saturating_sub(layout.item_rects.len());
        let more_label = format!("+{extra} more packages");
        push_text(
            text_cache,
            renderer,
            &more_label,
            11.0,
            pal.popover_text_muted,
            (ov_rect.loc.x + 6, ov_rect.loc.y + 3),
            elements,
        );
    }

    if let Some(empty_rect) = layout.empty_card_rect {
        let line = if snapshot.state == UpdateCheckState::Unavailable {
            "pacman-contrib (checkupdates) required."
        } else if snapshot.state == UpdateCheckState::Checking {
            "Fetching Arch & RustyOS package status..."
        } else {
            "All installed Arch & RustyOS packages are current."
        };
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            truncate_ascii(line, 44).as_str(),
            11.0,
            pal.popover_text_muted,
            to_logical_rect(empty_rect),
        ) {
            elements.push(el);
        }
    }
}

fn render_updates_footer_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &UpdatesPopoverLayout,
    state: &UpdatesRenderState<'_>,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let check_label = if state.snapshot.state == UpdateCheckState::Checking {
        "Checking..."
    } else {
        "Check Now"
    };
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        check_label,
        crate::text::FontWeight::Bold,
        11.5,
        pal.popover_text_primary,
        to_logical_rect(layout.check_btn_rect),
    ) {
        elements.push(el);
    }

    let open_label = "Software Update...";
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        open_label,
        crate::text::FontWeight::Bold,
        11.5,
        [255, 255, 255, 255],
        to_logical_rect(layout.open_btn_rect),
    ) {
        elements.push(el);
    }
}

/// Appends Software Updates popover text labels (`RUSTYOS-076`).
pub(super) fn render_updates_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &UpdatesRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = compute_updates_popover_layout(screen_size, state);
    let pal = ThemePalette::active();
    let px = layout.panel_rect.loc.x;
    let py = layout.panel_rect.loc.y;

    let (title, subtitle) = updates_header_copy(state.snapshot);
    push_bold_text(
        text_cache,
        renderer,
        &title,
        13.0,
        pal.popover_text_primary,
        (px + 58, py + 12),
        elements,
    );
    push_text(
        text_cache,
        renderer,
        &subtitle,
        11.0,
        pal.popover_text_muted,
        (px + 58, py + 31),
        elements,
    );

    render_updates_body_text(
        text_cache,
        renderer,
        &layout,
        state.snapshot,
        &pal,
        elements,
    );
    render_updates_footer_text(text_cache, renderer, &layout, state, &pal, elements);
}

fn push_updates_glyphs(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &UpdatesPopoverLayout,
    snapshot: &UpdateStatusSnapshot,
    pal: &ThemePalette,
) {
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Updates { available: false },
        18,
        [255, 255, 255, 255],
        Point::from((layout.badge_rect.loc.x + 8, layout.badge_rect.loc.y + 8)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Refresh,
        14,
        pal.popover_icon,
        Point::from((
            layout.refresh_btn_rect.loc.x + 7,
            layout.refresh_btn_rect.loc.y + 7,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    for (rect, item) in layout.item_rects.iter().zip(&snapshot.items) {
        if item.is_rustyos || item.requires_restart {
            let dot_color = if item.is_rustyos {
                pal.accent_rgba
            } else {
                [245, 166, 35, 255]
            };
            if let Some(el) = shape_cache.circle_element(
                renderer,
                6,
                dot_color,
                None,
                Point::from((rect.loc.x + 10, rect.loc.y + 10)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
        }
    }
}

fn push_updates_cards_and_buttons(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &UpdatesPopoverLayout,
    state: &UpdatesRenderState<'_>,
    pal: &ThemePalette,
) {
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.badge_rect,
        10,
        pal.accent_hover_fill,
        Some(pal.accent_hover_border),
    );
    let refresh_fill = if state.hovered_hit == Some(UpdatesPopoverHit::CheckNow) {
        pal.accent_hover_fill
    } else {
        pal.control_pill_off_rgba
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.refresh_btn_rect,
        14,
        refresh_fill,
        Some(pal.border_rgba),
    );

    let row_fill = scale_glass_rgba(
        if pal.is_dark {
            [40, 42, 48, 152]
        } else {
            [244, 246, 250, 178]
        },
        state.glass_opacity,
        state.reduce_transparency,
    );
    if let Some(summary_rect) = layout.summary_rect {
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            summary_rect,
            8,
            row_fill,
            Some(pal.border_rgba),
        );
    }
    for rect in &layout.item_rects {
        push_rounded_glass_box(out, shape_cache, renderer, *rect, 8, row_fill, None);
    }
    if let Some(empty_rect) = layout.empty_card_rect {
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            empty_rect,
            10,
            row_fill,
            Some(pal.border_rgba),
        );
    }

    let check_fill = if state.hovered_hit == Some(UpdatesPopoverHit::CheckNow) {
        pal.accent_hover_fill
    } else {
        pal.control_pill_off_rgba
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.check_btn_rect,
        9,
        check_fill,
        Some(pal.border_rgba),
    );
    let open_fill = if state.hovered_hit == Some(UpdatesPopoverHit::OpenUpdater) {
        pal.accent_rgba
    } else {
        pal.accent_hover_fill
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.open_btn_rect,
        9,
        open_fill,
        Some(pal.accent_hover_border),
    );
}

/// Appends Software Updates popover Liquid Glass surfaces, badges, and buttons (`RUSTYOS-076`).
pub(super) fn render_updates_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &UpdatesRenderState<'_>,
) {
    let layout = compute_updates_popover_layout(screen_size, state);
    let pal = ThemePalette::active();

    push_updates_glyphs(out, shape_cache, renderer, &layout, state.snapshot, &pal);
    push_updates_cards_and_buttons(out, shape_cache, renderer, &layout, state, &pal);

    for sep_y in [layout.header_sep_y, layout.footer_sep_y] {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((layout.panel_rect.loc.x + 14, sep_y)),
                Size::from((layout.panel_rect.size.w - 28, 1)),
            ),
            pal.popover_separator,
        )));
    }

    push_liquid_glass_panel_elevation(
        out,
        shape_cache,
        renderer,
        layout.panel_rect,
        16,
        GlassPanelStyle::Popover,
        state.glass_opacity,
        state.reduce_transparency,
        rusty_ui::shadow::ShadowElevation::Popup,
    );
}
