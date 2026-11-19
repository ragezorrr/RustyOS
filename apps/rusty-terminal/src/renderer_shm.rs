//! Pure-CPU `ARGB8888` premultiplied `wl_shm` fallback renderer matching macOS Tahoe GPU styling.

use std::collections::HashMap;

use crate::font::{FontSet, GlyphKey, GlyphKind, RasterizedGlyph};
use crate::gpu_egl::truncate_to_fit;
use crate::layout::{HitTarget, Rect, TerminalLayout, WINDOW_CORNER_RADIUS};
use crate::state::{TerminalAppState, TerminalTab};

/// Software glyph cache reused across `wl_shm` fallback frames.
#[derive(Debug, Default)]
pub(crate) struct ShmFallbackRenderer {
    glyphs: HashMap<GlyphKey, Option<RasterizedGlyph>>,
}

impl ShmFallbackRenderer {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Renders `app` into a premultiplied BGRA8 (`ARGB8888` little-endian) buffer (`width * height * 4`).
    pub(crate) fn render(&mut self, app: &TerminalAppState, canvas: &mut [u8], stride_px: i32) {
        let layout = app.layout();
        let w = layout.win_w;
        let h = layout.win_h;
        if w <= 0 || h <= 0 || canvas.len() < (stride_px.max(w) * h * 4) as usize {
            return;
        }
        canvas.fill(0);

        fill_rounded_rect_premul(
            canvas,
            stride_px,
            w,
            h,
            Rect::new(0, 0, w, h),
            WINDOW_CORNER_RADIUS,
            app.theme.window_bg,
            Some(app.theme.window_border),
        );
        fill_rounded_header(
            canvas,
            stride_px,
            w,
            h,
            layout.header_rect,
            WINDOW_CORNER_RADIUS,
            app.theme.header_bg,
        );
        blend_rect(
            canvas,
            stride_px,
            w,
            h,
            Rect::new(0, layout.header_rect.h - 1, w, 1),
            app.theme.header_separator,
            None,
        );

        self.render_grid(app, &layout, canvas, stride_px);
        self.render_chrome(app, &layout, canvas, stride_px);
    }

    fn render_grid(
        &mut self,
        app: &TerminalAppState,
        layout: &TerminalLayout,
        canvas: &mut [u8],
        stride_px: i32,
    ) {
        let Some(tab) = app.active_tab() else {
            return;
        };
        let cw = app.metrics.cell_w;
        let ch = app.metrics.cell_h;
        let sb_len = tab.engine.scrollback_len();
        let whole_back = tab.scroll_lines.current().floor().max(0.0) as usize;
        let top_abs = sb_len.saturating_sub(whole_back.min(sb_len));
        let (cur_col, cur_row) = tab.engine.cursor_pos();
        let cur_abs = sb_len + cur_row;

        for r in 0..usize::from(layout.rows) {
            let abs_line = top_abs + r;
            let Some(cells) = tab.engine.row_cells(abs_line) else {
                continue;
            };
            let py = layout.grid_rect.y + (r as i32) * ch;
            for (c, cell) in cells.iter().enumerate().take(usize::from(layout.cols)) {
                let px = layout.grid_rect.x + (c as i32) * cw;
                self.render_shm_cell(
                    app,
                    tab,
                    layout,
                    canvas,
                    stride_px,
                    cell,
                    abs_line,
                    c,
                    px,
                    py,
                    abs_line == cur_abs && c == cur_col && tab.engine.modes.cursor_visible,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_shm_cell(
        &mut self,
        app: &TerminalAppState,
        tab: &TerminalTab,
        layout: &TerminalLayout,
        canvas: &mut [u8],
        stride_px: i32,
        cell: &crate::vt::Cell,
        abs_line: usize,
        col: usize,
        px: i32,
        py: i32,
        is_cursor: bool,
    ) {
        let cw = app.metrics.cell_w;
        let ch = app.metrics.cell_h;
        let clip = layout.grid_rect;
        let cell_rect = Rect::new(px, py, cw, ch);
        let mut fg = app.theme.resolve_fg(cell.fg, cell.attrs.bold);
        let mut bg = app.theme.resolve_bg(cell.bg);
        if cell.attrs.inverse {
            let old = fg;
            fg = bg.unwrap_or([
                app.theme.window_bg[0],
                app.theme.window_bg[1],
                app.theme.window_bg[2],
                255,
            ]);
            bg = Some(old);
        }
        if cell.attrs.dim {
            fg[3] = (u16::from(fg[3]) * 165 / 255) as u8;
        }
        if let Some(bg_c) = bg {
            blend_rect(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                cell_rect,
                bg_c,
                Some(clip),
            );
        }
        if tab.selection.is_some_and(|s| s.contains(abs_line, col)) {
            blend_rect(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                cell_rect,
                app.theme.selection_bg,
                Some(clip),
            );
        }
        if is_cursor && (!app.focused || app.cursor_blink_on) {
            let mut cur_c = app.theme.cursor_bg;
            if !app.focused {
                cur_c[3] = 100;
            }
            let cur_rect = match tab.engine.modes.cursor_style {
                crate::vt::CursorStyle::Bar => Rect::new(px, py + 1, 2, (ch - 2).max(4)),
                crate::vt::CursorStyle::Underline => Rect::new(px, py + ch - 2, cw, 2),
                crate::vt::CursorStyle::Block => {
                    if app.focused {
                        fg = app.theme.cursor_fg;
                    }
                    cell_rect
                }
            };
            blend_rect(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                cur_rect,
                cur_c,
                Some(clip),
            );
        }
        if cell.ch != ' ' && cell.ch != '\0' {
            let kind = if cell.attrs.bold {
                GlyphKind::MonoBold
            } else {
                GlyphKind::MonoRegular
            };
            let key = GlyphKey {
                ch: cell.ch,
                kind,
                size_tenths: (app.metrics.font_size * 10.0).round() as u16,
                cell_w: cw as u16,
                cell_h: ch as u16,
            };
            if let Some(g) = self.cached_glyph(&app.fonts, app.metrics.baseline_y, key) {
                blit_glyph_premul(
                    canvas,
                    stride_px,
                    layout.win_w,
                    layout.win_h,
                    &g,
                    px + g.offset_x,
                    py + g.offset_y,
                    fg,
                    Some(clip),
                );
            }
        }
    }

    fn render_chrome(
        &mut self,
        app: &TerminalAppState,
        layout: &TerminalLayout,
        canvas: &mut [u8],
        stride_px: i32,
    ) {
        self.render_chrome_buttons(app, layout, canvas, stride_px);
        self.render_chrome_tabs_and_title(app, layout, canvas, stride_px);
        if let Some(thumb) = layout.scrollbar_thumb {
            fill_rounded_rect_premul(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                thumb,
                3,
                app.theme.scrollbar_thumb,
                None,
            );
        }
    }

    fn render_chrome_buttons(
        &mut self,
        app: &TerminalAppState,
        layout: &TerminalLayout,
        canvas: &mut [u8],
        stride_px: i32,
    ) {
        for (rect, focused_c) in [
            (layout.close_rect, [0xFF, 0x5F, 0x57, 255]),
            (layout.minimize_rect, [0xFE, 0xBC, 0x2E, 255]),
            (layout.maximize_rect, [0x28, 0xC8, 0x40, 255]),
        ] {
            let c = if app.focused {
                focused_c
            } else {
                [110, 114, 122, 255]
            };
            fill_rounded_rect_premul(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                rect,
                7,
                c,
                None,
            );
        }

        for (target, rect, is_on, glyph) in [
            (
                HitTarget::GlassToggle,
                layout.glass_btn_rect,
                !app.theme.reduce_transparency,
                "\u{E001}",
            ),
            (
                HitTarget::SearchToggle,
                layout.search_btn_rect,
                app.search.open,
                "\u{E002}",
            ),
            (
                HitTarget::NewTabButton,
                layout.new_tab_btn_rect,
                false,
                "\u{E003}",
            ),
        ] {
            let fill = if is_on {
                [
                    app.theme.accent[0],
                    app.theme.accent[1],
                    app.theme.accent[2],
                    64,
                ]
            } else if app.hover == Some(target) {
                app.theme.tab_hover_bg
            } else {
                app.theme.badge_bg
            };
            fill_rounded_rect_premul(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                rect,
                7,
                fill,
                Some(app.theme.tab_border),
            );
            let tw = app.fonts.measure_ui_text(glyph, 14.0, true);
            let tx = rect.x + ((rect.w as f32 - tw) * 0.5).round() as i32;
            let ty = rect.y + app.fonts.ui_baseline_in_box(rect.h, 14.0);
            let tc = if is_on {
                app.theme.accent
            } else {
                app.theme.text_primary
            };
            self.draw_ui_text(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                &app.fonts,
                glyph,
                14.0,
                true,
                tx,
                ty,
                tc,
            );
        }
    }

    fn render_chrome_tabs_and_title(
        &mut self,
        app: &TerminalAppState,
        layout: &TerminalLayout,
        canvas: &mut [u8],
        stride_px: i32,
    ) {
        for (idx, tl) in layout.tab_rects.iter().enumerate() {
            let active = idx == app.active_tab;
            let fill = if active {
                app.theme.tab_active_bg
            } else {
                app.theme.badge_bg
            };
            let border = active.then_some(app.theme.tab_border);
            fill_rounded_rect_premul(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                tl.pill,
                8,
                fill,
                border,
            );
            if let Some(tab) = app.tabs.get(idx) {
                let label = truncate_to_fit(
                    &app.fonts,
                    &tab.display_title(),
                    12.0,
                    active,
                    tl.pill.w - 30,
                );
                let ty = tl.pill.y + app.fonts.ui_baseline_in_box(tl.pill.h, 12.0);
                self.draw_ui_text(
                    canvas,
                    stride_px,
                    layout.win_w,
                    layout.win_h,
                    &app.fonts,
                    &label,
                    12.0,
                    active,
                    tl.pill.x + 10,
                    ty,
                    app.theme.text_primary,
                );
            }
        }

        if layout.tab_rects.is_empty() {
            let base = app
                .active_tab()
                .map_or_else(|| String::from("Terminal"), TerminalTab::display_title);
            let label = format!("{base} — {}×{}", layout.cols, layout.rows);
            let tw = app.fonts.measure_ui_text(&label, app.metrics.ui_size, true);
            let tx =
                layout.title_rect.x + ((layout.title_rect.w as f32 - tw) * 0.5).max(0.0) as i32;
            let ty = layout.header_rect.y
                + app
                    .fonts
                    .ui_baseline_in_box(layout.header_rect.h, app.metrics.ui_size);
            self.draw_ui_text(
                canvas,
                stride_px,
                layout.win_w,
                layout.win_h,
                &app.fonts,
                &label,
                app.metrics.ui_size,
                true,
                tx,
                ty,
                app.theme.text_primary,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_ui_text(
        &mut self,
        canvas: &mut [u8],
        stride_px: i32,
        win_w: i32,
        win_h: i32,
        fonts: &FontSet,
        text: &str,
        size_px: f32,
        bold: bool,
        start_x: i32,
        baseline_y: i32,
        color: [u8; 4],
    ) {
        let kind = if bold {
            GlyphKind::UiBold
        } else {
            GlyphKind::UiRegular
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size_tenths = (size_px * 10.0).round() as u16;
        let mut cx = start_x as f32;
        for ch in text.chars() {
            if ch.is_whitespace() {
                cx += fonts.ui_char_advance(ch, size_px, bold);
                continue;
            }
            let key = GlyphKey {
                ch,
                kind,
                size_tenths,
                cell_w: 0,
                cell_h: 0,
            };
            if let Some(g) = self.cached_glyph(fonts, 0, key) {
                let gx = cx.round() as i32 + g.offset_x;
                blit_glyph_premul(
                    canvas,
                    stride_px,
                    win_w,
                    win_h,
                    &g,
                    gx,
                    baseline_y + g.offset_y,
                    color,
                    None,
                );
                cx += g.advance_x;
            }
        }
    }

    fn cached_glyph(
        &mut self,
        fonts: &FontSet,
        baseline_y: i32,
        key: GlyphKey,
    ) -> Option<RasterizedGlyph> {
        if let Some(cached) = self.glyphs.get(&key) {
            return cached.clone();
        }
        let g = fonts.rasterize(key, baseline_y);
        self.glyphs.insert(key, g.clone());
        g
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rounded_rect_premul(
    canvas: &mut [u8],
    stride_px: i32,
    win_w: i32,
    win_h: i32,
    rect: Rect,
    radius: i32,
    fill: [u8; 4],
    border: Option<[u8; 4]>,
) {
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = rect.right().min(win_w);
    let y1 = rect.bottom().min(win_h);
    let cx = rect.x as f32 + rect.w as f32 * 0.5;
    let cy = rect.y as f32 + rect.h as f32 * 0.5;
    let hx = rect.w as f32 * 0.5;
    let hy = rect.h as f32 * 0.5;
    let r = (radius as f32).min(hx).min(hy);

    for py in y0..y1 {
        let dy = (py as f32 + 0.5 - cy).abs() - (hy - r);
        for px in x0..x1 {
            let dx = (px as f32 + 0.5 - cx).abs() - (hx - r);
            let dist = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
            let cov = (0.6 - dist).clamp(0.0, 1.0);
            if cov <= 0.001 {
                continue;
            }
            let src = if let Some(b) = border {
                if dist >= -1.0 {
                    [b[0], b[1], b[2], ((f32::from(b[3]) * cov).round() as u8)]
                } else {
                    [
                        fill[0],
                        fill[1],
                        fill[2],
                        ((f32::from(fill[3]) * cov).round() as u8),
                    ]
                }
            } else {
                [
                    fill[0],
                    fill[1],
                    fill[2],
                    ((f32::from(fill[3]) * cov).round() as u8),
                ]
            };
            blend_pixel_premul(canvas, stride_px, px, py, src);
        }
    }
}

fn fill_rounded_header(
    canvas: &mut [u8],
    stride_px: i32,
    win_w: i32,
    win_h: i32,
    rect: Rect,
    radius: i32,
    fill: [u8; 4],
) {
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = rect.right().min(win_w);
    let y1 = rect.bottom().min(win_h);
    let r = radius as f32;
    for py in y0..y1 {
        for px in x0..x1 {
            let mut cov = 1.0_f32;
            if py < rect.y + radius {
                let fy = (rect.y + radius) as f32 - (py as f32 + 0.5);
                if px < rect.x + radius {
                    let fx = (rect.x + radius) as f32 - (px as f32 + 0.5);
                    cov = (r - fx.hypot(fy) + 0.5).clamp(0.0, 1.0);
                } else if px >= rect.right() - radius {
                    let fx = (px as f32 + 0.5) - (rect.right() - radius) as f32;
                    cov = (r - fx.hypot(fy) + 0.5).clamp(0.0, 1.0);
                }
            }
            if cov > 0.001 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let a = (f32::from(fill[3]) * cov).round() as u8;
                blend_pixel_premul(canvas, stride_px, px, py, [fill[0], fill[1], fill[2], a]);
            }
        }
    }
}

fn blend_rect(
    canvas: &mut [u8],
    stride_px: i32,
    win_w: i32,
    win_h: i32,
    rect: Rect,
    color: [u8; 4],
    clip: Option<Rect>,
) {
    let cx0 = clip.map_or(0, |c| c.x);
    let cy0 = clip.map_or(0, |c| c.y);
    let cx1 = clip.map_or(win_w, Rect::right);
    let cy1 = clip.map_or(win_h, Rect::bottom);
    let x0 = rect.x.max(cx0).max(0);
    let y0 = rect.y.max(cy0).max(0);
    let x1 = rect.right().min(cx1).min(win_w);
    let y1 = rect.bottom().min(cy1).min(win_h);
    for py in y0..y1 {
        for px in x0..x1 {
            blend_pixel_premul(canvas, stride_px, px, py, color);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_glyph_premul(
    canvas: &mut [u8],
    stride_px: i32,
    win_w: i32,
    win_h: i32,
    glyph: &RasterizedGlyph,
    gx: i32,
    gy: i32,
    color: [u8; 4],
    clip: Option<Rect>,
) {
    let cx0 = clip.map_or(0, |c| c.x).max(0);
    let cy0 = clip.map_or(0, |c| c.y).max(0);
    let cx1 = clip.map_or(win_w, Rect::right).min(win_w);
    let cy1 = clip.map_or(win_h, Rect::bottom).min(win_h);

    let gw = glyph.width as i32;
    let gh = glyph.height as i32;
    for row in 0..gh {
        let py = gy + row;
        if py < cy0 || py >= cy1 {
            continue;
        }
        for col in 0..gw {
            let px = gx + col;
            if px < cx0 || px >= cx1 {
                continue;
            }
            let mask = glyph.bitmap[(row * gw + col) as usize];
            if mask == 0 {
                continue;
            }
            let alpha = ((u16::from(color[3]) * u16::from(mask)) / 255) as u8;
            blend_pixel_premul(
                canvas,
                stride_px,
                px,
                py,
                [color[0], color[1], color[2], alpha],
            );
        }
    }
}

fn blend_pixel_premul(canvas: &mut [u8], stride_px: i32, x: i32, y: i32, rgba: [u8; 4]) {
    #[allow(clippy::cast_sign_loss)]
    let off = ((y * stride_px + x) * 4) as usize;
    let a = u16::from(rgba[3]);
    if a == 0 || off + 3 >= canvas.len() {
        return;
    }
    let inv = 255 - a;
    let sr = u16::from(rgba[0]) * a / 255;
    let sg = u16::from(rgba[1]) * a / 255;
    let sb = u16::from(rgba[2]) * a / 255;

    let db = u16::from(canvas[off]);
    let dg = u16::from(canvas[off + 1]);
    let dr = u16::from(canvas[off + 2]);
    let da = u16::from(canvas[off + 3]);

    canvas[off] = (sb + db * inv / 255).min(255) as u8;
    canvas[off + 1] = (sg + dg * inv / 255).min(255) as u8;
    canvas[off + 2] = (sr + dr * inv / 255).min(255) as u8;
    canvas[off + 3] = (a + da * inv / 255).min(255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shm_renderer_preserves_rounded_corner_alpha_and_writes_glyph_pixels() {
        let mut app = TerminalAppState::new(640, 400, None, Vec::new(), None, None, None, false);
        let _ = app.tabs[0].engine.process_bytes(b"echo rusty-shm");
        let mut shm = ShmFallbackRenderer::new();
        let mut buf = vec![0_u8; (640 * 400 * 4) as usize];
        shm.render(&app, &mut buf, 640);

        // Pixel (0, 0) outside 14px rounded corner must remain transparent.
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);
        // Center of close traffic light button (21, 20) must be coral-red & opaque.
        let tl_off = ((20 * 640 + 21) * 4) as usize;
        assert_eq!(buf[tl_off + 3], 255);
        assert!(buf[tl_off + 2] > 200);
    }
}
