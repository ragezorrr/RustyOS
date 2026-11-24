//! Standard window-control traffic light layout and vector microglyph geometry.

use crate::geometry::{
    Rect, TRAFFIC_LIGHT_DIAMETER, TRAFFIC_LIGHT_GAP, TRAFFIC_LIGHT_MARGIN_LEFT,
    TRAFFIC_LIGHT_MARGIN_RIGHT,
};
use crate::theme::TrafficLightKind;

/// Horizontal placement of window controls in the titlebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrafficLightPlacement {
    /// Placed on the left: `[Close] [Minimize] [Maximize]`.
    Left,
    /// Placed on the right: `[Minimize] [Maximize] [Close]`.
    #[default]
    Right,
}

/// Geometry and hit-testing for the 3 window controls (Close, Minimize, Maximize).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrafficLightButtons {
    pub close_rect: Rect,
    pub minimize_rect: Rect,
    pub maximize_rect: Rect,
    pub hover_zone: Rect,
    pub center_y: f32,
    pub placement: TrafficLightPlacement,
}

impl TrafficLightButtons {
    /// Creates the right-aligned traffic-light layout for a given window width and header height.
    #[must_use]
    pub fn for_window(width: u32, header_height: u32) -> Self {
        Self::with_placement(width, header_height, TrafficLightPlacement::Right)
    }

    /// Creates traffic-light layout with explicit placement (`Left` or `Right`).
    #[must_use]
    pub fn with_placement(
        width: u32,
        header_height: u32,
        placement: TrafficLightPlacement,
    ) -> Self {
        let btn_d = TRAFFIC_LIGHT_DIAMETER;
        let btn_y = (header_height.saturating_sub(btn_d) / 2) as i32;
        let center_y = btn_y as f32 + (btn_d as f32 * 0.5);

        let (min_x, max_x, close_x) = match placement {
            TrafficLightPlacement::Left => {
                let close_x = TRAFFIC_LIGHT_MARGIN_LEFT as i32;
                let min_x = close_x + btn_d as i32 + TRAFFIC_LIGHT_GAP as i32;
                let max_x = min_x + btn_d as i32 + TRAFFIC_LIGHT_GAP as i32;
                (min_x, max_x, close_x)
            }
            TrafficLightPlacement::Right => {
                let right_edge = (width as i32) - (TRAFFIC_LIGHT_MARGIN_RIGHT as i32);
                let close_x = right_edge - btn_d as i32;
                let max_x = close_x - (TRAFFIC_LIGHT_GAP as i32) - btn_d as i32;
                let min_x = max_x - (TRAFFIC_LIGHT_GAP as i32) - btn_d as i32;
                (min_x, max_x, close_x)
            }
        };

        let close_rect = Rect::new(close_x, btn_y, btn_d, btn_d);
        let minimize_rect = Rect::new(min_x, btn_y, btn_d, btn_d);
        let maximize_rect = Rect::new(max_x, btn_y, btn_d, btn_d);

        let min_bound_x = min_x.min(close_x);
        let max_bound_x = min_x.max(close_x) + btn_d as i32;

        let hover_zone = Rect::new(
            (min_bound_x - 4).max(0),
            (btn_y - 4).max(0),
            (max_bound_x + 6 - (min_bound_x - 4).max(0)) as u32,
            btn_d + 8,
        );

        Self {
            close_rect,
            minimize_rect,
            maximize_rect,
            hover_zone,
            center_y,
            placement,
        }
    }

    /// Creates the standard traffic-light layout for a given titlebar height at default 800 px width.
    #[must_use]
    pub fn new(header_height: u32) -> Self {
        Self::for_window(800, header_height)
    }

    /// Default CSD layout (header height 40 px, standard 800 px width, left placement).
    #[must_use]
    pub fn default_csd() -> Self {
        Self::with_placement(
            800,
            crate::geometry::TITLEBAR_HEIGHT,
            TrafficLightPlacement::Left,
        )
    }

    /// Default SSD layout (header height 36 px, standard 800 px width, right placement).
    #[must_use]
    pub fn default_ssd() -> Self {
        Self::with_placement(
            800,
            crate::geometry::SSD_TITLEBAR_HEIGHT,
            TrafficLightPlacement::Right,
        )
    }

    /// Default SSD layout for a specific window width (right placement).
    #[must_use]
    pub fn default_ssd_for_width(width: u32) -> Self {
        Self::with_placement(
            width,
            crate::geometry::SSD_TITLEBAR_HEIGHT,
            TrafficLightPlacement::Right,
        )
    }

    /// Default CSD layout for a specific window width (left placement).
    #[must_use]
    pub fn default_csd_for_width(width: u32) -> Self {
        Self::with_placement(
            width,
            crate::geometry::TITLEBAR_HEIGHT,
            TrafficLightPlacement::Left,
        )
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
        let rect = match kind {
            TrafficLightKind::Close => self.close_rect,
            TrafficLightKind::Minimize => self.minimize_rect,
            TrafficLightKind::Maximize => self.maximize_rect,
        };
        (rect.x as f32 + (rect.width as f32 * 0.5), self.center_y)
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
