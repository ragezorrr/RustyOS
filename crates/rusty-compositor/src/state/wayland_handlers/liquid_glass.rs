//! First-party Wayland Liquid Glass protocol handler.
//!
//! Provides the server-side implementation of `rustyos_liquid_glass_v1`,
//! enabling client surfaces to declare frosted glass effects, corner radii,
//! vibrancy styles, and subregions for GPU hardware composition.

#![allow(missing_docs, clippy::all, unreachable_pub)]

use std::sync::{Arc, Mutex};

use smithay::{
    reexports::wayland_server::{
        Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
        backend::GlobalId,
        protocol::{wl_region::WlRegion, wl_surface::WlSurface},
    },
    utils::{Logical, Rectangle},
    wayland::compositor,
};

use super::super::RustyCompositorState;

#[allow(clippy::wildcard_imports)]
pub mod protocol {
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    #[allow(clippy::wildcard_imports)]
    pub mod __interfaces {
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../../protocols/rustyos-liquid-glass-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!("../../protocols/rustyos-liquid-glass-v1.xml");
}

pub use protocol::rustyos_liquid_glass_manager_v1::RustyosLiquidGlassManagerV1;
pub use protocol::rustyos_surface_glass_v1::{
    Error as GlassError, RustyosSurfaceGlassV1, Style as GlassStyle,
};

/// Glass preset style configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlassStylePreset {
    #[default]
    Popover = 0,
    Sidebar = 1,
    ControlPill = 2,
    BasePanel = 3,
    Custom = 4,
}

impl From<GlassStyle> for GlassStylePreset {
    fn from(s: GlassStyle) -> Self {
        match s {
            GlassStyle::Popover => GlassStylePreset::Popover,
            GlassStyle::Sidebar => GlassStylePreset::Sidebar,
            GlassStyle::ControlPill => GlassStylePreset::ControlPill,
            GlassStyle::BasePanel => GlassStylePreset::BasePanel,
            GlassStyle::Custom => GlassStylePreset::Custom,
        }
    }
}

/// Double-buffered Liquid Glass surface state attached to [`WlSurface`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiquidGlassSurfaceCachedState {
    /// Whether liquid glass backdrop blur is active for this surface.
    pub active: bool,
    /// Optional bounding region in surface-local logical pixels (None = entire surface).
    pub region: Option<Vec<Rectangle<i32, Logical>>>,
    /// Rounded corner radius in pixels.
    pub corner_radius: u32,
    /// Style preset.
    pub style: GlassStylePreset,
    /// Optional custom tint overlay (RGBA).
    pub tint: Option<[u8; 4]>,
}

impl LiquidGlassSurfaceCachedState {
    /// Returns `true` when Liquid Glass is active and covers at least one non-empty region.
    #[must_use]
    pub fn has_effective_area(&self) -> bool {
        self.active
            && self
                .region
                .as_ref()
                .is_none_or(|rects| rects.iter().any(|r| r.size.w > 0 && r.size.h > 0))
    }
}

impl compositor::Cacheable for LiquidGlassSurfaceCachedState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// User data attached to [`RustyosSurfaceGlassV1`].
#[derive(Debug, Clone)]
pub struct LiquidGlassSurfaceUserData {
    surface: Arc<Mutex<Option<WlSurface>>>,
}

impl LiquidGlassSurfaceUserData {
    pub fn new(surface: WlSurface) -> Self {
        Self {
            surface: Arc::new(Mutex::new(Some(surface))),
        }
    }

    pub fn wl_surface(&self) -> Option<WlSurface> {
        self.surface.lock().ok().and_then(|guard| guard.clone())
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.surface.lock() {
            *guard = None;
        }
    }
}

/// State holder for the `rustyos_liquid_glass_manager_v1` global.
#[derive(Debug)]
pub struct LiquidGlassManagerState {
    pub global: GlobalId,
}

impl LiquidGlassManagerState {
    pub fn new(display: &DisplayHandle) -> Self {
        let global =
            display.create_global::<RustyCompositorState, RustyosLiquidGlassManagerV1, _>(1, ());
        Self { global }
    }
}

impl GlobalDispatch<RustyosLiquidGlassManagerV1, ()> for RustyCompositorState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<RustyosLiquidGlassManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<RustyosLiquidGlassManagerV1, ()> for RustyCompositorState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &RustyosLiquidGlassManagerV1,
        request: protocol::rustyos_liquid_glass_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use protocol::rustyos_liquid_glass_manager_v1::Request;

        match request {
            Request::GetSurfaceGlass { id, surface } => {
                let user_data = LiquidGlassSurfaceUserData::new(surface.clone());
                let _glass_res = data_init.init(id, user_data);

                // Initialize pending cached state on the surface
                compositor::with_states(&surface, |states| {
                    let mut cached = states.cached_state.get::<LiquidGlassSurfaceCachedState>();
                    cached.pending().active = true;
                    // Default corner radius to 16 for popovers
                    if cached.pending().corner_radius == 0 {
                        cached.pending().corner_radius = 16;
                    }
                });

                state.active_liquid_glass_surfaces.insert(surface.id());
            }
            Request::Destroy => {}
        }
    }
}

impl Dispatch<RustyosSurfaceGlassV1, LiquidGlassSurfaceUserData> for RustyCompositorState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &RustyosSurfaceGlassV1,
        request: protocol::rustyos_surface_glass_v1::Request,
        data: &LiquidGlassSurfaceUserData,
        dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use protocol::rustyos_surface_glass_v1::Request;

        let Some(surface) = data.wl_surface() else {
            return;
        };

        match request {
            Request::SetRegion { region } => {
                let rects = region.map(|r: WlRegion| {
                    compositor::get_region_attributes(&r)
                        .rects
                        .into_iter()
                        .map(|(_, rect)| rect)
                        .collect::<Vec<_>>()
                });
                compositor::with_states(&surface, |states| {
                    let mut cached = states.cached_state.get::<LiquidGlassSurfaceCachedState>();
                    cached.pending().region = rects;
                    cached.pending().active = true;
                });
            }
            Request::SetCornerRadius { radius } => {
                compositor::with_states(&surface, |states| {
                    let mut cached = states.cached_state.get::<LiquidGlassSurfaceCachedState>();
                    cached.pending().corner_radius = radius;
                    cached.pending().active = true;
                });
            }
            Request::SetStyle { style } => {
                let preset = match style {
                    WEnum::Value(s) => GlassStylePreset::from(s),
                    WEnum::Unknown(val) => {
                        dh.post_error(
                            resource,
                            GlassError::InvalidStyle as u32,
                            format!("Unrecognized glass style: {val}"),
                        );
                        return;
                    }
                };
                compositor::with_states(&surface, |states| {
                    let mut cached = states.cached_state.get::<LiquidGlassSurfaceCachedState>();
                    cached.pending().style = preset;
                    cached.pending().active = true;
                });
            }
            Request::SetTint { r, g, b, a } => {
                let tint = [
                    (r.min(255)) as u8,
                    (g.min(255)) as u8,
                    (b.min(255)) as u8,
                    (a.min(255)) as u8,
                ];
                compositor::with_states(&surface, |states| {
                    let mut cached = states.cached_state.get::<LiquidGlassSurfaceCachedState>();
                    cached.pending().tint = Some(tint);
                    cached.pending().active = true;
                });
            }
            Request::Commit => {
                // Explicit commit marker: ensure pending is marked active
                compositor::with_states(&surface, |states| {
                    let mut cached = states.cached_state.get::<LiquidGlassSurfaceCachedState>();
                    cached.pending().active = true;
                });
            }
            Request::Destroy => {
                compositor::with_states(&surface, |states| {
                    let mut cached = states.cached_state.get::<LiquidGlassSurfaceCachedState>();
                    cached.pending().active = false;
                });
                state.active_liquid_glass_surfaces.remove(&surface.id());
                data.clear();
            }
        }
    }
}

/// Retrieve the active liquid glass surface configuration if active.
pub(crate) fn surface_liquid_glass_state(
    surface: &WlSurface,
) -> Option<LiquidGlassSurfaceCachedState> {
    compositor::with_states(surface, |states| {
        let current = states
            .cached_state
            .get::<LiquidGlassSurfaceCachedState>()
            .current()
            .clone();
        if current.active { Some(current) } else { None }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use smithay::wayland::compositor::Cacheable;

    #[test]
    fn test_liquid_glass_cached_state_defaults() {
        let state = LiquidGlassSurfaceCachedState::default();
        assert!(!state.active);
        assert_eq!(state.corner_radius, 0);
        assert_eq!(state.style, GlassStylePreset::Popover);
        assert_eq!(state.region, None);
        assert_eq!(state.tint, None);
    }

    #[test]
    fn test_liquid_glass_style_conversions() {
        assert_eq!(
            GlassStylePreset::from(GlassStyle::Popover),
            GlassStylePreset::Popover
        );
        assert_eq!(
            GlassStylePreset::from(GlassStyle::Sidebar),
            GlassStylePreset::Sidebar
        );
        assert_eq!(
            GlassStylePreset::from(GlassStyle::ControlPill),
            GlassStylePreset::ControlPill
        );
        assert_eq!(
            GlassStylePreset::from(GlassStyle::BasePanel),
            GlassStylePreset::BasePanel
        );
        assert_eq!(
            GlassStylePreset::from(GlassStyle::Custom),
            GlassStylePreset::Custom
        );
    }

    #[test]
    fn test_liquid_glass_cached_state_commit_and_merge() {
        let mut state = LiquidGlassSurfaceCachedState {
            active: true,
            region: Some(vec![Rectangle::new((10, 20).into(), (300, 400).into())]),
            corner_radius: 16,
            style: GlassStylePreset::Popover,
            tint: Some([255, 255, 255, 30]),
        };

        let dummy_display = smithay::reexports::wayland_server::Display::<()>::new().unwrap();
        let dh = dummy_display.handle();
        let committed = state.commit(&dh);
        assert_eq!(committed, state);

        let mut target = LiquidGlassSurfaceCachedState::default();
        committed.merge_into(&mut target, &dh);
        assert_eq!(target.active, true);
        assert_eq!(target.corner_radius, 16);
        assert_eq!(target.style, GlassStylePreset::Popover);
        assert_eq!(target.tint, Some([255, 255, 255, 30]));
        assert!(target.region.is_some());
    }

    #[test]
    fn test_liquid_glass_has_effective_area_rejects_empty_regions() {
        let inactive = LiquidGlassSurfaceCachedState::default();
        assert!(!inactive.has_effective_area());

        let full_surface = LiquidGlassSurfaceCachedState {
            active: true,
            region: None,
            ..Default::default()
        };
        assert!(full_surface.has_effective_area());

        let empty_region = LiquidGlassSurfaceCachedState {
            active: true,
            region: Some(Vec::new()),
            ..Default::default()
        };
        assert!(!empty_region.has_effective_area());

        let zero_rect = LiquidGlassSurfaceCachedState {
            active: true,
            region: Some(vec![Rectangle::new((0, 0).into(), (0, 200).into())]),
            ..Default::default()
        };
        assert!(!zero_rect.has_effective_area());

        let valid_rect = LiquidGlassSurfaceCachedState {
            active: true,
            region: Some(vec![Rectangle::new((0, 0).into(), (196, 600).into())]),
            ..Default::default()
        };
        assert!(valid_rect.has_effective_area());
    }
}
