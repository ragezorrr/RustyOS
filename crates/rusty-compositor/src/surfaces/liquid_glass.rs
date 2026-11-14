//! Hardware-accelerated Dual-Kawase backdrop blur and Liquid Glass shader (`RUSTYOS-069`).
//!
//! Uses Smithay's safe `GlesTexProgram` and offscreen `GlesTexture` FBO ping-pong APIs
//! (`unsafe_code = "deny"`). Whenever GLES backdrop preparation is skipped (lock screen,
//! `reduce_transparency = true`, secondary pass without backdrop) or fails at runtime,
//! `UiShapeCache` falls back transparently to CPU-rasterized glass panels.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::element::TextureShaderElement;
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName,
    UniformType, UniformValue,
};
use smithay::backend::renderer::utils::{CommitCounter, DamageBag, DamageSet, OpaqueRegions};
use smithay::backend::renderer::{Bind, Frame, Offscreen, Renderer, Texture};
use smithay::utils::{
    Buffer as BufferCoord, Logical, Physical, Point, Rectangle, Scale, Size, Transform,
};
use tracing::warn;

use super::GlassPanelStyle;
use crate::shell_render::CompositorRenderElement;

/// Physical pixel padding added around backdrop damage to account for the 5-pass Dual-Kawase
/// filter footprint so neighboring blurred pixels stay coherent without edge seams.
const BLUR_DAMAGE_PAD_PHYS: i32 = 32;

/// Number of past blur damage commits retained for `OutputDamageTracker`.
const BLUR_DAMAGE_LIMIT: usize = 16;

/// GLSL ES 1.00 Dual-Kawase 5-tap downsample fragment shader compatible with Smithay `GlesTexProgram`.
pub(crate) const KAWASE_DOWN_FRAG: &str = r"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

uniform vec2 u_texel_step;
uniform vec2 u_half_texel;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

vec4 sample_clamped(vec2 uv) {
    vec2 c = clamp(uv, u_half_texel, vec2(1.0) - u_half_texel);
    return texture2D(tex, c);
}

void main() {
    vec2 d = u_texel_step;
    vec4 sum = sample_clamped(v_coords) * 4.0;
    sum += sample_clamped(v_coords - d);
    sum += sample_clamped(v_coords + d);
    sum += sample_clamped(v_coords + vec2(d.x, -d.y));
    sum += sample_clamped(v_coords + vec2(-d.x, d.y));
    vec4 color = sum / 8.0;

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

#if defined(DEBUG_FLAGS)
    if (tint == 1.0) {
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
    }
#endif

    gl_FragColor = color;
}
";

/// GLSL ES 1.00 Dual-Kawase 8-tap upsample fragment shader compatible with Smithay `GlesTexProgram`.
pub(crate) const KAWASE_UP_FRAG: &str = r"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

uniform vec2 u_texel_step;
uniform vec2 u_half_texel;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

vec4 sample_clamped(vec2 uv) {
    vec2 c = clamp(uv, u_half_texel, vec2(1.0) - u_half_texel);
    return texture2D(tex, c);
}

void main() {
    vec2 d = u_texel_step;
    vec4 sum = sample_clamped(v_coords + vec2(-d.x * 2.0, 0.0));
    sum += sample_clamped(v_coords + vec2(-d.x, d.y)) * 2.0;
    sum += sample_clamped(v_coords + vec2(0.0, d.y * 2.0));
    sum += sample_clamped(v_coords + vec2(d.x, d.y)) * 2.0;
    sum += sample_clamped(v_coords + vec2(d.x * 2.0, 0.0));
    sum += sample_clamped(v_coords + vec2(d.x, -d.y)) * 2.0;
    sum += sample_clamped(v_coords + vec2(0.0, -d.y * 2.0));
    sum += sample_clamped(v_coords + vec2(-d.x, -d.y)) * 2.0;
    vec4 color = sum / 12.0;

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

#if defined(DEBUG_FLAGS)
    if (tint == 1.0) {
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
    }
#endif

    gl_FragColor = color;
}
";

/// GLSL ES 1.00 Golden Gate Liquid Glass composite fragment shader:
/// analytical rounded-box SDF, optical edge lensing/dispersion, vibrance boost,
/// light/dark frosted tint, top sheen, and directional specular bevel rim.
pub(crate) const LIQUID_GLASS_FRAG: &str = r"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

uniform vec4 u_uv_rect;
uniform vec4 u_uv_bounds;
uniform vec4 u_size_radius;
uniform vec4 u_tint_rgba;
uniform vec4 u_sat_refract;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

void main() {
    vec2 uv_size = max(u_uv_rect.zw, vec2(0.0001));
    vec2 panel_uv = clamp((v_coords - u_uv_rect.xy) / uv_size, 0.0, 1.0);
    vec2 panel_size = max(u_size_radius.xy, vec2(1.0));
    vec2 p = panel_uv * panel_size - panel_size * 0.5;

    float radius = clamp(u_size_radius.z, 0.0, min(panel_size.x, panel_size.y) * 0.5);
    vec2 half_ext = max(panel_size * 0.5 - vec2(radius), vec2(0.0));
    vec2 q = abs(p) - half_ext;
    float dist = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;

    float coverage = 1.0 - smoothstep(-0.75, 0.75, dist);
    if (coverage <= 0.001) {
        gl_FragColor = vec4(0.0);
        return;
    }

    vec2 grad = vec2(0.0);
    if (q.x > 0.0 && q.y > 0.0) {
        vec2 rq = max(q, vec2(0.0001));
        grad = sign(p) * normalize(rq);
    } else if (q.x > q.y) {
        grad = vec2(sign(p.x), 0.0);
    } else {
        grad = vec2(0.0, sign(p.y));
    }

    float edge_depth = clamp(-dist, 0.0, 32.0);
    float lens_band = exp(-edge_depth / 9.0) * smoothstep(22.0, 0.0, edge_depth);
    vec2 uv_per_px = uv_size / panel_size;
    vec2 lens_uv = grad * (u_sat_refract.y * lens_band) * uv_per_px;
    vec2 disp_uv = grad * (0.42 * lens_band) * uv_per_px;

    vec2 uv_g = clamp(v_coords - lens_uv, u_uv_bounds.xy, u_uv_bounds.zw);
    vec2 uv_r = clamp(v_coords - lens_uv - disp_uv, u_uv_bounds.xy, u_uv_bounds.zw);
    vec2 uv_b = clamp(v_coords - lens_uv + disp_uv, u_uv_bounds.xy, u_uv_bounds.zw);

    vec4 base_sample = texture2D(tex, uv_g);
#if defined(NO_ALPHA)
    base_sample.a = 1.0;
#endif
    vec3 bg = vec3(
        texture2D(tex, uv_r).r,
        base_sample.g,
        texture2D(tex, uv_b).b
    );

    float luma = dot(bg, vec3(0.2126, 0.7152, 0.0722));
    bg = clamp(mix(vec3(luma), bg, u_sat_refract.x), 0.0, 1.0);

    vec3 glass = mix(bg, u_tint_rgba.rgb, u_tint_rgba.a);

    float sheen = pow(clamp(1.0 - panel_uv.y, 0.0, 1.0), 2.8) * u_sat_refract.w;
    glass += vec3(sheen);

    float rim_mask = 1.0 - smoothstep(0.0, 1.35, abs(dist + 0.35));
    float caustic_band = 1.0 - smoothstep(0.0, 3.0, abs(dist + 2.0));
    float top_dir = clamp(-grad.y, 0.0, 1.0);
    float side_dir = abs(grad.x) * 0.52;
    float bottom_dir = clamp(grad.y, 0.0, 1.0) * 0.36;
    float spec_weight = u_sat_refract.z * (0.30 + 0.70 * top_dir + side_dir + bottom_dir);
    glass += vec3(rim_mask * spec_weight + caustic_band * u_sat_refract.z * 0.06);

    float inner_lip = (1.0 - smoothstep(0.0, 1.5, abs(dist + 1.4))) * (1.0 - top_dir) * 0.03;
    glass = clamp(glass - vec3(inner_lip), 0.0, 1.0);

    float out_alpha = coverage * alpha;
#if defined(DEBUG_FLAGS)
    if (tint == 1.0) {
        glass = vec3(0.0, 0.2, 0.0) + glass * 0.8;
    }
#endif

    gl_FragColor = vec4(glass * out_alpha, out_alpha);
}
";

/// Computed Liquid Glass material uniforms derived from Golden Gate Light/Dark theme + opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlassMaterialUniforms {
    /// Target frosted tint RGB and blend weight `a` in `[0.0, 1.0]`.
    pub tint_rgba: [f32; 4],
    /// Saturation multiplier, optical edge refraction strength in logical px,
    /// directional rim weight, and top sheen weight.
    pub sat_refract_rim_sheen: [f32; 4],
}

/// Computes Golden Gate Liquid Glass shader material parameters for `style`, `glass_opacity`, and polarity.
#[must_use]
pub(crate) fn compute_glass_material(
    style: GlassPanelStyle,
    glass_opacity: f32,
    is_dark: bool,
) -> GlassMaterialUniforms {
    let g = crate::config::clamp_glass_opacity(glass_opacity);
    match (style, is_dark) {
        (GlassPanelStyle::Dock, false) => GlassMaterialUniforms {
            tint_rgba: [0.97, 0.98, 1.0, 0.08 + 0.14 * g],
            sat_refract_rim_sheen: [1.28, 6.2, 0.82, 0.14],
        },
        (GlassPanelStyle::Dock, true) => GlassMaterialUniforms {
            tint_rgba: [0.08, 0.085, 0.10, 0.22 + 0.20 * g],
            sat_refract_rim_sheen: [1.24, 5.8, 0.42, 0.09],
        },
        (GlassPanelStyle::Popover, false) => GlassMaterialUniforms {
            tint_rgba: [0.97, 0.98, 1.0, 0.14 + 0.18 * g],
            sat_refract_rim_sheen: [1.24, 4.8, 0.78, 0.12],
        },
        (GlassPanelStyle::Popover, true) => GlassMaterialUniforms {
            tint_rgba: [0.10, 0.105, 0.125, 0.34 + 0.22 * g],
            sat_refract_rim_sheen: [1.20, 4.8, 0.40, 0.08],
        },
        (GlassPanelStyle::ControlModule, false) => GlassMaterialUniforms {
            tint_rgba: [0.985, 0.99, 1.0, 0.20 + 0.20 * g],
            sat_refract_rim_sheen: [1.18, 3.4, 0.66, 0.10],
        },
        (GlassPanelStyle::ControlModule, true) => GlassMaterialUniforms {
            tint_rgba: [0.16, 0.17, 0.20, 0.30 + 0.22 * g],
            sat_refract_rim_sheen: [1.18, 3.6, 0.32, 0.06],
        },
    }
}

/// FBO resolution ladder for Dual-Kawase blur (scene -> 1/2 -> 1/4 -> 1/8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlurFboSizes {
    pub half: Size<i32, BufferCoord>,
    pub quarter: Size<i32, BufferCoord>,
    pub eighth: Size<i32, BufferCoord>,
}

/// Computes downsampled offscreen FBO dimensions (never empty `>= 1x1`) for `scene_phys_size`.
#[must_use]
pub(crate) fn compute_blur_fbo_sizes(scene_phys_size: Size<i32, Physical>) -> BlurFboSizes {
    let w = scene_phys_size.w.max(1);
    let h = scene_phys_size.h.max(1);
    let half = Size::<i32, BufferCoord>::from(((w + 1) / 2, (h + 1) / 2));
    let quarter = Size::<i32, BufferCoord>::from(((half.w + 1) / 2, (half.h + 1) / 2));
    let eighth = Size::<i32, BufferCoord>::from(((quarter.w + 1) / 2, (quarter.h + 1) / 2));
    BlurFboSizes {
        half: Size::from((half.w.max(1), half.h.max(1))),
        quarter: Size::from((quarter.w.max(1), quarter.h.max(1))),
        eighth: Size::from((eighth.w.max(1), eighth.h.max(1))),
    }
}

/// Maps a logical shell panel rectangle onto `half_tex` crop coordinates and normalized shader UV rects.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn compute_panel_src_and_uv(
    panel_logical_rect: Rectangle<i32, Logical>,
    output_scale: f64,
    scene_phys_size: Size<i32, Physical>,
    half_tex_size: Size<i32, BufferCoord>,
) -> (Rectangle<f64, Logical>, [f32; 4], [f32; 4]) {
    let clamped_scale = crate::config::clamp_display_scale(output_scale);
    let scene_w = f64::from(scene_phys_size.w.max(1));
    let scene_h = f64::from(scene_phys_size.h.max(1));
    let half_w = f64::from(half_tex_size.w.max(1));
    let half_h = f64::from(half_tex_size.h.max(1));

    let sx = half_w / scene_w;
    let sy = half_h / scene_h;

    let phys_loc: Point<i32, Physical> = panel_logical_rect
        .loc
        .to_physical_precise_round(clamped_scale);
    let phys_w = f64::from(panel_logical_rect.size.w.max(1)) * clamped_scale;
    let phys_h = f64::from(panel_logical_rect.size.h.max(1)) * clamped_scale;

    let src_x = f64::from(phys_loc.x) * sx;
    let src_y = f64::from(phys_loc.y) * sy;
    let src_w = (phys_w * sx).max(1e-3);
    let src_h = (phys_h * sy).max(1e-3);
    let src = Rectangle::new((src_x, src_y).into(), (src_w, src_h).into());

    let uv_rect = [
        (src_x / half_w) as f32,
        (src_y / half_h) as f32,
        (src_w / half_w) as f32,
        (src_h / half_h) as f32,
    ];
    let half_px_x = (0.5 / half_w) as f32;
    let half_px_y = (0.5 / half_h) as f32;
    let uv_bounds = [
        half_px_x,
        half_px_y,
        (1.0 - half_px_x).max(half_px_x),
        (1.0 - half_px_y).max(half_px_y),
    ];

    (src, uv_rect, uv_bounds)
}

/// Expands physical backdrop damage rectangles by the Kawase blur footprint and maps them into `half_tex` coords.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn expand_and_scale_damage_to_half(
    damage_rects: &[Rectangle<i32, Physical>],
    scene_phys_size: Size<i32, Physical>,
    half_tex_size: Size<i32, BufferCoord>,
    padding_phys: i32,
) -> Vec<Rectangle<i32, BufferCoord>> {
    let scene_w = scene_phys_size.w.max(1);
    let scene_h = scene_phys_size.h.max(1);
    let half_w = half_tex_size.w.max(1);
    let half_h = half_tex_size.h.max(1);
    let sx = f64::from(half_w) / f64::from(scene_w);
    let sy = f64::from(half_h) / f64::from(scene_h);
    let pad = padding_phys.max(0);

    damage_rects
        .iter()
        .filter(|r| r.size.w > 0 && r.size.h > 0)
        .filter_map(|r| {
            let x0 = (r.loc.x - pad).clamp(0, scene_w);
            let y0 = (r.loc.y - pad).clamp(0, scene_h);
            let x1 = (r.loc.x + r.size.w + pad).clamp(0, scene_w);
            let y1 = (r.loc.y + r.size.h + pad).clamp(0, scene_h);
            if x1 <= x0 || y1 <= y0 {
                return None;
            }
            let hx0 = (f64::from(x0) * sx).floor() as i32;
            let hy0 = (f64::from(y0) * sy).floor() as i32;
            let hx1 = (f64::from(x1) * sx).ceil() as i32;
            let hy1 = (f64::from(y1) * sy).ceil() as i32;
            let cw0 = hx0.clamp(0, half_w);
            let cy0 = hy0.clamp(0, half_h);
            let cw1 = hx1.clamp(cw0, half_w);
            let cy1 = hy1.clamp(cy0, half_h);
            (cw1 > cw0 && cy1 > cy0)
                .then(|| Rectangle::new((cw0, cy0).into(), (cw1 - cw0, cy1 - cy0).into()))
        })
        .collect()
}

/// Render element wrapper preserving a stable per-panel [`Id`] over Smithay's [`TextureShaderElement`].
///
/// `TextureShaderElement::new` generates a fresh `Id::new()` per call; keeping a stable `Id` here
/// lets the main output's `OutputDamageTracker` skip redraws when neither the backdrop damage bag
/// nor the panel's own geometry/material parameters changed.
#[derive(Debug)]
pub struct LiquidGlassElement {
    id: Id,
    shader: TextureShaderElement,
}

impl Element for LiquidGlassElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.shader.current_commit()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.shader.geometry(scale)
    }

    fn transform(&self) -> Transform {
        self.shader.transform()
    }

    fn src(&self) -> Rectangle<f64, BufferCoord> {
        self.shader.src()
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.shader.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.shader.opaque_regions(scale)
    }

    fn alpha(&self) -> f32 {
        self.shader.alpha()
    }

    fn kind(&self) -> Kind {
        self.shader.kind()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.shader.location(scale)
    }
}

impl RenderElement<GlesRenderer> for LiquidGlassElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, BufferCoord>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        self.shader.draw(frame, src, dst, damage, opaque_regions)
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        self.shader.underlying_storage(renderer)
    }
}

struct GlassShaderPrograms {
    kawase_down: GlesTexProgram,
    kawase_up: GlesTexProgram,
    composite: GlesTexProgram,
}

impl std::fmt::Debug for GlassShaderPrograms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlassShaderPrograms")
            .finish_non_exhaustive()
    }
}

struct OutputGlassBuffers {
    phys_size: Size<i32, Physical>,
    scale_milli: i64,
    scene_tex: GlesTexture,
    half_a: GlesTexture,
    quarter_a: GlesTexture,
    eighth_a: GlesTexture,
    quarter_b: GlesTexture,
    half_out: GlesTexture,
    scene_tracker: OutputDamageTracker,
    has_valid_scene: bool,
    has_valid_blur: bool,
    blur_damage: DamageBag<i32, BufferCoord>,
}

impl std::fmt::Debug for OutputGlassBuffers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputGlassBuffers")
            .field("phys_size", &self.phys_size)
            .field("scale_milli", &self.scale_milli)
            .field("has_valid_blur", &self.has_valid_blur)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PanelSlotKey {
    output_id: u32,
    style: GlassPanelStyle,
    loc_x: i32,
    loc_y: i32,
    width: u16,
    height: u16,
    occ: u16,
}

#[derive(Debug, Clone)]
struct CachedPanelSlot {
    id: Id,
    params_hash: u64,
    last_frame: u64,
}

/// Manages offscreen scene capture, Dual-Kawase blur ping-pong FBOs, and Liquid Glass elements.
#[derive(Debug, Default)]
pub(crate) struct LiquidGlassPipeline {
    programs: Option<GlassShaderPrograms>,
    compile_failed: bool,
    outputs: HashMap<u32, OutputGlassBuffers>,
    active_output: Option<u32>,
    active_scale: f64,
    frame_counter: u64,
    frame_slot_counts: HashMap<(u32, GlassPanelStyle, i32, i32, u16, u16), u16>,
    slots: HashMap<PanelSlotKey, CachedPanelSlot>,
}

impl LiquidGlassPipeline {
    /// Clears any active backdrop selection before or after building an output's render list.
    pub(crate) fn clear_active(&mut self) {
        self.active_output = None;
        self.frame_slot_counts.clear();
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn has_active_backdrop(&self) -> bool {
        self.active_output
            .is_some_and(|id| self.outputs.get(&id).is_some_and(|buf| buf.has_valid_blur))
    }

    /// Renders `backdrop_elements` incrementally into `output_id`'s offscreen FBO and runs
    /// the 5-pass Dual-Kawase blur whenever the underlying scene reports damage.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn prepare_backdrop(
        &mut self,
        renderer: &mut GlesRenderer,
        output_id: u32,
        logical_size: Size<i32, Logical>,
        output_scale: f64,
        backdrop_elements: &[CompositorRenderElement],
    ) -> bool {
        self.clear_active();
        let clamped_scale = crate::config::clamp_display_scale(output_scale);
        let phys_size_i32 =
            Size::<i32, Logical>::from((logical_size.w.max(1), logical_size.h.max(1)))
                .to_f64()
                .to_physical(clamped_scale)
                .to_i32_round::<i32>();
        let scene_phys =
            Size::<i32, Physical>::from((phys_size_i32.w.max(1), phys_size_i32.h.max(1)));
        let scale_milli = (clamped_scale * 1000.0).round() as i64;

        if !self.ensure_programs(renderer) {
            return false;
        }
        if !self.ensure_output_buffers(renderer, output_id, scene_phys, clamped_scale, scale_milli)
        {
            self.outputs.remove(&output_id);
            return false;
        }

        let Some(programs) = self.programs.as_ref() else {
            return false;
        };
        let Some(buffers) = self.outputs.get_mut(&output_id) else {
            return false;
        };

        let result = run_backdrop_blur_passes(renderer, programs, buffers, backdrop_elements);
        match result {
            Ok(()) => {
                self.frame_counter = self.frame_counter.wrapping_add(1);
                self.active_output = Some(output_id);
                self.active_scale = clamped_scale;
                if self.slots.len() > 128 {
                    let cutoff = self.frame_counter.saturating_sub(32);
                    self.slots.retain(|_, v| v.last_frame >= cutoff);
                }
                true
            }
            Err(err) => {
                warn!(
                    "Liquid Glass GLES backdrop pass failed on output {output_id}: {err}; using CPU fallback"
                );
                buffers.has_valid_scene = false;
                buffers.has_valid_blur = false;
                false
            }
        }
    }

    /// Creates a Liquid Glass GPU shader element sampling the active output's blurred backdrop,
    /// returning `None` if no valid GLES backdrop is prepared (triggering CPU fallback).
    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub(crate) fn make_glass_element(
        &mut self,
        renderer: &mut GlesRenderer,
        style: GlassPanelStyle,
        width: i32,
        height: i32,
        radius: i32,
        glass_opacity: f32,
        reduce_transparency: bool,
        loc: Point<i32, Logical>,
    ) -> Option<LiquidGlassElement> {
        if reduce_transparency || width <= 0 || height <= 0 {
            return None;
        }
        let output_id = self.active_output?;
        let buffers = self.outputs.get(&output_id)?;
        if !buffers.has_valid_blur {
            return None;
        }
        let program = self.programs.as_ref()?.composite.clone();

        let w_u16 = u16::try_from(width).ok()?;
        let h_u16 = u16::try_from(height).ok()?;
        let clamped_r = radius.clamp(0, width.min(height) / 2);
        let r_u16 = u16::try_from(clamped_r).ok()?;
        let glass_pct = (crate::config::clamp_glass_opacity(glass_opacity) * 100.0).round() as u8;
        let is_dark = crate::shell_render::active_appearance().is_dark();

        let half_tex = buffers.half_out.clone();
        let half_size = half_tex.size();
        let blur_snapshot = buffers.blur_damage.snapshot();
        let panel_rect = Rectangle::new(loc, Size::from((width, height)));
        let (src_in_half, uv_rect, uv_bounds) =
            compute_panel_src_and_uv(panel_rect, self.active_scale, buffers.phys_size, half_size);
        let material = compute_glass_material(style, f32::from(glass_pct) / 100.0, is_dark);

        let params_hash =
            compute_panel_params_hash(style, width, height, clamped_r, glass_pct, is_dark, uv_rect);
        let stable_id = self.resolve_slot_id(output_id, style, loc, w_u16, h_u16, params_hash);

        let inner = TextureRenderElement::from_texture_with_damage(
            stable_id.clone(),
            renderer.context_id(),
            loc.to_f64().to_physical(1.0),
            half_tex,
            1,
            Transform::Normal,
            Some(1.0),
            Some(src_in_half),
            Some(Size::from((width, height))),
            None,
            blur_snapshot,
            Kind::Unspecified,
        );
        let uniforms =
            build_glass_uniforms(uv_rect, uv_bounds, w_u16, h_u16, r_u16, glass_pct, material);

        Some(LiquidGlassElement {
            id: stable_id,
            shader: TextureShaderElement::new(inner, program, uniforms),
        })
    }

    fn resolve_slot_id(
        &mut self,
        output_id: u32,
        style: GlassPanelStyle,
        loc: Point<i32, Logical>,
        width: u16,
        height: u16,
        params_hash: u64,
    ) -> Id {
        let base_slot = (output_id, style, loc.x, loc.y, width, height);
        let occ = {
            let count = self.frame_slot_counts.entry(base_slot).or_insert(0);
            let cur = *count;
            *count = count.saturating_add(1);
            cur
        };
        let slot_key = PanelSlotKey {
            output_id,
            style,
            loc_x: loc.x,
            loc_y: loc.y,
            width,
            height,
            occ,
        };
        let frame_gen = self.frame_counter;
        match self.slots.get_mut(&slot_key) {
            Some(entry) if entry.params_hash == params_hash => {
                entry.last_frame = frame_gen;
                entry.id.clone()
            }
            Some(entry) => {
                entry.id = Id::new();
                entry.params_hash = params_hash;
                entry.last_frame = frame_gen;
                entry.id.clone()
            }
            None => {
                let id = Id::new();
                self.slots.insert(
                    slot_key,
                    CachedPanelSlot {
                        id: id.clone(),
                        params_hash,
                        last_frame: frame_gen,
                    },
                );
                id
            }
        }
    }

    fn ensure_programs(&mut self, renderer: &mut GlesRenderer) -> bool {
        if self.programs.is_some() {
            return true;
        }
        if self.compile_failed {
            return false;
        }
        let kawase_uniforms = [
            UniformName::new("u_texel_step", UniformType::_2f),
            UniformName::new("u_half_texel", UniformType::_2f),
        ];
        let composite_uniforms = [
            UniformName::new("u_uv_rect", UniformType::_4f),
            UniformName::new("u_uv_bounds", UniformType::_4f),
            UniformName::new("u_size_radius", UniformType::_4f),
            UniformName::new("u_tint_rgba", UniformType::_4f),
            UniformName::new("u_sat_refract", UniformType::_4f),
        ];
        let compiled = (|| -> Result<GlassShaderPrograms, GlesError> {
            let kawase_down =
                renderer.compile_custom_texture_shader(KAWASE_DOWN_FRAG, &kawase_uniforms)?;
            let kawase_up =
                renderer.compile_custom_texture_shader(KAWASE_UP_FRAG, &kawase_uniforms)?;
            let composite =
                renderer.compile_custom_texture_shader(LIQUID_GLASS_FRAG, &composite_uniforms)?;
            Ok(GlassShaderPrograms {
                kawase_down,
                kawase_up,
                composite,
            })
        })();
        match compiled {
            Ok(programs) => {
                self.programs = Some(programs);
                true
            }
            Err(err) => {
                warn!(
                    "Failed to compile Liquid Glass GLES shaders ({err}); falling back to CPU glass"
                );
                self.compile_failed = true;
                false
            }
        }
    }

    fn ensure_output_buffers(
        &mut self,
        renderer: &mut GlesRenderer,
        output_id: u32,
        scene_phys: Size<i32, Physical>,
        clamped_scale: f64,
        scale_milli: i64,
    ) -> bool {
        let needs_recreate = self
            .outputs
            .get(&output_id)
            .is_none_or(|buf| buf.phys_size != scene_phys || buf.scale_milli != scale_milli);
        if !needs_recreate {
            return true;
        }
        let fbo_sizes = compute_blur_fbo_sizes(scene_phys);
        let scene_buf_size = Size::<i32, BufferCoord>::from((scene_phys.w, scene_phys.h));
        let created = (|| -> Result<OutputGlassBuffers, GlesError> {
            let scene_tex = renderer.create_buffer(Fourcc::Abgr8888, scene_buf_size)?;
            let half_a = renderer.create_buffer(Fourcc::Abgr8888, fbo_sizes.half)?;
            let quarter_a = renderer.create_buffer(Fourcc::Abgr8888, fbo_sizes.quarter)?;
            let eighth_a = renderer.create_buffer(Fourcc::Abgr8888, fbo_sizes.eighth)?;
            let quarter_b = renderer.create_buffer(Fourcc::Abgr8888, fbo_sizes.quarter)?;
            let half_out = renderer.create_buffer(Fourcc::Abgr8888, fbo_sizes.half)?;
            let mut blur_damage = DamageBag::new(BLUR_DAMAGE_LIMIT);
            blur_damage.reset();
            let scene_tracker =
                OutputDamageTracker::new(scene_phys, Scale::from(clamped_scale), Transform::Normal);
            Ok(OutputGlassBuffers {
                phys_size: scene_phys,
                scale_milli,
                scene_tex,
                half_a,
                quarter_a,
                eighth_a,
                quarter_b,
                half_out,
                scene_tracker,
                has_valid_scene: false,
                has_valid_blur: false,
                blur_damage,
            })
        })();
        match created {
            Ok(buf) => {
                self.outputs.insert(output_id, buf);
                true
            }
            Err(err) => {
                warn!("Failed to allocate Liquid Glass FBOs for output {output_id}: {err}");
                false
            }
        }
    }
}

fn run_backdrop_blur_passes(
    renderer: &mut GlesRenderer,
    programs: &GlassShaderPrograms,
    buffers: &mut OutputGlassBuffers,
    backdrop_elements: &[CompositorRenderElement],
) -> Result<(), String> {
    let age = usize::from(buffers.has_valid_scene);
    let damage_rects: Option<Vec<Rectangle<i32, Physical>>> = {
        let mut fb = renderer
            .bind(&mut buffers.scene_tex)
            .map_err(|e| format!("bind scene FBO: {e}"))?;
        let frame_result = buffers
            .scene_tracker
            .render_output(
                renderer,
                &mut fb,
                age,
                backdrop_elements,
                crate::frame_composer::BACKGROUND_COLOR,
            )
            .map_err(|e| format!("render backdrop scene: {e}"))?;
        frame_result.damage.cloned()
    };
    buffers.has_valid_scene = true;

    let Some(damage) = damage_rects else {
        // Scene behind glass did not change this frame: keep existing blurred FBO untouched.
        return Ok(());
    };
    if damage.is_empty() && buffers.has_valid_blur {
        return Ok(());
    }

    run_kawase_blit(
        renderer,
        &buffers.scene_tex,
        &mut buffers.half_a,
        &programs.kawase_down,
        1.0,
    )?;
    run_kawase_blit(
        renderer,
        &buffers.half_a,
        &mut buffers.quarter_a,
        &programs.kawase_down,
        1.25,
    )?;
    run_kawase_blit(
        renderer,
        &buffers.quarter_a,
        &mut buffers.eighth_a,
        &programs.kawase_down,
        1.5,
    )?;
    run_kawase_blit(
        renderer,
        &buffers.eighth_a,
        &mut buffers.quarter_b,
        &programs.kawase_up,
        1.25,
    )?;
    run_kawase_blit(
        renderer,
        &buffers.quarter_b,
        &mut buffers.half_out,
        &programs.kawase_up,
        1.15,
    )?;

    let half_size = buffers.half_out.size();
    let scaled_damage = expand_and_scale_damage_to_half(
        &damage,
        buffers.phys_size,
        half_size,
        BLUR_DAMAGE_PAD_PHYS,
    );
    if !buffers.has_valid_blur {
        buffers.blur_damage.add([Rectangle::from_size(half_size)]);
        buffers.has_valid_blur = true;
    } else if !scaled_damage.is_empty() {
        buffers.blur_damage.add(scaled_damage);
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn run_kawase_blit(
    renderer: &mut GlesRenderer,
    src_tex: &GlesTexture,
    dst_tex: &mut GlesTexture,
    program: &GlesTexProgram,
    offset_px: f32,
) -> Result<(), String> {
    let src_size = src_tex.size();
    let dst_size = dst_tex.size();
    let src_w = src_size.w.max(1) as f32;
    let src_h = src_size.h.max(1) as f32;
    let uniforms = vec![
        Uniform::new(
            "u_texel_step",
            UniformValue::_2f(offset_px / src_w, offset_px / src_h),
        ),
        Uniform::new("u_half_texel", UniformValue::_2f(0.5 / src_w, 0.5 / src_h)),
    ];

    let inner = TextureRenderElement::from_static_texture(
        Id::new(),
        renderer.context_id(),
        Point::from((0.0, 0.0)),
        src_tex.clone(),
        1,
        Transform::Normal,
        Some(1.0),
        Some(Rectangle::from_size(Size::from((
            f64::from(src_size.w.max(1)),
            f64::from(src_size.h.max(1)),
        )))),
        Some(Size::from((dst_size.w.max(1), dst_size.h.max(1)))),
        Some(vec![Rectangle::from_size(dst_size)]),
        Kind::Unspecified,
    );
    let elem = TextureShaderElement::new(inner, program.clone(), uniforms);
    let dst_phys = Size::<i32, Physical>::from((dst_size.w.max(1), dst_size.h.max(1)));
    let dst_rect = Rectangle::from_size(dst_phys);

    let mut fb = renderer
        .bind(dst_tex)
        .map_err(|e| format!("bind Kawase dst FBO: {e}"))?;
    let mut frame = renderer
        .render(&mut fb, dst_phys, Transform::Normal)
        .map_err(|e| format!("begin Kawase frame: {e}"))?;
    elem.draw(&mut frame, elem.src(), dst_rect, &[dst_rect], &[dst_rect])
        .map_err(|e| format!("draw Kawase quad: {e}"))?;
    let _sync = frame
        .finish()
        .map_err(|e| format!("finish Kawase frame: {e}"))?;
    Ok(())
}

fn build_glass_uniforms(
    uv_rect: [f32; 4],
    uv_bounds: [f32; 4],
    width: u16,
    height: u16,
    radius: u16,
    glass_pct: u8,
    material: GlassMaterialUniforms,
) -> Vec<Uniform<'static>> {
    vec![
        Uniform::new(
            "u_uv_rect",
            UniformValue::_4f(uv_rect[0], uv_rect[1], uv_rect[2], uv_rect[3]),
        ),
        Uniform::new(
            "u_uv_bounds",
            UniformValue::_4f(uv_bounds[0], uv_bounds[1], uv_bounds[2], uv_bounds[3]),
        ),
        Uniform::new(
            "u_size_radius",
            UniformValue::_4f(
                f32::from(width),
                f32::from(height),
                f32::from(radius),
                f32::from(glass_pct) / 100.0,
            ),
        ),
        Uniform::new(
            "u_tint_rgba",
            UniformValue::_4f(
                material.tint_rgba[0],
                material.tint_rgba[1],
                material.tint_rgba[2],
                material.tint_rgba[3],
            ),
        ),
        Uniform::new(
            "u_sat_refract",
            UniformValue::_4f(
                material.sat_refract_rim_sheen[0],
                material.sat_refract_rim_sheen[1],
                material.sat_refract_rim_sheen[2],
                material.sat_refract_rim_sheen[3],
            ),
        ),
    ]
}

fn compute_panel_params_hash(
    style: GlassPanelStyle,
    width: i32,
    height: i32,
    radius: i32,
    glass_pct: u8,
    is_dark: bool,
    uv_rect: [f32; 4],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    style.hash(&mut hasher);
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    radius.hash(&mut hasher);
    glass_pct.hash(&mut hasher);
    is_dark.hash(&mut hasher);
    for v in uv_rect {
        v.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_sources_include_smithay_contract_defines_and_uniforms() {
        for shader in [KAWASE_DOWN_FRAG, KAWASE_UP_FRAG, LIQUID_GLASS_FRAG] {
            assert!(shader.starts_with("#version 100\n"));
            assert!(shader.contains("//_DEFINES_"));
            assert!(shader.contains("#if defined(EXTERNAL)"));
            assert!(shader.contains("#if defined(NO_ALPHA)"));
            assert!(shader.contains("#if defined(DEBUG_FLAGS)"));
            assert!(shader.contains("texture2D("));
        }
        assert!(KAWASE_DOWN_FRAG.contains("u_texel_step"));
        assert!(KAWASE_DOWN_FRAG.contains("u_half_texel"));
        assert!(KAWASE_UP_FRAG.contains("u_texel_step"));
        assert!(LIQUID_GLASS_FRAG.contains("u_uv_rect"));
        assert!(LIQUID_GLASS_FRAG.contains("u_uv_bounds"));
        assert!(LIQUID_GLASS_FRAG.contains("u_size_radius"));
        assert!(LIQUID_GLASS_FRAG.contains("u_tint_rgba"));
        assert!(LIQUID_GLASS_FRAG.contains("u_sat_refract"));
    }

    #[test]
    fn blur_fbo_ladder_scales_down_and_never_underflows() {
        let sizes = compute_blur_fbo_sizes(Size::from((1920, 1080)));
        assert_eq!(sizes.half, Size::from((960, 540)));
        assert_eq!(sizes.quarter, Size::from((480, 270)));
        assert_eq!(sizes.eighth, Size::from((240, 135)));

        let tiny = compute_blur_fbo_sizes(Size::from((3, 3)));
        assert_eq!(tiny.half, Size::from((2, 2)));
        assert_eq!(tiny.quarter, Size::from((1, 1)));
        assert_eq!(tiny.eighth, Size::from((1, 1)));
    }

    #[test]
    fn panel_src_and_uv_respects_hidpi_scale_and_safe_bounds() {
        // 1920x1080 logical @ 2.0 scale = 3840x2160 phys, half FBO = 1920x1080 buffer
        let panel = Rectangle::new((400, 900).into(), (600, 64).into());
        let (src, uv_rect, uv_bounds) = compute_panel_src_and_uv(
            panel,
            2.0,
            Size::from((3840, 2160)),
            Size::from((1920, 1080)),
        );
        assert!((src.loc.x - 400.0).abs() < 1e-3);
        assert!((src.loc.y - 900.0).abs() < 1e-3);
        assert!((src.size.w - 600.0).abs() < 1e-3);
        assert!((src.size.h - 64.0).abs() < 1e-3);
        assert!((uv_rect[0] - (400.0 / 1920.0)).abs() < 1e-5);
        assert!((uv_rect[1] - (900.0 / 1080.0)).abs() < 1e-5);
        assert!(uv_bounds[0] > 0.0 && uv_bounds[2] < 1.0);
        assert!(uv_bounds[1] > 0.0 && uv_bounds[3] < 1.0);
    }

    #[test]
    fn backdrop_damage_expansion_covers_blur_pad_and_clamps_to_half_fbo() {
        let half = Size::from((960, 540));
        let phys = Size::from((1920, 1080));
        let dmg = expand_and_scale_damage_to_half(
            &[Rectangle::new((100, 100).into(), (20, 20).into())],
            phys,
            half,
            32,
        );
        assert_eq!(dmg.len(), 1);
        // (100 - 32)/2 = 34, (120 + 32)/2 = 76
        assert_eq!(dmg[0], Rectangle::new((34, 34).into(), (42, 42).into()));

        let corner = expand_and_scale_damage_to_half(
            &[Rectangle::new((0, 0).into(), (10, 10).into())],
            phys,
            half,
            32,
        );
        assert_eq!(corner[0].loc, Point::from((0, 0)));
    }

    #[test]
    fn liquid_glass_pipeline_defaults_to_cpu_fallback_when_inactive() {
        let mut pipeline = LiquidGlassPipeline::default();
        assert!(!pipeline.has_active_backdrop());
        pipeline.clear_active();
        assert!(!pipeline.has_active_backdrop());

        let light = compute_glass_material(GlassPanelStyle::Dock, 0.85, false);
        let dark = compute_glass_material(GlassPanelStyle::Dock, 0.85, true);
        assert!(light.tint_rgba[0] > 0.8 && dark.tint_rgba[0] < 0.2);
        assert!(light.sat_refract_rim_sheen[2] > dark.sat_refract_rim_sheen[2]);
    }
}
