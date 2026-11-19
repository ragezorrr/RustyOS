//! Terminal application state, tabs, smooth scrolling, input encoding, search, and selection.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rusty_animation::{AnimatedValue, Easing};
use smithay_client_toolkit::reexports::calloop::ping::Ping;
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::font::{CellMetrics, FontSet};
use crate::layout::{HitTarget, MIN_HEIGHT, MIN_WIDTH, TerminalLayout};
use crate::pty::{PtyEvent, PtySession, PtySpawnConfig};
use crate::theme::{ConfigWatcher, TerminalTheme};
use crate::vt::{GridPoint, TerminalEngine, TerminalModes, TextSelection};

const SCROLL_ANIM_DURATION: Duration = Duration::from_millis(120);
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(550);
const DOUBLE_CLICK_MS: u128 = 380;

/// Side-effect actions requested by terminal state transitions for the Wayland loop to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalAction {
    CloseWindow,
    MinimizeWindow,
    ToggleMaximize,
    StartInteractiveMove,
    CopyClipboard(String),
    CopyPrimary(String),
    RequestPaste,
    UpdateGlassStyle,
}

/// Incremental damage granularity to minimize GPU/SHM work on idle/blink frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DamageLevel {
    None,
    CursorOnly,
    GridAndScrollbar,
    Full,
}

/// Active scrollback search state (`Ctrl+Shift+F`).
#[derive(Debug, Clone, Default)]
pub(crate) struct SearchState {
    pub(crate) open: bool,
    pub(crate) query: String,
    pub(crate) matches: Vec<GridPoint>,
    pub(crate) active_idx: usize,
}

/// One open terminal tab combining VT grid state, PTY child, selection, and smooth scroll.
#[derive(Debug)]
pub(crate) struct TerminalTab {
    pub(crate) id: u64,
    pub(crate) engine: TerminalEngine,
    pub(crate) pty: Option<PtySession>,
    pub(crate) scroll_lines: AnimatedValue,
    pub(crate) scroll_target: f64,
    pub(crate) exited_code: Option<i32>,
    pub(crate) selection: Option<TextSelection>,
    pub(crate) selecting: bool,
    pub(crate) last_click_at: Option<Instant>,
    pub(crate) click_streak: u8,
}

impl TerminalTab {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(id: u64, cols: usize, rows: usize, pty: Option<PtySession>) -> Self {
        Self {
            id,
            engine: TerminalEngine::new(cols, rows),
            pty,
            scroll_lines: AnimatedValue::new_settled(0.0),
            scroll_target: 0.0,
            exited_code: None,
            selection: None,
            selecting: false,
            last_click_at: None,
            click_streak: 0,
        }
    }

    #[must_use]
    pub(crate) fn display_title(&self) -> String {
        let base = if self.engine.title.trim().is_empty() {
            "Terminal"
        } else {
            self.engine.title.trim()
        };
        if let Some(code) = self.exited_code {
            format!("{base} (exit {code})")
        } else {
            base.to_owned()
        }
    }
}

/// Top-level `rusty-terminal` application state shared across GPU/SHM renderers & Wayland handlers.
#[derive(Debug)]
pub(crate) struct TerminalAppState {
    pub(crate) tabs: Vec<TerminalTab>,
    pub(crate) active_tab: usize,
    next_tab_id: u64,
    pub(crate) program: Option<PathBuf>,
    pub(crate) program_args: Vec<String>,
    pub(crate) initial_cwd: Option<PathBuf>,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) maximized: bool,
    pub(crate) focused: bool,
    pub(crate) pointer_pos: Option<(f64, f64)>,
    pub(crate) hover: Option<HitTarget>,
    pub(crate) modifiers: Modifiers,
    pub(crate) font_size: f32,
    pub(crate) fonts: FontSet,
    pub(crate) metrics: CellMetrics,
    pub(crate) config_watcher: ConfigWatcher,
    pub(crate) glass_override: Option<bool>,
    pub(crate) theme: TerminalTheme,
    pub(crate) search: SearchState,
    pub(crate) damage: DamageLevel,
    pub(crate) start_time: Instant,
    pub(crate) cursor_blink_on: bool,
    last_blink_flip: Instant,
    last_config_poll: Instant,
    last_header_click: Option<(Instant, i32, i32)>,
    pty_tx: mpsc::Sender<PtyEvent>,
    pty_rx: mpsc::Receiver<PtyEvent>,
    pty_ping: Option<Ping>,
    pub(crate) pending_actions: Vec<TerminalAction>,
}

impl TerminalAppState {
    /// Initializes terminal state, fonts, theme, and optional initial PTY tab.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        width: i32,
        height: i32,
        program: Option<PathBuf>,
        program_args: Vec<String>,
        initial_cwd: Option<PathBuf>,
        glass_override: Option<bool>,
        pty_ping: Option<Ping>,
        spawn_initial_pty: bool,
    ) -> Self {
        let fonts = FontSet::load();
        let font_size = 14.0_f32;
        let metrics = fonts.metrics_for_size(font_size);
        let config_watcher = ConfigWatcher::new();
        let theme = TerminalTheme::from_config(&config_watcher.current, glass_override);
        let (pty_tx, pty_rx) = mpsc::channel();
        let now = Instant::now();

        let mut app = Self {
            tabs: Vec::new(),
            active_tab: 0,
            next_tab_id: 1,
            program,
            program_args,
            initial_cwd,
            width: width.max(MIN_WIDTH),
            height: height.max(MIN_HEIGHT),
            maximized: false,
            focused: true,
            pointer_pos: None,
            hover: None,
            modifiers: Modifiers::default(),
            font_size,
            fonts,
            metrics,
            config_watcher,
            glass_override,
            theme,
            search: SearchState::default(),
            damage: DamageLevel::Full,
            start_time: now,
            cursor_blink_on: true,
            last_blink_flip: now,
            last_config_poll: now,
            last_header_click: None,
            pty_tx,
            pty_rx,
            pty_ping,
            pending_actions: Vec::new(),
        };

        if spawn_initial_pty {
            app.spawn_tab(None);
        } else {
            let layout = app.layout();
            app.tabs.push(TerminalTab::new(
                1,
                usize::from(layout.cols),
                usize::from(layout.rows),
                None,
            ));
            app.next_tab_id = 2;
        }

        app
    }

    /// Computes current layout snapshot from dimensions, tab count, search bar, and active scrollback.
    #[must_use]
    pub(crate) fn layout(&self) -> TerminalLayout {
        let (scrollback_len, scroll_offset) =
            self.tabs.get(self.active_tab).map_or((0, 0.0), |tab| {
                (tab.engine.scrollback_len(), tab.scroll_lines.current())
            });
        TerminalLayout::compute(
            self.width,
            self.height,
            self.tabs.len().max(1),
            self.search.open,
            self.metrics,
            scrollback_len,
            scroll_offset,
        )
    }

    #[must_use]
    pub(crate) fn active_tab(&self) -> Option<&TerminalTab> {
        self.tabs.get(self.active_tab)
    }

    pub(crate) fn active_tab_mut(&mut self) -> Option<&mut TerminalTab> {
        self.tabs.get_mut(self.active_tab)
    }

    pub(crate) fn mark_damage(&mut self, level: DamageLevel) {
        if level > self.damage {
            self.damage = level;
        }
    }

    /// Spawns a new tab inheriting current tab working directory (when available) or initial cwd.
    pub(crate) fn spawn_tab(&mut self, explicit_cwd: Option<PathBuf>) {
        let inherited_cwd = explicit_cwd
            .or_else(|| {
                self.active_tab()
                    .and_then(|t| t.engine.cwd.as_deref())
                    .map(PathBuf::from)
            })
            .or_else(|| self.initial_cwd.clone());

        let layout = self.layout();
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;

        let cfg = PtySpawnConfig {
            tab_id,
            program: self.program.clone(),
            args: self.program_args.clone(),
            working_dir: inherited_cwd,
            cols: layout.cols,
            rows: layout.rows,
            pixel_w: u16::try_from(layout.grid_rect.w.max(1)).unwrap_or(u16::MAX),
            pixel_h: u16::try_from(layout.grid_rect.h.max(1)).unwrap_or(u16::MAX),
        };

        let pty = PtySession::spawn(&cfg, self.pty_tx.clone(), self.pty_ping.clone()).ok();
        let mut tab = TerminalTab::new(
            tab_id,
            usize::from(layout.cols),
            usize::from(layout.rows),
            pty,
        );
        if tab.pty.is_none() {
            let _ = tab
                .engine
                .process_bytes(b"\r\n[rusty-terminal: failed to spawn shell PTY]\r\n");
        }
        self.tabs.push(tab);
        self.active_tab = self.tabs.len().saturating_sub(1);
        self.refresh_search_matches();
        self.mark_damage(DamageLevel::Full);
    }

    /// Closes tab at `idx`, closing the window if no tabs remain.
    pub(crate) fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            self.pending_actions.push(TerminalAction::CloseWindow);
            return;
        }
        if idx < self.tabs.len() {
            let _ = self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            self.sync_all_tabs_grid_size();
            self.refresh_search_matches();
            self.mark_damage(DamageLevel::Full);
        }
    }

    pub(crate) fn select_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() && idx != self.active_tab {
            self.active_tab = idx;
            self.refresh_search_matches();
            self.mark_damage(DamageLevel::Full);
        }
    }

    /// Handles window configure resize (`width`, `height`) and notifies active PTYs of new cols/rows.
    pub(crate) fn resize(&mut self, width: i32, height: i32, maximized: bool) {
        let next_w = width.max(MIN_WIDTH);
        let next_h = height.max(MIN_HEIGHT);
        if self.width == next_w && self.height == next_h && self.maximized == maximized {
            return;
        }
        self.width = next_w;
        self.height = next_h;
        self.maximized = maximized;
        self.sync_all_tabs_grid_size();
        self.mark_damage(DamageLevel::Full);
    }

    pub(crate) fn adjust_font_size(&mut self, delta: f32, reset: bool) {
        let next = if reset {
            14.0
        } else {
            (self.font_size + delta).clamp(10.0, 26.0)
        };
        if (next - self.font_size).abs() < 0.05 {
            return;
        }
        self.font_size = next;
        self.metrics = self.fonts.metrics_for_size(self.font_size);
        self.sync_all_tabs_grid_size();
        self.mark_damage(DamageLevel::Full);
    }

    fn sync_all_tabs_grid_size(&mut self) {
        let layout = self.layout();
        let cols_u = usize::from(layout.cols);
        let rows_u = usize::from(layout.rows);
        let pw = u16::try_from(layout.grid_rect.w.max(1)).unwrap_or(u16::MAX);
        let ph = u16::try_from(layout.grid_rect.h.max(1)).unwrap_or(u16::MAX);
        for tab in &mut self.tabs {
            tab.engine.resize(cols_u, rows_u);
            let max_sb = tab.engine.scrollback_len() as f64;
            tab.scroll_target = tab.scroll_target.clamp(0.0, max_sb);
            if let Some(pty) = &tab.pty {
                pty.resize(layout.cols, layout.rows, pw, ph);
            }
        }
    }

    /// Drains incoming PTY output / exit events across all tabs.
    pub(crate) fn drain_pty_events(&mut self) -> bool {
        let mut any = false;
        let mut close_indices = Vec::new();
        while let Ok(ev) = self.pty_rx.try_recv() {
            any = true;
            match ev {
                PtyEvent::Output { tab_id, bytes } => {
                    let reduced = self.theme.reduced_motion;
                    let now = self.start_time.elapsed();
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        let replies = tab.engine.process_bytes(&bytes);
                        if let Some(pty) = &mut tab.pty {
                            for reply in replies {
                                pty.write_bytes(&reply);
                            }
                        }
                        if tab.scroll_target > 0.0 && tab.engine.modes.alt_screen {
                            tab.scroll_target = 0.0;
                            tab.scroll_lines.retarget(
                                0.0,
                                now,
                                Duration::ZERO,
                                Easing::Linear,
                                reduced,
                            );
                        }
                    }
                    self.mark_damage(DamageLevel::Full);
                }
                PtyEvent::Exited { tab_id, code } => {
                    if let Some((idx, tab)) = self
                        .tabs
                        .iter_mut()
                        .enumerate()
                        .find(|(_, t)| t.id == tab_id)
                    {
                        tab.exited_code = Some(code.unwrap_or(0));
                        tab.pty = None;
                        close_indices.push(idx);
                    }
                }
            }
        }
        // Close exited tabs cleanly (latest index first so indices stay valid).
        close_indices.sort_unstable_by(|a, b| b.cmp(a));
        for idx in close_indices {
            if self.tabs.len() <= 1 {
                self.pending_actions.push(TerminalAction::CloseWindow);
            } else {
                self.close_tab(idx);
            }
        }
        if any && self.search.open {
            self.refresh_search_matches();
        }
        any
    }

    /// Advances smooth scroll animations, cursor blink, and live `config.toml` reload (~2Hz poll).
    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.start_time);
        let mut animating = false;

        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            if tab.scroll_lines.is_active() {
                let _ = tab.scroll_lines.tick(elapsed);
                if idx == self.active_tab {
                    self.damage = self.damage.max(DamageLevel::GridAndScrollbar);
                }
                animating = animating || tab.scroll_lines.is_active();
            }
        }

        if self.focused
            && now.saturating_duration_since(self.last_blink_flip) >= CURSOR_BLINK_INTERVAL
        {
            self.cursor_blink_on = !self.cursor_blink_on;
            self.last_blink_flip = now;
            self.mark_damage(DamageLevel::CursorOnly);
        }

        if now.saturating_duration_since(self.last_config_poll) >= Duration::from_millis(450) {
            self.last_config_poll = now;
            if self.config_watcher.poll_reload() {
                self.theme =
                    TerminalTheme::from_config(&self.config_watcher.current, self.glass_override);
                self.pending_actions.push(TerminalAction::UpdateGlassStyle);
                self.mark_damage(DamageLevel::Full);
            }
        }

        animating
    }

    #[must_use]
    pub(crate) fn has_active_animation(&self) -> bool {
        self.tabs.iter().any(|t| t.scroll_lines.is_active())
    }

    pub(crate) fn scroll_active_tab(&mut self, delta_lines: f64) {
        let reduced = self.theme.reduced_motion;
        let now = self.start_time.elapsed();
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if tab.engine.modes.alt_screen {
            return;
        }
        let max_sb = tab.engine.scrollback_len() as f64;
        tab.scroll_target = (tab.scroll_target + delta_lines).clamp(0.0, max_sb);
        tab.scroll_lines.retarget(
            tab.scroll_target,
            now,
            SCROLL_ANIM_DURATION,
            Easing::EaseOutCubic,
            reduced,
        );
        self.mark_damage(DamageLevel::GridAndScrollbar);
    }

    pub(crate) fn snap_scroll_to_bottom(&mut self) {
        let now = self.start_time.elapsed();
        let reduced = self.theme.reduced_motion;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.scroll_target = 0.0;
            tab.scroll_lines
                .retarget(0.0, now, Duration::ZERO, Easing::Linear, reduced);
        }
    }

    /// Handles mouse press / drag / release, supporting both CSD chrome and terminal selection / SGR mouse.
    pub(crate) fn handle_pointer_button(&mut self, px: f64, py: f64, button: u8, pressed: bool) {
        #[allow(clippy::cast_possible_truncation)]
        let ix = px.round() as i32;
        #[allow(clippy::cast_possible_truncation)]
        let iy = py.round() as i32;
        let layout = self.layout();
        let target = layout.hit_test(ix, iy);

        if pressed && button == 0 && self.handle_chrome_click(target, &layout, ix, iy) {
            return;
        }

        if target == HitTarget::TerminalGrid {
            let modes = self
                .active_tab()
                .map_or_else(TerminalModes::default, |t| t.engine.modes);
            if modes.mouse_clicks || modes.mouse_drag || modes.mouse_sgr {
                let (sb, off) = self.active_tab().map_or((0, 0.0), |t| {
                    (t.engine.scrollback_len(), t.scroll_lines.current())
                });
                let (_, col, row) = layout.point_to_cell(self.metrics, sb, off, ix, iy);
                let seq = encode_mouse_sgr(button, pressed, false, self.modifiers, col, row);
                if let Some(pty) = self.active_tab_mut().and_then(|t| t.pty.as_mut()) {
                    pty.write_bytes(&seq);
                }
                return;
            }
            if button == 0 {
                self.handle_selection_button(&layout, ix, iy, pressed);
            } else if !pressed && button == 1 {
                self.pending_actions.push(TerminalAction::RequestPaste);
            }
        } else if !pressed
            && button == 0
            && let Some(tab) = self.active_tab_mut()
        {
            tab.selecting = false;
        }
    }

    fn handle_chrome_click(
        &mut self,
        target: HitTarget,
        layout: &TerminalLayout,
        ix: i32,
        iy: i32,
    ) -> bool {
        match target {
            HitTarget::CloseButton => self.pending_actions.push(TerminalAction::CloseWindow),
            HitTarget::MinimizeButton => self.pending_actions.push(TerminalAction::MinimizeWindow),
            HitTarget::MaximizeButton => self.pending_actions.push(TerminalAction::ToggleMaximize),
            HitTarget::GlassToggle => {
                let currently_glass = !self.theme.reduce_transparency;
                self.glass_override = Some(!currently_glass);
                self.theme =
                    TerminalTheme::from_config(&self.config_watcher.current, self.glass_override);
                self.pending_actions.push(TerminalAction::UpdateGlassStyle);
                self.mark_damage(DamageLevel::Full);
            }
            HitTarget::SearchToggle => self.toggle_search(),
            HitTarget::NewTabButton => self.spawn_tab(None),
            HitTarget::Tab(idx) => {
                self.select_tab(idx);
                self.pending_actions
                    .push(TerminalAction::StartInteractiveMove);
            }
            HitTarget::TabClose(idx) => self.close_tab(idx),
            HitTarget::HeaderDrag => {
                let now = Instant::now();
                let is_double = self.last_header_click.is_some_and(|(prev, px, py)| {
                    now.duration_since(prev).as_millis() <= DOUBLE_CLICK_MS
                        && (ix - px).abs() <= 6
                        && (iy - py).abs() <= 6
                });
                if is_double {
                    self.last_header_click = None;
                    self.pending_actions.push(TerminalAction::ToggleMaximize);
                } else {
                    self.last_header_click = Some((now, ix, iy));
                    self.pending_actions
                        .push(TerminalAction::StartInteractiveMove);
                }
            }
            HitTarget::SearchPrev => self.step_search(-1),
            HitTarget::SearchNext => self.step_search(1),
            HitTarget::SearchClose => {
                self.search.open = false;
                self.sync_all_tabs_grid_size();
                self.mark_damage(DamageLevel::Full);
            }
            HitTarget::SearchBar => {}
            HitTarget::ScrollbarTrack => {
                self.jump_scrollbar(layout, iy, ix);
            }
            HitTarget::TerminalGrid => return false,
        }
        true
    }

    fn jump_scrollbar(&mut self, layout: &TerminalLayout, iy: i32, _ix: i32) {
        let track = layout.scrollbar_track;
        let frac_from_top =
            f64::from((iy - track.y).clamp(0, track.h.max(1))) / f64::from(track.h.max(1));
        let now = self.start_time.elapsed();
        let reduced = self.theme.reduced_motion;
        if let Some(tab) = self.active_tab_mut() {
            let max_sb = tab.engine.scrollback_len() as f64;
            let target = ((1.0 - frac_from_top) * max_sb).clamp(0.0, max_sb);
            tab.scroll_target = target;
            tab.scroll_lines.retarget(
                target,
                now,
                SCROLL_ANIM_DURATION,
                Easing::EaseOutCubic,
                reduced,
            );
            self.mark_damage(DamageLevel::GridAndScrollbar);
        }
    }

    fn handle_selection_button(
        &mut self,
        layout: &TerminalLayout,
        ix: i32,
        iy: i32,
        pressed: bool,
    ) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if !pressed {
            tab.selecting = false;
            if let Some(sel) = tab.selection {
                let text = tab.engine.extract_selection_text(sel);
                if !text.is_empty() {
                    self.pending_actions.push(TerminalAction::CopyPrimary(text));
                }
            }
            return;
        }

        let now = Instant::now();
        let same_click = tab
            .last_click_at
            .is_some_and(|prev| now.duration_since(prev).as_millis() <= DOUBLE_CLICK_MS);
        tab.click_streak = if same_click {
            tab.click_streak.saturating_add(1).min(3)
        } else {
            1
        };
        tab.last_click_at = Some(now);

        let (point, _, _) = layout.point_to_cell(
            self.metrics,
            tab.engine.scrollback_len(),
            tab.scroll_lines.current(),
            ix,
            iy,
        );

        match tab.click_streak {
            2 => {
                let cols = tab.engine.cols();
                let mut start_col = point.col;
                let mut end_col = point.col;
                if let Some(cells) = tab.engine.row_cells(point.abs_line) {
                    while start_col > 0 && is_word_char(cells[start_col - 1].ch) {
                        start_col -= 1;
                    }
                    while end_col + 1 < cols && is_word_char(cells[end_col + 1].ch) {
                        end_col += 1;
                    }
                }
                tab.selection = Some(TextSelection {
                    anchor: GridPoint {
                        abs_line: point.abs_line,
                        col: start_col,
                    },
                    active: GridPoint {
                        abs_line: point.abs_line,
                        col: end_col,
                    },
                });
                tab.selecting = false;
            }
            3 => {
                tab.selection = Some(TextSelection {
                    anchor: GridPoint {
                        abs_line: point.abs_line,
                        col: 0,
                    },
                    active: GridPoint {
                        abs_line: point.abs_line,
                        col: tab.engine.cols().saturating_sub(1),
                    },
                });
                tab.selecting = false;
            }
            _ => {
                tab.selection = Some(TextSelection {
                    anchor: point,
                    active: point,
                });
                tab.selecting = true;
            }
        }
        self.mark_damage(DamageLevel::GridAndScrollbar);
    }

    pub(crate) fn handle_pointer_motion(&mut self, px: f64, py: f64) {
        self.pointer_pos = Some((px, py));
        #[allow(clippy::cast_possible_truncation)]
        let ix = px.round() as i32;
        #[allow(clippy::cast_possible_truncation)]
        let iy = py.round() as i32;
        let layout = self.layout();
        let next_hover = Some(layout.hit_test(ix, iy));
        if next_hover != self.hover {
            self.hover = next_hover;
            self.mark_damage(DamageLevel::Full);
        }

        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.selecting {
                let (pt, _, _) = layout.point_to_cell(
                    self.metrics,
                    tab.engine.scrollback_len(),
                    tab.scroll_lines.current(),
                    ix,
                    iy,
                );
                if let Some(sel) = &mut tab.selection
                    && sel.active != pt
                {
                    sel.active = pt;
                    self.mark_damage(DamageLevel::GridAndScrollbar);
                }
            } else if tab.engine.modes.mouse_drag {
                let (_, col, row) = layout.point_to_cell(
                    self.metrics,
                    tab.engine.scrollback_len(),
                    tab.scroll_lines.current(),
                    ix,
                    iy,
                );
                let seq = encode_mouse_sgr(0, true, true, self.modifiers, col, row);
                if let Some(pty) = &mut tab.pty {
                    pty.write_bytes(&seq);
                }
            }
        }
    }

    pub(crate) fn handle_scroll_wheel(&mut self, vertical_steps: f64) {
        let modes = self
            .active_tab()
            .map_or_else(TerminalModes::default, |t| t.engine.modes);
        if modes.alt_screen && (modes.mouse_clicks || modes.mouse_sgr) {
            let btn = if vertical_steps < 0.0 { 64 } else { 65 };
            let (px, py) = self.pointer_pos.unwrap_or((100.0, 100.0));
            let layout = self.layout();
            #[allow(clippy::cast_possible_truncation)]
            let (_, col, row) = layout.point_to_cell(self.metrics, 0, 0.0, px as i32, py as i32);
            let seq = encode_mouse_sgr(btn, true, false, self.modifiers, col, row);
            if let Some(pty) = self.active_tab_mut().and_then(|t| t.pty.as_mut()) {
                pty.write_bytes(&seq);
            }
            return;
        }
        // Negative axis steps scroll up into history in Wayland.
        self.scroll_active_tab(-vertical_steps * 3.0);
    }

    pub(crate) fn set_focus(&mut self, focused: bool) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        self.cursor_blink_on = true;
        self.last_blink_flip = Instant::now();
        if let Some(tab) = self.active_tab_mut()
            && tab.engine.modes.focus_reporting
        {
            let seq: &[u8] = if focused { b"\x1b[I" } else { b"\x1b[O" };
            if let Some(pty) = &mut tab.pty {
                pty.write_bytes(seq);
            }
        }
        self.mark_damage(DamageLevel::Full);
    }

    /// Handles keyboard input: first checks UI shortcuts & search bar, then forwards VT sequence to PTY.
    pub(crate) fn handle_key_press(&mut self, keysym: Keysym, utf8: Option<&str>) {
        if self.handle_shortcut(keysym) {
            return;
        }
        if self.search.open && self.handle_search_key(keysym, utf8) {
            return;
        }
        let modes = self
            .active_tab()
            .map_or_else(TerminalModes::default, |t| t.engine.modes);
        if let Some(bytes) = encode_key_to_vt(keysym, utf8, self.modifiers, modes) {
            self.snap_scroll_to_bottom();
            self.cursor_blink_on = true;
            self.last_blink_flip = Instant::now();
            if let Some(tab) = self.active_tab_mut() {
                tab.selection = None;
                if let Some(pty) = &mut tab.pty {
                    pty.write_bytes(&bytes);
                }
            }
            self.mark_damage(DamageLevel::CursorOnly);
        }
    }

    /// Pastes text into active PTY, respecting bracketed paste mode (`?2004`).
    pub(crate) fn paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.search.open {
            self.search
                .query
                .push_str(text.trim_end_matches(['\r', '\n']));
            self.refresh_search_matches();
            self.mark_damage(DamageLevel::Full);
            return;
        }
        self.snap_scroll_to_bottom();
        let bracketed = self
            .active_tab()
            .is_some_and(|t| t.engine.modes.bracketed_paste);
        let payload = if bracketed {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.as_bytes().to_vec()
        };
        if let Some(pty) = self.active_tab_mut().and_then(|t| t.pty.as_mut()) {
            pty.write_bytes(&payload);
        }
    }

    fn handle_shortcut(&mut self, keysym: Keysym) -> bool {
        let ctrl_shift = self.modifiers.ctrl && self.modifiers.shift;
        if ctrl_shift && matches!(keysym, Keysym::c | Keysym::C)
            || (self.modifiers.ctrl && keysym == Keysym::Insert)
        {
            if let Some(tab) = self.active_tab()
                && let Some(sel) = tab.selection
            {
                let text = tab.engine.extract_selection_text(sel);
                if !text.is_empty() {
                    self.pending_actions
                        .push(TerminalAction::CopyClipboard(text));
                }
            }
            return true;
        }
        if ctrl_shift && matches!(keysym, Keysym::v | Keysym::V)
            || (self.modifiers.shift && keysym == Keysym::Insert)
        {
            self.pending_actions.push(TerminalAction::RequestPaste);
            return true;
        }
        if ctrl_shift && matches!(keysym, Keysym::t | Keysym::T) {
            self.spawn_tab(None);
            return true;
        }
        if ctrl_shift && matches!(keysym, Keysym::w | Keysym::W) {
            self.close_tab(self.active_tab);
            return true;
        }
        if ctrl_shift && matches!(keysym, Keysym::f | Keysym::F) {
            self.toggle_search();
            return true;
        }
        if ctrl_shift && matches!(keysym, Keysym::k | Keysym::K) {
            if let Some(tab) = self.active_tab_mut() {
                tab.engine.clear_scrollback_and_reset_view();
                tab.scroll_target = 0.0;
                self.snap_scroll_to_bottom();
                self.mark_damage(DamageLevel::Full);
            }
            return true;
        }
        if self.modifiers.ctrl
            && matches!(
                keysym,
                Keysym::Tab | Keysym::ISO_Left_Tab | Keysym::Page_Down | Keysym::Page_Up
            )
        {
            if self.tabs.len() > 1 {
                let back = self.modifiers.shift
                    || keysym == Keysym::Page_Up
                    || keysym == Keysym::ISO_Left_Tab;
                let next = if back {
                    (self.active_tab + self.tabs.len() - 1) % self.tabs.len()
                } else {
                    (self.active_tab + 1) % self.tabs.len()
                };
                self.select_tab(next);
            }
            return true;
        }
        if self.modifiers.ctrl && matches!(keysym, Keysym::plus | Keysym::equal | Keysym::KP_Add) {
            self.adjust_font_size(1.0, false);
            return true;
        }
        if self.modifiers.ctrl && matches!(keysym, Keysym::minus | Keysym::KP_Subtract) {
            self.adjust_font_size(-1.0, false);
            return true;
        }
        if self.modifiers.ctrl && matches!(keysym, Keysym::_0 | Keysym::KP_0) {
            self.adjust_font_size(0.0, true);
            return true;
        }
        if self.modifiers.shift && keysym == Keysym::Page_Up {
            let step = f64::from(self.layout().rows.saturating_sub(2).max(1));
            self.scroll_active_tab(step);
            return true;
        }
        if self.modifiers.shift && keysym == Keysym::Page_Down {
            let step = f64::from(self.layout().rows.saturating_sub(2).max(1));
            self.scroll_active_tab(-step);
            return true;
        }
        false
    }

    fn toggle_search(&mut self) {
        self.search.open = !self.search.open;
        if self.search.open {
            self.refresh_search_matches();
        }
        self.sync_all_tabs_grid_size();
        self.mark_damage(DamageLevel::Full);
    }

    fn handle_search_key(&mut self, keysym: Keysym, utf8: Option<&str>) -> bool {
        match keysym {
            Keysym::Escape => {
                self.search.open = false;
                self.sync_all_tabs_grid_size();
                self.mark_damage(DamageLevel::Full);
            }
            Keysym::BackSpace => {
                self.search.query.pop();
                self.refresh_search_matches();
                self.mark_damage(DamageLevel::Full);
            }
            Keysym::Return | Keysym::KP_Enter => {
                let dir = if self.modifiers.shift { -1 } else { 1 };
                self.step_search(dir);
            }
            _ => {
                if let Some(txt) = utf8
                    && !self.modifiers.ctrl
                    && !self.modifiers.alt
                    && txt.chars().all(|c| !c.is_control())
                {
                    self.search.query.push_str(txt);
                    self.refresh_search_matches();
                    self.mark_damage(DamageLevel::Full);
                }
            }
        }
        true
    }

    fn refresh_search_matches(&mut self) {
        self.search.matches.clear();
        self.search.active_idx = 0;
        if self.search.query.is_empty() {
            return;
        }
        if let Some(tab) = self.tabs.get(self.active_tab) {
            self.search.matches = tab.engine.search_scrollback(&self.search.query);
            if !self.search.matches.is_empty() {
                self.search.active_idx = self.search.matches.len() - 1;
                self.scroll_to_active_match();
            }
        }
    }

    fn step_search(&mut self, delta: isize) {
        if self.search.matches.is_empty() {
            return;
        }
        let len = self.search.matches.len();
        #[allow(clippy::cast_possible_wrap)]
        let cur = self.search.active_idx as isize;
        #[allow(clippy::cast_possible_wrap)]
        let next = (cur + delta).rem_euclid(len as isize);
        #[allow(clippy::cast_sign_loss)]
        {
            self.search.active_idx = next as usize;
        }
        self.scroll_to_active_match();
        self.mark_damage(DamageLevel::Full);
    }

    fn scroll_to_active_match(&mut self) {
        let Some(&pt) = self.search.matches.get(self.search.active_idx) else {
            return;
        };
        let now = self.start_time.elapsed();
        let reduced = self.theme.reduced_motion;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let sb = tab.engine.scrollback_len();
            let target = (sb as f64 - pt.abs_line as f64).clamp(0.0, sb as f64);
            tab.scroll_target = target;
            tab.scroll_lines.retarget(
                target,
                now,
                SCROLL_ANIM_DURATION,
                Easing::EaseOutCubic,
                reduced,
            );
        }
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '~' | ':')
}

/// Encodes keyboard keysyms + modifiers + UTF-8 text into VT/xterm escape sequences.
#[must_use]
pub(crate) fn encode_key_to_vt(
    keysym: Keysym,
    utf8: Option<&str>,
    mods: Modifiers,
    term_modes: TerminalModes,
) -> Option<Vec<u8>> {
    let mod_param = xterm_modifier_param(mods);

    if let Some(seq) = encode_special_key(keysym, mod_param, term_modes) {
        return Some(seq);
    }

    if mods.ctrl
        && !mods.alt
        && let Some(b) = encode_ctrl_keysym(keysym, utf8)
    {
        return Some(vec![b]);
    }

    if let Some(txt) = utf8
        && !txt.is_empty()
    {
        if mods.alt {
            let mut out = Vec::with_capacity(1 + txt.len());
            out.push(0x1B);
            out.extend_from_slice(txt.as_bytes());
            return Some(out);
        }
        return Some(txt.as_bytes().to_vec());
    }
    None
}

fn xterm_modifier_param(mods: Modifiers) -> u8 {
    let bits = u8::from(mods.shift) + (u8::from(mods.alt) * 2) + (u8::from(mods.ctrl) * 4);
    if bits == 0 { 0 } else { bits + 1 }
}

fn encode_special_key(keysym: Keysym, mod_param: u8, modes: TerminalModes) -> Option<Vec<u8>> {
    let simple = match keysym {
        Keysym::Return | Keysym::KP_Enter => Some(b"\r".as_slice()),
        Keysym::Escape => Some(b"\x1b".as_slice()),
        Keysym::Tab => Some(b"\t".as_slice()),
        Keysym::ISO_Left_Tab => Some(b"\x1b[Z".as_slice()),
        _ => None,
    };
    if let Some(bytes) = simple {
        return Some(bytes.to_vec());
    }
    if keysym == Keysym::BackSpace {
        return Some(if mod_param == 3 {
            b"\x1b\x7f".to_vec()
        } else {
            b"\x7f".to_vec()
        });
    }

    if let Some(letter) = match keysym {
        Keysym::Up => Some(b'A'),
        Keysym::Down => Some(b'B'),
        Keysym::Right => Some(b'C'),
        Keysym::Left => Some(b'D'),
        Keysym::Home => Some(b'H'),
        Keysym::End => Some(b'F'),
        _ => None,
    } {
        return Some(if mod_param == 0 {
            let prefix = if modes.app_cursor_keys { b'O' } else { b'[' };
            vec![0x1B, prefix, letter]
        } else {
            format!("\x1b[1;{mod_param}{}", char::from(letter)).into_bytes()
        });
    }

    let tilde_code = match keysym {
        Keysym::Insert => Some(2),
        Keysym::Delete => Some(3),
        Keysym::Page_Up => Some(5),
        Keysym::Page_Down => Some(6),
        Keysym::F5 => Some(15),
        Keysym::F6 => Some(17),
        Keysym::F7 => Some(18),
        Keysym::F8 => Some(19),
        Keysym::F9 => Some(20),
        Keysym::F10 => Some(21),
        Keysym::F11 => Some(23),
        Keysym::F12 => Some(24),
        _ => None,
    };
    if let Some(code) = tilde_code {
        return Some(if mod_param == 0 {
            format!("\x1b[{code}~").into_bytes()
        } else {
            format!("\x1b[{code};{mod_param}~").into_bytes()
        });
    }

    let f1_f4 = match keysym {
        Keysym::F1 => Some(b'P'),
        Keysym::F2 => Some(b'Q'),
        Keysym::F3 => Some(b'R'),
        Keysym::F4 => Some(b'S'),
        _ => None,
    };
    f1_f4.map(|l| {
        if mod_param == 0 {
            vec![0x1B, b'O', l]
        } else {
            format!("\x1b[1;{mod_param}{}", char::from(l)).into_bytes()
        }
    })
}

fn encode_ctrl_keysym(keysym: Keysym, utf8: Option<&str>) -> Option<u8> {
    if keysym == Keysym::space {
        return Some(0x00);
    }
    let ch = utf8.and_then(|s| s.chars().next()).or_else(|| {
        let raw = keysym.raw();
        if (0x61..=0x7a).contains(&raw) || (0x41..=0x5a).contains(&raw) {
            char::from_u32(raw)
        } else {
            None
        }
    })?;
    match ch {
        '@' | ' ' => Some(0x00),
        'a'..='z' => Some((ch as u8) - b'a' + 1),
        'A'..='Z' => Some((ch as u8) - b'A' + 1),
        '[' => Some(0x1B),
        '\\' => Some(0x1C),
        ']' => Some(0x1D),
        '^' | '6' => Some(0x1E),
        '_' | '/' => Some(0x1F),
        _ => None,
    }
}

/// Encodes pointer button/motion in xterm SGR 1006 format (`CSI < btn ; col ; row M/m`).
#[must_use]
pub(crate) fn encode_mouse_sgr(
    button: u8,
    pressed: bool,
    motion: bool,
    mods: Modifiers,
    col_0: usize,
    row_0: usize,
) -> Vec<u8> {
    let mut cb = button;
    if motion {
        cb |= 32;
    }
    if mods.shift {
        cb |= 4;
    }
    if mods.alt {
        cb |= 8;
    }
    if mods.ctrl {
        cb |= 16;
    }
    let suffix = if pressed || motion { 'M' } else { 'm' };
    format!("\x1b[<{};{};{}{}", cb, col_0 + 1, row_0 + 1, suffix).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_ctrl_app_cursor_and_sgr_mouse_correctly() {
        let mut modes = TerminalModes::default();
        assert_eq!(
            encode_key_to_vt(Keysym::Up, None, Modifiers::default(), modes),
            Some(b"\x1b[A".to_vec())
        );
        modes.app_cursor_keys = true;
        assert_eq!(
            encode_key_to_vt(Keysym::Up, None, Modifiers::default(), modes),
            Some(b"\x1bOA".to_vec())
        );
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert_eq!(
            encode_key_to_vt(Keysym::c, Some("c"), ctrl, modes),
            Some(vec![0x03])
        );
        assert_eq!(
            encode_mouse_sgr(0, true, false, Modifiers::default(), 9, 4),
            b"\x1b[<0;10;5M".to_vec()
        );
    }

    #[test]
    fn search_and_smooth_scroll_jump_within_bounds() {
        let mut app = TerminalAppState::new(900, 560, None, Vec::new(), None, None, None, false);
        for i in 0..80 {
            let line = format!("line-{i:03} alpha-needle\r\n");
            let _ = app.tabs[0].engine.process_bytes(line.as_bytes());
        }
        app.scroll_active_tab(12.0);
        assert!(app.tabs[0].scroll_target > 0.0);
        app.search.open = true;
        app.search.query = "alpha-needle".to_owned();
        app.refresh_search_matches();
        assert_eq!(app.search.matches.len(), 80);
    }

    #[test]
    fn header_drag_and_double_click_maximize_actions() {
        let mut app = TerminalAppState::new(900, 560, None, Vec::new(), None, None, None, false);
        // Single press on header title area starts interactive move.
        app.handle_pointer_button(320.0, 18.0, 0, true);
        assert_eq!(
            app.pending_actions,
            vec![TerminalAction::StartInteractiveMove]
        );
        app.pending_actions.clear();

        // Second press far away (>6px) starts a new drag instead of maximizing.
        app.handle_pointer_button(420.0, 18.0, 0, true);
        assert_eq!(
            app.pending_actions,
            vec![TerminalAction::StartInteractiveMove]
        );
        app.pending_actions.clear();

        // Immediate second click at the same spot toggles maximize.
        app.handle_pointer_button(422.0, 18.0, 0, true);
        assert_eq!(app.pending_actions, vec![TerminalAction::ToggleMaximize]);
    }
}
