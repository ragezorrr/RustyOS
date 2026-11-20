//! Geometric primitives and standard dimension constants for RustyOS UI.

pub const WINDOW_RADIUS: u32 = 20;
pub const TITLEBAR_HEIGHT: u32 = 40;
pub const SSD_TITLEBAR_HEIGHT: u32 = 28;

pub const TRAFFIC_LIGHT_DIAMETER: u32 = 14;
pub const TRAFFIC_LIGHT_RADIUS: f32 = 7.0;
pub const TRAFFIC_LIGHT_GAP: u32 = 8;
pub const TRAFFIC_LIGHT_STEP: u32 = 22;
pub const TRAFFIC_LIGHT_MARGIN_LEFT: u32 = 14;
pub const TRAFFIC_LIGHT_CENTERS_X: [f32; 3] = [21.0, 43.0, 65.0];

/// 2D integer point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// 2D integer size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// 2D integer rectangle with coordinate helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn from_size(size: Size) -> Self {
        Self {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        }
    }

    #[must_use]
    pub const fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    #[must_use]
    pub const fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    #[must_use]
    pub const fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    #[must_use]
    pub fn contains_f64(&self, px: f64, py: f64) -> bool {
        px >= f64::from(self.x)
            && px < f64::from(self.right())
            && py >= f64::from(self.y)
            && py < f64::from(self.bottom())
    }

    #[must_use]
    pub fn center_f32(&self) -> (f32, f32) {
        (
            self.x as f32 + self.width as f32 * 0.5,
            self.y as f32 + self.height as f32 * 0.5,
        )
    }

    #[must_use]
    pub fn inset(&self, dx: i32, dy: i32) -> Self {
        let x = self.x + dx;
        let y = self.y + dy;
        let width = (self.width as i32 - 2 * dx).max(0) as u32;
        let height = (self.height as i32 - 2 * dy).max(0) as u32;
        Self {
            x,
            y,
            width,
            height,
        }
    }
}
