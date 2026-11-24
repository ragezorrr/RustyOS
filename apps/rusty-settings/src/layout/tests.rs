use super::*;
use crate::state::StartupOptions;

#[test]
fn default_appearance_layout_matches_882x600_metrics() {
    let mut app = SettingsAppState::new(&StartupOptions::default());
    let l = compute_layout(&app, 882, 600);
    assert_eq!(l.sidebar_rect.w, 230);
    assert_eq!(l.sidebar_items.len(), 20);
    assert!(l.max_page_scroll > 0);
    assert!(l.page_scrollbar_thumb.is_some());
    assert_eq!(hit_test(&l, 20.0, 20.0), Some(SettingsHit::TrafficClose));
    assert_eq!(hit_test(&l, 400.0, 20.0), Some(SettingsHit::WindowDrag));
    assert!(l.appearance.is_some());
    if let Some(app_cards) = &l.appearance {
        assert_eq!(app_cards.tiles.len(), 3);
        assert_eq!(app_cards.swatches.len(), 7);
        assert_eq!(app_cards.links.len(), 3);
        // Dark tile at x=403..511, y=90..166
        assert_eq!(
            hit_test(&l, 450.0, 120.0),
            Some(SettingsHit::SelectTheme(ThemeChoice::Dark))
        );
        // Auto tile at x=535..643
        assert_eq!(
            hit_test(&l, 580.0, 120.0),
            Some(SettingsHit::SelectTheme(ThemeChoice::Auto))
        );
        // Accent swatch #2 (Green: divider 231 + margin 24 + pad 14 + 2*52 + 19 = 392)
        assert_eq!(
            hit_test(&l, 231.0 + 24.0 + 14.0 + 2.0 * 52.0 + 19.0, 272.0),
            Some(SettingsHit::SelectAccent(2))
        );
        // Fonts navigation row
        assert_eq!(
            hit_test(&l, 400.0, 345.0),
            Some(SettingsHit::PushPage(SettingsPage::Fonts))
        );
    }
    assert!(app.scroll_by(false, 36, l.max_sidebar_scroll, l.max_page_scroll));
    let scrolled = compute_layout(&app, 882, 600);
    if let Some(app_cards) = &scrolled.appearance {
        assert_eq!(app_cards.mode_card_rect.y, 78 - 36);
    }
}

#[test]
fn wifi_and_network_pages_expose_service_cards_and_ap_rows() {
    let mut app = SettingsAppState::new(&StartupOptions::default());
    let sample_net = rusty_services::NetworkStatus {
        connected: true,
        wired_connected: true,
        wired_interface: Some(String::from("enp3s0")),
        wired_label: Some(String::from("Wired")),
        wifi_connected: true,
        wifi_enabled: true,
        interface: Some(String::from("enp3s0")),
        wifi_ssid: Some(String::from("Lab5G")),
        wifi_signal_percent: Some(82),
        wifi_networks: vec![
            rusty_services::WifiAccessPoint {
                ssid: String::from("Lab5G"),
                signal_percent: 82,
                secure: true,
                connected: true,
                known: true,
            },
            rusty_services::WifiAccessPoint {
                ssid: String::from("Lobby"),
                signal_percent: 45,
                secure: false,
                connected: false,
                known: false,
            },
        ],
        status_label: String::from("Wi-Fi: Lab5G"),
    };

    app.current_page = SettingsPage::Network;
    app.network = sample_net.clone();
    let net_layout = compute_layout(&app, 882, 600);
    assert!(net_layout.sections.len() >= 4);
    let wifi_card_row = &net_layout.sections[0].rows[0];
    assert_eq!(
        hit_test(
            &net_layout,
            f64::from(wifi_card_row.rect.x + 80),
            f64::from(wifi_card_row.rect.y + 20)
        ),
        Some(SettingsHit::PushPage(SettingsPage::Wifi))
    );

    app.current_page = SettingsPage::Wifi;
    app.network = sample_net;
    let wifi_layout = compute_layout(&app, 882, 600);
    assert!(wifi_layout.sections.len() >= 3);
    let header_row = &wifi_layout.sections[0].rows[0];
    assert_eq!(
        hit_test(
            &wifi_layout,
            f64::from(header_row.rect.right() - 24),
            f64::from(header_row.rect.y + 24)
        ),
        Some(SettingsHit::Control(RowControlId::WifiEnabledToggle))
    );
    let ap0 = &wifi_layout.sections[1].rows[0];
    assert_eq!(
        hit_test(
            &wifi_layout,
            f64::from(ap0.rect.x + 60),
            f64::from(ap0.rect.y + 18)
        ),
        Some(SettingsHit::Control(RowControlId::WifiConnectIndex(0)))
    );
    let ap1 = &wifi_layout.sections[2].rows[0];
    assert_eq!(
        hit_test(
            &wifi_layout,
            f64::from(ap1.rect.x + 60),
            f64::from(ap1.rect.y + 18)
        ),
        Some(SettingsHit::Control(RowControlId::WifiConnectIndex(1)))
    );
}

#[test]
fn notifications_page_exposes_header_policies_and_app_rows() {
    let mut app = SettingsAppState::new(&StartupOptions::default());
    app.current_page = SettingsPage::Notifications;
    let layout = compute_layout(&app, 882, 600);
    assert!(layout.sections.len() >= 4);

    let hero_row = &layout.sections[0].rows[0];
    assert_eq!(
        hit_test(
            &layout,
            f64::from(hero_row.rect.x + 80),
            f64::from(hero_row.rect.y + 24)
        ),
        Some(SettingsHit::Control(RowControlId::NotifAllowToggle))
    );

    let app0 = &layout.sections[2].rows[0];
    assert_eq!(
        hit_test(
            &layout,
            f64::from(app0.rect.x + 80),
            f64::from(app0.rect.y + 20)
        ),
        Some(SettingsHit::Control(RowControlId::NotifAppToggle(0)))
    );
    let was_enabled = app.notif_app_enabled[0];
    app.apply_row_action(RowControlId::NotifAppToggle(0));
    assert_eq!(app.notif_app_enabled[0], !was_enabled);
}

#[test]
fn software_update_page_exposes_status_and_actions() {
    let app = SettingsAppState::new(&StartupOptions {
        initial_page: SettingsPage::SoftwareUpdate,
        theme_override: None,
        ..StartupOptions::default()
    });
    assert_eq!(app.current_page, SettingsPage::SoftwareUpdate);
    let layout = compute_layout(&app, 882, 600);
    assert!(layout.sections.len() >= 2);
    assert_eq!(
        layout.sections[0]
            .section_label
            .as_ref()
            .map(|(s, _)| s.as_str()),
        Some("Software Update")
    );
}

#[test]
fn new_settings_pages_build_sections_and_hit_targets() {
    let mut app = SettingsAppState::new(&StartupOptions::default());

    app.current_page = SettingsPage::Bluetooth;
    app.bluetooth = rusty_services::BluetoothSnapshot {
        status: rusty_services::BluetoothStatus {
            available: true,
            powered: true,
            connected_device: Some(String::from("AirPods")),
            status_label: String::from("BT: AirPods"),
        },
        discovering: false,
        adapter_name: Some(String::from("rustyos-host")),
        adapter_address: None,
        devices: vec![rusty_services::BluetoothDevice {
            address: String::from("11:22:33:44:55:66"),
            name: String::from("AirPods"),
            icon: String::from("audio-headset"),
            paired: true,
            connected: true,
            trusted: true,
            battery_percent: Some(80),
        }],
    };
    let bluetooth_layout = compute_layout(&app, 882, 600);
    assert!(bluetooth_layout.sections.len() >= 3);
    let dev_row = &bluetooth_layout.sections[1].rows[0];
    let (act_r, info_r) = bt_device_action_rects(dev_row.rect);
    assert_eq!(
        hit_test(
            &bluetooth_layout,
            f64::from(act_r.x + 12),
            f64::from(act_r.y + 8)
        ),
        Some(SettingsHit::Control(RowControlId::BluetoothActionIndex(0)))
    );
    assert_eq!(
        hit_test(
            &bluetooth_layout,
            f64::from(info_r.x + 8),
            f64::from(info_r.y + 8)
        ),
        Some(SettingsHit::Control(RowControlId::BluetoothInfoIndex(0)))
    );

    app.current_page = SettingsPage::Battery;
    let power_layout = compute_layout(&app, 882, 600);
    assert!(power_layout.low_power_dropdown_rect.is_some());
    if let Some(dd) = power_layout.low_power_dropdown_rect {
        assert_eq!(
            hit_test(&power_layout, f64::from(dd.x + 10), f64::from(dd.y + 10)),
            Some(SettingsHit::LowPowerDropdownTrigger)
        );
    }

    for pg in [
        SettingsPage::About,
        SettingsPage::Users,
        SettingsPage::Shortcuts,
    ] {
        app.current_page = pg;
        let l = compute_layout(&app, 882, 600);
        assert_ne!(l.sections.len(), 0);
    }
}
