//! Standard window-control traffic light layout and vector microglyph geometry.

use crate::geometry::{
    Rect, TRAFFIC_LIGHT_CENTERS_X, TRAFFIC_LIGHT_DIAMETER, TRAFFIC_LIGHT_GAP,
    TRAFFIC_LIGHT_MARGIN_LEFT,
};
use crate::theme::TrafficLightKind;

/// Geometry and hit-testing for the 3 window controls (Close, Minimize, Maximize).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrafficLightButtons {
    pub close_rect: Rect,
    pub minimize_rect: Rect,
    pub maximize_rect: Rect,
    pub hover_zone: Rect,
    pub center_y: f32,
}

impl TrafficLightButtons {
    /// Creates the standard traffic-light layout for a given titlebar height (typically 40 for CSD, 28 for SSD).
    #[must_use]
    pub fn new(header_height: u32) -> Self {
        let btn_d = TRAFFIC_LIGHT_DIAMETER;
        let btn_y = (header_height.saturating_sub(btn_d) / 2) as i32;
        let center_y = btn_y as f32 + (btn_d as f32 * 0.5);

        let close_x = TRAFFIC_LIGHT_MARGIN_LEFT as i32;
        let min_x = close_x + btn_d as i32 + TRAFFIC_LIGHT_GAP as i32;
        let max_x = min_x + btn_d as i32 + TRAFFIC_LIGHT_GAP as i32;

        let close_rect = Rect::new(close_x, btn_y, btn_d, btn_d);
        let minimize_rect = Rect::new(min_x, btn_y, btn_d, btn_d);
        let maximize_rect = Rect::new(max_x, btn_y, btn_d, btn_d);

        let hover_zone = Rect::new(
            (close_x - 4).max(0),
            (btn_y - 4).max(0),
            (max_x + btn_d as i32 + 6 - (close_x - 4).max(0)) as u32,
            btn_d + 8,
        );

        Self {
            close_rect,
            minimize_rect,
            maximize_rect,
            hover_zone,
            center_y,
        }
    }

    /// Default CSD layout (header height 40 px, buttons at y=13).
    #[must_use]
    pub fn default_csd() -> Self {
        Self::new(crate::geometry::TITLEBAR_HEIGHT)
    }

    /// Default SSD layout (header height 28 px, buttons at y=7).
    #[must_use]
    pub fn default_ssd() -> Self {
        Self::new(crate::geometry::SSD_TITLEBAR_HEIGHT)
    }

    /// Hit-tests pointer coordinates against individual buttons with a 2 px margin.
    #[must_use]
    pub fn hit_test(&self, px: f64, py: f64) -> Option<TrafficLightKind> {
        let y_min = f64::from(self.close_rect.y - 2);
        let y_max = f64::from(self.close_rect.bottom() + 2);
        if !(y_min..=y_max).contains(&py) {
            return None;
        }

        let margin = 2.0;
        let c_left = f64::from(self.close_rect.x) - margin;
        let c_right = f64::from(self.close_rect.right()) + margin;
        if (c_left..=c_right).contains(&px) {
            return Some(TrafficLightKind::Close);
        }

        let m_left = f64::from(self.minimize_rect.x) - margin;
        let m_right = f64::from(self.minimize_rect.right()) + margin;
        if (m_left..=m_right).contains(&px) {
            return Some(TrafficLightKind::Minimize);
        }

        let x_left = f64::from(self.maximize_rect.x) - margin;
        let x_right = f64::from(self.maximize_rect.right()) + margin;
        if (x_left..=x_right).contains(&px) {
            return Some(TrafficLightKind::Maximize);
        }

        None
    }

    /// Checks if `(px, py)` is hovering over the traffic lights cluster.
    #[must_use]
    pub fn is_hovered(&self, px: f64, py: f64) -> bool {
        self.hover_zone.contains_f64(px, py)
    }

    /// Center coordinates `(cx, cy)` for a given button kind.
    #[must_use]
    pub fn center_of(&self, kind: TrafficLightKind) -> (f32, f32) {
        let cx = match kind {
            TrafficLightKind::Close => TRAFFIC_LIGHT_CENTERS_X[0],
            TrafficLightKind::Minimize => TRAFFIC_LIGHT_CENTERS_X[1],
            TrafficLightKind::Maximize => TRAFFIC_LIGHT_CENTERS_X[2],
        };
        (cx, self.center_y)
    }
}

/// Relative 2D line segment `(dx0, dy0, dx1, dy1)` for drawing microglyphs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphSegment {
    pub dx0: f32,
    pub dy0: f32,
    pub dx1: f32,
    pub dy1: f32,
}

impl GlyphSegment {
    #[must_use]
    pub const fn new(dx0: f32, dy0: f32, dx1: f32, dy1: f32) -> Self {
        Self { dx0, dy0, dx1, dy1 }
    }
}

/// Returns the vector segments defining the hover microglyph for the given button kind.
#[must_use]
pub fn glyph_segments(kind: TrafficLightKind) -> &'static [GlyphSegment] {
    const CLOSE_SEGMENTS: [GlyphSegment; 2] = [
        GlyphSegment::new(-2.6, -2.6, 2.6, 2.6),
        GlyphSegment::new(-2.6, 2.6, 2.6, -2.6),
    ];
    const MINIMIZE_SEGMENTS: [GlyphSegment; 1] = [GlyphSegment::new(-3.2, 0.0, 3.2, 0.0)];
    const MAXIMIZE_SEGMENTS: [GlyphSegment; 2] = [
        GlyphSegment::new(-3.0, 0.0, 3.0, 0.0),
        GlyphSegment::new(0.0, -3.0, 0.0, 3.0),
    ];

    match kind {
        TrafficLightKind::Close => &CLOSE_SEGMENTS,
        TrafficLightKind::Minimize => &MINIMIZE_SEGMENTS,
        TrafficLightKind::Maximize => &MAXIMIZE_SEGMENTS,
    }
}
