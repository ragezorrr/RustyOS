//! Pure-Rust PNG & SNI pixmap icon decoder and GPU-cached XDG icon resolver (`RUSTYOS-033`).
//!
//! Decodes `.png` desktop/tray icons and freedesktop `StatusNotifierItem` `IconPixmap`
//! buffers into premultiplied `Argb8888` [`MemoryRenderBuffer`] textures without adding
//! external image-decoding dependencies or touching the compositor event loop repeatedly.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const MAX_ICON_DIM: u32 = 512;
const MAX_PNG_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_INFLATE_BYTES: usize = 4 * 1024 * 1024;

/// Raw D-Bus `StatusNotifierItem.IconPixmap` frame in network byte order (`ARGB32`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayPixmap {
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
    /// Big-endian ARGB32 pixel data (`width * height * 4` bytes).
    pub argb_be: Vec<u8>,
    /// Fast content hash used as GPU cache discriminator when an applet updates its pixmap.
    pub fingerprint: u64,
}

impl TrayPixmap {
    /// Constructs a [`TrayPixmap`] validating length bounds.
    #[must_use]
    pub fn from_argb_be(width: i32, height: i32, argb_be: Vec<u8>) -> Option<Self> {
        let w = usize::try_from(width).ok()?;
        let h = usize::try_from(height).ok()?;
        if w == 0 || h == 0 || w > 512 || h > 512 {
            return None;
        }
        let expected = w.checked_mul(h)?.checked_mul(4)?;
        if argb_be.len() != expected {
            return None;
        }
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for &byte in argb_be.iter().step_by(3) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        Some(Self {
            width,
            height,
            argb_be,
            fingerprint: hash,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IconCacheKey {
    source_key: String,
    size_px: u16,
    fingerprint: u64,
}

/// CPU-decoded, GPU-cached application & tray icon store (`RUSTYOS-033`).
#[derive(Debug, Default)]
pub struct IconCache {
    search_roots: Option<Vec<PathBuf>>,
    resolved_paths: HashMap<String, Option<PathBuf>>,
    buffers: HashMap<IconCacheKey, Option<MemoryRenderBuffer>>,
}

impl IconCache {
    /// Creates an empty [`IconCache`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if `icon_name` resolves to a decodable PNG icon cached/loadable at `size_px`.
    #[must_use]
    pub fn has_named_icon(&mut self, icon_name: &str, size_px: i32) -> bool {
        let Ok(size_u16) = u16::try_from(size_px) else {
            return false;
        };
        if icon_name.trim().is_empty() || size_px <= 0 {
            return false;
        }
        self.ensure_named_buffer(icon_name, size_u16).is_some()
    }

    /// Renders a GPU-cached XDG / filesystem PNG icon named `icon_name` scaled to `size_px x size_px`.
    pub fn icon_element_by_name(
        &mut self,
        renderer: &mut GlesRenderer,
        icon_name: &str,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let size_u16 = u16::try_from(size_px).ok()?;
        if icon_name.trim().is_empty() || size_px <= 0 {
            return None;
        }
        self.ensure_named_buffer(icon_name, size_u16);
        let key = IconCacheKey {
            source_key: format!("name:{icon_name}"),
            size_px: size_u16,
            fingerprint: 0,
        };
        let buf = self.buffers.get(&key)?.as_ref()?;
        element_from_icon_buffer(renderer, buf, size_px, loc)
    }

    /// Renders a GPU-cached D-Bus SNI [`TrayPixmap`] scaled to `size_px x size_px`.
    pub fn icon_element_from_pixmap(
        &mut self,
        renderer: &mut GlesRenderer,
        sni_key: &str,
        pixmap: &TrayPixmap,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let size_u16 = u16::try_from(size_px).ok()?;
        if size_px <= 0 {
            return None;
        }
        let key = IconCacheKey {
            source_key: format!("pixmap:{sni_key}"),
            size_px: size_u16,
            fingerprint: pixmap.fingerprint,
        };
        let buf_opt = self.buffers.entry(key).or_insert_with(|| {
            let dst = usize::from(size_u16);
            let bgra = argb32_be_to_premul_bgra(pixmap.width, pixmap.height, &pixmap.argb_be, dst)?;
            Some(upload_bgra_buffer(size_px, &bgra))
        });
        let buf = buf_opt.as_ref()?;
        element_from_icon_buffer(renderer, buf, size_px, loc)
    }

    fn ensure_named_buffer(
        &mut self,
        icon_name: &str,
        size_u16: u16,
    ) -> Option<&MemoryRenderBuffer> {
        let key = IconCacheKey {
            source_key: format!("name:{icon_name}"),
            size_px: size_u16,
            fingerprint: 0,
        };
        if !self.buffers.contains_key(&key) {
            let path = self.resolve_icon_path(icon_name);
            let decoded = path.and_then(|p| load_png_as_bgra(&p, usize::from(size_u16)));
            let buf = decoded.map(|bgra| upload_bgra_buffer(i32::from(size_u16), &bgra));
            self.buffers.insert(key.clone(), buf);
        }
        self.buffers.get(&key).and_then(Option::as_ref)
    }

    fn resolve_icon_path(&mut self, icon_name: &str) -> Option<PathBuf> {
        let normalized = icon_name.trim();
        if normalized.is_empty() {
            return None;
        }
        if let Some(cached) = self.resolved_paths.get(normalized) {
            return cached.clone();
        }
        let roots = self
            .search_roots
            .get_or_insert_with(discover_icon_search_roots);
        let found = find_icon_png_in_roots(normalized, roots);
        self.resolved_paths
            .insert(normalized.to_owned(), found.clone());
        found
    }
}

fn element_from_icon_buffer(
    renderer: &mut GlesRenderer,
    buf: &MemoryRenderBuffer,
    size_px: i32,
    loc: Point<i32, Logical>,
) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        loc.to_f64().to_physical(1.0),
        buf,
        None,
        Some(Rectangle::<f64, Logical>::from_size(Size::from((
            f64::from(size_px),
            f64::from(size_px),
        )))),
        Some(Size::from((size_px, size_px))),
        Kind::Unspecified,
    )
    .ok()
}

fn upload_bgra_buffer(size_px: i32, bgra: &[u8]) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        bgra,
        Fourcc::Argb8888,
        (size_px, size_px),
        1,
        Transform::Normal,
        None,
    )
}

fn load_png_as_bgra(path: &Path, dst_size: usize) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    if usize::try_from(meta.len()).unwrap_or(usize::MAX) > MAX_PNG_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let (w, h, rgba) = decode_png_rgba(&bytes)?;
    scale_rgba_to_premul_bgra(w, h, &rgba, dst_size)
}

fn discover_icon_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        if !xdg_data_home.trim().is_empty() {
            roots.push(PathBuf::from(&xdg_data_home).join("icons"));
            roots.push(PathBuf::from(xdg_data_home).join("pixmaps"));
        }
    } else if let Ok(home) = env::var("HOME")
        && !home.trim().is_empty()
    {
        roots.push(PathBuf::from(&home).join(".local/share/icons"));
        roots.push(PathBuf::from(home).join(".local/share/pixmaps"));
    }
    roots.push(PathBuf::from("/usr/local/share/icons"));
    roots.push(PathBuf::from("/usr/share/icons"));
    roots.push(PathBuf::from("/usr/share/pixmaps"));
    roots
}

fn find_icon_png_in_roots(icon_name: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let direct = Path::new(icon_name);
    if direct.is_absolute() && direct.is_file() {
        let is_png = direct
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("png"));
        return is_png.then(|| direct.to_path_buf());
    }

    let stem = icon_name
        .strip_suffix(".png")
        .or_else(|| icon_name.strip_suffix(".PNG"))
        .unwrap_or(icon_name);
    let file_name = format!("{stem}.png");
    let lower_file_name = format!("{}.png", stem.to_ascii_lowercase());

    let themes = ["hicolor", "Papirus", "Adwaita", "breeze", "Yaru", "Numix"];
    let size_dirs = [
        "48x48", "64x64", "32x32", "128x128", "24x24", "256x256", "16x16",
    ];
    let cats = ["apps", "status", "categories", "devices", "actions"];

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        // Check flat pixmaps root (`root/<name>.png`)
        for candidate_name in [&file_name, &lower_file_name] {
            let flat = root.join(candidate_name);
            if flat.is_file() {
                return Some(flat);
            }
        }
        for theme in themes {
            let theme_dir = root.join(theme);
            if !theme_dir.is_dir() {
                continue;
            }
            for size in size_dirs {
                for cat in cats {
                    for candidate_name in [&file_name, &lower_file_name] {
                        let candidate = theme_dir.join(size).join(cat).join(candidate_name);
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                        let alt = theme_dir.join(cat).join(size).join(candidate_name);
                        if alt.is_file() {
                            return Some(alt);
                        }
                    }
                }
            }
        }
    }
    None
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

/// Scales straight `RGBA` pixels to `dst_size x dst_size` premultiplied `Argb8888` (`[B, G, R, A]`).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names
)]
pub fn scale_rgba_to_premul_bgra(
    src_w: usize,
    src_h: usize,
    rgba: &[u8],
    dst_size: usize,
) -> Option<Vec<u8>> {
    if src_w == 0 || src_h == 0 || dst_size == 0 {
        return None;
    }
    if rgba.len() != src_w.checked_mul(src_h)?.checked_mul(4)? {
        return None;
    }

    let mut out = vec![0_u8; dst_size * dst_size * 4];
    for dy in 0..dst_size {
        let src_yf = ((dy as f32) + 0.5) * (src_h as f32) / (dst_size as f32) - 0.5;
        let y0 = (src_yf.floor().max(0.0) as usize).min(src_h - 1);
        let y1 = (y0 + 1).min(src_h - 1);
        let fy = (src_yf - y0 as f32).clamp(0.0, 1.0);

        for dx in 0..dst_size {
            let src_xf = ((dx as f32) + 0.5) * (src_w as f32) / (dst_size as f32) - 0.5;
            let x0 = (src_xf.floor().max(0.0) as usize).min(src_w - 1);
            let x1 = (x0 + 1).min(src_w - 1);
            let fx = (src_xf - x0 as f32).clamp(0.0, 1.0);

            let p00 = sample_rgba_premul(rgba, src_w, x0, y0);
            let p10 = sample_rgba_premul(rgba, src_w, x1, y0);
            let p01 = sample_rgba_premul(rgba, src_w, x0, y1);
            let p11 = sample_rgba_premul(rgba, src_w, x1, y1);

            let dst_off = (dy * dst_size + dx) * 4;
            for ch in 0..4 {
                let top = p00[ch] + (p10[ch] - p00[ch]) * fx;
                let bot = p01[ch] + (p11[ch] - p01[ch]) * fx;
                let val = top + (bot - top) * fy;
                // Output channel order for Fourcc::Argb8888 on little-endian is [B, G, R, A].
                let out_ch = match ch {
                    0 => 2,
                    1 => 1,
                    2 => 0,
                    _ => 3,
                };
                out[dst_off + out_ch] = val.round().clamp(0.0, 255.0) as u8;
            }
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

/// Decodes a standard non-interlaced PNG byte buffer into `(width, height, straight_rgba)`.
#[must_use]
pub fn decode_png_rgba(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return None;
    }

    let mut pos = 8;
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut bit_depth: u8 = 0;
    let mut color_type: u8 = 0;
    let mut ihdr_seen = false;
    let mut palette: Vec<u8> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();

    while pos + 12 <= data.len() {
        let len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        let chunk_type = &data[pos..pos + 4];
        pos += 4;
        if pos + len + 4 > data.len() {
            return None;
        }
        let payload = &data[pos..pos + len];
        pos += len + 4; // skip payload + CRC

        match chunk_type {
            b"IHDR" => {
                if payload.len() != 13 {
                    return None;
                }
                width = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                height = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                bit_depth = payload[8];
                color_type = payload[9];
                let compression = payload[10];
                let filter = payload[11];
                let interlace = payload[12];
                if width == 0
                    || height == 0
                    || width > MAX_ICON_DIM
                    || height > MAX_ICON_DIM
                    || compression != 0
                    || filter != 0
                    || interlace != 0
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

    let channels: usize = match color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    if !matches!(bit_depth, 1 | 2 | 4 | 8 | 16) {
        return None;
    }

    let w = usize::try_from(width).ok()?;
    let h = usize::try_from(height).ok()?;
    let bits_per_pixel = channels.checked_mul(usize::from(bit_depth))?;
    let stride = w.checked_mul(bits_per_pixel)?.div_ceil(8);
    let bpp = bits_per_pixel.div_ceil(8);
    let expected_raw = h.checked_mul(stride.checked_add(1)?)?;

    let raw = zlib_decompress(&idat, expected_raw)?;
    if raw.len() < expected_raw {
        return None;
    }

    let unfiltered = unfilter_scanlines(&raw[..expected_raw], h, stride, bpp)?;
    let rgba = expand_samples_to_rgba(&unfiltered, w, h, bit_depth, color_type, &palette, &trns)?;
    Some((w, h, rgba))
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
            match (color_type, bit_depth) {
                (6, 8) => {
                    let off = x * 4;
                    out[dst..dst + 4].copy_from_slice(&row[off..off + 4]);
                }
                (6, 16) => {
                    let off = x * 8;
                    out[dst] = row[off];
                    out[dst + 1] = row[off + 2];
                    out[dst + 2] = row[off + 4];
                    out[dst + 3] = row[off + 6];
                }
                (2, 8) => {
                    let off = x * 3;
                    out[dst] = row[off];
                    out[dst + 1] = row[off + 1];
                    out[dst + 2] = row[off + 2];
                    out[dst + 3] = 255;
                }
                (2, 16) => {
                    let off = x * 6;
                    out[dst] = row[off];
                    out[dst + 1] = row[off + 2];
                    out[dst + 2] = row[off + 4];
                    out[dst + 3] = 255;
                }
                (4, 8) => {
                    let off = x * 2;
                    let g = row[off];
                    out[dst] = g;
                    out[dst + 1] = g;
                    out[dst + 2] = g;
                    out[dst + 3] = row[off + 1];
                }
                (4, 16) => {
                    let off = x * 4;
                    let g = row[off];
                    out[dst] = g;
                    out[dst + 1] = g;
                    out[dst + 2] = g;
                    out[dst + 3] = row[off + 2];
                }
                (3, depth @ (1 | 2 | 4 | 8)) => {
                    let idx = usize::from(read_subbyte_sample(row, x, depth)?);
                    let poff = idx.checked_mul(3)?;
                    if poff + 3 > palette.len() {
                        return None;
                    }
                    out[dst] = palette[poff];
                    out[dst + 1] = palette[poff + 1];
                    out[dst + 2] = palette[poff + 2];
                    out[dst + 3] = trns.get(idx).copied().unwrap_or(255);
                }
                (0, depth @ (1 | 2 | 4 | 8)) => {
                    let raw_v = read_subbyte_sample(row, x, depth)?;
                    let max_v = ((1_u16 << depth) - 1).max(1);
                    let gray = u8::try_from((u16::from(raw_v) * 255) / max_v).ok()?;
                    out[dst] = gray;
                    out[dst + 1] = gray;
                    out[dst + 2] = gray;
                    out[dst + 3] = 255;
                }
                (0, 16) => {
                    let gray = row[x * 2];
                    out[dst] = gray;
                    out[dst + 1] = gray;
                    out[dst + 2] = gray;
                    out[dst + 3] = 255;
                }
                _ => return None,
            }
        }
    }
    Some(out)
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
        // Flush any buffered whole bytes in bit_buf back into cursor position first.
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
                if out.len() >= MAX_INFLATE_BYTES {
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
                if distance == 0 || distance > out.len() || out.len() + length > MAX_INFLATE_BYTES {
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

fn zlib_decompress(data: &[u8], expected_hint: usize) -> Option<Vec<u8>> {
    if data.len() < 6 {
        return None;
    }
    let cmf = data[0];
    let flg = data[1];
    if (cmf & 0x0f) != 8 || (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 || (flg & 0x20) != 0 {
        return None;
    }

    let mut reader = BitReader::new(&data[2..]);
    let mut out = Vec::with_capacity(expected_hint.min(MAX_INFLATE_BYTES));

    loop {
        let bfinal = reader.read_bits(1)?;
        let btype = reader.read_bits(2)?;
        match btype {
            0 => {
                reader.align_byte();
                // Account for already-read partial bits when reading LEN/NLEN from byte stream
                let buffered_bytes = usize::from(reader.bit_count / 8);
                reader.byte_pos = reader.byte_pos.checked_sub(buffered_bytes)?;
                reader.bit_buf = 0;
                reader.bit_count = 0;
                let len = reader.read_u16_aligned()?;
                let nlen = reader.read_u16_aligned()?;
                if len != !nlen {
                    return None;
                }
                if out.len() + usize::from(len) > MAX_INFLATE_BYTES {
                    return None;
                }
                reader.copy_aligned_bytes(usize::from(len), &mut out)?;
            }
            1 => {
                let (litlen, dist) = fixed_huffman_tables()?;
                decode_huffman_block(&mut reader, &litlen, &dist, &mut out)?;
            }
            2 => {
                let (litlen, dist) = dynamic_huffman_tables(&mut reader)?;
                decode_huffman_block(&mut reader, &litlen, &dist, &mut out)?;
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

    #[test]
    fn roundtrip_png_decode_and_downscale_to_premul_bgra() -> Result<(), String> {
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
        ];
        let png = crate::screenshot::encode_png_rgba(2, 2, &rgba)?;
        let (w, h, decoded) =
            decode_png_rgba(&png).ok_or_else(|| "PNG decoding failed".to_owned())?;
        assert_eq!((w, h), (2, 2));
        assert_eq!(decoded, rgba);

        let bgra = scale_rgba_to_premul_bgra(w, h, &decoded, 2)
            .ok_or_else(|| "BGRA scaling failed".to_owned())?;
        assert_eq!(bgra.len(), 16);
        // First pixel (255, 0, 0, 255) -> BGRA [0, 0, 255, 255]
        assert_eq!(&bgra[0..4], &[0, 0, 255, 255]);
        // Fourth pixel (255, 255, 255, 128) -> premultiplied [128, 128, 128, 128]
        assert_eq!(&bgra[12..16], &[128, 128, 128, 128]);
        Ok(())
    }

    #[test]
    fn converts_sni_argb32_pixmap_and_validates_dimensions() -> Result<(), String> {
        // 1x1 pixel ARGB big-endian: alpha=0x80, r=0xFF, g=0x00, b=0x00
        let pix = TrayPixmap::from_argb_be(1, 1, vec![0x80, 0xFF, 0x00, 0x00])
            .ok_or_else(|| "Valid 1x1 pixmap should succeed".to_owned())?;
        let bgra = argb32_be_to_premul_bgra(pix.width, pix.height, &pix.argb_be, 2)
            .ok_or_else(|| "Pixmap conversion should succeed".to_owned())?;
        assert_eq!(bgra.len(), 16);
        // Premultiplied BGRA of (r=255, g=0, b=0, a=128): B=0, G=0, R=128, A=128
        assert_eq!(&bgra[0..4], &[0, 0, 128, 128]);

        assert!(TrayPixmap::from_argb_be(0, 10, Vec::new()).is_none());
        assert!(TrayPixmap::from_argb_be(2, 2, vec![0; 4]).is_none());
        Ok(())
    }

    #[test]
    fn resolves_png_icons_in_xdg_theme_hierarchy() -> Result<(), String> {
        let base = std::env::temp_dir().join(format!("rustyos-icons-test-{}", std::process::id()));
        let app_dir = base.join("hicolor/48x48/apps");
        fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
        let icon_file = app_dir.join("test-app.png");
        let png = crate::screenshot::encode_png_rgba(1, 1, &[10, 20, 30, 255])?;
        fs::write(&icon_file, png).map_err(|e| e.to_string())?;

        let resolved = find_icon_png_in_roots("test-app", std::slice::from_ref(&base));
        assert_eq!(resolved, Some(icon_file));
        assert_eq!(
            find_icon_png_in_roots("nonexistent-app", std::slice::from_ref(&base)),
            None
        );

        let _ = fs::remove_dir_all(&base);
        Ok(())
    }
}
