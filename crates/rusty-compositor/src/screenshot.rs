//! Zero-dependency PNG screenshot capture & async file/clipboard saver (`RUSTYOS-031`).
//!
//! Renders the active frame into an offscreen `GlesTexture`, reads back only the
//! requested full-screen or focused-window region (`ExportMem::copy_texture`), and
//! encodes/writes PNG + invokes `wl-copy` on a background thread so the compositor
//! event loop never blocks on compression or disk I/O.

use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use rusty_wm::Rect;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
use smithay::utils::{Buffer as BufferCoord, Physical, Point, Rectangle, Size, Transform};

use crate::shell_render::CompositorRenderElement;
use crate::winit_backend::BACKGROUND_COLOR;

/// Requested region for an on-demand compositor screenshot capture (`RUSTYOS-031`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotRequest {
    /// Capture the entire compositor output.
    FullScreen,
    /// Capture the active/focused window bounds (falls back to full output if none).
    FocusedWindow,
}

/// Asynchronous screenshot save completion queue polled by the event loop without blocking.
#[derive(Debug, Default, Clone)]
pub struct ScreenshotResultQueue {
    queue: Arc<Mutex<VecDeque<Result<PathBuf, String>>>>,
}

impl ScreenshotResultQueue {
    /// Pops the next completed screenshot save result, if any.
    #[must_use]
    pub fn try_pop(&self) -> Option<Result<PathBuf, String>> {
        self.queue.try_lock().ok().and_then(|mut q| q.pop_front())
    }

    fn push(&self, item: Result<PathBuf, String>) {
        if let Ok(mut q) = self.queue.lock() {
            q.push_back(item);
        }
    }
}

/// Clamps an optional logical/physical crop rectangle inside `output_size`.
#[must_use]
pub fn clamp_capture_region(
    output_size: Size<i32, Physical>,
    crop: Option<Rect>,
) -> Rectangle<i32, BufferCoord> {
    let out_w = output_size.w.max(1);
    let out_h = output_size.h.max(1);
    let Some(crop_rect) = crop else {
        return Rectangle::new(Point::from((0, 0)), Size::from((out_w, out_h)));
    };
    let cx = crop_rect.x.clamp(0, out_w - 1);
    let cy = crop_rect.y.clamp(0, out_h - 1);
    let cw = crop_rect.width.max(1).min(out_w - cx);
    let ch = crop_rect.height.max(1).min(out_h - cy);
    Rectangle::new(Point::from((cx, cy)), Size::from((cw, ch)))
}

static SCREENSHOT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Reusable offscreen GLES texture target avoiding per-frame allocations during `ScreenCast` streaming.
#[derive(Default)]
pub struct OffscreenCaptureBuffer {
    cached: Option<(Size<i32, BufferCoord>, GlesTexture)>,
}

impl fmt::Debug for OffscreenCaptureBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OffscreenCaptureBuffer")
            .field("cached_size", &self.cached.as_ref().map(|(sz, _)| *sz))
            .finish()
    }
}

impl OffscreenCaptureBuffer {
    /// Renders `elements` into a cached offscreen `GlesTexture` and copies `crop` (or full output)
    /// as tightly packed opaque `RGBA8` bytes.
    ///
    /// # Errors
    ///
    /// Returns an error string if offscreen allocation, rendering, or GPU readback fails.
    #[allow(clippy::cast_possible_truncation)]
    pub fn capture_rgba(
        &mut self,
        renderer: &mut GlesRenderer,
        elements: &[CompositorRenderElement],
        output_size: Size<i32, Physical>,
        output_scale: f64,
        crop: Option<Rect>,
    ) -> Result<(u32, u32, Vec<u8>), String> {
        let scale = crate::config::clamp_display_scale(output_scale);
        let phys_crop = crop.map(|r| {
            Rect::new(
                (f64::from(r.x) * scale).round() as i32,
                (f64::from(r.y) * scale).round() as i32,
                (f64::from(r.width) * scale).round() as i32,
                (f64::from(r.height) * scale).round() as i32,
            )
        });
        let buf_size = Size::<i32, BufferCoord>::from((output_size.w.max(1), output_size.h.max(1)));
        let needs_alloc = self.cached.as_ref().is_none_or(|(sz, _)| *sz != buf_size);
        if needs_alloc {
            let tex: GlesTexture = renderer
                .create_buffer(Fourcc::Abgr8888, buf_size)
                .map_err(|err| format!("offscreen buffer allocation failed: {err}"))?;
            self.cached = Some((buf_size, tex));
        }

        let Some((_, tex)) = self.cached.as_mut() else {
            return Err("offscreen texture missing after allocation".to_owned());
        };
        {
            let mut fb = renderer
                .bind(tex)
                .map_err(|err| format!("offscreen bind failed: {err}"))?;
            let mut damage = OutputDamageTracker::new(output_size, scale, Transform::Normal);
            damage
                .render_output(renderer, &mut fb, 0, elements, BACKGROUND_COLOR)
                .map_err(|err| format!("offscreen render_output failed: {err}"))?;
        }

        let region = clamp_capture_region(output_size, phys_crop);
        let mapping = renderer
            .copy_texture(tex, region, Fourcc::Abgr8888)
            .map_err(|err| format!("GPU copy_texture failed: {err}"))?;
        let bytes = renderer
            .map_texture(&mapping)
            .map_err(|err| format!("GPU map_texture failed: {err}"))?;

        let width = u32::try_from(region.size.w).map_err(|_| "invalid capture width".to_owned())?;
        let height =
            u32::try_from(region.size.h).map_err(|_| "invalid capture height".to_owned())?;
        let expected_len = usize::try_from(width)
            .ok()
            .and_then(|w| usize::try_from(height).ok().map(|h| w * h * 4))
            .ok_or_else(|| "capture dimensions overflow".to_owned())?;
        if bytes.len() < expected_len {
            return Err(format!(
                "mapped buffer too short ({} < {expected_len})",
                bytes.len()
            ));
        }

        let mut rgba = bytes[..expected_len].to_vec();
        for px in rgba.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        Ok((width, height, rgba))
    }
}

/// Renders `elements` into an offscreen `GlesTexture` and copies `crop` (or full output) as opaque RGBA bytes.
///
/// # Errors
///
/// Returns an error string if offscreen allocation, rendering, or GPU readback fails.
pub fn capture_render_list_rgba(
    renderer: &mut GlesRenderer,
    elements: &[CompositorRenderElement],
    output_size: Size<i32, Physical>,
    output_scale: f64,
    crop: Option<Rect>,
) -> Result<(u32, u32, Vec<u8>), String> {
    let mut scratch = OffscreenCaptureBuffer::default();
    scratch.capture_rgba(renderer, elements, output_size, output_scale, crop)
}

/// Encodes and writes an XDG Portal screenshot on a background thread and replies with a `file://` URI.
pub fn spawn_save_portal_screenshot(
    request_id: u64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    wayland_socket: String,
    portal: rusty_services::PortalReplySender,
) {
    let _ = thread::Builder::new()
        .name("rustyos-portal-shot".to_owned())
        .spawn(move || {
            let result = encode_png_rgba(width, height, &rgba).and_then(|png_bytes| {
                let path = unique_screenshot_path();
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
                }
                fs::write(&path, &png_bytes)
                    .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
                copy_png_to_clipboard(&png_bytes, &wayland_socket);
                Ok(rusty_services::path_to_file_uri(&path))
            });
            portal.send_reply(rusty_services::PortalReply::ScreenshotResult { request_id, result });
        });
}

/// Converts output-local logical coordinates into clamped physical buffer pixel coordinates.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn logical_point_to_buffer_pixel(
    local_x: f64,
    local_y: f64,
    output_scale: f64,
    width: u32,
    height: u32,
) -> (i32, i32) {
    let scale = crate::config::clamp_display_scale(output_scale);
    let max_x = i32::try_from(width.saturating_sub(1)).unwrap_or(i32::MAX);
    let max_y = i32::try_from(height.saturating_sub(1)).unwrap_or(i32::MAX);
    let px = (local_x * scale).round() as i32;
    let py = (local_y * scale).round() as i32;
    (px.clamp(0, max_x), py.clamp(0, max_y))
}

/// Samples a single pixel from tightly packed `RGBA8` bytes and normalizes channels to `0.0..=1.0`.
///
/// # Errors
///
/// Returns an error string if dimensions are zero or the RGBA buffer is too small.
pub fn sample_rgba_pixel(
    width: u32,
    height: u32,
    rgba: &[u8],
    x: i32,
    y: i32,
) -> Result<(f64, f64, f64), String> {
    if width == 0 || height == 0 {
        return Err("sample dimensions must be positive".to_owned());
    }
    let w = usize::try_from(width).map_err(|_| "width overflow".to_owned())?;
    let h = usize::try_from(height).map_err(|_| "height overflow".to_owned())?;
    let expected_len = w
        .checked_mul(h)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| "sample buffer size overflow".to_owned())?;
    if rgba.len() < expected_len {
        return Err(format!(
            "RGBA buffer too short for {}x{}: {} < {expected_len}",
            width,
            height,
            rgba.len()
        ));
    }
    let max_x = i32::try_from(width - 1).unwrap_or(i32::MAX);
    let max_y = i32::try_from(height - 1).unwrap_or(i32::MAX);
    let cx = usize::try_from(x.clamp(0, max_x)).unwrap_or(0);
    let cy = usize::try_from(y.clamp(0, max_y)).unwrap_or(0);
    let offset = (cy * w + cx) * 4;
    let pixel = [
        rgba[offset],
        rgba[offset + 1],
        rgba[offset + 2],
        rgba[offset + 3],
    ];
    Ok(rusty_services::rgba_pixel_to_portal_color(pixel))
}

/// Spawns PNG encoding, file writing, and Wayland clipboard copy (`wl-copy`) on a background thread.
pub fn spawn_save_screenshot(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    wayland_socket: String,
    results: &ScreenshotResultQueue,
) {
    let queue = results.clone();
    let _ = thread::Builder::new()
        .name("rustyos-screenshot".to_owned())
        .spawn(move || {
            let res = save_screenshot_sync(width, height, &rgba, &wayland_socket);
            queue.push(res);
        });
}

fn save_screenshot_sync(
    width: u32,
    height: u32,
    rgba: &[u8],
    wayland_socket: &str,
) -> Result<PathBuf, String> {
    let png_bytes = encode_png_rgba(width, height, rgba)?;
    let path = default_screenshot_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    fs::write(&path, &png_bytes)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;

    copy_png_to_clipboard(&png_bytes, wayland_socket);
    Ok(path)
}

fn copy_png_to_clipboard(png_bytes: &[u8], wayland_socket: &str) {
    if let Ok(mut child) = Command::new("wl-copy")
        .args(["--type", "image/png"])
        .env("WAYLAND_DISPLAY", wayland_socket)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(png_bytes);
        }
        let _ = child.wait();
    }
}

/// Computes a collision-free screenshot path (`~/Pictures/Screenshots/RustyOS_<ts>[_seq].png`).
#[must_use]
pub fn unique_screenshot_path() -> PathBuf {
    let base = default_screenshot_path();
    if !base.exists() {
        return base;
    }
    let dir = base
        .parent()
        .map_or_else(std::env::temp_dir, Path::to_path_buf);
    let stem = base
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("RustyOS");
    loop {
        let seq = SCREENSHOT_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        let candidate = dir.join(format!("{stem}_{seq}.png"));
        if !candidate.exists() {
            return candidate;
        }
    }
}

/// Computes the default screenshot output path (`~/Pictures/Screenshots/RustyOS_<ts>.png`).
#[must_use]
pub fn default_screenshot_path() -> PathBuf {
    let pictures_dir = std::env::var_os("XDG_PICTURES_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join("Pictures"))
        })
        .unwrap_or_else(std::env::temp_dir);
    pictures_dir
        .join("Screenshots")
        .join(default_screenshot_filename())
}

/// Generates a deterministic timestamped filename `RustyOS_YYYY-MM-DD_HH-MM-SS.png`.
#[must_use]
pub fn default_screenshot_filename() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (year, month, day, hour, minute, second) = utc_components_from_epoch(secs);
    format!("RustyOS_{year:04}-{month:02}-{day:02}_{hour:02}-{minute:02}-{second:02}.png")
}

fn utc_components_from_epoch(epoch_secs: u64) -> (i64, u32, u32, u64, u64, u64) {
    let secs_day = epoch_secs % 86_400;
    let hour = secs_day / 3_600;
    let minute = (secs_day % 3_600) / 60;
    let second = secs_day % 60;

    // Howard Hinnant's civil-from-days algorithm
    let days = i64::try_from(epoch_secs / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = y + i64::from(m <= 2);
    (year, m, d, hour, minute, second)
}

/// Encodes tightly packed `RGBA8` (`width * height * 4` bytes) into a valid PNG byte buffer.
///
/// Uses valid uncompressed DEFLATE stored blocks (`BTYPE=00`) so zero external compression crates
/// are required while remaining 100% compliant with PNG viewers and `wl-copy`.
///
/// # Errors
///
/// Returns an error string if dimensions are zero or buffer length does not match `width * height * 4`.
pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("PNG dimensions must be positive".to_owned());
    }
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(4))
        .ok_or_else(|| "PNG row stride overflow".to_owned())?;
    let total_rgba = row_bytes
        .checked_mul(usize::try_from(height).unwrap_or(usize::MAX))
        .ok_or_else(|| "PNG buffer size overflow".to_owned())?;
    if rgba.len() != total_rgba {
        return Err(format!(
            "RGBA length mismatch: expected {total_rgba}, got {}",
            rgba.len()
        ));
    }

    // Build raw filtered scanlines (`0` filter type per row).
    let raw_len = (row_bytes + 1) * usize::try_from(height).unwrap_or(0);
    let mut raw = Vec::with_capacity(raw_len);
    for row in rgba.chunks_exact(row_bytes) {
        raw.push(0);
        raw.extend_from_slice(row);
    }

    let zlib = wrap_zlib_stored(&raw);
    let mut png = Vec::with_capacity(64 + zlib.len());
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 6; // RGBA
    write_png_chunk(&mut png, *b"IHDR", &ihdr);
    write_png_chunk(&mut png, *b"IDAT", &zlib);
    write_png_chunk(&mut png, *b"IEND", &[]);

    Ok(png)
}

fn wrap_zlib_stored(raw: &[u8]) -> Vec<u8> {
    const MAX_BLOCK: usize = 65_535;
    let block_count = raw.len().div_ceil(MAX_BLOCK).max(1);
    let mut out = Vec::with_capacity(6 + raw.len() + block_count * 5);
    // Zlib header: CMF=0x78 (DEFLATE, 32K window), FLG=0x01 (FCHECK makes header % 31 == 0)
    out.push(0x78);
    out.push(0x01);

    let mut offset = 0usize;
    while offset < raw.len() || offset == 0 {
        let end = (offset + MAX_BLOCK).min(raw.len());
        let is_final = end == raw.len();
        let len = u16::try_from(end - offset).unwrap_or(u16::MAX);
        out.push(u8::from(is_final)); // BFINAL | BTYPE=00
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&raw[offset..end]);
        offset = end;
        if is_final {
            break;
        }
    }

    let adler = adler32(raw);
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

fn write_png_chunk(out: &mut Vec<u8>, chunk_type: [u8; 4], data: &[u8]) {
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&chunk_type);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(&chunk_type);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in bytes {
        a = (a + u32::from(byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

/// Checks if a file path uses the expected `.png` extension.
#[must_use]
pub fn is_png_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_png_rgba_writes_valid_png_signature_and_chunks() -> Result<(), String> {
        // 2x2 RGBA pixels
        let rgba = vec![
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            255, 255, 255, 255, // white
        ];
        let png = encode_png_rgba(2, 2, &rgba)?;
        assert_eq!(&png[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        assert!(is_png_path(Path::new(&default_screenshot_filename())));
        assert!(is_png_path(&default_screenshot_path()));
        Ok(())
    }

    #[test]
    fn clamp_capture_region_bounds_inside_output() {
        let out = Size::from((1280, 800));
        let full = clamp_capture_region(out, None);
        assert_eq!(full.size.w, 1280);
        assert_eq!(full.size.h, 800);

        let cropped = clamp_capture_region(out, Some(Rect::new(1100, 700, 300, 200)));
        assert_eq!(cropped.loc.x, 1100);
        assert_eq!(cropped.loc.y, 700);
        assert_eq!(cropped.size.w, 180);
        assert_eq!(cropped.size.h, 100);
    }

    #[test]
    fn samples_rgba_pixels_and_clamps_buffer_coordinates() -> Result<(), String> {
        let rgba = vec![
            255, 0, 0, 255, // (0, 0)
            0, 255, 0, 255, // (1, 0)
            0, 0, 255, 255, // (0, 1)
            255, 128, 64, 255, // (1, 1)
        ];
        assert_eq!(sample_rgba_pixel(2, 2, &rgba, 0, 0)?, (1.0, 0.0, 0.0));
        assert_eq!(
            sample_rgba_pixel(2, 2, &rgba, 9, 9)?,
            (1.0, 128.0 / 255.0, 64.0 / 255.0)
        );
        assert_eq!(
            logical_point_to_buffer_pixel(100.0, 50.0, 2.0, 400, 400),
            (200, 100)
        );
        assert_eq!(
            logical_point_to_buffer_pixel(-5.0, 999.0, 1.5, 400, 400),
            (0, 399)
        );
        assert!(is_png_path(&unique_screenshot_path()));
        Ok(())
    }
}
