//! Control sub-rect geometry and pointer hit-testing for `rusty-settings`.

#[allow(clippy::wildcard_imports)]
use super::*;

#[must_use]
pub(crate) fn switch_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 38, row.y + (row.h - 20) / 2, 38, 20)
}

#[must_use]
pub(crate) fn choice_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 124, row.y + (row.h - 26) / 2, 124, 26)
}

#[must_use]
pub(crate) fn stepper_rects(row: Rect) -> (Rect, Rect, Rect) {
    let plus = Rect::new(row.right() - 16 - 26, row.y + (row.h - 26) / 2, 26, 26);
    let val = Rect::new(plus.x - 62, plus.y, 58, 26);
    let minus = Rect::new(val.x - 30, plus.y, 26, 26);
    (minus, val, plus)
}

#[must_use]
pub(crate) fn dropdown_trigger_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 176, row.y + (row.h - 26) / 2, 176, 26)
}

#[must_use]
pub(crate) fn battery_tabs_inner_rect(row: Rect) -> Rect {
    let w = (row.w - 32).clamp(180, 320);
    Rect::new(row.x + (row.w - w) / 2, row.y + (row.h - 28) / 2, w, 28)
}

#[must_use]
pub(crate) fn bt_device_action_rects(row: Rect) -> (Rect, Rect) {
    let info_r = Rect::new(row.right() - 16 - 22, row.y + (row.h - 22) / 2, 22, 22);
    let action_r = Rect::new(info_r.x - 10 - 92, row.y + (row.h - 24) / 2, 92, 24);
    (action_r, info_r)
}

#[must_use]
pub(crate) fn about_more_info_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 104, row.y + (row.h - 28) / 2, 104, 28)
}

#[must_use]
pub(crate) fn shortcut_pill_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 16 - 144, row.y + (row.h - 26) / 2, 144, 26)
}

#[must_use]
pub(crate) fn hit_test(layout: &SettingsLayout, px: f64, py: f64) -> Option<SettingsHit> {
    if let Some(edge) = hit_test_resize_edge(layout.win_w, layout.win_h, px, py) {
        return Some(SettingsHit::ResizeEdge(edge));
    }
    if layout.traffic_close.contains_f64(px, py) {
        return Some(SettingsHit::TrafficClose);
    }
    if layout.traffic_min.contains_f64(px, py) {
        return Some(SettingsHit::TrafficMinimize);
    }
    if layout.traffic_max.contains_f64(px, py) {
        return Some(SettingsHit::TrafficMaximize);
    }
    if layout.search_rect.contains_f64(px, py) {
        return Some(SettingsHit::SearchField);
    }
    if layout.nav_back.contains_f64(px, py) {
        return Some(SettingsHit::NavBack);
    }
    if layout.nav_forward.contains_f64(px, py) {
        return Some(SettingsHit::NavForward);
    }
    if py >= f64::from(layout.sidebar_viewport.y) {
        for item in &layout.sidebar_items {
            if item.rect.contains_f64(px, py) {
                return Some(SettingsHit::SidebarItem(item.page));
            }
        }
    }
    if py < f64::from(HEADER_HEIGHT) {
        return Some(SettingsHit::WindowDrag);
    }
    if let Some(thumb) = layout.page_scrollbar_thumb {
        let expanded = Rect::new(thumb.x - 4, thumb.y, thumb.w + 8, thumb.h);
        if expanded.contains_f64(px, py) {
            return Some(SettingsHit::ScrollbarThumb);
        }
    }
    if let Some(app_layout) = &layout.appearance
        && let Some(hit) = hit_test_appearance(app_layout, px, py)
    {
        return Some(hit);
    }
    if let Some(dp) = &layout.dock_positions {
        for &(idx, _, ring, _, _, _) in &dp.tiles {
            if ring.contains_f64(px, py) {
                return Some(SettingsHit::DockPosTile(idx));
            }
        }
    }
    for sec in &layout.sections {
        for row in &sec.rows {
            if let Some(hit) = hit_test_row(row, px, py) {
                return Some(hit);
            }
        }
    }
    None
}

fn hit_test_appearance(app: &AppearanceCardsLayout, px: f64, py: f64) -> Option<SettingsHit> {
    for tile in &app.tiles {
        if tile.ring_rect.contains_f64(px, py) || tile.label_rect.contains_f64(px, py) {
            return Some(SettingsHit::SelectTheme(tile.choice));
        }
    }
    for (idx, (rect, _, _)) in app.swatches.iter().enumerate() {
        if rect.contains_f64(px, py) {
            return Some(SettingsHit::SelectAccent(idx));
        }
    }
    for row in &app.links {
        if row.rect.contains_f64(px, py)
            && let CardRowControl::Navigation { target, .. } = row.control
        {
            return Some(SettingsHit::PushPage(target));
        }
    }
    None
}

fn hit_test_row(row: &CardRowLayout, px: f64, py: f64) -> Option<SettingsHit> {
    match &row.control {
        CardRowControl::WifiNetwork {
            action,
            info_action,
            ..
        } => {
            if row.rect.contains_f64(px, py) {
                let info_x = f64::from(row.rect.right() - 34);
                if px >= info_x {
                    return Some(SettingsHit::Control(*info_action));
                }
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::NotificationAppRow { action, .. } => {
            if row.rect.contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::Switch { action, .. }
        | CardRowControl::WifiHeaderSwitch { action, .. }
        | CardRowControl::ServiceHeaderSwitch { action, .. }
        | CardRowControl::NotificationHeader { action, .. } => {
            if switch_rect(row.rect).contains_f64(px, py) || row.rect.contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::BluetoothDeviceRow {
            action,
            info_action,
            ..
        } => {
            let (act_r, info_r) = bt_device_action_rects(row.rect);
            if info_r.contains_f64(px, py) {
                return Some(SettingsHit::Control(*info_action));
            }
            if act_r.contains_f64(px, py) || row.rect.contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::DropdownTrigger { .. } => {
            if dropdown_trigger_rect(row.rect).contains_f64(px, py) {
                return Some(SettingsHit::LowPowerDropdownTrigger);
            }
        }
        CardRowControl::BatterySegmentedTabs => {
            let tabs_r = battery_tabs_inner_rect(row.rect);
            if tabs_r.contains_f64(px, py) {
                let idx = usize::from(px >= f64::from(tabs_r.x + tabs_r.w / 2));
                return Some(SettingsHit::BatteryTab(idx));
            }
        }
        CardRowControl::AboutHeroCard { action, .. } => {
            if about_more_info_rect(row.rect).contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::ShortcutBinding { action, .. } => {
            if shortcut_pill_rect(row.rect).contains_f64(px, py) || row.rect.contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::Choice { action, .. } => {
            if choice_rect(row.rect).contains_f64(px, py) {
                return Some(SettingsHit::Control(*action));
            }
        }
        CardRowControl::Stepper { down, up, .. } => {
            let (minus, _, plus) = stepper_rects(row.rect);
            if minus.contains_f64(px, py) {
                return Some(SettingsHit::StepperMinus(*down));
            }
            if plus.contains_f64(px, py) {
                return Some(SettingsHit::StepperPlus(*up));
            }
        }
        CardRowControl::Navigation { target, .. }
        | CardRowControl::ServiceStatus {
            target: Some(target),
            ..
        } => {
            if row.rect.contains_f64(px, py) {
                return Some(SettingsHit::PushPage(*target));
            }
        }
        CardRowControl::ServiceStatus { target: None, .. }
        | CardRowControl::Info { .. }
        | CardRowControl::UserProfileCard { .. } => {}
    }
    None
}

fn hit_test_resize_edge(win_w: i32, win_h: i32, px: f64, py: f64) -> Option<ResizeEdgeZone> {
    let b = f64::from(RESIZE_BORDER_PX);
    let w = f64::from(win_w);
    let h = f64::from(win_h);
    let left = px >= 0.0 && px < b;
    let right = px >= w - b && px < w;
    let top = py >= 0.0 && py < b;
    let bottom = py >= h - b && py < h;
    match (left, right, top, bottom) {
        (true, false, true, false) => Some(ResizeEdgeZone::TopLeft),
        (false, true, true, false) => Some(ResizeEdgeZone::TopRight),
        (true, false, false, true) => Some(ResizeEdgeZone::BottomLeft),
        (false, true, false, true) => Some(ResizeEdgeZone::BottomRight),
        (true, false, false, false) => Some(ResizeEdgeZone::Left),
        (false, true, false, false) => Some(ResizeEdgeZone::Right),
        (false, false, true, false) => Some(ResizeEdgeZone::Top),
        (false, false, false, true) => Some(ResizeEdgeZone::Bottom),
        _ => None,
    }
}
