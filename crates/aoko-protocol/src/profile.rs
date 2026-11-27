//! Profile persistence, ported from `Aoko/Core/Profile.cs`.
//!
//! Serialisation is `System.Text.Json`-pinned: camelCase keys (`MinCPS` ->
//! `minCPS`, not `minCps`), indented output, unknown fields ignored, missing
//! fields defaulted, and an existing user profile round-trips.
//!
//! Three setter cascades are part of the compatibility contract and are
//! reproduced here, including on load:
//!
//! * `chestStealerMenuCheck` cascades to the three sub-checks unless one of them
//!   was set explicitly, so an older profile that only has the menu flag keeps
//!   its meaning.
//! * `jitterEnabled` derives `randomizationMode` unless the mode was set
//!   explicitly.
//! * `autoRodSlotMode` clamps to `[0, 9]` and `autoRodExtensionTicks` to
//!   `[1, 40]`.
//!
//! The managed host loads through the setters in document order, so this crate
//! deserialises into an all-optional wire struct and applies the present values
//! through the same setters.
//!
//! # Reading a document
//!
//! Three rules keep a profile file recoverable:
//!
//! * **Versioned.** Every save writes [`SCHEMA_VERSION_KEY`], and the one-way
//!   migrations run only for a document older than [`PROFILE_SCHEMA_VERSION`].
//!   A file with no version is a v1 file, which is what the managed host wrote.
//! * **Keys this build does not know survive.** They are kept on
//!   [`Profile::unknown_keys`] and written back verbatim, so a file a newer build
//!   or a hand edit gave extra keys is not pruned by saving it here.
//! * **A bad field costs one field.** Each key is decoded on its own: a value of
//!   the wrong type falls back to the field's default and is reported through
//!   [`ProfileLoad::problems`], where the whole document used to fail to load.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::block_esp::BlockEspTargetData;
use crate::hud::{HudLayout, KNOWN_ELEMENT_IDS};
use crate::kill_aura::KillAuraSettings;

/// The profile schema this build writes and understands.
///
/// `1` is the shape the managed host shipped, which wrote no version at all: a document
/// with no [`SCHEMA_VERSION_KEY`] is read as v1 and migrated. `2` adds the key itself and
/// keeps the keys this build does not know, so a later shape change can tell a saved v2 file
/// from a hand-edited one instead of re-running every key heuristics.
pub const PROFILE_SCHEMA_VERSION: i64 = 2;

/// The top-level key that records [`PROFILE_SCHEMA_VERSION`].
///
/// The managed reader ignores a key it does not know, so the `dev` host loads a file this
/// build wrote; the reverse is what the migrations cover.
pub const SCHEMA_VERSION_KEY: &str = "schemaVersion";

/// Click randomisation mode. Serialised as a number, matching the managed enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickRandomizationMode {
    Basic = 0,
    #[default]
    Medium = 1,
    Advanced = 2,
}

impl ClickRandomizationMode {
    #[must_use]
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    #[must_use]
    pub fn from_i64(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Basic),
            1 => Some(Self::Medium),
            2 => Some(Self::Advanced),
            _ => None,
        }
    }
}

impl Serialize for ClickRandomizationMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_i32(self.as_i32())
    }
}

impl<'de> Deserialize<'de> for ClickRandomizationMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = i64::deserialize(deserializer)?;
        Self::from_i64(raw).ok_or_else(|| serde::de::Error::custom("unknown randomization mode"))
    }
}

/// One HUD element entry as stored inside a profile.
///
/// All three fields default to `0`, matching the managed `double` defaults, so a
/// hand-edited entry with a missing `scale` reaches the clamp rules as `0.0` and
/// is then replaced by the element's canonical scale — exactly as
/// `ProfileManager.BuildHudLayout` behaves.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HudElementLayoutValue {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub scale: f64,
    /// Keys this build does not know on this element, written back on save.
    ///
    /// An element is the likeliest place for a newer writer to add a setting, because the
    /// bridge sends the layout it renders back through `hudLayout`.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// A saved configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Profile {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "guiTheme")]
    pub gui_theme: String,
    #[serde(rename = "moduleListStyle")]
    pub module_list_style: String,
    #[serde(rename = "showLogo")]
    pub show_logo: bool,
    #[serde(rename = "devMode")]
    pub dev_mode: bool,
    #[serde(rename = "discordRpcEnabled")]
    pub discord_rpc_enabled: bool,
    #[serde(rename = "isArmed")]
    pub is_armed: bool,
    #[serde(rename = "minCPS")]
    pub min_cps: f32,
    #[serde(rename = "maxCPS")]
    pub max_cps: f32,
    #[serde(rename = "leftClickEnabled")]
    pub left_click_enabled: bool,
    #[serde(rename = "rightClickEnabled")]
    pub right_click_enabled: bool,
    #[serde(rename = "rightMinCPS")]
    pub right_min_cps: f32,
    #[serde(rename = "rightMaxCPS")]
    pub right_max_cps: f32,
    #[serde(rename = "rightClickOnlyBlock")]
    pub right_click_only_block: bool,
    #[serde(rename = "breakBlocksEnabled")]
    pub break_blocks_enabled: bool,
    #[serde(rename = "clickInChests")]
    pub click_in_chests: bool,
    #[serde(rename = "aimAssistEnabled")]
    pub aim_assist_enabled: bool,
    #[serde(rename = "aimAssistFov")]
    pub aim_assist_fov: f32,
    #[serde(rename = "aimAssistRange")]
    pub aim_assist_range: f32,
    #[serde(rename = "aimAssistStrength")]
    pub aim_assist_strength: i32,
    #[serde(rename = "triggerbotEnabled")]
    pub triggerbot_enabled: bool,
    #[serde(rename = "triggerbotOnlyCrosshair")]
    pub triggerbot_only_crosshair: bool,
    #[serde(rename = "triggerbotOnlyIfCanAttack")]
    pub triggerbot_only_if_can_attack: bool,
    #[serde(rename = "triggerbotCooldownThreshold")]
    pub triggerbot_cooldown_threshold: i32,
    #[serde(rename = "triggerbotHitChance")]
    pub triggerbot_hit_chance: i32,
    #[serde(rename = "triggerbotRequireClick")]
    pub triggerbot_require_click: bool,
    #[serde(rename = "killAuraEnabled")]
    pub kill_aura_enabled: bool,
    #[serde(rename = "killAuraSettings")]
    pub kill_aura_settings: KillAuraSettings,
    #[serde(rename = "killAuraRange")]
    pub kill_aura_range: f32,
    #[serde(rename = "killAuraAimRange")]
    pub kill_aura_aim_range: f32,
    #[serde(rename = "killAuraMinTurnSpeed")]
    pub kill_aura_min_turn_speed: f32,
    #[serde(rename = "killAuraMaxTurnSpeed")]
    pub kill_aura_max_turn_speed: f32,
    #[serde(rename = "killAuraTargetMode")]
    pub kill_aura_target_mode: String,
    #[serde(rename = "killAuraSwitchDelayMs")]
    pub kill_aura_switch_delay_ms: i32,
    #[serde(rename = "killAuraRandomization")]
    pub kill_aura_randomization: i32,
    #[serde(rename = "killAuraClickMode")]
    pub kill_aura_click_mode: String,
    #[serde(rename = "killAuraMinCps")]
    pub kill_aura_min_cps: f32,
    #[serde(rename = "killAuraMaxCps")]
    pub kill_aura_max_cps: f32,
    #[serde(rename = "killAuraFov")]
    pub kill_aura_fov: i32,
    #[serde(rename = "killAuraRotMode")]
    pub kill_aura_rot_mode: String,
    #[serde(rename = "killAuraRequirePress")]
    pub kill_aura_require_press: bool,
    #[serde(rename = "killAuraWeaponsOnly")]
    pub kill_aura_weapons_only: bool,
    #[serde(rename = "speedBridgeEnabled")]
    pub speed_bridge_enabled: bool,
    #[serde(rename = "speedBridgeBlockOnly")]
    pub speed_bridge_block_only: bool,
    #[serde(rename = "speedBridgeDelayMs")]
    pub speed_bridge_delay_ms: i32,
    #[serde(rename = "speedBridgeHoldingShiftOnly")]
    pub speed_bridge_holding_shift_only: bool,
    #[serde(rename = "speedBridgeLookingDownOnly")]
    pub speed_bridge_looking_down_only: bool,
    #[serde(rename = "gtbHelperEnabled")]
    pub gtb_helper_enabled: bool,
    #[serde(rename = "pixelPartyAssistEnabled")]
    pub pixel_party_assist_enabled: bool,
    #[serde(rename = "pixelPartyScanRadius")]
    pub pixel_party_scan_radius: i32,
    #[serde(rename = "pixelPartyAutoLookEnabled")]
    pub pixel_party_auto_look_enabled: bool,
    #[serde(rename = "pixelPartyAutoWalkEnabled")]
    pub pixel_party_auto_walk_enabled: bool,
    #[serde(rename = "nametagsEnabled")]
    pub nametags_enabled: bool,
    #[serde(rename = "nickHiderEnabled")]
    pub nick_hider_enabled: bool,
    #[serde(rename = "nickHiderAlias")]
    pub nick_hider_alias: String,
    #[serde(rename = "showModuleList")]
    pub show_module_list: bool,
    #[serde(rename = "closestPlayerInfoEnabled")]
    pub closest_player_info_enabled: bool,
    #[serde(rename = "fightStatusEnabled")]
    pub fight_status_enabled: bool,
    #[serde(rename = "nametagShowHealth")]
    pub nametag_show_health: bool,
    #[serde(rename = "nametagShowArmor")]
    pub nametag_show_armor: bool,
    #[serde(rename = "nametagShowHeldItem")]
    pub nametag_show_held_item: bool,
    #[serde(rename = "nametagHideVanilla")]
    pub nametag_hide_vanilla: bool,
    #[serde(rename = "nametagRange")]
    pub nametag_range: i32,
    #[serde(rename = "chestEspEnabled")]
    pub chest_esp_enabled: bool,
    #[serde(rename = "chestEspRange")]
    pub chest_esp_range: i32,
    #[serde(rename = "chestStealerEnabled")]
    pub chest_stealer_enabled: bool,
    #[serde(rename = "chestStealerDelayMs")]
    pub chest_stealer_delay_ms: i32,
    #[serde(rename = "refillEnabled")]
    pub refill_enabled: bool,
    #[serde(rename = "refillDelayMs")]
    pub refill_delay_ms: i32,
    #[serde(rename = "blockEspEnabled")]
    pub block_esp_enabled: bool,
    #[serde(rename = "blockEspBoxes")]
    pub block_esp_boxes: bool,
    #[serde(rename = "blockEspTracers")]
    pub block_esp_tracers: bool,
    #[serde(rename = "blockEspHud")]
    pub block_esp_hud: bool,
    #[serde(rename = "blockEspMaxCount")]
    pub block_esp_max_count: i32,
    #[serde(rename = "blockEspRange")]
    pub block_esp_range: i32,
    #[serde(rename = "blockEspTargets")]
    pub block_esp_targets: Vec<BlockEspTargetData>,
    #[serde(rename = "bedPlatesEnabled")]
    pub bed_plates_enabled: bool,
    #[serde(rename = "bedPlatesShowDistance")]
    pub bed_plates_show_distance: bool,
    #[serde(rename = "bedPlatesRange")]
    pub bed_plates_range: i32,
    #[serde(rename = "reachEnabled")]
    pub reach_enabled: bool,
    #[serde(rename = "reachMin")]
    pub reach_min: f32,
    #[serde(rename = "reachMax")]
    pub reach_max: f32,
    #[serde(rename = "reachChance")]
    pub reach_chance: i32,
    #[serde(rename = "reachChanceMode")]
    pub reach_chance_mode: i32,
    #[serde(rename = "reachOnlyWhileSprinting")]
    pub reach_only_while_sprinting: bool,
    #[serde(rename = "reachDisableInWater")]
    pub reach_disable_in_water: bool,
    #[serde(rename = "reachVerticalCheck")]
    pub reach_vertical_check: bool,
    #[serde(rename = "velocityEnabled")]
    pub velocity_enabled: bool,
    #[serde(rename = "velocityHorizontal")]
    pub velocity_horizontal: i32,
    #[serde(rename = "velocityVertical")]
    pub velocity_vertical: i32,
    #[serde(rename = "velocityChance")]
    pub velocity_chance: i32,
    #[serde(rename = "autoTotemEnabled")]
    pub auto_totem_enabled: bool,
    #[serde(rename = "autoTotemMode")]
    pub auto_totem_mode: i32,
    #[serde(rename = "autoTotemHealth")]
    pub auto_totem_health: i32,
    #[serde(rename = "autoTotemElytra")]
    pub auto_totem_elytra: bool,
    #[serde(rename = "autoTotemDelay")]
    pub auto_totem_delay: i32,
    #[serde(rename = "autoTotemBehaviorMode")]
    pub auto_totem_behavior_mode: i32,
    #[serde(rename = "autoRodEnabled")]
    pub auto_rod_enabled: bool,
    #[serde(rename = "throwpotEnabled")]
    pub throwpot_enabled: bool,
    #[serde(rename = "autoHealEnabled")]
    pub auto_heal_enabled: bool,
    #[serde(rename = "autoHealHealth")]
    pub auto_heal_health: i32,
    #[serde(rename = "autoHealDelayMs")]
    pub auto_heal_delay_ms: i32,
    #[serde(rename = "autoRodVerifyForcedSlot")]
    pub auto_rod_verify_forced_slot: bool,
    #[serde(rename = "autoRodHoldToExtend")]
    pub auto_rod_hold_to_extend: bool,
    #[serde(rename = "autoRodActionKey")]
    pub auto_rod_action_key: i32,
    #[serde(rename = "throwpotActionKey")]
    pub throwpot_action_key: i32,
    #[serde(rename = "autoToolEnabled")]
    pub auto_tool_enabled: bool,
    #[serde(rename = "autoToolSwapWeapon")]
    pub auto_tool_swap_weapon: bool,
    #[serde(rename = "autoToolInstantSwap")]
    pub auto_tool_instant_swap: bool,
    #[serde(rename = "autoToolSwapToDelay")]
    pub auto_tool_swap_to_delay: i32,
    #[serde(rename = "autoToolSwapBack")]
    pub auto_tool_swap_back: bool,
    #[serde(rename = "autoToolSwapBackDelay")]
    pub auto_tool_swap_back_delay: i32,
    #[serde(rename = "autoToolRequireMouseDown")]
    pub auto_tool_require_mouse_down: bool,
    #[serde(rename = "autoToolOnlySneaking")]
    pub auto_tool_only_sneaking: bool,
    #[serde(rename = "autoToolBedwarsMode")]
    pub auto_tool_bedwars_mode: bool,
    #[serde(rename = "antiDebuffEnabled")]
    pub anti_debuff_enabled: bool,
    #[serde(rename = "hitDelayFixEnabled")]
    pub hit_delay_fix_enabled: bool,
    #[serde(rename = "fireballWarningEnabled")]
    pub fireball_warning_enabled: bool,
    #[serde(rename = "fireballWarningBox")]
    pub fireball_warning_box: bool,
    #[serde(rename = "fireballWarningArrow")]
    pub fireball_warning_arrow: bool,
    #[serde(rename = "fireballWarningSound")]
    pub fireball_warning_sound: bool,
    #[serde(rename = "fastPlaceEnabled")]
    pub fast_place_enabled: bool,
    #[serde(rename = "noHurtCamEnabled")]
    pub no_hurt_cam_enabled: bool,
    #[serde(rename = "sprintEnabled")]
    pub sprint_enabled: bool,
    #[serde(rename = "tntTimerEnabled")]
    pub tnt_timer_enabled: bool,
    #[serde(rename = "wTapEnabled")]
    pub wtap_enabled: bool,
    #[serde(rename = "jumpResetEnabled")]
    pub jump_reset_enabled: bool,
    #[serde(rename = "hitSelectEnabled")]
    pub hit_select_enabled: bool,
    #[serde(rename = "trajectoriesEnabled")]
    pub trajectories_enabled: bool,
    #[serde(rename = "murderMysteryEnabled")]
    pub murder_mystery_enabled: bool,
    #[serde(rename = "breakProgressEnabled")]
    pub break_progress_enabled: bool,
    #[serde(rename = "jumpResetChance")]
    pub jump_reset_chance: i32,
    #[serde(rename = "hudLayout")]
    pub hud_layout: Option<BTreeMap<String, HudElementLayoutValue>>,
    #[serde(rename = "moduleKeys")]
    pub module_keys: BTreeMap<String, i32>,
    #[serde(rename = "theme")]
    pub theme: String,

    // Cascading / clamped properties, reached through accessors.
    #[serde(rename = "randomizationMode")]
    randomization_mode: ClickRandomizationMode,
    #[serde(rename = "jitterEnabled")]
    jitter_enabled: bool,
    #[serde(rename = "chestStealerMenuCheck")]
    chest_stealer_menu_check: bool,
    #[serde(rename = "chestStealerTitleCheck")]
    chest_stealer_title_check: bool,
    #[serde(rename = "chestStealerCustomItemsCheck")]
    chest_stealer_custom_items_check: bool,
    #[serde(rename = "chestStealerPhysicalCheck")]
    chest_stealer_physical_check: bool,
    #[serde(rename = "autoRodSlotMode")]
    auto_rod_slot_mode: i32,
    #[serde(rename = "autoRodExtensionTicks")]
    auto_rod_extension_ticks: i32,
    #[serde(skip)]
    pub has_explicit_randomization_mode: bool,
    #[serde(skip)]
    pub has_explicit_sub_checks: bool,

    /// Top-level keys this build does not know, written back unchanged by
    /// [`to_profile_json`].
    ///
    /// A profile written by a newer build, or edited by hand, can carry keys this schema has
    /// no field for. They are kept out of the profile's own behaviour and out of every
    /// payload, and only travel: read from the document, written to the next document, so
    /// saving a config here does not delete them. [`SCHEMA_VERSION_KEY`] is not one of them —
    /// it is written from [`PROFILE_SCHEMA_VERSION`] on every save.
    #[serde(flatten)]
    pub unknown_keys: BTreeMap<String, Value>,
}

/// The managed default module-key map: every keybindable module at key 0.
#[must_use]
pub fn default_module_keys() -> BTreeMap<String, i32> {
    MODULE_KEY_DEFAULTS
        .iter()
        .map(|(id, key)| ((*id).to_owned(), *key))
        .collect()
}

/// Module ids the profile carries a bind for, in the managed declaration order.
pub const MODULE_KEY_DEFAULTS: [(&str, i32); 39] = [
    ("autoclicker", 0),
    ("rightclick", 0),
    ("aimassist", 0),
    ("triggerbot", 0),
    ("killaura", 0),
    ("speedbridge", 0),
    ("gtbhelper", 0),
    ("pixelpartyassist", 0),
    ("nametags", 0),
    ("nickhider", 0),
    ("closestplayer", 0),
    ("fightstatus", 0),
    ("chestesp", 0),
    ("cheststealer", 0),
    ("refill", 0),
    ("blockesp", 0),
    ("bedplates", 0),
    ("reach", 0),
    ("velocity", 0),
    ("autototem", 0),
    ("autorod", 0),
    ("throwpot", 0),
    ("autoheal", 0),
    ("autotool", 0),
    ("antidebuff", 0),
    ("hitdelayfix", 0),
    ("fireballwarning", 0),
    ("fastplace", 0),
    ("nohurtcam", 0),
    ("sprint", 0),
    ("tnttimer", 0),
    ("wtap", 0),
    ("jumpreset", 0),
    ("hitselect", 0),
    ("trajectories", 0),
    ("murdermystery", 0),
    ("breakprogress", 0),
    ("panic", 0),
    ("hudeditor", 0),
];

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Default".to_owned(),
            gui_theme: "Slate".to_owned(),
            module_list_style: "Default".to_owned(),
            show_logo: true,
            dev_mode: false,
            discord_rpc_enabled: true,
            is_armed: false,
            min_cps: 8.0,
            max_cps: 12.0,
            left_click_enabled: true,
            right_click_enabled: false,
            right_min_cps: 10.0,
            right_max_cps: 14.0,
            right_click_only_block: false,
            break_blocks_enabled: false,
            click_in_chests: false,
            aim_assist_enabled: false,
            aim_assist_fov: 30.0,
            aim_assist_range: 4.5,
            aim_assist_strength: 40,
            triggerbot_enabled: false,
            triggerbot_only_crosshair: true,
            triggerbot_only_if_can_attack: true,
            triggerbot_cooldown_threshold: 92,
            triggerbot_hit_chance: 100,
            triggerbot_require_click: true,
            kill_aura_enabled: false,
            kill_aura_settings: KillAuraSettings::default(),
            kill_aura_range: 2.9,
            kill_aura_aim_range: 3.5,
            kill_aura_min_turn_speed: 10.0,
            kill_aura_max_turn_speed: 20.0,
            kill_aura_target_mode: "distance".to_owned(),
            kill_aura_switch_delay_ms: 350,
            kill_aura_randomization: 18,
            kill_aura_click_mode: "cooldown".to_owned(),
            kill_aura_min_cps: 10.0,
            kill_aura_max_cps: 14.0,
            kill_aura_fov: 120,
            kill_aura_rot_mode: "legit".to_owned(),
            kill_aura_require_press: false,
            kill_aura_weapons_only: true,
            speed_bridge_enabled: false,
            speed_bridge_block_only: true,
            speed_bridge_delay_ms: 85,
            speed_bridge_holding_shift_only: true,
            speed_bridge_looking_down_only: true,
            gtb_helper_enabled: false,
            pixel_party_assist_enabled: false,
            pixel_party_scan_radius: 28,
            pixel_party_auto_look_enabled: false,
            pixel_party_auto_walk_enabled: false,
            nametags_enabled: false,
            nick_hider_enabled: false,
            nick_hider_alias: String::new(),
            show_module_list: true,
            closest_player_info_enabled: false,
            fight_status_enabled: false,
            nametag_show_health: true,
            nametag_show_armor: true,
            nametag_show_held_item: true,
            nametag_hide_vanilla: false,
            nametag_range: 4,
            chest_esp_enabled: false,
            chest_esp_range: 4,
            chest_stealer_enabled: false,
            chest_stealer_delay_ms: 120,
            refill_enabled: false,
            refill_delay_ms: 120,
            block_esp_enabled: false,
            block_esp_boxes: true,
            block_esp_tracers: false,
            block_esp_hud: true,
            block_esp_max_count: 64,
            block_esp_range: 4,
            block_esp_targets: BlockEspTargetData::build_defaults(),
            bed_plates_enabled: false,
            bed_plates_show_distance: true,
            bed_plates_range: 4,
            reach_enabled: false,
            reach_min: 3.0,
            reach_max: 3.0,
            reach_chance: 100,
            reach_chance_mode: 0,
            reach_only_while_sprinting: false,
            reach_disable_in_water: false,
            reach_vertical_check: false,
            velocity_enabled: false,
            velocity_horizontal: 100,
            velocity_vertical: 100,
            velocity_chance: 100,
            auto_totem_enabled: false,
            auto_totem_mode: 0,
            auto_totem_health: 10,
            auto_totem_elytra: true,
            auto_totem_delay: 0,
            auto_totem_behavior_mode: 0,
            auto_rod_enabled: false,
            throwpot_enabled: false,
            auto_heal_enabled: false,
            auto_heal_health: 17,
            auto_heal_delay_ms: 500,
            auto_rod_verify_forced_slot: true,
            auto_rod_hold_to_extend: false,
            auto_rod_action_key: 0,
            throwpot_action_key: 0,
            auto_tool_enabled: false,
            auto_tool_swap_weapon: true,
            auto_tool_instant_swap: true,
            auto_tool_swap_to_delay: 50,
            auto_tool_swap_back: false,
            auto_tool_swap_back_delay: 350,
            auto_tool_require_mouse_down: true,
            auto_tool_only_sneaking: false,
            auto_tool_bedwars_mode: true,
            anti_debuff_enabled: false,
            hit_delay_fix_enabled: false,
            fireball_warning_enabled: false,
            fireball_warning_box: true,
            fireball_warning_arrow: true,
            fireball_warning_sound: true,
            fast_place_enabled: false,
            no_hurt_cam_enabled: false,
            sprint_enabled: false,
            tnt_timer_enabled: false,
            wtap_enabled: false,
            jump_reset_enabled: false,
            hit_select_enabled: false,
            trajectories_enabled: false,
            murder_mystery_enabled: false,
            break_progress_enabled: false,
            jump_reset_chance: 80,
            hud_layout: None,
            module_keys: default_module_keys(),
            theme: "Slate".to_owned(),
            randomization_mode: ClickRandomizationMode::Medium,
            jitter_enabled: true,
            chest_stealer_menu_check: true,
            chest_stealer_title_check: true,
            chest_stealer_custom_items_check: true,
            chest_stealer_physical_check: true,
            auto_rod_slot_mode: 0,
            auto_rod_extension_ticks: 4,
            has_explicit_randomization_mode: false,
            has_explicit_sub_checks: false,
            unknown_keys: BTreeMap::new(),
        }
    }
}

impl Profile {
    /// Click randomisation mode actually applied.
    #[must_use]
    pub fn randomization_mode(&self) -> ClickRandomizationMode {
        self.randomization_mode
    }

    /// Sets the mode and marks it explicit, so `jitterEnabled` stops deriving it.
    pub fn set_randomization_mode(&mut self, mode: ClickRandomizationMode) -> bool {
        self.has_explicit_randomization_mode = true;
        if self.randomization_mode == mode {
            return false;
        }
        self.randomization_mode = mode;
        true
    }

    #[must_use]
    pub fn jitter_enabled(&self) -> bool {
        self.jitter_enabled
    }

    /// Setting jitter derives the randomisation mode (Medium/Basic) unless the
    /// mode was set explicitly.
    pub fn set_jitter_enabled(&mut self, value: bool) -> bool {
        self.jitter_enabled = value;
        if !self.has_explicit_randomization_mode {
            self.randomization_mode = if value {
                ClickRandomizationMode::Medium
            } else {
                ClickRandomizationMode::Basic
            };
        }
        true
    }

    #[must_use]
    pub fn chest_stealer_menu_check(&self) -> bool {
        self.chest_stealer_menu_check
    }

    /// The legacy single flag: setting it cascades to the three sub-checks unless
    /// one was set explicitly.
    pub fn set_chest_stealer_menu_check(&mut self, value: bool) -> bool {
        self.chest_stealer_menu_check = value;
        if !self.has_explicit_sub_checks {
            self.chest_stealer_title_check = value;
            self.chest_stealer_custom_items_check = value;
            self.chest_stealer_physical_check = value;
        }
        true
    }

    #[must_use]
    pub fn chest_stealer_title_check(&self) -> bool {
        self.chest_stealer_title_check
    }

    pub fn set_chest_stealer_title_check(&mut self, value: bool) -> bool {
        self.chest_stealer_title_check = value;
        self.has_explicit_sub_checks = true;
        true
    }

    #[must_use]
    pub fn chest_stealer_custom_items_check(&self) -> bool {
        self.chest_stealer_custom_items_check
    }

    pub fn set_chest_stealer_custom_items_check(&mut self, value: bool) -> bool {
        self.chest_stealer_custom_items_check = value;
        self.has_explicit_sub_checks = true;
        true
    }

    #[must_use]
    pub fn chest_stealer_physical_check(&self) -> bool {
        self.chest_stealer_physical_check
    }

    pub fn set_chest_stealer_physical_check(&mut self, value: bool) -> bool {
        self.chest_stealer_physical_check = value;
        self.has_explicit_sub_checks = true;
        true
    }

    /// Clears the dedicated Auto Rod and Throwpot action binds when they collide
    /// with a general module bind.
    ///
    /// Mirrors the ordering in `Profile.ApplyTo`: the general module binds are
    /// applied first and win, so a dedicated bind that duplicates any module bind is
    /// refused and the stored value reset to `0`. The general bind is always
    /// preserved. A zero or negative action key never collides, which is also why the
    /// fresh profile state is a no-op here.
    ///
    /// The app layer calls this once, right after it has applied `module_keys` and
    /// the two action binds; loading a document does not normalise it, so a profile
    /// file round-trips as stored.
    ///
    /// Returns whether anything changed.
    pub fn normalize_action_binds(&mut self) -> bool {
        let collides = |key: i32, module_keys: &BTreeMap<String, i32>| {
            key > 0 && module_keys.values().any(|value| *value == key)
        };

        let mut changed = false;
        if collides(self.auto_rod_action_key, &self.module_keys) {
            self.auto_rod_action_key = 0;
            changed = true;
        }
        if collides(self.throwpot_action_key, &self.module_keys) {
            self.throwpot_action_key = 0;
            changed = true;
        }
        changed
    }

    /// Whether a dedicated action bind currently collides with a module bind.
    #[must_use]
    pub fn has_action_bind_collision(&self) -> bool {
        let collides = |key: i32| key > 0 && self.module_keys.values().any(|value| *value == key);
        collides(self.auto_rod_action_key) || collides(self.throwpot_action_key)
    }

    /// Forced Auto Rod slot, clamped to `[0, 9]`.
    #[must_use]
    pub fn auto_rod_slot_mode(&self) -> i32 {
        self.auto_rod_slot_mode
    }

    pub fn set_auto_rod_slot_mode(&mut self, value: i32) -> bool {
        let clamped = value.clamp(0, 9);
        if self.auto_rod_slot_mode == clamped {
            return false;
        }
        self.auto_rod_slot_mode = clamped;
        true
    }

    /// Extra Auto Rod hold ticks, clamped to `[1, 40]`.
    #[must_use]
    pub fn auto_rod_extension_ticks(&self) -> i32 {
        self.auto_rod_extension_ticks
    }

    pub fn set_auto_rod_extension_ticks(&mut self, value: i32) -> bool {
        let clamped = value.clamp(1, 40);
        if self.auto_rod_extension_ticks == clamped {
            return false;
        }
        self.auto_rod_extension_ticks = clamped;
        true
    }

    /// The HUD layout stored in the profile, rebuilt through the clamp rules.
    ///
    /// A profile that predates `hudLayout` yields canonical defaults.
    #[must_use]
    pub fn to_hud_layout(&self) -> HudLayout {
        let Some(stored) = &self.hud_layout else {
            return HudLayout::new();
        };

        let mut json = serde_json::Map::with_capacity(stored.len());
        for (id, value) in stored {
            let mut element = serde_json::Map::with_capacity(3);
            element.insert("x".to_owned(), Value::from(value.x));
            element.insert("y".to_owned(), Value::from(value.y));
            element.insert("scale".to_owned(), Value::from(value.scale));
            json.insert(id.clone(), Value::Object(element));
        }

        HudLayout::from_json(&Value::Object(json))
    }

    /// The stored form of a live layout: all seven element ids.
    #[must_use]
    pub fn hud_layout_map(layout: &HudLayout) -> BTreeMap<String, HudElementLayoutValue> {
        KNOWN_ELEMENT_IDS
            .iter()
            .map(|id| {
                let element = layout.get(id);
                (
                    (*id).to_owned(),
                    HudElementLayoutValue {
                        x: element.x,
                        y: element.y,
                        scale: element.scale,
                        unknown: BTreeMap::new(),
                    },
                )
            })
            .collect()
    }
}

/// Deserialisation shim: every field optional so a profile written by an older or
/// newer build still loads. Present values are applied exactly as the managed
/// property setters would.
///
/// Built key by key by [`ProfileWire::from_root`], which is what keeps one malformed
/// value to one field instead of the whole document.
struct ProfileWire {
    name: Option<String>,
    gui_theme: Option<String>,
    module_list_style: Option<String>,
    show_logo: Option<bool>,
    dev_mode: Option<bool>,
    discord_rpc_enabled: Option<bool>,
    is_armed: Option<bool>,
    min_cps: Option<f32>,
    max_cps: Option<f32>,
    left_click_enabled: Option<bool>,
    right_click_enabled: Option<bool>,
    right_min_cps: Option<f32>,
    right_max_cps: Option<f32>,
    right_click_only_block: Option<bool>,
    break_blocks_enabled: Option<bool>,
    click_in_chests: Option<bool>,
    aim_assist_enabled: Option<bool>,
    aim_assist_fov: Option<f32>,
    aim_assist_range: Option<f32>,
    aim_assist_strength: Option<i32>,
    triggerbot_enabled: Option<bool>,
    triggerbot_only_crosshair: Option<bool>,
    triggerbot_only_if_can_attack: Option<bool>,
    triggerbot_cooldown_threshold: Option<i32>,
    triggerbot_hit_chance: Option<i32>,
    triggerbot_require_click: Option<bool>,
    kill_aura_enabled: Option<bool>,
    kill_aura_settings: Option<KillAuraSettings>,
    kill_aura_range: Option<f32>,
    kill_aura_aim_range: Option<f32>,
    kill_aura_min_turn_speed: Option<f32>,
    kill_aura_max_turn_speed: Option<f32>,
    kill_aura_target_mode: Option<String>,
    kill_aura_switch_delay_ms: Option<i32>,
    kill_aura_randomization: Option<i32>,
    kill_aura_click_mode: Option<String>,
    kill_aura_min_cps: Option<f32>,
    kill_aura_max_cps: Option<f32>,
    kill_aura_fov: Option<i32>,
    kill_aura_rot_mode: Option<String>,
    kill_aura_require_press: Option<bool>,
    kill_aura_weapons_only: Option<bool>,
    speed_bridge_enabled: Option<bool>,
    speed_bridge_block_only: Option<bool>,
    speed_bridge_delay_ms: Option<i32>,
    speed_bridge_holding_shift_only: Option<bool>,
    speed_bridge_looking_down_only: Option<bool>,
    gtb_helper_enabled: Option<bool>,
    pixel_party_assist_enabled: Option<bool>,
    pixel_party_scan_radius: Option<i32>,
    pixel_party_auto_look_enabled: Option<bool>,
    pixel_party_auto_walk_enabled: Option<bool>,
    nametags_enabled: Option<bool>,
    nick_hider_enabled: Option<bool>,
    nick_hider_alias: Option<String>,
    show_module_list: Option<bool>,
    closest_player_info_enabled: Option<bool>,
    fight_status_enabled: Option<bool>,
    nametag_show_health: Option<bool>,
    nametag_show_armor: Option<bool>,
    nametag_show_held_item: Option<bool>,
    nametag_hide_vanilla: Option<bool>,
    nametag_range: Option<i32>,
    chest_esp_enabled: Option<bool>,
    chest_esp_range: Option<i32>,
    chest_stealer_enabled: Option<bool>,
    chest_stealer_delay_ms: Option<i32>,
    refill_enabled: Option<bool>,
    refill_delay_ms: Option<i32>,
    block_esp_enabled: Option<bool>,
    block_esp_boxes: Option<bool>,
    block_esp_tracers: Option<bool>,
    block_esp_hud: Option<bool>,
    block_esp_max_count: Option<i32>,
    block_esp_range: Option<i32>,
    block_esp_targets: Option<Vec<BlockEspTargetData>>,
    bed_plates_enabled: Option<bool>,
    bed_plates_show_distance: Option<bool>,
    bed_plates_range: Option<i32>,
    reach_enabled: Option<bool>,
    reach_min: Option<f32>,
    reach_max: Option<f32>,
    reach_chance: Option<i32>,
    reach_chance_mode: Option<i32>,
    reach_only_while_sprinting: Option<bool>,
    reach_disable_in_water: Option<bool>,
    reach_vertical_check: Option<bool>,
    velocity_enabled: Option<bool>,
    velocity_horizontal: Option<i32>,
    velocity_vertical: Option<i32>,
    velocity_chance: Option<i32>,
    auto_totem_enabled: Option<bool>,
    auto_totem_mode: Option<i32>,
    auto_totem_health: Option<i32>,
    auto_totem_elytra: Option<bool>,
    auto_totem_delay: Option<i32>,
    auto_totem_behavior_mode: Option<i32>,
    auto_rod_enabled: Option<bool>,
    throwpot_enabled: Option<bool>,
    auto_heal_enabled: Option<bool>,
    auto_heal_health: Option<i32>,
    auto_heal_delay_ms: Option<i32>,
    auto_rod_verify_forced_slot: Option<bool>,
    auto_rod_hold_to_extend: Option<bool>,
    auto_rod_action_key: Option<i32>,
    throwpot_action_key: Option<i32>,
    auto_tool_enabled: Option<bool>,
    auto_tool_swap_weapon: Option<bool>,
    auto_tool_instant_swap: Option<bool>,
    auto_tool_swap_to_delay: Option<i32>,
    auto_tool_swap_back: Option<bool>,
    auto_tool_swap_back_delay: Option<i32>,
    auto_tool_require_mouse_down: Option<bool>,
    auto_tool_only_sneaking: Option<bool>,
    auto_tool_bedwars_mode: Option<bool>,
    anti_debuff_enabled: Option<bool>,
    hit_delay_fix_enabled: Option<bool>,
    fireball_warning_enabled: Option<bool>,
    fireball_warning_box: Option<bool>,
    fireball_warning_arrow: Option<bool>,
    fireball_warning_sound: Option<bool>,
    fast_place_enabled: Option<bool>,
    no_hurt_cam_enabled: Option<bool>,
    sprint_enabled: Option<bool>,
    tnt_timer_enabled: Option<bool>,
    wtap_enabled: Option<bool>,
    jump_reset_enabled: Option<bool>,
    hit_select_enabled: Option<bool>,
    trajectories_enabled: Option<bool>,
    murder_mystery_enabled: Option<bool>,
    break_progress_enabled: Option<bool>,
    jump_reset_chance: Option<i32>,
    hud_layout: Option<Option<BTreeMap<String, HudElementLayoutValue>>>,
    module_keys: Option<BTreeMap<String, i32>>,
    theme: Option<String>,
    randomization_mode: Option<ClickRandomizationMode>,
    jitter_enabled: Option<bool>,
    chest_stealer_menu_check: Option<bool>,
    chest_stealer_title_check: Option<bool>,
    chest_stealer_custom_items_check: Option<bool>,
    chest_stealer_physical_check: Option<bool>,
    auto_rod_slot_mode: Option<i32>,
    auto_rod_extension_ticks: Option<i32>,
}

impl ProfileWire {
    /// Reads every key this schema knows, taking each one out of `fields`.
    ///
    /// What no `take` claims is left behind as the document's unknown keys, and a value
    /// that does not fit its field is reported and skipped, so the rest of the document
    /// still loads.
    fn from_root(fields: &mut ProfileFields) -> Self {
        Self {
            name: fields.take("name"),
            gui_theme: fields.take("guiTheme"),
            module_list_style: fields.take("moduleListStyle"),
            show_logo: fields.take("showLogo"),
            dev_mode: fields.take("devMode"),
            discord_rpc_enabled: fields.take("discordRpcEnabled"),
            is_armed: fields.take("isArmed"),
            min_cps: fields.take("minCPS"),
            max_cps: fields.take("maxCPS"),
            left_click_enabled: fields.take("leftClickEnabled"),
            right_click_enabled: fields.take("rightClickEnabled"),
            right_min_cps: fields.take("rightMinCPS"),
            right_max_cps: fields.take("rightMaxCPS"),
            right_click_only_block: fields.take("rightClickOnlyBlock"),
            break_blocks_enabled: fields.take("breakBlocksEnabled"),
            click_in_chests: fields.take("clickInChests"),
            aim_assist_enabled: fields.take("aimAssistEnabled"),
            aim_assist_fov: fields.take("aimAssistFov"),
            aim_assist_range: fields.take("aimAssistRange"),
            aim_assist_strength: fields.take("aimAssistStrength"),
            triggerbot_enabled: fields.take("triggerbotEnabled"),
            triggerbot_only_crosshair: fields.take("triggerbotOnlyCrosshair"),
            triggerbot_only_if_can_attack: fields.take("triggerbotOnlyIfCanAttack"),
            triggerbot_cooldown_threshold: fields.take("triggerbotCooldownThreshold"),
            triggerbot_hit_chance: fields.take("triggerbotHitChance"),
            triggerbot_require_click: fields.take("triggerbotRequireClick"),
            kill_aura_enabled: fields.take("killAuraEnabled"),
            kill_aura_settings: fields.take("killAuraSettings"),
            kill_aura_range: fields.take("killAuraRange"),
            kill_aura_aim_range: fields.take("killAuraAimRange"),
            kill_aura_min_turn_speed: fields.take("killAuraMinTurnSpeed"),
            kill_aura_max_turn_speed: fields.take("killAuraMaxTurnSpeed"),
            kill_aura_target_mode: fields.take("killAuraTargetMode"),
            kill_aura_switch_delay_ms: fields.take("killAuraSwitchDelayMs"),
            kill_aura_randomization: fields.take("killAuraRandomization"),
            kill_aura_click_mode: fields.take("killAuraClickMode"),
            kill_aura_min_cps: fields.take("killAuraMinCps"),
            kill_aura_max_cps: fields.take("killAuraMaxCps"),
            kill_aura_fov: fields.take("killAuraFov"),
            kill_aura_rot_mode: fields.take("killAuraRotMode"),
            kill_aura_require_press: fields.take("killAuraRequirePress"),
            kill_aura_weapons_only: fields.take("killAuraWeaponsOnly"),
            speed_bridge_enabled: fields.take("speedBridgeEnabled"),
            speed_bridge_block_only: fields.take("speedBridgeBlockOnly"),
            speed_bridge_delay_ms: fields.take("speedBridgeDelayMs"),
            speed_bridge_holding_shift_only: fields.take("speedBridgeHoldingShiftOnly"),
            speed_bridge_looking_down_only: fields.take("speedBridgeLookingDownOnly"),
            gtb_helper_enabled: fields.take("gtbHelperEnabled"),
            pixel_party_assist_enabled: fields.take("pixelPartyAssistEnabled"),
            pixel_party_scan_radius: fields.take("pixelPartyScanRadius"),
            pixel_party_auto_look_enabled: fields.take("pixelPartyAutoLookEnabled"),
            pixel_party_auto_walk_enabled: fields.take("pixelPartyAutoWalkEnabled"),
            nametags_enabled: fields.take("nametagsEnabled"),
            nick_hider_enabled: fields.take("nickHiderEnabled"),
            nick_hider_alias: fields.take("nickHiderAlias"),
            show_module_list: fields.take("showModuleList"),
            closest_player_info_enabled: fields.take("closestPlayerInfoEnabled"),
            fight_status_enabled: fields.take("fightStatusEnabled"),
            nametag_show_health: fields.take("nametagShowHealth"),
            nametag_show_armor: fields.take("nametagShowArmor"),
            nametag_show_held_item: fields.take("nametagShowHeldItem"),
            nametag_hide_vanilla: fields.take("nametagHideVanilla"),
            nametag_range: fields.take("nametagRange"),
            chest_esp_enabled: fields.take("chestEspEnabled"),
            chest_esp_range: fields.take("chestEspRange"),
            chest_stealer_enabled: fields.take("chestStealerEnabled"),
            chest_stealer_delay_ms: fields.take("chestStealerDelayMs"),
            refill_enabled: fields.take("refillEnabled"),
            refill_delay_ms: fields.take("refillDelayMs"),
            block_esp_enabled: fields.take("blockEspEnabled"),
            block_esp_boxes: fields.take("blockEspBoxes"),
            block_esp_tracers: fields.take("blockEspTracers"),
            block_esp_hud: fields.take("blockEspHud"),
            block_esp_max_count: fields.take("blockEspMaxCount"),
            block_esp_range: fields.take("blockEspRange"),
            block_esp_targets: fields.take("blockEspTargets"),
            bed_plates_enabled: fields.take("bedPlatesEnabled"),
            bed_plates_show_distance: fields.take("bedPlatesShowDistance"),
            bed_plates_range: fields.take("bedPlatesRange"),
            reach_enabled: fields.take("reachEnabled"),
            reach_min: fields.take("reachMin"),
            reach_max: fields.take("reachMax"),
            reach_chance: fields.take("reachChance"),
            reach_chance_mode: fields.take("reachChanceMode"),
            reach_only_while_sprinting: fields.take("reachOnlyWhileSprinting"),
            reach_disable_in_water: fields.take("reachDisableInWater"),
            reach_vertical_check: fields.take("reachVerticalCheck"),
            velocity_enabled: fields.take("velocityEnabled"),
            velocity_horizontal: fields.take("velocityHorizontal"),
            velocity_vertical: fields.take("velocityVertical"),
            velocity_chance: fields.take("velocityChance"),
            auto_totem_enabled: fields.take("autoTotemEnabled"),
            auto_totem_mode: fields.take("autoTotemMode"),
            auto_totem_health: fields.take("autoTotemHealth"),
            auto_totem_elytra: fields.take("autoTotemElytra"),
            auto_totem_delay: fields.take("autoTotemDelay"),
            auto_totem_behavior_mode: fields.take("autoTotemBehaviorMode"),
            auto_rod_enabled: fields.take("autoRodEnabled"),
            throwpot_enabled: fields.take("throwpotEnabled"),
            auto_heal_enabled: fields.take("autoHealEnabled"),
            auto_heal_health: fields.take("autoHealHealth"),
            auto_heal_delay_ms: fields.take("autoHealDelayMs"),
            auto_rod_verify_forced_slot: fields.take("autoRodVerifyForcedSlot"),
            auto_rod_hold_to_extend: fields.take("autoRodHoldToExtend"),
            auto_rod_action_key: fields.take("autoRodActionKey"),
            throwpot_action_key: fields.take("throwpotActionKey"),
            auto_tool_enabled: fields.take("autoToolEnabled"),
            auto_tool_swap_weapon: fields.take("autoToolSwapWeapon"),
            auto_tool_instant_swap: fields.take("autoToolInstantSwap"),
            auto_tool_swap_to_delay: fields.take("autoToolSwapToDelay"),
            auto_tool_swap_back: fields.take("autoToolSwapBack"),
            auto_tool_swap_back_delay: fields.take("autoToolSwapBackDelay"),
            auto_tool_require_mouse_down: fields.take("autoToolRequireMouseDown"),
            auto_tool_only_sneaking: fields.take("autoToolOnlySneaking"),
            auto_tool_bedwars_mode: fields.take("autoToolBedwarsMode"),
            anti_debuff_enabled: fields.take("antiDebuffEnabled"),
            hit_delay_fix_enabled: fields.take("hitDelayFixEnabled"),
            fireball_warning_enabled: fields.take("fireballWarningEnabled"),
            fireball_warning_box: fields.take("fireballWarningBox"),
            fireball_warning_arrow: fields.take("fireballWarningArrow"),
            fireball_warning_sound: fields.take("fireballWarningSound"),
            fast_place_enabled: fields.take("fastPlaceEnabled"),
            no_hurt_cam_enabled: fields.take("noHurtCamEnabled"),
            sprint_enabled: fields.take("sprintEnabled"),
            tnt_timer_enabled: fields.take("tntTimerEnabled"),
            wtap_enabled: fields.take("wTapEnabled"),
            jump_reset_enabled: fields.take("jumpResetEnabled"),
            hit_select_enabled: fields.take("hitSelectEnabled"),
            trajectories_enabled: fields.take("trajectoriesEnabled"),
            murder_mystery_enabled: fields.take("murderMysteryEnabled"),
            break_progress_enabled: fields.take("breakProgressEnabled"),
            jump_reset_chance: fields.take("jumpResetChance"),
            hud_layout: fields.take("hudLayout"),
            module_keys: fields.take("moduleKeys"),
            theme: fields.take("theme"),
            randomization_mode: fields.take("randomizationMode"),
            jitter_enabled: fields.take("jitterEnabled"),
            chest_stealer_menu_check: fields.take("chestStealerMenuCheck"),
            chest_stealer_title_check: fields.take("chestStealerTitleCheck"),
            chest_stealer_custom_items_check: fields.take("chestStealerCustomItemsCheck"),
            chest_stealer_physical_check: fields.take("chestStealerPhysicalCheck"),
            auto_rod_slot_mode: fields.take("autoRodSlotMode"),
            auto_rod_extension_ticks: fields.take("autoRodExtensionTicks"),
        }
    }
}

impl From<ProfileWire> for Profile {
    fn from(wire: ProfileWire) -> Self {
        let mut profile = Profile::default();

        // Cascading properties first, in managed declaration order: the mode then
        // jitter, the legacy menu flag then the three sub-checks. This is the
        // order the managed writer emits, so a round-trip is stable.
        if let Some(value) = wire.randomization_mode {
            profile.set_randomization_mode(value);
        }
        if let Some(value) = wire.jitter_enabled {
            profile.set_jitter_enabled(value);
        }
        if let Some(value) = wire.chest_stealer_menu_check {
            profile.set_chest_stealer_menu_check(value);
        }
        if let Some(value) = wire.chest_stealer_title_check {
            profile.set_chest_stealer_title_check(value);
        }
        if let Some(value) = wire.chest_stealer_custom_items_check {
            profile.set_chest_stealer_custom_items_check(value);
        }
        if let Some(value) = wire.chest_stealer_physical_check {
            profile.set_chest_stealer_physical_check(value);
        }
        if let Some(value) = wire.auto_rod_slot_mode {
            profile.set_auto_rod_slot_mode(value);
        }
        if let Some(value) = wire.auto_rod_extension_ticks {
            profile.set_auto_rod_extension_ticks(value);
        }

        if let Some(value) = wire.name {
            profile.name = value;
        }
        if let Some(value) = wire.gui_theme {
            profile.gui_theme = value;
        }
        if let Some(value) = wire.module_list_style {
            profile.module_list_style = value;
        }
        if let Some(value) = wire.show_logo {
            profile.show_logo = value;
        }
        if let Some(value) = wire.dev_mode {
            profile.dev_mode = value;
        }
        if let Some(value) = wire.discord_rpc_enabled {
            profile.discord_rpc_enabled = value;
        }
        if let Some(value) = wire.is_armed {
            profile.is_armed = value;
        }
        if let Some(value) = wire.min_cps {
            profile.min_cps = value;
        }
        if let Some(value) = wire.max_cps {
            profile.max_cps = value;
        }
        if let Some(value) = wire.left_click_enabled {
            profile.left_click_enabled = value;
        }
        if let Some(value) = wire.right_click_enabled {
            profile.right_click_enabled = value;
        }
        if let Some(value) = wire.right_min_cps {
            profile.right_min_cps = value;
        }
        if let Some(value) = wire.right_max_cps {
            profile.right_max_cps = value;
        }
        if let Some(value) = wire.right_click_only_block {
            profile.right_click_only_block = value;
        }
        if let Some(value) = wire.break_blocks_enabled {
            profile.break_blocks_enabled = value;
        }
        if let Some(value) = wire.click_in_chests {
            profile.click_in_chests = value;
        }
        if let Some(value) = wire.aim_assist_enabled {
            profile.aim_assist_enabled = value;
        }
        if let Some(value) = wire.aim_assist_fov {
            profile.aim_assist_fov = value;
        }
        if let Some(value) = wire.aim_assist_range {
            profile.aim_assist_range = value;
        }
        if let Some(value) = wire.aim_assist_strength {
            profile.aim_assist_strength = value;
        }
        if let Some(value) = wire.triggerbot_enabled {
            profile.triggerbot_enabled = value;
        }
        if let Some(value) = wire.triggerbot_only_crosshair {
            profile.triggerbot_only_crosshair = value;
        }
        if let Some(value) = wire.triggerbot_only_if_can_attack {
            profile.triggerbot_only_if_can_attack = value;
        }
        if let Some(value) = wire.triggerbot_cooldown_threshold {
            profile.triggerbot_cooldown_threshold = value;
        }
        if let Some(value) = wire.triggerbot_hit_chance {
            profile.triggerbot_hit_chance = value;
        }
        if let Some(value) = wire.triggerbot_require_click {
            profile.triggerbot_require_click = value;
        }
        if let Some(value) = wire.kill_aura_enabled {
            profile.kill_aura_enabled = value;
        }
        if let Some(value) = wire.kill_aura_settings {
            profile.kill_aura_settings = value;
        }
        if let Some(value) = wire.kill_aura_range {
            profile.kill_aura_range = value;
        }
        if let Some(value) = wire.kill_aura_aim_range {
            profile.kill_aura_aim_range = value;
        }
        if let Some(value) = wire.kill_aura_min_turn_speed {
            profile.kill_aura_min_turn_speed = value;
        }
        if let Some(value) = wire.kill_aura_max_turn_speed {
            profile.kill_aura_max_turn_speed = value;
        }
        if let Some(value) = wire.kill_aura_target_mode {
            profile.kill_aura_target_mode = value;
        }
        if let Some(value) = wire.kill_aura_switch_delay_ms {
            profile.kill_aura_switch_delay_ms = value;
        }
        if let Some(value) = wire.kill_aura_randomization {
            profile.kill_aura_randomization = value;
        }
        if let Some(value) = wire.kill_aura_click_mode {
            profile.kill_aura_click_mode = value;
        }
        if let Some(value) = wire.kill_aura_min_cps {
            profile.kill_aura_min_cps = value;
        }
        if let Some(value) = wire.kill_aura_max_cps {
            profile.kill_aura_max_cps = value;
        }
        if let Some(value) = wire.kill_aura_fov {
            profile.kill_aura_fov = value;
        }
        if let Some(value) = wire.kill_aura_rot_mode {
            profile.kill_aura_rot_mode = value;
        }
        if let Some(value) = wire.kill_aura_require_press {
            profile.kill_aura_require_press = value;
        }
        if let Some(value) = wire.kill_aura_weapons_only {
            profile.kill_aura_weapons_only = value;
        }
        if let Some(value) = wire.speed_bridge_enabled {
            profile.speed_bridge_enabled = value;
        }
        if let Some(value) = wire.speed_bridge_block_only {
            profile.speed_bridge_block_only = value;
        }
        if let Some(value) = wire.speed_bridge_delay_ms {
            profile.speed_bridge_delay_ms = value;
        }
        if let Some(value) = wire.speed_bridge_holding_shift_only {
            profile.speed_bridge_holding_shift_only = value;
        }
        if let Some(value) = wire.speed_bridge_looking_down_only {
            profile.speed_bridge_looking_down_only = value;
        }
        if let Some(value) = wire.gtb_helper_enabled {
            profile.gtb_helper_enabled = value;
        }
        if let Some(value) = wire.pixel_party_assist_enabled {
            profile.pixel_party_assist_enabled = value;
        }
        if let Some(value) = wire.pixel_party_scan_radius {
            profile.pixel_party_scan_radius = value;
        }
        if let Some(value) = wire.pixel_party_auto_look_enabled {
            profile.pixel_party_auto_look_enabled = value;
        }
        if let Some(value) = wire.pixel_party_auto_walk_enabled {
            profile.pixel_party_auto_walk_enabled = value;
        }
        if let Some(value) = wire.nametags_enabled {
            profile.nametags_enabled = value;
        }
        if let Some(value) = wire.nick_hider_enabled {
            profile.nick_hider_enabled = value;
        }
        if let Some(value) = wire.nick_hider_alias {
            profile.nick_hider_alias = value;
        }
        if let Some(value) = wire.show_module_list {
            profile.show_module_list = value;
        }
        if let Some(value) = wire.closest_player_info_enabled {
            profile.closest_player_info_enabled = value;
        }
        if let Some(value) = wire.fight_status_enabled {
            profile.fight_status_enabled = value;
        }
        if let Some(value) = wire.nametag_show_health {
            profile.nametag_show_health = value;
        }
        if let Some(value) = wire.nametag_show_armor {
            profile.nametag_show_armor = value;
        }
        if let Some(value) = wire.nametag_show_held_item {
            profile.nametag_show_held_item = value;
        }
        if let Some(value) = wire.nametag_hide_vanilla {
            profile.nametag_hide_vanilla = value;
        }
        if let Some(value) = wire.nametag_range {
            profile.nametag_range = value;
        }
        if let Some(value) = wire.chest_esp_enabled {
            profile.chest_esp_enabled = value;
        }
        if let Some(value) = wire.chest_esp_range {
            profile.chest_esp_range = value;
        }
        if let Some(value) = wire.chest_stealer_enabled {
            profile.chest_stealer_enabled = value;
        }
        if let Some(value) = wire.chest_stealer_delay_ms {
            profile.chest_stealer_delay_ms = value;
        }
        if let Some(value) = wire.refill_enabled {
            profile.refill_enabled = value;
        }
        if let Some(value) = wire.refill_delay_ms {
            profile.refill_delay_ms = value;
        }
        if let Some(value) = wire.block_esp_enabled {
            profile.block_esp_enabled = value;
        }
        if let Some(value) = wire.block_esp_boxes {
            profile.block_esp_boxes = value;
        }
        if let Some(value) = wire.block_esp_tracers {
            profile.block_esp_tracers = value;
        }
        if let Some(value) = wire.block_esp_hud {
            profile.block_esp_hud = value;
        }
        if let Some(value) = wire.block_esp_max_count {
            profile.block_esp_max_count = value;
        }
        if let Some(value) = wire.block_esp_range {
            profile.block_esp_range = value;
        }
        if let Some(value) = wire.block_esp_targets {
            profile.block_esp_targets = value;
        }
        if let Some(value) = wire.bed_plates_enabled {
            profile.bed_plates_enabled = value;
        }
        if let Some(value) = wire.bed_plates_show_distance {
            profile.bed_plates_show_distance = value;
        }
        if let Some(value) = wire.bed_plates_range {
            profile.bed_plates_range = value;
        }
        if let Some(value) = wire.reach_enabled {
            profile.reach_enabled = value;
        }
        if let Some(value) = wire.reach_min {
            profile.reach_min = value;
        }
        if let Some(value) = wire.reach_max {
            profile.reach_max = value;
        }
        if let Some(value) = wire.reach_chance {
            profile.reach_chance = value;
        }
        if let Some(value) = wire.reach_chance_mode {
            profile.reach_chance_mode = value;
        }
        if let Some(value) = wire.reach_only_while_sprinting {
            profile.reach_only_while_sprinting = value;
        }
        if let Some(value) = wire.reach_disable_in_water {
            profile.reach_disable_in_water = value;
        }
        if let Some(value) = wire.reach_vertical_check {
            profile.reach_vertical_check = value;
        }
        if let Some(value) = wire.velocity_enabled {
            profile.velocity_enabled = value;
        }
        if let Some(value) = wire.velocity_horizontal {
            profile.velocity_horizontal = value;
        }
        if let Some(value) = wire.velocity_vertical {
            profile.velocity_vertical = value;
        }
        if let Some(value) = wire.velocity_chance {
            profile.velocity_chance = value;
        }
        if let Some(value) = wire.auto_totem_enabled {
            profile.auto_totem_enabled = value;
        }
        if let Some(value) = wire.auto_totem_mode {
            profile.auto_totem_mode = value;
        }
        if let Some(value) = wire.auto_totem_health {
            profile.auto_totem_health = value;
        }
        if let Some(value) = wire.auto_totem_elytra {
            profile.auto_totem_elytra = value;
        }
        if let Some(value) = wire.auto_totem_delay {
            profile.auto_totem_delay = value;
        }
        if let Some(value) = wire.auto_totem_behavior_mode {
            profile.auto_totem_behavior_mode = value;
        }
        if let Some(value) = wire.auto_rod_enabled {
            profile.auto_rod_enabled = value;
        }
        if let Some(value) = wire.throwpot_enabled {
            profile.throwpot_enabled = value;
        }
        if let Some(value) = wire.auto_heal_enabled {
            profile.auto_heal_enabled = value;
        }
        if let Some(value) = wire.auto_heal_health {
            profile.auto_heal_health = value;
        }
        if let Some(value) = wire.auto_heal_delay_ms {
            profile.auto_heal_delay_ms = value;
        }
        if let Some(value) = wire.auto_rod_verify_forced_slot {
            profile.auto_rod_verify_forced_slot = value;
        }
        if let Some(value) = wire.auto_rod_hold_to_extend {
            profile.auto_rod_hold_to_extend = value;
        }
        if let Some(value) = wire.auto_rod_action_key {
            profile.auto_rod_action_key = value;
        }
        if let Some(value) = wire.throwpot_action_key {
            profile.throwpot_action_key = value;
        }
        if let Some(value) = wire.auto_tool_enabled {
            profile.auto_tool_enabled = value;
        }
        if let Some(value) = wire.auto_tool_swap_weapon {
            profile.auto_tool_swap_weapon = value;
        }
        if let Some(value) = wire.auto_tool_instant_swap {
            profile.auto_tool_instant_swap = value;
        }
        if let Some(value) = wire.auto_tool_swap_to_delay {
            profile.auto_tool_swap_to_delay = value;
        }
        if let Some(value) = wire.auto_tool_swap_back {
            profile.auto_tool_swap_back = value;
        }
        if let Some(value) = wire.auto_tool_swap_back_delay {
            profile.auto_tool_swap_back_delay = value;
        }
        if let Some(value) = wire.auto_tool_require_mouse_down {
            profile.auto_tool_require_mouse_down = value;
        }
        if let Some(value) = wire.auto_tool_only_sneaking {
            profile.auto_tool_only_sneaking = value;
        }
        if let Some(value) = wire.auto_tool_bedwars_mode {
            profile.auto_tool_bedwars_mode = value;
        }
        if let Some(value) = wire.anti_debuff_enabled {
            profile.anti_debuff_enabled = value;
        }
        if let Some(value) = wire.hit_delay_fix_enabled {
            profile.hit_delay_fix_enabled = value;
        }
        if let Some(value) = wire.fireball_warning_enabled {
            profile.fireball_warning_enabled = value;
        }
        if let Some(value) = wire.fireball_warning_box {
            profile.fireball_warning_box = value;
        }
        if let Some(value) = wire.fireball_warning_arrow {
            profile.fireball_warning_arrow = value;
        }
        if let Some(value) = wire.fireball_warning_sound {
            profile.fireball_warning_sound = value;
        }
        if let Some(value) = wire.fast_place_enabled {
            profile.fast_place_enabled = value;
        }
        if let Some(value) = wire.no_hurt_cam_enabled {
            profile.no_hurt_cam_enabled = value;
        }
        if let Some(value) = wire.sprint_enabled {
            profile.sprint_enabled = value;
        }
        if let Some(value) = wire.tnt_timer_enabled {
            profile.tnt_timer_enabled = value;
        }
        if let Some(value) = wire.wtap_enabled {
            profile.wtap_enabled = value;
        }
        if let Some(value) = wire.jump_reset_enabled {
            profile.jump_reset_enabled = value;
        }
        if let Some(value) = wire.hit_select_enabled {
            profile.hit_select_enabled = value;
        }
        if let Some(value) = wire.trajectories_enabled {
            profile.trajectories_enabled = value;
        }
        if let Some(value) = wire.murder_mystery_enabled {
            profile.murder_mystery_enabled = value;
        }
        if let Some(value) = wire.break_progress_enabled {
            profile.break_progress_enabled = value;
        }
        if let Some(value) = wire.jump_reset_chance {
            profile.jump_reset_chance = value;
        }
        if let Some(value) = wire.hud_layout {
            profile.hud_layout = value;
        }
        if let Some(value) = wire.module_keys {
            profile.module_keys = value;
        }
        if let Some(value) = wire.theme {
            profile.theme = value;
        }

        profile
    }
}

/// One field of a profile document that could not be read.
///
/// The field keeps its default and the rest of the document loads; this is what the reader
/// saw instead of a value. An empty [`key`](Self::key) means the document itself, not one of
/// its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProblem {
    /// The document key, spelled as the file spells it (`minCPS`, `killAuraSettings`, ...).
    pub key: String,
    /// What the decoder said about the value.
    pub message: String,
}

impl ProfileProblem {
    /// A problem with the document rather than with one of its fields.
    #[must_use]
    pub fn document(message: impl Into<String>) -> Self {
        Self {
            key: String::new(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProfileProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.key.is_empty() {
            return write!(formatter, "{}", self.message);
        }
        write!(formatter, "{}: {}", self.key, self.message)
    }
}

/// One profile document as read: the profile, its schema version, and everything the reader
/// had to skip.
///
/// [`profile`](Self::profile) is `None` only when the document is not a JSON object, which is
/// the managed `LoadProfile` catch-all. A document with a malformed field still loads — that
/// field at its default — and the field is named in [`problems`](Self::problems), so a caller
/// can report what it cost instead of losing the file.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileLoad {
    pub profile: Option<Profile>,
    /// The schema version the document declared, `1` when it declared none.
    pub schema_version: i64,
    pub problems: Vec<ProfileProblem>,
}

/// The parts of a document that were not decoded, and the fields that did not fit.
///
/// Every known key is taken out of `remaining` as it is read, so what is left when the wire is
/// built is exactly the part of the document this schema does not know. Reading key by key is
/// also what keeps one malformed value to one field: it is reported and skipped, and the field
/// it belonged to falls back to its default.
struct ProfileFields {
    remaining: serde_json::Map<String, Value>,
    problems: Vec<ProfileProblem>,
}

impl ProfileFields {
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
    /// [`SCHEMA_VERSION_KEY`] is read separately rather than kept here, so it is written from
    /// [`PROFILE_SCHEMA_VERSION`] and a document cannot carry two of them.
    fn unknown(&mut self) -> BTreeMap<String, Value> {
        self.remaining.remove(SCHEMA_VERSION_KEY);
        std::mem::take(&mut self.remaining).into_iter().collect()
    }
}

/// The schema version a document declares, defaulting to the unversioned v1 shape.
///
/// A `schemaVersion` that is not an integer reads as v1: the migrations are key-shaped, so
/// running them over a document that does not need them is a no-op.
fn schema_version_of(root: &serde_json::Map<String, Value>) -> i64 {
    root.get(SCHEMA_VERSION_KEY)
        .and_then(Value::as_i64)
        .unwrap_or(1)
}

impl<'de> Deserialize<'de> for Profile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let root = Value::deserialize(deserializer)?;
        let Some(object) = root.as_object() else {
            return Err(serde::de::Error::custom("a profile is a JSON object"));
        };
        let mut fields = ProfileFields::new(object);
        let mut profile = Profile::from(ProfileWire::from_root(&mut fields));
        profile.unknown_keys = fields.unknown();
        Ok(profile)
    }
}

/// Name of the internal auto-save slot, hidden from the user-facing config list.
pub const AUTO_SAVE_CONFIG_NAME: &str = "config";

/// Longest config-name stem the manager accepts.
pub const MAX_CONFIG_NAME_LENGTH: usize = 64;

/// Folder name the shipped app used before the rename.
const LEGACY_APP_FOLDER: &str = "LegoClicker";

/// `ProfileManager.GetProfilesDirectory`.
#[must_use]
pub fn get_profiles_directory(app_data: &Path) -> PathBuf {
    app_data.join("Aoko").join("profiles")
}

/// `ProfileManager.GetLegacyProfilesDirectory`.
#[must_use]
pub fn get_legacy_profiles_directory(app_data: &Path) -> PathBuf {
    app_data.join(LEGACY_APP_FOLDER).join("profiles")
}

/// Produces a safe file-name stem from user input.
///
/// Trims, replaces path separators with `_`, strips the characters Windows
/// rejects in a file name (the shipped host is Windows-only), then trims again,
/// drops trailing dots, refuses `.`/`..`, and caps the length at 64.
#[must_use]
pub fn sanitize_config_name(name: Option<&str>) -> String {
    let Some(name) = name else {
        return String::new();
    };
    if name.trim().is_empty() {
        return String::new();
    }

    let replaced = name.trim().replace(['\\', '/'], "_");
    let cleaned: String = replaced
        .chars()
        .filter(|character| !is_invalid_file_name_char(*character))
        .collect();

    let mut cleaned = cleaned.trim().to_owned();
    while cleaned.ends_with('.') {
        cleaned.pop();
    }

    if cleaned == "." || cleaned == ".." {
        return String::new();
    }

    if cleaned.chars().count() > MAX_CONFIG_NAME_LENGTH {
        cleaned = cleaned.chars().take(MAX_CONFIG_NAME_LENGTH).collect();
    }

    cleaned
}

/// `Path.GetInvalidFileNameChars()` on Windows.
fn is_invalid_file_name_char(character: char) -> bool {
    (character as u32) < 32
        || matches!(
            character,
            '"' | '<' | '>' | '|' | ':' | '*' | '?' | '\\' | '/'
        )
}

/// Serialises a profile with camelCase keys and indented output.
///
/// The bytes are **not** reproducible from the managed writer, and chasing that would
/// be wasted work: the eight setter-managed keys are emitted last here (the managed
/// serialiser follows its own property order), and `f32` values print as `8.0` where
/// `.NET` prints `8`. Per section 8 of the migration architecture, key order and float
/// formatting are not part of the profile contract.
///
/// The contract we do hold: every key and every value round-trips. A file written by
/// this function loads back with identical values, a file written by the managed host
/// loads here, and an untouched profile is written identically across saves.
///
/// The document starts with [`SCHEMA_VERSION_KEY`], which the managed reader ignores as an
/// unknown key, and ends with [`Profile::unknown_keys`], so a key this build does not know
/// survives the round trip. A value the profile was loaded with but could not decode is gone:
/// the field it belonged to is at its default by then.
#[must_use]
pub fn to_profile_json(profile: &Profile) -> String {
    serde_json::to_string_pretty(&ProfileDocument {
        schema_version: PROFILE_SCHEMA_VERSION,
        profile,
    })
    .unwrap_or_default()
}

/// The document [`to_profile_json`] writes: the schema version, then the profile's own keys in
/// declaration order, then the keys it does not know.
#[derive(Serialize)]
struct ProfileDocument<'a> {
    #[serde(rename = "schemaVersion")]
    schema_version: i64,
    #[serde(flatten)]
    profile: &'a Profile,
}

/// Reads a profile document and runs the one-way migrations, reporting every field it had to
/// skip.
///
/// The returned [`ProfileLoad::profile`] is `None` only when the document is not a JSON object,
/// matching `LoadProfile`'s catch-all. A field with the wrong type is reported in
/// [`ProfileLoad::problems`] and keeps its default, so one bad value costs one field rather than
/// the file.
///
/// The migrations run only for a document older than [`PROFILE_SCHEMA_VERSION`]: they are keyed
/// on the shape the managed host wrote, and re-running them over a document this build produced
/// would rewrite values the user has since changed.
#[must_use]
pub fn load_profile(json: &str) -> ProfileLoad {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return ProfileLoad {
            profile: None,
            schema_version: PROFILE_SCHEMA_VERSION,
            problems: vec![ProfileProblem::document("the file is not JSON")],
        };
    };
    let Some(object) = root.as_object() else {
        return ProfileLoad {
            profile: None,
            schema_version: PROFILE_SCHEMA_VERSION,
            problems: vec![ProfileProblem::document(
                "the document is not a JSON object",
            )],
        };
    };

    let schema_version = schema_version_of(object);
    let mut fields = ProfileFields::new(object);
    let mut profile = Profile::from(ProfileWire::from_root(&mut fields));
    profile.unknown_keys = fields.unknown();
    if schema_version < PROFILE_SCHEMA_VERSION {
        migrate_silent_aura_to_kill_aura(&root, &mut profile);
        migrate_flat_kill_aura_settings(&root, &mut profile);
    }

    ProfileLoad {
        profile: Some(profile),
        schema_version,
        problems: fields.problems,
    }
}

/// [`load_profile`] without the report: the profile, or `None` when the document is not a JSON
/// object.
///
/// A malformed field still loads at its default here; a caller that wants to know which field
/// it was, and what the decoder said, uses [`load_profile`].
#[must_use]
pub fn load_profile_from_json(json: &str) -> Option<Profile> {
    load_profile(json).profile
}

/// Carries forward only SilentAura values with identical OpenMyau semantics.
pub fn migrate_silent_aura_to_kill_aura(root: &Value, profile: &mut Profile) {
    profile.module_keys.remove("silentaura");

    let silent_enabled = matches!(root.get("silentAuraEnabled"), Some(Value::Bool(true)));
    if !silent_enabled || profile.kill_aura_enabled {
        return;
    }

    profile.kill_aura_enabled = true;

    if let Some(range) = root.get("silentAuraRange").and_then(Value::as_f64) {
        // The managed `TryGetSingle` refuses a value that is not a float32.
        if let Ok(range) = serde_json::from_value::<f32>(Value::from(range)) {
            profile.kill_aura_settings.set_attack_range(range);
        }
    }
    if let Some(min_cps) = root.get("silentAuraMinCps").and_then(Value::as_f64) {
        if let Ok(min_cps) = serde_json::from_value::<f32>(Value::from(min_cps)) {
            profile
                .kill_aura_settings
                .set_min_cps(round_half_even(min_cps) as i32);
        }
    }
    if let Some(max_cps) = root.get("silentAuraMaxCps").and_then(Value::as_f64) {
        if let Ok(max_cps) = serde_json::from_value::<f32>(Value::from(max_cps)) {
            profile
                .kill_aura_settings
                .set_max_cps(round_half_even(max_cps) as i32);
        }
    }
}

/// Migrates the pre-nesting flat kill-aura fields into `killAuraSettings`, only
/// when the profile has no nested object yet.
pub fn migrate_flat_kill_aura_settings(root: &Value, profile: &mut Profile) {
    if root.get("killAuraSettings").is_some() {
        return;
    }

    let attack_range = profile.kill_aura_range;
    let sort = profile.kill_aura_target_mode.clone();
    let switch_delay = profile.kill_aura_switch_delay_ms;
    let min_cps = profile.kill_aura_min_cps;
    let max_cps = profile.kill_aura_max_cps;
    let fov = profile.kill_aura_fov;
    let rotations = match profile.kill_aura_rot_mode.as_str() {
        "legit" => "legit",
        "lock" => "lockview",
        _ => "silent",
    };
    let require_press = profile.kill_aura_require_press;
    let weapons_only = profile.kill_aura_weapons_only;

    let settings = &mut profile.kill_aura_settings;
    settings.set_attack_range(attack_range);
    settings.set_sort(&sort);
    settings.set_switch_delay(switch_delay);
    settings.set_min_cps(round_half_even(min_cps) as i32);
    settings.set_max_cps(round_half_even(max_cps) as i32);
    settings.set_fov(fov);
    settings.set_rotations(rotations);
    settings.set_require_press(require_press);
    settings.set_weapons_only(weapons_only);
}

/// `MathF.Round`, which rounds half to even.
fn round_half_even(value: f32) -> f32 {
    let floor = value.floor();
    let fraction = value - floor;
    if (fraction - 0.5).abs() < f32::EPSILON {
        if (floor as i64) % 2 == 0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        value.round()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every save stamps the version, on the profile's own document rather than around it.
    #[test]
    fn a_saved_document_carries_the_schema_version() {
        let document = serde_json::from_str::<Value>(&to_profile_json(&Profile::default()))
            .expect("a saved profile is JSON");

        assert_eq!(
            document.get(SCHEMA_VERSION_KEY).and_then(Value::as_i64),
            Some(PROFILE_SCHEMA_VERSION)
        );
        assert_eq!(
            document.get("name").and_then(Value::as_str),
            Some("Default"),
            "the version joins the profile's keys instead of wrapping them"
        );
        assert!(
            document.get("unknownKeys").is_none(),
            "the unknown-key map is flattened into the document, not nested under a key"
        );
    }

    /// A file the managed host wrote declares no version, so it reads as v1 and gets the
    /// migrations.
    #[test]
    fn a_versionless_document_reads_as_v1_and_migrates() {
        let loaded = load_profile(
            r#"{"name":"Legacy","silentAuraEnabled":true,"moduleKeys":{"silentaura":65}}"#,
        );

        assert_eq!(loaded.schema_version, 1);
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let profile = loaded.profile.expect("a v1 document loads");
        assert!(profile.kill_aura_enabled, "SilentAura carried over");
        assert!(
            !profile.module_keys.contains_key("silentaura"),
            "the retired module bind is dropped"
        );
    }

    /// The same document stamped current: the migrations are for the shape the managed host
    /// wrote, and running them again would rewrite values the user has since changed.
    #[test]
    fn a_current_document_does_not_rerun_the_migrations() {
        let loaded = load_profile(
            r#"{"schemaVersion":2,"silentAuraEnabled":true,"moduleKeys":{"silentaura":65}}"#,
        );

        assert_eq!(loaded.schema_version, PROFILE_SCHEMA_VERSION);
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let profile = loaded.profile.expect("a v2 document loads");
        assert!(
            !profile.kill_aura_enabled,
            "the SilentAura migration did not run"
        );
        assert_eq!(profile.module_keys.get("silentaura").copied(), Some(65));
        assert!(
            profile.unknown_keys.contains_key("silentAuraEnabled"),
            "the retired key is carried as an unknown one, not acted on"
        );
    }

    /// A key this build does not know comes back unchanged, which is what a config written by a
    /// newer build, or edited by hand, needs from a save here.
    #[test]
    fn unknown_keys_survive_a_load_and_save() {
        let document = json!({
            "schemaVersion": 2,
            "name": "From A Newer Build",
            "minCPS": 9.5,
            "futureToggle": true,
            "futureField": {"nested": 1, "list": [1, 2, 3]},
            "futureTable": {"a": null},
        });

        let loaded = load_profile(&document.to_string());
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let profile = loaded.profile.expect("the document loads");
        assert_eq!(profile.min_cps, 9.5, "a known key still lands in its field");
        assert_eq!(
            profile.unknown_keys.get("futureToggle"),
            Some(&Value::Bool(true))
        );

        let saved =
            serde_json::from_str::<Value>(&to_profile_json(&profile)).expect("the save is JSON");
        for key in ["futureToggle", "futureField", "futureTable"] {
            assert_eq!(
                saved.get(key),
                document.get(key),
                "{key} was pruned by the save"
            );
        }
        assert_eq!(saved.get("minCPS"), Some(&json!(9.5)));

        // The document is stable: loading the save and saving again changes nothing, so an
        // untouched config is not rewritten differently on every save.
        let again = load_profile(&saved.to_string())
            .profile
            .expect("the save loads");
        assert_eq!(again.unknown_keys, profile.unknown_keys);
        assert_eq!(to_profile_json(&again), to_profile_json(&profile));

        // The serde path is the same reader, so it keeps them too.
        let direct = serde_json::from_value::<Profile>(document.clone()).expect("serde loads");
        assert_eq!(direct.unknown_keys, profile.unknown_keys);
    }

    /// A key this build does not know *inside* a stored HUD element survives as well: the
    /// bridge is the writer most likely to add one.
    #[test]
    fn a_nested_unknown_key_survives_inside_a_hud_element() {
        let document = json!({
            "hudLayout": {
                "modulelist": {"x": 0.25, "y": 0.5, "scale": 1.5, "pinned": true},
            },
        });

        let loaded = load_profile(&document.to_string());
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let profile = loaded.profile.expect("the document loads");
        let element = profile
            .hud_layout
            .as_ref()
            .and_then(|stored| stored.get("modulelist"))
            .expect("the element is stored");
        assert_eq!(element.x, 0.25);
        assert_eq!(element.unknown.get("pinned"), Some(&Value::Bool(true)));
        assert_eq!(
            profile.to_hud_layout().get("modulelist").y,
            0.5,
            "the live layout reads the three known fields"
        );

        let saved =
            serde_json::from_str::<Value>(&to_profile_json(&profile)).expect("the save is JSON");
        assert_eq!(
            saved["hudLayout"]["modulelist"]["pinned"],
            Value::Bool(true)
        );
        assert_eq!(saved["hudLayout"]["modulelist"]["scale"], json!(1.5));
    }

    /// One value of the wrong type costs that field and nothing else, and the reader names the
    /// field it skipped instead of refusing the file.
    #[test]
    fn one_wrong_typed_field_does_not_cost_the_document() {
        let loaded = load_profile(r#"{"name":"Half","minCPS":"9","maxCPS":14}"#);

        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.problems.len(), 1, "{:?}", loaded.problems);
        assert_eq!(loaded.problems[0].key, "minCPS");
        assert!(
            loaded.problems[0].to_string().starts_with("minCPS: "),
            "the report names the field: {}",
            loaded.problems[0]
        );

        let profile = loaded.profile.expect("the document still loads");
        assert_eq!(profile.name, "Half", "a good field is applied");
        assert_eq!(profile.max_cps, 14.0, "its neighbour is applied");
        assert_eq!(
            profile.min_cps,
            Profile::default().min_cps,
            "the bad field falls back to its default"
        );

        // The plain reader is the same load without the report.
        let plain = load_profile_from_json(r#"{"minCPS":"9"}"#).expect("the document loads");
        assert_eq!(plain.min_cps, Profile::default().min_cps);
    }

    /// A document that is not an object is the one case that still refuses to load, and it says
    /// why rather than looking like an empty profile.
    #[test]
    fn a_document_that_is_not_an_object_is_refused() {
        for text in ["[]", "\"profile\"", "{oops"] {
            let loaded = load_profile(text);
            assert!(loaded.profile.is_none(), "{text} is not a profile");
            assert_eq!(loaded.problems.len(), 1, "{text}");
            assert_eq!(
                loaded.problems[0].key, "",
                "the problem is the document's, not a field's"
            );
            assert!(load_profile_from_json(text).is_none());
        }
    }
}
