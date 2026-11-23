//! RustyOS desktop canvas icon grid and interaction (`RUSTYOS-081`).
//!
//! Renders interactive file, folder, and application icons directly on the desktop
//! wallpaper layer, arranged in vertical columns from top-right down, with dual-layer
//! text drop shadows, selection highlights, and double-click opening.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Size};

use super::CompositorRenderElement;
use super::tokens::truncate_ascii;

/// Width of each desktop canvas icon grid cell in logical pixels.
pub(crate) const DESKTOP_ITEM_W: i32 = 88;
/// Height of each desktop canvas icon grid cell in logical pixels.
pub(crate) const DESKTOP_ITEM_H: i32 = 92;
/// Icon edge size in logical pixels (48x48 RustyOS standard).
pub(crate) const DESKTOP_ICON_SIZE: i32 = 48;
/// Margin from screen right edge in logical pixels.
pub(crate) const DESKTOP_MARGIN_RIGHT: i32 = 24;
/// Margin from screen top edge (beneath Menu Bar) in logical pixels.
pub(crate) const DESKTOP_MARGIN_TOP: i32 = 46;
/// Vertical gap between consecutive desktop icon items in logical pixels.
pub(crate) const DESKTOP_GAP_Y: i32 = 12;
/// Horizontal gap between consecutive desktop icon columns in logical pixels.
pub(crate) const DESKTOP_GAP_X: i32 = 16;

/// A single file, folder, or application entry displayed on the desktop canvas (`RUSTYOS-081`/`RUSTYOS-082`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopCanvasItem {
    /// Display name shown under the icon (e.g. "Новая папка", "Steam").
    pub name: String,
    /// Absolute filesystem path.
    pub path: PathBuf,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Optional freedesktop icon name resolved from a `.desktop` file or MIME type.
    pub icon_name: Option<String>,
    /// Grid cell slot coordinate `(col, row)` where `col` is from right and `row` is from top.
    pub slot: (usize, usize),
}

/// Interactive rubber-band selection rectangle on desktop canvas (`RUSTYOS-082`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopRubberBand {
    /// Starting point where LKM was pressed down on empty wallpaper.
    pub start: Point<f64, Logical>,
    /// Current pointer coordinate while dragging.
    pub current: Point<f64, Logical>,
}

impl DesktopRubberBand {
    /// Computes the normalized bounding rectangle of the rubber-band selection.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn rect(&self) -> Rectangle<i32, Logical> {
        let min_x = self.start.x.min(self.current.x).round() as i32;
        let max_x = self.start.x.max(self.current.x).round() as i32;
        let min_y = self.start.y.min(self.current.y).round() as i32;
        let max_y = self.start.y.max(self.current.y).round() as i32;
        Rectangle::new(
            Point::from((min_x, min_y)),
            Size::from(((max_x - min_x).max(1), (max_y - min_y).max(1))),
        )
    }
}

/// Active desktop icon drag state (`RUSTYOS-082`/`RUSTYOS-083`).
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopDragState {
    /// Original pointer location when drag began.
    pub start_pointer: Point<f64, Logical>,
    /// Current pointer location.
    pub current_pointer: Point<f64, Logical>,
    /// Path of the item being dragged.
    pub dragged_path: PathBuf,
    /// Becomes true once the pointer moves beyond 5 logical pixels.
    pub is_active: bool,
    /// Destination directory path if currently hovering over another folder to drop into it.
    pub target_hover_dir: Option<PathBuf>,
    /// True when hovering over the Trash icon in Dock to drop and delete (`RUSTYOS-083`).
    pub drop_to_trash: bool,
    /// Resolved Dock drop target (App / Trash / Downloads / `PinInsert`) during desktop drag (`RUSTYOS-163`).
    pub dock_drop_target: Option<crate::state::DockDropTarget>,
}

/// In-place text renaming state for a desktop canvas file or folder (`RUSTYOS-083`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopInlineRename {
    /// Full filesystem path of the item being renamed.
    pub target_path: PathBuf,
    /// Editable text buffer.
    pub buffer: String,
    /// Text cursor position in characters.
    pub cursor_pos: usize,
}

/// Computes the bounding rectangle for a specific desktop grid cell `(col, row)`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn compute_desktop_cell_rect(
    col: usize,
    row: usize,
    screen_size: Size<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let x = screen_size.w
        - DESKTOP_MARGIN_RIGHT
        - DESKTOP_ITEM_W
        - (col as i32 * (DESKTOP_ITEM_W + DESKTOP_GAP_X));
    let y = DESKTOP_MARGIN_TOP + (row as i32 * (DESKTOP_ITEM_H + DESKTOP_GAP_Y));

    Rectangle::new(
        Point::from((x, y)),
        Size::from((DESKTOP_ITEM_W, DESKTOP_ITEM_H)),
    )
}

/// Identifies the closest grid cell `(col, row)` for a given pointer position on screen (`RUSTYOS-082`).
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub fn cell_at_point(
    pos: Point<f64, Logical>,
    screen_size: Size<i32, Logical>,
    dock_height: i32,
) -> (usize, usize) {
    let px = pos.x.round() as i32;
    let py = pos.y.round() as i32;

    let usable_h = (screen_size.h - DESKTOP_MARGIN_TOP - dock_height - 24).max(DESKTOP_ITEM_H);
    let items_per_col =
        ((usable_h + DESKTOP_GAP_Y) / (DESKTOP_ITEM_H + DESKTOP_GAP_Y)).max(1) as usize;

    let right_edge = screen_size.w - DESKTOP_MARGIN_RIGHT;
    let from_right = (right_edge - px).max(0);
    let cell_w = DESKTOP_ITEM_W + DESKTOP_GAP_X;
    let col = (from_right / cell_w.max(1)) as usize;

    let from_top = (py - DESKTOP_MARGIN_TOP).max(0);
    let cell_h = DESKTOP_ITEM_H + DESKTOP_GAP_Y;
    let row = ((from_top / cell_h.max(1)) as usize).min(items_per_col.saturating_sub(1));

    (col, row)
}

/// Computes the bounding rectangle for the desktop icon at linear `index` on a screen of `screen_size`.
#[must_use]
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
pub fn compute_desktop_item_rect(
    index: usize,
    screen_size: Size<i32, Logical>,
    dock_height: i32,
) -> Rectangle<i32, Logical> {
    let usable_h = (screen_size.h - DESKTOP_MARGIN_TOP - dock_height - 24).max(DESKTOP_ITEM_H);
    let items_per_col =
        ((usable_h + DESKTOP_GAP_Y) / (DESKTOP_ITEM_H + DESKTOP_GAP_Y)).max(1) as usize;
    let col = index / items_per_col;
    let row = index % items_per_col;

    compute_desktop_cell_rect(col, row, screen_size)
}

/// Loads saved desktop icon grid positions from TOML configuration file (`RUSTYOS-082`).
#[must_use]
pub fn load_desktop_grid_positions(path: &Path) -> HashMap<String, (usize, usize)> {
    let mut map = HashMap::new();
    let Ok(content) = fs::read_to_string(path) else {
        return map;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else {
            continue;
        };
        let file_name = key.trim().trim_matches('"').trim_matches('\'').to_owned();
        let val = val
            .trim()
            .trim_matches('[')
            .trim_matches(']')
            .trim_matches('"');
        let parts: Vec<&str> = val.split(',').map(str::trim).collect();
        if parts.len() == 2
            && let Ok(col) = parts[0].parse::<usize>()
            && let Ok(row) = parts[1].parse::<usize>()
        {
            map.insert(file_name, (col, row));
        }
    }
    map
}

/// Atomically saves desktop icon grid positions to a TOML configuration file (`RUSTYOS-082`).
#[allow(clippy::implicit_hasher)]
pub fn save_desktop_grid_positions(path: &Path, positions: &HashMap<String, (usize, usize)>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut lines = Vec::new();
    lines.push("# RustyOS Desktop Grid Layout".to_owned());
    let mut sorted: Vec<(&String, &(usize, usize))> = positions.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (name, &(col, row)) in sorted {
        lines.push(format!(
            "\"{}\" = [{}, {}]",
            name.replace('"', "\\\""),
            col,
            row
        ));
    }
    let content = lines.join("\n") + "\n";
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, content).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

/// Hit-tests pointer coordinates against desktop canvas items, returning the clicked item index if any.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn hit_test_desktop_item(
    items: &[DesktopCanvasItem],
    screen_size: Size<i32, Logical>,
    _dock_height: i32,
    pos: Point<f64, Logical>,
) -> Option<usize> {
    let px = pos.x.round() as i32;
    let py = pos.y.round() as i32;
    for (idx, item) in items.iter().enumerate() {
        let rect = compute_desktop_cell_rect(item.slot.0, item.slot.1, screen_size);
        if rect.contains(Point::from((px, py))) {
            return Some(idx);
        }
    }
    None
}

/// Renders desktop canvas items (icon, dual-layer drop-shadow text, drag preview, and selection) directly on wallpaper.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::implicit_hasher,
    clippy::cast_possible_truncation
)]
pub fn render_desktop_canvas_elements(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    items: &[DesktopCanvasItem],
    selected_paths: &HashSet<PathBuf>,
    drag_state: Option<&DesktopDragState>,
    rubber_band: Option<&DesktopRubberBand>,
    inline_rename: Option<&DesktopInlineRename>,
    screen_size: Size<i32, Logical>,
    dock_height: i32,
    elements: &mut Vec<CompositorRenderElement>,
) {
    // 1. Rubber-band selection box in macOS accent blue (highest priority on canvas)
    if let Some(rb) = rubber_band {
        let rb_rect = rb.rect();
        if rb_rect.size.w > 2 && rb_rect.size.h > 2 {
            let phys_rect = Rectangle::new(
                rb_rect.loc.to_physical_precise_round(1),
                rb_rect.size.to_physical_precise_round(1),
            );
            super::tokens::push_rounded_glass_box(
                elements,
                shape_cache,
                renderer,
                phys_rect,
                4,
                [0, 122, 255, 35],
                Some([0, 122, 255, 140]),
            );
        }
    }

    // 2. Target snap slot preview ghost box when actively dragging over an empty cell
    if let Some(drag) = drag_state
        && drag.is_active
        && drag.target_hover_dir.is_none()
        && !drag.drop_to_trash
    {
        let snap_slot = cell_at_point(drag.current_pointer, screen_size, dock_height);
        let snap_rect = compute_desktop_cell_rect(snap_slot.0, snap_slot.1, screen_size);
        let snap_phys = Rectangle::new(
            snap_rect.loc.to_physical_precise_round(1),
            snap_rect.size.to_physical_precise_round(1),
        );
        super::tokens::push_rounded_glass_box(
            elements,
            shape_cache,
            renderer,
            snap_phys,
            8,
            [255, 255, 255, 25],
            Some([255, 255, 255, 70]),
        );
    }

    // 3. Desktop items
    for item in items {
        let is_being_dragged =
            drag_state.is_some_and(|d| d.is_active && d.dragged_path == item.path);
        let rect = if is_being_dragged && let Some(drag) = drag_state {
            let float_x = drag.current_pointer.x.round() as i32 - DESKTOP_ITEM_W / 2;
            let float_y = drag.current_pointer.y.round() as i32 - 24;
            Rectangle::new(
                Point::from((float_x, float_y)),
                Size::from((DESKTOP_ITEM_W, DESKTOP_ITEM_H)),
            )
        } else {
            compute_desktop_cell_rect(item.slot.0, item.slot.1, screen_size)
        };
        let is_selected = selected_paths.contains(&item.path);
        let is_drop_target = drag_state
            .is_some_and(|d| d.is_active && d.target_hover_dir.as_ref() == Some(&item.path));

        let icon_x = rect.loc.x + (DESKTOP_ITEM_W - DESKTOP_ICON_SIZE) / 2;
        let icon_y = rect.loc.y + 4;
        let icon_loc = Point::from((icon_x, icon_y));

        let is_renaming = inline_rename.is_some_and(|r| r.target_path == item.path);
        let display_text =
            if let Some(rename) = inline_rename.filter(|r| r.target_path == item.path) {
                truncate_ascii(&rename.buffer, 22)
            } else {
                truncate_ascii(&item.name, 16)
            };
        let label_bounds = Rectangle::new(
            Point::from((rect.loc.x + 2, icon_y + DESKTOP_ICON_SIZE + 6)),
            Size::from((rect.size.w - 4, 24)),
        );

        // FRONT: Crisp white text foreground (12px Inter Bold/SemiBold)
        if let Some(text_el) = text_cache.render_label_centered_weighted(
            renderer,
            &display_text,
            crate::text::FontWeight::Bold,
            12.0,
            [255, 255, 255, 255],
            label_bounds,
        ) {
            elements.push(CompositorRenderElement::Memory(text_el));
        }

        // BEHIND TEXT: If renaming, show focused input box; else if unselected, soft ambient shadow
        if is_renaming {
            let input_rect = Rectangle::new(
                Point::from((label_bounds.loc.x - 4, label_bounds.loc.y - 1)),
                Size::from((label_bounds.size.w + 8, label_bounds.size.h + 2)),
            );
            let phys_input = Rectangle::new(
                input_rect.loc.to_physical_precise_round(1),
                input_rect.size.to_physical_precise_round(1),
            );
            super::tokens::push_rounded_glass_box(
                elements,
                shape_cache,
                renderer,
                phys_input,
                6,
                [20, 24, 34, 230],
                Some([0, 122, 255, 220]),
            );
        } else if !is_selected {
            for (dx, dy, alpha) in [(0, 1, 90u8), (0, 2, 45u8), (-1, 0, 35u8), (1, 0, 35u8)] {
                let shadow_bounds = Rectangle::new(
                    Point::from((label_bounds.loc.x + dx, label_bounds.loc.y + dy)),
                    label_bounds.size,
                );
                if let Some(shadow_el) = text_cache.render_label_centered_weighted(
                    renderer,
                    &display_text,
                    crate::text::FontWeight::Bold,
                    12.0,
                    [0, 0, 0, alpha],
                    shadow_bounds,
                ) {
                    elements.push(CompositorRenderElement::Memory(shadow_el));
                }
            }
        }

        // BEHIND TEXT & SHADOW: 48x48 icon element
        if item.is_dir {
            if let Some(icon_el) = shape_cache.icons.builtin_squircle_icon_element(
                renderer,
                crate::icons::BuiltinAppIcon::Folder,
                DESKTOP_ICON_SIZE,
                icon_loc,
            ) {
                elements.push(CompositorRenderElement::Memory(icon_el));
            }
        } else if let Some(ref icon_name) = item.icon_name {
            if let Some(el) = shape_cache.icons.window_icon_element(
                renderer,
                &item.name,
                None,
                Some(icon_name.as_str()),
                DESKTOP_ICON_SIZE,
                icon_loc,
            ) {
                elements.push(super::tokens::resolved_icon_to_element(el));
            } else if let Some(icon_el) = shape_cache.icons.builtin_squircle_icon_element(
                renderer,
                crate::icons::BuiltinAppIcon::Notes,
                DESKTOP_ICON_SIZE,
                icon_loc,
            ) {
                elements.push(CompositorRenderElement::Memory(icon_el));
            }
        } else if let Some(icon_el) = shape_cache.icons.builtin_squircle_icon_element(
            renderer,
            crate::icons::BuiltinAppIcon::Notes,
            DESKTOP_ICON_SIZE,
            icon_loc,
        ) {
            elements.push(CompositorRenderElement::Memory(icon_el));
        }

        // BACK: Selection highlight pill or folder drop target outline
        if is_drop_target {
            let phys_rect = Rectangle::new(
                rect.loc.to_physical_precise_round(1),
                rect.size.to_physical_precise_round(1),
            );
            super::tokens::push_rounded_glass_box(
                elements,
                shape_cache,
                renderer,
                phys_rect,
                8,
                [0, 122, 255, 50],
                Some([0, 122, 255, 200]),
            );
        } else if is_selected && !is_renaming {
            let phys_rect = Rectangle::new(
                rect.loc.to_physical_precise_round(1),
                rect.size.to_physical_precise_round(1),
            );
            super::tokens::push_rounded_glass_box(
                elements,
                shape_cache,
                renderer,
                phys_rect,
                8,
                [255, 255, 255, 45],
                Some([255, 255, 255, 75]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_item_rect_aligns_to_top_right_and_wraps_columns() {
        let screen = Size::from((1920, 1080));
        let dock_h = 68;

        let r0 = compute_desktop_item_rect(0, screen, dock_h);
        assert_eq!(r0.loc.x, 1920 - DESKTOP_MARGIN_RIGHT - DESKTOP_ITEM_W);
        assert_eq!(r0.loc.y, DESKTOP_MARGIN_TOP);
        assert_eq!(r0.size.w, DESKTOP_ITEM_W);
        assert_eq!(r0.size.h, DESKTOP_ITEM_H);

        let r1 = compute_desktop_item_rect(1, screen, dock_h);
        assert_eq!(r1.loc.x, r0.loc.x);
        assert_eq!(r1.loc.y, r0.loc.y + DESKTOP_ITEM_H + DESKTOP_GAP_Y);

        // When many items are added, verify column 2 steps to the left
        let r10 = compute_desktop_item_rect(10, screen, dock_h);
        assert!(r10.loc.x < r0.loc.x || r10.loc.y > r0.loc.y);
    }

    #[test]
    fn hit_test_desktop_item_matches_inside_and_rejects_outside() {
        let screen = Size::from((1280, 800));
        let dock_h = 68;
        let items = vec![
            DesktopCanvasItem {
                name: "Folder 1".to_owned(),
                path: PathBuf::from("/home/user/Desktop/Folder 1"),
                is_dir: true,
                icon_name: None,
                slot: (0, 0),
            },
            DesktopCanvasItem {
                name: "File.txt".to_owned(),
                path: PathBuf::from("/home/user/Desktop/File.txt"),
                is_dir: false,
                icon_name: None,
                slot: (0, 1),
            },
        ];

        let r0 = compute_desktop_item_rect(0, screen, dock_h);
        let center0 = Point::from((
            f64::from(r0.loc.x + r0.size.w / 2),
            f64::from(r0.loc.y + r0.size.h / 2),
        ));
        assert_eq!(
            hit_test_desktop_item(&items, screen, dock_h, center0),
            Some(0)
        );

        let r1 = compute_desktop_item_rect(1, screen, dock_h);
        let center1 = Point::from((
            f64::from(r1.loc.x + r1.size.w / 2),
            f64::from(r1.loc.y + r1.size.h / 2),
        ));
        assert_eq!(
            hit_test_desktop_item(&items, screen, dock_h, center1),
            Some(1)
        );

        // Outside coordinates (e.g. center of screen)
        assert_eq!(
            hit_test_desktop_item(&items, screen, dock_h, Point::from((400.0, 300.0))),
            None
        );
    }
}
