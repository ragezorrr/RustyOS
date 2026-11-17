//! SDF geometric primitives and pixel rasterizers for [`super::UiShapeCache`].

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::{Size, Transform};

use super::{CornerQuadrant, GlassPanelStyle, TrafficLightKind};

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(super) fn premul_pixel(r: f32, g: f32, b: f32, a: f32) -> [u8; 4] {
    let ac = a.clamp(0.0, 1.0);
    let pb = (b.clamp(0.0, 255.0) * ac).round() as u8;
    let pg = (g.clamp(0.0, 255.0) * ac).round() as u8;
    let pr = (r.clamp(0.0, 255.0) * ac).round() as u8;
    let pa = (ac * 255.0).round() as u8;
    [pb, pg, pr, pa]
}

pub(super) fn composite_over(fg: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
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
pub(super) fn sample_circle_pixel(
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

pub(super) fn rasterize_circle_buffer(
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
pub(super) fn sample_corner_pixel(
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

pub(super) fn rasterize_corner_buffer(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShadowEdgeSide {
    Top,
    Bottom,
    Left,
    Right,
}

fn shadow_falloff_weight(dist_outside: f32, spread: f32) -> f32 {
    let t = (dist_outside / spread.max(1.0)).clamp(0.0, 1.0);
    let smooth_end = smoothstep01(1.0, 0.72, t);
    (-(t * t) * 4.0).exp() * smooth_end
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(super) fn sample_shadow_corner_pixel(
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
    let alpha = (f32::from(max_alpha) / 255.0) * shadow_falloff_weight(dist - r_f, s_f);
    premul_pixel(
        f32::from(shadow_rgb[0]),
        f32::from(shadow_rgb[1]),
        f32::from(shadow_rgb[2]),
        alpha,
    )
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
pub(super) fn sample_shadow_edge_alpha(
    step: i32,
    spread: i32,
    side: ShadowEdgeSide,
    max_alpha: u8,
) -> u8 {
    let s = spread.max(1);
    let dist_out = match side {
        ShadowEdgeSide::Top | ShadowEdgeSide::Left => (s - 1 - step) as f32 + 0.5,
        ShadowEdgeSide::Bottom | ShadowEdgeSide::Right => step as f32 + 0.5,
    };
    ((f32::from(max_alpha) * shadow_falloff_weight(dist_out, s as f32)).round()).clamp(0.0, 255.0)
        as u8
}

#[allow(clippy::cast_precision_loss)]
pub(super) fn rasterize_shadow_edge_buffer(
    spread: i32,
    side: ShadowEdgeSide,
    shadow_rgb: [u8; 3],
    max_alpha: u8,
) -> MemoryRenderBuffer {
    let s = spread.max(1);
    let (w, h) = match side {
        ShadowEdgeSide::Top | ShadowEdgeSide::Bottom => (1, s),
        ShadowEdgeSide::Left | ShadowEdgeSide::Right => (s, 1),
    };
    let mut mem_buf = MemoryRenderBuffer::new(Fourcc::Argb8888, (w, h), 1, Transform::Normal, None);
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let max_a = f32::from(max_alpha) / 255.0;
            let s_f = s as f32;
            for step in 0..s {
                let dist_out = match side {
                    ShadowEdgeSide::Top | ShadowEdgeSide::Left => (s - 1 - step) as f32 + 0.5,
                    ShadowEdgeSide::Bottom | ShadowEdgeSide::Right => step as f32 + 0.5,
                };
                let alpha = max_a * shadow_falloff_weight(dist_out, s_f);
                let px_bytes = premul_pixel(
                    f32::from(shadow_rgb[0]),
                    f32::from(shadow_rgb[1]),
                    f32::from(shadow_rgb[2]),
                    alpha,
                );
                let idx = usize::try_from(step).unwrap_or(0) * 4;
                if idx + 4 <= pixels.len() {
                    pixels[idx..idx + 4].copy_from_slice(&px_bytes);
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((w, h)),
            )])
        });
    }
    mem_buf
}

pub(super) fn rasterize_shadow_corner_buffer(
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

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
pub(super) fn sample_wallpaper_pixel(px: i32, py: i32, width: i32, height: i32) -> [u8; 4] {
    sample_wallpaper_pixel_themed(
        px,
        py,
        width,
        height,
        crate::shell_render::active_appearance().is_dark(),
    )
}

fn smoothstep01(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fold_blend_rgb(a: [f32; 3], b: [f32; 3], k: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * k,
        a[1] + (b[1] - a[1]) * k,
        a[2] + (b[2] - a[2]) * k,
    ]
}

fn golden_gate_sky_rgb(norm_x: f32, norm_y: f32, is_dark: bool) -> [f32; 3] {
    let top_horizon = 1.0 - smoothstep01(0.0, 0.42, norm_y);
    let right_cool = smoothstep01(0.28, 0.95, norm_x * 0.72 + norm_y * 0.28);
    if is_dark {
        [
            20.0 + 18.0 * (1.0 - norm_y) - 6.0 * right_cool + 6.0 * top_horizon,
            24.0 + 16.0 * (1.0 - norm_y) - 4.0 * right_cool + 8.0 * top_horizon,
            34.0 + 18.0 * norm_x + 12.0 * right_cool + 10.0 * top_horizon,
        ]
    } else {
        let sky = [
            92.0 - 18.0 * right_cool - 14.0 * top_horizon,
            116.0 - 14.0 * right_cool - 10.0 * top_horizon,
            148.0 + 16.0 * right_cool - 8.0 * top_horizon,
        ];
        let warm_field = [
            196.0 + 34.0 * (1.0 - norm_x),
            174.0 + 28.0 * (1.0 - norm_x),
            146.0 - 8.0 * norm_x,
        ];
        fold_blend_rgb(
            warm_field,
            sky,
            (0.58 * top_horizon + 0.34 * right_cool).clamp(0.0, 1.0),
        )
    }
}

fn apply_golden_gate_folds(mut rgb: [f32; 3], norm_x: f32, norm_y: f32, is_dark: bool) -> [f32; 3] {
    let s1_y = 0.26 + 0.22 * norm_x + 0.11 * ((norm_x - 0.35) * 4.2).sin();
    let d1 = norm_y - s1_y;
    let sheet1 = smoothstep01(-0.018, 0.032, d1);
    let s1_shade = (d1.max(0.0) / 0.36).clamp(0.0, 1.0);
    let s1_color = if is_dark {
        [
            78.0 - 42.0 * s1_shade + 18.0 * (1.0 - norm_x),
            64.0 - 30.0 * s1_shade + 10.0 * (1.0 - norm_x),
            54.0 - 14.0 * s1_shade,
        ]
    } else {
        [
            228.0 - 54.0 * s1_shade,
            204.0 - 52.0 * s1_shade,
            168.0 - 42.0 * s1_shade + 18.0 * norm_x,
        ]
    };
    rgb = fold_blend_rgb(rgb, s1_color, sheet1);
    let crest1 = (-(d1 / 0.022).powi(2)).exp() * (0.45 + 0.55 * (1.0 - norm_x));
    let c1_amp = if is_dark { 38.0 } else { 32.0 };
    rgb[0] += c1_amp * crest1;
    rgb[1] += (c1_amp * 0.90) * crest1;
    rgb[2] += (c1_amp * 0.72) * crest1;

    let s2_y = 0.68 - 0.30 * norm_x + 0.095 * ((norm_x * 3.8) + 0.6).cos();
    let d2 = norm_y - s2_y;
    let sheet2 = smoothstep01(-0.016, 0.028, d2);
    let s2_shade = (d2.max(0.0) / 0.32).clamp(0.0, 1.0);
    let s2_color = if is_dark {
        [
            36.0 + 30.0 * (1.0 - s2_shade) * (1.0 - norm_x),
            40.0 + 24.0 * (1.0 - s2_shade) * (1.0 - norm_x),
            56.0 + 18.0 * (1.0 - s2_shade),
        ]
    } else {
        [
            144.0 + 68.0 * (1.0 - s2_shade) * (1.0 - 0.55 * norm_x) - 34.0 * s2_shade * norm_x,
            152.0 + 46.0 * (1.0 - s2_shade) * (1.0 - 0.55 * norm_x) - 18.0 * s2_shade * norm_x,
            174.0 - 14.0 * (1.0 - norm_x) + 22.0 * s2_shade * norm_x,
        ]
    };
    rgb = fold_blend_rgb(rgb, s2_color, sheet2);
    let shadow2 = (-((d2 + 0.032) / 0.048).powi(2)).exp();
    let s2_dark = if is_dark { 0.24 } else { 0.22 };
    rgb[0] *= 1.0 - s2_dark * shadow2;
    rgb[1] *= 1.0 - s2_dark * shadow2;
    rgb[2] *= 1.0 - (s2_dark * 0.72) * shadow2;

    let crest2 = (-(d2 / 0.018).powi(2)).exp() * (0.40 + 0.60 * (1.0 - (norm_x - 0.42).abs()));
    let c2_amp = if is_dark { 52.0 } else { 42.0 };
    rgb[0] += c2_amp * crest2;
    rgb[1] += (c2_amp * 0.92) * crest2;
    rgb[2] += (c2_amp * 0.78) * crest2;

    // Softened lower wave in the Dock region (`RUSTYOS-099`) so no dark diagonal band reads as shadow behind Dock.
    let s3_y = 0.86 - 0.12 * (norm_x - 0.25) - 0.025 * ((norm_x - 0.55) * 4.5).sin();
    let d3 = norm_y - s3_y;
    let sheet3 = smoothstep01(-0.26, 0.22, d3);
    let s3_color = if is_dark {
        [
            22.0 + 10.0 * (1.0 - norm_x),
            28.0 + 10.0 * (1.0 - norm_x),
            42.0 + 12.0 * norm_x,
        ]
    } else {
        [
            128.0 + 26.0 * (1.0 - norm_x),
            138.0 + 22.0 * (1.0 - norm_x),
            162.0 + 14.0 * norm_x,
        ]
    };
    rgb = fold_blend_rgb(rgb, s3_color, sheet3 * 0.35);
    let crest3 = (-(d3 / 0.045).powi(2)).exp() * norm_x;
    [
        rgb[0] + 6.0 * crest3,
        rgb[1] + 7.0 * crest3,
        rgb[2] + 8.0 * crest3,
    ]
}

#[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
pub(super) fn sample_wallpaper_pixel_themed(
    px: i32,
    py: i32,
    width: i32,
    height: i32,
    is_dark: bool,
) -> [u8; 4] {
    const BAYER_4X4: [[f32; 4]; 4] = [
        [0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0],
        [12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0],
        [3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0],
        [15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0],
    ];

    let norm_x = (px as f32 + 0.5) / width.max(1) as f32;
    let norm_y = (py as f32 + 0.5) / height.max(1) as f32;
    let base = golden_gate_sky_rgb(norm_x, norm_y, is_dark);
    let rgb = apply_golden_gate_folds(base, norm_x, norm_y, is_dark);

    // Ordered 4x4 Bayer dithering to eliminate 8-bit gradient banding
    let bx = usize::try_from(px.rem_euclid(4)).unwrap_or(0);
    let by = usize::try_from(py.rem_euclid(4)).unwrap_or(0);
    let dither = (BAYER_4X4[by][bx] - 0.5) * 1.45;
    premul_pixel(rgb[0] + dither, rgb[1] + dither, rgb[2] + dither, 1.0)
}

pub(super) fn rasterize_wallpaper_buffer(w: i32, h: i32, is_dark: bool) -> MemoryRenderBuffer {
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
                        let p = sample_wallpaper_pixel_themed(px, py, w, h, is_dark);
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

pub(super) fn sd_circle(u: f32, v: f32, cx: f32, cy: f32, r: f32) -> f32 {
    (u - cx).hypot(v - cy) - r
}

pub(super) fn sd_rounded_box(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (u - cx).abs() - (hw - r);
    let qy = (v - cy).abs() - (hh - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

pub(super) fn sd_segment(u: f32, v: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> f32 {
    let pax = u - ax;
    let pay = v - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = bax * bax + bay * bay;
    let h = if denom > 1e-6 {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (pax - bax * h).hypot(pay - bay * h) - r
}

pub(super) fn sd_cov(sd: f32, edge_w: f32) -> f32 {
    (0.5 - sd / edge_w.max(0.01)).clamp(0.0, 1.0)
}

pub(super) fn blend_premul_over(bg: [f32; 4], fg_rgb: [f32; 3], fg_a: f32) -> [f32; 4] {
    let a = fg_a.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    [
        fg_rgb[0] * a + bg[0] * inv,
        fg_rgb[1] * a + bg[1] * inv,
        fg_rgb[2] * a + bg[2] * inv,
        a + bg[3] * inv,
    ]
}

pub(super) fn premul_tint(tint: [f32; 4], cov: f32) -> [f32; 4] {
    let a = (tint[3] * cov).clamp(0.0, 1.0);
    [tint[0] * a, tint[1] * a, tint[2] * a, a]
}

/// Signed distance to an upward-opening circular arc with true rounded endcaps (`Inigo Quilez sdArc`).
pub(super) fn sd_arc_up(
    u: f32,
    v: f32,
    cx: f32,
    cy: f32,
    radius: f32,
    half_angle_rad: f32,
    stroke: f32,
) -> f32 {
    let px = (u - cx).abs();
    let py = cy - v;
    let (sin_a, cos_a) = half_angle_rad.sin_cos();
    let dist = if px * cos_a < py * sin_a {
        (px.hypot(py) - radius).abs()
    } else {
        (px - radius * sin_a).hypot(py - radius * cos_a)
    };
    dist - stroke
}

/// Signed distance to an upward-opening circular pie sector (`half_angle_rad`, rounded by `corner`).
pub(super) fn sd_pie_up(
    u: f32,
    v: f32,
    cx: f32,
    cy: f32,
    radius: f32,
    half_angle_rad: f32,
    corner: f32,
) -> f32 {
    let px = (u - cx).abs();
    let py = cy - v;
    let (sin_a, cos_a) = half_angle_rad.sin_cos();
    let radial_d = px.hypot(py) - radius;
    let proj = (px * sin_a + py * cos_a).clamp(0.0, radius);
    let ray_dist = (px - sin_a * proj).hypot(py - cos_a * proj);
    let outside_sign = if px * cos_a > py * sin_a { 1.0 } else { -1.0 };
    radial_d.max(ray_dist * outside_sign) - corner
}

#[allow(clippy::cast_precision_loss)]
pub(super) fn sample_fallback_avatar_pixel(
    px: i32,
    py: i32,
    diameter: i32,
    seed: u32,
    selected: bool,
) -> [u8; 4] {
    const SUBSAMPLES: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
    let dim = diameter.max(1) as f32;
    let edge = 1.05 / dim;
    let palette = match seed % 4 {
        0 => ([74.0_f32, 118.0, 184.0], [182.0_f32, 212.0, 238.0]),
        1 => ([86.0_f32, 134.0, 224.0], [186.0_f32, 220.0, 250.0]),
        2 => ([158.0_f32, 128.0, 96.0], [232.0_f32, 214.0, 188.0]),
        _ => ([94.0_f32, 148.0, 142.0], [214.0_f32, 232.0, 194.0]),
    };
    let mut sum = [0.0_f32; 4];
    for sy in SUBSAMPLES {
        for sx in SUBSAMPLES {
            let u = (px as f32 + sx) / dim;
            let v = (py as f32 + sy) / dim;
            let outer = sd_circle(u, v, 0.5, 0.5, 0.475);
            let cov = sd_cov(outer, edge);
            if cov <= 0.001 {
                continue;
            }
            let radial = ((u - 0.38).hypot(v - 0.30) / 0.72).clamp(0.0, 1.0);
            let mut red = palette.1[0] * (1.0 - radial) + palette.0[0] * radial;
            let mut green = palette.1[1] * (1.0 - radial) + palette.0[1] * radial;
            let mut blue = palette.1[2] * (1.0 - radial) + palette.0[2] * radial;

            let head = sd_circle(u, v, 0.50, 0.38, 0.155);
            let shoulders = sd_circle(u, v, 0.50, 0.90, 0.34);
            let fig = sd_cov(head.min(shoulders), edge);
            if fig > 0.001 {
                let fa = fig * 0.90;
                red = red * (1.0 - fa) + 248.0 * fa;
                green = green * (1.0 - fa) + 250.0 * fa;
                blue = blue * (1.0 - fa) + 254.0 * fa;
            }

            let rim_sd = (outer + 0.018).abs() - 0.016;
            let rim_cov = sd_cov(rim_sd, edge) * if selected { 0.92 } else { 0.62 };
            if rim_cov > 0.001 {
                red = red * (1.0 - rim_cov) + 255.0 * rim_cov;
                green = green * (1.0 - rim_cov) + 255.0 * rim_cov;
                blue = blue * (1.0 - rim_cov) + 255.0 * rim_cov;
            }

            sum[0] += red * cov;
            sum[1] += green * cov;
            sum[2] += blue * cov;
            sum[3] += cov;
        }
    }
    let avg_a = (sum[3] * 0.0625).clamp(0.0, 1.0);
    if avg_a <= 0.001 {
        return [0, 0, 0, 0];
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    [
        (sum[2] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (sum[1] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (sum[0] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (avg_a * 255.0).clamp(0.0, 255.0).round() as u8,
    ]
}

pub(crate) fn signed_distance_polygon(px: f32, py: f32, verts: &[(f32, f32)]) -> f32 {
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

pub(super) fn rasterize_rgba_rect(
    width: i32,
    height: i32,
    sample: impl Fn(i32, i32) -> [u8; 4],
) -> MemoryRenderBuffer {
    let w = width.max(1);
    let h = height.max(1);
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
                        let p = sample(px, py);
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

#[cfg(test)]
pub(super) fn sample_dock_glass_pixel(
    px: i32,
    py: i32,
    width: i32,
    height: i32,
    radius: i32,
    glass_opacity: f32,
    reduce_transparency: bool,
) -> [u8; 4] {
    sample_glass_panel_pixel(
        px,
        py,
        width,
        height,
        radius,
        glass_opacity,
        reduce_transparency,
        GlassPanelStyle::Dock,
    )
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::too_many_arguments)]
pub(super) fn sample_glass_panel_pixel(
    px: i32,
    py: i32,
    width: i32,
    height: i32,
    radius: i32,
    glass_opacity: f32,
    reduce_transparency: bool,
    style: GlassPanelStyle,
) -> [u8; 4] {
    sample_glass_panel_pixel_themed(
        px,
        py,
        width,
        height,
        radius,
        glass_opacity,
        reduce_transparency,
        style,
        crate::shell_render::active_appearance().is_dark(),
    )
}

fn base_glass_layer(
    style: GlassPanelStyle,
    is_dark: bool,
    _t_v: f32,
    dither: f32,
    glass_scale: f32,
) -> [f32; 4] {
    let (base_a, rgb) = match (is_dark, style) {
        // RUSTYOS-100: CutefishOS uniform glass layer across Dock, Popover (`popupGlass`), and ControlModule (`fishuiGlass`).
        (true, GlassPanelStyle::Dock) => (0.29 * (0.45 + 0.55 * glass_scale), [96.0, 96.0, 98.0]),
        (true, GlassPanelStyle::Popover) => {
            (0.76 * (0.50 + 0.50 * glass_scale), [43.0, 43.0, 48.0])
        }
        (true, GlassPanelStyle::ControlModule) => {
            (0.28 * (0.48 + 0.52 * glass_scale), [60.0, 60.0, 68.0])
        }
        (false, GlassPanelStyle::Dock) => {
            (0.25 * (0.44 + 0.56 * glass_scale), [153.0, 153.0, 153.0])
        }
        (false, GlassPanelStyle::Popover) => {
            (0.62 * (0.52 + 0.48 * glass_scale), [250.0, 250.0, 252.0])
        }
        (false, GlassPanelStyle::ControlModule) => {
            (0.22 * (0.48 + 0.52 * glass_scale), [184.0, 188.0, 198.0])
        }
    };
    [
        (rgb[0] + dither) * base_a,
        (rgb[1] + dither) * base_a,
        (rgb[2] + dither) * base_a,
        base_a,
    ]
}

fn outward_normal_2d(u: f32, v: f32, hw: f32, hh: f32, r: f32) -> (f32, f32) {
    let px = u - hw;
    let py = v - hh;
    let hx = (hw - r).max(0.0);
    let hy = (hh - r).max(0.0);
    let qx = px.abs() - hx;
    let qy = py.abs() - hy;
    let sx = if px >= 0.0 { 1.0 } else { -1.0 };
    let sy = if py >= 0.0 { 1.0 } else { -1.0 };
    if qx > 0.0 && qy > 0.0 {
        let rx = qx.max(1e-4);
        let ry = qy.max(1e-4);
        let len = rx.hypot(ry);
        (sx * rx / len, sy * ry / len)
    } else {
        let blend = smoothstep01(-2.0, 2.0, qx - qy);
        let gx = blend * sx;
        let gy = (1.0 - blend) * sy;
        let glen = gx.hypot(gy).max(1e-4);
        (gx / glen, gy / glen)
    }
}

fn cutefish_rim_alpha(outward_y: f32, sd: f32, rim_peak: f32) -> f32 {
    let lit = outward_y.abs() * (1.0 - 0.65 * (0.5 + 0.5 * outward_y));
    let inset = -sd;
    let hairline = smoothstep01(1.2, 0.0, (inset - 1.0).abs());
    let glow = if inset > 0.5 {
        (-(inset - 2.0).max(0.0) / 1.7).exp() * 0.16
    } else {
        0.0
    };
    ((hairline + glow) * lit * rim_peak).clamp(0.0, 0.55)
}

#[allow(clippy::cast_precision_loss, clippy::too_many_arguments)]
pub(super) fn sample_glass_panel_pixel_themed(
    px: i32,
    py: i32,
    width: i32,
    height: i32,
    radius: i32,
    glass_opacity: f32,
    reduce_transparency: bool,
    style: GlassPanelStyle,
    is_dark: bool,
) -> [u8; 4] {
    const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

    let w_f = width.max(1) as f32;
    let h_f = height.max(1) as f32;
    let hw = w_f * 0.5;
    let hh = h_f * 0.5;
    let r = (radius as f32).clamp(0.0, hw.min(hh));
    let u = px as f32 + 0.5;
    let v = py as f32 + 0.5;

    let sd = sd_rounded_box(u, v, hw, hh, hw, hh, r);
    let cov = sd_cov(sd, 1.15);
    if cov <= 0.001 {
        return [0, 0, 0, 0];
    }

    let t_v = (v / h_f).clamp(0.0, 1.0);
    let by = usize::try_from(py.rem_euclid(4)).unwrap_or(0);
    let bx = usize::try_from(px.rem_euclid(4)).unwrap_or(0);
    let dither = (f32::from(BAYER4[by][bx]) / 16.0 - 0.5) * 0.9;

    if reduce_transparency {
        let (base_rgb, alpha) = match (is_dark, style) {
            (true, GlassPanelStyle::Dock) => ([34.0, 34.0, 38.0], 246.0 / 255.0),
            (true, GlassPanelStyle::Popover) => ([38.0, 38.0, 42.0], 248.0 / 255.0),
            (true, GlassPanelStyle::ControlModule) => ([54.0, 54.0, 60.0], 232.0 / 255.0),
            (false, GlassPanelStyle::Dock) => ([236.0, 238.0, 243.0], 246.0 / 255.0),
            (false, GlassPanelStyle::Popover) => ([246.0, 247.0, 250.0], 248.0 / 255.0),
            (false, GlassPanelStyle::ControlModule) => ([238.0, 240.0, 245.0], 238.0 / 255.0),
        };
        return premul_pixel(
            base_rgb[0] + dither,
            base_rgb[1] + dither,
            base_rgb[2] + dither,
            alpha * cov,
        );
    }

    let glass_scale = crate::config::clamp_glass_opacity(glass_opacity);
    let mut layer = base_glass_layer(style, is_dark, t_v, dither, glass_scale);

    // RUSTYOS-100: Unified CutefishOS directional uRim across Dock, Popover, and ControlModule (no sheen/corner flares).
    let (_nx, ny) = outward_normal_2d(u, v, hw, hh, r);
    let rim_peak = match (is_dark, style) {
        (false, GlassPanelStyle::Dock) => 0.43,
        (true, GlassPanelStyle::Dock | GlassPanelStyle::ControlModule) => 0.35,
        (false, GlassPanelStyle::Popover) => 0.36,
        (true, GlassPanelStyle::Popover) => 0.30,
        (false, GlassPanelStyle::ControlModule) => 0.42,
    };
    let rim_alpha = cutefish_rim_alpha(ny, sd, rim_peak);
    if rim_alpha > 0.001 {
        layer = blend_premul_over(layer, [255.0, 255.0, 255.0], rim_alpha);
    }

    let out_a = (layer[3] * cov).clamp(0.0, 1.0);
    if layer[3] <= 1e-4 || out_a <= 1e-4 {
        return [0, 0, 0, 0];
    }
    premul_pixel(
        layer[0] / layer[3],
        layer[1] / layer[3],
        layer[2] / layer[3],
        out_a,
    )
}

#[allow(clippy::cast_precision_loss)]
pub(super) fn sample_traffic_light_pixel(
    px: i32,
    py: i32,
    diameter: i32,
    kind: TrafficLightKind,
    focused: bool,
    hovered_glyph: bool,
) -> [u8; 4] {
    const SUBSAMPLES: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
    let dim = diameter.max(1) as f32;
    let edge = 1.05 / dim;
    let active_look = focused || hovered_glyph;
    let (fill_top, fill_bot, ring_rgb, glyph_rgb) = if active_look {
        match kind {
            TrafficLightKind::Close => (
                [255.0, 108.0, 99.0],
                [246.0, 78.0, 70.0],
                [194.0, 56.0, 50.0],
                [76.0, 14.0, 12.0],
            ),
            TrafficLightKind::Minimize => (
                [255.0, 198.0, 62.0],
                [246.0, 174.0, 32.0],
                [196.0, 134.0, 24.0],
                [92.0, 58.0, 4.0],
            ),
            TrafficLightKind::Maximize => (
                [56.0, 212.0, 78.0],
                [34.0, 186.0, 54.0],
                [24.0, 140.0, 40.0],
                [10.0, 68.0, 18.0],
            ),
        }
    } else {
        (
            [128.0, 130.0, 136.0],
            [106.0, 108.0, 114.0],
            [84.0, 86.0, 92.0],
            [42.0, 44.0, 48.0],
        )
    };

    let mut sum = [0.0_f32; 4];
    for sy in SUBSAMPLES {
        for sx in SUBSAMPLES {
            let u = (px as f32 + sx) / dim;
            let v = (py as f32 + sy) / dim;
            let body_sd = sd_circle(u, v, 0.50, 0.50, 0.44);
            let body_cov = sd_cov(body_sd, edge);
            if body_cov <= 0.001 {
                continue;
            }
            let t = v.clamp(0.0, 1.0);
            let mut rgb = [
                fill_top[0] + (fill_bot[0] - fill_top[0]) * t,
                fill_top[1] + (fill_bot[1] - fill_top[1]) * t,
                fill_top[2] + (fill_bot[2] - fill_top[2]) * t,
            ];
            let base_a = if active_look { 1.0 } else { 0.82 };
            let mut prem = [rgb[0] * base_a, rgb[1] * base_a, rgb[2] * base_a, base_a];

            // Soft top specular dot & rim stroke so the dot reads like backlit glass gel.
            let gloss_sd = sd_circle(u, v, 0.50, 0.34, 0.24);
            let gloss_cov = sd_cov(gloss_sd, edge * 2.5) * (1.0 - t).clamp(0.0, 1.0);
            if gloss_cov > 0.0 && active_look {
                prem = blend_premul_over(prem, [255.0, 250.0, 255.0], gloss_cov * 0.22);
            }
            let rim_sd = sd_circle(u, v, 0.50, 0.50, 0.415).abs() - 0.025;
            let rim_cov = sd_cov(rim_sd, edge);
            if rim_cov > 0.0 {
                prem = blend_premul_over(prem, ring_rgb, rim_cov * 0.55);
            }
            if hovered_glyph {
                let g_sd = match kind {
                    TrafficLightKind::Close => sd_segment(u, v, 0.33, 0.33, 0.67, 0.67, 0.055)
                        .min(sd_segment(u, v, 0.67, 0.33, 0.33, 0.67, 0.055)),
                    TrafficLightKind::Minimize => sd_segment(u, v, 0.30, 0.50, 0.70, 0.50, 0.058),
                    TrafficLightKind::Maximize => sd_segment(u, v, 0.30, 0.50, 0.70, 0.50, 0.055)
                        .min(sd_segment(u, v, 0.50, 0.30, 0.50, 0.70, 0.055)),
                };
                let g_cov = sd_cov(g_sd, edge);
                if g_cov > 0.0 {
                    prem = blend_premul_over(prem, glyph_rgb, g_cov * 0.88);
                }
            }

            rgb = if prem[3] > 1e-4 {
                [prem[0] / prem[3], prem[1] / prem[3], prem[2] / prem[3]]
            } else {
                rgb
            };
            let out_a = (prem[3] * body_cov).clamp(0.0, 1.0);
            sum[0] += rgb[0] * out_a;
            sum[1] += rgb[1] * out_a;
            sum[2] += rgb[2] * out_a;
            sum[3] += out_a;
        }
    }
    let avg_a = (sum[3] * 0.0625).clamp(0.0, 1.0);
    if avg_a <= 0.001 {
        return [0, 0, 0, 0];
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    [
        (sum[2] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (sum[1] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (sum[0] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (avg_a * 255.0).clamp(0.0, 255.0).round() as u8,
    ]
}

#[allow(clippy::cast_precision_loss)]
pub(super) fn sample_dock_icon_shadow_pixel(px: i32, py: i32, icon_size: i32, pad: i32) -> [u8; 4] {
    let total = (icon_size + pad * 2).max(1) as f32;
    let center = total * 0.5;
    let half = (icon_size as f32 * 0.5 - 1.5).max(1.0);
    let r = (icon_size as f32 * 0.23).min(half);
    let sd = sd_rounded_box(
        px as f32 + 0.5,
        py as f32 + 0.5,
        center,
        center,
        half,
        half,
        r,
    );
    let spread = (pad as f32 - 0.4).max(1.0);
    let t = (sd.max(0.0) / spread).clamp(0.0, 1.0);
    let falloff = 1.0 - t;
    let alpha = if sd <= 0.0 {
        0.10
    } else {
        0.10 * falloff * falloff * falloff
    };
    premul_pixel(6.0, 8.0, 14.0, alpha)
}
