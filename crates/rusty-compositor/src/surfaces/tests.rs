use super::glyphs::*;
use super::sdf::*;
use super::xcursor::*;
use super::*;

#[test]
fn circle_and_corner_sdf_preserve_clean_edges_and_opaque_interior() {
    // Circle center must be opaque, extreme corner pixel outside circle must be transparent
    let center = sample_circle_pixel(6, 6, 12, [229, 107, 117, 255], None);
    let corner = sample_circle_pixel(0, 0, 12, [229, 107, 117, 255], None);
    assert_eq!(center[3], 255);
    assert_eq!(corner[3], 0);

    // TopLeft corner patch: (0, 0) outside arc is 0, inner pixel (9, 9) is filled
    let outer = sample_corner_pixel(
        0,
        0,
        10,
        CornerQuadrant::TopLeft,
        [32, 24, 46, 245],
        Some([255, 255, 255, 56]),
    );
    let inner = sample_corner_pixel(
        9,
        9,
        10,
        CornerQuadrant::TopLeft,
        [32, 24, 46, 245],
        Some([255, 255, 255, 56]),
    );
    assert_eq!(outer[3], 0);
    assert!(inner[3] >= 240);

    // Shadow corner: inside inner_radius is 0 alpha so glass stays clean; beyond inner_radius fades smoothly
    let inside_glass =
        sample_shadow_corner_pixel(23, 23, 10, 14, CornerQuadrant::TopLeft, [5, 2, 12], 96);
    let outside_halo =
        sample_shadow_corner_pixel(12, 12, 10, 14, CornerQuadrant::TopLeft, [5, 2, 12], 96);
    assert_eq!(inside_glass[3], 0);
    assert!(outside_halo[3] > 0 && outside_halo[3] <= 96);

    // Ambient wallpaper gradient is opaque and modulated smoothly around aurora glow
    let glow_px = sample_wallpaper_pixel(62, 35, 256, 160);
    let dark_px = sample_wallpaper_pixel(250, 10, 256, 160);
    assert_eq!(glow_px[3], 255);
    assert_eq!(dark_px[3], 255);
    assert!(glow_px[0] > dark_px[0]);

    // Desktop arrow cursor: opaque dark interior, bright rim, and transparent empty corner
    let arrow_inside = sample_cursor_arrow_pixel(5, 9);
    let arrow_outside = sample_cursor_arrow_pixel(22, 2);
    assert_eq!(arrow_inside[3], 255);
    assert!(arrow_inside[2] < 80);
    assert_eq!(arrow_outside[3], 0);
}

#[test]
fn parses_binary_xcursor_chunk_and_selects_closest_size() -> Result<(), String> {
    let mut blob = Vec::new();
    // Header: magic "Xcur", header_size=16, version=0x10000, ntoc=2
    blob.extend_from_slice(b"Xcur");
    blob.extend_from_slice(&16_u32.to_le_bytes());
    blob.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    blob.extend_from_slice(&2_u32.to_le_bytes());
    // TOC 0: size 16 at offset 40
    blob.extend_from_slice(&0xfffd_0002_u32.to_le_bytes());
    blob.extend_from_slice(&16_u32.to_le_bytes());
    blob.extend_from_slice(&40_u32.to_le_bytes());
    // TOC 1: size 24 at offset 80
    blob.extend_from_slice(&0xfffd_0002_u32.to_le_bytes());
    blob.extend_from_slice(&24_u32.to_le_bytes());
    blob.extend_from_slice(&80_u32.to_le_bytes());

    // Chunk 0 (1x1 at offset 40, total 40 bytes)
    blob.extend_from_slice(&36_u32.to_le_bytes());
    blob.extend_from_slice(&0xfffd_0002_u32.to_le_bytes());
    blob.extend_from_slice(&16_u32.to_le_bytes());
    blob.extend_from_slice(&1_u32.to_le_bytes());
    blob.extend_from_slice(&1_u32.to_le_bytes());
    blob.extend_from_slice(&1_u32.to_le_bytes());
    blob.extend_from_slice(&0_u32.to_le_bytes());
    blob.extend_from_slice(&0_u32.to_le_bytes());
    blob.extend_from_slice(&0_u32.to_le_bytes());
    blob.extend_from_slice(&0xFFFF_0000_u32.to_le_bytes()); // opaque red

    // Chunk 1 (1x1 at offset 80: blue pixel, hotspot (0,0), width=1, height=1)
    blob.extend_from_slice(&36_u32.to_le_bytes());
    blob.extend_from_slice(&0xfffd_0002_u32.to_le_bytes());
    blob.extend_from_slice(&24_u32.to_le_bytes());
    blob.extend_from_slice(&1_u32.to_le_bytes());
    blob.extend_from_slice(&1_u32.to_le_bytes());
    blob.extend_from_slice(&1_u32.to_le_bytes());
    blob.extend_from_slice(&0_u32.to_le_bytes());
    blob.extend_from_slice(&0_u32.to_le_bytes());
    blob.extend_from_slice(&0_u32.to_le_bytes());
    blob.extend_from_slice(&0xFF00_00FF_u32.to_le_bytes()); // ARGB opaque blue

    let (w, h, xhot, yhot, bgra) =
        parse_xcursor_bytes(&blob, 24).ok_or_else(|| "Xcur decode failed".to_owned())?;
    assert_eq!((w, h, xhot, yhot), (1, 1, 0, 0));
    assert_eq!(bgra, vec![0xFF, 0x00, 0x00, 0xFF]);
    Ok(())
}

#[test]
fn hidpi_cursor_origin_and_plane_limit_scale_correctly() {
    // 1.5x display: pointer (100, 50), hotspot (4.0, 2.0) => logical (96, 48) => physical (144, 72)
    let phys = scaled_cursor_physical_origin(Point::from((100, 50)), Point::from((4.0, 2.0)), 1.5);
    assert_eq!(phys, Point::from((144.0, 72.0)));

    // 2.0x scale pixel target is clamped cleanly from default/env size
    let px_1x = xcursor_target_pixel_size(1.0);
    let px_2x = xcursor_target_pixel_size(2.0);
    assert!(px_2x >= px_1x);

    // 24x24 cursor at 2.0x = 48x48 fits inside 64x64 plane, whereas 4.0x = 96x96 falls back to Unspecified.
    let plane_64 = Some(Size::<u32, BufferCoord>::from((64, 64)));
    assert_eq!(
        resolve_cursor_element_kind(true, Size::from((24, 24)), 2.0, plane_64),
        Kind::Cursor
    );
    assert_eq!(
        resolve_cursor_element_kind(true, Size::from((24, 24)), 4.0, plane_64),
        Kind::Unspecified
    );
    assert_eq!(
        resolve_cursor_element_kind(false, Size::from((24, 24)), 1.0, plane_64),
        Kind::Unspecified
    );
}

#[test]
fn dithered_wallpaper_eliminates_flat_banding_and_ui_glyphs_rasterize_cleanly() {
    // Check Bayer 4x4 dither modulates adjacent gradient steps cleanly
    let mut blue_samples = std::collections::BTreeSet::new();
    for y in 0..4 {
        for x in 0..4 {
            let px = sample_wallpaper_pixel(200 + x, 120 + y, 640, 360);
            assert_eq!(px[3], 255);
            blue_samples.insert(px[0]);
        }
    }
    assert!(blue_samples.len() >= 2);

    // Verify all 6 TopBar/Spotlight SDF UI glyph kinds produce smooth non-empty centers & clean edges
    let white = [245, 240, 255, 255];
    for kind in [
        UiGlyphKind::RustyLogo,
        UiGlyphKind::Wifi {
            connected: true,
            signal_percent: Some(85),
        },
        UiGlyphKind::Wifi {
            connected: false,
            signal_percent: None,
        },
        UiGlyphKind::Volume {
            level_percent: 80,
            muted: false,
        },
        UiGlyphKind::Volume {
            level_percent: 0,
            muted: true,
        },
        UiGlyphKind::Battery {
            percent: 65,
            charging: true,
        },
        UiGlyphKind::Search,
        UiGlyphKind::ControlCenter,
        UiGlyphKind::Info,
        UiGlyphKind::TerminalPrompt,
        UiGlyphKind::Folder,
        UiGlyphKind::Grid,
        UiGlyphKind::LayoutSplit,
        UiGlyphKind::Motion,
        UiGlyphKind::Transparency,
        UiGlyphKind::Refresh,
        UiGlyphKind::Lock,
        UiGlyphKind::Shortcuts,
        UiGlyphKind::WindowMax,
        UiGlyphKind::WindowMin,
        UiGlyphKind::Close,
        UiGlyphKind::ArrowNext,
        UiGlyphKind::ArrowPrev,
        UiGlyphKind::Bluetooth,
        UiGlyphKind::Sun,
    ] {
        let mut non_zero = 0_usize;
        for py in 0..16 {
            for px in 0..16 {
                let p = sample_ui_glyph_pixel(px, py, 16, kind, white);
                if p[3] > 24 {
                    non_zero += 1;
                }
            }
        }
        assert!(non_zero >= 12, "Glyph {kind:?} too sparse: {non_zero}");
        let corner = sample_ui_glyph_pixel(0, 0, 16, kind, white);
        assert_eq!(corner[3], 0, "Glyph {kind:?} corner should be transparent");
    }

    // TopBar wide rectangular Battery (26x20) fills wider horizontal aspect with transparent corners
    let mut wide_cols = 0_usize;
    for px in 0..26 {
        let p = sample_ui_glyph_pixel_rect(
            px,
            10,
            26,
            20,
            UiGlyphKind::Battery {
                percent: 82,
                charging: true,
            },
            white,
        );
        if p[3] > 24 {
            wide_cols += 1;
        }
    }
    assert!(
        wide_cols >= 20,
        "Rectangular battery too narrow: {wide_cols}"
    );
    assert_eq!(
        sample_ui_glyph_pixel_rect(
            0,
            0,
            26,
            20,
            UiGlyphKind::Battery {
                percent: 82,
                charging: true,
            },
            white
        )[3],
        0
    );
}

#[test]
fn topbar_wifi_and_battery_glyphs_match_live_state() {
    let white = [245, 240, 255, 255];

    // RUSTYOS-052: 3-band Wi-Fi fan (wedge anchor + 2 arcs) reacts dynamically to live signal bars
    let band1 = |sig| {
        sample_ui_glyph_pixel(
            10,
            14,
            20,
            UiGlyphKind::Wifi {
                connected: true,
                signal_percent: sig,
            },
            white,
        )[3]
    };
    let band2 = |sig| {
        sample_ui_glyph_pixel(
            10,
            9,
            20,
            UiGlyphKind::Wifi {
                connected: true,
                signal_percent: sig,
            },
            white,
        )[3]
    };
    let band3 = |sig| {
        sample_ui_glyph_pixel(
            10,
            5,
            20,
            UiGlyphKind::Wifi {
                connected: true,
                signal_percent: sig,
            },
            white,
        )[3]
    };
    assert!(band1(Some(15)) > 180 && band2(Some(15)) < 120 && band3(Some(15)) < 120);
    assert!(band1(Some(42)) > 180 && band2(Some(42)) > 180 && band3(Some(42)) < 120);
    assert!(band1(Some(85)) > 180 && band2(Some(85)) > 180 && band3(Some(85)) > 180);
    let gap23 = sample_ui_glyph_pixel(
        10,
        7,
        20,
        UiGlyphKind::Wifi {
            connected: true,
            signal_percent: Some(100),
        },
        white,
    )[3];
    assert!(
        gap23 < 30,
        "expected clear gap between bands 2 and 3, got alpha={gap23}"
    );

    // macOS Control Center glyph: hollow top outline capsule + solid bottom capsule with right cutout
    let cc_top_hollow = sample_ui_glyph_pixel(8, 5, 16, UiGlyphKind::ControlCenter, white);
    let cc_bot_solid = sample_ui_glyph_pixel(5, 11, 16, UiGlyphKind::ControlCenter, white);
    let cc_bot_cutout = sample_ui_glyph_pixel(11, 11, 16, UiGlyphKind::ControlCenter, white);
    assert!(cc_top_hollow[3] < 90);
    assert!(cc_bot_solid[0] > 180 && cc_bot_solid[3] > 180);
    assert!(cc_bot_cutout[3] < 90);
}

#[test]
fn topbar_battery_charging_cutout_and_charge_fill() {
    let white = [245, 240, 255, 255];
    let bat_charging_core = sample_ui_glyph_pixel_rect(
        11,
        10,
        26,
        20,
        UiGlyphKind::Battery {
            percent: 100,
            charging: true,
        },
        white,
    );
    let bat_charging_side = sample_ui_glyph_pixel_rect(
        5,
        10,
        26,
        20,
        UiGlyphKind::Battery {
            percent: 100,
            charging: true,
        },
        white,
    );
    let mut charging_halo_min_alpha = u8::MAX;
    let mut unplugged_min_alpha = u8::MAX;
    for py in 7..=13 {
        for px in 8..=15 {
            let p_chg = sample_ui_glyph_pixel_rect(
                px,
                py,
                26,
                20,
                UiGlyphKind::Battery {
                    percent: 100,
                    charging: true,
                },
                white,
            );
            let p_dc = sample_ui_glyph_pixel_rect(
                px,
                py,
                26,
                20,
                UiGlyphKind::Battery {
                    percent: 100,
                    charging: false,
                },
                white,
            );
            charging_halo_min_alpha = charging_halo_min_alpha.min(p_chg[3]);
            unplugged_min_alpha = unplugged_min_alpha.min(p_dc[3]);
        }
    }
    assert!(bat_charging_side[0] > 200 && bat_charging_core[0] > 180);
    assert!(
        charging_halo_min_alpha < 170,
        "expected transparent cutout moat around white charging bolt: min_a={charging_halo_min_alpha}"
    );
    assert!(
        unplugged_min_alpha > 210,
        "expected solid white 100% fill when unplugged: min_a={unplugged_min_alpha}"
    );

    // Low charge shows proportional fill (bright left vs empty translucent right track)
    let bat_25_left = sample_ui_glyph_pixel_rect(
        5,
        10,
        26,
        20,
        UiGlyphKind::Battery {
            percent: 25,
            charging: false,
        },
        white,
    );
    let bat_25_right = sample_ui_glyph_pixel_rect(
        18,
        10,
        26,
        20,
        UiGlyphKind::Battery {
            percent: 25,
            charging: false,
        },
        white,
    );
    assert!(bat_25_left[0] > 200 && bat_25_right[3] < 100);
}

#[test]
fn popover_glass_and_traffic_lights_rasterize_cleanly() {
    let pop_corner =
        sample_glass_panel_pixel(0, 0, 248, 220, 16, 0.78, false, GlassPanelStyle::Popover);
    let pop_center = sample_glass_panel_pixel(
        124,
        110,
        248,
        220,
        16,
        0.78,
        false,
        GlassPanelStyle::Popover,
    );
    let pop_top_rim =
        sample_glass_panel_pixel(124, 0, 248, 220, 16, 0.78, false, GlassPanelStyle::Popover);
    assert_eq!(pop_corner[3], 0);
    assert!(pop_center[3] > 120 && pop_center[3] < 240);
    assert!(pop_top_rim[3] > 0);

    let mod_center = sample_glass_panel_pixel(
        70,
        32,
        140,
        64,
        14,
        0.78,
        false,
        GlassPanelStyle::ControlModule,
    );
    assert!(mod_center[3] > 16 && mod_center[3] < pop_center[3]);
    for glyph in [
        UiGlyphKind::MediaPrev,
        UiGlyphKind::MediaPlay,
        UiGlyphKind::MediaPause,
        UiGlyphKind::MediaNext,
        UiGlyphKind::MusicNote,
        UiGlyphKind::Moon,
        UiGlyphKind::OverviewSpaces,
        UiGlyphKind::Keyboard,
        UiGlyphKind::PowerSymbol,
        UiGlyphKind::Plus,
        UiGlyphKind::Check,
        UiGlyphKind::ChevronRight,
        UiGlyphKind::Trash,
    ] {
        let center = sample_ui_glyph_pixel(10, 10, 20, glyph, [255, 255, 255, 255]);
        let corner = sample_ui_glyph_pixel(0, 0, 20, glyph, [255, 255, 255, 255]);
        assert_eq!(corner[3], 0);
        let any_lit = (0..20)
            .flat_map(|y| (0..20).map(move |x| (x, y)))
            .any(|(x, y)| sample_ui_glyph_pixel(x, y, 20, glyph, [255, 255, 255, 255])[3] > 0);
        assert!(any_lit || center[3] > 0);
    }
    let avatar_corner = sample_fallback_avatar_pixel(0, 0, 44, 0, true);
    let avatar_center = sample_fallback_avatar_pixel(22, 22, 44, 0, true);
    assert_eq!(avatar_corner[3], 0);
    assert_eq!(avatar_center[3], 255);

    // Traffic lights: clean circular contour + hover glyph contrast
    let close_plain = sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Close, true, false);
    let close_hover = sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Close, true, true);
    let close_unfocused =
        sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Close, false, false);
    let min_hover = sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Minimize, true, true);
    let max_hover = sample_traffic_light_pixel(6, 6, 12, TrafficLightKind::Maximize, true, true);
    let corner = sample_traffic_light_pixel(0, 0, 12, TrafficLightKind::Close, true, true);
    assert_eq!(corner[3], 0);
    assert_eq!(close_plain[3], 255);
    assert!(close_unfocused[3] >= 180 && close_unfocused != close_plain);
    // Hover glyph darkens the center of Close/Min/Max compared to plain gel sphere.
    assert_ne!(close_plain, close_hover);
    assert_ne!(min_hover, close_hover);
    assert_ne!(max_hover, close_hover);
}

#[test]
fn dock_glass_and_icon_shadow_rasterize_cleanly() {
    let width = 320;
    let height = 68;
    let radius = 22;

    // Translucent frosted Dock glass: corners transparent, top rim brighter than center,
    // and center remains light/translucent rather than an opaque dark slab.
    let corner = sample_dock_glass_pixel(0, 0, width, height, radius, 0.78, false);
    let top_rim = sample_dock_glass_pixel(width / 2, 0, width, height, radius, 0.78, false);
    let center = sample_dock_glass_pixel(width / 2, height / 2, width, height, radius, 0.78, false);
    assert_eq!(corner[3], 0);
    assert!(top_rim[3] > center[3]);
    assert!(center[3] > 20 && center[3] <= 120);

    // Reduce-transparency mode falls back to opaque dark plum-slate container.
    let opaque_center =
        sample_dock_glass_pixel(width / 2, height / 2, width, height, radius, 0.78, true);
    assert!(opaque_center[3] >= 245);

    // Soft icon drop shadow fades cleanly to zero at outer pad corners.
    let icon_size = 50;
    let pad = 7;
    let shadow_corner = sample_dock_icon_shadow_pixel(0, 0, icon_size, pad);
    let shadow_center = sample_dock_icon_shadow_pixel(
        i32::midpoint(icon_size, pad * 2),
        i32::midpoint(icon_size, pad * 2),
        icon_size,
        pad,
    );
    assert_eq!(shadow_corner[3], 0);
    assert!(shadow_center[3] >= 20 && shadow_center[3] <= 34);
}
