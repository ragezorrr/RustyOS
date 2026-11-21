//! `PolicyKit` authentication modal, XDG Portal consent dialog, and `PickColor` banner.
//!
//! Submodules (`RUSTYOS-091`):
//! - `polkit`: `PolicyKit` authentication prompt modal, password field, and verifying feedback.
//! - `portal`: XDG Desktop Portal source-selection consent modal (`ScreenCast`, `Screenshot`).
//! - `magnifier`: `PickColor` magnifier loupe HUD banner and pixel selector.

use smithay::utils::{Point, Rectangle, Size};

pub(crate) mod magnifier;
pub(crate) mod polkit;
pub(crate) mod portal;
pub(crate) mod screenshot_hud;

pub(crate) use magnifier::*;
pub use polkit::*;
pub use portal::*;
pub use screenshot_hud::*;

pub(crate) fn phys_to_logical_rect(
    rect: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Logical> {
    Rectangle::new(
        Point::from((rect.loc.x, rect.loc.y)),
        Size::from((rect.size.w, rect.size.h)),
    )
}
