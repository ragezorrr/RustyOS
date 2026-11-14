//! GPU-cached SDF shape primitives (`UiShapeCache`) for macOS-style rounded corners,
//! anti-aliased circles, and soft drop shadows (`RUSTYOS-022`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::input::pointer::CursorIcon;
use smithay::utils::{Buffer as BufferCoord, Logical, Point, Rectangle, Size, Transform};

mod glyphs;
mod sdf;
#[cfg(test)]
mod tests;
mod xcursor;

use glyphs::rasterize_ui_glyph_buffer_rect;
pub(crate) use sdf::signed_distance_polygon;
use sdf::{
    rasterize_circle_buffer, rasterize_corner_buffer, rasterize_rgba_rect,
    rasterize_shadow_corner_buffer, rasterize_wallpaper_buffer, sample_dock_icon_shadow_pixel,
    sample_fallback_avatar_pixel, sample_glass_panel_pixel, sample_traffic_light_pixel,
};
use xcursor::{load_xcursor_for_icon, rasterize_cursor_arrow_buffer};
pub(crate) use xcursor::{
    resolve_cursor_element_kind, scaled_cursor_physical_origin, xcursor_target_pixel_size,
};

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

/// Top-lit frosted glass panel variant used by shell surfaces (`RUSTYOS-050`/`051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlassPanelStyle {
    /// Floating bottom Dock container (`RUSTYOS-050`).
    Dock,
    /// Elevated menus, Control Center, Spotlight, Notification HUD & OSD (`RUSTYOS-051`).
    Popover,
    /// Lighter inner glass tile/track inside Control Center (`RUSTYOS-051`).
    ControlModule,
}

/// Interactive macOS-style window traffic light button kind (`RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrafficLightKind {
    Close,
    Minimize,
    Maximize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlassPanelKey {
    style: GlassPanelStyle,
    width: u16,
    height: u16,
    radius: u16,
    glass_pct: u8,
    reduce_transparency: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TrafficLightKey {
    kind: TrafficLightKind,
    diameter: u16,
    focused: bool,
    hovered_glyph: bool,
}

#[derive(Debug)]
pub(super) struct CachedCursor {
    pub(super) buffer: MemoryRenderBuffer,
    pub(super) logical_size: Size<i32, Logical>,
    pub(super) raw_size: Size<f64, Logical>,
    pub(super) hotspot: Point<f64, Logical>,
}

/// Procedural anti-aliased UI icon glyphs for `TopBar`, Menus, Control Center, `Spotlight` & `OSD` (`RUSTYOS-048`..`051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiGlyphKind {
    /// Faceted RustyOS diamond/prism emblem replacing missing fallback glyphs.
    RustyLogo,
    /// Smooth 3-band concentric Wi-Fi fan with live signal bar indication.
    Wifi {
        connected: bool,
        signal_percent: Option<u8>,
    },
    /// Speaker cone + wave arcs or mute slash.
    Volume { level_percent: u8, muted: bool },
    /// Rounded pill battery outline + level fill + optional charging bolt.
    Battery { percent: u8, charging: bool },
    /// Magnifying glass search loupe (`TopBar` / `Spotlight`).
    Search,
    /// Dual macOS Control Center toggle switches.
    ControlCenter,
    /// Circled info `i` glyph for `About RustyOS`.
    Info,
    /// Compact `>_` terminal outline glyph.
    TerminalPrompt,
    /// Minimal folder outline glyph for `Files`.
    Folder,
    /// 2x2 workspace grid glyph for `Overview`.
    Grid,
    /// Split-pane layout toggle glyph (`Float / Tile`).
    LayoutSplit,
    /// Overlapping motion rings glyph (`Reduced Motion`).
    Motion,
    /// Overlapping translucent squares glyph (`Opaque / Clear`).
    Transparency,
    /// Circular refresh arrow glyph.
    Refresh,
    /// Padlock glyph (`Lock Screen`).
    Lock,
    /// Looped command-key symbol for `Keyboard Shortcuts`.
    Shortcuts,
    /// Window expand / maximize outline glyph.
    WindowMax,
    /// Window minimize underline glyph.
    WindowMin,
    /// Crisp close `X` glyph.
    Close,
    /// Forward arrow `->` (`Next Workspace` / `Focus Next`).
    ArrowNext,
    /// Backward arrow `<-` (`Previous Workspace`).
    ArrowPrev,
    /// Classic Bluetooth rune glyph.
    Bluetooth,
    /// 8-ray sun glyph for `Display` / `Glass` / `Brightness`.
    Sun,
    /// Double-triangle seek-previous `<<` glyph for Now Playing (`RUSTYOS-053`).
    MediaPrev,
    /// Solid rounded triangle play glyph for Now Playing (`RUSTYOS-053`).
    MediaPlay,
    /// Dual vertical bar pause glyph for Now Playing (`RUSTYOS-053`).
    MediaPause,
    /// Double-triangle seek-next `>>` glyph for Now Playing (`RUSTYOS-053`).
    MediaNext,
    /// Eighth-note glyph for Now Playing artwork tile (`RUSTYOS-053`).
    MusicNote,
    /// Crescent moon glyph for `Focus` Do Not Disturb capsule (`RUSTYOS-053`).
    Moon,
    /// Overlapping rounded window panes for `Overview` spaces circle (`RUSTYOS-053`).
    OverviewSpaces,
    /// Compact ANSI keyboard outline for Lock Screen input source switcher (`RUSTYOS-054`).
    Keyboard,
    /// Power `⏻` glyph for Lock Screen top-right power controls (`RUSTYOS-054`).
    PowerSymbol,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AvatarKey {
    path: Option<PathBuf>,
    diameter: u16,
    seed: u32,
    selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    kind: UiGlyphKind,
    width_px: u16,
    height_px: u16,
    tint_rgba: [u8; 4],
}

#[derive(Debug, Default)]
struct CachedWallpaper {
    revision: u64,
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
    glass_panels: HashMap<GlassPanelKey, MemoryRenderBuffer>,
    traffic_lights: HashMap<TrafficLightKey, MemoryRenderBuffer>,
    dock_icon_shadows: HashMap<u16, MemoryRenderBuffer>,
    avatars: HashMap<AvatarKey, MemoryRenderBuffer>,
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
        self.glass_panel_element(
            renderer,
            GlassPanelStyle::Dock,
            width,
            height,
            radius,
            glass_opacity,
            reduce_transparency,
            loc,
        )
    }

    /// Returns a GPU-cached Liquid Glass panel container for menus, Control Center, Spotlight,
    /// Notification HUD, OSD, and Dock (`RUSTYOS-050` / `RUSTYOS-051`).
    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn glass_panel_element(
        &mut self,
        renderer: &mut GlesRenderer,
        style: GlassPanelStyle,
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
        let key = GlassPanelKey {
            style,
            width: w_u16,
            height: h_u16,
            radius: r_u16,
            glass_pct,
            reduce_transparency,
        };
        if !self.glass_panels.contains_key(&key) {
            if self.glass_panels.len() >= 48 {
                self.glass_panels.clear();
            }
            let clamped_glass = f32::from(glass_pct) / 100.0;
            let buf = rasterize_rgba_rect(width, height, |px, py| {
                sample_glass_panel_pixel(
                    px,
                    py,
                    width,
                    height,
                    i32::from(r_u16),
                    clamped_glass,
                    reduce_transparency,
                    style,
                )
            });
            self.glass_panels.insert(key, buf);
        }
        let buf = self.glass_panels.get(&key)?;
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

    /// Returns a GPU-cached 4x4 SSAA gel-lit traffic light window control button (`RUSTYOS-051`).
    pub fn traffic_light_element(
        &mut self,
        renderer: &mut GlesRenderer,
        kind: TrafficLightKind,
        diameter: i32,
        focused: bool,
        hovered_glyph: bool,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if diameter <= 0 {
            return None;
        }
        let d_u16 = u16::try_from(diameter).ok()?;
        let key = TrafficLightKey {
            kind,
            diameter: d_u16,
            focused,
            hovered_glyph,
        };
        let buf = self.traffic_lights.entry(key).or_insert_with(|| {
            rasterize_rgba_rect(diameter, diameter, |px, py| {
                sample_traffic_light_pixel(px, py, diameter, kind, focused, hovered_glyph)
            })
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

    /// Invalidates cached custom wallpaper GPU textures when compositor `wallpaper_revision` changes (`RUSTYOS-062`).
    pub fn sync_wallpaper_revision(&mut self, revision: u64) {
        if self.wallpaper.revision != revision {
            self.wallpaper.custom_key = None;
            self.wallpaper.custom_buffer = None;
            self.wallpaper.revision = revision;
        }
    }

    /// Returns a GPU-cached wallpaper scaled cleanly to `screen_size`, preferring `custom_path` (`.png`)
    /// from `[ui] wallpaper` and falling back to the ambient Purple Glass gradient (`RUSTYOS-034`).
    pub fn wallpaper_element(
        &mut self,
        renderer: &mut GlesRenderer,
        screen_size: Size<i32, Logical>,
        custom_path: Option<&Path>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        self.wallpaper_element_with_alpha(renderer, screen_size, custom_path, 1.0)
    }

    /// Returns a GPU-cached wallpaper element scaled to `screen_size` with explicit opacity (`RUSTYOS-054`).
    pub fn wallpaper_element_with_alpha(
        &mut self,
        renderer: &mut GlesRenderer,
        screen_size: Size<i32, Logical>,
        custom_path: Option<&Path>,
        alpha: f32,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        const WALLPAPER_W: i32 = 640;
        const WALLPAPER_H: i32 = 360;
        let alpha = alpha.clamp(0.0, 1.0);
        if screen_size.w <= 0 || screen_size.h <= 0 || alpha <= 0.001 {
            return None;
        }
        let alpha_opt = (alpha < 0.999).then_some(alpha);

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
                    alpha_opt,
                    None,
                    None,
                    Kind::Unspecified,
                )
                .ok();
            }
        } else if self.wallpaper.custom_key.is_some() || self.wallpaper.custom_buffer.is_some() {
            self.wallpaper.custom_key = None;
            self.wallpaper.custom_buffer = None;
        }

        let buf = self
            .wallpaper
            .fallback_buffer
            .get_or_insert_with(|| rasterize_wallpaper_buffer(WALLPAPER_W, WALLPAPER_H));
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, smithay::utils::Physical>::from((0.0, 0.0)),
            buf,
            alpha_opt,
            Some(Rectangle::<f64, Logical>::from_size(Size::from((
                f64::from(WALLPAPER_W),
                f64::from(WALLPAPER_H),
            )))),
            Some(screen_size),
            Kind::Unspecified,
        )
        .ok()
    }

    /// Returns a GPU-cached circular user avatar badge (`~/.face`/`AccountsService` PNG or crisp
    /// stylized silhouette) for the Lock Screen (`RUSTYOS-054`).
    pub fn user_avatar_element(
        &mut self,
        renderer: &mut GlesRenderer,
        diameter: i32,
        avatar_path: Option<&Path>,
        seed: u32,
        selected: bool,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if diameter <= 0 {
            return None;
        }
        let d_u16 = u16::try_from(diameter).ok()?;
        let key = AvatarKey {
            path: avatar_path.map(Path::to_path_buf),
            diameter: d_u16,
            seed,
            selected,
        };
        if !self.avatars.contains_key(&key) {
            let d_usize = usize::try_from(diameter).ok()?;
            let loaded = avatar_path
                .and_then(|p| crate::icons::load_circular_avatar_bgra(p, d_usize))
                .map(|bgra| {
                    MemoryRenderBuffer::from_slice(
                        &bgra,
                        Fourcc::Argb8888,
                        (diameter, diameter),
                        1,
                        Transform::Normal,
                        None,
                    )
                });
            let buf = loaded.unwrap_or_else(|| {
                rasterize_rgba_rect(diameter, diameter, |px, py| {
                    sample_fallback_avatar_pixel(px, py, diameter, seed, selected)
                })
            });
            self.avatars.insert(key.clone(), buf);
        }
        let buf = self.avatars.get(&key)?;
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
        self.ui_glyph_rect_element(renderer, kind, size_px, size_px, tint_rgba, loc)
    }

    /// Returns a GPU-cached 4x4 SSAA rectangular procedural icon element positioned at `loc`.
    pub fn ui_glyph_rect_element(
        &mut self,
        renderer: &mut GlesRenderer,
        kind: UiGlyphKind,
        width_px: i32,
        height_px: i32,
        tint_rgba: [u8; 4],
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let w_u16 = u16::try_from(width_px).ok()?;
        let h_u16 = u16::try_from(height_px).ok()?;
        if width_px <= 0 || height_px <= 0 {
            return None;
        }
        let key = GlyphKey {
            kind,
            width_px: w_u16,
            height_px: h_u16,
            tint_rgba,
        };
        let buf = self.glyphs.entry(key).or_insert_with(|| {
            rasterize_ui_glyph_buffer_rect(kind, width_px, height_px, tint_rgba)
        });
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_f64().to_physical(1.0),
            buf,
            None,
            Some(Rectangle::<f64, Logical>::from_size(Size::from((
                f64::from(width_px),
                f64::from(height_px),
            )))),
            Some(Size::from((width_px, height_px))),
            Kind::Unspecified,
        )
        .ok()
    }
}
