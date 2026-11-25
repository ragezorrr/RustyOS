//! VT100 / `xterm-256color` / 24-bit `TrueColor` terminal parser and cell grid (`RUSTYOS-113`).

use std::collections::VecDeque;

const MAX_SCROLLBACK_LINES: usize = 10_000;
const MAX_CSI_PARAMS: usize = 16;
const MAX_OSC_BYTES: usize = 512;

/// Color specification stored in each terminal cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TermColor {
    #[default]
    DefaultFg,
    DefaultBg,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Bitflags describing cell visual styles and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct CellAttrs {
    pub(crate) bold: bool,
    pub(crate) dim: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) inverse: bool,
    pub(crate) strike: bool,
}

/// Single character cell inside the terminal grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cell {
    pub(crate) ch: char,
    pub(crate) fg: TermColor,
    pub(crate) bg: TermColor,
    pub(crate) attrs: CellAttrs,
    pub(crate) hyperlink: u16,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: TermColor::DefaultFg,
            bg: TermColor::DefaultBg,
            attrs: CellAttrs::default(),
            hyperlink: 0,
        }
    }
}

/// Text selection coordinate measured in absolute history line (`0` = oldest scrollback row) and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GridPoint {
    pub(crate) abs_line: usize,
    pub(crate) col: usize,
}

/// Highlighted text selection range in absolute grid coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextSelection {
    pub(crate) anchor: GridPoint,
    pub(crate) active: GridPoint,
}

impl TextSelection {
    #[must_use]
    pub(crate) fn ordered(self) -> (GridPoint, GridPoint) {
        if self.anchor <= self.active {
            (self.anchor, self.active)
        } else {
            (self.active, self.anchor)
        }
    }

    #[must_use]
    pub(crate) fn contains(self, abs_line: usize, col: usize) -> bool {
        let (start, end) = self.ordered();
        if start == end {
            return false;
        }
        let p = GridPoint { abs_line, col };
        p >= start && p <= end
    }
}

#[derive(Debug, Clone)]
struct CursorState {
    col: usize,
    row: usize,
    fg: TermColor,
    bg: TermColor,
    attrs: CellAttrs,
    pending_wrap: bool,
    g0_line_drawing: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            fg: TermColor::DefaultFg,
            bg: TermColor::DefaultBg,
            attrs: CellAttrs::default(),
            pending_wrap: false,
            g0_line_drawing: false,
        }
    }
}

/// Visual cursor shape controlled via DECSCUSR (`CSI Ps SP q`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CursorStyle {
    #[default]
    Bar,
    Block,
    Underline,
}

/// Terminal mode flags controlled via DECSET/DECRST (`CSI ? ... h/l`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct TerminalModes {
    pub(crate) cursor_visible: bool,
    pub(crate) cursor_style: CursorStyle,
    pub(crate) app_cursor_keys: bool,
    pub(crate) alt_screen: bool,
    pub(crate) bracketed_paste: bool,
    pub(crate) focus_reporting: bool,
    pub(crate) mouse_clicks: bool,
    pub(crate) mouse_drag: bool,
    pub(crate) mouse_sgr: bool,
    pub(crate) synchronized_output: bool,
}

impl Default for TerminalModes {
    fn default() -> Self {
        Self {
            cursor_visible: true,
            cursor_style: CursorStyle::Bar,
            app_cursor_keys: false,
            alt_screen: false,
            bracketed_paste: false,
            focus_reporting: false,
            mouse_clicks: false,
            mouse_drag: false,
            mouse_sgr: false,
            synchronized_output: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    Ground,
    Escape,
    EscapeIntermediate(u8),
    CsiEntry,
    OscString,
    OscEsc,
}

#[derive(Debug, Clone)]
struct ScreenBuffer {
    cols: usize,
    rows: usize,
    lines: Vec<Vec<Cell>>,
    scrollback: VecDeque<Vec<Cell>>,
    scroll_top: usize,
    scroll_bottom: usize,
    cursor: CursorState,
    saved_cursor: CursorState,
}

impl ScreenBuffer {
    fn new(cols: usize, rows: usize, keep_scrollback: bool) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let blank_row = vec![Cell::default(); cols];
        Self {
            cols,
            rows,
            lines: vec![blank_row; rows],
            scrollback: VecDeque::with_capacity(if keep_scrollback { 256 } else { 0 }),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            cursor: CursorState::default(),
            saved_cursor: CursorState::default(),
        }
    }

    fn blank_row(&self) -> Vec<Cell> {
        vec![
            Cell {
                ch: ' ',
                fg: TermColor::DefaultFg,
                bg: self.cursor.bg,
                attrs: CellAttrs::default(),
                hyperlink: 0,
            };
            self.cols
        ]
    }

    fn resize(&mut self, new_cols: usize, new_rows: usize) {
        let new_cols = new_cols.max(1);
        let new_rows = new_rows.max(1);
        for row in &mut self.lines {
            row.resize(new_cols, Cell::default());
        }
        while self.lines.len() < new_rows {
            self.lines.push(vec![Cell::default(); new_cols]);
        }
        if self.lines.len() > new_rows {
            // First drop trailing empty/unused rows below the active cursor so initial
            // window configure or search-bar resize does not push blank rows into scrollback.
            while self.lines.len() > new_rows && self.lines.len() - 1 > self.cursor.row {
                let _ = self.lines.pop();
            }
            if self.lines.len() > new_rows {
                let extra = self.lines.len() - new_rows;
                for line in self.lines.drain(..extra) {
                    if self.scrollback.capacity() > 0 {
                        if self.scrollback.len() >= MAX_SCROLLBACK_LINES {
                            self.scrollback.pop_front();
                        }
                        self.scrollback.push_back(line);
                    }
                }
                self.cursor.row = self.cursor.row.saturating_sub(extra);
            }
        }
        self.cols = new_cols;
        self.rows = new_rows;
        self.scroll_top = 0;
        self.scroll_bottom = new_rows - 1;
        self.cursor.col = self.cursor.col.min(new_cols - 1);
        self.cursor.row = self.cursor.row.min(new_rows - 1);
        self.cursor.pending_wrap = false;
    }

    fn scroll_up(&mut self, top: usize, bottom: usize, count: usize, push_history: bool) {
        if top > bottom || bottom >= self.rows {
            return;
        }
        let span = bottom - top + 1;
        let steps = count.min(span);
        for _ in 0..steps {
            let removed = self.lines.remove(top);
            if push_history && top == 0 && bottom + 1 == self.rows {
                if self.scrollback.len() >= MAX_SCROLLBACK_LINES {
                    self.scrollback.pop_front();
                }
                self.scrollback.push_back(removed);
            }
            self.lines.insert(bottom, self.blank_row());
        }
    }

    fn scroll_down(&mut self, top: usize, bottom: usize, count: usize) {
        if top > bottom || bottom >= self.rows {
            return;
        }
        let span = bottom - top + 1;
        let steps = count.min(span);
        for _ in 0..steps {
            let _ = self.lines.remove(bottom);
            self.lines.insert(top, self.blank_row());
        }
    }
}

/// Complete terminal state machine, grids, scrollback, and metadata (`RUSTYOS-113`).
#[derive(Debug, Clone)]
pub(crate) struct TerminalEngine {
    primary: ScreenBuffer,
    alternate: ScreenBuffer,
    pub(crate) modes: TerminalModes,
    pub(crate) title: String,
    pub(crate) cwd: Option<String>,
    pub(crate) bell_pending: bool,
    parse_state: ParseState,
    csi_raw: Vec<u8>,
    osc_raw: Vec<u8>,
    utf8_buf: [u8; 4],
    utf8_len: usize,
    utf8_expected: usize,
    active_hyperlink: u16,
    pub(crate) hyperlinks: Vec<String>,
    csi_u_keyboard_stack: Vec<u8>,
}

impl TerminalEngine {
    #[must_use]
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
        Self {
            primary: ScreenBuffer::new(cols, rows, true),
            alternate: ScreenBuffer::new(cols, rows, false),
            modes: TerminalModes::default(),
            title: String::from("Terminal"),
            cwd: None,
            bell_pending: false,
            parse_state: ParseState::Ground,
            csi_raw: Vec::with_capacity(64),
            osc_raw: Vec::with_capacity(128),
            utf8_buf: [0; 4],
            utf8_len: 0,
            utf8_expected: 0,
            active_hyperlink: 0,
            hyperlinks: Vec::new(),
            csi_u_keyboard_stack: Vec::new(),
        }
    }

    /// Resolves an `OSC 8` hyperlink ID (`1..`) back to its URI.
    #[must_use]
    pub(crate) fn hyperlink_url(&self, id: u16) -> Option<&str> {
        id.checked_sub(1)
            .and_then(|idx| self.hyperlinks.get(usize::from(idx)))
            .map(String::as_str)
    }

    /// Active `CSI u` enhanced keyboard protocol flags (top of stack, `0` when disabled).
    #[must_use]
    pub(crate) fn csi_u_keyboard_flags(&self) -> u8 {
        self.csi_u_keyboard_stack.last().copied().unwrap_or(0)
    }

    #[must_use]
    pub(crate) const fn cols(&self) -> usize {
        self.primary.cols
    }

    #[must_use]
    pub(crate) const fn rows(&self) -> usize {
        self.primary.rows
    }

    #[must_use]
    pub(crate) fn scrollback_len(&self) -> usize {
        if self.modes.alt_screen {
            0
        } else {
            self.primary.scrollback.len()
        }
    }

    #[must_use]
    pub(crate) fn total_lines(&self) -> usize {
        self.scrollback_len() + self.rows()
    }

    #[must_use]
    pub(crate) fn cursor_pos(&self) -> (usize, usize) {
        let s = self.active_screen();
        (s.cursor.col, s.cursor.row)
    }

    #[must_use]
    pub(crate) fn row_cells(&self, abs_line: usize) -> Option<&[Cell]> {
        if self.modes.alt_screen {
            return self.alternate.lines.get(abs_line).map(Vec::as_slice);
        }
        let sb_len = self.primary.scrollback.len();
        if abs_line < sb_len {
            self.primary.scrollback.get(abs_line).map(Vec::as_slice)
        } else {
            self.primary.lines.get(abs_line - sb_len).map(Vec::as_slice)
        }
    }

    #[must_use]
    pub(crate) fn extract_selection_text(&self, sel: TextSelection) -> String {
        let (start, end) = sel.ordered();
        if start == end {
            return String::new();
        }
        let max_line = self.total_lines().saturating_sub(1);
        let first = start.abs_line.min(max_line);
        let last = end.abs_line.min(max_line);
        let mut out = String::new();
        for abs_line in first..=last {
            let Some(cells) = self.row_cells(abs_line) else {
                continue;
            };
            let c0 = if abs_line == first { start.col } else { 0 };
            let c1 = if abs_line == last {
                end.col.min(cells.len().saturating_sub(1))
            } else {
                cells.len().saturating_sub(1)
            };
            if c0 <= c1 && c0 < cells.len() {
                let mut line_str: String = cells[c0..=c1]
                    .iter()
                    .filter(|c| c.ch != '\0')
                    .map(|c| c.ch)
                    .collect();
                while line_str.ends_with(' ') {
                    line_str.pop();
                }
                out.push_str(&line_str);
            }
            if abs_line < last {
                out.push('\n');
            }
        }
        out
    }

    #[must_use]
    pub(crate) fn search_scrollback(&self, query: &str) -> Vec<GridPoint> {
        let needle = query.trim();
        if needle.is_empty() {
            return Vec::new();
        }
        let needle_lower = needle.to_lowercase();
        let total = self.total_lines();
        let mut matches = Vec::new();
        for abs_line in 0..total {
            let Some(cells) = self.row_cells(abs_line) else {
                continue;
            };
            let mut lower = String::with_capacity(cells.len());
            let mut byte_to_col: Vec<usize> = Vec::with_capacity(cells.len());
            for (col, cell) in cells.iter().enumerate() {
                if cell.ch == '\0' {
                    continue;
                }
                for lc in cell.ch.to_lowercase() {
                    let start_len = lower.len();
                    lower.push(lc);
                    byte_to_col.resize(lower.len(), col);
                    debug_assert!(lower.len() > start_len);
                }
            }
            let mut offset = 0;
            while let Some(pos) = lower[offset..].find(&needle_lower) {
                let byte_idx = offset + pos;
                let col = byte_to_col.get(byte_idx).copied().unwrap_or(0);
                matches.push(GridPoint { abs_line, col });
                offset = byte_idx + needle_lower.len().max(1);
                if matches.len() >= 2_000 {
                    return matches;
                }
            }
        }
        matches
    }

    pub(crate) fn clear_scrollback_and_reset_view(&mut self) {
        if !self.modes.alt_screen {
            self.primary.scrollback.clear();
        }
        let screen = self.active_screen_mut();
        for row in &mut screen.lines {
            row.fill(Cell::default());
        }
        screen.cursor.col = 0;
        screen.cursor.row = 0;
        screen.cursor.pending_wrap = false;
    }

    pub(crate) fn resize(&mut self, cols: usize, rows: usize) {
        self.primary.resize(cols, rows);
        self.alternate.resize(cols, rows);
    }

    /// Feeds raw PTY output bytes into the VT parser and returns any terminal responses (e.g. DSR/DA1)
    /// that must be written back to the PTY.
    pub(crate) fn process_bytes(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut replies = Vec::new();
        for &b in bytes {
            self.step_byte(b, &mut replies);
        }
        replies
    }

    fn active_screen(&self) -> &ScreenBuffer {
        if self.modes.alt_screen {
            &self.alternate
        } else {
            &self.primary
        }
    }

    fn active_screen_mut(&mut self) -> &mut ScreenBuffer {
        if self.modes.alt_screen {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    fn step_byte(&mut self, b: u8, replies: &mut Vec<Vec<u8>>) {
        match self.parse_state {
            ParseState::Ground => self.step_ground(b),
            ParseState::Escape => self.step_escape(b),
            ParseState::EscapeIntermediate(inter) => {
                if inter == b'(' {
                    self.active_screen_mut().cursor.g0_line_drawing = b == b'0';
                }
                self.parse_state = ParseState::Ground;
            }
            ParseState::CsiEntry => {
                if (0x40..=0x7E).contains(&b) {
                    let seq = std::mem::take(&mut self.csi_raw);
                    self.parse_state = ParseState::Ground;
                    self.dispatch_csi(&seq, b, replies);
                } else if b == 0x1B {
                    self.csi_raw.clear();
                    self.parse_state = ParseState::Escape;
                } else if self.csi_raw.len() < 128 {
                    if b == 0x08 || b == 0x0D || b == 0x0A {
                        self.handle_control(b);
                    } else {
                        self.csi_raw.push(b);
                    }
                }
            }
            ParseState::OscString => {
                if b == 0x07 {
                    self.finish_osc();
                } else if b == 0x1B {
                    self.parse_state = ParseState::OscEsc;
                } else if self.osc_raw.len() < MAX_OSC_BYTES {
                    self.osc_raw.push(b);
                }
            }
            ParseState::OscEsc => {
                if b == b'\\' {
                    self.finish_osc();
                } else {
                    self.osc_raw.clear();
                    self.parse_state = ParseState::Ground;
                    self.step_escape(b);
                }
            }
        }
    }

    fn step_ground(&mut self, b: u8) {
        if self.utf8_expected > 0 {
            if (b & 0xC0) == 0x80 {
                self.utf8_buf[self.utf8_len] = b;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_expected {
                    let s =
                        std::str::from_utf8(&self.utf8_buf[..self.utf8_len]).unwrap_or("\u{FFFD}");
                    let ch = s.chars().next().unwrap_or('\u{FFFD}');
                    self.utf8_len = 0;
                    self.utf8_expected = 0;
                    self.put_char(ch);
                }
                return;
            }
            self.utf8_len = 0;
            self.utf8_expected = 0;
        }

        if b < 0x20 || b == 0x7F {
            self.handle_control(b);
            return;
        }
        if b < 0x80 {
            let mapped = if self.active_screen().cursor.g0_line_drawing {
                map_dec_special_graphics(b as char)
            } else {
                b as char
            };
            self.put_char(mapped);
            return;
        }

        let expected = if (b & 0xE0) == 0xC0 {
            2
        } else if (b & 0xF0) == 0xE0 {
            3
        } else if (b & 0xF8) == 0xF0 {
            4
        } else {
            self.put_char('\u{FFFD}');
            return;
        };
        self.utf8_buf[0] = b;
        self.utf8_len = 1;
        self.utf8_expected = expected;
    }

    fn handle_control(&mut self, b: u8) {
        match b {
            0x07 => self.bell_pending = true,
            0x08 => {
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.col = s.cursor.col.saturating_sub(1);
            }
            0x09 => {
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                let next_tab = ((s.cursor.col / 8) + 1) * 8;
                s.cursor.col = next_tab.min(s.cols - 1);
            }
            0x0A..=0x0C => self.line_feed(),
            0x0D => {
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.col = 0;
            }
            0x1B => {
                self.parse_state = ParseState::Escape;
            }
            _ => {}
        }
    }

    fn line_feed(&mut self) {
        let keep_sb = !self.modes.alt_screen;
        let s = self.active_screen_mut();
        s.cursor.pending_wrap = false;
        if s.cursor.row == s.scroll_bottom {
            s.scroll_up(s.scroll_top, s.scroll_bottom, 1, keep_sb);
        } else if s.cursor.row + 1 < s.rows {
            s.cursor.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        let s = self.active_screen_mut();
        s.cursor.pending_wrap = false;
        if s.cursor.row == s.scroll_top {
            s.scroll_down(s.scroll_top, s.scroll_bottom, 1);
        } else {
            s.cursor.row = s.cursor.row.saturating_sub(1);
        }
    }

    fn put_char(&mut self, ch: char) {
        let width = char_display_width(ch);
        if width == 0 {
            return;
        }
        let keep_sb = !self.modes.alt_screen;
        let hyperlink = self.active_hyperlink;
        let s = self.active_screen_mut();
        if s.cursor.pending_wrap || (width == 2 && s.cursor.col + 1 >= s.cols && s.cols >= 2) {
            s.cursor.pending_wrap = false;
            s.cursor.col = 0;
            if s.cursor.row == s.scroll_bottom {
                s.scroll_up(s.scroll_top, s.scroll_bottom, 1, keep_sb);
            } else if s.cursor.row + 1 < s.rows {
                s.cursor.row += 1;
            }
        }
        let r = s.cursor.row.min(s.rows - 1);
        let c = s.cursor.col.min(s.cols - 1);
        s.lines[r][c] = Cell {
            ch,
            fg: s.cursor.fg,
            bg: s.cursor.bg,
            attrs: s.cursor.attrs,
            hyperlink,
        };
        if width == 2 && c + 1 < s.cols {
            s.lines[r][c + 1] = Cell {
                ch: '\0',
                fg: s.cursor.fg,
                bg: s.cursor.bg,
                attrs: s.cursor.attrs,
                hyperlink,
            };
        }
        if s.cursor.col + width >= s.cols {
            s.cursor.col = s.cols.saturating_sub(1);
            s.cursor.pending_wrap = true;
        } else {
            s.cursor.col += width;
        }
    }

    fn step_escape(&mut self, b: u8) {
        self.parse_state = ParseState::Ground;
        match b {
            b'[' => {
                self.csi_raw.clear();
                self.parse_state = ParseState::CsiEntry;
            }
            b']' => {
                self.osc_raw.clear();
                self.parse_state = ParseState::OscString;
            }
            b'(' | b')' | b'*' | b'+' => {
                self.parse_state = ParseState::EscapeIntermediate(b);
            }
            b'7' => {
                let s = self.active_screen_mut();
                s.saved_cursor = s.cursor.clone();
            }
            b'8' => {
                let s = self.active_screen_mut();
                s.cursor = s.saved_cursor.clone();
                s.cursor.col = s.cursor.col.min(s.cols - 1);
                s.cursor.row = s.cursor.row.min(s.rows - 1);
            }
            b'D' => self.line_feed(),
            b'E' => {
                self.active_screen_mut().cursor.col = 0;
                self.line_feed();
            }
            b'M' => self.reverse_index(),
            b'c' => {
                let cols = self.cols();
                let rows = self.rows();
                *self = Self::new(cols, rows);
            }
            _ => {}
        }
    }

    fn finish_osc(&mut self) {
        let raw = std::mem::take(&mut self.osc_raw);
        self.parse_state = ParseState::Ground;
        let text = String::from_utf8_lossy(&raw);
        let Some((code_str, payload)) = text.split_once(';') else {
            return;
        };
        match code_str {
            "0" | "2" => {
                let clean = payload.trim();
                if !clean.is_empty() {
                    clean.clone_into(&mut self.title);
                }
            }
            "7" => {
                if let Some(rest) = payload.trim().strip_prefix("file://")
                    && let Some((_, path)) = rest.split_once('/')
                {
                    let decoded = decode_percent_uri_path(&format!("/{path}"));
                    self.cwd = Some(decoded);
                }
            }
            "8" => {
                if let Some((_, uri_raw)) = payload.split_once(';') {
                    let uri = uri_raw.trim();
                    if uri.is_empty() {
                        self.active_hyperlink = 0;
                    } else if let Some(pos) = self.hyperlinks.iter().position(|u| u == uri) {
                        self.active_hyperlink = u16::try_from(pos + 1).unwrap_or(0);
                    } else if self.hyperlinks.len() < usize::from(u16::MAX - 1) {
                        self.hyperlinks.push(uri.to_owned());
                        self.active_hyperlink = u16::try_from(self.hyperlinks.len()).unwrap_or(0);
                    }
                }
            }
            _ => {}
        }
    }

    fn dispatch_csi(&mut self, seq: &[u8], final_byte: u8, replies: &mut Vec<Vec<u8>>) {
        let private = seq
            .first()
            .copied()
            .filter(|&b| matches!(b, b'?' | b'>' | b'<' | b'='));
        let body = if private.is_some() { &seq[1..] } else { seq };
        let params = parse_csi_params(body);
        if final_byte == b'u' && private.is_some() {
            self.dispatch_csi_u_keyboard(private, &params, replies);
            return;
        }
        if private == Some(b'?') {
            self.dispatch_dec_private(&params, final_byte);
            return;
        }
        if private.is_some() {
            return;
        }
        // DECSCUSR (`CSI Ps SP q`): cursor shape selection.
        if final_byte == b'q' && body.ends_with(b" ") {
            let p0 = parse_csi_params(&body[..body.len() - 1])
                .first()
                .copied()
                .unwrap_or(0);
            self.modes.cursor_style = match p0 {
                1 | 2 => CursorStyle::Block,
                3 | 4 => CursorStyle::Underline,
                _ => CursorStyle::Bar,
            };
            return;
        }
        let p0 = params.first().copied().unwrap_or(0);
        if self.dispatch_csi_cursor(&params, p0, final_byte) {
            return;
        }
        self.dispatch_csi_edit(&params, p0, body, final_byte, replies);
    }

    fn dispatch_csi_cursor(&mut self, params: &[u16], p0: u16, final_byte: u8) -> bool {
        match final_byte {
            b'A' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.row = s.cursor.row.saturating_sub(n).max(s.scroll_top);
            }
            b'B' | b'e' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.row = (s.cursor.row + n).min(s.scroll_bottom);
            }
            b'C' | b'a' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.col = (s.cursor.col + n).min(s.cols - 1);
            }
            b'D' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.col = s.cursor.col.saturating_sub(n);
            }
            b'E' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.col = 0;
                s.cursor.row = (s.cursor.row + n).min(s.scroll_bottom);
            }
            b'F' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.col = 0;
                s.cursor.row = s.cursor.row.saturating_sub(n).max(s.scroll_top);
            }
            b'G' | b'`' => {
                let col = p0.max(1).saturating_sub(1) as usize;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.col = col.min(s.cols - 1);
            }
            b'd' => {
                let row = p0.max(1).saturating_sub(1) as usize;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.row = row.min(s.rows - 1);
            }
            b'H' | b'f' => {
                let row = params.first().copied().unwrap_or(1).max(1) as usize - 1;
                let col = params.get(1).copied().unwrap_or(1).max(1) as usize - 1;
                let s = self.active_screen_mut();
                s.cursor.pending_wrap = false;
                s.cursor.row = row.min(s.rows - 1);
                s.cursor.col = col.min(s.cols - 1);
            }
            b's' => {
                let s = self.active_screen_mut();
                s.saved_cursor = s.cursor.clone();
            }
            b'u' => {
                let s = self.active_screen_mut();
                s.cursor = s.saved_cursor.clone();
                s.cursor.col = s.cursor.col.min(s.cols - 1);
                s.cursor.row = s.cursor.row.min(s.rows - 1);
            }
            _ => return false,
        }
        true
    }

    fn dispatch_csi_edit(
        &mut self,
        params: &[u16],
        p0: u16,
        body: &[u8],
        final_byte: u8,
        replies: &mut Vec<Vec<u8>>,
    ) {
        match final_byte {
            b'J' => self.erase_in_display(p0),
            b'K' => self.erase_in_line(p0),
            b'X' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                let r = s.cursor.row;
                let blank = Cell {
                    bg: s.cursor.bg,
                    ..Cell::default()
                };
                for c in s.cursor.col..(s.cursor.col + n).min(s.cols) {
                    s.lines[r][c] = blank;
                }
            }
            b'L' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                if s.cursor.row >= s.scroll_top && s.cursor.row <= s.scroll_bottom {
                    s.scroll_down(s.cursor.row, s.scroll_bottom, n);
                }
            }
            b'M' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                if s.cursor.row >= s.scroll_top && s.cursor.row <= s.scroll_bottom {
                    s.scroll_up(s.cursor.row, s.scroll_bottom, n, false);
                }
            }
            b'P' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                let r = s.cursor.row;
                let c = s.cursor.col;
                let del = n.min(s.cols - c);
                s.lines[r].drain(c..c + del);
                s.lines[r].resize(s.cols, Cell::default());
            }
            b'@' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                let r = s.cursor.row;
                let c = s.cursor.col;
                let blank = Cell {
                    bg: s.cursor.bg,
                    ..Cell::default()
                };
                for _ in 0..n.min(s.cols - c) {
                    s.lines[r].insert(c, blank);
                }
                s.lines[r].truncate(s.cols);
            }
            b'S' => {
                let n = p0.max(1) as usize;
                let keep_sb = !self.modes.alt_screen;
                let s = self.active_screen_mut();
                s.scroll_up(s.scroll_top, s.scroll_bottom, n, keep_sb);
            }
            b'T' => {
                let n = p0.max(1) as usize;
                let s = self.active_screen_mut();
                s.scroll_down(s.scroll_top, s.scroll_bottom, n);
            }
            b'r' => {
                let s = self.active_screen_mut();
                let top = params.first().copied().unwrap_or(1).max(1) as usize - 1;
                let bot_raw = params.get(1).copied().unwrap_or(0) as usize;
                let bot = if bot_raw == 0 {
                    s.rows - 1
                } else {
                    (bot_raw - 1).min(s.rows - 1)
                };
                if top < bot {
                    s.scroll_top = top;
                    s.scroll_bottom = bot;
                } else {
                    s.scroll_top = 0;
                    s.scroll_bottom = s.rows - 1;
                }
                s.cursor.col = 0;
                s.cursor.row = 0;
                s.cursor.pending_wrap = false;
            }
            b'm' => self.apply_sgr(body),
            b'n' => {
                let s = self.active_screen();
                if p0 == 5 {
                    replies.push(b"\x1b[0n".to_vec());
                } else if p0 == 6 {
                    let resp = format!("\x1b[{};{}R", s.cursor.row + 1, s.cursor.col + 1);
                    replies.push(resp.into_bytes());
                }
            }
            b'c' if p0 == 0 => replies.push(b"\x1b[?62;22c".to_vec()),
            _ => {}
        }
    }

    fn dispatch_csi_u_keyboard(
        &mut self,
        prefix: Option<u8>,
        params: &[u16],
        replies: &mut Vec<Vec<u8>>,
    ) {
        match prefix {
            Some(b'?') => {
                let flags = self.csi_u_keyboard_flags();
                replies.push(format!("\x1b[?{flags}u").into_bytes());
            }
            Some(b'>') => {
                let flags = u8::try_from(params.first().copied().unwrap_or(0)).unwrap_or(0);
                if self.csi_u_keyboard_stack.len() >= 16 {
                    self.csi_u_keyboard_stack.remove(0);
                }
                self.csi_u_keyboard_stack.push(flags);
            }
            Some(b'<') => {
                let count = usize::from(params.first().copied().unwrap_or(1).max(1));
                let new_len = self.csi_u_keyboard_stack.len().saturating_sub(count);
                self.csi_u_keyboard_stack.truncate(new_len);
            }
            Some(b'=') => {
                let flags = u8::try_from(params.first().copied().unwrap_or(0)).unwrap_or(0);
                let mode = params.get(1).copied().unwrap_or(1);
                let cur = self.csi_u_keyboard_flags();
                let next = match mode {
                    2 => cur | flags,
                    3 => cur & !flags,
                    _ => flags,
                };
                if let Some(top) = self.csi_u_keyboard_stack.last_mut() {
                    *top = next;
                } else {
                    self.csi_u_keyboard_stack.push(next);
                }
            }
            _ => {}
        }
    }

    fn dispatch_dec_private(&mut self, params: &[u16], final_byte: u8) {
        let enable = match final_byte {
            b'h' => true,
            b'l' => false,
            _ => return,
        };
        for &p in params {
            match p {
                1 => self.modes.app_cursor_keys = enable,
                25 => self.modes.cursor_visible = enable,
                47 | 1047 => self.switch_alt_screen(enable, false),
                1049 => self.switch_alt_screen(enable, true),
                1000 => self.modes.mouse_clicks = enable,
                1002 | 1003 => self.modes.mouse_drag = enable,
                1004 => self.modes.focus_reporting = enable,
                1006 => self.modes.mouse_sgr = enable,
                2004 => self.modes.bracketed_paste = enable,
                2026 => self.modes.synchronized_output = enable,
                _ => {}
            }
        }
    }

    fn switch_alt_screen(&mut self, enable: bool, save_restore: bool) {
        if self.modes.alt_screen == enable {
            return;
        }
        if enable {
            if save_restore {
                self.primary.saved_cursor = self.primary.cursor.clone();
            }
            let cols = self.primary.cols;
            let rows = self.primary.rows;
            self.alternate = ScreenBuffer::new(cols, rows, false);
            self.modes.alt_screen = true;
        } else {
            self.modes.alt_screen = false;
            if save_restore {
                self.primary.cursor = self.primary.saved_cursor.clone();
                self.primary.cursor.col = self.primary.cursor.col.min(self.primary.cols - 1);
                self.primary.cursor.row = self.primary.cursor.row.min(self.primary.rows - 1);
            }
        }
    }

    fn erase_in_display(&mut self, mode: u16) {
        let s = self.active_screen_mut();
        let blank = Cell {
            bg: s.cursor.bg,
            ..Cell::default()
        };
        match mode {
            0 => {
                let r = s.cursor.row;
                for c in s.cursor.col..s.cols {
                    s.lines[r][c] = blank;
                }
                for line in &mut s.lines[r + 1..] {
                    line.fill(blank);
                }
            }
            1 => {
                let r = s.cursor.row;
                for line in &mut s.lines[..r] {
                    line.fill(blank);
                }
                for c in 0..=s.cursor.col.min(s.cols - 1) {
                    s.lines[r][c] = blank;
                }
            }
            2 => {
                for line in &mut s.lines {
                    line.fill(blank);
                }
            }
            3 => {
                for line in &mut s.lines {
                    line.fill(blank);
                }
                if !self.modes.alt_screen {
                    self.primary.scrollback.clear();
                }
            }
            _ => {}
        }
    }

    fn erase_in_line(&mut self, mode: u16) {
        let s = self.active_screen_mut();
        let blank = Cell {
            bg: s.cursor.bg,
            ..Cell::default()
        };
        let r = s.cursor.row;
        match mode {
            0 => {
                for c in s.cursor.col..s.cols {
                    s.lines[r][c] = blank;
                }
            }
            1 => {
                for c in 0..=s.cursor.col.min(s.cols - 1) {
                    s.lines[r][c] = blank;
                }
            }
            2 => s.lines[r].fill(blank),
            _ => {}
        }
    }

    fn apply_sgr(&mut self, raw: &[u8]) {
        let text = std::str::from_utf8(raw).unwrap_or("");
        if text.is_empty() {
            let c = &mut self.active_screen_mut().cursor;
            c.fg = TermColor::DefaultFg;
            c.bg = TermColor::DefaultBg;
            c.attrs = CellAttrs::default();
            return;
        }
        let tokens: Vec<u16> = text
            .split([';', ':'])
            .map(|part| part.parse::<u16>().unwrap_or(0))
            .collect();
        let c = &mut self.active_screen_mut().cursor;
        let mut i = 0;
        while i < tokens.len() {
            match tokens[i] {
                0 => {
                    c.fg = TermColor::DefaultFg;
                    c.bg = TermColor::DefaultBg;
                    c.attrs = CellAttrs::default();
                }
                1 => c.attrs.bold = true,
                2 => c.attrs.dim = true,
                3 => c.attrs.italic = true,
                4 => c.attrs.underline = true,
                7 => c.attrs.inverse = true,
                9 => c.attrs.strike = true,
                21 | 22 => {
                    c.attrs.bold = false;
                    c.attrs.dim = false;
                }
                23 => c.attrs.italic = false,
                24 => c.attrs.underline = false,
                27 => c.attrs.inverse = false,
                29 => c.attrs.strike = false,
                30..=37 => c.fg = TermColor::Indexed((tokens[i] - 30) as u8),
                39 => c.fg = TermColor::DefaultFg,
                40..=47 => c.bg = TermColor::Indexed((tokens[i] - 40) as u8),
                49 => c.bg = TermColor::DefaultBg,
                90..=97 => c.fg = TermColor::Indexed((tokens[i] - 90 + 8) as u8),
                100..=107 => c.bg = TermColor::Indexed((tokens[i] - 100 + 8) as u8),
                38 | 48 => {
                    let is_fg = tokens[i] == 38;
                    if let Some((color, consumed)) = parse_extended_sgr_color(&tokens[i + 1..]) {
                        if is_fg {
                            c.fg = color;
                        } else {
                            c.bg = color;
                        }
                        i += consumed;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

fn parse_csi_params(body: &[u8]) -> Vec<u16> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(4);
    let mut cur: u16 = 0;
    let mut has_digit = false;
    for &b in body {
        if b.is_ascii_digit() {
            has_digit = true;
            cur = cur.saturating_mul(10).saturating_add(u16::from(b - b'0'));
        } else if b == b';' {
            out.push(if has_digit { cur } else { 0 });
            cur = 0;
            has_digit = false;
            if out.len() >= MAX_CSI_PARAMS {
                break;
            }
        }
    }
    if out.len() < MAX_CSI_PARAMS {
        out.push(if has_digit { cur } else { 0 });
    }
    out
}

fn parse_extended_sgr_color(rest: &[u16]) -> Option<(TermColor, usize)> {
    let mode = *rest.first()?;
    if mode == 5 {
        let idx = (*rest.get(1)?).min(255) as u8;
        return Some((TermColor::Indexed(idx), 2));
    }
    if mode == 2 {
        // Supports both `38;2;R;G;B` and ITU colon format `38:2::R:G:B`.
        if rest.len() >= 5 && rest[1] == 0 {
            let r = rest[2].min(255) as u8;
            let g = rest[3].min(255) as u8;
            let b = rest[4].min(255) as u8;
            return Some((TermColor::Rgb(r, g, b), 5));
        }
        if rest.len() >= 4 {
            let r = rest[1].min(255) as u8;
            let g = rest[2].min(255) as u8;
            let b = rest[3].min(255) as u8;
            return Some((TermColor::Rgb(r, g, b), 4));
        }
    }
    None
}

/// Built-in terminal display width (`0`, `1`, or `2` columns) without external crates (`RUSTYOS-169`).
#[must_use]
pub(crate) fn char_display_width(ch: char) -> usize {
    let cp = ch as u32;
    if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
        return 0;
    }
    if matches!(
        cp,
        0x0300..=0x036F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0x20D0..=0x20FF
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
            | 0xFEFF
            | 0xE0100..=0xE01EF
    ) {
        return 0;
    }
    if matches!(
        cp,
        0x1100..=0x115F
            | 0x2329..=0x232A
            | 0x2E80..=0x303E
            | 0x3040..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
            | 0x20000..=0x3FFFD
    ) {
        return 2;
    }
    1
}

/// Percent-decodes an `OSC 7` `file://` URI path (`%20`, UTF-8 sequences).
#[must_use]
pub(crate) fn decode_percent_uri_path(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

const fn map_dec_special_graphics(ch: char) -> char {
    match ch {
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'q' => '─',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'a' => '▒',
        '`' => '◆',
        '~' => '·',
        _ => ch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utf8_truecolor_osc_title_and_scrollback() -> Result<(), &'static str> {
        let mut vt = TerminalEngine::new(10, 3);
        let replies = vt.process_bytes(
            b"\x1b]2;anc@rustyos: ~\x07\x1b[38;2;10;132;255;1m\xD0\x9F\xD1\x80\xD0\xB8\xD0\xB2\xD0\xB5\xD1\x82\x1b[0m\r\nline2\r\nline3\r\nline4\x1b[6n",
        );
        assert_eq!(vt.title, "anc@rustyos: ~");
        assert_eq!(replies, vec![b"\x1b[3;6R".to_vec()]);
        assert_eq!(vt.scrollback_len(), 1);
        let sb_row = vt.row_cells(0).ok_or("scrollback row 0 missing")?;
        assert_eq!(sb_row[0].ch, 'П');
        assert_eq!(sb_row[0].fg, TermColor::Rgb(10, 132, 255));
        assert!(sb_row[0].attrs.bold);
        Ok(())
    }

    #[test]
    fn handles_alt_screen_and_decstbm_scroll_regions() -> Result<(), &'static str> {
        let mut vt = TerminalEngine::new(8, 4);
        let _ = vt.process_bytes(b"main\x1b[?1049h\x1b[2;3r\x1b[2;1Halt1\r\nalt2\r\nalt3");
        assert!(vt.modes.alt_screen);
        assert_eq!(vt.row_cells(1).ok_or("r1 missing")?[0].ch, 'a');
        let _ = vt.process_bytes(b"\x1b[?1049l");
        assert!(!vt.modes.alt_screen);
        assert_eq!(
            vt.row_cells(0).ok_or("primary restored missing")?[0].ch,
            'm'
        );
        Ok(())
    }

    #[test]
    fn selection_and_search_extract_clean_lines() {
        let mut vt = TerminalEngine::new(16, 4);
        let _ = vt.process_bytes(b"hello world\r\nsecond line");
        let hits = vt.search_scrollback("WORLD");
        assert_eq!(
            hits,
            vec![GridPoint {
                abs_line: 0,
                col: 6
            }]
        );
        let sel = TextSelection {
            anchor: GridPoint {
                abs_line: 0,
                col: 6,
            },
            active: GridPoint {
                abs_line: 1,
                col: 5,
            },
        };
        assert_eq!(vt.extract_selection_text(sel), "world\nsecond");
    }

    #[test]
    fn shrink_drops_trailing_blank_rows_without_spurious_scrollback_and_parses_cursor_style()
    -> Result<(), &'static str> {
        let mut vt = TerminalEngine::new(80, 24);
        assert_eq!(vt.modes.cursor_style, CursorStyle::Bar);
        let _ = vt.process_bytes(b"anc@rustyos ~ $ ");
        vt.resize(80, 19);
        assert_eq!(vt.scrollback_len(), 0);
        assert_eq!(vt.cursor_pos(), (16, 0));
        assert_eq!(vt.row_cells(0).ok_or("row 0 missing")?[0].ch, 'a');

        let _ = vt.process_bytes(b"\x1b[2 q");
        assert_eq!(vt.modes.cursor_style, CursorStyle::Block);
        let _ = vt.process_bytes(b"\x1b[4 q");
        assert_eq!(vt.modes.cursor_style, CursorStyle::Underline);
        let _ = vt.process_bytes(b"\x1b[6 q");
        assert_eq!(vt.modes.cursor_style, CursorStyle::Bar);
        Ok(())
    }

    #[test]
    fn handles_wide_chars_combining_marks_osc7_percent_decode_and_unicode_search()
    -> Result<(), &'static str> {
        assert_eq!(char_display_width('a'), 1);
        assert_eq!(char_display_width('\u{0301}'), 0);
        assert_eq!(char_display_width('中'), 2);
        assert_eq!(char_display_width('🚀'), 2);

        let mut vt = TerminalEngine::new(20, 4);
        let _ = vt.process_bytes("a\u{0301}中x\r\nПривет Мир".as_bytes());
        let row0 = vt.row_cells(0).ok_or("row 0 missing")?;
        assert_eq!(row0[0].ch, 'a');
        assert_eq!(row0[1].ch, '中');
        assert_eq!(row0[2].ch, '\0');
        assert_eq!(row0[3].ch, 'x');

        let matches = vt.search_scrollback("привет");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].abs_line, 1);
        assert_eq!(matches[0].col, 0);

        let _ = vt.process_bytes(
            b"\x1b]7;file://host/home/user/My%20Docs/%D0%9F%D1%80%D0%BE%D0%B5%D0%BA%D1%82\x07",
        );
        assert_eq!(vt.cwd.as_deref(), Some("/home/user/My Docs/Проект"));
        Ok(())
    }
}
