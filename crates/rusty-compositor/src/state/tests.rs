use super::wayland_handlers::desktop_protocols::{ToplevelIconEntry, ToplevelIconSurfaceState};
use super::{ClientState, RustyCompositorState};
use smithay::reexports::{
    calloop::EventLoop,
    wayland_protocols::xdg::toplevel_icon::v1::server::xdg_toplevel_icon_v1::XdgToplevelIconV1,
    wayland_server::{
        Display, Resource,
        backend::{ClientId, Handle, ObjectData, ObjectId, protocol::Message},
        protocol::wl_buffer,
    },
};
use smithay::wayland::{
    buffer::BufferHandler, compositor::Cacheable, xdg_toplevel_icon::XdgToplevelIconUserData,
};
use std::error::Error;
use std::io::Read;
use std::os::unix::io::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

#[derive(Debug)]
struct TestDummyObjectData;

impl ObjectData<RustyCompositorState> for TestDummyObjectData {
    fn request(
        self: Arc<Self>,
        _handle: &Handle,
        _data: &mut RustyCompositorState,
        _client_id: ClientId,
        _msg: Message<ObjectId, OwnedFd>,
    ) -> Option<Arc<dyn ObjectData<RustyCompositorState>>> {
        None
    }

    fn destroyed(
        self: Arc<Self>,
        _handle: &Handle,
        _data: &mut RustyCompositorState,
        _client_id: ClientId,
        _object_id: ObjectId,
    ) {
    }
}

#[test]
fn xdg_toplevel_icon_destroy_before_buffer_destroy_and_slot_override_avoid_false_no_buffer()
-> Result<(), Box<dyn Error>> {
    let mut display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut dh = display.handle();
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-toplevel-icon-cleanup".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;

    let (server_stream, mut client_stream) = UnixStream::pair()?;
    client_stream.set_nonblocking(true)?;
    let client = dh.insert_client(server_stream, Arc::new(ClientState::default()))?;

    let buf_old = client
        .create_resource_from_objdata::<wl_buffer::WlBuffer, RustyCompositorState>(
            &dh,
            1,
            Arc::new(TestDummyObjectData),
        )?;
    let buf_new = client
        .create_resource_from_objdata::<wl_buffer::WlBuffer, RustyCompositorState>(
            &dh,
            1,
            Arc::new(TestDummyObjectData),
        )?;
    let icon = client
        .create_resource::<XdgToplevelIconV1, XdgToplevelIconUserData, RustyCompositorState>(
            &dh,
            1,
            XdgToplevelIconUserData::default(),
        )?;

    let mut entry = ToplevelIconEntry {
        resource: icon.clone(),
        immutable: true,
        icon_name: Some("utilities-terminal".to_owned()),
        buffers: Vec::new(),
    };
    entry.add_buffer_replacing_slot(buf_old.clone(), 16, 16, 1);
    entry.add_buffer_replacing_slot(buf_new.clone(), 16, 16, 1);
    let snapshot = entry.snapshot_surface_state();
    compositor.toplevel_icons.insert(icon.id(), entry);

    // Destroying superseded slot buffer while icon is alive must not raise no_buffer.
    compositor.buffer_destroyed(&buf_old);
    display.flush_clients()?;
    let mut wire_buf = [0_u8; 256];
    assert!(matches!(
        client_stream.read(&mut wire_buf),
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
    ));

    // Simulate xdg_toplevel_icon_v1.destroy() prior to wl_buffer.destroy() (Qt6 sequence).
    compositor.toplevel_icons.remove(&icon.id());
    compositor.buffer_destroyed(&buf_new);
    display.flush_clients()?;
    assert!(matches!(
        client_stream.read(&mut wire_buf),
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
    ));

    // Snapshot committed to double-buffered ToplevelIconSurfaceState preserves icon_name.
    let mut committed = ToplevelIconSurfaceState::default();
    let mut pending = snapshot;
    let next = pending.commit(&dh);
    next.merge_into(&mut committed, &dh);
    assert_eq!(committed.icon_name.as_deref(), Some("utilities-terminal"));
    assert_eq!(committed.buffers.len(), 1);

    Ok(())
}

#[test]
fn xdg_toplevel_icon_buffer_destroyed_while_icon_alive_raises_no_buffer()
-> Result<(), Box<dyn Error>> {
    let mut display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut dh = display.handle();
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-toplevel-icon-error".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;

    let (server_stream, mut client_stream) = UnixStream::pair()?;
    client_stream.set_nonblocking(true)?;
    let client = dh.insert_client(server_stream, Arc::new(ClientState::default()))?;

    let buf = client.create_resource_from_objdata::<wl_buffer::WlBuffer, RustyCompositorState>(
        &dh,
        1,
        Arc::new(TestDummyObjectData),
    )?;
    let icon = client
        .create_resource::<XdgToplevelIconV1, XdgToplevelIconUserData, RustyCompositorState>(
            &dh,
            1,
            XdgToplevelIconUserData::default(),
        )?;

    let mut entry = ToplevelIconEntry {
        resource: icon.clone(),
        immutable: false,
        icon_name: None,
        buffers: Vec::new(),
    };
    entry.add_buffer_replacing_slot(buf.clone(), 32, 32, 1);
    compositor.toplevel_icons.insert(icon.id(), entry);

    compositor.buffer_destroyed(&buf);
    display.flush_clients()?;

    let mut wire_buf = [0_u8; 512];
    let bytes = client_stream.read(&mut wire_buf)?;
    assert!(bytes > 0);
    let text = String::from_utf8_lossy(&wire_buf[..bytes]);
    assert!(text.contains("destroyed before the toplevel icon"));

    Ok(())
}

#[test]
fn fullscreen_unoccluded_bypass_yields_for_polkit_portal_and_pick_color_without_exposing_chrome()
-> Result<(), Box<dyn Error>> {
    let display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-fullscreen-modals".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;

    assert!(!compositor.has_foreground_layer_surfaces());

    let w1 = compositor.wm.next_window_id();
    let _ = compositor.wm.map_window(w1, Some((900, 600)));
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor.apply_wm_action(rusty_wm::WmAction::ToggleFullscreen, serial);
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), Some(w1));
    assert!(compositor.should_hide_shell_chrome_for_fullscreen());

    compositor.polkit_prompt = Some(super::PolkitPromptState {
        cookie: "cookie-1".to_owned(),
        action_id: "org.freedesktop.policykit.exec".to_owned(),
        message: "Authentication required".to_owned(),
        identity_user: "root".to_owned(),
        prompt_label: "Password".to_owned(),
        echo_on: false,
        checking: false,
        error_message: None,
        info_message: None,
    });
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), None);
    assert!(compositor.should_hide_shell_chrome_for_fullscreen());
    compositor.polkit_prompt = None;
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), Some(w1));

    compositor.portal_prompt = Some(super::PortalPromptState {
        request_id: 7,
        app_id: "org.example.Recorder".to_owned(),
        is_screencast: true,
        session_handle: Some("/org/freedesktop/portal/desktop/session/1".to_owned()),
        requested_target: 1,
        allowed_types: 1,
        include_cursor: true,
        cursor_mode_editable: true,
        persist_mode: 0,
        candidates: Vec::new(),
        selected: 0,
    });
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), None);
    assert!(compositor.should_hide_shell_chrome_for_fullscreen());
    compositor.portal_prompt = None;

    compositor.portal_pick_color = Some(super::PendingPickColorState {
        request_id: 8,
        app_id: "org.example.Picker".to_owned(),
    });
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), None);
    assert!(compositor.should_hide_shell_chrome_for_fullscreen());
    compositor.portal_pick_color = None;

    // Interactive shell overlay (Spotlight) explicitly exposes desktop chrome above fullscreen windows.
    compositor.handle_ipc_request(rusty_ipc::ShellRequest::ToggleLauncher);
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), None);
    assert!(!compositor.should_hide_shell_chrome_for_fullscreen());
    compositor.handle_ipc_request(rusty_ipc::ShellRequest::ToggleLauncher);
    compositor.handle_ipc_request(rusty_ipc::ShellRequest::SetReducedMotion { enabled: true });
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), Some(w1));
    assert!(compositor.should_hide_shell_chrome_for_fullscreen());

    // System Settings modal also overrides fullscreen chrome hiding and direct scanout.
    compositor.open_settings();
    assert!(compositor.settings_active);
    assert!(compositor.has_blocking_shell_overlay());
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), None);
    assert!(!compositor.should_hide_shell_chrome_for_fullscreen());
    compositor.close_settings();
    assert_eq!(compositor.active_unoccluded_fullscreen_window(), Some(w1));

    Ok(())
}

#[test]
fn persistent_config_save_reload_and_settings_toggles_roundtrip() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::{SettingsHit, SettingsTab};

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-settings-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        smithay::utils::Size::from((1280, 800)),
    )?;
    let _ = (&mut display, &mut event_loop);

    let temp_dir =
        std::env::temp_dir().join(format!("rustyos-state-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let cfg_path = temp_dir.join("config.toml");
    compositor.config_path = Some(cfg_path.clone());

    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor
        .dispatch_top_bar_menu_action(crate::shell_render::TopBarMenuAction::OpenSettings, serial);
    assert!(compositor.settings_active);

    for hit in [
        SettingsHit::SelectTab(SettingsTab::Appearance),
        SettingsHit::GlassOpacityUp,
        SettingsHit::ToggleReduceTransparency,
        SettingsHit::ToggleReducedMotion,
        SettingsHit::SelectTab(SettingsTab::MenuBar),
        SettingsHit::ToggleMenuWorkspaces,
        SettingsHit::ToggleClock24h,
        SettingsHit::SelectTab(SettingsTab::Displays),
        SettingsHit::ScaleUp,
        SettingsHit::CycleRotation,
        SettingsHit::CycleVrrMode,
        SettingsHit::TogglePreferMaxRefresh,
        SettingsHit::SelectTab(SettingsTab::Security),
        SettingsHit::ToggleRequirePassword,
        SettingsHit::ToggleLockOnSleep,
        SettingsHit::TogglePolkitAgent,
    ] {
        compositor.apply_settings_hit(hit);
    }

    assert_eq!(compositor.config_status_label, "Saved");
    let disk_cfg = crate::config::UserConfig::load_from_path(&cfg_path);
    assert!((disk_cfg.glass_opacity - 0.83).abs() < 0.01);
    assert!(disk_cfg.reduce_transparency && disk_cfg.reduced_motion);
    assert!(disk_cfg.menubar_show_workspaces && disk_cfg.clock_24h);
    assert!((disk_cfg.display_scale - 1.25).abs() < 1e-4);
    assert_eq!(
        disk_cfg.display_rotation,
        crate::config::DisplayRotation::Rotate90
    );
    assert_eq!(disk_cfg.vrr_mode, crate::config::VrrMode::Always);
    assert!(!disk_cfg.prefer_max_refresh && !disk_cfg.require_password_on_lock);
    assert!(!disk_cfg.lock_on_sleep && !disk_cfg.enable_polkit_agent);

    std::fs::write(
        &cfg_path,
        "[keyboard]\nlayout = \"us,ru\"\nrepeat_delay = 310\nrepeat_rate = 40\n\
         [ui]\nreduced_motion = false\nglass_opacity = 0.55\nreduce_transparency = false\n\
         menubar_show_workspaces = false\nmenubar_show_volume = true\nclock_24h = false\n\
         [display]\nscale = 1.5\nrotation = \"180\"\nvrr_mode = \"off\"\nprefer_max_refresh = true\n\
         [security]\nrequire_password_on_lock = true\nlock_on_sleep = true\nenable_polkit_agent = true\n",
    )?;
    compositor.handle_ipc_request(rusty_ipc::ShellRequest::ReloadConfig);
    assert_eq!(compositor.config_status_label, "Reloaded");
    assert!((compositor.glass_opacity - 0.55).abs() < 0.01);
    assert!(!compositor.reduce_transparency && !compositor.animations.reduced_motion());
    assert!(!compositor.menubar_show_workspaces && compositor.menubar_show_volume);
    assert!(!compositor.clock_24h);
    assert!((compositor.output_map.primary().scale - 1.5).abs() < 1e-4);
    assert_eq!(
        compositor.output_map.primary().rotation,
        crate::config::DisplayRotation::Rotate180
    );
    assert_eq!(compositor.user_config.vrr_mode, crate::config::VrrMode::Off);
    assert!(compositor.user_config.require_password_on_lock);

    compositor.apply_settings_hit(SettingsHit::Close);
    assert!(!compositor.settings_active);
    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn keyboard_settings_wallpaper_reload_and_menubar_click_roundtrip() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::{
        SettingsHit, SettingsTab, TopBarLayoutOptions, compute_top_bar_right_layout_with_options,
    };

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-kbd-settings-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        smithay::utils::Size::from((1280, 800)),
    )?;
    let _ = (&mut display, &mut event_loop);

    let temp_dir =
        std::env::temp_dir().join(format!("rustyos-state-kbd-wp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;
    let cfg_path = temp_dir.join("config.toml");
    let wp_path = temp_dir.join("custom.png");
    std::fs::write(&wp_path, b"stub-png")?;
    compositor.config_path = Some(cfg_path.clone());

    for hit in [
        SettingsHit::SelectTab(SettingsTab::Keyboard),
        SettingsHit::CycleKeyboardLayout,
        SettingsHit::CycleKeyboardOptions,
        SettingsHit::RepeatRateUp,
        SettingsHit::RepeatDelayDown,
        SettingsHit::CycleActiveInputSource,
        SettingsHit::ToggleMenuInputSource,
    ] {
        compositor.apply_settings_hit(hit);
    }

    let disk_cfg = crate::config::UserConfig::load_from_path(&cfg_path);
    assert_eq!(disk_cfg.keyboard_layout, "us,ua");
    assert_eq!(disk_cfg.keyboard_options, "grp:caps_toggle");
    assert_eq!(disk_cfg.repeat_rate, 30);
    assert_eq!(disk_cfg.repeat_delay, 150);
    assert!(disk_cfg.menubar_show_input_source);
    assert_eq!(compositor.active_kbd_layout, "UA");

    let tel = compositor.telemetry.snapshot();
    let clock_text = tel.clock_date_hhmm.as_str();
    let opts = TopBarLayoutOptions {
        has_battery: tel.power.has_battery,
        notification_count: compositor.notification_count,
        show_volume: compositor.menubar_show_volume,
        show_input_source: true,
        show_workspaces: compositor.menubar_show_workspaces,
        show_tray: compositor.menubar_show_tray,
    };
    let right = compute_top_bar_right_layout_with_options(1280, clock_text, opts);
    let kbd_x = f64::from(right.kbd_x.ok_or("missing TopBar kbd rect")? + 10);
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor.pointer_location = (kbd_x, 12.0).into();
    compositor.shell_redraw_needed = false;
    assert!(compositor.handle_top_bar_pointer_click(1280, serial, 272));
    assert_eq!(compositor.active_kbd_layout, "EN");
    assert!(compositor.shell_redraw_needed);

    assert!(compositor.handle_top_bar_pointer_click(1280, serial, 273));
    assert!(compositor.settings_active);
    assert_eq!(compositor.settings_tab, SettingsTab::Keyboard);

    std::fs::write(
        &cfg_path,
        format!("[ui]\nwallpaper = \"{}\"\n", wp_path.display()),
    )?;
    let rev_before = compositor.wallpaper_revision;
    compositor.handle_ipc_request(rusty_ipc::ShellRequest::ReloadConfig);
    assert_eq!(compositor.user_config.wallpaper.as_ref(), Some(&wp_path));
    assert!(compositor.wallpaper_revision > rev_before);

    compositor.apply_settings_hit(SettingsHit::ToggleWallpaper);
    assert_eq!(compositor.user_config.wallpaper, None);
    compositor.apply_settings_hit(SettingsHit::ToggleWallpaper);
    assert_eq!(compositor.user_config.wallpaper.as_ref(), Some(&wp_path));

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn multi_output_settings_targets_each_display_independently_with_unplug_fallback()
-> Result<(), Box<dyn Error>> {
    use crate::config::{DisplayRotation, UserConfig};
    use crate::shell_render::SettingsHit;
    use smithay::output::{Mode, PhysicalProperties, Scale, Subpixel};

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-multi-output-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    let temp_dir = std::env::temp_dir().join(format!("rustyos-multi-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;
    let cfg_path = temp_dir.join("config.toml");
    compositor.config_path = Some(cfg_path.clone());

    let primary_id = compositor.output_map.primary().id;
    let primary_name = compositor.output_map.primary().name.clone();
    let dp1_id = compositor.add_output(
        "DP-1",
        PhysicalProperties {
            size: (600, 340).into(),
            subpixel: Subpixel::Unknown,
            make: "RustyOS".to_owned(),
            model: "DP-1".to_owned(),
        },
        Mode {
            size: (2560, 1440).into(),
            refresh: 144_000,
        },
        Scale::Integer(1),
        (1280, 0).into(),
    );

    assert_eq!(compositor.selected_settings_output().id, primary_id);
    assert_eq!(compositor.settings_render_state().output_count, 2);
    assert_eq!(
        compositor.settings_render_state().target_connector,
        primary_name.as_str()
    );

    compositor.apply_settings_hit(SettingsHit::CycleTargetOutput);
    assert_eq!(compositor.selected_settings_output().id, dp1_id);
    let targeted = compositor.settings_render_state();
    assert_eq!(targeted.target_connector, "DP-1");
    assert_eq!(targeted.target_output_index, 1);
    assert_eq!(targeted.display_width_px, 2560);
    assert_eq!(targeted.display_height_px, 1440);
    assert_eq!(targeted.display_refresh_hz, 144);

    compositor.apply_settings_hit(SettingsHit::ScaleUp);
    compositor.apply_settings_hit(SettingsHit::CycleRotation);
    let dp1_entry = compositor.output_map.get(dp1_id).ok_or("missing DP-1")?;
    assert!((dp1_entry.scale - 1.25).abs() < f64::EPSILON);
    assert_eq!(dp1_entry.rotation, DisplayRotation::Rotate90);
    let primary_entry = compositor.output_map.primary();
    assert!((primary_entry.scale - 1.0).abs() < f64::EPSILON);
    assert_eq!(primary_entry.rotation, DisplayRotation::Rotate0);
    assert!((compositor.user_config.display_scale - 1.0).abs() < f64::EPSILON);
    assert_eq!(
        compositor.user_config.display_rotation,
        DisplayRotation::Rotate0
    );

    let reloaded = UserConfig::load_from_path(&cfg_path);
    assert!((reloaded.scale_for_output("DP-1") - 1.25).abs() < f64::EPSILON);
    assert_eq!(
        reloaded.rotation_for_output("DP-1", None),
        DisplayRotation::Rotate90
    );

    compositor.apply_settings_hit(SettingsHit::CycleTargetOutput);
    assert_eq!(compositor.selected_settings_output().id, primary_id);
    compositor.apply_settings_hit(SettingsHit::CycleTargetOutput);
    assert_eq!(compositor.selected_settings_output().id, dp1_id);
    compositor.output_map.remove(dp1_id);
    assert_eq!(compositor.selected_settings_output().id, primary_id);
    assert_eq!(compositor.settings_render_state().output_count, 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn context_menu_desktop_folder_multi_monitor_dismiss_and_actions() -> Result<(), Box<dyn Error>> {
    use super::context_menu_actions::create_unique_desktop_folder;
    use crate::shell_render::{ContextMenuAction, SettingsTab};
    use rusty_wm::LayoutMode;
    use smithay::output::{Mode, PhysicalProperties, Scale, Subpixel};

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-ctx-desktop-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    let temp_dir = std::env::temp_dir().join(format!("rustyos-ctx-desk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let f1 = create_unique_desktop_folder(&temp_dir)?;
    let f2 = create_unique_desktop_folder(&temp_dir)?;
    assert_eq!(f1.file_name().and_then(|n| n.to_str()), Some("New Folder"));
    assert_eq!(
        f2.file_name().and_then(|n| n.to_str()),
        Some("New Folder 2")
    );

    let dp1_id = compositor.add_output(
        "DP-1",
        PhysicalProperties {
            size: (600, 340).into(),
            subpixel: Subpixel::Unknown,
            make: "RustyOS".to_owned(),
            model: "DP-1".to_owned(),
        },
        Mode {
            size: (2560, 1440).into(),
            refresh: 144_000,
        },
        Scale::Integer(1),
        (1280, 0).into(),
    );

    compositor.open_desktop_context_menu((1500.0, 200.0).into());
    let ctx = compositor
        .context_menu
        .as_ref()
        .ok_or("missing DP-1 menu")?;
    assert_eq!(ctx.output_id, dp1_id);
    assert_eq!(ctx.anchor_local, (220, 200).into());

    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    // Outside PKM (`273`) closes old menu and returns `Some(false)` to allow immediate reopen.
    assert_eq!(
        compositor.handle_context_menu_pointer_click(273, (1300.0, 50.0).into(), serial),
        Some(false)
    );
    assert!(compositor.context_menu.is_none());

    compositor.open_desktop_context_menu((300.0, 300.0).into());
    assert_eq!(
        compositor.context_menu.as_ref().map(|c| c.output_id),
        Some(compositor.output_map.primary().id)
    );
    // Outside LKM (`272`) dismisses cleanly and consumes the click (`Some(true)`).
    assert_eq!(
        compositor.handle_context_menu_pointer_click(272, (10.0, 750.0).into(), serial),
        Some(true)
    );
    assert!(compositor.context_menu.is_none());

    compositor.execute_context_menu_action(ContextMenuAction::DesktopDisplaySettings, serial);
    assert!(compositor.settings_active);
    assert_eq!(compositor.settings_tab, SettingsTab::Displays);
    compositor.execute_context_menu_action(ContextMenuAction::DesktopChangeWallpaper, serial);
    assert_eq!(compositor.settings_tab, SettingsTab::Appearance);
    let initial_layout = compositor.wm.active_layout_mode();
    compositor.execute_context_menu_action(ContextMenuAction::DesktopToggleLayoutMode, serial);
    assert_ne!(compositor.wm.active_layout_mode(), initial_layout);
    assert!(matches!(
        compositor.wm.active_layout_mode(),
        LayoutMode::Floating | LayoutMode::MasterStack
    ));

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn context_menu_dock_window_alt_force_quit_and_open_at_login() -> Result<(), Box<dyn Error>> {
    use crate::config::UserConfig;
    use crate::shell_render::{ContextMenuAction, ContextMenuEntry, DockHit};

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-ctx-dock-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    let temp_dir = std::env::temp_dir().join(format!("rustyos-ctx-dock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;
    let cfg_path = temp_dir.join("config.toml");
    compositor.config_path = Some(cfg_path.clone());

    let w1 = compositor.wm.next_window_id();
    let _ = compositor.wm.map_window(w1, Some((640, 480)));
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();

    compositor.open_dock_context_menu(
        DockHit::WindowSlot(0),
        (1280, 800).into(),
        (640.0, 760.0).into(),
        false,
    );
    let has_action = |ctx: &crate::shell_render::ContextMenuState, target: &ContextMenuAction| {
        ctx.entries.iter().any(|entry| {
            matches!(
                entry,
                ContextMenuEntry::Item { action, .. } if action == target
            )
        })
    };
    let ctx = compositor
        .context_menu
        .as_ref()
        .ok_or("missing window menu")?;
    assert!(has_action(ctx, &ContextMenuAction::WindowQuit(w1)));
    assert!(!has_action(ctx, &ContextMenuAction::WindowForceQuit(w1)));

    compositor.sync_context_menu_alt_modifier(true);
    let ctx_alt = compositor.context_menu.as_ref().ok_or("missing alt menu")?;
    assert!(has_action(ctx_alt, &ContextMenuAction::WindowForceQuit(w1)));

    compositor.sync_context_menu_alt_modifier(false);
    let ctx_rel = compositor
        .context_menu
        .as_ref()
        .ok_or("missing norm menu")?;
    assert!(has_action(ctx_rel, &ContextMenuAction::WindowQuit(w1)));

    compositor.execute_context_menu_action(ContextMenuAction::WindowMinimize(w1), serial);
    assert!(compositor.is_window_minimized(w1));
    compositor.execute_context_menu_action(ContextMenuAction::WindowMoveToWorkspace(w1, 2), serial);
    assert!(!compositor.is_window_minimized(w1));
    assert_eq!(compositor.wm.window(w1).map(|w| w.workspace.0), Some(2));

    compositor.execute_context_menu_action(
        ContextMenuAction::DockToggleOpenAtLogin(DockHit::PinnedTerminal),
        serial,
    );
    assert_eq!(compositor.user_config.autostart.len(), 1);
    let reloaded = UserConfig::load_from_path(&cfg_path);
    assert_eq!(reloaded.autostart, compositor.user_config.autostart);

    compositor.execute_context_menu_action(
        ContextMenuAction::DockToggleOpenAtLogin(DockHit::PinnedTerminal),
        serial,
    );
    assert_eq!(compositor.user_config.autostart.len(), 0);

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn context_menu_downloads_sort_persists_toml_and_updates_items() -> Result<(), Box<dyn Error>> {
    use crate::config::{DownloadsSortMode, UserConfig};
    use crate::shell_render::{ContextMenuAction, ContextMenuEntry};

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-ctx-dl-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    let temp_dir = std::env::temp_dir().join(format!("rustyos-ctx-dl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let downloads_dir = temp_dir.join("Downloads");
    std::fs::create_dir_all(&downloads_dir)?;
    std::fs::write(downloads_dir.join("zeta.txt"), b"tiny")?;
    std::fs::write(downloads_dir.join("alpha.bin"), vec![42_u8; 4096])?;
    std::fs::write(downloads_dir.join(".hidden"), b"skip")?;
    let cfg_path = temp_dir.join("config.toml");
    compositor.config_path = Some(cfg_path.clone());

    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor.execute_context_menu_action(
        ContextMenuAction::DownloadsSetSort(DownloadsSortMode::Name),
        serial,
    );
    assert_eq!(
        compositor.user_config.downloads_sort,
        DownloadsSortMode::Name
    );
    assert_eq!(
        UserConfig::load_from_path(&cfg_path).downloads_sort,
        DownloadsSortMode::Name
    );

    let by_name =
        compositor.test_refresh_downloads_sort_and_entries(&downloads_dir, DownloadsSortMode::Name);
    let labels: Vec<&str> = by_name
        .iter()
        .filter_map(|e| match e {
            ContextMenuEntry::Item {
                label,
                action: ContextMenuAction::DownloadsOpenEntry(_),
                ..
            } => Some(label.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(labels, vec!["alpha.bin", "zeta.txt"]);

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn context_menu_trash_safe_cleanup_worker_and_drain() -> Result<(), Box<dyn Error>> {
    use super::context_menu_actions::{empty_trash_dir_safely, trash_item_count};

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-ctx-trash-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    let temp_dir = std::env::temp_dir().join(format!("rustyos-ctx-trash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let not_trash = temp_dir.join("NotTrash");
    std::fs::create_dir_all(&not_trash)?;
    assert!(empty_trash_dir_safely(&not_trash).is_err());

    let outside_dir = temp_dir.join("protected_target");
    std::fs::create_dir_all(&outside_dir)?;
    let sentinel = outside_dir.join("keep.txt");
    std::fs::write(&sentinel, b"must-survive")?;

    let trash_root = temp_dir.join("Trash");
    let files_dir = trash_root.join("files");
    let info_dir = trash_root.join("info");
    std::fs::create_dir_all(&files_dir)?;
    std::fs::create_dir_all(&info_dir)?;
    std::fs::write(files_dir.join("doc.txt"), b"bye")?;
    std::fs::create_dir_all(files_dir.join("folder"))?;
    std::fs::write(files_dir.join("folder/inner.txt"), b"nested")?;
    std::os::unix::fs::symlink(&outside_dir, files_dir.join("link_out"))?;
    std::fs::write(info_dir.join("doc.txt.trashinfo"), b"[Trash Info]")?;
    assert_eq!(trash_item_count(&trash_root), 3);

    compositor.spawn_empty_trash_worker_at(trash_root.clone());
    assert!(compositor.trash_empty_rx.is_some());
    for _ in 0..100 {
        compositor.drain_trash_empty_results();
        if compositor.trash_empty_rx.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(compositor.trash_empty_rx.is_none());
    assert_eq!(trash_item_count(&trash_root), 0);
    assert!(sentinel.exists());
    assert!(
        compositor
            .notifications
            .iter()
            .any(|n| n.title == "Trash Emptied")
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn dock_and_context_menu_route_file_management_to_rusty_files() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::{ContextMenuAction, DockHit};
    use smithay::utils::Serial;

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-files-routing-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    assert_eq!(
        RustyCompositorState::dock_preferred_candidates(DockHit::PinnedFiles),
        Some(["rusty-files"].as_slice())
    );
    assert_eq!(
        compositor
            .launcher
            .preferred_icon(&["rusty-files"], "fallback-icon"),
        "system-file-manager"
    );

    let temp_dir = std::env::temp_dir().join(format!("rustyos-files-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;
    compositor.config_path = Some(temp_dir.join("config.toml"));
    compositor.execute_context_menu_action(
        ContextMenuAction::DockToggleOpenAtLogin(DockHit::PinnedFiles),
        Serial::from(1),
    );
    assert_eq!(
        compositor.user_config.autostart,
        vec!["rusty-files".to_owned()]
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn has_active_compositor_animations_tracks_control_center_and_lock_screen()
-> Result<(), Box<dyn Error>> {
    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-shell-anim-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    assert!(!compositor.has_active_compositor_animations());

    compositor.toggle_quick_settings();
    assert!(compositor.quick_settings_anim.is_active());
    assert!(compositor.has_active_compositor_animations());
    compositor.quick_settings_anim = rusty_animation::AnimatedValue::new_settled(1.0);
    assert!(!compositor.has_active_compositor_animations());

    compositor.set_screen_locked(true, smithay::utils::SERIAL_COUNTER.next_serial());
    assert!(compositor.lock_screen_anim.is_active());
    assert!(compositor.has_active_compositor_animations());
    Ok(())
}
