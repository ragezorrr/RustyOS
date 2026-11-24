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
pub mod clipboard;
pub mod controls;
pub mod dialog;
pub mod draw;
pub mod focus;
pub mod geometry;
pub mod gpu_gles;
pub mod list;
pub mod menu;
pub mod motion;
pub mod scroll;
pub mod shadow;
pub mod text;
pub mod text_input;
pub mod theme;
pub mod traffic_lights;
pub mod widgets;
pub mod window_frame;
pub mod window_header;

pub use canvas::{PixelFormat, UiCanvas};
pub use clipboard::{CLIPBOARD_MIME_TYPES, ClipboardSink, MemoryClipboard, WaylandClipboardBridge};
pub use controls::{Checkbox, ControlKey, DropdownSelect, SegmentedTabs, Slider};
pub use dialog::{DialogAction, DialogLayout, DialogSheet};
pub use draw::{
    UiDrawCmd, UiDrawList, UiPainter, intersect_rect, render_draw_list_to_canvas, squircle_sdf_dist,
};
pub use focus::{FocusScope, WidgetId};
pub use geometry::{
    Point, Rect, SSD_TITLEBAR_HEIGHT, Size, TITLEBAR_HEIGHT, TRAFFIC_LIGHT_CENTERS_X,
    TRAFFIC_LIGHT_DIAMETER, TRAFFIC_LIGHT_GAP, TRAFFIC_LIGHT_MARGIN_LEFT,
    TRAFFIC_LIGHT_MARGIN_RIGHT, TRAFFIC_LIGHT_RADIUS, TRAFFIC_LIGHT_STEP, WINDOW_RADIUS,
};
pub use gpu_gles::GlesSurfaceRenderer;
pub use list::{ListRow, ListSelectionMode, SelectableList};
pub use menu::{
    ContextMenuSession, MenuEntry, MenuItem, MenuLayout, MenuRowLayout, draw_menu_background,
    draw_menu_row, paint_menu_with_painter,
};
pub use motion::{CaretBlink, HoverPressMotion, ScrollMotion, ToggleMotion};
pub use scroll::ScrollView;
pub use shadow::{
    ShadowElevation, ShadowLobe, ShadowStyle, draw_elevation_shadow, draw_elevation_shadow_with,
};
pub use text::{
    FontWeight, GlyphRasterKey, RasterizedGlyph, UiFontEngine, clamp_char_boundary,
    next_word_boundary, prev_word_boundary, word_bounds_at,
};
pub use text_input::{
    InputModifiers, TextBuffer, TextField, TextFieldHit, TextInputEvent, TextInputKey,
};
pub use theme::{
    ACCENT_DARK_PALETTE, ACCENT_PALETTE, ThemeMode, ThemePalette, TrafficLightKind, accent_for,
};
pub use traffic_lights::{
    GlyphSegment, TrafficLightButtons, TrafficLightPlacement, glyph_segments,
};
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
        let left_csd =
            TrafficLightButtons::with_placement(800, TITLEBAR_HEIGHT, TrafficLightPlacement::Left);
        assert_eq!(left_csd.close_rect, Rect::new(14, 13, 14, 14));
        assert_eq!(left_csd.minimize_rect, Rect::new(36, 13, 14, 14));
        assert_eq!(left_csd.maximize_rect, Rect::new(58, 13, 14, 14));
        assert_eq!(left_csd.center_y, 20.0);
        assert_eq!(left_csd.center_of(TrafficLightKind::Close), (21.0, 20.0));

        let csd =
            TrafficLightButtons::with_placement(800, TITLEBAR_HEIGHT, TrafficLightPlacement::Right);
        assert_eq!(csd.close_rect, Rect::new(772, 13, 14, 14));
        assert_eq!(csd.maximize_rect, Rect::new(750, 13, 14, 14));
        assert_eq!(csd.minimize_rect, Rect::new(728, 13, 14, 14));
        assert_eq!(csd.center_y, 20.0);

        assert_eq!(csd.center_of(TrafficLightKind::Close), (779.0, 20.0));
        assert_eq!(csd.center_of(TrafficLightKind::Minimize), (735.0, 20.0));
        assert_eq!(csd.center_of(TrafficLightKind::Maximize), (757.0, 20.0));

        assert_eq!(csd.hit_test(779.0, 20.0), Some(TrafficLightKind::Close));
        assert_eq!(csd.hit_test(735.0, 20.0), Some(TrafficLightKind::Minimize));
        assert_eq!(csd.hit_test(757.0, 20.0), Some(TrafficLightKind::Maximize));
        assert_eq!(csd.hit_test(500.0, 20.0), None);

        assert!(csd.is_hovered(779.0, 20.0));
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
        assert_eq!(dark.accent, ACCENT_DARK_PALETTE[1]);
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

        let csd =
            TrafficLightButtons::with_placement(100, TITLEBAR_HEIGHT, TrafficLightPlacement::Left);
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

    #[test]
    fn shadow_elevation_and_rendering() {
        let active = ShadowStyle::for_elevation(ShadowElevation::ActiveWindow);
        assert_eq!(active.lobes[0].sigma, 24.0);
        assert_eq!(active.lobes[0].offset.1, 14.0);
        assert!((active.lobes[0].opacity - (0.22 * 0.85)).abs() < 0.001);
        assert_eq!(active.lobes[1].sigma, 9.0);
        assert_eq!(active.lobes[1].offset.1, 7.5);
        assert_eq!(active.lobes[2].sigma, 1.5);
        assert_eq!(active.lobes[2].offset.1, 1.0);
        assert!(active.lobes[2].opacity > 0.0 && active.lobes[2].opacity < 0.06);

        let inactive = ShadowStyle::for_elevation(ShadowElevation::InactiveWindow);
        assert_eq!(inactive.lobes[0].sigma, 24.0);
        assert!((inactive.lobes[0].opacity - (0.22 * 0.48)).abs() < 0.001);
        assert!(active.lobes[2].opacity > inactive.lobes[2].opacity);

        let menu = ShadowStyle::for_elevation(ShadowElevation::MenuOrTooltip);
        assert_eq!(menu.lobes[0].sigma, 14.0);
        assert_eq!(menu.lobes[2].opacity, 0.0);

        let popup = ShadowStyle::for_elevation(ShadowElevation::Popup);
        assert_eq!(popup.lobes[0].sigma, 10.0);
        assert_eq!(popup.lobes[2].opacity, 0.0);

        let dock = ShadowStyle::for_elevation(ShadowElevation::Dock);
        assert_eq!(dock.lobes[0].sigma, 10.0);
        assert_eq!(dock.lobes[2].opacity, 0.0); // No contact line

        let mut buf = vec![0u8; 100 * 100 * 4];
        let mut canvas = UiCanvas::new(&mut buf, 100, 100, 100 * 4, PixelFormat::Bgra8888);
        let pal = ThemePalette::new(false, 0);
        draw_elevation_shadow(
            &mut canvas,
            Rect::new(20, 20, 60, 60),
            14,
            ShadowElevation::Popup,
            &pal,
        );
    }

    #[test]
    fn focus_scope_tab_traversal_and_reduced_motion() {
        use std::time::Duration;

        let id_a = WidgetId::from_tag("wifi_ssid");
        let id_b = WidgetId::from_tag("wifi_pass");
        let id_c = WidgetId::from_tag("connect_btn");
        assert_ne!(id_a, id_b);

        let mut focus = FocusScope::new();
        focus.begin_frame();
        focus.register(id_a);
        focus.register(id_b);
        focus.register(id_c);
        focus.register(id_a); // deduplicated

        let t0 = Duration::from_millis(10);
        assert_eq!(focus.cycle_focus(false, t0, false), Some(id_a));
        assert!(focus.is_focused(id_a));
        assert!(focus.tick(Duration::from_millis(200)));
        assert!((focus.ring_alpha() - 1.0).abs() < 0.01);

        assert_eq!(focus.cycle_focus(false, t0, true), Some(id_b));
        assert_eq!(focus.cycle_focus(true, t0, true), Some(id_a));
        assert_eq!(focus.cycle_focus(true, t0, true), Some(id_c));

        focus.clear_focus(t0, true);
        assert_eq!(focus.focused(), None);
        assert_eq!(focus.ring_alpha(), 0.0);
    }

    #[test]
    fn text_field_editing_selection_word_bounds_and_clipboard() {
        use std::time::Duration;

        let mut fonts = UiFontEngine::new();
        let mut clip = MemoryClipboard::new();
        let mut field = TextField::new(Rect::new(10, 10, 220, 32), "Type here");
        let now = Duration::from_millis(100);

        for ch in "hello rusty world".chars() {
            let ev = field.handle_key(
                TextInputKey::Char(ch),
                InputModifiers::default(),
                now,
                &mut fonts,
                &mut clip,
            );
            assert_eq!(ev, TextInputEvent::Changed);
        }
        assert_eq!(field.buffer.as_str(), "hello rusty world");

        // Double click near middle word "rusty" -> selects "rusty"
        let rusty_x = 20.0 + f64::from(fonts.x_for_byte("hello ru", 8, 13.0, false));
        let hit = field.on_pointer_down(rusty_x, 24.0, 2, now, &mut fonts);
        assert_eq!(hit, Some(TextFieldHit::Input));
        assert_eq!(field.buffer.selected_text(), Some("rusty"));

        // Copy, move end, insert space + paste
        assert_eq!(
            field.handle_key(
                TextInputKey::Char('c'),
                InputModifiers {
                    ctrl: true,
                    shift: false,
                    alt: false,
                },
                now,
                &mut fonts,
                &mut clip,
            ),
            TextInputEvent::CaretMoved
        );
        assert_eq!(clip.get_text().as_deref(), Some("rusty"));

        field.handle_key(
            TextInputKey::End,
            InputModifiers::default(),
            now,
            &mut fonts,
            &mut clip,
        );
        field.handle_key(
            TextInputKey::Char(' '),
            InputModifiers::default(),
            now,
            &mut fonts,
            &mut clip,
        );
        field.handle_key(
            TextInputKey::Char('v'),
            InputModifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
            now,
            &mut fonts,
            &mut clip,
        );
        assert_eq!(field.buffer.as_str(), "hello rusty world rusty");

        // Masked password blocks copy/cut
        let mut pw = TextField::password(Rect::new(10, 50, 200, 32), "Password");
        pw.buffer.set_text("secret");
        pw.buffer.select_all();
        assert_eq!(
            pw.handle_key(
                TextInputKey::Char('c'),
                InputModifiers {
                    ctrl: true,
                    shift: false,
                    alt: false,
                },
                now,
                &mut fonts,
                &mut clip,
            ),
            TextInputEvent::Ignored
        );
        assert_eq!(clip.get_text().as_deref(), Some("rusty"));
    }

    #[test]
    fn scroll_view_thumb_drag_and_smooth_scroll_respects_reduced_motion() {
        use std::time::Duration;

        let mut sv = ScrollView::new(Rect::new(0, 0, 300, 200));
        sv.set_bounds(Rect::new(0, 0, 300, 200), 1000);
        assert_eq!(sv.max_scroll(), 800.0);
        assert!(sv.thumb_rect().is_some());

        let t0 = Duration::from_millis(10);
        sv.scroll_by(200.0, t0, false);
        assert!(sv.motion.is_active());
        assert!(sv.tick(Duration::from_millis(90)));
        assert!(sv.offset_y() > 0 && sv.offset_y() < 200);
        let _ = sv.tick(Duration::from_millis(200));
        assert_eq!(sv.offset_y(), 200);

        sv.scroll_by(150.0, Duration::from_millis(250), true);
        assert!(!sv.motion.is_active());
        assert_eq!(sv.offset_y(), 350);

        let thumb = sv.thumb_rect().unwrap_or(Rect::new(0, 0, 0, 0));
        assert!(sv.on_pointer_down(
            f64::from(thumb.x + 2),
            f64::from(thumb.y + 4),
            Duration::from_millis(260),
            false,
        ));
        assert!(sv.on_pointer_move(f64::from(thumb.x + 2), f64::from(thumb.y + 44)));
        assert!(sv.offset_y() > 350);
        assert!(sv.on_pointer_up());
    }

    #[test]
    fn controls_dropdown_tabs_checkbox_and_slider_interactions() {
        use std::time::Duration;

        let mut dd = DropdownSelect::new(
            Rect::new(20, 20, 180, 28),
            vec![
                String::from("60 Hz"),
                String::from("120 Hz"),
                String::from("144 Hz"),
            ],
            0,
        );
        assert_eq!(dd.selected_label(), "60 Hz");
        assert_eq!(dd.handle_key(ControlKey::Space), None);
        assert!(dd.open);
        assert_eq!(dd.handle_key(ControlKey::Down), None);
        assert_eq!(dd.handle_key(ControlKey::Enter), Some(1));
        assert!(!dd.open);
        assert_eq!(dd.selected_label(), "120 Hz");

        let mut tabs = SegmentedTabs::new(
            Rect::new(20, 70, 240, 28),
            vec![
                String::from("Light"),
                String::from("Dark"),
                String::from("Auto"),
            ],
            0,
        );
        let t0 = Duration::from_millis(10);
        assert_eq!(tabs.hit_test(140.0, 84.0), Some(1));
        assert_eq!(tabs.handle_key(ControlKey::Right, t0, false), Some(1));
        assert!(tabs.tick(Duration::from_millis(200)));
        assert_eq!(tabs.selected, 1);

        let mut cb = Checkbox::new(20, 120, "Enable Night Light", false);
        assert_eq!(cb.handle_key(ControlKey::Space), Some(true));
        assert!(cb.checked);

        let mut slider = Slider::new(Rect::new(20, 160, 200, 24), 0.25);
        let val = slider.on_pointer_down(120.0, 172.0);
        assert!(val.is_some_and(|v| (v - 0.5).abs() < 0.08));
        assert!(slider.on_pointer_up());
        assert_eq!(slider.handle_key(ControlKey::End), Some(1.0));
    }

    #[test]
    fn selectable_list_dialog_sheet_and_context_menu_session() {
        use std::time::Duration;

        let t0 = Duration::from_millis(10);
        let rows = vec![
            ListRow::new("Wi-Fi")
                .with_trailing("Connected")
                .with_chevron(true),
            ListRow::new("Bluetooth").with_subtitle("Visible as RustyOS"),
        ];
        let mut list = SelectableList::new(Rect::new(10, 10, 280, 100), rows);
        assert_eq!(list.move_selection(1, t0, true), Some(0));
        assert_eq!(list.move_selection(1, t0, true), Some(1));
        assert_eq!(list.on_pointer_down(40.0, 20.0), Some(0));

        let mut sheet = DialogSheet::prompt(
            "New Folder",
            "Enter a folder name:",
            "Create",
            "Folder name",
            "Untitled Folder",
        );
        let layout = sheet.sync_input_rect(800, 600);
        assert!(layout.input_rect.is_some());
        assert_eq!(
            sheet.hit_test(
                f64::from(layout.primary_rect.x + 10),
                f64::from(layout.primary_rect.y + 10),
                800,
                600,
            ),
            Some(DialogAction::Confirm)
        );

        let mut ctx = ContextMenuSession::new();
        ctx.open_clamped(
            1200,
            780,
            180,
            1280,
            800,
            vec![
                MenuEntry::Item(MenuItem::new("Open", 10)),
                MenuEntry::Separator,
                MenuEntry::Item(MenuItem::new("Delete", 20).with_danger(true)),
            ],
        );
        assert!(ctx.is_open());
        assert_eq!(ctx.move_hover(1), Some(0));
        assert_eq!(ctx.move_hover(1), Some(2));
        assert_eq!(ctx.activate_hovered(), Some(20));
        assert!(!ctx.is_open());
    }

    #[test]
    fn draw_list_cpu_clipping_and_painter_output() {
        use std::time::Duration;

        let mut fonts = UiFontEngine::new();
        let pal = ThemePalette::new(false, 0);
        let mut dl = UiDrawList::new();
        let mut toggle_anim = ToggleMotion::new(false);
        toggle_anim.set_active(true, Duration::from_millis(0), true);

        {
            let mut painter = UiPainter::new(&mut dl);
            painter.fill_rect(Rect::new(0, 0, 240, 180), pal.window_bg);
            painter.push_clip(Rect::new(10, 10, 100, 40));
            painter.fill_rect(Rect::new(0, 0, 240, 180), [255, 0, 0, 255]);
            painter.pop_clip();
            let sw = ToggleSwitch::new(Rect::new(20, 70, 38, 20), true);
            sw.paint_animated(&mut painter, toggle_anim.progress(), &pal);
            let mut tf = TextField::search(Rect::new(20, 110, 180, 28), "Search");
            tf.buffer.set_text("RustyOS");
            tf.paint(&mut painter, &mut fonts, &pal, true, 1.0, Duration::ZERO);
        }

        let mut buf = vec![0_u8; 240 * 180 * 4];
        let mut canvas = UiCanvas::new(&mut buf, 240, 180, 240 * 4, PixelFormat::Bgra8888);
        render_draw_list_to_canvas(&dl, &mut canvas, &mut fonts);

        // Inside clip (x=20, y=20) should be pure red BGRA=(0,0,255,255); outside clip stays window_bg!
        let in_idx = (20 * 240 + 20) * 4;
        assert_eq!([buf[in_idx + 2], buf[in_idx + 1], buf[in_idx]], [255, 0, 0]);
        let out_idx = (20 * 240 + 180) * 4;
        assert_eq!(
            [buf[out_idx + 2], buf[out_idx + 1], buf[out_idx]],
            [pal.window_bg[0], pal.window_bg[1], pal.window_bg[2]]
        );
    }
}
