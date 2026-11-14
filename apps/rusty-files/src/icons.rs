//! Crisp pixel-aligned 14-18 px UI vector glyphs and 4x4 SSAA Golden Gate
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

/// Renders a crisp pixel-aligned 14-16 px Golden Gate sidebar icon centered at `(cx, cy)`.
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
            c.fill_circle_f(sx, sy, 4.6, rgba(red, green, blue, 255));
        }
        SidebarIconKind::Recents => {
            c.stroke_circle_f(sx, sy, 5.4, 1.25, accent);
            c.stroke_polyline_aa(
                &[(sx, sy - 3.0), (sx, sy), (sx + 2.5, sy + 1.5)],
                false,
                1.25,
                accent,
            );
        }
        SidebarIconKind::Shared => draw_shared_people_icon(c, sx, sy, accent),
        SidebarIconKind::Applications => draw_applications_a_icon(c, sx, sy, accent),
        SidebarIconKind::Desktop => {
            c.stroke_rounded_rect_f(
                snap_edge(sx - 6.0),
                snap_edge(sy - 5.0),
                12.0,
                8.0,
                1.5,
                1.15,
                accent,
            );
            c.stroke_line_aa(
                sx - 4.0,
                snap_half(sy + 1.0),
                sx + 4.0,
                snap_half(sy + 1.0),
                1.0,
                accent,
            );
            c.stroke_line_aa(sx, sy + 3.0, sx, sy + 5.0, 1.2, accent);
            c.stroke_line_aa(
                sx - 3.0,
                snap_half(sy + 5.0),
                sx + 3.0,
                snap_half(sy + 5.0),
                1.2,
                accent,
            );
        }
        SidebarIconKind::Documents => draw_dogear_document_icon(c, sx, sy, accent, 1.15),
        SidebarIconKind::Downloads => {
            c.stroke_circle_f(sx, sy, 5.4, 1.2, accent);
            c.stroke_line_aa(sx, sy - 2.8, sx, sy + 2.4, 1.25, accent);
            c.stroke_polyline_aa(
                &[(sx - 2.4, sy + 0.3), (sx, sy + 2.7), (sx + 2.4, sy + 0.3)],
                false,
                1.25,
                accent,
            );
        }
        SidebarIconKind::Home => draw_home_house_icon(c, sx, sy, neutral),
        SidebarIconKind::Volume => draw_drive_volume_icon(c, sx, sy, pal),
        SidebarIconKind::Network => draw_network_globe_icon(c, sx, sy, accent),
        SidebarIconKind::Trash => draw_trash_can_icon(c, sx, sy, neutral),
    }
}

fn draw_shared_people_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_circle_f(cx - 1.8, cy - 2.2, 1.9, 1.15, col);
    c.stroke_polyline_aa(
        &[
            (cx - 5.0, cy + 4.2),
            (cx - 3.6, cy + 1.5),
            (cx, cy + 1.5),
            (cx + 1.4, cy + 4.2),
        ],
        false,
        1.15,
        col,
    );
    c.stroke_circle_f(cx + 2.7, cy - 2.4, 1.6, 1.1, col);
    c.stroke_polyline_aa(
        &[
            (cx + 1.8, cy + 1.5),
            (cx + 4.1, cy + 1.5),
            (cx + 5.3, cy + 4.0),
        ],
        false,
        1.1,
        col,
    );
}

fn draw_applications_a_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_line_aa(cx - 4.0, cy + 4.5, cx + 0.4, cy - 4.5, 1.25, col);
    c.stroke_line_aa(cx - 0.4, cy - 4.5, cx + 4.0, cy + 4.5, 1.25, col);
    c.stroke_line_aa(
        cx - 4.8,
        snap_half(cy + 1.5),
        cx + 4.8,
        snap_half(cy + 1.5),
        1.2,
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
        &[(cx - 6.0, cy - 0.2), (cx, cy - 5.4), (cx + 6.0, cy - 0.2)],
        false,
        1.3,
        col,
    );
    c.stroke_line_aa(
        snap_half(cx + 3.0),
        cy - 4.4,
        snap_half(cx + 3.0),
        cy - 2.6,
        1.2,
        col,
    );
    let left = snap_half(cx - 4.2);
    let right = snap_half(cx + 4.2);
    let bot = snap_half(cy + 5.0);
    c.stroke_polyline_aa(
        &[
            (left, cy - 1.2),
            (left, bot),
            (right, bot),
            (right, cy - 1.2),
        ],
        false,
        1.2,
        col,
    );
    c.stroke_rounded_rect_f(
        snap_edge(cx - 1.5),
        snap_edge(cy + 0.8),
        3.0,
        4.2,
        0.8,
        1.05,
        col,
    );
}

fn draw_drive_volume_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, pal: &Palette) {
    let (top, bot, border, led) = if pal.is_dark {
        (
            rgba(104, 93, 130, 255),
            rgba(74, 65, 96, 255),
            rgba(156, 144, 184, 255),
            rgba(196, 185, 224, 255),
        )
    } else {
        (
            rgba(232, 236, 242, 255),
            rgba(198, 204, 214, 255),
            rgba(110, 118, 132, 255),
            rgba(92, 100, 114, 255),
        )
    };
    let left = snap_edge(cx - 6.0);
    let upper = snap_edge(cy - 4.0);
    c.fill_rounded_rect_vgrad_f(left, upper, 12.0, 8.0, 2.0, top, bot);
    c.stroke_rounded_rect_f(left, upper, 12.0, 8.0, 2.0, 1.05, border);
    c.stroke_line_aa(
        left + 1.5,
        snap_half(upper + 5.0),
        left + 10.5,
        snap_half(upper + 5.0),
        0.9,
        border,
    );
    c.fill_circle_f(left + 9.2, upper + 6.5, 0.75, led);
}

fn draw_network_globe_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    c.stroke_circle_f(cx, cy, 5.4, 1.2, col);
    c.stroke_line_aa(cx - 5.0, cy, cx + 5.0, cy, 1.0, col);
    c.stroke_line_aa(cx, cy - 5.0, cx, cy + 5.0, 1.0, col);
    c.stroke_ellipse_aa(cx, cy, 2.6, 5.3, 1.0, col);
}

fn draw_trash_can_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    let lid_y = snap_half(cy - 3.5);
    c.stroke_line_aa(cx - 5.0, lid_y, cx + 5.0, lid_y, 1.2, col);
    c.stroke_polyline_aa(
        &[
            (cx - 2.0, lid_y),
            (cx - 1.4, lid_y - 1.8),
            (cx + 1.4, lid_y - 1.8),
            (cx + 2.0, lid_y),
        ],
        false,
        1.1,
        col,
    );
    c.stroke_polyline_aa(
        &[
            (cx - 3.9, lid_y),
            (cx - 3.1, snap_half(cy + 5.2)),
            (cx + 3.1, snap_half(cy + 5.2)),
            (cx + 3.9, lid_y),
        ],
        false,
        1.15,
        col,
    );
    c.stroke_line_aa(cx - 1.3, cy - 1.2, cx - 1.0, cy + 3.3, 1.0, col);
    c.stroke_line_aa(cx + 1.3, cy - 1.2, cx + 1.0, cy + 3.3, 1.0, col);
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
            } else if pal.is_dark {
                (
                    rgba(116, 100, 214, 255),
                    rgba(160, 146, 252, 255),
                    rgba(124, 108, 230, 255),
                    rgba(196, 186, 255, 190),
                )
            } else {
                (
                    rgba(48, 138, 234, 255),
                    rgba(114, 194, 255, 255),
                    rgba(54, 148, 246, 255),
                    rgba(176, 224, 255, 220),
                )
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

/// Draws large high-detail 4x4 SSAA Golden Gate folder or file badge centered at `(cx, cy)`.
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
                size_tenths,
                qx,
                qy,
            };
            blit_or_render_sprite(c, key, dst_x, dst_y, sprite_dim, sprite_dim, |offscreen| {
                draw_large_golden_gate_folder(
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

fn draw_large_golden_gate_folder(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    size: f32,
    folder_name: &str,
    tag_tint: Option<FileTagColor>,
    pal: &Palette,
) {
    let w = size;
    let h = size * 0.78;
    let left = (cx - w * 0.5).round();
    let top = (cy - h * 0.5).round();
    let tab_w = w * 0.38;
    let tab_h = h * 0.14;
    let rad = (size * 0.09).clamp(3.0, 8.0);

    let (back_col, top_col, bot_col, lip_col, rim_col, engrave_col) =
        folder_palette_for(tag_tint, pal);

    // Subtle soft ambient drop shadow underneath folder body
    c.fill_rounded_rect_f(
        left + 1.0,
        top + tab_h + 3.0,
        w - 2.0,
        h - tab_h,
        rad,
        rgba(0, 0, 0, if pal.is_dark { 58 } else { 28 }),
    );

    // Back folder flap with 4x4 RGSS beveled Golden Gate tab transition
    let back_pts = [
        (left + 2.0, top + tab_h + 4.0),
        (left + 2.0, top + rad * 0.5),
        (left + rad * 0.5, top),
        (left + tab_w, top),
        (left + tab_w + tab_h * 1.15, top + tab_h),
        (left + w - rad * 0.5, top + tab_h),
        (left + w, top + tab_h + rad * 0.5),
        (left + w, top + h * 0.45),
        (left, top + h * 0.45),
    ];
    c.fill_poly_aa(&back_pts, back_col);

    // Subtle crisp paper sheet highlight peeking along the back/front seam
    c.fill_rounded_rect_f(left + 3.0, top + tab_h + 1.5, w - 6.0, 4.0, 1.5, lip_col);

    // Front folder body with Golden Gate vertical sky-blue (or tag-tinted) gradient
    let front_top = top + tab_h + 3.0;
    let front_h = h - tab_h - 3.0;
    c.fill_rounded_rect_vgrad_f(left, front_top, w, front_h, rad, top_col, bot_col);
    c.stroke_line_aa(
        left + rad,
        front_top + 0.8,
        left + w - rad,
        front_top + 0.8,
        1.0,
        rgba(255, 255, 255, 115),
    );
    c.stroke_rounded_rect_f(left, front_top, w, front_h, rad, 1.0, rim_col);

    draw_folder_category_engraving(
        c,
        cx,
        front_top + front_h * 0.52,
        size * 0.36,
        folder_name,
        engrave_col,
    );
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
            125,
        );
        return (back, top, bot, lip, rim, engrave);
    }
    if pal.is_dark {
        (
            rgba(102, 86, 198, 255),
            rgba(156, 140, 252, 255),
            rgba(116, 98, 224, 255),
            rgba(220, 212, 255, 220),
            rgba(182, 168, 255, 130),
            rgba(62, 48, 138, 130),
        )
    } else {
        (
            rgba(50, 142, 232, 255),
            rgba(122, 202, 255, 255),
            rgba(60, 162, 246, 255),
            rgba(228, 245, 255, 235),
            rgba(46, 132, 218, 115),
            rgba(28, 98, 178, 110),
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
        rgba(47, 38, 66, 255)
    } else {
        rgba(255, 255, 255, 255)
    };
    let fold_col = if pal.is_dark {
        rgba(71, 58, 98, 255)
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
    for dy in [-4.5_f32, -1.0, 2.5] {
        for dx in [-5.0_f32, -1.0, 3.0] {
            c.fill_rounded_rect_f(snap_edge(gx + dx), snap_edge(gy + dy), 2.0, 2.0, 0.5, col);
        }
    }
    let vx = snap_half(pill_x + 38.5);
    c.stroke_polyline_aa(
        &[(vx - 3.0, gy - 1.5), (vx, gy + 1.5), (vx + 3.0, gy - 1.5)],
        false,
        1.3,
        col,
    );
}

pub(crate) fn draw_folder_plus_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    c.stroke_polyline_aa(
        &[
            (sx - 6.0, sy - 4.5),
            (sx - 1.8, sy - 4.5),
            (sx - 0.3, sy - 2.8),
            (sx + 6.0, sy - 2.8),
            (sx + 6.0, sy + 4.5),
            (sx - 6.0, sy + 4.5),
        ],
        true,
        1.2,
        col,
    );
    c.stroke_line_aa(sx, sy - 0.8, sx, sy + 2.8, 1.15, col);
    c.stroke_line_aa(
        sx - 1.8,
        snap_half(sy + 1.0),
        sx + 1.8,
        snap_half(sy + 1.0),
        1.15,
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
