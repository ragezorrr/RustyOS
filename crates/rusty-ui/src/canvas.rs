//! High-performance subpixel 2D canvas drawing primitives.

use crate::geometry::Rect;
use crate::theme::TrafficLightKind;
use crate::traffic_lights::{TrafficLightButtons, glyph_segments};

/// Pixel color format for the destination memory buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PixelFormat {
    /// Blue-Green-Red-Alpha (standard Wayland `wl_shm` ARGB8888 little-endian).
    #[default]
    Bgra8888,
    /// Red-Green-Blue-Alpha (standard GPU EGL / image format).
    Rgba8888,
}

/// Lightweight 2D canvas view over a borrowed byte slice.
pub struct UiCanvas<'a> {
    pub buffer: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: PixelFormat,
}

impl std::fmt::Debug for UiCanvas<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiCanvas")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .field("format", &self.format)
            .field("buffer_len", &self.buffer.len())
            .finish()
    }
}

impl<'a> UiCanvas<'a> {
    /// Creates a new canvas view over raw pixel memory.
    #[must_use]
    pub fn new(
        buffer: &'a mut [u8],
        width: u32,
        height: u32,
        stride: usize,
        format: PixelFormat,
    ) -> Self {
        Self {
            buffer,
            width,
            height,
            stride,
            format,
        }
    }

    /// Fills the entire buffer with a solid color.
    pub fn clear(&mut self, color: [u8; 4]) {
        self.fill_rect(Rect::new(0, 0, self.width, self.height), color);
    }

    /// Alpha-blends a single pixel `(x, y)` with clamping.
    #[inline]
    pub fn blend_pixel(&mut self, x: i32, y: i32, [r, g, b, a]: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || a == 0 {
            return;
        }

        let idx = y as usize * self.stride + x as usize * 4;
        if idx + 4 > self.buffer.len() {
            return;
        }

        if a == 255 {
            match self.format {
                PixelFormat::Bgra8888 => {
                    self.buffer[idx] = b;
                    self.buffer[idx + 1] = g;
                    self.buffer[idx + 2] = r;
                    self.buffer[idx + 3] = 255;
                }
                PixelFormat::Rgba8888 => {
                    self.buffer[idx] = r;
                    self.buffer[idx + 1] = g;
                    self.buffer[idx + 2] = b;
                    self.buffer[idx + 3] = 255;
                }
            }
            return;
        }

        let alpha = a as u32;
        let inv = 255 - alpha;

        let (dst_r, dst_g, dst_b) = match self.format {
            PixelFormat::Bgra8888 => (
                self.buffer[idx + 2] as u32,
                self.buffer[idx + 1] as u32,
                self.buffer[idx] as u32,
            ),
            PixelFormat::Rgba8888 => (
                self.buffer[idx] as u32,
                self.buffer[idx + 1] as u32,
                self.buffer[idx + 2] as u32,
            ),
        };

        let out_r = ((r as u32 * alpha + dst_r * inv) / 255) as u8;
        let out_g = ((g as u32 * alpha + dst_g * inv) / 255) as u8;
        let out_b = ((b as u32 * alpha + dst_b * inv) / 255) as u8;

        match self.format {
            PixelFormat::Bgra8888 => {
                self.buffer[idx] = out_b;
                self.buffer[idx + 1] = out_g;
                self.buffer[idx + 2] = out_r;
                self.buffer[idx + 3] = 255;
            }
            PixelFormat::Rgba8888 => {
                self.buffer[idx] = out_r;
                self.buffer[idx + 1] = out_g;
                self.buffer[idx + 2] = out_b;
                self.buffer[idx + 3] = 255;
            }
        }
    }

    /// Fills an axis-aligned rectangle.
    pub fn fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
        let x0 = rect.x.clamp(0, self.width as i32) as u32;
        let x1 = rect.right().clamp(0, self.width as i32) as u32;
        let y0 = rect.y.clamp(0, self.height as i32) as u32;
        let y1 = rect.bottom().clamp(0, self.height as i32) as u32;

        if x0 >= x1 || y0 >= y1 {
            return;
        }

        if color[3] == 255 {
            let [r, g, b, _] = color;
            let (b0, b1, b2) = match self.format {
                PixelFormat::Bgra8888 => (b, g, r),
                PixelFormat::Rgba8888 => (r, g, b),
            };

            for y in y0..y1 {
                let row_offset = y as usize * self.stride;
                for x in x0..x1 {
                    let idx = row_offset + x as usize * 4;
                    self.buffer[idx] = b0;
                    self.buffer[idx + 1] = b1;
                    self.buffer[idx + 2] = b2;
                    self.buffer[idx + 3] = 255;
                }
            }
        } else {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.blend_pixel(x as i32, y as i32, color);
                }
            }
        }
    }

    /// Draws an anti-aliased circle with smooth subpixel boundary.
    pub fn draw_circle_aa(&mut self, cx: f32, cy: f32, radius: f32, [r, g, b, a]: [u8; 4]) {
        let min_x = (cx - radius - 1.0).floor() as i32;
        let max_x = (cx + radius + 1.0).ceil() as i32;
        let min_y = (cy - radius - 1.0).floor() as i32;
        let max_y = (cy + radius + 1.0).ceil() as i32;

        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let dx = px as f32 + 0.5 - cx;
                let dy = py as f32 + 0.5 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                let delta = dist - radius;

                if delta <= -0.5 {
                    self.blend_pixel(px, py, [r, g, b, a]);
                } else if delta < 0.5 {
                    let cov = 0.5 - delta;
                    let alpha = ((a as f32) * cov.clamp(0.0, 1.0)).round() as u8;
                    if alpha > 0 {
                        self.blend_pixel(px, py, [r, g, b, alpha]);
                    }
                }
            }
        }
    }

    /// Strokes an anti-aliased line segment with subpixel accuracy.
    pub fn stroke_line_aa(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        width: f32,
        [r, g, b, a]: [u8; 4],
    ) {
        let half_w = width * 0.5;
        let min_x = (x0.min(x1) - half_w - 1.0).floor() as i32;
        let max_x = (x0.max(x1) + half_w + 1.0).ceil() as i32;
        let min_y = (y0.min(y1) - half_w - 1.0).floor() as i32;
        let max_y = (y0.max(y1) + half_w + 1.0).ceil() as i32;

        let dx = x1 - x0;
        let dy = y1 - y0;
        let len_sq = dx * dx + dy * dy;

        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = if len_sq <= 1e-4 {
                    ((px_f - x0).powi(2) + (py_f - y0).powi(2)).sqrt()
                } else {
                    let t = (((px_f - x0) * dx + (py_f - y0) * dy) / len_sq).clamp(0.0, 1.0);
                    let proj_x = x0 + t * dx;
                    let proj_y = y0 + t * dy;
                    ((px_f - proj_x).powi(2) + (py_f - proj_y).powi(2)).sqrt()
                };

                let delta = dist - half_w;
                if delta <= -0.5 {
                    self.blend_pixel(px, py, [r, g, b, a]);
                } else if delta < 0.5 {
                    let cov = 0.5 - delta;
                    let alpha = ((a as f32) * cov.clamp(0.0, 1.0)).round() as u8;
                    if alpha > 0 {
                        self.blend_pixel(px, py, [r, g, b, alpha]);
                    }
                }
            }
        }
    }

    /// Renders a fast rounded rectangle with anti-aliased corners.
    pub fn fast_rounded_rect(&mut self, rect: Rect, radius: f32, color: [u8; 4]) {
        let r = radius
            .min(rect.width as f32 * 0.5)
            .min(rect.height as f32 * 0.5);
        if r <= 0.5 {
            self.fill_rect(rect, color);
            return;
        }

        let inner_y0 = rect.y + r.ceil() as i32;
        let inner_y1 = rect.bottom() - r.ceil() as i32;
        if inner_y1 > inner_y0 {
            self.fill_rect(
                Rect::new(rect.x, inner_y0, rect.width, (inner_y1 - inner_y0) as u32),
                color,
            );
        }

        let inner_x0 = rect.x + r.ceil() as i32;
        let inner_x1 = rect.right() - r.ceil() as i32;
        let corner_w = (inner_x1 - inner_x0).max(0) as u32;

        if inner_y0 > rect.y && corner_w > 0 {
            self.fill_rect(
                Rect::new(inner_x0, rect.y, corner_w, (inner_y0 - rect.y) as u32),
                color,
            );
        }
        if rect.bottom() > inner_y1 && corner_w > 0 {
            self.fill_rect(
                Rect::new(
                    inner_x0,
                    inner_y1,
                    corner_w,
                    (rect.bottom() - inner_y1) as u32,
                ),
                color,
            );
        }

        let centers = [
            (rect.x as f32 + r, rect.y as f32 + r),
            (rect.right() as f32 - r, rect.y as f32 + r),
            (rect.x as f32 + r, rect.bottom() as f32 - r),
            (rect.right() as f32 - r, rect.bottom() as f32 - r),
        ];

        let bounds = [
            (
                rect.x,
                rect.x + r.ceil() as i32,
                rect.y,
                rect.y + r.ceil() as i32,
            ),
            (
                rect.right() - r.ceil() as i32,
                rect.right(),
                rect.y,
                rect.y + r.ceil() as i32,
            ),
            (
                rect.x,
                rect.x + r.ceil() as i32,
                rect.bottom() - r.ceil() as i32,
                rect.bottom(),
            ),
            (
                rect.right() - r.ceil() as i32,
                rect.right(),
                rect.bottom() - r.ceil() as i32,
                rect.bottom(),
            ),
        ];

        for i in 0..4 {
            let (cx, cy) = centers[i];
            let (bx0, bx1, by0, by1) = bounds[i];
            for py in by0..by1 {
                for px in bx0..bx1 {
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let delta = dist - r;
                    if delta <= -0.5 {
                        self.blend_pixel(px, py, color);
                    } else if delta < 0.5 {
                        let cov = 0.5 - delta;
                        let alpha = ((color[3] as f32) * cov.clamp(0.0, 1.0)).round() as u8;
                        if alpha > 0 {
                            self.blend_pixel(px, py, [color[0], color[1], color[2], alpha]);
                        }
                    }
                }
            }
        }
    }

    /// Alias for `fast_rounded_rect`.
    #[inline]
    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: u32, color: [u8; 4]) {
        self.fast_rounded_rect(rect, radius as f32, color);
    }

    /// Strokes a 1px border around a rounded rectangle.
    pub fn stroke_rounded_rect(&mut self, rect: Rect, radius: u32, color: [u8; 4]) {
        let r = (radius as f32)
            .min(rect.width as f32 / 2.0)
            .min(rect.height as f32 / 2.0);
        let rx0 = rect.x as f32 + r;
        let rx1 = rect.right() as f32 - r;
        let ry0 = rect.y as f32 + r;
        let ry1 = rect.bottom() as f32 - r;

        // Top and bottom straight lines
        self.stroke_line_aa(
            rx0,
            rect.y as f32 + 0.5,
            rx1,
            rect.y as f32 + 0.5,
            1.0,
            color,
        );
        self.stroke_line_aa(
            rx0,
            rect.bottom() as f32 - 0.5,
            rx1,
            rect.bottom() as f32 - 0.5,
            1.0,
            color,
        );
        // Left and right straight lines
        self.stroke_line_aa(
            rect.x as f32 + 0.5,
            ry0,
            rect.x as f32 + 0.5,
            ry1,
            1.0,
            color,
        );
        self.stroke_line_aa(
            rect.right() as f32 - 0.5,
            ry0,
            rect.right() as f32 - 0.5,
            ry1,
            1.0,
            color,
        );
    }

    /// Renders standard traffic lights with active/inactive colors, pressed state, and hover vector microglyphs.
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
            self.draw_circle_aa(cx, cy, 7.0, color);

            if hovered {
                let ink = TrafficLightKind::glyph_ink();
                for seg in glyph_segments(kind) {
                    self.stroke_line_aa(
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
}
