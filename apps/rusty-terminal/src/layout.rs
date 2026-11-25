//! RustyOS CSD window layout, hit-testing, and cell grid geometry.

use crate::font::CellMetrics;
use crate::vt::GridPoint;

pub(crate) const DEFAULT_WIDTH: i32 = 900;
pub(crate) const DEFAULT_HEIGHT: i32 = 560;
pub(crate) const MIN_WIDTH: i32 = 480;
pub(crate) const MIN_HEIGHT: i32 = 320;
pub(crate) const HEADER_HEIGHT: i32 = 40;
#[allow(clippy::cast_possible_wrap)]
pub(crate) const WINDOW_CORNER_RADIUS: i32 = rusty_ui::WINDOW_RADIUS as i32;
pub(crate) const TERMINAL_PAD_X: i32 = 12;
pub(crate) const TERMINAL_PAD_Y: i32 = 8;
pub(crate) const SEARCH_BAR_HEIGHT: i32 = 38;
pub(crate) const SCROLLBAR_WIDTH: i32 = 6;

/// Integer axis-aligned screen rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Rect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
}

impl Rect {
    #[must_use]
    pub(crate) const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub(crate) const fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    #[must_use]
    pub(crate) const fn right(self) -> i32 {
        self.x + self.w
    }

    #[must_use]
    pub(crate) const fn bottom(self) -> i32 {
        self.y + self.h
    }
}

/// Direction for splitting a terminal pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitDirection {
    /// Side-by-side (left | right).
    Horizontal,
    /// Stacked (top / bottom).
    Vertical,
}

/// Binary split tree node representing panes inside a tab.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PaneNode {
    Leaf(u64),
    Split {
        dir: SplitDirection,
        ratio: f32,
        first: Box<Self>,
        second: Box<Self>,
    },
}

/// Computed screen rectangle and grid dimensions for one pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaneRect {
    pub(crate) pane_id: u64,
    pub(crate) rect: Rect,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

impl PaneNode {
    #[must_use]
    pub(crate) fn split_leaf(&mut self, target_id: u64, new_id: u64, dir: SplitDirection) -> bool {
        match self {
            Self::Leaf(id) if *id == target_id => {
                *self = Self::Split {
                    dir,
                    ratio: 0.5,
                    first: Box::new(Self::Leaf(target_id)),
                    second: Box::new(Self::Leaf(new_id)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_leaf(target_id, new_id, dir)
                    || second.split_leaf(target_id, new_id, dir)
            }
        }
    }

    #[must_use]
    pub(crate) fn remove_leaf(self, target_id: u64) -> Option<Self> {
        match self {
            Self::Leaf(id) => (id != target_id).then_some(Self::Leaf(id)),
            Self::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                let f = first.remove_leaf(target_id);
                let s = second.remove_leaf(target_id);
                match (f, s) {
                    (Some(a), Some(b)) => Some(Self::Split {
                        dir,
                        ratio,
                        first: Box::new(a),
                        second: Box::new(b),
                    }),
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
        }
    }

    pub(crate) fn collect_rects(
        &self,
        bounds: Rect,
        metrics: CellMetrics,
        out: &mut Vec<PaneRect>,
        dividers: &mut Vec<Rect>,
    ) {
        match self {
            Self::Leaf(id) => {
                let cols = ((bounds.w / metrics.cell_w.max(1)).clamp(4, 512)) as u16;
                let rows = ((bounds.h / metrics.cell_h.max(1)).clamp(2, 256)) as u16;
                out.push(PaneRect {
                    pane_id: *id,
                    rect: bounds,
                    cols,
                    rows,
                });
            }
            Self::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                let pad = 6_i32;
                match dir {
                    SplitDirection::Horizontal => {
                        let usable_w = (bounds.w - pad * 2 - 1).max(2);
                        let w1 = ((usable_w as f32) * ratio.clamp(0.2, 0.8)).round() as i32;
                        let div_x = bounds.x + w1 + pad;
                        dividers.push(Rect::new(div_x, bounds.y, 1, bounds.h));
                        let r1 = Rect::new(bounds.x, bounds.y, w1.max(1), bounds.h);
                        let r2 = Rect::new(
                            div_x + 1 + pad,
                            bounds.y,
                            (bounds.right() - (div_x + 1 + pad)).max(1),
                            bounds.h,
                        );
                        first.collect_rects(r1, metrics, out, dividers);
                        second.collect_rects(r2, metrics, out, dividers);
                    }
                    SplitDirection::Vertical => {
                        let usable_h = (bounds.h - pad * 2 - 1).max(2);
                        let h1 = ((usable_h as f32) * ratio.clamp(0.2, 0.8)).round() as i32;
                        let div_y = bounds.y + h1 + pad;
                        dividers.push(Rect::new(bounds.x, div_y, bounds.w, 1));
                        let r1 = Rect::new(bounds.x, bounds.y, bounds.w, h1.max(1));
                        let r2 = Rect::new(
                            bounds.x,
                            div_y + 1 + pad,
                            bounds.w,
                            (bounds.bottom() - (div_y + 1 + pad)).max(1),
                        );
                        first.collect_rects(r1, metrics, out, dividers);
                        second.collect_rects(r2, metrics, out, dividers);
                    }
                }
            }
        }
    }
}

/// Maps window coordinates `(px, py)` to a cell inside an arbitrary [`PaneRect`].
#[must_use]
pub(crate) fn point_to_cell_in_rect(
    pane: PaneRect,
    metrics: CellMetrics,
    scrollback_len: usize,
    scroll_offset_lines: f64,
    px: i32,
    py: i32,
) -> (GridPoint, usize, usize) {
    let rel_x = (px - pane.rect.x).max(0);
    let rel_y = (py - pane.rect.y).max(0);
    let col = usize::try_from(rel_x / metrics.cell_w.max(1))
        .unwrap_or(0)
        .min(usize::from(pane.cols).saturating_sub(1));
    let row = usize::try_from(rel_y / metrics.cell_h.max(1))
        .unwrap_or(0)
        .min(usize::from(pane.rows).saturating_sub(1));
    let whole_back = scroll_offset_lines.floor().max(0.0) as usize;
    let top_abs = scrollback_len.saturating_sub(whole_back.min(scrollback_len));
    (
        GridPoint {
            abs_line: top_abs + row,
            col,
        },
        col,
        row,
    )
}

/// Interactive region hit target in the CSD window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitTarget {
    CloseButton,
    MinimizeButton,
    MaximizeButton,
    GlassToggle,
    SearchToggle,
    NewTabButton,
    Tab(usize),
    TabClose(usize),
    HeaderDrag,
    SearchPrev,
    SearchNext,
    SearchClose,
    SearchBar,
    ScrollbarTrack,
    TerminalGrid,
}

/// Geometry of a single header tab pill when multiple tabs are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabLayout {
    pub(crate) pill: Rect,
    pub(crate) close_btn: Rect,
}

/// Precomputed layout rectangles for the current window size, tab count, and font metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalLayout {
    pub(crate) win_w: i32,
    pub(crate) win_h: i32,
    pub(crate) header_rect: Rect,
    pub(crate) close_rect: Rect,
    pub(crate) minimize_rect: Rect,
    pub(crate) maximize_rect: Rect,
    pub(crate) glass_btn_rect: Rect,
    pub(crate) search_btn_rect: Rect,
    pub(crate) new_tab_btn_rect: Rect,
    pub(crate) title_rect: Rect,
    pub(crate) tab_rects: Vec<TabLayout>,
    pub(crate) search_bar_rect: Option<Rect>,
    pub(crate) search_input_rect: Option<Rect>,
    pub(crate) search_prev_rect: Option<Rect>,
    pub(crate) search_next_rect: Option<Rect>,
    pub(crate) search_close_rect: Option<Rect>,
    pub(crate) grid_rect: Rect,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) scrollbar_track: Rect,
    pub(crate) scrollbar_thumb: Option<Rect>,
}

impl TerminalLayout {
    /// Computes complete CSD + terminal grid geometry.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn compute(
        win_w: i32,
        win_h: i32,
        tab_count: usize,
        search_open: bool,
        metrics: CellMetrics,
        scrollback_len: usize,
        scroll_offset_lines: f64,
    ) -> Self {
        let w = win_w.max(MIN_WIDTH);
        let h = win_h.max(MIN_HEIGHT);

        let header_rect = Rect::new(0, 0, w, HEADER_HEIGHT);
        let tl = rusty_ui::TrafficLightButtons::default_csd();
        let btn_d = i32::try_from(tl.close_rect.width).unwrap_or(14);
        let close_rect = Rect::new(tl.close_rect.x, tl.close_rect.y, btn_d, btn_d);
        let minimize_rect = Rect::new(tl.minimize_rect.x, tl.minimize_rect.y, btn_d, btn_d);
        let maximize_rect = Rect::new(tl.maximize_rect.x, tl.maximize_rect.y, btn_d, btn_d);

        let btn_w = 28;
        let btn_h = 26;
        let btn_y = 7;
        let new_tab_btn_rect = Rect::new(w - 12 - btn_w, btn_y, btn_w, btn_h);
        let search_btn_rect = Rect::new(new_tab_btn_rect.x - 6 - btn_w, btn_y, btn_w, btn_h);
        let glass_btn_rect = Rect::new(search_btn_rect.x - 6 - btn_w, btn_y, btn_w, btn_h);

        let strip_x = 84;
        let strip_right = (glass_btn_rect.x - 10).max(strip_x + 80);
        let strip_w = (strip_right - strip_x).max(60);
        let title_rect = Rect::new(strip_x, 0, strip_w, HEADER_HEIGHT);

        let tab_rects = if tab_count > 1 {
            compute_tabs_layout(strip_x, strip_w, tab_count)
        } else {
            Vec::new()
        };

        let search_top = HEADER_HEIGHT + 6;
        let search_bars = if search_open {
            compute_search_bar(w, search_top)
        } else {
            SearchBarRects::default()
        };

        let content_top = if search_open {
            search_top + SEARCH_BAR_HEIGHT + 4
        } else {
            HEADER_HEIGHT
        };

        let avail_w = (w - TERMINAL_PAD_X * 2 - SCROLLBAR_WIDTH - 4).max(metrics.cell_w * 2);
        let avail_h = (h - content_top - TERMINAL_PAD_Y * 2).max(metrics.cell_h * 2);

        let cols = (avail_w / metrics.cell_w.max(1)).clamp(2, 512) as u16;
        let rows = (avail_h / metrics.cell_h.max(1)).clamp(2, 256) as u16;

        let grid_w = i32::from(cols) * metrics.cell_w;
        let grid_h = i32::from(rows) * metrics.cell_h;
        let grid_rect = Rect::new(TERMINAL_PAD_X, content_top + TERMINAL_PAD_Y, grid_w, grid_h);

        let scrollbar_track = Rect::new(
            w - SCROLLBAR_WIDTH - 4,
            content_top + 6,
            SCROLLBAR_WIDTH,
            (h - content_top - 12).max(20),
        );
        let scrollbar_thumb = compute_scrollbar_thumb(
            scrollbar_track,
            usize::from(rows),
            scrollback_len,
            scroll_offset_lines,
        );

        Self {
            win_w: w,
            win_h: h,
            header_rect,
            close_rect,
            minimize_rect,
            maximize_rect,
            glass_btn_rect,
            search_btn_rect,
            new_tab_btn_rect,
            title_rect,
            tab_rects,
            search_bar_rect: search_bars.bar,
            search_input_rect: search_bars.input,
            search_prev_rect: search_bars.prev,
            search_next_rect: search_bars.next,
            search_close_rect: search_bars.close,
            grid_rect,
            cols,
            rows,
            scrollbar_track,
            scrollbar_thumb,
        }
    }

    /// Maps window coordinates `(px, py)` to a semantic interactive target.
    #[must_use]
    pub(crate) fn hit_test(&self, px: i32, py: i32) -> HitTarget {
        if self.close_rect.contains(px, py) {
            return HitTarget::CloseButton;
        }
        if self.minimize_rect.contains(px, py) {
            return HitTarget::MinimizeButton;
        }
        if self.maximize_rect.contains(px, py) {
            return HitTarget::MaximizeButton;
        }
        if self.new_tab_btn_rect.contains(px, py) {
            return HitTarget::NewTabButton;
        }
        if self.search_btn_rect.contains(px, py) {
            return HitTarget::SearchToggle;
        }
        if self.glass_btn_rect.contains(px, py) {
            return HitTarget::GlassToggle;
        }
        for (idx, tab) in self.tab_rects.iter().enumerate() {
            if tab.close_btn.contains(px, py) {
                return HitTarget::TabClose(idx);
            }
            if tab.pill.contains(px, py) {
                return HitTarget::Tab(idx);
            }
        }
        if self.header_rect.contains(px, py) {
            return HitTarget::HeaderDrag;
        }
        if self.search_prev_rect.is_some_and(|r| r.contains(px, py)) {
            return HitTarget::SearchPrev;
        }
        if self.search_next_rect.is_some_and(|r| r.contains(px, py)) {
            return HitTarget::SearchNext;
        }
        if self.search_close_rect.is_some_and(|r| r.contains(px, py)) {
            return HitTarget::SearchClose;
        }
        if self.search_bar_rect.is_some_and(|r| r.contains(px, py)) {
            return HitTarget::SearchBar;
        }
        if self.scrollbar_thumb.is_some() && self.scrollbar_track.contains(px, py) {
            return HitTarget::ScrollbarTrack;
        }
        HitTarget::TerminalGrid
    }

    /// Converts pixel `(px, py)` into `(abs_grid_point, col_0based, row_0based)`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn point_to_cell(
        &self,
        metrics: CellMetrics,
        scrollback_len: usize,
        scroll_offset_lines: f64,
        px: i32,
        py: i32,
    ) -> (GridPoint, usize, usize) {
        let rel_x = (px - self.grid_rect.x).clamp(0, self.grid_rect.w.saturating_sub(1));
        let rel_y = (py - self.grid_rect.y).clamp(0, self.grid_rect.h.saturating_sub(1));
        let col = (rel_x / metrics.cell_w.max(1)).min(i32::from(self.cols) - 1) as usize;
        let row = (rel_y / metrics.cell_h.max(1)).min(i32::from(self.rows) - 1) as usize;

        let scroll_back = scroll_offset_lines.round().max(0.0) as usize;
        let top_abs = scrollback_len.saturating_sub(scroll_back.min(scrollback_len));
        (
            GridPoint {
                abs_line: top_abs + row,
                col,
            },
            col,
            row,
        )
    }
}

fn compute_tabs_layout(strip_x: i32, strip_w: i32, count: usize) -> Vec<TabLayout> {
    let gap = 6;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let count_i = count.max(1) as i32;
    let total_gaps = gap * (count_i - 1);
    let pill_w = ((strip_w - total_gaps) / count_i).clamp(72, 220);
    let mut out = Vec::with_capacity(count);
    for idx in 0..count {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let x = strip_x + (idx as i32) * (pill_w + gap);
        let pill = Rect::new(x, 7, pill_w, 26);
        let close_btn = Rect::new(pill.right() - 20, pill.y + 5, 16, 16);
        out.push(TabLayout { pill, close_btn });
    }
    out
}

#[derive(Default)]
struct SearchBarRects {
    bar: Option<Rect>,
    input: Option<Rect>,
    prev: Option<Rect>,
    next: Option<Rect>,
    close: Option<Rect>,
}

fn compute_search_bar(win_w: i32, top_y: i32) -> SearchBarRects {
    let bar_w = (win_w - TERMINAL_PAD_X * 2).clamp(240, 380);
    let bar_x = win_w - TERMINAL_PAD_X - bar_w;
    let bar = Rect::new(bar_x, top_y, bar_w, SEARCH_BAR_HEIGHT);
    let close = Rect::new(bar.right() - 30, top_y + 7, 24, 24);
    let next = Rect::new(close.x - 26, top_y + 7, 24, 24);
    let prev = Rect::new(next.x - 26, top_y + 7, 24, 24);
    let input = Rect::new(bar.x + 10, top_y + 6, (prev.x - bar.x - 16).max(60), 26);
    SearchBarRects {
        bar: Some(bar),
        input: Some(input),
        prev: Some(prev),
        next: Some(next),
        close: Some(close),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn compute_scrollbar_thumb(
    track: Rect,
    visible_rows: usize,
    scrollback_len: usize,
    scroll_offset_lines: f64,
) -> Option<Rect> {
    if scrollback_len == 0 {
        return None;
    }
    let total = scrollback_len + visible_rows;
    let ratio = (visible_rows as f64 / total as f64).clamp(0.06, 1.0);
    let thumb_h = (f64::from(track.h) * ratio).round().max(24.0) as i32;
    let travel = (track.h - thumb_h).max(0);
    // `scroll_offset_lines == 0` sits at bottom of track; `== scrollback_len` sits at top.
    let frac_from_top = 1.0 - (scroll_offset_lines / (scrollback_len as f64)).clamp(0.0, 1.0);
    let thumb_y = track.y + (f64::from(travel) * frac_from_top).round() as i32;
    Some(Rect::new(track.x, thumb_y, track.w, thumb_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_clamps_minimum_and_places_tabs_and_traffic_lights() {
        let metrics = CellMetrics {
            font_size: 14.0,
            ui_size: 13.0,
            cell_w: 9,
            cell_h: 19,
            baseline_y: 14,
        };
        let layout = TerminalLayout::compute(900, 560, 2, true, metrics, 200, 50.0);
        assert!(layout.cols >= 80);
        assert!(layout.rows >= 20);
        assert_eq!(layout.hit_test(20, 20), HitTarget::CloseButton);
        assert_eq!(layout.hit_test(42, 20), HitTarget::MinimizeButton);
        assert_eq!(layout.hit_test(64, 20), HitTarget::MaximizeButton);
        assert_eq!(layout.tab_rects.len(), 2);
        assert!(layout.search_bar_rect.is_some());
        assert!(layout.scrollbar_thumb.is_some());

        let tab0 = layout.tab_rects[0].pill;
        assert_eq!(layout.hit_test(tab0.x + 8, tab0.y + 8), HitTarget::Tab(0));
    }
}
