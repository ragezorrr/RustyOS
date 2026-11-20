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
pub mod menu;
pub mod theme;
pub mod traffic_lights;
pub mod widgets;
pub mod window_frame;
pub mod window_header;

pub use canvas::{PixelFormat, UiCanvas};
pub use geometry::{
    Point, Rect, SSD_TITLEBAR_HEIGHT, Size, TITLEBAR_HEIGHT, TRAFFIC_LIGHT_CENTERS_X,
    TRAFFIC_LIGHT_DIAMETER, TRAFFIC_LIGHT_GAP, TRAFFIC_LIGHT_MARGIN_LEFT, TRAFFIC_LIGHT_RADIUS,
    TRAFFIC_LIGHT_STEP, WINDOW_RADIUS,
};
pub use menu::{
    MenuEntry, MenuItem, MenuLayout, MenuRowLayout, draw_menu_background, draw_menu_row,
};
pub use theme::{ACCENT_PALETTE, ThemeMode, ThemePalette, TrafficLightKind};
pub use traffic_lights::{GlyphSegment, TrafficLightButtons, glyph_segments};
pub use widgets::{
    ButtonStyle, GroupCard, PushButton, SearchField, SearchFieldHit, ToggleSwitch, WidgetState,
};
pub use window_frame::{WindowFrame, WindowHitTarget};
pub use window_header::{
    ACTIVE_WINDOW_BLUR_OPACITY, CSD_TITLEBAR_HEIGHT, DIALOG_HEADER_HEIGHT,
    INACTIVE_WINDOW_BLUR_OPACITY, WindowHeader, WindowHeaderConfig,
};

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

    #[test]
    fn widgets_toggle_switch_and_buttons() {
        let sw_rect = Rect::new(100, 100, 38, 20);
        let toggle = ToggleSwitch::new(sw_rect, true);
        assert!(toggle.is_hovered(110.0, 110.0));
        assert!(!toggle.is_hovered(90.0, 110.0));

        let btn_rect = Rect::new(50, 50, 80, 30);
        let btn = PushButton::new(btn_rect, ButtonStyle::Primary);
        assert!(btn.is_hovered(60.0, 60.0));
        assert_eq!(btn.style, ButtonStyle::Primary);

        let mut buf = vec![0u8; 200 * 200 * 4];
        let mut canvas = UiCanvas::new(&mut buf, 200, 200, 200 * 4, PixelFormat::Bgra8888);
        let pal = ThemePalette::new(false, 0);

        toggle.draw(&mut canvas, &pal);
        btn.draw(&mut canvas, &pal);

        let card = GroupCard::new(Rect::new(10, 10, 180, 100));
        card.draw(&mut canvas, &pal);
        card.draw_row_divider(&mut canvas, 50, 16, &pal);

        let mut search = SearchField::new(Rect::new(20, 20, 160, 28));
        search.has_text = true;
        assert_eq!(search.hit_test(30.0, 30.0), Some(SearchFieldHit::Input));
        let clear_r = search.clear_btn_rect();
        assert_eq!(
            search.hit_test(f64::from(clear_r.x + 2), f64::from(clear_r.y + 2)),
            Some(SearchFieldHit::Clear)
        );
        search.draw(&mut canvas, &pal);
    }

    #[test]
    fn menu_layout_and_window_header() {
        let entries = vec![
            MenuEntry::Item(MenuItem::new("New File", 1).with_shortcut("⌘N")),
            MenuEntry::Item(MenuItem::new("Open...", 2).with_checked(true)),
            MenuEntry::Separator,
            MenuEntry::Item(MenuItem::new("Delete", 3).with_danger(true)),
        ];

        let layout = MenuLayout::compute(100, 100, 180, &entries);
        assert_eq!(layout.rect.width, 180);
        // 5 + 26 + 26 + 12 + 26 + 5 = 100
        assert_eq!(layout.rect.height, 100);
        assert!(layout.has_check_column);
        assert_eq!(layout.rows.len(), 4);
        assert!(!layout.rows[0].is_separator);
        assert!(layout.rows[2].is_separator);
        assert_eq!(layout.hit_test(120.0, 115.0), Some(0)); // First item
        assert_eq!(layout.hit_test(120.0, 140.0), Some(1)); // Second item
        assert_eq!(layout.hit_test(120.0, 160.0), None); // Separator
        assert_eq!(layout.hit_test(120.0, 175.0), Some(3)); // Delete item

        let (sub_x, sub_y) = layout.cascade_submenu_anchor(0);
        assert_eq!(sub_x, 100 + 180 - 2);
        assert_eq!(sub_y, 100);

        let mut buf = vec![0u8; 300 * 300 * 4];
        let mut canvas = UiCanvas::new(&mut buf, 300, 300, 300 * 4, PixelFormat::Bgra8888);
        let pal = ThemePalette::new(false, 0);

        draw_menu_background(&mut canvas, layout.rect, &pal, true);
        for row in &layout.rows {
            draw_menu_row(&mut canvas, row, row.index == 0, &pal);
        }

        let mut header = WindowHeader::new("RustyOS App");
        assert_eq!(header.config.height, 40);
        assert!(header.leading_inset() > 70.0);
        assert_eq!(
            header.on_pointer_down(21.0, 20.0, 800),
            WindowHitTarget::TrafficLight(TrafficLightKind::Close)
        );
        assert_eq!(
            header.on_pointer_down(200.0, 20.0, 800),
            WindowHitTarget::TitlebarDrag
        );
    }
}
