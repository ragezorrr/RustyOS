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
struct CachedCursor {
    buffer: MemoryRenderBuffer,
    logical_size: Size<i32, Logical>,
    raw_size: Size<f64, Logical>,
    hotspot: Point<f64, Logical>,
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

/// Signed distance to an upward-opening circular pie sector (`half_angle_rad`, rounded by `corner`).
fn sd_pie_up(
    u: f32,
    v: f32,
    cx: f32,
    cy: f32,
    radius: f32,
    half_angle_rad: f32,
    corner: f32,
) -> f32 {
    let px = (u - cx).abs();
    let py = cy - v;
    let (sin_a, cos_a) = half_angle_rad.sin_cos();
    let radial_d = px.hypot(py) - radius;
    let proj = (px * sin_a + py * cos_a).clamp(0.0, radius);
    let ray_dist = (px - sin_a * proj).hypot(py - cos_a * proj);
    let outside_sign = if px * cos_a > py * sin_a { 1.0 } else { -1.0 };
    radial_d.max(ray_dist * outside_sign) - corner
}

fn sample_rusty_logo_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_wifi_glyph_uv(
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

fn sample_search_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let ring = sd_circle(u, v, 0.42, 0.42, 0.255).abs() - 0.062;
    let handle = sd_segment(u, v, 0.61, 0.61, 0.87, 0.87, 0.064);
    let cov = sd_cov(ring.min(handle), edge);
    premul_tint(tint, cov)
}

fn sample_control_center_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_info_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let ring = sd_circle(u, v, 0.50, 0.50, 0.36).abs() - 0.044;
    let dot = sd_circle(u, v, 0.50, 0.31, 0.048);
    let stem = sd_segment(u, v, 0.50, 0.44, 0.50, 0.69, 0.046);
    premul_tint(tint, sd_cov(ring.min(dot).min(stem), edge))
}

fn sample_terminal_prompt_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let frame = sd_rounded_box(u, v, 0.50, 0.50, 0.37, 0.31, 0.10).abs() - 0.042;
    let chev_top = sd_segment(u, v, 0.27, 0.37, 0.41, 0.50, 0.045);
    let chev_bot = sd_segment(u, v, 0.41, 0.50, 0.27, 0.63, 0.045);
    let dash = sd_segment(u, v, 0.48, 0.63, 0.72, 0.63, 0.045);
    premul_tint(
        tint,
        sd_cov(frame.min(chev_top).min(chev_bot).min(dash), edge),
    )
}

fn sample_folder_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_grid_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let tl = sd_rounded_box(u, v, 0.31, 0.31, 0.125, 0.125, 0.045);
    let tr = sd_rounded_box(u, v, 0.69, 0.31, 0.125, 0.125, 0.045);
    let bl = sd_rounded_box(u, v, 0.31, 0.69, 0.125, 0.125, 0.045);
    let br = sd_rounded_box(u, v, 0.69, 0.69, 0.125, 0.125, 0.045);
    let cov = sd_cov(tl.min(tr).min(bl).min(br), edge);
    premul_tint(tint, cov)
}

fn sample_layout_split_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_motion_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_transparency_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_refresh_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let dx = u - 0.50;
    let dy = v - 0.52;
    let ring = (dx.hypot(dy) - 0.29).abs() - 0.052;
    // Open upper-right gap between top and 3 o'clock before the arrowhead.
    let gap = u8::from(dx > 0.04 && dy < -0.02 && dy > -0.26);
    let arc = if gap == 1 { 0.2 } else { ring };
    let head = signed_distance_polygon(u, v, &[(0.52, 0.14), (0.76, 0.26), (0.55, 0.38)]);
    premul_tint(tint, sd_cov(arc.min(head), edge))
}

fn sample_lock_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_shortcuts_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_window_max_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let frame = sd_rounded_box(u, v, 0.50, 0.50, 0.35, 0.29, 0.09).abs() - 0.042;
    let diag1 = sd_segment(u, v, 0.33, 0.67, 0.67, 0.33, 0.042);
    let diag2 = sd_segment(u, v, 0.52, 0.33, 0.67, 0.33, 0.042);
    let diag3 = sd_segment(u, v, 0.67, 0.33, 0.67, 0.48, 0.042);
    premul_tint(tint, sd_cov(frame.min(diag1).min(diag2).min(diag3), edge))
}

fn sample_window_min_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let frame = sd_rounded_box(u, v, 0.50, 0.50, 0.35, 0.29, 0.09).abs() - 0.042;
    let dash = sd_segment(u, v, 0.32, 0.64, 0.68, 0.64, 0.046);
    premul_tint(tint, sd_cov(frame.min(dash), edge))
}

fn sample_close_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let d1 = sd_segment(u, v, 0.28, 0.28, 0.72, 0.72, 0.062);
    let d2 = sd_segment(u, v, 0.72, 0.28, 0.28, 0.72, 0.062);
    premul_tint(tint, sd_cov(d1.min(d2), edge))
}

fn sample_arrow_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4], forward: bool) -> [f32; 4] {
    let x = if forward { u } else { 1.0 - u };
    let shaft = sd_segment(x, v, 0.20, 0.50, 0.78, 0.50, 0.052);
    let tip_top = sd_segment(x, v, 0.55, 0.27, 0.78, 0.50, 0.052);
    let tip_bot = sd_segment(x, v, 0.55, 0.73, 0.78, 0.50, 0.052);
    premul_tint(tint, sd_cov(shaft.min(tip_top).min(tip_bot), edge))
}

fn sample_bluetooth_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let spine = sd_segment(u, v, 0.48, 0.16, 0.48, 0.84, 0.048);
    let top_r = sd_segment(u, v, 0.48, 0.16, 0.69, 0.34, 0.046);
    let cross1 = sd_segment(u, v, 0.69, 0.34, 0.29, 0.68, 0.046);
    let bot_r = sd_segment(u, v, 0.48, 0.84, 0.69, 0.66, 0.046);
    let cross2 = sd_segment(u, v, 0.69, 0.66, 0.29, 0.32, 0.046);
    let d = spine.min(top_r).min(cross1).min(bot_r).min(cross2);
    premul_tint(tint, sd_cov(d, edge))
}

fn sample_sun_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_media_play_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let tri = signed_distance_polygon(u, v, &[(0.28, 0.20), (0.80, 0.50), (0.28, 0.80)]) - 0.028;
    premul_tint(tint, sd_cov(tri, edge))
}

fn sample_media_pause_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let left = sd_rounded_box(u, v, 0.34, 0.50, 0.088, 0.27, 0.032);
    let right = sd_rounded_box(u, v, 0.66, 0.50, 0.088, 0.27, 0.032);
    premul_tint(tint, sd_cov(left.min(right), edge))
}

fn sample_media_seek_glyph_uv(
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

fn sample_music_note_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let head1 = sd_circle(u, v, 0.31, 0.71, 0.115);
    let head2 = sd_circle(u, v, 0.67, 0.65, 0.115);
    let stem1 = sd_segment(u, v, 0.40, 0.27, 0.40, 0.70, 0.042);
    let stem2 = sd_segment(u, v, 0.76, 0.21, 0.76, 0.64, 0.042);
    let beam = sd_segment(u, v, 0.40, 0.27, 0.76, 0.21, 0.056);
    let d = head1.min(head2).min(stem1).min(stem2).min(beam);
    premul_tint(tint, sd_cov(d, edge))
}

fn sample_moon_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let outer = sd_circle(u, v, 0.47, 0.52, 0.30);
    let cut = sd_circle(u, v, 0.61, 0.40, 0.24);
    premul_tint(tint, sd_cov(outer.max(-cut), edge))
}

fn sample_overview_spaces_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_keyboard_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
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

fn sample_power_symbol_glyph_uv(u: f32, v: f32, edge: f32, tint: [f32; 4]) -> [f32; 4] {
    let dx = u - 0.50;
    let dy = v - 0.53;
    let ring = (dx.hypot(dy) - 0.28).abs() - 0.044;
    let top_notch = sd_rounded_box(u, v, 0.50, 0.27, 0.14, 0.14, 0.02);
    let arc = ring.max(-top_notch);
    let stem = sd_segment(u, v, 0.50, 0.16, 0.50, 0.47, 0.048);
    premul_tint(tint, sd_cov(arc.min(stem), edge))
}

#[allow(clippy::cast_precision_loss)]
fn sample_fallback_avatar_pixel(
    px: i32,
    py: i32,
    diameter: i32,
    seed: u32,
    selected: bool,
) -> [u8; 4] {
    const SUBSAMPLES: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
    let dim = diameter.max(1) as f32;
    let edge = 1.05 / dim;
    let palette = match seed % 4 {
        0 => ([122.0_f32, 92.0, 214.0], [238.0_f32, 198.0, 154.0]),
        1 => ([86.0_f32, 134.0, 224.0], [186.0_f32, 220.0, 250.0]),
        2 => ([168.0_f32, 92.0, 166.0], [244.0_f32, 186.0, 206.0]),
        _ => ([94.0_f32, 148.0, 142.0], [214.0_f32, 232.0, 194.0]),
    };
    let mut sum = [0.0_f32; 4];
    for sy in SUBSAMPLES {
        for sx in SUBSAMPLES {
            let u = (px as f32 + sx) / dim;
            let v = (py as f32 + sy) / dim;
            let outer = sd_circle(u, v, 0.5, 0.5, 0.475);
            let cov = sd_cov(outer, edge);
            if cov <= 0.001 {
                continue;
            }
            let radial = ((u - 0.38).hypot(v - 0.30) / 0.72).clamp(0.0, 1.0);
            let mut red = palette.1[0] * (1.0 - radial) + palette.0[0] * radial;
            let mut green = palette.1[1] * (1.0 - radial) + palette.0[1] * radial;
            let mut blue = palette.1[2] * (1.0 - radial) + palette.0[2] * radial;

            let head = sd_circle(u, v, 0.50, 0.38, 0.155);
            let shoulders = sd_circle(u, v, 0.50, 0.90, 0.34);
            let fig = sd_cov(head.min(shoulders), edge);
            if fig > 0.001 {
                let fa = fig * 0.90;
                red = red * (1.0 - fa) + 252.0 * fa;
                green = green * (1.0 - fa) + 248.0 * fa;
                blue = blue * (1.0 - fa) + 255.0 * fa;
            }

            let rim_sd = (outer + 0.018).abs() - 0.016;
            let rim_cov = sd_cov(rim_sd, edge) * if selected { 0.92 } else { 0.62 };
            if rim_cov > 0.001 {
                red = red * (1.0 - rim_cov) + 255.0 * rim_cov;
                green = green * (1.0 - rim_cov) + 252.0 * rim_cov;
                blue = blue * (1.0 - rim_cov) + 255.0 * rim_cov;
            }

            sum[0] += red * cov;
            sum[1] += green * cov;
            sum[2] += blue * cov;
            sum[3] += cov;
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

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
fn sample_ui_glyph_pixel(
    px: i32,
    py: i32,
    size_px: i32,
    kind: UiGlyphKind,
    tint_rgba: [u8; 4],
) -> [u8; 4] {
    sample_ui_glyph_pixel_rect(px, py, size_px, size_px, kind, tint_rgba)
}

#[allow(clippy::cast_precision_loss)]
fn sample_ui_glyph_pixel_rect(
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

fn rasterize_ui_glyph_buffer_rect(
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

pub(crate) fn signed_distance_polygon(px: f32, py: f32, verts: &[(f32, f32)]) -> f32 {
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

#[cfg(test)]
fn sample_dock_glass_pixel(
    px: i32,
    py: i32,
    width: i32,
    height: i32,
    radius: i32,
    glass_opacity: f32,
    reduce_transparency: bool,
) -> [u8; 4] {
    sample_glass_panel_pixel(
        px,
        py,
        width,
        height,
        radius,
        glass_opacity,
        reduce_transparency,
        GlassPanelStyle::Dock,
    )
}

#[allow(clippy::cast_precision_loss, clippy::too_many_arguments)]
fn sample_glass_panel_pixel(
    px: i32,
    py: i32,
    width: i32,
    height: i32,
    radius: i32,
    glass_opacity: f32,
    reduce_transparency: bool,
    style: GlassPanelStyle,
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
        let (base_rgb, rim_scale, alpha) = match style {
            GlassPanelStyle::Dock => ([31.0, 25.0, 45.0], 1.0, 246.0 / 255.0),
            GlassPanelStyle::Popover => ([34.0, 27.0, 50.0], 1.15, 248.0 / 255.0),
            GlassPanelStyle::ControlModule => ([54.0, 44.0, 76.0], 0.90, 232.0 / 255.0),
        };
        let rim_mix = rim_band * (0.08 + 0.12 * dir_light) * rim_scale;
        let r_col = (base_rgb[0] - 4.0 * t_v + dither) * (1.0 - rim_mix) + 244.0 * rim_mix;
        let g_col = (base_rgb[1] - 4.0 * t_v + dither) * (1.0 - rim_mix) + 238.0 * rim_mix;
        let b_col = (base_rgb[2] - 5.0 * t_v + dither) * (1.0 - rim_mix) + 255.0 * rim_mix;
        return premul_pixel(r_col, g_col, b_col, alpha * cov);
    }

    let glass_scale = crate::config::clamp_glass_opacity(glass_opacity);
    let mut layer = match style {
        GlassPanelStyle::Dock => {
            let base_a = (0.32 - 0.08 * t_v) * (0.42 + 0.58 * glass_scale);
            [
                (118.0 - 30.0 * t_v + dither) * base_a,
                (112.0 - 30.0 * t_v + dither) * base_a,
                (142.0 - 32.0 * t_v + dither) * base_a,
                base_a,
            ]
        }
        GlassPanelStyle::Popover => {
            let base_a = (0.76 - 0.08 * t_v) * (0.48 + 0.52 * glass_scale);
            [
                (48.0 - 12.0 * t_v + dither) * base_a,
                (37.0 - 10.0 * t_v + dither) * base_a,
                (70.0 - 16.0 * t_v + dither) * base_a,
                base_a,
            ]
        }
        GlassPanelStyle::ControlModule => {
            let base_a = (0.24 - 0.04 * t_v) * (0.48 + 0.52 * glass_scale);
            [
                (176.0 - 22.0 * t_v + dither) * base_a,
                (166.0 - 20.0 * t_v + dither) * base_a,
                (198.0 - 16.0 * t_v + dither) * base_a,
                base_a,
            ]
        }
    };

    let sheen_limit = match style {
        GlassPanelStyle::Dock => 0.22,
        GlassPanelStyle::Popover => 0.16,
        GlassPanelStyle::ControlModule => 0.20,
    };
    if t_v < sheen_limit && sd < -0.5 {
        let sheen_peak = match style {
            GlassPanelStyle::Dock | GlassPanelStyle::Popover => 0.055,
            GlassPanelStyle::ControlModule => 0.065,
        };
        let sheen_a = (1.0 - t_v / sheen_limit) * sheen_peak * glass_scale;
        layer = blend_premul_over(layer, [255.0, 248.0, 255.0], sheen_a);
    }

    let mod_rim_light = dir_light.max(((t_v - 0.78) * 1.8).clamp(0.0, 0.55));
    let rim_alpha = match style {
        GlassPanelStyle::Dock => rim_band * (0.08 + 0.30 * dir_light),
        GlassPanelStyle::Popover => rim_band * (0.14 + 0.26 * dir_light),
        GlassPanelStyle::ControlModule => rim_band * (0.10 + 0.28 * mod_rim_light),
    };
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
fn sample_traffic_light_pixel(
    px: i32,
    py: i32,
    diameter: i32,
    kind: TrafficLightKind,
    focused: bool,
    hovered_glyph: bool,
) -> [u8; 4] {
    const SUBSAMPLES: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
    let dim = diameter.max(1) as f32;
    let edge = 1.05 / dim;
    let active_look = focused || hovered_glyph;
    let (fill_top, fill_bot, ring_rgb, glyph_rgb) = if active_look {
        match kind {
            TrafficLightKind::Close => (
                [255.0, 108.0, 99.0],
                [246.0, 78.0, 70.0],
                [194.0, 56.0, 50.0],
                [76.0, 14.0, 12.0],
            ),
            TrafficLightKind::Minimize => (
                [255.0, 198.0, 62.0],
                [246.0, 174.0, 32.0],
                [196.0, 134.0, 24.0],
                [92.0, 58.0, 4.0],
            ),
            TrafficLightKind::Maximize => (
                [56.0, 212.0, 78.0],
                [34.0, 186.0, 54.0],
                [24.0, 140.0, 40.0],
                [10.0, 68.0, 18.0],
            ),
        }
    } else {
        (
            [128.0, 118.0, 152.0],
            [106.0, 96.0, 130.0],
            [84.0, 74.0, 106.0],
            [42.0, 36.0, 58.0],
        )
    };

    let mut sum = [0.0_f32; 4];
    for sy in SUBSAMPLES {
        for sx in SUBSAMPLES {
            let u = (px as f32 + sx) / dim;
            let v = (py as f32 + sy) / dim;
            let body_sd = sd_circle(u, v, 0.50, 0.50, 0.44);
            let body_cov = sd_cov(body_sd, edge);
            if body_cov <= 0.001 {
                continue;
            }
            let t = v.clamp(0.0, 1.0);
            let mut rgb = [
                fill_top[0] + (fill_bot[0] - fill_top[0]) * t,
                fill_top[1] + (fill_bot[1] - fill_top[1]) * t,
                fill_top[2] + (fill_bot[2] - fill_top[2]) * t,
            ];
            let base_a = if active_look { 1.0 } else { 0.82 };
            let mut prem = [rgb[0] * base_a, rgb[1] * base_a, rgb[2] * base_a, base_a];

            // Soft top specular dot & rim stroke so the dot reads like backlit glass gel.
            let gloss_sd = sd_circle(u, v, 0.50, 0.34, 0.24);
            let gloss_cov = sd_cov(gloss_sd, edge * 2.5) * (1.0 - t).clamp(0.0, 1.0);
            if gloss_cov > 0.0 && active_look {
                prem = blend_premul_over(prem, [255.0, 250.0, 255.0], gloss_cov * 0.22);
            }
            let rim_sd = sd_circle(u, v, 0.50, 0.50, 0.415).abs() - 0.025;
            let rim_cov = sd_cov(rim_sd, edge);
            if rim_cov > 0.0 {
                prem = blend_premul_over(prem, ring_rgb, rim_cov * 0.55);
            }
            if hovered_glyph {
                let g_sd = match kind {
                    TrafficLightKind::Close => sd_segment(u, v, 0.33, 0.33, 0.67, 0.67, 0.055)
                        .min(sd_segment(u, v, 0.67, 0.33, 0.33, 0.67, 0.055)),
                    TrafficLightKind::Minimize => sd_segment(u, v, 0.30, 0.50, 0.70, 0.50, 0.058),
                    TrafficLightKind::Maximize => sd_segment(u, v, 0.30, 0.50, 0.70, 0.50, 0.055)
                        .min(sd_segment(u, v, 0.50, 0.30, 0.50, 0.70, 0.055)),
                };
                let g_cov = sd_cov(g_sd, edge);
                if g_cov > 0.0 {
                    prem = blend_premul_over(prem, glyph_rgb, g_cov * 0.88);
                }
            }

            rgb = if prem[3] > 1e-4 {
                [prem[0] / prem[3], prem[1] / prem[3], prem[2] / prem[3]]
            } else {
                rgb
            };
            let out_a = (prem[3] * body_cov).clamp(0.0, 1.0);
            sum[0] += rgb[0] * out_a;
            sum[1] += rgb[1] * out_a;
            sum[2] += rgb[2] * out_a;
            sum[3] += out_a;
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
        0.10
    } else {
        0.10 * falloff * falloff * falloff
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
            UiGlyphKind::Wifi {
                connected: true,
                signal_percent: Some(85),
            },
            UiGlyphKind::Wifi {
                connected: false,
                signal_percent: None,
            },
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
            UiGlyphKind::Info,
            UiGlyphKind::TerminalPrompt,
            UiGlyphKind::Folder,
            UiGlyphKind::Grid,
            UiGlyphKind::LayoutSplit,
            UiGlyphKind::Motion,
            UiGlyphKind::Transparency,
            UiGlyphKind::Refresh,
            UiGlyphKind::Lock,
            UiGlyphKind::Shortcuts,
            UiGlyphKind::WindowMax,
            UiGlyphKind::WindowMin,
            UiGlyphKind::Close,
            UiGlyphKind::ArrowNext,
            UiGlyphKind::ArrowPrev,
            UiGlyphKind::Bluetooth,
            UiGlyphKind::Sun,
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
            assert!(non_zero >= 12, "Glyph {kind:?} too sparse: {non_zero}");
            let corner = sample_ui_glyph_pixel(0, 0, 16, kind, white);
            assert_eq!(corner[3], 0, "Glyph {kind:?} corner should be transparent");
        }

        // TopBar wide rectangular Battery (26x20) fills wider horizontal aspect with transparent corners
        let mut wide_cols = 0_usize;
        for px in 0..26 {
            let p = sample_ui_glyph_pixel_rect(
                px,
                10,
                26,
                20,
                UiGlyphKind::Battery {
                    percent: 82,
                    charging: true,
                },
                white,
            );
            if p[3] > 24 {
                wide_cols += 1;
            }
        }
        assert!(
            wide_cols >= 20,
            "Rectangular battery too narrow: {wide_cols}"
        );
        assert_eq!(
            sample_ui_glyph_pixel_rect(
                0,
                0,
                26,
                20,
                UiGlyphKind::Battery {
                    percent: 82,
                    charging: true,
                },
                white
            )[3],
            0
        );
    }

    #[test]
    fn topbar_wifi_and_battery_glyphs_match_live_state() {
        let white = [245, 240, 255, 255];

        // RUSTYOS-052: 3-band Wi-Fi fan (wedge anchor + 2 arcs) reacts dynamically to live signal bars
        let band1 = |sig| {
            sample_ui_glyph_pixel(
                10,
                14,
                20,
                UiGlyphKind::Wifi {
                    connected: true,
                    signal_percent: sig,
                },
                white,
            )[3]
        };
        let band2 = |sig| {
            sample_ui_glyph_pixel(
                10,
                9,
                20,
                UiGlyphKind::Wifi {
                    connected: true,
                    signal_percent: sig,
                },
                white,
            )[3]
        };
        let band3 = |sig| {
            sample_ui_glyph_pixel(
                10,
                5,
                20,
                UiGlyphKind::Wifi {
                    connected: true,
                    signal_percent: sig,
                },
                white,
            )[3]
        };
        assert!(band1(Some(15)) > 180 && band2(Some(15)) < 120 && band3(Some(15)) < 120);
        assert!(band1(Some(42)) > 180 && band2(Some(42)) > 180 && band3(Some(42)) < 120);
        assert!(band1(Some(85)) > 180 && band2(Some(85)) > 180 && band3(Some(85)) > 180);
        let gap23 = sample_ui_glyph_pixel(
            10,
            7,
            20,
            UiGlyphKind::Wifi {
                connected: true,
                signal_percent: Some(100),
            },
            white,
        )[3];
        assert!(
            gap23 < 30,
            "expected clear gap between bands 2 and 3, got alpha={gap23}"
        );

        // macOS Control Center glyph: hollow top outline capsule + solid bottom capsule with right cutout
        let cc_top_hollow = sample_ui_glyph_pixel(8, 5, 16, UiGlyphKind::ControlCenter, white);
        let cc_bot_solid = sample_ui_glyph_pixel(5, 11, 16, UiGlyphKind::ControlCenter, white);
        let cc_bot_cutout = sample_ui_glyph_pixel(11, 11, 16, UiGlyphKind::ControlCenter, white);
        assert!(cc_top_hollow[3] < 90);
        assert!(cc_bot_solid[0] > 180 && cc_bot_solid[3] > 180);
        assert!(cc_bot_cutout[3] < 90);
    }

    #[test]
    fn topbar_battery_charging_cutout_and_charge_fill() {
        let white = [245, 240, 255, 255];
        let bat_charging_core = sample_ui_glyph_pixel_rect(
            11,
            10,
            26,
            20,
            UiGlyphKind::Battery {
                percent: 100,
                charging: true,
            },
            white,
        );
        let bat_charging_side = sample_ui_glyph_pixel_rect(
            5,
            10,
            26,
            20,
            UiGlyphKind::Battery {
                percent: 100,
                charging: true,
            },
            white,
        );
        let mut charging_halo_min_alpha = u8::MAX;
        let mut unplugged_min_alpha = u8::MAX;
        for py in 7..=13 {
            for px in 8..=15 {
                let p_chg = sample_ui_glyph_pixel_rect(
                    px,
                    py,
                    26,
                    20,
                    UiGlyphKind::Battery {
                        percent: 100,
                        charging: true,
                    },
                    white,
                );
                let p_dc = sample_ui_glyph_pixel_rect(
                    px,
                    py,
                    26,
                    20,
                    UiGlyphKind::Battery {
                        percent: 100,
                        charging: false,
                    },
                    white,
                );
                charging_halo_min_alpha = charging_halo_min_alpha.min(p_chg[3]);
                unplugged_min_alpha = unplugged_min_alpha.min(p_dc[3]);
            }
        }
        assert!(bat_charging_side[0] > 200 && bat_charging_core[0] > 180);
        assert!(
            charging_halo_min_alpha < 170,
            "expected transparent cutout moat around white charging bolt: min_a={charging_halo_min_alpha}"
        );
        assert!(
            unplugged_min_alpha > 210,
            "expected solid white 100% fill when unplugged: min_a={unplugged_min_alpha}"
        );

        // Low charge shows proportional fill (bright left vs empty translucent right track)
        let bat_25_left = sample_ui_glyph_pixel_rect(
            5,
            10,
            26,
            20,
            UiGlyphKind::Battery {
                percent: 25,
                charging: false,
            },
            white,
        );
        let bat_25_right = sample_ui_glyph_pixel_rect(
            18,
            10,
            26,
            20,
            UiGlyphKind::Battery {
                percent: 25,
                charging: false,
            },
            white,
        );
        assert!(bat_25_left[0] > 200 && bat_25_right[3] < 100);
    }

    #[test]
    fn popover_glass_and_traffic_lights_rasterize_cleanly() {
        let pop_corner =
            sample_glass_panel_pixel(0, 0, 248, 220, 16, 0.78, false, GlassPanelStyle::Popover);
        let pop_center = sample_glass_panel_pixel(
            124,
            110,
            248,
            220,
            16,
            0.78,
            false,
            GlassPanelStyle::Popover,
        );
        let pop_top_rim =
            sample_glass_panel_pixel(124, 0, 248, 220, 16, 0.78, false, GlassPanelStyle::Popover);
        assert_eq!(pop_corner[3], 0);
        assert!(pop_center[3] > 120 && pop_center[3] < 240);
        assert!(pop_top_rim[3] > 0);

        let mod_center = sample_glass_panel_pixel(
            70,
            32,
            140,
            64,
            14,
            0.78,
            false,
            GlassPanelStyle::ControlModule,
        );
        assert!(mod_center[3] > 16 && mod_center[3] < pop_center[3]);
        for glyph in [
            UiGlyphKind::MediaPrev,
            UiGlyphKind::MediaPlay,
            UiGlyphKind::MediaPause,
            UiGlyphKind::MediaNext,
            UiGlyphKind::MusicNote,
            UiGlyphKind::Moon,
            UiGlyphKind::OverviewSpaces,
            UiGlyphKind::Keyboard,
            UiGlyphKind::PowerSymbol,
        ] {
            let center = sample_ui_glyph_pixel(10, 10, 20, glyph, [255, 255, 255, 255]);
            let corner = sample_ui_glyph_pixel(0, 0, 20, glyph, [255, 255, 255, 255]);
            assert_eq!(corner[3], 0);
            let any_lit = (0..20)
                .flat_map(|y| (0..20).map(move |x| (x, y)))
                .any(|(x, y)| sample_ui_glyph_pixel(x, y, 20, glyph, [255, 255, 255, 255])[3] > 0);
            assert!(any_lit || center[3] > 0);
        }
        let avatar_corner = sample_fallback_avatar_pixel(0, 0, 44, 0, true);
        let avatar_center = sample_fallback_avatar_pixel(22, 22, 44, 0, true);
        assert_eq!(avatar_corner[3], 0);
        assert_eq!(avatar_center[3], 255);

        // Traffic lights: clean circular contour + hover glyph contrast
        let close_plain =
            sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Close, true, false);
        let close_hover = sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Close, true, true);
        let close_unfocused =
            sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Close, false, false);
        let min_hover =
            sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Minimize, true, true);
        let max_hover =
            sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Maximize, true, true);
        let corner = sample_traffic_light_pixel(0, 0, 12, TrafficLightKind::Close, true, true);
        assert_eq!(corner[3], 0);
        assert_eq!(close_plain[3], 255);
        assert!(close_unfocused[3] >= 180 && close_unfocused != close_plain);
        // Hover glyph darkens the center of Close/Min/Max compared to plain gel sphere.
        assert_ne!(close_plain, close_hover);
        assert_ne!(min_hover, close_hover);
        assert_ne!(max_hover, close_hover);
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
        assert!(shadow_center[3] >= 20 && shadow_center[3] <= 34);
    }
}
