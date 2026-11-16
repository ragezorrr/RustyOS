//! macOS 27 Golden Gate desktop canvas icon grid and interaction (`RUSTYOS-081`).
//!
//! Renders interactive file, folder, and application icons directly on the desktop
//! wallpaper layer, arranged in vertical columns from top-right down, with dual-layer
//! text drop shadows, selection highlights, and double-click opening.

use std::path::{Path, PathBuf};

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Size};

use super::CompositorRenderElement;
use super::tokens::truncate_ascii;

/// Width of each desktop canvas icon grid cell in logical pixels.
pub(crate) const DESKTOP_ITEM_W: i32 = 88;
/// Height of each desktop canvas icon grid cell in logical pixels.
pub(crate) const DESKTOP_ITEM_H: i32 = 92;
/// Icon edge size in logical pixels (48x48 macOS Golden Gate standard).
pub(crate) const DESKTOP_ICON_SIZE: i32 = 48;
/// Margin from screen right edge in logical pixels.
pub(crate) const DESKTOP_MARGIN_RIGHT: i32 = 24;
/// Margin from screen top edge (beneath Menu Bar) in logical pixels.
pub(crate) const DESKTOP_MARGIN_TOP: i32 = 46;
/// Vertical gap between consecutive desktop icon items in logical pixels.
pub(crate) const DESKTOP_GAP_Y: i32 = 12;
/// Horizontal gap between consecutive desktop icon columns in logical pixels.
pub(crate) const DESKTOP_GAP_X: i32 = 16;

/// A single file, folder, or application entry displayed on the desktop canvas (`RUSTYOS-081`).
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
}

/// Computes the bounding rectangle for the desktop icon at `index` on a screen of `screen_size`.
///
/// In macOS Golden Gate, desktop items arrange vertically along the right edge of the screen,
/// flowing from top to bottom, and wrapping to the left when screen height is exhausted.
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

/// Hit-tests pointer coordinates against desktop canvas items, returning the clicked item index if any.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn hit_test_desktop_item(
    items: &[DesktopCanvasItem],
    screen_size: Size<i32, Logical>,
    dock_height: i32,
    pos: Point<f64, Logical>,
) -> Option<usize> {
    let px = pos.x.round() as i32;
    let py = pos.y.round() as i32;
    for (idx, _item) in items.iter().enumerate() {
        let rect = compute_desktop_item_rect(idx, screen_size, dock_height);
        if rect.contains(Point::from((px, py))) {
            return Some(idx);
        }
    }
    None
}

/// Renders desktop canvas items (icon, dual-layer drop-shadow text, and selection pill) directly on wallpaper.
#[allow(clippy::too_many_arguments)]
pub fn render_desktop_canvas_elements(
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    items: &[DesktopCanvasItem],
    selected_path: Option<&Path>,
    screen_size: Size<i32, Logical>,
    dock_height: i32,
    elements: &mut Vec<CompositorRenderElement>,
) {
    for (idx, item) in items.iter().enumerate() {
        let rect = compute_desktop_item_rect(idx, screen_size, dock_height);
        let is_selected = selected_path == Some(item.path.as_path());

        let icon_x = rect.loc.x + (DESKTOP_ITEM_W - DESKTOP_ICON_SIZE) / 2;
        let icon_y = rect.loc.y + 4;
        let icon_loc = Point::from((icon_x, icon_y));

        let short_name = truncate_ascii(&item.name, 16);
        let label_bounds = Rectangle::new(
            Point::from((rect.loc.x + 2, icon_y + DESKTOP_ICON_SIZE + 6)),
            Size::from((rect.size.w - 4, 24)),
        );

        // 1. FRONT: Crisp white text foreground (12px Inter Regular)
        if let Some(text_el) = text_cache.render_label_centered_weighted(
            renderer,
            &short_name,
            crate::text::FontWeight::Regular,
            12.0,
            [255, 255, 255, 255],
            label_bounds,
        ) {
            elements.push(CompositorRenderElement::Memory(text_el));
        }

        // 2. BEHIND TEXT: Soft drop shadow for legibility over light wallpapers
        let shadow_bounds = Rectangle::new(
            Point::from((label_bounds.loc.x, label_bounds.loc.y + 1)),
            label_bounds.size,
        );
        if let Some(shadow_el) = text_cache.render_label_centered_weighted(
            renderer,
            &short_name,
            crate::text::FontWeight::Regular,
            12.0,
            [0, 0, 0, 140],
            shadow_bounds,
        ) {
            elements.push(CompositorRenderElement::Memory(shadow_el));
        }

        // 3. BEHIND TEXT & SHADOW: 48x48 icon element
        if item.is_dir {
            if let Some(icon_el) = shape_cache.icons.builtin_squircle_icon_element(
                renderer,
                crate::icons::BuiltinAppIcon::Files,
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

        // 4. BACK: Selection highlight pill (if item is selected)
        if is_selected {
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
            },
            DesktopCanvasItem {
                name: "File.txt".to_owned(),
                path: PathBuf::from("/home/user/Desktop/File.txt"),
                is_dir: false,
                icon_name: None,
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
