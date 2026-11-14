//! Pure-Rust PNG and PPM image decoding and premultiplied `BGRA` scaling helpers.
//!
//! Extracted from the compositor icon/wallpaper decoder so both `rusty-compositor`
//! and `apps/rusty-files` share one zero-dependency, `unsafe`-free implementation
//! with strict dimension, file-size, and decompression limits.

use std::fs;
use std::io::Read as _;
use std::path::Path;

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Maximum pixel dimension allowed for icons/tray pixmaps.
pub const MAX_ICON_DIM: u32 = 512;
/// Maximum PNG file size allowed for XDG icon loading (`2 MiB`).
pub const MAX_PNG_FILE_BYTES: usize = 2 * 1024 * 1024;
/// Maximum inflated raw scanline bytes for icons (`4 MiB`).
pub const MAX_INFLATE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum pixel dimension allowed for wallpapers and Finder image previews.
pub const MAX_WALLPAPER_DIM: u32 = 4096;
/// Maximum file size allowed for wallpapers and Finder image previews (`32 MiB`).
pub const MAX_WALLPAPER_FILE_BYTES: usize = 32 * 1024 * 1024;
/// Maximum inflated scanline bytes for wallpapers and Finder image previews (`80 MiB`).
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

/// Reads `(width, height)` from a `.png` or `.ppm` image header for `Get Info` / inspector views.
#[must_use]
pub fn read_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    let mut file = fs::File::open(path).ok()?;
    let mut head = [0_u8; 256];
    let n = file.read(&mut head).ok()?;
    let slice = &head[..n];
    read_png_dimensions_from_bytes(slice).or_else(|| read_ppm_dimensions_from_bytes(slice))
}

/// Loads and decodes a `.png` or `.ppm` raster file into aspect-fit premultiplied `BGRA` pixels.
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
    if usize::try_from(meta.len()).unwrap_or(usize::MAX) > MAX_WALLPAPER_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let (src_w, src_h, rgba) = if bytes.starts_with(&PNG_SIGNATURE) {
        decode_png_rgba_limited(&bytes, MAX_WALLPAPER_DIM, MAX_WALLPAPER_INFLATE_BYTES)?
    } else if bytes.starts_with(b"P6") || bytes.starts_with(b"P3") {
        decode_ppm_rgba_limited(&bytes, MAX_WALLPAPER_DIM)?
    } else {
        return None;
    };

    let (dst_w, dst_h) = fit_aspect_dimensions(src_w, src_h, max_w, max_h)?;
    let bgra_premul = scale_rgba_rect_to_premul_bgra(src_w, src_h, &rgba, dst_w, dst_h)?;
    Some(DecodedPreviewImage {
        orig_width: u32::try_from(src_w).ok()?,
        orig_height: u32::try_from(src_h).ok()?,
        width: dst_w,
        height: dst_h,
        bgra_premul,
    })
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
}
