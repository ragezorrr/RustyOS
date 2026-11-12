//! Wayland layer-shell client integration for `rusty-shell` (`RUSTYOS-017`).
//!
//! Connects to `rusty-compositor` via Wayland protocol and maps native
//! `TopBar` and `Dock` surfaces using `zwlr_layer_shell_v1`.

use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_seat,
    delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    backend::WaylandError,
    globals::{BindError, registry_queue_init},
    protocol::{wl_output, wl_seat, wl_shm, wl_surface},
};

use crate::DesktopShell;

/// Error during Wayland client connection or rendering.
#[derive(Debug)]
pub enum WaylandClientError {
    /// Failed to connect to Wayland display.
    Connect(wayland_client::ConnectError),
    /// Failed to initialize registry queue.
    Global(wayland_client::globals::GlobalError),
    /// Failed to bind required global protocol.
    Bind(BindError),
    /// Protocol or dispatch error.
    Dispatch(wayland_client::DispatchError),
    /// Socket flush error.
    Flush(WaylandError),
    /// SHM pool allocation failure.
    ShmPool(String),
}

impl fmt::Display for WaylandClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(err) => write!(f, "Wayland connection failed: {err}"),
            Self::Global(err) => write!(f, "Wayland globals initialization failed: {err}"),
            Self::Bind(err) => write!(f, "Wayland protocol bind failed: {err}"),
            Self::Dispatch(err) => write!(f, "Wayland dispatch failed: {err}"),
            Self::Flush(err) => write!(f, "Wayland flush failed: {err}"),
            Self::ShmPool(msg) => write!(f, "SHM pool allocation failed: {msg}"),
        }
    }
}

impl Error for WaylandClientError {}

impl From<wayland_client::ConnectError> for WaylandClientError {
    fn from(err: wayland_client::ConnectError) -> Self {
        Self::Connect(err)
    }
}

impl From<wayland_client::globals::GlobalError> for WaylandClientError {
    fn from(err: wayland_client::globals::GlobalError) -> Self {
        Self::Global(err)
    }
}

impl From<BindError> for WaylandClientError {
    fn from(err: BindError) -> Self {
        Self::Bind(err)
    }
}

impl From<wayland_client::DispatchError> for WaylandClientError {
    fn from(err: wayland_client::DispatchError) -> Self {
        Self::Dispatch(err)
    }
}

impl From<WaylandError> for WaylandClientError {
    fn from(err: WaylandError) -> Self {
        Self::Flush(err)
    }
}

/// Premultiplied ARGB8888 design token colors (`DESIGN.md` v2.0 "RustyOS Purple Glass").
pub mod colors {
    // 0xAARRGGBB in little-endian premultiplied byte representation: [B, G, R, A]
    pub const SURFACE_ELEVATED: [u8; 4] = [0x2C, 0x17, 0x1E, 0xF5]; // #20182E @ 96%
    pub const SURFACE_OVERLAY: [u8; 4] = [0x36, 0x1F, 0x28, 0xFA]; // #292037 @ 98%
    pub const BORDER_HIGHLIGHT: [u8; 4] = [0x38, 0x38, 0x38, 0x38]; // 22% white glass rim
    pub const BORDER_SUBTLE: [u8; 4] = [0x1F, 0x1F, 0x1F, 0x1F]; // 12% white
    pub const ACCENT_NEON: [u8; 4] = [0xFF, 0x6E, 0xA5, 0xFF]; // #A56EFF
    pub const ACCENT_MUTED: [u8; 4] = [0xB5, 0x50, 0x76, 0xFF]; // #7650B5
    pub const TEXT_BRIGHT: [u8; 4] = [0xFA, 0xF0, 0xF4, 0xFF]; // #F4F0FA
    pub const TEXT_MUTED: [u8; 4] = [0xB5, 0x9B, 0xA6, 0xFF]; // #A69BB5
    pub const TEXT_SUBTLE: [u8; 4] = [0x85, 0x6B, 0x76, 0xFF]; // #766B85
    pub const SUCCESS_GREEN: [u8; 4] = [0xA0, 0xD6, 0x83, 0xFF]; // #83D6A0
}

/// Internal state for SCTK event dispatch.
pub struct ShellClientData {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub seat_state: SeatState,
    pub compositor_state: CompositorState,
    pub shm: Shm,
    pub layer_shell: LayerShell,
    pub pool: SlotPool,

    pub top_bar_surface: Option<LayerSurface>,
    pub dock_surface: Option<LayerSurface>,
    pub launcher_surface: Option<LayerSurface>,
    pub top_bar_size: (u32, u32),
    pub dock_size: (u32, u32),
    pub launcher_size: (u32, u32),

    pub shell_state: DesktopShell,
    pub running: bool,
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for ShellClientData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellClientData")
            .field("top_bar_size", &self.top_bar_size)
            .field("dock_size", &self.dock_size)
            .field("running", &self.running)
            .finish_non_exhaustive()
    }
}

fn fill_rect(
    canvas: &mut [u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    color: [u8; 4],
) {
    for row in y..(y + h) {
        let row_offset = row * stride;
        for col in x..(x + w) {
            let px = row_offset + col * 4;
            if px + 4 <= canvas.len() {
                canvas[px..px + 4].copy_from_slice(&color);
            }
        }
    }
}

fn inside_rounded_box(px: usize, py: usize, w: usize, h: usize, radius: usize) -> bool {
    if px >= w || py >= h {
        return false;
    }
    let r = radius.min(w / 2).min(h / 2);
    if r == 0 {
        return true;
    }
    let dx = if px < r {
        r - 1 - px
    } else {
        px.saturating_sub(w - r)
    };
    let dy = if py < r {
        r - 1 - py
    } else {
        py.saturating_sub(h - r)
    };
    if dx == 0 || dy == 0 {
        return true;
    }
    dx * dx + dy * dy <= r * r
}

#[allow(clippy::too_many_arguments)]
fn fill_rounded_rect(
    canvas: &mut [u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    radius: usize,
    color: [u8; 4],
) {
    for ly in 0..h {
        let row_offset = (y + ly) * stride;
        for lx in 0..w {
            if inside_rounded_box(lx, ly, w, h, radius) {
                let px = row_offset + (x + lx) * 4;
                if px + 4 <= canvas.len() {
                    canvas[px..px + 4].copy_from_slice(&color);
                }
            }
        }
    }
}

fn fill_rounded_rect_with_rim(
    canvas: &mut [u8],
    stride: usize,
    w: usize,
    h: usize,
    radius: usize,
    fill: [u8; 4],
    rim: [u8; 4],
) {
    for py in 0..h {
        let row_offset = py * stride;
        for px in 0..w {
            if !inside_rounded_box(px, py, w, h, radius) {
                continue;
            }
            let is_interior = px > 0
                && py > 0
                && w > 2
                && h > 2
                && inside_rounded_box(px - 1, py - 1, w - 2, h - 2, radius.saturating_sub(1));
            let col = if is_interior { fill } else { rim };
            let off = row_offset + px * 4;
            if off + 4 <= canvas.len() {
                canvas[off..off + 4].copy_from_slice(&col);
            }
        }
    }
}

const BUNDLED_INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const BUNDLED_INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

fn system_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        const CANDIDATES: &[&str] = &[
            "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
        ];
        if let Ok(env_path) = std::env::var("RUSTYOS_FONT_PATH")
            && let Ok(bytes) = std::fs::read(env_path)
            && let Ok(font) = Font::from_bytes(bytes, FontSettings::default())
        {
            return Some(font);
        }
        if let Ok(font) = Font::from_bytes(BUNDLED_INTER_REGULAR, FontSettings::default()) {
            return Some(font);
        }
        for path in CANDIDATES {
            if Path::new(path).exists()
                && let Ok(bytes) = std::fs::read(path)
                && let Ok(font) = Font::from_bytes(bytes, FontSettings::default())
            {
                return Some(font);
            }
        }
        None
    })
    .as_ref()
}

fn bold_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| Font::from_bytes(BUNDLED_INTER_SEMIBOLD, FontSettings::default()).ok())
        .as_ref()
        .or_else(system_font)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::too_many_arguments
)]
fn draw_text_argb8888(
    canvas: &mut [u8],
    stride: usize,
    canvas_w: usize,
    canvas_h: usize,
    text: &str,
    size_px: f32,
    rgba: [u8; 4],
    origin_x: i32,
    origin_y: i32,
    centered_in_box: Option<(usize, usize)>,
) {
    draw_text_weighted_argb8888(
        canvas,
        stride,
        canvas_w,
        canvas_h,
        text,
        false,
        size_px,
        rgba,
        origin_x,
        origin_y,
        centered_in_box,
    );
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::too_many_arguments
)]
fn draw_text_weighted_argb8888(
    canvas: &mut [u8],
    stride: usize,
    canvas_w: usize,
    canvas_h: usize,
    text: &str,
    bold: bool,
    size_px: f32,
    rgba: [u8; 4],
    origin_x: i32,
    origin_y: i32,
    centered_in_box: Option<(usize, usize)>,
) -> usize {
    let Some(metric_font) = system_font() else {
        return 0;
    };
    let font = if bold {
        bold_font().unwrap_or(metric_font)
    } else {
        metric_font
    };
    if text.is_empty() {
        return 0;
    }

    let line_metrics = metric_font.horizontal_line_metrics(size_px);
    let ascent = line_metrics.map_or(size_px * 0.8, |m| m.ascent);
    let descent = line_metrics.map_or(-size_px * 0.2, |m| m.descent);
    let line_h = (ascent - descent).ceil().max(size_px);

    let mut glyphs = Vec::with_capacity(text.len());
    let mut total_w = 0.0f32;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, size_px);
        glyphs.push((total_w, metrics, bitmap));
        total_w += metrics.advance_width;
    }

    let (base_x, base_y) = if let Some((bw, bh)) = centered_in_box {
        let bx = origin_x as f32 + ((bw as f32 - total_w) * 0.5);
        let by = origin_y as f32 + ((bh as f32 - line_h) * 0.5) + ascent;
        (bx, by)
    } else {
        (origin_x as f32, origin_y as f32 + ascent)
    };

    let [r, g, b, a] = rgba;
    for (adv_x, metrics, bitmap) in &glyphs {
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        let gx = (base_x + adv_x + metrics.xmin as f32).round() as i32;
        let gy = (base_y - metrics.ymin as f32 - metrics.height as f32).round() as i32;

        for row in 0..metrics.height {
            let dst_y = gy + row as i32;
            if dst_y < 0 || dst_y >= canvas_h as i32 {
                continue;
            }
            for col in 0..metrics.width {
                let dst_x = gx + col as i32;
                if dst_x < 0 || dst_x >= canvas_w as i32 {
                    continue;
                }
                let cov = bitmap[row * metrics.width + col];
                if cov == 0 {
                    continue;
                }
                let idx = (dst_y as usize) * stride + (dst_x as usize) * 4;
                if idx + 4 > canvas.len() {
                    continue;
                }
                let alpha = (u16::from(cov) * u16::from(a)) / 255;
                let src_b = ((u16::from(b) * alpha) / 255) as u8;
                let src_g = ((u16::from(g) * alpha) / 255) as u8;
                let src_r = ((u16::from(r) * alpha) / 255) as u8;
                let inv_a = 255 - alpha;

                canvas[idx] = src_b.saturating_add(((u16::from(canvas[idx]) * inv_a) / 255) as u8);
                canvas[idx + 1] =
                    src_g.saturating_add(((u16::from(canvas[idx + 1]) * inv_a) / 255) as u8);
                canvas[idx + 2] =
                    src_r.saturating_add(((u16::from(canvas[idx + 2]) * inv_a) / 255) as u8);
                canvas[idx + 3] = (alpha as u8)
                    .saturating_add(((u16::from(canvas[idx + 3]) * inv_a) / 255) as u8);
            }
        }
    }
    total_w.ceil() as usize
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn measure_text_width_px(text: &str, bold: bool, size_px: f32) -> usize {
    let Some(base) = system_font() else {
        return text.len() * 8;
    };
    let font = if bold {
        bold_font().unwrap_or(base)
    } else {
        base
    };
    let w: f32 = text
        .chars()
        .map(|ch| font.metrics(ch, size_px).advance_width)
        .sum();
    (w.ceil() as usize).max(1)
}

/// Renders transparent macOS-style `TopBar` into an ARGB8888 buffer slice (`RUSTYOS-049`).
pub fn render_top_bar_pixels(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    top_bar: &crate::TopBarState,
) {
    let stride = (width * 4) as usize;
    let w = width as usize;
    let h = height as usize;
    let item_h = h.saturating_sub(4).max(18);
    let item_y = h.saturating_sub(item_h) / 2;

    canvas.fill(0);

    // Monochrome chamfered hexagon emblem placeholder at left edge (x = 12)
    if h >= 20 && w >= 40 {
        let cy = h / 2;
        fill_rounded_rect(
            canvas,
            stride,
            13,
            cy.saturating_sub(6),
            12,
            12,
            4,
            [252, 250, 255, 255],
        );
    }

    let pure_white = [252, 250, 255, 255];
    let menu_white = [245, 242, 252, 245];
    let font_px = 13.0;
    let mut cursor_x = 36_usize;

    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    {
        let app_adv = draw_text_weighted_argb8888(
            canvas,
            stride,
            w,
            h,
            "RustyOS",
            true,
            font_px,
            pure_white,
            cursor_x as i32,
            item_y as i32,
            Some((measure_text_width_px("RustyOS", true, font_px) + 6, item_h)),
        );
        cursor_x += app_adv + 16;

        for label in ["File", "Edit", "View", "Go", "Window", "Help"] {
            let box_w = measure_text_width_px(label, false, font_px) + 12;
            if cursor_x + box_w > w.saturating_sub(180) {
                break;
            }
            let _ = draw_text_weighted_argb8888(
                canvas,
                stride,
                w,
                h,
                label,
                false,
                font_px,
                menu_white,
                cursor_x as i32,
                item_y as i32,
                Some((box_w, item_h)),
            );
            cursor_x += box_w + 2;
        }

        if w > 180 {
            let clock_w = measure_text_width_px(&top_bar.clock_label, false, font_px) + 8;
            let clock_x = w.saturating_sub(clock_w + 12);
            let _ = draw_text_weighted_argb8888(
                canvas,
                stride,
                w,
                h,
                &top_bar.clock_label,
                false,
                font_px,
                pure_white,
                clock_x as i32,
                item_y as i32,
                Some((clock_w, item_h)),
            );
        }
    }
}

/// Renders `Dock` into an ARGB8888 stadium capsule buffer according to `DockItem` entries (`RUSTYOS-048`).
pub fn render_dock_pixels(canvas: &mut [u8], width: u32, height: u32, dock: &[crate::DockItem]) {
    let stride = (width * 4) as usize;
    let w = width as usize;
    let h = height as usize;

    canvas.fill(0);

    // Concentric rounded glass Dock container without heavy capsule ends (`RUSTYOS-050`)
    let radius = 20.min(h / 2).max(1);
    fill_rounded_rect_with_rim(
        canvas,
        stride,
        w,
        h,
        radius,
        colors::SURFACE_ELEVATED,
        colors::BORDER_HIGHLIGHT,
    );

    // Dock item squircles without resting grey backing boxes (`RUSTYOS-050`)
    let item_size: usize = h.saturating_sub(18).clamp(28, 50);
    let item_y: usize = 6;
    let item_gap: usize = 6;
    let divider_w: usize = 12;
    let pinned_count: usize = 6;
    let left_count = pinned_count + dock.len();
    let max_slots = w
        .saturating_sub(16 + divider_w)
        .checked_div(item_size + item_gap)
        .unwrap_or(0);
    let total_items = (left_count + 2).min(max_slots);
    let mut item_x = 10;

    for i in 0..total_items {
        if i == left_count {
            fill_rect(
                canvas,
                stride,
                item_x + divider_w / 2 - 3,
                item_y + 4,
                1,
                item_size.saturating_sub(8),
                colors::BORDER_HIGHLIGHT,
            );
            item_x += divider_w;
        }

        let icon_color: [u8; 4] = match i {
            0 => [0xF8, 0x94, 0x2E, 0xFF], // Files sky blue [B,G,R,A]
            1 => [0xF2, 0xEE, 0xEC, 0xFF], // Overview silver grid card
            2 => [0x3C, 0x2C, 0x2E, 0xFF], // Terminal dark slate-indigo
            3 => [0xFA, 0xF6, 0xFF, 0xFF], // Browser compass white
            4 => [0x24, 0xD6, 0xFF, 0xFF], // Notes warm amber/cream
            5 => [0xBE, 0xB6, 0xB2, 0xFF], // Settings metallic gear
            n if n == left_count => [0xEC, 0x8C, 0x26, 0xFF], // Downloads blue folder
            n if n == left_count + 1 => [0xD4, 0xCE, 0xC8, 0xFF], // Trash silver cylinder
            _ => colors::ACCENT_MUTED,     // Running app squircle
        };
        let corner_r = (item_size / 4).max(6);
        fill_rounded_rect(
            canvas, stride, item_x, item_y, item_size, item_size, corner_r, icon_color,
        );

        // Minimal 4x4 circular active indicator dot under Files and running windows
        if i == 0 || (i >= pinned_count && i < left_count) {
            fill_rounded_rect(
                canvas,
                stride,
                item_x + (item_size - 4) / 2,
                h.saturating_sub(6),
                4,
                4,
                2,
                [0xFF, 0xF6, 0xF8, 0xF0],
            );
        }

        item_x += item_size + item_gap;
    }
}

/// Renders Spotlight application launcher into an ARGB8888 buffer slice.
#[allow(clippy::too_many_lines)]
pub fn render_launcher_pixels(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    launcher: &crate::AppLauncher,
) {
    let stride = (width * 4) as usize;
    let w = width as usize;
    let h = height as usize;

    // Outer glass highlight rim
    fill_rect(canvas, stride, 0, 0, w, h, colors::BORDER_HIGHLIGHT);

    // Modal background body (elevated glass surface)
    if w > 2 && h > 2 {
        fill_rect(canvas, stride, 1, 1, w - 2, h - 2, colors::SURFACE_ELEVATED);
    }

    // Search bar container
    let search_bar_h = 44;
    fill_rect(
        canvas,
        stride,
        16,
        16,
        w.saturating_sub(32),
        search_bar_h,
        colors::SURFACE_OVERLAY,
    );
    fill_rect(
        canvas,
        stride,
        16,
        16 + search_bar_h - 1,
        w.saturating_sub(32),
        1,
        colors::BORDER_SUBTLE,
    );

    // Search accent icon
    fill_rect(canvas, stride, 28, 26, 24, 24, colors::ACCENT_NEON);

    // Cursor indicator
    fill_rect(canvas, stride, 62, 26, 2, 24, colors::TEXT_BRIGHT);

    let has_font = system_font().is_some();
    if has_font {
        let prompt_text = if launcher.query.is_empty() {
            "Search RustyOS..."
        } else {
            &launcher.query
        };
        let prompt_color = if launcher.query.is_empty() {
            [166, 155, 181, 255]
        } else {
            [244, 240, 250, 255]
        };
        draw_text_argb8888(
            canvas,
            stride,
            w,
            h,
            prompt_text,
            14.5,
            prompt_color,
            70,
            28,
            None,
        );
    }

    // Divider line
    fill_rect(
        canvas,
        stride,
        16,
        70,
        w.saturating_sub(32),
        1,
        colors::BORDER_SUBTLE,
    );

    // Filtered entries
    let entries = launcher.filtered_entries();
    let row_h = 44;
    let start_y = 80;
    let max_rows = 3.min(entries.len());

    for (i, entry) in entries.iter().take(max_rows).enumerate() {
        let row_y = start_y + i * (row_h + 4);
        let is_selected = i == 0;
        let bg_color = if is_selected {
            colors::SURFACE_OVERLAY
        } else {
            colors::SURFACE_ELEVATED
        };
        fill_rect(
            canvas,
            stride,
            16,
            row_y,
            w.saturating_sub(32),
            row_h,
            bg_color,
        );

        if is_selected {
            fill_rect(canvas, stride, 16, row_y, 3, row_h, colors::ACCENT_NEON);
        }

        // App icon badge
        let icon_color = match i {
            0 => colors::ACCENT_NEON,
            1 => colors::ACCENT_MUTED,
            _ => colors::TEXT_SUBTLE,
        };
        fill_rect(canvas, stride, 28, row_y + 10, 24, 24, icon_color);

        if has_font {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let ry = row_y as i32;
            draw_text_argb8888(
                canvas,
                stride,
                w,
                h,
                &entry.name,
                13.5,
                [244, 240, 250, 255],
                62,
                ry + 6,
                None,
            );
            draw_text_argb8888(
                canvas,
                stride,
                w,
                h,
                &entry.exec,
                10.5,
                [166, 155, 181, 255],
                62,
                ry + 24,
                None,
            );
        } else {
            // App title and subtitle indicators fallback
            let title_w = (entry.name.len() * 8).clamp(60, 180);
            fill_rect(
                canvas,
                stride,
                62,
                row_y + 12,
                title_w,
                8,
                colors::TEXT_BRIGHT,
            );
            fill_rect(canvas, stride, 62, row_y + 26, 90, 6, colors::TEXT_MUTED);
        }
    }
}

impl ShellClientData {
    /// Renders `TopBar` into an ARGB8888 buffer slice.
    pub fn render_top_bar_pixels(&self, canvas: &mut [u8], width: u32, height: u32) {
        render_top_bar_pixels(canvas, width, height, &self.shell_state.top_bar);
    }

    /// Renders `Dock` into an ARGB8888 buffer slice.
    pub fn render_dock_pixels(&self, canvas: &mut [u8], width: u32, height: u32) {
        render_dock_pixels(canvas, width, height, &self.shell_state.dock);
    }

    /// Renders Spotlight application launcher into an ARGB8888 buffer slice.
    pub fn render_launcher_pixels(&self, canvas: &mut [u8], width: u32, height: u32) {
        render_launcher_pixels(canvas, width, height, &self.shell_state.launcher);
    }
}

// SCTK Delegate Implementations
impl ProvidesRegistryState for ShellClientData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(ShellClientData);
delegate_output!(ShellClientData);
delegate_compositor!(ShellClientData);
delegate_shm!(ShellClientData);
delegate_seat!(ShellClientData);
delegate_layer!(ShellClientData);

impl OutputHandler for ShellClientData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl CompositorHandler for ShellClientData {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_scale: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wayland_client::protocol::wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl ShmHandler for ShellClientData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for ShellClientData {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl LayerShellHandler for ShellClientData {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.running = false;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (mut width, mut height) = configure.new_size;

        if let Some(top_bar) = &self.top_bar_surface
            && top_bar == layer
        {
            if width == 0 {
                width = 1280;
            }
            if height == 0 {
                height = 24;
            }
            self.top_bar_size = (width, height);
        }

        if let Some(dock) = &self.dock_surface
            && dock == layer
        {
            if width == 0 {
                width = 240;
            }
            if height == 0 {
                height = 56;
            }
            self.dock_size = (width, height);
        }

        if let Some(launcher) = &self.launcher_surface
            && launcher == layer
        {
            if width == 0 {
                width = 560;
            }
            if height == 0 {
                height = 240;
            }
            self.launcher_size = (width, height);
        }
    }
}

/// Standalone Wayland Layer-Shell client managing `TopBar` and `Dock`.
pub struct ShellWaylandClient {
    pub conn: Connection,
    pub queue: wayland_client::EventQueue<ShellClientData>,
    pub data: ShellClientData,
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for ShellWaylandClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellWaylandClient")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl ShellWaylandClient {
    /// Connects to Wayland and initializes layer-shell globals and surfaces.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandClientError`] if the connection fails or required protocols are absent.
    pub fn connect(initial_shell: DesktopShell) -> Result<Self, WaylandClientError> {
        let conn = Connection::connect_to_env()?;
        let (globals, queue) = registry_queue_init(&conn)?;
        let qh = queue.handle();

        let compositor_state = CompositorState::bind(&globals, &qh)?;
        let layer_shell = LayerShell::bind(&globals, &qh)?;
        let shm = Shm::bind(&globals, &qh)?;
        let pool = SlotPool::new(1280 * 720 * 4, &shm)
            .map_err(|e| WaylandClientError::ShmPool(format!("{e:?}")))?;

        let mut data = ShellClientData {
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &qh),
            seat_state: SeatState::new(&globals, &qh),
            compositor_state,
            shm,
            layer_shell,
            pool,
            top_bar_surface: None,
            dock_surface: None,
            launcher_surface: None,
            top_bar_size: (1280, 32),
            dock_size: (488, 68),
            launcher_size: (560, 240),
            shell_state: initial_shell,
            running: true,
        };

        // Create TopBar layer surface
        let top_surface = data.compositor_state.create_surface(&qh);
        let top_layer = data.layer_shell.create_layer_surface(
            &qh,
            top_surface,
            Layer::Top,
            Some("topbar".to_owned()),
            None,
        );
        top_layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        top_layer.set_size(0, 24);
        top_layer.set_exclusive_zone(24);
        top_layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        top_layer.commit();
        data.top_bar_surface = Some(top_layer);

        // Create Dock layer surface
        let dock_surface = data.compositor_state.create_surface(&qh);
        let dock_layer = data.layer_shell.create_layer_surface(
            &qh,
            dock_surface,
            Layer::Bottom,
            Some("dock".to_owned()),
            None,
        );
        dock_layer.set_anchor(Anchor::BOTTOM);
        dock_layer.set_size(488, 68);
        dock_layer.set_margin(0, 0, 8, 0);
        dock_layer.set_exclusive_zone(68);
        dock_layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        dock_layer.commit();
        data.dock_surface = Some(dock_layer);

        // Create Spotlight Launcher layer surface (Layer::Overlay, centered)
        let launcher_surface = data.compositor_state.create_surface(&qh);
        let launcher_layer = data.layer_shell.create_layer_surface(
            &qh,
            launcher_surface,
            Layer::Overlay,
            Some("spotlight".to_owned()),
            None,
        );
        launcher_layer.set_anchor(Anchor::empty());
        launcher_layer.set_size(560, 240);
        launcher_layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        launcher_layer.commit();
        data.launcher_surface = Some(launcher_layer);

        Ok(Self { conn, queue, data })
    }

    /// Dispatches pending Wayland protocol events.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandClientError`] on socket read or protocol dispatch error.
    pub fn dispatch_pending(&mut self) -> Result<usize, WaylandClientError> {
        self.queue
            .dispatch_pending(&mut self.data)
            .map_err(Into::into)
    }

    /// Renders current `TopBar` and `Dock` frames into SHM buffers and commits them.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandClientError`] if buffer allocation or commitment fails.
    #[allow(clippy::cast_possible_wrap)]
    pub fn draw_surfaces(&mut self) -> Result<(), WaylandClientError> {
        // Draw TopBar
        if let Some(top_bar) = &self.data.top_bar_surface {
            let (w, h) = self.data.top_bar_size;
            let stride = (w * 4) as i32;
            let (buffer, canvas) = self
                .data
                .pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
                .map_err(|e| WaylandClientError::ShmPool(format!("{e:?}")))?;

            render_top_bar_pixels(canvas, w, h, &self.data.shell_state.top_bar);
            top_bar.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
            top_bar.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
            top_bar.commit();
        }

        // Draw Dock
        if let Some(dock) = &self.data.dock_surface {
            let (w, h) = self.data.dock_size;
            let stride = (w * 4) as i32;
            let (buffer, canvas) = self
                .data
                .pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
                .map_err(|e| WaylandClientError::ShmPool(format!("{e:?}")))?;

            render_dock_pixels(canvas, w, h, &self.data.shell_state.dock);
            dock.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
            dock.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
            dock.commit();
        }

        // Draw Spotlight Launcher
        if let Some(launcher) = &self.data.launcher_surface {
            if self.data.shell_state.launcher.open {
                let (w, h) = self.data.launcher_size;
                let stride = (w * 4) as i32;
                let (buffer, canvas) = self
                    .data
                    .pool
                    .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
                    .map_err(|e| WaylandClientError::ShmPool(format!("{e:?}")))?;

                render_launcher_pixels(canvas, w, h, &self.data.shell_state.launcher);
                launcher.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
                launcher
                    .wl_surface()
                    .damage_buffer(0, 0, w as i32, h as i32);
                launcher.commit();
            } else {
                launcher.wl_surface().attach(None, 0, 0);
                launcher.commit();
            }
        }

        self.conn.flush()?;
        Ok(())
    }

    /// Synchronizes shell state and triggers a redraw.
    pub fn update_state(&mut self, state: DesktopShell) {
        self.data.shell_state = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_top_bar_pixels_generates_valid_surface_buffer() {
        let shell = DesktopShell::default();
        let (w, h) = (1280u32, 24u32);
        let mut buffer = vec![0u8; (w * h * 4) as usize];

        render_top_bar_pixels(&mut buffer, w, h, &shell.top_bar);

        // Transparent Menu Bar by default (zero solid backdrop / border scrim)
        assert_eq!(buffer[0..4], [0, 0, 0, 0]);
        let border_offset = ((h - 1) * w * 4) as usize;
        assert_eq!(buffer[border_offset..border_offset + 4], [0, 0, 0, 0]);
        // Monochrome emblem and Inter Menu Bar labels rendered into the buffer
        assert!(buffer.as_chunks::<4>().0.iter().any(|px| px[3] > 200));
    }

    #[test]
    fn render_dock_pixels_generates_valid_glass_rim_and_tiles() {
        let shell = DesktopShell::default();
        let (w, h) = (488u32, 68u32);
        let mut buffer = vec![0u8; (w * h * 4) as usize];

        render_dock_pixels(&mut buffer, w, h, &shell.dock);

        // Corners outside concentric Dock radius remain transparent
        assert_eq!(buffer[0..4], [0, 0, 0, 0]);
        // Top-center edge matches BORDER_HIGHLIGHT glass rim
        let top_rim_px = ((w / 2) * 4) as usize;
        assert_eq!(buffer[top_rim_px..top_rim_px + 4], colors::BORDER_HIGHLIGHT);
        // Interior glass body pixel above tiles matches SURFACE_ELEVATED
        let interior_px = ((w * 4) * 2 + (w / 2) * 4) as usize;
        assert_eq!(
            buffer[interior_px..interior_px + 4],
            colors::SURFACE_ELEVATED
        );
        // Active running dot sits centered under the first (Files) tile
        let dot_px = ((w * 4) * (h - 5) + 35 * 4) as usize;
        assert_eq!(buffer[dot_px..dot_px + 4], [0xFF, 0xF6, 0xF8, 0xF0]);
    }

    #[test]
    fn render_launcher_pixels_generates_valid_modal_and_search_bar() {
        let shell = DesktopShell::default();
        let (w, h) = (560u32, 240u32);
        let mut buffer = vec![0u8; (w * h * 4) as usize];

        render_launcher_pixels(&mut buffer, w, h, &shell.launcher);

        // Verify top-left corner matches BORDER_HIGHLIGHT rim
        assert_eq!(buffer[0..4], colors::BORDER_HIGHLIGHT);
        // Verify interior pixel matches SURFACE_ELEVATED
        let interior_px = (w * 4 * 2 + 8) as usize;
        assert_eq!(
            buffer[interior_px..interior_px + 4],
            colors::SURFACE_ELEVATED
        );
    }
}
