//! Central application state, user action dispatch, and frame rendering for `rusty-view`.

use std::path::Path;

use rusty_ui::{
    Rect, ThemeMode, ThemePalette, TrafficLightKind, UiFontEngine, UiPainter, WindowFrame,
    WindowHitTarget,
};

use crate::document::ImageDocument;
use crate::hud::InfoHud;
use crate::playlist::Playlist;
use crate::toolbar::{Toolbar, ToolbarAction};
use crate::viewport::Viewport;

pub(crate) const DEFAULT_VIEWER_WIDTH: u32 = 900;
pub(crate) const DEFAULT_VIEWER_HEIGHT: u32 = 650;

/// State for the `rusty-view` application.
#[derive(Debug)]
pub(crate) struct ViewerState {
    pub(crate) document: Option<ImageDocument>,
    pub(crate) playlist: Playlist,
    pub(crate) viewport: Viewport,
    pub(crate) toolbar: Toolbar,
    pub(crate) hud: InfoHud,
    pub(crate) frame: WindowFrame,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) is_fullscreen: bool,
    pub(crate) dirty: bool,
}

impl ViewerState {
    /// Creates a new `ViewerState` with the optional initial path and window dimensions.
    #[must_use]
    pub(crate) fn new(
        initial_path: Option<&Path>,
        win_w: u32,
        win_h: u32,
        theme_mode: ThemeMode,
    ) -> Self {
        let mut frame = WindowFrame::new("Image Viewer");
        frame.buttons = rusty_ui::TrafficLightButtons::default_csd_for_width(win_w);
        let mut state = Self {
            document: None,
            playlist: Playlist::default(),
            viewport: Viewport::default(),
            toolbar: Toolbar::default(),
            hud: InfoHud::default(),
            frame,
            theme_mode,
            is_fullscreen: false,
            dirty: true,
        };

        if let Some(path) = initial_path {
            state.open_path(path, win_w, win_h);
        }

        state
    }

    /// Opens an image file and fits it in the window.
    pub(crate) fn open_path(&mut self, path: &Path, win_w: u32, win_h: u32) {
        self.playlist = Playlist::for_path(path);
        if let Some(doc) = ImageDocument::load_from_path(path) {
            self.frame.title = format!("{} — Image Viewer", doc.filename);
            self.viewport
                .fit_to_window(win_w, win_h, doc.width, doc.height);
            self.document = Some(doc);
        } else {
            "Image Viewer".clone_into(&mut self.frame.title);
            self.document = None;
        }
        self.dirty = true;
    }

    /// Switches to the previous image in the current directory.
    pub(crate) fn action_prev(&mut self, win_w: u32, win_h: u32) {
        if let Some(prev_path) = self.playlist.prev().cloned()
            && let Some(doc) = ImageDocument::load_from_path(&prev_path)
        {
            self.frame.title = format!("{} — Image Viewer", doc.filename);
            self.viewport
                .fit_to_window(win_w, win_h, doc.width, doc.height);
            self.document = Some(doc);
            self.dirty = true;
        }
    }

    /// Switches to the next image in the current directory.
    pub(crate) fn action_next(&mut self, win_w: u32, win_h: u32) {
        if let Some(next_path) = self.playlist.next().cloned()
            && let Some(doc) = ImageDocument::load_from_path(&next_path)
        {
            self.frame.title = format!("{} — Image Viewer", doc.filename);
            self.viewport
                .fit_to_window(win_w, win_h, doc.width, doc.height);
            self.document = Some(doc);
            self.dirty = true;
        }
    }

    /// Zooms in on the image.
    pub(crate) fn action_zoom_in(&mut self, win_w: u32, win_h: u32) {
        if let Some(doc) = &self.document {
            self.viewport.zoom_in(win_w, win_h, doc.width, doc.height);
            self.dirty = true;
        }
    }

    /// Zooms out from the image.
    pub(crate) fn action_zoom_out(&mut self, win_w: u32, win_h: u32) {
        if let Some(doc) = &self.document {
            self.viewport.zoom_out(win_w, win_h, doc.width, doc.height);
            self.dirty = true;
        }
    }

    /// Zooms to 100% 1:1 scale.
    pub(crate) fn action_actual_size(&mut self) {
        self.viewport.actual_size();
        self.dirty = true;
    }

    /// Fits the image to the current window bounds.
    pub(crate) fn action_fit(&mut self, win_w: u32, win_h: u32) {
        if let Some(doc) = &self.document {
            self.viewport
                .fit_to_window(win_w, win_h, doc.width, doc.height);
            self.dirty = true;
        }
    }

    /// Rotates the image 90 degrees clockwise.
    pub(crate) fn action_rotate_cw(&mut self, win_w: u32, win_h: u32) {
        if let Some(doc) = self.document.as_mut() {
            doc.rotate_cw();
            if self.viewport.is_fitting {
                self.viewport
                    .fit_to_window(win_w, win_h, doc.width, doc.height);
            }
            self.dirty = true;
        }
    }

    /// Rotates the image 90 degrees counter-clockwise.
    pub(crate) fn action_rotate_ccw(&mut self, win_w: u32, win_h: u32) {
        if let Some(doc) = self.document.as_mut() {
            doc.rotate_ccw();
            if self.viewport.is_fitting {
                self.viewport
                    .fit_to_window(win_w, win_h, doc.width, doc.height);
            }
            self.dirty = true;
        }
    }

    /// Flips the image horizontally.
    pub(crate) fn action_flip_h(&mut self) {
        if let Some(doc) = self.document.as_mut() {
            doc.flip_horizontal();
            self.dirty = true;
        }
    }

    /// Toggles the info overlay card.
    pub(crate) fn action_toggle_info(&mut self) {
        self.hud.toggle();
        self.dirty = true;
    }

    /// Handles pointer movement.
    pub(crate) fn on_pointer_moved(&mut self, px: f64, py: f64, win_w: u32, win_h: u32) {
        if self.toolbar.on_pointer_moved(px, py, win_w, win_h) {
            self.dirty = true;
        }
        if self.viewport.drag_start.is_some() {
            self.viewport.drag_to(px, py);
            self.dirty = true;
        } else {
            // Check traffic light hover
            let target = self.frame.hit_test(px, py, win_w);
            let traffic_hovered = matches!(target, WindowHitTarget::TrafficLight(_));
            if self.frame.traffic_hovered != traffic_hovered {
                self.frame.traffic_hovered = traffic_hovered;
                self.dirty = true;
            }
        }
    }

    /// Handles pointer click down. Returns the hit target for dragging/traffic lights.
    pub(crate) fn on_pointer_down(
        &mut self,
        px: f64,
        py: f64,
        win_w: u32,
        win_h: u32,
    ) -> WindowHitTarget {
        // 1. Check toolbar press first (action fires once on release in `on_pointer_up`)
        if self.toolbar.on_pointer_down(px, py, win_w, win_h).is_some() {
            self.dirty = true;
            return WindowHitTarget::Content;
        }

        // 2. Check window frame / traffic lights
        let target = self.frame.on_pointer_down(px, py, win_w);
        if target != WindowHitTarget::Content {
            self.dirty = true;
            return target;
        }

        // 3. Otherwise start panning canvas
        self.viewport.start_drag(px, py);
        WindowHitTarget::Content
    }

    /// Handles pointer click release.
    pub(crate) fn on_pointer_up(
        &mut self,
        px: f64,
        py: f64,
        win_w: u32,
        win_h: u32,
    ) -> Option<TrafficLightKind> {
        self.viewport.end_drag();
        if let Some(action) = self.toolbar.on_pointer_up(px, py, win_w, win_h) {
            self.dirty = true;
            match action {
                ToolbarAction::Prev => self.action_prev(win_w, win_h),
                ToolbarAction::Next => self.action_next(win_w, win_h),
                ToolbarAction::ZoomOut => self.action_zoom_out(win_w, win_h),
                ToolbarAction::ZoomReset => self.action_actual_size(),
                ToolbarAction::ZoomIn => self.action_zoom_in(win_w, win_h),
                ToolbarAction::FitToWindow => self.action_fit(win_w, win_h),
                ToolbarAction::RotateCw => self.action_rotate_cw(win_w, win_h),
                ToolbarAction::ToggleInfo => self.action_toggle_info(),
            }
        }
        let traffic = self.frame.on_pointer_up(px, py);
        self.dirty = true;
        traffic
    }

    /// Handles pointer mouse wheel for zooming.
    pub(crate) fn on_wheel(
        &mut self,
        delta_y: f64,
        cursor_x: f64,
        cursor_y: f64,
        win_w: u32,
        win_h: u32,
    ) {
        let Some(doc) = &self.document else {
            return;
        };
        let factor = if delta_y < 0.0 { 1.15 } else { 1.0 / 1.15 };
        self.viewport.zoom_at(
            factor, cursor_x, cursor_y, win_w, win_h, doc.width, doc.height,
        );
        self.toolbar.last_motion = std::time::Instant::now();
        self.toolbar.opacity = 1.0;
        self.dirty = true;
    }

    /// Renders the entire window: canvas background, centered image, traffic lights, and floating toolbar.
    pub(crate) fn paint(
        &mut self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        win_w: u32,
        win_h: u32,
    ) {
        let pal = ThemePalette::new(self.theme_mode == ThemeMode::Dark, 0);

        // 1. Dark / Light translucent CSD backdrop (opaque in fullscreen, Liquid Glass when windowed)
        let bg_alpha = if self.is_fullscreen { 255 } else { 215 };
        let bg_color = if pal.is_dark {
            [18, 19, 23, bg_alpha]
        } else {
            [240, 242, 247, bg_alpha]
        };
        let window_radius = if self.is_fullscreen || self.frame.is_maximized {
            0.0
        } else {
            rusty_ui::WINDOW_RADIUS as f32
        };
        let win_rect = Rect::new(0, 0, win_w, win_h);
        if window_radius > 0.0 {
            painter.fill_rounded_rect(win_rect, window_radius, bg_color);
        } else {
            painter.fill_rect(win_rect, bg_color);
        }

        // 2. Render Image (inset slightly from border when windowed)
        if let Some(doc) = &self.document {
            let dst_rect = self
                .viewport
                .compute_dest_rect(win_w, win_h, doc.width, doc.height);
            painter.draw_image(
                dst_rect,
                doc.cache_key,
                doc.width,
                doc.height,
                doc.pixels.clone(),
                doc.format,
            );
        } else {
            // Empty state placeholder
            let msg = "No image loaded — Open an image to view";
            let color = if pal.is_dark {
                [120, 125, 138, 255]
            } else {
                [140, 145, 158, 255]
            };
            painter.draw_text_centered(fonts, msg, win_rect, 14.0, false, color);
        }

        // 3. Subtle CSD header title & traffic lights (unless in fullscreen mode)
        if !self.is_fullscreen {
            let title_fg = if pal.is_dark {
                [220, 224, 234, 210]
            } else {
                [42, 46, 56, 210]
            };
            let title_rect = Rect::new(88, 0, win_w.saturating_sub(176), 40);
            painter.draw_text_centered(fonts, &self.frame.title, title_rect, 12.5, true, title_fg);

            painter.draw_traffic_lights(
                &self.frame.buttons,
                self.frame.is_focused,
                self.frame.traffic_hovered,
                self.frame.pressed_button,
            );
        }

        // 4. Floating Liquid Glass Toolbar
        let zoom_str = self.viewport.formatted_zoom();
        self.toolbar
            .paint(painter, fonts, &pal, win_w, win_h, &zoom_str);

        // 5. Information HUD
        if let Some(doc) = &self.document {
            self.hud
                .paint(painter, &pal, win_w, doc, &self.playlist, &self.viewport);
        }

        // 6. Crisp 1px CSD glass border
        if window_radius > 0.0 {
            let border = if pal.is_dark {
                [255, 255, 255, 28]
            } else {
                [0, 0, 0, 24]
            };
            painter.stroke_rounded_rect(win_rect, window_radius, 1.0, border);
        }
    }
}
