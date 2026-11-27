//! Capability negotiation, ported from `Aoko/Core/BridgeCapabilities.cs`.
//!
//! The payload is four **string lists** of lowercase identifiers, never bitflags.
//! Both shipped bridges publish them as compile-time literals in
//! `bridge_capabilities.h`; the host normalises with `Trim().ToLowerInvariant()`
//! and compares case-insensitively.
//!
//! Two rules are load-bearing and come from the arbitration record:
//!
//! 1. `for_version_fallback` preserves the *managed* module and setting tables
//!    exactly as shipped, quirks included (155/143 settings). Those are
//!    deliberately stale against the native headers, so a module or setting a
//!    bridge adds later stays invisible until that bridge's own `capabilities`
//!    frame names it. Do not "fix" them.
//!
//!    `state` is the one array kept in step with the bridges: `LEGACY_STATE_FIELDS`
//!    carries the managed list **plus** the three fields the 1.8.9 bridge started
//!    publishing (`fov`, `viewportwidth`, `viewportheight`). A field the bridge
//!    actually puts on the wire must not answer "unknown" before its handshake
//!    frame arrives (27/32 state fields).
//! 2. Decoding is never gated on the `state` list (architecture decision 2).
//!    Capabilities gate writes and UI affordances, not deserialization.

use std::collections::HashSet;

use serde_json::Value;

/// Session/server command names as advertised in the `actions` array.
pub mod actions {
    /// Advertised action identifiers (`BridgeCapabilities.SupportsAction`).
    pub const ACCOUNT_GET_SESSION: &str = "account.getSession";
    pub const ACCOUNT_SET_SESSION: &str = "account.setSession";
    pub const ACCOUNT_RESTORE_SESSION: &str = "account.restoreSession";
    pub const SERVER_JOIN: &str = "server.join";

    /// Wire action names carried inside `accountCommand` / `serverCommand`.
    pub const GET_SESSION: &str = "getSession";
    pub const SET_SESSION: &str = "setSession";
    pub const RESTORE_SESSION: &str = "restoreSession";
    pub const JOIN: &str = "join";

    /// Every action both shipped bridges advertise, in emission order.
    pub const ALL: [&str; 4] = [
        ACCOUNT_GET_SESSION,
        ACCOUNT_SET_SESSION,
        ACCOUNT_RESTORE_SESSION,
        SERVER_JOIN,
    ];
}

pub const MODERN_MODULES: &[&str] = &[
    "autoclicker",
    "rightclick",
    "jitter",
    "clickinchests",
    "breakblocks",
    "aimassist",
    "triggerbot",
    "killaura",
    "speedbridge",
    "gtbhelper",
    "nametags",
    "nickhider",
    "closestplayer",
    "fightstatus",
    "chestesp",
    "cheststealer",
    "refill",
    "throwpot",
    "autoheal",
    "blockesp",
    "bedplates",
    "reach",
    "velocity",
    "autototem",
    "autorod",
    "autotool",
    "antidebuff",
    "hitdelayfix",
    "fireballwarning",
    "fastplace",
    "nohurtcam",
    "sprint",
    "tnttimer",
    "wtap",
    "jumpreset",
    "hitselect",
    "trajectories",
    "murdermystery",
    "breakprogress",
    "pixelpartyassist",
    "hudeditor",
];

pub const MODERN_SETTINGS: &[&str] = &[
    "mincps",
    "maxcps",
    "left",
    "right",
    "rightmincps",
    "rightmaxcps",
    "rightblock",
    "breakblocks",
    "jitter",
    "clickinchests",
    "aimassistfov",
    "aimassistrange",
    "aimassiststrength",
    "triggerbot",
    "killaura",
    "killaurarange",
    "killauraaimrange",
    "killauraminturnspeed",
    "killauramaxturnspeed",
    "killauratargetmode",
    "killauraswitchdelayms",
    "killaurarandomization",
    "killauraclickmode",
    "killauramincps",
    "killauramaxcps",
    "killaurafov",
    "killaurarotmode",
    "killaurarequirepress",
    "killauraweaponsonly",
    "speedbridge",
    "speedbridgeblockonly",
    "speedbridgedelayms",
    "speedbridgeholdingshiftonly",
    "speedbridgelookingdownonly",
    "gtbhint",
    "gtbcount",
    "gtbpreview",
    "nametags",
    "nickhiderenabled",
    "nickhideralias",
    "closestplayerinfo",
    "fightstatus",
    "keybindfightstatus",
    "nametagshowhealth",
    "nametagshowarmor",
    "nametagshowhelditem",
    "nametaghidevanilla",
    "nametagsrange",
    "chestesp",
    "chesteprange",
    "cheststealerenabled",
    "cheststealerdelayms",
    "cheststealermenucheck",
    "cheststealertitlecheck",
    "cheststealercustomitemscheck",
    "cheststealerphysicalcheck",
    "refillenabled",
    "refilldelayms",
    "throwpotenabled",
    "keybindthrowpot",
    "autohealenabled",
    "autohealhealth",
    "autohealdelayms",
    "blockespenabled",
    "blockespboxes",
    "blockesptracers",
    "blockesphud",
    "blockespmaxcount",
    "blockesprange",
    "blockespblocks",
    "keybindblockesp",
    "bedplatesenabled",
    "bedplatesshowdistance",
    "bedplatesrange",
    "keybindbedplates",
    "reachenabled",
    "reachmin",
    "reachmax",
    "reachchance",
    "reachchancemode",
    "reachonlywhilesprinting",
    "reachdisableinwater",
    "reachverticalcheck",
    "velocityenabled",
    "velocityhorizontal",
    "velocityvertical",
    "velocitychance",
    "reloadmappingsnonce",
    "showmodulelist",
    "moduleliststyle",
    "showlogo",
    "guitheme",
    "keybindautoclicker",
    "keybindtriggerbot",
    "keybindspeedbridge",
    "keybindnametags",
    "keybindclosestplayer",
    "keybindchestesp",
    "keybindcheststealer",
    "keybindrefill",
    "autototemenabled",
    "autototemmode",
    "autototemhealth",
    "autototemelytra",
    "autototemdelay",
    "autototembehaviormode",
    "autorodenabled",
    "autorodslotmode",
    "autorodverifyforcedslot",
    "autorodextensionticks",
    "autorodholdtoextend",
    "keybindautorod",
    "autotoolenabled",
    "autotoolswapweapon",
    "autotoolinstantswap",
    "autotoolswaptodelay",
    "autotoolswapback",
    "autotoolswapbackdelay",
    "autotoolrequiremousedown",
    "autotoolonlysneaking",
    "autotoolbedwarsmode",
    "keybindautotool",
    "antidebuffenabled",
    "hitdelayfixenabled",
    "fireballwarning",
    "fireballwarningbox",
    "fireballwarningarrow",
    "fireballwarningsound",
    "keybindfireballwarning",
    "fastplace",
    "keybindfastplace",
    "nohurtcam",
    "keybindnohurtcam",
    "sprint",
    "keybindsprint",
    "tnttimer",
    "keybindtnttimer",
    "wtap",
    "keybindwtap",
    "jumpreset",
    "keybindjumpreset",
    "hitselect",
    "keybindhitselect",
    "trajectories",
    "keybindtrajectories",
    "murdermystery",
    "keybindmurdermystery",
    "breakprogress",
    "keybindbreakprogress",
    "jumpresetchance",
    "pixelpartyassist",
    "pixelpartyscanradius",
    "pixelpartyautolook",
    "pixelpartyautowalk",
    "hudeditor",
];

pub const MODERN_STATE_FIELDS: &[&str] = &[
    "inworld",
    "actionbar",
    "holdingblock",
    "lookingatblock",
    "fov",
    "viewportwidth",
    "viewportheight",
    "lookingatentity",
    "lookingatentitylatched",
    "breakingblock",
    "lookingatentityhurttime",
    "modulestatus",
    "attackcooldown",
    "attackcooldownpertick",
    "killauraunavailablereason",
    "killaurahastarget",
    "killaurablocking",
    "lastactionid",
    "lastactionresult",
    "statems",
    "cheststealerstate",
    "refillstate",
    "pixelpartytargetfound",
    "pixelpartytargetyaw",
    "pixelpartytargetdist",
    "pixelpartyyawdelta",
    "hudlayout",
];

pub const LEGACY_MODULES: &[&str] = &[
    "autoclicker",
    "rightclick",
    "jitter",
    "clickinchests",
    "breakblocks",
    "aimassist",
    "killaura",
    "speedbridge",
    "gtbhelper",
    "nametags",
    "nickhider",
    "closestplayer",
    "fightstatus",
    "chestesp",
    "cheststealer",
    "refill",
    "throwpot",
    "autoheal",
    "blockesp",
    "bedplates",
    "reach",
    "velocity",
    "autorod",
    "autotool",
    "antidebuff",
    "hitdelayfix",
    "fireballwarning",
    "fastplace",
    "nohurtcam",
    "sprint",
    "tnttimer",
    "wtap",
    "jumpreset",
    "hitselect",
    "trajectories",
    "murdermystery",
    "breakprogress",
    "pixelpartyassist",
    "hudeditor",
];

pub const LEGACY_SETTINGS: &[&str] = &[
    "mincps",
    "maxcps",
    "left",
    "right",
    "rightmincps",
    "rightmaxcps",
    "rightblock",
    "breakblocks",
    "jitter",
    "clickinchests",
    "aimassistfov",
    "aimassistrange",
    "aimassiststrength",
    "killaura",
    "killaurarange",
    "killauraaimrange",
    "killauraminturnspeed",
    "killauramaxturnspeed",
    "killauratargetmode",
    "killauraswitchdelayms",
    "killaurarandomization",
    "killauraclickmode",
    "killauramincps",
    "killauramaxcps",
    "killaurafov",
    "killaurarotmode",
    "killaurarequirepress",
    "killauraweaponsonly",
    "speedbridge",
    "speedbridgeblockonly",
    "speedbridgedelayms",
    "speedbridgeholdingshiftonly",
    "speedbridgelookingdownonly",
    "nametags",
    "nickhiderenabled",
    "nickhideralias",
    "closestplayerinfo",
    "fightstatus",
    "keybindfightstatus",
    "nametagshowhealth",
    "nametagshowarmor",
    "nametagshowhelditem",
    "nametaghidevanilla",
    "nametagsrange",
    "chestesp",
    "chesteprange",
    "cheststealerenabled",
    "cheststealerdelayms",
    "cheststealermenucheck",
    "cheststealertitlecheck",
    "cheststealercustomitemscheck",
    "cheststealerphysicalcheck",
    "refillenabled",
    "refilldelayms",
    "throwpotenabled",
    "keybindthrowpot",
    "autohealenabled",
    "autohealhealth",
    "autohealdelayms",
    "blockespenabled",
    "blockespboxes",
    "blockesptracers",
    "blockesphud",
    "blockespmaxcount",
    "blockesprange",
    "blockespblocks",
    "bedplatesenabled",
    "bedplatesshowdistance",
    "bedplatesrange",
    "keybindbedplates",
    "reachenabled",
    "reachmin",
    "reachmax",
    "reachchance",
    "reachchancemode",
    "reachonlywhilesprinting",
    "reachdisableinwater",
    "reachverticalcheck",
    "velocityenabled",
    "velocityhorizontal",
    "velocityvertical",
    "velocitychance",
    "autorodenabled",
    "autorodslotmode",
    "autorodverifyforcedslot",
    "autorodextensionticks",
    "autorodholdtoextend",
    "keybindautorod",
    "autotoolenabled",
    "autotoolswapweapon",
    "autotoolinstantswap",
    "autotoolswaptodelay",
    "autotoolswapback",
    "autotoolswapbackdelay",
    "autotoolrequiremousedown",
    "autotoolonlysneaking",
    "autotoolbedwarsmode",
    "keybindautotool",
    "antidebuffenabled",
    "hitdelayfixenabled",
    "fireballwarning",
    "fireballwarningbox",
    "fireballwarningarrow",
    "fireballwarningsound",
    "keybindfireballwarning",
    "fastplace",
    "keybindfastplace",
    "nohurtcam",
    "keybindnohurtcam",
    "sprint",
    "keybindsprint",
    "tnttimer",
    "keybindtnttimer",
    "wtap",
    "keybindwtap",
    "jumpreset",
    "keybindjumpreset",
    "hitselect",
    "keybindhitselect",
    "trajectories",
    "keybindtrajectories",
    "murdermystery",
    "keybindmurdermystery",
    "breakprogress",
    "keybindbreakprogress",
    "jumpresetchance",
    "showmodulelist",
    "moduleliststyle",
    "showlogo",
    "guitheme",
    "keybindautoclicker",
    "keybindspeedbridge",
    "keybindnametags",
    "keybindclosestplayer",
    "keybindchestesp",
    "keybindcheststealer",
    "keybindrefill",
    "keybindblockesp",
    "pixelpartyassist",
    "pixelpartyscanradius",
    "pixelpartyautolook",
    "pixelpartyautowalk",
    "hudeditor",
];

/// State fields the 1.8.9 bridge publishes.
///
/// The managed list plus `fov`, `viewportwidth` and `viewportheight`: the legacy
/// bridge emits the camera FOV and the client-rect viewport it projects entities in
/// (`ReadCameraState18` and `GetClientRect` in `bridge.cpp`) on every state line, and
/// its own `capabilities` frame advertises all three, so the answer before that
/// frame arrives must not be "unknown" for a field that is already on the wire.
pub const LEGACY_STATE_FIELDS: &[&str] = &[
    "inworld",
    "actionbar",
    "holdingblock",
    "lookingatblock",
    "fov",
    "viewportwidth",
    "viewportheight",
    "lookingatentity",
    "lookingatentitylatched",
    "breakingblock",
    "lookingatentityhurttime",
    "modulestatus",
    "attackcooldown",
    "attackcooldownpertick",
    "killauraunavailablereason",
    "killaurahastarget",
    "killaurablocking",
    "lastactionid",
    "lastactionresult",
    "statems",
    "cheststealerstate",
    "refillstate",
    "pitch",
    "health",
    "posx",
    "posy",
    "posz",
    "pixelpartytargetfound",
    "pixelpartytargetyaw",
    "pixelpartytargetdist",
    "pixelpartyyawdelta",
    "hudlayout",
];

/// Capability set advertised by the connected bridge.
///
/// Every stored identifier is already trimmed and lowercased, so lookups are a
/// single hash probe for the common (already-lowercase) call site.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BridgeCapabilities {
    modules: HashSet<Box<str>>,
    settings: HashSet<Box<str>>,
    state_fields: HashSet<Box<str>>,
    actions: HashSet<Box<str>>,
}

impl BridgeCapabilities {
    /// Empty set. Present so a caller can build up a payload for tests.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Synthesises the capability set the host publishes before a
    /// `capabilities` frame arrives (and again after every disconnect).
    ///
    /// `1.21*` and `26.*` select the modern table; everything else, including an
    /// absent version, selects legacy. This mirrors `GameStateClient`, which
    /// starts with `ForVersionFallback("1.8.9")`. Module and setting ids are the
    /// managed tables verbatim; the legacy `state` list is the managed list plus the
    /// three fields its bridge publishes, as the module note records.
    #[must_use]
    pub fn for_version_fallback(injected_version: Option<&str>) -> Self {
        let version = injected_version.unwrap_or("").trim();
        let is_121 = !version.is_empty() && version.to_lowercase().starts_with("1.21");
        let is_261 = !version.is_empty() && version.to_lowercase().starts_with("26.");

        if is_121 || is_261 {
            Self {
                modules: build_set(MODERN_MODULES),
                settings: build_set(MODERN_SETTINGS),
                state_fields: build_set(MODERN_STATE_FIELDS),
                actions: build_action_set(),
            }
        } else {
            Self {
                modules: build_set(LEGACY_MODULES),
                settings: build_set(LEGACY_SETTINGS),
                state_fields: build_set(LEGACY_STATE_FIELDS),
                actions: build_action_set(),
            }
        }
    }

    #[must_use]
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    #[must_use]
    pub fn setting_count(&self) -> usize {
        self.settings.len()
    }

    #[must_use]
    pub fn state_field_count(&self) -> usize {
        self.state_fields.len()
    }

    #[must_use]
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Module ids the bridge can drive.
    pub fn modules(&self) -> impl Iterator<Item = &str> {
        self.modules.iter().map(Box::as_ref)
    }

    /// Config key names the bridge advertises. Not identical to the keys it
    /// actually parses; see `WIRE_CONTRACT.md`.
    pub fn settings(&self) -> impl Iterator<Item = &str> {
        self.settings.iter().map(Box::as_ref)
    }

    /// State field names the bridge advertises.
    pub fn state_fields(&self) -> impl Iterator<Item = &str> {
        self.state_fields.iter().map(Box::as_ref)
    }

    /// Session/server command names the bridge advertises.
    pub fn actions(&self) -> impl Iterator<Item = &str> {
        self.actions.iter().map(Box::as_ref)
    }

    #[must_use]
    pub fn supports_module(&self, module_id: &str) -> bool {
        contains_normalized(&self.modules, module_id)
    }

    #[must_use]
    pub fn supports_setting(&self, setting_name: &str) -> bool {
        contains_normalized(&self.settings, setting_name)
    }

    #[must_use]
    pub fn supports_state_field(&self, field_name: &str) -> bool {
        contains_normalized(&self.state_fields, field_name)
    }

    /// Whether the bridge advertises a command such as `account.setSession`.
    /// Actions are not modules, settings, or state fields.
    #[must_use]
    pub fn supports_action(&self, action_name: &str) -> bool {
        contains_normalized(&self.actions, action_name)
    }

    /// Decodes a `capabilities` payload over `fallback`.
    ///
    /// Per-array rules: a missing or empty array inherits that array from the
    /// fallback. If all four are empty the whole payload is rejected and the
    /// fallback is kept. Non-string array elements are skipped rather than
    /// aborting the parse.
    #[must_use]
    pub fn from_payload(payload: &Value, fallback: &Self) -> Self {
        let modules = parse_string_array(payload.get("modules"));
        let settings = parse_string_array(payload.get("settings"));
        let state_fields = parse_string_array(payload.get("state"));
        let actions = parse_string_array(payload.get("actions"));

        if modules.is_empty()
            && settings.is_empty()
            && state_fields.is_empty()
            && actions.is_empty()
        {
            return fallback.clone();
        }

        Self {
            modules: inherit_if_empty(modules, &fallback.modules),
            settings: inherit_if_empty(settings, &fallback.settings),
            state_fields: inherit_if_empty(state_fields, &fallback.state_fields),
            actions: inherit_if_empty(actions, &fallback.actions),
        }
    }
}

fn inherit_if_empty(parsed: HashSet<Box<str>>, fallback: &HashSet<Box<str>>) -> HashSet<Box<str>> {
    if parsed.is_empty() {
        fallback.clone()
    } else {
        parsed
    }
}

/// Allocation-free membership test: the inverted string is only built when the
/// query is not already in canonical form.
fn contains_normalized(set: &HashSet<Box<str>>, query: &str) -> bool {
    let needs_normalizing = query
        .bytes()
        .any(|byte| byte.is_ascii_uppercase() || byte == b' ' || byte == b'\t');
    if !needs_normalizing {
        return set.contains(query);
    }

    let normalized = query.trim().to_lowercase();
    !normalized.is_empty() && set.contains(normalized.as_str())
}

fn build_set(values: &[&str]) -> HashSet<Box<str>> {
    let mut set = HashSet::with_capacity(values.len());
    for value in values {
        let normalized = value.trim().to_lowercase();
        if !normalized.is_empty() {
            set.insert(normalized.into_boxed_str());
        }
    }
    set
}

fn build_action_set() -> HashSet<Box<str>> {
    build_set(&actions::ALL)
}

/// A non-array node parses as empty, matching `ParseStringArray` in the host.
fn parse_string_array(node: Option<&Value>) -> HashSet<Box<str>> {
    let Some(Value::Array(items)) = node else {
        return HashSet::new();
    };

    let mut set = HashSet::with_capacity(items.len());
    for item in items {
        let Value::String(raw) = item else {
            continue;
        };
        let normalized = raw.trim().to_lowercase();
        if !normalized.is_empty() {
            set.insert(normalized.into_boxed_str());
        }
    }
    set
}
