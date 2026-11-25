//! Safe Wayland-EGL + OpenGL ES 2.0 batched UI renderer with CPU `wl_shm` fallback (`RUSTYOS-130`).
//!
//! Architecture extends `apps/rusty-terminal`'s 2-pass batched GLES2 renderer with layer-preserving
//! clip-scoped batching across three minimal GLSL ES 1.00 programs:
//! - `RECT_PROG`: Unified rounded/cubic-squircle SDF shapes, borders, circles, lines, and Gaussian shadows.
//! - `TEXT_PROG`: Lazy `1024x1024` `GL_ALPHA` glyph atlas packed via row-shelf allocator.
//! - `SPRITE_PROG`: Keyed RGBA8888/BGRA8888 texture quad blits for hybrid icon/pixmap layers.
//!
//! # Safety Justification
//!
//! Raw `unsafe` FFI calls are strictly confined inside this module for `libEGL`/`libGLESv2` symbol
//! resolution (`dlopen`/`eglGetProcAddress`), EGL surface/context lifecycle, and GLES2 buffer/texture
//! uploads. Callers interact exclusively through the safe [`GlesSurfaceRenderer`] wrapper, which
//! returns `Err(String)` on initialization or swap failure so callers cleanly fall back to `UiCanvas`.

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::ptr;

use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_egl::WlEglSurface;

use crate::canvas::PixelFormat;
use crate::draw::{UiDrawCmd, UiDrawList, intersect_rect};
use crate::geometry::Rect;
use crate::shadow::{ShadowElevation, ShadowStyle};
use crate::text::{FontWeight, GlyphRasterKey, UiFontEngine};

const ATLAS_SIZE: u32 = 1024;
const MAX_SPRITE_CACHE: usize = 256;

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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RectVertex {
    pos: [f32; 2],
    center: [f32; 2],
    half: [f32; 2],
    radius_border: [f32; 2],
    kind_sigma: [f32; 2],
    fill: [f32; 4],
    stroke: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct TextVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SpriteVertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

#[derive(Debug, Clone, Copy)]
struct AtlasEntry {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    width: i32,
    height: i32,
    xmin: i32,
    ymin: i32,
    advance_x: f32,
}

#[derive(Debug, Clone)]
struct ShelfPacker {
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_h: u32,
}

impl ShelfPacker {
    const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cursor_x: 1,
            cursor_y: 1,
            row_h: 0,
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
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
        let x = self.cursor_x;
        let y = self.cursor_y;
        self.cursor_x += w + 1;
        self.row_h = self.row_h.max(h);
        Some((x, y))
    }

    fn reset(&mut self) {
        self.cursor_x = 1;
        self.cursor_y = 1;
        self.row_h = 0;
    }
}

#[derive(Debug, Clone, Copy)]
struct SpriteTexEntry {
    tex: u32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawBatchKind {
    Rect,
    Text,
    Sprite(u32),
}

#[derive(Debug, Clone, Copy)]
struct BatchSpan {
    kind: DrawBatchKind,
    clip: Option<Rect>,
    start: usize,
    end: usize,
}

/// Safe wrapper over a Wayland-EGL window surface and GLES 2.0 batched UI programs.
pub struct GlesSurfaceRenderer {
    dl_egl_handle: *mut c_void,
    egl: egl_ffi::Egl,
    gl: gl_ffi::Gles2,
    egl_display: egl_ffi::types::EGLDisplay,
    egl_context: egl_ffi::types::EGLContext,
    egl_surface: egl_ffi::types::EGLSurface,
    wl_egl_surface: WlEglSurface,
    rect_prog: u32,
    rect_u_viewport: i32,
    rect_vbo: u32,
    text_prog: u32,
    text_u_viewport: i32,
    text_u_atlas: i32,
    text_vbo: u32,
    sprite_prog: u32,
    sprite_u_viewport: i32,
    sprite_u_tex: i32,
    sprite_vbo: u32,
    atlas_tex: u32,
    packer: ShelfPacker,
    atlas_entries: HashMap<GlyphRasterKey, Option<AtlasEntry>>,
    sprites: HashMap<u64, SpriteTexEntry>,
    streaming_tex: u32,
    streaming_w: u32,
    streaming_h: u32,
    streaming_scratch: Vec<u8>,
    supports_bgra: bool,
    rect_verts: Vec<RectVertex>,
    text_verts: Vec<TextVertex>,
    sprite_verts: Vec<SpriteVertex>,
    batches: Vec<BatchSpan>,
    width: i32,
    height: i32,
    logical_width: i32,
    logical_height: i32,
    render_scale: i32,
}

impl std::fmt::Debug for GlesSurfaceRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlesSurfaceRenderer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("cached_glyphs", &self.atlas_entries.len())
            .field("cached_sprites", &self.sprites.len())
            .finish_non_exhaustive()
    }
}

impl GlesSurfaceRenderer {
    /// Initializes Wayland-EGL + GLES 2.0 over `wl_display_ptr` and `wl_surface`.
    ///
    /// # Errors
    ///
    /// Returns an explanatory error string when `libEGL.so.1`, Wayland EGL extensions,
    /// ARGB8888 ES2 config, or GLES 2.0 shader compilation are unavailable, allowing callers
    /// to fall back cleanly to `wl_shm` CPU rendering.
    pub fn new(
        wl_display_ptr: *mut c_void,
        wl_surface: &WlSurface,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let dl_egl = open_libegl()?;
        let egl = load_egl(dl_egl);
        let egl_display = get_wayland_egl_display(&egl, wl_display_ptr)?;

        let (mut major, mut minor) = (0, 0);
        // SAFETY: `egl_display` was validated by `get_wayland_egl_display`.
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

        // SAFETY: Binding OpenGL ES API on valid initialized display.
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
        // SAFETY: Valid display and ARGB8888 ES2 config.
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

        // SAFETY: Valid current display, surface, and ES2 context on this client UI thread.
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

        unsafe {
            egl.SwapInterval(egl_display, 0);
        }

        let gl = gl_ffi::Gles2::load_with(|sym| get_gl_proc(&egl, dl_egl, sym));
        Self::init_gl(
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
    fn init_gl(
        dl_egl_handle: *mut c_void,
        egl: egl_ffi::Egl,
        gl: gl_ffi::Gles2,
        egl_display: egl_ffi::types::EGLDisplay,
        egl_context: egl_ffi::types::EGLContext,
        egl_surface: egl_ffi::types::EGLSurface,
        wl_egl_surface: WlEglSurface,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let rect_prog = compile_program(
            &gl,
            RECT_VERT_SRC,
            RECT_FRAG_SRC,
            &[
                c"a_pos",
                c"a_rect_center",
                c"a_rect_half",
                c"a_radius_border",
                c"a_kind_sigma",
                c"a_fill",
                c"a_stroke",
            ],
        )?;
        let text_prog = compile_program(
            &gl,
            TEXT_VERT_SRC,
            TEXT_FRAG_SRC,
            &[c"a_pos", c"a_uv", c"a_color"],
        )?;
        let sprite_prog =
            compile_program(&gl, SPRITE_VERT_SRC, SPRITE_FRAG_SRC, &[c"a_pos", c"a_uv"])?;

        let mut bufs = [0_u32; 3];
        let mut atlas_tex = 0_u32;
        Self::setup_gl_buffers_and_atlas(&gl, &mut bufs, &mut atlas_tex)?;

        let rect_u_viewport = get_uniform_loc(&gl, rect_prog, c"u_viewport");
        let text_u_viewport = get_uniform_loc(&gl, text_prog, c"u_viewport");
        let text_u_atlas = get_uniform_loc(&gl, text_prog, c"u_atlas");
        let sprite_u_viewport = get_uniform_loc(&gl, sprite_prog, c"u_viewport");
        let sprite_u_tex = get_uniform_loc(&gl, sprite_prog, c"u_tex");

        Ok(Self::new_struct_fields(
            dl_egl_handle,
            egl,
            gl,
            egl_display,
            egl_context,
            egl_surface,
            wl_egl_surface,
            rect_prog,
            rect_u_viewport,
            bufs[0],
            text_prog,
            text_u_viewport,
            text_u_atlas,
            bufs[1],
            sprite_prog,
            sprite_u_viewport,
            sprite_u_tex,
            bufs[2],
            atlas_tex,
            width,
            height,
        ))
    }

    fn setup_gl_buffers_and_atlas(
        gl: &gl_ffi::Gles2,
        bufs: &mut [u32; 3],
        atlas_tex: &mut u32,
    ) -> Result<(), String> {
        // SAFETY: Valid current ES2 context allocating VBOs and glyph atlas texture.
        unsafe {
            gl.GenBuffers(3, bufs.as_mut_ptr());
            gl.GenTextures(1, ptr::from_mut(atlas_tex));
            gl.BindTexture(gl_ffi::TEXTURE_2D, *atlas_tex);
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
            let err = gl.GetError();
            if err != gl_ffi::NO_ERROR {
                return Err(format!("GL init error: 0x{err:x}"));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn new_struct_fields(
        dl_egl_handle: *mut c_void,
        egl: egl_ffi::Egl,
        gl: gl_ffi::Gles2,
        egl_display: egl_ffi::types::EGLDisplay,
        egl_context: egl_ffi::types::EGLContext,
        egl_surface: egl_ffi::types::EGLSurface,
        wl_egl_surface: WlEglSurface,
        rect_prog: u32,
        rect_u_viewport: i32,
        rect_vbo: u32,
        text_prog: u32,
        text_u_viewport: i32,
        text_u_atlas: i32,
        text_vbo: u32,
        sprite_prog: u32,
        sprite_u_viewport: i32,
        sprite_u_tex: i32,
        sprite_vbo: u32,
        atlas_tex: u32,
        width: i32,
        height: i32,
    ) -> Self {
        let supports_bgra = unsafe {
            let ext_ptr = gl.GetString(gl_ffi::EXTENSIONS);
            !ext_ptr.is_null()
                && CStr::from_ptr(ext_ptr.cast())
                    .to_string_lossy()
                    .split_whitespace()
                    .any(|ext| ext == "GL_EXT_texture_format_BGRA8888")
        };
        Self {
            dl_egl_handle,
            egl,
            gl,
            egl_display,
            egl_context,
            egl_surface,
            wl_egl_surface,
            rect_prog,
            rect_u_viewport,
            rect_vbo,
            text_prog,
            text_u_viewport,
            text_u_atlas,
            text_vbo,
            sprite_prog,
            sprite_u_viewport,
            sprite_u_tex,
            sprite_vbo,
            atlas_tex,
            packer: ShelfPacker::new(ATLAS_SIZE, ATLAS_SIZE),
            atlas_entries: HashMap::with_capacity(512),
            sprites: HashMap::with_capacity(64),
            streaming_tex: 0,
            streaming_w: 0,
            streaming_h: 0,
            streaming_scratch: Vec::new(),
            supports_bgra,
            rect_verts: Vec::with_capacity(2048),
            text_verts: Vec::with_capacity(4096),
            sprite_verts: Vec::with_capacity(128),
            batches: Vec::with_capacity(64),
            width,
            height,
            logical_width: width,
            logical_height: height,
            render_scale: 1,
        }
    }

    /// Updates Wayland-EGL backing dimensions when window size changes.
    pub fn resize(&mut self, width: i32, height: i32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width != w || self.height != h {
            self.width = w;
            self.height = h;
            self.wl_egl_surface.resize(w, h, 0, 0);
        }
    }

    /// Presents a premultiplied `BGRA8888` CPU framebuffer using one reusable streaming GL texture
    /// with optional partial-damage `glTexSubImage2D` upload (`RUSTYOS-169`).
    ///
    /// Eliminates per-frame `Arc<[u8]>` cloning, `glGenTextures` churn, and 700 MB–2.1 GB VRAM leaks.
    ///
    /// # Errors
    ///
    /// Returns `Err` if EGL context activation, buffer bounds validation, or `eglSwapBuffers` fails.
    #[allow(clippy::too_many_lines)]
    pub fn present_bgra_frame(
        &mut self,
        pixels: &[u8],
        buf_w: u32,
        buf_h: u32,
        buffer_scale: i32,
        damage: Option<Rect>,
        clear_rgba: [u8; 4],
    ) -> Result<(), String> {
        const GL_BGRA_EXT: u32 = 0x80E1;

        let w = i32::try_from(buf_w).unwrap_or(1).max(1);
        let h = i32::try_from(buf_h).unwrap_or(1).max(1);
        let scale = buffer_scale.max(1);
        let vp_w = w.saturating_mul(scale);
        let vp_h = h.saturating_mul(scale);
        let expected_len = (buf_w as usize)
            .saturating_mul(buf_h as usize)
            .saturating_mul(4);
        if pixels.len() < expected_len {
            return Err(String::from(
                "streaming BGRA buffer smaller than dimensions",
            ));
        }

        // SAFETY: Valid current EGL display, window surface, and ES2 context.
        unsafe {
            if self.egl.MakeCurrent(
                self.egl_display,
                self.egl_surface,
                self.egl_surface,
                self.egl_context,
            ) == egl_ffi::FALSE
            {
                return Err(String::from("eglMakeCurrent lost during streaming frame"));
            }
        }
        self.resize(vp_w, vp_h);

        let need_alloc =
            self.streaming_tex == 0 || self.streaming_w != buf_w || self.streaming_h != buf_h;
        let gl_fmt = if self.supports_bgra {
            GL_BGRA_EXT
        } else {
            gl_ffi::RGBA
        };

        // SAFETY: Valid current ES2 context; texture upload slices are bounds-checked above.
        unsafe {
            if self.streaming_tex == 0 {
                self.gl
                    .GenTextures(1, ptr::from_mut(&mut self.streaming_tex));
            }
            self.gl.BindTexture(gl_ffi::TEXTURE_2D, self.streaming_tex);
            self.gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_MIN_FILTER,
                gl_ffi::LINEAR as i32,
            );
            self.gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_MAG_FILTER,
                gl_ffi::LINEAR as i32,
            );
            self.gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_WRAP_S,
                gl_ffi::CLAMP_TO_EDGE as i32,
            );
            self.gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_WRAP_T,
                gl_ffi::CLAMP_TO_EDGE as i32,
            );
            self.gl.PixelStorei(gl_ffi::UNPACK_ALIGNMENT, 4);

            if need_alloc {
                let upload_ptr = if self.supports_bgra {
                    pixels.as_ptr()
                } else {
                    swizzle_bgra_region_into(
                        pixels,
                        buf_w,
                        0,
                        0,
                        buf_w,
                        buf_h,
                        &mut self.streaming_scratch,
                    );
                    self.streaming_scratch.as_ptr()
                };
                self.gl.TexImage2D(
                    gl_ffi::TEXTURE_2D,
                    0,
                    gl_fmt as i32,
                    w,
                    h,
                    0,
                    gl_fmt,
                    gl_ffi::UNSIGNED_BYTE,
                    upload_ptr.cast(),
                );
                self.streaming_w = buf_w;
                self.streaming_h = buf_h;
            } else {
                let (dx, dy, dw, dh) = clamp_damage_to_buffer(damage, buf_w, buf_h);
                if dw > 0 && dh > 0 {
                    let upload_ptr = if self.supports_bgra && dx == 0 && dw == buf_w {
                        let start_byte = (dy as usize) * (buf_w as usize) * 4;
                        pixels[start_byte..].as_ptr()
                    } else if self.supports_bgra {
                        copy_bgra_region_into(
                            pixels,
                            buf_w,
                            dx,
                            dy,
                            dw,
                            dh,
                            &mut self.streaming_scratch,
                        );
                        self.streaming_scratch.as_ptr()
                    } else {
                        swizzle_bgra_region_into(
                            pixels,
                            buf_w,
                            dx,
                            dy,
                            dw,
                            dh,
                            &mut self.streaming_scratch,
                        );
                        self.streaming_scratch.as_ptr()
                    };
                    self.gl.TexSubImage2D(
                        gl_ffi::TEXTURE_2D,
                        0,
                        dx as i32,
                        dy as i32,
                        dw as i32,
                        dh as i32,
                        gl_fmt,
                        gl_ffi::UNSIGNED_BYTE,
                        upload_ptr.cast(),
                    );
                }
            }

            let clear_f = rgba_u8_to_f32_premul(clear_rgba);
            self.gl.Viewport(0, 0, vp_w, vp_h);
            self.gl.Disable(gl_ffi::SCISSOR_TEST);
            self.gl.Enable(gl_ffi::BLEND);
            self.gl.BlendFunc(gl_ffi::ONE, gl_ffi::ONE_MINUS_SRC_ALPHA);
            self.gl
                .ClearColor(clear_f[0], clear_f[1], clear_f[2], clear_f[3]);
            self.gl.Clear(gl_ffi::COLOR_BUFFER_BIT);
        }

        let wf = w as f32;
        let hf = h as f32;
        self.sprite_verts.clear();
        self.sprite_verts.extend_from_slice(&[
            SpriteVertex {
                pos: [0.0, 0.0],
                uv: [0.0, 0.0],
            },
            SpriteVertex {
                pos: [wf, 0.0],
                uv: [1.0, 0.0],
            },
            SpriteVertex {
                pos: [wf, hf],
                uv: [1.0, 1.0],
            },
            SpriteVertex {
                pos: [0.0, 0.0],
                uv: [0.0, 0.0],
            },
            SpriteVertex {
                pos: [wf, hf],
                uv: [1.0, 1.0],
            },
            SpriteVertex {
                pos: [0.0, hf],
                uv: [0.0, 1.0],
            },
        ]);

        // SAFETY: Valid ES2 context and `streaming_tex`.
        unsafe {
            self.flush_sprites(w, h, self.streaming_tex, 0, self.sprite_verts.len());
            if self.egl.SwapBuffers(self.egl_display, self.egl_surface) == egl_ffi::FALSE {
                return Err(String::from("eglSwapBuffers failed"));
            }
        }
        Ok(())
    }

    /// Executes `draw_list` in exact layer order on GLES 2.0 and swaps buffers.
    ///
    /// # Errors
    ///
    /// Returns `Err` if EGL context activation or `eglSwapBuffers` fails.
    pub fn render_draw_list(
        &mut self,
        list: &UiDrawList,
        fonts: &mut UiFontEngine,
        width: i32,
        height: i32,
        clear_rgba: [u8; 4],
    ) -> Result<(), String> {
        self.render_draw_list_scaled(list, fonts, width, height, 1, clear_rgba)
    }

    /// Renders `list` at logical dimensions `width x height` into a `scale`-multiplied `HiDPI`
    /// physical EGL surface (`scale >= 1`).
    ///
    /// # Errors
    ///
    /// Returns `Err` if EGL context activation or `eglSwapBuffers` fails.
    pub fn render_draw_list_scaled(
        &mut self,
        list: &UiDrawList,
        fonts: &mut UiFontEngine,
        width: i32,
        height: i32,
        scale: i32,
        clear_rgba: [u8; 4],
    ) -> Result<(), String> {
        let w = width.max(1);
        let h = height.max(1);
        let s = scale.max(1);
        let phys_w = w.saturating_mul(s);
        let phys_h = h.saturating_mul(s);
        // SAFETY: Valid current EGL display, window surface, and ES2 context.
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
        self.resize(phys_w, phys_h);
        self.logical_width = w;
        self.logical_height = h;
        self.render_scale = s;
        self.rect_verts.clear();
        self.text_verts.clear();
        self.sprite_verts.clear();
        self.batches.clear();

        let clear_f = rgba_u8_to_f32_premul(clear_rgba);
        // SAFETY: Valid current ES2 context; clear framebuffer before compiling batches so
        // mid-frame glyph atlas overflows can safely flush accumulated text batches (`RUSTYOS-169`).
        unsafe {
            self.gl.Viewport(0, 0, phys_w, phys_h);
            self.gl.Disable(gl_ffi::SCISSOR_TEST);
            self.gl.Enable(gl_ffi::BLEND);
            self.gl.BlendFunc(gl_ffi::ONE, gl_ffi::ONE_MINUS_SRC_ALPHA);
            self.gl
                .ClearColor(clear_f[0], clear_f[1], clear_f[2], clear_f[3]);
            self.gl.Clear(gl_ffi::COLOR_BUFFER_BIT);
        }

        self.compile_batches(list, fonts, w, h);
        self.flush_pending_batches_scaled(w, h, s);

        // SAFETY: Valid current ES2 context.
        unsafe {
            self.gl.Disable(gl_ffi::SCISSOR_TEST);
            if self.egl.SwapBuffers(self.egl_display, self.egl_surface) == egl_ffi::FALSE {
                return Err(String::from("eglSwapBuffers failed"));
            }
        }
        Ok(())
    }

    fn flush_pending_batches_scaled(&mut self, w: i32, h: i32, scale: i32) {
        let s = scale.max(1);
        // SAFETY: Valid current ES2 context; VBO pointers/slices match active batch spans.
        unsafe {
            for span in &self.batches {
                if let Some(sc) = span.clip {
                    if sc.width == 0 || sc.height == 0 {
                        continue;
                    }
                    self.gl.Enable(gl_ffi::SCISSOR_TEST);
                    let sc_y = (h - (sc.y + sc.height as i32)).max(0);
                    self.gl.Scissor(
                        sc.x.max(0).saturating_mul(s),
                        sc_y.saturating_mul(s),
                        (sc.width as i32).saturating_mul(s),
                        (sc.height as i32).saturating_mul(s),
                    );
                } else {
                    self.gl.Disable(gl_ffi::SCISSOR_TEST);
                }

                match span.kind {
                    DrawBatchKind::Rect => self.flush_rects(w, h, span.start, span.end),
                    DrawBatchKind::Text => self.flush_text(w, h, span.start, span.end),
                    DrawBatchKind::Sprite(tex) => {
                        self.flush_sprites(w, h, tex, span.start, span.end);
                    }
                }
            }
            self.gl.Disable(gl_ffi::SCISSOR_TEST);
        }
        self.rect_verts.clear();
        self.text_verts.clear();
        self.sprite_verts.clear();
        self.batches.clear();
    }

    fn record_batch(&mut self, kind: DrawBatchKind, clip: Option<Rect>, start: usize, end: usize) {
        if end <= start {
            return;
        }
        if let Some(last) = self.batches.last_mut()
            && last.kind == kind
            && last.clip == clip
            && last.end == start
        {
            last.end = end;
            return;
        }
        self.batches.push(BatchSpan {
            kind,
            clip,
            start,
            end,
        });
    }

    fn compile_batches(&mut self, list: &UiDrawList, fonts: &mut UiFontEngine, w: i32, h: i32) {
        let full = Rect::new(0, 0, w as u32, h as u32);
        let mut clip_stack: Vec<Option<Rect>> = vec![None];

        for cmd in &list.commands {
            let active_clip = *clip_stack.last().unwrap_or(&None);
            match cmd {
                UiDrawCmd::PushClip(r) => {
                    let base = active_clip.unwrap_or(full);
                    clip_stack.push(Some(intersect_rect(base, *r)));
                }
                UiDrawCmd::PopClip => {
                    if clip_stack.len() > 1 {
                        clip_stack.pop();
                    }
                }
                other => self.compile_draw_cmd(other, fonts, active_clip),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_rect_cmd(
        &mut self,
        active_clip: Option<Rect>,
        rect: Rect,
        radius: f32,
        border_width: f32,
        kind: f32,
        fill: [u8; 4],
        stroke: [u8; 4],
    ) {
        let s = self.rect_verts.len();
        self.push_rect_quad(rect, radius, border_width, kind, 0.0, fill, stroke);
        let e = self.rect_verts.len();
        self.record_batch(DrawBatchKind::Rect, active_clip, s, e);
    }

    fn compile_draw_cmd(
        &mut self,
        cmd: &UiDrawCmd,
        fonts: &mut UiFontEngine,
        active_clip: Option<Rect>,
    ) {
        match cmd {
            UiDrawCmd::PushClip(_) | UiDrawCmd::PopClip => {}
            UiDrawCmd::Rect { rect, color } => {
                self.record_rect_cmd(active_clip, *rect, 0.0, 0.0, 0.0, *color, [0, 0, 0, 0]);
            }
            UiDrawCmd::RoundedRect {
                rect,
                radius,
                fill,
                border_width,
                border_color,
                squircle,
            } => {
                self.record_rect_cmd(
                    active_clip,
                    *rect,
                    *radius,
                    *border_width,
                    if *squircle { 1.0 } else { 0.0 },
                    *fill,
                    *border_color,
                );
            }
            UiDrawCmd::Circle {
                cx,
                cy,
                radius,
                fill,
                stroke_width,
                stroke_color,
            } => {
                let d = (radius * 2.0).ceil().max(1.0) as u32;
                let r = Rect::new(
                    (cx - radius).floor() as i32,
                    (cy - radius).floor() as i32,
                    d,
                    d,
                );
                self.record_rect_cmd(
                    active_clip,
                    r,
                    *radius,
                    *stroke_width,
                    0.0,
                    *fill,
                    *stroke_color,
                );
            }
            UiDrawCmd::Line {
                x0,
                y0,
                x1,
                y1,
                width,
                color,
            } => {
                let s = self.rect_verts.len();
                self.push_line_quad(*x0, *y0, *x1, *y1, *width, *color);
                let e = self.rect_verts.len();
                self.record_batch(DrawBatchKind::Rect, active_clip, s, e);
            }
            other => self.compile_rich_cmd(other, fonts, active_clip),
        }
    }

    fn compile_rich_cmd(
        &mut self,
        cmd: &UiDrawCmd,
        fonts: &mut UiFontEngine,
        active_clip: Option<Rect>,
    ) {
        match cmd {
            UiDrawCmd::Shadow {
                rect,
                radius,
                elevation,
                is_dark,
            } => {
                let s = self.rect_verts.len();
                let rgb = if *is_dark { [0, 0, 0] } else { [15, 20, 30] };
                self.push_elevation_shadow(*rect, *radius, *elevation, rgb);
                let e = self.rect_verts.len();
                self.record_batch(DrawBatchKind::Rect, active_clip, s, e);
            }
            UiDrawCmd::Text {
                text,
                x,
                y_baseline,
                size_px,
                bold,
                color,
            } => {
                let mut s = self.text_verts.len();
                self.push_text_run(
                    fonts,
                    text,
                    *x,
                    *y_baseline,
                    *size_px,
                    *bold,
                    *color,
                    active_clip,
                    &mut s,
                );
                let e = self.text_verts.len();
                self.record_batch(DrawBatchKind::Text, active_clip, s, e);
            }
            UiDrawCmd::Image {
                rect,
                cache_key,
                width,
                height,
                pixels,
                format,
            } => {
                if let Some(tex) =
                    self.ensure_sprite_texture(*cache_key, *width, *height, pixels, *format)
                {
                    let s = self.sprite_verts.len();
                    self.push_sprite_quad(*rect);
                    let e = self.sprite_verts.len();
                    self.record_batch(DrawBatchKind::Sprite(tex), active_clip, s, e);
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_rect_quad(
        &mut self,
        rect: Rect,
        radius: f32,
        border_width: f32,
        kind: f32,
        shadow_sigma: f32,
        fill: [u8; 4],
        stroke: [u8; 4],
    ) {
        let pad = if shadow_sigma > 0.0 {
            (shadow_sigma * 2.2).ceil()
        } else {
            1.0
        };
        let x0 = rect.x as f32;
        let y0 = rect.y as f32;
        let hw = rect.width as f32 * 0.5;
        let hh = rect.height as f32 * 0.5;
        let cx = x0 + hw;
        let cy = y0 + hh;
        let x1 = x0 + rect.width as f32;
        let y1 = y0 + rect.height as f32;
        let qx0 = x0 - pad;
        let qy0 = y0 - pad;
        let qx1 = x1 + pad;
        let qy1 = y1 + pad;
        let fill_f = rgba_u8_to_f32_straight(fill);
        let stroke_f = rgba_u8_to_f32_straight(stroke);

        let mk = |px: f32, py: f32| RectVertex {
            pos: [px, py],
            center: [cx, cy],
            half: [hw, hh],
            radius_border: [radius, border_width],
            kind_sigma: [kind, shadow_sigma],
            fill: fill_f,
            stroke: stroke_f,
        };
        self.rect_verts.extend_from_slice(&[
            mk(qx0, qy0),
            mk(qx1, qy0),
            mk(qx1, qy1),
            mk(qx0, qy0),
            mk(qx1, qy1),
            mk(qx0, qy1),
        ]);
    }

    fn push_line_quad(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: [u8; 4]) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = dx.hypot(dy);
        if len < 0.01 {
            return;
        }
        if dy.abs() <= 0.1 || dx.abs() <= 0.1 {
            let lx = x0.min(x1) - width * 0.5;
            let ly = y0.min(y1) - width * 0.5;
            let lw = dx.abs().max(width).ceil() as u32;
            let lh = dy.abs().max(width).ceil() as u32;
            self.push_rect_quad(
                Rect::new(lx.floor() as i32, ly.floor() as i32, lw.max(1), lh.max(1)),
                0.0,
                0.0,
                0.0,
                0.0,
                color,
                [0, 0, 0, 0],
            );
            return;
        }
        // True rotated segment quad along normal `(-dy/len, dx/len) * half_w` (`RUSTYOS-169`).
        let half_w = (width * 0.5).max(0.5);
        let nx = (-dy / len) * half_w;
        let ny = (dx / len) * half_w;
        let fill_f = rgba_u8_to_f32_straight(color);
        let cx = f32::midpoint(x0, x1);
        let cy = f32::midpoint(y0, y1);
        let big_half = (len + width).max(4.0);
        let mk = |px: f32, py: f32| RectVertex {
            pos: [px, py],
            center: [cx, cy],
            half: [big_half, big_half],
            radius_border: [0.0, 0.0],
            kind_sigma: [0.0, 0.0],
            fill: fill_f,
            stroke: [0.0, 0.0, 0.0, 0.0],
        };
        let p0 = mk(x0 + nx, y0 + ny);
        let p1 = mk(x1 + nx, y1 + ny);
        let p2 = mk(x1 - nx, y1 - ny);
        let p3 = mk(x0 - nx, y0 - ny);
        self.rect_verts.extend_from_slice(&[p0, p1, p2, p0, p2, p3]);
    }

    fn push_elevation_shadow(
        &mut self,
        rect: Rect,
        radius: i32,
        elevation: ShadowElevation,
        rgb: [u8; 3],
    ) {
        if elevation == ShadowElevation::None {
            return;
        }
        let style = ShadowStyle::for_elevation(elevation);
        for lobe in style.lobes {
            if lobe.opacity <= 0.002 {
                continue;
            }
            let lobe_rect = Rect::new(
                rect.x + lobe.offset.0.round() as i32,
                rect.y + lobe.offset.1.round() as i32,
                rect.width,
                rect.height,
            );
            let alpha = (lobe.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            self.push_rect_quad(
                lobe_rect,
                radius as f32,
                0.0,
                1.0,
                lobe.sigma.max(0.75),
                [rgb[0], rgb[1], rgb[2], alpha],
                [0, 0, 0, 0],
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_text_run(
        &mut self,
        fonts: &mut UiFontEngine,
        text: &str,
        x: f32,
        y_baseline: f32,
        size_px: f32,
        bold: bool,
        color: [u8; 4],
        active_clip: Option<Rect>,
        batch_start: &mut usize,
    ) {
        let col = rgba_u8_to_f32_straight(color);
        let weight = if bold {
            FontWeight::SemiBold
        } else {
            FontWeight::Regular
        };
        let size_tenths = (size_px.clamp(6.0, 200.0) * 10.0).round() as u16;
        let mut cx = x;

        for ch in text.chars() {
            let key = GlyphRasterKey {
                weight,
                size_tenths,
                ch,
            };
            let entry_opt = if let Some(&cached) = self.atlas_entries.get(&key) {
                cached
            } else {
                let inserted =
                    self.upload_glyph(fonts, key, ch, size_px, bold, active_clip, batch_start);
                self.atlas_entries.insert(key, inserted);
                inserted
            };
            let Some(entry) = entry_opt else {
                continue;
            };
            if entry.width > 0 && entry.height > 0 {
                let gx0 = (cx + entry.xmin as f32).round();
                let gy0 = (y_baseline - entry.ymin as f32 - entry.height as f32).round();
                let gx1 = gx0 + entry.width as f32;
                let gy1 = gy0 + entry.height as f32;
                self.text_verts.extend_from_slice(&[
                    TextVertex {
                        pos: [gx0, gy0],
                        uv: [entry.u0, entry.v0],
                        color: col,
                    },
                    TextVertex {
                        pos: [gx1, gy0],
                        uv: [entry.u1, entry.v0],
                        color: col,
                    },
                    TextVertex {
                        pos: [gx1, gy1],
                        uv: [entry.u1, entry.v1],
                        color: col,
                    },
                    TextVertex {
                        pos: [gx0, gy0],
                        uv: [entry.u0, entry.v0],
                        color: col,
                    },
                    TextVertex {
                        pos: [gx1, gy1],
                        uv: [entry.u1, entry.v1],
                        color: col,
                    },
                    TextVertex {
                        pos: [gx0, gy1],
                        uv: [entry.u0, entry.v1],
                        color: col,
                    },
                ]);
            }
            cx += entry.advance_x;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn upload_glyph(
        &mut self,
        fonts: &mut UiFontEngine,
        _key: GlyphRasterKey,
        ch: char,
        size_px: f32,
        bold: bool,
        active_clip: Option<Rect>,
        batch_start: &mut usize,
    ) -> Option<AtlasEntry> {
        let glyph = fonts.rasterize_glyph(ch, size_px, bold);
        if glyph.width == 0 || glyph.height == 0 {
            return Some(AtlasEntry {
                u0: 0.0,
                v0: 0.0,
                u1: 0.0,
                v1: 0.0,
                width: 0,
                height: 0,
                xmin: 0,
                ymin: 0,
                advance_x: glyph.advance_x,
            });
        }
        let w = glyph.width as u32;
        let h = glyph.height as u32;
        let (ax, ay) = if let Some(pos) = self.packer.allocate(w, h) {
            pos
        } else {
            // Record the in-progress text run before flushing so already-pushed glyphs
            // are rendered rather than dropped when `flush_pending_batches` clears `text_verts`.
            if self.text_verts.len() > *batch_start {
                let end = self.text_verts.len();
                self.record_batch(DrawBatchKind::Text, active_clip, *batch_start, end);
            }
            if !self.batches.is_empty() || !self.text_verts.is_empty() {
                self.flush_pending_batches_scaled(
                    self.logical_width,
                    self.logical_height,
                    self.render_scale,
                );
            }
            *batch_start = 0;
            self.reset_glyph_atlas();
            self.packer.allocate(w, h)?
        };

        // SAFETY: Valid current ES2 context and `atlas_tex`; pixel slice fits `w * h`.
        unsafe {
            self.gl.BindTexture(gl_ffi::TEXTURE_2D, self.atlas_tex);
            self.gl.PixelStorei(gl_ffi::UNPACK_ALIGNMENT, 1);
            self.gl.TexSubImage2D(
                gl_ffi::TEXTURE_2D,
                0,
                ax as i32,
                ay as i32,
                w as i32,
                h as i32,
                gl_ffi::ALPHA,
                gl_ffi::UNSIGNED_BYTE,
                glyph.coverage.as_ptr().cast(),
            );
        }

        let inv = 1.0 / ATLAS_SIZE as f32;
        Some(AtlasEntry {
            u0: ax as f32 * inv,
            v0: ay as f32 * inv,
            u1: (ax + w) as f32 * inv,
            v1: (ay + h) as f32 * inv,
            width: w as i32,
            height: h as i32,
            xmin: glyph.xmin,
            ymin: glyph.ymin,
            advance_x: glyph.advance_x,
        })
    }

    fn reset_glyph_atlas(&mut self) {
        self.packer.reset();
        self.atlas_entries.clear();
        let zeros = vec![0_u8; (ATLAS_SIZE * ATLAS_SIZE) as usize];
        // SAFETY: Clears entire `ATLAS_SIZE x ATLAS_SIZE` alpha texture on valid ES2 context.
        unsafe {
            self.gl.BindTexture(gl_ffi::TEXTURE_2D, self.atlas_tex);
            self.gl.PixelStorei(gl_ffi::UNPACK_ALIGNMENT, 1);
            self.gl.TexSubImage2D(
                gl_ffi::TEXTURE_2D,
                0,
                0,
                0,
                ATLAS_SIZE as i32,
                ATLAS_SIZE as i32,
                gl_ffi::ALPHA,
                gl_ffi::UNSIGNED_BYTE,
                zeros.as_ptr().cast(),
            );
        }
    }

    fn ensure_sprite_texture(
        &mut self,
        key: u64,
        width: u32,
        height: u32,
        pixels: &[u8],
        format: PixelFormat,
    ) -> Option<u32> {
        if let Some(existing) = self.sprites.get(&key)
            && existing.width == width
            && existing.height == height
        {
            return Some(existing.tex);
        }
        if pixels.len()
            < (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(4)
        {
            return None;
        }
        if self.sprites.len() >= MAX_SPRITE_CACHE {
            // Flush pending batches before deleting cached GL textures so sprites already
            // referenced in the current frame draw cleanly before eviction.
            if !self.batches.is_empty() {
                self.flush_pending_batches_scaled(
                    self.logical_width,
                    self.logical_height,
                    self.render_scale,
                );
            }
            self.clear_sprite_cache();
        }

        let mut rgba_tmp = Vec::new();
        let upload_ptr = match format {
            PixelFormat::Rgba8888 => pixels.as_ptr(),
            PixelFormat::Bgra8888 => {
                rgba_tmp.resize(pixels.len(), 0);
                for (dst, src) in rgba_tmp
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(pixels.as_chunks::<4>().0.iter())
                {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    dst[3] = src[3];
                }
                rgba_tmp.as_ptr()
            }
        };

        let mut tex = 0_u32;
        // SAFETY: Valid current ES2 context; texture dimensions match `upload_ptr` RGBA buffer.
        unsafe {
            self.gl.GenTextures(1, ptr::from_mut(&mut tex));
            self.gl.BindTexture(gl_ffi::TEXTURE_2D, tex);
            self.gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_MIN_FILTER,
                gl_ffi::LINEAR as i32,
            );
            self.gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_MAG_FILTER,
                gl_ffi::LINEAR as i32,
            );
            self.gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_WRAP_S,
                gl_ffi::CLAMP_TO_EDGE as i32,
            );
            self.gl.TexParameteri(
                gl_ffi::TEXTURE_2D,
                gl_ffi::TEXTURE_WRAP_T,
                gl_ffi::CLAMP_TO_EDGE as i32,
            );
            self.gl.PixelStorei(gl_ffi::UNPACK_ALIGNMENT, 4);
            self.gl.TexImage2D(
                gl_ffi::TEXTURE_2D,
                0,
                gl_ffi::RGBA as i32,
                width as i32,
                height as i32,
                0,
                gl_ffi::RGBA,
                gl_ffi::UNSIGNED_BYTE,
                upload_ptr.cast(),
            );
        }
        if let Some(old) = self
            .sprites
            .insert(key, SpriteTexEntry { tex, width, height })
        {
            // SAFETY: Deletes replaced old GL texture name.
            unsafe { self.gl.DeleteTextures(1, ptr::from_ref(&old.tex)) };
        }
        Some(tex)
    }

    fn clear_sprite_cache(&mut self) {
        for entry in self.sprites.values() {
            // SAFETY: Valid active GL context deleting cached sprite texture.
            unsafe { self.gl.DeleteTextures(1, ptr::from_ref(&entry.tex)) };
        }
        self.sprites.clear();
    }

    fn push_sprite_quad(&mut self, rect: Rect) {
        let x0 = rect.x as f32;
        let y0 = rect.y as f32;
        let x1 = rect.right() as f32;
        let y1 = rect.bottom() as f32;
        self.sprite_verts.extend_from_slice(&[
            SpriteVertex {
                pos: [x0, y0],
                uv: [0.0, 0.0],
            },
            SpriteVertex {
                pos: [x1, y0],
                uv: [1.0, 0.0],
            },
            SpriteVertex {
                pos: [x1, y1],
                uv: [1.0, 1.0],
            },
            SpriteVertex {
                pos: [x0, y0],
                uv: [0.0, 0.0],
            },
            SpriteVertex {
                pos: [x1, y1],
                uv: [1.0, 1.0],
            },
            SpriteVertex {
                pos: [x0, y1],
                uv: [0.0, 1.0],
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
            for idx in 0..=6 {
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
                2,
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
                40 as *const c_void,
            );
            self.gl.VertexAttribPointer(
                6,
                4,
                gl_ffi::FLOAT,
                gl_ffi::FALSE,
                stride,
                56 as *const c_void,
            );
            self.gl.DrawArrays(gl_ffi::TRIANGLES, 0, slice.len() as i32);
            for idx in 0..=6 {
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

    unsafe fn flush_sprites(&self, win_w: i32, win_h: i32, tex: u32, start: usize, end: usize) {
        if end <= start {
            return;
        }
        let slice = &self.sprite_verts[start..end];
        let stride = std::mem::size_of::<SpriteVertex>() as i32;
        unsafe {
            self.gl.UseProgram(self.sprite_prog);
            self.gl
                .Uniform2f(self.sprite_u_viewport, win_w as f32, win_h as f32);
            self.gl.ActiveTexture(gl_ffi::TEXTURE0);
            self.gl.BindTexture(gl_ffi::TEXTURE_2D, tex);
            self.gl.Uniform1i(self.sprite_u_tex, 0);
            self.gl.BindBuffer(gl_ffi::ARRAY_BUFFER, self.sprite_vbo);
            self.gl.BufferData(
                gl_ffi::ARRAY_BUFFER,
                std::mem::size_of_val(slice) as isize,
                slice.as_ptr().cast(),
                gl_ffi::STREAM_DRAW,
            );
            self.gl.EnableVertexAttribArray(0);
            self.gl.EnableVertexAttribArray(1);
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
            self.gl.DrawArrays(gl_ffi::TRIANGLES, 0, slice.len() as i32);
            self.gl.DisableVertexAttribArray(0);
            self.gl.DisableVertexAttribArray(1);
        }
    }
}

impl Drop for GlesSurfaceRenderer {
    fn drop(&mut self) {
        // SAFETY: Tears down GL objects cleanly before EGL surface/context and `wl_egl_window` drop.
        unsafe {
            if self.egl.MakeCurrent(
                self.egl_display,
                self.egl_surface,
                self.egl_surface,
                self.egl_context,
            ) == egl_ffi::TRUE
            {
                self.clear_sprite_cache();
                if self.streaming_tex != 0 {
                    self.gl
                        .DeleteTextures(1, ptr::from_ref(&self.streaming_tex));
                }
                let bufs = [self.rect_vbo, self.text_vbo, self.sprite_vbo];
                self.gl.DeleteBuffers(3, bufs.as_ptr());
                self.gl.DeleteTextures(1, ptr::from_ref(&self.atlas_tex));
                self.gl.DeleteProgram(self.rect_prog);
                self.gl.DeleteProgram(self.text_prog);
                self.gl.DeleteProgram(self.sprite_prog);
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

fn clamp_damage_to_buffer(damage: Option<Rect>, buf_w: u32, buf_h: u32) -> (u32, u32, u32, u32) {
    let Some(r) = damage else {
        return (0, 0, buf_w, buf_h);
    };
    let x0 = r.x.max(0) as u32;
    let y0 = r.y.max(0) as u32;
    let x1 = (r.x.saturating_add(r.width as i32)).max(0) as u32;
    let y1 = (r.y.saturating_add(r.height as i32)).max(0) as u32;
    let cx0 = x0.min(buf_w);
    let cy0 = y0.min(buf_h);
    let cx1 = x1.min(buf_w);
    let cy1 = y1.min(buf_h);
    (cx0, cy0, cx1.saturating_sub(cx0), cy1.saturating_sub(cy0))
}

fn copy_bgra_region_into(
    src: &[u8],
    buf_w: u32,
    dx: u32,
    dy: u32,
    dw: u32,
    dh: u32,
    out: &mut Vec<u8>,
) {
    let row_bytes = (dw as usize) * 4;
    out.resize(row_bytes * (dh as usize), 0);
    let stride = (buf_w as usize) * 4;
    for row in 0..(dh as usize) {
        let src_off = ((dy as usize) + row) * stride + (dx as usize) * 4;
        let dst_off = row * row_bytes;
        out[dst_off..dst_off + row_bytes].copy_from_slice(&src[src_off..src_off + row_bytes]);
    }
}

fn swizzle_bgra_region_into(
    src: &[u8],
    buf_w: u32,
    dx: u32,
    dy: u32,
    dw: u32,
    dh: u32,
    out: &mut Vec<u8>,
) {
    let row_bytes = (dw as usize) * 4;
    out.resize(row_bytes * (dh as usize), 0);
    let stride = (buf_w as usize) * 4;
    for row in 0..(dh as usize) {
        let src_off = ((dy as usize) + row) * stride + (dx as usize) * 4;
        let dst_off = row * row_bytes;
        let src_row = &src[src_off..src_off + row_bytes];
        let dst_row = &mut out[dst_off..dst_off + row_bytes];
        for (dst_px, src_px) in dst_row
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src_row.as_chunks::<4>().0.iter())
        {
            dst_px[0] = src_px[2];
            dst_px[1] = src_px[1];
            dst_px[2] = src_px[0];
            dst_px[3] = src_px[3];
        }
    }
}

fn rgba_u8_to_f32_straight(c: [u8; 4]) -> [f32; 4] {
    [
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
        f32::from(c[3]) / 255.0,
    ]
}

fn rgba_u8_to_f32_premul(c: [u8; 4]) -> [f32; 4] {
    let a = f32::from(c[3]) / 255.0;
    [
        (f32::from(c[0]) / 255.0) * a,
        (f32::from(c[1]) / 255.0) * a,
        (f32::from(c[2]) / 255.0) * a,
        a,
    ]
}

fn open_libegl() -> Result<*mut c_void, String> {
    for candidate in [c"libEGL.so.1", c"libEGL.so"] {
        // SAFETY: Passing static NUL-terminated library name to `dlopen`.
        let handle = unsafe { libc::dlopen(candidate.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if !handle.is_null() {
            return Ok(handle);
        }
    }
    Err(String::from("unable to open libEGL.so.1"))
}

fn load_egl(dl_egl: *mut c_void) -> egl_ffi::Egl {
    egl_ffi::Egl::load_with(|sym| {
        let Ok(c) = CString::new(sym) else {
            return ptr::null();
        };
        // SAFETY: `dl_egl` is valid opened handle from `open_libegl`.
        unsafe { libc::dlsym(dl_egl, c.as_ptr()) }
    })
}

fn get_gl_proc(egl: &egl_ffi::Egl, dl_egl: *mut c_void, sym: &str) -> *const c_void {
    let Ok(c) = CString::new(sym) else {
        return ptr::null();
    };
    // SAFETY: Valid loaded `egl` and `dl_egl` handle with NUL-terminated ASCII symbol name.
    let p = unsafe { egl.GetProcAddress(c.as_ptr()) } as *const c_void;
    if !p.is_null() {
        return p;
    }
    unsafe { libc::dlsym(dl_egl, c.as_ptr()) }
}

fn get_wayland_egl_display(
    egl: &egl_ffi::Egl,
    wl_display_ptr: *mut c_void,
) -> Result<egl_ffi::types::EGLDisplay, String> {
    if egl.GetPlatformDisplayEXT.is_loaded() {
        // SAFETY: Valid `wl_display*` from `wayland_client::Connection::display_ptr`.
        let dpy = unsafe {
            egl.GetPlatformDisplayEXT(egl_ffi::PLATFORM_WAYLAND_KHR, wl_display_ptr, ptr::null())
        };
        if !dpy.is_null() {
            return Ok(dpy);
        }
    }
    // SAFETY: Fallback for drivers without `EGL_EXT_platform_base`.
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
    // SAFETY: Valid initialized `dpy`, NUL-terminated attribute list `EGL_NONE`.
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

fn compile_program(
    gl: &gl_ffi::Gles2,
    vert_src: &str,
    frag_src: &str,
    attribs: &[&CStr],
) -> Result<u32, String> {
    let vs = compile_shader(gl, gl_ffi::VERTEX_SHADER, vert_src)?;
    let fs = compile_shader(gl, gl_ffi::FRAGMENT_SHADER, frag_src)?;
    // SAFETY: Valid active ES2 context and compiled shaders.
    unsafe {
        let prog = gl.CreateProgram();
        gl.AttachShader(prog, vs);
        gl.AttachShader(prog, fs);
        for (idx, name) in attribs.iter().enumerate() {
            gl.BindAttribLocation(prog, idx as u32, name.as_ptr());
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
    // SAFETY: Valid active ES2 context; UTF-8 shader passed with explicit length.
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
    // SAFETY: Valid linked ES2 program and NUL-terminated uniform identifier.
    unsafe { gl.GetUniformLocation(prog, name.as_ptr()) }
}

pub(crate) const RECT_VERT_SRC: &str = "#version 100
attribute vec2 a_pos;
attribute vec2 a_rect_center;
attribute vec2 a_rect_half;
attribute vec2 a_radius_border;
attribute vec2 a_kind_sigma;
attribute vec4 a_fill;
attribute vec4 a_stroke;
uniform vec2 u_viewport;
varying vec2 v_pos;
varying vec2 v_center;
varying vec2 v_half;
varying vec2 v_rb;
varying vec2 v_ks;
varying vec4 v_fill;
varying vec4 v_stroke;
void main() {
    vec2 ndc = (a_pos / u_viewport) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    v_pos = a_pos;
    v_center = a_rect_center;
    v_half = a_rect_half;
    v_rb = a_radius_border;
    v_ks = a_kind_sigma;
    v_fill = a_fill;
    v_stroke = a_stroke;
}
";

pub(crate) const RECT_FRAG_SRC: &str = "#version 100
precision mediump float;
varying vec2 v_pos;
varying vec2 v_center;
varying vec2 v_half;
varying vec2 v_rb;
varying vec2 v_ks;
varying vec4 v_fill;
varying vec4 v_stroke;

float squircleDist(vec2 p, vec2 h, float r) {
    float minH = min(h.x, h.y);
    float rc = clamp(r, 0.0, minH);
    if (abs(rc - minH) < 0.75) {
        vec2 q = abs(p) - (h - vec2(rc));
        return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - rc;
    }
    float rext = min(rc * 1.42, minH);
    vec2 q = abs(p) - (h - vec2(rext));
    if (q.x > 0.0 && q.y > 0.0) {
        vec2 n = q / max(rext, 0.0001);
        float d3 = n.x * n.x * n.x + n.y * n.y * n.y;
        return (pow(max(d3, 0.0), 1.0 / 3.0) - 1.0) * rext;
    }
    return max(q.x, q.y) - rext;
}

float roundDist(vec2 p, vec2 h, float r) {
    float rc = clamp(r, 0.0, min(h.x, h.y));
    vec2 q = abs(p) - (h - vec2(rc));
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - rc;
}

float erfApprox(float x) {
    float s = sign(x);
    float a = abs(x);
    float t = 1.0 / (1.0 + 0.3275911 * a);
    float y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * exp(-a * a);
    return s * y;
}

void main() {
    vec2 rel = v_pos - v_center;
    float r = v_rb.x;
    float bw = v_rb.y;
    float sigma = v_ks.y;
    float dist = v_ks.x > 0.5 ? squircleDist(rel, v_half, r) : roundDist(rel, v_half, r);
    if (sigma > 0.0) {
        float w = 0.5 * (1.0 - erfApprox(dist / (1.41421356 * sigma)));
        float a = v_fill.a * w;
        if (a <= 0.002) discard;
        gl_FragColor = vec4(v_fill.rgb * a, a);
        return;
    }
    float outer_cov = smoothstep(0.6, -0.6, dist);
    if (outer_cov <= 0.001) discard;
    if (bw > 0.0 && v_stroke.a > 0.0) {
        float border_mix = smoothstep(-bw - 0.5, -bw + 0.5, dist);
        float a_out = mix(v_fill.a, v_stroke.a, border_mix);
        vec3 rgb_out = mix(v_fill.rgb * v_fill.a, v_stroke.rgb * v_stroke.a, border_mix);
        gl_FragColor = vec4(rgb_out * outer_cov, a_out * outer_cov);
        return;
    }
    gl_FragColor = vec4(v_fill.rgb * v_fill.a * outer_cov, v_fill.a * outer_cov);
}
";

pub(crate) const TEXT_VERT_SRC: &str = "#version 100
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

pub(crate) const TEXT_FRAG_SRC: &str = "#version 100
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

pub(crate) const SPRITE_VERT_SRC: &str = "#version 100
attribute vec2 a_pos;
attribute vec2 a_uv;
uniform vec2 u_viewport;
varying vec2 v_uv;
void main() {
    vec2 ndc = (a_pos / u_viewport) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    v_uv = a_uv;
}
";

pub(crate) const SPRITE_FRAG_SRC: &str = "#version 100
precision mediump float;
uniform sampler2D u_tex;
varying vec2 v_uv;
void main() {
    vec4 px = texture2D(u_tex, v_uv);
    if (px.a <= 0.002) discard;
    gl_FragColor = px;
}
";

#[cfg(test)]
mod tests {
    use super::{Rect, clamp_damage_to_buffer, copy_bgra_region_into, swizzle_bgra_region_into};

    #[test]
    fn clamps_damage_rect_to_buffer_bounds() {
        assert_eq!(clamp_damage_to_buffer(None, 800, 600), (0, 0, 800, 600));
        assert_eq!(
            clamp_damage_to_buffer(Some(Rect::new(-10, 50, 100, 700)), 800, 600),
            (0, 50, 90, 550)
        );
        assert_eq!(
            clamp_damage_to_buffer(Some(Rect::new(900, 10, 50, 50)), 800, 600).2,
            0
        );
    }

    #[test]
    fn copies_and_swizzles_bgra_subregion() {
        // 2x2 BGRA buffer: (0,0)=[10,20,30,255], (1,0)=[1,2,3,255], (0,1)=[4,5,6,255], (1,1)=[7,8,9,255]
        let bgra = [
            10, 20, 30, 255, 1, 2, 3, 255, // row 0
            4, 5, 6, 255, 7, 8, 9, 255, // row 1
        ];
        let mut scratch = Vec::new();
        copy_bgra_region_into(&bgra, 2, 1, 0, 1, 2, &mut scratch);
        assert_eq!(scratch, [1, 2, 3, 255, 7, 8, 9, 255]);

        swizzle_bgra_region_into(&bgra, 2, 1, 0, 1, 2, &mut scratch);
        assert_eq!(scratch, [3, 2, 1, 255, 9, 8, 7, 255]);
    }
}
