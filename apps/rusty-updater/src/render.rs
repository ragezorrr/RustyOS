//! Pure CPU premultiplied `ARGB8888` renderer for `rusty-updater` (`RUSTYOS-077`).
//!
//! Adheres to `macOS 27 Golden Gate Light & Dark` tokens (`DESIGN.md §5.1, §12`) using
//! bundled `Inter` fonts (`fontdue`) and SDF shapes with strict scissor clipping.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};
use rusty_services::file_manager::FilesThemeMode;
use rusty_services::updates::{PackageUpdateItem, UpdateCheckState, UpdateTxPhase};

use crate::layout::{Rect, UpdaterHit, UpdaterLayout, WINDOW_CORNER_RADIUS};
use crate::state::{LOG_LINE_HEIGHT, ROW_HEIGHT, UpdaterAppState};

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

pub(crate) struct Palette {
    pub window_bg: [u8; 4],
    pub window_border: [u8; 4],
    pub card_bg: [u8; 4],
    pub pill_bg: [u8; 4],
    pub pill_hover: [u8; 4],
    pub row_alt: [u8; 4],
    pub row_selected: [u8; 4],
    pub divider: [u8; 4],
    pub accent: [u8; 4],
    pub accent_hover: [u8; 4],
    pub accent_soft: [u8; 4],
    pub text_primary: [u8; 4],
    pub text_secondary: [u8; 4],
    pub text_muted: [u8; 4],
    pub success: [u8; 4],
    pub warning: [u8; 4],
    pub danger: [u8; 4],
    pub track_bg: [u8; 4],
    pub scrollbar: [u8; 4],
}

#[must_use]
pub(crate) const fn palette_for(theme: FilesThemeMode) -> Palette {
    match theme {
        FilesThemeMode::LightGoldenGate => Palette {
            window_bg: [245, 246, 250, 255],
            window_border: [0, 0, 0, 24],
            card_bg: [255, 255, 255, 255],
            pill_bg: [255, 255, 255, 255],
            pill_hover: [236, 237, 241, 255],
            row_alt: [249, 250, 253, 255],
            row_selected: [224, 237, 255, 255],
            divider: [238, 239, 243, 255],
            accent: [0, 122, 255, 255],
            accent_hover: [0, 108, 235, 255],
            accent_soft: [228, 240, 255, 255],
            text_primary: [29, 31, 36, 255],
            text_secondary: [102, 110, 124, 255],
            text_muted: [134, 138, 146, 255],
            success: [52, 199, 89, 255],
            warning: [245, 166, 35, 255],
            danger: [229, 72, 77, 255],
            track_bg: [226, 229, 236, 255],
            scrollbar: [152, 158, 170, 165],
        },
        FilesThemeMode::DarkPurpleGlass => Palette {
            window_bg: [28, 28, 29, 255],
            window_border: [255, 255, 255, 22],
            card_bg: [44, 44, 45, 255],
            pill_bg: [44, 44, 45, 255],
            pill_hover: [58, 58, 62, 255],
            row_alt: [38, 38, 40, 255],
            row_selected: [22, 58, 98, 255],
            divider: [56, 56, 60, 255],
            accent: [10, 132, 255, 255],
            accent_hover: [36, 148, 255, 255],
            accent_soft: [28, 56, 92, 255],
            text_primary: [242, 242, 247, 255],
            text_secondary: [174, 174, 182, 255],
            text_muted: [134, 134, 142, 255],
            success: [48, 209, 88, 255],
            warning: [255, 179, 64, 255],
            danger: [255, 92, 92, 255],
            track_bg: [58, 58, 64, 255],
            scrollbar: [158, 162, 172, 150],
        },
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

    fn effective_clip(&self, r: Rect) -> Rect {
        let mut x0 = r.x.max(0);
        let mut y0 = r.y.max(0);
        let mut x1 = r.right().min(self.w);
        let mut y1 = r.bottom().min(self.h);
        if let Some(c) = self.clip {
            x0 = x0.max(c.x);
            y0 = y0.max(c.y);
            x1 = x1.min(c.right());
            y1 = y1.min(c.bottom());
        }
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    fn with_clip(&mut self, rect: Rect, f: impl FnOnce(&mut Self)) {
        let saved = self.clip;
        self.clip = Some(self.effective_clip(rect));
        f(self);
        self.clip = saved;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn blend_px(&mut self, x: i32, y: i32, rgba: [u8; 4], cov: u8) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || cov == 0 || rgba[3] == 0 {
            return;
        }
        if let Some(c) = self.clip
            && !c.contains_f64(f64::from(x), f64::from(y))
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
        let eff = self.effective_clip(rect);
        for py in eff.y..eff.bottom() {
            for px in eff.x..eff.right() {
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
        let eff = self.effective_clip(rect);
        if eff.w <= 0 || eff.h <= 0 || color[3] == 0 {
            return;
        }
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        let rw = rect.w as f32;
        let rh = rect.h as f32;
        let rad = radius as f32;
        for py in eff.y..eff.bottom() {
            for px in eff.x..eff.right() {
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
    fn stroke_rounded_rect(&mut self, rect: Rect, radius: i32, color: [u8; 4]) {
        let eff = self.effective_clip(rect);
        if eff.w <= 2 || eff.h <= 2 || color[3] == 0 {
            return;
        }
        let cx = rect.x as f32 + rect.w as f32 * 0.5;
        let cy = rect.y as f32 + rect.h as f32 * 0.5;
        let rw = rect.w as f32;
        let rh = rect.h as f32;
        let rad = radius as f32;
        for py in eff.y..eff.bottom() {
            for px in eff.x..eff.right() {
                let d_out = sd_rounded_box(px as f32 + 0.5 - cx, py as f32 + 0.5 - cy, rw, rh, rad);
                if d_out > 0.7 || d_out < -1.7 {
                    continue;
                }
                let band = (1.0 - (d_out + 0.45).abs()).clamp(0.0, 1.0);
                let cov = (band * 255.0).round() as u8;
                self.blend_px(px, py, color, cov);
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        let bx0 = (cx - radius - 1.0).floor() as i32;
        let by0 = (cy - radius - 1.0).floor() as i32;
        let bx1 = (cx + radius + 2.0).ceil() as i32;
        let by1 = (cy + radius + 2.0).ceil() as i32;
        let eff = self.effective_clip(Rect::new(bx0, by0, bx1 - bx0, by1 - by0));
        for py in eff.y..eff.bottom() {
            for px in eff.x..eff.right() {
                let cov = aa_cov8(px, py, |x, y| (x - cx).hypot(y - cy) - radius);
                if cov > 0 {
                    self.blend_px(px, py, color, cov);
                }
            }
        }
    }

    fn draw_text(&mut self, text: &str, x: i32, y: i32, size_px: f32, bold: bool, color: [u8; 4]) {
        let mut cursor_x = x;
        #[allow(clippy::cast_possible_truncation)]
        let baseline = y + (size_px * 0.92).round() as i32;
        for ch in text.chars() {
            if let Some(advance) = with_cached_glyph(ch, size_px, bold, |glyph| {
                let gx0 = cursor_x + glyph.xmin;
                let gy0 = baseline - glyph.ymin - i32::try_from(glyph.height).unwrap_or(0);
                for row in 0..glyph.height {
                    for col in 0..glyph.width {
                        let cov = glyph.bitmap[row * glyph.width + col];
                        if cov > 0 {
                            let dx = gx0 + i32::try_from(col).unwrap_or(0);
                            let dy = gy0 + i32::try_from(row).unwrap_or(0);
                            self.blend_px(dx, dy, color, cov);
                        }
                    }
                }
                glyph.advance
            }) {
                cursor_x += advance;
            }
        }
    }

    fn measure_text(text: &str, size_px: f32, bold: bool) -> i32 {
        let mut total = 0;
        for ch in text.chars() {
            if let Some(adv) = with_cached_glyph(ch, size_px, bold, |g| g.advance) {
                total += adv;
            }
        }
        total
    }

    fn draw_text_centered(
        &mut self,
        rect: Rect,
        text: &str,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
    ) {
        let tw = Self::measure_text(text, size_px, bold);
        #[allow(clippy::cast_possible_truncation)]
        let th = (size_px * 1.15).round() as i32;
        let tx = rect.x + (rect.w - tw) / 2;
        let ty = rect.y + (rect.h - th) / 2;
        self.with_clip(rect, |c| {
            c.draw_text(text, tx, ty, size_px, bold, color);
        });
    }
}

fn truncate_str(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw.to_owned();
    }
    let keep = max_chars.saturating_sub(3);
    let mut out: String = raw.chars().take(keep).collect();
    out.push_str("...");
    out
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn draw_updater_emblem(canvas: &mut Canvas<'_>, rect: Rect, color: [u8; 4]) {
    let cx = rect.x as f32 + rect.w as f32 * 0.5;
    let cy = rect.y as f32 + rect.h as f32 * 0.5;
    let scale = (rect.w.min(rect.h) as f32) * 0.5;
    let eff = canvas.effective_clip(rect);
    for py in eff.y..eff.bottom() {
        for px in eff.x..eff.right() {
            let cov = aa_cov8(px, py, |x, y| {
                let ux = (x - cx) / scale;
                let uy = (y - cy) / scale;
                let ring = (ux.hypot(uy) - 0.62).abs() - 0.095;
                let stem = sd_segment(ux, uy, 0.0, -0.34, 0.0, 0.30, 0.095);
                let wing_l = sd_segment(ux, uy, -0.26, 0.06, 0.0, 0.33, 0.095);
                let wing_r = sd_segment(ux, uy, 0.26, 0.06, 0.0, 0.33, 0.095);
                ring.min(stem).min(wing_l).min(wing_r) * scale
            });
            if cov > 0 {
                canvas.blend_px(px, py, color, cov);
            }
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn draw_refresh_icon(canvas: &mut Canvas<'_>, rect: Rect, color: [u8; 4]) {
    let cx = rect.x as f32 + rect.w as f32 * 0.5;
    let cy = rect.y as f32 + rect.h as f32 * 0.5;
    let eff = canvas.effective_clip(rect);
    for py in eff.y..eff.bottom() {
        for px in eff.x..eff.right() {
            let cov = aa_cov8(px, py, |x, y| {
                let ux = (x - cx) / 7.5;
                let uy = (y - cy) / 7.5;
                let ring = (ux.hypot(uy) - 0.68).abs() - 0.12;
                let gap = ux > 0.15 && uy < -0.10 && uy > -0.65;
                let arc = if gap { 1.0 } else { ring };
                let head_a = sd_segment(ux, uy, 0.22, -0.82, 0.68, -0.45, 0.11);
                let head_b = sd_segment(ux, uy, 0.68, -0.45, 0.26, -0.18, 0.11);
                arc.min(head_a).min(head_b) * 7.5
            });
            if cov > 0 {
                canvas.blend_px(px, py, color, cov);
            }
        }
    }
}

fn draw_traffic_lights(canvas: &mut Canvas<'_>, hover: Option<&UpdaterHit>) {
    let lights = [
        (21.0_f32, 20.0_f32, [0xEE, 0x5A, 0x6A, 0xFF]),
        (43.0, 20.0, [0xF4, 0xB6, 0x3F, 0xFF]),
        (65.0, 20.0, [0x3C, 0xC6, 0x8A, 0xFF]),
    ];
    for (cx, cy, fill) in lights {
        canvas.fill_circle(cx, cy, 7.0, fill);
    }
    let show_glyphs = matches!(
        hover,
        Some(UpdaterHit::TrafficClose | UpdaterHit::TrafficMinimize | UpdaterHit::TrafficMaximize)
    );
    if show_glyphs {
        canvas.draw_text("x", 18, 13, 10.0, true, [0, 0, 0, 150]);
        canvas.draw_text("-", 40, 13, 11.0, true, [0, 0, 0, 150]);
        canvas.draw_text("+", 62, 13, 10.5, true, [0, 0, 0, 150]);
    }
}

fn render_header_and_hero(
    canvas: &mut Canvas<'_>,
    app: &UpdaterAppState,
    layout: &UpdaterLayout,
    pal: &Palette,
    hover: Option<&UpdaterHit>,
) {
    let title = "Software Update";
    let tw = Canvas::measure_text(title, 13.0, true);
    canvas.draw_text(
        title,
        (layout.win_w - tw) / 2,
        12,
        13.0,
        true,
        pal.text_primary,
    );
    draw_traffic_lights(canvas, hover);

    let ref_bg = if hover == Some(&UpdaterHit::CheckNow) {
        pal.pill_hover
    } else {
        pal.pill_bg
    };
    canvas.fill_rounded_rect(layout.refresh_btn, 14, ref_bg);
    draw_refresh_icon(canvas, layout.refresh_btn, pal.text_primary);

    canvas.fill_rounded_rect(layout.hero_card, 12, pal.card_bg);

    let badge_bg = if app.tx_phase == UpdateTxPhase::Complete && !app.tx_restart_recommended {
        pal.success
    } else if app.tx_phase == UpdateTxPhase::Failed {
        pal.danger
    } else {
        pal.accent
    };
    canvas.fill_rounded_rect(layout.emblem_rect, 12, badge_bg);
    draw_updater_emblem(canvas, layout.emblem_rect, [255, 255, 255, 255]);

    let headline = hero_headline(app);
    let tx = layout.emblem_rect.right() + 16;
    let max_text_w = layout.primary_btn.x - tx - 12;
    canvas.with_clip(
        Rect::new(tx, layout.hero_card.y + 12, max_text_w.max(60), 68),
        |c| {
            c.draw_text(
                &headline,
                tx,
                layout.hero_card.y + 22,
                16.0,
                true,
                pal.text_primary,
            );
            let sub = hero_subtitle(app);
            c.draw_text(
                &sub,
                tx,
                layout.hero_card.y + 47,
                11.5,
                false,
                pal.text_secondary,
            );
        },
    );

    let btn_label = if app.is_upgrading() {
        "Cancel"
    } else if app.snapshot.total_count > 0 {
        "Update Now"
    } else {
        "Check Again"
    };
    let btn_bg = if app.is_upgrading() {
        pal.pill_hover
    } else if hover == Some(&UpdaterHit::PrimaryAction) {
        pal.accent_hover
    } else {
        pal.accent
    };
    let btn_text_color = if app.is_upgrading() {
        pal.text_primary
    } else {
        [255, 255, 255, 255]
    };
    canvas.fill_rounded_rect(layout.primary_btn, 10, btn_bg);
    canvas.draw_text_centered(layout.primary_btn, btn_label, 12.5, true, btn_text_color);
}

fn hero_headline(app: &UpdaterAppState) -> String {
    if app.is_upgrading() {
        return app.tx_phase.label().to_owned();
    }
    if app.is_checking() {
        return "Checking for Updates...".to_owned();
    }
    if app.tx_phase == UpdateTxPhase::Complete {
        return if app.tx_restart_recommended {
            "Updates Installed - Restart Recommended".to_owned()
        } else {
            "System Updates Installed".to_owned()
        };
    }
    match app.snapshot.state {
        UpdateCheckState::Available => {
            if app.snapshot.total_count == 1 {
                "1 Update Available".to_owned()
            } else {
                format!("{} Updates Available", app.snapshot.total_count)
            }
        }
        UpdateCheckState::UpToDate => "Your System is Up to Date".to_owned(),
        UpdateCheckState::Unavailable => "Update Check Unavailable".to_owned(),
        UpdateCheckState::Checking => "Checking for Updates...".to_owned(),
        UpdateCheckState::Idle => "Software Update".to_owned(),
    }
}

fn hero_subtitle(app: &UpdaterAppState) -> String {
    let mut parts = vec![app.summary.distro_name.clone()];
    if !app.summary.kernel_release.is_empty() {
        parts.push(format!("Kernel {}", app.summary.kernel_release));
    }
    if let Some(ver) = &app.summary.rustyos_installed_version {
        if let Some(candidate) = app
            .snapshot
            .items
            .iter()
            .find(|it| it.is_rustyos)
            .map(|it| &it.new_version)
        {
            parts.push(format!("RustyOS {ver} -> {candidate}"));
        } else {
            parts.push(format!("RustyOS {ver}"));
        }
    }
    if app.dry_run {
        parts.push("Dry-run".to_owned());
    }
    truncate_str(&parts.join("  -  "), 62)
}

fn render_progress_and_tabs(
    canvas: &mut Canvas<'_>,
    app: &UpdaterAppState,
    layout: &UpdaterLayout,
    pal: &Palette,
    hover: Option<&UpdaterHit>,
) {
    if let (Some(pcard), Some(ptrack)) = (layout.progress_card, layout.progress_track) {
        canvas.fill_rounded_rect(pcard, 12, pal.card_bg);
        let status = app
            .tx_summary
            .as_deref()
            .unwrap_or_else(|| app.tx_phase.label());
        canvas.draw_text(
            &truncate_str(status, 56),
            ptrack.x,
            pcard.y + 8,
            11.5,
            true,
            pal.text_primary,
        );
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pct_val = (app.tx_progress.clamp(0.0, 1.0) * 100.0).round() as u32;
        let pct_str = format!("{pct_val}%");
        let pw = Canvas::measure_text(&pct_str, 11.0, false);
        canvas.draw_text(
            &pct_str,
            ptrack.right() - pw,
            pcard.y + 8,
            11.0,
            false,
            pal.text_secondary,
        );
        canvas.fill_rounded_rect(ptrack, 3, pal.track_bg);
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let fill_w = ((ptrack.w as f32) * app.tx_progress.clamp(0.0, 1.0)).round() as i32;
        if fill_w > 0 {
            let fill_color = match app.tx_phase {
                UpdateTxPhase::Failed => pal.danger,
                UpdateTxPhase::Complete => pal.success,
                _ => pal.accent,
            };
            canvas.fill_rounded_rect(
                Rect::new(ptrack.x, ptrack.y, fill_w.max(6).min(ptrack.w), ptrack.h),
                3,
                fill_color,
            );
        }
    }

    for &(filter, rect) in &layout.filter_tabs {
        let active = app.filter == filter && !app.show_logs;
        let hovered = hover == Some(&UpdaterHit::FilterTab(filter));
        let (bg, fg) = if active {
            (pal.accent, [255, 255, 255, 255])
        } else if hovered {
            (pal.pill_hover, pal.text_primary)
        } else {
            (pal.pill_bg, pal.text_secondary)
        };
        canvas.fill_rounded_rect(rect, 10, bg);
        let label = format!("{} ({})", filter.label(), app.filter_count(filter));
        canvas.draw_text_centered(rect, &label, 11.5, active, fg);
    }

    let log_active = app.show_logs;
    let log_bg = if log_active {
        pal.accent_soft
    } else if hover == Some(&UpdaterHit::ToggleLogs) {
        pal.pill_hover
    } else {
        pal.pill_bg
    };
    let log_text_color = if log_active {
        pal.accent
    } else {
        pal.text_secondary
    };
    canvas.fill_rounded_rect(layout.logs_toggle_btn, 10, log_bg);
    let log_label = if log_active { "Packages" } else { "View Log" };
    canvas.draw_text_centered(
        layout.logs_toggle_btn,
        log_label,
        11.5,
        log_active,
        log_text_color,
    );
}

#[allow(clippy::cast_precision_loss)]
fn render_package_row(
    canvas: &mut Canvas<'_>,
    idx: usize,
    rect: Rect,
    item: &PackageUpdateItem,
    selected: bool,
    pal: &Palette,
) {
    if selected {
        canvas.fill_rounded_rect(rect, 8, pal.row_selected);
    } else if idx % 2 == 1 {
        canvas.fill_rounded_rect(rect, 6, pal.row_alt);
    }
    if idx > 0 && !selected {
        canvas.fill_rect(
            Rect::new(rect.x + 12, rect.y - 1, rect.w - 24, 1),
            pal.divider,
        );
    }
    if item.is_rustyos || item.requires_restart {
        let dot_color = if item.is_rustyos {
            pal.accent
        } else {
            pal.warning
        };
        canvas.fill_circle(
            rect.x as f32 + 14.0,
            rect.y as f32 + rect.h as f32 * 0.5,
            3.5,
            dot_color,
        );
    }
    let name_x = rect.x
        + if item.is_rustyos || item.requires_restart {
            26
        } else {
            14
        };
    let name_color = if item.is_rustyos {
        pal.accent
    } else {
        pal.text_primary
    };
    let name_str = truncate_str(&item.name, 28);
    canvas.draw_text(&name_str, name_x, rect.y + 10, 12.5, true, name_color);

    if item.is_rustyos {
        let nw = Canvas::measure_text(&name_str, 12.5, true);
        let badge = Rect::new(name_x + nw + 8, rect.y + 8, 56, 19);
        canvas.fill_rounded_rect(badge, 6, pal.accent_soft);
        canvas.draw_text_centered(badge, "RustyOS", 10.0, true, pal.accent);
    }

    let ver = truncate_str(&format!("{} -> {}", item.old_version, item.new_version), 34);
    let vw = Canvas::measure_text(&ver, 11.5, false);
    canvas.draw_text(
        &ver,
        rect.right() - 14 - vw,
        rect.y + 11,
        11.5,
        false,
        pal.text_secondary,
    );
}

fn render_list_and_footer(
    canvas: &mut Canvas<'_>,
    app: &UpdaterAppState,
    layout: &UpdaterLayout,
    pal: &Palette,
    hover: Option<&UpdaterHit>,
) {
    canvas.fill_rounded_rect(layout.list_card, 12, pal.card_bg);

    let vp = layout.list_viewport;
    canvas.with_clip(vp, |c| {
        if app.show_logs {
            if app.tx_logs.is_empty() {
                c.draw_text_centered(
                    vp,
                    "No update transaction output recorded yet.",
                    12.0,
                    false,
                    pal.text_muted,
                );
            } else {
                for (i, line) in app.tx_logs.iter().enumerate() {
                    let ly = vp.y + 8 + i32::try_from(i).unwrap_or(0) * LOG_LINE_HEIGHT
                        - app.log_scroll_y;
                    if ly + LOG_LINE_HEIGHT < vp.y || ly > vp.bottom() {
                        continue;
                    }
                    c.draw_text(
                        &truncate_str(line, 86),
                        vp.x + 12,
                        ly + 4,
                        11.5,
                        false,
                        pal.text_primary,
                    );
                }
            }
        } else {
            let filtered = app.filtered_items();
            if filtered.is_empty() {
                let msg = if app.is_checking() {
                    "Checking Arch Linux & RustyOS package repositories..."
                } else if let Some(err) = &app.snapshot.error_hint {
                    err.as_str()
                } else {
                    "No pending updates matching this filter."
                };
                c.draw_text_centered(vp, &truncate_str(msg, 64), 12.5, false, pal.text_muted);
            } else {
                for &(idx, rect) in &layout.row_rects {
                    if let Some(item) = filtered.get(idx) {
                        render_package_row(c, idx, rect, item, app.selected_idx == Some(idx), pal);
                    }
                }
            }
        }
    });

    draw_overlay_scrollbar(canvas, app, layout, pal);

    let footer_text = if app.tx_restart_recommended {
        "Restart or log out of your session to apply updated RustyOS components.".to_owned()
    } else if !app.dry_run && app.summary.helper_path.is_none() {
        format!(
            "Checked {}  -  Run scripts/install.sh --system for pkexec",
            app.snapshot.last_checked_label
        )
    } else if app.dry_run {
        format!(
            "Checked {}  -  Simulation mode (--dry-run)",
            app.snapshot.last_checked_label
        )
    } else if !app.summary.pacman_repo_configured && app.summary.repo_pkgbuild_dir.is_none() {
        format!(
            "Checked {}  -  Add [rustyos] to /etc/pacman.conf for binary updates",
            app.snapshot.last_checked_label
        )
    } else {
        format!("Last checked: {}", app.snapshot.last_checked_label)
    };
    canvas.draw_text(
        &truncate_str(&footer_text, 72),
        layout.footer_rect.x + 4,
        layout.footer_rect.y + 11,
        11.0,
        false,
        pal.text_muted,
    );

    if let Some(rb) = layout.rebuild_btn {
        let rb_bg = if hover == Some(&UpdaterHit::RebuildLocalPkg) {
            pal.pill_hover
        } else {
            pal.pill_bg
        };
        canvas.fill_rounded_rect(rb, 10, rb_bg);
        canvas.draw_text_centered(rb, "Rebuild RustyOS pkg", 11.0, true, pal.text_primary);
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn draw_overlay_scrollbar(
    canvas: &mut Canvas<'_>,
    app: &UpdaterAppState,
    layout: &UpdaterLayout,
    pal: &Palette,
) {
    let vp = layout.list_viewport;
    let (content_h, scroll_y) = if app.show_logs {
        (
            i32::try_from(app.tx_logs.len()).unwrap_or(0) * LOG_LINE_HEIGHT + 12,
            app.log_scroll_y,
        )
    } else {
        (
            i32::try_from(app.filtered_items().len()).unwrap_or(0) * ROW_HEIGHT + 8,
            app.scroll_y,
        )
    };
    if content_h <= vp.h || vp.h <= 16 {
        return;
    }
    let ratio = (vp.h as f32 / content_h as f32).clamp(0.08, 1.0);
    let thumb_h = ((vp.h as f32 * ratio).round() as i32).max(24);
    let max_scroll = (content_h - vp.h).max(1);
    let track_span = (vp.h - thumb_h - 8).max(0);
    let thumb_y =
        vp.y + 4 + ((scroll_y as f32 / max_scroll as f32) * track_span as f32).round() as i32;
    canvas.fill_rounded_rect(
        Rect::new(vp.right() - 7, thumb_y, 4, thumb_h),
        2,
        pal.scrollbar,
    );
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn apply_window_corner_mask(buf: &mut [u8], w: i32, h: i32) {
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    let rad = WINDOW_CORNER_RADIUS as f32;
    let wf = w as f32;
    let hf = h as f32;
    for py in 0..h {
        let near_v = py <= WINDOW_CORNER_RADIUS + 2 || py >= h - WINDOW_CORNER_RADIUS - 3;
        if !near_v {
            continue;
        }
        for px in 0..w {
            let near_h = px <= WINDOW_CORNER_RADIUS + 2 || px >= w - WINDOW_CORNER_RADIUS - 3;
            if !near_h {
                continue;
            }
            let d = sd_rounded_box(px as f32 + 0.5 - cx, py as f32 + 0.5 - cy, wf, hf, rad);
            if d > 0.55 {
                let idx = ((py * w + px) * 4) as usize;
                if idx + 3 < buf.len() {
                    buf[idx..idx + 4].fill(0);
                }
            } else if d > -0.55 {
                let cov = (0.55 - d).clamp(0.0, 1.0);
                let idx = ((py * w + px) * 4) as usize;
                if idx + 3 < buf.len() {
                    for ch in &mut buf[idx..idx + 4] {
                        *ch = (f32::from(*ch) * cov).round() as u8;
                    }
                }
            }
        }
    }
}

/// Renders the entire `rusty-updater` CSD window into premultiplied `ARGB8888` bytes (`RUSTYOS-077`).
pub(crate) fn render_window_pixels(
    buf: &mut [u8],
    width: u32,
    height: u32,
    app: &UpdaterAppState,
    layout: &UpdaterLayout,
    hover: Option<&UpdaterHit>,
) {
    let pal = palette_for(app.theme);
    let mut canvas = Canvas::new(buf, width, height);
    let win_rect = Rect::new(0, 0, layout.win_w, layout.win_h);
    canvas.fill_rect(win_rect, pal.window_bg);

    render_header_and_hero(&mut canvas, app, layout, &pal, hover);
    render_progress_and_tabs(&mut canvas, app, layout, &pal, hover);
    render_list_and_footer(&mut canvas, app, layout, &pal, hover);

    canvas.stroke_rounded_rect(win_rect, WINDOW_CORNER_RADIUS, pal.window_border);
    apply_window_corner_mask(buf, layout.win_w, layout.win_h);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::compute_layout;
    use crate::state::StartupOptions;

    #[test]
    fn renders_golden_gate_light_and_dark_themes_with_clean_corners() {
        for theme in [
            FilesThemeMode::LightGoldenGate,
            FilesThemeMode::DarkPurpleGlass,
        ] {
            let app = UpdaterAppState::new(&StartupOptions {
                theme_override: Some(theme),
                dry_run: true,
                check_immediately: false,
                pkgbuild_dir: None,
            });
            let w = 740_u32;
            let h = 560_u32;
            let layout = compute_layout(&app, w, h);
            let mut buf = vec![0_u8; (w * h * 4) as usize];
            render_window_pixels(&mut buf, w, h, &app, &layout, None);

            // Outer (0, 0) pixel must stay transparent due to WINDOW_CORNER_RADIUS mask.
            assert_eq!(buf[3], 0);
            // Center pixel must match opaque card/window tone depending on theme.
            let mid_idx = (((h / 2) * w + (w / 2)) * 4) as usize;
            assert_eq!(buf[mid_idx + 3], 255);
            if theme == FilesThemeMode::LightGoldenGate {
                assert!(buf[mid_idx + 2] > 200);
            } else {
                assert!(buf[mid_idx + 2] < 80);
            }
        }
    }
}
