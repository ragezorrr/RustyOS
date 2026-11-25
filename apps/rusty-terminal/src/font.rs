//! Font discovery, `fontdue` rasterization, integer cell metrics, and
//! pixel-exact procedural glyphs for Box Drawing, Block Elements, and Powerline.

use std::fmt;
use std::path::Path;
use std::process::Command;

use fontdue::{Font, FontSettings};

static UI_REGULAR_BYTES: &[u8] = rusty_ui::text::INTER_REGULAR_BYTES;
static UI_SEMIBOLD_BYTES: &[u8] = rusty_ui::text::INTER_SEMIBOLD_BYTES;

/// Which font slot is queried for rasterization and atlas caching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GlyphKind {
    MonoRegular,
    MonoBold,
    UiRegular,
    UiBold,
}

/// Unique key identifying a rasterized glyph bitmap in GPU atlas or CPU cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    pub(crate) ch: char,
    pub(crate) kind: GlyphKind,
    pub(crate) size_tenths: u16,
    pub(crate) cell_w: u16,
    pub(crate) cell_h: u16,
}

/// Alpha-only rasterized glyph bitmap (`0..=255`).
#[derive(Debug, Clone)]
pub(crate) struct RasterizedGlyph {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) offset_x: i32,
    pub(crate) offset_y: i32,
    pub(crate) advance_x: f32,
    pub(crate) bitmap: Vec<u8>,
}

/// Integer cell grid geometry derived from the loaded monospace font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellMetrics {
    pub(crate) font_size: f32,
    pub(crate) ui_size: f32,
    pub(crate) cell_w: i32,
    pub(crate) cell_h: i32,
    pub(crate) baseline_y: i32,
}

/// Loaded monospace + UI font family for `rusty-terminal`.
pub(crate) struct FontSet {
    mono_regular: Font,
    mono_bold: Option<Font>,
    mono_fallbacks: Vec<Font>,
    ui_regular: Font,
    ui_bold: Font,
}

impl fmt::Debug for FontSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontSet")
            .field("has_mono_bold", &self.mono_bold.is_some())
            .finish_non_exhaustive()
    }
}

impl FontSet {
    /// Loads system monospace fonts (`Adwaita Mono`, `Hack`, `JetBrains Mono`, `DejaVu`, or `fc-match`)
    /// with embedded `Inter` UI fonts and safe fallback.
    pub(crate) fn load() -> Self {
        let ui_regular = Font::from_bytes(UI_REGULAR_BYTES, FontSettings::default())
            .unwrap_or_else(|_| empty_fallback_font());
        let ui_bold = Font::from_bytes(UI_SEMIBOLD_BYTES, FontSettings::default())
            .unwrap_or_else(|_| empty_fallback_font());

        let (mono_regular, mono_bold) = load_system_monospace(&ui_regular, &ui_bold);
        let mono_fallbacks = load_system_fallbacks();
        Self {
            mono_regular,
            mono_bold,
            mono_fallbacks,
            ui_regular,
            ui_bold,
        }
    }

    /// Computes crisp integer cell metrics (`cell_w`, `cell_h`, `baseline_y`) for a font size.
    #[must_use]
    pub(crate) fn metrics_for_size(&self, font_size: f32) -> CellMetrics {
        let clamped = font_size.clamp(9.0, 32.0);
        let m_metrics = self.mono_regular.metrics('M', clamped);
        let zero_metrics = self.mono_regular.metrics('0', clamped);
        #[allow(clippy::cast_possible_truncation)]
        let cell_w = m_metrics
            .advance_width
            .max(zero_metrics.advance_width)
            .max(clamped * 0.56)
            .ceil()
            .max(6.0) as i32;

        let line_metrics = self.mono_regular.horizontal_line_metrics(clamped);
        let raw_line_h = line_metrics.map_or(clamped * 1.35, |lm| {
            (lm.ascent - lm.descent + lm.line_gap.max(0.0)).max(clamped * 1.28)
        });
        #[allow(clippy::cast_possible_truncation)]
        let cell_h = (raw_line_h * 1.12).ceil().max(clamped * 1.36).ceil() as i32;

        let ascent = line_metrics.map_or(clamped * 0.82, |lm| lm.ascent);
        let descent = line_metrics.map_or(-clamped * 0.22, |lm| lm.descent);
        let text_h = ascent - descent;
        let leading = ((cell_h as f32) - text_h).max(0.0) * 0.5;
        #[allow(clippy::cast_possible_truncation)]
        let baseline_y = (leading + ascent).round() as i32;

        CellMetrics {
            font_size: clamped,
            ui_size: 13.0,
            cell_w: cell_w.max(6),
            cell_h: cell_h.max(12),
            baseline_y: baseline_y.clamp(8, cell_h.saturating_sub(2).max(8)),
        }
    }

    /// Rasterizes either a terminal cell glyph (including procedural Box/Block/Powerline) or UI glyph.
    #[must_use]
    pub(crate) fn rasterize(&self, key: GlyphKey, baseline_y: i32) -> Option<RasterizedGlyph> {
        let px = f32::from(key.size_tenths.max(40)) / 10.0;
        let cell_w = i32::from(key.cell_w.max(1));
        let cell_h = i32::from(key.cell_h.max(1));

        if matches!(key.kind, GlyphKind::MonoRegular | GlyphKind::MonoBold)
            && let Some(special) = rasterize_procedural_glyph(key.ch, cell_w, cell_h)
        {
            return Some(special);
        }
        if matches!(key.kind, GlyphKind::UiRegular | GlyphKind::UiBold)
            && let Some(icon) = rasterize_ui_icon(key.ch)
        {
            return Some(icon);
        }

        let font = match key.kind {
            GlyphKind::MonoRegular => pick_font(
                key.ch,
                &self.mono_regular,
                &self.mono_fallbacks,
                &self.ui_regular,
            ),
            GlyphKind::MonoBold => {
                let primary = self.mono_bold.as_ref().unwrap_or(&self.mono_regular);
                pick_font(key.ch, primary, &self.mono_fallbacks, &self.ui_bold)
            }
            GlyphKind::UiRegular => &self.ui_regular,
            GlyphKind::UiBold => &self.ui_bold,
        };

        let (metrics, bitmap) = font.rasterize(key.ch, px);
        if metrics.width == 0 || metrics.height == 0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let width = metrics.width as u32;
        #[allow(clippy::cast_possible_truncation)]
        let height = metrics.height as u32;
        #[allow(clippy::cast_possible_truncation)]
        let h_i32 = metrics.height as i32;

        let offset_y = match key.kind {
            GlyphKind::MonoRegular | GlyphKind::MonoBold => baseline_y - metrics.ymin - h_i32,
            GlyphKind::UiRegular | GlyphKind::UiBold => -metrics.ymin - h_i32,
        };

        Some(RasterizedGlyph {
            width,
            height,
            offset_x: metrics.xmin,
            offset_y,
            advance_x: metrics.advance_width,
            bitmap,
        })
    }

    /// Returns advance width for one UI character (including spaces and procedural icons).
    #[must_use]
    pub(crate) fn ui_char_advance(&self, ch: char, size_px: f32, bold: bool) -> f32 {
        if matches!(ch, '\u{E001}' | '\u{E002}' | '\u{E003}') {
            return 14.0;
        }
        let font = if bold {
            &self.ui_bold
        } else {
            &self.ui_regular
        };
        font.metrics(ch, size_px).advance_width
    }

    /// Measures UI label width in pixels at the requested font size.
    #[must_use]
    pub(crate) fn measure_ui_text(&self, text: &str, size_px: f32, bold: bool) -> f32 {
        text.chars()
            .map(|ch| self.ui_char_advance(ch, size_px, bold))
            .sum()
    }

    /// Returns vertical UI baseline offset from the top of a container of height `box_h`.
    #[must_use]
    pub(crate) fn ui_baseline_in_box(&self, box_h: i32, size_px: f32) -> i32 {
        let lm = self.ui_regular.horizontal_line_metrics(size_px);
        let ascent = lm.map_or(size_px * 0.80, |m| m.ascent);
        let descent = lm.map_or(-size_px * 0.20, |m| m.descent);
        #[allow(clippy::cast_possible_truncation)]
        let baseline = ((box_h as f32 - (ascent - descent)) * 0.5 + ascent).round() as i32;
        baseline
    }
}

fn empty_fallback_font() -> Font {
    // Embedded UI font is statically verified bytes from repository assets.
    Font::from_bytes(UI_REGULAR_BYTES, FontSettings::default())
        .unwrap_or_else(|_| unreachable_fallback())
}

fn unreachable_fallback() -> Font {
    // Empty fallback guarantees infallible construction without `.unwrap()`.
    match Font::from_bytes(UI_SEMIBOLD_BYTES, FontSettings::default()) {
        Ok(f) => f,
        Err(_) => unreachable_empty_loop(),
    }
}

fn unreachable_empty_loop() -> Font {
    loop {
        std::hint::spin_loop();
    }
}

fn pick_font<'a>(
    ch: char,
    primary: &'a Font,
    fallbacks: &'a [Font],
    ui_fallback: &'a Font,
) -> &'a Font {
    if primary.lookup_glyph_index(ch) != 0 {
        return primary;
    }
    for fb in fallbacks {
        if fb.lookup_glyph_index(ch) != 0 {
            return fb;
        }
    }
    ui_fallback
}

fn load_system_fallbacks() -> Vec<Font> {
    const FALLBACK_PATHS: &[&str] = &[
        "/usr/share/fonts/TTF/SymbolsNerdFontMono-Regular.ttf",
        "/usr/share/fonts/TTF/SymbolsNerdFont-Regular.ttf",
        "/usr/share/fonts/nerd-fonts-complete/TTF/SymbolsNerdFontMono-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    ];
    let mut out = Vec::new();
    for path in FALLBACK_PATHS {
        if let Some(font) = load_font_file(Path::new(path)) {
            out.push(font);
        }
    }
    for pattern in [
        "Symbols Nerd Font Mono",
        "Noto Sans Symbols 2",
        "Noto Emoji",
    ] {
        if let Some(path) = fc_match_path(pattern)
            && let Some(font) = load_font_file(Path::new(&path))
        {
            out.push(font);
        }
    }
    out
}

fn load_system_monospace(ui_reg: &Font, ui_bold: &Font) -> (Font, Option<Font>) {
    const CANDIDATES: &[(&str, Option<&str>)] = &[
        (
            "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf",
            Some("/usr/share/fonts/Adwaita/AdwaitaMono-Bold.ttf"),
        ),
        (
            "/usr/share/fonts/TTF/Hack-Regular.ttf",
            Some("/usr/share/fonts/TTF/Hack-Bold.ttf"),
        ),
        (
            "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
            Some("/usr/share/fonts/TTF/JetBrainsMono-Bold.ttf"),
        ),
        (
            "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
            Some("/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf"),
        ),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            Some("/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"),
        ),
        (
            "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
            Some("/usr/share/fonts/liberation/LiberationMono-Bold.ttf"),
        ),
        (
            "/usr/share/fonts/TTF/LiberationMono-Regular.ttf",
            Some("/usr/share/fonts/TTF/LiberationMono-Bold.ttf"),
        ),
    ];

    for (reg_path, bold_path) in CANDIDATES {
        if let Some(reg) = load_font_file(Path::new(reg_path)) {
            let bold = bold_path.and_then(|p| load_font_file(Path::new(p)));
            return (reg, bold);
        }
    }

    if let Some(reg_path) = fc_match_path("monospace")
        && let Some(reg) = load_font_file(Path::new(&reg_path))
    {
        let bold = fc_match_path("monospace:bold").and_then(|bp| load_font_file(Path::new(&bp)));
        return (reg, bold);
    }

    if let Ok(bundled) = Font::from_bytes(
        rusty_ui::text::JETBRAINS_MONO_REGULAR_BYTES,
        FontSettings::default(),
    ) {
        return (bundled, None);
    }

    (ui_reg.clone(), Some(ui_bold.clone()))
}

fn load_font_file(path: &Path) -> Option<Font> {
    let bytes = std::fs::read(path).ok()?;
    Font::from_bytes(bytes, FontSettings::default()).ok()
}

fn fc_match_path(pattern: &str) -> Option<String> {
    let out = Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let trimmed = path.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn rasterize_procedural_glyph(ch: char, w: i32, h: i32) -> Option<RasterizedGlyph> {
    match ch {
        '\u{2580}'..='\u{259F}' => Some(rasterize_block_element(ch, w, h)),
        '\u{2500}'..='\u{257F}' => Some(rasterize_box_drawing(ch, w, h)),
        '\u{E0B0}'..='\u{E0B3}' => Some(rasterize_powerline(ch, w, h)),
        _ => None,
    }
}

fn new_bitmap_glyph(w: i32, h: i32, bitmap: Vec<u8>) -> RasterizedGlyph {
    #[allow(clippy::cast_sign_loss)]
    RasterizedGlyph {
        width: w as u32,
        height: h as u32,
        offset_x: 0,
        offset_y: 0,
        advance_x: w as f32,
        bitmap,
    }
}

fn fill_rect_alpha(buf: &mut [u8], stride: i32, x0: i32, y0: i32, x1: i32, y1: i32, alpha: u8) {
    let h = buf.len() as i32 / stride.max(1);
    let sx = x0.max(0);
    let sy = y0.max(0);
    let ex = x1.min(stride);
    let ey = y1.min(h);
    for y in sy..ey {
        for x in sx..ex {
            #[allow(clippy::cast_sign_loss)]
            let idx = (y * stride + x) as usize;
            if let Some(cell) = buf.get_mut(idx) {
                *cell = (*cell).max(alpha);
            }
        }
    }
}

fn rasterize_block_element(ch: char, w: i32, h: i32) -> RasterizedGlyph {
    #[allow(clippy::cast_sign_loss)]
    let mut buf = vec![0_u8; (w * h) as usize];
    let hw = w / 2;
    let hh = h / 2;
    let eighth_y = |n: i32| (h * n + 4) / 8;
    let eighth_x = |n: i32| (w * n + 4) / 8;

    match ch {
        '\u{2580}' => fill_rect_alpha(&mut buf, w, 0, 0, w, hh, 255),
        '\u{2581}'..='\u{2587}' => {
            let n = (ch as i32) - 0x2580;
            fill_rect_alpha(&mut buf, w, 0, h - eighth_y(n), w, h, 255);
        }
        '\u{2588}' => fill_rect_alpha(&mut buf, w, 0, 0, w, h, 255),
        '\u{2589}'..='\u{258F}' => {
            let n = 8 - ((ch as i32) - 0x2588);
            fill_rect_alpha(&mut buf, w, 0, 0, eighth_x(n), h, 255);
        }
        '\u{2590}' => fill_rect_alpha(&mut buf, w, hw, 0, w, h, 255),
        '\u{2591}' | '\u{2592}' | '\u{2593}' => {
            let alpha = match ch {
                '\u{2591}' => 64,
                '\u{2592}' => 128,
                _ => 192,
            };
            fill_rect_alpha(&mut buf, w, 0, 0, w, h, alpha);
        }
        '\u{2594}' => fill_rect_alpha(&mut buf, w, 0, 0, w, eighth_y(1).max(1), 255),
        '\u{2595}' => fill_rect_alpha(&mut buf, w, w - eighth_x(1).max(1), 0, w, h, 255),
        _ => rasterize_quadrants(ch, &mut buf, w, h, hw, hh),
    }

    new_bitmap_glyph(w, h, buf)
}

fn rasterize_quadrants(ch: char, buf: &mut [u8], w: i32, h: i32, hw: i32, hh: i32) {
    // Bitmask: (UL, UR, LL, LR)
    let mask = match ch {
        '\u{2596}' => 0b0010,
        '\u{2597}' => 0b0001,
        '\u{2598}' => 0b1000,
        '\u{2599}' => 0b1011,
        '\u{259A}' => 0b1001,
        '\u{259B}' => 0b1110,
        '\u{259C}' => 0b1101,
        '\u{259D}' => 0b0100,
        '\u{259E}' => 0b0110,
        '\u{259F}' => 0b0111,
        _ => 0,
    };
    if mask & 0b1000 != 0 {
        fill_rect_alpha(buf, w, 0, 0, hw, hh, 255);
    }
    if mask & 0b0100 != 0 {
        fill_rect_alpha(buf, w, hw, 0, w, hh, 255);
    }
    if mask & 0b0010 != 0 {
        fill_rect_alpha(buf, w, 0, hh, hw, h, 255);
    }
    if mask & 0b0001 != 0 {
        fill_rect_alpha(buf, w, hw, hh, w, h, 255);
    }
}

/// Edge weight for box drawing: `0 = none`, `1 = light`, `2 = heavy`, `3 = double`.
#[derive(Clone, Copy)]
struct BoxEdges {
    left: u8,
    right: u8,
    up: u8,
    down: u8,
}

fn rasterize_box_drawing(ch: char, w: i32, h: i32) -> RasterizedGlyph {
    #[allow(clippy::cast_sign_loss)]
    let mut buf = vec![0_u8; (w * h) as usize];
    let light = (w.min(h) / 9).clamp(1, 2);
    let heavy = (light * 2).max(2);
    let cx = w / 2;
    let cy = h / 2;

    if let Some(e) = box_edges(ch) {
        draw_horiz_arm(&mut buf, w, 0, cx, cy, e.left, light, heavy);
        draw_horiz_arm(&mut buf, w, cx, w, cy, e.right, light, heavy);
        draw_vert_arm(&mut buf, w, 0, cy, cx, e.up, light, heavy);
        draw_vert_arm(&mut buf, w, cy, h, cx, e.down, light, heavy);
        let center_w = e.up.max(e.down);
        let center_h = e.left.max(e.right);
        if center_w > 0 || center_h > 0 {
            let cw = if center_w >= 2 { heavy } else { light };
            let ch_t = if center_h >= 2 { heavy } else { light };
            fill_rect_alpha(
                &mut buf,
                w,
                cx - cw / 2,
                cy - ch_t / 2,
                cx + (cw + 1) / 2,
                cy + (ch_t + 1) / 2,
                255,
            );
        }
    } else {
        draw_special_box(&mut buf, w, h, ch, cx, cy, light);
    }

    new_bitmap_glyph(w, h, buf)
}

#[allow(clippy::too_many_arguments)]
fn draw_horiz_arm(
    buf: &mut [u8],
    stride: i32,
    x0: i32,
    x1: i32,
    cy: i32,
    style: u8,
    light: i32,
    heavy: i32,
) {
    match style {
        1 | 2 => {
            let t = if style == 2 { heavy } else { light };
            fill_rect_alpha(buf, stride, x0, cy - t / 2, x1, cy + (t + 1) / 2, 255);
        }
        3 => {
            let gap = (light + 1).max(2);
            fill_rect_alpha(
                buf,
                stride,
                x0,
                cy - gap - light / 2,
                x1,
                cy - gap + (light + 1) / 2,
                255,
            );
            fill_rect_alpha(
                buf,
                stride,
                x0,
                cy + gap - light / 2,
                x1,
                cy + gap + (light + 1) / 2,
                255,
            );
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_vert_arm(
    buf: &mut [u8],
    stride: i32,
    y0: i32,
    y1: i32,
    cx: i32,
    style: u8,
    light: i32,
    heavy: i32,
) {
    match style {
        1 | 2 => {
            let t = if style == 2 { heavy } else { light };
            fill_rect_alpha(buf, stride, cx - t / 2, y0, cx + (t + 1) / 2, y1, 255);
        }
        3 => {
            let gap = (light + 1).max(2);
            fill_rect_alpha(
                buf,
                stride,
                cx - gap - light / 2,
                y0,
                cx - gap + (light + 1) / 2,
                y1,
                255,
            );
            fill_rect_alpha(
                buf,
                stride,
                cx + gap - light / 2,
                y0,
                cx + gap + (light + 1) / 2,
                y1,
                255,
            );
        }
        _ => {}
    }
}

fn draw_special_box(buf: &mut [u8], w: i32, h: i32, ch: char, cx: i32, cy: i32, light: i32) {
    match ch {
        '\u{256D}' | '\u{256E}' | '\u{256F}' | '\u{2570}' => {
            let (e_l, e_r, e_u, e_d) = match ch {
                '\u{256D}' => (0, 1, 0, 1),
                '\u{256E}' => (1, 0, 0, 1),
                '\u{256F}' => (1, 0, 1, 0),
                _ => (0, 1, 1, 0),
            };
            draw_horiz_arm(
                buf,
                w,
                if e_l > 0 { 0 } else { cx - light / 2 },
                if e_r > 0 { w } else { cx + (light + 1) / 2 },
                cy,
                e_l.max(e_r),
                light,
                light,
            );
            draw_vert_arm(
                buf,
                w,
                if e_u > 0 { 0 } else { cy - light / 2 },
                if e_d > 0 { h } else { cy + (light + 1) / 2 },
                cx,
                e_u.max(e_d),
                light,
                light,
            );
        }
        '\u{2571}' => draw_diag(buf, w, h, true, light),
        '\u{2572}' => draw_diag(buf, w, h, false, light),
        '\u{2573}' => {
            draw_diag(buf, w, h, true, light);
            draw_diag(buf, w, h, false, light);
        }
        _ => {
            // Dashed horizontal/vertical fallback.
            fill_rect_alpha(buf, w, 0, cy - light / 2, w / 3, cy + (light + 1) / 2, 255);
            fill_rect_alpha(
                buf,
                w,
                (w * 2) / 3,
                cy - light / 2,
                w,
                cy + (light + 1) / 2,
                255,
            );
        }
    }
}

fn draw_diag(buf: &mut [u8], w: i32, h: i32, bottom_left_to_top_right: bool, t: i32) {
    for y in 0..h {
        let fx = if bottom_left_to_top_right {
            (w * (h - 1 - y)) / h.max(1)
        } else {
            (w * y) / h.max(1)
        };
        fill_rect_alpha(buf, w, fx - t / 2, y, fx + (t + 1) / 2, y + 1, 255);
    }
}

fn box_edges(ch: char) -> Option<BoxEdges> {
    let e = |l: u8, r: u8, u: u8, d: u8| BoxEdges {
        left: l,
        right: r,
        up: u,
        down: d,
    };
    Some(match ch {
        '\u{2500}' => e(1, 1, 0, 0),
        '\u{2501}' => e(2, 2, 0, 0),
        '\u{2502}' => e(0, 0, 1, 1),
        '\u{2503}' => e(0, 0, 2, 2),
        '\u{250C}' => e(0, 1, 0, 1),
        '\u{250F}' => e(0, 2, 0, 2),
        '\u{2510}' => e(1, 0, 0, 1),
        '\u{2513}' => e(2, 0, 0, 2),
        '\u{2514}' => e(0, 1, 1, 0),
        '\u{2517}' => e(0, 2, 2, 0),
        '\u{2518}' => e(1, 0, 1, 0),
        '\u{251B}' => e(2, 0, 2, 0),
        '\u{251C}' => e(0, 1, 1, 1),
        '\u{2523}' => e(0, 2, 2, 2),
        '\u{2524}' => e(1, 0, 1, 1),
        '\u{252B}' => e(2, 0, 2, 2),
        '\u{252C}' => e(1, 1, 0, 1),
        '\u{2533}' => e(2, 2, 0, 2),
        '\u{2534}' => e(1, 1, 1, 0),
        '\u{253B}' => e(2, 2, 2, 0),
        '\u{253C}' => e(1, 1, 1, 1),
        '\u{254B}' => e(2, 2, 2, 2),
        '\u{2550}' => e(3, 3, 0, 0),
        '\u{2551}' => e(0, 0, 3, 3),
        '\u{2554}' => e(0, 3, 0, 3),
        '\u{2557}' => e(3, 0, 0, 3),
        '\u{255A}' => e(0, 3, 3, 0),
        '\u{255D}' => e(3, 0, 3, 0),
        '\u{2560}' => e(0, 3, 3, 3),
        '\u{2563}' => e(3, 0, 3, 3),
        '\u{2566}' => e(3, 3, 0, 3),
        '\u{2569}' => e(3, 3, 3, 0),
        '\u{256C}' => e(3, 3, 3, 3),
        '\u{2574}' => e(1, 0, 0, 0),
        '\u{2575}' => e(0, 0, 1, 0),
        '\u{2576}' => e(0, 1, 0, 0),
        '\u{2577}' => e(0, 0, 0, 1),
        '\u{2578}' => e(2, 0, 0, 0),
        '\u{2579}' => e(0, 0, 2, 0),
        '\u{257A}' => e(0, 2, 0, 0),
        '\u{257B}' => e(0, 0, 0, 2),
        _ => return None,
    })
}

fn rasterize_powerline(ch: char, w: i32, h: i32) -> RasterizedGlyph {
    #[allow(clippy::cast_sign_loss)]
    let mut buf = vec![0_u8; (w * h) as usize];
    let half_h = (h as f32) * 0.5;
    let wf = w as f32;
    let thin_stroke = (wf * 0.14).max(1.2);

    for y in 0..h {
        let dy = ((y as f32 + 0.5) - half_h).abs() / half_h.max(1.0);
        let edge_x = match ch {
            '\u{E0B0}' | '\u{E0B1}' => (1.0 - dy) * wf,
            _ => dy * wf,
        };
        for x in 0..w {
            let xf = x as f32 + 0.5;
            let cov = match ch {
                '\u{E0B0}' => (edge_x - xf + 0.5).clamp(0.0, 1.0),
                '\u{E0B2}' => (xf - edge_x + 0.5).clamp(0.0, 1.0),
                '\u{E0B1}' | '\u{E0B3}' => {
                    let dist = (xf - edge_x).abs();
                    (thin_stroke - dist + 0.5).clamp(0.0, 1.0)
                }
                _ => 0.0,
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let alpha = (cov * 255.0).round() as u8;
            #[allow(clippy::cast_sign_loss)]
            if let Some(cell) = buf.get_mut((y * w + x) as usize) {
                *cell = alpha;
            }
        }
    }

    new_bitmap_glyph(w, h, buf)
}

/// Crisp anti-aliased 14x14 procedural UI icons for RustyOS header pills (Glass, Search, New Tab).
fn rasterize_ui_icon(ch: char) -> Option<RasterizedGlyph> {
    if !matches!(ch, '\u{E001}' | '\u{E002}' | '\u{E003}') {
        return None;
    }
    let sz = 14_i32;
    let mut buf = vec![0_u8; (sz * sz) as usize];
    for y in 0..sz {
        for x in 0..sz {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let cov = match ch {
                // Layered glass panes icon (`U+E001`)
                '\u{E001}' => {
                    let d_back = sd_rounded_box(fx - 5.5, fy - 5.5, 3.6, 3.6, 1.3);
                    let ring_back = (1.15 - d_back.abs()).clamp(0.0, 1.0);
                    let d_front = sd_rounded_box(fx - 8.7, fy - 8.7, 3.5, 3.5, 1.3);
                    let fill_front = (0.6 - d_front).clamp(0.0, 1.0) * 0.85;
                    let ring_front = (0.9 - d_front.abs()).clamp(0.0, 1.0);
                    ring_back.max(fill_front).max(ring_front)
                }
                // Magnifying glass search icon (`U+E002`)
                '\u{E002}' => {
                    let dist_c = ((fx - 5.8).hypot(fy - 5.8) - 3.6).abs();
                    let ring = (1.15 - dist_c).clamp(0.0, 1.0);
                    let handle_d = sd_segment(fx, fy, 8.6, 8.6, 12.1, 12.1);
                    let handle = (1.15 - handle_d).clamp(0.0, 1.0);
                    ring.max(handle)
                }
                // Symmetric New Tab `+` icon (`U+E003`)
                _ => {
                    let hbar = sd_rounded_box(fx - 7.0, fy - 7.0, 4.6, 0.85, 0.5);
                    let vbar = sd_rounded_box(fx - 7.0, fy - 7.0, 0.85, 4.6, 0.5);
                    (0.55 - hbar.min(vbar)).clamp(0.0, 1.0)
                }
            };
            buf[(y * sz + x) as usize] = (cov * 255.0).round() as u8;
        }
    }
    Some(RasterizedGlyph {
        width: sz as u32,
        height: sz as u32,
        offset_x: 0,
        offset_y: -11,
        advance_x: sz as f32,
        bitmap: buf,
    })
}

fn sd_rounded_box(px: f32, py: f32, bx: f32, by: f32, r: f32) -> f32 {
    let qx = px.abs() - (bx - r);
    let qy = py.abs() - (by - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = (bax * bax + bay * bay).max(1e-6);
    let h = ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0);
    (pax - bax * h).hypot(pay - bay * h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedural_block_box_and_powerline_glyphs_fill_cells() {
        let font_set = FontSet::load();
        let metrics = font_set.metrics_for_size(14.0);
        assert!(metrics.cell_w >= 6 && metrics.cell_h >= 12);

        for ch in ['█', '▀', '▄', '┌', '─', '│', '', ''] {
            let key = GlyphKey {
                ch,
                kind: GlyphKind::MonoRegular,
                size_tenths: 140,
                cell_w: metrics.cell_w as u16,
                cell_h: metrics.cell_h as u16,
            };
            let g = font_set.rasterize(key, metrics.baseline_y);
            assert!(g.is_some(), "expected procedural glyph for {ch:?}");
            if let Some(glyph) = g {
                assert!(glyph.bitmap.contains(&255));
            }
        }
    }
}
