//! `RustyOS` refresh-rate independent animation engine (`RUSTYOS-008`, ADR-0004).
//!
//! Provides monotonic time progression, macOS-inspired easing curves, interruptible
//! and reversible transitions (`retarget` / `reverse` / `cancel`), and first-class
//! `reduced_motion` support without heap allocations on the hot rendering path.

use std::collections::BTreeMap;
use std::time::Duration;

/// Canonical motion design tokens from `DESIGN.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionTokens {
    /// Whether reduced-motion accessibility mode is enabled.
    pub reduced_motion: bool,
    /// Short transition duration (`120 ms` by default, e.g. focus/hover/quick snap).
    pub short_duration: Duration,
    /// Standard transition duration (`180 ms` by default, e.g. window snap/maximize/tiling).
    pub standard_duration: Duration,
    /// Long transition duration (`260 ms` by default, e.g. workspace slide/overview).
    pub long_duration: Duration,
}

impl Default for MotionTokens {
    fn default() -> Self {
        Self {
            reduced_motion: false,
            short_duration: Duration::from_millis(120),
            standard_duration: Duration::from_millis(180),
            long_duration: Duration::from_millis(260),
        }
    }
}

impl MotionTokens {
    /// Creates motion tokens with the specified `reduced_motion` preference.
    #[must_use]
    pub fn with_reduced_motion(reduced_motion: bool) -> Self {
        Self {
            reduced_motion,
            ..Self::default()
        }
    }
}

/// Mathematical easing curves mapping normalized time `t in [0.0, 1.0]` to progress `[0.0, 1.0]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Easing {
    /// Constant velocity interpolation (`f(t) = t`).
    Linear,
    /// Decelerating cubic curve (`1 - (1 - t)^3`), ideal for window snap and placement.
    #[default]
    EaseOutCubic,
    /// Symmetric acceleration and deceleration cubic curve, ideal for workspace slides.
    EaseInOutCubic,
    /// Smooth, fast-out quartic curve (`1 - (1 - t)^4`) inspired by macOS window transitions.
    EaseOutQuart,
}

impl Easing {
    /// Evaluates the easing curve for `t` clamped to `[0.0, 1.0]`.
    #[must_use]
    pub fn sample(self, t: f64) -> f64 {
        let clamped = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => clamped,
            Self::EaseOutCubic => {
                let inv = 1.0 - clamped;
                1.0 - (inv * inv * inv)
            }
            Self::EaseInOutCubic => {
                if clamped < 0.5 {
                    4.0 * clamped * clamped * clamped
                } else {
                    let inv = -2.0 * clamped + 2.0;
                    1.0 - (inv * inv * inv) / 2.0
                }
            }
            Self::EaseOutQuart => {
                let inv = 1.0 - clamped;
                let sq = inv * inv;
                1.0 - (sq * sq)
            }
        }
    }
}

/// A scalar value animated over monotonic time (`Duration` since compositor startup).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimatedValue {
    start_val: f64,
    current_val: f64,
    target_val: f64,
    start_time: Duration,
    duration: Duration,
    easing: Easing,
    active: bool,
}

impl AnimatedValue {
    /// Creates a stationary [`AnimatedValue`] initialized at `initial`.
    #[must_use]
    pub const fn new_settled(initial: f64) -> Self {
        Self {
            start_val: initial,
            current_val: initial,
            target_val: initial,
            start_time: Duration::ZERO,
            duration: Duration::ZERO,
            easing: Easing::EaseOutCubic,
            active: false,
        }
    }

    /// Returns the current interpolated value.
    #[must_use]
    pub const fn current(&self) -> f64 {
        self.current_val
    }

    /// Returns the target value.
    #[must_use]
    pub const fn target(&self) -> f64 {
        self.target_val
    }

    /// Returns `true` if the transition is currently in flight.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Starts or retargets the animation toward `new_target` starting from the current position.
    ///
    /// If `reduced_motion` is `true` or `duration` is zero, snaps immediately to `new_target`.
    pub fn retarget(
        &mut self,
        new_target: f64,
        now: Duration,
        duration: Duration,
        easing: Easing,
        reduced_motion: bool,
    ) {
        if self.active {
            let _ = self.tick(now);
        }

        if reduced_motion || duration.is_zero() || (new_target - self.current_val).abs() < 0.01 {
            self.start_val = new_target;
            self.current_val = new_target;
            self.target_val = new_target;
            self.duration = Duration::ZERO;
            self.active = false;
            return;
        }

        self.start_val = self.current_val;
        self.target_val = new_target;
        self.start_time = now;
        self.duration = duration;
        self.easing = easing;
        self.active = true;
    }

    /// Reverses an in-flight transition back toward `start_val`, scaled to the elapsed fraction.
    pub fn reverse(&mut self, now: Duration, reduced_motion: bool) {
        if !self.active {
            return;
        }
        let _ = self.tick(now);
        let elapsed = now.saturating_sub(self.start_time);
        let reverse_duration = elapsed.min(self.duration);
        let previous_start = self.start_val;
        self.retarget(
            previous_start,
            now,
            reverse_duration,
            self.easing,
            reduced_motion,
        );
    }

    /// Cancels the animation immediately at its current value.
    pub fn cancel_at_current(&mut self) {
        self.start_val = self.current_val;
        self.target_val = self.current_val;
        self.active = false;
    }

    /// Snaps the value immediately to `target_val`.
    pub fn snap_to_target(&mut self) {
        self.start_val = self.target_val;
        self.current_val = self.target_val;
        self.active = false;
    }

    /// Advances the animation to monotonic timestamp `now` and returns the updated value.
    pub fn tick(&mut self, now: Duration) -> f64 {
        if !self.active {
            return self.current_val;
        }

        let elapsed = now.saturating_sub(self.start_time);
        if elapsed >= self.duration || self.duration.is_zero() {
            self.current_val = self.target_val;
            self.active = false;
            return self.current_val;
        }

        let raw_t = elapsed.as_secs_f64() / self.duration.as_secs_f64();
        let eased = self.easing.sample(raw_t);
        self.current_val = self.start_val + (self.target_val - self.start_val) * eased;
        self.current_val
    }
}

/// Integer rectangle `(x, y, width, height)` in logical compositor coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicalRect {
    /// Left X coordinate.
    pub x: i32,
    /// Top Y coordinate.
    pub y: i32,
    /// Width in logical pixels.
    pub width: i32,
    /// Height in logical pixels.
    pub height: i32,
}

impl LogicalRect {
    /// Constructs a new [`LogicalRect`].
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Smoothly animated window geometry rectangle (`x`, `y`, `width`, `height`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimatedRect {
    x: AnimatedValue,
    y: AnimatedValue,
    width: AnimatedValue,
    height: AnimatedValue,
}

impl AnimatedRect {
    /// Creates a settled [`AnimatedRect`] at `initial`.
    #[must_use]
    pub fn new_settled(initial: LogicalRect) -> Self {
        Self {
            x: AnimatedValue::new_settled(f64::from(initial.x)),
            y: AnimatedValue::new_settled(f64::from(initial.y)),
            width: AnimatedValue::new_settled(f64::from(initial.width)),
            height: AnimatedValue::new_settled(f64::from(initial.height)),
        }
    }

    /// Retargets all four components toward `target`.
    pub fn retarget(
        &mut self,
        target: LogicalRect,
        now: Duration,
        duration: Duration,
        easing: Easing,
        reduced_motion: bool,
    ) {
        self.x
            .retarget(f64::from(target.x), now, duration, easing, reduced_motion);
        self.y
            .retarget(f64::from(target.y), now, duration, easing, reduced_motion);
        self.width.retarget(
            f64::from(target.width),
            now,
            duration,
            easing,
            reduced_motion,
        );
        self.height.retarget(
            f64::from(target.height),
            now,
            duration,
            easing,
            reduced_motion,
        );
    }

    /// Advances all four components to monotonic timestamp `now` and returns the rounded [`LogicalRect`].
    pub fn tick(&mut self, now: Duration) -> LogicalRect {
        let x = round_to_i32(self.x.tick(now));
        let y = round_to_i32(self.y.tick(now));
        let width = round_to_i32(self.width.tick(now)).max(1);
        let height = round_to_i32(self.height.tick(now)).max(1);
        LogicalRect::new(x, y, width, height)
    }

    /// Returns the current interpolated [`LogicalRect`] without advancing time.
    #[must_use]
    pub fn current(&self) -> LogicalRect {
        LogicalRect::new(
            round_to_i32(self.x.current()),
            round_to_i32(self.y.current()),
            round_to_i32(self.width.current()).max(1),
            round_to_i32(self.height.current()).max(1),
        )
    }

    /// Returns the target [`LogicalRect`].
    #[must_use]
    pub fn target(&self) -> LogicalRect {
        LogicalRect::new(
            round_to_i32(self.x.target()),
            round_to_i32(self.y.target()),
            round_to_i32(self.width.target()).max(1),
            round_to_i32(self.height.target()).max(1),
        )
    }

    /// Returns `true` if any component is still animating.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.x.is_active()
            || self.y.is_active()
            || self.width.is_active()
            || self.height.is_active()
    }

    /// Snaps all components immediately to their target values.
    pub fn snap_to_target(&mut self) {
        self.x.snap_to_target();
        self.y.snap_to_target();
        self.width.snap_to_target();
        self.height.snap_to_target();
    }
}

/// Central coordinator for window geometry transitions and workspace slide animations.
#[derive(Debug, Clone)]
pub struct AnimationEngine {
    tokens: MotionTokens,
    windows: BTreeMap<u64, AnimatedRect>,
    workspace_slide_x: AnimatedValue,
}

impl Default for AnimationEngine {
    fn default() -> Self {
        Self::new(MotionTokens::default())
    }
}

impl AnimationEngine {
    /// Creates a new [`AnimationEngine`] with the given [`MotionTokens`].
    #[must_use]
    pub const fn new(tokens: MotionTokens) -> Self {
        Self {
            tokens,
            windows: BTreeMap::new(),
            workspace_slide_x: AnimatedValue::new_settled(0.0),
        }
    }

    /// Returns the active [`MotionTokens`].
    #[must_use]
    pub const fn tokens(&self) -> MotionTokens {
        self.tokens
    }

    /// Returns `true` if `reduced_motion` mode is currently enabled.
    #[must_use]
    pub const fn reduced_motion(&self) -> bool {
        self.tokens.reduced_motion
    }

    /// Sets `reduced_motion` mode. When enabled, any in-flight animations immediately snap to target.
    pub fn set_reduced_motion(&mut self, enabled: bool) {
        self.tokens.reduced_motion = enabled;
        if enabled {
            for rect in self.windows.values_mut() {
                rect.snap_to_target();
            }
            self.workspace_slide_x.snap_to_target();
        }
    }

    /// Toggles `reduced_motion` mode and returns the new state.
    pub fn toggle_reduced_motion(&mut self) -> bool {
        let next = !self.tokens.reduced_motion;
        self.set_reduced_motion(next);
        next
    }

    /// Sets or retargets the animated geometry for `window_id`.
    ///
    /// First-time registration settles at `target` immediately (or animates if `animate_initial` is desired),
    /// while subsequent changes smoothly transition over `standard_duration` with [`Easing::EaseOutQuart`].
    pub fn set_window_target(&mut self, window_id: u64, target: LogicalRect, now: Duration) {
        if let Some(anim) = self.windows.get_mut(&window_id) {
            if anim.target() != target {
                anim.retarget(
                    target,
                    now,
                    self.tokens.standard_duration,
                    Easing::EaseOutQuart,
                    self.tokens.reduced_motion,
                );
            }
        } else {
            self.windows
                .insert(window_id, AnimatedRect::new_settled(target));
        }
    }

    /// Immediately snaps `window_id` to `target` without easing (e.g. during interactive mouse drag).
    pub fn snap_window_immediate(&mut self, window_id: u64, target: LogicalRect) {
        self.windows
            .insert(window_id, AnimatedRect::new_settled(target));
    }

    /// Removes animation state for a closed window.
    pub fn remove_window(&mut self, window_id: u64) {
        self.windows.remove(&window_id);
    }

    /// Triggers a horizontal workspace slide transition (e.g. switching from `from_ws` to `to_ws`
    /// across an output of width `output_width`).
    pub fn trigger_workspace_slide(
        &mut self,
        from_ws: u8,
        to_ws: u8,
        output_width: i32,
        now: Duration,
    ) {
        if from_ws == to_ws || self.tokens.reduced_motion {
            self.workspace_slide_x.snap_to_target();
            return;
        }
        let direction = if to_ws > from_ws { 1.0 } else { -1.0 };
        let initial_offset = f64::from(output_width) * 0.18 * direction;
        self.workspace_slide_x = AnimatedValue::new_settled(initial_offset);
        self.workspace_slide_x.retarget(
            0.0,
            now,
            self.tokens.long_duration,
            Easing::EaseOutCubic,
            false,
        );
    }

    /// Advances all active window and workspace transitions to monotonic timestamp `now`.
    ///
    /// Returns `true` if at least one animation is still in flight after this tick.
    pub fn tick(&mut self, now: Duration) -> bool {
        let mut any_active = false;
        for rect in self.windows.values_mut() {
            let _ = rect.tick(now);
            if rect.is_active() {
                any_active = true;
            }
        }
        let _ = self.workspace_slide_x.tick(now);
        if self.workspace_slide_x.is_active() {
            any_active = true;
        }
        any_active
    }

    /// Returns `true` if any window or workspace animation is currently active.
    #[must_use]
    pub fn has_active_animations(&self) -> bool {
        self.workspace_slide_x.is_active() || self.windows.values().any(AnimatedRect::is_active)
    }

    /// Returns the current animated placement (`x, y`) for `window_id`, including any active
    /// workspace slide offset.
    #[must_use]
    pub fn animated_window_origin(&self, window_id: u64) -> Option<(i32, i32)> {
        let rect = self.windows.get(&window_id)?.current();
        let slide = round_to_i32(self.workspace_slide_x.current());
        Some((rect.x.saturating_add(slide), rect.y))
    }

    /// Returns the current animated [`LogicalRect`] for `window_id` (with workspace slide applied to `x`).
    #[must_use]
    pub fn animated_window_rect(&self, window_id: u64) -> Option<LogicalRect> {
        let mut rect = self.windows.get(&window_id)?.current();
        let slide = round_to_i32(self.workspace_slide_x.current());
        rect.x = rect.x.saturating_add(slide);
        Some(rect)
    }
}

#[allow(clippy::cast_possible_truncation)]
fn round_to_i32(val: f64) -> i32 {
    val.round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves_satisfy_boundary_and_monotonicity_invariants() {
        for curve in [
            Easing::Linear,
            Easing::EaseOutCubic,
            Easing::EaseInOutCubic,
            Easing::EaseOutQuart,
        ] {
            assert!((curve.sample(0.0) - 0.0).abs() < 1e-9);
            assert!((curve.sample(1.0) - 1.0).abs() < 1e-9);

            let mut prev = 0.0;
            for step in 1..=100 {
                let t = f64::from(step) / 100.0;
                let current = curve.sample(t);
                assert!(current >= prev, "curve {curve:?} is non-monotonic at t={t}");
                prev = current;
            }
        }
    }

    #[test]
    fn refresh_rate_independence_across_60_120_144_240_hz_and_frame_drops() {
        for refresh_hz in [60_u64, 120, 144, 240] {
            let mut val = AnimatedValue::new_settled(0.0);
            let duration = Duration::from_millis(180);
            val.retarget(
                1000.0,
                Duration::ZERO,
                duration,
                Easing::EaseOutQuart,
                false,
            );

            let frame_step = Duration::from_nanos(1_000_000_000 / refresh_hz);
            let mut now = Duration::ZERO;
            while now < duration {
                now += frame_step;
                let _ = val.tick(now);
            }

            assert!(!val.is_active());
            assert!((val.current() - 1000.0).abs() < 1e-6);
        }

        // Simulate severe frame drop (jumping from 16ms straight to 200ms) -> still lands cleanly at target.
        let mut dropped = AnimatedValue::new_settled(100.0);
        dropped.retarget(
            500.0,
            Duration::ZERO,
            Duration::from_millis(180),
            Easing::EaseOutCubic,
            false,
        );
        let mid = dropped.tick(Duration::from_millis(16));
        assert!(mid > 100.0 && mid < 500.0);
        let end = dropped.tick(Duration::from_millis(200));
        assert!(!dropped.is_active());
        assert!((end - 500.0).abs() < 1e-6);
    }

    #[test]
    fn mid_flight_interruption_retarget_and_reversal_preserve_continuity() {
        let mut val = AnimatedValue::new_settled(0.0);
        val.retarget(
            400.0,
            Duration::ZERO,
            Duration::from_millis(200),
            Easing::Linear,
            false,
        );

        // At t = 100ms, value is at 200.0.
        let pos_at_100ms = val.tick(Duration::from_millis(100));
        assert!((pos_at_100ms - 200.0).abs() < 1e-5);

        // Interrupt at t = 100ms and retarget to 600.0: must start smoothly from 200.0 without jump.
        val.retarget(
            600.0,
            Duration::from_millis(100),
            Duration::from_millis(200),
            Easing::Linear,
            false,
        );
        assert!((val.current() - 200.0).abs() < 1e-5);

        // Advance 50ms (to t = 150ms) -> should be at 300.0, then reverse back toward 200.0.
        let pos_at_150ms = val.tick(Duration::from_millis(150));
        assert!((pos_at_150ms - 300.0).abs() < 1e-5);
        val.reverse(Duration::from_millis(150), false);
        assert!(val.is_active());
        assert!((val.target() - 200.0).abs() < 1e-5);

        let final_pos = val.tick(Duration::from_millis(200));
        assert!(!val.is_active());
        assert!((final_pos - 200.0).abs() < 1e-5);
    }

    #[test]
    fn reduced_motion_snaps_immediately_and_cancels_in_flight_transitions() {
        let mut engine = AnimationEngine::new(MotionTokens::with_reduced_motion(false));
        engine.set_window_target(1, LogicalRect::new(32, 32, 800, 600), Duration::ZERO);
        assert!(!engine.has_active_animations());

        // Retarget window 1 to left-half snap -> starts active animation.
        engine.set_window_target(
            1,
            LogicalRect::new(12, 12, 622, 776),
            Duration::from_millis(10),
        );
        assert!(engine.has_active_animations());

        // Enabling reduced motion mid-flight snaps immediately to target.
        engine.set_reduced_motion(true);
        assert!(!engine.has_active_animations());
        assert_eq!(
            engine.animated_window_rect(1),
            Some(LogicalRect::new(12, 12, 622, 776))
        );

        // Further transitions while reduced_motion is true settle instantaneously.
        engine.set_window_target(
            1,
            LogicalRect::new(646, 12, 622, 776),
            Duration::from_millis(20),
        );
        assert!(!engine.has_active_animations());
        assert_eq!(engine.animated_window_origin(1), Some((646, 12)));
    }
}
