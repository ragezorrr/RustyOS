//! Adaptive `TopBar` contrast analysis from the wallpaper's top strip (`RUSTYOS-141`, ADR-0082).
//!
//! Computes mean relative luminance and texture detail across the top `bar_height`
//! rows of a BGRA wallpaper buffer so the `TopBar` automatically selects crisp
//! light or dark text/icons and an optional protective scrim on busy textures.

/// Detail threshold above which a wallpaper strip is treated as visually busy
/// (foliage, rock, fine texture) rather than smooth (sky, water, flat color).
const BUSY_DETAIL_THRESHOLD: f64 = 8.0;
/// On busy strips, light foreground remains legible up to a higher average luminance
/// once lifted by a subtle dark scrim.
const BUSY_LIGHT_LUMINANCE_MAX: f64 = 0.45;
/// Smooth backgrounds brighter than this use dark foreground without any scrim.
const PALE_LUMINANCE_MIN: f64 = 0.60;

/// Adaptive visual style computed from the wallpaper strip behind the `TopBar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarBackdropStyle {
    /// True when the `TopBar` should render light (white) text and glyphs.
    pub prefer_light_text: bool,
    /// Optional subtle scrim alpha (`0..=96`) to lift text contrast over busy textures.
    pub scrim_alpha: u8,
    /// True when the scrim should be dark (`[0, 0, 0, scrim_alpha]`), false for light scrim.
    pub dark_scrim: bool,
}

impl Default for BarBackdropStyle {
    fn default() -> Self {
        Self {
            prefer_light_text: true,
            scrim_alpha: 0,
            dark_scrim: true,
        }
    }
}

/// Computes sRGB relative luminance (`0.0..=1.0`) per WCAG 2.1.
#[must_use]
pub fn relative_luminance(red: f64, green: f64, blue: f64) -> f64 {
    let linear = |v: f64| {
        let s = (v / 255.0).clamp(0.0, 1.0);
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
}

/// Returns true if white text achieves at least 3:1 contrast against `luminance`.
#[must_use]
pub fn prefers_light_foreground(luminance: f64) -> bool {
    1.05 / (luminance.max(0.0) + 0.05) >= 3.0
}

fn bgra_luma(b: u8, g: u8, r: u8) -> i32 {
    (i32::from(r) * 11 + i32::from(g) * 16 + i32::from(b) * 5) / 32
}

/// Analyzes the top `bar_height` rows of a premultiplied/opaque BGRA wallpaper buffer.
///
/// Subsamples horizontally to at most 256 samples for O(1) cost (<0.1 ms) when
/// loading or switching wallpapers.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn analyze_top_bar_backdrop_bgra(
    pixels_bgra: &[u8],
    width: usize,
    height: usize,
    bar_height: usize,
) -> BarBackdropStyle {
    if width == 0
        || height == 0
        || pixels_bgra.len() < width.saturating_mul(height).saturating_mul(4)
    {
        return BarBackdropStyle::default();
    }
    let rows = bar_height.clamp(1, height);
    let step_x = (width / 256).max(1);
    let step_y = (rows / 16).max(1);

    let mut sum_r: u64 = 0;
    let mut sum_g: u64 = 0;
    let mut sum_b: u64 = 0;
    let mut count: u64 = 0;
    let mut detail_sum: u64 = 0;
    let mut detail_count: u64 = 0;

    let mut prev_row: Vec<i32> = Vec::new();
    for y in (0..rows).step_by(step_y) {
        let mut cur_row: Vec<i32> = Vec::with_capacity(width / step_x + 1);
        let mut prev_luma: Option<i32> = None;
        for (col_idx, x) in (0..width).step_by(step_x).enumerate() {
            let off = (y * width + x) * 4;
            let b = pixels_bgra[off];
            let g = pixels_bgra[off + 1];
            let r = pixels_bgra[off + 2];
            sum_r += u64::from(r);
            sum_g += u64::from(g);
            sum_b += u64::from(b);
            count += 1;

            let luma = bgra_luma(b, g, r);
            if let Some(pl) = prev_luma {
                detail_sum += u64::from(luma.abs_diff(pl));
                detail_count += 1;
            }
            if let Some(&above) = prev_row.get(col_idx) {
                detail_sum += u64::from(luma.abs_diff(above));
                detail_count += 1;
            }
            prev_luma = Some(luma);
            cur_row.push(luma);
        }
        prev_row = cur_row;
    }

    if count == 0 {
        return BarBackdropStyle::default();
    }

    let mean_r = sum_r as f64 / count as f64;
    let mean_g = sum_g as f64 / count as f64;
    let mean_b = sum_b as f64 / count as f64;
    let lum = relative_luminance(mean_r, mean_g, mean_b);
    let detail = if detail_count > 0 {
        detail_sum as f64 / detail_count as f64
    } else {
        0.0
    };
    let busy = detail >= BUSY_DETAIL_THRESHOLD;

    if busy {
        if lum <= BUSY_LIGHT_LUMINANCE_MAX {
            BarBackdropStyle {
                prefer_light_text: true,
                scrim_alpha: 64,
                dark_scrim: true,
            }
        } else {
            BarBackdropStyle {
                prefer_light_text: false,
                scrim_alpha: 56,
                dark_scrim: false,
            }
        }
    } else if prefers_light_foreground(lum) {
        BarBackdropStyle {
            prefer_light_text: true,
            scrim_alpha: 0,
            dark_scrim: true,
        }
    } else if lum >= PALE_LUMINANCE_MIN {
        BarBackdropStyle {
            prefer_light_text: false,
            scrim_alpha: 0,
            dark_scrim: false,
        }
    } else {
        BarBackdropStyle {
            prefer_light_text: false,
            scrim_alpha: 32,
            dark_scrim: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_bgra(width: usize, height: usize, blue: u8, green: u8, red: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(width * height * 4);
        for _ in 0..(width * height) {
            out.extend_from_slice(&[blue, green, red, 255]);
        }
        out
    }

    #[test]
    fn dark_and_bright_wallpapers_pick_opposite_foregrounds() {
        let dark = solid_bgra(128, 48, 24, 24, 28);
        let dark_style = analyze_top_bar_backdrop_bgra(&dark, 128, 48, 28);
        assert!(dark_style.prefer_light_text);
        assert_eq!(dark_style.scrim_alpha, 0);

        let bright = solid_bgra(128, 48, 245, 245, 245);
        let bright_style = analyze_top_bar_backdrop_bgra(&bright, 128, 48, 28);
        assert!(!bright_style.prefer_light_text);
        assert_eq!(bright_style.scrim_alpha, 0);
    }

    #[test]
    fn busy_strip_enables_protective_scrim() {
        let mut busy = Vec::with_capacity(128 * 32 * 4);
        for y in 0..32 {
            for x in 0..128 {
                let v = if (x + y) % 2 == 0 { 10 } else { 150 };
                busy.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let style = analyze_top_bar_backdrop_bgra(&busy, 128, 32, 28);
        assert!(style.scrim_alpha > 0);
    }
}
