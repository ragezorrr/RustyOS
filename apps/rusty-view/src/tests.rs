//! Unit tests for `rusty-view` image document, playlist, viewport, and toolbar components.

use std::path::PathBuf;
use std::sync::Arc;

use rusty_ui::{PixelFormat, ThemeMode};

use crate::document::{ImageDocument, format_file_size};
use crate::hud::InfoHud;
use crate::playlist::{Playlist, is_supported_image_extension};
use crate::state::ViewerState;
use crate::toolbar::{Toolbar, ToolbarAction};
use crate::viewport::Viewport;

#[test]
fn test_format_file_size() {
    assert_eq!(format_file_size(0), "0 B");
    assert_eq!(format_file_size(512), "512 B");
    assert_eq!(format_file_size(1024), "1.0 KB");
    assert_eq!(format_file_size(1536), "1.5 KB");
    assert_eq!(format_file_size(1024 * 1024), "1.0 MB");
    assert_eq!(format_file_size(1024 * 1024 * 1024 * 2), "2.0 GB");
}

#[test]
fn test_is_supported_image_extension() {
    assert!(is_supported_image_extension(&PathBuf::from("photo.png")));
    assert!(is_supported_image_extension(&PathBuf::from("PHOTO.PNG")));
    assert!(is_supported_image_extension(&PathBuf::from("test.bmp")));
    assert!(is_supported_image_extension(&PathBuf::from("image.ppm")));
    assert!(is_supported_image_extension(&PathBuf::from("canvas.pnm")));
    assert!(!is_supported_image_extension(&PathBuf::from("doc.pdf")));
    assert!(!is_supported_image_extension(&PathBuf::from("vector.svg")));
    assert!(!is_supported_image_extension(&PathBuf::from(
        "archive.tar.gz"
    )));
}

#[test]
fn test_document_rotations_and_flip() {
    // 2x3 dummy image: 6 pixels RGBA
    let raw_pixels: Vec<u8> = vec![
        1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255, 4, 4, 4, 255, 5, 5, 5, 255, 6, 6, 6, 255,
    ];
    let mut doc = ImageDocument {
        filename: "test.png".to_owned(),
        width: 2,
        height: 3,
        pixels: Arc::from(raw_pixels.into_boxed_slice()),
        format: PixelFormat::Rgba8888,
        file_size_bytes: 24,
        rotation_deg: 0,
        flipped_h: false,
    };

    // Rotate CW 90: width becomes 3, height becomes 2
    doc.rotate_cw();
    assert_eq!(doc.width, 3);
    assert_eq!(doc.height, 2);
    assert_eq!(doc.rotation_deg, 90);

    // Rotate CW 90 again: width 2, height 3
    doc.rotate_cw();
    assert_eq!(doc.width, 2);
    assert_eq!(doc.height, 3);
    assert_eq!(doc.rotation_deg, 180);

    // Rotate CCW 90: returns to 90 deg
    doc.rotate_ccw();
    assert_eq!(doc.width, 3);
    assert_eq!(doc.height, 2);
    assert_eq!(doc.rotation_deg, 90);

    // Flip horizontal
    assert!(!doc.flipped_h);
    doc.flip_horizontal();
    assert!(doc.flipped_h);
    doc.flip_horizontal();
    assert!(!doc.flipped_h);
}

#[test]
fn test_playlist_carousel_navigation() {
    let mut playlist = Playlist {
        items: vec![
            PathBuf::from("/tmp/1.png"),
            PathBuf::from("/tmp/2.bmp"),
            PathBuf::from("/tmp/3.ppm"),
        ],
        current_index: Some(0),
    };

    assert_eq!(playlist.index_label(), Some((1, 3)));

    // Next
    assert_eq!(playlist.next(), Some(&PathBuf::from("/tmp/2.bmp")));
    assert_eq!(playlist.index_label(), Some((2, 3)));

    assert_eq!(playlist.next(), Some(&PathBuf::from("/tmp/3.ppm")));
    assert_eq!(playlist.index_label(), Some((3, 3)));

    // Next wraps to first
    assert_eq!(playlist.next(), Some(&PathBuf::from("/tmp/1.png")));
    assert_eq!(playlist.index_label(), Some((1, 3)));

    // Prev wraps to last
    assert_eq!(playlist.prev(), Some(&PathBuf::from("/tmp/3.ppm")));
    assert_eq!(playlist.index_label(), Some((3, 3)));

    assert_eq!(playlist.prev(), Some(&PathBuf::from("/tmp/2.bmp")));
    assert_eq!(playlist.index_label(), Some((2, 3)));
}

#[test]
fn test_viewport_zoom_and_panning() {
    let mut vp = Viewport::default();
    assert!((vp.scale - 1.0).abs() < f32::EPSILON);

    // Fit to window
    vp.fit_to_window(800, 600, 1600, 1200);
    assert!(vp.is_fitting);
    assert!(vp.scale <= 0.5);

    // Zoom in
    vp.zoom_in(800, 600, 1600, 1200);
    assert!(!vp.is_fitting);
    assert!(vp.scale > 0.5);

    // Actual size
    vp.actual_size();
    assert!((vp.scale - 1.0).abs() < f32::EPSILON);
    assert!((vp.pan_x).abs() < f32::EPSILON);
    assert!((vp.pan_y).abs() < f32::EPSILON);

    // Drag / pan
    vp.start_drag(100.0, 100.0);
    vp.drag_to(150.0, 130.0);
    assert_eq!(vp.pan_x, 50.0);
    assert_eq!(vp.pan_y, 30.0);
    vp.end_drag();
    assert!(vp.drag_start.is_none());

    // Destination rect centering
    let rect = vp.compute_dest_rect(800, 600, 400, 300);
    assert_eq!(rect.width, 400);
    assert_eq!(rect.height, 300);
    assert_eq!(rect.x, 250);
    assert_eq!(rect.y, 180);
}

#[test]
fn test_toolbar_geometry_and_hud() {
    let rect = Toolbar::compute_rect(1000, 700);
    assert_eq!(rect.width, 330);
    assert_eq!(rect.height, 42);
    // Centered horizontally: (1000 - 330) / 2 = 335
    assert_eq!(rect.x, 335);
    // Padded 18px from bottom: 700 - 42 - 18 = 640
    assert_eq!(rect.y, 640);

    let mut toolbar = Toolbar::default();
    assert_eq!(toolbar.opacity, 1.0);

    // Clicking prev button (first slot at x ~ 335 + 8)
    let action = toolbar.on_pointer_down(350.0, 650.0, 1000, 700);
    assert_eq!(action, Some(ToolbarAction::Prev));

    let mut hud = InfoHud::default();
    assert!(!hud.is_visible);
    hud.toggle();
    assert!(hud.is_visible);
    hud.toggle();
    assert!(!hud.is_visible);
}

#[test]
fn test_viewer_state_initialization() {
    let mut state = ViewerState::new(None, 800, 600, ThemeMode::Dark);
    assert!(state.document.is_none());
    assert!(state.dirty);
    assert_eq!(state.theme_mode, ThemeMode::Dark);

    // Action toggle info
    state.action_toggle_info();
    assert!(state.hud.is_visible);

    // Zoom in on empty document does not crash
    state.action_zoom_in(800, 600);
    state.action_actual_size();
    state.action_fit(800, 600);

    // Toolbar ToggleInfo click fires only once on pointer_up (not twice on down + up)
    let hud_before = state.hud.is_visible;
    let bar = Toolbar::compute_rect(1000, 700);
    // ToggleInfo slot is the 7th slot: x = bar.x + 12 + (32+4)+(30+4)+(52+4)+(30+4)+(36+4)+(36+4) = bar.x + 252
    let info_x = f64::from(bar.x + 260);
    let info_y = f64::from(bar.y + 20);
    let _ = state.on_pointer_down(info_x, info_y, 1000, 700);
    assert_eq!(state.hud.is_visible, hud_before);
    let _ = state.on_pointer_up(info_x, info_y, 1000, 700);
    assert_ne!(state.hud.is_visible, hud_before);
}
