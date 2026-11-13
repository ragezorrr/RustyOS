//! Quick Settings Control Center layout, slider/button hit-testing, text, and glass islands.

use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::CompositorRenderElement;
use super::tokens::{
    COLOR_ACCENT, COLOR_ACCENT_HOVER, COLOR_ACCENT_MUTED, COLOR_SUBTLE, COLOR_SURFACE,
    COLOR_SURFACE_OVERLAY, TOP_BAR_HEIGHT, anim_offset, make_solid_element,
    push_liquid_glass_panel, push_rounded_glass_box, rect_contains, rounded_rect_contains,
    truncate_ascii,
};

/// Live data for the Quick Settings Control Center popover (`RUSTYOS-021`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct QuickSettingsRenderState<'a> {
    /// Active workspace layout mode (`"floating"` or `"master-stack"`).
    pub layout_mode: &'a str,
    /// Accessibility reduced-motion flag.
    pub reduced_motion: bool,
    /// Human-readable network label (SSID / Ethernet / Disconnected).
    pub network_label: &'a str,
    /// Whether network interface is up.
    pub net_connected: bool,
    /// Live Wi-Fi radio enabled flag (`RUSTYOS-053`).
    pub wifi_enabled: bool,
    /// Live Wi-Fi signal strength percentage (`0..=100`, `RUSTYOS-052`).
    pub wifi_signal_percent: Option<u8>,
    /// Whether a Bluetooth adapter is available (`RUSTYOS-027`).
    pub bt_available: bool,
    /// Whether Bluetooth adapter power is enabled (`RUSTYOS-027`).
    pub bt_powered: bool,
    /// Concise Bluetooth status label (`RUSTYOS-027`).
    pub bt_label: &'a str,
    /// Battery charge percentage.
    pub battery_percent: u8,
    /// Whether a hardware battery was detected.
    pub has_battery: bool,
    /// Whether AC adapter is connected.
    pub ac_online: bool,
    /// Whether battery is actively charging (`RUSTYOS-052`).
    pub battery_charging: bool,
    /// Whether sysfs display backlight control is available (`RUSTYOS-053`).
    pub brightness_available: bool,
    /// Current hardware backlight brightness percentage (`0..=100`, `RUSTYOS-053`).
    pub brightness_percent: u8,
    /// Master output volume percentage (`0..=100`).
    pub volume_percent: u8,
    /// Master audio mute flag.
    pub audio_muted: bool,
    /// Whether an active MPRIS2 media player is registered (`RUSTYOS-053`).
    pub media_active: bool,
    /// Whether active MPRIS2 media playback is currently playing (`RUSTYOS-053`).
    pub media_playing: bool,
    /// Active MPRIS2 track title (empty when nothing is playing) (`RUSTYOS-053`).
    pub media_title: &'a str,
    /// Whether Focus (Do Not Disturb) notification suppression is active (`RUSTYOS-053`).
    pub focus_mode: bool,
    /// Median frame interval in microseconds (`p50`).
    pub frame_p50_us: u64,
    /// Number of visible windows on active workspace.
    pub window_count: usize,
    /// Animated reveal progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub progress: f64,
    /// Global shell glass opacity scalar (`0.35..=1.0`) (`RUSTYOS-048`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels mode (`RUSTYOS-048`).
    pub reduce_transparency: bool,
}

/// Active slider drag target inside the Control Center (`RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickSettingsSliderDrag {
    /// Master Sound volume slider.
    Sound,
    /// Display brightness slider (falling back to glass opacity when no backlight exists) (`RUSTYOS-053`).
    Display,
}

/// Interactive click target inside or outside the Quick Settings popover (`RUSTYOS-021`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickSettingsHit {
    /// Toggle Wi-Fi radio via NetworkManager / `nmcli` (`RUSTYOS-053`).
    ToggleWifi,
    /// Mute / unmute audio output.
    ToggleMute,
    /// Decrease audio volume.
    VolumeDown,
    /// Increase audio volume.
    VolumeUp,
    /// Directly set volume percentage (`0..=100`) along the Sound capsule slider (`RUSTYOS-048`).
    SetVolumePercent(u8),
    /// Toggle `reduce_transparency` high-contrast mode (`RUSTYOS-048`).
    ToggleReduceTransparency,
    /// Set Display brightness (or fallback glass opacity) along the Display track (`0..=100`) (`RUSTYOS-053`).
    SetDisplayPercent(u8),
    /// Skip to previous track via MPRIS2 (`RUSTYOS-053`).
    MediaPrev,
    /// Toggle playback via MPRIS2 (`RUSTYOS-053`).
    MediaPlayPause,
    /// Skip to next track via MPRIS2 (`RUSTYOS-053`).
    MediaNext,
    /// Toggle workspace layout (`Floating <-> MasterStack`).
    ToggleLayout,
    /// Open Workspace Overview from Control Center (`RUSTYOS-053`).
    ToggleOverview,
    /// Toggle accessibility `reduced_motion`.
    ToggleReducedMotion,
    /// Toggle Focus (Do Not Disturb) notification suppression (`RUSTYOS-053`).
    ToggleFocus,
    /// Trigger immediate background telemetry refresh.
    RefreshMetrics,
    /// Toggle Bluetooth adapter power via BlueZ (`RUSTYOS-027`).
    ToggleBluetooth,
    /// Engage desktop Lock Screen from Quick Settings (`RUSTYOS-027`).
    LockSession,
    /// Click inside popover frame (absorb event, keep open).
    InsidePopover,
    /// Click outside popover (dismiss Quick Settings).
    OutsidePopover,
}

pub(super) fn quick_settings_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let pop_w = 316.min((width - 24).max(240));
    let pop_h = 456.min((height - TOP_BAR_HEIGHT - 20).max(240));
    let pop_x = width - pop_w - 12;
    let pop_y = TOP_BAR_HEIGHT + 10;
    (pop_x, pop_y, pop_w, pop_h)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct QuickSettingsLayout {
    pub(super) px: i32,
    pub(super) py: i32,
    pub(super) pw: i32,
    pub(super) ph: i32,
    pub(super) wifi_pill: Rectangle<i32, smithay::utils::Physical>,
    pub(super) bt_pill: Rectangle<i32, smithay::utils::Physical>,
    pub(super) now_playing_card: Rectangle<i32, smithay::utils::Physical>,
    pub(super) media_prev_btn: Rectangle<i32, smithay::utils::Physical>,
    pub(super) media_play_btn: Rectangle<i32, smithay::utils::Physical>,
    pub(super) media_next_btn: Rectangle<i32, smithay::utils::Physical>,
    pub(super) power_pill: Rectangle<i32, smithay::utils::Physical>,
    pub(super) layout_btn: Rectangle<i32, smithay::utils::Physical>,
    pub(super) overview_btn: Rectangle<i32, smithay::utils::Physical>,
    pub(super) motion_btn: Rectangle<i32, smithay::utils::Physical>,
    pub(super) transparency_btn: Rectangle<i32, smithay::utils::Physical>,
    pub(super) focus_pill: Rectangle<i32, smithay::utils::Physical>,
    pub(super) display_card: Rectangle<i32, smithay::utils::Physical>,
    pub(super) display_track: Rectangle<i32, smithay::utils::Physical>,
    pub(super) sound_card: Rectangle<i32, smithay::utils::Physical>,
    pub(super) sound_track: Rectangle<i32, smithay::utils::Physical>,
    pub(super) sound_mute_zone: Rectangle<i32, smithay::utils::Physical>,
    pub(super) sound_left_mute_zone: Rectangle<i32, smithay::utils::Physical>,
    pub(super) lock_btn: Rectangle<i32, smithay::utils::Physical>,
}

pub(super) fn quick_settings_layout(width: i32, height: i32, anim_dy: i32) -> QuickSettingsLayout {
    let (px, base_py, pw, ph) = quick_settings_rect(width, height);
    let py = base_py + anim_dy;
    let gap = 12;
    let col_w = (pw - gap) / 2;
    let right_w = pw - col_w - gap;
    let left_x = px;
    let right_x = px + col_w + gap;
    let btn_d = 66.min((col_w - 8) / 2);

    let y_bt = py + 64;
    let y_row3 = py + 130;
    let y_row4 = y_row3 + 78;
    let y_disp = y_row4 + 78;
    let y_snd = y_disp + 70;
    let y_lock = y_snd + 70;

    let media_btn_y = y_bt + 10;
    let media_btn_w = ((right_w - 16) / 3).max(28);
    let lock_w = 118.min(pw);

    QuickSettingsLayout {
        px,
        py,
        pw,
        ph,
        wifi_pill: Rectangle::new(Point::from((left_x, py)), Size::from((col_w, 54))),
        bt_pill: Rectangle::new(Point::from((left_x, y_bt)), Size::from((col_w, 54))),
        now_playing_card: Rectangle::new(Point::from((right_x, py)), Size::from((right_w, 118))),
        media_prev_btn: Rectangle::new(
            Point::from((right_x + 8, media_btn_y)),
            Size::from((media_btn_w, 36)),
        ),
        media_play_btn: Rectangle::new(
            Point::from((right_x + 8 + media_btn_w, media_btn_y)),
            Size::from((media_btn_w, 36)),
        ),
        media_next_btn: Rectangle::new(
            Point::from((right_x + 8 + media_btn_w * 2, media_btn_y)),
            Size::from((media_btn_w, 36)),
        ),
        power_pill: Rectangle::new(Point::from((left_x, y_row3 + 3)), Size::from((col_w, 60))),
        layout_btn: Rectangle::new(Point::from((right_x, y_row3)), Size::from((btn_d, btn_d))),
        overview_btn: Rectangle::new(
            Point::from((right_x + right_w - btn_d, y_row3)),
            Size::from((btn_d, btn_d)),
        ),
        motion_btn: Rectangle::new(Point::from((left_x, y_row4)), Size::from((btn_d, btn_d))),
        transparency_btn: Rectangle::new(
            Point::from((left_x + col_w - btn_d, y_row4)),
            Size::from((btn_d, btn_d)),
        ),
        focus_pill: Rectangle::new(
            Point::from((right_x, y_row4 + 3)),
            Size::from((right_w, 60)),
        ),
        display_card: Rectangle::new(Point::from((px, y_disp)), Size::from((pw, 58))),
        display_track: Rectangle::new(
            Point::from((px + 34, y_disp + 31)),
            Size::from(((pw - 68).max(24), 20)),
        ),
        sound_card: Rectangle::new(Point::from((px, y_snd)), Size::from((pw, 58))),
        sound_track: Rectangle::new(
            Point::from((px + 34, y_snd + 31)),
            Size::from(((pw - 106).max(24), 20)),
        ),
        sound_mute_zone: Rectangle::new(
            Point::from((px + pw - 39, y_snd + 23)),
            Size::from((30, 30)),
        ),
        sound_left_mute_zone: Rectangle::new(
            Point::from((px + 8, y_snd + 28)),
            Size::from((24, 24)),
        ),
        lock_btn: Rectangle::new(
            Point::from((px + (pw - lock_w) / 2, y_lock)),
            Size::from((lock_w, 30)),
        ),
    }
}

/// Evaluates pointer clicks against floating Control Center islands (`RUSTYOS-053`).
#[must_use]
pub fn check_quick_settings_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> QuickSettingsHit {
    let layout = quick_settings_layout(screen_size.w, screen_size.h, 0);
    let pop_rect = Rectangle::new(
        Point::from((layout.px, layout.py)),
        Size::from((layout.pw, layout.ph)),
    );
    if !rect_contains(pop_rect, x, y) {
        return QuickSettingsHit::OutsidePopover;
    }

    if rounded_rect_contains(layout.wifi_pill, layout.wifi_pill.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleWifi;
    }
    if rounded_rect_contains(layout.bt_pill, layout.bt_pill.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleBluetooth;
    }
    if rect_contains(layout.media_prev_btn, x, y) {
        return QuickSettingsHit::MediaPrev;
    }
    if rect_contains(layout.media_play_btn, x, y) {
        return QuickSettingsHit::MediaPlayPause;
    }
    if rect_contains(layout.media_next_btn, x, y) {
        return QuickSettingsHit::MediaNext;
    }
    if rounded_rect_contains(layout.power_pill, layout.power_pill.size.h / 2, x, y) {
        return QuickSettingsHit::RefreshMetrics;
    }
    if rounded_rect_contains(layout.layout_btn, layout.layout_btn.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleLayout;
    }
    if rounded_rect_contains(layout.overview_btn, layout.overview_btn.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleOverview;
    }
    if rounded_rect_contains(layout.motion_btn, layout.motion_btn.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleReducedMotion;
    }
    if rounded_rect_contains(
        layout.transparency_btn,
        layout.transparency_btn.size.h / 2,
        x,
        y,
    ) {
        return QuickSettingsHit::ToggleReduceTransparency;
    }
    if rounded_rect_contains(layout.focus_pill, layout.focus_pill.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleFocus;
    }
    if rect_contains(layout.display_track, x, y) {
        return QuickSettingsHit::SetDisplayPercent(quick_settings_slider_percent(
            screen_size,
            QuickSettingsSliderDrag::Display,
            x,
        ));
    }
    if rounded_rect_contains(
        layout.sound_mute_zone,
        layout.sound_mute_zone.size.h / 2,
        x,
        y,
    ) || rect_contains(layout.sound_left_mute_zone, x, y)
    {
        return QuickSettingsHit::ToggleMute;
    }
    if rect_contains(layout.sound_track, x, y) {
        return QuickSettingsHit::SetVolumePercent(quick_settings_slider_percent(
            screen_size,
            QuickSettingsSliderDrag::Sound,
            x,
        ));
    }
    if rounded_rect_contains(layout.lock_btn, layout.lock_btn.size.h / 2, x, y) {
        return QuickSettingsHit::LockSession;
    }

    QuickSettingsHit::InsidePopover
}

/// Identifies whether `(x, y)` initiated a continuous slider drag inside Control Center (`RUSTYOS-051`).
#[must_use]
pub fn check_quick_settings_slider_drag(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> Option<QuickSettingsSliderDrag> {
    let layout = quick_settings_layout(screen_size.w, screen_size.h, 0);
    if rect_contains(layout.display_track, x, y) {
        Some(QuickSettingsSliderDrag::Display)
    } else if rect_contains(layout.sound_track, x, y)
        && !rounded_rect_contains(
            layout.sound_mute_zone,
            layout.sound_mute_zone.size.h / 2,
            x,
            y,
        )
        && !rect_contains(layout.sound_left_mute_zone, x, y)
    {
        Some(QuickSettingsSliderDrag::Sound)
    } else {
        None
    }
}

/// Maps pointer `x` along an active Control Center slider track to `0..=100` (`RUSTYOS-053`).
#[must_use]
pub fn quick_settings_slider_percent(
    screen_size: Size<i32, smithay::utils::Physical>,
    drag: QuickSettingsSliderDrag,
    x: f64,
) -> u8 {
    let layout = quick_settings_layout(screen_size.w, screen_size.h, 0);
    let track = match drag {
        QuickSettingsSliderDrag::Sound => layout.sound_track,
        QuickSettingsSliderDrag::Display => layout.display_track,
    };
    let track_x = f64::from(track.loc.x);
    let track_w = f64::from(track.size.w.max(1));
    let norm = ((x - track_x) / track_w).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    ((norm * 100.0).round() as u8).clamp(0, 100)
}

pub(super) fn render_quick_settings_slider_solid(
    elements: &mut Vec<SolidColorRenderElement>,
    card: Rectangle<i32, smithay::utils::Physical>,
    track: Rectangle<i32, smithay::utils::Physical>,
    norm: f32,
    fill_color: Color32F,
) {
    elements.push(make_solid_element(card, COLOR_SURFACE_OVERLAY));
    elements.push(make_solid_element(track, COLOR_SURFACE));
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let fill_w = ((track.size.w as f32) * norm.clamp(0.0, 1.0)).round() as i32;
    if fill_w > 0 {
        elements.push(make_solid_element(
            Rectangle::new(track.loc, Size::from((fill_w, track.size.h))),
            fill_color,
        ));
    }
}

pub(super) fn render_quick_settings_popover(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    qs: &QuickSettingsRenderState<'_>,
) {
    let layout = quick_settings_layout(width, height, 0);
    let solid_tiles = [
        (layout.wifi_pill, qs.net_connected || qs.wifi_enabled),
        (layout.bt_pill, qs.bt_available && qs.bt_powered),
        (layout.now_playing_card, false),
        (layout.power_pill, false),
        (layout.layout_btn, qs.layout_mode == "master-stack"),
        (layout.overview_btn, false),
        (layout.motion_btn, qs.reduced_motion),
        (layout.transparency_btn, qs.reduce_transparency),
        (layout.focus_pill, qs.focus_mode),
        (layout.lock_btn, false),
    ];
    for (rect, active) in solid_tiles {
        elements.push(make_solid_element(
            rect,
            if active {
                COLOR_ACCENT_MUTED
            } else {
                COLOR_SURFACE_OVERLAY
            },
        ));
    }

    let display_norm = if qs.brightness_available {
        f32::from(qs.brightness_percent.min(100)) / 100.0
    } else {
        (crate::config::clamp_glass_opacity(qs.glass_opacity) - crate::config::MIN_GLASS_OPACITY)
            / (1.0 - crate::config::MIN_GLASS_OPACITY)
    };
    render_quick_settings_slider_solid(
        elements,
        layout.display_card,
        layout.display_track,
        display_norm,
        COLOR_ACCENT_HOVER,
    );
    render_quick_settings_slider_solid(
        elements,
        layout.sound_card,
        layout.sound_track,
        f32::from(qs.volume_percent.min(100)) / 100.0,
        if qs.audio_muted {
            COLOR_SUBTLE
        } else {
            COLOR_ACCENT
        },
    );
}

pub(super) const QS_TEXT_BRIGHT: [u8; 4] = [255, 253, 255, 255];
pub(super) const QS_TEXT_MUTED: [u8; 4] = [226, 222, 238, 240];

pub(super) fn concise_wifi_subtitle(label: &str, connected: bool, wifi_enabled: bool) -> String {
    let cleaned = label
        .trim()
        .strip_prefix("Wi-Fi:")
        .map_or_else(|| label.trim(), str::trim);
    if !wifi_enabled && !connected {
        "Off".to_owned()
    } else if !connected {
        "Not Connected".to_owned()
    } else if cleaned.eq_ignore_ascii_case("Wi-Fi Connected") || cleaned.is_empty() {
        "Connected".to_owned()
    } else {
        truncate_ascii(cleaned, 11)
    }
}

pub(super) fn concise_bt_subtitle(bt_label: &str, bt_available: bool, bt_powered: bool) -> String {
    if !bt_available {
        return "Unavailable".to_owned();
    }
    if !bt_powered {
        return "Off".to_owned();
    }
    let cleaned = bt_label
        .trim()
        .strip_prefix("BT:")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("On");
    truncate_ascii(cleaned, 11)
}

pub(super) fn render_control_center_capsule_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    pill: Rectangle<i32, smithay::utils::Physical>,
    title: &str,
    sub: &str,
    bright: bool,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let top_y = pill.loc.y + (pill.size.h - 29) / 2;
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        title,
        crate::text::FontWeight::Bold,
        12.0,
        if bright {
            QS_TEXT_BRIGHT
        } else {
            QS_TEXT_MUTED
        },
        Point::from((pill.loc.x + 54, top_y)),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label(
        renderer,
        sub,
        10.5,
        QS_TEXT_MUTED,
        Point::from((pill.loc.x + 54, top_y + 16)),
    ) {
        elements.push(el);
    }
}

pub(super) fn render_cc_secondary_labels(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let media_label = if qs.media_active && !qs.media_title.trim().is_empty() {
        truncate_ascii(qs.media_title.trim(), 15)
    } else {
        "Not Playing".to_owned()
    };
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        &media_label,
        crate::text::FontWeight::Bold,
        12.0,
        QS_TEXT_BRIGHT,
        Point::from((
            layout.now_playing_card.loc.x + 14,
            layout.now_playing_card.loc.y + 53,
        )),
    ) {
        elements.push(el);
    }

    if qs.focus_mode {
        render_control_center_capsule_text(
            text_cache,
            renderer,
            layout.focus_pill,
            "Focus",
            "On",
            true,
            elements,
        );
    } else if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "Focus",
        crate::text::FontWeight::Bold,
        12.0,
        QS_TEXT_BRIGHT,
        Point::from((
            layout.focus_pill.loc.x + 56,
            layout.focus_pill.loc.y + (layout.focus_pill.size.h - 15) / 2,
        )),
    ) {
        elements.push(el);
    }

    for (card, title) in [
        (layout.display_card, "Display"),
        (layout.sound_card, "Sound"),
    ] {
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            title,
            crate::text::FontWeight::Bold,
            12.0,
            QS_TEXT_BRIGHT,
            Point::from((card.loc.x + 14, card.loc.y + 9)),
        ) {
            elements.push(el);
        }
    }

    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        "Lock Screen",
        crate::text::FontWeight::Bold,
        11.0,
        QS_TEXT_BRIGHT,
        Rectangle::new(
            Point::from((layout.lock_btn.loc.x, layout.lock_btn.loc.y)),
            Size::from((layout.lock_btn.size.w, layout.lock_btn.size.h)),
        ),
    ) {
        elements.push(el);
    }
}

pub(super) fn render_quick_settings_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    width: i32,
    height: i32,
    qs: &QuickSettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = quick_settings_layout(width, height, anim_offset(qs.progress, -16.0));
    let _ = qs.frame_p50_us;
    let _ = qs.window_count;

    let wifi_sub = concise_wifi_subtitle(qs.network_label, qs.net_connected, qs.wifi_enabled);
    render_control_center_capsule_text(
        text_cache,
        renderer,
        layout.wifi_pill,
        "Wi-Fi",
        &wifi_sub,
        true,
        elements,
    );

    let bt_sub = concise_bt_subtitle(qs.bt_label, qs.bt_available, qs.bt_powered);
    render_control_center_capsule_text(
        text_cache,
        renderer,
        layout.bt_pill,
        "Bluetooth",
        &bt_sub,
        qs.bt_available,
        elements,
    );

    let power_sub = if qs.has_battery {
        if qs.ac_online {
            format!("{}% • AC", qs.battery_percent)
        } else {
            format!("{}%", qs.battery_percent)
        }
    } else {
        "AC Power".to_owned()
    };
    render_control_center_capsule_text(
        text_cache,
        renderer,
        layout.power_pill,
        "Power",
        &power_sub,
        true,
        elements,
    );

    render_cc_secondary_labels(text_cache, renderer, &layout, qs, elements);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_cc_capsule_icon(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    pill: Rectangle<i32, smithay::utils::Physical>,
    active: bool,
    glyph: crate::surfaces::UiGlyphKind,
    active_glyph_rgba: [u8; 4],
    inactive_glyph_rgba: [u8; 4],
) {
    let circle_d = 36;
    let circle_x = pill.loc.x + 10;
    let circle_y = pill.loc.y + (pill.size.h - circle_d) / 2;
    let glyph_size = 17;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        glyph,
        glyph_size,
        if active {
            active_glyph_rgba
        } else {
            inactive_glyph_rgba
        },
        Point::from((
            circle_x + (circle_d - glyph_size) / 2,
            circle_y + (circle_d - glyph_size) / 2,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let circle_fill = if active {
        [252, 252, 255, 252]
    } else {
        [255, 255, 255, 42]
    };
    let circle_border = if active {
        [255, 255, 255, 255]
    } else {
        [255, 255, 255, 62]
    };
    if let Some(el) = shape_cache.circle_element(
        renderer,
        circle_d,
        circle_fill,
        Some(circle_border),
        Point::from((circle_x, circle_y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
}

pub(super) fn push_cc_now_playing_contents(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
) {
    let card = layout.now_playing_card;
    let art_rect = Rectangle::new(
        Point::from((card.loc.x + 14, card.loc.y + 12)),
        Size::from((36, 36)),
    );
    let note_rgba = if qs.media_active {
        [252, 250, 255, 245]
    } else {
        [244, 240, 255, 130]
    };
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::MusicNote,
        16,
        note_rgba,
        Point::from((art_rect.loc.x + 10, art_rect.loc.y + 10)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let art_bg = if qs.media_active {
        [108, 90, 224, 215]
    } else {
        [255, 255, 255, 38]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        art_rect,
        9,
        art_bg,
        Some([255, 255, 255, 44]),
    );

    let play_kind = if qs.media_playing {
        crate::surfaces::UiGlyphKind::MediaPause
    } else {
        crate::surfaces::UiGlyphKind::MediaPlay
    };
    let transport_tint = if qs.media_active {
        [255, 252, 255, 248]
    } else {
        [240, 236, 248, 200]
    };
    for (btn_rect, kind, g_size) in [
        (
            layout.media_prev_btn,
            crate::surfaces::UiGlyphKind::MediaPrev,
            17,
        ),
        (layout.media_play_btn, play_kind, 20),
        (
            layout.media_next_btn,
            crate::surfaces::UiGlyphKind::MediaNext,
            17,
        ),
    ] {
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            kind,
            g_size,
            transport_tint,
            Point::from((
                btn_rect.loc.x + (btn_rect.size.w - g_size) / 2,
                btn_rect.loc.y + (btn_rect.size.h - g_size) / 2,
            )),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
}

pub(super) fn push_cc_thin_slider(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    track: Rectangle<i32, smithay::utils::Physical>,
    norm: f32,
    active_fill: bool,
) {
    let bar_h = 6;
    let bar_y = track.loc.y + (track.size.h - bar_h) / 2;
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let fill_w = ((track.size.w as f32) * norm.clamp(0.0, 1.0))
        .round()
        .clamp(6.0, track.size.w as f32) as i32;
    let fill_rgba = if active_fill {
        [255, 253, 255, 245]
    } else {
        [190, 184, 204, 185]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        Rectangle::new(
            Point::from((track.loc.x, bar_y)),
            Size::from((fill_w, bar_h)),
        ),
        bar_h / 2,
        fill_rgba,
        None,
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        Rectangle::new(
            Point::from((track.loc.x, bar_y)),
            Size::from((track.size.w, bar_h)),
        ),
        bar_h / 2,
        [255, 255, 255, 58],
        None,
    );
}

pub(super) fn push_cc_display_slider(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
) {
    let white = [255, 253, 255, 245];
    let disp_cy = layout.display_track.loc.y + layout.display_track.size.h / 2;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Sun,
        12,
        white,
        Point::from((layout.display_card.loc.x + 14, disp_cy - 6)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Sun,
        16,
        white,
        Point::from((
            layout.display_card.loc.x + layout.display_card.size.w - 28,
            disp_cy - 8,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let disp_norm = if qs.brightness_available {
        f32::from(qs.brightness_percent.min(100)) / 100.0
    } else {
        (crate::config::clamp_glass_opacity(qs.glass_opacity) - crate::config::MIN_GLASS_OPACITY)
            / (1.0 - crate::config::MIN_GLASS_OPACITY)
    };
    push_cc_thin_slider(
        out,
        shape_cache,
        renderer,
        layout.display_track,
        disp_norm,
        true,
    );
}

pub(super) fn push_cc_sound_slider(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
) {
    let white = [255, 253, 255, 245];
    let snd_cy = layout.sound_track.loc.y + layout.sound_track.size.h / 2;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: 0,
            muted: false,
        },
        13,
        white,
        Point::from((layout.sound_card.loc.x + 13, snd_cy - 6)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: 90,
            muted: false,
        },
        15,
        white,
        Point::from((
            layout.sound_card.loc.x + layout.sound_card.size.w - 64,
            snd_cy - 7,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let mute_fg = if qs.audio_muted {
        [248, 86, 86, 255]
    } else {
        white
    };
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: qs.volume_percent,
            muted: qs.audio_muted,
        },
        15,
        mute_fg,
        Point::from((
            layout.sound_mute_zone.loc.x + (layout.sound_mute_zone.size.w - 15) / 2,
            layout.sound_mute_zone.loc.y + (layout.sound_mute_zone.size.h - 15) / 2,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let mute_circle_bg = if qs.audio_muted {
        [252, 252, 255, 245]
    } else {
        [255, 255, 255, 42]
    };
    if let Some(el) = shape_cache.circle_element(
        renderer,
        layout.sound_mute_zone.size.w,
        mute_circle_bg,
        Some([255, 255, 255, 68]),
        Point::from((layout.sound_mute_zone.loc.x, layout.sound_mute_zone.loc.y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let vol_norm = f32::from(qs.volume_percent.min(100)) / 100.0;
    push_cc_thin_slider(
        out,
        shape_cache,
        renderer,
        layout.sound_track,
        vol_norm,
        !qs.audio_muted,
    );
}

pub(super) fn push_cc_round_toggle_buttons(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
) {
    let white = [255, 252, 255, 255];
    let is_tile = qs.layout_mode == "master-stack";
    for (btn_rect, active, kind) in [
        (
            layout.layout_btn,
            is_tile,
            crate::surfaces::UiGlyphKind::LayoutSplit,
        ),
        (
            layout.overview_btn,
            false,
            crate::surfaces::UiGlyphKind::OverviewSpaces,
        ),
        (
            layout.motion_btn,
            qs.reduced_motion,
            crate::surfaces::UiGlyphKind::Motion,
        ),
        (
            layout.transparency_btn,
            qs.reduce_transparency,
            crate::surfaces::UiGlyphKind::Transparency,
        ),
    ] {
        let g_size = 21;
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            kind,
            g_size,
            white,
            Point::from((
                btn_rect.loc.x + (btn_rect.size.w - g_size) / 2,
                btn_rect.loc.y + (btn_rect.size.h - g_size) / 2,
            )),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if active
            && let Some(el) = shape_cache.circle_element(
                renderer,
                btn_rect.size.w,
                [88, 134, 252, 118],
                Some([255, 255, 255, 110]),
                Point::from((btn_rect.loc.x, btn_rect.loc.y)),
            )
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
}

pub(super) fn render_quick_settings_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    width: i32,
    height: i32,
    qs: &QuickSettingsRenderState<'_>,
) {
    let layout = quick_settings_layout(width, height, anim_offset(qs.progress, -16.0));
    let white = [255, 252, 255, 255];
    let blue_active = [24, 128, 250, 255];

    let wifi_active = qs.net_connected || qs.wifi_enabled;
    push_cc_capsule_icon(
        out,
        shape_cache,
        renderer,
        layout.wifi_pill,
        wifi_active,
        crate::surfaces::UiGlyphKind::Wifi {
            connected: qs.net_connected,
            signal_percent: qs.wifi_signal_percent,
        },
        blue_active,
        white,
    );

    let bt_active = qs.bt_available && qs.bt_powered;
    push_cc_capsule_icon(
        out,
        shape_cache,
        renderer,
        layout.bt_pill,
        bt_active,
        crate::surfaces::UiGlyphKind::Bluetooth,
        blue_active,
        if qs.bt_available {
            white
        } else {
            [210, 204, 224, 170]
        },
    );

    let power_healthy = qs.ac_online || qs.battery_charging || qs.battery_percent > 20;
    let power_icon_active = if qs.ac_online || qs.battery_charging {
        [22, 163, 74, 255]
    } else if power_healthy {
        blue_active
    } else {
        [239, 68, 68, 255]
    };
    push_cc_capsule_icon(
        out,
        shape_cache,
        renderer,
        layout.power_pill,
        power_healthy,
        crate::surfaces::UiGlyphKind::Battery {
            percent: qs.battery_percent,
            charging: qs.ac_online || qs.battery_charging,
        },
        power_icon_active,
        white,
    );

    push_cc_now_playing_contents(out, shape_cache, renderer, &layout, qs);
    push_cc_round_toggle_buttons(out, shape_cache, renderer, &layout, qs);

    push_cc_capsule_icon(
        out,
        shape_cache,
        renderer,
        layout.focus_pill,
        qs.focus_mode,
        crate::surfaces::UiGlyphKind::Moon,
        [106, 84, 212, 255],
        white,
    );

    push_cc_display_slider(out, shape_cache, renderer, &layout, qs);
    push_cc_sound_slider(out, shape_cache, renderer, &layout, qs);

    for (rect, radius) in [
        (layout.wifi_pill, layout.wifi_pill.size.h / 2),
        (layout.bt_pill, layout.bt_pill.size.h / 2),
        (layout.now_playing_card, 24),
        (layout.power_pill, layout.power_pill.size.h / 2),
        (layout.layout_btn, layout.layout_btn.size.h / 2),
        (layout.overview_btn, layout.overview_btn.size.h / 2),
        (layout.motion_btn, layout.motion_btn.size.h / 2),
        (layout.transparency_btn, layout.transparency_btn.size.h / 2),
        (layout.focus_pill, layout.focus_pill.size.h / 2),
        (layout.display_card, 22),
        (layout.sound_card, 22),
        (layout.lock_btn, layout.lock_btn.size.h / 2),
    ] {
        push_liquid_glass_panel(
            out,
            shape_cache,
            renderer,
            rect,
            radius,
            crate::surfaces::GlassPanelStyle::ControlModule,
            qs.glass_opacity,
            qs.reduce_transparency,
            Some((10, 26)),
        );
    }
}
