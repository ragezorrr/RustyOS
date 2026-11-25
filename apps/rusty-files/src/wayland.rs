//! Shared `rusty-app` Wayland CSD host integration for `rusty-files`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusty_app::{
    AppConfig, AppContext, AppError, ConfigureInfo, CursorShape, PopupConfig, ResizeEdge, RustyApp,
    WindowDecorationMode, run_app,
};
use rusty_services::file_manager::{FileViewMode, ListingTarget, TransferKind, UserFileLocations};
use smithay_client_toolkit::seat::{
    keyboard::{KeyEvent, Keysym},
    pointer::{AxisScroll, BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind},
};
use wayland_client::protocol::wl_pointer;

use crate::layout::{
    FilesHit, FilesLayout, PopupCommand, PopupSurfaceSpec, ResizeEdgeZone, SIDEBAR_WIDTH,
    WINDOW_CORNER_RADIUS, compute_layout, compute_popup_surface_spec, hit_test, scroll_area_at,
};
use crate::liquid_glass::{GlassRegion, LiquidGlassConfig};
use crate::render::{
    canvas::with_font_engine, render_popup_surface_pixels_with_style,
    render_window_pixels_with_options,
};
use crate::state::{
    FileDragState, FilesAppState, MarqueeSelectionState, OpenPopupMenu, ScrollTargetArea,
    SearchScope, StartupOptions,
};

const DEFAULT_WIDTH: u32 = 1060;
const DEFAULT_HEIGHT: u32 = 660;
const POPUP_SHADOW_PAD: i32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedDropTarget {
    Directory(PathBuf),
    Trash,
}

fn rects_intersect(a: crate::layout::Rect, b: crate::layout::Rect) -> bool {
    a.x < b.right() && a.right() > b.x && a.y < b.bottom() && a.bottom() > b.y
}

pub(crate) fn encode_paths_uri_list(paths: &[PathBuf]) -> String {
    let mut out = String::new();
    for p in paths {
        out.push_str(&rusty_services::path_to_file_uri(p));
        out.push_str("\r\n");
    }
    out
}

pub(crate) fn encode_gnome_copied_files(kind: TransferKind, paths: &[PathBuf]) -> String {
    let verb = match kind {
        TransferKind::Copy => "copy",
        TransferKind::Move => "cut",
    };
    let mut out = String::from(verb);
    for p in paths {
        out.push('\n');
        out.push_str(&rusty_services::path_to_file_uri(p));
    }
    out
}

pub(crate) fn parse_gnome_copied_files(raw: &str) -> Option<(bool, Vec<PathBuf>)> {
    let mut lines = raw.lines();
    let verb = lines.next()?.trim();
    let copy_mode = match verb {
        "copy" => true,
        "cut" => false,
        _ => return None,
    };
    let rest: String = lines.collect::<Vec<_>>().join("\n");
    let paths = parse_uri_list_paths(&rest);
    (!paths.is_empty()).then_some((copy_mode, paths))
}

pub(crate) fn resolve_drop_target(
    app: &FilesAppState,
    hit: Option<&FilesHit>,
) -> Option<ResolvedDropTarget> {
    let hit = hit?;
    match hit {
        FilesHit::ColumnRow { col_idx, row_idx } => app
            .columns
            .get(*col_idx)
            .and_then(|c| c.visible_entries.get(*row_idx))
            .filter(|e| e.is_navigable_dir())
            .map(|e| ResolvedDropTarget::Directory(e.path.clone())),
        FilesHit::ListRow { row_idx }
        | FilesHit::IconCell { idx: row_idx }
        | FilesHit::GalleryThumb { idx: row_idx } => app
            .columns
            .first()
            .and_then(|c| c.visible_entries.get(*row_idx))
            .filter(|e| e.is_navigable_dir())
            .map(|e| ResolvedDropTarget::Directory(e.path.clone())),
        FilesHit::SidebarItem(loc) => match app.locations.resolve_target(loc) {
            ListingTarget::Directory(dir) => Some(ResolvedDropTarget::Directory(dir)),
            ListingTarget::Trash if !app.is_trash_view() => Some(ResolvedDropTarget::Trash),
            _ => None,
        },
        FilesHit::PathCrumb(dir) => Some(ResolvedDropTarget::Directory(dir.clone())),
        FilesHit::ColumnEmpty { col_idx } => match app.columns.get(*col_idx).map(|c| &c.target) {
            Some(ListingTarget::Directory(dir)) => Some(ResolvedDropTarget::Directory(dir.clone())),
            _ => None,
        },
        _ => None,
    }
}

#[allow(dead_code)]
pub(crate) fn resolve_drop_target_dir(
    app: &FilesAppState,
    hit: Option<&FilesHit>,
) -> Option<PathBuf> {
    match resolve_drop_target(app, hit)? {
        ResolvedDropTarget::Directory(dir) => Some(dir),
        ResolvedDropTarget::Trash => None,
    }
}

pub(crate) fn parse_uri_list_paths(raw: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("file://") else {
            continue;
        };
        let path_part = if rest.starts_with('/') {
            rest
        } else if let Some((_, p)) = rest.split_once('/') {
            &rest[rest.len() - p.len() - 1..]
        } else {
            continue;
        };
        let bytes = path_part.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%'
                && i + 2 < bytes.len()
                && let (Some(hi), Some(lo)) = (
                    (bytes[i + 1] as char).to_digit(16),
                    (bytes[i + 2] as char).to_digit(16),
                )
            {
                if let Ok(byte) = u8::try_from((hi << 4) | lo) {
                    decoded.push(byte);
                }
                i += 3;
                continue;
            }
            decoded.push(bytes[i]);
            i += 1;
        }
        let path_str = String::from_utf8_lossy(&decoded);
        if path_str.starts_with('/') {
            out.push(PathBuf::from(path_str.as_ref()));
        }
    }
    out
}

struct FilesApp {
    app: FilesAppState,
    layout_cache: Option<FilesLayout>,
    width: u32,
    height: u32,
    pointer_pressed: bool,
    click_streak: u8,
    last_click: Option<(Instant, FilesHit)>,
    last_input_serial: u32,
    last_popup: Option<OpenPopupMenu>,
    active_popup_spec: Option<PopupSurfaceSpec>,
    popup_hover_idx: Option<usize>,
    pending_external_paste_dir: Option<PathBuf>,
    ignore_next_selection_clear: bool,
    picker_result: Arc<AtomicBool>,
}

impl FilesApp {
    fn layout(&mut self) -> FilesLayout {
        self.layout_for(self.width, self.height)
    }

    fn layout_for(&mut self, w: u32, h: u32) -> FilesLayout {
        let l = compute_layout(&self.app, w, h);
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

    fn sync_glass(&self, ctx: &mut AppContext<'_>) {
        if self.app.reduce_transparency {
            ctx.set_liquid_glass(None);
            return;
        }
        let wi = i32::try_from(self.width).unwrap_or(1).max(1);
        let hi = i32::try_from(self.height).unwrap_or(1).max(1);
        let glass_w = (SIDEBAR_WIDTH + WINDOW_CORNER_RADIUS).min(wi).max(1);
        let radius = u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14);
        ctx.set_liquid_glass(Some(LiquidGlassConfig::sidebar(radius).with_region(
            GlassRegion::Rect {
                x: 0,
                y: 0,
                width: glass_w,
                height: hi,
            },
        )));
    }

    fn sync_popup(&mut self, ctx: &mut AppContext<'_>) {
        if self.app.popup == self.last_popup {
            return;
        }
        self.last_popup = self.app.popup.clone();
        let Some(menu) = self.app.popup.clone() else {
            self.active_popup_spec = None;
            self.popup_hover_idx = None;
            ctx.close_popup();
            return;
        };
        let layout = self.layout();
        let spec = compute_popup_surface_spec(&self.app, &layout, &menu, POPUP_SHADOW_PAD);
        self.active_popup_spec = Some(spec.clone());
        self.popup_hover_idx = None;
        let radius = u32::try_from(rusty_ui::menu::MENU_RADIUS).unwrap_or(10);
        ctx.open_popup(PopupConfig {
            width: u32::try_from(spec.surface_width.max(1)).unwrap_or(1),
            height: u32::try_from(spec.surface_height.max(1)).unwrap_or(1),
            anchor_x: spec.anchor_x,
            anchor_y: spec.anchor_y,
            offset_x: -POPUP_SHADOW_PAD,
            offset_y: -POPUP_SHADOW_PAD,
            grab_serial: Some(self.last_input_serial),
            glass: (!self.app.reduce_transparency).then(|| LiquidGlassConfig::popover(radius)),
        });
    }

    fn sync_clipboard_io(&mut self, ctx: &mut AppContext<'_>) {
        if let Some(clip) = self.app.take_pending_uri_clipboard() {
            let gnome_copied_files = encode_gnome_copied_files(clip.kind, &clip.paths).into_bytes();
            let uri_list = encode_paths_uri_list(&clip.paths).into_bytes();
            let plain_text = clip
                .paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes();
            self.ignore_next_selection_clear = true;
            let _ = ctx.set_clipboard_mimes(
                vec![
                    (
                        String::from("x-special/gnome-copied-files"),
                        gnome_copied_files,
                    ),
                    (String::from("text/uri-list"), uri_list),
                    (String::from("text/plain;charset=utf-8"), plain_text.clone()),
                    (String::from("text/plain"), plain_text),
                ],
                self.last_input_serial,
            );
        }
        if let Some(dest_dir) = self.app.take_pending_external_uri_paste_dir()
            && ctx.request_clipboard_mime(&["x-special/gnome-copied-files", "text/uri-list"])
        {
            self.pending_external_paste_dir = Some(dest_dir);
        }
    }

    fn start_external_file_drag(&mut self, ctx: &mut AppContext<'_>, sources: &[PathBuf]) -> bool {
        if sources.is_empty() {
            return false;
        }
        let uri_list = encode_paths_uri_list(sources).into_bytes();
        let plain_text = sources
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        ctx.start_drag(
            vec![
                (String::from("text/uri-list"), uri_list),
                (String::from("text/plain;charset=utf-8"), plain_text.clone()),
                (String::from("text/plain"), plain_text),
            ],
            self.last_input_serial,
        )
    }

    fn handle_pointer_press(
        &mut self,
        ctx: &mut AppContext<'_>,
        px: f64,
        py: f64,
        button: u32,
        serial: u32,
    ) {
        self.last_input_serial = serial;
        if button == BTN_LEFT {
            self.pointer_pressed = true;
            let layout = self.layout();
            let press_hit = hit_test(&layout, px, py);
            let empty_canvas =
                matches!(press_hit, Some(FilesHit::IconsEmpty | FilesHit::ListEmpty));
            let mods = ctx.modifiers();
            let baseline: BTreeSet<PathBuf> = if mods.ctrl || mods.shift {
                self.app
                    .selected_entries()
                    .into_iter()
                    .map(|e| e.path)
                    .collect()
            } else {
                BTreeSet::new()
            };
            self.execute_left_click(ctx, px, py, serial);
            if self.app.popup.is_none()
                && self.app.quick_look.is_none()
                && self.app.batch_rename.is_none()
            {
                if empty_canvas {
                    self.app.marquee = Some(MarqueeSelectionState::start(
                        (px, py),
                        mods.ctrl || mods.shift,
                        baseline,
                    ));
                } else {
                    let drag_paths: Vec<PathBuf> = self
                        .app
                        .selected_entries()
                        .into_iter()
                        .map(|e| e.path)
                        .collect();
                    if !drag_paths.is_empty() {
                        self.app.file_drag = Some(FileDragState::start(drag_paths, (px, py)));
                    }
                }
            }
        } else if button == BTN_RIGHT {
            self.execute_right_click(ctx, px, py, serial);
        }
    }

    fn handle_pointer_motion(&mut self, ctx: &mut AppContext<'_>, px: f64, py: f64) {
        if self.update_active_text_drag(px) {
            ctx.set_cursor_shape(CursorShape::Text);
            self.app.dirty = true;
            return;
        }
        if self.pointer_pressed && self.app.file_drag.is_some() {
            self.handle_file_drag_motion(ctx, px, py);
            return;
        }
        if self.pointer_pressed && self.app.marquee.is_some() {
            self.handle_marquee_motion(px, py);
            return;
        }
        let layout = self.layout();
        let hit = hit_test(&layout, px, py);
        update_cursor_shape(ctx, hit.as_ref(), &layout);
        self.app.set_hovered_hit(hit);
    }

    fn handle_file_drag_motion(&mut self, ctx: &mut AppContext<'_>, px: f64, py: f64) {
        let copy_mode = ctx.modifiers().ctrl;
        let near_or_outside_edge = px <= 4.0
            || py <= 4.0
            || px >= f64::from(self.width) - 4.0
            || py >= f64::from(self.height) - 4.0;
        let mut drag_active = false;
        let mut start_external: Option<Vec<PathBuf>> = None;
        if let Some(drag) = self.app.file_drag.as_mut() {
            drag.copy_mode = copy_mode;
            if drag.update_pos((px, py)) {
                self.app.dirty = true;
            }
            drag_active = drag.active;
            if drag.active && near_or_outside_edge {
                start_external = Some(drag.sources.clone());
            }
        }
        if let Some(drag_sources) = start_external
            && self.start_external_file_drag(ctx, &drag_sources)
        {
            self.app.file_drag = None;
            self.pointer_pressed = false;
            self.app.dirty = true;
            return;
        }
        if drag_active {
            let layout = self.layout();
            let hit = hit_test(&layout, px, py);
            let resolved = resolve_drop_target(&self.app, hit.as_ref());
            if let Some(drag) = self.app.file_drag.as_mut() {
                let (valid_dir, valid_trash) = match resolved {
                    Some(ResolvedDropTarget::Directory(d)) if drag.can_drop_on(&d) => {
                        (Some(d), false)
                    }
                    Some(ResolvedDropTarget::Trash) if !drag.sources.is_empty() => (None, true),
                    _ => (None, false),
                };
                if drag.target_dir != valid_dir || drag.target_is_trash != valid_trash {
                    drag.target_dir = valid_dir;
                    drag.target_is_trash = valid_trash;
                    self.app.dirty = true;
                }
            }
            self.app.set_hovered_hit(hit);
            ctx.set_cursor_shape(CursorShape::Grabbing);
        }
    }

    fn handle_marquee_motion(&mut self, px: f64, py: f64) {
        let Some(m) = self.app.marquee.as_mut() else {
            return;
        };
        let active = m.update_pos((px, py));
        let additive = m.additive;
        let baseline = m.baseline_paths.clone();
        let m_rect = m.rect();
        if active && let Some(mr) = m_rect {
            let layout = self.layout();
            let mut hits = Vec::new();
            if let Some(grid) = &layout.icons_grid {
                for cell in &grid.cells {
                    if rects_intersect(mr, cell.rect) {
                        hits.push(cell.row_idx);
                    }
                }
            } else {
                for &(row_idx, row_rect) in &layout.list_rows {
                    if rects_intersect(mr, row_rect) {
                        hits.push(row_idx);
                    }
                }
            }
            self.app.apply_marquee_selection(&hits, &baseline, additive);
        }
        self.app.dirty = true;
    }

    fn handle_pointer_release(&mut self) {
        self.pointer_pressed = false;
        if self.app.marquee.take().is_some() {
            self.app.dirty = true;
        }
        self.app.search_field.on_pointer_up();
        self.app.rename_field.on_pointer_up();
        self.app.goto_field.on_pointer_up();
        if let Some(drag) = self.app.file_drag.take()
            && drag.active
        {
            if drag.target_is_trash {
                self.app.action_trash_paths(drag.sources);
            } else if let Some(dest) = drag.target_dir.clone()
                && drag.can_drop_on(&dest)
            {
                self.app
                    .action_drop_paths(drag.sources, dest, drag.copy_mode);
            }
        }
    }

    fn execute_left_click(&mut self, ctx: &mut AppContext<'_>, px: f64, py: f64, serial: u32) {
        let prev_popup = self.app.popup.clone();
        if prev_popup.is_some() {
            self.app.popup = None;
            self.app.dirty = true;
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
                ctx.start_interactive_resize(map_resize_edge(zone), serial);
            }
            FilesHit::WindowDrag => ctx.start_interactive_move(serial),
            FilesHit::TrafficClose => self.app.request_quit(),
            FilesHit::TrafficMinimize => ctx.set_minimized(),
            FilesHit::TrafficMaximize => {
                let next = !self.app.is_maximized();
                ctx.set_maximized(next);
                self.app.set_maximized(next);
            }
            other => {
                self.handle_app_hit(
                    ctx,
                    other,
                    double_click,
                    &layout,
                    px,
                    py,
                    prev_popup.as_ref(),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn handle_app_hit(
        &mut self,
        ctx: &mut AppContext<'_>,
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
        let modifiers = ctx.modifiers();
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
                    modifiers.ctrl,
                    modifiers.shift,
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
            FilesHit::ListRowChevron { row_idx } => self.app.toggle_list_row_expanded(row_idx),
            FilesHit::ListRow { row_idx } => {
                self.app.select_entry_with_modifiers(
                    0,
                    row_idx,
                    modifiers.ctrl,
                    modifiers.shift,
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
            FilesHit::PopupItem(cmd) => self.execute_popup_command(ctx, cmd),
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
                self.handle_stage_b_hit(other, double_click, layout, px, py, modifiers);
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
        mods: smithay_client_toolkit::seat::keyboard::Modifiers,
    ) {
        match hit {
            FilesHit::IconCell { idx } | FilesHit::GalleryThumb { idx } => {
                self.app
                    .select_entry_with_modifiers(0, idx, mods.ctrl, mods.shift, double_click);
            }
            FilesHit::GalleryStageOpen if double_click => self.app.activate_selected(),
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
            FilesHit::BatchRenameMode(_)
            | FilesHit::BatchRenameField(_)
            | FilesHit::BatchRenamePrefixToggle
            | FilesHit::BatchRenameCancel
            | FilesHit::BatchRenameConfirm => self.handle_batch_rename_hit(&hit),
            FilesHit::TransferStop => self.app.cancel_active_transfer(),
            FilesHit::ConflictDecision(decision) => self.app.resolve_transfer_conflict(decision),
            _ => {}
        }
    }

    fn handle_batch_rename_hit(&mut self, hit: &FilesHit) {
        match hit {
            FilesHit::BatchRenameMode(mode) => {
                if let Some(br) = &mut self.app.batch_rename {
                    br.mode = *mode;
                    br.active_field = 0;
                    br.error = None;
                    self.app.dirty = true;
                }
            }
            FilesHit::BatchRenameField(field) => {
                if let Some(br) = &mut self.app.batch_rename {
                    br.active_field = *field;
                    self.app.dirty = true;
                }
            }
            FilesHit::BatchRenamePrefixToggle => {
                if let Some(br) = &mut self.app.batch_rename {
                    br.add_as_prefix = !br.add_as_prefix;
                    br.error = None;
                    self.app.dirty = true;
                }
            }
            FilesHit::BatchRenameCancel => {
                self.app.batch_rename = None;
                self.app.dirty = true;
            }
            FilesHit::BatchRenameConfirm => self.app.commit_batch_rename(),
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

    fn execute_right_click(&mut self, ctx: &mut AppContext<'_>, px: f64, py: f64, serial: u32) {
        self.last_input_serial = serial;
        let layout = self.layout();
        let hit = hit_test(&layout, px, py);

        let (entry, dir) = match hit {
            Some(FilesHit::ColumnRow { col_idx, row_idx }) => {
                let already_selected = self.app.active_column == col_idx
                    && self
                        .app
                        .columns
                        .get(col_idx)
                        .is_some_and(|c| c.is_selected(row_idx));
                if !already_selected {
                    self.app.select_entry(col_idx, row_idx, false);
                }
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
                let already_selected = self
                    .app
                    .columns
                    .first()
                    .is_some_and(|c| c.is_selected(row_idx));
                if !already_selected {
                    self.app.select_entry(0, row_idx, false);
                }
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
                #[allow(clippy::cast_possible_truncation)]
                ctx.show_window_menu(serial, px.round() as i32, py.round() as i32);
                self.app.popup = None;
                self.sync_popup(ctx);
                return;
            }
        };

        #[allow(clippy::cast_possible_truncation)]
        let (x, y) = (px.round() as i32, py.round() as i32);
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

    fn execute_popup_command(&mut self, ctx: &mut AppContext<'_>, cmd: PopupCommand) {
        self.app.popup = None;
        match cmd {
            PopupCommand::SortBy(field) => self.app.set_sort(field),
            PopupCommand::SetSortOrder(order) => self.app.set_sort_order(order),
            PopupCommand::SetGroupBy(mode) => self.app.set_group_by(mode),
            PopupCommand::UndoLastAction => self.app.action_undo(),
            PopupCommand::NewFolder => self.app.action_new_folder(),
            PopupCommand::NewTab => self.app.new_tab(None),
            PopupCommand::NewTabIn(path) => self.app.new_tab(Some(ListingTarget::Directory(path))),
            PopupCommand::OpenInTerminal(dir) => self.app.action_open_in_terminal(&dir),
            PopupCommand::ExtractArchive(path) => self.app.action_extract_archive(path),
            PopupCommand::CompressToZip(path) => self.app.action_compress_to_zip(&path),
            PopupCommand::ToggleTabBar => self.app.toggle_tab_bar(),
            PopupCommand::TogglePathBar => self.app.toggle_path_bar(),
            PopupCommand::ShareCopyPath(path) => {
                self.app
                    .share_copy_text(path.display().to_string(), "Copied path");
            }
            PopupCommand::ShareCopyUri(path) => {
                self.app
                    .share_copy_text(format!("file://{}", path.display()), "Copied link");
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
            PopupCommand::RunExecutable(path) => self.app.launch_executable_path(&path, true),
            PopupCommand::RunInTerminal(path) => self.app.action_run_in_terminal(&path),
            PopupCommand::MoveToTrash(entry) => {
                let selected = self.app.selected_entries();
                if selected.len() > 1 && selected.iter().any(|e| e.path == entry.path) {
                    self.app.action_move_selected_to_trash();
                } else {
                    self.app.trash_entry(&entry);
                }
            }
            PopupCommand::RestoreEntry(entry) => self.app.restore_entry(&entry),
            PopupCommand::EmptyTrash => self.app.action_empty_trash(),
            PopupCommand::ToggleTag { path, color } => self.app.toggle_tag_on_path(&path, color),
            PopupCommand::ZoomWindow => ctx.set_maximized(true),
            PopupCommand::MinimizeWindow => ctx.set_minimized(),
            PopupCommand::CloseWindow => self.app.request_quit(),
        }
        self.sync_clipboard_io(ctx);
    }

    fn handle_axis_scroll(
        &mut self,
        ctx: &AppContext<'_>,
        px: f64,
        py: f64,
        horizontal: AxisScroll,
        vertical: AxisScroll,
        source: Option<wl_pointer::AxisSource>,
    ) {
        let layout = self.cached_or_layout();
        let (v_delta, v_smooth) = compute_axis_delta(vertical, source);
        let (h_delta, h_smooth) = compute_axis_delta(horizontal, source);
        let shift = ctx.modifiers().shift;

        let effective_h = if h_delta.abs() >= 0.01 {
            (h_delta, h_smooth)
        } else if shift && v_delta.abs() >= 0.01 {
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
            if self.app.view_mode == FileViewMode::Columns {
                self.layout_cache = None;
            }
            if shift {
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

impl RustyApp for FilesApp {
    fn on_configure(&mut self, ctx: &mut AppContext<'_>, info: ConfigureInfo) {
        self.width = info.width;
        self.height = info.height;
        self.layout_cache = None;
        self.app.set_maximized(info.is_maximized);
        self.app.dirty = true;
        self.sync_glass(ctx);
        ctx.request_redraw();
    }

    fn on_key(&mut self, ctx: &mut AppContext<'_>, event: KeyEvent) {
        let mods = ctx.modifiers();
        if let Some(cached) = &self.layout_cache {
            self.app.last_icon_grid_cols =
                FilesAppState::icon_grid_columns_for_width(cached.content_rect.w);
        }
        if self.app.has_focused_text_input() {
            let nav_gt = self.app.go_to_folder.is_some()
                && matches!(
                    event.keysym,
                    Keysym::Up | Keysym::Down | Keysym::Tab | Keysym::ISO_Left_Tab
                );
            if !nav_gt && let Some(ui_key) = map_keysym_to_ui(event.keysym, event.utf8.as_deref()) {
                let ui_mods = rusty_ui::InputModifiers {
                    ctrl: mods.ctrl,
                    shift: mods.shift,
                    alt: mods.alt,
                };
                let handled = with_font_engine(|fe| {
                    self.app
                        .handle_focused_text_key(ui_key, ui_mods, fe, ctx.clipboard_mut())
                });
                self.sync_clipboard_io(ctx);
                if handled {
                    self.sync_popup(ctx);
                    return;
                }
            }
        }
        let (key_name, ch_opt) = keysym_to_name_and_text(event.keysym, event.utf8.as_deref());
        let mut buf = [0_u8; 4];
        let text_owned = event
            .utf8
            .as_deref()
            .filter(|s| !s.is_empty() && s.chars().all(|c| !c.is_control()))
            .map(str::to_owned)
            .or_else(|| ch_opt.map(|ch| ch.encode_utf8(&mut buf).to_owned()));
        self.app
            .handle_key_input(key_name, text_owned.as_deref(), mods.ctrl, mods.shift);
        self.sync_clipboard_io(ctx);
        self.sync_popup(ctx);
    }

    fn on_pointer(&mut self, ctx: &mut AppContext<'_>, events: &[PointerEvent]) {
        for ev in events {
            let (px, py) = ev.position;
            let on_traffic = hit_test(&self.cached_or_layout(), px, py).is_some_and(|h| {
                matches!(
                    h,
                    FilesHit::TrafficClose | FilesHit::TrafficMinimize | FilesHit::TrafficMaximize
                )
            });
            if on_traffic != self.app.traffic_hovered {
                self.app.traffic_hovered = on_traffic;
                self.app.dirty = true;
            }
            match ev.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.handle_pointer_motion(ctx, px, py);
                }
                PointerEventKind::Leave { .. } => {
                    if self.pointer_pressed
                        && let Some(drag) = self.app.file_drag.as_ref()
                        && drag.active
                    {
                        let drag_sources = drag.sources.clone();
                        if self.start_external_file_drag(ctx, &drag_sources) {
                            self.app.file_drag = None;
                            self.pointer_pressed = false;
                            self.app.dirty = true;
                        }
                    }
                    if self.app.traffic_hovered {
                        self.app.traffic_hovered = false;
                        self.app.dirty = true;
                    }
                    self.app.set_hovered_hit(None);
                }
                PointerEventKind::Press { button, serial, .. } => {
                    self.handle_pointer_press(ctx, px, py, button, serial);
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => self.handle_pointer_release(),
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    source,
                    ..
                } => self.handle_axis_scroll(ctx, px, py, horizontal, vertical, source),
                PointerEventKind::Release { .. } => {}
            }
        }
        self.sync_popup(ctx);
        self.sync_clipboard_io(ctx);
    }

    fn on_popup_pointer(&mut self, ctx: &mut AppContext<'_>, events: &[PointerEvent]) {
        let Some(spec) = self.active_popup_spec.as_ref() else {
            return;
        };
        let mut clicked_cmd: Option<PopupCommand> = None;
        let mut should_close = false;
        let mut hover_changed = false;

        for ev in events {
            let px = ev.position.0 + f64::from(spec.layout.rect.x - POPUP_SHADOW_PAD);
            let py = ev.position.1 + f64::from(spec.layout.rect.y - POPUP_SHADOW_PAD);
            match ev.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let next = spec
                        .layout
                        .items
                        .iter()
                        .position(|it| it.command.is_some() && it.rect.contains_f64(px, py));
                    if next != self.popup_hover_idx {
                        self.popup_hover_idx = next;
                        hover_changed = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.popup_hover_idx.take().is_some() {
                        hover_changed = true;
                    }
                }
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } => {
                    if let Some(item) = spec
                        .layout
                        .items
                        .iter()
                        .find(|it| it.rect.contains_f64(px, py))
                    {
                        if let Some(cmd) = &item.command {
                            clicked_cmd = Some(cmd.clone());
                        }
                    } else {
                        should_close = true;
                    }
                }
                _ => {}
            }
        }

        if let Some(cmd) = clicked_cmd {
            self.execute_popup_command(ctx, cmd);
            self.sync_popup(ctx);
        } else if should_close {
            self.app.popup = None;
            self.sync_popup(ctx);
        } else if hover_changed {
            ctx.request_popup_redraw();
        }
    }

    fn on_popup_closed(&mut self, _ctx: &mut AppContext<'_>) {
        self.active_popup_spec = None;
        self.popup_hover_idx = None;
        self.last_popup = None;
        if self.app.popup.take().is_some() {
            self.app.dirty = true;
        }
    }

    fn on_tick(&mut self, ctx: &mut AppContext<'_>) {
        let now = Instant::now();
        let _ = self.app.poll_background_loads();
        if self.app.poll_system_theme() {
            self.sync_glass(ctx);
        }
        let _ = self.app.update_caret_blink(now);
        if let Some(pasted) = ctx.clipboard_mut().poll_received_paste() {
            let changed = with_font_engine(|fe| self.app.apply_focused_paste(&pasted, fe));
            if changed {
                self.app.dirty = true;
            }
        }
        let title = format!("{} - RustyOS Files", self.app.window_title);
        ctx.set_title(&title);
        self.sync_popup(ctx);
        self.sync_clipboard_io(ctx);
        if !self.app.is_running() {
            let ok = self.app.picker_mode.is_none()
                || (self.app.picker_confirmed && !self.app.picker_cancelled);
            self.picker_result.store(ok, Ordering::Relaxed);
            ctx.request_exit();
            return;
        }
        if self.app.dirty || self.app.has_active_animations() || self.app.scrollbar_needs_tick(now)
        {
            let _ = self.app.tick_animations(now);
            self.app.dirty = false;
            ctx.request_redraw();
        }
    }

    fn poll_interval(&self) -> Duration {
        let now = Instant::now();
        if self.app.has_pending_jobs() {
            return Duration::from_millis(8);
        }
        let caret = self
            .app
            .has_focused_text_input()
            .then(|| self.app.time_until_caret_blink(now));
        let scroll = self.app.time_until_scrollbar_tick(now);
        let next = match (caret, scroll) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let refresh = Duration::from_millis(500);
        next.map_or(refresh, |d| d.min(refresh))
    }

    fn on_clipboard_offer_changed(&mut self, ctx: &mut AppContext<'_>) {
        if self.ignore_next_selection_clear {
            self.ignore_next_selection_clear = false;
            return;
        }
        self.app.has_external_uri_clipboard = ctx
            .has_clipboard_mime("x-special/gnome-copied-files")
            || ctx.has_clipboard_mime("text/uri-list");
    }

    fn on_clipboard_data(&mut self, _ctx: &mut AppContext<'_>, mime: &str, data: &[u8]) {
        let Some(dest_dir) = self.pending_external_paste_dir.take() else {
            return;
        };
        let text = String::from_utf8_lossy(data);
        let (copy_mode, paths) = if mime == "x-special/gnome-copied-files" {
            parse_gnome_copied_files(&text).unwrap_or_else(|| (true, parse_uri_list_paths(&text)))
        } else {
            (true, parse_uri_list_paths(&text))
        };
        if !paths.is_empty() {
            self.app.action_drop_paths(paths, dest_dir, copy_mode);
        }
    }

    fn on_dnd_motion(
        &mut self,
        _ctx: &mut AppContext<'_>,
        x: f64,
        y: f64,
        mimes: &[String],
    ) -> bool {
        let layout = self.layout();
        let hit = hit_test(&layout, x, y);
        self.app.set_hovered_hit(hit);
        mimes.iter().any(|m| m == "text/uri-list")
    }

    fn on_dnd_leave(&mut self, _ctx: &mut AppContext<'_>) {
        self.app.set_hovered_hit(None);
    }

    fn on_dnd_drop(
        &mut self,
        _ctx: &mut AppContext<'_>,
        x: f64,
        y: f64,
        is_move: bool,
        _mime: &str,
        data: &[u8],
    ) {
        let layout = self.layout();
        let hit = hit_test(&layout, x, y);
        let target = resolve_drop_target(&self.app, hit.as_ref()).or_else(|| {
            self.app
                .active_writable_directory()
                .map(ResolvedDropTarget::Directory)
        });
        let Some(target) = target else {
            return;
        };
        let paths = parse_uri_list_paths(&String::from_utf8_lossy(data));
        if paths.is_empty() {
            return;
        }
        match target {
            ResolvedDropTarget::Directory(dest_dir) => {
                self.app.action_drop_paths(paths, dest_dir, !is_move);
            }
            ResolvedDropTarget::Trash => self.app.action_trash_paths(paths),
        }
    }

    fn paint(
        &mut self,
        _painter: &mut rusty_ui::UiPainter<'_>,
        _fonts: &mut rusty_ui::UiFontEngine,
        _width: u32,
        _height: u32,
    ) {
    }

    fn paint_bgra(
        &mut self,
        canvas_bgra: &mut [u8],
        width: u32,
        height: u32,
        has_glass: bool,
    ) -> bool {
        let layout = self.layout_for(width, height);
        let sidebar_glass = has_glass && !self.app.reduce_transparency;
        render_window_pixels_with_options(
            canvas_bgra,
            width,
            height,
            &self.app,
            &layout,
            false,
            sidebar_glass,
        );
        true
    }

    fn paint_popup_bgra(&mut self, canvas_bgra: &mut [u8], width: u32, height: u32) -> bool {
        let Some(spec) = &self.active_popup_spec else {
            return false;
        };
        render_popup_surface_pixels_with_style(
            canvas_bgra,
            width,
            height,
            &spec.layout,
            self.popup_hover_idx,
            self.app.theme,
            self.app.accent_index,
            !self.app.reduce_transparency,
        );
        true
    }
}

fn update_cursor_shape(ctx: &mut AppContext<'_>, hit: Option<&FilesHit>, layout: &FilesLayout) {
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
    ctx.set_cursor_shape(shape);
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
        Keysym::period => "period",
        Keysym::greater => "greater",
        Keysym::KP_Decimal => "KP_Decimal",
        _ => "",
    };
    let ch = utf8
        .and_then(|s| s.chars().next())
        .filter(|c| !c.is_control())
        .or_else(|| keysym.key_char().filter(|c| !c.is_control()));
    (name, ch)
}

/// Runs `rusty-files` through the shared `rusty-app` Wayland host.
///
/// # Errors
/// Returns [`AppError`] on Wayland connection, global binding, or event loop failure.
pub(crate) fn run(options: &StartupOptions) -> Result<bool, AppError> {
    let locs = UserFileLocations::detect();
    let app = FilesAppState::new(locs, options);
    let initial_title = format!("{} - RustyOS Files", app.window_title);
    let glass = (!app.reduce_transparency).then(|| {
        let wi = i32::try_from(DEFAULT_WIDTH).unwrap_or(1).max(1);
        let hi = i32::try_from(DEFAULT_HEIGHT).unwrap_or(1).max(1);
        let glass_w = (SIDEBAR_WIDTH + WINDOW_CORNER_RADIUS).min(wi).max(1);
        LiquidGlassConfig::sidebar(u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14)).with_region(
            GlassRegion::Rect {
                x: 0,
                y: 0,
                width: glass_w,
                height: hi,
            },
        )
    });
    let mut cfg = AppConfig::new(
        initial_title,
        "org.rustyos.files",
        (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    )
    .with_min_size(640, 420)
    .with_decorations(WindowDecorationMode::Client)
    .with_no_gpu(options.no_gpu);
    if let Some(g) = glass {
        cfg = cfg.with_liquid_glass(g);
    }

    let picker_result = Arc::new(AtomicBool::new(app.picker_mode.is_none()));
    let state = FilesApp {
        app,
        layout_cache: None,
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        pointer_pressed: false,
        click_streak: 1,
        last_click: None,
        last_input_serial: 0,
        last_popup: None,
        active_popup_spec: None,
        popup_hover_idx: None,
        pending_external_paste_dir: None,
        ignore_next_selection_clear: false,
        picker_result: Arc::clone(&picker_result),
    };
    run_app(cfg, state)?;
    Ok(picker_result.load(Ordering::Relaxed))
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
        let decoded = super::parse_uri_list_paths(&format!(
            "# comment\r\n{encoded}file://localhost/var/log/a%20b.log\r\n"
        ));
        assert_eq!(
            decoded,
            vec![
                PathBuf::from("/home/alice/My Report #1.pdf"),
                PathBuf::from("/tmp/notes.txt"),
                PathBuf::from("/var/log/a b.log"),
            ]
        );
    }

    #[test]
    fn encodes_and_parses_gnome_copied_files_copy_and_cut() {
        let paths = vec![
            PathBuf::from("/tmp/doc 1.txt"),
            PathBuf::from("/tmp/img.png"),
        ];
        let copy_raw = super::encode_gnome_copied_files(
            rusty_services::file_manager::TransferKind::Copy,
            &paths,
        );
        assert_eq!(
            super::parse_gnome_copied_files(&copy_raw),
            Some((true, paths.clone()))
        );

        let cut_raw = super::encode_gnome_copied_files(
            rusty_services::file_manager::TransferKind::Move,
            &paths,
        );
        assert_eq!(
            super::parse_gnome_copied_files(&cut_raw),
            Some((false, paths))
        );
        assert_eq!(
            super::parse_gnome_copied_files("invalid\nfile:///tmp/a"),
            None
        );
    }

    #[test]
    fn resolves_sidebar_trash_drop_target_when_not_inside_trash() {
        let opts = crate::state::StartupOptions::default();
        let locs = rusty_services::file_manager::UserFileLocations::detect();
        let mut app = crate::state::FilesAppState::new(locs, &opts);
        let trash_hit = crate::layout::FilesHit::SidebarItem(
            rusty_services::file_manager::SidebarLocation::Trash,
        );
        assert_eq!(
            super::resolve_drop_target(&app, Some(&trash_hit)),
            Some(super::ResolvedDropTarget::Trash)
        );
        app.navigate_sidebar(rusty_services::file_manager::SidebarLocation::Trash, false);
        assert_eq!(super::resolve_drop_target(&app, Some(&trash_hit)), None);
    }
}
