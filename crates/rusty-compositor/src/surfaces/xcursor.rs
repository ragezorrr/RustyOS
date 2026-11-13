//! `XCursor` theme discovery, binary parser, and fallback arrow cursor rasterization.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::input::pointer::CursorIcon;
use smithay::utils::{Buffer as BufferCoord, Logical, Physical, Point, Size, Transform};

use super::CachedCursor;
use super::sdf::{composite_over, premul_pixel, signed_distance_polygon};

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(super) fn sample_cursor_arrow_pixel(px: i32, py: i32) -> [u8; 4] {
    const ARROW_VERTS: [(f32, f32); 7] = [
        (2.5, 2.5),
        (2.5, 18.2),
        (6.4, 14.5),
        (8.9, 20.3),
        (11.1, 19.3),
        (8.6, 13.5),
        (13.8, 13.3),
    ];
    let x = px as f32 + 0.5;
    let y = py as f32 + 0.5;
    let sd = signed_distance_polygon(x, y, &ARROW_VERTS);
    let shadow_sd = signed_distance_polygon(x - 0.7, y - 1.1, &ARROW_VERTS);

    let shadow_alpha = ((3.1 - shadow_sd) / 2.2).clamp(0.0, 1.0) * 0.38;
    let mut out = premul_pixel(8.0, 5.0, 18.0, shadow_alpha);

    let rim_cov = (1.5 - sd).clamp(0.0, 1.0);
    if rim_cov > 0.0 {
        let rim = premul_pixel(250.0, 248.0, 255.0, rim_cov);
        out = composite_over(rim, out);
    }

    let fill_cov = (0.35 - sd).clamp(0.0, 1.0);
    if fill_cov > 0.0 {
        let fill = premul_pixel(22.0, 16.0, 38.0, fill_cov);
        out = composite_over(fill, out);
    }

    out
}

pub(super) fn rasterize_cursor_arrow_buffer() -> MemoryRenderBuffer {
    const CURSOR_DIM: i32 = 24;
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (CURSOR_DIM, CURSOR_DIM),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let w_u = usize::try_from(CURSOR_DIM).unwrap_or(0);
            for py in 0..CURSOR_DIM {
                for px in 0..CURSOR_DIM {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * w_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        pixels[idx..idx + 4].copy_from_slice(&sample_cursor_arrow_pixel(px, py));
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((CURSOR_DIM, CURSOR_DIM)),
            )])
        });
    }
    mem_buf
}

pub(super) fn xcursor_target_size() -> u32 {
    env::var("XCURSOR_SIZE")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&n| (8..=128).contains(&n))
        .unwrap_or(24)
}

/// Computes the target `XCursor` pixel size for an output scale factor (`RUSTYOS-046`).
#[must_use]
pub(crate) fn xcursor_target_pixel_size(output_scale: f64) -> u32 {
    let clamped = crate::config::clamp_display_scale(output_scale);
    let px = (f64::from(xcursor_target_size()) * clamped)
        .round()
        .clamp(16.0, 128.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        px as u32
    }
}

/// Maps logical pointer + hotspot coordinates into snapped physical cursor origin (`RUSTYOS-046`).
#[must_use]
pub(crate) fn scaled_cursor_physical_origin(
    pointer_loc: Point<i32, Logical>,
    hotspot: Point<f64, Logical>,
    output_scale: f64,
) -> Point<f64, Physical> {
    let clamped = crate::config::clamp_display_scale(output_scale);
    let logical_origin = Point::<f64, Logical>::from((
        f64::from(pointer_loc.x) - hotspot.x,
        f64::from(pointer_loc.y) - hotspot.y,
    ));
    logical_origin
        .to_physical(clamped)
        .to_i32_round::<i32>()
        .to_f64()
}

/// Checks whether a logical cursor size fits within a hardware DRM cursor plane at `output_scale` (`RUSTYOS-046`).
#[must_use]
pub(crate) fn cursor_fits_plane_limit(
    logical_size: Size<i32, Logical>,
    output_scale: f64,
    plane_limit: Option<Size<u32, BufferCoord>>,
) -> bool {
    let Some(limit) = plane_limit else {
        return true;
    };
    let clamped = crate::config::clamp_display_scale(output_scale);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let phys_w = (f64::from(logical_size.w.max(0)) * clamped)
        .round()
        .max(0.0) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let phys_h = (f64::from(logical_size.h.max(0)) * clamped)
        .round()
        .max(0.0) as u32;
    phys_w > 0 && phys_h > 0 && phys_w <= limit.w && phys_h <= limit.h
}

/// Resolves whether a cursor element should request hardware cursor plane placement or fallback to primary composition (`RUSTYOS-046`).
#[must_use]
pub(crate) fn resolve_cursor_element_kind(
    prefer_hw_cursor: bool,
    logical_size: Size<i32, Logical>,
    output_scale: f64,
    plane_limit: Option<Size<u32, BufferCoord>>,
) -> Kind {
    if prefer_hw_cursor && cursor_fits_plane_limit(logical_size, output_scale, plane_limit) {
        Kind::Cursor
    } else {
        Kind::Unspecified
    }
}

pub(super) fn xcursor_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = env::var("HOME")
        && !home.trim().is_empty()
    {
        roots.push(PathBuf::from(&home).join(".icons"));
        roots.push(PathBuf::from(&home).join(".local/share/icons"));
    }
    if let Ok(xdg_dirs) = env::var("XDG_DATA_DIRS") {
        for dir in xdg_dirs.split(':').filter(|s| !s.trim().is_empty()) {
            roots.push(PathBuf::from(dir).join("icons"));
        }
    } else {
        roots.push(PathBuf::from("/usr/local/share/icons"));
        roots.push(PathBuf::from("/usr/share/icons"));
    }
    roots
}

pub(super) fn read_theme_inherits(theme_dir: &Path) -> Vec<String> {
    for file in ["cursor.theme", "index.theme"] {
        let path = theme_dir.join(file);
        if let Ok(text) = fs::read_to_string(path) {
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("Inherits=") {
                    return rest
                        .split([',', ';'])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
            }
        }
    }
    Vec::new()
}

pub(super) fn xcursor_candidate_names(icon: CursorIcon) -> Vec<&'static str> {
    let mut names = vec![icon.name()];
    let aliases: &[&str] = match icon {
        CursorIcon::Default => &["left_ptr", "arrow", "top_left_arrow"],
        CursorIcon::ContextMenu => &["context-menu", "left_ptr"],
        CursorIcon::Help => &["question_arrow", "help", "left_ptr"],
        CursorIcon::Pointer => &["hand2", "hand1", "pointing_hand", "hand"],
        CursorIcon::Progress => &["left_ptr_watch", "half-busy", "watch"],
        CursorIcon::Wait => &["watch", "clock"],
        CursorIcon::Cell => &["plus", "crosshair"],
        CursorIcon::Crosshair => &["tcross", "cross", "plus"],
        CursorIcon::Text => &["xterm", "ibeam", "text"],
        CursorIcon::VerticalText => &["vertical-text", "xterm"],
        CursorIcon::Alias => &["link", "dnd-link"],
        CursorIcon::Copy => &["copy", "dnd-copy", "plus"],
        CursorIcon::Move => &["fleur", "size_all", "all-scroll"],
        CursorIcon::NoDrop => &["no-drop", "dnd-no-drop", "crossed_circle"],
        CursorIcon::NotAllowed => &["crossed_circle", "forbidden", "circle"],
        CursorIcon::Grab => &["openhand", "hand1", "hand2"],
        CursorIcon::Grabbing => &["closedhand", "grabbing", "fleur"],
        CursorIcon::EResize => &["right_side", "sb_right_arrow", "ew-resize"],
        CursorIcon::NResize => &["top_side", "sb_up_arrow", "ns-resize"],
        CursorIcon::NeResize => &["top_right_corner", "nesw-resize"],
        CursorIcon::NwResize => &["top_left_corner", "nwse-resize"],
        CursorIcon::SResize => &["bottom_side", "sb_down_arrow", "ns-resize"],
        CursorIcon::SeResize => &["bottom_right_corner", "nwse-resize"],
        CursorIcon::SwResize => &["bottom_left_corner", "nesw-resize"],
        CursorIcon::WResize => &["left_side", "sb_left_arrow", "ew-resize"],
        CursorIcon::EwResize | CursorIcon::ColResize => {
            &["sb_h_double_arrow", "h_double_arrow", "size_hor"]
        }
        CursorIcon::NsResize | CursorIcon::RowResize => {
            &["sb_v_double_arrow", "v_double_arrow", "size_ver"]
        }
        CursorIcon::NeswResize => &["fd_double_arrow", "size_bdiag", "top_right_corner"],
        CursorIcon::NwseResize => &["bd_double_arrow", "size_fdiag", "top_left_corner"],
        CursorIcon::AllScroll => &["fleur", "size_all", "move"],
        CursorIcon::ZoomIn => &["zoom-in"],
        CursorIcon::ZoomOut => &["zoom-out"],
        _ => &["left_ptr"],
    };
    for &alias in aliases {
        if !names.contains(&alias) {
            names.push(alias);
        }
    }
    if !names.contains(&"left_ptr") {
        names.push("left_ptr");
    }
    names
}

pub(super) fn load_xcursor_for_icon(
    icon: CursorIcon,
    theme_override: Option<&str>,
    target_size: u32,
    output_scale: f64,
) -> Option<CachedCursor> {
    let mut themes: Vec<String> = Vec::new();
    if let Some(t) = theme_override.map(str::trim).filter(|t| !t.is_empty()) {
        themes.push(t.to_owned());
    }
    if let Ok(env_theme) = env::var("XCURSOR_THEME")
        && !env_theme.trim().is_empty()
        && !themes.iter().any(|t| t == env_theme.trim())
    {
        themes.push(env_theme.trim().to_owned());
    }
    for fallback in [
        "default",
        "Adwaita",
        "breeze_cursors",
        "Breeze",
        "DMZ-White",
    ] {
        if !themes.iter().any(|t| t == fallback) {
            themes.push(fallback.to_owned());
        }
    }

    let roots = xcursor_search_roots();
    // Expand 1-level `Inherits=` (important on distros where `default/index.theme` points to active theme).
    let mut expanded_themes = Vec::new();
    for theme in themes {
        if !expanded_themes.contains(&theme) {
            expanded_themes.push(theme.clone());
        }
        for root in &roots {
            for inh in read_theme_inherits(&root.join(&theme)) {
                if !expanded_themes.contains(&inh) {
                    expanded_themes.push(inh);
                }
            }
        }
    }

    let names = xcursor_candidate_names(icon);
    let nominal_size = xcursor_target_size();
    let clamped_scale = crate::config::clamp_display_scale(output_scale);

    for theme in &expanded_themes {
        for root in &roots {
            let cursors_dir = root.join(theme).join("cursors");
            if !cursors_dir.is_dir() {
                continue;
            }
            for &name in &names {
                let candidate = cursors_dir.join(name);
                if let Ok(bytes) = fs::read(&candidate)
                    && let Some((w, h, xhot, yhot, bgra)) = parse_xcursor_bytes(&bytes, target_size)
                {
                    let eff_scale =
                        if clamped_scale > 1.0 && f64::from(w.max(h)) > f64::from(nominal_size) {
                            (f64::from(w.max(h)) / f64::from(nominal_size)).min(clamped_scale)
                        } else {
                            1.0
                        };
                    #[allow(clippy::cast_possible_truncation)]
                    let logical_size = Size::from((
                        (f64::from(w) / eff_scale).round().max(1.0) as i32,
                        (f64::from(h) / eff_scale).round().max(1.0) as i32,
                    ));
                    let raw_size = Size::from((f64::from(w), f64::from(h)));
                    let hotspot =
                        Point::from((f64::from(xhot) / eff_scale, f64::from(yhot) / eff_scale));
                    let buffer = MemoryRenderBuffer::from_slice(
                        &bgra,
                        Fourcc::Argb8888,
                        (w, h),
                        1,
                        Transform::Normal,
                        None,
                    );
                    return Some(CachedCursor {
                        buffer,
                        logical_size,
                        raw_size,
                        hotspot,
                    });
                }
            }
        }
    }
    None
}

/// Parses a freedesktop/X11 binary `Xcur` cursor file, choosing the frame closest to `target_size`
/// and returning `(width, height, xhot, yhot, premul_bgra_bytes)` (`RUSTYOS-034`).
pub(crate) fn parse_xcursor_bytes(
    data: &[u8],
    target_size: u32,
) -> Option<(i32, i32, i32, i32, Vec<u8>)> {
    const XCURSOR_MAGIC: &[u8; 4] = b"Xcur";
    const XCURSOR_IMAGE_TYPE: u32 = 0xfffd_0002;

    if data.len() < 16 || &data[0..4] != XCURSOR_MAGIC {
        return None;
    }
    let header_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let ntoc = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if header_size < 16 || ntoc == 0 || ntoc > 4096 {
        return None;
    }
    let toc_end = header_size.checked_add(ntoc.checked_mul(12)?)?;
    if toc_end > data.len() {
        return None;
    }

    let mut best_pos: Option<(u32, bool, usize)> = None;
    for i in 0..ntoc {
        let off = header_size + i * 12;
        let chunk_type =
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        if chunk_type != XCURSOR_IMAGE_TYPE {
            continue;
        }
        let subtype =
            u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        let pos = u32::from_le_bytes([data[off + 8], data[off + 9], data[off + 10], data[off + 11]])
            as usize;
        let diff = subtype.abs_diff(target_size);
        let prefer_ge = subtype >= target_size;
        let replace = match best_pos {
            None => true,
            Some((best_diff, best_ge, _)) => {
                diff < best_diff || (diff == best_diff && prefer_ge && !best_ge)
            }
        };
        if replace {
            best_pos = Some((diff, prefer_ge, pos));
        }
    }

    let (_, _, pos) = best_pos?;
    let chunk_hdr_end = pos.checked_add(36)?;
    if chunk_hdr_end > data.len() {
        return None;
    }
    let c_type = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
    if c_type != XCURSOR_IMAGE_TYPE {
        return None;
    }
    let width = u32::from_le_bytes([
        data[pos + 16],
        data[pos + 17],
        data[pos + 18],
        data[pos + 19],
    ]);
    let height = u32::from_le_bytes([
        data[pos + 20],
        data[pos + 21],
        data[pos + 22],
        data[pos + 23],
    ]);
    let xhot = u32::from_le_bytes([
        data[pos + 24],
        data[pos + 25],
        data[pos + 26],
        data[pos + 27],
    ]);
    let yhot = u32::from_le_bytes([
        data[pos + 28],
        data[pos + 29],
        data[pos + 30],
        data[pos + 31],
    ]);
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return None;
    }
    let pixel_count = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    let pixel_bytes = pixel_count.checked_mul(4)?;
    let pixels_end = chunk_hdr_end.checked_add(pixel_bytes)?;
    if pixels_end > data.len() {
        return None;
    }

    // XCursor pixels are already premultiplied LE u32 ARGB `(A<<24)|(R<<16)|(G<<8)|B`, which in LE bytes
    // is `[B, G, R, A]` — exact byte layout for `Fourcc::Argb8888`!
    let bgra = data[chunk_hdr_end..pixels_end].to_vec();
    let w_i32 = i32::try_from(width).ok()?;
    let h_i32 = i32::try_from(height).ok()?;
    let xh_i32 = i32::try_from(xhot.min(width.saturating_sub(1))).ok()?;
    let yh_i32 = i32::try_from(yhot.min(height.saturating_sub(1))).ok()?;
    Some((w_i32, h_i32, xh_i32, yh_i32, bgra))
}
