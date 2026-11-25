//! Clipboard abstraction and Wayland `wl_data_device` copy/paste helpers (`RUSTYOS-130`).

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::mpsc::{self, Receiver, Sender};

use wayland_client::Proxy;
use wayland_client::protocol::wl_data_source;

/// Supported UTF-8 clipboard MIME types in preference order.
pub const CLIPBOARD_MIME_TYPES: [&str; 4] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "TEXT",
    "text/plain",
];

/// Abstract clipboard sink used by [`crate::TextInputState`] for testing and Wayland integration.
pub trait ClipboardSink {
    /// Returns cached or synchronous clipboard text when available.
    fn get_text(&mut self) -> Option<String>;
    /// Publishes `text` to the clipboard.
    fn set_text(&mut self, text: &str);
}

/// Simple in-memory clipboard for unit tests and local intra-window fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryClipboard {
    content: String,
}

impl MemoryClipboard {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            content: String::new(),
        }
    }
}

impl ClipboardSink for MemoryClipboard {
    fn get_text(&mut self) -> Option<String> {
        (!self.content.is_empty()).then(|| self.content.clone())
    }

    fn set_text(&mut self, text: &str) {
        text.clone_into(&mut self.content);
    }
}

/// Non-blocking bridge between Wayland `wl_data_device` / `wl_data_source` events and widgets.
///
/// Widgets write through [`ClipboardSink::set_text`] (recording pending Wayland selection
/// publication) while async offer reads arrive back cleanly via [`Self::poll_received_paste`].
pub struct WaylandClipboardBridge {
    local_cache: String,
    pending_copy: Option<String>,
    pending_paste_request: bool,
    copy_sources: Vec<(wl_data_source::WlDataSource, String)>,
    paste_tx: Sender<String>,
    paste_rx: Receiver<String>,
}

impl std::fmt::Debug for WaylandClipboardBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaylandClipboardBridge")
            .field("local_len", &self.local_cache.len())
            .field("active_sources", &self.copy_sources.len())
            .field("pending_paste_request", &self.pending_paste_request)
            .finish_non_exhaustive()
    }
}

impl Default for WaylandClipboardBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl WaylandClipboardBridge {
    #[must_use]
    pub fn new() -> Self {
        let (paste_tx, paste_rx) = mpsc::channel();
        Self {
            local_cache: String::new(),
            pending_copy: None,
            pending_paste_request: false,
            copy_sources: Vec::new(),
            paste_tx,
            paste_rx,
        }
    }

    /// Picks the highest-priority UTF-8 MIME type advertised by a `wl_data_offer`.
    #[must_use]
    pub fn pick_utf8_mime(mimes: &[String]) -> Option<String> {
        for candidate in CLIPBOARD_MIME_TYPES {
            if mimes.iter().any(|m| m == candidate) {
                return Some((*candidate).to_owned());
            }
        }
        mimes.iter().find(|m| m.starts_with("text/")).cloned()
    }

    /// Takes queued copied text to publish via `WlDataDeviceManager::create_copy_paste_source`.
    pub fn take_pending_copy(&mut self) -> Option<String> {
        self.pending_copy.take()
    }

    /// Registers a created Wayland data source with its payload for future `send_request` callbacks.
    ///
    /// Destroys any superseded selection sources so stale `WlDataSource` objects never accumulate
    /// when a compositor declines `set_selection` (`RUSTYOS-169`).
    pub fn register_source(&mut self, source: wl_data_source::WlDataSource, text: String) {
        for (old, _) in self.copy_sources.drain(..) {
            old.destroy();
        }
        self.copy_sources.push((source, text));
    }

    /// Serves a `WlDataSource::Send` request on `write_fd` asynchronously on a background thread
    /// so large payloads (`> 64 KiB`) or stalled receivers never freeze the UI event loop (`RUSTYOS-169`).
    pub fn serve_send_request(
        &self,
        source: &wl_data_source::WlDataSource,
        mime: &str,
        write_fd: OwnedFd,
    ) {
        if !CLIPBOARD_MIME_TYPES.contains(&mime) && !mime.starts_with("text/") {
            return;
        }
        if let Some((_, text)) = self
            .copy_sources
            .iter()
            .find(|(s, _)| s.id() == source.id())
        {
            let payload = text.as_bytes().to_vec();
            let _ = std::thread::Builder::new()
                .name("rusty-ui-clip-send".to_owned())
                .spawn(move || {
                    let mut file = File::from(write_fd);
                    let _ = file.write_all(&payload);
                });
        }
    }

    /// Drops and destroys a cancelled `WlDataSource`.
    pub fn remove_source(&mut self, source: &wl_data_source::WlDataSource) {
        self.copy_sources.retain(|(s, _)| s.id() != source.id());
        source.destroy();
    }

    /// Flags that a widget requested a clipboard paste from Wayland (`Ctrl+V`).
    pub const fn request_wayland_paste(&mut self) {
        self.pending_paste_request = true;
    }

    /// Returns `true` if a widget requested an async Wayland selection read this turn.
    pub const fn take_pending_paste_request(&mut self) -> bool {
        let was = self.pending_paste_request;
        self.pending_paste_request = false;
        was
    }

    /// Spawns a bounded reader thread over a Wayland `WlDataOffer::Receive` pipe (`<= 4 MiB`),
    /// then invokes `wake()` (e.g. a `calloop::ping::Ping`) once UTF-8 text arrives.
    pub fn spawn_receive_from_fd<F>(&self, read_fd: OwnedFd, wake: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let tx = self.paste_tx.clone();
        std::thread::spawn(move || {
            let file = File::from(read_fd);
            let mut bytes = Vec::new();
            let _ = file.take(4 * 1024 * 1024).read_to_end(&mut bytes);
            if let Ok(text) = String::from_utf8(bytes)
                && !text.is_empty()
            {
                let _ = tx.send(text);
                wake();
            }
        });
    }

    /// Polls any asynchronously received Wayland paste payload and updates `local_cache`.
    pub fn poll_received_paste(&mut self) -> Option<String> {
        let mut latest = None;
        while let Ok(text) = self.paste_rx.try_recv() {
            self.local_cache.clone_from(&text);
            latest = Some(text);
        }
        latest
    }

    /// Clears the in-process clipboard cache when another Wayland client takes ownership.
    pub fn clear_local_cache(&mut self) {
        self.local_cache.clear();
    }
}

impl ClipboardSink for WaylandClipboardBridge {
    fn get_text(&mut self) -> Option<String> {
        if let Some(async_text) = self.poll_received_paste() {
            return Some(async_text);
        }
        self.pending_paste_request = true;
        (!self.local_cache.is_empty()).then(|| self.local_cache.clone())
    }

    fn set_text(&mut self, text: &str) {
        text.clone_into(&mut self.local_cache);
        self.pending_copy = Some(self.local_cache.clone());
    }
}
