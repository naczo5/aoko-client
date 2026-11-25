//! Discord rich-presence IPC framing.
//!
//! Windows uses a named pipe, Linux a unix domain socket. Both carry the same
//! little-endian opcode/length header, so framing lives here and only the pipe
//! differs per platform.

use std::fmt;

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
pub const MAX_FRAME_PAYLOAD: usize = 64 * 1024;

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

pub trait DiscordRpcTransport: Send {
    fn connect(&mut self, app_id: &str) -> Result<(), DiscordTransportError>;
    fn write_frame(&mut self, opcode: u32, payload: &[u8]) -> Result<(), DiscordTransportError>;
    fn read_frame(&mut self, out: &mut Vec<u8>) -> Result<Option<DiscordFrame>, DiscordTransportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
