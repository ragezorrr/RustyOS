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
    /// Floating desktop with cascading initial placement and quick snap helpers.
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
    /// Top-left 25% quarter of the work area (`RUSTYOS-166`).
    TopLeft,
    /// Top-right 25% quarter of the work area (`RUSTYOS-166`).
    TopRight,
    /// Bottom-left 25% quarter of the work area (`RUSTYOS-166`).
    BottomLeft,
    /// Bottom-right 25% quarter of the work area (`RUSTYOS-166`).
    BottomRight,
    /// Full work area maximization (`RUSTYOS-166`).
    Maximize,
}

/// Computes the destination logical rectangle for `target` inside `work_area` with `gap_px` spacing (`RUSTYOS-166`).
#[must_use]
pub fn compute_snap_rect(work_area: Rect, gap_px: i32, target: SnapTarget) -> Rect {
    let gap = gap_px.max(0);
    let half_w = ((work_area.width - gap * 3) / 2).max(100);
    let full_h = (work_area.height - gap * 2).max(100);
    let half_h = ((work_area.height - gap * 3) / 2).max(100);
    let left_x = work_area.x + gap;
    let right_x = work_area.x + gap * 2 + half_w;
    let top_y = work_area.y + gap;
    let bottom_y = work_area.y + gap * 2 + half_h;

    match target {
        SnapTarget::Maximize => work_area,
        SnapTarget::LeftHalf => Rect::new(left_x, top_y, half_w, full_h),
        SnapTarget::RightHalf => Rect::new(right_x, top_y, half_w, full_h),
        SnapTarget::TopLeft => Rect::new(left_x, top_y, half_w, half_h),
        SnapTarget::TopRight => Rect::new(right_x, top_y, half_w, half_h),
        SnapTarget::BottomLeft => Rect::new(left_x, bottom_y, half_w, half_h),
        SnapTarget::BottomRight => Rect::new(right_x, bottom_y, half_w, half_h),
        SnapTarget::Center => {
            let w = ((work_area.width * 7) / 10).max(200);
            let h = ((work_area.height * 75) / 100).max(160);
            let x = work_area.x + (work_area.width - w) / 2;
            let y = work_area.y + (work_area.height - h) / 2;
            Rect::new(x, y, w, h)
        }
    }
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
    /// Toggle true unbordered fullscreen on the focused window (`Alt+F` / XDG fullscreen).
    ToggleFullscreen,
    /// Snap the focused window to a specific region (`Alt+Left`, `Alt+Right`, `Alt+C`).
    SnapFocused(SnapTarget),
}

/// State of a managed window inside [`WindowManager`].
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWindow {
    /// Unique identifier of the window.
    pub id: WindowId,
    /// Workspace to which the window currently belongs.
    pub workspace: WorkspaceId,
    /// Current effective logical geometry (either floating, snapped, maximized, fullscreen, or tiled).
    pub geometry: Rect,
    /// Saved floating geometry restored when leaving maximized, fullscreen, or tiled states.
    pub saved_floating_geometry: Rect,
    /// Whether the window is currently maximized on its workspace.
    pub maximized: bool,
    /// Whether the window is currently in true unbordered fullscreen mode (`RUSTYOS-041`).
    pub fullscreen: bool,
    /// Whether the window is currently minimized to the Dock (`RUSTYOS-165`).
    pub minimized: bool,
    /// Optional client-declared maximum logical size `(max_w, max_h)` where `0` means unbounded (`RUSTYOS-165`).
    pub max_size: Option<(i32, i32)>,
    /// Optional parent window for Wayland `xdg_toplevel.set_parent` or X11 `WM_TRANSIENT_FOR` (`RUSTYOS-167`).
    pub parent: Option<WindowId>,
    /// Whether the window is a transient/dialog/utility window that stays floating in tiled layouts (`RUSTYOS-167`).
    pub is_dialog: bool,
    /// Whether the window blocks focus on its parent window (`RUSTYOS-167`).
    pub is_modal: bool,
}

impl ManagedWindow {
    /// Returns `true` when this window should stay floating even in `LayoutMode::MasterStack` (`RUSTYOS-167`).
    #[must_use]
    pub const fn is_floating_transient(&self) -> bool {
        self.is_dialog || self.is_modal || self.parent.is_some()
    }
}

/// State of a display output managed by [`WindowManager`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedOutput {
    /// Unique identifier of the output.
    pub id: OutputId,
    /// Usable logical work area of the output (reserving shell panels/docks).
    pub work_area: Rect,
    /// Complete physical/logical display bounds used for fullscreen direct scanout (`RUSTYOS-041`).
    pub full_area: Rect,
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
        let clean_work = sanitize_rect(primary_work_area);
        let outputs = vec![ManagedOutput {
            id: primary_id,
            work_area: clean_work,
            full_area: clean_work,
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
        self.upsert_output_with_full_area(id, clean_area, clean_area);
    }

    /// Adds or updates a display output with explicit usable work area and full display bounds
    /// (`RUSTYOS-041`), then recomputes layout for its active workspace.
    pub fn upsert_output_with_full_area(&mut self, id: OutputId, work_area: Rect, full_area: Rect) {
        let clean_work = sanitize_rect(work_area);
        let clean_full = sanitize_rect(full_area);
        if let Some(existing) = self.outputs.iter_mut().find(|o| o.id == id) {
            existing.work_area = clean_work;
            existing.full_area = clean_full;
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
            work_area: clean_work,
            full_area: clean_full,
            active_workspace: next_ws,
        });
        self.relayout_workspace(next_ws);
    }

    /// Removes an output (never removes the last remaining output).
    ///
    /// Migrates any windows that were assigned to the disconnected output's workspace
    /// onto the remaining active output's workspace while preserving stacking order
    /// and `saved_floating_geometry` dimensions (`RUSTYOS-165`).
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

        let target_ws = self.outputs[0].active_workspace;
        let work_area = self.outputs[0].work_area;
        let removed_ws = removed.active_workspace;

        if removed_ws != target_ws {
            let migrated_ids: Vec<WindowId> = if let Some(ws) = self.workspace_mut(removed_ws) {
                ws.focused = None;
                std::mem::take(&mut ws.stacking)
            } else {
                Vec::new()
            };

            let mut migrated_set = Vec::with_capacity(migrated_ids.len());
            for win_id in migrated_ids {
                if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
                    win.workspace = target_ws;
                    win.saved_floating_geometry = clamp_rect_to_work_area(
                        Rect::new(
                            work_area.x + 32,
                            work_area.y + 32,
                            win.saved_floating_geometry.width,
                            win.saved_floating_geometry.height,
                        ),
                        work_area,
                    );
                    if !win.maximized && !win.fullscreen {
                        win.geometry = win.saved_floating_geometry;
                    }
                    migrated_set.push(win_id);
                }
            }
            // Catch any stray windows referencing `removed_ws` not in `removed_ws.stacking`.
            for win in &mut self.windows {
                if win.workspace == removed_ws {
                    win.workspace = target_ws;
                    win.saved_floating_geometry = clamp_rect_to_work_area(
                        Rect::new(
                            work_area.x + 32,
                            work_area.y + 32,
                            win.saved_floating_geometry.width,
                            win.saved_floating_geometry.height,
                        ),
                        work_area,
                    );
                    if !win.maximized && !win.fullscreen {
                        win.geometry = win.saved_floating_geometry;
                    }
                    migrated_set.push(win.id);
                }
            }

            if let Some(dst) = self.workspace_mut(target_ws) {
                for win_id in migrated_set {
                    if !dst.stacking.contains(&win_id) {
                        dst.stacking.push(win_id);
                    }
                }
            }
            if self
                .workspace(target_ws)
                .is_some_and(|ws| ws.focused.is_none())
            {
                let next_focus = self.topmost_unminimized_in(target_ws);
                if let Some(dst) = self.workspace_mut(target_ws) {
                    dst.focused = next_focus;
                }
            }
        }

        self.relayout_workspace(target_ws);
        true
    }

    /// Returns the managed output metadata for `id`, if connected (`RUSTYOS-165`).
    #[must_use]
    pub fn output(&self, id: OutputId) -> Option<&ManagedOutput> {
        self.outputs.iter().find(|o| o.id == id)
    }

    /// Returns all currently connected managed outputs (`RUSTYOS-165`).
    #[must_use]
    pub fn outputs(&self) -> &[ManagedOutput] {
        &self.outputs
    }

    /// Returns the output currently displaying `ws_id`, if any (`RUSTYOS-165`).
    #[must_use]
    pub fn output_for_workspace(&self, ws_id: WorkspaceId) -> Option<&ManagedOutput> {
        self.outputs.iter().find(|o| o.active_workspace == ws_id)
    }

    /// Returns the output currently owning `win_id`'s workspace, falling back to `focused_output` (`RUSTYOS-165`).
    #[must_use]
    pub fn output_for_window(&self, win_id: WindowId) -> Option<&ManagedOutput> {
        let ws_id = self.window(win_id)?.workspace;
        self.output_for_workspace(ws_id)
            .or_else(|| self.output(self.focused_output))
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
            result.extend(self.visible_windows_for_output(output.id));
        }
        result
    }

    /// Returns visible windows on `output_id`'s active workspace, ordered back-to-front (`RUSTYOS-042`).
    #[must_use]
    pub fn visible_windows_for_output(&self, output_id: OutputId) -> Vec<&ManagedWindow> {
        let mut result = Vec::new();
        if let Some(output) = self.outputs.iter().find(|o| o.id == output_id)
            && let Some(ws) = self.workspace(output.active_workspace)
        {
            for &win_id in &ws.stacking {
                if let Some(win) = self.window(win_id) {
                    result.push(win);
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
            fullscreen: false,
            minimized: false,
            max_size: None,
            parent: None,
            is_dialog: false,
            is_modal: false,
        });

        if let Some(ws) = self.workspace_mut(ws_id) {
            ws.stacking.push(id);
            ws.focused = Some(id);
        }

        self.relayout_workspace(ws_id);
        self.window(id).map_or(floating_rect, |w| w.geometry)
    }

    /// Assigns or clears the transient parent of `win_id` (`xdg_toplevel.set_parent` / `WM_TRANSIENT_FOR`)
    /// with cycle protection, workspace co-location, and Z-order enforcement (`RUSTYOS-167`).
    pub fn set_window_parent(&mut self, win_id: WindowId, parent: Option<WindowId>) -> bool {
        let Some(curr_parent) = self.window(win_id).map(|w| w.parent) else {
            return false;
        };
        let valid_parent = parent.filter(|&pid| {
            pid != win_id && self.window(pid).is_some() && !self.is_ancestor_of(win_id, pid)
        });
        let changed = curr_parent != valid_parent;
        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            win.parent = valid_parent;
            if valid_parent.is_some() {
                win.is_dialog = true;
            }
        }
        if let Some(pid) = valid_parent
            && let Some(parent_ws) = self.window(pid).map(|w| w.workspace)
            && self
                .window(win_id)
                .is_some_and(|w| w.workspace != parent_ws)
        {
            let _ = self.move_window_to_workspace(win_id, parent_ws);
        }
        if let Some(ws_id) = self.window(win_id).map(|w| w.workspace) {
            self.enforce_transient_stacking_order(ws_id);
            self.relayout_workspace(ws_id);
        }
        changed
    }

    /// Sets explicit dialog/modal flags for `win_id` (`RUSTYOS-167`).
    pub fn set_window_dialog_flags(
        &mut self,
        win_id: WindowId,
        is_dialog: bool,
        is_modal: bool,
    ) -> bool {
        let Some(ws_id) = self.window(win_id).map(|w| w.workspace) else {
            return false;
        };
        let changed = if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            let eff_dialog = is_dialog || is_modal || win.parent.is_some();
            let diff = win.is_dialog != eff_dialog || win.is_modal != is_modal;
            win.is_dialog = eff_dialog;
            win.is_modal = is_modal;
            diff
        } else {
            false
        };
        if changed {
            self.enforce_transient_stacking_order(ws_id);
            self.relayout_workspace(ws_id);
        }
        changed
    }

    /// Centers `win_id` over its transient parent window clamped inside the output `work_area` (`RUSTYOS-167`).
    pub fn center_window_on_parent(&mut self, win_id: WindowId) -> Option<Rect> {
        let (ws_id, parent_id, rect, locked) = self.window(win_id).map(|w| {
            (
                w.workspace,
                w.parent,
                w.geometry,
                w.maximized || w.fullscreen,
            )
        })?;
        if locked {
            return Some(rect);
        }
        let parent_rect = self.window(parent_id?)?.geometry;
        let work_area = self.work_area_for_workspace(ws_id);
        let centered = clamp_rect_to_work_area(
            Rect::new(
                parent_rect.x + (parent_rect.width - rect.width) / 2,
                parent_rect.y + (parent_rect.height - rect.height) / 2,
                rect.width,
                rect.height,
            ),
            work_area,
        );
        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            win.geometry = centered;
            win.saved_floating_geometry = centered;
        }
        Some(centered)
    }

    /// Returns all transient descendants of `root_id` on the same workspace in bottom-to-top stacking order (`RUSTYOS-167`).
    #[must_use]
    pub fn transient_descendants(&self, root_id: WindowId) -> Vec<WindowId> {
        let Some(root_ws) = self.window(root_id).map(|w| w.workspace) else {
            return Vec::new();
        };
        let Some(ws) = self.workspace(root_ws) else {
            return Vec::new();
        };
        ws.stacking
            .iter()
            .copied()
            .filter(|&cand| cand != root_id && self.is_ancestor_of(root_id, cand))
            .collect()
    }

    /// Returns the topmost unminimized modal descendant of `root_id` on the same workspace, if any (`RUSTYOS-167`).
    #[must_use]
    pub fn active_modal_descendant(&self, root_id: WindowId) -> Option<WindowId> {
        let root_ws = self.window(root_id)?.workspace;
        let ws = self.workspace(root_ws)?;
        ws.stacking.iter().rev().copied().find(|&cand| {
            cand != root_id
                && self.is_ancestor_of(root_id, cand)
                && self
                    .window(cand)
                    .is_some_and(|w| w.is_modal && !w.minimized)
        })
    }

    /// Moves `win_id` together with its same-workspace transient descendants onto `target_ws` (`RUSTYOS-167`).
    pub fn move_window_to_workspace(&mut self, win_id: WindowId, target_ws: WorkspaceId) -> bool {
        let target_ws = WorkspaceId::new_clamped(target_ws.0);
        let Some((source_ws, is_minimized)) =
            self.window(win_id).map(|w| (w.workspace, w.minimized))
        else {
            return false;
        };
        if source_ws == target_ws {
            return true;
        }

        let mut group = Vec::with_capacity(4);
        group.push(win_id);
        for desc in self.transient_descendants(win_id) {
            if !group.contains(&desc) {
                group.push(desc);
            }
        }

        if let Some(src) = self.workspace_mut(source_ws) {
            src.stacking.retain(|w| !group.contains(w));
        }
        let next_src_focus = self.topmost_unminimized_in(source_ws);
        if let Some(src) = self.workspace_mut(source_ws)
            && src.focused.is_some_and(|f| group.contains(&f))
        {
            src.focused = next_src_focus;
        }

        for &id in &group {
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
                win.workspace = target_ws;
            }
        }

        if let Some(dst) = self.workspace_mut(target_ws) {
            for &id in &group {
                if !dst.stacking.contains(&id) {
                    dst.stacking.push(id);
                }
            }
        }
        self.enforce_transient_stacking_order(target_ws);
        if !is_minimized {
            let focus_target = self.active_modal_descendant(win_id).unwrap_or(win_id);
            if let Some(dst) = self.workspace_mut(target_ws) {
                dst.focused = Some(focus_target);
            }
        }
        self.relayout_workspace(source_ws);
        self.relayout_workspace(target_ws);
        true
    }

    /// Unmaps and removes a window, clearing orphaned parent links and transferring focus
    /// preferentially back to its transient parent (or the next topmost non-minimized window).
    pub fn unmap_window(&mut self, id: WindowId) -> Option<WindowId> {
        let pos = self.windows.iter().position(|w| w.id == id)?;
        let removed = self.windows.remove(pos);
        let ws_id = removed.workspace;

        for win in &mut self.windows {
            if win.parent == Some(id) {
                win.parent = None;
            }
        }

        let was_focused = if let Some(ws) = self.workspace_mut(ws_id) {
            ws.stacking.retain(|&w| w != id);
            ws.focused == Some(id)
        } else {
            false
        };
        if was_focused {
            let parent_focus = removed.parent.and_then(|pid| {
                self.window(pid)
                    .filter(|p| p.workspace == ws_id && !p.minimized)
                    .map(|_| self.active_modal_descendant(pid).unwrap_or(pid))
            });
            let next_focus = parent_focus.or_else(|| self.topmost_unminimized_in(ws_id));
            if let Some(ws) = self.workspace_mut(ws_id) {
                ws.focused = next_focus;
            }
        }

        self.enforce_transient_stacking_order(ws_id);
        self.relayout_workspace(ws_id);
        self.workspace(ws_id).and_then(|ws| ws.focused)
    }

    /// Sets the minimized state of `win_id`, updating workspace focus and tiling layout (`RUSTYOS-165`).
    ///
    /// Returns `true` if `win_id` existed and its minimized state actually changed.
    pub fn set_window_minimized(&mut self, win_id: WindowId, minimized: bool) -> bool {
        let Some(ws_id) = self.window(win_id).map(|w| w.workspace) else {
            return false;
        };
        let changed = if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            if win.minimized == minimized {
                false
            } else {
                win.minimized = minimized;
                true
            }
        } else {
            false
        };
        if !changed {
            return false;
        }

        if minimized {
            if self
                .workspace(ws_id)
                .is_some_and(|ws| ws.focused == Some(win_id))
            {
                let next_focus = self.topmost_unminimized_in(ws_id);
                if let Some(ws) = self.workspace_mut(ws_id) {
                    ws.focused = next_focus;
                }
            }
        } else {
            if let Some(output) = self.outputs.iter().find(|o| o.active_workspace == ws_id) {
                self.focused_output = output.id;
            }
            if let Some(ws) = self.workspace_mut(ws_id) {
                if let Some(pos) = ws.stacking.iter().position(|&w| w == win_id) {
                    let id = ws.stacking.remove(pos);
                    ws.stacking.push(id);
                }
                ws.focused = Some(win_id);
            }
            self.enforce_transient_stacking_order(ws_id);
        }

        self.relayout_workspace(ws_id);
        true
    }

    /// Focuses and raises the specified window (and switches the focused output if needed).
    /// Unminimizes the window if it was minimized so focus never points to a hidden window.
    /// If the window has an active modal descendant, focus redirects to the modal and all
    /// transient descendants remain stacked above their parent (`RUSTYOS-167`).
    pub fn focus_and_raise(&mut self, id: WindowId) -> bool {
        let Some((ws_id, was_minimized)) = self.window(id).map(|w| (w.workspace, w.minimized))
        else {
            return false;
        };
        if was_minimized {
            return self.set_window_minimized(id, false);
        }

        let focus_target = self.active_modal_descendant(id).unwrap_or(id);
        let descendants = self.transient_descendants(id);

        if let Some(output) = self.outputs.iter().find(|o| o.active_workspace == ws_id) {
            self.focused_output = output.id;
        }

        let raised = if let Some(ws) = self.workspace_mut(ws_id) {
            if let Some(pos) = ws.stacking.iter().position(|&w| w == id) {
                let win_id = ws.stacking.remove(pos);
                ws.stacking.push(win_id);
            }
            for &desc in &descendants {
                if let Some(pos) = ws.stacking.iter().position(|&w| w == desc) {
                    let child_id = ws.stacking.remove(pos);
                    ws.stacking.push(child_id);
                }
            }
            if focus_target != id
                && let Some(pos) = ws.stacking.iter().position(|&w| w == focus_target)
            {
                let modal_id = ws.stacking.remove(pos);
                ws.stacking.push(modal_id);
            }
            ws.focused = Some(focus_target);
            true
        } else {
            false
        };
        if raised {
            self.enforce_transient_stacking_order(ws_id);
        }
        raised
    }

    /// Finds the topmost visible non-minimized window containing the given logical coordinates `(px, py)`.
    #[must_use]
    pub fn window_under_point(&self, px: i32, py: i32) -> Option<WindowId> {
        for output in self.outputs.iter().rev() {
            if let Some(ws) = self.workspace(output.active_workspace) {
                for &win_id in ws.stacking.iter().rev() {
                    if let Some(win) = self.window(win_id)
                        && !win.minimized
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

    /// Moves the currently focused window (and its transient descendants) to `target_ws`
    /// while keeping the current workspace active (`RUSTYOS-167`).
    pub fn move_focused_to_workspace(&mut self, target_ws: WorkspaceId) -> Option<WindowId> {
        let win_id = self.focused_window()?;
        if self.move_window_to_workspace(win_id, target_ws) {
            Some(win_id)
        } else {
            None
        }
    }

    /// Migrates `win_id` (and its same-workspace transient descendants) onto `target_output`'s
    /// active workspace (e.g. when dragged across monitors or targeted by
    /// `xdg_toplevel::set_fullscreen(output)`) without snapping floating coordinates (`RUSTYOS-165`, `RUSTYOS-167`).
    pub fn migrate_window_to_output(&mut self, win_id: WindowId, target_output: OutputId) -> bool {
        let Some(target_ws) = self.output(target_output).map(|o| o.active_workspace) else {
            return false;
        };
        let Some((source_ws, is_minimized, needs_relayout)) = self
            .window(win_id)
            .map(|w| (w.workspace, w.minimized, w.maximized || w.fullscreen))
        else {
            return false;
        };
        if source_ws == target_ws {
            return false;
        }

        let mut group = Vec::with_capacity(4);
        group.push(win_id);
        for desc in self.transient_descendants(win_id) {
            if !group.contains(&desc) {
                group.push(desc);
            }
        }

        if let Some(src) = self.workspace_mut(source_ws) {
            src.stacking.retain(|w| !group.contains(w));
        }
        let next_src_focus = self.topmost_unminimized_in(source_ws);
        if let Some(src) = self.workspace_mut(source_ws)
            && src.focused.is_some_and(|f| group.contains(&f))
        {
            src.focused = next_src_focus;
        }

        for &id in &group {
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
                win.workspace = target_ws;
            }
        }

        let dst_is_tiled = self
            .workspace(target_ws)
            .is_some_and(|ws| ws.layout == LayoutMode::MasterStack);
        if let Some(dst) = self.workspace_mut(target_ws) {
            for &id in &group {
                if !dst.stacking.contains(&id) {
                    dst.stacking.push(id);
                }
            }
        }
        self.focused_output = target_output;
        self.enforce_transient_stacking_order(target_ws);
        if !is_minimized {
            let focus_target = self.active_modal_descendant(win_id).unwrap_or(win_id);
            if let Some(dst) = self.workspace_mut(target_ws) {
                dst.focused = Some(focus_target);
            }
        }
        self.relayout_workspace(source_ws);
        if needs_relayout || dst_is_tiled {
            self.relayout_workspace(target_ws);
        }
        true
    }

    /// Moves the focused window to the next connected output (if more than one output exists),
    /// preserving relative position inside the destination output's work area (`RUSTYOS-165`).
    pub fn move_focused_to_next_output(&mut self) -> Option<WindowId> {
        if self.outputs.len() <= 1 {
            return self.focused_window();
        }
        let curr_idx = self
            .outputs
            .iter()
            .position(|o| o.id == self.focused_output)?;
        let src_work = self.outputs[curr_idx].work_area;
        let next_idx = (curr_idx + 1) % self.outputs.len();
        let next_output_id = self.outputs[next_idx].id;
        let next_ws = self.outputs[next_idx].active_workspace;
        let dst_work = self.outputs[next_idx].work_area;

        let win_id = self.focused_window()?;
        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            let rel_x = win.saved_floating_geometry.x - src_work.x;
            let rel_y = win.saved_floating_geometry.y - src_work.y;
            win.saved_floating_geometry = clamp_rect_to_work_area(
                Rect::new(
                    dst_work.x + rel_x,
                    dst_work.y + rel_y,
                    win.saved_floating_geometry.width,
                    win.saved_floating_geometry.height,
                ),
                dst_work,
            );
            if !win.maximized && !win.fullscreen {
                win.geometry = win.saved_floating_geometry;
            }
        }

        let moved = self.move_focused_to_workspace(next_ws)?;
        self.focused_output = next_output_id;
        Some(moved)
    }

    /// Cycles focus forward or backward among non-minimized windows on the active workspace
    /// without losing stacking invariants (`RUSTYOS-165`).
    pub fn cycle_focus(&mut self, forward: bool) -> Option<WindowId> {
        let ws_id = self.active_workspace_id();
        let active_ids: Vec<WindowId> = self
            .workspace(ws_id)?
            .stacking
            .iter()
            .copied()
            .filter(|&id| self.window(id).is_some_and(|w| !w.minimized))
            .collect();
        if active_ids.is_empty() {
            if let Some(ws) = self.workspace_mut(ws_id) {
                ws.focused = None;
            }
            return None;
        }
        if active_ids.len() == 1 {
            let only = active_ids[0];
            if let Some(ws) = self.workspace_mut(ws_id) {
                ws.focused = Some(only);
            }
            return Some(only);
        }

        let ws = self.workspace_mut(ws_id)?;
        let next_id = if forward {
            // Move topmost non-minimized window to the bottom and raise the previous non-minimized one.
            let top = *active_ids.last()?;
            let next = active_ids[active_ids.len() - 2];
            ws.stacking.retain(|&w| w != top && w != next);
            ws.stacking.insert(0, top);
            ws.stacking.push(next);
            next
        } else {
            // Bring bottommost non-minimized window to the top.
            let bottom = active_ids[0];
            ws.stacking.retain(|&w| w != bottom);
            ws.stacking.push(bottom);
            bottom
        };

        ws.focused = Some(next_id);
        let effective_focus = self.active_modal_descendant(next_id).unwrap_or(next_id);
        if effective_focus != next_id
            && let Some(ws) = self.workspace_mut(ws_id)
        {
            ws.focused = Some(effective_focus);
        }
        self.enforce_transient_stacking_order(ws_id);
        Some(effective_focus)
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

    /// Explicitly enables or disables maximized state for `win_id` without altering focus or stacking
    /// order (`RUSTYOS-165`).
    pub fn set_window_maximized(&mut self, win_id: WindowId, enabled: bool) -> Option<Rect> {
        let ws_id = self.window(win_id)?.workspace;
        let work_area = self.work_area_for_workspace(ws_id);

        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            win.fullscreen = false;
            win.maximized = enabled;
            if enabled {
                win.geometry = work_area;
            } else {
                win.geometry = win.saved_floating_geometry;
            }
        }

        self.relayout_workspace(ws_id);
        self.window(win_id).map(|w| w.geometry)
    }

    /// Toggles maximized state of the currently focused window.
    /// When currently fullscreen, exits fullscreen into maximized state (`RUSTYOS-165`).
    pub fn toggle_maximize_focused(&mut self) -> Option<Rect> {
        let win_id = self.focused_window()?;
        let win = self.window(win_id)?;
        let next = if win.fullscreen { true } else { !win.maximized };
        self.set_window_maximized(win_id, next)
    }

    /// Explicitly enables or disables fullscreen mode for `win_id` (`RUSTYOS-041`).
    ///
    /// In fullscreen mode, the window spans its output's complete `full_area` (ignoring
    /// `TopBar`/`Dock` margins). Leaving fullscreen restores maximized or floating geometry.
    pub fn set_window_fullscreen(&mut self, win_id: WindowId, enabled: bool) -> Option<Rect> {
        self.set_window_fullscreen_on_output(win_id, enabled, None)
    }

    /// Explicitly enables or disables fullscreen mode for `win_id`, optionally targeting a specific
    /// `target_output` as requested by `xdg_toplevel::set_fullscreen(output)` (`RUSTYOS-165`).
    pub fn set_window_fullscreen_on_output(
        &mut self,
        win_id: WindowId,
        enabled: bool,
        target_output: Option<OutputId>,
    ) -> Option<Rect> {
        if enabled && let Some(out_id) = target_output {
            let _ = self.migrate_window_to_output(win_id, out_id);
        }
        let ws_id = self.window(win_id)?.workspace;
        let full_area = self.full_area_for_workspace(ws_id);
        let work_area = self.work_area_for_workspace(ws_id);

        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            if win.fullscreen == enabled {
                return Some(win.geometry);
            }
            win.fullscreen = enabled;
            if enabled {
                win.geometry = full_area;
            } else if win.maximized {
                win.geometry = work_area;
            } else {
                win.geometry = win.saved_floating_geometry;
            }
        }

        self.relayout_workspace(ws_id);
        self.window(win_id).map(|w| w.geometry)
    }

    /// Updates the client-declared maximum logical size for `win_id` (`0` = unbounded per axis) (`RUSTYOS-165`).
    pub fn set_window_max_size(&mut self, win_id: WindowId, max_size: Option<(i32, i32)>) -> bool {
        let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) else {
            return false;
        };
        let normalized = max_size.and_then(|(w, h)| {
            let cw = w.max(0);
            let ch = h.max(0);
            (cw > 0 || ch > 0).then_some((cw, ch))
        });
        if win.max_size == normalized {
            return false;
        }
        win.max_size = normalized;
        true
    }

    /// Toggles fullscreen state of the currently focused window (`RUSTYOS-041`).
    pub fn toggle_fullscreen_focused(&mut self) -> Option<Rect> {
        let win_id = self.focused_window()?;
        let next_state = !self.window(win_id)?.fullscreen;
        self.set_window_fullscreen(win_id, next_state)
    }

    /// Snaps `win_id` to a [`SnapTarget`] within its workspace's output work area (`RUSTYOS-166`).
    pub fn snap_window(&mut self, win_id: WindowId, target: SnapTarget) -> Option<Rect> {
        if target == SnapTarget::Maximize {
            return self.set_window_maximized(win_id, true);
        }
        let ws_id = self.window(win_id)?.workspace;
        let work_area = self.work_area_for_workspace(ws_id);
        let snapped = compute_snap_rect(work_area, self.gap_px, target);

        if let Some(win) = self.windows.iter_mut().find(|w| w.id == win_id) {
            win.fullscreen = false;
            win.maximized = false;
            win.geometry = snapped;
            win.saved_floating_geometry = snapped;
        }

        self.relayout_workspace(ws_id);
        self.window(win_id).map(|w| w.geometry)
    }

    /// Snaps the focused window to a [`SnapTarget`] and switches its state accordingly.
    pub fn snap_focused(&mut self, target: SnapTarget) -> Option<Rect> {
        let win_id = self.focused_window()?;
        self.snap_window(win_id, target)
    }

    /// Moves a window to `(x, y)` in logical space, clearing any maximized or fullscreen flag.
    pub fn move_window(&mut self, id: WindowId, x: i32, y: i32) -> bool {
        let Some(win) = self.windows.iter_mut().find(|w| w.id == id) else {
            return false;
        };
        if win.maximized || win.fullscreen {
            win.maximized = false;
            win.fullscreen = false;
            win.geometry.width = win.saved_floating_geometry.width;
            win.geometry.height = win.saved_floating_geometry.height;
        }
        win.geometry.x = x;
        win.geometry.y = y;
        win.saved_floating_geometry.x = x;
        win.saved_floating_geometry.y = y;
        true
    }

    /// Resizes and repositions a window to `rect` in logical space (`RUSTYOS-022`),
    /// enforcing minimum interactive bounds `(240, 160)` and clearing any maximized or fullscreen flag.
    pub fn resize_window(&mut self, id: WindowId, rect: Rect) -> bool {
        self.resize_window_with_min(id, rect, 240, 160)
    }

    /// Resizes and repositions a window to `rect` in logical space while enforcing explicit
    /// minimum bounds `(min_w, min_h)` and any client-declared `max_size` (`RUSTYOS-112` / `RUSTYOS-165`).
    pub fn resize_window_with_min(
        &mut self,
        id: WindowId,
        rect: Rect,
        min_w: i32,
        min_h: i32,
    ) -> bool {
        let Some(win) = self.windows.iter_mut().find(|w| w.id == id) else {
            return false;
        };
        let floor_w = min_w.max(64);
        let floor_h = min_h.max(64);
        let mut width = rect.width.max(floor_w);
        let mut height = rect.height.max(floor_h);
        if let Some((max_w, max_h)) = win.max_size {
            if max_w > 0 {
                width = width.min(max_w.max(floor_w));
            }
            if max_h > 0 {
                height = height.min(max_h.max(floor_h));
            }
        }
        let sanitized = Rect::new(rect.x, rect.y, width, height);
        win.maximized = false;
        win.fullscreen = false;
        win.geometry = sanitized;
        win.saved_floating_geometry = sanitized;
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
            WmAction::ToggleFullscreen => {
                self.toggle_fullscreen_focused();
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

    fn topmost_unminimized_in(&self, ws_id: WorkspaceId) -> Option<WindowId> {
        self.workspace(ws_id)?
            .stacking
            .iter()
            .rev()
            .copied()
            .find(|&id| self.window(id).is_some_and(|w| !w.minimized))
    }

    fn work_area_for_workspace(&self, ws_id: WorkspaceId) -> Rect {
        self.outputs
            .iter()
            .find(|o| o.active_workspace == ws_id)
            .or_else(|| self.outputs.iter().find(|o| o.id == self.focused_output))
            .map_or_else(|| Rect::new(0, 0, 1280, 800), |o| o.work_area)
    }

    fn full_area_for_workspace(&self, ws_id: WorkspaceId) -> Rect {
        self.outputs
            .iter()
            .find(|o| o.active_workspace == ws_id)
            .or_else(|| self.outputs.iter().find(|o| o.id == self.focused_output))
            .map_or_else(|| Rect::new(0, 0, 1280, 800), |o| o.full_area)
    }

    fn is_ancestor_of(&self, ancestor: WindowId, candidate: WindowId) -> bool {
        let mut cursor = self.window(candidate).and_then(|w| w.parent);
        let mut steps = 0usize;
        let max_steps = self.windows.len();
        while let Some(pid) = cursor {
            if pid == ancestor {
                return true;
            }
            steps += 1;
            if steps > max_steps {
                break;
            }
            cursor = self.window(pid).and_then(|w| w.parent);
        }
        false
    }

    /// Enforces the Z-order invariant that every transient child on `ws_id` is stacked above its
    /// parent while preserving relative ordering among unrelated windows (`RUSTYOS-167`).
    fn enforce_transient_stacking_order(&mut self, ws_id: WorkspaceId) {
        let parents: Vec<(WindowId, WindowId)> = self
            .windows
            .iter()
            .filter_map(|w| {
                (w.workspace == ws_id)
                    .then_some(w.parent.map(|pid| (w.id, pid)))
                    .flatten()
            })
            .collect();
        if parents.is_empty() {
            return;
        }
        let Some(ws) = self.workspace_mut(ws_id) else {
            return;
        };
        let max_passes = ws.stacking.len().max(1);
        for _ in 0..max_passes {
            let mut moved_any = false;
            for &(child_id, parent_id) in &parents {
                let Some(child_pos) = ws.stacking.iter().position(|&id| id == child_id) else {
                    continue;
                };
                let Some(parent_pos) = ws.stacking.iter().position(|&id| id == parent_id) else {
                    continue;
                };
                if child_pos < parent_pos {
                    let removed = ws.stacking.remove(child_pos);
                    // After removing `child_pos < parent_pos`, `parent_id` is now at `parent_pos - 1`.
                    ws.stacking.insert(parent_pos, removed);
                    moved_any = true;
                }
            }
            if !moved_any {
                break;
            }
        }
    }

    fn relayout_workspace(&mut self, ws_id: WorkspaceId) {
        self.enforce_transient_stacking_order(ws_id);
        let work_area = self.work_area_for_workspace(ws_id);
        let full_area = self.full_area_for_workspace(ws_id);
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
                        if win.fullscreen {
                            win.geometry = full_area;
                        } else if win.maximized {
                            win.geometry = work_area;
                        } else {
                            win.geometry =
                                clamp_rect_to_work_area(win.saved_floating_geometry, work_area);
                        }
                    }
                }
            }
            LayoutMode::MasterStack => {
                let mut tiled_ids = Vec::with_capacity(stacking.len());
                for &win_id in stacking.iter().rev() {
                    if let Some(win) = self.window(win_id) {
                        if win.minimized {
                            continue;
                        }
                        if win.fullscreen {
                            if let Some(w_mut) = self.windows.iter_mut().find(|w| w.id == win_id) {
                                w_mut.geometry = full_area;
                            }
                        } else if win.is_floating_transient() {
                            if let Some(w_mut) = self.windows.iter_mut().find(|w| w.id == win_id) {
                                w_mut.geometry = clamp_rect_to_work_area(
                                    w_mut.saved_floating_geometry,
                                    work_area,
                                );
                            }
                        } else {
                            tiled_ids.push(win_id);
                        }
                    }
                }

                let count = tiled_ids.len();
                if count == 0 {
                    return;
                }

                // In MasterStack, the topmost (most recently focused/mapped) non-fullscreen window is Master on the left,
                // and remaining windows stack vertically on the right.
                if count == 1 {
                    let single_rect = Rect::new(
                        work_area.x + gap,
                        work_area.y + gap,
                        (work_area.width - gap * 2).max(100),
                        (work_area.height - gap * 2).max(100),
                    );
                    if let Some(win) = self.windows.iter_mut().find(|w| w.id == tiled_ids[0]) {
                        win.geometry = single_rect;
                    }
                    return;
                }

                let master_w = ((work_area.width - gap * 3) / 2).max(100);
                let stack_w = (work_area.width - gap * 3 - master_w).max(100);
                let master_h = (work_area.height - gap * 2).max(100);

                let master_rect =
                    Rect::new(work_area.x + gap, work_area.y + gap, master_w, master_h);
                if let Some(win) = self.windows.iter_mut().find(|w| w.id == tiled_ids[0]) {
                    win.geometry = master_rect;
                }

                let stack_count = i32::try_from(count - 1).unwrap_or(1);
                let total_stack_gaps = gap * (stack_count + 1);
                let each_h = ((work_area.height - total_stack_gaps) / stack_count).max(60);
                let stack_x = work_area.x + gap * 2 + master_w;

                for (idx, &win_id) in tiled_ids.iter().skip(1).enumerate() {
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

        let g2 = wm.window(w2).map_or_default(|w| w.geometry);
        let g1 = wm.window(w1).map_or_default(|w| w.geometry);
        // Topmost window (w2) is Master on the left; w1 is on the right stack.
        assert!(g2.x < g1.x);
        assert_eq!(g2.width, g1.width);

        // Toggle back to Floating and test LeftHalf snap.
        wm.apply_action(WmAction::ToggleLayoutMode);
        assert_eq!(wm.active_layout_mode(), LayoutMode::Floating);
        wm.apply_action(WmAction::SnapFocused(SnapTarget::LeftHalf));
        let snapped = wm.window(w2).map_or_default(|w| w.geometry);
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
        let geom = wm.window(w1).map_or_default(|w| w.geometry);
        assert!(geom.x >= 1920);
    }

    #[test]
    fn resize_window_enforces_minimum_bounds_and_clears_maximized() {
        let mut wm = WindowManager::new(Rect::new(0, 32, 1280, 700));
        let w1 = wm.next_window_id();
        wm.map_window(w1, None);
        wm.apply_action(WmAction::ToggleMaximizeFocused);
        assert!(wm.window(w1).is_some_and(|w| w.maximized));

        assert!(wm.resize_window(w1, Rect::new(150, 90, 120, 80)));
        let resized = wm.window(w1).cloned();
        assert!(resized.as_ref().is_some_and(|w| !w.maximized));
        assert_eq!(
            resized.as_ref().map(|w| w.geometry),
            Some(Rect::new(150, 90, 240, 160))
        );
        assert_eq!(
            resized.as_ref().map(|w| w.saved_floating_geometry),
            Some(Rect::new(150, 90, 240, 160))
        );
        assert!(wm.resize_window_with_min(w1, Rect::new(150, 90, 480, 320), 640, 500));
        assert_eq!(
            wm.window(w1).map(|w| w.geometry),
            Some(Rect::new(150, 90, 640, 500))
        );
    }

    #[test]
    fn fullscreen_uses_full_output_bounds_and_restores_prior_state() {
        let work = Rect::new(0, 28, 1920, 980);
        let full = Rect::new(0, 0, 1920, 1080);
        let mut wm = WindowManager::new(work);
        wm.upsert_output_with_full_area(OutputId(1), work, full);

        let w1 = wm.next_window_id();
        let initial = wm.map_window(w1, Some((900, 600)));
        assert_eq!(wm.window(w1).map(|w| w.fullscreen), Some(false));

        // Entering fullscreen uses full display bounds (0, 0, 1920, 1080).
        wm.apply_action(WmAction::ToggleFullscreen);
        assert_eq!(wm.window(w1).map(|w| w.fullscreen), Some(true));
        assert_eq!(wm.window(w1).map(|w| w.geometry), Some(full));

        // Exiting fullscreen restores saved floating geometry.
        wm.apply_action(WmAction::ToggleFullscreen);
        assert_eq!(wm.window(w1).map(|w| w.fullscreen), Some(false));
        assert_eq!(wm.window(w1).map(|w| w.geometry), Some(initial));

        // If maximized before fullscreen, exiting fullscreen restores maximized work area.
        wm.apply_action(WmAction::ToggleMaximizeFocused);
        assert_eq!(wm.window(w1).map(|w| w.geometry), Some(work));
        assert_eq!(wm.set_window_fullscreen(w1, true), Some(full));
        assert_eq!(wm.set_window_fullscreen(w1, false), Some(work));
    }

    #[test]
    fn visible_windows_for_output_isolates_per_display_workspaces() {
        let mut wm = WindowManager::new(Rect::new(0, 0, 1920, 1080));
        wm.upsert_output(OutputId(2), Rect::new(1920, 0, 2560, 1440));

        let w1 = wm.next_window_id();
        wm.map_window(w1, Some((800, 600)));

        wm.move_focused_to_next_output();
        let w2 = wm.next_window_id();
        wm.map_window(w2, Some((1000, 700)));

        assert_eq!(
            wm.visible_windows_for_output(OutputId(2))
                .iter()
                .map(|w| w.id)
                .collect::<Vec<_>>(),
            vec![w1, w2]
        );
        assert_eq!(wm.visible_windows_for_output(OutputId(1)).len(), 0);
    }

    #[test]
    fn remove_output_migrates_stacking_and_preserves_saved_floating_size() {
        let mut wm = WindowManager::new(Rect::new(0, 28, 1920, 1052));
        wm.upsert_output(OutputId(2), Rect::new(1920, 0, 2560, 1440));
        wm.set_focused_output(OutputId(2));

        let w1 = wm.next_window_id();
        let initial = wm.map_window(w1, Some((720, 480)));
        assert_eq!(wm.window(w1).map(|w| w.workspace), Some(WorkspaceId(2)));
        let _ = wm.set_window_maximized(w1, true);
        assert_eq!(wm.window(w1).map(|w| w.geometry.width), Some(2560));

        assert!(wm.remove_output(OutputId(2)));
        assert_eq!(wm.focused_output(), OutputId(1));
        let vis: Vec<WindowId> = wm
            .visible_windows_for_output(OutputId(1))
            .iter()
            .map(|w| w.id)
            .collect();
        assert_eq!(vis, vec![w1]);
        assert_eq!(wm.focused_window(), Some(w1));
        assert_eq!(wm.window(w1).map(|w| w.geometry.width), Some(1920));
        // Unmaximizing must still restore the saved 720x480 floating size, not 2560x1440.
        let restored = wm.set_window_maximized(w1, false);
        assert_eq!(
            restored.map(|r| (r.width, r.height)),
            Some((initial.width, initial.height))
        );
    }

    #[test]
    fn minimized_windows_skip_focus_tiling_hit_test_and_cycle() {
        let mut wm = WindowManager::new(Rect::new(0, 28, 1280, 772));
        let w1 = wm.next_window_id();
        let w2 = wm.next_window_id();
        wm.map_window(w1, Some((600, 400)));
        wm.map_window(w2, Some((600, 400)));
        wm.apply_action(WmAction::ToggleLayoutMode);
        assert_eq!(wm.active_layout_mode(), LayoutMode::MasterStack);

        // Minimizing w2 transfers focus to w1 and lets w1 expand to full single-tile width.
        assert!(wm.set_window_minimized(w2, true));
        assert!(!wm.set_window_minimized(w2, true));
        assert_eq!(wm.focused_window(), Some(w1));
        assert_eq!(
            wm.window(w1).map(|w| w.geometry.width),
            Some(1280 - wm.gap_px * 2)
        );
        assert_eq!(wm.cycle_focus(true), Some(w1));

        // Minimizing the last visible window clears focused_window cleanly.
        assert!(wm.set_window_minimized(w1, true));
        assert_eq!(wm.focused_window(), None);
        assert_eq!(wm.window_under_point(100, 100), None);
        assert_eq!(wm.cycle_focus(true), None);

        // Restoring w2 refocuses and raises it.
        assert!(wm.set_window_minimized(w2, false));
        assert_eq!(wm.focused_window(), Some(w2));
    }

    #[test]
    fn direct_maximize_max_size_fullscreen_target_and_migrate_output_work() {
        let mut wm = WindowManager::new(Rect::new(0, 28, 1920, 1052));
        wm.upsert_output_with_full_area(
            OutputId(2),
            Rect::new(1920, 0, 2560, 1440),
            Rect::new(1920, 0, 2560, 1440),
        );

        let w1 = wm.next_window_id();
        let w2 = wm.next_window_id();
        wm.map_window(w1, Some((600, 400)));
        wm.map_window(w2, Some((600, 400)));
        assert_eq!(wm.focused_window(), Some(w2));

        // Maximizing unfocused w1 must not steal focus or raise w1 above w2.
        let _ = wm.set_window_maximized(w1, true);
        assert_eq!(wm.focused_window(), Some(w2));
        let order: Vec<WindowId> = wm
            .visible_windows_for_output(OutputId(1))
            .iter()
            .map(|w| w.id)
            .collect();
        assert_eq!(order, vec![w1, w2]);

        // Enforcing client max_size caps resize bounds.
        assert!(wm.set_window_max_size(w2, Some((800, 600))));
        assert!(wm.resize_window_with_min(w2, Rect::new(100, 100, 1400, 900), 300, 200));
        assert_eq!(
            wm.window(w2).map(|w| w.geometry),
            Some(Rect::new(100, 100, 800, 600))
        );

        // Fullscreen targeting OutputId(2) migrates w2 onto Output 2's full_area.
        let fs = wm.set_window_fullscreen_on_output(w2, true, Some(OutputId(2)));
        assert_eq!(fs, Some(Rect::new(1920, 0, 2560, 1440)));
        assert_eq!(wm.window(w2).map(|w| w.workspace), Some(WorkspaceId(2)));

        // Migrating w1 to Output 2 works cleanly.
        assert!(wm.migrate_window_to_output(w1, OutputId(2)));
        assert_eq!(wm.window(w1).map(|w| w.workspace), Some(WorkspaceId(2)));
    }

    #[test]
    fn snap_window_quarters_and_maximize_preserve_saved_floating_geometry() {
        let work = Rect::new(0, 28, 1280, 772);
        let mut wm = WindowManager::new(work);
        let w1 = wm.next_window_id();
        let initial = wm.map_window(w1, Some((640, 420)));

        // Snapping to Maximize preserves saved_floating_geometry.
        let max_rect = wm.snap_window(w1, SnapTarget::Maximize);
        assert_eq!(max_rect, Some(work));
        assert!(wm.window(w1).is_some_and(|w| w.maximized));
        assert_eq!(
            wm.window(w1).map(|w| w.saved_floating_geometry),
            Some(initial)
        );

        // Quarter snap targets tile into 2x2 non-overlapping quadrants within work_area.
        let tl = compute_snap_rect(work, wm.gap_px, SnapTarget::TopLeft);
        let tr = compute_snap_rect(work, wm.gap_px, SnapTarget::TopRight);
        let bl = compute_snap_rect(work, wm.gap_px, SnapTarget::BottomLeft);
        let br = compute_snap_rect(work, wm.gap_px, SnapTarget::BottomRight);
        assert_eq!(tl.x, work.x + wm.gap_px);
        assert_eq!(tl.y, work.y + wm.gap_px);
        assert!(tr.x >= tl.x + tl.width + wm.gap_px);
        assert!(bl.y >= tl.y + tl.height + wm.gap_px);
        assert_eq!(br.x, tr.x);
        assert_eq!(br.y, bl.y);
        assert_eq!(wm.snap_window(w1, SnapTarget::BottomRight), Some(br));
        assert!(wm.window(w1).is_some_and(|w| !w.maximized));
    }

    #[test]
    fn transient_and_modal_dialogs_enforce_centering_stacking_tiling_migration_and_focus_return() {
        let work = Rect::new(0, 28, 1280, 772);
        let mut wm = WindowManager::new(work);
        wm.upsert_output_with_full_area(
            OutputId(2),
            Rect::new(1280, 0, 1920, 1080),
            Rect::new(1280, 0, 1920, 1080),
        );

        let parent = wm.next_window_id();
        let other = wm.next_window_id();
        let dialog = wm.next_window_id();
        wm.map_window(parent, Some((800, 500)));
        wm.move_window(parent, 200, 140);
        wm.map_window(other, Some((500, 400)));
        wm.map_window(dialog, Some((360, 220)));

        // Set parent + modal and verify cycle rejection.
        assert!(wm.set_window_parent(dialog, Some(parent)));
        assert!(!wm.set_window_parent(parent, Some(dialog)));
        assert_eq!(wm.window(parent).and_then(|w| w.parent), None);
        assert!(wm.set_window_dialog_flags(dialog, true, true));

        // Centering over parent places dialog cleanly inside parent geometry.
        let centered = Rect::new(200 + (800 - 360) / 2, 140 + (500 - 220) / 2, 360, 220);
        assert_eq!(wm.center_window_on_parent(dialog), Some(centered));

        // Clicking/focusing parent redirects focus to active modal and keeps dialog stacked above parent.
        assert!(wm.focus_and_raise(parent));
        assert_eq!(wm.focused_window(), Some(dialog));
        let order: Vec<WindowId> = wm
            .visible_windows_for_output(OutputId(1))
            .iter()
            .map(|w| w.id)
            .collect();
        let parent_pos = order.iter().position(|&id| id == parent);
        let dialog_pos = order.iter().position(|&id| id == dialog);
        assert!(dialog_pos > parent_pos);

        // MasterStack leaves transient dialog floating while tiling parent & other.
        assert_eq!(wm.toggle_layout_mode(), LayoutMode::MasterStack);
        assert_eq!(wm.window(dialog).map(|w| w.geometry), Some(centered));
        assert_ne!(
            wm.window(parent).map(|w| w.geometry.width),
            Some(800),
            "non-dialog parent should be tiled in MasterStack"
        );

        // Moving parent to another workspace brings its transient dialog along.
        assert!(wm.move_window_to_workspace(parent, WorkspaceId(3)));
        assert_eq!(wm.window(parent).map(|w| w.workspace), Some(WorkspaceId(3)));
        assert_eq!(wm.window(dialog).map(|w| w.workspace), Some(WorkspaceId(3)));
        assert_eq!(
            wm.workspace(WorkspaceId(3)).and_then(|ws| ws.focused),
            Some(dialog)
        );

        // Migrating parent to Output 2 brings dialog onto Output 2's workspace.
        assert!(wm.migrate_window_to_output(parent, OutputId(2)));
        assert_eq!(wm.window(parent).map(|w| w.workspace), Some(WorkspaceId(2)));
        assert_eq!(wm.window(dialog).map(|w| w.workspace), Some(WorkspaceId(2)));

        // Closing modal dialog returns focus directly to its parent.
        let next_focus = wm.unmap_window(dialog);
        assert_eq!(next_focus, Some(parent));
    }
}
