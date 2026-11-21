//! Built-in `"RustyOS"` `@2x` SVG→PNG cursor theme with exact hotspot mapping
//! and area-averaged `HiDPI` resampling (`RUSTYOS-151`).

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::input::pointer::CursorIcon;
use smithay::utils::{Point, Rectangle, Size, Transform};

use super::CachedCursor;

const PNG_DEFAULT: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/default.png");
const PNG_POINTER: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/pointer.png");
const PNG_TEXT: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/text.png");
const PNG_CROSSHAIR: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/crosshair.png");
const PNG_MOVE: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/move.png");
const PNG_GRAB: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/grab.png");
const PNG_GRABBING: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/grabbing.png");
const PNG_EW_RESIZE: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/ew_resize.png");
const PNG_NS_RESIZE: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/ns_resize.png");
const PNG_NWSE_RESIZE: &[u8] =
    include_bytes!("../../../../assets/icons/cursors/png/nwse_resize.png");
const PNG_NESW_RESIZE: &[u8] =
    include_bytes!("../../../../assets/icons/cursors/png/nesw_resize.png");
const PNG_NOT_ALLOWED: &[u8] =
    include_bytes!("../../../../assets/icons/cursors/png/not_allowed.png");
const PNG_WAIT: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/wait.png");
const PNG_PROGRESS: &[u8] = include_bytes!("../../../../assets/icons/cursors/png/progress.png");

/// Built-in cursor asset specification: `(png_bytes, hotspot_x_norm, hotspot_y_norm)`.
/// Hotspots are normalized to `[0.0, 1.0]` in the 32x32 SVG viewBox.
#[must_use]
pub(super) const fn builtin_cursor_spec(icon: CursorIcon) -> (&'static [u8], f64, f64) {
    match icon {
        CursorIcon::Pointer => (PNG_POINTER, 12.2 / 32.0, 4.5 / 32.0),
        CursorIcon::Text | CursorIcon::VerticalText => (PNG_TEXT, 0.5, 0.5),
        CursorIcon::Crosshair | CursorIcon::Cell => (PNG_CROSSHAIR, 0.5, 0.5),
        CursorIcon::Move | CursorIcon::AllScroll => (PNG_MOVE, 0.5, 0.5),
        CursorIcon::Grab => (PNG_GRAB, 0.5, 0.5),
        CursorIcon::Grabbing => (PNG_GRABBING, 0.5, 0.5),
        CursorIcon::EwResize
        | CursorIcon::EResize
        | CursorIcon::WResize
        | CursorIcon::ColResize => (PNG_EW_RESIZE, 0.5, 0.5),
        CursorIcon::NsResize
        | CursorIcon::NResize
        | CursorIcon::SResize
        | CursorIcon::RowResize => (PNG_NS_RESIZE, 0.5, 0.5),
        CursorIcon::NwseResize | CursorIcon::NwResize | CursorIcon::SeResize => {
            (PNG_NWSE_RESIZE, 0.5, 0.5)
        }
        CursorIcon::NeswResize | CursorIcon::NeResize | CursorIcon::SwResize => {
            (PNG_NESW_RESIZE, 0.5, 0.5)
        }
        CursorIcon::NotAllowed | CursorIcon::NoDrop => (PNG_NOT_ALLOWED, 0.5, 0.5),
        CursorIcon::Wait => (PNG_WAIT, 0.5, 0.5),
        CursorIcon::Progress => (PNG_PROGRESS, 5.5 / 32.0, 3.5 / 32.0),
        _ => (PNG_DEFAULT, 5.5 / 32.0, 3.5 / 32.0),
    }
}

/// Returns true when the requested theme should use the built-in `"RustyOS"` cursor set.
#[must_use]
pub(super) fn is_builtin_rustyos_theme(theme_override: Option<&str>) -> bool {
    match theme_override.map(str::trim) {
        None | Some("") => true,
        Some(name) => name.eq_ignore_ascii_case("RustyOS"),
    }
}

/// Decodes and resamples a built-in RustyOS cursor into a `CachedCursor` at `target_px`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn load_builtin_cursor(
    icon: CursorIcon,
    target_px: u32,
    output_scale: f64,
) -> Option<CachedCursor> {
    let (png_bytes, hot_x, hot_y) = builtin_cursor_spec(icon);
    let (src_w, src_h, rgba) = rusty_services::image_decode::decode_png_rgba(png_bytes)?;
    if src_w == 0 || src_h == 0 {
        return None;
    }
    let dim = target_px.clamp(16, 128) as usize;
    let dim_i32 = i32::try_from(dim).unwrap_or(24);
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (dim_i32, dim_i32),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|buf| {
            resample_rgba_to_premul_bgra(&rgba, src_w, src_h, buf, dim);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((
                dim_i32, dim_i32,
            )))])
        });
    }
    let clamped_scale = crate::config::clamp_display_scale(output_scale);
    let logical_dim = ((f64::from(dim_i32)) / clamped_scale).round().max(12.0);
    let logical_dim_i32 = logical_dim as i32;
    Some(CachedCursor {
        buffer: mem_buf,
        logical_size: Size::from((logical_dim_i32, logical_dim_i32)),
        raw_size: Size::from((f64::from(dim_i32), f64::from(dim_i32))),
        hotspot: Point::from((hot_x * logical_dim, hot_y * logical_dim)),
    })
}

/// Area-averaged (box-filtered) downsampler / bilinear upsampler from straight RGBA to premultiplied BGRA.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn resample_rgba_to_premul_bgra(
    src_rgba: &[u8],
    src_w: usize,
    src_h: usize,
    dst_bgra: &mut [u8],
    dst_dim: usize,
) {
    for dy in 0..dst_dim {
        let sy0 = (dy * src_h) / dst_dim;
        let sy1 = (((dy + 1) * src_h) / dst_dim).max(sy0 + 1).min(src_h);
        for dx in 0..dst_dim {
            let sx0 = (dx * src_w) / dst_dim;
            let sx1 = (((dx + 1) * src_w) / dst_dim).max(sx0 + 1).min(src_w);
            let mut sum_r = 0_u64;
            let mut sum_g = 0_u64;
            let mut sum_b = 0_u64;
            let mut sum_a = 0_u64;
            let mut count = 0_u64;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let off = (sy * src_w + sx) * 4;
                    let sa = u64::from(src_rgba[off + 3]);
                    sum_r += u64::from(src_rgba[off]) * sa;
                    sum_g += u64::from(src_rgba[off + 1]) * sa;
                    sum_b += u64::from(src_rgba[off + 2]) * sa;
                    sum_a += sa;
                    count += 1;
                }
            }
            let dst_off = (dy * dst_dim + dx) * 4;
            if dst_off + 3 >= dst_bgra.len() {
                continue;
            }
            if count == 0 || sum_a == 0 {
                dst_bgra[dst_off..dst_off + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let denom = count * 255;
            dst_bgra[dst_off] = ((sum_b + denom / 2) / denom).min(255) as u8;
            dst_bgra[dst_off + 1] = ((sum_g + denom / 2) / denom).min(255) as u8;
            dst_bgra[dst_off + 2] = ((sum_r + denom / 2) / denom).min(255) as u8;
            dst_bgra[dst_off + 3] = (sum_a / count).min(255) as u8;
        }
    }
}

const XCURSOR_IMAGE_TYPE: u32 = 0xfffd_0002;
const XCURSOR_THEME_SIZES: &[u32] = &[24, 32, 48];

/// Canonical `XCursor` file names and standard X11/Wayland aliases for the 14 built-in RustyOS cursors.
const XCURSOR_EXPORT_SPECS: &[(&str, CursorIcon, &[&str])] = &[
    (
        "default",
        CursorIcon::Default,
        &["left_ptr", "arrow", "top_left_arrow", "right_ptr"],
    ),
    (
        "pointer",
        CursorIcon::Pointer,
        &["hand", "hand1", "hand2", "pointing_hand", "link"],
    ),
    (
        "text",
        CursorIcon::Text,
        &["xterm", "ibeam", "vertical-text"],
    ),
    (
        "crosshair",
        CursorIcon::Crosshair,
        &["cross", "tcross", "cell"],
    ),
    (
        "move",
        CursorIcon::Move,
        &["fleur", "all-scroll", "size_all"],
    ),
    ("grab", CursorIcon::Grab, &["openhand"]),
    (
        "grabbing",
        CursorIcon::Grabbing,
        &["closedhand", "dnd-move"],
    ),
    (
        "ew-resize",
        CursorIcon::EwResize,
        &[
            "sb_h_double_arrow",
            "h_double_arrow",
            "size_hor",
            "col-resize",
            "e-resize",
            "w-resize",
        ],
    ),
    (
        "ns-resize",
        CursorIcon::NsResize,
        &[
            "sb_v_double_arrow",
            "v_double_arrow",
            "size_ver",
            "row-resize",
            "n-resize",
            "s-resize",
        ],
    ),
    (
        "nwse-resize",
        CursorIcon::NwseResize,
        &[
            "bd_double_arrow",
            "size_fdiag",
            "nw-resize",
            "se-resize",
            "top_left_corner",
            "bottom_right_corner",
        ],
    ),
    (
        "nesw-resize",
        CursorIcon::NeswResize,
        &[
            "fd_double_arrow",
            "size_bdiag",
            "ne-resize",
            "sw-resize",
            "top_right_corner",
            "bottom_left_corner",
        ],
    ),
    (
        "not-allowed",
        CursorIcon::NotAllowed,
        &["crossed_circle", "no-drop", "forbidden"],
    ),
    ("wait", CursorIcon::Wait, &["watch"]),
    (
        "progress",
        CursorIcon::Progress,
        &["left_ptr_watch", "half-busy"],
    ),
];

/// Encodes one built-in PNG cursor into a valid multi-size binary `Xcur` (`v1.0`) file.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names
)]
fn encode_xcursor_bytes(
    png_bytes: &[u8],
    hot_x_norm: f64,
    hot_y_norm: f64,
    sizes: &[u32],
) -> Option<Vec<u8>> {
    let (src_w, src_h, rgba) = rusty_services::image_decode::decode_png_rgba(png_bytes)?;
    if src_w == 0 || src_h == 0 || sizes.is_empty() {
        return None;
    }
    let ntoc = u32::try_from(sizes.len()).ok()?;
    let header_and_toc = 16_usize + sizes.len() * 12;
    let mut chunks: Vec<(u32, Vec<u8>)> = Vec::with_capacity(sizes.len());
    for &dim_u32 in sizes {
        let dim = usize::try_from(dim_u32.clamp(16, 128)).ok()?;
        let mut pixels = vec![0_u8; dim * dim * 4];
        resample_rgba_to_premul_bgra(&rgba, src_w, src_h, &mut pixels, dim);
        let xhot = (hot_x_norm * f64::from(dim_u32))
            .round()
            .clamp(0.0, f64::from(dim_u32.saturating_sub(1))) as u32;
        let yhot = (hot_y_norm * f64::from(dim_u32))
            .round()
            .clamp(0.0, f64::from(dim_u32.saturating_sub(1))) as u32;
        let mut chunk = Vec::with_capacity(36 + pixels.len());
        chunk.extend_from_slice(&36_u32.to_le_bytes());
        chunk.extend_from_slice(&XCURSOR_IMAGE_TYPE.to_le_bytes());
        chunk.extend_from_slice(&dim_u32.to_le_bytes());
        chunk.extend_from_slice(&1_u32.to_le_bytes());
        chunk.extend_from_slice(&dim_u32.to_le_bytes());
        chunk.extend_from_slice(&dim_u32.to_le_bytes());
        chunk.extend_from_slice(&xhot.to_le_bytes());
        chunk.extend_from_slice(&yhot.to_le_bytes());
        chunk.extend_from_slice(&0_u32.to_le_bytes());
        chunk.extend_from_slice(&pixels);
        chunks.push((dim_u32, chunk));
    }

    let total_size: usize = header_and_toc + chunks.iter().map(|(_, c)| c.len()).sum::<usize>();
    let mut out = Vec::with_capacity(total_size);
    out.extend_from_slice(b"Xcur");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    out.extend_from_slice(&ntoc.to_le_bytes());

    let mut offset = header_and_toc;
    for (dim_u32, chunk) in &chunks {
        let pos = u32::try_from(offset).ok()?;
        out.extend_from_slice(&XCURSOR_IMAGE_TYPE.to_le_bytes());
        out.extend_from_slice(&dim_u32.to_le_bytes());
        out.extend_from_slice(&pos.to_le_bytes());
        offset += chunk.len();
    }
    for (_, chunk) in chunks {
        out.extend_from_slice(&chunk);
    }
    Some(out)
}

/// Materializes the built-in `"RustyOS"` `XCursor` theme under `icons_root/RustyOS/`
/// so third-party clients (Firefox, Chromium, GTK, Qt, XWayland) load the exact same
/// RustyOS cursor set as the compositor.
pub(crate) fn install_rustyos_xcursor_theme_to(
    icons_root: &std::path::Path,
) -> std::io::Result<()> {
    let theme_dir = icons_root.join("RustyOS");
    let cursors_dir = theme_dir.join("cursors");
    if cursors_dir.join("default").is_file() && cursors_dir.join("pointer").is_file() {
        return Ok(());
    }
    std::fs::create_dir_all(&cursors_dir)?;
    let index_path = theme_dir.join("index.theme");
    if !index_path.exists() {
        std::fs::write(
            &index_path,
            "[Icon Theme]\nName=RustyOS\nComment=RustyOS Built-in HiDPI Cursor Theme\n",
        )?;
    }
    for &(canonical, icon, aliases) in XCURSOR_EXPORT_SPECS {
        let (png_bytes, hx, hy) = builtin_cursor_spec(icon);
        if let Some(xcur_bytes) = encode_xcursor_bytes(png_bytes, hx, hy, XCURSOR_THEME_SIZES) {
            let target = cursors_dir.join(canonical);
            std::fs::write(&target, xcur_bytes)?;
            for &alias in aliases {
                let link_path = cursors_dir.join(alias);
                if !link_path.exists() {
                    let _ = std::os::unix::fs::symlink(canonical, &link_path);
                }
            }
        }
    }
    Ok(())
}

/// Idempotently installs the `"RustyOS"` `XCursor` theme into `~/.local/share/icons/RustyOS`
/// and sets `~/.icons/default/index.theme` (`Inherits=RustyOS`) if unset.
pub fn ensure_user_xcursor_theme_installed() {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return;
    };
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map_or_else(|| home.join(".local/share"), std::path::PathBuf::from);
    let _ = install_rustyos_xcursor_theme_to(&data_home.join("icons"));

    let default_icons_dir = home.join(".icons/default");
    let default_index = default_icons_dir.join("index.theme");
    if !default_index.exists() && std::fs::create_dir_all(&default_icons_dir).is_ok() {
        let _ = std::fs::write(&default_index, "[Icon Theme]\nInherits=RustyOS\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtin_rustyos_cursors_decode_and_scale_with_valid_hotspots() {
        let icons = [
            CursorIcon::Default,
            CursorIcon::Pointer,
            CursorIcon::Text,
            CursorIcon::Crosshair,
            CursorIcon::Move,
            CursorIcon::Grab,
            CursorIcon::Grabbing,
            CursorIcon::EwResize,
            CursorIcon::NsResize,
            CursorIcon::NwseResize,
            CursorIcon::NeswResize,
            CursorIcon::NotAllowed,
            CursorIcon::Wait,
            CursorIcon::Progress,
        ];
        for icon in icons {
            let (bytes, hx, hy) = builtin_cursor_spec(icon);
            assert!((0.0..=1.0).contains(&hx));
            assert!((0.0..=1.0).contains(&hy));
            let decoded = rusty_services::image_decode::decode_png_rgba(bytes);
            assert!(decoded.is_some(), "cursor {icon:?} failed to decode");
            if let Some((w, h, rgba)) = decoded {
                assert_eq!(w, 64);
                assert_eq!(h, 64);
                assert_eq!(rgba.len(), 64 * 64 * 4);
            }
            let cached = load_builtin_cursor(icon, 48, 2.0);
            assert!(cached.is_some());
            if let Some(c) = cached {
                assert_eq!(c.logical_size, Size::from((24, 24)));
                assert!(c.hotspot.x >= 0.0 && c.hotspot.x <= 24.0);
                assert!(c.hotspot.y >= 0.0 && c.hotspot.y <= 24.0);
            }
        }
        assert!(is_builtin_rustyos_theme(None));
        assert!(is_builtin_rustyos_theme(Some("RustyOS")));
        assert!(!is_builtin_rustyos_theme(Some("Adwaita")));
    }

    #[test]
    fn exports_valid_binary_xcursor_theme_files_and_aliases() -> std::io::Result<()> {
        let root =
            std::env::temp_dir().join(format!("rustyos-xcursor-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        install_rustyos_xcursor_theme_to(&root)?;
        let default_bytes = std::fs::read(root.join("RustyOS/cursors/default"))?;
        assert!(default_bytes.starts_with(b"Xcur"));
        assert!(root.join("RustyOS/cursors/left_ptr").exists());
        assert!(root.join("RustyOS/cursors/hand2").exists());
        assert!(root.join("RustyOS/index.theme").is_file());
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
