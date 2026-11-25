//! Pure-Rust PNG & SNI pixmap icon decoder and GPU-cached XDG icon resolver (`RUSTYOS-033`).
//!
//! Decodes `.png` desktop/tray icons and freedesktop `StatusNotifierItem` `IconPixmap`
//! buffers into premultiplied `Argb8888` [`MemoryRenderBuffer`] textures without adding
//! external image-decoding dependencies or touching the compositor event loop repeatedly.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::texture::{TextureBuffer, TextureRenderElement};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{ImportMemWl, buffer_dimensions};
use smithay::reexports::wayland_server::Resource as _;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};

use rusty_services::image_decode::MAX_PNG_FILE_BYTES;
#[cfg(test)]
use rusty_services::image_decode::scale_cover_rgba_to_premul_bgra;
pub use rusty_services::image_decode::{
    argb32_be_to_premul_bgra, decode_png_rgba, scale_rgba_to_premul_bgra,
};
pub(crate) use rusty_services::image_decode::{load_circular_avatar_bgra, load_png_wallpaper_bgra};

/// Raw D-Bus `StatusNotifierItem.IconPixmap` frame in network byte order (`ARGB32`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayPixmap {
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
    /// Big-endian ARGB32 pixel data (`width * height * 4` bytes).
    pub argb_be: Vec<u8>,
    /// Fast content hash used as GPU cache discriminator when an applet updates its pixmap.
    pub fingerprint: u64,
}

impl TrayPixmap {
    /// Constructs a [`TrayPixmap`] validating length bounds.
    #[must_use]
    pub fn from_argb_be(width: i32, height: i32, argb_be: Vec<u8>) -> Option<Self> {
        let w = usize::try_from(width).ok()?;
        let h = usize::try_from(height).ok()?;
        if w == 0 || h == 0 || w > 512 || h > 512 {
            return None;
        }
        let expected = w.checked_mul(h)?.checked_mul(4)?;
        if argb_be.len() != expected {
            return None;
        }
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for &byte in argb_be.iter().step_by(3) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        Some(Self {
            width,
            height,
            argb_be,
            fingerprint: hash,
        })
    }
}

/// Cached `xdg_toplevel_icon_v1` metadata and SHM buffers for a managed window (`RUSTYOS-047`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShmToplevelIcon {
    /// Monotonic revision bumped whenever `xdg_toplevel_icon_v1` updates for the window.
    pub revision: u64,
    /// Optional XDG icon theme name from `xdg_toplevel_icon.set_name`.
    pub icon_name: Option<String>,
    /// Client-provided SHM `(wl_buffer, scale)` candidates from `xdg_toplevel_icon.add_buffer`.
    pub buffers: Vec<(WlBuffer, i32)>,
}

/// Resolved window icon render element emitted by [`IconCache::window_icon_element`] (`RUSTYOS-047`).
#[derive(Debug)]
pub enum ResolvedIconElement {
    /// CPU-decoded PNG or SNI pixmap icon element.
    Memory(Box<MemoryRenderBufferRenderElement<GlesRenderer>>),
    /// Client-provided Wayland SHM texture icon element (`xdg_toplevel_icon_v1`).
    Texture(TextureRenderElement<GlesTexture>),
}

#[derive(Debug)]
struct CachedShmTexture {
    buffer: TextureBuffer<GlesTexture>,
    src_size: Size<i32, smithay::utils::Buffer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IconCacheKey {
    source_key: String,
    size_px: u16,
    fingerprint: u64,
}

/// Procedural macOS-style app icons for Dock pinned slots and unthemed windows (`RUSTYOS-048`/`050`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinAppIcon {
    /// Obsidian-violet Terminal squircle with `>_` prompt motif.
    Terminal,
    /// Iconic two-tone macOS Finder face squircle (`Files`).
    Files,
    /// Free-standing RustyOS folder (for Desktop & directories).
    Folder,
    /// Silver frosted App Overview / Launcher 3x3 grid squircle.
    Overview,
    /// White & azure Compass Browser squircle (`RUSTYOS-050`).
    Browser,
    /// Warm amber-headed ruled Notes squircle (`RUSTYOS-050`).
    Notes,
    /// Brushed silver metallic gear Settings squircle (`RUSTYOS-050`).
    Settings,
    /// Free-standing stacked translucent blue Downloads folder (`RUSTYOS-050`).
    Downloads,
    /// Free-standing tapered frosted silver Trash wastebasket (`RUSTYOS-050`).
    Trash,
    /// Free-standing frosted silver Trash wastebasket filled with crumpled paper (`RUSTYOS-083`).
    TrashFull,
    /// Original amethyst-glass RustyOS crest squircle for system notifications (`RUSTYOS-051`).
    RustySystem,
    /// RustyOS azure Software Update squircle (`RUSTYOS-077`).
    Updater,
    /// Frosted jewel-tone window card squircle for windows lacking an XDG PNG or SHM icon.
    WindowFallback {
        /// Deterministic color bucket derived from the application ID.
        hue_seed: u8,
        /// Highlight tint when the window/group is active.
        active: bool,
    },
}

const ICON_CACHE_BUFFER_PRUNE_THRESHOLD: usize = 256;
const ICON_CACHE_SHM_PRUNE_THRESHOLD: usize = 128;
const ICON_CACHE_MAX_AGE_FRAMES: u64 = 1800;

#[derive(Debug)]
struct AgedIconEntry<T> {
    value: T,
    last_frame: u64,
}

fn prune_aged_icon_map<V>(
    map: &mut HashMap<IconCacheKey, AgedIconEntry<V>>,
    frame: u64,
    threshold: usize,
    max_age: u64,
) {
    if map.len() > threshold
        && (map.len() > threshold.saturating_mul(2) || frame.is_multiple_of(64))
    {
        let cutoff = frame.saturating_sub(max_age);
        map.retain(|_, entry| entry.last_frame >= cutoff);
    }
}

/// CPU-decoded & Wayland SHM GPU-cached application & tray icon store (`RUSTYOS-033`, `RUSTYOS-047`).
#[derive(Debug, Default)]
pub struct IconCache {
    search_roots: Option<Vec<PathBuf>>,
    resolved_paths: HashMap<String, Option<PathBuf>>,
    buffers: HashMap<IconCacheKey, AgedIconEntry<Option<MemoryRenderBuffer>>>,
    shm_textures: HashMap<IconCacheKey, AgedIconEntry<Option<CachedShmTexture>>>,
    builtin_squircles: HashMap<(BuiltinAppIcon, u16, bool, u8), MemoryRenderBuffer>,
    frame_counter: u64,
}

impl IconCache {
    /// Creates an empty [`IconCache`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances the frame counter and prunes stale cached icon buffers and SHM textures (`RUSTYOS-119`).
    pub fn begin_frame(&mut self) {
        self.frame_counter = self.frame_counter.saturating_add(1);
        prune_aged_icon_map(
            &mut self.buffers,
            self.frame_counter,
            ICON_CACHE_BUFFER_PRUNE_THRESHOLD,
            ICON_CACHE_MAX_AGE_FRAMES,
        );
        prune_aged_icon_map(
            &mut self.shm_textures,
            self.frame_counter,
            ICON_CACHE_SHM_PRUNE_THRESHOLD,
            ICON_CACHE_MAX_AGE_FRAMES,
        );
        if self.builtin_squircles.len() > 128 {
            self.builtin_squircles.clear();
        }
    }

    #[cfg(test)]
    pub(crate) fn cached_buffer_count(&self) -> usize {
        self.buffers.len()
    }

    /// Returns `true` if any window icon source (`xdg_toplevel_icon_v1` SHM/name or XDG fallback)
    /// resolves for `window_key` at `size_px`.
    pub fn has_window_icon(
        &mut self,
        renderer: &mut GlesRenderer,
        window_key: &str,
        toplevel_icon: Option<&ShmToplevelIcon>,
        fallback_icon_name: Option<&str>,
        size_px: i32,
    ) -> bool {
        let Ok(size_u16) = u16::try_from(size_px) else {
            return false;
        };
        if size_px <= 0 {
            return false;
        }
        if let Some(icon) = toplevel_icon {
            if self
                .ensure_shm_texture(renderer, window_key, icon, size_u16)
                .is_some()
            {
                return true;
            }
            if icon
                .icon_name
                .as_deref()
                .is_some_and(|name| self.has_named_icon(name, size_px))
            {
                return true;
            }
        }
        fallback_icon_name.is_some_and(|name| self.has_named_icon(name, size_px))
    }

    /// Resolves a window icon element from `xdg_toplevel_icon_v1` SHM/name or XDG fallback.
    pub fn window_icon_element(
        &mut self,
        renderer: &mut GlesRenderer,
        window_key: &str,
        toplevel_icon: Option<&ShmToplevelIcon>,
        fallback_icon_name: Option<&str>,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<ResolvedIconElement> {
        if let Some(icon) = toplevel_icon {
            if let Some(tex_el) =
                self.icon_element_from_shm(renderer, window_key, icon, size_px, loc)
            {
                return Some(ResolvedIconElement::Texture(tex_el));
            }
            if let Some(name) = icon.icon_name.as_deref()
                && let Some(mem_el) = self.icon_element_by_name(renderer, name, size_px, loc)
            {
                return Some(ResolvedIconElement::Memory(Box::new(mem_el)));
            }
        }
        fallback_icon_name
            .and_then(|name| self.icon_element_by_name(renderer, name, size_px, loc))
            .map(|el| ResolvedIconElement::Memory(Box::new(el)))
    }

    /// Renders a GPU-cached `xdg_toplevel_icon_v1` SHM icon fitted into `size_px x size_px`.
    pub fn icon_element_from_shm(
        &mut self,
        renderer: &mut GlesRenderer,
        window_key: &str,
        shm_icon: &ShmToplevelIcon,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<TextureRenderElement<GlesTexture>> {
        let size_u16 = u16::try_from(size_px).ok()?;
        if size_px <= 0 {
            return None;
        }
        let cached = self.ensure_shm_texture(renderer, window_key, shm_icon, size_u16)?;
        let (fitted_loc, fitted_size) =
            fit_icon_rect(loc, size_px, cached.src_size.w, cached.src_size.h);
        Some(TextureRenderElement::from_texture_buffer(
            fitted_loc.to_f64().to_physical(1.0),
            &cached.buffer,
            None,
            None,
            Some(fitted_size),
            Kind::Unspecified,
        ))
    }

    fn ensure_shm_texture(
        &mut self,
        renderer: &mut GlesRenderer,
        window_key: &str,
        shm_icon: &ShmToplevelIcon,
        size_u16: u16,
    ) -> Option<&CachedShmTexture> {
        if shm_icon.buffers.is_empty() {
            return None;
        }
        let source_key = format!("shm_icon:{window_key}");
        let key = IconCacheKey {
            source_key: source_key.clone(),
            size_px: size_u16,
            fingerprint: shm_icon.revision,
        };
        let frame = self.frame_counter;
        if !self.shm_textures.contains_key(&key) {
            self.shm_textures
                .retain(|k, _| k.source_key != source_key || k.fingerprint == shm_icon.revision);
            let target_logical = i32::from(size_u16);
            let candidate_dims: Vec<(i32, i32, i32)> = shm_icon
                .buffers
                .iter()
                .map(|(buf, scale)| {
                    if buf.is_alive()
                        && let Some(dim) = buffer_dimensions(buf)
                    {
                        (dim.w, dim.h, *scale)
                    } else {
                        (0, 0, *scale)
                    }
                })
                .collect();
            let imported =
                select_best_shm_icon_index(&candidate_dims, target_logical).and_then(|idx| {
                    let (buf, scale) = &shm_icon.buffers[idx];
                    let (w, h, _) = candidate_dims[idx];
                    let src_size = Size::from((w, h));
                    let damage = [Rectangle::from_size(src_size)];
                    let tex = renderer.import_shm_buffer(buf, None, &damage).ok()?;
                    let buffer = TextureBuffer::from_texture(
                        renderer,
                        tex,
                        (*scale).max(1),
                        Transform::Normal,
                        None,
                    );
                    Some(CachedShmTexture { buffer, src_size })
                });
            self.shm_textures.insert(
                key.clone(),
                AgedIconEntry {
                    value: imported,
                    last_frame: frame,
                },
            );
        }
        let entry = self.shm_textures.get_mut(&key)?;
        entry.last_frame = frame;
        entry.value.as_ref()
    }

    /// Returns `true` if `icon_name` resolves to a decodable PNG icon cached/loadable at `size_px`.
    #[must_use]
    pub fn has_named_icon(&mut self, icon_name: &str, size_px: i32) -> bool {
        let Ok(size_u16) = u16::try_from(size_px) else {
            return false;
        };
        if icon_name.trim().is_empty() || size_px <= 0 {
            return false;
        }
        self.ensure_named_buffer(icon_name, size_u16).is_some()
    }

    /// Renders a GPU-cached XDG / filesystem PNG icon named `icon_name` scaled to `size_px x size_px`.
    pub fn icon_element_by_name(
        &mut self,
        renderer: &mut GlesRenderer,
        icon_name: &str,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let size_u16 = u16::try_from(size_px).ok()?;
        if icon_name.trim().is_empty() || size_px <= 0 {
            return None;
        }
        let buf = self.ensure_named_buffer(icon_name, size_u16)?;
        element_from_icon_buffer(renderer, buf, size_px, loc)
    }

    /// Renders a GPU-cached D-Bus SNI [`TrayPixmap`] scaled to `size_px x size_px`.
    pub fn icon_element_from_pixmap(
        &mut self,
        renderer: &mut GlesRenderer,
        sni_key: &str,
        pixmap: &TrayPixmap,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let size_u16 = u16::try_from(size_px).ok()?;
        if size_px <= 0 {
            return None;
        }
        let frame = self.frame_counter;
        let key = IconCacheKey {
            source_key: format!("pixmap:{sni_key}"),
            size_px: size_u16,
            fingerprint: pixmap.fingerprint,
        };
        let entry = self.buffers.entry(key).or_insert_with(|| {
            let dst = usize::from(size_u16);
            let value = argb32_be_to_premul_bgra(pixmap.width, pixmap.height, &pixmap.argb_be, dst)
                .map(|bgra| upload_bgra_buffer(size_px, &bgra));
            AgedIconEntry {
                value,
                last_frame: frame,
            }
        });
        entry.last_frame = frame;
        let buf = entry.value.as_ref()?;
        element_from_icon_buffer(renderer, buf, size_px, loc)
    }

    /// Renders a circular contact avatar pixmap (`RUSTYOS-173`).
    #[allow(clippy::too_many_arguments)]
    pub fn avatar_element(
        &mut self,
        renderer: &mut GlesRenderer,
        avatar_key: &str,
        rgba: &[u8],
        src_w: u32,
        src_h: u32,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let size_u16 = u16::try_from(size_px).ok()?;
        if size_px <= 0 || rgba.is_empty() || src_w == 0 || src_h == 0 {
            return None;
        }
        let frame = self.frame_counter;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(rgba, &mut hasher);
        let fp = std::hash::Hasher::finish(&hasher);
        let key = IconCacheKey {
            source_key: format!("avatar:{avatar_key}"),
            size_px: size_u16,
            fingerprint: fp,
        };
        let entry = self.buffers.entry(key).or_insert_with(|| {
            let scale = crate::surfaces::GLYPH_BUFFER_SCALE;
            let dev_px = i32::from(size_u16) * scale;
            let value = (|| {
                let dev_usize = usize::try_from(dev_px).ok()?;
                let sw = src_w as usize;
                let sh = src_h as usize;
                let (crop_w, crop_h, crop_buf);
                let crop_slice: &[u8] = if sw == sh {
                    crop_w = sw;
                    crop_h = sh;
                    rgba
                } else {
                    let side = sw.min(sh);
                    let x0 = (sw - side) / 2;
                    let y0 = (sh - side) / 2;
                    let mut cropped =
                        Vec::with_capacity(side.saturating_mul(side).saturating_mul(4));
                    for y in 0..side {
                        let start = ((y0 + y) * sw + x0) * 4;
                        let end = start + side * 4;
                        if end <= rgba.len() {
                            cropped.extend_from_slice(&rgba[start..end]);
                        }
                    }
                    crop_w = side;
                    crop_h = side;
                    crop_buf = cropped;
                    &crop_buf
                };
                #[allow(clippy::cast_possible_truncation)]
                let scaled_bgra = scale_rgba_to_premul_bgra(crop_w, crop_h, crop_slice, dev_usize)?;
                // Apply antialiased circular mask
                #[allow(clippy::cast_precision_loss)]
                let radius = (dev_px as f32) * 0.5;
                let cx = radius;
                let cy = radius;
                let mut masked = scaled_bgra;
                for y in 0..dev_usize {
                    for x in 0..dev_usize {
                        #[allow(clippy::cast_precision_loss)]
                        let dx = x as f32 + 0.5 - cx;
                        #[allow(clippy::cast_precision_loss)]
                        let dy = y as f32 + 0.5 - cy;
                        let dist = dx.hypot(dy);
                        let idx = (y * dev_usize + x) * 4;
                        if dist >= radius {
                            masked[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                        } else if dist > radius - 1.0 {
                            let alpha = (radius - dist).clamp(0.0, 1.0);
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let b = (f32::from(masked[idx]) * alpha) as u8;
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let g = (f32::from(masked[idx + 1]) * alpha) as u8;
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let r = (f32::from(masked[idx + 2]) * alpha) as u8;
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let a = (f32::from(masked[idx + 3]) * alpha) as u8;
                            masked[idx..idx + 4].copy_from_slice(&[b, g, r, a]);
                        }
                    }
                }
                Some(upload_bgra_buffer_scaled(dev_px, scale, &masked))
            })();
            AgedIconEntry {
                value,
                last_frame: frame,
            }
        });
        entry.last_frame = frame;
        let buf = entry.value.as_ref()?;
        element_from_icon_buffer(renderer, buf, size_px, loc)
    }

    fn ensure_named_buffer(
        &mut self,
        icon_name: &str,
        size_u16: u16,
    ) -> Option<&MemoryRenderBuffer> {
        let frame = self.frame_counter;
        let key = IconCacheKey {
            source_key: format!("name:{icon_name}"),
            size_px: size_u16,
            fingerprint: 0,
        };
        if !self.buffers.contains_key(&key) {
            let path = self.resolve_icon_path(icon_name);
            let decoded = path.and_then(|p| load_png_as_bgra_scaled(&p, usize::from(size_u16)));
            let buf = decoded.and_then(|(dev_size, scale, bgra)| {
                let dev_px = i32::try_from(dev_size).ok()?;
                Some(upload_bgra_buffer_scaled(dev_px, scale, &bgra))
            });
            self.buffers.insert(
                key.clone(),
                AgedIconEntry {
                    value: buf,
                    last_frame: frame,
                },
            );
        }
        let entry = self.buffers.get_mut(&key)?;
        entry.last_frame = frame;
        entry.value.as_ref()
    }

    fn resolve_icon_path(&mut self, icon_name: &str) -> Option<PathBuf> {
        let normalized = icon_name.trim();
        if normalized.is_empty() {
            return None;
        }
        if let Some(cached) = self.resolved_paths.get(normalized) {
            return cached.clone();
        }
        let roots = self
            .search_roots
            .get_or_insert_with(discover_icon_search_roots);
        let resolved = find_icon_png_in_roots(normalized, roots);
        self.resolved_paths
            .insert(normalized.to_owned(), resolved.clone());
        resolved
    }

    /// Renders a GPU-cached XDG PNG icon scaled & framed inside a macOS 27 squircle (`RUSTYOS-114`).
    pub fn framed_xdg_squircle_icon_element(
        &mut self,
        renderer: &mut GlesRenderer,
        icon_name: &str,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let raster_px = dock_raster_bucket_px(size_px);
        let size_u16 = u16::try_from(raster_px).ok()?;
        if icon_name.trim().is_empty() || size_px <= 0 {
            return None;
        }
        let is_dark = matches!(
            crate::shell_render::active_appearance(),
            crate::config::AppearanceMode::Dark
        );
        let accent_idx = crate::shell_render::active_accent_index();
        let fp = (u64::from(is_dark) << 8) | u64::from(accent_idx);
        let frame = self.frame_counter;
        let key = IconCacheKey {
            source_key: format!("framed_name:{icon_name}"),
            size_px: size_u16,
            fingerprint: fp,
        };
        if let Some(entry) = self.buffers.get_mut(&key) {
            entry.last_frame = frame;
            let buf = entry.value.as_ref()?;
            return element_from_icon_buffer(renderer, buf, size_px, loc);
        }
        let buf = self.resolve_icon_path(icon_name).and_then(|path| {
            let meta = fs::metadata(&path).ok()?;
            if usize::try_from(meta.len()).unwrap_or(usize::MAX) > MAX_PNG_FILE_BYTES {
                return None;
            }
            let bytes = fs::read(path).ok()?;
            let (w, h, rgba) = decode_png_rgba(&bytes)?;
            let scale = crate::surfaces::GLYPH_BUFFER_SCALE;
            let dev_px = i32::from(size_u16) * scale;
            let dev_usize = usize::try_from(dev_px).ok()?;
            let (_, inner_dim) = crate::icon_art::squircle_logo_box(dev_usize);
            let logo_bgra = scale_rgba_to_premul_bgra(w, h, &rgba, inner_dim)?;
            let bgra = crate::icon_art::composite_xdg_in_squircle(
                dev_px, &logo_bgra, inner_dim, is_dark, accent_idx,
            );
            Some(upload_bgra_buffer_scaled(dev_px, scale, &bgra))
        });
        self.buffers.insert(
            key.clone(),
            AgedIconEntry {
                value: buf,
                last_frame: frame,
            },
        );
        let buf = self.buffers.get(&key)?.value.as_ref()?;
        element_from_icon_buffer(renderer, buf, size_px, loc)
    }

    /// Renders a GPU-cached procedural RustyOS squircle icon styled with active theme & accent (`RUSTYOS-048`/`114`).
    pub fn builtin_squircle_icon_element(
        &mut self,
        renderer: &mut GlesRenderer,
        icon: BuiltinAppIcon,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        if size_px <= 0 {
            return None;
        }
        let raster_px = dock_raster_bucket_px(size_px);
        let size_u16 = u16::try_from(raster_px).ok()?;
        let is_dark = matches!(
            crate::shell_render::active_appearance(),
            crate::config::AppearanceMode::Dark
        );
        let accent_idx = crate::shell_render::active_accent_index();
        let buf = self
            .builtin_squircles
            .entry((icon, size_u16, is_dark, accent_idx))
            .or_insert_with(|| {
                rasterize_builtin_squircle_buffer(icon, raster_px, is_dark, accent_idx)
            });
        element_from_icon_buffer(renderer, buf, size_px, loc)
    }
}

/// Maps transient magnified icon sizes (`> 58 px`) to a single `@2x` `96 px` (`192 dev px`)
/// GPU-cached raster bucket so smooth cursor sweeps never re-rasterize 30 intermediate pixmaps.
#[must_use]
const fn dock_raster_bucket_px(size_px: i32) -> i32 {
    if size_px > 58 && size_px <= 112 {
        96
    } else {
        size_px
    }
}

/// Selects the best matching `(width, height, scale)` icon buffer index for `target_logical_size`.
#[must_use]
pub(crate) fn select_best_shm_icon_index(
    dims: &[(i32, i32, i32)],
    target_logical_size: i32,
) -> Option<usize> {
    let target = target_logical_size.max(1);
    dims.iter()
        .enumerate()
        .filter(|&(_, &(w, h, _))| w > 0 && h > 0)
        .min_by_key(|&(_, &(w, h, raw_scale))| {
            let scale = raw_scale.max(1);
            let logical_extent = (w.max(h) / scale).max(1);
            let undersized_penalty = i32::from(logical_extent < target);
            let delta = (logical_extent - target).abs();
            // Prefer buffers >= target size, closest to target, then higher HiDPI scale density.
            (undersized_penalty, delta, -scale)
        })
        .map(|(idx, _)| idx)
}

/// Fits a `src_w x src_h` icon cleanly into a square `size_px x size_px` cell at `loc`.
#[must_use]
pub(crate) fn fit_icon_rect(
    loc: Point<i32, Logical>,
    size_px: i32,
    src_w: i32,
    src_h: i32,
) -> (Point<i32, Logical>, Size<i32, Logical>) {
    let box_size = size_px.max(1);
    if src_w <= 0 || src_h <= 0 || src_w == src_h {
        return (loc, Size::from((box_size, box_size)));
    }
    if src_w > src_h {
        let h = i32::try_from((i64::from(box_size) * i64::from(src_h)) / i64::from(src_w))
            .unwrap_or(box_size)
            .clamp(1, box_size);
        (
            Point::from((loc.x, loc.y + (box_size - h) / 2)),
            Size::from((box_size, h)),
        )
    } else {
        let w = i32::try_from((i64::from(box_size) * i64::from(src_w)) / i64::from(src_h))
            .unwrap_or(box_size)
            .clamp(1, box_size);
        (
            Point::from((loc.x + (box_size - w) / 2, loc.y)),
            Size::from((w, box_size)),
        )
    }
}

fn element_from_icon_buffer(
    renderer: &mut GlesRenderer,
    buf: &MemoryRenderBuffer,
    size_px: i32,
    loc: Point<i32, Logical>,
) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        loc.to_f64().to_physical(1.0),
        buf,
        None,
        None,
        Some(Size::from((size_px, size_px))),
        Kind::Unspecified,
    )
    .ok()
}

fn upload_bgra_buffer(size_px: i32, bgra: &[u8]) -> MemoryRenderBuffer {
    upload_bgra_buffer_scaled(size_px, 1, bgra)
}

fn upload_bgra_buffer_scaled(dev_px: i32, scale: i32, bgra: &[u8]) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        bgra,
        Fourcc::Argb8888,
        (dev_px, dev_px),
        scale,
        Transform::Normal,
        None,
    )
}

fn load_png_as_bgra_scaled(path: &Path, logical_size: usize) -> Option<(usize, i32, Vec<u8>)> {
    let meta = fs::metadata(path).ok()?;
    if usize::try_from(meta.len()).unwrap_or(usize::MAX) > MAX_PNG_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let (w, h, rgba) = decode_png_rgba(&bytes)?;
    let scale = if w >= logical_size.saturating_mul(2) && h >= logical_size.saturating_mul(2) {
        crate::surfaces::GLYPH_BUFFER_SCALE
    } else {
        1
    };
    let dev_size = logical_size.checked_mul(usize::try_from(scale).ok()?)?;
    let bgra = scale_rgba_to_premul_bgra(w, h, &rgba, dev_size)?;
    Some((dev_size, scale, bgra))
}

fn discover_icon_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        if !xdg_data_home.trim().is_empty() {
            roots.push(PathBuf::from(&xdg_data_home).join("icons"));
            roots.push(PathBuf::from(xdg_data_home).join("pixmaps"));
        }
    } else if let Ok(home) = env::var("HOME")
        && !home.trim().is_empty()
    {
        roots.push(PathBuf::from(&home).join(".local/share/icons"));
        roots.push(PathBuf::from(home).join(".local/share/pixmaps"));
    }
    roots.push(PathBuf::from("/usr/local/share/icons"));
    roots.push(PathBuf::from("/usr/share/icons"));
    roots.push(PathBuf::from("/usr/share/pixmaps"));
    roots
}

fn find_icon_png_in_roots(icon_name: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let direct = Path::new(icon_name);
    if direct.is_absolute() && direct.is_file() {
        let is_png = direct
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("png"));
        return is_png.then(|| direct.to_path_buf());
    }

    let stem = icon_name
        .strip_suffix(".png")
        .or_else(|| icon_name.strip_suffix(".PNG"))
        .unwrap_or(icon_name);
    let file_name = format!("{stem}.png");
    let lower_file_name = format!("{}.png", stem.to_ascii_lowercase());

    let themes = [
        "hicolor",
        "Papirus",
        "Papirus-Dark",
        "Adwaita",
        "AdwaitaLegacy",
        "Qogir",
        "Qogir-Dark",
        "breeze",
        "breeze-dark",
        "Yaru",
        "Numix",
    ];
    let size_dirs = [
        "128x128", "256x256", "96x96", "64x64", "48x48", "32x32", "24x24", "16x16", "128", "64",
        "48", "32",
    ];
    let cats = [
        "apps",
        "status",
        "categories",
        "devices",
        "actions",
        "places",
        "mimetypes",
        "legacy",
    ];

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        // Check flat pixmaps root (`root/<name>.png`)
        for candidate_name in [&file_name, &lower_file_name] {
            let flat = root.join(candidate_name);
            if flat.is_file() {
                return Some(flat);
            }
        }
        for theme in themes {
            let theme_dir = root.join(theme);
            if !theme_dir.is_dir() {
                continue;
            }
            for size in size_dirs {
                for cat in cats {
                    for candidate_name in [&file_name, &lower_file_name] {
                        let candidate = theme_dir.join(size).join(cat).join(candidate_name);
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                        let alt = theme_dir.join(cat).join(size).join(candidate_name);
                        if alt.is_file() {
                            return Some(alt);
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
fn sample_builtin_squircle_pixel(px: i32, py: i32, size_px: i32, icon: BuiltinAppIcon) -> [u8; 4] {
    crate::icon_art::sample_builtin_squircle_pixel(px, py, size_px, icon, false, 0)
}

/// Rasterizes a procedural squircle icon into a `2x` `HiDPI` buffer (`scale = 2`, `RUSTYOS-111`);
/// the logical size stays `size_px`.
fn rasterize_builtin_squircle_buffer(
    icon: BuiltinAppIcon,
    size_px: i32,
    is_dark: bool,
    accent_idx: u8,
) -> MemoryRenderBuffer {
    let scale = crate::surfaces::GLYPH_BUFFER_SCALE;
    let dev_px = size_px * scale;
    let dim = usize::try_from(dev_px).unwrap_or(1).max(1);
    let mut bgra = vec![0_u8; dim * dim * 4];
    for py in 0..dev_px {
        for px in 0..dev_px {
            let row = usize::try_from(py).unwrap_or(0);
            let col = usize::try_from(px).unwrap_or(0);
            let idx = (row * dim + col) * 4;
            if idx + 4 <= bgra.len() {
                let p = crate::icon_art::sample_builtin_squircle_pixel(
                    px, py, dev_px, icon, is_dark, accent_idx,
                );
                bgra[idx..idx + 4].copy_from_slice(&p);
            }
        }
    }
    upload_bgra_buffer_scaled(dev_px, scale, &bgra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_png_decode_and_downscale_to_premul_bgra() -> Result<(), String> {
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
        ];
        let png = crate::screenshot::encode_png_rgba(2, 2, &rgba)?;
        let (w, h, decoded) =
            decode_png_rgba(&png).ok_or_else(|| "PNG decoding failed".to_owned())?;
        assert_eq!((w, h), (2, 2));
        assert_eq!(decoded, rgba);

        let bgra = scale_rgba_to_premul_bgra(w, h, &decoded, 2)
            .ok_or_else(|| "BGRA scaling failed".to_owned())?;
        assert_eq!(bgra.len(), 16);
        // First pixel (255, 0, 0, 255) -> BGRA [0, 0, 255, 255]
        assert_eq!(&bgra[0..4], &[0, 0, 255, 255]);
        // Fourth pixel (255, 255, 255, 128) -> premultiplied [128, 128, 128, 128]
        assert_eq!(&bgra[12..16], &[128, 128, 128, 128]);
        Ok(())
    }

    #[test]
    fn converts_sni_argb32_pixmap_and_validates_dimensions() -> Result<(), String> {
        // 1x1 pixel ARGB big-endian: alpha=0x80, r=0xFF, g=0x00, b=0x00
        let pix = TrayPixmap::from_argb_be(1, 1, vec![0x80, 0xFF, 0x00, 0x00])
            .ok_or_else(|| "Valid 1x1 pixmap should succeed".to_owned())?;
        let bgra = argb32_be_to_premul_bgra(pix.width, pix.height, &pix.argb_be, 2)
            .ok_or_else(|| "Pixmap conversion should succeed".to_owned())?;
        assert_eq!(bgra.len(), 16);
        // Premultiplied BGRA of (r=255, g=0, b=0, a=128): B=0, G=0, R=128, A=128
        assert_eq!(&bgra[0..4], &[0, 0, 128, 128]);

        assert!(TrayPixmap::from_argb_be(0, 10, Vec::new()).is_none());
        assert!(TrayPixmap::from_argb_be(2, 2, vec![0; 4]).is_none());
        Ok(())
    }

    #[test]
    fn resolves_png_icons_in_xdg_theme_hierarchy() -> Result<(), String> {
        let base = std::env::temp_dir().join(format!("rustyos-icons-test-{}", std::process::id()));
        let app_dir = base.join("hicolor/48x48/apps");
        fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
        let icon_file = app_dir.join("test-app.png");
        let png = crate::screenshot::encode_png_rgba(1, 1, &[10, 20, 30, 255])?;
        fs::write(&icon_file, png).map_err(|e| e.to_string())?;

        let resolved = find_icon_png_in_roots("test-app", std::slice::from_ref(&base));
        assert_eq!(resolved, Some(icon_file));
        assert_eq!(
            find_icon_png_in_roots("nonexistent-app", std::slice::from_ref(&base)),
            None
        );

        let _ = fs::remove_dir_all(&base);
        Ok(())
    }

    #[test]
    fn center_crop_wallpaper_cover_preserves_opaque_center_color() -> Result<(), String> {
        // 4x2 source: left column red, middle two columns blue, right column green.
        // Center-cropping into a 2x2 square should pick the central blue pixels.
        let mut rgba = Vec::new();
        for _y in 0..2 {
            rgba.extend_from_slice(&[255, 0, 0, 255]);
            rgba.extend_from_slice(&[0, 0, 255, 255]);
            rgba.extend_from_slice(&[0, 0, 255, 255]);
            rgba.extend_from_slice(&[0, 255, 0, 255]);
        }
        let bgra = scale_cover_rgba_to_premul_bgra(4, 2, &rgba, 2, 2)
            .ok_or_else(|| "cover scale must succeed".to_owned())?;
        assert_eq!(bgra.len(), 16);
        assert_eq!(&bgra[0..4], &[255, 0, 0, 255]);
        Ok(())
    }

    #[test]
    fn select_best_shm_icon_index_prefers_exact_or_larger_hidpi_candidates() {
        let dims = [(16, 16, 1), (48, 48, 2), (48, 48, 1), (0, 0, 1)];
        // Target 24px -> (48, 48, 2) has logical extent 24 with scale=2.
        assert_eq!(select_best_shm_icon_index(&dims, 24), Some(1));
        // Target 48px -> (48, 48, 1) has logical extent 48.
        assert_eq!(select_best_shm_icon_index(&dims, 48), Some(2));
        // Target 64px -> none >= 64, picks closest (48, 48, 1) or higher-scale.
        assert_eq!(select_best_shm_icon_index(&dims, 64), Some(2));
        // All invalid/dead entries return None.
        assert_eq!(
            select_best_shm_icon_index(&[(0, 0, 1), (-4, 32, 1)], 48),
            None
        );
        assert_eq!(select_best_shm_icon_index(&[], 48), None);
    }

    #[test]
    fn fit_icon_rect_preserves_aspect_and_centers_non_square_icons() {
        let loc = Point::from((10, 20));
        assert_eq!(
            fit_icon_rect(loc, 48, 64, 64),
            (Point::from((10, 20)), Size::from((48, 48)))
        );
        assert_eq!(
            fit_icon_rect(loc, 48, 64, 32),
            (Point::from((10, 32)), Size::from((48, 24)))
        );
        assert_eq!(
            fit_icon_rect(loc, 48, 32, 64),
            (Point::from((22, 20)), Size::from((24, 48)))
        );
        assert_eq!(
            fit_icon_rect(loc, 48, 0, -5),
            (Point::from((10, 20)), Size::from((48, 48)))
        );
        assert_eq!(
            ShmToplevelIcon::default(),
            ShmToplevelIcon {
                revision: 0,
                icon_name: None,
                buffers: Vec::new(),
            }
        );
    }

    #[test]
    fn resolves_adwaita_legacy_places_and_generates_distinct_squircle_icons() -> Result<(), String>
    {
        let base =
            std::env::temp_dir().join(format!("rustyos-icons-legacy-test-{}", std::process::id()));
        let legacy_dir = base.join("AdwaitaLegacy/48x48/places");
        fs::create_dir_all(&legacy_dir).map_err(|e| e.to_string())?;
        let icon_file = legacy_dir.join("system-file-manager.png");
        let png = crate::screenshot::encode_png_rgba(1, 1, &[56, 189, 248, 255])?;
        fs::write(&icon_file, png).map_err(|e| e.to_string())?;

        let resolved = find_icon_png_in_roots("system-file-manager", std::slice::from_ref(&base));
        assert_eq!(resolved, Some(icon_file));
        let _ = fs::remove_dir_all(&base);

        // Verify all procedural macOS Dock icons have clean transparent corners & distinct centers
        let term_center = sample_builtin_squircle_pixel(24, 24, 48, BuiltinAppIcon::Terminal);
        let files_center = sample_builtin_squircle_pixel(24, 30, 48, BuiltinAppIcon::Files);
        let overview_center = sample_builtin_squircle_pixel(24, 24, 48, BuiltinAppIcon::Overview);
        let browser_center = sample_builtin_squircle_pixel(24, 24, 48, BuiltinAppIcon::Browser);
        let notes_center = sample_builtin_squircle_pixel(24, 24, 48, BuiltinAppIcon::Notes);
        let settings_center = sample_builtin_squircle_pixel(24, 24, 48, BuiltinAppIcon::Settings);
        let rusty_center = sample_builtin_squircle_pixel(24, 24, 48, BuiltinAppIcon::RustySystem);
        let updater_center = sample_builtin_squircle_pixel(24, 24, 48, BuiltinAppIcon::Updater);
        let downloads_center = sample_builtin_squircle_pixel(24, 30, 48, BuiltinAppIcon::Downloads);
        let trash_center = sample_builtin_squircle_pixel(24, 24, 48, BuiltinAppIcon::Trash);
        let fallback_center = sample_builtin_squircle_pixel(
            24,
            24,
            48,
            BuiltinAppIcon::WindowFallback {
                hue_seed: 1,
                active: true,
            },
        );
        for px in [
            term_center,
            files_center,
            overview_center,
            browser_center,
            notes_center,
            settings_center,
            rusty_center,
            updater_center,
            downloads_center,
            fallback_center,
        ] {
            assert_eq!(px[3], 255);
        }
        assert!(trash_center[3] >= 240);
        assert_ne!(term_center, files_center);
        assert_ne!(files_center, overview_center);
        assert_ne!(browser_center, notes_center);
        assert_ne!(settings_center, downloads_center);
        assert_ne!(rusty_center, overview_center);
        let rusty_bg = sample_builtin_squircle_pixel(10, 24, 48, BuiltinAppIcon::RustySystem);
        // Argb8888 premultiplied pixel order is `[B, G, R, A]`.
        assert!(
            rusty_bg[0] > rusty_bg[1] && rusty_bg[0] > rusty_bg[2],
            "RustySystem icon background must use RustyOS blue, got {rusty_bg:?}"
        );
        assert_ne!(updater_center, notes_center);
        assert_ne!(downloads_center, trash_center);

        for kind in [
            BuiltinAppIcon::Files,
            BuiltinAppIcon::Folder,
            BuiltinAppIcon::Browser,
            BuiltinAppIcon::RustySystem,
            BuiltinAppIcon::Updater,
            BuiltinAppIcon::Downloads,
            BuiltinAppIcon::Trash,
        ] {
            let corner = sample_builtin_squircle_pixel(0, 0, 48, kind);
            assert_eq!(corner[3], 0, "{kind:?} corner should stay transparent");
        }
        Ok(())
    }

    #[test]
    fn folder_accent_dark_and_xdg_squircle_framing_work() {
        // Accent index & Dark appearance dynamically recolor procedural folders & Finder/Apps shells.
        let folder_blue = crate::icon_art::sample_builtin_squircle_pixel(
            24,
            28,
            48,
            BuiltinAppIcon::Folder,
            false,
            0,
        );
        let folder_coral = crate::icon_art::sample_builtin_squircle_pixel(
            24,
            28,
            48,
            BuiltinAppIcon::Folder,
            false,
            1,
        );
        let overview_dark = crate::icon_art::sample_builtin_squircle_pixel(
            6,
            24,
            48,
            BuiltinAppIcon::Overview,
            true,
            0,
        );
        assert!(
            folder_blue[0] > folder_blue[2],
            "default folder must stay Sky Blue"
        );
        assert!(
            folder_coral[2] > folder_coral[0],
            "coral accent folder must turn Coral Red"
        );
        assert!(
            overview_dark[0] < 80 && overview_dark[1] < 80 && overview_dark[2] < 80,
            "dark Overview shell should use dark frosted graphite, got {overview_dark:?}"
        );

        let logo = vec![255_u8; 32 * 32 * 4];
        let framed = crate::icon_art::composite_xdg_in_squircle(48, &logo, 32, true, 0);
        assert_eq!(framed.len(), 48 * 48 * 4);
        assert_eq!(
            framed[3], 0,
            "framed XDG squircle corner must stay transparent"
        );
    }

    #[test]
    fn icon_cache_evicts_stale_entries_while_keeping_active_icons() {
        let mut cache = IconCache::new();
        let active_key = IconCacheKey {
            source_key: "name:active-app".to_owned(),
            size_px: 48,
            fingerprint: 0,
        };
        cache.buffers.insert(
            active_key.clone(),
            AgedIconEntry {
                value: None,
                last_frame: 0,
            },
        );
        for i in 0..270 {
            cache.buffers.insert(
                IconCacheKey {
                    source_key: format!("name:stale-app-{i}"),
                    size_px: 48,
                    fingerprint: 0,
                },
                AgedIconEntry {
                    value: None,
                    last_frame: 0,
                },
            );
        }
        assert_eq!(cache.cached_buffer_count(), 271);

        for f in 1..=1920 {
            cache.begin_frame();
            if f == 1500
                && let Some(entry) = cache.buffers.get_mut(&active_key)
            {
                entry.last_frame = cache.frame_counter;
            }
        }
        assert_eq!(cache.cached_buffer_count(), 1);
        assert!(cache.buffers.contains_key(&active_key));
    }
}
