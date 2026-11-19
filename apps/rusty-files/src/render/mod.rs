//! Pure CPU premultiplied `ARGB8888` renderer for `rusty-files` (`RUSTYOS-065`, ADR-0030, ADR-0045).
//!
//! Submodules organize distinct visual layers:
//! - `canvas`: 2D rasterization, Fontdue Inter font cache, SDF antialiased primitives and shadows;
//! - `chrome`: window CSD traffic lights, toolbar, search scope bar, tab strip, path bar;
//! - `sidebar`: translucent Golden Gate sidebar and warning banners;
//! - `views`: Miller Columns, List View, Icon Grid, Gallery View, preview inspector, status bar;
//! - `dialogs`: modal overlays (Quick Look, Get Info, Go to Folder, Rename, Transfer Conflict);
//! - `popup`: 1:1 macOS 27 Liquid Glass popovers and context menus (`RUSTYOS-087`).

pub(crate) mod canvas;
pub(crate) mod chrome;
pub(crate) mod dialogs;
pub(crate) mod popup;
pub(crate) mod sidebar;
pub(crate) mod views;

pub(crate) use canvas::*;
pub(crate) use chrome::*;
pub(crate) use dialogs::*;
pub(crate) use popup::*;
pub(crate) use sidebar::*;
pub(crate) use views::*;

use rusty_services::file_manager::{FileViewMode, FilesThemeMode};

use crate::layout::{FilesLayout, Rect, WINDOW_CORNER_RADIUS};
use crate::state::FilesAppState;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub window_bg: [u8; 4],
    pub window_border: [u8; 4],
    pub sidebar_bg: [u8; 4],
    pub sidebar_border: [u8; 4],
    pub sidebar_active: [u8; 4],
    pub pill_bg: [u8; 4],
    pub pill_border: [u8; 4],
    pub pill_active: [u8; 4],
    pub divider: [u8; 4],
    pub scrollbar: [u8; 4],
    pub accent: [u8; 4],
    pub accent_text: [u8; 4],
    pub secondary_select: [u8; 4],
    pub text_primary: [u8; 4],
    pub text_secondary: [u8; 4],
    pub text_muted: [u8; 4],
    pub row_alt: [u8; 4],
    pub popup_bg: [u8; 4],
    pub popup_border: [u8; 4],
    pub popover_bg: [u8; 4],
    pub popover_rim: [u8; 4],
    pub popover_border: [u8; 4],
    pub popover_separator: [u8; 4],
    pub popover_danger: [u8; 4],
    pub popover_text_primary: [u8; 4],
    pub popover_text_secondary: [u8; 4],
    pub is_dark: bool,
}

pub(crate) const fn rgba(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    [r, g, b, a]
}

pub(crate) const fn palette_for(theme: FilesThemeMode) -> Palette {
    match theme {
        FilesThemeMode::LightGoldenGate => Palette {
            window_bg: rgba(255, 255, 255, 255),
            window_border: rgba(0, 0, 0, 24),
            sidebar_bg: rgba(236, 237, 241, 255),
            sidebar_border: rgba(218, 220, 226, 255),
            sidebar_active: rgba(217, 218, 221, 255),
            pill_bg: rgba(242, 243, 247, 255),
            pill_border: rgba(212, 217, 228, 195),
            pill_active: rgba(222, 224, 230, 255),
            divider: rgba(232, 234, 240, 255),
            scrollbar: rgba(140, 148, 162, 115),
            accent: rgba(0, 122, 255, 255),
            accent_text: rgba(255, 255, 255, 255),
            secondary_select: rgba(229, 232, 239, 255),
            text_primary: rgba(29, 31, 36, 255),
            text_secondary: rgba(102, 110, 124, 255),
            text_muted: rgba(134, 136, 142, 255),
            row_alt: rgba(247, 248, 251, 255),
            popup_bg: rgba(250, 251, 253, 255),
            popup_border: rgba(204, 209, 219, 255),
            popover_bg: rgba(246, 248, 252, 242),
            popover_rim: rgba(255, 255, 255, 80),
            popover_border: rgba(0, 0, 0, 28),
            popover_separator: rgba(0, 0, 0, 18),
            popover_danger: rgba(215, 48, 62, 255),
            popover_text_primary: rgba(26, 28, 32, 255),
            popover_text_secondary: rgba(110, 114, 122, 255),
            is_dark: false,
        },
        FilesThemeMode::DarkPurpleGlass => Palette {
            window_bg: rgba(30, 30, 32, 255),
            window_border: rgba(255, 255, 255, 22),
            sidebar_bg: rgba(38, 38, 40, 255),
            sidebar_border: rgba(54, 54, 58, 255),
            sidebar_active: rgba(62, 62, 65, 255),
            pill_bg: rgba(44, 44, 46, 255),
            pill_border: rgba(98, 98, 108, 105),
            pill_active: rgba(64, 64, 68, 255),
            divider: rgba(52, 52, 58, 255),
            scrollbar: rgba(175, 178, 188, 115),
            accent: rgba(10, 132, 255, 255),
            accent_text: rgba(255, 255, 255, 255),
            secondary_select: rgba(54, 54, 62, 255),
            text_primary: rgba(242, 242, 247, 255),
            text_secondary: rgba(174, 174, 182, 255),
            text_muted: rgba(132, 132, 140, 255),
            row_alt: rgba(35, 35, 38, 255),
            popup_bg: rgba(40, 40, 44, 255),
            popup_border: rgba(78, 78, 86, 255),
            popover_bg: rgba(32, 34, 38, 242),
            popover_rim: rgba(255, 255, 255, 38),
            popover_border: rgba(255, 255, 255, 32),
            popover_separator: rgba(255, 255, 255, 20),
            popover_danger: rgba(255, 105, 97, 255),
            popover_text_primary: rgba(242, 244, 248, 255),
            popover_text_secondary: rgba(168, 172, 180, 255),
            is_dark: true,
        },
    }
}

/// Returns the theme palette adjusted to `[ui].accent_index` (`RUSTYOS-114`).
pub(crate) const fn palette_for_with_accent(theme: FilesThemeMode, accent_index: usize) -> Palette {
    let mut pal = palette_for(theme);
    pal.accent = rusty_services::file_manager::accent_rgba_for_index(accent_index, pal.is_dark);
    pal
}

/// Scales overlay scrollbar alpha factor `[0.0..=1.0]` for transient post-scroll fading (`RUSTYOS-071`).
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn overlay_scrollbar_color(base: [u8; 4], factor: f32) -> [u8; 4] {
    let clamped = factor.clamp(0.0, 1.0);
    if clamped <= 0.005 {
        return [base[0], base[1], base[2], 0];
    }
    let a = (f32::from(base[3]) * clamped).round().clamp(0.0, 255.0) as u8;
    [base[0], base[1], base[2], a]
}

/// Renders one complete frame of `rusty-files` (including inline fallback popups) for tests.
#[cfg(test)]
pub(crate) fn render_window_pixels(
    buf: &mut [u8],
    width: u32,
    height: u32,
    state: &FilesAppState,
    layout: &FilesLayout,
) {
    render_window_pixels_with_popup(buf, width, height, state, layout, true);
}

/// Renders one complete frame of `rusty-files` into an `ARGB8888` SHM buffer, optionally
/// omitting inline popups when native Wayland `xdg_popup` surfaces are active.
pub(crate) fn render_window_pixels_with_popup(
    buf: &mut [u8],
    width: u32,
    height: u32,
    state: &FilesAppState,
    layout: &FilesLayout,
    include_inline_popup: bool,
) {
    let pal = palette_for_with_accent(state.theme, state.accent_index);
    let mut c = Canvas::new(buf, width, height);
    c.clear();

    let win_rect = Rect::new(0, 0, layout.width, layout.height);
    c.fill_rounded_rect(win_rect, WINDOW_CORNER_RADIUS, pal.window_bg);

    render_sidebar(&mut c, state, layout, &pal);
    render_toolbar(&mut c, state, layout, &pal);

    if let Some(tb) = &layout.tab_bar {
        render_tab_bar(&mut c, tb, &pal);
    }

    if let Some(sb) = &layout.search_scope_bar {
        render_search_scope_bar(&mut c, state, sb, &pal);
    }

    if let Some(banner) = layout.trash_banner_rect {
        render_trash_banner(&mut c, layout, banner, &pal);
    }

    match state.view_mode {
        FileViewMode::Icons => {
            if let Some(grid) = &layout.icons_grid {
                render_icons_grid(&mut c, state, grid, &pal);
            }
        }
        FileViewMode::Gallery => {
            if let Some(gal) = &layout.gallery {
                render_gallery_view(&mut c, state, gal, &pal);
            }
        }
        FileViewMode::List => {
            render_list_view(&mut c, state, layout, &pal);
        }
        FileViewMode::Columns => {
            render_miller_columns(&mut c, state, layout, &pal);
            if let Some(preview) = &layout.preview {
                render_preview_inspector(&mut c, state, preview, &pal);
            }
        }
    }

    render_status_bar(&mut c, state, layout, &pal);
    render_dialog_overlays(&mut c, state, layout, &pal);

    if include_inline_popup && let Some(popup) = &layout.popup {
        render_popup(&mut c, popup, None, &pal);
    }

    // Draw 1px hairline border first, then mask anti-aliased rounded window corners.
    c.stroke_rounded_rect(win_rect, WINDOW_CORNER_RADIUS, pal.window_border);
    c.clip_rounded_corners(win_rect, WINDOW_CORNER_RADIUS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::compute_layout;
    use crate::state::StartupOptions;
    use rusty_services::file_manager::UserFileLocations;
    use std::fs;

    #[test]
    fn renders_golden_gate_light_and_dark_themes_cleanly() {
        let root =
            std::env::temp_dir().join(format!("rustyos-files-render-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        let mut state = FilesAppState::new(locs, &StartupOptions::default());
        state.complete_pending_loads_sync();

        let (w, h) = (920_u32, 600_u32);
        let mut buf = vec![0_u8; (w * h * 4) as usize];
        let layout_light = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &layout_light);

        // Rounded top-left pixel outside 20px radius remains transparent
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);
        // Full-bleed edge-to-edge sidebar reaches x=2 with no white outer gutter and ends with 1px divider
        let pal_light = palette_for(FilesThemeMode::LightGoldenGate);
        let side_edge_idx = ((220 * w + 2) * 4) as usize;
        assert_eq!(
            &buf[side_edge_idx..side_edge_idx + 4],
            &[
                pal_light.sidebar_bg[2],
                pal_light.sidebar_bg[1],
                pal_light.sidebar_bg[0],
                pal_light.sidebar_bg[3],
            ]
        );
        let div_idx = ((220 * w + 195) * 4) as usize;
        assert_eq!(
            &buf[div_idx..div_idx + 4],
            &[
                pal_light.sidebar_border[2],
                pal_light.sidebar_border[1],
                pal_light.sidebar_border[0],
                pal_light.sidebar_border[3],
            ]
        );
        // Network sidebar icon in Locations stays neutral (not accent blue)
        if let Some(net_row) = layout_light
            .sidebar_rows
            .iter()
            .find(|r| r.location == rusty_services::file_manager::SidebarLocation::Network)
        {
            let net_has_accent = (net_row.rect.y..net_row.rect.bottom()).any(|py| {
                ((net_row.rect.x + 8)..(net_row.rect.x + 23)).any(|px| {
                    let idx = ((py.cast_unsigned() * w + px.cast_unsigned()) * 4) as usize;
                    buf[idx + 2] == pal_light.accent[0]
                        && buf[idx + 1] == pal_light.accent[1]
                        && buf[idx] == pal_light.accent[2]
                })
            });
            assert!(!net_has_accent, "Locations Network icon must be neutral");
        }
        // Traffic light close button in edge-to-edge sidebar header (#EE5A6A -> BGRA [106, 90, 238, 255])
        let red_idx = ((20 * w + 21) * 4) as usize;
        assert_eq!(&buf[red_idx..red_idx + 4], &[106, 90, 238, 255]);
        // Content background white (#FFFFFF)
        let bg_idx = ((120 * w + 650) * 4) as usize;
        assert_eq!(&buf[bg_idx..bg_idx + 4], &[255, 255, 255, 255]);

        // Toggle to Dark Golden Gate and re-render (`#1E1E20` -> BGRA [32, 30, 30, 255])
        state.toggle_theme();
        let layout_dark = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &layout_dark);
        assert_eq!(&buf[bg_idx..bg_idx + 4], &[32, 30, 30, 255]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scrolled_sidebar_and_overflowing_columns_never_bleed_outside_clips() {
        let root =
            std::env::temp_dir().join(format!("rustyos-files-clip-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("home/Documents");
        let _ = fs::create_dir_all(&docs);
        for i in 0..25 {
            let _ = fs::create_dir_all(docs.join(format!("Dir_{i:02}")));
        }
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        let mut state = FilesAppState::new(
            locs,
            &StartupOptions {
                initial_path: Some(docs),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();
        let (w, h) = (768_u32, 461_u32);
        let mut buf_unscrolled = vec![0_u8; (w * h * 4) as usize];
        let layout_unscrolled = compute_layout(&state, w, h);
        render_window_pixels(&mut buf_unscrolled, w, h, &state, &layout_unscrolled);

        // Scroll sidebar far enough so top rows shift into traffic-light y=23..37 zone
        state.sidebar_scroll_y = 85;
        state.select_entry(0, 14, false);

        let mut buf = vec![0_u8; (w * h * 4) as usize];
        let layout = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &layout);

        // Every pixel of traffic-light header band (y=18..45) stays identical to unscrolled frame
        for py in 18..45_u32 {
            for px in 16..180_u32 {
                let idx = ((py * w + px) * 4) as usize;
                assert_eq!(&buf[idx..idx + 4], &buf_unscrolled[idx..idx + 4]);
            }
        }

        // Status bar top divider & 2px margin above it remain clean white without folder bleed
        let gap_above_footer =
            (((layout.status_bar_rect.y - 2).cast_unsigned() * w + 250) * 4) as usize;
        assert_eq!(
            &buf[gap_above_footer..gap_above_footer + 4],
            &[255, 255, 255, 255]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stage_b_icons_gallery_tabs_and_overlays_render_within_rounded_clip() {
        let root = std::env::temp_dir().join(format!(
            "rustyos-files-stageb-render-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("home/Documents");
        let _ = fs::create_dir_all(docs.join("SubFolder"));
        let _ = fs::write(docs.join("readme.txt"), "hello rusty files\n");
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        let mut state = FilesAppState::new(
            locs,
            &StartupOptions {
                initial_path: Some(docs),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();
        let (w, h) = (960_u32, 620_u32);
        let mut buf = vec![0_u8; (w * h * 4) as usize];

        state.set_view_mode(FileViewMode::Icons);
        state.new_tab(None);
        let icons_layout = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &icons_layout);
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);

        state.set_view_mode(FileViewMode::Gallery);
        state.toggle_quick_look_selected();
        let gal_ql_layout = compute_layout(&state, w, h);
        render_window_pixels(&mut buf, w, h, &state, &gal_ql_layout);
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);

        // Search active focus ring + visible caret (`|`) when focused
        state.quick_look = None;
        state.search_active = true;
        state.caret_visible = true;
        state.set_search_query(String::from("read"));
        let search_layout = compute_layout(&state, w, h);
        render_window_pixels_with_popup(&mut buf, w, h, &state, &search_layout, false);
        let pal = palette_for_with_accent(state.theme, state.accent_index);
        let pill = search_layout.search_pill_rect;
        let caret_y = (pill.y + 12).cast_unsigned();
        let has_accent_caret = ((pill.x + 26)..pill.right() - 22).any(|px| {
            let idx = ((caret_y * w + px.cast_unsigned()) * 4) as usize;
            buf[idx + 2] == pal.accent[0]
                && buf[idx + 1] == pal.accent[1]
                && buf[idx] == pal.accent[2]
        });
        assert!(
            has_accent_caret,
            "Search input caret must render in accent color"
        );

        // Standalone native xdg_popup buffer renders hover highlight row cleanly
        let popup_menu = crate::state::OpenPopupMenu::SortMenu;
        let spec =
            crate::layout::compute_popup_surface_spec(&state, &search_layout, &popup_menu, 8);
        let pw = spec.surface_width.cast_unsigned();
        let ph = spec.surface_height.cast_unsigned();
        let mut pbuf = vec![0_u8; (pw * ph * 4) as usize];
        render_popup_surface_pixels(&mut pbuf, pw, ph, &spec.layout, Some(1), state.theme);
        let hovered_row = &spec.layout.items[1].rect;
        let mid_idx = (((hovered_row.y + hovered_row.h / 2).cast_unsigned() * pw
            + (hovered_row.x + 12).cast_unsigned())
            * 4) as usize;
        assert_eq!(pbuf[mid_idx + 2], pal.accent[0]);
        assert_eq!(pbuf[mid_idx + 1], pal.accent[1]);
        assert_eq!(pbuf[mid_idx], pal.accent[2]);

        // Overlay scrollbar helper + Share popover + Dark capsule specular sheen checks (`RUSTYOS-071`)
        assert_eq!(overlay_scrollbar_color(pal.scrollbar, 0.0)[3], 0);
        assert_eq!(overlay_scrollbar_color(pal.scrollbar, 1.0), pal.scrollbar);
        let share_spec = crate::layout::compute_popup_surface_spec(
            &state,
            &search_layout,
            &crate::state::OpenPopupMenu::ShareMenu,
            8,
        );
        assert!(
            share_spec
                .layout
                .items
                .iter()
                .any(|it| it.label == "Copy Path")
        );
        assert!(
            share_spec
                .layout
                .items
                .iter()
                .any(|it| it.label == "New Folder")
        );
        state.toggle_theme();
        state.overlay_scrollbar_alpha = 1.0;
        let dark_layout = compute_layout(&state, w, h);
        render_window_pixels_with_popup(&mut buf, w, h, &state, &dark_layout, false);
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn warm_scroll_frames_render_consistently_across_all_views() {
        let root =
            std::env::temp_dir().join(format!("rusty_files_scroll_bench_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("home/Documents");
        assert!(fs::create_dir_all(&docs).is_ok());
        for i in 0..36 {
            if i % 3 == 0 {
                assert!(fs::create_dir_all(docs.join(format!("Folder_{i:02}"))).is_ok());
            } else {
                assert!(
                    fs::write(
                        docs.join(format!("note_{i:02}.rs")),
                        b"fn main() { println!(\"hello\"); }\n",
                    )
                    .is_ok()
                );
            }
        }
        let locs = UserFileLocations::with_roots(
            &root.join("home"),
            &root.join("cfg"),
            &root.join("data"),
        );
        let mut state = FilesAppState::new(
            locs,
            &StartupOptions {
                initial_path: Some(docs),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();
        let (w, h) = (1060_u32, 660_u32);
        let mut buf = vec![0_u8; (w * h * 4) as usize];
        let mut buf2 = vec![0_u8; (w * h * 4) as usize];

        for mode in [
            FileViewMode::List,
            FileViewMode::Icons,
            FileViewMode::Columns,
            FileViewMode::Gallery,
        ] {
            state.set_view_mode(mode);
            let layout = compute_layout(&state, w, h);
            // Warm icon/glyph caches then verify deterministic repeat & inspect speed.
            render_window_pixels_with_popup(&mut buf, w, h, &state, &layout, false);
            let warm_start = std::time::Instant::now();
            for _ in 0..5 {
                render_window_pixels_with_popup(&mut buf2, w, h, &state, &layout, false);
            }
            let avg_ms = warm_start.elapsed().as_secs_f64() * 1000.0 / 5.0;
            eprintln!("bench {mode:?} {w}x{h}: warm_avg={avg_ms:.2} ms/frame");
            assert_eq!(buf, buf2);
            assert_eq!(&buf2[0..4], &[0, 0, 0, 0]);
        }

        let _ = fs::remove_dir_all(&root);
    }
}
