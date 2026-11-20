//! Reusable selectable list rows and keyboard-navigable list widget (`RUSTYOS-130`).

use std::time::Duration;

use crate::draw::UiPainter;
use crate::geometry::Rect;
use crate::scroll::ScrollView;
use crate::text::UiFontEngine;
use crate::theme::ThemePalette;
use crate::widgets::GroupCard;

/// Selection highlight presentation for list items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListSelectionMode {
    #[default]
    AccentPill,
    NeutralPill,
    None,
}

/// Single list item model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    pub title: String,
    pub subtitle: Option<String>,
    pub trailing_value: Option<String>,
    pub has_chevron: bool,
    pub checked: bool,
    pub disabled: bool,
}

impl ListRow {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            trailing_value: None,
            has_chevron: false,
            checked: false,
            disabled: false,
        }
    }

    #[must_use]
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    #[must_use]
    pub fn with_trailing(mut self, value: impl Into<String>) -> Self {
        self.trailing_value = Some(value.into());
        self
    }

    #[must_use]
    pub const fn with_chevron(mut self, chevron: bool) -> Self {
        self.has_chevron = chevron;
        self
    }

    #[must_use]
    pub const fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

/// Keyboard- and pointer-driven list view inside a [`ScrollView`].
#[derive(Debug, Clone, PartialEq)]
pub struct SelectableList {
    pub scroll: ScrollView,
    pub rows: Vec<ListRow>,
    pub selected: Option<usize>,
    pub hovered: Option<usize>,
    pub row_height: i32,
    pub selection_mode: ListSelectionMode,
    pub in_card: bool,
}

impl SelectableList {
    #[must_use]
    pub fn new(viewport: Rect, rows: Vec<ListRow>) -> Self {
        let row_height = 38;
        let content_h = rows.len() as i32 * row_height;
        let mut scroll = ScrollView::new(viewport);
        scroll.set_bounds(viewport, content_h);
        Self {
            scroll,
            rows,
            selected: None,
            hovered: None,
            row_height,
            selection_mode: ListSelectionMode::NeutralPill,
            in_card: true,
        }
    }

    pub fn sync_bounds(&mut self, viewport: Rect) {
        let total = self.rows.len() as i32 * self.row_height;
        self.scroll.set_bounds(viewport, total);
    }

    #[must_use]
    pub fn row_rect(&self, idx: usize) -> Option<Rect> {
        if idx >= self.rows.len() {
            return None;
        }
        let y = self.scroll.viewport.y + idx as i32 * self.row_height - self.scroll.offset_y();
        Some(Rect::new(
            self.scroll.viewport.x,
            y,
            self.scroll.viewport.width,
            self.row_height as u32,
        ))
    }

    #[must_use]
    pub fn hit_test(&self, px: f64, py: f64) -> Option<usize> {
        if !self.scroll.viewport.contains_f64(px, py) {
            return None;
        }
        let rel_y = py as i32 - self.scroll.viewport.y + self.scroll.offset_y();
        if rel_y < 0 {
            return None;
        }
        let idx = (rel_y / self.row_height) as usize;
        (idx < self.rows.len() && !self.rows[idx].disabled).then_some(idx)
    }

    pub fn on_pointer_down(&mut self, px: f64, py: f64) -> Option<usize> {
        let idx = self.hit_test(px, py)?;
        self.selected = Some(idx);
        Some(idx)
    }

    pub fn on_pointer_move(&mut self, px: f64, py: f64) -> bool {
        let next = self.hit_test(px, py);
        if self.hovered == next {
            false
        } else {
            self.hovered = next;
            true
        }
    }

    /// Moves selection up (`-1`) or down (`+1`) while scrolling the newly selected row into view.
    pub fn move_selection(
        &mut self,
        step: isize,
        now: Duration,
        reduced_motion: bool,
    ) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let len = self.rows.len() as isize;
        let start = self
            .selected
            .map_or(if step >= 0 { -1 } else { 0 }, |i| i as isize);
        for offset in 1..=len {
            let cand = ((start + step.signum() * offset).rem_euclid(len)) as usize;
            if !self.rows[cand].disabled {
                self.selected = Some(cand);
                let top = cand as i32 * self.row_height;
                self.scroll
                    .ensure_visible(top, top + self.row_height, now, reduced_motion);
                return Some(cand);
            }
        }
        None
    }

    pub fn paint(&self, painter: &mut UiPainter<'_>, fonts: &mut UiFontEngine, pal: &ThemePalette) {
        let vp = self.scroll.viewport;
        if self.in_card {
            painter.fill_rounded_rect(vp, GroupCard::new(vp).corner_radius, pal.card_bg);
        }
        self.scroll.push_clip(painter);

        for (idx, row) in self.rows.iter().enumerate() {
            if let Some(rr) = self.row_rect(idx)
                && rr.bottom() >= vp.y
                && rr.y <= vp.bottom()
            {
                self.paint_row(painter, fonts, pal, idx, row, rr);
            }
        }
        painter.pop_clip();
        self.scroll.paint_scrollbar(painter, pal);
    }

    fn paint_row(
        &self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        pal: &ThemePalette,
        idx: usize,
        row: &ListRow,
        rr: Rect,
    ) {
        let is_sel = self.selected == Some(idx);
        let is_hov = self.hovered == Some(idx) && !row.disabled;
        paint_row_bg(painter, pal, rr, is_sel, is_hov, self.selection_mode);

        let accent_text = is_sel && self.selection_mode == ListSelectionMode::AccentPill;
        let title_col = if row.disabled {
            pal.text_muted
        } else if accent_text {
            [255, 255, 255, 255]
        } else {
            pal.text_primary
        };
        let title_y = if row.subtitle.is_some() {
            rr.y as f32 + 17.0
        } else {
            rr.y as f32 + f32::midpoint(rr.height as f32, 13.0 * 0.72)
        };
        let max_title_w = (rr.width as f32 - 64.0).max(20.0);
        let title_fit = fonts.ellipsize(&row.title, max_title_w, 13.0, false);
        painter.draw_text(
            title_fit,
            rr.x as f32 + 14.0,
            title_y,
            13.0,
            false,
            title_col,
        );

        if let Some(sub) = &row.subtitle {
            let sub_col = if accent_text {
                [255, 255, 255, 215]
            } else {
                pal.text_secondary
            };
            let sub_fit = fonts.ellipsize(sub, max_title_w, 11.5, false);
            painter.draw_text(
                sub_fit,
                rr.x as f32 + 14.0,
                rr.y as f32 + 31.0,
                11.5,
                false,
                sub_col,
            );
        }

        if let Some(val) = &row.trailing_value {
            let vw = fonts.measure_width(val, 12.0, false);
            let rx = rr.right() as f32 - if row.has_chevron { 26.0 } else { 14.0 } - vw;
            let tc = if accent_text {
                [255, 255, 255, 230]
            } else {
                pal.text_secondary
            };
            painter.draw_text(val.clone(), rx, title_y, 12.0, false, tc);
        }

        if row.has_chevron {
            let cx = rr.right() as f32 - 14.0;
            let cy = rr.y as f32 + rr.height as f32 * 0.5;
            let col = if accent_text {
                [255, 255, 255, 220]
            } else {
                pal.text_muted
            };
            painter.draw_line(cx - 3.0, cy - 4.0, cx + 1.0, cy, 1.3, col);
            painter.draw_line(cx + 1.0, cy, cx - 3.0, cy + 4.0, 1.3, col);
        }

        if self.in_card && idx + 1 < self.rows.len() {
            painter.fill_rect(
                Rect::new(rr.x + 14, rr.bottom() - 1, rr.width.saturating_sub(28), 1),
                pal.divider,
            );
        }
    }
}

fn paint_row_bg(
    painter: &mut UiPainter<'_>,
    pal: &ThemePalette,
    rr: Rect,
    is_sel: bool,
    is_hov: bool,
    mode: ListSelectionMode,
) {
    let pill = Rect::new(
        rr.x + 6,
        rr.y + 3,
        rr.width.saturating_sub(12),
        rr.height.saturating_sub(6),
    );
    if is_sel && mode != ListSelectionMode::None {
        let col = match mode {
            ListSelectionMode::AccentPill => pal.accent,
            ListSelectionMode::NeutralPill => pal.card_active_bg,
            ListSelectionMode::None => [0, 0, 0, 0],
        };
        painter.fill_rounded_rect(pill, 10.0, col);
    } else if is_hov {
        let col = if pal.is_dark {
            [255, 255, 255, 16]
        } else {
            [0, 0, 0, 12]
        };
        painter.fill_rounded_rect(pill, 10.0, col);
    }
}
