//! Per-page card section builders for `rusty-settings`.

use crate::state::{RowControlId, SHORTCUT_DEFS, SettingsAppState, SettingsPage};

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn build_standard_page_sections(
    app: &SettingsAppState,
    page: SettingsPage,
    content_x: i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut cur_y = 56;
    match page {
        SettingsPage::Wallpaper
        | SettingsPage::MenuBar
        | SettingsPage::Notifications
        | SettingsPage::DateTime
        | SettingsPage::Language
        | SettingsPage::SoftwareUpdate
        | SettingsPage::Wifi
        | SettingsPage::Bluetooth
        | SettingsPage::Network
        | SettingsPage::About
        | SettingsPage::Battery
        | SettingsPage::Users => {
            build_system_page_sections(app, page, content_x, &mut cur_y, content_w)
        }
        SettingsPage::Display | SettingsPage::Sound | SettingsPage::Keyboard => {
            build_hardware_page_sections(app, page, content_x, &mut cur_y, content_w)
        }
        SettingsPage::Shortcuts => {
            build_shortcuts_page_sections(app, content_x, &mut cur_y, content_w)
        }
        SettingsPage::Mouse
        | SettingsPage::Touchpad
        | SettingsPage::Fonts
        | SettingsPage::IconTheme
        | SettingsPage::CursorStyle => {
            build_style_page_sections(app, page, content_x, &mut cur_y, content_w)
        }
        SettingsPage::Appearance | SettingsPage::DockDesktop => Vec::new(),
    }
}

#[allow(clippy::too_many_lines)]
fn build_system_page_sections(
    app: &SettingsAppState,
    page: SettingsPage,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    match page {
        SettingsPage::Wallpaper => {
            let (mode, source) = match app.custom_wallpaper.as_deref() {
                None | Some("rustyos-dynamic") => (
                    "Dynamic (4K)",
                    "Auto-switches rustyos-light-4k.png / rustyos-dark-4k.png",
                ),
                Some("rustyos-light") => ("Light (4K)", "assets/wallpapers/rustyos-light-4k.png"),
                Some("rustyos-dark") => ("Dark (4K)", "assets/wallpapers/rustyos-dark-4k.png"),
                Some(custom) => ("Custom PNG", custom),
            };
            vec![place_card_section(
                Some("Desktop Picture"),
                content_x,
                cur_y,
                content_w,
                vec![
                    row_choice("Wallpaper", RowControlId::ToggleWallpaper, mode.to_owned()),
                    row_info("Source", source.to_owned()),
                    row_info("Resolution", String::from("3840 × 2160 (4K UHD, 16:9)")),
                ],
            )]
        }
        SettingsPage::MenuBar => {
            vec![
                place_card_section(
                    Some("Status Icons"),
                    content_x,
                    cur_y,
                    content_w,
                    vec![
                        row_switch(
                            "Show Wi-Fi Indicator",
                            RowControlId::MenubarWifiToggle,
                            app.menubar_wifi,
                        ),
                        row_switch(
                            "Show Battery Indicator",
                            RowControlId::MenubarBatteryToggle,
                            app.menubar_battery,
                        ),
                        row_switch(
                            "Show Battery Percentage",
                            RowControlId::MenubarBatteryPercentageToggle,
                            app.menubar_battery_percentage,
                        ),
                        row_switch(
                            "Show Volume Indicator",
                            RowControlId::MenubarVolumeToggle,
                            app.menubar_volume,
                        ),
                        row_switch(
                            "Show Bluetooth Indicator",
                            RowControlId::MenubarBluetoothToggle,
                            app.menubar_bluetooth,
                        ),
                        row_switch(
                            "Show Input Source Badge",
                            RowControlId::MenubarInputToggle,
                            app.menubar_input,
                        ),
                        row_switch(
                            "Show Workspace Strip",
                            RowControlId::MenubarWorkspacesToggle,
                            app.menubar_workspaces,
                        ),
                        row_switch(
                            "Show System Tray Applets",
                            RowControlId::MenubarTrayToggle,
                            app.menubar_tray,
                        ),
                    ],
                ),
                place_card_section(
                    Some("Appearance & Clock"),
                    content_x,
                    cur_y,
                    content_w,
                    vec![
                        row_switch(
                            "Adaptive Wallpaper Contrast",
                            RowControlId::MenubarAdaptiveContrastToggle,
                            app.menubar_adaptive_contrast,
                        ),
                        row_switch(
                            "Use 24-Hour Clock",
                            RowControlId::Clock24hToggle,
                            app.clock_24h,
                        ),
                        row_choice(
                            "Clock Language",
                            RowControlId::ClockLocaleCycle,
                            match app.clock_locale.as_str() {
                                "ru" => "Русский",
                                "uk" => "Українська",
                                _ => "English",
                            }
                            .to_owned(),
                        ),
                    ],
                ),
            ]
        }
        SettingsPage::DateTime => {
            let now_label = rusty_services::local_clock_short_datetime_localized(
                &app.clock_locale,
                app.clock_24h,
            );
            let sync_label = if !app.timedate_hint.is_empty() {
                app.timedate_hint.clone()
            } else if app.timedate.ntp_synchronized {
                String::from("Synchronized via NTP")
            } else if app.timedate.ntp_enabled {
                String::from("NTP enabled (syncing...)")
            } else {
                String::from("Manual")
            };
            let manual_label = if app.timedate.ntp_enabled {
                String::from("Auto (NTP)")
            } else {
                String::from("Set...")
            };
            vec![
                place_card_section(
                    Some("Date & Time"),
                    content_x,
                    cur_y,
                    content_w,
                    vec![
                        row_info("Current Time", now_label),
                        row_switch(
                            "Set Time and Date Automatically",
                            RowControlId::DateTimeNtpToggle,
                            app.timedate.ntp_enabled,
                        ),
                        row_info("Synchronization", sync_label),
                        row_choice(
                            "Date & Time",
                            RowControlId::DateTimeManualOpen,
                            manual_label,
                        ),
                        row_choice(
                            "Time Zone",
                            RowControlId::DateTimeTimezoneOpen,
                            app.timedate.timezone.clone(),
                        ),
                        row_choice(
                            "Quick Time Zone",
                            RowControlId::DateTimeTimezoneCycle,
                            String::from("Next Preset"),
                        ),
                    ],
                ),
                place_card_section(
                    Some("Clock & Time Format"),
                    content_x,
                    cur_y,
                    content_w,
                    vec![
                        row_switch(
                            "Use 24-Hour Clock",
                            RowControlId::Clock24hToggle,
                            app.clock_24h,
                        ),
                        row_choice(
                            "Clock Language",
                            RowControlId::ClockLocaleCycle,
                            match app.clock_locale.as_str() {
                                "ru" => "Русский",
                                "uk" => "Українська",
                                _ => "English",
                            }
                            .to_owned(),
                        ),
                    ],
                ),
            ]
        }
        SettingsPage::Language => {
            vec![place_card_section(
                Some("Preferred Language"),
                content_x,
                cur_y,
                content_w,
                vec![
                    row_choice(
                        "System & Clock Language",
                        RowControlId::ClockLocaleCycle,
                        match app.clock_locale.as_str() {
                            "ru" => "Русский",
                            "uk" => "Українська",
                            _ => "English",
                        }
                        .to_owned(),
                    ),
                    row_info(
                        "Supported Languages",
                        "English • Русский • Українська".to_owned(),
                    ),
                ],
            )]
        }
        SettingsPage::Notifications => {
            build_notifications_page_sections(app, content_x, cur_y, content_w)
        }
        SettingsPage::SoftwareUpdate => {
            build_software_update_sections(app, content_x, cur_y, content_w)
        }
        SettingsPage::Wifi => build_wifi_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::Bluetooth => build_bluetooth_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::Network => build_network_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::About => build_about_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::Battery => build_battery_page_sections(app, content_x, cur_y, content_w),
        SettingsPage::Users => build_users_page_sections(app, content_x, cur_y, content_w),
        _ => Vec::new(),
    }
}

fn build_bluetooth_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let subtitle = if !app.bluetooth_status_hint.is_empty() {
        app.bluetooth_status_hint.clone()
    } else if !app.bluetooth.status.available {
        String::from("No Bluetooth hardware adapter detected.")
    } else if app.bluetooth.status.powered {
        let host = app
            .bluetooth
            .adapter_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&app.about_info.hostname);
        format!("Discoverable as \"{host}\"")
    } else {
        String::from("Turn on Bluetooth to connect headphones, mice, and keyboards.")
    };

    let mut header_rows = vec![RowSpec {
        label: String::from("Bluetooth"),
        height: 62,
        control: CardRowControl::ServiceHeaderSwitch {
            badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
            badge_kind: ServiceBadgeKind::Bluetooth,
            subtitle,
            action: RowControlId::BluetoothPoweredToggle,
            active: app.bluetooth.status.powered,
        },
    }];
    if app.bluetooth.status.available && app.bluetooth.status.powered {
        header_rows.push(row_choice(
            if app.bluetooth.discovering {
                "Scanning for Nearby Devices..."
            } else {
                "Nearby Discovery"
            },
            RowControlId::BluetoothScanToggle,
            String::from(if app.bluetooth.discovering {
                "Stop Scan"
            } else {
                "Scan"
            }),
        ));
    }
    sections.push(place_card_section(
        None,
        content_x,
        cur_y,
        content_w,
        header_rows,
    ));

    append_bluetooth_device_sections(app, content_x, cur_y, content_w, &mut sections);
    sections
}

fn append_bluetooth_device_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    sections: &mut Vec<CardSectionLayout>,
) {
    let mut my_rows = Vec::new();
    let mut nearby_rows = Vec::new();
    for (idx, dev) in app.bluetooth.devices.iter().enumerate() {
        let sub = if dev.connected {
            dev.battery_percent.map_or_else(
                || String::from("Connected"),
                |pct| format!("Connected · {pct}%"),
            )
        } else if dev.paired {
            String::from("Not Connected")
        } else {
            dev.address.clone()
        };
        let row = RowSpec {
            label: dev.name.clone(),
            height: 50,
            control: CardRowControl::BluetoothDeviceRow {
                subtitle: sub,
                connected: dev.connected,
                paired: dev.paired,
                action: RowControlId::BluetoothActionIndex(idx),
                info_action: RowControlId::BluetoothInfoIndex(idx),
            },
        };
        if dev.paired || dev.connected {
            my_rows.push(row);
        } else {
            nearby_rows.push(row);
        }
    }

    if my_rows.is_empty() {
        my_rows.push(row_info(
            "Paired Devices",
            String::from(if app.bluetooth.status.available {
                "None"
            } else {
                "Adapter Unavailable"
            }),
        ));
    }
    sections.push(place_card_section(
        Some("My Devices"),
        content_x,
        cur_y,
        content_w,
        my_rows,
    ));

    if app.bluetooth.status.powered {
        if nearby_rows.is_empty() {
            nearby_rows.push(row_info(
                "Nearby Devices",
                String::from(if app.bluetooth.discovering {
                    "Searching..."
                } else {
                    "Click Scan to discover devices"
                }),
            ));
        }
        sections.push(place_card_section(
            Some("Nearby Devices"),
            content_x,
            cur_y,
            content_w,
            nearby_rows,
        ));
    }
}

fn build_battery_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let st = &app.power.status;
    let power_sub = if st.has_battery {
        let verb = match st.state {
            rusty_services::BatteryState::Charging => "Charging",
            rusty_services::BatteryState::Discharging => "On Battery",
            rusty_services::BatteryState::Full => "Fully Charged",
            rusty_services::BatteryState::Unknown => "Battery Detected",
        };
        format!("{}% · {verb}", st.percentage)
    } else {
        String::from("AC Power · No Hardware Battery Detected")
    };

    let mut top_rows = vec![
        RowSpec {
            label: String::from("Battery & Power"),
            height: 58,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0x34, 0xC7, 0x59, 0xFF],
                badge_kind: ServiceBadgeKind::Battery,
                subtitle: power_sub,
                dot_rgba: if st.ac_online {
                    [40, 205, 65, 255]
                } else {
                    [245, 166, 35, 255]
                },
                target: None,
            },
        },
        RowSpec {
            label: String::from("Low Power Mode"),
            height: 44,
            control: CardRowControl::DropdownTrigger {
                value: app.low_power_dropdown.selected_label().to_owned(),
            },
        },
        row_choice(
            "Battery Health",
            RowControlId::BatteryDetailsOpen,
            format!("{} (i)", app.power.condition.label()),
        ),
    ];
    if !app.power.available_profiles.is_empty()
        && let Some(active) = app.power.active_profile
    {
        top_rows.push(row_choice(
            "System Power Profile",
            RowControlId::PowerProfileCycle,
            active.label().to_owned(),
        ));
    }
    sections.push(place_card_section(
        None, content_x, cur_y, content_w, top_rows,
    ));
    sections.push(build_battery_details_section(
        app, content_x, cur_y, content_w,
    ));
    sections
}

fn build_battery_details_section(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> CardSectionLayout {
    let st = &app.power.status;
    let mut detail_rows = vec![RowSpec {
        label: String::new(),
        height: 44,
        control: CardRowControl::BatterySegmentedTabs,
    }];
    if app.battery_tab.selected == 0 {
        detail_rows.push(row_info(
            "Power Source",
            String::from(if st.ac_online {
                "Power Adapter (AC)"
            } else {
                "Battery"
            }),
        ));
        if st.has_battery {
            detail_rows.push(row_info("Charge Level", format!("{}%", st.percentage)));
        }
        if let Some(mins) = app.power.time_remaining_minutes {
            let h = mins / 60;
            let m = mins % 60;
            detail_rows.push(row_info("Time Estimate", format!("{h}h {m:02}m")));
        }
        if let Some(watts) = app.power.power_watts {
            detail_rows.push(row_info("Power Rate", format!("{watts:.1} W")));
        }
        if let Some(model) = &app.power.model_name {
            detail_rows.push(row_info("Battery Model", model.clone()));
        }
    } else {
        let cap_str = app
            .power
            .health_percent
            .map_or_else(|| String::from("N/A"), |p| format!("{p}%"));
        detail_rows.push(row_info("Maximum Capacity", cap_str));
        if let Some(cycles) = app.power.cycle_count {
            detail_rows.push(row_info("Cycle Count", cycles.to_string()));
        }
        let sleep_lbl = if app.power_display_sleep_minutes == 0 {
            String::from("Never")
        } else {
            format!("{} min", app.power_display_sleep_minutes)
        };
        detail_rows.push(row_choice(
            "Turn Display Off on Inactivity",
            RowControlId::PowerDisplaySleepCycle,
            sleep_lbl,
        ));
        let lock_lbl = if app.power_auto_lock_minutes == 0 {
            String::from("Off")
        } else {
            format!("{} min", app.power_auto_lock_minutes)
        };
        detail_rows.push(row_choice(
            "Auto-Lock Screen After",
            RowControlId::PowerAutoLockCycle,
            lock_lbl,
        ));
        detail_rows.push(row_switch(
            "Slightly Dim Display on Battery",
            RowControlId::PowerDimOnBatteryToggle,
            app.power_dim_on_battery,
        ));
        detail_rows.push(row_switch(
            "Lock Screen on Sleep",
            RowControlId::LockSleepToggle,
            app.lock_on_sleep,
        ));
        detail_rows.push(row_choice(
            "Additional Power Settings",
            RowControlId::PowerOptionsOpen,
            String::from("Options..."),
        ));
    }

    place_card_section(
        Some("Energy & Health"),
        content_x,
        cur_y,
        content_w,
        detail_rows,
    )
}

fn build_about_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let about = &app.about_info;
    let mut sections = Vec::new();
    sections.push(place_card_section(
        None,
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: about.hostname.clone(),
            height: 82,
            control: CardRowControl::AboutHeroCard {
                subtitle: format!("{} ({})", about.os_name, about.architecture),
                action: RowControlId::AboutMoreInfoOpen,
            },
        }],
    ));
    sections.push(place_card_section(
        Some("Hardware & Software"),
        content_x,
        cur_y,
        content_w,
        vec![
            row_info("Processor", about.cpu.clone()),
            row_info("Memory", about.memory.clone()),
            row_info("Graphics", about.graphics.clone()),
            row_info("Startup Disk", about.startup_disk.clone()),
            row_info("Kernel", about.kernel.clone()),
            row_info("Compositor", String::from("RustyOS Wayland (Smithay)")),
        ],
    ));
    sections.push(place_card_section(
        Some("Related"),
        content_x,
        cur_y,
        content_w,
        vec![
            RowSpec {
                label: String::from("Software Update"),
                height: 44,
                control: CardRowControl::Navigation {
                    target: SettingsPage::SoftwareUpdate,
                    leading: Some(SettingsPage::SoftwareUpdate),
                },
            },
            RowSpec {
                label: String::from("Displays"),
                height: 44,
                control: CardRowControl::Navigation {
                    target: SettingsPage::Display,
                    leading: Some(SettingsPage::Display),
                },
            },
        ],
    ));
    sections
}

fn build_users_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let details = app.user_details.as_ref();
    let username = details.map_or("user", |d| d.username.as_str());
    let display_name = details.map_or(username, |d| d.display_name.as_str());
    let role = if details.is_some_and(|d| d.is_admin) {
        "Administrator"
    } else {
        "Standard User"
    };
    let initials: String = display_name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    let initials = if initials.is_empty() {
        String::from("U")
    } else {
        initials
    };

    sections.push(place_card_section(
        Some("Current Account"),
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: display_name.to_owned(),
            height: 62,
            control: CardRowControl::UserProfileCard {
                initials,
                subtitle: format!("@{username} · {role}"),
                badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
            },
        }],
    ));

    if let Some(det) = details {
        let groups_str = if det.groups.is_empty() {
            String::from("None")
        } else {
            let joined = det.groups.join(", ");
            if joined.len() > 38 {
                format!("{} groups ({})", det.groups.len(), det.groups[0])
            } else {
                joined
            }
        };
        sections.push(place_card_section(
            Some("Account Details"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_info("Username", det.username.clone()),
                row_info("Account Role", String::from(role)),
                row_info("User / Group ID", format!("{} : {}", det.uid, det.gid)),
                row_info("Home Directory", det.home_dir.clone()),
                row_info("Login Shell", det.shell.clone()),
                row_info("Session", det.session_type.clone()),
                row_info("Groups", groups_str),
            ],
        ));
    }

    let mut local_rows: Vec<RowSpec> = app
        .all_users
        .iter()
        .map(|u| {
            row_info_string(
                u.display_name.clone(),
                format!("@{} (UID {})", u.username, u.uid),
            )
        })
        .collect();
    local_rows.push(row_info(
        "Manage Users",
        String::from("Read-only (AccountsService & Polkit separate)"),
    ));
    sections.push(place_card_section(
        Some("Local Users"),
        content_x,
        cur_y,
        content_w,
        local_rows,
    ));
    sections
}

fn build_shortcuts_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut system_rows = Vec::new();
    let mut wm_rows = Vec::new();
    for (idx, def) in SHORTCUT_DEFS.iter().enumerate() {
        let row = RowSpec {
            label: def.label.to_owned(),
            height: 44,
            control: CardRowControl::ShortcutBinding {
                combo: app.shortcuts[idx].clone(),
                action: RowControlId::ShortcutEditIndex(idx),
            },
        };
        if def.category == "System & Shell" {
            system_rows.push(row);
        } else {
            wm_rows.push(row);
        }
    }
    vec![
        place_card_section(
            Some("System & Shell"),
            content_x,
            cur_y,
            content_w,
            system_rows,
        ),
        place_card_section(
            Some("Windows & Tiling"),
            content_x,
            cur_y,
            content_w,
            wm_rows,
        ),
        place_card_section(
            Some("Defaults"),
            content_x,
            cur_y,
            content_w,
            vec![row_choice(
                "Restore Default Keybindings",
                RowControlId::ShortcutsResetDefaults,
                String::from("Reset Defaults"),
            )],
        ),
    ]
}

#[allow(clippy::too_many_lines)]
fn build_notifications_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = vec![
        place_card_section_with_gap(
            None,
            content_x,
            cur_y,
            content_w,
            vec![RowSpec {
                label: String::from("Notifications"),
                height: 64,
                control: CardRowControl::NotificationHeader {
                    subtitle: String::from(
                        "Customize banners, sounds, and Notification Center behavior.",
                    ),
                    action: RowControlId::NotifAllowToggle,
                    active: app.notif_allow,
                },
            }],
            18,
        ),
        place_card_section(
            Some("Notification Center"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Show previews",
                    RowControlId::NotifShowPreviewsCycle,
                    app.notif_show_previews.clone(),
                ),
                row_switch(
                    "when display is sleeping",
                    RowControlId::NotifWhenSleepingToggle,
                    app.notif_when_sleeping,
                ),
                row_switch(
                    "when screen is locked",
                    RowControlId::NotifWhenLockedToggle,
                    app.notif_when_locked,
                ),
                row_choice(
                    "when mirroring or sharing the display",
                    RowControlId::NotifMirroringCycle,
                    String::from(if app.notif_mirroring_off {
                        "Notifications Off"
                    } else {
                        "Allow"
                    }),
                ),
            ],
        ),
    ];

    append_notification_app_rows_section(app, content_x, cur_y, content_w, &mut sections);

    sections.push(place_card_section(
        Some("Update Notifications"),
        content_x,
        cur_y,
        content_w,
        vec![row_switch(
            "Automatic Update Checks",
            RowControlId::UpdatesEnabledToggle,
            app.updates_enabled,
        )],
    ));
    sections
}

fn append_notification_app_rows_section(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    sections: &mut Vec<CardSectionLayout>,
) {
    let app_specs: [(&str, NotificationAppKind, [u8; 4]); 5] = [
        (
            "Files",
            NotificationAppKind::Files,
            [0x0A, 0x84, 0xFF, 0xFF],
        ),
        (
            "Terminal",
            NotificationAppKind::Terminal,
            [0x36, 0x3A, 0x45, 0xFF],
        ),
        (
            "Browser",
            NotificationAppKind::Browser,
            [0x24, 0x91, 0xFF, 0xFF],
        ),
        (
            "Software Update",
            NotificationAppKind::Updater,
            [0x34, 0xC7, 0x59, 0xFF],
        ),
        (
            "System Settings",
            NotificationAppKind::Settings,
            [0x76, 0x7A, 0x84, 0xFF],
        ),
    ];
    let app_rows = app_specs
        .into_iter()
        .enumerate()
        .map(|(idx, (name, kind, tint))| {
            let active = app.notif_app_enabled.get(idx).copied().unwrap_or(true);
            RowSpec {
                label: name.to_owned(),
                height: 52,
                control: CardRowControl::NotificationAppRow {
                    app_kind: kind,
                    badge_tint: tint,
                    subtitle: String::from(if active {
                        "Banners, Badges, Sounds"
                    } else {
                        "Off"
                    }),
                    active,
                    action: RowControlId::NotifAppToggle(idx),
                },
            }
        })
        .collect();
    sections.push(place_card_section(
        Some("Application Notifications"),
        content_x,
        cur_y,
        content_w,
        app_rows,
    ));
}

fn build_wifi_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let mut header_rows = vec![RowSpec {
        label: String::from("Wi-Fi"),
        height: 62,
        control: CardRowControl::WifiHeaderSwitch {
            subtitle: String::from("Set up Wi-Fi to wirelessly connect to the internet."),
            action: RowControlId::WifiEnabledToggle,
            active: app.network.wifi_enabled,
        },
    }];
    if app.network.wifi_connected || app.network.wifi_ssid.is_some() {
        let active_name = app
            .network
            .wifi_ssid
            .clone()
            .unwrap_or_else(|| String::from("Connected Network"));
        let sig_str = app.network.wifi_signal_percent.map_or_else(
            || String::from("Connected"),
            |s| format!("Connected · {s}%"),
        );
        header_rows.push(RowSpec {
            label: active_name,
            height: 54,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
                badge_kind: ServiceBadgeKind::Wifi,
                subtitle: sig_str,
                dot_rgba: [40, 205, 65, 255],
                target: None,
            },
        });
        header_rows.push(RowSpec {
            label: String::from("Current Connection"),
            height: 44,
            control: CardRowControl::Choice {
                action: RowControlId::WifiDisconnectActive,
                value: String::from("Disconnect"),
            },
        });
    }
    sections.push(place_card_section(
        None,
        content_x,
        cur_y,
        content_w,
        header_rows,
    ));

    if app.network.wifi_enabled {
        append_wifi_network_sections(app, content_x, cur_y, content_w, &mut sections);
    } else {
        sections.push(place_card_section(
            Some("Known Networks"),
            content_x,
            cur_y,
            content_w,
            vec![row_info("Wi-Fi Radio", String::from("Off"))],
        ));
    }

    sections.push(place_card_section(
        Some("Security & Lock Screen"),
        content_x,
        cur_y,
        content_w,
        vec![
            row_switch(
                "Require Password on Lock",
                RowControlId::LockPasswordToggle,
                app.require_password_on_lock,
            ),
            row_switch(
                "Lock Screen on Sleep",
                RowControlId::LockSleepToggle,
                app.lock_on_sleep,
            ),
            row_switch(
                "Built-in Polkit Agent",
                RowControlId::PolkitToggle,
                app.enable_polkit_agent,
            ),
        ],
    ));
    sections
}

fn append_wifi_network_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    sections: &mut Vec<CardSectionLayout>,
) {
    let mut known_rows = Vec::new();
    let mut other_rows = Vec::new();
    for (idx, ap) in app.network.wifi_networks.iter().enumerate() {
        let row = RowSpec {
            label: ap.ssid.clone(),
            height: 40,
            control: CardRowControl::WifiNetwork {
                connected: ap.connected,
                secure: ap.secure,
                signal_percent: ap.signal_percent,
                action: RowControlId::WifiConnectIndex(idx),
                info_action: RowControlId::WifiInfoIndex(idx),
            },
        };
        if ap.connected || ap.known {
            known_rows.push(row);
        } else {
            other_rows.push(row);
        }
    }
    if !known_rows.is_empty() {
        sections.push(place_card_section(
            Some("Known Networks"),
            content_x,
            cur_y,
            content_w,
            known_rows,
        ));
    }
    other_rows.push(RowSpec {
        label: String::from("Scan Nearby Networks"),
        height: 44,
        control: CardRowControl::Choice {
            action: RowControlId::NetworkRefresh,
            value: String::from("Refresh"),
        },
    });
    sections.push(place_card_section(
        Some("Other Networks"),
        content_x,
        cur_y,
        content_w,
        other_rows,
    ));
}

fn build_network_page_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();
    let green_dot = [40, 205, 65, 255];
    let amber_dot = [245, 166, 35, 255];
    let gray_dot = [152, 152, 160, 255];

    let (wifi_sub, wifi_dot) = if app.network.wifi_connected {
        let name = app.network.wifi_ssid.as_deref().unwrap_or("Connected");
        (format!("Connected · {name}"), green_dot)
    } else if app.network.wifi_enabled {
        (String::from("On · Not Connected"), amber_dot)
    } else {
        (String::from("Off"), gray_dot)
    };
    sections.push(place_card_section_with_gap(
        None,
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: String::from("Wi-Fi"),
            height: 54,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
                badge_kind: ServiceBadgeKind::Wifi,
                subtitle: wifi_sub,
                dot_rgba: wifi_dot,
                target: Some(SettingsPage::Wifi),
            },
        }],
        12,
    ));

    let (eth_sub, eth_dot) = if app.network.wired_connected {
        let label = app.network.wired_interface.as_deref().map_or_else(
            || String::from("Connected"),
            |iface| format!("Connected · {iface}"),
        );
        (label, green_dot)
    } else {
        (String::from("Cable Unplugged"), gray_dot)
    };
    sections.push(place_card_section_with_gap(
        None,
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: String::from("Ethernet"),
            height: 54,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0x00, 0x7A, 0xFF, 0xFF],
                badge_kind: ServiceBadgeKind::Ethernet,
                subtitle: eth_sub,
                dot_rgba: eth_dot,
                target: None,
            },
        }],
        12,
    ));

    sections.push(place_card_section_with_gap(
        None,
        content_x,
        cur_y,
        content_w,
        vec![RowSpec {
            label: String::from("Firewall"),
            height: 54,
            control: CardRowControl::ServiceStatus {
                badge_tint: [0xFF, 0x8D, 0x28, 0xFF],
                badge_kind: ServiceBadgeKind::Firewall,
                subtitle: String::from("Inactive"),
                dot_rgba: gray_dot,
                target: None,
            },
        }],
        20,
    ));

    sections.push(place_card_section(
        Some("Other Services"),
        content_x,
        cur_y,
        content_w,
        vec![
            row_info(
                "Primary Interface",
                app.network
                    .interface
                    .clone()
                    .unwrap_or_else(|| String::from("None")),
            ),
            row_info("Backend", String::from("NetworkManager / D-Bus")),
            row_choice(
                "Network Status",
                RowControlId::NetworkRefresh,
                String::from("Refresh"),
            ),
        ],
    ));
    sections
}

#[allow(clippy::cast_possible_truncation)]
fn build_hardware_page_sections(
    app: &SettingsAppState,
    page: SettingsPage,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    match page {
        SettingsPage::Display => vec![place_card_section(
            Some("Display Configuration"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Active Monitor",
                    RowControlId::DisplayOutputCycle,
                    app.active_display_label(),
                ),
                row_stepper(
                    "HiDPI Scale",
                    RowControlId::ScaleDown,
                    RowControlId::ScaleUp,
                    format!("{}%", (app.display_scale * 100.0).round() as i32),
                ),
                row_switch(
                    "Prefer Maximum Refresh Rate",
                    RowControlId::PreferMaxRefreshToggle,
                    app.prefer_max_refresh,
                ),
                row_choice(
                    "Adaptive Sync (VRR)",
                    RowControlId::VrrModeCycle,
                    app.vrr_mode.clone(),
                ),
                row_choice(
                    "Rotation",
                    RowControlId::RotationCycle,
                    format!("{} deg", app.display_rotation),
                ),
            ],
        )],
        SettingsPage::Sound => vec![place_card_section(
            Some("Output Volume"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_stepper(
                    "Master Volume",
                    RowControlId::VolumeDown,
                    RowControlId::VolumeUp,
                    format!("{}%", app.volume_percent),
                ),
                row_switch("Mute Output", RowControlId::ToggleMute, app.muted),
            ],
        )],
        SettingsPage::Keyboard => vec![place_card_section(
            Some("Keyboard & Input Sources"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Input Layouts",
                    RowControlId::KeyboardLayoutCycle,
                    app.keyboard_layout.clone(),
                ),
                row_choice(
                    "Switch Shortcut",
                    RowControlId::KeyboardOptionCycle,
                    app.keyboard_options.clone(),
                ),
                row_stepper(
                    "Repeat Rate",
                    RowControlId::RepeatRateDown,
                    RowControlId::RepeatRateUp,
                    format!("{}/s", app.repeat_rate),
                ),
                row_stepper(
                    "Repeat Delay",
                    RowControlId::RepeatDelayDown,
                    RowControlId::RepeatDelayUp,
                    format!("{} ms", app.repeat_delay),
                ),
                RowSpec {
                    label: String::from("Keyboard Shortcuts..."),
                    height: 44,
                    control: CardRowControl::Navigation {
                        target: SettingsPage::Shortcuts,
                        leading: Some(SettingsPage::Shortcuts),
                    },
                },
            ],
        )],
        _ => Vec::new(),
    }
}

fn build_style_page_sections(
    app: &SettingsAppState,
    page: SettingsPage,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    match page {
        SettingsPage::Mouse => vec![place_card_section(
            Some("Pointer & Mouse"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Cursor Style",
                    RowControlId::CursorThemeCycle,
                    app.cursor_theme.clone(),
                ),
                row_info("Acceleration Profile", String::from("Adaptive (libinput)")),
            ],
        )],
        SettingsPage::Touchpad => vec![place_card_section(
            Some("Touchpad"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_info("Tap to Click", String::from("Enabled")),
                row_info("Two-Finger Scroll", String::from("Natural")),
            ],
        )],
        SettingsPage::Fonts => vec![place_card_section(
            Some("System Typography"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_info("UI Font", String::from("Inter Regular / SemiBold")),
                row_info("Monospace", String::from("JetBrains Mono")),
                row_info("Default Size", String::from("9 pt")),
            ],
        )],
        SettingsPage::IconTheme => vec![place_card_section(
            Some("Icon Theme"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_info("Active Theme", String::from("RustyOS System")),
                row_info("Dock Icons", String::from("Built-in Vector Squircles")),
            ],
        )],
        SettingsPage::CursorStyle => vec![place_card_section(
            Some("Cursor Style"),
            content_x,
            cur_y,
            content_w,
            vec![
                row_choice(
                    "Cursor Theme",
                    RowControlId::CursorThemeCycle,
                    app.cursor_theme.clone(),
                ),
                row_stepper(
                    "Cursor Size",
                    RowControlId::CursorSizeDown,
                    RowControlId::CursorSizeUp,
                    format!("{} px", app.cursor_size),
                ),
                row_info(
                    "Built-in Shapes",
                    String::from("14 HiDPI SVG Cursors (Default, Pointer, Text, Resize, Grab)"),
                ),
            ],
        )],
        _ => Vec::new(),
    }
}

pub(super) struct RowSpec {
    label: String,
    height: i32,
    control: CardRowControl,
}

pub(super) fn row_switch(label: &'static str, action: RowControlId, active: bool) -> RowSpec {
    RowSpec {
        label: label.to_owned(),
        height: 44,
        control: CardRowControl::Switch { action, active },
    }
}

pub(super) fn row_stepper(
    label: &'static str,
    down: RowControlId,
    up: RowControlId,
    value: String,
) -> RowSpec {
    RowSpec {
        label: label.to_owned(),
        height: 44,
        control: CardRowControl::Stepper { down, up, value },
    }
}

pub(super) fn row_choice(label: &'static str, action: RowControlId, value: String) -> RowSpec {
    RowSpec {
        label: label.to_owned(),
        height: 44,
        control: CardRowControl::Choice { action, value },
    }
}

pub(super) fn row_info(label: &'static str, value: String) -> RowSpec {
    RowSpec {
        label: label.to_owned(),
        height: 44,
        control: CardRowControl::Info { value },
    }
}

pub(super) fn row_info_string(label: String, value: String) -> RowSpec {
    RowSpec {
        label,
        height: 44,
        control: CardRowControl::Info { value },
    }
}

fn build_software_update_sections(
    app: &SettingsAppState,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
) -> Vec<CardSectionLayout> {
    let mut sections = Vec::new();

    // Section 1: Update Status
    let mut status_rows = Vec::new();
    if app.updates_checking {
        status_rows.push(row_info("Status", "Checking for updates...".to_owned()));
    } else if let Some(phase) = app.updates_tx_phase {
        let phase_str = phase.label();
        status_rows.push(row_info(
            "Progress",
            format!("{} ({:.0}%)", phase_str, app.updates_tx_progress * 100.0),
        ));
        if let Some(summary) = &app.updates_tx_summary {
            status_rows.push(row_info_string("Summary".to_owned(), summary.clone()));
        }
    } else if app.updates_snapshot.total_count > 0 {
        status_rows.push(row_info_string(
            "Available Updates".to_owned(),
            format!("{} pending packages", app.updates_snapshot.total_count),
        ));
        status_rows.push(row_choice(
            "Install",
            RowControlId::UpdatesInstallNow,
            "Update Now".to_owned(),
        ));
        status_rows.push(row_choice(
            "Refresh",
            RowControlId::UpdatesCheckNow,
            "Check Again".to_owned(),
        ));
    } else {
        status_rows.push(row_info(
            "System Status",
            "RustyOS is up to date".to_owned(),
        ));
        status_rows.push(row_info_string(
            "Last Checked".to_owned(),
            app.updates_snapshot.last_checked_label.clone(),
        ));
        status_rows.push(row_choice(
            "Check",
            RowControlId::UpdatesCheckNow,
            "Check for Updates".to_owned(),
        ));
    }

    sections.push(place_card_section(
        Some("Software Update"),
        content_x,
        cur_y,
        content_w,
        status_rows,
    ));

    // Section 2: Pending Packages List
    if app.updates_snapshot.total_count > 0 && app.updates_tx_phase.is_none() {
        let mut pkg_rows = Vec::new();
        for item in &app.updates_snapshot.items {
            let label = if item.is_rustyos {
                format!("{} [RustyOS]", item.name)
            } else if item.requires_restart {
                format!("{} [Core]", item.name)
            } else {
                item.name.clone()
            };
            pkg_rows.push(row_info_string(
                label,
                format!("{} -> {}", item.old_version, item.new_version),
            ));
        }
        if !pkg_rows.is_empty() {
            sections.push(place_card_section(
                Some("Pending Packages"),
                content_x,
                cur_y,
                content_w,
                pkg_rows,
            ));
        }
    }

    // Section 3: Preferences
    let auto_rows = vec![row_switch(
        "Check Automatically",
        RowControlId::UpdatesEnabledToggle,
        app.updates_enabled,
    )];
    sections.push(place_card_section(
        Some("Preferences"),
        content_x,
        cur_y,
        content_w,
        auto_rows,
    ));

    sections
}

pub(super) fn place_card_section(
    title: Option<&str>,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    specs: Vec<RowSpec>,
) -> CardSectionLayout {
    place_card_section_with_gap(title, content_x, cur_y, content_w, specs, 20)
}

pub(super) fn place_card_section_with_gap(
    title: Option<&str>,
    content_x: i32,
    cur_y: &mut i32,
    content_w: i32,
    specs: Vec<RowSpec>,
    bottom_gap: i32,
) -> CardSectionLayout {
    let section_label = title.map(|t| {
        let lbl_rect = Rect::new(content_x + 16, *cur_y, 260, 18);
        *cur_y += 22;
        (t.to_owned(), lbl_rect)
    });
    let card_h: i32 = specs.iter().map(|s| s.height).sum::<i32>().max(44);
    let card_rect = Rect::new(content_x, *cur_y, content_w, card_h);
    let mut rows = Vec::with_capacity(specs.len());
    let mut ry = card_rect.y;
    for spec in specs {
        let rh = spec.height;
        rows.push(CardRowLayout {
            rect: Rect::new(content_x, ry, content_w, rh),
            label: spec.label,
            control: spec.control,
        });
        ry += rh;
    }
    *cur_y = card_rect.bottom() + bottom_gap;
    CardSectionLayout {
        section_label,
        card_rect,
        rows,
    }
}
