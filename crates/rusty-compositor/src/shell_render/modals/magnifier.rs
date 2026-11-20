//! `PickColor` magnifier loupe HUD pill layout and Liquid Glass rendering (`RUSTYOS-091`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{
    TOP_BAR_HEIGHT, ThemePalette, push_liquid_glass_panel_elevation, truncate_ascii,
};
use crate::shell_render::top_bar::TopBarTelemetry;

use super::phys_to_logical_rect;

pub(crate) fn pick_color_pill_rect(
    screen_size: Size<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    let w = screen_size.w.max(320);
    let pw = 390.min(w - 32).max(240);
    let ph = 36;
    let px = (w - pw) / 2;
    let py = TOP_BAR_HEIGHT + 12;
    Rectangle::new(Point::from((px, py)), Size::from((pw, ph)))
}

pub(crate) fn render_pick_color_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    app_id: &str,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pal = ThemePalette::active();
    let pill = pick_color_pill_rect(screen_size);
    let msg = truncate_ascii(
        &format!(
            "Pick Color ({}) · Click pixel · Esc cancel",
            if app_id.is_empty() { "App" } else { app_id }
        ),
        48,
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        &msg,
        crate::text::FontWeight::Bold,
        12.0,
        pal.popover_text_primary,
        phys_to_logical_rect(pill),
    ) {
        elements.push(el);
    }
}

pub(crate) fn render_pick_color_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
) {
    let pill = pick_color_pill_rect(screen_size);
    push_liquid_glass_panel_elevation(
        out,
        shape_cache,
        renderer,
        pill,
        12,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        rusty_ui::shadow::ShadowElevation::Popup,
    );
}
