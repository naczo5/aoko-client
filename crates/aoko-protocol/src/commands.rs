//! Outbound account/server command frames and inbound line classification.
//!
//! Ported from `Aoko/Core/BridgeCommandProtocol.cs`. The explicit native side is
//! written against these exact lines, so the builders are pinned by literal
//! string tests.

use std::fmt;
use std::sync::atomic::{AtomicI32, Ordering};

use serde::Serialize;
use serde_json::Value;

/// One JSON document per line, terminated by a single LF (`BridgeActions.LineTerminator`).
pub const LINE_TERMINATOR: &str = "\n";

/// Envelope `type` values.
pub mod wire_type {
    pub const CONFIG: &str = "config";
    pub const MODULE_ACTION: &str = "moduleAction";
    pub const ACCOUNT_COMMAND: &str = "accountCommand";
    pub const SERVER_COMMAND: &str = "serverCommand";
    pub const CAPABILITIES: &str = "capabilities";
    pub const STATE: &str = "state";
    /// Accepted inbound, never emitted by a shipped bridge.
    pub const STATE_PATCH: &str = "statePatch";
    pub const COMMAND_RESULT: &str = "commandResult";
}

/// An ACCOUNTS command for the bridge.
///
/// Only [`AccountCommand::set_session`] carries credentials, and neither `Debug`
/// nor `Display` prints them, so this type is safe to log.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountCommand {
    action: String,
    name: Option<String>,
    uuid: Option<String>,
    msa_token: Option<String>,
    refresh_token: Option<String>,
}

impl AccountCommand {
    #[must_use]
    pub fn get_session() -> Self {
        Self {
            action: crate::capabilities::actions::GET_SESSION.to_owned(),
            name: None,
            uuid: None,
            msa_token: None,
            refresh_token: None,
        }
    }

    #[must_use]
    pub fn set_session(
        name: impl Into<String>,
        uuid: impl Into<String>,
        msa_token: impl Into<String>,
        refresh_token: Option<String>,
    ) -> Self {
        Self {
            action: crate::capabilities::actions::SET_SESSION.to_owned(),
            name: Some(name.into()),
            uuid: Some(uuid.into()),
            msa_token: Some(msa_token.into()),
            refresh_token,
        }
    }

    #[must_use]
    pub fn restore_session() -> Self {
        Self {
            action: crate::capabilities::actions::RESTORE_SESSION.to_owned(),
            name: None,
            uuid: None,
            msa_token: None,
            refresh_token: None,
        }
    }

    #[must_use]
    pub fn action(&self) -> &str {
        &self.action
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub fn uuid(&self) -> Option<&str> {
        self.uuid.as_deref()
    }

    /// Minecraft services access token. Never written to logs.
    #[must_use]
    pub fn msa_token(&self) -> Option<&str> {
        self.msa_token.as_deref()
    }

    #[must_use]
    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }

    /// Whether this command sends credentials (`setSession`, case-insensitive).
    #[must_use]
    pub fn carries_credentials(&self) -> bool {
        self.action
            .eq_ignore_ascii_case(crate::capabilities::actions::SET_SESSION)
    }
}

impl fmt::Display for AccountCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountCommand {{ Action = {} }}", self.action)
    }
}

impl fmt::Debug for AccountCommand {
    /// Same redaction as the managed `ToString()`: credentials never reach a log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Monotonic, thread-safe request-id source for bridge commands.
///
/// Deliberately separate from the module-action ids echoed as `lastActionId`.
/// Ids start at 1 because the counter is pre-incremented.
#[derive(Debug, Default)]
pub struct BridgeRequestIdSource {
    next: AtomicI32,
}

impl BridgeRequestIdSource {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_id(&self) -> i32 {
        self.next.fetch_add(1, Ordering::SeqCst) + 1
    }
}

#[derive(Serialize)]
struct AccountCommandLine<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    action: &'a str,
    #[serde(rename = "requestId")]
    request_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uuid: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<&'a str>,
    #[serde(rename = "refreshToken", skip_serializing_if = "Option::is_none")]
    refresh_token: Option<&'a str>,
}

#[derive(Serialize)]
struct ServerCommandLine<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    action: &'static str,
    #[serde(rename = "requestId")]
    request_id: i32,
    host: &'a str,
    port: u16,
}

/// Serialises one `accountCommand` line, LF-terminated.
///
/// `name`, `uuid`, and `token` are written only for `setSession` (empty string
/// substituted for a null field). `refreshToken` is written only when non-empty.
#[must_use]
pub fn build_account_command_line(request_id: i32, command: &AccountCommand) -> String {
    let carries_credentials = command.carries_credentials();
    let refresh_token = command
        .refresh_token
        .as_deref()
        .filter(|token| !token.is_empty());

    let line = AccountCommandLine {
        kind: wire_type::ACCOUNT_COMMAND,
        action: &command.action,
        request_id,
        name: carries_credentials.then_some(command.name.as_deref().unwrap_or("")),
        uuid: carries_credentials.then_some(command.uuid.as_deref().unwrap_or("")),
        token: carries_credentials.then_some(command.msa_token.as_deref().unwrap_or("")),
        refresh_token: if carries_credentials {
            refresh_token
        } else {
            None
        },
    };

    let mut out = serde_json::to_string(&line).unwrap_or_default();
    out.push_str(LINE_TERMINATOR);
    out
}

/// Serialises one `serverCommand` line, LF-terminated. All four fields are
/// always written.
#[must_use]
pub fn build_server_command_line(request_id: i32, host: &str, port: u16) -> String {
    let line = ServerCommandLine {
        kind: wire_type::SERVER_COMMAND,
        action: crate::capabilities::actions::JOIN,
        request_id,
        host,
        port,
    };

    let mut out = serde_json::to_string(&line).unwrap_or_default();
    out.push_str(LINE_TERMINATOR);
    out
}

/// The inbound handler one bridge line must run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeInboundKind {
    /// Anything that is not a reply or a capabilities frame: state or a patch.
    State,
    /// A correlated account/server reply.
    CommandResult,
    /// The capabilities frame a bridge sends once per connection.
    Capabilities,
}

/// Decides the handler for an inbound line from the *parsed* `type` property.
///
/// Matching on serialized text meant a reply written with a space
/// (`"type" : "commandResult"`) or with the property reordered fell through to
/// the state branch, where it deserialized into an empty state and blanked the
/// HUD while the pending command still waited out its budget.
#[must_use]
pub fn classify(node: &Value) -> BridgeInboundKind {
    let kind = read_type(node);

    if kind.eq_ignore_ascii_case(wire_type::COMMAND_RESULT) {
        return BridgeInboundKind::CommandResult;
    }
    if kind.eq_ignore_ascii_case(wire_type::CAPABILITIES) {
        return BridgeInboundKind::Capabilities;
    }

    // A missing `type` is state: the 1.8.9 bridge sends the document untyped.
    BridgeInboundKind::State
}

/// The message type, or an empty string when the line is not an object with a
/// string `type`.
#[must_use]
pub fn read_type(node: &Value) -> &str {
    match node {
        Value::Object(payload) => payload.get("type").and_then(Value::as_str).unwrap_or(""),
        _ => "",
    }
}
