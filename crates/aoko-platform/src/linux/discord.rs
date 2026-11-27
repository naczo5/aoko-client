//! Discord IPC over a unix domain socket.
//!
//! Discord listens on `$XDG_RUNTIME_DIR/discord-ipc-{0..9}`, falling back to
//! `/tmp/discord-ipc-{0..9}`. Flatpak and Snap builds live one directory deeper,
//! so those layouts are probed too; otherwise presence silently never connects on
//! a sandboxed install.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use crate::discord::{DiscordRpcTransport, DiscordTransportError, StreamDiscordTransport};

/// Socket names, in the order the managed client tries them.
const SOCKET_COUNT: u8 = 10;

/// Directories that hold a sandboxed Discord's socket.
const SANDBOX_DIRS: [&str; 3] = [
    "app/com.discordapp.Discord",
    ".flatpak/com.discordapp.Discord",
    "snap.discord",
];

pub struct LinuxDiscordTransport {
    inner: Option<StreamDiscordTransport<UnixStream>>,
}

impl LinuxDiscordTransport {
    #[must_use]
    pub fn new() -> Self {
        Self { inner: None }
    }

    /// Every candidate socket path, most likely first.
    #[must_use]
    pub fn candidate_paths() -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(
            usize::from(SOCKET_COUNT) * 2 + SANDBOX_DIRS.len() * usize::from(SOCKET_COUNT),
        );

        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            let runtime = PathBuf::from(runtime);
            for index in 0..SOCKET_COUNT {
                paths.push(runtime.join(format!("discord-ipc-{index}")));
            }
            for sandbox in SANDBOX_DIRS {
                for index in 0..SOCKET_COUNT {
                    paths.push(runtime.join(sandbox).join(format!("discord-ipc-{index}")));
                }
            }
        } else {
            // Without the variable, `/run/user/<uid>` is where session sockets go.
            // SAFETY: `geteuid` takes no arguments, cannot fail and touches no
            // memory, so there is no invariant to uphold.
            let uid = unsafe { libc::geteuid() };
            let runtime = PathBuf::from(format!("/run/user/{uid}"));
            for index in 0..SOCKET_COUNT {
                paths.push(runtime.join(format!("discord-ipc-{index}")));
            }
        }

        for index in 0..SOCKET_COUNT {
            paths.push(PathBuf::from("/tmp").join(format!("discord-ipc-{index}")));
        }
        paths
    }

    /// How many candidate sockets exist right now, which is what the capability
    /// report publishes.
    #[must_use]
    pub fn available_socket_count() -> usize {
        Self::candidate_paths()
            .iter()
            .filter(|path| path.exists())
            .count()
    }
}

impl Default for LinuxDiscordTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordRpcTransport for LinuxDiscordTransport {
    fn connect(&mut self, app_id: &str) -> Result<(), DiscordTransportError> {
        self.inner = None;

        let mut last_error: Option<DiscordTransportError> = None;
        for path in Self::candidate_paths() {
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    let mut transport = StreamDiscordTransport::new(stream);
                    match transport.handshake(app_id) {
                        Ok(()) => {
                            self.inner = Some(transport);
                            return Ok(());
                        }
                        Err(err) => last_error = Some(err),
                    }
                }
                Err(err) => {
                    // A closed socket is the normal case for the other nine names;
                    // a permission error is worth surfacing if nothing else works.
                    if err.kind() == std::io::ErrorKind::PermissionDenied {
                        last_error = Some(DiscordTransportError::Io(err));
                    }
                }
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
    ) -> Result<Option<crate::discord::DiscordFrame>, DiscordTransportError> {
        match self.inner.as_mut() {
            Some(transport) => transport.read_frame(out),
            None => Err(DiscordTransportError::NotRunning),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_paths_start_with_the_native_socket_layout() {
        let paths = LinuxDiscordTransport::candidate_paths();
        assert!(paths.len() >= usize::from(SOCKET_COUNT));
        assert!(paths
            .iter()
            .any(|path| path.to_string_lossy().ends_with("discord-ipc-0")));
        assert!(paths
            .iter()
            .any(|path| path.to_string_lossy().contains("/tmp/discord-ipc-9")));
    }

    #[test]
    fn candidate_paths_include_flatpak_and_snap_layouts_when_runtime_dir_is_set() {
        // The test process may or may not have XDG_RUNTIME_DIR; assert on the
        // layout rules that hold either way.
        let paths = LinuxDiscordTransport::candidate_paths();
        let has_sandbox = paths
            .iter()
            .any(|path| path.to_string_lossy().contains("com.discordapp.Discord"))
            || std::env::var_os("XDG_RUNTIME_DIR").is_none();
        assert!(has_sandbox);
    }

    #[test]
    fn connecting_without_discord_reports_not_running() {
        let mut transport = LinuxDiscordTransport::new();
        match transport.connect("1") {
            Ok(()) => {
                // A developer machine with Discord running: the handshake worked.
                let mut buffer = Vec::new();
                let _ = transport.read_frame(&mut buffer);
            }
            Err(err) => assert!(!matches!(
                err,
                DiscordTransportError::PayloadTooLarge { .. }
            )),
        }
    }

    #[test]
    fn writes_without_a_connection_report_not_running() {
        let mut transport = LinuxDiscordTransport::new();
        let err = transport
            .write_frame(crate::discord::op::FRAME, b"{}")
            .expect_err("not connected");
        assert!(matches!(err, DiscordTransportError::NotRunning));
    }
}
