//! Toolbar dropdown and context-menu popup layout builders for `rusty-files`.

use rusty_services::file_manager::{FileEntry, FileSortField, FileSortOrder, FileTagColor};

use crate::state::{FilesAppState, OpenPopupMenu};

#[allow(clippy::wildcard_imports)]
use super::*;

fn measure_popup_height(entries: &[PopupRowSpec]) -> i32 {
    let row_h = rusty_ui::menu::ROW_HEIGHT;
    let sep_h = rusty_ui::menu::SEPARATOR_HEIGHT;
    let mut total_h = rusty_ui::menu::VERTICAL_CONTENT_MARGIN * 2;
    for (_, _, _, cmd) in entries {
        total_h += if cmd.is_some() { row_h } else { sep_h };
    }
    total_h
}

fn build_popup_items(
    x: i32,
    y: i32,
    menu_w: i32,
    total_h: i32,
    entries: Vec<PopupRowSpec>,
) -> PopupLayout {
    let row_h = rusty_ui::menu::ROW_HEIGHT;
    let sep_h = rusty_ui::menu::SEPARATOR_HEIGHT;
    let rect = Rect::new(x, y, menu_w, total_h);
    let mut items = Vec::with_capacity(entries.len());
    let mut cy = y + rusty_ui::menu::VERTICAL_CONTENT_MARGIN;
    for (label, checked, swatch_rgb, command) in entries {
        let h = if command.is_some() { row_h } else { sep_h };
        items.push(PopupItemLayout {
            rect: Rect::new(x, cy, menu_w, h),
            label,
            checked,
            swatch_rgb,
            command,
        });
        cy += h;
    }
    PopupLayout { rect, items }
}

pub(super) fn compute_popup_layout(
    state: &FilesAppState,
    layout: &FilesLayout,
    popup: &OpenPopupMenu,
) -> PopupLayout {
    let (anchor_x, anchor_y, menu_w, entries) = build_popup_entries(state, layout, popup);
    let total_h = measure_popup_height(&entries);
    let x = anchor_x.clamp(8, (layout.width - menu_w - 8).max(8));
    let y = anchor_y.clamp(8, (layout.height - total_h - 8).max(8));
    build_popup_items(x, y, menu_w, total_h, entries)
}

/// Computes an unconstrained `xdg_popup` surface spec so menus can extend outside the parent window.
pub(crate) fn compute_popup_surface_spec(
    state: &FilesAppState,
    layout: &FilesLayout,
    popup: &OpenPopupMenu,
    shadow_pad: i32,
) -> PopupSurfaceSpec {
    let (anchor_x, anchor_y, menu_w, entries) = build_popup_entries(state, layout, popup);
    let total_h = measure_popup_height(&entries);
    let pad = shadow_pad.max(0);
    let popup_layout = build_popup_items(pad, pad, menu_w, total_h, entries);
    PopupSurfaceSpec {
        anchor_x,
        anchor_y,
        surface_width: menu_w + pad * 2,
        surface_height: total_h + pad * 2,
        shadow_pad: pad,
        layout: popup_layout,
    }
}

type PopupRowSpec = (String, bool, Option<[u8; 3]>, Option<PopupCommand>);

fn build_popup_entries(
    state: &FilesAppState,
    layout: &FilesLayout,
    popup: &OpenPopupMenu,
) -> (i32, i32, i32, Vec<PopupRowSpec>) {
    match popup {
        OpenPopupMenu::SortMenu => (
            layout.sort_pill_rect.x,
            layout.sort_pill_rect.bottom() + 6,
            176,
            sort_menu_rows(state),
        ),
        OpenPopupMenu::ShareMenu => (
            layout.action_primary_rect.x - 16,
            layout.action_primary_rect.bottom() + 6,
            196,
            share_menu_rows(state),
        ),
        OpenPopupMenu::ActionsMenu => (
            layout.action_pill_rect.right() - 210,
            layout.action_pill_rect.bottom() + 6,
            210,
            actions_menu_rows(state),
        ),
        OpenPopupMenu::TagPicker { target_path } => (
            layout.action_tag_rect.x - 40,
            layout.action_tag_rect.bottom() + 6,
            164,
            tag_menu_rows(state, target_path),
        ),
        OpenPopupMenu::ContextMenu {
            x, y, target_entry, ..
        } => (*x, *y, 200, context_menu_rows(state, target_entry.as_ref())),
    }
}

fn share_menu_rows(state: &FilesAppState) -> Vec<PopupRowSpec> {
    let mut rows = Vec::new();
    if state.is_trash_view()
        && let Some(entry) = state.selected_entry()
    {
        rows.push((
            String::from("Put Back"),
            false,
            None,
            Some(PopupCommand::RestoreEntry(entry.clone())),
        ));
        rows.push((String::new(), false, None, None));
    }
    if let Some(path) = state.share_target_path() {
        rows.push((
            String::from("Copy Path"),
            false,
            None,
            Some(PopupCommand::ShareCopyPath(path.clone())),
        ));
        rows.push((
            String::from("Copy Link (file://)"),
            false,
            None,
            Some(PopupCommand::ShareCopyUri(path)),
        ));
        if state.selected_entry().is_some() {
            rows.push((
                String::from("Copy Item"),
                false,
                None,
                Some(PopupCommand::CopySelected),
            ));
        }
        rows.push((String::new(), false, None, None));
    }
    rows.push((
        String::from("New Folder"),
        false,
        None,
        Some(PopupCommand::NewFolder),
    ));
    rows
}

fn sort_menu_rows(state: &FilesAppState) -> Vec<PopupRowSpec> {
    let fields = [
        FileSortField::Name,
        FileSortField::DateModified,
        FileSortField::Size,
        FileSortField::Kind,
    ];
    let mut rows: Vec<PopupRowSpec> = fields
        .iter()
        .map(|f| {
            (
                String::from(f.label()),
                state.sort_field == *f,
                None,
                Some(PopupCommand::SortBy(*f)),
            )
        })
        .collect();
    rows.push((String::new(), false, None, None));
    rows.push((
        String::from("Ascending"),
        state.sort_order == FileSortOrder::Ascending,
        None,
        Some(PopupCommand::SetSortOrder(FileSortOrder::Ascending)),
    ));
    rows.push((
        String::from("Descending"),
        state.sort_order == FileSortOrder::Descending,
        None,
        Some(PopupCommand::SetSortOrder(FileSortOrder::Descending)),
    ));
    rows
}

fn actions_menu_rows(state: &FilesAppState) -> Vec<PopupRowSpec> {
    let mut rows = vec![
        (
            String::from("New Folder"),
            false,
            None,
            Some(PopupCommand::NewFolder),
        ),
        (
            String::from("New Tab"),
            false,
            None,
            Some(PopupCommand::NewTab),
        ),
        (
            String::from("Go to Folder..."),
            false,
            None,
            Some(PopupCommand::GoToFolder),
        ),
        (
            String::from("Show Tab Bar"),
            state.tab_bar_visible(),
            None,
            Some(PopupCommand::ToggleTabBar),
        ),
        (
            String::from("Show Path Bar"),
            state.show_path_bar,
            None,
            Some(PopupCommand::TogglePathBar),
        ),
    ];
    append_selection_action_rows(state, &mut rows);
    if !state.is_trash_view() && state.clipboard.is_some() {
        rows.push((
            String::from("Paste Item"),
            false,
            None,
            Some(PopupCommand::PasteIntoActive),
        ));
    }
    rows.push((
        String::from("Empty Trash"),
        false,
        None,
        Some(PopupCommand::EmptyTrash),
    ));
    rows.push((String::new(), false, None, None));
    rows.push((
        String::from("Show Hidden Files"),
        state.show_hidden,
        None,
        Some(PopupCommand::ToggleShowHidden),
    ));
    let theme_label = match state.theme {
        rusty_services::file_manager::FilesThemeMode::Light => "Dark Theme",
        rusty_services::file_manager::FilesThemeMode::Dark => "Light Theme",
    };
    rows.push((
        String::from(theme_label),
        false,
        None,
        Some(PopupCommand::ToggleTheme),
    ));
    rows
}

fn append_selection_action_rows(state: &FilesAppState, rows: &mut Vec<PopupRowSpec>) {
    let Some(entry) = state.selected_entry() else {
        return;
    };
    rows.push((String::new(), false, None, None));
    rows.push((
        String::from("Quick Look"),
        false,
        None,
        Some(PopupCommand::QuickLook),
    ));
    rows.push((
        String::from("Get Info"),
        false,
        None,
        Some(PopupCommand::GetInfoSelected),
    ));
    if state.is_trash_view() {
        rows.push((
            String::from("Put Back"),
            false,
            None,
            Some(PopupCommand::RestoreEntry(entry.clone())),
        ));
    } else {
        rows.push((
            String::from("Rename..."),
            false,
            None,
            Some(PopupCommand::RenameSelected),
        ));
        rows.push((
            String::from("Copy"),
            false,
            None,
            Some(PopupCommand::CopySelected),
        ));
        rows.push((
            String::from("Cut"),
            false,
            None,
            Some(PopupCommand::CutSelected),
        ));
        rows.push((
            String::from("Duplicate"),
            false,
            None,
            Some(PopupCommand::DuplicateSelected),
        ));
        rows.push((
            String::from("Move to Trash"),
            false,
            None,
            Some(PopupCommand::MoveToTrash(entry.clone())),
        ));
    }
}

fn tag_menu_rows(state: &FilesAppState, target_path: &std::path::Path) -> Vec<PopupRowSpec> {
    let assigned = state.tags_for(target_path);
    FileTagColor::ALL
        .iter()
        .map(|c| {
            (
                String::from(c.label()),
                assigned.contains(c),
                Some(c.rgb()),
                Some(PopupCommand::ToggleTag {
                    path: target_path.to_path_buf(),
                    color: *c,
                }),
            )
        })
        .collect()
}

fn context_menu_rows(state: &FilesAppState, entry: Option<&FileEntry>) -> Vec<PopupRowSpec> {
    let mut rows = Vec::new();
    if let Some(item) = entry {
        append_context_item_rows(state, item, &mut rows);
    } else {
        rows.push((
            String::from("New Folder"),
            false,
            None,
            Some(PopupCommand::NewFolder),
        ));
        rows.push((
            String::from("New Tab"),
            false,
            None,
            Some(PopupCommand::NewTab),
        ));
        if let Some(dir) = state.active_directory_path() {
            rows.push((
                String::from("Open in Terminal"),
                false,
                None,
                Some(PopupCommand::OpenInTerminal(dir)),
            ));
        }
        if !state.is_trash_view() && state.clipboard.is_some() {
            rows.push((
                String::from("Paste Item"),
                false,
                None,
                Some(PopupCommand::PasteIntoActive),
            ));
        }
        rows.push((
            String::from("Get Info"),
            false,
            None,
            Some(PopupCommand::GetInfoSelected),
        ));
        rows.push((
            String::from("Show Path Bar"),
            state.show_path_bar,
            None,
            Some(PopupCommand::TogglePathBar),
        ));
        rows.push((
            String::from("Show Hidden Files"),
            state.show_hidden,
            None,
            Some(PopupCommand::ToggleShowHidden),
        ));
    }
    if state.is_trash_view() {
        rows.push((String::new(), false, None, None));
        rows.push((
            String::from("Empty Trash"),
            false,
            None,
            Some(PopupCommand::EmptyTrash),
        ));
    }
    rows
}

fn append_primary_open_rows(state: &FilesAppState, item: &FileEntry, rows: &mut Vec<PopupRowSpec>) {
    let exec_state = if item.is_navigable_dir() || state.is_trash_view() {
        crate::state::ExecutableFileState::NotExecutable
    } else {
        crate::state::classify_executable_file(&item.path)
    };
    match exec_state {
        crate::state::ExecutableFileState::Runnable => {
            rows.push((
                String::from("Run as Program"),
                false,
                None,
                Some(PopupCommand::RunExecutable(item.path.clone())),
            ));
            rows.push((
                String::from("Run in Terminal"),
                false,
                None,
                Some(PopupCommand::RunInTerminal(item.path.clone())),
            ));
        }
        crate::state::ExecutableFileState::BinaryMissingExecBit => {
            rows.push((
                String::from("Allow Executing & Run"),
                false,
                None,
                Some(PopupCommand::RunExecutable(item.path.clone())),
            ));
            rows.push((
                String::from("Run in Terminal"),
                false,
                None,
                Some(PopupCommand::RunInTerminal(item.path.clone())),
            ));
        }
        crate::state::ExecutableFileState::ScriptMissingExecBit => {
            rows.push((
                String::from("Open"),
                false,
                None,
                Some(PopupCommand::OpenEntry(item.clone())),
            ));
            rows.push((
                String::from("Allow Executing & Run"),
                false,
                None,
                Some(PopupCommand::RunExecutable(item.path.clone())),
            ));
            rows.push((
                String::from("Run in Terminal"),
                false,
                None,
                Some(PopupCommand::RunInTerminal(item.path.clone())),
            ));
        }
        crate::state::ExecutableFileState::NotExecutable => {
            rows.push((
                String::from("Open"),
                false,
                None,
                Some(PopupCommand::OpenEntry(item.clone())),
            ));
        }
    }
}

fn append_context_item_rows(state: &FilesAppState, item: &FileEntry, rows: &mut Vec<PopupRowSpec>) {
    append_primary_open_rows(state, item, rows);
    if item.is_navigable_dir() {
        rows.push((
            String::from("Open in New Tab"),
            false,
            None,
            Some(PopupCommand::NewTabIn(item.path.clone())),
        ));
        rows.push((
            String::from("Open in Terminal"),
            false,
            None,
            Some(PopupCommand::OpenInTerminal(item.path.clone())),
        ));
    }
    rows.push((
        String::from("Quick Look"),
        false,
        None,
        Some(PopupCommand::QuickLook),
    ));
    rows.push((
        String::from("Get Info"),
        false,
        None,
        Some(PopupCommand::GetInfoPath(item.path.clone())),
    ));
    if state.is_trash_view() {
        rows.push((
            String::from("Put Back"),
            false,
            None,
            Some(PopupCommand::RestoreEntry(item.clone())),
        ));
    } else {
        if crate::state::is_extractable_archive(&item.path) {
            rows.push((
                String::from("Extract Here"),
                false,
                None,
                Some(PopupCommand::ExtractArchive(item.path.clone())),
            ));
        }
        rows.push((
            String::from("Compress to .zip"),
            false,
            None,
            Some(PopupCommand::CompressToZip(item.path.clone())),
        ));
        rows.push((String::new(), false, None, None));
        rows.push((
            String::from("Rename..."),
            false,
            None,
            Some(PopupCommand::RenameSelected),
        ));
        rows.push((
            String::from("Copy"),
            false,
            None,
            Some(PopupCommand::CopySelected),
        ));
        rows.push((
            String::from("Cut"),
            false,
            None,
            Some(PopupCommand::CutSelected),
        ));
        rows.push((
            String::from("Duplicate"),
            false,
            None,
            Some(PopupCommand::DuplicateSelected),
        ));
        rows.push((
            String::from("Move to Trash"),
            false,
            None,
            Some(PopupCommand::MoveToTrash(item.clone())),
        ));
        rows.push((String::new(), false, None, None));
        rows.extend(tag_menu_rows(state, &item.path));
    }
}
