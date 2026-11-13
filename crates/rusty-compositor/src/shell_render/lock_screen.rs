//! macOS-style interactive Lock Screen layout, hit-testing, and Liquid Glass composition.

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::CompositorRenderElement;
use super::tokens::{
    anim_offset, make_solid_element, premul, push_liquid_glass_panel, push_rounded_glass_box,
    rect_contains, truncate_ascii,
};

/// Live data for the macOS-style interactive Lock Screen (`RUSTYOS-025`/`054`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct LockScreenRenderState<'a> {
    /// Lock Screen date header in neutral C locale (`"Thu Sep 10"`, `"Fri Nov 13"`).
    pub date_label: &'a str,
    /// Local wall-clock time without AM/PM suffix (`"8:52"` or `"14:52"`).
    pub clock_hhmm: &'a str,
    /// Active keyboard layout badge (`"EN"`, `"RU"`, etc.).
    pub keyboard_layout: &'a str,
    /// Whether hardware battery is present.
    pub has_battery: bool,
    /// Battery charge percentage (`0..=100`).
    pub battery_percent: u8,
    /// True when charging or connected to AC power.
    pub battery_charging: bool,
    /// True when Wi-Fi / network connection is active.
    pub wifi_connected: bool,
    /// Live Wi-Fi signal strength (`0..=100`).
    pub wifi_signal_percent: Option<u8>,
    /// Optional custom PNG wallpaper path matching active desktop background.
    pub wallpaper_path: Option<&'a std::path::Path>,
    /// Human user accounts rendered at bottom-center (`RUSTYOS-054`).
    pub users: &'a [rusty_services::UserAccount],
    /// Currently highlighted/selected account index in `users`.
    pub selected_user: usize,
    /// True while the top-right power actions popover is open.
    pub power_menu_open: bool,
    /// Require PAM password verification on Lock Screen (`RUSTYOS-056`).
    pub require_password: bool,
    /// Character length of typed secret (only dots `•` rendered, never plaintext) (`RUSTYOS-056`).
    pub password_len: usize,
    /// True while background `unix_chkpwd` verification is in flight (`RUSTYOS-056`).
    pub auth_checking: bool,
    /// Last PAM rejection or error status rendered below the password pill (`RUSTYOS-056`).
    pub auth_error: Option<&'a str>,
    /// User `[ui] glass_opacity` setting (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque mode (`reduce_transparency`).
    pub reduce_transparency: bool,
    /// Animated reveal progress (`0.0` unlocked -> `1.0` locked).
    pub progress: f64,
}

/// Interactive click targets on the macOS-style Lock Screen (`RUSTYOS-054`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockScreenHit {
    /// Unlock the desktop session as user `usize` (instant when password disabled).
    UnlockUser(usize),
    /// Submit the typed password via the pill submit button (`RUSTYOS-056`).
    SubmitPassword,
    /// Select another account `usize` in the multi-user stack.
    SelectUser(usize),
    /// Cycle active XKB keyboard layout (`U.S.` / `RU`).
    ToggleKeyboardLayout,
    /// Open or close the top-right power popover (`Sleep` / `Restart` / `Shut Down`).
    TogglePowerMenu,
    /// Dispatch a power lifecycle action from the power popover.
    PowerAction(rusty_services::SessionAction),
    /// Clicked outside an open power popover to close it.
    DismissPowerMenu,
    /// Non-interactive Lock Screen wallpaper/clock region.
    None,
}

pub(super) fn format_lock_kbd_label(layout: &str) -> &str {
    let trimmed = layout.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("EN")
        || trimmed.eq_ignore_ascii_case("US")
    {
        "U.S."
    } else {
        trimmed
    }
}

pub(super) fn default_lock_user_fallback() -> rusty_services::UserAccount {
    rusty_services::UserAccount {
        username: "user".to_owned(),
        display_name: "RustyOS User".to_owned(),
        uid: 1000,
        avatar_path: None,
    }
}

#[derive(Debug, Clone)]
pub(super) struct LockScreenLayout {
    pub(super) kbd_hit_rect: Rectangle<i32, smithay::utils::Physical>,
    pub(super) kbd_text_x: i32,
    pub(super) kbd_glyph_x: i32,
    pub(super) battery_x: Option<i32>,
    pub(super) wifi_x: i32,
    pub(super) power_btn_rect: Rectangle<i32, smithay::utils::Physical>,
    pub(super) power_menu_rect: Rectangle<i32, smithay::utils::Physical>,
    pub(super) power_item_rects: [Rectangle<i32, smithay::utils::Physical>; 3],
    pub(super) date_y: i32,
    pub(super) date_size: f32,
    pub(super) clock_y: i32,
    pub(super) clock_size: f32,
    pub(super) user_row_rects: Vec<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) avatar_x: i32,
    pub(super) name_x: i32,
    pub(super) unlock_hint_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) password_pill_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) password_submit_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) auth_status_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
}

pub(super) struct LockUserStackLayout {
    pub(super) user_row_rects: Vec<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) avatar_x: i32,
    pub(super) name_x: i32,
    pub(super) unlock_hint_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) password_pill_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) password_submit_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    pub(super) auth_status_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub(super) fn compute_lock_user_stack_rects(
    w: i32,
    h: i32,
    user_names: &[&str],
    slide_dy: i32,
    require_password: bool,
) -> LockUserStackLayout {
    let avatar_d = 38;
    let row_gap = 12;
    let step = avatar_d + row_gap;
    let count = user_names.len().max(1);
    let single_user = count == 1;
    let count_i32 = i32::try_from(count).unwrap_or(1);
    let users_h = count_i32 * avatar_d + (count_i32 - 1) * row_gap;
    let extra_h = if require_password {
        58
    } else if single_user {
        26
    } else {
        0
    };
    let stack_h = users_h + extra_h;
    let stack_top_y = (h - 68 - stack_h).min((f64::from(h) * 0.745).round() as i32) + slide_dy;

    let mut max_name_w = 24;
    for name in user_names {
        let nw = crate::text::measure_ui_text_width(name, 13.5, crate::text::FontWeight::Bold);
        max_name_w = max_name_w.max(nw.min(220));
    }
    let group_w = (avatar_d + 10 + max_name_w).clamp(64, 300);
    let avatar_x = (w - group_w) / 2;
    let name_x = avatar_x + avatar_d + 10;

    let mut user_row_rects = Vec::with_capacity(count);
    for idx in 0..count {
        let row_y = stack_top_y + i32::try_from(idx).unwrap_or(0) * step;
        user_row_rects.push(Rectangle::new(
            Point::from((avatar_x - 8, row_y - 4)),
            Size::from((group_w + 16, avatar_d + 8)),
        ));
    }

    let unlock_hint_rect = (!require_password && single_user).then(|| {
        let hint_w = 224.min(w - 32);
        Rectangle::new(
            Point::from(((w - hint_w) / 2, stack_top_y + users_h + 10)),
            Size::from((hint_w, 22)),
        )
    });

    let (password_pill_rect, password_submit_rect, auth_status_rect) = if require_password {
        let pill_w = 228.min(w - 32);
        let pill_h = 30;
        let pill_x = (w - pill_w) / 2;
        let pill_y = stack_top_y + users_h + 10;
        let status_w = 340.min(w - 32);
        (
            Some(Rectangle::new(
                Point::from((pill_x, pill_y)),
                Size::from((pill_w, pill_h)),
            )),
            Some(Rectangle::new(
                Point::from((pill_x + pill_w - 29, pill_y + 1)),
                Size::from((28, 28)),
            )),
            Some(Rectangle::new(
                Point::from(((w - status_w) / 2, pill_y + pill_h + 6)),
                Size::from((status_w, 18)),
            )),
        )
    } else {
        (None, None, None)
    };

    LockUserStackLayout {
        user_row_rects,
        avatar_x,
        name_x,
        unlock_hint_rect,
        password_pill_rect,
        password_submit_rect,
        auth_status_rect,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub(super) fn compute_lock_screen_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    keyboard_layout: &str,
    has_battery: bool,
    user_names: &[&str],
    slide_dy: i32,
    require_password: bool,
) -> LockScreenLayout {
    let w = screen_size.w.max(320);
    let h = screen_size.h.max(240);

    let pwr_w = 22;
    let pwr_x = w - 12 - pwr_w;
    let power_btn_rect = Rectangle::new(Point::from((pwr_x, 7)), Size::from((pwr_w, 22)));

    let wifi_w = 18;
    let wifi_x = pwr_x - 10 - wifi_w;
    let mut left_cursor = wifi_x;

    let battery_x = if has_battery {
        let bx = left_cursor - 11 - 24;
        left_cursor = bx;
        Some(bx)
    } else {
        None
    };

    let kbd_glyph_w = 18;
    let kbd_glyph_x = left_cursor - 12 - kbd_glyph_w;
    let kbd_label = format_lock_kbd_label(keyboard_layout);
    let kbd_text_w =
        crate::text::measure_ui_text_width(kbd_label, 12.0, crate::text::FontWeight::Bold)
            .clamp(18, 64);
    let kbd_text_x = kbd_glyph_x - 5 - kbd_text_w;
    let kbd_hit_rect = Rectangle::new(
        Point::from((kbd_text_x - 6, 6)),
        Size::from((kbd_text_w + 5 + kbd_glyph_w + 10, 24)),
    );

    let pm_w = 148;
    let pm_h = 104;
    let pm_x = (w - 12 - pm_w).max(8);
    let pm_y = 34;
    let power_menu_rect = Rectangle::new(Point::from((pm_x, pm_y)), Size::from((pm_w, pm_h)));
    let power_item_rects = [
        Rectangle::new(
            Point::from((pm_x + 6, pm_y + 7)),
            Size::from((pm_w - 12, 30)),
        ),
        Rectangle::new(
            Point::from((pm_x + 6, pm_y + 37)),
            Size::from((pm_w - 12, 30)),
        ),
        Rectangle::new(
            Point::from((pm_x + 6, pm_y + 67)),
            Size::from((pm_w - 12, 30)),
        ),
    ];

    let date_size = (f64::from(h) * 0.025).clamp(16.0, 26.0) as f32;
    let clock_size = (f64::from(h) * 0.136).clamp(66.0, 132.0) as f32;
    let date_y = (f64::from(h) * 0.088).round() as i32 + slide_dy;
    let clock_y = date_y + (date_size * 0.92).round() as i32;
    let stack = compute_lock_user_stack_rects(w, h, user_names, slide_dy, require_password);

    LockScreenLayout {
        kbd_hit_rect,
        kbd_text_x,
        kbd_glyph_x,
        battery_x,
        wifi_x,
        power_btn_rect,
        power_menu_rect,
        power_item_rects,
        date_y,
        date_size,
        clock_y,
        clock_size,
        user_row_rects: stack.user_row_rects,
        avatar_x: stack.avatar_x,
        name_x: stack.name_x,
        unlock_hint_rect: stack.unlock_hint_rect,
        password_pill_rect: stack.password_pill_rect,
        password_submit_rect: stack.password_submit_rect,
        auth_status_rect: stack.auth_status_rect,
    }
}

/// Hit-tests `(x, y)` on the interactive macOS-style Lock Screen (`RUSTYOS-054`).
#[must_use]
pub fn hit_test_lock_screen(
    screen_size: Size<i32, smithay::utils::Physical>,
    lock: &LockScreenRenderState<'_>,
    x: f64,
    y: f64,
) -> LockScreenHit {
    let fallback_user = default_lock_user_fallback();
    let users = if lock.users.is_empty() {
        std::slice::from_ref(&fallback_user)
    } else {
        lock.users
    };
    let names: Vec<&str> = users.iter().map(|u| u.display_name.as_str()).collect();
    let layout = compute_lock_screen_layout(
        screen_size,
        lock.keyboard_layout,
        lock.has_battery,
        &names,
        0,
        lock.require_password,
    );

    if rect_contains(layout.power_btn_rect, x, y) {
        return LockScreenHit::TogglePowerMenu;
    }

    if lock.power_menu_open {
        let actions = [
            rusty_services::SessionAction::Suspend,
            rusty_services::SessionAction::Reboot,
            rusty_services::SessionAction::PowerOff,
        ];
        for (rect, action) in layout.power_item_rects.into_iter().zip(actions) {
            if rect_contains(rect, x, y) {
                return LockScreenHit::PowerAction(action);
            }
        }
        if rect_contains(layout.power_menu_rect, x, y) {
            return LockScreenHit::None;
        }
        return LockScreenHit::DismissPowerMenu;
    }

    if rect_contains(layout.kbd_hit_rect, x, y) {
        return LockScreenHit::ToggleKeyboardLayout;
    }

    if let Some(submit_rect) = layout.password_submit_rect
        && rect_contains(submit_rect, x, y)
    {
        return LockScreenHit::SubmitPassword;
    }

    if let Some(pill_rect) = layout.password_pill_rect
        && rect_contains(pill_rect, x, y)
    {
        return LockScreenHit::None;
    }

    let selected = lock.selected_user.min(users.len().saturating_sub(1));
    for (idx, row_rect) in layout.user_row_rects.iter().enumerate() {
        if rect_contains(*row_rect, x, y) {
            return if users.len() <= 1 || idx == selected {
                LockScreenHit::UnlockUser(idx)
            } else {
                LockScreenHit::SelectUser(idx)
            };
        }
    }

    if let Some(hint_rect) = layout.unlock_hint_rect
        && rect_contains(hint_rect, x, y)
    {
        return LockScreenHit::UnlockUser(selected);
    }

    LockScreenHit::None
}

/// Hit-tests `(x, y)` against the primary Lock Screen unlock row/hint (`RUSTYOS-025`/`054`).
#[must_use]
pub fn check_lock_screen_unlock_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> bool {
    let default_state = LockScreenRenderState {
        date_label: "Thu Sep 10",
        clock_hhmm: "8:52",
        keyboard_layout: "EN",
        has_battery: true,
        battery_percent: 100,
        battery_charging: true,
        wifi_connected: true,
        wifi_signal_percent: Some(90),
        wallpaper_path: None,
        users: &[],
        selected_user: 0,
        power_menu_open: false,
        require_password: false,
        password_len: 0,
        auth_checking: false,
        auth_error: None,
        glass_opacity: 0.78,
        reduce_transparency: false,
        progress: 1.0,
    };
    matches!(
        hit_test_lock_screen(screen_size, &default_state, x, y),
        LockScreenHit::UnlockUser(_)
    )
}

#[cfg(test)]
#[must_use]
pub(crate) fn lock_screen_primary_row_center(
    screen_size: Size<i32, smithay::utils::Physical>,
    user_names: &[&str],
) -> (f64, f64) {
    let layout = compute_lock_screen_layout(screen_size, "EN", true, user_names, 0, false);
    let row = layout.user_row_rects[0];
    (
        f64::from(row.loc.x + row.size.w / 2),
        f64::from(row.loc.y + row.size.h / 2),
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn render_lock_power_menu(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    if !lock.power_menu_open || fade <= 0.25 {
        return;
    }
    let text_alpha = (244.0 * fade).clamp(0.0, 255.0).round() as u8;
    let text_rgba = [246, 248, 252, text_alpha];
    let labels = ["Sleep", "Restart...", "Shut Down..."];
    for (idx, (rect, label)) in layout.power_item_rects.iter().zip(labels).enumerate() {
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            label,
            crate::text::FontWeight::Regular,
            12.5,
            text_rgba,
            Point::from((rect.loc.x + 12, rect.loc.y + 7)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if idx < 2 {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(
                    Point::from((rect.loc.x + 8, rect.loc.y + rect.size.h - 1)),
                    Size::from((rect.size.w - 16, 1)),
                ),
                premul(1.0, 1.0, 1.0, 0.14 * fade),
            )));
        }
    }
    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        layout.power_menu_rect,
        12,
        crate::surfaces::GlassPanelStyle::Popover,
        lock.glass_opacity,
        lock.reduce_transparency,
        Some((14, (44.0 * fade).round() as u8)),
    );
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn render_lock_top_right_status(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    if fade <= 0.15 {
        return;
    }
    let white_a = (242.0 * fade).clamp(0.0, 255.0).round() as u8;
    let shadow_a = (72.0 * fade).clamp(0.0, 255.0).round() as u8;
    let white = [255, 255, 255, white_a];
    let kbd_label = format_lock_kbd_label(lock.keyboard_layout);

    if text_cache.has_font() {
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            kbd_label,
            crate::text::FontWeight::Bold,
            12.0,
            white,
            Point::from((layout.kbd_text_x, 10)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            kbd_label,
            crate::text::FontWeight::Bold,
            12.0,
            [0, 0, 0, shadow_a],
            Point::from((layout.kbd_text_x, 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    if let Some(el) = shape_cache.ui_glyph_rect_element(
        renderer,
        crate::surfaces::UiGlyphKind::Keyboard,
        18,
        14,
        white,
        Point::from((layout.kbd_glyph_x, 11)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(bx) = layout.battery_x
        && let Some(el) = shape_cache.ui_glyph_rect_element(
            renderer,
            crate::surfaces::UiGlyphKind::Battery {
                percent: lock.battery_percent,
                charging: lock.battery_charging,
            },
            24,
            18,
            white,
            Point::from((bx, 9)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Wifi {
            connected: lock.wifi_connected,
            signal_percent: lock.wifi_signal_percent,
        },
        18,
        white,
        Point::from((layout.wifi_x, 9)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::PowerSymbol,
        16,
        white,
        Point::from((
            layout.power_btn_rect.loc.x + 3,
            layout.power_btn_rect.loc.y + 3,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if lock.power_menu_open
        && let Some(el) = shape_cache.circle_element(
            renderer,
            layout.power_btn_rect.size.w,
            [255, 255, 255, (56.0 * fade).round() as u8],
            Some([255, 255, 255, (92.0 * fade).round() as u8]),
            Point::from((layout.power_btn_rect.loc.x, layout.power_btn_rect.loc.y)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn render_lock_clock_header(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_width: i32,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    if fade <= 0.15 || !text_cache.has_font() {
        return;
    }
    let date_bounds = Rectangle::new(
        Point::from((0, layout.date_y)),
        Size::from((screen_width, (layout.date_size * 1.4).round() as i32)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        lock.date_label,
        crate::text::FontWeight::Bold,
        layout.date_size,
        [248, 249, 252, (238.0 * fade).round() as u8],
        date_bounds,
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let date_shadow_bounds = Rectangle::new(Point::from((0, layout.date_y + 1)), date_bounds.size);
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        lock.date_label,
        crate::text::FontWeight::Bold,
        layout.date_size,
        [0, 0, 0, (48.0 * fade).round() as u8],
        date_shadow_bounds,
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    let clock_bounds = Rectangle::new(
        Point::from((0, layout.clock_y)),
        Size::from((screen_width, (layout.clock_size * 1.15).round() as i32)),
    );
    // macOS Liquid Glass lock clock: clean translucent numerals letting wallpaper glow through.
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        lock.clock_hhmm,
        crate::text::FontWeight::Bold,
        layout.clock_size,
        [255, 255, 255, (186.0 * fade).round() as u8],
        clock_bounds,
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn render_lock_users_section(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    if fade <= 0.18 {
        return;
    }
    let fallback_user = default_lock_user_fallback();
    let users = if lock.users.is_empty() {
        std::slice::from_ref(&fallback_user)
    } else {
        lock.users
    };
    let selected = lock.selected_user.min(users.len().saturating_sub(1));
    let multi_user = users.len() > 1;
    let name_alpha = (246.0 * fade).round() as u8;
    let shadow_alpha = (64.0 * fade).round() as u8;

    for (idx, (user, row_rect)) in users.iter().zip(&layout.user_row_rects).enumerate() {
        let row_y = row_rect.loc.y + 4;
        let is_selected = idx == selected;
        if text_cache.has_font() {
            let text_rgba = if is_selected || !multi_user {
                [255, 255, 255, name_alpha]
            } else {
                [232, 236, 242, (206.0 * fade).round() as u8]
            };
            if let Some(el) = text_cache.render_label_weighted(
                renderer,
                &user.display_name,
                crate::text::FontWeight::Bold,
                13.5,
                text_rgba,
                Point::from((layout.name_x, row_y + 10)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
            if let Some(el) = text_cache.render_label_weighted(
                renderer,
                &user.display_name,
                crate::text::FontWeight::Bold,
                13.5,
                [0, 0, 0, shadow_alpha],
                Point::from((layout.name_x, row_y + 11)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
        }
        if let Some(el) = shape_cache.user_avatar_element(
            renderer,
            38,
            user.avatar_path.as_deref().map(std::path::Path::new),
            user.uid,
            is_selected,
            Point::from((layout.avatar_x, row_y)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    if let Some(hint_rect) = layout.unlock_hint_rect
        && text_cache.has_font()
    {
        let hint_logical = Rectangle::new(
            Point::from((hint_rect.loc.x, hint_rect.loc.y)),
            Size::from((hint_rect.size.w, hint_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            "Click or press Return to unlock",
            crate::text::FontWeight::Regular,
            11.0,
            [244, 247, 252, (170.0 * fade).round() as u8],
            hint_logical,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let shadow_logical = Rectangle::new(
            Point::from((hint_rect.loc.x, hint_rect.loc.y + 1)),
            hint_logical.size,
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            "Click or press Return to unlock",
            crate::text::FontWeight::Regular,
            11.0,
            [0, 0, 0, shadow_alpha],
            shadow_logical,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    if lock.require_password {
        render_lock_password_pill(out, text_cache, shape_cache, renderer, layout, lock, fade);
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn render_lock_password_pill(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    let Some(pill_rect) = layout.password_pill_rect else {
        return;
    };
    let dot_count = lock.password_len.min(36);
    let dots = "•".repeat(dot_count);
    if text_cache.has_font() {
        let (pill_label, pill_color) = if lock.auth_checking {
            (
                "Verifying...",
                [232, 236, 252, (215.0 * fade).round() as u8],
            )
        } else if dot_count == 0 {
            (
                "Enter Password",
                [240, 244, 252, (165.0 * fade).round() as u8],
            )
        } else {
            (dots.as_str(), [255, 255, 255, (245.0 * fade).round() as u8])
        };
        let text_bounds = Rectangle::new(
            Point::from((pill_rect.loc.x + 12, pill_rect.loc.y)),
            Size::from(((pill_rect.size.w - 42).max(24), pill_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            pill_label,
            crate::text::FontWeight::Regular,
            12.5,
            pill_color,
            text_bounds,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    if let Some(submit_rect) = layout.password_submit_rect {
        if let Some(arrow) = shape_cache.ui_glyph_element(
            renderer,
            crate::surfaces::UiGlyphKind::ArrowNext,
            12,
            [250, 250, 255, (240.0 * fade).round() as u8],
            Point::from((submit_rect.loc.x + 8, submit_rect.loc.y + 8)),
        ) {
            out.push(CompositorRenderElement::Memory(arrow));
        }
        let btn_alpha = if dot_count > 0 { 68.0 } else { 36.0 };
        if let Some(circle) = shape_cache.circle_element(
            renderer,
            22,
            [255, 255, 255, (btn_alpha * fade).round() as u8],
            Some([255, 255, 255, (92.0 * fade).round() as u8]),
            Point::from((submit_rect.loc.x + 3, submit_rect.loc.y + 3)),
        ) {
            out.push(CompositorRenderElement::Memory(circle));
        }
    }

    let border_rgba = if lock.auth_error.is_some() {
        [255, 132, 138, (195.0 * fade).round() as u8]
    } else {
        [255, 255, 255, (112.0 * fade).round() as u8]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        pill_rect,
        15,
        [255, 255, 255, (42.0 * fade).round() as u8],
        Some(border_rgba),
    );

    render_lock_auth_status_label(out, text_cache, renderer, layout, lock, fade);
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn render_lock_auth_status_label(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    let shadow_alpha = (64.0 * fade).round() as u8;
    if let Some(status_rect) = layout.auth_status_rect
        && text_cache.has_font()
    {
        let status_owned;
        let (status_label, status_color) = if lock.auth_checking {
            (
                "Checking credentials...",
                [236, 240, 252, (205.0 * fade).round() as u8],
            )
        } else if let Some(err) = lock.auth_error {
            status_owned = truncate_ascii(err, 48);
            (
                status_owned.as_str(),
                [255, 152, 152, (245.0 * fade).round() as u8],
            )
        } else {
            (
                "Enter password and press Return",
                [244, 247, 252, (176.0 * fade).round() as u8],
            )
        };
        let status_bounds = Rectangle::new(
            Point::from((status_rect.loc.x, status_rect.loc.y)),
            Size::from((status_rect.size.w, status_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            status_label,
            crate::text::FontWeight::Regular,
            11.0,
            status_color,
            status_bounds,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let shadow_bounds = Rectangle::new(
            Point::from((status_rect.loc.x, status_rect.loc.y + 1)),
            Size::from((status_rect.size.w, status_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            status_label,
            crate::text::FontWeight::Regular,
            11.0,
            [0, 0, 0, shadow_alpha],
            shadow_bounds,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
}

/// Generates topmost front-to-back macOS-style Lock Screen backdrop, frosted clock,
/// minimal top-right controls, and bottom user selector (`RUSTYOS-025`/`054`).
#[allow(clippy::cast_possible_truncation)]
pub fn generate_lock_screen_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    lock: &LockScreenRenderState<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let p = lock.progress.clamp(0.0, 1.0);
    if p <= 0.01 {
        return out;
    }

    let fallback_user = default_lock_user_fallback();
    let users = if lock.users.is_empty() {
        std::slice::from_ref(&fallback_user)
    } else {
        lock.users
    };
    let names: Vec<&str> = users.iter().map(|u| u.display_name.as_str()).collect();
    let slide_dy = anim_offset(p, 16.0);
    let layout = compute_lock_screen_layout(
        screen_size,
        lock.keyboard_layout,
        lock.has_battery,
        &names,
        slide_dy,
        lock.require_password,
    );
    let fade = p as f32;

    render_lock_power_menu(
        &mut out,
        text_cache,
        shape_cache,
        renderer,
        &layout,
        lock,
        fade,
    );
    render_lock_top_right_status(
        &mut out,
        text_cache,
        shape_cache,
        renderer,
        &layout,
        lock,
        fade,
    );
    render_lock_clock_header(
        &mut out,
        text_cache,
        renderer,
        screen_size.w,
        &layout,
        lock,
        fade,
    );
    render_lock_users_section(
        &mut out,
        text_cache,
        shape_cache,
        renderer,
        &layout,
        lock,
        fade,
    );

    if let Some(bg) = shape_cache.wallpaper_element_with_alpha(
        renderer,
        Size::from((screen_size.w, screen_size.h)),
        lock.wallpaper_path,
        fade,
    ) {
        out.push(CompositorRenderElement::Memory(bg));
    } else {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, 0)), screen_size),
            premul(0.08, 0.09, 0.13, fade),
        )));
    }

    out
}
