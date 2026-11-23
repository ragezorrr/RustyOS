//! Compact `TopBar` Liquid Glass popovers for Volume, Battery, and Bluetooth (`RUSTYOS-141`, ADR-0082).

use rusty_services::{
    AudioStatus, BatteryState, BluetoothStatus, PowerProfile, PowerStatus,
    format_battery_time_remaining,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Physical, Point, Rectangle, Size};

use crate::surfaces::GlassPanelStyle;

use super::CompositorRenderElement;
use super::tokens::{
    MENU_POPOVER_RADIUS, TOP_BAR_HEIGHT, ThemePalette, make_solid_element,
    push_liquid_glass_panel_elevation, push_rounded_glass_box, truncate_ascii,
};

const POPOVER_WIDTH: i32 = 264;
const HOVER_PILL_RADIUS: i32 = rusty_ui::menu::HOVER_PILL_RADIUS;

/// Which compact `TopBar` status popover is currently open (`RUSTYOS-141`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarQuickPopoverKind {
    /// Master audio volume + mute + Sound Settings shortcut.
    Volume,
    /// Battery status + percentage toggle + power profile selector + Battery Settings shortcut.
    Battery,
    /// Bluetooth power toggle + status summary + Bluetooth Settings shortcut.
    Bluetooth,
}

/// Interactive hit regions inside [`TopBarQuickPopoverKind`] (`RUSTYOS-141`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarQuickPopoverHit {
    /// Mute / unmute master audio output.
    ToggleMute,
    /// Step master volume down (`-10%`).
    VolumeDown,
    /// Step master volume up (`+10%`).
    VolumeUp,
    /// Select audio output sink at index `0..N` (`RUSTYOS-156`).
    SelectAudioOutput(usize),
    /// Toggle numeric battery percentage beside the `TopBar` battery glyph.
    ToggleBatteryPercentage,
    /// Switch active system power profile.
    SetPowerProfile(PowerProfile),
    /// Toggle Bluetooth adapter power.
    ToggleBluetooth,
    /// Open the corresponding `rusty-settings` page.
    OpenSettings,
    /// Click landed on passive background inside the popover.
    InsidePopover,
    /// Click landed outside the popover (dismisses it).
    OutsidePopover,
}

/// Live view state driving [`super::ShellOverlayView::TopBarQuickPopover`].
#[derive(Debug, Clone, Copy)]
pub struct TopBarQuickPopoverState<'a> {
    /// Active popover kind (`Volume`, `Battery`, or `Bluetooth`).
    pub kind: TopBarQuickPopoverKind,
    /// Right anchor X coordinate in `TopBar` coordinates.
    pub anchor_right_x: i32,
    /// Global shell glass opacity multiplier (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels mode.
    pub reduce_transparency: bool,
    /// Live audio telemetry.
    pub audio: &'a AudioStatus,
    /// Live battery / power telemetry.
    pub power: &'a PowerStatus,
    /// Live Bluetooth telemetry.
    pub bluetooth: &'a BluetoothStatus,
    /// Whether numeric battery percentage is currently shown in `TopBar`.
    pub show_battery_percentage: bool,
    /// Currently hovered hit region inside the popover.
    pub hovered_hit: Option<TopBarQuickPopoverHit>,
}

/// Precomputed physical coordinates for a compact `TopBar` quick popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBarQuickPopoverLayout {
    /// Outer popover bounding rectangle.
    pub panel_rect: Rectangle<i32, Physical>,
    /// Interactive action rows (`(hit, rect, label, active)`).
    pub rows: Vec<(
        TopBarQuickPopoverHit,
        Rectangle<i32, Physical>,
        String,
        bool,
    )>,
    /// Header title text.
    pub title: String,
    /// Secondary subtitle / status text.
    pub subtitle: String,
}

/// Computes deterministic layout for the active compact `TopBar` quick popover.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn compute_topbar_quick_popover_layout(
    screen_w: i32,
    state: &TopBarQuickPopoverState<'_>,
) -> TopBarQuickPopoverLayout {
    let x = (state.anchor_right_x - POPOVER_WIDTH).clamp(8, (screen_w - POPOVER_WIDTH - 8).max(8));
    let y = TOP_BAR_HEIGHT + 6;

    let (title, subtitle, specs): (String, String, Vec<(TopBarQuickPopoverHit, String, bool)>) =
        match state.kind {
            TopBarQuickPopoverKind::Volume => {
                let status = if state.audio.muted {
                    format!(
                        "Muted • {}",
                        truncate_ascii(&state.audio.active_output_name, 22)
                    )
                } else {
                    format!(
                        "{}% • {}",
                        state.audio.volume_percent,
                        truncate_ascii(&state.audio.active_output_name, 22)
                    )
                };
                let mut rows = vec![
                    (
                        TopBarQuickPopoverHit::ToggleMute,
                        if state.audio.muted {
                            "Unmute Audio".to_owned()
                        } else {
                            "Mute Audio".to_owned()
                        },
                        state.audio.muted,
                    ),
                    (
                        TopBarQuickPopoverHit::VolumeDown,
                        "Volume -10%".to_owned(),
                        false,
                    ),
                    (
                        TopBarQuickPopoverHit::VolumeUp,
                        "Volume +10%".to_owned(),
                        false,
                    ),
                ];
                for (idx, dev) in state.audio.outputs.iter().take(6).enumerate() {
                    rows.push((
                        TopBarQuickPopoverHit::SelectAudioOutput(idx),
                        truncate_ascii(&dev.name, 28),
                        dev.is_default,
                    ));
                }
                rows.push((
                    TopBarQuickPopoverHit::OpenSettings,
                    "Sound Settings...".to_owned(),
                    false,
                ));
                ("Sound".to_owned(), status, rows)
            }
            TopBarQuickPopoverKind::Battery => {
                let status = format_battery_popover_status(state.power);
                (
                    "Battery".to_owned(),
                    status,
                    vec![
                        (
                            TopBarQuickPopoverHit::ToggleBatteryPercentage,
                            "Show Percentage in Menu Bar".to_owned(),
                            state.show_battery_percentage,
                        ),
                        (
                            TopBarQuickPopoverHit::SetPowerProfile(PowerProfile::LowPower),
                            "Power Saver".to_owned(),
                            false,
                        ),
                        (
                            TopBarQuickPopoverHit::SetPowerProfile(PowerProfile::Balanced),
                            "Balanced".to_owned(),
                            false,
                        ),
                        (
                            TopBarQuickPopoverHit::SetPowerProfile(PowerProfile::Performance),
                            "Performance".to_owned(),
                            false,
                        ),
                        (
                            TopBarQuickPopoverHit::OpenSettings,
                            "Battery Settings...".to_owned(),
                            false,
                        ),
                    ],
                )
            }
            TopBarQuickPopoverKind::Bluetooth => {
                let status = if !state.bluetooth.available {
                    "No Bluetooth Adapter".to_owned()
                } else if state.bluetooth.powered {
                    state
                        .bluetooth
                        .connected_device
                        .clone()
                        .unwrap_or_else(|| "On • Not Connected".to_owned())
                } else {
                    "Off".to_owned()
                };
                (
                    "Bluetooth".to_owned(),
                    status,
                    vec![
                        (
                            TopBarQuickPopoverHit::ToggleBluetooth,
                            if state.bluetooth.powered {
                                "Turn Bluetooth Off".to_owned()
                            } else {
                                "Turn Bluetooth On".to_owned()
                            },
                            state.bluetooth.powered,
                        ),
                        (
                            TopBarQuickPopoverHit::OpenSettings,
                            "Bluetooth Settings...".to_owned(),
                            false,
                        ),
                    ],
                )
            }
        };

    let row_h = 28;
    let header_h = 48;
    let pad_bottom = 10;
    let row_count = i32::try_from(specs.len()).unwrap_or(0);
    let panel_h = header_h + row_count * row_h + pad_bottom;
    let panel_rect = Rectangle::new(Point::from((x, y)), Size::from((POPOVER_WIDTH, panel_h)));

    let rows = specs
        .into_iter()
        .enumerate()
        .map(|(idx, (hit, label, active))| {
            let ry = y + header_h + i32::try_from(idx).unwrap_or(0) * row_h;
            let rect = Rectangle::new(
                Point::from((x + 6, ry)),
                Size::from((POPOVER_WIDTH - 12, row_h - 2)),
            );
            (hit, rect, label, active)
        })
        .collect();

    TopBarQuickPopoverLayout {
        panel_rect,
        rows,
        title,
        subtitle,
    }
}

/// Hit-tests pointer coordinates against the compact `TopBar` quick popover.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn hit_test_topbar_quick_popover(
    screen_w: i32,
    state: &TopBarQuickPopoverState<'_>,
    px: f64,
    py: f64,
) -> TopBarQuickPopoverHit {
    let layout = compute_topbar_quick_popover_layout(screen_w, state);
    let pt = Point::from((px.round() as i32, py.round() as i32));
    if !layout.panel_rect.contains(pt) {
        return TopBarQuickPopoverHit::OutsidePopover;
    }
    for (hit, rect, _, _) in &layout.rows {
        if rect.contains(pt) {
            return *hit;
        }
    }
    TopBarQuickPopoverHit::InsidePopover
}

/// Appends text labels for [`super::ShellOverlayView::TopBarQuickPopover`].
pub(super) fn render_topbar_quick_popover_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &TopBarQuickPopoverState<'_>,
    elements: &mut Vec<
        smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<GlesRenderer>,
    >,
) {
    let layout = compute_topbar_quick_popover_layout(screen_size.w, state);
    let pal = ThemePalette::active();
    let px = layout.panel_rect.loc.x;
    let py = layout.panel_rect.loc.y;

    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        &layout.title,
        crate::text::FontWeight::Bold,
        13.0,
        pal.popover_text_primary,
        Point::from((px + 14, py + 10)),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        &layout.subtitle,
        crate::text::FontWeight::Regular,
        11.0,
        pal.popover_text_secondary,
        Point::from((px + 14, py + 26)),
    ) {
        elements.push(el);
    }

    for (hit, rect, label, active) in &layout.rows {
        let hovered = state.hovered_hit == Some(*hit);
        let color = if hovered {
            [255, 255, 255, 255]
        } else {
            pal.popover_text_primary
        };
        let display = if *active {
            format!("✓  {label}")
        } else {
            label.clone()
        };
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            &display,
            crate::text::FontWeight::Regular,
            12.5,
            color,
            Point::from((rect.loc.x + 10, rect.loc.y + 6)),
        ) {
            elements.push(el);
        }
    }
}

/// Emits Liquid Glass background, hover pills, and dividers for the quick popover.
pub(super) fn render_topbar_quick_popover_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &TopBarQuickPopoverState<'_>,
) {
    let layout = compute_topbar_quick_popover_layout(screen_size.w, state);
    let pal = ThemePalette::active();

    for (hit, rect, _, active) in &layout.rows {
        let hovered = state.hovered_hit == Some(*hit);
        if hovered || *active {
            let fill = if hovered {
                pal.accent_rgba
            } else {
                pal.control_pill_off_rgba
            };
            push_rounded_glass_box(
                out,
                shape_cache,
                renderer,
                *rect,
                HOVER_PILL_RADIUS,
                fill,
                None,
            );
        }
    }

    let sep_rect = Rectangle::new(
        Point::from((layout.panel_rect.loc.x + 12, layout.panel_rect.loc.y + 44)),
        Size::from(((layout.panel_rect.size.w - 24).max(12), 1)),
    );
    out.push(CompositorRenderElement::Solid(make_solid_element(
        sep_rect,
        pal.popover_separator,
    )));

    push_liquid_glass_panel_elevation(
        out,
        shape_cache,
        renderer,
        layout.panel_rect,
        MENU_POPOVER_RADIUS,
        GlassPanelStyle::Popover,
        state.glass_opacity,
        state.reduce_transparency,
        rusty_ui::shadow::ShadowElevation::MenuOrTooltip,
    );
}

/// Formats the compact Battery popover subtitle including time remaining when available (`RUSTYOS-164`).
#[must_use]
pub(crate) fn format_battery_popover_status(power: &PowerStatus) -> String {
    if !power.has_battery {
        return "AC Power".to_owned();
    }
    match power.state {
        BatteryState::Charging => power.time_remaining_minutes.map_or_else(
            || format!("{}% • Charging", power.percentage),
            |mins| {
                format!(
                    "{}% • Charging • {}",
                    power.percentage,
                    format_battery_time_remaining(mins, true)
                )
            },
        ),
        BatteryState::Discharging => power.time_remaining_minutes.map_or_else(
            || format!("{}% • On Battery", power.percentage),
            |mins| {
                format!(
                    "{}% • {}",
                    power.percentage,
                    format_battery_time_remaining(mins, false)
                )
            },
        ),
        BatteryState::Full | BatteryState::Unknown => format!(
            "{}% • {}",
            power.percentage,
            if power.ac_online {
                "Plugged In"
            } else {
                "On Battery"
            }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_popover_status_formats_time_remaining_and_fallback_states() {
        let discharging = PowerStatus {
            percentage: 82,
            state: BatteryState::Discharging,
            ac_online: false,
            has_battery: true,
            time_remaining_minutes: Some(135),
        };
        assert_eq!(
            format_battery_popover_status(&discharging),
            "82% • 2h 15m left"
        );

        let charging = PowerStatus {
            percentage: 64,
            state: BatteryState::Charging,
            ac_online: true,
            has_battery: true,
            time_remaining_minutes: Some(45),
        };
        assert_eq!(
            format_battery_popover_status(&charging),
            "64% • Charging • 45m to full"
        );

        let ac_full = PowerStatus {
            percentage: 100,
            state: BatteryState::Full,
            ac_online: true,
            has_battery: true,
            time_remaining_minutes: None,
        };
        assert_eq!(format_battery_popover_status(&ac_full), "100% • Plugged In");

        let desktop = PowerStatus::default();
        assert_eq!(format_battery_popover_status(&desktop), "AC Power");
    }

    #[test]
    fn volume_and_battery_popovers_layout_and_hit_test_cleanly() {
        let audio = AudioStatus {
            volume_percent: 60,
            muted: false,
            ..AudioStatus::default()
        };
        let power = PowerStatus::default();
        let bt = BluetoothStatus::default();
        let state = TopBarQuickPopoverState {
            kind: TopBarQuickPopoverKind::Volume,
            anchor_right_x: 1180,
            glass_opacity: 0.78,
            reduce_transparency: false,
            audio: &audio,
            power: &power,
            bluetooth: &bt,
            show_battery_percentage: false,
            hovered_hit: None,
        };
        let layout = compute_topbar_quick_popover_layout(1280, &state);
        assert_eq!(layout.title, "Sound");
        assert_eq!(layout.rows.len(), 4);
        let first = layout.rows[0].1;
        let hit = hit_test_topbar_quick_popover(
            1280,
            &state,
            f64::from(first.loc.x + 10),
            f64::from(first.loc.y + 10),
        );
        assert_eq!(hit, TopBarQuickPopoverHit::ToggleMute);
        assert_eq!(
            hit_test_topbar_quick_popover(1280, &state, 10.0, 10.0),
            TopBarQuickPopoverHit::OutsidePopover
        );
    }
}
