//! Pure CPU premultiplied `ARGB8888` renderer for `rusty-installer` (`RUSTYOS-115`).
//!
//! Matches `macOS 27 Golden Gate Light & Dark` (`DESIGN.md` §5, §12) with `Inter`
//! typography, SDF rounded geometry, `Liquid Glass` alpha, and `[ui].accent_index`.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};
use rusty_services::file_manager::accent_rgba_for_index;

use crate::disks::DualBootKind;
use crate::layout::{
    CARD_CORNER_RADIUS, InstallerHit, InstallerLayout, PILL_CORNER_RADIUS, Rect, StepContentLayout,
    TextFieldRect, ThemeTileRect, WINDOW_CORNER_RADIUS, WelcomeAction, WifiRowRect,
};
use crate::state::{
    BootloaderKind, DiskInstallMode, InstallPhase, InstallerAppState, KEYBOARD_PRESETS,
    LANGUAGE_OPTIONS, TextFieldId, ThemeChoice, WizardMode,
};

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
    pub dark: bool,
    pub window_bg: [u8; 4],
    pub window_border: [u8; 4],
    pub rim_top: [u8; 4],
    pub card_bg: [u8; 4],
    pub card_hover: [u8; 4],
    pub card_border: [u8; 4],
    pub field_bg: [u8; 4],
    pub pill_bg: [u8; 4],
    pub pill_hover: [u8; 4],
    pub divider: [u8; 4],
    pub accent: [u8; 4],
    pub accent_soft: [u8; 4],
    pub text_primary: [u8; 4],
    pub text_secondary: [u8; 4],
    pub text_muted: [u8; 4],
    pub danger: [u8; 4],
    pub ok_green: [u8; 4],
}

#[must_use]
pub(crate) fn palette_for(dark: bool, accent_idx: usize, has_glass: bool) -> Palette {
    let acc = accent_rgba_for_index(accent_idx, dark);
    let accent_soft = [acc[0], acc[1], acc[2], if dark { 46 } else { 30 }];
    let bg_alpha = if has_glass {
        if dark { 222 } else { 232 }
    } else {
        255
    };
    if dark {
        Palette {
            dark: true,
            window_bg: [30, 31, 34, bg_alpha],
            window_border: [255, 255, 255, 34],
            rim_top: [255, 255, 255, 42],
            card_bg: [42, 43, 48, 224],
            card_hover: [52, 54, 60, 238],
            card_border: [255, 255, 255, 26],
            field_bg: [24, 25, 28, 240],
            pill_bg: [58, 60, 66, 236],
            pill_hover: [72, 75, 82, 245],
            divider: [255, 255, 255, 22],
            accent: [acc[0], acc[1], acc[2], 255],
            accent_soft,
            text_primary: [244, 245, 248, 255],
            text_secondary: [182, 185, 194, 255],
            text_muted: [130, 134, 144, 255],
            danger: [255, 82, 82, 255],
            ok_green: [48, 209, 88, 255],
        }
    } else {
        Palette {
            dark: false,
            window_bg: [246, 247, 250, bg_alpha],
            window_border: [0, 0, 0, 28],
            rim_top: [255, 255, 255, 210],
            card_bg: [255, 255, 255, 236],
            card_hover: [248, 250, 255, 245],
            card_border: [0, 0, 0, 20],
            field_bg: [255, 255, 255, 255],
            pill_bg: [228, 231, 238, 235],
            pill_hover: [216, 220, 228, 245],
            divider: [0, 0, 0, 18],
            accent: [acc[0], acc[1], acc[2], 255],
            accent_soft,
            text_primary: [24, 26, 30, 255],
            text_secondary: [88, 92, 102, 255],
            text_muted: [132, 136, 148, 255],
            danger: [215, 42, 42, 255],
            ok_green: [40, 184, 72, 255],
        }
    }
}

struct Canvas<'a> {
    buf: &'a mut [u8],
    w: i32,
    h: i32,
}

impl<'a> Canvas<'a> {
    fn new(buf: &'a mut [u8], w: u32, h: u32) -> Self {
        Self {
            buf,
            w: i32::try_from(w).unwrap_or(1),
            h: i32::try_from(h).unwrap_or(1),
        }
    }

    fn clear(&mut self) {
        self.buf.fill(0);
    }

    #[inline]
    #[allow(clippy::cast_sign_loss)]
    fn blend_px(&mut self, x: i32, y: i32, color: [u8; 4], cov: u8) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || cov == 0 {
            return;
        }
        let eff_a = (u32::from(color[3]) * u32::from(cov) + 127) / 255;
        if eff_a == 0 {
            return;
        }
        let sr = (u32::from(color[0]) * eff_a + 127) / 255;
        let sg = (u32::from(color[1]) * eff_a + 127) / 255;
        let sb = (u32::from(color[2]) * eff_a + 127) / 255;
        let inv = 255 - eff_a;
        let idx = ((y * self.w + x) * 4) as usize;
        if idx + 3 >= self.buf.len() {
            return;
        }
        if eff_a == 255 {
            self.buf[idx] = sb as u8;
            self.buf[idx + 1] = sg as u8;
            self.buf[idx + 2] = sr as u8;
            self.buf[idx + 3] = 255;
        } else {
            self.buf[idx] = (sb + (u32::from(self.buf[idx]) * inv + 127) / 255).min(255) as u8;
            self.buf[idx + 1] =
                (sg + (u32::from(self.buf[idx + 1]) * inv + 127) / 255).min(255) as u8;
            self.buf[idx + 2] =
                (sr + (u32::from(self.buf[idx + 2]) * inv + 127) / 255).min(255) as u8;
            self.buf[idx + 3] =
                (eff_a + (u32::from(self.buf[idx + 3]) * inv + 127) / 255).min(255) as u8;
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn fill_rounded_rect(&mut self, r: Rect, radius: i32, color: [u8; 4]) {
        if r.w <= 0 || r.h <= 0 || color[3] == 0 {
            return;
        }
        let rad = radius.max(0).min(r.w / 2).min(r.h / 2) as f32;
        let x0 = r.x.max(0);
        let y0 = r.y.max(0);
        let x1 = r.right().min(self.w);
        let y1 = r.bottom().min(self.h);
        let hx = r.w as f32 * 0.5;
        let hy = r.h as f32 * 0.5;
        let cx = r.x as f32 + hx;
        let cy = r.y as f32 + hy;

        for py in y0..y1 {
            let dy = (py as f32 + 0.5 - cy).abs() - (hy - rad);
            for px in x0..x1 {
                let dx = (px as f32 + 0.5 - cx).abs() - (hx - rad);
                let dist = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - rad;
                let cov = ((0.5 - dist).clamp(0.0, 1.0) * 255.0).round() as u8;
                if cov > 0 {
                    self.blend_px(px, py, color, cov);
                }
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn stroke_rounded_rect(&mut self, r: Rect, radius: i32, thickness: f32, color: [u8; 4]) {
        if r.w <= 0 || r.h <= 0 || color[3] == 0 {
            return;
        }
        let rad = radius.max(0).min(r.w / 2).min(r.h / 2) as f32;
        let x0 = r.x.max(0);
        let y0 = r.y.max(0);
        let x1 = r.right().min(self.w);
        let y1 = r.bottom().min(self.h);
        let hx = r.w as f32 * 0.5;
        let hy = r.h as f32 * 0.5;
        let cx = r.x as f32 + hx;
        let cy = r.y as f32 + hy;
        let half_t = thickness * 0.5;

        for py in y0..y1 {
            let dy = (py as f32 + 0.5 - cy).abs() - (hy - rad);
            for px in x0..x1 {
                let dx = (px as f32 + 0.5 - cx).abs() - (hx - rad);
                let dist = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - rad;
                let edge_dist = (dist + half_t).abs() - half_t;
                let cov = ((0.5 - edge_dist).clamp(0.0, 1.0) * 255.0).round() as u8;
                if cov > 0 {
                    self.blend_px(px, py, color, cov);
                }
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        let x0 = (cx - radius - 1.0).floor().max(0.0) as i32;
        let y0 = (cy - radius - 1.0).floor().max(0.0) as i32;
        let x1 = (cx + radius + 1.0).ceil().min(self.w as f32) as i32;
        let y1 = (cy + radius + 1.0).ceil().min(self.h as f32) as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                let d = (px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy) - radius;
                let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0).round() as u8;
                if cov > 0 {
                    self.blend_px(px, py, color, cov);
                }
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn draw_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thickness: f32, color: [u8; 4]) {
        let pad = thickness * 0.5 + 1.5;
        let min_x = x0.min(x1).sub_pad(pad).max(0.0) as i32;
        let max_x = (x0.max(x1) + pad).ceil().min(self.w as f32) as i32;
        let min_y = y0.min(y1).sub_pad(pad).max(0.0) as i32;
        let max_y = (y0.max(y1) + pad).ceil().min(self.h as f32) as i32;
        let vx = x1 - x0;
        let vy = y1 - y0;
        let len_sq = vx * vx + vy * vy;
        let r = thickness * 0.5;
        for py in min_y..max_y {
            for px in min_x..max_x {
                let wx = px as f32 + 0.5 - x0;
                let wy = py as f32 + 0.5 - y0;
                let t = if len_sq > 1e-6 {
                    ((wx * vx + wy * vy) / len_sq).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let d = (wx - vx * t).hypot(wy - vy * t) - r;
                let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0).round() as u8;
                if cov > 0 {
                    self.blend_px(px, py, color, cov);
                }
            }
        }
    }

    fn text_width(&self, text: &str, size_px: f32, bold: bool) -> i32 {
        let _ = self.w;
        text.chars()
            .map(|ch| with_cached_glyph(ch, size_px, bold, |g| g.advance).unwrap_or(6))
            .sum()
    }

    fn draw_text(
        &mut self,
        x: i32,
        y_baseline: i32,
        text: &str,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
    ) {
        self.draw_text_clipped(x, y_baseline, text, size_px, bold, color, self.w);
    }

    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap
    )]
    fn draw_text_clipped(
        &mut self,
        x: i32,
        y_baseline: i32,
        text: &str,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
        max_x: i32,
    ) {
        let mut pen_x = x;
        for ch in text.chars() {
            let adv = with_cached_glyph(ch, size_px, bold, |g| {
                let gx0 = pen_x + g.xmin;
                let gy0 = y_baseline - g.ymin - (g.height as i32);
                for row in 0..g.height {
                    let py = gy0 + row as i32;
                    for col in 0..g.width {
                        let px = gx0 + col as i32;
                        if px >= max_x {
                            continue;
                        }
                        let cov = g.bitmap[row * g.width + col];
                        if cov > 0 {
                            self.blend_px(px, py, color, cov);
                        }
                    }
                }
                g.advance
            })
            .unwrap_or(6);
            pen_x += adv;
            if pen_x >= max_x {
                break;
            }
        }
    }

    fn draw_text_centered(
        &mut self,
        r: Rect,
        text: &str,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
    ) {
        let tw = self.text_width(text, size_px, bold);
        #[allow(clippy::cast_possible_truncation)]
        let baseline = r.y + i32::midpoint(r.h, (size_px * 0.72).round() as i32);
        self.draw_text(r.x + (r.w - tw) / 2, baseline, text, size_px, bold, color);
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn mask_window_corners(&mut self, radius: i32) {
        let rad = radius as f32;
        let hx = self.w as f32 * 0.5;
        let hy = self.h as f32 * 0.5;
        let cx = hx;
        let cy = hy;
        for py in 0..self.h {
            let dy = (py as f32 + 0.5 - cy).abs() - (hy - rad);
            if dy <= 0.0 {
                continue;
            }
            for px in 0..self.w {
                let dx = (px as f32 + 0.5 - cx).abs() - (hx - rad);
                if dx <= 0.0 {
                    continue;
                }
                let dist = dx.hypot(dy) - rad;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov < 1.0 {
                    let idx = ((py * self.w + px) * 4) as usize;
                    if idx + 3 < self.buf.len() {
                        for c in 0..4 {
                            self.buf[idx + c] = (f32::from(self.buf[idx + c]) * cov).round() as u8;
                        }
                    }
                }
            }
        }
    }
}

trait SubPad {
    fn sub_pad(self, p: f32) -> f32;
}
impl SubPad for f32 {
    fn sub_pad(self, p: f32) -> f32 {
        (self - p).floor()
    }
}

pub(crate) fn render_window_pixels(
    buf: &mut [u8],
    width: u32,
    height: u32,
    app: &InstallerAppState,
    layout: &InstallerLayout,
    hover: Option<InstallerHit>,
    has_glass: bool,
) {
    let pal = palette_for(app.resolved_dark, app.accent_index, has_glass);
    let mut c = Canvas::new(buf, width, height);
    c.clear();

    let full = Rect::new(0, 0, layout.win_w, layout.win_h);
    c.fill_rounded_rect(full, WINDOW_CORNER_RADIUS, pal.window_bg);
    c.fill_rounded_rect(
        Rect::new(
            WINDOW_CORNER_RADIUS,
            1,
            layout.win_w - WINDOW_CORNER_RADIUS * 2,
            1,
        ),
        0,
        pal.rim_top,
    );
    render_header(&mut c, app, layout, hover, &pal);
    render_content(&mut c, app, layout, hover, &pal);
    render_footer(&mut c, app, layout, hover, &pal);

    c.mask_window_corners(WINDOW_CORNER_RADIUS);
    c.stroke_rounded_rect(full, WINDOW_CORNER_RADIUS, 1.0, pal.window_border);
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
fn render_header(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    layout: &InstallerLayout,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    let tl_hovered = matches!(
        hover,
        Some(
            InstallerHit::TrafficClose
                | InstallerHit::TrafficMinimize
                | InstallerHit::TrafficMaximize
        )
    );
    let lights = [
        (&layout.traffic_close, [0xEE, 0x5A, 0x6A, 255], 'x'),
        (&layout.traffic_min, [0xF4, 0xB6, 0x3F, 255], '-'),
        (&layout.traffic_max, [0x3C, 0xC6, 0x8A, 255], '+'),
    ];
    for (rect, col, glyph) in lights {
        let cx = rect.x as f32 + 7.0;
        let cy = rect.y as f32 + 7.0;
        c.fill_circle(cx, cy, 7.0, col);
        if tl_hovered {
            draw_traffic_glyph(c, cx, cy, glyph);
        }
    }

    let title = match app.mode {
        crate::state::WizardMode::Install => app.install_step.title(),
        crate::state::WizardMode::Oobe => app.oobe_step.title(),
    };
    c.draw_text_centered(
        Rect::new(0, 4, layout.win_w, 24),
        title,
        13.5,
        true,
        pal.text_primary,
    );

    let total = i32::try_from(layout.step_count).unwrap_or(1);
    let dots_w = (total - 1) * 14;
    let x0 = layout.step_dots_center_x - dots_w / 2;
    for i in 0..layout.step_count {
        let color = match i.cmp(&layout.active_step_idx) {
            std::cmp::Ordering::Equal => pal.accent,
            std::cmp::Ordering::Less => [pal.accent[0], pal.accent[1], pal.accent[2], 140],
            std::cmp::Ordering::Greater => pal.pill_bg,
        };
        let r = if i == layout.active_step_idx {
            3.5
        } else {
            2.6
        };
        c.fill_circle(
            (x0 + i32::try_from(i).unwrap_or(0) * 14) as f32,
            35.0,
            r,
            color,
        );
    }

    if app.dry_run {
        let badge = Rect::new(layout.win_w - 118, 10, 102, 22);
        c.fill_rounded_rect(badge, 11, pal.accent_soft);
        c.draw_text_centered(badge, "Simulation", 11.5, true, pal.accent);
    }

    c.fill_rounded_rect(
        Rect::new(0, layout.header_rect.bottom(), layout.win_w, 1),
        0,
        pal.divider,
    );
}

fn draw_traffic_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, kind: char) {
    let ink = [0, 0, 0, 150];
    match kind {
        'x' => {
            c.draw_line(cx - 2.6, cy - 2.6, cx + 2.6, cy + 2.6, 0.7, ink);
            c.draw_line(cx + 2.6, cy - 2.6, cx - 2.6, cy + 2.6, 0.7, ink);
        }
        '-' => c.draw_line(cx - 3.2, cy, cx + 3.2, cy, 0.75, ink),
        '+' => {
            c.draw_line(cx - 3.0, cy, cx + 3.0, cy, 0.75, ink);
            c.draw_line(cx, cy - 3.0, cx, cy + 3.0, 0.75, ink);
        }
        _ => {}
    }
}

fn render_footer(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    layout: &InstallerLayout,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    let f = layout.footer_rect;
    c.fill_rounded_rect(Rect::new(0, f.y, f.w, 1), 0, pal.divider);

    if let Some(back) = layout.back_btn {
        let bg = if hover == Some(InstallerHit::BackButton) {
            pal.pill_hover
        } else {
            pal.pill_bg
        };
        c.fill_rounded_rect(back, PILL_CORNER_RADIUS, bg);
        c.draw_text_centered(back, layout.back_label, 13.5, true, pal.text_primary);
    }

    if let Some(err) = app.form_error.as_deref().or(app.install_error.as_deref()) {
        c.draw_text_clipped(
            150,
            f.y + 38,
            err,
            12.5,
            false,
            pal.danger,
            layout.primary_btn.x - 16,
        );
    }

    let p = layout.primary_btn;
    let p_color = if app.phase == InstallPhase::Running {
        [pal.accent[0], pal.accent[1], pal.accent[2], 150]
    } else if hover == Some(InstallerHit::PrimaryButton) {
        [
            pal.accent[0].saturating_add(16),
            pal.accent[1].saturating_add(16),
            pal.accent[2].saturating_add(16),
            255,
        ]
    } else {
        pal.accent
    };
    c.fill_rounded_rect(p, PILL_CORNER_RADIUS, p_color);
    c.draw_text_centered(p, layout.primary_label, 13.5, true, [255, 255, 255, 255]);
}

fn render_content(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    layout: &InstallerLayout,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    match &layout.content {
        StepContentLayout::Welcome {
            hero_emblem,
            cards,
            status_rect,
        } => render_welcome(c, app, *hero_emblem, cards, *status_rect, hover, pal),
        StepContentLayout::SelectDisk {
            mode_whole,
            mode_dualboot,
            cards,
            refresh_btn,
            boot_systemd,
            boot_grub,
        } => render_select_disk(
            c,
            app,
            layout.content_rect,
            [*mode_whole, *mode_dualboot],
            cards,
            *refresh_btn,
            [*boot_systemd, *boot_grub],
            hover,
            pal,
        ),
        StepContentLayout::AccountAndStyle {
            fields,
            kb_chips,
            theme_chips,
            swatches,
        } => render_account_and_style(
            c,
            app,
            layout.content_rect,
            fields,
            kb_chips,
            theme_chips,
            swatches,
            pal,
        ),
        StepContentLayout::Installing {
            emblem_rect,
            progress_rect,
            toggle_logs_btn,
            log_box,
            summary_lines,
        } => render_installing(
            c,
            app,
            layout.content_rect,
            *emblem_rect,
            *progress_rect,
            *toggle_logs_btn,
            *log_box,
            summary_lines,
            pal,
        ),
        other => render_content_oobe_and_net(c, app, layout.content_rect, other, hover, pal),
    }
}

fn render_content_oobe_and_net(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    content_rect: Rect,
    content: &StepContentLayout,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    match content {
        StepContentLayout::Hello { hero_emblem, rows } => {
            render_oobe_hello(c, content_rect, *hero_emblem, rows, hover, pal);
        }
        StepContentLayout::Keyboard { rows } => {
            render_oobe_keyboard(c, content_rect, rows, hover, pal);
        }
        StepContentLayout::Network {
            card_rect,
            wifi_toggle_btn,
            refresh_btn,
            wifi_rows,
            password_field,
            connect_btn,
        } => render_network_step(
            c,
            app,
            content_rect,
            *card_rect,
            *wifi_toggle_btn,
            *refresh_btn,
            wifi_rows,
            password_field.as_ref(),
            *connect_btn,
            hover,
            pal,
        ),
        StepContentLayout::OobeAccount {
            fields,
            autologin_row,
        } => render_oobe_account(c, app, content_rect, fields, *autologin_row, pal),
        StepContentLayout::Appearance {
            tiles,
            accent_card,
            swatches,
        } => render_oobe_appearance(c, content_rect, tiles, *accent_card, swatches, pal),
        StepContentLayout::Finish {
            hero_emblem,
            summary_card,
        } => render_oobe_finish(c, app, content_rect, *hero_emblem, *summary_card, pal),
        StepContentLayout::Welcome { .. }
        | StepContentLayout::SelectDisk { .. }
        | StepContentLayout::AccountAndStyle { .. }
        | StepContentLayout::Installing { .. } => {}
    }
}

fn render_oobe_hello(
    c: &mut Canvas<'_>,
    area: Rect,
    hero_emblem: Rect,
    rows: &[crate::layout::OptionRowRect],
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    draw_rustyos_emblem(c, hero_emblem, pal.accent, pal.dark);
    c.draw_text_centered(
        Rect::new(area.x, hero_emblem.bottom() + 10, area.w, 34),
        "Hello  •  Привет  •  Вітаємо",
        24.0,
        true,
        pal.text_primary,
    );
    c.draw_text_centered(
        Rect::new(area.x, hero_emblem.bottom() + 44, area.w, 22),
        "Select your primary language and region",
        13.5,
        false,
        pal.text_secondary,
    );
    for row in rows {
        let hov = hover == Some(InstallerHit::SelectLanguage(row.idx));
        draw_option_row(
            c,
            row.rect,
            &row.title,
            &row.subtitle,
            row.selected,
            hov,
            pal,
        );
    }
}

fn render_oobe_keyboard(
    c: &mut Canvas<'_>,
    area: Rect,
    rows: &[crate::layout::OptionRowRect],
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    c.draw_text(
        area.x + 48,
        area.y + 42,
        "Choose your keyboard layout",
        22.0,
        true,
        pal.text_primary,
    );
    c.draw_text(
        area.x + 48,
        area.y + 68,
        "You can switch layouts anytime from the Menu Bar indicator.",
        13.5,
        false,
        pal.text_secondary,
    );
    for row in rows {
        let hov = hover == Some(InstallerHit::SelectKeyboard(row.idx));
        draw_option_row(
            c,
            row.rect,
            &row.title,
            &row.subtitle,
            row.selected,
            hov,
            pal,
        );
    }
}

fn render_welcome(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    hero_emblem: Rect,
    cards: &[crate::layout::ActionCardRect],
    status_rect: Rect,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    draw_rustyos_emblem(c, hero_emblem, pal.accent, pal.dark);
    c.draw_text_centered(
        Rect::new(
            hero_emblem.x - 160,
            hero_emblem.bottom() + 6,
            hero_emblem.w + 320,
            28,
        ),
        "Welcome to RustyOS",
        22.0,
        true,
        pal.text_primary,
    );
    c.draw_text_centered(
        Rect::new(
            hero_emblem.x - 200,
            hero_emblem.bottom() + 34,
            hero_emblem.w + 400,
            20,
        ),
        "Install RustyOS onto your drive or verify hardware compatibility first",
        13.0,
        false,
        pal.text_secondary,
    );

    for card in cards {
        let hovered = hover == Some(InstallerHit::WelcomeCard(card.action));
        let bg = if hovered { pal.card_hover } else { pal.card_bg };
        c.fill_rounded_rect(card.rect, CARD_CORNER_RADIUS, bg);
        c.stroke_rounded_rect(
            card.rect,
            CARD_CORNER_RADIUS,
            1.0,
            if hovered { pal.accent } else { pal.card_border },
        );
        let badge = Rect::new(card.rect.x + 14, card.rect.y + 11, 36, 36);
        c.fill_rounded_rect(badge, 10, pal.accent_soft);
        let glyph = match card.action {
            WelcomeAction::InstallToDisk => "OS",
            WelcomeAction::RunDoctor => "OK",
            WelcomeAction::OpenTerminal => ">_",
        };
        c.draw_text_centered(badge, glyph, 13.0, true, pal.accent);
        c.draw_text(
            badge.right() + 14,
            card.rect.y + 24,
            card.title,
            14.5,
            true,
            pal.text_primary,
        );
        c.draw_text(
            badge.right() + 14,
            card.rect.y + 44,
            card.subtitle,
            12.5,
            false,
            pal.text_secondary,
        );
    }

    if let Some(status) = &app.utility_status {
        c.fill_rounded_rect(status_rect, 10, pal.accent_soft);
        c.draw_text_clipped(
            status_rect.x + 14,
            status_rect.y + 21,
            status,
            12.0,
            false,
            pal.text_primary,
            status_rect.right() - 14,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_select_disk(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    area: Rect,
    mode_btns: [Rect; 2],
    cards: &[crate::layout::DiskCardRect],
    refresh_btn: Rect,
    boot_btns: [Rect; 2],
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    c.draw_text(
        area.x + 12,
        area.y + 26,
        "Choose Installation Destination",
        18.0,
        true,
        pal.text_primary,
    );
    let rbg = if hover == Some(InstallerHit::RefreshDisks) {
        pal.pill_hover
    } else {
        pal.pill_bg
    };
    c.fill_rounded_rect(refresh_btn, 14, rbg);
    c.draw_text_centered(refresh_btn, "Refresh", 12.5, true, pal.text_primary);

    draw_choice_pill(
        c,
        mode_btns[0],
        "Erase Entire Disk",
        app.disk_mode == DiskInstallMode::WholeDisk,
        pal,
    );
    draw_choice_pill(
        c,
        mode_btns[1],
        "Dual-Boot (Partition / Free)",
        app.disk_mode == DiskInstallMode::DualBoot,
        pal,
    );
    let hint = match app.disk_mode {
        DiskInstallMode::WholeDisk => "Wipes all partitions on selected drive",
        DiskInstallMode::DualBoot => "Preserves existing OS partitions and shared EFI",
    };
    c.draw_text_clipped(
        mode_btns[1].right() + 12,
        mode_btns[1].y + 19,
        hint,
        12.0,
        false,
        if app.disk_mode == DiskInstallMode::DualBoot {
            pal.ok_green
        } else {
            pal.text_secondary
        },
        area.right() - 12,
    );

    render_storage_cards(c, app, cards, hover, pal);

    draw_choice_pill(
        c,
        boot_btns[0],
        "systemd-boot (UEFI)",
        app.bootloader == BootloaderKind::SystemdBoot,
        pal,
    );
    draw_choice_pill(
        c,
        boot_btns[1],
        "GRUB (BIOS/UEFI)",
        app.bootloader == BootloaderKind::Grub,
        pal,
    );
}

fn render_storage_cards(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    cards: &[crate::layout::DiskCardRect],
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    for card in cards {
        let hovered = hover == Some(InstallerHit::SelectDisk(card.idx));
        match app.disk_mode {
            DiskInstallMode::WholeDisk => {
                let Some(disk) = app.disks.get(card.idx) else {
                    continue;
                };
                let meta = format!(
                    "{}  •  {}  •  {}",
                    disk.path,
                    disk.size_label,
                    disk.transport.to_ascii_uppercase()
                );
                let badge = if !disk.is_selectable() {
                    if disk.mounted {
                        "Protected (Mounted)"
                    } else {
                        "Too Small"
                    }
                } else if card.selected {
                    "Selected Target"
                } else {
                    "Available"
                };
                draw_storage_card(
                    c,
                    card.rect,
                    &disk.model,
                    &meta,
                    badge,
                    disk.is_selectable(),
                    card.selected,
                    hovered,
                    pal,
                );
            }
            DiskInstallMode::DualBoot => {
                let Some(target) = app.dualboot_targets.get(card.idx) else {
                    continue;
                };
                let badge = if !target.selectable {
                    target.badge_label
                } else if card.selected {
                    match target.kind {
                        DualBootKind::UnallocatedGap { .. } => "Dual-Boot Free Gap",
                        DualBootKind::ExistingPartition { .. } => "Dual-Boot Target",
                    }
                } else {
                    target.badge_label
                };
                draw_storage_card(
                    c,
                    card.rect,
                    &target.title,
                    &target.subtitle,
                    badge,
                    target.selectable,
                    card.selected,
                    hovered,
                    pal,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
fn draw_storage_card(
    c: &mut Canvas<'_>,
    r: Rect,
    title: &str,
    subtitle: &str,
    badge: &str,
    selectable: bool,
    selected: bool,
    hovered: bool,
    pal: &Palette,
) {
    let bg = if hovered && selectable {
        pal.card_hover
    } else {
        pal.card_bg
    };
    c.fill_rounded_rect(r, CARD_CORNER_RADIUS, bg);
    let border = if selected {
        pal.accent
    } else {
        pal.card_border
    };
    c.stroke_rounded_rect(
        r,
        CARD_CORNER_RADIUS,
        if selected { 2.0 } else { 1.0 },
        border,
    );

    let icon = Rect::new(r.x + 14, r.y + 9, 42, 40);
    c.fill_rounded_rect(icon, 9, [168, 174, 184, 255]);
    c.fill_rounded_rect(
        Rect::new(icon.x + 2, icon.y + 2, icon.w - 4, 24),
        7,
        [214, 218, 224, 255],
    );
    let led_color = if selected { pal.accent } else { pal.ok_green };
    c.fill_circle(
        (icon.right() - 8) as f32,
        (icon.bottom() - 7) as f32,
        2.5,
        led_color,
    );

    let title_color = if selectable {
        pal.text_primary
    } else {
        pal.text_muted
    };
    let tag_rect = Rect::new(r.right() - 144, r.y + 17, 130, 24);
    c.draw_text_clipped(
        icon.right() + 14,
        r.y + 24,
        title,
        14.0,
        true,
        title_color,
        tag_rect.x - 10,
    );
    c.draw_text_clipped(
        icon.right() + 14,
        r.y + 44,
        subtitle,
        11.8,
        false,
        pal.text_secondary,
        tag_rect.x - 10,
    );

    if !selectable {
        c.fill_rounded_rect(tag_rect, 12, pal.pill_bg);
        c.draw_text_centered(tag_rect, badge, 11.0, false, pal.text_muted);
    } else if selected {
        c.fill_rounded_rect(tag_rect, 12, pal.accent_soft);
        c.draw_text_centered(tag_rect, badge, 11.5, true, pal.accent);
    } else {
        c.fill_rounded_rect(tag_rect, 12, pal.pill_bg);
        c.draw_text_centered(tag_rect, badge, 11.5, false, pal.text_secondary);
    }
}

#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
fn render_account_and_style(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    area: Rect,
    fields: &[TextFieldRect],
    kb_chips: &[(usize, Rect, &'static str)],
    theme_chips: &[(ThemeChoice, Rect, &'static str)],
    swatches: &[(usize, Rect, [u8; 4], bool)],
    pal: &Palette,
) {
    c.draw_text(
        area.x + 6,
        area.y + 22,
        "Account & System Preferences",
        18.0,
        true,
        pal.text_primary,
    );
    for field in fields {
        draw_text_field(c, app, field, pal);
    }

    let rx = fields.first().map_or(area.x + 360, |f| f.rect.right() + 20);
    c.draw_text(
        rx,
        area.y + 58,
        "Keyboard Layout",
        13.0,
        true,
        pal.text_secondary,
    );
    for &(idx, r, lbl) in kb_chips {
        draw_choice_pill(c, r, lbl, app.keyboard_idx == idx, pal);
    }

    c.draw_text(
        rx,
        area.y + 132,
        "Appearance Mode",
        13.0,
        true,
        pal.text_secondary,
    );
    for &(choice, r, lbl) in theme_chips {
        draw_choice_pill(c, r, lbl, app.theme_choice == choice, pal);
    }

    c.draw_text(
        rx,
        area.y + 208,
        "Accent Color",
        13.0,
        true,
        pal.text_secondary,
    );
    draw_accent_swatches(c, swatches, pal);
}

#[allow(
    clippy::too_many_arguments,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
fn render_installing(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    area: Rect,
    emblem_rect: Rect,
    progress_rect: Rect,
    toggle_logs_btn: Rect,
    log_box: Option<Rect>,
    summary_lines: &[String],
    pal: &Palette,
) {
    draw_rustyos_emblem(c, emblem_rect, pal.accent, pal.dark);
    let started = app.phase != InstallPhase::Review;
    let has_failed = app.install_error.is_some();
    let heading = match app.phase {
        InstallPhase::Finished => "Installation Complete",
        InstallPhase::Running => "Installing RustyOS...",
        InstallPhase::Review if has_failed => "Installation Failed",
        InstallPhase::Review => "Ready to Install RustyOS",
    };
    c.draw_text_centered(
        Rect::new(area.x, emblem_rect.bottom() + 6, area.w, 28),
        heading,
        20.0,
        true,
        if has_failed {
            pal.danger
        } else {
            pal.text_primary
        },
    );
    let msg_w = c.text_width(&app.stage_message, 13.0, false);
    if msg_w <= area.w - 48 {
        c.draw_text_centered(
            Rect::new(area.x, emblem_rect.bottom() + 34, area.w, 20),
            &app.stage_message,
            13.0,
            false,
            pal.text_secondary,
        );
    } else {
        c.draw_text_clipped(
            area.x + 24,
            emblem_rect.bottom() + 48,
            &app.stage_message,
            13.0,
            false,
            pal.text_secondary,
            area.right() - 24,
        );
    }

    if started || has_failed {
        c.fill_rounded_rect(progress_rect, 6, pal.pill_bg);
        let frac = app.progress_anim.current().clamp(0.0, 1.0);
        let fill_w = (f64::from(progress_rect.w) * frac).round() as i32;
        if fill_w > 4 {
            c.fill_rounded_rect(
                Rect::new(progress_rect.x, progress_rect.y, fill_w, progress_rect.h),
                6,
                if has_failed { pal.danger } else { pal.accent },
            );
        }
        c.fill_rounded_rect(toggle_logs_btn, 14, pal.pill_bg);
        c.draw_text_centered(
            toggle_logs_btn,
            if app.show_logs {
                "Hide Details"
            } else {
                "Show Details"
            },
            12.0,
            true,
            pal.text_primary,
        );
        if let Some(lb) = log_box {
            c.fill_rounded_rect(lb, CARD_CORNER_RADIUS, pal.field_bg);
            c.stroke_rounded_rect(lb, CARD_CORNER_RADIUS, 1.0, pal.card_border);
            let max_lines = ((lb.h - 16) / 18).max(1) as usize;
            let start = app.log_lines.len().saturating_sub(max_lines);
            for (i, line) in app.log_lines[start..].iter().enumerate() {
                c.draw_text_clipped(
                    lb.x + 12,
                    lb.y + 22 + i32::try_from(i).unwrap_or(0) * 18,
                    line,
                    11.5,
                    false,
                    pal.text_secondary,
                    lb.right() - 12,
                );
            }
        }
    } else {
        let card = Rect::new(area.x + 48, progress_rect.y - 6, area.w - 96, 138);
        c.fill_rounded_rect(card, CARD_CORNER_RADIUS, pal.card_bg);
        c.stroke_rounded_rect(card, CARD_CORNER_RADIUS, 1.0, pal.card_border);
        for (i, line) in summary_lines.iter().enumerate() {
            c.draw_text_clipped(
                card.x + 18,
                card.y + 34 + i32::try_from(i).unwrap_or(0) * 32,
                line,
                13.5,
                i == 0,
                pal.text_primary,
                card.right() - 18,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_network_step(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    area: Rect,
    card_rect: Rect,
    wifi_toggle_btn: Rect,
    refresh_btn: Rect,
    wifi_rows: &[WifiRowRect],
    password_field: Option<&TextFieldRect>,
    connect_btn: Option<Rect>,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    c.draw_text(
        area.x + 36,
        area.y + 34,
        "Connect to the Internet",
        21.0,
        true,
        pal.text_primary,
    );
    c.draw_text_clipped(
        area.x + 36,
        area.y + 56,
        if app.mode == WizardMode::Install {
            "Ethernet is detected automatically, or select a Wi-Fi network to download Arch & RustyOS packages."
        } else {
            "Network access enables Arch package updates and installing optional apps right away."
        },
        13.0,
        false,
        pal.text_secondary,
        area.right() - 36,
    );
    render_network_status_card(c, app, card_rect, wifi_toggle_btn, refresh_btn, hover, pal);
    render_wifi_list_and_auth(
        c,
        app,
        area,
        card_rect.bottom(),
        wifi_rows,
        password_field,
        connect_btn,
        hover,
        pal,
    );
}

fn render_network_status_card(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    card_rect: Rect,
    wifi_toggle_btn: Rect,
    refresh_btn: Rect,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    c.fill_rounded_rect(card_rect, CARD_CORNER_RADIUS, pal.card_bg);
    c.stroke_rounded_rect(card_rect, CARD_CORNER_RADIUS, 1.0, pal.card_border);

    let tbg = if hover == Some(InstallerHit::ToggleWifi) {
        pal.pill_hover
    } else {
        pal.pill_bg
    };
    c.fill_rounded_rect(wifi_toggle_btn, 14, tbg);
    c.draw_text_centered(
        wifi_toggle_btn,
        if app.network.wifi_enabled {
            "Wi-Fi: On"
        } else {
            "Wi-Fi: Off"
        },
        12.0,
        true,
        pal.text_primary,
    );

    let rbg = if hover == Some(InstallerHit::RefreshNetwork) {
        pal.pill_hover
    } else {
        pal.pill_bg
    };
    c.fill_rounded_rect(refresh_btn, 14, rbg);
    c.draw_text_centered(
        refresh_btn,
        if app.network.scanning {
            "Scanning..."
        } else {
            "Scan"
        },
        12.0,
        true,
        pal.text_primary,
    );

    let badge = Rect::new(card_rect.x + 16, card_rect.y + 15, 44, 44);
    let badge_bg = if app.network.internet_ready {
        pal.accent_soft
    } else {
        pal.pill_bg
    };
    c.fill_rounded_rect(badge, 11, badge_bg);
    c.draw_text_centered(
        badge,
        if app.network.ethernet_online {
            "ETH"
        } else if app.network.wifi_connected {
            "OK"
        } else {
            "NET"
        },
        13.0,
        true,
        if app.network.internet_ready {
            pal.accent
        } else {
            pal.text_secondary
        },
    );
    c.draw_text(
        badge.right() + 14,
        card_rect.y + 33,
        if app.network.internet_ready {
            "Internet Connected"
        } else {
            "Not Connected"
        },
        15.0,
        true,
        pal.text_primary,
    );
    c.draw_text_clipped(
        badge.right() + 14,
        card_rect.y + 53,
        &app.network.detail,
        12.5,
        false,
        pal.text_secondary,
        wifi_toggle_btn.x - 10,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_wifi_list_and_auth(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    area: Rect,
    card_bottom: i32,
    wifi_rows: &[WifiRowRect],
    password_field: Option<&TextFieldRect>,
    connect_btn: Option<Rect>,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    if wifi_rows.is_empty() {
        c.draw_text_clipped(
            area.x + 36,
            card_bottom + 26,
            if app.network.wifi_enabled {
                "No Wi-Fi networks found yet — click Scan, or plug in Ethernet / USB tethering."
            } else {
                "Wi-Fi radio is turned off."
            },
            12.5,
            false,
            pal.text_muted,
            area.right() - 36,
        );
    } else {
        for row in wifi_rows {
            let hovered = hover == Some(InstallerHit::SelectWifi(row.idx));
            let bg = if row.selected {
                pal.accent_soft
            } else if hovered {
                pal.pill_hover
            } else {
                pal.card_bg
            };
            let border = if row.selected {
                pal.accent
            } else {
                pal.card_border
            };
            c.fill_rounded_rect(row.rect, 10, bg);
            c.stroke_rounded_rect(row.rect, 10, if row.selected { 1.6 } else { 1.0 }, border);
            let status_txt = if row.ap.connected {
                format!("Connected  •  {}%", row.ap.signal_percent)
            } else if row.ap.known {
                format!("Saved  •  {}%", row.ap.signal_percent)
            } else if row.ap.secure {
                format!("Secured  •  {}%", row.ap.signal_percent)
            } else {
                format!("Open  •  {}%", row.ap.signal_percent)
            };
            let title_limit = row.rect.right() - 170;
            c.draw_text_clipped(
                row.rect.x + 14,
                row.rect.y + 22,
                &row.ap.ssid,
                13.5,
                true,
                pal.text_primary,
                title_limit,
            );
            c.draw_text_clipped(
                row.rect.right() - 156,
                row.rect.y + 22,
                &status_txt,
                12.0,
                row.ap.connected,
                if row.ap.connected {
                    pal.accent
                } else {
                    pal.text_secondary
                },
                row.rect.right() - 12,
            );
        }
    }

    render_wifi_auth_controls(c, app, area, password_field, connect_btn, hover, pal);
}

fn render_wifi_auth_controls(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    area: Rect,
    password_field: Option<&TextFieldRect>,
    connect_btn: Option<Rect>,
    hover: Option<InstallerHit>,
    pal: &Palette,
) {
    if let Some(pf) = password_field {
        draw_text_field(c, app, pf, pal);
    }
    if let Some(btn) = connect_btn {
        let cbg = if hover == Some(InstallerHit::ConnectWifi) {
            [
                pal.accent[0].saturating_add(14),
                pal.accent[1].saturating_add(14),
                pal.accent[2].saturating_add(14),
                255,
            ]
        } else {
            pal.accent
        };
        c.fill_rounded_rect(btn, 14, cbg);
        c.draw_text_centered(
            btn,
            if app.network.connecting_ssid.is_some() {
                "Connecting..."
            } else {
                "Connect"
            },
            12.5,
            true,
            [255, 255, 255, 255],
        );
    }
    if let Some(err) = &app.network.connect_error {
        c.draw_text_clipped(
            area.x + 36,
            area.bottom() - 2,
            err,
            12.0,
            false,
            pal.danger,
            area.right() - 36,
        );
    }
}

fn render_oobe_account(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    area: Rect,
    fields: &[TextFieldRect],
    autologin_row: Rect,
    pal: &Palette,
) {
    c.draw_text(
        area.x + 64,
        area.y + 30,
        "Create Your Computer Account",
        21.0,
        true,
        pal.text_primary,
    );
    c.draw_text(
        area.x + 64,
        area.y + 52,
        "Configure your local profile and hostname for RustyOS.",
        13.0,
        false,
        pal.text_secondary,
    );
    for field in fields {
        draw_text_field(c, app, field, pal);
    }
    c.fill_rounded_rect(autologin_row, 10, pal.card_bg);
    c.stroke_rounded_rect(autologin_row, 10, 1.0, pal.card_border);
    let pill = Rect::new(autologin_row.x + 12, autologin_row.y + 9, 18, 18);
    c.fill_rounded_rect(
        pill,
        5,
        if app.account.autologin {
            pal.accent
        } else {
            pal.pill_bg
        },
    );
    if app.account.autologin {
        c.draw_line(
            pill.x as f32 + 4.5,
            pill.y as f32 + 9.5,
            pill.x as f32 + 8.0,
            pill.y as f32 + 13.0,
            2.0,
            [255, 255, 255, 255],
        );
        c.draw_line(
            pill.x as f32 + 8.0,
            pill.y as f32 + 13.0,
            pill.x as f32 + 13.5,
            pill.y as f32 + 5.5,
            2.0,
            [255, 255, 255, 255],
        );
    }
    c.draw_text(
        pill.right() + 12,
        autologin_row.y + 23,
        "Sign in automatically on boot (greetd session)",
        13.0,
        false,
        pal.text_primary,
    );
}

#[allow(clippy::cast_precision_loss)]
fn render_oobe_appearance(
    c: &mut Canvas<'_>,
    area: Rect,
    tiles: &[ThemeTileRect; 3],
    accent_card: Rect,
    swatches: &[(usize, Rect, [u8; 4], bool)],
    pal: &Palette,
) {
    c.draw_text_centered(
        Rect::new(area.x, area.y + 12, area.w, 30),
        "Choose Your Look",
        22.0,
        true,
        pal.text_primary,
    );
    c.draw_text_centered(
        Rect::new(area.x, area.y + 42, area.w, 22),
        "Select Golden Gate Light, Dark, or Auto and customize your system accent color.",
        13.0,
        false,
        pal.text_secondary,
    );

    for tile in tiles {
        let p = tile.preview_rect;
        c.fill_rounded_rect(tile.ring_rect, 14, pal.card_bg);
        match tile.choice {
            ThemeChoice::Light => {
                c.fill_rounded_rect(p, 9, [242, 245, 250, 255]);
                c.fill_rounded_rect(
                    Rect::new(p.x + 12, p.y + 18, p.w - 24, p.h - 26),
                    6,
                    [255, 255, 255, 255],
                );
                c.fill_rounded_rect(Rect::new(p.x + 20, p.y + 28, 34, 6), 3, pal.accent);
            }
            ThemeChoice::Dark => {
                c.fill_rounded_rect(p, 9, [30, 32, 38, 255]);
                c.fill_rounded_rect(
                    Rect::new(p.x + 12, p.y + 18, p.w - 24, p.h - 26),
                    6,
                    [48, 50, 58, 255],
                );
                c.fill_rounded_rect(Rect::new(p.x + 20, p.y + 28, 34, 6), 3, pal.accent);
            }
            ThemeChoice::Auto => {
                c.fill_rounded_rect(p, 9, [242, 245, 250, 255]);
                c.fill_rounded_rect(
                    Rect::new(p.x + p.w / 2, p.y, p.w / 2, p.h),
                    8,
                    [30, 32, 38, 255],
                );
                c.fill_rounded_rect(Rect::new(p.x + 20, p.y + 28, 34, 6), 3, pal.accent);
            }
        }
        c.stroke_rounded_rect(
            tile.ring_rect,
            14,
            if tile.selected { 2.5 } else { 1.0 },
            if tile.selected {
                pal.accent
            } else {
                pal.card_border
            },
        );
        c.draw_text_centered(
            Rect::new(
                tile.ring_rect.x,
                tile.ring_rect.bottom() + 4,
                tile.ring_rect.w,
                22,
            ),
            tile.label,
            13.0,
            tile.selected,
            if tile.selected {
                pal.accent
            } else {
                pal.text_primary
            },
        );
    }

    c.fill_rounded_rect(accent_card, CARD_CORNER_RADIUS, pal.card_bg);
    c.stroke_rounded_rect(accent_card, CARD_CORNER_RADIUS, 1.0, pal.card_border);
    c.draw_text(
        accent_card.x + 18,
        accent_card.y + 24,
        "Accent Color",
        13.5,
        true,
        pal.text_primary,
    );
    draw_accent_swatches(c, swatches, pal);
}

fn render_oobe_finish(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    area: Rect,
    hero_emblem: Rect,
    summary_card: Rect,
    pal: &Palette,
) {
    draw_rustyos_emblem(c, hero_emblem, pal.accent, pal.dark);
    c.draw_text_centered(
        Rect::new(area.x, hero_emblem.bottom() + 14, area.w, 32),
        "Your RustyOS Desktop is Ready",
        23.0,
        true,
        pal.text_primary,
    );
    c.draw_text_centered(
        Rect::new(area.x, hero_emblem.bottom() + 46, area.w, 22),
        "Settings will be saved and applied immediately.",
        13.5,
        false,
        pal.text_secondary,
    );
    c.fill_rounded_rect(summary_card, CARD_CORNER_RADIUS, pal.card_bg);
    c.stroke_rounded_rect(summary_card, CARD_CORNER_RADIUS, 1.0, pal.card_border);

    let (_, lang_lbl, _) = LANGUAGE_OPTIONS[app.language_idx];
    let (_, kb_lbl, _, _) = KEYBOARD_PRESETS[app.keyboard_idx];
    let theme_lbl = if app.resolved_dark {
        "Golden Gate Dark"
    } else {
        "Golden Gate Light"
    };
    let rows = [
        format!("Language & Region:  {lang_lbl}"),
        format!("Keyboard Layout:    {kb_lbl}"),
        format!(
            "User Profile:       {} ({})",
            app.account.full_name, app.account.username
        ),
        format!(
            "Theme & Accent:     {theme_lbl}  •  Accent #{}",
            app.accent_index + 1
        ),
    ];
    for (i, line) in rows.iter().enumerate() {
        #[allow(clippy::cast_possible_wrap)]
        c.draw_text(
            summary_card.x + 22,
            summary_card.y + 38 + (i as i32) * 34,
            line,
            13.5,
            i == 3,
            pal.text_primary,
        );
    }
}

fn draw_option_row(
    c: &mut Canvas<'_>,
    r: Rect,
    title: &str,
    subtitle: &str,
    selected: bool,
    hovered: bool,
    pal: &Palette,
) {
    let bg = if selected {
        pal.accent_soft
    } else if hovered {
        pal.card_hover
    } else {
        pal.card_bg
    };
    c.fill_rounded_rect(r, CARD_CORNER_RADIUS, bg);
    c.stroke_rounded_rect(
        r,
        CARD_CORNER_RADIUS,
        if selected { 1.8 } else { 1.0 },
        if selected {
            pal.accent
        } else {
            pal.card_border
        },
    );
    c.draw_text(r.x + 18, r.y + 23, title, 14.0, true, pal.text_primary);
    c.draw_text(
        r.x + 18,
        r.y + 41,
        subtitle,
        12.0,
        false,
        pal.text_secondary,
    );
    if selected {
        let dot = Rect::new(r.right() - 34, r.y + (r.h - 20) / 2, 20, 20);
        c.fill_circle(dot.x as f32 + 10.0, dot.y as f32 + 10.0, 9.0, pal.accent);
        c.draw_line(
            dot.x as f32 + 6.0,
            dot.y as f32 + 10.2,
            dot.x as f32 + 9.0,
            dot.y as f32 + 13.5,
            2.0,
            [255, 255, 255, 255],
        );
        c.draw_line(
            dot.x as f32 + 9.0,
            dot.y as f32 + 13.5,
            dot.x as f32 + 14.5,
            dot.y as f32 + 7.0,
            2.0,
            [255, 255, 255, 255],
        );
    }
}

fn draw_choice_pill(c: &mut Canvas<'_>, r: Rect, label: &str, active: bool, pal: &Palette) {
    c.fill_rounded_rect(r, 10, if active { pal.accent_soft } else { pal.pill_bg });
    if active {
        c.stroke_rounded_rect(r, 10, 1.5, pal.accent);
    }
    c.draw_text_centered(
        r,
        label,
        12.0,
        active,
        if active { pal.accent } else { pal.text_primary },
    );
}

fn draw_text_field(
    c: &mut Canvas<'_>,
    app: &InstallerAppState,
    field: &TextFieldRect,
    pal: &Palette,
) {
    c.draw_text(
        field.rect.x,
        field.rect.y - 6,
        field.label,
        12.0,
        true,
        pal.text_secondary,
    );
    let focused = app.caret.focused_field == Some(field.id);
    c.fill_rounded_rect(field.rect, 9, pal.field_bg);
    c.stroke_rounded_rect(
        field.rect,
        9,
        if focused { 2.0 } else { 1.0 },
        if focused { pal.accent } else { pal.card_border },
    );
    let raw = match field.id {
        TextFieldId::FullName => app.account.full_name.as_str(),
        TextFieldId::Username => app.account.username.as_str(),
        TextFieldId::Hostname => app.account.hostname.as_str(),
        TextFieldId::Password => app.account.password.as_str(),
        TextFieldId::ConfirmPassword => app.account.confirm_password.as_str(),
        TextFieldId::WifiPassword => app.wifi_password.as_str(),
    };
    let display = if field.masked {
        "•".repeat(raw.chars().count())
    } else {
        raw.to_owned()
    };
    let tx = field.rect.x + 11;
    let ty = field.rect.y + 22;
    c.draw_text_clipped(
        tx,
        ty,
        &display,
        13.5,
        false,
        pal.text_primary,
        field.rect.right() - 10,
    );
    if focused && app.caret.visible {
        let tw = c.text_width(&display, 13.5, false);
        let cx = (tx + tw).min(field.rect.right() - 8);
        c.fill_rounded_rect(
            Rect::new(cx, field.rect.y + 7, 2, field.rect.h - 14),
            1,
            pal.accent,
        );
    }
}

#[allow(clippy::cast_precision_loss)]
fn draw_accent_swatches(
    c: &mut Canvas<'_>,
    swatches: &[(usize, Rect, [u8; 4], bool)],
    pal: &Palette,
) {
    for &(_, r, rgba, active) in swatches {
        let cx = r.x as f32 + r.w as f32 * 0.5;
        let cy = r.y as f32 + r.h as f32 * 0.5;
        let rad = r.w as f32 * 0.38;
        if active {
            c.fill_circle(cx, cy, rad + 4.5, pal.accent_soft);
            c.fill_circle(
                cx,
                cy,
                rad + 2.5,
                [255, 255, 255, if pal.dark { 220 } else { 255 }],
            );
        }
        c.fill_circle(cx, cy, rad, [rgba[0], rgba[1], rgba[2], 255]);
        if active {
            c.draw_line(
                cx - 4.5,
                cy + 0.5,
                cx - 1.2,
                cy + 4.0,
                2.0,
                [255, 255, 255, 255],
            );
            c.draw_line(
                cx - 1.2,
                cy + 4.0,
                cx + 4.8,
                cy - 3.5,
                2.0,
                [255, 255, 255, 255],
            );
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn draw_rustyos_emblem(c: &mut Canvas<'_>, r: Rect, accent: [u8; 4], dark: bool) {
    c.fill_rounded_rect(r, (r.w as f32 * 0.24) as i32, accent);
    let inner = Rect::new(r.x + 2, r.y + 2, r.w - 4, r.h / 2);
    c.fill_rounded_rect(
        inner,
        (r.w as f32 * 0.2) as i32,
        [255, 255, 255, if dark { 24 } else { 36 }],
    );
    let cx = r.x as f32 + r.w as f32 * 0.5;
    let cy = r.y as f32 + r.h as f32 * 0.5;
    c.fill_circle(cx, cy, r.w as f32 * 0.28, [255, 255, 255, 240]);
    c.fill_circle(cx, cy, r.w as f32 * 0.14, accent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::compute_layout;
    use crate::state::{StartupOptions, WizardMode};

    #[test]
    fn renders_install_and_oobe_with_clean_corners_and_accent() {
        for (dark, accent) in [(false, 0_usize), (true, 1_usize)] {
            let opts = StartupOptions {
                mode: WizardMode::Oobe,
                env_kind: crate::state::RunEnv::Demo,
                theme_override: Some(if dark {
                    rusty_services::file_manager::FilesThemeMode::DarkPurpleGlass
                } else {
                    rusty_services::file_manager::FilesThemeMode::LightGoldenGate
                }),
                accent_override: Some(accent),
                ..Default::default()
            };
            let app = InstallerAppState::new(&opts);
            let layout = compute_layout(&app, 800, 580);
            let mut buf = vec![0_u8; 800 * 580 * 4];
            render_window_pixels(&mut buf, 800, 580, &app, &layout, None, true);
            assert_eq!(
                buf[3], 0,
                "top-left pixel outside rounded corner must be transparent"
            );
            let center_alpha = buf[((290 * 800 + 400) * 4) + 3];
            assert!(
                center_alpha >= 215,
                "center glass body alpha must be >= 215"
            );
        }
    }

    #[test]
    fn renders_install_network_wifi_and_failure_details_cleanly() {
        let mut app = InstallerAppState::new(&StartupOptions {
            mode: WizardMode::Install,
            env_kind: crate::state::RunEnv::Demo,
            ..Default::default()
        });
        app.install_step = crate::state::InstallStep::Network;
        app.select_wifi_row(1);
        let net_layout = compute_layout(&app, 800, 580);
        let mut buf = vec![0_u8; 800 * 580 * 4];
        render_window_pixels(&mut buf, 800, 580, &app, &net_layout, None, true);
        assert_eq!(buf[3], 0);
        assert!(buf.iter().skip(3).step_by(4).any(|&a| a >= 215));

        app.install_step = crate::state::InstallStep::Installing;
        app.install_error = Some(String::from("Installer failed: error: core.db unreachable"));
        app.show_logs = true;
        app.log_lines.push(String::from(
            "[stderr] error: failed to synchronize all databases",
        ));
        let err_layout = compute_layout(&app, 800, 580);
        render_window_pixels(&mut buf, 800, 580, &app, &err_layout, None, true);
        assert_eq!(buf[3], 0);
    }

    #[test]
    fn traffic_lights_render_vector_hover_glyphs() {
        let app = InstallerAppState::new(&StartupOptions::default());
        let layout = compute_layout(&app, 800, 580);
        let mut unhovered = vec![0_u8; 800 * 580 * 4];
        render_window_pixels(&mut unhovered, 800, 580, &app, &layout, None, true);

        let mut hovered = vec![0_u8; 800 * 580 * 4];
        render_window_pixels(
            &mut hovered,
            800,
            580,
            &app,
            &layout,
            Some(crate::layout::InstallerHit::TrafficClose),
            true,
        );

        let min_center_off = ((20 * 800 + 43) * 4) as usize;
        assert!(hovered[min_center_off + 2] < unhovered[min_center_off + 2]);
    }
}
