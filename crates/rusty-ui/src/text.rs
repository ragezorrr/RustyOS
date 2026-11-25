//! Shared `Inter` font loading, glyph rasterization cache, measurement, and caret math (`RUSTYOS-130`).

use std::collections::HashMap;

use fontdue::{Font, FontSettings};

use crate::canvas::UiCanvas;
use crate::geometry::Rect;

/// Bundled `Inter Regular` (`OFL-1.1`) TrueType font bytes (`RUSTYOS-130`, `RUSTYOS-147`).
pub const INTER_REGULAR_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
/// Bundled `Inter SemiBold` (`OFL-1.1`) TrueType font bytes (`RUSTYOS-130`, `RUSTYOS-147`).
pub const INTER_SEMIBOLD_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
/// Bundled `Inter Bold` (`OFL-1.1`) TrueType font bytes (`RUSTYOS-147`).
pub const INTER_BOLD_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-Bold.ttf");
/// Bundled `JetBrains Mono Regular` (`OFL-1.1`) TrueType font bytes (`RUSTYOS-147`).
pub const JETBRAINS_MONO_REGULAR_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

/// Font weight selector for UI text rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    SemiBold,
    Bold,
    Mono,
}

/// Cache key identifying one rasterized glyph bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphRasterKey {
    pub weight: FontWeight,
    pub size_tenths: u16,
    pub ch: char,
}

/// Cached alpha-mask glyph metrics and coverage bitmap.
#[derive(Debug, Clone)]
pub struct RasterizedGlyph {
    pub width: usize,
    pub height: usize,
    pub xmin: i32,
    pub ymin: i32,
    pub advance_x: f32,
    pub coverage: Vec<u8>,
}

/// Shared UI font rasterizer and text metrics engine backed by bundled `Inter` fonts.
pub struct UiFontEngine {
    regular: Option<Font>,
    semibold: Option<Font>,
    bold: Option<Font>,
    mono: Option<Font>,
    glyphs: HashMap<GlyphRasterKey, RasterizedGlyph>,
    empty_glyph: RasterizedGlyph,
}

impl std::fmt::Debug for UiFontEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiFontEngine")
            .field("cached_glyphs", &self.glyphs.len())
            .finish_non_exhaustive()
    }
}

impl Default for UiFontEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl UiFontEngine {
    /// Creates a new font engine with bundled `Inter` and `JetBrains Mono` fonts.
    #[must_use]
    pub fn new() -> Self {
        let regular = Font::from_bytes(INTER_REGULAR_BYTES, FontSettings::default()).ok();
        let semibold = Font::from_bytes(INTER_SEMIBOLD_BYTES, FontSettings::default())
            .ok()
            .or_else(|| regular.clone());
        let bold = Font::from_bytes(INTER_BOLD_BYTES, FontSettings::default())
            .ok()
            .or_else(|| semibold.clone());
        let mono = Font::from_bytes(JETBRAINS_MONO_REGULAR_BYTES, FontSettings::default())
            .ok()
            .or_else(|| regular.clone());
        Self {
            regular,
            semibold,
            bold,
            mono,
            glyphs: HashMap::with_capacity(512),
            empty_glyph: RasterizedGlyph {
                width: 0,
                height: 0,
                xmin: 0,
                ymin: 0,
                advance_x: 0.0,
                coverage: Vec::new(),
            },
        }
    }

    #[inline]
    fn size_tenths(size_px: f32) -> u16 {
        (size_px.clamp(6.0, 200.0) * 10.0).round() as u16
    }

    /// Returns a reference to the cached rasterized glyph for `(ch, size_px, bold)`.
    pub fn rasterize_glyph(&mut self, ch: char, size_px: f32, bold: bool) -> &RasterizedGlyph {
        let weight = if bold {
            FontWeight::SemiBold
        } else {
            FontWeight::Regular
        };
        self.rasterize_glyph_weighted(ch, size_px, weight)
    }

    /// Returns a reference to the cached rasterized glyph for `(ch, size_px, weight)`.
    pub fn rasterize_glyph_weighted(
        &mut self,
        ch: char,
        size_px: f32,
        weight: FontWeight,
    ) -> &RasterizedGlyph {
        let key = GlyphRasterKey {
            weight,
            size_tenths: Self::size_tenths(size_px),
            ch,
        };
        if !self.glyphs.contains_key(&key) {
            let font_opt = match weight {
                FontWeight::Regular => self.regular.as_ref(),
                FontWeight::SemiBold => self.semibold.as_ref(),
                FontWeight::Bold => self.bold.as_ref(),
                FontWeight::Mono => self.mono.as_ref(),
            };
            let Some(font) = font_opt else {
                return &self.empty_glyph;
            };
            let px = f32::from(key.size_tenths) / 10.0;
            let (metrics, coverage) = font.rasterize(ch, px);
            self.glyphs.insert(
                key,
                RasterizedGlyph {
                    width: metrics.width,
                    height: metrics.height,
                    xmin: metrics.xmin,
                    ymin: metrics.ymin,
                    advance_x: metrics.advance_width,
                    coverage,
                },
            );
        }
        &self.glyphs[&key]
    }

    /// Computes horizontal advance width of `text` in pixels.
    #[must_use]
    pub fn measure_width(&mut self, text: &str, size_px: f32, bold: bool) -> f32 {
        let mut w = 0.0_f32;
        for ch in text.chars() {
            w += self.rasterize_glyph(ch, size_px, bold).advance_x;
        }
        w
    }

    /// Computes horizontal pixel offset from text start to UTF-8 `byte_idx`.
    #[must_use]
    pub fn x_for_byte(&mut self, text: &str, byte_idx: usize, size_px: f32, bold: bool) -> f32 {
        let clamped = clamp_char_boundary(text, byte_idx);
        let mut x = 0.0_f32;
        for (idx, ch) in text.char_indices() {
            if idx >= clamped {
                break;
            }
            x += self.rasterize_glyph(ch, size_px, bold).advance_x;
        }
        x
    }

    /// Finds the nearest UTF-8 character boundary corresponding to pixel offset `target_x`.
    #[must_use]
    pub fn byte_for_x(&mut self, text: &str, target_x: f32, size_px: f32, bold: bool) -> usize {
        if target_x <= 0.0 {
            return 0;
        }
        let mut x = 0.0_f32;
        for (idx, ch) in text.char_indices() {
            let adv = self.rasterize_glyph(ch, size_px, bold).advance_x;
            if target_x < x + adv * 0.5 {
                return idx;
            }
            x += adv;
        }
        text.len()
    }

    /// Truncates `text` with trailing ellipsis (`…`) so its rendered width fits within `max_width`.
    #[must_use]
    pub fn ellipsize(&mut self, text: &str, max_width: f32, size_px: f32, bold: bool) -> String {
        if max_width <= 0.0 || text.is_empty() {
            return String::new();
        }
        if self.measure_width(text, size_px, bold) <= max_width {
            return text.to_owned();
        }
        let ellipsis_w = self.measure_width("…", size_px, bold);
        let budget = (max_width - ellipsis_w).max(0.0);
        let mut used = 0.0_f32;
        let mut end = 0_usize;
        for (idx, ch) in text.char_indices() {
            let adv = self.rasterize_glyph(ch, size_px, bold).advance_x;
            if used + adv > budget {
                break;
            }
            used += adv;
            end = idx + ch.len_utf8();
        }
        let mut out = String::with_capacity(end + 3);
        out.push_str(&text[..end]);
        out.push('…');
        out
    }

    /// Renders `text` onto a CPU [`UiCanvas`] with optional clipping rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_on_canvas(
        &mut self,
        canvas: &mut UiCanvas<'_>,
        text: &str,
        x: f32,
        y_baseline: f32,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
        clip: Option<Rect>,
    ) {
        if color[3] == 0 || text.is_empty() {
            return;
        }
        let clip_rect = clip.unwrap_or_else(|| Rect::new(0, 0, canvas.width, canvas.height));
        if clip_rect.width == 0 || clip_rect.height == 0 {
            return;
        }

        let mut cursor_x = x;
        for ch in text.chars() {
            let glyph = self.rasterize_glyph(ch, size_px, bold).clone();
            if cursor_x + glyph.xmin as f32 + glyph.width as f32 >= clip_rect.x as f32
                && cursor_x + (glyph.xmin as f32) < clip_rect.right() as f32
            {
                let gx0 = cursor_x.round() as i32 + glyph.xmin;
                let gy0 = y_baseline.round() as i32 - glyph.ymin - glyph.height as i32;
                blit_glyph_to_canvas(canvas, &glyph, gx0, gy0, color, clip_rect);
            }
            cursor_x += glyph.advance_x;
        }
    }

    /// Renders `text` centered inside `rect` on a CPU [`UiCanvas`].
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_centered_on_canvas(
        &mut self,
        canvas: &mut UiCanvas<'_>,
        text: &str,
        rect: Rect,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
        clip: Option<Rect>,
    ) {
        let tw = self.measure_width(text, size_px, bold);
        let tx = rect.x as f32 + (rect.width as f32 - tw) * 0.5;
        let baseline = rect.y as f32 + f32::midpoint(rect.height as f32, size_px * 0.72);
        self.draw_text_on_canvas(canvas, text, tx, baseline, size_px, bold, color, clip);
    }
}

/// Blits one rasterized glyph coverage bitmap onto [`UiCanvas`] within `clip`.
fn blit_glyph_to_canvas(
    canvas: &mut UiCanvas<'_>,
    glyph: &RasterizedGlyph,
    gx0: i32,
    gy0: i32,
    color: [u8; 4],
    clip: Rect,
) {
    let base_alpha = u32::from(color[3]);
    let min_x = clip.x.max(0);
    let min_y = clip.y.max(0);
    let max_x = clip.right().min(canvas.width as i32);
    let max_y = clip.bottom().min(canvas.height as i32);

    for row in 0..glyph.height {
        let py = gy0 + row as i32;
        if py < min_y || py >= max_y {
            continue;
        }
        let row_base = row * glyph.width;
        for col in 0..glyph.width {
            let px = gx0 + col as i32;
            if px < min_x || px >= max_x {
                continue;
            }
            let cov = u32::from(glyph.coverage[row_base + col]);
            if cov == 0 {
                continue;
            }
            let a = ((cov * base_alpha + 127) / 255) as u8;
            if a > 0 {
                canvas.blend_pixel(px, py, [color[0], color[1], color[2], a]);
            }
        }
    }
}

/// Clamps `byte_idx` down to the nearest valid UTF-8 character boundary within `text`.
#[must_use]
pub fn clamp_char_boundary(text: &str, byte_idx: usize) -> usize {
    let mut idx = byte_idx.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Returns `(start_byte, end_byte)` of the word surrounding `byte_idx` for double-click selection.
#[must_use]
pub fn word_bounds_at(text: &str, byte_idx: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let idx = clamp_char_boundary(text, byte_idx.min(text.len().saturating_sub(1)));
    let is_word_char = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '-';

    let anchor_is_word = text[idx..].chars().next().is_some_and(is_word_char);

    let mut start = idx;
    for (ci, ch) in text[..idx].char_indices().rev() {
        if is_word_char(ch) != anchor_is_word && ch.is_whitespace() == anchor_is_word {
            break;
        }
        if is_word_char(ch) == anchor_is_word {
            start = ci;
        } else {
            break;
        }
    }

    let mut end = idx;
    for (ci, ch) in text[idx..].char_indices() {
        if is_word_char(ch) != anchor_is_word {
            break;
        }
        end = idx + ci + ch.len_utf8();
    }
    if start == end && idx < text.len() {
        if let Some(ch) = text[idx..].chars().next() {
            end = idx + ch.len_utf8();
        }
    }
    (start, end)
}

/// Returns the start byte of the previous word before `cursor` (`Ctrl+Left` / `Ctrl+Backspace`).
#[must_use]
pub fn prev_word_boundary(text: &str, cursor: usize) -> usize {
    let cur = clamp_char_boundary(text, cursor);
    let mut iter = text[..cur].char_indices().rev().peekable();
    while let Some(&(_, ch)) = iter.peek() {
        if !ch.is_whitespace() {
            break;
        }
        iter.next();
    }
    let mut last_idx = 0;
    let first_is_word = iter
        .peek()
        .is_some_and(|&(_, ch)| ch.is_alphanumeric() || ch == '_');
    for (idx, ch) in iter {
        let is_word = ch.is_alphanumeric() || ch == '_';
        if ch.is_whitespace() || is_word != first_is_word {
            break;
        }
        last_idx = idx;
    }
    last_idx
}

/// Returns the end byte of the next word after `cursor` (`Ctrl+Right` / `Ctrl+Delete`).
#[must_use]
pub fn next_word_boundary(text: &str, cursor: usize) -> usize {
    let cur = clamp_char_boundary(text, cursor);
    let mut iter = text[cur..].char_indices().peekable();
    while let Some(&(_, ch)) = iter.peek() {
        if !ch.is_whitespace() {
            break;
        }
        iter.next();
    }
    let Some(&(_, first)) = iter.peek() else {
        return text.len();
    };
    let first_is_word = first.is_alphanumeric() || first == '_';
    let mut end = cur;
    for (rel_idx, ch) in iter {
        let is_word = ch.is_alphanumeric() || ch == '_';
        if ch.is_whitespace() || is_word != first_is_word {
            break;
        }
        end = cur + rel_idx + ch.len_utf8();
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_engine_supports_bold_and_mono_weights() {
        let mut engine = UiFontEngine::new();
        let bold_a = engine.rasterize_glyph_weighted('A', 14.0, FontWeight::Bold);
        assert!(bold_a.width > 0 && bold_a.height > 0);
        let adv_i = engine
            .rasterize_glyph_weighted('i', 14.0, FontWeight::Mono)
            .advance_x;
        let adv_w = engine
            .rasterize_glyph_weighted('W', 14.0, FontWeight::Mono)
            .advance_x;
        assert!((adv_i - adv_w).abs() < 0.01);
    }
}
