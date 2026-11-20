//! Control transition and scroll motion helpers backed by `rusty-animation` (`RUSTYOS-130`).

use std::time::Duration;

use rusty_animation::{AnimatedValue, Easing};

/// Interruptible toggle track/thumb motion (`0.0 <-> 1.0`, `120 ms` `EaseOutCubic`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToggleMotion {
    value: AnimatedValue,
    target_on: bool,
}

impl ToggleMotion {
    /// Standard switch transition duration (`120 ms`).
    pub const DURATION: Duration = Duration::from_millis(120);

    #[must_use]
    pub const fn new(active: bool) -> Self {
        let v = if active { 1.0 } else { 0.0 };
        Self {
            value: AnimatedValue::new_settled(v),
            target_on: active,
        }
    }

    /// Retargets switch state toward `active` (snaps immediately if `reduced_motion`).
    pub fn set_active(&mut self, active: bool, now: Duration, reduced_motion: bool) {
        if self.target_on != active {
            self.target_on = active;
            let target = if active { 1.0 } else { 0.0 };
            self.value.retarget(
                target,
                now,
                Self::DURATION,
                Easing::EaseOutCubic,
                reduced_motion,
            );
        }
    }

    /// Advances animation state; returns `true` while still transitioning.
    pub fn tick(&mut self, now: Duration) -> bool {
        let was = self.value.is_active();
        let _ = self.value.tick(now);
        was || self.value.is_active()
    }

    /// Normalized thumb progress in `[0.0, 1.0]`.
    #[must_use]
    pub const fn progress(&self) -> f32 {
        self.value.current() as f32
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.value.is_active()
    }
}

/// Smooth hover and press highlight transition for interactive controls (`120 ms`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoverPressMotion {
    hover: AnimatedValue,
    press: AnimatedValue,
    target_hover: bool,
    target_press: bool,
}

impl Default for HoverPressMotion {
    fn default() -> Self {
        Self::new()
    }
}

impl HoverPressMotion {
    pub const DURATION: Duration = Duration::from_millis(120);

    #[must_use]
    pub const fn new() -> Self {
        Self {
            hover: AnimatedValue::new_settled(0.0),
            press: AnimatedValue::new_settled(0.0),
            target_hover: false,
            target_press: false,
        }
    }

    pub fn update_state(
        &mut self,
        hovered: bool,
        pressed: bool,
        now: Duration,
        reduced_motion: bool,
    ) {
        if self.target_hover != hovered {
            self.target_hover = hovered;
            self.hover.retarget(
                if hovered { 1.0 } else { 0.0 },
                now,
                Self::DURATION,
                Easing::EaseOutCubic,
                reduced_motion,
            );
        }
        if self.target_press != pressed {
            self.target_press = pressed;
            self.press.retarget(
                if pressed { 1.0 } else { 0.0 },
                now,
                Duration::from_millis(80),
                Easing::EaseOutCubic,
                reduced_motion,
            );
        }
    }

    pub fn tick(&mut self, now: Duration) -> bool {
        let was_h = self.hover.is_active();
        let was_p = self.press.is_active();
        let _ = self.hover.tick(now);
        let _ = self.press.tick(now);
        was_h || was_p || self.hover.is_active() || self.press.is_active()
    }

    #[must_use]
    pub const fn hover_alpha(&self) -> f32 {
        self.hover.current() as f32
    }

    #[must_use]
    pub const fn press_alpha(&self) -> f32 {
        self.press.current() as f32
    }

    #[must_use]
    pub const fn is_animating(&self) -> bool {
        self.hover.is_active() || self.press.is_active()
    }
}

/// Refresh-rate independent scrollable offset interpolator (`160 ms` `EaseOutQuart`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMotion {
    offset: AnimatedValue,
    max_offset: f64,
}

impl Default for ScrollMotion {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollMotion {
    pub const DURATION: Duration = Duration::from_millis(160);

    #[must_use]
    pub const fn new() -> Self {
        Self {
            offset: AnimatedValue::new_settled(0.0),
            max_offset: 0.0,
        }
    }

    /// Updates maximum scroll extent and clamps target & current offsets.
    pub fn set_max_offset(&mut self, max_offset: f64) {
        self.max_offset = max_offset.max(0.0);
        if self.offset.target() > self.max_offset {
            self.offset = AnimatedValue::new_settled(self.max_offset);
        }
    }

    /// Immediately jumps to `target` without interpolation (used when dragging scrollbar thumb).
    pub fn snap_to(&mut self, target: f64) {
        self.offset = AnimatedValue::new_settled(target.clamp(0.0, self.max_offset));
    }

    /// Smoothly scrolls by `delta` relative to current target.
    pub fn scroll_by(&mut self, delta: f64, now: Duration, reduced_motion: bool) {
        let target = (self.offset.target() + delta).clamp(0.0, self.max_offset);
        self.scroll_to(target, now, reduced_motion);
    }

    /// Smoothly animates toward absolute `target`.
    pub fn scroll_to(&mut self, target: f64, now: Duration, reduced_motion: bool) {
        let clamped = target.clamp(0.0, self.max_offset);
        self.offset.retarget(
            clamped,
            now,
            Self::DURATION,
            Easing::EaseOutQuart,
            reduced_motion,
        );
    }

    pub fn tick(&mut self, now: Duration) -> bool {
        let was = self.offset.is_active();
        let _ = self.offset.tick(now);
        was || self.offset.is_active()
    }

    #[must_use]
    pub const fn current(&self) -> f64 {
        self.offset.current()
    }

    #[must_use]
    pub const fn target(&self) -> f64 {
        self.offset.target()
    }

    #[must_use]
    pub const fn max_offset(&self) -> f64 {
        self.max_offset
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.offset.is_active()
    }
}

/// Deterministic caret blink timer that stays visible immediately after typing or focus changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaretBlink {
    last_activity: Duration,
    half_period: Duration,
}

impl Default for CaretBlink {
    fn default() -> Self {
        Self::new()
    }
}

impl CaretBlink {
    /// Standard cursor blink half-period (`530 ms`).
    pub const HALF_PERIOD: Duration = Duration::from_millis(530);

    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_activity: Duration::ZERO,
            half_period: Self::HALF_PERIOD,
        }
    }

    /// Resets blink phase so the caret remains visible for at least one half-period.
    pub const fn reset(&mut self, now: Duration) {
        self.last_activity = now;
    }

    /// Returns whether the caret should be rendered at monotonic timestamp `now`.
    #[must_use]
    pub fn is_visible(&self, now: Duration) -> bool {
        let elapsed = now.saturating_sub(self.last_activity).as_millis();
        let period = self.half_period.as_millis().max(1);
        (elapsed / period).is_multiple_of(2)
    }
}
