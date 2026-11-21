//! Crisp SVG-backed sidebar/file/folder icons and pixel-aligned toolbar glyphs
//! for `rusty-files` (`RUSTYOS-065`, `RUSTYOS-066`, `RUSTYOS-149`).

use rusty_services::file_manager::{FileEntry, FileEntryKind, FileTagColor, FileViewMode};

use crate::render::{Canvas, Palette, rgba};
use crate::state::SidebarIconKind;
use crate::svg_icons::{
    SidebarSvgAsset, ToolbarSvgAsset, draw_entry_svg, draw_sidebar_svg, draw_toolbar_svg,
    entry_icon_asset,
};

/// Snaps a stroke center coordinate to half-pixel center so 1-px lines stay crisp.
#[inline]
pub(crate) fn snap_half(coord: f32) -> f32 {
    coord.floor() + 0.5
}

/// Renders a crisp 16 px SVG-backed RustyOS sidebar icon centered at `(cx, cy)`.
pub(crate) fn draw_sidebar_icon(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    kind: SidebarIconKind,
    pal: &Palette,
) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    let accent = pal.accent;
    let neutral = pal.text_secondary;
    match kind {
        SidebarIconKind::Tag(tag) => {
            let [red, green, blue] = tag.rgb();
            c.fill_circle_f(sx, sy, 4.25, rgba(red, green, blue, 255));
        }
        SidebarIconKind::Recents => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Recents, accent);
        }
        SidebarIconKind::Shared => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Shared, accent);
        }
        SidebarIconKind::Applications => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Applications, accent);
        }
        SidebarIconKind::Desktop => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Desktop, accent);
        }
        SidebarIconKind::Documents => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Documents, accent);
        }
        SidebarIconKind::Downloads => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Downloads, accent);
        }
        SidebarIconKind::Home => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Home, neutral);
        }
        SidebarIconKind::Volume => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Drive, neutral);
        }
        SidebarIconKind::RemovableDrive => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Usb, accent);
        }
        SidebarIconKind::Network => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Network, neutral);
        }
        SidebarIconKind::Trash => {
            draw_sidebar_svg(c, cx, cy, 16, SidebarSvgAsset::Trash, neutral);
        }
    }
}

/// Renders a crisp 12 px eject glyph (`⏏`) for removable storage drives in the sidebar.
pub(crate) fn draw_eject_icon(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    draw_sidebar_svg(c, cx, cy, 12, SidebarSvgAsset::Eject, col);
}

/// Draws compact 16 px SVG file/folder entry icons in Columns/List/Gallery header.
pub(crate) fn draw_entry_icon(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    entry: &FileEntry,
    _selected_blue: bool,
    _pal: &Palette,
) {
    let asset = entry_icon_asset(entry);
    draw_entry_svg(c, cx, cy, 17, asset);
}

fn folder_emblem_asset(name: &str) -> Option<SidebarSvgAsset> {
    match name.to_ascii_lowercase().as_str() {
        "desktop" | "рабочий стол" => Some(SidebarSvgAsset::Desktop),
        "documents" | "документы" => Some(SidebarSvgAsset::Documents),
        "downloads" | "загрузки" => Some(SidebarSvgAsset::Downloads),
        "pictures" | "изображения" => Some(SidebarSvgAsset::Pictures),
        "music" | "музыка" => Some(SidebarSvgAsset::Music),
        "movies" | "videos" | "видео" => Some(SidebarSvgAsset::Videos),
        "public" | "shared" | "общедоступные" => Some(SidebarSvgAsset::Shared),
        "applications" | "apps" => Some(SidebarSvgAsset::Applications),
        _ => None,
    }
}

/// Draws large crisp `@2x` SVG folder or typed file artwork centered at `(cx, cy)`.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn draw_large_entry_artwork(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    size: f32,
    entry: &FileEntry,
    primary_tag: Option<FileTagColor>,
    _pal: &Palette,
) {
    let dim = size.round().clamp(16.0, 144.0) as i32;
    let asset = entry_icon_asset(entry);
    draw_entry_svg(c, cx, cy, dim, asset);

    if matches!(
        entry.kind,
        FileEntryKind::Directory | FileEntryKind::MountPoint
    ) {
        if let Some(emblem) = folder_emblem_asset(&entry.name) {
            let emblem_sz = (size * 0.34).round().clamp(12.0, 44.0) as i32;
            // Subtle embossed white emblem centered on the front face of the folder.
            draw_sidebar_svg(
                c,
                cx,
                cy + size * 0.08,
                emblem_sz,
                emblem,
                rgba(255, 255, 255, 195),
            );
        }
        if let Some(tag) = primary_tag {
            let [r, g, b] = tag.rgb();
            let dot_r = (size * 0.085).clamp(3.0, 7.0);
            let dot_x = cx + size * 0.28;
            let dot_y = cy + size * 0.22;
            c.fill_circle_f(dot_x, dot_y, dot_r + 1.2, rgba(255, 255, 255, 230));
            c.fill_circle_f(dot_x, dot_y, dot_r, rgba(r, g, b, 255));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_chevron_aa(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    left: bool,
    half_w: f32,
    half_h: f32,
    thickness: f32,
    col: [u8; 4],
) {
    let dir = if left { 1.0 } else { -1.0 };
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    let pts = [
        (sx + dir * half_w * 0.55, sy - half_h),
        (sx - dir * half_w * 0.55, sy),
        (sx + dir * half_w * 0.55, sy + half_h),
    ];
    c.stroke_polyline_aa(&pts, false, thickness, col);
}

/// Renders crisp `@2x` SVG navigation chevron (`<` or `>`) in the toolbar pill.
pub(crate) fn draw_nav_chevron_svg(c: &mut Canvas<'_>, cx: f32, cy: f32, back: bool, col: [u8; 4]) {
    let asset = if back {
        ToolbarSvgAsset::Back
    } else {
        ToolbarSvgAsset::Forward
    };
    draw_toolbar_svg(c, cx, cy, 16, asset, col);
}

/// Renders crisp `@2x` SVG disclosure chevron for collapsible sidebar section headers.
pub(crate) fn draw_section_chevron_svg(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    collapsed: bool,
    col: [u8; 4],
) {
    let asset = if collapsed {
        ToolbarSvgAsset::ChevronRight
    } else {
        ToolbarSvgAsset::ChevronDown
    };
    draw_toolbar_svg(c, cx, cy, 11, asset, col);
}

/// Renders crisp `@2x` SVG view-mode icons in the toolbar pill.
pub(crate) fn draw_view_mode_icon(
    c: &mut Canvas<'_>,
    cx: f32,
    cy: f32,
    mode: FileViewMode,
    col: [u8; 4],
) {
    let asset = match mode {
        FileViewMode::Icons => ToolbarSvgAsset::ViewIcons,
        FileViewMode::List => ToolbarSvgAsset::ViewList,
        FileViewMode::Columns => ToolbarSvgAsset::ViewColumns,
        FileViewMode::Gallery => ToolbarSvgAsset::ViewGallery,
    };
    draw_toolbar_svg(c, cx, cy, 16, asset, col);
}

pub(crate) fn draw_sort_pill_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    draw_toolbar_svg(c, cx, cy, 16, ToolbarSvgAsset::Sort, col);
}

/// Draws the RustyOS `Share` tray + upward arrow toolbar glyph (`RUSTYOS-071`, `RUSTYOS-150`).
pub(crate) fn draw_share_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    draw_toolbar_svg(c, cx, cy, 16, ToolbarSvgAsset::Share, col);
}

pub(crate) fn draw_tag_outline_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    draw_toolbar_svg(c, cx, cy, 16, ToolbarSvgAsset::Tag, col);
}

pub(crate) fn draw_ellipsis_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    draw_toolbar_svg(c, cx, cy, 16, ToolbarSvgAsset::More, col);
}

pub(crate) fn draw_search_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, col: [u8; 4]) {
    draw_toolbar_svg(c, cx, cy, 15, ToolbarSvgAsset::Search, col);
}

pub(crate) fn draw_plus_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, half: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    c.stroke_line_aa(sx - half, sy, sx + half, sy, 1.35, col);
    c.stroke_line_aa(sx, sy - half, sx, sy + half, 1.35, col);
}

pub(crate) fn draw_close_x_glyph(c: &mut Canvas<'_>, cx: f32, cy: f32, half: f32, col: [u8; 4]) {
    let sx = snap_half(cx);
    let sy = snap_half(cy);
    c.stroke_line_aa(sx - half, sy - half, sx + half, sy + half, 1.35, col);
    c.stroke_line_aa(sx - half, sy + half, sx + half, sy - half, 1.35, col);
}
