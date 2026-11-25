//! `rusty-app` host integration and input dispatch for `rusty-settings` (`RUSTYOS-103`).

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use rusty_app::{
    AppConfig, AppContext, AppError, BTN_LEFT, ConfigureInfo, GlassRegion, KeyEvent, Keysym,
    LiquidGlassConfig, Modifiers, PointerEvent, PointerEventKind, ResizeEdge, RustyApp,
    WindowDecorationMode, run_app,
};
use smithay_client_toolkit::seat::pointer::AxisScroll;
use wayland_client::protocol::wl_pointer;

use crate::layout::{
    HEADER_HEIGHT, MIN_HEIGHT, MIN_WIDTH, ResizeEdgeZone, SIDEBAR_WIDTH, SettingsHit,
    SettingsLayout, WINDOW_CORNER_RADIUS, compute_layout, hit_test,
};
use crate::render::{render_window_pixels_with_glass, with_font_engine};
use crate::state::{
    DamageScope, ServiceCommand, ServiceSnapshot, SettingsAppState, SettingsPage, StartupOptions,
    execute_coalesced_service_batch,
};

const DEFAULT_WIDTH: u32 = 882;
const DEFAULT_HEIGHT: u32 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepperDragKind {
    Volume,
    Scale,
    GlassOpacity,
}

struct SettingsApp {
    app: SettingsAppState,
    clipboard: rusty_ui::MemoryClipboard,
    service_tx: Sender<ServiceCommand>,
    service_rx: Receiver<ServiceSnapshot>,
    width: u32,
    height: u32,
    pointer_pos: (f64, f64),
    pointer_pressed: bool,
    scrollbar_drag: Option<(f64, i32)>,
    stepper_drag: Option<(StepperDragKind, f64, i32)>,
    click_streak: u8,
    last_click_at: Option<Instant>,
    hover_hit: Option<SettingsHit>,
    layout_cache: Option<SettingsLayout>,
}

impl SettingsApp {
    fn new(opts: &StartupOptions) -> Self {
        let mut app = SettingsAppState::new(opts);
        let (service_tx, service_rx) = spawn_service_worker();
        let init_layout = compute_layout(&app, DEFAULT_WIDTH, DEFAULT_HEIGHT);
        app.sync_toggles_from_layout(&init_layout);
        Self {
            app,
            clipboard: rusty_ui::MemoryClipboard::default(),
            service_tx,
            service_rx,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            pointer_pos: (0.0, 0.0),
            pointer_pressed: false,
            scrollbar_drag: None,
            stepper_drag: None,
            click_streak: 0,
            last_click_at: None,
            hover_hit: None,
            layout_cache: Some(init_layout),
        }
    }

    fn current_layout(&mut self) -> SettingsLayout {
        if let Some(cached) = &self.layout_cache {
            return cached.clone();
        }
        let l = compute_layout(&self.app, self.width, self.height);
        self.layout_cache = Some(l.clone());
        l
    }

    fn flush_pending_service_commands(&mut self) {
        for cmd in self.app.take_pending_service_commands() {
            let _ = self.service_tx.send(cmd);
        }
    }

    fn sync_glass(&self, ctx: &mut AppContext<'_>) {
        if self.app.reduce_transparency {
            ctx.set_liquid_glass(None);
        } else {
            ctx.set_liquid_glass(Some(sidebar_glass_config(self.height)));
        }
    }

    fn sync_dirty(&mut self, ctx: &mut AppContext<'_>) {
        if !self.app.running {
            ctx.request_exit();
            return;
        }
        self.sync_glass(ctx);
        if self.app.dirty {
            ctx.request_redraw();
        }
    }

    fn handle_key_press(&mut self, ctx: &mut AppContext<'_>, event: &KeyEvent) {
        let mods = ctx.modifiers();
        if mods.ctrl && matches!(event.keysym, Keysym::q | Keysym::Q) {
            self.app.request_quit();
            ctx.request_exit();
            return;
        }
        if self.app.active_modal.is_some() {
            if matches!(
                self.app.active_modal,
                Some(crate::state::SettingsModal::ShortcutEdit { .. })
            ) && let Some(combo) = format_captured_shortcut(mods, event.keysym)
            {
                if self.app.apply_captured_shortcut(&combo) {
                    self.layout_cache = None;
                }
                return;
            }
            if let Some(ui_key) = map_keysym_to_ui(event.keysym, event.utf8.as_deref()) {
                let ui_mods = rusty_ui::InputModifiers {
                    ctrl: mods.ctrl,
                    shift: mods.shift,
                    alt: mods.alt,
                };
                if with_font_engine(|fe| {
                    self.app
                        .handle_modal_text_key(ui_key, ui_mods, fe, &mut self.clipboard)
                }) {
                    self.layout_cache = None;
                    self.flush_pending_service_commands();
                }
            }
            return;
        }
        if self.app.current_page == SettingsPage::Battery && self.app.low_power_dropdown.open {
            let ctrl_key = match event.keysym {
                Keysym::Up => Some(rusty_ui::ControlKey::Up),
                Keysym::Down => Some(rusty_ui::ControlKey::Down),
                Keysym::Home => Some(rusty_ui::ControlKey::Home),
                Keysym::End => Some(rusty_ui::ControlKey::End),
                Keysym::Return | Keysym::KP_Enter => Some(rusty_ui::ControlKey::Enter),
                Keysym::Escape => Some(rusty_ui::ControlKey::Escape),
                _ => None,
            };
            if let Some(ck) = ctrl_key {
                if let Some(idx) = self.app.low_power_dropdown.handle_key(ck) {
                    self.app.select_low_power_mode(idx);
                    self.flush_pending_service_commands();
                } else {
                    self.app.mark_dirty(DamageScope::Page);
                }
                self.layout_cache = None;
                return;
            }
        }
        if mods.ctrl && matches!(event.keysym, Keysym::f | Keysym::F) {
            self.app.set_search_focused(true);
            return;
        }
        if self.app.search_focused
            && let Some(ui_key) = map_keysym_to_ui(event.keysym, event.utf8.as_deref())
        {
            let ui_mods = rusty_ui::InputModifiers {
                ctrl: mods.ctrl,
                shift: mods.shift,
                alt: mods.alt,
            };
            let handled = with_font_engine(|fe| {
                self.app
                    .handle_search_text_key(ui_key, ui_mods, fe, &mut self.clipboard)
            });
            if handled {
                self.layout_cache = None;
                self.flush_pending_service_commands();
                return;
            }
        }
        if !self.app.search_focused {
            let layout = self.current_layout();
            let page_step = (layout.page_viewport.h - 40).max(120);
            let delta = match event.keysym {
                Keysym::Up => -44,
                Keysym::Down => 44,
                Keysym::Page_Up => -page_step,
                Keysym::Page_Down => page_step,
                Keysym::Home => -layout.max_page_scroll,
                Keysym::End => layout.max_page_scroll,
                _ => 0,
            };
            if delta != 0
                && self.app.scroll_smooth_by(
                    false,
                    f64::from(delta),
                    layout.max_sidebar_scroll,
                    layout.max_page_scroll,
                    true,
                )
            {
                self.layout_cache = None;
            }
        }
    }

    fn execute_left_click(&mut self, ctx: &mut AppContext<'_>, px: f64, py: f64, serial: u32) {
        let now = Instant::now();
        let multi = self
            .last_click_at
            .is_some_and(|t| now.saturating_duration_since(t) <= Duration::from_millis(380));
        self.click_streak = if multi {
            self.click_streak.saturating_add(1).min(3)
        } else {
            1
        };
        let double_click = self.click_streak == 2;
        self.last_click_at = Some(now);

        let layout = self.current_layout();
        if self.app.active_modal.is_some() {
            self.handle_modal_left_press(px, py);
            return;
        }
        if self.app.current_page == SettingsPage::Battery
            && self.app.low_power_dropdown.open
            && let Some(dd_rect) = layout.low_power_dropdown_rect
        {
            self.app.low_power_dropdown.rect = rusty_ui::Rect::new(
                dd_rect.x,
                dd_rect.y,
                u32::try_from(dd_rect.w.max(1)).unwrap_or(1),
                u32::try_from(dd_rect.h.max(1)).unwrap_or(1),
            );
            let picked = self.app.low_power_dropdown.on_pointer_down(px, py);
            if let Some(idx) = picked {
                self.app.select_low_power_mode(idx);
                self.flush_pending_service_commands();
                self.layout_cache = None;
                return;
            }
            if self.app.low_power_dropdown.open {
                self.app.mark_dirty(DamageScope::Page);
                return;
            }
            self.app.mark_dirty(DamageScope::Page);
            self.layout_cache = None;
            if dd_rect.contains_f64(px, py) {
                return;
            }
        }
        let hit = hit_test(&layout, px, py);
        if !matches!(hit, Some(SettingsHit::SearchField)) && self.app.search_focused {
            self.app.set_search_focused(false);
        }
        if hit.is_none() && self.handle_scrollbar_track_jump(&layout, px, py) {
            return;
        }
        let Some(hit) = hit else {
            return;
        };
        self.dispatch_hit(ctx, hit, &layout, (px, py), serial, double_click);
        self.flush_pending_service_commands();
    }

    #[allow(clippy::cast_possible_truncation)]
    fn handle_scrollbar_track_jump(&mut self, layout: &SettingsLayout, px: f64, py: f64) -> bool {
        if layout.max_page_scroll <= 0
            || px < f64::from(layout.win_w - 11)
            || py < f64::from(HEADER_HEIGHT)
        {
            return false;
        }
        let thumb_h = layout.page_scrollbar_thumb.map_or(28, |t| t.h);
        let track_top = f64::from(HEADER_HEIGHT + 6);
        let travel = f64::from((layout.win_h - HEADER_HEIGHT - 14 - thumb_h).max(1));
        let rel = ((py - track_top - f64::from(thumb_h) * 0.5) / travel).clamp(0.0, 1.0);
        let target_y = (rel * f64::from(layout.max_page_scroll)).round() as i32;
        self.app
            .set_page_scroll_immediate(target_y, layout.max_page_scroll);
        self.layout_cache = None;
        true
    }

    #[allow(clippy::cast_possible_truncation)]
    fn update_scrollbar_drag(&mut self, py: f64) -> bool {
        let Some((start_py, start_scroll)) = self.scrollbar_drag else {
            return false;
        };
        let layout = self.current_layout();
        let thumb_h = layout.page_scrollbar_thumb.map_or(28, |t| t.h);
        let travel = (layout.win_h - HEADER_HEIGHT - 14 - thumb_h).max(1);
        let delta_scroll = ((py - start_py) * f64::from(layout.max_page_scroll) / f64::from(travel))
            .round() as i32;
        self.app
            .set_page_scroll_immediate(start_scroll + delta_scroll, layout.max_page_scroll);
        self.layout_cache = None;
        true
    }

    #[allow(clippy::cast_possible_truncation)]
    fn update_stepper_drag(&mut self, px: f64) -> bool {
        let Some((kind, start_x, start_val)) = self.stepper_drag else {
            return false;
        };
        let dx = px - start_x;
        if dx.abs() < 4.0 {
            return false;
        }
        match kind {
            StepperDragKind::Volume => {
                let next = (start_val + (dx / 2.0).round() as i32).clamp(0, 100);
                self.app.set_volume_percent(u8::try_from(next).unwrap_or(0));
            }
            StepperDragKind::Scale => {
                let steps = (dx / 28.0).round() as i32;
                let next_pct = (start_val + steps * 25).clamp(100, 300);
                self.app.set_display_scale(f64::from(next_pct) / 100.0);
            }
            StepperDragKind::GlassOpacity => {
                let next = (start_val + (dx / 2.0).round() as i32).clamp(35, 100);
                self.app.set_glass_opacity(u8::try_from(next).unwrap_or(80));
            }
        }
        self.layout_cache = None;
        self.flush_pending_service_commands();
        true
    }

    fn dispatch_hit(
        &mut self,
        ctx: &mut AppContext<'_>,
        hit: SettingsHit,
        layout: &SettingsLayout,
        pos: (f64, f64),
        serial: u32,
        double_click: bool,
    ) {
        let (px, py) = pos;
        match hit {
            SettingsHit::ResizeEdge(zone) => {
                ctx.start_interactive_resize(map_resize_edge(zone), serial);
            }
            SettingsHit::WindowDrag => {
                if double_click {
                    ctx.set_maximized(!self.app.maximized);
                } else {
                    ctx.start_interactive_move(serial);
                }
            }
            SettingsHit::TrafficClose => self.app.request_quit(),
            SettingsHit::TrafficMinimize => ctx.set_minimized(),
            SettingsHit::TrafficMaximize => ctx.set_maximized(!self.app.maximized),
            other => self.dispatch_content_hit(other, layout, px, py),
        }
    }

    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    fn dispatch_content_hit(
        &mut self,
        hit: SettingsHit,
        layout: &SettingsLayout,
        px: f64,
        py: f64,
    ) {
        match hit {
            SettingsHit::NavBack => {
                self.app.go_back();
                self.layout_cache = None;
            }
            SettingsHit::NavForward => {
                self.app.go_forward();
                self.layout_cache = None;
            }
            SettingsHit::SearchField => {
                self.app.set_search_focused(true);
                let sr = layout.search_rect;
                self.app.search_field.rect = rusty_ui::Rect::new(
                    sr.x,
                    sr.y,
                    u32::try_from(sr.w.max(1)).unwrap_or(1),
                    u32::try_from(sr.h.max(1)).unwrap_or(1),
                );
                let click_n = self.click_streak;
                let elapsed = self.app.epoch.elapsed();
                let hit = with_font_engine(|fe| {
                    self.app
                        .search_field
                        .on_pointer_down(px, py, click_n, elapsed, fe)
                });
                if matches!(hit, Some(rusty_ui::TextFieldHit::ClearButton)) {
                    self.app.search_query.clear();
                    self.app.reset_sidebar_scroll();
                    self.layout_cache = None;
                    self.app.mark_dirty(DamageScope::Full);
                }
            }
            SettingsHit::ScrollbarThumb => {
                self.scrollbar_drag = Some((py, self.app.page_scroll_y));
            }
            SettingsHit::SidebarItem(page) | SettingsHit::PushPage(page) => {
                self.app.navigate_to(page);
                self.layout_cache = None;
            }
            SettingsHit::SelectTheme(choice) => {
                self.app.choose_appearance(choice);
                self.layout_cache = None;
            }
            SettingsHit::SelectAccent(idx) => {
                self.app.choose_accent(idx);
                self.layout_cache = None;
            }
            SettingsHit::DockPosTile(idx) => {
                self.app
                    .apply_row_action(crate::state::RowControlId::DockPositionSet(idx));
                self.layout_cache = None;
            }
            SettingsHit::BatteryTab(idx) => {
                self.app.select_battery_tab(idx);
                self.layout_cache = None;
            }
            SettingsHit::LowPowerDropdownTrigger => {
                if let Some(dd_rect) = layout.low_power_dropdown_rect {
                    self.app.low_power_dropdown.rect = rusty_ui::Rect::new(
                        dd_rect.x,
                        dd_rect.y,
                        u32::try_from(dd_rect.w.max(1)).unwrap_or(1),
                        u32::try_from(dd_rect.h.max(1)).unwrap_or(1),
                    );
                }
                let _ = self.app.low_power_dropdown.on_pointer_down(px, py);
                self.app.mark_dirty(DamageScope::Page);
            }
            SettingsHit::Control(id)
            | SettingsHit::StepperMinus(id)
            | SettingsHit::StepperPlus(id) => {
                self.stepper_drag = match id {
                    crate::state::RowControlId::VolumeDown
                    | crate::state::RowControlId::VolumeUp => Some((
                        StepperDragKind::Volume,
                        px,
                        i32::from(self.app.volume_percent),
                    )),
                    crate::state::RowControlId::ScaleDown | crate::state::RowControlId::ScaleUp => {
                        Some((
                            StepperDragKind::Scale,
                            px,
                            (self.app.display_scale * 100.0).round() as i32,
                        ))
                    }
                    crate::state::RowControlId::GlassOpacityDown
                    | crate::state::RowControlId::GlassOpacityUp => Some((
                        StepperDragKind::GlassOpacity,
                        px,
                        (self.app.glass_opacity * 100.0).round() as i32,
                    )),
                    _ => None,
                };
                self.app.apply_row_action(id);
                self.layout_cache = None;
                let next_layout = self.current_layout();
                self.app.sync_toggles_from_layout(&next_layout);
            }
            SettingsHit::ResizeEdge(_)
            | SettingsHit::WindowDrag
            | SettingsHit::TrafficClose
            | SettingsHit::TrafficMinimize
            | SettingsHit::TrafficMaximize => {}
        }
    }

    fn handle_modal_left_press(&mut self, px: f64, py: f64) {
        let w = self.width.max(1);
        let h = self.height.max(1);
        let streak = self.click_streak;
        let elapsed = self.app.epoch.elapsed();
        if let Some(modal) = &mut self.app.active_modal {
            if let Some(cb) = modal.wifi_show_password_checkbox(w, h)
                && cb.hit_rect.contains_f64(px, py)
            {
                self.app.toggle_wifi_password_visibility();
                return;
            }
            let sheet = modal.sheet_mut();
            let layout = sheet.sync_input_rect(w, h);
            if let (Some(tf), Some(ir)) = (&mut sheet.input, layout.input_rect)
                && ir.contains_f64(px, py)
            {
                let _ = with_font_engine(|fe| tf.on_pointer_down(px, py, streak, elapsed, fe));
            } else {
                let _ = sheet.on_pointer_down(px, py, w, h);
            }
            self.app.mark_dirty(DamageScope::Full);
        }
    }

    fn handle_modal_pointer_motion(&mut self, px: f64, py: f64) -> bool {
        if self.app.active_modal.is_none() {
            return false;
        }
        let w = self.width.max(1);
        let h = self.height.max(1);
        let pressed = self.pointer_pressed;
        if let Some(modal) = &mut self.app.active_modal {
            let sheet = modal.sheet_mut();
            let mut changed = sheet.on_pointer_move(px, py, w, h);
            if pressed && let Some(tf) = &mut sheet.input {
                changed |= with_font_engine(|fe| tf.on_pointer_move(px, fe));
            }
            if changed {
                self.app.mark_dirty(DamageScope::Full);
            }
        }
        true
    }

    fn handle_page_pointer_motion(&mut self, px: f64, py: f64) {
        if self.pointer_pressed && self.app.search_focused {
            let changed = with_font_engine(|fe| self.app.search_field.on_pointer_move(px, fe));
            if changed {
                self.app.mark_dirty(DamageScope::Sidebar);
            }
        }
        let layout = self.current_layout();
        if self.app.current_page == SettingsPage::Battery
            && self.app.low_power_dropdown.open
            && let Some(dd_rect) = layout.low_power_dropdown_rect
        {
            self.app.low_power_dropdown.rect = rusty_ui::Rect::new(
                dd_rect.x,
                dd_rect.y,
                u32::try_from(dd_rect.w.max(1)).unwrap_or(1),
                u32::try_from(dd_rect.h.max(1)).unwrap_or(1),
            );
            if self.app.low_power_dropdown.on_pointer_move(px, py) {
                self.app.mark_dirty(DamageScope::Page);
            }
        }
        let next = hit_test(&layout, px, py);
        if next != self.hover_hit {
            let scope =
                hit_damage_scope(self.hover_hit.as_ref()).merge(hit_damage_scope(next.as_ref()));
            self.hover_hit = next;
            if scope != DamageScope::None {
                self.app.mark_dirty(scope);
            }
        }
    }

    fn handle_modal_left_release(&mut self, px: f64, py: f64) {
        if self.app.active_modal.is_none() {
            return;
        }
        let w = self.width.max(1);
        let h = self.height.max(1);
        let act = if let Some(modal) = &mut self.app.active_modal {
            let sheet = modal.sheet_mut();
            if let Some(tf) = &mut sheet.input {
                tf.on_pointer_up();
            }
            sheet.on_pointer_up(px, py, w, h)
        } else {
            None
        };
        match act {
            Some(rusty_ui::DialogAction::Confirm) => {
                self.app.confirm_active_modal();
                self.layout_cache = None;
                self.flush_pending_service_commands();
            }
            Some(rusty_ui::DialogAction::Cancel) => {
                self.app.dismiss_active_modal();
                self.layout_cache = None;
            }
            None => self.app.mark_dirty(DamageScope::Full),
        }
    }
}

impl RustyApp for SettingsApp {
    fn on_configure(&mut self, ctx: &mut AppContext<'_>, info: ConfigureInfo) {
        let size_changed = info.width != self.width || info.height != self.height;
        let max_changed = self.app.maximized != info.is_maximized;
        if size_changed || max_changed || self.app.focused != info.is_activated {
            self.width = info.width;
            self.height = info.height;
            self.layout_cache = None;
            if size_changed || max_changed {
                let layout = self.current_layout();
                self.app
                    .clamp_scroll_bounds(layout.max_sidebar_scroll, layout.max_page_scroll);
            }
            self.app.maximized = info.is_maximized;
            self.app.focused = info.is_activated;
            self.app.mark_dirty(if size_changed || max_changed {
                DamageScope::Full
            } else {
                DamageScope::Sidebar
            });
        }
        self.sync_glass(ctx);
        ctx.request_redraw();
    }

    fn on_key(&mut self, ctx: &mut AppContext<'_>, event: KeyEvent) {
        self.handle_key_press(ctx, &event);
        self.sync_dirty(ctx);
    }

    fn on_pointer(&mut self, ctx: &mut AppContext<'_>, events: &[PointerEvent]) {
        for ev in events {
            self.pointer_pos = ev.position;
            match ev.kind {
                PointerEventKind::Enter { .. } => {
                    if !self.handle_modal_pointer_motion(ev.position.0, ev.position.1)
                        && !self.update_scrollbar_drag(ev.position.1)
                    {
                        self.handle_page_pointer_motion(ev.position.0, ev.position.1);
                    }
                }
                PointerEventKind::Motion { .. } => {
                    if !self.handle_modal_pointer_motion(ev.position.0, ev.position.1)
                        && !self.update_stepper_drag(ev.position.0)
                        && !self.update_scrollbar_drag(ev.position.1)
                    {
                        self.handle_page_pointer_motion(ev.position.0, ev.position.1);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if let Some(prev) = self.hover_hit.take() {
                        let scope = hit_damage_scope(Some(&prev));
                        if scope != DamageScope::None {
                            self.app.mark_dirty(scope);
                        }
                    }
                }
                PointerEventKind::Press {
                    button: BTN_LEFT,
                    serial,
                    ..
                } => {
                    self.pointer_pressed = true;
                    self.execute_left_click(ctx, ev.position.0, ev.position.1, serial);
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => {
                    self.pointer_pressed = false;
                    self.scrollbar_drag = None;
                    self.stepper_drag = None;
                    self.app.search_field.on_pointer_up();
                    self.handle_modal_left_release(ev.position.0, ev.position.1);
                }
                PointerEventKind::Axis {
                    vertical, source, ..
                } => {
                    let (delta_px, is_discrete) = axis_scroll_delta_px(vertical, source);
                    if delta_px.abs() >= 1e-4 {
                        let layout = self.current_layout();
                        let in_sidebar = self.pointer_pos.0 < f64::from(SIDEBAR_WIDTH);
                        if self.app.scroll_smooth_by(
                            in_sidebar,
                            delta_px,
                            layout.max_sidebar_scroll,
                            layout.max_page_scroll,
                            is_discrete,
                        ) {
                            self.layout_cache = None;
                        }
                    }
                }
                PointerEventKind::Press { .. } | PointerEventKind::Release { .. } => {}
            }
        }
        self.sync_dirty(ctx);
    }

    fn on_tick(&mut self, ctx: &mut AppContext<'_>) {
        let now = Instant::now();
        let _ = self.app.update_caret_blink(now);
        if self.app.step_ui_animations(now) {
            self.layout_cache = None;
        }
        if self.app.poll_external_config_if_due(now) {
            self.layout_cache = None;
        }
        while let Ok(snap) = self.service_rx.try_recv() {
            if self.app.apply_service_snapshot(snap) {
                self.layout_cache = None;
            }
        }
        if self.app.poll_updates() {
            self.layout_cache = None;
        }
        if self.app.step_scroll_animation(now) {
            self.layout_cache = None;
        }
        self.sync_dirty(ctx);
    }

    fn poll_interval(&self) -> Duration {
        if self.app.is_ui_animating() || self.app.active_modal.is_some() {
            Duration::from_millis(16)
        } else if self.app.search_focused {
            Duration::from_millis(120)
        } else {
            Duration::from_millis(200)
        }
    }

    fn on_close_requested(&mut self, _ctx: &mut AppContext<'_>) -> bool {
        self.app.request_quit();
        true
    }

    fn paint(
        &mut self,
        _painter: &mut rusty_ui::UiPainter<'_>,
        _fonts: &mut rusty_ui::UiFontEngine,
        _width: u32,
        _height: u32,
    ) {
    }

    fn paint_bgra(
        &mut self,
        canvas_bgra: &mut [u8],
        width: u32,
        height: u32,
        has_glass: bool,
    ) -> bool {
        self.width = width;
        self.height = height;
        let layout = self.current_layout();
        render_window_pixels_with_glass(
            canvas_bgra,
            width,
            height,
            &self.app,
            &layout,
            self.hover_hit.as_ref(),
            has_glass,
        );
        self.app.dirty = false;
        true
    }
}

pub(crate) fn run(opts: &StartupOptions) -> Result<(), AppError> {
    let cfg = AppConfig::new(
        "Settings",
        "org.rustyos.Settings",
        (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    )
    .with_min_size(MIN_WIDTH, MIN_HEIGHT)
    .with_decorations(WindowDecorationMode::Client)
    .with_no_gpu(opts.no_gpu)
    .with_liquid_glass(sidebar_glass_config(DEFAULT_HEIGHT));
    run_app(cfg, SettingsApp::new(opts))
}

fn sidebar_glass_config(height: u32) -> LiquidGlassConfig {
    let h = i32::try_from(height).unwrap_or(600).max(1);
    let glass_w = (SIDEBAR_WIDTH + WINDOW_CORNER_RADIUS).max(1);
    LiquidGlassConfig::sidebar(u32::try_from(WINDOW_CORNER_RADIUS).unwrap_or(14)).with_region(
        GlassRegion::Rect {
            x: 0,
            y: 0,
            width: glass_w,
            height: h,
        },
    )
}

fn spawn_service_worker() -> (Sender<ServiceCommand>, Receiver<ServiceSnapshot>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (snap_tx, snap_rx) = mpsc::channel();
    let _ = thread::Builder::new()
        .name(String::from("settings-services"))
        .spawn(move || {
            while let Ok(first) = cmd_rx.recv() {
                let mut batch = vec![first];
                while let Ok(next) = cmd_rx.try_recv() {
                    batch.push(next);
                }
                let snap = execute_coalesced_service_batch(batch);
                if snap_tx.send(snap).is_err() {
                    break;
                }
            }
        });
    (cmd_tx, snap_rx)
}

const fn hit_damage_scope(hit: Option<&SettingsHit>) -> DamageScope {
    match hit {
        None | Some(SettingsHit::ResizeEdge(_) | SettingsHit::WindowDrag) => DamageScope::None,
        Some(
            SettingsHit::TrafficClose
            | SettingsHit::TrafficMinimize
            | SettingsHit::TrafficMaximize
            | SettingsHit::SearchField
            | SettingsHit::SidebarItem(_),
        ) => DamageScope::Sidebar,
        Some(
            SettingsHit::NavBack
            | SettingsHit::NavForward
            | SettingsHit::SelectTheme(_)
            | SettingsHit::SelectAccent(_)
            | SettingsHit::PushPage(_)
            | SettingsHit::DockPosTile(_)
            | SettingsHit::BatteryTab(_)
            | SettingsHit::LowPowerDropdownTrigger
            | SettingsHit::Control(_)
            | SettingsHit::StepperMinus(_)
            | SettingsHit::StepperPlus(_)
            | SettingsHit::ScrollbarThumb,
        ) => DamageScope::Page,
    }
}

fn format_captured_shortcut(mods: Modifiers, keysym: Keysym) -> Option<String> {
    let has_mod = mods.logo || mods.ctrl || mods.alt;
    let key_name: String = match keysym {
        Keysym::Escape | Keysym::Return | Keysym::KP_Enter | Keysym::BackSpace => return None,
        Keysym::space => String::from("Space"),
        Keysym::Tab | Keysym::ISO_Left_Tab => String::from("Tab"),
        Keysym::Left => String::from("Left"),
        Keysym::Right => String::from("Right"),
        Keysym::Up => String::from("Up"),
        Keysym::Down => String::from("Down"),
        Keysym::Print => String::from("Print"),
        Keysym::F1 => String::from("F1"),
        Keysym::F2 => String::from("F2"),
        Keysym::F3 => String::from("F3"),
        Keysym::F4 => String::from("F4"),
        Keysym::F5 => String::from("F5"),
        Keysym::F6 => String::from("F6"),
        Keysym::F7 => String::from("F7"),
        Keysym::F8 => String::from("F8"),
        Keysym::F9 => String::from("F9"),
        Keysym::F10 => String::from("F10"),
        Keysym::F11 => String::from("F11"),
        Keysym::F12 => String::from("F12"),
        _ => {
            if !has_mod {
                return None;
            }
            let ch = keysym.key_char()?;
            if !ch.is_ascii_alphanumeric() {
                return None;
            }
            ch.to_ascii_uppercase().to_string()
        }
    };
    let is_standalone_fn_key = matches!(
        keysym,
        Keysym::Print
            | Keysym::F1
            | Keysym::F2
            | Keysym::F3
            | Keysym::F4
            | Keysym::F5
            | Keysym::F6
            | Keysym::F7
            | Keysym::F8
            | Keysym::F9
            | Keysym::F10
            | Keysym::F11
            | Keysym::F12
    );
    if !has_mod && !is_standalone_fn_key {
        return None;
    }
    let mut parts = Vec::with_capacity(4);
    if mods.logo {
        parts.push("Super");
    }
    if mods.ctrl {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    if mods.shift {
        parts.push("Shift");
    }
    parts.push(&key_name);
    Some(parts.join("+"))
}

fn axis_scroll_delta_px(axis: AxisScroll, source: Option<wl_pointer::AxisSource>) -> (f64, bool) {
    if axis.discrete != 0 {
        return (f64::from(axis.discrete) * 32.0, true);
    }
    if axis.absolute.abs() < 1e-4 {
        return (0.0, false);
    }
    if matches!(
        source,
        Some(wl_pointer::AxisSource::Wheel | wl_pointer::AxisSource::WheelTilt)
    ) {
        (axis.absolute * 2.8, true)
    } else {
        (axis.absolute * 1.25, false)
    }
}

const fn map_resize_edge(zone: ResizeEdgeZone) -> ResizeEdge {
    match zone {
        ResizeEdgeZone::Top => ResizeEdge::Top,
        ResizeEdgeZone::Bottom => ResizeEdge::Bottom,
        ResizeEdgeZone::Left => ResizeEdge::Left,
        ResizeEdgeZone::Right => ResizeEdge::Right,
        ResizeEdgeZone::TopLeft => ResizeEdge::TopLeft,
        ResizeEdgeZone::TopRight => ResizeEdge::TopRight,
        ResizeEdgeZone::BottomLeft => ResizeEdge::BottomLeft,
        ResizeEdgeZone::BottomRight => ResizeEdge::BottomRight,
    }
}

fn map_keysym_to_ui(keysym: Keysym, utf8: Option<&str>) -> Option<rusty_ui::TextInputKey> {
    match keysym {
        Keysym::Left => Some(rusty_ui::TextInputKey::Left),
        Keysym::Right => Some(rusty_ui::TextInputKey::Right),
        Keysym::Home => Some(rusty_ui::TextInputKey::Home),
        Keysym::End => Some(rusty_ui::TextInputKey::End),
        Keysym::BackSpace => Some(rusty_ui::TextInputKey::Backspace),
        Keysym::Delete => Some(rusty_ui::TextInputKey::Delete),
        Keysym::Return | Keysym::KP_Enter => Some(rusty_ui::TextInputKey::Enter),
        Keysym::Escape => Some(rusty_ui::TextInputKey::Escape),
        _ => utf8
            .and_then(|s| s.chars().next())
            .filter(|ch| !ch.is_control())
            .map(rusty_ui::TextInputKey::Char)
            .or_else(|| {
                keysym
                    .key_char()
                    .filter(|ch| !ch.is_control())
                    .map(rusty_ui::TextInputKey::Char)
            }),
    }
}
