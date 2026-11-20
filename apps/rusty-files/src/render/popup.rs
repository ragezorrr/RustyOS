//! macOS 27 Golden Gate Liquid Glass popover and context menu rendering for `rusty-files`
//! (`RUSTYOS-087`, `RUSTYOS-107`, ADR-0043, ADR-0058).
//!
//! Provides 1:1 visual parity with compositor-managed popovers (`crates/rusty-compositor/src/shell_render/context_menu.rs`):
//! - Concentric corner radius `MENU_POPOVER_RADIUS = 13` with `HOVER_PILL_RADIUS = 7` and `5 px` padding;
//! - Multi-stage soft Gaussian drop shadow;
//! - Translucent Liquid Glass material with specular top rim highlight;
//! - Inset floating hover pills (`HOVER_PILL_RADIUS = 7`, `HOVER_PILL_INSET_H = 5`, `HOVER_PILL_INSET_V = 1`);
//! - Two-mode left alignment (`14 px` without checkmarks/swatches; `28 px` with checkmarks; `36 px` with tag swatches);
//! - Right-aligned keyboard shortcut column (`⌘O`, `⌘I`, `⌘⌫`, `^⌘F`, `⌘M`, `⌘W`);
//! - Hairline dividers with 12px horizontal inset;
//! - Danger color tint for destructive commands (`Empty Trash`, `Move to Trash`).

use rusty_services::file_manager::FilesThemeMode;

use super::{Canvas, Palette, palette_for_with_accent, rgba};
use crate::layout::{PopupCommand, PopupLayout, Rect};

/// Concentric outer corner radius matching system popovers (`14 px`).
pub(crate) const MENU_POPOVER_RADIUS: i32 = rusty_ui::menu::MENU_RADIUS;

/// Concentric corner radius for the floating hover pill (`9 px` inside `5 px` padding).
pub(crate) const HOVER_PILL_RADIUS: i32 = rusty_ui::menu::HOVER_PILL_RADIUS;

/// Horizontal inset for the floating hover pill (`5 px`).
pub(crate) const HOVER_PILL_INSET_H: i32 = rusty_ui::menu::CONTENT_MARGIN;

/// Vertical inset for the floating hover pill (`1 px`).
pub(crate) const HOVER_PILL_INSET_V: i32 = 1;

/// Horizontal inset for hairline section dividers (`12 px`).
pub(crate) const HAIRLINE_DIVIDER_INSET: i32 = 12;

/// Maps a `PopupCommand` to its standard macOS 27 keyboard shortcut hint.
#[must_use]
pub(crate) fn shortcut_for_command(cmd: &PopupCommand) -> Option<&'static str> {
    match cmd {
        PopupCommand::NewFolder => Some("⇧⌘N"),
        PopupCommand::NewTab | PopupCommand::NewTabIn(_) => Some("⌘T"),
        PopupCommand::QuickLook => Some("Space"),
        PopupCommand::GetInfoSelected | PopupCommand::GetInfoPath(_) => Some("⌘I"),
        PopupCommand::RenameSelected => Some("⏎"),
        PopupCommand::CopySelected => Some("⌘C"),
        PopupCommand::CutSelected => Some("⌘X"),
        PopupCommand::PasteIntoActive => Some("⌘V"),
        PopupCommand::DuplicateSelected => Some("⌘D"),
        PopupCommand::MoveToTrash(_) => Some("⌘⌫"),
        PopupCommand::ZoomWindow => Some("^⌘F"),
        PopupCommand::MinimizeWindow => Some("⌘M"),
        PopupCommand::CloseWindow => Some("⌘W"),
        PopupCommand::ToggleShowHidden => Some("⇧⌘."),
        PopupCommand::SortBy(_)
        | PopupCommand::SetSortOrder(_)
        | PopupCommand::ToggleTabBar
        | PopupCommand::TogglePathBar
        | PopupCommand::ShareCopyPath(_)
        | PopupCommand::ShareCopyUri(_)
        | PopupCommand::GoToFolder
        | PopupCommand::ToggleTheme
        | PopupCommand::OpenEntry(_)
        | PopupCommand::RestoreEntry(_)
        | PopupCommand::EmptyTrash
        | PopupCommand::ToggleTag { .. } => None,
    }
}

/// Identifies destructive commands that render in `popover_danger`.
#[must_use]
pub(crate) const fn is_danger_command(cmd: &PopupCommand) -> bool {
    matches!(cmd, PopupCommand::EmptyTrash | PopupCommand::MoveToTrash(_))
}

/// Renders a standalone native `xdg_popup` surface buffer (`ARGB8888`) with optional
/// compositor Liquid Glass backing (`has_compositor_glass == true`) or CPU fallback chrome.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_popup_surface_pixels(
    buf: &mut [u8],
    width: u32,
    height: u32,
    popup: &PopupLayout,
    hover_idx: Option<usize>,
    theme: FilesThemeMode,
) {
    render_popup_surface_pixels_with_glass(buf, width, height, popup, hover_idx, theme, false);
}

/// Renders a standalone native `xdg_popup` surface buffer (`ARGB8888`); when
/// `has_compositor_glass == true`, skips CPU shadow/opaque fill so the compositor
/// `LiquidGlassElement` (`GlassPanelStyle::Popover`, `r = 13`) provides live blur and rim.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn render_popup_surface_pixels_with_glass(
    buf: &mut [u8],
    width: u32,
    height: u32,
    popup: &PopupLayout,
    hover_idx: Option<usize>,
    theme: FilesThemeMode,
    has_compositor_glass: bool,
) {
    render_popup_surface_pixels_with_style(
        buf,
        width,
        height,
        popup,
        hover_idx,
        theme,
        0,
        has_compositor_glass,
    );
}

/// Renders a standalone native `xdg_popup` surface buffer (`ARGB8888`) with `[ui].accent_index`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_popup_surface_pixels_with_style(
    buf: &mut [u8],
    width: u32,
    height: u32,
    popup: &PopupLayout,
    hover_idx: Option<usize>,
    theme: FilesThemeMode,
    accent_index: usize,
    has_compositor_glass: bool,
) {
    let pal = palette_for_with_accent(theme, accent_index);
    let mut c = Canvas::new(buf, width, height);
    c.clear();
    render_popup_with_chrome(&mut c, popup, hover_idx, &pal, !has_compositor_glass);
}

/// Draws the soft shadow, translucent glass body, directional hairline rim, and outer shade border.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
fn render_popup_chrome(c: &mut Canvas<'_>, r: Rect, pal: &Palette) {
    let shadow_rect = rusty_ui::Rect::new(r.x, r.y, r.w.max(0) as u32, r.h.max(0) as u32);
    rusty_ui::draw_elevation_shadow_with(
        shadow_rect,
        u32::try_from(MENU_POPOVER_RADIUS).unwrap_or(14),
        rusty_ui::ShadowElevation::MenuOrTooltip,
        pal.is_dark,
        |sr, rad, col| {
            c.fill_rounded_rect(
                Rect::new(sr.x, sr.y, sr.width as i32, sr.height as i32),
                rad,
                col,
            );
        },
    );

    c.fill_rounded_rect(r, MENU_POPOVER_RADIUS, pal.popover_bg);

    // System directional inner 1px hairline (top 100%, bottom 35%, no side glare)
    let bot_rim_alpha = (u16::from(pal.popover_rim[3]) * 35 / 100) as u8;
    let bot_rim = rgba(
        pal.popover_rim[0],
        pal.popover_rim[1],
        pal.popover_rim[2],
        bot_rim_alpha,
    );
    c.stroke_line_aa(
        r.x as f32 + 13.0,
        r.y as f32 + 1.2,
        (r.x + r.w) as f32 - 13.0,
        r.y as f32 + 1.2,
        1.0,
        pal.popover_rim,
    );
    c.stroke_line_aa(
        r.x as f32 + 13.0,
        (r.y + r.h) as f32 - 1.2,
        (r.x + r.w) as f32 - 13.0,
        (r.y + r.h) as f32 - 1.2,
        1.0,
        bot_rim,
    );

    c.stroke_rounded_rect(r, MENU_POPOVER_RADIUS, pal.popover_border);
}

/// Renders an inline/CPU Liquid Glass popup/context menu with full CPU chrome fallback.
pub(crate) fn render_popup(
    c: &mut Canvas<'_>,
    popup: &PopupLayout,
    hover_idx: Option<usize>,
    pal: &Palette,
) {
    render_popup_with_chrome(c, popup, hover_idx, pal, true);
}

/// Renders a Liquid Glass popup/context menu.
#[allow(clippy::cast_precision_loss)]
fn render_popup_with_chrome(
    c: &mut Canvas<'_>,
    popup: &PopupLayout,
    hover_idx: Option<usize>,
    pal: &Palette,
    draw_chrome: bool,
) {
    if draw_chrome {
        render_popup_chrome(c, popup.rect, pal);
    }
    let any_non_swatch_checked = popup
        .items
        .iter()
        .any(|i| i.checked && i.swatch_rgb.is_none());

    // Render individual rows
    for (idx, item) in popup.items.iter().enumerate() {
        let Some(cmd) = &item.command else {
            // Hairline section divider
            let sep_y = item.rect.y + item.rect.h / 2;
            c.fill_rect(
                Rect::new(
                    item.rect.x + HAIRLINE_DIVIDER_INSET,
                    sep_y,
                    item.rect.w - HAIRLINE_DIVIDER_INSET * 2,
                    1,
                ),
                pal.popover_separator,
            );
            continue;
        };

        let hovered = hover_idx == Some(idx);
        if hovered {
            // Inset floating hover pill (macOS 27 Golden Gate 1:1)
            let pill = Rect::new(
                item.rect.x + HOVER_PILL_INSET_H,
                item.rect.y + HOVER_PILL_INSET_V,
                item.rect.w - HOVER_PILL_INSET_H * 2,
                item.rect.h - HOVER_PILL_INSET_V * 2,
            );
            c.fill_rounded_rect(pill, HOVER_PILL_RADIUS, pal.accent);
        }

        // Checkmark indicator: trailing on tag rows, leading on checked menus (e.g. SortMenu)
        if item.checked {
            let cx = if item.swatch_rgb.is_some() {
                (item.rect.right() - 18) as f32
            } else {
                item.rect.x as f32 + 14.0
            };
            let cy = item.rect.y as f32 + (item.rect.h as f32 / 2.0);
            c.stroke_polyline_aa(
                &[
                    (cx - 3.2, cy - 0.2),
                    (cx - 0.7, cy + 2.6),
                    (cx + 4.0, cy - 2.8),
                ],
                false,
                1.45,
                if hovered { pal.accent_text } else { pal.accent },
            );
        }

        // Two-mode left alignment:
        // - Tag swatches: 5px circle centered at x + 19 (left edge x + 14), text at x + 32
        // - Non-swatch checked menus (Sort/View): check at x + 14, text at x + 28
        // - Plain rows (Open..Move to Trash): text directly at x + 14 flush with tag circles
        let text_x = if let Some([sr, sg, sb]) = item.swatch_rgb {
            let cx = item.rect.x as f32 + 19.0;
            let cy = item.rect.y as f32 + (item.rect.h as f32 / 2.0);
            c.fill_circle_f(cx, cy, 4.8, rgba(sr, sg, sb, 255));
            c.stroke_circle_f(cx, cy, 4.8, 1.0, rgba(0, 0, 0, 30));
            item.rect.x + 32
        } else if any_non_swatch_checked {
            item.rect.x + 28
        } else {
            item.rect.x + 14
        };

        // Right-aligned keyboard shortcut
        let shortcut = shortcut_for_command(cmd);
        let shortcut_w = shortcut.map_or(0, |sc| Canvas::measure_text(sc, 12.0, false));

        // Primary row label text
        let max_label_w = (item.rect.right() - text_x - shortcut_w - 18).max(30);
        let label = Canvas::fit_text(&item.label, max_label_w, 13.0, false);
        let label_col = if hovered {
            pal.accent_text
        } else if is_danger_command(cmd) {
            pal.popover_danger
        } else {
            pal.popover_text_primary
        };
        let text_y = item.rect.y + ((item.rect.h - 14) / 2);
        c.draw_text(&label, text_x, text_y, 13.0, false, label_col, item.rect);

        // Draw keyboard shortcut badge if present
        if let Some(sc) = shortcut {
            let sc_col = if hovered {
                pal.accent_text
            } else {
                pal.popover_text_secondary
            };
            let sc_x = item.rect.right() - shortcut_w - 14;
            c.draw_text(sc, sc_x, text_y + 1, 12.0, false, sc_col, item.rect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::PopupItemLayout;
    use rusty_services::file_manager::FileEntry;
    use std::path::PathBuf;

    #[test]
    fn test_shortcuts_and_danger_classification() {
        assert_eq!(shortcut_for_command(&PopupCommand::NewFolder), Some("⇧⌘N"));
        assert_eq!(shortcut_for_command(&PopupCommand::NewTab), Some("⌘T"));
        assert_eq!(
            shortcut_for_command(&PopupCommand::QuickLook),
            Some("Space")
        );
        assert_eq!(
            shortcut_for_command(&PopupCommand::GetInfoSelected),
            Some("⌘I")
        );
        assert_eq!(
            shortcut_for_command(&PopupCommand::RenameSelected),
            Some("⏎")
        );
        assert_eq!(
            shortcut_for_command(&PopupCommand::CopySelected),
            Some("⌘C")
        );
        assert_eq!(shortcut_for_command(&PopupCommand::CutSelected), Some("⌘X"));
        assert_eq!(
            shortcut_for_command(&PopupCommand::PasteIntoActive),
            Some("⌘V")
        );
        assert_eq!(
            shortcut_for_command(&PopupCommand::DuplicateSelected),
            Some("⌘D")
        );
        assert_eq!(shortcut_for_command(&PopupCommand::ZoomWindow), Some("^⌘F"));
        assert_eq!(
            shortcut_for_command(&PopupCommand::MinimizeWindow),
            Some("⌘M")
        );
        assert_eq!(shortcut_for_command(&PopupCommand::CloseWindow), Some("⌘W"));

        let dummy_entry = FileEntry {
            name: String::from("file.txt"),
            path: PathBuf::from("/tmp/file.txt"),
            kind: rusty_services::file_manager::FileEntryKind::RegularFile,
            size_bytes: 1024,
            modified_unix_secs: Some(0),
            hidden: false,
            symlink_target_is_dir: false,
            desktop_exec: None,
            trash_original_path: None,
            trash_deletion_date: None,
            subtitle: None,
        };
        assert_eq!(
            shortcut_for_command(&PopupCommand::MoveToTrash(dummy_entry.clone())),
            Some("⌘⌫")
        );

        assert!(is_danger_command(&PopupCommand::EmptyTrash));
        assert!(is_danger_command(&PopupCommand::MoveToTrash(dummy_entry)));
        assert!(!is_danger_command(&PopupCommand::NewFolder));
        assert!(!is_danger_command(&PopupCommand::NewTab));
    }

    #[test]
    fn test_liquid_glass_popup_rendering_tokens_and_geometry() {
        let items = vec![
            PopupItemLayout {
                rect: Rect::new(8, 13, 214, 24),
                label: String::from("New Folder"),
                checked: false,
                swatch_rgb: None,
                command: Some(PopupCommand::NewFolder),
            },
            PopupItemLayout {
                rect: Rect::new(8, 37, 214, 9),
                label: String::new(),
                checked: false,
                swatch_rgb: None,
                command: None,
            },
            PopupItemLayout {
                rect: Rect::new(8, 46, 214, 24),
                label: String::from("Move to Trash"),
                checked: false,
                swatch_rgb: None,
                command: Some(PopupCommand::MoveToTrash(FileEntry {
                    name: String::from("test.txt"),
                    path: PathBuf::from("/tmp/test.txt"),
                    kind: rusty_services::file_manager::FileEntryKind::RegularFile,
                    size_bytes: 42,
                    modified_unix_secs: Some(0),
                    hidden: false,
                    symlink_target_is_dir: false,
                    desktop_exec: None,
                    trash_original_path: None,
                    trash_deletion_date: None,
                    subtitle: None,
                })),
            },
        ];
        let layout = PopupLayout {
            rect: Rect::new(8, 8, 214, 67),
            items,
        };

        let pw = 230_u32;
        let ph = 83_u32;
        let mut buf = vec![0_u8; (pw * ph * 4) as usize];

        // Render with first item hovered in Light theme
        render_popup_surface_pixels(&mut buf, pw, ph, &layout, Some(0), FilesThemeMode::Light);

        let pal = palette_for_with_accent(FilesThemeMode::Light, 0);
        // Verify hover pill fill at row 0 mid-point inside left margin of pill (x = 8 + 10 = 18, y = 13 + 12 = 25)
        let pill_idx = ((25 * pw + 18) * 4) as usize;
        assert_eq!(buf[pill_idx + 2], pal.accent[0]); // R
        assert_eq!(buf[pill_idx + 1], pal.accent[1]); // G
        assert_eq!(buf[pill_idx], pal.accent[2]); // B

        // Verify popover background body inside unhovered bottom padding has high alpha (Liquid Glass)
        let body_idx = ((72 * pw + 30) * 4) as usize;
        assert!(buf[body_idx + 3] >= 235);
    }
}
