//! Shared Wayland/SCTK, GPU GLES2/SHM, and Liquid Glass application host for RustyOS apps.
//!
//! Centralizes the Wayland event loop, `xdg_toplevel` lifecycle, hardware-accelerated
//! [`rusty_ui::GlesSurfaceRenderer`] presentation (with automatic CPU `wl_shm` fallback),
//! and canonical `rustyos_liquid_glass_v1` client bindings so native apps remain thin and
//! consistent.

#![forbid(unsafe_code)]

pub mod app_menu;
pub mod host;
pub mod liquid_glass;

pub use app_menu::{
    AppMenuItemSpec, AppMenuSectionSpec, RustyosAppMenuManagerV1, RustyosSurfaceAppMenuV1,
};

pub use host::{
    AppConfig, AppContext, AppError, ConfigureInfo, CursorShape, PopupConfig, ResizeEdge, RustyApp,
    WindowDecorationMode, run_app,
};
pub use liquid_glass::{
    GlassRegion, GlassStyle, LiquidGlassConfig, RustyosLiquidGlassManagerV1, RustyosSurfaceGlassV1,
};
pub use smithay_client_toolkit::seat::{
    keyboard::{KeyEvent, Keysym, Modifiers},
    pointer::{BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, PointerEvent, PointerEventKind},
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_builder_sets_defaults_and_glass() {
        let cfg = AppConfig::new("Viewer", "org.rustyos.view", (900, 650))
            .with_min_size(400, 300)
            .with_decorations(WindowDecorationMode::Client)
            .with_fullscreen(false)
            .with_liquid_glass(LiquidGlassConfig::popover(14));

        assert_eq!(cfg.title, "Viewer");
        assert_eq!(cfg.app_id, "org.rustyos.view");
        assert_eq!(cfg.default_size, (900, 650));
        assert_eq!(cfg.min_size, Some((400, 300)));
        assert_eq!(cfg.decorations, WindowDecorationMode::Client);
        assert!(!cfg.fullscreen);
        assert_eq!(
            cfg.glass,
            Some(LiquidGlassConfig {
                style: GlassStyle::Popover,
                corner_radius: 14,
                tint: None,
                region: GlassRegion::FullSurface,
            })
        );
    }

    #[test]
    fn liquid_glass_config_supports_custom_tint_and_regions() {
        let custom =
            LiquidGlassConfig::custom(16, [24, 28, 36, 190]).with_region(GlassRegion::Rect {
                x: 0,
                y: 0,
                width: 240,
                height: 600,
            });
        assert_eq!(custom.style, GlassStyle::Custom);
        assert_eq!(custom.corner_radius, 16);
        assert_eq!(custom.tint, Some([24, 28, 36, 190]));
        assert_eq!(
            custom.region,
            GlassRegion::Rect {
                x: 0,
                y: 0,
                width: 240,
                height: 600,
            }
        );
    }
}
