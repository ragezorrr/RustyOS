//! Standalone movable `Get Info` inspector window for `rusty-files`.

use std::path::Path;

use rusty_app::{
    AppConfig, AppContext, AppError, ConfigureInfo, CursorShape, ResizeEdge, RustyApp,
    WindowDecorationMode, run_app,
};
use rusty_services::file_manager::UserFileLocations;
use smithay_client_toolkit::seat::{
    keyboard::{KeyEvent, Keysym},
    pointer::{BTN_LEFT, PointerEvent, PointerEventKind},
};

use crate::layout::{GetInfoLayout, Rect, compute_get_info_layout_in_rect};
use crate::liquid_glass::LiquidGlassConfig;
use crate::render::render_get_info_window;
use crate::state::{FilesAppState, StartupOptions};

const INFO_WINDOW_WIDTH: u32 = 320;
const INFO_WINDOW_HEIGHT: u32 = 460;
const INFO_MIN_WIDTH: u32 = 280;
const INFO_MIN_HEIGHT: u32 = 380;
const RESIZE_BORDER_PX: f64 = 6.0;

struct GetInfoWindowApp {
    app: FilesAppState,
    width: u32,
    height: u32,
    last_input_serial: u32,
}

impl GetInfoWindowApp {
    fn layout(&self) -> GetInfoLayout {
        let wi = i32::try_from(self.width.max(1)).unwrap_or(1).max(1);
        let hi = i32::try_from(self.height.max(1)).unwrap_or(1).max(1);
        compute_get_info_layout_in_rect(Rect::new(0, 0, wi, hi))
    }
}

fn resize_edge_at(px: f64, py: f64, width: u32, height: u32) -> Option<ResizeEdge> {
    let w = f64::from(width);
    let h = f64::from(height);
    let left = px <= RESIZE_BORDER_PX;
    let right = px >= w - RESIZE_BORDER_PX;
    let top = py <= RESIZE_BORDER_PX;
    let bottom = py >= h - RESIZE_BORDER_PX;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeEdge::TopLeft),
        (_, true, true, _) => Some(ResizeEdge::TopRight),
        (true, _, _, true) => Some(ResizeEdge::BottomLeft),
        (_, true, _, true) => Some(ResizeEdge::BottomRight),
        (true, _, _, _) => Some(ResizeEdge::Left),
        (_, true, _, _) => Some(ResizeEdge::Right),
        (_, _, true, _) => Some(ResizeEdge::Top),
        (_, _, _, true) => Some(ResizeEdge::Bottom),
        _ => None,
    }
}

const fn cursor_for_edge(edge: ResizeEdge) -> CursorShape {
    match edge {
        ResizeEdge::Left | ResizeEdge::Right => CursorShape::EwResize,
        ResizeEdge::Top | ResizeEdge::Bottom => CursorShape::NsResize,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorShape::NwseResize,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorShape::NeswResize,
        _ => CursorShape::Default,
    }
}

impl RustyApp for GetInfoWindowApp {
    fn on_configure(&mut self, _ctx: &mut AppContext<'_>, info: ConfigureInfo) {
        self.width = info.width.max(INFO_MIN_WIDTH);
        self.height = info.height.max(INFO_MIN_HEIGHT);
        self.app.dirty = true;
    }

    fn on_tick(&mut self, ctx: &mut AppContext<'_>) {
        if self.app.poll_background_loads() || self.app.dirty {
            self.app.dirty = false;
            ctx.request_redraw();
        }
    }

    fn on_key(&mut self, ctx: &mut AppContext<'_>, event: KeyEvent) {
        let mods = ctx.modifiers();
        let close = event.keysym == Keysym::Escape
            || (mods.ctrl && matches!(event.keysym, Keysym::w | Keysym::W | Keysym::i | Keysym::I));
        if close {
            ctx.request_exit();
        }
    }

    fn on_pointer(&mut self, ctx: &mut AppContext<'_>, events: &[PointerEvent]) {
        let gi = self.layout();
        for ev in events {
            let (px, py) = ev.position;
            match ev.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let over_btn = gi.close_rect.contains_f64(px, py)
                        || gi.more_toggle_rect.contains_f64(px, py)
                        || gi.tags.iter().any(|(_, r)| r.contains_f64(px, py));
                    if over_btn {
                        ctx.set_cursor_shape(CursorShape::Pointer);
                    } else if let Some(edge) = resize_edge_at(px, py, self.width, self.height) {
                        ctx.set_cursor_shape(cursor_for_edge(edge));
                    } else {
                        ctx.set_cursor_shape(CursorShape::Default);
                    }
                }
                PointerEventKind::Press {
                    serial,
                    button: BTN_LEFT,
                    ..
                } => {
                    self.last_input_serial = serial;
                    if gi.close_rect.contains_f64(px, py) {
                        ctx.request_exit();
                        return;
                    }
                    if gi.more_toggle_rect.contains_f64(px, py) {
                        if let Some(gs) = &mut self.app.get_info {
                            gs.more_expanded = !gs.more_expanded;
                            ctx.request_redraw();
                        }
                        return;
                    }
                    if let Some((color, _)) = gi.tags.iter().find(|(_, r)| r.contains_f64(px, py)) {
                        if let Some(path) =
                            self.app.get_info.as_ref().map(|g| g.target_path.clone())
                        {
                            self.app.toggle_tag_on_path(&path, *color);
                            ctx.request_redraw();
                        }
                        return;
                    }
                    if let Some(edge) = resize_edge_at(px, py, self.width, self.height) {
                        ctx.start_interactive_resize(edge, serial);
                        return;
                    }
                    ctx.start_interactive_move(serial);
                }
                _ => {}
            }
        }
    }

    fn uses_bgra(&self) -> bool {
        true
    }

    fn paint(
        &mut self,
        _painter: &mut rusty_ui::UiPainter<'_>,
        _fonts: &mut rusty_ui::UiFontEngine,
        _width: u32,
        _height: u32,
    ) {
    }

    fn paint_bgra(
        &mut self,
        canvas_bgra: &mut [u8],
        width: u32,
        height: u32,
        has_glass: bool,
    ) -> bool {
        self.width = width;
        self.height = height;
        let gi = self.layout();
        render_get_info_window(canvas_bgra, width, height, &self.app, &gi, has_glass);
        true
    }
}

/// Launches a standalone movable `Get Info` window for `target_path`.
pub(crate) fn run(target_path: &Path, options: &StartupOptions) -> Result<bool, AppError> {
    let locs = UserFileLocations::detect();
    let mut app = FilesAppState::new(locs, options);
    app.open_get_info_sheet_for_path(target_path);

    let name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("File");
    let mut cfg = AppConfig::new(
        format!("{name} Info"),
        "org.rustyos.files.info",
        (INFO_WINDOW_WIDTH, INFO_WINDOW_HEIGHT),
    )
    .with_min_size(INFO_MIN_WIDTH, INFO_MIN_HEIGHT)
    .with_decorations(WindowDecorationMode::Client)
    .with_no_gpu(options.no_gpu);
    if !app.reduce_transparency {
        cfg = cfg.with_liquid_glass(LiquidGlassConfig::popover(12));
    }

    run_app(
        cfg,
        GetInfoWindowApp {
            app,
            width: INFO_WINDOW_WIDTH,
            height: INFO_WINDOW_HEIGHT,
            last_input_serial: 0,
        },
    )?;
    Ok(true)
}
