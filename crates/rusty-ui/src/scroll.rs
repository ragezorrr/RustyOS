//! Reusable scrollable viewport, scrollbar thumb drag, and smooth scroll state (`RUSTYOS-130`).

use std::time::Duration;

use crate::draw::UiPainter;
use crate::geometry::Rect;
use crate::motion::ScrollMotion;
use crate::theme::ThemePalette;

/// Scrollbar layout geometry and smooth offset controller backed by [`ScrollMotion`].
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollView {
    pub viewport: Rect,
    pub content_height: i32,
    pub motion: ScrollMotion,
    dragging: Option<(f64, f64)>,
    pub hovered_thumb: bool,
}

impl Default for ScrollView {
    fn default() -> Self {
        Self::new(Rect::new(0, 0, 1, 1))
    }
}

impl ScrollView {
    pub const TRACK_WIDTH: i32 = 8;
    pub const THUMB_WIDTH: i32 = 5;
    pub const MIN_THUMB_H: i32 = 28;
    pub const MARGIN_RIGHT: i32 = 4;
    pub const MARGIN_Y: i32 = 6;

    #[must_use]
    pub const fn new(viewport: Rect) -> Self {
        Self {
            viewport,
            content_height: 0,
            motion: ScrollMotion::new(),
            dragging: None,
            hovered_thumb: false,
        }
    }

    pub fn set_bounds(&mut self, viewport: Rect, content_height: i32) {
        self.viewport = viewport;
        self.content_height = content_height.max(0);
        let max_off = (self.content_height - self.viewport.height as i32).max(0) as f64;
        self.motion.set_max_offset(max_off);
    }

    #[must_use]
    pub fn max_scroll(&self) -> f64 {
        self.motion.max_offset()
    }

    #[must_use]
    pub const fn offset_y(&self) -> i32 {
        self.motion.current() as i32
    }

    pub fn scroll_by(&mut self, delta_y: f64, now: Duration, reduced_motion: bool) {
        self.motion.scroll_by(delta_y, now, reduced_motion);
    }

    pub fn scroll_to(&mut self, offset_y: f64, now: Duration, reduced_motion: bool) {
        self.motion.scroll_to(offset_y, now, reduced_motion);
    }

    /// Scrolls minimally so `[item_top_rel..item_bottom_rel]` in content coordinates is visible.
    pub fn ensure_visible(
        &mut self,
        item_top_rel: i32,
        item_bottom_rel: i32,
        now: Duration,
        reduced_motion: bool,
    ) {
        let cur = self.motion.target();
        let vh = self.viewport.height as f64;
        if (item_top_rel as f64) < cur {
            self.motion
                .scroll_to(f64::from(item_top_rel), now, reduced_motion);
        } else if (item_bottom_rel as f64) > cur + vh {
            self.motion
                .scroll_to(f64::from(item_bottom_rel) - vh, now, reduced_motion);
        }
    }

    pub fn tick(&mut self, now: Duration) -> bool {
        self.motion.tick(now)
    }

    /// Computes vertical scrollbar thumb rectangle when `content_height > viewport.height`.
    #[must_use]
    pub fn thumb_rect(&self) -> Option<Rect> {
        let max_s = self.max_scroll();
        if max_s <= 0.5 || self.viewport.height < 40 {
            return None;
        }
        let track_h = (self.viewport.height as i32 - Self::MARGIN_Y * 2).max(Self::MIN_THUMB_H);
        let ratio = (self.viewport.height as f64 / self.content_height as f64).clamp(0.08, 1.0);
        let thumb_h = ((track_h as f64 * ratio).round() as i32).clamp(Self::MIN_THUMB_H, track_h);
        let travel = (track_h - thumb_h).max(0);
        let frac = (self.motion.current() / max_s).clamp(0.0, 1.0);
        let thumb_y = self.viewport.y + Self::MARGIN_Y + (travel as f64 * frac).round() as i32;
        let thumb_x = self.viewport.right() - Self::MARGIN_RIGHT - Self::THUMB_WIDTH;
        Some(Rect::new(
            thumb_x,
            thumb_y,
            Self::THUMB_WIDTH as u32,
            thumb_h as u32,
        ))
    }

    /// Handles pointer press on thumb (starts drag) or scrollbar track (jumps to position).
    pub fn on_pointer_down(
        &mut self,
        px: f64,
        py: f64,
        now: Duration,
        reduced_motion: bool,
    ) -> bool {
        let Some(thumb) = self.thumb_rect() else {
            return false;
        };
        let hit_thumb = Rect::new(thumb.x - 4, thumb.y, thumb.width + 8, thumb.height);
        if hit_thumb.contains_f64(px, py) {
            self.dragging = Some((py, self.motion.current()));
            return true;
        }
        let track = Rect::new(
            thumb.x - 4,
            self.viewport.y + Self::MARGIN_Y,
            (Self::TRACK_WIDTH + 4) as u32,
            self.viewport
                .height
                .saturating_sub((Self::MARGIN_Y * 2) as u32),
        );
        if track.contains_f64(px, py) {
            let rel = ((py - f64::from(track.y) - f64::from(thumb.height) * 0.5)
                / f64::from((track.height as i32 - thumb.height as i32).max(1)))
            .clamp(0.0, 1.0);
            self.motion
                .scroll_to(rel * self.max_scroll(), now, reduced_motion);
            return true;
        }
        false
    }

    pub fn on_pointer_move(&mut self, px: f64, py: f64) -> bool {
        if let Some((start_py, start_off)) = self.dragging {
            let Some(thumb) = self.thumb_rect() else {
                return false;
            };
            let track_h =
                (self.viewport.height as i32 - Self::MARGIN_Y * 2 - thumb.height as i32).max(1);
            let delta_py = py - start_py;
            let delta_off = (delta_py / f64::from(track_h)) * self.max_scroll();
            self.motion.snap_to(start_off + delta_off);
            return true;
        }
        let prev = self.hovered_thumb;
        self.hovered_thumb = self
            .thumb_rect()
            .is_some_and(|r| Rect::new(r.x - 4, r.y, r.width + 8, r.height).contains_f64(px, py));
        prev != self.hovered_thumb
    }

    pub fn on_pointer_up(&mut self) -> bool {
        self.dragging.take().is_some()
    }

    /// Pushes content viewport clipping onto `painter`.
    pub fn push_clip(&self, painter: &mut UiPainter<'_>) {
        painter.push_clip(self.viewport);
    }

    /// Draws the thin rounded scrollbar thumb when scrollable.
    pub fn paint_scrollbar(&self, painter: &mut UiPainter<'_>, pal: &ThemePalette) {
        let Some(thumb) = self.thumb_rect() else {
            return;
        };
        let active = self.dragging.is_some() || self.hovered_thumb;
        let col = if pal.is_dark {
            [255, 255, 255, if active { 118 } else { 64 }]
        } else {
            [0, 0, 0, if active { 96 } else { 52 }]
        };
        painter.fill_rounded_rect(thumb, thumb.width as f32 * 0.5, col);
    }
}
