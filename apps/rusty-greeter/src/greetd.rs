//! Zero-dependency `greetd` JSON-IPC framing over Unix domain sockets (`GREETD_SOCK`, `RUSTYOS-136`).

#![allow(unreachable_pub)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Outgoing `greetd` IPC request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreetdRequest {
    /// Start an authentication session for `username`.
    CreateSession { username: String },
    /// Reply to an auth challenge (e.g. password).
    PostAuthMessageResponse { response: Option<String> },
    /// Launch the authenticated user session command.
    StartSession { cmd: Vec<String>, env: Vec<String> },
    /// Cancel an in-progress authentication session.
    CancelSession,
}

/// Incoming `greetd` IPC response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreetdResponse {
    /// Last request succeeded.
    Success,
    /// Last request failed with `error_type` and `description`.
    Error {
        error_type: String,
        description: String,
    },
    /// PAM / `greetd` requested user interaction (`secret`, `visible`, `info`, `error`).
    AuthMessage {
        auth_message_type: String,
        auth_message: String,
    },
}

fn escape_json(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn format_json_array(items: &[String]) -> String {
    let mut out = String::from("[");
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&escape_json(item));
    }
    out.push(']');
    out
}

/// Serializes a `GreetdRequest` into a native-endian length-prefixed JSON frame.
#[must_use]
pub fn encode_greetd_frame(req: &GreetdRequest) -> Vec<u8> {
    let json = match req {
        GreetdRequest::CreateSession { username } => {
            format!(
                "{{\"type\":\"create_session\",\"username\":{}}}",
                escape_json(username)
            )
        }
        GreetdRequest::PostAuthMessageResponse { response } => match response {
            Some(val) => format!(
                "{{\"type\":\"post_auth_message_response\",\"response\":{}}}",
                escape_json(val)
            ),
            None => "{\"type\":\"post_auth_message_response\"}".to_owned(),
        },
        GreetdRequest::StartSession { cmd, env } => format!(
            "{{\"type\":\"start_session\",\"cmd\":{},\"env\":{}}}",
            format_json_array(cmd),
            format_json_array(env)
        ),
        GreetdRequest::CancelSession => "{\"type\":\"cancel_session\"}".to_owned(),
    };
    let bytes = json.as_bytes();
    let len = u32::try_from(bytes.len()).unwrap_or(0);
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&len.to_ne_bytes());
    frame.extend_from_slice(bytes);
    frame
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut offset = 0;
    while let Some(pos) = json[offset..].find(&needle) {
        let after_key = offset + pos + needle.len();
        let rest = json[after_key..].trim_start();
        if let Some(after_colon) = rest.strip_prefix(':') {
            let val_start = after_colon.trim_start().strip_prefix('"')?;
            let mut out = String::new();
            let mut chars = val_start.chars();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    match chars.next()? {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        other => out.push(other),
                    }
                } else if ch == '"' {
                    return Some(out);
                } else {
                    out.push(ch);
                }
            }
            return None;
        }
        offset = after_key;
    }
    None
}

/// Parses a UTF-8 JSON payload from `greetd` into [`GreetdResponse`].
///
/// # Errors
/// Returns an error if the JSON payload does not contain a recognized `type` field.
pub fn decode_greetd_json(json: &str) -> Result<GreetdResponse, String> {
    let kind = extract_json_string(json, "type")
        .ok_or_else(|| "missing `type` field in greetd response".to_owned())?;
    match kind.as_str() {
        "success" => Ok(GreetdResponse::Success),
        "error" => Ok(GreetdResponse::Error {
            error_type: extract_json_string(json, "error_type").unwrap_or_default(),
            description: extract_json_string(json, "description").unwrap_or_default(),
        }),
        "auth_message" => Ok(GreetdResponse::AuthMessage {
            auth_message_type: extract_json_string(json, "auth_message_type").unwrap_or_default(),
            auth_message: extract_json_string(json, "auth_message").unwrap_or_default(),
        }),
        other => Err(format!("unknown greetd response type `{other}`")),
    }
}

/// Synchronous `greetd` Unix socket client.
pub struct GreetdClient {
    stream: UnixStream,
}

impl GreetdClient {
    /// Connects to the `greetd` IPC socket at `path`.
    ///
    /// # Errors
    /// Returns an error if connecting to the Unix socket fails.
    pub fn connect(path: &Path) -> Result<Self, String> {
        let stream = UnixStream::connect(path)
            .map_err(|err| format!("failed to connect to {}: {err}", path.display()))?;
        Ok(Self { stream })
    }

    /// Sends `req` and reads a single `GreetdResponse` frame.
    ///
    /// # Errors
    /// Returns an error on socket I/O or malformed response framing.
    pub fn roundtrip(&mut self, req: &GreetdRequest) -> Result<GreetdResponse, String> {
        let frame = encode_greetd_frame(req);
        self.stream
            .write_all(&frame)
            .map_err(|err| format!("greetd write failed: {err}"))?;
        let mut len_buf = [0u8; 4];
        self.stream
            .read_exact(&mut len_buf)
            .map_err(|err| format!("greetd header read failed: {err}"))?;
        let len = u32::from_ne_bytes(len_buf) as usize;
        if len == 0 || len > 65_536 {
            return Err(format!("invalid greetd payload length: {len}"));
        }
        let mut payload = vec![0u8; len];
        self.stream
            .read_exact(&mut payload)
            .map_err(|err| format!("greetd payload read failed: {err}"))?;
        let text =
            String::from_utf8(payload).map_err(|err| format!("greetd payload not UTF-8: {err}"))?;
        decode_greetd_json(&text)
    }

    /// Authenticates `username` with `password` and starts `session_cmd`.
    ///
    /// # Errors
    /// Returns an error if authentication or session startup is rejected by `greetd`.
    pub fn authenticate_and_start(
        &mut self,
        username: &str,
        password: &str,
        session_cmd: Vec<String>,
    ) -> Result<(), String> {
        let mut resp = self.roundtrip(&GreetdRequest::CreateSession {
            username: username.to_owned(),
        })?;
        loop {
            match resp {
                GreetdResponse::Success => {
                    let start = self.roundtrip(&GreetdRequest::StartSession {
                        cmd: session_cmd,
                        env: Vec::new(),
                    })?;
                    return match start {
                        GreetdResponse::Success => Ok(()),
                        GreetdResponse::Error { description, .. } => Err(description),
                        GreetdResponse::AuthMessage { auth_message, .. } => Err(auth_message),
                    };
                }
                GreetdResponse::AuthMessage {
                    auth_message_type, ..
                } => {
                    let answer = if auth_message_type == "secret" || auth_message_type == "visible"
                    {
                        Some(password.to_owned())
                    } else {
                        None
                    };
                    resp = self
                        .roundtrip(&GreetdRequest::PostAuthMessageResponse { response: answer })?;
                }
                GreetdResponse::Error { description, .. } => {
                    let _ = self.roundtrip(&GreetdRequest::CancelSession);
                    return Err(description);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greetd_json_ipc_roundtrips_all_request_and_response_variants() -> Result<(), String> {
        let frame = encode_greetd_frame(&GreetdRequest::CreateSession {
            username: "alice".to_owned(),
        });
        let len = u32::from_ne_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(len + 4, frame.len());
        let body = std::str::from_utf8(&frame[4..]).map_err(|e| e.to_string())?;
        assert!(body.contains("\"type\":\"create_session\""));
        assert!(body.contains("\"username\":\"alice\""));

        assert_eq!(
            decode_greetd_json("{\"type\":\"success\"}")?,
            GreetdResponse::Success
        );
        assert_eq!(
            decode_greetd_json(
                "{\"type\":\"auth_message\",\"auth_message_type\":\"secret\",\"auth_message\":\"Password:\"}"
            )?,
            GreetdResponse::AuthMessage {
                auth_message_type: "secret".to_owned(),
                auth_message: "Password:".to_owned(),
            }
        );
        assert_eq!(
            decode_greetd_json(
                "{\"type\":\"error\",\"error_type\":\"auth_error\",\"description\":\"Authentication failed\"}"
            )?,
            GreetdResponse::Error {
                error_type: "auth_error".to_owned(),
                description: "Authentication failed".to_owned(),
            }
        );

        // Live UnixStream pair roundtrip
        let (mut server, client_stream) =
            UnixStream::pair().map_err(|e| format!("socketpair: {e}"))?;
        let mut client = GreetdClient {
            stream: client_stream,
        };
        let handle = std::thread::spawn(move || {
            let mut hdr = [0u8; 4];
            if server.read_exact(&mut hdr).is_err() {
                return;
            }
            let n = u32::from_ne_bytes(hdr) as usize;
            let mut buf = vec![0u8; n];
            if server.read_exact(&mut buf).is_err() {
                return;
            }
            let reply = b"{\"type\":\"success\"}";
            let rlen = u32::try_from(reply.len()).unwrap_or(0);
            let _ = server.write_all(&rlen.to_ne_bytes());
            let _ = server.write_all(reply);
        });
        let resp = client.roundtrip(&GreetdRequest::CancelSession)?;
        assert_eq!(resp, GreetdResponse::Success);
        let _ = handle.join();
        Ok(())
    }
}
