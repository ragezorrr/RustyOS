use rusty_services::PowerProfile;
use smithay::utils::Transform;

use super::*;

#[test]
fn parses_valid_toml_with_comments_and_multiline_autostart() {
    let raw = r#"
        # RustyOS desktop preferences
        [keyboard]
        layout = "us,ru,de" # multi-layout
        variant = ",winkeys,"
        options = "grp:win_space_toggle"
        repeat_delay = 280
        repeat_rate = 40

        [ui]
        reduced_motion = true
        wallpaper = "/usr/share/backgrounds/rustyos.png"
        cursor_theme = "Adwaita"

        [autostart]
        commands = [
            "konsole",
            "nm-applet --indicator",
        ]
    "#;

    let cfg = UserConfig::parse_toml(raw);
    assert_eq!(cfg.keyboard_layout, "us,ru,de");
    assert_eq!(cfg.keyboard_variant, ",winkeys,");
    assert_eq!(cfg.keyboard_options, "grp:win_space_toggle");
    assert_eq!(cfg.repeat_delay, 280);
    assert_eq!(cfg.repeat_rate, 40);
    assert!(cfg.reduced_motion);
    assert_eq!(
        cfg.wallpaper,
        Some(PathBuf::from("/usr/share/backgrounds/rustyos.png"))
    );
    assert_eq!(cfg.cursor_theme.as_deref(), Some("Adwaita"));
    assert_eq!(cfg.autostart, vec!["konsole", "nm-applet --indicator"]);
}

#[test]
fn missing_or_out_of_range_values_fall_back_safely() {
    let raw = r"
        [keyboard]
        repeat_delay = 5
        repeat_rate = 999
    ";
    let cfg = UserConfig::parse_toml(raw);
    assert_eq!(cfg.keyboard_layout, "us,ru");
    assert_eq!(cfg.keyboard_options, "grp:alt_shift_toggle");
    assert_eq!(cfg.repeat_delay, 50);
    assert_eq!(cfg.repeat_rate, 120);
    assert!(!cfg.reduced_motion);
    assert_eq!(cfg.wallpaper, None);
    assert_eq!(cfg.cursor_theme, None);
    assert_eq!(cfg.autostart, Vec::<String>::new());

    let legacy_dock = UserConfig::parse_toml(
        r"
        [dock]
        icon_size = 56
        magnification_size = 76
        ",
    );
    assert_eq!(legacy_dock.dock_icon_size, 56);
}

#[test]
fn layout_short_code_maps_xkb_names_and_configured_codes() {
    assert_eq!(layout_short_code("English (US)", 0, "us,ru"), "EN");
    assert_eq!(layout_short_code("Russian", 1, "us,ru"), "RU");
    assert_eq!(layout_short_code("Unknown Layout", 1, "us,pl"), "PL");
}

#[test]
fn parses_display_vrr_and_refresh_preferences() {
    let defaults = UserConfig::default();
    assert!(defaults.prefer_max_refresh);
    assert_eq!(defaults.vrr_mode, VrrMode::Fullscreen);

    let cfg = UserConfig::parse_toml(
        r#"
        [display]
        prefer_max_refresh = false
        vrr = "always"
        "#,
    );
    assert!(!cfg.prefer_max_refresh);
    assert_eq!(cfg.vrr_mode, VrrMode::Always);

    let cfg_off = UserConfig::parse_toml(
        r#"
        [display]
        vrr_mode = "off"
        "#,
    );
    assert_eq!(cfg_off.vrr_mode, VrrMode::Off);
}

#[test]
fn parses_display_fractional_and_per_output_scales() {
    let cfg = UserConfig::parse_toml(
        r#"
        [display]
        scale = 1.25
        scales = { "DP-1" = 2.0, "HDMI-A-1" = 8.0 }

        [display.scales]
        "eDP-1" = 1.5
        dp-2 = 0.25
        "#,
    );
    assert!((cfg.display_scale - 1.25).abs() < 1e-6);
    assert!((cfg.scale_for_output("DP-1") - 2.0).abs() < 1e-6);
    assert!((cfg.scale_for_output("HDMI-A-1") - 4.0).abs() < 1e-6);
    assert!((cfg.scale_for_output("eDP-1") - 1.5).abs() < 1e-6);
    assert!((cfg.scale_for_output("DP-2") - 0.5).abs() < 1e-6);
    assert!((cfg.scale_for_output("DP-9") - 1.25).abs() < 1e-6);
    assert!(cfg.has_explicit_output_scale("eDP-1"));
    assert!(!cfg.has_explicit_output_scale("DP-9"));
}

#[test]
fn parses_ui_glass_opacity_and_reduce_transparency() {
    let defaults = UserConfig::default();
    assert!((defaults.glass_opacity - DEFAULT_GLASS_OPACITY).abs() < 1e-6);
    assert!(!defaults.reduce_transparency);
    assert!(!defaults.menubar_show_workspaces);
    assert!(!defaults.menubar_show_tray);
    assert!(!defaults.menubar_show_input_source);
    assert!(!defaults.menubar_show_volume);
    assert!(!defaults.clock_24h);
    assert_eq!(defaults.window_shadow, WindowShadowMode::Subtle);

    let cfg = UserConfig::parse_toml(
        r#"
        [ui]
        glass_opacity = 0.62
        reduce_transparency = true
        menubar_show_workspaces = true
        menubar_show_tray = true
        menubar_show_input_source = true
        menubar_show_volume = true
        clock_24h = true
        window_shadow = "none"
        "#,
    );
    assert!((cfg.glass_opacity - 0.62).abs() < 1e-6);
    assert!(cfg.reduce_transparency);
    assert!(cfg.menubar_show_workspaces);
    assert!(cfg.menubar_show_tray);
    assert!(cfg.menubar_show_input_source);
    assert!(cfg.menubar_show_volume);
    assert!(cfg.clock_24h);
    assert_eq!(cfg.window_shadow, WindowShadowMode::None);

    let normal_cfg = UserConfig::parse_toml("[ui]\nwindow_shadow = \"normal\"\n");
    assert_eq!(normal_cfg.window_shadow, WindowShadowMode::Normal);
    let bool_off = UserConfig::parse_toml("[ui]\nwindow_shadow = false\n");
    assert_eq!(bool_off.window_shadow, WindowShadowMode::None);
    let bool_on = UserConfig::parse_toml("[ui]\nwindow_shadow = true\n");
    assert_eq!(bool_on.window_shadow, WindowShadowMode::Subtle);

    let clamped_low = UserConfig::parse_toml("[ui]\nglass_opacity = 0.05\n");
    assert!((clamped_low.glass_opacity - MIN_GLASS_OPACITY).abs() < 1e-6);

    let clamped_high = UserConfig::parse_toml("[ui]\nglass_opacity = 4.5\n");
    assert!((clamped_high.glass_opacity - 1.0).abs() < 1e-6);
    assert!((clamp_glass_opacity(f32::NAN) - DEFAULT_GLASS_OPACITY).abs() < 1e-6);
}

#[test]
fn parses_display_rotations_and_per_output_overrides() {
    assert_eq!(DisplayRotation::parse("0"), Some(DisplayRotation::Rotate0));
    assert_eq!(
        DisplayRotation::parse("normal"),
        Some(DisplayRotation::Rotate0)
    );
    assert_eq!(
        DisplayRotation::parse("90deg"),
        Some(DisplayRotation::Rotate90)
    );
    assert_eq!(
        DisplayRotation::parse("inverted"),
        Some(DisplayRotation::Rotate180)
    );
    assert_eq!(
        DisplayRotation::parse("\"270\""),
        Some(DisplayRotation::Rotate270)
    );
    assert_eq!(DisplayRotation::parse("45"), None);

    let cfg = UserConfig::parse_toml(
        r#"
        [display]
        rotation = 90
        rotations = { "HDMI-A-1" = 270 }

        [display.rotations]
        "DP-1" = "inverted"

        [display.outputs."eDP-1"]
        rotation = 180
        scale = 1.5
        "#,
    );
    assert_eq!(cfg.display_rotation, DisplayRotation::Rotate90);
    assert_eq!(
        cfg.rotation_for_output("HDMI-A-1", None),
        DisplayRotation::Rotate270
    );
    assert_eq!(
        cfg.rotation_for_output("dp-1", None),
        DisplayRotation::Rotate180
    );
    assert_eq!(
        cfg.rotation_for_output("eDP-1", None),
        DisplayRotation::Rotate180
    );
    assert!((cfg.scale_for_output("eDP-1") - 1.5).abs() < 1e-6);
    assert_eq!(
        cfg.rotation_for_output("DP-9", None),
        DisplayRotation::Rotate90
    );
    assert_eq!(
        cfg.rotation_for_output("eDP-1", Some(DisplayRotation::Rotate0)),
        DisplayRotation::Rotate0
    );
    assert_eq!(
        DisplayRotation::Rotate180.to_wl_transform(),
        Transform::_180
    );
    assert_eq!(
        DisplayRotation::Rotate180.to_winit_transform(),
        Transform::Flipped
    );
    assert_eq!(
        DisplayRotation::Rotate0.to_winit_transform(),
        Transform::Flipped180
    );
}

#[test]
fn parses_security_section_preferences() {
    let defaults = UserConfig::default();
    assert!(defaults.require_password_on_lock);
    assert!(defaults.lock_on_sleep);
    assert!(defaults.enable_polkit_agent);

    let cfg = UserConfig::parse_toml(
        r"
        [security]
        require_password_on_lock = false
        lock_on_sleep = false
        enable_polkit_agent = false
        ",
    );
    assert!(!cfg.require_password_on_lock);
    assert!(!cfg.lock_on_sleep);
    assert!(!cfg.enable_polkit_agent);
}

#[test]
fn toml_serialization_and_atomic_save_roundtrip_all_sections()
-> Result<(), Box<dyn std::error::Error>> {
    let mut cfg = UserConfig {
        keyboard_layout: "us,ua".to_owned(),
        keyboard_variant: ",phonetic".to_owned(),
        keyboard_options: "grp:caps_toggle".to_owned(),
        repeat_delay: 280,
        repeat_rate: 42,
        appearance: AppearanceMode::Dark,
        ui_accent_index: 3,
        reduced_motion: true,
        glass_opacity: 0.62,
        reduce_transparency: true,
        menubar_show_workspaces: true,
        menubar_show_tray: true,
        menubar_show_input_source: true,
        menubar_show_volume: true,
        clock_24h: true,
        prefer_max_refresh: false,
        vrr_mode: VrrMode::Always,
        display_scale: 1.5,
        display_rotation: DisplayRotation::Rotate90,
        wallpaper: Some(PathBuf::from("/usr/share/wallpapers/rustyos.png")),
        cursor_theme: Some("Adwaita".to_owned()),
        autostart: vec![
            "foot --server".to_owned(),
            "nm-applet \"quoted\"".to_owned(),
        ],
        require_password_on_lock: false,
        lock_on_sleep: false,
        enable_polkit_agent: false,
        dock_icon_size: 56,
        dock_position: DockPosition::Left,
        dock_minimize_effect: DockMinimizeEffect::Scale,
        dock_minimize_into_app: true,
        dock_visibility: DockVisibilityMode::AlwaysHide,
        dock_auto_hide: true,
        dock_animate_opening: false,
        dock_show_indicators: false,
        dock_show_recent: false,
        dock_pinned_apps: vec![
            "files".to_owned(),
            "overview".to_owned(),
            "firefox".to_owned(),
            "terminal".to_owned(),
        ],
        dock_downloads_display: DownloadsDisplayMode::Folder,
        dock_downloads_view: DownloadsViewMode::Grid,
        downloads_sort: DownloadsSortMode::Kind,
        updates_enabled: false,
        updates_interval_minutes: 360,
        updates_show_in_menu_bar: false,
        ..UserConfig::default()
    };
    cfg.output_scales.insert("eDP-1".to_owned(), 1.75);
    cfg.output_scales.insert("DP-1".to_owned(), 2.0);
    cfg.output_rotations
        .insert("DP-1".to_owned(), DisplayRotation::Rotate270);

    let serialized = cfg.to_toml_string();
    let reparsed = UserConfig::parse_toml(&serialized);
    assert_eq!(reparsed, cfg);

    let temp_dir = std::env::temp_dir().join(format!(
        "rustyos-config-test-{}-{}",
        std::process::id(),
        reparsed.repeat_delay
    ));
    let path = temp_dir.join("nested/config.toml");
    cfg.save_to_path(&path)?;
    let reloaded = UserConfig::load_from_path(&path);
    assert_eq!(reloaded, cfg);
    let _ = fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn keyboard_preset_cycling_and_labels_cover_builtin_and_custom_values() {
    assert_eq!(next_keyboard_layout_preset("us"), "us,ru");
    assert_eq!(next_keyboard_layout_preset("us,es"), "us");
    assert_eq!(next_keyboard_layout_preset("us,jp"), "us,ru");

    assert_eq!(
        next_keyboard_option_preset("grp:alt_shift_toggle"),
        "grp:caps_toggle"
    );
    assert_eq!(next_keyboard_option_preset(""), "grp:alt_shift_toggle");
    assert_eq!(keyboard_option_label("grp:ctrl_shift_toggle"), "Ctrl+Shift");
    assert_eq!(keyboard_option_label(""), "Off");
    assert_eq!(keyboard_option_label("lv3:ralt_switch"), "Custom");
    assert!(default_user_wallpaper_path().is_none_or(|p| p.ends_with("rustyos/wallpaper.png")));
}

#[test]
fn parses_updates_section_and_clamps_interval() {
    let defaults = UserConfig::default();
    assert!(defaults.updates_enabled);
    assert_eq!(
        defaults.updates_interval_minutes,
        DEFAULT_UPDATES_INTERVAL_MINUTES
    );
    assert!(defaults.updates_show_in_menu_bar);

    let low = UserConfig::parse_toml(
        "[updates]\nenabled = false\ncheck_interval_minutes = 2\nshow_in_menu_bar = false\n",
    );
    assert!(!low.updates_enabled);
    assert_eq!(low.updates_interval_minutes, MIN_UPDATES_INTERVAL_MINUTES);
    assert!(!low.updates_show_in_menu_bar);

    let high = UserConfig::parse_toml("[updates]\ncheck_interval_minutes = 999999\n");
    assert_eq!(high.updates_interval_minutes, MAX_UPDATES_INTERVAL_MINUTES);
}

#[test]
fn parses_notifications_section_and_roundtrips() {
    let defaults = UserConfig::default();
    assert!(defaults.notifications_allow);
    assert_eq!(
        defaults.notifications_show_previews,
        NotificationPreviewsMode::WhenUnlocked
    );
    assert!(!defaults.notifications_when_sleeping);
    assert!(defaults.notifications_when_locked);
    assert!(defaults.notifications_mirroring_off);

    let parsed = UserConfig::parse_toml(
        "[notifications]\nallow = false\nshow_previews = \"never\"\nwhen_sleeping = true\nwhen_locked = false\nmirroring_off = false\n",
    );
    assert!(!parsed.notifications_allow);
    assert_eq!(
        parsed.notifications_show_previews,
        NotificationPreviewsMode::Never
    );
    assert!(parsed.notifications_when_sleeping);
    assert!(!parsed.notifications_when_locked);
    assert!(!parsed.notifications_mirroring_off);

    let reparsed = UserConfig::parse_toml(&parsed.to_toml_string());
    assert_eq!(reparsed, parsed);
}

#[test]
fn parses_power_and_shortcuts_sections_and_roundtrips() {
    let raw = r#"
        [power]
        low_power_mode = "battery"
        display_sleep_minutes = 15
        auto_lock_minutes = 5
        dim_on_battery = false

        [shortcuts]
        launcher = "Ctrl+Space"
        close_window = "Super+Q"
        overview = "Alt+Tab"
    "#;
    let parsed = UserConfig::parse_toml(raw);
    assert_eq!(parsed.power_low_power_mode, LowPowerMode::BatteryOnly);
    assert_eq!(parsed.power_display_sleep_minutes, 15);
    assert_eq!(parsed.power_auto_lock_minutes, 5);
    assert!(!parsed.power_dim_on_battery);
    assert_eq!(parsed.shortcuts.launcher, "Ctrl+Space");
    assert_eq!(parsed.shortcuts.close_window, "Super+Q");
    assert_eq!(parsed.shortcuts.overview, "Alt+Tab");

    let reparsed = UserConfig::parse_toml(&parsed.to_toml_string());
    assert_eq!(reparsed, parsed);

    assert_eq!(
        LowPowerMode::BatteryOnly.desired_profile(false, true),
        PowerProfile::LowPower
    );
    assert_eq!(
        LowPowerMode::BatteryOnly.desired_profile(true, true),
        PowerProfile::Balanced
    );
    assert_eq!(
        LowPowerMode::Never.desired_profile(false, true),
        PowerProfile::Balanced
    );
}

#[test]
fn parses_dock_visibility_modes_and_preserves_legacy_auto_hide_compat() {
    let legacy = UserConfig::parse_toml("[dock]\nauto_hide = true\n");
    assert_eq!(legacy.dock_visibility, DockVisibilityMode::AlwaysHide);
    assert_eq!(
        legacy.effective_dock_visibility(),
        DockVisibilityMode::AlwaysHide
    );
    assert!(legacy.dock_auto_hide);

    let smart_with_legacy_false =
        UserConfig::parse_toml("[dock]\nvisibility = \"smart_hide\"\nauto_hide = false\n");
    assert_eq!(
        smart_with_legacy_false.dock_visibility,
        DockVisibilityMode::SmartHide
    );
    assert!(!smart_with_legacy_false.dock_auto_hide);
    assert!(smart_with_legacy_false.dock_visibility.expands_work_area());
    assert!(
        smart_with_legacy_false
            .dock_visibility
            .uses_auto_hide_reveal()
    );

    let roundtrip = UserConfig::parse_toml(&smart_with_legacy_false.to_toml_string());
    assert_eq!(roundtrip.dock_visibility, DockVisibilityMode::SmartHide);
    assert!(!roundtrip.dock_auto_hide);

    assert_eq!(
        DockVisibilityMode::parse("intellihide"),
        Some(DockVisibilityMode::SmartHide)
    );
    assert_eq!(
        DockVisibilityMode::AlwaysShow.next(),
        DockVisibilityMode::SmartHide
    );
    assert_eq!(
        DockVisibilityMode::SmartHide.next(),
        DockVisibilityMode::AlwaysHide
    );
    assert_eq!(
        DockVisibilityMode::AlwaysHide.next(),
        DockVisibilityMode::AlwaysShow
    );
}
