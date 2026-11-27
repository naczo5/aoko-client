//! Discord IPC over a named pipe.
//!
//! Discord listens on `\\.\pipe\discord-ipc-{0..9}`. The managed client tried each
//! in order and handshook on the first that accepted, which is what
//! [`DiscordRpcTransport::connect`] reproduces; the pipe handle is adopted by a
//! `std::fs::File`, so reads and writes go through the same `StreamDiscordTransport`
//! framing as the Linux socket.

use std::os::windows::io::FromRawHandle;

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

use crate::discord::{
    DiscordFrame, DiscordRpcTransport, DiscordTransportError, StreamDiscordTransport,
};

use super::handles::OwnedHandle;

/// `GENERIC_READ | GENERIC_WRITE`.
const GENERIC_READ_WRITE: u32 = 0xC000_0000;

/// Socket count the managed client probes.
const PIPE_COUNT: u8 = 10;

pub struct WindowsDiscordTransport {
    inner: Option<StreamDiscordTransport<std::fs::File>>,
}

impl WindowsDiscordTransport {
    #[must_use]
    pub fn new() -> Self {
        Self { inner: None }
    }

    /// `\\.\pipe\discord-ipc-{index}`.
    #[must_use]
    pub fn pipe_name(index: u8) -> String {
        format!(r"\\.\pipe\discord-ipc-{index}")
    }

    /// Whether a Discord pipe exists right now.
    ///
    /// Opens with zero desired access, which is the documented way to test for a
    /// named pipe without writing to it.
    #[must_use]
    pub fn pipe_exists(index: u8) -> bool {
        open_pipe(index, 0).is_some()
    }

    #[must_use]
    pub fn available_pipe_count() -> usize {
        (0..PIPE_COUNT)
            .filter(|index| Self::pipe_exists(*index))
            .count()
    }
}

impl Default for WindowsDiscordTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordRpcTransport for WindowsDiscordTransport {
    fn connect(&mut self, app_id: &str) -> Result<(), DiscordTransportError> {
        self.inner = None;

        let mut last_error: Option<DiscordTransportError> = None;
        for index in 0..PIPE_COUNT {
            let Some(handle) = open_pipe(index, GENERIC_READ_WRITE) else {
                continue;
            };
            // SAFETY: `handle` is an owned synchronous pipe handle from
            // `CreateFileW`, and `File` takes over closing it.
            let file = unsafe { std::fs::File::from_raw_handle(handle.into_raw().0) };
            let mut transport = StreamDiscordTransport::new(file);
            match transport.handshake(app_id) {
                Ok(()) => {
                    self.inner = Some(transport);
                    return Ok(());
                }
                Err(err) => last_error = Some(err),
            }
        }

        Err(last_error.unwrap_or(DiscordTransportError::NotRunning))
    }

    fn write_frame(&mut self, opcode: u32, payload: &[u8]) -> Result<(), DiscordTransportError> {
        match self.inner.as_mut() {
            Some(transport) => transport.write_frame(opcode, payload),
            None => Err(DiscordTransportError::NotRunning),
        }
    }

    fn read_frame(
        &mut self,
        out: &mut Vec<u8>,
    ) -> Result<Option<DiscordFrame>, DiscordTransportError> {
        match self.inner.as_mut() {
            Some(transport) => transport.read_frame(out),
            None => Err(DiscordTransportError::NotRunning),
        }
    }
}

fn open_pipe(index: u8, desired_access: u32) -> Option<OwnedHandle> {
    let name = WindowsDiscordTransport::pipe_name(index);
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: `wide` is a live, NUL-terminated UTF-16 buffer that outlives the call,
    // and every other argument is a plain flag value.
    let result = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    };

    match result {
        Ok(handle) => OwnedHandle::new(handle),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_names_use_the_windows_named_pipe_path() {
        assert_eq!(
            WindowsDiscordTransport::pipe_name(0),
            r"\\.\pipe\discord-ipc-0"
        );
        assert_eq!(
            WindowsDiscordTransport::pipe_name(9),
            r"\\.\pipe\discord-ipc-9"
        );
    }

    #[test]
    fn probing_for_pipes_never_panics_when_discord_is_absent() {
        let count = WindowsDiscordTransport::available_pipe_count();
        assert!(count <= usize::from(PIPE_COUNT));
    }

    #[test]
    fn writes_without_a_connection_report_not_running() {
        let mut transport = WindowsDiscordTransport::new();
        let err = transport
            .write_frame(crate::discord::op::FRAME, b"{}")
            .expect_err("not connected");
        assert!(matches!(err, DiscordTransportError::NotRunning));
    }

    #[test]
    fn connecting_without_discord_is_an_error_not_a_panic() {
        let mut transport = WindowsDiscordTransport::new();
        match transport.connect("1") {
            Ok(()) => {
                let mut buffer = Vec::new();
                let _ = transport.read_frame(&mut buffer);
            }
            Err(err) => {
                // Discord absent, or the app id was rejected by a running Discord.
                assert!(!matches!(
                    err,
                    DiscordTransportError::PayloadTooLarge { .. }
                ));
            }
        }
    }
}
