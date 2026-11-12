//! Deterministic window management model and policies for `RustyOS` (`RUSTYOS-004`).
//!
//! This crate separates high-level window management policy (outputs, workspaces,
//! stacking order, focus tracking, floating placement, snap targets, and master-stack
//! tiling) from low-level Wayland protocol and GPU rendering details.

/// Strongly-typed identifier for a display output (monitor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputId(pub u32);

/// Strongly-typed identifier for a virtual workspace (`1..=9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(pub u8);

impl WorkspaceId {
    /// Minimum valid 1-based workspace index.
    pub const MIN: u8 = 1;
    /// Maximum valid 1-based workspace index.
    pub const MAX: u8 = 9;

    /// Creates a validated [`WorkspaceId`] clamped to `1..=9`.
    #[must_use]
    pub const fn new_clamped(index: u8) -> Self {
        if index < Self::MIN {
            Self(Self::MIN)
        } else if index > Self::MAX {
            Self(Self::MAX)
        } else {
            Self(index)
        }
    }
}

/// Strongly-typed identifier for a managed window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// Logical 2D rectangle representing window or output work-area geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// Logical X coordinate.
    pub x: i32,
    /// Logical Y coordinate.
    pub y: i32,
    /// Logical width in pixels (always `>= 1` for valid windows/outputs).
    pub width: i32,
    /// Logical height in pixels (always `>= 1` for valid windows/outputs).
    pub height: i32,
}

impl Rect {
    /// Creates a new logical rectangle.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns `true` if the given logical coordinate lies inside this rectangle.
    #[must_use]
    pub const fn contains_point(self, px: i32, py: i32) -> bool {
        px >= self.x && px < (self.x + self.width) && py >= self.y && py < (self.y + self.height)
    }
}

/// Layout mode applied to a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutMode {
    /// macOS-like floating desktop with cascading initial placement and quick snap helpers.
    #[default]
    Floating,
    /// Deterministic master-stack tiling layout.
    MasterStack,
}

/// Quick window snap targets within the active output work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapTarget {
    /// Left 50% of the work area.
    LeftHalf,
    /// Right 50% of the work area.
    RightHalf,
    /// Centered 70% of the work area.
    Center,
}

/// High-level window management actions triggered by keyboard shortcuts or shell UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmAction {
    /// Cycle focus to the next window in the active workspace (`Alt+Tab` / `Alt+J`).
    FocusNext,
    /// Cycle focus to the previous window in the active workspace (`Alt+Shift+Tab` / `Alt+K`).
    FocusPrev,
    /// Request closing the currently focused window (`Alt+W` / `Alt+Q`).
    CloseFocused,
    /// Switch the focused output to the given workspace (`Alt+1..9`).
    SwitchWorkspace(WorkspaceId),
    /// Move the focused window to the given workspace (`Alt+Shift+1..9`).
    MoveFocusedToWorkspace(WorkspaceId),
    /// Move the focused window to the next connected output (`Alt+O`).
    MoveFocusedToNextOutput,
    /// Toggle between [`LayoutMode::Floating`] and [`LayoutMode::MasterStack`] (`Alt+L`).
    ToggleLayoutMode,
    /// Toggle maximization of the focused window (`Alt+M`).
    ToggleMaximizeFocused,
    /// Snap the focused window to a specific region (`Alt+Left`, `Alt+Right`, `Alt+C`).
    SnapFocused(SnapTarget),
}

/// State of a managed window inside [`WindowManager`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWindow {
    /// Unique identifier of the window.
    pub id: WindowId,
    /// Workspace to which the window currently belongs.
    pub workspace: WorkspaceId,
    /// Current effective logical geometry (either floating, snapped, maximized, or tiled).
    pub geometry: Rect,
    /// Saved floating geometry restored when leaving maximized or tiled states.
    pub saved_floating_geometry: Rect,
    /// Whether the window is currently maximized on its workspace.
    pub maximized: bool,
}

/// State of a display output managed by [`WindowManager`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedOutput {
    /// Unique identifier of the output.
    pub id: OutputId,
    /// Usable logical work area of the output.
    pub work_area: Rect,
    /// Workspace currently displayed on this output.
    pub active_workspace: WorkspaceId,
}

/// State of a virtual workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// Identifier of the workspace (`1..=9`).
    pub id: WorkspaceId,
    /// Active layout policy for this workspace.
    pub layout: LayoutMode,
    /// Windows mapped on this workspace in bottom-to-top stacking (`z-order`).
    /// The last element is the topmost window.
    pub stacking: Vec<WindowId>,
    /// Currently focused window on this workspace, if any.
    pub focused: Option<WindowId>,
}

impl Workspace {
    #[must_use]
    const fn new(id: WorkspaceId) -> Self {
        Self {
            id,
            layout: LayoutMode::Floating,
            stacking: Vec::new(),
            focused: None,
        }
    }
}

/// Deterministic window manager state machine.
#[derive(Debug, Clone)]
pub struct WindowManager {
    outputs: Vec<ManagedOutput>,
    focused_output: OutputId,
    workspaces: Vec<Workspace>,
    windows: Vec<ManagedWindow>,
    next_window_raw_id: u64,
    /// Outer and inner gap in logical pixels for tiled/snapped windows (from `DESIGN.md` tokens).
    pub gap_px: i32,
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new(Rect::new(0, 0, 1280, 800))
    }
}

impl WindowManager {
    /// Creates a new [`WindowManager`] with a primary output (`OutputId(1)`) and 9 workspaces.
    #[must_use]
    pub fn new(primary_work_area: Rect) -> Self {
        let primary_id = OutputId(1);
        let outputs = vec![ManagedOutput {
            id: primary_id,
            work_area: sanitize_rect(primary_work_area),
            active_workspace: WorkspaceId(1),
        }];

        let workspaces = (WorkspaceId::MIN..=WorkspaceId::MAX)
            .map(|idx| Workspace::new(WorkspaceId(idx)))
            .collect();

        Self {
            outputs,
            focused_output: primary_id,
            workspaces,
            windows: Vec::new(),
            next_window_raw_id: 1,
            gap_px: 12,
        }
    }

    /// Allocates a new monotonically increasing [`WindowId`].
    pub const fn next_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_raw_id);
        self.next_window_raw_id = self.next_window_raw_id.saturating_add(1);
        id
    }

    /// Returns the currently focused [`OutputId`].
    #[must_use]
    pub const fn focused_output(&self) -> OutputId {
        self.focused_output
    }

    /// Sets the focused output if it exists.
    pub fn set_focused_output(&mut self, output_id: OutputId) -> bool {
        if self.outputs.iter().any(|o| o.id == output_id) {
            self.focused_output = output_id;
            true
        } else {
            false
        }
    }

    /// Adds or updates a display output and recomputes layout for its active workspace.
    pub fn upsert_output(&mut self, id: OutputId, work_area: Rect) {
        let clean_area = sanitize_rect(work_area);
        if let Some(existing) = self.outputs.iter_mut().find(|o| o.id == id) {
            existing.work_area = clean_area;
            let ws = existing.active_workspace;
            self.relayout_workspace(ws);
            return;
        }

        // Assign the first unused workspace if possible, fallback to Workspace 1.
        let used: Vec<WorkspaceId> = self.outputs.iter().map(|o| o.active_workspace).collect();
        let next_ws = (WorkspaceId::MIN..=WorkspaceId::MAX)
            .map(WorkspaceId)
            .find(|ws| !used.contains(ws))
            .unwrap_or(WorkspaceId(1));

        self.outputs.push(ManagedOutput {
            id,
            work_area: clean_area,
            active_workspace: next_ws,
        });
        self.relayout_workspace(next_ws);
    }

    /// Removes an output (never removes the last remaining output).
    pub fn remove_output(&mut self, id: OutputId) -> bool {
        if self.outputs.len() <= 1 {
            return false;
        }
        let Some(pos) = self.outputs.iter().position(|o| o.id == id) else {
            return false;
        };
        let removed = self.outputs.remove(pos);
        if self.focused_output == removed.id {
            self.focused_output = self.outputs[0].id;
        }
        self.relayout_workspace(removed.active_workspace);
        true
    }

    /// Returns the active [`WorkspaceId`] on the currently focused output.
    #[must_use]
    pub fn active_workspace_id(&self) -> WorkspaceId {
        self.outputs
            .iter()
            .find(|o| o.id == self.focused_output)
            .map_or(WorkspaceId(1), |o| o.active_workspace)
    }

    /// Returns the active [`LayoutMode`] on the currently focused output's workspace.
    #[must_use]
    pub fn active_layout_mode(&self) -> LayoutMode {
        let ws_id = self.active_workspace_id();
        self.workspace(ws_id)
            .map_or(LayoutMode::Floating, |ws| ws.layout)
    }

    /// Returns a reference to the given [`Workspace`].
    #[must_use]
    pub fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|ws| ws.id == id)
    }

    /// Returns a reference to a [`ManagedWindow`] by its ID.
    #[must_use]
    pub fn window(&self, id: WindowId) -> Option<&ManagedWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// Returns the currently focused [`WindowId`] on the focused output's active workspace.
    #[must_use]
    pub fn focused_window(&self) -> Option<WindowId> {
        let ws_id = self.active_workspace_id();
        self.workspace(ws_id).and_then(|ws| ws.focused)
    }

    /// Returns all windows visible across all active workspaces on connected outputs,
    /// ordered back-to-front (bottom-to-top in `z-order`).
    #[must_use]
    pub fn visible_windows(&self) -> Vec<&ManagedWindow> {
        let mut result = Vec::new();
        for output in &self.outputs {
            if let Some(ws) = self.workspace(output.active_workspace) {
                for &win_id in &ws.stacking {
                    if let Some(win) = self.window(win_id) {
                        result.push(win);
                    }
                }
            }
        }
        result
    }

    /// Opens and maps a new window onto the active workspace of the focused output,
    /// raising and focusing it automatically.
    pub fn map_window(&mut self, id: WindowId, preferred_size: Option<(i32, i32)>) -> Rect {
        if let Some(existing) = self.window(id) {
            return existing.geometry;
        }

        let ws_id = self.active_workspace_id();
        let work_area = self.work_area_for_workspace(ws_id);
        let existing_count = self
            .workspace(ws_id)
            .map_or(0, |ws| i32::try_from(ws.stacking.len()).unwrap_or(0));

        let floating_rect =
            compute_initial_floating_rect(work_area, existing_count, preferred_size);

        self.windows.push(ManagedWindow {
            id,
            workspace: ws_id,
            geometry: floating_rect,
            saved_floating_geometry: floating_rect,
            maximized: false,
        });

        if let Some(ws) = self.workspace_mut(ws_id) {
            ws.stacking.push(id);
            ws.focused = Some(id);
        }

        self.relayout_workspace(ws_id);
        self.window(id).map_or(floating_rect, |w| w.geometry)
    }

    /// Unmaps and removes a window, transferring focus deterministically to the next topmost
    /// window on the same workspace.
    pub fn unmap_window(&mut self, id: WindowId) -> Option<WindowId> {
        let pos = self.windows.iter().position(|w| w.id == id)?;
        let removed = self.windows.remove(pos);
        let ws_id = removed.workspace;

        if let Some(ws) = self.workspace_mut(ws_id) {
            ws.stacking.retain(|&w| w != id);
            if ws.focused == Some(id) {
                ws.focused = ws.stacking.last().copied();
            }
        }

        self.relayout_workspace(ws_id);
        self.workspace(ws_id).and_then(|ws| ws.focused)
    }

    /// Focuses and raises the specified window (and switches the focused output if needed).
    pub fn focus_and_raise(&mut self, id: WindowId) -> bool {
        let Some(ws_id) = self.window(id).map(|w| w.workspace) else {
            return false;
        };

        if let Some(output) = self.outputs.iter().find(|o| o.active_workspace == ws_id) {
            self.focused_output = output.id;
        }

        if let Some(ws) = self.workspace_mut(ws_id) {
            if let Some(pos) = ws.stacking.iter().position(|&w| w == id) {
                let win_id = ws.stacking.remove(pos);
                ws.stacking.push(win_id);
            }
            ws.focused = Some(id);
            true
        } else {
            false
        }
    }

    /// Finds the topmost visible window containing the given logical coordinates `(px, py)`.
    #[must_use]
    pub fn window_under_point(&self, px: i32, py: i32) -> Option<WindowId> {
        for output in self.outputs.iter().rev() {
            if let Some(ws) = self.workspace(output.active_workspace) {
                for &win_id in ws.stacking.iter().rev() {
                    if let Some(win) = self.window(win_id)
                        && win.geometry.contains_point(px, py)
                    {
                        return Some(win_id);
                    }
                }
            }
        }
        None
    }

    /// Switches the active workspace on the focused output.
    /// If another output already displays `target_ws`, the two outputs swap workspaces.
    pub fn switch_workspace(&mut self, target_ws: WorkspaceId) {
        let target_ws = WorkspaceId::new_clamped(target_ws.0);
        let current_ws = self.active_workspace_id();
        if current_ws == target_ws {
            return;
        }

        if let Some(other_output) = self
            .outputs
            .iter_mut()
            .find(|o| o.id != self.focused_output && o.active_workspace == target_ws)
        {
            other_output.active_workspace = current_ws;
        }

        if let Some(focused_out) = self
            .outputs
            .iter_mut()
            .find(|o| o.id == self.focused_output)
        {
            focused_out.active_workspace = target_ws;
        }

        self.relayout_workspace(current_ws);
        self.relayout_workspace(target_ws);
    }

    /// Moves the currently focused window to `target_ws` while keeping the current workspace active.
    pub fn move_focused_to_workspace(&mut self, target_ws: WorkspaceId) -> Option<WindowId> {
        let target_ws = WorkspaceId::new_clamped(target_ws.0);
        let source_ws = self.active_workspace_id();
        let win_id = self.focused_window()?;
        if source_ws == target_ws {
            return Some(win_id);
        }

        if let Some(src) = self.workspace_mut(source_ws) {
            src.stacking.retain(|&w| w != win_id);
            src.focused = src.stacking.last().copied();
        }

        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            win.workspace = target_ws;
        }

        if let Some(dst) = self.workspace_mut(target_ws) {
            dst.stacking.push(win_id);
            dst.focused = Some(win_id);
        }

        self.relayout_workspace(source_ws);
        self.relayout_workspace(target_ws);
        Some(win_id)
    }

    /// Moves the focused window to the next connected output (if more than one output exists).
    pub fn move_focused_to_next_output(&mut self) -> Option<WindowId> {
        if self.outputs.len() <= 1 {
            return self.focused_window();
        }
        let curr_idx = self
            .outputs
            .iter()
            .position(|o| o.id == self.focused_output)?;
        let next_idx = (curr_idx + 1) % self.outputs.len();
        let next_output_id = self.outputs[next_idx].id;
        let next_ws = self.outputs[next_idx].active_workspace;

        let moved = self.move_focused_to_workspace(next_ws)?;
        self.focused_output = next_output_id;
        Some(moved)
    }

    /// Cycles focus forward or backward on the active workspace without losing stacking invariants.
    pub fn cycle_focus(&mut self, forward: bool) -> Option<WindowId> {
        let ws_id = self.active_workspace_id();
        let ws = self.workspace_mut(ws_id)?;
        if ws.stacking.is_empty() {
            ws.focused = None;
            return None;
        }
        if ws.stacking.len() == 1 {
            let only = ws.stacking[0];
            ws.focused = Some(only);
            return Some(only);
        }

        let next_id = if forward {
            // Rotate top window to bottom and bring the previous one to top.
            let top = ws.stacking.pop()?;
            ws.stacking.insert(0, top);
            *ws.stacking.last()?
        } else {
            // Bring bottom window to top.
            let bottom = ws.stacking.remove(0);
            ws.stacking.push(bottom);
            bottom
        };

        ws.focused = Some(next_id);
        Some(next_id)
    }

    /// Toggles the layout mode of the active workspace between `Floating` and `MasterStack`.
    pub fn toggle_layout_mode(&mut self) -> LayoutMode {
        let ws_id = self.active_workspace_id();
        let new_mode = if let Some(ws) = self.workspace_mut(ws_id) {
            ws.layout = match ws.layout {
                LayoutMode::Floating => LayoutMode::MasterStack,
                LayoutMode::MasterStack => LayoutMode::Floating,
            };
            ws.layout
        } else {
            LayoutMode::Floating
        };

        self.relayout_workspace(ws_id);
        new_mode
    }

    /// Toggles maximized state of the currently focused window.
    pub fn toggle_maximize_focused(&mut self) -> Option<Rect> {
        let ws_id = self.active_workspace_id();
        let win_id = self.focused_window()?;
        let work_area = self.work_area_for_workspace(ws_id);

        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            win.maximized = !win.maximized;
            if win.maximized {
                win.geometry = work_area;
            } else {
                win.geometry = win.saved_floating_geometry;
            }
        }

        self.relayout_workspace(ws_id);
        self.window(win_id).map(|w| w.geometry)
    }

    /// Snaps the focused window to a [`SnapTarget`] and switches its state to non-maximized floating.
    pub fn snap_focused(&mut self, target: SnapTarget) -> Option<Rect> {
        let ws_id = self.active_workspace_id();
        let win_id = self.focused_window()?;
        let work_area = self.work_area_for_workspace(ws_id);
        let gap = self.gap_px.max(0);

        let snapped = match target {
            SnapTarget::LeftHalf => {
                let half_w = ((work_area.width - gap * 3) / 2).max(100);
                let h = (work_area.height - gap * 2).max(100);
                Rect::new(work_area.x + gap, work_area.y + gap, half_w, h)
            }
            SnapTarget::RightHalf => {
                let half_w = ((work_area.width - gap * 3) / 2).max(100);
                let h = (work_area.height - gap * 2).max(100);
                let x = work_area.x + gap * 2 + half_w;
                Rect::new(x, work_area.y + gap, half_w, h)
            }
            SnapTarget::Center => {
                let w = ((work_area.width * 7) / 10).max(200);
                let h = ((work_area.height * 75) / 100).max(160);
                let x = work_area.x + (work_area.width - w) / 2;
                let y = work_area.y + (work_area.height - h) / 2;
                Rect::new(x, y, w, h)
            }
        };

        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            win.maximized = false;
            win.geometry = snapped;
            win.saved_floating_geometry = snapped;
        }

        self.relayout_workspace(ws_id);
        self.window(win_id).map(|w| w.geometry)
    }

    /// Moves a window to `(x, y)` in logical space, clearing any maximized flag.
    pub fn move_window(&mut self, id: WindowId, x: i32, y: i32) -> bool {
        let Some(win) = self.windows.iter_mut().find(|w| w.id == id) else {
            return false;
        };
        if win.maximized {
            win.maximized = false;
            win.geometry.width = win.saved_floating_geometry.width;
            win.geometry.height = win.saved_floating_geometry.height;
        }
        win.geometry.x = x;
        win.geometry.y = y;
        win.saved_floating_geometry.x = x;
        win.saved_floating_geometry.y = y;
        true
    }

    /// Applies a [`WmAction`] to the state machine.
    ///
    /// If the action requests closing a window (`WmAction::CloseFocused`), returns `Some(WindowId)`
    /// so the caller (compositor) can send `xdg_toplevel::close` to that client.
    pub fn apply_action(&mut self, action: WmAction) -> Option<WindowId> {
        match action {
            WmAction::FocusNext => {
                self.cycle_focus(true);
                None
            }
            WmAction::FocusPrev => {
                self.cycle_focus(false);
                None
            }
            WmAction::CloseFocused => self.focused_window(),
            WmAction::SwitchWorkspace(ws) => {
                self.switch_workspace(ws);
                None
            }
            WmAction::MoveFocusedToWorkspace(ws) => {
                self.move_focused_to_workspace(ws);
                None
            }
            WmAction::MoveFocusedToNextOutput => {
                self.move_focused_to_next_output();
                None
            }
            WmAction::ToggleLayoutMode => {
                self.toggle_layout_mode();
                None
            }
            WmAction::ToggleMaximizeFocused => {
                self.toggle_maximize_focused();
                None
            }
            WmAction::SnapFocused(target) => {
                self.snap_focused(target);
                None
            }
        }
    }

    fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|ws| ws.id == id)
    }

    fn work_area_for_workspace(&self, ws_id: WorkspaceId) -> Rect {
        self.outputs
            .iter()
            .find(|o| o.active_workspace == ws_id)
            .or_else(|| self.outputs.iter().find(|o| o.id == self.focused_output))
            .map_or_else(|| Rect::new(0, 0, 1280, 800), |o| o.work_area)
    }

    fn relayout_workspace(&mut self, ws_id: WorkspaceId) {
        let work_area = self.work_area_for_workspace(ws_id);
        let gap = self.gap_px.max(0);

        let Some((layout, stacking)) = self
            .workspace(ws_id)
            .map(|ws| (ws.layout, ws.stacking.clone()))
        else {
            return;
        };

        match layout {
            LayoutMode::Floating => {
                for win_id in stacking {
                    if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
                        if win.maximized {
                            win.geometry = work_area;
                        } else {
                            win.geometry =
                                clamp_rect_to_work_area(win.saved_floating_geometry, work_area);
                        }
                    }
                }
            }
            LayoutMode::MasterStack => {
                let count = stacking.len();
                if count == 0 {
                    return;
                }

                // In MasterStack, the topmost (most recently focused/mapped) window is Master on the left,
                // and remaining windows stack vertically on the right.
                let ordered: Vec<WindowId> = stacking.into_iter().rev().collect();

                if count == 1 {
                    let single_rect = Rect::new(
                        work_area.x + gap,
                        work_area.y + gap,
                        (work_area.width - gap * 2).max(100),
                        (work_area.height - gap * 2).max(100),
                    );
                    if let Some(win) = self.windows.iter_mut().find(|w| w.id == ordered[0]) {
                        win.geometry = single_rect;
                    }
                    return;
                }

                let master_w = ((work_area.width - gap * 3) / 2).max(100);
                let stack_w = (work_area.width - gap * 3 - master_w).max(100);
                let master_h = (work_area.height - gap * 2).max(100);

                let master_rect =
                    Rect::new(work_area.x + gap, work_area.y + gap, master_w, master_h);
                if let Some(win) = self.windows.iter_mut().find(|w| w.id == ordered[0]) {
                    win.geometry = master_rect;
                }

                let stack_count = i32::try_from(count - 1).unwrap_or(1);
                let total_stack_gaps = gap * (stack_count + 1);
                let each_h = ((work_area.height - total_stack_gaps) / stack_count).max(60);
                let stack_x = work_area.x + gap * 2 + master_w;

                for (idx, &win_id) in ordered.iter().skip(1).enumerate() {
                    let idx_i32 = i32::try_from(idx).unwrap_or(0);
                    let y = work_area.y + gap + idx_i32 * (each_h + gap);
                    let rect = Rect::new(stack_x, y, stack_w, each_h);
                    if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
                        win.geometry = rect;
                    }
                }
            }
        }
    }
}

const fn sanitize_rect(rect: Rect) -> Rect {
    Rect::new(
        rect.x,
        rect.y,
        if rect.width < 100 { 100 } else { rect.width },
        if rect.height < 100 { 100 } else { rect.height },
    )
}

fn compute_initial_floating_rect(
    work_area: Rect,
    existing_count: i32,
    preferred_size: Option<(i32, i32)>,
) -> Rect {
    let default_w = ((work_area.width * 3) / 5).clamp(320, work_area.width);
    let default_h = ((work_area.height * 2) / 3).clamp(240, work_area.height);
    let (w, h) = preferred_size.map_or((default_w, default_h), |(pw, ph)| {
        (
            pw.clamp(200, work_area.width),
            ph.clamp(150, work_area.height),
        )
    });

    let step = 28;
    let offset_x = 32 + (existing_count * step) % (work_area.width / 3).max(32);
    let offset_y = 32 + (existing_count * step) % (work_area.height / 3).max(32);

    clamp_rect_to_work_area(
        Rect::new(work_area.x + offset_x, work_area.y + offset_y, w, h),
        work_area,
    )
}

fn clamp_rect_to_work_area(rect: Rect, work_area: Rect) -> Rect {
    let width = rect.width.clamp(120, work_area.width);
    let height = rect.height.clamp(100, work_area.height);
    let max_x = work_area.x + work_area.width - width;
    let max_y = work_area.y + work_area.height - height;
    let x = rect.x.clamp(work_area.x, max_x.max(work_area.x));
    let y = rect.y.clamp(work_area.y, max_y.max(work_area.y));
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_window_focus_and_stacking_determinism() {
        let mut wm = WindowManager::new(Rect::new(0, 0, 1280, 800));
        let w1 = wm.next_window_id();
        let w2 = wm.next_window_id();
        let w3 = wm.next_window_id();

        wm.map_window(w1, None);
        wm.map_window(w2, None);
        wm.map_window(w3, None);

        assert_eq!(wm.focused_window(), Some(w3));
        let visible: Vec<WindowId> = wm.visible_windows().into_iter().map(|w| w.id).collect();
        assert_eq!(visible, vec![w1, w2, w3]);

        // Cycle focus forward: w3 moves to bottom, w2 becomes focused on top.
        assert_eq!(wm.cycle_focus(true), Some(w2));
        assert_eq!(wm.focused_window(), Some(w2));

        // Explicit click/focus on w1 raises it to top.
        assert!(wm.focus_and_raise(w1));
        assert_eq!(wm.focused_window(), Some(w1));
        let visible_after: Vec<WindowId> = wm.visible_windows().into_iter().map(|w| w.id).collect();
        assert_eq!(visible_after, vec![w3, w2, w1]);

        // Unmapping w1 transfers focus deterministically to w2.
        assert_eq!(wm.unmap_window(w1), Some(w2));
        assert_eq!(wm.focused_window(), Some(w2));
    }

    #[test]
    fn multi_workspace_switching_and_window_migration() {
        let mut wm = WindowManager::new(Rect::new(0, 0, 1920, 1080));
        let w1 = wm.next_window_id();
        let w2 = wm.next_window_id();

        wm.map_window(w1, None);
        wm.map_window(w2, None);
        assert_eq!(wm.active_workspace_id(), WorkspaceId(1));

        // Move focused window (w2) to Workspace 2.
        wm.apply_action(WmAction::MoveFocusedToWorkspace(WorkspaceId(2)));
        assert_eq!(wm.focused_window(), Some(w1));
        assert_eq!(wm.visible_windows().len(), 1);

        // Switch to Workspace 2: w2 is now visible and focused.
        wm.apply_action(WmAction::SwitchWorkspace(WorkspaceId(2)));
        assert_eq!(wm.active_workspace_id(), WorkspaceId(2));
        assert_eq!(wm.focused_window(), Some(w2));
        assert_eq!(wm.visible_windows().len(), 1);
    }

    #[test]
    fn master_stack_tiling_and_snap_geometries() {
        let mut wm = WindowManager::new(Rect::new(0, 0, 1280, 800));
        let w1 = wm.next_window_id();
        let w2 = wm.next_window_id();

        wm.map_window(w1, None);
        wm.map_window(w2, None);

        // Toggle to MasterStack layout.
        wm.apply_action(WmAction::ToggleLayoutMode);
        assert_eq!(wm.active_layout_mode(), LayoutMode::MasterStack);

        let g2 = wm.window(w2).map(|w| w.geometry).unwrap_or_default();
        let g1 = wm.window(w1).map(|w| w.geometry).unwrap_or_default();
        // Topmost window (w2) is Master on the left; w1 is on the right stack.
        assert!(g2.x < g1.x);
        assert_eq!(g2.width, g1.width);

        // Toggle back to Floating and test LeftHalf snap.
        wm.apply_action(WmAction::ToggleLayoutMode);
        assert_eq!(wm.active_layout_mode(), LayoutMode::Floating);
        wm.apply_action(WmAction::SnapFocused(SnapTarget::LeftHalf));
        let snapped = wm.window(w2).map(|w| w.geometry).unwrap_or_default();
        assert_eq!(snapped.x, wm.gap_px);
    }

    #[test]
    fn multi_output_workspace_assignment_and_window_move() {
        let mut wm = WindowManager::new(Rect::new(0, 0, 1920, 1080));
        wm.upsert_output(OutputId(2), Rect::new(1920, 0, 1920, 1080));

        let w1 = wm.next_window_id();
        wm.map_window(w1, None);
        assert_eq!(wm.window(w1).map(|w| w.workspace), Some(WorkspaceId(1)));

        // Move w1 to the second output (which has active Workspace 2).
        wm.apply_action(WmAction::MoveFocusedToNextOutput);
        assert_eq!(wm.focused_output(), OutputId(2));
        assert_eq!(wm.window(w1).map(|w| w.workspace), Some(WorkspaceId(2)));
        let geom = wm.window(w1).map(|w| w.geometry).unwrap_or_default();
        assert!(geom.x >= 1920);
    }
}
