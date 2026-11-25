//! Pure-Rust PulseAudio/PipeWire native socket client (`RUSTYOS-171`, `ADR-0100`, `D-114`).
//!
//! Connects directly to `$XDG_RUNTIME_DIR/pulse/native` or `/run/user/<uid>/pulse/native`
//! over a UNIX domain stream, providing sub-millisecond volume adjustments, sink querying,
//! default sink switching, and event subscription without spawning subprocesses or using `unsafe`.

#![allow(clippy::doc_markdown, clippy::missing_errors_doc)]

use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

// --- PulseAudio Native Protocol Constants ---

pub const PA_PROTOCOL_VERSION: u32 = 0xc000_0023; // Version 35 + SHM/memfd flags
pub const PA_COMMAND_ERROR: u32 = 0;
pub const PA_COMMAND_TIMEOUT: u32 = 1;
pub const PA_COMMAND_REPLY: u32 = 2;
pub const PA_COMMAND_AUTH: u32 = 8;
pub const PA_COMMAND_SET_CLIENT_NAME: u32 = 9;
pub const PA_COMMAND_GET_SERVER_INFO: u32 = 20;
pub const PA_COMMAND_GET_SINK_INFO: u32 = 21;
pub const PA_COMMAND_GET_SINK_INFO_LIST: u32 = 22;
pub const PA_COMMAND_SUBSCRIBE: u32 = 35;
pub const PA_COMMAND_SET_SINK_VOLUME: u32 = 36;
pub const PA_COMMAND_SET_SINK_MUTE: u32 = 39;
pub const PA_COMMAND_SET_DEFAULT_SINK: u32 = 44;
pub const PA_COMMAND_SUBSCRIBE_EVENT: u32 = 68;

pub const PA_VOLUME_NORM: u32 = 65_536;
pub const PA_INVALID_INDEX: u32 = 0xFFFF_FFFF;

pub const PA_SUBSCRIPTION_MASK_SINK: u32 = 0x0001;
pub const PA_SUBSCRIPTION_MASK_SERVER: u32 = 0x0010;

/// Upper bound on a single PulseAudio wire packet payload (`1 MiB`) to prevent OOM DoS.
pub const MAX_PA_PACKET_BYTES: usize = 1024 * 1024;
const MAX_PROPLIST_NESTING_DEPTH: usize = 16;

/// Information about an audio output sink retrieved from the audio daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseSinkInfo {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub volume_percent: u8,
    pub muted: bool,
}

/// Errors originating from the PulseAudio native protocol interaction.
#[derive(Debug)]
pub enum PulseError {
    Io(io::Error),
    Protocol(String),
    Server { code: u32, message: String },
    NotConnected,
}

impl fmt::Display for PulseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "PulseAudio IO error: {err}"),
            Self::Protocol(msg) => write!(f, "PulseAudio protocol error: {msg}"),
            Self::Server { code, message } => {
                write!(f, "PulseAudio server error {code}: {message}")
            }
            Self::NotConnected => write!(f, "PulseAudio socket not connected"),
        }
    }
}

impl std::error::Error for PulseError {}

impl From<io::Error> for PulseError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

// --- TagStruct Serializer ---

/// Serializer for PulseAudio tagged binary packets.
#[derive(Debug, Default)]
pub struct TagWriter {
    buf: Vec<u8>,
}

impl TagWriter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(256),
        }
    }

    pub fn put_u32(&mut self, val: u32) {
        self.buf.push(b'L');
        self.buf.extend_from_slice(&val.to_be_bytes());
    }

    pub fn put_string(&mut self, s: &str) {
        self.buf.push(b't');
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }

    pub fn put_arbitrary(&mut self, bytes: &[u8]) {
        self.buf.push(b'x');
        #[allow(clippy::cast_possible_truncation)]
        let len = bytes.len() as u32;
        self.buf.extend_from_slice(&len.to_be_bytes());
        self.buf.extend_from_slice(bytes);
    }

    pub fn put_bool(&mut self, val: bool) {
        self.buf.push(if val { b'1' } else { b'0' });
    }

    pub fn put_cvolume(&mut self, volumes: &[u32]) {
        self.buf.push(b'v');
        #[allow(clippy::cast_possible_truncation)]
        self.buf.push(volumes.len() as u8);
        for v in volumes {
            self.buf.extend_from_slice(&v.to_be_bytes());
        }
    }

    pub fn put_app_name_proplist(&mut self, name: &str) {
        self.buf.push(b'P');
        self.buf.push(b't');
        self.buf.extend_from_slice(b"application.name\0");
        #[allow(clippy::cast_possible_truncation)]
        let val_len = (name.len() + 1) as u32;
        self.buf.push(b'L');
        self.buf.extend_from_slice(&val_len.to_be_bytes());
        self.buf.push(b'x');
        self.buf.extend_from_slice(&val_len.to_be_bytes());
        self.buf.extend_from_slice(name.as_bytes());
        self.buf.push(0);
        self.buf.push(b'N');
    }

    #[must_use]
    pub fn finish_packet(self) -> Vec<u8> {
        #[allow(clippy::cast_possible_truncation)]
        let payload_len = self.buf.len() as u32;
        let mut packet = Vec::with_capacity(20 + self.buf.len());
        packet.extend_from_slice(&payload_len.to_be_bytes());
        packet.extend_from_slice(&0xFFFF_FFFF_u32.to_be_bytes()); // channel
        packet.extend_from_slice(&0_u32.to_be_bytes()); // offset_hi
        packet.extend_from_slice(&0_u32.to_be_bytes()); // offset_lo
        packet.extend_from_slice(&0_u32.to_be_bytes()); // flags
        packet.extend_from_slice(&self.buf);
        packet
    }
}

// --- TagStruct Deserializer ---

/// Safe parser for PulseAudio tagged binary responses.
#[derive(Debug)]
pub struct TagReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> TagReader<'a> {
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    #[must_use]
    pub fn has_more(&self) -> bool {
        self.pos < self.data.len()
    }

    #[must_use]
    pub fn peek_tag(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    pub fn get_u32(&mut self) -> Result<u32, PulseError> {
        if self.pos >= self.data.len() || self.data[self.pos] != b'L' {
            let actual = self.data.get(self.pos).copied().unwrap_or(0);
            return Err(PulseError::Protocol(format!(
                "expected 'L' for u32 at pos {}, got {actual}",
                self.pos
            )));
        }
        self.pos += 1;
        if self.pos + 4 > self.data.len() {
            return Err(PulseError::Protocol("unexpected EOF reading u32".into()));
        }
        let bytes: [u8; 4] = self.data[self.pos..self.pos + 4]
            .try_into()
            .map_err(|_| PulseError::Protocol("slice to array conversion failed".into()))?;
        self.pos += 4;
        Ok(u32::from_be_bytes(bytes))
    }

    pub fn get_string(&mut self) -> Result<Option<String>, PulseError> {
        if self.pos >= self.data.len() {
            return Err(PulseError::Protocol("unexpected EOF reading string".into()));
        }
        let tag = self.data[self.pos];
        self.pos += 1;
        if tag == b'N' {
            return Ok(None);
        }
        if tag != b't' {
            return Err(PulseError::Protocol(format!(
                "expected 't' or 'N' for string, got {tag}"
            )));
        }
        let start = self.pos;
        while self.pos < self.data.len() && self.data[self.pos] != 0 {
            self.pos += 1;
        }
        if self.pos >= self.data.len() {
            return Err(PulseError::Protocol(
                "unterminated string in tagstruct".into(),
            ));
        }
        let str_bytes = &self.data[start..self.pos];
        self.pos += 1; // skip trailing null byte
        Ok(Some(String::from_utf8_lossy(str_bytes).into_owned()))
    }

    pub fn get_cvolume(&mut self) -> Result<Vec<u32>, PulseError> {
        if self.pos >= self.data.len() || self.data[self.pos] != b'v' {
            return Err(PulseError::Protocol("expected 'v' for cvolume".into()));
        }
        self.pos += 1;
        if self.pos >= self.data.len() {
            return Err(PulseError::Protocol(
                "unexpected EOF in cvolume channels".into(),
            ));
        }
        let channels = self.data[self.pos] as usize;
        self.pos += 1;
        if self.pos + channels * 4 > self.data.len() {
            return Err(PulseError::Protocol(
                "unexpected EOF in cvolume channel values".into(),
            ));
        }
        let mut vols = Vec::with_capacity(channels);
        for _ in 0..channels {
            let bytes: [u8; 4] = self.data[self.pos..self.pos + 4]
                .try_into()
                .map_err(|_| PulseError::Protocol("slice to array conversion failed".into()))?;
            self.pos += 4;
            vols.push(u32::from_be_bytes(bytes));
        }
        Ok(vols)
    }

    pub fn get_bool(&mut self) -> Result<bool, PulseError> {
        if self.pos >= self.data.len() {
            return Err(PulseError::Protocol("unexpected EOF reading bool".into()));
        }
        let tag = self.data[self.pos];
        self.pos += 1;
        match tag {
            b'1' => Ok(true),
            b'0' => Ok(false),
            _ => Err(PulseError::Protocol(format!(
                "expected '1' or '0' for bool, got {tag}"
            ))),
        }
    }

    pub fn skip_one(&mut self) -> Result<(), PulseError> {
        self.skip_one_depth(0)
    }

    fn skip_one_depth(&mut self, depth: usize) -> Result<(), PulseError> {
        if depth > MAX_PROPLIST_NESTING_DEPTH {
            return Err(PulseError::Protocol(
                "PulseAudio tag nesting exceeds limit".into(),
            ));
        }
        if self.pos >= self.data.len() {
            return Ok(());
        }
        let tag = self.data[self.pos];
        self.pos += 1;
        match tag {
            b'L' => {
                if self.pos.checked_add(4).is_none_or(|n| n > self.data.len()) {
                    return Err(PulseError::Protocol("EOF in skip L".into()));
                }
                self.pos += 4;
            }
            b'U' | b's' => {
                if self.pos.checked_add(8).is_none_or(|n| n > self.data.len()) {
                    return Err(PulseError::Protocol("EOF in skip U/s".into()));
                }
                self.pos += 8;
            }
            b't' => {
                while self.pos < self.data.len() && self.data[self.pos] != 0 {
                    self.pos += 1;
                }
                if self.pos < self.data.len() {
                    self.pos += 1;
                }
            }
            b'a' => {
                if self.pos.checked_add(6).is_none_or(|n| n > self.data.len()) {
                    return Err(PulseError::Protocol("EOF in skip a".into()));
                }
                self.pos += 6;
            }
            b'm' => {
                if self.pos >= self.data.len() {
                    return Err(PulseError::Protocol("EOF in skip m".into()));
                }
                let ch = self.data[self.pos] as usize;
                let next = self
                    .pos
                    .checked_add(1)
                    .and_then(|p| p.checked_add(ch))
                    .ok_or_else(|| PulseError::Protocol("overflow in skip m".into()))?;
                if next > self.data.len() {
                    return Err(PulseError::Protocol("EOF in skip m".into()));
                }
                self.pos = next;
            }
            b'v' => {
                if self.pos >= self.data.len() {
                    return Err(PulseError::Protocol("EOF in skip v".into()));
                }
                let ch = self.data[self.pos] as usize;
                let next = self
                    .pos
                    .checked_add(1)
                    .and_then(|p| p.checked_add(ch.saturating_mul(4)))
                    .ok_or_else(|| PulseError::Protocol("overflow in skip v".into()))?;
                if next > self.data.len() {
                    return Err(PulseError::Protocol("EOF in skip v".into()));
                }
                self.pos = next;
            }
            b'x' => {
                if self.pos.checked_add(4).is_none_or(|n| n > self.data.len()) {
                    return Err(PulseError::Protocol("EOF in skip x len".into()));
                }
                let bytes: [u8; 4] = self.data[self.pos..self.pos + 4]
                    .try_into()
                    .map_err(|_| PulseError::Protocol("slice to array conversion failed".into()))?;
                let len = u32::from_be_bytes(bytes) as usize;
                let next = self
                    .pos
                    .checked_add(4)
                    .and_then(|p| p.checked_add(len))
                    .ok_or_else(|| PulseError::Protocol("overflow in skip x".into()))?;
                if next > self.data.len() {
                    return Err(PulseError::Protocol("EOF in skip x".into()));
                }
                self.pos = next;
            }
            b'P' => {
                while self.pos < self.data.len() && self.data[self.pos] != b'N' {
                    self.skip_one_depth(depth + 1)?;
                }
                if self.pos >= self.data.len() {
                    return Err(PulseError::Protocol("EOF in skip P".into()));
                }
                self.pos += 1;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Parses the leading fields of a sink struct into a `PulseSinkInfo`.
pub fn parse_sink_info_fields(reader: &mut TagReader<'_>) -> Result<PulseSinkInfo, PulseError> {
    let index = reader.get_u32()?;
    let name = reader.get_string()?.unwrap_or_default();
    let description = reader.get_string()?.unwrap_or_default();
    reader.skip_one()?; // sample_spec
    reader.skip_one()?; // channel_map
    reader.skip_one()?; // module_index
    let volumes = reader.get_cvolume()?;
    let muted = reader.get_bool()?;

    let volume_percent = if volumes.is_empty() {
        0
    } else {
        let sum: u64 = volumes.iter().map(|&v| u64::from(v)).sum();
        #[allow(clippy::cast_possible_truncation)]
        let channels = volumes.len() as u64;
        let avg = sum / channels;
        #[allow(clippy::cast_possible_truncation)]
        let pct = ((avg * 100 + (u64::from(PA_VOLUME_NORM) / 2)) / u64::from(PA_VOLUME_NORM))
            .min(100) as u8;
        pct
    };

    Ok(PulseSinkInfo {
        index,
        name,
        description,
        volume_percent,
        muted,
    })
}

// --- PulseAudio Native Socket Client ---

/// Non-blocking, pure safe Rust client to the PulseAudio / PipeWire UNIX domain socket.
#[derive(Debug)]
pub struct PulseClient {
    stream: UnixStream,
    tag_seq: u32,
}

impl PulseClient {
    /// Attempts to locate the active PulseAudio / PipeWire socket path.
    #[must_use]
    pub fn find_socket_path() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("PULSE_SERVER") {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let p = PathBuf::from(runtime_dir).join("pulse/native");
            if p.exists() {
                return Some(p);
            }
        }
        if let Ok(meta) = std::fs::metadata("/proc/self") {
            use std::os::unix::fs::MetadataExt;
            let uid = meta.uid();
            let p = PathBuf::from(format!("/run/user/{uid}/pulse/native"));
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    /// Reads the PulseAudio authentication cookie from standard disk locations.
    #[must_use]
    pub fn read_auth_cookie() -> Vec<u8> {
        if let Ok(path) = std::env::var("PULSE_COOKIE")
            && let Ok(data) = std::fs::read(path)
            && data.len() == 256
        {
            return data;
        }
        if let Ok(home) = std::env::var("HOME") {
            let p1 = Path::new(&home).join(".config/pulse/cookie");
            if let Ok(data) = std::fs::read(p1)
                && data.len() == 256
            {
                return data;
            }
            let p2 = Path::new(&home).join(".pulse-cookie");
            if let Ok(data) = std::fs::read(p2)
                && data.len() == 256
            {
                return data;
            }
        }
        vec![0u8; 256]
    }

    /// Connects to the local PulseAudio / PipeWire socket and performs authentication and registration.
    pub fn connect() -> Result<Self, PulseError> {
        let sock_path = Self::find_socket_path().ok_or(PulseError::NotConnected)?;
        let mut stream = UnixStream::connect(sock_path)?;
        stream.set_read_timeout(Some(Duration::from_millis(600)))?;
        stream.set_write_timeout(Some(Duration::from_millis(600)))?;

        let cookie = Self::read_auth_cookie();

        // 1. Send PA_COMMAND_AUTH
        let mut auth = TagWriter::new();
        auth.put_u32(PA_COMMAND_AUTH);
        auth.put_u32(0); // tag 0
        auth.put_u32(PA_PROTOCOL_VERSION);
        auth.put_arbitrary(&cookie);
        stream.write_all(&auth.finish_packet())?;

        let auth_resp = Self::read_packet(&mut stream)?;
        let mut r = TagReader::new(&auth_resp);
        let resp_cmd = r.get_u32()?;
        let _resp_tag = r.get_u32()?;
        if resp_cmd == PA_COMMAND_ERROR {
            let err_code = r.get_u32().unwrap_or(0);
            return Err(PulseError::Server {
                code: err_code,
                message: "Authentication rejected by PulseAudio/PipeWire".into(),
            });
        }
        if resp_cmd != PA_COMMAND_REPLY {
            return Err(PulseError::Protocol(format!(
                "unexpected reply to AUTH: {resp_cmd}"
            )));
        }

        // 2. Send PA_COMMAND_SET_CLIENT_NAME
        let mut client_name = TagWriter::new();
        client_name.put_u32(PA_COMMAND_SET_CLIENT_NAME);
        client_name.put_u32(1); // tag 1
        client_name.put_app_name_proplist("RustyOS");
        stream.write_all(&client_name.finish_packet())?;

        let name_resp = Self::read_packet(&mut stream)?;
        let mut r2 = TagReader::new(&name_resp);
        let name_cmd = r2.get_u32()?;
        if name_cmd != PA_COMMAND_REPLY {
            return Err(PulseError::Protocol(format!(
                "unexpected reply to SET_CLIENT_NAME: {name_cmd}"
            )));
        }

        Ok(Self { stream, tag_seq: 2 })
    }

    /// Reads a 20-byte framed packet from the underlying socket with a specified timeout,
    /// restoring the previous socket read timeout afterward.
    pub fn read_packet_with_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, PulseError> {
        let prev = self.stream.read_timeout().ok().flatten();
        self.stream.set_read_timeout(Some(timeout))?;
        let res = Self::read_packet(&mut self.stream);
        let _ = self.stream.set_read_timeout(prev);
        res
    }

    /// Reads a 20-byte framed packet from the underlying socket.
    pub fn read_packet(stream: &mut UnixStream) -> Result<Vec<u8>, PulseError> {
        let mut hdr = [0u8; 20];
        stream.read_exact(&mut hdr)?;
        let len_bytes: [u8; 4] = hdr[0..4]
            .try_into()
            .map_err(|_| PulseError::Protocol("header slice to array conversion failed".into()))?;
        let length = u32::from_be_bytes(len_bytes) as usize;
        if length > MAX_PA_PACKET_BYTES {
            return Err(PulseError::Protocol(format!(
                "packet length {length} exceeds maximum {MAX_PA_PACKET_BYTES}"
            )));
        }
        let mut body = vec![0u8; length];
        stream.read_exact(&mut body)?;
        Ok(body)
    }

    fn read_reply_for_tag(&mut self, expected_tag: u32, op: &str) -> Result<Vec<u8>, PulseError> {
        for _ in 0..64 {
            let resp = Self::read_packet(&mut self.stream)?;
            let mut r = TagReader::new(&resp);
            let cmd = r.get_u32()?;
            if cmd == PA_COMMAND_SUBSCRIBE_EVENT {
                continue;
            }
            let resp_tag = r.get_u32()?;
            if cmd == PA_COMMAND_ERROR {
                let err_code = r.get_u32().unwrap_or(0);
                return Err(PulseError::Server {
                    code: err_code,
                    message: format!("{op} failed"),
                });
            }
            if cmd != PA_COMMAND_REPLY || resp_tag != expected_tag {
                return Err(PulseError::Protocol(format!(
                    "unexpected response to {op}: cmd={cmd}, tag={resp_tag}"
                )));
            }
            return Ok(resp);
        }
        Err(PulseError::Protocol(format!(
            "exceeded subscribe event limit waiting for {op} reply"
        )))
    }

    fn next_tag(&mut self) -> u32 {
        let t = self.tag_seq;
        self.tag_seq = self.tag_seq.wrapping_add(1);
        t
    }

    /// Queries information about the default audio sink (`@DEFAULT_SINK@`).
    pub fn get_default_sink(&mut self) -> Result<PulseSinkInfo, PulseError> {
        let tag = self.next_tag();
        let mut w = TagWriter::new();
        w.put_u32(PA_COMMAND_GET_SINK_INFO);
        w.put_u32(tag);
        w.put_u32(PA_INVALID_INDEX);
        w.put_string("@DEFAULT_SINK@");
        self.stream.write_all(&w.finish_packet())?;

        let resp = self.read_reply_for_tag(tag, "GET_SINK_INFO")?;
        let mut r = TagReader::new(&resp);
        let _ = r.get_u32()?;
        let _ = r.get_u32()?;
        parse_sink_info_fields(&mut r)
    }

    /// Sets the volume of the target sink (or `@DEFAULT_SINK@`) in percent (`0..=100`).
    pub fn set_sink_volume(&mut self, sink: &str, percent: u8) -> Result<(), PulseError> {
        let tag = self.next_tag();
        let clamped = percent.min(100);
        let vol = (u32::from(clamped) * PA_VOLUME_NORM) / 100;

        let mut w = TagWriter::new();
        w.put_u32(PA_COMMAND_SET_SINK_VOLUME);
        w.put_u32(tag);
        w.put_u32(PA_INVALID_INDEX);
        w.put_string(sink);
        w.put_cvolume(&[vol, vol]); // stereo channels
        self.stream.write_all(&w.finish_packet())?;

        let _ = self.read_reply_for_tag(tag, "SET_SINK_VOLUME")?;
        Ok(())
    }

    /// Sets the mute status of the target sink (or `@DEFAULT_SINK@`).
    pub fn set_sink_mute(&mut self, sink: &str, mute: bool) -> Result<(), PulseError> {
        let tag = self.next_tag();
        let mut w = TagWriter::new();
        w.put_u32(PA_COMMAND_SET_SINK_MUTE);
        w.put_u32(tag);
        w.put_u32(PA_INVALID_INDEX);
        w.put_string(sink);
        w.put_bool(mute);
        self.stream.write_all(&w.finish_packet())?;

        let _ = self.read_reply_for_tag(tag, "SET_SINK_MUTE")?;
        Ok(())
    }

    /// Sets the default audio sink by sink name.
    pub fn set_default_sink(&mut self, sink_name: &str) -> Result<(), PulseError> {
        let tag = self.next_tag();
        let mut w = TagWriter::new();
        w.put_u32(PA_COMMAND_SET_DEFAULT_SINK);
        w.put_u32(tag);
        w.put_string(sink_name);
        self.stream.write_all(&w.finish_packet())?;

        let _ = self.read_reply_for_tag(tag, "SET_DEFAULT_SINK")?;
        Ok(())
    }

    /// Subscribes to sink and server change events on this socket connection.
    pub fn subscribe(&mut self, mask: u32) -> Result<(), PulseError> {
        let tag = self.next_tag();
        let mut w = TagWriter::new();
        w.put_u32(PA_COMMAND_SUBSCRIBE);
        w.put_u32(tag);
        w.put_u32(mask);
        self.stream.write_all(&w.finish_packet())?;

        let _ = self.read_reply_for_tag(tag, "SUBSCRIBE")?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn tag_writer_encodes_u32_string_cvolume_and_bool() {
        let mut w = TagWriter::new();
        w.put_u32(42);
        w.put_string("RustyOS");
        w.put_bool(true);
        w.put_cvolume(&[65536, 32768]);

        let packet = w.finish_packet();
        assert!(packet.len() > 20);

        // Header check
        let len = u32::from_be_bytes(packet[0..4].try_into().unwrap());
        assert_eq!(len as usize, packet.len() - 20);

        // Reader check
        let mut r = TagReader::new(&packet[20..]);
        assert_eq!(r.get_u32().unwrap(), 42);
        assert_eq!(r.get_string().unwrap().as_deref(), Some("RustyOS"));
        assert!(r.get_bool().unwrap());
        let vols = r.get_cvolume().unwrap();
        assert_eq!(vols, vec![65536, 32768]);
    }

    #[test]
    fn parse_sink_info_fields_decodes_synthetic_payload() {
        let mut w = TagWriter::new();
        w.put_u32(64); // sink index
        w.put_string("alsa_output.pci"); // name
        w.put_string("Built-in Speaker"); // description
        // sample_spec: tag 'a', 6 bytes
        w.buf.push(b'a');
        w.buf.extend_from_slice(&[7, 2, 0, 0, 0xBB, 0x80]);
        // channel_map: tag 'm', 1 + 2 bytes
        w.buf.push(b'm');
        w.buf.extend_from_slice(&[2, 1, 2]);
        // module_index: tag 'L', u32
        w.put_u32(10);
        // cvolume: 50%
        let vol_50 = 32_768_u32;
        w.put_cvolume(&[vol_50, vol_50]);
        // muted: false
        w.put_bool(false);

        let packet = w.finish_packet();
        let mut r = TagReader::new(&packet[20..]);
        let sink = parse_sink_info_fields(&mut r).unwrap();

        assert_eq!(sink.index, 64);
        assert_eq!(sink.name, "alsa_output.pci");
        assert_eq!(sink.description, "Built-in Speaker");
        assert_eq!(sink.volume_percent, 50);
        assert!(!sink.muted);
    }

    #[test]
    fn tag_reader_rejects_truncated_and_deeply_nested_proplists() {
        let mut truncated_m = TagReader::new(&[b'm', 8, 1, 2]);
        assert!(truncated_m.skip_one().is_err());

        let mut truncated_x = TagReader::new(&[b'x', 0, 0, 0, 20, 1, 2]);
        assert!(truncated_x.skip_one().is_err());

        let mut deep_p = vec![b'P'; 32];
        deep_p.extend(std::iter::repeat_n(b'N', 32));
        let mut nested = TagReader::new(&deep_p);
        assert!(nested.skip_one().is_err());
    }

    #[test]
    fn read_packet_rejects_oversized_header_and_skips_subscribe_events() {
        let (mut srv, mut cli) = UnixStream::pair().unwrap();
        // 1. Oversized 16 MiB packet header must fail without allocating 16 MiB.
        let mut bad_hdr = [0u8; 20];
        bad_hdr[0..4].copy_from_slice(&(16_u32 * 1024 * 1024).to_be_bytes());
        srv.write_all(&bad_hdr).unwrap();
        assert!(PulseClient::read_packet(&mut cli).is_err());

        // 2. Interleaved PA_COMMAND_SUBSCRIBE_EVENT is skipped before PA_COMMAND_REPLY.
        let mut evt = TagWriter::new();
        evt.put_u32(PA_COMMAND_SUBSCRIBE_EVENT);
        evt.put_u32(0);
        srv.write_all(&evt.finish_packet()).unwrap();

        let mut reply = TagWriter::new();
        reply.put_u32(PA_COMMAND_REPLY);
        reply.put_u32(7);
        srv.write_all(&reply.finish_packet()).unwrap();

        let mut client = PulseClient {
            stream: cli,
            tag_seq: 8,
        };
        assert!(client.read_reply_for_tag(7, "TEST").is_ok());
    }
}
