//! Correlated bridge replies and the user-facing sentence mapping.
//!
//! Ported from `Aoko/Core/BridgeCommandResult.cs`. The bridge error vocabulary is
//! turned into a sentence the GUI can show as-is; unknown codes never reach the
//! UI because a future `describeThrowable` string can carry Java exception text.

use serde_json::Value;

use crate::capabilities::actions;
use crate::commands::wire_type;

/// Error codes produced by the client itself.
pub mod error {
    pub const NOT_CONNECTED: &str = "not_connected";
    pub const SEND_FAILED: &str = "send_failed";
    pub const TIMEOUT: &str = "timeout";
    pub const CONNECTION_LOST: &str = "connection_lost";
    pub const INVALID_ADDRESS: &str = "invalid_address";
    pub const INVALID_SESSION_DATA: &str = "invalid_session_data";
}

const IN_WORLD_MESSAGE: &str = "Return to the main menu before switching accounts.";
const NOT_READY_MESSAGE: &str = "Wait until the client reaches the main menu, then retry.";
const TIMEOUT_MESSAGE: &str = "The game did not answer in time. Stay on the main menu and retry.";
const CONNECT_PATH_MESSAGE: &str =
    "This client's connect screen could not be resolved, so the join cannot be started.";
const UNMAPPED_CODE_MESSAGE: &str =
    "The game refused the request. Wait until the client reaches the main menu, then retry.";
const NO_REASON_MESSAGE: &str = "The game refused the request without a reason. \
                                 Wait until the client reaches the main menu, then retry.";

/// A decoded `commandResult`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BridgeCommandResult {
    pub ok: bool,
    pub action: String,
    pub username: String,
    pub uuid: String,
    pub in_world: bool,
    pub ready: bool,
    pub error: String,
}

impl BridgeCommandResult {
    /// Builds a result, substituting empty strings for absent text fields.
    #[must_use]
    pub fn new(
        ok: bool,
        action: Option<&str>,
        username: Option<&str>,
        uuid: Option<&str>,
        in_world: bool,
        ready: bool,
        error: Option<&str>,
    ) -> Self {
        Self {
            ok,
            action: action.unwrap_or("").to_owned(),
            username: username.unwrap_or("").to_owned(),
            uuid: uuid.unwrap_or("").to_owned(),
            in_world,
            ready,
            error: error.unwrap_or("").to_owned(),
        }
    }

    /// A command that never reached the bridge.
    #[must_use]
    pub fn failure(action: Option<&str>, error: Option<&str>) -> Self {
        Self::new(false, action, None, None, false, false, error)
    }

    /// Keeps the requested action when the reply omits it.
    #[must_use]
    pub fn with_action(mut self, action: Option<&str>) -> Self {
        if self.action.is_empty() {
            if let Some(action) = action.filter(|value| !value.is_empty()) {
                self.action = action.to_owned();
            }
        }
        self
    }

    /// Decodes a `commandResult` payload.
    ///
    /// Returns `None` for any other message type, a non-object document, or a
    /// missing/non-positive `requestId`, so the read loop leaves such lines alone.
    #[must_use]
    pub fn try_parse(node: &Value) -> Option<(i32, Self)> {
        let Value::Object(payload) = node else {
            return None;
        };

        if !read_string(payload, "type").eq_ignore_ascii_case(wire_type::COMMAND_RESULT) {
            return None;
        }

        let request_id = match payload.get("requestId") {
            Some(Value::Number(number)) => number.as_i64()?,
            _ => return None,
        };
        let request_id = i32::try_from(request_id).ok().filter(|id| *id > 0)?;

        let result = Self {
            ok: read_bool(payload, "ok"),
            action: read_string(payload, "action"),
            username: read_string(payload, "username"),
            uuid: read_string(payload, "uuid"),
            in_world: read_bool(payload, "inWorld"),
            ready: read_bool(payload, "ready"),
            error: read_string(payload, "error"),
        };

        Some((request_id, result))
    }

    /// User-facing sentence for this reply. Never includes a token.
    #[must_use]
    pub fn message(&self) -> String {
        if self.ok {
            self.success_message()
        } else {
            failure_message(&self.error)
        }
    }

    fn success_message(&self) -> String {
        let has_username = !self.username.is_empty();

        if self.action.eq_ignore_ascii_case(actions::GET_SESSION) {
            return if has_username {
                format!("Signed in as {}.", self.username)
            } else {
                "No account is signed in right now.".to_owned()
            };
        }

        if self.action.eq_ignore_ascii_case(actions::SET_SESSION) {
            return if has_username {
                format!("Switched to {}.", self.username)
            } else {
                "Account switched.".to_owned()
            };
        }

        if self.action.eq_ignore_ascii_case(actions::RESTORE_SESSION) {
            return if has_username {
                format!("Restored {}.", self.username)
            } else {
                "Session restored.".to_owned()
            };
        }

        if self.action.eq_ignore_ascii_case(actions::JOIN) {
            return "Join started. The client is connecting to the server.".to_owned();
        }

        "The game completed the command.".to_owned()
    }
}

/// Maps a bridge error code (or free-text reason) to a vetted sentence.
#[must_use]
pub fn failure_message(error: &str) -> String {
    let code = error.trim();
    if code.is_empty() {
        return NO_REASON_MESSAGE.to_owned();
    }

    let normalized = code.to_lowercase();
    let mapped = match normalized.as_str() {
        "in_world" => Some(IN_WORLD_MESSAGE),
        "game_thread_timeout" | error::TIMEOUT => Some(TIMEOUT_MESSAGE),
        "connect_path_not_found" => Some(CONNECT_PATH_MESSAGE),
        "not_ready" | "notready" => Some(NOT_READY_MESSAGE),
        error::INVALID_SESSION_DATA => {
            Some("The account details were rejected. Re-import the account and retry.")
        }
        "unsupported_session_constructor" => Some(
            "This Minecraft version's session constructor is not supported, \
             so the account cannot be switched.",
        ),
        "session_write_not_observed" => {
            Some("The game did not apply the new session. Stay on the main menu and retry.")
        }
        "no_launch_session" => Some("The client has no launch session to restore."),
        error::INVALID_ADDRESS => Some("That server address is not valid."),
        error::NOT_CONNECTED => Some(
            "The GUI is not connected to the game yet. Inject first, then wait for the main menu.",
        ),
        error::SEND_FAILED => Some("The command could not be sent to the game."),
        error::CONNECTION_LOST => {
            Some("The connection to the game dropped before the command finished.")
        }
        _ => None,
    };

    if let Some(message) = mapped {
        return message.to_owned();
    }

    // Free-text reasons such as "Session helper not ready (wait on main menu,
    // then retry)" or "Disconnect from the world before switching" still map.
    if normalized.contains("not ready") {
        return NOT_READY_MESSAGE.to_owned();
    }
    if normalized.contains("disconnect from the world") {
        return IN_WORLD_MESSAGE.to_owned();
    }

    UNMAPPED_CODE_MESSAGE.to_owned()
}

fn read_string(payload: &serde_json::Map<String, Value>, name: &str) -> String {
    payload
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn read_bool(payload: &serde_json::Map<String, Value>, name: &str) -> bool {
    matches!(payload.get(name), Some(Value::Bool(true)))
}
