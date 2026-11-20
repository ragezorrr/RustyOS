//! Keyboard focus tracking and Tab-order navigation across client widgets (`RUSTYOS-130`).

use std::time::Duration;

use rusty_animation::{AnimatedValue, Easing};

/// Stable identifier for a focusable widget within a [`FocusScope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WidgetId(pub u64);

impl WidgetId {
    /// Creates an explicit numeric widget identifier.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Derives a deterministic 64-bit FNV-1a widget ID from a readable tag string.
    #[must_use]
    pub const fn from_tag(tag: &str) -> Self {
        let bytes = tag.as_bytes();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        Self(hash)
    }
}

/// Manages active focus, per-frame Tab traversal order, and animated focus ring opacity.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusScope {
    focused: Option<WidgetId>,
    tab_order: Vec<WidgetId>,
    ring_alpha: AnimatedValue,
}

impl Default for FocusScope {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusScope {
    /// Focus ring fade duration (`110 ms`).
    pub const RING_DURATION: Duration = Duration::from_millis(110);

    #[must_use]
    pub fn new() -> Self {
        Self {
            focused: None,
            tab_order: Vec::with_capacity(16),
            ring_alpha: AnimatedValue::new_settled(0.0),
        }
    }

    /// Clears the per-frame tab order before rebuilding controls for a layout pass.
    pub fn begin_frame(&mut self) {
        self.tab_order.clear();
    }

    /// Registers one enabled focusable control in layout/tab order (deduplicated).
    pub fn register(&mut self, id: WidgetId) {
        if !self.tab_order.contains(&id) {
            self.tab_order.push(id);
        }
    }

    /// Returns currently focused widget, if any.
    #[must_use]
    pub const fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// Returns `true` when `id` is currently focused.
    #[must_use]
    pub fn is_focused(&self, id: WidgetId) -> bool {
        self.focused == Some(id)
    }

    /// Moves focus directly to `id`.
    pub fn set_focus(&mut self, id: WidgetId, now: Duration, reduced_motion: bool) {
        if self.focused != Some(id) {
            self.focused = Some(id);
            self.ring_alpha = AnimatedValue::new_settled(0.0);
            self.ring_alpha.retarget(
                1.0,
                now,
                Self::RING_DURATION,
                Easing::EaseOutCubic,
                reduced_motion,
            );
        }
    }

    /// Clears focus cleanly (for example on `Escape` or outside click).
    pub fn clear_focus(&mut self, now: Duration, reduced_motion: bool) {
        if self.focused.take().is_some() {
            self.ring_alpha.retarget(
                0.0,
                now,
                Self::RING_DURATION,
                Easing::EaseOutCubic,
                reduced_motion,
            );
        }
    }

    /// Advances focus to the next (or previous if `reverse`) control in `tab_order`.
    pub fn cycle_focus(
        &mut self,
        reverse: bool,
        now: Duration,
        reduced_motion: bool,
    ) -> Option<WidgetId> {
        if self.tab_order.is_empty() {
            self.clear_focus(now, reduced_motion);
            return None;
        }
        let next_idx = match self
            .focused
            .and_then(|cur| self.tab_order.iter().position(|&id| id == cur))
        {
            None => {
                if reverse {
                    self.tab_order.len() - 1
                } else {
                    0
                }
            }
            Some(idx) => {
                if reverse {
                    (idx + self.tab_order.len() - 1) % self.tab_order.len()
                } else {
                    (idx + 1) % self.tab_order.len()
                }
            }
        };
        let next_id = self.tab_order[next_idx];
        self.set_focus(next_id, now, reduced_motion);
        Some(next_id)
    }

    /// Advances focus ring fade animation. Returns `true` while still animating.
    pub fn tick(&mut self, now: Duration) -> bool {
        let was = self.ring_alpha.is_active();
        let _ = self.ring_alpha.tick(now);
        was || self.ring_alpha.is_active()
    }

    /// Animated focus ring opacity in `[0.0, 1.0]`.
    #[must_use]
    pub const fn ring_alpha(&self) -> f32 {
        self.ring_alpha.current() as f32
    }
}
