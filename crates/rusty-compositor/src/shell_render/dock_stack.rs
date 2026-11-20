//! Graphical Dock `Downloads` Stack popover layout, hit-testing, and Liquid Glass rendering (`RUSTYOS-073`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Physical, Point, Rectangle, Size};

use crate::config::{DockPosition, DownloadsViewMode};
use crate::surfaces::{GlassPanelStyle, UiGlyphKind};

use super::CompositorRenderElement;
use super::tokens::{
    TOP_BAR_HEIGHT, ThemePalette, make_solid_element, push_liquid_glass_panel,
    push_liquid_glass_panel_elevation, push_rounded_glass_box, rect_contains, truncate_ascii,
};

const STACK_FAN_LIMIT: usize = 6;
const STACK_GRID_LIMIT: usize = 12;
const STACK_LIST_LIMIT: usize = 10;

#[inline]
fn count_to_i32(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// Single file/folder item shown inside a Dock Stack popover (`RUSTYOS-073`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockStackEntry {
    /// Display filename.
    pub name: String,
    /// Full host filesystem path opened when clicked.
    pub path: std::path::PathBuf,
    /// Whether this item is a directory.
    pub is_dir: bool,
    /// Secondary subtitle/metadata (`"Folder"`, `"42 KB"`, `"PDF"`, ...).
    pub detail: String,
    /// Uppercase short extension tag rendered on file badges (`"PDF"`, `"PNG"`, `"RS"`).
    pub extension: String,
}

/// Snapshot passed per frame to render and hit-test the Dock Stack popover (`RUSTYOS-073`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockStackRenderState<'a> {
    /// Sorted entries read from the target stack directory (`~/Downloads`).
    pub entries: &'a [DockStackEntry],
    /// Configured view mode (`Automatic`, `Fan`, `Grid`, or `List`).
    pub view_mode: DownloadsViewMode,
    /// Active Dock edge (`Bottom`, `Left`, `Right`) for popover anchoring.
    pub dock_position: DockPosition,
    /// Geometry of the clicked Dock slot (used to anchor Fan/Grid/List cleanly).
    pub anchor_rect: Rectangle<i32, Physical>,
    /// Currently hovered item index in `entries`.
    pub hovered_item: Option<usize>,
    /// Whether the `"Open in Files"` footer action is currently hovered.
    pub hovered_open_in_files: bool,
    /// Stack header label (e.g. `"Downloads"`).
    pub title: &'a str,
}

/// Interactive hit result inside an open Dock Stack (`RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockStackHit {
    /// User clicked an item (`index` into `entries`).
    Entry(usize),
    /// User clicked `"Open in Files"` in the stack footer.
    OpenInFiles,
    /// User clicked empty chrome inside the stack panel background.
    InsideBackground,
}

/// Single rendered slot inside [`DockStackLayout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockStackItemSlot {
    /// Original index inside `DockStackRenderState::entries`.
    pub entry_index: usize,
    /// Combined clickable bounding box.
    pub hit_rect: Rectangle<i32, Physical>,
    /// Icon/thumbnail badge rectangle.
    pub tile_rect: Rectangle<i32, Physical>,
    /// Optional separate text pill rectangle (used in `Fan` mode).
    pub pill_rect: Option<Rectangle<i32, Physical>>,
}

/// Single-source-of-truth geometry shared by Dock Stack rendering and pointer hit-testing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockStackLayout {
    /// Effective resolved mode (`Fan`, `Grid`, or `List`).
    pub resolved_mode: DownloadsViewMode,
    /// Glass container box for `Grid` and `List` (and empty `Fan`).
    pub panel_rect: Option<Rectangle<i32, Physical>>,
    /// Clickable visible entries.
    pub items: Vec<DockStackItemSlot>,
    /// Clickable `"Open in Files"` pill / footer row.
    pub footer_rect: Rectangle<i32, Physical>,
}

fn clamp_anchor_panel(
    screen_size: Size<i32, Physical>,
    anchor_rect: Rectangle<i32, Physical>,
    dock_position: DockPosition,
    panel_w: i32,
    panel_h: i32,
) -> Rectangle<i32, Physical> {
    let min_x = 10;
    let max_x = (screen_size.w - panel_w - 10).max(min_x);
    let min_y = TOP_BAR_HEIGHT + 8;
    let max_y = (screen_size.h - panel_h - 10).max(min_y);
    let (raw_x, raw_y) = match dock_position {
        DockPosition::Bottom => (
            anchor_rect.loc.x + (anchor_rect.size.w - panel_w) / 2,
            anchor_rect.loc.y - panel_h - 14,
        ),
        DockPosition::Left => (
            anchor_rect.loc.x + anchor_rect.size.w + 16,
            anchor_rect.loc.y + (anchor_rect.size.h - panel_h) / 2,
        ),
        DockPosition::Right => (
            anchor_rect.loc.x - panel_w - 16,
            anchor_rect.loc.y + (anchor_rect.size.h - panel_h) / 2,
        ),
    };
    Rectangle::new(
        Point::from((raw_x.clamp(min_x, max_x), raw_y.clamp(min_y, max_y))),
        Size::from((panel_w, panel_h)),
    )
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn compute_fan_layout(
    screen_size: Size<i32, Physical>,
    state: &DockStackRenderState<'_>,
) -> DockStackLayout {
    let count = state.entries.len().min(STACK_FAN_LIMIT);
    let anchor = state.anchor_rect;
    let curve_inward = if anchor.loc.x + anchor.size.w / 2 > screen_size.w / 2 {
        -1.0_f64
    } else {
        1.0_f64
    };
    let tile_sz = 44;
    let pill_w = 186;
    let pill_h = 30;
    let base_x = anchor.loc.x + (anchor.size.w - tile_sz) / 2;
    let mut items = Vec::with_capacity(count);
    let step_y = ((anchor.loc.y - TOP_BAR_HEIGHT - 64) / count_to_i32(count.max(1))).clamp(38, 52);

    for idx in 0..count {
        let step = f64::from(count_to_i32(idx));
        let dx = (curve_inward * step * step * 3.8).round() as i32;
        let tx = (base_x + dx).clamp(12, (screen_size.w - tile_sz - 12).max(12));
        let ty = (anchor.loc.y - 16 - (count_to_i32(idx) + 1) * step_y).max(TOP_BAR_HEIGHT + 44);
        let tile_rect = Rectangle::new(Point::from((tx, ty)), Size::from((tile_sz, tile_sz)));

        let pill_x = if curve_inward < 0.0 {
            (tx - pill_w - 10).max(10)
        } else {
            (tx + tile_sz + 10).min((screen_size.w - pill_w - 10).max(10))
        };
        let pill_rect = Rectangle::new(
            Point::from((pill_x, ty + (tile_sz - pill_h) / 2)),
            Size::from((pill_w, pill_h)),
        );
        let hit_x = tx.min(pill_x);
        let hit_right = (tx + tile_sz).max(pill_x + pill_w);
        let hit_rect = Rectangle::new(
            Point::from((hit_x, ty)),
            Size::from(((hit_right - hit_x).max(tile_sz), tile_sz)),
        );
        items.push(DockStackItemSlot {
            entry_index: idx,
            hit_rect,
            tile_rect,
            pill_rect: Some(pill_rect),
        });
    }

    let top_y = items
        .last()
        .map_or(anchor.loc.y - 68, |slot| slot.tile_rect.loc.y - 38)
        .max(TOP_BAR_HEIGHT + 8);
    let foot_w = 148;
    let foot_x = if curve_inward < 0.0 {
        (base_x - foot_w - 10).max(10)
    } else {
        (base_x + tile_sz + 10).min((screen_size.w - foot_w - 10).max(10))
    };
    DockStackLayout {
        resolved_mode: DownloadsViewMode::Fan,
        panel_rect: None,
        items,
        footer_rect: Rectangle::new(Point::from((foot_x, top_y)), Size::from((foot_w, 28))),
    }
}

fn compute_grid_layout(
    screen_size: Size<i32, Physical>,
    state: &DockStackRenderState<'_>,
) -> DockStackLayout {
    let count = state.entries.len().min(STACK_GRID_LIMIT);
    let cols = if count <= 3 { count.max(2) } else { 4 };
    let rows = count.max(1).div_ceil(cols);
    let cell_w = 78;
    let cell_h = 74;
    let gap = 8;
    let pad = 14;
    let header_h = 30;
    let footer_h = 32;
    let panel_w =
        pad * 2 + count_to_i32(cols) * cell_w + count_to_i32(cols.saturating_sub(1)) * gap;
    let body_h = if count == 0 {
        48
    } else {
        count_to_i32(rows) * cell_h + count_to_i32(rows.saturating_sub(1)) * gap
    };
    let panel_h = pad + header_h + body_h + 10 + footer_h + 10;
    let panel = clamp_anchor_panel(
        screen_size,
        state.anchor_rect,
        state.dock_position,
        panel_w,
        panel_h,
    );

    let mut items = Vec::with_capacity(count);
    for idx in 0..count {
        let col = count_to_i32(idx % cols);
        let row = count_to_i32(idx / cols);
        let cx = panel.loc.x + pad + col * (cell_w + gap);
        let cy = panel.loc.y + 8 + header_h + row * (cell_h + gap);
        let hit_rect = Rectangle::new(Point::from((cx, cy)), Size::from((cell_w, cell_h)));
        let tile_rect = Rectangle::new(
            Point::from((cx + (cell_w - 38) / 2, cy + 8)),
            Size::from((38, 38)),
        );
        items.push(DockStackItemSlot {
            entry_index: idx,
            hit_rect,
            tile_rect,
            pill_rect: None,
        });
    }
    let footer_rect = Rectangle::new(
        Point::from((panel.loc.x + 10, panel.loc.y + panel.size.h - footer_h - 6)),
        Size::from((panel.size.w - 20, footer_h)),
    );
    DockStackLayout {
        resolved_mode: DownloadsViewMode::Grid,
        panel_rect: Some(panel),
        items,
        footer_rect,
    }
}

fn compute_list_layout(
    screen_size: Size<i32, Physical>,
    state: &DockStackRenderState<'_>,
) -> DockStackLayout {
    let count = state.entries.len().min(STACK_LIST_LIMIT);
    let panel_w = 278;
    let header_h = 30;
    let row_h = 28;
    let footer_h = 30;
    let body_h = if count == 0 {
        40
    } else {
        count_to_i32(count) * row_h
    };
    let panel_h = 8 + header_h + 6 + body_h + 12 + footer_h + 6;
    let panel = clamp_anchor_panel(
        screen_size,
        state.anchor_rect,
        state.dock_position,
        panel_w,
        panel_h,
    );

    let mut items = Vec::with_capacity(count);
    for idx in 0..count {
        let ry = panel.loc.y + 8 + header_h + 6 + count_to_i32(idx) * row_h;
        let hit_rect = Rectangle::new(
            Point::from((panel.loc.x + 8, ry)),
            Size::from((panel_w - 16, row_h)),
        );
        let tile_rect = Rectangle::new(
            Point::from((panel.loc.x + 16, ry + 4)),
            Size::from((20, 20)),
        );
        items.push(DockStackItemSlot {
            entry_index: idx,
            hit_rect,
            tile_rect,
            pill_rect: None,
        });
    }
    let footer_rect = Rectangle::new(
        Point::from((panel.loc.x + 8, panel.loc.y + panel.size.h - footer_h - 6)),
        Size::from((panel_w - 16, footer_h)),
    );
    DockStackLayout {
        resolved_mode: DownloadsViewMode::List,
        panel_rect: Some(panel),
        items,
        footer_rect,
    }
}

/// Computes single-source-of-truth geometry for the active Dock Stack popover (`RUSTYOS-073`).
#[must_use]
pub fn compute_dock_stack_layout(
    screen_size: Size<i32, Physical>,
    state: &DockStackRenderState<'_>,
) -> DockStackLayout {
    let resolved = match state.view_mode.resolve_for_count(state.entries.len()) {
        DownloadsViewMode::Fan
            if !state.entries.is_empty() && state.dock_position == DockPosition::Bottom =>
        {
            DownloadsViewMode::Fan
        }
        DownloadsViewMode::Fan => {
            if state.entries.is_empty() {
                DownloadsViewMode::List
            } else {
                DownloadsViewMode::Grid
            }
        }
        other => other,
    };
    match resolved {
        DownloadsViewMode::Fan => compute_fan_layout(screen_size, state),
        DownloadsViewMode::Grid | DownloadsViewMode::Automatic => {
            compute_grid_layout(screen_size, state)
        }
        DownloadsViewMode::List => compute_list_layout(screen_size, state),
    }
}

/// Performs pointer hit-testing inside the open Dock Stack (`RUSTYOS-073`).
#[must_use]
pub fn hit_test_dock_stack(
    screen_size: Size<i32, Physical>,
    state: &DockStackRenderState<'_>,
    x: f64,
    y: f64,
) -> Option<DockStackHit> {
    let layout = compute_dock_stack_layout(screen_size, state);
    if rect_contains(layout.footer_rect, x, y) {
        return Some(DockStackHit::OpenInFiles);
    }
    for item in &layout.items {
        if rect_contains(item.hit_rect, x, y) {
            return Some(DockStackHit::Entry(item.entry_index));
        }
    }
    if layout
        .panel_rect
        .is_some_and(|panel| rect_contains(panel, x, y))
    {
        return Some(DockStackHit::InsideBackground);
    }
    None
}

fn entry_badge_colors(entry: &DockStackEntry, pal: &ThemePalette) -> ([u8; 4], [u8; 4]) {
    if entry.is_dir {
        ([64, 156, 255, 225], [120, 192, 255, 190])
    } else {
        match entry.extension.as_str() {
            "PDF" => ([232, 74, 74, 232], [255, 130, 130, 170]),
            "PNG" | "JPG" | "WEBP" | "SVG" => ([48, 176, 199, 230], [116, 214, 228, 170]),
            "ZIP" | "TAR" | "GZ" | "XZ" => ([245, 158, 56, 230], [255, 198, 118, 170]),
            _ => (pal.control_pill_off_rgba, pal.border_rgba),
        }
    }
}

fn push_item_badge(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    entry: &DockStackEntry,
    tile_rect: Rectangle<i32, Physical>,
    pal: &ThemePalette,
) {
    let (fill, border) = entry_badge_colors(entry, pal);
    let glyph = if entry.is_dir {
        UiGlyphKind::Folder
    } else {
        UiGlyphKind::Info
    };
    let g_size = (tile_rect.size.w * 54 / 100).clamp(11, 22);
    let gx = tile_rect.loc.x + (tile_rect.size.w - g_size) / 2;
    let gy = tile_rect.loc.y + (tile_rect.size.h - g_size) / 2;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        glyph,
        g_size,
        [255, 255, 255, 240],
        Point::from((gx, gy)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        tile_rect,
        (tile_rect.size.w / 4).clamp(5, 10),
        fill,
        Some(border),
    );
}

#[allow(clippy::too_many_arguments)]
fn render_dock_stack_item_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    resolved_mode: DownloadsViewMode,
    slot: &DockStackItemSlot,
    entry: &DockStackEntry,
    hovered: bool,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let fg = if hovered && resolved_mode != DownloadsViewMode::Grid {
        [255, 255, 255, 255]
    } else {
        pal.popover_text_primary
    };
    match resolved_mode {
        DownloadsViewMode::Fan => {
            if let Some(pill) = slot.pill_rect {
                let shown = truncate_ascii(&entry.name, 22);
                if let Some(el) = text_cache.render_label(
                    renderer,
                    &shown,
                    12.0,
                    fg,
                    Point::from((pill.loc.x + 11, pill.loc.y + 7)),
                ) {
                    elements.push(el);
                }
            }
        }
        DownloadsViewMode::Grid | DownloadsViewMode::Automatic => {
            let shown = truncate_ascii(&entry.name, 11);
            let lbl_bounds = Rectangle::new(
                Point::from((slot.hit_rect.loc.x + 2, slot.hit_rect.loc.y + 50)),
                Size::from((slot.hit_rect.size.w - 4, 20)),
            );
            if let Some(el) =
                text_cache.render_label_centered(renderer, &shown, 11.0, fg, lbl_bounds)
            {
                elements.push(el);
            }
        }
        DownloadsViewMode::List => {
            let shown = truncate_ascii(&entry.name, 24);
            if let Some(el) = text_cache.render_label(
                renderer,
                &shown,
                12.0,
                fg,
                Point::from((slot.hit_rect.loc.x + 34, slot.hit_rect.loc.y + 7)),
            ) {
                elements.push(el);
            }
            let detail = truncate_ascii(&entry.detail, 10);
            let (dw, _) = text_cache.measure_text(&detail, 10.5);
            let d_fg = if hovered {
                [240, 245, 255, 255]
            } else {
                pal.popover_text_muted
            };
            if let Some(el) = text_cache.render_label(
                renderer,
                &detail,
                10.5,
                d_fg,
                Point::from((
                    slot.hit_rect.loc.x + slot.hit_rect.size.w - 10 - dw,
                    slot.hit_rect.loc.y + 8,
                )),
            ) {
                elements.push(el);
            }
        }
    }
}

/// Appends Dock Stack text labels in front-to-back order (`RUSTYOS-073`).
pub(super) fn render_dock_stack_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &DockStackRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = compute_dock_stack_layout(screen_size, state);
    let pal = ThemePalette::active();

    if let Some(panel) = layout.panel_rect {
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            state.title,
            crate::text::FontWeight::Bold,
            13.0,
            pal.popover_text_primary,
            Point::from((panel.loc.x + 14, panel.loc.y + 10)),
        ) {
            elements.push(el);
        }
        let count_str = format!("{} items", state.entries.len());
        let (cw, _) = text_cache.measure_text(&count_str, 11.0);
        if let Some(el) = text_cache.render_label(
            renderer,
            &count_str,
            11.0,
            pal.popover_text_muted,
            Point::from((panel.loc.x + panel.size.w - 14 - cw, panel.loc.y + 12)),
        ) {
            elements.push(el);
        }
        if state.entries.is_empty() {
            let empty_bounds = Rectangle::new(
                Point::from((panel.loc.x + 12, panel.loc.y + 42)),
                Size::from((panel.size.w - 24, 30)),
            );
            if let Some(el) = text_cache.render_label_centered(
                renderer,
                "Downloads is empty",
                12.0,
                pal.popover_text_muted,
                empty_bounds,
            ) {
                elements.push(el);
            }
        }
    }

    for slot in &layout.items {
        if let Some(entry) = state.entries.get(slot.entry_index) {
            render_dock_stack_item_text(
                text_cache,
                renderer,
                layout.resolved_mode,
                slot,
                entry,
                state.hovered_item == Some(slot.entry_index),
                &pal,
                elements,
            );
        }
    }

    let foot_fg = if state.hovered_open_in_files {
        [255, 255, 255, 255]
    } else {
        pal.accent_rgba
    };
    let foot_bounds = Rectangle::new(
        Point::from((layout.footer_rect.loc.x, layout.footer_rect.loc.y)),
        Size::from((layout.footer_rect.size.w, layout.footer_rect.size.h)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        "Open in Files",
        crate::text::FontWeight::Bold,
        12.0,
        foot_fg,
        foot_bounds,
    ) {
        elements.push(el);
    }
}

/// Appends Dock Stack Liquid Glass panels, file badges, and hover pills (`RUSTYOS-073`).
#[allow(clippy::too_many_arguments)]
pub(super) fn render_dock_stack_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    glass_opacity: f32,
    reduce_transparency: bool,
    state: &DockStackRenderState<'_>,
) {
    let layout = compute_dock_stack_layout(screen_size, state);
    let pal = ThemePalette::active();

    if state.hovered_open_in_files {
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            layout.footer_rect,
            9,
            pal.accent_hover_fill,
            Some(pal.accent_hover_border),
        );
    } else if layout.panel_rect.is_none() {
        push_liquid_glass_panel_elevation(
            out,
            shape_cache,
            renderer,
            layout.footer_rect,
            14,
            GlassPanelStyle::Popover,
            glass_opacity,
            reduce_transparency,
            rusty_ui::shadow::ShadowElevation::Popup,
        );
    }

    for slot in &layout.items {
        let Some(entry) = state.entries.get(slot.entry_index) else {
            continue;
        };
        let hovered = state.hovered_item == Some(slot.entry_index);
        push_item_badge(out, shape_cache, renderer, entry, slot.tile_rect, &pal);
        if let Some(pill) = slot.pill_rect {
            if hovered {
                push_rounded_glass_box(
                    out,
                    shape_cache,
                    renderer,
                    pill,
                    12,
                    pal.accent_hover_fill,
                    Some(pal.accent_hover_border),
                );
            } else {
                push_liquid_glass_panel(
                    out,
                    shape_cache,
                    renderer,
                    pill,
                    12,
                    GlassPanelStyle::Popover,
                    glass_opacity,
                    reduce_transparency,
                    Some((10, 38)),
                );
            }
        } else if hovered {
            let hl = if layout.resolved_mode == DownloadsViewMode::Grid {
                if pal.is_dark {
                    [255, 255, 255, 32]
                } else {
                    [0, 0, 0, 20]
                }
            } else {
                pal.accent_hover_fill
            };
            push_rounded_glass_box(out, shape_cache, renderer, slot.hit_rect, 8, hl, None);
        }
    }

    if let Some(panel) = layout.panel_rect {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((panel.loc.x + 12, layout.footer_rect.loc.y - 4)),
                Size::from((panel.size.w - 24, 1)),
            ),
            pal.popover_separator,
        )));
        push_liquid_glass_panel_elevation(
            out,
            shape_cache,
            renderer,
            panel,
            16,
            GlassPanelStyle::Popover,
            glass_opacity,
            reduce_transparency,
            rusty_ui::shadow::ShadowElevation::Popup,
        );
    }
}
