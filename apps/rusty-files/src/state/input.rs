//! Keyboard input dispatch, scroll axis damping, caret blinking and animation ticks.

use std::time::{Duration, Instant};

use rusty_services::file_manager::{FileViewMode, TransferConflictDecision, suggest_go_to_paths};

use super::*;

impl FilesAppState {
    pub(crate) fn handle_key_input(
        &mut self,
        key_name: &str,
        utf8: Option<&str>,
        ctrl: bool,
        shift: bool,
    ) {
        if self.handle_modal_key_input(key_name, utf8, ctrl) {
            return;
        }
        if self.handle_shortcut_key_input(key_name, utf8, ctrl, shift) {
            return;
        }
        if key_name == "Escape" {
            if self.popup.is_some() {
                self.popup = None;
            } else if self.search_active {
                if self.search_query.is_empty() {
                    self.search_active = false;
                    self.set_search_scope(SearchScope::ThisFolder);
                } else {
                    self.set_search_query(String::new());
                }
            }
            self.dirty = true;
            return;
        }
        if self.search_active {
            if key_name == "BackSpace" {
                self.search_query.pop();
                self.refresh_search_results();
                self.reset_caret_blink();
                self.dirty = true;
                return;
            }
            if let Some(text) = utf8
                && !ctrl
                && text.chars().all(|c| !c.is_control())
                && !text.is_empty()
            {
                self.search_query.push_str(text);
                self.refresh_search_results();
                self.reset_caret_blink();
                self.dirty = true;
                return;
            }
        }
        if key_name == "Space" {
            self.toggle_quick_look_selected();
            return;
        }
        self.handle_navigation_key(key_name);
    }

    fn handle_modal_key_input(&mut self, key_name: &str, utf8: Option<&str>, ctrl: bool) -> bool {
        if let Some(tr) = &self.active_transfer
            && tr.conflict.is_some()
        {
            if key_name == "Escape" {
                self.resolve_transfer_conflict(TransferConflictDecision::Stop);
                return true;
            }
            return true;
        }
        if self.inline_rename.is_some() {
            self.handle_inline_rename_key(key_name, utf8, ctrl);
            return true;
        }
        if self.go_to_folder.is_some() {
            self.handle_go_to_folder_key(key_name, utf8, ctrl);
            return true;
        }
        if self.get_info.is_some() && key_name == "Escape" {
            self.get_info = None;
            self.dirty = true;
            return true;
        }
        if self.quick_look.is_some() {
            match key_name {
                "Escape" | "Space" => {
                    self.quick_look = None;
                    self.dirty = true;
                }
                "Left" | "Up" => self.step_quick_look(-1),
                "Right" | "Down" => self.step_quick_look(1),
                "Return" | "KP_Enter" => {
                    self.quick_look = None;
                    self.activate_selected();
                }
                _ => {}
            }
            return true;
        }
        false
    }

    fn handle_shortcut_key_input(
        &mut self,
        key_name: &str,
        utf8: Option<&str>,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        if key_name == "F2" {
            self.start_inline_rename_selected();
            return true;
        }
        if !ctrl {
            return false;
        }
        if key_name == "Tab" {
            self.cycle_next_tab(shift);
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'f') {
            self.search_active = true;
            self.popup = None;
            self.reset_caret_blink();
            self.dirty = true;
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'n') {
            self.action_new_folder();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 't') {
            self.new_tab(None);
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'w') {
            self.close_active_tab();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'i') {
            if self.get_info.is_some() {
                self.get_info = None;
                self.dirty = true;
            } else {
                self.open_get_info_selected();
            }
            return true;
        }
        if shift && key_matches_ascii(key_name, utf8, 'g') {
            self.open_go_to_folder();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'c') {
            self.action_copy_selected();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'x') {
            self.action_cut_selected();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'v') {
            self.action_paste_into_active();
            return true;
        }
        if key_matches_ascii(key_name, utf8, 'd') {
            self.action_duplicate_selected();
            return true;
        }
        false
    }

    fn handle_inline_rename_key(&mut self, key_name: &str, utf8: Option<&str>, ctrl: bool) {
        match key_name {
            "Escape" => {
                self.inline_rename = None;
                self.dirty = true;
            }
            "Return" | "KP_Enter" => self.commit_inline_rename(),
            "BackSpace" => {
                if let Some(rn) = &mut self.inline_rename {
                    rn.draft.pop();
                    rn.error = None;
                    self.reset_caret_blink();
                    self.dirty = true;
                }
            }
            _ => {
                if let Some(text) = utf8
                    && !ctrl
                    && text.chars().all(|c| !c.is_control())
                    && !text.is_empty()
                    && let Some(rn) = &mut self.inline_rename
                {
                    rn.draft.push_str(text);
                    rn.error = None;
                    self.reset_caret_blink();
                    self.dirty = true;
                }
            }
        }
    }

    fn handle_go_to_folder_key(&mut self, key_name: &str, utf8: Option<&str>, ctrl: bool) {
        match key_name {
            "Escape" => {
                self.go_to_folder = None;
                self.dirty = true;
            }
            "Return" | "KP_Enter" => self.submit_go_to_folder(),
            "Up" => {
                if let Some(gt) = &mut self.go_to_folder
                    && !gt.suggestions.is_empty()
                {
                    gt.selected_suggestion = gt.selected_suggestion.saturating_sub(1);
                    self.dirty = true;
                }
            }
            "Down" => {
                if let Some(gt) = &mut self.go_to_folder
                    && !gt.suggestions.is_empty()
                {
                    gt.selected_suggestion =
                        (gt.selected_suggestion + 1).min(gt.suggestions.len() - 1);
                    self.dirty = true;
                }
            }
            "Tab" => {
                let current_dir = self.active_writable_directory();
                if let Some(gt) = &mut self.go_to_folder
                    && let Some(chosen) = gt.suggestions.get(gt.selected_suggestion).cloned()
                {
                    gt.input = format_go_to_display(&chosen, &self.locations);
                    gt.error = None;
                    gt.suggestions =
                        suggest_go_to_paths(&gt.input, current_dir.as_deref(), &self.locations, 6);
                    gt.selected_suggestion = 0;
                    self.reset_caret_blink();
                    self.dirty = true;
                }
            }
            "BackSpace" => {
                let current_dir = self.active_writable_directory();
                if let Some(gt) = &mut self.go_to_folder {
                    gt.input.pop();
                    gt.error = None;
                    gt.suggestions =
                        suggest_go_to_paths(&gt.input, current_dir.as_deref(), &self.locations, 6);
                    gt.selected_suggestion = 0;
                    self.reset_caret_blink();
                    self.dirty = true;
                }
            }
            _ => {
                if let Some(text) = utf8
                    && !ctrl
                    && text.chars().all(|c| !c.is_control())
                    && !text.is_empty()
                {
                    let current_dir = self.active_writable_directory();
                    if let Some(gt) = &mut self.go_to_folder {
                        gt.input.push_str(text);
                        gt.error = None;
                        gt.suggestions = suggest_go_to_paths(
                            &gt.input,
                            current_dir.as_deref(),
                            &self.locations,
                            6,
                        );
                        gt.selected_suggestion = 0;
                        self.reset_caret_blink();
                        self.dirty = true;
                    }
                }
            }
        }
    }

    fn handle_navigation_key(&mut self, key_name: &str) {
        match self.view_mode {
            FileViewMode::Icons => {
                let step = isize::try_from(self.last_icon_grid_cols.max(1)).unwrap_or(5);
                match key_name {
                    "Right" => self.move_selection_delta(1),
                    "Left" => self.move_selection_delta(-1),
                    "Down" => self.move_selection_delta(step),
                    "Up" => self.move_selection_delta(-step),
                    "Return" | "KP_Enter" => self.activate_selected(),
                    "BackSpace" => self.go_back(),
                    "Delete" => self.action_move_selected_to_trash(),
                    _ => {}
                }
            }
            FileViewMode::Gallery => match key_name {
                "Right" | "Down" => self.move_selection_delta(1),
                "Left" | "Up" => self.move_selection_delta(-1),
                "Return" | "KP_Enter" => self.activate_selected(),
                "BackSpace" => self.go_back(),
                "Delete" => self.action_move_selected_to_trash(),
                _ => {}
            },
            FileViewMode::Columns | FileViewMode::List => match key_name {
                "Down" => self.move_selection_delta(1),
                "Up" => self.move_selection_delta(-1),
                "Right" | "Return" | "KP_Enter" => self.activate_selected(),
                "Left" => {
                    if self.view_mode == FileViewMode::Columns && self.active_column > 0 {
                        self.active_column -= 1;
                        self.columns.truncate(self.active_column + 1);
                        self.follow_right_column = true;
                        self.dirty = true;
                    }
                }
                "BackSpace" => self.go_back(),
                "Delete" => self.action_move_selected_to_trash(),
                _ => {}
            },
        }
    }

    fn move_selection_delta(&mut self, delta: isize) {
        let Some(col_idx) = Some(self.active_column).filter(|&i| i < self.columns.len()) else {
            return;
        };
        let len = self.columns[col_idx].visible_entries.len();
        if len == 0 {
            return;
        }
        let next_idx = match self.columns[col_idx].selected_index {
            None => 0,
            Some(cur) => {
                let raw = cur.saturating_add_signed(delta);
                raw.min(len - 1)
            }
        };
        self.select_entry(col_idx, next_idx, false);
    }

    /// Computes horizontal column count for `Icons` grid given content viewport width.
    #[must_use]
    pub(crate) fn icon_grid_columns_for_width(content_w: i32) -> usize {
        let usable = (content_w - 28).max(ICON_CELL_W);
        usize::try_from((usable / ICON_CELL_W).max(1)).unwrap_or(1)
    }

    /// Applies continuous (`smooth = false`) or eased (`smooth = true`) scroll impulse to `area`.
    pub(crate) fn scroll_axis(
        &mut self,
        area: ScrollTargetArea,
        delta_px: f64,
        smooth: bool,
        viewport_extent: i32,
    ) {
        match area {
            ScrollTargetArea::Sidebar => {
                let content_h = sidebar_total_height(&self.sidebar_items);
                let max_scroll = (content_h - viewport_extent + 6).max(0);
                if self.sidebar_scroll_anim.rounded() != self.sidebar_scroll_y {
                    self.sidebar_scroll_anim
                        .set_immediate(self.sidebar_scroll_y);
                }
                self.sidebar_scroll_y = self
                    .sidebar_scroll_anim
                    .impulse(delta_px, max_scroll, smooth);
            }
            ScrollTargetArea::List => {
                let count = self
                    .columns
                    .first()
                    .map_or(0, |c| i32::try_from(c.visible_entries.len()).unwrap_or(0));
                let content_h = count * LIST_ROW_HEIGHT;
                let max_scroll = (content_h - viewport_extent + 8).max(0);
                if self.list_scroll_anim.rounded() != self.list_scroll_y {
                    self.list_scroll_anim.set_immediate(self.list_scroll_y);
                }
                self.list_scroll_y = self.list_scroll_anim.impulse(delta_px, max_scroll, smooth);
            }
            ScrollTargetArea::Icons => {
                let count = self.columns.first().map_or(0, |c| c.visible_entries.len());
                let cols = self.last_icon_grid_cols.max(1);
                let rows = i32::try_from(count.div_ceil(cols)).unwrap_or(0);
                let content_h = rows * ICON_CELL_H + 16;
                let max_scroll = (content_h - viewport_extent + 12).max(0);
                if self.icons_scroll_anim.rounded() != self.icons_scroll_y {
                    self.icons_scroll_anim.set_immediate(self.icons_scroll_y);
                }
                self.icons_scroll_y = self.icons_scroll_anim.impulse(delta_px, max_scroll, smooth);
                self.ensure_stage_b_assets_requested();
            }
            ScrollTargetArea::GalleryStrip => {
                let count = self
                    .columns
                    .first()
                    .map_or(0, |c| i32::try_from(c.visible_entries.len()).unwrap_or(0));
                let content_w = count * 70 + 24;
                let max_scroll = (content_w - viewport_extent + 12).max(0);
                if self.gallery_strip_scroll_anim.rounded() != self.gallery_strip_scroll_x {
                    self.gallery_strip_scroll_anim
                        .set_immediate(self.gallery_strip_scroll_x);
                }
                self.gallery_strip_scroll_x = self
                    .gallery_strip_scroll_anim
                    .impulse(delta_px, max_scroll, smooth);
                self.ensure_stage_b_assets_requested();
            }
            ScrollTargetArea::Column(col_idx) => {
                if let Some(col) = self.columns.get_mut(col_idx) {
                    let count = i32::try_from(col.visible_entries.len()).unwrap_or(0);
                    let content_h = count * ROW_HEIGHT;
                    let max_scroll = (content_h - viewport_extent + 12).max(0);
                    if col.scroll_anim.rounded() != col.scroll_y {
                        col.scroll_anim.set_immediate(col.scroll_y);
                    }
                    col.scroll_y = col.scroll_anim.impulse(delta_px, max_scroll, smooth);
                }
            }
        }
        self.note_scroll_activity();
        if smooth {
            self.last_anim_tick.get_or_insert_with(Instant::now);
        }
        self.dirty = true;
    }

    fn note_scroll_activity(&mut self) {
        self.last_scroll_activity = Some(Instant::now());
        self.overlay_scrollbar_alpha = 1.0;
    }

    /// Scrolls Miller columns horizontally (`Columns` view) or gallery strip horizontally.
    pub(crate) fn scroll_horizontal(&mut self, delta_px: f64, smooth: bool, content_w: i32) {
        if self.view_mode == FileViewMode::Gallery {
            self.scroll_axis(ScrollTargetArea::GalleryStrip, delta_px, smooth, content_w);
            return;
        }
        if self.view_mode != FileViewMode::Columns {
            return;
        }
        let total_cols_w = i32::try_from(self.columns.len()).unwrap_or(1) * MILLER_COLUMN_WIDTH;
        let preview_reserve = if self.selected_entry().is_some_and(|e| !e.is_navigable_dir()) {
            240
        } else {
            0
        };
        let max_overflow = (total_cols_w + preview_reserve - content_w).max(0);
        if max_overflow == 0 {
            return;
        }
        if self.follow_right_column {
            self.column_scroll_x = max_overflow;
            self.column_scroll_x_anim.set_immediate(max_overflow);
            self.follow_right_column = false;
        }
        self.column_scroll_x = self
            .column_scroll_x_anim
            .impulse(delta_px, max_overflow, smooth);
        self.note_scroll_activity();
        if smooth {
            self.last_anim_tick.get_or_insert_with(Instant::now);
        }
        self.dirty = true;
    }

    /// Advances smooth scroll animations and overlay scrollbar fade aligned with frame cadence.
    pub(crate) fn tick_animations(&mut self, now: Instant) -> bool {
        let mut moved = false;
        let mut active = false;

        if let Some(prev) = self.last_anim_tick {
            let dt = now
                .saturating_duration_since(prev)
                .as_secs_f64()
                .clamp(0.001, 0.033);
            self.last_anim_tick = Some(now);

            let (v, m, a) = self.sidebar_scroll_anim.tick(dt);
            if m {
                self.sidebar_scroll_y = v;
                moved = true;
            }
            active |= a;

            let (v, m, a) = self.list_scroll_anim.tick(dt);
            if m {
                self.list_scroll_y = v;
                moved = true;
            }
            active |= a;

            let (v, m, a) = self.icons_scroll_anim.tick(dt);
            if m {
                self.icons_scroll_y = v;
                self.ensure_stage_b_assets_requested();
                moved = true;
            }
            active |= a;

            let (v, m, a) = self.gallery_strip_scroll_anim.tick(dt);
            if m {
                self.gallery_strip_scroll_x = v;
                self.ensure_stage_b_assets_requested();
                moved = true;
            }
            active |= a;

            let (v, m, a) = self.column_scroll_x_anim.tick(dt);
            if m {
                self.column_scroll_x = v;
                moved = true;
            }
            active |= a;

            for col in &mut self.columns {
                let (v, m, a) = col.scroll_anim.tick(dt);
                if m {
                    col.scroll_y = v;
                    moved = true;
                }
                active |= a;
            }

            if !active {
                self.last_anim_tick = None;
            }
        } else if self.has_smooth_axes_active() {
            self.last_anim_tick = Some(now);
            active = true;
        }

        let (sb_moved, sb_fading) = self.tick_overlay_scrollbar(now);
        moved |= sb_moved;
        active |= sb_fading;

        if moved {
            self.dirty = true;
        }
        active
    }

    fn tick_overlay_scrollbar(&mut self, now: Instant) -> (bool, bool) {
        let Some(at) = self.last_scroll_activity else {
            return (false, false);
        };
        let elapsed = now.saturating_duration_since(at).as_secs_f32();
        let next_alpha = if elapsed <= 0.75 {
            1.0
        } else if elapsed >= 1.15 {
            0.0
        } else {
            1.0 - (elapsed - 0.75) / 0.40
        };
        let mut moved = (next_alpha - self.overlay_scrollbar_alpha).abs() > 0.005;
        if moved {
            self.overlay_scrollbar_alpha = next_alpha;
        }
        if elapsed >= 1.15 {
            self.overlay_scrollbar_alpha = 0.0;
            self.last_scroll_activity = None;
            moved = true;
            (moved, false)
        } else {
            (moved, elapsed >= 0.75)
        }
    }

    fn has_smooth_axes_active(&self) -> bool {
        self.sidebar_scroll_anim.is_active()
            || self.list_scroll_anim.is_active()
            || self.icons_scroll_anim.is_active()
            || self.gallery_strip_scroll_anim.is_active()
            || self.column_scroll_x_anim.is_active()
            || self.columns.iter().any(|c| c.scroll_anim.is_active())
    }

    #[must_use]
    pub(crate) fn has_active_animations(&self) -> bool {
        self.has_smooth_axes_active()
            || (self.overlay_scrollbar_alpha > 0.0 && self.overlay_scrollbar_alpha < 0.999)
    }

    /// True when the overlay scrollbar fade timer has crossed its hold window and needs redraws.
    #[must_use]
    pub(crate) fn scrollbar_needs_tick(&self, now: Instant) -> bool {
        self.last_scroll_activity
            .is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_millis(750))
    }

    /// Remaining hold delay before overlay scrollbar starts fading out.
    #[must_use]
    pub(crate) fn time_until_scrollbar_tick(&self, now: Instant) -> Option<Duration> {
        let at = self.last_scroll_activity?;
        let elapsed = now.saturating_duration_since(at);
        if elapsed < Duration::from_millis(750) {
            Some(
                Duration::from_millis(750)
                    .saturating_sub(elapsed)
                    .max(Duration::from_millis(10)),
            )
        } else {
            Some(Duration::from_millis(16))
        }
    }

    #[must_use]
    pub(crate) fn has_pending_jobs(&self) -> bool {
        self.columns.iter().any(|c| c.loading)
            || self.pending_home_search
            || !self.pending_previews.is_empty()
            || self.pending_info_path.is_some()
            || self.active_transfer.is_some()
    }

    #[must_use]
    pub(crate) const fn has_focused_text_input(&self) -> bool {
        self.search_active || self.inline_rename.is_some() || self.go_to_folder.is_some()
    }

    pub(crate) fn reset_caret_blink(&mut self) {
        self.caret_visible = true;
        self.last_caret_tick = Instant::now();
        self.dirty = true;
    }

    #[must_use]
    pub(crate) fn time_until_caret_blink(&self, now: Instant) -> Duration {
        let elapsed = now.saturating_duration_since(self.last_caret_tick);
        Duration::from_millis(530)
            .saturating_sub(elapsed)
            .max(Duration::from_millis(10))
    }

    pub(crate) fn sync_text_fields_from_strings(&mut self) {
        if self.search_field.buffer.as_str() != self.search_query {
            self.search_field.buffer.set_text(&self.search_query);
        }
        if let Some(rn) = &self.inline_rename
            && self.rename_field.buffer.as_str() != rn.draft
        {
            self.rename_field.buffer.set_text(&rn.draft);
        }
        if let Some(gt) = &self.go_to_folder
            && self.goto_field.buffer.as_str() != gt.input
        {
            self.goto_field.buffer.set_text(&gt.input);
        }
    }

    pub(crate) fn apply_focused_paste(
        &mut self,
        text: &str,
        fonts: &mut rusty_ui::UiFontEngine,
    ) -> bool {
        if !self.has_focused_text_input() || text.is_empty() {
            return false;
        }
        let mut mem = rusty_ui::MemoryClipboard::new();
        rusty_ui::ClipboardSink::set_text(&mut mem, text);
        self.handle_focused_text_key(
            rusty_ui::TextInputKey::Char('v'),
            rusty_ui::InputModifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
            fonts,
            &mut mem,
        )
    }

    pub(crate) fn handle_focused_text_key(
        &mut self,
        key: rusty_ui::TextInputKey,
        mods: rusty_ui::InputModifiers,
        fonts: &mut rusty_ui::UiFontEngine,
        clipboard: &mut dyn rusty_ui::ClipboardSink,
    ) -> bool {
        if !self.has_focused_text_input() {
            return false;
        }
        if mods.ctrl
            && let rusty_ui::TextInputKey::Char(ch) = key
            && !matches!(ch.to_ascii_lowercase(), 'a' | 'c' | 'x' | 'v')
        {
            return false;
        }
        self.sync_text_fields_from_strings();
        let now = self.epoch.elapsed();

        if self.inline_rename.is_some() {
            return self.process_rename_text_key(key, mods, now, fonts, clipboard);
        }
        if self.go_to_folder.is_some() {
            return self.process_goto_text_key(key, mods, now, fonts, clipboard);
        }
        if self.search_active {
            if matches!(key, rusty_ui::TextInputKey::Enter) {
                return false;
            }
            let ev = self
                .search_field
                .handle_key(key, mods, now, fonts, clipboard);
            match ev {
                rusty_ui::TextInputEvent::Ignored => return false,
                rusty_ui::TextInputEvent::Cancelled => {
                    if self.search_query.is_empty() {
                        self.search_active = false;
                        self.set_search_scope(SearchScope::ThisFolder);
                    } else {
                        self.set_search_query(String::new());
                    }
                    self.dirty = true;
                    return true;
                }
                rusty_ui::TextInputEvent::Changed => {
                    self.search_query = self.search_field.buffer.as_str().to_owned();
                    self.refresh_search_results();
                }
                rusty_ui::TextInputEvent::CaretMoved | rusty_ui::TextInputEvent::Submitted => {}
            }
            self.reset_caret_blink();
            self.dirty = true;
            return true;
        }
        false
    }

    fn process_rename_text_key(
        &mut self,
        key: rusty_ui::TextInputKey,
        mods: rusty_ui::InputModifiers,
        now: Duration,
        fonts: &mut rusty_ui::UiFontEngine,
        clipboard: &mut dyn rusty_ui::ClipboardSink,
    ) -> bool {
        let ev = self
            .rename_field
            .handle_key(key, mods, now, fonts, clipboard);
        match ev {
            rusty_ui::TextInputEvent::Ignored => return false,
            rusty_ui::TextInputEvent::Cancelled => {
                self.inline_rename = None;
                self.dirty = true;
            }
            rusty_ui::TextInputEvent::Submitted => self.commit_inline_rename(),
            rusty_ui::TextInputEvent::Changed => {
                if let Some(rn) = &mut self.inline_rename {
                    self.rename_field.buffer.as_str().clone_into(&mut rn.draft);
                    rn.error = None;
                }
                self.reset_caret_blink();
                self.dirty = true;
            }
            rusty_ui::TextInputEvent::CaretMoved => {
                self.reset_caret_blink();
                self.dirty = true;
            }
        }
        true
    }

    fn process_goto_text_key(
        &mut self,
        key: rusty_ui::TextInputKey,
        mods: rusty_ui::InputModifiers,
        now: Duration,
        fonts: &mut rusty_ui::UiFontEngine,
        clipboard: &mut dyn rusty_ui::ClipboardSink,
    ) -> bool {
        let ev = self.goto_field.handle_key(key, mods, now, fonts, clipboard);
        match ev {
            rusty_ui::TextInputEvent::Ignored => return false,
            rusty_ui::TextInputEvent::Cancelled => {
                self.go_to_folder = None;
                self.dirty = true;
            }
            rusty_ui::TextInputEvent::Submitted => self.submit_go_to_folder(),
            rusty_ui::TextInputEvent::Changed => {
                let current_dir = self.active_writable_directory();
                let next_str = self.goto_field.buffer.as_str().to_owned();
                if let Some(gt) = &mut self.go_to_folder {
                    gt.input = next_str;
                    gt.error = None;
                    gt.suggestions =
                        suggest_go_to_paths(&gt.input, current_dir.as_deref(), &self.locations, 6);
                    gt.selected_suggestion = 0;
                }
                self.reset_caret_blink();
                self.dirty = true;
            }
            rusty_ui::TextInputEvent::CaretMoved => {
                self.reset_caret_blink();
                self.dirty = true;
            }
        }
        true
    }

    pub(crate) fn update_caret_blink(&mut self, now: Instant) -> bool {
        if !self.has_focused_text_input() {
            if !self.caret_visible {
                self.caret_visible = true;
            }
            return false;
        }
        if now.saturating_duration_since(self.last_caret_tick) >= Duration::from_millis(530) {
            self.caret_visible = !self.caret_visible;
            self.last_caret_tick = now;
            self.dirty = true;
            return true;
        }
        false
    }

    // --- Tabs ---
}
