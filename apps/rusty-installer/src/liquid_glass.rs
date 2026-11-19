//! Client-side bindings for `rustyos_liquid_glass_v1` (`RUSTYOS-115`).

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
