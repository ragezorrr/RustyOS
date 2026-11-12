//! Low-overhead frame timing and pacing instrumentation (`RUSTYOS-003`).
//!
//! Uses a fixed-capacity ring buffer preallocated at startup so recording frame samples
//! on the hot rendering path performs zero heap allocations and takes no locks.

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
}

/// Aggregated statistical summary of frame pacing and render durations.
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
    /// Count of render passes that took longer than the target frame budget (event-loop stall).
    pub render_stalls: u64,
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
}

impl FrameTimingSummary {
    /// Formats a concise single-line summary suitable for `tracing::info!` output.
    #[must_use]
    pub fn to_log_line(&self) -> String {
        format!(
            "frames={} target_us={} interval_us(p50/p95/p99/max)={}/{}/{}/{} render_us(p50/p95/p99/max)={}/{}/{}/{} late={} missed={} stalls={}",
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
            self.late_frames,
            self.missed_frames,
            self.render_stalls,
        )
    }

    /// Formats a structured JSON report including hardware/software context.
    #[must_use]
    pub fn to_json(&self, backend: &str, mode_size: (i32, i32)) -> String {
        let mut out = String::with_capacity(512);
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
        let _ = writeln!(out, "  }}");
        let _ = write!(out, "}}");
        out
    }
}

/// Preallocated ring-buffer collector for frame pacing and render duration metrics.
#[derive(Debug, Clone)]
pub struct FrameTimingCollector {
    samples: Vec<FrameSample>,
    capacity: usize,
    write_cursor: usize,
    total_frames: u64,
    target_refresh_mhz: i32,
    target_interval_us: u64,
    late_threshold_us: u64,
    missed_threshold_us: u64,
    late_frames: u64,
    missed_frames: u64,
    render_stalls: u64,
    last_present_instant: Option<Instant>,
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

        Self {
            samples: Vec::with_capacity(cap),
            capacity: cap,
            write_cursor: 0,
            total_frames: 0,
            target_refresh_mhz: refresh_mhz,
            target_interval_us,
            late_threshold_us,
            missed_threshold_us,
            late_frames: 0,
            missed_frames: 0,
            render_stalls: 0,
            last_present_instant: None,
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
    }

    /// Records a completed frame using monotonic `Instant` timestamps.
    ///
    /// Performs zero heap allocations once the ring buffer reaches `capacity`.
    pub fn record_frame(
        &mut self,
        render_start: Instant,
        present_done: Instant,
        damage_rects: u32,
    ) -> FrameSample {
        let render_duration = present_done.saturating_duration_since(render_start);
        let interval = self
            .last_present_instant
            .map(|prev| present_done.saturating_duration_since(prev));
        self.last_present_instant = Some(present_done);

        self.record_durations(render_duration, interval, damage_rects)
    }

    /// Records a frame from explicit durations (useful for deterministic unit tests).
    pub fn record_durations(
        &mut self,
        render_duration: Duration,
        interval: Option<Duration>,
        damage_rects: u32,
    ) -> FrameSample {
        self.total_frames = self.total_frames.saturating_add(1);

        let render_duration_us = u64::try_from(render_duration.as_micros()).unwrap_or(u64::MAX);
        let interval_us = interval.map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX));

        if render_duration_us > self.target_interval_us {
            self.render_stalls = self.render_stalls.saturating_add(1);
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

        let interval_min_us = intervals.first().copied().unwrap_or(0);
        let interval_p50_us = percentile_sorted(&intervals, 50);
        let interval_p95_us = percentile_sorted(&intervals, 95);
        let interval_p99_us = percentile_sorted(&intervals, 99);
        let interval_max_us = intervals.last().copied().unwrap_or(0);

        let render_p50_us = percentile_sorted(&renders, 50);
        let render_p95_us = percentile_sorted(&renders, 95);
        let render_p99_us = percentile_sorted(&renders, 99);
        let render_max_us = renders.last().copied().unwrap_or(0);

        FrameTimingSummary {
            total_frames: self.total_frames,
            retained_samples: self.samples.len(),
            target_refresh_mhz: self.target_refresh_mhz,
            target_interval_us: self.target_interval_us,
            late_threshold_us: self.late_threshold_us,
            late_frames: self.late_frames,
            missed_frames: self.missed_frames,
            render_stalls: self.render_stalls,
            interval_min_us,
            interval_p50_us,
            interval_p95_us,
            interval_p99_us,
            interval_max_us,
            render_p50_us,
            render_p95_us,
            render_p99_us,
            render_max_us,
        }
    }
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
}
