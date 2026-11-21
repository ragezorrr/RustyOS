//! Interactive Screenshot HUD (`Region / Window / Full Screen`, `Copy / Save`, `RUSTYOS-133`).

use rusty_wm::Rect;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{
    DOCK_HEIGHT, DOCK_MARGIN_BOTTOM, ThemePalette, make_solid_element, premul,
    push_liquid_glass_panel_elevation, push_rounded_glass_box, rect_contains,
};
use crate::shell_render::top_bar::TopBarTelemetry;

use super::phys_to_logical_rect;

/// Active capture mode in the interactive `ScreenshotHud` (`RUSTYOS-133`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotMode {
    /// Drag to select an arbitrary rectangular screen region.
    Region,
    /// Hover and click a window to capture its exact bounds.
    Window,
    /// Capture the full active display output.
    FullScreen,
}

/// Interactive click targets on the bottom `ScreenshotHud` Liquid Glass pill (`RUSTYOS-133`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotHudHit {
    /// Switch to `Region` selection mode.
    ModeRegion,
    /// Switch to `Window` selection mode.
    ModeWindow,
    /// Switch to `Full Screen` mode.
    ModeFullScreen,
    /// Toggle between `Save` (file + clipboard) and `Copy` (clipboard only).
    ToggleCopyOnly,
    /// Execute capture for the current mode/selection.
    Capture,
    /// Dismiss the Screenshot HUD without capturing.
    Cancel,
}

/// Live compositor state while the interactive `ScreenshotHud` is active (`RUSTYOS-133`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotHudState {
    /// Active capture mode.
    pub mode: ScreenshotMode,
    /// When `true`, copy PNG to clipboard only (fallback to file if `wl-copy` unavailable).
    pub copy_only: bool,
    /// Pointer drag start `(x, y)` in `Region` mode.
    pub drag_start: Option<(i32, i32)>,
    /// Current pointer `(x, y)` while dragging in `Region` mode.
    pub drag_current: Option<(i32, i32)>,
    /// Finalized selection rectangle in `Region` mode.
    pub selection: Option<Rect>,
    /// Currently hovered window `(id, geometry)` in `Window` mode.
    pub hovered_window: Option<(rusty_wm::WindowId, Rect)>,
    /// Currently hovered HUD pill button, if any.
    pub hover_hit: Option<ScreenshotHudHit>,
}

impl Default for ScreenshotHudState {
    fn default() -> Self {
        Self {
            mode: ScreenshotMode::Region,
            copy_only: false,
            drag_start: None,
            drag_current: None,
            selection: None,
            hovered_window: None,
            hover_hit: None,
        }
    }
}

impl ScreenshotHudState {
    /// Returns the active region rectangle (either in-progress drag or finalized selection).
    #[must_use]
    pub fn active_region_rect(&self) -> Option<Rect> {
        if let (Some(start), Some(cur)) = (self.drag_start, self.drag_current) {
            normalize_selection_rect(start, cur)
        } else {
            self.selection
        }
    }
}

/// Normalizes two corner points into a positive-size `Rect` (minimum `4x4` px).
#[must_use]
pub fn normalize_selection_rect(start: (i32, i32), end: (i32, i32)) -> Option<Rect> {
    let x0 = start.0.min(end.0);
    let y0 = start.1.min(end.1);
    let x1 = start.0.max(end.0);
    let y1 = start.1.max(end.1);
    let w = x1 - x0;
    let h = y1 - y0;
    if w < 4 || h < 4 {
        return None;
    }
    Some(Rect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    })
}

/// Computes the centered bottom Liquid Glass pill rectangle for `ScreenshotHud`.
#[must_use]
pub fn screenshot_hud_pill_rect(
    screen_size: Size<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    let w = screen_size.w.max(360);
    let h = screen_size.h.max(240);
    let pw = 540.min(w - 32).max(340);
    let ph = 50;
    let px = (w - pw) / 2;
    let py = (h - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - ph - 18).max(48);
    Rectangle::new(Point::from((px, py)), Size::from((pw, ph)))
}

struct HudButtonRects {
    region: Rectangle<i32, smithay::utils::Physical>,
    window: Rectangle<i32, smithay::utils::Physical>,
    fullscreen: Rectangle<i32, smithay::utils::Physical>,
    copy_toggle: Rectangle<i32, smithay::utils::Physical>,
    capture: Rectangle<i32, smithay::utils::Physical>,
    cancel: Rectangle<i32, smithay::utils::Physical>,
}

fn hud_button_rects(screen_size: Size<i32, smithay::utils::Physical>) -> HudButtonRects {
    let pill = screenshot_hud_pill_rect(screen_size);
    let bh = 32;
    let by = pill.loc.y + (pill.size.h - bh) / 2;
    let gap = 6;
    let mut x = pill.loc.x + 10;

    let region = Rectangle::new(Point::from((x, by)), Size::from((72, bh)));
    x += 72 + gap;
    let window = Rectangle::new(Point::from((x, by)), Size::from((74, bh)));
    x += 74 + gap;
    let fullscreen = Rectangle::new(Point::from((x, by)), Size::from((92, bh)));
    x += 92 + 14;
    let copy_toggle = Rectangle::new(Point::from((x, by)), Size::from((72, bh)));
    x += 72 + 12;
    let capture = Rectangle::new(Point::from((x, by)), Size::from((84, bh)));
    x += 84 + gap;
    let cancel_w = (pill.loc.x + pill.size.w - 10 - x).max(56);
    let cancel = Rectangle::new(Point::from((x, by)), Size::from((cancel_w, bh)));

    HudButtonRects {
        region,
        window,
        fullscreen,
        copy_toggle,
        capture,
        cancel,
    }
}

/// Hit-tests `(x, y)` against the interactive `ScreenshotHud` pill buttons.
#[must_use]
pub fn hit_test_screenshot_hud(
    screen_size: Size<i32, smithay::utils::Physical>,
    point: Point<i32, smithay::utils::Physical>,
) -> Option<ScreenshotHudHit> {
    let r = hud_button_rects(screen_size);
    let px = f64::from(point.x);
    let py = f64::from(point.y);
    if rect_contains(r.region, px, py) {
        Some(ScreenshotHudHit::ModeRegion)
    } else if rect_contains(r.window, px, py) {
        Some(ScreenshotHudHit::ModeWindow)
    } else if rect_contains(r.fullscreen, px, py) {
        Some(ScreenshotHudHit::ModeFullScreen)
    } else if rect_contains(r.copy_toggle, px, py) {
        Some(ScreenshotHudHit::ToggleCopyOnly)
    } else if rect_contains(r.capture, px, py) {
        Some(ScreenshotHudHit::Capture)
    } else if rect_contains(r.cancel, px, py) {
        Some(ScreenshotHudHit::Cancel)
    } else {
        None
    }
}

pub(crate) fn render_screenshot_hud_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    hud: &ScreenshotHudState,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
    let r = hud_button_rects(screen_size);
    let active_fg = [255, 255, 255, 255];
    let items: [(&str, Rectangle<i32, smithay::utils::Physical>, bool); 6] = [
        ("Region", r.region, hud.mode == ScreenshotMode::Region),
        ("Window", r.window, hud.mode == ScreenshotMode::Window),
        (
            "Full Screen",
            r.fullscreen,
            hud.mode == ScreenshotMode::FullScreen,
        ),
        (
            if hud.copy_only { "Copy" } else { "Save" },
            r.copy_toggle,
            hud.copy_only,
        ),
        ("Capture", r.capture, true),
        ("Cancel", r.cancel, false),
    ];
    for (label, rect, bold) in items {
        let fg = if bold {
            active_fg
        } else {
            pal.popover_text_primary
        };
        let weight = if bold {
            crate::text::FontWeight::Bold
        } else {
            crate::text::FontWeight::Regular
        };
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            label,
            weight,
            11.5,
            fg,
            phys_to_logical_rect(rect),
        ) {
            elements.push(el);
        }
    }

    if hud.mode == ScreenshotMode::Region
        && let Some(sel) = hud.active_region_rect()
    {
        let badge = format!("{} × {}", sel.width, sel.height);
        let bx = (sel.x + (sel.width - 88) / 2).clamp(8, (screen_size.w - 96).max(8));
        let by = if sel.y >= 34 { sel.y - 26 } else { sel.y + 8 };
        let badge_rect = Rectangle::new(Point::from((bx, by)), Size::from((88, 22)));
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            &badge,
            crate::text::FontWeight::Bold,
            11.0,
            active_fg,
            phys_to_logical_rect(badge_rect),
        ) {
            elements.push(el);
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn render_screenshot_hud_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    hud: &ScreenshotHudState,
) {
    let r = hud_button_rects(screen_size);
    let accent = [10, 132, 255, 238];
    let hover_bg = [120, 130, 148, 88];
    let mode_rects = [
        (
            r.region,
            hud.mode == ScreenshotMode::Region,
            ScreenshotHudHit::ModeRegion,
        ),
        (
            r.window,
            hud.mode == ScreenshotMode::Window,
            ScreenshotHudHit::ModeWindow,
        ),
        (
            r.fullscreen,
            hud.mode == ScreenshotMode::FullScreen,
            ScreenshotHudHit::ModeFullScreen,
        ),
        (
            r.copy_toggle,
            hud.copy_only,
            ScreenshotHudHit::ToggleCopyOnly,
        ),
        (r.capture, true, ScreenshotHudHit::Capture),
        (r.cancel, false, ScreenshotHudHit::Cancel),
    ];
    for (rect, active, hit) in mode_rects {
        let fill = if active {
            Some(accent)
        } else if hud.hover_hit == Some(hit) {
            Some(hover_bg)
        } else {
            None
        };
        if let Some(bg) = fill {
            push_rounded_glass_box(out, shape_cache, renderer, rect, 9, bg, None);
        }
    }

    let pill = screenshot_hud_pill_rect(screen_size);
    push_liquid_glass_panel_elevation(
        out,
        shape_cache,
        renderer,
        pill,
        16,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        rusty_ui::shadow::ShadowElevation::Popup,
    );

    let width = screen_size.w.max(1);
    let height = screen_size.h.max(1);
    let scrim = premul(0.04, 0.05, 0.07, 0.42);
    match hud.mode {
        ScreenshotMode::Region => {
            if let Some(sel) = hud.active_region_rect() {
                let bx = (sel.x + (sel.width - 88) / 2).clamp(8, (width - 96).max(8));
                let by = if sel.y >= 34 { sel.y - 26 } else { sel.y + 8 };
                push_rounded_glass_box(
                    out,
                    shape_cache,
                    renderer,
                    Rectangle::new(Point::from((bx, by)), Size::from((88, 22))),
                    7,
                    [20, 22, 28, 220],
                    Some([255, 255, 255, 80]),
                );
                let border = [255, 255, 255, 235];
                let sel_rect = Rectangle::new(
                    Point::from((sel.x, sel.y)),
                    Size::from((sel.width, sel.height)),
                );
                push_rounded_glass_box(
                    out,
                    shape_cache,
                    renderer,
                    sel_rect,
                    2,
                    [0, 0, 0, 0],
                    Some(border),
                );
                let sx0 = sel.x.clamp(0, width);
                let sy0 = sel.y.clamp(0, height);
                let sx1 = (sel.x + sel.width).clamp(sx0, width);
                let sy1 = (sel.y + sel.height).clamp(sy0, height);
                if sy0 > 0 {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(Point::from((0, 0)), Size::from((width, sy0))),
                        scrim,
                    )));
                }
                if sy1 < height {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(Point::from((0, sy1)), Size::from((width, height - sy1))),
                        scrim,
                    )));
                }
                if sx0 > 0 {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(Point::from((0, sy0)), Size::from((sx0, sy1 - sy0))),
                        scrim,
                    )));
                }
                if sx1 < width {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(
                            Point::from((sx1, sy0)),
                            Size::from((width - sx1, sy1 - sy0)),
                        ),
                        scrim,
                    )));
                }
            } else {
                out.push(CompositorRenderElement::Solid(make_solid_element(
                    Rectangle::new(Point::from((0, 0)), Size::from((width, height))),
                    premul(0.04, 0.05, 0.07, 0.18),
                )));
            }
        }
        ScreenshotMode::Window => {
            if let Some((_, win_rect)) = hud.hovered_window {
                push_rounded_glass_box(
                    out,
                    shape_cache,
                    renderer,
                    Rectangle::new(
                        Point::from((win_rect.x, win_rect.y)),
                        Size::from((win_rect.width.max(1), win_rect.height.max(1))),
                    ),
                    14,
                    [10, 132, 255, 42],
                    Some([64, 156, 255, 240]),
                );
            }
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((0, 0)), Size::from((width, height))),
                premul(0.04, 0.05, 0.07, 0.16),
            )));
        }
        ScreenshotMode::FullScreen => {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((0, 0)), Size::from((width, height))),
                premul(0.04, 0.05, 0.07, 0.12),
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_hud_hit_test_and_selection_normalization() {
        let screen = Size::from((1280, 800));
        let r = hud_button_rects(screen);
        let center = |rect: Rectangle<i32, smithay::utils::Physical>| {
            Point::from((rect.loc.x + rect.size.w / 2, rect.loc.y + rect.size.h / 2))
        };
        assert_eq!(
            hit_test_screenshot_hud(screen, center(r.region)),
            Some(ScreenshotHudHit::ModeRegion)
        );
        assert_eq!(
            hit_test_screenshot_hud(screen, center(r.window)),
            Some(ScreenshotHudHit::ModeWindow)
        );
        assert_eq!(
            hit_test_screenshot_hud(screen, center(r.fullscreen)),
            Some(ScreenshotHudHit::ModeFullScreen)
        );
        assert_eq!(
            hit_test_screenshot_hud(screen, center(r.copy_toggle)),
            Some(ScreenshotHudHit::ToggleCopyOnly)
        );
        assert_eq!(
            hit_test_screenshot_hud(screen, center(r.capture)),
            Some(ScreenshotHudHit::Capture)
        );
        assert_eq!(
            hit_test_screenshot_hud(screen, center(r.cancel)),
            Some(ScreenshotHudHit::Cancel)
        );

        assert_eq!(normalize_selection_rect((100, 100), (102, 102)), None);
        assert_eq!(
            normalize_selection_rect((320, 240), (120, 80)),
            Some(Rect {
                x: 120,
                y: 80,
                width: 200,
                height: 160,
            })
        );
    }
}
