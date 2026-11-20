//! Interactive Spotlight application catalog, filtering, and spawning (`RUSTYOS-018b`).

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use tracing::{info, warn};

const MAX_DESKTOP_FILES: usize = 512;
const MAX_QUERY_CHARS: usize = 128;

/// Single discovered or built-in desktop application entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherApp {
    /// Desktop entry identifier (e.g. `org.rustyos.terminal` or `foot.desktop`).
    pub id: String,
    /// Human-readable application name.
    pub name: String,
    /// Secondary description / comment / command hint.
    pub subtitle: String,
    /// Executable command template (`Exec=` field with desktop codes stripped on launch).
    pub exec: String,
    /// Search keywords from `Keywords=` or built-in tags.
    pub keywords: Vec<String>,
    /// Short 1-2 character monogram rendered inside the icon tile.
    pub badge: String,
    /// Optional XDG icon name or filesystem path from `Icon=`.
    pub icon: Option<String>,
}

/// Filtered candidate ready for rendering or launching in Spotlight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherCandidate {
    /// Display title.
    pub name: String,
    /// Display subtitle.
    pub subtitle: String,
    /// Command string to execute on activation.
    pub exec: String,
    /// Short 1-2 char monogram badge.
    pub badge: String,
    /// Optional XDG icon name or filesystem path from `.desktop` `Icon=`.
    pub icon: Option<String>,
    /// `true` when this candidate executes the raw typed command rather than a catalog app.
    pub is_custom_command: bool,
}

/// Live state for the Spotlight launcher inside `rusty-compositor`.
#[derive(Debug)]
pub struct LauncherState {
    /// Current UTF-8 search query.
    pub query: String,
    /// Currently highlighted index inside [`Self::filtered_candidates`].
    pub selected_index: usize,
    /// Application catalog populated from `.desktop` files plus built-in fallbacks.
    pub catalog: Vec<LauncherApp>,
    /// Optional active `XWayland` `DISPLAY` string (e.g. `:0`) exported to child apps (`RUSTYOS-026`).
    pub x11_display: Option<String>,
    /// Spawned child processes reaped non-blockingly via `try_wait`.
    children: Vec<Child>,
}

impl Default for LauncherState {
    fn default() -> Self {
        Self::new()
    }
}

impl LauncherState {
    /// Creates a new [`LauncherState`] discovering system `.desktop` entries and built-in fallbacks.
    #[must_use]
    pub fn new() -> Self {
        let mut catalog = discover_desktop_apps();
        merge_builtin_fallbacks(&mut catalog);
        sort_catalog(&mut catalog);
        Self {
            query: String::new(),
            selected_index: 0,
            catalog,
            x11_display: None,
            children: Vec::new(),
        }
    }

    /// Creates a [`LauncherState`] from an explicit catalog (primarily for deterministic tests).
    #[must_use]
    pub fn with_catalog(mut catalog: Vec<LauncherApp>) -> Self {
        sort_catalog(&mut catalog);
        Self {
            query: String::new(),
            selected_index: 0,
            catalog,
            x11_display: None,
            children: Vec::new(),
        }
    }

    /// Clears the current search query and resets selection to the top item.
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected_index = 0;
    }

    /// Appends printable UTF-8 text to the query and resets selection to the best match.
    pub fn push_text(&mut self, text: &str) {
        let mut changed = false;
        for ch in text.chars() {
            if ch.is_control() || ch == '\0' {
                continue;
            }
            if self.query.chars().count() >= MAX_QUERY_CHARS {
                break;
            }
            self.query.push(ch);
            changed = true;
        }
        if changed {
            self.selected_index = 0;
        }
    }

    /// Deletes the last UTF-8 character from the query and resets selection.
    pub fn pop_char(&mut self) {
        if self.query.pop().is_some() {
            self.selected_index = 0;
        }
    }

    /// Returns filtered and relevance-sorted candidates for the current [`Self::query`].
    #[must_use]
    pub fn filtered_candidates(&self) -> Vec<LauncherCandidate> {
        let needle = self.query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return self
                .catalog
                .iter()
                .map(|app| LauncherCandidate {
                    name: app.name.clone(),
                    subtitle: app.subtitle.clone(),
                    exec: app.exec.clone(),
                    badge: app.badge.clone(),
                    icon: app.icon.clone(),
                    is_custom_command: false,
                })
                .collect();
        }

        let mut scored: Vec<(u8, &LauncherApp)> = self
            .catalog
            .iter()
            .filter_map(|app| {
                let score = match_score(app, &needle);
                (score > 0).then_some((score, app))
            })
            .collect();

        scored.sort_by(|(score_a, app_a), (score_b, app_b)| {
            score_b.cmp(score_a).then_with(|| {
                app_a
                    .name
                    .to_ascii_lowercase()
                    .cmp(&app_b.name.to_ascii_lowercase())
            })
        });

        let mut results: Vec<LauncherCandidate> = scored
            .into_iter()
            .map(|(_, app)| LauncherCandidate {
                name: app.name.clone(),
                subtitle: app.subtitle.clone(),
                exec: app.exec.clone(),
                badge: app.badge.clone(),
                icon: app.icon.clone(),
                is_custom_command: false,
            })
            .collect();

        if results.is_empty() {
            let trimmed = self.query.trim();
            if !trimmed.is_empty() {
                results.push(LauncherCandidate {
                    name: format!("Run \"{trimmed}\""),
                    subtitle: "Execute command in RustyOS session".to_owned(),
                    exec: trimmed.to_owned(),
                    badge: ">_".to_owned(),
                    icon: None,
                    is_custom_command: true,
                });
            }
        }

        results
    }

    /// Returns `(window_start_index, visible_slice)` for a viewport of at most `max_rows`.
    #[must_use]
    pub fn visible_window(&self, max_rows: usize) -> (usize, Vec<LauncherCandidate>) {
        if max_rows == 0 {
            return (0, Vec::new());
        }
        let all = self.filtered_candidates();
        if all.is_empty() {
            return (0, Vec::new());
        }
        let selected = self.selected_index.min(all.len() - 1);
        let start = if all.len() <= max_rows {
            0
        } else {
            let max_start = all.len() - max_rows;
            selected.saturating_sub(max_rows - 1).min(max_start)
        };
        let end = (start + max_rows).min(all.len());
        (start, all[start..end].to_vec())
    }

    /// Moves [`Self::selected_index`] up (`-1`) or down (`+1`) with wrap-around.
    pub fn move_selection(&mut self, delta: isize) {
        let len = self.filtered_candidates().len();
        if len == 0 {
            self.selected_index = 0;
            return;
        }
        let cur = self.selected_index.min(len - 1);
        let len_isize = isize::try_from(len).unwrap_or(isize::MAX);
        let cur_isize = isize::try_from(cur).unwrap_or(0);
        let next = (cur_isize + delta).rem_euclid(len_isize);
        self.selected_index = usize::try_from(next).unwrap_or(0);
    }

    /// Returns the command string of the currently selected candidate, if any.
    #[must_use]
    pub fn selected_exec(&self) -> Option<String> {
        let candidates = self.filtered_candidates();
        if candidates.is_empty() {
            return None;
        }
        let idx = self.selected_index.min(candidates.len() - 1);
        Some(candidates[idx].exec.clone())
    }

    /// Returns the command string for a visible row offset `row_offset` (`0..max_rows`).
    #[must_use]
    pub fn exec_at_visible_row(&self, max_rows: usize, row_offset: usize) -> Option<String> {
        let (_start, visible) = self.visible_window(max_rows);
        visible.get(row_offset).map(|item| item.exec.clone())
    }

    /// Finds and spawns the first available application matching `preferred_bins` or `keywords` (`RUSTYOS-023`).
    pub fn launch_preferred(
        &mut self,
        preferred_bins: &[&str],
        wayland_display: &str,
        ipc_socket: &Path,
    ) -> bool {
        if let Some(cmd) = self.find_available_command(preferred_bins) {
            return self
                .launch_command(&cmd, wayland_display, ipc_socket)
                .is_ok();
        }
        false
    }

    /// Resolves the best XDG icon name/path for a window using its `app_id`/`WM_CLASS` and title (`RUSTYOS-033`, `RUSTYOS-080`).
    #[must_use]
    pub fn icon_for_app_hints(&self, app_id: Option<&str>, title: &str) -> Option<String> {
        let hint = app_id.map(str::trim).filter(|s| !s.is_empty());
        if let Some(raw_hint) = hint {
            let lower = raw_hint.to_ascii_lowercase();
            let short = lower.rsplit('.').next().unwrap_or(&lower);

            // Pass 1: exact app ID / desktop file stem match takes strict precedence (e.g. google-chrome.desktop -> google-chrome).
            for app in &self.catalog {
                let id_stem = app
                    .id
                    .strip_suffix(".desktop")
                    .unwrap_or(&app.id)
                    .to_ascii_lowercase();
                if (id_stem == lower || id_stem == short)
                    && let Some(ref icon) = app.icon
                {
                    return Some(icon.clone());
                }
            }

            // Pass 2: match executable or app name, but ignore Chrome/Chromium PWA web apps (--app-id=) when searching for generic binaries.
            for app in &self.catalog {
                if app.exec.contains("--app-id=") {
                    continue;
                }
                let bin = exec_basename(&app.exec);
                let name_lower = app.name.to_ascii_lowercase();
                if (bin == lower || bin == short || name_lower == lower)
                    && let Some(ref icon) = app.icon
                {
                    return Some(icon.clone());
                }
            }

            // Pass 3: any match including PWAs if nothing else matched.
            for app in &self.catalog {
                let bin = exec_basename(&app.exec);
                let name_lower = app.name.to_ascii_lowercase();
                if (bin == lower || bin == short || name_lower == lower)
                    && let Some(ref icon) = app.icon
                {
                    return Some(icon.clone());
                }
            }

            return Some(raw_hint.to_owned());
        }

        let title_lower = title.trim().to_ascii_lowercase();
        if !title_lower.is_empty() {
            for app in &self.catalog {
                if app.name.to_ascii_lowercase() == title_lower
                    && let Some(ref icon) = app.icon
                {
                    return Some(icon.clone());
                }
            }
        }
        None
    }

    /// Resolves the icon name for the first matching installed preferred binary (used by pinned Dock slots).
    #[must_use]
    pub fn preferred_icon<'a>(&'a self, preferred_bins: &[&str], fallback: &'a str) -> &'a str {
        for bin in preferred_bins {
            let needle = bin.to_ascii_lowercase();
            // Pass 1: exact desktop ID stem match
            for app in &self.catalog {
                let id_stem = app
                    .id
                    .strip_suffix(".desktop")
                    .unwrap_or(&app.id)
                    .to_ascii_lowercase();
                if id_stem == needle
                    && let Some(ref icon) = app.icon
                {
                    return icon.as_str();
                }
            }
            // Pass 2: executable basename match, skipping PWA web apps
            for app in &self.catalog {
                if app.exec.contains("--app-id=") {
                    continue;
                }
                if exec_basename(&app.exec) == needle
                    && let Some(ref icon) = app.icon
                {
                    return icon.as_str();
                }
            }
        }
        fallback
    }

    #[must_use]
    pub(crate) fn find_available_command(&self, preferred_bins: &[&str]) -> Option<String> {
        self.find_available_command_with(preferred_bins, command_exists_on_path)
    }

    fn find_available_command_with<F>(&self, preferred_bins: &[&str], exists: F) -> Option<String>
    where
        F: Fn(&str) -> bool,
    {
        for &preferred in preferred_bins {
            for app in &self.catalog {
                if app.exec.contains("--app-id=") {
                    continue;
                }
                let bin = exec_basename(&app.exec);
                if bin.eq_ignore_ascii_case(preferred) && exists(&app.exec) {
                    return Some(clean_desktop_exec(&app.exec));
                }
            }
            if exists(preferred) {
                return Some(preferred.to_owned());
            }
        }
        None
    }

    /// Preferred executable candidates for canonical built-in Dock keys (`RUSTYOS-073`).
    #[must_use]
    pub(crate) fn canonical_preferred_bins(app_key: &str) -> Option<&'static [&'static str]> {
        const FILES: &[&str] = &["rusty-files"];
        const SETTINGS: &[&str] = &["rusty-settings"];
        const TERMINAL: &[&str] = &[
            "rusty-terminal",
            "foot",
            "kitty",
            "alacritty",
            "wezterm",
            "ghostty",
            "weston-terminal",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
        ];
        const BROWSER: &[&str] = &[
            "firefox",
            "chromium",
            "google-chrome",
            "google-chrome-stable",
            "brave-browser",
            "epiphany",
            "falkon",
        ];
        const NOTES: &[&str] = &[
            "gnome-text-editor",
            "gedit",
            "kate",
            "mousepad",
            "featherpad",
            "obsidian",
        ];
        const CHROME: &[&str] = &["google-chrome", "google-chrome-stable", "chromium"];
        match app_key.trim().to_ascii_lowercase().as_str() {
            "files" | "rusty-files" => Some(FILES),
            "settings" | "rusty-settings" | "system-settings" | "updater" | "rusty-updater" => {
                Some(SETTINGS)
            }
            "terminal" | "rusty-terminal" => Some(TERMINAL),
            "browser" => Some(BROWSER),
            "notes" => Some(NOTES),
            "google-chrome" | "google-chrome-stable" | "chrome" => Some(CHROME),
            _ => None,
        }
    }

    /// Resolves a launch command for either a canonical Dock key or a custom `.desktop`/binary key.
    #[must_use]
    pub(crate) fn find_command_for_dock_key(&self, app_key: &str) -> Option<String> {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        if let Some(bins) = Self::canonical_preferred_bins(&key) {
            return self.find_available_command(bins);
        }
        let dot_suffix = format!(".{key}");
        for app in &self.catalog {
            let id_lower = app.id.trim_end_matches(".desktop").to_ascii_lowercase();
            let bin = exec_basename(&app.exec);
            let matched = app.id.eq_ignore_ascii_case(&key)
                || id_lower == key
                || id_lower.ends_with(&dot_suffix)
                || bin.eq_ignore_ascii_case(&key)
                || app.name.eq_ignore_ascii_case(&key);
            if matched && command_exists_on_path(&app.exec) {
                return Some(clean_desktop_exec(&app.exec));
            }
        }
        if command_exists_on_path(&key) {
            return Some(key);
        }
        key.rsplit('.')
            .next()
            .filter(|short| *short != key && command_exists_on_path(short))
            .map(ToOwned::to_owned)
    }

    /// Launches a canonical or custom Dock application key.
    pub(crate) fn launch_by_dock_key(
        &mut self,
        app_key: &str,
        wayland_display: &str,
        ipc_socket: &Path,
    ) -> bool {
        self.find_command_for_dock_key(app_key).is_some_and(|cmd| {
            self.launch_command(&cmd, wayland_display, ipc_socket)
                .is_ok()
        })
    }

    /// Resolves an XDG icon name/path for a canonical or custom Dock key.
    #[must_use]
    pub(crate) fn icon_for_dock_key(&self, app_key: &str) -> Option<String> {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        if let Some(bins) = Self::canonical_preferred_bins(&key) {
            let icon = self.preferred_icon(bins, "");
            if !icon.is_empty() {
                return Some(icon.to_owned());
            }
        }
        self.icon_for_app_hints(Some(&key), &key)
    }

    /// Non-blockingly reaps any exited child processes spawned from Spotlight.
    pub fn reap_children(&mut self) {
        self.children
            .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
    }

    /// Spawns `program` with explicit `args`, resolving `PATH` or sibling workspace binaries.
    ///
    /// # Errors
    ///
    /// Returns a human-readable error string if `program` is empty or fails to spawn.
    pub fn launch_program_with_args(
        &mut self,
        program: &str,
        args: &[&str],
        wayland_display: &str,
        ipc_socket: &Path,
    ) -> Result<u32, String> {
        self.reap_children();
        let trimmed = program.trim();
        if trimmed.is_empty() {
            return Err("empty application command".to_owned());
        }
        let arg_strings: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        self.spawn_resolved_program(trimmed, &arg_strings, wayland_display, ipc_socket)
    }

    /// Spawns a desktop `Exec=` command asynchronously with `WAYLAND_DISPLAY` and `RUSTYOS_IPC_SOCKET`.
    ///
    /// # Errors
    ///
    /// Returns a human-readable error string if the command is empty or fails to spawn.
    pub fn launch_command(
        &mut self,
        command: &str,
        wayland_display: &str,
        ipc_socket: &Path,
    ) -> Result<u32, String> {
        self.reap_children();
        let tokens = parse_exec_argv(command);
        let Some((program, args)) = tokens.split_first() else {
            return Err("empty application command".to_owned());
        };
        self.spawn_resolved_program(program, args, wayland_display, ipc_socket)
    }

    fn spawn_resolved_program(
        &mut self,
        program: &str,
        args: &[String],
        wayland_display: &str,
        ipc_socket: &Path,
    ) -> Result<u32, String> {
        let resolved = resolve_program_path(program);
        let program_path = resolved.as_deref().unwrap_or_else(|| Path::new(program));
        let mut cmd = Command::new(program_path);
        cmd.args(args)
            .env("WAYLAND_DISPLAY", wayland_display)
            .env("RUSTYOS_IPC_SOCKET", ipc_socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(disp) = &self.x11_display {
            cmd.env("DISPLAY", disp);
        }
        let child = cmd
            .spawn()
            .map_err(|err| format!("failed to launch '{program}': {err}"))?;

        let pid = child.id();
        info!(
            program = %program,
            resolved = %program_path.display(),
            pid,
            wayland_display = %wayland_display,
            "Launched application"
        );
        self.children.push(child);
        Ok(pid)
    }

    /// Launches whichever candidate is currently selected.
    ///
    /// # Errors
    ///
    /// Returns an error if no candidate exists or process spawn fails.
    pub fn launch_selected(
        &mut self,
        wayland_display: &str,
        ipc_socket: &Path,
    ) -> Result<u32, String> {
        let Some(exec) = self.selected_exec() else {
            return Err("no matching application to launch".to_owned());
        };
        let res = self.launch_command(&exec, wayland_display, ipc_socket);
        if let Err(err) = &res {
            warn!(error = %err, exec = %exec, "Spotlight launch failed");
        }
        res
    }
}

fn match_score(app: &LauncherApp, needle: &str) -> u8 {
    let name_lower = app.name.to_ascii_lowercase();
    if name_lower == needle {
        return 120;
    }
    if name_lower.starts_with(needle) {
        return 100;
    }
    if name_lower
        .split_whitespace()
        .any(|word| word.starts_with(needle))
    {
        return 80;
    }
    if name_lower.contains(needle) {
        return 60;
    }
    if app.exec.to_ascii_lowercase().contains(needle) {
        return 45;
    }
    if app
        .keywords
        .iter()
        .any(|kw| kw.to_ascii_lowercase().contains(needle))
    {
        return 30;
    }
    if app.subtitle.to_ascii_lowercase().contains(needle) {
        return 15;
    }
    0
}

fn priority_rank(app: &LauncherApp) -> u8 {
    let bin = exec_basename(&app.exec);
    match bin.as_str() {
        "rusty-terminal" => 0,
        "rusty-files" => 1,
        "rusty-settings" => 2,
        "firefox" | "chromium" | "google-chrome" | "google-chrome-stable" | "brave" => 3,
        "foot" | "kitty" | "alacritty" | "wezterm" | "ghostty" | "weston-terminal" | "konsole"
        | "gnome-terminal" | "xfce4-terminal" => 4,
        "es2gears_wayland" | "glmark2-wayland" => 5,
        "rusty-installer" => 6,
        _ => 10,
    }
}

fn sort_catalog(catalog: &mut [LauncherApp]) {
    catalog.sort_by(|a, b| {
        priority_rank(a).cmp(&priority_rank(b)).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
}

fn exec_basename(exec: &str) -> String {
    let first = parse_exec_argv(exec).into_iter().next().unwrap_or_default();
    Path::new(&first)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&first)
        .to_ascii_lowercase()
}

pub(crate) fn clean_desktop_exec(exec: &str) -> String {
    parse_exec_argv(exec)
        .into_iter()
        .map(|arg| {
            if arg.chars().any(char::is_whitespace) || arg.contains(['"', '\'', '\\']) {
                format!("'{}'", arg.replace('\'', "'\\''"))
            } else {
                arg
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn command_exists_on_path(exec: &str) -> bool {
    parse_exec_argv(exec)
        .into_iter()
        .next()
        .is_some_and(|program| resolve_program_path(&program).is_some())
}

pub(crate) fn resolve_program_path(program: &str) -> Option<PathBuf> {
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    resolve_program_path_with(program, env::var_os("PATH").as_deref(), exe_dir.as_deref())
}

fn resolve_program_path_with(
    program: &str,
    path_env: Option<&std::ffi::OsStr>,
    exe_dir: Option<&Path>,
) -> Option<PathBuf> {
    let trimmed = program.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() || trimmed.contains('/') {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    // Prefer sibling binaries next to the running compositor (`target/debug`, `/usr/bin`)
    // before falling back to `PATH`, so dev `--winit` runs use matching freshly built apps.
    if let Some(dir) = exe_dir {
        let sibling = dir.join(candidate);
        if sibling.is_file() {
            return Some(sibling);
        }
        if dir.file_name() == Some(std::ffi::OsStr::new("deps"))
            && let Some(target_dir) = dir.parent()
        {
            let parent_sibling = target_dir.join(candidate);
            if parent_sibling.is_file() {
                return Some(parent_sibling);
            }
        }
    }
    if let Some(paths) = path_env {
        for dir in env::split_paths(paths) {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

fn discover_desktop_apps() -> Vec<LauncherApp> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        if !xdg_data_home.trim().is_empty() {
            dirs.push(PathBuf::from(xdg_data_home).join("applications"));
        }
    } else if let Ok(home) = env::var("HOME")
        && !home.trim().is_empty()
    {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    dirs.push(PathBuf::from("/usr/local/share/applications"));
    dirs.push(PathBuf::from("/usr/share/applications"));

    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut apps: Vec<LauncherApp> = Vec::new();

    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if apps.len() >= MAX_DESKTOP_FILES {
                break;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            let id = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("app.desktop")
                .to_owned();
            if let Some(app) = parse_desktop_entry(&contents, &id) {
                let dedupe_key = format!(
                    "{}::{}",
                    app.name.to_ascii_lowercase(),
                    exec_basename(&app.exec)
                );
                if seen_keys.insert(dedupe_key) {
                    apps.push(app);
                }
            }
        }
    }

    apps
}

fn merge_builtin_fallbacks(catalog: &mut Vec<LauncherApp>) {
    let existing_bins: HashSet<String> =
        catalog.iter().map(|app| exec_basename(&app.exec)).collect();

    let defaults = [
        LauncherApp {
            id: "org.rustyos.terminal".to_owned(),
            name: "Terminal".to_owned(),
            subtitle: "Command line shell (RustyOS GPU Terminal)".to_owned(),
            exec: "rusty-terminal".to_owned(),
            keywords: vec![
                "terminal".to_owned(),
                "shell".to_owned(),
                "console".to_owned(),
                "cli".to_owned(),
            ],
            badge: ">_".to_owned(),
            icon: Some("utilities-terminal".to_owned()),
        },
        LauncherApp {
            id: "org.rustyos.files".to_owned(),
            name: "Files".to_owned(),
            subtitle: "Browse folders & documents (RustyOS Files)".to_owned(),
            exec: "rusty-files".to_owned(),
            keywords: vec!["folder".to_owned(), "manager".to_owned()],
            badge: "Fi".to_owned(),
            icon: Some("system-file-manager".to_owned()),
        },
        LauncherApp {
            id: "org.rustyos.updater".to_owned(),
            name: "Software Update".to_owned(),
            subtitle: "Update Arch Linux & RustyOS packages".to_owned(),
            exec: "rusty-settings --page updates".to_owned(),
            keywords: vec![
                "update".to_owned(),
                "upgrade".to_owned(),
                "pacman".to_owned(),
                "arch".to_owned(),
            ],
            badge: "Up".to_owned(),
            icon: Some("system-software-update".to_owned()),
        },
        LauncherApp {
            id: "org.rustyos.settings".to_owned(),
            name: "System Settings".to_owned(),
            subtitle: "Appearance, displays, network & sound (RustyOS)".to_owned(),
            exec: "rusty-settings".to_owned(),
            keywords: vec![
                "settings".to_owned(),
                "preferences".to_owned(),
                "appearance".to_owned(),
                "theme".to_owned(),
                "display".to_owned(),
            ],
            badge: "Se".to_owned(),
            icon: Some("preferences-system".to_owned()),
        },
        LauncherApp {
            id: "org.rustyos.installer".to_owned(),
            name: "Install RustyOS".to_owned(),
            subtitle: "Install RustyOS to disk or run Setup Assistant".to_owned(),
            exec: "rusty-installer --install".to_owned(),
            keywords: vec![
                "install".to_owned(),
                "installer".to_owned(),
                "setup".to_owned(),
                "oobe".to_owned(),
                "disk".to_owned(),
            ],
            badge: "In".to_owned(),
            icon: Some("system-software-install".to_owned()),
        },
        LauncherApp {
            id: "org.rustyos.gears".to_owned(),
            name: "ES2 Gears Demo".to_owned(),
            subtitle: "OpenGL ES Wayland benchmark".to_owned(),
            exec: "es2gears_wayland".to_owned(),
            keywords: vec!["opengl".to_owned(), "gl".to_owned(), "benchmark".to_owned()],
            badge: "GL".to_owned(),
            icon: Some("glxgears".to_owned()),
        },
    ];

    for item in defaults {
        if !existing_bins.contains(&exec_basename(&item.exec)) {
            catalog.push(item);
        }
    }
}

/// Parses a single XDG `.desktop` file `[Desktop Entry]` section.
#[must_use]
pub fn parse_desktop_entry(contents: &str, id: &str) -> Option<LauncherApp> {
    let mut in_desktop_entry = false;
    let mut entry_type: Option<String> = None;
    let mut name: Option<String> = None;
    let mut comment: Option<String> = None;
    let mut generic_name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut icon: Option<String> = None;
    let mut keywords: Vec<String> = Vec::new();
    let mut no_display = false;
    let mut hidden = false;

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if in_desktop_entry {
                break;
            }
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim();
        match key {
            "Type" => entry_type = Some(val.to_owned()),
            "Name" => name = Some(val.to_owned()),
            "Comment" => comment = Some(val.to_owned()),
            "GenericName" => generic_name = Some(val.to_owned()),
            "Exec" => exec = Some(val.to_owned()),
            "Icon" => icon = (!val.is_empty()).then(|| val.to_owned()),
            "Keywords" => {
                keywords = val
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            "NoDisplay" => no_display = val.eq_ignore_ascii_case("true"),
            "Hidden" => hidden = val.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    if no_display || hidden {
        return None;
    }
    if entry_type.as_deref() != Some("Application") {
        return None;
    }
    let name = name.filter(|s| !s.is_empty())?;
    let exec = exec.filter(|s| !parse_exec_argv(s).is_empty())?;
    let subtitle = comment
        .or(generic_name)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| exec_basename(&exec));
    let badge = make_badge(&name, &exec);

    Some(LauncherApp {
        id: id.to_owned(),
        name,
        subtitle,
        exec,
        keywords,
        badge,
        icon,
    })
}

/// Splits an XDG `Exec=` command string into program + arguments, stripping `%f`/`%u`/etc. field codes.
#[must_use]
pub fn parse_exec_argv(exec: &str) -> Vec<String> {
    let mut raw_tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut token_started = false;

    for ch in exec.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            token_started = true;
            continue;
        }
        if ch == '\\' && !in_single {
            escaped = true;
            token_started = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            token_started = true;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            token_started = true;
            continue;
        }
        if ch.is_whitespace() && !in_single && !in_double {
            if token_started || !current.is_empty() {
                raw_tokens.push(std::mem::take(&mut current));
                token_started = false;
            }
            continue;
        }
        current.push(ch);
        token_started = true;
    }
    if token_started || !current.is_empty() {
        raw_tokens.push(current);
    }

    raw_tokens
        .into_iter()
        .filter_map(|tok| {
            if matches!(
                tok.as_str(),
                "%f" | "%F"
                    | "%u"
                    | "%U"
                    | "%d"
                    | "%D"
                    | "%n"
                    | "%N"
                    | "%i"
                    | "%c"
                    | "%k"
                    | "@@u"
                    | "@@"
            ) {
                None
            } else {
                let cleaned = tok.replace("%%", "%");
                (!cleaned.is_empty()).then_some(cleaned)
            }
        })
        .collect()
}

fn make_badge(name: &str, exec: &str) -> String {
    let bin = exec_basename(exec);
    if matches!(
        bin.as_str(),
        "rusty-terminal"
            | "foot"
            | "kitty"
            | "alacritty"
            | "wezterm"
            | "ghostty"
            | "weston-terminal"
    ) {
        return ">_".to_owned();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.len() >= 2 {
        let mut out = String::new();
        if let Some(c1) = words[0].chars().next() {
            out.push(c1.to_ascii_uppercase());
        }
        if let Some(c2) = words[1].chars().next() {
            out.push(c2.to_ascii_uppercase());
        }
        if !out.is_empty() {
            return out;
        }
    }
    name.chars().take(2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_desktop_entry_and_strips_field_codes() -> Result<(), String> {
        let desktop = r"
[Desktop Entry]
Type=Application
Name=Firefox Web Browser
Comment=Browse the World Wide Web
Exec=/usr/lib/firefox/firefox --new-window %U
Icon=firefox
Keywords=Internet;WWW;Browser;
NoDisplay=false

[Desktop Action new-private-window]
Name=New Private Window
Exec=firefox --private-window
";
        let app = parse_desktop_entry(desktop, "firefox.desktop")
            .ok_or_else(|| "valid desktop entry should parse".to_owned())?;
        assert_eq!(app.name, "Firefox Web Browser");
        assert_eq!(app.subtitle, "Browse the World Wide Web");
        assert_eq!(app.icon.as_deref(), Some("firefox"));
        assert_eq!(app.keywords, vec!["Internet", "WWW", "Browser"]);
        assert_eq!(
            parse_exec_argv(&app.exec),
            vec!["/usr/lib/firefox/firefox", "--new-window"]
        );
        Ok(())
    }

    #[test]
    fn skips_nodisplay_and_non_application_entries() {
        let hidden = "[Desktop Entry]\nType=Application\nName=Hidden\nExec=foo\nNoDisplay=true\n";
        assert!(parse_desktop_entry(hidden, "hidden.desktop").is_none());

        let link = "[Desktop Entry]\nType=Link\nName=Docs\nURL=https://example.com\n";
        assert!(parse_desktop_entry(link, "link.desktop").is_none());
    }

    #[test]
    fn filtering_scoring_selection_and_custom_command_fallback_work() {
        let mut state = LauncherState::with_catalog(vec![
            LauncherApp {
                id: "1".to_owned(),
                name: "Files".to_owned(),
                subtitle: "File Manager".to_owned(),
                exec: "rusty-files".to_owned(),
                keywords: vec!["folder".to_owned()],
                badge: "Fi".to_owned(),
                icon: Some("system-file-manager".to_owned()),
            },
            LauncherApp {
                id: "2".to_owned(),
                name: "Terminal".to_owned(),
                subtitle: "Shell".to_owned(),
                exec: "rusty-terminal".to_owned(),
                keywords: vec!["console".to_owned()],
                badge: ">_".to_owned(),
                icon: Some("utilities-terminal".to_owned()),
            },
            LauncherApp {
                id: "3".to_owned(),
                name: "ES2 Gears Demo".to_owned(),
                subtitle: "Benchmark".to_owned(),
                exec: "es2gears_wayland".to_owned(),
                keywords: vec!["opengl".to_owned()],
                badge: "EG".to_owned(),
                icon: None,
            },
            LauncherApp {
                id: "4".to_owned(),
                name: "Audio Mixer".to_owned(),
                subtitle: "Volume Control".to_owned(),
                exec: "pavucontrol".to_owned(),
                keywords: vec!["sound".to_owned()],
                badge: "AM".to_owned(),
                icon: None,
            },
        ]);

        assert_eq!(state.filtered_candidates().len(), 4);
        assert_eq!(state.selected_exec().as_deref(), Some("rusty-terminal"));

        state.move_selection(1);
        assert_eq!(state.selected_index, 1);
        state.move_selection(-2); // wraps around 4 items: 1 - 2 mod 4 = 3
        assert_eq!(state.selected_index, 3);

        // Windowed viewport scrolling keeps selected index visible
        let (start, win) = state.visible_window(3);
        assert_eq!(start, 1);
        assert_eq!(win.len(), 3);

        // Query filtering matches keyword & resets selection
        state.push_text("open");
        let filtered = state.filtered_candidates();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].exec, "es2gears_wayland");
        assert_eq!(state.selected_index, 0);

        // Popping chars and typing arbitrary binary triggers custom command fallback
        state.reset();
        state.push_text("weston-terminal");
        let fallback = state.filtered_candidates();
        assert_eq!(fallback.len(), 1);
        assert!(fallback[0].is_custom_command);
        assert_eq!(fallback[0].exec, "weston-terminal");
    }

    #[test]
    fn resolves_sibling_rusty_files_binary_without_path_entry() -> Result<(), std::io::Error> {
        let base = std::env::temp_dir().join(format!("rustyos-bin-resolve-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let deps = base.join("deps");
        let path_dir = base.join("path_bin");
        fs::create_dir_all(&deps)?;
        fs::create_dir_all(&path_dir)?;
        let bin = base.join("rusty-files");
        let stale_path_bin = path_dir.join("rusty-files");
        fs::write(&bin, b"#!/bin/sh\n")?;
        fs::write(&stale_path_bin, b"#!/bin/sh\n")?;

        let resolved_direct = resolve_program_path_with("rusty-files", None, Some(&base));
        assert_eq!(resolved_direct.as_deref(), Some(bin.as_path()));

        let resolved_over_path =
            resolve_program_path_with("rusty-files", Some(path_dir.as_os_str()), Some(&base));
        assert_eq!(resolved_over_path.as_deref(), Some(bin.as_path()));

        let resolved_from_deps = resolve_program_path_with("rusty-files", None, Some(&deps));
        assert_eq!(resolved_from_deps.as_deref(), Some(bin.as_path()));

        let _ = fs::remove_dir_all(&base);
        Ok(())
    }

    #[test]
    fn prioritizes_native_app_id_and_ignores_pwa_when_resolving_browser_icons() {
        let mut launcher = LauncherState::new();
        launcher.catalog = vec![
            LauncherApp {
                id: "chrome-pommaclcbfghclhalboakcipcmmndhcj-Default.desktop".to_owned(),
                name: "Google Chat".to_owned(),
                subtitle: String::new(),
                exec: "/opt/google/chrome/google-chrome --profile-directory=Default --app-id=pommaclcbfghclhalboakcipcmmndhcj".to_owned(),
                keywords: Vec::new(),
                badge: "GC".to_owned(),
                icon: Some("chrome-pommaclcbfghclhalboakcipcmmndhcj-Default".to_owned()),
            },
            LauncherApp {
                id: "google-chrome.desktop".to_owned(),
                name: "Google Chrome".to_owned(),
                subtitle: "Web Browser".to_owned(),
                exec: "/usr/bin/google-chrome-stable %U".to_owned(),
                keywords: Vec::new(),
                badge: "GC".to_owned(),
                icon: Some("google-chrome".to_owned()),
            },
        ];

        let icon = launcher.icon_for_app_hints(Some("google-chrome"), "Google Chrome");
        assert_eq!(icon.as_deref(), Some("google-chrome"));

        let dock_icon = launcher.icon_for_dock_key("google-chrome");
        assert_eq!(dock_icon.as_deref(), Some("google-chrome"));
    }

    #[test]
    fn terminal_dock_key_prefers_rusty_terminal_with_fallback() {
        let bins = LauncherState::canonical_preferred_bins("terminal").unwrap_or(&[]);
        assert_eq!(bins.first().copied(), Some("rusty-terminal"));
        assert!(bins.contains(&"foot"));

        // Even if third-party terminals sort alphabetically before "Terminal",
        // `find_available_command_with` and Spotlight ranking must pick `rusty-terminal` first.
        let launcher = LauncherState::with_catalog(vec![
            LauncherApp {
                id: "Alacritty.desktop".to_owned(),
                name: "Alacritty".to_owned(),
                subtitle: "Terminal".to_owned(),
                exec: "/usr/bin/alacritty".to_owned(),
                keywords: Vec::new(),
                badge: ">_".to_owned(),
                icon: Some("Alacritty".to_owned()),
            },
            LauncherApp {
                id: "foot.desktop".to_owned(),
                name: "Foot".to_owned(),
                subtitle: "Wayland Terminal".to_owned(),
                exec: "/usr/bin/foot".to_owned(),
                keywords: Vec::new(),
                badge: ">_".to_owned(),
                icon: Some("foot".to_owned()),
            },
            LauncherApp {
                id: "org.rustyos.terminal.desktop".to_owned(),
                name: "Terminal".to_owned(),
                subtitle: "RustyOS Terminal".to_owned(),
                exec: "rusty-terminal".to_owned(),
                keywords: vec!["terminal".to_owned()],
                badge: ">_".to_owned(),
                icon: Some("utilities-terminal".to_owned()),
            },
        ]);
        assert_eq!(
            launcher.catalog.first().map(|a| exec_basename(&a.exec)),
            Some("rusty-terminal".to_owned())
        );
        let chosen = launcher.find_available_command_with(bins, |_| true);
        assert_eq!(chosen.as_deref(), Some("rusty-terminal"));
    }
}
