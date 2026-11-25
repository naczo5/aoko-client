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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::block_esp::BlockEspTargetData;
use crate::hud::{HudLayout, KNOWN_ELEMENT_IDS};
use crate::kill_aura::KillAuraSettings;

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
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct HudElementLayoutValue {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub scale: f64,
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
                    },
                )
            })
            .collect()
    }
}

/// Deserialisation shim: every field optional so a profile written by an older or
/// newer build still loads. Present values are applied exactly as the managed
/// property setters would.
#[derive(Debug, Default, Deserialize)]
struct ProfileWire {
    #[serde(rename = "name", default)]
    name: Option<String>,
    #[serde(rename = "guiTheme", default)]
    gui_theme: Option<String>,
    #[serde(rename = "moduleListStyle", default)]
    module_list_style: Option<String>,
    #[serde(rename = "showLogo", default)]
    show_logo: Option<bool>,
    #[serde(rename = "devMode", default)]
    dev_mode: Option<bool>,
    #[serde(rename = "discordRpcEnabled", default)]
    discord_rpc_enabled: Option<bool>,
    #[serde(rename = "isArmed", default)]
    is_armed: Option<bool>,
    #[serde(rename = "minCPS", default)]
    min_cps: Option<f32>,
    #[serde(rename = "maxCPS", default)]
    max_cps: Option<f32>,
    #[serde(rename = "leftClickEnabled", default)]
    left_click_enabled: Option<bool>,
    #[serde(rename = "rightClickEnabled", default)]
    right_click_enabled: Option<bool>,
    #[serde(rename = "rightMinCPS", default)]
    right_min_cps: Option<f32>,
    #[serde(rename = "rightMaxCPS", default)]
    right_max_cps: Option<f32>,
    #[serde(rename = "rightClickOnlyBlock", default)]
    right_click_only_block: Option<bool>,
    #[serde(rename = "breakBlocksEnabled", default)]
    break_blocks_enabled: Option<bool>,
    #[serde(rename = "clickInChests", default)]
    click_in_chests: Option<bool>,
    #[serde(rename = "aimAssistEnabled", default)]
    aim_assist_enabled: Option<bool>,
    #[serde(rename = "aimAssistFov", default)]
    aim_assist_fov: Option<f32>,
    #[serde(rename = "aimAssistRange", default)]
    aim_assist_range: Option<f32>,
    #[serde(rename = "aimAssistStrength", default)]
    aim_assist_strength: Option<i32>,
    #[serde(rename = "triggerbotEnabled", default)]
    triggerbot_enabled: Option<bool>,
    #[serde(rename = "triggerbotOnlyCrosshair", default)]
    triggerbot_only_crosshair: Option<bool>,
    #[serde(rename = "triggerbotOnlyIfCanAttack", default)]
    triggerbot_only_if_can_attack: Option<bool>,
    #[serde(rename = "triggerbotCooldownThreshold", default)]
    triggerbot_cooldown_threshold: Option<i32>,
    #[serde(rename = "triggerbotHitChance", default)]
    triggerbot_hit_chance: Option<i32>,
    #[serde(rename = "triggerbotRequireClick", default)]
    triggerbot_require_click: Option<bool>,
    #[serde(rename = "killAuraEnabled", default)]
    kill_aura_enabled: Option<bool>,
    #[serde(rename = "killAuraSettings", default)]
    kill_aura_settings: Option<KillAuraSettings>,
    #[serde(rename = "killAuraRange", default)]
    kill_aura_range: Option<f32>,
    #[serde(rename = "killAuraAimRange", default)]
    kill_aura_aim_range: Option<f32>,
    #[serde(rename = "killAuraMinTurnSpeed", default)]
    kill_aura_min_turn_speed: Option<f32>,
    #[serde(rename = "killAuraMaxTurnSpeed", default)]
    kill_aura_max_turn_speed: Option<f32>,
    #[serde(rename = "killAuraTargetMode", default)]
    kill_aura_target_mode: Option<String>,
    #[serde(rename = "killAuraSwitchDelayMs", default)]
    kill_aura_switch_delay_ms: Option<i32>,
    #[serde(rename = "killAuraRandomization", default)]
    kill_aura_randomization: Option<i32>,
    #[serde(rename = "killAuraClickMode", default)]
    kill_aura_click_mode: Option<String>,
    #[serde(rename = "killAuraMinCps", default)]
    kill_aura_min_cps: Option<f32>,
    #[serde(rename = "killAuraMaxCps", default)]
    kill_aura_max_cps: Option<f32>,
    #[serde(rename = "killAuraFov", default)]
    kill_aura_fov: Option<i32>,
    #[serde(rename = "killAuraRotMode", default)]
    kill_aura_rot_mode: Option<String>,
    #[serde(rename = "killAuraRequirePress", default)]
    kill_aura_require_press: Option<bool>,
    #[serde(rename = "killAuraWeaponsOnly", default)]
    kill_aura_weapons_only: Option<bool>,
    #[serde(rename = "speedBridgeEnabled", default)]
    speed_bridge_enabled: Option<bool>,
    #[serde(rename = "speedBridgeBlockOnly", default)]
    speed_bridge_block_only: Option<bool>,
    #[serde(rename = "speedBridgeDelayMs", default)]
    speed_bridge_delay_ms: Option<i32>,
    #[serde(rename = "speedBridgeHoldingShiftOnly", default)]
    speed_bridge_holding_shift_only: Option<bool>,
    #[serde(rename = "speedBridgeLookingDownOnly", default)]
    speed_bridge_looking_down_only: Option<bool>,
    #[serde(rename = "gtbHelperEnabled", default)]
    gtb_helper_enabled: Option<bool>,
    #[serde(rename = "pixelPartyAssistEnabled", default)]
    pixel_party_assist_enabled: Option<bool>,
    #[serde(rename = "pixelPartyScanRadius", default)]
    pixel_party_scan_radius: Option<i32>,
    #[serde(rename = "pixelPartyAutoLookEnabled", default)]
    pixel_party_auto_look_enabled: Option<bool>,
    #[serde(rename = "pixelPartyAutoWalkEnabled", default)]
    pixel_party_auto_walk_enabled: Option<bool>,
    #[serde(rename = "nametagsEnabled", default)]
    nametags_enabled: Option<bool>,
    #[serde(rename = "nickHiderEnabled", default)]
    nick_hider_enabled: Option<bool>,
    #[serde(rename = "nickHiderAlias", default)]
    nick_hider_alias: Option<String>,
    #[serde(rename = "showModuleList", default)]
    show_module_list: Option<bool>,
    #[serde(rename = "closestPlayerInfoEnabled", default)]
    closest_player_info_enabled: Option<bool>,
    #[serde(rename = "fightStatusEnabled", default)]
    fight_status_enabled: Option<bool>,
    #[serde(rename = "nametagShowHealth", default)]
    nametag_show_health: Option<bool>,
    #[serde(rename = "nametagShowArmor", default)]
    nametag_show_armor: Option<bool>,
    #[serde(rename = "nametagShowHeldItem", default)]
    nametag_show_held_item: Option<bool>,
    #[serde(rename = "nametagHideVanilla", default)]
    nametag_hide_vanilla: Option<bool>,
    #[serde(rename = "nametagRange", default)]
    nametag_range: Option<i32>,
    #[serde(rename = "chestEspEnabled", default)]
    chest_esp_enabled: Option<bool>,
    #[serde(rename = "chestEspRange", default)]
    chest_esp_range: Option<i32>,
    #[serde(rename = "chestStealerEnabled", default)]
    chest_stealer_enabled: Option<bool>,
    #[serde(rename = "chestStealerDelayMs", default)]
    chest_stealer_delay_ms: Option<i32>,
    #[serde(rename = "refillEnabled", default)]
    refill_enabled: Option<bool>,
    #[serde(rename = "refillDelayMs", default)]
    refill_delay_ms: Option<i32>,
    #[serde(rename = "blockEspEnabled", default)]
    block_esp_enabled: Option<bool>,
    #[serde(rename = "blockEspBoxes", default)]
    block_esp_boxes: Option<bool>,
    #[serde(rename = "blockEspTracers", default)]
    block_esp_tracers: Option<bool>,
    #[serde(rename = "blockEspHud", default)]
    block_esp_hud: Option<bool>,
    #[serde(rename = "blockEspMaxCount", default)]
    block_esp_max_count: Option<i32>,
    #[serde(rename = "blockEspRange", default)]
    block_esp_range: Option<i32>,
    #[serde(rename = "blockEspTargets", default)]
    block_esp_targets: Option<Vec<BlockEspTargetData>>,
    #[serde(rename = "bedPlatesEnabled", default)]
    bed_plates_enabled: Option<bool>,
    #[serde(rename = "bedPlatesShowDistance", default)]
    bed_plates_show_distance: Option<bool>,
    #[serde(rename = "bedPlatesRange", default)]
    bed_plates_range: Option<i32>,
    #[serde(rename = "reachEnabled", default)]
    reach_enabled: Option<bool>,
    #[serde(rename = "reachMin", default)]
    reach_min: Option<f32>,
    #[serde(rename = "reachMax", default)]
    reach_max: Option<f32>,
    #[serde(rename = "reachChance", default)]
    reach_chance: Option<i32>,
    #[serde(rename = "reachChanceMode", default)]
    reach_chance_mode: Option<i32>,
    #[serde(rename = "reachOnlyWhileSprinting", default)]
    reach_only_while_sprinting: Option<bool>,
    #[serde(rename = "reachDisableInWater", default)]
    reach_disable_in_water: Option<bool>,
    #[serde(rename = "reachVerticalCheck", default)]
    reach_vertical_check: Option<bool>,
    #[serde(rename = "velocityEnabled", default)]
    velocity_enabled: Option<bool>,
    #[serde(rename = "velocityHorizontal", default)]
    velocity_horizontal: Option<i32>,
    #[serde(rename = "velocityVertical", default)]
    velocity_vertical: Option<i32>,
    #[serde(rename = "velocityChance", default)]
    velocity_chance: Option<i32>,
    #[serde(rename = "autoTotemEnabled", default)]
    auto_totem_enabled: Option<bool>,
    #[serde(rename = "autoTotemMode", default)]
    auto_totem_mode: Option<i32>,
    #[serde(rename = "autoTotemHealth", default)]
    auto_totem_health: Option<i32>,
    #[serde(rename = "autoTotemElytra", default)]
    auto_totem_elytra: Option<bool>,
    #[serde(rename = "autoTotemDelay", default)]
    auto_totem_delay: Option<i32>,
    #[serde(rename = "autoTotemBehaviorMode", default)]
    auto_totem_behavior_mode: Option<i32>,
    #[serde(rename = "autoRodEnabled", default)]
    auto_rod_enabled: Option<bool>,
    #[serde(rename = "throwpotEnabled", default)]
    throwpot_enabled: Option<bool>,
    #[serde(rename = "autoHealEnabled", default)]
    auto_heal_enabled: Option<bool>,
    #[serde(rename = "autoHealHealth", default)]
    auto_heal_health: Option<i32>,
    #[serde(rename = "autoHealDelayMs", default)]
    auto_heal_delay_ms: Option<i32>,
    #[serde(rename = "autoRodVerifyForcedSlot", default)]
    auto_rod_verify_forced_slot: Option<bool>,
    #[serde(rename = "autoRodHoldToExtend", default)]
    auto_rod_hold_to_extend: Option<bool>,
    #[serde(rename = "autoRodActionKey", default)]
    auto_rod_action_key: Option<i32>,
    #[serde(rename = "throwpotActionKey", default)]
    throwpot_action_key: Option<i32>,
    #[serde(rename = "autoToolEnabled", default)]
    auto_tool_enabled: Option<bool>,
    #[serde(rename = "autoToolSwapWeapon", default)]
    auto_tool_swap_weapon: Option<bool>,
    #[serde(rename = "autoToolInstantSwap", default)]
    auto_tool_instant_swap: Option<bool>,
    #[serde(rename = "autoToolSwapToDelay", default)]
    auto_tool_swap_to_delay: Option<i32>,
    #[serde(rename = "autoToolSwapBack", default)]
    auto_tool_swap_back: Option<bool>,
    #[serde(rename = "autoToolSwapBackDelay", default)]
    auto_tool_swap_back_delay: Option<i32>,
    #[serde(rename = "autoToolRequireMouseDown", default)]
    auto_tool_require_mouse_down: Option<bool>,
    #[serde(rename = "autoToolOnlySneaking", default)]
    auto_tool_only_sneaking: Option<bool>,
    #[serde(rename = "autoToolBedwarsMode", default)]
    auto_tool_bedwars_mode: Option<bool>,
    #[serde(rename = "antiDebuffEnabled", default)]
    anti_debuff_enabled: Option<bool>,
    #[serde(rename = "hitDelayFixEnabled", default)]
    hit_delay_fix_enabled: Option<bool>,
    #[serde(rename = "fireballWarningEnabled", default)]
    fireball_warning_enabled: Option<bool>,
    #[serde(rename = "fireballWarningBox", default)]
    fireball_warning_box: Option<bool>,
    #[serde(rename = "fireballWarningArrow", default)]
    fireball_warning_arrow: Option<bool>,
    #[serde(rename = "fireballWarningSound", default)]
    fireball_warning_sound: Option<bool>,
    #[serde(rename = "fastPlaceEnabled", default)]
    fast_place_enabled: Option<bool>,
    #[serde(rename = "noHurtCamEnabled", default)]
    no_hurt_cam_enabled: Option<bool>,
    #[serde(rename = "sprintEnabled", default)]
    sprint_enabled: Option<bool>,
    #[serde(rename = "tntTimerEnabled", default)]
    tnt_timer_enabled: Option<bool>,
    #[serde(rename = "wTapEnabled", default)]
    wtap_enabled: Option<bool>,
    #[serde(rename = "jumpResetEnabled", default)]
    jump_reset_enabled: Option<bool>,
    #[serde(rename = "hitSelectEnabled", default)]
    hit_select_enabled: Option<bool>,
    #[serde(rename = "trajectoriesEnabled", default)]
    trajectories_enabled: Option<bool>,
    #[serde(rename = "murderMysteryEnabled", default)]
    murder_mystery_enabled: Option<bool>,
    #[serde(rename = "breakProgressEnabled", default)]
    break_progress_enabled: Option<bool>,
    #[serde(rename = "jumpResetChance", default)]
    jump_reset_chance: Option<i32>,
    #[serde(rename = "hudLayout", default)]
    hud_layout: Option<Option<BTreeMap<String, HudElementLayoutValue>>>,
    #[serde(rename = "moduleKeys", default)]
    module_keys: Option<BTreeMap<String, i32>>,
    #[serde(rename = "theme", default)]
    theme: Option<String>,
    #[serde(rename = "randomizationMode", default)]
    randomization_mode: Option<ClickRandomizationMode>,
    #[serde(rename = "jitterEnabled", default)]
    jitter_enabled: Option<bool>,
    #[serde(rename = "chestStealerMenuCheck", default)]
    chest_stealer_menu_check: Option<bool>,
    #[serde(rename = "chestStealerTitleCheck", default)]
    chest_stealer_title_check: Option<bool>,
    #[serde(rename = "chestStealerCustomItemsCheck", default)]
    chest_stealer_custom_items_check: Option<bool>,
    #[serde(rename = "chestStealerPhysicalCheck", default)]
    chest_stealer_physical_check: Option<bool>,
    #[serde(rename = "autoRodSlotMode", default)]
    auto_rod_slot_mode: Option<i32>,
    #[serde(rename = "autoRodExtensionTicks", default)]
    auto_rod_extension_ticks: Option<i32>,
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

impl<'de> Deserialize<'de> for Profile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Profile::from(ProfileWire::deserialize(deserializer)?))
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

/// Serialises a profile the way `ProfileManager.JsonOptions` does: camelCase keys
/// and indented output.
#[must_use]
pub fn to_profile_json(profile: &Profile) -> String {
    serde_json::to_string_pretty(profile).unwrap_or_default()
}

/// Deserialises a profile and runs the one-way migrations.
///
/// Returns `None` when the document is not a JSON object or does not fit the
/// schema at all, matching `LoadProfile`'s catch-all.
#[must_use]
pub fn load_profile_from_json(json: &str) -> Option<Profile> {
    let root: Value = serde_json::from_str(json).ok()?;
    if !root.is_object() {
        return None;
    }

    let mut profile: Profile = Profile::from(ProfileWire::deserialize(&root).ok()?);
    migrate_silent_aura_to_kill_aura(&root, &mut profile);
    migrate_flat_kill_aura_settings(&root, &mut profile);
    Some(profile)
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
