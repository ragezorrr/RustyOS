//! Deterministic Golden Gate geometry & hit-testing for `rusty-updater` (`RUSTYOS-077`).

use rusty_services::updates::UpdateTxPhase;

use crate::state::{UpdaterAppState, UpdaterFilter};

pub(crate) const WINDOW_CORNER_RADIUS: i32 = 20;
const TITLEBAR_HEIGHT: i32 = 52;
const RESIZE_BORDER_PX: i32 = 5;

/// Axis-aligned integer rectangle in surface-local coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    #[must_use]
    pub(crate) const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub(crate) const fn right(self) -> i32 {
        self.x + self.w
    }

    #[must_use]
    pub(crate) const fn bottom(self) -> i32 {
        self.y + self.h
    }

    #[must_use]
    pub(crate) fn contains_f64(self, px: f64, py: f64) -> bool {
        self.w > 0
            && self.h > 0
            && px >= f64::from(self.x)
            && px < f64::from(self.right())
            && py >= f64::from(self.y)
            && py < f64::from(self.bottom())
    }
}

/// CSD window edge/corner resize hotspot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResizeEdgeZone {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Interactive click regions inside `rusty-updater`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdaterHit {
    ResizeEdge(ResizeEdgeZone),
    WindowDrag,
    TrafficClose,
    TrafficMinimize,
    TrafficMaximize,
    CheckNow,
    PrimaryAction,
    FilterTab(UpdaterFilter),
    ToggleLogs,
    RebuildLocalPkg,
    PackageRow(usize),
}

/// Precomputed layout rectangles shared between renderer and pointer hit-testing.
#[derive(Debug, Clone)]
pub(crate) struct UpdaterLayout {
    pub win_w: i32,
    pub win_h: i32,
    pub header_rect: Rect,
    pub traffic_close: Rect,
    pub traffic_min: Rect,
    pub traffic_max: Rect,
    pub refresh_btn: Rect,
    pub hero_card: Rect,
    pub emblem_rect: Rect,
    pub primary_btn: Rect,
    pub progress_card: Option<Rect>,
    pub progress_track: Option<Rect>,
    pub filter_tabs: [(UpdaterFilter, Rect); 3],
    pub logs_toggle_btn: Rect,
    pub list_card: Rect,
    pub list_viewport: Rect,
    pub row_rects: Vec<(usize, Rect)>,
    pub footer_rect: Rect,
    pub rebuild_btn: Option<Rect>,
}

#[must_use]
pub(crate) fn compute_layout(app: &UpdaterAppState, width: u32, height: u32) -> UpdaterLayout {
    let win_w = i32::try_from(width.max(480)).unwrap_or(740);
    let win_h = i32::try_from(height.max(380)).unwrap_or(560);
    let pad = 20;

    let header_rect = Rect::new(0, 0, win_w, TITLEBAR_HEIGHT);
    let traffic_close = Rect::new(18, 18, 14, 14);
    let traffic_min = Rect::new(38, 18, 14, 14);
    let traffic_max = Rect::new(58, 18, 14, 14);
    let refresh_btn = Rect::new(win_w - pad - 30, 11, 30, 30);

    let hero_card = Rect::new(pad, TITLEBAR_HEIGHT + 6, win_w - pad * 2, 92);
    let emblem_rect = Rect::new(hero_card.x + 18, hero_card.y + 22, 48, 48);
    let primary_btn = Rect::new(
        hero_card.right() - 148,
        hero_card.y + (hero_card.h - 32) / 2,
        130,
        32,
    );

    let show_progress = app.is_upgrading()
        || !app.tx_logs.is_empty()
        || matches!(
            app.tx_phase,
            UpdateTxPhase::Complete | UpdateTxPhase::Failed | UpdateTxPhase::Cancelled
        );

    let mut cur_y = hero_card.bottom() + 12;
    let (progress_card, progress_track) = if show_progress {
        let pcard = Rect::new(pad, cur_y, win_w - pad * 2, 44);
        let ptrack = Rect::new(pcard.x + 16, pcard.y + 28, pcard.w - 32, 6);
        cur_y = pcard.bottom() + 12;
        (Some(pcard), Some(ptrack))
    } else {
        (None, None)
    };

    let tab_y = cur_y;
    let tab_x = pad;
    let tab_widths = [88, 108, 108];
    let filter_tabs = [
        (
            UpdaterFilter::All,
            Rect::new(tab_x, tab_y, tab_widths[0], 28),
        ),
        (
            UpdaterFilter::RustyOs,
            Rect::new(tab_x + tab_widths[0] + 8, tab_y, tab_widths[1], 28),
        ),
        (
            UpdaterFilter::RestartRequired,
            Rect::new(
                tab_x + tab_widths[0] + tab_widths[1] + 16,
                tab_y,
                tab_widths[2],
                28,
            ),
        ),
    ];
    let logs_toggle_btn = Rect::new(win_w - pad - 96, tab_y, 96, 28);

    let list_top = tab_y + 38;
    let footer_h = 36;
    let footer_rect = Rect::new(pad, win_h - footer_h - 6, win_w - pad * 2, footer_h);
    let rebuild_btn = app
        .summary
        .repo_pkgbuild_dir
        .is_some()
        .then(|| Rect::new(footer_rect.right() - 148, footer_rect.y + 4, 148, 28));

    let list_h = (footer_rect.y - 10 - list_top).max(96);
    let list_card = Rect::new(pad, list_top, win_w - pad * 2, list_h);
    let list_viewport = Rect::new(
        list_card.x + 2,
        list_card.y + 4,
        list_card.w - 4,
        list_card.h - 8,
    );

    let row_rects = compute_visible_row_rects(app, list_viewport);

    UpdaterLayout {
        win_w,
        win_h,
        header_rect,
        traffic_close,
        traffic_min,
        traffic_max,
        refresh_btn,
        hero_card,
        emblem_rect,
        primary_btn,
        progress_card,
        progress_track,
        filter_tabs,
        logs_toggle_btn,
        list_card,
        list_viewport,
        row_rects,
        footer_rect,
        rebuild_btn,
    }
}

fn compute_visible_row_rects(app: &UpdaterAppState, list_viewport: Rect) -> Vec<(usize, Rect)> {
    if app.show_logs {
        return Vec::new();
    }
    let items = app.filtered_items();
    let mut rows = Vec::new();
    for (idx, _) in items.iter().enumerate() {
        let idx_i32 = i32::try_from(idx).unwrap_or(0);
        let ry = list_viewport.y + 4 + idx_i32 * crate::state::ROW_HEIGHT - app.scroll_y;
        if ry + crate::state::ROW_HEIGHT < list_viewport.y || ry > list_viewport.bottom() {
            continue;
        }
        rows.push((
            idx,
            Rect::new(
                list_viewport.x + 6,
                ry,
                list_viewport.w - 12,
                crate::state::ROW_HEIGHT - 2,
            ),
        ));
    }
    rows
}

#[must_use]
pub(crate) fn detect_resize_edge(
    layout: &UpdaterLayout,
    px: f64,
    py: f64,
) -> Option<ResizeEdgeZone> {
    let w = f64::from(layout.win_w);
    let h = f64::from(layout.win_h);
    let border = f64::from(RESIZE_BORDER_PX);
    if px < 0.0 || py < 0.0 || px > w || py > h {
        return None;
    }
    let left = px <= border;
    let right = px >= w - border;
    let top = py <= border;
    let bottom = py >= h - border;
    match (top, bottom, left, right) {
        (true, _, true, _) => Some(ResizeEdgeZone::TopLeft),
        (true, _, _, true) => Some(ResizeEdgeZone::TopRight),
        (_, true, true, _) => Some(ResizeEdgeZone::BottomLeft),
        (_, true, _, true) => Some(ResizeEdgeZone::BottomRight),
        (true, _, _, _) => Some(ResizeEdgeZone::Top),
        (_, true, _, _) => Some(ResizeEdgeZone::Bottom),
        (_, _, true, _) => Some(ResizeEdgeZone::Left),
        (_, _, _, true) => Some(ResizeEdgeZone::Right),
        _ => None,
    }
}

#[must_use]
pub(crate) fn hit_test(layout: &UpdaterLayout, px: f64, py: f64) -> Option<UpdaterHit> {
    if let Some(edge) = detect_resize_edge(layout, px, py) {
        return Some(UpdaterHit::ResizeEdge(edge));
    }
    if layout.traffic_close.contains_f64(px, py) {
        return Some(UpdaterHit::TrafficClose);
    }
    if layout.traffic_min.contains_f64(px, py) {
        return Some(UpdaterHit::TrafficMinimize);
    }
    if layout.traffic_max.contains_f64(px, py) {
        return Some(UpdaterHit::TrafficMaximize);
    }
    if layout.refresh_btn.contains_f64(px, py) {
        return Some(UpdaterHit::CheckNow);
    }
    if layout.header_rect.contains_f64(px, py) {
        return Some(UpdaterHit::WindowDrag);
    }
    if layout.primary_btn.contains_f64(px, py) {
        return Some(UpdaterHit::PrimaryAction);
    }
    for &(filter, rect) in &layout.filter_tabs {
        if rect.contains_f64(px, py) {
            return Some(UpdaterHit::FilterTab(filter));
        }
    }
    if layout.logs_toggle_btn.contains_f64(px, py) {
        return Some(UpdaterHit::ToggleLogs);
    }
    if let Some(rb) = layout.rebuild_btn
        && rb.contains_f64(px, py)
    {
        return Some(UpdaterHit::RebuildLocalPkg);
    }
    if layout.list_viewport.contains_f64(px, py) {
        for &(idx, rect) in &layout.row_rects {
            if rect.contains_f64(px, py) {
                return Some(UpdaterHit::PackageRow(idx));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StartupOptions;

    #[test]
    fn layout_and_hit_testing_cover_header_buttons_tabs_and_rows() {
        let app = UpdaterAppState::new(&StartupOptions {
            dry_run: true,
            ..StartupOptions::default()
        });
        let layout = compute_layout(&app, 740, 560);
        assert_eq!(layout.row_rects.len(), 5);
        assert!(layout.list_card.bottom() < layout.footer_rect.y);

        assert_eq!(
            hit_test(
                &layout,
                f64::from(layout.traffic_close.x + 6),
                f64::from(layout.traffic_close.y + 6)
            ),
            Some(UpdaterHit::TrafficClose)
        );
        assert_eq!(
            hit_test(
                &layout,
                f64::from(layout.primary_btn.x + 20),
                f64::from(layout.primary_btn.y + 12)
            ),
            Some(UpdaterHit::PrimaryAction)
        );
        assert_eq!(
            hit_test(
                &layout,
                f64::from(layout.filter_tabs[1].1.x + 14),
                f64::from(layout.filter_tabs[1].1.y + 10)
            ),
            Some(UpdaterHit::FilterTab(UpdaterFilter::RustyOs))
        );
        assert_eq!(
            hit_test(
                &layout,
                f64::from(layout.row_rects[0].1.x + 20),
                f64::from(layout.row_rects[0].1.y + 12)
            ),
            Some(UpdaterHit::PackageRow(0))
        );
        assert_eq!(hit_test(&layout, 350.0, 20.0), Some(UpdaterHit::WindowDrag));
    }
}
