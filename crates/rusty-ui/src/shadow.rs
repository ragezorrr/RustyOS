//! Unified 3-lobe analytical Gaussian drop shadow model.
//!
//! Provides mathematically standardized 3-lobe Gaussian shadows matching the
//! desktop design system across both CPU canvas rasterization (`UiCanvas`) and
//! compositor GPU shaders.
//!
//! Each shadow elevation consists of three physical Gaussian lobes:
//! 1. **Wide Ambient Lobe:** Diffuse ambient falloff with radius `wide`, vertical offset `offset`,
//!    and peak opacity `0.22 * strength`.
//! 2. **Narrow Directional Lobe:** Directional shading with radius `narrow`, vertical offset `(offset + 1) / 2`,
//!    and peak opacity `0.15 * strength`.
//! 3. **Contact Lobe:** Micro-contrast edge with radius `1.5 px`, vertical offset `1.0 px`,
//!    and peak opacity `0.16 * strength * contact`.

use crate::canvas::UiCanvas;
use crate::geometry::Rect;
use crate::theme::ThemePalette;

/// Standardized desktop shadow elevation level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShadowElevation {
    /// Active focused window.
    ActiveWindow,
    /// Inactive background window.
    InactiveWindow,
    /// Context menus, top-bar menus, and tooltips.
    MenuOrTooltip,
    /// Modal dialogs, action cards, and drop-down popovers.
    Popup,
    /// Desktop floating Dock (diffuse ambient float without contact line).
    Dock,
    /// No shadow.
    None,
}

/// A single Gaussian lobe of a drop shadow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowLobe {
    /// Gaussian standard deviation / radius in logical pixels.
    pub sigma: f32,
    /// Translation offset `(dx, dy)` in logical pixels.
    pub offset: (f32, f32),
    /// Peak opacity factor (0.0..=1.0).
    pub opacity: f32,
}

/// A composite 3-lobe Gaussian shadow style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowStyle {
    /// The three Gaussian lobes: `[wide, narrow, contact]`.
    pub lobes: [ShadowLobe; 3],
}

impl ShadowStyle {
    /// Constructs a 3-lobe shadow specification.
    #[must_use]
    #[allow(clippy::manual_midpoint)]
    pub const fn new(wide: f32, narrow: f32, offset: f32, strength: f32, contact: f32) -> Self {
        Self {
            lobes: [
                ShadowLobe {
                    sigma: wide,
                    offset: (0.0, offset),
                    opacity: 0.22 * strength,
                },
                ShadowLobe {
                    sigma: narrow,
                    offset: (0.0, (offset + 1.0) * 0.5),
                    opacity: 0.15 * strength,
                },
                ShadowLobe {
                    sigma: 1.5,
                    offset: (0.0, 1.0),
                    opacity: 0.16 * strength * contact,
                },
            ],
        }
    }

    /// Resolves the standardized shadow style for a given elevation.
    #[must_use]
    pub const fn for_elevation(elevation: ShadowElevation) -> Self {
        match elevation {
            ShadowElevation::None => Self {
                lobes: [
                    ShadowLobe {
                        sigma: 0.0,
                        offset: (0.0, 0.0),
                        opacity: 0.0,
                    },
                    ShadowLobe {
                        sigma: 0.0,
                        offset: (0.0, 0.0),
                        opacity: 0.0,
                    },
                    ShadowLobe {
                        sigma: 0.0,
                        offset: (0.0, 0.0),
                        opacity: 0.0,
                    },
                ],
            },
            ShadowElevation::Dock => Self::new(10.0, 4.0, 3.0, 0.25, 0.0),
            ShadowElevation::Popup => Self::new(8.0, 4.0, 3.0, 0.70, 1.0),
            ShadowElevation::MenuOrTooltip => Self::new(14.0, 6.0, 8.0, 0.85, 1.0),
            ShadowElevation::InactiveWindow => Self::new(24.0, 9.0, 14.0, 0.55, 1.0),
            ShadowElevation::ActiveWindow => Self::new(24.0, 9.0, 14.0, 1.00, 1.0),
        }
    }

    /// Maximum reach of the shadow past the bounding box in logical pixels.
    #[must_use]
    pub fn reach(&self) -> f32 {
        let mut max_reach = 0.0f32;
        for lobe in &self.lobes {
            if lobe.opacity > 0.0 {
                let r = lobe.sigma * 3.0 + lobe.offset.1.abs();
                if r > max_reach {
                    max_reach = r;
                }
            }
        }
        max_reach
    }
}

/// Renders a 3-lobe Gaussian shadow around `rect` onto a CPU [`UiCanvas`].
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn draw_elevation_shadow(
    canvas: &mut UiCanvas,
    rect: Rect,
    radius: u32,
    elevation: ShadowElevation,
    pal: &ThemePalette,
) {
    if elevation == ShadowElevation::None {
        return;
    }
    let style = ShadowStyle::for_elevation(elevation);
    let shadow_color = if pal.is_dark {
        [0u8, 0u8, 0u8]
    } else {
        [15u8, 20u8, 30u8]
    };

    // Render each active lobe through multi-pass Gaussian layer approximations
    for lobe in &style.lobes {
        if lobe.opacity <= 0.001 {
            continue;
        }

        let sigma = lobe.sigma;
        let dy = lobe.offset.1.round() as i32;
        let base_alpha = (lobe.opacity * 255.0).clamp(0.0, 255.0);

        if sigma <= 2.0 {
            // Tight contact lobe: 2 sharp subpixel passes
            for (step, alpha_mult) in [(0, 0.65f32), (1, 0.35f32)] {
                let g = step;
                let a = (base_alpha * alpha_mult).round().clamp(1.0, 255.0) as u8;
                let target_rect = Rect::new(
                    rect.x - g,
                    rect.y - g + dy,
                    rect.width + (g * 2) as u32,
                    rect.height + (g * 2) as u32,
                );
                canvas.fill_rounded_rect(
                    target_rect,
                    radius + g as u32,
                    [shadow_color[0], shadow_color[1], shadow_color[2], a],
                );
            }
        } else {
            // Diffuse lobe: 3-4 Gaussian slice steps approximating e^(-x^2 / 2sigma^2)
            let steps = if sigma > 12.0 { 4 } else { 3 };
            let max_spread = (sigma * 1.5).round() as i32;
            for i in 1..=steps {
                let frac = i as f32 / steps as f32;
                let spread = (max_spread as f32 * frac).round() as i32;
                let weight = (-2.0 * frac * frac).exp() * (1.0 / steps as f32);
                let a = (base_alpha * weight * 1.4).round().clamp(1.0, 255.0) as u8;

                let target_rect = Rect::new(
                    rect.x - spread,
                    rect.y - spread + dy,
                    rect.width + (spread * 2) as u32,
                    rect.height + (spread * 2) as u32,
                );
                canvas.fill_rounded_rect(
                    target_rect,
                    radius + spread as u32,
                    [shadow_color[0], shadow_color[1], shadow_color[2], a],
                );
            }
        }
    }
}
