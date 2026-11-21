//! Embedded rasterized RustyOS SVG icons for `rusty-files` (`RUSTYOS-149`).
//!
//! Source SVGs live in `assets/icons/files/svg/` and are pre-rasterized at `@2x`
//! (`32x32` sidebar template glyphs, `128x128` file/folder icons) into
//! `assets/icons/files/png/` so `rusty-files` needs zero runtime SVG
//! dependencies while rendering crisp anti-aliased icons.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use rusty_services::file_manager::{FileEntry, FileEntryKind};

use crate::render::Canvas;

// --- Sidebar template PNGs (32x32 @2x, white glyphs tinted at blit time) ---
const PNG_SB_RECENTS: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_recents.png");
const PNG_SB_SHARED: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_shared.png");
const PNG_SB_APPLICATIONS: &[u8] =
    include_bytes!("../../../assets/icons/files/png/sidebar_applications.png");
const PNG_SB_DESKTOP: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_desktop.png");
const PNG_SB_DOCUMENTS: &[u8] =
    include_bytes!("../../../assets/icons/files/png/sidebar_documents.png");
const PNG_SB_DOWNLOADS: &[u8] =
    include_bytes!("../../../assets/icons/files/png/sidebar_downloads.png");
const PNG_SB_PICTURES: &[u8] =
    include_bytes!("../../../assets/icons/files/png/sidebar_pictures.png");
const PNG_SB_MUSIC: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_music.png");
const PNG_SB_VIDEOS: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_videos.png");
const PNG_SB_HOME: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_home.png");
const PNG_SB_DRIVE: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_drive.png");
const PNG_SB_USB: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_usb.png");
const PNG_SB_EJECT: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_eject.png");
const PNG_SB_NETWORK: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_network.png");
const PNG_SB_TRASH: &[u8] = include_bytes!("../../../assets/icons/files/png/sidebar_trash.png");

// --- Large file & folder PNGs (128x128 @2x) ---
const PNG_FOLDER: &[u8] = include_bytes!("../../../assets/icons/files/png/folder.png");
const PNG_FILE_GENERIC: &[u8] = include_bytes!("../../../assets/icons/files/png/file_generic.png");
const PNG_FILE_TEXT: &[u8] = include_bytes!("../../../assets/icons/files/png/file_text.png");
const PNG_FILE_CODE: &[u8] = include_bytes!("../../../assets/icons/files/png/file_code.png");
const PNG_FILE_IMAGE: &[u8] = include_bytes!("../../../assets/icons/files/png/file_image.png");
const PNG_FILE_AUDIO: &[u8] = include_bytes!("../../../assets/icons/files/png/file_audio.png");
const PNG_FILE_VIDEO: &[u8] = include_bytes!("../../../assets/icons/files/png/file_video.png");
const PNG_FILE_ARCHIVE: &[u8] = include_bytes!("../../../assets/icons/files/png/file_archive.png");
const PNG_FILE_PDF: &[u8] = include_bytes!("../../../assets/icons/files/png/file_pdf.png");
const PNG_FILE_APP: &[u8] = include_bytes!("../../../assets/icons/files/png/file_app.png");

// --- Toolbar & section template PNGs (32x32 @2x, white glyphs tinted at blit time) ---
const PNG_TB_BACK: &[u8] = include_bytes!("../../../assets/icons/files/png/toolbar_back.png");
const PNG_TB_FORWARD: &[u8] = include_bytes!("../../../assets/icons/files/png/toolbar_forward.png");
const PNG_TB_VIEW_ICONS: &[u8] =
    include_bytes!("../../../assets/icons/files/png/toolbar_view_icons.png");
const PNG_TB_VIEW_LIST: &[u8] =
    include_bytes!("../../../assets/icons/files/png/toolbar_view_list.png");
const PNG_TB_VIEW_COLUMNS: &[u8] =
    include_bytes!("../../../assets/icons/files/png/toolbar_view_columns.png");
const PNG_TB_VIEW_GALLERY: &[u8] =
    include_bytes!("../../../assets/icons/files/png/toolbar_view_gallery.png");
const PNG_TB_SORT: &[u8] = include_bytes!("../../../assets/icons/files/png/toolbar_sort.png");
const PNG_TB_SHARE: &[u8] = include_bytes!("../../../assets/icons/files/png/toolbar_share.png");
const PNG_TB_TAG: &[u8] = include_bytes!("../../../assets/icons/files/png/toolbar_tag.png");
const PNG_TB_MORE: &[u8] = include_bytes!("../../../assets/icons/files/png/toolbar_more.png");
const PNG_TB_SEARCH: &[u8] = include_bytes!("../../../assets/icons/files/png/toolbar_search.png");
const PNG_TB_CHEVRON_DOWN: &[u8] =
    include_bytes!("../../../assets/icons/files/png/toolbar_chevron_down.png");
const PNG_TB_CHEVRON_RIGHT: &[u8] =
    include_bytes!("../../../assets/icons/files/png/toolbar_chevron_right.png");

/// Toolbar & chrome template SVG asset keys (`RUSTYOS-150`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ToolbarSvgAsset {
    Back,
    Forward,
    ViewIcons,
    ViewList,
    ViewColumns,
    ViewGallery,
    Sort,
    Share,
    Tag,
    More,
    Search,
    ChevronDown,
    ChevronRight,
}

impl ToolbarSvgAsset {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 13] = [
        Self::Back,
        Self::Forward,
        Self::ViewIcons,
        Self::ViewList,
        Self::ViewColumns,
        Self::ViewGallery,
        Self::Sort,
        Self::Share,
        Self::Tag,
        Self::More,
        Self::Search,
        Self::ChevronDown,
        Self::ChevronRight,
    ];

    const fn png_bytes(self) -> &'static [u8] {
        match self {
            Self::Back => PNG_TB_BACK,
            Self::Forward => PNG_TB_FORWARD,
            Self::ViewIcons => PNG_TB_VIEW_ICONS,
            Self::ViewList => PNG_TB_VIEW_LIST,
            Self::ViewColumns => PNG_TB_VIEW_COLUMNS,
            Self::ViewGallery => PNG_TB_VIEW_GALLERY,
            Self::Sort => PNG_TB_SORT,
            Self::Share => PNG_TB_SHARE,
            Self::Tag => PNG_TB_TAG,
            Self::More => PNG_TB_MORE,
            Self::Search => PNG_TB_SEARCH,
            Self::ChevronDown => PNG_TB_CHEVRON_DOWN,
            Self::ChevronRight => PNG_TB_CHEVRON_RIGHT,
        }
    }
}

/// Sidebar template SVG asset keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SidebarSvgAsset {
    Recents,
    Shared,
    Applications,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    Home,
    Drive,
    Usb,
    Eject,
    Network,
    Trash,
}

impl SidebarSvgAsset {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 15] = [
        Self::Recents,
        Self::Shared,
        Self::Applications,
        Self::Desktop,
        Self::Documents,
        Self::Downloads,
        Self::Pictures,
        Self::Music,
        Self::Videos,
        Self::Home,
        Self::Drive,
        Self::Usb,
        Self::Eject,
        Self::Network,
        Self::Trash,
    ];

    const fn png_bytes(self) -> &'static [u8] {
        match self {
            Self::Recents => PNG_SB_RECENTS,
            Self::Shared => PNG_SB_SHARED,
            Self::Applications => PNG_SB_APPLICATIONS,
            Self::Desktop => PNG_SB_DESKTOP,
            Self::Documents => PNG_SB_DOCUMENTS,
            Self::Downloads => PNG_SB_DOWNLOADS,
            Self::Pictures => PNG_SB_PICTURES,
            Self::Music => PNG_SB_MUSIC,
            Self::Videos => PNG_SB_VIDEOS,
            Self::Home => PNG_SB_HOME,
            Self::Drive => PNG_SB_DRIVE,
            Self::Usb => PNG_SB_USB,
            Self::Eject => PNG_SB_EJECT,
            Self::Network => PNG_SB_NETWORK,
            Self::Trash => PNG_SB_TRASH,
        }
    }
}

/// Large file & folder SVG asset keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EntrySvgAsset {
    Folder,
    Generic,
    Text,
    Code,
    Image,
    Audio,
    Video,
    Archive,
    Pdf,
    App,
}

impl EntrySvgAsset {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 10] = [
        Self::Folder,
        Self::Generic,
        Self::Text,
        Self::Code,
        Self::Image,
        Self::Audio,
        Self::Video,
        Self::Archive,
        Self::Pdf,
        Self::App,
    ];

    const fn png_bytes(self) -> &'static [u8] {
        match self {
            Self::Folder => PNG_FOLDER,
            Self::Generic => PNG_FILE_GENERIC,
            Self::Text => PNG_FILE_TEXT,
            Self::Code => PNG_FILE_CODE,
            Self::Image => PNG_FILE_IMAGE,
            Self::Audio => PNG_FILE_AUDIO,
            Self::Video => PNG_FILE_VIDEO,
            Self::Archive => PNG_FILE_ARCHIVE,
            Self::Pdf => PNG_FILE_PDF,
            Self::App => PNG_FILE_APP,
        }
    }
}

/// Classifies a `FileEntry` into its crisp SVG file/folder icon asset.
#[must_use]
pub(crate) fn entry_icon_asset(entry: &FileEntry) -> EntrySvgAsset {
    match entry.kind {
        FileEntryKind::Directory | FileEntryKind::MountPoint => EntrySvgAsset::Folder,
        FileEntryKind::Application => EntrySvgAsset::App,
        _ => classify_extension(&entry.name),
    }
}

/// Classifies a filename by extension into a file icon asset.
#[must_use]
pub(crate) fn classify_extension(name: &str) -> EntrySvgAsset {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tar.gz")
        || lower.ends_with(".tar.zst")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".tar.bz2")
    {
        return EntrySvgAsset::Archive;
    }
    let ext = lower.rsplit('.').next().unwrap_or("");
    if ext == lower {
        return EntrySvgAsset::Generic;
    }
    match ext {
        "pdf" => EntrySvgAsset::Pdf,
        "zip" | "tar" | "gz" | "tgz" | "zst" | "xz" | "bz2" | "7z" | "rar" => {
            EntrySvgAsset::Archive
        }
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" | "ico" | "tiff" | "avif"
        | "heic" => EntrySvgAsset::Image,
        "mp3" | "flac" | "wav" | "ogg" | "opus" | "m4a" | "aac" => EntrySvgAsset::Audio,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "flv" | "m4v" => EntrySvgAsset::Video,
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "c" | "h" | "cpp" | "hpp" | "go" | "java"
        | "sh" | "bash" | "zsh" | "toml" | "json" | "yaml" | "yml" | "xml" | "html" | "css"
        | "scss" | "sql" | "glsl" | "wgsl" | "rb" | "php" | "kt" | "swift" => EntrySvgAsset::Code,
        "txt" | "md" | "rst" | "log" | "csv" | "ini" | "conf" | "cfg" | "env" | "rtf" | "doc"
        | "docx" | "odt" => EntrySvgAsset::Text,
        _ => EntrySvgAsset::Generic,
    }
}

/// Decoded straight-RGBA PNG bitmap.
struct DecodedRgba {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

fn decode_once<'a>(
    slot: &'a OnceLock<Option<DecodedRgba>>,
    bytes: &[u8],
) -> Option<&'a DecodedRgba> {
    slot.get_or_init(|| {
        rusty_services::image_decode::decode_png_rgba(bytes).map(|(w, h, rgba)| DecodedRgba {
            w,
            h,
            rgba,
        })
    })
    .as_ref()
}

fn decoded_sidebar(asset: SidebarSvgAsset) -> Option<&'static DecodedRgba> {
    static SLOTS: [OnceLock<Option<DecodedRgba>>; 15] = [const { OnceLock::new() }; 15];
    let idx = match asset {
        SidebarSvgAsset::Recents => 0,
        SidebarSvgAsset::Shared => 1,
        SidebarSvgAsset::Applications => 2,
        SidebarSvgAsset::Desktop => 3,
        SidebarSvgAsset::Documents => 4,
        SidebarSvgAsset::Downloads => 5,
        SidebarSvgAsset::Pictures => 6,
        SidebarSvgAsset::Music => 7,
        SidebarSvgAsset::Videos => 8,
        SidebarSvgAsset::Home => 9,
        SidebarSvgAsset::Drive => 10,
        SidebarSvgAsset::Usb => 11,
        SidebarSvgAsset::Eject => 12,
        SidebarSvgAsset::Network => 13,
        SidebarSvgAsset::Trash => 14,
    };
    decode_once(&SLOTS[idx], asset.png_bytes())
}

fn decoded_entry(asset: EntrySvgAsset) -> Option<&'static DecodedRgba> {
    static SLOTS: [OnceLock<Option<DecodedRgba>>; 10] = [const { OnceLock::new() }; 10];
    let idx = match asset {
        EntrySvgAsset::Folder => 0,
        EntrySvgAsset::Generic => 1,
        EntrySvgAsset::Text => 2,
        EntrySvgAsset::Code => 3,
        EntrySvgAsset::Image => 4,
        EntrySvgAsset::Audio => 5,
        EntrySvgAsset::Video => 6,
        EntrySvgAsset::Archive => 7,
        EntrySvgAsset::Pdf => 8,
        EntrySvgAsset::App => 9,
    };
    decode_once(&SLOTS[idx], asset.png_bytes())
}

fn decoded_toolbar(asset: ToolbarSvgAsset) -> Option<&'static DecodedRgba> {
    static SLOTS: [OnceLock<Option<DecodedRgba>>; 13] = [const { OnceLock::new() }; 13];
    let idx = match asset {
        ToolbarSvgAsset::Back => 0,
        ToolbarSvgAsset::Forward => 1,
        ToolbarSvgAsset::ViewIcons => 2,
        ToolbarSvgAsset::ViewList => 3,
        ToolbarSvgAsset::ViewColumns => 4,
        ToolbarSvgAsset::ViewGallery => 5,
        ToolbarSvgAsset::Sort => 6,
        ToolbarSvgAsset::Share => 7,
        ToolbarSvgAsset::Tag => 8,
        ToolbarSvgAsset::More => 9,
        ToolbarSvgAsset::Search => 10,
        ToolbarSvgAsset::ChevronDown => 11,
        ToolbarSvgAsset::ChevronRight => 12,
    };
    decode_once(&SLOTS[idx], asset.png_bytes())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScaledSpriteKey {
    Sidebar {
        asset: SidebarSvgAsset,
        size: i32,
        tint: [u8; 4],
    },
    Toolbar {
        asset: ToolbarSvgAsset,
        size: i32,
        tint: [u8; 4],
    },
    Entry {
        asset: EntrySvgAsset,
        size: i32,
    },
}

const MAX_SCALED_SPRITES: usize = 256;

thread_local! {
    static SCALED_SPRITE_CACHE: RefCell<HashMap<ScaledSpriteKey, Vec<u8>>> =
        RefCell::new(HashMap::new());
}

/// Area-averaged (box-filtered) downsampler from straight RGBA to premultiplied BGRA.
/// When `tint` is `Some([r, g, b, a])`, uses the source alpha channel with the tint color.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn resample_to_premul_bgra(src: &DecodedRgba, dst_dim: i32, tint: Option<[u8; 4]>) -> Vec<u8> {
    let dim = usize::try_from(dst_dim.max(1)).unwrap_or(1);
    let mut out = vec![0_u8; dim * dim * 4];
    for dy in 0..dim {
        let sy0 = (dy * src.h) / dim;
        let sy1 = (((dy + 1) * src.h) / dim).max(sy0 + 1).min(src.h);
        for dx in 0..dim {
            let sx0 = (dx * src.w) / dim;
            let sx1 = (((dx + 1) * src.w) / dim).max(sx0 + 1).min(src.w);
            let mut sum_r = 0_u64;
            let mut sum_g = 0_u64;
            let mut sum_b = 0_u64;
            let mut sum_a = 0_u64;
            let mut count = 0_u64;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let off = (sy * src.w + sx) * 4;
                    let sa = u64::from(src.rgba[off + 3]);
                    sum_r += u64::from(src.rgba[off]) * sa;
                    sum_g += u64::from(src.rgba[off + 1]) * sa;
                    sum_b += u64::from(src.rgba[off + 2]) * sa;
                    sum_a += sa;
                    count += 1;
                }
            }
            if count == 0 || sum_a == 0 {
                continue;
            }
            let avg_a = (sum_a / count).min(255);
            let dst_off = (dy * dim + dx) * 4;
            if let Some([tr, tg, tb, ta]) = tint {
                let eff_a = (avg_a * u64::from(ta) + 127) / 255;
                out[dst_off] = ((u64::from(tb) * eff_a + 127) / 255) as u8;
                out[dst_off + 1] = ((u64::from(tg) * eff_a + 127) / 255) as u8;
                out[dst_off + 2] = ((u64::from(tr) * eff_a + 127) / 255) as u8;
                out[dst_off + 3] = eff_a as u8;
            } else {
                // sum_r is already alpha-weighted (`r * a`), so dividing by `count * 255`
                // directly produces premultiplied channel value in 0..=255.
                let denom = count * 255;
                out[dst_off] = ((sum_b + denom / 2) / denom).min(255) as u8;
                out[dst_off + 1] = ((sum_g + denom / 2) / denom).min(255) as u8;
                out[dst_off + 2] = ((sum_r + denom / 2) / denom).min(255) as u8;
                out[dst_off + 3] = avg_a as u8;
            }
        }
    }
    out
}

/// Blits a crisp, tinted sidebar SVG icon centered at `(cx, cy)`.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn draw_sidebar_svg(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    size_px: i32,
    asset: SidebarSvgAsset,
    tint_rgba: [u8; 4],
) {
    let Some(src) = decoded_sidebar(asset) else {
        return;
    };
    let dim = size_px.clamp(8, 64);
    let key = ScaledSpriteKey::Sidebar {
        asset,
        size: dim,
        tint: tint_rgba,
    };
    let half = f32::from(i16::try_from(dim).unwrap_or(16)) * 0.5;
    let dst_x = (cx - half).round() as i32;
    let dst_y = (cy - half).round() as i32;

    SCALED_SPRITE_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if map.len() >= MAX_SCALED_SPRITES && !map.contains_key(&key) {
            map.clear();
        }
        let sprite = map
            .entry(key)
            .or_insert_with(|| resample_to_premul_bgra(src, dim, Some(tint_rgba)));
        c.blit_premul_sprite(dst_x, dst_y, dim, dim, sprite);
    });
}

/// Blits a crisp, tinted toolbar SVG icon centered at `(cx, cy)`.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn draw_toolbar_svg(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    size_px: i32,
    asset: ToolbarSvgAsset,
    tint_rgba: [u8; 4],
) {
    let Some(src) = decoded_toolbar(asset) else {
        return;
    };
    let dim = size_px.clamp(8, 64);
    let key = ScaledSpriteKey::Toolbar {
        asset,
        size: dim,
        tint: tint_rgba,
    };
    let half = f32::from(i16::try_from(dim).unwrap_or(16)) * 0.5;
    let dst_x = (cx - half).round() as i32;
    let dst_y = (cy - half).round() as i32;

    SCALED_SPRITE_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if map.len() >= MAX_SCALED_SPRITES && !map.contains_key(&key) {
            map.clear();
        }
        let sprite = map
            .entry(key)
            .or_insert_with(|| resample_to_premul_bgra(src, dim, Some(tint_rgba)));
        c.blit_premul_sprite(dst_x, dst_y, dim, dim, sprite);
    });
}

/// Blits a crisp full-color file or folder SVG icon centered at `(cx, cy)`.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn draw_entry_svg(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    size_px: i32,
    asset: EntrySvgAsset,
) {
    let Some(src) = decoded_entry(asset) else {
        return;
    };
    let dim = size_px.clamp(12, 160);
    let key = ScaledSpriteKey::Entry { asset, size: dim };
    let half = f32::from(i16::try_from(dim).unwrap_or(64)) * 0.5;
    let dst_x = (cx - half).round() as i32;
    let dst_y = (cy - half).round() as i32;

    SCALED_SPRITE_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if map.len() >= MAX_SCALED_SPRITES && !map.contains_key(&key) {
            map.clear();
        }
        let sprite = map
            .entry(key)
            .or_insert_with(|| resample_to_premul_bgra(src, dim, None));
        c.blit_premul_sprite(dst_x, dst_y, dim, dim, sprite);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_files_svg_pngs_decode_cleanly() {
        for asset in SidebarSvgAsset::ALL {
            let decoded = decoded_sidebar(asset);
            assert!(decoded.is_some(), "sidebar icon {asset:?} failed to decode");
            if let Some(img) = decoded {
                assert_eq!(img.w, 32);
                assert_eq!(img.h, 32);
                assert_eq!(img.rgba.len(), 32 * 32 * 4);
            }
        }
        for asset in ToolbarSvgAsset::ALL {
            let decoded = decoded_toolbar(asset);
            assert!(decoded.is_some(), "toolbar icon {asset:?} failed to decode");
            if let Some(img) = decoded {
                assert_eq!(img.w, 32);
                assert_eq!(img.h, 32);
                assert_eq!(img.rgba.len(), 32 * 32 * 4);
            }
        }
        for asset in EntrySvgAsset::ALL {
            let decoded = decoded_entry(asset);
            assert!(decoded.is_some(), "entry icon {asset:?} failed to decode");
            if let Some(img) = decoded {
                assert_eq!(img.w, 128);
                assert_eq!(img.h, 128);
                assert_eq!(img.rgba.len(), 128 * 128 * 4);
            }
        }
    }

    #[test]
    fn extension_classifier_maps_common_file_types() {
        assert_eq!(classify_extension("report.pdf"), EntrySvgAsset::Pdf);
        assert_eq!(classify_extension("backup.tar.gz"), EntrySvgAsset::Archive);
        assert_eq!(classify_extension("bundle.zip"), EntrySvgAsset::Archive);
        assert_eq!(classify_extension("main.rs"), EntrySvgAsset::Code);
        assert_eq!(classify_extension("photo.PNG"), EntrySvgAsset::Image);
        assert_eq!(classify_extension("track.flac"), EntrySvgAsset::Audio);
        assert_eq!(classify_extension("clip.mp4"), EntrySvgAsset::Video);
        assert_eq!(classify_extension("notes.md"), EntrySvgAsset::Text);
        assert_eq!(classify_extension("README"), EntrySvgAsset::Generic);
    }
}
