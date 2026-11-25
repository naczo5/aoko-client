//! Discord rich-presence IPC framing and transport.
//!
//! Windows uses a named pipe, Linux a unix domain socket. Both carry the same
//! little-endian opcode/length header, so framing lives here and only the pipe
//! differs per platform.
//!
//! Framing is ported from `DiscordRichPresenceService.DiscordIpcClient`: a
//! `u32` opcode, a `u32` payload length, then that many UTF-8 bytes of JSON.
//! There is no delimiter, so a reader must consume the header before the body.

use std::fmt;
use std::io::{Read, Write};

/// discord-ipc opcodes.
pub mod op {
    pub const HANDSHAKE: u32 = 0;
    pub const FRAME: u32 = 1;
    pub const CLOSE: u32 = 2;
    pub const PING: u32 = 3;
    pub const PONG: u32 = 4;
}

/// Header size: u32 opcode + u32 payload length.
pub const FRAME_HEADER_LEN: usize = 8;

/// Upper bound on a presence payload, so a hostile peer cannot allocate freely.
///
/// This is the managed `MaxPayloadBytes`: `1024 * 1024`, rejected only when a
/// length is strictly greater, which is how `DiscordIpcClient` bounded a frame.
pub const MAX_FRAME_PAYLOAD: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordFrame {
    pub opcode: u32,
    pub payload: Vec<u8>,
}

impl DiscordFrame {
    /// Serialises header + payload into `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(&self.opcode.to_le_bytes());
        out.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.payload);
    }

    /// Parses a header, returning `(opcode, payload_len)`.
    pub fn parse_header(header: &[u8; FRAME_HEADER_LEN]) -> (u32, usize) {
        let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        (opcode, len as usize)
    }
}

#[derive(Debug)]
pub enum DiscordTransportError {
    NotRunning,
    Io(std::io::Error),
    PayloadTooLarge { len: usize },
}

impl fmt::Display for DiscordTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRunning => f.write_str("Discord IPC socket not found"),
            Self::Io(err) => write!(f, "Discord IPC error: {err}"),
            Self::PayloadTooLarge { len } => {
                write!(f, "Discord frame of {len} bytes exceeds the cap")
            }
        }
    }
}

impl std::error::Error for DiscordTransportError {}

impl From<std::io::Error> for DiscordTransportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

pub trait DiscordRpcTransport: Send {
    /// Connects, sends the `HANDSHAKE` frame and waits for Discord's reply.
    fn connect(&mut self, app_id: &str) -> Result<(), DiscordTransportError>;

    /// Writes one frame. The payload must already be serialized JSON.
    fn write_frame(&mut self, opcode: u32, payload: &[u8]) -> Result<(), DiscordTransportError>;

    /// Reads one frame, returning `Ok(None)` when Discord closed cleanly.
    ///
    /// `out` doubles as the payload scratch: it is cleared on entry and the
    /// allocation moves into the returned frame.
    fn read_frame(
        &mut self,
        out: &mut Vec<u8>,
    ) -> Result<Option<DiscordFrame>, DiscordTransportError>;
}

/// The `HANDSHAKE` payload the managed client sends: `{"v":1,"client_id":"..."}`.
#[must_use]
pub fn handshake_payload(app_id: &str) -> Vec<u8> {
    let payload = serde_json::json!({ "v": 1, "client_id": app_id });
    payload.to_string().into_bytes()
}

/// Reads one frame from `reader`.
///
/// Returns `Ok(None)` when the peer closed cleanly between frames. A peer that
/// closes mid-header or mid-payload is an error, because the stream position is
/// then unrecoverable.
pub fn read_frame_from<R: Read>(
    reader: &mut R,
    scratch: &mut Vec<u8>,
) -> Result<Option<DiscordFrame>, DiscordTransportError> {
    let mut header = [0u8; FRAME_HEADER_LEN];
    let filled = fill(reader, &mut header)?;
    if filled == 0 {
        return Ok(None);
    }
    if filled < FRAME_HEADER_LEN {
        return Err(DiscordTransportError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "Discord IPC peer closed mid-header",
        )));
    }

    let (opcode, len) = DiscordFrame::parse_header(&header);
    if len > MAX_FRAME_PAYLOAD {
        return Err(DiscordTransportError::PayloadTooLarge { len });
    }

    scratch.clear();
    scratch.resize(len, 0);
    let mut filled = 0usize;
    while filled < len {
        let read = reader.read(&mut scratch[filled..])?;
        if read == 0 {
            return Err(DiscordTransportError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Discord IPC peer closed mid-payload",
            )));
        }
        filled += read;
    }

    Ok(Some(DiscordFrame {
        opcode,
        payload: std::mem::take(scratch),
    }))
}

/// Writes one frame to `writer` using `scratch` as the encode buffer.
pub fn write_frame_to<W: Write>(
    writer: &mut W,
    opcode: u32,
    payload: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<(), DiscordTransportError> {
    if payload.len() > MAX_FRAME_PAYLOAD {
        return Err(DiscordTransportError::PayloadTooLarge { len: payload.len() });
    }
    scratch.clear();
    scratch.extend_from_slice(&opcode.to_le_bytes());
    scratch.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    scratch.extend_from_slice(payload);
    writer.write_all(scratch)?;
    writer.flush()?;
    Ok(())
}

fn fill<R: Read>(reader: &mut R, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0usize;
    while filled < buffer.len() {
        let read = reader.read(&mut buffer[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

/// Frame transport over any duplex byte stream.
///
/// `std::fs::File` wrapping a named-pipe handle on Windows and
/// `UnixStream` on Linux both satisfy `Read + Write`, which is what keeps one
/// frame path for both platforms.
pub struct StreamDiscordTransport<S> {
    stream: Option<S>,
    scratch: Vec<u8>,
    app_id: String,
}

impl<S> StreamDiscordTransport<S>
where
    S: Read + Write,
{
    #[must_use]
    pub fn new(stream: S) -> Self {
        Self {
            stream: Some(stream),
            scratch: Vec::with_capacity(FRAME_HEADER_LEN + 512),
            app_id: String::new(),
        }
    }

    /// Sends the handshake and waits for Discord's `READY` frame.
    ///
    /// Mirrors `DiscordIpcClient.TryConnectAsync`: handshake, then one read that
    /// must not fail, or the connection is considered unusable.
    pub fn handshake(&mut self, app_id: &str) -> Result<(), DiscordTransportError> {
        let payload = handshake_payload(app_id);
        let stream = self.stream.as_mut().ok_or_else(|| {
            DiscordTransportError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Discord IPC stream is not connected",
            ))
        })?;
        write_frame_to(stream, op::HANDSHAKE, &payload, &mut self.scratch)?;
        let mut discard = Vec::with_capacity(256);
        match read_frame_from(stream, &mut discard)? {
            Some(_) => {
                self.app_id.clear();
                self.app_id.push_str(app_id);
                Ok(())
            }
            None => Err(DiscordTransportError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Discord IPC closed before the handshake reply",
            ))),
        }
    }

    /// Application id negotiated by the last successful [`Self::handshake`].
    #[must_use]
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    pub fn disconnect(&mut self) {
        self.stream = None;
        self.app_id.clear();
    }
}

impl<S> DiscordRpcTransport for StreamDiscordTransport<S>
where
    S: Read + Write + Send,
{
    fn connect(&mut self, app_id: &str) -> Result<(), DiscordTransportError> {
        if self.stream.is_none() {
            return Err(DiscordTransportError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Discord IPC stream already closed; build a new transport",
            )));
        }
        self.handshake(app_id)
    }

    fn write_frame(&mut self, opcode: u32, payload: &[u8]) -> Result<(), DiscordTransportError> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            DiscordTransportError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Discord IPC stream is not connected",
            ))
        })?;
        write_frame_to(stream, opcode, payload, &mut self.scratch)
    }

    fn read_frame(
        &mut self,
        out: &mut Vec<u8>,
    ) -> Result<Option<DiscordFrame>, DiscordTransportError> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            DiscordTransportError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Discord IPC stream is not connected",
            ))
        })?;
        // `out` doubles as the payload scratch: it is cleared on entry and the
        // allocation moves into the returned frame.
        read_frame_from(stream, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_read_cap_is_the_managed_one_mebibyte() {
        // `DiscordRichPresenceService.MaxPayloadBytes` is `1024 * 1024`; anything
        // tighter would reject frames the managed client accepted.
        assert_eq!(MAX_FRAME_PAYLOAD, 1024 * 1024);
    }

    #[test]
    fn frame_round_trips_through_header() {
        let frame = DiscordFrame {
            opcode: op::HANDSHAKE,
            payload: b"{\"v\":1}".to_vec(),
        };
        let mut buf = Vec::new();
        frame.encode(&mut buf);
        assert_eq!(buf.len(), FRAME_HEADER_LEN + frame.payload.len());

        let mut header = [0u8; FRAME_HEADER_LEN];
        header.copy_from_slice(&buf[..FRAME_HEADER_LEN]);
        let (opcode, len) = DiscordFrame::parse_header(&header);
        assert_eq!(opcode, op::HANDSHAKE);
        assert_eq!(len, frame.payload.len());
    }

    #[test]
    fn header_is_little_endian() {
        let header = [1, 0, 0, 0, 0x10, 0, 0, 0];
        let (opcode, len) = DiscordFrame::parse_header(&header);
        assert_eq!(opcode, op::FRAME);
        assert_eq!(len, 16);
    }

    #[test]
    fn encoded_bytes_match_the_managed_client_layout() {
        let frame = DiscordFrame {
            opcode: op::FRAME,
            payload: b"{}".to_vec(),
        };
        let mut buf = Vec::new();
        frame.encode(&mut buf);
        assert_eq!(buf, vec![1, 0, 0, 0, 2, 0, 0, 0, b'{', b'}']);
    }

    #[test]
    fn handshake_payload_matches_the_managed_client() {
        let payload = handshake_payload("1488996793374212272");
        let json: serde_json::Value = serde_json::from_slice(&payload).expect("valid json");
        // IPC payloads are JSON objects, and object key order is not part of any
        // contract here (only the TCP framing bytes are). The managed client sent
        // `v` then `client_id`; `serde_json` emits them alphabetically.
        assert_eq!(json["v"], 1);
        assert_eq!(json["client_id"], "1488996793374212272");
        assert_eq!(json.as_object().map(serde_json::Map::len), Some(2));
    }

    #[test]
    fn handshake_payload_escapes_the_app_id() {
        let payload = handshake_payload("a\"b");
        let json: serde_json::Value = serde_json::from_slice(&payload).expect("valid json");
        assert_eq!(json["client_id"], "a\"b");
        assert_eq!(json["v"], 1);
    }

    #[test]
    fn read_frame_round_trips_a_written_frame() {
        let mut wire: Vec<u8> = Vec::new();
        let mut scratch = Vec::new();
        write_frame_to(
            &mut wire,
            op::FRAME,
            b"{\"cmd\":\"SET_ACTIVITY\"}",
            &mut scratch,
        )
        .expect("write");

        let mut reader = std::io::Cursor::new(wire);
        let mut frame_buf = Vec::new();
        let frame = read_frame_from(&mut reader, &mut frame_buf)
            .expect("read")
            .expect("a frame");
        assert_eq!(frame.opcode, op::FRAME);
        assert_eq!(frame.payload, b"{\"cmd\":\"SET_ACTIVITY\"}");
    }

    #[test]
    fn read_frame_reports_clean_eof_as_none() {
        let mut reader = std::io::Cursor::new(Vec::new());
        let mut scratch = Vec::new();
        assert!(read_frame_from(&mut reader, &mut scratch)
            .expect("clean eof")
            .is_none());
    }

    #[test]
    fn read_frame_rejects_a_truncated_header() {
        let mut reader = std::io::Cursor::new(vec![1, 0, 0]);
        let mut scratch = Vec::new();
        let err = read_frame_from(&mut reader, &mut scratch).expect_err("truncated header");
        assert!(matches!(err, DiscordTransportError::Io(_)));
    }

    #[test]
    fn read_frame_rejects_a_truncated_payload() {
        let mut reader = std::io::Cursor::new(vec![1, 0, 0, 0, 8, 0, 0, 0, b'{']);
        let mut scratch = Vec::new();
        let err = read_frame_from(&mut reader, &mut scratch).expect_err("truncated payload");
        assert!(matches!(err, DiscordTransportError::Io(_)));
    }

    #[test]
    fn read_frame_rejects_an_oversized_payload_without_allocating_it() {
        let oversized = (MAX_FRAME_PAYLOAD + 1) as u32;
        let mut header = vec![1, 0, 0, 0];
        header.extend_from_slice(&oversized.to_le_bytes());
        let mut reader = std::io::Cursor::new(header);
        let mut scratch = Vec::new();
        match read_frame_from(&mut reader, &mut scratch) {
            Err(DiscordTransportError::PayloadTooLarge { len }) => {
                assert_eq!(len, MAX_FRAME_PAYLOAD + 1);
            }
            other => panic!("expected PayloadTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn write_frame_rejects_an_oversized_payload() {
        let payload = vec![0u8; MAX_FRAME_PAYLOAD + 1];
        let mut wire = Vec::new();
        let mut scratch = Vec::new();
        let err = write_frame_to(&mut wire, op::FRAME, &payload, &mut scratch)
            .expect_err("oversized payload");
        assert!(matches!(err, DiscordTransportError::PayloadTooLarge { .. }));
        assert!(wire.is_empty(), "nothing may be written on rejection");
    }

    /// Two cross-wired byte buffers, so the framing path is exercised on any
    /// host without an OS socket. `read` on an empty inbox is a clean EOF, so
    /// the reply is primed before the handshake runs.
    #[derive(Clone)]
    struct Duplex {
        inbox: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        outbox: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl Duplex {
        fn pair() -> (Self, Self) {
            let to_ours = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let to_theirs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            (
                Self {
                    inbox: to_ours.clone(),
                    outbox: to_theirs.clone(),
                },
                Self {
                    inbox: to_theirs,
                    outbox: to_ours,
                },
            )
        }

        fn prime_inbox(&self, opcode: u32, payload: &[u8]) {
            let mut encoded = Vec::new();
            write_frame_to(&mut encoded, opcode, payload, &mut Vec::new()).expect("encode");
            if let Ok(mut inbox) = self.inbox.lock() {
                inbox.extend_from_slice(encoded.as_slice());
            }
        }

        /// Takes everything the peer wrote to us, which is this side's inbox.
        fn take_received(&self) -> Vec<u8> {
            self.inbox
                .lock()
                .map(|mut buffer| std::mem::take(&mut *buffer))
                .unwrap_or_default()
        }
    }

    impl Read for Duplex {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let Ok(mut inbox) = self.inbox.lock() else {
                return Err(std::io::Error::other("inbox poisoned"));
            };
            let take = buffer.len().min(inbox.len());
            buffer[..take].copy_from_slice(&inbox[..take]);
            inbox.drain(..take);
            Ok(take)
        }
    }

    impl Write for Duplex {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            let Ok(mut outbox) = self.outbox.lock() else {
                return Err(std::io::Error::other("outbox poisoned"));
            };
            outbox.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn stream_transport_handshakes_over_an_in_memory_duplex() {
        let (ours, theirs) = Duplex::pair();
        ours.prime_inbox(op::FRAME, br#"{"cmd":"DISPATCH","evt":"READY"}"#);

        let mut transport = StreamDiscordTransport::new(ours.clone());
        DiscordRpcTransport::connect(&mut transport, "1").expect("handshake");
        assert_eq!(transport.app_id(), "1");
        assert!(transport.is_connected());

        let written = theirs.take_received();
        let mut reader = std::io::Cursor::new(written);
        let mut scratch = Vec::new();
        let frame = read_frame_from(&mut reader, &mut scratch)
            .expect("read handshake")
            .expect("a handshake frame");
        assert_eq!(frame.opcode, op::HANDSHAKE);
        let json: serde_json::Value =
            serde_json::from_slice(&frame.payload).expect("valid handshake json");
        assert_eq!(json["v"], 1);
        assert_eq!(json["client_id"], "1");
    }

    #[test]
    fn stream_transport_fails_the_handshake_when_discord_closes() {
        let (ours, _theirs) = Duplex::pair();
        let mut transport = StreamDiscordTransport::new(ours);
        let err = DiscordRpcTransport::connect(&mut transport, "1").expect_err("closed peer");
        assert!(matches!(err, DiscordTransportError::Io(_)));
        assert!(transport.app_id().is_empty());
    }

    #[test]
    fn stream_transport_rejects_use_after_disconnect() {
        let (ours, _theirs) = Duplex::pair();
        let mut transport = StreamDiscordTransport::new(ours);
        transport.disconnect();
        let err = DiscordRpcTransport::write_frame(&mut transport, op::FRAME, b"{}")
            .expect_err("disconnected");
        assert!(matches!(err, DiscordTransportError::Io(_)));
    }
}
