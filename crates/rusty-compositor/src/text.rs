//! CPU glyph rasterizer and GPU-cached label renderer using `fontdue` and bundled `Inter` (`RUSTYOS-020`, `RUSTYOS-049`).
//!
//! Rasterizes crisp anti-aliased labels into Smithay [`MemoryRenderBuffer`] textures on first
//! draw and reuses the uploaded GPU textures on subsequent frames for minimal overhead.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};
use tracing::{info, warn};

const BUNDLED_INTER_REGULAR: &[u8] = rusty_ui::text::INTER_REGULAR_BYTES;
const BUNDLED_INTER_SEMIBOLD: &[u8] = rusty_ui::text::INTER_BOLD_BYTES;

const REGULAR_FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/inter/Inter-Regular.ttf",
    "/usr/share/fonts/TTF/Inter-Regular.ttf",
    "/usr/share/fonts/otf/Inter-Regular.otf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf",
];

const BOLD_FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/inter/Inter-Bold.ttf",
    "/usr/share/fonts/inter/Inter-SemiBold.ttf",
    "/usr/share/fonts/TTF/Inter-SemiBold.ttf",
    "/usr/share/fonts/TTF/Inter-Bold.ttf",
    "/usr/share/fonts/noto/NotoSans-SemiBold.ttf",
    "/usr/share/fonts/noto/NotoSans-Bold.ttf",
];

/// Typographic font weight for compositor shell labels (`RUSTYOS-049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    /// Clean Inter Regular weight (menu items, clock, body copy).
    #[default]
    Regular,
    /// Crisp Inter SemiBold/Bold weight (active application title in Menu Bar, headings).
    Bold,
}

const TEXT_CACHE_PRUNE_THRESHOLD: usize = 256;
const TEXT_CACHE_HARD_CAP: usize = 512;
const TEXT_CACHE_MAX_AGE_FRAMES: u64 = 1800;

#[derive(Debug)]
struct CachedLabelVariant {
    size_tenths: u16,
    color: [u8; 4],
    weight: FontWeight,
    buffer_scale: i32,
    buffer: MemoryRenderBuffer,
    last_frame: u64,
}

struct LoadedFonts {
    regular: Option<Font>,
    bold: Option<Font>,
}

static SHARED_FONTS: OnceLock<LoadedFonts> = OnceLock::new();

fn shared_fonts() -> &'static LoadedFonts {
    SHARED_FONTS.get_or_init(|| LoadedFonts {
        regular: load_regular_font(),
        bold: load_bold_font(),
    })
}

/// Measures logical pixel advance width of `text` using the active UI font (`Inter`) (`RUSTYOS-049`).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn measure_ui_text_width(text: &str, size_px: f32, weight: FontWeight) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let fonts = shared_fonts();
    let font = match weight {
        FontWeight::Bold => fonts.bold.as_ref().or(fonts.regular.as_ref()),
        FontWeight::Regular => fonts.regular.as_ref(),
    };
    let Some(font) = font else {
        #[allow(clippy::cast_possible_wrap)]
        return (text.chars().count() as i32) * 7 + 4;
    };
    let mut width_f = 0.0_f32;
    for ch in text.chars() {
        width_f += font.metrics(ch, size_px).advance_width;
    }
    (width_f.ceil() as i32).max(1) + 2
}

/// GPU-backed label texture cache powered by `fontdue` and bundled `Inter`.
pub struct TextCache {
    fonts: &'static LoadedFonts,
    buffers: HashMap<String, Vec<CachedLabelVariant>>,
    frame_counter: u64,
    active_scale: i32,
}

impl fmt::Debug for TextCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant_count: usize = self.buffers.values().map(Vec::len).sum();
        f.debug_struct("TextCache")
            .field("font_loaded", &self.fonts.regular.is_some())
            .field("bold_font_loaded", &self.fonts.bold.is_some())
            .field("cached_labels", &variant_count)
            .field("frame_counter", &self.frame_counter)
            .field("active_scale", &self.active_scale)
            .finish()
    }
}

impl Default for TextCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TextCache {
    /// Initializes the font rasterizer from `RUSTYOS_FONT_PATH`, system `Inter`, or bundled `Inter` OFL assets.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fonts: shared_fonts(),
            buffers: HashMap::new(),
            frame_counter: 0,
            active_scale: 1,
        }
    }

    /// Sets the integer backing scale (`1..=4`) used to rasterize labels sharply on `HiDPI` outputs.
    #[allow(clippy::cast_possible_truncation)]
    pub fn set_active_scale(&mut self, scale: f64) {
        self.active_scale = scale.ceil().clamp(1.0, 4.0) as i32;
    }

    /// Advances the cache frame generation and prunes stale glyph buffers (`RUSTYOS-119`).
    pub fn begin_frame(&mut self) {
        self.frame_counter = self.frame_counter.saturating_add(1);
        let total: usize = self.buffers.values().map(Vec::len).sum();
        if total <= TEXT_CACHE_PRUNE_THRESHOLD {
            return;
        }
        if total > TEXT_CACHE_HARD_CAP || self.frame_counter.is_multiple_of(64) {
            let cutoff = self.frame_counter.saturating_sub(TEXT_CACHE_MAX_AGE_FRAMES);
            self.buffers.retain(|_, variants| {
                variants.retain(|v| v.last_frame >= cutoff);
                !variants.is_empty()
            });
        }
    }

    /// Returns total cached label variants across strings (used in tests/telemetry).
    #[must_use]
    pub fn cached_variant_count(&self) -> usize {
        self.buffers.values().map(Vec::len).sum()
    }

    /// Returns `true` if a TrueType/OpenType font was successfully loaded.
    #[must_use]
    pub const fn has_font(&self) -> bool {
        self.fonts.regular.is_some()
    }

    fn font_for_weight(&self, weight: FontWeight) -> Option<&Font> {
        match weight {
            FontWeight::Regular => self.fonts.regular.as_ref(),
            FontWeight::Bold => self.fonts.bold.as_ref().or(self.fonts.regular.as_ref()),
        }
    }

    /// Measures logical pixel `(width, height)` for `text` at `size_px` using [`FontWeight::Regular`].
    #[must_use]
    pub fn measure_text(&self, text: &str, size_px: f32) -> (i32, i32) {
        self.measure_text_weighted(text, FontWeight::Regular, size_px)
    }

    /// Measures logical pixel `(width, height)` for `text` at `size_px` with `weight` (`RUSTYOS-049`).
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn measure_text_weighted(
        &self,
        text: &str,
        weight: FontWeight,
        size_px: f32,
    ) -> (i32, i32) {
        let Some(font) = self.font_for_weight(weight) else {
            return (0, 0);
        };
        if text.is_empty() {
            return (0, 0);
        }
        // Use regular font metrics for shared line box height so Bold & Regular share one optical baseline.
        let metric_font = self.fonts.regular.as_ref().unwrap_or(font);
        let line_metrics = metric_font.horizontal_line_metrics(size_px);
        let ascent = line_metrics.map_or(size_px * 0.8, |m| m.ascent);
        let descent = line_metrics.map_or(-size_px * 0.2, |m| m.descent);
        let height = (ascent - descent).ceil().max(size_px) as i32 + 2;

        let mut width_f = 0.0f32;
        for ch in text.chars() {
            width_f += font.metrics(ch, size_px).advance_width;
        }
        ((width_f.ceil() as i32).max(1) + 2, height.max(1))
    }

    /// Renders `text` at `(loc.x, loc.y)` in logical coordinates using [`FontWeight::Regular`].
    pub fn render_label(
        &mut self,
        renderer: &mut GlesRenderer,
        text: &str,
        size_px: f32,
        rgba: [u8; 4],
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        self.render_label_weighted(renderer, text, FontWeight::Regular, size_px, rgba, loc)
    }

    /// Renders `text` at `(loc.x, loc.y)` in logical coordinates with explicit `weight` (`RUSTYOS-049`).
    pub fn render_label_weighted(
        &mut self,
        renderer: &mut GlesRenderer,
        text: &str,
        weight: FontWeight,
        size_px: f32,
        rgba: [u8; 4],
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if text.is_empty() {
            return None;
        }
        let size_tenths = size_tenths_key(size_px);
        let buffer_scale = self.active_scale.clamp(1, 4);
        let frame = self.frame_counter;
        if let Some(vars) = self.buffers.get_mut(text)
            && let Some(v) = vars.iter_mut().find(|v| {
                v.size_tenths == size_tenths
                    && v.color == rgba
                    && v.weight == weight
                    && v.buffer_scale == buffer_scale
            })
        {
            v.last_frame = frame;
            return element_from_label_buffer(renderer, &v.buffer, loc);
        }

        let buf = self.rasterize_and_cache_label(text, weight, size_px, rgba, buffer_scale)?;
        element_from_label_buffer(renderer, buf, loc)
    }

    fn rasterize_and_cache_label(
        &mut self,
        text: &str,
        weight: FontWeight,
        size_px: f32,
        rgba: [u8; 4],
        buffer_scale: i32,
    ) -> Option<&MemoryRenderBuffer> {
        let font = self.font_for_weight(weight)?;
        let metric_font = self.fonts.regular.as_ref().unwrap_or(font);
        let (buffer, _size) =
            rasterize_label_buffer(font, metric_font, text, size_px, rgba, buffer_scale)?;
        let frame = self.frame_counter;
        let vars = self.buffers.entry(text.to_owned()).or_default();
        vars.push(CachedLabelVariant {
            size_tenths: size_tenths_key(size_px),
            color: rgba,
            weight,
            buffer_scale,
            buffer,
            last_frame: frame,
        });
        vars.last().map(|v| &v.buffer)
    }

    #[cfg(test)]
    fn cache_or_touch_for_test(&mut self, text: &str, size_px: f32, rgba: [u8; 4]) -> bool {
        let size_tenths = size_tenths_key(size_px);
        let buffer_scale = self.active_scale.clamp(1, 4);
        let frame = self.frame_counter;
        if let Some(vars) = self.buffers.get_mut(text)
            && let Some(v) = vars.iter_mut().find(|v| {
                v.size_tenths == size_tenths
                    && v.color == rgba
                    && v.weight == FontWeight::Regular
                    && v.buffer_scale == buffer_scale
            })
        {
            v.last_frame = frame;
            return true;
        }
        self.rasterize_and_cache_label(text, FontWeight::Regular, size_px, rgba, buffer_scale)
            .is_some()
    }

    /// Renders `text` centered inside `bounds` (logical rectangle) using [`FontWeight::Regular`].
    pub fn render_label_centered(
        &mut self,
        renderer: &mut GlesRenderer,
        text: &str,
        size_px: f32,
        rgba: [u8; 4],
        bounds: Rectangle<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        self.render_label_centered_weighted(
            renderer,
            text,
            FontWeight::Regular,
            size_px,
            rgba,
            bounds,
        )
    }

    /// Renders `text` centered inside `bounds` (logical rectangle) with explicit `weight` (`RUSTYOS-049`).
    pub fn render_label_centered_weighted(
        &mut self,
        renderer: &mut GlesRenderer,
        text: &str,
        weight: FontWeight,
        size_px: f32,
        rgba: [u8; 4],
        bounds: Rectangle<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let (w, h) = self.measure_text_weighted(text, weight, size_px);
        if w == 0 || h == 0 {
            return None;
        }
        let x = bounds.loc.x + (bounds.size.w - w) / 2;
        let y = bounds.loc.y + (bounds.size.h - h) / 2;
        self.render_label_weighted(renderer, text, weight, size_px, rgba, Point::from((x, y)))
    }
}

fn load_regular_font() -> Option<Font> {
    if let Ok(env_path) = std::env::var("RUSTYOS_FONT_PATH")
        && let Ok(bytes) = fs::read(&env_path)
        && let Ok(font) = Font::from_bytes(bytes, FontSettings::default())
    {
        info!(path = %env_path, "Loaded system font from RUSTYOS_FONT_PATH");
        return Some(font);
    }

    if let Ok(font) = Font::from_bytes(BUNDLED_INTER_REGULAR, FontSettings::default()) {
        info!("Loaded bundled Inter Regular UI font");
        return Some(font);
    }

    for path in REGULAR_FONT_CANDIDATES {
        if Path::new(path).exists()
            && let Ok(bytes) = fs::read(path)
            && let Ok(font) = Font::from_bytes(bytes, FontSettings::default())
        {
            info!(path = %path, "Loaded fallback UI font");
            return Some(font);
        }
    }

    warn!("No UI font found; UI text labels will be omitted");
    None
}

fn load_bold_font() -> Option<Font> {
    if let Ok(font) = Font::from_bytes(BUNDLED_INTER_SEMIBOLD, FontSettings::default()) {
        info!("Loaded bundled Inter SemiBold UI font");
        return Some(font);
    }

    for path in BOLD_FONT_CANDIDATES {
        if Path::new(path).exists()
            && let Ok(bytes) = fs::read(path)
            && let Ok(font) = Font::from_bytes(bytes, FontSettings::default())
        {
            info!(path = %path, "Loaded fallback Bold UI font");
            return Some(font);
        }
    }

    None
}

/// Rasterizes a UTF-8 string into an isolated `Argb8888` [`MemoryRenderBuffer`].
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap
)]
fn rasterize_label_buffer(
    font: &Font,
    metric_font: &Font,
    text: &str,
    size_px: f32,
    rgba: [u8; 4],
    buffer_scale: i32,
) -> Option<(MemoryRenderBuffer, Size<i32, Logical>)> {
    let scale_i = buffer_scale.clamp(1, 4);
    let scale_f = scale_i as f32;
    let scaled_px = size_px * scale_f;
    let line_metrics = metric_font.horizontal_line_metrics(scaled_px);
    let ascent = line_metrics.map_or(scaled_px * 0.8, |m| m.ascent);
    let descent = line_metrics.map_or(-scaled_px * 0.2, |m| m.descent);
    let raw_height = ((ascent - descent).ceil().max(scaled_px) as i32 + 2 * scale_i).max(scale_i);
    let height = ((raw_height + scale_i - 1) / scale_i) * scale_i;
    let baseline_y = ascent.ceil() + scale_f;

    let mut glyphs = Vec::with_capacity(text.len());
    let mut cursor_x = scale_f;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, scaled_px);
        glyphs.push((cursor_x, metrics, bitmap));
        cursor_x += metrics.advance_width;
    }

    let raw_width = (cursor_x.ceil() as i32 + scale_i).max(scale_i);
    let width = ((raw_width + scale_i - 1) / scale_i) * scale_i;
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (width, height),
        scale_i,
        Transform::Normal,
        None,
    );

    let [r, g, b, a] = rgba;
    let mut ctx = mem_buf.render();
    let draw_res = ctx.draw(|pixels| {
        pixels.fill(0);
        let w_usize = width as usize;
        let h_usize = height as usize;

        for (gx, metrics, bitmap) in &glyphs {
            if metrics.width == 0 || metrics.height == 0 {
                continue;
            }
            let start_x = (gx + metrics.xmin as f32).round() as i32;
            let start_y = (baseline_y - metrics.ymin as f32 - metrics.height as f32).round() as i32;

            for row in 0..metrics.height {
                let dst_y = start_y + row as i32;
                if dst_y < 0 || dst_y >= h_usize as i32 {
                    continue;
                }
                for col in 0..metrics.width {
                    let dst_x = start_x + col as i32;
                    if dst_x < 0 || dst_x >= w_usize as i32 {
                        continue;
                    }
                    let cov = bitmap[row * metrics.width + col];
                    if cov == 0 {
                        continue;
                    }
                    let idx = (dst_y as usize * w_usize + dst_x as usize) * 4;
                    if idx + 4 > pixels.len() {
                        continue;
                    }
                    // Premultiplied Argb8888 little-endian: [B, G, R, A]
                    let alpha = (u16::from(cov) * u16::from(a)) / 255;
                    let pb = ((u16::from(b) * alpha) / 255) as u8;
                    let pg = ((u16::from(g) * alpha) / 255) as u8;
                    let pr = ((u16::from(r) * alpha) / 255) as u8;
                    let pa = alpha as u8;

                    // Alpha-over blend in case adjacent glyphs overlap slightly
                    let inv_a = 255 - u16::from(pa);
                    pixels[idx] = pb.saturating_add(((u16::from(pixels[idx]) * inv_a) / 255) as u8);
                    pixels[idx + 1] =
                        pg.saturating_add(((u16::from(pixels[idx + 1]) * inv_a) / 255) as u8);
                    pixels[idx + 2] =
                        pr.saturating_add(((u16::from(pixels[idx + 2]) * inv_a) / 255) as u8);
                    pixels[idx + 3] =
                        pa.saturating_add(((u16::from(pixels[idx + 3]) * inv_a) / 255) as u8);
                }
            }
        }

        Result::<_, ()>::Ok(vec![Rectangle::from_size((width, height).into())])
    });

    if draw_res.is_err() {
        return None;
    }
    drop(ctx);

    Some((mem_buf, Size::from((width, height))))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn size_tenths_key(size_px: f32) -> u16 {
    (size_px.max(1.0) * 10.0).round() as u16
}

fn element_from_label_buffer(
    renderer: &mut GlesRenderer,
    buffer: &MemoryRenderBuffer,
    loc: Point<i32, Logical>,
) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
    let phys_loc = loc.to_physical_precise_round(1.0);
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        phys_loc,
        buffer,
        None,
        None,
        None,
        Kind::Unspecified,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_cache_loads_system_font_or_degrades_gracefully() {
        let cache = TextCache::new();
        assert!(cache.has_font(), "Bundled Inter font must always load");
        let (w_reg, h_reg) = cache.measure_text_weighted("RustyOS", FontWeight::Regular, 13.0);
        let (w_bold, h_bold) = cache.measure_text_weighted("RustyOS", FontWeight::Bold, 13.0);
        assert!(w_reg > 10);
        assert!(w_bold >= w_reg);
        assert_eq!(h_reg, h_bold);
        assert!(h_reg >= 13);
        assert!(measure_ui_text_width("File", 13.0, FontWeight::Regular) > 10);
    }

    #[test]
    fn text_cache_evicts_stale_labels_while_keeping_active_entries() {
        let mut cache = TextCache::new();
        let white = [255, 255, 255, 255];
        assert!(cache.cache_or_touch_for_test("Active Dock Label", 13.0, white));
        for i in 0..300 {
            let s = format!("stale-label-{i}");
            assert!(cache.cache_or_touch_for_test(&s, 13.0, white));
        }
        assert_eq!(cache.cached_variant_count(), 301);

        for f in 1..=1920 {
            cache.begin_frame();
            if f == 1500 {
                assert!(cache.cache_or_touch_for_test("Active Dock Label", 13.0, white));
            }
        }
        assert_eq!(cache.cached_variant_count(), 1);
        assert!(cache.buffers.contains_key("Active Dock Label"));
    }

    #[test]
    fn text_cache_rasterizes_distinct_hidpi_variants_by_scale() {
        let mut cache = TextCache::new();
        let white = [255, 255, 255, 255];

        cache.set_active_scale(1.0);
        assert!(cache.cache_or_touch_for_test("HiDPI Label", 14.0, white));
        assert_eq!(cache.cached_variant_count(), 1);

        cache.set_active_scale(2.0);
        assert!(cache.cache_or_touch_for_test("HiDPI Label", 14.0, white));
        assert_eq!(cache.cached_variant_count(), 2);

        assert!(cache.buffers.get("HiDPI Label").is_some_and(|vars| {
            vars.iter().any(|v| v.buffer_scale == 1) && vars.iter().any(|v| v.buffer_scale == 2)
        }));
    }
}
