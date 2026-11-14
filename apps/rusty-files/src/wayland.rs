//! Wayland CSD (`xdg_toplevel`) client event loop and SHM presentation for `rusty-files`
//! (`RUSTYOS-065`, ADR-0030).

use std::fmt;
use std::time::{Duration, Instant};

use rusty_services::file_manager::ListingTarget;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Surface},
    delegate_compositor, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, delegate_xdg_popup, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::EventLoop,
        calloop_wayland_source::WaylandSource,
        protocols::xdg::shell::client::{
            xdg_positioner::{Anchor, ConstraintAdjustment, Gravity},
            xdg_toplevel::ResizeEdge,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{
            AxisScroll, BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler,
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
    Connection, Dispatch, QueueHandle, WEnum,
    backend::WaylandError,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::layout::{
    FilesHit, FilesLayout, PopupCommand, PopupSurfaceSpec, ResizeEdgeZone, compute_layout,
    compute_popup_surface_spec, hit_test, scroll_area_at,
};
use crate::render::{render_popup_surface_pixels, render_window_pixels_with_popup};
use crate::state::OpenPopupMenu;
use crate::state::{FilesAppState, ScrollTargetArea, StartupOptions};
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
    menu_key: OpenPopupMenu,
    spec: PopupSurfaceSpec,
    hover_idx: Option<usize>,
    last_rendered_hover: Option<usize>,
    configured: bool,
    configured_size: (u32, u32),
    buffers: Vec<Buffer>,
}

struct WaylandClientData {
    qh: QueueHandle<Self>,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor_state: CompositorState,
    shm: Shm,
    xdg_shell: XdgShell,
    pool: SlotPool,
    window: Option<Window>,
    main_buffers: Vec<Buffer>,
    buffer_size: (u32, u32),
    frame_sent_at: Option<Instant>,
    active_popup: Option<ActiveWaylandPopup>,
    popup_trigger_serial: Option<u32>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mod_shift: bool,
    mod_ctrl: bool,
    width: u32,
    height: u32,
    configured: bool,
    pointer_pos: (f64, f64),
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
                draw_popup_surface(&mut self.pool, ap, self.app.theme)?;
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

        if let (Some(seat), Some(serial)) = (&self.seat, self.popup_trigger_serial) {
            popup.xdg_popup().grab(seat, serial);
        }
        popup.wl_surface().commit();

        let req_w = u32::try_from(spec.surface_width.max(1)).unwrap_or(1);
        let req_h = u32::try_from(spec.surface_height.max(1)).unwrap_or(1);
        self.active_popup = Some(ActiveWaylandPopup {
            popup,
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
                        let _ = draw_popup_surface(&mut self.pool, ap, self.app.theme);
                    }
                }
            }
            PointerEventKind::Leave { .. } => {
                if ap.hover_idx.is_some() {
                    ap.hover_idx = None;
                    if ap.configured {
                        let _ = draw_popup_surface(&mut self.pool, ap, self.app.theme);
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
        let double_click = self.last_click.as_ref().is_some_and(|(t, prev)| {
            now.duration_since(*t) <= Duration::from_millis(400) && prev == &hit
        });
        self.last_click = if double_click {
            None
        } else {
            Some((now, hit.clone()))
        };

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
            other => self.handle_app_hit(other, double_click, &layout, px, prev_popup.as_ref()),
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
            FilesHit::ActionPrimary => {
                if self.app.is_trash_view() {
                    if let Some(entry) = self.app.selected_entry().cloned() {
                        self.app.restore_entry(&entry);
                    }
                } else {
                    self.app.action_new_folder();
                }
            }
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
                } else {
                    self.app.search_active = true;
                    self.app.reset_caret_blink();
                }
                self.app.dirty = true;
            }
            FilesHit::SearchClear => self.app.set_search_query(String::new()),
            FilesHit::EmptyTrashBanner => self.app.action_empty_trash(),
            FilesHit::ThemeFooterToggle => self.app.toggle_theme(),
            FilesHit::SidebarItem(loc) => self.app.navigate_sidebar(loc, true),
            FilesHit::ColumnRow { col_idx, row_idx } => {
                self.app.select_entry(col_idx, row_idx, double_click);
            }
            FilesHit::ColumnEmpty { col_idx } => {
                self.app.popup = None;
                self.app.active_column = col_idx;
                if let Some(col) = self.app.columns.get_mut(col_idx) {
                    col.selected_index = None;
                }
                self.app.columns.truncate(col_idx + 1);
                self.app.dirty = true;
            }
            FilesHit::ListHeaderSort(field) => self.app.set_sort(field),
            FilesHit::ListRow { row_idx } => self.app.select_entry(0, row_idx, double_click),
            FilesHit::ListEmpty | FilesHit::IconsEmpty => {
                self.app.popup = None;
                if let Some(col) = self.app.columns.first_mut() {
                    col.selected_index = None;
                }
                self.app.dirty = true;
            }
            FilesHit::PreviewOpen => self.app.activate_selected(),
            FilesHit::PreviewTrashOrRestore => self.app.action_move_selected_to_trash(),
            FilesHit::PreviewTag { path, color } => self.app.toggle_tag_on_path(&path, color),
            FilesHit::PopupItem(cmd) => self.execute_popup_command(cmd),
            other => self.handle_stage_b_hit(other, double_click),
        }
    }

    fn handle_stage_b_hit(&mut self, hit: FilesHit, double_click: bool) {
        match hit {
            FilesHit::IconCell { idx } | FilesHit::GalleryThumb { idx } => {
                self.app.select_entry(0, idx, double_click);
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
                    self.app.reset_caret_blink();
                    self.app.dirty = true;
                }
            }
            FilesHit::GoToSuggestion(idx) => self.app.pick_go_to_suggestion(idx),
            FilesHit::TransferStop => self.app.cancel_active_transfer(),
            FilesHit::ConflictDecision(decision) => self.app.resolve_transfer_conflict(decision),
            _ => {}
        }
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

    fn execute_popup_command(&mut self, cmd: PopupCommand) {
        self.active_popup = None;
        self.app.popup = None;
        match cmd {
            PopupCommand::SortBy(field) => self.app.set_sort(field),
            PopupCommand::SetSortOrder(order) => self.app.set_sort_order(order),
            PopupCommand::NewFolder => self.app.action_new_folder(),
            PopupCommand::NewTab => self.app.new_tab(None),
            PopupCommand::NewTabIn(path) => self.app.new_tab(Some(ListingTarget::Directory(path))),
            PopupCommand::ToggleTabBar => self.app.toggle_tab_bar(),
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
            PopupCommand::MoveToTrash(entry) => self.app.trash_entry(&entry),
            PopupCommand::RestoreEntry(entry) => self.app.restore_entry(&entry),
            PopupCommand::EmptyTrash => self.app.action_empty_trash(),
            PopupCommand::ToggleTag { path, color } => self.app.toggle_tag_on_path(&path, color),
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
        } else if self.mod_shift && v_delta.abs() >= 0.01 {
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
            if self.mod_shift {
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

/// Connects to Wayland compositor, maps the Golden Gate CSD window, and runs the event loop.
///
/// # Errors
/// Returns `WaylandAppError` on Wayland connection, protocol binding, or rendering failure.
pub(crate) fn run(options: &StartupOptions) -> Result<(), WaylandAppError> {
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
    window.commit();

    let mut data = WaylandClientData {
        qh,
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &queue.handle()),
        seat_state: SeatState::new(&globals, &queue.handle()),
        compositor_state,
        shm,
        xdg_shell,
        pool,
        window: Some(window),
        main_buffers: Vec::new(),
        buffer_size: (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        frame_sent_at: None,
        active_popup: None,
        popup_trigger_serial: None,
        seat: None,
        pointer: None,
        keyboard: None,
        mod_shift: false,
        mod_ctrl: false,
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        configured: false,
        pointer_pos: (0.0, 0.0),
        last_click: None,
        last_title: initial_title,
        app,
        layout_cache: None,
    };

    let mut event_loop: EventLoop<'static, WaylandClientData> =
        EventLoop::try_new().map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| WaylandAppError::EventLoop(format!("{e:?}")))?;

    while data.app.is_running() {
        let now = Instant::now();
        let _ = data.app.poll_background_loads();
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
            && (data.app.dirty || data.app.has_active_animations())
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
        } else if data.app.has_focused_text_input() {
            Some(data.app.time_until_caret_blink(now))
        } else {
            None
        };

        event_loop
            .dispatch(timeout, &mut data)
            .map_err(|e| WaylandAppError::EventLoop(e.to_string()))?;
    }
    Ok(())
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
fn draw_main_frame(data: &mut WaylandClientData) -> Result<(), WaylandAppError> {
    let layout = data.layout();
    let Some(win) = &data.window else {
        return Ok(());
    };
    let w = data.width.max(1);
    let h = data.height.max(1);
    if data.buffer_size != (w, h) {
        data.main_buffers.clear();
        data.buffer_size = (w, h);
    }
    let idx = acquire_reusable_buffer(&mut data.pool, &mut data.main_buffers, w, h)?;
    let buf_ref = &data.main_buffers[idx];
    let canvas = buf_ref
        .canvas(&mut data.pool)
        .ok_or_else(|| WaylandAppError::ShmPool(String::from("Main SHM slot unexpectedly busy")))?;

    render_window_pixels_with_popup(canvas, w, h, &data.app, &layout, false);
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
) -> Result<(), WaylandAppError> {
    let (w, h) = (ap.configured_size.0.max(1), ap.configured_size.1.max(1));
    let idx = acquire_reusable_buffer(pool, &mut ap.buffers, w, h)?;
    let buf_ref = &ap.buffers[idx];
    let canvas = buf_ref.canvas(pool).ok_or_else(|| {
        WaylandAppError::ShmPool(String::from("Popup SHM slot unexpectedly busy"))
    })?;

    render_popup_surface_pixels(canvas, w, h, &ap.spec.layout, ap.hover_idx, theme);
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
delegate_pointer!(WaylandClientData);
delegate_xdg_shell!(WaylandClientData);
delegate_xdg_window!(WaylandClientData);
delegate_xdg_popup!(WaylandClientData);

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
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
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
            self.pointer = Some(ptr);
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(seat.get_keyboard(qh, ()));
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
        self.configured = true;
        self.frame_sent_at = None;
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
        let _ = draw_popup_surface(&mut self.pool, ap, self.app.theme);
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
            match ev.kind {
                PointerEventKind::Press { button, serial, .. } => {
                    let (px, py) = ev.position;
                    if button == BTN_LEFT {
                        self.execute_left_click(px, py, serial);
                    } else if button == BTN_RIGHT {
                        self.execute_right_click(px, py, serial);
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
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for WaylandClientData {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Modifiers { mods_depressed, .. } => {
                state.mod_shift = (mods_depressed & 1) != 0;
                state.mod_ctrl = (mods_depressed & 4) != 0;
            }
            wl_keyboard::Event::Leave { .. } => {
                state.mod_shift = false;
                state.mod_ctrl = false;
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                if let Some(cached) = &state.layout_cache {
                    state.app.last_icon_grid_cols =
                        FilesAppState::icon_grid_columns_for_width(cached.content_rect.w);
                }
                let (key_name, utf8_opt) = decode_evdev_key(key, state.mod_shift);
                let mut utf8_buf = [0_u8; 4];
                let text = utf8_opt.map(|ch| ch.encode_utf8(&mut utf8_buf) as &str);
                state
                    .app
                    .handle_key_input(key_name, text, state.mod_ctrl, state.mod_shift);
            }
            _ => {}
        }
    }
}

const fn decode_evdev_key(evdev_code: u32, shift: bool) -> (&'static str, Option<char>) {
    match evdev_code {
        1 => ("Escape", None),
        14 => ("BackSpace", None),
        15 => ("Tab", None),
        28 | 96 => ("Return", None),
        57 => ("Space", Some(' ')),
        60 => ("F2", None),
        103 => ("Up", None),
        105 => ("Left", None),
        106 => ("Right", None),
        108 => ("Down", None),
        111 => ("Delete", None),
        2..=11 => {
            let digits = if shift {
                ['!', '@', '#', '$', '%', '^', '&', '*', '(', ')']
            } else {
                ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0']
            };
            ("", Some(digits[(evdev_code - 2) as usize]))
        }
        12 => ("", Some(if shift { '_' } else { '-' })),
        13 => ("", Some(if shift { '+' } else { '=' })),
        16..=25 => {
            let row = b"qwertyuiop";
            let ch = row[(evdev_code - 16) as usize] as char;
            ("", Some(if shift { ch.to_ascii_uppercase() } else { ch }))
        }
        26 => ("", Some(if shift { '{' } else { '[' })),
        27 => ("", Some(if shift { '}' } else { ']' })),
        30..=38 => {
            let row = b"asdfghjkl";
            let ch = row[(evdev_code - 30) as usize] as char;
            ("", Some(if shift { ch.to_ascii_uppercase() } else { ch }))
        }
        39 => ("", Some(if shift { ':' } else { ';' })),
        40 => ("", Some(if shift { '"' } else { '\'' })),
        41 => ("", Some(if shift { '~' } else { '`' })),
        43 => ("", Some(if shift { '|' } else { '\\' })),
        44..=50 => {
            let row = b"zxcvbnm";
            let ch = row[(evdev_code - 44) as usize] as char;
            ("", Some(if shift { ch.to_ascii_uppercase() } else { ch }))
        }
        51 => ("", Some(if shift { '<' } else { ',' })),
        52 => ("", Some(if shift { '>' } else { '.' })),
        53 => ("", Some(if shift { '?' } else { '/' })),
        _ => ("", None),
    }
}
