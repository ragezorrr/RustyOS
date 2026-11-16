//! Hardware GLES Genie & Scale window minimize/restore warp shader (`RUSTYOS-073`).
//!
//! Captures an offscreen snapshot texture of a closing/minimizing window once at
//! minimize/restore start using Smithay's safe `OutputDamageTracker::render_output`
//! plus `GlesTexture`, then renders the curvilinear Genie funnel or smooth Scale morph
//! toward the target Dock icon rect using `GlesTexProgram` (`unsafe_code = "deny"`).

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::gles::element::TextureShaderElement;
use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType, UniformValue,
};
use smithay::backend::renderer::utils::DamageBag;
use smithay::backend::renderer::{Bind, Color32F, Offscreen, Renderer, Texture};
use smithay::utils::{
    Buffer as BufferCoord, Logical, Physical, Point, Rectangle, Scale, Size, Transform,
};
use tracing::warn;

use crate::config::{DockMinimizeEffect, DockPosition};
use crate::shell_render::CompositorRenderElement;

use super::LiquidGlassElement;

/// GLSL ES 1.00 fragment shader performing Dock `Genie` S-curve funneling or `Scale` morphing.
pub(crate) const WINDOW_MINIMIZE_FRAG: &str = r"#version 100

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

uniform vec4 u_win_uv;
uniform vec4 u_dock_uv;
uniform vec4 u_params;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

void main() {
    float t = clamp(u_params.x, 0.0, 1.0);
    bool is_genie = u_params.y > 0.5;
    float edge = u_params.z;

    float p_a;
    float p_c;
    vec2 win_a;
    vec2 win_c;
    vec2 dock_a;
    vec2 dock_c;

    if (edge < 0.5) {
        p_a = v_coords.y;
        p_c = v_coords.x;
        win_a = vec2(u_win_uv.y, u_win_uv.y + u_win_uv.w);
        win_c = vec2(u_win_uv.x, u_win_uv.x + u_win_uv.z);
        dock_a = vec2(u_dock_uv.y, u_dock_uv.y + u_dock_uv.w);
        dock_c = vec2(u_dock_uv.x, u_dock_uv.x + u_dock_uv.z);
    } else if (edge < 1.5) {
        p_a = 1.0 - v_coords.x;
        p_c = v_coords.y;
        win_a = vec2(1.0 - (u_win_uv.x + u_win_uv.z), 1.0 - u_win_uv.x);
        win_c = vec2(u_win_uv.y, u_win_uv.y + u_win_uv.w);
        dock_a = vec2(1.0 - (u_dock_uv.x + u_dock_uv.z), 1.0 - u_dock_uv.x);
        dock_c = vec2(u_dock_uv.y, u_dock_uv.y + u_dock_uv.w);
    } else {
        p_a = v_coords.x;
        p_c = v_coords.y;
        win_a = vec2(u_win_uv.x, u_win_uv.x + u_win_uv.z);
        win_c = vec2(u_win_uv.y, u_win_uv.y + u_win_uv.w);
        dock_a = vec2(u_dock_uv.x, u_dock_uv.x + u_dock_uv.z);
        dock_c = vec2(u_dock_uv.y, u_dock_uv.y + u_dock_uv.w);
    }

    float cur_a0;
    float cur_a1;
    if (is_genie) {
        float pull_back = smoothstep(0.12, 1.0, t);
        float pull_lead = smoothstep(0.0, 0.82, t);
        cur_a0 = mix(win_a.x, dock_a.x, pull_back);
        cur_a1 = mix(win_a.y, dock_a.y, pull_lead);
    } else {
        cur_a0 = mix(win_a.x, dock_a.x, t);
        cur_a1 = mix(win_a.y, dock_a.y, t);
    }

    float span_a = max(cur_a1 - cur_a0, 0.0005);
    float raw_v = (p_a - cur_a0) / span_a;
    if (raw_v < 0.0 || raw_v > 1.0) {
        gl_FragColor = vec4(0.0);
        return;
    }

    float s_curve = 0.0;
    if (is_genie) {
        float t_row = clamp(t * 1.55 - (1.0 - raw_v) * 0.55, 0.0, 1.0);
        s_curve = smoothstep(0.0, 1.0, t_row);
    } else {
        s_curve = t;
    }

    float row_c0 = mix(win_c.x, dock_c.x, s_curve);
    float row_c1 = mix(win_c.y, dock_c.y, s_curve);
    float span_c = max(row_c1 - row_c0, 0.0005);
    float uv_c = (p_c - row_c0) / span_c;
    if (uv_c < 0.0 || uv_c > 1.0) {
        gl_FragColor = vec4(0.0);
        return;
    }

    vec2 sample_uv;
    if (edge < 0.5) {
        sample_uv = vec2(uv_c, raw_v);
    } else if (edge < 1.5) {
        sample_uv = vec2(1.0 - raw_v, uv_c);
    } else {
        sample_uv = vec2(raw_v, uv_c);
    }

    vec2 tex_clamped = clamp(sample_uv, vec2(0.001), vec2(0.999));
    vec4 color = texture2D(tex, tex_clamped);
#if defined(NO_ALPHA)
    color.a = 1.0;
#endif

    float aa_a = min(smoothstep(0.0, 0.004, raw_v), smoothstep(1.0, 0.996, raw_v));
    float aa_c = min(smoothstep(0.0, 0.004, uv_c), smoothstep(1.0, 0.996, uv_c));
    float end_fade = 1.0 - smoothstep(0.88, 1.0, t);
    float out_alpha = color.a * aa_a * aa_c * end_fade * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0) {
        color.rgb = vec3(0.0, 0.2, 0.0) + color.rgb * 0.8;
    }
#endif

    gl_FragColor = vec4(color.rgb * out_alpha, out_alpha);
}
";

type WarpFrameKey = (u32, u8, u8, [i32; 8]);

/// Cached snapshot texture and incremental damage state for an active minimize/restore animation (`RUSTYOS-073`).
pub(crate) struct MinimizedWindowSnapshot {
    pub texture: GlesTexture,
    pub stable_id: Id,
    pub damage: DamageBag<i32, BufferCoord>,
    pub last_anim_key: Option<WarpFrameKey>,
}

impl std::fmt::Debug for MinimizedWindowSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MinimizedWindowSnapshot")
            .field("size", &self.texture.size())
            .finish_non_exhaustive()
    }
}

/// Manages the offscreen window snapshot textures and warp shader for `Genie`/`Scale` animations.
#[derive(Default)]
pub(crate) struct WindowMinimizePipeline {
    program: Option<GlesTexProgram>,
    compile_failed: bool,
    snapshots: HashMap<u64, MinimizedWindowSnapshot>,
}

impl std::fmt::Debug for WindowMinimizePipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowMinimizePipeline")
            .field("has_program", &self.program.is_some())
            .field("compile_failed", &self.compile_failed)
            .field("snapshot_count", &self.snapshots.len())
            .finish()
    }
}

impl WindowMinimizePipeline {
    pub(crate) fn ensure_program(&mut self, renderer: &mut GlesRenderer) -> bool {
        if self.program.is_some() {
            return true;
        }
        if self.compile_failed {
            return false;
        }
        let uniforms = [
            UniformName::new("u_win_uv", UniformType::_4f),
            UniformName::new("u_dock_uv", UniformType::_4f),
            UniformName::new("u_params", UniformType::_4f),
        ];
        match renderer.compile_custom_texture_shader(WINDOW_MINIMIZE_FRAG, &uniforms) {
            Ok(prog) => {
                self.program = Some(prog);
                true
            }
            Err(err) => {
                warn!("Failed to compile Dock minimize GLES shader ({err}); using box fallback");
                self.compile_failed = true;
                false
            }
        }
    }

    /// Captures window-local `elements` (origin `(0, 0)`, logical size `win_size`) into an offscreen [`GlesTexture`].
    pub(crate) fn capture_snapshot(
        &mut self,
        renderer: &mut GlesRenderer,
        window_id: u64,
        win_size: Size<i32, Logical>,
        output_scale: f64,
        local_elements: &[CompositorRenderElement],
    ) -> bool {
        if win_size.w <= 0 || win_size.h <= 0 || !self.ensure_program(renderer) {
            return false;
        }
        let clamped_scale = crate::config::clamp_display_scale(output_scale);
        #[allow(clippy::cast_possible_truncation)]
        let phys_w = (f64::from(win_size.w) * clamped_scale).round().max(1.0) as i32;
        #[allow(clippy::cast_possible_truncation)]
        let phys_h = (f64::from(win_size.h) * clamped_scale).round().max(1.0) as i32;
        let phys_size = Size::<i32, Physical>::from((phys_w, phys_h));
        let buf_size = Size::<i32, BufferCoord>::from((phys_w, phys_h));

        let reusable = self
            .snapshots
            .remove(&window_id)
            .filter(|s| s.texture.size() == buf_size);
        let (mut tex, stable_id, mut damage) = if let Some(mut old) = reusable {
            old.damage.reset();
            (old.texture, old.stable_id, old.damage)
        } else {
            let tex = match renderer.create_buffer(Fourcc::Abgr8888, buf_size) {
                Ok(t) => t,
                Err(err) => {
                    warn!("Failed to allocate minimize snapshot FBO for win {window_id}: {err}");
                    return false;
                }
            };
            (tex, Id::new(), DamageBag::new(8))
        };

        let mut tracker =
            OutputDamageTracker::new(phys_size, Scale::from(clamped_scale), Transform::Normal);
        let render_ok = (|| -> Result<(), String> {
            let mut fb = renderer
                .bind(&mut tex)
                .map_err(|e| format!("bind snapshot FBO: {e}"))?;
            tracker
                .render_output(renderer, &mut fb, 0, local_elements, Color32F::TRANSPARENT)
                .map_err(|e| format!("render minimize snapshot: {e}"))?;
            Ok(())
        })();
        if let Err(err) = render_ok {
            warn!("Minimize snapshot render failed for win {window_id}: {err}");
            return false;
        }

        damage.add([Rectangle::from_size(buf_size)]);
        self.snapshots.insert(
            window_id,
            MinimizedWindowSnapshot {
                texture: tex,
                stable_id,
                damage,
                last_anim_key: None,
            },
        );
        true
    }

    /// Returns `true` if a GPU snapshot exists for `window_id`.
    #[must_use]
    pub(crate) fn has_snapshot(&self, window_id: u64) -> bool {
        self.snapshots.contains_key(&window_id)
    }

    /// Retains only cached GPU snapshots matching `keep` (`RUSTYOS-073`).
    pub(crate) fn retain_snapshots(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.snapshots.retain(|&win_id, _| keep(win_id));
    }

    /// Emits a warp shader element interpolating between `win_rect` and `dock_rect` (`progress`: `0..=1`).
    #[allow(
        clippy::too_many_arguments,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub(crate) fn make_warp_element(
        &mut self,
        renderer: &mut GlesRenderer,
        window_id: u64,
        win_rect: Rectangle<i32, Logical>,
        dock_rect: Rectangle<i32, Logical>,
        progress: f64,
        effect: DockMinimizeEffect,
        position: DockPosition,
    ) -> Option<LiquidGlassElement> {
        let program = self.program.as_ref()?.clone();
        let snap = self.snapshots.get_mut(&window_id)?;
        let (bounds, win_uv, dock_uv) = compute_warp_bounds_and_uvs(win_rect, dock_rect);
        let edge_u8: u8 = match position {
            DockPosition::Bottom => 0,
            DockPosition::Left => 1,
            DockPosition::Right => 2,
        };
        let is_genie = effect == DockMinimizeEffect::Genie;
        let clamped_t = progress.clamp(0.0, 1.0);
        let progress_q = (clamped_t * 8192.0).round() as u32;
        let cur_key: WarpFrameKey = (
            progress_q,
            u8::from(is_genie),
            edge_u8,
            [
                win_rect.loc.x,
                win_rect.loc.y,
                win_rect.size.w,
                win_rect.size.h,
                dock_rect.loc.x,
                dock_rect.loc.y,
                dock_rect.size.w,
                dock_rect.size.h,
            ],
        );
        let tex_size = snap.texture.size();
        if snap.last_anim_key != Some(cur_key) {
            snap.last_anim_key = Some(cur_key);
            snap.damage.add([Rectangle::from_size(tex_size)]);
        }

        let uniforms = vec![
            Uniform::new(
                "u_win_uv",
                UniformValue::_4f(win_uv[0], win_uv[1], win_uv[2], win_uv[3]),
            ),
            Uniform::new(
                "u_dock_uv",
                UniformValue::_4f(dock_uv[0], dock_uv[1], dock_uv[2], dock_uv[3]),
            ),
            Uniform::new(
                "u_params",
                UniformValue::_4f(
                    clamped_t as f32,
                    if is_genie { 1.0 } else { 0.0 },
                    f32::from(edge_u8),
                    0.0,
                ),
            ),
        ];

        let inner = TextureRenderElement::from_texture_with_damage(
            snap.stable_id.clone(),
            renderer.context_id(),
            Point::from((f64::from(bounds.loc.x), f64::from(bounds.loc.y))),
            snap.texture.clone(),
            1,
            Transform::Normal,
            Some(1.0),
            Some(Rectangle::from_size(Size::from((
                f64::from(tex_size.w.max(1)),
                f64::from(tex_size.h.max(1)),
            )))),
            Some(Size::from((bounds.size.w.max(1), bounds.size.h.max(1)))),
            None,
            snap.damage.snapshot(),
            Kind::Unspecified,
        );
        let shader = TextureShaderElement::new(inner, program, uniforms);
        Some(LiquidGlassElement::from_parts(
            snap.stable_id.clone(),
            shader,
        ))
    }
}

/// Computes the union quad (`bounds`) and normalized UV sub-rectangles for `win_rect` and `dock_rect`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub(crate) fn compute_warp_bounds_and_uvs(
    win_rect: Rectangle<i32, Logical>,
    dock_rect: Rectangle<i32, Logical>,
) -> (Rectangle<i32, Logical>, [f32; 4], [f32; 4]) {
    let bx0 = win_rect.loc.x.min(dock_rect.loc.x);
    let by0 = win_rect.loc.y.min(dock_rect.loc.y);
    let bx1 = (win_rect.loc.x + win_rect.size.w).max(dock_rect.loc.x + dock_rect.size.w);
    let by1 = (win_rect.loc.y + win_rect.size.h).max(dock_rect.loc.y + dock_rect.size.h);
    let bw = (bx1 - bx0).max(1);
    let bh = (by1 - by0).max(1);
    let bounds = Rectangle::new(Point::from((bx0, by0)), Size::from((bw, bh)));

    let bounds_w = bw as f32;
    let bounds_h = bh as f32;
    let win_uv = [
        (win_rect.loc.x - bx0) as f32 / bounds_w,
        (win_rect.loc.y - by0) as f32 / bounds_h,
        win_rect.size.w.max(1) as f32 / bounds_w,
        win_rect.size.h.max(1) as f32 / bounds_h,
    ];
    let dock_uv = [
        (dock_rect.loc.x - bx0) as f32 / bounds_w,
        (dock_rect.loc.y - by0) as f32 / bounds_h,
        dock_rect.size.w.max(1) as f32 / bounds_w,
        dock_rect.size.h.max(1) as f32 / bounds_h,
    ];
    (bounds, win_uv, dock_uv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimize_shader_meets_smithay_contract() {
        assert!(WINDOW_MINIMIZE_FRAG.starts_with("#version 100\n"));
        assert!(WINDOW_MINIMIZE_FRAG.contains("//_DEFINES_"));
        assert!(WINDOW_MINIMIZE_FRAG.contains("#if defined(EXTERNAL)"));
        assert!(WINDOW_MINIMIZE_FRAG.contains("#if defined(NO_ALPHA)"));
        assert!(WINDOW_MINIMIZE_FRAG.contains("#if defined(DEBUG_FLAGS)"));
        assert!(WINDOW_MINIMIZE_FRAG.contains("u_win_uv"));
        assert!(WINDOW_MINIMIZE_FRAG.contains("u_dock_uv"));
        assert!(WINDOW_MINIMIZE_FRAG.contains("u_params"));
    }

    #[test]
    fn warp_bounds_and_uvs_cover_both_window_and_dock_rects() {
        let win = Rectangle::new((120, 80).into(), (640, 480).into());
        let dock = Rectangle::new((420, 730).into(), (52, 52).into());
        let (bounds, win_uv, dock_uv) = compute_warp_bounds_and_uvs(win, dock);
        assert_eq!(bounds, Rectangle::new((120, 80).into(), (640, 702).into()));
        assert!((win_uv[0] - 0.0).abs() < 1e-4);
        assert!((win_uv[1] - 0.0).abs() < 1e-4);
        assert!(dock_uv[0] > 0.4 && dock_uv[1] > 0.9);
    }
}
