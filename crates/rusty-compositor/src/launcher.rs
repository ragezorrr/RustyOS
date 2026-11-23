//! Interactive Spotlight application catalog, filtering, and spawning (`RUSTYOS-018b`).

use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;

use tracing::{info, warn};

const MAX_DESKTOP_FILES: usize = 1024;
const MAX_QUERY_CHARS: usize = 128;

/// Callback invoked when a background `.desktop` catalog rescan finishes (`RUSTYOS-164`).
pub type CatalogReloadCallback = Arc<dyn Fn() + Send + Sync>;

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
    /// Supported MIME types from `.desktop` `MimeType=` (`RUSTYOS-163`).
    pub mime_types: Vec<String>,
}

/// Filtered candidate ready for rendering or launching in Spotlight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherCandidate {
    /// Canonical Dock application key for drag-to-pin (`None` for raw custom commands) (`RUSTYOS-163`).
    pub app_key: Option<String>,
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
pub struct LauncherState {
    /// Current UTF-8 search query.
    pub query: String,
    /// Currently highlighted index inside [`Self::filtered_candidates`].
    pub selected_index: usize,
    /// Application catalog populated from `.desktop` files plus built-in fallbacks.
    pub catalog: Vec<LauncherApp>,
    /// Optional active `XWayland` `DISPLAY` string (e.g. `:0`) exported to child apps (`RUSTYOS-026`).
    pub x11_display: Option<String>,
    /// Monotonic sequence counter for `systemd --user` transient scopes (`RUSTYOS-155`).
    launch_seq: u64,
    /// Spawned child processes reaped non-blockingly via `try_wait`.
    children: Vec<Child>,
    /// Prevents overlapping background `.desktop` rescan threads (`RUSTYOS-164`).
    reload_in_flight: Arc<AtomicBool>,
    /// Pending catalog produced by a background rescan thread (`RUSTYOS-164`).
    reloaded_catalog: Arc<Mutex<Option<Vec<LauncherApp>>>>,
    /// Optional event-loop wakeup callback invoked once a background rescan finishes (`RUSTYOS-164`).
    reload_waker: Arc<Mutex<Option<CatalogReloadCallback>>>,
}

impl fmt::Debug for LauncherState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LauncherState")
            .field("query", &self.query)
            .field("selected_index", &self.selected_index)
            .field("catalog_len", &self.catalog.len())
            .field(
                "reload_in_flight",
                &self.reload_in_flight.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
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
            launch_seq: 0,
            children: Vec::new(),
            reload_in_flight: Arc::new(AtomicBool::new(false)),
            reloaded_catalog: Arc::new(Mutex::new(None)),
            reload_waker: Arc::new(Mutex::new(None)),
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
            launch_seq: 0,
            children: Vec::new(),
            reload_in_flight: Arc::new(AtomicBool::new(false)),
            reloaded_catalog: Arc::new(Mutex::new(None)),
            reload_waker: Arc::new(Mutex::new(None)),
        }
    }

    /// Registers a wakeup callback invoked when a background `.desktop` rescan completes (`RUSTYOS-164`).
    pub fn set_reload_waker(&self, waker: CatalogReloadCallback) {
        if let Ok(mut guard) = self.reload_waker.lock() {
            *guard = Some(waker);
        }
    }

    /// Triggers a non-blocking background `.desktop` rescan across all XDG/Flatpak directories (`RUSTYOS-164`).
    pub fn request_async_reload(&self) {
        self.request_async_reload_dirs(rusty_services::discover_application_dirs());
    }

    /// Triggers a non-blocking background `.desktop` rescan over explicit `dirs` (`RUSTYOS-164`).
    pub fn request_async_reload_dirs(&self, dirs: Vec<PathBuf>) {
        if self.reload_in_flight.swap(true, Ordering::AcqRel) {
            return;
        }
        let in_flight = Arc::clone(&self.reload_in_flight);
        let slot = Arc::clone(&self.reloaded_catalog);
        let waker = Arc::clone(&self.reload_waker);
        let spawned = thread::Builder::new()
            .name("rustyos-app-scan".to_owned())
            .spawn(move || {
                let mut catalog = discover_desktop_apps_in_dirs(&dirs);
                merge_builtin_fallbacks(&mut catalog);
                sort_catalog(&mut catalog);
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(catalog);
                }
                in_flight.store(false, Ordering::Release);
                let cb = waker.lock().ok().and_then(|g| g.clone());
                if let Some(wake) = cb {
                    wake();
                }
            });
        if spawned.is_err() {
            self.reload_in_flight.store(false, Ordering::Release);
        }
    }

    /// Atomically applies a completed background `.desktop` rescan without blocking the event loop (`RUSTYOS-164`).
    pub fn take_reloaded_catalog(&mut self) -> bool {
        let next = self
            .reloaded_catalog
            .try_lock()
            .ok()
            .and_then(|mut guard| guard.take());
        next.is_some_and(|catalog| self.replace_catalog(catalog))
    }

    /// Replaces the live catalog if it changed and keeps `selected_index` within bounds (`RUSTYOS-164`).
    pub fn replace_catalog(&mut self, mut catalog: Vec<LauncherApp>) -> bool {
        sort_catalog(&mut catalog);
        if self.catalog == catalog {
            return false;
        }
        self.catalog = catalog;
        let max_idx = self.filtered_candidates().len().saturating_sub(1);
        if self.selected_index > max_idx {
            self.selected_index = max_idx;
        }
        true
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
                    app_key: Some(crate::shell_render::canonical_dock_app_key(
                        Some(&app.id),
                        &app.name,
                        rusty_wm::WindowId(0),
                    )),
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
                app_key: Some(crate::shell_render::canonical_dock_app_key(
                    Some(&app.id),
                    &app.name,
                    rusty_wm::WindowId(0),
                )),
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
                    app_key: None,
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

    /// Resolves `(raw_exec_template, supported_mime_types)` for `app_key` (`RUSTYOS-163`).
    #[must_use]
    pub(crate) fn find_exec_and_mimes_for_dock_key(
        &self,
        app_key: &str,
    ) -> Option<(String, Vec<String>)> {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        let dot_suffix = format!(".{key}");
        let preferred_bins = Self::canonical_preferred_bins(&key);
        for app in &self.catalog {
            let id_lower = app.id.trim_end_matches(".desktop").to_ascii_lowercase();
            let bin = exec_basename(&app.exec);
            let matched_bin = preferred_bins
                .is_some_and(|bins| bins.iter().any(|b| b.eq_ignore_ascii_case(&bin)));
            let matched = matched_bin
                || app.id.eq_ignore_ascii_case(&key)
                || id_lower == key
                || id_lower.ends_with(&dot_suffix)
                || bin.eq_ignore_ascii_case(&key)
                || app.name.eq_ignore_ascii_case(&key);
            if matched && command_exists_on_path(&app.exec) {
                return Some((app.exec.clone(), app.mime_types.clone()));
            }
        }
        self.find_command_for_dock_key(&key)
            .map(|cmd| (cmd, Vec::new()))
    }

    /// Returns `true` if dropping `paths` onto Dock slot `app_key` is supported (`RUSTYOS-163`).
    #[must_use]
    pub(crate) fn app_accepts_dropped_paths(&self, app_key: &str, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return false;
        }
        let key = app_key.trim().to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "overview" | "settings" | "updater" | "downloads" | "trash"
        ) {
            return false;
        }
        if matches!(
            key.as_str(),
            "files" | "terminal" | "browser" | "firefox" | "google-chrome" | "notes"
        ) {
            return true;
        }
        let Some((exec_tpl, mimes)) = self.find_exec_and_mimes_for_dock_key(&key) else {
            return false;
        };
        if mimes.is_empty() {
            // Allow custom/untagged apps whose `Exec=` accepts files/URLs or any local binary.
            return exec_tpl.contains("%f")
                || exec_tpl.contains("%F")
                || exec_tpl.contains("%u")
                || exec_tpl.contains("%U")
                || !exec_tpl.is_empty();
        }
        paths.iter().any(|path| {
            let file_mime = mime_for_path(path);
            mimes
                .iter()
                .any(|supported| desktop_mime_matches(supported, file_mime))
        })
    }

    /// Launches a Dock application with dropped file `paths`, honoring `.desktop` `%f`/`%F`/`%u`/`%U`
    /// field codes and setting working directory for terminal apps (`RUSTYOS-163`).
    pub(crate) fn launch_by_dock_key_with_paths(
        &mut self,
        app_key: &str,
        paths: &[PathBuf],
        wayland_display: &str,
        ipc_socket: &Path,
    ) -> bool {
        if paths.is_empty() {
            return self.launch_by_dock_key(app_key, wayland_display, ipc_socket);
        }
        let key = app_key.trim().to_ascii_lowercase();
        let Some((exec_tpl, _)) = self.find_exec_and_mimes_for_dock_key(&key) else {
            return false;
        };

        if key == "terminal" {
            let target_dir = paths.first().and_then(|p| {
                if p.is_dir() {
                    Some(p.as_path())
                } else {
                    p.parent()
                }
            });
            let tokens = parse_exec_argv(&exec_tpl);
            let Some((program, args)) = tokens.split_first() else {
                return false;
            };
            self.reap_children();
            return self
                .spawn_resolved_program_in_dir(
                    program,
                    args,
                    target_dir,
                    wayland_display,
                    ipc_socket,
                )
                .is_ok();
        }

        let effective_paths: Vec<PathBuf> = if key == "files" {
            paths
                .iter()
                .take(1)
                .map(|p| {
                    if p.is_dir() {
                        p.clone()
                    } else {
                        p.parent().map_or_else(|| p.clone(), Path::to_path_buf)
                    }
                })
                .collect()
        } else {
            paths.to_vec()
        };
        let tokens = expand_exec_argv_with_paths(&exec_tpl, &effective_paths);
        let Some((program, args)) = tokens.split_first() else {
            return false;
        };
        self.reap_children();
        self.spawn_resolved_program_in_dir(program, args, None, wayland_display, ipc_socket)
            .is_ok()
    }

    /// Parses a dropped `.desktop` file, registers it in the in-memory catalog if missing,
    /// and returns `(canonical_dock_key, LauncherApp)` (`RUSTYOS-163`).
    pub(crate) fn resolve_desktop_file_to_dock_key(
        &mut self,
        desktop_path: &Path,
    ) -> Option<(String, LauncherApp)> {
        let contents = fs::read_to_string(desktop_path).ok()?;
        let id = desktop_path.file_name()?.to_string_lossy().into_owned();
        let app = parse_desktop_entry(&contents, &id)?;
        let key = crate::shell_render::canonical_dock_app_key(
            Some(&app.id),
            &app.name,
            rusty_wm::WindowId(0),
        );
        if !self
            .catalog
            .iter()
            .any(|c| c.id.eq_ignore_ascii_case(&app.id))
        {
            self.catalog.push(app.clone());
        }
        Some((key, app))
    }

    /// Returns `true` when `app_key` resolves to an installed `.desktop` entry or executable on `PATH` (`RUSTYOS-161`).
    #[must_use]
    pub(crate) fn has_installed_dock_entry(&self, app_key: &str) -> bool {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return false;
        }
        if self.find_command_for_dock_key(&key).is_some() {
            return true;
        }
        let dot_suffix = format!(".{key}");
        self.catalog.iter().any(|app| {
            let id_lower = app.id.trim_end_matches(".desktop").to_ascii_lowercase();
            let bin = exec_basename(&app.exec);
            app.id.eq_ignore_ascii_case(&key)
                || id_lower == key
                || id_lower.ends_with(&dot_suffix)
                || bin.eq_ignore_ascii_case(&key)
                || app.name.eq_ignore_ascii_case(&key)
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

    /// Non-blockingly reaps any exited child processes spawned by the compositor.
    pub fn reap_children(&mut self) {
        self.children
            .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
    }

    /// Registers an externally spawned child process (`--spawn`, `autostart`, `plymouth`)
    /// so [`Self::reap_children`] reaps it non-blockingly once it exits.
    pub(crate) fn track_child(&mut self, child: Child) {
        self.reap_children();
        self.children.push(child);
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
        self.spawn_resolved_program_in_dir(program, args, None, wayland_display, ipc_socket)
    }

    fn spawn_resolved_program_in_dir(
        &mut self,
        program: &str,
        args: &[String],
        working_dir: Option<&Path>,
        wayland_display: &str,
        ipc_socket: &Path,
    ) -> Result<u32, String> {
        let resolved = resolve_program_path(program);
        let program_path = resolved.as_deref().unwrap_or_else(|| Path::new(program));
        self.launch_seq = self.launch_seq.wrapping_add(1);
        let use_systemd_scope =
            !cfg!(test) && rusty_services::appruntime::is_systemd_user_scope_available();
        let (launch_bin, launch_args) = rusty_services::appruntime::build_scoped_launch_command(
            program_path,
            args,
            self.launch_seq,
            use_systemd_scope,
        );

        let mut cmd = Command::new(&launch_bin);
        cmd.args(&launch_args)
            .env("WAYLAND_DISPLAY", wayland_display)
            .env("RUSTYOS_IPC_SOCKET", ipc_socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(dir) = working_dir
            && dir.is_dir()
        {
            cmd.current_dir(dir);
        }
        if let Some(disp) = &self.x11_display {
            cmd.env("DISPLAY", disp);
        }
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) if use_systemd_scope => {
                warn!(
                    program = %program,
                    error = %err,
                    "systemd-run scope spawn failed, falling back to direct process spawn"
                );
                let (_, fallback_args) = rusty_services::appruntime::build_scoped_launch_command(
                    program_path,
                    args,
                    self.launch_seq,
                    false,
                );
                let mut fallback_cmd = Command::new(program_path);
                fallback_cmd
                    .args(&fallback_args)
                    .env("WAYLAND_DISPLAY", wayland_display)
                    .env("RUSTYOS_IPC_SOCKET", ipc_socket)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                if let Some(dir) = working_dir
                    && dir.is_dir()
                {
                    fallback_cmd.current_dir(dir);
                }
                if let Some(disp) = &self.x11_display {
                    fallback_cmd.env("DISPLAY", disp);
                }
                fallback_cmd
                    .spawn()
                    .map_err(|e| format!("failed to launch '{program}': {e}"))?
            }
            Err(err) => return Err(format!("failed to launch '{program}': {err}")),
        };

        let pid = child.id();
        info!(
            program = %program,
            resolved = %program_path.display(),
            scoped = use_systemd_scope,
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
    discover_desktop_apps_in_dirs(&rusty_services::discover_application_dirs())
}

pub(crate) fn discover_desktop_apps_in_dirs(dirs: &[PathBuf]) -> Vec<LauncherApp> {
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut apps: Vec<LauncherApp> = Vec::new();

    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
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
            mime_types: vec!["inode/directory".to_owned()],
        },
        LauncherApp {
            id: "org.rustyos.files".to_owned(),
            name: "Files".to_owned(),
            subtitle: "Browse folders & documents (RustyOS Files)".to_owned(),
            exec: "rusty-files".to_owned(),
            keywords: vec!["folder".to_owned(), "manager".to_owned()],
            badge: "Fi".to_owned(),
            icon: Some("system-file-manager".to_owned()),
            mime_types: vec!["inode/directory".to_owned()],
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
            mime_types: Vec::new(),
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
            mime_types: Vec::new(),
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
            mime_types: Vec::new(),
        },
        LauncherApp {
            id: "org.rustyos.gears".to_owned(),
            name: "ES2 Gears Demo".to_owned(),
            subtitle: "OpenGL ES Wayland benchmark".to_owned(),
            exec: "es2gears_wayland".to_owned(),
            keywords: vec!["opengl".to_owned(), "gl".to_owned(), "benchmark".to_owned()],
            badge: "GL".to_owned(),
            icon: Some("glxgears".to_owned()),
            mime_types: Vec::new(),
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
    let mut mime_types: Vec<String> = Vec::new();
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
            "MimeType" => {
                mime_types = val
                    .split(';')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
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
        mime_types,
    })
}

/// Splits an XDG `Exec=` command string into program + arguments, stripping `%f`/`%u`/etc. field codes.
#[must_use]
pub fn parse_exec_argv(exec: &str) -> Vec<String> {
    expand_exec_argv_with_paths(exec, &[])
}

/// Splits an XDG `Exec=` command string and substitutes `%f`/`%F`/`%u`/`%U` field codes with `paths`
/// (appending `paths` as positional arguments when no file/URL field code is present) (`RUSTYOS-163`).
#[must_use]
pub fn expand_exec_argv_with_paths(exec: &str, paths: &[PathBuf]) -> Vec<String> {
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

    let mut out = Vec::with_capacity(raw_tokens.len() + paths.len());
    let mut expanded_paths = false;
    for tok in raw_tokens {
        match tok.as_str() {
            "%f" => {
                expanded_paths = true;
                if let Some(first) = paths.first() {
                    out.push(first.to_string_lossy().into_owned());
                }
            }
            "%F" => {
                expanded_paths = true;
                for p in paths {
                    out.push(p.to_string_lossy().into_owned());
                }
            }
            "%u" => {
                expanded_paths = true;
                if let Some(first) = paths.first() {
                    out.push(rusty_services::path_to_file_uri(first));
                }
            }
            "%U" => {
                expanded_paths = true;
                for p in paths {
                    out.push(rusty_services::path_to_file_uri(p));
                }
            }
            "%d" | "%D" | "%n" | "%N" | "%i" | "%c" | "%k" | "@@u" | "@@" => {}
            _ => {
                let cleaned = tok.replace("%%", "%");
                if !cleaned.is_empty() {
                    out.push(cleaned);
                }
            }
        }
    }
    if !expanded_paths && !paths.is_empty() && !out.is_empty() {
        for p in paths {
            out.push(p.to_string_lossy().into_owned());
        }
    }
    out
}

/// Infers a lightweight MIME type from a local filesystem path (`RUSTYOS-163`).
#[must_use]
pub fn mime_for_path(path: &Path) -> &'static str {
    if path.is_dir() {
        return "inode/directory";
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("desktop") => "application/x-desktop",
        Some("html" | "htm" | "xhtml") => "text/html",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("mp4" | "m4v") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("ogg" | "opus") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("zip") => "application/zip",
        Some("tar" | "gz" | "xz" | "zst" | "bz2") => "application/x-archive",
        Some(
            "txt" | "md" | "rs" | "toml" | "json" | "yaml" | "yml" | "ini" | "conf" | "cfg" | "log"
            | "py" | "js" | "ts" | "c" | "h" | "cpp" | "hpp" | "sh" | "css" | "xml" | "csv",
        ) => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Checks whether a `.desktop` `MimeType=` entry accepts `file_mime` (`RUSTYOS-163`).
#[must_use]
pub fn desktop_mime_matches(supported: &str, file_mime: &str) -> bool {
    let s = supported.trim().to_ascii_lowercase();
    let f = file_mime.trim().to_ascii_lowercase();
    if s.is_empty() || f.is_empty() {
        return false;
    }
    if s == f || s == "*/*" || s == "application/octet-stream" {
        return true;
    }
    if let Some(prefix) = s.strip_suffix("/*") {
        return f
            .split_once('/')
            .is_some_and(|(f_type, _)| f_type == prefix);
    }
    false
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
                mime_types: vec!["inode/directory".to_owned()],
            },
            LauncherApp {
                id: "2".to_owned(),
                name: "Terminal".to_owned(),
                subtitle: "Shell".to_owned(),
                exec: "rusty-terminal".to_owned(),
                keywords: vec!["console".to_owned()],
                badge: ">_".to_owned(),
                icon: Some("utilities-terminal".to_owned()),
                mime_types: Vec::new(),
            },
            LauncherApp {
                id: "3".to_owned(),
                name: "ES2 Gears Demo".to_owned(),
                subtitle: "Benchmark".to_owned(),
                exec: "es2gears_wayland".to_owned(),
                keywords: vec!["opengl".to_owned()],
                badge: "EG".to_owned(),
                icon: None,
                mime_types: Vec::new(),
            },
            LauncherApp {
                id: "4".to_owned(),
                name: "Audio Mixer".to_owned(),
                subtitle: "Volume Control".to_owned(),
                exec: "pavucontrol".to_owned(),
                keywords: vec!["sound".to_owned()],
                badge: "AM".to_owned(),
                icon: None,
                mime_types: Vec::new(),
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
                mime_types: Vec::new(),
            },
            LauncherApp {
                id: "google-chrome.desktop".to_owned(),
                name: "Google Chrome".to_owned(),
                subtitle: "Web Browser".to_owned(),
                exec: "/usr/bin/google-chrome-stable %U".to_owned(),
                keywords: Vec::new(),
                badge: "GC".to_owned(),
                icon: Some("google-chrome".to_owned()),
                mime_types: vec!["text/html".to_owned()],
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
                mime_types: Vec::new(),
            },
            LauncherApp {
                id: "foot.desktop".to_owned(),
                name: "Foot".to_owned(),
                subtitle: "Wayland Terminal".to_owned(),
                exec: "/usr/bin/foot".to_owned(),
                keywords: Vec::new(),
                badge: ">_".to_owned(),
                icon: Some("foot".to_owned()),
                mime_types: Vec::new(),
            },
            LauncherApp {
                id: "org.rustyos.terminal.desktop".to_owned(),
                name: "Terminal".to_owned(),
                subtitle: "RustyOS Terminal".to_owned(),
                exec: "rusty-terminal".to_owned(),
                keywords: vec!["terminal".to_owned()],
                badge: ">_".to_owned(),
                icon: Some("utilities-terminal".to_owned()),
                mime_types: Vec::new(),
            },
        ]);
        assert_eq!(
            launcher.catalog.first().map(|a| exec_basename(&a.exec)),
            Some("rusty-terminal".to_owned())
        );
        let chosen = launcher.find_available_command_with(bins, |_| true);
        assert_eq!(chosen.as_deref(), Some("rusty-terminal"));
    }

    #[test]
    fn track_child_and_reap_children_collects_exited_process() -> Result<(), String> {
        let mut launcher = LauncherState::with_catalog(Vec::new());
        let child = Command::new("/bin/true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("spawn /bin/true failed: {err}"))?;
        launcher.track_child(child);
        for _ in 0..50 {
            launcher.reap_children();
            if launcher.children.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(launcher.children.is_empty());
        Ok(())
    }
}
