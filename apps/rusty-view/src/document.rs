//! Image document handling, decoding, and pixel transformations for `rusty-view`.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rusty_services::image_decode::decode_image_rgba_limited;
use rusty_ui::PixelFormat;

const MAX_VIEW_DIMENSION: u32 = 16_384;
static NEXT_DOC_CACHE_KEY: AtomicU64 = AtomicU64::new(1);

fn next_cache_key() -> u64 {
    NEXT_DOC_CACHE_KEY.fetch_add(1, Ordering::Relaxed)
}

/// Premultiplies straight RGBA8888 pixels in-place for `UiDrawCmd::Image`.
pub(crate) fn premultiply_rgba_in_place(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        if a == 255 {
            continue;
        }
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
            continue;
        }
        px[0] = ((u16::from(px[0]) * a + 127) / 255) as u8;
        px[1] = ((u16::from(px[1]) * a + 127) / 255) as u8;
        px[2] = ((u16::from(px[2]) * a + 127) / 255) as u8;
    }
}

/// Represents an active loaded image in memory with its oriented pixels and metadata.
#[derive(Debug, Clone)]
pub(crate) struct ImageDocument {
    pub(crate) cache_key: u64,
    pub(crate) filename: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Arc<[u8]>,
    pub(crate) format: PixelFormat,
    pub(crate) file_size_bytes: u64,
    pub(crate) rotation_deg: u16,
    pub(crate) flipped_h: bool,
}

impl ImageDocument {
    /// Loads and decodes an image file from the specified path.
    #[must_use]
    pub(crate) fn load_from_path(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let file_size_bytes = meta.len();
        let bytes = fs::read(path).ok()?;
        let (w, h, mut rgba) = decode_image_rgba_limited(&bytes, MAX_VIEW_DIMENSION)?;
        premultiply_rgba_in_place(&mut rgba);

        let filename = path
            .file_name()
            .map_or_else(|| "Image".to_owned(), |n| n.to_string_lossy().into_owned());

        let orig_w = u32::try_from(w).ok()?;
        let orig_h = u32::try_from(h).ok()?;

        Some(Self {
            cache_key: next_cache_key(),
            filename,
            width: orig_w,
            height: orig_h,
            pixels: Arc::from(rgba.into_boxed_slice()),
            format: PixelFormat::Rgba8888,
            file_size_bytes,
            rotation_deg: 0,
            flipped_h: false,
        })
    }

    /// Formats human-readable file size.
    #[must_use]
    pub(crate) fn formatted_file_size(&self) -> String {
        format_file_size(self.file_size_bytes)
    }

    /// Rotates the image 90 degrees clockwise in-place.
    pub(crate) fn rotate_cw(&mut self) {
        let (new_w, new_h, rotated) = rotate_rgba_90_cw(&self.pixels, self.width, self.height);
        self.cache_key = next_cache_key();
        self.width = new_w;
        self.height = new_h;
        self.pixels = Arc::from(rotated.into_boxed_slice());
        self.rotation_deg = (self.rotation_deg + 90) % 360;
    }

    /// Rotates the image 90 degrees counter-clockwise in-place.
    pub(crate) fn rotate_ccw(&mut self) {
        let (new_w, new_h, rotated) = rotate_rgba_90_ccw(&self.pixels, self.width, self.height);
        self.cache_key = next_cache_key();
        self.width = new_w;
        self.height = new_h;
        self.pixels = Arc::from(rotated.into_boxed_slice());
        self.rotation_deg = (self.rotation_deg + 270) % 360;
    }

    /// Horizontally flips the image pixels in-place.
    pub(crate) fn flip_horizontal(&mut self) {
        let flipped = flip_rgba_h(&self.pixels, self.width, self.height);
        self.cache_key = next_cache_key();
        self.pixels = Arc::from(flipped.into_boxed_slice());
        self.flipped_h = !self.flipped_h;
    }
}

fn rotate_rgba_90_cw(src: &[u8], w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let w_usize = w as usize;
    let h_usize = h as usize;
    let new_w = h_usize;
    let new_h = w_usize;
    let mut dst = vec![0u8; new_w * new_h * 4];

    for y in 0..h_usize {
        for x in 0..w_usize {
            let src_idx = (y * w_usize + x) * 4;
            let dst_x = h_usize - 1 - y;
            let dst_y = x;
            let dst_idx = (dst_y * new_w + dst_x) * 4;
            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }

    (h, w, dst)
}

fn rotate_rgba_90_ccw(src: &[u8], w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let w_usize = w as usize;
    let h_usize = h as usize;
    let new_w = h_usize;
    let new_h = w_usize;
    let mut dst = vec![0u8; new_w * new_h * 4];

    for y in 0..h_usize {
        for x in 0..w_usize {
            let src_idx = (y * w_usize + x) * 4;
            let dst_x = y;
            let dst_y = w_usize - 1 - x;
            let dst_idx = (dst_y * new_w + dst_x) * 4;
            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }

    (h, w, dst)
}

fn flip_rgba_h(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let w_usize = w as usize;
    let h_usize = h as usize;
    let mut dst = vec![0u8; w_usize * h_usize * 4];

    for y in 0..h_usize {
        for x in 0..w_usize {
            let src_idx = (y * w_usize + x) * 4;
            let dst_x = w_usize - 1 - x;
            let dst_idx = (y * w_usize + dst_x) * 4;
            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }

    dst
}

/// Formats raw bytes into standard B, KB, MB, GB string representations.
#[must_use]
pub(crate) fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        let int_part = bytes / GB;
        let frac = (bytes % GB * 10) / GB;
        format!("{int_part}.{frac} GB")
    } else if bytes >= MB {
        let int_part = bytes / MB;
        let frac = (bytes % MB * 10) / MB;
        format!("{int_part}.{frac} MB")
    } else if bytes >= KB {
        let int_part = bytes / KB;
        let frac = (bytes % KB * 10) / KB;
        format!("{int_part}.{frac} KB")
    } else {
        format!("{bytes} B")
    }
}
