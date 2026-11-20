//! CSD Window header state, hit testing, and double-click maximize handling.

use std::time::{Duration, Instant};

use crate::geometry::{Rect, TITLEBAR_HEIGHT};
use crate::theme::TrafficLightKind;
use crate::traffic_lights::TrafficLightButtons;

/// Hit-test target in the window chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowHitTarget {
    TrafficLight(TrafficLightKind),
    TitlebarDrag,
    Content,
}

/// Standard CSD header state and interaction manager.
#[derive(Debug, Clone)]
pub struct WindowFrame {
    pub title: String,
    pub header_height: u32,
    pub buttons: TrafficLightButtons,
    pub is_focused: bool,
    pub is_maximized: bool,
    pub traffic_hovered: bool,
    pub pressed_button: Option<TrafficLightKind>,
    last_title_click: Option<(Instant, f64, f64)>,
}

impl WindowFrame {
    /// Creates a new CSD window frame state with standard 40 px header height.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            header_height: TITLEBAR_HEIGHT,
            buttons: TrafficLightButtons::default_csd(),
            is_focused: true,
            is_maximized: false,
            traffic_hovered: false,
            pressed_button: None,
            last_title_click: None,
        }
    }

    /// Evaluates which chrome target lies at `(px, py)` for the given `window_width`.
    #[must_use]
    pub fn hit_test(&self, px: f64, py: f64, window_width: u32) -> WindowHitTarget {
        if py < 0.0
            || py >= f64::from(self.header_height)
            || px < 0.0
            || px >= f64::from(window_width)
        {
            return WindowHitTarget::Content;
        }

        if let Some(kind) = self.buttons.hit_test(px, py) {
            return WindowHitTarget::TrafficLight(kind);
        }

        WindowHitTarget::TitlebarDrag
    }

    /// Updates hover state on pointer motion. Returns `true` if a repaint is needed.
    pub fn on_pointer_motion(&mut self, px: f64, py: f64) -> bool {
        let hovered = self.buttons.is_hovered(px, py);
        if self.traffic_hovered != hovered {
            self.traffic_hovered = hovered;
            return true;
        }
        false
    }

    /// Clears hover state when pointer leaves window. Returns `true` if changed.
    pub fn on_pointer_leave(&mut self) -> bool {
        if self.traffic_hovered || self.pressed_button.is_some() {
            self.traffic_hovered = false;
            self.pressed_button = None;
            return true;
        }
        false
    }

    /// Handles pointer press at `(px, py)`. Returns the hit target.
    pub fn on_pointer_down(&mut self, px: f64, py: f64, window_width: u32) -> WindowHitTarget {
        let target = self.hit_test(px, py, window_width);
        if let WindowHitTarget::TrafficLight(kind) = target {
            self.pressed_button = Some(kind);
        }
        target
    }

    /// Handles pointer release at `(px, py)`. Returns `Some(kind)` if released on the same pressed button.
    pub fn on_pointer_up(&mut self, px: f64, py: f64) -> Option<TrafficLightKind> {
        let pressed = self.pressed_button.take();
        if let Some(kind) = pressed {
            if self.buttons.hit_test(px, py) == Some(kind) {
                return Some(kind);
            }
        }
        None
    }

    /// Detects standard double click on the titlebar drag area to toggle maximize.
    pub fn check_double_click_maximize(&mut self, px: f64, py: f64, window_width: u32) -> bool {
        if self.hit_test(px, py, window_width) != WindowHitTarget::TitlebarDrag {
            return false;
        }

        let now = Instant::now();
        let max_interval = Duration::from_millis(400);
        let max_dist = 6.0;

        if let Some((prev_time, prev_x, prev_y)) = self.last_title_click {
            if now.duration_since(prev_time) <= max_interval
                && (px - prev_x).hypot(py - prev_y) <= max_dist
            {
                self.last_title_click = None;
                self.is_maximized = !self.is_maximized;
                return true;
            }
        }

        self.last_title_click = Some((now, px, py));
        false
    }

    /// Bounds of the header bar.
    #[must_use]
    pub const fn header_rect(&self, window_width: u32) -> Rect {
        Rect::new(0, 0, window_width, self.header_height)
    }
}
