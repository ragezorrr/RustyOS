use smithay::utils::{Point, Rectangle};

use super::dock::*;
use super::lock_screen::*;
use super::modals::*;
use super::quick_settings::*;
use super::spotlight_overview::*;
use super::ssd::*;
use super::tokens::anim_offset;
use super::top_bar::*;
use super::*;

#[test]
fn usable_work_area_reserves_top_and_bottom_margins() {
    let size = Size::from((1920, 1080));
    let work = usable_work_area(size);
    assert_eq!(work.x, 0);
    assert_eq!(work.y, TOP_BAR_HEIGHT);
    assert_eq!(work.width, 1920);
    let expected_h = 1080 - TOP_BAR_HEIGHT - (DOCK_HEIGHT + DOCK_MARGIN_BOTTOM * 2);
    assert_eq!(work.height, expected_h);
}

#[test]
fn generate_shell_render_elements_produces_non_empty_layout() {
    let size = Size::from((1280, 800));
    let top_tel = TopBarTelemetry::default();
    let elements = generate_shell_render_elements(
        size,
        1,
        "master-stack",
        2,
        true,
        &top_tel,
        ShellOverlayView::None,
        false,
    );
    assert!(!elements.is_empty());

    let sample_items = [crate::launcher::LauncherCandidate {
        name: "Terminal".to_owned(),
        subtitle: "foot".to_owned(),
        exec: "foot".to_owned(),
        badge: ">_".to_owned(),
        icon: Some("utilities-terminal".to_owned()),
        is_custom_command: false,
    }];
    let spot = SpotlightRenderState {
        query: "foot",
        selected_in_window: 0,
        cursor_offset_x: 32,
        items: &sample_items,
        progress: 1.0,
    };
    let elements_with_launcher = generate_shell_render_elements(
        size,
        1,
        "master-stack",
        2,
        true,
        &top_tel,
        ShellOverlayView::Spotlight(&spot),
        false,
    );
    assert!(elements_with_launcher.len() > elements.len());

    let qs = QuickSettingsRenderState {
        layout_mode: "master-stack",
        reduced_motion: false,
        network_label: "Wi-Fi Connected",
        net_connected: true,
        wifi_enabled: true,
        wifi_connected: true,
        wifi_ssid: Some("Lab5G"),
        wifi_signal_percent: Some(80),
        bt_available: true,
        bt_powered: true,
        bt_label: "BT: On",
        battery_percent: 88,
        has_battery: true,
        ac_online: true,
        battery_charging: true,
        brightness_available: false,
        brightness_percent: 100,
        volume_percent: 75,
        audio_muted: false,
        media_active: false,
        media_playing: false,
        media_title: "",
        focus_mode: false,
        frame_p50_us: 16_667,
        window_count: 2,
        progress: 1.0,
        glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
        reduce_transparency: false,
    };
    let elements_with_qs = generate_shell_render_elements(
        size,
        1,
        "master-stack",
        2,
        true,
        &top_tel,
        ShellOverlayView::QuickSettings(&qs),
        false,
    );
    assert!(elements_with_qs.len() > elements.len());
}

#[test]
fn top_bar_hit_testing_works_accurately() {
    assert!(check_top_bar_status_click(1280, 1200.0, 12.0));
    assert!(!check_top_bar_status_click(1280, 100.0, 12.0));

    let left_ws = compute_top_bar_left_layout_with_options("RustyOS", true);
    let layout_mid = f64::from(left_ws.layout_x + left_ws.layout_w / 2);
    assert!(check_top_bar_layout_click(layout_mid, 12.0));
    assert!(!check_top_bar_layout_click_with_options(
        "RustyOS", false, layout_mid, 12.0
    ));
    assert!(!check_top_bar_layout_click(50.0, 12.0));
    assert!(!check_top_bar_layout_click(300.0, 12.0));

    // Test TopBar Left Menu hits (`RUSTYOS-038`, `RUSTYOS-049`, `RUSTYOS-067`):
    // In authentic macOS Menu Bar behavior, `Files` (Finder equivalent) is the active frontmost
    // application when on the Desktop, and all menus (`SystemLogo`, `ActiveApp`, `File..Help`) are clickable.
    assert_eq!(
        check_top_bar_menu_click(f64::from(TOP_BAR_LOGO_X + TOP_BAR_LOGO_W / 2), 12.0),
        Some(TopBarMenuHit::SystemLogo)
    );

    let left = compute_top_bar_left_layout("Files");
    for (hit, bx, bw) in [
        (TopBarMenuHit::ActiveApp, left.app_x, left.app_w),
        (TopBarMenuHit::File, left.file_x, left.file_w),
        (TopBarMenuHit::Edit, left.edit_x, left.edit_w),
        (TopBarMenuHit::View, left.view_x, left.view_w),
        (TopBarMenuHit::Go, left.go_x, left.go_w),
        (TopBarMenuHit::Window, left.win_x, left.win_w),
        (TopBarMenuHit::Help, left.help_x, left.help_w),
    ] {
        assert_eq!(
            check_top_bar_menu_click_for_app("Files", f64::from(bx + bw / 2), 12.0),
            Some(hit)
        );
    }
    assert_eq!(
        check_top_bar_menu_click_for_app("Files", f64::from(left.help_x + left.help_w + 15), 12.0),
        None
    );
    assert_eq!(
        check_top_bar_menu_click(15.0, f64::from(TOP_BAR_HEIGHT + 5)),
        None
    );
}

#[test]
fn quick_settings_hit_testing_works_accurately() {
    let size = Size::from((1280, 800));
    let layout = quick_settings_layout(size.w, size.h, 0);
    let cases = [
        (100, 200, QuickSettingsHit::OutsidePopover),
        (
            layout.px + 2,
            layout.py + 2,
            QuickSettingsHit::InsidePopover,
        ),
        (
            layout.wifi_pill.loc.x + 60,
            layout.wifi_pill.loc.y + 27,
            QuickSettingsHit::ToggleWifi,
        ),
        (
            layout.bt_pill.loc.x + 60,
            layout.bt_pill.loc.y + 27,
            QuickSettingsHit::ToggleBluetooth,
        ),
        (
            layout.media_prev_btn.loc.x + 12,
            layout.media_prev_btn.loc.y + 12,
            QuickSettingsHit::MediaPrev,
        ),
        (
            layout.media_play_btn.loc.x + 12,
            layout.media_play_btn.loc.y + 12,
            QuickSettingsHit::MediaPlayPause,
        ),
        (
            layout.media_next_btn.loc.x + 12,
            layout.media_next_btn.loc.y + 12,
            QuickSettingsHit::MediaNext,
        ),
        (
            layout.power_pill.loc.x + 60,
            layout.power_pill.loc.y + 30,
            QuickSettingsHit::RefreshMetrics,
        ),
        (
            layout.layout_btn.loc.x + 33,
            layout.layout_btn.loc.y + 33,
            QuickSettingsHit::ToggleLayout,
        ),
        (
            layout.overview_btn.loc.x + 33,
            layout.overview_btn.loc.y + 33,
            QuickSettingsHit::ToggleOverview,
        ),
        (
            layout.motion_btn.loc.x + 33,
            layout.motion_btn.loc.y + 33,
            QuickSettingsHit::ToggleReducedMotion,
        ),
        (
            layout.transparency_btn.loc.x + 33,
            layout.transparency_btn.loc.y + 33,
            QuickSettingsHit::ToggleReduceTransparency,
        ),
        (
            layout.focus_pill.loc.x + 60,
            layout.focus_pill.loc.y + 30,
            QuickSettingsHit::ToggleFocus,
        ),
        (
            layout.sound_mute_zone.loc.x + 15,
            layout.sound_mute_zone.loc.y + 15,
            QuickSettingsHit::ToggleMute,
        ),
        (
            layout.lock_btn.loc.x + 50,
            layout.lock_btn.loc.y + 15,
            QuickSettingsHit::LockSession,
        ),
    ];
    for (px, py, expected) in cases {
        assert_eq!(
            check_quick_settings_click(size, f64::from(px), f64::from(py)),
            expected
        );
    }
}

#[test]
fn spotlight_click_detects_rows_inside_and_outside_modal() {
    let size = Size::from((1280, 800));
    let (mx, my, mw, _mh) = spotlight_modal_rect(size.w, size.h);
    assert_eq!(
        check_spotlight_click(size, f64::from(mx + 40), f64::from(my + 95)),
        SpotlightHit::Row(0)
    );
    assert_eq!(
        check_spotlight_click(size, f64::from(mx + 40), f64::from(my + 145)),
        SpotlightHit::Row(1)
    );
    assert_eq!(
        check_spotlight_click(size, f64::from(mx + 40), f64::from(my + 30)),
        SpotlightHit::InsideModal
    );
    // Transparent gap between search pill (h=54) and results card (y=my+64) closes Spotlight.
    assert_eq!(
        check_spotlight_click(size, f64::from(mx + 40), f64::from(my + 58)),
        SpotlightHit::OutsideModal
    );
    assert_eq!(
        check_spotlight_click(size, f64::from(mx + mw + 20), f64::from(my + 30)),
        SpotlightHit::OutsideModal
    );
}

#[test]
fn workspace_click_mapping_accurately_detects_all_nine_workspaces() {
    let left = compute_top_bar_left_layout_with_options("RustyOS", true);
    for ws in 1u8..=9u8 {
        let x = f64::from(left.ws_start_x) + f64::from(i32::from(ws - 1) * WS_BTN_STRIDE) + 5.0;
        let detected = check_top_bar_workspace_click(x, 12.0);
        assert_eq!(detected, Some(ws));
        assert_eq!(
            check_top_bar_workspace_click_with_options("RustyOS", false, x, 12.0),
            None
        );
    }

    // Clicks outside top bar height or beyond bounds return None
    assert_eq!(
        check_top_bar_workspace_click(15.0, f64::from(TOP_BAR_HEIGHT + 10)),
        None
    );
    assert_eq!(check_top_bar_workspace_click(2.0, 10.0), None);
}

#[test]
fn ssd_hit_testing_detects_traffic_lights_and_drag_in_top_to_bottom_order() {
    let win1 = rusty_wm::WindowId(1);
    let win2 = rusty_wm::WindowId(2);
    let windows = vec![
        (win1, rusty_wm::Rect::new(100, 100, 500, 400)),
        (win2, rusty_wm::Rect::new(100, 100, 500, 400)),
    ];

    // Overlapping window win2 is topmost (last in bottom-to-top list)
    assert_eq!(
        check_window_ssd_hit(&windows, 121.0, 114.0),
        Some(SsdHit::Close(win2))
    );
    assert_eq!(
        check_window_ssd_hit(&windows, 143.0, 114.0),
        Some(SsdHit::Minimize(win2))
    );
    assert_eq!(
        check_window_ssd_hit(&windows, 165.0, 114.0),
        Some(SsdHit::Maximize(win2))
    );
    assert_eq!(
        check_window_ssd_hit(&windows, 300.0, 114.0),
        Some(SsdHit::TitlebarDrag(win2))
    );
    // Click below titlebar inside client area returns None (must not hit resize edges)
    assert_eq!(
        check_window_ssd_hit(&windows, 300.0, f64::from(100 + TITLEBAR_HEIGHT + 20)),
        None
    );

    // Edge and corner resize hits (`RUSTYOS-022`)
    assert_eq!(
        check_window_ssd_hit(&windows, 300.0, 101.0),
        Some(SsdHit::Resize(
            win2,
            ResizeEdges {
                top: true,
                bottom: false,
                left: false,
                right: false,
            }
        ))
    );
    assert_eq!(
        check_window_ssd_hit(&windows, 101.0, 300.0),
        Some(SsdHit::Resize(
            win2,
            ResizeEdges {
                top: false,
                bottom: false,
                left: true,
                right: false,
            }
        ))
    );
    assert_eq!(
        check_window_ssd_hit(&windows, 599.0, 499.0),
        Some(SsdHit::Resize(
            win2,
            ResizeEdges {
                top: false,
                bottom: true,
                left: false,
                right: true,
            }
        ))
    );
}

#[test]
fn generate_window_ssd_elements_creates_titlebar_traffic_lights_and_borders() {
    let elements = generate_window_ssd_elements(rusty_wm::Rect::new(40, 40, 600, 400), true, false);
    // 4 border strips + titlebar bg + divider + 3 traffic lights + title pill = 10 elements
    assert_eq!(elements.len(), 10);
}

#[test]
fn dock_and_overview_hit_testing_match_rendered_slots_and_cards() {
    let size = Size::from((1280, 800));
    let win_count = 2;

    for (slot_idx, expected) in [
        (0, DockHit::PinnedFiles),
        (1, DockHit::ToggleOverview),
        (2, DockHit::PinnedTerminal),
        (3, DockHit::PinnedBrowser),
        (4, DockHit::PinnedNotes),
        (5, DockHit::PinnedSettings),
        (6, DockHit::WindowSlot(0)),
        (7, DockHit::WindowSlot(1)),
        (8, DockHit::PinnedDownloads),
        (9, DockHit::PinnedTrash),
    ] {
        let slot_rect = dock_slot_rect(size.w, size.h, win_count, slot_idx)
            .unwrap_or_else(|| Rectangle::from_size(Size::from((0, 0))));
        let cx = f64::from(slot_rect.loc.x + slot_rect.size.w / 2);
        let cy = f64::from(slot_rect.loc.y + slot_rect.size.h / 2);
        assert_eq!(check_dock_slot(size, win_count, cx, cy), Some(slot_idx));
        assert_eq!(check_dock_click(size, win_count, cx, cy), Some(expected));
    }

    // Above Dock returns None
    assert_eq!(check_dock_click(size, win_count, 640.0, 200.0), None);

    // Overview cards
    let cards = overview_card_rects(size, 3);
    assert_eq!(cards.len(), 3);
    for (idx, card) in cards.iter().enumerate() {
        let cx = f64::from(card.loc.x + card.size.w / 2);
        let cy = f64::from(card.loc.y + card.size.h / 2);
        assert_eq!(
            check_overview_click(size, 3, cx, cy),
            OverviewHit::Card(idx)
        );
    }
    assert_eq!(
        check_overview_click(size, 3, 10.0, 100.0),
        OverviewHit::Dismiss
    );
    assert_eq!(make_window_badge("RustyOS_Main : bash — Konsole"), "RM");
    assert_eq!(make_window_badge("Cursor Agents"), "CA");
    assert_eq!(extract_app_name("RustyOS_Main : bash — Konsole"), "Konsole");
    assert_eq!(
        extract_app_name("Document.txt — Text Editor"),
        "Text Editor"
    );
    assert_eq!(
        extract_app_name("Inbox - Mozilla Thunderbird"),
        "Mozilla Thund…"
    );
    assert_eq!(extract_app_name("Alacritty"), "Alacritty");
    assert_eq!(extract_app_name(""), "RustyOS");
}

#[test]
fn notification_toast_and_badge_hit_testing_match_rendered_bounds() {
    let size = Size::from((1280, 800));
    let rects = notification_toast_rects(size, 6);
    // Capped at MAX_VISIBLE_NOTIFICATIONS (4)
    assert_eq!(rects.len(), MAX_VISIBLE_NOTIFICATIONS);

    for (idx, rect) in rects.iter().enumerate() {
        let cx = f64::from(rect.loc.x + rect.size.w / 2);
        let cy = f64::from(rect.loc.y + rect.size.h / 2);
        assert_eq!(check_notification_click(size, 4, cx, cy), Some(idx));
    }

    // Outside toasts returns None
    assert_eq!(check_notification_click(size, 4, 100.0, 200.0), None);
    assert_eq!(check_notification_click(size, 0, 1100.0, 60.0), None);

    // macOS 27 TopBar hides any legacy `N:k` counter pill; Clock opens Notification Center.
    let (bx, by, bw, bh) = notification_badge_rect(size.w);
    assert_eq!(bw, 0);
    assert!(!check_top_bar_notification_click(
        size.w,
        2,
        f64::from(bx),
        f64::from(by + bh / 2)
    ));
    assert!(!check_top_bar_notification_click(
        size.w,
        0,
        f64::from(bx),
        f64::from(by + bh / 2)
    ));

    // TopBar keyboard layout pill (`EN`/`RU`) stays anchored consistently regardless of alerts
    let (kbd_x, kbd_y, kbd_w, kbd_h) = kbd_badge_rect(size.w, 0);
    assert!(check_top_bar_kbd_click(
        size.w,
        0,
        f64::from(kbd_x + kbd_w / 2),
        f64::from(kbd_y + kbd_h / 2)
    ));
    let (notif_kbd_x, notif_kbd_y, notif_kbd_w, notif_kbd_h) = kbd_badge_rect(size.w, 2);
    assert_eq!(notif_kbd_x, kbd_x);
    assert!(notif_kbd_x + notif_kbd_w <= bx);
    assert!(check_top_bar_kbd_click(
        size.w,
        2,
        f64::from(notif_kbd_x + notif_kbd_w / 2),
        f64::from(notif_kbd_y + notif_kbd_h / 2)
    ));

    // TopBar system tray pills (`RUSTYOS-032`) sit cleanly left of keyboard badge
    let tray_rects = tray_slot_rects(size.w, 2, 3);
    assert_eq!(tray_rects.len(), 3);
    assert!(tray_rects[2].0 + tray_rects[2].2 <= notif_kbd_x);
    for (idx, (tx, ty, tw, th)) in tray_rects.into_iter().enumerate() {
        assert_eq!(
            check_top_bar_tray_click(size.w, 2, 3, f64::from(tx + tw / 2), f64::from(ty + th / 2)),
            Some(idx)
        );
    }
    assert_eq!(check_top_bar_tray_click(size.w, 2, 3, 20.0, 14.0), None);
}

#[test]
fn lock_screen_hit_testing_and_animation_offsets_match_bounds() {
    let size = Size::from((1280, 800));
    let single_users = [rusty_services::UserAccount {
        username: "anc".to_owned(),
        display_name: "anc".to_owned(),
        uid: 1000,
        avatar_path: None,
    }];
    let single_layout = compute_lock_screen_layout(size, "EN", true, &["anc"], 0, false);
    let single_state = LockScreenRenderState {
        date_label: "Thu Sep 10",
        clock_hhmm: "8:52",
        keyboard_layout: "EN",
        has_battery: true,
        battery_percent: 100,
        battery_charging: true,
        wifi_connected: true,
        wifi_signal_percent: Some(90),
        wallpaper_path: None,
        users: &single_users,
        selected_user: 0,
        power_menu_open: false,
        require_password: false,
        password_len: 0,
        auth_checking: false,
        auth_error: None,
        glass_opacity: 0.78,
        reduce_transparency: false,
        progress: 1.0,
    };
    let row0 = single_layout.user_row_rects[0];
    assert_eq!(
        hit_test_lock_screen(
            size,
            &single_state,
            f64::from(row0.loc.x + row0.size.w / 2),
            f64::from(row0.loc.y + row0.size.h / 2)
        ),
        LockScreenHit::UnlockUser(0)
    );
    assert!(check_lock_screen_unlock_click(
        size,
        f64::from(row0.loc.x + row0.size.w / 2),
        f64::from(row0.loc.y + row0.size.h / 2)
    ));
    assert!(!check_lock_screen_unlock_click(size, 40.0, 40.0));
    assert_eq!(
        hit_test_lock_screen(
            size,
            &single_state,
            f64::from(single_layout.kbd_hit_rect.loc.x + 6),
            f64::from(single_layout.kbd_hit_rect.loc.y + 8)
        ),
        LockScreenHit::ToggleKeyboardLayout
    );
    assert_eq!(
        hit_test_lock_screen(
            size,
            &single_state,
            f64::from(single_layout.power_btn_rect.loc.x + 8),
            f64::from(single_layout.power_btn_rect.loc.y + 8)
        ),
        LockScreenHit::TogglePowerMenu
    );
    assert_eq!(anim_offset(1.0, -22.0), 0);
    assert_eq!(anim_offset(0.0, -22.0), -22);
}

#[test]
fn lock_screen_power_menu_and_multi_user_hit_testing() {
    let size = Size::from((1280, 800));
    let multi_users = [
        rusty_services::UserAccount {
            username: "anc".to_owned(),
            display_name: "anc".to_owned(),
            uid: 1000,
            avatar_path: None,
        },
        rusty_services::UserAccount {
            username: "stephen".to_owned(),
            display_name: "Stephen Hackett".to_owned(),
            uid: 1001,
            avatar_path: None,
        },
    ];
    let multi_layout =
        compute_lock_screen_layout(size, "RU", true, &["anc", "Stephen Hackett"], 0, false);
    let mut multi_state = LockScreenRenderState {
        date_label: "Thu Sep 10",
        clock_hhmm: "8:52",
        keyboard_layout: "RU",
        has_battery: true,
        battery_percent: 100,
        battery_charging: true,
        wifi_connected: true,
        wifi_signal_percent: Some(90),
        wallpaper_path: None,
        users: &multi_users,
        selected_user: 0,
        power_menu_open: false,
        require_password: false,
        password_len: 0,
        auth_checking: false,
        auth_error: None,
        glass_opacity: 0.78,
        reduce_transparency: false,
        progress: 1.0,
    };
    let row1 = multi_layout.user_row_rects[1];
    assert_eq!(
        hit_test_lock_screen(
            size,
            &multi_state,
            f64::from(row1.loc.x + 12),
            f64::from(row1.loc.y + 12)
        ),
        LockScreenHit::SelectUser(1)
    );
    multi_state.selected_user = 1;
    assert_eq!(
        hit_test_lock_screen(
            size,
            &multi_state,
            f64::from(row1.loc.x + 12),
            f64::from(row1.loc.y + 12)
        ),
        LockScreenHit::UnlockUser(1)
    );

    multi_state.power_menu_open = true;
    let sleep_item = multi_layout.power_item_rects[0];
    let shutdown_item = multi_layout.power_item_rects[2];
    assert_eq!(
        hit_test_lock_screen(
            size,
            &multi_state,
            f64::from(sleep_item.loc.x + 20),
            f64::from(sleep_item.loc.y + 10)
        ),
        LockScreenHit::PowerAction(rusty_services::SessionAction::Suspend)
    );
    assert_eq!(
        hit_test_lock_screen(
            size,
            &multi_state,
            f64::from(shutdown_item.loc.x + 20),
            f64::from(shutdown_item.loc.y + 10)
        ),
        LockScreenHit::PowerAction(rusty_services::SessionAction::PowerOff)
    );
    assert_eq!(
        hit_test_lock_screen(size, &multi_state, 40.0, 40.0),
        LockScreenHit::DismissPowerMenu
    );

    assert_eq!(anim_offset(1.0, -22.0), 0);
    assert_eq!(anim_offset(0.0, -22.0), -22);
}

#[test]
fn csd_frame_hit_testing_ignores_client_header_and_preserves_outer_resize() {
    let win = SsdHitWindow {
        id: rusty_wm::WindowId(42),
        rect: rusty_wm::Rect::new(100, 80, 600, 420),
        has_ssd: false,
    };
    let list = [win];

    // Interior header spots (macOS traffic-light zone, titlebar interior, corner close button)
    // must pass through to the CSD application.
    assert_eq!(check_window_frame_hit(&list, 118.0, 94.0), None);
    assert_eq!(check_window_frame_hit(&list, 350.0, 94.0), None);
    assert_eq!(check_window_frame_hit(&list, 692.0, 94.0), None);

    // Outer border band outside the client surface still triggers resize.
    assert_eq!(
        check_window_frame_hit(&list, 97.0, 200.0),
        Some(SsdHit::Resize(
            rusty_wm::WindowId(42),
            ResizeEdges {
                left: true,
                right: false,
                top: false,
                bottom: false,
            }
        ))
    );
}

#[test]
fn osd_pill_bounds_center_above_dock() {
    let out = Size::from((1280, 800));
    let bar_pill = osd_pill_rect(out, true);
    let msg_pill = osd_pill_rect(out, false);
    assert_eq!(bar_pill.size.w, 256);
    assert_eq!(bar_pill.size.h, 56);
    assert_eq!(msg_pill.size.h, 40);
    assert_eq!(bar_pill.loc.x, (1280 - 256) / 2);
    assert!(bar_pill.loc.y + bar_pill.size.h < 800 - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM);
}

#[test]
fn switcher_layout_and_click_hit_testing_match_bounds() {
    let out = Size::from((1280, 800));
    let (panel, cards) = switcher_layout(out, 3);
    assert_eq!(cards.len(), 3);
    assert!(panel.size.w > cards[2].size.w * 3);
    assert_eq!(panel.loc.x + panel.size.w / 2, 640);
    for (idx, rect) in cards.iter().enumerate() {
        assert_eq!(
            check_switcher_click(
                out,
                3,
                f64::from(rect.loc.x + rect.size.w / 2),
                f64::from(rect.loc.y + rect.size.h / 2)
            ),
            Some(idx)
        );
    }
    assert_eq!(check_switcher_click(out, 3, 20.0, 20.0), None);
}

#[test]
fn macos27_quick_settings_glass_and_volume_slider_hit_testing() {
    let size = Size::from((1280, 800));
    let layout = quick_settings_layout(size.w, size.h, 0);
    let sound_x = f64::from(layout.sound_track.loc.x);
    let sound_w = f64::from(layout.sound_track.size.w);
    let sound_y = f64::from(layout.sound_track.loc.y + layout.sound_track.size.h / 2);
    let disp_x = f64::from(layout.display_track.loc.x);
    let disp_w = f64::from(layout.display_track.size.w);
    let disp_y = f64::from(layout.display_track.loc.y + layout.display_track.size.h / 2);

    assert_eq!(
        check_quick_settings_click(size, sound_x + sound_w * 0.5, sound_y),
        QuickSettingsHit::SetVolumePercent(50)
    );
    assert_eq!(
        check_quick_settings_slider_drag(size, sound_x + sound_w * 0.5, sound_y),
        Some(QuickSettingsSliderDrag::Sound)
    );
    assert_eq!(
        quick_settings_slider_percent(size, QuickSettingsSliderDrag::Sound, sound_x),
        0
    );
    assert_eq!(
        check_quick_settings_click(
            size,
            f64::from(layout.transparency_btn.loc.x + 24),
            f64::from(layout.transparency_btn.loc.y + 16)
        ),
        QuickSettingsHit::ToggleReduceTransparency
    );
    assert_eq!(
        check_quick_settings_click(size, disp_x + 1.0, disp_y),
        QuickSettingsHit::SetDisplayPercent(0)
    );
    assert_eq!(
        check_quick_settings_slider_drag(size, disp_x + 40.0, disp_y),
        Some(QuickSettingsSliderDrag::Display)
    );
    assert_eq!(
        check_quick_settings_click(size, disp_x + disp_w - 1.0, disp_y),
        QuickSettingsHit::SetDisplayPercent(100)
    );
    assert_eq!(
        quick_settings_slider_percent(
            size,
            QuickSettingsSliderDrag::Display,
            disp_x + disp_w * 0.5
        ),
        50
    );

    let tinted = scale_glass_rgba([20, 14, 34, 200], 0.50, false);
    assert!(tinted[3] < 200 && tinted[3] >= 40);
    let opaque = scale_glass_rgba([20, 14, 34, 180], 0.40, true);
    assert!(opaque[3] >= 246);
    assert_eq!(layout.display_card.size.h, 66);
    assert_eq!(layout.sound_card.size.h, 66);
    assert_eq!(layout.layout_btn.size.w, layout.layout_btn.size.h);
    assert_eq!(
        layout.now_playing_card.size.w,
        layout.now_playing_card.size.h
    );
    assert_eq!(layout.wifi_pill.size.h, layout.bt_pill.size.h);
    assert_eq!(layout.power_pill.size.h, layout.focus_pill.size.h);
}

#[test]
#[allow(clippy::too_many_lines)]
fn macos27_top_bar_dynamic_left_right_and_menu_popover_hit_testing() {
    let without_bat = compute_top_bar_right_layout(1280, false, 0, "Wed Apr 1  9:41 AM");
    let with_bat = compute_top_bar_right_layout(1280, true, 0, "Wed Apr 1  9:41 AM");
    assert!(without_bat.battery_x.is_none());
    assert!(without_bat.volume_x.is_none());
    assert!(without_bat.kbd_x.is_none());
    assert!(with_bat.battery_x.is_some());
    let bat_x = with_bat.battery_x.unwrap_or(i32::MAX);
    // Exact macOS right order: Battery · Wi-Fi · Search · Control Center · Clock
    assert!(bat_x < with_bat.wifi_x);
    assert!(with_bat.wifi_x < with_bat.search_x);
    assert!(with_bat.search_x < with_bat.control_center_x);
    assert!(with_bat.control_center_x < with_bat.clock_x);

    let all_opts = compute_top_bar_right_layout_with_options(
        1280,
        "Wed Apr 1  9:41 AM",
        TopBarLayoutOptions {
            has_battery: true,
            notification_count: 0,
            show_volume: true,
            show_input_source: true,
            show_workspaces: true,
            show_tray: true,
            show_updates: true,
            ..TopBarLayoutOptions::default()
        },
    );
    assert!(all_opts.volume_x.is_some());
    assert!(all_opts.kbd_x.is_some());
    assert!(all_opts.updates_x.is_some());
    assert!(all_opts.kbd_x.unwrap_or(0) < all_opts.updates_x.unwrap_or(0));
    assert!(all_opts.updates_x.unwrap_or(0) < all_opts.volume_x.unwrap_or(0));
    assert!(all_opts.volume_x.unwrap_or(0) < all_opts.battery_x.unwrap_or(0));

    assert!(check_top_bar_search_click_for_options(
        1280,
        "Wed Apr 1  9:41 AM",
        TopBarLayoutOptions::default(),
        f64::from(without_bat.search_x + without_bat.search_w / 2),
        12.0,
    ));

    let short_left = compute_top_bar_left_layout_with_options("RustyOS", true);
    let wide_left = compute_top_bar_left_layout_with_options("Visual Studio Code", true);
    assert!(wide_left.app_w > short_left.app_w);
    assert!(wide_left.help_x > short_left.help_x);
    assert!(wide_left.ws_start_x > short_left.ws_start_x);
    assert_eq!(
        check_top_bar_workspace_click_with_options(
            "Visual Studio Code",
            true,
            f64::from(wide_left.ws_start_x + 4),
            12.0
        ),
        Some(1)
    );
    assert!(check_top_bar_layout_click_for_app(
        "Visual Studio Code",
        f64::from(wide_left.layout_x + 10),
        12.0
    ));

    let (mx, my, _mw, _mh) = top_bar_menu_popover_rect("Files", TopBarMenuHit::File);
    assert_eq!(
        check_top_bar_menu_popover_click(
            "Files",
            TopBarMenuHit::File,
            f64::from(mx + 24),
            f64::from(my + 16),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::OpenFiles)
    );
    assert_eq!(
        check_top_bar_menu_popover_click(
            "Files",
            TopBarMenuHit::File,
            f64::from(mx + 24),
            f64::from(my + 44),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::NewDesktopFolder)
    );
    let (gx, gy, _gw, _gh) = top_bar_menu_popover_rect("Files", TopBarMenuHit::Go);
    assert_eq!(
        check_top_bar_menu_popover_click(
            "Files",
            TopBarMenuHit::Go,
            f64::from(gx + 24),
            f64::from(gy + 16),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::GoRecents)
    );
    assert_eq!(
        check_top_bar_menu_popover_click(
            "Files",
            TopBarMenuHit::Go,
            f64::from(gx + 24),
            f64::from(gy + 44),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::GoDesktop)
    );
    assert_eq!(
        check_top_bar_menu_popover_click("Files", TopBarMenuHit::File, 800.0, 400.0),
        TopBarMenuPopoverClick::Outside
    );

    let sys_items = top_bar_menu_items(TopBarMenuHit::SystemLogo);
    assert!(
        sys_items
            .iter()
            .any(|(l, a)| (*l == "Suspend" || *l == "Sleep") && *a == TopBarMenuAction::Sleep)
    );
    assert!(
        sys_items
            .iter()
            .any(|(l, a)| *l == "Restart..." && *a == TopBarMenuAction::Restart)
    );
    assert!(
        sys_items
            .iter()
            .any(|(l, a)| *l == "Shut Down..." && *a == TopBarMenuAction::ShutDown)
    );
    assert!(
        sys_items
            .iter()
            .any(|(l, a)| *l == "Log Out..." && *a == TopBarMenuAction::LogOut)
    );
}

#[test]
fn macos_dock_divider_gap_tooltip_and_radii_constants() {
    assert_eq!(DOCK_HEIGHT, 68);
    assert_eq!(DOCK_RADIUS, 18);
    assert_eq!(DOCK_ICON_SIZE, 50);
    assert_eq!(DOCK_ITEM_GAP, 6);
    assert_eq!(WINDOW_RADIUS, 14);
    let last_app_slot_opt = dock_slot_rect(1280, 800, 2, 7);
    let downloads_slot_opt = dock_slot_rect(1280, 800, 2, 8);
    assert!(last_app_slot_opt.is_some());
    assert!(downloads_slot_opt.is_some());
    let last_app_slot =
        last_app_slot_opt.unwrap_or_else(|| Rectangle::from_size(Size::from((0, 0))));
    let downloads_slot =
        downloads_slot_opt.unwrap_or_else(|| Rectangle::from_size(Size::from((0, 0))));
    assert_eq!(
        downloads_slot.loc.x - (last_app_slot.loc.x + last_app_slot.size.w),
        DOCK_ITEM_GAP + DOCK_DIVIDER_GAP
    );

    let dock_view = DockViewState {
        pinned_icons: [None, None],
        windows: &[],
        hover_slot: Some(0),
        overview_active: false,
        hover_progress: 1.0,
        reduced_motion: false,
        ..DockViewState::default()
    };
    let tooltip_opt = dock_tooltip_rect_and_label(1280, 800, &dock_view);
    assert!(tooltip_opt.is_some());
    if let Some((pill_rect, label)) = tooltip_opt {
        assert_eq!(label, "Files");
        assert!(pill_rect.loc.y + pill_rect.size.h < 800 - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM);
    }
}

#[test]
fn macos27_menu_entries_and_overview_spaces_hit_testing() {
    // 1. Rich menu entries have separators, leading SDF icons, shortcuts & accurate hover rows
    let win_entries = top_bar_menu_entries(TopBarMenuHit::Window);
    assert!(
        win_entries
            .iter()
            .any(|e| matches!(e, TopBarMenuEntry::Separator))
    );
    let (wx, wy, _mw, _mh) = top_bar_menu_popover_rect("RustyOS", TopBarMenuHit::Window);
    assert_eq!(
        check_top_bar_menu_popover_hover_row(
            "RustyOS",
            TopBarMenuHit::Window,
            f64::from(wx + 24),
            f64::from(wy + MENU_POPOVER_PAD_Y + 10)
        ),
        Some(0)
    );
    // Separator gap does not trigger hover highlight
    assert_eq!(
        check_top_bar_menu_popover_hover_row(
            "RustyOS",
            TopBarMenuHit::Window,
            f64::from(wx + 24),
            f64::from(wy + MENU_POPOVER_PAD_Y + MENU_ITEM_HEIGHT + 4)
        ),
        None
    );

    // 2. Overview Spaces strip hit-testing switches workspace cleanly
    let size = Size::from((1280, 800));
    let spaces = [
        OverviewWorkspaceSpace {
            id: 1,
            window_count: 1,
            active: true,
            preview_rects: [
                (0.1, 0.1, 0.4, 0.4),
                (0.0, 0.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 0.0),
            ],
        },
        OverviewWorkspaceSpace {
            id: 2,
            window_count: 2,
            active: false,
            preview_rects: [
                (0.1, 0.1, 0.35, 0.7),
                (0.55, 0.1, 0.35, 0.7),
                (0.0, 0.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 0.0),
            ],
        },
    ];
    let space_rects = overview_space_rects(size, spaces.len());
    assert_eq!(space_rects.len(), 2);
    let s2 = space_rects[1];
    assert_eq!(
        check_overview_click_with_spaces(
            size,
            &spaces,
            1,
            f64::from(s2.loc.x + s2.size.w / 2),
            f64::from(s2.loc.y + s2.size.h / 2)
        ),
        OverviewHit::SwitchWorkspace(2)
    );
}

#[test]
fn macos27_traffic_hover_notifications_and_osd_routing() {
    let size = Size::from((1280, 800));

    // 3. SSD traffic-light hover helper respects stacking & CSD windows
    let ssd_win = SsdHitWindow {
        id: rusty_wm::WindowId(10),
        rect: rusty_wm::Rect::new(100, 80, 500, 360),
        has_ssd: true,
    };
    let csd_win = SsdHitWindow {
        id: rusty_wm::WindowId(11),
        rect: rusty_wm::Rect::new(200, 120, 400, 300),
        has_ssd: false,
    };
    assert_eq!(
        check_window_traffic_lights_hover(&[ssd_win], 121.0, 94.0),
        Some(rusty_wm::WindowId(10))
    );
    assert_eq!(
        check_window_traffic_lights_hover(&[ssd_win], 300.0, 92.0),
        None
    );
    assert_eq!(
        check_window_traffic_lights_hover(&[csd_win], 216.0, 132.0),
        None
    );

    // 4. Notifications (344 px Golden Gate card, squircle icon choice) & OSD SDF glyph routing
    let notif_rects = notification_toast_rects(size, 1);
    assert_eq!(notif_rects[0].size.w, 344);
    assert_eq!(
        notification_app_icon("Foot Terminal"),
        crate::icons::BuiltinAppIcon::Terminal
    );
    assert_eq!(
        notification_app_icon("System Update"),
        crate::icons::BuiltinAppIcon::Updater
    );
    assert_eq!(
        notification_app_icon("Display Connected"),
        crate::icons::BuiltinAppIcon::RustySystem
    );
    assert_eq!(
        osd_glyph_kind("Brightness", Some(70), false),
        crate::surfaces::UiGlyphKind::Sun
    );
    assert_eq!(
        osd_glyph_kind("Volume", Some(45), false),
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: 45,
            muted: false
        }
    );
    assert_eq!(
        osd_glyph_kind("Screenshot Saved", None, false),
        crate::surfaces::UiGlyphKind::Info
    );
}

#[test]
fn lock_screen_password_pill_and_polkit_modal_hit_testing() {
    let size = Size::from((1280, 800));
    let layout = compute_lock_screen_layout(size, "EN", true, &["anc"], 0, true);
    assert!(layout.password_submit_rect.is_some());
    assert!(layout.password_pill_rect.is_some());
    if let (Some(submit), Some(pill)) = (layout.password_submit_rect, layout.password_pill_rect) {
        let lock_state = LockScreenRenderState {
            date_label: "Thu Sep 10",
            clock_hhmm: "8:52",
            keyboard_layout: "EN",
            has_battery: true,
            battery_percent: 100,
            battery_charging: false,
            wifi_connected: true,
            wifi_signal_percent: Some(88),
            wallpaper_path: None,
            users: &[],
            selected_user: 0,
            power_menu_open: false,
            require_password: true,
            password_len: 5,
            auth_checking: false,
            auth_error: None,
            glass_opacity: 0.78,
            reduce_transparency: false,
            progress: 1.0,
        };
        assert_eq!(
            hit_test_lock_screen(
                size,
                &lock_state,
                f64::from(submit.loc.x + submit.size.w / 2),
                f64::from(submit.loc.y + submit.size.h / 2)
            ),
            LockScreenHit::SubmitPassword
        );
        assert_eq!(
            hit_test_lock_screen(
                size,
                &lock_state,
                f64::from(pill.loc.x + 24),
                f64::from(pill.loc.y + 12)
            ),
            LockScreenHit::None
        );
    }

    let pk_layout = polkit_modal_layout(size);
    assert_eq!(
        hit_test_polkit_dialog(
            size,
            f64::from(pk_layout.submit.loc.x + 20),
            f64::from(pk_layout.submit.loc.y + 14)
        ),
        PolkitHit::Submit
    );
    assert_eq!(
        hit_test_polkit_dialog(
            size,
            f64::from(pk_layout.cancel.loc.x + 20),
            f64::from(pk_layout.cancel.loc.y + 14)
        ),
        PolkitHit::Cancel
    );
    assert_eq!(
        hit_test_polkit_dialog(
            size,
            f64::from(pk_layout.input.loc.x + 30),
            f64::from(pk_layout.input.loc.y + 14)
        ),
        PolkitHit::PasswordField
    );
    assert_eq!(hit_test_polkit_dialog(size, 20.0, 20.0), PolkitHit::Outside);
}

#[test]
fn portal_modal_hit_testing() {
    let size = Size::from((1280, 800));
    let portal_layout = portal_modal_layout(size, 3, 1, true);
    assert_eq!(
        hit_test_portal_dialog(
            size,
            3,
            1,
            true,
            f64::from(portal_layout.row_rects[1].loc.x + 20),
            f64::from(portal_layout.row_rects[1].loc.y + 14),
        ),
        PortalHit::Row(1)
    );
    if let Some(toggle) = portal_layout.cursor_toggle {
        assert_eq!(
            hit_test_portal_dialog(
                size,
                3,
                1,
                true,
                f64::from(toggle.loc.x + 20),
                f64::from(toggle.loc.y + 10),
            ),
            PortalHit::CursorToggle
        );
    }
    assert_eq!(
        hit_test_portal_dialog(
            size,
            3,
            1,
            true,
            f64::from(portal_layout.accept.loc.x + 20),
            f64::from(portal_layout.accept.loc.y + 14),
        ),
        PortalHit::Accept
    );
    assert_eq!(
        hit_test_portal_dialog(
            size,
            3,
            1,
            true,
            f64::from(portal_layout.cancel.loc.x + 20),
            f64::from(portal_layout.cancel.loc.y + 14),
        ),
        PortalHit::Cancel
    );
    assert_eq!(
        hit_test_portal_dialog(size, 3, 1, true, 20.0, 20.0),
        PortalHit::Outside
    );
}

#[test]
fn overlay_and_chrome_visibility_predicates_isolate_modals() {
    let pk = PolkitPromptRenderState {
        message: "Auth",
        action_id: "org.freedesktop.policykit.exec",
        identity_user: "root",
        prompt_label: "Password",
        input_display: "",
        checking: false,
        error_message: None,
        info_message: None,
    };
    let portal = PortalPromptRenderState {
        app_id: "app",
        is_screencast: false,
        include_cursor: false,
        cursor_mode_editable: false,
        rows: &[],
        selected: 0,
    };
    let sw = WindowSwitcherRenderState {
        cards: &[],
        selected: 0,
    };
    let ov = OverviewRenderState {
        active_workspace: 1,
        layout_mode: "floating",
        windows: &[],
        spaces: &[],
        progress: 1.0,
        filter_label: None,
    };

    assert!(!notifications_hidden_by_overlay(ShellOverlayView::None));
    assert!(!notifications_hidden_by_overlay(
        ShellOverlayView::Overview(&ov)
    ));
    assert!(!notifications_hidden_by_overlay(
        ShellOverlayView::PickColor("app")
    ));
    assert!(notifications_hidden_by_overlay(ShellOverlayView::Polkit(
        &pk
    )));
    assert!(notifications_hidden_by_overlay(ShellOverlayView::Portal(
        &portal
    )));

    assert!(!dock_tooltip_hidden_by_overlay(ShellOverlayView::None));
    assert!(!dock_tooltip_hidden_by_overlay(
        ShellOverlayView::PickColor("app")
    ));
    assert!(dock_tooltip_hidden_by_overlay(ShellOverlayView::Switcher(
        &sw
    )));
    assert!(dock_tooltip_hidden_by_overlay(ShellOverlayView::Polkit(
        &pk
    )));
    assert!(dock_tooltip_hidden_by_overlay(ShellOverlayView::Portal(
        &portal
    )));
}

#[test]
fn context_menu_clamps_within_primary_and_secondary_outputs() {
    use crate::surfaces::UiGlyphKind;
    use rusty_wm::OutputId;

    let entries = vec![
        ContextMenuEntry::item(
            "New Folder",
            UiGlyphKind::Plus,
            ContextMenuAction::DesktopNewFolder,
        ),
        ContextMenuEntry::Separator,
        ContextMenuEntry::item(
            "Display Settings...",
            UiGlyphKind::ControlCenter,
            ContextMenuAction::DesktopDisplaySettings,
        ),
    ];

    let primary_size = Size::from((1280, 800));
    let mut bottom_right = ContextMenuState::new(
        OutputId(1),
        Point::from((1270, 795)),
        ContextMenuTarget::Desktop,
        entries.clone(),
    );
    let br_layout = compute_context_menu_layout(primary_size, &bottom_right, true);
    assert!(br_layout.primary_rect.loc.x + br_layout.primary_rect.size.w <= primary_size.w - 8);
    assert!(br_layout.primary_rect.loc.y + br_layout.primary_rect.size.h <= primary_size.h - 8);
    assert!(br_layout.primary_rect.loc.y >= TOP_BAR_HEIGHT + 6);

    bottom_right.anchor_local = Point::from((2, 4));
    let top_left_primary = compute_context_menu_layout(primary_size, &bottom_right, true);
    assert_eq!(top_left_primary.primary_rect.loc.y, TOP_BAR_HEIGHT + 6);
    assert!(top_left_primary.primary_rect.loc.x >= 8);

    let secondary_size = Size::from((2560, 1440));
    let top_left_secondary = ContextMenuState::new(
        OutputId(2),
        Point::from((2, 4)),
        ContextMenuTarget::Desktop,
        entries,
    );
    let sec_layout = compute_context_menu_layout(secondary_size, &top_left_secondary, false);
    assert_eq!(sec_layout.primary_rect.loc.y, 8);
    assert_eq!(sec_layout.primary_rect.loc.x, 8);
}

#[test]
fn context_menu_cascade_submenu_hover_flip_and_hit_testing() -> Result<(), &'static str> {
    use crate::surfaces::UiGlyphKind;
    use rusty_wm::OutputId;

    let sub_items = vec![
        ContextMenuEntry::item(
            "Desktop 1",
            UiGlyphKind::OverviewSpaces,
            ContextMenuAction::SwitchWorkspace(1),
        )
        .with_checked(true),
        ContextMenuEntry::item(
            "Desktop 2",
            UiGlyphKind::OverviewSpaces,
            ContextMenuAction::SwitchWorkspace(2),
        ),
    ];
    let entries = vec![
        ContextMenuEntry::submenu_item("Move to Desktop", UiGlyphKind::OverviewSpaces, sub_items),
        ContextMenuEntry::Separator,
        ContextMenuEntry::item(
            "Disabled Item",
            UiGlyphKind::Info,
            ContextMenuAction::OpenSubmenu,
        )
        .with_enabled(false),
    ];
    let primary_size = Size::from((1280, 800));
    let mut state = ContextMenuState::new(
        OutputId(1),
        Point::from((1250, 120)),
        ContextMenuTarget::Desktop,
        entries,
    );
    let initial = compute_context_menu_layout(primary_size, &state, true);
    let row0 = initial.primary_rows[0];
    let hover_x = f64::from(row0.loc.x + row0.size.w / 2);
    let hover_y = f64::from(row0.loc.y + row0.size.h / 2);
    assert!(update_context_menu_hover(
        primary_size,
        &mut state,
        true,
        hover_x,
        hover_y
    ));
    assert_eq!(state.open_submenu_parent, Some(0));
    assert_eq!(
        hit_test_context_menu(primary_size, &state, true, hover_x, hover_y),
        ContextMenuHit::ExpandSubmenu(0)
    );

    let with_sub = compute_context_menu_layout(primary_size, &state, true);
    let (sub_rect, sub_rows) = with_sub.submenu.ok_or("expected expanded submenu layout")?;
    assert!(
        sub_rect.loc.x + sub_rect.size.w
            <= with_sub.primary_rect.loc.x + rusty_ui::menu::SUBMENU_OVERLAP
    );
    let child2 = sub_rows[1];
    assert_eq!(
        hit_test_context_menu(
            primary_size,
            &state,
            true,
            f64::from(child2.loc.x + 12),
            f64::from(child2.loc.y + child2.size.h / 2),
        ),
        ContextMenuHit::Action(ContextMenuAction::SwitchWorkspace(2))
    );
    assert_eq!(
        hit_test_context_menu(primary_size, &state, true, 12.0, 780.0),
        ContextMenuHit::Outside
    );
    Ok(())
}

#[test]
fn context_menu_keyboard_navigation_skips_separators_and_handles_submenus() {
    use crate::surfaces::UiGlyphKind;
    use rusty_wm::OutputId;
    use smithay::input::keyboard::Keysym;

    let sub_items = vec![
        ContextMenuEntry::item(
            "Name",
            UiGlyphKind::LayoutSplit,
            ContextMenuAction::DesktopToggleLayoutMode,
        ),
        ContextMenuEntry::item(
            "Size",
            UiGlyphKind::LayoutSplit,
            ContextMenuAction::DesktopDisplaySettings,
        ),
    ];
    let entries = vec![
        ContextMenuEntry::submenu_item("Options", UiGlyphKind::ControlCenter, sub_items),
        ContextMenuEntry::Separator,
        ContextMenuEntry::item(
            "Disabled",
            UiGlyphKind::Info,
            ContextMenuAction::OpenSubmenu,
        )
        .with_enabled(false),
        ContextMenuEntry::item(
            "Display Settings...",
            UiGlyphKind::ControlCenter,
            ContextMenuAction::DesktopDisplaySettings,
        ),
    ];
    let mut state = ContextMenuState::new(
        OutputId(1),
        Point::from((100, 100)),
        ContextMenuTarget::Desktop,
        entries,
    );

    assert_eq!(
        handle_context_menu_key(&mut state, Keysym::Down),
        ContextMenuKeyOutcome::Redraw
    );
    assert_eq!(state.selected_row, Some(0));

    assert_eq!(
        handle_context_menu_key(&mut state, Keysym::Down),
        ContextMenuKeyOutcome::Redraw
    );
    assert_eq!(state.selected_row, Some(3));

    assert_eq!(
        handle_context_menu_key(&mut state, Keysym::Up),
        ContextMenuKeyOutcome::Redraw
    );
    assert_eq!(state.selected_row, Some(0));

    assert_eq!(
        handle_context_menu_key(&mut state, Keysym::Right),
        ContextMenuKeyOutcome::Redraw
    );
    assert_eq!(state.open_submenu_parent, Some(0));
    assert_eq!(state.submenu_selected_row, Some(0));

    assert_eq!(
        handle_context_menu_key(&mut state, Keysym::Down),
        ContextMenuKeyOutcome::Redraw
    );
    assert_eq!(state.submenu_selected_row, Some(1));

    assert_eq!(
        handle_context_menu_key(&mut state, Keysym::Return),
        ContextMenuKeyOutcome::Execute(ContextMenuAction::DesktopDisplaySettings)
    );
    assert_eq!(
        handle_context_menu_key(&mut state, Keysym::Escape),
        ContextMenuKeyOutcome::Redraw
    );
    assert_eq!(state.open_submenu_parent, None);
    assert_eq!(
        handle_context_menu_key(&mut state, Keysym::Escape),
        ContextMenuKeyOutcome::Close
    );
}

#[test]
fn app_aware_topbar_classification_and_layout_hides_go_for_non_files() {
    assert_eq!(
        classify_top_bar_app(None, ""),
        ("Desktop".to_owned(), TopBarAppProfile::Desktop)
    );
    assert_eq!(
        classify_top_bar_app(Some("org.rustyos.Desktop"), ""),
        ("Desktop".to_owned(), TopBarAppProfile::Desktop)
    );
    assert_eq!(
        check_top_bar_menu_click_for_profile(TopBarAppProfile::Desktop, "Desktop", 60.0, 12.0),
        Some(TopBarMenuHit::ActiveApp)
    );
    assert_eq!(
        classify_top_bar_app(Some("org.rustyos.Files"), "Downloads"),
        ("Files".to_owned(), TopBarAppProfile::Files)
    );
    assert_eq!(
        classify_top_bar_app(Some("firefox"), "YouTube — Mozilla Firefox"),
        ("Firefox".to_owned(), TopBarAppProfile::Browser)
    );
    assert_eq!(
        classify_top_bar_app(Some("org.mozilla.firefox"), "New Tab"),
        ("Firefox".to_owned(), TopBarAppProfile::Browser)
    );
    assert_eq!(
        classify_top_bar_app(Some("Navigator"), "Mozilla Firefox"),
        ("Firefox".to_owned(), TopBarAppProfile::Browser)
    );
    assert_eq!(
        classify_top_bar_app(Some("org.kde.konsole"), "anc@rustyos: ~"),
        ("Konsole".to_owned(), TopBarAppProfile::Terminal)
    );
    assert_eq!(
        classify_top_bar_app(Some("code"), "lib.rs - Visual Studio Code"),
        ("VS Code".to_owned(), TopBarAppProfile::Editor)
    );
    assert_eq!(
        classify_top_bar_app(Some("org.gimp.GIMP"), "Canvas - GIMP"),
        ("GIMP".to_owned(), TopBarAppProfile::GenericApp)
    );

    let files_left =
        compute_top_bar_left_layout_for_profile(TopBarAppProfile::Files, "Files", true);
    assert!(files_left.has_go);
    assert!(files_left.go_w > 0);
    assert_eq!(
        check_top_bar_menu_click_for_profile(
            TopBarAppProfile::Files,
            "Files",
            f64::from(files_left.go_x + files_left.go_w / 2),
            12.0,
        ),
        Some(TopBarMenuHit::Go)
    );

    let ff_left =
        compute_top_bar_left_layout_for_profile(TopBarAppProfile::Browser, "Firefox", true);
    assert!(!ff_left.has_go);
    assert_eq!(ff_left.go_w, 0);
    assert_eq!(ff_left.win_x, ff_left.view_x + ff_left.view_w);
    assert!(ff_left.ws_start_x < files_left.ws_start_x);
    assert_eq!(
        check_top_bar_menu_click_for_profile(
            TopBarAppProfile::Browser,
            "Firefox",
            f64::from(ff_left.win_x + 4),
            12.0,
        ),
        Some(TopBarMenuHit::Window)
    );
    assert_eq!(
        check_top_bar_workspace_click_for_profile(
            TopBarAppProfile::Browser,
            "Firefox",
            true,
            f64::from(ff_left.ws_start_x + 4),
            12.0,
        ),
        Some(1)
    );
    assert!(check_top_bar_layout_click_for_profile(
        TopBarAppProfile::Browser,
        "Firefox",
        true,
        f64::from(ff_left.layout_x + 8),
        12.0,
    ));
}

#[test]
fn app_aware_topbar_dropdowns_and_popover_hit_testing_match_profile() {
    let labels_of = |profile, app, menu| -> Vec<String> {
        resolve_top_bar_menu_entries(profile, app, menu)
            .into_iter()
            .filter_map(|e| match e {
                ResolvedTopBarMenuItem::Item { label, .. } => Some(label.into_owned()),
                ResolvedTopBarMenuItem::Separator => None,
            })
            .collect()
    };

    let ff_app_labels = labels_of(
        TopBarAppProfile::Browser,
        "Firefox",
        TopBarMenuHit::ActiveApp,
    );
    assert_eq!(ff_app_labels, vec!["Hide Firefox", "Hide Others", "Quit"]);

    let files_app_labels = labels_of(TopBarAppProfile::Files, "Files", TopBarMenuHit::ActiveApp);
    assert_eq!(files_app_labels, vec!["Hide Files", "Hide Others", "Quit"]);

    let desktop_app_entries = resolve_top_bar_menu_entries(
        TopBarAppProfile::Desktop,
        "Desktop",
        TopBarMenuHit::ActiveApp,
    );
    assert_eq!(
        desktop_app_entries,
        vec![
            ResolvedTopBarMenuItem::Item {
                label: std::borrow::Cow::Owned("Hide Desktop".to_owned()),
                icon: None,
                shortcut: None,
                action: TopBarMenuAction::HideActiveApp,
                enabled: false,
            },
            ResolvedTopBarMenuItem::Item {
                label: std::borrow::Cow::Borrowed("Hide Others"),
                icon: None,
                shortcut: None,
                action: TopBarMenuAction::HideOtherApps,
                enabled: true,
            },
            ResolvedTopBarMenuItem::Separator,
            ResolvedTopBarMenuItem::Item {
                label: std::borrow::Cow::Borrowed("Quit"),
                icon: None,
                shortcut: None,
                action: TopBarMenuAction::QuitActiveApp,
                enabled: false,
            },
        ]
    );

    let ff_file_labels = labels_of(TopBarAppProfile::Browser, "Firefox", TopBarMenuHit::File);
    assert!(ff_file_labels.iter().any(|l| l == "New Tab"));
    assert!(ff_file_labels.iter().any(|l| l == "Open Location..."));
    assert!(!ff_file_labels.iter().any(|l| l == "New Desktop Folder"));

    let term_edit =
        resolve_top_bar_menu_entries(TopBarAppProfile::Terminal, "Konsole", TopBarMenuHit::Edit);
    assert!(term_edit.iter().any(|e| matches!(
        e,
        ResolvedTopBarMenuItem::Item {
            action: TopBarMenuAction::AppShortcut(AppShortcutAction::CopyShift),
            ..
        }
    )));

    assert_eq!(
        resolve_top_bar_menu_entries(TopBarAppProfile::Browser, "Firefox", TopBarMenuHit::Go),
        Vec::new()
    );

    let (mx, my, _mw, _mh) = top_bar_menu_popover_rect_for_profile(
        TopBarAppProfile::Browser,
        "Firefox",
        TopBarMenuHit::File,
    );
    assert_eq!(
        check_top_bar_menu_popover_click_for_profile(
            TopBarAppProfile::Browser,
            "Firefox",
            TopBarMenuHit::File,
            f64::from(mx + 24),
            f64::from(my + MENU_POPOVER_PAD_Y + 8),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::AppShortcut(AppShortcutAction::NewTab))
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn desktop_active_app_and_system_menu_structure_matches_reference() {
    // 1. Desktop Profile menu entries: Hide Desktop (disabled), Hide Others (enabled), Separator, Quit (disabled)
    let desktop_entries = resolve_top_bar_menu_entries(
        TopBarAppProfile::Desktop,
        "Desktop",
        TopBarMenuHit::ActiveApp,
    );
    assert_eq!(desktop_entries.len(), 4);
    assert!(matches!(
        &desktop_entries[0],
        ResolvedTopBarMenuItem::Item {
            label,
            enabled: false,
            action: TopBarMenuAction::HideActiveApp,
            ..
        } if *label == "Hide Desktop"
    ));
    assert!(matches!(
        &desktop_entries[1],
        ResolvedTopBarMenuItem::Item {
            label,
            enabled: true,
            action: TopBarMenuAction::HideOtherApps,
            ..
        } if *label == "Hide Others"
    ));
    assert!(matches!(
        desktop_entries[2],
        ResolvedTopBarMenuItem::Separator
    ));
    assert!(matches!(
        &desktop_entries[3],
        ResolvedTopBarMenuItem::Item {
            label,
            enabled: false,
            action: TopBarMenuAction::QuitActiveApp,
            ..
        } if *label == "Quit"
    ));

    // 2. Hit-testing popover click respects `enabled` flag
    let (dx, dy, _dw, _dh) = top_bar_menu_popover_rect_for_profile(
        TopBarAppProfile::Desktop,
        "Desktop",
        TopBarMenuHit::ActiveApp,
    );
    // Row 0: Hide Desktop (disabled -> Inside, no action)
    assert_eq!(
        check_top_bar_menu_popover_click_for_profile(
            TopBarAppProfile::Desktop,
            "Desktop",
            TopBarMenuHit::ActiveApp,
            f64::from(dx + 20),
            f64::from(dy + MENU_POPOVER_PAD_Y + 8),
        ),
        TopBarMenuPopoverClick::Inside
    );
    // Row 1: Hide Others (enabled -> triggers Action)
    assert_eq!(
        check_top_bar_menu_popover_click_for_profile(
            TopBarAppProfile::Desktop,
            "Desktop",
            TopBarMenuHit::ActiveApp,
            f64::from(dx + 20),
            f64::from(dy + MENU_POPOVER_PAD_Y + MENU_ITEM_HEIGHT + 8),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::HideOtherApps)
    );
    // Row 3: Quit (disabled -> Inside, no action)
    assert_eq!(
        check_top_bar_menu_popover_click_for_profile(
            TopBarAppProfile::Desktop,
            "Desktop",
            TopBarMenuHit::ActiveApp,
            f64::from(dx + 20),
            f64::from(dy + MENU_POPOVER_PAD_Y + 2 * MENU_ITEM_HEIGHT + MENU_SEPARATOR_HEIGHT + 8),
        ),
        TopBarMenuPopoverClick::Inside
    );

    // 3. System logo menu has About RustyOS, System Settings..., session actions
    let system_entries = resolve_top_bar_menu_entries(
        TopBarAppProfile::Desktop,
        "Desktop",
        TopBarMenuHit::SystemLogo,
    );
    let system_labels: Vec<&str> = system_entries
        .iter()
        .filter_map(|e| match e {
            ResolvedTopBarMenuItem::Item { label, .. } => Some(&**label),
            ResolvedTopBarMenuItem::Separator => None,
        })
        .collect();
    assert_eq!(
        system_labels,
        vec![
            "About RustyOS",
            "System Settings...",
            "Suspend",
            "Restart...",
            "Shut Down...",
            "Lock Screen",
            "Log Out...",
        ]
    );

    // 4. Desktop has no Go menu
    assert!(!TopBarAppProfile::Desktop.has_go_menu());
}

#[test]
fn dock_hover_edge_positions_and_auto_hide_work() {
    use crate::config::DockPosition;

    let screen = Size::from((1280, 800));
    let tiles = build_default_dock_tiles(&[], false);
    let rest_view = DockViewState {
        tiles: &tiles,
        icon_size: 50,
        position: DockPosition::Bottom,
        auto_hide_reveal: 1.0,
        show_indicators: true,
        ..DockViewState::default()
    };
    let rest = compute_dock_layout(screen, &rest_view);
    let view = DockViewState {
        hover_slot: Some(1),
        hover_progress: 1.0,
        ..rest_view
    };
    let bottom = compute_dock_layout(screen, &view);
    assert_eq!(bottom.capsule_rect, rest.capsule_rect);
    assert_eq!(bottom.slots[1].icon_rect.size.w, 58);
    assert!(bottom.slots[1].icon_rect.loc.y < rest.slots[1].icon_rect.loc.y);
    assert_eq!(bottom.slots[0].icon_rect, rest.slots[0].icon_rect);
    assert_eq!(bottom.slots[2].icon_rect, rest.slots[2].icon_rect);

    let reduced = compute_dock_layout(
        screen,
        &DockViewState {
            reduced_motion: true,
            ..view
        },
    );
    assert_eq!(reduced.slots[1].icon_rect.size.w, 50);
    assert_eq!(
        reduced.slots[1].icon_rect.loc.y,
        rest.slots[1].icon_rect.loc.y - 2
    );

    let left_view = DockViewState {
        position: DockPosition::Left,
        ..view
    };
    let left = compute_dock_layout(screen, &left_view);
    assert!(left.capsule_rect.size.h > left.capsule_rect.size.w);
    assert!(left.capsule_rect.loc.x <= 12);

    let right_view = DockViewState {
        position: DockPosition::Right,
        ..view
    };
    let right = compute_dock_layout(screen, &right_view);
    assert!(right.capsule_rect.loc.x + right.capsule_rect.size.w >= 1260);
    assert!(is_pointer_in_dock_auto_hide_zone(
        screen,
        &right_view,
        false,
        1278.0,
        400.0
    ));
    assert!(!is_pointer_in_dock_auto_hide_zone(
        screen,
        &right_view,
        false,
        640.0,
        400.0
    ));

    let wa_auto = usable_work_area_with_dock(screen, DockPosition::Bottom, 50, true);
    assert_eq!(wa_auto.height, 800 - TOP_BAR_HEIGHT);
    let wa_left = usable_work_area_with_dock(screen, DockPosition::Left, 50, false);
    assert!(wa_left.x > 50 && wa_left.width < 1230);
    assert_eq!(
        canonical_dock_app_key(Some("foot"), "zsh", rusty_wm::WindowId(1)),
        canonical_dock_app_key(Some("foot"), "vim", rusty_wm::WindowId(2))
    );
    assert_eq!(
        canonical_dock_app_key(
            Some("Google-chrome"),
            "New Tab - Google Chrome",
            rusty_wm::WindowId(3)
        ),
        "google-chrome"
    );
    assert_eq!(
        canonical_dock_app_key(
            Some("org.mozilla.firefox"),
            "Download Google Chrome - Mozilla Firefox",
            rusty_wm::WindowId(4),
        ),
        "firefox"
    );
    assert_eq!(dock_label_for_key("google-chrome"), "Google Chrome");
    assert_eq!(dock_label_for_key("firefox"), "Firefox");
}

#[test]
fn dock_stack_layouts_and_hit_testing_work() {
    use crate::config::{DockPosition, DownloadsViewMode};

    let entries: Vec<DockStackEntry> = (0..5)
        .map(|idx| DockStackEntry {
            name: format!("file_{idx}.pdf"),
            path: std::path::PathBuf::from(format!("/tmp/file_{idx}.pdf")),
            is_dir: false,
            detail: "12 KB".to_owned(),
            extension: "PDF".to_owned(),
        })
        .collect();
    let anchor = Rectangle::new(Point::from((840, 730)), Size::from((52, 52)));
    let fan_state = DockStackRenderState {
        entries: &entries,
        view_mode: DownloadsViewMode::Automatic,
        dock_position: DockPosition::Bottom,
        anchor_rect: anchor,
        hovered_item: None,
        hovered_open_in_files: false,
        title: "Downloads",
    };
    let screen = Size::from((1280, 800));
    let fan_layout = compute_dock_stack_layout(screen, &fan_state);
    assert_eq!(fan_layout.resolved_mode, DownloadsViewMode::Fan);
    assert_eq!(fan_layout.items.len(), 5);
    let first = &fan_layout.items[0].hit_rect;
    assert_eq!(
        hit_test_dock_stack(
            screen,
            &fan_state,
            f64::from(first.loc.x + 8),
            f64::from(first.loc.y + 8)
        ),
        Some(DockStackHit::Entry(0))
    );
    let foot = fan_layout.footer_rect;
    assert_eq!(
        hit_test_dock_stack(
            screen,
            &fan_state,
            f64::from(foot.loc.x + 12),
            f64::from(foot.loc.y + 8)
        ),
        Some(DockStackHit::OpenInFiles)
    );
    assert_eq!(hit_test_dock_stack(screen, &fan_state, 40.0, 40.0), None);

    let left_grid = DockStackRenderState {
        dock_position: DockPosition::Left,
        ..fan_state
    };
    assert_eq!(
        compute_dock_stack_layout(screen, &left_grid).resolved_mode,
        DownloadsViewMode::Grid
    );
}

#[test]
fn top_bar_updates_indicator_and_popover_hit_testing() {
    let screen = Size::from((1280, 800));
    let opts = TopBarLayoutOptions {
        show_updates: true,
        ..TopBarLayoutOptions::default()
    };
    let right = compute_top_bar_right_layout_with_options(1280, "Wed Apr 1  9:41 AM", opts);
    let ux = right.updates_x.unwrap_or(0);
    assert!(ux > 0);
    assert!(check_top_bar_updates_click_for_options(
        1280,
        "Wed Apr 1  9:41 AM",
        opts,
        f64::from(ux + right.updates_w / 2),
        12.0,
    ));
    assert!(!check_top_bar_status_click_for_options(
        1280,
        "Wed Apr 1  9:41 AM",
        opts,
        f64::from(ux + right.updates_w / 2),
        12.0,
    ));

    let snapshot = rusty_services::UpdateStatusSnapshot {
        state: rusty_services::UpdateCheckState::Available,
        total_count: 2,
        arch_count: 1,
        rustyos_available: true,
        restart_recommended: true,
        last_checked_label: "11:30".to_owned(),
        error_hint: None,
        items: vec![
            rusty_services::PackageUpdateItem {
                name: "rustyos-git".to_owned(),
                old_version: "0.1.0-1".to_owned(),
                new_version: "0.1.1-1".to_owned(),
                is_rustyos: true,
                requires_restart: true,
            },
            rusty_services::PackageUpdateItem {
                name: "mesa".to_owned(),
                old_version: "25.0.1-1".to_owned(),
                new_version: "25.0.2-1".to_owned(),
                is_rustyos: false,
                requires_restart: true,
            },
        ],
    };
    let state = UpdatesRenderState {
        glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
        reduce_transparency: false,
        snapshot: &snapshot,
        anchor_right_x: ux + right.updates_w,
        hovered_hit: None,
        updater_installed: false,
    };
    let layout = compute_updates_popover_layout(screen, &state);
    assert_eq!(layout.item_rects.len(), 2);
    assert!(layout.panel_rect.loc.x + layout.panel_rect.size.w <= screen.w - 12);

    assert_eq!(
        hit_test_updates_popover(
            screen,
            &state,
            f64::from(layout.check_btn_rect.loc.x + 12),
            f64::from(layout.check_btn_rect.loc.y + 10),
        ),
        UpdatesPopoverHit::CheckNow
    );
    assert_eq!(
        hit_test_updates_popover(
            screen,
            &state,
            f64::from(layout.refresh_btn_rect.loc.x + 8),
            f64::from(layout.refresh_btn_rect.loc.y + 8),
        ),
        UpdatesPopoverHit::CheckNow
    );
    assert_eq!(
        hit_test_updates_popover(
            screen,
            &state,
            f64::from(layout.open_btn_rect.loc.x + 20),
            f64::from(layout.open_btn_rect.loc.y + 10),
        ),
        UpdatesPopoverHit::OpenUpdater
    );
    assert_eq!(
        hit_test_updates_popover(screen, &state, 20.0, 20.0),
        UpdatesPopoverHit::OutsidePopover
    );
}

#[test]
fn wifi_ethernet_subtitle_isolation_works() {
    // Ethernet label must never leak under Control Center's Wi-Fi capsule
    assert_eq!(
        quick_settings::concise_wifi_subtitle("Проводное соединение 1", true, false),
        "Off"
    );
    assert_eq!(
        quick_settings::concise_wifi_subtitle("Ethernet Connected", true, true),
        "On"
    );
    assert_eq!(
        quick_settings::concise_wifi_subtitle("Wi-Fi: Studio5G", true, true),
        "Studio5G"
    );
    assert_eq!(
        quick_settings::concise_wifi_subtitle_for_state(
            "Проводное соединение 1",
            true,
            Some("Binotel"),
            true,
        ),
        "Binotel"
    );

    let right = compute_top_bar_right_layout(1280, true, 0, "Wed Apr 1  9:41 AM");
    assert!(check_top_bar_wifi_click_for_options(
        1280,
        "Wed Apr 1  9:41 AM",
        TopBarLayoutOptions {
            has_battery: true,
            ..TopBarLayoutOptions::default()
        },
        f64::from(right.wifi_x + 10),
        12.0,
    ));
}

#[test]
fn wifi_popover_layout_and_hit_testing_work() {
    let screen = Size::from((1280, 800));
    let right = compute_top_bar_right_layout(1280, true, 0, "Wed Apr 1  9:41 AM");

    let net = rusty_services::NetworkStatus {
        connected: true,
        wired_connected: true,
        wired_interface: Some("enp3s0".to_owned()),
        wired_label: Some("Проводное соединение 1".to_owned()),
        wifi_connected: false,
        wifi_enabled: true,
        interface: Some("enp3s0".to_owned()),
        wifi_ssid: None,
        wifi_signal_percent: None,
        wifi_networks: vec![
            rusty_services::WifiAccessPoint {
                ssid: "Home5G".to_owned(),
                signal_percent: 88,
                secure: true,
                connected: false,
                known: true,
            },
            rusty_services::WifiAccessPoint {
                ssid: "GuestWiFi".to_owned(),
                signal_percent: 45,
                secure: false,
                connected: false,
                known: false,
            },
        ],
        status_label: "Проводное соединение 1".to_owned(),
    };
    let state = WifiPopoverRenderState {
        glass_opacity: 0.78,
        reduce_transparency: false,
        network: &net,
        anchor_right_x: right.wifi_x + 20,
        hovered_hit: None,
    };
    let layout = compute_wifi_popover_layout(screen, &state);
    assert!(layout.wired_rect.is_some());
    assert_eq!(layout.known_rows.len(), 1);
    assert_eq!(layout.other_rows.len(), 1);
    assert_eq!(
        hit_test_wifi_popover(
            screen,
            &state,
            f64::from(layout.switch_rect.loc.x + 10),
            f64::from(layout.switch_rect.loc.y + 10),
        ),
        WifiPopoverHit::ToggleWifi
    );
    assert_eq!(
        hit_test_wifi_popover(
            screen,
            &state,
            f64::from(layout.known_rows[0].rect.loc.x + 40),
            f64::from(layout.known_rows[0].rect.loc.y + 12),
        ),
        WifiPopoverHit::SelectNetwork(0)
    );
    assert_eq!(
        hit_test_wifi_popover(
            screen,
            &state,
            f64::from(layout.other_rows[0].rect.loc.x + 40),
            f64::from(layout.other_rows[0].rect.loc.y + 12),
        ),
        WifiPopoverHit::SelectNetwork(1)
    );
    assert_eq!(
        hit_test_wifi_popover(
            screen,
            &state,
            f64::from(layout.settings_rect.loc.x + 40),
            f64::from(layout.settings_rect.loc.y + 12),
        ),
        WifiPopoverHit::OpenWifiSettings
    );
    assert_eq!(
        hit_test_wifi_popover(screen, &state, 20.0, 20.0),
        WifiPopoverHit::OutsidePopover
    );
}

#[test]
fn top_bar_clock_and_control_center_click_regions_are_disjoint() {
    let screen_w = 1280;
    let clock_text = "Wed Apr 1  9:41 AM";
    let opts = TopBarLayoutOptions::default();
    let right = compute_top_bar_right_layout_with_options(screen_w, clock_text, opts);

    let cc_mid = f64::from(right.control_center_x + right.control_center_w / 2);
    let clock_mid = f64::from(right.clock_x + right.clock_w / 2);
    assert!(check_top_bar_status_click_for_options(
        screen_w, clock_text, opts, cc_mid, 12.0
    ));
    assert!(!check_top_bar_clock_click_for_options(
        screen_w, clock_text, opts, cc_mid, 12.0
    ));
    assert!(check_top_bar_clock_click_for_options(
        screen_w, clock_text, opts, clock_mid, 12.0
    ));
    assert!(!check_top_bar_status_click_for_options(
        screen_w, clock_text, opts, clock_mid, 12.0
    ));
}

#[test]
fn notification_center_layout_and_hit_testing_work() {
    let screen = Size::from((1280, 800));
    let items = [
        NotificationCenterItemView {
            id: 101,
            urgency: rusty_ipc::Urgency::Normal,
            title: "Backup Complete",
            body: "Snapshot stored safely.",
        },
        NotificationCenterItemView {
            id: 102,
            urgency: rusty_ipc::Urgency::Low,
            title: "Messages",
            body: "Ready for review.",
        },
        NotificationCenterItemView {
            id: 103,
            urgency: rusty_ipc::Urgency::Critical,
            title: "Software Update",
            body: "2 packages pending.",
        },
    ];
    let state = NotificationCenterRenderState {
        glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
        reduce_transparency: false,
        hide_previews: false,
        notifications: &items,
        task_states: [false, true, false, false],
        task_labels: DEFAULT_NOTIFICATION_TASKS,
        hovered_hit: None,
        location_name: "Local Station",
    };
    let layout = compute_notification_center_layout(screen, &state);
    assert!(layout.lead_card.is_some());
    assert!(layout.clear_all_rect.is_some());
    assert_eq!(layout.stack_cards.len(), 2);
    assert!(
        layout.panel_rect.loc.y + layout.panel_rect.size.h
            <= screen.h - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM,
        "Notification Center overlapped Dock on 1280x800: bottom={}",
        layout.panel_rect.loc.y + layout.panel_rect.size.h
    );

    if let Some(clear) = layout.clear_all_rect {
        assert_eq!(
            hit_test_notification_center(
                screen,
                &state,
                f64::from(clear.loc.x + 10),
                f64::from(clear.loc.y + 10),
            ),
            NotificationCenterHit::ClearAll
        );
    }
    if let Some(lead) = layout.lead_card {
        assert_eq!(
            hit_test_notification_center(
                screen,
                &state,
                f64::from(lead.card_rect.loc.x + 60),
                f64::from(lead.card_rect.loc.y + 20),
            ),
            NotificationCenterHit::DismissNotification(101)
        );
    }
    assert_eq!(
        hit_test_notification_center(
            screen,
            &state,
            f64::from(layout.task_rows[2].loc.x + 20),
            f64::from(layout.task_rows[2].loc.y + 12),
        ),
        NotificationCenterHit::ToggleTask(2)
    );
    assert_eq!(
        hit_test_notification_center(
            screen,
            &state,
            f64::from(layout.edit_widgets_rect.loc.x + 30),
            f64::from(layout.edit_widgets_rect.loc.y + 12),
        ),
        NotificationCenterHit::EditWidgets
    );
    assert_eq!(
        hit_test_notification_center(
            screen,
            &state,
            f64::from(layout.close_panel_rect.loc.x + 12),
            f64::from(layout.close_panel_rect.loc.y + 12),
        ),
        NotificationCenterHit::ClosePanel
    );
    assert_eq!(
        hit_test_notification_center(screen, &state, 20.0, 20.0),
        NotificationCenterHit::OutsidePanel
    );
}
