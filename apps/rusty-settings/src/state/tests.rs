#[allow(clippy::wildcard_imports)]
use super::config_io::*;
use super::*;

#[test]
fn toml_patch_preserves_existing_sections_and_updates_keys() {
    let input = "[ui]\ntheme = \"dark\"\n[autostart]\ncommands = [\"foot\"]\n";
    let out = patch_toml_text(input, "ui", "theme", "light");
    assert!(out.contains("theme = \"light\""));
    assert!(out.contains("commands = [\"foot\"]"));
    let out2 = patch_toml_text(&out, "ui", "accent_index", "2");
    assert!(out2.contains("accent_index = 2"));
}

#[test]
fn search_filters_sidebar_pages_by_keyword_and_category() {
    let mut st = SettingsAppState::defaults_with_page(SettingsPage::Appearance);
    st.search_query = String::from("dock");
    assert_eq!(st.visible_sidebar_pages(), vec![SettingsPage::DockDesktop]);
    st.search_query = String::from("network");
    assert_eq!(
        st.visible_sidebar_pages(),
        vec![
            SettingsPage::Wifi,
            SettingsPage::Bluetooth,
            SettingsPage::Network
        ]
    );
}

#[test]
fn smooth_scroll_accumulates_subpixel_deltas_and_eases_to_target() {
    let mut st = SettingsAppState::defaults_with_page(SettingsPage::Keyboard);
    assert!(!st.scroll_smooth_by(false, 0.4, 100, 400, false));
    assert!(st.scroll_smooth_by(false, 0.4, 100, 400, false));
    assert_eq!(st.page_scroll_y, 1);
    assert!(!st.scroll_smooth_by(false, 0.6, 100, 400, false));
    assert_eq!(st.page_scroll_y, 1);
    assert!(st.scroll_smooth_by(false, 0.4, 100, 400, false));
    assert_eq!(st.page_scroll_y, 2);

    // Discrete wheel impulse animates smoothly when !reduced_motion
    let mut anim = SettingsAppState::defaults_with_page(SettingsPage::Keyboard);
    assert!(anim.scroll_smooth_by(false, 36.0, 0, 400, true));
    assert!(anim.page_scroll_y > 0 && anim.page_scroll_y < 36);
    assert!(anim.is_scroll_animating());
    let mut t = Instant::now();
    for _ in 0..40 {
        t += Duration::from_millis(16);
        let _ = anim.step_scroll_animation(t);
    }
    assert_eq!(anim.page_scroll_y, 36);
    assert!(!anim.is_scroll_animating());

    // Reduced motion snaps immediately without trailing ticks
    let mut rm = SettingsAppState::defaults_with_page(SettingsPage::Keyboard);
    rm.reduced_motion = true;
    assert!(rm.scroll_smooth_by(false, 36.0, 0, 400, true));
    assert_eq!(rm.page_scroll_y, 36);
    assert!(!rm.is_scroll_animating());
}

#[test]
fn damage_scope_merges_and_coalesces_service_commands() {
    assert_eq!(
        DamageScope::Caret.merge(DamageScope::Sidebar),
        DamageScope::Sidebar
    );
    assert_eq!(
        DamageScope::Sidebar.merge(DamageScope::Page),
        DamageScope::Full
    );

    let coalesced = coalesce_service_commands(vec![
        ServiceCommand::Refresh,
        ServiceCommand::SetVolume(45),
        ServiceCommand::SetVolume(55),
        ServiceCommand::ToggleMute,
        ServiceCommand::ToggleMute,
        ServiceCommand::Refresh,
    ]);
    assert_eq!(coalesced, vec![ServiceCommand::SetVolume(55)]);

    let refresh_only =
        coalesce_service_commands(vec![ServiceCommand::Refresh, ServiceCommand::Refresh]);
    assert_eq!(refresh_only, vec![ServiceCommand::Refresh]);
}

#[test]
fn search_field_editing_clipboard_and_toggle_animations_work() {
    let mut st = SettingsAppState::defaults_with_page(SettingsPage::Appearance);
    let mut fe = rusty_ui::UiFontEngine::new();
    let mut mem = rusty_ui::MemoryClipboard::new();
    st.set_search_focused(true);
    assert!(st.apply_search_paste("display", &mut fe));
    assert_eq!(st.search_query, "display");
    let ctrl = rusty_ui::InputModifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };
    assert!(st.handle_search_text_key(rusty_ui::TextInputKey::Char('a'), ctrl, &mut fe, &mut mem));
    assert!(st.handle_search_text_key(rusty_ui::TextInputKey::Char('c'), ctrl, &mut fe, &mut mem));
    assert_eq!(
        rusty_ui::ClipboardSink::get_text(&mut mem).as_deref(),
        Some("display")
    );

    st.navigate_to(SettingsPage::DockDesktop);
    let l1 = crate::layout::compute_layout(&st, 882, 600);
    st.sync_toggles_from_layout(&l1);
    let p0 = st.switch_progress(RowControlId::DockIndicatorsToggle, st.dock_show_indicators);
    st.dock_show_indicators = !st.dock_show_indicators;
    let l2 = crate::layout::compute_layout(&st, 882, 600);
    st.sync_toggles_from_layout(&l2);
    std::thread::sleep(Duration::from_millis(20));
    let _ = st.step_ui_animations(Instant::now());
    let p1 = st.switch_progress(RowControlId::DockIndicatorsToggle, st.dock_show_indicators);
    assert!((p1 - p0).abs() > 0.01);

    assert_eq!(dock_visibility_label(&st.dock_visibility), "Always Show");
    st.apply_toml_device_kv("dock", "visibility", "smart_hide");
    assert_eq!(st.dock_visibility, "smart_hide");
    assert!(!st.dock_auto_hide);
    assert_eq!(dock_visibility_label(&st.dock_visibility), "Smart Hide");
    st.apply_toml_device_kv("dock", "auto_hide", "true");
    assert_eq!(st.dock_visibility, "always_hide");
    assert!(st.dock_auto_hide);
    assert_eq!(dock_visibility_label(&st.dock_visibility), "Always Hide");
}

#[test]
fn shortcut_validation_and_power_config_parsing_work() {
    assert_eq!(
        validate_shortcut_input(" super + shift + r "),
        Ok(String::from("Super+Shift+R"))
    );
    assert_eq!(validate_shortcut_input("alt+w"), Ok(String::from("Alt+W")));
    assert!(validate_shortcut_input("w").is_err());
    assert!(validate_shortcut_input("ctrl+alt+backspace").is_err());

    let mut st = SettingsAppState::defaults_with_page(SettingsPage::Battery);
    st.apply_toml_kv("power", "low_power_mode", "battery");
    st.apply_toml_kv("power", "display_sleep_minutes", "15");
    st.apply_toml_kv("power", "auto_lock_minutes", "5");
    st.apply_toml_kv("power", "dim_on_battery", "false");
    assert_eq!(st.power_low_power_mode, "battery");
    assert_eq!(st.low_power_dropdown.selected, 1);
    assert_eq!(st.power_display_sleep_minutes, 15);
    assert_eq!(st.power_auto_lock_minutes, 5);
    assert!(!st.power_dim_on_battery);

    st.apply_toml_kv("shortcuts", "launcher", "ctrl+space");
    assert_eq!(st.shortcuts[0], "Ctrl+Space");
}

#[test]
fn wifi_password_modal_disconnect_forget_and_error_retry_work() {
    let mut st = SettingsAppState::new(&StartupOptions {
        initial_page: SettingsPage::Wifi,
        wifi_prompt_ssid: Some(String::from("Cafe-5G")),
        ..Default::default()
    });
    assert!(matches!(
        st.active_modal,
        Some(SettingsModal::WifiPassword { ref ssid, show_password: false, .. }) if ssid == "Cafe-5G"
    ));
    assert!(
        st.active_modal
            .as_ref()
            .and_then(|m| m.sheet().input.as_ref())
            .is_some_and(|tf| tf.masked)
    );
    st.toggle_wifi_password_visibility();
    assert!(
        st.active_modal
            .as_ref()
            .and_then(|m| m.sheet().input.as_ref())
            .is_some_and(|tf| !tf.masked)
    );
    st.pending_service_cmds.clear();

    // Empty password stays open with validation message
    st.confirm_active_modal();
    assert!(matches!(
        st.active_modal,
        Some(SettingsModal::WifiPassword { .. })
    ));
    assert_eq!(st.pending_service_cmds.len(), 0);

    // Non-empty password enqueues ConnectWifiWithPassword
    if let Some(modal) = &mut st.active_modal
        && let Some(tf) = modal.sheet_mut().input.as_mut()
    {
        tf.buffer.set_text("super-secret-psk");
    }
    st.confirm_active_modal();
    assert!(st.active_modal.is_none());
    assert_eq!(
        st.pending_service_cmds,
        vec![ServiceCommand::ConnectWifiWithPassword {
            ssid: String::from("Cafe-5G"),
            password: String::from("super-secret-psk"),
        }]
    );
}

#[test]
fn wifi_error_reopens_modal_and_forget_disconnect_rescan_work() {
    let mut st = SettingsAppState::defaults_with_page(SettingsPage::Wifi);

    // Failed connect snapshot re-opens password dialog with nmcli error
    let err_snap = ServiceSnapshot {
        network: NetworkStatus {
            wifi_enabled: true,
            wifi_networks: vec![
                rusty_services::WifiAccessPoint {
                    ssid: String::from("Home-WiFi"),
                    signal_percent: 90,
                    secure: true,
                    connected: true,
                    known: true,
                },
                rusty_services::WifiAccessPoint {
                    ssid: String::from("Cafe-5G"),
                    signal_percent: 65,
                    secure: true,
                    connected: false,
                    known: false,
                },
            ],
            wifi_connected: true,
            wifi_ssid: Some(String::from("Home-WiFi")),
            ..NetworkStatus::default()
        },
        bluetooth: BluetoothSnapshot::default(),
        power: PowerDetails::default(),
        timedate: TimeDateStatus::default(),
        timedate_error: None,
        volume_percent: 50,
        muted: false,
        wifi_action_result: Some(WifiActionOutcome {
            ssid: String::from("Cafe-5G"),
            kind: WifiActionKind::Connect,
            result: Err(String::from("Wi-Fi network 'Cafe-5G' requires a password")),
        }),
    };
    assert!(st.apply_service_snapshot(err_snap));
    assert!(matches!(
        st.active_modal,
        Some(SettingsModal::WifiPassword { ref ssid, ref sheet, .. })
            if ssid == "Cafe-5G" && sheet.message.contains("requires a password")
    ));
    st.dismiss_active_modal();

    // Info button on saved network opens Forget modal; confirm enqueues ForgetWifi
    st.pending_service_cmds.clear();
    assert!(st.try_apply_service_action(RowControlId::WifiInfoIndex(0)));
    assert!(matches!(
        st.active_modal,
        Some(SettingsModal::WifiNetworkInfo { ref ssid, .. }) if ssid == "Home-WiFi"
    ));
    st.confirm_active_modal();
    assert_eq!(
        st.pending_service_cmds,
        vec![ServiceCommand::ForgetWifi(String::from("Home-WiFi"))]
    );

    // Disconnect active enqueues DisconnectWifi; Refresh enqueues RescanWifi
    st.pending_service_cmds.clear();
    assert!(st.try_apply_service_action(RowControlId::WifiDisconnectActive));
    assert!(st.try_apply_service_action(RowControlId::NetworkRefresh));
    assert_eq!(
        coalesce_service_commands(st.pending_service_cmds.clone()),
        vec![
            ServiceCommand::DisconnectWifi(String::from("Home-WiFi")),
            ServiceCommand::RescanWifi,
        ]
    );
}

#[test]
fn datetime_ntp_timezone_and_manual_time_modals_work() {
    let mut st = SettingsAppState::defaults_with_page(SettingsPage::DateTime);
    st.available_timezones = vec![
        String::from("UTC"),
        String::from("Europe/Kyiv"),
        String::from("Europe/Berlin"),
    ];

    // Toggle NTP on
    assert!(st.try_apply_service_action(RowControlId::DateTimeNtpToggle));
    assert!(st.timedate.ntp_enabled);
    assert_eq!(
        st.pending_service_cmds,
        vec![ServiceCommand::SetNtpEnabled(true)]
    );
    // Manual time blocked while NTP enabled
    assert!(st.try_apply_service_action(RowControlId::DateTimeManualOpen));
    assert!(st.active_modal.is_none());
    assert!(st.timedate_hint.contains("Automatic"));

    // Disable NTP and open manual date/time modal
    st.pending_service_cmds.clear();
    assert!(st.try_apply_service_action(RowControlId::DateTimeNtpToggle));
    assert!(st.try_apply_service_action(RowControlId::DateTimeManualOpen));
    assert!(matches!(
        st.active_modal,
        Some(SettingsModal::ManualDateTime { .. })
    ));
    if let Some(modal) = &mut st.active_modal
        && let Some(tf) = modal.sheet_mut().input.as_mut()
    {
        tf.buffer.set_text("2026-11-23 18:45");
    }
    st.confirm_active_modal();
    assert!(st.active_modal.is_none());

    // Timezone modal resolves unique partial match ("kyiv" -> "Europe/Kyiv")
    assert!(st.try_apply_service_action(RowControlId::DateTimeTimezoneOpen));
    if let Some(modal) = &mut st.active_modal
        && let Some(tf) = modal.sheet_mut().input.as_mut()
    {
        tf.buffer.set_text("kyiv");
    }
    st.confirm_active_modal();
    assert_eq!(st.timedate.timezone, "Europe/Kyiv");
    assert_eq!(
        coalesce_service_commands(st.pending_service_cmds.clone()),
        vec![
            ServiceCommand::SetNtpEnabled(false),
            ServiceCommand::SetTimezone(String::from("Europe/Kyiv")),
            ServiceCommand::SetManualDateTime(String::from("2026-11-23 18:45:00")),
        ]
    );
}

#[test]
fn glass_opacity_slider_clamps_to_compositor_bounds() {
    let mut st = SettingsAppState::defaults_with_page(SettingsPage::Appearance);
    st.set_glass_opacity(10);
    assert!((st.glass_opacity - 0.35).abs() < 1e-4);
    st.set_glass_opacity(110);
    assert!((st.glass_opacity - 1.0).abs() < 1e-4);
}
