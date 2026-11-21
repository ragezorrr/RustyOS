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
}
