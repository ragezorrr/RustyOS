//! Low-overhead frame timing, input-to-present latency, event-loop dispatch,
//! and damage-ratio instrumentation (`RUSTYOS-003` & `RUSTYOS-007`).
//!
//! Uses fixed-capacity ring buffers preallocated at startup so recording frame and
//! dispatch samples on the hot rendering path performs zero heap allocations and takes no locks.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// Default ring buffer capacity (4096 samples ~= 68 seconds at 60 Hz).
pub const DEFAULT_SAMPLE_CAPACITY: usize = 4096;

/// Single frame timing sample recorded by the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSample {
    /// Monotonic frame sequence number (1-based).
    pub frame_index: u64,
    /// Duration spent binding, rendering, flushing, and submitting the frame (microseconds).
    pub render_duration_us: u64,
    /// Elapsed interval since the previous frame submission completed (microseconds).
    /// `None` for the very first recorded frame.
    pub interval_us: Option<u64>,
    /// Number of damage rectangles submitted for this frame (`0` if undamaged).
    pub damage_rects: u32,
    /// Ratio of damaged pixels to total output pixels in permille (`0..=1000`).
    pub damage_ratio_permille: u16,
    /// End-to-end latency from the earliest pending input event to `present_done` (microseconds).
    pub input_to_present_us: Option<u64>,
}

/// Aggregated statistical summary of frame pacing, render durations, input latency,
/// event-loop dispatch, and damage tracking efficiency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameTimingSummary {
    /// Total frames recorded since collector creation.
    pub total_frames: u64,
    /// Number of samples retained in the ring buffer window.
    pub retained_samples: usize,
    /// Target refresh rate in millihertz (e.g. `60_000` for 60 Hz).
    pub target_refresh_mhz: i32,
    /// Theoretical frame budget in microseconds (e.g. `16_667` us for 60 Hz).
    pub target_interval_us: u64,
    /// Threshold above which a frame interval is classified as late (`1.5x` target interval).
    pub late_threshold_us: u64,
    /// Count of frame intervals exceeding `late_threshold_us` (`> 1.5x` target budget).
    pub late_frames: u64,
    /// Count of frame intervals exceeding `2.0x` target budget (missed display refresh).
    pub missed_frames: u64,
    /// Count of render passes that took longer than the target frame budget (render stall).
    pub render_stalls: u64,
    /// Count of non-render event-loop dispatch cycles exceeding half a frame budget.
    pub dispatch_stalls: u64,
    /// Count of frames with zero damaged pixels (`damage_rects == 0`).
    pub zero_damage_frames: u64,
    /// Average damaged area ratio in permille (`0..=1000`).
    pub damage_ratio_avg_permille: u16,
    /// 95th percentile damaged area ratio in permille (`0..=1000`).
    pub damage_ratio_p95_permille: u16,
    /// Minimum inter-frame interval in microseconds.
    pub interval_min_us: u64,
    /// 50th percentile (median) inter-frame interval in microseconds.
    pub interval_p50_us: u64,
    /// 95th percentile inter-frame interval in microseconds.
    pub interval_p95_us: u64,
    /// 99th percentile inter-frame interval in microseconds.
    pub interval_p99_us: u64,
    /// Maximum inter-frame interval in microseconds.
    pub interval_max_us: u64,
    /// 50th percentile (median) frame render duration in microseconds.
    pub render_p50_us: u64,
    /// 95th percentile frame render duration in microseconds.
    pub render_p95_us: u64,
    /// 99th percentile frame render duration in microseconds.
    pub render_p99_us: u64,
    /// Maximum frame render duration in microseconds.
    pub render_max_us: u64,
    /// Number of frames that presented a new input event.
    pub input_samples: usize,
    /// 50th percentile input-to-present latency in microseconds.
    pub input_p50_us: u64,
    /// 95th percentile input-to-present latency in microseconds.
    pub input_p95_us: u64,
    /// 99th percentile input-to-present latency in microseconds.
    pub input_p99_us: u64,
    /// Maximum input-to-present latency in microseconds.
    pub input_max_us: u64,
    /// 50th percentile event-loop client dispatch duration in microseconds.
    pub dispatch_p50_us: u64,
    /// 95th percentile event-loop client dispatch duration in microseconds.
    pub dispatch_p95_us: u64,
    /// 99th percentile event-loop client dispatch duration in microseconds.
    pub dispatch_p99_us: u64,
    /// Maximum event-loop client dispatch duration in microseconds.
    pub dispatch_max_us: u64,
}

impl FrameTimingSummary {
    /// Formats a concise single-line summary suitable for `tracing::info!` output.
    #[must_use]
    pub fn to_log_line(&self) -> String {
        format!(
            "frames={} target_us={} interval_us(p50/p95/p99/max)={}/{}/{}/{} render_us(p50/p95/p99/max)={}/{}/{}/{} input_us(n={}:p50/p95/max)={}/{}/{} dispatch_us(p95/max)={}/{} damage_permille(avg/p95/zero)={}/{}/{} late={} missed={} stalls={}/{}",
            self.total_frames,
            self.target_interval_us,
            self.interval_p50_us,
            self.interval_p95_us,
            self.interval_p99_us,
            self.interval_max_us,
            self.render_p50_us,
            self.render_p95_us,
            self.render_p99_us,
            self.render_max_us,
            self.input_samples,
            self.input_p50_us,
            self.input_p95_us,
            self.input_max_us,
            self.dispatch_p95_us,
            self.dispatch_max_us,
            self.damage_ratio_avg_permille,
            self.damage_ratio_p95_permille,
            self.zero_damage_frames,
            self.late_frames,
            self.missed_frames,
            self.render_stalls,
            self.dispatch_stalls,
        )
    }

    /// Formats a structured JSON report including hardware/software context.
    #[must_use]
    pub fn to_json(&self, backend: &str, mode_size: (i32, i32)) -> String {
        let mut out = String::with_capacity(1024);
        let _ = writeln!(out, "{{");
        let _ = writeln!(out, "  \"backend\": \"{backend}\",");
        let _ = writeln!(
            out,
            "  \"resolution\": \"{}x{}\",",
            mode_size.0, mode_size.1
        );
        let _ = writeln!(
            out,
            "  \"target_refresh_mhz\": {},",
            self.target_refresh_mhz
        );
        let _ = writeln!(
            out,
            "  \"target_interval_us\": {},",
            self.target_interval_us
        );
        let _ = writeln!(out, "  \"total_frames\": {},", self.total_frames);
        let _ = writeln!(out, "  \"retained_samples\": {},", self.retained_samples);
        let _ = writeln!(out, "  \"late_frames\": {},", self.late_frames);
        let _ = writeln!(out, "  \"missed_frames\": {},", self.missed_frames);
        let _ = writeln!(out, "  \"render_stalls\": {},", self.render_stalls);
        let _ = writeln!(out, "  \"dispatch_stalls\": {},", self.dispatch_stalls);
        let _ = writeln!(
            out,
            "  \"zero_damage_frames\": {},",
            self.zero_damage_frames
        );
        let _ = writeln!(out, "  \"damage_ratio_permille\": {{");
        let _ = writeln!(out, "    \"avg\": {},", self.damage_ratio_avg_permille);
        let _ = writeln!(out, "    \"p95\": {}", self.damage_ratio_p95_permille);
        let _ = writeln!(out, "  }},");
        let _ = writeln!(out, "  \"interval_us\": {{");
        let _ = writeln!(out, "    \"min\": {},", self.interval_min_us);
        let _ = writeln!(out, "    \"p50\": {},", self.interval_p50_us);
        let _ = writeln!(out, "    \"p95\": {},", self.interval_p95_us);
        let _ = writeln!(out, "    \"p99\": {},", self.interval_p99_us);
        let _ = writeln!(out, "    \"max\": {}", self.interval_max_us);
        let _ = writeln!(out, "  }},");
        let _ = writeln!(out, "  \"render_duration_us\": {{");
        let _ = writeln!(out, "    \"p50\": {},", self.render_p50_us);
        let _ = writeln!(out, "    \"p95\": {},", self.render_p95_us);
        let _ = writeln!(out, "    \"p99\": {},", self.render_p99_us);
        let _ = writeln!(out, "    \"max\": {}", self.render_max_us);
        let _ = writeln!(out, "  }},");
        let _ = writeln!(out, "  \"input_to_present_us\": {{");
        let _ = writeln!(out, "    \"samples\": {},", self.input_samples);
        let _ = writeln!(out, "    \"p50\": {},", self.input_p50_us);
        let _ = writeln!(out, "    \"p95\": {},", self.input_p95_us);
        let _ = writeln!(out, "    \"p99\": {},", self.input_p99_us);
        let _ = writeln!(out, "    \"max\": {}", self.input_max_us);
        let _ = writeln!(out, "  }},");
        let _ = writeln!(out, "  \"event_loop_dispatch_us\": {{");
        let _ = writeln!(out, "    \"p50\": {},", self.dispatch_p50_us);
        let _ = writeln!(out, "    \"p95\": {},", self.dispatch_p95_us);
        let _ = writeln!(out, "    \"p99\": {},", self.dispatch_p99_us);
        let _ = writeln!(out, "    \"max\": {}", self.dispatch_max_us);
        let _ = writeln!(out, "  }}");
        let _ = write!(out, "}}");
        out
    }
}

/// Preallocated ring-buffer collector for frame pacing, render durations,
/// input-to-present latency, event-loop dispatch, and damage ratio metrics.
#[derive(Debug, Clone)]
pub struct FrameTimingCollector {
    samples: Vec<FrameSample>,
    dispatch_samples_us: Vec<u64>,
    capacity: usize,
    write_cursor: usize,
    dispatch_cursor: usize,
    total_frames: u64,
    target_refresh_mhz: i32,
    target_interval_us: u64,
    late_threshold_us: u64,
    missed_threshold_us: u64,
    dispatch_stall_threshold_us: u64,
    late_frames: u64,
    missed_frames: u64,
    render_stalls: u64,
    dispatch_stalls: u64,
    zero_damage_frames: u64,
    last_present_instant: Option<Instant>,
    pending_input_instant: Option<Instant>,
}

impl FrameTimingCollector {
    /// Creates a new collector with the specified ring-buffer capacity and refresh rate in mHz.
    #[must_use]
    pub fn new(capacity: usize, target_refresh_mhz: i32) -> Self {
        let cap = capacity.max(16);
        let refresh_mhz = target_refresh_mhz.max(1_000);
        let refresh_u64 = u64::try_from(refresh_mhz).unwrap_or(60_000);
        let target_interval_us = 1_000_000_000_u64 / refresh_u64;
        let late_threshold_us = target_interval_us + (target_interval_us / 2);
        let missed_threshold_us = target_interval_us.saturating_mul(2);
        let dispatch_stall_threshold_us = (target_interval_us / 2).max(1_000);

        Self {
            samples: Vec::with_capacity(cap),
            dispatch_samples_us: Vec::with_capacity(cap),
            capacity: cap,
            write_cursor: 0,
            dispatch_cursor: 0,
            total_frames: 0,
            target_refresh_mhz: refresh_mhz,
            target_interval_us,
            late_threshold_us,
            missed_threshold_us,
            dispatch_stall_threshold_us,
            late_frames: 0,
            missed_frames: 0,
            render_stalls: 0,
            dispatch_stalls: 0,
            zero_damage_frames: 0,
            last_present_instant: None,
            pending_input_instant: None,
        }
    }

    /// Updates the target refresh rate in millihertz (e.g. on output mode change).
    pub fn set_target_refresh_mhz(&mut self, target_refresh_mhz: i32) {
        let refresh_mhz = target_refresh_mhz.max(1_000);
        let refresh_u64 = u64::try_from(refresh_mhz).unwrap_or(60_000);
        let target_interval_us = 1_000_000_000_u64 / refresh_u64;
        self.target_refresh_mhz = refresh_mhz;
        self.target_interval_us = target_interval_us;
        self.late_threshold_us = target_interval_us + (target_interval_us / 2);
        self.missed_threshold_us = target_interval_us.saturating_mul(2);
        self.dispatch_stall_threshold_us = (target_interval_us / 2).max(1_000);
    }

    /// Marks the arrival of an input event (`Instant::now()`).
    ///
    /// Preserves the earliest unpresented input timestamp so the next presented frame
    /// records the true worst-case input-to-present latency for that frame window.
    pub fn mark_input_event(&mut self, timestamp: Instant) {
        if self.pending_input_instant.is_none() {
            self.pending_input_instant = Some(timestamp);
        }
    }

    /// Records the duration of a non-render event-loop dispatch cycle.
    pub fn record_dispatch_duration(&mut self, duration: Duration) {
        let us = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        if us > self.dispatch_stall_threshold_us {
            self.dispatch_stalls = self.dispatch_stalls.saturating_add(1);
        }

        if self.dispatch_samples_us.len() < self.capacity {
            self.dispatch_samples_us.push(us);
        } else {
            self.dispatch_samples_us[self.dispatch_cursor] = us;
        }
        self.dispatch_cursor = (self.dispatch_cursor + 1) % self.capacity;
    }

    /// Records a completed frame using monotonic `Instant` timestamps and pixel damage stats.
    ///
    /// Performs zero heap allocations once the ring buffer reaches `capacity`.
    pub fn record_frame(
        &mut self,
        render_start: Instant,
        present_done: Instant,
        damage_rects: u32,
        damaged_pixels: u64,
        total_output_pixels: u64,
    ) -> FrameSample {
        let render_duration = present_done.saturating_duration_since(render_start);
        let interval = self
            .last_present_instant
            .map(|prev| present_done.saturating_duration_since(prev));
        self.last_present_instant = Some(present_done);

        let input_latency = self
            .pending_input_instant
            .take()
            .map(|input_ts| present_done.saturating_duration_since(input_ts));

        let ratio_permille = compute_damage_permille(damaged_pixels, total_output_pixels);

        self.record_durations_ext(
            render_duration,
            interval,
            damage_rects,
            ratio_permille,
            input_latency,
        )
    }

    /// Records a frame from explicit durations (useful for deterministic unit tests).
    pub fn record_durations(
        &mut self,
        render_duration: Duration,
        interval: Option<Duration>,
        damage_rects: u32,
    ) -> FrameSample {
        let default_permille = if damage_rects == 0 { 0 } else { 1000 };
        self.record_durations_ext(
            render_duration,
            interval,
            damage_rects,
            default_permille,
            None,
        )
    }

    /// Records a frame with explicit damage ratio and optional input-to-present latency.
    pub fn record_durations_ext(
        &mut self,
        render_duration: Duration,
        interval: Option<Duration>,
        damage_rects: u32,
        damage_ratio_permille: u16,
        input_latency: Option<Duration>,
    ) -> FrameSample {
        self.total_frames = self.total_frames.saturating_add(1);

        let render_duration_us = u64::try_from(render_duration.as_micros()).unwrap_or(u64::MAX);
        let interval_us = interval.map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX));
        let input_to_present_us =
            input_latency.map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX));

        if render_duration_us > self.target_interval_us {
            self.render_stalls = self.render_stalls.saturating_add(1);
        }

        if damage_rects == 0 || damage_ratio_permille == 0 {
            self.zero_damage_frames = self.zero_damage_frames.saturating_add(1);
        }

        if let Some(iv_us) = interval_us {
            if iv_us > self.late_threshold_us {
                self.late_frames = self.late_frames.saturating_add(1);
            }
            if iv_us > self.missed_threshold_us {
                self.missed_frames = self.missed_frames.saturating_add(1);
            }
        }

        let sample = FrameSample {
            frame_index: self.total_frames,
            render_duration_us,
            interval_us,
            damage_rects,
            damage_ratio_permille: damage_ratio_permille.min(1000),
            input_to_present_us,
        };

        if self.samples.len() < self.capacity {
            self.samples.push(sample);
        } else {
            self.samples[self.write_cursor] = sample;
        }
        self.write_cursor = (self.write_cursor + 1) % self.capacity;

        sample
    }

    /// Returns the total number of frames recorded so far.
    #[must_use]
    pub const fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Computes percentile statistics (`p50`, `p95`, `p99`, `max`) over the retained window.
    #[must_use]
    pub fn summary(&self) -> FrameTimingSummary {
        let mut intervals: Vec<u64> = self.samples.iter().filter_map(|s| s.interval_us).collect();
        intervals.sort_unstable();

        let mut renders: Vec<u64> = self.samples.iter().map(|s| s.render_duration_us).collect();
        renders.sort_unstable();

        let mut inputs: Vec<u64> = self
            .samples
            .iter()
            .filter_map(|s| s.input_to_present_us)
            .collect();
        inputs.sort_unstable();

        let mut dispatches: Vec<u64> = self.dispatch_samples_us.clone();
        dispatches.sort_unstable();

        let mut damages: Vec<u64> = self
            .samples
            .iter()
            .map(|s| u64::from(s.damage_ratio_permille))
            .collect();
        damages.sort_unstable();

        let damage_avg = if damages.is_empty() {
            0
        } else {
            let sum: u64 = damages.iter().sum();
            let avg = sum / u64::try_from(damages.len()).unwrap_or(1);
            u16::try_from(avg.min(1000)).unwrap_or(1000)
        };
        let damage_p95 = u16::try_from(percentile_sorted(&damages, 95).min(1000)).unwrap_or(1000);

        FrameTimingSummary {
            total_frames: self.total_frames,
            retained_samples: self.samples.len(),
            target_refresh_mhz: self.target_refresh_mhz,
            target_interval_us: self.target_interval_us,
            late_threshold_us: self.late_threshold_us,
            late_frames: self.late_frames,
            missed_frames: self.missed_frames,
            render_stalls: self.render_stalls,
            dispatch_stalls: self.dispatch_stalls,
            zero_damage_frames: self.zero_damage_frames,
            damage_ratio_avg_permille: damage_avg,
            damage_ratio_p95_permille: damage_p95,
            interval_min_us: intervals.first().copied().unwrap_or(0),
            interval_p50_us: percentile_sorted(&intervals, 50),
            interval_p95_us: percentile_sorted(&intervals, 95),
            interval_p99_us: percentile_sorted(&intervals, 99),
            interval_max_us: intervals.last().copied().unwrap_or(0),
            render_p50_us: percentile_sorted(&renders, 50),
            render_p95_us: percentile_sorted(&renders, 95),
            render_p99_us: percentile_sorted(&renders, 99),
            render_max_us: renders.last().copied().unwrap_or(0),
            input_samples: inputs.len(),
            input_p50_us: percentile_sorted(&inputs, 50),
            input_p95_us: percentile_sorted(&inputs, 95),
            input_p99_us: percentile_sorted(&inputs, 99),
            input_max_us: inputs.last().copied().unwrap_or(0),
            dispatch_p50_us: percentile_sorted(&dispatches, 50),
            dispatch_p95_us: percentile_sorted(&dispatches, 95),
            dispatch_p99_us: percentile_sorted(&dispatches, 99),
            dispatch_max_us: dispatches.last().copied().unwrap_or(0),
        }
    }
}

fn compute_damage_permille(damaged_pixels: u64, total_output_pixels: u64) -> u16 {
    if total_output_pixels == 0 || damaged_pixels == 0 {
        return 0;
    }
    let clamped = damaged_pixels.min(total_output_pixels);
    let permille = clamped.saturating_mul(1000) / total_output_pixels;
    u16::try_from(permille.min(1000)).unwrap_or(1000)
}

fn percentile_sorted(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) * pct) / 100;
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_tracks_percentiles_and_late_frames() {
        let mut collector = FrameTimingCollector::new(64, 60_000);
        assert_eq!(collector.target_interval_us, 16_666);

        // First frame has no interval.
        collector.record_durations(Duration::from_micros(900), None, 1);

        // 19 steady 60 Hz frames (~16.6 ms interval, 800 us render).
        for _ in 0..19 {
            collector.record_durations(
                Duration::from_micros(800),
                Some(Duration::from_micros(16_667)),
                1,
            );
        }

        // 1 late frame (28 ms > 1.5x budget) and 1 missed frame + render stall (35 ms > 2.0x budget, 18 ms render).
        collector.record_durations(
            Duration::from_micros(1_200),
            Some(Duration::from_millis(28)),
            1,
        );
        collector.record_durations(
            Duration::from_millis(18),
            Some(Duration::from_millis(35)),
            2,
        );

        let summary = collector.summary();
        assert_eq!(summary.total_frames, 22);
        assert_eq!(summary.retained_samples, 22);
        assert_eq!(summary.late_frames, 2);
        assert_eq!(summary.missed_frames, 1);
        assert_eq!(summary.render_stalls, 1);
        assert_eq!(summary.interval_p50_us, 16_667);
        assert_eq!(summary.interval_max_us, 35_000);
        assert_eq!(summary.render_max_us, 18_000);

        let json = summary.to_json("nested-winit", (1280, 800));
        assert!(json.contains("\"total_frames\": 22"));
        assert!(json.contains("\"late_frames\": 2"));
    }

    #[test]
    fn ring_buffer_wraps_without_growing_beyond_capacity() {
        let mut collector = FrameTimingCollector::new(16, 120_000);
        for i in 0..40_u64 {
            collector.record_durations(
                Duration::from_micros(500 + i),
                Some(Duration::from_micros(8_333)),
                1,
            );
        }

        let summary = collector.summary();
        assert_eq!(summary.total_frames, 40);
        assert_eq!(summary.retained_samples, 16);
    }

    #[test]
    fn collector_tracks_input_latency_dispatch_stalls_and_damage_ratio() {
        let mut collector = FrameTimingCollector::new(32, 60_000);

        let base = Instant::now();
        let input_ts = base;
        let render_start = base + Duration::from_millis(4);
        let present_done = base + Duration::from_millis(6);

        collector.mark_input_event(input_ts);
        // Subsequent input before present should not overwrite the earliest pending input timestamp.
        collector.mark_input_event(base + Duration::from_millis(2));

        let sample = collector.record_frame(render_start, present_done, 2, 256_000, 1_024_000);
        assert_eq!(sample.damage_ratio_permille, 250);
        assert_eq!(sample.input_to_present_us, Some(6_000));

        // Frame with zero damage and no input.
        collector.record_durations_ext(
            Duration::from_micros(400),
            Some(Duration::from_micros(16_667)),
            0,
            0,
            None,
        );

        // Normal dispatch (120 us) and stalled dispatch (10 ms > 8.33 ms threshold).
        collector.record_dispatch_duration(Duration::from_micros(120));
        collector.record_dispatch_duration(Duration::from_millis(10));

        let summary = collector.summary();
        assert_eq!(summary.input_samples, 1);
        assert_eq!(summary.input_p50_us, 6_000);
        assert_eq!(summary.zero_damage_frames, 1);
        assert_eq!(summary.damage_ratio_avg_permille, 125);
        assert_eq!(summary.dispatch_stalls, 1);
        assert_eq!(summary.dispatch_max_us, 10_000);
    }
}
