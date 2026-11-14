//! macOS-aligned Liquid Glass right-click context menus (`RUSTYOS-064`).

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::input::keyboard::Keysym;
use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::config::DownloadsSortMode;
use crate::surfaces::{GlassPanelStyle, UiGlyphKind};
use crate::text::FontWeight;

use super::CompositorRenderElement;
use super::dock::DockHit;
use super::tokens::{
    COLOR_BORDER, MENU_POPOVER_RADIUS, TOP_BAR_HEIGHT, make_solid_element, push_liquid_glass_panel,
    push_rounded_glass_box, truncate_ascii,
};
use super::top_bar::{MENU_ITEM_HEIGHT, MENU_POPOVER_PAD_Y, MENU_SEPARATOR_HEIGHT};

const MENU_TEXT_BRIGHT: [u8; 4] = [244, 240, 250, 255];
const MENU_TEXT_MUTED: [u8; 4] = [166, 155, 181, 255];
const MENU_TEXT_DISABLED: [u8; 4] = [132, 122, 148, 175];
const MENU_TEXT_DANGER: [u8; 4] = [255, 134, 148, 255];
const CONTEXT_MENU_MIN_WIDTH: i32 = 216;
const CONTEXT_MENU_MAX_WIDTH: i32 = 296;
const SUBMENU_GAP_PX: i32 = 4;

/// Executable leaf or submenu action triggered from a compositor context menu (`RUSTYOS-064`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    /// Creates a new uniquely named directory in `~/Desktop`.
    DesktopNewFolder,
    /// Opens `~/Desktop` via `xdg-open` / file manager.
    DesktopOpenFiles,
    /// Opens `System Settings -> Appearance`.
    DesktopChangeWallpaper,
    /// Opens `System Settings -> Displays`.
    DesktopDisplaySettings,
    /// Launches the preferred terminal emulator.
    DesktopOpenTerminal,
    /// Enters Workspace Overview / Mission Control.
    DesktopMissionControl,
    /// Toggles `Tile` / `Float` layout mode for the active workspace.
    DesktopToggleLayoutMode,

    /// Launches a pinned Dock application slot.
    DockLaunchApp(DockHit),
    /// Toggles the pinned application's entry in `UserConfig::autostart`.
    DockToggleOpenAtLogin(DockHit),
    /// Opens Mission Control / Workspace Overview to inspect all windows.
    DockShowAllWindows,
    /// Sends a graceful close request to windows matching a pinned Dock application.
    DockQuitApp(DockHit),
    /// Sends `SIGKILL` to matching app client processes (`Alt + Right-Click`).
    DockForceQuitApp(DockHit),

    /// Toggles Workspace Overview from the Overview Dock slot.
    OverviewToggle,
    /// Switches active workspace (`1..=9`).
    SwitchWorkspace(u8),

    /// Focuses or unminimizes the selected Dock window.
    WindowFocusOrRestore(rusty_wm::WindowId),
    /// Minimizes the selected Dock window.
    WindowMinimize(rusty_wm::WindowId),
    /// Moves the selected Dock window to workspace `1..=9`.
    WindowMoveToWorkspace(rusty_wm::WindowId, u8),
    /// Requests graceful close (`xdg_toplevel::close` / `WM_DELETE_WINDOW`).
    WindowQuit(rusty_wm::WindowId),
    /// Force-terminates the selected window's owning client process (`Alt` modifier).
    WindowForceQuit(rusty_wm::WindowId),

    /// Opens `~/Downloads` in the file manager.
    DownloadsOpenFolder,
    /// Opens an individual file or directory listed in the `Downloads` menu.
    DownloadsOpenEntry(std::path::PathBuf),
    /// Changes `[dock].downloads_sort` and persists `UserConfig`.
    DownloadsSetSort(DownloadsSortMode),

    /// Opens `trash:///`.
    TrashOpen,
    /// Starts non-blocking background cleanup of XDG Trash (`files` & `info`).
    TrashEmpty,

    /// Parent row marker that expands a cascade submenu instead of closing immediately.
    OpenSubmenu,
}

/// Visual and interactive row entry inside a context menu panel (`RUSTYOS-064`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuEntry {
    /// Interactive action or submenu parent item.
    Item {
        /// Primary menu row title.
        label: String,
        /// Leading monochrome glyph (`UiGlyphKind`).
        icon: UiGlyphKind,
        /// Optional right-aligned keyboard shortcut or size/meta hint.
        right_hint: Option<String>,
        /// Draws a leading/trailing state checkmark (`✓`) when `true`.
        checked: bool,
        /// Grays out and skips selection when `false`.
        enabled: bool,
        /// Tints destructive actions (`Force Quit`, `Empty Trash...`).
        danger: bool,
        /// Leaf action invoked on click or `Enter`.
        action: ContextMenuAction,
        /// Optional 1-level cascade submenu entries.
        submenu: Vec<ContextMenuEntry>,
    },
    /// Thin horizontal hairline divider between sections.
    Separator,
}

impl ContextMenuEntry {
    /// Creates an enabled leaf context menu item.
    #[must_use]
    pub fn item(label: impl Into<String>, icon: UiGlyphKind, action: ContextMenuAction) -> Self {
        Self::Item {
            label: label.into(),
            icon,
            right_hint: None,
            checked: false,
            enabled: true,
            danger: false,
            action,
            submenu: Vec::new(),
        }
    }

    /// Creates a submenu parent item displaying a trailing chevron (`>`).
    #[must_use]
    pub fn submenu_item(
        label: impl Into<String>,
        icon: UiGlyphKind,
        submenu: Vec<ContextMenuEntry>,
    ) -> Self {
        Self::Item {
            label: label.into(),
            icon,
            right_hint: None,
            checked: false,
            enabled: !submenu.is_empty(),
            danger: false,
            action: ContextMenuAction::OpenSubmenu,
            submenu,
        }
    }

    /// Attaches a right-aligned shortcut or detail hint to an [`Self::Item`].
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        if let Self::Item { right_hint, .. } = &mut self {
            *right_hint = Some(hint.into());
        }
        self
    }

    /// Marks whether an [`Self::Item`] shows an active checkmark (`✓`).
    #[must_use]
    pub fn with_checked(mut self, is_checked: bool) -> Self {
        if let Self::Item { checked, .. } = &mut self {
            *checked = is_checked;
        }
        self
    }

    /// Marks whether an [`Self::Item`] is interactive.
    #[must_use]
    pub fn with_enabled(mut self, is_enabled: bool) -> Self {
        if let Self::Item { enabled, .. } = &mut self {
            *enabled = is_enabled;
        }
        self
    }

    /// Marks whether an [`Self::Item`] uses danger tinting (`Force Quit`, `Empty Trash...`).
    #[must_use]
    pub fn with_danger(mut self, is_danger: bool) -> Self {
        if let Self::Item { danger, .. } = &mut self {
            *danger = is_danger;
        }
        self
    }

    fn is_enabled_item(&self) -> bool {
        matches!(self, Self::Item { enabled: true, .. })
    }
}

/// Target surface/slot that spawned the open context menu (`RUSTYOS-064`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuTarget {
    /// Empty Desktop area on an output.
    Desktop,
    /// Specific slot inside the bottom Dock.
    DockSlot(DockHit),
}

/// Complete live compositor context menu overlay state (`RUSTYOS-064`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuState {
    /// Output identifier where the menu opened and is clamped.
    pub output_id: rusty_wm::OutputId,
    /// Output-local logical anchor coordinates of the initial right-click.
    pub anchor_local: Point<i32, Logical>,
    /// Context target that generated `entries`.
    pub target: ContextMenuTarget,
    /// Root context menu rows.
    pub entries: Vec<ContextMenuEntry>,
    /// Highlighted row index inside `entries`.
    pub selected_row: Option<usize>,
    /// Expanded parent index inside `entries`, if a cascade submenu is open.
    pub open_submenu_parent: Option<usize>,
    /// Highlighted row index inside the expanded submenu.
    pub submenu_selected_row: Option<usize>,
}

impl ContextMenuState {
    /// Constructs a new closed-submenu context menu anchored at `anchor_local`.
    #[must_use]
    pub fn new(
        output_id: rusty_wm::OutputId,
        anchor_local: Point<i32, Logical>,
        target: ContextMenuTarget,
        entries: Vec<ContextMenuEntry>,
    ) -> Self {
        Self {
            output_id,
            anchor_local,
            target,
            entries,
            selected_row: None,
            open_submenu_parent: None,
            submenu_selected_row: None,
        }
    }

    /// Returns expanded submenu entries when `open_submenu_parent` points to a valid submenu item.
    #[must_use]
    pub fn open_submenu_entries(&self) -> Option<&[ContextMenuEntry]> {
        let idx = self.open_submenu_parent?;
        match self.entries.get(idx)? {
            ContextMenuEntry::Item { submenu, .. } if !submenu.is_empty() => Some(submenu),
            _ => None,
        }
    }
}

/// Outer popover rectangle and row rectangles for an open cascade submenu (`RUSTYOS-064`).
pub type ContextMenuSubmenuLayout = (
    Rectangle<i32, smithay::utils::Physical>,
    Vec<Rectangle<i32, smithay::utils::Physical>>,
);

/// Computed bounds for the primary popover and optional cascade submenu (`RUSTYOS-064`).
#[derive(Debug, Clone)]
pub struct ContextMenuLayout {
    /// Outer primary popover rectangle in output-local logical pixels.
    pub primary_rect: Rectangle<i32, smithay::utils::Physical>,
    /// Row rectangles matching `state.entries` 1:1.
    pub primary_rows: Vec<Rectangle<i32, smithay::utils::Physical>>,
    /// Expanded cascade submenu popover & row rectangles when open.
    pub submenu: Option<ContextMenuSubmenuLayout>,
}

/// Pointer hit result inside [`ContextMenuState`] (`RUSTYOS-064`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuHit {
    /// Pointer landed outside all open context menu panels.
    Outside,
    /// Pointer landed on non-interactive padding, separator, or disabled row.
    Inside,
    /// Pointer landed on a submenu parent row in the primary panel.
    ExpandSubmenu(usize),
    /// Pointer clicked an enabled executable item.
    Action(ContextMenuAction),
}

/// Keyboard handling result returned by [`handle_context_menu_key`] (`RUSTYOS-064`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuKeyOutcome {
    /// Key did not change state.
    None,
    /// Selection or submenu focus changed; request shell redraw.
    Redraw,
    /// Dismiss the context menu.
    Close,
    /// Execute a leaf action and dismiss the menu.
    Execute(ContextMenuAction),
}

fn measure_panel_size(entries: &[ContextMenuEntry]) -> (i32, i32) {
    let mut max_w = CONTEXT_MENU_MIN_WIDTH;
    let mut content_h = 0;
    for entry in entries {
        match entry {
            ContextMenuEntry::Item {
                label,
                right_hint,
                checked,
                submenu,
                ..
            } => {
                let shown = truncate_ascii(label, 28);
                let label_w = crate::text::measure_ui_text_width(&shown, 12.0, FontWeight::Regular);
                let right_w = if !submenu.is_empty() || *checked {
                    24
                } else if let Some(hint) = right_hint {
                    crate::text::measure_ui_text_width(hint, 11.0, FontWeight::Regular) + 18
                } else {
                    0
                };
                max_w = max_w.max(36 + label_w + 16 + right_w + 14);
                content_h += MENU_ITEM_HEIGHT;
            }
            ContextMenuEntry::Separator => {
                content_h += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    (
        max_w.clamp(CONTEXT_MENU_MIN_WIDTH, CONTEXT_MENU_MAX_WIDTH),
        MENU_POPOVER_PAD_Y * 2 + content_h,
    )
}

fn build_panel_rows(
    panel_rect: Rectangle<i32, smithay::utils::Physical>,
    entries: &[ContextMenuEntry],
) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    let mut rows = Vec::with_capacity(entries.len());
    let mut cursor_y = panel_rect.loc.y + MENU_POPOVER_PAD_Y;
    for entry in entries {
        let h = match entry {
            ContextMenuEntry::Item { .. } => MENU_ITEM_HEIGHT,
            ContextMenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
        };
        rows.push(Rectangle::new(
            Point::from((panel_rect.loc.x, cursor_y)),
            Size::from((panel_rect.size.w, h)),
        ));
        cursor_y += h;
    }
    rows
}

fn clamp_panel_in_output(
    screen_size: Size<i32, smithay::utils::Physical>,
    is_primary_output: bool,
    raw_x: i32,
    raw_y: i32,
    raw_w: i32,
    raw_h: i32,
) -> Rectangle<i32, smithay::utils::Physical> {
    let min_top = if is_primary_output {
        TOP_BAR_HEIGHT + 6
    } else {
        8
    };
    let pw = raw_w.min((screen_size.w - 16).max(160));
    let ph =
        raw_h.min((screen_size.h - min_top - 8).max(MENU_ITEM_HEIGHT + MENU_POPOVER_PAD_Y * 2));
    let px = raw_x.clamp(8, (screen_size.w - pw - 8).max(8));
    let py = raw_y.clamp(min_top, (screen_size.h - ph - 8).max(min_top));
    Rectangle::new(Point::from((px, py)), Size::from((pw, ph)))
}

/// Computes multi-monitor clamped rectangles for [`ContextMenuState`] (`RUSTYOS-064`).
#[must_use]
pub fn compute_context_menu_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    state: &ContextMenuState,
    is_primary_output: bool,
) -> ContextMenuLayout {
    let (raw_w, raw_h) = measure_panel_size(&state.entries);
    let raw_x = if state.anchor_local.x + raw_w > screen_size.w - 8 {
        state.anchor_local.x - raw_w
    } else {
        state.anchor_local.x
    };
    let raw_y = if state.anchor_local.y + raw_h > screen_size.h - 8 {
        state.anchor_local.y - raw_h
    } else {
        state.anchor_local.y
    };
    let primary_rect =
        clamp_panel_in_output(screen_size, is_primary_output, raw_x, raw_y, raw_w, raw_h);
    let primary_rows = build_panel_rows(primary_rect, &state.entries);

    let submenu = state
        .open_submenu_parent
        .zip(state.open_submenu_entries())
        .and_then(|(parent_idx, sub_entries)| {
            let parent_row = primary_rows.get(parent_idx)?;
            let (sw, sh) = measure_panel_size(sub_entries);
            let right_x = primary_rect.loc.x + primary_rect.size.w + SUBMENU_GAP_PX;
            let sub_x = if right_x + sw <= screen_size.w - 8 {
                right_x
            } else {
                primary_rect.loc.x - sw - SUBMENU_GAP_PX
            };
            let sub_y = parent_row.loc.y - MENU_POPOVER_PAD_Y;
            let sub_rect =
                clamp_panel_in_output(screen_size, is_primary_output, sub_x, sub_y, sw, sh);
            let sub_rows = build_panel_rows(sub_rect, sub_entries);
            Some((sub_rect, sub_rows))
        });

    ContextMenuLayout {
        primary_rect,
        primary_rows,
        submenu,
    }
}

fn rect_contains(rect: Rectangle<i32, smithay::utils::Physical>, x: f64, y: f64) -> bool {
    let rx = f64::from(rect.loc.x);
    let ry = f64::from(rect.loc.y);
    x >= rx && x < rx + f64::from(rect.size.w) && y >= ry && y < ry + f64::from(rect.size.h)
}

fn hit_row_index(
    rows: &[Rectangle<i32, smithay::utils::Physical>],
    x: f64,
    y: f64,
) -> Option<usize> {
    rows.iter().position(|row| rect_contains(*row, x, y))
}

/// Hit-tests `(local_x, local_y)` against an open context menu and its cascade submenu (`RUSTYOS-064`).
#[must_use]
pub fn hit_test_context_menu(
    screen_size: Size<i32, smithay::utils::Physical>,
    state: &ContextMenuState,
    is_primary_output: bool,
    local_x: f64,
    local_y: f64,
) -> ContextMenuHit {
    let layout = compute_context_menu_layout(screen_size, state, is_primary_output);

    if let Some((sub_rect, ref sub_rows)) = layout.submenu
        && rect_contains(sub_rect, local_x, local_y)
    {
        if let Some(sub_entries) = state.open_submenu_entries()
            && let Some(row_idx) = hit_row_index(sub_rows, local_x, local_y)
            && let Some(ContextMenuEntry::Item {
                enabled: true,
                action,
                ..
            }) = sub_entries.get(row_idx)
        {
            return ContextMenuHit::Action(action.clone());
        }
        return ContextMenuHit::Inside;
    }

    if !rect_contains(layout.primary_rect, local_x, local_y) {
        return ContextMenuHit::Outside;
    }

    if let Some(row_idx) = hit_row_index(&layout.primary_rows, local_x, local_y)
        && let Some(ContextMenuEntry::Item {
            enabled: true,
            action,
            submenu,
            ..
        }) = state.entries.get(row_idx)
    {
        if !submenu.is_empty() {
            return ContextMenuHit::ExpandSubmenu(row_idx);
        }
        return ContextMenuHit::Action(action.clone());
    }

    ContextMenuHit::Inside
}

/// Updates hovered row and cascade submenu expansion on pointer motion; returns `true` if changed (`RUSTYOS-064`).
pub fn update_context_menu_hover(
    screen_size: Size<i32, smithay::utils::Physical>,
    state: &mut ContextMenuState,
    is_primary_output: bool,
    local_x: f64,
    local_y: f64,
) -> bool {
    let layout = compute_context_menu_layout(screen_size, state, is_primary_output);
    let prev = (
        state.selected_row,
        state.open_submenu_parent,
        state.submenu_selected_row,
    );

    if let Some((sub_rect, ref sub_rows)) = layout.submenu
        && rect_contains(sub_rect, local_x, local_y)
    {
        let next_sub = state.open_submenu_entries().and_then(|sub_entries| {
            hit_row_index(sub_rows, local_x, local_y).filter(|&idx| {
                sub_entries
                    .get(idx)
                    .is_some_and(ContextMenuEntry::is_enabled_item)
            })
        });
        state.submenu_selected_row = next_sub;
        return prev
            != (
                state.selected_row,
                state.open_submenu_parent,
                state.submenu_selected_row,
            );
    }

    if rect_contains(layout.primary_rect, local_x, local_y) {
        let hit_idx = hit_row_index(&layout.primary_rows, local_x, local_y);
        if let Some((
            idx,
            ContextMenuEntry::Item {
                enabled: true,
                submenu,
                ..
            },
        )) = hit_idx.and_then(|idx| state.entries.get(idx).map(|e| (idx, e)))
        {
            state.selected_row = Some(idx);
            if submenu.is_empty() {
                state.open_submenu_parent = None;
                state.submenu_selected_row = None;
            } else {
                if state.open_submenu_parent != Some(idx) {
                    state.submenu_selected_row = None;
                }
                state.open_submenu_parent = Some(idx);
            }
        } else {
            state.selected_row = None;
            state.submenu_selected_row = None;
        }
    } else {
        state.selected_row = None;
        state.submenu_selected_row = None;
    }

    prev != (
        state.selected_row,
        state.open_submenu_parent,
        state.submenu_selected_row,
    )
}

fn step_enabled_index(
    entries: &[ContextMenuEntry],
    current: Option<usize>,
    forward: bool,
) -> Option<usize> {
    let enabled_indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| entry.is_enabled_item().then_some(idx))
        .collect();
    if enabled_indices.is_empty() {
        return None;
    }
    let Some(cur) = current else {
        return if forward {
            enabled_indices.first().copied()
        } else {
            enabled_indices.last().copied()
        };
    };
    let pos = enabled_indices.iter().position(|&idx| idx == cur);
    match pos {
        Some(i) if forward => Some(enabled_indices[(i + 1) % enabled_indices.len()]),
        Some(i) => Some(enabled_indices[(i + enabled_indices.len() - 1) % enabled_indices.len()]),
        None => enabled_indices.first().copied(),
    }
}

/// Processes keyboard navigation inside an active [`ContextMenuState`] (`RUSTYOS-064`).
pub fn handle_context_menu_key(state: &mut ContextMenuState, sym: Keysym) -> ContextMenuKeyOutcome {
    match sym {
        Keysym::Escape => {
            if state.open_submenu_parent.is_some() {
                state.open_submenu_parent = None;
                state.submenu_selected_row = None;
                ContextMenuKeyOutcome::Redraw
            } else {
                ContextMenuKeyOutcome::Close
            }
        }
        Keysym::Left => {
            if state.open_submenu_parent.is_some() {
                state.open_submenu_parent = None;
                state.submenu_selected_row = None;
                ContextMenuKeyOutcome::Redraw
            } else {
                ContextMenuKeyOutcome::None
            }
        }
        Keysym::Right => {
            if let Some(parent_idx) = state.selected_row
                && let Some(ContextMenuEntry::Item {
                    enabled: true,
                    submenu,
                    ..
                }) = state.entries.get(parent_idx)
                && !submenu.is_empty()
            {
                state.open_submenu_parent = Some(parent_idx);
                state.submenu_selected_row = step_enabled_index(submenu, None, true);
                return ContextMenuKeyOutcome::Redraw;
            }
            ContextMenuKeyOutcome::None
        }
        Keysym::Down | Keysym::Up => {
            let forward = sym == Keysym::Down;
            if state.open_submenu_parent.is_some()
                && state.submenu_selected_row.is_some()
                && let Some(sub_entries) = state.open_submenu_entries()
            {
                state.submenu_selected_row =
                    step_enabled_index(sub_entries, state.submenu_selected_row, forward);
            } else {
                state.open_submenu_parent = None;
                state.submenu_selected_row = None;
                state.selected_row =
                    step_enabled_index(&state.entries, state.selected_row, forward);
            }
            ContextMenuKeyOutcome::Redraw
        }
        Keysym::Return | Keysym::KP_Enter => {
            if let Some(sub_idx) = state.submenu_selected_row
                && let Some(sub_entries) = state.open_submenu_entries()
                && let Some(ContextMenuEntry::Item {
                    enabled: true,
                    action,
                    ..
                }) = sub_entries.get(sub_idx)
            {
                return ContextMenuKeyOutcome::Execute(action.clone());
            }
            if let Some(row_idx) = state.selected_row
                && let Some(ContextMenuEntry::Item {
                    enabled: true,
                    action,
                    submenu,
                    ..
                }) = state.entries.get(row_idx)
            {
                if submenu.is_empty() {
                    return ContextMenuKeyOutcome::Execute(action.clone());
                }
                state.open_submenu_parent = Some(row_idx);
                state.submenu_selected_row = step_enabled_index(submenu, None, true);
                return ContextMenuKeyOutcome::Redraw;
            }
            ContextMenuKeyOutcome::None
        }
        _ => ContextMenuKeyOutcome::None,
    }
}

fn render_panel_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    panel_rect: Rectangle<i32, smithay::utils::Physical>,
    entries: &[ContextMenuEntry],
    rows: &[Rectangle<i32, smithay::utils::Physical>],
    selected_row: Option<usize>,
    elements: &mut Vec<
        smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<GlesRenderer>,
    >,
) {
    let mx = panel_rect.loc.x;
    let mw = panel_rect.size.w;
    for (idx, (entry, row_rect)) in entries.iter().zip(rows).enumerate() {
        let ContextMenuEntry::Item {
            label,
            right_hint,
            enabled,
            danger,
            submenu,
            ..
        } = entry
        else {
            continue;
        };
        let hovered = *enabled && selected_row == Some(idx);
        let fg = if !*enabled {
            MENU_TEXT_DISABLED
        } else if hovered {
            [255, 252, 255, 255]
        } else if *danger {
            MENU_TEXT_DANGER
        } else {
            MENU_TEXT_BRIGHT
        };
        let shown = truncate_ascii(label, 28);
        if let Some(el) = text_cache.render_label(
            renderer,
            &shown,
            12.0,
            fg,
            Point::from((mx + 34, row_rect.loc.y + 6)),
        ) {
            elements.push(el);
        }
        if submenu.is_empty()
            && let Some(hint) = right_hint
        {
            let (hint_w, _) = text_cache.measure_text(hint, 11.0);
            let hint_fg = if hovered {
                [248, 240, 255, 255]
            } else {
                MENU_TEXT_MUTED
            };
            if let Some(el) = text_cache.render_label(
                renderer,
                hint,
                11.0,
                hint_fg,
                Point::from((mx + mw - 14 - hint_w, row_rect.loc.y + 7)),
            ) {
                elements.push(el);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_panel_shapes(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    panel_rect: Rectangle<i32, smithay::utils::Physical>,
    entries: &[ContextMenuEntry],
    rows: &[Rectangle<i32, smithay::utils::Physical>],
    selected_row: Option<usize>,
    glass_opacity: f32,
    reduce_transparency: bool,
    out: &mut Vec<CompositorRenderElement>,
) {
    let mx = panel_rect.loc.x;
    let mw = panel_rect.size.w;

    for (idx, (entry, row_rect)) in entries.iter().zip(rows).enumerate() {
        let row_y = row_rect.loc.y;
        match entry {
            ContextMenuEntry::Item {
                icon,
                checked,
                enabled,
                danger,
                submenu,
                ..
            } => {
                let hovered = *enabled && selected_row == Some(idx);
                let icon_tint = if !*enabled {
                    MENU_TEXT_DISABLED
                } else if hovered {
                    [255, 252, 255, 255]
                } else if *danger {
                    MENU_TEXT_DANGER
                } else {
                    [218, 208, 238, 240]
                };
                if let Some(el) = shape_cache.ui_glyph_element(
                    renderer,
                    *icon,
                    14,
                    icon_tint,
                    Point::from((mx + 13, row_y + 7)),
                ) {
                    out.push(CompositorRenderElement::Memory(el));
                }
                if !submenu.is_empty() {
                    if let Some(el) = shape_cache.ui_glyph_element(
                        renderer,
                        UiGlyphKind::ChevronRight,
                        12,
                        icon_tint,
                        Point::from((mx + mw - 22, row_y + 8)),
                    ) {
                        out.push(CompositorRenderElement::Memory(el));
                    }
                } else if *checked
                    && let Some(el) = shape_cache.ui_glyph_element(
                        renderer,
                        UiGlyphKind::Check,
                        12,
                        if hovered {
                            [255, 252, 255, 255]
                        } else {
                            [198, 152, 255, 255]
                        },
                        Point::from((mx + mw - 22, row_y + 8)),
                    )
                {
                    out.push(CompositorRenderElement::Memory(el));
                }
                if hovered {
                    push_rounded_glass_box(
                        out,
                        shape_cache,
                        renderer,
                        Rectangle::new(
                            Point::from((mx + 6, row_y + 1)),
                            Size::from((mw - 12, MENU_ITEM_HEIGHT - 2)),
                        ),
                        8,
                        [165, 110, 255, 200],
                        Some([215, 185, 255, 110]),
                    );
                }
            }
            ContextMenuEntry::Separator => {
                out.push(CompositorRenderElement::Solid(make_solid_element(
                    Rectangle::new(Point::from((mx + 12, row_y + 4)), Size::from((mw - 24, 1))),
                    COLOR_BORDER,
                )));
            }
        }
    }

    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        panel_rect,
        MENU_POPOVER_RADIUS,
        GlassPanelStyle::Popover,
        glass_opacity,
        reduce_transparency,
        Some((16, 52)),
    );
}

/// Appends context menu text labels in front-to-back order (`RUSTYOS-064`).
pub(super) fn render_context_menu_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    state: &ContextMenuState,
    is_primary_output: bool,
    elements: &mut Vec<
        smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<GlesRenderer>,
    >,
) {
    let layout = compute_context_menu_layout(screen_size, state, is_primary_output);
    if let Some((sub_rect, ref sub_rows)) = layout.submenu
        && let Some(sub_entries) = state.open_submenu_entries()
    {
        render_panel_text(
            text_cache,
            renderer,
            sub_rect,
            sub_entries,
            sub_rows,
            state.submenu_selected_row,
            elements,
        );
    }
    render_panel_text(
        text_cache,
        renderer,
        layout.primary_rect,
        &state.entries,
        &layout.primary_rows,
        state.selected_row.or(state.open_submenu_parent),
        elements,
    );
}

/// Appends context menu glass panels, glyphs, dividers, and hover highlights (`RUSTYOS-064`).
#[allow(clippy::too_many_arguments)]
pub(super) fn render_context_menu_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    glass_opacity: f32,
    reduce_transparency: bool,
    state: &ContextMenuState,
    is_primary_output: bool,
) {
    let layout = compute_context_menu_layout(screen_size, state, is_primary_output);
    if let Some((sub_rect, ref sub_rows)) = layout.submenu
        && let Some(sub_entries) = state.open_submenu_entries()
    {
        render_panel_shapes(
            shape_cache,
            renderer,
            sub_rect,
            sub_entries,
            sub_rows,
            state.submenu_selected_row,
            glass_opacity,
            reduce_transparency,
            out,
        );
    }
    render_panel_shapes(
        shape_cache,
        renderer,
        layout.primary_rect,
        &state.entries,
        &layout.primary_rows,
        state.selected_row.or(state.open_submenu_parent),
        glass_opacity,
        reduce_transparency,
        out,
    );
}

/// Generates standalone front-to-back render elements for a secondary output context menu (`RUSTYOS-064`).
#[allow(clippy::too_many_arguments)]
pub fn generate_context_menu_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    glass_opacity: f32,
    reduce_transparency: bool,
    state: &ContextMenuState,
    is_primary_output: bool,
) -> Vec<CompositorRenderElement> {
    let mut text = Vec::new();
    render_context_menu_text(
        text_cache,
        renderer,
        screen_size,
        state,
        is_primary_output,
        &mut text,
    );
    let mut out: Vec<CompositorRenderElement> = text
        .into_iter()
        .map(CompositorRenderElement::Memory)
        .collect();
    render_context_menu_composite(
        &mut out,
        shape_cache,
        renderer,
        screen_size,
        glass_opacity,
        reduce_transparency,
        state,
        is_primary_output,
    );
    out
}
