//! Multi-monitor layout, dynamic display management, and hotplug tracking (`RUSTYOS-040`).
//!
//! Provides data structures and orchestration for multiple display outputs:
//! - [`OutputEntry`]: Represents an individual display output (KMS connector or virtual Winit window),
//!   tracking its [`OutputId`], Wayland [`Output`] handle, global placement coordinates, resolution,
//!   scale factor, and refresh rate.
//! - [`OutputMap`]: Collection of managed outputs in global layout space, providing hit-testing,
//!   adjacent placement calculation for hotplugged displays, and multi-monitor bounding box queries.

use rusty_wm::{OutputId, Rect};
use smithay::{
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    utils::{Logical, Physical, Point, Rectangle, Size},
};
use tracing::info;

/// Metadata and handle for an active display output in compositor space.
#[derive(Debug, Clone)]
pub struct OutputEntry {
    /// Unique numeric identifier mapped to `rusty_wm`.
    pub id: OutputId,
    /// Human-readable connector or output name (e.g., `"eDP-1"`, `"HDMI-A-1"`, `"DP-1"`).
    pub name: String,
    /// Smithay Wayland output handle.
    pub output: Output,
    /// Top-left origin of this output in compositor global logical coordinate space.
    pub location: Point<i32, Logical>,
    /// Native physical resolution in hardware pixels.
    pub physical_size: Size<i32, Physical>,
    /// Fractional or integer UI scaling factor.
    pub scale: f64,
    /// Refresh rate in millihertz (e.g. 60,000 for 60 Hz, 144,000 for 144 Hz).
    pub refresh_milli_hz: i32,
    /// Whether this output is the primary desktop display.
    pub is_primary: bool,
}

impl OutputEntry {
    /// Constructs a new [`OutputEntry`].
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: OutputId,
        name: impl Into<String>,
        output: Output,
        location: Point<i32, Logical>,
        physical_size: Size<i32, Physical>,
        scale: f64,
        refresh_milli_hz: i32,
        is_primary: bool,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            output,
            location,
            physical_size,
            scale: if scale > 0.1 { scale } else { 1.0 },
            refresh_milli_hz,
            is_primary,
        }
    }

    /// Computes the bounding rectangle of this output in compositor logical coordinates.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn logical_geometry(&self) -> Rectangle<i32, Logical> {
        let logical_w = (f64::from(self.physical_size.w) / self.scale).round() as i32;
        let logical_h = (f64::from(self.physical_size.h) / self.scale).round() as i32;
        Rectangle::new(self.location, (logical_w.max(1), logical_h.max(1)).into())
    }

    /// Returns the window manager work area rectangle corresponding to this output.
    #[must_use]
    pub fn wm_rect(&self) -> Rect {
        let geo = self.logical_geometry();
        Rect::new(geo.loc.x, geo.loc.y, geo.size.w.max(1), geo.size.h.max(1))
    }

    /// Refresh rate in rounded integer hertz (e.g., 60 or 144).
    #[must_use]
    pub fn refresh_hz(&self) -> u32 {
        u32::try_from((self.refresh_milli_hz + 500) / 1000).unwrap_or(60)
    }
}

/// Collection and coordinate layout manager for all connected outputs.
#[derive(Debug, Clone)]
pub struct OutputMap {
    entries: Vec<OutputEntry>,
    next_id_counter: u32,
}

impl OutputMap {
    /// Creates a new [`OutputMap`] initialized with the given primary output.
    #[must_use]
    pub fn new(primary: OutputEntry) -> Self {
        let next_counter = primary.id.0.saturating_add(1);
        Self {
            entries: vec![primary],
            next_id_counter: next_counter.max(2),
        }
    }

    /// Returns the slice of all managed output entries.
    #[must_use]
    pub fn entries(&self) -> &[OutputEntry] {
        &self.entries
    }

    /// Returns a mutable slice of all managed output entries.
    pub fn entries_mut(&mut self) -> &mut [OutputEntry] {
        &mut self.entries
    }

    /// Returns a reference to the primary output entry.
    #[must_use]
    pub fn primary(&self) -> &OutputEntry {
        self.entries
            .iter()
            .find(|e| e.is_primary)
            .unwrap_or(&self.entries[0])
    }

    /// Returns a mutable reference to the primary output entry.
    pub fn primary_mut(&mut self) -> &mut OutputEntry {
        let primary_idx = self.entries.iter().position(|e| e.is_primary).unwrap_or(0);
        &mut self.entries[primary_idx]
    }

    /// Finds an output entry by its [`OutputId`].
    #[must_use]
    pub fn get(&self, id: OutputId) -> Option<&OutputEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Finds a mutable output entry by its [`OutputId`].
    pub fn get_mut(&mut self, id: OutputId) -> Option<&mut OutputEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Finds an output entry associated with the given Smithay [`Output`].
    #[must_use]
    pub fn find_by_output(&self, output: &Output) -> Option<&OutputEntry> {
        self.entries.iter().find(|e| &e.output == output)
    }

    /// Allocates the next available [`OutputId`].
    pub fn next_output_id(&mut self) -> OutputId {
        let id = OutputId(self.next_id_counter);
        self.next_id_counter = self.next_id_counter.saturating_add(1);
        id
    }

    /// Adds an output entry to the map.
    pub fn add(&mut self, entry: OutputEntry) {
        info!(
            id = entry.id.0,
            name = %entry.name,
            loc = ?entry.location,
            size = ?entry.physical_size,
            scale = entry.scale,
            refresh = entry.refresh_milli_hz,
            "Added output to OutputMap"
        );
        self.entries.push(entry);
    }

    /// Removes an output entry by its [`OutputId`].
    ///
    /// Never removes the last remaining output. If the removed output was primary,
    /// the first remaining output is automatically designated as primary.
    pub fn remove(&mut self, id: OutputId) -> Option<OutputEntry> {
        if self.entries.len() <= 1 {
            return None;
        }
        let pos = self.entries.iter().position(|e| e.id == id)?;
        let removed = self.entries.remove(pos);
        if removed.is_primary && !self.entries.is_empty() {
            self.entries[0].is_primary = true;
            info!(
                new_primary_id = self.entries[0].id.0,
                new_primary_name = %self.entries[0].name,
                "Promoted output to primary after removal"
            );
        }
        info!(
            id = removed.id.0,
            name = %removed.name,
            "Removed output from OutputMap"
        );
        Some(removed)
    }

    /// Returns the [`OutputId`] of the output containing the given logical point, if any.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn output_under(&self, point: Point<f64, Logical>) -> Option<OutputId> {
        let px = point.x.round() as i32;
        let py = point.y.round() as i32;
        for entry in &self.entries {
            let geo = entry.logical_geometry();
            if geo.contains((px, py)) {
                return Some(entry.id);
            }
        }
        None
    }

    /// Computes the union bounding box enclosing all managed outputs in compositor space.
    #[must_use]
    pub fn total_bounds(&self) -> Rectangle<i32, Logical> {
        if self.entries.is_empty() {
            return Rectangle::new((0, 0).into(), (1280, 800).into());
        }

        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;

        for entry in &self.entries {
            let geo = entry.logical_geometry();
            min_x = min_x.min(geo.loc.x);
            min_y = min_y.min(geo.loc.y);
            max_x = max_x.max(geo.loc.x + geo.size.w);
            max_y = max_y.max(geo.loc.y + geo.size.h);
        }

        Rectangle::new(
            (min_x, min_y).into(),
            ((max_x - min_x).max(1), (max_y - min_y).max(1)).into(),
        )
    }

    /// Computes the top-left location for placing a new display output horizontally adjacent
    /// to the right of all existing displays.
    #[must_use]
    pub fn next_adjacent_location(&self) -> Point<i32, Logical> {
        let mut max_x = 0;
        for entry in &self.entries {
            let geo = entry.logical_geometry();
            max_x = max_x.max(geo.loc.x + geo.size.w);
        }
        Point::from((max_x, 0))
    }

    /// Collects all underlying Smithay [`Output`] handles.
    #[must_use]
    pub fn smithay_outputs(&self) -> Vec<Output> {
        self.entries.iter().map(|e| e.output.clone()).collect()
    }

    /// Number of active displays.
    #[must_use]
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

/// Helper to create a Smithay [`Output`] for testing or headless execution.
#[must_use]
pub fn create_test_output(name: &str, size: Size<i32, Physical>, refresh_milli_hz: i32) -> Output {
    let output = Output::new(
        name.to_owned(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "RustyOS".to_owned(),
            model: "Virtual Display".to_owned(),
        },
    );
    let mode = Mode {
        size,
        refresh: refresh_milli_hz,
    };
    output.change_current_state(
        Some(mode),
        Some(smithay::utils::Transform::Normal),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_entry_geometry_scales_correctly() {
        let out = create_test_output("DP-1", (3840, 2160).into(), 144_000);
        let entry = OutputEntry::new(
            OutputId(1),
            "DP-1",
            out,
            (0, 0).into(),
            (3840, 2160).into(),
            2.0,
            144_000,
            true,
        );

        let geo = entry.logical_geometry();
        assert_eq!(geo.loc, (0, 0).into());
        assert_eq!(geo.size, (1920, 1080).into());
        assert_eq!(entry.refresh_hz(), 144);

        let wm_rect = entry.wm_rect();
        assert_eq!(wm_rect.x, 0);
        assert_eq!(wm_rect.y, 0);
        assert_eq!(wm_rect.width, 1920);
        assert_eq!(wm_rect.height, 1080);
    }

    #[test]
    fn output_map_placement_and_hit_testing() {
        let out1 = create_test_output("eDP-1", (1920, 1080).into(), 60_000);
        let entry1 = OutputEntry::new(
            OutputId(1),
            "eDP-1",
            out1,
            (0, 0).into(),
            (1920, 1080).into(),
            1.0,
            60_000,
            true,
        );

        let mut map = OutputMap::new(entry1);
        assert_eq!(map.count(), 1);
        assert_eq!(map.primary().id, OutputId(1));
        assert_eq!(map.next_adjacent_location(), (1920, 0).into());

        // Add a secondary 240 Hz monitor to the right
        let next_loc = map.next_adjacent_location();
        let next_id = map.next_output_id();
        let out2 = create_test_output("HDMI-A-1", (2560, 1440).into(), 240_000);
        let entry2 = OutputEntry::new(
            next_id,
            "HDMI-A-1",
            out2,
            next_loc,
            (2560, 1440).into(),
            1.0,
            240_000,
            false,
        );
        map.add(entry2);

        assert_eq!(map.count(), 2);
        let total = map.total_bounds();
        assert_eq!(total.loc, (0, 0).into());
        assert_eq!(total.size, (1920 + 2560, 1440).into());

        // Hit testing
        assert_eq!(map.output_under((500.0, 500.0).into()), Some(OutputId(1)));
        assert_eq!(map.output_under((2000.0, 500.0).into()), Some(OutputId(2)));
        assert_eq!(map.output_under((6000.0, 500.0).into()), None);

        // Removal preserves remaining output and promotes to primary if needed
        let removed = map.remove(OutputId(1));
        assert!(removed.is_some());
        assert_eq!(map.count(), 1);
        assert_eq!(map.primary().id, OutputId(2));
        assert!(map.primary().is_primary);

        // Cannot remove the last remaining output
        assert!(map.remove(OutputId(2)).is_none());
        assert_eq!(map.count(), 1);
    }
}
