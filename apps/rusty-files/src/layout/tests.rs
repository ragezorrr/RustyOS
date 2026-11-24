use crate::state::{OpenPopupMenu, ScrollTargetArea};

use super::*;
use crate::state::StartupOptions;
use rusty_services::file_manager::UserFileLocations;

fn test_state() -> (FilesAppState, std::path::PathBuf) {
    let root =
        std::env::temp_dir().join(format!("rustyos-files-layout-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let locs =
        UserFileLocations::with_roots(&root.join("home"), &root.join("cfg"), &root.join("data"));
    (FilesAppState::new(locs, &StartupOptions::default()), root)
}

#[test]
fn rustyos_layout_and_hit_testing_work() {
    let (mut state, root) = test_state();
    let layout = compute_layout(&state, 1060, 660);

    assert_eq!(layout.sidebar_rect, Rect::new(0, 0, SIDEBAR_WIDTH, 660));
    assert_eq!(hit_test(&layout, 25.0, 25.0), Some(FilesHit::TrafficClose));
    assert_eq!(
        hit_test(&layout, 45.0, 25.0),
        Some(FilesHit::TrafficMinimize)
    );
    assert_eq!(
        hit_test(&layout, 65.0, 25.0),
        Some(FilesHit::TrafficMaximize)
    );
    assert_eq!(hit_test(&layout, 224.0, 30.0), Some(FilesHit::NavBack));
    assert_eq!(hit_test(&layout, 260.0, 30.0), Some(FilesHit::NavForward));

    let cols_seg = layout.view_segments[2].1;
    assert_eq!(
        hit_test(
            &layout,
            f64::from(cols_seg.x + 12),
            f64::from(cols_seg.y + 12)
        ),
        Some(FilesHit::ViewMode(FileViewMode::Columns))
    );

    let search = layout.search_pill_rect;
    assert_eq!(
        hit_test(&layout, f64::from(search.x + 12), f64::from(search.y + 12)),
        Some(FilesHit::SearchToggle)
    );

    // Popup clamping keeps popover inside window bounds even near bottom-right corner
    state.popup = Some(OpenPopupMenu::ContextMenu {
        x: 1050,
        y: 650,
        target_entry: None,
        target_dir: None,
    });
    let popup_layout = compute_layout(&state, 1060, 660);
    if let Some(p) = &popup_layout.popup {
        assert!(p.rect.right() <= 1052);
        assert!(p.rect.bottom() <= 652);
    }
    state.popup = None;
    state.search_active = true;
    let narrow = compute_layout(&state, 768, 461);
    assert!(narrow.nav_pill_rect.right() < narrow.view_pill_rect.x);
    assert!(narrow.view_pill_rect.right() < narrow.sort_pill_rect.x);
    assert!(narrow.sort_pill_rect.right() < narrow.action_pill_rect.x);
    assert!(narrow.action_pill_rect.right() < narrow.search_pill_rect.x);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stage_b_views_tabs_crumbs_and_overlays_hit_test() {
    let (mut state, root) = test_state();
    let docs = state.locations.home.join("Documents");
    let _ = std::fs::create_dir_all(&docs);
    state.navigate_to_directory(docs.clone());
    state.complete_pending_loads_sync();

    state.set_view_mode(FileViewMode::Icons);
    let default_layout = compute_layout(&state, 1060, 660);
    assert!(default_layout.icons_grid.is_some());
    assert_eq!(default_layout.path_crumbs.len(), 0);
    assert!(default_layout.search_pill_rect.w >= 120);
    let narrow_unfocused = compute_layout(&state, 768, 461);
    assert_eq!(narrow_unfocused.search_pill_rect.w, 32);

    state.toggle_path_bar();
    let icons_layout = compute_layout(&state, 1060, 660);
    if let Some(crumb) = icons_layout.path_crumbs.last() {
        assert_eq!(
            hit_test(
                &icons_layout,
                f64::from(crumb.rect.x + 6),
                f64::from(crumb.rect.y + 6)
            ),
            Some(FilesHit::PathCrumb(crumb.path.clone()))
        );
    }

    state.search_active = true;
    state.set_search_query(String::from("doc"));
    let search_layout = compute_layout(&state, 1060, 660);
    if let Some(sb) = &search_layout.search_scope_bar {
        assert_eq!(
            hit_test(
                &search_layout,
                f64::from(sb.home_rect.x + 8),
                f64::from(sb.home_rect.y + 8)
            ),
            Some(FilesHit::SearchScope(SearchScope::Home))
        );
    }
    state.search_active = false;
    state.set_search_query(String::new());

    state.new_tab(None);
    state.set_view_mode(FileViewMode::Gallery);
    let gal_layout = compute_layout(&state, 1060, 660);
    assert!(gal_layout.tab_bar.is_some());
    if let Some(tb) = &gal_layout.tab_bar {
        assert_eq!(
            hit_test(
                &gal_layout,
                f64::from(tb.add_rect.x + 8),
                f64::from(tb.add_rect.y + 8)
            ),
            Some(FilesHit::TabAdd)
        );
    }
    assert!(gal_layout.gallery.is_some());
    if let Some(gal) = &gal_layout.gallery {
        assert_eq!(
            hit_test(
                &gal_layout,
                f64::from(gal.quick_look_btn.x + 8),
                f64::from(gal.quick_look_btn.y + 8)
            ),
            Some(FilesHit::GalleryQuickLookBtn)
        );
        assert_eq!(
            scroll_area_at(
                &gal_layout,
                f64::from(gal.strip_rect.x + 12),
                f64::from(gal.strip_rect.y + 12)
            ),
            Some(ScrollTargetArea::GalleryStrip)
        );
    }

    state.open_go_to_folder();
    let goto_layout = compute_layout(&state, 1060, 660);
    assert!(goto_layout.overlays.go_to_folder.is_some());
    assert_eq!(
        hit_test(&goto_layout, 80.0, 200.0),
        Some(FilesHit::GoToClose)
    );
    if let Some(gt) = &goto_layout.overlays.go_to_folder {
        assert_eq!(
            hit_test(
                &goto_layout,
                f64::from(gt.clear_rect.x + 8),
                f64::from(gt.clear_rect.y + 8)
            ),
            Some(FilesHit::GoToClear)
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn xdg_popup_surface_spec_unclamped_for_outside_window_overflow() {
    let (state, root) = test_state();
    let layout = compute_layout(&state, 1060, 660);
    let menu = OpenPopupMenu::ContextMenu {
        x: 1020,
        y: 620,
        target_entry: None,
        target_dir: Some(root.clone()),
    };
    let spec = compute_popup_surface_spec(&state, &layout, &menu, 8);
    assert_eq!(spec.anchor_x, 1020);
    assert_eq!(spec.anchor_y, 620);
    assert_eq!(spec.shadow_pad, 8);
    assert!(spec.anchor_x + spec.layout.rect.w > layout.width);
    assert!(spec.anchor_y + spec.layout.rect.h > layout.height);
    assert_eq!(spec.surface_width, spec.layout.rect.w + 16);
    assert_eq!(spec.surface_height, spec.layout.rect.h + 16);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn search_scope_clear_and_share_button_hit_test() {
    let (mut state, root) = test_state();
    let idle = compute_layout(&state, 768, 461);
    assert_eq!(
        hit_test(
            &idle,
            f64::from(idle.action_primary_rect.x + 12),
            f64::from(idle.action_primary_rect.y + 16),
        ),
        Some(FilesHit::ActionPrimary)
    );

    state.search_active = true;
    state.set_search_query(String::from("doc"));
    let search_1060 = compute_layout(&state, 1060, 660);
    assert!(search_1060.search_scope_bar.is_some());
    if let Some(sb) = &search_1060.search_scope_bar {
        assert!(sb.clear_rect.is_some());
        if let Some(clear) = sb.clear_rect {
            assert_eq!(
                hit_test(
                    &search_1060,
                    f64::from(clear.x + 10),
                    f64::from(clear.y + 8)
                ),
                Some(FilesHit::SearchScopeClear)
            );
        }
    }
    let search_768 = compute_layout(&state, 768, 461);
    assert_eq!(
        hit_test(&search_768, 768.0 - 36.0, 72.0),
        Some(FilesHit::SearchScopeClear)
    );
    let _ = std::fs::remove_dir_all(&root);
}
