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
    /// Sky-blue Files / Finder folder squircle.
    Files,
    /// Silver frosted App Overview / Launcher grid squircle.
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
    /// Original amethyst-glass RustyOS crest squircle for system notifications (`RUSTYOS-051`).
    RustySystem,
    /// Frosted jewel-tone window card squircle for windows lacking an XDG PNG or SHM icon.
    WindowFallback {
        /// Deterministic color bucket derived from the application ID.
        hue_seed: u8,
        /// Highlight tint when the window/group is active.
        active: bool,
    },
}

/// CPU-decoded & Wayland SHM GPU-cached application & tray icon store (`RUSTYOS-033`, `RUSTYOS-047`).
#[derive(Debug, Default)]
pub struct IconCache {
    search_roots: Option<Vec<PathBuf>>,
    resolved_paths: HashMap<String, Option<PathBuf>>,
    buffers: HashMap<IconCacheKey, Option<MemoryRenderBuffer>>,
    shm_textures: HashMap<IconCacheKey, Option<CachedShmTexture>>,
    builtin_squircles: HashMap<(BuiltinAppIcon, u16), MemoryRenderBuffer>,
}

impl IconCache {
    /// Creates an empty [`IconCache`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
            self.shm_textures.insert(key.clone(), imported);
        }
        self.shm_textures.get(&key).and_then(Option::as_ref)
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
        self.ensure_named_buffer(icon_name, size_u16);
        let key = IconCacheKey {
            source_key: format!("name:{icon_name}"),
            size_px: size_u16,
            fingerprint: 0,
        };
        let buf = self.buffers.get(&key)?.as_ref()?;
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
        let key = IconCacheKey {
            source_key: format!("pixmap:{sni_key}"),
            size_px: size_u16,
            fingerprint: pixmap.fingerprint,
        };
        let buf_opt = self.buffers.entry(key).or_insert_with(|| {
            let dst = usize::from(size_u16);
            let bgra = argb32_be_to_premul_bgra(pixmap.width, pixmap.height, &pixmap.argb_be, dst)?;
            Some(upload_bgra_buffer(size_px, &bgra))
        });
        let buf = buf_opt.as_ref()?;
        element_from_icon_buffer(renderer, buf, size_px, loc)
    }

    fn ensure_named_buffer(
        &mut self,
        icon_name: &str,
        size_u16: u16,
    ) -> Option<&MemoryRenderBuffer> {
        let key = IconCacheKey {
            source_key: format!("name:{icon_name}"),
            size_px: size_u16,
            fingerprint: 0,
        };
        if !self.buffers.contains_key(&key) {
            let path = self.resolve_icon_path(icon_name);
            let decoded = path.and_then(|p| load_png_as_bgra(&p, usize::from(size_u16)));
            let buf = decoded.map(|bgra| upload_bgra_buffer(i32::from(size_u16), &bgra));
            self.buffers.insert(key.clone(), buf);
        }
        self.buffers.get(&key).and_then(Option::as_ref)
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

    /// Renders a GPU-cached procedural macOS 27 squircle icon at `size_px x size_px` (`RUSTYOS-048`).
    pub fn builtin_squircle_icon_element(
        &mut self,
        renderer: &mut GlesRenderer,
        icon: BuiltinAppIcon,
        size_px: i32,
        loc: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        let size_u16 = u16::try_from(size_px).ok()?;
        if size_px <= 0 {
            return None;
        }
        let buf = self
            .builtin_squircles
            .entry((icon, size_u16))
            .or_insert_with(|| rasterize_builtin_squircle_buffer(icon, size_px));
        element_from_icon_buffer(renderer, buf, size_px, loc)
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
        Some(Rectangle::<f64, Logical>::from_size(Size::from((
            f64::from(size_px),
            f64::from(size_px),
        )))),
        Some(Size::from((size_px, size_px))),
        Kind::Unspecified,
    )
    .ok()
}

fn upload_bgra_buffer(size_px: i32, bgra: &[u8]) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        bgra,
        Fourcc::Argb8888,
        (size_px, size_px),
        1,
        Transform::Normal,
        None,
    )
}

fn load_png_as_bgra(path: &Path, dst_size: usize) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    if usize::try_from(meta.len()).unwrap_or(usize::MAX) > MAX_PNG_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let (w, h, rgba) = decode_png_rgba(&bytes)?;
    scale_rgba_to_premul_bgra(w, h, &rgba, dst_size)
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
        "48x48", "64x64", "32x32", "128x128", "24x24", "256x256", "96x96", "16x16", "48", "64",
        "32", "128",
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

fn squircle_sd(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32) -> f32 {
    let dx = ((u - cx).abs() / hw.max(0.01)).powf(4.8);
    let dy = ((v - cy).abs() / hh.max(0.01)).powf(4.8);
    ((dx + dy).powf(1.0 / 4.8) - 1.0) * hw.min(hh)
}

fn sq_sd_rounded_box(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (u - cx).abs() - (hw - r);
    let qy = (v - cy).abs() - (hh - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

fn sq_sd_segment(u: f32, v: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> f32 {
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

fn sq_sd_ellipse(u: f32, v: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let nx = (u - cx) / rx.max(0.01);
    let ny = (v - cy) / ry.max(0.01);
    (nx.hypot(ny) - 1.0) * rx.min(ry)
}

fn sq_cov(sd: f32, edge: f32) -> f32 {
    (0.5 - sd / edge.max(0.005)).clamp(0.0, 1.0)
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

fn lerp_rgb(top: [f32; 3], bot: [f32; 3], t: f32) -> [f32; 3] {
    let c = t.clamp(0.0, 1.0);
    [
        top[0] * (1.0 - c) + bot[0] * c,
        top[1] * (1.0 - c) + bot[1] * c,
        top[2] * (1.0 - c) + bot[2] * c,
    ]
}

fn sample_downloads_uv(u: f32, v: f32, edge: f32) -> [f32; 4] {
    let mut col = [0.0_f32; 4];

    // Stacked translucent back folder with top-left tab
    let tab = sq_sd_rounded_box(u, v, 0.31, 0.25, 0.14, 0.055, 0.03);
    let back = sq_sd_rounded_box(u, v, 0.50, 0.37, 0.34, 0.14, 0.045);
    let back_cov = sq_cov(tab.min(back), edge);
    if back_cov > 0.0 {
        let back_rgb = lerp_rgb([56.0, 140.0, 232.0], [34.0, 96.0, 198.0], (v - 0.19) * 3.2);
        col = blend_rgba(
            col,
            [back_rgb[0], back_rgb[1], back_rgb[2], back_cov * 0.90],
        );
    }

    // Middle folder stack layer
    let mid = sq_sd_rounded_box(u, v, 0.50, 0.45, 0.36, 0.15, 0.045);
    let mid_cov = sq_cov(mid, edge);
    if mid_cov > 0.0 {
        let mid_rgb = lerp_rgb([96.0, 184.0, 252.0], [42.0, 126.0, 226.0], (v - 0.30) * 3.0);
        col = blend_rgba(col, [mid_rgb[0], mid_rgb[1], mid_rgb[2], mid_cov * 0.94]);
    }

    // Glossy front Downloads folder card + downward arrow emblem
    let front = sq_sd_rounded_box(u, v, 0.50, 0.60, 0.38, 0.23, 0.05);
    let front_cov = sq_cov(front, edge);
    if front_cov > 0.0 {
        let front_rgb = lerp_rgb([96.0, 196.0, 255.0], [30.0, 108.0, 222.0], (v - 0.37) * 2.2);
        let top_lip = (1.0 - ((v - 0.37).max(0.0) / 0.035)).clamp(0.0, 1.0);
        let mut fc = lerp_rgb(front_rgb, [196.0, 234.0, 255.0], top_lip * 0.35);

        let stem = sq_sd_segment(u, v, 0.50, 0.48, 0.50, 0.68, 0.028);
        let arm_l = sq_sd_segment(u, v, 0.40, 0.60, 0.50, 0.70, 0.026);
        let arm_r = sq_sd_segment(u, v, 0.60, 0.60, 0.50, 0.70, 0.026);
        let arrow_cov = sq_cov(stem.min(arm_l).min(arm_r), edge);
        if arrow_cov > 0.0 {
            fc = lerp_rgb(fc, [232.0, 245.0, 255.0], arrow_cov * 0.85);
        }
        col = blend_rgba(col, [fc[0], fc[1], fc[2], front_cov]);
    }

    col
}

fn sample_trash_uv(u: f32, v: f32, edge: f32) -> [f32; 4] {
    let mut col = [0.0_f32; 4];
    let body_top = 0.22_f32;
    let body_bot = 0.84_f32;
    let t_y = ((v - body_top) / (body_bot - body_top)).clamp(0.0, 1.0);
    let hw = 0.31 - 0.075 * t_y;

    // Tapered cylindrical silver-glass wastebasket: rim ellipse (rx=0.31) sits tangent to wall hw(0)=0.31
    let dx = (u - 0.50).abs() - hw;
    let dy_top = body_top - v;
    let dy_bot = v - body_bot;
    let base_cap = sq_sd_ellipse(u, v, 0.50, body_bot, 0.235, 0.042);
    let top_rim = sq_sd_ellipse(u, v, 0.50, body_top, 0.31, 0.052);
    let wall_sd = dx.max(dy_top).max(dy_bot);
    let body_sd = wall_sd.min(base_cap).min(top_rim);
    let body_cov = sq_cov(body_sd, edge);

    if body_cov > 0.0 {
        let nx = ((u - 0.50) / hw.max(0.05)).clamp(-1.0, 1.0);
        // Brushed cylindrical reflection highlight strongest left-of-center + subtle rib flute
        let spec = (-(nx + 0.32).powi(2) / 0.14).exp() * 0.38;
        let edge_shade = (nx * nx) * 0.22;
        let rib = ((nx * 7.0 * std::f32::consts::PI).cos() * 0.035).clamp(-0.04, 0.04);
        let shade = (0.74 + spec - edge_shade - t_y * 0.10 + rib).clamp(0.38, 1.0);
        let red = 218.0 * shade;
        let green = 222.0 * shade;
        let blue = 230.0 * shade;
        col = [red, green, blue, body_cov * 0.96];
    }

    // Hollow recessed dark inner opening + bright metallic rim ellipse
    let hollow = sq_sd_ellipse(u, v, 0.50, body_top, 0.275, 0.034);
    let hollow_cov = sq_cov(hollow, edge);
    if hollow_cov > 0.0 {
        let h_rgb = lerp_rgb([64.0, 68.0, 78.0], [116.0, 122.0, 134.0], (u - 0.22) * 1.4);
        col = blend_rgba(col, [h_rgb[0], h_rgb[1], h_rgb[2], hollow_cov * 0.88]);
    }
    let rim_ring = (top_rim.abs() - 0.010).max(0.0) - 0.002;
    let rim_cov = sq_cov(rim_ring, edge);
    if rim_cov > 0.0 {
        col = blend_rgba(col, [242.0, 245.0, 250.0, rim_cov * 0.88]);
    }

    col
}

fn sample_squircle_uv(u: f32, v: f32, edge: f32, icon: BuiltinAppIcon) -> [f32; 4] {
    match icon {
        BuiltinAppIcon::Downloads => return sample_downloads_uv(u, v, edge),
        BuiltinAppIcon::Trash => return sample_trash_uv(u, v, edge),
        _ => {}
    }

    let shell_sd = squircle_sd(u, v, 0.50, 0.50, 0.455, 0.455);
    let shell_cov = sq_cov(shell_sd, edge);
    if shell_cov <= 0.001 {
        return [0.0; 4];
    }
    let grad_t = (v * 1.1 - 0.05).clamp(0.0, 1.0);
    let (top_rgb, bot_rgb) = match icon {
        BuiltinAppIcon::Terminal => ([48.0, 44.0, 60.0], [18.0, 16.0, 28.0]),
        BuiltinAppIcon::Files => ([90.0, 206.0, 255.0], [26.0, 114.0, 242.0]),
        BuiltinAppIcon::Overview => ([250.0, 250.0, 253.0], [222.0, 225.0, 236.0]),
        BuiltinAppIcon::Browser => ([250.0, 251.0, 255.0], [224.0, 229.0, 240.0]),
        BuiltinAppIcon::Notes => ([252.0, 251.0, 246.0], [235.0, 233.0, 226.0]),
        BuiltinAppIcon::Settings => ([178.0, 182.0, 190.0], [114.0, 118.0, 128.0]),
        BuiltinAppIcon::RustySystem => ([152.0, 102.0, 252.0], [76.0, 34.0, 154.0]),
        BuiltinAppIcon::Downloads | BuiltinAppIcon::Trash => ([0.0; 3], [0.0; 3]),
        BuiltinAppIcon::WindowFallback { hue_seed, active } => {
            let palettes: [([f32; 3], [f32; 3]); 4] = [
                ([139.0, 92.0, 246.0], [76.0, 29.0, 149.0]),
                ([45.0, 212.0, 191.0], [15.0, 118.0, 110.0]),
                ([244.0, 114.0, 182.0], [157.0, 23.0, 77.0]),
                ([96.0, 165.0, 250.0], [30.0, 64.0, 175.0]),
            ];
            let base = palettes[usize::from(hue_seed) % palettes.len()];
            if active {
                base
            } else {
                (
                    [base.0[0] * 0.85, base.0[1] * 0.85, base.0[2] * 0.85],
                    [base.1[0] * 0.85, base.1[1] * 0.85, base.1[2] * 0.85],
                )
            }
        }
    };
    let base_rgb = lerp_rgb(top_rgb, bot_rgb, grad_t);
    let mut col = [base_rgb[0], base_rgb[1], base_rgb[2], 1.0];

    col = paint_squircle_glyph(u, v, edge, icon, col);

    // Subtle macOS-style top specular lip only (no harsh white ring around the bottom/sides)
    if shell_sd > -0.022 && v < 0.52 {
        let top_weight = ((0.52 - v) / 0.48).clamp(0.0, 1.0);
        col = blend_rgba(col, [252.0, 250.0, 255.0, 0.26 * top_weight]);
    } else if v > 0.88 {
        let bot_shade = ((v - 0.88) / 0.12).clamp(0.0, 1.0) * 0.08;
        col = blend_rgba(col, [8.0, 6.0, 14.0, bot_shade]);
    }
    [col[0], col[1], col[2], col[3] * shell_cov]
}

fn paint_overview_glyph(u: f32, v: f32, edge: f32, mut col: [f32; 4]) -> [f32; 4] {
    let pill = sq_sd_rounded_box(u, v, 0.50, 0.245, 0.21, 0.038, 0.036);
    let pill_cov = sq_cov(pill, edge);
    if pill_cov > 0.0 {
        col = blend_rgba(col, [182.0, 186.0, 198.0, pill_cov * 0.72]);
    }
    let tile_colors: [[f32; 3]; 6] = [
        [56.0, 189.0, 248.0],
        [52.0, 211.0, 153.0],
        [248.0, 113.0, 113.0],
        [251.0, 191.0, 36.0],
        [167.0, 139.0, 250.0],
        [148.0, 163.0, 184.0],
    ];
    for (idx, &tile_color) in tile_colors.iter().enumerate() {
        let col_i = idx % 3;
        let row_i = idx / 3;
        #[allow(clippy::cast_precision_loss)]
        let cx = 0.28 + col_i as f32 * 0.22;
        #[allow(clippy::cast_precision_loss)]
        let cy = 0.44 + row_i as f32 * 0.23;
        let tile_sd = sq_sd_rounded_box(u, v, cx, cy, 0.075, 0.075, 0.025);
        let cov = sq_cov(tile_sd, edge);
        if cov > 0.0 {
            col = blend_rgba(col, [tile_color[0], tile_color[1], tile_color[2], cov]);
        }
    }
    col
}

fn paint_browser_glyph(u: f32, v: f32, edge: f32, mut col: [f32; 4]) -> [f32; 4] {
    let dial_sd = (u - 0.50).hypot(v - 0.50) - 0.345;
    let dial_cov = sq_cov(dial_sd, edge);
    if dial_cov > 0.0 {
        let dial_rgb = lerp_rgb([64.0, 184.0, 255.0], [18.0, 96.0, 236.0], (v - 0.16) * 1.45);
        col = blend_rgba(col, [dial_rgb[0], dial_rgb[1], dial_rgb[2], dial_cov]);

        if dial_sd > -0.055 && dial_sd < -0.022 {
            let angle = (v - 0.50).atan2(u - 0.50);
            let tick = ((angle * 12.0).cos() * 0.5 + 0.5).powf(4.0);
            col = blend_rgba(col, [235.0, 246.0, 255.0, tick * 0.55 * dial_cov]);
        }
    }
    let dx = u - 0.50;
    let dy = v - 0.50;
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let along = dx * s - dy * s;
    let perp = (dx * s + dy * s).abs();
    let half_len = 0.275_f32;
    let half_w = 0.068_f32;
    let t = (along.abs() / half_len).clamp(0.0, 1.0);
    let width_at = half_w * (1.0 - t);
    let needle_sd = (perp - width_at).max(along.abs() - half_len);
    let needle_cov = sq_cov(needle_sd, edge * 0.9);
    if needle_cov > 0.0 {
        let needle_rgb = if along >= 0.0 {
            [252.0, 72.0, 58.0]
        } else {
            [248.0, 250.0, 255.0]
        };
        col = blend_rgba(
            col,
            [needle_rgb[0], needle_rgb[1], needle_rgb[2], needle_cov],
        );
    }
    let pivot_cov = sq_cov((u - 0.50).hypot(v - 0.50) - 0.026, edge);
    if pivot_cov > 0.0 {
        col = blend_rgba(col, [215.0, 224.0, 238.0, pivot_cov]);
    }
    col
}

fn paint_settings_glyph(u: f32, v: f32, edge: f32, mut col: [f32; 4]) -> [f32; 4] {
    let dx = u - 0.50;
    let dy = v - 0.50;
    let r = dx.hypot(dy);
    let well_cov = sq_cov(r - 0.35, edge);
    if well_cov > 0.0 {
        let well_rgb = lerp_rgb([74.0, 78.0, 86.0], [98.0, 102.0, 112.0], (v - 0.15) * 1.4);
        col = blend_rgba(
            col,
            [well_rgb[0], well_rgb[1], well_rgb[2], well_cov * 0.92],
        );
    }
    let angle = dy.atan2(dx);
    let teeth = ((angle * 10.0).cos() * 3.2).clamp(-1.0, 1.0);
    let outer_r = 0.285 + 0.034 * teeth;
    let gear_sd = (r - outer_r).max(0.145 - r);
    let gear_cov = sq_cov(gear_sd, edge);
    if gear_cov > 0.0 {
        let g_rgb = lerp_rgb(
            [238.0, 241.0, 246.0],
            [164.0, 170.0, 180.0],
            (v - 0.2) * 1.6,
        );
        col = blend_rgba(col, [g_rgb[0], g_rgb[1], g_rgb[2], gear_cov]);
    }
    let spoke_mod = (angle * 3.0).cos().abs();
    let in_spokes = r < 0.16 && r > 0.055 && spoke_mod > 0.72;
    let hub_cov = sq_cov((r - 0.058).max(0.03 - r), edge);
    if in_spokes || hub_cov > 0.0 {
        let a = if in_spokes { 0.9 } else { hub_cov };
        col = blend_rgba(col, [212.0, 217.0, 226.0, a]);
    }
    col
}

fn paint_rusty_system_glyph(u: f32, v: f32, edge: f32, mut col: [f32; 4]) -> [f32; 4] {
    // Faceted pentagon emblem with bright inner prism core for RustyOS system alerts.
    let outer = [
        (0.50, 0.21),
        (0.77, 0.41),
        (0.67, 0.74),
        (0.33, 0.74),
        (0.23, 0.41),
    ];
    let outer_sd = crate::surfaces::signed_distance_polygon(u, v, &outer);
    let outer_cov = sq_cov(outer_sd, edge);
    if outer_cov > 0.0 {
        let grad = lerp_rgb(
            [255.0, 250.0, 255.0],
            [218.0, 198.0, 255.0],
            (v - 0.2) * 1.6,
        );
        col = blend_rgba(col, [grad[0], grad[1], grad[2], outer_cov * 0.96]);
    }
    let hole = [
        (0.50, 0.36),
        (0.63, 0.46),
        (0.58, 0.61),
        (0.42, 0.61),
        (0.37, 0.46),
    ];
    let hole_sd = crate::surfaces::signed_distance_polygon(u, v, &hole);
    let hole_cov = sq_cov(hole_sd, edge);
    if hole_cov > 0.0 {
        col = blend_rgba(col, [104.0, 52.0, 194.0, hole_cov * 0.92]);
    }
    let core_cov = sq_cov((u - 0.50).hypot(v - 0.50) - 0.055, edge);
    if core_cov > 0.0 {
        col = blend_rgba(col, [252.0, 246.0, 255.0, core_cov * 0.95]);
    }
    col
}

fn paint_squircle_glyph(
    u: f32,
    v: f32,
    edge: f32,
    icon: BuiltinAppIcon,
    mut col: [f32; 4],
) -> [f32; 4] {
    match icon {
        BuiltinAppIcon::Terminal => {
            // Subtle metallic inner bevel lip
            let inner_bevel = squircle_sd(u, v, 0.50, 0.50, 0.405, 0.405);
            let ring_cov = sq_cov((inner_bevel.abs() - 0.011).max(-inner_bevel - 0.03), edge);
            if ring_cov > 0.0 {
                col = blend_rgba(col, [188.0, 184.0, 212.0, ring_cov * 0.22]);
            }
            let leg_top = sq_sd_segment(u, v, 0.26, 0.34, 0.43, 0.48, 0.036);
            let leg_bot = sq_sd_segment(u, v, 0.43, 0.48, 0.26, 0.62, 0.036);
            let chevron_cov = sq_cov(leg_top.min(leg_bot), edge);
            if chevron_cov > 0.0 {
                col = blend_rgba(col, [244.0, 240.0, 255.0, chevron_cov]);
            }
            let bar = sq_sd_segment(u, v, 0.50, 0.62, 0.72, 0.62, 0.034);
            let bar_cov = sq_cov(bar, edge);
            if bar_cov > 0.0 {
                col = blend_rgba(col, [64.0, 224.0, 160.0, bar_cov]);
            }
        }
        BuiltinAppIcon::Files => {
            let tab = sq_sd_rounded_box(u, v, 0.34, 0.31, 0.14, 0.06, 0.03);
            let back = sq_sd_rounded_box(u, v, 0.50, 0.44, 0.31, 0.14, 0.05);
            let flap_cov = sq_cov(tab.min(back), edge);
            if flap_cov > 0.0 {
                col = blend_rgba(col, [22.0, 82.0, 198.0, flap_cov * 0.78]);
            }
            let paper = sq_sd_rounded_box(u, v, 0.50, 0.40, 0.26, 0.075, 0.025);
            let paper_cov = sq_cov(paper, edge);
            if paper_cov > 0.0 {
                col = blend_rgba(col, [248.0, 251.0, 255.0, paper_cov * 0.96]);
            }
            let front = sq_sd_rounded_box(u, v, 0.50, 0.57, 0.31, 0.18, 0.055);
            let front_cov = sq_cov(front, edge);
            if front_cov > 0.0 {
                let f_rgb = lerp_rgb(
                    [224.0, 244.0, 255.0],
                    [142.0, 204.0, 255.0],
                    (v - 0.39) * 2.8,
                );
                col = blend_rgba(col, [f_rgb[0], f_rgb[1], f_rgb[2], front_cov]);
            }
        }
        BuiltinAppIcon::Overview => return paint_overview_glyph(u, v, edge, col),
        BuiltinAppIcon::Browser => return paint_browser_glyph(u, v, edge, col),
        BuiltinAppIcon::Notes => {
            // Warm amber header bar across top + cream ruled notebook paper lines
            if v < 0.295 {
                let hdr_rgb = lerp_rgb([255.0, 214.0, 36.0], [244.0, 182.0, 12.0], v / 0.295);
                col = [hdr_rgb[0], hdr_rgb[1], hdr_rgb[2], 1.0];
            }
            let divider_sd = (v - 0.305).abs() - 0.010;
            let div_cov = sq_cov(divider_sd, edge);
            if div_cov > 0.0 {
                col = blend_rgba(col, [186.0, 168.0, 134.0, div_cov * 0.65]);
            }
            for rule_y in [0.46_f32, 0.60, 0.74] {
                let rule_sd = sq_sd_segment(u, v, 0.14, rule_y, 0.86, rule_y, 0.008);
                let rule_cov = sq_cov(rule_sd, edge);
                if rule_cov > 0.0 {
                    col = blend_rgba(col, [196.0, 192.0, 184.0, rule_cov * 0.80]);
                }
            }
        }
        BuiltinAppIcon::Settings => return paint_settings_glyph(u, v, edge, col),
        BuiltinAppIcon::RustySystem => return paint_rusty_system_glyph(u, v, edge, col),
        BuiltinAppIcon::Downloads | BuiltinAppIcon::Trash => {}
        BuiltinAppIcon::WindowFallback { .. } => {
            let win = sq_sd_rounded_box(u, v, 0.50, 0.51, 0.27, 0.22, 0.05);
            let win_cov = sq_cov(win, edge);
            if win_cov > 0.0 {
                col = blend_rgba(col, [246.0, 244.0, 255.0, win_cov * 0.88]);
                if v < 0.39 {
                    col = blend_rgba(col, [215.0, 205.0, 245.0, win_cov * 0.55]);
                }
            }
            for (cx, dot_rgb) in [
                (0.32, [255.0, 95.0, 87.0]),
                (0.39, [254.0, 188.0, 46.0]),
                (0.46, [40.0, 200.0, 64.0]),
            ] {
                let d_cov = sq_cov((u - cx).hypot(v - 0.345) - 0.022, edge);
                if d_cov > 0.0 {
                    col = blend_rgba(col, [dot_rgb[0], dot_rgb[1], dot_rgb[2], d_cov]);
                }
            }
        }
    }
    col
}

#[allow(clippy::cast_precision_loss)]
fn sample_builtin_squircle_pixel(px: i32, py: i32, size_px: i32, icon: BuiltinAppIcon) -> [u8; 4] {
    let dim = size_px.max(1) as f32;
    let edge = 1.15 / dim;
    let mut sum = [0.0_f32; 4];
    for sy in [0.125_f32, 0.375, 0.625, 0.875] {
        for sx in [0.125_f32, 0.375, 0.625, 0.875] {
            let u = (px as f32 + sx) / dim;
            let v = (py as f32 + sy) / dim;
            let s = sample_squircle_uv(u, v, edge, icon);
            let a = s[3].clamp(0.0, 1.0);
            sum[0] += s[0] * a;
            sum[1] += s[1] * a;
            sum[2] += s[2] * a;
            sum[3] += a;
        }
    }
    let avg_a = (sum[3] * (1.0 / 16.0)).clamp(0.0, 1.0);
    if avg_a <= 0.001 {
        return [0, 0, 0, 0];
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    [
        (sum[2] * (1.0 / 16.0)).clamp(0.0, 255.0).round() as u8,
        (sum[1] * (1.0 / 16.0)).clamp(0.0, 255.0).round() as u8,
        (sum[0] * (1.0 / 16.0)).clamp(0.0, 255.0).round() as u8,
        (avg_a * 255.0).clamp(0.0, 255.0).round() as u8,
    ]
}

fn rasterize_builtin_squircle_buffer(icon: BuiltinAppIcon, size_px: i32) -> MemoryRenderBuffer {
    let dim = usize::try_from(size_px).unwrap_or(1).max(1);
    let mut bgra = vec![0_u8; dim * dim * 4];
    for py in 0..size_px {
        for px in 0..size_px {
            let row = usize::try_from(py).unwrap_or(0);
            let col = usize::try_from(px).unwrap_or(0);
            let idx = (row * dim + col) * 4;
            if idx + 4 <= bgra.len() {
                let p = sample_builtin_squircle_pixel(px, py, size_px, icon);
                bgra[idx..idx + 4].copy_from_slice(&p);
            }
        }
    }
    upload_bgra_buffer(size_px, &bgra)
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
        assert_ne!(downloads_center, trash_center);

        for kind in [
            BuiltinAppIcon::Files,
            BuiltinAppIcon::Browser,
            BuiltinAppIcon::RustySystem,
            BuiltinAppIcon::Downloads,
            BuiltinAppIcon::Trash,
        ] {
            let corner = sample_builtin_squircle_pixel(0, 0, 48, kind);
            assert_eq!(corner[3], 0, "{kind:?} corner should stay transparent");
        }
        Ok(())
    }
}
