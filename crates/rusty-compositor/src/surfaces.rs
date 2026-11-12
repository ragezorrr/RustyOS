//! GPU-cached SDF shape primitives (`UiShapeCache`) for macOS-style rounded corners,
//! anti-aliased circles, and soft drop shadows (`RUSTYOS-022`).

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};

/// Corner quadrant identifier for 9-slice rounded boxes and shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CornerQuadrant {
    /// Upper-left corner.
    TopLeft,
    /// Upper-right corner.
    TopRight,
    /// Lower-left corner.
    BottomLeft,
    /// Lower-right corner.
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CircleKey {
    diameter: u16,
    fill_rgba: [u8; 4],
    rim_rgba: Option<[u8; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CornerKey {
    radius: u16,
    quadrant: CornerQuadrant,
    fill_rgba: [u8; 4],
    border_rgba: Option<[u8; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ShadowCornerKey {
    inner_radius: u16,
    spread: u16,
    quadrant: CornerQuadrant,
    shadow_rgb: [u8; 3],
    max_alpha: u8,
}

/// CPU-rasterized, GPU-cached cache of SDF rounded corners, circles, shadow patches, and wallpaper.
#[derive(Debug, Default)]
pub struct UiShapeCache {
    circles: HashMap<CircleKey, (MemoryRenderBuffer, Size<i32, Logical>)>,
    corners: HashMap<CornerKey, (MemoryRenderBuffer, Size<i32, Logical>)>,
    shadow_corners: HashMap<ShadowCornerKey, (MemoryRenderBuffer, Size<i32, Logical>)>,
    wallpaper: Option<MemoryRenderBuffer>,
    cursor_arrow: Option<MemoryRenderBuffer>,
    /// Decoded & GPU-cached application and tray icon textures (`RUSTYOS-033`).
    pub icons: crate::icons::IconCache,
}

impl UiShapeCache {
    /// Creates an empty [`UiShapeCache`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a cached anti-aliased circular render element positioned at `loc`.
    pub fn circle_element(
        &mut self,
        renderer: &mut GlesRenderer,
        diameter: i32,
        fill_rgba: [u8; 4],
        rim_rgba: Option<[u8; 4]>,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if diameter <= 0 {
            return None;
        }
        let d_u16 = u16::try_from(diameter).ok()?;
        let key = CircleKey {
            diameter: d_u16,
            fill_rgba,
            rim_rgba,
        };
        let (buf, _size) = self.circles.entry(key).or_insert_with(|| {
            let buf = rasterize_circle_buffer(diameter, fill_rgba, rim_rgba);
            (buf, Size::from((diameter, diameter)))
        });
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a cached `radius x radius` rounded corner patch with optional 1px outer rim.
    pub fn corner_element(
        &mut self,
        renderer: &mut GlesRenderer,
        radius: i32,
        quadrant: CornerQuadrant,
        fill_rgba: [u8; 4],
        border_rgba: Option<[u8; 4]>,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if radius <= 0 {
            return None;
        }
        let r_u16 = u16::try_from(radius).ok()?;
        let key = CornerKey {
            radius: r_u16,
            quadrant,
            fill_rgba,
            border_rgba,
        };
        let (buf, _size) = self.corners.entry(key).or_insert_with(|| {
            let buf = rasterize_corner_buffer(radius, quadrant, fill_rgba, border_rgba);
            (buf, Size::from((radius, radius)))
        });
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a cached `(inner_radius + spread) x (inner_radius + spread)` soft radial shadow corner.
    ///
    /// Pixels strictly inside `inner_radius` are kept transparent (`alpha = 0`) so shadows never
    /// bleed underneath translucent glass interiors.
    #[allow(clippy::too_many_arguments)]
    pub fn shadow_corner_element(
        &mut self,
        renderer: &mut GlesRenderer,
        inner_radius: i32,
        spread: i32,
        quadrant: CornerQuadrant,
        shadow_rgb: [u8; 3],
        max_alpha: u8,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if spread <= 0 || inner_radius < 0 {
            return None;
        }
        let dim = inner_radius + spread;
        let r_u16 = u16::try_from(inner_radius).ok()?;
        let s_u16 = u16::try_from(spread).ok()?;
        let key = ShadowCornerKey {
            inner_radius: r_u16,
            spread: s_u16,
            quadrant,
            shadow_rgb,
            max_alpha,
        };
        let (buf, _size) = self.shadow_corners.entry(key).or_insert_with(|| {
            let buf = rasterize_shadow_corner_buffer(
                inner_radius,
                spread,
                quadrant,
                shadow_rgb,
                max_alpha,
            );
            (buf, Size::from((dim, dim)))
        });
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a GPU-cached ambient Purple Glass wallpaper scaled smoothly to `screen_size` (`RUSTYOS-025`).
    pub fn wallpaper_element(
        &mut self,
        renderer: &mut GlesRenderer,
        screen_size: Size<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        const WALLPAPER_W: i32 = 256;
        const WALLPAPER_H: i32 = 160;
        if screen_size.w <= 0 || screen_size.h <= 0 {
            return None;
        }
        let buf = self
            .wallpaper
            .get_or_insert_with(|| rasterize_wallpaper_buffer(WALLPAPER_W, WALLPAPER_H));
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, smithay::utils::Physical>::from((0.0, 0.0)),
            buf,
            None,
            Some(Rectangle::<f64, Logical>::from_size(Size::from((
                f64::from(WALLPAPER_W),
                f64::from(WALLPAPER_H),
            )))),
            Some(screen_size),
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a GPU-cached anti-aliased desktop pointer arrow (24x24, hotspot at `(2, 2)`).
    pub fn cursor_arrow_element(
        &mut self,
        renderer: &mut GlesRenderer,
        top_left: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let buf = self
            .cursor_arrow
            .get_or_insert_with(rasterize_cursor_arrow_buffer);
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            top_left.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Cursor,
        )
        .ok()
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn premul_pixel(r: f32, g: f32, b: f32, a: f32) -> [u8; 4] {
    let ac = a.clamp(0.0, 1.0);
    let pb = (b.clamp(0.0, 255.0) * ac).round() as u8;
    let pg = (g.clamp(0.0, 255.0) * ac).round() as u8;
    let pr = (r.clamp(0.0, 255.0) * ac).round() as u8;
    let pa = (ac * 255.0).round() as u8;
    [pb, pg, pr, pa]
}

fn composite_over(fg: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
    let fg_a = f32::from(fg[3]) / 255.0;
    let bg_a = f32::from(bg[3]) / 255.0;
    let out_a = fg_a + bg_a * (1.0 - fg_a);
    let inv = 1.0 - fg_a;
    let b = (f32::from(fg[0]) + f32::from(bg[0]) * inv).clamp(0.0, 255.0);
    let g = (f32::from(fg[1]) + f32::from(bg[1]) * inv).clamp(0.0, 255.0);
    let r = (f32::from(fg[2]) + f32::from(bg[2]) * inv).clamp(0.0, 255.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    [
        b.round() as u8,
        g.round() as u8,
        r.round() as u8,
        (out_a.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_circle_pixel(
    px: i32,
    py: i32,
    diameter: i32,
    fill_rgba: [u8; 4],
    rim_rgba: Option<[u8; 4]>,
) -> [u8; 4] {
    let half = diameter as f32 * 0.5;
    let dx = (px as f32 + 0.5) - half;
    let dy = (py as f32 + 0.5) - half;
    let dist = dx.hypot(dy);
    let outer_r = half - 0.5;
    let cov = (0.5 - (dist - outer_r)).clamp(0.0, 1.0);
    if cov <= 0.0 {
        return [0, 0, 0, 0];
    }

    let fill_a = (f32::from(fill_rgba[3]) / 255.0) * cov;
    let fill_px = premul_pixel(
        f32::from(fill_rgba[0]),
        f32::from(fill_rgba[1]),
        f32::from(fill_rgba[2]),
        fill_a,
    );

    if let Some(rim) = rim_rgba {
        let rim_t = (dist - (outer_r - 1.25)).clamp(0.0, 1.0) * cov;
        if rim_t > 0.0 {
            let rim_a = (f32::from(rim[3]) / 255.0) * rim_t;
            let rim_px = premul_pixel(
                f32::from(rim[0]),
                f32::from(rim[1]),
                f32::from(rim[2]),
                rim_a,
            );
            return composite_over(rim_px, fill_px);
        }
    }

    fill_px
}

fn rasterize_circle_buffer(
    diameter: i32,
    fill_rgba: [u8; 4],
    rim_rgba: Option<[u8; 4]>,
) -> MemoryRenderBuffer {
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (diameter, diameter),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let dim_u = usize::try_from(diameter).unwrap_or(0);
            for py in 0..diameter {
                for px in 0..diameter {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * dim_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample_circle_pixel(px, py, diameter, fill_rgba, rim_rgba);
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((diameter, diameter)),
            )])
        });
    }
    mem_buf
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_corner_pixel(
    px: i32,
    py: i32,
    radius: i32,
    quadrant: CornerQuadrant,
    fill_rgba: [u8; 4],
    border_rgba: Option<[u8; 4]>,
) -> [u8; 4] {
    let r_f = radius as f32;
    let lx = match quadrant {
        CornerQuadrant::TopLeft | CornerQuadrant::BottomLeft => r_f - (px as f32 + 0.5),
        CornerQuadrant::TopRight | CornerQuadrant::BottomRight => px as f32 + 0.5,
    };
    let ly = match quadrant {
        CornerQuadrant::TopLeft | CornerQuadrant::TopRight => r_f - (py as f32 + 0.5),
        CornerQuadrant::BottomLeft | CornerQuadrant::BottomRight => py as f32 + 0.5,
    };
    let dist = lx.hypot(ly);
    let d_out = dist - (r_f - 0.5);
    let outer_cov = (0.5 - d_out).clamp(0.0, 1.0);
    if outer_cov <= 0.0 {
        return [0, 0, 0, 0];
    }

    let fill_a = (f32::from(fill_rgba[3]) / 255.0) * outer_cov;
    let fill_px = premul_pixel(
        f32::from(fill_rgba[0]),
        f32::from(fill_rgba[1]),
        f32::from(fill_rgba[2]),
        fill_a,
    );

    if let Some(border) = border_rgba {
        // 1px outer ring along the rounded arc matching straight 1px edge border strips
        let ring_cov = (d_out + 1.15).clamp(0.0, 1.0) * outer_cov;
        if ring_cov > 0.0 {
            let b_a = (f32::from(border[3]) / 255.0) * ring_cov;
            let b_px = premul_pixel(
                f32::from(border[0]),
                f32::from(border[1]),
                f32::from(border[2]),
                b_a,
            );
            return composite_over(b_px, fill_px);
        }
    }

    fill_px
}

fn rasterize_corner_buffer(
    radius: i32,
    quadrant: CornerQuadrant,
    fill_rgba: [u8; 4],
    border_rgba: Option<[u8; 4]>,
) -> MemoryRenderBuffer {
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (radius, radius),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let r_u = usize::try_from(radius).unwrap_or(0);
            for py in 0..radius {
                for px in 0..radius {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * r_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p =
                            sample_corner_pixel(px, py, radius, quadrant, fill_rgba, border_rgba);
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((radius, radius)),
            )])
        });
    }
    mem_buf
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_shadow_corner_pixel(
    px: i32,
    py: i32,
    inner_radius: i32,
    spread: i32,
    quadrant: CornerQuadrant,
    shadow_rgb: [u8; 3],
    max_alpha: u8,
) -> [u8; 4] {
    let dim = (inner_radius + spread) as f32;
    let r_f = inner_radius as f32;
    let s_f = spread as f32;

    let lx = match quadrant {
        CornerQuadrant::TopLeft | CornerQuadrant::BottomLeft => dim - (px as f32 + 0.5),
        CornerQuadrant::TopRight | CornerQuadrant::BottomRight => px as f32 + 0.5,
    };
    let ly = match quadrant {
        CornerQuadrant::TopLeft | CornerQuadrant::TopRight => dim - (py as f32 + 0.5),
        CornerQuadrant::BottomLeft | CornerQuadrant::BottomRight => py as f32 + 0.5,
    };
    let dist = lx.hypot(ly);
    if dist < r_f {
        return [0, 0, 0, 0];
    }
    let t = ((dist - r_f) / s_f).clamp(0.0, 1.0);
    let falloff = (1.0 - t) * (1.0 - t);
    let alpha = (f32::from(max_alpha) / 255.0) * falloff;
    premul_pixel(
        f32::from(shadow_rgb[0]),
        f32::from(shadow_rgb[1]),
        f32::from(shadow_rgb[2]),
        alpha,
    )
}

fn rasterize_shadow_corner_buffer(
    inner_radius: i32,
    spread: i32,
    quadrant: CornerQuadrant,
    shadow_rgb: [u8; 3],
    max_alpha: u8,
) -> MemoryRenderBuffer {
    let dim = inner_radius + spread;
    let mut mem_buf =
        MemoryRenderBuffer::new(Fourcc::Argb8888, (dim, dim), 1, Transform::Normal, None);
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let dim_u = usize::try_from(dim).unwrap_or(0);
            for py in 0..dim {
                for px in 0..dim {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * dim_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample_shadow_corner_pixel(
                            px,
                            py,
                            inner_radius,
                            spread,
                            quadrant,
                            shadow_rgb,
                            max_alpha,
                        );
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((dim, dim)),
            )])
        });
    }
    mem_buf
}

#[allow(clippy::cast_precision_loss)]
fn sample_wallpaper_pixel(px: i32, py: i32, width: i32, height: i32) -> [u8; 4] {
    let norm_x = (px as f32 + 0.5) / width.max(1) as f32;
    let norm_y = (py as f32 + 0.5) / height.max(1) as f32;

    // Deep Purple Glass base vertical gradient (#0F0A19 -> #181128)
    let mut red = 15.0 + 9.0 * norm_y;
    let mut green = 10.0 + 7.0 * norm_y;
    let mut blue = 25.0 + 16.0 * norm_y;

    // Primary upper-left neon violet aurora glow
    let dist1 = (norm_x - 0.24).hypot((norm_y - 0.22) * 0.85);
    let glow1 = (1.0 - (dist1 / 0.55)).clamp(0.0, 1.0);
    let amp1 = glow1 * glow1 * 0.22;
    red += 165.0 * amp1;
    green += 110.0 * amp1;
    blue += 255.0 * amp1;

    // Secondary bottom-right deep amethyst glow
    let dist2 = (norm_x - 0.78).hypot((norm_y - 0.76) * 0.9);
    let glow2 = (1.0 - (dist2 / 0.62)).clamp(0.0, 1.0);
    let amp2 = glow2 * glow2 * 0.26;
    red += 95.0 * amp2;
    green += 54.0 * amp2;
    blue += 178.0 * amp2;

    premul_pixel(red, green, blue, 1.0)
}

fn rasterize_wallpaper_buffer(w: i32, h: i32) -> MemoryRenderBuffer {
    let mut mem_buf = MemoryRenderBuffer::new(Fourcc::Argb8888, (w, h), 1, Transform::Normal, None);
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let w_u = usize::try_from(w).unwrap_or(0);
            for py in 0..h {
                for px in 0..w {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * w_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample_wallpaper_pixel(px, py, w, h);
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((w, h)),
            )])
        });
    }
    mem_buf
}

fn signed_distance_polygon(px: f32, py: f32, verts: &[(f32, f32)]) -> f32 {
    let mut d = f32::MAX;
    let mut s = 1.0_f32;
    let len = verts.len();
    for i in 0..len {
        let j = if i == 0 { len - 1 } else { i - 1 };
        let vi = verts[i];
        let vj = verts[j];
        let ex = vi.0 - vj.0;
        let ey = vi.1 - vj.1;
        let wx = px - vj.0;
        let wy = py - vj.1;
        let denom = ex * ex + ey * ey;
        let t = if denom > 0.0 {
            ((wx * ex + wy * ey) / denom).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bx = wx - ex * t;
        let by = wy - ey * t;
        d = d.min(bx * bx + by * by);

        let c1 = py >= vj.1;
        let c2 = py < vi.1;
        let c3 = ex * wy > ey * wx;
        if (c1 && c2 && c3) || (!c1 && !c2 && !c3) {
            s = -s;
        }
    }
    s * d.sqrt()
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_cursor_arrow_pixel(px: i32, py: i32) -> [u8; 4] {
    const ARROW_VERTS: [(f32, f32); 7] = [
        (2.5, 2.5),
        (2.5, 18.2),
        (6.4, 14.5),
        (8.9, 20.3),
        (11.1, 19.3),
        (8.6, 13.5),
        (13.8, 13.3),
    ];
    let x = px as f32 + 0.5;
    let y = py as f32 + 0.5;
    let sd = signed_distance_polygon(x, y, &ARROW_VERTS);
    let shadow_sd = signed_distance_polygon(x - 0.7, y - 1.1, &ARROW_VERTS);

    let shadow_alpha = ((3.1 - shadow_sd) / 2.2).clamp(0.0, 1.0) * 0.38;
    let mut out = premul_pixel(8.0, 5.0, 18.0, shadow_alpha);

    let rim_cov = (1.5 - sd).clamp(0.0, 1.0);
    if rim_cov > 0.0 {
        let rim = premul_pixel(250.0, 248.0, 255.0, rim_cov);
        out = composite_over(rim, out);
    }

    let fill_cov = (0.35 - sd).clamp(0.0, 1.0);
    if fill_cov > 0.0 {
        let fill = premul_pixel(22.0, 16.0, 38.0, fill_cov);
        out = composite_over(fill, out);
    }

    out
}

fn rasterize_cursor_arrow_buffer() -> MemoryRenderBuffer {
    const CURSOR_DIM: i32 = 24;
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (CURSOR_DIM, CURSOR_DIM),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let w_u = usize::try_from(CURSOR_DIM).unwrap_or(0);
            for py in 0..CURSOR_DIM {
                for px in 0..CURSOR_DIM {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * w_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        pixels[idx..idx + 4].copy_from_slice(&sample_cursor_arrow_pixel(px, py));
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((CURSOR_DIM, CURSOR_DIM)),
            )])
        });
    }
    mem_buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_and_corner_sdf_preserve_clean_edges_and_opaque_interior() {
        // Circle center must be opaque, extreme corner pixel outside circle must be transparent
        let center = sample_circle_pixel(6, 6, 12, [229, 107, 117, 255], None);
        let corner = sample_circle_pixel(0, 0, 12, [229, 107, 117, 255], None);
        assert_eq!(center[3], 255);
        assert_eq!(corner[3], 0);

        // TopLeft corner patch: (0, 0) outside arc is 0, inner pixel (9, 9) is filled
        let outer = sample_corner_pixel(
            0,
            0,
            10,
            CornerQuadrant::TopLeft,
            [32, 24, 46, 245],
            Some([255, 255, 255, 56]),
        );
        let inner = sample_corner_pixel(
            9,
            9,
            10,
            CornerQuadrant::TopLeft,
            [32, 24, 46, 245],
            Some([255, 255, 255, 56]),
        );
        assert_eq!(outer[3], 0);
        assert!(inner[3] >= 240);

        // Shadow corner: inside inner_radius is 0 alpha so glass stays clean; beyond inner_radius fades smoothly
        let inside_glass =
            sample_shadow_corner_pixel(23, 23, 10, 14, CornerQuadrant::TopLeft, [5, 2, 12], 96);
        let outside_halo =
            sample_shadow_corner_pixel(12, 12, 10, 14, CornerQuadrant::TopLeft, [5, 2, 12], 96);
        assert_eq!(inside_glass[3], 0);
        assert!(outside_halo[3] > 0 && outside_halo[3] <= 96);

        // Ambient wallpaper gradient is opaque and modulated smoothly around aurora glow
        let glow_px = sample_wallpaper_pixel(62, 35, 256, 160);
        let dark_px = sample_wallpaper_pixel(250, 10, 256, 160);
        assert_eq!(glow_px[3], 255);
        assert_eq!(dark_px[3], 255);
        assert!(glow_px[0] > dark_px[0]);

        // Desktop arrow cursor: opaque dark interior, bright rim, and transparent empty corner
        let arrow_inside = sample_cursor_arrow_pixel(5, 9);
        let arrow_outside = sample_cursor_arrow_pixel(22, 2);
        assert_eq!(arrow_inside[3], 255);
        assert!(arrow_inside[2] < 80);
        assert_eq!(arrow_outside[3], 0);
    }
}
