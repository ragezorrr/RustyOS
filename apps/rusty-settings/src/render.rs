//! Pure CPU premultiplied `ARGB8888` renderer for `rusty-settings` (`RUSTYOS-103`).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::cell::RefCell;

use crate::layout::{
    AppearanceCardsLayout, CARD_CORNER_RADIUS, CardRowControl, CardRowLayout, CardSectionLayout,
    DockPositionCardLayout, NotificationAppKind, Rect, ServiceBadgeKind, SettingsHit,
    SettingsLayout, WINDOW_CORNER_RADIUS, about_more_info_rect, battery_tabs_inner_rect,
    bt_device_action_rects, choice_rect, dropdown_trigger_rect, shortcut_pill_rect, stepper_rects,
    switch_rect,
};
use crate::state::{ACCENT_COLORS, RowControlId, SettingsAppState, SettingsPage, ThemeChoice};

thread_local! {
    static FONT_ENGINE: RefCell<rusty_ui::UiFontEngine> =
        RefCell::new(rusty_ui::UiFontEngine::new());
    static SPRITE_CACHE: RefCell<Vec<(SpriteKey, Vec<u8>)>> = RefCell::new(Vec::with_capacity(32));
}

pub(crate) fn with_font_engine<R>(f: impl FnOnce(&mut rusty_ui::UiFontEngine) -> R) -> R {
    FONT_ENGINE.with(|cell| f(&mut cell.borrow_mut()))
}

fn to_ui_rect(r: Rect) -> rusty_ui::Rect {
    rusty_ui::Rect::new(r.x, r.y, r.w.max(0) as u32, r.h.max(0) as u32)
}

fn make_ui_canvas(buf: &mut [u8], w: i32, h: i32) -> rusty_ui::UiCanvas<'_> {
    let uw = w.max(0) as u32;
    let uh = h.max(0) as u32;
    rusty_ui::UiCanvas::new(
        buf,
        uw,
        uh,
        (uw as usize) * 4,
        rusty_ui::PixelFormat::Bgra8888,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteKey {
    RowIcon(SettingsPage, i32, i32),
    ServiceBadge(ServiceBadgeKind, [u8; 4], i32, i32),
    NotifBadge(NotificationAppKind, [u8; 4], i32, i32),
    ThemePreview(ThemeChoice, i32, i32),
}

fn blit_cached_sprite(
    c: &mut Canvas<'_>,
    dst: Rect,
    key: SpriteKey,
    draw: impl FnOnce(&mut Canvas<'_>, Rect),
) {
    if c.is_clipped_out(dst) || dst.w <= 0 || dst.h <= 0 {
        return;
    }
    SPRITE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, sprite)) = cache.iter().find(|(k, _)| *k == key) {
            c.blit_premul_bgra(dst, sprite);
            return;
        }
        let Ok(byte_len) =
            usize::try_from(dst.w).and_then(|w| usize::try_from(dst.h).map(|h| w * h * 4))
        else {
            return;
        };
        let mut sprite = vec![0_u8; byte_len];
        let local_rect = Rect::new(0, 0, dst.w, dst.h);
        {
            let mut sc = Canvas::new(&mut sprite, dst.w as u32, dst.h as u32);
            draw(&mut sc, local_rect);
        }
        c.blit_premul_bgra(dst, &sprite);
        if cache.len() < 96 {
            cache.push((key, sprite));
        }
    });
}

#[derive(Clone, Copy)]
struct Palette {
    is_dark: bool,
    sidebar_bg: [u8; 4],
    divider: [u8; 4],
    page_bg: [u8; 4],
    card_bg: [u8; 4],
    row_separator: [u8; 4],
    sidebar_active: [u8; 4],
    sidebar_hover: [u8; 4],
    control_bg: [u8; 4],
    accent: [u8; 4],
    text_primary: [u8; 4],
    text_secondary: [u8; 4],
    text_muted: [u8; 4],
    text_disabled: [u8; 4],
}

fn palette_for(app: &SettingsAppState) -> Palette {
    let accent = ACCENT_COLORS[app.accent_index.min(ACCENT_COLORS.len() - 1)];
    if app.is_dark_ui() {
        Palette {
            is_dark: true,
            sidebar_bg: [38, 38, 40, 255],
            divider: [56, 56, 58, 255],
            page_bg: [28, 28, 29, 255],
            card_bg: [44, 44, 45, 255],
            row_separator: [60, 60, 62, 255],
            sidebar_active: [62, 62, 65, 255],
            sidebar_hover: [52, 52, 55, 255],
            control_bg: [60, 60, 64, 255],
            accent,
            text_primary: [255, 255, 255, 255],
            text_secondary: [186, 186, 194, 255],
            text_muted: [138, 138, 146, 255],
            text_disabled: [104, 104, 112, 255],
        }
    } else {
        Palette {
            is_dark: false,
            sidebar_bg: [236, 237, 241, 255],
            divider: [224, 225, 232, 255],
            page_bg: [245, 246, 250, 255],
            card_bg: [255, 255, 255, 255],
            row_separator: [235, 236, 242, 255],
            sidebar_active: [217, 218, 221, 255],
            sidebar_hover: [226, 227, 231, 255],
            control_bg: [242, 244, 248, 255],
            accent,
            text_primary: [50, 50, 56, 255],
            text_secondary: [100, 100, 110, 255],
            text_muted: [128, 130, 140, 255],
            text_disabled: [182, 184, 192, 255],
        }
    }
}

#[inline]
const fn div_255(v: u16) -> u16 {
    (v + 1 + (v >> 8)) >> 8
}

#[allow(clippy::cast_precision_loss)]
fn sd_rounded_box(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let hw = w * 0.5;
    let hh = h * 0.5;
    let rad = r.min(hw).min(hh).max(0.0);
    let qx = px.abs() - (hw - rad);
    let qy = py.abs() - (hh - rad);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad
}

#[allow(clippy::cast_precision_loss)]
fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, rad: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = bax * bax + bay * bay;
    let t = if denom <= 1e-6 {
        0.0
    } else {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    };
    (pax - bax * t).hypot(pay - bay * t) - rad
}

/// 4-tap rotated-grid supersampled edge coverage `0..=255` for a signed-distance field, evaluated at
/// the pixel `(px, py)` in absolute canvas coordinates (`RUSTYOS-111`).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn aa_cov8(px: i32, py: i32, sd: impl Fn(f32, f32) -> f32) -> u8 {
    const TAPS: [(f32, f32); 4] = [
        (-0.375, -0.125),
        (0.125, -0.375),
        (0.375, 0.125),
        (-0.125, 0.375),
    ];
    let cx = px as f32 + 0.5;
    let cy = py as f32 + 0.5;
    let d0 = sd(cx, cy);
    if d0 <= -1.0 {
        return 255;
    }
    if d0 >= 1.2 {
        return 0;
    }
    let mut sum = 0.0_f32;
    for (ox, oy) in TAPS {
        sum += (0.5 - sd(cx + ox, cy + oy)).clamp(0.0, 1.0);
    }
    (sum * 0.25 * 255.0).round() as u8
}

struct Canvas<'a> {
    buf: &'a mut [u8],
    w: i32,
    h: i32,
    clip: Option<Rect>,
}

impl<'a> Canvas<'a> {
    fn new(buf: &'a mut [u8], width: u32, height: u32) -> Self {
        Self {
            buf,
            w: i32::try_from(width).unwrap_or(0),
            h: i32::try_from(height).unwrap_or(0),
            clip: None,
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn blend_px(&mut self, x: i32, y: i32, rgba: [u8; 4], cov: u8) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || cov == 0 || rgba[3] == 0 {
            return;
        }
        if let Some(clip) = self.clip
            && (x < clip.x || x >= clip.right() || y < clip.y || y >= clip.bottom())
        {
            return;
        }
        let alpha = div_255(u16::from(rgba[3]) * u16::from(cov));
        if alpha == 0 {
            return;
        }
        let idx = ((y * self.w + x) * 4) as usize;
        if idx + 3 >= self.buf.len() {
            return;
        }
        if alpha == 255 {
            self.buf[idx] = rgba[2];
            self.buf[idx + 1] = rgba[1];
            self.buf[idx + 2] = rgba[0];
            self.buf[idx + 3] = 255;
            return;
        }
        let inv = 255 - alpha;
        let sb = div_255(u16::from(rgba[2]) * alpha);
        let sg = div_255(u16::from(rgba[1]) * alpha);
        let sr = div_255(u16::from(rgba[0]) * alpha);
        let db = div_255(u16::from(self.buf[idx]) * inv);
        let dg = div_255(u16::from(self.buf[idx + 1]) * inv);
        let dr = div_255(u16::from(self.buf[idx + 2]) * inv);
        let da = div_255(u16::from(self.buf[idx + 3]) * inv);
        self.buf[idx] = (sb + db).min(255) as u8;
        self.buf[idx + 1] = (sg + dg).min(255) as u8;
        self.buf[idx + 2] = (sr + dr).min(255) as u8;
        self.buf[idx + 3] = (alpha + da).min(255) as u8;
    }

    fn clamped_bounds(&self, rect: Rect) -> (i32, i32, i32, i32) {
        let mut x0 = rect.x.max(0);
        let mut y0 = rect.y.max(0);
        let mut x1 = rect.right().min(self.w);
        let mut y1 = rect.bottom().min(self.h);
        if let Some(clip) = self.clip {
            x0 = x0.max(clip.x);
            y0 = y0.max(clip.y);
            x1 = x1.min(clip.right());
            y1 = y1.min(clip.bottom());
        }
        (x0, y0, x1, y1)
    }

    fn is_clipped_out(&self, rect: Rect) -> bool {
        let (x0, y0, x1, y1) = self.clamped_bounds(rect);
        x0 >= x1 || y0 >= y1
    }

    fn blend_span_opaque(&mut self, py: i32, x0: i32, x1: i32, color: [u8; 4]) {
        if py < 0 || py >= self.h || x0 >= x1 || color[3] == 0 {
            return;
        }
        let row_start = (py * self.w + x0) as usize * 4;
        let row_end = (py * self.w + x1) as usize * 4;
        if row_end > self.buf.len() {
            return;
        }
        if color[3] == 255 {
            let px = [color[2], color[1], color[0], 255];
            let (chunks, _) = self.buf[row_start..row_end].as_chunks_mut::<4>();
            chunks.fill(px);
            return;
        }
        let alpha = u16::from(color[3]);
        let inv = 255 - alpha;
        let sb = div_255(u16::from(color[2]) * alpha);
        let sg = div_255(u16::from(color[1]) * alpha);
        let sr = div_255(u16::from(color[0]) * alpha);
        for dst in self.buf[row_start..row_end].as_chunks_mut::<4>().0 {
            let db = div_255(u16::from(dst[0]) * inv);
            let dg = div_255(u16::from(dst[1]) * inv);
            let dr = div_255(u16::from(dst[2]) * inv);
            let da = div_255(u16::from(dst[3]) * inv);
            dst[0] = (sb + db).min(255) as u8;
            dst[1] = (sg + dg).min(255) as u8;
            dst[2] = (sr + dr).min(255) as u8;
            dst[3] = (alpha + da).min(255) as u8;
        }
    }

    fn blit_premul_bgra(&mut self, dst: Rect, sprite: &[u8]) {
        let (x0, y0, x1, y1) = self.clamped_bounds(dst);
        if x0 >= x1 || y0 >= y1 || dst.w <= 0 || dst.h <= 0 {
            return;
        }
        for py in y0..y1 {
            let sy = (py - dst.y) as usize;
            let row_src_base = sy * dst.w as usize;
            let row_dst_base = (py * self.w) as usize;
            for px in x0..x1 {
                let sx = (px - dst.x) as usize;
                let s_idx = (row_src_base + sx) * 4;
                let sa = sprite[s_idx + 3];
                if sa == 0 {
                    continue;
                }
                let d_idx = (row_dst_base + px as usize) * 4;
                if d_idx + 3 >= self.buf.len() {
                    break;
                }
                if sa == 255 {
                    self.buf[d_idx..d_idx + 4].copy_from_slice(&sprite[s_idx..s_idx + 4]);
                    continue;
                }
                let inv = 255 - u16::from(sa);
                for ch in 0..4 {
                    let s_ch = u16::from(sprite[s_idx + ch]);
                    let d_ch = div_255(u16::from(self.buf[d_idx + ch]) * inv);
                    self.buf[d_idx + ch] = (s_ch + d_ch).min(255) as u8;
                }
            }
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
        let (x0, y0, x1, y1) = self.clamped_bounds(rect);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for py in y0..y1 {
            self.blend_span_opaque(py, x0, x1, color);
        }
    }

    fn rounded_interior_span(
        rect: Rect,
        margin_edge: i32,
        margin_corner: i32,
        x0: i32,
        x1: i32,
        py: i32,
    ) -> Option<(i32, i32)> {
        if py < rect.y + margin_edge || py >= rect.bottom() - margin_edge {
            return None;
        }
        let pad_x = if py >= rect.y + margin_corner && py < rect.bottom() - margin_corner {
            margin_edge
        } else {
            margin_corner
        };
        let sx0 = (rect.x + pad_x).max(x0);
        let sx1 = (rect.right() - pad_x).min(x1);
        (sx0 < sx1).then_some((sx0, sx1))
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn fill_rounded_rect(&mut self, rect: Rect, radius: i32, color: [u8; 4]) {
        let (x0, y0, x1, y1) = self.clamped_bounds(rect);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        let rw = rect.w as f32;
        let rh = rect.h as f32;
        let rad = (radius as f32).min(rw * 0.5).min(rh * 0.5).max(0.0);
        let corner_pad = rad.ceil() as i32 + 2;
        for py in y0..y1 {
            if let Some((sx0, sx1)) = Self::rounded_interior_span(rect, 2, corner_pad, x0, x1, py) {
                for px in x0..sx0 {
                    let cov = aa_cov8(px, py, |x, y| sd_rounded_box(x - cx, y - cy, rw, rh, rad));
                    if cov > 0 {
                        self.blend_px(px, py, color, cov);
                    }
                }
                self.blend_span_opaque(py, sx0, sx1, color);
                for px in sx1..x1 {
                    let cov = aa_cov8(px, py, |x, y| sd_rounded_box(x - cx, y - cy, rw, rh, rad));
                    if cov > 0 {
                        self.blend_px(px, py, color, cov);
                    }
                }
            } else {
                for px in x0..x1 {
                    let cov = aa_cov8(px, py, |x, y| sd_rounded_box(x - cx, y - cy, rw, rh, rad));
                    if cov > 0 {
                        self.blend_px(px, py, color, cov);
                    }
                }
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn stroke_rounded_rect(&mut self, rect: Rect, radius: f32, stroke_w: f32, color: [u8; 4]) {
        let (x0, y0, x1, y1) = self.clamped_bounds(rect);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        let rw = rect.w as f32;
        let rh = rect.h as f32;
        let rad = radius.min(rw * 0.5).min(rh * 0.5).max(0.0);
        let edge_pad = stroke_w.ceil() as i32 + 2;
        let corner_pad = rad.ceil() as i32 + edge_pad;
        for py in y0..y1 {
            if let Some((sx0, sx1)) =
                Self::rounded_interior_span(rect, edge_pad, corner_pad, x0, x1, py)
            {
                for px in (x0..sx0).chain(sx1..x1) {
                    let cov = aa_cov8(px, py, |x, y| {
                        let d = sd_rounded_box(x - cx, y - cy, rw, rh, rad);
                        (d + stroke_w * 0.5).abs() - stroke_w * 0.5
                    });
                    if cov > 0 {
                        self.blend_px(px, py, color, cov);
                    }
                }
            } else {
                for px in x0..x1 {
                    let cov = aa_cov8(px, py, |x, y| {
                        let d = sd_rounded_box(x - cx, y - cy, rw, rh, rad);
                        (d + stroke_w * 0.5).abs() - stroke_w * 0.5
                    });
                    if cov > 0 {
                        self.blend_px(px, py, color, cov);
                    }
                }
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        let bounds = Rect::new(
            (cx - radius - 1.5).floor() as i32,
            (cy - radius - 1.5).floor() as i32,
            ((radius + 1.5) * 2.0).ceil() as i32 + 1,
            ((radius + 1.5) * 2.0).ceil() as i32 + 1,
        );
        let (x0, y0, x1, y1) = self.clamped_bounds(bounds);
        for py in y0..y1 {
            for px in x0..x1 {
                let cov = aa_cov8(px, py, |x, y| (x - cx).hypot(y - cy) - radius);
                if cov > 0 {
                    self.blend_px(px, py, color, cov);
                }
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn fill_gradient_squircle(&mut self, rect: Rect, radius: i32, bottom_tint: [u8; 4]) {
        let (x0, y0, x1, y1) = self.clamped_bounds(rect);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let top_tint = [
            (f32::from(bottom_tint[0]) * 1.15).round().min(255.0) as u8,
            (f32::from(bottom_tint[1]) * 1.15).round().min(255.0) as u8,
            (f32::from(bottom_tint[2]) * 1.15).round().min(255.0) as u8,
            255,
        ];
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        let rw = rect.w as f32;
        let rh = rect.h as f32;
        let rad = (radius as f32).min(rw * 0.5).min(rh * 0.5).max(0.0);
        let corner_pad = rad.ceil() as i32 + 2;
        for py in y0..y1 {
            let t = ((py - rect.y) as f32 / rh.max(1.0)).clamp(0.0, 1.0);
            let col = lerp_rgba(top_tint, bottom_tint, t);
            if let Some((sx0, sx1)) = Self::rounded_interior_span(rect, 2, corner_pad, x0, x1, py) {
                for px in x0..sx0 {
                    let cov = aa_cov8(px, py, |x, y| sd_rounded_box(x - cx, y - cy, rw, rh, rad));
                    if cov > 0 {
                        self.blend_px(px, py, col, cov);
                    }
                }
                self.blend_span_opaque(py, sx0, sx1, col);
                for px in sx1..x1 {
                    let cov = aa_cov8(px, py, |x, y| sd_rounded_box(x - cx, y - cy, rw, rh, rad));
                    if cov > 0 {
                        self.blend_px(px, py, col, cov);
                    }
                }
            } else {
                for px in x0..x1 {
                    let cov = aa_cov8(px, py, |x, y| sd_rounded_box(x - cx, y - cy, rw, rh, rad));
                    if cov > 0 {
                        self.blend_px(px, py, col, cov);
                    }
                }
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn draw_segment(&mut self, ax: f32, ay: f32, bx: f32, by: f32, radius: f32, color: [u8; 4]) {
        let bx0 = (ax.min(bx) - radius - 2.0).floor() as i32;
        let by0 = (ay.min(by) - radius - 2.0).floor() as i32;
        let bx1 = (ax.max(bx) + radius + 2.0).ceil() as i32;
        let by1 = (ay.max(by) + radius + 2.0).ceil() as i32;
        let (x0, y0, x1, y1) = self.clamped_bounds(Rect::new(bx0, by0, bx1 - bx0, by1 - by0));
        for py in y0..y1 {
            for px in x0..x1 {
                let cov = aa_cov8(px, py, |x, y| sd_segment(x, y, ax, ay, bx, by, radius));
                if cov > 0 {
                    self.blend_px(px, py, color, cov);
                }
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn measure_text(&self, text: &str, size_px: f32, bold: bool) -> i32 {
        let _ = self.w;
        with_font_engine(|fe| fe.measure_width(text, size_px, bold).round() as i32)
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn draw_text(&mut self, text: &str, x: i32, y: i32, size_px: f32, bold: bool, color: [u8; 4]) {
        if let Some(clip) = self.clip
            && (y >= clip.bottom() || y + size_px.ceil() as i32 + 4 <= clip.y || x >= clip.right())
        {
            return;
        }
        let baseline = y + (size_px * 0.84).round() as i32;
        let clip_ui = self.clip.map(to_ui_rect);
        let mut uc = make_ui_canvas(self.buf, self.w, self.h);
        with_font_engine(|fe| {
            fe.draw_text_on_canvas(
                &mut uc,
                text,
                x as f32,
                baseline as f32,
                size_px,
                bold,
                color,
                clip_ui,
            );
        });
    }

    fn draw_text_centered(
        &mut self,
        text: &str,
        rect: Rect,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
    ) {
        let tw = self.measure_text(text, size_px, bold);
        let th = size_px.round() as i32;
        let x = rect.x + (rect.w - tw) / 2;
        let y = rect.y + (rect.h - th) / 2;
        self.draw_text(text, x, y, size_px, bold, color);
    }

    /// Masks outer window frame pixels cleanly to rounded corner shape (`14px` radius).
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn apply_window_corner_mask(&mut self, radius: i32) {
        let cx = self.w as f32 * 0.5;
        let cy = self.h as f32 * 0.5;
        let rw = self.w as f32;
        let rh = self.h as f32;
        let rad = radius as f32;
        let corner_pad = radius + 2;
        for py in 0..self.h {
            let near_v = py < corner_pad || py >= self.h - corner_pad;
            for px in 0..self.w {
                if !near_v && px >= corner_pad && px < self.w - corner_pad {
                    continue;
                }
                let d = sd_rounded_box(px as f32 + 0.5 - cx, py as f32 + 0.5 - cy, rw, rh, rad);
                let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0).round() as u16;
                let idx = ((py * self.w + px) * 4) as usize;
                if cov == 0 {
                    self.buf[idx..idx + 4].fill(0);
                } else if cov < 255 {
                    for c in &mut self.buf[idx..idx + 4] {
                        *c = div_255(u16::from(*c) * cov) as u8;
                    }
                }
            }
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn lerp_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let f = |x: u8, y: u8| -> u8 {
        (f32::from(x) + (f32::from(y) - f32::from(x)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2]), f(a[3], b[3])]
}

pub(crate) fn render_window_pixels_with_glass(
    buf: &mut [u8],
    width: u32,
    height: u32,
    app: &SettingsAppState,
    layout: &SettingsLayout,
    hover: Option<&SettingsHit>,
    sidebar_glass: bool,
) {
    let pal = palette_for(app);
    buf.fill(0);
    let mut c = Canvas::new(buf, width, height);

    // 1. Split-pane background: translucent frosted sidebar (or solid fallback) left + solid canvas right.
    let sidebar_bg = if sidebar_glass {
        if pal.is_dark {
            [34, 35, 39, 158]
        } else {
            [242, 243, 247, 152]
        }
    } else {
        pal.sidebar_bg
    };
    c.fill_rect(layout.sidebar_rect, sidebar_bg);
    c.fill_rect(Rect::new(layout.divider_x, 0, 1, layout.win_h), pal.divider);
    c.fill_rect(layout.page_rect, pal.page_bg);

    // 2. Left sidebar header (traffic lights + borderless capsule search) and scrollable items.
    render_sidebar(&mut c, app, layout, &pal, hover, sidebar_glass);

    // 3. Scrollable right page content clipped cleanly below header, then header + scrollbar thumb.
    c.clip = Some(layout.page_viewport);
    if let Some(app_cards) = &layout.appearance {
        render_appearance_page(&mut c, app, app_cards, hover, &pal);
    } else if let Some(dp) = &layout.dock_positions {
        render_dock_positions_card(&mut c, dp, &pal);
    }
    for sec in &layout.sections {
        render_card_section(&mut c, app, sec, hover, &pal);
    }
    c.clip = None;
    render_page_header(&mut c, app, layout, &pal);
    if let Some(thumb) = layout.page_scrollbar_thumb {
        let hovered = matches!(hover, Some(SettingsHit::ScrollbarThumb));
        let thumb_col = if pal.is_dark {
            [255, 255, 255, if hovered { 128 } else { 92 }]
        } else {
            [0, 0, 0, if hovered { 104 } else { 68 }]
        };
        c.fill_rounded_rect(thumb, 2, thumb_col);
    }

    render_floating_overlays(&mut c, app, layout, &pal);

    // 4. 1px hairline border clipped cleanly by anti-aliased outer rounded corners (radius = 14px).
    c.stroke_rounded_rect(
        Rect::new(0, 0, layout.win_w, layout.win_h),
        WINDOW_CORNER_RADIUS as f32,
        1.0,
        if pal.is_dark {
            [255, 255, 255, 22]
        } else {
            [0, 0, 0, 24]
        },
    );
    c.apply_window_corner_mask(WINDOW_CORNER_RADIUS);
}

fn render_sidebar_traffic_and_search(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    layout: &SettingsLayout,
    pal: &Palette,
    hover: Option<&SettingsHit>,
) {
    let tl_hovered = matches!(
        hover,
        Some(
            SettingsHit::TrafficClose | SettingsHit::TrafficMinimize | SettingsHit::TrafficMaximize
        )
    );
    let tl = rusty_ui::TrafficLightButtons::default_csd();
    let lights = [
        (layout.traffic_close, rusty_ui::TrafficLightKind::Close),
        (layout.traffic_min, rusty_ui::TrafficLightKind::Minimize),
        (layout.traffic_max, rusty_ui::TrafficLightKind::Maximize),
    ];
    for (rect, kind) in lights {
        let col = if app.focused {
            kind.active_color()
        } else {
            rusty_ui::TrafficLightKind::inactive_color()
        };
        let cx = rect.x as f32 + 7.0;
        let cy = tl.center_y;
        c.fill_circle(cx, cy, 7.0, col);
        if tl_hovered {
            let ink = rusty_ui::TrafficLightKind::glyph_ink();
            for seg in rusty_ui::glyph_segments(kind) {
                c.draw_segment(
                    cx + seg.dx0,
                    cy + seg.dy0,
                    cx + seg.dx1,
                    cy + seg.dy1,
                    0.75,
                    ink,
                );
            }
        }
    }

    // Circular Search capsule powered by unified `rusty_ui::TextField` (`RUSTYOS-130`)
    let mut sf = app.search_field.clone();
    sf.rect = to_ui_rect(layout.search_rect);
    if sf.buffer.as_str() != app.search_query {
        sf.buffer.set_text(&app.search_query);
    }
    let ui_pal = rusty_ui::ThemePalette::new(pal.is_dark, app.accent_index);
    let focus_alpha = app.focus.ring_alpha();
    let elapsed = app.epoch.elapsed();
    let mut dl = rusty_ui::UiDrawList::new();
    with_font_engine(|fe| {
        let mut painter = rusty_ui::UiPainter::new(&mut dl);
        sf.paint(
            &mut painter,
            fe,
            &ui_pal,
            app.search_focused,
            focus_alpha,
            elapsed,
        );
        let mut uc = make_ui_canvas(c.buf, c.w, c.h);
        rusty_ui::render_draw_list_to_canvas(&dl, &mut uc, fe);
    });
}

fn render_sidebar(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    layout: &SettingsLayout,
    pal: &Palette,
    hover: Option<&SettingsHit>,
    sidebar_glass: bool,
) {
    render_sidebar_traffic_and_search(c, app, layout, pal, hover);

    c.clip = Some(layout.sidebar_viewport);
    for cat in &layout.categories {
        if c.is_clipped_out(cat.rect) {
            continue;
        }
        c.draw_text(
            cat.label,
            cat.rect.x,
            cat.rect.y,
            11.0,
            false,
            pal.text_muted,
        );
    }

    for item in &layout.sidebar_items {
        if c.is_clipped_out(item.rect) {
            continue;
        }
        let is_hovered = hover == Some(&SettingsHit::SidebarItem(item.page));
        let active_pill = if sidebar_glass {
            if pal.is_dark {
                [255, 255, 255, 34]
            } else {
                [0, 0, 0, 24]
            }
        } else {
            pal.sidebar_active
        };
        let hover_pill = if sidebar_glass {
            if pal.is_dark {
                [255, 255, 255, 18]
            } else {
                [0, 0, 0, 14]
            }
        } else {
            pal.sidebar_hover
        };
        if item.selected {
            c.fill_rounded_rect(item.rect, 10, active_pill);
        } else if is_hovered {
            c.fill_rounded_rect(item.rect, 10, hover_pill);
        }
        render_row_icon(c, item.icon_rect, item.page);
        c.draw_text(
            item.page.title(),
            item.icon_rect.right() + 9,
            item.rect.y + 9,
            12.5,
            false,
            pal.text_primary,
        );
    }
    c.clip = None;
}

fn render_page_header(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    layout: &SettingsLayout,
    pal: &Palette,
) {
    let back_col = if app.can_go_back() {
        pal.text_primary
    } else {
        pal.text_disabled
    };
    let fwd_col = if app.can_go_forward() {
        pal.text_primary
    } else {
        pal.text_disabled
    };
    draw_chevron(
        c,
        layout.nav_back.x as f32 + 11.0,
        layout.nav_back.y as f32 + 9.0,
        true,
        back_col,
    );
    draw_chevron(
        c,
        layout.nav_forward.x as f32 + 11.0,
        layout.nav_forward.y as f32 + 9.0,
        false,
        fwd_col,
    );
    c.draw_text(
        app.current_page.title(),
        layout.title_rect.x,
        layout.title_rect.y,
        13.5,
        true,
        pal.text_primary,
    );
}

fn render_appearance_page(
    c: &mut Canvas<'_>,
    state: &SettingsAppState,
    app: &AppearanceCardsLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
) {
    c.draw_text(
        "Appearance",
        app.mode_label_rect.x,
        app.mode_label_rect.y,
        12.0,
        false,
        pal.text_primary,
    );
    c.fill_rounded_rect(app.mode_card_rect, CARD_CORNER_RADIUS, pal.card_bg);

    for tile in &app.tiles {
        render_theme_preview_artwork(c, tile.artwork_rect, tile.choice);
        if tile.selected {
            c.stroke_rounded_rect(tile.ring_rect, 13.0, 2.0, pal.accent);
        }
        c.draw_text_centered(
            tile.label,
            tile.label_rect,
            12.0,
            false,
            if tile.selected {
                pal.accent
            } else {
                pal.text_primary
            },
        );
    }

    c.draw_text(
        "Accent color",
        app.accent_label_rect.x,
        app.accent_label_rect.y,
        12.0,
        false,
        pal.text_primary,
    );
    c.fill_rounded_rect(app.accent_card_rect, CARD_CORNER_RADIUS, pal.card_bg);

    for &(rect, rgba, selected) in &app.swatches {
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        if selected {
            let ring_col = [rgba[0], rgba[1], rgba[2], 125];
            c.stroke_rounded_rect(rect, 19.0, 2.5, ring_col);
            c.fill_circle(cx, cy, 13.0, rgba);
            c.draw_segment(cx - 4.0, cy + 0.5, cx - 1.0, cy + 3.5, 1.1, [255; 4]);
            c.draw_segment(cx - 1.0, cy + 3.5, cx + 4.5, cy - 3.2, 1.1, [255; 4]);
        } else {
            c.fill_circle(cx, cy, 13.0, rgba);
        }
    }

    c.fill_rounded_rect(app.links_card_rect, CARD_CORNER_RADIUS, pal.card_bg);
    for (idx, row) in app.links.iter().enumerate() {
        render_card_row(c, state, row, hover, pal);
        if idx + 1 < app.links.len() {
            c.fill_rect(
                Rect::new(row.rect.x + 16, row.rect.bottom() - 1, row.rect.w - 32, 1),
                pal.row_separator,
            );
        }
    }
}

fn render_theme_preview_artwork(c: &mut Canvas<'_>, art: Rect, choice: ThemeChoice) {
    blit_cached_sprite(
        c,
        art,
        SpriteKey::ThemePreview(choice, art.w, art.h),
        |sc, local_art| render_theme_preview_direct(sc, local_art, choice),
    );
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn blit_rgba_sprite_rounded(
    c: &mut Canvas<'_>,
    dst: Rect,
    src_w: usize,
    src_h: usize,
    rgba: &[u8],
    radius: f32,
    tint_white: bool,
) {
    if dst.w <= 0 || dst.h <= 0 || src_w == 0 || src_h == 0 {
        return;
    }
    let cx = dst.x as f32 + dst.w as f32 * 0.5;
    let cy = dst.y as f32 + dst.h as f32 * 0.5;
    let rw = dst.w as f32;
    let rh = dst.h as f32;
    for py in dst.y..dst.bottom() {
        let sy = ((py - dst.y) as usize * src_h / dst.h as usize).min(src_h - 1);
        for px in dst.x..dst.right() {
            let sx = ((px - dst.x) as usize * src_w / dst.w as usize).min(src_w - 1);
            let off = (sy * src_w + sx) * 4;
            let src_a = rgba[off + 3];
            if src_a == 0 {
                continue;
            }
            let cov = if radius > 0.0 {
                let d = sd_rounded_box(px as f32 + 0.5 - cx, py as f32 + 0.5 - cy, rw, rh, radius);
                if d >= 0.6 {
                    continue;
                }
                ((0.6 - d).clamp(0.0, 1.0) * 255.0).round() as u8
            } else {
                255
            };
            let color = if tint_white {
                [255, 255, 255, src_a]
            } else {
                [rgba[off], rgba[off + 1], rgba[off + 2], src_a]
            };
            c.blend_px(px, py, color, cov);
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn render_theme_preview_direct(c: &mut Canvas<'_>, art: Rect, choice: ThemeChoice) {
    let key = match choice {
        ThemeChoice::Light => "light",
        ThemeChoice::Dark => "dark",
        ThemeChoice::Auto => "auto",
    };
    if let Some((sw, sh, rgba)) =
        crate::svg_icons::theme_illustration_png(key).and_then(crate::svg_icons::decode_png)
    {
        blit_rgba_sprite_rounded(c, art, sw, sh, &rgba, 10.0, false);
        return;
    }
    let cx = art.x as f32 + art.w as f32 * 0.5;
    let cy = art.y as f32 + art.h as f32 * 0.5;
    let rw = art.w as f32;
    let rh = art.h as f32;
    let win_rect = Rect::new(art.x, art.y + 16, 78, 52);

    for py in art.y..art.bottom() {
        for px in art.x..art.right() {
            let lx = px as f32 + 0.5;
            let ly = py as f32 + 0.5;
            let d = sd_rounded_box(lx - cx, ly - cy, rw, rh, 10.0);
            if d >= 0.6 {
                continue;
            }
            let cov = ((0.6 - d).clamp(0.0, 1.0) * 255.0).round() as u8;
            let ux = (lx - art.x as f32) / rw;
            let uy = (ly - art.y as f32) / rh;
            // Warm sand/pastel backdrop visible in top and right strips like light_mode.svg
            let bg = [
                (176.0 + ux * 46.0 + uy * 18.0).clamp(0.0, 255.0) as u8,
                (194.0 - ux * 16.0 - uy * 8.0).clamp(0.0, 255.0) as u8,
                (204.0 - ux * 42.0 - uy * 26.0).clamp(0.0, 255.0) as u8,
                255,
            ];
            let in_win = px < win_rect.right() && py >= win_rect.y;
            let mut color = bg;
            if in_win {
                let dark_half = match choice {
                    ThemeChoice::Light => false,
                    ThemeChoice::Dark => true,
                    ThemeChoice::Auto => ux > 0.44 + (0.5 - uy) * 0.20,
                };
                color = if dark_half {
                    [20, 20, 22, 255]
                } else {
                    [248, 249, 252, 255]
                };
            }
            c.blend_px(px, py, color, cov);
        }
    }

    // Mini window controls (-  []  x) top-right of inner preview window
    let ink = match choice {
        ThemeChoice::Light => [92, 96, 108, 255],
        ThemeChoice::Dark | ThemeChoice::Auto => [210, 212, 220, 255],
    };
    let cy_btn = (win_rect.y + 8) as f32;
    let x_min = (win_rect.right() - 28) as f32;
    let x_box = (win_rect.right() - 19) as f32;
    let x_cls = (win_rect.right() - 10) as f32;
    c.draw_segment(x_min - 2.0, cy_btn, x_min + 2.0, cy_btn, 0.6, ink);
    c.stroke_rounded_rect(
        Rect::new(x_box as i32 - 2, cy_btn as i32 - 2, 5, 5),
        0.5,
        0.9,
        ink,
    );
    c.draw_segment(
        x_cls - 2.0,
        cy_btn - 2.0,
        x_cls + 2.0,
        cy_btn + 2.0,
        0.6,
        ink,
    );
    c.draw_segment(
        x_cls + 2.0,
        cy_btn - 2.0,
        x_cls - 2.0,
        cy_btn + 2.0,
        0.6,
        ink,
    );
}

fn render_dock_positions_card(c: &mut Canvas<'_>, dp: &DockPositionCardLayout, pal: &Palette) {
    c.draw_text(
        "Dock",
        dp.section_rect.x,
        dp.section_rect.y,
        12.0,
        false,
        pal.text_primary,
    );
    c.fill_rounded_rect(dp.card_rect, CARD_CORNER_RADIUS, pal.card_bg);
    for &(idx, label, ring, art, lbl_rect, selected) in &dp.tiles {
        c.fill_rounded_rect(
            art,
            10,
            if pal.is_dark {
                [58, 66, 76, 255]
            } else {
                [226, 232, 240, 255]
            },
        );
        let dock_bar = match idx {
            0 => Rect::new(art.x + 4, art.y + 12, 8, art.h - 24),
            2 => Rect::new(art.right() - 12, art.y + 12, 8, art.h - 24),
            _ => Rect::new(art.x + 12, art.bottom() - 12, art.w - 24, 8),
        };
        c.fill_rounded_rect(dock_bar, 4, pal.card_bg);
        if selected {
            c.stroke_rounded_rect(ring, 12.0, 2.0, pal.accent);
        }
        c.draw_text_centered(
            label,
            lbl_rect,
            12.0,
            false,
            if selected {
                pal.accent
            } else {
                pal.text_primary
            },
        );
    }
}

fn render_card_section(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    sec: &CardSectionLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
) {
    if let Some((title, rect)) = &sec.section_label
        && !c.is_clipped_out(*rect)
    {
        c.draw_text(title, rect.x, rect.y, 12.0, false, pal.text_primary);
    }
    if c.is_clipped_out(sec.card_rect) {
        return;
    }
    c.fill_rounded_rect(sec.card_rect, CARD_CORNER_RADIUS, pal.card_bg);
    for (idx, row) in sec.rows.iter().enumerate() {
        if c.is_clipped_out(row.rect) {
            continue;
        }
        render_card_row(c, app, row, hover, pal);
        if idx + 1 < sec.rows.len() {
            c.fill_rect(
                Rect::new(row.rect.x + 16, row.rect.bottom() - 1, row.rect.w - 32, 1),
                pal.row_separator,
            );
        }
    }
}

fn render_service_badge(c: &mut Canvas<'_>, rect: Rect, tint: [u8; 4], kind: ServiceBadgeKind) {
    blit_cached_sprite(
        c,
        rect,
        SpriteKey::ServiceBadge(kind, tint, rect.w, rect.h),
        |sc, local| {
            sc.fill_gradient_squircle(local, 8, tint);
            draw_service_badge_glyph(sc, local, kind);
        },
    );
}

fn render_notif_badge(c: &mut Canvas<'_>, rect: Rect, tint: [u8; 4], kind: NotificationAppKind) {
    blit_cached_sprite(
        c,
        rect,
        SpriteKey::NotifBadge(kind, tint, rect.w, rect.h),
        |sc, local| {
            sc.fill_gradient_squircle(local, 8, tint);
            draw_notification_app_glyph(sc, local, kind);
        },
    );
}

fn render_network_service_row(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    row: &CardRowLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
) -> bool {
    match &row.control {
        CardRowControl::ServiceStatus {
            badge_tint,
            badge_kind,
            subtitle,
            dot_rgba,
            target,
        } => {
            let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 11, 32, 32);
            render_service_badge(c, icon_rect, *badge_tint, *badge_kind);
            let tx = icon_rect.right() + 12;
            c.draw_text(
                &row.label,
                tx,
                row.rect.y + 11,
                13.0,
                true,
                pal.text_primary,
            );
            c.fill_circle(tx as f32 + 4.0, row.rect.y as f32 + 34.5, 3.8, *dot_rgba);
            c.draw_text(
                subtitle,
                tx + 12,
                row.rect.y + 28,
                11.5,
                false,
                pal.text_secondary,
            );
            if target.is_some() {
                draw_chevron(
                    c,
                    row.rect.right() as f32 - 22.0,
                    row.rect.y as f32 + row.rect.h as f32 * 0.5,
                    false,
                    pal.text_muted,
                );
            }
            true
        }
        CardRowControl::WifiHeaderSwitch {
            subtitle,
            action,
            active,
        } => {
            render_header_switch_row(
                c,
                app,
                row,
                hover,
                pal,
                [0x00, 0x7A, 0xFF, 0xFF],
                ServiceBadgeKind::Wifi,
                subtitle,
                *action,
                *active,
            );
            true
        }
        CardRowControl::ServiceHeaderSwitch { .. }
        | CardRowControl::BluetoothDeviceRow { .. }
        | CardRowControl::DropdownTrigger { .. }
        | CardRowControl::BatterySegmentedTabs
        | CardRowControl::AboutHeroCard { .. }
        | CardRowControl::UserProfileCard { .. }
        | CardRowControl::ShortcutBinding { .. } => {
            render_rusty_ui_card_row(c, app, row, hover, pal)
        }
        CardRowControl::WifiNetwork {
            connected,
            secure,
            signal_percent,
            ..
        } => {
            render_wifi_ap_row(c, row, pal, *connected, *secure, *signal_percent);
            true
        }
        _ => render_notification_settings_row(c, app, row, hover, pal),
    }
}

fn render_rusty_ui_card_row(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    row: &CardRowLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
) -> bool {
    match &row.control {
        CardRowControl::ServiceHeaderSwitch {
            badge_tint,
            badge_kind,
            subtitle,
            action,
            active,
        } => {
            render_header_switch_row(
                c,
                app,
                row,
                hover,
                pal,
                *badge_tint,
                *badge_kind,
                subtitle,
                *action,
                *active,
            );
            true
        }
        CardRowControl::BluetoothDeviceRow {
            subtitle,
            connected,
            paired,
            action,
            info_action,
        } => {
            render_bluetooth_device_row(
                c,
                app,
                row,
                hover,
                pal,
                subtitle,
                *connected,
                *paired,
                *action,
                *info_action,
            );
            true
        }
        CardRowControl::DropdownTrigger { .. } => {
            c.draw_text(
                &row.label,
                row.rect.x + 16,
                row.rect.y + (row.rect.h - 14) / 2,
                12.5,
                false,
                pal.text_primary,
            );
            let mut synced = app.low_power_dropdown.clone();
            synced.rect = to_ui_rect(dropdown_trigger_rect(row.rect));
            synced.hovered_trigger = matches!(hover, Some(SettingsHit::LowPowerDropdownTrigger));
            paint_with_ui_painter(c, app, pal, |painter, fe, ui_pal| {
                synced.paint_trigger(painter, fe, ui_pal, synced.open, 0.9);
            });
            true
        }
        CardRowControl::BatterySegmentedTabs => {
            let mut synced = app.battery_tab.clone();
            synced.rect = to_ui_rect(battery_tabs_inner_rect(row.rect));
            paint_with_ui_painter(c, app, pal, |painter, fe, ui_pal| {
                synced.paint(painter, fe, ui_pal, false, 0.0);
            });
            true
        }
        CardRowControl::AboutHeroCard { subtitle, action } => {
            render_about_hero_row(c, app, row, hover, pal, subtitle, *action);
            true
        }
        CardRowControl::UserProfileCard {
            initials,
            subtitle,
            badge_tint,
        } => {
            render_user_profile_card(c, row, pal, initials, subtitle, *badge_tint);
            true
        }
        CardRowControl::ShortcutBinding { combo, action } => {
            render_shortcut_binding_row(c, row, hover, pal, combo, *action);
            true
        }
        _ => false,
    }
}

fn render_user_profile_card(
    c: &mut Canvas<'_>,
    row: &CardRowLayout,
    pal: &Palette,
    initials: &str,
    subtitle: &str,
    badge_tint: [u8; 4],
) {
    let avatar_r = Rect::new(row.rect.x + 16, row.rect.y + 11, 40, 40);
    c.fill_rounded_rect(avatar_r, 20, badge_tint);
    c.draw_text_centered(initials, avatar_r, 15.0, true, [255, 255, 255, 255]);
    let tx = avatar_r.right() + 14;
    c.draw_text(
        &row.label,
        tx,
        row.rect.y + 13,
        13.5,
        true,
        pal.text_primary,
    );
    c.draw_text(
        subtitle,
        tx,
        row.rect.y + 34,
        11.5,
        false,
        pal.text_secondary,
    );
}

fn render_shortcut_binding_row(
    c: &mut Canvas<'_>,
    row: &CardRowLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
    combo: &str,
    action: RowControlId,
) {
    c.draw_text(
        &row.label,
        row.rect.x + 16,
        row.rect.y + (row.rect.h - 14) / 2,
        12.5,
        false,
        pal.text_primary,
    );
    let pill = shortcut_pill_rect(row.rect);
    let hovered = matches!(hover, Some(SettingsHit::Control(id)) if *id == action);
    let bg = if hovered {
        pal.sidebar_active
    } else {
        pal.control_bg
    };
    c.fill_rounded_rect(pill, 8, bg);
    c.draw_text_centered(combo, pill, 11.5, true, pal.text_primary);
}

fn render_notification_settings_row(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    row: &CardRowLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
) -> bool {
    match &row.control {
        CardRowControl::NotificationHeader {
            subtitle,
            action,
            active,
        } => {
            let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 16, 32, 32);
            render_notif_badge(
                c,
                icon_rect,
                [0xFF, 0x3B, 0x30, 0xFF],
                NotificationAppKind::Bell,
            );
            let tx = icon_rect.right() + 12;
            c.draw_text(
                &row.label,
                tx,
                row.rect.y + 13,
                13.5,
                true,
                pal.text_primary,
            );
            c.draw_text(
                subtitle,
                tx,
                row.rect.y + 33,
                11.0,
                false,
                pal.text_secondary,
            );
            draw_toggle_switch(c, switch_rect(row.rect), *action, *active, app, hover, pal);
            true
        }
        CardRowControl::NotificationAppRow {
            app_kind,
            badge_tint,
            subtitle,
            active,
            action,
        } => {
            let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 10, 32, 32);
            render_notif_badge(c, icon_rect, *badge_tint, *app_kind);
            let tx = icon_rect.right() + 12;
            c.draw_text(
                &row.label,
                tx,
                row.rect.y + 10,
                13.0,
                true,
                pal.text_primary,
            );
            c.draw_text(
                subtitle,
                tx,
                row.rect.y + 28,
                11.0,
                false,
                pal.text_secondary,
            );
            draw_toggle_switch(c, switch_rect(row.rect), *action, *active, app, hover, pal);
            true
        }
        _ => false,
    }
}

fn draw_notification_app_glyph(c: &mut Canvas<'_>, rect: Rect, kind: NotificationAppKind) {
    let cx = rect.x as f32 + rect.w as f32 * 0.5;
    let cy = rect.y as f32 + rect.h as f32 * 0.5;
    let w = [255, 255, 255, 255];
    match kind {
        NotificationAppKind::Bell => {
            c.stroke_rounded_rect(Rect::new(rect.x + 10, rect.y + 8, 12, 12), 5.0, 1.4, w);
            c.draw_segment(cx - 7.0, cy + 4.5, cx + 7.0, cy + 4.5, 0.85, w);
            c.fill_circle(cx, cy + 6.5, 1.5, w);
        }
        NotificationAppKind::Files => {
            c.fill_rounded_rect(Rect::new(rect.x + 8, rect.y + 10, 16, 12), 2, w);
            c.fill_rounded_rect(Rect::new(rect.x + 8, rect.y + 8, 8, 4), 1, w);
        }
        NotificationAppKind::Terminal => {
            c.draw_segment(cx - 6.0, cy - 3.0, cx - 2.0, cy + 0.5, 1.1, w);
            c.draw_segment(cx - 2.0, cy + 0.5, cx - 6.0, cy + 4.0, 1.1, w);
            c.draw_segment(cx, cy + 4.0, cx + 6.0, cy + 4.0, 1.0, w);
        }
        NotificationAppKind::Browser => {
            c.stroke_rounded_rect(Rect::new(rect.x + 7, rect.y + 7, 18, 18), 9.0, 1.4, w);
            c.draw_segment(cx - 3.0, cy + 3.0, cx + 3.0, cy - 3.0, 1.2, w);
        }
        NotificationAppKind::Updater => {
            c.stroke_rounded_rect(Rect::new(rect.x + 7, rect.y + 7, 18, 18), 9.0, 1.4, w);
            c.draw_segment(cx, cy - 4.0, cx, cy + 4.0, 1.0, w);
            c.draw_segment(cx - 3.5, cy + 1.0, cx, cy + 4.5, 1.0, w);
            c.draw_segment(cx + 3.5, cy + 1.0, cx, cy + 4.5, 1.0, w);
        }
        NotificationAppKind::Settings => {
            c.stroke_rounded_rect(Rect::new(rect.x + 8, rect.y + 8, 16, 16), 8.0, 2.0, w);
            c.fill_circle(cx, cy, 2.2, w);
        }
    }
}

fn render_wifi_ap_row(
    c: &mut Canvas<'_>,
    row: &CardRowLayout,
    pal: &Palette,
    connected: bool,
    secure: bool,
    signal_percent: u8,
) {
    let row_mid_y = row.rect.y as f32 + row.rect.h as f32 * 0.5;
    if connected {
        let cx = row.rect.x as f32 + 20.0;
        c.draw_segment(
            cx - 4.0,
            row_mid_y + 0.4,
            cx - 1.2,
            row_mid_y + 3.4,
            0.9,
            pal.text_primary,
        );
        c.draw_segment(
            cx - 1.2,
            row_mid_y + 3.4,
            cx + 4.2,
            row_mid_y - 3.0,
            0.9,
            pal.text_primary,
        );
    }
    c.draw_text(
        &row.label,
        row.rect.x + 38,
        row.rect.y + (row.rect.h - 14) / 2,
        12.5,
        connected,
        pal.text_primary,
    );
    if secure {
        draw_lock_icon(
            c,
            row.rect.right() as f32 - 64.0,
            row_mid_y,
            pal.text_secondary,
        );
    }
    draw_wifi_signal_icon(
        c,
        row.rect.right() as f32 - 42.0,
        row_mid_y + 4.2,
        signal_percent,
        pal,
    );
    let dots_x = row.rect.right() as f32 - 20.0;
    c.stroke_rounded_rect(
        Rect::new(dots_x as i32 - 7, row_mid_y as i32 - 7, 14, 14),
        7.0,
        1.1,
        pal.text_muted,
    );
    for dx in [-3.0_f32, 0.0, 3.0] {
        c.fill_circle(dots_x + dx, row_mid_y, 0.95, pal.text_muted);
    }
}

fn render_card_row(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    row: &CardRowLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
) {
    if render_network_service_row(c, app, row, hover, pal) {
        return;
    }

    let label_x = match &row.control {
        CardRowControl::Navigation {
            leading: Some(icon_page),
            ..
        } => {
            let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 10, 24, 24);
            render_row_icon(c, icon_rect, *icon_page);
            icon_rect.right() + 12
        }
        _ => row.rect.x + 16,
    };
    c.draw_text(
        &row.label,
        label_x,
        row.rect.y + (row.rect.h - 14) / 2,
        12.5,
        false,
        pal.text_primary,
    );

    match &row.control {
        CardRowControl::Switch { action, active } => {
            draw_toggle_switch(c, switch_rect(row.rect), *action, *active, app, hover, pal);
        }
        CardRowControl::Stepper { value, .. } => {
            let (minus, val_rect, plus) = stepper_rects(row.rect);
            for box_rect in [minus, val_rect, plus] {
                c.fill_rounded_rect(box_rect, 8, pal.control_bg);
            }
            c.draw_text_centered("-", minus, 12.5, true, pal.text_primary);
            c.draw_text_centered(value, val_rect, 11.5, false, pal.text_primary);
            c.draw_text_centered("+", plus, 12.5, true, pal.text_primary);
        }
        CardRowControl::Choice { value, .. } => {
            let pill = choice_rect(row.rect);
            c.fill_rounded_rect(pill, 8, pal.control_bg);
            c.draw_text_centered(value, pill, 11.5, false, pal.text_primary);
        }
        CardRowControl::Navigation { .. } => {
            draw_chevron(
                c,
                row.rect.right() as f32 - 22.0,
                row.rect.y as f32 + row.rect.h as f32 * 0.5,
                false,
                pal.text_muted,
            );
        }
        CardRowControl::Info { value } => {
            let vw = c.measure_text(value, 12.0, false);
            c.draw_text(
                value,
                row.rect.right() - 16 - vw,
                row.rect.y + (row.rect.h - 14) / 2,
                12.0,
                false,
                pal.text_secondary,
            );
        }
        CardRowControl::ServiceStatus { .. }
        | CardRowControl::WifiHeaderSwitch { .. }
        | CardRowControl::ServiceHeaderSwitch { .. }
        | CardRowControl::BluetoothDeviceRow { .. }
        | CardRowControl::DropdownTrigger { .. }
        | CardRowControl::BatterySegmentedTabs
        | CardRowControl::AboutHeroCard { .. }
        | CardRowControl::UserProfileCard { .. }
        | CardRowControl::ShortcutBinding { .. }
        | CardRowControl::WifiNetwork { .. }
        | CardRowControl::NotificationHeader { .. }
        | CardRowControl::NotificationAppRow { .. } => {}
    }
}

fn paint_with_ui_painter(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    pal: &Palette,
    paint_fn: impl FnOnce(
        &mut rusty_ui::UiPainter<'_>,
        &mut rusty_ui::UiFontEngine,
        &rusty_ui::ThemePalette,
    ),
) {
    let ui_pal = rusty_ui::ThemePalette::new(pal.is_dark, app.accent_index);
    let mut dl = rusty_ui::UiDrawList::new();
    with_font_engine(|fe| {
        {
            let mut painter = rusty_ui::UiPainter::new(&mut dl);
            if let Some(clip) = c.clip {
                painter.push_clip(to_ui_rect(clip));
            }
            paint_fn(&mut painter, fe, &ui_pal);
            if c.clip.is_some() {
                painter.pop_clip();
            }
        }
        let mut uc = make_ui_canvas(c.buf, c.w, c.h);
        rusty_ui::render_draw_list_to_canvas(&dl, &mut uc, fe);
    });
}

fn render_floating_overlays(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    layout: &SettingsLayout,
    pal: &Palette,
) {
    let has_dropdown = app.current_page == SettingsPage::Battery
        && app.low_power_dropdown.open
        && layout.low_power_dropdown_rect.is_some();
    if !has_dropdown && app.active_modal.is_none() {
        return;
    }
    let elapsed = app.epoch.elapsed();
    let dd_rect = layout.low_power_dropdown_rect;
    paint_with_ui_painter(c, app, pal, |painter, fe, ui_pal| {
        if has_dropdown && let Some(dr) = dd_rect {
            let mut synced = app.low_power_dropdown.clone();
            synced.rect = to_ui_rect(dr);
            synced.paint_popup(painter, fe, ui_pal);
        }
        if let Some(modal) = &app.active_modal {
            let mut sheet = modal.sheet().clone();
            let _ = sheet.sync_input_rect(layout.win_w as u32, layout.win_h as u32);
            sheet.paint(
                painter,
                fe,
                layout.win_w as u32,
                layout.win_h as u32,
                ui_pal,
                elapsed,
            );
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn render_header_switch_row(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    row: &CardRowLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
    badge_tint: [u8; 4],
    badge_kind: ServiceBadgeKind,
    subtitle: &str,
    action: RowControlId,
    active: bool,
) {
    let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 15, 32, 32);
    render_service_badge(c, icon_rect, badge_tint, badge_kind);
    let tx = icon_rect.right() + 12;
    c.draw_text(
        &row.label,
        tx,
        row.rect.y + 12,
        13.5,
        true,
        pal.text_primary,
    );
    c.draw_text(
        subtitle,
        tx,
        row.rect.y + 32,
        11.0,
        false,
        pal.text_secondary,
    );
    draw_toggle_switch(c, switch_rect(row.rect), action, active, app, hover, pal);
}

#[allow(clippy::too_many_arguments)]
fn render_bluetooth_device_row(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    row: &CardRowLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
    subtitle: &str,
    connected: bool,
    paired: bool,
    action: RowControlId,
    info_action: RowControlId,
) {
    let mid_y = row.rect.y as f32 + row.rect.h as f32 * 0.5;
    let dot_col = if connected {
        [40, 205, 65, 255]
    } else {
        pal.text_disabled
    };
    c.fill_circle(row.rect.x as f32 + 22.0, mid_y, 3.8, dot_col);
    c.draw_text(
        &row.label,
        row.rect.x + 36,
        row.rect.y + 9,
        13.0,
        connected,
        pal.text_primary,
    );
    c.draw_text(
        subtitle,
        row.rect.x + 36,
        row.rect.y + 27,
        11.0,
        false,
        pal.text_secondary,
    );

    let (act_r, info_r) = bt_device_action_rects(row.rect);
    let act_lbl = if connected {
        "Disconnect"
    } else if paired {
        "Connect"
    } else {
        "Pair"
    };
    let mut btn = rusty_ui::PushButton::new(to_ui_rect(act_r), rusty_ui::ButtonStyle::Secondary);
    if matches!(hover, Some(SettingsHit::Control(id)) if *id == action) {
        btn.state = rusty_ui::WidgetState::Hover;
    }
    paint_with_ui_painter(c, app, pal, |painter, fe, ui_pal| {
        btn.paint_labeled(painter, fe, act_lbl, ui_pal, false);
    });

    let info_hovered = matches!(hover, Some(SettingsHit::Control(id)) if *id == info_action);
    let info_col = if info_hovered {
        pal.accent
    } else {
        pal.text_muted
    };
    let icx = info_r.x as f32 + info_r.w as f32 * 0.5;
    let icy = info_r.y as f32 + info_r.h as f32 * 0.5;
    c.stroke_rounded_rect(
        Rect::new(info_r.x + 2, info_r.y + 2, info_r.w - 4, info_r.h - 4),
        9.0,
        1.2,
        info_col,
    );
    c.draw_text_centered(
        "i",
        Rect::new(icx as i32 - 6, icy as i32 - 7, 12, 14),
        11.0,
        true,
        info_col,
    );
}

fn render_about_hero_row(
    c: &mut Canvas<'_>,
    app: &SettingsAppState,
    row: &CardRowLayout,
    hover: Option<&SettingsHit>,
    pal: &Palette,
    subtitle: &str,
    action: RowControlId,
) {
    let badge_r = Rect::new(row.rect.x + 18, row.rect.y + 21, 40, 40);
    render_service_badge(
        c,
        badge_r,
        [0x00, 0x7A, 0xFF, 0xFF],
        ServiceBadgeKind::About,
    );
    let tx = badge_r.right() + 14;
    c.draw_text(
        &row.label,
        tx,
        row.rect.y + 20,
        15.0,
        true,
        pal.text_primary,
    );
    c.draw_text(
        subtitle,
        tx,
        row.rect.y + 42,
        12.0,
        false,
        pal.text_secondary,
    );

    let btn_r = about_more_info_rect(row.rect);
    let mut btn = rusty_ui::PushButton::new(to_ui_rect(btn_r), rusty_ui::ButtonStyle::Secondary);
    if matches!(hover, Some(SettingsHit::Control(id)) if *id == action) {
        btn.state = rusty_ui::WidgetState::Hover;
    }
    paint_with_ui_painter(c, app, pal, |painter, fe, ui_pal| {
        btn.paint_labeled(painter, fe, "More Info...", ui_pal, false);
    });
}

fn draw_toggle_switch(
    c: &mut Canvas<'_>,
    tr: Rect,
    action: RowControlId,
    active: bool,
    app: &SettingsAppState,
    hover: Option<&SettingsHit>,
    pal: &Palette,
) {
    let mut sw = rusty_ui::ToggleSwitch::new(to_ui_rect(tr), active);
    if matches!(hover, Some(SettingsHit::Control(id)) if *id == action) {
        sw.state = rusty_ui::WidgetState::Hover;
    }
    let progress = app.switch_progress(action, active);
    let ui_pal = rusty_ui::ThemePalette::new(pal.is_dark, app.accent_index);
    let mut dl = rusty_ui::UiDrawList::new();
    {
        let mut painter = rusty_ui::UiPainter::new(&mut dl);
        if let Some(clip) = c.clip {
            painter.push_clip(to_ui_rect(clip));
        }
        sw.paint_animated(&mut painter, progress, &ui_pal);
        if c.clip.is_some() {
            painter.pop_clip();
        }
    }
    let mut uc = make_ui_canvas(c.buf, c.w, c.h);
    with_font_engine(|fe| {
        rusty_ui::render_draw_list_to_canvas(&dl, &mut uc, fe);
    });
}

fn draw_service_badge_glyph(c: &mut Canvas<'_>, rect: Rect, kind: ServiceBadgeKind) {
    let cx = rect.x as f32 + rect.w as f32 * 0.5;
    let cy = rect.y as f32 + rect.h as f32 * 0.5;
    let w = [255, 255, 255, 255];
    match kind {
        ServiceBadgeKind::Wifi => {
            c.fill_circle(cx, cy + 5.2, 1.5, w);
            draw_wifi_arc(c, cx, cy + 5.2, 4.6, w);
            draw_wifi_arc(c, cx, cy + 5.2, 7.8, w);
        }
        ServiceBadgeKind::Ethernet => {
            c.fill_rounded_rect(Rect::new(cx as i32 - 3, cy as i32 - 8, 6, 4), 1, w);
            c.draw_segment(cx, cy - 4.0, cx, cy, 0.75, w);
            c.draw_segment(cx - 6.0, cy, cx + 6.0, cy, 0.75, w);
            c.draw_segment(cx - 6.0, cy, cx - 6.0, cy + 4.0, 0.75, w);
            c.draw_segment(cx + 6.0, cy, cx + 6.0, cy + 4.0, 0.75, w);
            c.fill_rounded_rect(Rect::new(cx as i32 - 9, cy as i32 + 3, 6, 4), 1, w);
            c.fill_rounded_rect(Rect::new(cx as i32 + 3, cy as i32 + 3, 6, 4), 1, w);
        }
        ServiceBadgeKind::Firewall => {
            c.stroke_rounded_rect(Rect::new(rect.x + 8, rect.y + 7, 16, 18), 5.5, 1.5, w);
            c.draw_segment(cx - 3.0, cy - 1.5, cx + 3.0, cy - 1.5, 0.85, w);
            c.draw_segment(cx + 1.0, cy - 3.5, cx + 3.2, cy - 1.5, 0.85, w);
            c.draw_segment(cx - 3.0, cy + 2.5, cx + 3.0, cy + 2.5, 0.85, w);
            c.draw_segment(cx - 3.2, cy + 2.5, cx - 1.0, cy + 4.5, 0.85, w);
        }
        ServiceBadgeKind::Bluetooth => draw_bluetooth_glyph(c, cx, cy, 7.0, w),
        ServiceBadgeKind::Battery => {
            c.stroke_rounded_rect(Rect::new(rect.x + 6, rect.y + 10, 18, 12), 2.5, 1.3, w);
            c.fill_rounded_rect(Rect::new(rect.x + 25, rect.y + 13, 2, 6), 1, w);
            c.fill_rounded_rect(Rect::new(rect.x + 9, rect.y + 13, 12, 6), 1, w);
        }
        ServiceBadgeKind::About => {
            c.stroke_rounded_rect(Rect::new(rect.x + 7, rect.y + 8, 26, 18), 3.0, 1.4, w);
            c.draw_segment(cx, cy + 5.0, cx, cy + 9.0, 1.3, w);
            c.draw_segment(cx - 5.5, cy + 9.0, cx + 5.5, cy + 9.0, 1.2, w);
        }
    }
}

fn draw_bluetooth_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, h: f32, w: [u8; 4]) {
    c.draw_segment(cx, cy - h, cx, cy + h, 1.1, w);
    c.draw_segment(cx, cy - h, cx + h * 0.55, cy - h * 0.45, 1.1, w);
    c.draw_segment(
        cx + h * 0.55,
        cy - h * 0.45,
        cx - h * 0.55,
        cy + h * 0.45,
        1.1,
        w,
    );
    c.draw_segment(cx, cy + h, cx + h * 0.55, cy + h * 0.45, 1.1, w);
    c.draw_segment(
        cx + h * 0.55,
        cy + h * 0.45,
        cx - h * 0.55,
        cy - h * 0.45,
        1.1,
        w,
    );
}

fn draw_lock_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_rounded_rect(Rect::new(cx as i32 - 3, cy as i32 - 6, 6, 6), 3.0, 1.1, col);
    c.fill_rounded_rect(Rect::new(cx as i32 - 4, cy as i32 - 1, 8, 6), 2, col);
}

fn draw_wifi_signal_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, signal_percent: u8, pal: &Palette) {
    let dim = pal.text_disabled;
    let lit = pal.text_primary;
    c.fill_circle(cx, cy, 1.25, lit);
    draw_wifi_arc(c, cx, cy, 3.6, if signal_percent > 28 { lit } else { dim });
    draw_wifi_arc(c, cx, cy, 6.2, if signal_percent > 54 { lit } else { dim });
}

fn render_row_icon(c: &mut Canvas<'_>, rect: Rect, page: SettingsPage) {
    blit_cached_sprite(
        c,
        rect,
        SpriteKey::RowIcon(page, rect.w, rect.h),
        |sc, local| render_row_icon_direct(sc, local, page),
    );
}

fn render_row_icon_direct(c: &mut Canvas<'_>, rect: Rect, page: SettingsPage) {
    c.fill_gradient_squircle(rect, 6, page.icon_tint());
    if let Some((sw, sh, rgba)) =
        crate::svg_icons::decode_png(crate::svg_icons::sidebar_icon_png(page))
    {
        let pad = (rect.w / 6).max(3);
        let glyph_rect = Rect::new(
            rect.x + pad,
            rect.y + pad,
            (rect.w - pad * 2).max(8),
            (rect.h - pad * 2).max(8),
        );
        blit_rgba_sprite_rounded(c, glyph_rect, sw, sh, &rgba, 0.0, true);
    } else {
        draw_sidebar_glyph(c, rect, page);
    }
}

#[allow(clippy::cast_precision_loss)]
fn draw_sidebar_glyph(c: &mut Canvas<'_>, rect: Rect, page: SettingsPage) {
    let cx = rect.x as f32 + rect.w as f32 * 0.5;
    let cy = rect.y as f32 + rect.h as f32 * 0.5;
    let w = [255, 255, 255, 255];
    match page {
        SettingsPage::Wifi => {
            c.fill_circle(cx, cy + 4.2, 1.3, w);
            draw_wifi_arc(c, cx, cy + 4.2, 4.0, w);
            draw_wifi_arc(c, cx, cy + 4.2, 6.8, w);
        }
        SettingsPage::Bluetooth
        | SettingsPage::Battery
        | SettingsPage::About
        | SettingsPage::Users
        | SettingsPage::Shortcuts
        | SettingsPage::MenuBar
        | SettingsPage::DateTime
        | SettingsPage::Language => draw_extra_sidebar_glyph(c, rect, page),
        SettingsPage::Network => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 5, 14, 14), 7.0, 1.3, w);
            c.stroke_rounded_rect(Rect::new(rect.x + 9, rect.y + 5, 6, 14), 3.0, 1.1, w);
            c.draw_segment(cx - 6.0, cy, cx + 6.0, cy, 0.55, w);
        }
        SettingsPage::Appearance => {
            // White ring + filled left half
            for py in rect.y + 4..rect.bottom() - 4 {
                for px in rect.x + 4..rect.right() - 4 {
                    let cov = aa_cov8(px, py, |x, y| {
                        let dx = x - cx;
                        let dy = y - cy;
                        let r = dx.hypot(dy);
                        let ring_d = (r - 5.9).abs() - 0.75;
                        let fill_d = (r - 6.2).max(dx);
                        ring_d.min(fill_d)
                    });
                    if cov > 0 {
                        c.blend_px(px, py, w, cov);
                    }
                }
            }
        }
        SettingsPage::Wallpaper => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 6, 14, 12), 2.5, 1.2, w);
            c.fill_circle(cx - 2.6, cy - 1.5, 1.3, w);
            c.draw_segment(cx - 3.0, cy + 3.5, cx + 0.5, cy, 1.2, w);
            c.draw_segment(cx + 0.5, cy, cx + 4.0, cy + 3.5, 1.2, w);
        }
        SettingsPage::DockDesktop => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 6, 14, 12), 2.5, 1.2, w);
            c.fill_rounded_rect(Rect::new(rect.x + 8, rect.y + 14, 8, 2), 1, w);
        }
        SettingsPage::Notifications => {
            c.stroke_rounded_rect(Rect::new(rect.x + 7, rect.y + 6, 10, 10), 4.0, 1.2, w);
            c.draw_segment(cx - 5.2, cy + 3.8, cx + 5.2, cy + 3.8, 0.7, w);
            c.fill_circle(cx, cy + 5.3, 1.2, w);
        }
        SettingsPage::SoftwareUpdate => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 5, 14, 14), 7.0, 1.3, w);
            c.draw_segment(cx, cy - 3.5, cx, cy + 2.5, 1.2, w);
            c.draw_segment(cx - 2.5, cy, cx, cy + 2.5, 1.1, w);
            c.draw_segment(cx + 2.5, cy, cx, cy + 2.5, 1.1, w);
        }
        SettingsPage::Display => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 5, 14, 11), 2.0, 1.3, w);
            c.draw_segment(cx - 3.0, cy + 5.8, cx + 3.0, cy + 5.8, 0.7, w);
            c.draw_segment(cx, cy + 4.0, cx, cy + 5.8, 0.9, w);
        }
        SettingsPage::Sound => {
            c.fill_rounded_rect(Rect::new(rect.x + 6, rect.y + 10, 3, 4), 1, w);
            c.draw_segment(cx - 3.5, cy, cx + 0.5, cy - 3.5, 1.2, w);
            c.draw_segment(cx - 3.5, cy, cx + 0.5, cy + 3.5, 1.2, w);
            c.draw_segment(cx + 0.5, cy - 3.5, cx + 0.5, cy + 3.5, 1.2, w);
            c.draw_segment(cx + 3.5, cy - 2.5, cx + 3.5, cy + 2.5, 0.75, w);
            c.draw_segment(cx + 5.6, cy - 4.0, cx + 5.6, cy + 4.0, 0.75, w);
        }
        SettingsPage::Keyboard => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 7, 14, 10), 2.0, 1.2, w);
            for dx in [-4, -1, 2, 4] {
                c.fill_rect(Rect::new(cx as i32 + dx, cy as i32 - 2, 1, 1), w);
                c.fill_rect(Rect::new(cx as i32 + dx, cy as i32, 1, 1), w);
            }
            c.fill_rect(Rect::new(cx as i32 - 3, cy as i32 + 2, 6, 1), w);
        }
        SettingsPage::Mouse | SettingsPage::CursorStyle => {
            c.draw_segment(cx - 3.0, cy - 4.5, cx - 1.5, cy + 3.5, 1.0, w);
            c.draw_segment(cx - 3.0, cy - 4.5, cx + 3.8, cy - 0.5, 1.0, w);
            c.draw_segment(cx - 1.5, cy + 3.5, cx + 1.2, cy + 1.2, 0.9, w);
            c.draw_segment(cx + 1.2, cy + 1.2, cx + 3.5, cy + 4.5, 1.1, w);
        }
        SettingsPage::Touchpad => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 6, 14, 12), 2.5, 1.2, w);
            c.draw_segment(cx - 6.0, cy + 2.5, cx + 6.0, cy + 2.5, 0.55, w);
            c.draw_segment(cx, cy + 2.5, cx, cy + 5.2, 0.55, w);
        }
        SettingsPage::Fonts => c.draw_text_centered("A", rect, 14.0, true, w),
        SettingsPage::IconTheme => {
            for (dx, dy) in [(-5, -5), (1, -5), (-5, 1), (1, 1)] {
                c.fill_rounded_rect(Rect::new(cx as i32 + dx, cy as i32 + dy, 4, 4), 1, w);
            }
        }
    }
}

fn draw_extra_sidebar_glyph(c: &mut Canvas<'_>, rect: Rect, page: SettingsPage) {
    let cx = rect.x as f32 + rect.w as f32 * 0.5;
    let cy = rect.y as f32 + rect.h as f32 * 0.5;
    let w = [255, 255, 255, 255];
    match page {
        SettingsPage::Bluetooth => draw_bluetooth_glyph(c, cx, cy, 5.8, w),
        SettingsPage::Battery => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 8, 12, 8), 2.0, 1.1, w);
            c.fill_rounded_rect(Rect::new(rect.x + 18, rect.y + 10, 2, 4), 1, w);
            c.fill_rounded_rect(Rect::new(rect.x + 7, rect.y + 10, 8, 4), 1, w);
        }
        SettingsPage::About => {
            c.stroke_rounded_rect(Rect::new(rect.x + 5, rect.y + 5, 14, 14), 7.0, 1.2, w);
            c.fill_circle(cx, cy - 3.2, 1.1, w);
            c.draw_segment(cx, cy - 0.5, cx, cy + 4.0, 1.2, w);
        }
        SettingsPage::Users => {
            c.fill_circle(cx - 1.5, cy - 2.4, 2.3, w);
            c.stroke_rounded_rect(Rect::new(rect.x + 6, cy as i32 + 1, 9, 5), 2.5, 1.1, w);
            c.fill_circle(cx + 3.8, cy - 1.5, 1.7, w);
        }
        SettingsPage::Shortcuts => {
            c.stroke_rounded_rect(Rect::new(rect.x + 6, rect.y + 6, 4, 4), 1.5, 1.0, w);
            c.stroke_rounded_rect(Rect::new(rect.x + 14, rect.y + 6, 4, 4), 1.5, 1.0, w);
            c.stroke_rounded_rect(Rect::new(rect.x + 6, rect.y + 14, 4, 4), 1.5, 1.0, w);
            c.stroke_rounded_rect(Rect::new(rect.x + 14, rect.y + 14, 4, 4), 1.5, 1.0, w);
            c.stroke_rounded_rect(Rect::new(rect.x + 9, rect.y + 9, 6, 6), 0.5, 1.0, w);
        }
        _ => {}
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn draw_wifi_arc(c: &mut Canvas<'_>, cx: f32, cy: f32, r: f32, col: [u8; 4]) {
    let x0 = (cx - r - 1.5).floor() as i32;
    let y0 = (cy - r - 1.5).floor() as i32;
    let x1 = (cx + r + 1.5).ceil() as i32;
    let y1 = cy.ceil() as i32;
    for py in y0..y1 {
        for px in x0..x1 {
            let cov = aa_cov8(px, py, |x, y| {
                let dx = x - cx;
                let dy = y - cy;
                if dy > -dx.abs() * 0.8 {
                    return 1.0e3;
                }
                (dx.hypot(dy) - r).abs() - 0.65
            });
            if cov > 0 {
                c.blend_px(px, py, col, cov);
            }
        }
    }
}

fn draw_chevron(c: &mut Canvas<'_>, cx: f32, cy: f32, left: bool, col: [u8; 4]) {
    let dir = if left { -1.0 } else { 1.0 };
    c.draw_segment(cx - 2.0 * dir, cy - 3.8, cx + 2.0 * dir, cy, 0.75, col);
    c.draw_segment(cx + 2.0 * dir, cy, cx - 2.0 * dir, cy + 3.8, 0.75, col);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_rounded_rect_matches_bruteforce_sdf_coverage() {
        let w = 80_u32;
        let h = 50_u32;
        let rect = Rect::new(8, 6, 60, 34);
        let color = [0x22, 0x66, 0xAA, 0xFF];

        let mut fast_buf = vec![0_u8; (w * h * 4) as usize];
        let mut fast = Canvas::new(&mut fast_buf, w, h);
        fast.fill_rounded_rect(rect, 10, color);

        let mut ref_buf = vec![0_u8; (w * h * 4) as usize];
        let mut ref_c = Canvas::new(&mut ref_buf, w, h);
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        for py in rect.y..rect.bottom() {
            for px in rect.x..rect.right() {
                let cov = aa_cov8(px, py, |x, y| {
                    sd_rounded_box(x - cx, y - cy, rect.w as f32, rect.h as f32, 10.0)
                });
                if cov > 0 {
                    ref_c.blend_px(px, py, color, cov);
                }
            }
        }
        assert_eq!(fast_buf, ref_buf);
    }

    #[test]
    fn cached_row_icon_and_theme_preview_match_direct_render() {
        let w = 140_u32;
        let h = 90_u32;
        let icon_rect = Rect::new(10, 10, 24, 24);
        let preview_rect = Rect::new(20, 12, 108, 68);

        let mut direct_buf = vec![240_u8; (w * h * 4) as usize];
        {
            let mut dc = Canvas::new(&mut direct_buf, w, h);
            render_row_icon_direct(&mut dc, icon_rect, SettingsPage::Appearance);
            render_theme_preview_direct(&mut dc, preview_rect, ThemeChoice::Auto);
        }

        let mut cached_buf = vec![240_u8; (w * h * 4) as usize];
        for _ in 0..2 {
            cached_buf.fill(240);
            let mut cc = Canvas::new(&mut cached_buf, w, h);
            render_row_icon(&mut cc, icon_rect, SettingsPage::Appearance);
            render_theme_preview_artwork(&mut cc, preview_rect, ThemeChoice::Auto);
        }

        assert_eq!(cached_buf, direct_buf);
    }

    #[test]
    fn sidebar_glass_and_unfocused_traffic_lights_render_cleanly() {
        let mut app = SettingsAppState::new(&crate::state::StartupOptions::default());
        app.focused = false;
        let layout = crate::layout::compute_layout(&app, 882, 600);
        let mut buf = vec![0_u8; 882 * 600 * 4];
        render_window_pixels_with_glass(&mut buf, 882, 600, &app, &layout, None, true);
        let side_idx = ((300 * 882 + 12) * 4) as usize;
        let page_idx = ((300 * 882 + 400) * 4) as usize;
        assert!(buf[side_idx + 3] < 200);
        assert_eq!(buf[page_idx + 3], 255);

        let tl = rusty_ui::TrafficLightButtons::default_csd();
        let (cx, cy) = tl.center_of(rusty_ui::TrafficLightKind::Close);
        let tl_idx = ((cy as usize) * 882 + (cx as usize)) * 4;
        let inactive = rusty_ui::TrafficLightKind::inactive_color();
        assert_eq!(
            &buf[tl_idx..tl_idx + 4],
            &[inactive[2], inactive[1], inactive[0], 255]
        );
    }
}
