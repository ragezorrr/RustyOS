//! Declarative 2D UI draw-list, painter API, and exact CPU rasterizer (`RUSTYOS-130`).

use std::sync::Arc;

use crate::canvas::{PixelFormat, UiCanvas};
use crate::geometry::Rect;
use crate::shadow::{ShadowElevation, draw_elevation_shadow_with};
use crate::text::UiFontEngine;
use crate::theme::TrafficLightKind;
use crate::traffic_lights::{TrafficLightButtons, glyph_segments};

/// Rasterization command recorded into a [`UiDrawList`] for GPU or CPU execution.
#[derive(Debug, Clone, PartialEq)]
pub enum UiDrawCmd {
    /// Axis-aligned solid fill rectangle.
    Rect { rect: Rect, color: [u8; 4] },
    /// Rounded rectangle or continuous cubic squircle with optional stroke border.
    RoundedRect {
        rect: Rect,
        radius: f32,
        fill: [u8; 4],
        border_width: f32,
        border_color: [u8; 4],
        squircle: bool,
    },
    /// Anti-aliased circle with optional outline stroke.
    Circle {
        cx: f32,
        cy: f32,
        radius: f32,
        fill: [u8; 4],
        stroke_width: f32,
        stroke_color: [u8; 4],
    },
    /// Anti-aliased line segment.
    Line {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        width: f32,
        color: [u8; 4],
    },
    /// 3-lobe analytical drop shadow resolved from [`ShadowElevation`].
    Shadow {
        rect: Rect,
        radius: i32,
        elevation: ShadowElevation,
        is_dark: bool,
    },
    /// UTF-8 text run positioned by `(x, y_baseline)`.
    Text {
        text: String,
        x: f32,
        y_baseline: f32,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
    },
    /// Cached premultiplied RGBA8888 or BGRA8888 pixel image quad.
    Image {
        rect: Rect,
        cache_key: u64,
        width: u32,
        height: u32,
        pixels: Arc<[u8]>,
        format: PixelFormat,
    },
    /// Pushes an intersecting scissor/clipping rectangle onto the clip stack.
    PushClip(Rect),
    /// Restores the previous clipping rectangle.
    PopClip,
}

/// Ordered sequence of draw commands emitted for one UI frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiDrawList {
    pub commands: Vec<UiDrawCmd>,
}

impl UiDrawList {
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: Vec::with_capacity(128),
        }
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Immediate-mode 2D painter recording commands into a [`UiDrawList`].
#[derive(Debug)]
pub struct UiPainter<'a> {
    list: &'a mut UiDrawList,
}

impl<'a> UiPainter<'a> {
    #[must_use]
    pub fn new(list: &'a mut UiDrawList) -> Self {
        Self { list }
    }

    #[must_use]
    pub fn draw_list(&self) -> &UiDrawList {
        self.list
    }

    pub fn fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
        if rect.width > 0 && rect.height > 0 && color[3] > 0 {
            self.list.commands.push(UiDrawCmd::Rect { rect, color });
        }
    }

    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, color: [u8; 4]) {
        self.rounded_rect_styled(rect, radius, color, 0.0, [0, 0, 0, 0], false);
    }

    pub fn fill_squircle(&mut self, rect: Rect, radius: f32, color: [u8; 4]) {
        self.rounded_rect_styled(rect, radius, color, 0.0, [0, 0, 0, 0], true);
    }

    pub fn stroke_rounded_rect(
        &mut self,
        rect: Rect,
        radius: f32,
        border_width: f32,
        border_color: [u8; 4],
    ) {
        self.rounded_rect_styled(
            rect,
            radius,
            [0, 0, 0, 0],
            border_width,
            border_color,
            false,
        );
    }

    pub fn rounded_rect_styled(
        &mut self,
        rect: Rect,
        radius: f32,
        fill: [u8; 4],
        border_width: f32,
        border_color: [u8; 4],
        squircle: bool,
    ) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        if fill[3] == 0 && (border_width <= 0.0 || border_color[3] == 0) {
            return;
        }
        self.list.commands.push(UiDrawCmd::RoundedRect {
            rect,
            radius,
            fill,
            border_width,
            border_color,
            squircle,
        });
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, fill: [u8; 4]) {
        if radius > 0.0 && fill[3] > 0 {
            self.list.commands.push(UiDrawCmd::Circle {
                cx,
                cy,
                radius,
                fill,
                stroke_width: 0.0,
                stroke_color: [0, 0, 0, 0],
            });
        }
    }

    pub fn stroke_circle(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        stroke_width: f32,
        stroke_color: [u8; 4],
    ) {
        if radius > 0.0 && stroke_width > 0.0 && stroke_color[3] > 0 {
            self.list.commands.push(UiDrawCmd::Circle {
                cx,
                cy,
                radius,
                fill: [0, 0, 0, 0],
                stroke_width,
                stroke_color,
            });
        }
    }

    pub fn draw_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: [u8; 4]) {
        if width > 0.0 && color[3] > 0 {
            self.list.commands.push(UiDrawCmd::Line {
                x0,
                y0,
                x1,
                y1,
                width,
                color,
            });
        }
    }

    pub fn draw_shadow(
        &mut self,
        rect: Rect,
        radius: i32,
        elevation: ShadowElevation,
        is_dark: bool,
    ) {
        if elevation != ShadowElevation::None && rect.width > 0 && rect.height > 0 {
            self.list.commands.push(UiDrawCmd::Shadow {
                rect,
                radius,
                elevation,
                is_dark,
            });
        }
    }

    pub fn draw_text(
        &mut self,
        text: impl Into<String>,
        x: f32,
        y_baseline: f32,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
    ) {
        let s = text.into();
        if !s.is_empty() && color[3] > 0 {
            self.list.commands.push(UiDrawCmd::Text {
                text: s,
                x,
                y_baseline,
                size_px,
                bold,
                color,
            });
        }
    }

    pub fn draw_text_centered(
        &mut self,
        fonts: &mut UiFontEngine,
        text: impl Into<String>,
        rect: Rect,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
    ) {
        let s = text.into();
        if s.is_empty() || color[3] == 0 {
            return;
        }
        let tw = fonts.measure_width(&s, size_px, bold);
        let tx = rect.x as f32 + (rect.width as f32 - tw) * 0.5;
        let baseline = rect.y as f32 + f32::midpoint(rect.height as f32, size_px * 0.72);
        self.draw_text(s, tx, baseline, size_px, bold, color);
    }

    /// Emits a RustyOS focus ring around `rect` modulated by `alpha` (`0.0..=1.0`).
    pub fn draw_focus_ring(&mut self, rect: Rect, radius: f32, accent: [u8; 4], alpha: f32) {
        let clamped = alpha.clamp(0.0, 1.0);
        if clamped <= 0.01 {
            return;
        }
        let ring_a = (195.0 * clamped).round() as u8;
        let ring_col = [accent[0], accent[1], accent[2], ring_a];
        let outer = Rect::new(rect.x - 2, rect.y - 2, rect.width + 4, rect.height + 4);
        self.stroke_rounded_rect(outer, radius + 2.0, 2.0, ring_col);
    }

    /// Emits vector traffic lights matching [`UiCanvas::draw_traffic_lights`].
    pub fn draw_traffic_lights(
        &mut self,
        buttons: &TrafficLightButtons,
        focused: bool,
        hovered: bool,
        pressed: Option<TrafficLightKind>,
    ) {
        for kind in [
            TrafficLightKind::Close,
            TrafficLightKind::Minimize,
            TrafficLightKind::Maximize,
        ] {
            let color = if !focused {
                TrafficLightKind::inactive_color()
            } else if pressed == Some(kind) {
                kind.pressed_color()
            } else {
                kind.active_color()
            };
            let (cx, cy) = buttons.center_of(kind);
            self.fill_circle(cx, cy, 7.0, color);
            if hovered {
                let ink = TrafficLightKind::glyph_ink();
                for seg in glyph_segments(kind) {
                    self.draw_line(
                        cx + seg.dx0,
                        cy + seg.dy0,
                        cx + seg.dx1,
                        cy + seg.dy1,
                        1.0,
                        ink,
                    );
                }
            }
        }
    }

    pub fn draw_image(
        &mut self,
        rect: Rect,
        cache_key: u64,
        width: u32,
        height: u32,
        pixels: Arc<[u8]>,
        format: PixelFormat,
    ) {
        if rect.width > 0 && rect.height > 0 && width > 0 && height > 0 && !pixels.is_empty() {
            self.list.commands.push(UiDrawCmd::Image {
                rect,
                cache_key,
                width,
                height,
                pixels,
                format,
            });
        }
    }

    pub fn push_clip(&mut self, rect: Rect) {
        self.list.commands.push(UiDrawCmd::PushClip(rect));
    }

    pub fn pop_clip(&mut self) {
        self.list.commands.push(UiDrawCmd::PopClip);
    }
}

/// Computes the intersection of two rectangles, clamping empty overlaps cleanly to `0x0`.
#[must_use]
pub fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    if x1 <= x0 || y1 <= y0 {
        Rect::new(x0, y0, 0, 0)
    } else {
        Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }
}

/// Executes a recorded [`UiDrawList`] onto a CPU [`UiCanvas`], honoring clip stack and elevation shadows.
pub fn render_draw_list_to_canvas(
    list: &UiDrawList,
    canvas: &mut UiCanvas<'_>,
    fonts: &mut UiFontEngine,
) {
    let full_clip = Rect::new(0, 0, canvas.width, canvas.height);
    let mut clip_stack = vec![full_clip];

    for cmd in &list.commands {
        let clip = *clip_stack.last().unwrap_or(&full_clip);
        match cmd {
            UiDrawCmd::PushClip(next) => {
                clip_stack.push(intersect_rect(clip, *next));
            }
            UiDrawCmd::PopClip => {
                if clip_stack.len() > 1 {
                    clip_stack.pop();
                }
            }
            other => exec_draw_cmd_on_canvas(other, canvas, fonts, clip),
        }
    }
}

fn exec_draw_cmd_on_canvas(
    cmd: &UiDrawCmd,
    canvas: &mut UiCanvas<'_>,
    fonts: &mut UiFontEngine,
    clip: Rect,
) {
    match cmd {
        UiDrawCmd::PushClip(_) | UiDrawCmd::PopClip => {}
        UiDrawCmd::Rect { rect, color } => canvas.fill_rect(intersect_rect(*rect, clip), *color),
        UiDrawCmd::RoundedRect {
            rect,
            radius,
            fill,
            border_width,
            border_color,
            squircle,
        } => rasterize_rounded_rect_clipped(
            canvas,
            *rect,
            *radius,
            *fill,
            *border_width,
            *border_color,
            *squircle,
            clip,
        ),
        UiDrawCmd::Circle {
            cx,
            cy,
            radius,
            fill,
            stroke_width,
            stroke_color,
        } => rasterize_circle_clipped(
            canvas,
            *cx,
            *cy,
            *radius,
            *fill,
            *stroke_width,
            *stroke_color,
            clip,
        ),
        UiDrawCmd::Line {
            x0,
            y0,
            x1,
            y1,
            width,
            color,
        } => rasterize_line_clipped(canvas, *x0, *y0, *x1, *y1, *width, *color, clip),
        UiDrawCmd::Shadow {
            rect,
            radius,
            elevation,
            is_dark,
        } if *elevation != ShadowElevation::None => draw_elevation_shadow_with(
            *rect,
            (*radius).max(0) as u32,
            *elevation,
            *is_dark,
            |r, rad, col| {
                rasterize_rounded_rect_clipped(
                    canvas,
                    r,
                    rad.max(0) as f32,
                    col,
                    0.0,
                    [0, 0, 0, 0],
                    false,
                    clip,
                );
            },
        ),
        UiDrawCmd::Shadow { .. } => {}
        UiDrawCmd::Text {
            text,
            x,
            y_baseline,
            size_px,
            bold,
            color,
        } => fonts.draw_text_on_canvas(
            canvas,
            text,
            *x,
            *y_baseline,
            *size_px,
            *bold,
            *color,
            Some(clip),
        ),
        UiDrawCmd::Image {
            rect,
            width,
            height,
            pixels,
            format,
            ..
        } => blit_image_clipped(canvas, *rect, *width, *height, pixels, *format, clip),
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_rounded_rect_clipped(
    canvas: &mut UiCanvas<'_>,
    rect: Rect,
    radius: f32,
    fill: [u8; 4],
    border_width: f32,
    border_color: [u8; 4],
    squircle: bool,
    clip: Rect,
) {
    let bounds = intersect_rect(rect, clip);
    if bounds.width == 0 || bounds.height == 0 {
        return;
    }
    let hw = rect.width as f32 * 0.5;
    let hh = rect.height as f32 * 0.5;
    let min_half = hw.min(hh);
    let r = radius.clamp(0.0, min_half);
    let cx = rect.x as f32 + hw;
    let cy = rect.y as f32 + hh;
    let has_border = border_width > 0.0 && border_color[3] > 0;

    if !squircle && !has_border && bounds == rect {
        canvas.fast_rounded_rect(rect, r, fill);
        return;
    }

    for py in bounds.y..bounds.bottom() {
        for px in bounds.x..bounds.right() {
            let lx = (px as f32 + 0.5 - cx).abs();
            let ly = (py as f32 + 0.5 - cy).abs();
            let sd = if squircle {
                squircle_sdf_dist(lx, ly, hw, hh, r)
            } else {
                let qx = lx - (hw - r);
                let qy = ly - (hh - r);
                (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r
            };
            let cov = (0.5 - sd).clamp(0.0, 1.0);
            if cov <= 0.001 {
                continue;
            }
            let out_col = if has_border {
                let b_mix = (sd + border_width + 0.5).clamp(0.0, 1.0);
                if fill[3] == 0 {
                    [
                        border_color[0],
                        border_color[1],
                        border_color[2],
                        (f32::from(border_color[3]) * b_mix * cov).round() as u8,
                    ]
                } else {
                    let col = lerp_rgba(fill, border_color, b_mix);
                    [
                        col[0],
                        col[1],
                        col[2],
                        (f32::from(col[3]) * cov).round() as u8,
                    ]
                }
            } else {
                [
                    fill[0],
                    fill[1],
                    fill[2],
                    (f32::from(fill[3]) * cov).round() as u8,
                ]
            };
            if out_col[3] > 0 {
                canvas.blend_pixel(px, py, out_col);
            }
        }
    }
}

/// Evaluates unified cubic continuous squircle SDF matching compositor `SURFACE_CLIP_FRAG`.
#[inline]
#[must_use]
pub fn squircle_sdf_dist(lx: f32, ly: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let min_h = hw.min(hh);
    let rc = r.clamp(0.0, min_h);
    let is_capsule = (rc - min_h).abs() < 0.75;
    if is_capsule {
        let qx = lx - (hw - rc);
        let qy = ly - (hh - rc);
        (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - rc
    } else {
        let rext = (rc * 1.42).min(min_h);
        let qx = lx - (hw - rext);
        let qy = ly - (hh - rext);
        if qx > 0.0 && qy > 0.0 {
            let nx = qx / rext.max(1e-4);
            let ny = qy / rext.max(1e-4);
            ((nx * nx * nx + ny * ny * ny).cbrt() - 1.0) * rext
        } else {
            qx.max(qy) - rext
        }
    }
}

#[inline]
fn lerp_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let u = 1.0 - t;
    [
        (f32::from(a[0]) * u + f32::from(b[0]) * t).round() as u8,
        (f32::from(a[1]) * u + f32::from(b[1]) * t).round() as u8,
        (f32::from(a[2]) * u + f32::from(b[2]) * t).round() as u8,
        (f32::from(a[3]) * u + f32::from(b[3]) * t).round() as u8,
    ]
}

#[allow(clippy::too_many_arguments)]
fn rasterize_circle_clipped(
    canvas: &mut UiCanvas<'_>,
    cx: f32,
    cy: f32,
    radius: f32,
    fill: [u8; 4],
    stroke_width: f32,
    stroke_color: [u8; 4],
    clip: Rect,
) {
    let min_x = ((cx - radius - 1.0).floor() as i32).max(clip.x);
    let max_x = ((cx + radius + 1.0).ceil() as i32).min(clip.right() - 1);
    let min_y = ((cy - radius - 1.0).floor() as i32).max(clip.y);
    let max_y = ((cy + radius + 1.0).ceil() as i32).min(clip.bottom() - 1);
    let has_stroke = stroke_width > 0.0 && stroke_color[3] > 0;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let dist = ((px as f32 + 0.5 - cx).powi(2) + (py as f32 + 0.5 - cy).powi(2)).sqrt();
            let sd = dist - radius;
            let cov = (0.5 - sd).clamp(0.0, 1.0);
            if cov <= 0.001 {
                continue;
            }
            let col = if has_stroke {
                let s_mix = (sd + stroke_width + 0.5).clamp(0.0, 1.0);
                lerp_rgba(fill, stroke_color, s_mix)
            } else {
                fill
            };
            let a = (f32::from(col[3]) * cov).round() as u8;
            if a > 0 {
                canvas.blend_pixel(px, py, [col[0], col[1], col[2], a]);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_line_clipped(
    canvas: &mut UiCanvas<'_>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    width: f32,
    color: [u8; 4],
    clip: Rect,
) {
    let half_w = width * 0.5;
    let min_x = ((x0.min(x1) - half_w - 1.0).floor() as i32).max(clip.x);
    let max_x = ((x0.max(x1) + half_w + 1.0).ceil() as i32).min(clip.right() - 1);
    let min_y = ((y0.min(y1) - half_w - 1.0).floor() as i32).max(clip.y);
    let max_y = ((y0.max(y1) + half_w + 1.0).ceil() as i32).min(clip.bottom() - 1);
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let pxf = px as f32 + 0.5;
            let pyf = py as f32 + 0.5;
            let dist = if len_sq <= 1e-4 {
                ((pxf - x0).powi(2) + (pyf - y0).powi(2)).sqrt()
            } else {
                let t = (((pxf - x0) * dx + (pyf - y0) * dy) / len_sq).clamp(0.0, 1.0);
                ((pxf - (x0 + t * dx)).powi(2) + (pyf - (y0 + t * dy)).powi(2)).sqrt()
            };
            let cov = (0.5 - (dist - half_w)).clamp(0.0, 1.0);
            let a = (f32::from(color[3]) * cov).round() as u8;
            if a > 0 {
                canvas.blend_pixel(px, py, [color[0], color[1], color[2], a]);
            }
        }
    }
}

fn blit_image_clipped(
    canvas: &mut UiCanvas<'_>,
    dst: Rect,
    src_w: u32,
    src_h: u32,
    pixels: &[u8],
    format: PixelFormat,
    clip: Rect,
) {
    let bounds = intersect_rect(dst, clip);
    if bounds.width == 0 || bounds.height == 0 || src_w == 0 || src_h == 0 {
        return;
    }
    for py in bounds.y..bounds.bottom() {
        let sy = (((py - dst.y) as u64 * u64::from(src_h)) / u64::from(dst.height.max(1)))
            .min(u64::from(src_h - 1)) as usize;
        for px in bounds.x..bounds.right() {
            let sx = (((px - dst.x) as u64 * u64::from(src_w)) / u64::from(dst.width.max(1)))
                .min(u64::from(src_w - 1)) as usize;
            let s_idx = (sy * src_w as usize + sx) * 4;
            if s_idx + 4 > pixels.len() {
                continue;
            }
            let (r, g, b, a) = match format {
                PixelFormat::Bgra8888 => (
                    pixels[s_idx + 2],
                    pixels[s_idx + 1],
                    pixels[s_idx],
                    pixels[s_idx + 3],
                ),
                PixelFormat::Rgba8888 => (
                    pixels[s_idx],
                    pixels[s_idx + 1],
                    pixels[s_idx + 2],
                    pixels[s_idx + 3],
                ),
            };
            if a == 0 {
                continue;
            }
            // Convert premultiplied src pixel cleanly to straight blend_pixel input if translucent.
            if a == 255 {
                canvas.blend_pixel(px, py, [r, g, b, 255]);
            } else {
                let ur = ((u16::from(r) * 255) / u16::from(a)).min(255) as u8;
                let ug = ((u16::from(g) * 255) / u16::from(a)).min(255) as u8;
                let ub = ((u16::from(b) * 255) / u16::from(a)).min(255) as u8;
                canvas.blend_pixel(px, py, [ur, ug, ub, a]);
            }
        }
    }
}
