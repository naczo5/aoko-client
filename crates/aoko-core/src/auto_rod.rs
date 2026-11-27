//! AutoRod: settings, the `moduleAction` frames, and the acknowledgement tracker.
//!
//! **There is no AutoRod loop in the managed host, and none is invented here.**
//! The feature is a bridge-side state machine: the managed side only forwards a
//! press/release frame when the action key is used (`InputHooks.cs:120-131` calls
//! `GameStateClient.SendAutoRodActionAsync` / `SendAutoRodReleaseAsync`), and the
//! native side does the rod switching. What is portable is therefore exactly three
//! things, and all three are here:
//!
//! 1. [`AutoRodSettings`] — the clamps and defaults of `Clicker.cs:1692-1748`.
//! 2. [`action_message`] / [`release_message`] — the wire frames, byte-for-byte
//!    contract from `GameStateClient.cs:953-975`, including the newline terminator
//!    that makes them line-delimited.
//! 3. [`ActionTracker`] — the pending-request bookkeeping from
//!    `GameStateClient.cs:111-139`: an action that the bridge never acknowledges
//!    within [`ACK_TIMEOUT_MS`] is reported once, and results `received`, `queued`
//!    and `started` count as acknowledgements while any other result is an error
//!    worth logging.
//!
//! The press latch and the six runtime gates are
//! [`crate::clicker::latch::PressLatch`] and
//! [`crate::clicker::guards::should_consume_auto_rod_action`].

use aoko_protocol::profile::Profile;

use crate::clicker::guards::should_consume_auto_rod_action;

/// `AutoRodSlotMode` clamp (`Clicker.cs:1697`).
pub const SLOT_MODE_RANGE: (i32, i32) = (0, 9);
/// `AutoRodExtensionTicks` clamp (`Clicker.cs:1732`).
pub const EXTENSION_TICKS_RANGE: (i32, i32) = (1, 40);
/// How long the bridge has to acknowledge an action (`GameStateClient.cs:130`).
pub const ACK_TIMEOUT_MS: i64 = 1_500;

/// The rod settings the bridge receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoRodSettings {
    /// `AutoRodEnabled`: the gate `SendAutoRodActionAsync` checks first.
    pub enabled: bool,
    /// `AutoRodSlotMode`, `0..=9`. `0` means "keep the current slot".
    pub slot_mode: i32,
    /// `AutoRodVerifyForcedSlot`.
    pub verify_forced_slot: bool,
    /// `AutoRodExtensionTicks`, `1..=40`.
    pub extension_ticks: i32,
    /// `AutoRodHoldToExtend`.
    pub hold_to_extend: bool,
}

impl Default for AutoRodSettings {
    fn default() -> Self {
        // `Clicker.cs:1679-1748` and `Profile.cs` defaults.
        Self {
            enabled: false,
            slot_mode: 0,
            verify_forced_slot: true,
            extension_ticks: 4,
            hold_to_extend: false,
        }
    }
}

impl AutoRodSettings {
    /// Applies the property clamps.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.slot_mode = self.slot_mode.clamp(SLOT_MODE_RANGE.0, SLOT_MODE_RANGE.1);
        self.extension_ticks = self
            .extension_ticks
            .clamp(EXTENSION_TICKS_RANGE.0, EXTENSION_TICKS_RANGE.1);
        self
    }

    /// `AutoRodUsesForcedSlot` (`Clicker.cs:1706`).
    #[must_use]
    pub const fn uses_forced_slot(self) -> bool {
        self.slot_mode > 0
    }

    /// `AutoRodUsesFixedExtension` (`Clicker.cs:1749`).
    #[must_use]
    pub const fn uses_fixed_extension(self) -> bool {
        !self.hold_to_extend
    }

    /// Reads the settings out of a profile.
    #[must_use]
    pub fn from_profile(profile: &Profile) -> Self {
        Self {
            enabled: profile.auto_rod_enabled,
            slot_mode: profile.auto_rod_slot_mode(),
            verify_forced_slot: profile.auto_rod_verify_forced_slot,
            extension_ticks: profile.auto_rod_extension_ticks(),
            hold_to_extend: profile.auto_rod_hold_to_extend,
        }
        .clamped()
    }
}

/// `BuildAutoRodActionMessage` (`GameStateClient.cs:953-967`).
///
/// The trailing newline is part of the contract: the bridge reads line-delimited
/// JSON, and `AutoRodManagedTests.ActionMessage_IsNewlineDelimitedAndHasExactContract`
/// asserts it on the managed side.
#[must_use]
pub fn action_message(settings: AutoRodSettings, request_id: u32) -> String {
    let settings = settings.clamped();
    format!(
        "{{\"type\":\"moduleAction\",\"action\":\"autoRod\",\"phase\":\"press\",\
\"enabled\":true,\"requestId\":{request_id},\"slotMode\":{},\"verifyForcedSlot\":{},\
\"extensionTicks\":{},\"holdToExtend\":{}}}\n",
        settings.slot_mode,
        settings.verify_forced_slot,
        settings.extension_ticks,
        settings.hold_to_extend,
    )
}

/// `BuildAutoRodReleaseMessage` (`GameStateClient.cs:969-975`).
///
/// Deliberately carries no `enabled` field: a release must be sent even after the
/// module was switched off, or the rod stays held.
#[must_use]
pub fn release_message() -> String {
    String::from("{\"type\":\"moduleAction\",\"action\":\"autoRod\",\"phase\":\"release\"}\n")
}

/// The six gates the action has to pass (`InputHooks.cs:161-164`).
///
/// Named so a caller does not have to remember the positional order of
/// [`should_consume_auto_rod_action`], which is easy to transpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActionGates {
    /// `AutoRodEnabled`.
    pub enabled: bool,
    /// Transport connected.
    pub connected: bool,
    /// Bridge advertises `autorod`.
    pub supports_module: bool,
    /// `GameState.InWorld`.
    pub in_world: bool,
    /// Any GUI screen is open.
    pub screen_open: bool,
    /// Game window is foreground.
    pub foreground: bool,
}

impl ActionGates {
    /// Whether the module owns this press.
    #[must_use]
    pub fn allows(self) -> bool {
        should_consume_auto_rod_action(
            self.enabled,
            self.supports_module,
            self.connected,
            self.foreground,
            self.in_world,
            self.screen_open,
        )
    }
}

/// What to do on one frame of the action key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionStep {
    /// Send the press frame and remember it as pending.
    Press {
        /// Request id to put in the frame.
        request_id: u32,
    },
    /// The key is still held and the press is already out: nothing to send.
    Hold,
    /// Send the release frame.
    Release,
    /// The key is up and nothing is held: nothing to send.
    Idle,
}

/// Coordinates the action latch, the runtime gates and the send sequence.
#[derive(Debug, Default)]
pub struct AutoRodAction {
    latch: crate::clicker::latch::PressLatch,
    next_request_id: u32,
}

impl AutoRodAction {
    /// A released action with no request in flight.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One frame with the key `down`.
    ///
    /// Mirrors `InputHooks.HandleAutoRodKey` (`InputHooks.cs:120-135`): the physical
    /// key is latched first, the gate is evaluated only on the first frame of the
    /// press, and the release is emitted whenever the press was owned.
    pub fn on_key(&mut self, down: bool, gates: &ActionGates) -> ActionStep {
        if down {
            let begin = self.latch.begin(gates.allows());
            if begin.trigger {
                // Request ids start at 1: zero is the "nothing pending" sentinel.
                self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
                return ActionStep::Press {
                    request_id: self.next_request_id,
                };
            }
            if begin.consume {
                return ActionStep::Hold;
            }
            return ActionStep::Idle;
        }

        if self.latch.end() {
            ActionStep::Release
        } else {
            ActionStep::Idle
        }
    }

    /// Whether a press is currently latched.
    #[must_use]
    pub fn is_down(&self) -> bool {
        self.latch.is_down()
    }
}

/// Outcome of feeding the bridge's last-action report to [`ActionTracker`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckOutcome {
    /// Nothing was pending.
    None,
    /// Still waiting, and the timeout has not passed.
    Pending,
    /// Acknowledged by the bridge.
    Acknowledged {
        /// Request id that was acknowledged.
        request_id: u32,
        /// The bridge's result string, when it sent one that is not a bare
        /// `received`/`queued`/`started`.
        error: Option<&'static str>,
    },
    /// The bridge never answered within [`ACK_TIMEOUT_MS`]. Reported once.
    TimedOut {
        /// Request id that timed out.
        request_id: u32,
    },
}

/// `TrackActionAcknowledgement` (`GameStateClient.cs:111-139`).
#[derive(Debug, Default)]
pub struct ActionTracker {
    pending_id: u32,
    sent_at_ms: i64,
    timeout_logged: bool,
}

impl ActionTracker {
    /// Nothing in flight.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a successfully sent action.
    pub fn note_sent(&mut self, request_id: u32, now_ms: i64) {
        if request_id == 0 {
            return;
        }
        self.pending_id = request_id;
        self.sent_at_ms = now_ms;
        self.timeout_logged = false;
    }

    /// Request id still waiting for an answer, `0` when idle.
    #[must_use]
    pub fn pending(&self) -> u32 {
        self.pending_id
    }

    /// Forgets the pending action, e.g. after a reconnect (`GameStateClient.cs:451`).
    pub fn clear(&mut self) {
        self.pending_id = 0;
        self.sent_at_ms = 0;
        self.timeout_logged = false;
    }

    /// Feeds one `lastActionId`/`lastActionResult` observation.
    ///
    /// `bridge_result` is the raw string the bridge reported. The C# compares it
    /// against three "acknowledged but not finished" values and logs anything else.
    pub fn observe(
        &mut self,
        last_action_id: u32,
        bridge_result: Option<&'static str>,
        now_ms: i64,
    ) -> AckOutcome {
        if self.pending_id == 0 {
            return AckOutcome::None;
        }

        if last_action_id >= self.pending_id {
            let request_id = self.pending_id;
            self.pending_id = 0;
            let error = match bridge_result {
                None => None,
                Some("") | Some("received") | Some("queued") | Some("started") => None,
                Some(other) => Some(other),
            };
            return AckOutcome::Acknowledged { request_id, error };
        }

        if self.sent_at_ms == 0 || now_ms - self.sent_at_ms < ACK_TIMEOUT_MS {
            return AckOutcome::Pending;
        }

        if self.timeout_logged {
            return AckOutcome::Pending;
        }
        self.timeout_logged = true;
        AckOutcome::TimedOut {
            request_id: self.pending_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_gates() -> ActionGates {
        ActionGates {
            enabled: true,
            connected: true,
            supports_module: true,
            in_world: true,
            screen_open: false,
            foreground: true,
        }
    }

    /// `AutoRodManagedTests.ModelAndProfile_DefaultsClampAndAllowVerificationOverride`.
    #[test]
    fn defaults_and_clamps_match_the_managed_properties() {
        let defaults = AutoRodSettings::default();
        assert!(!defaults.enabled);
        assert!(defaults.verify_forced_slot);
        assert_eq!(defaults.extension_ticks, 4);
        assert!(!defaults.hold_to_extend);
        assert_eq!(defaults.slot_mode, 0);
        assert!(!defaults.uses_forced_slot());
        assert!(defaults.uses_fixed_extension());

        let clamped = AutoRodSettings {
            enabled: true,
            slot_mode: 12,
            verify_forced_slot: false,
            extension_ticks: 99,
            hold_to_extend: true,
        }
        .clamped();
        assert_eq!(clamped.slot_mode, 9);
        assert_eq!(clamped.extension_ticks, 40);
        assert!(clamped.uses_forced_slot());
        assert!(!clamped.uses_fixed_extension());

        // The low ends clamp too: extension ticks have a floor of one.
        let low = AutoRodSettings {
            slot_mode: -3,
            extension_ticks: 0,
            ..AutoRodSettings::default()
        }
        .clamped();
        assert_eq!(low.slot_mode, 0);
        assert_eq!(low.extension_ticks, 1);
    }

    /// `AutoRodManagedTests.Profile_RoundTripPreservesAutoRodSettingsAndActionBind`.
    #[test]
    fn a_profile_round_trips_the_rod_settings() {
        let mut profile = Profile::default();
        profile.auto_rod_enabled = true;
        profile.set_auto_rod_slot_mode(7);
        profile.auto_rod_verify_forced_slot = false;
        profile.set_auto_rod_extension_ticks(12);
        profile.auto_rod_hold_to_extend = true;
        profile.auto_rod_action_key = 0x05;

        let json = serde_json::to_string(&profile).expect("profile serializes");
        let restored: Profile = serde_json::from_str(&json).expect("profile parses");

        assert!(restored.auto_rod_enabled);
        assert_eq!(restored.auto_rod_slot_mode(), 7);
        assert!(!restored.auto_rod_verify_forced_slot);
        assert_eq!(restored.auto_rod_extension_ticks(), 12);
        assert!(restored.auto_rod_hold_to_extend);
        assert_eq!(restored.auto_rod_action_key, 0x05);
    }

    /// `AutoRodManagedTests.ActionMessage_IsNewlineDelimitedAndHasExactContract`.
    #[test]
    fn the_action_frame_matches_the_wire_contract() {
        let message = action_message(
            AutoRodSettings {
                enabled: true,
                slot_mode: 12,
                verify_forced_slot: true,
                extension_ticks: 99,
                hold_to_extend: true,
            },
            42,
        );

        assert!(message.ends_with('\n'), "frames must be line-delimited");
        let value: serde_json::Value = serde_json::from_str(&message).expect("valid json");
        assert_eq!(value["type"], "moduleAction");
        assert_eq!(value["action"], "autoRod");
        assert_eq!(value["phase"], "press");
        assert_eq!(value["enabled"], true);
        assert_eq!(value["requestId"], 42);
        // Clamped on the way out, exactly like `Math.Clamp(slotMode, 0, 9)`.
        assert_eq!(value["slotMode"], 9);
        assert_eq!(value["verifyForcedSlot"], true);
        assert_eq!(value["extensionTicks"], 40);
        assert_eq!(value["holdToExtend"], true);

        let release = release_message();
        assert!(release.ends_with('\n'));
        let value: serde_json::Value = serde_json::from_str(&release).expect("valid json");
        assert_eq!(value["type"], "moduleAction");
        assert_eq!(value["action"], "autoRod");
        assert_eq!(value["phase"], "release");
        assert!(value.get("enabled").is_none(), "release carries no enabled");
    }

    /// `AutoRodManagedTests.ActionLatch_TriggersOnceUntilReleaseAndKeepsConsumptionPaired`,
    /// end to end through the gate.
    #[test]
    fn the_action_triggers_once_per_press_and_releases_when_owned() {
        let mut action = AutoRodAction::new();
        let gates = open_gates();

        let first = action.on_key(true, &gates);
        let ActionStep::Press { request_id } = first else {
            panic!("first frame must press: {first:?}");
        };
        assert_eq!(request_id, 1);
        assert!(action.is_down());

        // Key repeat frames do not press again.
        assert_eq!(action.on_key(true, &gates), ActionStep::Hold);
        assert_eq!(action.on_key(true, &gates), ActionStep::Hold);

        assert_eq!(action.on_key(false, &gates), ActionStep::Release);
        assert!(!action.is_down());
        // A second release frame has nothing to send.
        assert_eq!(action.on_key(false, &gates), ActionStep::Idle);

        // The next press gets a fresh id.
        assert_eq!(
            action.on_key(true, &gates),
            ActionStep::Press { request_id: 2 }
        );
    }

    /// A press that starts while the gate is closed is passed through and never
    /// becomes a trigger (`InputHooks.cs:206-222`).
    #[test]
    fn a_blocked_press_is_passed_through_without_a_release() {
        let mut action = AutoRodAction::new();
        let closed = ActionGates {
            screen_open: true,
            ..open_gates()
        };

        assert_eq!(action.on_key(true, &closed), ActionStep::Idle);
        // Still down, still not owned.
        assert_eq!(action.on_key(true, &closed), ActionStep::Idle);
        assert_eq!(action.on_key(false, &closed), ActionStep::Idle);

        // Opening the gate mid-press must not turn it into a trigger.
        let gates = open_gates();
        assert_eq!(action.on_key(true, &closed), ActionStep::Idle);
        assert_eq!(action.on_key(true, &gates), ActionStep::Idle);
        assert_eq!(action.on_key(false, &gates), ActionStep::Idle);
    }

    /// `TrackActionAcknowledgement` (`GameStateClient.cs:111-139`).
    #[test]
    fn an_action_is_acknowledged_or_reported_once_after_the_timeout() {
        let mut tracker = ActionTracker::new();
        assert_eq!(tracker.observe(5, Some("queued"), 0), AckOutcome::None);

        tracker.note_sent(7, 1_000);
        assert_eq!(tracker.pending(), 7);

        // A lower id is not an acknowledgement.
        assert_eq!(tracker.observe(6, None, 1_100), AckOutcome::Pending);
        // A newer id acknowledges, and a bare status is not an error.
        assert_eq!(
            tracker.observe(7, Some("started"), 1_200),
            AckOutcome::Acknowledged {
                request_id: 7,
                error: None
            }
        );
        assert_eq!(tracker.pending(), 0);

        // A real failure result is surfaced once, and the id is still consumed.
        tracker.note_sent(8, 2_000);
        assert_eq!(
            tracker.observe(9, Some("rejected"), 2_050),
            AckOutcome::Acknowledged {
                request_id: 8,
                error: Some("rejected")
            }
        );

        // No answer: reported exactly once after 1500 ms.
        tracker.note_sent(9, 3_000);
        assert_eq!(tracker.observe(8, None, 4_499), AckOutcome::Pending);
        assert_eq!(
            tracker.observe(8, None, 4_500),
            AckOutcome::TimedOut { request_id: 9 }
        );
        assert_eq!(tracker.observe(8, None, 9_000), AckOutcome::Pending);

        // A reconnect clears whatever was pending.
        tracker.clear();
        assert_eq!(tracker.pending(), 0);
        assert_eq!(tracker.observe(0, None, 10_000), AckOutcome::None);
    }
}
