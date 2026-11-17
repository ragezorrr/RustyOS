//! `TopBar` layout/hit-testing/rendering, global menus, Notification HUD, and OSD pill.
//!
//! Submodules decomposed per `CutefishOS` / macOS 27 architecture (`RUSTYOS-090`):
//! - `panel`: `TopBar` bar layouts, workspace indicator strips, telemetry, and main bar renderer.
//! - `menus`: macOS-style dynamic Menu Bar items, popovers, keybinding hints, and action routing.
//! - `status`: Control Center trigger, SNI tray slots, updates badge, battery/wifi glyphs.
//! - `notifications`: Notification HUD toasts, unread badge pills, and floating OSD volume/brightness pills.

pub(crate) mod menus;
pub(crate) mod notifications;
pub(crate) mod panel;
pub(crate) mod status;

pub use menus::*;
pub use notifications::*;
pub use panel::*;
pub use status::*;
