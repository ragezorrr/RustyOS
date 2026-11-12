//! Wayland layer-shell client integration for `rusty-shell` (`RUSTYOS-017`).
//!
//! Connects to `rusty-compositor` via Wayland protocol and maps native
//! `TopBar` and `Dock` surfaces using `zwlr_layer_shell_v1`.

use std::error::Error;
use std::fmt;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_seat,
    delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    backend::WaylandError,
    globals::{BindError, registry_queue_init},
    protocol::{wl_output, wl_seat, wl_shm, wl_surface},
};

use crate::DesktopShell;

/// Error during Wayland client connection or rendering.
#[derive(Debug)]
pub enum WaylandClientError {
    /// Failed to connect to Wayland display.
    Connect(wayland_client::ConnectError),
    /// Failed to initialize registry queue.
    Global(wayland_client::globals::GlobalError),
    /// Failed to bind required global protocol.
    Bind(BindError),
    /// Protocol or dispatch error.
    Dispatch(wayland_client::DispatchError),
    /// Socket flush error.
    Flush(WaylandError),
    /// SHM pool allocation failure.
    ShmPool(String),
}

impl fmt::Display for WaylandClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(err) => write!(f, "Wayland connection failed: {err}"),
            Self::Global(err) => write!(f, "Wayland globals initialization failed: {err}"),
            Self::Bind(err) => write!(f, "Wayland protocol bind failed: {err}"),
            Self::Dispatch(err) => write!(f, "Wayland dispatch failed: {err}"),
            Self::Flush(err) => write!(f, "Wayland flush failed: {err}"),
            Self::ShmPool(msg) => write!(f, "SHM pool allocation failed: {msg}"),
        }
    }
}

impl Error for WaylandClientError {}

impl From<wayland_client::ConnectError> for WaylandClientError {
    fn from(err: wayland_client::ConnectError) -> Self {
        Self::Connect(err)
    }
}

impl From<wayland_client::globals::GlobalError> for WaylandClientError {
    fn from(err: wayland_client::globals::GlobalError) -> Self {
        Self::Global(err)
    }
}

impl From<BindError> for WaylandClientError {
    fn from(err: BindError) -> Self {
        Self::Bind(err)
    }
}

impl From<wayland_client::DispatchError> for WaylandClientError {
    fn from(err: wayland_client::DispatchError) -> Self {
        Self::Dispatch(err)
    }
}

impl From<WaylandError> for WaylandClientError {
    fn from(err: WaylandError) -> Self {
        Self::Flush(err)
    }
}

/// Premultiplied ARGB8888 design token colors (`DESIGN.md` v2.0 "RustyOS Purple Glass").
pub mod colors {
    // 0xAARRGGBB in little-endian premultiplied byte representation: [B, G, R, A]
    pub const SURFACE_ELEVATED: [u8; 4] = [0x2C, 0x17, 0x1E, 0xF5]; // #20182E @ 96%
    pub const SURFACE_OVERLAY: [u8; 4] = [0x36, 0x1F, 0x28, 0xFA]; // #292037 @ 98%
    pub const BORDER_HIGHLIGHT: [u8; 4] = [0x38, 0x38, 0x38, 0x38]; // 22% white glass rim
    pub const BORDER_SUBTLE: [u8; 4] = [0x1F, 0x1F, 0x1F, 0x1F]; // 12% white
    pub const ACCENT_NEON: [u8; 4] = [0xFF, 0x6E, 0xA5, 0xFF]; // #A56EFF
    pub const ACCENT_MUTED: [u8; 4] = [0xB5, 0x50, 0x76, 0xFF]; // #7650B5
    pub const TEXT_BRIGHT: [u8; 4] = [0xFA, 0xF0, 0xF4, 0xFF]; // #F4F0FA
    pub const TEXT_MUTED: [u8; 4] = [0xB5, 0x9B, 0xA6, 0xFF]; // #A69BB5
    pub const TEXT_SUBTLE: [u8; 4] = [0x85, 0x6B, 0x76, 0xFF]; // #766B85
    pub const SUCCESS_GREEN: [u8; 4] = [0xA0, 0xD6, 0x83, 0xFF]; // #83D6A0
}

/// Internal state for SCTK event dispatch.
pub struct ShellClientData {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub seat_state: SeatState,
    pub compositor_state: CompositorState,
    pub shm: Shm,
    pub layer_shell: LayerShell,
    pub pool: SlotPool,

    pub top_bar_surface: Option<LayerSurface>,
    pub dock_surface: Option<LayerSurface>,
    pub launcher_surface: Option<LayerSurface>,
    pub top_bar_size: (u32, u32),
    pub dock_size: (u32, u32),
    pub launcher_size: (u32, u32),

    pub shell_state: DesktopShell,
    pub running: bool,
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for ShellClientData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellClientData")
            .field("top_bar_size", &self.top_bar_size)
            .field("dock_size", &self.dock_size)
            .field("running", &self.running)
            .finish_non_exhaustive()
    }
}

fn fill_rect(
    canvas: &mut [u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    color: [u8; 4],
) {
    for row in y..(y + h) {
        let row_offset = row * stride;
        for col in x..(x + w) {
            let px = row_offset + col * 4;
            if px + 4 <= canvas.len() {
                canvas[px..px + 4].copy_from_slice(&color);
            }
        }
    }
}

/// Renders `TopBar` into an ARGB8888 buffer slice according to `TopBarState`.
#[allow(clippy::too_many_lines)]
pub fn render_top_bar_pixels(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    top_bar: &crate::TopBarState,
) {
    let stride = (width * 4) as usize;
    let w = width as usize;
    let h = height as usize;

    // Background: surface_elevated (#20182E)
    fill_rect(canvas, stride, 0, 0, w, h, colors::SURFACE_ELEVATED);

    // Bottom border line: 1px subtle glass border
    if h > 1 {
        fill_rect(canvas, stride, 0, h - 1, w, 1, colors::BORDER_SUBTLE);
    }

    // Workspace pills (1..=9)
    let active_ws = top_bar.active_workspace;
    let mut x = 12;
    let pill_w = 26;
    let pill_h = 20;
    let pill_y = 6;

    for ws in 1..=9 {
        if ws == active_ws {
            fill_rect(
                canvas,
                stride,
                x,
                pill_y,
                pill_w,
                pill_h,
                colors::ACCENT_NEON,
            );
            // Active pip
            fill_rect(
                canvas,
                stride,
                x + (pill_w - 8) / 2,
                pill_y + pill_h - 3,
                8,
                2,
                colors::TEXT_BRIGHT,
            );
        } else {
            fill_rect(
                canvas,
                stride,
                x,
                pill_y,
                pill_w,
                pill_h,
                colors::SURFACE_OVERLAY,
            );
            fill_rect(
                canvas,
                stride,
                x + (pill_w - 6) / 2,
                pill_y + (pill_h - 2) / 2,
                6,
                2,
                colors::TEXT_SUBTLE,
            );
        }
        x += pill_w + 4;
    }

    // Layout indicator pill
    fill_rect(
        canvas,
        stride,
        x + 8,
        pill_y,
        56,
        pill_h,
        colors::SURFACE_OVERLAY,
    );
    let layout_accent = if top_bar.layout_mode == "master-stack" {
        colors::ACCENT_NEON
    } else {
        colors::TEXT_SUBTLE
    };
    fill_rect(canvas, stride, x + 8, pill_y, 3, pill_h, layout_accent);

    // Status pill on the right
    if w > 150 {
        let status_w = 130;
        let status_x = w - status_w - 12;
        fill_rect(
            canvas,
            stride,
            status_x,
            pill_y,
            status_w,
            pill_h,
            colors::SURFACE_OVERLAY,
        );
        fill_rect(
            canvas,
            stride,
            status_x,
            pill_y,
            status_w,
            1,
            colors::BORDER_SUBTLE,
        );
        // Green status dot
        fill_rect(
            canvas,
            stride,
            status_x + 8,
            pill_y + 6,
            8,
            8,
            colors::SUCCESS_GREEN,
        );
        // Status bars
        fill_rect(
            canvas,
            stride,
            status_x + 24,
            pill_y + 8,
            40,
            4,
            colors::TEXT_MUTED,
        );
        fill_rect(
            canvas,
            stride,
            status_x + 72,
            pill_y + 6,
            46,
            8,
            colors::ACCENT_MUTED,
        );
    }
}

/// Renders `Dock` into an ARGB8888 buffer slice according to `DockItem` entries.
pub fn render_dock_pixels(canvas: &mut [u8], width: u32, height: u32, dock: &[crate::DockItem]) {
    let stride = (width * 4) as usize;
    let w = width as usize;
    let h = height as usize;

    // Outer glass highlight rim
    fill_rect(canvas, stride, 0, 0, w, h, colors::BORDER_HIGHLIGHT);

    // Dock background body
    if w > 2 && h > 2 {
        fill_rect(canvas, stride, 1, 1, w - 2, h - 2, colors::SURFACE_ELEVATED);
    }

    // Dock item tiles
    let item_size = 40;
    let item_y = 8;
    let items_count = (3 + dock.len()).min((w.saturating_sub(16)) / 52);
    let mut item_x = 8;

    for i in 0..items_count {
        fill_rect(
            canvas,
            stride,
            item_x,
            item_y,
            item_size,
            item_size,
            colors::SURFACE_OVERLAY,
        );

        // Inner icon plate
        let inner_size = 28;
        let inner_offset = (item_size - inner_size) / 2;
        let icon_color = match i {
            0 => colors::ACCENT_NEON,
            1 => colors::ACCENT_MUTED,
            2 => colors::TEXT_SUBTLE,
            _ => colors::TEXT_MUTED,
        };
        fill_rect(
            canvas,
            stride,
            item_x + inner_offset,
            item_y + inner_offset,
            inner_size,
            inner_size,
            icon_color,
        );

        // Active indicator pip
        fill_rect(
            canvas,
            stride,
            item_x + (item_size - 8) / 2,
            h.saturating_sub(5),
            8,
            2,
            colors::ACCENT_NEON,
        );

        item_x += item_size + 12;
    }
}

/// Renders Spotlight application launcher into an ARGB8888 buffer slice.
#[allow(clippy::too_many_lines)]
pub fn render_launcher_pixels(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    launcher: &crate::AppLauncher,
) {
    let stride = (width * 4) as usize;
    let w = width as usize;
    let h = height as usize;

    // Outer glass highlight rim
    fill_rect(canvas, stride, 0, 0, w, h, colors::BORDER_HIGHLIGHT);

    // Modal background body (elevated glass surface)
    if w > 2 && h > 2 {
        fill_rect(canvas, stride, 1, 1, w - 2, h - 2, colors::SURFACE_ELEVATED);
    }

    // Search bar container
    let search_bar_h = 44;
    fill_rect(
        canvas,
        stride,
        16,
        16,
        w.saturating_sub(32),
        search_bar_h,
        colors::SURFACE_OVERLAY,
    );
    fill_rect(
        canvas,
        stride,
        16,
        16 + search_bar_h - 1,
        w.saturating_sub(32),
        1,
        colors::BORDER_SUBTLE,
    );

    // Search accent icon
    fill_rect(canvas, stride, 28, 26, 24, 24, colors::ACCENT_NEON);

    // Cursor indicator
    fill_rect(canvas, stride, 62, 26, 2, 24, colors::TEXT_BRIGHT);

    // Divider line
    fill_rect(
        canvas,
        stride,
        16,
        70,
        w.saturating_sub(32),
        1,
        colors::BORDER_SUBTLE,
    );

    // Filtered entries
    let entries = launcher.filtered_entries();
    let row_h = 44;
    let start_y = 80;
    let max_rows = 3.min(entries.len());

    for (i, entry) in entries.iter().take(max_rows).enumerate() {
        let row_y = start_y + i * (row_h + 4);
        let is_selected = i == 0;
        let bg_color = if is_selected {
            colors::SURFACE_OVERLAY
        } else {
            colors::SURFACE_ELEVATED
        };
        fill_rect(
            canvas,
            stride,
            16,
            row_y,
            w.saturating_sub(32),
            row_h,
            bg_color,
        );

        if is_selected {
            fill_rect(canvas, stride, 16, row_y, 3, row_h, colors::ACCENT_NEON);
        }

        // App icon badge
        let icon_color = match i {
            0 => colors::ACCENT_NEON,
            1 => colors::ACCENT_MUTED,
            _ => colors::TEXT_SUBTLE,
        };
        fill_rect(canvas, stride, 28, row_y + 10, 24, 24, icon_color);

        // App title and subtitle indicators
        let title_w = (entry.name.len() * 8).clamp(60, 180);
        fill_rect(
            canvas,
            stride,
            62,
            row_y + 12,
            title_w,
            8,
            colors::TEXT_BRIGHT,
        );
        fill_rect(canvas, stride, 62, row_y + 26, 90, 6, colors::TEXT_MUTED);
    }
}

impl ShellClientData {
    /// Renders `TopBar` into an ARGB8888 buffer slice.
    pub fn render_top_bar_pixels(&self, canvas: &mut [u8], width: u32, height: u32) {
        render_top_bar_pixels(canvas, width, height, &self.shell_state.top_bar);
    }

    /// Renders `Dock` into an ARGB8888 buffer slice.
    pub fn render_dock_pixels(&self, canvas: &mut [u8], width: u32, height: u32) {
        render_dock_pixels(canvas, width, height, &self.shell_state.dock);
    }

    /// Renders Spotlight application launcher into an ARGB8888 buffer slice.
    pub fn render_launcher_pixels(&self, canvas: &mut [u8], width: u32, height: u32) {
        render_launcher_pixels(canvas, width, height, &self.shell_state.launcher);
    }
}

// SCTK Delegate Implementations
impl ProvidesRegistryState for ShellClientData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(ShellClientData);
delegate_output!(ShellClientData);
delegate_compositor!(ShellClientData);
delegate_shm!(ShellClientData);
delegate_seat!(ShellClientData);
delegate_layer!(ShellClientData);

impl OutputHandler for ShellClientData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl CompositorHandler for ShellClientData {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_scale: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wayland_client::protocol::wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl ShmHandler for ShellClientData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for ShellClientData {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl LayerShellHandler for ShellClientData {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.running = false;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (mut width, mut height) = configure.new_size;

        if let Some(top_bar) = &self.top_bar_surface
            && top_bar == layer
        {
            if width == 0 {
                width = 1280;
            }
            if height == 0 {
                height = 32;
            }
            self.top_bar_size = (width, height);
        }

        if let Some(dock) = &self.dock_surface
            && dock == layer
        {
            if width == 0 {
                width = 240;
            }
            if height == 0 {
                height = 56;
            }
            self.dock_size = (width, height);
        }

        if let Some(launcher) = &self.launcher_surface
            && launcher == layer
        {
            if width == 0 {
                width = 560;
            }
            if height == 0 {
                height = 240;
            }
            self.launcher_size = (width, height);
        }
    }
}

/// Standalone Wayland Layer-Shell client managing `TopBar` and `Dock`.
pub struct ShellWaylandClient {
    pub conn: Connection,
    pub queue: wayland_client::EventQueue<ShellClientData>,
    pub data: ShellClientData,
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for ShellWaylandClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellWaylandClient")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl ShellWaylandClient {
    /// Connects to Wayland and initializes layer-shell globals and surfaces.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandClientError`] if the connection fails or required protocols are absent.
    pub fn connect(initial_shell: DesktopShell) -> Result<Self, WaylandClientError> {
        let conn = Connection::connect_to_env()?;
        let (globals, queue) = registry_queue_init(&conn)?;
        let qh = queue.handle();

        let compositor_state = CompositorState::bind(&globals, &qh)?;
        let layer_shell = LayerShell::bind(&globals, &qh)?;
        let shm = Shm::bind(&globals, &qh)?;
        let pool = SlotPool::new(1280 * 720 * 4, &shm)
            .map_err(|e| WaylandClientError::ShmPool(format!("{e:?}")))?;

        let mut data = ShellClientData {
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &qh),
            seat_state: SeatState::new(&globals, &qh),
            compositor_state,
            shm,
            layer_shell,
            pool,
            top_bar_surface: None,
            dock_surface: None,
            launcher_surface: None,
            top_bar_size: (1280, 32),
            dock_size: (240, 56),
            launcher_size: (560, 240),
            shell_state: initial_shell,
            running: true,
        };

        // Create TopBar layer surface
        let top_surface = data.compositor_state.create_surface(&qh);
        let top_layer = data.layer_shell.create_layer_surface(
            &qh,
            top_surface,
            Layer::Top,
            Some("topbar".to_owned()),
            None,
        );
        top_layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        top_layer.set_size(0, 32);
        top_layer.set_exclusive_zone(32);
        top_layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        top_layer.commit();
        data.top_bar_surface = Some(top_layer);

        // Create Dock layer surface
        let dock_surface = data.compositor_state.create_surface(&qh);
        let dock_layer = data.layer_shell.create_layer_surface(
            &qh,
            dock_surface,
            Layer::Bottom,
            Some("dock".to_owned()),
            None,
        );
        dock_layer.set_anchor(Anchor::BOTTOM);
        dock_layer.set_size(240, 56);
        dock_layer.set_margin(0, 0, 12, 0);
        dock_layer.set_exclusive_zone(68);
        dock_layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        dock_layer.commit();
        data.dock_surface = Some(dock_layer);

        // Create Spotlight Launcher layer surface (Layer::Overlay, centered)
        let launcher_surface = data.compositor_state.create_surface(&qh);
        let launcher_layer = data.layer_shell.create_layer_surface(
            &qh,
            launcher_surface,
            Layer::Overlay,
            Some("spotlight".to_owned()),
            None,
        );
        launcher_layer.set_anchor(Anchor::empty());
        launcher_layer.set_size(560, 240);
        launcher_layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        launcher_layer.commit();
        data.launcher_surface = Some(launcher_layer);

        Ok(Self { conn, queue, data })
    }

    /// Dispatches pending Wayland protocol events.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandClientError`] on socket read or protocol dispatch error.
    pub fn dispatch_pending(&mut self) -> Result<usize, WaylandClientError> {
        self.queue
            .dispatch_pending(&mut self.data)
            .map_err(Into::into)
    }

    /// Renders current `TopBar` and `Dock` frames into SHM buffers and commits them.
    ///
    /// # Errors
    ///
    /// Returns [`WaylandClientError`] if buffer allocation or commitment fails.
    #[allow(clippy::cast_possible_wrap)]
    pub fn draw_surfaces(&mut self) -> Result<(), WaylandClientError> {
        // Draw TopBar
        if let Some(top_bar) = &self.data.top_bar_surface {
            let (w, h) = self.data.top_bar_size;
            let stride = (w * 4) as i32;
            let (buffer, canvas) = self
                .data
                .pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
                .map_err(|e| WaylandClientError::ShmPool(format!("{e:?}")))?;

            render_top_bar_pixels(canvas, w, h, &self.data.shell_state.top_bar);
            top_bar.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
            top_bar.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
            top_bar.commit();
        }

        // Draw Dock
        if let Some(dock) = &self.data.dock_surface {
            let (w, h) = self.data.dock_size;
            let stride = (w * 4) as i32;
            let (buffer, canvas) = self
                .data
                .pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
                .map_err(|e| WaylandClientError::ShmPool(format!("{e:?}")))?;

            render_dock_pixels(canvas, w, h, &self.data.shell_state.dock);
            dock.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
            dock.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
            dock.commit();
        }

        // Draw Spotlight Launcher
        if let Some(launcher) = &self.data.launcher_surface {
            if self.data.shell_state.launcher.open {
                let (w, h) = self.data.launcher_size;
                let stride = (w * 4) as i32;
                let (buffer, canvas) = self
                    .data
                    .pool
                    .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
                    .map_err(|e| WaylandClientError::ShmPool(format!("{e:?}")))?;

                render_launcher_pixels(canvas, w, h, &self.data.shell_state.launcher);
                launcher.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
                launcher
                    .wl_surface()
                    .damage_buffer(0, 0, w as i32, h as i32);
                launcher.commit();
            } else {
                launcher.wl_surface().attach(None, 0, 0);
                launcher.commit();
            }
        }

        self.conn.flush()?;
        Ok(())
    }

    /// Synchronizes shell state and triggers a redraw.
    pub fn update_state(&mut self, state: DesktopShell) {
        self.data.shell_state = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_top_bar_pixels_generates_valid_surface_buffer() {
        let shell = DesktopShell::default();
        let (w, h) = (1280u32, 32u32);
        let mut buffer = vec![0u8; (w * h * 4) as usize];

        render_top_bar_pixels(&mut buffer, w, h, &shell.top_bar);

        // Verify background pixels match SURFACE_ELEVATED (#20182E)
        assert_eq!(buffer[0..4], colors::SURFACE_ELEVATED);
        // Verify bottom border line has subtle border
        let border_offset = ((h - 1) * w * 4) as usize;
        assert_eq!(
            buffer[border_offset..border_offset + 4],
            colors::BORDER_SUBTLE
        );
    }

    #[test]
    fn render_dock_pixels_generates_valid_glass_rim_and_tiles() {
        let shell = DesktopShell::default();
        let (w, h) = (240u32, 56u32);
        let mut buffer = vec![0u8; (w * h * 4) as usize];

        render_dock_pixels(&mut buffer, w, h, &shell.dock);

        // Verify top-left corner matches BORDER_HIGHLIGHT rim
        assert_eq!(buffer[0..4], colors::BORDER_HIGHLIGHT);
        // Verify interior pixel matches SURFACE_ELEVATED
        let interior_px = (w * 4 * 2 + 8) as usize;
        assert_eq!(
            buffer[interior_px..interior_px + 4],
            colors::SURFACE_ELEVATED
        );
    }

    #[test]
    fn render_launcher_pixels_generates_valid_modal_and_search_bar() {
        let shell = DesktopShell::default();
        let (w, h) = (560u32, 240u32);
        let mut buffer = vec![0u8; (w * h * 4) as usize];

        render_launcher_pixels(&mut buffer, w, h, &shell.launcher);

        // Verify top-left corner matches BORDER_HIGHLIGHT rim
        assert_eq!(buffer[0..4], colors::BORDER_HIGHLIGHT);
        // Verify interior pixel matches SURFACE_ELEVATED
        let interior_px = (w * 4 * 2 + 8) as usize;
        assert_eq!(
            buffer[interior_px..interior_px + 4],
            colors::SURFACE_ELEVATED
        );
    }
}
