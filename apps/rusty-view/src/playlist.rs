//! Directory scanning, sorting, and carousel playlist navigation for `rusty-view`.

use std::fs;
use std::path::{Path, PathBuf};

/// Ordered list of images in the same folder as the active image.
#[derive(Debug, Clone, Default)]
pub(crate) struct Playlist {
    pub(crate) items: Vec<PathBuf>,
    pub(crate) current_index: Option<usize>,
}

impl Playlist {
    /// Builds a playlist scanning the parent folder of `target`.
    #[must_use]
    pub(crate) fn for_path(target: &Path) -> Self {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        let mut items = Vec::new();

        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_supported_image_extension(&path) {
                    items.push(path);
                }
            }
        }

        items.sort_by(|a, b| {
            let name_a = a.file_name().map_or("", |s| s.to_str().unwrap_or(""));
            let name_b = b.file_name().map_or("", |s| s.to_str().unwrap_or(""));
            name_a.cmp(name_b)
        });

        let target_canonical = target
            .canonicalize()
            .unwrap_or_else(|_| target.to_path_buf());
        let current_index = items
            .iter()
            .position(|p| p == target || p.canonicalize().is_ok_and(|c| c == target_canonical));

        let current_index = if let Some(idx) = current_index {
            Some(idx)
        } else if target.is_file() {
            items.push(target.to_path_buf());
            Some(items.len() - 1)
        } else {
            None
        };

        Self {
            items,
            current_index,
        }
    }

    /// Moves to next image in carousel order, wrapping around.
    pub(crate) fn next(&mut self) -> Option<&PathBuf> {
        if self.items.is_empty() {
            return None;
        }
        let cur = self.current_index.unwrap_or(0);
        let next_idx = (cur + 1) % self.items.len();
        self.current_index = Some(next_idx);
        self.items.get(next_idx)
    }

    /// Moves to previous image in carousel order, wrapping around.
    pub(crate) fn prev(&mut self) -> Option<&PathBuf> {
        if self.items.is_empty() {
            return None;
        }
        let cur = self.current_index.unwrap_or(0);
        let prev_idx = if cur == 0 {
            self.items.len().saturating_sub(1)
        } else {
            cur - 1
        };
        self.current_index = Some(prev_idx);
        self.items.get(prev_idx)
    }

    /// Returns human-readable `(current 1-based, total)` indices if available.
    #[must_use]
    pub(crate) fn index_label(&self) -> Option<(usize, usize)> {
        if self.items.is_empty() {
            return None;
        }
        let cur = self.current_index.map_or(1, |idx| idx + 1);
        Some((cur, self.items.len()))
    }
}

/// Checks if file extension matches PNG, BMP, or PPM/PNM.
#[must_use]
pub(crate) fn is_supported_image_extension(path: &Path) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    let Some(ext_str) = ext.to_str() else {
        return false;
    };
    matches!(
        ext_str.to_ascii_lowercase().as_str(),
        "png" | "bmp" | "ppm" | "pnm"
    )
}
