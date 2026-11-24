//! Canonical client-side bindings and configuration types for `rustyos_liquid_glass_v1`.

#![allow(missing_docs, clippy::all, unreachable_pub)]

#[allow(clippy::wildcard_imports)]
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    #[allow(clippy::wildcard_imports)]
    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../../protocols/rustyos-liquid-glass-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("../../protocols/rustyos-liquid-glass-v1.xml");
}

pub use protocol::rustyos_liquid_glass_manager_v1::RustyosLiquidGlassManagerV1;
pub use protocol::rustyos_surface_glass_v1::{RustyosSurfaceGlassV1, Style as GlassStyle};

/// Region on a client surface that receives compositor-side Liquid Glass blur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlassRegion {
    /// Apply glass across the entire surface bounds.
    #[default]
    FullSurface,
    /// Apply glass only to a specific surface-local logical rectangle.
    Rect {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    /// Disable glass blur while keeping the protocol object alive.
    Empty,
}

/// High-level declarative Liquid Glass configuration for a Wayland surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidGlassConfig {
    /// Optical preset style from `rustyos_surface_glass_v1`.
    pub style: GlassStyle,
    /// Rounded corner radius in logical pixels.
    pub corner_radius: u32,
    /// Optional RGBA tint overlay (`0..=255`), honored by `GlassStyle::Custom`.
    pub tint: Option<[u8; 4]>,
    /// Target surface-local region for the glass backdrop.
    pub region: GlassRegion,
}

impl LiquidGlassConfig {
    /// Popover-style frosted glass covering the full surface.
    #[must_use]
    pub const fn popover(corner_radius: u32) -> Self {
        Self {
            style: GlassStyle::Popover,
            corner_radius,
            tint: None,
            region: GlassRegion::FullSurface,
        }
    }

    /// Sidebar-style frosted glass covering the full surface.
    #[must_use]
    pub const fn sidebar(corner_radius: u32) -> Self {
        Self {
            style: GlassStyle::Sidebar,
            corner_radius,
            tint: None,
            region: GlassRegion::FullSurface,
        }
    }

    /// Custom-tinted frosted glass covering the full surface.
    #[must_use]
    pub const fn custom(corner_radius: u32, tint: [u8; 4]) -> Self {
        Self {
            style: GlassStyle::Custom,
            corner_radius,
            tint: Some(tint),
            region: GlassRegion::FullSurface,
        }
    }

    /// Overrides the target glass region.
    #[must_use]
    pub const fn with_region(mut self, region: GlassRegion) -> Self {
        self.region = region;
        self
    }
}
