//! Wayland-EGL + OpenGL ES 2.0 batched GPU renderer with dynamic 1024x1024 Glyph Atlas (`RUSTYOS-113`).
//!
//! Architecture mirrors Alacritty's 2-pass batched GLES2 renderer:
//! - Pass 1 (`RECT_PROG`): SDF rounded rects, borders, circles (traffic lights), cell backgrounds,
//!   selections, search highlights, cursor, and scrollbar thumb in a single batched draw call.
//! - Pass 2 (`TEXT_PROG`): Sampled `GL_ALPHA` glyph atlas quads with `glScissor` clipping for
//!   scrollable terminal rows vs. header/search chrome.

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::ptr;

use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_egl::WlEglSurface;

use crate::font::{FontSet, GlyphKey, GlyphKind};
use crate::layout::{Rect, TerminalLayout, WINDOW_CORNER_RADIUS};
use crate::state::{TerminalAppState, TerminalTab};

const ATLAS_SIZE: u32 = 1024;
const MAX_RECT_VERTS: usize = 32_768;
const MAX_TEXT_VERTS: usize = 65_536;

mod egl_ffi {
    #![allow(
        clippy::all,
        clippy::pedantic,
        clippy::panic,
        unsafe_op_in_unsafe_fn,
        missing_debug_implementations,
        missing_copy_implementations,
        unreachable_pub,
        non_camel_case_types,
        non_snake_case,
        dead_code,
        unused_imports
    )]

    use std::os::raw::{c_int, c_void};

    pub type khronos_utime_nanoseconds_t = u64;
    pub type khronos_uint64_t = u64;
    pub type khronos_ssize_t = isize;
    pub type EGLNativeDisplayType = *const c_void;
    pub type EGLNativePixmapType = *const c_void;
    pub type EGLNativeWindowType = *const c_void;
    pub type EGLint = c_int;
    pub type NativeDisplayType = EGLNativeDisplayType;
    pub type NativePixmapType = EGLNativePixmapType;
    pub type NativeWindowType = EGLNativeWindowType;

    include!(concat!(env!("OUT_DIR"), "/egl_bindings.rs"));
}

mod gl_ffi {
    #![allow(
        clippy::all,
        clippy::pedantic,
        clippy::panic,
        unsafe_op_in_unsafe_fn,
        missing_debug_implementations,
        missing_copy_implementations,
        unreachable_pub,
        non_camel_case_types,
        non_snake_case,
        dead_code,
        unused_imports
    )]

    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

/// Coordinates + UV mapping of one cached glyph inside the `1024x1024` GPU atlas texture.
#[derive(Debug, Clone, Copy)]
struct AtlasEntry {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    width: i32,
    height: i32,
    offset_x: i32,
    offset_y: i32,
    advance_x: f32,
}

/// Row-shelf bin packer over a `1024x1024` single-channel atlas texture (`1px` gutter padding).
#[derive(Debug, Clone)]
pub(crate) struct ShelfPacker {
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_h: u32,
}

impl ShelfPacker {
    #[must_use]
    pub(crate) const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cursor_x: 1,
            cursor_y: 1,
            row_h: 0,
        }
    }

    pub(crate) fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w == 0 || h == 0 || w + 2 > self.width || h + 2 > self.height {
            return None;
        }
        if self.cursor_x + w + 1 > self.width {
            self.cursor_x = 1;
            self.cursor_y += self.row_h + 1;
            self.row_h = 0;
        }
        if self.cursor_y + h + 1 > self.height {
            return None;
        }
        let pos = (self.cursor_x, self.cursor_y);
        self.cursor_x += w + 1;
        self.row_h = self.row_h.max(h);
        Some(pos)
    }

    pub(crate) fn reset(&mut self) {
        self.cursor_x = 1;
        self.cursor_y = 1;
        self.row_h = 0;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RectVertex {
    pos: [f32; 2],
    center: [f32; 2],
    half: [f32; 2],
    radius_border: [f32; 2],
    fill: [f32; 4],
    stroke: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TextVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

/// Owns the Wayland EGL surface, GLES 2.0 context, batched programs, and GPU Glyph Atlas.
pub(crate) struct GpuRenderer {
    dl_egl_handle: *mut c_void,
    egl: egl_ffi::Egl,
    gl: gl_ffi::Gles2,
    egl_display: egl_ffi::types::EGLDisplay,
    egl_context: egl_ffi::types::EGLContext,
    egl_surface: egl_ffi::types::EGLSurface,
    // Must outlive `egl_surface` destruction in Drop, so destroyed AFTER eglDestroySurface.
    wl_egl_surface: WlEglSurface,
    rect_prog: u32,
    rect_u_viewport: i32,
    rect_vbo: u32,
    text_prog: u32,
    text_u_viewport: i32,
    text_u_atlas: i32,
    text_vbo: u32,
    atlas_tex: u32,
    packer: ShelfPacker,
    atlas_entries: HashMap<GlyphKey, Option<AtlasEntry>>,
    rect_verts: Vec<RectVertex>,
    text_verts: Vec<TextVertex>,
    width: i32,
    height: i32,
}

impl std::fmt::Debug for GpuRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuRenderer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("cached_glyphs", &self.atlas_entries.len())
            .finish_non_exhaustive()
    }
}

impl GpuRenderer {
    /// Initializes Wayland-EGL + GLES 2.0 on `wl_surface`, returning `Err` if unavailable
    /// so caller cleanly falls back to `wl_shm`.
    pub(crate) fn new(
        wl_display_ptr: *mut c_void,
        wl_surface: &WlSurface,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let dl_egl = open_libegl()?;
        let egl = load_egl(dl_egl);
        let egl_display = get_wayland_egl_display(&egl, wl_display_ptr)?;

        let mut major = 0;
        let mut minor = 0;
        // SAFETY: `egl_display` was returned cleanly by `eglGetPlatformDisplayEXT`/`eglGetDisplay`.
        if unsafe {
            egl.Initialize(
                egl_display,
                ptr::from_mut(&mut major),
                ptr::from_mut(&mut minor),
            )
        } == egl_ffi::FALSE
        {
            unsafe { libc::dlclose(dl_egl) };
            return Err(String::from("eglInitialize failed"));
        }

        // SAFETY: Binding OpenGL ES client API on valid initialized display.
        if unsafe { egl.BindAPI(egl_ffi::OPENGL_ES_API) } == egl_ffi::FALSE {
            unsafe {
                egl.Terminate(egl_display);
                libc::dlclose(dl_egl);
            }
            return Err(String::from("eglBindAPI(OPENGL_ES_API) failed"));
        }

        let config = choose_egl_config(&egl, egl_display)?;
        let ctx_attribs = [
            egl_ffi::CONTEXT_CLIENT_VERSION as i32,
            2,
            egl_ffi::NONE as i32,
        ];
        // SAFETY: Valid display & chosen ARGB8888 ES2 config.
        let egl_context =
            unsafe { egl.CreateContext(egl_display, config, ptr::null(), ctx_attribs.as_ptr()) };
        if egl_context.is_null() {
            unsafe {
                egl.Terminate(egl_display);
                libc::dlclose(dl_egl);
            }
            return Err(String::from("eglCreateContext(ES2) failed"));
        }

        let w = width.max(1);
        let h = height.max(1);
        let wl_egl_surface = WlEglSurface::new(wl_surface.id(), w, h)
            .map_err(|e| format!("WlEglSurface::new failed: {e}"))?;

        // SAFETY: `wl_egl_surface.ptr()` is valid `wl_egl_window*` matching Wayland EGL display.
        let egl_surface = unsafe {
            egl.CreateWindowSurface(egl_display, config, wl_egl_surface.ptr(), ptr::null())
        };
        if egl_surface.is_null() {
            unsafe {
                egl.DestroyContext(egl_display, egl_context);
                egl.Terminate(egl_display);
                libc::dlclose(dl_egl);
            }
            return Err(String::from("eglCreateWindowSurface failed"));
        }

        // SAFETY: Valid current display, surface, and ES2 context on this UI thread.
        if unsafe { egl.MakeCurrent(egl_display, egl_surface, egl_surface, egl_context) }
            == egl_ffi::FALSE
        {
            unsafe {
                egl.DestroySurface(egl_display, egl_surface);
                egl.DestroyContext(egl_display, egl_context);
                egl.Terminate(egl_display);
                libc::dlclose(dl_egl);
            }
            return Err(String::from("eglMakeCurrent failed"));
        }

        // Avoid blocking in Mesa swap interval; frame pacing uses Wayland `wl_surface.frame`.
        unsafe {
            egl.SwapInterval(egl_display, 0);
        }

        let gl = gl_ffi::Gles2::load_with(|sym| get_gl_proc(&egl, dl_egl, sym));
        Self::init_gl_resources(
            dl_egl,
            egl,
            gl,
            egl_display,
            egl_context,
            egl_surface,
            wl_egl_surface,
            w,
            h,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn init_gl_resources(
        dl_egl: *mut c_void,
        egl: egl_ffi::Egl,
        gl: gl_ffi::Gles2,
        egl_display: egl_ffi::types::EGLDisplay,
        egl_context: egl_ffi::types::EGLContext,
        egl_surface: egl_ffi::types::EGLSurface,
        wl_egl_surface: WlEglSurface,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let rect_prog = compile_program(&gl, RECT_VERT_SRC, RECT_FRAG_SRC)?;
        let text_prog = compile_program(&gl, TEXT_VERT_SRC, TEXT_FRAG_SRC)?;
        let rect_u_viewport = get_uniform_loc(&gl, rect_prog, c"u_viewport");
        let text_u_viewport = get_uniform_loc(&gl, text_prog, c"u_viewport");
        let text_u_atlas = get_uniform_loc(&gl, text_prog, c"u_atlas");

        let mut bufs = [0_u32; 2];
        let mut tex = 0_u32;
        // SAFETY: Valid active GLES2 context.
        unsafe {
            gl.GenBuffers(2, bufs.as_mut_ptr());
            gl.GenTextures(1, ptr::from_mut(&mut tex));
            gl.BindTexture(gl_ffi::TEXTURE_2D, tex);
            gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_MIN_FILTER,
                gl_ffi::LINEAR as i32,
            );
            gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_MAG_FILTER,
                gl_ffi::LINEAR as i32,
            );
            gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_WRAP_S,
                gl_ffi::CLAMP_TO_EDGE as i32,
            );
            gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_WRAP_T,
                gl_ffi::CLAMP_TO_EDGE as i32,
            );
            let zeros = vec![0_u8; (ATLAS_SIZE * ATLAS_SIZE) as usize];
            gl.PixelStorei(gl_ffi::UNPACK_ALIGNMENT, 1);
            gl.TexImage2D(
                gl_ffi::TEXTURE_2D,
                0,
                gl_ffi::ALPHA as i32,
                ATLAS_SIZE as i32,
                ATLAS_SIZE as i32,
                0,
                gl_ffi::ALPHA,
                gl_ffi::UNSIGNED_BYTE,
                zeros.as_ptr().cast(),
            );
        }

        let err = unsafe { gl.GetError() };
        if err != gl_ffi::NO_ERROR {
            return Err(format!("GL init error: 0x{err:x}"));
        }

        Ok(Self {
            dl_egl_handle: dl_egl,
            egl,
            gl,
            egl_display,
            egl_context,
            egl_surface,
            wl_egl_surface,
            rect_prog,
            rect_u_viewport,
            rect_vbo: bufs[0],
            text_prog,
            text_u_viewport,
            text_u_atlas,
            text_vbo: bufs[1],
            atlas_tex: tex,
            packer: ShelfPacker::new(ATLAS_SIZE, ATLAS_SIZE),
            atlas_entries: HashMap::with_capacity(512),
            rect_verts: Vec::with_capacity(2048),
            text_verts: Vec::with_capacity(4096),
            width,
            height,
        })
    }

    pub(crate) fn resize(&mut self, width: i32, height: i32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width != w || self.height != h {
            self.width = w;
            self.height = h;
            self.wl_egl_surface.resize(w, h, 0, 0);
        }
    }

    /// Renders complete RustyOS window + cells via 2 batched passes and presents via `eglSwapBuffers`.
    pub(crate) fn render(&mut self, app: &TerminalAppState) -> Result<(), String> {
        // SAFETY: Valid current EGL context.
        unsafe {
            if self.egl.MakeCurrent(
                self.egl_display,
                self.egl_surface,
                self.egl_surface,
                self.egl_context,
            ) == egl_ffi::FALSE
            {
                return Err(String::from("eglMakeCurrent lost during frame"));
            }
        }

        let layout = app.layout();
        self.resize(layout.win_w, layout.win_h);
        self.rect_verts.clear();
        self.text_verts.clear();

        let (header_verts_end, chrome_text_start) = self.build_batches(app, &layout);

        unsafe {
            self.gl.Viewport(0, 0, layout.win_w, layout.win_h);
            self.gl.Disable(gl_ffi::SCISSOR_TEST);
            self.gl.Enable(gl_ffi::BLEND);
            self.gl.BlendFunc(gl_ffi::ONE, gl_ffi::ONE_MINUS_SRC_ALPHA);
            self.gl.ClearColor(0.0, 0.0, 0.0, 0.0);
            self.gl.Clear(gl_ffi::COLOR_BUFFER_BIT);

            // Pass 1a: rounded translucent window base (`0..6`).
            self.flush_rects(layout.win_w, layout.win_h, 0, 6);

            // Pass 1b: top-rounded header clipped cleanly at `y = HEADER_HEIGHT` (`6..header_verts_end`).
            self.gl.Enable(gl_ffi::SCISSOR_TEST);
            self.gl.Scissor(
                0,
                (layout.win_h - layout.header_rect.h).max(0),
                layout.win_w.max(1),
                layout.header_rect.h.max(1),
            );
            self.flush_rects(layout.win_w, layout.win_h, 6, header_verts_end);
            self.gl.Disable(gl_ffi::SCISSOR_TEST);

            // Pass 1c: all remaining rects (separator, cell bgs, tabs, pills, traffic lights, cursor, border).
            self.flush_rects(
                layout.win_w,
                layout.win_h,
                header_verts_end,
                self.rect_verts.len(),
            );

            // Pass 2a: scrolled terminal grid glyphs clipped cleanly inside `grid_rect`!
            self.gl.Enable(gl_ffi::SCISSOR_TEST);
            let sc_y = layout.win_h - (layout.grid_rect.y + layout.grid_rect.h);
            self.gl.Scissor(
                layout.grid_rect.x.max(0),
                sc_y.max(0),
                layout.grid_rect.w.max(1),
                layout.grid_rect.h.max(1),
            );
            self.flush_text(layout.win_w, layout.win_h, 0, chrome_text_start);
            self.gl.Disable(gl_ffi::SCISSOR_TEST);

            // Pass 2b: header, tab titles, and search bar text (unclipped to grid_rect).
            self.flush_text(
                layout.win_w,
                layout.win_h,
                chrome_text_start,
                self.text_verts.len(),
            );

            if self.egl.SwapBuffers(self.egl_display, self.egl_surface) == egl_ffi::FALSE {
                return Err(String::from("eglSwapBuffers failed"));
            }
        }
        Ok(())
    }

    fn build_batches(&mut self, app: &TerminalAppState, layout: &TerminalLayout) -> (usize, usize) {
        let theme = &app.theme;
        // 0..6: Rounded window glass base (`r = 14`).
        self.push_sdf_rect(
            Rect::new(0, 0, layout.win_w, layout.win_h),
            WINDOW_CORNER_RADIUS as f32,
            0.0,
            theme.window_bg,
            [0, 0, 0, 0],
        );
        // 6..12: Top-rounded header rect extending below header bottom (scissored at HEADER_HEIGHT).
        self.push_sdf_rect(
            Rect::new(
                0,
                0,
                layout.win_w,
                layout.header_rect.h + WINDOW_CORNER_RADIUS,
            ),
            WINDOW_CORNER_RADIUS as f32,
            0.0,
            theme.header_bg,
            [0, 0, 0, 0],
        );
        let header_verts_end = self.rect_verts.len();

        // Header separator line.
        self.push_sdf_rect(
            Rect::new(0, layout.header_rect.h - 1, layout.win_w, 1),
            0.0,
            0.0,
            theme.header_separator,
            [0, 0, 0, 0],
        );

        self.emit_grid_pass(app, layout);
        self.emit_chrome_rects(app, layout);

        // Final crisp 1px outer border ring around full rounded window perimeter.
        self.push_sdf_rect(
            Rect::new(0, 0, layout.win_w, layout.win_h),
            WINDOW_CORNER_RADIUS as f32,
            1.0,
            [0, 0, 0, 0],
            theme.window_border,
        );

        let chrome_text_start = self.text_verts.len();
        self.emit_chrome_text(app, layout);
        (header_verts_end, chrome_text_start)
    }

    fn emit_grid_pass(&mut self, app: &TerminalAppState, layout: &TerminalLayout) {
        let Some(tab) = app.active_tab() else {
            return;
        };
        let cw = app.metrics.cell_w;
        let ch = app.metrics.cell_h;
        let sb_len = tab.engine.scrollback_len();
        let scroll_px = tab.scroll_lines.current() * f64::from(ch);
        let scroll_rows_f = scroll_px / f64::from(ch.max(1));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let whole_back = scroll_rows_f.floor().max(0.0) as usize;
        let frac_px = ((scroll_rows_f - whole_back as f64) * f64::from(ch)) as f32;
        let top_abs = sb_len.saturating_sub(whole_back.min(sb_len));
        let visible_rows = usize::from(layout.rows) + 2;

        let search_highlights = &app.search.matches;
        let active_search = search_highlights.get(app.search.active_idx).copied();
        let (cur_col, cur_row) = tab.engine.cursor_pos();
        let cur_abs = sb_len + cur_row;

        for rel_r in 0..visible_rows {
            let abs_line = top_abs.saturating_sub(usize::from(frac_px > 0.5)) + rel_r;
            let Some(cells) = tab.engine.row_cells(abs_line) else {
                continue;
            };
            let row_y = layout.grid_rect.y as f32
                + (rel_r as f32 - if frac_px > 0.5 { 1.0 } else { 0.0 }) * ch as f32
                + frac_px;

            for (col, cell) in cells.iter().enumerate().take(usize::from(layout.cols)) {
                let x = layout.grid_rect.x as f32 + (col as f32) * (cw as f32);
                self.emit_cell(
                    app,
                    tab,
                    cell,
                    abs_line,
                    col,
                    x,
                    row_y,
                    cw,
                    ch,
                    search_highlights,
                    active_search,
                    cur_abs,
                    cur_col,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_cell(
        &mut self,
        app: &TerminalAppState,
        tab: &TerminalTab,
        cell: &crate::vt::Cell,
        abs_line: usize,
        col: usize,
        x: f32,
        y: f32,
        cw: i32,
        ch: i32,
        search_matches: &[crate::vt::GridPoint],
        active_search: Option<crate::vt::GridPoint>,
        cur_abs: usize,
        cur_col: usize,
    ) {
        let mut fg = app.theme.resolve_fg(cell.fg, cell.attrs.bold);
        let mut bg = app.theme.resolve_bg(cell.bg);

        if cell.attrs.inverse {
            let old_fg = fg;
            fg = bg.unwrap_or([
                app.theme.window_bg[0],
                app.theme.window_bg[1],
                app.theme.window_bg[2],
                255,
            ]);
            bg = Some(old_fg);
        }

        if cell.attrs.dim {
            fg[3] = (u16::from(fg[3]) * 165 / 255) as u8;
        }

        if let Some(bg_col) = bg {
            self.push_rect_f(x, y, cw as f32, ch as f32, 0.0, 0.0, bg_col, [0, 0, 0, 0]);
        }

        if tab.selection.is_some_and(|s| s.contains(abs_line, col)) {
            self.push_rect_f(
                x,
                y,
                cw as f32,
                ch as f32,
                0.0,
                0.0,
                app.theme.selection_bg,
                [0, 0, 0, 0],
            );
        }

        if app.search.open && !app.search.query.is_empty() {
            let qlen = app.search.query.chars().count();
            let hit_bg = active_search
                .filter(|m| m.abs_line == abs_line && col >= m.col && col < m.col + qlen)
                .map(|_| app.theme.search_current_bg)
                .or_else(|| {
                    search_matches
                        .iter()
                        .any(|m| m.abs_line == abs_line && col >= m.col && col < m.col + qlen)
                        .then_some(app.theme.search_match_bg)
                });
            if let Some(hc) = hit_bg {
                self.push_rect_f(x, y, cw as f32, ch as f32, 2.0, 0.0, hc, [0, 0, 0, 0]);
            }
        }

        let is_cursor = abs_line == cur_abs && col == cur_col && tab.engine.modes.cursor_visible;
        if is_cursor {
            self.emit_cursor_rect(app, tab.engine.modes.cursor_style, x, y, cw, ch, &mut fg);
        }

        self.emit_cell_glyph_and_decor(app, cell, x, y, cw, ch, fg);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_cursor_rect(
        &mut self,
        app: &TerminalAppState,
        style: crate::vt::CursorStyle,
        x: f32,
        y: f32,
        cw: i32,
        ch: i32,
        fg: &mut [u8; 4],
    ) {
        let show = !app.focused || app.cursor_blink_on;
        let mut cur_col = app.theme.cursor_bg;
        if !app.focused {
            cur_col[3] = 100;
        }
        match style {
            crate::vt::CursorStyle::Bar => {
                if show {
                    self.push_rect_f(
                        x + 0.5,
                        y + 1.5,
                        2.0,
                        (ch as f32 - 3.0).max(4.0),
                        1.0,
                        0.0,
                        cur_col,
                        [0, 0, 0, 0],
                    );
                }
            }
            crate::vt::CursorStyle::Underline => {
                if show {
                    self.push_rect_f(
                        x,
                        y + (ch as f32 - 2.0).max(1.0),
                        cw as f32,
                        2.0,
                        1.0,
                        0.0,
                        cur_col,
                        [0, 0, 0, 0],
                    );
                }
            }
            crate::vt::CursorStyle::Block => {
                if !app.focused {
                    self.push_rect_f(
                        x,
                        y,
                        cw as f32,
                        ch as f32,
                        2.0,
                        1.0,
                        [0, 0, 0, 0],
                        app.theme.cursor_bg,
                    );
                } else if show {
                    self.push_rect_f(
                        x,
                        y,
                        cw as f32,
                        ch as f32,
                        2.0,
                        0.0,
                        app.theme.cursor_bg,
                        [0, 0, 0, 0],
                    );
                    *fg = app.theme.cursor_fg;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_cell_glyph_and_decor(
        &mut self,
        app: &TerminalAppState,
        cell: &crate::vt::Cell,
        x: f32,
        y: f32,
        cw: i32,
        ch: i32,
        fg: [u8; 4],
    ) {
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
            self.emit_glyph_quad(
                &app.fonts,
                app.metrics.baseline_y,
                key,
                x.round(),
                y.round(),
                fg,
            );
        }

        if cell.attrs.underline {
            self.push_rect_f(
                x,
                y + app.metrics.baseline_y as f32 + 2.0,
                cw as f32,
                1.0,
                0.0,
                0.0,
                fg,
                [0, 0, 0, 0],
            );
        }
        if cell.attrs.strike {
            self.push_rect_f(
                x,
                y + (ch as f32) * 0.48,
                cw as f32,
                1.0,
                0.0,
                0.0,
                fg,
                [0, 0, 0, 0],
            );
        }
    }

    #[allow(clippy::too_many_lines, clippy::similar_names)]
    fn emit_chrome_rects(&mut self, app: &TerminalAppState, layout: &TerminalLayout) {
        let unfocused = !app.focused;
        let close_c = if unfocused {
            [110, 114, 122, 255]
        } else {
            [0xEE, 0x5A, 0x6A, 255]
        };
        let min_c = if unfocused {
            [110, 114, 122, 255]
        } else {
            [0xF4, 0xB6, 0x3F, 255]
        };
        let max_c = if unfocused {
            [110, 114, 122, 255]
        } else {
            [0x3C, 0xC6, 0x8A, 255]
        };
        self.push_sdf_rect(layout.close_rect, 7.0, 0.0, close_c, [0, 0, 0, 0]);
        self.push_sdf_rect(layout.minimize_rect, 7.0, 0.0, min_c, [0, 0, 0, 0]);
        self.push_sdf_rect(layout.maximize_rect, 7.0, 0.0, max_c, [0, 0, 0, 0]);

        let tl_hovered = matches!(
            app.hover,
            Some(
                crate::layout::HitTarget::CloseButton
                    | crate::layout::HitTarget::MinimizeButton
                    | crate::layout::HitTarget::MaximizeButton
            )
        );
        if tl_hovered {
            let ink = rusty_ui::TrafficLightKind::glyph_ink();
            for (cx, cy, kind) in [
                (
                    layout.close_rect.x as f32 + 7.0,
                    layout.close_rect.y as f32 + 7.0,
                    rusty_ui::TrafficLightKind::Close,
                ),
                (
                    layout.minimize_rect.x as f32 + 7.0,
                    layout.minimize_rect.y as f32 + 7.0,
                    rusty_ui::TrafficLightKind::Minimize,
                ),
                (
                    layout.maximize_rect.x as f32 + 7.0,
                    layout.maximize_rect.y as f32 + 7.0,
                    rusty_ui::TrafficLightKind::Maximize,
                ),
            ] {
                for seg in rusty_ui::glyph_segments(kind) {
                    let steps = 6;
                    for i in 0..=steps {
                        #[allow(clippy::cast_precision_loss)]
                        let t = i as f32 / steps as f32;
                        let x = cx + seg.dx0 + (seg.dx1 - seg.dx0) * t;
                        let y = cy + seg.dy0 + (seg.dy1 - seg.dy0) * t;
                        self.push_rect_f(x - 0.5, y - 0.5, 1.1, 1.1, 0.2, 0.0, ink, [0, 0, 0, 0]);
                    }
                }
            }
        }

        for (idx, tab_l) in layout.tab_rects.iter().enumerate() {
            let active = idx == app.active_tab;
            let hovered = app.hover == Some(crate::layout::HitTarget::Tab(idx));
            let fill = if active {
                app.theme.tab_active_bg
            } else if hovered {
                app.theme.tab_hover_bg
            } else {
                app.theme.badge_bg
            };
            let border = if active {
                app.theme.tab_border
            } else {
                [0, 0, 0, 0]
            };
            self.push_sdf_rect(
                tab_l.pill,
                8.0,
                if active { 1.0 } else { 0.0 },
                fill,
                border,
            );
        }

        // Right-side header pills (Glass toggle, Search, New Tab +)
        for (target, rect, is_on) in [
            (
                crate::layout::HitTarget::GlassToggle,
                layout.glass_btn_rect,
                !app.theme.reduce_transparency,
            ),
            (
                crate::layout::HitTarget::SearchToggle,
                layout.search_btn_rect,
                app.search.open,
            ),
            (
                crate::layout::HitTarget::NewTabButton,
                layout.new_tab_btn_rect,
                false,
            ),
        ] {
            let hovered = app.hover == Some(target);
            let fill = if is_on {
                [
                    app.theme.accent[0],
                    app.theme.accent[1],
                    app.theme.accent[2],
                    64,
                ]
            } else if hovered {
                app.theme.tab_hover_bg
            } else {
                app.theme.badge_bg
            };
            self.push_sdf_rect(rect, 7.0, 1.0, fill, app.theme.tab_border);
        }

        if let (Some(bar), Some(input)) = (layout.search_bar_rect, layout.search_input_rect) {
            self.push_sdf_rect(bar, 10.0, 1.0, app.theme.search_bg, app.theme.search_border);
            self.push_sdf_rect(input, 7.0, 1.0, app.theme.badge_bg, app.theme.tab_border);
            for btn in [
                layout.search_prev_rect,
                layout.search_next_rect,
                layout.search_close_rect,
            ]
            .into_iter()
            .flatten()
            {
                self.push_sdf_rect(btn, 6.0, 0.0, app.theme.badge_bg, [0, 0, 0, 0]);
            }
        }

        if let Some(thumb) = layout.scrollbar_thumb {
            self.push_sdf_rect(thumb, 3.0, 0.0, app.theme.scrollbar_thumb, [0, 0, 0, 0]);
        }
    }

    fn emit_chrome_text(&mut self, app: &TerminalAppState, layout: &TerminalLayout) {
        let ui_size = app.metrics.ui_size;
        if layout.tab_rects.is_empty() {
            let base = app
                .active_tab()
                .map_or_else(|| String::from("Terminal"), TerminalTab::display_title);
            let label = format!("{base} — {}×{}", layout.cols, layout.rows);
            let tw = app.fonts.measure_ui_text(&label, ui_size, true);
            let tx =
                layout.title_rect.x as f32 + ((layout.title_rect.w as f32 - tw) * 0.5).max(0.0);
            let ty = layout.header_rect.y as f32
                + app.fonts.ui_baseline_in_box(layout.header_rect.h, ui_size) as f32;
            self.emit_ui_text(
                &app.fonts,
                &label,
                ui_size,
                true,
                tx.round(),
                ty,
                app.theme.text_primary,
            );
        } else {
            for (idx, tab_l) in layout.tab_rects.iter().enumerate() {
                let Some(tab) = app.tabs.get(idx) else {
                    continue;
                };
                let active = idx == app.active_tab;
                let color = if active {
                    app.theme.text_primary
                } else {
                    app.theme.text_secondary
                };
                let label = truncate_to_fit(
                    &app.fonts,
                    &tab.display_title(),
                    12.0,
                    active,
                    tab_l.pill.w - 30,
                );
                let ty =
                    tab_l.pill.y as f32 + app.fonts.ui_baseline_in_box(tab_l.pill.h, 12.0) as f32;
                self.emit_ui_text(
                    &app.fonts,
                    &label,
                    12.0,
                    active,
                    (tab_l.pill.x + 10) as f32,
                    ty,
                    color,
                );
                self.emit_ui_text(
                    &app.fonts,
                    "×",
                    12.5,
                    false,
                    (tab_l.close_btn.x + 4) as f32,
                    ty,
                    app.theme.text_secondary,
                );
            }
        }

        // Crisp procedural SF-Symbol-like icons: Glass (`\u{E001}`), Search (`\u{E002}`), New Tab (`\u{E003}`).
        for (rect, label, active) in [
            (
                layout.glass_btn_rect,
                "\u{E001}",
                !app.theme.reduce_transparency,
            ),
            (layout.search_btn_rect, "\u{E002}", app.search.open),
            (layout.new_tab_btn_rect, "\u{E003}", false),
        ] {
            let tw = app.fonts.measure_ui_text(label, 14.0, true);
            let tx = rect.x as f32 + (rect.w as f32 - tw) * 0.5;
            let ty = rect.y as f32 + app.fonts.ui_baseline_in_box(rect.h, 14.0) as f32;
            let c = if active {
                app.theme.accent
            } else {
                app.theme.text_primary
            };
            self.emit_ui_text(&app.fonts, label, 14.0, true, tx.round(), ty, c);
        }

        if let (Some(input), Some(prev), Some(next), Some(close)) = (
            layout.search_input_rect,
            layout.search_prev_rect,
            layout.search_next_rect,
            layout.search_close_rect,
        ) {
            self.emit_search_bar_text(app, input, prev, next, close);
        }
    }

    fn emit_search_bar_text(
        &mut self,
        app: &TerminalAppState,
        input: Rect,
        prev: Rect,
        next: Rect,
        close: Rect,
    ) {
        let q = if app.search.query.is_empty() {
            String::from("Find in terminal...")
        } else {
            format!(
                "{}  ({}/{})",
                app.search.query,
                if app.search.matches.is_empty() {
                    0
                } else {
                    app.search.active_idx + 1
                },
                app.search.matches.len()
            )
        };
        let qc = if app.search.query.is_empty() {
            app.theme.text_secondary
        } else {
            app.theme.text_primary
        };
        let ty = input.y as f32 + app.fonts.ui_baseline_in_box(input.h, 12.5) as f32;
        self.emit_ui_text(&app.fonts, &q, 12.5, false, (input.x + 8) as f32, ty, qc);
        for (r, s) in [(prev, "↑"), (next, "↓"), (close, "×")] {
            let tw = app.fonts.measure_ui_text(s, 13.0, true);
            let bx = r.x as f32 + (r.w as f32 - tw) * 0.5;
            let by = r.y as f32 + app.fonts.ui_baseline_in_box(r.h, 13.0) as f32;
            self.emit_ui_text(
                &app.fonts,
                s,
                13.0,
                true,
                bx.round(),
                by,
                app.theme.text_primary,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_ui_text(
        &mut self,
        fonts: &FontSet,
        text: &str,
        size_px: f32,
        bold: bool,
        start_x: f32,
        baseline_y: f32,
        color: [u8; 4],
    ) {
        let kind = if bold {
            GlyphKind::UiBold
        } else {
            GlyphKind::UiRegular
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size_tenths = (size_px * 10.0).round() as u16;
        let mut cursor_x = start_x;
        for ch in text.chars() {
            if ch.is_whitespace() {
                cursor_x += fonts.ui_char_advance(ch, size_px, bold);
                continue;
            }
            let key = GlyphKey {
                ch,
                kind,
                size_tenths,
                cell_w: 0,
                cell_h: 0,
            };
            if let Some(entry) = self.ensure_glyph(fonts, 0, key) {
                let gx = cursor_x.round() + entry.offset_x as f32;
                let gy = baseline_y.round() + entry.offset_y as f32;
                self.push_text_quad(
                    gx,
                    gy,
                    entry.width as f32,
                    entry.height as f32,
                    entry.u0,
                    entry.v0,
                    entry.u1,
                    entry.v1,
                    color,
                );
                cursor_x += entry.advance_x;
            }
        }
    }

    fn emit_glyph_quad(
        &mut self,
        fonts: &FontSet,
        baseline_y: i32,
        key: GlyphKey,
        cell_x: f32,
        cell_y: f32,
        color: [u8; 4],
    ) {
        if let Some(entry) = self.ensure_glyph(fonts, baseline_y, key) {
            self.push_text_quad(
                cell_x + entry.offset_x as f32,
                cell_y + entry.offset_y as f32,
                entry.width as f32,
                entry.height as f32,
                entry.u0,
                entry.v0,
                entry.u1,
                entry.v1,
                color,
            );
        }
    }

    fn ensure_glyph(
        &mut self,
        fonts: &FontSet,
        baseline_y: i32,
        key: GlyphKey,
    ) -> Option<AtlasEntry> {
        if let Some(cached) = self.atlas_entries.get(&key).copied() {
            return cached;
        }
        let Some(glyph) = fonts.rasterize(key, baseline_y) else {
            self.atlas_entries.insert(key, None);
            return None;
        };
        let slot = if let Some(s) = self.packer.allocate(glyph.width, glyph.height) {
            s
        } else {
            self.packer.reset();
            self.atlas_entries.clear();
            self.packer.allocate(glyph.width, glyph.height)?
        };

        // SAFETY: Valid `atlas_tex` bound on current GLES2 context; `glyph.bitmap` has `width*height` bytes.
        unsafe {
            self.gl.BindTexture(gl_ffi::TEXTURE_2D, self.atlas_tex);
            self.gl.PixelStorei(gl_ffi::UNPACK_ALIGNMENT, 1);
            self.gl.TexSubImage2D(
                gl_ffi::TEXTURE_2D,
                0,
                slot.0 as i32,
                slot.1 as i32,
                glyph.width as i32,
                glyph.height as i32,
                gl_ffi::ALPHA,
                gl_ffi::UNSIGNED_BYTE,
                glyph.bitmap.as_ptr().cast(),
            );
        }

        let inv = 1.0 / ATLAS_SIZE as f32;
        let entry = AtlasEntry {
            u0: (slot.0 as f32) * inv,
            v0: (slot.1 as f32) * inv,
            u1: ((slot.0 + glyph.width) as f32) * inv,
            v1: ((slot.1 + glyph.height) as f32) * inv,
            width: glyph.width as i32,
            height: glyph.height as i32,
            offset_x: glyph.offset_x,
            offset_y: glyph.offset_y,
            advance_x: glyph.advance_x,
        };
        self.atlas_entries.insert(key, Some(entry));
        Some(entry)
    }

    fn push_sdf_rect(
        &mut self,
        r: Rect,
        radius: f32,
        border_w: f32,
        fill: [u8; 4],
        stroke: [u8; 4],
    ) {
        self.push_rect_f(
            r.x as f32, r.y as f32, r.w as f32, r.h as f32, radius, border_w, fill, stroke,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn push_rect_f(
        &mut self,
        x0: f32,
        y0: f32,
        rect_w: f32,
        rect_h: f32,
        radius: f32,
        border_w: f32,
        fill: [u8; 4],
        stroke: [u8; 4],
    ) {
        if rect_w <= 0.0 || rect_h <= 0.0 || self.rect_verts.len() + 6 > MAX_RECT_VERTS {
            return;
        }
        let fill_col = rgba_u8_to_f32(fill);
        let stroke_col = rgba_u8_to_f32(stroke);
        let cx = x0 + rect_w * 0.5;
        let cy = y0 + rect_h * 0.5;
        let hx = rect_w * 0.5;
        let hy = rect_h * 0.5;
        let rb = [radius.min(hx).min(hy), border_w];
        let mk_vert = |px: f32, py: f32| RectVertex {
            pos: [px, py],
            center: [cx, cy],
            half: [hx, hy],
            radius_border: rb,
            fill: fill_col,
            stroke: stroke_col,
        };
        let (x1, y1) = (x0 + rect_w, y0 + rect_h);
        self.rect_verts.extend_from_slice(&[
            mk_vert(x0, y0),
            mk_vert(x1, y0),
            mk_vert(x1, y1),
            mk_vert(x0, y0),
            mk_vert(x1, y1),
            mk_vert(x0, y1),
        ]);
    }

    #[allow(clippy::too_many_arguments)]
    fn push_text_quad(
        &mut self,
        x0: f32,
        y0: f32,
        quad_w: f32,
        quad_h: f32,
        u0: f32,
        v0: f32,
        u1: f32,
        v1: f32,
        color: [u8; 4],
    ) {
        if quad_w <= 0.0 || quad_h <= 0.0 || self.text_verts.len() + 6 > MAX_TEXT_VERTS {
            return;
        }
        let col = rgba_u8_to_f32(color);
        let (x1, y1) = (x0 + quad_w, y0 + quad_h);
        self.text_verts.extend_from_slice(&[
            TextVertex {
                pos: [x0, y0],
                uv: [u0, v0],
                color: col,
            },
            TextVertex {
                pos: [x1, y0],
                uv: [u1, v0],
                color: col,
            },
            TextVertex {
                pos: [x1, y1],
                uv: [u1, v1],
                color: col,
            },
            TextVertex {
                pos: [x0, y0],
                uv: [u0, v0],
                color: col,
            },
            TextVertex {
                pos: [x1, y1],
                uv: [u1, v1],
                color: col,
            },
            TextVertex {
                pos: [x0, y1],
                uv: [u0, v1],
                color: col,
            },
        ]);
    }

    unsafe fn flush_rects(&self, win_w: i32, win_h: i32, start: usize, end: usize) {
        if end <= start {
            return;
        }
        let slice = &self.rect_verts[start..end];
        let stride = std::mem::size_of::<RectVertex>() as i32;
        unsafe {
            self.gl.UseProgram(self.rect_prog);
            self.gl
                .Uniform2f(self.rect_u_viewport, win_w as f32, win_h as f32);
            self.gl.BindBuffer(gl_ffi::ARRAY_BUFFER, self.rect_vbo);
            self.gl.BufferData(
                gl_ffi::ARRAY_BUFFER,
                std::mem::size_of_val(slice) as isize,
                slice.as_ptr().cast(),
                gl_ffi::STREAM_DRAW,
            );
            for idx in 0..=5 {
                self.gl.EnableVertexAttribArray(idx);
            }
            self.gl
                .VertexAttribPointer(0, 2, gl_ffi::FLOAT, gl_ffi::FALSE, stride, ptr::null());
            self.gl.VertexAttribPointer(
                1,
                2,
                gl_ffi::FLOAT,
                gl_ffi::FALSE,
                stride,
                8 as *const c_void,
            );
            self.gl.VertexAttribPointer(
                2,
                2,
                gl_ffi::FLOAT,
                gl_ffi::FALSE,
                stride,
                16 as *const c_void,
            );
            self.gl.VertexAttribPointer(
                3,
                2,
                gl_ffi::FLOAT,
                gl_ffi::FALSE,
                stride,
                24 as *const c_void,
            );
            self.gl.VertexAttribPointer(
                4,
                4,
                gl_ffi::FLOAT,
                gl_ffi::FALSE,
                stride,
                32 as *const c_void,
            );
            self.gl.VertexAttribPointer(
                5,
                4,
                gl_ffi::FLOAT,
                gl_ffi::FALSE,
                stride,
                48 as *const c_void,
            );
            self.gl.DrawArrays(gl_ffi::TRIANGLES, 0, slice.len() as i32);
            for idx in 0..=5 {
                self.gl.DisableVertexAttribArray(idx);
            }
        }
    }

    unsafe fn flush_text(&self, win_w: i32, win_h: i32, start: usize, end: usize) {
        if end <= start {
            return;
        }
        let slice = &self.text_verts[start..end];
        let stride = std::mem::size_of::<TextVertex>() as i32;
        unsafe {
            self.gl.UseProgram(self.text_prog);
            self.gl
                .Uniform2f(self.text_u_viewport, win_w as f32, win_h as f32);
            self.gl.ActiveTexture(gl_ffi::TEXTURE0);
            self.gl.BindTexture(gl_ffi::TEXTURE_2D, self.atlas_tex);
            self.gl.Uniform1i(self.text_u_atlas, 0);
            self.gl.BindBuffer(gl_ffi::ARRAY_BUFFER, self.text_vbo);
            self.gl.BufferData(
                gl_ffi::ARRAY_BUFFER,
                std::mem::size_of_val(slice) as isize,
                slice.as_ptr().cast(),
                gl_ffi::STREAM_DRAW,
            );
            for idx in 0..=2 {
                self.gl.EnableVertexAttribArray(idx);
            }
            self.gl
                .VertexAttribPointer(0, 2, gl_ffi::FLOAT, gl_ffi::FALSE, stride, ptr::null());
            self.gl.VertexAttribPointer(
                1,
                2,
                gl_ffi::FLOAT,
                gl_ffi::FALSE,
                stride,
                8 as *const c_void,
            );
            self.gl.VertexAttribPointer(
                2,
                4,
                gl_ffi::FLOAT,
                gl_ffi::FALSE,
                stride,
                16 as *const c_void,
            );
            self.gl.DrawArrays(gl_ffi::TRIANGLES, 0, slice.len() as i32);
            for idx in 0..=2 {
                self.gl.DisableVertexAttribArray(idx);
            }
        }
    }
}

impl Drop for GpuRenderer {
    fn drop(&mut self) {
        // SAFETY: Releases GL objects while context current, then detaches EGL surface/context
        // strictly before `wl_egl_surface` drops (`wl_egl_window_destroy`).
        unsafe {
            if self.egl.MakeCurrent(
                self.egl_display,
                self.egl_surface,
                self.egl_surface,
                self.egl_context,
            ) == egl_ffi::TRUE
            {
                let bufs = [self.rect_vbo, self.text_vbo];
                self.gl.DeleteBuffers(2, bufs.as_ptr());
                self.gl.DeleteTextures(1, ptr::from_ref(&self.atlas_tex));
                self.gl.DeleteProgram(self.rect_prog);
                self.gl.DeleteProgram(self.text_prog);
            }
            self.egl
                .MakeCurrent(self.egl_display, ptr::null(), ptr::null(), ptr::null());
            self.egl.DestroySurface(self.egl_display, self.egl_surface);
            self.egl.DestroyContext(self.egl_display, self.egl_context);
            self.egl.Terminate(self.egl_display);
            if !self.dl_egl_handle.is_null() {
                libc::dlclose(self.dl_egl_handle);
            }
        }
    }
}

pub(crate) fn truncate_to_fit(
    fonts: &FontSet,
    text: &str,
    size_px: f32,
    bold: bool,
    max_w: i32,
) -> String {
    if fonts.measure_ui_text(text, size_px, bold) <= max_w as f32 {
        return text.to_owned();
    }
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        let probe = format!("{out}…");
        if fonts.measure_ui_text(&probe, size_px, bold) > max_w as f32 {
            out.pop();
            break;
        }
    }
    out.push('…');
    out
}

fn rgba_u8_to_f32(c: [u8; 4]) -> [f32; 4] {
    [
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
        f32::from(c[3]) / 255.0,
    ]
}

fn open_libegl() -> Result<*mut c_void, String> {
    for name in [c"libEGL.so.1", c"libEGL.so"] {
        // SAFETY: Static NUL-terminated library name passed to POSIX `dlopen`.
        let h = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if !h.is_null() {
            return Ok(h);
        }
    }
    Err(String::from("libEGL.so.1 not available on host"))
}

fn load_egl(dl_egl: *mut c_void) -> egl_ffi::Egl {
    let egl = egl_ffi::Egl::load_with(|sym| dlsym_ptr(dl_egl, sym));
    egl_ffi::Egl::load_with(|sym| {
        let p = dlsym_ptr(dl_egl, sym);
        if !p.is_null() {
            return p;
        }
        if let Ok(c) = CString::new(sym) {
            // SAFETY: Valid `eglGetProcAddress` function pointer if loaded above.
            let gp = unsafe { egl.GetProcAddress(c.as_ptr()) };
            return gp as *const c_void;
        }
        ptr::null()
    })
}

fn get_gl_proc(egl: &egl_ffi::Egl, dl_egl: *mut c_void, sym: &str) -> *const c_void {
    if let Ok(c) = CString::new(sym) {
        // SAFETY: Valid active EGL library & ASCII symbol name.
        let p = unsafe { egl.GetProcAddress(c.as_ptr()) } as *const c_void;
        if !p.is_null() {
            return p;
        }
    }
    dlsym_ptr(dl_egl, sym)
}

fn dlsym_ptr(handle: *mut c_void, sym: &str) -> *const c_void {
    let Ok(c) = CString::new(sym) else {
        return ptr::null();
    };
    // SAFETY: `handle` came from successful `dlopen` and `c` is NUL-terminated.
    unsafe { libc::dlsym(handle, c.as_ptr()) }
}

fn get_wayland_egl_display(
    egl: &egl_ffi::Egl,
    wl_display_ptr: *mut c_void,
) -> Result<egl_ffi::types::EGLDisplay, String> {
    if egl.GetPlatformDisplayEXT.is_loaded() {
        // SAFETY: `wl_display_ptr` is active `wl_display*` from `wayland_client::Backend::display_ptr()`.
        let dpy = unsafe {
            egl.GetPlatformDisplayEXT(egl_ffi::PLATFORM_WAYLAND_KHR, wl_display_ptr, ptr::null())
        };
        if !dpy.is_null() {
            return Ok(dpy);
        }
    }
    // SAFETY: Legacy fallback for older Mesa builds.
    let dpy = unsafe { egl.GetDisplay(wl_display_ptr) };
    if dpy.is_null() {
        Err(String::from(
            "eglGetDisplay(Wayland) returned EGL_NO_DISPLAY",
        ))
    } else {
        Ok(dpy)
    }
}

fn choose_egl_config(
    egl: &egl_ffi::Egl,
    dpy: egl_ffi::types::EGLDisplay,
) -> Result<egl_ffi::types::EGLConfig, String> {
    let attribs = [
        egl_ffi::SURFACE_TYPE as i32,
        egl_ffi::WINDOW_BIT as i32,
        egl_ffi::RENDERABLE_TYPE as i32,
        egl_ffi::OPENGL_ES2_BIT as i32,
        egl_ffi::RED_SIZE as i32,
        8,
        egl_ffi::GREEN_SIZE as i32,
        8,
        egl_ffi::BLUE_SIZE as i32,
        8,
        egl_ffi::ALPHA_SIZE as i32,
        8,
        egl_ffi::NONE as i32,
    ];
    let mut cfg: egl_ffi::types::EGLConfig = ptr::null();
    let mut count = 0;
    // SAFETY: Valid initialized `dpy`, NUL-terminated attribute array `EGL_NONE`.
    let ok = unsafe {
        egl.ChooseConfig(
            dpy,
            attribs.as_ptr(),
            ptr::from_mut(&mut cfg),
            1,
            ptr::from_mut(&mut count),
        )
    };
    if ok == egl_ffi::TRUE && count > 0 && !cfg.is_null() {
        Ok(cfg)
    } else {
        Err(String::from(
            "eglChooseConfig found no ARGB8888 GLES2 config",
        ))
    }
}

fn compile_program(gl: &gl_ffi::Gles2, vert_src: &str, frag_src: &str) -> Result<u32, String> {
    let vs = compile_shader(gl, gl_ffi::VERTEX_SHADER, vert_src)?;
    let fs = compile_shader(gl, gl_ffi::FRAGMENT_SHADER, frag_src)?;
    // SAFETY: Valid active ES2 context and compiled shader objects.
    unsafe {
        let prog = gl.CreateProgram();
        gl.AttachShader(prog, vs);
        gl.AttachShader(prog, fs);
        if vert_src == RECT_VERT_SRC {
            for (idx, name) in [
                c"a_pos",
                c"a_rect_center",
                c"a_rect_half",
                c"a_radius_border",
                c"a_fill",
                c"a_stroke",
            ]
            .into_iter()
            .enumerate()
            {
                gl.BindAttribLocation(prog, idx as u32, name.as_ptr());
            }
        } else {
            for (idx, name) in [c"a_pos", c"a_uv", c"a_color"].into_iter().enumerate() {
                gl.BindAttribLocation(prog, idx as u32, name.as_ptr());
            }
        }
        gl.LinkProgram(prog);
        let mut status = 0;
        gl.GetProgramiv(prog, gl_ffi::LINK_STATUS, ptr::from_mut(&mut status));
        gl.DeleteShader(vs);
        gl.DeleteShader(fs);
        if status == 0 {
            let mut log = vec![0_u8; 512];
            let mut len = 0;
            gl.GetProgramInfoLog(prog, 512, ptr::from_mut(&mut len), log.as_mut_ptr().cast());
            gl.DeleteProgram(prog);
            let msg = String::from_utf8_lossy(&log[..len.max(0) as usize]).into_owned();
            return Err(format!("GL program link failed: {msg}"));
        }
        Ok(prog)
    }
}

fn compile_shader(gl: &gl_ffi::Gles2, kind: u32, src: &str) -> Result<u32, String> {
    // SAFETY: `src` valid UTF-8 passed with explicit length to `glShaderSource`.
    unsafe {
        let shader = gl.CreateShader(kind);
        let src_ptr = src.as_ptr().cast::<gl_ffi::types::GLchar>();
        let len = src.len() as i32;
        gl.ShaderSource(shader, 1, ptr::from_ref(&src_ptr), ptr::from_ref(&len));
        gl.CompileShader(shader);
        let mut status = 0;
        gl.GetShaderiv(shader, gl_ffi::COMPILE_STATUS, ptr::from_mut(&mut status));
        if status == 0 {
            let mut log = vec![0_u8; 512];
            let mut log_len = 0;
            gl.GetShaderInfoLog(
                shader,
                512,
                ptr::from_mut(&mut log_len),
                log.as_mut_ptr().cast(),
            );
            gl.DeleteShader(shader);
            let msg = String::from_utf8_lossy(&log[..log_len.max(0) as usize]).into_owned();
            return Err(format!("GL shader compile failed: {msg}"));
        }
        Ok(shader)
    }
}

fn get_uniform_loc(gl: &gl_ffi::Gles2, prog: u32, name: &CStr) -> i32 {
    // SAFETY: Valid linked GL program and NUL-terminated ASCII uniform name.
    unsafe { gl.GetUniformLocation(prog, name.as_ptr()) }
}

const RECT_VERT_SRC: &str = "#version 100
attribute vec2 a_pos;
attribute vec2 a_rect_center;
attribute vec2 a_rect_half;
attribute vec2 a_radius_border;
attribute vec4 a_fill;
attribute vec4 a_stroke;
uniform vec2 u_viewport;
varying vec2 v_pos;
varying vec2 v_center;
varying vec2 v_half;
varying vec2 v_rb;
varying vec4 v_fill;
varying vec4 v_stroke;
void main() {
    vec2 ndc = (a_pos / u_viewport) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    v_pos = a_pos;
    v_center = a_rect_center;
    v_half = a_rect_half;
    v_rb = a_radius_border;
    v_fill = a_fill;
    v_stroke = a_stroke;
}
";

const RECT_FRAG_SRC: &str = "#version 100
precision mediump float;
varying vec2 v_pos;
varying vec2 v_center;
varying vec2 v_half;
varying vec2 v_rb;
varying vec4 v_fill;
varying vec4 v_stroke;
void main() {
    float r = v_rb.x;
    float bw = v_rb.y;
    vec2 p = abs(v_pos - v_center) - (v_half - vec2(r));
    float dist = length(max(p, 0.0)) + min(max(p.x, p.y), 0.0) - r;
    float outer_cov = smoothstep(0.6, -0.6, dist);
    if (outer_cov <= 0.001) discard;
    vec4 col = v_fill;
    if (bw > 0.0 && v_stroke.a > 0.0) {
        float border_mix = smoothstep(-bw - 0.5, -bw + 0.5, dist);
        float a_out = mix(v_fill.a, v_stroke.a, border_mix);
        vec3 rgb_out = mix(v_fill.rgb * v_fill.a, v_stroke.rgb * v_stroke.a, border_mix);
        gl_FragColor = vec4(rgb_out * outer_cov, a_out * outer_cov);
        return;
    }
    gl_FragColor = vec4(col.rgb * col.a * outer_cov, col.a * outer_cov);
}
";

const TEXT_VERT_SRC: &str = "#version 100
attribute vec2 a_pos;
attribute vec2 a_uv;
attribute vec4 a_color;
uniform vec2 u_viewport;
varying vec2 v_uv;
varying vec4 v_color;
void main() {
    vec2 ndc = (a_pos / u_viewport) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    v_uv = a_uv;
    v_color = a_color;
}
";

const TEXT_FRAG_SRC: &str = "#version 100
precision mediump float;
uniform sampler2D u_atlas;
varying vec2 v_uv;
varying vec4 v_color;
void main() {
    float cov = texture2D(u_atlas, v_uv).a * v_color.a;
    if (cov <= 0.002) discard;
    gl_FragColor = vec4(v_color.rgb * cov, cov);
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_packer_allocates_without_overlap_and_resets_cleanly() {
        let mut packer = ShelfPacker::new(64, 64);
        let a = packer.allocate(16, 18);
        let b = packer.allocate(20, 14);
        let c = packer.allocate(30, 16);
        assert_eq!(a, Some((1, 1)));
        assert_eq!(b, Some((18, 1)));
        assert_eq!(c, Some((1, 20)));
        packer.reset();
        assert_eq!(packer.allocate(16, 18), Some((1, 1)));
    }
}
