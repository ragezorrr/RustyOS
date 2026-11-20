//! Compositor-side rounded-corner clipping for client Wayland surfaces (`RUSTYOS-128`).
//!
//! Wraps non-fullscreen [`WaylandSurfaceRenderElement`]s in [`ClippedSurfaceElement`] so every
//! client window and subsurface conforms to the unified continuous cubic squircle silhouette
//! (`WINDOW_RADIUS = 14 px`) without blocking Direct Scanout on fullscreen windows.

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::ImportMem;
use smithay::backend::renderer::element::surface::{
    WaylandSurfaceRenderElement, WaylandSurfaceTexture,
};
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName,
    UniformType, UniformValue,
};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::utils::{Buffer as BufferCoord, Physical, Point, Rectangle, Scale, Size, Transform};
use tracing::warn;

/// GLSL ES 1.00 texture override shader clipping client surfaces to the unified cubic squircle.
pub(crate) const SURFACE_CLIP_FRAG: &str = r"#version 100

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

uniform vec4 u_ndc_scale_b;
uniform vec4 u_inv_a;
uniform vec4 u_clip_rect;
uniform vec4 u_clip_params;
uniform vec4 u_solid_color;

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

void main() {
    vec4 color;
    if (u_clip_params.y > 0.5) {
        color = u_solid_color;
    } else {
        color = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
        color = vec4(color.rgb, 1.0) * alpha;
#else
        color = color * alpha;
#endif
    }

    if (u_clip_params.w > 0.5) {
        vec2 ndc = gl_FragCoord.xy * u_ndc_scale_b.xy - vec2(1.0);
        vec2 d_ndc = ndc - u_ndc_scale_b.zw;
        vec2 p_phys = vec2(dot(u_inv_a.xy, d_ndc), dot(u_inv_a.zw, d_ndc));
        float d = squircleDistance(p_phys - u_clip_rect.xy, u_clip_rect.zw, u_clip_params.x);
        float cov = smoothstep(0.5, -0.5, d);
        if (cov <= 0.001) {
            gl_FragColor = vec4(0.0);
            return;
        }
        color *= cov;
    }

    color *= u_clip_params.z;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0) {
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
    }
#endif

    gl_FragColor = color;
}
";

/// Cached GLES clip program + 1x1 white carrier texture for `SolidColor` Wayland surfaces.
#[derive(Default)]
pub(crate) struct WindowClipPipeline {
    program: Option<GlesTexProgram>,
    solid_carrier: Option<GlesTexture>,
    compile_failed: bool,
}

impl std::fmt::Debug for WindowClipPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowClipPipeline")
            .field("has_program", &self.program.is_some())
            .field("has_solid_carrier", &self.solid_carrier.is_some())
            .field("compile_failed", &self.compile_failed)
            .finish()
    }
}

impl WindowClipPipeline {
    /// Ensures the surface-clip shader program and 1x1 carrier texture are ready for use.
    pub(crate) fn ensure_ready(&mut self, renderer: &mut GlesRenderer) -> bool {
        if self.program.is_some() && self.solid_carrier.is_some() {
            return true;
        }
        if self.compile_failed {
            return false;
        }
        if self.program.is_none() {
            let uniforms = [
                UniformName::new("u_ndc_scale_b", UniformType::_4f),
                UniformName::new("u_inv_a", UniformType::_4f),
                UniformName::new("u_clip_rect", UniformType::_4f),
                UniformName::new("u_clip_params", UniformType::_4f),
                UniformName::new("u_solid_color", UniformType::_4f),
            ];
            match renderer.compile_custom_texture_shader(SURFACE_CLIP_FRAG, &uniforms) {
                Ok(prog) => self.program = Some(prog),
                Err(err) => {
                    warn!("Failed to compile window clip GLES shader ({err}); leaving unclipped");
                    self.compile_failed = true;
                    return false;
                }
            }
        }
        if self.solid_carrier.is_none() {
            match renderer.import_memory(
                &[255, 255, 255, 255],
                Fourcc::Abgr8888,
                Size::from((1, 1)),
                false,
            ) {
                Ok(tex) => self.solid_carrier = Some(tex),
                Err(err) => {
                    warn!(
                        "Failed to allocate 1x1 window clip carrier texture ({err}); leaving unclipped"
                    );
                    self.compile_failed = true;
                    return false;
                }
            }
        }
        true
    }

    /// Wraps a non-fullscreen client `WaylandSurfaceRenderElement` with rounded-corner clipping,
    /// falling back to `CompositorRenderElement::Surface(inner)` if GLES shader setup fails.
    #[must_use]
    pub(crate) fn wrap_or_surface(
        &mut self,
        renderer: &mut GlesRenderer,
        inner: WaylandSurfaceRenderElement<GlesRenderer>,
        clip_rect_phys: Rectangle<i32, Physical>,
        clip_radius_phys: f32,
        extra_alpha: f32,
    ) -> crate::shell_render::CompositorRenderElement {
        if !self.ensure_ready(renderer) {
            return crate::shell_render::CompositorRenderElement::Surface(inner);
        }
        let (Some(program), Some(solid_carrier)) =
            (self.program.clone(), self.solid_carrier.clone())
        else {
            return crate::shell_render::CompositorRenderElement::Surface(inner);
        };
        crate::shell_render::CompositorRenderElement::ClippedSurface(ClippedSurfaceElement {
            inner,
            program,
            solid_carrier,
            clip_rect_phys,
            clip_radius_phys: clip_radius_phys.max(0.0),
            extra_alpha: extra_alpha.clamp(0.0, 1.0),
        })
    }
}

/// Render element wrapper enforcing unified cubic squircle corner clipping on a client surface.
#[derive(Debug)]
pub struct ClippedSurfaceElement {
    inner: WaylandSurfaceRenderElement<GlesRenderer>,
    program: GlesTexProgram,
    solid_carrier: GlesTexture,
    clip_rect_phys: Rectangle<i32, Physical>,
    clip_radius_phys: f32,
    extra_alpha: f32,
}

/// Computes `([ndc_scale_x, ndc_scale_y, bx, by], [inv00, inv01, inv10, inv11])` from a
/// column-major 3x3 Smithay projection matrix `p`. Returns `None` if singular.
#[must_use]
pub(crate) fn invert_projection_2x2(p: &[f32; 9]) -> Option<([f32; 4], [f32; 4])> {
    // Column-major 3x3:
    // col0 = [p[0], p[1], p[2]], col1 = [p[3], p[4], p[5]], col2 = [p[6], p[7], p[8]]
    let a00 = p[0];
    let a01 = p[3];
    let a10 = p[1];
    let a11 = p[4];
    let det = a00 * a11 - a01 * a10;
    if det.abs() <= 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let ndc_scale_x = a00.hypot(a01);
    let ndc_scale_y = a10.hypot(a11);
    Some((
        [ndc_scale_x, ndc_scale_y, p[6], p[7]],
        [a11 * inv_det, -a01 * inv_det, -a10 * inv_det, a00 * inv_det],
    ))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn subtract_corner_cutouts(
    inner_opaque: OpaqueRegions<i32, Physical>,
    elem_geo: Rectangle<i32, Physical>,
    clip_rect_phys: Rectangle<i32, Physical>,
    clip_radius_phys: f32,
    extra_alpha: f32,
) -> OpaqueRegions<i32, Physical> {
    if extra_alpha < 0.999 {
        return OpaqueRegions::default();
    }
    if clip_radius_phys <= 0.5 || inner_opaque.is_empty() {
        return inner_opaque;
    }

    let r_cut = (clip_radius_phys * 1.42).ceil() as i32 + 2;
    let fx0 = clip_rect_phys.loc.x;
    let fy0 = clip_rect_phys.loc.y;
    let fx1 = fx0.saturating_add(clip_rect_phys.size.w.max(0));
    let fy1 = fy0.saturating_add(clip_rect_phys.size.h.max(0));

    let mut out = Vec::with_capacity(inner_opaque.len().saturating_mul(3));
    for loc_r in inner_opaque {
        let ox0 = elem_geo.loc.x.saturating_add(loc_r.loc.x);
        let oy0 = elem_geo.loc.y.saturating_add(loc_r.loc.y);
        let ox1 = ox0.saturating_add(loc_r.size.w);
        let oy1 = oy0.saturating_add(loc_r.size.h);
        if ox1 <= ox0 || oy1 <= oy0 {
            continue;
        }

        // Center body strip spanning outside top & bottom corner bands
        let mid_y0 = oy0.max(fy0.saturating_add(r_cut));
        let mid_y1 = oy1.min(fy1.saturating_sub(r_cut));
        if mid_y1 > mid_y0 {
            out.push(Rectangle::new(
                Point::from((ox0 - elem_geo.loc.x, mid_y0 - elem_geo.loc.y)),
                Size::from((ox1 - ox0, mid_y1 - mid_y0)),
            ));
        }

        // Top horizontal safe strip between top-left and top-right squircle cutouts
        let top_y1 = oy1.min(fy0.saturating_add(r_cut));
        if top_y1 > oy0 {
            let tx0 = ox0.max(fx0.saturating_add(r_cut));
            let tx1 = ox1.min(fx1.saturating_sub(r_cut));
            if tx1 > tx0 {
                out.push(Rectangle::new(
                    Point::from((tx0 - elem_geo.loc.x, oy0 - elem_geo.loc.y)),
                    Size::from((tx1 - tx0, top_y1 - oy0)),
                ));
            }
        }

        // Bottom horizontal safe strip between bottom-left and bottom-right squircle cutouts
        let bot_y0 = oy0.max(fy1.saturating_sub(r_cut));
        if oy1 > bot_y0 {
            let bx0 = ox0.max(fx0.saturating_add(r_cut));
            let bx1 = ox1.min(fx1.saturating_sub(r_cut));
            if bx1 > bx0 {
                out.push(Rectangle::new(
                    Point::from((bx0 - elem_geo.loc.x, bot_y0 - elem_geo.loc.y)),
                    Size::from((bx1 - bx0, oy1 - bot_y0)),
                ));
            }
        }
    }

    out.into_iter().collect()
}

impl Element for ClippedSurfaceElement {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }

    fn src(&self) -> Rectangle<f64, BufferCoord> {
        self.inner.src()
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        subtract_corner_cutouts(
            self.inner.opaque_regions(scale),
            self.inner.geometry(scale),
            self.clip_rect_phys,
            self.clip_radius_phys,
            self.extra_alpha,
        )
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha() * self.extra_alpha
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for ClippedSurfaceElement {
    #[allow(clippy::cast_precision_loss)]
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, BufferCoord>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let proj = invert_projection_2x2(frame.projection());
        let (ndc_scale_b, inv_a, clip_enabled) = match proj {
            Some((nsb, ia)) => (nsb, ia, 1.0_f32),
            None => ([0.0; 4], [0.0; 4], 0.0_f32),
        };

        let cw = (self.clip_rect_phys.size.w.max(1)) as f32;
        let ch = (self.clip_rect_phys.size.h.max(1)) as f32;
        let cx = self.clip_rect_phys.loc.x as f32 + cw * 0.5;
        let cy = self.clip_rect_phys.loc.y as f32 + ch * 0.5;

        let (use_solid, solid_color) = match self.inner.texture() {
            WaylandSurfaceTexture::Texture(_) => (0.0_f32, [0.0; 4]),
            WaylandSurfaceTexture::SolidColor(color) => {
                let c = *color * self.inner.alpha();
                (1.0_f32, [c.r(), c.g(), c.b(), c.a()])
            }
        };

        let uniforms = vec![
            Uniform::new(
                "u_ndc_scale_b",
                UniformValue::_4f(
                    ndc_scale_b[0],
                    ndc_scale_b[1],
                    ndc_scale_b[2],
                    ndc_scale_b[3],
                ),
            ),
            Uniform::new(
                "u_inv_a",
                UniformValue::_4f(inv_a[0], inv_a[1], inv_a[2], inv_a[3]),
            ),
            Uniform::new("u_clip_rect", UniformValue::_4f(cx, cy, cw * 0.5, ch * 0.5)),
            Uniform::new(
                "u_clip_params",
                UniformValue::_4f(
                    self.clip_radius_phys,
                    use_solid,
                    self.extra_alpha,
                    clip_enabled,
                ),
            ),
            Uniform::new(
                "u_solid_color",
                UniformValue::_4f(
                    solid_color[0],
                    solid_color[1],
                    solid_color[2],
                    solid_color[3],
                ),
            ),
        ];

        if use_solid > 0.5 {
            frame.clear_tex_program_override();
            frame.render_texture_from_to(
                &self.solid_carrier,
                Rectangle::from_size(Size::from((1.0, 1.0))),
                dst,
                damage,
                opaque_regions,
                Transform::Normal,
                1.0,
                Some(&self.program),
                &uniforms,
            )
        } else {
            frame.override_default_tex_program(self.program.clone(), uniforms);
            let r = self.inner.draw(frame, src, dst, damage, opaque_regions);
            frame.clear_tex_program_override();
            r
        }
    }

    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        // Returning `None` ensures windowed clients stay on GLES composition so the
        // squircle clip runs every frame while raw unclipped fullscreen windows preserve Direct Scanout.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mul_mat3_col(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
        let mut out = [0.0_f32; 9];
        for col in 0..3 {
            for row in 0..3 {
                out[col * 3 + row] =
                    a[row] * b[col * 3] + a[3 + row] * b[col * 3 + 1] + a[6 + row] * b[col * 3 + 2];
            }
        }
        out
    }

    fn eval_projection(p: &[f32; 9], x: f32, y: f32) -> (f32, f32) {
        (p[0] * x + p[3] * y + p[6], p[1] * x + p[4] * y + p[7])
    }

    fn unproject_fragcoord(
        p: &[f32; 9],
        vp_w: f32,
        vp_h: f32,
        x_phys: f32,
        y_phys: f32,
    ) -> Option<(f32, f32)> {
        let (ndc_x, ndc_y) = eval_projection(p, x_phys, y_phys);
        let frag_x = (ndc_x * 0.5 + 0.5) * vp_w;
        let frag_y = (ndc_y * 0.5 + 0.5) * vp_h;
        let (nsb, ia) = invert_projection_2x2(p)?;
        let rec_ndc_x = frag_x * nsb[0] - 1.0;
        let rec_ndc_y = frag_y * nsb[1] - 1.0;
        let dx = rec_ndc_x - nsb[2];
        let dy = rec_ndc_y - nsb[3];
        Some((ia[0] * dx + ia[1] * dy, ia[2] * dx + ia[3] * dy))
    }

    #[test]
    fn projection_roundtrip_recovers_physical_coords_across_all_transforms() {
        let orig_w = 1920_f32;
        let orig_h = 1080_f32;
        // Column-major linear transforms (col0, col1, col2) matching Smithay Transform::matrix()
        let cases: [(&str, f32, f32, [f32; 9]); 8] = [
            (
                "Normal",
                orig_w,
                orig_h,
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            ),
            (
                "90",
                orig_h,
                orig_w,
                [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            ),
            (
                "180",
                orig_w,
                orig_h,
                [-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0],
            ),
            (
                "270",
                orig_h,
                orig_w,
                [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            ),
            (
                "Flipped",
                orig_w,
                orig_h,
                [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            ),
            (
                "Flipped90",
                orig_h,
                orig_w,
                [0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            ),
            (
                "Flipped180",
                orig_w,
                orig_h,
                [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0],
            ),
            (
                "Flipped270",
                orig_h,
                orig_w,
                [0.0, -1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            ),
        ];
        let flip180: [f32; 9] = [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0];
        for (label, out_w, out_h, t_mat) in cases {
            let x = 2.0 / out_w;
            let y = 2.0 / out_h;
            let renderer: [f32; 9] = [x, 0.0, 0.0, 0.0, -y, 0.0, -1.0, 1.0, 1.0];
            let p = mul_mat3_col(&flip180, &mul_mat3_col(&t_mat, &renderer));
            let rec = unproject_fragcoord(&p, orig_w, orig_h, 317.5, 429.5);
            assert!(rec.is_some(), "singular projection for {label}");
            if let Some((rx, ry)) = rec {
                assert!(
                    (rx - 317.5).abs() < 1e-3 && (ry - 429.5).abs() < 1e-3,
                    "failed for {label}: got ({rx}, {ry})"
                );
            }
        }
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn subtract_corner_cutouts_removes_corners_and_keeps_center_opaque() {
        let clip_rect = Rectangle::new(Point::from((100, 100)), Size::from((800, 600)));
        let elem_geo = clip_rect;
        let inner = OpaqueRegions::from_slice(&[Rectangle::from_size(Size::from((800, 600)))]);
        let trimmed = subtract_corner_cutouts(inner, elem_geo, clip_rect, 14.0, 1.0);
        assert_eq!(trimmed.len(), 3);
        let r_cut = (14.0_f32 * 1.42).ceil() as i32 + 2;
        let tl_corner = Rectangle::new(Point::from((0, 0)), Size::from((r_cut, r_cut)));
        let br_corner = Rectangle::new(
            Point::from((800 - r_cut, 600 - r_cut)),
            Size::from((r_cut, r_cut)),
        );
        for r in trimmed.iter() {
            assert!(
                r.intersection(tl_corner).is_none(),
                "top-left corner must not be opaque: {r:?}"
            );
            assert!(
                r.intersection(br_corner).is_none(),
                "bottom-right corner must not be opaque: {r:?}"
            );
        }
        let inner2 = OpaqueRegions::from_slice(&[Rectangle::from_size(Size::from((800, 600)))]);
        assert!(subtract_corner_cutouts(inner2, elem_geo, clip_rect, 14.0, 0.5).is_empty());
    }
}
