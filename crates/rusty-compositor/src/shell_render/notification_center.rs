//! Right-edge macOS 27 Golden Gate `Notification Center` overlay (`RUSTYOS-109`).
//!
//! Renders delivered notification cards (`Lead + Notification Center stack`), interactive
//! `Clear All` / dismiss buttons, the gradient `Weather` forecast card, interactive `Reminders`
//! quick-task checklist widget, and `Edit Widgets` / close footer bar in front-to-back order.

use std::cell::RefCell;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Physical, Point, Rectangle, Size, Transform};

use crate::surfaces::{GlassPanelStyle, UiGlyphKind};

use super::CompositorRenderElement;
use super::tokens::{
    DOCK_HEIGHT, DOCK_MARGIN_BOTTOM, TOP_BAR_HEIGHT, ThemePalette, make_solid_element,
    push_liquid_glass_panel, push_rounded_glass_box, push_soft_shadow, truncate_ascii,
};
use super::top_bar::notification_app_icon;

const NC_WIDTH: i32 = 344;
const NC_RIGHT_MARGIN: i32 = 14;
const NC_TOP_MARGIN: i32 = 12;
const NC_CARD_HEIGHT: i32 = 68;
const NC_CARD_RADIUS: i32 = 20;
const NC_WIDGET_RADIUS: i32 = 22;
const NC_WEATHER_HEIGHT: i32 = 148;
const NC_REMINDERS_HEIGHT: i32 = 136;
const NC_EDIT_BAR_HEIGHT: i32 = 26;

/// Default checklist items displayed in the interactive Reminders widget (`RUSTYOS-109`).
pub const DEFAULT_NOTIFICATION_TASKS: [&str; 4] = [
    "Check System Updates",
    "Review Display Layout",
    "Sync Workspace Backup",
    "Verify Audio & Focus",
];

/// Interactive hit-test region inside [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationCenterHit {
    /// Dismiss a single notification card by ID.
    DismissNotification(u64),
    /// Clear all delivered notifications from Notification Center.
    ClearAll,
    /// Toggle checklist task item `0..4` inside the Reminders widget.
    ToggleTask(usize),
    /// Open System Settings on the `Notifications` pane (`Edit Widgets`).
    EditWidgets,
    /// Explicitly close Notification Center via the bottom circle `x` button.
    ClosePanel,
    /// Click landed inside the Notification Center column (do not dismiss).
    InsidePanel,
    /// Click landed outside Notification Center (dismiss overlay).
    OutsidePanel,
}

/// Read-only view of a single delivered notification inside Notification Center (`RUSTYOS-109`).
#[derive(Debug, Clone, Copy)]
pub struct NotificationCenterItemView<'a> {
    /// Monotonic compositor notification ID.
    pub id: u64,
    /// Freedesktop urgency level.
    pub urgency: rusty_ipc::Urgency,
    /// Formatted notification title / application prefix.
    pub title: &'a str,
    /// Sanitized body text.
    pub body: &'a str,
}

/// Live view state driving [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109`).
#[derive(Debug, Clone)]
pub struct NotificationCenterRenderState<'a> {
    /// Global shell glass opacity (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque fallback flag.
    pub reduce_transparency: bool,
    /// True when `[notifications].show_previews` hides body previews.
    pub hide_previews: bool,
    /// Ordered newest-first notification items.
    pub notifications: &'a [NotificationCenterItemView<'a>],
    /// Interactive checked state of the 4 Reminders checklist tasks.
    pub task_states: [bool; 4],
    /// Labels of the 4 Reminders checklist tasks.
    pub task_labels: [&'static str; 4],
    /// Currently hovered control inside Notification Center.
    pub hovered_hit: Option<NotificationCenterHit>,
    /// Weather card location heading.
    pub location_name: &'a str,
}

/// Positioned notification card slot in [`NotificationCenterLayout`] (`RUSTYOS-109`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationCardSlot {
    /// Notification identifier associated with this card.
    pub id: u64,
    /// Index into `NotificationCenterRenderState::notifications`.
    pub notif_index: usize,
    /// Full card bounding rectangle.
    pub card_rect: Rectangle<i32, Physical>,
    /// Circular hover close button rectangle.
    pub close_rect: Rectangle<i32, Physical>,
    /// True when a stacked-edge lip is drawn beneath this card.
    pub has_stack_edge: bool,
}

/// Precomputed layout rectangles for [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCenterLayout {
    /// Outer bounding rectangle enclosing all visible Notification Center cards and widgets.
    pub panel_rect: Rectangle<i32, Physical>,
    /// Optional lead notification banner above the `Notification Center` heading when `>= 2` items exist.
    pub lead_card: Option<NotificationCardSlot>,
    /// `"Notification Center"` section heading bounds.
    pub header_rect: Rectangle<i32, Physical>,
    /// Circular `x` clear-all button next to the heading when notifications exist.
    pub clear_all_rect: Option<Rectangle<i32, Physical>>,
    /// Placeholder card shown when the notification list is empty.
    pub empty_rect: Option<Rectangle<i32, Physical>>,
    /// Stacked notification cards rendered below `"Notification Center"`.
    pub stack_cards: Vec<NotificationCardSlot>,
    /// Gradient Weather widget bounds.
    pub weather_rect: Rectangle<i32, Physical>,
    /// Interactive Reminders checklist widget bounds.
    pub reminders_rect: Rectangle<i32, Physical>,
    /// Interactive checklist row hit rectangles (`0..4`).
    pub task_rows: [Rectangle<i32, Physical>; 4],
    /// Bottom `"Edit Widgets"` pill button bounds.
    pub edit_widgets_rect: Rectangle<i32, Physical>,
    /// Bottom circular `x` close button bounds.
    pub close_panel_rect: Rectangle<i32, Physical>,
}

type CachedWeatherCard = Option<((i32, i32, bool), MemoryRenderBuffer)>;

thread_local! {
    static WEATHER_CARD_CACHE: RefCell<CachedWeatherCard> = const { RefCell::new(None) };
}

fn point_in_rect(rect: Rectangle<i32, Physical>, x: f64, y: f64) -> bool {
    x >= f64::from(rect.loc.x)
        && x < f64::from(rect.loc.x + rect.size.w)
        && y >= f64::from(rect.loc.y)
        && y < f64::from(rect.loc.y + rect.size.h)
}

fn card_slot(
    id: u64,
    notif_index: usize,
    px: i32,
    y: i32,
    w: i32,
    has_stack_edge: bool,
) -> NotificationCardSlot {
    let card_rect = Rectangle::new(Point::from((px, y)), Size::from((w, NC_CARD_HEIGHT)));
    let close_rect = Rectangle::new(Point::from((px + w - 28, y + 10)), Size::from((20, 20)));
    NotificationCardSlot {
        id,
        notif_index,
        card_rect,
        close_rect,
        has_stack_edge,
    }
}

/// Computes Golden Gate layout for [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109`).
#[must_use]
pub fn compute_notification_center_layout(
    screen_size: Size<i32, Physical>,
    state: &NotificationCenterRenderState<'_>,
) -> NotificationCenterLayout {
    let w = NC_WIDTH.min((screen_size.w - 28).max(260));
    let px = (screen_size.w - w - NC_RIGHT_MARGIN).max(14);
    let top_y = TOP_BAR_HEIGHT + NC_TOP_MARGIN;
    let max_bottom = (screen_size.h - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - 16).max(520);
    let widgets_block_h = NC_WEATHER_HEIGHT + 12 + NC_REMINDERS_HEIGHT + 12 + NC_EDIT_BAR_HEIGHT;
    let cards_budget = (max_bottom - top_y - widgets_block_h - 14).max(110);

    let mut cur_y = top_y;
    let count = state.notifications.len();

    let lead_card = if count >= 2 {
        let slot = card_slot(state.notifications[0].id, 0, px, cur_y, w, count > 3);
        cur_y += NC_CARD_HEIGHT + 14;
        Some(slot)
    } else {
        None
    };

    let header_rect = Rectangle::new(Point::from((px + 2, cur_y)), Size::from((w - 34, 24)));
    let clear_all_rect = (count > 0)
        .then(|| Rectangle::new(Point::from((px + w - 24, cur_y)), Size::from((24, 24))));
    cur_y += 28;

    let mut empty_rect = None;
    let mut stack_cards = Vec::new();
    if count == 0 {
        let er = Rectangle::new(Point::from((px, cur_y)), Size::from((w, 48)));
        empty_rect = Some(er);
        cur_y += 48 + 14;
    } else {
        let start_idx = usize::from(lead_card.is_some());
        let avail_for_stack = (top_y + cards_budget - cur_y).max(NC_CARD_HEIGHT);
        let max_stack = usize::try_from(((avail_for_stack + 10) / (NC_CARD_HEIGHT + 10)).max(1))
            .unwrap_or(1)
            .min(4);
        let slice = &state.notifications[start_idx..count.min(start_idx + max_stack)];
        for (offset, item) in slice.iter().enumerate() {
            let notif_index = start_idx + offset;
            let is_last = offset + 1 == slice.len();
            let hidden_more = count > start_idx + slice.len();
            stack_cards.push(card_slot(
                item.id,
                notif_index,
                px,
                cur_y,
                w,
                is_last && hidden_more,
            ));
            cur_y += NC_CARD_HEIGHT + if is_last { 14 } else { 10 };
        }
    }

    let weather_rect = Rectangle::new(Point::from((px, cur_y)), Size::from((w, NC_WEATHER_HEIGHT)));
    cur_y += NC_WEATHER_HEIGHT + 12;

    let reminders_rect = Rectangle::new(
        Point::from((px, cur_y)),
        Size::from((w, NC_REMINDERS_HEIGHT)),
    );
    let task_x = px + 124;
    let task_w = (w - 134).max(80);
    let task_rows = std::array::from_fn(|idx| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let ry = reminders_rect.loc.y + 10 + (idx as i32) * 29;
        Rectangle::new(Point::from((task_x, ry)), Size::from((task_w, 29)))
    });
    cur_y += NC_REMINDERS_HEIGHT + 12;

    let edit_w = 104;
    let close_w = NC_EDIT_BAR_HEIGHT;
    let bar_w = edit_w + 10 + close_w;
    let bar_x = px + (w - bar_w) / 2;
    let edit_widgets_rect =
        Rectangle::new(Point::from((bar_x, cur_y)), Size::from((edit_w, close_w)));
    let close_panel_rect = Rectangle::new(
        Point::from((bar_x + edit_w + 10, cur_y)),
        Size::from((close_w, close_w)),
    );
    let bottom_y = cur_y + NC_EDIT_BAR_HEIGHT;

    NotificationCenterLayout {
        panel_rect: Rectangle::new(Point::from((px, top_y)), Size::from((w, bottom_y - top_y))),
        lead_card,
        header_rect,
        clear_all_rect,
        empty_rect,
        stack_cards,
        weather_rect,
        reminders_rect,
        task_rows,
        edit_widgets_rect,
        close_panel_rect,
    }
}

/// Hit-tests `(x, y)` against [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109`).
#[must_use]
pub fn hit_test_notification_center(
    screen_size: Size<i32, Physical>,
    state: &NotificationCenterRenderState<'_>,
    x: f64,
    y: f64,
) -> NotificationCenterHit {
    let layout = compute_notification_center_layout(screen_size, state);
    if let Some(clear_rect) = layout.clear_all_rect
        && point_in_rect(clear_rect, x, y)
    {
        return NotificationCenterHit::ClearAll;
    }
    for slot in layout.lead_card.iter().chain(layout.stack_cards.iter()) {
        if point_in_rect(slot.close_rect, x, y) || point_in_rect(slot.card_rect, x, y) {
            return NotificationCenterHit::DismissNotification(slot.id);
        }
    }
    for (idx, &row_rect) in layout.task_rows.iter().enumerate() {
        if point_in_rect(row_rect, x, y) {
            return NotificationCenterHit::ToggleTask(idx);
        }
    }
    if point_in_rect(layout.edit_widgets_rect, x, y) {
        return NotificationCenterHit::EditWidgets;
    }
    if point_in_rect(layout.close_panel_rect, x, y) {
        return NotificationCenterHit::ClosePanel;
    }
    if point_in_rect(layout.panel_rect, x, y) {
        return NotificationCenterHit::InsidePanel;
    }
    NotificationCenterHit::OutsidePanel
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn rasterize_weather_gradient_card(
    w: i32,
    h: i32,
    radius: i32,
    is_dark: bool,
) -> MemoryRenderBuffer {
    let mut mem = MemoryRenderBuffer::new(Fourcc::Argb8888, (w, h), 1, Transform::Normal, None);
    let top_rgb: [f32; 3] = if is_dark {
        [24.0, 66.0, 118.0]
    } else {
        [37.0, 96.0, 160.0]
    };
    let bot_rgb: [f32; 3] = if is_dark {
        [52.0, 102.0, 158.0]
    } else {
        [80.0, 139.0, 199.0]
    };
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    let hx = cx - radius as f32;
    let hy = cy - radius as f32;
    let r = radius as f32;
    let denom = (h - 1).max(1) as f32;

    {
        let mut ctx = mem.render();
        let _ = ctx.draw(|pixels| {
            let wu = usize::try_from(w).unwrap_or(0);
            for py in 0..h {
                let t = py as f32 / denom;
                let base_r = top_rgb[0] + (bot_rgb[0] - top_rgb[0]) * t;
                let base_g = top_rgb[1] + (bot_rgb[1] - top_rgb[1]) * t;
                let base_b = top_rgb[2] + (bot_rgb[2] - top_rgb[2]) * t;
                for px in 0..w {
                    let qx = (px as f32 + 0.5 - cx).abs() - hx;
                    let qy = (py as f32 + 0.5 - cy).abs() - hy;
                    let dist = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                    let cov = (0.5 - dist).clamp(0.0, 1.0);
                    if cov <= 0.001 {
                        continue;
                    }
                    let rim = if dist > -1.35 && py < h / 2 {
                        0.18 * (1.0 - ((-dist) / 1.35).clamp(0.0, 1.0))
                    } else {
                        0.0
                    };
                    let rr = (base_r + (255.0 - base_r) * rim).clamp(0.0, 255.0);
                    let gg = (base_g + (255.0 - base_g) * rim).clamp(0.0, 255.0);
                    let bb = (base_b + (255.0 - base_b) * rim).clamp(0.0, 255.0);
                    let idx = (usize::try_from(py).unwrap_or(0) * wu
                        + usize::try_from(px).unwrap_or(0))
                        * 4;
                    if idx + 4 <= pixels.len() {
                        pixels[idx] = (bb * cov).round() as u8;
                        pixels[idx + 1] = (gg * cov).round() as u8;
                        pixels[idx + 2] = (rr * cov).round() as u8;
                        pixels[idx + 3] = (242.0 * cov).round() as u8;
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((w, h)))])
        });
    }
    mem
}

fn push_weather_card_background(
    out: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, Physical>,
    is_dark: bool,
) {
    let key = (rect.size.w, rect.size.h, is_dark);
    let el = WEATHER_CARD_CACHE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let needs_rebuild = guard.as_ref().is_none_or(|(k, _)| *k != key);
        if needs_rebuild {
            *guard = Some((
                key,
                rasterize_weather_gradient_card(
                    rect.size.w,
                    rect.size.h,
                    NC_WIDGET_RADIUS,
                    is_dark,
                ),
            ));
        }
        guard.as_ref().and_then(|(_, buf)| {
            MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                (f64::from(rect.loc.x), f64::from(rect.loc.y)),
                buf,
                None,
                None,
                None,
                Kind::Unspecified,
            )
            .ok()
        })
    });
    if let Some(el) = el {
        out.push(CompositorRenderElement::Memory(el));
    }
}

fn push_label(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    text: &str,
    size: f32,
    rgba: [u8; 4],
    pos: (i32, i32),
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(el) = text_cache.render_label(renderer, text, size, rgba, Point::from(pos)) {
        elements.push(el);
    }
}

fn push_bold_label(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    text: &str,
    size: f32,
    rgba: [u8; 4],
    pos: (i32, i32),
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        text,
        crate::text::FontWeight::Bold,
        size,
        rgba,
        Point::from(pos),
    ) {
        elements.push(el);
    }
}

fn render_notification_card_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    slot: NotificationCardSlot,
    item: NotificationCenterItemView<'_>,
    hide_previews: bool,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let x = slot.card_rect.loc.x;
    let y = slot.card_rect.loc.y;
    let w = slot.card_rect.size.w;

    push_bold_label(
        text_cache,
        renderer,
        &truncate_ascii(item.title, 26),
        13.0,
        pal.popover_text_primary,
        (x + 64, y + 13),
        elements,
    );
    let body_src = if hide_previews {
        "Notification preview hidden"
    } else {
        item.body
    };
    push_label(
        text_cache,
        renderer,
        &truncate_ascii(body_src, 34),
        11.5,
        pal.popover_text_secondary,
        (x + 64, y + 34),
        elements,
    );

    let (nw, _) = text_cache.measure_text("now", 10.5);
    push_label(
        text_cache,
        renderer,
        "now",
        10.5,
        pal.popover_text_muted,
        (x + w - 16 - nw, y + 14),
        elements,
    );
}

fn render_weather_widget_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, Physical>,
    location_name: &str,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    const HOURS: [(&str, &str); 6] = [
        ("10 AM", "21°"),
        ("11 AM", "22°"),
        ("12 PM", "23°"),
        ("1 PM", "24°"),
        ("2 PM", "23°"),
        ("3 PM", "21°"),
    ];
    let x = rect.loc.x;
    let y = rect.loc.y;
    let w = rect.size.w;
    let white = [255, 255, 255, 255];
    let soft = [255, 255, 255, 212];

    push_bold_label(
        text_cache,
        renderer,
        &truncate_ascii(location_name, 18),
        13.5,
        white,
        (x + 16, y + 14),
        elements,
    );
    push_label(
        text_cache,
        renderer,
        "22°",
        28.0,
        white,
        (x + 16, y + 34),
        elements,
    );

    let cond = "Clear Sky";
    let (cw, _) = text_cache.measure_text(cond, 11.5);
    push_bold_label(
        text_cache,
        renderer,
        cond,
        11.5,
        white,
        (x + w - 16 - cw, y + 40),
        elements,
    );
    let hl = "H:24° L:15°";
    let (hw, _) = text_cache.measure_text(hl, 10.5);
    push_label(
        text_cache,
        renderer,
        hl,
        10.5,
        soft,
        (x + w - 16 - hw, y + 57),
        elements,
    );

    let col_w = (w - 24) / 6;
    for (idx, (hr, temp)) in HOURS.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let cx = x + 12 + (idx as i32) * col_w;
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            hr,
            10.0,
            soft,
            Rectangle::new(Point::from((cx, y + 84)), Size::from((col_w, 16))),
        ) {
            elements.push(el);
        }
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            temp,
            crate::text::FontWeight::Bold,
            12.0,
            white,
            Rectangle::new(Point::from((cx, y + 122)), Size::from((col_w, 18))),
        ) {
            elements.push(el);
        }
    }
}

fn render_reminders_widget_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &NotificationCenterLayout,
    state: &NotificationCenterRenderState<'_>,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let rx = layout.reminders_rect.loc.x;
    let ry = layout.reminders_rect.loc.y;
    let remaining = state.task_states.iter().filter(|done| !**done).count();
    let count_str = remaining.to_string();

    push_bold_label(
        text_cache,
        renderer,
        &count_str,
        28.0,
        pal.popover_text_primary,
        (rx + 18, ry + 56),
        elements,
    );
    push_bold_label(
        text_cache,
        renderer,
        "Reminders",
        13.0,
        pal.accent_rgba,
        (rx + 18, ry + 98),
        elements,
    );

    for (idx, (&row_rect, label)) in layout
        .task_rows
        .iter()
        .zip(state.task_labels.iter())
        .enumerate()
    {
        let done = state.task_states[idx];
        let fg = if done {
            pal.popover_text_muted
        } else {
            pal.popover_text_primary
        };
        push_label(
            text_cache,
            renderer,
            &truncate_ascii(label, 24),
            12.0,
            fg,
            (row_rect.loc.x + 24, row_rect.loc.y + 7),
            elements,
        );
    }
}

/// Appends foreground labels for [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109`).
pub(super) fn render_notification_center_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &NotificationCenterRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = compute_notification_center_layout(screen_size, state);
    let pal = ThemePalette::active();

    if let Some(lead) = layout.lead_card
        && let Some(&item) = state.notifications.get(lead.notif_index)
    {
        render_notification_card_text(
            text_cache,
            renderer,
            lead,
            item,
            state.hide_previews,
            &pal,
            elements,
        );
    }

    push_bold_label(
        text_cache,
        renderer,
        "Notification Center",
        15.0,
        [255, 255, 255, 244],
        (layout.header_rect.loc.x, layout.header_rect.loc.y + 2),
        elements,
    );

    if let Some(empty) = layout.empty_rect
        && let Some(el) = text_cache.render_label_centered(
            renderer,
            "No New Notifications",
            12.5,
            pal.popover_text_muted,
            Rectangle::new(
                Point::from((empty.loc.x, empty.loc.y)),
                Size::from((empty.size.w, empty.size.h)),
            ),
        )
    {
        elements.push(el);
    }

    for &slot in &layout.stack_cards {
        if let Some(&item) = state.notifications.get(slot.notif_index) {
            render_notification_card_text(
                text_cache,
                renderer,
                slot,
                item,
                state.hide_previews,
                &pal,
                elements,
            );
        }
    }

    render_weather_widget_text(
        text_cache,
        renderer,
        layout.weather_rect,
        state.location_name,
        elements,
    );
    render_reminders_widget_text(text_cache, renderer, &layout, state, &pal, elements);

    let edit_fg = if state.hovered_hit == Some(NotificationCenterHit::EditWidgets) {
        [255, 255, 255, 255]
    } else {
        pal.popover_text_primary
    };
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        "Edit Widgets",
        crate::text::FontWeight::Bold,
        11.5,
        edit_fg,
        Rectangle::new(
            Point::from((
                layout.edit_widgets_rect.loc.x,
                layout.edit_widgets_rect.loc.y,
            )),
            Size::from((
                layout.edit_widgets_rect.size.w,
                layout.edit_widgets_rect.size.h,
            )),
        ),
    ) {
        elements.push(el);
    }
}

fn push_notification_card_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    slot: NotificationCardSlot,
    item: NotificationCenterItemView<'_>,
    state: &NotificationCenterRenderState<'_>,
    pal: &ThemePalette,
) {
    let x = slot.card_rect.loc.x;
    let y = slot.card_rect.loc.y;
    let hovered = state.hovered_hit == Some(NotificationCenterHit::DismissNotification(slot.id));

    if hovered {
        let close_fill = if pal.is_dark {
            [255, 255, 255, 44]
        } else {
            [0, 0, 0, 26]
        };
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Close,
            10,
            pal.popover_icon,
            Point::from((slot.close_rect.loc.x + 5, slot.close_rect.loc.y + 5)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if let Some(el) = shape_cache.circle_element(
            renderer,
            20,
            close_fill,
            Some(pal.border_highlight_rgba),
            Point::from((slot.close_rect.loc.x, slot.close_rect.loc.y)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    let app_icon = notification_app_icon(item.title);
    if let Some(icon_el) = shape_cache.icons.builtin_squircle_icon_element(
        renderer,
        app_icon,
        38,
        Point::from((x + 14, y + 15)),
    ) {
        out.push(CompositorRenderElement::Memory(icon_el));
    }

    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        slot.card_rect,
        NC_CARD_RADIUS,
        GlassPanelStyle::Popover,
        state.glass_opacity,
        state.reduce_transparency,
        Some((14, 44)),
    );

    if slot.has_stack_edge {
        let lip = Rectangle::new(
            Point::from((x + 10, slot.card_rect.loc.y + slot.card_rect.size.h - 14)),
            Size::from(((slot.card_rect.size.w - 20).max(80), 20)),
        );
        push_liquid_glass_panel(
            out,
            shape_cache,
            renderer,
            lip,
            16,
            GlassPanelStyle::Popover,
            state.glass_opacity,
            state.reduce_transparency,
            Some((10, 32)),
        );
    }
}

fn push_weather_widget_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    weather_rect: Rectangle<i32, Physical>,
    is_dark: bool,
) {
    let wx = weather_rect.loc.x;
    let wy = weather_rect.loc.y;
    let ww = weather_rect.size.w;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Sun,
        20,
        [255, 214, 10, 255],
        Point::from((wx + ww - 36, wy + 15)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let col_w = (ww - 24) / 6;
    for idx in 0..6 {
        let cx = wx + 12 + idx * col_w + (col_w - 16) / 2;
        let glyph = if idx < 4 {
            UiGlyphKind::Sun
        } else {
            UiGlyphKind::CloudSun
        };
        let tint = if idx < 4 {
            [255, 214, 10, 255]
        } else {
            [255, 255, 255, 245]
        };
        if let Some(el) =
            shape_cache.ui_glyph_element(renderer, glyph, 16, tint, Point::from((cx, wy + 102)))
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
    push_weather_card_background(out, renderer, weather_rect, is_dark);
    push_soft_shadow(
        out,
        shape_cache,
        renderer,
        weather_rect,
        NC_WIDGET_RADIUS,
        16,
        44,
    );
}

fn push_reminders_widget_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &NotificationCenterLayout,
    state: &NotificationCenterRenderState<'_>,
    pal: &ThemePalette,
) {
    let rx = layout.reminders_rect.loc.x;
    let ry = layout.reminders_rect.loc.y;
    for (idx, &row_rect) in layout.task_rows.iter().enumerate() {
        if state.task_states[idx]
            && let Some(el) = shape_cache.ui_glyph_element(
                renderer,
                UiGlyphKind::Check,
                10,
                [255, 255, 255, 255],
                Point::from((row_rect.loc.x + 6, row_rect.loc.y + 9)),
            )
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Checklist,
        18,
        [255, 255, 255, 255],
        Point::from((rx + 25, ry + 21)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.circle_element(
        renderer,
        32,
        pal.accent_rgba,
        None,
        Point::from((rx + 18, ry + 14)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    for (idx, &row_rect) in layout.task_rows.iter().enumerate() {
        let done = state.task_states[idx];
        let (fill, border) = if done {
            (pal.accent_rgba, None)
        } else {
            let b = if pal.is_dark {
                [140, 144, 152, 165]
            } else {
                [160, 164, 174, 190]
            };
            ([0, 0, 0, 0], Some(b))
        };
        if let Some(el) = shape_cache.circle_element(
            renderer,
            14,
            fill,
            border,
            Point::from((row_rect.loc.x + 4, row_rect.loc.y + 7)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if state.hovered_hit == Some(NotificationCenterHit::ToggleTask(idx)) {
            let hover_fill = if pal.is_dark {
                [255, 255, 255, 16]
            } else {
                [0, 0, 0, 12]
            };
            push_rounded_glass_box(out, shape_cache, renderer, row_rect, 7, hover_fill, None);
        }
        if idx < 3 {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(
                    Point::from((row_rect.loc.x + 4, row_rect.loc.y + row_rect.size.h - 1)),
                    Size::from(((row_rect.size.w - 8).max(20), 1)),
                ),
                pal.popover_separator,
            )));
        }
    }

    let rem_fill = if pal.is_dark {
        [44, 44, 48, 244]
    } else {
        [255, 255, 255, 248]
    };
    let rem_border = if pal.is_dark {
        Some([255, 255, 255, 24])
    } else {
        Some([0, 0, 0, 14])
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.reminders_rect,
        NC_WIDGET_RADIUS,
        rem_fill,
        rem_border,
    );
    push_soft_shadow(
        out,
        shape_cache,
        renderer,
        layout.reminders_rect,
        NC_WIDGET_RADIUS,
        16,
        44,
    );
}

fn push_clear_and_bottom_bars_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &NotificationCenterLayout,
    state: &NotificationCenterRenderState<'_>,
    pal: &ThemePalette,
) {
    if state.hovered_hit == Some(NotificationCenterHit::EditWidgets) {
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            layout.edit_widgets_rect,
            13,
            pal.accent_rgba,
            None,
        );
    } else {
        push_liquid_glass_panel(
            out,
            shape_cache,
            renderer,
            layout.edit_widgets_rect,
            13,
            GlassPanelStyle::Popover,
            state.glass_opacity,
            state.reduce_transparency,
            Some((10, 32)),
        );
    }

    let close_hovered = state.hovered_hit == Some(NotificationCenterHit::ClosePanel);
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Close,
        10,
        if close_hovered {
            [255, 255, 255, 255]
        } else {
            pal.popover_icon
        },
        Point::from((
            layout.close_panel_rect.loc.x + 8,
            layout.close_panel_rect.loc.y + 8,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if close_hovered {
        if let Some(el) = shape_cache.circle_element(
            renderer,
            26,
            pal.accent_rgba,
            None,
            Point::from((layout.close_panel_rect.loc.x, layout.close_panel_rect.loc.y)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    } else {
        push_liquid_glass_panel(
            out,
            shape_cache,
            renderer,
            layout.close_panel_rect,
            13,
            GlassPanelStyle::Popover,
            state.glass_opacity,
            state.reduce_transparency,
            Some((10, 32)),
        );
    }
}

/// Appends Liquid Glass cards, widgets, icons, and hover highlights for [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109`).
pub(super) fn render_notification_center_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &NotificationCenterRenderState<'_>,
) {
    let layout = compute_notification_center_layout(screen_size, state);
    let pal = ThemePalette::active();

    if let Some(lead) = layout.lead_card
        && let Some(&item) = state.notifications.get(lead.notif_index)
    {
        push_notification_card_composite(out, shape_cache, renderer, lead, item, state, &pal);
    }

    if let Some(clear_rect) = layout.clear_all_rect {
        let hovered = state.hovered_hit == Some(NotificationCenterHit::ClearAll);
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Close,
            10,
            if hovered {
                [255, 255, 255, 255]
            } else {
                pal.popover_icon
            },
            Point::from((clear_rect.loc.x + 7, clear_rect.loc.y + 7)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if hovered {
            if let Some(el) = shape_cache.circle_element(
                renderer,
                24,
                pal.accent_rgba,
                None,
                Point::from((clear_rect.loc.x, clear_rect.loc.y)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
        } else {
            push_liquid_glass_panel(
                out,
                shape_cache,
                renderer,
                clear_rect,
                12,
                GlassPanelStyle::Popover,
                state.glass_opacity,
                state.reduce_transparency,
                Some((8, 28)),
            );
        }
    }

    if let Some(empty_rect) = layout.empty_rect {
        push_liquid_glass_panel(
            out,
            shape_cache,
            renderer,
            empty_rect,
            16,
            GlassPanelStyle::Popover,
            state.glass_opacity,
            state.reduce_transparency,
            Some((12, 36)),
        );
    }

    for &slot in &layout.stack_cards {
        if let Some(&item) = state.notifications.get(slot.notif_index) {
            push_notification_card_composite(out, shape_cache, renderer, slot, item, state, &pal);
        }
    }

    push_weather_widget_composite(out, shape_cache, renderer, layout.weather_rect, pal.is_dark);
    push_reminders_widget_composite(out, shape_cache, renderer, &layout, state, &pal);
    push_clear_and_bottom_bars_composite(out, shape_cache, renderer, &layout, state, &pal);
}
