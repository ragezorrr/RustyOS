//! Pure CPU premultiplied `ARGB8888` renderer for `rusty-files` (`RUSTYOS-065`, ADR-0030).
//!
//! Implements the macOS 27 Golden Gate light appearance (default) and RustyOS
//! Purple Glass dark appearance using bundled `Inter` fonts (`fontdue`) and
//! subpixel SDF / supersampled vector icons with strict scissor clipping.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};
use rusty_services::file_manager::{
    DecodedPreviewImage, FileEntry, FileSortField, FileSortOrder, FileViewMode, FilesThemeMode,
    format_byte_size, format_unix_timestamp,
};

use crate::dialogs::render_dialog_overlays;
use crate::icons::{
    draw_chevron_aa, draw_ellipsis_glyph, draw_entry_icon, draw_folder_plus_glyph,
    draw_search_glyph, draw_sidebar_icon, draw_sort_pill_glyph, draw_tag_outline_glyph,
    draw_view_mode_icon,
};
use crate::layout::{
    ColumnLayout, FilesLayout, PopupLayout, PreviewLayout, Rect, SIDEBAR_CORNER_RADIUS,
    WINDOW_CORNER_RADIUS,
};
use crate::state::{FilesAppState, LIST_ROW_HEIGHT, ROW_HEIGHT, sidebar_total_height};
use crate::views::{render_gallery_view, render_icons_grid, render_path_bar, render_tab_bar};

const INTER_REGULAR_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

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

fn regular_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| Font::from_bytes(INTER_REGULAR_BYTES, FontSettings::default()).ok())
        .as_ref()
}

fn semibold_font() -> Option<&'static Font> {
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub window_bg: [u8; 4],
    pub window_border: [u8; 4],
    pub sidebar_bg: [u8; 4],
    pub sidebar_border: [u8; 4],
    pub sidebar_active: [u8; 4],
    pub pill_bg: [u8; 4],
    pub pill_border: [u8; 4],
    pub pill_active: [u8; 4],
    pub divider: [u8; 4],
    pub scrollbar: [u8; 4],
    pub accent: [u8; 4],
    pub accent_text: [u8; 4],
    pub secondary_select: [u8; 4],
    pub text_primary: [u8; 4],
    pub text_secondary: [u8; 4],
    pub text_muted: [u8; 4],
    pub row_alt: [u8; 4],
    pub popup_bg: [u8; 4],
    pub popup_border: [u8; 4],
    pub is_dark: bool,
}

pub(crate) const fn rgba(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    [r, g, b, a]
}

pub(crate) const fn palette_for(theme: FilesThemeMode) -> Palette {
    match theme {
        FilesThemeMode::LightGoldenGate => Palette {
            window_bg: rgba(255, 255, 255, 255),
            window_border: rgba(210, 213, 222, 255),
            sidebar_bg: rgba(241, 243, 247, 252),
            sidebar_border: rgba(224, 227, 234, 255),
            sidebar_active: rgba(221, 226, 235, 255),
            pill_bg: rgba(249, 250, 252, 255),
            pill_border: rgba(218, 222, 230, 255),
            pill_active: rgba(229, 232, 239, 255),
            divider: rgba(232, 234, 240, 255),
            scrollbar: rgba(140, 148, 162, 115),
            accent: rgba(0, 122, 255, 255),
            accent_text: rgba(255, 255, 255, 255),
            secondary_select: rgba(229, 232, 239, 255),
            text_primary: rgba(29, 31, 36, 255),
            text_secondary: rgba(102, 110, 124, 255),
            text_muted: rgba(145, 152, 166, 255),
            row_alt: rgba(247, 248, 251, 255),
            popup_bg: rgba(250, 251, 253, 255),
            popup_border: rgba(204, 209, 219, 255),
            is_dark: false,
        },
        FilesThemeMode::DarkPurpleGlass => Palette {
            window_bg: rgba(29, 22, 43, 255),
            window_border: rgba(76, 63, 102, 255),
            sidebar_bg: rgba(41, 32, 58, 252),
            sidebar_border: rgba(69, 56, 94, 255),
            sidebar_active: rgba(65, 52, 91, 255),
            pill_bg: rgba(46, 36, 65, 255),
            pill_border: rgba(82, 68, 110, 255),
            pill_active: rgba(75, 61, 103, 255),
            divider: rgba(54, 43, 74, 255),
            scrollbar: rgba(176, 162, 208, 115),
            accent: rgba(165, 110, 255, 255),
            accent_text: rgba(255, 255, 255, 255),
            secondary_select: rgba(57, 45, 79, 255),
            text_primary: rgba(244, 240, 250, 255),
            text_secondary: rgba(184, 175, 201, 255),
            text_muted: rgba(141, 130, 164, 255),
            row_alt: rgba(34, 27, 50, 255),
            popup_bg: rgba(44, 34, 62, 255),
            popup_border: rgba(94, 78, 124, 255),
            is_dark: true,
        },
    }
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

    fn effective_clip(&self, bounds: Rect) -> Rect {
        let mut eff = intersect_rect(bounds, Rect::new(0, 0, self.w, self.h));
        if let Some(sc) = self.clip {
            eff = intersect_rect(eff, sc);
        }
        eff
    }

    fn clear(&mut self) {
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
    fn blit_premul_sprite_clipped(
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
    fn blend_pixel_unchecked(&mut self, x: i32, y: i32, color: [u8; 4], cov: u8) {
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
    fn blend_pixel(&mut self, x: i32, y: i32, color: [u8; 4], cov: u8) {
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
    fn blend_sdf(&mut self, x: i32, y: i32, color: [u8; 4], signed_dist: f32) {
        if signed_dist < 0.55 {
            let cov = ((0.55 - signed_dist).clamp(0.0, 1.0) * 255.0).round() as u8;
            self.blend_pixel(x, y, color, cov);
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn blend_sdf_clipped(&mut self, x: i32, y: i32, color: [u8; 4], signed_dist: f32) {
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
    fn fill_circle(&mut self, cx: i32, cy: i32, radius: f32, color: [u8; 4]) {
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
        if total_h <= rect.h || rect.h <= 16 {
            return;
        }
        let track_h = rect.h - 8;
        let thumb_h = ((rect.h * track_h) / total_h).max(20).min(track_h);
        let max_scroll = (total_h - rect.h).max(1);
        let offset = ((scroll_y.clamp(0, max_scroll) * (track_h - thumb_h)) / max_scroll)
            .clamp(0, track_h - thumb_h);
        let tx = rect.right() - 5;
        let ty = rect.y + 4 + offset;
        #[allow(clippy::cast_precision_loss)]
        self.fill_rounded_rect_f(tx as f32, ty as f32, 2.6, thumb_h as f32, 1.3, color);
    }

    #[allow(clippy::cast_sign_loss)]
    fn clip_rounded_corners(&mut self, rect: Rect, radius: i32) {
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

fn intersect_rect(a: Rect, b: Rect) -> Rect {
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

fn sd_rounded_box(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let qx = px.abs() - (w * 0.5 - r);
    let qy = py.abs() - (h * 0.5 - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
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

fn poly_bounds(pts: &[(f32, f32)]) -> (f32, f32, f32, f32) {
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

fn point_in_poly(px: f32, py: f32, pts: &[(f32, f32)]) -> bool {
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
fn rounded_rect_coverage(x: i32, y: i32, rect: Rect, r: i32) -> u8 {
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

/// Renders one complete frame of `rusty-files` (including inline fallback popups) for tests.
#[cfg(test)]
pub(crate) fn render_window_pixels(
    buf: &mut [u8],
    width: u32,
    height: u32,
    state: &FilesAppState,
    layout: &FilesLayout,
) {
    render_window_pixels_with_popup(buf, width, height, state, layout, true);
}

/// Renders one complete frame of `rusty-files` into an `ARGB8888` SHM buffer, optionally
/// omitting inline popups when native Wayland `xdg_popup` surfaces are active.
pub(crate) fn render_window_pixels_with_popup(
    buf: &mut [u8],
    width: u32,
    height: u32,
    state: &FilesAppState,
    layout: &FilesLayout,
    include_inline_popup: bool,
) {
    let pal = palette_for(state.theme);
    let mut c = Canvas::new(buf, width, height);
    c.clear();

    let win_rect = Rect::new(0, 0, layout.width, layout.height);
    c.fill_rounded_rect(win_rect, WINDOW_CORNER_RADIUS, pal.window_bg);

    render_sidebar(&mut c, state, layout, &pal);
    render_toolbar(&mut c, state, layout, &pal);

    if let Some(tb) = &layout.tab_bar {
        render_tab_bar(&mut c, tb, &pal);
    }

    if let Some(banner) = layout.trash_banner_rect {
        render_trash_banner(&mut c, layout, banner, &pal);
    }

    match state.view_mode {
        FileViewMode::Icons => {
            if let Some(grid) = &layout.icons_grid {
                render_icons_grid(&mut c, state, grid, &pal);
            }
        }
        FileViewMode::Gallery => {
            if let Some(gal) = &layout.gallery {
                render_gallery_view(&mut c, state, gal, &pal);
            }
        }
        FileViewMode::List => {
            render_list_view(&mut c, state, layout, &pal);
        }
        FileViewMode::Columns => {
            render_miller_columns(&mut c, state, layout, &pal);
            if let Some(preview) = &layout.preview {
                render_preview_inspector(&mut c, state, preview, &pal);
            }
        }
    }

    render_status_bar(&mut c, state, layout, &pal);
    render_dialog_overlays(&mut c, state, layout, &pal);

    if include_inline_popup && let Some(popup) = &layout.popup {
        render_popup(&mut c, popup, None, &pal);
    }

    // Clip any outer-edge corner bleed before drawing crisp CSD hairline ring.
    c.clip_rounded_corners(win_rect, WINDOW_CORNER_RADIUS);
    c.stroke_rounded_rect(win_rect, WINDOW_CORNER_RADIUS, pal.window_border);
}

fn render_sidebar(c: &mut Canvas<'_>, state: &FilesAppState, layout: &FilesLayout, pal: &Palette) {
    c.fill_rounded_rect(layout.sidebar_rect, SIDEBAR_CORNER_RADIUS, pal.sidebar_bg);
    c.stroke_rounded_rect(
        layout.sidebar_rect,
        SIDEBAR_CORNER_RADIUS,
        pal.sidebar_border,
    );

    // Golden Gate traffic lights inside floating sidebar header (strictly above sidebar_clip_rect)
    c.fill_circle_f(29.0, 30.0, 6.0, rgba(255, 95, 87, 255));
    c.fill_circle_f(49.0, 30.0, 6.0, rgba(254, 188, 46, 255));
    c.fill_circle_f(69.0, 30.0, 6.0, rgba(40, 200, 64, 255));

    let clip = layout.sidebar_clip_rect;
    c.with_clip(clip, |c| {
        for row in &layout.sidebar_rows {
            let item = &state.sidebar_items[row.index];
            if let Some(hdr_y) = row.section_header_y
                && let Some(section) = item.section
            {
                c.draw_text(
                    section,
                    row.rect.x + 8,
                    hdr_y,
                    11.0,
                    true,
                    pal.text_muted,
                    clip,
                );
            }

            let selected = state.active_sidebar.as_ref() == Some(&item.location);
            if selected {
                c.fill_rounded_rect(row.rect, 7, pal.sidebar_active);
            }
            #[allow(clippy::cast_precision_loss)]
            draw_sidebar_icon(
                c,
                row.rect.x as f32 + 15.0,
                row.rect.y as f32 + (row.rect.h as f32) * 0.5,
                item.icon,
                pal,
            );
            let label = Canvas::fit_text(&item.label, row.rect.w - 36, 13.0, selected);
            c.draw_text(
                &label,
                row.rect.x + 29,
                row.rect.y + 4,
                13.0,
                selected,
                pal.text_primary,
                clip,
            );
        }
        let total_h = sidebar_total_height(&state.sidebar_items);
        c.draw_scrollbar(clip, state.sidebar_scroll_y, total_h, pal.scrollbar);
    });
}

fn render_toolbar(c: &mut Canvas<'_>, state: &FilesAppState, layout: &FilesLayout, pal: &Palette) {
    c.fill_rounded_rect(layout.nav_pill_rect, 16, pal.pill_bg);
    c.stroke_rounded_rect(layout.nav_pill_rect, 16, pal.pill_border);
    c.fill_rect(
        Rect::new(
            layout.nav_pill_rect.x + 34,
            layout.nav_pill_rect.y + 7,
            1,
            18,
        ),
        pal.divider,
    );
    let back_col = if state.can_go_back() {
        pal.text_primary
    } else {
        pal.text_muted
    };
    let fwd_col = if state.can_go_forward() {
        pal.text_primary
    } else {
        pal.text_muted
    };
    #[allow(clippy::cast_precision_loss)]
    {
        draw_chevron_aa(
            c,
            layout.nav_back_rect.x as f32 + 17.5,
            layout.nav_back_rect.y as f32 + 16.0,
            true,
            2.6,
            4.8,
            1.5,
            back_col,
        );
        draw_chevron_aa(
            c,
            layout.nav_forward_rect.x as f32 + 16.5,
            layout.nav_forward_rect.y as f32 + 16.0,
            false,
            2.6,
            4.8,
            1.5,
            fwd_col,
        );
    }

    if layout.title_max_x - layout.title_x >= 28 {
        let title_clip = Rect::new(layout.title_x, 12, layout.title_max_x - layout.title_x, 38);
        let title = Canvas::fit_text(&state.window_title, title_clip.w, 15.0, true);
        c.draw_text(
            &title,
            layout.title_x,
            21,
            15.0,
            true,
            pal.text_primary,
            title_clip,
        );
    }
    render_toolbar_right_cluster(c, state, layout, pal);
}

#[allow(clippy::cast_precision_loss)]
fn render_toolbar_right_cluster(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    // 4-segment View Switcher pill (`Icons` | `List` | `Columns` | `Gallery`)
    c.fill_rounded_rect(layout.view_pill_rect, 16, pal.pill_bg);
    c.stroke_rounded_rect(layout.view_pill_rect, 16, pal.pill_border);
    for (mode, seg_rect) in layout.view_segments {
        let active = state.view_mode == mode;
        if active {
            let pill = Rect::new(
                seg_rect.x + 2,
                seg_rect.y + 2,
                seg_rect.w - 4,
                seg_rect.h - 4,
            );
            c.fill_rounded_rect(pill, 14, pal.pill_active);
            c.stroke_rounded_rect(pill, 14, pal.pill_border);
        }
        let col = if active {
            pal.text_primary
        } else {
            pal.text_secondary
        };
        draw_view_mode_icon(
            c,
            seg_rect.x as f32 + (seg_rect.w as f32) * 0.5,
            seg_rect.y as f32 + (seg_rect.h as f32) * 0.5,
            mode,
            col,
        );
    }

    // Sort pill
    c.fill_rounded_rect(layout.sort_pill_rect, 16, pal.pill_bg);
    c.stroke_rounded_rect(layout.sort_pill_rect, 16, pal.pill_border);
    draw_sort_pill_glyph(
        c,
        layout.sort_pill_rect.x as f32,
        layout.sort_pill_rect.y as f32 + 16.0,
        pal.text_primary,
    );

    // Actions pill (`New Folder` | `Tag` | `...`)
    c.fill_rounded_rect(layout.action_pill_rect, 16, pal.pill_bg);
    c.stroke_rounded_rect(layout.action_pill_rect, 16, pal.pill_border);
    draw_folder_plus_glyph(
        c,
        layout.action_primary_rect.x as f32 + 17.5,
        layout.action_primary_rect.y as f32 + 16.0,
        pal.text_primary,
    );
    draw_tag_outline_glyph(
        c,
        layout.action_tag_rect.x as f32 + 17.0,
        layout.action_tag_rect.y as f32 + 16.0,
        pal.text_primary,
    );
    draw_ellipsis_glyph(
        c,
        layout.action_more_rect.x as f32 + 16.5,
        layout.action_more_rect.y as f32 + 16.0,
        pal.text_primary,
    );

    render_toolbar_search_pill(c, state, layout, pal);
}

#[allow(clippy::cast_precision_loss)]
fn render_toolbar_search_pill(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    let pill = layout.search_pill_rect;
    if state.search_active {
        c.fill_rounded_rect(
            Rect::new(pill.x - 1, pill.y - 1, pill.w + 2, pill.h + 2),
            17,
            rgba(pal.accent[0], pal.accent[1], pal.accent[2], 70),
        );
        c.fill_rounded_rect(pill, 16, pal.window_bg);
        c.stroke_rounded_rect(pill, 16, pal.accent);
    } else {
        c.fill_rounded_rect(pill, 16, pal.pill_bg);
        c.stroke_rounded_rect(pill, 16, pal.pill_border);
    }
    let icon_cx = if state.search_active {
        pill.x as f32 + 14.5
    } else {
        pill.x as f32 + (pill.w as f32) * 0.5 - 0.5
    };
    let icon_col = if state.search_active {
        pal.accent
    } else {
        pal.text_primary
    };
    draw_search_glyph(c, icon_cx, pill.y as f32 + 15.5, icon_col);
    if state.search_active {
        let input_clip = Rect::new(pill.x + 26, pill.y + 2, pill.w - 48, 28);
        let (fitted, col, text_x, caret_x) = if state.search_query.is_empty() {
            (
                String::from("Search..."),
                pal.text_muted,
                input_clip.x + 3,
                input_clip.x,
            )
        } else {
            let fitted =
                Canvas::fit_text_tail(&state.search_query, (input_clip.w - 4).max(10), 12.5, false);
            let w = Canvas::measure_text(&fitted, 12.5, false);
            let cx = (input_clip.x + w + 1).min(input_clip.right() - 2);
            (fitted, pal.text_primary, input_clip.x, cx)
        };
        c.draw_text(&fitted, text_x, pill.y + 8, 12.5, false, col, input_clip);
        if state.caret_visible {
            c.fill_rect(Rect::new(caret_x, pill.y + 8, 2, 16), pal.accent);
        }
        if !state.search_query.is_empty() {
            let cx = layout.search_clear_rect.x as f32 + 9.0;
            let cy = layout.search_clear_rect.y as f32 + 10.0;
            c.fill_circle_f(cx, cy, 6.2, pal.pill_active);
            c.stroke_line_aa(
                cx - 2.3,
                cy - 2.3,
                cx + 2.3,
                cy + 2.3,
                1.25,
                pal.text_secondary,
            );
            c.stroke_line_aa(
                cx + 2.3,
                cy - 2.3,
                cx - 2.3,
                cy + 2.3,
                1.25,
                pal.text_secondary,
            );
        }
    }
}

fn render_trash_banner(c: &mut Canvas<'_>, layout: &FilesLayout, banner: Rect, pal: &Palette) {
    c.fill_rounded_rect(banner, 8, pal.row_alt);
    c.draw_text(
        "Trash — deleted files remain here until you empty Trash",
        banner.x + 12,
        banner.y + 8,
        12.5,
        false,
        pal.text_secondary,
        banner,
    );
    if let Some(btn) = layout.empty_trash_btn_rect {
        c.fill_rounded_rect(btn, 12, pal.pill_bg);
        c.stroke_rounded_rect(btn, 12, pal.pill_border);
        c.draw_text(
            "Empty",
            btn.x + 16,
            btn.y + 4,
            12.0,
            true,
            pal.text_primary,
            btn,
        );
    }
}

fn render_miller_columns(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    c.with_clip(layout.content_rect, |c| {
        for col_layout in &layout.columns {
            render_miller_column_pane(c, state, layout, col_layout, pal);
        }
    });
}

fn render_miller_column_pane(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    col_layout: &ColumnLayout,
    pal: &Palette,
) {
    let Some(col) = state.columns.get(col_layout.col_idx) else {
        return;
    };
    let is_focused_col =
        col_layout.col_idx == state.active_column || col_layout.col_idx + 1 == state.columns.len();

    // Column divider stops strictly at content_rect.bottom(), above the status bar margin
    c.fill_rect(
        Rect::new(
            col_layout.rect.right() - 1,
            layout.content_rect.y,
            1,
            layout.content_rect.h,
        ),
        pal.divider,
    );

    let col_clip = Rect::new(
        col_layout.rect.x,
        layout.content_rect.y,
        col_layout.rect.w - 1,
        layout.content_rect.h,
    );

    c.with_clip(col_clip, |c| {
        if col.loading {
            c.draw_text(
                "Loading...",
                col_layout.rect.x + 14,
                col_layout.rect.y + 12,
                12.5,
                false,
                pal.text_muted,
                col_clip,
            );
            return;
        }
        if let Some(err) = &col.error {
            let msg = Canvas::fit_text(err, col_layout.rect.w - 24, 12.0, false);
            c.draw_text(
                &msg,
                col_layout.rect.x + 12,
                col_layout.rect.y + 12,
                12.0,
                false,
                rgba(220, 72, 72, 255),
                col_clip,
            );
            return;
        }
        if col.visible_entries.is_empty() {
            c.draw_text(
                "Empty",
                col_layout.rect.x + 16,
                col_layout.rect.y + 14,
                12.5,
                false,
                pal.text_muted,
                col_clip,
            );
            return;
        }

        for &(row_idx, row_rect) in &col_layout.rows {
            let Some(entry) = col.visible_entries.get(row_idx) else {
                continue;
            };
            let selected = col.selected_index == Some(row_idx);
            let primary_active = selected && is_focused_col;
            render_miller_column_row(
                c,
                state,
                entry,
                row_rect,
                col_clip,
                selected,
                primary_active,
                pal,
            );
        }

        let total_h = i32::try_from(col.visible_entries.len()).unwrap_or(0) * ROW_HEIGHT + 12;
        c.draw_scrollbar(col_clip, col.scroll_y, total_h, pal.scrollbar);
    });
}

#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
fn render_miller_column_row(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    entry: &FileEntry,
    row_rect: Rect,
    col_clip: Rect,
    selected: bool,
    primary_active: bool,
    pal: &Palette,
) {
    if selected {
        let bg = if primary_active {
            pal.accent
        } else {
            pal.secondary_select
        };
        c.fill_rounded_rect(row_rect, 7, bg);
    }
    let fg = if primary_active {
        pal.accent_text
    } else {
        pal.text_primary
    };
    draw_entry_icon(
        c,
        row_rect.x as f32 + 13.0,
        row_rect.y as f32 + (row_rect.h as f32) * 0.5,
        entry,
        primary_active,
        pal,
    );

    let tags = state.tags_for(&entry.path);
    let tags_w = i32::try_from(tags.len().min(3)).unwrap_or(0) * 9;
    let chev_w = if entry.is_navigable_dir() { 14 } else { 4 };
    let text_w = (row_rect.w - 30 - tags_w - chev_w).max(20);
    let label = Canvas::fit_text(&entry.name, text_w, 13.0, selected);
    c.draw_text(
        &label,
        row_rect.x + 26,
        row_rect.y + 5,
        13.0,
        selected,
        fg,
        col_clip,
    );

    let mut dot_x = row_rect.right() - chev_w - 6;
    for tag in tags.iter().take(3).rev() {
        let [r, g, b] = tag.rgb();
        c.fill_circle(dot_x, row_rect.y + 13, 3.5, rgba(r, g, b, 255));
        dot_x -= 9;
    }
    if entry.is_navigable_dir() {
        let chev_col = if primary_active {
            pal.accent_text
        } else {
            pal.text_muted
        };
        draw_chevron_aa(
            c,
            row_rect.right() as f32 - 8.5,
            row_rect.y as f32 + 13.0,
            false,
            1.9,
            3.5,
            1.35,
            chev_col,
        );
    }
}

#[allow(clippy::cast_precision_loss)]
fn render_preview_inspector(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    preview: &PreviewLayout,
    pal: &Palette,
) {
    let entry = &preview.entry;
    let cx = preview.rect.x + preview.rect.w / 2;
    let card_clip = preview.rect;

    c.with_clip(card_clip, |c| {
        let badge = Rect::new(cx - 32, preview.rect.y + 34, 64, 72);
        c.fill_rounded_rect(badge, 12, pal.row_alt);
        c.stroke_rounded_rect(badge, 12, pal.divider);
        draw_entry_icon(
            c,
            badge.x as f32 + 32.0,
            badge.y as f32 + 24.0,
            entry,
            false,
            pal,
        );
        let kind_w = Canvas::measure_text(entry.kind_label(), 10.5, true);
        c.draw_text(
            entry.kind_label(),
            cx - kind_w / 2,
            badge.y + 44,
            10.5,
            true,
            pal.text_secondary,
            badge,
        );

        let name = Canvas::fit_text(&entry.name, preview.rect.w - 40, 15.0, true);
        let name_w = Canvas::measure_text(&name, 15.0, true);
        c.draw_text(
            &name,
            cx - name_w / 2,
            preview.rect.y + 122,
            15.0,
            true,
            pal.text_primary,
            card_clip,
        );

        let meta_sub = format!(
            "{}  -  {}",
            entry.kind_label(),
            format_byte_size(entry.size_bytes)
        );
        let meta_w = Canvas::measure_text(&meta_sub, 12.0, false);
        c.draw_text(
            &meta_sub,
            cx - meta_w / 2,
            preview.rect.y + 146,
            12.0,
            false,
            pal.text_secondary,
            card_clip,
        );

        if let Some(secs) = entry.modified_unix_secs {
            let mod_text = format!("Modified: {}", format_unix_timestamp(secs));
            let mw = Canvas::measure_text(&mod_text, 11.5, false);
            c.draw_text(
                &mod_text,
                cx - mw / 2,
                preview.rect.y + 174,
                11.5,
                false,
                pal.text_muted,
                card_clip,
            );
        }

        render_preview_actions_and_tags(c, state, preview, pal);
    });
}

fn render_preview_actions_and_tags(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    preview: &PreviewLayout,
    pal: &Palette,
) {
    c.fill_rounded_rect(preview.open_btn, 14, pal.pill_bg);
    c.stroke_rounded_rect(preview.open_btn, 14, pal.pill_border);
    let open_w = Canvas::measure_text("Open", 12.5, true);
    c.draw_text(
        "Open",
        preview.open_btn.x + (preview.open_btn.w - open_w) / 2,
        preview.open_btn.y + 7,
        12.5,
        true,
        pal.text_primary,
        preview.open_btn,
    );

    let trash_label = if state.is_trash_view() {
        "Put Back"
    } else {
        "Move to Trash"
    };
    c.fill_rounded_rect(preview.trash_btn, 14, pal.pill_bg);
    c.stroke_rounded_rect(preview.trash_btn, 14, pal.pill_border);
    let tl_w = Canvas::measure_text(trash_label, 12.0, false);
    c.draw_text(
        trash_label,
        preview.trash_btn.x + (preview.trash_btn.w - tl_w) / 2,
        preview.trash_btn.y + 7,
        12.0,
        false,
        pal.text_primary,
        preview.trash_btn,
    );

    let active_tags = state.tags_for(&preview.entry.path);
    for (color, rect) in &preview.tag_dots {
        let [r, g, b] = color.rgb();
        c.fill_circle(rect.x + 8, rect.y + 8, 6.0, rgba(r, g, b, 255));
        if active_tags.contains(color) {
            c.fill_circle(rect.x + 8, rect.y + 8, 2.2, rgba(255, 255, 255, 255));
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn render_list_view(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    let hdr_y = layout.content_rect.y;
    for (field, rect) in &layout.list_headers {
        c.draw_text(
            field.label(),
            rect.x + 12,
            rect.y + 5,
            11.5,
            true,
            pal.text_secondary,
            *rect,
        );
        if state.sort_field == *field {
            let label_w = Canvas::measure_text(field.label(), 11.5, true);
            let cx = (rect.x + 18 + label_w) as f32;
            let cy = rect.y as f32 + 12.5;
            let pts = match state.sort_order {
                FileSortOrder::Ascending => {
                    [(cx - 3.0, cy + 1.5), (cx, cy - 1.5), (cx + 3.0, cy + 1.5)]
                }
                FileSortOrder::Descending => {
                    [(cx - 3.0, cy - 1.5), (cx, cy + 1.5), (cx + 3.0, cy - 1.5)]
                }
            };
            c.stroke_polyline_aa(&pts, false, 1.3, pal.text_secondary);
        }
    }
    c.fill_rect(
        Rect::new(
            layout.content_rect.x + 8,
            hdr_y + 25,
            layout.content_rect.w - 16,
            1,
        ),
        pal.divider,
    );

    let Some(col) = state.columns.first() else {
        return;
    };
    let clip = Rect::new(
        layout.content_rect.x,
        hdr_y + 27,
        layout.content_rect.w,
        (layout.content_rect.h - 27).max(20),
    );
    let cols_x = list_column_offsets(layout);

    c.with_clip(clip, |c| {
        for &(row_idx, rect) in &layout.list_rows {
            if let Some(entry) = col.visible_entries.get(row_idx) {
                let selected = col.selected_index == Some(row_idx);
                render_list_row(c, entry, rect, row_idx, selected, cols_x, clip, pal);
            }
        }
        let total_h = i32::try_from(col.visible_entries.len()).unwrap_or(0) * LIST_ROW_HEIGHT + 8;
        c.draw_scrollbar(clip, state.list_scroll_y, total_h, pal.scrollbar);
    });
}

fn list_column_offsets(layout: &FilesLayout) -> (i32, i32, i32) {
    let date_x = layout
        .list_headers
        .iter()
        .find(|(f, _)| *f == FileSortField::DateModified)
        .map_or(layout.content_rect.x + 280, |(_, r)| r.x);
    let size_x = layout
        .list_headers
        .iter()
        .find(|(f, _)| *f == FileSortField::Size)
        .map_or(date_x + 140, |(_, r)| r.x);
    let kind_x = layout
        .list_headers
        .iter()
        .find(|(f, _)| *f == FileSortField::Kind)
        .map_or(size_x + 78, |(_, r)| r.x);
    (date_x, size_x, kind_x)
}

#[allow(clippy::cast_precision_loss, clippy::too_many_arguments)]
fn render_list_row(
    c: &mut Canvas<'_>,
    entry: &FileEntry,
    rect: Rect,
    row_idx: usize,
    selected: bool,
    cols_x: (i32, i32, i32),
    clip: Rect,
    pal: &Palette,
) {
    let (date_x, size_x, kind_x) = cols_x;
    if selected {
        c.fill_rounded_rect(rect, 7, pal.accent);
    } else if row_idx % 2 == 1 {
        c.fill_rounded_rect(rect, 7, pal.row_alt);
    }
    let fg = if selected {
        pal.accent_text
    } else {
        pal.text_primary
    };
    let sub_fg = if selected {
        pal.accent_text
    } else {
        pal.text_secondary
    };

    draw_entry_icon(
        c,
        rect.x as f32 + 15.0,
        rect.y as f32 + (rect.h as f32) * 0.5,
        entry,
        selected,
        pal,
    );
    let name = Canvas::fit_text(&entry.name, (date_x - rect.x - 36).max(60), 13.0, selected);
    c.draw_text(&name, rect.x + 30, rect.y + 7, 13.0, selected, fg, clip);

    let date_str = entry
        .modified_unix_secs
        .map_or_else(|| String::from("--"), format_unix_timestamp);
    c.draw_text(&date_str, date_x + 8, rect.y + 7, 12.0, false, sub_fg, clip);

    let size_str = if entry.is_navigable_dir() {
        String::from("--")
    } else {
        format_byte_size(entry.size_bytes)
    };
    c.draw_text(&size_str, size_x + 8, rect.y + 7, 12.0, false, sub_fg, clip);
    c.draw_text(
        entry.kind_label(),
        kind_x + 8,
        rect.y + 7,
        12.0,
        false,
        sub_fg,
        clip,
    );
}

fn render_status_bar(
    c: &mut Canvas<'_>,
    state: &FilesAppState,
    layout: &FilesLayout,
    pal: &Palette,
) {
    c.fill_rect(
        Rect::new(
            layout.status_bar_rect.x + 4,
            layout.status_bar_rect.y,
            layout.status_bar_rect.w - 8,
            1,
        ),
        pal.divider,
    );
    let summary = state.status_summary();
    if layout.path_crumbs.is_empty() {
        let max_text_w = (layout.theme_toggle_rect.x - layout.status_bar_rect.x - 16).max(40);
        let fitted = Canvas::fit_text(&summary, max_text_w, 11.5, false);
        c.draw_text(
            &fitted,
            layout.status_bar_rect.x + 8,
            layout.status_bar_rect.y + 6,
            11.5,
            false,
            pal.text_secondary,
            layout.status_bar_rect,
        );
    } else {
        render_path_bar(c, &layout.path_crumbs, pal);
        let right_edge = layout
            .path_crumbs
            .last()
            .map_or(layout.status_bar_rect.x + 12, |crumb| {
                crumb.rect.right() + 12
            });
        let avail_summary = (layout.theme_toggle_rect.x - right_edge - 10).max(0);
        if avail_summary >= 36 {
            let fitted = Canvas::fit_text(&summary, avail_summary, 11.5, false);
            let sum_w = Canvas::measure_text(&fitted, 11.5, false);
            c.draw_text(
                &fitted,
                layout.theme_toggle_rect.x - sum_w - 10,
                layout.status_bar_rect.y + 6,
                11.5,
                false,
                pal.text_secondary,
                layout.status_bar_rect,
            );
        }
    }

    c.fill_rounded_rect(layout.theme_toggle_rect, 9, pal.pill_bg);
    c.stroke_rounded_rect(layout.theme_toggle_rect, 9, pal.pill_border);
    let label = match state.theme {
        FilesThemeMode::LightGoldenGate => "Theme: Light",
        FilesThemeMode::DarkPurpleGlass => "Theme: Dark",
    };
    let lbl_w = Canvas::measure_text(label, 11.0, true);
    c.draw_text(
        label,
        layout.theme_toggle_rect.x + (layout.theme_toggle_rect.w - lbl_w) / 2,
        layout.theme_toggle_rect.y + 4,
        11.0,
        true,
        pal.text_secondary,
        layout.theme_toggle_rect,
    );
}

/// Renders a standalone native `xdg_popup` surface buffer (`ARGB8888`) with soft outer shadow
/// and hover row highlighting.
pub(crate) fn render_popup_surface_pixels(
    buf: &mut [u8],
    width: u32,
    height: u32,
    popup: &PopupLayout,
    hover_idx: Option<usize>,
    theme: FilesThemeMode,
) {
    let pal = palette_for(theme);
    let mut c = Canvas::new(buf, width, height);
    c.clear();
    render_popup(&mut c, popup, hover_idx, &pal);
}

#[allow(clippy::cast_precision_loss)]
fn render_popup(c: &mut Canvas<'_>, popup: &PopupLayout, hover_idx: Option<usize>, pal: &Palette) {
    let r = popup.rect;
    c.fill_rounded_rect(
        Rect::new(r.x - 3, r.y - 1, r.w + 6, r.h + 6),
        15,
        rgba(0, 0, 0, 8),
    );
    c.fill_rounded_rect(
        Rect::new(r.x - 2, r.y, r.w + 4, r.h + 4),
        14,
        rgba(0, 0, 0, 14),
    );
    c.fill_rounded_rect(
        Rect::new(r.x - 1, r.y + 1, r.w + 2, r.h + 3),
        13,
        rgba(0, 0, 0, 20),
    );
    c.fill_rounded_rect(r, 12, pal.popup_bg);
    c.stroke_rounded_rect(r, 12, pal.popup_border);

    for (idx, item) in popup.items.iter().enumerate() {
        if item.command.is_none() {
            c.fill_rect(
                Rect::new(item.rect.x + 6, item.rect.y + 4, item.rect.w - 12, 1),
                pal.divider,
            );
            continue;
        }
        let hovered = hover_idx == Some(idx);
        if hovered {
            c.fill_rounded_rect(item.rect, 6, pal.accent);
        }
        let fg = if hovered {
            pal.accent_text
        } else {
            pal.text_primary
        };
        if item.checked {
            let cx = item.rect.x as f32 + 10.5;
            let cy = item.rect.y as f32 + 14.5;
            c.stroke_polyline_aa(
                &[
                    (cx - 3.2, cy - 0.3),
                    (cx - 0.7, cy + 2.5),
                    (cx + 3.8, cy - 2.8),
                ],
                false,
                1.45,
                if hovered { pal.accent_text } else { pal.accent },
            );
        }
        let text_x = if let Some([r, g, b]) = item.swatch_rgb {
            c.fill_circle_f(
                item.rect.x as f32 + 26.0,
                item.rect.y as f32 + 14.0,
                4.8,
                rgba(r, g, b, 255),
            );
            item.rect.x + 38
        } else {
            item.rect.x + 22
        };
        let label = Canvas::fit_text(&item.label, item.rect.right() - text_x - 8, 12.5, false);
        c.draw_text(&label, text_x, item.rect.y + 6, 12.5, false, fg, item.rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::compute_layout;
    use crate::state::StartupOptions;
    use rusty_services::file_manager::UserFileLocations;
    use std::fs;

    #[test]
    fn renders_golden_gate_light_and_dark_themes_cleanly() {
        let root =
            std::env::temp_dir().join(format!("rustyos-files-render-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        let mut state = FilesAppState::new(locs, &StartupOptions::default());
        state.complete_pending_loads_sync();

        let (w, h) = (920_u32, 600_u32);
        let mut buf = vec![0_u8; (w * h * 4) as usize];
        let layout_light = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &layout_light);

        // Rounded top-left pixel outside 20px radius remains transparent
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);
        // Traffic light close button inside floating sidebar is red (#FF5F57 -> BGRA [87, 95, 255, 255])
        let red_idx = ((30 * w + 29) * 4) as usize;
        assert_eq!(&buf[red_idx..red_idx + 4], &[87, 95, 255, 255]);
        // Content background white (#FFFFFF)
        let bg_idx = ((120 * w + 650) * 4) as usize;
        assert_eq!(&buf[bg_idx..bg_idx + 4], &[255, 255, 255, 255]);

        // Toggle to DarkPurpleGlass and re-render
        state.toggle_theme();
        let layout_dark = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &layout_dark);
        assert_eq!(&buf[bg_idx..bg_idx + 4], &[43, 22, 29, 255]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scrolled_sidebar_and_overflowing_columns_never_bleed_outside_clips() {
        let root =
            std::env::temp_dir().join(format!("rustyos-files-clip-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("home/Documents");
        let _ = fs::create_dir_all(&docs);
        for i in 0..25 {
            let _ = fs::create_dir_all(docs.join(format!("Dir_{i:02}")));
        }
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        let mut state = FilesAppState::new(
            locs,
            &StartupOptions {
                initial_path: Some(docs),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();
        let (w, h) = (768_u32, 461_u32);
        let mut buf_unscrolled = vec![0_u8; (w * h * 4) as usize];
        let layout_unscrolled = compute_layout(&state, w, h);
        render_window_pixels(&mut buf_unscrolled, w, h, &state, &layout_unscrolled);

        // Scroll sidebar far enough so top rows shift into traffic-light y=23..37 zone
        state.sidebar_scroll_y = 85;
        state.select_entry(0, 14, false);

        let mut buf = vec![0_u8; (w * h * 4) as usize];
        let layout = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &layout);

        // Every pixel of traffic-light header band (y=18..45) stays identical to unscrolled frame
        for py in 18..45_u32 {
            for px in 16..180_u32 {
                let idx = ((py * w + px) * 4) as usize;
                assert_eq!(&buf[idx..idx + 4], &buf_unscrolled[idx..idx + 4]);
            }
        }

        // Status bar top divider & 2px margin above it remain clean white without folder bleed
        let gap_above_footer =
            (((layout.status_bar_rect.y - 2).cast_unsigned() * w + 250) * 4) as usize;
        assert_eq!(
            &buf[gap_above_footer..gap_above_footer + 4],
            &[255, 255, 255, 255]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stage_b_icons_gallery_tabs_and_overlays_render_within_rounded_clip() {
        let root = std::env::temp_dir().join(format!(
            "rustyos-files-stageb-render-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("home/Documents");
        let _ = fs::create_dir_all(docs.join("SubFolder"));
        let _ = fs::write(docs.join("readme.txt"), "hello rusty files\n");
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        let mut state = FilesAppState::new(
            locs,
            &StartupOptions {
                initial_path: Some(docs),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();
        let (w, h) = (960_u32, 620_u32);
        let mut buf = vec![0_u8; (w * h * 4) as usize];

        state.set_view_mode(FileViewMode::Icons);
        state.new_tab(None);
        let icons_layout = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &icons_layout);
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);

        state.set_view_mode(FileViewMode::Gallery);
        state.toggle_quick_look_selected();
        let gal_ql_layout = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &gal_ql_layout);
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);

        // Search active focus ring + visible caret (`|`) when focused
        state.quick_look = None;
        state.search_active = true;
        state.caret_visible = true;
        state.set_search_query(String::from("read"));
        let search_layout = compute_layout(&state, w, h);
        render_window_pixels_with_popup(&mut buf, w, h, &state, &search_layout, false);
        let pal = palette_for(state.theme);
        let pill = search_layout.search_pill_rect;
        let caret_y = (pill.y + 12).cast_unsigned();
        let has_accent_caret = ((pill.x + 26)..pill.right() - 22).any(|px| {
            let idx = ((caret_y * w + px.cast_unsigned()) * 4) as usize;
            buf[idx + 2] == pal.accent[0]
                && buf[idx + 1] == pal.accent[1]
                && buf[idx] == pal.accent[2]
        });
        assert!(
            has_accent_caret,
            "Search input caret must render in accent color"
        );

        // Standalone native xdg_popup buffer renders hover highlight row cleanly
        let popup_menu = crate::state::OpenPopupMenu::SortMenu;
        let spec =
            crate::layout::compute_popup_surface_spec(&state, &search_layout, &popup_menu, 8);
        let pw = spec.surface_width.cast_unsigned();
        let ph = spec.surface_height.cast_unsigned();
        let mut pbuf = vec![0_u8; (pw * ph * 4) as usize];
        render_popup_surface_pixels(&mut pbuf, pw, ph, &spec.layout, Some(1), state.theme);
        let hovered_row = &spec.layout.items[1].rect;
        let mid_idx = (((hovered_row.y + hovered_row.h / 2).cast_unsigned() * pw
            + (hovered_row.x + 12).cast_unsigned())
            * 4) as usize;
        assert_eq!(pbuf[mid_idx + 2], pal.accent[0]);
        assert_eq!(pbuf[mid_idx + 1], pal.accent[1]);
        assert_eq!(pbuf[mid_idx], pal.accent[2]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn warm_scroll_frames_render_consistently_across_all_views() {
        let root =
            std::env::temp_dir().join(format!("rusty_files_scroll_bench_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("home/Documents");
        assert!(fs::create_dir_all(&docs).is_ok());
        for i in 0..36 {
            if i % 3 == 0 {
                assert!(fs::create_dir_all(docs.join(format!("Folder_{i:02}"))).is_ok());
            } else {
                assert!(
                    fs::write(
                        docs.join(format!("note_{i:02}.rs")),
                        b"fn main() { println!(\"hello\"); }\n",
                    )
                    .is_ok()
                );
            }
        }
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        let mut state = FilesAppState::new(
            locs,
            &StartupOptions {
                initial_path: Some(docs),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();
        let (w, h) = (1060_u32, 660_u32);
        let mut buf = vec![0_u8; (w * h * 4) as usize];
        let mut buf2 = vec![0_u8; (w * h * 4) as usize];

        for mode in [
            FileViewMode::List,
            FileViewMode::Icons,
            FileViewMode::Columns,
            FileViewMode::Gallery,
        ] {
            state.set_view_mode(mode);
            let layout = compute_layout(&state, w, h);
            // Warm icon/glyph caches then verify deterministic repeat & inspect speed.
            render_window_pixels_with_popup(&mut buf, w, h, &state, &layout, false);
            let warm_start = std::time::Instant::now();
            for _ in 0..5 {
                render_window_pixels_with_popup(&mut buf2, w, h, &state, &layout, false);
            }
            let avg_ms = warm_start.elapsed().as_secs_f64() * 1000.0 / 5.0;
            eprintln!("bench {mode:?} {w}x{h}: warm_avg={avg_ms:.2} ms/frame");
            assert_eq!(buf, buf2);
            assert_eq!(&buf2[0..4], &[0, 0, 0, 0]);
        }

        let _ = fs::remove_dir_all(&root);
    }
}
