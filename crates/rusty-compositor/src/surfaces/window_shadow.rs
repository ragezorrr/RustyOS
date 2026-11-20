//! Seamless analytical GLES SDF drop shadows (`RUSTYOS-075`).
//!
//! Renders single-pass continuous contact + vertically offset ambient Gaussian
//! shadows with screen-space 4x4 ordered dithering using Smithay's safe
//! `GlesTexProgram` + `TextureShaderElement` (`unsafe_code = "deny"`), replacing
//! multi-pass offset strips and discrete `Solid` gradient bands.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::gles::element::TextureShaderElement;
use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType, UniformValue,
};
use smithay::backend::renderer::{ImportMem, Renderer};
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};
use tracing::warn;

use super::LiquidGlassElement;

const MAX_SHADOW_SLOTS: usize = 256;

/// GLSL ES 1.00 fragment shader for seamless macOS Golden Gate rounded-box drop shadows.
pub(crate) const WINDOW_SHADOW_FRAG: &str = r"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

uniform vec4 u_bounds_win;
uniform vec4 u_win_geom;
uniform vec4 u_shadow_contact;
uniform vec4 u_shadow_rgb;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

float sd_rounded_box(vec2 p, vec2 half_ext, float r) {
    vec2 q = abs(p) - half_ext;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 bounds_size = max(u_bounds_win.xy, vec2(1.0));
    vec2 win_size = max(u_win_geom.xy, vec2(1.0));
    float radius = clamp(u_win_geom.z, 0.0, min(win_size.x, win_size.y) * 0.5);
    vec2 half_ext = max(win_size * 0.5 - vec2(radius), vec2(0.0));

    vec2 frag_px = v_coords * bounds_size;
    vec2 win_center = u_bounds_win.zw + win_size * 0.5;
    vec2 p_win = frag_px - win_center;
    float sd_win = sd_rounded_box(p_win, half_ext, radius);

    // Mask out interior strictly so shadow never darkens translucent client/glass interiors.
    float outside_mask = smoothstep(-0.75, 0.35, sd_win);
    if (outside_mask <= 0.001) {
        gl_FragColor = vec4(0.0);
        return;
    }

    float d_c = max(sd_win, 0.0);
    float norm_c = clamp(d_c / max(u_shadow_contact.y, 1.0), 0.0, 1.0);
    float gauss_c = exp(-(norm_c * norm_c) * 4.5) * smoothstep(1.0, 0.80, norm_c);

    // Ambient drop shifts downward continuously across the bottom boundary (zero +5/+12 seam).
    vec2 p_amb = p_win - vec2(0.0, u_win_geom.w);
    float sd_amb = sd_rounded_box(p_amb, half_ext, radius);
    float d_a = max(sd_amb, 0.0);
    float norm_a = clamp(d_a / max(u_shadow_contact.w, 1.0), 0.0, 1.0);
    float gauss_a = exp(-(norm_a * norm_a) * 3.8) * smoothstep(1.0, 0.72, norm_a);

    float shadow_a = clamp(
        u_shadow_contact.x * gauss_c + u_shadow_contact.z * gauss_a,
        0.0,
        1.0
    );

    vec2 cell = mod(floor(gl_FragCoord.xy), 4.0);
    float checker = mod(cell.x * 3.0 + cell.y * 5.0 + mod(cell.x + cell.y, 2.0) * 2.0, 16.0);
    float dither = (checker / 16.0 - 0.5) / 255.0;

    float out_alpha = clamp((shadow_a + dither) * outside_mask * alpha, 0.0, 1.0);
    vec4 texel = texture2D(tex, clamp(v_coords, 0.0, 1.0));
#if defined(NO_ALPHA)
    texel.a = 1.0;
#endif
    vec3 col = u_shadow_rgb.rgb * texel.a;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0) {
        col = vec3(0.0, 0.2, 0.0) + col * 0.8;
    }
#endif

    gl_FragColor = vec4(col * out_alpha, out_alpha);
}
";

/// Analytical contact + ambient Gaussian drop shadow specification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WindowShadowSpec {
    pub win_rect: Rectangle<i32, Logical>,
    pub radius: i32,
    pub drop_y: f32,
    pub contact_alpha: f32,
    pub contact_spread: f32,
    pub amb_alpha: f32,
    pub amb_spread: f32,
    pub shadow_rgb: [u8; 3],
}

impl WindowShadowSpec {
    /// Standard active/inactive window shadow preset (`RUSTYOS-075`, `RUSTYOS-080`).
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn for_window(
        win_rect: Rectangle<i32, Logical>,
        radius: i32,
        is_focused: bool,
    ) -> Self {
        Self::for_window_mode(
            win_rect,
            radius,
            is_focused,
            crate::config::WindowShadowMode::Subtle,
        )
    }

    /// Standardized 3-lobe Gaussian shadow specification for a desktop elevation (`RUSTYOS-126`).
    #[must_use]
    pub(crate) fn for_elevation(
        win_rect: Rectangle<i32, Logical>,
        radius: i32,
        elevation: rusty_ui::shadow::ShadowElevation,
        shadow_rgb: [u8; 3],
    ) -> Self {
        let style = rusty_ui::shadow::ShadowStyle::for_elevation(elevation);
        let wide_lobe = style.lobes[0];
        let narrow_lobe = style.lobes[1];
        let contact_lobe = style.lobes[2];
        Self {
            win_rect,
            radius,
            drop_y: wide_lobe.offset.1,
            contact_alpha: narrow_lobe.opacity + contact_lobe.opacity,
            contact_spread: narrow_lobe.sigma,
            amb_alpha: wide_lobe.opacity,
            amb_spread: wide_lobe.sigma,
            shadow_rgb,
        }
    }

    /// Standard active/inactive window shadow preset for a specific mode (`RUSTYOS-080`, `RUSTYOS-126`).
    #[must_use]
    pub(crate) fn for_window_mode(
        win_rect: Rectangle<i32, Logical>,
        radius: i32,
        is_focused: bool,
        mode: crate::config::WindowShadowMode,
    ) -> Self {
        let elevation = if is_focused {
            rusty_ui::shadow::ShadowElevation::ActiveWindow
        } else {
            rusty_ui::shadow::ShadowElevation::InactiveWindow
        };
        let mut spec = Self::for_elevation(win_rect, radius, elevation, [6, 8, 14]);
        if mode == crate::config::WindowShadowMode::Subtle {
            spec.amb_alpha *= 0.85;
            spec.contact_alpha *= 0.85;
        }
        spec
    }

    /// Unified shell popover/card shadow preset derived from `spread` and `max_alpha`.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn for_panel(
        win_rect: Rectangle<i32, Logical>,
        radius: i32,
        spread: i32,
        shadow_rgb: [u8; 3],
        max_alpha: u8,
    ) -> Self {
        let max_a = f32::from(max_alpha) / 255.0;
        let sp = spread.max(1) as f32;
        Self {
            win_rect,
            radius,
            drop_y: (sp * 0.16).min(4.0),
            contact_alpha: max_a * 0.36,
            contact_spread: (sp * 0.45).max(3.5),
            amb_alpha: max_a * 0.64,
            amb_spread: sp,
            shadow_rgb,
        }
    }

    /// Computes outer shadow bounding box `(bounds, pad_left, pad_top)` in logical pixels.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn bounds_and_offsets(&self) -> (Rectangle<i32, Logical>, i32, i32) {
        let pad_x = self.amb_spread.max(self.contact_spread).ceil().max(1.0) as i32;
        let pad_top = (self.amb_spread - self.drop_y)
            .max(self.contact_spread)
            .ceil()
            .max(1.0) as i32;
        let pad_bot = (self.amb_spread + self.drop_y)
            .max(self.contact_spread)
            .ceil()
            .max(1.0) as i32;
        let bounds = Rectangle::new(
            Point::from((self.win_rect.loc.x - pad_x, self.win_rect.loc.y - pad_top)),
            Size::from((
                self.win_rect.size.w.max(1) + 2 * pad_x,
                self.win_rect.size.h.max(1) + pad_top + pad_bot,
            )),
        );
        (bounds, pad_x, pad_top)
    }

    fn params_hash(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.win_rect.size.w.hash(&mut h);
        self.win_rect.size.h.hash(&mut h);
        self.radius.hash(&mut h);
        self.drop_y.to_bits().hash(&mut h);
        self.contact_alpha.to_bits().hash(&mut h);
        self.contact_spread.to_bits().hash(&mut h);
        self.amb_alpha.to_bits().hash(&mut h);
        self.amb_spread.to_bits().hash(&mut h);
        self.shadow_rgb.hash(&mut h);
        h.finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ShadowSlotKey {
    output_id: u32,
    loc_x: i32,
    loc_y: i32,
    width: i32,
    height: i32,
    occ: u16,
}

#[derive(Debug)]
struct CachedShadowSlot {
    id: Id,
    params_hash: u64,
    last_frame: u64,
}

/// Manages GLES SDF drop shadow shader compilation, 1x1 carrier texture, and stable element IDs.
#[derive(Default)]
pub(crate) struct WindowShadowPipeline {
    program: Option<GlesTexProgram>,
    dummy_tex: Option<GlesTexture>,
    compile_failed: bool,
    active_output_id: u32,
    frame_counter: u64,
    frame_slot_counts: HashMap<(u32, i32, i32, i32, i32), u16>,
    slots: HashMap<ShadowSlotKey, CachedShadowSlot>,
}

impl std::fmt::Debug for WindowShadowPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowShadowPipeline")
            .field("has_program", &self.program.is_some())
            .field("has_dummy_tex", &self.dummy_tex.is_some())
            .field("compile_failed", &self.compile_failed)
            .field("slot_count", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl WindowShadowPipeline {
    pub(crate) fn begin_frame(&mut self, output_id: u32) {
        self.active_output_id = output_id;
        self.frame_counter = self.frame_counter.wrapping_add(1);
        self.frame_slot_counts.clear();
        if self.slots.len() > MAX_SHADOW_SLOTS {
            let frame_gen = self.frame_counter;
            self.slots
                .retain(|_, slot| frame_gen.saturating_sub(slot.last_frame) <= 4);
        }
    }

    fn ensure_resources(&mut self, renderer: &mut GlesRenderer) -> bool {
        if self.program.is_some() && self.dummy_tex.is_some() {
            return true;
        }
        if self.compile_failed {
            return false;
        }
        if self.program.is_none() {
            let uniforms = [
                UniformName::new("u_bounds_win", UniformType::_4f),
                UniformName::new("u_win_geom", UniformType::_4f),
                UniformName::new("u_shadow_contact", UniformType::_4f),
                UniformName::new("u_shadow_rgb", UniformType::_4f),
            ];
            match renderer.compile_custom_texture_shader(WINDOW_SHADOW_FRAG, &uniforms) {
                Ok(prog) => self.program = Some(prog),
                Err(err) => {
                    warn!("Failed to compile window shadow GLES shader ({err}); using CPU 9-slice");
                    self.compile_failed = true;
                    return false;
                }
            }
        }
        if self.dummy_tex.is_none() {
            match renderer.import_memory(
                &[255, 255, 255, 255],
                Fourcc::Abgr8888,
                Size::from((1, 1)),
                false,
            ) {
                Ok(tex) => self.dummy_tex = Some(tex),
                Err(err) => {
                    warn!(
                        "Failed to allocate 1x1 shadow carrier texture ({err}); using CPU 9-slice"
                    );
                    self.compile_failed = true;
                    return false;
                }
            }
        }
        true
    }

    /// Builds a single hardware-accelerated seamless shadow [`LiquidGlassElement`].
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn make_shadow_element(
        &mut self,
        renderer: &mut GlesRenderer,
        spec: WindowShadowSpec,
    ) -> Option<LiquidGlassElement> {
        if spec.win_rect.size.w <= 0
            || spec.win_rect.size.h <= 0
            || spec.amb_spread <= 0.0
            || !self.ensure_resources(renderer)
        {
            return None;
        }
        let program = self.program.as_ref()?.clone();
        let tex = self.dummy_tex.as_ref()?.clone();

        let (bounds, pad_x, pad_top) = spec.bounds_and_offsets();
        let params_hash = spec.params_hash();
        let stable_id = self.resolve_slot_id(spec.win_rect, params_hash);

        let uniforms = vec![
            Uniform::new(
                "u_bounds_win",
                UniformValue::_4f(
                    bounds.size.w as f32,
                    bounds.size.h as f32,
                    pad_x as f32,
                    pad_top as f32,
                ),
            ),
            Uniform::new(
                "u_win_geom",
                UniformValue::_4f(
                    spec.win_rect.size.w as f32,
                    spec.win_rect.size.h as f32,
                    spec.radius.max(0) as f32,
                    spec.drop_y,
                ),
            ),
            Uniform::new(
                "u_shadow_contact",
                UniformValue::_4f(
                    spec.contact_alpha,
                    spec.contact_spread,
                    spec.amb_alpha,
                    spec.amb_spread,
                ),
            ),
            Uniform::new(
                "u_shadow_rgb",
                UniformValue::_4f(
                    f32::from(spec.shadow_rgb[0]) / 255.0,
                    f32::from(spec.shadow_rgb[1]) / 255.0,
                    f32::from(spec.shadow_rgb[2]) / 255.0,
                    1.0,
                ),
            ),
        ];

        let inner = TextureRenderElement::from_static_texture(
            stable_id.clone(),
            renderer.context_id(),
            Point::from((f64::from(bounds.loc.x), f64::from(bounds.loc.y))),
            tex,
            1,
            Transform::Normal,
            Some(1.0),
            Some(Rectangle::from_size(Size::from((1.0, 1.0)))),
            Some(bounds.size),
            None,
            Kind::Unspecified,
        );
        let shader = TextureShaderElement::new(inner, program, uniforms);
        Some(LiquidGlassElement::from_parts(stable_id, shader))
    }

    fn resolve_slot_id(&mut self, win_rect: Rectangle<i32, Logical>, params_hash: u64) -> Id {
        let base = (
            self.active_output_id,
            win_rect.loc.x,
            win_rect.loc.y,
            win_rect.size.w,
            win_rect.size.h,
        );
        let occ = {
            let entry = self.frame_slot_counts.entry(base).or_insert(0);
            let cur = *entry;
            *entry = entry.saturating_add(1);
            cur
        };
        let key = ShadowSlotKey {
            output_id: base.0,
            loc_x: base.1,
            loc_y: base.2,
            width: base.3,
            height: base.4,
            occ,
        };
        let frame_gen = self.frame_counter;
        match self.slots.get_mut(&key) {
            Some(slot) if slot.params_hash == params_hash => {
                slot.last_frame = frame_gen;
                slot.id.clone()
            }
            Some(slot) => {
                slot.id = Id::new();
                slot.params_hash = params_hash;
                slot.last_frame = frame_gen;
                slot.id.clone()
            }
            None => {
                let id = Id::new();
                self.slots.insert(
                    key,
                    CachedShadowSlot {
                        id: id.clone(),
                        params_hash,
                        last_frame: frame_gen,
                    },
                );
                id
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_shadow_shader_satisfies_smithay_contract() {
        assert!(WINDOW_SHADOW_FRAG.starts_with("#version 100\n"));
        assert!(WINDOW_SHADOW_FRAG.contains("//_DEFINES_"));
        assert!(WINDOW_SHADOW_FRAG.contains("#if defined(EXTERNAL)"));
        assert!(WINDOW_SHADOW_FRAG.contains("#if defined(NO_ALPHA)"));
        assert!(WINDOW_SHADOW_FRAG.contains("#if defined(DEBUG_FLAGS)"));
        assert!(WINDOW_SHADOW_FRAG.contains("texture2D("));
        for uniform in [
            "u_bounds_win",
            "u_win_geom",
            "u_shadow_contact",
            "u_shadow_rgb",
        ] {
            assert!(WINDOW_SHADOW_FRAG.contains(uniform));
        }
    }

    #[test]
    fn focused_and_unfocused_shadow_bounds_cover_vertical_drop_without_clipping() {
        let win = Rectangle::new((100, 120).into(), (800, 500).into());
        let focused = WindowShadowSpec::for_window(win, 20, true);
        let unfocused = WindowShadowSpec::for_window(win, 20, false);
        let (fb, fpx, fpt) = focused.bounds_and_offsets();
        let (ub, upx, upt) = unfocused.bounds_and_offsets();
        assert_eq!(fpx, 24);
        assert_eq!(fpt, 10);
        assert_eq!(upx, 24);
        assert_eq!(upt, 10);
        assert_eq!(fb.loc, Point::from((76, 110)));
        assert_eq!(fb.size, Size::from((848, 548)));
        // Scene bounds remain stable across focus changes to prevent layout jitter
        assert_eq!(fb.size, ub.size);
        assert!(focused.amb_alpha > unfocused.amb_alpha);
        assert!(focused.contact_alpha > unfocused.contact_alpha);
    }
}
