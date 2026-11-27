//! Kill Aura configuration, ported from `Aoko/Core/KillAuraSettings.cs`.
//!
//! OpenMyau-Plus parity at commit `fc7c95d`. Mode strings follow the upstream
//! names and order; several upstream controls are inert data and are preserved
//! as-is. Every setter clamps and returns whether the value actually changed, so
//! a controller can drive change notification without this crate owning one.
//!
//! Three cascades are load-bearing and are ported exactly:
//!
//! * `min_cps`/`max_cps` and `min_turn_speed`/`max_turn_speed` keep their pair ordered.
//! * `liquid_bounce_body_point_min`/`max` do the same.
//!
//! # Reading a `killAuraSettings` object
//!
//! Two rules keep the object recoverable, the same two [`crate::profile`] applies to the
//! document around it:
//!
//! * **A bad field costs one field.** Every known key is decoded on its own, so a value of the
//!   wrong type falls back to that field's default and the other sixty-one still load. What the
//!   reader skipped is named on [`KillAuraSettings::problems`], because serde has one channel
//!   per value and a skipped field is not an error.
//! * **Keys this build does not know survive.** They are kept on
//!   [`KillAuraSettings::unknown_keys`] and written back verbatim, so an object a newer build
//!   wrote is not pruned by saving it here. The nine read-only flags the managed host wrote
//!   beside the data fields are the exception: they are derived from those fields, so they are
//!   dropped rather than carried stale (see `DERIVED_KEYS`).

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::profile::ProfileProblem;

/// Kill Aura configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct KillAuraSettings {
    cps_mode: String,
    mode: String,
    sort: String,
    auto_block: String,
    attack_tick: i32,
    auto_block_require_press: bool,
    auto_block_cps: f32,
    auto_block_range: f32,
    swing_range: f32,
    attack_range: f32,
    fov: i32,
    min_cps: i32,
    max_cps: i32,
    switch_delay: i32,
    rotations: String,
    dead_zone_size: f32,
    max_turn_speed: f32,
    min_turn_speed: f32,
    acceleration: f32,
    deceleration: f32,
    use_overshoot: bool,
    overshoot_strength: f32,
    overshoot_recovery: f32,
    noise_strength: f32,
    visualize_aim: bool,
    smooth_back: bool,
    move_fix: String,
    smoothing: i32,
    raven_smoothing: i32,
    raven_predict_ticks: i32,
    raven_yaw_random: i32,
    grok_max_skew: f32,
    angle_step: i32,
    through_walls: bool,
    require_press: bool,
    allow_mining: bool,
    weapons_only: bool,
    allow_tools: bool,
    inventory_check: bool,
    bot_check: bool,
    players: bool,
    bosses: bool,
    mobs: bool,
    animals: bool,
    golems: bool,
    silverfish: bool,
    teams: bool,
    show_target: String,
    debug_log: String,
    randomize: bool,
    randomize_range: f32,
    y_randomize_strength: f32,
    liquid_bounce_horizontal_speed: f32,
    liquid_bounce_vertical_speed: f32,
    liquid_bounce_smooth_factor: f32,
    liquid_bounce_predict: bool,
    liquid_bounce_predict_size: f32,
    liquid_bounce_randomize: bool,
    liquid_bounce_randomize_range: f32,
    liquid_bounce_horizontal_search: f32,
    liquid_bounce_body_point_min: f32,
    liquid_bounce_body_point_max: f32,

    /// Keys inside `killAuraSettings` that this build does not know, written back by the
    /// [`Serialize`](serde::Serialize) impl.
    ///
    /// A newer build, or a hand edit, can add a key to the object. It has no field here, so it
    /// travels from the file it was read from to the next save instead of being pruned — the
    /// rule [`crate::profile::Profile::unknown_keys`] follows one level up. The nine derived
    /// flags are not among them; see `DERIVED_KEYS`.
    pub unknown_keys: BTreeMap<String, Value>,

    /// Fields the object this value was read from had to skip, named by their key in it.
    ///
    /// A value of the wrong type costs its own field — which keeps its default — and nothing
    /// else, so the rest of the object still loads. The report is not serialised: the field it
    /// belonged to is at its default by the time this is set, and it is here so that a caller
    /// can say what the load cost instead of the skip passing unreported.
    problems: Vec<ProfileProblem>,
}

/// Allowed `CpsMode` values in upstream order.
pub const CPS_MODES: [&str; 2] = ["normal", "record"];
/// Allowed `Mode` values in upstream order.
pub const MODES: [&str; 2] = ["single", "switch"];
/// Allowed `Sort` values in upstream order.
pub const SORTS: [&str; 4] = ["distance", "health", "hurttime", "fov"];
/// Allowed `AutoBlock` values in upstream order.
pub const AUTO_BLOCKS: [&str; 10] = [
    "none", "vanilla", "spoof", "hypixel", "blink", "interact", "swap", "legit", "fake", "morden",
];
/// Allowed `Rotations` values in upstream order.
pub const ROTATIONS: [&str; 7] = [
    "none",
    "legit",
    "silent",
    "lockview",
    "liquidbounce",
    "hypixel",
    "grok",
];
/// Allowed `MoveFix` values in upstream order.
pub const MOVE_FIXES: [&str; 3] = ["none", "silent", "strict"];
/// Allowed `ShowTarget` values in upstream order.
pub const SHOW_TARGETS: [&str; 2] = ["none", "default"];
/// Allowed `DebugLog` values in upstream order.
pub const DEBUG_LOGS: [&str; 2] = ["none", "health"];

impl Default for KillAuraSettings {
    fn default() -> Self {
        Self {
            cps_mode: "normal".to_owned(),
            mode: "switch".to_owned(),
            sort: "health".to_owned(),
            auto_block: "hypixel".to_owned(),
            attack_tick: 0,
            auto_block_require_press: false,
            auto_block_cps: 8.0,
            auto_block_range: 6.0,
            swing_range: 3.5,
            attack_range: 3.0,
            fov: 360,
            min_cps: 14,
            max_cps: 14,
            switch_delay: 150,
            rotations: "silent".to_owned(),
            dead_zone_size: 0.5,
            max_turn_speed: 25.0,
            min_turn_speed: 5.0,
            acceleration: 2.5,
            deceleration: 1.5,
            use_overshoot: true,
            overshoot_strength: 5.0,
            overshoot_recovery: 0.2,
            noise_strength: 0.2,
            visualize_aim: true,
            smooth_back: true,
            move_fix: "silent".to_owned(),
            smoothing: 0,
            raven_smoothing: 0,
            raven_predict_ticks: 0,
            raven_yaw_random: 0,
            grok_max_skew: 12.0,
            angle_step: 90,
            through_walls: true,
            require_press: false,
            allow_mining: true,
            weapons_only: true,
            allow_tools: false,
            inventory_check: true,
            bot_check: true,
            players: true,
            bosses: false,
            mobs: false,
            animals: false,
            golems: false,
            silverfish: false,
            teams: true,
            show_target: "none".to_owned(),
            debug_log: "none".to_owned(),
            randomize: true,
            randomize_range: 0.4,
            y_randomize_strength: 0.3,
            liquid_bounce_horizontal_speed: 180.0,
            liquid_bounce_vertical_speed: 180.0,
            liquid_bounce_smooth_factor: 0.5,
            liquid_bounce_predict: true,
            liquid_bounce_predict_size: 1.0,
            liquid_bounce_randomize: true,
            liquid_bounce_randomize_range: 0.5,
            liquid_bounce_horizontal_search: 0.5,
            liquid_bounce_body_point_min: 0.1,
            liquid_bounce_body_point_max: 0.9,
            unknown_keys: BTreeMap::new(),
            problems: Vec::new(),
        }
    }
}

/// Normalises a mode string the way `SetMode` does: trim, lowercase, and fall
/// back to the first allowed value when it is not in the list.
fn normalize_mode<'a>(value: &str, allowed: &[&'a str]) -> &'a str {
    let probe = value.trim().to_lowercase();
    allowed
        .iter()
        .find(|candidate| **candidate == probe)
        .copied()
        .unwrap_or(allowed[0])
}

fn set_canonical_mode(target: &mut String, value: &str, allowed: &[&str]) -> bool {
    let normalized = normalize_mode(value, allowed);
    if target == normalized {
        return false;
    }
    target.clear();
    target.push_str(normalized);
    true
}

macro_rules! scalar_property {
    ($get:ident, $set:ident, $field:ident, $ty:ty) => {
        #[must_use]
        pub fn $get(&self) -> $ty {
            self.$field
        }

        pub fn $set(&mut self, value: $ty) -> bool {
            if self.$field == value {
                return false;
            }
            self.$field = value;
            true
        }
    };
}

macro_rules! clamped_property {
    ($get:ident, $set:ident, $field:ident, $ty:ty, $min:expr, $max:expr) => {
        #[must_use]
        pub fn $get(&self) -> $ty {
            self.$field
        }

        pub fn $set(&mut self, value: $ty) -> bool {
            let clamped = value.clamp($min, $max);
            if self.$field == clamped {
                return false;
            }
            self.$field = clamped;
            true
        }
    };
}

use serde::ser::SerializeMap as _;

/// Generates the getter/setter pair for one canonical mode string.
macro_rules! mode_accessors {
    ($get:ident, $set:ident, $field:ident, $allowed:expr) => {
        #[must_use]
        pub fn $get(&self) -> &str {
            &self.$field
        }

        /// Normalises the input; unknown values fall back to the first allowed one.
        pub fn $set(&mut self, value: &str) -> bool {
            set_canonical_mode(&mut self.$field, value, $allowed)
        }
    };
}

impl KillAuraSettings {
    mode_accessors!(cps_mode, set_cps_mode, cps_mode, &CPS_MODES);
    mode_accessors!(mode, set_mode, mode, &MODES);
    mode_accessors!(sort, set_sort, sort, &SORTS);
    mode_accessors!(auto_block, set_auto_block, auto_block, &AUTO_BLOCKS);
    mode_accessors!(rotations, set_rotations, rotations, &ROTATIONS);
    mode_accessors!(move_fix, set_move_fix, move_fix, &MOVE_FIXES);
    mode_accessors!(show_target, set_show_target, show_target, &SHOW_TARGETS);
    mode_accessors!(debug_log, set_debug_log, debug_log, &DEBUG_LOGS);

    clamped_property!(attack_tick, set_attack_tick, attack_tick, i32, 0, 5);
    scalar_property!(
        auto_block_require_press,
        set_auto_block_require_press,
        auto_block_require_press,
        bool
    );
    clamped_property!(
        auto_block_cps,
        set_auto_block_cps,
        auto_block_cps,
        f32,
        1.0,
        10.0
    );
    clamped_property!(
        auto_block_range,
        set_auto_block_range,
        auto_block_range,
        f32,
        3.0,
        8.0
    );
    clamped_property!(swing_range, set_swing_range, swing_range, f32, 3.0, 6.0);
    clamped_property!(attack_range, set_attack_range, attack_range, f32, 3.0, 6.0);
    clamped_property!(fov, set_fov, fov, i32, 30, 360);
    clamped_property!(switch_delay, set_switch_delay, switch_delay, i32, 0, 1000);
    clamped_property!(
        dead_zone_size,
        set_dead_zone_size,
        dead_zone_size,
        f32,
        0.0,
        2.0
    );
    clamped_property!(acceleration, set_acceleration, acceleration, f32, 0.1, 10.0);
    clamped_property!(deceleration, set_deceleration, deceleration, f32, 0.1, 10.0);
    scalar_property!(use_overshoot, set_use_overshoot, use_overshoot, bool);
    clamped_property!(
        overshoot_strength,
        set_overshoot_strength,
        overshoot_strength,
        f32,
        0.0,
        20.0
    );
    clamped_property!(
        overshoot_recovery,
        set_overshoot_recovery,
        overshoot_recovery,
        f32,
        0.01,
        1.0
    );
    clamped_property!(
        noise_strength,
        set_noise_strength,
        noise_strength,
        f32,
        0.0,
        2.0
    );
    scalar_property!(visualize_aim, set_visualize_aim, visualize_aim, bool);
    scalar_property!(smooth_back, set_smooth_back, smooth_back, bool);
    clamped_property!(smoothing, set_smoothing, smoothing, i32, 0, 100);
    clamped_property!(
        raven_smoothing,
        set_raven_smoothing,
        raven_smoothing,
        i32,
        0,
        10
    );
    clamped_property!(
        raven_predict_ticks,
        set_raven_predict_ticks,
        raven_predict_ticks,
        i32,
        0,
        5
    );
    clamped_property!(
        raven_yaw_random,
        set_raven_yaw_random,
        raven_yaw_random,
        i32,
        0,
        5
    );
    clamped_property!(
        grok_max_skew,
        set_grok_max_skew,
        grok_max_skew,
        f32,
        6.0,
        25.0
    );
    clamped_property!(angle_step, set_angle_step, angle_step, i32, 30, 180);
    scalar_property!(through_walls, set_through_walls, through_walls, bool);
    scalar_property!(require_press, set_require_press, require_press, bool);
    scalar_property!(allow_mining, set_allow_mining, allow_mining, bool);
    scalar_property!(weapons_only, set_weapons_only, weapons_only, bool);
    scalar_property!(allow_tools, set_allow_tools, allow_tools, bool);
    scalar_property!(inventory_check, set_inventory_check, inventory_check, bool);
    scalar_property!(bot_check, set_bot_check, bot_check, bool);
    scalar_property!(players, set_players, players, bool);
    scalar_property!(bosses, set_bosses, bosses, bool);
    scalar_property!(mobs, set_mobs, mobs, bool);
    scalar_property!(animals, set_animals, animals, bool);
    scalar_property!(golems, set_golems, golems, bool);
    scalar_property!(silverfish, set_silverfish, silverfish, bool);
    scalar_property!(teams, set_teams, teams, bool);
    scalar_property!(randomize, set_randomize, randomize, bool);
    clamped_property!(
        randomize_range,
        set_randomize_range,
        randomize_range,
        f32,
        0.0,
        1.0
    );
    clamped_property!(
        y_randomize_strength,
        set_y_randomize_strength,
        y_randomize_strength,
        f32,
        0.0,
        1.0
    );
    clamped_property!(
        liquid_bounce_horizontal_speed,
        set_liquid_bounce_horizontal_speed,
        liquid_bounce_horizontal_speed,
        f32,
        1.0,
        180.0
    );
    clamped_property!(
        liquid_bounce_vertical_speed,
        set_liquid_bounce_vertical_speed,
        liquid_bounce_vertical_speed,
        f32,
        1.0,
        180.0
    );
    clamped_property!(
        liquid_bounce_smooth_factor,
        set_liquid_bounce_smooth_factor,
        liquid_bounce_smooth_factor,
        f32,
        0.1,
        1.0
    );
    scalar_property!(
        liquid_bounce_predict,
        set_liquid_bounce_predict,
        liquid_bounce_predict,
        bool
    );
    clamped_property!(
        liquid_bounce_predict_size,
        set_liquid_bounce_predict_size,
        liquid_bounce_predict_size,
        f32,
        0.0,
        3.0
    );
    scalar_property!(
        liquid_bounce_randomize,
        set_liquid_bounce_randomize,
        liquid_bounce_randomize,
        bool
    );
    clamped_property!(
        liquid_bounce_randomize_range,
        set_liquid_bounce_randomize_range,
        liquid_bounce_randomize_range,
        f32,
        0.0,
        1.0
    );
    clamped_property!(
        liquid_bounce_horizontal_search,
        set_liquid_bounce_horizontal_search,
        liquid_bounce_horizontal_search,
        f32,
        0.0,
        1.0
    );

    /// Minimum clicks per second, in `[1, 20]`. Raises `MaxCps` when it would
    /// otherwise fall below the new minimum.
    #[must_use]
    pub fn min_cps(&self) -> i32 {
        self.min_cps
    }

    pub fn set_min_cps(&mut self, value: i32) -> bool {
        let clamped = value.clamp(1, 20);
        if self.min_cps == clamped {
            return false;
        }
        self.min_cps = clamped;
        if self.max_cps < clamped {
            self.max_cps = clamped;
        }
        true
    }

    /// Maximum clicks per second, in `[1, 20]`. Lowers `MinCps` when it would
    /// otherwise exceed the new maximum.
    #[must_use]
    pub fn max_cps(&self) -> i32 {
        self.max_cps
    }

    pub fn set_max_cps(&mut self, value: i32) -> bool {
        let clamped = value.clamp(1, 20);
        if self.max_cps == clamped {
            return false;
        }
        self.max_cps = clamped;
        if self.min_cps > clamped {
            self.min_cps = clamped;
        }
        true
    }

    /// Maximum turn speed, in `[5, 180]`. Lowers `MinTurnSpeed` when needed.
    #[must_use]
    pub fn max_turn_speed(&self) -> f32 {
        self.max_turn_speed
    }

    pub fn set_max_turn_speed(&mut self, value: f32) -> bool {
        let clamped = value.clamp(5.0, 180.0);
        if self.max_turn_speed == clamped {
            return false;
        }
        self.max_turn_speed = clamped;
        if self.min_turn_speed > clamped {
            self.min_turn_speed = clamped;
        }
        true
    }

    /// Minimum turn speed, in `[1, 90]`. Raises `MaxTurnSpeed` when needed.
    #[must_use]
    pub fn min_turn_speed(&self) -> f32 {
        self.min_turn_speed
    }

    pub fn set_min_turn_speed(&mut self, value: f32) -> bool {
        let clamped = value.clamp(1.0, 90.0);
        if self.min_turn_speed == clamped {
            return false;
        }
        self.min_turn_speed = clamped;
        if self.max_turn_speed < clamped {
            self.max_turn_speed = clamped;
        }
        true
    }

    /// Lower body-point anchor, in `[0, 1]`. Raises the maximum when needed.
    #[must_use]
    pub fn liquid_bounce_body_point_min(&self) -> f32 {
        self.liquid_bounce_body_point_min
    }

    pub fn set_liquid_bounce_body_point_min(&mut self, value: f32) -> bool {
        let clamped = value.clamp(0.0, 1.0);
        if self.liquid_bounce_body_point_min == clamped {
            return false;
        }
        self.liquid_bounce_body_point_min = clamped;
        if self.liquid_bounce_body_point_max < clamped {
            self.liquid_bounce_body_point_max = clamped;
        }
        true
    }

    /// Upper body-point anchor, in `[0, 1]`. Lowers the minimum when needed.
    #[must_use]
    pub fn liquid_bounce_body_point_max(&self) -> f32 {
        self.liquid_bounce_body_point_max
    }

    pub fn set_liquid_bounce_body_point_max(&mut self, value: f32) -> bool {
        let clamped = value.clamp(0.0, 1.0);
        if self.liquid_bounce_body_point_max == clamped {
            return false;
        }
        self.liquid_bounce_body_point_max = clamped;
        if self.liquid_bounce_body_point_min > clamped {
            self.liquid_bounce_body_point_min = clamped;
        }
        true
    }

    #[must_use]
    pub fn is_liquid_bounce(&self) -> bool {
        self.rotations == "liquidbounce"
    }

    #[must_use]
    pub fn is_hypixel_rotation(&self) -> bool {
        self.rotations == "hypixel"
    }

    #[must_use]
    pub fn is_grok_rotation(&self) -> bool {
        self.rotations == "grok"
    }

    #[must_use]
    pub fn is_swap_auto_block(&self) -> bool {
        self.auto_block == "swap"
    }

    #[must_use]
    pub fn overshoot_controls_enabled(&self) -> bool {
        self.is_liquid_bounce() && self.use_overshoot
    }

    #[must_use]
    pub fn randomize_controls_enabled(&self) -> bool {
        self.is_liquid_bounce() && self.randomize
    }

    #[must_use]
    pub fn predict_controls_enabled(&self) -> bool {
        self.is_liquid_bounce() && self.liquid_bounce_predict
    }

    #[must_use]
    pub fn liquid_randomize_controls_enabled(&self) -> bool {
        self.is_liquid_bounce() && self.liquid_bounce_randomize
    }

    #[must_use]
    pub fn allow_tools_enabled(&self) -> bool {
        self.weapons_only
    }

    /// Fields the object this value was read from had to skip, in the order the decoder met
    /// them.
    ///
    /// Empty for a value built here or read from an object this build wrote. A caller that
    /// reports a load names these beside the document's own problems
    /// ([`crate::profile::ProfileLoad::problems`]), so a malformed setting is a field at its
    /// default the user was told about rather than a silent one.
    #[must_use]
    pub fn problems(&self) -> &[ProfileProblem] {
        &self.problems
    }

    /// Copies every field from `source`, unknown keys and the report included.
    ///
    /// A whole-value copy rather than a second run through the setters: `source` already went
    /// through them when it was built or read, so its ordered pairs and clamped values are
    /// exactly what those setters would produce again.
    pub fn copy_from(&mut self, source: &Self) {
        *self = source.clone();
    }
}

/// Deserialisation shim: every field optional, so an object written by an older build still
/// loads. Each present value is routed through its setter, which is where the clamps and
/// cascades live.
///
/// Built key by key by [`KillAuraSettingsWire::from_fields`], which is what keeps one malformed
/// value to one field instead of costing the whole object.
struct KillAuraSettingsWire {
    cps_mode: Option<String>,
    mode: Option<String>,
    sort: Option<String>,
    auto_block: Option<String>,
    attack_tick: Option<i32>,
    auto_block_require_press: Option<bool>,
    auto_block_cps: Option<f32>,
    auto_block_range: Option<f32>,
    swing_range: Option<f32>,
    attack_range: Option<f32>,
    fov: Option<i32>,
    min_cps: Option<i32>,
    max_cps: Option<i32>,
    switch_delay: Option<i32>,
    rotations: Option<String>,
    dead_zone_size: Option<f32>,
    max_turn_speed: Option<f32>,
    min_turn_speed: Option<f32>,
    acceleration: Option<f32>,
    deceleration: Option<f32>,
    use_overshoot: Option<bool>,
    overshoot_strength: Option<f32>,
    overshoot_recovery: Option<f32>,
    noise_strength: Option<f32>,
    visualize_aim: Option<bool>,
    smooth_back: Option<bool>,
    move_fix: Option<String>,
    smoothing: Option<i32>,
    raven_smoothing: Option<i32>,
    raven_predict_ticks: Option<i32>,
    raven_yaw_random: Option<i32>,
    grok_max_skew: Option<f32>,
    angle_step: Option<i32>,
    through_walls: Option<bool>,
    require_press: Option<bool>,
    allow_mining: Option<bool>,
    weapons_only: Option<bool>,
    allow_tools: Option<bool>,
    inventory_check: Option<bool>,
    bot_check: Option<bool>,
    players: Option<bool>,
    bosses: Option<bool>,
    mobs: Option<bool>,
    animals: Option<bool>,
    golems: Option<bool>,
    silverfish: Option<bool>,
    teams: Option<bool>,
    show_target: Option<String>,
    debug_log: Option<String>,
    randomize: Option<bool>,
    randomize_range: Option<f32>,
    y_randomize_strength: Option<f32>,
    liquid_bounce_horizontal_speed: Option<f32>,
    liquid_bounce_vertical_speed: Option<f32>,
    liquid_bounce_smooth_factor: Option<f32>,
    liquid_bounce_predict: Option<bool>,
    liquid_bounce_predict_size: Option<f32>,
    liquid_bounce_randomize: Option<bool>,
    liquid_bounce_randomize_range: Option<f32>,
    liquid_bounce_horizontal_search: Option<f32>,
    liquid_bounce_body_point_min: Option<f32>,
    liquid_bounce_body_point_max: Option<f32>,
}

impl KillAuraSettingsWire {
    /// Reads every key this schema knows, taking each one out of `fields`.
    ///
    /// What no `take` claims is left behind as the object's unknown keys, and a value that does
    /// not fit its field is reported and skipped, so the rest of the object still loads.
    fn from_fields(fields: &mut KillAuraFields) -> Self {
        Self {
            cps_mode: fields.take("cpsMode"),
            mode: fields.take("mode"),
            sort: fields.take("sort"),
            auto_block: fields.take("autoBlock"),
            attack_tick: fields.take("attackTick"),
            auto_block_require_press: fields.take("autoBlockRequirePress"),
            auto_block_cps: fields.take("autoBlockCps"),
            auto_block_range: fields.take("autoBlockRange"),
            swing_range: fields.take("swingRange"),
            attack_range: fields.take("attackRange"),
            fov: fields.take("fov"),
            min_cps: fields.take("minCps"),
            max_cps: fields.take("maxCps"),
            switch_delay: fields.take("switchDelay"),
            rotations: fields.take("rotations"),
            dead_zone_size: fields.take("deadZoneSize"),
            max_turn_speed: fields.take("maxTurnSpeed"),
            min_turn_speed: fields.take("minTurnSpeed"),
            acceleration: fields.take("acceleration"),
            deceleration: fields.take("deceleration"),
            use_overshoot: fields.take("useOvershoot"),
            overshoot_strength: fields.take("overshootStrength"),
            overshoot_recovery: fields.take("overshootRecovery"),
            noise_strength: fields.take("noiseStrength"),
            visualize_aim: fields.take("visualizeAim"),
            smooth_back: fields.take("smoothBack"),
            move_fix: fields.take("moveFix"),
            smoothing: fields.take("smoothing"),
            raven_smoothing: fields.take("ravenSmoothing"),
            raven_predict_ticks: fields.take("ravenPredictTicks"),
            raven_yaw_random: fields.take("ravenYawRandom"),
            grok_max_skew: fields.take("grokMaxSkew"),
            angle_step: fields.take("angleStep"),
            through_walls: fields.take("throughWalls"),
            require_press: fields.take("requirePress"),
            allow_mining: fields.take("allowMining"),
            weapons_only: fields.take("weaponsOnly"),
            allow_tools: fields.take("allowTools"),
            inventory_check: fields.take("inventoryCheck"),
            bot_check: fields.take("botCheck"),
            players: fields.take("players"),
            bosses: fields.take("bosses"),
            mobs: fields.take("mobs"),
            animals: fields.take("animals"),
            golems: fields.take("golems"),
            silverfish: fields.take("silverfish"),
            teams: fields.take("teams"),
            show_target: fields.take("showTarget"),
            debug_log: fields.take("debugLog"),
            randomize: fields.take("randomize"),
            randomize_range: fields.take("randomizeRange"),
            y_randomize_strength: fields.take("yRandomizeStrength"),
            liquid_bounce_horizontal_speed: fields.take("liquidBounceHorizontalSpeed"),
            liquid_bounce_vertical_speed: fields.take("liquidBounceVerticalSpeed"),
            liquid_bounce_smooth_factor: fields.take("liquidBounceSmoothFactor"),
            liquid_bounce_predict: fields.take("liquidBouncePredict"),
            liquid_bounce_predict_size: fields.take("liquidBouncePredictSize"),
            liquid_bounce_randomize: fields.take("liquidBounceRandomize"),
            liquid_bounce_randomize_range: fields.take("liquidBounceRandomizeRange"),
            liquid_bounce_horizontal_search: fields.take("liquidBounceHorizontalSearch"),
            liquid_bounce_body_point_min: fields.take("liquidBounceBodyPointMin"),
            liquid_bounce_body_point_max: fields.take("liquidBounceBodyPointMax"),
        }
    }
}

/// The read-only properties the managed host wrote beside the data fields
/// (`Aoko/Core/KillAuraSettings.cs:143-151`).
///
/// Each one is a function of those fields: `IsLiquidBounce` of `Rotations`, `AllowToolsEnabled`
/// of `WeaponsOnly`. The managed getter recomputes it on every read and this crate's accessors
/// do the same, so a value read out of a file can only disagree with the fields beside it. They
/// are dropped instead of being kept as unknown keys, because writing one back would state an
/// answer for fields the user has changed since — the rule
/// [`crate::profile::SCHEMA_VERSION_KEY`] follows one level up.
const DERIVED_KEYS: [&str; 9] = [
    "isLiquidBounce",
    "isHypixelRotation",
    "isGrokRotation",
    "isSwapAutoBlock",
    "overshootControlsEnabled",
    "randomizeControlsEnabled",
    "predictControlsEnabled",
    "liquidRandomizeControlsEnabled",
    "allowToolsEnabled",
];

/// The parts of a `killAuraSettings` object that were not decoded, and the fields that did not
/// fit.
///
/// The same shape as `profile::ProfileFields`, one level down: every known key is taken out of
/// `remaining` as it is read, so what is left when the settings are built is exactly the part of
/// the object this schema does not know, and a malformed value is reported and skipped rather
/// than costing the object it sits in.
struct KillAuraFields {
    remaining: serde_json::Map<String, Value>,
    problems: Vec<ProfileProblem>,
}

impl KillAuraFields {
    fn new(root: &serde_json::Map<String, Value>) -> Self {
        Self {
            remaining: root.clone(),
            problems: Vec::new(),
        }
    }

    /// The value of `key`, or `None` when it is absent or does not fit `T`.
    ///
    /// `null` reads as absent rather than as a value of the wrong type, which is what the
    /// managed serialiser wrote for an unset property.
    fn take<T: DeserializeOwned>(&mut self, key: &str) -> Option<T> {
        let value = self.remaining.remove(key)?;
        if value.is_null() {
            return None;
        }
        match serde_json::from_value(value) {
            Ok(decoded) => Some(decoded),
            Err(error) => {
                self.problems.push(ProfileProblem {
                    key: key.to_owned(),
                    message: error.to_string(),
                });
                None
            }
        }
    }

    /// The keys no `take` claimed, which are the ones this build does not know.
    ///
    /// [`DERIVED_KEYS`] is removed first rather than kept: those flags are recomputed from the
    /// data fields, so the copy in a file can only be a stale answer.
    fn unknown(&mut self) -> BTreeMap<String, Value> {
        for key in DERIVED_KEYS {
            self.remaining.remove(key);
        }
        std::mem::take(&mut self.remaining).into_iter().collect()
    }
}

impl From<KillAuraSettingsWire> for KillAuraSettings {
    fn from(wire: KillAuraSettingsWire) -> Self {
        let mut target = KillAuraSettings::default();
        if let Some(value) = wire.cps_mode {
            target.set_cps_mode(&value);
        }
        if let Some(value) = wire.mode {
            target.set_mode(&value);
        }
        if let Some(value) = wire.sort {
            target.set_sort(&value);
        }
        if let Some(value) = wire.auto_block {
            target.set_auto_block(&value);
        }
        if let Some(value) = wire.attack_tick {
            target.set_attack_tick(value);
        }
        if let Some(value) = wire.auto_block_require_press {
            target.set_auto_block_require_press(value);
        }
        if let Some(value) = wire.auto_block_cps {
            target.set_auto_block_cps(value);
        }
        if let Some(value) = wire.auto_block_range {
            target.set_auto_block_range(value);
        }
        if let Some(value) = wire.swing_range {
            target.set_swing_range(value);
        }
        if let Some(value) = wire.attack_range {
            target.set_attack_range(value);
        }
        if let Some(value) = wire.fov {
            target.set_fov(value);
        }
        if let Some(value) = wire.min_cps {
            target.set_min_cps(value);
        }
        if let Some(value) = wire.max_cps {
            target.set_max_cps(value);
        }
        if let Some(value) = wire.switch_delay {
            target.set_switch_delay(value);
        }
        if let Some(value) = wire.rotations {
            target.set_rotations(&value);
        }
        if let Some(value) = wire.dead_zone_size {
            target.set_dead_zone_size(value);
        }
        if let Some(value) = wire.max_turn_speed {
            target.set_max_turn_speed(value);
        }
        if let Some(value) = wire.min_turn_speed {
            target.set_min_turn_speed(value);
        }
        if let Some(value) = wire.acceleration {
            target.set_acceleration(value);
        }
        if let Some(value) = wire.deceleration {
            target.set_deceleration(value);
        }
        if let Some(value) = wire.use_overshoot {
            target.set_use_overshoot(value);
        }
        if let Some(value) = wire.overshoot_strength {
            target.set_overshoot_strength(value);
        }
        if let Some(value) = wire.overshoot_recovery {
            target.set_overshoot_recovery(value);
        }
        if let Some(value) = wire.noise_strength {
            target.set_noise_strength(value);
        }
        if let Some(value) = wire.visualize_aim {
            target.set_visualize_aim(value);
        }
        if let Some(value) = wire.smooth_back {
            target.set_smooth_back(value);
        }
        if let Some(value) = wire.move_fix {
            target.set_move_fix(&value);
        }
        if let Some(value) = wire.smoothing {
            target.set_smoothing(value);
        }
        if let Some(value) = wire.raven_smoothing {
            target.set_raven_smoothing(value);
        }
        if let Some(value) = wire.raven_predict_ticks {
            target.set_raven_predict_ticks(value);
        }
        if let Some(value) = wire.raven_yaw_random {
            target.set_raven_yaw_random(value);
        }
        if let Some(value) = wire.grok_max_skew {
            target.set_grok_max_skew(value);
        }
        if let Some(value) = wire.angle_step {
            target.set_angle_step(value);
        }
        if let Some(value) = wire.through_walls {
            target.set_through_walls(value);
        }
        if let Some(value) = wire.require_press {
            target.set_require_press(value);
        }
        if let Some(value) = wire.allow_mining {
            target.set_allow_mining(value);
        }
        if let Some(value) = wire.weapons_only {
            target.set_weapons_only(value);
        }
        if let Some(value) = wire.allow_tools {
            target.set_allow_tools(value);
        }
        if let Some(value) = wire.inventory_check {
            target.set_inventory_check(value);
        }
        if let Some(value) = wire.bot_check {
            target.set_bot_check(value);
        }
        if let Some(value) = wire.players {
            target.set_players(value);
        }
        if let Some(value) = wire.bosses {
            target.set_bosses(value);
        }
        if let Some(value) = wire.mobs {
            target.set_mobs(value);
        }
        if let Some(value) = wire.animals {
            target.set_animals(value);
        }
        if let Some(value) = wire.golems {
            target.set_golems(value);
        }
        if let Some(value) = wire.silverfish {
            target.set_silverfish(value);
        }
        if let Some(value) = wire.teams {
            target.set_teams(value);
        }
        if let Some(value) = wire.show_target {
            target.set_show_target(&value);
        }
        if let Some(value) = wire.debug_log {
            target.set_debug_log(&value);
        }
        if let Some(value) = wire.randomize {
            target.set_randomize(value);
        }
        if let Some(value) = wire.randomize_range {
            target.set_randomize_range(value);
        }
        if let Some(value) = wire.y_randomize_strength {
            target.set_y_randomize_strength(value);
        }
        if let Some(value) = wire.liquid_bounce_horizontal_speed {
            target.set_liquid_bounce_horizontal_speed(value);
        }
        if let Some(value) = wire.liquid_bounce_vertical_speed {
            target.set_liquid_bounce_vertical_speed(value);
        }
        if let Some(value) = wire.liquid_bounce_smooth_factor {
            target.set_liquid_bounce_smooth_factor(value);
        }
        if let Some(value) = wire.liquid_bounce_predict {
            target.set_liquid_bounce_predict(value);
        }
        if let Some(value) = wire.liquid_bounce_predict_size {
            target.set_liquid_bounce_predict_size(value);
        }
        if let Some(value) = wire.liquid_bounce_randomize {
            target.set_liquid_bounce_randomize(value);
        }
        if let Some(value) = wire.liquid_bounce_randomize_range {
            target.set_liquid_bounce_randomize_range(value);
        }
        if let Some(value) = wire.liquid_bounce_horizontal_search {
            target.set_liquid_bounce_horizontal_search(value);
        }
        if let Some(value) = wire.liquid_bounce_body_point_min {
            target.set_liquid_bounce_body_point_min(value);
        }
        if let Some(value) = wire.liquid_bounce_body_point_max {
            target.set_liquid_bounce_body_point_max(value);
        }
        target
    }
}

impl<'de> Deserialize<'de> for KillAuraSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let root = Value::deserialize(deserializer)?;
        let Some(object) = root.as_object() else {
            return Err(serde::de::Error::custom(
                "a killAuraSettings value is an object",
            ));
        };

        let mut fields = KillAuraFields::new(object);
        let wire = KillAuraSettingsWire::from_fields(&mut fields);
        let mut settings = KillAuraSettings::from(wire);
        settings.unknown_keys = fields.unknown();
        settings.problems = std::mem::take(&mut fields.problems);
        Ok(settings)
    }
}

impl serde::Serialize for KillAuraSettings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // The data fields, then the keys this build does not know. The managed host also wrote
        // nine read-only derived flags (`isLiquidBounce`, `allowToolsEnabled`, ...); they are
        // recomputed from these fields, so they are dropped on the way in (`DERIVED_KEYS`) and
        // never written here — a copy would state an answer for fields the user has changed
        // since. A map rather than a struct, because the unknown keys bring their own names.
        let mut map = serializer.serialize_map(Some(62 + self.unknown_keys.len()))?;
        map.serialize_entry("cpsMode", self.cps_mode())?;
        map.serialize_entry("mode", self.mode())?;
        map.serialize_entry("sort", self.sort())?;
        map.serialize_entry("autoBlock", self.auto_block())?;
        map.serialize_entry("attackTick", &self.attack_tick())?;
        map.serialize_entry("autoBlockRequirePress", &self.auto_block_require_press())?;
        map.serialize_entry("autoBlockCps", &self.auto_block_cps())?;
        map.serialize_entry("autoBlockRange", &self.auto_block_range())?;
        map.serialize_entry("swingRange", &self.swing_range())?;
        map.serialize_entry("attackRange", &self.attack_range())?;
        map.serialize_entry("fov", &self.fov())?;
        map.serialize_entry("minCps", &self.min_cps())?;
        map.serialize_entry("maxCps", &self.max_cps())?;
        map.serialize_entry("switchDelay", &self.switch_delay())?;
        map.serialize_entry("rotations", self.rotations())?;
        map.serialize_entry("deadZoneSize", &self.dead_zone_size())?;
        map.serialize_entry("maxTurnSpeed", &self.max_turn_speed())?;
        map.serialize_entry("minTurnSpeed", &self.min_turn_speed())?;
        map.serialize_entry("acceleration", &self.acceleration())?;
        map.serialize_entry("deceleration", &self.deceleration())?;
        map.serialize_entry("useOvershoot", &self.use_overshoot())?;
        map.serialize_entry("overshootStrength", &self.overshoot_strength())?;
        map.serialize_entry("overshootRecovery", &self.overshoot_recovery())?;
        map.serialize_entry("noiseStrength", &self.noise_strength())?;
        map.serialize_entry("visualizeAim", &self.visualize_aim())?;
        map.serialize_entry("smoothBack", &self.smooth_back())?;
        map.serialize_entry("moveFix", self.move_fix())?;
        map.serialize_entry("smoothing", &self.smoothing())?;
        map.serialize_entry("ravenSmoothing", &self.raven_smoothing())?;
        map.serialize_entry("ravenPredictTicks", &self.raven_predict_ticks())?;
        map.serialize_entry("ravenYawRandom", &self.raven_yaw_random())?;
        map.serialize_entry("grokMaxSkew", &self.grok_max_skew())?;
        map.serialize_entry("angleStep", &self.angle_step())?;
        map.serialize_entry("throughWalls", &self.through_walls())?;
        map.serialize_entry("requirePress", &self.require_press())?;
        map.serialize_entry("allowMining", &self.allow_mining())?;
        map.serialize_entry("weaponsOnly", &self.weapons_only())?;
        map.serialize_entry("allowTools", &self.allow_tools())?;
        map.serialize_entry("inventoryCheck", &self.inventory_check())?;
        map.serialize_entry("botCheck", &self.bot_check())?;
        map.serialize_entry("players", &self.players())?;
        map.serialize_entry("bosses", &self.bosses())?;
        map.serialize_entry("mobs", &self.mobs())?;
        map.serialize_entry("animals", &self.animals())?;
        map.serialize_entry("golems", &self.golems())?;
        map.serialize_entry("silverfish", &self.silverfish())?;
        map.serialize_entry("teams", &self.teams())?;
        map.serialize_entry("showTarget", self.show_target())?;
        map.serialize_entry("debugLog", self.debug_log())?;
        map.serialize_entry("randomize", &self.randomize())?;
        map.serialize_entry("randomizeRange", &self.randomize_range())?;
        map.serialize_entry("yRandomizeStrength", &self.y_randomize_strength())?;
        map.serialize_entry(
            "liquidBounceHorizontalSpeed",
            &self.liquid_bounce_horizontal_speed(),
        )?;
        map.serialize_entry(
            "liquidBounceVerticalSpeed",
            &self.liquid_bounce_vertical_speed(),
        )?;
        map.serialize_entry(
            "liquidBounceSmoothFactor",
            &self.liquid_bounce_smooth_factor(),
        )?;
        map.serialize_entry("liquidBouncePredict", &self.liquid_bounce_predict())?;
        map.serialize_entry(
            "liquidBouncePredictSize",
            &self.liquid_bounce_predict_size(),
        )?;
        map.serialize_entry("liquidBounceRandomize", &self.liquid_bounce_randomize())?;
        map.serialize_entry(
            "liquidBounceRandomizeRange",
            &self.liquid_bounce_randomize_range(),
        )?;
        map.serialize_entry(
            "liquidBounceHorizontalSearch",
            &self.liquid_bounce_horizontal_search(),
        )?;
        map.serialize_entry(
            "liquidBounceBodyPointMin",
            &self.liquid_bounce_body_point_min(),
        )?;
        map.serialize_entry(
            "liquidBounceBodyPointMax",
            &self.liquid_bounce_body_point_max(),
        )?;
        for (key, value) in &self.unknown_keys {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One value of the wrong type costs its own field, and the reader names it instead of
    /// losing the other sixty-one.
    #[test]
    fn one_wrong_typed_field_costs_one_field() {
        let text = r#"{"minCps":"9","maxCps":18}"#;
        let settings: KillAuraSettings = serde_json::from_str(text).expect("deserialise");

        assert_eq!(settings.max_cps(), 18, "the field that fits still loads");
        assert_eq!(
            settings.min_cps(),
            KillAuraSettings::default().min_cps(),
            "the field that does not keeps its default"
        );
        assert_eq!(settings.problems().len(), 1, "{:?}", settings.problems());
        assert_eq!(settings.problems()[0].key, "minCps");
    }

    /// A key this build does not know inside the object survives a load and the save after it.
    #[test]
    fn an_unknown_key_survives_the_round_trip() {
        let text = r#"{"maxCps":18,"futureToggle":true}"#;
        let read: KillAuraSettings = serde_json::from_str(text).expect("deserialise");
        assert!(read.problems().is_empty(), "{:?}", read.problems());
        assert_eq!(
            read.unknown_keys.get("futureToggle"),
            Some(&Value::Bool(true))
        );

        let saved = serde_json::to_value(&read).expect("serialise");
        assert_eq!(saved["futureToggle"], Value::Bool(true));
        assert_eq!(saved["maxCps"], json!(18));

        let again: KillAuraSettings = serde_json::from_value(saved).expect("deserialise");
        assert_eq!(again.unknown_keys, read.unknown_keys);
    }

    /// The nine read-only flags the managed host wrote are recomputed from the data fields, so
    /// a stale copy is dropped instead of being carried into the next save.
    #[test]
    fn the_derived_flags_are_dropped_rather_than_kept() {
        let text = r#"{"rotations":"none","isLiquidBounce":true}"#;
        let read: KillAuraSettings = serde_json::from_str(text).expect("deserialise");

        assert!(!read.is_liquid_bounce(), "the data field decides");
        for key in DERIVED_KEYS {
            assert!(!read.unknown_keys.contains_key(key), "{key}");
        }
    }
}
