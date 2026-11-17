//! Pure CPU 2D canvas, Fontdue Inter glyph rasterization, and SDF anti-aliased drawing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};
use rusty_services::file_manager::DecodedPreviewImage;

use crate::layout::Rect;

const INTER_REGULAR_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/Inter-SemiBold.ttf");

/// 4x4 rotated-grid supersampling offsets in `[0.0, 1.0)` for polygon anti-aliasing.
const RGSS_16: [(f32, f32); 16] = [
    (0.125, 0.0625),
    (0.375, 0.1875),
    (0.625, 0.0625),
    (0.875, 0.1875),
    (0.0625, 0.375),
    (0.3125, 0.3125),
    (0.5625, 0.4375),
    (0.8125, 0.3125),
    (0.1875, 0.625),
    (0.4375, 0.5625),
    (0.6875, 0.6875),
    (0.9375, 0.5625),
    (0.0625, 0.875),
    (0.3125, 0.9375),
    (0.5625, 0.8125),
    (0.8125, 0.9375),
];

pub(crate) fn regular_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| Font::from_bytes(INTER_REGULAR_BYTES, FontSettings::default()).ok())
        .as_ref()
}

pub(crate) fn semibold_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| Font::from_bytes(INTER_SEMIBOLD_BYTES, FontSettings::default()).ok())
        .as_ref()
        .or_else(regular_font)
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct GlyphKey {
    ch: char,
    bold: bool,
    tenths_px: u16,
}

struct CachedGlyph {
    xmin: i32,
    ymin: i32,
    width: usize,
    height: usize,
    advance: i32,
    bitmap: Vec<u8>,
}

thread_local! {
    static GLYPH_CACHE: RefCell<HashMap<GlyphKey, CachedGlyph>> =
        RefCell::new(HashMap::with_capacity(512));
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn with_cached_glyph<R>(
    ch: char,
    size_px: f32,
    bold: bool,
    f: impl FnOnce(&CachedGlyph) -> R,
) -> Option<R> {
    let tenths_px = (size_px * 10.0).round().clamp(10.0, 2000.0) as u16;
    let key = GlyphKey {
        ch,
        bold,
        tenths_px,
    };
    GLYPH_CACHE.with(|cell| {
        let mut map = cell.borrow_mut();
        if map.len() > 4096 {
            map.clear();
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = map.entry(key) {
            let font = if bold {
                semibold_font()
            } else {
                regular_font()
            }?;
            let (metrics, bitmap) = font.rasterize(ch, f32::from(tenths_px) / 10.0);
            let cached = CachedGlyph {
                xmin: metrics.xmin,
                ymin: metrics.ymin,
                width: metrics.width,
                height: metrics.height,
                advance: (metrics.advance_width.round() as i32).max(1),
                bitmap,
            };
            entry.insert(cached);
        }
        map.get(&key).map(f)
    })
}

pub(crate) struct Canvas<'a> {
    pub(crate) buf: &'a mut [u8],
    pub(crate) w: i32,
    pub(crate) h: i32,
    clip: Option<Rect>,
}

#[inline]
const fn div_255(v: u16) -> u16 {
    (v + 1 + (v >> 8)) >> 8
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(buf: &'a mut [u8], width: u32, height: u32) -> Self {
        Self {
            buf,
            w: i32::try_from(width).unwrap_or(0),
            h: i32::try_from(height).unwrap_or(0),
            clip: None,
        }
    }

    pub(crate) fn effective_clip(&self, bounds: Rect) -> Rect {
        let mut eff = intersect_rect(bounds, Rect::new(0, 0, self.w, self.h));
        if let Some(sc) = self.clip {
            eff = intersect_rect(eff, sc);
        }
        eff
    }

    pub(crate) fn clear(&mut self) {
        self.buf.fill(0);
    }

    pub(crate) fn with_clip(&mut self, rect: Rect, f: impl FnOnce(&mut Self)) {
        let next = match self.clip {
            Some(prev) => intersect_rect(prev, rect),
            None => rect,
        };
        let saved = self.clip;
        self.clip = Some(next);
        f(self);
        self.clip = saved;
    }

    /// Blits a premultiplied-BGRA sprite 1:1 without scaling inside the active clip and `bounds`.
    #[allow(clippy::cast_sign_loss)]
    pub(crate) fn blit_premul_sprite(
        &mut self,
        dst_x: i32,
        dst_y: i32,
        src_w: i32,
        src_h: i32,
        bgra_premul: &[u8],
    ) {
        self.blit_premul_sprite_clipped(
            dst_x,
            dst_y,
            src_w,
            src_h,
            bgra_premul,
            Rect::new(0, 0, self.w, self.h),
        );
    }

    #[allow(clippy::cast_sign_loss)]
    pub(crate) fn blit_premul_sprite_clipped(
        &mut self,
        dst_x: i32,
        dst_y: i32,
        src_w: i32,
        src_h: i32,
        bgra_premul: &[u8],
        bounds: Rect,
    ) {
        if src_w <= 0 || src_h <= 0 {
            return;
        }
        let eff = self.effective_clip(intersect_rect(
            bounds,
            Rect::new(dst_x, dst_y, src_w, src_h),
        ));
        if eff.w <= 0 || eff.h <= 0 {
            return;
        }
        let dst_stride = usize::try_from(self.w).unwrap_or(0) * 4;
        let src_stride = usize::try_from(src_w).unwrap_or(0) * 4;
        let x_skip = usize::try_from(eff.x - dst_x).unwrap_or(0) * 4;
        let row_bytes = usize::try_from(eff.w).unwrap_or(0) * 4;
        let dst_x_bytes = usize::try_from(eff.x).unwrap_or(0) * 4;

        for py in eff.y..eff.bottom() {
            let src_off = usize::try_from(py - dst_y).unwrap_or(0) * src_stride + x_skip;
            let dst_off = usize::try_from(py).unwrap_or(0) * dst_stride + dst_x_bytes;
            if src_off + row_bytes > bgra_premul.len() || dst_off + row_bytes > self.buf.len() {
                continue;
            }
            let src_row = &bgra_premul[src_off..src_off + row_bytes];
            let dst_row = &mut self.buf[dst_off..dst_off + row_bytes];
            for (dp, sp) in dst_row
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src_row.as_chunks::<4>().0)
            {
                let [sb, sg, sr, sa] = *sp;
                if sa == 0 {
                    continue;
                }
                if sa == 255 {
                    *dp = [sb, sg, sr, 255];
                } else {
                    let inv = 255 - u16::from(sa);
                    let db = u16::from(dp[0]) * inv;
                    let dg = u16::from(dp[1]) * inv;
                    let dr = u16::from(dp[2]) * inv;
                    let da = u16::from(dp[3]) * inv;
                    dp[0] = (u16::from(sb) + ((db + 1 + (db >> 8)) >> 8)).min(255) as u8;
                    dp[1] = (u16::from(sg) + ((dg + 1 + (dg >> 8)) >> 8)).min(255) as u8;
                    dp[2] = (u16::from(sr) + ((dr + 1 + (dr >> 8)) >> 8)).min(255) as u8;
                    dp[3] = (u16::from(sa) + ((da + 1 + (da >> 8)) >> 8)).min(255) as u8;
                }
            }
        }
    }

    /// Blits a premultiplied-BGRA decoded preview image scaled smoothly inside `(dst_x, dst_y, dst_w, dst_h)`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn blit_bgra_premul_fit(
        &mut self,
        dst_x: i32,
        dst_y: i32,
        dst_w: i32,
        dst_h: i32,
        img: &DecodedPreviewImage,
        clip: Rect,
    ) {
        if dst_w <= 0 || dst_h <= 0 || img.width == 0 || img.height == 0 {
            return;
        }
        let sw = i32::try_from(img.width).unwrap_or(0);
        let sh = i32::try_from(img.height).unwrap_or(0);
        if sw == dst_w && sh == dst_h {
            self.blit_premul_sprite_clipped(dst_x, dst_y, sw, sh, &img.bgra_premul, clip);
            return;
        }
        let eff = self.effective_clip(intersect_rect(clip, Rect::new(dst_x, dst_y, dst_w, dst_h)));
        if eff.w <= 0 || eff.h <= 0 {
            return;
        }
        let src_w_i64 = i64::from(sw);
        let height_i64 = i64::from(sh);
        let dw = i64::from(dst_w);
        let dh = i64::from(dst_h);
        let stride = img.width;
        for py in eff.y..eff.bottom() {
            let sy = usize::try_from(
                ((i64::from(py - dst_y) * height_i64) / dh).clamp(0, height_i64 - 1),
            )
            .unwrap_or(0);
            for px in eff.x..eff.right() {
                let sx = usize::try_from(
                    ((i64::from(px - dst_x) * src_w_i64) / dw).clamp(0, src_w_i64 - 1),
                )
                .unwrap_or(0);
                let s_idx = (sy * stride + sx) * 4;
                let d_idx = ((py * self.w + px) * 4) as usize;
                if s_idx + 3 >= img.bgra_premul.len() || d_idx + 3 >= self.buf.len() {
                    continue;
                }
                let sb = u16::from(img.bgra_premul[s_idx]);
                let sg = u16::from(img.bgra_premul[s_idx + 1]);
                let sr = u16::from(img.bgra_premul[s_idx + 2]);
                let sa = u16::from(img.bgra_premul[s_idx + 3]);
                if sa == 0 {
                    continue;
                }
                if sa == 255 {
                    self.buf[d_idx] = sb as u8;
                    self.buf[d_idx + 1] = sg as u8;
                    self.buf[d_idx + 2] = sr as u8;
                    self.buf[d_idx + 3] = 255;
                } else {
                    let inv = 255 - sa;
                    self.buf[d_idx] =
                        (sb + div_255(u16::from(self.buf[d_idx]) * inv)).min(255) as u8;
                    self.buf[d_idx + 1] =
                        (sg + div_255(u16::from(self.buf[d_idx + 1]) * inv)).min(255) as u8;
                    self.buf[d_idx + 2] =
                        (sr + div_255(u16::from(self.buf[d_idx + 2]) * inv)).min(255) as u8;
                    self.buf[d_idx + 3] =
                        (sa + div_255(u16::from(self.buf[d_idx + 3]) * inv)).min(255) as u8;
                }
            }
        }
    }

    #[inline]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn blend_pixel_unchecked(&mut self, x: i32, y: i32, color: [u8; 4], cov: u8) {
        let alpha = div_255(u16::from(color[3]) * u16::from(cov));
        if alpha == 0 {
            return;
        }
        let idx = ((y * self.w + x) * 4) as usize;
        if idx + 3 >= self.buf.len() {
            return;
        }
        if alpha == 255 {
            self.buf[idx] = color[2];
            self.buf[idx + 1] = color[1];
            self.buf[idx + 2] = color[0];
            self.buf[idx + 3] = 255;
            return;
        }
        let inv = 255 - alpha;
        let sb = div_255(u16::from(color[2]) * alpha);
        let sg = div_255(u16::from(color[1]) * alpha);
        let sr = div_255(u16::from(color[0]) * alpha);

        let db = div_255(u16::from(self.buf[idx]) * inv);
        let dg = div_255(u16::from(self.buf[idx + 1]) * inv);
        let dr = div_255(u16::from(self.buf[idx + 2]) * inv);
        let da = div_255(u16::from(self.buf[idx + 3]) * inv);

        self.buf[idx] = (sb + db).min(255) as u8;
        self.buf[idx + 1] = (sg + dg).min(255) as u8;
        self.buf[idx + 2] = (sr + dr).min(255) as u8;
        self.buf[idx + 3] = (alpha + da).min(255) as u8;
    }

    #[allow(clippy::cast_sign_loss)]
    pub(crate) fn blend_pixel(&mut self, x: i32, y: i32, color: [u8; 4], cov: u8) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || cov == 0 || color[3] == 0 {
            return;
        }
        if let Some(clip) = self.clip
            && (x < clip.x || y < clip.y || x >= clip.right() || y >= clip.bottom())
        {
            return;
        }
        self.blend_pixel_unchecked(x, y, color, cov);
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn blend_sdf(&mut self, x: i32, y: i32, color: [u8; 4], signed_dist: f32) {
        if signed_dist < 0.55 {
            let cov = ((0.55 - signed_dist).clamp(0.0, 1.0) * 255.0).round() as u8;
            self.blend_pixel(x, y, color, cov);
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn blend_sdf_clipped(&mut self, x: i32, y: i32, color: [u8; 4], signed_dist: f32) {
        if signed_dist < 0.55 {
            let cov = ((0.55 - signed_dist).clamp(0.0, 1.0) * 255.0).round() as u8;
            self.blend_pixel_unchecked(x, y, color, cov);
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
        if color[3] == 0 || rect.w <= 0 || rect.h <= 0 {
            return;
        }
        let eff = self.effective_clip(rect);
        if eff.w <= 0 || eff.h <= 0 {
            return;
        }
        let stride = usize::try_from(self.w).unwrap_or(0) * 4;
        let x0 = usize::try_from(eff.x).unwrap_or(0) * 4;
        let x1 = usize::try_from(eff.right()).unwrap_or(0) * 4;
        if color[3] == 255 {
            let px_bytes = [color[2], color[1], color[0], 255];
            let first_row_off = usize::try_from(eff.y).unwrap_or(0) * stride;
            if first_row_off + x1 > self.buf.len() {
                return;
            }
            for chunk in self.buf[first_row_off + x0..first_row_off + x1]
                .as_chunks_mut::<4>()
                .0
            {
                chunk.copy_from_slice(&px_bytes);
            }
            let src_start = first_row_off + x0;
            let src_end = first_row_off + x1;
            for y in (eff.y + 1)..eff.bottom() {
                let row_off = usize::try_from(y).unwrap_or(0) * stride;
                if row_off + x1 > self.buf.len() {
                    continue;
                }
                self.buf.copy_within(src_start..src_end, row_off + x0);
            }
            return;
        }

        let alpha = u16::from(color[3]);
        let inv = 255 - alpha;
        let sb = div_255(u16::from(color[2]) * alpha);
        let sg = div_255(u16::from(color[1]) * alpha);
        let sr = div_255(u16::from(color[0]) * alpha);
        for y in eff.y..eff.bottom() {
            let row_off = usize::try_from(y).unwrap_or(0) * stride;
            if row_off + x1 > self.buf.len() {
                continue;
            }
            for dp in self.buf[row_off + x0..row_off + x1].as_chunks_mut::<4>().0 {
                dp[0] = (sb + div_255(u16::from(dp[0]) * inv)).min(255) as u8;
                dp[1] = (sg + div_255(u16::from(dp[1]) * inv)).min(255) as u8;
                dp[2] = (sr + div_255(u16::from(dp[2]) * inv)).min(255) as u8;
                dp[3] = (alpha + div_255(u16::from(dp[3]) * inv)).min(255) as u8;
            }
        }
    }

    pub(crate) fn fill_rounded_rect(&mut self, rect: Rect, radius: i32, color: [u8; 4]) {
        if color[3] == 0 || rect.w <= 0 || rect.h <= 0 {
            return;
        }
        let r = radius.min(rect.w / 2).min(rect.h / 2).max(0);
        if r == 0 {
            self.fill_rect(rect, color);
            return;
        }
        let mid_w = rect.w - 2 * r;
        let mid_h = rect.h - 2 * r;
        if mid_h > 0 {
            self.fill_rect(Rect::new(rect.x, rect.y + r, rect.w, mid_h), color);
        }
        if mid_w > 0 {
            self.fill_rect(Rect::new(rect.x + r, rect.y, mid_w, r), color);
            self.fill_rect(Rect::new(rect.x + r, rect.bottom() - r, mid_w, r), color);
        }

        let corners = [
            Rect::new(rect.x, rect.y, r, r),
            Rect::new(rect.right() - r, rect.y, r, r),
            Rect::new(rect.x, rect.bottom() - r, r, r),
            Rect::new(rect.right() - r, rect.bottom() - r, r, r),
        ];
        for c_rect in corners {
            let eff = self.effective_clip(c_rect);
            if eff.w <= 0 || eff.h <= 0 {
                continue;
            }
            for y in eff.y..eff.bottom() {
                for x in eff.x..eff.right() {
                    let cov = rounded_rect_coverage(x, y, rect, r);
                    if cov > 0 {
                        self.blend_pixel_unchecked(x, y, color, cov);
                    }
                }
            }
        }
    }

    pub(crate) fn stroke_rounded_rect(&mut self, rect: Rect, radius: i32, color: [u8; 4]) {
        if color[3] == 0 || rect.w <= 2 || rect.h <= 2 {
            return;
        }
        let r = radius.min(rect.w / 2).min(rect.h / 2).max(0);
        if r == 0 {
            self.fill_rect(Rect::new(rect.x, rect.y, rect.w, 1), color);
            self.fill_rect(Rect::new(rect.x, rect.bottom() - 1, rect.w, 1), color);
            self.fill_rect(Rect::new(rect.x, rect.y + 1, 1, rect.h - 2), color);
            self.fill_rect(
                Rect::new(rect.right() - 1, rect.y + 1, 1, rect.h - 2),
                color,
            );
            return;
        }

        let mid_w = rect.w - 2 * r;
        let mid_h = rect.h - 2 * r;
        if mid_w > 0 {
            self.fill_rect(Rect::new(rect.x + r, rect.y, mid_w, 1), color);
            self.fill_rect(Rect::new(rect.x + r, rect.bottom() - 1, mid_w, 1), color);
        }
        if mid_h > 0 {
            self.fill_rect(Rect::new(rect.x, rect.y + r, 1, mid_h), color);
            self.fill_rect(Rect::new(rect.right() - 1, rect.y + r, 1, mid_h), color);
        }

        let inner = Rect::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2);
        let ir = (r - 1).max(0);
        let corners = [
            Rect::new(rect.x, rect.y, r, r),
            Rect::new(rect.right() - r, rect.y, r, r),
            Rect::new(rect.x, rect.bottom() - r, r, r),
            Rect::new(rect.right() - r, rect.bottom() - r, r, r),
        ];
        for c_rect in corners {
            let eff = self.effective_clip(c_rect);
            if eff.w <= 0 || eff.h <= 0 {
                continue;
            }
            for y in eff.y..eff.bottom() {
                for x in eff.x..eff.right() {
                    let outer_cov = rounded_rect_coverage(x, y, rect, r);
                    let inner_cov = rounded_rect_coverage(x, y, inner, ir);
                    let ring = outer_cov.saturating_sub(inner_cov);
                    if ring > 0 {
                        self.blend_pixel_unchecked(x, y, color, ring);
                    }
                }
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub(crate) fn fill_rounded_rect_f(
        &mut self,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: [u8; 4],
    ) {
        if color[3] == 0 {
            return;
        }
        let cx = left + width * 0.5;
        let cy = top + height * 0.5;
        let bx0 = left.floor() as i32 - 1;
        let by0 = top.floor() as i32 - 1;
        let bx1 = (left + width).ceil() as i32 + 1;
        let by1 = (top + height).ceil() as i32 + 1;
        let eff = self.effective_clip(Rect::new(bx0, by0, bx1 - bx0, by1 - by0));
        for py in eff.y..eff.bottom() {
            for px in eff.x..eff.right() {
                let dist = sd_rounded_box(
                    px as f32 + 0.5 - cx,
                    py as f32 + 0.5 - cy,
                    width,
                    height,
                    radius,
                );
                self.blend_sdf_clipped(px, py, color, dist);
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::too_many_arguments
    )]
    pub(crate) fn fill_rounded_rect_vgrad_f(
        &mut self,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        radius: f32,
        top_col: [u8; 4],
        bot_col: [u8; 4],
    ) {
        let cx = left + width * 0.5;
        let cy = top + height * 0.5;
        let bx0 = left.floor() as i32 - 1;
        let by0 = top.floor() as i32 - 1;
        let bx1 = (left + width).ceil() as i32 + 1;
        let by1 = (top + height).ceil() as i32 + 1;
        let eff = self.effective_clip(Rect::new(bx0, by0, bx1 - bx0, by1 - by0));
        let inv_h = if height > 0.1 { 1.0 / height } else { 0.0 };
        for py in eff.y..eff.bottom() {
            let mix = (((py as f32 + 0.5) - top) * inv_h).clamp(0.0, 1.0);
            let col = lerp_rgba(top_col, bot_col, mix);
            if col[3] == 0 {
                continue;
            }
            for px in eff.x..eff.right() {
                let dist = sd_rounded_box(
                    px as f32 + 0.5 - cx,
                    py as f32 + 0.5 - cy,
                    width,
                    height,
                    radius,
                );
                self.blend_sdf_clipped(px, py, col, dist);
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::too_many_arguments
    )]
    pub(crate) fn stroke_rounded_rect_f(
        &mut self,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        radius: f32,
        thickness: f32,
        color: [u8; 4],
    ) {
        if color[3] == 0 {
            return;
        }
        let cx = left + width * 0.5;
        let cy = top + height * 0.5;
        let half_t = thickness * 0.5;
        let pad = half_t.ceil() as i32 + 1;
        let bx0 = left.floor() as i32 - pad;
        let by0 = top.floor() as i32 - pad;
        let bx1 = (left + width).ceil() as i32 + pad;
        let by1 = (top + height).ceil() as i32 + pad;
        let eff = self.effective_clip(Rect::new(bx0, by0, bx1 - bx0, by1 - by0));
        for py in eff.y..eff.bottom() {
            for px in eff.x..eff.right() {
                let dist = sd_rounded_box(
                    px as f32 + 0.5 - cx,
                    py as f32 + 0.5 - cy,
                    width,
                    height,
                    radius,
                )
                .abs()
                    - half_t;
                self.blend_sdf_clipped(px, py, color, dist);
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn fill_circle(&mut self, cx: i32, cy: i32, radius: f32, color: [u8; 4]) {
        self.fill_circle_f(cx as f32, cy as f32, radius, color);
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub(crate) fn fill_circle_f(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        let x0 = ((cx - radius).floor() as i32 - 1).max(0);
        let y0 = ((cy - radius).floor() as i32 - 1).max(0);
        let x1 = ((cx + radius).ceil() as i32 + 2).min(self.w);
        let y1 = ((cy + radius).ceil() as i32 + 2).min(self.h);
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = (x as f32 + 0.5) - cx;
                let dy = (y as f32 + 0.5) - cy;
                self.blend_sdf(x, y, color, dx.hypot(dy) - radius);
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub(crate) fn stroke_circle_f(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        thickness: f32,
        color: [u8; 4],
    ) {
        let half_t = thickness * 0.5;
        let bound = radius + half_t + 1.5;
        let x0 = ((cx - bound).floor() as i32).max(0);
        let y0 = ((cy - bound).floor() as i32).max(0);
        let x1 = ((cx + bound).ceil() as i32 + 1).min(self.w);
        let y1 = ((cy + bound).ceil() as i32 + 1).min(self.h);
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = (x as f32 + 0.5) - cx;
                let dy = (y as f32 + 0.5) - cy;
                let dist = (dx.hypot(dy) - radius).abs() - half_t;
                self.blend_sdf(x, y, color, dist);
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub(crate) fn stroke_ellipse_aa(
        &mut self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        thickness: f32,
        color: [u8; 4],
    ) {
        let half_t = thickness * 0.5;
        let x0 = ((cx - rx - half_t - 1.5).floor() as i32).max(0);
        let y0 = ((cy - ry - half_t - 1.5).floor() as i32).max(0);
        let x1 = ((cx + rx + half_t + 2.0).ceil() as i32).min(self.w);
        let y1 = ((cy + ry + half_t + 2.0).ceil() as i32).min(self.h);
        let horiz_inv_sq = 1.0 / (rx * rx);
        let vert_inv_sq = 1.0 / (ry * ry);
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = (x as f32 + 0.5) - cx;
                let dy = (y as f32 + 0.5) - cy;
                let f = dx * dx * horiz_inv_sq + dy * dy * vert_inv_sq - 1.0;
                let gx = 2.0 * dx * horiz_inv_sq;
                let gy = 2.0 * dy * vert_inv_sq;
                let grad = gx.hypot(gy).max(0.001);
                self.blend_sdf(x, y, color, (f / grad).abs() - half_t);
            }
        }
    }

    pub(crate) fn stroke_line_aa(
        &mut self,
        ax: f32,
        ay: f32,
        bx: f32,
        by: f32,
        thickness: f32,
        color: [u8; 4],
    ) {
        self.stroke_polyline_aa(&[(ax, ay), (bx, by)], false, thickness, color);
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub(crate) fn stroke_polyline_aa(
        &mut self,
        pts: &[(f32, f32)],
        closed: bool,
        thickness: f32,
        color: [u8; 4],
    ) {
        if pts.len() < 2 {
            return;
        }
        let (min_x, min_y, max_x, max_y) = poly_bounds(pts);
        let half_t = thickness * 0.5;
        let pad = half_t + 1.5;
        let x0 = ((min_x - pad).floor() as i32).max(0);
        let y0 = ((min_y - pad).floor() as i32).max(0);
        let x1 = ((max_x + pad).ceil() as i32 + 1).min(self.w);
        let y1 = ((max_y + pad).ceil() as i32 + 1).min(self.h);
        let seg_count = if closed { pts.len() } else { pts.len() - 1 };

        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut best = f32::MAX;
                for i in 0..seg_count {
                    let (ax, ay) = pts[i];
                    let (bx, by) = pts[(i + 1) % pts.len()];
                    best = best.min(dist_to_segment(px, py, ax, ay, bx, by));
                }
                self.blend_sdf(x, y, color, best - half_t);
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    pub(crate) fn fill_poly_aa(&mut self, pts: &[(f32, f32)], color: [u8; 4]) {
        if pts.len() < 3 {
            return;
        }
        let (min_x, min_y, max_x, max_y) = poly_bounds(pts);
        let x0 = (min_x.floor() as i32).max(0);
        let y0 = (min_y.floor() as i32).max(0);
        let x1 = (max_x.ceil() as i32 + 1).min(self.w);
        let y1 = (max_y.ceil() as i32 + 1).min(self.h);

        for y in y0..y1 {
            for x in x0..x1 {
                let mut inside = 0_u16;
                for &(ox, oy) in &RGSS_16 {
                    if point_in_poly(x as f32 + ox, y as f32 + oy, pts) {
                        inside += 1;
                    }
                }
                if inside > 0 {
                    let cov = ((inside * 255) / 16) as u8;
                    self.blend_pixel(x, y, color, cov);
                }
            }
        }
    }

    pub(crate) fn draw_scrollbar(
        &mut self,
        rect: Rect,
        scroll_y: i32,
        total_h: i32,
        color: [u8; 4],
    ) {
        if color[3] == 0 || total_h <= rect.h || rect.h <= 16 {
            return;
        }
        let track_h = rect.h - 8;
        let thumb_h = ((rect.h * track_h) / total_h).max(20).min(track_h);
        let max_scroll = (total_h - rect.h).max(1);
        let offset = ((scroll_y.clamp(0, max_scroll) * (track_h - thumb_h)) / max_scroll)
            .clamp(0, track_h - thumb_h);
        let tx = rect.right() - 6;
        let ty = rect.y + 4 + offset;
        #[allow(clippy::cast_precision_loss)]
        self.fill_rounded_rect_f(tx as f32, ty as f32, 3.0, thumb_h as f32, 1.5, color);
    }

    #[allow(clippy::cast_sign_loss)]
    pub(crate) fn clip_rounded_corners(&mut self, rect: Rect, radius: i32) {
        let r = radius.min(rect.w / 2).min(rect.h / 2).max(0);
        if r <= 0 {
            return;
        }
        let regions = [
            (rect.x, rect.y, r, r),
            (rect.right() - r, rect.y, r, r),
            (rect.x, rect.bottom() - r, r, r),
            (rect.right() - r, rect.bottom() - r, r, r),
        ];
        for (rx, ry, rw, rh) in regions {
            let x0 = rx.max(0);
            let y0 = ry.max(0);
            let x1 = (rx + rw).min(self.w);
            let y1 = (ry + rh).min(self.h);
            for y in y0..y1 {
                for x in x0..x1 {
                    let cov = rounded_rect_coverage(x, y, rect, r);
                    if cov < 255 {
                        let idx = ((y * self.w + x) * 4) as usize;
                        if idx + 3 < self.buf.len() {
                            for ch in 0..4 {
                                self.buf[idx + ch] = u8::try_from(div_255(
                                    u16::from(self.buf[idx + ch]) * u16::from(cov),
                                ))
                                .unwrap_or(255);
                            }
                        }
                    }
                }
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss
    )]
    pub(crate) fn draw_text(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
        clip: Rect,
    ) {
        if text.is_empty() || color[3] == 0 {
            return;
        }
        let mut eff = intersect_rect(clip, Rect::new(0, 0, self.w, self.h));
        if let Some(c) = self.clip {
            eff = intersect_rect(eff, c);
        }
        if eff.w <= 0 || eff.h <= 0 || y >= eff.bottom() {
            return;
        }
        let mut cursor_x = x;
        let baseline_y = y + (size_px * 0.82).round() as i32;

        for ch in text.chars() {
            if cursor_x >= eff.right() {
                break;
            }
            let adv = with_cached_glyph(ch, size_px, bold, |g| {
                let gx0 = cursor_x + g.xmin;
                let gy0 = baseline_y - g.ymin - g.height as i32;
                let x_start = gx0.max(eff.x);
                let x_end = (gx0 + g.width as i32).min(eff.right());
                let y_start = gy0.max(eff.y);
                let y_end = (gy0 + g.height as i32).min(eff.bottom());

                if x_start < x_end && y_start < y_end {
                    let col_start = (x_start - gx0) as usize;
                    let span = (x_end - x_start) as usize;
                    let dst_x0 = (x_start as usize) * 4;
                    let dst_x1 = dst_x0 + span * 4;
                    let dst_stride = (self.w as usize) * 4;
                    let ca = u16::from(color[3]);
                    let cr = u16::from(color[0]);
                    let cg = u16::from(color[1]);
                    let cb = u16::from(color[2]);
                    for py in y_start..y_end {
                        let row = (py - gy0) as usize;
                        let src_off = row * g.width + col_start;
                        let dst_off = (py as usize) * dst_stride;
                        if src_off + span > g.bitmap.len() || dst_off + dst_x1 > self.buf.len() {
                            continue;
                        }
                        let cov_row = &g.bitmap[src_off..src_off + span];
                        let dst_row = &mut self.buf[dst_off + dst_x0..dst_off + dst_x1];
                        for (dp, &cov) in dst_row.as_chunks_mut::<4>().0.iter_mut().zip(cov_row) {
                            if cov == 0 {
                                continue;
                            }
                            let a_t = ca * u16::from(cov);
                            let alpha = (a_t + 1 + (a_t >> 8)) >> 8;
                            if alpha == 0 {
                                continue;
                            }
                            if alpha == 255 {
                                *dp = [color[2], color[1], color[0], 255];
                                continue;
                            }
                            let inv = 255 - alpha;
                            let blue_s = cb * alpha;
                            let green_s = cg * alpha;
                            let red_s = cr * alpha;
                            let blue_d = u16::from(dp[0]) * inv;
                            let green_d = u16::from(dp[1]) * inv;
                            let red_d = u16::from(dp[2]) * inv;
                            let alpha_d = u16::from(dp[3]) * inv;
                            dp[0] = (((blue_s + 1 + (blue_s >> 8)) >> 8)
                                + ((blue_d + 1 + (blue_d >> 8)) >> 8))
                                .min(255) as u8;
                            dp[1] = (((green_s + 1 + (green_s >> 8)) >> 8)
                                + ((green_d + 1 + (green_d >> 8)) >> 8))
                                .min(255) as u8;
                            dp[2] = (((red_s + 1 + (red_s >> 8)) >> 8)
                                + ((red_d + 1 + (red_d >> 8)) >> 8))
                                .min(255) as u8;
                            dp[3] = (alpha + ((alpha_d + 1 + (alpha_d >> 8)) >> 8)).min(255) as u8;
                        }
                    }
                }
                g.advance
            });
            let Some(step) = adv else {
                return;
            };
            cursor_x += step;
        }
    }

    pub(crate) fn measure_text(text: &str, size_px: f32, bold: bool) -> i32 {
        let mut total = 0_i32;
        for ch in text.chars() {
            if let Some(adv) = with_cached_glyph(ch, size_px, bold, |g| g.advance) {
                total += adv;
            } else {
                return i32::try_from(text.len()).unwrap_or(0) * 7;
            }
        }
        total
    }

    pub(crate) fn fit_text(text: &str, max_w: i32, size_px: f32, bold: bool) -> String {
        if max_w <= 10 {
            return String::new();
        }
        if Self::measure_text(text, size_px, bold) <= max_w {
            return text.to_owned();
        }
        let mut out = String::new();
        let ellipsis = "...";
        let ell_w = Self::measure_text(ellipsis, size_px, bold);
        let mut acc = 0;
        for ch in text.chars() {
            let mut single = [0_u8; 4];
            let s = ch.encode_utf8(&mut single);
            let cw = Self::measure_text(s, size_px, bold);
            if acc + cw + ell_w > max_w {
                break;
            }
            out.push(ch);
            acc += cw;
        }
        out.push_str(ellipsis);
        out
    }

    /// Fits the trailing end of `text` within `max_w` (prepending `...`) for active text inputs.
    pub(crate) fn fit_text_tail(text: &str, max_w: i32, size_px: f32, bold: bool) -> String {
        if max_w <= 10 {
            return String::new();
        }
        if Self::measure_text(text, size_px, bold) <= max_w {
            return text.to_owned();
        }
        let ellipsis = "...";
        let ell_w = Self::measure_text(ellipsis, size_px, bold);
        let mut kept: Vec<char> = Vec::new();
        let mut acc = 0;
        for ch in text.chars().rev() {
            let mut single = [0_u8; 4];
            let s = ch.encode_utf8(&mut single);
            let cw = Self::measure_text(s, size_px, bold);
            if acc + cw + ell_w > max_w {
                break;
            }
            kept.push(ch);
            acc += cw;
        }
        kept.reverse();
        let mut out = String::from(ellipsis);
        for ch in kept {
            out.push(ch);
        }
        out
    }
}

pub(crate) fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn lerp_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let inv = 1.0 - t;
    [
        (f32::from(a[0]) * inv + f32::from(b[0]) * t).round() as u8,
        (f32::from(a[1]) * inv + f32::from(b[1]) * t).round() as u8,
        (f32::from(a[2]) * inv + f32::from(b[2]) * t).round() as u8,
        (f32::from(a[3]) * inv + f32::from(b[3]) * t).round() as u8,
    ]
}

pub(crate) fn sd_rounded_box(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let qx = px.abs() - (w * 0.5 - r);
    let qy = py.abs() - (h * 0.5 - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

pub(crate) fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let vx = bx - ax;
    let vy = by - ay;
    let len_sq = vx * vx + vy * vy;
    let t = if len_sq > 1e-6 {
        (((px - ax) * vx + (py - ay) * vy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (px - (ax + vx * t)).hypot(py - (ay + vy * t))
}

pub(crate) fn poly_bounds(pts: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for &(x, y) in pts {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x, max_y)
}

pub(crate) fn point_in_poly(px: f32, py: f32, pts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = pts.len().saturating_sub(1);
    for (i, &(xi, yi)) in pts.iter().enumerate() {
        let (xj, yj) = pts[j];
        let crosses = ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi);
        if crosses {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn rounded_rect_coverage(x: i32, y: i32, rect: Rect, r: i32) -> u8 {
    if x < rect.x || y < rect.y || x >= rect.right() || y >= rect.bottom() {
        return 0;
    }
    if r <= 0 {
        return 255;
    }
    let rx0 = rect.x + r;
    let rx1 = rect.right() - r;
    let ry0 = rect.y + r;
    let ry1 = rect.bottom() - r;

    let offset_x = if x < rx0 {
        (rx0 - x) as f32 - 0.5
    } else if x >= rx1 {
        (x - rx1) as f32 + 0.5
    } else {
        return 255;
    };
    let offset_y = if y < ry0 {
        (ry0 - y) as f32 - 0.5
    } else if y >= ry1 {
        (y - ry1) as f32 + 0.5
    } else {
        return 255;
    };
    let dist = offset_x.hypot(offset_y) - (r as f32);
    if dist <= -0.5 {
        255
    } else if dist >= 0.5 {
        0
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cov = ((0.5 - dist) * 255.0).round() as u8;
        cov
    }
}
