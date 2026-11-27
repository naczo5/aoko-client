//! Which process is listening on the bridge port.
//!
//! `Aoko/Core/TcpPortHelper.cs` is a diagnostic, not part of the wire contract:
//! it runs `netstat -ano -p tcp` and answers "is the selected PID's bridge
//! already listening", "is some other process holding the port" and "did we end
//! up connected to a different PID".
//!
//! Following the placement rule in `MIGRATION_ARCHITECTURE.md`, the spawn lives
//! here and the line model lives in `aoko_protocol::netstat`. The parser is still
//! injectable ([`TcpPortProbe::with_parser`]) so a test or a future dialect can
//! replace it without touching the spawn path.

use std::ffi::OsString;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::error::PlatformError;

/// How long `netstat` gets before it is killed. `TcpPortHelper` used
/// `WaitForExit(5000)`.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Upper bound on captured stdout, so a hostile `netstat` cannot balloon us.
pub const MAX_CAPTURE_BYTES: u64 = 4 * 1024 * 1024;

/// `fn(line, port) -> Option<pid>`, the seam between the spawn and the parser.
pub type NetstatLineParser = fn(&str, u16) -> Option<u32>;

/// The protocol crate's `TcpPortHelper` port, adapted to the pid type this crate
/// reports.
///
/// `aoko_protocol::netstat::parse_listener_pid` answers with an `i32` because the
/// managed helper parsed one; a listening pid is always positive by that
/// function's own contract, so the conversion cannot fail in practice.
#[must_use]
pub fn protocol_listener_pid(line: &str, port: u16) -> Option<u32> {
    let pid = aoko_protocol::netstat::parse_listener_pid(line, port)?;
    u32::try_from(pid).ok().filter(|pid| *pid > 0)
}

/// Spawns `netstat` and hands its stdout to a line parser.
pub struct TcpPortProbe {
    program: OsString,
    parser: NetstatLineParser,
    timeout: Duration,
}

impl Default for TcpPortProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl TcpPortProbe {
    /// The managed configuration: `netstat -ano -p tcp` parsed by `aoko-protocol`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            program: OsString::from("netstat"),
            parser: protocol_listener_pid,
            timeout: PROBE_TIMEOUT,
        }
    }

    /// Replaces the parser, e.g. with another platform's `netstat` dialect.
    #[must_use]
    pub fn with_parser(mut self, parser: NetstatLineParser) -> Self {
        self.parser = parser;
        self
    }

    /// Overrides the program path, for tests and for exotic PATH setups.
    #[must_use]
    pub fn with_program(mut self, program: impl Into<OsString>) -> Self {
        self.program = program.into();
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Pid listening on `port`, or `None` when nothing is.
    ///
    /// A missing `netstat` yields `Ok(None)`, matching `TcpPortHelper`, which
    /// swallowed every failure. Anything else is reported so a banner can explain
    /// why the diagnostic is unavailable.
    pub fn probe(&self, port: u16) -> Result<Option<u32>, PlatformError> {
        let output = match self.capture() {
            Ok(output) => output,
            Err(ProbeFailure::NotInstalled) => {
                tracing::debug!("netstat is not on PATH; port probe is unavailable");
                return Ok(None);
            }
            Err(ProbeFailure::TimedOut) => {
                return Err(PlatformError::os(
                    "netstat",
                    format!("did not finish within {:?}", self.timeout),
                ));
            }
            Err(ProbeFailure::Io(detail)) => {
                return Err(PlatformError::os("netstat", detail));
            }
        };

        Ok(self.probe_output(&output, port))
    }

    /// Parses already-captured output. Split out so the line rules are testable
    /// with no child process.
    #[must_use]
    pub fn probe_output(&self, output: &str, port: u16) -> Option<u32> {
        output.lines().find_map(|line| (self.parser)(line, port))
    }

    /// Every listener pid in the dump, which the host uses to tell "our pid is
    /// already listening" from "another process owns the port".
    #[must_use]
    pub fn probe_output_all(&self, output: &str, port: u16) -> Vec<u32> {
        output
            .lines()
            .filter_map(|line| (self.parser)(line, port))
            .collect()
    }

    fn capture(&self) -> Result<String, ProbeFailure> {
        let mut child = Command::new(&self.program)
            .args(["-ano", "-p", "tcp"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    ProbeFailure::NotInstalled
                } else {
                    ProbeFailure::Io(err.to_string())
                }
            })?;

        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProbeFailure::Io("netstat produced no stdout".to_string()));
        };

        // Read on a helper thread so a wedged child cannot block us forever.
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let result = stdout
                .by_ref()
                .take(MAX_CAPTURE_BYTES)
                .read_to_end(&mut buffer)
                .map(|_| String::from_utf8_lossy(&buffer).into_owned());
            let _ = sender.send(result);
        });

        match receiver.recv_timeout(self.timeout) {
            Ok(Ok(text)) => {
                let _ = child.wait();
                Ok(text)
            }
            Ok(Err(err)) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(ProbeFailure::Io(err.to_string()))
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(ProbeFailure::TimedOut)
            }
        }
    }
}

enum ProbeFailure {
    NotInstalled,
    TimedOut,
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows from `TcpPortHelperTests.ListeningPortRegex_MatchesExpectedLines`,
    /// driven through the whole probe rather than the protocol regex.
    #[test]
    fn probe_output_matches_the_managed_regex_cases() {
        let probe = TcpPortProbe::new();
        let cases: [(&str, u16, bool); 3] = [
            (
                "TCP    0.0.0.0:25590          0.0.0.0:0              LISTENING       12345",
                25590,
                true,
            ),
            (
                "TCP    127.0.0.1:25590        0.0.0.0:0              LISTENING       67890",
                25590,
                true,
            ),
            (
                "TCP    0.0.0.0:255901         0.0.0.0:0              LISTENING       11111",
                25590,
                false,
            ),
        ];

        for (line, port, should_match) in cases {
            assert_eq!(
                probe.probe_output(line, port).is_some(),
                should_match,
                "probe_output({line:?}, {port})"
            );
        }
    }

    #[test]
    fn probe_output_finds_the_listener_among_many_lines() {
        let output = concat!(
            "Active Connections\n",
            "\n",
            "  Proto  Local Address          Foreign Address        State           PID\n",
            "  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       916\n",
            "  TCP    127.0.0.1:25590        0.0.0.0:0              LISTENING       4242\n",
            "  TCP    127.0.0.1:50000        127.0.0.1:25590        ESTABLISHED     7777\n"
        );
        assert_eq!(TcpPortProbe::new().probe_output(output, 25590), Some(4242));
    }

    #[test]
    fn probe_output_all_separates_our_listener_from_a_foreign_one() {
        let output = concat!(
            "  TCP    127.0.0.1:25590        0.0.0.0:0              LISTENING       4242\n",
            "  TCP    127.0.0.1:25590        0.0.0.0:0              LISTENING       5151\n",
            "  TCP    127.0.0.1:50000        127.0.0.1:25590        ESTABLISHED     7777\n"
        );
        assert_eq!(
            TcpPortProbe::new().probe_output_all(output, 25590),
            vec![4242, 5151]
        );
    }

    #[test]
    fn an_injected_parser_takes_over_the_rules() {
        fn always_seven(_line: &str, _port: u16) -> Option<u32> {
            Some(7)
        }
        let probe = TcpPortProbe::new().with_parser(always_seven);
        assert_eq!(probe.probe_output("anything at all", 25590), Some(7));
    }

    #[test]
    fn the_default_parser_is_the_protocol_crate() {
        let line = "  TCP    127.0.0.1:25590        0.0.0.0:0              LISTENING       4242";
        assert_eq!(
            protocol_listener_pid(line, 25590),
            aoko_protocol::netstat::parse_listener_pid(line, 25590)
                .and_then(|pid| u32::try_from(pid).ok())
        );
        assert_eq!(protocol_listener_pid(line, 25590), Some(4242));
    }

    #[test]
    fn a_missing_program_is_reported_as_no_listener() {
        let probe = TcpPortProbe::new().with_program("aoko-nonexistent-netstat-xyz");
        assert_eq!(
            probe.probe(25590).expect("missing netstat is not fatal"),
            None
        );
    }
}
