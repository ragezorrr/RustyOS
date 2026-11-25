//! Pure-Rust PNG and PPM image decoding and premultiplied `BGRA` scaling helpers.
//!
//! Extracted from the compositor icon/wallpaper decoder so both `rusty-compositor`
//! and `apps/rusty-files` share one zero-dependency, `unsafe`-free implementation
//! with strict dimension, file-size, and decompression limits.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Maximum pixel dimension allowed for icons/tray pixmaps.
pub const MAX_ICON_DIM: u32 = 512;
/// Maximum PNG file size allowed for XDG icon loading (`2 MiB`).
pub const MAX_PNG_FILE_BYTES: usize = 2 * 1024 * 1024;
/// Maximum inflated raw scanline bytes for icons (`4 MiB`).
pub const MAX_INFLATE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum pixel dimension allowed for wallpapers and file manager image previews.
pub const MAX_WALLPAPER_DIM: u32 = 4096;
/// Maximum file size allowed for wallpapers and file manager image previews (`32 MiB`).
pub const MAX_WALLPAPER_FILE_BYTES: usize = 32 * 1024 * 1024;
/// Maximum inflated scanline bytes for wallpapers and file manager image previews (`80 MiB`).
pub const MAX_WALLPAPER_INFLATE_BYTES: usize = 80 * 1024 * 1024;

/// Decoded image preview scaled into premultiplied `Argb8888` (`[B, G, R, A]`) pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPreviewImage {
    /// Original source width in pixels.
    pub orig_width: u32,
    /// Original source height in pixels.
    pub orig_height: u32,
    /// Scaled buffer width in pixels.
    pub width: usize,
    /// Scaled buffer height in pixels.
    pub height: usize,
    /// Premultiplied `[B, G, R, A]` pixels (`width * height * 4` bytes).
    pub bgra_premul: Vec<u8>,
}

/// Converts raw big-endian `ARGB32` pixels into scaled premultiplied `Argb8888` (`[B, G, R, A]`).
#[must_use]
pub fn argb32_be_to_premul_bgra(
    width: i32,
    height: i32,
    argb_be: &[u8],
    dst_size: usize,
) -> Option<Vec<u8>> {
    let w = usize::try_from(width).ok()?;
    let h = usize::try_from(height).ok()?;
    if w == 0 || h == 0 || dst_size == 0 || argb_be.len() != w.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    let mut rgba = vec![0_u8; argb_be.len()];
    let (src_chunks, _) = argb_be.as_chunks::<4>();
    let (dst_chunks, _) = rgba.as_chunks_mut::<4>();
    for (src, dst) in src_chunks.iter().zip(dst_chunks.iter_mut()) {
        dst[0] = src[1];
        dst[1] = src[2];
        dst[2] = src[3];
        dst[3] = src[0];
    }
    scale_rgba_to_premul_bgra(w, h, &rgba, dst_size)
}

/// Computes integer aspect-preserving dimensions fitting inside `max_w x max_h`.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn fit_aspect_dimensions(
    src_w: usize,
    src_h: usize,
    max_w: usize,
    max_h: usize,
) -> Option<(usize, usize)> {
    if src_w == 0 || src_h == 0 || max_w == 0 || max_h == 0 {
        return None;
    }
    let scale_w = max_w as f32 / src_w as f32;
    let scale_h = max_h as f32 / src_h as f32;
    let scale = scale_w.min(scale_h).min(1.0);
    let dst_w = ((src_w as f32) * scale).round().max(1.0) as usize;
    let dst_h = ((src_h as f32) * scale).round().max(1.0) as usize;
    Some((dst_w.min(max_w), dst_h.min(max_h)))
}

/// Scales straight `RGBA` pixels to `dst_size x dst_size` premultiplied `Argb8888` (`[B, G, R, A]`).
#[must_use]
pub fn scale_rgba_to_premul_bgra(
    src_w: usize,
    src_h: usize,
    rgba: &[u8],
    dst_size: usize,
) -> Option<Vec<u8>> {
    scale_rgba_rect_to_premul_bgra(src_w, src_h, rgba, dst_size, dst_size)
}

/// Scales straight `RGBA` pixels into `dst_w x dst_h` premultiplied `Argb8888` (`[B, G, R, A]`).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn scale_rgba_rect_to_premul_bgra(
    src_w: usize,
    src_h: usize,
    rgba: &[u8],
    dst_w: usize,
    dst_h: usize,
) -> Option<Vec<u8>> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return None;
    }
    if rgba.len() != src_w.checked_mul(src_h)?.checked_mul(4)? {
        return None;
    }
    if (dst_w < src_w || dst_h < src_h) && src_w.checked_mul(src_h)? <= LANCZOS_MAX_SRC_PIXELS {
        return Some(downscale_lanczos_linear(src_w, src_h, rgba, dst_w, dst_h));
    }

    let total_bytes = dst_w.checked_mul(dst_h)?.checked_mul(4)?;
    let mut out = vec![0_u8; total_bytes];
    for dy in 0..dst_h {
        let sample_row = ((dy as f32) + 0.5) * (src_h as f32) / (dst_h as f32) - 0.5;
        for dx in 0..dst_w {
            let sample_col = ((dx as f32) + 0.5) * (src_w as f32) / (dst_w as f32) - 0.5;
            let val = sample_rgba_bilinear_premul(rgba, src_w, src_h, sample_col, sample_row);
            let dst_off = (dy * dst_w + dx) * 4;
            out[dst_off] = val[2].round().clamp(0.0, 255.0) as u8;
            out[dst_off + 1] = val[1].round().clamp(0.0, 255.0) as u8;
            out[dst_off + 2] = val[0].round().clamp(0.0, 255.0) as u8;
            out[dst_off + 3] = val[3].round().clamp(0.0, 255.0) as u8;
        }
    }
    Some(out)
}

/// Largest source (in pixels) routed through the memory-hungry `f32` Lanczos downscaler (`RUSTYOS-111`).
const LANCZOS_MAX_SRC_PIXELS: usize = 2048 * 2048;

#[inline]
fn lanczos3(x: f32) -> f32 {
    let x = x.abs();
    if x < 1e-6 {
        1.0
    } else if x >= 3.0 {
        0.0
    } else {
        let px = std::f32::consts::PI * x;
        3.0 * px.sin() * (px / 3.0).sin() / (px * px)
    }
}

/// Per-destination-index `(first_src_index, normalized weights)` for a separable Lanczos-3 axis pass.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
fn lanczos_axis_weights(src_len: usize, dst_len: usize) -> Vec<(usize, Vec<f32>)> {
    let ratio = src_len as f32 / dst_len as f32;
    let filter_scale = ratio.max(1.0);
    let support = 3.0 * filter_scale;
    (0..dst_len)
        .map(|d| {
            let center = (d as f32 + 0.5) * ratio - 0.5;
            let lo = ((center - support).ceil() as i64).max(0) as usize;
            let hi = (((center + support).floor() as i64).min(src_len as i64 - 1) as usize).max(lo);
            let mut ws: Vec<f32> = (lo..=hi)
                .map(|i| lanczos3((i as f32 - center) / filter_scale))
                .collect();
            let sum: f32 = ws.iter().sum();
            if sum.abs() < 1e-6 {
                ws = vec![1.0];
            } else {
                for w in &mut ws {
                    *w /= sum;
                }
            }
            (lo, ws)
        })
        .collect()
}

fn srgb_to_linear_f32(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_f32(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// High-quality area-aware downscale: separable Lanczos-3 in linear light on premultiplied colour,
/// returning premultiplied `Argb8888` (`[B, G, R, A]`) without stair-stepping or dark fringes (`RUSTYOS-111`).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn downscale_lanczos_linear(
    src_w: usize,
    src_h: usize,
    rgba: &[u8],
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    let mut lut = [0.0_f32; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        *v = srgb_to_linear_f32(i as f32 / 255.0);
    }
    let mut lin = vec![0.0_f32; src_w * src_h * 4];
    let (lin_px, _) = lin.as_chunks_mut::<4>();
    let (src_px, _) = rgba.as_chunks::<4>();
    for (dst, px) in lin_px.iter_mut().zip(src_px) {
        let a = f32::from(px[3]) / 255.0;
        dst[0] = lut[usize::from(px[0])] * a;
        dst[1] = lut[usize::from(px[1])] * a;
        dst[2] = lut[usize::from(px[2])] * a;
        dst[3] = a;
    }

    let wx = lanczos_axis_weights(src_w, dst_w);
    let wy = lanczos_axis_weights(src_h, dst_h);

    let mut tmp = vec![0.0_f32; dst_w * src_h * 4];
    for y in 0..src_h {
        for (dx, (start, ws)) in wx.iter().enumerate() {
            let mut acc = [0.0_f32; 4];
            for (k, w) in ws.iter().enumerate() {
                let o = (y * src_w + start + k) * 4;
                for (a, v) in acc.iter_mut().zip(&lin[o..o + 4]) {
                    *a += v * w;
                }
            }
            let o = (y * dst_w + dx) * 4;
            tmp[o..o + 4].copy_from_slice(&acc);
        }
    }

    let mut out = vec![0_u8; dst_w * dst_h * 4];
    for (dy, (start, ws)) in wy.iter().enumerate() {
        for dx in 0..dst_w {
            let mut acc = [0.0_f32; 4];
            for (k, w) in ws.iter().enumerate() {
                let o = ((start + k) * dst_w + dx) * 4;
                for (a, v) in acc.iter_mut().zip(&tmp[o..o + 4]) {
                    *a += v * w;
                }
            }
            let alpha = acc[3].clamp(0.0, 1.0);
            let off = (dy * dst_w + dx) * 4;
            if alpha <= 1.0 / 512.0 {
                continue;
            }
            let enc = |lin_premul: f32| -> u8 {
                let straight = linear_to_srgb_f32(lin_premul.clamp(0.0, alpha) / alpha);
                (straight * alpha * 255.0).clamp(0.0, 255.0).round() as u8
            };
            out[off] = enc(acc[2]);
            out[off + 1] = enc(acc[1]);
            out[off + 2] = enc(acc[0]);
            out[off + 3] = (alpha * 255.0).round() as u8;
        }
    }
    out
}

#[inline]
fn sample_rgba_premul(rgba: &[u8], width: usize, x: usize, y: usize) -> [f32; 4] {
    let off = (y * width + x) * 4;
    let a = f32::from(rgba[off + 3]) / 255.0;
    [
        f32::from(rgba[off]) * a,
        f32::from(rgba[off + 1]) * a,
        f32::from(rgba[off + 2]) * a,
        f32::from(rgba[off + 3]),
    ]
}

#[inline]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_rgba_bilinear_premul(
    rgba: &[u8],
    src_w: usize,
    src_h: usize,
    sample_col: f32,
    sample_row: f32,
) -> [f32; 4] {
    let y0 = (sample_row.floor().max(0.0) as usize).min(src_h - 1);
    let y1 = (y0 + 1).min(src_h - 1);
    let fy = (sample_row - y0 as f32).clamp(0.0, 1.0);

    let x0 = (sample_col.floor().max(0.0) as usize).min(src_w - 1);
    let x1 = (x0 + 1).min(src_w - 1);
    let fx = (sample_col - x0 as f32).clamp(0.0, 1.0);

    let p00 = sample_rgba_premul(rgba, src_w, x0, y0);
    let p10 = sample_rgba_premul(rgba, src_w, x1, y0);
    let p01 = sample_rgba_premul(rgba, src_w, x0, y1);
    let p11 = sample_rgba_premul(rgba, src_w, x1, y1);

    let mut out = [0.0_f32; 4];
    for ch in 0..4 {
        let top = p00[ch] + (p10[ch] - p00[ch]) * fx;
        let bot = p01[ch] + (p11[ch] - p01[ch]) * fx;
        out[ch] = top + (bot - top) * fy;
    }
    out
}

/// Loads a user `.png` wallpaper and scales it with aspect-preserving center-crop ("cover")
/// into opaque premultiplied `Argb8888` (`[B, G, R, 255]`) pixels (`RUSTYOS-034`).
#[must_use]
pub fn load_png_wallpaper_bgra(path: &Path, dst_w: usize, dst_h: usize) -> Option<Vec<u8>> {
    if dst_w == 0 || dst_h == 0 || dst_w > 4096 || dst_h > 4096 {
        return None;
    }
    let meta = fs::metadata(path).ok()?;
    if usize::try_from(meta.len()).unwrap_or(usize::MAX) > MAX_WALLPAPER_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let (w, h, rgba) =
        decode_png_rgba_limited(&bytes, MAX_WALLPAPER_DIM, MAX_WALLPAPER_INFLATE_BYTES)?;
    scale_cover_rgba_to_premul_bgra(w, h, &rgba, dst_w, dst_h)
}

/// Scales straight `RGBA` pixels into `dst_w x dst_h` opaque `Argb8888` (`[B, G, R, 255]`)
/// using aspect-preserving center-crop ("cover") bilinear sampling.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn scale_cover_rgba_to_premul_bgra(
    src_w: usize,
    src_h: usize,
    rgba: &[u8],
    dst_w: usize,
    dst_h: usize,
) -> Option<Vec<u8>> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return None;
    }
    if rgba.len() != src_w.checked_mul(src_h)?.checked_mul(4)? {
        return None;
    }

    let sw = src_w as f32;
    let sh = src_h as f32;
    let dw = dst_w as f32;
    let dh = dst_h as f32;
    let scale = (dw / sw).max(dh / sh);
    let crop_w = dw / scale;
    let crop_h = dh / scale;
    let origin_col = (sw - crop_w) * 0.5;
    let origin_row = (sh - crop_h) * 0.5;

    let total_bytes = dst_w.checked_mul(dst_h)?.checked_mul(4)?;
    let mut out = vec![0_u8; total_bytes];
    let bg_rgb = [15.0_f32, 10.0_f32, 25.0_f32];
    for dy in 0..dst_h {
        let sample_row = origin_row + ((dy as f32) + 0.5) * crop_h / dh - 0.5;
        for dx in 0..dst_w {
            let sample_col = origin_col + ((dx as f32) + 0.5) * crop_w / dw - 0.5;
            let val = sample_rgba_bilinear_premul(rgba, src_w, src_h, sample_col, sample_row);
            let alpha = (val[3] / 255.0).clamp(0.0, 1.0);
            let inv = (1.0 - alpha).clamp(0.0, 1.0);
            let dst_off = (dy * dst_w + dx) * 4;
            out[dst_off] = (val[2] + bg_rgb[2] * inv).round().clamp(0.0, 255.0) as u8;
            out[dst_off + 1] = (val[1] + bg_rgb[1] * inv).round().clamp(0.0, 255.0) as u8;
            out[dst_off + 2] = (val[0] + bg_rgb[0] * inv).round().clamp(0.0, 255.0) as u8;
            out[dst_off + 3] = 255;
        }
    }
    Some(out)
}

/// Decodes a user avatar PNG (`~/.face`, `AccountsService`) and crops it into an
/// anti-aliased `diameter x diameter` circle with a crisp white glass rim (`RUSTYOS-054`).
#[must_use]
pub fn load_circular_avatar_bgra(path: &Path, diameter: usize) -> Option<Vec<u8>> {
    if diameter == 0 || diameter > 512 {
        return None;
    }
    let meta = fs::metadata(path).ok()?;
    if usize::try_from(meta.len()).unwrap_or(usize::MAX) > MAX_WALLPAPER_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let (w, h, rgba) = decode_png_rgba_limited(&bytes, MAX_ICON_DIM, MAX_INFLATE_BYTES)?;
    let mut bgra = scale_cover_rgba_to_premul_bgra(w, h, &rgba, diameter, diameter)?;
    apply_circular_avatar_mask(&mut bgra, diameter);
    Some(bgra)
}

/// Applies a smooth anti-aliased circular mask and bright glass rim to `BGRA` pixels.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn apply_circular_avatar_mask(bgra: &mut [u8], diameter: usize) {
    if diameter == 0 || bgra.len() < diameter.saturating_mul(diameter).saturating_mul(4) {
        return;
    }
    let center = (diameter as f32 - 1.0) * 0.5;
    let radius = diameter as f32 * 0.5 - 0.5;
    for y in 0..diameter {
        for x in 0..diameter {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = dx.hypot(dy);
            let sd = dist - radius;
            let cov = (0.5 - sd).clamp(0.0, 1.0);
            let rim = (1.0 - (dist - (radius - 1.0)).abs() / 1.1).clamp(0.0, 1.0) * 0.65;
            let off = (y * diameter + x) * 4;
            if cov <= 0.001 {
                bgra[off] = 0;
                bgra[off + 1] = 0;
                bgra[off + 2] = 0;
                bgra[off + 3] = 0;
            } else {
                let b = f32::from(bgra[off]) * (1.0 - rim) + 255.0 * rim;
                let g = f32::from(bgra[off + 1]) * (1.0 - rim) + 250.0 * rim;
                let r = f32::from(bgra[off + 2]) * (1.0 - rim) + 255.0 * rim;
                bgra[off] = (b * cov).round().clamp(0.0, 255.0) as u8;
                bgra[off + 1] = (g * cov).round().clamp(0.0, 255.0) as u8;
                bgra[off + 2] = (r * cov).round().clamp(0.0, 255.0) as u8;
                bgra[off + 3] = (255.0 * cov).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Reads `(width, height)` quickly from PNG `IHDR` header bytes without full decompression.
#[must_use]
pub fn read_png_dimensions_from_bytes(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || data[..8] != PNG_SIGNATURE {
        return None;
    }
    let chunk_len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if chunk_len != 13 || &data[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    (width > 0 && height > 0).then_some((width, height))
}

/// Reads `(width, height)` from a supported image/SVG header for `Get Info` / inspector views.
#[must_use]
pub fn read_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    let mut file = fs::File::open(path).ok()?;
    let mut head = [0_u8; 8192];
    let n = file.read(&mut head).ok()?;
    let slice = &head[..n];
    read_image_dimensions_from_bytes(slice)
}

/// Reads `(width, height)` from header bytes across PNG, PPM, JPEG, WebP, GIF, BMP, and SVG.
#[must_use]
pub fn read_image_dimensions_from_bytes(slice: &[u8]) -> Option<(u32, u32)> {
    read_png_dimensions_from_bytes(slice)
        .or_else(|| read_ppm_dimensions_from_bytes(slice))
        .or_else(|| read_jpeg_dimensions_from_bytes(slice))
        .or_else(|| read_webp_dimensions_from_bytes(slice))
        .or_else(|| read_gif_dimensions_from_bytes(slice))
        .or_else(|| read_bmp_dimensions_from_bytes(slice))
        .or_else(|| read_svg_dimensions_from_bytes(slice))
}

/// Reads `(width, height)` from a JPEG byte slice by scanning SOF markers.
#[must_use]
pub fn read_jpeg_dimensions_from_bytes(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut i = 2_usize;
    while i + 3 < data.len() {
        while i < data.len() && data[i] == 0xFF {
            i += 1;
        }
        if i >= data.len() {
            break;
        }
        let marker = data[i];
        i += 1;
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            continue;
        }
        if i + 1 >= data.len() {
            break;
        }
        let seg_len = usize::from(u16::from_be_bytes([data[i], data[i + 1]]));
        if seg_len < 2 || i + seg_len > data.len() {
            break;
        }
        let is_sof = matches!(
            marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
        );
        if is_sof && seg_len >= 7 {
            let h = u32::from(u16::from_be_bytes([data[i + 3], data[i + 4]]));
            let w = u32::from(u16::from_be_bytes([data[i + 5], data[i + 6]]));
            return (w > 0 && h > 0).then_some((w, h));
        }
        i += seg_len;
    }
    None
}

/// Reads `(width, height)` from a `RIFF....WEBP` (`VP8 `, `VP8L`, or `VP8X`) header.
#[must_use]
pub fn read_webp_dimensions_from_bytes(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 30 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return None;
    }
    match &data[12..16] {
        b"VP8X" => {
            let w = 1
                + (u32::from(data[24]) | (u32::from(data[25]) << 8) | (u32::from(data[26]) << 16));
            let h = 1
                + (u32::from(data[27]) | (u32::from(data[28]) << 8) | (u32::from(data[29]) << 16));
            (w > 0 && h > 0).then_some((w, h))
        }
        b"VP8L" if data[20] == 0x2F => {
            let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            (w > 0 && h > 0).then_some((w, h))
        }
        b"VP8 " if data[23..26] == [0x9D, 0x01, 0x2A] => {
            let w = u32::from(u16::from_le_bytes([data[26], data[27]]) & 0x3FFF);
            let h = u32::from(u16::from_le_bytes([data[28], data[29]]) & 0x3FFF);
            (w > 0 && h > 0).then_some((w, h))
        }
        _ => None,
    }
}

/// Reads `(width, height)` from a `GIF87a` or `GIF89a` header.
#[must_use]
pub fn read_gif_dimensions_from_bytes(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 10 || (&data[0..6] != b"GIF87a" && &data[0..6] != b"GIF89a") {
        return None;
    }
    let w = u32::from(u16::from_le_bytes([data[6], data[7]]));
    let h = u32::from(u16::from_le_bytes([data[8], data[9]]));
    (w > 0 && h > 0).then_some((w, h))
}

/// Reads integer `(width, height)` from an SVG root tag (`width`/`height` or `viewBox`).
#[must_use]
pub fn read_svg_dimensions_from_bytes(data: &[u8]) -> Option<(u32, u32)> {
    let text = std::str::from_utf8(data).ok()?;
    let lower = text.to_ascii_lowercase();
    let svg_start = lower.find("<svg")?;
    let tag_end = lower[svg_start..].find('>')? + svg_start;
    let tag = &text[svg_start..=tag_end];
    let w = extract_xml_attr(tag, "width").and_then(|v| parse_svg_len_u32(&v));
    let h = extract_xml_attr(tag, "height").and_then(|v| parse_svg_len_u32(&v));
    if let (Some(w), Some(h)) = (w, h) {
        return Some((w, h));
    }
    let vb = extract_xml_attr(tag, "viewBox")?;
    let nums: Vec<f64> = vb
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Some((nums[2].round() as u32, nums[3].round() as u32));
    }
    None
}

fn extract_xml_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{}=", attr_name.to_ascii_lowercase());
    let mut offset = 0_usize;
    while let Some(pos) = lower[offset..].find(&needle) {
        let abs = offset + pos;
        let prev_ok = abs == 0
            || tag.as_bytes()[abs - 1].is_ascii_whitespace()
            || tag.as_bytes()[abs - 1] == b'<';
        if prev_ok {
            let after_eq = &tag[abs + needle.len()..];
            let trimmed = after_eq.trim_start();
            let quote = trimmed.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &trimmed[1..];
                let end = inner.find(quote)?;
                return Some(inner[..end].to_owned());
            }
        }
        offset = abs + needle.len();
    }
    None
}

fn parse_svg_len_u32(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.ends_with('%') {
        return None;
    }
    let num_end = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(trimmed.len());
    let val = trimmed[..num_end].parse::<f64>().ok()?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let px = val.round() as u32;
    (px > 0).then_some(px)
}

/// Classification of previewable image, vector, document, and video formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMediaKind {
    /// PNG raster (`image/png`).
    Png,
    /// Portable Pixmap (`image/x-portable-pixmap`).
    Ppm,
    /// Scalable Vector Graphics (`image/svg+xml`).
    Svg,
    /// Portable Document Format (`application/pdf`).
    Pdf,
    /// Video container (`video/*`).
    Video,
    /// Other raster images (`jpeg`, `webp`, `gif`, `bmp`, `tiff`, `avif`, `heic`, `ico`).
    OtherRaster,
}

/// Returns the preview decoder kind for a filename, or `None` if it should fall back to text preview.
#[must_use]
pub fn classify_preview_filename(name: &str) -> Option<PreviewMediaKind> {
    let (_, ext) = name.rsplit_once('.')?;
    match ext.to_ascii_lowercase().as_str() {
        "png" => Some(PreviewMediaKind::Png),
        "ppm" => Some(PreviewMediaKind::Ppm),
        "svg" | "svgz" => Some(PreviewMediaKind::Svg),
        "pdf" => Some(PreviewMediaKind::Pdf),
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "ogv" => Some(PreviewMediaKind::Video),
        "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "avif" | "heic" | "heif"
        | "ico" => Some(PreviewMediaKind::OtherRaster),
        _ => None,
    }
}

/// Returns `true` when `name` has an extension supported by image/SVG/PDF/video previews.
#[must_use]
pub fn is_supported_preview_filename(name: &str) -> bool {
    classify_preview_filename(name).is_some()
}

const PREVIEW_HELPER_TIMEOUT: Duration = Duration::from_secs(4);
static THUMB_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Loads and decodes an image, SVG, PDF first page, or video frame into aspect-fit premultiplied `BGRA`.
#[must_use]
pub fn load_raster_preview_bgra(
    path: &Path,
    max_w: usize,
    max_h: usize,
) -> Option<DecodedPreviewImage> {
    if max_w == 0 || max_h == 0 {
        return None;
    }
    let meta = fs::metadata(path).ok()?;
    let header_kind = read_magic_kind(path);
    let ext_kind = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(classify_preview_filename);
    let kind = header_kind.or(ext_kind)?;

    if matches!(
        kind,
        PreviewMediaKind::Png
            | PreviewMediaKind::Ppm
            | PreviewMediaKind::Svg
            | PreviewMediaKind::OtherRaster
    ) && usize::try_from(meta.len()).unwrap_or(usize::MAX) <= MAX_WALLPAPER_FILE_BYTES
        && let Ok(bytes) = fs::read(path)
    {
        if bytes.starts_with(&PNG_SIGNATURE) {
            return decode_inline_and_scale_png(&bytes, max_w, max_h);
        }
        if bytes.starts_with(b"P6") || bytes.starts_with(b"P3") {
            return decode_inline_and_scale_ppm(&bytes, max_w, max_h);
        }
        if bytes.starts_with(b"BM")
            && let Some((src_w, src_h, rgba)) = decode_bmp_rgba_limited(&bytes, MAX_WALLPAPER_DIM)
        {
            return scale_decoded_rgba(src_w, src_h, &rgba, max_w, max_h);
        }
    }

    let orig_dims = read_image_dimensions(path);
    let (src_w, src_h, rgba) = match kind {
        PreviewMediaKind::Png | PreviewMediaKind::Ppm => return None,
        PreviewMediaKind::Svg => decode_svg_rgba(path, max_w, max_h)?,
        PreviewMediaKind::Pdf => decode_pdf_first_page_rgba(path, max_w.max(max_h))?,
        PreviewMediaKind::Video => decode_video_frame_rgba(path, max_w.max(max_h))?,
        PreviewMediaKind::OtherRaster => decode_other_raster_rgba(path)?,
    };

    let (orig_width, orig_height) =
        orig_dims.unwrap_or((u32::try_from(src_w).ok()?, u32::try_from(src_h).ok()?));
    let (dst_w, dst_h) = fit_aspect_dimensions(src_w, src_h, max_w, max_h)?;
    let bgra_premul = scale_rgba_rect_to_premul_bgra(src_w, src_h, &rgba, dst_w, dst_h)?;
    Some(DecodedPreviewImage {
        orig_width,
        orig_height,
        width: dst_w,
        height: dst_h,
        bgra_premul,
    })
}

fn read_magic_kind(path: &Path) -> Option<PreviewMediaKind> {
    let mut file = fs::File::open(path).ok()?;
    let mut head = [0_u8; 64];
    let n = file.read(&mut head).ok()?;
    let slice = &head[..n];
    if slice.starts_with(&PNG_SIGNATURE) {
        return Some(PreviewMediaKind::Png);
    }
    if slice.starts_with(b"P6") || slice.starts_with(b"P3") {
        return Some(PreviewMediaKind::Ppm);
    }
    if slice.starts_with(b"%PDF-") {
        return Some(PreviewMediaKind::Pdf);
    }
    None
}

fn decode_inline_and_scale_png(
    bytes: &[u8],
    max_w: usize,
    max_h: usize,
) -> Option<DecodedPreviewImage> {
    let (src_w, src_h, rgba) =
        decode_png_rgba_limited(bytes, MAX_WALLPAPER_DIM, MAX_WALLPAPER_INFLATE_BYTES)?;
    scale_decoded_rgba(src_w, src_h, &rgba, max_w, max_h)
}

fn decode_inline_and_scale_ppm(
    bytes: &[u8],
    max_w: usize,
    max_h: usize,
) -> Option<DecodedPreviewImage> {
    let (src_w, src_h, rgba) = decode_ppm_rgba_limited(bytes, MAX_WALLPAPER_DIM)?;
    scale_decoded_rgba(src_w, src_h, &rgba, max_w, max_h)
}

fn scale_decoded_rgba(
    src_w: usize,
    src_h: usize,
    rgba: &[u8],
    max_w: usize,
    max_h: usize,
) -> Option<DecodedPreviewImage> {
    let (dst_w, dst_h) = fit_aspect_dimensions(src_w, src_h, max_w, max_h)?;
    let bgra_premul = scale_rgba_rect_to_premul_bgra(src_w, src_h, rgba, dst_w, dst_h)?;
    Some(DecodedPreviewImage {
        orig_width: u32::try_from(src_w).ok()?,
        orig_height: u32::try_from(src_h).ok()?,
        width: dst_w,
        height: dst_h,
        bgra_premul,
    })
}

fn run_png_stdout_decoder(cmd: &mut Command) -> Option<(usize, usize, Vec<u8>)> {
    let out = crate::run_command_bounded(cmd, PREVIEW_HELPER_TIMEOUT).ok()?;
    if !out.status.success() || out.stdout.len() < PNG_SIGNATURE.len() {
        return None;
    }
    decode_png_rgba_limited(&out.stdout, MAX_WALLPAPER_DIM, MAX_WALLPAPER_INFLATE_BYTES)
}

fn run_ppm_stdout_decoder(cmd: &mut Command) -> Option<(usize, usize, Vec<u8>)> {
    let out = crate::run_command_bounded(cmd, PREVIEW_HELPER_TIMEOUT).ok()?;
    if !out.status.success() || out.stdout.len() < 4 {
        return None;
    }
    decode_ppm_rgba_limited(&out.stdout, MAX_WALLPAPER_DIM)
}

fn run_ffmpeg_png_frame(path: &Path, seek_secs: Option<&str>) -> Option<(usize, usize, Vec<u8>)> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-v", "error"]);
    if let Some(ss) = seek_secs {
        cmd.args(["-ss", ss]);
    }
    cmd.arg("-i").arg(path).args([
        "-frames:v",
        "1",
        "-f",
        "image2pipe",
        "-vcodec",
        "png",
        "pipe:1",
    ]);
    run_png_stdout_decoder(&mut cmd)
}

fn decode_svg_rgba(path: &Path, max_w: usize, max_h: usize) -> Option<(usize, usize, Vec<u8>)> {
    let mut cmd = Command::new("rsvg-convert");
    cmd.args([
        "-w",
        &max_w.clamp(32, 2048).to_string(),
        "-h",
        &max_h.clamp(32, 2048).to_string(),
        "--keep-aspect-ratio",
        "-f",
        "png",
        "-o",
        "-",
    ])
    .arg(path);
    run_png_stdout_decoder(&mut cmd).or_else(|| run_ffmpeg_png_frame(path, None))
}

fn decode_pdf_first_page_rgba(path: &Path, target_dim: usize) -> Option<(usize, usize, Vec<u8>)> {
    let seq = THUMB_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let prefix = PathBuf::from(format!(
        "/tmp/rustyos-pdf-thumb-{}-{seq}",
        std::process::id()
    ));
    let png_path = PathBuf::from(format!("{}.png", prefix.display()));
    let mut pdftoppm_cmd = Command::new("pdftoppm");
    pdftoppm_cmd
        .args([
            "-png",
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            "-scale-to",
            &target_dim.clamp(64, 2048).to_string(),
        ])
        .arg(path)
        .arg(&prefix);
    let pdftoppm_res = crate::run_command_bounded(&mut pdftoppm_cmd, PREVIEW_HELPER_TIMEOUT)
        .ok()
        .filter(|o| o.status.success())
        .and_then(|_| fs::read(&png_path).ok())
        .and_then(|bytes| {
            decode_png_rgba_limited(&bytes, MAX_WALLPAPER_DIM, MAX_WALLPAPER_INFLATE_BYTES)
        });
    let _ = fs::remove_file(&png_path);
    if pdftoppm_res.is_some() {
        return pdftoppm_res;
    }

    let mut mutool_cmd = Command::new("mutool");
    mutool_cmd
        .args(["draw", "-F", "png", "-o", "-", "-r", "144"])
        .arg(path)
        .arg("1");
    run_png_stdout_decoder(&mut mutool_cmd)
}

fn decode_video_frame_rgba(path: &Path, target_dim: usize) -> Option<(usize, usize, Vec<u8>)> {
    let mut tn_cmd = Command::new("ffmpegthumbnailer");
    tn_cmd.arg("-i").arg(path).args([
        "-o",
        "-",
        "-c",
        "png",
        "-s",
        &target_dim.clamp(64, 1024).to_string(),
        "-f",
    ]);
    run_png_stdout_decoder(&mut tn_cmd)
        .or_else(|| run_ffmpeg_png_frame(path, Some("1")))
        .or_else(|| run_ffmpeg_png_frame(path, None))
}

fn decode_other_raster_rgba(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(ext.as_deref(), Some("jpg" | "jpeg")) {
        let mut djpeg = Command::new("djpeg");
        djpeg.arg("-rgb").arg(path);
        if let Some(decoded) = run_ppm_stdout_decoder(&mut djpeg) {
            return Some(decoded);
        }
    } else if ext.as_deref() == Some("webp") {
        let mut dwebp = Command::new("dwebp");
        dwebp.args(["-ppm"]).arg(path).args(["-o", "-"]);
        if let Some(decoded) = run_ppm_stdout_decoder(&mut dwebp) {
            return Some(decoded);
        }
    }
    run_ffmpeg_png_frame(path, None)
}

/// Decodes a standard non-interlaced PNG byte buffer into `(width, height, straight_rgba)`.
#[must_use]
pub fn decode_png_rgba(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    decode_png_rgba_limited(data, MAX_ICON_DIM, MAX_INFLATE_BYTES)
}

/// Decodes a standard non-interlaced PNG byte buffer with explicit dimension and inflate bounds.
#[must_use]
pub fn decode_png_rgba_limited(
    data: &[u8],
    max_dim: u32,
    max_inflate_bytes: usize,
) -> Option<(usize, usize, Vec<u8>)> {
    let parsed = parse_png_chunks(data, max_dim)?;
    let channels: usize = match parsed.color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    if !matches!(parsed.bit_depth, 1 | 2 | 4 | 8 | 16) {
        return None;
    }

    let w = usize::try_from(parsed.width).ok()?;
    let h = usize::try_from(parsed.height).ok()?;
    let bits_per_pixel = channels.checked_mul(usize::from(parsed.bit_depth))?;
    let stride = w.checked_mul(bits_per_pixel)?.div_ceil(8);
    let bpp = bits_per_pixel.div_ceil(8);
    let expected_raw = h.checked_mul(stride.checked_add(1)?)?;

    let raw = zlib_decompress(&parsed.idat, expected_raw, max_inflate_bytes)?;
    if raw.len() < expected_raw {
        return None;
    }

    let unfiltered = unfilter_scanlines(&raw[..expected_raw], h, stride, bpp)?;
    let rgba = expand_samples_to_rgba(
        &unfiltered,
        w,
        h,
        parsed.bit_depth,
        parsed.color_type,
        &parsed.palette,
        &parsed.trns,
    )?;
    Some((w, h, rgba))
}

struct ParsedPngChunks {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    palette: Vec<u8>,
    trns: Vec<u8>,
    idat: Vec<u8>,
}

fn parse_png_chunks(data: &[u8], max_dim: u32) -> Option<ParsedPngChunks> {
    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return None;
    }

    let mut pos = 8;
    let mut width = 0_u32;
    let mut height = 0_u32;
    let mut bit_depth = 0_u8;
    let mut color_type = 0_u8;
    let mut ihdr_seen = false;
    let mut palette = Vec::new();
    let mut trns = Vec::new();
    let mut idat = Vec::new();

    while pos + 12 <= data.len() {
        let len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        let chunk_type = &data[pos..pos + 4];
        pos += 4;
        if pos.checked_add(len)?.checked_add(4)? > data.len() {
            return None;
        }
        let payload = &data[pos..pos + len];
        pos += len + 4;

        match chunk_type {
            b"IHDR" => {
                if payload.len() != 13 {
                    return None;
                }
                width = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                height = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                bit_depth = payload[8];
                color_type = payload[9];
                if width == 0
                    || height == 0
                    || width > max_dim
                    || height > max_dim
                    || payload[10] != 0
                    || payload[11] != 0
                    || payload[12] != 0
                {
                    return None;
                }
                ihdr_seen = true;
            }
            b"PLTE" => palette.extend_from_slice(payload),
            b"tRNS" => trns.extend_from_slice(payload),
            b"IDAT" => idat.extend_from_slice(payload),
            b"IEND" => break,
            _ => {}
        }
    }

    if !ihdr_seen || idat.is_empty() {
        return None;
    }
    Some(ParsedPngChunks {
        width,
        height,
        bit_depth,
        color_type,
        palette,
        trns,
        idat,
    })
}

/// Parses `(width, height)` from a `P3`/`P6` Netpbm PPM header.
#[must_use]
pub fn read_ppm_dimensions_from_bytes(data: &[u8]) -> Option<(u32, u32)> {
    let mut pos = 0;
    let magic = next_ppm_token(data, &mut pos)?;
    if magic != b"P3" && magic != b"P6" {
        return None;
    }
    let w = parse_ascii_u32(next_ppm_token(data, &mut pos)?)?;
    let h = parse_ascii_u32(next_ppm_token(data, &mut pos)?)?;
    (w > 0 && h > 0).then_some((w, h))
}

/// Decodes a `P3` ASCII or `P6` binary Netpbm PPM image into `(width, height, straight_rgba)`.
#[must_use]
pub fn decode_ppm_rgba_limited(data: &[u8], max_dim: u32) -> Option<(usize, usize, Vec<u8>)> {
    let mut pos = 0;
    let magic = next_ppm_token(data, &mut pos)?;
    let is_binary = match magic {
        b"P6" => true,
        b"P3" => false,
        _ => return None,
    };
    let width = parse_ascii_u32(next_ppm_token(data, &mut pos)?)?;
    let height = parse_ascii_u32(next_ppm_token(data, &mut pos)?)?;
    let maxval = parse_ascii_u32(next_ppm_token(data, &mut pos)?)?;
    if width == 0
        || height == 0
        || width > max_dim
        || height > max_dim
        || maxval == 0
        || maxval > 255
    {
        return None;
    }

    let ppm_w = usize::try_from(width).ok()?;
    let ppm_h = usize::try_from(height).ok()?;
    let pixel_count = ppm_w.checked_mul(ppm_h)?;
    let mut rgba = vec![255_u8; pixel_count.checked_mul(4)?];

    if is_binary {
        let ws = *data.get(pos)?;
        if !ws.is_ascii_whitespace() {
            return None;
        }
        pos += 1;
        let needed = pixel_count.checked_mul(3)?;
        let rgb = data.get(pos..pos.checked_add(needed)?)?;
        for idx in 0..pixel_count {
            let src = idx * 3;
            let dst = idx * 4;
            rgba[dst] = scale_ppm_channel(rgb[src], maxval);
            rgba[dst + 1] = scale_ppm_channel(rgb[src + 1], maxval);
            rgba[dst + 2] = scale_ppm_channel(rgb[src + 2], maxval);
        }
    } else {
        for idx in 0..pixel_count {
            let red = parse_ascii_u32(next_ppm_token(data, &mut pos)?)?;
            let green = parse_ascii_u32(next_ppm_token(data, &mut pos)?)?;
            let blue = parse_ascii_u32(next_ppm_token(data, &mut pos)?)?;
            if red > maxval || green > maxval || blue > maxval {
                return None;
            }
            let dst = idx * 4;
            rgba[dst] = scale_ppm_channel(u8::try_from(red).ok()?, maxval);
            rgba[dst + 1] = scale_ppm_channel(u8::try_from(green).ok()?, maxval);
            rgba[dst + 2] = scale_ppm_channel(u8::try_from(blue).ok()?, maxval);
        }
    }

    Some((ppm_w, ppm_h, rgba))
}

fn scale_ppm_channel(v: u8, maxval: u32) -> u8 {
    if maxval == 255 {
        return v;
    }
    let scaled = (u32::from(v) * 255) / maxval;
    u8::try_from(scaled.min(255)).unwrap_or(255)
}

fn next_ppm_token<'a>(data: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    while *pos < data.len() {
        let b = data[*pos];
        if b.is_ascii_whitespace() {
            *pos += 1;
        } else if b == b'#' {
            while *pos < data.len() && data[*pos] != b'\n' {
                *pos += 1;
            }
        } else {
            break;
        }
    }
    let start = *pos;
    while *pos < data.len() && !data[*pos].is_ascii_whitespace() && data[*pos] != b'#' {
        *pos += 1;
    }
    (start < *pos).then_some(&data[start..*pos])
}

fn parse_ascii_u32(bytes: &[u8]) -> Option<u32> {
    std::str::from_utf8(bytes).ok()?.parse::<u32>().ok()
}

fn unfilter_scanlines(raw: &[u8], height: usize, stride: usize, bpp: usize) -> Option<Vec<u8>> {
    let mut out = vec![0_u8; height * stride];
    for row in 0..height {
        let filter = raw[row * (stride + 1)];
        let src = &raw[row * (stride + 1) + 1..(row + 1) * (stride + 1)];
        let row_start = row * stride;
        let prev_start = row.saturating_sub(1) * stride;

        for i in 0..stride {
            let x = src[i];
            let a = if i >= bpp {
                out[row_start + i - bpp]
            } else {
                0
            };
            let b = if row > 0 { out[prev_start + i] } else { 0 };
            let c = if row > 0 && i >= bpp {
                out[prev_start + i - bpp]
            } else {
                0
            };

            let recon = match filter {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => {
                    let avg = u8::try_from(u16::midpoint(u16::from(a), u16::from(b))).ok()?;
                    x.wrapping_add(avg)
                }
                4 => x.wrapping_add(paeth_predictor(a, b, c)),
                _ => return None,
            };
            out[row_start + i] = recon;
        }
    }
    Some(out)
}

const fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let ia = a as i32;
    let ib = b as i32;
    let ic = c as i32;
    let p = ia + ib - ic;
    let pa = (p - ia).abs();
    let pb = (p - ib).abs();
    let pc = (p - ic).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn expand_samples_to_rgba(
    data: &[u8],
    w: usize,
    h: usize,
    bit_depth: u8,
    color_type: u8,
    palette: &[u8],
    trns: &[u8],
) -> Option<Vec<u8>> {
    let mut out = vec![0_u8; w.checked_mul(h)?.checked_mul(4)?];
    let channels: usize = match color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    let stride = w
        .checked_mul(channels * usize::from(bit_depth))?
        .div_ceil(8);

    for y in 0..h {
        let row = &data[y * stride..(y + 1) * stride];
        for x in 0..w {
            let dst = (y * w + x) * 4;
            let px = decode_sample_pixel(row, x, color_type, bit_depth, palette, trns)?;
            out[dst..dst + 4].copy_from_slice(&px);
        }
    }
    Some(out)
}

fn decode_sample_pixel(
    row: &[u8],
    x: usize,
    color_type: u8,
    bit_depth: u8,
    palette: &[u8],
    trns: &[u8],
) -> Option<[u8; 4]> {
    match (color_type, bit_depth) {
        (6, 8) => {
            let off = x * 4;
            Some([row[off], row[off + 1], row[off + 2], row[off + 3]])
        }
        (6, 16) => {
            let off = x * 8;
            Some([row[off], row[off + 2], row[off + 4], row[off + 6]])
        }
        (2, 8) => {
            let off = x * 3;
            Some([row[off], row[off + 1], row[off + 2], 255])
        }
        (2, 16) => {
            let off = x * 6;
            Some([row[off], row[off + 2], row[off + 4], 255])
        }
        (4, 8) => {
            let g = row[x * 2];
            Some([g, g, g, row[x * 2 + 1]])
        }
        (4, 16) => {
            let g = row[x * 4];
            Some([g, g, g, row[x * 4 + 2]])
        }
        (3, depth @ (1 | 2 | 4 | 8)) => {
            let idx = usize::from(read_subbyte_sample(row, x, depth)?);
            let poff = idx.checked_mul(3)?;
            if poff + 3 > palette.len() {
                return None;
            }
            Some([
                palette[poff],
                palette[poff + 1],
                palette[poff + 2],
                trns.get(idx).copied().unwrap_or(255),
            ])
        }
        (0, depth @ (1 | 2 | 4 | 8)) => {
            let raw_v = read_subbyte_sample(row, x, depth)?;
            let max_v = ((1_u16 << depth) - 1).max(1);
            let gray = u8::try_from((u16::from(raw_v) * 255) / max_v).ok()?;
            Some([gray, gray, gray, 255])
        }
        (0, 16) => {
            let gray = row[x * 2];
            Some([gray, gray, gray, 255])
        }
        _ => None,
    }
}

fn read_subbyte_sample(row: &[u8], x: usize, depth: u8) -> Option<u8> {
    if depth == 8 {
        return row.get(x).copied();
    }
    let samples_per_byte = 8 / usize::from(depth);
    let byte = *row.get(x / samples_per_byte)?;
    let shift = (samples_per_byte - 1 - (x % samples_per_byte)) * usize::from(depth);
    let mask = u8::try_from((1_u16 << depth) - 1).ok()?;
    Some((byte >> shift) & mask)
}

// --- Pure-Rust Zlib + DEFLATE (RFC 1951) Decompressor ---

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_buf: u32,
    bit_count: u8,
}

impl<'a> BitReader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_buf: 0,
            bit_count: 0,
        }
    }

    fn ensure_bits(&mut self, n: u8) -> bool {
        while self.bit_count < n {
            let Some(&next) = self.data.get(self.byte_pos) else {
                return false;
            };
            self.byte_pos += 1;
            self.bit_buf |= u32::from(next) << self.bit_count;
            self.bit_count += 8;
        }
        true
    }

    fn read_bits(&mut self, n: u8) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        if !self.ensure_bits(n) {
            return None;
        }
        let val = self.bit_buf & ((1_u32 << n) - 1);
        self.bit_buf >>= n;
        self.bit_count -= n;
        Some(val)
    }

    fn align_byte(&mut self) {
        let discard = self.bit_count % 8;
        self.bit_buf >>= discard;
        self.bit_count -= discard;
    }

    fn read_u16_aligned(&mut self) -> Option<u16> {
        let lo = u16::try_from(self.read_bits(8)?).ok()?;
        let hi = u16::try_from(self.read_bits(8)?).ok()?;
        Some(lo | (hi << 8))
    }

    fn copy_aligned_bytes(&mut self, len: usize, out: &mut Vec<u8>) -> Option<()> {
        self.align_byte();
        let buffered_bytes = usize::from(self.bit_count / 8);
        self.byte_pos = self.byte_pos.checked_sub(buffered_bytes)?;
        self.bit_buf = 0;
        self.bit_count = 0;
        let slice = self.data.get(self.byte_pos..self.byte_pos + len)?;
        out.extend_from_slice(slice);
        self.byte_pos += len;
        Some(())
    }
}

struct HuffTable {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl HuffTable {
    fn from_lengths(lengths: &[u8]) -> Option<Self> {
        let mut counts = [0_u16; 16];
        for &len in lengths {
            if usize::from(len) >= 16 {
                return None;
            }
            if len > 0 {
                counts[usize::from(len)] = counts[usize::from(len)].checked_add(1)?;
            }
        }
        let mut offsets = [0_u16; 16];
        for bits in 1..15 {
            offsets[bits + 1] = offsets[bits].checked_add(counts[bits])?;
        }
        let mut symbols = vec![0_u16; lengths.len()];
        for (sym, &len) in lengths.iter().enumerate() {
            if len != 0 {
                let l = usize::from(len);
                let dst = usize::from(offsets[l]);
                offsets[l] += 1;
                symbols[dst] = u16::try_from(sym).ok()?;
            }
        }
        Some(Self { counts, symbols })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Option<u16> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: usize = 0;
        for len in 1..=15 {
            let bit = i32::try_from(reader.read_bits(1)?).ok()?;
            code |= bit;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                let sym_idx = index + usize::try_from(code - first).ok()?;
                return self.symbols.get(sym_idx).copied();
            }
            index += usize::from(self.counts[len]);
            first += count;
            first <<= 1;
            code <<= 1;
        }
        None
    }
}

fn fixed_huffman_tables() -> Option<(HuffTable, HuffTable)> {
    let mut litlen = [0_u8; 288];
    litlen[..144].fill(8);
    litlen[144..256].fill(9);
    litlen[256..280].fill(7);
    litlen[280..288].fill(8);
    let dist = [5_u8; 32];
    Some((
        HuffTable::from_lengths(&litlen)?,
        HuffTable::from_lengths(&dist)?,
    ))
}

fn dynamic_huffman_tables(reader: &mut BitReader<'_>) -> Option<(HuffTable, HuffTable)> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let hlit = usize::try_from(reader.read_bits(5)?).ok()? + 257;
    let hdist = usize::try_from(reader.read_bits(5)?).ok()? + 1;
    let hclen = usize::try_from(reader.read_bits(4)?).ok()? + 4;
    if hlit > 286 || hdist > 30 || hclen > 19 {
        return None;
    }

    let mut cl_lengths = [0_u8; 19];
    for i in 0..hclen {
        cl_lengths[ORDER[i]] = u8::try_from(reader.read_bits(3)?).ok()?;
    }
    let cl_table = HuffTable::from_lengths(&cl_lengths)?;

    let total = hlit + hdist;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        let sym = cl_table.decode(reader)?;
        match sym {
            0..=15 => lengths.push(u8::try_from(sym).ok()?),
            16 => {
                let prev = *lengths.last()?;
                let rep = usize::try_from(reader.read_bits(2)?).ok()? + 3;
                for _ in 0..rep {
                    lengths.push(prev);
                }
            }
            17 => {
                let rep = usize::try_from(reader.read_bits(3)?).ok()? + 3;
                lengths.resize(lengths.len() + rep, 0);
            }
            18 => {
                let rep = usize::try_from(reader.read_bits(7)?).ok()? + 11;
                lengths.resize(lengths.len() + rep, 0);
            }
            _ => return None,
        }
    }
    if lengths.len() != total {
        return None;
    }
    Some((
        HuffTable::from_lengths(&lengths[..hlit])?,
        HuffTable::from_lengths(&lengths[hlit..])?,
    ))
}

fn decode_huffman_block(
    reader: &mut BitReader<'_>,
    litlen: &HuffTable,
    dist: &HuffTable,
    out: &mut Vec<u8>,
    max_inflate_bytes: usize,
) -> Option<()> {
    const LEN_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];

    loop {
        let sym = usize::from(litlen.decode(reader)?);
        match sym.cmp(&256) {
            std::cmp::Ordering::Less => {
                if out.len() >= max_inflate_bytes {
                    return None;
                }
                out.push(u8::try_from(sym).ok()?);
            }
            std::cmp::Ordering::Equal => {
                return Some(());
            }
            std::cmp::Ordering::Greater => {
                let l_idx = sym.checked_sub(257)?;
                if l_idx >= LEN_BASE.len() {
                    return None;
                }
                let length =
                    LEN_BASE[l_idx] + usize::try_from(reader.read_bits(LEN_EXTRA[l_idx])?).ok()?;
                let d_sym = usize::from(dist.decode(reader)?);
                if d_sym >= DIST_BASE.len() {
                    return None;
                }
                let distance = DIST_BASE[d_sym]
                    + usize::try_from(reader.read_bits(DIST_EXTRA[d_sym])?).ok()?;
                if distance == 0 || distance > out.len() || out.len() + length > max_inflate_bytes {
                    return None;
                }
                for _ in 0..length {
                    let b = out[out.len() - distance];
                    out.push(b);
                }
            }
        }
    }
}

fn zlib_decompress(data: &[u8], expected_hint: usize, max_inflate_bytes: usize) -> Option<Vec<u8>> {
    if data.len() < 6 {
        return None;
    }
    let cmf = data[0];
    let flg = data[1];
    if (cmf & 0x0f) != 8 || (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 || (flg & 0x20) != 0 {
        return None;
    }

    let mut reader = BitReader::new(&data[2..]);
    let mut out = Vec::with_capacity(expected_hint.min(max_inflate_bytes));

    loop {
        let bfinal = reader.read_bits(1)?;
        let btype = reader.read_bits(2)?;
        match btype {
            0 => {
                reader.align_byte();
                let buffered_bytes = usize::from(reader.bit_count / 8);
                reader.byte_pos = reader.byte_pos.checked_sub(buffered_bytes)?;
                reader.bit_buf = 0;
                reader.bit_count = 0;
                let len = reader.read_u16_aligned()?;
                let nlen = reader.read_u16_aligned()?;
                if len != !nlen || out.len() + usize::from(len) > max_inflate_bytes {
                    return None;
                }
                reader.copy_aligned_bytes(usize::from(len), &mut out)?;
            }
            1 => {
                let (litlen, dist) = fixed_huffman_tables()?;
                decode_huffman_block(&mut reader, &litlen, &dist, &mut out, max_inflate_bytes)?;
            }
            2 => {
                let (litlen, dist) = dynamic_huffman_tables(&mut reader)?;
                decode_huffman_block(&mut reader, &litlen, &dist, &mut out, max_inflate_bytes)?;
            }
            _ => return None,
        }
        if bfinal == 1 {
            break;
        }
    }

    Some(out)
}

/// Parses `(width, height)` from an uncompressed or bitfield BMP header.
#[must_use]
pub fn read_bmp_dimensions_from_bytes(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 26 || &data[0..2] != b"BM" {
        return None;
    }
    let dib_header_size = u32::from_le_bytes(data.get(14..18)?.try_into().ok()?);
    if dib_header_size < 12 {
        return None;
    }
    let (w, h) = if dib_header_size == 12 {
        let w = u16::from_le_bytes(data.get(18..20)?.try_into().ok()?);
        let h = u16::from_le_bytes(data.get(20..22)?.try_into().ok()?);
        (u32::from(w), u32::from(h))
    } else {
        let w_raw = i32::from_le_bytes(data.get(18..22)?.try_into().ok()?);
        let h_raw = i32::from_le_bytes(data.get(22..26)?.try_into().ok()?);
        (w_raw.unsigned_abs(), h_raw.unsigned_abs())
    };
    (w > 0 && h > 0).then_some((w, h))
}

/// Decodes an uncompressed or bitfield 8-bit paletted, 24-bit RGB, or 32-bit RGBA BMP image into `(width, height, straight_rgba)`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn decode_bmp_rgba_limited(data: &[u8], max_dim: u32) -> Option<(usize, usize, Vec<u8>)> {
    if data.len() < 26 || &data[0..2] != b"BM" {
        return None;
    }
    let pixel_offset =
        usize::try_from(u32::from_le_bytes(data.get(10..14)?.try_into().ok()?)).ok()?;
    let dib_size = usize::try_from(u32::from_le_bytes(data.get(14..18)?.try_into().ok()?)).ok()?;
    if dib_size < 12 {
        return None;
    }

    let (width, height, top_down, bpp, compression) = if dib_size == 12 {
        let w = u32::from(u16::from_le_bytes(data.get(18..20)?.try_into().ok()?));
        let h = u32::from(u16::from_le_bytes(data.get(20..22)?.try_into().ok()?));
        let planes = u16::from_le_bytes(data.get(22..24)?.try_into().ok()?);
        let bpp = u16::from_le_bytes(data.get(24..26)?.try_into().ok()?);
        if planes != 1 {
            return None;
        }
        (w, h, false, bpp, 0)
    } else {
        let w_raw = i32::from_le_bytes(data.get(18..22)?.try_into().ok()?);
        let h_raw = i32::from_le_bytes(data.get(22..26)?.try_into().ok()?);
        let planes = u16::from_le_bytes(data.get(26..28)?.try_into().ok()?);
        let bpp = u16::from_le_bytes(data.get(28..30)?.try_into().ok()?);
        let comp = if data.len() >= 34 {
            u32::from_le_bytes(data.get(30..34)?.try_into().ok()?)
        } else {
            0
        };
        if planes != 1 {
            return None;
        }
        (
            w_raw.unsigned_abs(),
            h_raw.unsigned_abs(),
            h_raw < 0,
            bpp,
            comp,
        )
    };

    if width == 0 || height == 0 || width > max_dim || height > max_dim {
        return None;
    }
    if compression != 0 && compression != 3 {
        return None;
    }

    let w_usize = usize::try_from(width).ok()?;
    let h_usize = usize::try_from(height).ok()?;
    let pixel_count = w_usize.checked_mul(h_usize)?;
    let mut rgba = vec![255_u8; pixel_count.checked_mul(4)?];

    match bpp {
        24 => {
            let row_stride = ((w_usize.checked_mul(3)?.checked_add(3)?) / 4) * 4;
            for y in 0..h_usize {
                let target_y = if top_down { y } else { h_usize - 1 - y };
                let row_start = pixel_offset.checked_add(y.checked_mul(row_stride)?)?;
                let row_bytes = data.get(row_start..row_start.checked_add(w_usize * 3)?)?;
                for x in 0..w_usize {
                    let src_idx = x * 3;
                    let dst_idx = (target_y * w_usize + x) * 4;
                    let b = row_bytes[src_idx];
                    let g = row_bytes[src_idx + 1];
                    let r = row_bytes[src_idx + 2];
                    rgba[dst_idx] = r;
                    rgba[dst_idx + 1] = g;
                    rgba[dst_idx + 2] = b;
                    rgba[dst_idx + 3] = 255;
                }
            }
        }
        32 => {
            let row_stride = w_usize.checked_mul(4)?;
            let mut has_non_zero_alpha = false;
            for y in 0..h_usize {
                let target_y = if top_down { y } else { h_usize - 1 - y };
                let row_start = pixel_offset.checked_add(y.checked_mul(row_stride)?)?;
                let row_bytes = data.get(row_start..row_start.checked_add(row_stride)?)?;
                for x in 0..w_usize {
                    let src_idx = x * 4;
                    let dst_idx = (target_y * w_usize + x) * 4;
                    let b = row_bytes[src_idx];
                    let g = row_bytes[src_idx + 1];
                    let r = row_bytes[src_idx + 2];
                    let a = row_bytes[src_idx + 3];
                    if a > 0 {
                        has_non_zero_alpha = true;
                    }
                    rgba[dst_idx] = r;
                    rgba[dst_idx + 1] = g;
                    rgba[dst_idx + 2] = b;
                    rgba[dst_idx + 3] = a;
                }
            }
            if !has_non_zero_alpha {
                for chunk in rgba.as_chunks_mut::<4>().0 {
                    chunk[3] = 255;
                }
            }
        }
        8 => {
            let palette_offset = 14 + dib_size;
            let palette_bytes = data.get(palette_offset..pixel_offset)?;
            if palette_bytes.len() < 4 {
                return None;
            }
            let palette_entries = palette_bytes.len() / 4;
            let row_stride = ((w_usize.checked_add(3)?) / 4) * 4;
            for y in 0..h_usize {
                let target_y = if top_down { y } else { h_usize - 1 - y };
                let row_start = pixel_offset.checked_add(y.checked_mul(row_stride)?)?;
                let row_bytes = data.get(row_start..row_start.checked_add(w_usize)?)?;
                for (x, &b_byte) in row_bytes.iter().take(w_usize).enumerate() {
                    let color_idx = usize::from(b_byte);
                    let dst_idx = (target_y * w_usize + x) * 4;
                    if color_idx < palette_entries {
                        let pal_start = color_idx * 4;
                        let b = palette_bytes[pal_start];
                        let g = palette_bytes[pal_start + 1];
                        let r = palette_bytes[pal_start + 2];
                        rgba[dst_idx] = r;
                        rgba[dst_idx + 1] = g;
                        rgba[dst_idx + 2] = b;
                        rgba[dst_idx + 3] = 255;
                    }
                }
            }
        }
        _ => return None,
    }

    Some((w_usize, h_usize, rgba))
}

/// Decodes an image of any supported raster format (PNG, BMP, PPM) into `(width, height, straight_rgba)`.
#[must_use]
pub fn decode_image_rgba_limited(data: &[u8], max_dim: u32) -> Option<(usize, usize, Vec<u8>)> {
    if data.starts_with(&PNG_SIGNATURE) {
        decode_png_rgba_limited(data, max_dim, MAX_WALLPAPER_INFLATE_BYTES)
    } else if data.starts_with(b"BM") {
        decode_bmp_rgba_limited(data, max_dim)
    } else if data.starts_with(b"P3") || data.starts_with(b"P6") {
        decode_ppm_rgba_limited(data, max_dim)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFF_u32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                let mask = if (crc & 1) != 0 { 0xEDB8_8320 } else { 0 };
                crc = (crc >> 1) ^ mask;
            }
        }
        !crc
    }

    fn adler32(bytes: &[u8]) -> u32 {
        let mut s1 = 1_u32;
        let mut s2 = 0_u32;
        for &b in bytes {
            s1 = (s1 + u32::from(b)) % 65_521;
            s2 = (s2 + s1) % 65_521;
        }
        (s2 << 16) | s1
    }

    fn build_uncompressed_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut out = Vec::from(PNG_SIGNATURE);
        let push_chunk = |dst: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]| {
            let len = u32::try_from(body.len()).unwrap_or(0);
            dst.extend_from_slice(&len.to_be_bytes());
            dst.extend_from_slice(tag);
            dst.extend_from_slice(body);
            let mut crc_in = Vec::with_capacity(4 + body.len());
            crc_in.extend_from_slice(tag);
            crc_in.extend_from_slice(body);
            dst.extend_from_slice(&crc32(&crc_in).to_be_bytes());
        };

        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        push_chunk(&mut out, b"IHDR", &ihdr);

        let w = width as usize;
        let h = height as usize;
        let mut raw = Vec::with_capacity(h * (1 + w * 4));
        for y in 0..h {
            raw.push(0);
            raw.extend_from_slice(&rgba[y * w * 4..(y + 1) * w * 4]);
        }
        let len = u16::try_from(raw.len()).unwrap_or(0);
        let mut zlib = vec![0x78, 0x01, 0x01];
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(&raw);
        zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
        push_chunk(&mut out, b"IDAT", &zlib);
        push_chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn png_header_and_aspect_fit_decode_work() -> Result<(), &'static str> {
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, //
            0, 0, 255, 255, 255, 255, 255, 255, //
        ];
        let png = build_uncompressed_png(4, 1, &rgba);
        assert_eq!(read_png_dimensions_from_bytes(&png), Some((4, 1)));
        let (w, h, dec) = decode_png_rgba(&png).ok_or("png decode failed")?;
        assert_eq!((w, h), (4, 1));
        assert_eq!(dec, rgba);

        assert_eq!(fit_aspect_dimensions(800, 400, 200, 200), Some((200, 100)));
        assert_eq!(fit_aspect_dimensions(100, 80, 200, 200), Some((100, 80)));

        let bgra = scale_rgba_rect_to_premul_bgra(4, 1, &dec, 2, 1).ok_or("scale failed")?;
        assert_eq!(bgra.len(), 8);
        Ok(())
    }

    #[test]
    fn ppm_p6_and_p3_decoding_and_bounds_work() -> Result<(), &'static str> {
        let p6 = b"P6\n# test\n2 1\n255\n\x10\x20\x30\xff\x80\x00";
        assert_eq!(read_ppm_dimensions_from_bytes(p6), Some((2, 1)));
        let (w, h, rgba) = decode_ppm_rgba_limited(p6, 64).ok_or("p6 decode failed")?;
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, vec![0x10, 0x20, 0x30, 255, 0xff, 0x80, 0x00, 255]);

        let p3 = b"P3\n1 1\n255\n12 34 56\n";
        let (w3, h3, rgba3) = decode_ppm_rgba_limited(p3, 64).ok_or("p3 decode failed")?;
        assert_eq!((w3, h3), (1, 1));
        assert_eq!(rgba3, vec![12, 34, 56, 255]);
        Ok(())
    }

    #[test]
    fn lanczos_linear_downscale_preserves_color_alpha_and_softens_edges() -> Result<(), String> {
        // Solid opaque colour stays exact after 8x8 -> 3x3 downscale (`[B, G, R, A]`).
        let solid: Vec<u8> = (0..64).flat_map(|_| [200_u8, 100, 50, 255]).collect();
        let out = scale_rgba_to_premul_bgra(8, 8, &solid, 3).ok_or("solid downscale failed")?;
        for px in out.as_chunks::<4>().0 {
            assert!(i32::from(px[0]).abs_diff(50) <= 1, "{px:?}");
            assert!(i32::from(px[1]).abs_diff(100) <= 1, "{px:?}");
            assert!(i32::from(px[2]).abs_diff(200) <= 1, "{px:?}");
            assert_eq!(px[3], 255);
        }

        // Left half opaque white / right half fully transparent: 16x16 -> 5x5 yields smooth coverage
        // ramp (intermediate alpha) with premultiplied invariant `rgb <= alpha`.
        let mut half = vec![0_u8; 16 * 16 * 4];
        for y in 0..16 {
            for x in 0..8 {
                let o = (y * 16 + x) * 4;
                half[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let out = scale_rgba_to_premul_bgra(16, 16, &half, 5).ok_or("half downscale failed")?;
        let row: Vec<[u8; 4]> = out[2 * 5 * 4..3 * 5 * 4].as_chunks::<4>().0.to_vec();
        assert!(row[0][3] >= 240 && row[1][3] >= 240, "{row:?}");
        assert!(row[3][3] <= 15 && row[4][3] <= 15, "{row:?}");
        assert!(row[2][3] > 32 && row[2][3] < 224, "{row:?}");
        assert!(
            row.iter()
                .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
        );
        Ok(())
    }

    #[test]
    fn bmp_24bit_and_32bit_decoding_and_bounds_work() -> Result<(), &'static str> {
        let mut bmp24 = vec![0_u8; 62];
        bmp24[0..2].copy_from_slice(b"BM");
        bmp24[2..6].copy_from_slice(&62_u32.to_le_bytes());
        bmp24[10..14].copy_from_slice(&54_u32.to_le_bytes());
        bmp24[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bmp24[18..22].copy_from_slice(&2_i32.to_le_bytes());
        bmp24[22..26].copy_from_slice(&1_i32.to_le_bytes());
        bmp24[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bmp24[28..30].copy_from_slice(&24_u16.to_le_bytes());
        bmp24[54..60].copy_from_slice(&[0x10, 0x20, 0x30, 0xFF, 0x80, 0x00]);

        assert_eq!(read_bmp_dimensions_from_bytes(&bmp24), Some((2, 1)));
        let (w, h, rgba) = decode_bmp_rgba_limited(&bmp24, 64).ok_or("bmp24 decode failed")?;
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, vec![0x30, 0x20, 0x10, 255, 0x00, 0x80, 0xFF, 255]);

        let (w_gen, h_gen, rgba_gen) =
            decode_image_rgba_limited(&bmp24, 64).ok_or("decode_image bmp failed")?;
        assert_eq!((w_gen, h_gen), (2, 1));
        assert_eq!(rgba_gen, rgba);

        Ok(())
    }

    #[test]
    fn expanded_header_dimension_readers_and_preview_classifier_work() {
        // Minimal JPEG SOF0 header: SOI + SOF0 (length 11: 2 len + 1 prec + 2 h + 2 w + 1 comp + 3 bytes)
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x78, 0x01, 0x40, 0x01, 0x01, 0x11,
            0x00,
        ];
        assert_eq!(read_image_dimensions_from_bytes(&jpeg), Some((320, 120)));

        // Minimal WebP VP8L lossless header (w=64, h=32 => stored as w-1=63, h-1=31)
        let bits: u32 = 63 | (31 << 14);
        let mut webp_l = vec![0_u8; 30];
        webp_l[0..4].copy_from_slice(b"RIFF");
        webp_l[8..12].copy_from_slice(b"WEBP");
        webp_l[12..16].copy_from_slice(b"VP8L");
        webp_l[20] = 0x2F;
        webp_l[21..25].copy_from_slice(&bits.to_le_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&webp_l), Some((64, 32)));

        // Minimal GIF89a header (w=128, h=96)
        let mut gif = [0_u8; 10];
        gif[0..6].copy_from_slice(b"GIF89a");
        gif[6..8].copy_from_slice(&128_u16.to_le_bytes());
        gif[8..10].copy_from_slice(&96_u16.to_le_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&gif), Some((128, 96)));

        // SVG with width/height attributes and SVG with viewBox fallback
        let svg_wh =
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="240px" height="180"></svg>"#;
        assert_eq!(read_image_dimensions_from_bytes(svg_wh), Some((240, 180)));
        let svg_vb = br#"<svg viewBox="0 0 512 256"></svg>"#;
        assert_eq!(read_image_dimensions_from_bytes(svg_vb), Some((512, 256)));

        assert_eq!(
            classify_preview_filename("photo.JPG"),
            Some(PreviewMediaKind::OtherRaster)
        );
        assert_eq!(
            classify_preview_filename("hero.webp"),
            Some(PreviewMediaKind::OtherRaster)
        );
        assert_eq!(
            classify_preview_filename("icon.svg"),
            Some(PreviewMediaKind::Svg)
        );
        assert_eq!(
            classify_preview_filename("report.pdf"),
            Some(PreviewMediaKind::Pdf)
        );
        assert_eq!(
            classify_preview_filename("clip.mp4"),
            Some(PreviewMediaKind::Video)
        );
        assert!(is_supported_preview_filename("demo.mkv"));
        assert!(!is_supported_preview_filename("Cargo.toml"));
    }
}
