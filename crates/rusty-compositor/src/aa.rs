//! Shared anti-aliasing helpers for procedural icons and glyphs (`RUSTYOS-111`).
//!
//! - [`R2_TAPS_16`]: 16-tap low-discrepancy `R2` sequence (`a1 = 0.7548776662`, `a2 = 0.5698402910`).
//!   Unlike an orthogonal `4x4` grid the taps never share rows or columns, so near-horizontal and
//!   near-vertical edges resolve into 16 distinct coverage levels instead of 4 (no visible stair steps).
//! - [`LinearAccum`]: accumulates sub-samples in linear light and returns premultiplied `BGRA`.

/// 16 `R2` sub-pixel offsets inside a pixel (`x`, `y` in `[0, 1)`).
pub(crate) const R2_TAPS_16: [(f32, f32); 16] = [
    (0.25488, 0.06984),
    (0.00976, 0.63968),
    (0.76463, 0.20952),
    (0.51951, 0.77936),
    (0.27439, 0.34920),
    (0.02927, 0.91904),
    (0.78414, 0.48888),
    (0.53902, 0.05872),
    (0.29390, 0.62856),
    (0.04878, 0.19840),
    (0.80365, 0.76824),
    (0.55853, 0.33808),
    (0.31341, 0.90792),
    (0.06829, 0.47776),
    (0.82316, 0.04760),
    (0.57804, 0.61744),
];

/// Converts an sRGB-encoded channel in `[0, 1]` to linear light.
#[must_use]
pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts a linear-light channel in `[0, 1]` to sRGB encoding.
#[must_use]
pub(crate) fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Sub-sample accumulator averaging colors in linear light (`16` taps).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct LinearAccum {
    rgb_lin: [f32; 3],
    alpha: f32,
}

impl LinearAccum {
    /// Adds one sample with straight (non-premultiplied) `0..=255` color and alpha `0.0..=1.0`.
    pub(crate) fn add_straight(&mut self, rgb: [f32; 3], alpha: f32) {
        let a = alpha.clamp(0.0, 1.0);
        if a <= 1e-5 {
            return;
        }
        for (acc, c) in self.rgb_lin.iter_mut().zip(rgb) {
            *acc += srgb_to_linear(c / 255.0) * a;
        }
        self.alpha += a;
    }

    /// Adds one sample with premultiplied `0..=255` color and alpha `0.0..=1.0`.
    pub(crate) fn add_premul(&mut self, rgb_premul: [f32; 3], alpha: f32) {
        let a = alpha.clamp(0.0, 1.0);
        if a <= 1e-5 {
            return;
        }
        self.add_straight([rgb_premul[0] / a, rgb_premul[1] / a, rgb_premul[2] / a], a);
    }

    /// Returns premultiplied `Argb8888` bytes (`[B, G, R, A]`) averaged over [`R2_TAPS_16`].
    #[must_use]
    pub(crate) fn finish_bgra(self) -> [u8; 4] {
        let avg_a = (self.alpha / 16.0).clamp(0.0, 1.0);
        if avg_a <= 0.001 {
            return [0, 0, 0, 0];
        }
        let inv = 1.0 / (16.0 * avg_a);
        let enc = |lin_sum: f32| -> u8 {
            let straight = linear_to_srgb(lin_sum * inv);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let out = (straight * avg_a * 255.0).clamp(0.0, 255.0).round() as u8;
            out
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let a8 = (avg_a * 255.0).clamp(0.0, 255.0).round() as u8;
        [
            enc(self.rgb_lin[2]),
            enc(self.rgb_lin[1]),
            enc(self.rgb_lin[0]),
            a8,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r2_taps_are_distinct_in_both_axes() {
        for (i, a) in R2_TAPS_16.iter().enumerate() {
            for b in R2_TAPS_16.iter().skip(i + 1) {
                assert!((a.0 - b.0).abs() > 1e-3);
                assert!((a.1 - b.1).abs() > 1e-3);
            }
            assert!((0.0..1.0).contains(&a.0));
            assert!((0.0..1.0).contains(&a.1));
        }
    }

    #[test]
    fn linear_accum_preserves_constant_color_and_coverage() {
        let mut acc = LinearAccum::default();
        for _ in 0..16 {
            acc.add_straight([0.0, 122.0, 255.0], 1.0);
        }
        let px = acc.finish_bgra();
        assert_eq!(px, [255, 122, 0, 255]);

        let mut half = LinearAccum::default();
        for _ in 0..8 {
            half.add_straight([200.0, 200.0, 200.0], 1.0);
        }
        let hp = half.finish_bgra();
        assert_eq!(hp[3], 128);
        assert!((i32::from(hp[0]) - 100).abs() <= 1);
        assert_eq!(LinearAccum::default().finish_bgra(), [0, 0, 0, 0]);
    }
}
