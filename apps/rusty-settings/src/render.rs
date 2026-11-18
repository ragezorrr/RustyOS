//! Pure CPU premultiplied `ARGB8888` renderer for `rusty-settings` (`RUSTYOS-103`).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};

use crate::layout::{
    AppearanceCardsLayout, CARD_CORNER_RADIUS, CardRowControl, CardRowLayout, CardSectionLayout,
    DockPositionCardLayout, NotificationAppKind, Rect, ServiceBadgeKind, SettingsHit,
    SettingsLayout, WINDOW_CORNER_RADIUS, choice_rect, stepper_rects, switch_rect,
};
use crate::state::{ACCENT_COLORS, SettingsAppState, SettingsPage, ThemeChoice};

const INTER_REGULAR_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

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
        RefCell::new(HashMap::with_capacity(256));
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
        if let std::collections::hash_map::Entry::Vacant(entry) = map.entry(key) {
            let font = if bold {
                semibold_font()
            } else {
                regular_font()
            }?;
            let (metrics, bitmap) = font.rasterize(ch, size_px);
            entry.insert(CachedGlyph {
                xmin: metrics.xmin,
                ymin: metrics.ymin,
                width: metrics.width,
                height: metrics.height,
                advance: metrics.advance_width.round() as i32,
                bitmap,
            });
        }
        map.get(&key).map(f)
    })
}

#[derive(Clone, Copy)]
struct Palette {
    is_dark: bool,
    sidebar_bg: [u8; 4],
    divider: [u8; 4],
    page_bg: [u8; 4],
    card_bg: [u8; 4],
    row_separator: [u8; 4],
    search_bg: [u8; 4],
    sidebar_active: [u8; 4],
    sidebar_hover: [u8; 4],
    control_bg: [u8; 4],
    switch_off: [u8; 4],
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
            search_bg: [50, 50, 52, 255],
            sidebar_active: [62, 62, 65, 255],
            sidebar_hover: [52, 52, 55, 255],
            control_bg: [60, 60, 64, 255],
            switch_off: [72, 72, 76, 255],
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
            search_bg: [255, 255, 255, 255],
            sidebar_active: [217, 218, 221, 255],
            sidebar_hover: [226, 227, 231, 255],
            control_bg: [242, 244, 248, 255],
            switch_off: [212, 214, 220, 255],
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

    fn fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
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
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend_px(px, py, color, 255);
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn fill_rounded_rect(&mut self, rect: Rect, radius: i32, color: [u8; 4]) {
        let x0 = rect.x.max(0);
        let y0 = rect.y.max(0);
        let x1 = rect.right().min(self.w);
        let y1 = rect.bottom().min(self.h);
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        let rw = rect.w as f32;
        let rh = rect.h as f32;
        let rad = radius as f32;
        for py in y0..y1 {
            for px in x0..x1 {
                let cov = aa_cov8(px, py, |x, y| sd_rounded_box(x - cx, y - cy, rw, rh, rad));
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
    fn stroke_rounded_rect(&mut self, rect: Rect, radius: f32, stroke_w: f32, color: [u8; 4]) {
        let x0 = rect.x.max(0);
        let y0 = rect.y.max(0);
        let x1 = rect.right().min(self.w);
        let y1 = rect.bottom().min(self.h);
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        let rw = rect.w as f32;
        let rh = rect.h as f32;
        for py in y0..y1 {
            for px in x0..x1 {
                let cov = aa_cov8(px, py, |x, y| {
                    let d = sd_rounded_box(x - cx, y - cy, rw, rh, radius);
                    // Inside `[ -stroke_w, 0 ]`
                    (d + stroke_w * 0.5).abs() - stroke_w * 0.5
                });
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
    fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        let x0 = (cx - radius - 1.5).floor() as i32;
        let y0 = (cy - radius - 1.5).floor() as i32;
        let x1 = (cx + radius + 1.5).ceil() as i32;
        let y1 = (cy + radius + 1.5).ceil() as i32;
        for py in y0.max(0)..y1.min(self.h) {
            for px in x0.max(0)..x1.min(self.w) {
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
        let top_tint = [
            (f32::from(bottom_tint[0]) * 1.15).round().min(255.0) as u8,
            (f32::from(bottom_tint[1]) * 1.15).round().min(255.0) as u8,
            (f32::from(bottom_tint[2]) * 1.15).round().min(255.0) as u8,
            255,
        ];
        let x0 = rect.x.max(0);
        let y0 = rect.y.max(0);
        let x1 = rect.right().min(self.w);
        let y1 = rect.bottom().min(self.h);
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        let rw = rect.w as f32;
        let rh = rect.h as f32;
        let rad = radius as f32;
        for py in y0..y1 {
            let t = ((py - rect.y) as f32 / rh.max(1.0)).clamp(0.0, 1.0);
            let col = lerp_rgba(top_tint, bottom_tint, t);
            for px in x0..x1 {
                let cov = aa_cov8(px, py, |x, y| sd_rounded_box(x - cx, y - cy, rw, rh, rad));
                if cov > 0 {
                    self.blend_px(px, py, col, cov);
                }
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn draw_segment(&mut self, ax: f32, ay: f32, bx: f32, by: f32, radius: f32, color: [u8; 4]) {
        let x0 = (ax.min(bx) - radius - 2.0).floor() as i32;
        let y0 = (ay.min(by) - radius - 2.0).floor() as i32;
        let x1 = (ax.max(bx) + radius + 2.0).ceil() as i32;
        let y1 = (ay.max(by) + radius + 2.0).ceil() as i32;
        for py in y0.max(0)..y1.min(self.h) {
            for px in x0.max(0)..x1.min(self.w) {
                let cov = aa_cov8(px, py, |x, y| sd_segment(x, y, ax, ay, bx, by, radius));
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
    fn measure_text(&self, text: &str, size_px: f32, bold: bool) -> i32 {
        let _ = self.w;
        text.chars()
            .filter_map(|ch| with_cached_glyph(ch, size_px, bold, |g| g.advance))
            .sum()
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss
    )]
    fn draw_text(&mut self, text: &str, x: i32, y: i32, size_px: f32, bold: bool, color: [u8; 4]) {
        let baseline = y + (size_px * 0.84).round() as i32;
        let mut pen_x = x;
        for ch in text.chars() {
            let Some(adv) = with_cached_glyph(ch, size_px, bold, |g| {
                let gx = pen_x + g.xmin;
                let gy = baseline - g.ymin - g.height as i32;
                for row in 0..g.height {
                    for col in 0..g.width {
                        let cov = g.bitmap[row * g.width + col];
                        if cov > 0 {
                            self.blend_px(gx + col as i32, gy + row as i32, color, cov);
                        }
                    }
                }
                g.advance
            }) else {
                continue;
            };
            pen_x += adv;
        }
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

pub(crate) fn render_window_pixels(
    buf: &mut [u8],
    width: u32,
    height: u32,
    app: &SettingsAppState,
    layout: &SettingsLayout,
    hover: Option<&SettingsHit>,
) {
    let pal = palette_for(app);
    let mut c = Canvas::new(buf, width, height);

    // 1. Flat split-pane background: grey sidebar (#ECEDF1) left + light canvas (#F5F6FA) right.
    c.fill_rect(layout.sidebar_rect, pal.sidebar_bg);
    c.fill_rect(Rect::new(layout.divider_x, 0, 1, layout.win_h), pal.divider);
    c.fill_rect(layout.page_rect, pal.page_bg);

    // 2. Left sidebar header (traffic lights + borderless capsule search) and scrollable items.
    render_sidebar(&mut c, app, layout, &pal, hover);

    // 3. Scrollable right page content clipped cleanly below header, then header + scrollbar thumb.
    c.clip = Some(layout.page_viewport);
    if let Some(app_cards) = &layout.appearance {
        render_appearance_page(&mut c, app_cards, &pal);
    } else if let Some(dp) = &layout.dock_positions {
        render_dock_positions_card(&mut c, dp, &pal);
    }
    for sec in &layout.sections {
        render_card_section(&mut c, sec, &pal);
    }
    c.clip = None;
    render_page_header(&mut c, app, layout, &pal);
    if let Some(thumb) = layout.page_scrollbar_thumb {
        let thumb_col = if pal.is_dark {
            [255, 255, 255, 92]
        } else {
            [0, 0, 0, 68]
        };
        c.fill_rounded_rect(thumb, 2, thumb_col);
    }

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

fn render_sidebar(
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
    let lights = [
        (layout.traffic_close, [0xEE, 0x5A, 0x6A, 0xFF], 'x'),
        (layout.traffic_min, [0xF4, 0xB6, 0x3F, 0xFF], '-'),
        (layout.traffic_max, [0x3C, 0xC6, 0x8A, 0xFF], '+'),
    ];
    for (rect, col, glyph) in lights {
        let cx = rect.x as f32 + 7.0;
        let cy = rect.y as f32 + 7.0;
        c.fill_circle(cx, cy, 7.0, col);
        if tl_hovered {
            draw_traffic_glyph(c, cx, cy, glyph);
        }
    }

    // Circular Search capsule without border when unfocused
    c.fill_rounded_rect(layout.search_rect, 14, pal.search_bg);
    if app.search_focused {
        let focus_col = [pal.accent[0], pal.accent[1], pal.accent[2], 140];
        c.stroke_rounded_rect(layout.search_rect, 14.0, 1.5, focus_col);
    }
    draw_search_loupe(
        c,
        (layout.search_rect.x + 15) as f32,
        (layout.search_rect.y + 14) as f32,
        pal.text_muted,
    );
    if app.search_query.is_empty() {
        c.draw_text(
            "Search",
            layout.search_rect.x + 28,
            layout.search_rect.y + 7,
            12.0,
            false,
            pal.text_muted,
        );
    } else {
        c.draw_text(
            &app.search_query,
            layout.search_rect.x + 28,
            layout.search_rect.y + 7,
            12.0,
            false,
            pal.text_primary,
        );
    }
    if app.search_focused && app.caret_visible {
        let tw = c.measure_text(&app.search_query, 12.0, false);
        c.fill_rect(
            Rect::new(
                layout.search_rect.x + 28 + tw + 1,
                layout.search_rect.y + 7,
                1,
                14,
            ),
            pal.accent,
        );
    }

    c.clip = Some(layout.sidebar_viewport);
    for cat in &layout.categories {
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
        let is_hovered = hover == Some(&SettingsHit::SidebarItem(item.page));
        if item.selected {
            c.fill_rounded_rect(item.rect, 10, pal.sidebar_active);
        } else if is_hovered {
            c.fill_rounded_rect(item.rect, 10, pal.sidebar_hover);
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

fn draw_traffic_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, kind: char) {
    let ink = [0, 0, 0, 150];
    match kind {
        'x' => {
            c.draw_segment(cx - 2.6, cy - 2.6, cx + 2.6, cy + 2.6, 0.7, ink);
            c.draw_segment(cx + 2.6, cy - 2.6, cx - 2.6, cy + 2.6, 0.7, ink);
        }
        '-' => c.draw_segment(cx - 3.2, cy, cx + 3.2, cy, 0.75, ink),
        '+' => {
            c.draw_segment(cx - 3.0, cy, cx + 3.0, cy, 0.75, ink);
            c.draw_segment(cx, cy - 3.0, cx, cy + 3.0, 0.75, ink);
        }
        _ => {}
    }
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

fn render_appearance_page(c: &mut Canvas<'_>, app: &AppearanceCardsLayout, pal: &Palette) {
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
        render_card_row(c, row, pal);
        if idx + 1 < app.links.len() {
            c.fill_rect(
                Rect::new(row.rect.x + 16, row.rect.bottom() - 1, row.rect.w - 32, 1),
                pal.row_separator,
            );
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn render_theme_preview_artwork(c: &mut Canvas<'_>, art: Rect, choice: ThemeChoice) {
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

fn render_card_section(c: &mut Canvas<'_>, sec: &CardSectionLayout, pal: &Palette) {
    if let Some((title, rect)) = &sec.section_label {
        c.draw_text(title, rect.x, rect.y, 12.0, false, pal.text_primary);
    }
    c.fill_rounded_rect(sec.card_rect, CARD_CORNER_RADIUS, pal.card_bg);
    for (idx, row) in sec.rows.iter().enumerate() {
        render_card_row(c, row, pal);
        if idx + 1 < sec.rows.len() {
            c.fill_rect(
                Rect::new(row.rect.x + 16, row.rect.bottom() - 1, row.rect.w - 32, 1),
                pal.row_separator,
            );
        }
    }
}

fn render_network_service_row(c: &mut Canvas<'_>, row: &CardRowLayout, pal: &Palette) -> bool {
    match &row.control {
        CardRowControl::ServiceStatus {
            badge_tint,
            badge_kind,
            subtitle,
            dot_rgba,
            target,
        } => {
            let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 11, 32, 32);
            c.fill_gradient_squircle(icon_rect, 8, *badge_tint);
            draw_service_badge_glyph(c, icon_rect, *badge_kind);
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
            subtitle, active, ..
        } => {
            let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 15, 32, 32);
            c.fill_gradient_squircle(icon_rect, 8, [0x00, 0x7A, 0xFF, 0xFF]);
            draw_service_badge_glyph(c, icon_rect, ServiceBadgeKind::Wifi);
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
            draw_toggle_switch(c, switch_rect(row.rect), *active, pal);
            true
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
        _ => render_notification_settings_row(c, row, pal),
    }
}

fn render_notification_settings_row(
    c: &mut Canvas<'_>,
    row: &CardRowLayout,
    pal: &Palette,
) -> bool {
    match &row.control {
        CardRowControl::NotificationHeader {
            subtitle, active, ..
        } => {
            let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 16, 32, 32);
            c.fill_gradient_squircle(icon_rect, 8, [0xFF, 0x3B, 0x30, 0xFF]);
            draw_notification_app_glyph(c, icon_rect, NotificationAppKind::Bell);
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
            draw_toggle_switch(c, switch_rect(row.rect), *active, pal);
            true
        }
        CardRowControl::NotificationAppRow {
            app_kind,
            badge_tint,
            subtitle,
            active,
            ..
        } => {
            let icon_rect = Rect::new(row.rect.x + 16, row.rect.y + 10, 32, 32);
            c.fill_gradient_squircle(icon_rect, 8, *badge_tint);
            draw_notification_app_glyph(c, icon_rect, *app_kind);
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
            draw_toggle_switch(c, switch_rect(row.rect), *active, pal);
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

fn render_card_row(c: &mut Canvas<'_>, row: &CardRowLayout, pal: &Palette) {
    if render_network_service_row(c, row, pal) {
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
        CardRowControl::Switch { active, .. } => {
            draw_toggle_switch(c, switch_rect(row.rect), *active, pal);
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
        | CardRowControl::WifiNetwork { .. }
        | CardRowControl::NotificationHeader { .. }
        | CardRowControl::NotificationAppRow { .. } => {}
    }
}

fn draw_toggle_switch(c: &mut Canvas<'_>, tr: Rect, active: bool, pal: &Palette) {
    c.fill_rounded_rect(tr, 10, if active { pal.accent } else { pal.switch_off });
    let knob_cx = if active {
        tr.right() as f32 - 10.0
    } else {
        tr.x as f32 + 10.0
    };
    c.fill_circle(knob_cx, tr.y as f32 + 10.0, 8.0, [255, 255, 255, 255]);
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
    }
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
    c.fill_gradient_squircle(rect, 6, page.icon_tint());
    draw_sidebar_glyph(c, rect, page);
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

fn draw_search_loupe(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_rounded_rect(Rect::new(cx as i32 - 5, cy as i32 - 5, 9, 9), 4.5, 1.3, col);
    c.draw_segment(cx + 2.0, cy + 2.0, cx + 5.0, cy + 5.0, 0.75, col);
}

fn draw_chevron(c: &mut Canvas<'_>, cx: f32, cy: f32, left: bool, col: [u8; 4]) {
    let dir = if left { -1.0 } else { 1.0 };
    c.draw_segment(cx - 2.0 * dir, cy - 3.8, cx + 2.0 * dir, cy, 0.75, col);
    c.draw_segment(cx + 2.0 * dir, cy, cx - 2.0 * dir, cy + 3.8, 0.75, col);
}
