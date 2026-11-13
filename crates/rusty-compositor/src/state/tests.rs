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
