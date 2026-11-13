//! Signed-distance vector glyphs (`4x4` SSAA) for `TopBar`, Control Center, and Lock Screen.

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::{Size, Transform};

use super::UiGlyphKind;
use super::sdf::{
    blend_premul_over, premul_tint, sd_arc_up, sd_circle, sd_cov, sd_pie_up, sd_rounded_box,
    sd_segment, signed_distance_polygon,
};

pub(super) fn sample_rusty_logo_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    // Original sleek chamfered hexagonal crest for RustyOS, pure monochrome with transparent inner cutout.
    const CREST: [(f32, f32); 6] = [
        (0.50, 0.10),
        (0.84, 0.29),
        (0.84, 0.71),
        (0.50, 0.90),
        (0.16, 0.71),
        (0.16, 0.29),
    ];
    let outer = signed_distance_polygon(u, v, &CREST) - 0.026;
    let inner_cut = sd_circle(u, v, 0.50, 0.50, 0.140);
    let sd = outer.max(-inner_cut);
    let cov = sd_cov(sd, edge);
    premul_tint(tint, cov)
}

pub(super) fn sample_wifi_glyph_uv(
    u: f32,
    v: f32,
    edge: f32,
    tint: [f32; 4],
    connected: bool,
    signal_percent: Option<u8>,
) -> [f32; 4] {
    // macOS 1:1 3-band Wi-Fi fan: rounded wedge anchor (band 1) + 2 wide concentric arcs (bands 2 & 3).
    let cx = 0.50;
    let cy = 0.785;
    let half_angle = 0.805_f32;
    let stroke = 0.064;

    let sd_b1 = sd_pie_up(u, v, cx, cy, 0.132, 0.75, 0.026);
    let sd_b2 = sd_arc_up(u, v, cx, cy, 0.315, half_angle, stroke);
    let sd_b3 = sd_arc_up(u, v, cx, cy, 0.530, half_angle, stroke);

    let active_bars = rusty_services::wifi_signal_bars(connected, signal_percent);
    let mut out = [0.0_f32; 4];
    for (band_idx, band_sd) in [(1_u8, sd_b1), (2, sd_b2), (3, sd_b3)] {
        let cov = sd_cov(band_sd, edge);
        if cov <= 0.0 {
            continue;
        }
        let band_opacity = if !connected {
            0.38
        } else if band_idx <= active_bars {
            1.0
        } else {
            0.28
        };
        out = blend_premul_over(
            out,
            [tint[0], tint[1], tint[2]],
            tint[3] * cov * band_opacity,
        );
    }

    if connected {
        return out;
    }
    let slash_cov = sd_cov(sd_segment(u, v, 0.20, 0.20, 0.80, 0.80, 0.044), edge);
    blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * slash_cov)
}

pub(super) fn sample_volume_glyph_uv(
    u: f32,
    v: f32,
    edge: f32,
    tint: [f32; 4],
    level_percent: u8,
    muted: bool,
) -> [f32; 4] {
    const HORN: [(f32, f32); 4] = [(0.24, 0.36), (0.46, 0.21), (0.46, 0.79), (0.24, 0.64)];
    let body = sd_rounded_box(u, v, 0.21, 0.50, 0.075, 0.13, 0.03);
    let cone = signed_distance_polygon(u, v, &HORN);
    let mut speaker_sd = body.min(cone);

    if !muted {
        let dx = u - 0.48;
        let dy = (v - 0.50).abs();
        let dist = dx.hypot(dy);
        let cone_sd = (dy - dx * 1.10).max(-dx + 0.05);
        if level_percent > 0 {
            speaker_sd = speaker_sd.min(((dist - 0.18).abs() - 0.042).max(cone_sd));
        }
        if level_percent > 48 {
            speaker_sd = speaker_sd.min(((dist - 0.32).abs() - 0.042).max(cone_sd));
        }
    }

    let speaker_cov = sd_cov(speaker_sd, edge);
    if muted {
        let base = premul_tint([tint[0], tint[1], tint[2], tint[3] * 0.55], speaker_cov);
        let slash_cov = sd_cov(sd_segment(u, v, 0.20, 0.20, 0.82, 0.80, 0.046), edge);
        return blend_premul_over(base, [248.0, 113.0, 113.0], tint[3] * slash_cov);
    }
    premul_tint(tint, speaker_cov)
}

pub(super) fn sample_battery_glyph_uv(
    u_iso: f32,
    v: f32,
    aspect: f32,
    edge: f32,
    tint: [f32; 4],
    percent: u8,
    charging: bool,
) -> [f32; 4] {
    // 1:1 macOS 27 battery: smooth pill silhouette (solid at 100%, translucent track when partial),
    // separated soft terminal nub, and white tilted lightning bolt with transparent cutout halo.
    let cx = aspect * 0.445;
    let hw = aspect * 0.388;
    let hh = 0.305;
    let body_outer = sd_rounded_box(u_iso, v, cx, 0.50, hw, hh, 0.165);
    let nub = sd_rounded_box(u_iso, v, cx + hw + 0.072, 0.50, 0.030, 0.098, 0.028);

    let mut out = [0.0_f32; 4];
    let body_cov = sd_cov(body_outer, edge);
    if body_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * body_cov * 0.32);
    }

    let frac = if percent >= 98 {
        1.0_f32
    } else {
        f32::from(percent.clamp(5, 100)) / 100.0
    };
    if frac < 1.0 {
        let rim_cov = sd_cov(body_outer.abs() - 0.026, edge);
        if rim_cov > 0.0 {
            out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * rim_cov * 0.55);
        }
    }

    let nub_cov = sd_cov(nub, edge);
    if nub_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * nub_cov * 0.68);
    }

    let fill_sd = if frac >= 1.0 {
        body_outer
    } else {
        let fill_right = (cx - hw) + 2.0 * hw * frac;
        body_outer.max(u_iso - fill_right)
    };
    let fill_cov = sd_cov(fill_sd, edge);
    if fill_cov > 0.0 {
        let fill_rgb = if !charging && percent <= 20 {
            [248.0_f32, 113.0, 113.0]
        } else {
            [tint[0], tint[1], tint[2]]
        };
        out = blend_premul_over(out, fill_rgb, tint[3] * fill_cov * 0.94);
    }

    if charging {
        let upper: [(f32, f32); 3] = [
            (cx + 0.075, 0.155),
            (cx - 0.185, 0.535),
            (cx + 0.040, 0.535),
        ];
        let lower: [(f32, f32); 3] = [
            (cx - 0.075, 0.845),
            (cx + 0.185, 0.465),
            (cx - 0.040, 0.465),
        ];
        let bolt_sd = signed_distance_polygon(u_iso, v, &upper)
            .min(signed_distance_polygon(u_iso, v, &lower))
            - 0.013;
        let halo_cov = sd_cov(bolt_sd - 0.042, edge * 0.85);
        if halo_cov > 0.0 {
            let keep = 1.0 - halo_cov;
            out = [out[0] * keep, out[1] * keep, out[2] * keep, out[3] * keep];
        }
        let bolt_cov = sd_cov(bolt_sd, edge * 0.85);
        if bolt_cov > 0.0 {
            out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * bolt_cov);
        }
    }
    out
}

pub(super) fn sample_search_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let ring = sd_circle(u, v, 0.42, 0.42, 0.255).abs() - 0.062;
    let handle = sd_segment(u, v, 0.61, 0.61, 0.87, 0.87, 0.064);
    let cov = sd_cov(ring.min(handle), edge);
    premul_tint(tint, cov)
}

pub(super) fn sample_control_center_glyph_uv(
    u: f32,
    v: f32,
    edge: f32,
    tint: [f32; 4],
) -> [f32; 4] {
    // macOS 1:1 Control Center: top hollow outline pill + outline knob on left; bottom solid pill + cutout knob on right.
    let top_capsule = sd_rounded_box(u, v, 0.50, 0.310, 0.37, 0.140, 0.140);
    let top_ring = top_capsule.abs() - 0.028;
    let top_knob_ring = sd_circle(u, v, 0.310, 0.310, 0.112).abs() - 0.028;

    let bot_capsule = sd_rounded_box(u, v, 0.50, 0.690, 0.37, 0.140, 0.140);
    let bot_knob_cut = sd_circle(u, v, 0.678, 0.690, 0.088);

    let mut out = [0.0_f32; 4];
    let top_cov = sd_cov(top_ring.min(top_knob_ring), edge);
    if top_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * top_cov * 0.92);
    }

    let bot_fill_sd = bot_capsule.max(-bot_knob_cut);
    let bot_cov = sd_cov(bot_fill_sd, edge);
    if bot_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * bot_cov * 0.95);
    }
    out
}

pub(super) fn sample_info_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let ring = sd_circle(u, v, 0.50, 0.50, 0.36).abs() - 0.044;
    let dot = sd_circle(u, v, 0.50, 0.31, 0.048);
    let stem = sd_segment(u, v, 0.50, 0.44, 0.50, 0.69, 0.046);
    premul_tint(tint, sd_cov(ring.min(dot).min(stem), edge))
}

pub(super) fn sample_terminal_prompt_glyph_uv(
    u: f32,
    v: f32,
    edge: f32,
    tint: [f32; 4],
) -> [f32; 4] {
    let frame = sd_rounded_box(u, v, 0.50, 0.50, 0.37, 0.31, 0.10).abs() - 0.042;
    let chev_top = sd_segment(u, v, 0.27, 0.37, 0.41, 0.50, 0.045);
    let chev_bot = sd_segment(u, v, 0.41, 0.50, 0.27, 0.63, 0.045);
    let dash = sd_segment(u, v, 0.48, 0.63, 0.72, 0.63, 0.045);
    premul_tint(
        tint,
        sd_cov(frame.min(chev_top).min(chev_bot).min(dash), edge),
    )
}

pub(super) fn sample_folder_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let body = sd_rounded_box(u, v, 0.50, 0.57, 0.37, 0.25, 0.08).abs() - 0.042;
    let tab = sd_rounded_box(u, v, 0.31, 0.30, 0.17, 0.08, 0.05);
    let body_fill = sd_rounded_box(u, v, 0.50, 0.57, 0.37, 0.25, 0.08);
    let mut out = [0.0_f32; 4];
    let fill_cov = sd_cov(body_fill.min(tab), edge);
    if fill_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * fill_cov * 0.24);
    }
    let rim_cov = sd_cov(body.min(tab.abs() - 0.036), edge);
    if rim_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * rim_cov * 0.96);
    }
    out
}

pub(super) fn sample_grid_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let tl = sd_rounded_box(u, v, 0.31, 0.31, 0.125, 0.125, 0.045);
    let tr = sd_rounded_box(u, v, 0.69, 0.31, 0.125, 0.125, 0.045);
    let bl = sd_rounded_box(u, v, 0.31, 0.69, 0.125, 0.125, 0.045);
    let br = sd_rounded_box(u, v, 0.69, 0.69, 0.125, 0.125, 0.045);
    let cov = sd_cov(tl.min(tr).min(bl).min(br), edge);
    premul_tint(tint, cov)
}

pub(super) fn sample_layout_split_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let frame = sd_rounded_box(u, v, 0.50, 0.50, 0.36, 0.29, 0.09).abs() - 0.044;
    let divider = sd_segment(u, v, 0.50, 0.22, 0.50, 0.78, 0.040);
    let left_pane = sd_rounded_box(u, v, 0.33, 0.50, 0.11, 0.20, 0.04);
    let mut out = [0.0_f32; 4];
    let fill_cov = sd_cov(left_pane, edge);
    if fill_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * fill_cov * 0.45);
    }
    let rim_cov = sd_cov(frame.min(divider), edge);
    if rim_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * rim_cov);
    }
    out
}

pub(super) fn sample_motion_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let back = sd_circle(u, v, 0.32, 0.58, 0.17).abs() - 0.036;
    let mid = sd_circle(u, v, 0.48, 0.50, 0.19).abs() - 0.040;
    let front = sd_circle(u, v, 0.66, 0.42, 0.20);
    let mut out = [0.0_f32; 4];
    let back_cov = sd_cov(back, edge);
    if back_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * back_cov * 0.45);
    }
    let mid_cov = sd_cov(mid, edge);
    if mid_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * mid_cov * 0.72);
    }
    let front_cov = sd_cov(front, edge);
    if front_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * front_cov);
    }
    out
}

pub(super) fn sample_transparency_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let back = sd_rounded_box(u, v, 0.39, 0.39, 0.22, 0.22, 0.07).abs() - 0.042;
    let front_fill = sd_rounded_box(u, v, 0.61, 0.61, 0.22, 0.22, 0.07);
    let front_rim = front_fill.abs() - 0.042;
    let mut out = [0.0_f32; 4];
    let back_cov = sd_cov(back, edge);
    if back_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * back_cov * 0.65);
    }
    let fill_cov = sd_cov(front_fill, edge);
    if fill_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * fill_cov * 0.36);
    }
    let rim_cov = sd_cov(front_rim, edge);
    if rim_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * rim_cov);
    }
    out
}

pub(super) fn sample_refresh_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let dx = u - 0.50;
    let dy = v - 0.52;
    let ring = (dx.hypot(dy) - 0.29).abs() - 0.052;
    // Open upper-right gap between top and 3 o'clock before the arrowhead.
    let gap = u8::from(dx > 0.04 && dy < -0.02 && dy > -0.26);
    let arc = if gap == 1 { 0.2 } else { ring };
    let head = signed_distance_polygon(u, v, &[(0.52, 0.14), (0.76, 0.26), (0.55, 0.38)]);
    premul_tint(tint, sd_cov(arc.min(head), edge))
}

pub(super) fn sample_lock_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let shackle_arc = sd_arc_up(u, v, 0.50, 0.36, 0.15, core::f32::consts::FRAC_PI_2, 0.048);
    let shackle_l = sd_segment(u, v, 0.35, 0.36, 0.35, 0.48, 0.048);
    let shackle_r = sd_segment(u, v, 0.65, 0.36, 0.65, 0.48, 0.048);
    let body = sd_rounded_box(u, v, 0.50, 0.65, 0.27, 0.20, 0.07);
    let keyhole = sd_circle(u, v, 0.50, 0.64, 0.055);
    let body_with_hole = body.max(-keyhole);
    let d = shackle_arc
        .min(shackle_l)
        .min(shackle_r)
        .min(body_with_hole);
    premul_tint(tint, sd_cov(d, edge))
}

pub(super) fn sample_shortcuts_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let bars = sd_segment(u, v, 0.36, 0.24, 0.36, 0.76, 0.042)
        .min(sd_segment(u, v, 0.64, 0.24, 0.64, 0.76, 0.042))
        .min(sd_segment(u, v, 0.24, 0.36, 0.76, 0.36, 0.042))
        .min(sd_segment(u, v, 0.24, 0.64, 0.76, 0.64, 0.042));
    let loops = (sd_circle(u, v, 0.26, 0.26, 0.10).abs() - 0.040)
        .min(sd_circle(u, v, 0.74, 0.26, 0.10).abs() - 0.040)
        .min(sd_circle(u, v, 0.26, 0.74, 0.10).abs() - 0.040)
        .min(sd_circle(u, v, 0.74, 0.74, 0.10).abs() - 0.040);
    premul_tint(tint, sd_cov(bars.min(loops), edge))
}

pub(super) fn sample_window_max_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let frame = sd_rounded_box(u, v, 0.50, 0.50, 0.35, 0.29, 0.09).abs() - 0.042;
    let diag1 = sd_segment(u, v, 0.33, 0.67, 0.67, 0.33, 0.042);
    let diag2 = sd_segment(u, v, 0.52, 0.33, 0.67, 0.33, 0.042);
    let diag3 = sd_segment(u, v, 0.67, 0.33, 0.67, 0.48, 0.042);
    premul_tint(tint, sd_cov(frame.min(diag1).min(diag2).min(diag3), edge))
}

pub(super) fn sample_window_min_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let frame = sd_rounded_box(u, v, 0.50, 0.50, 0.35, 0.29, 0.09).abs() - 0.042;
    let dash = sd_segment(u, v, 0.32, 0.64, 0.68, 0.64, 0.046);
    premul_tint(tint, sd_cov(frame.min(dash), edge))
}

pub(super) fn sample_close_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let d1 = sd_segment(u, v, 0.28, 0.28, 0.72, 0.72, 0.062);
    let d2 = sd_segment(u, v, 0.72, 0.28, 0.28, 0.72, 0.062);
    premul_tint(tint, sd_cov(d1.min(d2), edge))
}

pub(super) fn sample_arrow_glyph_uv(
    u: f32,
    v: f32,
    edge: f32,
    tint: [f32; 4],
    forward: bool,
) -> [f32; 4] {
    let x = if forward { u } else { 1.0 - u };
    let shaft = sd_segment(x, v, 0.20, 0.50, 0.78, 0.50, 0.052);
    let tip_top = sd_segment(x, v, 0.55, 0.27, 0.78, 0.50, 0.052);
    let tip_bot = sd_segment(x, v, 0.55, 0.73, 0.78, 0.50, 0.052);
    premul_tint(tint, sd_cov(shaft.min(tip_top).min(tip_bot), edge))
}

pub(super) fn sample_bluetooth_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let spine = sd_segment(u, v, 0.48, 0.16, 0.48, 0.84, 0.048);
    let top_r = sd_segment(u, v, 0.48, 0.16, 0.69, 0.34, 0.046);
    let cross1 = sd_segment(u, v, 0.69, 0.34, 0.29, 0.68, 0.046);
    let bot_r = sd_segment(u, v, 0.48, 0.84, 0.69, 0.66, 0.046);
    let cross2 = sd_segment(u, v, 0.69, 0.66, 0.29, 0.32, 0.046);
    let d = spine.min(top_r).min(cross1).min(bot_r).min(cross2);
    premul_tint(tint, sd_cov(d, edge))
}

pub(super) fn sample_sun_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let core = sd_circle(u, v, 0.50, 0.50, 0.175);
    let mut rays = f32::MAX;
    for i in 0_i8..8 {
        let angle = f32::from(i) * core::f32::consts::FRAC_PI_4;
        let (sa, ca) = angle.sin_cos();
        let ray = sd_segment(
            u,
            v,
            0.50 + ca * 0.27,
            0.50 + sa * 0.27,
            0.50 + ca * 0.39,
            0.50 + sa * 0.39,
            0.042,
        );
        rays = rays.min(ray);
    }
    premul_tint(tint, sd_cov(core.min(rays), edge))
}

pub(super) fn sample_media_play_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let tri = signed_distance_polygon(u, v, &[(0.28, 0.20), (0.80, 0.50), (0.28, 0.80)]) - 0.028;
    premul_tint(tint, sd_cov(tri, edge))
}

pub(super) fn sample_media_pause_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let left = sd_rounded_box(u, v, 0.34, 0.50, 0.088, 0.27, 0.032);
    let right = sd_rounded_box(u, v, 0.66, 0.50, 0.088, 0.27, 0.032);
    premul_tint(tint, sd_cov(left.min(right), edge))
}

pub(super) fn sample_media_seek_glyph_uv(
    u: f32,
    v: f32,
    edge: f32,
    tint: [f32; 4],
    forward: bool,
) -> [f32; 4] {
    let x = if forward { u } else { 1.0 - u };
    let first = signed_distance_polygon(x, v, &[(0.14, 0.27), (0.48, 0.50), (0.14, 0.73)]) - 0.022;
    let second = signed_distance_polygon(x, v, &[(0.49, 0.27), (0.84, 0.50), (0.49, 0.73)]) - 0.022;
    premul_tint(tint, sd_cov(first.min(second), edge))
}

pub(super) fn sample_music_note_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let head1 = sd_circle(u, v, 0.31, 0.71, 0.115);
    let head2 = sd_circle(u, v, 0.67, 0.65, 0.115);
    let stem1 = sd_segment(u, v, 0.40, 0.27, 0.40, 0.70, 0.042);
    let stem2 = sd_segment(u, v, 0.76, 0.21, 0.76, 0.64, 0.042);
    let beam = sd_segment(u, v, 0.40, 0.27, 0.76, 0.21, 0.056);
    let d = head1.min(head2).min(stem1).min(stem2).min(beam);
    premul_tint(tint, sd_cov(d, edge))
}

pub(super) fn sample_moon_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let outer = sd_circle(u, v, 0.47, 0.52, 0.30);
    let cut = sd_circle(u, v, 0.61, 0.40, 0.24);
    premul_tint(tint, sd_cov(outer.max(-cut), edge))
}

pub(super) fn sample_overview_spaces_glyph_uv(
    u: f32,
    v: f32,
    edge: f32,
    tint: [f32; 4],
) -> [f32; 4] {
    let back = sd_rounded_box(u, v, 0.39, 0.41, 0.23, 0.18, 0.065);
    let back_rim = back.abs() - 0.038;
    let front = sd_rounded_box(u, v, 0.60, 0.59, 0.23, 0.18, 0.065);
    let front_rim = front.abs() - 0.042;
    let masked_back = back_rim.max(-front);
    let mut out = [0.0_f32; 4];
    let back_cov = sd_cov(masked_back, edge);
    if back_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * back_cov * 0.72);
    }
    let front_cov = sd_cov(front_rim, edge);
    if front_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * front_cov);
    }
    out
}

pub(super) fn sample_keyboard_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let frame = sd_rounded_box(u, v, 0.50, 0.50, 0.43, 0.28, 0.085);
    let rim = frame.abs() - 0.036;
    let mut keys = 1e6_f32;
    for row_y in [0.38_f32, 0.50_f32] {
        for col_x in [0.22_f32, 0.36_f32, 0.50_f32, 0.64_f32, 0.78_f32] {
            keys = keys.min(sd_rounded_box(u, v, col_x, row_y, 0.036, 0.032, 0.014));
        }
    }
    let space = sd_rounded_box(u, v, 0.50, 0.63, 0.20, 0.034, 0.016);
    let d = rim.min(keys).min(space);
    premul_tint(tint, sd_cov(d, edge))
}

pub(super) fn sample_power_symbol_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let dx = u - 0.50;
    let dy = v - 0.53;
    let ring = (dx.hypot(dy) - 0.28).abs() - 0.044;
    let top_notch = sd_rounded_box(u, v, 0.50, 0.27, 0.14, 0.14, 0.02);
    let arc = ring.max(-top_notch);
    let stem = sd_segment(u, v, 0.50, 0.16, 0.50, 0.47, 0.048);
    premul_tint(tint, sd_cov(arc.min(stem), edge))
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
pub(super) fn sample_ui_glyph_pixel(
    px: i32,
    py: i32,
    size_px: i32,
    kind: UiGlyphKind,
    tint_rgba: [u8; 4],
) -> [u8; 4] {
    sample_ui_glyph_pixel_rect(px, py, size_px, size_px, kind, tint_rgba)
}

#[allow(clippy::cast_precision_loss)]
pub(super) fn sample_ui_glyph_pixel_rect(
    px: i32,
    py: i32,
    width_px: i32,
    height_px: i32,
    kind: UiGlyphKind,
    tint_rgba: [u8; 4],
) -> [u8; 4] {
    const SUBSAMPLES: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
    let dim_h = height_px.max(1) as f32;
    let dim_w = width_px.max(1) as f32;
    let aspect = dim_w / dim_h;
    let edge = 1.05 / dim_h;
    let tint = [
        f32::from(tint_rgba[0]),
        f32::from(tint_rgba[1]),
        f32::from(tint_rgba[2]),
        f32::from(tint_rgba[3]) / 255.0,
    ];
    let mut sum = [0.0_f32; 4];
    for sy in SUBSAMPLES {
        for sx in SUBSAMPLES {
            let u_iso = (px as f32 + sx) / dim_h;
            let u = u_iso - (aspect - 1.0) * 0.5;
            let v = (py as f32 + sy) / dim_h;
            let s = match kind {
                UiGlyphKind::RustyLogo => sample_rusty_logo_uv(u, v, edge, tint),
                UiGlyphKind::Wifi {
                    connected,
                    signal_percent,
                } => sample_wifi_glyph_uv(u, v, edge, tint, connected, signal_percent),
                UiGlyphKind::Volume {
                    level_percent,
                    muted,
                } => sample_volume_glyph_uv(u, v, edge, tint, level_percent, muted),
                UiGlyphKind::Battery { percent, charging } => {
                    sample_battery_glyph_uv(u_iso, v, aspect, edge, tint, percent, charging)
                }
                UiGlyphKind::Search => sample_search_glyph_uv(u, v, edge, tint),
                UiGlyphKind::ControlCenter => sample_control_center_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Info => sample_info_glyph_uv(u, v, edge, tint),
                UiGlyphKind::TerminalPrompt => sample_terminal_prompt_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Folder => sample_folder_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Grid => sample_grid_glyph_uv(u, v, edge, tint),
                UiGlyphKind::LayoutSplit => sample_layout_split_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Motion => sample_motion_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Transparency => sample_transparency_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Refresh => sample_refresh_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Lock => sample_lock_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Shortcuts => sample_shortcuts_glyph_uv(u, v, edge, tint),
                UiGlyphKind::WindowMax => sample_window_max_glyph_uv(u, v, edge, tint),
                UiGlyphKind::WindowMin => sample_window_min_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Close => sample_close_glyph_uv(u, v, edge, tint),
                UiGlyphKind::ArrowNext => sample_arrow_glyph_uv(u, v, edge, tint, true),
                UiGlyphKind::ArrowPrev => sample_arrow_glyph_uv(u, v, edge, tint, false),
                UiGlyphKind::Bluetooth => sample_bluetooth_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Sun => sample_sun_glyph_uv(u, v, edge, tint),
                UiGlyphKind::MediaPrev => sample_media_seek_glyph_uv(u, v, edge, tint, false),
                UiGlyphKind::MediaPlay => sample_media_play_glyph_uv(u, v, edge, tint),
                UiGlyphKind::MediaPause => sample_media_pause_glyph_uv(u, v, edge, tint),
                UiGlyphKind::MediaNext => sample_media_seek_glyph_uv(u, v, edge, tint, true),
                UiGlyphKind::MusicNote => sample_music_note_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Moon => sample_moon_glyph_uv(u, v, edge, tint),
                UiGlyphKind::OverviewSpaces => sample_overview_spaces_glyph_uv(u, v, edge, tint),
                UiGlyphKind::Keyboard => sample_keyboard_glyph_uv(u, v, edge, tint),
                UiGlyphKind::PowerSymbol => sample_power_symbol_glyph_uv(u, v, edge, tint),
            };
            sum[0] += s[0];
            sum[1] += s[1];
            sum[2] += s[2];
            sum[3] += s[3];
        }
    }
    let avg_a = (sum[3] * 0.0625).clamp(0.0, 1.0);
    if avg_a <= 0.001 {
        return [0, 0, 0, 0];
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    [
        (sum[2] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (sum[1] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (sum[0] * 0.0625).clamp(0.0, 255.0).round() as u8,
        (avg_a * 255.0).clamp(0.0, 255.0).round() as u8,
    ]
}

pub(super) fn rasterize_ui_glyph_buffer_rect(
    kind: UiGlyphKind,
    width_px: i32,
    height_px: i32,
    tint_rgba: [u8; 4],
) -> MemoryRenderBuffer {
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (width_px, height_px),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let w_u = usize::try_from(width_px).unwrap_or(0);
            for py in 0..height_px {
                for px in 0..width_px {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * w_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample_ui_glyph_pixel_rect(
                            px, py, width_px, height_px, kind, tint_rgba,
                        );
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((width_px, height_px)),
            )])
        });
    }
    mem_buf
}
