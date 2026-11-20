//! Unified interactive controls: `DropdownSelect`, `SegmentedTabs`, `Checkbox`, and `Slider` (`RUSTYOS-130`).

use std::time::Duration;

use rusty_animation::{AnimatedValue, Easing};

use crate::draw::UiPainter;
use crate::geometry::Rect;
use crate::menu::{MENU_RADIUS, MenuEntry, MenuItem, MenuLayout, paint_menu_with_painter};
use crate::shadow::ShadowElevation;
use crate::text::UiFontEngine;
use crate::theme::ThemePalette;

/// Keyboard event abstraction for standard discrete controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Enter,
    Space,
    Escape,
}

/// Interactive single-choice dropdown selector backed by [`MenuLayout`] for its popup list.
#[derive(Debug, Clone, PartialEq)]
pub struct DropdownSelect {
    pub rect: Rect,
    pub options: Vec<String>,
    pub selected: usize,
    pub open: bool,
    pub hovered_trigger: bool,
    pub hovered_option: Option<usize>,
    pub disabled: bool,
}

impl DropdownSelect {
    #[must_use]
    pub fn new(rect: Rect, options: Vec<String>, selected: usize) -> Self {
        let clamped = if options.is_empty() {
            0
        } else {
            selected.min(options.len() - 1)
        };
        Self {
            rect,
            options,
            selected: clamped,
            open: false,
            hovered_trigger: false,
            hovered_option: None,
            disabled: false,
        }
    }

    #[must_use]
    pub fn selected_label(&self) -> &str {
        self.options.get(self.selected).map_or("", String::as_str)
    }

    /// Computes popup [`MenuLayout`] anchored neatly beneath the trigger capsule.
    #[must_use]
    pub fn popup_layout(&self) -> MenuLayout {
        let entries: Vec<MenuEntry<usize>> = self
            .options
            .iter()
            .enumerate()
            .map(|(idx, label)| {
                MenuEntry::Item(
                    MenuItem::new(label.clone(), idx).with_checked(idx == self.selected),
                )
            })
            .collect();
        MenuLayout::compute(
            self.rect.x,
            self.rect.bottom() + 4,
            self.rect.width as i32,
            &entries,
        )
    }

    pub fn on_pointer_down(&mut self, px: f64, py: f64) -> Option<usize> {
        if self.disabled {
            return None;
        }
        if self.open {
            let layout = self.popup_layout();
            if let Some(row_idx) = layout.hit_test(px, py) {
                self.selected = row_idx.min(self.options.len().saturating_sub(1));
                self.open = false;
                self.hovered_option = None;
                return Some(self.selected);
            }
            if !self.rect.contains_f64(px, py) {
                self.open = false;
                self.hovered_option = None;
                return None;
            }
        }
        if self.rect.contains_f64(px, py) {
            self.open = !self.open;
            self.hovered_option = self.open.then_some(self.selected);
        }
        None
    }

    pub fn on_pointer_move(&mut self, px: f64, py: f64) -> bool {
        let prev_t = self.hovered_trigger;
        let prev_o = self.hovered_option;
        self.hovered_trigger = !self.disabled && self.rect.contains_f64(px, py);
        self.hovered_option = if self.open {
            self.popup_layout().hit_test(px, py)
        } else {
            None
        };
        prev_t != self.hovered_trigger || prev_o != self.hovered_option
    }

    /// Handles keyboard opening, navigation, selection, or dismissal; returns newly chosen index.
    pub fn handle_key(&mut self, key: ControlKey) -> Option<usize> {
        if self.disabled || self.options.is_empty() {
            return None;
        }
        match key {
            ControlKey::Escape if self.open => {
                self.open = false;
                self.hovered_option = None;
                None
            }
            ControlKey::Space | ControlKey::Enter => {
                if self.open {
                    if let Some(idx) = self.hovered_option {
                        self.selected = idx.min(self.options.len() - 1);
                    }
                    self.open = false;
                    self.hovered_option = None;
                    Some(self.selected)
                } else {
                    self.open = true;
                    self.hovered_option = Some(self.selected);
                    None
                }
            }
            ControlKey::Up if self.open => {
                let cur = self.hovered_option.unwrap_or(self.selected);
                self.hovered_option = Some(if cur == 0 {
                    self.options.len() - 1
                } else {
                    cur - 1
                });
                None
            }
            ControlKey::Down if self.open => {
                let cur = self.hovered_option.unwrap_or(self.selected);
                self.hovered_option = Some((cur + 1) % self.options.len());
                None
            }
            ControlKey::Up | ControlKey::Left if !self.open => {
                self.selected = if self.selected == 0 {
                    self.options.len() - 1
                } else {
                    self.selected - 1
                };
                Some(self.selected)
            }
            ControlKey::Down | ControlKey::Right if !self.open => {
                self.selected = (self.selected + 1) % self.options.len();
                Some(self.selected)
            }
            _ => None,
        }
    }

    /// Draws the collapsed trigger capsule (without open popup overlay).
    pub fn paint_trigger(
        &self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        pal: &ThemePalette,
        focused: bool,
        focus_alpha: f32,
    ) {
        let bg = if pal.is_dark {
            if self.hovered_trigger {
                [0x40, 0x40, 0x44, 255]
            } else {
                [0x34, 0x34, 0x38, 255]
            }
        } else if self.hovered_trigger {
            [0xE6, 0xE7, 0xEB, 255]
        } else {
            [0xEE, 0xEF, 0xF3, 255]
        };
        painter.rounded_rect_styled(self.rect, 8.0, bg, 1.0, pal.divider, false);
        if focused {
            painter.draw_focus_ring(self.rect, 8.0, pal.accent, focus_alpha.max(0.9));
        }

        let text_budget = (self.rect.width as f32 - 32.0).max(12.0);
        let label = fonts.ellipsize(self.selected_label(), text_budget, 12.5, false);
        let baseline = self.rect.y as f32 + f32::midpoint(self.rect.height as f32, 12.5 * 0.72);
        let text_col = if self.disabled {
            pal.text_muted
        } else {
            pal.text_primary
        };
        painter.draw_text(
            label,
            self.rect.x as f32 + 10.0,
            baseline,
            12.5,
            false,
            text_col,
        );

        // Compact double chevron badge on trailing edge.
        let cx = self.rect.right() as f32 - 12.0;
        let cy = self.rect.y as f32 + self.rect.height as f32 * 0.5;
        painter.draw_line(cx - 3.0, cy - 2.0, cx, cy - 5.0, 1.3, pal.text_secondary);
        painter.draw_line(cx, cy - 5.0, cx + 3.0, cy - 2.0, 1.3, pal.text_secondary);
        painter.draw_line(cx - 3.0, cy + 2.0, cx, cy + 5.0, 1.3, pal.text_secondary);
        painter.draw_line(cx, cy + 5.0, cx + 3.0, cy + 2.0, 1.3, pal.text_secondary);
    }

    /// Draws the expanded dropdown menu overlay on top of content (`ShadowElevation::MenuOrTooltip`).
    pub fn paint_popup(
        &self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        pal: &ThemePalette,
    ) {
        if !self.open {
            return;
        }
        let layout = self.popup_layout();
        painter.draw_shadow(
            layout.rect,
            MENU_RADIUS,
            ShadowElevation::MenuOrTooltip,
            pal.is_dark,
        );
        let entries: Vec<MenuEntry<usize>> = self
            .options
            .iter()
            .enumerate()
            .map(|(idx, label)| {
                MenuEntry::Item(
                    MenuItem::new(label.clone(), idx).with_checked(idx == self.selected),
                )
            })
            .collect();
        paint_menu_with_painter(
            painter,
            fonts,
            &layout,
            &entries,
            self.hovered_option,
            pal,
            false,
        );
    }
}

/// Animated segmented tab bar (`Tabs` / segmented mode selector) backed by `rusty-animation`.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentedTabs {
    pub rect: Rect,
    pub labels: Vec<String>,
    pub selected: usize,
    pub hovered: Option<usize>,
    indicator_pos: AnimatedValue,
}

impl SegmentedTabs {
    pub const DURATION: Duration = Duration::from_millis(140);

    #[must_use]
    pub fn new(rect: Rect, labels: Vec<String>, selected: usize) -> Self {
        let idx = if labels.is_empty() {
            0
        } else {
            selected.min(labels.len() - 1)
        };
        Self {
            rect,
            labels,
            selected: idx,
            hovered: None,
            indicator_pos: AnimatedValue::new_settled(idx as f64),
        }
    }

    pub fn select(&mut self, index: usize, now: Duration, reduced_motion: bool) -> bool {
        if self.labels.is_empty() || index >= self.labels.len() || self.selected == index {
            return false;
        }
        self.selected = index;
        self.indicator_pos.retarget(
            index as f64,
            now,
            Self::DURATION,
            Easing::EaseOutCubic,
            reduced_motion,
        );
        true
    }

    pub fn tick(&mut self, now: Duration) -> bool {
        let was = self.indicator_pos.is_active();
        let _ = self.indicator_pos.tick(now);
        was || self.indicator_pos.is_active()
    }

    #[must_use]
    pub fn segment_rect(&self, idx: usize) -> Option<Rect> {
        if idx >= self.labels.len() {
            return None;
        }
        let n = self.labels.len() as i32;
        let inner_w = (self.rect.width as i32 - 4).max(n);
        let seg_w = inner_w / n;
        let sx = self.rect.x + 2 + idx as i32 * seg_w;
        let w = if idx + 1 == self.labels.len() {
            (self.rect.right() - 2 - sx).max(1)
        } else {
            seg_w.max(1)
        };
        Some(Rect::new(
            sx,
            self.rect.y + 2,
            w as u32,
            self.rect.height.saturating_sub(4).max(1),
        ))
    }

    #[must_use]
    pub fn hit_test(&self, px: f64, py: f64) -> Option<usize> {
        if !self.rect.contains_f64(px, py) || self.labels.is_empty() {
            return None;
        }
        for idx in 0..self.labels.len() {
            if let Some(sr) = self.segment_rect(idx)
                && sr.contains_f64(px, py)
            {
                return Some(idx);
            }
        }
        None
    }

    pub fn handle_key(
        &mut self,
        key: ControlKey,
        now: Duration,
        reduced_motion: bool,
    ) -> Option<usize> {
        if self.labels.is_empty() {
            return None;
        }
        let target = match key {
            ControlKey::Left => Some(if self.selected == 0 {
                self.labels.len() - 1
            } else {
                self.selected - 1
            }),
            ControlKey::Right => Some((self.selected + 1) % self.labels.len()),
            ControlKey::Home => Some(0),
            ControlKey::End => Some(self.labels.len() - 1),
            _ => None,
        }?;
        self.select(target, now, reduced_motion).then_some(target)
    }

    pub fn paint(
        &self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        pal: &ThemePalette,
        focused: bool,
        focus_alpha: f32,
    ) {
        let track_bg = if pal.is_dark {
            [0x24, 0x24, 0x27, 255]
        } else {
            [0xE6, 0xE7, 0xEB, 255]
        };
        let radius = (self.rect.height as f32 * 0.5).min(10.0);
        painter.fill_rounded_rect(self.rect, radius, track_bg);
        if focused {
            painter.draw_focus_ring(self.rect, radius, pal.accent, focus_alpha.max(0.9));
        }

        if !self.labels.is_empty() {
            let inner_w = (self.rect.width as f32 - 4.0).max(1.0);
            let seg_w = inner_w / self.labels.len() as f32;
            let pill_x = self.rect.x as f32 + 2.0 + self.indicator_pos.current() as f32 * seg_w;
            let pill_rect = Rect::new(
                pill_x.round() as i32,
                self.rect.y + 2,
                seg_w.round().max(1.0) as u32,
                self.rect.height.saturating_sub(4).max(1),
            );
            let thumb_bg = if pal.is_dark {
                [0x3E, 0x3E, 0x43, 255]
            } else {
                [255, 255, 255, 255]
            };
            painter.fill_rounded_rect(pill_rect, (radius - 2.0).max(4.0), thumb_bg);
        }

        for (idx, label) in self.labels.iter().enumerate() {
            if let Some(sr) = self.segment_rect(idx) {
                let col = if idx == self.selected {
                    pal.text_primary
                } else {
                    pal.text_secondary
                };
                painter.draw_text_centered(
                    fonts,
                    label.clone(),
                    sr,
                    12.5,
                    idx == self.selected,
                    col,
                );
            }
        }
    }
}

/// Standard rounded checkbox control with vector checkmark and focus ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkbox {
    pub box_rect: Rect,
    pub hit_rect: Rect,
    pub label: String,
    pub checked: bool,
    pub hovered: bool,
    pub disabled: bool,
}

impl Checkbox {
    #[must_use]
    pub fn new(x: i32, y: i32, label: impl Into<String>, checked: bool) -> Self {
        let label = label.into();
        let box_rect = Rect::new(x, y, 16, 16);
        let hit_w = 24 + (label.chars().count() as i32 * 8).max(40);
        Self {
            box_rect,
            hit_rect: Rect::new(x, y - 2, hit_w as u32, 20),
            label,
            checked,
            hovered: false,
            disabled: false,
        }
    }

    pub fn on_pointer_down(&mut self, px: f64, py: f64) -> Option<bool> {
        if !self.disabled && self.hit_rect.contains_f64(px, py) {
            self.checked = !self.checked;
            Some(self.checked)
        } else {
            None
        }
    }

    pub fn handle_key(&mut self, key: ControlKey) -> Option<bool> {
        if !self.disabled && matches!(key, ControlKey::Space | ControlKey::Enter) {
            self.checked = !self.checked;
            Some(self.checked)
        } else {
            None
        }
    }

    pub fn paint(
        &self,
        painter: &mut UiPainter<'_>,
        pal: &ThemePalette,
        focused: bool,
        focus_alpha: f32,
    ) {
        if self.checked {
            painter.fill_rounded_rect(self.box_rect, 4.5, pal.accent);
            let x = self.box_rect.x as f32;
            let y = self.box_rect.y as f32;
            let white = [255, 255, 255, 255];
            painter.draw_line(x + 4.0, y + 8.5, x + 6.8, y + 11.4, 1.7, white);
            painter.draw_line(x + 6.8, y + 11.4, x + 12.2, y + 5.0, 1.7, white);
        } else {
            let bg = if pal.is_dark {
                [0x2C, 0x2C, 0x30, 255]
            } else {
                [255, 255, 255, 255]
            };
            painter.rounded_rect_styled(self.box_rect, 4.5, bg, 1.0, pal.divider, false);
        }
        if focused {
            painter.draw_focus_ring(self.box_rect, 4.5, pal.accent, focus_alpha.max(0.9));
        }
        if !self.label.is_empty() {
            let tx = self.box_rect.right() as f32 + 8.0;
            let baseline = self.box_rect.y as f32 + 12.5;
            painter.draw_text(
                self.label.clone(),
                tx,
                baseline,
                13.0,
                false,
                if self.disabled {
                    pal.text_muted
                } else {
                    pal.text_primary
                },
            );
        }
    }
}

/// Horizontal continuous value slider (`0.0..=1.0`) with thumb drag and arrow key stepping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slider {
    pub rect: Rect,
    pub value: f32,
    pub step: f32,
    pub dragging: bool,
    pub hovered: bool,
}

impl Slider {
    #[must_use]
    pub const fn new(rect: Rect, value: f32) -> Self {
        Self {
            rect,
            value: value.clamp(0.0, 1.0),
            step: 0.05,
            dragging: false,
            hovered: false,
        }
    }

    fn value_at_x(&self, px: f64) -> f32 {
        let r = 8.0_f64;
        let start = f64::from(self.rect.x) + r;
        let span = (f64::from(self.rect.width) - r * 2.0).max(1.0);
        ((px - start) / span).clamp(0.0, 1.0) as f32
    }

    pub fn on_pointer_down(&mut self, px: f64, py: f64) -> Option<f32> {
        if !self.rect.contains_f64(px, py) {
            return None;
        }
        self.dragging = true;
        self.value = self.value_at_x(px);
        Some(self.value)
    }

    pub fn on_pointer_move(&mut self, px: f64, py: f64) -> Option<f32> {
        self.hovered = self.rect.contains_f64(px, py);
        if self.dragging {
            let next = self.value_at_x(px);
            if (next - self.value).abs() > 1e-4 {
                self.value = next;
                return Some(self.value);
            }
        }
        None
    }

    pub fn on_pointer_up(&mut self) -> bool {
        let was = self.dragging;
        self.dragging = false;
        was
    }

    pub fn handle_key(&mut self, key: ControlKey) -> Option<f32> {
        let delta = match key {
            ControlKey::Left | ControlKey::Down => -self.step,
            ControlKey::Right | ControlKey::Up => self.step,
            ControlKey::Home => -1.0,
            ControlKey::End => 1.0,
            _ => return None,
        };
        let next = (self.value + delta).clamp(0.0, 1.0);
        if (next - self.value).abs() > 1e-4 {
            self.value = next;
            Some(self.value)
        } else {
            None
        }
    }

    pub fn paint(
        &self,
        painter: &mut UiPainter<'_>,
        pal: &ThemePalette,
        focused: bool,
        focus_alpha: f32,
    ) {
        let cy = self.rect.y + self.rect.height as i32 / 2;
        let track_h = 4_u32;
        let r = 8_i32;
        let track_x = self.rect.x + r;
        let track_w = (self.rect.width as i32 - r * 2).max(1) as u32;
        let track_bg = if pal.is_dark {
            [0x3B, 0x3B, 0x40, 255]
        } else {
            [0xDC, 0xDD, 0xE2, 255]
        };
        painter.fill_rounded_rect(Rect::new(track_x, cy - 2, track_w, track_h), 2.0, track_bg);
        let fill_w = (track_w as f32 * self.value).round().max(0.0) as u32;
        if fill_w > 0 {
            painter.fill_rounded_rect(Rect::new(track_x, cy - 2, fill_w, track_h), 2.0, pal.accent);
        }

        let thumb_cx = track_x as f32 + track_w as f32 * self.value;
        let thumb_cy = cy as f32;
        if focused {
            painter.stroke_circle(
                thumb_cx,
                thumb_cy,
                10.0,
                2.0,
                [
                    pal.accent[0],
                    pal.accent[1],
                    pal.accent[2],
                    (190.0 * focus_alpha.max(0.9)).round() as u8,
                ],
            );
        }
        painter.fill_circle(thumb_cx, thumb_cy + 0.7, 7.5, [0, 0, 0, 36]);
        painter.fill_circle(thumb_cx, thumb_cy, 7.2, [255, 255, 255, 255]);
    }
}
