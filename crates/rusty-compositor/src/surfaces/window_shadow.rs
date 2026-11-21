//! Seamless analytical 3-lobe Gaussian GLES SDF drop shadows (`RUSTYOS-075`, `RUSTYOS-128`).
//!
//! Renders single-pass continuous 3-lobe (wide + narrow + 1.5px contact) analytical
//! Gaussian shadows with cubic squircle geometry and screen-space 4x4 ordered dithering
//! using Smithay's safe `GlesTexProgram` + `TextureShaderElement` (`unsafe_code = "deny"`).

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

/// GLSL ES 1.00 fragment shader for seamless RustyOS rounded-box drop shadows.
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
uniform vec4 u_lobe1;
uniform vec4 u_lobe2;
uniform vec4 u_lobe3;
uniform vec4 u_shadow_rgb;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

float squircleDistance(vec2 p, vec2 halfSize, float r) {
    float minHalf = min(halfSize.x, halfSize.y);
    float rClamped = clamp(r, 0.0, minHalf);
    if (rClamped >= minHalf - 0.5 || rClamped <= 0.5) {
        vec2 q = abs(p) - halfSize + vec2(rClamped);
        return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - rClamped;
    }
    float rExt = clamp(rClamped * 1.42, 0.0, minHalf);
    vec2 q = abs(p) - halfSize + vec2(rExt);
    if (min(q.x, q.y) <= 0.0) {
        return max(q.x, q.y) - rExt;
    }
    vec2 u = max(q / rExt, vec2(0.0));
    float s3 = u.x * u.x * u.x + u.y * u.y * u.y;
    float s1 = pow(max(s3, 1e-8), 1.0 / 3.0);
    float s2 = s1 * s1;
    vec2 grad = (u * u) / max(s2, 1e-6);
    float gradLen = max(length(grad), 0.707);
    return ((s1 - 1.0) * rExt) / gradLen;
}

float evalLobe(vec2 p_win, vec2 half_size, float radius, vec4 lobe) {
    if (lobe.x <= 0.0005 || lobe.y <= 0.001) {
        return 0.0;
    }
    float d = max(squircleDistance(p_win - lobe.zw, half_size, radius), 0.0);
    float spread = max(lobe.y, 0.5);
    float norm_d = d / spread;
    float edge_fade = smoothstep(1.75, 1.35, norm_d);
    return lobe.x * exp(-(norm_d * norm_d) * 2.2) * edge_fade;
}

void main() {
    vec2 bounds_size = max(u_bounds_win.xy, vec2(1.0));
    vec2 win_size = max(u_win_geom.xy, vec2(1.0));
    vec2 half_size = win_size * 0.5;
    float radius = clamp(u_win_geom.z, 0.0, min(half_size.x, half_size.y));

    vec2 frag_px = v_coords * bounds_size;
    vec2 win_center = u_bounds_win.zw + half_size;
    vec2 p_win = frag_px - win_center;
    float sd_win = squircleDistance(p_win, half_size, radius);

    // Mask out interior strictly so shadow never darkens translucent client/glass interiors.
    float outside_mask = smoothstep(-0.75, 0.35, sd_win);
    if (outside_mask <= 0.001) {
        gl_FragColor = vec4(0.0);
        return;
    }

    float shadow_a = clamp(
        evalLobe(p_win, half_size, radius, u_lobe1)
            + evalLobe(p_win, half_size, radius, u_lobe2)
            + evalLobe(p_win, half_size, radius, u_lobe3),
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

/// Analytical 3-lobe Gaussian drop shadow specification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WindowShadowSpec {
    pub win_rect: Rectangle<i32, Logical>,
    pub radius: i32,
    pub lobes: [rusty_ui::shadow::ShadowLobe; 3],
    pub shadow_rgb: [u8; 3],
    pub opacity: f32,
}

impl WindowShadowSpec {
    /// Standard active/inactive window shadow preset (`RUSTYOS-075`, `RUSTYOS-080`).
    #[cfg(test)]
    #[must_use]
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

    /// Standardized 3-lobe Gaussian shadow specification for a desktop elevation (`RUSTYOS-126`, `RUSTYOS-128`).
    #[must_use]
    pub(crate) fn for_elevation(
        win_rect: Rectangle<i32, Logical>,
        radius: i32,
        elevation: rusty_ui::shadow::ShadowElevation,
        shadow_rgb: [u8; 3],
    ) -> Self {
        let style = rusty_ui::shadow::ShadowStyle::for_elevation(elevation);
        Self {
            win_rect,
            radius,
            lobes: style.lobes,
            shadow_rgb,
            opacity: 1.0,
        }
    }

    /// Returns a copy of this shadow specification scaled by `opacity` (`0.0..=1.0`).
    #[must_use]
    pub(crate) fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
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
            for lobe in &mut spec.lobes {
                lobe.opacity *= 0.85;
            }
        }
        spec
    }

    /// Computes outer shadow bounding box `(bounds, pad_left, pad_top)` in logical pixels
    /// without truncating any active lobe tail.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn bounds_and_offsets(&self) -> (Rectangle<i32, Logical>, i32, i32) {
        let mut max_left = 1.0_f32;
        let mut max_right = 1.0_f32;
        let mut max_top = 1.0_f32;
        let mut max_bot = 1.0_f32;
        for lobe in &self.lobes {
            if lobe.opacity <= 0.0005 || lobe.sigma <= 0.0 {
                continue;
            }
            let reach = lobe.sigma * 1.75;
            max_left = max_left.max(reach - lobe.offset.0);
            max_right = max_right.max(reach + lobe.offset.0);
            max_top = max_top.max(reach - lobe.offset.1);
            max_bot = max_bot.max(reach + lobe.offset.1);
        }
        let pad_left = max_left.ceil() as i32;
        let pad_right = max_right.ceil() as i32;
        let pad_top = max_top.ceil() as i32;
        let pad_bot = max_bot.ceil() as i32;
        let bounds = Rectangle::new(
            Point::from((
                self.win_rect.loc.x - pad_left,
                self.win_rect.loc.y - pad_top,
            )),
            Size::from((
                self.win_rect.size.w.max(1) + pad_left + pad_right,
                self.win_rect.size.h.max(1) + pad_top + pad_bot,
            )),
        );
        (bounds, pad_left, pad_top)
    }

    fn params_hash(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.win_rect.size.w.hash(&mut h);
        self.win_rect.size.h.hash(&mut h);
        self.radius.hash(&mut h);
        for lobe in &self.lobes {
            lobe.sigma.to_bits().hash(&mut h);
            lobe.offset.0.to_bits().hash(&mut h);
            lobe.offset.1.to_bits().hash(&mut h);
            lobe.opacity.to_bits().hash(&mut h);
        }
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
                UniformName::new("u_lobe1", UniformType::_4f),
                UniformName::new("u_lobe2", UniformType::_4f),
                UniformName::new("u_lobe3", UniformType::_4f),
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

    /// Builds a single hardware-accelerated 3-lobe shadow [`LiquidGlassElement`].
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn make_shadow_element(
        &mut self,
        renderer: &mut GlesRenderer,
        spec: WindowShadowSpec,
    ) -> Option<LiquidGlassElement> {
        let has_active_lobe = spec
            .lobes
            .iter()
            .any(|l| l.opacity > 0.0005 && l.sigma > 0.0);
        if spec.win_rect.size.w <= 0
            || spec.win_rect.size.h <= 0
            || !has_active_lobe
            || !self.ensure_resources(renderer)
        {
            return None;
        }
        let program = self.program.as_ref()?.clone();
        let tex = self.dummy_tex.as_ref()?.clone();

        let (bounds, pad_x, pad_top) = spec.bounds_and_offsets();
        let params_hash = spec.params_hash();
        let stable_id = self.resolve_slot_id(spec.win_rect, params_hash);

        let l1 = spec.lobes[0];
        let l2 = spec.lobes[1];
        let l3 = spec.lobes[2];
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
                    0.0,
                ),
            ),
            Uniform::new(
                "u_lobe1",
                UniformValue::_4f(l1.opacity, l1.sigma, l1.offset.0, l1.offset.1),
            ),
            Uniform::new(
                "u_lobe2",
                UniformValue::_4f(l2.opacity, l2.sigma, l2.offset.0, l2.offset.1),
            ),
            Uniform::new(
                "u_lobe3",
                UniformValue::_4f(l3.opacity, l3.sigma, l3.offset.0, l3.offset.1),
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
            Some(spec.opacity.clamp(0.0, 1.0)),
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
            "u_lobe1",
            "u_lobe2",
            "u_lobe3",
            "u_shadow_rgb",
        ] {
            assert!(WINDOW_SHADOW_FRAG.contains(uniform));
        }
    }

    #[test]
    fn focused_and_unfocused_shadow_bounds_cover_vertical_drop_without_clipping() {
        let win = Rectangle::new((100, 120).into(), (800, 500).into());
        let focused = WindowShadowSpec::for_window(win, 14, true);
        let unfocused = WindowShadowSpec::for_window(win, 14, false);
        let (fb, fpx, fpt) = focused.bounds_and_offsets();
        let (ub, upx, upt) = unfocused.bounds_and_offsets();
        assert_eq!(fpx, 42);
        assert_eq!(fpt, 28);
        assert_eq!(upx, 42);
        assert_eq!(upt, 28);
        assert_eq!(fb.loc, Point::from((58, 92)));
        assert_eq!(fb.size, Size::from((884, 584)));
        // Scene bounds remain stable across focus changes to prevent layout jitter
        assert_eq!(fb.size, ub.size);
        assert!(focused.lobes[0].opacity > unfocused.lobes[0].opacity);
        assert!(focused.lobes[1].opacity > unfocused.lobes[1].opacity);
        assert!(focused.lobes[2].opacity > unfocused.lobes[2].opacity);
    }
}
