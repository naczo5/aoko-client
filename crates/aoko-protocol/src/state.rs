//! Inbound game state, as produced by both native bridges.
//!
//! Ported from `Aoko/Core/GameState.cs`. The JSON key spellings are the contract;
//! every field carries an explicit `rename` because the managed host serialises
//! with `JsonNamingPolicy.CamelCase`, which is *not* the same transformation as
//! `serde`'s `rename_all = "camelCase"` for acronyms (`MinCPS` -> `minCPS`, not
//! `minCps`).
//!
//! Decoding is deliberately permissive (architecture decision 2): absent fields
//! take the C# default, unknown fields are ignored, and `null` for a collection
//! degrades to the empty collection rather than failing the whole frame.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde::{Deserialize, Deserializer};

fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn default_screen_name() -> String {
    "unknown".to_owned()
}

fn default_health() -> f32 {
    -1.0
}

fn default_fov() -> f32 {
    70.0
}

fn default_attack_cooldown() -> f32 {
    1.0
}

fn default_attack_cooldown_per_tick() -> f32 {
    0.08
}

/// One telemetry frame from the bridge.
///
/// The legacy (1.8.9) bridge emits this document *without* a `type` property, so
/// nothing here may require a discriminator.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameState {
    #[serde(default)]
    pub mapped: bool,
    #[serde(rename = "guiOpen", default)]
    pub gui_open: bool,
    #[serde(rename = "inWorld", default)]
    pub in_world: bool,
    #[serde(rename = "screenName", default = "default_screen_name")]
    pub screen_name: String,
    #[serde(rename = "actionBar", default)]
    pub action_bar: String,
    #[serde(default = "default_health")]
    pub health: f32,
    #[serde(default = "default_fov")]
    pub fov: f32,
    #[serde(rename = "viewportWidth", default)]
    pub viewport_width: i32,
    #[serde(rename = "viewportHeight", default)]
    pub viewport_height: i32,
    #[serde(rename = "holdingBlock", default)]
    pub holding_block: bool,
    #[serde(rename = "lookingAtBlock", default)]
    pub looking_at_block: bool,
    #[serde(rename = "lookingAtEntity", default)]
    pub looking_at_entity: bool,
    #[serde(rename = "lookingAtEntityLatched", default)]
    pub looking_at_entity_latched: bool,
    /// `hurtTime` of the living entity under the crosshair; 0 when none.
    #[serde(rename = "lookingAtEntityHurtTime", default)]
    pub looking_at_entity_hurt_time: i32,
    /// Runtime reasons a supported module is idle, keyed by catalog id.
    #[serde(rename = "moduleStatus", default, deserialize_with = "null_to_default")]
    pub module_status: BTreeMap<String, String>,
    #[serde(rename = "breakingBlock", default)]
    pub breaking_block: bool,
    #[serde(rename = "attackCooldown", default = "default_attack_cooldown")]
    pub attack_cooldown: f32,
    #[serde(
        rename = "attackCooldownPerTick",
        default = "default_attack_cooldown_per_tick"
    )]
    pub attack_cooldown_per_tick: f32,
    #[serde(rename = "killAuraUnavailableReason", default)]
    pub kill_aura_unavailable_reason: String,
    #[serde(rename = "killAuraHasTarget", default)]
    pub kill_aura_has_target: bool,
    #[serde(rename = "killAuraBlocking", default)]
    pub kill_aura_blocking: bool,
    /// Last `moduleAction` request the bridge accepted, for acknowledgement.
    #[serde(rename = "lastActionId", default)]
    pub last_action_id: i32,
    #[serde(rename = "lastActionResult", default)]
    pub last_action_result: String,
    #[serde(rename = "stateMs", default)]
    pub state_ms: u64,
    #[serde(rename = "posX", default)]
    pub pos_x: f64,
    #[serde(rename = "posY", default)]
    pub pos_y: f64,
    #[serde(rename = "posZ", default)]
    pub pos_z: f64,
    #[serde(default)]
    pub pitch: f32,
    #[serde(default, deserialize_with = "null_to_default")]
    pub entities: Vec<EntityInfo>,
    #[serde(rename = "chestStealerState", default)]
    pub chest_stealer_state: Option<ChestStealerState>,
    #[serde(rename = "refillState", default)]
    pub refill_state: Option<RefillState>,
    #[serde(rename = "pixelPartyTargetFound", default)]
    pub pixel_party_target_found: bool,
    #[serde(rename = "pixelPartyTargetYaw", default)]
    pub pixel_party_target_yaw: f32,
    #[serde(rename = "pixelPartyTargetDist", default)]
    pub pixel_party_target_dist: f32,
    #[serde(rename = "pixelPartyYawDelta", default)]
    pub pixel_party_yaw_delta: f32,
    /// Whether the agent is connected and sending data. Never on the wire.
    #[serde(skip)]
    pub is_connected: bool,
    /// Timestamp of the last received update. Never on the wire.
    #[serde(skip)]
    pub last_update: Option<SystemTime>,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            mapped: false,
            gui_open: false,
            in_world: false,
            screen_name: default_screen_name(),
            action_bar: String::new(),
            health: default_health(),
            fov: default_fov(),
            viewport_width: 0,
            viewport_height: 0,
            holding_block: false,
            looking_at_block: false,
            looking_at_entity: false,
            looking_at_entity_latched: false,
            looking_at_entity_hurt_time: 0,
            module_status: BTreeMap::new(),
            breaking_block: false,
            attack_cooldown: default_attack_cooldown(),
            attack_cooldown_per_tick: default_attack_cooldown_per_tick(),
            kill_aura_unavailable_reason: String::new(),
            kill_aura_has_target: false,
            kill_aura_blocking: false,
            last_action_id: 0,
            last_action_result: String::new(),
            state_ms: 0,
            pos_x: 0.0,
            pos_y: 0.0,
            pos_z: 0.0,
            pitch: 0.0,
            entities: Vec::new(),
            chest_stealer_state: None,
            refill_state: None,
            pixel_party_target_found: false,
            pixel_party_target_yaw: 0.0,
            pixel_party_target_dist: 0.0,
            pixel_party_yaw_delta: 0.0,
            is_connected: false,
            last_update: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChestStealerState {
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub physical: bool,
    #[serde(rename = "windowId", default = "minus_one")]
    pub window_id: i32,
    #[serde(rename = "screenWidth", default)]
    pub screen_width: i32,
    #[serde(rename = "screenHeight", default)]
    pub screen_height: i32,
    #[serde(default, deserialize_with = "null_to_default")]
    pub slots: Vec<ChestStealerSlot>,
}

fn minus_one() -> i32 {
    -1
}

impl Default for ChestStealerState {
    fn default() -> Self {
        Self {
            ready: false,
            physical: false,
            window_id: -1,
            screen_width: 0,
            screen_height: 0,
            slots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct ChestStealerSlot {
    #[serde(default)]
    pub index: i32,
    #[serde(rename = "slotNumber", default)]
    pub slot_number: i32,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
}

/// Healing-item slots reported while the survival inventory is open.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RefillState {
    #[serde(default)]
    pub ready: bool,
    #[serde(rename = "windowId", default = "minus_one")]
    pub window_id: i32,
    #[serde(rename = "screenWidth", default)]
    pub screen_width: i32,
    #[serde(rename = "screenHeight", default)]
    pub screen_height: i32,
    #[serde(default, deserialize_with = "null_to_default")]
    pub slots: Vec<ChestStealerSlot>,
}

impl Default for RefillState {
    fn default() -> Self {
        Self {
            ready: false,
            window_id: -1,
            screen_width: 0,
            screen_height: 0,
            slots: Vec::new(),
        }
    }
}

impl RefillState {
    /// Adapts to the shared GUI coordinate mapper, which is `physical`-aware.
    #[must_use]
    pub fn to_chest_stealer_state(&self) -> ChestStealerState {
        ChestStealerState {
            ready: self.ready,
            physical: true,
            window_id: self.window_id,
            screen_width: self.screen_width,
            screen_height: self.screen_height,
            slots: self.slots.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EntityInfo {
    #[serde(default)]
    pub sx: f32,
    #[serde(default)]
    pub sy: f32,
    #[serde(default)]
    pub dist: f64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub hp: f32,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub z: f64,
}

impl Default for EntityInfo {
    fn default() -> Self {
        Self {
            sx: 0.0,
            sy: 0.0,
            dist: 0.0,
            name: String::new(),
            hp: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

impl EntityInfo {
    /// Alias kept from `EntityInfo.Distance` in the managed host.
    #[must_use]
    pub fn distance(&self) -> f64 {
        self.dist
    }
}
