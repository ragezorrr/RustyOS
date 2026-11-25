//! Window header, titlebar metrics, and window framing styles.

use crate::theme::TrafficLightKind;
pub use crate::window_frame::{WindowFrame, WindowHitTarget};

/// Standard client-side decoration header height (40 px).
pub const CSD_TITLEBAR_HEIGHT: u32 = 40;

/// Standard server-side decoration header height (`36 px`, matching [`crate::geometry::SSD_TITLEBAR_HEIGHT`]).
pub const SSD_TITLEBAR_HEIGHT: u32 = crate::geometry::SSD_TITLEBAR_HEIGHT;

/// Standard modal dialog header height (42 px = 2 * 10 px inset + 22 px button).
pub const DIALOG_HEADER_HEIGHT: u32 = 42;

/// Standard window corner radius (14 px).
pub const WINDOW_RADIUS: u32 = 14;

/// Small component corner radius (8 px).
pub const SMALL_RADIUS: u32 = 8;

/// Medium component corner radius (10 px).
pub const MEDIUM_RADIUS: u32 = 10;

/// Large component corner radius (12 px).
pub const LARGE_RADIUS: u32 = 12;

/// Standard window translucency with blur for active focused windows (85%).
pub const ACTIVE_WINDOW_BLUR_OPACITY: f32 = 0.85;

/// Standard window translucency with blur for inactive unfocused windows (92%).
pub const INACTIVE_WINDOW_BLUR_OPACITY: f32 = 0.92;

/// Double-click timeout in milliseconds (400 ms).
pub const DOUBLE_CLICK_TIMEOUT_MS: u64 = 400;

/// Maximum pointer travel for double-click detection (4 px).
pub const DOUBLE_CLICK_DISTANCE: f64 = 4.0;

/// Minimum drag distance to initiate interactive window move (8 px).
pub const DRAG_START_DISTANCE: f64 = 8.0;

/// Configuration options for a window header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowHeaderConfig {
    pub height: u32,
    pub button_margin: f32,
    pub buttons_centered: bool,
    pub content_under_buttons: bool,
    pub close_only: bool,
}

impl Default for WindowHeaderConfig {
    fn default() -> Self {
        Self {
            height: CSD_TITLEBAR_HEIGHT,
            button_margin: 4.0,
            buttons_centered: false,
            content_under_buttons: false,
            close_only: false,
        }
    }
}

/// Advanced Window Header state manager.
#[derive(Debug, Clone)]
pub struct WindowHeader {
    pub config: WindowHeaderConfig,
    pub frame: WindowFrame,
}

impl WindowHeader {
    /// Creates a standard CSD window header with 40 px height.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self::with_config(title, WindowHeaderConfig::default())
    }

    /// Creates a window header with custom configuration.
    #[must_use]
    pub fn with_config(title: impl Into<String>, config: WindowHeaderConfig) -> Self {
        let mut frame = WindowFrame::new(title);
        frame.header_height = config.height;
        Self { config, frame }
    }

    /// Inset reserved for window control buttons (trailing inset on right, or leading inset if left-aligned).
    #[must_use]
    pub fn button_inset(&self) -> f32 {
        if self.config.close_only {
            self.config.button_margin + 22.0 + 8.0
        } else {
            74.0
        }
    }

    /// Leading inset (legacy compatibility).
    #[must_use]
    pub fn leading_inset(&self) -> f32 {
        self.button_inset()
    }

    /// Checks if a pointer press hits buttons or titlebar drag.
    #[must_use]
    pub fn on_pointer_down(&mut self, px: f64, py: f64, window_width: u32) -> WindowHitTarget {
        self.frame.on_pointer_down(px, py, window_width)
    }

    /// Handles pointer motion. Returns `true` if hover state changed.
    pub fn on_pointer_motion(&mut self, px: f64, py: f64) -> bool {
        self.frame.on_pointer_motion(px, py)
    }

    /// Handles pointer leave.
    pub fn on_pointer_leave(&mut self) -> bool {
        self.frame.on_pointer_leave()
    }

    /// Handles pointer release.
    pub fn on_pointer_up(&mut self, px: f64, py: f64) -> Option<TrafficLightKind> {
        self.frame.on_pointer_up(px, py)
    }

    /// Checks double click on titlebar for toggle maximize.
    pub fn check_double_click_maximize(&mut self, px: f64, py: f64, window_width: u32) -> bool {
        self.frame.check_double_click_maximize(px, py, window_width)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ssd_titlebar_height_matches_geometry_constant() {
        assert_eq!(
            super::SSD_TITLEBAR_HEIGHT,
            crate::geometry::SSD_TITLEBAR_HEIGHT
        );
        assert_eq!(super::SSD_TITLEBAR_HEIGHT, 36);
    }
}
