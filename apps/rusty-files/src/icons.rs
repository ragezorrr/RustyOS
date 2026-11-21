//! Crisp pixel-aligned 14-18 px UI vector glyphs and 4x4 SSAA RustyOS
//! folder artwork for `rusty-files` (`RUSTYOS-065`, `RUSTYOS-066`).

use rusty_services::file_manager::{FileEntry, FileEntryKind, FileTagColor, FileViewMode};

use crate::render::{Canvas, Palette, lerp_rgba, rgba};
use crate::state::SidebarIconKind;

/// Snaps a stroke center coordinate to half-pixel center so 1-px lines stay crisp.
#[inline]
pub(crate) fn snap_half(coord: f32) -> f32 {
    coord.floor() + 0.5
}

/// Snaps a box edge coordinate to an exact integer pixel boundary.
#[inline]
pub(crate) fn snap_edge(coord: f32) -> f32 {
    coord.round()
}

/// Renders a crisp pixel-aligned 14-16 px RustyOS sidebar icon centered at `(cx, cy)`.
pub(crate) fn draw_sidebar_icon(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    kind: SidebarIconKind,
    pal: &Palette,
) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    let accent = pal.accent;
    let neutral = pal.text_secondary;
    match kind {
        SidebarIconKind::Tag(tag) => {
            let [red, green, blue] = tag.rgb();
            c.fill_circle_f(sx, sy, 4.25, rgba(red, green, blue, 255));
        }
        SidebarIconKind::Recents => {
            c.stroke_circle_f(sx, sy, 5.2, 1.15, accent);
            c.stroke_polyline_aa(
                &[(sx, sy - 2.8), (sx, sy), (sx + 2.3, sy + 1.4)],
                false,
                1.15,
                accent,
            );
        }
        SidebarIconKind::Shared => draw_shared_people_icon(c, sx, sy, accent),
        SidebarIconKind::Applications => draw_applications_a_icon(c, sx, sy, accent),
        SidebarIconKind::Desktop => {
            c.stroke_rounded_rect_f(
                snap_edge(sx - 5.5),
                snap_edge(sy - 4.8),
                11.0,
                7.6,
                1.4,
                1.05,
                accent,
            );
            c.stroke_line_aa(
                sx - 3.6,
                snap_half(sy + 0.8),
                sx + 3.6,
                snap_half(sy + 0.8),
                0.95,
                accent,
            );
            c.stroke_line_aa(sx, sy + 2.8, sx, sy + 4.8, 1.05, accent);
            c.stroke_line_aa(
                sx - 2.8,
                snap_half(sy + 4.8),
                sx + 2.8,
                snap_half(sy + 4.8),
                1.05,
                accent,
            );
        }
        SidebarIconKind::Documents => draw_dogear_document_icon(c, sx, sy, accent, 1.1),
        SidebarIconKind::Downloads => {
            c.stroke_circle_f(sx, sy, 5.2, 1.1, accent);
            c.stroke_line_aa(sx, sy - 2.6, sx, sy + 2.2, 1.15, accent);
            c.stroke_polyline_aa(
                &[(sx - 2.2, sy + 0.2), (sx, sy + 2.5), (sx + 2.2, sy + 0.2)],
                false,
                1.15,
                accent,
            );
        }
        SidebarIconKind::Home => draw_home_house_icon(c, sx, sy, neutral),
        SidebarIconKind::Volume => draw_drive_volume_icon(c, sx, sy, pal),
        SidebarIconKind::Network => draw_network_globe_icon(c, sx, sy, neutral),
        SidebarIconKind::Trash => draw_trash_can_icon(c, sx, sy, neutral),
    }
}

fn draw_shared_people_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_circle_f(cx - 1.8, cy - 2.2, 1.8, 1.05, col);
    c.stroke_polyline_aa(
        &[
            (cx - 4.8, cy + 4.0),
            (cx - 3.4, cy + 1.4),
            (cx - 0.2, cy + 1.4),
            (cx + 1.2, cy + 4.0),
        ],
        false,
        1.05,
        col,
    );
    c.stroke_circle_f(cx + 2.6, cy - 2.4, 1.5, 1.0, col);
    c.stroke_polyline_aa(
        &[
            (cx + 1.7, cy + 1.4),
            (cx + 3.9, cy + 1.4),
            (cx + 5.0, cy + 3.8),
        ],
        false,
        1.0,
        col,
    );
}

fn draw_applications_a_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_line_aa(cx - 3.6, cy + 4.3, cx + 0.3, cy - 4.3, 1.15, col);
    c.stroke_line_aa(cx - 0.3, cy - 4.3, cx + 3.6, cy + 4.3, 1.15, col);
    c.stroke_line_aa(
        cx - 4.5,
        snap_half(cy + 1.4),
        cx + 4.5,
        snap_half(cy + 1.4),
        1.05,
        col,
    );
}

pub(crate) fn draw_dogear_document_icon(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    col: [u8; 4],
    weight: f32,
) {
    let left = snap_half(cx - 4.5);
    let right = snap_half(cx + 4.5);
    let top = snap_half(cy - 5.5);
    let bot = snap_half(cy + 5.5);
    c.stroke_polyline_aa(
        &[
            (left, top),
            (left + 5.8, top),
            (right, top + 3.2),
            (right, bot),
            (left, bot),
        ],
        true,
        weight,
        col,
    );
    c.stroke_polyline_aa(
        &[
            (left + 5.8, top),
            (left + 5.8, top + 3.2),
            (right, top + 3.2),
        ],
        false,
        weight * 0.9,
        col,
    );
    c.stroke_line_aa(
        left + 2.0,
        snap_half(cy + 0.5),
        right - 2.0,
        snap_half(cy + 0.5),
        1.0,
        col,
    );
    c.stroke_line_aa(
        left + 2.0,
        snap_half(cy + 2.8),
        right - 3.0,
        snap_half(cy + 2.8),
        1.0,
        col,
    );
}

fn draw_home_house_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_polyline_aa(
        &[(cx - 5.5, cy - 0.2), (cx, cy - 5.0), (cx + 5.5, cy - 0.2)],
        false,
        1.15,
        col,
    );
    c.stroke_line_aa(
        snap_half(cx + 2.8),
        cy - 4.1,
        snap_half(cx + 2.8),
        cy - 2.4,
        1.05,
        col,
    );
    let left = snap_half(cx - 3.9);
    let right = snap_half(cx + 3.9);
    let bot = snap_half(cy + 4.8);
    c.stroke_polyline_aa(
        &[
            (left, cy - 1.1),
            (left, bot),
            (right, bot),
            (right, cy - 1.1),
        ],
        false,
        1.1,
        col,
    );
    c.stroke_rounded_rect_f(
        snap_edge(cx - 1.4),
        snap_edge(cy + 0.8),
        2.8,
        4.0,
        0.7,
        0.95,
        col,
    );
}

fn draw_drive_volume_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, pal: &Palette) {
    let (border, slot) = if pal.is_dark {
        (rgba(174, 174, 182, 255), rgba(140, 140, 148, 220))
    } else {
        (pal.text_secondary, rgba(120, 122, 128, 220))
    };
    let left = snap_edge(cx - 5.8);
    let upper = snap_edge(cy - 3.2);
    c.stroke_rounded_rect_f(left, upper, 11.6, 6.4, 1.8, 1.05, border);
    c.stroke_line_aa(
        left + 1.5,
        snap_half(upper + 4.1),
        left + 10.1,
        snap_half(upper + 4.1),
        0.9,
        slot,
    );
    c.fill_circle_f(left + 9.0, upper + 5.1, 0.65, border);
}

fn draw_network_globe_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_circle_f(cx, cy, 5.2, 1.05, col);
    c.stroke_line_aa(cx - 4.8, cy, cx + 4.8, cy, 0.95, col);
    c.stroke_line_aa(cx, cy - 4.8, cx, cy + 4.8, 0.95, col);
    c.stroke_ellipse_aa(cx, cy, 2.5, 5.1, 0.95, col);
}

fn draw_trash_can_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    let lid_y = snap_half(cy - 3.3);
    c.stroke_line_aa(cx - 4.7, lid_y, cx + 4.7, lid_y, 1.1, col);
    c.stroke_polyline_aa(
        &[
            (cx - 1.9, lid_y),
            (cx - 1.3, lid_y - 1.7),
            (cx + 1.3, lid_y - 1.7),
            (cx + 1.9, lid_y),
        ],
        false,
        1.0,
        col,
    );
    c.stroke_polyline_aa(
        &[
            (cx - 3.7, lid_y),
            (cx - 2.9, snap_half(cy + 4.9)),
            (cx + 2.9, snap_half(cy + 4.9)),
            (cx + 3.7, lid_y),
        ],
        false,
        1.05,
        col,
    );
    c.stroke_line_aa(cx - 1.2, cy - 1.1, cx - 0.9, cy + 3.1, 0.95, col);
    c.stroke_line_aa(cx + 1.2, cy - 1.1, cx + 0.9, cy + 3.1, 0.95, col);
}

/// Draws compact 14 px file/folder entry icons in Columns/List/Gallery header.
struct CachedIconSprite {
    w: i32,
    h: i32,
    bgra_premul: Vec<u8>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
enum IconSpriteKey {
    SmallFolder {
        selected_blue: bool,
        is_dark: bool,
        accent: [u8; 4],
    },
    SmallApp {
        selected_blue: bool,
        accent: [u8; 4],
    },
    SmallDoc {
        fg: [u8; 4],
    },
    LargeFolder {
        category: u8,
        tag: Option<FileTagColor>,
        is_dark: bool,
        accent: [u8; 4],
        size_tenths: u16,
        qx: u8,
        qy: u8,
    },
    LargeApp {
        accent: [u8; 4],
        border: [u8; 4],
        size_tenths: u16,
        qx: u8,
        qy: u8,
    },
    LargeDoc {
        short_ext: String,
        is_dark: bool,
        accent: [u8; 4],
        border: [u8; 4],
        size_tenths: u16,
        qx: u8,
        qy: u8,
    },
}

std::thread_local! {
    static ICON_SPRITE_CACHE: std::cell::RefCell<std::collections::HashMap<IconSpriteKey, CachedIconSprite>> =
        std::cell::RefCell::new(std::collections::HashMap::with_capacity(64));
}

fn folder_category_key(name: &str) -> (u8, &'static str) {
    match name.to_ascii_lowercase().as_str() {
        "desktop" | "рабочий стол" => (1, "desktop"),
        "documents" | "документы" => (2, "documents"),
        "downloads" | "загрузки" => (3, "downloads"),
        "pictures" | "изображения" => (4, "pictures"),
        "music" | "музыка" => (5, "music"),
        "movies" | "videos" | "видео" => (6, "movies"),
        "public" | "shared" | "общедоступные" => (7, "public"),
        "applications" | "apps" => (8, "applications"),
        _ => (0, ""),
    }
}

fn blit_or_render_sprite(
    c: &mut Canvas<'_>,
    key: IconSpriteKey,
    dst_x: i32,
    dst_y: i32,
    w: i32,
    h: i32,
    draw_fn: impl FnOnce(&mut Canvas<'_>),
) {
    ICON_SPRITE_CACHE.with(|cell| {
        let mut map = cell.borrow_mut();
        if map.len() > 256 {
            map.clear();
        }
        let sprite = map.entry(key).or_insert_with(|| {
            let wu = u32::try_from(w.max(1)).unwrap_or(1);
            let hu = u32::try_from(h.max(1)).unwrap_or(1);
            let mut buf = vec![0_u8; (wu * hu * 4) as usize];
            {
                let mut offscreen = Canvas::new(&mut buf, wu, hu);
                draw_fn(&mut offscreen);
            }
            CachedIconSprite {
                w,
                h,
                bgra_premul: buf,
            }
        });
        c.blit_premul_sprite(dst_x, dst_y, sprite.w, sprite.h, &sprite.bgra_premul);
    });
}

/// Draws compact 14 px file/folder entry icons in Columns/List/Gallery header.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn draw_entry_icon(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    entry: &FileEntry,
    selected_blue: bool,
    pal: &Palette,
) {
    const SPRITE_SZ: i32 = 24;
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    let key = match entry.kind {
        FileEntryKind::Directory | FileEntryKind::MountPoint => IconSpriteKey::SmallFolder {
            selected_blue,
            is_dark: pal.is_dark,
            accent: pal.accent,
        },
        FileEntryKind::Application => IconSpriteKey::SmallApp {
            selected_blue,
            accent: pal.accent,
        },
        _ => IconSpriteKey::SmallDoc {
            fg: if selected_blue {
                rgba(255, 255, 255, 235)
            } else {
                pal.text_secondary
            },
        },
    };
    let dst_x = (sx - 12.5).round() as i32;
    let dst_y = (sy - 12.5).round() as i32;
    blit_or_render_sprite(c, key, dst_x, dst_y, SPRITE_SZ, SPRITE_SZ, |offscreen| {
        draw_entry_icon_uncached(offscreen, 12.5, 12.5, entry, selected_blue, pal);
    });
}

fn draw_entry_icon_uncached(
    c: &mut Canvas<'_>,
    sx: f32,
    sy: f32,
    entry: &FileEntry,
    selected_blue: bool,
    pal: &Palette,
) {
    match entry.kind {
        FileEntryKind::Directory | FileEntryKind::MountPoint => {
            let (back_col, top_col, bot_col, hilite) = if selected_blue {
                (
                    rgba(210, 232, 255, 255),
                    rgba(255, 255, 255, 250),
                    rgba(232, 243, 255, 245),
                    rgba(255, 255, 255, 255),
                )
            } else {
                let (b, t, bo, lip, _, _) = folder_palette_for(None, pal);
                (b, t, bo, lip)
            };
            c.fill_poly_aa(
                &[
                    (sx - 7.0, sy - 5.5),
                    (sx - 2.0, sy - 5.5),
                    (sx - 0.3, sy - 3.8),
                    (sx + 6.8, sy - 3.8),
                    (sx + 6.8, sy - 1.5),
                    (sx - 7.0, sy - 1.5),
                ],
                back_col,
            );
            c.fill_rounded_rect_vgrad_f(
                snap_edge(sx - 7.0),
                snap_edge(sy - 3.0),
                14.0,
                9.0,
                1.8,
                top_col,
                bot_col,
            );
            c.stroke_line_aa(
                sx - 5.4,
                snap_half(sy - 2.6),
                sx + 5.4,
                snap_half(sy - 2.6),
                0.85,
                hilite,
            );
        }
        FileEntryKind::Application => {
            let sq_col = if selected_blue {
                rgba(255, 255, 255, 240)
            } else {
                pal.accent
            };
            c.fill_rounded_rect_f(
                snap_edge(sx - 6.0),
                snap_edge(sy - 6.0),
                12.0,
                12.0,
                3.0,
                sq_col,
            );
            let fg = if selected_blue {
                pal.accent
            } else {
                rgba(255, 255, 255, 245)
            };
            c.stroke_line_aa(sx - 2.4, sy + 2.7, sx + 0.3, sy - 2.7, 1.15, fg);
            c.stroke_line_aa(sx - 0.3, sy - 2.7, sx + 2.4, sy + 2.7, 1.15, fg);
            c.stroke_line_aa(
                sx - 3.0,
                snap_half(sy + 1.0),
                sx + 3.0,
                snap_half(sy + 1.0),
                1.1,
                fg,
            );
        }
        _ => {
            let fg = if selected_blue {
                rgba(255, 255, 255, 235)
            } else {
                pal.text_secondary
            };
            draw_dogear_document_icon(c, sx, sy, fg, 1.15);
        }
    }
}

/// Draws large high-detail 4x4 SSAA RustyOS folder or file badge centered at `(cx, cy)`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn draw_large_entry_artwork(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    size: f32,
    entry: &FileEntry,
    primary_tag: Option<FileTagColor>,
    pal: &Palette,
) {
    let size_tenths = (size * 10.0).round().clamp(10.0, 4000.0) as u16;
    let pad = 10_i32;
    let sprite_dim = (size.ceil() as i32 + pad * 2).max(24);
    let half = sprite_dim / 2;
    let qx = (((cx - cx.floor()) * 4.0).round() as u8) & 3;
    let qy = (((cy - cy.floor()) * 4.0).round() as u8) & 3;
    let dst_x = cx.floor() as i32 - half;
    let dst_y = cy.floor() as i32 - half;
    let half_f = f32::from(i16::try_from(half).unwrap_or(64));
    let ax = half_f + f32::from(qx) * 0.25;
    let ay = half_f + f32::from(qy) * 0.25;

    match entry.kind {
        FileEntryKind::Directory | FileEntryKind::MountPoint => {
            let (category, canonical_name) = folder_category_key(&entry.name);
            let key = IconSpriteKey::LargeFolder {
                category,
                tag: primary_tag,
                is_dark: pal.is_dark,
                accent: pal.accent,
                size_tenths,
                qx,
                qy,
            };
            blit_or_render_sprite(c, key, dst_x, dst_y, sprite_dim, sprite_dim, |offscreen| {
                draw_large_rustyos_folder(
                    offscreen,
                    ax,
                    ay,
                    size,
                    canonical_name,
                    primary_tag,
                    pal,
                );
            });
        }
        FileEntryKind::Application => {
            let key = IconSpriteKey::LargeApp {
                accent: pal.accent,
                border: pal.pill_border,
                size_tenths,
                qx,
                qy,
            };
            blit_or_render_sprite(c, key, dst_x, dst_y, sprite_dim, sprite_dim, |offscreen| {
                draw_large_app_artwork(offscreen, ax, ay, size, pal);
            });
        }
        _ => {
            let ext = entry
                .name
                .rsplit_once('.')
                .map_or("FILE", |(_, e)| e)
                .to_ascii_uppercase();
            let short_ext: String = ext.chars().take(4).collect();
            let key = IconSpriteKey::LargeDoc {
                short_ext,
                is_dark: pal.is_dark,
                accent: pal.accent,
                border: pal.pill_border,
                size_tenths,
                qx,
                qy,
            };
            blit_or_render_sprite(c, key, dst_x, dst_y, sprite_dim, sprite_dim, |offscreen| {
                draw_large_document_artwork(offscreen, ax, ay, size, entry, pal);
            });
        }
    }
}

const FOLDER_AA_16: [(f32, f32); 16] = [
    (0.125, 0.0625),
    (0.375, 0.1875),
    (0.625, 0.0625),
    (0.875, 0.1875),
    (0.0625, 0.375),
    (0.3125, 0.3125),
    (0.5625, 0.4375),
    (0.8125, 0.3125),
    (0.1875, 0.625),
    (0.4375, 0.5625),
    (0.6875, 0.6875),
    (0.9375, 0.5625),
    (0.0625, 0.875),
    (0.3125, 0.9375),
    (0.5625, 0.8125),
    (0.8125, 0.9375),
];

#[inline]
fn f_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
#[allow(clippy::many_single_char_names)]
fn f_sd_round_box(px: f32, py: f32, bx: f32, by: f32, bw: f32, bh: f32, radius: f32) -> f32 {
    let hw = bw * 0.5;
    let hh = bh * 0.5;
    let rad = radius.min(hw).min(hh).max(0.0);
    let qx = (px - (bx + hw)).abs() - (hw - rad);
    let qy = (py - (by + hh)).abs() - (hh - rad);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad
}

#[inline]
fn f_mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let k = t.clamp(0.0, 1.0);
    [
        a[0] * (1.0 - k) + b[0] * k,
        a[1] * (1.0 - k) + b[1] * k,
        a[2] * (1.0 - k) + b[2] * k,
    ]
}

#[inline]
fn f_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let sa = src[3].clamp(0.0, 1.0);
    let inv = 1.0 - sa;
    [
        src[0] * sa + dst[0] * inv,
        src[1] * sa + dst[1] * inv,
        src[2] * sa + dst[2] * inv,
        (sa + dst[3] * inv).clamp(0.0, 1.0),
    ]
}

#[inline]
fn col_f3(c: [u8; 4]) -> [f32; 3] {
    [f32::from(c[0]), f32::from(c[1]), f32::from(c[2])]
}

#[allow(clippy::too_many_arguments)]
fn sample_folder_subpixel(
    px: f32,
    py: f32,
    left: f32,
    top: f32,
    w: f32,
    h: f32,
    tab_h: f32,
    rad: f32,
    back_f: [f32; 3],
    top_f: [f32; 3],
    bot_f: [f32; 3],
    lip_f: [f32; 3],
    is_dark: bool,
) -> [f32; 4] {
    let front_top = top + tab_h + w * 0.036;
    let front_h = h - (front_top - top);
    let mut col = [0.0_f32; 4];

    // 1. Soft diffused drop shadow underneath front pocket.
    let sh_sd = f_sd_round_box(px, py, left, front_top + 1.8, w, front_h, rad + 1.0);
    if sh_sd < 4.5 {
        let a = (1.0 - ((sh_sd + 1.2).max(0.0) / 5.7))
            .clamp(0.0, 1.0)
            .powi(2);
        col = f_over(col, [4.0, 6.0, 14.0, a * if is_dark { 0.42 } else { 0.20 }]);
    }

    // 2. Back plate with smooth S-shoulder curve (replaces flat polygon).
    let shoulder = f_smoothstep(left + w * 0.31, left + w * 0.48, px);
    let back_edge_top = top + (tab_h + w * 0.008) * shoulder;
    let back_box = f_sd_round_box(px, py, left + 0.5, top, w - 1.0, h * 0.55, rad * 0.85);
    let back_sd = back_box.max(back_edge_top - py);
    let back_cov = (0.5 - back_sd).clamp(0.0, 1.0);
    if back_cov > 0.0 {
        let bt = ((py - top) / (tab_h + 6.0).max(1.0)).clamp(0.0, 1.0);
        let b_rgb = f_mix(back_f, f_mix(back_f, top_f, 0.35), bt);
        col = f_over(col, [b_rgb[0], b_rgb[1], b_rgb[2], back_cov]);
        if back_sd > -1.2 && py < top + tab_h + 3.0 {
            col = f_over(col, [255.0, 255.0, 255.0, back_cov * 0.20]);
        }
    }

    // 3. Clean bright paper sheet peeking softly along fold seam.
    let sheet_sd = f_sd_round_box(
        px,
        py,
        left + w * 0.04,
        top + tab_h + w * 0.012,
        w * 0.92,
        h * 0.30,
        rad * 0.45,
    );
    let sheet_cov = (0.5 - sheet_sd).clamp(0.0, 1.0);
    if sheet_cov > 0.0 {
        col = f_over(col, [lip_f[0], lip_f[1], lip_f[2], sheet_cov * 0.96]);
    }

    // 4. Glossy volumetric front pocket with upper highlight, inner bevel & soft bottom vignette.
    let front_sd = f_sd_round_box(px, py, left, front_top, w, front_h, rad);
    let front_cov = (0.5 - front_sd).clamp(0.0, 1.0);
    if front_cov > 0.0 {
        let vt = ((py - front_top) / front_h.max(1.0)).clamp(0.0, 1.0);
        let cx = left + w * 0.5;
        let cy_g = front_top + front_h * 0.28;
        let radial =
            (((px - cx) / (w * 0.55)).hypot((py - cy_g) / (front_h * 0.72))).clamp(0.0, 1.0);
        let mut fc = f_mix(top_f, bot_f, (vt * 0.84 + radial * 0.16).clamp(0.0, 1.0));
        let top_lip = (1.0 - ((py - front_top).max(0.0) / 2.2)).clamp(0.0, 1.0);
        fc = f_mix(fc, [250.0, 253.0, 255.0], top_lip * 0.40);
        if front_sd > -1.2 {
            if py < front_top + front_h * 0.5 {
                fc = f_mix(fc, [255.0, 255.0, 255.0], 0.14);
            } else {
                fc = f_mix(fc, back_f, 0.22);
            }
        }
        col = f_over(col, [fc[0], fc[1], fc[2], front_cov]);
    }
    col
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn draw_large_rustyos_folder(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    size: f32,
    folder_name: &str,
    tag_tint: Option<FileTagColor>,
    pal: &Palette,
) {
    let w = size;
    let h = size * 0.80;
    let left = (cx - w * 0.5).round();
    let top = (cy - h * 0.5).round();
    let tab_h = h * 0.115;
    let rad = (size * 0.085).clamp(3.0, 8.0);

    let (back_col, top_col, bot_col, lip_col, _, engrave_col) = folder_palette_for(tag_tint, pal);
    let back_f = col_f3(back_col);
    let top_f = col_f3(top_col);
    let bot_f = col_f3(bot_col);
    let lip_f = col_f3(lip_col);

    let sw = usize::try_from(c.w.max(1)).unwrap_or(1);
    let sh = usize::try_from(c.h.max(1)).unwrap_or(1);
    let mut folder_buf = vec![0_u8; sw * sh * 4];
    let min_x = ((left - 6.0).floor() as i32).max(0);
    let min_y = ((top - 4.0).floor() as i32).max(0);
    let max_x = ((left + w + 6.0).ceil() as i32).min(c.w);
    let max_y = ((top + h + 8.0).ceil() as i32).min(c.h);

    for py in min_y..max_y {
        for px in min_x..max_x {
            let mut acc = [0.0_f32; 4];
            for (sx, sy) in FOLDER_AA_16 {
                let s = sample_folder_subpixel(
                    px as f32 + sx,
                    py as f32 + sy,
                    left,
                    top,
                    w,
                    h,
                    tab_h,
                    rad,
                    back_f,
                    top_f,
                    bot_f,
                    lip_f,
                    pal.is_dark,
                );
                let sa = s[3];
                acc[0] += s[2] * sa;
                acc[1] += s[1] * sa;
                acc[2] += s[0] * sa;
                acc[3] += sa;
            }
            let idx = (py as usize * sw + px as usize) * 4;
            if idx + 4 <= folder_buf.len() {
                let inv = 1.0 / 16.0;
                folder_buf[idx] = (acc[0] * inv).round().clamp(0.0, 255.0) as u8;
                folder_buf[idx + 1] = (acc[1] * inv).round().clamp(0.0, 255.0) as u8;
                folder_buf[idx + 2] = (acc[2] * inv).round().clamp(0.0, 255.0) as u8;
                folder_buf[idx + 3] = (acc[3] * inv * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    c.blit_premul_sprite(
        0,
        0,
        i32::try_from(sw).unwrap_or(1),
        i32::try_from(sh).unwrap_or(1),
        &folder_buf,
    );

    // Dual-pass debossed category glyph stamped into the front pocket.
    let front_top = top + tab_h + w * 0.036;
    let front_h = h - (front_top - top);
    let glyph_cy = front_top + front_h * 0.51;
    let glyph_r = size * 0.35;
    let hilite_engrave = rgba(255, 255, 255, 58);
    draw_folder_category_engraving(c, cx, glyph_cy + 0.65, glyph_r, folder_name, hilite_engrave);
    draw_folder_category_engraving(c, cx, glyph_cy, glyph_r, folder_name, engrave_col);
}

#[allow(clippy::type_complexity)]
fn folder_palette_for(
    tag_tint: Option<FileTagColor>,
    pal: &Palette,
) -> ([u8; 4], [u8; 4], [u8; 4], [u8; 4], [u8; 4], [u8; 4]) {
    if let Some(tag) = tag_tint {
        let [tr, tg, tb] = tag.rgb();
        let base = rgba(tr, tg, tb, 255);
        let top = lerp_rgba(base, rgba(255, 255, 255, 255), 0.28);
        let bot = lerp_rgba(base, rgba(0, 0, 0, 255), 0.14);
        let back = lerp_rgba(base, rgba(0, 0, 0, 255), 0.22);
        let lip = lerp_rgba(base, rgba(255, 255, 255, 255), 0.72);
        let rim = rgba(back[0], back[1], back[2], 140);
        let engrave = rgba(
            back[0].saturating_sub(24),
            back[1].saturating_sub(24),
            back[2].saturating_sub(24),
            135,
        );
        return (back, top, bot, lip, rim, engrave);
    }
    let default_accent = rusty_services::file_manager::accent_rgba_for_index(0, pal.is_dark);
    if pal.accent != default_accent {
        let base = pal.accent;
        let top = lerp_rgba(
            base,
            rgba(255, 255, 255, 255),
            if pal.is_dark { 0.28 } else { 0.34 },
        );
        let bot = lerp_rgba(
            base,
            rgba(0, 0, 0, 255),
            if pal.is_dark { 0.14 } else { 0.10 },
        );
        let back = lerp_rgba(base, rgba(0, 0, 0, 255), 0.24);
        let lip = lerp_rgba(base, rgba(255, 255, 255, 255), 0.76);
        let rim = rgba(back[0], back[1], back[2], 130);
        let engrave = rgba(
            back[0].saturating_sub(20),
            back[1].saturating_sub(20),
            back[2].saturating_sub(20),
            128,
        );
        return (back, top, bot, lip, rim, engrave);
    }
    if pal.is_dark {
        (
            rgba(38, 128, 224, 255),
            rgba(116, 200, 255, 255),
            rgba(44, 142, 240, 255),
            rgba(222, 244, 255, 235),
            rgba(64, 154, 242, 130),
            rgba(20, 86, 162, 130),
        )
    } else {
        (
            rgba(42, 134, 230, 255),
            rgba(120, 204, 255, 255),
            rgba(48, 150, 244, 255),
            rgba(234, 248, 255, 240),
            rgba(42, 126, 214, 115),
            rgba(24, 92, 172, 122),
        )
    }
}

fn draw_folder_category_engraving(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    glyph_r: f32,
    folder_name: &str,
    col: [u8; 4],
) {
    let lower = folder_name.to_ascii_lowercase();
    let stroke = (glyph_r * 0.13).clamp(1.2, 2.4);
    match lower.as_str() {
        "desktop" | "рабочий стол" => {
            c.stroke_rounded_rect_f(
                cx - glyph_r * 0.78,
                cy - glyph_r * 0.56,
                glyph_r * 1.56,
                glyph_r * 1.02,
                2.2,
                stroke,
                col,
            );
            c.stroke_line_aa(
                cx - glyph_r * 0.45,
                cy + glyph_r * 0.18,
                cx + glyph_r * 0.45,
                cy + glyph_r * 0.18,
                stroke,
                col,
            );
        }
        "documents" | "документы" => draw_engraved_doc(c, cx, cy, glyph_r, stroke, col),
        "downloads" | "загрузки" => {
            c.stroke_circle_f(cx, cy, glyph_r * 0.75, stroke, col);
            c.stroke_line_aa(
                cx,
                cy - glyph_r * 0.38,
                cx,
                cy + glyph_r * 0.34,
                stroke,
                col,
            );
            c.stroke_polyline_aa(
                &[
                    (cx - glyph_r * 0.34, cy + glyph_r * 0.04),
                    (cx, cy + glyph_r * 0.4),
                    (cx + glyph_r * 0.34, cy + glyph_r * 0.04),
                ],
                false,
                stroke,
                col,
            );
        }
        "pictures" | "изображения" | "music" | "музыка" | "movies" | "videos" | "видео" =>
        {
            draw_engraved_media(c, cx, cy, glyph_r, &lower, stroke, col);
        }
        "public" | "shared" | "общедоступные" => {
            c.stroke_circle_f(cx, cy - glyph_r * 0.28, glyph_r * 0.26, stroke, col);
            c.stroke_polyline_aa(
                &[
                    (cx - glyph_r * 0.54, cy + glyph_r * 0.54),
                    (cx - glyph_r * 0.34, cy + glyph_r * 0.14),
                    (cx + glyph_r * 0.34, cy + glyph_r * 0.14),
                    (cx + glyph_r * 0.54, cy + glyph_r * 0.54),
                ],
                false,
                stroke,
                col,
            );
        }
        "applications" | "apps" => {
            c.stroke_line_aa(
                cx - glyph_r * 0.46,
                cy + glyph_r * 0.54,
                cx + glyph_r * 0.06,
                cy - glyph_r * 0.54,
                stroke,
                col,
            );
            c.stroke_line_aa(
                cx - glyph_r * 0.06,
                cy - glyph_r * 0.54,
                cx + glyph_r * 0.46,
                cy + glyph_r * 0.54,
                stroke,
                col,
            );
            c.stroke_line_aa(
                cx - glyph_r * 0.56,
                cy + glyph_r * 0.18,
                cx + glyph_r * 0.56,
                cy + glyph_r * 0.18,
                stroke,
                col,
            );
        }
        _ => {}
    }
}

fn draw_engraved_doc(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    glyph_r: f32,
    stroke: f32,
    col: [u8; 4],
) {
    let rw = glyph_r * 1.1;
    let rh = glyph_r * 1.32;
    let lx = cx - rw * 0.5;
    let ty = cy - rh * 0.5;
    c.stroke_polyline_aa(
        &[
            (lx, ty),
            (lx + rw * 0.62, ty),
            (lx + rw, ty + rh * 0.28),
            (lx + rw, ty + rh),
            (lx, ty + rh),
        ],
        true,
        stroke,
        col,
    );
    c.stroke_polyline_aa(
        &[
            (lx + rw * 0.62, ty),
            (lx + rw * 0.62, ty + rh * 0.28),
            (lx + rw, ty + rh * 0.28),
        ],
        false,
        stroke * 0.9,
        col,
    );
}

fn draw_engraved_media(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    glyph_r: f32,
    lower: &str,
    stroke: f32,
    col: [u8; 4],
) {
    if lower == "pictures" || lower == "изображения" {
        c.stroke_rounded_rect_f(
            cx - glyph_r * 0.76,
            cy - glyph_r * 0.62,
            glyph_r * 1.52,
            glyph_r * 1.24,
            2.2,
            stroke,
            col,
        );
        c.fill_circle_f(cx - glyph_r * 0.32, cy - glyph_r * 0.22, stroke * 0.9, col);
        c.stroke_polyline_aa(
            &[
                (cx - glyph_r * 0.58, cy + glyph_r * 0.42),
                (cx - glyph_r * 0.14, cy - glyph_r * 0.02),
                (cx + glyph_r * 0.18, cy + glyph_r * 0.24),
                (cx + glyph_r * 0.38, cy + glyph_r * 0.06),
                (cx + glyph_r * 0.60, cy + glyph_r * 0.42),
            ],
            false,
            stroke,
            col,
        );
    } else if lower == "music" || lower == "музыка" {
        c.stroke_polyline_aa(
            &[
                (cx - glyph_r * 0.28, cy + glyph_r * 0.44),
                (cx - glyph_r * 0.28, cy - glyph_r * 0.44),
                (cx + glyph_r * 0.42, cy - glyph_r * 0.54),
                (cx + glyph_r * 0.42, cy + glyph_r * 0.32),
            ],
            false,
            stroke,
            col,
        );
        c.fill_circle_f(cx - glyph_r * 0.42, cy + glyph_r * 0.44, stroke * 1.15, col);
        c.fill_circle_f(cx + glyph_r * 0.28, cy + glyph_r * 0.32, stroke * 1.15, col);
    } else {
        c.stroke_rounded_rect_f(
            cx - glyph_r * 0.78,
            cy - glyph_r * 0.58,
            glyph_r * 1.56,
            glyph_r * 1.16,
            2.0,
            stroke,
            col,
        );
        c.stroke_line_aa(
            cx - glyph_r * 0.42,
            cy - glyph_r * 0.58,
            cx - glyph_r * 0.42,
            cy + glyph_r * 0.58,
            stroke * 0.9,
            col,
        );
        c.stroke_line_aa(
            cx + glyph_r * 0.42,
            cy - glyph_r * 0.58,
            cx + glyph_r * 0.42,
            cy + glyph_r * 0.58,
            stroke * 0.9,
            col,
        );
    }
}

fn draw_large_app_artwork(c: &mut Canvas<'_>, cx: f32, cy: f32, size: f32, pal: &Palette) {
    let box_sz = (size * 0.84).round();
    let left = (cx - box_sz * 0.5).round();
    let top = (cy - box_sz * 0.5).round();
    let top_col = lerp_rgba(pal.accent, rgba(255, 255, 255, 255), 0.24);
    c.fill_rounded_rect_vgrad_f(
        left,
        top,
        box_sz,
        box_sz,
        box_sz * 0.22,
        top_col,
        pal.accent,
    );
    c.stroke_rounded_rect_f(
        left,
        top,
        box_sz,
        box_sz,
        box_sz * 0.22,
        1.0,
        pal.pill_border,
    );
    let fg = rgba(255, 255, 255, 240);
    let r = box_sz * 0.25;
    c.stroke_line_aa(cx - r * 0.85, cy + r, cx + r * 0.1, cy - r, 2.0, fg);
    c.stroke_line_aa(cx - r * 0.1, cy - r, cx + r * 0.85, cy + r, 2.0, fg);
    c.stroke_line_aa(cx - r, cy + r * 0.35, cx + r, cy + r * 0.35, 1.9, fg);
}

fn draw_large_document_artwork(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    size: f32,
    entry: &FileEntry,
    pal: &Palette,
) {
    let w = (size * 0.72).round();
    let h = (size * 0.86).round();
    let left = (cx - w * 0.5).round();
    let top = (cy - h * 0.5).round();
    let fold = (w * 0.26).round();
    let fill = if pal.is_dark {
        rgba(46, 46, 52, 255)
    } else {
        rgba(255, 255, 255, 255)
    };
    let fold_col = if pal.is_dark {
        rgba(68, 68, 76, 255)
    } else {
        rgba(232, 236, 244, 255)
    };
    c.fill_rounded_rect_f(left + 1.0, top + 2.0, w, h, 5.0, rgba(0, 0, 0, 22));
    c.fill_poly_aa(
        &[
            (left, top),
            (left + w - fold, top),
            (left + w, top + fold),
            (left + w, top + h),
            (left, top + h),
        ],
        fill,
    );
    c.fill_poly_aa(
        &[
            (left + w - fold, top),
            (left + w - fold, top + fold),
            (left + w, top + fold),
        ],
        fold_col,
    );
    c.stroke_polyline_aa(
        &[
            (left, top),
            (left + w - fold, top),
            (left + w, top + fold),
            (left + w, top + h),
            (left, top + h),
        ],
        true,
        1.1,
        pal.pill_border,
    );
    let ext = entry
        .name
        .rsplit_once('.')
        .map_or("FILE", |(_, e)| e)
        .to_ascii_uppercase();
    let short_ext: String = ext.chars().take(4).collect();
    let font_sz = (size * 0.16).clamp(8.5, 13.5);
    let tw = Canvas::measure_text(&short_ext, font_sz, true);
    #[allow(clippy::cast_possible_truncation)]
    let clip = crate::layout::Rect::new(left as i32, top as i32, w as i32, h as i32);
    #[allow(clippy::cast_possible_truncation)]
    c.draw_text(
        &short_ext,
        cx as i32 - tw / 2,
        cy as i32 - 3,
        font_sz,
        true,
        pal.accent,
        clip,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_chevron_aa(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    left: bool,
    half_w: f32,
    half_h: f32,
    thickness: f32,
    col: [u8; 4],
) {
    let dir = if left { 1.0 } else { -1.0 };
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    let pts = [
        (sx + dir * half_w * 0.55, sy - half_h),
        (sx - dir * half_w * 0.55, sy),
        (sx + dir * half_w * 0.55, sy + half_h),
    ];
    c.stroke_polyline_aa(&pts, false, thickness, col);
}

/// Renders crisp pixel-aligned view-mode icons in the toolbar pill.
pub(crate) fn draw_view_mode_icon(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    mode: FileViewMode,
    col: [u8; 4],
) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    match mode {
        FileViewMode::Icons => {
            for dy in [-5.0_f32, 1.0] {
                for dx in [-5.0_f32, 1.0] {
                    c.stroke_rounded_rect_f(
                        snap_edge(sx + dx),
                        snap_edge(sy + dy),
                        4.0,
                        4.0,
                        0.9,
                        1.15,
                        col,
                    );
                }
            }
        }
        FileViewMode::List => {
            for dy in [-4.0_f32, 0.0, 4.0] {
                let py = snap_half(sy + dy);
                c.fill_circle_f(snap_half(sx - 5.0), py, 1.05, col);
                c.stroke_line_aa(sx - 2.0, py, sx + 6.0, py, 1.25, col);
            }
        }
        FileViewMode::Columns => {
            let lx = snap_edge(sx - 7.0);
            let ty = snap_edge(sy - 5.5);
            c.stroke_rounded_rect_f(lx, ty, 14.0, 11.0, 2.0, 1.2, col);
            c.stroke_line_aa(
                snap_half(sx - 2.5),
                ty + 1.0,
                snap_half(sx - 2.5),
                ty + 10.0,
                1.1,
                col,
            );
            c.stroke_line_aa(
                snap_half(sx + 2.5),
                ty + 1.0,
                snap_half(sx + 2.5),
                ty + 10.0,
                1.1,
                col,
            );
        }
        FileViewMode::Gallery => {
            let lx = snap_edge(sx - 6.5);
            let ty = snap_edge(sy - 5.5);
            c.stroke_rounded_rect_f(lx, ty, 13.0, 7.0, 1.4, 1.15, col);
            for dx in [-5.5_f32, -2.5, 0.5, 3.5] {
                c.fill_rounded_rect_f(snap_edge(sx + dx), snap_edge(sy + 3.0), 2.0, 2.0, 0.4, col);
            }
        }
    }
}

pub(crate) fn draw_sort_pill_glyph(c: &mut Canvas<'_>, pill_x: f32, cy: f32, col: [u8; 4]) {
    let gx = snap_half(pill_x + 19.0);
    let gy = snap_half(cy);
    for dy in [-2.6_f32, 0.8] {
        for dx in [-5.8_f32, -1.5, 2.8] {
            c.fill_rounded_rect_f(snap_edge(gx + dx), snap_edge(gy + dy), 3.0, 2.0, 0.7, col);
        }
    }
    let vx = snap_half(pill_x + 39.0);
    c.stroke_polyline_aa(
        &[(vx - 3.0, gy - 1.5), (vx, gy + 1.5), (vx + 3.0, gy - 1.5)],
        false,
        1.3,
        col,
    );
}

/// Draws the RustyOS `Share` tray + upward arrow toolbar glyph (`RUSTYOS-071`).
pub(crate) fn draw_share_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    // Tray open at the top middle (`square.and.arrow.up`)
    c.stroke_polyline_aa(
        &[
            (sx - 2.3, sy - 2.2),
            (sx - 5.2, sy - 2.2),
            (sx - 5.2, sy + 5.8),
            (sx + 5.2, sy + 5.8),
            (sx + 5.2, sy - 2.2),
            (sx + 2.3, sy - 2.2),
        ],
        false,
        1.25,
        col,
    );
    // Vertical stem + upward chevron arrow
    c.stroke_line_aa(sx, sy + 2.2, sx, sy - 5.8, 1.25, col);
    c.stroke_polyline_aa(
        &[(sx - 2.8, sy - 3.2), (sx, sy - 6.0), (sx + 2.8, sy - 3.2)],
        false,
        1.25,
        col,
    );
}

pub(crate) fn draw_tag_outline_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    c.stroke_polyline_aa(
        &[
            (sx - 5.0, sy - 5.0),
            (sx - 0.6, sy - 5.3),
            (sx + 5.3, sy + 0.6),
            (sx + 0.6, sy + 5.3),
            (sx - 5.3, sy - 0.6),
        ],
        true,
        1.2,
        col,
    );
    c.stroke_circle_f(sx - 2.2, sy - 2.2, 1.1, 0.95, col);
}

pub(crate) fn draw_ellipsis_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    for dx in [-4.5_f32, 0.0, 4.5] {
        c.fill_circle_f(sx + dx, sy, 1.25, col);
    }
}

pub(crate) fn draw_search_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    c.stroke_circle_f(sx - 1.2, sy - 1.2, 4.2, 1.35, col);
    c.stroke_line_aa(sx + 1.8, sy + 1.8, sx + 5.0, sy + 5.0, 1.5, col);
}

pub(crate) fn draw_plus_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, half: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    c.stroke_line_aa(sx - half, sy, sx + half, sy, 1.35, col);
    c.stroke_line_aa(sx, sy - half, sx, sy + half, 1.35, col);
}

pub(crate) fn draw_close_x_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, half: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    c.stroke_line_aa(sx - half, sy - half, sx + half, sy + half, 1.35, col);
    c.stroke_line_aa(sx - half, sy + half, sx + half, sy - half, 1.35, col);
}
