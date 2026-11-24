//! Wayland CSD (`xdg_toplevel`) client event loop and SHM presentation for `rusty-files`
//! (`RUSTYOS-065`, ADR-0030).

use std::fmt;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusty_services::file_manager::ListingTarget;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region, Surface},
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::DataSourceHandler,
    },
    delegate_compositor, delegate_data_device, delegate_keyboard, delegate_output,
    delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_xdg_popup,
    delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle, ping},
        calloop_wayland_source::WaylandSource,
        csd_frame::WindowState,
        protocols::{
            wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
                Shape as CursorShape, WpCursorShapeDeviceV1,
            },
            xdg::shell::client::{
                xdg_positioner::{Anchor, ConstraintAdjustment, Gravity},
                xdg_toplevel::ResizeEdge,
            },
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{
            AxisScroll, BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler,
            cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgPositioner, XdgShell, XdgSurface,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle,
    backend::WaylandError,
    globals::registry_queue_init,
    protocol::{
        wl_data_device, wl_data_device_manager::DndAction, wl_data_source, wl_keyboard, wl_output,
        wl_pointer, wl_seat, wl_shm, wl_surface,
    },
};

use crate::layout::{
    FilesHit, FilesLayout, PopupCommand, PopupSurfaceSpec, ResizeEdgeZone, SIDEBAR_WIDTH,
    WINDOW_CORNER_RADIUS, compute_layout, compute_popup_surface_spec, hit_test, scroll_area_at,
};
use crate::liquid_glass::{GlassStyle, RustyosLiquidGlassManagerV1, RustyosSurfaceGlassV1};
use crate::render::{
    canvas::with_font_engine, render_popup_surface_pixels_with_style,
    render_window_pixels_with_options,
};
use crate::state::OpenPopupMenu;
use crate::state::{FilesAppState, ScrollTargetArea, SearchScope, StartupOptions};
use rusty_services::file_manager::UserFileLocations;

const DEFAULT_WIDTH: u32 = 1060;
const DEFAULT_HEIGHT: u32 = 660;
const POPUP_SHADOW_PAD: i32 = 8;

/// Wayland client error returned by `run`.
#[derive(Debug)]
pub(crate) enum WaylandAppError {
    Connect(wayland_client::ConnectError),
    Global(wayland_client::globals::GlobalError),
    Bind(wayland_client::globals::BindError),
    Dispatch(wayland_client::DispatchError),
    Backend(WaylandError),
    ShmPool(String),
    EventLoop(String),
    XdgProtocol(String),
}

impl fmt::Display for WaylandAppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "Wayland connect error: {e}"),
            Self::Global(e) => write!(f, "Wayland global error: {e}"),
            Self::Bind(e) => write!(f, "Wayland bind error: {e}"),
            Self::Dispatch(e) => write!(f, "Wayland dispatch error: {e}"),
            Self::Backend(e) => write!(f, "Wayland protocol error: {e}"),
            Self::ShmPool(e) => write!(f, "SHM pool error: {e}"),
            Self::EventLoop(e) => write!(f, "Event loop error: {e}"),
            Self::XdgProtocol(e) => write!(f, "XDG shell error: {e}"),
        }
    }
}

impl std::error::Error for WaylandAppError {}

impl From<wayland_client::ConnectError> for WaylandAppError {
    fn from(value: wayland_client::ConnectError) -> Self {
        Self::Connect(value)
    }
}
impl From<wayland_client::globals::GlobalError> for WaylandAppError {
    fn from(value: wayland_client::globals::GlobalError) -> Self {
        Self::Global(value)
    }
}
impl From<wayland_client::globals::BindError> for WaylandAppError {
    fn from(value: wayland_client::globals::BindError) -> Self {
        Self::Bind(value)
    }
}
impl From<wayland_client::DispatchError> for WaylandAppError {
    fn from(value: wayland_client::DispatchError) -> Self {
        Self::Dispatch(value)
    }
}
impl From<WaylandError> for WaylandAppError {
    fn from(value: WaylandError) -> Self {
        Self::Backend(value)
    }
}

struct ActiveWaylandPopup {
    popup: Popup,
    glass: Option<RustyosSurfaceGlassV1>,
    menu_key: OpenPopupMenu,
    spec: PopupSurfaceSpec,
    hover_idx: Option<usize>,
    last_rendered_hover: Option<usize>,
    configured: bool,
    configured_size: (u32, u32),
    buffers: Vec<Buffer>,
}

impl Drop for ActiveWaylandPopup {
    fn drop(&mut self) {
        if let Some(ref glass) = self.glass {
            glass.destroy();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderBackend {
    Gpu,
    Cpu,
}

struct ActiveWaylandDragSource {
    source: wl_data_source::WlDataSource,
    uri_list: Vec<u8>,
    plain_text: Vec<u8>,
    selected_action: DndAction,
}

pub(crate) fn encode_paths_uri_list(paths: &[std::path::PathBuf]) -> String {
    let mut out = String::new();
    for p in paths {
        out.push_str(&rusty_services::path_to_file_uri(p));
        out.push_str("\r\n");
    }
    out
}

#[allow(clippy::struct_excessive_bools)]
struct WaylandClientData {
    conn: Connection,
    qh: QueueHandle<Self>,
    loop_handle: LoopHandle<'static, Self>,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor_state: CompositorState,
    shm: Shm,
    data_device_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    clipboard: rusty_ui::WaylandClipboardBridge,
    active_dnd_source: Option<ActiveWaylandDragSource>,
    paste_ping: ping::Ping,
    last_input_serial: u32,
    xdg_shell: XdgShell,
    glass_manager: Option<RustyosLiquidGlassManagerV1>,
    window_glass: Option<RustyosSurfaceGlassV1>,
    last_glass_state: Option<(u32, u32, bool)>,
    pool: SlotPool,
    window: Option<Window>,
    gles: Option<rusty_ui::GlesSurfaceRenderer>,
    render_backend: RenderBackend,
    cpu_backbuf: Vec<u8>,
    frame_rev: u64,
    main_buffers: Vec<Buffer>,
    buffer_size: (u32, u32),
    frame_sent_at: Option<Instant>,
    active_popup: Option<ActiveWaylandPopup>,
    popup_trigger_serial: Option<u32>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shape_manager: Option<CursorShapeManager>,
    cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    pointer_enter_serial: u32,
    last_cursor_shape: Option<CursorShape>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,
    width: u32,
    height: u32,
    configured: bool,
    pointer_pos: (f64, f64),
    pointer_pressed: bool,
    click_streak: u8,
    last_click: Option<(Instant, FilesHit)>,
    last_title: String,
    app: FilesAppState,
    layout_cache: Option<FilesLayout>,
}

impl WaylandClientData {
    fn layout(&mut self) -> FilesLayout {
        let l = compute_layout(&self.app, self.width, self.height);
        self.app.last_icon_grid_cols = FilesAppState::icon_grid_columns_for_width(l.content_rect.w);
        self.app.last_content_w = l.content_rect.w;
        self.layout_cache = Some(l.clone());
        l
    }

    fn cached_or_layout(&mut self) -> FilesLayout {
        if let Some(l) = &self.layout_cache {
            return l.clone();
        }
        self.layout()
    }

    fn sync_window_glass_region(&mut self) {
        let Some(ref g) = self.window_glass else {
            return;
        };
        let next = (self.width, self.height, !self.app.reduce_transparency);
        if self.last_glass_state == Some(next) {
            return;
        }
        self.last_glass_state = Some(next);
        if let Ok(region) = Region::new(&self.compositor_state) {
            if !self.app.reduce_transparency {
                let wi = i32::try_from(self.width).unwrap_or(1);
                let hi = i32::try_from(self.height).unwrap_or(1);
                let glass_w = (SIDEBAR_WIDTH + WINDOW_CORNER_RADIUS).min(wi).max(1);
                region.add(0, 0, glass_w, hi.max(1));
            }
            g.set_region(Some(region.wl_region()));
        }
        g.set_corner_radius(u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14));
        g.set_style(GlassStyle::Sidebar);
        g.commit();
    }

    fn close_popup(&mut self) {
        if self.active_popup.is_some() || self.app.popup.is_some() {
            self.active_popup = None;
            self.app.popup = None;
            self.app.dirty = true;
        }
    }

    fn sync_popup_surface(&mut self) -> Result<(), WaylandAppError> {
        let Some(menu) = self.app.popup.clone() else {
            self.active_popup = None;
            return Ok(());
        };
        if !self.configured {
            return Ok(());
        }
        if let Some(ap) = &mut self.active_popup
            && ap.menu_key == menu
        {
            if ap.configured && ap.hover_idx != ap.last_rendered_hover {
                draw_popup_surface(&mut self.pool, ap, self.app.theme, self.app.accent_index)?;
            }
            return Ok(());
        }

        // Replace any previous popup before mapping the new native xdg_popup.
        self.active_popup = None;
        let layout = self.layout();
        let spec = compute_popup_surface_spec(&self.app, &layout, &menu, POPUP_SHADOW_PAD);
        let Some(win) = &self.window else {
            return Ok(());
        };
        let positioner = XdgPositioner::new(&self.xdg_shell)
            .map_err(|e| WaylandAppError::XdgProtocol(format!("{e:?}")))?;
        positioner.set_size(spec.surface_width.max(1), spec.surface_height.max(1));
        positioner.set_anchor_rect(spec.anchor_x, spec.anchor_y, 1, 1);
        positioner.set_anchor(Anchor::TopLeft);
        positioner.set_gravity(Gravity::BottomRight);
        positioner.set_offset(-spec.shadow_pad, -spec.shadow_pad);
        positioner.set_constraint_adjustment(
            ConstraintAdjustment::FlipX
                | ConstraintAdjustment::FlipY
                | ConstraintAdjustment::SlideX
                | ConstraintAdjustment::SlideY,
        );

        let surface = Surface::new(&self.compositor_state, &self.qh)
            .map_err(|e| WaylandAppError::XdgProtocol(format!("{e:?}")))?;
        let popup = Popup::from_surface(
            Some(win.xdg_surface()),
            &positioner,
            &self.qh,
            surface,
            &self.xdg_shell,
        )
        .map_err(|e| WaylandAppError::XdgProtocol(format!("{e:?}")))?;

        let glass = self.glass_manager.as_ref().map(|mgr| {
            let g = mgr.get_surface_glass(popup.wl_surface(), &self.qh, ());
            if let Ok(region) = Region::new(&self.compositor_state) {
                let r = spec.layout.rect;
                region.add(r.x, r.y, r.w, r.h);
                g.set_region(Some(region.wl_region()));
            }
            g.set_corner_radius(u32::try_from(rusty_ui::menu::MENU_RADIUS).unwrap_or(14));
            g.set_style(GlassStyle::Popover);
            g.commit();
            g
        });

        if let (Some(seat), Some(serial)) = (&self.seat, self.popup_trigger_serial) {
            popup.xdg_popup().grab(seat, serial);
        }
        popup.wl_surface().commit();

        let req_w = u32::try_from(spec.surface_width.max(1)).unwrap_or(1);
        let req_h = u32::try_from(spec.surface_height.max(1)).unwrap_or(1);
        self.active_popup = Some(ActiveWaylandPopup {
            popup,
            glass,
            menu_key: menu,
            spec,
            hover_idx: None,
            last_rendered_hover: None,
            configured: false,
            configured_size: (req_w, req_h),
            buffers: Vec::new(),
        });
        Ok(())
    }

    fn handle_popup_pointer_event(&mut self, ev: &PointerEvent) {
        let Some(ap) = &mut self.active_popup else {
            return;
        };
        if ap.popup.wl_surface() != &ev.surface {
            return;
        }
        let (px, py) = ev.position;
        match ev.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                let next_hover = ap
                    .spec
                    .layout
                    .items
                    .iter()
                    .position(|item| item.command.is_some() && item.rect.contains_f64(px, py));
                if next_hover != ap.hover_idx {
                    ap.hover_idx = next_hover;
                    if ap.configured {
                        let _ = draw_popup_surface(
                            &mut self.pool,
                            ap,
                            self.app.theme,
                            self.app.accent_index,
                        );
                    }
                }
            }
            PointerEventKind::Leave { .. } => {
                if ap.hover_idx.is_some() {
                    ap.hover_idx = None;
                    if ap.configured {
                        let _ = draw_popup_surface(
                            &mut self.pool,
                            ap,
                            self.app.theme,
                            self.app.accent_index,
                        );
                    }
                }
            }
            PointerEventKind::Press {
                button: BTN_LEFT, ..
            } => {
                let hit_cmd = ap
                    .spec
                    .layout
                    .items
                    .iter()
                    .find(|item| item.rect.contains_f64(px, py))
                    .and_then(|item| item.command.clone());
                let inside_card = ap.spec.layout.rect.contains_f64(px, py);
                if let Some(cmd) = hit_cmd {
                    self.execute_popup_command(cmd);
                    let _ = self.sync_popup_surface();
                } else if !inside_card {
                    self.close_popup();
                    let _ = self.sync_popup_surface();
                }
            }
            PointerEventKind::Press {
                button: BTN_RIGHT, ..
            } => {
                self.close_popup();
                let _ = self.sync_popup_surface();
            }
            _ => {}
        }
    }

    fn execute_left_click(&mut self, px: f64, py: f64, serial: u32) {
        self.last_input_serial = serial;
        self.popup_trigger_serial = Some(serial);
        let prev_popup = self.app.popup.clone();
        if prev_popup.is_some() {
            self.close_popup();
        }

        let layout = self.layout();
        let Some(hit) = hit_test(&layout, px, py) else {
            return;
        };

        let now = Instant::now();
        let multi = self.last_click.as_ref().is_some_and(|(t, prev)| {
            now.duration_since(*t) <= Duration::from_millis(400) && prev == &hit
        });
        self.click_streak = if multi {
            self.click_streak.saturating_add(1).min(3)
        } else {
            1
        };
        let double_click = self.click_streak == 2;
        self.last_click = Some((now, hit.clone()));

        match hit {
            FilesHit::ResizeEdge(zone) => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.resize(seat, serial, map_resize_edge(zone));
                }
            }
            FilesHit::WindowDrag => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    win.move_(seat, serial);
                }
            }
            FilesHit::TrafficClose => self.app.request_quit(),
            FilesHit::TrafficMinimize => {
                if let Some(win) = &self.window {
                    win.set_minimized();
                }
            }
            FilesHit::TrafficMaximize => self.toggle_maximize(),
            other => self.handle_app_hit(other, double_click, &layout, px, py, prev_popup.as_ref()),
        }
    }

    fn toggle_maximize(&mut self) {
        let Some(win) = &self.window else {
            return;
        };
        let next = !self.app.is_maximized();
        if next {
            win.set_maximized();
        } else {
            win.unset_maximized();
        }
        self.app.set_maximized(next);
    }

    fn handle_app_hit(
        &mut self,
        hit: FilesHit,
        double_click: bool,
        layout: &FilesLayout,
        px: f64,
        py: f64,
        prev_popup: Option<&OpenPopupMenu>,
    ) {
        if !matches!(hit, FilesHit::InlineRenameBox) && self.app.inline_rename.is_some() {
            self.app.inline_rename = None;
            self.app.dirty = true;
        }
        match hit {
            FilesHit::NavBack => self.app.go_back(),
            FilesHit::NavForward => self.app.go_forward(),
            FilesHit::ViewMode(mode) => self.app.set_view_mode(mode),
            FilesHit::SortButton => self.toggle_popup(OpenPopupMenu::SortMenu, prev_popup),
            FilesHit::ActionMoreMenu => self.toggle_popup(OpenPopupMenu::ActionsMenu, prev_popup),
            FilesHit::ActionPrimary => self.toggle_popup(OpenPopupMenu::ShareMenu, prev_popup),
            FilesHit::ActionTagPicker => {
                let target_path = self
                    .app
                    .selected_entry()
                    .map(|e| e.path.clone())
                    .or_else(|| self.app.active_writable_directory());
                if let Some(path) = target_path {
                    self.toggle_popup(OpenPopupMenu::TagPicker { target_path: path }, prev_popup);
                }
            }
            FilesHit::SearchToggle => {
                let clicked_icon = px <= f64::from(layout.search_pill_rect.x + 26);
                if self.app.search_active && self.app.search_query.is_empty() && clicked_icon {
                    self.app.search_active = false;
                    self.app.set_search_scope(SearchScope::ThisFolder);
                } else {
                    self.app.search_active = true;
                    self.press_text_box(0, layout.search_pill_rect, px, py);
                }
                self.app.dirty = true;
            }
            FilesHit::SearchClear => self.app.set_search_query(String::new()),
            FilesHit::SearchScope(scope) => self.app.set_search_scope(scope),
            FilesHit::SearchScopeClear => {
                self.app.search_active = false;
                self.app.set_search_scope(SearchScope::ThisFolder);
                self.app.set_search_query(String::new());
            }
            FilesHit::EmptyTrashBanner => self.app.action_empty_trash(),
            FilesHit::SidebarSectionHeader(sec) => self.app.toggle_sidebar_section(sec),
            FilesHit::SidebarItem(loc) => self.app.navigate_sidebar(loc, true),
            FilesHit::SidebarRemovableMount(dev) => self.app.mount_or_open_removable_volume(&dev),
            FilesHit::SidebarRemovableEject(dev) => self.app.eject_removable_volume(&dev),
            FilesHit::ColumnRow { col_idx, row_idx } => {
                self.app.select_entry_with_modifiers(
                    col_idx,
                    row_idx,
                    self.modifiers.ctrl,
                    self.modifiers.shift,
                    double_click,
                );
            }
            FilesHit::ColumnEmpty { col_idx } => {
                self.app.popup = None;
                self.app.active_column = col_idx;
                if let Some(col) = self.app.columns.get_mut(col_idx) {
                    col.clear_selection();
                }
                self.app.columns.truncate(col_idx + 1);
                self.app.dirty = true;
            }
            FilesHit::ListHeaderSort(field) => self.app.set_sort(field),
            FilesHit::ListRow { row_idx } => {
                self.app.select_entry_with_modifiers(
                    0,
                    row_idx,
                    self.modifiers.ctrl,
                    self.modifiers.shift,
                    double_click,
                );
            }
            FilesHit::ListEmpty | FilesHit::IconsEmpty => {
                self.app.popup = None;
                if let Some(col) = self.app.columns.first_mut() {
                    col.clear_selection();
                }
                self.app.dirty = true;
            }
            FilesHit::PreviewOpen => self.app.activate_selected(),
            FilesHit::PreviewTrashOrRestore => self.app.action_move_selected_to_trash(),
            FilesHit::PreviewTag { path, color } => self.app.toggle_tag_on_path(&path, color),
            FilesHit::PopupItem(cmd) => self.execute_popup_command(cmd),
            FilesHit::PickerFilenameInput => {
                self.app.search_active = false;
                self.app.picker_filename_focused = true;
                self.app.sync_text_fields_from_strings();
                self.app.reset_caret_blink();
                self.app.dirty = true;
            }
            FilesHit::PickerCancel => self.app.cancel_file_picker(),
            FilesHit::PickerConfirm => self.app.confirm_file_picker(),
            other => {
                self.app.picker_filename_focused = false;
                self.handle_stage_b_hit(other, double_click, layout, px, py);
            }
        }
    }

    fn handle_stage_b_hit(
        &mut self,
        hit: FilesHit,
        double_click: bool,
        layout: &FilesLayout,
        px: f64,
        py: f64,
    ) {
        match hit {
            FilesHit::IconCell { idx } | FilesHit::GalleryThumb { idx } => {
                self.app.select_entry_with_modifiers(
                    0,
                    idx,
                    self.modifiers.ctrl,
                    self.modifiers.shift,
                    double_click,
                );
            }
            FilesHit::GalleryStageOpen => {
                if double_click {
                    self.app.activate_selected();
                }
            }
            FilesHit::GalleryShowMoreInfo => {
                self.app.gallery_more_info = !self.app.gallery_more_info;
                self.app.dirty = true;
            }
            FilesHit::GalleryQuickLookBtn => self.app.toggle_quick_look_selected(),
            FilesHit::GalleryInfoBtn => self.app.open_get_info_selected(),
            FilesHit::GalleryOpenBtn => self.app.activate_selected(),
            FilesHit::GalleryTrashBtn => self.app.action_move_selected_to_trash(),
            FilesHit::GalleryTag { path, color } => self.app.toggle_tag_on_path(&path, color),
            FilesHit::TabAdd => self.app.new_tab(None),
            FilesHit::TabSelect(idx) => self.app.switch_tab(idx),
            FilesHit::TabClose(idx) => self.app.close_tab(idx),
            FilesHit::PathCrumb(path) => self.app.navigate_to_directory(path),
            FilesHit::QuickLookClose => {
                self.app.quick_look = None;
                self.app.dirty = true;
            }
            FilesHit::QuickLookPrev => self.app.step_quick_look(-1),
            FilesHit::QuickLookNext => self.app.step_quick_look(1),
            FilesHit::QuickLookOpen => {
                self.app.quick_look = None;
                self.app.activate_selected();
            }
            FilesHit::GetInfoClose => {
                self.app.get_info = None;
                self.app.dirty = true;
            }
            FilesHit::GetInfoMoreToggle => {
                if let Some(gi) = &mut self.app.get_info {
                    gi.more_expanded = !gi.more_expanded;
                    self.app.dirty = true;
                }
            }
            FilesHit::GetInfoTag { color, .. } => {
                if let Some(target_path) = self.app.get_info.as_ref().map(|g| g.target_path.clone())
                {
                    self.app.toggle_tag_on_path(&target_path, color);
                }
            }
            FilesHit::GoToClose => {
                self.app.go_to_folder = None;
                self.app.dirty = true;
            }
            FilesHit::GoToClear => {
                if let Some(gt) = &mut self.app.go_to_folder {
                    gt.input.clear();
                    gt.suggestions.clear();
                    gt.selected_suggestion = 0;
                    gt.error = None;
                    self.app.goto_field.buffer.set_text("");
                    self.app.reset_caret_blink();
                    self.app.dirty = true;
                }
            }
            FilesHit::GoToModalBody => {
                if let Some(gt) = &layout.overlays.go_to_folder
                    && gt.input_rect.contains_f64(px, py)
                {
                    self.press_text_box(1, gt.input_rect, px, py);
                }
            }
            FilesHit::InlineRenameBox => {
                if let Some(ir) = layout.overlays.inline_rename_rect {
                    self.press_text_box(2, ir, px, py);
                }
            }
            FilesHit::GoToSuggestion(idx) => self.app.pick_go_to_suggestion(idx),
            FilesHit::TransferStop => self.app.cancel_active_transfer(),
            FilesHit::ConflictDecision(decision) => self.app.resolve_transfer_conflict(decision),
            _ => {}
        }
    }

    fn press_text_box(&mut self, which: u8, rect: crate::layout::Rect, px: f64, py: f64) {
        let ui_rect = rusty_ui::Rect::new(
            rect.x,
            rect.y,
            u32::try_from(rect.w.max(1)).unwrap_or(1),
            u32::try_from(rect.h.max(1)).unwrap_or(1),
        );
        self.app.sync_text_fields_from_strings();
        let click_n = self.click_streak;
        let elapsed = self.app.epoch.elapsed();
        let tf = match which {
            1 => &mut self.app.goto_field,
            2 => &mut self.app.rename_field,
            _ => &mut self.app.search_field,
        };
        tf.rect = ui_rect;
        with_font_engine(|fe| {
            let _ = tf.on_pointer_down(px, py, click_n, elapsed, fe);
        });
        self.app.reset_caret_blink();
        self.app.dirty = true;
    }

    fn start_wayland_file_drag(&mut self, sources: &[std::path::PathBuf]) -> bool {
        if sources.is_empty() {
            return false;
        }
        let (Some(ddm), Some(dd), Some(win)) =
            (&self.data_device_manager, &self.data_device, &self.window)
        else {
            return false;
        };
        let uri_list = encode_paths_uri_list(sources).into_bytes();
        let plain_text = sources
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        let drag_source = ddm.create_drag_and_drop_source(
            &self.qh,
            ["text/uri-list", "text/plain;charset=utf-8", "text/plain"],
            DndAction::Copy | DndAction::Move,
        );
        drag_source.start_drag(dd, win.wl_surface(), None, self.last_input_serial);
        let _ = self.conn.flush();
        self.active_dnd_source = Some(ActiveWaylandDragSource {
            source: drag_source.inner().clone(),
            uri_list,
            plain_text,
            selected_action: DndAction::None,
        });
        true
    }

    fn sync_clipboard_io(&mut self) {
        if let Some(text) = self.clipboard.take_pending_copy()
            && let (Some(ddm), Some(dd)) = (&self.data_device_manager, &self.data_device)
        {
            let source = ddm.create_copy_paste_source(&self.qh, rusty_ui::CLIPBOARD_MIME_TYPES);
            source.set_selection(dd, self.last_input_serial);
            self.clipboard.register_source(source.inner().clone(), text);
        }
        if self.clipboard.take_pending_paste_request()
            && let Some(dd) = &self.data_device
            && let Some(offer) = dd.data().selection_offer()
        {
            let mime = offer.with_mime_types(rusty_ui::WaylandClipboardBridge::pick_utf8_mime);
            if let Some(mime) = mime
                && let Ok(read_pipe) = offer.receive(mime)
            {
                let _ = self.conn.flush();
                let wake = self.paste_ping.clone();
                self.clipboard
                    .spawn_receive_from_fd(OwnedFd::from(read_pipe), move || wake.ping());
            }
        }
    }

    fn handle_key(&mut self, keysym: Keysym, utf8: Option<&str>) {
        if let Some(cached) = &self.layout_cache {
            self.app.last_icon_grid_cols =
                FilesAppState::icon_grid_columns_for_width(cached.content_rect.w);
        }
        if self.app.has_focused_text_input() {
            let nav_gt = self.app.go_to_folder.is_some()
                && matches!(
                    keysym,
                    Keysym::Up | Keysym::Down | Keysym::Tab | Keysym::ISO_Left_Tab
                );
            if !nav_gt && let Some(ui_key) = map_keysym_to_ui(keysym, utf8) {
                let mods = rusty_ui::InputModifiers {
                    ctrl: self.modifiers.ctrl,
                    shift: self.modifiers.shift,
                    alt: self.modifiers.alt,
                };
                let handled = with_font_engine(|fe| {
                    self.app
                        .handle_focused_text_key(ui_key, mods, fe, &mut self.clipboard)
                });
                self.sync_clipboard_io();
                if handled {
                    return;
                }
            }
        }
        let (key_name, ch_opt) = keysym_to_name_and_text(keysym, utf8);
        let mut buf = [0_u8; 4];
        let text_owned = utf8
            .filter(|s| !s.is_empty() && s.chars().all(|c| !c.is_control()))
            .map(str::to_owned)
            .or_else(|| ch_opt.map(|ch| ch.encode_utf8(&mut buf).to_owned()));
        self.app.handle_key_input(
            key_name,
            text_owned.as_deref(),
            self.modifiers.ctrl,
            self.modifiers.shift,
        );
    }

    fn update_active_text_drag(&mut self, px: f64) -> bool {
        if !self.pointer_pressed || !self.app.has_focused_text_input() {
            return false;
        }
        with_font_engine(|fe| {
            if self.app.inline_rename.is_some() {
                self.app.rename_field.on_pointer_move(px, fe)
            } else if self.app.go_to_folder.is_some() {
                self.app.goto_field.on_pointer_move(px, fe)
            } else if self.app.search_active {
                self.app.search_field.on_pointer_move(px, fe)
            } else {
                false
            }
        })
    }

    fn execute_right_click(&mut self, px: f64, py: f64, serial: u32) {
        self.popup_trigger_serial = Some(serial);
        let layout = self.layout();
        let hit = hit_test(&layout, px, py);

        let (entry, dir) = match hit {
            Some(FilesHit::ColumnRow { col_idx, row_idx }) => {
                self.app.select_entry(col_idx, row_idx, false);
                let e = self
                    .app
                    .columns
                    .get(col_idx)
                    .and_then(|c| c.visible_entries.get(row_idx))
                    .cloned();
                (e, self.app.active_writable_directory())
            }
            Some(
                FilesHit::ListRow { row_idx }
                | FilesHit::IconCell { idx: row_idx }
                | FilesHit::GalleryThumb { idx: row_idx },
            ) => {
                self.app.select_entry(0, row_idx, false);
                let e = self
                    .app
                    .columns
                    .first()
                    .and_then(|c| c.visible_entries.get(row_idx))
                    .cloned();
                (e, self.app.active_writable_directory())
            }
            Some(FilesHit::GalleryStageOpen) => (
                self.app.selected_entry().cloned(),
                self.app.active_writable_directory(),
            ),
            Some(FilesHit::ColumnEmpty { col_idx }) => {
                self.app.active_column = col_idx;
                (None, self.app.active_writable_directory())
            }
            Some(FilesHit::ListEmpty | FilesHit::IconsEmpty) => {
                (None, self.app.active_writable_directory())
            }
            None if layout.content_rect.contains_f64(px, py) => {
                (None, self.app.active_writable_directory())
            }
            _ => {
                if let (Some(win), Some(seat)) = (&self.window, &self.seat) {
                    #[allow(clippy::cast_possible_truncation)]
                    win.show_window_menu(seat, serial, (px.round() as i32, py.round() as i32));
                }
                self.close_popup();
                return;
            }
        };

        #[allow(clippy::cast_possible_truncation)]
        let (x, y) = (px.round() as i32, py.round() as i32);
        self.active_popup = None;
        self.app.popup = Some(OpenPopupMenu::ContextMenu {
            x,
            y,
            target_entry: entry,
            target_dir: dir,
        });
        self.app.dirty = true;
    }

    fn toggle_popup(&mut self, menu: OpenPopupMenu, prev_popup: Option<&OpenPopupMenu>) {
        if prev_popup == Some(&menu) {
            self.app.popup = None;
        } else {
            self.app.popup = Some(menu);
        }
        self.app.dirty = true;
    }

    fn apply_cursor_shape(&mut self, shape: CursorShape) {
        if self.last_cursor_shape == Some(shape) {
            return;
        }
        if let Some(dev) = &self.cursor_shape_device {
            dev.set_shape(self.pointer_enter_serial, shape);
            self.last_cursor_shape = Some(shape);
        }
    }

    fn update_cursor_shape(&mut self, hit: Option<&FilesHit>, layout: &FilesLayout) {
        let shape = match hit {
            Some(FilesHit::ResizeEdge(ResizeEdgeZone::Left | ResizeEdgeZone::Right)) => {
                CursorShape::EwResize
            }
            Some(FilesHit::ResizeEdge(ResizeEdgeZone::Top | ResizeEdgeZone::Bottom)) => {
                CursorShape::NsResize
            }
            Some(FilesHit::ResizeEdge(ResizeEdgeZone::TopLeft | ResizeEdgeZone::BottomRight)) => {
                CursorShape::NwseResize
            }
            Some(FilesHit::ResizeEdge(ResizeEdgeZone::TopRight | ResizeEdgeZone::BottomLeft)) => {
                CursorShape::NeswResize
            }
            Some(FilesHit::SearchToggle) if layout.search_pill_rect.w >= 72 => CursorShape::Text,
            Some(
                FilesHit::TrafficClose
                | FilesHit::TrafficMinimize
                | FilesHit::TrafficMaximize
                | FilesHit::NavBack
                | FilesHit::NavForward
                | FilesHit::ViewMode(_)
                | FilesHit::SortButton
                | FilesHit::ActionPrimary
                | FilesHit::ActionTagPicker
                | FilesHit::ActionMoreMenu
                | FilesHit::SearchToggle
                | FilesHit::SearchClear
                | FilesHit::SearchScope(_)
                | FilesHit::SearchScopeClear
                | FilesHit::EmptyTrashBanner
                | FilesHit::SidebarSectionHeader(_)
                | FilesHit::SidebarItem(_)
                | FilesHit::SidebarRemovableMount(_)
                | FilesHit::SidebarRemovableEject(_)
                | FilesHit::ListHeaderSort(_)
                | FilesHit::TabAdd
                | FilesHit::TabSelect(_)
                | FilesHit::TabClose(_)
                | FilesHit::PathCrumb(_)
                | FilesHit::PickerCancel
                | FilesHit::PickerConfirm,
            ) => CursorShape::Pointer,
            Some(FilesHit::PickerFilenameInput) => CursorShape::Text,
            _ => CursorShape::Default,
        };
        self.apply_cursor_shape(shape);
    }

    fn execute_popup_command(&mut self, cmd: PopupCommand) {
        self.active_popup = None;
        self.app.popup = None;
        match cmd {
            PopupCommand::SortBy(field) => self.app.set_sort(field),
            PopupCommand::SetSortOrder(order) => self.app.set_sort_order(order),
            PopupCommand::NewFolder => self.app.action_new_folder(),
            PopupCommand::NewTab => self.app.new_tab(None),
            PopupCommand::NewTabIn(path) => self.app.new_tab(Some(ListingTarget::Directory(path))),
            PopupCommand::OpenInTerminal(dir) => self.app.action_open_in_terminal(&dir),
            PopupCommand::ExtractArchive(path) => self.app.action_extract_archive(path),
            PopupCommand::CompressToZip(path) => self.app.action_compress_to_zip(&path),
            PopupCommand::ToggleTabBar => self.app.toggle_tab_bar(),
            PopupCommand::TogglePathBar => self.app.toggle_path_bar(),
            PopupCommand::ShareCopyPath(path) => {
                let text = path.display().to_string();
                self.app.share_copy_text(text, "Copied path");
            }
            PopupCommand::ShareCopyUri(path) => {
                let text = format!("file://{}", path.display());
                self.app.share_copy_text(text, "Copied link");
            }
            PopupCommand::QuickLook => self.app.toggle_quick_look_selected(),
            PopupCommand::GetInfoSelected => self.app.open_get_info_selected(),
            PopupCommand::GetInfoPath(path) => self.app.open_get_info_for_path(&path),
            PopupCommand::GoToFolder => self.app.open_go_to_folder(),
            PopupCommand::CopySelected => self.app.action_copy_selected(),
            PopupCommand::CutSelected => self.app.action_cut_selected(),
            PopupCommand::PasteIntoActive => self.app.action_paste_into_active(),
            PopupCommand::DuplicateSelected => self.app.action_duplicate_selected(),
            PopupCommand::RenameSelected => self.app.start_inline_rename_selected(),
            PopupCommand::ToggleShowHidden => self.app.toggle_show_hidden(),
            PopupCommand::ToggleTheme => self.app.toggle_theme(),
            PopupCommand::OpenEntry(entry) => {
                if entry.is_navigable_dir() && !self.app.is_trash_view() {
                    self.app.activate_selected();
                } else {
                    self.app.launch_entry(&entry);
                }
            }
            PopupCommand::RunExecutable(path) => {
                self.app.launch_executable_path(&path, true);
            }
            PopupCommand::RunInTerminal(path) => {
                self.app.action_run_in_terminal(&path);
            }
            PopupCommand::MoveToTrash(entry) => self.app.trash_entry(&entry),
            PopupCommand::RestoreEntry(entry) => self.app.restore_entry(&entry),
            PopupCommand::EmptyTrash => self.app.action_empty_trash(),
            PopupCommand::ToggleTag { path, color } => self.app.toggle_tag_on_path(&path, color),
            PopupCommand::ZoomWindow => {
                if let Some(win) = &self.window {
                    win.set_maximized();
                }
            }
            PopupCommand::MinimizeWindow => {
                if let Some(win) = &self.window {
                    win.set_minimized();
                }
            }
            PopupCommand::CloseWindow => {
                self.app.request_quit();
            }
        }
    }

    fn handle_axis_scroll(
        &mut self,
        px: f64,
        py: f64,
        horizontal: AxisScroll,
        vertical: AxisScroll,
        source: Option<wl_pointer::AxisSource>,
    ) {
        let layout = self.cached_or_layout();
        let (v_delta, v_smooth) = compute_axis_delta(vertical, source);
        let (h_delta, h_smooth) = compute_axis_delta(horizontal, source);

        let effective_h = if h_delta.abs() >= 0.01 {
            (h_delta, h_smooth)
        } else if self.modifiers.shift && v_delta.abs() >= 0.01 {
            (v_delta, v_smooth)
        } else {
            (0.0, false)
        };

        if effective_h.0.abs() >= 0.01 && layout.content_rect.contains_f64(px, py) {
            let extent = layout
                .gallery
                .as_ref()
                .map_or(layout.content_rect.w, |g| g.strip_rect.w);
            self.app
                .scroll_horizontal(effective_h.0, effective_h.1, extent.max(60));
            if self.app.view_mode == rusty_services::file_manager::FileViewMode::Columns {
                self.layout_cache = None;
            }
            if self.modifiers.shift {
                return;
            }
        }

        if v_delta.abs() < 0.01 {
            return;
        }
        let Some(area) = scroll_area_at(&layout, px, py) else {
            return;
        };
        let viewport_h = match area {
            ScrollTargetArea::Sidebar => layout.sidebar_clip_rect.h,
            ScrollTargetArea::List => layout.content_rect.h - 28,
            ScrollTargetArea::Icons | ScrollTargetArea::Column(_) => layout.content_rect.h,
            ScrollTargetArea::GalleryStrip => layout
                .gallery
                .as_ref()
                .map_or(layout.content_rect.w, |g| g.strip_rect.w),
        };
        self.app
            .scroll_axis(area, v_delta, v_smooth, viewport_h.max(60));
    }
}

fn compute_axis_delta(axis: AxisScroll, source: Option<wl_pointer::AxisSource>) -> (f64, bool) {
    if axis.discrete != 0 {
        return (f64::from(axis.discrete) * 44.0, true);
    }
    if axis.absolute.abs() < 0.01 {
        return (0.0, false);
    }
    let is_wheel = matches!(
        source,
        Some(wl_pointer::AxisSource::Wheel | wl_pointer::AxisSource::WheelTilt)
    ) || (source.is_none() && axis.absolute.abs() >= 10.0);
    if is_wheel {
        (axis.absolute * 2.8, true)
    } else {
        (axis.absolute * 1.25, false)
    }
}

const fn map_resize_edge(zone: ResizeEdgeZone) -> ResizeEdge {
    match zone {
        ResizeEdgeZone::Top => ResizeEdge::Top,
        ResizeEdgeZone::Bottom => ResizeEdge::Bottom,
        ResizeEdgeZone::Left => ResizeEdge::Left,
        ResizeEdgeZone::Right => ResizeEdge::Right,
        ResizeEdgeZone::TopLeft => ResizeEdge::TopLeft,
        ResizeEdgeZone::TopRight => ResizeEdge::TopRight,
        ResizeEdgeZone::BottomLeft => ResizeEdge::BottomLeft,
        ResizeEdgeZone::BottomRight => ResizeEdge::BottomRight,
    }
}

/// Connects to Wayland compositor, maps the RustyOS CSD window, and runs the event loop.
///
/// # Errors
/// Returns `WaylandAppError` on Wayland connection, protocol binding, or rendering failure.
#[allow(clippy::too_many_lines)]
pub(crate) fn run(options: &StartupOptions) -> Result<bool, WaylandAppError> {
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let compositor_state = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let pool = SlotPool::new((DEFAULT_WIDTH * DEFAULT_HEIGHT * 4 * 4) as usize, &shm)
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;

    let locations = UserFileLocations::detect();
    let app = FilesAppState::new(locations, options);
    let initial_title = app.window_title.clone();

    let surface = compositor_state.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestClient, &qh);
    window.set_title(&initial_title);
    window.set_app_id("org.rustyos.Files");
    window.set_min_size(Some((720, 440)));

    let glass_manager: Option<RustyosLiquidGlassManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let window_glass = glass_manager.as_ref().map(|mgr| {
        let g = mgr.get_surface_glass(window.wl_surface(), &qh, ());
        g.set_corner_radius(u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14));
        g.set_style(GlassStyle::Sidebar);
        g.commit();
        g
    });

    window.commit();

    let (paste_ping, paste_source) =
        ping::make_ping().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let mut event_loop: EventLoop<'static, WaylandClientData> =
        EventLoop::try_new().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    let loop_handle = event_loop.handle();
    insert_files_sources(&loop_handle, &conn, queue, paste_source)?;

    let no_gpu_env = std::env::var_os("RUSTYOS_NO_GPU").is_some_and(|v| v == "1");
    let render_backend = if options.no_gpu || no_gpu_env {
        RenderBackend::Cpu
    } else {
        RenderBackend::Gpu
    };

    let mut data = WaylandClientData {
        conn: conn.clone(),
        qh: qh.clone(),
        loop_handle,
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        compositor_state,
        shm,
        data_device_manager: DataDeviceManagerState::bind(&globals, &qh).ok(),
        data_device: None,
        clipboard: rusty_ui::WaylandClipboardBridge::new(),
        active_dnd_source: None,
        paste_ping,
        last_input_serial: 0,
        xdg_shell,
        glass_manager,
        window_glass,
        last_glass_state: None,
        pool,
        window: Some(window),
        gles: None,
        render_backend,
        cpu_backbuf: Vec::new(),
        frame_rev: 0,
        main_buffers: Vec::new(),
        buffer_size: (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        frame_sent_at: None,
        active_popup: None,
        popup_trigger_serial: None,
        seat: None,
        pointer: None,
        cursor_shape_manager: CursorShapeManager::bind(&globals, &qh).ok(),
        cursor_shape_device: None,
        pointer_enter_serial: 0,
        last_cursor_shape: None,
        keyboard: None,
        modifiers: Modifiers::default(),
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        configured: false,
        pointer_pos: (0.0, 0.0),
        pointer_pressed: false,
        click_streak: 1,
        last_click: None,
        last_title: initial_title,
        app,
        layout_cache: None,
    };

    while data.app.is_running() {
        let now = Instant::now();
        let _ = data.app.poll_background_loads();
        if data.app.poll_system_theme() {
            data.sync_window_glass_region();
        }
        let _ = data.app.update_caret_blink(now);

        if data.app.window_title != data.last_title
            && let Some(win) = &data.window
        {
            let title = format!("{} - RustyOS Files", data.app.window_title);
            win.set_title(&title);
            data.last_title.clone_from(&data.app.window_title);
        }

        data.sync_popup_surface()?;

        if let Some(sent_at) = data.frame_sent_at
            && now.saturating_duration_since(sent_at) >= Duration::from_millis(120)
        {
            data.frame_sent_at = None;
        }

        if data.configured
            && data.frame_sent_at.is_none()
            && (data.app.dirty
                || data.app.has_active_animations()
                || data.app.scrollbar_needs_tick(now))
        {
            let _ = data.app.tick_animations(now);
            draw_main_frame(&mut data)?;
            data.app.dirty = false;
        }
        conn.flush()?;

        let timeout = if data.app.has_pending_jobs() {
            Some(Duration::from_millis(8))
        } else if data.frame_sent_at.is_some() {
            Some(Duration::from_millis(24))
        } else {
            let caret = data
                .app
                .has_focused_text_input()
                .then(|| data.app.time_until_caret_blink(now));
            let scroll = data.app.time_until_scrollbar_tick(now);
            let next = match (caret, scroll) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let refresh_interval = Duration::from_millis(500);
            Some(next.map_or(refresh_interval, |d| d.min(refresh_interval)))
        };

        event_loop
            .dispatch(timeout, &mut data)
            .map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    }
    let ok =
        data.app.picker_mode.is_none() || (data.app.picker_confirmed && !data.app.picker_cancelled);
    Ok(ok)
}

fn acquire_reusable_buffer(
    pool: &mut SlotPool,
    buffers: &mut Vec<Buffer>,
    w: u32,
    h: u32,
) -> Result<usize, WaylandAppError> {
    let stride = i32::try_from(w * 4).unwrap_or(i32::MAX);
    let wi = i32::try_from(w).unwrap_or(1);
    let hi = i32::try_from(h).unwrap_or(1);
    if let Some(idx) = buffers
        .iter()
        .position(|b| !b.slot().has_active_buffers() && b.height() == hi && b.stride() == stride)
    {
        return Ok(idx);
    }
    let (buffer, _) = pool
        .create_buffer(wi, hi, stride, wl_shm::Format::Argb8888)
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
    buffers.push(buffer);
    Ok(buffers.len() - 1)
}

#[allow(clippy::cast_possible_wrap)]
fn try_draw_files_gles(data: &mut WaylandClientData, layout: &FilesLayout, w: u32, h: u32) -> bool {
    if data.gles.is_none() {
        return false;
    }
    let len = (w as usize) * (h as usize) * 4;
    if data.cpu_backbuf.len() != len {
        data.cpu_backbuf.resize(len, 0);
    }
    let sidebar_glass = data.window_glass.is_some() && !data.app.reduce_transparency;
    render_window_pixels_with_options(
        &mut data.cpu_backbuf,
        w,
        h,
        &data.app,
        layout,
        false,
        sidebar_glass,
    );
    data.frame_rev = data.frame_rev.wrapping_add(1);
    let rev = data.frame_rev;
    let pixels: Arc<[u8]> = Arc::from(data.cpu_backbuf.as_slice());
    let mut dl = rusty_ui::UiDrawList::new();
    {
        let mut painter = rusty_ui::UiPainter::new(&mut dl);
        painter.draw_image(
            rusty_ui::Rect::new(0, 0, w, h),
            rev,
            w,
            h,
            pixels,
            rusty_ui::PixelFormat::Bgra8888,
        );
    }
    if let Some(win) = &data.window {
        win.set_window_geometry(0, 0, w, h);
        win.wl_surface().frame(&data.qh, win.wl_surface().clone());
    }
    let wi = i32::try_from(w).unwrap_or(1);
    let hi = i32::try_from(h).unwrap_or(1);
    let res = if let Some(gles) = &mut data.gles {
        with_font_engine(|fe| gles.render_draw_list(&dl, fe, wi, hi, [0, 0, 0, 0]))
    } else {
        Ok(())
    };
    match res {
        Ok(()) => {
            data.frame_sent_at = Some(Instant::now());
            true
        }
        Err(err) => {
            tracing::warn!(%err, "rusty-files GLES2 swap failed; falling back to wl_shm");
            data.gles = None;
            data.render_backend = RenderBackend::Cpu;
            false
        }
    }
}

#[allow(clippy::cast_possible_wrap)]
fn draw_main_frame(data: &mut WaylandClientData) -> Result<(), WaylandAppError> {
    let layout = data.layout();
    if data.window.is_none() {
        return Ok(());
    }
    let w = data.width.max(1);
    let h = data.height.max(1);
    if try_draw_files_gles(data, &layout, w, h) {
        return Ok(());
    }
    let Some(win) = &data.window else {
        return Ok(());
    };
    if data.buffer_size != (w, h) {
        data.main_buffers.clear();
        data.buffer_size = (w, h);
    }
    let idx = acquire_reusable_buffer(&mut data.pool, &mut data.main_buffers, w, h)?;
    let buf_ref = &data.main_buffers[idx];
    let canvas = buf_ref
        .canvas(&mut data.pool)
        .ok_or_else(|| WaylandAppError::ShmPool(String::from("Main SHM slot unexpectedly busy")))?;

    let sidebar_glass = data.window_glass.is_some() && !data.app.reduce_transparency;
    render_window_pixels_with_options(canvas, w, h, &data.app, &layout, false, sidebar_glass);
    win.set_window_geometry(0, 0, w, h);
    buf_ref
        .attach_to(win.wl_surface())
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
    win.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
    win.wl_surface().frame(&data.qh, win.wl_surface().clone());
    win.commit();
    data.frame_sent_at = Some(Instant::now());
    Ok(())
}

#[allow(clippy::cast_possible_wrap)]
fn draw_popup_surface(
    pool: &mut SlotPool,
    ap: &mut ActiveWaylandPopup,
    theme: rusty_services::file_manager::FilesThemeMode,
    accent_index: usize,
) -> Result<(), WaylandAppError> {
    let (w, h) = (ap.configured_size.0.max(1), ap.configured_size.1.max(1));
    let idx = acquire_reusable_buffer(pool, &mut ap.buffers, w, h)?;
    let buf_ref = &ap.buffers[idx];
    let canvas = buf_ref.canvas(pool).ok_or_else(|| {
        WaylandAppError::ShmPool(String::from("Popup SHM slot unexpectedly busy"))
    })?;

    render_popup_surface_pixels_with_style(
        canvas,
        w,
        h,
        &ap.spec.layout,
        ap.hover_idx,
        theme,
        accent_index,
        ap.glass.is_some(),
    );
    ap.popup
        .xdg_surface()
        .set_window_geometry(0, 0, w as i32, h as i32);
    buf_ref
        .attach_to(ap.popup.wl_surface())
        .map_err(|e| WaylandAppError::ShmPool(format!("{e:?}")))?;
    ap.popup
        .wl_surface()
        .damage_buffer(0, 0, w as i32, h as i32);
    ap.popup.wl_surface().commit();
    ap.last_rendered_hover = ap.hover_idx;
    Ok(())
}

impl ProvidesRegistryState for WaylandClientData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(WaylandClientData);
delegate_output!(WaylandClientData);
delegate_compositor!(WaylandClientData);
delegate_shm!(WaylandClientData);
delegate_seat!(WaylandClientData);
delegate_keyboard!(WaylandClientData);
delegate_pointer!(WaylandClientData);
delegate_data_device!(WaylandClientData);
delegate_xdg_shell!(WaylandClientData);
delegate_xdg_window!(WaylandClientData);
delegate_xdg_popup!(WaylandClientData);

impl Dispatch<RustyosLiquidGlassManagerV1, ()> for WaylandClientData {
    fn event(
        _state: &mut Self,
        _proxy: &RustyosLiquidGlassManagerV1,
        _event: crate::liquid_glass::protocol::rustyos_liquid_glass_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RustyosSurfaceGlassV1, ()> for WaylandClientData {
    fn event(
        _state: &mut Self,
        _proxy: &RustyosSurfaceGlassV1,
        _event: crate::liquid_glass::protocol::rustyos_surface_glass_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl OutputHandler for WaylandClientData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl CompositorHandler for WaylandClientData {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self
            .window
            .as_ref()
            .is_some_and(|w| w.wl_surface() == surface)
        {
            self.frame_sent_at = None;
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl ShmHandler for WaylandClientData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for WaylandClientData {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if self.data_device.is_none()
            && let Some(ddm) = &self.data_device_manager
        {
            self.data_device = Some(ddm.get_data_device(qh, &seat));
        }
        if self.seat.is_none() {
            self.seat = Some(seat);
        }
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if self.seat.is_none() {
            self.seat = Some(seat.clone());
        }
        if cap == Capability::Pointer
            && self.pointer.is_none()
            && let Ok(ptr) = self.seat_state.get_pointer(qh, &seat)
        {
            self.cursor_shape_device = self
                .cursor_shape_manager
                .as_ref()
                .map(|m| m.get_shape_device(&ptr, qh));
            self.pointer = Some(ptr);
        }
        if cap == Capability::Keyboard
            && self.keyboard.is_none()
            && let Ok(kbd) = self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                Box::new(|state, _kbd, ev| {
                    state.handle_key(ev.keysym, ev.utf8.as_deref());
                }),
            )
        {
            self.keyboard = Some(kbd);
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Pointer
            && let Some(ptr) = self.pointer.take()
        {
            ptr.release();
        }
        if cap == Capability::Keyboard
            && let Some(kbd) = self.keyboard.take()
        {
            kbd.release();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl WindowHandler for WaylandClientData {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.app.request_quit();
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let new_w = configure
            .new_size
            .0
            .map_or(DEFAULT_WIDTH, std::num::NonZeroU32::get)
            .max(480);
        let new_h = configure
            .new_size
            .1
            .map_or(DEFAULT_HEIGHT, std::num::NonZeroU32::get)
            .max(360);
        if (self.width, self.height) != (new_w, new_h) {
            self.main_buffers.clear();
            self.layout_cache = None;
        }
        self.width = new_w;
        self.height = new_h;
        self.app.set_maximized(configure.is_maximized());
        let next_focused = configure.state.contains(WindowState::ACTIVATED);
        if self.app.focused != next_focused {
            self.app.focused = next_focused;
        }
        if self.render_backend == RenderBackend::Gpu
            && self.gles.is_none()
            && let Some(win) = &self.window
        {
            match rusty_ui::GlesSurfaceRenderer::new(
                self.conn.backend().display_ptr().cast(),
                win.wl_surface(),
                new_w.cast_signed(),
                new_h.cast_signed(),
            ) {
                Ok(renderer) => {
                    self.gles = Some(renderer);
                }
                Err(err) => {
                    tracing::warn!(%err, "rusty-files GLES2 unavailable; using CPU wl_shm fallback");
                    self.render_backend = RenderBackend::Cpu;
                }
            }
        }
        self.configured = true;
        self.frame_sent_at = None;
        self.sync_window_glass_region();
        self.app.dirty = true;
    }
}

impl PopupHandler for WaylandClientData {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        let Some(ap) = &mut self.active_popup else {
            return;
        };
        if ap.popup.wl_surface() != popup.wl_surface() {
            return;
        }
        let req_w = u32::try_from(ap.spec.surface_width.max(1)).unwrap_or(1);
        let req_h = u32::try_from(ap.spec.surface_height.max(1)).unwrap_or(1);
        let w = if config.width > 0 {
            u32::try_from(config.width).unwrap_or(req_w)
        } else {
            req_w
        };
        let h = if config.height > 0 {
            u32::try_from(config.height).unwrap_or(req_h)
        } else {
            req_h
        };
        if ap.configured_size != (w, h) {
            ap.buffers.clear();
            ap.configured_size = (w, h);
        }
        ap.configured = true;
        let _ = draw_popup_surface(&mut self.pool, ap, self.app.theme, self.app.accent_index);
    }

    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup) {
        if self
            .active_popup
            .as_ref()
            .is_some_and(|ap| ap.popup.wl_surface() == popup.wl_surface())
        {
            self.close_popup();
        }
    }
}

impl PointerHandler for WaylandClientData {
    #[allow(clippy::too_many_lines)]
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            if self
                .active_popup
                .as_ref()
                .is_some_and(|ap| ap.popup.wl_surface() == &ev.surface)
            {
                self.handle_popup_pointer_event(ev);
                continue;
            }
            self.pointer_pos = ev.position;
            let (px, py) = ev.position;
            let tl_hovered = (10.0..=76.0).contains(&px) && (8.0..=32.0).contains(&py);
            if self.app.traffic_hovered != tl_hovered {
                self.app.traffic_hovered = tl_hovered;
                self.app.dirty = true;
            }
            match ev.kind {
                PointerEventKind::Enter { serial } => {
                    self.pointer_enter_serial = serial;
                    self.last_cursor_shape = None;
                    let layout = compute_layout(&self.app, self.width, self.height);
                    let hit = hit_test(&layout, px, py);
                    self.update_cursor_shape(hit.as_ref(), &layout);
                    self.app.set_hovered_hit(hit);
                }
                PointerEventKind::Motion { .. } => {
                    if self.update_active_text_drag(px) {
                        self.apply_cursor_shape(CursorShape::Text);
                        self.app.dirty = true;
                    } else if self.pointer_pressed
                        && let Some(drag) = self.app.file_drag.as_mut()
                    {
                        drag.copy_mode = self.modifiers.ctrl;
                        let is_active = drag.active;
                        if drag.update_pos((px, py)) {
                            self.app.dirty = true;
                        }
                        let near_or_outside_edge = px <= 4.0
                            || py <= 4.0
                            || px >= f64::from(self.width) - 4.0
                            || py >= f64::from(self.height) - 4.0;
                        if drag.active && near_or_outside_edge {
                            let drag_sources = drag.sources.clone();
                            if self.start_wayland_file_drag(&drag_sources) {
                                self.app.file_drag = None;
                                self.pointer_pressed = false;
                                self.app.dirty = true;
                                continue;
                            }
                        }
                        if is_active {
                            self.apply_cursor_shape(CursorShape::Grabbing);
                        }
                    } else {
                        let layout = compute_layout(&self.app, self.width, self.height);
                        let hit = hit_test(&layout, px, py);
                        self.update_cursor_shape(hit.as_ref(), &layout);
                        self.app.set_hovered_hit(hit);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.pointer_pressed
                        && let Some(drag) = self.app.file_drag.as_ref()
                        && drag.active
                    {
                        let drag_sources = drag.sources.clone();
                        if self.start_wayland_file_drag(&drag_sources) {
                            self.app.file_drag = None;
                            self.pointer_pressed = false;
                            self.app.dirty = true;
                        }
                    }
                    if self.app.traffic_hovered {
                        self.app.traffic_hovered = false;
                        self.app.dirty = true;
                    }
                    self.last_cursor_shape = None;
                    self.app.set_hovered_hit(None);
                }
                PointerEventKind::Press { button, serial, .. } => {
                    let (px, py) = ev.position;
                    if button == BTN_LEFT {
                        self.pointer_pressed = true;
                        self.execute_left_click(px, py, serial);
                        if self.app.popup.is_none() && self.app.quick_look.is_none() {
                            let drag_paths: Vec<std::path::PathBuf> = self
                                .app
                                .selected_entries()
                                .into_iter()
                                .map(|e| e.path)
                                .collect();
                            if !drag_paths.is_empty() {
                                self.app.file_drag =
                                    Some(crate::state::FileDragState::start(drag_paths, (px, py)));
                            }
                        }
                    } else if button == BTN_RIGHT {
                        self.execute_right_click(px, py, serial);
                    }
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => {
                    self.pointer_pressed = false;
                    self.app.search_field.on_pointer_up();
                    self.app.rename_field.on_pointer_up();
                    self.app.goto_field.on_pointer_up();
                    if let Some(drag) = self.app.file_drag.take()
                        && drag.active
                        && let Some(dest) = drag.target_dir.clone()
                        && drag.can_drop_on(&dest)
                    {
                        self.app
                            .action_drop_paths(drag.sources, dest, drag.copy_mode);
                    }
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    source,
                    ..
                } => {
                    self.handle_axis_scroll(
                        ev.position.0,
                        ev.position.1,
                        horizontal,
                        vertical,
                        source,
                    );
                }
                PointerEventKind::Release { .. } => {}
            }
        }
    }
}

fn insert_files_sources(
    loop_handle: &LoopHandle<'static, WaylandClientData>,
    conn: &Connection,
    queue: EventQueue<WaylandClientData>,
    paste_source: ping::PingSource,
) -> Result<(), WaylandAppError> {
    WaylandSource::new(conn.clone(), queue)
        .insert(loop_handle.clone())
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;
    loop_handle
        .insert_source(paste_source, |(), (), data| {
            if let Some(pasted) = data.clipboard.poll_received_paste() {
                let changed = with_font_engine(|fe| data.app.apply_focused_paste(&pasted, fe));
                if changed {
                    data.app.dirty = true;
                }
            }
        })
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;
    Ok(())
}

impl KeyboardHandler for WaylandClientData {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.modifiers = Modifiers::default();
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.handle_key(event.keysym, event.utf8.as_deref());
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl DataDeviceHandler for WaylandClientData {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
    ) {
    }
    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
    fn drop_performed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
}

impl DataOfferHandler for WaylandClientData {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl DataSourceHandler for WaylandClientData {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: Option<String>,
    ) {
    }
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
        mime: String,
        mut write_pipe: WritePipe,
    ) {
        if let Some(active) = self.active_dnd_source.as_ref()
            && active.source == *source
        {
            let payload = if mime.eq_ignore_ascii_case("text/uri-list") {
                active.uri_list.clone()
            } else {
                active.plain_text.clone()
            };
            let _ = std::thread::Builder::new()
                .name("rusty-files-dnd-send".to_owned())
                .spawn(move || {
                    use std::io::Write;
                    let _ = write_pipe.write_all(&payload);
                });
            return;
        }
        self.clipboard
            .serve_send_request(source, &mime, OwnedFd::from(write_pipe));
    }
    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
    ) {
        if self
            .active_dnd_source
            .as_ref()
            .is_some_and(|active| active.source == *source)
        {
            self.active_dnd_source = None;
            source.destroy();
        }
        self.clipboard.remove_source(source);
    }
    fn dnd_dropped(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }
    fn dnd_finished(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
    ) {
        if let Some(active) = self.active_dnd_source.take() {
            if active.source == *source {
                source.destroy();
                if active.selected_action == DndAction::Move {
                    self.app.reload_active_column();
                    self.app.dirty = true;
                }
            } else {
                self.active_dnd_source = Some(active);
            }
        }
    }
    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
        action: DndAction,
    ) {
        if let Some(active) = self.active_dnd_source.as_mut()
            && active.source == *source
        {
            active.selected_action = action;
        }
    }
}

fn map_keysym_to_ui(keysym: Keysym, utf8: Option<&str>) -> Option<rusty_ui::TextInputKey> {
    match keysym {
        Keysym::Left => Some(rusty_ui::TextInputKey::Left),
        Keysym::Right => Some(rusty_ui::TextInputKey::Right),
        Keysym::Home => Some(rusty_ui::TextInputKey::Home),
        Keysym::End => Some(rusty_ui::TextInputKey::End),
        Keysym::BackSpace => Some(rusty_ui::TextInputKey::Backspace),
        Keysym::Delete => Some(rusty_ui::TextInputKey::Delete),
        Keysym::Return | Keysym::KP_Enter => Some(rusty_ui::TextInputKey::Enter),
        Keysym::Escape => Some(rusty_ui::TextInputKey::Escape),
        _ => utf8
            .and_then(|s| s.chars().next())
            .filter(|ch| !ch.is_control())
            .map(rusty_ui::TextInputKey::Char)
            .or_else(|| {
                keysym
                    .key_char()
                    .filter(|ch| !ch.is_control())
                    .map(rusty_ui::TextInputKey::Char)
            }),
    }
}

fn keysym_to_name_and_text(keysym: Keysym, utf8: Option<&str>) -> (&'static str, Option<char>) {
    let name = match keysym {
        Keysym::Escape => "Escape",
        Keysym::BackSpace => "BackSpace",
        Keysym::Tab | Keysym::ISO_Left_Tab => "Tab",
        Keysym::Return | Keysym::KP_Enter => "Return",
        Keysym::space => "Space",
        Keysym::F2 => "F2",
        Keysym::Up => "Up",
        Keysym::Down => "Down",
        Keysym::Left => "Left",
        Keysym::Right => "Right",
        Keysym::Delete => "Delete",
        _ => "",
    };
    let ch = utf8
        .and_then(|s| s.chars().next())
        .filter(|c| !c.is_control())
        .or_else(|| keysym.key_char().filter(|c| !c.is_control()));
    (name, ch)
}

#[cfg(test)]
mod tests {
    use super::encode_paths_uri_list;
    use std::path::PathBuf;

    #[test]
    fn encodes_paths_as_rfc2483_crlf_uri_list() {
        let encoded = encode_paths_uri_list(&[
            PathBuf::from("/home/alice/My Report #1.pdf"),
            PathBuf::from("/tmp/notes.txt"),
        ]);
        assert_eq!(
            encoded,
            "file:///home/alice/My%20Report%20%231.pdf\r\nfile:///tmp/notes.txt\r\n"
        );
    }
}
