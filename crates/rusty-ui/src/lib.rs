//! Shared UI foundations, geometry, themes, and canvas drawing for RustyOS.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::collapsible_if,
    clippy::match_same_arms
)]

pub mod canvas;
pub mod geometry;
pub mod theme;
pub mod traffic_lights;
pub mod window_frame;

pub use canvas::{PixelFormat, UiCanvas};
pub use geometry::{
    Point, Rect, SSD_TITLEBAR_HEIGHT, Size, TITLEBAR_HEIGHT, TRAFFIC_LIGHT_CENTERS_X,
    TRAFFIC_LIGHT_DIAMETER, TRAFFIC_LIGHT_GAP, TRAFFIC_LIGHT_MARGIN_LEFT, TRAFFIC_LIGHT_RADIUS,
    TRAFFIC_LIGHT_STEP, WINDOW_RADIUS,
};
pub use theme::{ACCENT_PALETTE, ThemeMode, ThemePalette, TrafficLightKind};
pub use traffic_lights::{GlyphSegment, TrafficLightButtons, glyph_segments};
pub use window_frame::{WindowFrame, WindowHitTarget};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_rect_contains_and_insets() {
        let r = Rect::new(10, 20, 100, 50);
        assert!(r.contains(10, 20));
        assert!(r.contains(109, 69));
        assert!(!r.contains(110, 70));
        assert!(!r.contains(9, 20));

        let inset = r.inset(5, 5);
        assert_eq!(inset, Rect::new(15, 25, 90, 40));
        assert_eq!(r.center_f32(), (60.0, 45.0));
    }

    #[test]
    fn traffic_light_buttons_hit_testing_and_centers() {
        let csd = TrafficLightButtons::default_csd();
        assert_eq!(csd.close_rect, Rect::new(14, 13, 14, 14));
        assert_eq!(csd.minimize_rect, Rect::new(36, 13, 14, 14));
        assert_eq!(csd.maximize_rect, Rect::new(58, 13, 14, 14));
        assert_eq!(csd.center_y, 20.0);

        assert_eq!(csd.center_of(TrafficLightKind::Close), (21.0, 20.0));
        assert_eq!(csd.center_of(TrafficLightKind::Minimize), (43.0, 20.0));
        assert_eq!(csd.center_of(TrafficLightKind::Maximize), (65.0, 20.0));

        assert_eq!(csd.hit_test(21.0, 20.0), Some(TrafficLightKind::Close));
        assert_eq!(csd.hit_test(43.0, 20.0), Some(TrafficLightKind::Minimize));
        assert_eq!(csd.hit_test(65.0, 20.0), Some(TrafficLightKind::Maximize));
        assert_eq!(csd.hit_test(100.0, 20.0), None);

        assert!(csd.is_hovered(21.0, 20.0));
        assert!(!csd.is_hovered(150.0, 20.0));
    }

    #[test]
    fn theme_palette_resolves_light_and_dark() {
        let light = ThemePalette::new(false, 0);
        assert!(!light.is_dark);
        assert_eq!(light.accent, ACCENT_PALETTE[0]);
        assert_eq!(light.window_bg, [0xF5, 0xF6, 0xFA, 255]);

        let dark = ThemePalette::new(true, 1);
        assert!(dark.is_dark);
        assert_eq!(dark.accent, ACCENT_PALETTE[1]);
        assert_eq!(dark.window_bg, [0x1C, 0x1C, 0x1D, 255]);

        let close = TrafficLightKind::Close;
        assert_eq!(close.active_color(), [0xEE, 0x5A, 0x6A, 255]);
        let pressed = close.pressed_color();
        assert_eq!(pressed[0], ((0xEEu16 * 8) / 10) as u8);
    }

    #[test]
    fn canvas_renders_traffic_lights_and_rounded_rect() {
        let mut buf = vec![0u8; 100 * 50 * 4];
        let mut canvas = UiCanvas::new(&mut buf, 100, 50, 400, PixelFormat::Bgra8888);
        canvas.clear([255, 255, 255, 255]);

        let csd = TrafficLightButtons::default_csd();
        canvas.draw_traffic_lights(&csd, true, true, Some(TrafficLightKind::Close));

        let idx = (20 * 400 + 21 * 4) as usize;
        assert_ne!(&canvas.buffer[idx..idx + 4], &[255, 255, 255, 255]);

        canvas.fast_rounded_rect(Rect::new(5, 5, 40, 20), 4.0, [10, 20, 30, 255]);
    }

    #[test]
    fn window_frame_interaction_and_double_click() {
        let mut frame = WindowFrame::new("Test Window");
        assert_eq!(
            frame.hit_test(21.0, 20.0, 800),
            WindowHitTarget::TrafficLight(TrafficLightKind::Close)
        );
        assert_eq!(
            frame.hit_test(200.0, 20.0, 800),
            WindowHitTarget::TitlebarDrag
        );
        assert_eq!(frame.hit_test(200.0, 100.0, 800), WindowHitTarget::Content);

        assert!(frame.on_pointer_motion(21.0, 20.0));
        assert!(frame.traffic_hovered);
        assert!(!frame.on_pointer_motion(43.0, 20.0));
        assert!(frame.on_pointer_leave());
        assert!(!frame.traffic_hovered);

        assert_eq!(
            frame.on_pointer_down(21.0, 20.0, 800),
            WindowHitTarget::TrafficLight(TrafficLightKind::Close)
        );
        assert_eq!(
            frame.on_pointer_up(21.0, 20.0),
            Some(TrafficLightKind::Close)
        );

        assert!(!frame.is_maximized);
        let _ = frame.check_double_click_maximize(200.0, 20.0, 800);
        let toggled = frame.check_double_click_maximize(201.0, 21.0, 800);
        assert!(toggled);
        assert!(frame.is_maximized);
    }
}
