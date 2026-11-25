//! The inbound codec and the transport-owning client seam.
//!
//! Ported from the read loop and the inbound handlers of
//! `Aoko/Core/GameStateClient.cs`, minus the socket: the real `TcpClient` lives
//! in `aoko-platform`, behind [`crate::transport::Transport`]. Everything that
//! decides what a line *means* is here, so it can be exercised against the
//! in-memory duplex peer.
//!
//! Rules that must not drift:
//!
//! * Dispatch is on the **parsed** `type` property, never on serialized text, so
//!   `{ "type" : "commandResult" }` still completes its request.
//! * A missing `type` on an object means state: the 1.8.9 bridge sends the
//!   document untyped, and a strict discriminator would reject the whole legacy
//!   wire (architecture decision 6).
//! * Decoding is never gated on the capability `state` list (decision 2).
//! * `commandResult` is last-wins on duplicate keys, which `serde_json` gives us.

use serde_json::Value;
use std::time::SystemTime;

use crate::capabilities::BridgeCapabilities;
use crate::command_result::BridgeCommandResult;
use crate::commands::{self, AccountCommand, BridgeInboundKind};
use crate::diagnostics::ManagedTransportDiagnostics;
use crate::hud::HudLayout;
use crate::state::GameState;
use crate::state_patch;
use crate::transport::{LineBuf, RecvOutcome, Transport, TransportError};

/// Why a line was not turned into a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreReason {
    /// The line was not JSON, or not a JSON object.
    NotJson,
    /// The state branch could not decode the document.
    InvalidState,
    /// A `commandResult` with a wrong type, missing `requestId`, or `requestId <= 0`.
    InvalidCommandResult,
}

/// One decoded bridge line.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// A complete or merged state frame, plus any `hudLayout` echo.
    ///
    /// Boxed because `GameState` is by far the largest thing on the wire: a state
    /// frame every frame interval would otherwise make every `Inbound` — and every
    /// `Pump` — pay for it by value.
    State {
        state: Box<GameState>,
        hud_layout: Option<HudLayout>,
    },
    Capabilities(BridgeCapabilities),
    CommandResult {
        request_id: i32,
        result: BridgeCommandResult,
    },
    Ignored(IgnoreReason),
}

/// Decodes inbound lines against the last state and the current capabilities.
#[derive(Debug, Clone)]
pub struct Decoder {
    state: GameState,
    capabilities: BridgeCapabilities,
    injected_version: String,
}

impl Decoder {
    /// Starts from the empty state and the `1.8.9` capability fallback, exactly
    /// like a freshly constructed `GameStateClient`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_injected_version("1.8.9")
    }

    #[must_use]
    pub fn with_injected_version(injected_version: impl Into<String>) -> Self {
        let injected_version = injected_version.into();
        let capabilities = BridgeCapabilities::for_version_fallback(Some(&injected_version));
        Self {
            state: GameState::default(),
            capabilities,
            injected_version,
        }
    }

    #[must_use]
    pub fn state(&self) -> &GameState {
        &self.state
    }

    #[must_use]
    pub fn capabilities(&self) -> &BridgeCapabilities {
        &self.capabilities
    }

    #[must_use]
    pub fn injected_version(&self) -> &str {
        &self.injected_version
    }

    /// Applies a new injected version and republishes the matching fallback, as
    /// `ConnectAsync` does before the `capabilities` frame arrives.
    pub fn set_injected_version(&mut self, injected_version: impl Into<String>) {
        self.injected_version = injected_version.into();
        self.capabilities = BridgeCapabilities::for_version_fallback(Some(&self.injected_version));
    }

    /// Drops the negotiated capabilities and republishes the fallback. This is
    /// what the read loop does in its `finally` block.
    pub fn reset_capabilities_to_fallback(&mut self) {
        self.capabilities = BridgeCapabilities::for_version_fallback(Some(&self.injected_version));
    }

    /// Decodes and applies one line.
    pub fn ingest(&mut self, line: &str) -> Inbound {
        let Ok(root) = serde_json::from_str::<Value>(line) else {
            return Inbound::Ignored(IgnoreReason::NotJson);
        };
        if !root.is_object() {
            return Inbound::Ignored(IgnoreReason::NotJson);
        }

        match commands::classify(&root) {
            BridgeInboundKind::Capabilities => {
                let parsed = BridgeCapabilities::from_payload(&root, &self.capabilities);
                self.capabilities = parsed.clone();
                Inbound::Capabilities(parsed)
            }
            BridgeInboundKind::CommandResult => match BridgeCommandResult::try_parse(&root) {
                Some((request_id, result)) => Inbound::CommandResult { request_id, result },
                None => Inbound::Ignored(IgnoreReason::InvalidCommandResult),
            },
            BridgeInboundKind::State => self.ingest_state(&root),
        }
    }

    fn ingest_state(&mut self, root: &Value) -> Inbound {
        let state = if state_patch::is_partial(root) {
            match root {
                Value::Object(patch) => state_patch::apply(&self.state, patch),
                _ => return Inbound::Ignored(IgnoreReason::InvalidState),
            }
        } else {
            match serde_json::from_value::<GameState>(root.clone()) {
                Ok(state) => state,
                Err(_) => return Inbound::Ignored(IgnoreReason::InvalidState),
            }
        };

        // The hudLayout is applied from the same parsed document, so a partial
        // frame that omits it leaves the previous layout alone.
        let hud_layout = root
            .get("hudLayout")
            .filter(|node| node.is_object())
            .map(HudLayout::from_json);

        let mut state = state;
        state.is_connected = true;
        state.last_update = Some(SystemTime::now());

        self.state = state.clone();
        Inbound::State {
            state: Box::new(state),
            hud_layout,
        }
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcome of one read from the transport.
#[derive(Debug, Clone, PartialEq)]
pub enum Pump {
    Frame(Inbound),
    /// An oversized line was consumed and discarded up to its newline.
    TooLong,
    /// The peer closed the connection.
    Closed,
}

/// Owns a [`Transport`] and the decode state.
///
/// This is the seam `aoko-platform` plugs a real socket into and `aoko-core`
/// drives; the crate itself never opens one.
#[derive(Debug)]
pub struct TransportClient<T: Transport> {
    transport: T,
    decoder: Decoder,
    diagnostics: ManagedTransportDiagnostics,
    line: LineBuf,
}

impl<T: Transport> TransportClient<T> {
    #[must_use]
    pub fn new(transport: T, decoder: Decoder, diagnostics: ManagedTransportDiagnostics) -> Self {
        Self {
            transport,
            decoder,
            diagnostics,
            line: LineBuf::with_capacity(4096),
        }
    }

    #[must_use]
    pub fn decoder(&self) -> &Decoder {
        &self.decoder
    }

    #[must_use]
    pub fn decoder_mut(&mut self) -> &mut Decoder {
        &mut self.decoder
    }

    #[must_use]
    pub fn diagnostics(&self) -> &ManagedTransportDiagnostics {
        &self.diagnostics
    }

    /// Reads and decodes one line.
    pub fn pump(&mut self) -> Result<Pump, TransportError> {
        let outcome = self.transport.recv_line(&mut self.line)?;
        match outcome {
            RecvOutcome::EndOfStream => Ok(Pump::Closed),
            RecvOutcome::TooLong => Ok(Pump::TooLong),
            RecvOutcome::Line => {
                // Frames are UTF-8 JSON; a line that is not valid UTF-8 cannot be
                // one, and is ignored rather than fatal.
                let line = self.line.as_str();
                let frame = self.decoder.ingest(line);
                Ok(Pump::Frame(frame))
            }
        }
    }

    /// Writes one already-serialised line.
    pub fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
        self.transport.send_line(line.as_bytes())
    }

    /// Sends one `accountCommand` line.
    pub fn send_account_command(
        &mut self,
        request_id: i32,
        command: &AccountCommand,
    ) -> Result<(), TransportError> {
        let line = commands::build_account_command_line(request_id, command);
        self.send_line(&line)
    }

    /// Sends one `serverCommand` line.
    pub fn send_server_command(
        &mut self,
        request_id: i32,
        host: &str,
        port: u16,
    ) -> Result<(), TransportError> {
        let line = commands::build_server_command_line(request_id, host, port);
        self.send_line(&line)
    }
}
