//! Discord Rich Presence, state half.
//!
//! Port of the pure part of `Aoko/Core/DiscordRichPresenceService.cs`: the
//! presence text (`BuildPresence`, `:260-301`), the mode/game lines
//! (`:303-352`), and the publish policy (`:167-197`). The IPC client itself
//! (`:395-527`, a named-pipe JSON frame codec) and the retry loop are I/O and
//! live behind `aoko_platform::DiscordRpcTransport`.
//!
//! Two things are preserved that look arbitrary but are not:
//!
//! * A changed signature still waits [`SIGNATURE_PUBLISH_COOLDOWN`]: Minecraft
//!   flips `GuiOpen` on every inventory open, and Discord rate-limits activity
//!   updates per application. A 45 s heartbeat keeps the timestamp alive in
//!   between (`:170-173`).
//! * `truncate` counts characters, not bytes. The managed code slices UTF-16 code
//!   units, which can split a surrogate pair; slicing bytes here would panic and
//!   slicing chars cannot.

use std::time::{Duration, Instant};

use aoko_protocol::state::GameState;

/// The application id the managed build ships with (`:16`).
pub const DEFAULT_APPLICATION_ID: &str = "1488996793374212272";

/// Discord's limit for `details` and `state` (`:297-299`).
pub const MAX_PRESENCE_CHARS: usize = 128;
/// The game line truncates sooner so the mode prefix always fits (`:325`).
pub const MAX_GAME_LINE_CHARS: usize = 80;

/// Minimum spacing between two signature-driven publishes (`:170`).
pub const SIGNATURE_PUBLISH_COOLDOWN: Duration = Duration::from_secs(5);
/// Unconditional re-publish interval (`:171`).
pub const HEARTBEAT: Duration = Duration::from_secs(45);

/// Refresh wait while disabled (`:147`).
pub const REFRESH_DISABLED_MS: u64 = 5_000;
/// Refresh wait after a connect failure (`:158`).
pub const REFRESH_UNAVAILABLE_MS: u64 = 10_000;
/// Refresh wait after a failed activity update (`:186`).
pub const REFRESH_DISCONNECTED_MS: u64 = 2_000;
/// Normal refresh wait (`:196`).
pub const REFRESH_ACTIVE_MS: u64 = 5_000;
/// The refresh wait is sliced so shutdown is not delayed by a full wait (`:246`).
pub const REFRESH_SLICE_MS: u64 = 250;

/// One activity payload plus the signature used to decide whether to re-send it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceSnapshot {
    /// Discord's `details` line.
    pub details: String,
    /// Discord's `state` line.
    pub state: String,
    /// Everything that, when changed, justifies a new publish.
    pub signature: String,
}

impl PresenceSnapshot {
    /// The status text the UI shows after a successful publish (`:392`).
    #[must_use]
    pub fn status_text(&self) -> &'static str {
        "Discord RPC active"
    }
}

/// The inputs `BuildPresence` reads off the clicker and the state client.
#[derive(Debug, Clone, Copy)]
pub struct PresenceInputs<'a> {
    /// `GameStateClient.IsConnected`.
    pub connected: bool,
    /// `GameStateClient.IsInjected`.
    pub injected: bool,
    /// `GameStateClient.InjectedVersion`, unnormalised.
    pub version: Option<&'a str>,
    /// `Clicker.StatusText`.
    pub status_text: &'a str,
    /// Modules that Discord should count (`CountEnabledModules`).
    pub enabled_modules: usize,
    /// `Clicker.IsClicking`.
    pub is_clicking: bool,
    /// `Clicker.IsArmed`.
    pub is_armed: bool,
    /// `Clicker.GtbHelperEnabled`.
    pub gtb_helper_enabled: bool,
    /// `GameStateClient.CurrentState`.
    pub state: &'a GameState,
}

/// `BuildPresence` (`:260-301`).
#[must_use]
pub fn build_presence(inputs: &PresenceInputs<'_>) -> PresenceSnapshot {
    let version = normalize_version_label(inputs.version);
    let (details, state_line) = if !inputs.connected && !inputs.injected {
        (
            format!("aoko client • {version}"),
            String::from("Waiting for Minecraft client"),
        )
    } else if !inputs.connected {
        (
            format!("aoko client • {version}"),
            String::from("Injected • waiting for bridge link"),
        )
    } else {
        (
            format!(
                "aoko client • {version} • {} modules on",
                inputs.enabled_modules
            ),
            build_game_state_line(inputs.state, inputs.gtb_helper_enabled),
        )
    };

    let details = truncate(&details, MAX_PRESENCE_CHARS).to_owned();
    let mode = mode_label(inputs.is_clicking, inputs.is_armed);
    let state = truncate(&format!("{mode} • {state_line}"), MAX_PRESENCE_CHARS).to_owned();

    // Field order and separators are the managed ones: the signature is compared
    // for equality only, but keeping it identical makes a mixed-version session
    // stop re-publishing on every tick.
    // `Health.ToString("0.0")` is part of the signature, so it is rendered once.
    let health = format!("{:.1}", inputs.state.health);
    let signature = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        u8::from(inputs.connected),
        u8::from(inputs.injected),
        version,
        inputs.status_text,
        inputs.enabled_modules,
        u8::from(inputs.state.gui_open),
        inputs.state.screen_name,
        health,
        inputs.state.action_bar,
    );

    PresenceSnapshot {
        details,
        state,
        signature,
    }
}

/// `BuildModeLabel` (`:303-310`).
#[must_use]
pub fn mode_label(is_clicking: bool, is_armed: bool) -> &'static str {
    if is_clicking {
        "Autoclicking"
    } else if is_armed {
        "Armed"
    } else {
        "Idle"
    }
}

/// `BuildGameStateLine` (`:312-327`).
#[must_use]
pub fn build_game_state_line(state: &GameState, gtb_helper_enabled: bool) -> String {
    if state.gui_open {
        return format!(
            "Menu: {}",
            map_screen_name(Some(state.screen_name.as_str()))
        );
    }

    if state.health >= 0.0 {
        // `{0.#}` in managed format: at most one fractional digit, no trailing zero.
        return format!("In game • HP {}", format_health(state.health));
    }

    let action_bar = state.action_bar.trim();
    if !action_bar.is_empty() && gtb_helper_enabled {
        return truncate(&format!("In game • {action_bar}"), MAX_GAME_LINE_CHARS).to_owned();
    }

    String::from("In game")
}

/// `MapScreenName` (`:329-351`).
#[must_use]
pub fn map_screen_name(raw: Option<&str>) -> &'static str {
    let Some(raw) = raw else {
        return "Open";
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "Open";
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("ingamemenu") || lower.contains("pause") {
        "Pause"
    } else if lower.contains("chat") {
        "Chat"
    } else if lower.contains("inventory") {
        "Inventory"
    } else if lower.contains("container") {
        "Container"
    } else if lower.contains("chest") {
        "Chest"
    } else if lower.contains("anvil") {
        "Anvil"
    } else if lower.contains("craft") {
        "Crafting"
    } else {
        "Open"
    }
}

/// `NormalizeVersionLabel` (`:356-370`).
///
/// Returns a `&'static str` for the three recognised version families and the
/// trimmed input otherwise, so the common path does not allocate.
#[must_use]
pub fn normalize_version_label(raw: Option<&str>) -> &str {
    let Some(raw) = raw else {
        return "Unknown";
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "Unknown";
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("1.21") {
        "1.21"
    } else if lower.starts_with("26.") {
        "26.1"
    } else if lower.starts_with("1.8") {
        "1.8.9"
    } else {
        trimmed
    }
}

/// `Truncate` (`:372-373`), counting characters so a multi-byte boundary cannot
/// split a grapheme or panic.
#[must_use]
pub fn truncate(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((byte_index, _)) => &text[..byte_index],
        None => text,
    }
}

/// `{0.#}`: one fractional digit, dropped when it is zero.
fn format_health(health: f32) -> String {
    if !health.is_finite() {
        return String::from("0");
    }
    let rounded = (f64::from(health) * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{rounded:.1}")
    }
}

/// Why the service is waiting, which decides how long the next wait is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshReason {
    /// Discord RPC is off in the profile.
    Disabled,
    /// The connect attempt failed.
    Unavailable,
    /// An activity update failed.
    Disconnected,
    /// Normal polling.
    Active,
}

impl RefreshReason {
    /// `WaitForRefreshAsync` durations (`:147`, `:158`, `:186`, `:196`).
    #[must_use]
    pub const fn wait_ms(self) -> u64 {
        match self {
            Self::Disabled => REFRESH_DISABLED_MS,
            Self::Unavailable => REFRESH_UNAVAILABLE_MS,
            Self::Disconnected => REFRESH_DISCONNECTED_MS,
            Self::Active => REFRESH_ACTIVE_MS,
        }
    }
}

/// The publish decision and its bookkeeping (`:167-197`).
///
/// Kept separate from the transport so the rate limiting is testable: the managed
/// loop consults `DateTimeOffset.UtcNow`, this one takes the `now` the caller
/// already has.
#[derive(Debug, Default)]
pub struct PublishPolicy {
    last_signature: Option<String>,
    last_publish: Option<Instant>,
}

impl PublishPolicy {
    /// Nothing published yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the snapshot should be sent at `now`.
    #[must_use]
    pub fn should_publish(&self, signature: &str, now: Instant) -> bool {
        let Some(last_publish) = self.last_publish else {
            return true;
        };
        let since = now.saturating_duration_since(last_publish);
        let signature_changed = self.last_signature.as_deref() != Some(signature);
        (signature_changed && since >= SIGNATURE_PUBLISH_COOLDOWN) || since >= HEARTBEAT
    }

    /// Records a successful publish.
    pub fn note_published(&mut self, signature: &str, now: Instant) {
        self.last_signature = Some(signature.to_owned());
        self.last_publish = Some(now);
    }

    /// Forgets the last publish, e.g. after a disconnect: the next connection must
    /// re-publish even with an unchanged signature (`:186-190`).
    pub fn forget(&mut self) {
        self.last_signature = None;
        self.last_publish = None;
    }

    /// Last published signature, if any.
    #[must_use]
    pub fn last_signature(&self) -> Option<&str> {
        self.last_signature.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(gui_open: bool, health: f32, screen: Option<&str>) -> GameState {
        GameState {
            gui_open,
            health,
            screen_name: screen.map_or_else(|| String::from("unknown"), str::to_owned),
            ..GameState::default()
        }
    }

    fn inputs<'a>(state: &'a GameState) -> PresenceInputs<'a> {
        PresenceInputs {
            connected: true,
            injected: true,
            version: Some("1.21.4"),
            status_text: "Armed",
            enabled_modules: 7,
            is_clicking: false,
            is_armed: true,
            gtb_helper_enabled: false,
            state,
        }
    }

    #[test]
    fn the_version_label_collapses_known_families() {
        assert_eq!(normalize_version_label(None), "Unknown");
        assert_eq!(normalize_version_label(Some("   ")), "Unknown");
        assert_eq!(normalize_version_label(Some("1.21.4")), "1.21");
        assert_eq!(normalize_version_label(Some("1.21")), "1.21");
        assert_eq!(normalize_version_label(Some("26.1-SNAPSHOT")), "26.1");
        assert_eq!(normalize_version_label(Some("1.8.9")), "1.8.9");
        assert_eq!(normalize_version_label(Some("1.8")), "1.8.9");
        // Anything else is passed through, trimmed.
        assert_eq!(normalize_version_label(Some(" 1.16.5 ")), "1.16.5");
    }

    #[test]
    fn screen_names_map_to_the_managed_labels() {
        assert_eq!(map_screen_name(None), "Open");
        assert_eq!(map_screen_name(Some("")), "Open");
        assert_eq!(
            map_screen_name(Some("net/minecraft/GuiIngameMenu")),
            "Pause"
        );
        assert_eq!(map_screen_name(Some("GuiChat")), "Chat");
        assert_eq!(map_screen_name(Some("GuiInventory")), "Inventory");
        assert_eq!(map_screen_name(Some("GuiContainer")), "Container");
        assert_eq!(map_screen_name(Some("GuiChest")), "Chest");
        assert_eq!(map_screen_name(Some("GuiAnvil")), "Anvil");
        assert_eq!(map_screen_name(Some("GuiCrafting")), "Crafting");
        assert_eq!(map_screen_name(Some("SomethingElse")), "Open");
        // The managed check is a lowercase `Contains`, so case never matters.
        assert_eq!(map_screen_name(Some("GUIPAUSE")), "Pause");
    }

    #[test]
    fn truncation_counts_characters_and_stays_on_a_boundary() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello", 3), "hel");
        assert_eq!(truncate("héllo", 2), "hé");
        // A multi-byte string truncates at a character boundary instead of panicking.
        assert_eq!(truncate("日本語テキスト", 3), "日本語");
        assert_eq!(truncate("", 4), "");
    }

    #[test]
    fn presence_lines_match_the_managed_wording() {
        let connected_state = state(false, 20.0, None);
        let snapshot = build_presence(&inputs(&connected_state));
        assert_eq!(snapshot.details, "aoko client • 1.21 • 7 modules on");
        assert_eq!(snapshot.state, "Armed • In game • HP 20");

        // Not connected, not injected.
        let disconnected = PresenceInputs {
            connected: false,
            injected: false,
            version: None,
            ..inputs(&connected_state)
        };
        let snapshot = build_presence(&disconnected);
        assert_eq!(snapshot.details, "aoko client • Unknown");
        assert_eq!(snapshot.state, "Armed • Waiting for Minecraft client");

        // Injected but no bridge yet.
        let injected = PresenceInputs {
            connected: false,
            injected: true,
            ..inputs(&connected_state)
        };
        assert_eq!(
            build_presence(&injected).state,
            "Armed • Injected • waiting for bridge link"
        );
    }

    #[test]
    fn the_game_line_prefers_the_menu_then_health_then_the_action_bar() {
        let menu = state(true, 20.0, Some("GuiChest"));
        assert_eq!(build_game_state_line(&menu, false), "Menu: Chest");

        // A negative health means the bridge has no health reading yet.
        let no_health = state(false, -1.0, None);
        assert_eq!(build_game_state_line(&no_health, false), "In game");

        let mut with_bar = state(false, -1.0, None);
        with_bar.action_bar = String::from("  Party: 3 players  ");
        // Without the GTB helper the action bar is not published.
        assert_eq!(build_game_state_line(&with_bar, false), "In game");
        assert_eq!(
            build_game_state_line(&with_bar, true),
            "In game • Party: 3 players"
        );

        // A long action bar is truncated to the game-line limit.
        let mut long_bar = state(false, -1.0, None);
        long_bar.action_bar = "x".repeat(200);
        assert_eq!(
            build_game_state_line(&long_bar, true).chars().count(),
            MAX_GAME_LINE_CHARS
        );

        assert_eq!(format_health(20.0), "20");
        assert_eq!(format_health(19.5), "19.5");
        assert_eq!(format_health(19.46), "19.5");
    }

    #[test]
    fn the_mode_label_follows_clicking_then_armed_then_idle() {
        assert_eq!(mode_label(true, true), "Autoclicking");
        assert_eq!(mode_label(true, false), "Autoclicking");
        assert_eq!(mode_label(false, true), "Armed");
        assert_eq!(mode_label(false, false), "Idle");
    }

    #[test]
    fn the_publish_policy_honours_the_cooldown_and_the_heartbeat() {
        let start = Instant::now();
        let mut policy = PublishPolicy::new();

        // First publish always happens.
        assert!(policy.should_publish("a", start));
        policy.note_published("a", start);

        // Unchanged signature: silent until the heartbeat.
        assert!(!policy.should_publish("a", start + SIGNATURE_PUBLISH_COOLDOWN));
        assert!(!policy.should_publish("a", start + HEARTBEAT - Duration::from_millis(1)));
        assert!(policy.should_publish("a", start + HEARTBEAT));

        // Changed signature inside the cooldown: still silent.
        assert!(!policy.should_publish("b", start + Duration::from_millis(1)));
        assert!(policy.should_publish("b", start + SIGNATURE_PUBLISH_COOLDOWN));

        policy.note_published("b", start + SIGNATURE_PUBLISH_COOLDOWN);
        assert_eq!(policy.last_signature(), Some("b"));

        // A disconnect forgets everything, so reconnecting re-publishes at once.
        policy.forget();
        assert_eq!(policy.last_signature(), None);
        assert!(policy.should_publish("b", start + SIGNATURE_PUBLISH_COOLDOWN));
    }

    #[test]
    fn each_refresh_reason_has_the_managed_wait() {
        assert_eq!(RefreshReason::Disabled.wait_ms(), 5_000);
        assert_eq!(RefreshReason::Unavailable.wait_ms(), 10_000);
        assert_eq!(RefreshReason::Disconnected.wait_ms(), 2_000);
        assert_eq!(RefreshReason::Active.wait_ms(), 5_000);
        // The wait is sliced so a stop request is honoured within a quarter second.
        assert!(REFRESH_SLICE_MS <= RefreshReason::Disabled.wait_ms());
    }

    #[test]
    fn the_signature_tracks_exactly_what_the_managed_code_included() {
        let base = state(false, 20.0, None);
        let first = build_presence(&inputs(&base)).signature;

        // The enabled-module count is the fifth managed field, so it participates.
        let more_modules = PresenceInputs {
            enabled_modules: 9,
            ..inputs(&base)
        };
        assert_ne!(build_presence(&more_modules).signature, first);

        // Gui state does appear.
        let gui = state(true, 20.0, Some("GuiChest"));
        assert_ne!(build_presence(&inputs(&gui)).signature, first);

        // Health is compared with one decimal, like `Health.ToString("0.0")`.
        let hurt = state(false, 18.0, None);
        assert_ne!(build_presence(&inputs(&hurt)).signature, first);
        let same = state(false, 20.04, None);
        assert_eq!(build_presence(&inputs(&same)).signature, first);
    }
}
