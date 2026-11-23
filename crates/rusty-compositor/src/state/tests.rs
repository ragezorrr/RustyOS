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

    Ok(())
}

#[test]
fn persistent_config_save_reload_and_settings_toggles_roundtrip() -> Result<(), Box<dyn Error>> {
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
    std::fs::create_dir_all(&temp_dir)?;
    let cfg_path = temp_dir.join("config.toml");
    compositor.config_path = Some(cfg_path.clone());

    compositor.set_glass_opacity(0.83);
    compositor.toggle_reduce_transparency();
    compositor.set_reduced_motion_pref(true);
    compositor.menubar_show_workspaces = true;
    compositor.clock_24h = true;
    compositor.user_config.display_scale = 1.25;
    compositor.user_config.display_rotation = crate::config::DisplayRotation::Rotate90;
    compositor.user_config.vrr_mode = crate::config::VrrMode::Always;
    compositor.user_config.prefer_max_refresh = false;
    compositor.user_config.require_password_on_lock = false;
    compositor.user_config.lock_on_sleep = false;
    compositor.user_config.enable_polkit_agent = false;
    compositor.sync_and_save_user_config();

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

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn keyboard_settings_wallpaper_reload_and_menubar_click_roundtrip() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::{TopBarLayoutOptions, compute_top_bar_right_layout_with_options};

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

    let mut next_cfg = compositor.user_config.clone();
    next_cfg.keyboard_layout = "us,ua".to_owned();
    next_cfg.keyboard_options = "grp:caps_toggle".to_owned();
    next_cfg.repeat_rate = 30;
    next_cfg.repeat_delay = 150;
    next_cfg.menubar_show_input_source = true;
    compositor.apply_user_config(next_cfg);
    compositor.cycle_keyboard_layout();
    compositor.sync_and_save_user_config();

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
        show_updates: compositor.should_show_top_bar_updates(),
        ..TopBarLayoutOptions::default()
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

    std::fs::write(
        &cfg_path,
        format!("[ui]\nwallpaper = \"{}\"\n", wp_path.display()),
    )?;
    let rev_before = compositor.wallpaper_revision;
    compositor.handle_ipc_request(rusty_ipc::ShellRequest::ReloadConfig);
    assert_eq!(compositor.user_config.wallpaper.as_ref(), Some(&wp_path));
    assert!(compositor.wallpaper_revision > rev_before);

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn multi_output_settings_targets_each_display_independently_with_unplug_fallback()
-> Result<(), Box<dyn Error>> {
    use crate::config::{DisplayRotation, UserConfig};
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

    assert_eq!(compositor.output_map.entries().len(), 2);
    let mut next_cfg = compositor.user_config.clone();
    next_cfg.output_scales.insert("DP-1".to_owned(), 1.25);
    next_cfg
        .output_rotations
        .insert("DP-1".to_owned(), DisplayRotation::Rotate90);
    compositor.apply_user_config(next_cfg);
    compositor.sync_and_save_user_config();

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

    compositor.output_map.remove(dp1_id);
    assert_eq!(compositor.output_map.primary().id, primary_id);
    assert_eq!(compositor.output_map.entries().len(), 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn context_menu_desktop_folder_multi_monitor_dismiss_and_actions() -> Result<(), Box<dyn Error>> {
    use super::context_menu_actions::create_unique_desktop_folder;
    use crate::shell_render::ContextMenuAction;
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

    let ru_dir = temp_dir.join("Рабочий стол");
    let ru_f1 = create_unique_desktop_folder(&ru_dir)?;
    let ru_f2 = create_unique_desktop_folder(&ru_dir)?;
    assert_eq!(
        ru_f1.file_name().and_then(|n| n.to_str()),
        Some("Новая папка")
    );
    assert_eq!(
        ru_f2.file_name().and_then(|n| n.to_str()),
        Some("Новая папка 2")
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
    compositor.execute_context_menu_action(ContextMenuAction::DesktopChangeWallpaper, serial);
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
fn desktop_canvas_folder_creation_and_item_interaction() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::{
        ContextMenuAction, DOCK_HEIGHT, compute_desktop_item_rect, hit_test_desktop_item,
    };
    use crate::state::context_menu_actions::scan_desktop_canvas_items;
    use smithay::utils::{Point, Size};

    let temp_dir = std::env::temp_dir().join(format!(
        "rustyos-desktop-canvas-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;

    // 1. Create a folder, desktop entry and regular file
    let folder_path = temp_dir.join("Новая папка");
    std::fs::create_dir(&folder_path)?;

    let desktop_file_path = temp_dir.join("steam.desktop");
    std::fs::write(
        &desktop_file_path,
        "[Desktop Entry]\nType=Application\nName=Steam\nExec=/usr/bin/steam\nIcon=steam\n",
    )?;

    let txt_path = temp_dir.join("notes.txt");
    std::fs::write(&txt_path, "Hello RustyOS")?;

    // 2. Scan desktop canvas items
    let items = scan_desktop_canvas_items(&temp_dir);
    assert_eq!(items.len(), 3);

    // Folders come first
    assert!(items[0].is_dir);
    assert_eq!(items[0].name, "Новая папка");

    // Desktop entry is parsed
    let steam_item = items
        .iter()
        .find(|i| i.name == "Steam")
        .ok_or("missing steam item")?;
    assert!(!steam_item.is_dir);
    assert_eq!(steam_item.icon_name.as_deref(), Some("steam"));

    // 3. Layout and hit testing
    let screen_size = Size::from((1920, 1080));
    let rect0 = compute_desktop_item_rect(0, screen_size, DOCK_HEIGHT);
    assert!(rect0.loc.x > 1800);
    assert_eq!(rect0.loc.y, 46);

    let inside_pos = Point::from((f64::from(rect0.loc.x + 10), f64::from(rect0.loc.y + 10)));
    assert_eq!(
        hit_test_desktop_item(&items, screen_size, DOCK_HEIGHT, inside_pos),
        Some(0)
    );

    // 4. Test compositor context menu on desktop item
    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-desktop-canvas-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1920, 1080).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    compositor.desktop_items = items.clone();
    let item0 = compositor.desktop_items[0].clone();
    compositor.open_desktop_item_context_menu((1850.0, 60.0).into(), &item0);
    let ctx = compositor
        .context_menu
        .as_ref()
        .ok_or("desktop item context menu")?;
    assert!(ctx.entries.iter().any(|e| matches!(
        e,
        crate::shell_render::ContextMenuEntry::Item {
            action: ContextMenuAction::DesktopItemOpen(_),
            ..
        }
    )));
    assert!(ctx.entries.iter().any(|e| matches!(
        e,
        crate::shell_render::ContextMenuEntry::Item {
            action: ContextMenuAction::DesktopItemTrash(_),
            ..
        }
    )));

    // Verify grid slots assigned
    assert_eq!(items[0].slot, (0, 0));
    assert_eq!(items[1].slot, (0, 1));
    assert_eq!(items[2].slot, (0, 2));

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn desktop_grid_drag_drop_and_rubber_band() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::{
        DOCK_HEIGHT, DesktopDragState, DesktopRubberBand, cell_at_point, compute_desktop_cell_rect,
        load_desktop_grid_positions, save_desktop_grid_positions,
    };
    use smithay::utils::{Point, Size};
    use std::collections::HashMap;

    let screen_size = Size::from((1920, 1080));

    // 1. Grid geometry and slot calculation
    let cell_0_0 = compute_desktop_cell_rect(0, 0, screen_size);
    assert_eq!(cell_0_0.loc.x, 1920 - 24 - 88);
    assert_eq!(cell_0_0.loc.y, 46);

    let cell_0_1 = compute_desktop_cell_rect(0, 1, screen_size);
    assert_eq!(cell_0_1.loc.x, cell_0_0.loc.x);
    assert_eq!(cell_0_1.loc.y, 46 + 92 + 12);

    let cell_1_0 = compute_desktop_cell_rect(1, 0, screen_size);
    assert_eq!(cell_1_0.loc.x, cell_0_0.loc.x - 88 - 16);
    assert_eq!(cell_1_0.loc.y, 46);

    // 2. Cell at point hit-testing
    let pt_inside_0_0 = Point::from((
        f64::from(cell_0_0.loc.x + 10),
        f64::from(cell_0_0.loc.y + 10),
    ));
    assert_eq!(
        cell_at_point(pt_inside_0_0, screen_size, DOCK_HEIGHT),
        (0, 0)
    );

    // 3. Grid position persistence
    let temp_dir = std::env::temp_dir().join(format!("rustyos-grid-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;
    let toml_path = temp_dir.join("desktop_grid.toml");

    let mut saved = HashMap::new();
    saved.insert("FolderA".to_owned(), (0, 0));
    saved.insert("file.txt".to_owned(), (1, 2));
    save_desktop_grid_positions(&toml_path, &saved);

    let loaded = load_desktop_grid_positions(&toml_path);
    assert_eq!(loaded.get("FolderA"), Some(&(0, 0)));
    assert_eq!(loaded.get("file.txt"), Some(&(1, 2)));

    // 4. Rubber band selection rectangle
    let rb = DesktopRubberBand {
        start: Point::from((1800.0, 40.0)),
        current: Point::from((1900.0, 160.0)),
    };
    let rb_rect = rb.rect();
    assert!(rb_rect.size.w > 0 && rb_rect.size.h > 0);
    assert!(rb_rect.overlaps(cell_0_0));

    // 5. Drag and drop file to folder
    let folder_a = temp_dir.join("FolderA");
    std::fs::create_dir_all(&folder_a)?;
    let file_txt = temp_dir.join("file.txt");
    std::fs::write(&file_txt, "content")?;

    let drag = DesktopDragState {
        start_pointer: Point::from((100.0, 100.0)),
        current_pointer: Point::from((200.0, 200.0)),
        dragged_path: file_txt.clone(),
        is_active: true,
        target_hover_dir: Some(folder_a.clone()),
        drop_to_trash: false,
    };
    if let Some(dest_dir) = drag.target_hover_dir {
        let dest = dest_dir.join(drag.dragged_path.file_name().ok_or("file name")?);
        std::fs::rename(&drag.dragged_path, &dest)?;
        assert!(dest.exists());
        assert!(!file_txt.exists());
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn desktop_inline_rename_and_trash_workflow() -> Result<(), Box<dyn Error>> {
    use crate::icons::BuiltinAppIcon;
    use crate::shell_render::{DesktopInlineRename, append_utility_dock_tiles};

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-desktop-rename-trash-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    let temp_dir = std::env::temp_dir().join(format!("rustyos-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;

    // 1. Inline rename
    let old_file = temp_dir.join("OldName.txt");
    std::fs::write(&old_file, "text data")?;
    compositor.selected_desktop_items.insert(old_file.clone());

    compositor.desktop_inline_rename = Some(DesktopInlineRename {
        target_path: old_file.clone(),
        buffer: "NewDocument.txt".to_owned(),
        cursor_pos: 15,
    });
    compositor.commit_desktop_inline_rename();

    let new_file = temp_dir.join("NewDocument.txt");
    assert!(!old_file.exists());
    assert!(new_file.exists());
    assert!(compositor.selected_desktop_items.contains(&new_file));
    assert!(compositor.desktop_inline_rename.is_none());

    // 2. Reject invalid rename (slash or empty)
    compositor.desktop_inline_rename = Some(DesktopInlineRename {
        target_path: new_file.clone(),
        buffer: "bad/name.txt".to_owned(),
        cursor_pos: 12,
    });
    compositor.commit_desktop_inline_rename();
    assert!(new_file.exists());

    // 3. Dynamic Dock Trash icons based on trash_has_items
    let mut tiles_empty = Vec::new();
    append_utility_dock_tiles(&mut tiles_empty, false);
    let trash_tile_empty = tiles_empty
        .iter()
        .find(|t| t.app_key == "trash")
        .ok_or("trash tile")?;
    assert_eq!(trash_tile_empty.builtin_icon, Some(BuiltinAppIcon::Trash));

    let mut tiles_full = Vec::new();
    append_utility_dock_tiles(&mut tiles_full, true);
    let trash_tile_full = tiles_full
        .iter()
        .find(|t| t.app_key == "trash")
        .ok_or("trash tile")?;
    assert_eq!(
        trash_tile_full.builtin_icon,
        Some(BuiltinAppIcon::TrashFull)
    );

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
    let ctx = compositor.context_menu.as_ref().ok_or("missing app menu")?;
    assert!(has_action(ctx, &ContextMenuAction::DockFocusWindow(w1)));
    assert!(has_action(
        ctx,
        &ContextMenuAction::DockQuitApp(DockHit::WindowSlot(0))
    ));
    assert!(!has_action(
        ctx,
        &ContextMenuAction::DockForceQuitApp(DockHit::WindowSlot(0))
    ));

    compositor.sync_context_menu_alt_modifier(true);
    let ctx_alt = compositor.context_menu.as_ref().ok_or("missing alt menu")?;
    assert!(has_action(
        ctx_alt,
        &ContextMenuAction::DockForceQuitApp(DockHit::WindowSlot(0))
    ));

    compositor.sync_context_menu_alt_modifier(false);
    let ctx_rel = compositor
        .context_menu
        .as_ref()
        .ok_or("missing norm menu")?;
    assert!(has_action(
        ctx_rel,
        &ContextMenuAction::DockQuitApp(DockHit::WindowSlot(0))
    ));

    compositor.minimize_window(w1, serial);
    assert!(compositor.is_window_minimized(w1));
    compositor.open_dock_context_menu(
        DockHit::MinimizedWindow(w1),
        (1280, 800).into(),
        (900.0, 760.0).into(),
        false,
    );
    let min_ctx = compositor
        .context_menu
        .as_ref()
        .ok_or("missing minimized window menu")?;
    assert!(has_action(min_ctx, &ContextMenuAction::WindowQuit(w1)));
    compositor.sync_context_menu_alt_modifier(true);
    let min_alt = compositor
        .context_menu
        .as_ref()
        .ok_or("missing min alt menu")?;
    assert!(has_action(min_alt, &ContextMenuAction::WindowForceQuit(w1)));

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
fn context_menu_window_management_actions_and_tiling() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::{ContextMenuAction, ContextMenuEntry, ContextMenuTarget};

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-ctx-win-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    let w1 = compositor.wm.next_window_id();
    let _ = compositor.wm.map_window(w1, Some((600, 400)));
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();

    // Open window context menu at (100.0, 100.0)
    compositor.open_window_context_menu((100.0, 100.0).into(), w1);
    let ctx = compositor
        .context_menu
        .as_ref()
        .ok_or("missing window context menu")?;
    assert_eq!(ctx.target, ContextMenuTarget::Window(w1));

    // Verify tile left action presence
    assert!(ctx.entries.iter().any(|e| matches!(
        e,
        ContextMenuEntry::Item {
            action: ContextMenuAction::WindowTileLeft(id),
            ..
        } if *id == w1
    )));

    // Test Alt modifier dynamically toggling Close <-> Force Quit
    compositor.sync_context_menu_alt_modifier(true);
    let ctx_alt = compositor.context_menu.as_ref().ok_or("missing alt menu")?;
    assert!(ctx_alt.entries.iter().any(|e| matches!(
        e,
        ContextMenuEntry::Item {
            action: ContextMenuAction::WindowForceQuit(id),
            danger: true,
            ..
        } if *id == w1
    )));

    compositor.sync_context_menu_alt_modifier(false);
    let ctx_norm = compositor
        .context_menu
        .as_ref()
        .ok_or("missing normal menu")?;
    assert!(ctx_norm.entries.iter().any(|e| matches!(
        e,
        ContextMenuEntry::Item {
            action: ContextMenuAction::WindowQuit(id),
            danger: false,
            ..
        } if *id == w1
    )));

    // Execute WindowTileLeft
    compositor.execute_context_menu_action(ContextMenuAction::WindowTileLeft(w1), serial);
    assert!(compositor.context_menu.is_none());
    let geo_left = compositor
        .wm
        .window(w1)
        .map(|w| w.geometry)
        .ok_or("missing w1")?;
    assert_eq!(geo_left.x, compositor.wm.gap_px);

    // Execute WindowTileRight
    compositor.execute_context_menu_action(ContextMenuAction::WindowTileRight(w1), serial);
    let geo_right = compositor
        .wm
        .window(w1)
        .map(|w| w.geometry)
        .ok_or("missing w1")?;
    assert!(geo_right.x > geo_left.x);

    // Execute WindowToggleMaximize
    assert!(!compositor.wm.window(w1).ok_or("missing w1")?.maximized);
    compositor.execute_context_menu_action(ContextMenuAction::WindowToggleMaximize(w1), serial);
    assert!(compositor.wm.window(w1).ok_or("missing w1")?.maximized);

    // Toggle back
    compositor.execute_context_menu_action(ContextMenuAction::WindowToggleMaximize(w1), serial);
    assert!(!compositor.wm.window(w1).ok_or("missing w1")?.maximized);

    // Move to Workspace 2
    compositor.execute_context_menu_action(ContextMenuAction::WindowMoveToWorkspace(w1, 2), serial);
    assert_eq!(compositor.wm.window(w1).ok_or("missing w1")?.workspace.0, 2);

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

#[test]
fn top_bar_active_app_context_and_about_action_default_to_files() -> Result<(), Box<dyn Error>> {
    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-topbar-context-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    assert_eq!(
        compositor.active_top_bar_app_context(),
        (
            "Desktop".to_owned(),
            crate::shell_render::TopBarAppProfile::Desktop
        )
    );
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor.dispatch_top_bar_menu_action(
        crate::shell_render::TopBarMenuAction::AboutActiveApp,
        serial,
    );
    assert!(
        compositor
            .notifications
            .iter()
            .any(|n| n.title == "About Desktop")
    );
    Ok(())
}

#[test]
fn dock_keep_in_dock_recent_apps_and_minimize_modes_work() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::DockSection;

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-dock-073-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    compositor.toggle_app_keep_in_dock("notes");
    assert!(
        !compositor
            .user_config
            .dock_pinned_apps
            .iter()
            .any(|p| p == "notes")
    );
    compositor.toggle_app_keep_in_dock("notes");
    assert!(
        compositor
            .user_config
            .dock_pinned_apps
            .iter()
            .any(|p| p == "notes")
    );

    let w1 = compositor.wm.next_window_id();
    let _ = compositor.wm.map_window(w1, Some((640, 480)));
    let w1_key = compositor
        .collect_dock_window_records()
        .into_iter()
        .find(|r| r.id == w1)
        .map(|r| r.app_key)
        .ok_or("missing w1 dock record")?;
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor.minimize_window(w1, serial);
    assert!(compositor.is_window_minimized(w1));
    assert_eq!(compositor.minimize_anims.len(), 1);
    assert!(!compositor.minimize_anims[0].restoring);

    let tiles_sep = compositor.build_dock_tiles();
    assert!(
        tiles_sep
            .iter()
            .any(|t| t.section == DockSection::MinimizedWindow && t.representative_win == Some(w1))
    );
    compositor.user_config.dock_minimize_into_app = true;
    let tiles_into = compositor.build_dock_tiles();
    assert!(
        !tiles_into
            .iter()
            .any(|t| t.section == DockSection::MinimizedWindow)
    );
    assert!(
        tiles_into
            .iter()
            .any(|t| t.app_key == w1_key && t.has_minimized)
    );

    compositor.restore_window(w1, serial);
    assert!(!compositor.is_window_minimized(w1));
    assert_eq!(compositor.minimize_anims.len(), 1);
    assert!(compositor.minimize_anims[0].restoring);
    compositor.finish_window_teardown(w1);
    assert!(
        compositor
            .recent_unpinned_apps
            .iter()
            .any(|r| r.app_key == w1_key)
    );
    assert!(
        compositor
            .build_dock_tiles()
            .iter()
            .any(|t| t.section == DockSection::UnpinnedOrRecent && t.app_key == w1_key)
    );
    compositor.user_config.dock_show_recent = false;
    assert!(
        !compositor
            .build_dock_tiles()
            .iter()
            .any(|t| t.section == DockSection::UnpinnedOrRecent && t.app_key == w1_key)
    );
    Ok(())
}

#[test]
fn dock_pin_drag_reorder_remove_and_settings_config_work() -> Result<(), Box<dyn Error>> {
    use crate::config::{
        DockMinimizeEffect, DockPosition, DownloadsDisplayMode, DownloadsSortMode,
    };
    use crate::shell_render::ContextMenuAction;

    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-dock-drag-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    let _ = (&mut display, &mut event_loop);

    let initial_len = compositor.user_config.dock_pinned_apps.len();
    assert!(initial_len >= 4);
    let last_pin = compositor.user_config.dock_pinned_apps[initial_len - 1].clone();
    compositor.commit_dock_pin_reorder_or_remove(initial_len - 1, 2, false);
    assert_eq!(compositor.user_config.dock_pinned_apps[2], last_pin);

    compositor.commit_dock_pin_reorder_or_remove(0, 0, true);
    assert_eq!(compositor.user_config.dock_pinned_apps.len(), initial_len);
    let notes_idx = compositor
        .user_config
        .dock_pinned_apps
        .iter()
        .position(|p| p == "notes")
        .ok_or("missing notes pin")?;
    compositor.commit_dock_pin_reorder_or_remove(notes_idx, notes_idx, true);
    assert!(
        !compositor
            .user_config
            .dock_pinned_apps
            .iter()
            .any(|p| p == "notes")
    );

    compositor.user_config.dock_position = DockPosition::Left;
    compositor.user_config.dock_minimize_effect = DockMinimizeEffect::Scale;
    compositor.user_config.dock_auto_hide = true;
    compositor.sync_and_save_user_config();
    assert_eq!(compositor.user_config.dock_position, DockPosition::Left);
    assert_eq!(
        compositor.user_config.dock_minimize_effect,
        DockMinimizeEffect::Scale
    );
    assert!(compositor.user_config.dock_auto_hide);

    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor.execute_context_menu_action(
        ContextMenuAction::DownloadsSetSort(DownloadsSortMode::Kind),
        serial,
    );
    compositor.execute_context_menu_action(
        ContextMenuAction::DownloadsSetDisplay(DownloadsDisplayMode::Folder),
        serial,
    );
    assert_eq!(
        compositor.user_config.downloads_sort,
        DownloadsSortMode::Kind
    );
    assert_eq!(
        compositor.user_config.dock_downloads_display,
        DownloadsDisplayMode::Folder
    );
    Ok(())
}

#[test]
fn updates_popover_and_topbar_menu_workflow_persist_settings()
-> Result<(), Box<dyn std::error::Error>> {
    let mut display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "wayland-rustyos-updates-test".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        smithay::utils::Size::from((1280, 800)),
    )?;
    let _ = (&mut display, &mut event_loop);

    let cfg_path = std::env::temp_dir().join(format!(
        "rustyos-updates-state-test-{}.toml",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&cfg_path);
    compositor.config_path = Some(cfg_path.clone());

    assert!(compositor.menubar_show_updates);
    assert!(compositor.updates_enabled);
    assert!(!compositor.should_show_top_bar_updates());

    compositor.updates_status = rusty_services::UpdateStatusSnapshot {
        state: rusty_services::UpdateCheckState::Available,
        total_count: 3,
        arch_count: 2,
        rustyos_available: true,
        restart_recommended: true,
        last_checked_label: "10:42".to_owned(),
        error_hint: None,
        items: vec![rusty_services::PackageUpdateItem {
            name: "rustyos-git".to_owned(),
            old_version: "0.1.0-1".to_owned(),
            new_version: "0.1.1-1".to_owned(),
            is_rustyos: true,
            requires_restart: true,
        }],
    };
    assert!(compositor.should_show_top_bar_updates());

    let tel = compositor.telemetry.snapshot();
    let clock_text = if compositor.clock_24h {
        tel.clock_date_24h.as_str()
    } else {
        tel.clock_date_hhmm.as_str()
    };
    let right = crate::shell_render::compute_top_bar_right_layout_with_options(
        1280,
        clock_text,
        crate::shell_render::TopBarLayoutOptions {
            has_battery: tel.power.has_battery,
            notification_count: compositor.notification_count,
            show_volume: compositor.menubar_show_volume,
            show_input_source: compositor.menubar_show_input_source,
            show_workspaces: compositor.menubar_show_workspaces,
            show_tray: compositor.menubar_show_tray,
            show_updates: true,
            ..crate::shell_render::TopBarLayoutOptions::default()
        },
    );
    let ux = right.updates_x.ok_or("missing updates_x")?;
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor.pointer_location = (f64::from(ux + 10), 12.0).into();
    assert!(compositor.handle_top_bar_pointer_click(1280, serial, 272));
    assert!(compositor.updates_popover_open);

    compositor.close_updates_popover();
    assert!(!compositor.updates_popover_open);

    compositor.dispatch_top_bar_menu_action(
        crate::shell_render::TopBarMenuAction::CheckForUpdates,
        serial,
    );
    assert!(compositor.updates_popover_open);

    compositor.open_software_updater_action();
    assert!(!compositor.updates_popover_open);

    compositor.menubar_show_updates = false;
    compositor.updates_enabled = false;
    compositor.sync_and_save_user_config();
    let saved = crate::config::UserConfig::load_from_path(&cfg_path);
    assert!(!saved.updates_show_in_menu_bar);
    assert!(!saved.updates_enabled);
    assert!(!compositor.should_show_top_bar_updates());

    let _ = std::fs::remove_file(&cfg_path);
    Ok(())
}

#[test]
fn test_rustyos_liquid_glass_protocol_integration() -> Result<(), Box<dyn Error>> {
    let display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let compositor = RustyCompositorState::new(
        &display,
        "rusty-test-liquid-glass".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;

    assert!(!compositor.has_active_client_liquid_glass());
    assert!(compositor.active_liquid_glass_surfaces.is_empty());
    Ok(())
}

#[test]
fn firefox_window_merges_into_pinned_browser_dock_slot() -> Result<(), Box<dyn Error>> {
    use crate::shell_render::{DockHit, DockSection};

    let display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-firefox-dock".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    compositor
        .launcher
        .catalog
        .push(crate::launcher::LauncherApp {
            id: "firefox.desktop".to_owned(),
            name: "Firefox".to_owned(),
            subtitle: "Web Browser".to_owned(),
            exec: "/bin/sh %U".to_owned(),
            keywords: vec!["browser".to_owned()],
            badge: "FF".to_owned(),
            icon: Some("firefox".to_owned()),
        });
    compositor.recent_unpinned_apps.push(super::RecentDockApp {
        app_key: "firefox".to_owned(),
        label: "Firefox".to_owned(),
        icon_name: Some("firefox".to_owned()),
    });
    let tiles = compositor.build_dock_tiles();
    let pinned_browser = tiles
        .iter()
        .find(|t| t.section == DockSection::Pinned && t.hit == DockHit::PinnedBrowser)
        .ok_or("missing pinned browser slot")?;
    assert_eq!(pinned_browser.app_key, "browser");
    assert_eq!(pinned_browser.label, "Firefox");
    assert!(
        !tiles
            .iter()
            .any(|t| t.section == DockSection::UnpinnedOrRecent && t.app_key == "firefox"),
        "Firefox should merge into the pinned Browser Dock slot without duplicating"
    );
    assert_eq!(
        crate::shell_render::canonical_dock_app_key(
            Some("org.mozilla.firefox"),
            "Mozilla Firefox",
            rusty_wm::WindowId(1),
        ),
        "firefox"
    );
    Ok(())
}

#[test]
fn reactive_calloop_ping_waker_wakes_event_loop() -> Result<(), Box<dyn Error>> {
    let display: Display<RustyCompositorState> = Display::new()?;
    let mut event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-ping-waker".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;

    // Calling wake_event_loop with no loop_waker should not panic.
    assert!(compositor.loop_waker.is_none());
    compositor.wake_event_loop();

    // Create a ping source and register it.
    let (ping, ping_source) = smithay::reexports::calloop::ping::make_ping()?;
    compositor.loop_waker = Some(ping);

    let ping_fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ping_fired_cb = Arc::clone(&ping_fired);
    event_loop
        .handle()
        .insert_source(ping_source, move |_event, _metadata, _state| {
            ping_fired_cb.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .map_err(|err| format!("insert ping source failed: {err}"))?;

    // Wake the event loop via state method.
    compositor.wake_event_loop();

    // Dispatching the event loop should trigger the ping source immediately.
    event_loop.dispatch(Some(std::time::Duration::from_millis(50)), &mut compositor)?;
    assert!(ping_fired.load(std::sync::atomic::Ordering::SeqCst));

    Ok(())
}

#[test]
fn focused_window_reports_active_focus_for_keyboard_pacing() -> Result<(), Box<dyn Error>> {
    let display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-focus-pacing".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;

    // Initially no window has focus.
    assert!(compositor.wm.focused_window().is_none());

    // Create and map a mock window.
    let win_id = compositor.wm.next_window_id();
    let _ = compositor.wm.map_window(win_id, Some((800, 600)));

    // Window now has focus.
    assert_eq!(compositor.wm.focused_window(), Some(win_id));

    // When window is unmapped (e.g. closed).
    compositor.wm.unmap_window(win_id);
    assert!(compositor.wm.focused_window().is_none());

    Ok(())
}

#[test]
fn window_open_and_close_lifecycle_animations_respect_reduced_motion() -> Result<(), Box<dyn Error>>
{
    let display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-lifecycle-anims".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    compositor.animations.set_reduced_motion(false);

    let win_id = compositor.wm.next_window_id();
    let rect = compositor.wm.map_window(win_id, Some((920, 640)));
    compositor.register_window_open_animation(win_id, rect);
    assert_eq!(compositor.lifecycle_anims.len(), 1);
    assert_eq!(
        compositor.lifecycle_anims[0].kind,
        crate::state::WindowLifecycleKind::Open
    );
    assert!(compositor.lifecycle_anims[0].snapshot_pending);

    // Start close transition explicitly and advance beyond 160ms so it settles cleanly.
    compositor.start_window_close_animation(win_id, rect, true);
    assert_eq!(compositor.lifecycle_anims.len(), 1);
    assert_eq!(
        compositor.lifecycle_anims[0].kind,
        crate::state::WindowLifecycleKind::Close
    );
    assert!(compositor.has_active_compositor_animations());

    let future = compositor.elapsed() + std::time::Duration::from_millis(220);
    let _ = compositor.lifecycle_anims[0].progress.tick(future);
    assert!(!compositor.lifecycle_anims[0].progress.is_active());
    compositor.tick_animations();
    assert!(
        compositor.lifecycle_anims.is_empty(),
        "completed close animation must be pruned on tick"
    );

    // With reduced_motion enabled, open and close lifecycle animations are skipped immediately.
    compositor.animations.set_reduced_motion(true);
    compositor.register_window_open_animation(win_id, rect);
    compositor.start_window_close_animation(win_id, rect, true);
    assert!(compositor.lifecycle_anims.is_empty());

    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn smart_hide_multimonitor_occlusion_and_auto_hide_delay_isolate_outputs()
-> Result<(), Box<dyn Error>> {
    use crate::shell_render::DockHit;
    use smithay::output::{Mode, PhysicalProperties, Scale, Subpixel};

    let display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-smart-hide-mm".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;
    compositor.animations.set_reduced_motion(false);

    let primary_id = compositor.output_map.primary().id;
    let secondary_id = compositor.add_output(
        "DP-1",
        PhysicalProperties {
            size: (600, 340).into(),
            subpixel: Subpixel::Unknown,
            make: "RustyOS".to_owned(),
            model: "DP-1".to_owned(),
        },
        Mode {
            size: (1920, 1080).into(),
            refresh: 60_000,
        },
        Scale::Integer(1),
        (1280, 0).into(),
    );

    let mut cfg = compositor.user_config.clone();
    cfg.dock_visibility = crate::config::DockVisibilityMode::SmartHide;
    cfg.dock_auto_hide = false;
    compositor.apply_user_config(cfg);

    // No windows yet: SmartHide stays revealed on both outputs.
    assert!(!compositor.is_dock_obstructed_on_output(primary_id));
    assert!(!compositor.is_dock_obstructed_on_output(secondary_id));
    assert!(
        compositor
            .output_docks
            .get(&primary_id)
            .is_some_and(|d| (d.reveal_anim.target() - 1.0).abs() < 1e-4)
    );
    assert!(
        compositor
            .output_docks
            .get(&secondary_id)
            .is_some_and(|d| (d.reveal_anim.target() - 1.0).abs() < 1e-4)
    );

    // Map and maximize a window on primary: only primary becomes obstructed and hides.
    let w1 = compositor.wm.next_window_id();
    let _ = compositor.wm.map_window(w1, Some((900, 600)));
    let _ = compositor
        .wm
        .apply_action(rusty_wm::WmAction::ToggleMaximizeFocused);
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    compositor.sync_space_with_wm(serial);

    assert!(compositor.is_dock_obstructed_on_output(primary_id));
    assert!(!compositor.is_dock_obstructed_on_output(secondary_id));
    assert!(
        compositor
            .output_docks
            .get(&primary_id)
            .is_some_and(|d| d.reveal_anim.target().abs() < 1e-4)
    );
    assert!(
        compositor
            .output_docks
            .get(&secondary_id)
            .is_some_and(|d| (d.reveal_anim.target() - 1.0).abs() < 1e-4)
    );

    // Edge hover on primary reveals primary Dock; leaving arms 200 ms hide delay before sliding out.
    compositor.set_dock_hovered_on_output(primary_id, true);
    assert!(
        compositor
            .output_docks
            .get(&primary_id)
            .is_some_and(
                |d| (d.reveal_anim.target() - 1.0).abs() < 1e-4 && d.hide_deadline.is_none()
            )
    );
    compositor.set_dock_hovered_on_output(primary_id, false);
    let deadline = compositor
        .output_docks
        .get(&primary_id)
        .and_then(|d| d.hide_deadline)
        .ok_or("expected 200ms auto-hide deadline on pointer leave")?;
    assert!(
        compositor
            .output_docks
            .get(&primary_id)
            .is_some_and(|d| (d.reveal_anim.target() - 1.0).abs() < 1e-4),
        "dock should stay open during the 200ms hide grace window"
    );
    compositor.tick_dock_states(deadline + std::time::Duration::from_millis(5));
    assert!(
        compositor
            .output_docks
            .get(&primary_id)
            .is_some_and(|d| d.hide_deadline.is_none() && d.reveal_anim.target().abs() < 1e-4)
    );

    // Dock context menu opened on secondary output anchors to secondary_id.
    compositor.open_dock_context_menu_on_output(
        secondary_id,
        DockHit::PinnedTrash,
        (1920, 1080).into(),
        (2200.0, 1040.0).into(),
        false,
    );
    assert_eq!(
        compositor.context_menu.as_ref().map(|c| c.output_id),
        Some(secondary_id)
    );

    Ok(())
}

#[test]
fn dock_cycles_three_windows_deterministically_and_escalates_graceful_quit()
-> Result<(), Box<dyn Error>> {
    use crate::shell_render::{ContextMenuAction, ContextMenuEntry, DockHit};

    let w1 = rusty_wm::WindowId(1);
    let w2 = rusty_wm::WindowId(2);
    let w3 = rusty_wm::WindowId(3);
    let ordered = [w1, w2, w3];

    // Cycling across 3 windows advances deterministically w1 -> w2 -> w3 -> w1.
    assert_eq!(
        RustyCompositorState::select_dock_app_target_window(&ordered, Some(w1), Some(w1)),
        Some(w2)
    );
    assert_eq!(
        RustyCompositorState::select_dock_app_target_window(&ordered, Some(w2), Some(w2)),
        Some(w3)
    );
    assert_eq!(
        RustyCompositorState::select_dock_app_target_window(&ordered, Some(w3), Some(w3)),
        Some(w1)
    );
    // Returning from another app restores the last-active window (MRU).
    assert_eq!(
        RustyCompositorState::select_dock_app_target_window(
            &ordered,
            Some(rusty_wm::WindowId(99)),
            Some(w2)
        ),
        Some(w2)
    );

    let display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-dock-cycle-quit".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;

    let mapped = compositor.wm.next_window_id();
    let _ = compositor.wm.map_window(mapped, Some((600, 400)));
    let app_key = compositor
        .dock_key_for_hit(DockHit::WindowSlot(0))
        .ok_or("expected unpinned window dock key")?;

    // Graceful Quit starts 3s grace window and escalates context menu to Force Quit.
    compositor.quit_dock_app(DockHit::WindowSlot(0), false);
    assert!(compositor.is_app_quit_pending(&app_key));
    let tiles = compositor.build_dock_tiles();
    let menu = compositor
        .build_dock_app_context_entries(DockHit::WindowSlot(0), &tiles, false)
        .ok_or("expected window dock menu")?;
    assert!(menu.iter().any(|entry| {
        matches!(
            entry,
            ContextMenuEntry::Item {
                action: ContextMenuAction::DockForceQuitApp(DockHit::WindowSlot(0)),
                danger: true,
                ..
            }
        )
    }));

    // Advancing past 3000 ms emits an OSD reminder while waiting for user Force Quit.
    let after_grace =
        compositor.elapsed() + super::DOCK_QUIT_GRACE + std::time::Duration::from_millis(50);
    compositor.tick_dock_states(after_grace);
    assert!(
        compositor
            .osd
            .as_ref()
            .is_some_and(|o| o.label.contains("Force Quit"))
    );

    compositor.quit_dock_app(DockHit::WindowSlot(0), true);
    assert!(!compositor.is_app_quit_pending(&app_key));

    Ok(())
}

#[test]
fn dock_pin_metadata_cache_survives_temporary_upgrade_disappearance() -> Result<(), Box<dyn Error>>
{
    use crate::shell_render::DockSection;

    let display: Display<RustyCompositorState> = Display::new()?;
    let event_loop: EventLoop<RustyCompositorState> = EventLoop::try_new()?;
    let mut compositor = RustyCompositorState::new(
        &display,
        "rusty-test-pin-grace".to_owned(),
        event_loop.get_signal(),
        event_loop.handle(),
        (1280, 800).into(),
    )?;

    compositor
        .user_config
        .dock_pinned_apps
        .push("upgradeapp".to_owned());
    compositor.dock_pin_meta_cache.insert(
        "upgradeapp".to_owned(),
        super::CachedDockPinMeta {
            icon_name: Some("upgrade-icon".to_owned()),
            label: "Upgrade App".to_owned(),
            command: Some("upgradeapp".to_owned()),
            missing_since: None,
        },
    );

    // Simulate desktop file briefly vanishing during package upgrade.
    compositor.refresh_dock_pin_metadata_cache();
    assert!(compositor.dock_pin_in_upgrade_grace("upgradeapp"));
    assert!(
        compositor
            .user_config
            .dock_pinned_apps
            .iter()
            .any(|p| p == "upgradeapp")
    );
    let tiles = compositor.build_dock_tiles();
    let tile = tiles
        .iter()
        .find(|t| t.section == DockSection::Pinned && t.app_key == "upgradeapp")
        .ok_or("pinned app must remain in Dock during upgrade grace")?;
    assert_eq!(tile.icon_name.as_deref(), Some("upgrade-icon"));
    assert_eq!(tile.label, "Upgrade App");

    Ok(())
}
