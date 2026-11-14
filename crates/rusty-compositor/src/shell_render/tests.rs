use super::dock::*;
use super::lock_screen::*;
use super::modals::*;
use super::quick_settings::*;
use super::spotlight_overview::*;
use super::ssd::*;
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

    // Test TopBar Left Menu hits (`RUSTYOS-038`, `RUSTYOS-049`: macOS File/Edit/View/Go/Window/Help)
    let left = compute_top_bar_left_layout("RustyOS");
    assert_eq!(
        check_top_bar_menu_click(f64::from(TOP_BAR_LOGO_X + TOP_BAR_LOGO_W / 2), 12.0),
        Some(TopBarMenuHit::SystemLogo)
    );
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
            check_top_bar_menu_click(f64::from(bx + bw / 2), 12.0),
            Some(hit)
        );
    }
    assert_eq!(
        check_top_bar_menu_click(f64::from(left.help_x + left.help_w + 15), 12.0),
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
        check_window_ssd_hit(&windows, 118.0, 114.0),
        Some(SsdHit::Close(win2))
    );
    assert_eq!(
        check_window_ssd_hit(&windows, 136.0, 114.0),
        Some(SsdHit::Minimize(win2))
    );
    assert_eq!(
        check_window_ssd_hit(&windows, 154.0, 114.0),
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

    // TopBar unread notification badge pill hit-testing
    let (bx, by, bw, bh) = notification_badge_rect(size.w);
    assert!(check_top_bar_notification_click(
        size.w,
        2,
        f64::from(bx + bw / 2),
        f64::from(by + bh / 2)
    ));
    assert!(!check_top_bar_notification_click(
        size.w,
        0,
        f64::from(bx + bw / 2),
        f64::from(by + bh / 2)
    ));

    // TopBar keyboard layout pill (`EN`/`RU`) avoids notification badge when present
    let (kbd_x, kbd_y, kbd_w, kbd_h) = kbd_badge_rect(size.w, 0);
    assert!(check_top_bar_kbd_click(
        size.w,
        0,
        f64::from(kbd_x + kbd_w / 2),
        f64::from(kbd_y + kbd_h / 2)
    ));
    let (notif_kbd_x, notif_kbd_y, notif_kbd_w, notif_kbd_h) = kbd_badge_rect(size.w, 2);
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
}

#[test]
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
        },
    );
    assert!(all_opts.volume_x.is_some());
    assert!(all_opts.kbd_x.is_some());
    assert!(all_opts.kbd_x.unwrap_or(0) < all_opts.volume_x.unwrap_or(0));
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

    let (mx, my, _mw, _mh) = top_bar_menu_popover_rect("RustyOS", TopBarMenuHit::File);
    assert_eq!(
        check_top_bar_menu_popover_click(
            "RustyOS",
            TopBarMenuHit::File,
            f64::from(mx + 24),
            f64::from(my + 16),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::NewTerminal)
    );
    let (gx, gy, _gw, _gh) = top_bar_menu_popover_rect("RustyOS", TopBarMenuHit::Go);
    assert_eq!(
        check_top_bar_menu_popover_click(
            "RustyOS",
            TopBarMenuHit::Go,
            f64::from(gx + 24),
            f64::from(gy + 16),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::OpenOverview)
    );
    assert_eq!(
        check_top_bar_menu_popover_click(
            "RustyOS",
            TopBarMenuHit::Go,
            f64::from(gx + 24),
            f64::from(gy + 44),
        ),
        TopBarMenuPopoverClick::Action(TopBarMenuAction::NextWorkspace)
    );
    assert_eq!(
        check_top_bar_menu_popover_click("RustyOS", TopBarMenuHit::File, 800.0, 400.0),
        TopBarMenuPopoverClick::Outside
    );
}

#[test]
fn macos_dock_divider_gap_tooltip_and_radii_constants() {
    assert_eq!(DOCK_HEIGHT, 68);
    assert_eq!(DOCK_RADIUS, 22);
    assert_eq!(DOCK_ICON_SIZE, 50);
    assert_eq!(DOCK_ITEM_GAP, 6);
    assert_eq!(WINDOW_RADIUS, 20);
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
        check_window_traffic_lights_hover(&[ssd_win], 116.0, 92.0),
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

    // 4. Notifications (340 px, squircle icon choice) & OSD SDF glyph routing
    let notif_rects = notification_toast_rects(size, 1);
    assert_eq!(notif_rects[0].size.w, 340);
    assert_eq!(
        notification_app_icon("Foot Terminal"),
        crate::icons::BuiltinAppIcon::Terminal
    );
    assert_eq!(
        notification_app_icon("System Update"),
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

    let settings_state = SettingsRenderState {
        active_tab: SettingsTab::Appearance,
        glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
        reduce_transparency: false,
        reduced_motion: false,
        menubar_show_workspaces: false,
        menubar_show_tray: false,
        menubar_show_input_source: false,
        menubar_show_volume: false,
        clock_24h: false,
        primary_connector: "eDP-1",
        display_scale: 1.5,
        display_rotation: crate::config::DisplayRotation::Rotate0,
        vrr_mode: crate::config::VrrMode::Fullscreen,
        prefer_max_refresh: true,
        require_password_on_lock: true,
        lock_on_sleep: true,
        enable_polkit_agent: true,
        config_status_label: "Saved",
    };
    assert!(notifications_hidden_by_overlay(ShellOverlayView::Settings(
        &settings_state
    )));
    assert!(dock_tooltip_hidden_by_overlay(ShellOverlayView::Settings(
        &settings_state
    )));
}

#[test]
fn settings_modal_hit_testing_covers_tabs_controls_and_buttons() {
    let size = Size::from((1280, 800));
    let mut state = SettingsRenderState {
        active_tab: SettingsTab::Appearance,
        glass_opacity: 0.78,
        reduce_transparency: false,
        reduced_motion: false,
        menubar_show_workspaces: false,
        menubar_show_tray: false,
        menubar_show_input_source: false,
        menubar_show_volume: false,
        clock_24h: false,
        primary_connector: "Winit-1",
        display_scale: 1.0,
        display_rotation: crate::config::DisplayRotation::Rotate0,
        vrr_mode: crate::config::VrrMode::Fullscreen,
        prefer_max_refresh: true,
        require_password_on_lock: true,
        lock_on_sleep: true,
        enable_polkit_agent: true,
        config_status_label: "",
    };

    // Centered modal box (580x432) -> (350, 184) .. (930, 616)
    assert_eq!(
        hit_test_settings_modal(size, &state, 20.0, 20.0),
        SettingsHit::Outside
    );
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(350 + 24), f64::from(184 + 122)),
        SettingsHit::SelectTab(SettingsTab::MenuBar)
    );
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(350 + 24), f64::from(184 + 210)),
        SettingsHit::SelectTab(SettingsTab::Security)
    );
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(350 + 580 - 30), f64::from(184 + 30)),
        SettingsHit::Close
    );
    assert_eq!(
        hit_test_settings_modal(
            size,
            &state,
            f64::from(350 + 580 - 150),
            f64::from(184 + 432 - 35)
        ),
        SettingsHit::ReloadConfig
    );

    // Row 0 glass opacity stepper '+' is near right edge of row 0 (y ~ 184 + 98 + 20 = 302)
    let content_right = 350 + 198 + (580 - 218);
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(content_right - 20), 302.0),
        SettingsHit::GlassOpacityUp
    );
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(content_right - 115), 302.0),
        SettingsHit::GlassOpacityDown
    );

    state.active_tab = SettingsTab::MenuBar;
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(content_right - 40), 302.0),
        SettingsHit::ToggleMenuWorkspaces
    );

    state.active_tab = SettingsTab::Displays;
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(content_right - 20), 302.0),
        SettingsHit::ScaleUp
    );
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(content_right - 40), 354.0),
        SettingsHit::CycleRotation
    );

    state.active_tab = SettingsTab::Security;
    assert_eq!(
        hit_test_settings_modal(size, &state, f64::from(content_right - 40), 302.0),
        SettingsHit::ToggleRequirePassword
    );
}
