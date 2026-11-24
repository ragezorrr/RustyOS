//! Canvas viewport coordinate projection, zoom, and pan dragging for `rusty-view`.

use rusty_ui::Rect;

pub(crate) const MIN_SCALE: f32 = 0.05;
pub(crate) const MAX_SCALE: f32 = 25.0;
pub(crate) const ZOOM_STEP: f32 = 1.25;

/// Viewport managing zoom scale, pan translation offset, and drag states.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Viewport {
    pub(crate) scale: f32,
    pub(crate) pan_x: f32,
    pub(crate) pan_y: f32,
    pub(crate) is_fitting: bool,
    pub(crate) drag_start: Option<(f64, f64, f32, f32)>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            scale: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            is_fitting: true,
            drag_start: None,
        }
    }
}

impl Viewport {
    /// Centers and fits the image within the window dimensions while preserving aspect ratio.
    pub(crate) fn fit_to_window(&mut self, win_w: u32, win_h: u32, img_w: u32, img_h: u32) {
        if img_w == 0 || img_h == 0 || win_w == 0 || win_h == 0 {
            self.scale = 1.0;
            self.pan_x = 0.0;
            self.pan_y = 0.0;
            self.is_fitting = true;
            return;
        }

        let padding_x = 24.0;
        let padding_y = 48.0;

        let avail_w = (win_w as f32 - padding_x).max(10.0);
        let avail_h = (win_h as f32 - padding_y).max(10.0);

        let scale_x = avail_w / (img_w as f32);
        let scale_y = avail_h / (img_h as f32);

        let fit_scale = scale_x.min(scale_y).min(1.0);

        self.scale = fit_scale.clamp(MIN_SCALE, MAX_SCALE);
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        self.is_fitting = true;
    }

    /// Resets scale to 1:1 (100%) and centers the image.
    pub(crate) fn actual_size(&mut self) {
        self.scale = 1.0;
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        self.is_fitting = false;
    }

    /// Zooms in by the standard step factor around the center of the window.
    pub(crate) fn zoom_in(&mut self, win_w: u32, win_h: u32, img_w: u32, img_h: u32) {
        let cx = f64::from(win_w) * 0.5;
        let cy = f64::from(win_h) * 0.5;
        self.zoom_at(ZOOM_STEP, cx, cy, win_w, win_h, img_w, img_h);
    }

    /// Zooms out by the standard step factor around the center of the window.
    pub(crate) fn zoom_out(&mut self, win_w: u32, win_h: u32, img_w: u32, img_h: u32) {
        let cx = f64::from(win_w) * 0.5;
        let cy = f64::from(win_h) * 0.5;
        self.zoom_at(1.0 / ZOOM_STEP, cx, cy, win_w, win_h, img_w, img_h);
    }

    /// Zooms by `factor` keeping the image point under `(cursor_x, cursor_y)` stationary.
    #[allow(clippy::too_many_arguments, clippy::similar_names)]
    pub(crate) fn zoom_at(
        &mut self,
        factor: f32,
        cursor_x: f64,
        cursor_y: f64,
        win_w: u32,
        win_h: u32,
        img_w: u32,
        img_h: u32,
    ) {
        let old_scale = self.scale;
        let new_scale = (old_scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        if (new_scale - old_scale).abs() < f32::EPSILON {
            return;
        }

        let cur_rect = self.compute_dest_rect(win_w, win_h, img_w, img_h);

        let rel_x = (cursor_x as f32 - cur_rect.x as f32) / old_scale;
        let rel_y = (cursor_y as f32 - cur_rect.y as f32) / old_scale;

        self.scale = new_scale;
        self.is_fitting = false;

        let new_left = cursor_x as f32 - rel_x * new_scale;
        let new_top = cursor_y as f32 - rel_y * new_scale;

        let new_cw = (img_w as f32) * new_scale;
        let new_ch = (img_h as f32) * new_scale;
        let center_x = (win_w as f32) * 0.5;
        let center_y = (win_h as f32) * 0.5;

        self.pan_x = (new_left + new_cw * 0.5) - center_x;
        self.pan_y = (new_top + new_ch * 0.5) - center_y;
    }

    /// Begins a pointer drag operation for panning.
    pub(crate) fn start_drag(&mut self, px: f64, py: f64) {
        self.drag_start = Some((px, py, self.pan_x, self.pan_y));
        self.is_fitting = false;
    }

    /// Moves the canvas during an active drag operation.
    pub(crate) fn drag_to(&mut self, px: f64, py: f64) {
        if let Some((start_px, start_py, start_pan_x, start_pan_y)) = self.drag_start {
            self.pan_x = start_pan_x + (px - start_px) as f32;
            self.pan_y = start_pan_y + (py - start_py) as f32;
        }
    }

    /// Ends an active drag operation.
    pub(crate) fn end_drag(&mut self) {
        self.drag_start = None;
    }

    /// Computes the bounding rectangle of the scaled image on window coordinates.
    #[must_use]
    pub(crate) fn compute_dest_rect(&self, win_w: u32, win_h: u32, img_w: u32, img_h: u32) -> Rect {
        let draw_w = ((img_w as f32) * self.scale).round() as i32;
        let draw_h = ((img_h as f32) * self.scale).round() as i32;

        let center_x = (win_w as i32) / 2 + (self.pan_x.round() as i32);
        let center_y = (win_h as i32) / 2 + (self.pan_y.round() as i32);

        let left = center_x - draw_w / 2;
        let top = center_y - draw_h / 2;

        Rect::new(left, top, draw_w.max(1) as u32, draw_h.max(1) as u32)
    }

    /// Formats the current zoom percentage as a human-readable string.
    #[must_use]
    pub(crate) fn formatted_zoom(&self) -> String {
        let pct = (self.scale * 100.0).round() as u32;
        format!("{pct}%")
    }
}
