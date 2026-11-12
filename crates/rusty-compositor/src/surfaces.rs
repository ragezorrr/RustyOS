//! GPU-cached SDF shape primitives (`UiShapeCache`) for macOS-style rounded corners,
//! anti-aliased circles, and soft drop shadows (`RUSTYOS-022`).

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::input::pointer::CursorIcon;
use smithay::utils::{Buffer as BufferCoord, Logical, Physical, Point, Rectangle, Size, Transform};

/// Corner quadrant identifier for 9-slice rounded boxes and shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CornerQuadrant {
    /// Upper-left corner.
    TopLeft,
    /// Upper-right corner.
    TopRight,
    /// Lower-left corner.
    BottomLeft,
    /// Lower-right corner.
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CircleKey {
    diameter: u16,
    fill_rgba: [u8; 4],
    rim_rgba: Option<[u8; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CornerKey {
    radius: u16,
    quadrant: CornerQuadrant,
    fill_rgba: [u8; 4],
    border_rgba: Option<[u8; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ShadowCornerKey {
    inner_radius: u16,
    spread: u16,
    quadrant: CornerQuadrant,
    shadow_rgb: [u8; 3],
    max_alpha: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DockGlassKey {
    width: u16,
    height: u16,
    radius: u16,
    glass_pct: u8,
    reduce_transparency: bool,
}

#[derive(Debug)]
struct CachedCursor {
    buffer: MemoryRenderBuffer,
    logical_size: Size<i32, Logical>,
    raw_size: Size<f64, Logical>,
    hotspot: Point<f64, Logical>,
}

/// Procedural anti-aliased UI icon glyphs for `TopBar` and `Spotlight` (`RUSTYOS-048`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiGlyphKind {
    /// Faceted RustyOS diamond/prism emblem replacing missing fallback glyphs.
    RustyLogo,
    /// Smooth concentric Wi-Fi arcs + anchor dot.
    Wifi { connected: bool },
    /// Speaker cone + wave arcs or mute slash.
    Volume { level_percent: u8, muted: bool },
    /// Rounded pill battery outline + level fill + optional charging bolt.
    Battery { percent: u8, charging: bool },
    /// Magnifying glass search loupe (`TopBar` / `Spotlight`).
    Search,
    /// Dual macOS Control Center toggle switches.
    ControlCenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    kind: UiGlyphKind,
    size_px: u16,
    tint_rgba: [u8; 4],
}

#[derive(Debug, Default)]
struct CachedWallpaper {
    custom_key: Option<(PathBuf, Size<i32, Logical>)>,
    custom_buffer: Option<MemoryRenderBuffer>,
    fallback_buffer: Option<MemoryRenderBuffer>,
}

/// CPU-rasterized, GPU-cached cache of SDF rounded corners, circles, shadow patches, cursors, glyphs, and wallpaper.
#[derive(Debug, Default)]
pub struct UiShapeCache {
    circles: HashMap<CircleKey, (MemoryRenderBuffer, Size<i32, Logical>)>,
    corners: HashMap<CornerKey, (MemoryRenderBuffer, Size<i32, Logical>)>,
    shadow_corners: HashMap<ShadowCornerKey, (MemoryRenderBuffer, Size<i32, Logical>)>,
    glyphs: HashMap<GlyphKey, MemoryRenderBuffer>,
    dock_glass_panels: HashMap<DockGlassKey, MemoryRenderBuffer>,
    dock_icon_shadows: HashMap<u16, MemoryRenderBuffer>,
    wallpaper: CachedWallpaper,
    cursor_arrow: Option<MemoryRenderBuffer>,
    xcursors: HashMap<(Option<String>, &'static str, u32), Option<CachedCursor>>,
    /// Decoded & GPU-cached application and tray icon textures (`RUSTYOS-033`).
    pub icons: crate::icons::IconCache,
}

impl UiShapeCache {
    /// Creates an empty [`UiShapeCache`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a cached anti-aliased circular render element positioned at `loc`.
    pub fn circle_element(
        &mut self,
        renderer: &mut GlesRenderer,
        diameter: i32,
        fill_rgba: [u8; 4],
        rim_rgba: Option<[u8; 4]>,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if diameter <= 0 {
            return None;
        }
        let d_u16 = u16::try_from(diameter).ok()?;
        let key = CircleKey {
            diameter: d_u16,
            fill_rgba,
            rim_rgba,
        };
        let (buf, _size) = self.circles.entry(key).or_insert_with(|| {
            let buf = rasterize_circle_buffer(diameter, fill_rgba, rim_rgba);
            (buf, Size::from((diameter, diameter)))
        });
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a cached `radius x radius` rounded corner patch with optional 1px outer rim.
    pub fn corner_element(
        &mut self,
        renderer: &mut GlesRenderer,
        radius: i32,
        quadrant: CornerQuadrant,
        fill_rgba: [u8; 4],
        border_rgba: Option<[u8; 4]>,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if radius <= 0 {
            return None;
        }
        let r_u16 = u16::try_from(radius).ok()?;
        let key = CornerKey {
            radius: r_u16,
            quadrant,
            fill_rgba,
            border_rgba,
        };
        let (buf, _size) = self.corners.entry(key).or_insert_with(|| {
            let buf = rasterize_corner_buffer(radius, quadrant, fill_rgba, border_rgba);
            (buf, Size::from((radius, radius)))
        });
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a cached `(inner_radius + spread) x (inner_radius + spread)` soft radial shadow corner.
    ///
    /// Pixels strictly inside `inner_radius` are kept transparent (`alpha = 0`) so shadows never
    /// bleed underneath translucent glass interiors.
    #[allow(clippy::too_many_arguments)]
    pub fn shadow_corner_element(
        &mut self,
        renderer: &mut GlesRenderer,
        inner_radius: i32,
        spread: i32,
        quadrant: CornerQuadrant,
        shadow_rgb: [u8; 3],
        max_alpha: u8,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if spread <= 0 || inner_radius < 0 {
            return None;
        }
        let dim = inner_radius + spread;
        let r_u16 = u16::try_from(inner_radius).ok()?;
        let s_u16 = u16::try_from(spread).ok()?;
        let key = ShadowCornerKey {
            inner_radius: r_u16,
            spread: s_u16,
            quadrant,
            shadow_rgb,
            max_alpha,
        };
        let (buf, _size) = self.shadow_corners.entry(key).or_insert_with(|| {
            let buf = rasterize_shadow_corner_buffer(
                inner_radius,
                spread,
                quadrant,
                shadow_rgb,
                max_alpha,
            );
            (buf, Size::from((dim, dim)))
        });
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a GPU-cached macOS-style translucent top-lit frosted glass Dock container (`RUSTYOS-050`).
    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn dock_glass_panel_element(
        &mut self,
        renderer: &mut GlesRenderer,
        width: i32,
        height: i32,
        radius: i32,
        glass_opacity: f32,
        reduce_transparency: bool,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if width <= 0 || height <= 0 {
            return None;
        }
        let w_u16 = u16::try_from(width).ok()?;
        let h_u16 = u16::try_from(height).ok()?;
        let r_u16 = u16::try_from(radius.clamp(0, width.min(height) / 2)).ok()?;
        let glass_pct = (crate::config::clamp_glass_opacity(glass_opacity) * 100.0).round() as u8;
        let key = DockGlassKey {
            width: w_u16,
            height: h_u16,
            radius: r_u16,
            glass_pct,
            reduce_transparency,
        };
        if !self.dock_glass_panels.contains_key(&key) {
            if self.dock_glass_panels.len() >= 16 {
                self.dock_glass_panels.clear();
            }
            let clamped_glass = f32::from(glass_pct) / 100.0;
            let buf = rasterize_rgba_rect(width, height, |px, py| {
                sample_dock_glass_pixel(
                    px,
                    py,
                    width,
                    height,
                    i32::from(r_u16),
                    clamped_glass,
                    reduce_transparency,
                )
            });
            self.dock_glass_panels.insert(key, buf);
        }
        let buf = self.dock_glass_panels.get(&key)?;
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a GPU-cached soft ambient drop shadow behind a Dock app icon (`RUSTYOS-050`).
    pub fn dock_icon_shadow_element(
        &mut self,
        renderer: &mut GlesRenderer,
        icon_size: i32,
        icon_loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if icon_size <= 0 {
            return None;
        }
        let key = u16::try_from(icon_size).ok()?;
        let pad = 7;
        let total = icon_size + pad * 2;
        let buf = self.dock_icon_shadows.entry(key).or_insert_with(|| {
            rasterize_rgba_rect(total, total, |px, py| {
                sample_dock_icon_shadow_pixel(px, py, icon_size, pad)
            })
        });
        let shadow_loc = Point::from((icon_loc.x - pad, icon_loc.y - pad + 3));
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            shadow_loc.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a GPU-cached wallpaper scaled cleanly to `screen_size`, preferring `custom_path` (`.png`)
    /// from `[ui] wallpaper` and falling back to the ambient Purple Glass gradient (`RUSTYOS-034`).
    pub fn wallpaper_element(
        &mut self,
        renderer: &mut GlesRenderer,
        screen_size: Size<i32, Logical>,
        custom_path: Option<&Path>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        const WALLPAPER_W: i32 = 640;
        const WALLPAPER_H: i32 = 360;
        if screen_size.w <= 0 || screen_size.h <= 0 {
            return None;
        }

        if let Some(path) = custom_path {
            let desired_key = (path.to_path_buf(), screen_size);
            if self.wallpaper.custom_key.as_ref() != Some(&desired_key) {
                let loaded = usize::try_from(screen_size.w)
                    .ok()
                    .zip(usize::try_from(screen_size.h).ok())
                    .and_then(|(w, h)| crate::icons::load_png_wallpaper_bgra(path, w, h))
                    .map(|bgra| {
                        MemoryRenderBuffer::from_slice(
                            &bgra,
                            Fourcc::Argb8888,
                            (screen_size.w, screen_size.h),
                            1,
                            Transform::Normal,
                            None,
                        )
                    });
                self.wallpaper.custom_buffer = loaded;
                self.wallpaper.custom_key = Some(desired_key);
            }
            if let Some(buf) = self.wallpaper.custom_buffer.as_ref() {
                return MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    Point::<f64, smithay::utils::Physical>::from((0.0, 0.0)),
                    buf,
                    None,
                    None,
                    None,
                    Kind::Unspecified,
                )
                .ok();
            }
        }

        let buf = self
            .wallpaper
            .fallback_buffer
            .get_or_insert_with(|| rasterize_wallpaper_buffer(WALLPAPER_W, WALLPAPER_H));
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, smithay::utils::Physical>::from((0.0, 0.0)),
            buf,
            None,
            Some(Rectangle::<f64, Logical>::from_size(Size::from((
                f64::from(WALLPAPER_W),
                f64::from(WALLPAPER_H),
            )))),
            Some(screen_size),
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a GPU-cached cursor element for `icon` positioned with its hotspot aligned to `pointer_loc`
    /// at scale `1.0`, using system/configured `XCursor` themes or the built-in crisp arrow fallback (`RUSTYOS-034`).
    pub fn named_cursor_element(
        &mut self,
        renderer: &mut GlesRenderer,
        icon: CursorIcon,
        theme_override: Option<&str>,
        pointer_loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        self.named_cursor_element_for_output(
            renderer,
            icon,
            theme_override,
            pointer_loc,
            1.0,
            None,
            Kind::Cursor,
        )
    }

    /// Returns a HiDPI-aware GPU-cached cursor element for `icon` scaled by `output_scale`,
    /// automatically falling back to `Kind::Unspecified` when `plane_limit` would be exceeded (`RUSTYOS-046`).
    #[allow(clippy::too_many_arguments)]
    pub fn named_cursor_element_for_output(
        &mut self,
        renderer: &mut GlesRenderer,
        icon: CursorIcon,
        theme_override: Option<&str>,
        pointer_loc: Point<i32, Logical>,
        output_scale: f64,
        plane_limit: Option<Size<u32, BufferCoord>>,
        kind: Kind,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let clamped_scale = crate::config::clamp_display_scale(output_scale);
        let target_px = xcursor_target_pixel_size(clamped_scale);
        let icon_name = icon.name();
        let cache_key = (theme_override.map(str::to_owned), icon_name, target_px);
        if !self.xcursors.contains_key(&cache_key) {
            let loaded = load_xcursor_for_icon(icon, theme_override, target_px, clamped_scale);
            self.xcursors.insert(cache_key.clone(), loaded);
        }
        if let Some(Some(cursor)) = self.xcursors.get(&cache_key) {
            let resolved_kind = resolve_cursor_element_kind(
                kind == Kind::Cursor,
                cursor.logical_size,
                clamped_scale,
                plane_limit,
            );
            let origin = scaled_cursor_physical_origin(pointer_loc, cursor.hotspot, clamped_scale);
            return MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                origin,
                &cursor.buffer,
                None,
                Some(Rectangle::from_size(cursor.raw_size)),
                Some(cursor.logical_size),
                resolved_kind,
            )
            .ok();
        }
        self.cursor_arrow_element_for_output(
            renderer,
            pointer_loc,
            clamped_scale,
            plane_limit,
            kind,
        )
    }

    /// Returns a GPU-cached anti-aliased desktop pointer arrow (24x24, hotspot at `(2, 2)`).
    pub fn cursor_arrow_element(
        &mut self,
        renderer: &mut GlesRenderer,
        top_left: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let buf = self
            .cursor_arrow
            .get_or_insert_with(rasterize_cursor_arrow_buffer);
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            top_left.to_f64().to_physical(1.0),
            buf,
            None,
            None,
            None,
            Kind::Cursor,
        )
        .ok()
    }

    fn cursor_arrow_element_for_output(
        &mut self,
        renderer: &mut GlesRenderer,
        pointer_loc: Point<i32, Logical>,
        output_scale: f64,
        plane_limit: Option<Size<u32, BufferCoord>>,
        kind: Kind,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let logical_size = Size::from((24, 24));
        let resolved_kind = resolve_cursor_element_kind(
            kind == Kind::Cursor,
            logical_size,
            output_scale,
            plane_limit,
        );
        let origin =
            scaled_cursor_physical_origin(pointer_loc, Point::from((2.0, 2.0)), output_scale);
        let buf = self
            .cursor_arrow
            .get_or_insert_with(rasterize_cursor_arrow_buffer);
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            origin,
            buf,
            None,
            None,
            Some(logical_size),
            resolved_kind,
        )
        .ok()
    }

    /// Returns a GPU-cached 4x4-supersampled SDF UI icon glyph for `TopBar` or `Spotlight` (`RUSTYOS-048`/`049`).
    pub fn ui_glyph_element(
        &mut self,
        renderer: &mut GlesRenderer,
        kind: UiGlyphKind,
        size_px: i32,
        tint_rgba: [u8; 4],
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let size_u16 = u16::try_from(size_px).ok()?;
        if size_px <= 0 {
            return None;
        }
        let key = GlyphKey {
            kind,
            size_px: size_u16,
            tint_rgba,
        };
        let buf = self
            .glyphs
            .entry(key)
            .or_insert_with(|| rasterize_ui_glyph_buffer(kind, size_px, tint_rgba));
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            Some(Rectangle::<f64, Logical>::from_size(Size::from((
                f64::from(size_px),
                f64::from(size_px),
            )))),
            Some(Size::from((size_px, size_px))),
            Kind::Unspecified,
        )
        .ok()
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn premul_pixel(r: f32, g: f32, b: f32, a: f32) -> [u8; 4] {
    let ac = a.clamp(0.0, 1.0);
    let pb = (b.clamp(0.0, 255.0) * ac).round() as u8;
    let pg = (g.clamp(0.0, 255.0) * ac).round() as u8;
    let pr = (r.clamp(0.0, 255.0) * ac).round() as u8;
    let pa = (ac * 255.0).round() as u8;
    [pb, pg, pr, pa]
}

fn composite_over(fg: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
    let fg_a = f32::from(fg[3]) / 255.0;
    let bg_a = f32::from(bg[3]) / 255.0;
    let out_a = fg_a + bg_a * (1.0 - fg_a);
    let inv = 1.0 - fg_a;
    let b = (f32::from(fg[0]) + f32::from(bg[0]) * inv).clamp(0.0, 255.0);
    let g = (f32::from(fg[1]) + f32::from(bg[1]) * inv).clamp(0.0, 255.0);
    let r = (f32::from(fg[2]) + f32::from(bg[2]) * inv).clamp(0.0, 255.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    [
        b.round() as u8,
        g.round() as u8,
        r.round() as u8,
        (out_a.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_circle_pixel(
    px: i32,
    py: i32,
    diameter: i32,
    fill_rgba: [u8; 4],
    rim_rgba: Option<[u8; 4]>,
) -> [u8; 4] {
    let half = diameter as f32 * 0.5;
    let dx = (px as f32 + 0.5) - half;
    let dy = (py as f32 + 0.5) - half;
    let dist = dx.hypot(dy);
    let outer_r = half - 0.5;
    let cov = (0.5 - (dist - outer_r)).clamp(0.0, 1.0);
    if cov <= 0.0 {
        return [0, 0, 0, 0];
    }

    let fill_a = (f32::from(fill_rgba[3]) / 255.0) * cov;
    let fill_px = premul_pixel(
        f32::from(fill_rgba[0]),
        f32::from(fill_rgba[1]),
        f32::from(fill_rgba[2]),
        fill_a,
    );

    if let Some(rim) = rim_rgba {
        let rim_t = (dist - (outer_r - 1.25)).clamp(0.0, 1.0) * cov;
        if rim_t > 0.0 {
            let rim_a = (f32::from(rim[3]) / 255.0) * rim_t;
            let rim_px = premul_pixel(
                f32::from(rim[0]),
                f32::from(rim[1]),
                f32::from(rim[2]),
                rim_a,
            );
            return composite_over(rim_px, fill_px);
        }
    }

    fill_px
}

fn rasterize_circle_buffer(
    diameter: i32,
    fill_rgba: [u8; 4],
    rim_rgba: Option<[u8; 4]>,
) -> MemoryRenderBuffer {
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (diameter, diameter),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let dim_u = usize::try_from(diameter).unwrap_or(0);
            for py in 0..diameter {
                for px in 0..diameter {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * dim_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample_circle_pixel(px, py, diameter, fill_rgba, rim_rgba);
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((diameter, diameter)),
            )])
        });
    }
    mem_buf
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_corner_pixel(
    px: i32,
    py: i32,
    radius: i32,
    quadrant: CornerQuadrant,
    fill_rgba: [u8; 4],
    border_rgba: Option<[u8; 4]>,
) -> [u8; 4] {
    let r_f = radius as f32;
    let lx = match quadrant {
        CornerQuadrant::TopLeft | CornerQuadrant::BottomLeft => r_f - (px as f32 + 0.5),
        CornerQuadrant::TopRight | CornerQuadrant::BottomRight => px as f32 + 0.5,
    };
    let ly = match quadrant {
        CornerQuadrant::TopLeft | CornerQuadrant::TopRight => r_f - (py as f32 + 0.5),
        CornerQuadrant::BottomLeft | CornerQuadrant::BottomRight => py as f32 + 0.5,
    };
    let dist = lx.hypot(ly);
    let d_out = dist - (r_f - 0.5);
    let outer_cov = (0.5 - d_out).clamp(0.0, 1.0);
    if outer_cov <= 0.0 {
        return [0, 0, 0, 0];
    }

    let fill_a = (f32::from(fill_rgba[3]) / 255.0) * outer_cov;
    let fill_px = premul_pixel(
        f32::from(fill_rgba[0]),
        f32::from(fill_rgba[1]),
        f32::from(fill_rgba[2]),
        fill_a,
    );

    if let Some(border) = border_rgba {
        // 1px outer ring along the rounded arc matching straight 1px edge border strips
        let ring_cov = (d_out + 1.15).clamp(0.0, 1.0) * outer_cov;
        if ring_cov > 0.0 {
            let b_a = (f32::from(border[3]) / 255.0) * ring_cov;
            let b_px = premul_pixel(
                f32::from(border[0]),
                f32::from(border[1]),
                f32::from(border[2]),
                b_a,
            );
            return composite_over(b_px, fill_px);
        }
    }

    fill_px
}

fn rasterize_corner_buffer(
    radius: i32,
    quadrant: CornerQuadrant,
    fill_rgba: [u8; 4],
    border_rgba: Option<[u8; 4]>,
) -> MemoryRenderBuffer {
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (radius, radius),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let r_u = usize::try_from(radius).unwrap_or(0);
            for py in 0..radius {
                for px in 0..radius {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * r_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p =
                            sample_corner_pixel(px, py, radius, quadrant, fill_rgba, border_rgba);
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((radius, radius)),
            )])
        });
    }
    mem_buf
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_shadow_corner_pixel(
    px: i32,
    py: i32,
    inner_radius: i32,
    spread: i32,
    quadrant: CornerQuadrant,
    shadow_rgb: [u8; 3],
    max_alpha: u8,
) -> [u8; 4] {
    let dim = (inner_radius + spread) as f32;
    let r_f = inner_radius as f32;
    let s_f = spread as f32;

    let lx = match quadrant {
        CornerQuadrant::TopLeft | CornerQuadrant::BottomLeft => dim - (px as f32 + 0.5),
        CornerQuadrant::TopRight | CornerQuadrant::BottomRight => px as f32 + 0.5,
    };
    let ly = match quadrant {
        CornerQuadrant::TopLeft | CornerQuadrant::TopRight => dim - (py as f32 + 0.5),
        CornerQuadrant::BottomLeft | CornerQuadrant::BottomRight => py as f32 + 0.5,
    };
    let dist = lx.hypot(ly);
    if dist < r_f {
        return [0, 0, 0, 0];
    }
    let t = ((dist - r_f) / s_f).clamp(0.0, 1.0);
    let falloff = (1.0 - t) * (1.0 - t);
    let alpha = (f32::from(max_alpha) / 255.0) * falloff;
    premul_pixel(
        f32::from(shadow_rgb[0]),
        f32::from(shadow_rgb[1]),
        f32::from(shadow_rgb[2]),
        alpha,
    )
}

fn rasterize_shadow_corner_buffer(
    inner_radius: i32,
    spread: i32,
    quadrant: CornerQuadrant,
    shadow_rgb: [u8; 3],
    max_alpha: u8,
) -> MemoryRenderBuffer {
    let dim = inner_radius + spread;
    let mut mem_buf =
        MemoryRenderBuffer::new(Fourcc::Argb8888, (dim, dim), 1, Transform::Normal, None);
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let dim_u = usize::try_from(dim).unwrap_or(0);
            for py in 0..dim {
                for px in 0..dim {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * dim_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample_shadow_corner_pixel(
                            px,
                            py,
                            inner_radius,
                            spread,
                            quadrant,
                            shadow_rgb,
                            max_alpha,
                        );
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((dim, dim)),
            )])
        });
    }
    mem_buf
}

#[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn sample_wallpaper_pixel(px: i32, py: i32, width: i32, height: i32) -> [u8; 4] {
    const BAYER_4X4: [[f32; 4]; 4] = [
        [0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0],
        [12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0],
        [3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0],
        [15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0],
    ];

    let norm_x = (px as f32 + 0.5) / width.max(1) as f32;
    let norm_y = (py as f32 + 0.5) / height.max(1) as f32;

    // Deep Purple Glass velvety base (#0E0918 -> #1A102B)
    let mut red = 14.0 + 11.0 * norm_y + 4.0 * (1.0 - norm_x);
    let mut green = 9.0 + 8.0 * norm_y;
    let mut blue = 24.0 + 18.0 * norm_y + 6.0 * (1.0 - norm_x);

    // Primary upper-left violet aurora bloom
    let dist1 = (norm_x - 0.23).hypot((norm_y - 0.22) * 0.88);
    let glow1 = (1.0 - (dist1 / 0.56)).clamp(0.0, 1.0);
    let amp1 = glow1 * glow1 * (3.0 - 2.0 * glow1) * 0.27;
    red += 168.0 * amp1;
    green += 110.0 * amp1;
    blue += 255.0 * amp1;

    // Secondary bottom-right orchid-amethyst sweep
    let dist2 = (norm_x - 0.78).hypot((norm_y - 0.77) * 0.90);
    let glow2 = (1.0 - (dist2 / 0.64)).clamp(0.0, 1.0);
    let amp2 = glow2 * glow2 * (3.0 - 2.0 * glow2) * 0.29;
    red += 108.0 * amp2;
    green += 56.0 * amp2;
    blue += 188.0 * amp2;

    // Subtle macOS-style layered diagonal silk ribbon across center-lower field
    let ribbon_center = 0.62 - 0.24 * norm_x + 0.09 * (norm_x * 4.2 + 0.4).sin();
    let ribbon_dist = ((norm_y - ribbon_center).abs() / 0.22).clamp(0.0, 1.0);
    let ribbon = (1.0 - ribbon_dist * ribbon_dist) * (1.0 - ribbon_dist);
    let ribbon_amp = ribbon * (0.14 + 0.08 * (norm_x * 2.2).cos());
    red += 155.0 * ribbon_amp;
    green += 82.0 * ribbon_amp;
    blue += 235.0 * ribbon_amp;

    // Soft rose-lilac crest rim near the top of the silk fold
    let crest_dist = ((norm_y - (ribbon_center - 0.035)).abs() / 0.055).clamp(0.0, 1.0);
    let crest = (1.0 - crest_dist) * (1.0 - crest_dist) * 0.075 * (1.0 - (norm_x - 0.48).abs());
    red += 210.0 * crest;
    green += 130.0 * crest;
    blue += 245.0 * crest;

    // Ordered 4x4 Bayer dithering to eliminate 8-bit dark gradient banding
    let bx = usize::try_from(px.rem_euclid(4)).unwrap_or(0);
    let by = usize::try_from(py.rem_euclid(4)).unwrap_or(0);
    let dither = (BAYER_4X4[by][bx] - 0.5) * 1.35;
    premul_pixel(red + dither, green + dither, blue + dither, 1.0)
}

fn rasterize_wallpaper_buffer(w: i32, h: i32) -> MemoryRenderBuffer {
    let mut mem_buf = MemoryRenderBuffer::new(Fourcc::Argb8888, (w, h), 1, Transform::Normal, None);
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let w_u = usize::try_from(w).unwrap_or(0);
            for py in 0..h {
                for px in 0..w {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * w_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample_wallpaper_pixel(px, py, w, h);
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((w, h)),
            )])
        });
    }
    mem_buf
}

fn sd_circle(u: f32, v: f32, cx: f32, cy: f32, r: f32) -> f32 {
    (u - cx).hypot(v - cy) - r
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

fn sd_cov(sd: f32, edge_w: f32) -> f32 {
    (0.5 - sd / edge_w.max(0.01)).clamp(0.0, 1.0)
}

fn blend_premul_over(bg: [f32; 4], fg_rgb: [f32; 3], fg_a: f32) -> [f32; 4] {
    let a = fg_a.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    [
        fg_rgb[0] * a + bg[0] * inv,
        fg_rgb[1] * a + bg[1] * inv,
        fg_rgb[2] * a + bg[2] * inv,
        a + bg[3] * inv,
    ]
}

fn premul_tint(tint: [f32; 4], cov: f32) -> [f32; 4] {
    let a = (tint[3] * cov).clamp(0.0, 1.0);
    [tint[0] * a, tint[1] * a, tint[2] * a, a]
}

/// Signed distance to an upward-opening circular arc with true rounded endcaps (`Inigo Quilez sdArc`).
fn sd_arc_up(
    u: f32,
    v: f32,
    cx: f32,
    cy: f32,
    radius: f32,
    half_angle_rad: f32,
    stroke: f32,
) -> f32 {
    let px = (u - cx).abs();
    let py = cy - v;
    let (sin_a, cos_a) = half_angle_rad.sin_cos();
    let dist = if px * cos_a < py * sin_a {
        (px.hypot(py) - radius).abs()
    } else {
        (px - radius * sin_a).hypot(py - radius * cos_a)
    };
    dist - stroke
}

fn sample_rusty_logo_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    // Original sleek chamfered hexagonal crest for RustyOS, pure monochrome with transparent inner cutout.
    const CREST: [(f32, f32); 6] = [
        (0.50, 0.14),
        (0.81, 0.31),
        (0.81, 0.67),
        (0.50, 0.86),
        (0.19, 0.67),
        (0.19, 0.31),
    ];
    let outer = signed_distance_polygon(u, v, &CREST) - 0.025;
    let inner_cut = sd_circle(u, v, 0.50, 0.50, 0.125);
    let sd = outer.max(-inner_cut);
    let cov = sd_cov(sd, edge);
    premul_tint(tint, cov)
}

fn sample_wifi_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4], connected: bool) -> [f32; 4] {
    let cx = 0.50;
    let cy = 0.77;
    let half_angle = 0.74_f32; // ~42.4 deg per side -> crisp macOS fan
    let stroke = 0.052;

    let mut sd = sd_circle(u, v, cx, cy, 0.072);
    for radius in [0.225_f32, 0.395, 0.565] {
        sd = sd.min(sd_arc_up(u, v, cx, cy, radius, half_angle, stroke));
    }

    let base_alpha = if connected { tint[3] } else { tint[3] * 0.42 };
    let cov = sd_cov(sd, edge);
    let base = premul_tint([tint[0], tint[1], tint[2], base_alpha], cov);
    if connected {
        return base;
    }
    let slash_cov = sd_cov(sd_segment(u, v, 0.20, 0.20, 0.80, 0.80, 0.044), edge);
    blend_premul_over(base, [tint[0], tint[1], tint[2]], tint[3] * slash_cov)
}

fn sample_volume_glyph_uv(
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

fn sample_battery_glyph_uv(
    u: f32,
    v: f32,
    edge: f32,
    tint: [f32; 4],
    percent: u8,
    charging: bool,
) -> [f32; 4] {
    let body_outer = sd_rounded_box(u, v, 0.45, 0.50, 0.35, 0.22, 0.10);
    let nub = sd_rounded_box(u, v, 0.85, 0.50, 0.034, 0.088, 0.025);
    let shell_ring = body_outer.abs() - 0.036;

    // Clean macOS monochrome pill: solid white when nearly full (>=92%), crisp translucent track + fill otherwise.
    let mut out = [0.0_f32; 4];
    let interior_cov = sd_cov(body_outer + 0.036, edge);
    if interior_cov > 0.0 {
        out = blend_premul_over(
            out,
            [tint[0], tint[1], tint[2]],
            tint[3] * interior_cov * 0.28,
        );
    }

    let frame_cov = sd_cov(shell_ring.min(nub), edge);
    if frame_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * frame_cov * 0.92);
    }

    let frac = if percent >= 92 {
        1.0_f32
    } else {
        f32::from(percent.clamp(8, 100)) / 100.0
    };
    let fill_max_hw = 0.27;
    let fill_hw = fill_max_hw * frac;
    let fill_cx = 0.18 + fill_hw;
    let fill_sd = sd_rounded_box(u, v, fill_cx, 0.50, fill_hw, 0.135, 0.048);
    let fill_cov = sd_cov(fill_sd, edge);
    if fill_cov > 0.0 {
        let fill_rgb = if !charging && percent <= 20 {
            [248.0_f32, 113.0, 113.0]
        } else {
            [tint[0], tint[1], tint[2]]
        };
        out = blend_premul_over(out, fill_rgb, tint[3] * fill_cov);
    }

    if charging {
        let b1 = sd_segment(u, v, 0.49, 0.33, 0.40, 0.51, 0.032);
        let b2 = sd_segment(u, v, 0.40, 0.51, 0.50, 0.51, 0.030);
        let b3 = sd_segment(u, v, 0.50, 0.51, 0.41, 0.67, 0.032);
        let bolt_cov = sd_cov(b1.min(b2).min(b3), edge);
        if bolt_cov > 0.0 {
            out = blend_premul_over(out, [18.0, 14.0, 28.0], tint[3] * bolt_cov * 0.88);
        }
    }
    out
}

fn sample_search_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let ring = sd_circle(u, v, 0.42, 0.42, 0.225).abs() - 0.056;
    let handle = sd_segment(u, v, 0.59, 0.59, 0.83, 0.83, 0.058);
    let cov = sd_cov(ring.min(handle), edge);
    premul_tint(tint, cov)
}

fn sample_control_center_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let top_capsule = sd_rounded_box(u, v, 0.50, 0.32, 0.34, 0.125, 0.125);
    let bot_capsule = sd_rounded_box(u, v, 0.50, 0.68, 0.34, 0.125, 0.125);
    let track_outline = (top_capsule.abs() - 0.034).min(bot_capsule.abs() - 0.034);
    let track_fill = top_capsule.min(bot_capsule);
    let knobs = sd_circle(u, v, 0.33, 0.32, 0.082).min(sd_circle(u, v, 0.67, 0.68, 0.082));

    let mut out = [0.0_f32; 4];
    let fill_cov = sd_cov(track_fill, edge);
    if fill_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * fill_cov * 0.26);
    }
    let rim_cov = sd_cov(track_outline, edge);
    if rim_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * rim_cov * 0.92);
    }
    let knob_cov = sd_cov(knobs, edge);
    if knob_cov > 0.0 {
        out = blend_premul_over(out, [tint[0], tint[1], tint[2]], tint[3] * knob_cov);
    }
    out
}

#[allow(clippy::cast_precision_loss)]
fn sample_ui_glyph_pixel(
    px: i32,
    py: i32,
    size_px: i32,
    kind: UiGlyphKind,
    tint_rgba: [u8; 4],
) -> [u8; 4] {
    const SUBSAMPLES: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
    let dim = size_px.max(1) as f32;
    let edge = 1.05 / dim;
    let tint = [
        f32::from(tint_rgba[0]),
        f32::from(tint_rgba[1]),
        f32::from(tint_rgba[2]),
        f32::from(tint_rgba[3]) / 255.0,
    ];
    let mut sum = [0.0_f32; 4];
    for sy in SUBSAMPLES {
        for sx in SUBSAMPLES {
            let u = (px as f32 + sx) / dim;
            let v = (py as f32 + sy) / dim;
            let s = match kind {
                UiGlyphKind::RustyLogo => sample_rusty_logo_uv(u, v, edge, tint),
                UiGlyphKind::Wifi { connected } => {
                    sample_wifi_glyph_uv(u, v, edge, tint, connected)
                }
                UiGlyphKind::Volume {
                    level_percent,
                    muted,
                } => sample_volume_glyph_uv(u, v, edge, tint, level_percent, muted),
                UiGlyphKind::Battery { percent, charging } => {
                    sample_battery_glyph_uv(u, v, edge, tint, percent, charging)
                }
                UiGlyphKind::Search => sample_search_glyph_uv(u, v, edge, tint),
                UiGlyphKind::ControlCenter => sample_control_center_glyph_uv(u, v, edge, tint),
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

fn rasterize_ui_glyph_buffer(
    kind: UiGlyphKind,
    size_px: i32,
    tint_rgba: [u8; 4],
) -> MemoryRenderBuffer {
    let mut mem_buf = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (size_px, size_px),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let dim_u = usize::try_from(size_px).unwrap_or(0);
            for py in 0..size_px {
                for px in 0..size_px {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * dim_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample_ui_glyph_pixel(px, py, size_px, kind, tint_rgba);
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((size_px, size_px)),
            )])
        });
    }
    mem_buf
}

fn signed_distance_polygon(px: f32, py: f32, verts: &[(f32, f32)]) -> f32 {
    let mut d = f32::MAX;
    let mut s = 1.0_f32;
    let len = verts.len();
    for i in 0..len {
        let j = if i == 0 { len - 1 } else { i - 1 };
        let vi = verts[i];
        let vj = verts[j];
        let ex = vi.0 - vj.0;
        let ey = vi.1 - vj.1;
        let wx = px - vj.0;
        let wy = py - vj.1;
        let denom = ex * ex + ey * ey;
        let t = if denom > 0.0 {
            ((wx * ex + wy * ey) / denom).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bx = wx - ex * t;
        let by = wy - ey * t;
        d = d.min(bx * bx + by * by);

        let c1 = py >= vj.1;
        let c2 = py < vi.1;
        let c3 = ex * wy > ey * wx;
        if (c1 && c2 && c3) || (!c1 && !c2 && !c3) {
            s = -s;
        }
    }
    s * d.sqrt()
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sample_cursor_arrow_pixel(px: i32, py: i32) -> [u8; 4] {
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

fn rasterize_cursor_arrow_buffer() -> MemoryRenderBuffer {
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

fn xcursor_target_size() -> u32 {
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

fn xcursor_search_roots() -> Vec<PathBuf> {
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

fn read_theme_inherits(theme_dir: &Path) -> Vec<String> {
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

fn xcursor_candidate_names(icon: CursorIcon) -> Vec<&'static str> {
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

fn load_xcursor_for_icon(
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

fn rasterize_rgba_rect(
    width: i32,
    height: i32,
    sample: impl Fn(i32, i32) -> [u8; 4],
) -> MemoryRenderBuffer {
    let w = width.max(1);
    let h = height.max(1);
    let mut mem_buf = MemoryRenderBuffer::new(Fourcc::Argb8888, (w, h), 1, Transform::Normal, None);
    {
        let mut ctx = mem_buf.render();
        let _ = ctx.draw(|pixels| {
            let w_u = usize::try_from(w).unwrap_or(0);
            for py in 0..h {
                for px in 0..w {
                    let row = usize::try_from(py).unwrap_or(0);
                    let col = usize::try_from(px).unwrap_or(0);
                    let idx = (row * w_u + col) * 4;
                    if idx + 4 <= pixels.len() {
                        let p = sample(px, py);
                        pixels[idx..idx + 4].copy_from_slice(&p);
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![smithay::utils::Rectangle::from_size(
                Size::from((w, h)),
            )])
        });
    }
    mem_buf
}

#[allow(clippy::cast_precision_loss)]
fn sample_dock_glass_pixel(
    px: i32,
    py: i32,
    width: i32,
    height: i32,
    radius: i32,
    glass_opacity: f32,
    reduce_transparency: bool,
) -> [u8; 4] {
    const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

    let w_f = width.max(1) as f32;
    let h_f = height.max(1) as f32;
    let hw = w_f * 0.5;
    let hh = h_f * 0.5;
    let r = (radius as f32).clamp(0.0, hw.min(hh));
    let u = px as f32 + 0.5;
    let v = py as f32 + 0.5;

    let sd = sd_rounded_box(u, v, hw, hh, hw, hh, r);
    let cov = sd_cov(sd, 1.15);
    if cov <= 0.001 {
        return [0, 0, 0, 0];
    }

    let t_v = (v / h_f).clamp(0.0, 1.0);
    let by = usize::try_from(py.rem_euclid(4)).unwrap_or(0);
    let bx = usize::try_from(px.rem_euclid(4)).unwrap_or(0);
    let dither = (f32::from(BAYER4[by][bx]) / 16.0 - 0.5) * 0.9;

    // 1 px perimeter highlight weighted cleanly toward the top curved edge like macOS liquid glass.
    let rim_band = (1.0 - ((-sd - 0.15).max(0.0) / 1.35)).clamp(0.0, 1.0);
    let dir_light = (1.0 - t_v * 0.72).clamp(0.26, 1.0);

    if reduce_transparency {
        let rim_mix = rim_band * (0.08 + 0.12 * dir_light);
        let r_col = (31.0 - 4.0 * t_v + dither) * (1.0 - rim_mix) + 244.0 * rim_mix;
        let g_col = (25.0 - 4.0 * t_v + dither) * (1.0 - rim_mix) + 238.0 * rim_mix;
        let b_col = (45.0 - 5.0 * t_v + dither) * (1.0 - rim_mix) + 255.0 * rim_mix;
        return premul_pixel(r_col, g_col, b_col, (246.0 / 255.0) * cov);
    }

    let glass_scale = crate::config::clamp_glass_opacity(glass_opacity);
    let base_a = (0.32 - 0.08 * t_v) * (0.42 + 0.58 * glass_scale);
    let mut layer = [
        (118.0 - 30.0 * t_v + dither) * base_a,
        (112.0 - 30.0 * t_v + dither) * base_a,
        (142.0 - 32.0 * t_v + dither) * base_a,
        base_a,
    ];

    if t_v < 0.22 && sd < -0.5 {
        let sheen_a = (1.0 - t_v / 0.22) * 0.055 * glass_scale;
        layer = blend_premul_over(layer, [255.0, 248.0, 255.0], sheen_a);
    }

    let rim_alpha = rim_band * (0.08 + 0.30 * dir_light);
    if rim_alpha > 0.001 {
        layer = blend_premul_over(layer, [255.0, 252.0, 255.0], rim_alpha);
    }

    let out_a = (layer[3] * cov).clamp(0.0, 1.0);
    if layer[3] <= 1e-4 || out_a <= 1e-4 {
        return [0, 0, 0, 0];
    }
    premul_pixel(
        layer[0] / layer[3],
        layer[1] / layer[3],
        layer[2] / layer[3],
        out_a,
    )
}

#[allow(clippy::cast_precision_loss)]
fn sample_dock_icon_shadow_pixel(px: i32, py: i32, icon_size: i32, pad: i32) -> [u8; 4] {
    let total = (icon_size + pad * 2).max(1) as f32;
    let center = total * 0.5;
    let half = (icon_size as f32 * 0.5 - 1.5).max(1.0);
    let r = (icon_size as f32 * 0.23).min(half);
    let sd = sd_rounded_box(
        px as f32 + 0.5,
        py as f32 + 0.5,
        center,
        center,
        half,
        half,
        r,
    );
    let spread = (pad as f32 - 0.4).max(1.0);
    let t = (sd.max(0.0) / spread).clamp(0.0, 1.0);
    let falloff = 1.0 - t;
    let alpha = if sd <= 0.0 {
        0.30
    } else {
        0.30 * falloff * falloff * falloff
    };
    premul_pixel(10.0, 6.0, 18.0, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_and_corner_sdf_preserve_clean_edges_and_opaque_interior() {
        // Circle center must be opaque, extreme corner pixel outside circle must be transparent
        let center = sample_circle_pixel(6, 6, 12, [229, 107, 117, 255], None);
        let corner = sample_circle_pixel(0, 0, 12, [229, 107, 117, 255], None);
        assert_eq!(center[3], 255);
        assert_eq!(corner[3], 0);

        // TopLeft corner patch: (0, 0) outside arc is 0, inner pixel (9, 9) is filled
        let outer = sample_corner_pixel(
            0,
            0,
            10,
            CornerQuadrant::TopLeft,
            [32, 24, 46, 245],
            Some([255, 255, 255, 56]),
        );
        let inner = sample_corner_pixel(
            9,
            9,
            10,
            CornerQuadrant::TopLeft,
            [32, 24, 46, 245],
            Some([255, 255, 255, 56]),
        );
        assert_eq!(outer[3], 0);
        assert!(inner[3] >= 240);

        // Shadow corner: inside inner_radius is 0 alpha so glass stays clean; beyond inner_radius fades smoothly
        let inside_glass =
            sample_shadow_corner_pixel(23, 23, 10, 14, CornerQuadrant::TopLeft, [5, 2, 12], 96);
        let outside_halo =
            sample_shadow_corner_pixel(12, 12, 10, 14, CornerQuadrant::TopLeft, [5, 2, 12], 96);
        assert_eq!(inside_glass[3], 0);
        assert!(outside_halo[3] > 0 && outside_halo[3] <= 96);

        // Ambient wallpaper gradient is opaque and modulated smoothly around aurora glow
        let glow_px = sample_wallpaper_pixel(62, 35, 256, 160);
        let dark_px = sample_wallpaper_pixel(250, 10, 256, 160);
        assert_eq!(glow_px[3], 255);
        assert_eq!(dark_px[3], 255);
        assert!(glow_px[0] > dark_px[0]);

        // Desktop arrow cursor: opaque dark interior, bright rim, and transparent empty corner
        let arrow_inside = sample_cursor_arrow_pixel(5, 9);
        let arrow_outside = sample_cursor_arrow_pixel(22, 2);
        assert_eq!(arrow_inside[3], 255);
        assert!(arrow_inside[2] < 80);
        assert_eq!(arrow_outside[3], 0);
    }

    #[test]
    fn parses_binary_xcursor_chunk_and_selects_closest_size() -> Result<(), String> {
        let mut blob = Vec::new();
        // Header: magic "Xcur", header_size=16, version=0x10000, ntoc=2
        blob.extend_from_slice(b"Xcur");
        blob.extend_from_slice(&16_u32.to_le_bytes());
        blob.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
        blob.extend_from_slice(&2_u32.to_le_bytes());
        // TOC 0: size 16 at offset 40
        blob.extend_from_slice(&0xfffd_0002_u32.to_le_bytes());
        blob.extend_from_slice(&16_u32.to_le_bytes());
        blob.extend_from_slice(&40_u32.to_le_bytes());
        // TOC 1: size 24 at offset 80
        blob.extend_from_slice(&0xfffd_0002_u32.to_le_bytes());
        blob.extend_from_slice(&24_u32.to_le_bytes());
        blob.extend_from_slice(&80_u32.to_le_bytes());

        // Chunk 0 (1x1 at offset 40, total 40 bytes)
        blob.extend_from_slice(&36_u32.to_le_bytes());
        blob.extend_from_slice(&0xfffd_0002_u32.to_le_bytes());
        blob.extend_from_slice(&16_u32.to_le_bytes());
        blob.extend_from_slice(&1_u32.to_le_bytes());
        blob.extend_from_slice(&1_u32.to_le_bytes());
        blob.extend_from_slice(&1_u32.to_le_bytes());
        blob.extend_from_slice(&0_u32.to_le_bytes());
        blob.extend_from_slice(&0_u32.to_le_bytes());
        blob.extend_from_slice(&0_u32.to_le_bytes());
        blob.extend_from_slice(&0xFFFF_0000_u32.to_le_bytes()); // opaque red

        // Chunk 1 (1x1 at offset 80: blue pixel, hotspot (0,0), width=1, height=1)
        blob.extend_from_slice(&36_u32.to_le_bytes());
        blob.extend_from_slice(&0xfffd_0002_u32.to_le_bytes());
        blob.extend_from_slice(&24_u32.to_le_bytes());
        blob.extend_from_slice(&1_u32.to_le_bytes());
        blob.extend_from_slice(&1_u32.to_le_bytes());
        blob.extend_from_slice(&1_u32.to_le_bytes());
        blob.extend_from_slice(&0_u32.to_le_bytes());
        blob.extend_from_slice(&0_u32.to_le_bytes());
        blob.extend_from_slice(&0_u32.to_le_bytes());
        blob.extend_from_slice(&0xFF00_00FF_u32.to_le_bytes()); // ARGB opaque blue

        let (w, h, xhot, yhot, bgra) =
            parse_xcursor_bytes(&blob, 24).ok_or_else(|| "Xcur decode failed".to_owned())?;
        assert_eq!((w, h, xhot, yhot), (1, 1, 0, 0));
        assert_eq!(bgra, vec![0xFF, 0x00, 0x00, 0xFF]);
        Ok(())
    }

    #[test]
    fn hidpi_cursor_origin_and_plane_limit_scale_correctly() {
        // 1.5x display: pointer (100, 50), hotspot (4.0, 2.0) => logical (96, 48) => physical (144, 72)
        let phys =
            scaled_cursor_physical_origin(Point::from((100, 50)), Point::from((4.0, 2.0)), 1.5);
        assert_eq!(phys, Point::from((144.0, 72.0)));

        // 2.0x scale pixel target is clamped cleanly from default/env size
        let px_1x = xcursor_target_pixel_size(1.0);
        let px_2x = xcursor_target_pixel_size(2.0);
        assert!(px_2x >= px_1x);

        // 24x24 cursor at 2.0x = 48x48 fits inside 64x64 plane, whereas 4.0x = 96x96 falls back to Unspecified.
        let plane_64 = Some(Size::<u32, BufferCoord>::from((64, 64)));
        assert_eq!(
            resolve_cursor_element_kind(true, Size::from((24, 24)), 2.0, plane_64),
            Kind::Cursor
        );
        assert_eq!(
            resolve_cursor_element_kind(true, Size::from((24, 24)), 4.0, plane_64),
            Kind::Unspecified
        );
        assert_eq!(
            resolve_cursor_element_kind(false, Size::from((24, 24)), 1.0, plane_64),
            Kind::Unspecified
        );
    }

    #[test]
    fn dithered_wallpaper_eliminates_flat_banding_and_ui_glyphs_rasterize_cleanly() {
        // Check Bayer 4x4 dither modulates adjacent gradient steps cleanly
        let mut blue_samples = std::collections::BTreeSet::new();
        for y in 0..4 {
            for x in 0..4 {
                let px = sample_wallpaper_pixel(200 + x, 120 + y, 640, 360);
                assert_eq!(px[3], 255);
                blue_samples.insert(px[0]);
            }
        }
        assert!(blue_samples.len() >= 2);

        // Verify all 6 TopBar/Spotlight SDF UI glyph kinds produce smooth non-empty centers & clean edges
        let white = [245, 240, 255, 255];
        for kind in [
            UiGlyphKind::RustyLogo,
            UiGlyphKind::Wifi { connected: true },
            UiGlyphKind::Wifi { connected: false },
            UiGlyphKind::Volume {
                level_percent: 80,
                muted: false,
            },
            UiGlyphKind::Volume {
                level_percent: 0,
                muted: true,
            },
            UiGlyphKind::Battery {
                percent: 65,
                charging: true,
            },
            UiGlyphKind::Search,
            UiGlyphKind::ControlCenter,
        ] {
            let mut non_zero = 0_usize;
            for py in 0..16 {
                for px in 0..16 {
                    let p = sample_ui_glyph_pixel(px, py, 16, kind, white);
                    if p[3] > 24 {
                        non_zero += 1;
                    }
                }
            }
            assert!(non_zero >= 18, "Glyph {kind:?} too sparse: {non_zero}");
            let corner = sample_ui_glyph_pixel(0, 0, 16, kind, white);
            assert_eq!(corner[3], 0, "Glyph {kind:?} corner should be transparent");
        }
    }

    #[test]
    fn dock_glass_and_icon_shadow_rasterize_cleanly() {
        let width = 320;
        let height = 68;
        let radius = 22;

        // Translucent frosted Dock glass: corners transparent, top rim brighter than center,
        // and center remains light/translucent rather than an opaque dark slab.
        let corner = sample_dock_glass_pixel(0, 0, width, height, radius, 0.78, false);
        let top_rim = sample_dock_glass_pixel(width / 2, 0, width, height, radius, 0.78, false);
        let center =
            sample_dock_glass_pixel(width / 2, height / 2, width, height, radius, 0.78, false);
        assert_eq!(corner[3], 0);
        assert!(top_rim[3] > center[3]);
        assert!(center[3] > 20 && center[3] <= 120);

        // Reduce-transparency mode falls back to opaque dark plum-slate container.
        let opaque_center =
            sample_dock_glass_pixel(width / 2, height / 2, width, height, radius, 0.78, true);
        assert!(opaque_center[3] >= 245);

        // Soft icon drop shadow fades cleanly to zero at outer pad corners.
        let icon_size = 50;
        let pad = 7;
        let shadow_corner = sample_dock_icon_shadow_pixel(0, 0, icon_size, pad);
        let shadow_center = sample_dock_icon_shadow_pixel(
            i32::midpoint(icon_size, pad * 2),
            i32::midpoint(icon_size, pad * 2),
            icon_size,
            pad,
        );
        assert_eq!(shadow_corner[3], 0);
        assert!(shadow_center[3] > 40);
    }
}
