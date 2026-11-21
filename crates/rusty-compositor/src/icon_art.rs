//! Procedural `RustyOS` 16-tap `R2` icon and folder rasterizers (`RUSTYOS-114`).
//!
//! Renders resolution-independent subpixel-antialiased Dock app icons, free-standing folders,
//! utility tiles (`Downloads`, `Trash`), and macOS squircle framing for default apps (Firefox),
//! reacting dynamically to Light/Dark appearance and `[ui].accent_index`.

use crate::icons::BuiltinAppIcon;

#[derive(Clone, Copy)]
struct AccentSpec {
    top: [f32; 3],
    bot: [f32; 3],
    deep: [f32; 3],
    sheet_tint: [f32; 3],
    glow: [f32; 3],
}

fn f_rgb(c: [u8; 4]) -> [f32; 3] {
    [f32::from(c[0]), f32::from(c[1]), f32::from(c[2])]
}

fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let k = t.clamp(0.0, 1.0);
    [
        a[0] * (1.0 - k) + b[0] * k,
        a[1] * (1.0 - k) + b[1] * k,
        a[2] * (1.0 - k) + b[2] * k,
    ]
}

fn blend_rgba(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let sa = src[3].clamp(0.0, 1.0);
    let inv = 1.0 - sa;
    [
        src[0] * sa + dst[0] * inv,
        src[1] * sa + dst[1] * inv,
        src[2] * sa + dst[2] * inv,
        (sa + dst[3] * inv).clamp(0.0, 1.0),
    ]
}

fn accent_spec(accent_idx: u8, is_dark: bool) -> AccentSpec {
    if accent_idx == 0 {
        return if is_dark {
            AccentSpec {
                top: [108.0, 202.0, 255.0],
                bot: [28.0, 128.0, 242.0],
                deep: [22.0, 96.0, 198.0],
                sheet_tint: [226.0, 244.0, 255.0],
                glow: [72.0, 178.0, 255.0],
            }
        } else {
            AccentSpec {
                top: [118.0, 208.0, 255.0],
                bot: [42.0, 146.0, 244.0],
                deep: [32.0, 118.0, 218.0],
                sheet_tint: [236.0, 248.0, 255.0],
                glow: [88.0, 190.0, 255.0],
            }
        };
    }
    let base = f_rgb(rusty_services::file_manager::accent_rgba_for_index(
        usize::from(accent_idx),
        is_dark,
    ));
    let white = [255.0, 255.0, 255.0];
    let black = [10.0, 12.0, 18.0];
    AccentSpec {
        top: lerp_rgb(base, white, if is_dark { 0.28 } else { 0.34 }),
        bot: lerp_rgb(base, black, if is_dark { 0.14 } else { 0.10 }),
        deep: lerp_rgb(base, black, 0.28),
        sheet_tint: lerp_rgb(base, white, 0.78),
        glow: lerp_rgb(base, white, 0.22),
    }
}

fn squircle_sd(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32) -> f32 {
    let dx = ((u - cx).abs() / hw.max(0.01)).powf(4.8);
    let dy = ((v - cy).abs() / hh.max(0.01)).powf(4.8);
    ((dx + dy).powf(1.0 / 4.8) - 1.0) * hw.min(hh)
}

fn sd_rounded_box(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (u - cx).abs() - (hw - r);
    let qy = (v - cy).abs() - (hh - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

fn sd_segment(u: f32, v: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> f32 {
    let pax = u - ax;
    let pay = v - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = bax * bax + bay * bay;
    let h = if denom > 1e-6 {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (pax - bax * h).hypot(pay - bay * h) - r
}

fn sd_ellipse(u: f32, v: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let nx = (u - cx) / rx.max(0.01);
    let ny = (v - cy) / ry.max(0.01);
    (nx.hypot(ny) - 1.0) * rx.min(ry)
}

fn cov(sd: f32, edge: f32) -> f32 {
    (0.5 - sd / edge.max(0.004)).clamp(0.0, 1.0)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Computes the exact macOS 27 smooth-shoulder folder back silhouette signed distance.
#[allow(clippy::too_many_arguments)]
fn folder_back_sd(u: f32, v: f32, x0: f32, x1: f32, y0: f32, y1: f32, drop: f32, r: f32) -> f32 {
    let top_y = y0 + drop * smoothstep(x0 + (x1 - x0) * 0.30, x0 + (x1 - x0) * 0.48, u);
    let dy_top = top_y - v;
    let dy_bot = v - y1;
    let dx_l = x0 - u;
    let dx_r = u - x1;
    let box_sd = sd_rounded_box(
        u,
        v,
        f32::midpoint(x0, x1),
        f32::midpoint(y0, y1),
        (x1 - x0) * 0.5,
        (y1 - y0) * 0.5,
        r,
    );
    // Use rounded corners while preserving smooth curved top-right shoulder dip.
    box_sd.max(dy_top).max(dy_bot).max(dx_l).max(dx_r)
}

fn sample_folder_uv(u: f32, v: f32, edge: f32, accent_idx: u8, is_dark: bool) -> [f32; 4] {
    let sp = accent_spec(accent_idx, is_dark);
    let mut col = [0.0_f32; 4];

    // Subtle soft ambient drop shadow underneath the folder base.
    let sh_sd = sd_rounded_box(u, v, 0.50, 0.565, 0.405, 0.285, 0.075);
    if sh_sd < 0.05 {
        let sh_a = (1.0 - ((sh_sd + 0.02).max(0.0) / 0.07))
            .clamp(0.0, 1.0)
            .powi(2);
        let sh_strength = if is_dark { 0.38 } else { 0.18 };
        col = blend_rgba(col, [4.0, 6.0, 12.0, sh_a * sh_strength]);
    }

    // Back tab plate with smooth curved S-shoulder.
    let back_sd = folder_back_sd(u, v, 0.08, 0.92, 0.16, 0.78, 0.068, 0.058);
    let back_cov = cov(back_sd, edge);
    if back_cov > 0.0 {
        let back_rgb = lerp_rgb(sp.deep, lerp_rgb(sp.deep, sp.top, 0.35), (v - 0.16) * 2.5);
        col = blend_rgba(col, [back_rgb[0], back_rgb[1], back_rgb[2], back_cov]);
        if back_sd > -0.018 && v < 0.30 {
            col = blend_rgba(col, [255.0, 255.0, 255.0, back_cov * 0.22]);
        }
    }

    // Crisp bright paper sheet peeking along the fold seam.
    let sheet_sd = sd_rounded_box(u, v, 0.50, 0.44, 0.385, 0.19, 0.032);
    let sheet_cov = cov(sheet_sd, edge);
    if sheet_cov > 0.0 {
        col = blend_rgba(
            col,
            [
                sp.sheet_tint[0],
                sp.sheet_tint[1],
                sp.sheet_tint[2],
                sheet_cov * 0.96,
            ],
        );
    }

    // Volumetric 3D front folder pocket with radial center glow and top specular rim.
    let front_sd = sd_rounded_box(u, v, 0.50, 0.555, 0.42, 0.275, 0.068);
    let front_cov = cov(front_sd, edge);
    if front_cov > 0.0 {
        let vt = ((v - 0.28) / 0.55).clamp(0.0, 1.0);
        let rad = ((u - 0.50) * 1.15).hypot((v - 0.42) * 1.45).clamp(0.0, 1.0);
        let mut front_rgb = lerp_rgb(sp.top, sp.bot, (vt * 0.82 + rad * 0.18).clamp(0.0, 1.0));
        // Subtle upper-lip inner specular highlight and soft bottom vignette.
        let top_lip = (1.0 - ((v - 0.28).max(0.0) / 0.032)).clamp(0.0, 1.0);
        front_rgb = lerp_rgb(front_rgb, [246.0, 252.0, 255.0], top_lip * 0.38);
        if front_sd > -0.014 {
            let edge_shade = if v < 0.50 { 0.14 } else { -0.14 };
            if edge_shade > 0.0 {
                front_rgb = lerp_rgb(front_rgb, [255.0, 255.0, 255.0], edge_shade);
            } else {
                front_rgb = lerp_rgb(front_rgb, sp.deep, -edge_shade * 1.8);
            }
        }
        col = blend_rgba(col, [front_rgb[0], front_rgb[1], front_rgb[2], front_cov]);
    }
    col
}

fn sample_downloads_uv(u: f32, v: f32, edge: f32, accent_idx: u8, is_dark: bool) -> [f32; 4] {
    let sp = accent_spec(accent_idx, is_dark);
    let mut col = [0.0_f32; 4];

    // Contact shadow under stacked Dock Downloads folder.
    let sh_sd = sd_rounded_box(u, v, 0.50, 0.615, 0.39, 0.245, 0.065);
    if sh_sd < 0.05 {
        let a = (1.0 - ((sh_sd + 0.015).max(0.0) / 0.065))
            .clamp(0.0, 1.0)
            .powi(2);
        col = blend_rgba(col, [4.0, 6.0, 12.0, a * if is_dark { 0.38 } else { 0.20 }]);
    }

    // Back stacked translucent folder tab.
    let back_sd = folder_back_sd(u, v, 0.13, 0.87, 0.15, 0.52, 0.062, 0.052);
    let back_cov = cov(back_sd, edge);
    if back_cov > 0.0 {
        let back_rgb = lerp_rgb(sp.deep, sp.bot, (v - 0.15) * 2.5);
        col = blend_rgba(
            col,
            [back_rgb[0], back_rgb[1], back_rgb[2], back_cov * 0.92],
        );
    }

    // Mid frosted sheet card.
    let mid_sd = sd_rounded_box(u, v, 0.50, 0.42, 0.385, 0.17, 0.048);
    let mid_cov = cov(mid_sd, edge);
    if mid_cov > 0.0 {
        let mid_rgb = lerp_rgb(sp.sheet_tint, sp.top, 0.55);
        col = blend_rgba(col, [mid_rgb[0], mid_rgb[1], mid_rgb[2], mid_cov * 0.94]);
    }

    // Front glossy Downloads pocket with embossed ring & downward arrow.
    let front_sd = sd_rounded_box(u, v, 0.50, 0.585, 0.405, 0.255, 0.064);
    let front_cov = cov(front_sd, edge);
    if front_cov > 0.0 {
        let vt = ((v - 0.33) / 0.51).clamp(0.0, 1.0);
        let mut fc = lerp_rgb(sp.top, sp.bot, vt);
        let lip = (1.0 - ((v - 0.33).max(0.0) / 0.032)).clamp(0.0, 1.0);
        fc = lerp_rgb(fc, [245.0, 251.0, 255.0], lip * 0.40);

        let cy = 0.59;
        let ring_sd = ((u - 0.50).hypot(v - cy) - 0.135).abs() - 0.016;
        let stem_sd = sd_segment(u, v, 0.50, cy - 0.075, 0.50, cy + 0.065, 0.019);
        let wing_l = sd_segment(u, v, 0.435, cy + 0.008, 0.50, cy + 0.072, 0.018);
        let wing_r = sd_segment(u, v, 0.565, cy + 0.008, 0.50, cy + 0.072, 0.018);
        let glyph_sd = ring_sd.min(stem_sd).min(wing_l).min(wing_r);
        let g_cov = cov(glyph_sd, edge);
        if g_cov > 0.0 {
            fc = lerp_rgb(fc, sp.sheet_tint, g_cov * 0.88);
        }
        col = blend_rgba(col, [fc[0], fc[1], fc[2], front_cov]);
    }
    col
}

fn sample_trash_uv(u: f32, v: f32, edge: f32, is_dark: bool, is_full: bool) -> [f32; 4] {
    let mut col = [0.0_f32; 4];
    let body_top = 0.225_f32;
    let body_bot = 0.845_f32;

    // Ground contact shadow ellipse.
    let ground_sd = sd_ellipse(u, v, 0.50, 0.865, 0.265, 0.055);
    if ground_sd < 0.035 {
        let ga = (1.0 - ((ground_sd + 0.015).max(0.0) / 0.05)).clamp(0.0, 1.0);
        col = blend_rgba(
            col,
            [6.0, 8.0, 14.0, ga * if is_dark { 0.42 } else { 0.22 }],
        );
    }

    let t_y = ((v - body_top) / (body_bot - body_top)).clamp(0.0, 1.0);
    let hw = 0.305 - 0.072 * t_y;
    let dx = (u - 0.50).abs() - hw;
    let dy_top = body_top - v;
    let dy_bot = v - body_bot;
    let base_cap = sd_ellipse(u, v, 0.50, body_bot, 0.233, 0.044);
    let top_rim = sd_ellipse(u, v, 0.50, body_top, 0.305, 0.054);
    let body_sd = dx.max(dy_top).max(dy_bot).min(base_cap).min(top_rim);
    let body_cov = cov(body_sd, edge);

    if body_cov > 0.0 {
        let nx = ((u - 0.50) / hw.max(0.05)).clamp(-1.0, 1.0);
        let spec = (-(nx + 0.30).powi(2) / 0.13).exp() * 0.36;
        let rim_spec = (-(nx - 0.65).powi(2) / 0.09).exp() * 0.16;
        let edge_shade = (nx * nx) * 0.24;
        let flute = ((nx * 7.0 * std::f32::consts::PI).cos() * 0.042).clamp(-0.045, 0.045);
        let shade = (0.76 + spec + rim_spec - edge_shade - t_y * 0.11 + flute).clamp(0.36, 1.0);
        let base = if is_dark {
            [138.0, 146.0, 160.0]
        } else {
            [222.0, 227.0, 236.0]
        };
        col = blend_rgba(
            col,
            [
                base[0] * shade,
                base[1] * shade,
                base[2] * shade,
                body_cov * 0.96,
            ],
        );
    }

    // Recessed inner cavity at perspective top opening.
    let hollow = sd_ellipse(u, v, 0.50, body_top, 0.272, 0.036);
    let hollow_cov = cov(hollow, edge);
    if hollow_cov > 0.0 {
        let h_rgb = if is_dark {
            lerp_rgb([28.0, 32.0, 40.0], [64.0, 70.0, 82.0], (u - 0.22) * 1.4)
        } else {
            lerp_rgb([74.0, 80.0, 92.0], [134.0, 140.0, 152.0], (u - 0.22) * 1.4)
        };
        col = blend_rgba(col, [h_rgb[0], h_rgb[1], h_rgb[2], hollow_cov * 0.92]);
    }

    if is_full {
        // Layered folded paper sheets peeking cleanly from inside behind the front lip.
        let clip_back = sd_rounded_box(u, v, 0.50, 0.145, 0.26, 0.11, 0.02);
        if clip_back < edge * 2.0 {
            let p1 = sd_rounded_box(
                u * 0.96 + v * 0.26,
                v * 0.96 - u * 0.26,
                0.44,
                0.08,
                0.075,
                0.06,
                0.015,
            );
            let p2 = sd_rounded_box(
                u * 0.96 - v * 0.24,
                v * 0.96 + u * 0.24,
                0.54,
                0.28,
                0.08,
                0.065,
                0.015,
            );
            let p3 = sd_ellipse(u, v, 0.49, 0.175, 0.068, 0.048);
            let paper_sd = p1.min(p2).min(p3).max(clip_back);
            let paper_cov = cov(paper_sd, edge);
            if paper_cov > 0.0 {
                let p_rgb = lerp_rgb(
                    [250.0, 251.0, 255.0],
                    [202.0, 210.0, 224.0],
                    (v - 0.11) * 7.0,
                );
                col = blend_rgba(col, [p_rgb[0], p_rgb[1], p_rgb[2], paper_cov * 0.97]);
            }
        }
    }

    // Metallic upper ellipse rim highlight.
    let rim_ring = (top_rim.abs() - 0.009).max(0.0) - 0.002;
    let rim_cov = cov(rim_ring, edge);
    if rim_cov > 0.0 {
        let rim_rgb = if is_dark {
            [196.0, 204.0, 218.0]
        } else {
            [248.0, 250.0, 255.0]
        };
        col = blend_rgba(col, [rim_rgb[0], rim_rgb[1], rim_rgb[2], rim_cov * 0.90]);
    }
    col
}

/// Classic macOS Finder face with curved central S-ridge, shadow, eyes, and smile.
fn paint_finder_face(u: f32, v: f32, edge: f32, sp: AccentSpec, is_dark: bool) -> [f32; 4] {
    // Signature Finder central S-profile: slight top slant, nose step around v=0.52..0.60, lower jaw.
    let nose_step = 0.072 * smoothstep(0.51, 0.56, v) * (1.0 - smoothstep(0.60, 0.65, v));
    let forehead_lean = -0.055 * smoothstep(0.18, 0.54, v);
    let spine_x = 0.525 + forehead_lean + nose_step;
    let d_spine = u - spine_x;

    // Right face base gradient (frosted pearl in Light, brushed dark obsidian in Dark).
    let vt = (v * 1.08 - 0.04).clamp(0.0, 1.0);
    let right_rgb = if is_dark {
        lerp_rgb([56.0, 62.0, 74.0], [28.0, 32.0, 40.0], vt)
    } else {
        lerp_rgb([246.0, 249.0, 255.0], [216.0, 224.0, 238.0], vt)
    };
    let mut col = [right_rgb[0], right_rgb[1], right_rgb[2], 1.0];

    // Soft shadow cast rightward by the elevated left Finder half onto the right face.
    if d_spine > -0.004 && d_spine < 0.065 {
        let sh = (1.0 - ((d_spine.max(0.0)) / 0.065)).clamp(0.0, 1.0).powi(2);
        col = blend_rgba(
            col,
            [8.0, 16.0, 36.0, sh * if is_dark { 0.34 } else { 0.16 }],
        );
    }

    // Left elevated luminous blue/accent face.
    let left_cov = cov(d_spine, edge);
    if left_cov > 0.0 {
        let left_rgb = lerp_rgb(sp.top, sp.bot, vt);
        col = blend_rgba(col, [left_rgb[0], left_rgb[1], left_rgb[2], left_cov]);
    }

    // Vertical capsule eyes + balanced wide smile arc with rounded endcaps.
    let eye_l = sd_segment(u, v, 0.315, 0.345, 0.315, 0.435, 0.025);
    let eye_r = sd_segment(u, v, 0.685, 0.345, 0.685, 0.435, 0.025);
    let cx = 0.50;
    let cy = 0.335;
    let arc_r = 0.375;
    let dx = u - cx;
    let dy = v - cy;
    let dist_arc = dx.hypot(dy);
    let max_sin = 0.64_f32;
    let sin_a = (dx / dist_arc.max(0.01)).clamp(-max_sin, max_sin);
    let cos_a = (1.0 - sin_a * sin_a).sqrt();
    let nearest_x = cx + arc_r * sin_a;
    let nearest_y = cy + arc_r * cos_a;
    let smile_sd = (u - nearest_x).hypot(v - nearest_y) - 0.023;

    let feature_sd = eye_l.min(eye_r).min(smile_sd);
    let f_cov = cov(feature_sd, edge * 0.9);
    if f_cov > 0.0 {
        let f_rgb = if is_dark {
            if u < spine_x {
                [248.0, 251.0, 255.0]
            } else {
                sp.glow
            }
        } else {
            [24.0, 34.0, 56.0]
        };
        col = blend_rgba(col, [f_rgb[0], f_rgb[1], f_rgb[2], f_cov * 0.92]);
    }
    col
}

fn paint_overview_3x3(u: f32, v: f32, edge: f32, sp: AccentSpec, is_dark: bool) -> [f32; 4] {
    let shell_rgb = if is_dark {
        lerp_rgb([52.0, 54.0, 62.0], [28.0, 30.0, 36.0], v)
    } else {
        lerp_rgb([250.0, 251.0, 254.0], [224.0, 228.0, 238.0], v)
    };
    let mut col = [shell_rgb[0], shell_rgb[1], shell_rgb[2], 1.0];

    let tiles: [([f32; 3], [f32; 3]); 9] = [
        (sp.top, sp.bot),
        ([88.0, 224.0, 128.0], [36.0, 178.0, 74.0]),
        ([255.0, 118.0, 102.0], [228.0, 52.0, 44.0]),
        ([255.0, 214.0, 74.0], [238.0, 152.0, 18.0]),
        ([112.0, 184.0, 255.0], [28.0, 112.0, 240.0]),
        ([156.0, 128.0, 248.0], [96.0, 68.0, 214.0]),
        ([76.0, 218.0, 232.0], [18.0, 154.0, 184.0]),
        ([255.0, 154.0, 86.0], [224.0, 96.0, 32.0]),
        ([188.0, 196.0, 210.0], [126.0, 134.0, 148.0]),
    ];

    for (idx, (t_top, t_bot)) in tiles.iter().enumerate() {
        let ci = idx % 3;
        let ri = idx / 3;
        #[allow(clippy::cast_precision_loss)]
        let cx = 0.275 + ci as f32 * 0.225;
        #[allow(clippy::cast_precision_loss)]
        let cy = 0.275 + ri as f32 * 0.225;

        let sh_cov = cov(
            sd_rounded_box(u, v, cx, cy + 0.012, 0.082, 0.082, 0.032),
            edge,
        );
        if sh_cov > 0.0 {
            col = blend_rgba(col, [0.0, 0.0, 0.0, sh_cov * 0.14]);
        }
        let tile_sd = sd_rounded_box(u, v, cx, cy, 0.080, 0.080, 0.030);
        let t_cov = cov(tile_sd, edge);
        if t_cov > 0.0 {
            let local_t = ((v - (cy - 0.080)) / 0.160).clamp(0.0, 1.0);
            let mut tr = lerp_rgb(*t_top, *t_bot, local_t);
            if tile_sd > -0.014 && v < cy {
                tr = lerp_rgb(tr, [255.0, 255.0, 255.0], 0.25);
            }
            col = blend_rgba(col, [tr[0], tr[1], tr[2], t_cov]);
        }
    }
    col
}

fn paint_terminal_squircle(u: f32, v: f32, edge: f32, sp: AccentSpec, is_dark: bool) -> [f32; 4] {
    // Anodized brushed metallic bezel casing.
    let bezel_rgb = if is_dark {
        lerp_rgb([92.0, 96.0, 108.0], [46.0, 48.0, 56.0], v)
    } else {
        lerp_rgb([234.0, 237.0, 242.0], [174.0, 180.0, 192.0], v)
    };
    let mut col = [bezel_rgb[0], bezel_rgb[1], bezel_rgb[2], 1.0];

    // Inner recessed dark glass terminal display.
    let screen_sd = squircle_sd(u, v, 0.50, 0.50, 0.392, 0.392);
    let screen_cov = cov(screen_sd, edge);
    if screen_cov > 0.0 {
        let mut scr = lerp_rgb([30.0, 32.0, 40.0], [14.0, 15.0, 20.0], (v - 0.11) * 1.25);
        // Subtle diagonal upper-left glass display reflection sweep.
        let gloss = ((0.76 - (u * 0.55 + v * 0.85)) / 0.25).clamp(0.0, 1.0);
        scr = lerp_rgb(scr, [86.0, 94.0, 114.0], gloss * 0.14);
        if screen_sd > -0.015 && v < 0.32 {
            scr = lerp_rgb(scr, [6.0, 7.0, 10.0], 0.45);
        }
        col = blend_rgba(col, [scr[0], scr[1], scr[2], screen_cov]);
    }

    // Crisp `>_` prompt anchored inside upper-left of the screen with accent glow bar.
    let l1 = sd_segment(u, v, 0.235, 0.285, 0.380, 0.395, 0.030);
    let l2 = sd_segment(u, v, 0.380, 0.395, 0.235, 0.505, 0.030);
    let chev_cov = cov(l1.min(l2), edge);
    if chev_cov > 0.0 {
        col = blend_rgba(col, [246.0, 248.0, 255.0, chev_cov]);
    }

    let cursor_sd = sd_rounded_box(u, v, 0.525, 0.500, 0.095, 0.026, 0.012);
    let c_cov = cov(cursor_sd, edge);
    if c_cov > 0.0 {
        col = blend_rgba(col, [sp.glow[0], sp.glow[1], sp.glow[2], c_cov]);
    }
    col
}

#[allow(clippy::many_single_char_names)]
fn paint_browser_compass(
    u: f32,
    v: f32,
    edge: f32,
    sp: AccentSpec,
    is_dark: bool,
    accent_idx: u8,
) -> [f32; 4] {
    let frame_rgb = if is_dark {
        lerp_rgb([56.0, 58.0, 66.0], [28.0, 30.0, 36.0], v)
    } else {
        lerp_rgb([251.0, 252.0, 255.0], [224.0, 229.0, 240.0], v)
    };
    let mut col = [frame_rgb[0], frame_rgb[1], frame_rgb[2], 1.0];

    let dx = u - 0.50;
    let dy = v - 0.50;
    let r = dx.hypot(dy);
    let dial_sd = r - 0.365;
    let dial_cov = cov(dial_sd, edge);
    if dial_cov > 0.0 {
        let dial_top = if accent_idx == 0 {
            [64.0, 194.0, 255.0]
        } else {
            sp.top
        };
        let dial_bot = if accent_idx == 0 {
            [14.0, 92.0, 232.0]
        } else {
            sp.bot
        };
        let mut dial_rgb = lerp_rgb(dial_top, dial_bot, ((v - 0.14) / 0.72).clamp(0.0, 1.0));
        // Subtle silver compass ring and 60 radial precision ticks.
        if dial_sd > -0.052 && dial_sd < -0.015 {
            let angle = dy.atan2(dx);
            let major = ((angle * 6.0).cos().abs()).powf(16.0);
            let minor = ((angle * 30.0).cos().abs()).powf(20.0) * 0.45;
            let tick = major.max(minor);
            dial_rgb = lerp_rgb(dial_rgb, [242.0, 249.0, 255.0], tick * 0.75);
        }
        col = blend_rgba(col, [dial_rgb[0], dial_rgb[1], dial_rgb[2], dial_cov]);
    }

    // Diamond compass needle rotated -45 deg with soft cast shadow underneath.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let along = dx * s - dy * s;
    let perp = dx * s + dy * s;
    let sh_along = (dx - 0.012) * s - (dy - 0.018) * s;
    let sh_perp = ((dx - 0.012) * s + (dy - 0.018) * s).abs();
    let half_len = 0.285_f32;
    let half_w = 0.072_f32;
    let sh_t = (sh_along.abs() / half_len).clamp(0.0, 1.0);
    let sh_cov = cov(
        (sh_perp - half_w * (1.0 - sh_t)).max(sh_along.abs() - half_len),
        edge * 1.8,
    );
    if sh_cov > 0.0 {
        col = blend_rgba(col, [0.0, 10.0, 32.0, sh_cov * 0.28]);
    }

    let t = (along.abs() / half_len).clamp(0.0, 1.0);
    let needle_sd = (perp.abs() - half_w * (1.0 - t)).max(along.abs() - half_len);
    let n_cov = cov(needle_sd, edge * 0.9);
    if n_cov > 0.0 {
        let faceted = perp < 0.0;
        let tip_rgb = if along >= 0.0 {
            if faceted {
                [255.0, 96.0, 82.0]
            } else {
                [224.0, 48.0, 38.0]
            }
        } else if faceted {
            [252.0, 253.0, 255.0]
        } else {
            [222.0, 228.0, 240.0]
        };
        col = blend_rgba(col, [tip_rgb[0], tip_rgb[1], tip_rgb[2], n_cov]);
    }
    let pin_cov = cov(r - 0.025, edge);
    if pin_cov > 0.0 {
        col = blend_rgba(col, [218.0, 226.0, 240.0, pin_cov]);
    }
    col
}

fn paint_notes_pad(
    u: f32,
    v: f32,
    edge: f32,
    sp: AccentSpec,
    is_dark: bool,
    accent_idx: u8,
) -> [f32; 4] {
    let body_rgb = if is_dark {
        lerp_rgb([46.0, 48.0, 54.0], [30.0, 32.0, 38.0], v)
    } else {
        lerp_rgb([253.0, 251.0, 245.0], [238.0, 234.0, 224.0], v)
    };
    let mut col = [body_rgb[0], body_rgb[1], body_rgb[2], 1.0];

    let cap_top = if accent_idx == 0 {
        [255.0, 214.0, 42.0]
    } else {
        lerp_rgb([255.0, 212.0, 46.0], sp.top, 0.35)
    };
    let cap_bot = if accent_idx == 0 {
        [242.0, 174.0, 12.0]
    } else {
        lerp_rgb([240.0, 170.0, 14.0], sp.bot, 0.35)
    };
    if v < 0.305 {
        let c_rgb = lerp_rgb(cap_top, cap_bot, (v / 0.305).clamp(0.0, 1.0));
        col = [c_rgb[0], c_rgb[1], c_rgb[2], 1.0];
    }

    // Subtle tear-off perforation seam & drop shadow beneath top gold cap.
    if (0.305..0.345).contains(&v) {
        let drop_a = (1.0 - ((v - 0.305) / 0.040)).clamp(0.0, 1.0).powi(2);
        col = blend_rgba(
            col,
            [0.0, 0.0, 0.0, drop_a * if is_dark { 0.35 } else { 0.14 }],
        );
    }
    let stitch_sd = ((v - 0.278).abs() - 0.008).max((u - 0.50).abs() - 0.42);
    let dash = (u * 26.0 * std::f32::consts::PI).cos() > 0.15;
    if dash && stitch_sd < edge {
        col = blend_rgba(col, [182.0, 126.0, 10.0, cov(stitch_sd, edge) * 0.55]);
    }

    let line_rgb = if is_dark {
        [78.0, 82.0, 94.0]
    } else {
        [206.0, 200.0, 188.0]
    };
    for ry in [0.46_f32, 0.61, 0.76] {
        let l_sd = sd_segment(u, v, 0.12, ry, 0.88, ry, 0.009);
        let l_cov = cov(l_sd, edge);
        if l_cov > 0.0 {
            col = blend_rgba(col, [line_rgb[0], line_rgb[1], line_rgb[2], l_cov * 0.86]);
        }
    }
    col
}

#[allow(clippy::many_single_char_names)]
fn paint_settings_gears(
    u: f32,
    v: f32,
    edge: f32,
    sp: AccentSpec,
    is_dark: bool,
    accent_idx: u8,
) -> [f32; 4] {
    let casing = if is_dark {
        lerp_rgb([68.0, 72.0, 82.0], [36.0, 38.0, 46.0], v)
    } else {
        lerp_rgb([188.0, 193.0, 202.0], [126.0, 132.0, 144.0], v)
    };
    let mut col = [casing[0], casing[1], casing[2], 1.0];

    let dx = u - 0.50;
    let dy = v - 0.50;
    let r = dx.hypot(dy);
    let well_cov = cov(r - 0.375, edge);
    if well_cov > 0.0 {
        let well_rgb = lerp_rgb([42.0, 44.0, 52.0], [72.0, 76.0, 86.0], (v - 0.14) * 1.4);
        col = blend_rgba(
            col,
            [well_rgb[0], well_rgb[1], well_rgb[2], well_cov * 0.96],
        );
    }

    // Sharp trapezoidal 14-tooth machined outer cog ring + 3 tapered diameter spokes + hub.
    let angle = dy.atan2(dx);
    let tooth_wave = (angle * 14.0).cos();
    let tooth_trap = (tooth_wave * 2.4).clamp(-1.0, 1.0);
    let outer_r = 0.285 + 0.038 * tooth_trap;
    let ring_sd = (r - outer_r).max(0.165 - r);
    let mut spoke_sd = 1.0_f32;
    for k in 0..3_u8 {
        let th = f32::from(k) * std::f32::consts::FRAC_PI_3;
        let (st, ct) = th.sin_cos();
        let seg = sd_segment(
            u,
            v,
            0.50 - ct * 0.172,
            0.50 - st * 0.172,
            0.50 + ct * 0.172,
            0.50 + st * 0.172,
            0.017,
        );
        spoke_sd = spoke_sd.min(seg);
    }
    let inner_cog_r = 0.125 + 0.015 * ((angle * 10.0).cos() * 2.2).clamp(-1.0, 1.0);
    let inner_sd = (r - inner_cog_r).max(0.068 - r);
    let hub_sd = r - 0.056;
    let gear_sd = ring_sd.min(spoke_sd).min(inner_sd).min(hub_sd);

    let g_cov = cov(gear_sd, edge);
    if g_cov > 0.0 {
        let metal = lerp_rgb(
            [242.0, 245.0, 250.0],
            [154.0, 162.0, 174.0],
            (v - 0.18) * 1.5,
        );
        col = blend_rgba(col, [metal[0], metal[1], metal[2], g_cov]);
    }
    if accent_idx != 0 {
        let trim_cov = cov((r - 0.158).abs() - 0.008, edge);
        if trim_cov > 0.0 {
            col = blend_rgba(col, [sp.glow[0], sp.glow[1], sp.glow[2], trim_cov * 0.85]);
        }
    }
    let axle_cov = cov(r - 0.025, edge);
    if axle_cov > 0.0 {
        col = blend_rgba(col, [72.0, 78.0, 90.0, axle_cov]);
    }
    col
}

fn paint_updater_or_system(
    u: f32,
    v: f32,
    edge: f32,
    sp: AccentSpec,
    is_updater: bool,
) -> [f32; 4] {
    let base_rgb = lerp_rgb(sp.top, sp.bot, v);
    let mut col = [base_rgb[0], base_rgb[1], base_rgb[2], 1.0];

    if is_updater {
        let ring_sd = ((u - 0.50).hypot(v - 0.50) - 0.255).abs() - 0.030;
        let stem_sd = sd_segment(u, v, 0.50, 0.34, 0.50, 0.60, 0.028);
        let wl = sd_segment(u, v, 0.39, 0.50, 0.50, 0.61, 0.028);
        let wr = sd_segment(u, v, 0.61, 0.50, 0.50, 0.61, 0.028);
        let m_cov = cov(ring_sd.min(stem_sd).min(wl).min(wr), edge);
        if m_cov > 0.0 {
            col = blend_rgba(col, [252.0, 254.0, 255.0, m_cov * 0.96]);
        }
    } else {
        let outer = [
            (0.50, 0.21),
            (0.77, 0.41),
            (0.67, 0.74),
            (0.33, 0.74),
            (0.23, 0.41),
        ];
        let hole = [
            (0.50, 0.36),
            (0.63, 0.46),
            (0.58, 0.61),
            (0.42, 0.61),
            (0.37, 0.46),
        ];
        let out_cov = cov(crate::surfaces::signed_distance_polygon(u, v, &outer), edge);
        if out_cov > 0.0 {
            let prism = lerp_rgb([255.0, 255.0, 255.0], sp.sheet_tint, (v - 0.2) * 1.5);
            col = blend_rgba(col, [prism[0], prism[1], prism[2], out_cov * 0.96]);
        }
        let hole_cov = cov(crate::surfaces::signed_distance_polygon(u, v, &hole), edge);
        if hole_cov > 0.0 {
            col = blend_rgba(col, [sp.deep[0], sp.deep[1], sp.deep[2], hole_cov * 0.94]);
        }
        let core_cov = cov((u - 0.50).hypot(v - 0.50) - 0.055, edge);
        if core_cov > 0.0 {
            col = blend_rgba(col, [252.0, 254.0, 255.0, core_cov * 0.96]);
        }
    }
    col
}

fn sample_squircle_uv(
    u: f32,
    v: f32,
    edge: f32,
    icon: BuiltinAppIcon,
    is_dark: bool,
    accent_idx: u8,
) -> [f32; 4] {
    match icon {
        BuiltinAppIcon::Folder => return sample_folder_uv(u, v, edge, accent_idx, is_dark),
        BuiltinAppIcon::Downloads => return sample_downloads_uv(u, v, edge, accent_idx, is_dark),
        BuiltinAppIcon::Trash => return sample_trash_uv(u, v, edge, is_dark, false),
        BuiltinAppIcon::TrashFull => return sample_trash_uv(u, v, edge, is_dark, true),
        _ => {}
    }

    let sp = accent_spec(accent_idx, is_dark);
    let shell_sd = squircle_sd(u, v, 0.50, 0.50, 0.455, 0.455);
    let shell_cov = cov(shell_sd, edge);
    if shell_cov <= 0.001 {
        // Soft ground contact shadow directly under squircle icon base.
        let sh_sd = squircle_sd(u, v - 0.022, 0.50, 0.50, 0.460, 0.460);
        if sh_sd < 0.032 {
            let a = (1.0 - (sh_sd.max(0.0) / 0.032)).clamp(0.0, 1.0).powi(2);
            return [4.0, 6.0, 14.0, a * if is_dark { 0.30 } else { 0.14 }];
        }
        return [0.0; 4];
    }

    let vt = (v * 1.1 - 0.05).clamp(0.0, 1.0);
    let mut col = match icon {
        BuiltinAppIcon::Files => paint_finder_face(u, v, edge, sp, is_dark),
        BuiltinAppIcon::Overview => paint_overview_3x3(u, v, edge, sp, is_dark),
        BuiltinAppIcon::Terminal => paint_terminal_squircle(u, v, edge, sp, is_dark),
        BuiltinAppIcon::Browser => paint_browser_compass(u, v, edge, sp, is_dark, accent_idx),
        BuiltinAppIcon::Notes => paint_notes_pad(u, v, edge, sp, is_dark, accent_idx),
        BuiltinAppIcon::Settings => paint_settings_gears(u, v, edge, sp, is_dark, accent_idx),
        BuiltinAppIcon::Updater => paint_updater_or_system(u, v, edge, sp, true),
        BuiltinAppIcon::RustySystem => paint_updater_or_system(u, v, edge, sp, false),
        BuiltinAppIcon::WindowFallback { hue_seed, active } => {
            let pal = if hue_seed == 0 && active {
                (sp.top, sp.bot)
            } else {
                let buckets: [([f32; 3], [f32; 3]); 4] = [
                    (sp.top, sp.bot),
                    ([45.0, 212.0, 191.0], [15.0, 118.0, 110.0]),
                    ([244.0, 114.0, 182.0], [157.0, 23.0, 77.0]),
                    ([96.0, 165.0, 250.0], [30.0, 64.0, 175.0]),
                ];
                let b = buckets[usize::from(hue_seed) % buckets.len()];
                if active {
                    b
                } else {
                    (
                        [b.0[0] * 0.82, b.0[1] * 0.82, b.0[2] * 0.82],
                        [b.1[0] * 0.82, b.1[1] * 0.82, b.1[2] * 0.82],
                    )
                }
            };
            let bg = lerp_rgb(pal.0, pal.1, vt);
            let mut c = [bg[0], bg[1], bg[2], 1.0];
            let win_cov = cov(sd_rounded_box(u, v, 0.50, 0.51, 0.27, 0.22, 0.05), edge);
            if win_cov > 0.0 {
                c = blend_rgba(c, [246.0, 248.0, 255.0, win_cov * 0.90]);
            }
            c
        }
        BuiltinAppIcon::Folder
        | BuiltinAppIcon::Downloads
        | BuiltinAppIcon::Trash
        | BuiltinAppIcon::TrashFull => [0.0; 4],
    };

    // Unified macOS 27 directional inner glass rim (top specular lip + soft bottom shade).
    if shell_sd > -0.024 && v < 0.50 {
        let w = ((0.50 - v) / 0.46).clamp(0.0, 1.0);
        col = blend_rgba(col, [255.0, 255.0, 255.0, 0.30 * w]);
    } else if shell_sd > -0.022 && v > 0.72 {
        let w = ((v - 0.72) / 0.28).clamp(0.0, 1.0);
        col = blend_rgba(col, [6.0, 8.0, 14.0, 0.14 * w]);
    }
    [col[0], col[1], col[2], col[3] * shell_cov]
}

/// Evaluates one `2x HiDPI` pixel of a procedural macOS 27 icon with `16-tap R2` (`LinearAccum`).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub(crate) fn sample_builtin_squircle_pixel(
    px: i32,
    py: i32,
    size_px: i32,
    icon: BuiltinAppIcon,
    is_dark: bool,
    accent_idx: u8,
) -> [u8; 4] {
    let dim = size_px.max(1) as f32;
    let edge = 1.15 / dim;
    let mut acc = crate::aa::LinearAccum::default();
    for (sx, sy) in crate::aa::R2_TAPS_16 {
        let u = (px as f32 + sx) / dim;
        let v = (py as f32 + sy) / dim;
        let s = sample_squircle_uv(u, v, edge, icon, is_dark, accent_idx);
        acc.add_straight([s[0], s[1], s[2]], s[3]);
    }
    acc.finish_bgra()
}

/// Returns `(pad_px, inner_dim_px)` for framing a pre-scaled XDG logo cleanly inside `dev_px`.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn squircle_logo_box(dev_px: usize) -> (usize, usize) {
    let pad = ((dev_px as f32) * 0.15).round() as usize;
    let inner = dev_px.saturating_sub(pad * 2).max(1);
    (pad, inner)
}

/// Renders a continuous macOS 27 squircle tile framing a centered premultiplied BGRA app logo (e.g. Firefox).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn composite_xdg_in_squircle(
    dev_px: i32,
    logo_bgra: &[u8],
    logo_dim: usize,
    is_dark: bool,
    accent_idx: u8,
) -> Vec<u8> {
    let dim = usize::try_from(dev_px.max(1)).unwrap_or(1);
    let dim_f = dim as f32;
    let edge = 1.15 / dim_f;
    let mut out = vec![0_u8; dim * dim * 4];
    let (pad, inner_dim) = squircle_logo_box(dim);
    let inner_end = pad + inner_dim;
    let sh_off = (dim / 36).max(1);
    let sp = accent_spec(accent_idx, is_dark);

    for py in 0..dim {
        for px in 0..dim {
            let mut acc = crate::aa::LinearAccum::default();
            for (sx, sy) in crate::aa::R2_TAPS_16 {
                let u = (px as f32 + sx) / dim_f;
                let v = (py as f32 + sy) / dim_f;
                let shell_sd = squircle_sd(u, v, 0.50, 0.50, 0.455, 0.455);
                let shell_cov = cov(shell_sd, edge);
                if shell_cov <= 0.001 {
                    let sh_sd = squircle_sd(u, v - 0.022, 0.50, 0.50, 0.460, 0.460);
                    if sh_sd < 0.032 {
                        let a = (1.0 - (sh_sd.max(0.0) / 0.032)).clamp(0.0, 1.0).powi(2);
                        acc.add_straight([4.0, 6.0, 14.0], a * if is_dark { 0.30 } else { 0.14 });
                    }
                    continue;
                }
                let base_rgb = if is_dark {
                    let g = lerp_rgb([56.0, 58.0, 68.0], [28.0, 30.0, 38.0], v);
                    if accent_idx == 0 {
                        g
                    } else {
                        lerp_rgb(g, sp.deep, 0.22)
                    }
                } else {
                    let g = lerp_rgb([252.0, 253.0, 255.0], [228.0, 232.0, 242.0], v);
                    if accent_idx == 0 {
                        g
                    } else {
                        lerp_rgb(g, sp.sheet_tint, 0.22)
                    }
                };
                let mut col = [base_rgb[0], base_rgb[1], base_rgb[2], 1.0];
                if shell_sd > -0.024 && v < 0.50 {
                    let w = ((0.50 - v) / 0.46).clamp(0.0, 1.0);
                    col = blend_rgba(col, [255.0, 255.0, 255.0, 0.30 * w]);
                } else if shell_sd > -0.022 && v > 0.72 {
                    let w = ((v - 0.72) / 0.28).clamp(0.0, 1.0);
                    col = blend_rgba(col, [6.0, 8.0, 14.0, 0.14 * w]);
                }
                acc.add_straight([col[0], col[1], col[2]], col[3] * shell_cov);
            }
            let idx = (py * dim + px) * 4;
            let mut bgra = acc.finish_bgra();
            if px >= pad
                && px < inner_end
                && py >= pad + sh_off
                && py < inner_end + sh_off
                && logo_dim == inner_dim
            {
                let lx = px - pad;
                let ly = py - pad - sh_off;
                let sidx = (ly * logo_dim + lx) * 4;
                if sidx + 4 <= logo_bgra.len() {
                    let sh_a =
                        (u16::from(logo_bgra[sidx + 3]) * if is_dark { 58 } else { 30 }) / 255;
                    let inv = 255 - sh_a;
                    for ch in bgra.iter_mut().take(3) {
                        *ch = ((u16::from(*ch) * inv + 127) / 255) as u8;
                    }
                }
            }
            if px >= pad && px < inner_end && py >= pad && py < inner_end && logo_dim > 0 {
                let lx = if logo_dim == inner_dim {
                    px - pad
                } else {
                    ((px - pad) * logo_dim) / inner_dim.max(1)
                };
                let ly = if logo_dim == inner_dim {
                    py - pad
                } else {
                    ((py - pad) * logo_dim) / inner_dim.max(1)
                };
                let logo_off = (ly.min(logo_dim - 1) * logo_dim + lx.min(logo_dim - 1)) * 4;
                if logo_off + 4 <= logo_bgra.len() {
                    let sa = u16::from(logo_bgra[logo_off + 3]);
                    let inv = 255 - sa;
                    for c in 0..3 {
                        let blended = u16::from(logo_bgra[logo_off + c])
                            + (u16::from(bgra[c]) * inv + 127) / 255;
                        bgra[c] = blended.min(255) as u8;
                    }
                    let out_a = sa + (u16::from(bgra[3]) * inv + 127) / 255;
                    bgra[3] = out_a.min(255) as u8;
                }
            }
            out[idx..idx + 4].copy_from_slice(&bgra);
        }
    }
    out
}
