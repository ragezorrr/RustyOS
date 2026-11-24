//! Compact information HUD overlay card for `rusty-view`.

use rusty_ui::{Rect, ThemePalette, UiPainter};

use crate::document::ImageDocument;
use crate::playlist::Playlist;
use crate::viewport::Viewport;

pub(crate) const HUD_WIDTH: u32 = 230;
pub(crate) const HUD_HEIGHT: u32 = 120;
pub(crate) const HUD_RADIUS: f32 = 12.0;

/// State for the informational overlay card displaying filename, dimensions, file size, and zoom.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InfoHud {
    pub(crate) is_visible: bool,
}

impl InfoHud {
    /// Toggles HUD visibility.
    pub(crate) fn toggle(&mut self) {
        self.is_visible = !self.is_visible;
    }

    /// Renders the informational HUD card in the top-right corner.
    pub(crate) fn paint(
        self,
        painter: &mut UiPainter<'_>,
        pal: &ThemePalette,
        win_w: u32,
        doc: &ImageDocument,
        playlist: &Playlist,
        viewport: &Viewport,
    ) {
        if !self.is_visible {
            return;
        }

        let margin_right = 20;
        let margin_top = 56;
        let hud_x = (win_w as i32) - (HUD_WIDTH as i32) - margin_right;
        let hud_y = margin_top;
        let rect = Rect::new(hud_x.max(8), hud_y, HUD_WIDTH, HUD_HEIGHT);

        let bg_color = if pal.is_dark {
            [28, 30, 36, 220]
        } else {
            [250, 252, 255, 235]
        };
        let border_color = if pal.is_dark {
            [255, 255, 255, 32]
        } else {
            [0, 0, 0, 24]
        };

        painter.fill_rounded_rect(rect, HUD_RADIUS, bg_color);
        painter.stroke_rounded_rect(rect, HUD_RADIUS, 1.0, border_color);

        let text_primary = if pal.is_dark {
            [240, 242, 248, 255]
        } else {
            [20, 24, 32, 255]
        };
        let text_secondary = if pal.is_dark {
            [160, 165, 178, 255]
        } else {
            [100, 105, 118, 255]
        };

        let start_x = rect.x as f32 + 14.0;
        let mut cur_y = rect.y as f32 + 20.0;

        let name_str = if doc.filename.len() > 24 {
            format!("{}…", &doc.filename[..22])
        } else {
            doc.filename.clone()
        };
        painter.draw_text(&name_str, start_x, cur_y, 13.0, true, text_primary);
        cur_y += 22.0;

        let dim_str = format!("{} × {} px", doc.width, doc.height);
        painter.draw_text(&dim_str, start_x, cur_y, 11.5, false, text_secondary);
        cur_y += 18.0;

        let rot_info = if doc.rotation_deg > 0 {
            format!(" • {}°", doc.rotation_deg)
        } else {
            String::new()
        };
        let size_str = format!("{}{rot_info}", doc.formatted_file_size());
        painter.draw_text(&size_str, start_x, cur_y, 11.5, false, text_secondary);
        cur_y += 18.0;

        let zoom_str = viewport.formatted_zoom();
        let idx_str = if let Some((cur, total)) = playlist.index_label() {
            format!("{zoom_str} • {cur}/{total}")
        } else {
            zoom_str
        };
        painter.draw_text(&idx_str, start_x, cur_y, 11.5, false, text_secondary);
    }
}
