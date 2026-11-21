//! `TopBar` layout/hit-testing/rendering, adaptive backdrop, global menus, Notification HUD, and OSD pill.
//!
//! Submodules (`RUSTYOS-090`, `RUSTYOS-141`..`143`):
//! - `backdrop`: adaptive wallpaper luminance/detail analysis for `TopBar` foreground contrast.
//! - `panel`: `TopBar` bar layouts, workspace indicator strips, telemetry, and main bar renderer.
//! - `menus`: dynamic Menu Bar items, popovers, keybinding hints, and action routing.
//! - `status`: Control Center trigger, SNI tray slots, updates badge, battery/wifi/privacy glyphs.
//! - `notifications`: Notification HUD toasts, unread badge pills, and floating OSD volume/brightness pills.

pub(crate) mod backdrop;
pub(crate) mod menus;
pub(crate) mod notifications;
pub(crate) mod panel;
pub(crate) mod status;

pub use backdrop::*;
pub use menus::*;
pub use notifications::*;
pub use panel::*;
pub use status::*;
