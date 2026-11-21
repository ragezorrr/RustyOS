//! Window context menu builders for RustyOS (`RUSTYOS-086`).
//!
//! Provides unified RustyOS aligned window management menus:
//! tiling (left/right half), zoom/restore, multi-display hopping, workspace assignment,
//! minimize, and close / force quit.

use rusty_wm::WindowId;

use crate::outputs::OutputMap;
use crate::shell_render::{ContextMenuAction, ContextMenuEntry};
use crate::surfaces::UiGlyphKind;

/// Builds the full macOS-aligned window management context menu for `win_id` (`RUSTYOS-086`).
#[must_use]
pub fn build_window_context_entries(
    win_id: WindowId,
    wm: &rusty_wm::WindowManager,
    output_map: &OutputMap,
    alt_held: bool,
) -> Option<Vec<ContextMenuEntry>> {
    let win = wm.window(win_id)?;
    let cur_ws = win.workspace.0;
    let is_maximized = win.maximized;

    let zoom_label = if is_maximized { "Restore Size" } else { "Zoom" };

    let move_sub: Vec<ContextMenuEntry> = (1..=9_u8)
        .map(|ws| {
            ContextMenuEntry::item(
                format!("Desktop {ws}"),
                UiGlyphKind::OverviewSpaces,
                ContextMenuAction::WindowMoveToWorkspace(win_id, ws),
            )
            .with_checked(ws == cur_ws)
        })
        .collect();

    let quit_entry = if alt_held {
        ContextMenuEntry::item(
            "Force Quit",
            UiGlyphKind::Close,
            ContextMenuAction::WindowForceQuit(win_id),
        )
        .with_danger(true)
    } else {
        ContextMenuEntry::item(
            "Close",
            UiGlyphKind::Close,
            ContextMenuAction::WindowQuit(win_id),
        )
        .with_hint("⌘W")
    };

    let has_multiple_outputs = output_map.count() > 1;

    let mut entries = vec![
        ContextMenuEntry::item(
            "Tile Window to Left",
            UiGlyphKind::LayoutSplit,
            ContextMenuAction::WindowTileLeft(win_id),
        )
        .with_hint("⌥←"),
        ContextMenuEntry::item(
            "Tile Window to Right",
            UiGlyphKind::LayoutSplit,
            ContextMenuAction::WindowTileRight(win_id),
        )
        .with_hint("⌥→"),
        ContextMenuEntry::item(
            zoom_label,
            UiGlyphKind::WindowMax,
            ContextMenuAction::WindowToggleMaximize(win_id),
        )
        .with_hint("⌥F"),
        ContextMenuEntry::Separator,
        ContextMenuEntry::submenu_item("Move to Desktop", UiGlyphKind::OverviewSpaces, move_sub),
    ];

    if has_multiple_outputs {
        entries.push(
            ContextMenuEntry::item(
                "Move to Next Display",
                UiGlyphKind::ArrowNext,
                ContextMenuAction::WindowMoveToNextOutput(win_id),
            )
            .with_hint("⌥O"),
        );
    }

    entries.extend([
        ContextMenuEntry::Separator,
        ContextMenuEntry::item(
            "Minimize",
            UiGlyphKind::WindowMin,
            ContextMenuAction::WindowMinimize(win_id),
        )
        .with_hint("⌘M"),
        quit_entry,
    ]);

    Some(entries)
}

/// Builds minimized Dock window context menu items (`RUSTYOS-064`, `RUSTYOS-086`).
#[must_use]
pub fn build_minimized_window_context_entries(
    win_id: WindowId,
    wm: &rusty_wm::WindowManager,
    minimized: bool,
    alt_held: bool,
) -> Option<Vec<ContextMenuEntry>> {
    let win = wm.window(win_id)?;
    let focused = !minimized && wm.focused_window() == Some(win_id);
    let cur_ws = win.workspace.0;

    let focus_label = if minimized {
        "Restore Window"
    } else if focused {
        "Focused Window"
    } else {
        "Show Window"
    };

    let move_sub: Vec<ContextMenuEntry> = (1..=9_u8)
        .map(|ws| {
            ContextMenuEntry::item(
                format!("Desktop {ws}"),
                UiGlyphKind::OverviewSpaces,
                ContextMenuAction::WindowMoveToWorkspace(win_id, ws),
            )
            .with_checked(ws == cur_ws)
        })
        .collect();

    let quit_entry = if alt_held {
        ContextMenuEntry::item(
            "Force Quit",
            UiGlyphKind::Close,
            ContextMenuAction::WindowForceQuit(win_id),
        )
        .with_danger(true)
    } else {
        ContextMenuEntry::item(
            "Quit",
            UiGlyphKind::Close,
            ContextMenuAction::WindowQuit(win_id),
        )
    };

    Some(vec![
        ContextMenuEntry::item(
            focus_label,
            UiGlyphKind::WindowMax,
            ContextMenuAction::WindowFocusOrRestore(win_id),
        )
        .with_checked(focused),
        ContextMenuEntry::item(
            "Minimize",
            UiGlyphKind::WindowMin,
            ContextMenuAction::WindowMinimize(win_id),
        )
        .with_enabled(!minimized),
        ContextMenuEntry::Separator,
        ContextMenuEntry::submenu_item("Move to Desktop", UiGlyphKind::OverviewSpaces, move_sub),
        ContextMenuEntry::Separator,
        quit_entry,
    ])
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::outputs::{OutputEntry, OutputMap, create_test_output};
    use rusty_wm::{Rect, WindowManager};

    #[test]
    fn window_context_menu_contains_mac_standard_actions_and_toggles_alt() {
        let mut wm = WindowManager::new(Rect::new(0, 0, 1920, 1080));
        let win_id = wm.next_window_id();
        let _ = wm.map_window(win_id, Some((800, 600)));

        let out = create_test_output("DP-1", (1920, 1080).into(), 60_000);
        let output_map = OutputMap::new(OutputEntry::new(
            rusty_wm::OutputId(1),
            "DP-1",
            out,
            (0, 0).into(),
            (1920, 1080).into(),
            1.0,
            60_000,
            true,
        ));

        // Normal state (Alt not held)
        let entries =
            build_window_context_entries(win_id, &wm, &output_map, false).expect("entries");
        assert!(entries.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::WindowTileLeft(id),
                ..
            } if *id == win_id
        )));
        assert!(entries.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::WindowTileRight(id),
                ..
            } if *id == win_id
        )));
        assert!(entries.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::WindowToggleMaximize(id),
                label,
                ..
            } if *id == win_id && label == "Zoom"
        )));
        assert!(entries.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::WindowMinimize(id),
                ..
            } if *id == win_id
        )));
        assert!(entries.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::WindowQuit(id),
                label,
                danger: false,
                ..
            } if *id == win_id && label == "Close"
        )));

        // Alt held state: "Close" -> "Force Quit" with danger
        let entries_alt =
            build_window_context_entries(win_id, &wm, &output_map, true).expect("entries_alt");
        assert!(entries_alt.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::WindowForceQuit(id),
                label,
                danger: true,
                ..
            } if *id == win_id && label == "Force Quit"
        )));

        // Maximized state: "Zoom" -> "Restore Size"
        wm.focus_and_raise(win_id);
        wm.toggle_maximize_focused();
        let entries_max =
            build_window_context_entries(win_id, &wm, &output_map, false).expect("entries_max");
        assert!(entries_max.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::WindowToggleMaximize(id),
                label,
                ..
            } if *id == win_id && label == "Restore Size"
        )));
    }

    #[test]
    fn window_context_menu_multi_monitor_includes_move_to_next_display() {
        let mut wm = WindowManager::new(Rect::new(0, 0, 1920, 1080));
        let win_id = wm.next_window_id();
        let _ = wm.map_window(win_id, Some((800, 600)));

        let out1 = create_test_output("DP-1", (1920, 1080).into(), 60_000);
        let mut output_map = OutputMap::new(OutputEntry::new(
            rusty_wm::OutputId(1),
            "DP-1",
            out1,
            (0, 0).into(),
            (1920, 1080).into(),
            1.0,
            60_000,
            true,
        ));
        let out2 = create_test_output("HDMI-1", (1920, 1080).into(), 60_000);
        output_map.add(OutputEntry::new(
            rusty_wm::OutputId(2),
            "HDMI-1",
            out2,
            (1920, 0).into(),
            (1920, 1080).into(),
            1.0,
            60_000,
            false,
        ));

        let entries =
            build_window_context_entries(win_id, &wm, &output_map, false).expect("entries");
        assert!(entries.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::WindowMoveToNextOutput(id),
                label,
                ..
            } if *id == win_id && label == "Move to Next Display"
        )));
    }
}
