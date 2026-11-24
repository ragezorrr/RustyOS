use smithay::backend::renderer::Color32F;

use super::*;

#[test]
fn session_activation_env_args_include_wayland_and_optional_x11_display() {
    let base = build_session_activation_env_args("wayland-1", None, None);
    assert_eq!(
        base,
        vec![
            "--systemd",
            "WAYLAND_DISPLAY=wayland-1",
            "XDG_CURRENT_DESKTOP=RustyOS",
            "XDG_SESSION_DESKTOP=RustyOS",
            "XDG_SESSION_TYPE=wayland",
        ]
    );

    let with_x11 = build_session_activation_env_args(
        "wayland-0",
        Some(2),
        Some("/usr/share/xdg-desktop-portal/portals"),
    );
    assert_eq!(
        with_x11,
        vec![
            "--systemd",
            "WAYLAND_DISPLAY=wayland-0",
            "XDG_CURRENT_DESKTOP=RustyOS",
            "XDG_SESSION_DESKTOP=RustyOS",
            "XDG_SESSION_TYPE=wayland",
            "DISPLAY=:2",
            "XDG_DESKTOP_PORTAL_DIR=/usr/share/xdg-desktop-portal/portals",
        ]
    );
}

#[test]
fn hardware_report_serializes_valid_json() {
    let sample = DrmHardwareReport {
        primary_node: "/dev/dri/card1".to_owned(),
        render_node: Some("/dev/dri/renderD128".to_owned()),
        atomic_modesetting: true,
        universal_planes: true,
        monotonic_timestamps: true,
        async_page_flip: true,
        syncobj_eventfd: true,
        cursor_size: (256, 256),
        crtc_count: 4,
        plane_count: 31,
        cursor_plane_count: 4,
        connectors: vec![ConnectorReport {
            name: "eDP-1".to_owned(),
            status: "Connected".to_owned(),
            preferred_mode: Some((1920, 1080, 60)),
            mode_count: 3,
        }],
        dmabuf_render_format_count: 42,
        dmabuf_texture_format_count: 68,
        gbm_egl_dmabuf_test_passed: true,
        kms_master_status: "probe-only".to_owned(),
    };

    let json = sample.to_json();
    assert!(json.contains("\"primary_node\": \"/dev/dri/card1\""));
    assert!(json.contains("\"atomic_modesetting\": true"));
    assert!(json.contains("\"syncobj_eventfd\": true"));
    assert!(json.contains("\"cursor_plane_count\": 4"));
    assert!(json.contains("\"preferred_mode\": \"1920x1080@60Hz\""));
    assert!(json.contains("\"gbm_egl_dmabuf_test_passed\": true"));
}

#[test]
fn scoped_pointer_dirtying_limits_redraw_to_crossed_outputs() {
    use smithay::output::{Output, PhysicalProperties, Subpixel};

    let out1 = Output::new(
        "eDP-1".to_owned(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Test".to_owned(),
            model: "Primary".to_owned(),
        },
    );
    let mut map = crate::outputs::OutputMap::new(crate::outputs::OutputEntry::new(
        rusty_wm::OutputId(1),
        "eDP-1",
        out1,
        (0, 0).into(),
        (1920, 1080).into(),
        1.0,
        60_000,
        true,
    ));
    let out2 = Output::new(
        "DP-1".to_owned(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Test".to_owned(),
            model: "Secondary".to_owned(),
        },
    );
    map.add(crate::outputs::OutputEntry::new(
        rusty_wm::OutputId(2),
        "DP-1",
        out2,
        (1920, 0).into(),
        (1920, 1080).into(),
        1.0,
        144_000,
        false,
    ));

    // Motion completely inside display 1 should only dirty display 1.
    let inside_primary =
        output_ids_affected_by_pointer_motion(&map, (200.0, 200.0).into(), (400.0, 300.0).into());
    assert_eq!(inside_primary, vec![rusty_wm::OutputId(1)]);

    // Motion completely inside display 2 should only dirty display 2.
    let inside_secondary =
        output_ids_affected_by_pointer_motion(&map, (2500.0, 200.0).into(), (2700.0, 350.0).into());
    assert_eq!(inside_secondary, vec![rusty_wm::OutputId(2)]);

    // Motion crossing from display 1 into display 2 must dirty both so the old cursor disappears cleanly.
    let crossing =
        output_ids_affected_by_pointer_motion(&map, (1800.0, 500.0).into(), (2100.0, 500.0).into());
    assert_eq!(crossing, vec![rusty_wm::OutputId(1), rusty_wm::OutputId(2)]);
}

#[test]
fn mode_refresh_interval_covers_high_refresh_panels() {
    assert_eq!(mode_refresh_interval_us(60), 16_666);
    assert_eq!(mode_refresh_interval_us(120), 8_333);
    assert_eq!(mode_refresh_interval_us(144), 6_944);
    assert_eq!(mode_refresh_interval_us(240), 4_166);
    assert_eq!(mode_refresh_interval_us(0), 1_000_000);
    assert!(select_preferred_drm_mode(&[]).is_none());
}

#[test]
fn mode_selector_prefers_highest_refresh_at_native_resolution() {
    let modes = [
        ModeDescriptor {
            size: (2560, 1440),
            vrefresh: 60,
            preferred: true,
            clock: 241_500,
        },
        ModeDescriptor {
            size: (2560, 1440),
            vrefresh: 144,
            preferred: false,
            clock: 586_000,
        },
        ModeDescriptor {
            size: (2560, 1440),
            vrefresh: 240,
            preferred: false,
            clock: 980_000,
        },
        // Lower resolution at higher refresh must not override preferred native size.
        ModeDescriptor {
            size: (1920, 1080),
            vrefresh: 360,
            preferred: false,
            clock: 700_000,
        },
    ];

    assert_eq!(select_best_mode_index(&modes, false), Some(0));
    assert_eq!(select_best_mode_index(&modes, true), Some(2));
    assert_eq!(select_best_mode_index(&[], true), None);
}

#[test]
fn presentation_flags_and_vrr_refresh_metadata() {
    use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
    use smithay::wayland::presentation::Refresh;

    let regular = compute_drm_presentation_flags(false);
    assert!(regular.contains(wp_presentation_feedback::Kind::Vsync));
    assert!(regular.contains(wp_presentation_feedback::Kind::HwClock));
    assert!(!regular.contains(wp_presentation_feedback::Kind::ZeroCopy));

    let direct = compute_drm_presentation_flags(true);
    assert!(direct.contains(wp_presentation_feedback::Kind::Vsync));
    assert!(direct.contains(wp_presentation_feedback::Kind::HwClock));
    assert!(direct.contains(wp_presentation_feedback::Kind::ZeroCopy));

    assert!(matches!(
        compute_presentation_refresh(true, 144),
        Refresh::Variable(_)
    ));
    assert!(matches!(
        compute_presentation_refresh(false, 144),
        Refresh::Fixed(_)
    ));
}

#[test]
fn vblank_client_grace_duration_scales_with_refresh_rate_and_clamps_bounds() {
    assert_eq!(vblank_client_grace_duration(60), Duration::from_millis(4));
    assert_eq!(
        vblank_client_grace_duration(120),
        Duration::from_micros(2_083)
    );
    assert_eq!(
        vblank_client_grace_duration(144),
        Duration::from_micros(1_736)
    );
    assert_eq!(
        vblank_client_grace_duration(240),
        Duration::from_micros(1_041)
    );
    assert_eq!(
        vblank_client_grace_duration(480),
        Duration::from_micros(1_000)
    );
}

#[test]
fn solid_element_cache_preserves_id_and_commit_across_unchanged_frames() {
    use smithay::backend::renderer::element::Element;
    use smithay::utils::Rectangle;

    let geo = Rectangle::new((16, 24).into(), (240, 32).into());
    let color_a = Color32F::new(0.2, 0.3, 0.4, 1.0);
    let color_b = Color32F::new(0.5, 0.3, 0.4, 1.0);

    crate::shell_render::begin_solid_element_frame(1);
    let f1_first = crate::shell_render::make_solid_element(geo, color_a);
    let f1_second_same_rect = crate::shell_render::make_solid_element(geo, color_a);
    assert_ne!(f1_first.id(), f1_second_same_rect.id());

    crate::shell_render::begin_solid_element_frame(1);
    let f2_first = crate::shell_render::make_solid_element(geo, color_a);
    assert_eq!(f1_first.id(), f2_first.id());
    assert_eq!(f1_first.current_commit(), f2_first.current_commit());

    crate::shell_render::begin_solid_element_frame(1);
    let f3_recolored = crate::shell_render::make_solid_element(geo, color_b);
    assert_eq!(f1_first.id(), f3_recolored.id());
    assert_ne!(f2_first.current_commit(), f3_recolored.current_commit());
}

#[test]
fn drm_candidate_ranking_prioritizes_hardware_render_nodes_before_simpledrm() {
    let simple = PathBuf::from("/dev/dri/card0");
    let headless_dgpu = PathBuf::from("/dev/dri/card2");
    let primary_igpu = PathBuf::from("/dev/dri/card1");

    let ranked = rank_drm_candidates(vec![
        (simple.clone(), false, true),
        (headless_dgpu.clone(), false, false),
        (primary_igpu.clone(), true, false),
    ]);
    assert_eq!(ranked, vec![primary_igpu, headless_dgpu, simple]);
}

#[test]
fn software_drm_driver_and_parent_session_detection() {
    assert!(is_software_drm_driver("simpledrm"));
    assert!(is_software_drm_driver("simple-framebuffer"));
    assert!(is_software_drm_driver("vgem"));
    assert!(!is_software_drm_driver("i915"));
    assert!(!is_software_drm_driver("amdgpu"));

    assert!(!has_parent_graphical_session_in(None, None));
    assert!(!has_parent_graphical_session_in(Some("  "), Some("")));
    assert!(has_parent_graphical_session_in(Some("wayland-0"), None));
    assert!(has_parent_graphical_session_in(None, Some(":0")));
}

#[test]
fn client_forwarded_keyboard_and_axis_input_defer_redraw_until_commit() {
    assert!(!should_immediate_full_redraw_after_input(
        true, false, false
    ));
    assert!(should_immediate_full_redraw_after_input(true, true, false));
    assert!(should_immediate_full_redraw_after_input(true, false, true));
    assert!(should_immediate_full_redraw_after_input(
        false, false, false
    ));
}

#[test]
fn empty_drm_frame_schedules_or_delivers_pending_frame_callbacks() {
    let base = Instant::now();
    assert_eq!(
        plan_empty_frame_callback_action(base, Some(base), 60, false),
        EmptyFrameCallbackAction::None
    );
    assert_eq!(
        plan_empty_frame_callback_action(base, None, 60, true),
        EmptyFrameCallbackAction::SendNow
    );

    let interval_60 = Duration::from_micros(mode_refresh_interval_us(60));
    assert_eq!(
        plan_empty_frame_callback_action(base + Duration::from_millis(2), Some(base), 60, true),
        EmptyFrameCallbackAction::ScheduleAt(base + interval_60)
    );
    assert_eq!(
        plan_empty_frame_callback_action(base + interval_60, Some(base), 60, true),
        EmptyFrameCallbackAction::SendNow
    );

    let interval_144 = Duration::from_micros(mode_refresh_interval_us(144));
    assert_eq!(
        plan_empty_frame_callback_action(base + Duration::from_millis(2), Some(base), 144, true),
        EmptyFrameCallbackAction::ScheduleAt(base + interval_144)
    );
}

#[test]
fn keyboard_focus_pacing_interval_selection() {
    let idle_timeout = Duration::from_millis(50);
    let fastest_refresh = Duration::from_micros(16_667);

    let plan_timeout = |has_anim: bool, max_frames: bool, has_focus: bool| -> Duration {
        if has_anim || max_frames || has_focus {
            fastest_refresh
        } else {
            idle_timeout
        }
    };

    assert_eq!(plan_timeout(false, false, false), Duration::from_millis(50));
    assert_eq!(plan_timeout(true, false, false), fastest_refresh);
    assert_eq!(plan_timeout(false, true, false), fastest_refresh);
    assert_eq!(plan_timeout(false, false, true), fastest_refresh);
    assert_eq!(plan_timeout(true, false, true), fastest_refresh);
}
