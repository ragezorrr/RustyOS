//! Right-edge RustyOS `Notification Center` overlay (`RUSTYOS-109`).
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
    push_elevation_shadow, push_liquid_glass_panel_elevation, push_rounded_glass_box,
    truncate_ascii,
};
use super::top_bar::{
    NOTIFICATION_CARD_HEIGHT, NOTIFICATION_CARD_RADIUS, NOTIFICATION_CARD_RIGHT_MARGIN,
    NOTIFICATION_CARD_TOP_MARGIN, NOTIFICATION_CARD_WIDTH, push_notification_card_chrome,
    push_notification_card_text_lines,
};

const NC_WIDTH: i32 = NOTIFICATION_CARD_WIDTH;
const NC_RIGHT_MARGIN: i32 = NOTIFICATION_CARD_RIGHT_MARGIN;
const NC_TOP_MARGIN: i32 = NOTIFICATION_CARD_TOP_MARGIN;
const NC_CARD_HEIGHT: i32 = NOTIFICATION_CARD_HEIGHT;
const NC_CARD_RADIUS: i32 = NOTIFICATION_CARD_RADIUS;
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
    /// Toggle expand/collapse for a notification card stack group by root notification ID (`RUSTYOS-173`).
    ToggleStack(u64),
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

/// Read-only view of a single delivered notification inside Notification Center (`RUSTYOS-109` / `RUSTYOS-173`).
#[derive(Debug, Clone)]
pub struct NotificationCenterItemView<'a> {
    /// Monotonic compositor notification ID.
    pub id: u64,
    /// Freedesktop urgency level.
    pub urgency: rusty_ipc::Urgency,
    /// Canonical application key.
    pub app_key: Option<&'a str>,
    /// Human-readable application display name (`"Telegram Desktop"`).
    pub app_name: Option<&'a str>,
    /// Formatted notification title / sender name.
    pub title: &'a str,
    /// Sanitized body text.
    pub body: &'a str,
    /// Concise relative age label (`"now"`, `"2m ago"`, `"1h ago"`) (`RUSTYOS-156`).
    pub age_label: String,
    /// Optional contact avatar pixmap (`RUSTYOS-173`).
    pub avatar: Option<&'a crate::state::NotificationAvatar>,
}

/// Live view state driving [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct NotificationCenterRenderState<'a> {
    /// Global shell glass opacity (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque fallback flag.
    pub reduce_transparency: bool,
    /// True when `[notifications].show_previews` hides body previews.
    pub hide_previews: bool,
    /// Ordered newest-first notification items.
    pub notifications: &'a [NotificationCenterItemView<'a>],
    /// Group keys of expanded notification stacks (`RUSTYOS-173`).
    pub expanded_stacks: &'a std::collections::HashSet<String>,
    /// Interactive checked state of the 4 Reminders checklist tasks.
    pub task_states: [bool; 4],
    /// Labels of the 4 Reminders checklist tasks.
    pub task_labels: [&'static str; 4],
    /// Currently hovered control inside Notification Center.
    pub hovered_hit: Option<NotificationCenterHit>,
    /// Weather card location / date heading.
    pub location_name: &'a str,
    /// Live system telemetry summary line rendered inside the top widget (`RUSTYOS-156`).
    pub system_summary: String,
    /// Show the top Weather & System widget (`RUSTYOS-156`).
    pub show_weather_widget: bool,
    /// Show the bottom Reminders checklist widget (`RUSTYOS-156`).
    pub show_reminders_widget: bool,
}

/// Positioned notification card slot in [`NotificationCenterLayout`] (`RUSTYOS-109` / `RUSTYOS-173`).
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
    /// Total count of notifications in this stack group.
    pub stack_count: usize,
    /// True if this stack is expanded.
    pub is_expanded: bool,
    /// Group root notification identifier.
    pub group_id: u64,
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

#[allow(clippy::too_many_arguments)]
fn card_slot(
    id: u64,
    notif_index: usize,
    px: i32,
    y: i32,
    w: i32,
    has_stack_edge: bool,
    stack_count: usize,
    is_expanded: bool,
    group_id: u64,
) -> NotificationCardSlot {
    let card_rect = Rectangle::new(Point::from((px, y)), Size::from((w, NC_CARD_HEIGHT)));
    let close_rect = Rectangle::new(Point::from((px + w - 28, y + 10)), Size::from((20, 20)));
    NotificationCardSlot {
        id,
        notif_index,
        card_rect,
        close_rect,
        has_stack_edge,
        stack_count,
        is_expanded,
        group_id,
    }
}

struct AppGroup {
    key: String,
    items: Vec<(usize, u64)>,
}

/// Computes RustyOS layout for [`ShellOverlayView::NotificationCenter`] (`RUSTYOS-109` / `RUSTYOS-173`).
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn compute_notification_center_layout(
    screen_size: Size<i32, Physical>,
    state: &NotificationCenterRenderState<'_>,
) -> NotificationCenterLayout {
    let w = NC_WIDTH.min((screen_size.w - 28).max(260));
    let px = (screen_size.w - w - NC_RIGHT_MARGIN).max(14);
    let top_y = TOP_BAR_HEIGHT + NC_TOP_MARGIN;
    let max_bottom = (screen_size.h - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - 16).max(520);
    let weather_block = i32::from(state.show_weather_widget) * (NC_WEATHER_HEIGHT + 12);
    let reminders_block = i32::from(state.show_reminders_widget) * (NC_REMINDERS_HEIGHT + 12);
    let widgets_block_h = weather_block + reminders_block + NC_EDIT_BAR_HEIGHT;
    let cards_budget = (max_bottom - top_y - widgets_block_h - 14).max(110);

    let mut cur_y = top_y;
    let count = state.notifications.len();

    // Group notifications by app/source into stacks ("стос") (`RUSTYOS-173`)
    let mut groups: Vec<AppGroup> = Vec::new();
    for (idx, item) in state.notifications.iter().enumerate() {
        let key = item
            .app_key
            .or(item.app_name)
            .unwrap_or(item.title)
            .to_owned();
        if let Some(g) = groups.iter_mut().find(|g| g.key == key) {
            g.items.push((idx, item.id));
        } else {
            groups.push(AppGroup {
                key,
                items: vec![(idx, item.id)],
            });
        }
    }

    let (lead_card, remaining_groups) = if count >= 2 && !groups.is_empty() {
        let first = &groups[0];
        let root_id = first.items[0].1;
        let stack_count = first.items.len();
        let is_expanded = state.expanded_stacks.contains(&root_id.to_string())
            || state.expanded_stacks.contains(&first.key);
        let has_stack_edge = stack_count > 1 && !is_expanded;
        let slot = card_slot(
            root_id,
            first.items[0].0,
            px,
            cur_y,
            w,
            has_stack_edge,
            stack_count,
            is_expanded,
            root_id,
        );
        cur_y += NC_CARD_HEIGHT + if has_stack_edge { 16 } else { 14 };
        (Some(slot), &groups[1..])
    } else {
        (None, groups.as_slice())
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
        if let Some(ref lead) = lead_card
            && lead.is_expanded
            && !groups.is_empty()
        {
            for &(notif_index, id) in &groups[0].items[1..] {
                stack_cards.push(card_slot(
                    id,
                    notif_index,
                    px,
                    cur_y,
                    w,
                    false,
                    lead.stack_count,
                    true,
                    lead.group_id,
                ));
                cur_y += NC_CARD_HEIGHT + 10;
            }
        }

        let avail_for_stack = (top_y + cards_budget - cur_y).max(NC_CARD_HEIGHT);
        let max_stack_slots =
            usize::try_from(((avail_for_stack + 10) / (NC_CARD_HEIGHT + 10)).max(1))
                .unwrap_or(1)
                .min(6);

        for group in remaining_groups {
            if stack_cards.len() >= max_stack_slots {
                break;
            }
            let root_id = group.items[0].1;
            let stack_count = group.items.len();
            let is_expanded = state.expanded_stacks.contains(&root_id.to_string())
                || state.expanded_stacks.contains(&group.key);
            if is_expanded {
                for &(notif_index, id) in &group.items {
                    if stack_cards.len() >= max_stack_slots {
                        break;
                    }
                    stack_cards.push(card_slot(
                        id,
                        notif_index,
                        px,
                        cur_y,
                        w,
                        false,
                        stack_count,
                        true,
                        root_id,
                    ));
                    cur_y += NC_CARD_HEIGHT + 10;
                }
            } else {
                let has_stack_edge = stack_count > 1;
                stack_cards.push(card_slot(
                    root_id,
                    group.items[0].0,
                    px,
                    cur_y,
                    w,
                    has_stack_edge,
                    stack_count,
                    false,
                    root_id,
                ));
                cur_y += NC_CARD_HEIGHT + if has_stack_edge { 16 } else { 10 };
            }
        }
    }

    let weather_rect = if state.show_weather_widget {
        let r = Rectangle::new(Point::from((px, cur_y)), Size::from((w, NC_WEATHER_HEIGHT)));
        cur_y += NC_WEATHER_HEIGHT + 12;
        r
    } else {
        Rectangle::new(Point::from((px, cur_y)), Size::from((0, 0)))
    };

    let (reminders_rect, task_rows) = if state.show_reminders_widget {
        let rem_rect = Rectangle::new(
            Point::from((px, cur_y)),
            Size::from((w, NC_REMINDERS_HEIGHT)),
        );
        let task_x = px + 124;
        let task_w = (w - 134).max(80);
        let rows = std::array::from_fn(|idx| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let ry = rem_rect.loc.y + 10 + (idx as i32) * 29;
            Rectangle::new(Point::from((task_x, ry)), Size::from((task_w, 29)))
        });
        cur_y += NC_REMINDERS_HEIGHT + 12;
        (rem_rect, rows)
    } else {
        let empty = Rectangle::new(Point::from((px, cur_y)), Size::from((0, 0)));
        (empty, [empty; 4])
    };

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
        if point_in_rect(slot.close_rect, x, y) {
            return NotificationCenterHit::DismissNotification(slot.id);
        }
        if point_in_rect(slot.card_rect, x, y) {
            if slot.stack_count > 1 {
                return NotificationCenterHit::ToggleStack(slot.group_id);
            }
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
    item: &NotificationCenterItemView<'_>,
    hide_previews: bool,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let body_src = if hide_previews {
        "Notification preview hidden"
    } else {
        item.body
    };
    push_notification_card_text_lines(
        text_cache,
        renderer,
        slot.card_rect,
        item.title,
        body_src,
        pal,
        elements,
    );
    if slot.stack_count > 1 && !slot.is_expanded {
        let badge_text = format!("+{}", slot.stack_count - 1);
        let badge_x = slot.card_rect.loc.x + slot.card_rect.size.w - 42;
        let badge_y = slot.card_rect.loc.y + slot.card_rect.size.h - 22;
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            &badge_text,
            crate::text::FontWeight::Bold,
            11.5,
            pal.accent_rgba,
            Point::from((badge_x, badge_y)),
        ) {
            elements.push(el);
        }
    }
}

fn render_weather_widget_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, Physical>,
    location_name: &str,
    system_summary: &str,
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
    let hl = if system_summary.trim().is_empty() {
        "H:24° L:15°".to_owned()
    } else {
        truncate_ascii(system_summary.trim(), 24)
    };
    let (hw, _) = text_cache.measure_text(&hl, 10.5);
    push_label(
        text_cache,
        renderer,
        &hl,
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
        && let Some(item) = state.notifications.get(lead.notif_index)
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

    let heading_color = if pal.is_dark {
        [255, 255, 255, 244]
    } else {
        [24, 26, 32, 240]
    };
    push_bold_label(
        text_cache,
        renderer,
        "Notification Center",
        15.0,
        heading_color,
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
        if let Some(item) = state.notifications.get(slot.notif_index) {
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

    if state.show_weather_widget {
        render_weather_widget_text(
            text_cache,
            renderer,
            layout.weather_rect,
            state.location_name,
            &state.system_summary,
            elements,
        );
    }
    if state.show_reminders_widget {
        render_reminders_widget_text(text_cache, renderer, &layout, state, &pal, elements);
    }

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
    item: &NotificationCenterItemView<'_>,
    state: &NotificationCenterRenderState<'_>,
    pal: &ThemePalette,
) {
    let hovered = state.hovered_hit == Some(NotificationCenterHit::DismissNotification(slot.id));
    let app_icon_key = item.app_key.or(item.app_name);
    push_notification_card_chrome(
        out,
        shape_cache,
        renderer,
        slot.card_rect,
        slot.close_rect,
        item.title,
        app_icon_key,
        item.avatar,
        hovered,
        state.glass_opacity,
        state.reduce_transparency,
        pal,
    );

    if slot.has_stack_edge && slot.stack_count > 1 {
        let lip1 = Rectangle::new(
            Point::from((
                slot.card_rect.loc.x + 16,
                slot.card_rect.loc.y + slot.card_rect.size.h - 6,
            )),
            Size::from(((slot.card_rect.size.w - 32).max(80), 16)),
        );
        push_liquid_glass_panel_elevation(
            out,
            shape_cache,
            renderer,
            lip1,
            16,
            GlassPanelStyle::NotificationCard,
            state.glass_opacity * 0.75,
            state.reduce_transparency,
            rusty_ui::shadow::ShadowElevation::Popup,
        );

        let lip2 = Rectangle::new(
            Point::from((
                slot.card_rect.loc.x + 8,
                slot.card_rect.loc.y + slot.card_rect.size.h - 10,
            )),
            Size::from(((slot.card_rect.size.w - 16).max(80), 18)),
        );
        push_liquid_glass_panel_elevation(
            out,
            shape_cache,
            renderer,
            lip2,
            17,
            GlassPanelStyle::NotificationCard,
            state.glass_opacity * 0.9,
            state.reduce_transparency,
            rusty_ui::shadow::ShadowElevation::Popup,
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
    push_elevation_shadow(
        out,
        shape_cache,
        renderer,
        weather_rect,
        NC_WIDGET_RADIUS,
        rusty_ui::shadow::ShadowElevation::Popup,
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
    push_elevation_shadow(
        out,
        shape_cache,
        renderer,
        layout.reminders_rect,
        NC_WIDGET_RADIUS,
        rusty_ui::shadow::ShadowElevation::Popup,
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
        push_liquid_glass_panel_elevation(
            out,
            shape_cache,
            renderer,
            layout.edit_widgets_rect,
            13,
            GlassPanelStyle::Popover,
            state.glass_opacity,
            state.reduce_transparency,
            rusty_ui::shadow::ShadowElevation::Popup,
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
        push_liquid_glass_panel_elevation(
            out,
            shape_cache,
            renderer,
            layout.close_panel_rect,
            13,
            GlassPanelStyle::Popover,
            state.glass_opacity,
            state.reduce_transparency,
            rusty_ui::shadow::ShadowElevation::Popup,
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
        && let Some(item) = state.notifications.get(lead.notif_index)
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
            push_liquid_glass_panel_elevation(
                out,
                shape_cache,
                renderer,
                clear_rect,
                12,
                GlassPanelStyle::Popover,
                state.glass_opacity,
                state.reduce_transparency,
                rusty_ui::shadow::ShadowElevation::Popup,
            );
        }
    }

    if let Some(empty_rect) = layout.empty_rect {
        push_liquid_glass_panel_elevation(
            out,
            shape_cache,
            renderer,
            empty_rect,
            NC_CARD_RADIUS,
            GlassPanelStyle::NotificationCard,
            state.glass_opacity,
            state.reduce_transparency,
            rusty_ui::shadow::ShadowElevation::Popup,
        );
    }

    for &slot in &layout.stack_cards {
        if let Some(item) = state.notifications.get(slot.notif_index) {
            push_notification_card_composite(out, shape_cache, renderer, slot, item, state, &pal);
        }
    }

    if state.show_weather_widget {
        push_weather_widget_composite(out, shape_cache, renderer, layout.weather_rect, pal.is_dark);
    }
    if state.show_reminders_widget {
        push_reminders_widget_composite(out, shape_cache, renderer, &layout, state, &pal);
    }
    push_clear_and_bottom_bars_composite(out, shape_cache, renderer, &layout, state, &pal);
}
