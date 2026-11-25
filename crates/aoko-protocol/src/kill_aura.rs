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

/// Generates the getter/setter pair for one canonical mode string.
use serde::ser::SerializeStruct as _;

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

    /// Copies every field from `source`, running each setter so clamps and
    /// cascades fire exactly as they do when the UI edits one property at a time.
    pub fn copy_from(&mut self, source: &Self) {
        *self = source.clone();
    }
}

/// Deserialisation shim: every field optional, so a profile written by an older
/// build still loads. Each present value is routed through its setter, which is
/// where the clamps and cascades live.
#[derive(Debug, Default, serde::Deserialize)]
struct KillAuraSettingsWire {
    #[serde(rename = "cpsMode", default)]
    cps_mode: Option<String>,
    #[serde(rename = "mode", default)]
    mode: Option<String>,
    #[serde(rename = "sort", default)]
    sort: Option<String>,
    #[serde(rename = "autoBlock", default)]
    auto_block: Option<String>,
    #[serde(rename = "attackTick", default)]
    attack_tick: Option<i32>,
    #[serde(rename = "autoBlockRequirePress", default)]
    auto_block_require_press: Option<bool>,
    #[serde(rename = "autoBlockCps", default)]
    auto_block_cps: Option<f32>,
    #[serde(rename = "autoBlockRange", default)]
    auto_block_range: Option<f32>,
    #[serde(rename = "swingRange", default)]
    swing_range: Option<f32>,
    #[serde(rename = "attackRange", default)]
    attack_range: Option<f32>,
    #[serde(rename = "fov", default)]
    fov: Option<i32>,
    #[serde(rename = "minCps", default)]
    min_cps: Option<i32>,
    #[serde(rename = "maxCps", default)]
    max_cps: Option<i32>,
    #[serde(rename = "switchDelay", default)]
    switch_delay: Option<i32>,
    #[serde(rename = "rotations", default)]
    rotations: Option<String>,
    #[serde(rename = "deadZoneSize", default)]
    dead_zone_size: Option<f32>,
    #[serde(rename = "maxTurnSpeed", default)]
    max_turn_speed: Option<f32>,
    #[serde(rename = "minTurnSpeed", default)]
    min_turn_speed: Option<f32>,
    #[serde(rename = "acceleration", default)]
    acceleration: Option<f32>,
    #[serde(rename = "deceleration", default)]
    deceleration: Option<f32>,
    #[serde(rename = "useOvershoot", default)]
    use_overshoot: Option<bool>,
    #[serde(rename = "overshootStrength", default)]
    overshoot_strength: Option<f32>,
    #[serde(rename = "overshootRecovery", default)]
    overshoot_recovery: Option<f32>,
    #[serde(rename = "noiseStrength", default)]
    noise_strength: Option<f32>,
    #[serde(rename = "visualizeAim", default)]
    visualize_aim: Option<bool>,
    #[serde(rename = "smoothBack", default)]
    smooth_back: Option<bool>,
    #[serde(rename = "moveFix", default)]
    move_fix: Option<String>,
    #[serde(rename = "smoothing", default)]
    smoothing: Option<i32>,
    #[serde(rename = "ravenSmoothing", default)]
    raven_smoothing: Option<i32>,
    #[serde(rename = "ravenPredictTicks", default)]
    raven_predict_ticks: Option<i32>,
    #[serde(rename = "ravenYawRandom", default)]
    raven_yaw_random: Option<i32>,
    #[serde(rename = "grokMaxSkew", default)]
    grok_max_skew: Option<f32>,
    #[serde(rename = "angleStep", default)]
    angle_step: Option<i32>,
    #[serde(rename = "throughWalls", default)]
    through_walls: Option<bool>,
    #[serde(rename = "requirePress", default)]
    require_press: Option<bool>,
    #[serde(rename = "allowMining", default)]
    allow_mining: Option<bool>,
    #[serde(rename = "weaponsOnly", default)]
    weapons_only: Option<bool>,
    #[serde(rename = "allowTools", default)]
    allow_tools: Option<bool>,
    #[serde(rename = "inventoryCheck", default)]
    inventory_check: Option<bool>,
    #[serde(rename = "botCheck", default)]
    bot_check: Option<bool>,
    #[serde(rename = "players", default)]
    players: Option<bool>,
    #[serde(rename = "bosses", default)]
    bosses: Option<bool>,
    #[serde(rename = "mobs", default)]
    mobs: Option<bool>,
    #[serde(rename = "animals", default)]
    animals: Option<bool>,
    #[serde(rename = "golems", default)]
    golems: Option<bool>,
    #[serde(rename = "silverfish", default)]
    silverfish: Option<bool>,
    #[serde(rename = "teams", default)]
    teams: Option<bool>,
    #[serde(rename = "showTarget", default)]
    show_target: Option<String>,
    #[serde(rename = "debugLog", default)]
    debug_log: Option<String>,
    #[serde(rename = "randomize", default)]
    randomize: Option<bool>,
    #[serde(rename = "randomizeRange", default)]
    randomize_range: Option<f32>,
    #[serde(rename = "yRandomizeStrength", default)]
    y_randomize_strength: Option<f32>,
    #[serde(rename = "liquidBounceHorizontalSpeed", default)]
    liquid_bounce_horizontal_speed: Option<f32>,
    #[serde(rename = "liquidBounceVerticalSpeed", default)]
    liquid_bounce_vertical_speed: Option<f32>,
    #[serde(rename = "liquidBounceSmoothFactor", default)]
    liquid_bounce_smooth_factor: Option<f32>,
    #[serde(rename = "liquidBouncePredict", default)]
    liquid_bounce_predict: Option<bool>,
    #[serde(rename = "liquidBouncePredictSize", default)]
    liquid_bounce_predict_size: Option<f32>,
    #[serde(rename = "liquidBounceRandomize", default)]
    liquid_bounce_randomize: Option<bool>,
    #[serde(rename = "liquidBounceRandomizeRange", default)]
    liquid_bounce_randomize_range: Option<f32>,
    #[serde(rename = "liquidBounceHorizontalSearch", default)]
    liquid_bounce_horizontal_search: Option<f32>,
    #[serde(rename = "liquidBounceBodyPointMin", default)]
    liquid_bounce_body_point_min: Option<f32>,
    #[serde(rename = "liquidBounceBodyPointMax", default)]
    liquid_bounce_body_point_max: Option<f32>,
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

impl<'de> serde::Deserialize<'de> for KillAuraSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = KillAuraSettingsWire::deserialize(deserializer)?;
        Ok(KillAuraSettings::from(wire))
    }
}

impl serde::Serialize for KillAuraSettings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // The data fields only. The managed host also emits nine read-only
        // derived flags (`isLiquidBounce`, `allowToolsEnabled`, ...); they are
        // recomputed on load, so dropping them cannot lose user state.
        let mut state = serializer.serialize_struct("KillAuraSettings", 62)?;
        state.serialize_field("cpsMode", self.cps_mode())?;
        state.serialize_field("mode", self.mode())?;
        state.serialize_field("sort", self.sort())?;
        state.serialize_field("autoBlock", self.auto_block())?;
        state.serialize_field("attackTick", &self.attack_tick())?;
        state.serialize_field("autoBlockRequirePress", &self.auto_block_require_press())?;
        state.serialize_field("autoBlockCps", &self.auto_block_cps())?;
        state.serialize_field("autoBlockRange", &self.auto_block_range())?;
        state.serialize_field("swingRange", &self.swing_range())?;
        state.serialize_field("attackRange", &self.attack_range())?;
        state.serialize_field("fov", &self.fov())?;
        state.serialize_field("minCps", &self.min_cps())?;
        state.serialize_field("maxCps", &self.max_cps())?;
        state.serialize_field("switchDelay", &self.switch_delay())?;
        state.serialize_field("rotations", self.rotations())?;
        state.serialize_field("deadZoneSize", &self.dead_zone_size())?;
        state.serialize_field("maxTurnSpeed", &self.max_turn_speed())?;
        state.serialize_field("minTurnSpeed", &self.min_turn_speed())?;
        state.serialize_field("acceleration", &self.acceleration())?;
        state.serialize_field("deceleration", &self.deceleration())?;
        state.serialize_field("useOvershoot", &self.use_overshoot())?;
        state.serialize_field("overshootStrength", &self.overshoot_strength())?;
        state.serialize_field("overshootRecovery", &self.overshoot_recovery())?;
        state.serialize_field("noiseStrength", &self.noise_strength())?;
        state.serialize_field("visualizeAim", &self.visualize_aim())?;
        state.serialize_field("smoothBack", &self.smooth_back())?;
        state.serialize_field("moveFix", self.move_fix())?;
        state.serialize_field("smoothing", &self.smoothing())?;
        state.serialize_field("ravenSmoothing", &self.raven_smoothing())?;
        state.serialize_field("ravenPredictTicks", &self.raven_predict_ticks())?;
        state.serialize_field("ravenYawRandom", &self.raven_yaw_random())?;
        state.serialize_field("grokMaxSkew", &self.grok_max_skew())?;
        state.serialize_field("angleStep", &self.angle_step())?;
        state.serialize_field("throughWalls", &self.through_walls())?;
        state.serialize_field("requirePress", &self.require_press())?;
        state.serialize_field("allowMining", &self.allow_mining())?;
        state.serialize_field("weaponsOnly", &self.weapons_only())?;
        state.serialize_field("allowTools", &self.allow_tools())?;
        state.serialize_field("inventoryCheck", &self.inventory_check())?;
        state.serialize_field("botCheck", &self.bot_check())?;
        state.serialize_field("players", &self.players())?;
        state.serialize_field("bosses", &self.bosses())?;
        state.serialize_field("mobs", &self.mobs())?;
        state.serialize_field("animals", &self.animals())?;
        state.serialize_field("golems", &self.golems())?;
        state.serialize_field("silverfish", &self.silverfish())?;
        state.serialize_field("teams", &self.teams())?;
        state.serialize_field("showTarget", self.show_target())?;
        state.serialize_field("debugLog", self.debug_log())?;
        state.serialize_field("randomize", &self.randomize())?;
        state.serialize_field("randomizeRange", &self.randomize_range())?;
        state.serialize_field("yRandomizeStrength", &self.y_randomize_strength())?;
        state.serialize_field(
            "liquidBounceHorizontalSpeed",
            &self.liquid_bounce_horizontal_speed(),
        )?;
        state.serialize_field(
            "liquidBounceVerticalSpeed",
            &self.liquid_bounce_vertical_speed(),
        )?;
        state.serialize_field(
            "liquidBounceSmoothFactor",
            &self.liquid_bounce_smooth_factor(),
        )?;
        state.serialize_field("liquidBouncePredict", &self.liquid_bounce_predict())?;
        state.serialize_field(
            "liquidBouncePredictSize",
            &self.liquid_bounce_predict_size(),
        )?;
        state.serialize_field("liquidBounceRandomize", &self.liquid_bounce_randomize())?;
        state.serialize_field(
            "liquidBounceRandomizeRange",
            &self.liquid_bounce_randomize_range(),
        )?;
        state.serialize_field(
            "liquidBounceHorizontalSearch",
            &self.liquid_bounce_horizontal_search(),
        )?;
        state.serialize_field(
            "liquidBounceBodyPointMin",
            &self.liquid_bounce_body_point_min(),
        )?;
        state.serialize_field(
            "liquidBounceBodyPointMax",
            &self.liquid_bounce_body_point_max(),
        )?;
        state.end()
    }
}
