//! The outbound `config` frame and the `moduleAction` frames.
//!
//! Ported from the payload builders in `Aoko/Core/GameStateClient.cs`.
//!
//! Object key order is **not** part of the wire contract (`ParseConfig` on both
//! bridges is key-based), but the framing is: one JSON document per line, LF
//! terminated, every scalar key always present, `hudLayout` always present.
//! Writing the line by hand rather than through `serde` is deliberate — a
//! `serde_json` float prints `8.0` where `System.Text.Json` prints `8`, and the
//! managed writer is the byte-for-byte reference.
//!
//! The payload is a *shape*: `aoko-core` fills every field from `Clicker`, and
//! this crate deliberately does not duplicate those defaults.

use crate::hud::HudLayout;

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigPayload {
    /// `_transportDiagnostics.Enabled`
    pub perf_diagnostics: bool,
    /// `clicker.IsArmed`
    pub armed: bool,
    /// `clicker.IsClicking`
    pub clicking: bool,
    /// `clicker.MinCPS`
    pub min_cps: f32,
    /// `clicker.MaxCPS`
    pub max_cps: f32,
    /// `clicker.LeftClickEnabled`
    pub left: bool,
    /// `clicker.RightClickEnabled`
    pub right: bool,
    /// `clicker.RightMinCPS`
    pub right_min_cps: f32,
    /// `clicker.RightMaxCPS`
    pub right_max_cps: f32,
    /// `clicker.RightClickOnlyBlock`
    pub right_block: bool,
    /// `clicker.BreakBlocksEnabled`
    pub break_blocks: bool,
    /// `clicker.JitterEnabled`
    pub jitter: bool,
    /// `(int)clicker.RandomizationMode`
    pub randomization_mode: i32,
    /// `clicker.ClickInChests`
    pub click_in_chests: bool,
    /// `clicker.AimAssistEnabled`
    pub aim_assist: bool,
    /// `clicker.AimAssistFov`
    pub aim_assist_fov: f32,
    /// `clicker.AimAssistRange`
    pub aim_assist_range: f32,
    /// `clicker.AimAssistStrength`
    pub aim_assist_strength: i32,
    /// `clicker.TriggerbotEnabled`
    pub triggerbot: bool,
    /// `clicker.KillAuraEnabled`
    pub kill_aura: bool,
    /// `ka.CpsMode`
    pub kill_aura_cps_mode: String,
    /// `ka.Mode`
    pub kill_aura_mode: String,
    /// `ka.Sort`
    pub kill_aura_sort: String,
    /// `ka.AutoBlock`
    pub kill_aura_auto_block: String,
    /// `ka.AttackTick`
    pub kill_aura_attack_tick: i32,
    /// `ka.AutoBlockRequirePress`
    pub kill_aura_auto_block_require_press: bool,
    /// `ka.AutoBlockCps`
    pub kill_aura_auto_block_cps: f32,
    /// `ka.AutoBlockRange`
    pub kill_aura_auto_block_range: f32,
    /// `ka.SwingRange`
    pub kill_aura_swing_range: f32,
    /// `ka.AttackRange`
    pub kill_aura_attack_range: f32,
    /// `ka.Fov`
    pub kill_aura_fov: i32,
    /// `ka.MinCps`
    pub kill_aura_min_cps: i32,
    /// `ka.MaxCps`
    pub kill_aura_max_cps: i32,
    /// `ka.SwitchDelay`
    pub kill_aura_switch_delay: i32,
    /// `ka.Rotations`
    pub kill_aura_rotations: String,
    /// `ka.DeadZoneSize`
    pub kill_aura_dead_zone: f32,
    /// `ka.MaxTurnSpeed`
    pub kill_aura_max_turn_speed: f32,
    /// `ka.MinTurnSpeed`
    pub kill_aura_min_turn_speed: f32,
    /// `ka.Acceleration`
    pub kill_aura_acceleration: f32,
    /// `ka.Deceleration`
    pub kill_aura_deceleration: f32,
    /// `ka.UseOvershoot`
    pub kill_aura_use_overshoot: bool,
    /// `ka.OvershootStrength`
    pub kill_aura_overshoot_strength: f32,
    /// `ka.OvershootRecovery`
    pub kill_aura_overshoot_recovery: f32,
    /// `ka.NoiseStrength`
    pub kill_aura_noise_strength: f32,
    /// `ka.VisualizeAim`
    pub kill_aura_visualize_aim: bool,
    /// `ka.SmoothBack`
    pub kill_aura_smooth_back: bool,
    /// `ka.MoveFix`
    pub kill_aura_move_fix: String,
    /// `ka.Smoothing`
    pub kill_aura_smoothing: i32,
    /// `ka.RavenSmoothing`
    pub kill_aura_raven_smoothing: i32,
    /// `ka.RavenPredictTicks`
    pub kill_aura_raven_predict_ticks: i32,
    /// `ka.RavenYawRandom`
    pub kill_aura_raven_yaw_random: i32,
    /// `ka.GrokMaxSkew`
    pub kill_aura_grok_max_skew: f32,
    /// `ka.AngleStep`
    pub kill_aura_angle_step: i32,
    /// `ka.ThroughWalls`
    pub kill_aura_through_walls: bool,
    /// `ka.RequirePress`
    pub kill_aura_require_press: bool,
    /// `ka.AllowMining`
    pub kill_aura_allow_mining: bool,
    /// `ka.WeaponsOnly`
    pub kill_aura_weapons_only: bool,
    /// `ka.AllowTools`
    pub kill_aura_allow_tools: bool,
    /// `ka.InventoryCheck`
    pub kill_aura_inventory_check: bool,
    /// `ka.BotCheck`
    pub kill_aura_bot_check: bool,
    /// `ka.Players`
    pub kill_aura_players: bool,
    /// `ka.Bosses`
    pub kill_aura_bosses: bool,
    /// `ka.Mobs`
    pub kill_aura_mobs: bool,
    /// `ka.Animals`
    pub kill_aura_animals: bool,
    /// `ka.Golems`
    pub kill_aura_golems: bool,
    /// `ka.Silverfish`
    pub kill_aura_silverfish: bool,
    /// `ka.Teams`
    pub kill_aura_teams: bool,
    /// `ka.ShowTarget`
    pub kill_aura_show_target: String,
    /// `ka.DebugLog`
    pub kill_aura_debug: String,
    /// `ka.Randomize`
    pub kill_aura_randomize: bool,
    /// `ka.RandomizeRange`
    pub kill_aura_randomize_range: f32,
    /// `ka.YRandomizeStrength`
    pub kill_aura_y_randomize: f32,
    /// `ka.LiquidBounceHorizontalSpeed`
    pub kill_aura_lb_horizontal_speed: f32,
    /// `ka.LiquidBounceVerticalSpeed`
    pub kill_aura_lb_vertical_speed: f32,
    /// `ka.LiquidBounceSmoothFactor`
    pub kill_aura_lb_smooth: f32,
    /// `ka.LiquidBouncePredict`
    pub kill_aura_lb_predict: bool,
    /// `ka.LiquidBouncePredictSize`
    pub kill_aura_lb_predict_size: f32,
    /// `ka.LiquidBounceRandomize`
    pub kill_aura_lb_randomize: bool,
    /// `ka.LiquidBounceRandomizeRange`
    pub kill_aura_lb_random_range: f32,
    /// `ka.LiquidBounceHorizontalSearch`
    pub kill_aura_lb_horizontal_search: f32,
    /// `ka.LiquidBounceBodyPointMin`
    pub kill_aura_lb_body_min: f32,
    /// `ka.LiquidBounceBodyPointMax`
    pub kill_aura_lb_body_max: f32,
    /// `clicker.SpeedBridgeEnabled`
    pub speed_bridge: bool,
    /// `clicker.SpeedBridgeBlockOnly`
    pub speed_bridge_block_only: bool,
    /// `clicker.SpeedBridgeDelayMs`
    pub speed_bridge_delay_ms: bool,
    /// `clicker.SpeedBridgeHoldingShiftOnly`
    pub speed_bridge_holding_shift_only: bool,
    /// `clicker.SpeedBridgeLookingDownOnly`
    pub speed_bridge_looking_down_only: bool,
    /// `clicker.GtbHelperEnabled`
    pub gtb_helper: bool,
    /// `clicker.PixelPartyAssistEnabled`
    pub pixel_party_assist: bool,
    /// `clicker.PixelPartyScanRadius`
    pub pixel_party_scan_radius: i32,
    /// `clicker.PixelPartyAutoLookEnabled`
    pub pixel_party_auto_look: bool,
    /// `clicker.PixelPartyAutoWalkEnabled`
    pub pixel_party_auto_walk: bool,
    /// `clicker.GtbCurrentHint`
    pub gtb_hint: String,
    /// `clicker.GtbMatchCount`
    pub gtb_count: i32,
    /// `clicker.GtbMatchesPreview`
    pub gtb_preview: String,
    /// `clicker.NametagsEnabled`
    pub nametags: bool,
    /// `clicker.IsNickHiderActive`
    pub nick_hider_enabled: bool,
    /// `NickHiderConfig.NormalizeAlias(clicker.NickHiderAlias)`
    pub nick_hider_alias: String,
    /// `clicker.ShowModuleList`
    pub show_module_list: bool,
    /// `ModuleListStyleToIndex(clicker.ModuleListStyle)`
    pub module_list_style: i32,
    /// `clicker.ShowLogo`
    pub show_logo: bool,
    /// `clicker.GuiTheme`
    pub gui_theme: String,
    /// `clicker.ClosestPlayerInfoEnabled`
    pub closest_player_info: bool,
    /// `clicker.FightStatusEnabled`
    pub fight_status: bool,
    /// `clicker.NametagShowHealth`
    pub nametag_show_health: bool,
    /// `clicker.NametagShowArmor`
    pub nametag_show_armor: bool,
    /// `clicker.NametagShowHeldItem`
    pub nametag_show_held_item: bool,
    /// `clicker.NametagHideVanilla`
    pub nametag_hide_vanilla: bool,
    /// `Volatile.Read(ref _reloadMappingsNonce)`
    pub reload_mappings_nonce: i32,
    /// `clicker.NametagRange`
    pub nametag_range: i32,
    /// `clicker.ChestEspEnabled`
    pub chest_esp: bool,
    /// `clicker.ChestEspRange`
    pub chest_esp_range: i32,
    /// `clicker.ChestStealerEnabled`
    pub chest_stealer_enabled: bool,
    /// `clicker.ChestStealerDelayMs`
    pub chest_stealer_delay_ms: i32,
    /// `clicker.ChestStealerMenuCheck`
    pub chest_stealer_menu_check: bool,
    /// `clicker.ChestStealerTitleCheck`
    pub chest_stealer_title_check: bool,
    /// `clicker.ChestStealerCustomItemsCheck`
    pub chest_stealer_custom_items_check: bool,
    /// `clicker.ChestStealerPhysicalCheck`
    pub chest_stealer_physical_check: bool,
    /// `clicker.RefillEnabled`
    pub refill_enabled: bool,
    /// `clicker.RefillDelayMs`
    pub refill_delay_ms: i32,
    /// `clicker.BlockEspEnabled`
    pub block_esp_enabled: bool,
    /// `clicker.BlockEspBoxes`
    pub block_esp_boxes: bool,
    /// `clicker.BlockEspTracers`
    pub block_esp_tracers: bool,
    /// `clicker.BlockEspHud`
    pub block_esp_hud: bool,
    /// `clicker.BlockEspMaxCount`
    pub block_esp_max_count: i32,
    /// `clicker.BlockEspRange`
    pub block_esp_range: i32,
    /// `clicker.BlockEspBlocksSerialized`
    pub block_esp_blocks: String,
    /// `clicker.BedPlatesEnabled`
    pub bed_plates_enabled: bool,
    /// `clicker.BedPlatesShowDistance`
    pub bed_plates_show_distance: bool,
    /// `clicker.BedPlatesRange`
    pub bed_plates_range: i32,
    /// `clicker.ReachEnabled`
    pub reach_enabled: bool,
    /// `clicker.ReachMin`
    pub reach_min: f32,
    /// `clicker.ReachMax`
    pub reach_max: f32,
    /// `clicker.ReachChance`
    pub reach_chance: i32,
    /// `clicker.ReachChanceMode`
    pub reach_chance_mode: i32,
    /// `clicker.ReachOnlyWhileSprinting`
    pub reach_only_while_sprinting: bool,
    /// `clicker.ReachDisableInWater`
    pub reach_disable_in_water: bool,
    /// `clicker.ReachVerticalCheck`
    pub reach_vertical_check: bool,
    /// `clicker.VelocityEnabled`
    pub velocity_enabled: bool,
    /// `clicker.VelocityHorizontal`
    pub velocity_horizontal: i32,
    /// `clicker.VelocityVertical`
    pub velocity_vertical: i32,
    /// `clicker.VelocityChance`
    pub velocity_chance: i32,
    /// `clicker.AutoTotemEnabled`
    pub auto_totem_enabled: bool,
    /// `clicker.AutoTotemMode`
    pub auto_totem_mode: i32,
    /// `clicker.AutoTotemHealth`
    pub auto_totem_health: i32,
    /// `clicker.AutoTotemElytra`
    pub auto_totem_elytra: bool,
    /// `clicker.AutoTotemDelay`
    pub auto_totem_delay: i32,
    /// `clicker.AutoTotemBehaviorMode`
    pub auto_totem_behavior_mode: i32,
    /// `clicker.AutoRodEnabled`
    pub auto_rod_enabled: bool,
    /// `clicker.AutoRodSlotMode`
    pub auto_rod_slot_mode: i32,
    /// `clicker.AutoRodVerifyForcedSlot`
    pub auto_rod_verify_forced_slot: bool,
    /// `clicker.AutoRodExtensionTicks`
    pub auto_rod_extension_ticks: i32,
    /// `clicker.AutoRodHoldToExtend`
    pub auto_rod_hold_to_extend: bool,
    /// `clicker.ThrowpotEnabled`
    pub throwpot_enabled: bool,
    /// `clicker.AutoHealEnabled`
    pub auto_heal_enabled: bool,
    /// `clicker.AutoHealHealth`
    pub auto_heal_health: i32,
    /// `clicker.AutoHealDelayMs`
    pub auto_heal_delay_ms: i32,
    /// `clicker.AutoToolEnabled`
    pub auto_tool_enabled: bool,
    /// `clicker.AutoToolSwapWeapon`
    pub auto_tool_swap_weapon: bool,
    /// `clicker.AutoToolInstantSwap`
    pub auto_tool_instant_swap: bool,
    /// `clicker.AutoToolSwapToDelay`
    pub auto_tool_swap_to_delay: i32,
    /// `clicker.AutoToolSwapBack`
    pub auto_tool_swap_back: bool,
    /// `clicker.AutoToolSwapBackDelay`
    pub auto_tool_swap_back_delay: i32,
    /// `clicker.AutoToolRequireMouseDown`
    pub auto_tool_require_mouse_down: bool,
    /// `clicker.AutoToolOnlySneaking`
    pub auto_tool_only_sneaking: bool,
    /// `clicker.AutoToolBedwarsMode`
    pub auto_tool_bedwars_mode: bool,
    /// `clicker.AntiDebuffEnabled`
    pub anti_debuff_enabled: bool,
    /// `clicker.HitDelayFixEnabled`
    pub hit_delay_fix_enabled: bool,
    /// `clicker.FireballWarningEnabled`
    pub fireball_warning: bool,
    /// `clicker.FireballWarningBox`
    pub fireball_warning_box: bool,
    /// `clicker.FireballWarningArrow`
    pub fireball_warning_arrow: bool,
    /// `clicker.FireballWarningSound`
    pub fireball_warning_sound: bool,
    /// `clicker.FastPlaceEnabled`
    pub fast_place: bool,
    /// `clicker.NoHurtCamEnabled`
    pub no_hurt_cam: bool,
    /// `clicker.SprintEnabled`
    pub sprint: bool,
    /// `clicker.TntTimerEnabled`
    pub tnt_timer: bool,
    /// `clicker.WTapEnabled`
    pub w_tap: bool,
    /// `clicker.JumpResetEnabled`
    pub jump_reset: bool,
    /// `clicker.HitSelectEnabled`
    pub hit_select: bool,
    /// `clicker.TrajectoriesEnabled`
    pub trajectories: bool,
    /// `clicker.MurderMysteryEnabled`
    pub murder_mystery: bool,
    /// `clicker.BreakProgressEnabled`
    pub break_progress: bool,
    /// `clicker.JumpResetChance`
    pub jump_reset_chance: i32,
    /// `InputHooks.GetModuleKey("autoclicker")`
    pub keybind_autoclicker: i32,
    /// `InputHooks.GetModuleKey("rightclick")`
    pub keybind_right_click: i32,
    /// `InputHooks.GetModuleKey("aimassist")`
    pub keybind_aim_assist: i32,
    /// `InputHooks.GetModuleKey("triggerbot")`
    pub keybind_triggerbot: i32,
    /// `InputHooks.GetModuleKey("killaura")`
    pub keybind_kill_aura: i32,
    /// `InputHooks.GetModuleKey("speedbridge")`
    pub keybind_speed_bridge: i32,
    /// `InputHooks.GetModuleKey("gtbhelper")`
    pub keybind_gtb_helper: i32,
    /// `InputHooks.GetModuleKey("nametags")`
    pub keybind_nametags: i32,
    /// `InputHooks.GetModuleKey("closestplayer")`
    pub keybind_closest_player: i32,
    /// `InputHooks.GetModuleKey("fightstatus")`
    pub keybind_fight_status: i32,
    /// `InputHooks.GetModuleKey("chestesp")`
    pub keybind_chest_esp: i32,
    /// `InputHooks.GetModuleKey("cheststealer")`
    pub keybind_chest_stealer: i32,
    /// `InputHooks.GetModuleKey("refill")`
    pub keybind_refill: i32,
    /// `InputHooks.GetModuleKey("blockesp")`
    pub keybind_block_esp: i32,
    /// `InputHooks.GetModuleKey("bedplates")`
    pub keybind_bed_plates: i32,
    /// `InputHooks.GetModuleKey("pixelpartyassist")`
    pub keybind_pixel_party_assist: i32,
    /// `InputHooks.GetModuleKey("autorod")`
    pub keybind_auto_rod: i32,
    /// `InputHooks.GetModuleKey("throwpot")`
    pub keybind_throwpot: i32,
    /// `InputHooks.GetModuleKey("autoheal")`
    pub keybind_autoheal: i32,
    /// `InputHooks.GetModuleKey("autotool")`
    pub keybind_auto_tool: i32,
    /// `InputHooks.GetModuleKey("fireballwarning")`
    pub keybind_fireball_warning: i32,
    /// `InputHooks.GetModuleKey("fastplace")`
    pub keybind_fast_place: i32,
    /// `InputHooks.GetModuleKey("nohurtcam")`
    pub keybind_no_hurt_cam: i32,
    /// `InputHooks.GetModuleKey("sprint")`
    pub keybind_sprint: i32,
    /// `InputHooks.GetModuleKey("tnttimer")`
    pub keybind_tnt_timer: i32,
    /// `InputHooks.GetModuleKey("wtap")`
    pub keybind_w_tap: i32,
    /// `InputHooks.GetModuleKey("jumpreset")`
    pub keybind_jump_reset: i32,
    /// `InputHooks.GetModuleKey("hitselect")`
    pub keybind_hit_select: i32,
    /// `InputHooks.GetModuleKey("trajectories")`
    pub keybind_trajectories: i32,
    /// `InputHooks.GetModuleKey("murdermystery")`
    pub keybind_murder_mystery: i32,
    /// `InputHooks.GetModuleKey("breakprogress")`
    pub keybind_break_progress: i32,
    /// `clicker.HudEditorActive`
    pub hud_editor: bool,
}

impl Default for ConfigPayload {
    /// The managed `Clicker` defaults.
    ///
    /// `GtbCurrentHint` and `GtbMatchesPreview` start as `"-"`, not `""`: the
    /// shipped UI renders a dash until the solver has something to show, and the
    /// bridge parses the config it is given rather than re-deriving a default.
    fn default() -> Self {
        Self {
            perf_diagnostics: Default::default(),
            armed: Default::default(),
            clicking: Default::default(),
            min_cps: Default::default(),
            max_cps: Default::default(),
            left: Default::default(),
            right: Default::default(),
            right_min_cps: Default::default(),
            right_max_cps: Default::default(),
            right_block: Default::default(),
            break_blocks: Default::default(),
            jitter: Default::default(),
            randomization_mode: Default::default(),
            click_in_chests: Default::default(),
            aim_assist: Default::default(),
            aim_assist_fov: Default::default(),
            aim_assist_range: Default::default(),
            aim_assist_strength: Default::default(),
            triggerbot: Default::default(),
            kill_aura: Default::default(),
            kill_aura_cps_mode: Default::default(),
            kill_aura_mode: Default::default(),
            kill_aura_sort: Default::default(),
            kill_aura_auto_block: Default::default(),
            kill_aura_attack_tick: Default::default(),
            kill_aura_auto_block_require_press: Default::default(),
            kill_aura_auto_block_cps: Default::default(),
            kill_aura_auto_block_range: Default::default(),
            kill_aura_swing_range: Default::default(),
            kill_aura_attack_range: Default::default(),
            kill_aura_fov: Default::default(),
            kill_aura_min_cps: Default::default(),
            kill_aura_max_cps: Default::default(),
            kill_aura_switch_delay: Default::default(),
            kill_aura_rotations: Default::default(),
            kill_aura_dead_zone: Default::default(),
            kill_aura_max_turn_speed: Default::default(),
            kill_aura_min_turn_speed: Default::default(),
            kill_aura_acceleration: Default::default(),
            kill_aura_deceleration: Default::default(),
            kill_aura_use_overshoot: Default::default(),
            kill_aura_overshoot_strength: Default::default(),
            kill_aura_overshoot_recovery: Default::default(),
            kill_aura_noise_strength: Default::default(),
            kill_aura_visualize_aim: Default::default(),
            kill_aura_smooth_back: Default::default(),
            kill_aura_move_fix: Default::default(),
            kill_aura_smoothing: Default::default(),
            kill_aura_raven_smoothing: Default::default(),
            kill_aura_raven_predict_ticks: Default::default(),
            kill_aura_raven_yaw_random: Default::default(),
            kill_aura_grok_max_skew: Default::default(),
            kill_aura_angle_step: Default::default(),
            kill_aura_through_walls: Default::default(),
            kill_aura_require_press: Default::default(),
            kill_aura_allow_mining: Default::default(),
            kill_aura_weapons_only: Default::default(),
            kill_aura_allow_tools: Default::default(),
            kill_aura_inventory_check: Default::default(),
            kill_aura_bot_check: Default::default(),
            kill_aura_players: Default::default(),
            kill_aura_bosses: Default::default(),
            kill_aura_mobs: Default::default(),
            kill_aura_animals: Default::default(),
            kill_aura_golems: Default::default(),
            kill_aura_silverfish: Default::default(),
            kill_aura_teams: Default::default(),
            kill_aura_show_target: Default::default(),
            kill_aura_debug: Default::default(),
            kill_aura_randomize: Default::default(),
            kill_aura_randomize_range: Default::default(),
            kill_aura_y_randomize: Default::default(),
            kill_aura_lb_horizontal_speed: Default::default(),
            kill_aura_lb_vertical_speed: Default::default(),
            kill_aura_lb_smooth: Default::default(),
            kill_aura_lb_predict: Default::default(),
            kill_aura_lb_predict_size: Default::default(),
            kill_aura_lb_randomize: Default::default(),
            kill_aura_lb_random_range: Default::default(),
            kill_aura_lb_horizontal_search: Default::default(),
            kill_aura_lb_body_min: Default::default(),
            kill_aura_lb_body_max: Default::default(),
            speed_bridge: Default::default(),
            speed_bridge_block_only: Default::default(),
            speed_bridge_delay_ms: Default::default(),
            speed_bridge_holding_shift_only: Default::default(),
            speed_bridge_looking_down_only: Default::default(),
            gtb_helper: Default::default(),
            pixel_party_assist: Default::default(),
            pixel_party_scan_radius: Default::default(),
            pixel_party_auto_look: Default::default(),
            pixel_party_auto_walk: Default::default(),
            gtb_hint: "-".to_owned(),
            gtb_count: Default::default(),
            gtb_preview: "-".to_owned(),
            nametags: Default::default(),
            nick_hider_enabled: Default::default(),
            nick_hider_alias: Default::default(),
            show_module_list: Default::default(),
            module_list_style: Default::default(),
            show_logo: Default::default(),
            gui_theme: Default::default(),
            closest_player_info: Default::default(),
            fight_status: Default::default(),
            nametag_show_health: Default::default(),
            nametag_show_armor: Default::default(),
            nametag_show_held_item: Default::default(),
            nametag_hide_vanilla: Default::default(),
            reload_mappings_nonce: Default::default(),
            nametag_range: Default::default(),
            chest_esp: Default::default(),
            chest_esp_range: Default::default(),
            chest_stealer_enabled: Default::default(),
            chest_stealer_delay_ms: Default::default(),
            chest_stealer_menu_check: Default::default(),
            chest_stealer_title_check: Default::default(),
            chest_stealer_custom_items_check: Default::default(),
            chest_stealer_physical_check: Default::default(),
            refill_enabled: Default::default(),
            refill_delay_ms: Default::default(),
            block_esp_enabled: Default::default(),
            block_esp_boxes: Default::default(),
            block_esp_tracers: Default::default(),
            block_esp_hud: Default::default(),
            block_esp_max_count: Default::default(),
            block_esp_range: Default::default(),
            block_esp_blocks: Default::default(),
            bed_plates_enabled: Default::default(),
            bed_plates_show_distance: Default::default(),
            bed_plates_range: Default::default(),
            reach_enabled: Default::default(),
            reach_min: Default::default(),
            reach_max: Default::default(),
            reach_chance: Default::default(),
            reach_chance_mode: Default::default(),
            reach_only_while_sprinting: Default::default(),
            reach_disable_in_water: Default::default(),
            reach_vertical_check: Default::default(),
            velocity_enabled: Default::default(),
            velocity_horizontal: Default::default(),
            velocity_vertical: Default::default(),
            velocity_chance: Default::default(),
            auto_totem_enabled: Default::default(),
            auto_totem_mode: Default::default(),
            auto_totem_health: Default::default(),
            auto_totem_elytra: Default::default(),
            auto_totem_delay: Default::default(),
            auto_totem_behavior_mode: Default::default(),
            auto_rod_enabled: Default::default(),
            auto_rod_slot_mode: Default::default(),
            auto_rod_verify_forced_slot: Default::default(),
            auto_rod_extension_ticks: Default::default(),
            auto_rod_hold_to_extend: Default::default(),
            throwpot_enabled: Default::default(),
            auto_heal_enabled: Default::default(),
            auto_heal_health: Default::default(),
            auto_heal_delay_ms: Default::default(),
            auto_tool_enabled: Default::default(),
            auto_tool_swap_weapon: Default::default(),
            auto_tool_instant_swap: Default::default(),
            auto_tool_swap_to_delay: Default::default(),
            auto_tool_swap_back: Default::default(),
            auto_tool_swap_back_delay: Default::default(),
            auto_tool_require_mouse_down: Default::default(),
            auto_tool_only_sneaking: Default::default(),
            auto_tool_bedwars_mode: Default::default(),
            anti_debuff_enabled: Default::default(),
            hit_delay_fix_enabled: Default::default(),
            fireball_warning: Default::default(),
            fireball_warning_box: Default::default(),
            fireball_warning_arrow: Default::default(),
            fireball_warning_sound: Default::default(),
            fast_place: Default::default(),
            no_hurt_cam: Default::default(),
            sprint: Default::default(),
            tnt_timer: Default::default(),
            w_tap: Default::default(),
            jump_reset: Default::default(),
            hit_select: Default::default(),
            trajectories: Default::default(),
            murder_mystery: Default::default(),
            break_progress: Default::default(),
            jump_reset_chance: Default::default(),
            keybind_autoclicker: Default::default(),
            keybind_right_click: Default::default(),
            keybind_aim_assist: Default::default(),
            keybind_triggerbot: Default::default(),
            keybind_kill_aura: Default::default(),
            keybind_speed_bridge: Default::default(),
            keybind_gtb_helper: Default::default(),
            keybind_nametags: Default::default(),
            keybind_closest_player: Default::default(),
            keybind_fight_status: Default::default(),
            keybind_chest_esp: Default::default(),
            keybind_chest_stealer: Default::default(),
            keybind_refill: Default::default(),
            keybind_block_esp: Default::default(),
            keybind_bed_plates: Default::default(),
            keybind_pixel_party_assist: Default::default(),
            keybind_auto_rod: Default::default(),
            keybind_throwpot: Default::default(),
            keybind_autoheal: Default::default(),
            keybind_auto_tool: Default::default(),
            keybind_fireball_warning: Default::default(),
            keybind_fast_place: Default::default(),
            keybind_no_hurt_cam: Default::default(),
            keybind_sprint: Default::default(),
            keybind_tnt_timer: Default::default(),
            keybind_w_tap: Default::default(),
            keybind_jump_reset: Default::default(),
            keybind_hit_select: Default::default(),
            keybind_trajectories: Default::default(),
            keybind_murder_mystery: Default::default(),
            keybind_break_progress: Default::default(),
            hud_editor: Default::default(),
        }
    }
}

impl ConfigPayload {
    /// Emits the full `config` line, LF-terminated.
    #[must_use]
    pub fn to_line(&self, hud_layout: &HudLayout) -> String {
        let mut out = String::with_capacity(4096);
        out.push_str("{\"type\":\"config\"");
        self.push_bool(&mut out, "perfDiagnostics", self.perf_diagnostics);
        self.push_bool(&mut out, "armed", self.armed);
        self.push_bool(&mut out, "clicking", self.clicking);
        self.push_float(&mut out, "minCPS", self.min_cps);
        self.push_float(&mut out, "maxCPS", self.max_cps);
        self.push_bool(&mut out, "left", self.left);
        self.push_bool(&mut out, "right", self.right);
        self.push_float(&mut out, "rightMinCPS", self.right_min_cps);
        self.push_float(&mut out, "rightMaxCPS", self.right_max_cps);
        self.push_bool(&mut out, "rightBlock", self.right_block);
        self.push_bool(&mut out, "breakBlocks", self.break_blocks);
        self.push_bool(&mut out, "jitter", self.jitter);
        self.push_int(&mut out, "randomizationMode", self.randomization_mode);
        self.push_bool(&mut out, "clickInChests", self.click_in_chests);
        self.push_bool(&mut out, "aimAssist", self.aim_assist);
        self.push_float(&mut out, "aimAssistFov", self.aim_assist_fov);
        self.push_float(&mut out, "aimAssistRange", self.aim_assist_range);
        self.push_int(&mut out, "aimAssistStrength", self.aim_assist_strength);
        self.push_bool(&mut out, "triggerbot", self.triggerbot);
        self.push_bool(&mut out, "killAura", self.kill_aura);
        self.push_text(&mut out, "killAuraCpsMode", &self.kill_aura_cps_mode);
        self.push_text(&mut out, "killAuraMode", &self.kill_aura_mode);
        self.push_text(&mut out, "killAuraSort", &self.kill_aura_sort);
        self.push_text(&mut out, "killAuraAutoBlock", &self.kill_aura_auto_block);
        self.push_int(&mut out, "killAuraAttackTick", self.kill_aura_attack_tick);
        self.push_bool(
            &mut out,
            "killAuraAutoBlockRequirePress",
            self.kill_aura_auto_block_require_press,
        );
        self.push_float(
            &mut out,
            "killAuraAutoBlockCps",
            self.kill_aura_auto_block_cps,
        );
        self.push_float(
            &mut out,
            "killAuraAutoBlockRange",
            self.kill_aura_auto_block_range,
        );
        self.push_float(&mut out, "killAuraSwingRange", self.kill_aura_swing_range);
        self.push_float(&mut out, "killAuraAttackRange", self.kill_aura_attack_range);
        self.push_int(&mut out, "killAuraFov", self.kill_aura_fov);
        self.push_int(&mut out, "killAuraMinCps", self.kill_aura_min_cps);
        self.push_int(&mut out, "killAuraMaxCps", self.kill_aura_max_cps);
        self.push_int(&mut out, "killAuraSwitchDelay", self.kill_aura_switch_delay);
        self.push_text(&mut out, "killAuraRotations", &self.kill_aura_rotations);
        self.push_float(&mut out, "killAuraDeadZone", self.kill_aura_dead_zone);
        self.push_float(
            &mut out,
            "killAuraMaxTurnSpeed",
            self.kill_aura_max_turn_speed,
        );
        self.push_float(
            &mut out,
            "killAuraMinTurnSpeed",
            self.kill_aura_min_turn_speed,
        );
        self.push_float(
            &mut out,
            "killAuraAcceleration",
            self.kill_aura_acceleration,
        );
        self.push_float(
            &mut out,
            "killAuraDeceleration",
            self.kill_aura_deceleration,
        );
        self.push_bool(
            &mut out,
            "killAuraUseOvershoot",
            self.kill_aura_use_overshoot,
        );
        self.push_float(
            &mut out,
            "killAuraOvershootStrength",
            self.kill_aura_overshoot_strength,
        );
        self.push_float(
            &mut out,
            "killAuraOvershootRecovery",
            self.kill_aura_overshoot_recovery,
        );
        self.push_float(
            &mut out,
            "killAuraNoiseStrength",
            self.kill_aura_noise_strength,
        );
        self.push_bool(
            &mut out,
            "killAuraVisualizeAim",
            self.kill_aura_visualize_aim,
        );
        self.push_bool(&mut out, "killAuraSmoothBack", self.kill_aura_smooth_back);
        self.push_text(&mut out, "killAuraMoveFix", &self.kill_aura_move_fix);
        self.push_int(&mut out, "killAuraSmoothing", self.kill_aura_smoothing);
        self.push_int(
            &mut out,
            "killAuraRavenSmoothing",
            self.kill_aura_raven_smoothing,
        );
        self.push_int(
            &mut out,
            "killAuraRavenPredictTicks",
            self.kill_aura_raven_predict_ticks,
        );
        self.push_int(
            &mut out,
            "killAuraRavenYawRandom",
            self.kill_aura_raven_yaw_random,
        );
        self.push_float(
            &mut out,
            "killAuraGrokMaxSkew",
            self.kill_aura_grok_max_skew,
        );
        self.push_int(&mut out, "killAuraAngleStep", self.kill_aura_angle_step);
        self.push_bool(
            &mut out,
            "killAuraThroughWalls",
            self.kill_aura_through_walls,
        );
        self.push_bool(
            &mut out,
            "killAuraRequirePress",
            self.kill_aura_require_press,
        );
        self.push_bool(&mut out, "killAuraAllowMining", self.kill_aura_allow_mining);
        self.push_bool(&mut out, "killAuraWeaponsOnly", self.kill_aura_weapons_only);
        self.push_bool(&mut out, "killAuraAllowTools", self.kill_aura_allow_tools);
        self.push_bool(
            &mut out,
            "killAuraInventoryCheck",
            self.kill_aura_inventory_check,
        );
        self.push_bool(&mut out, "killAuraBotCheck", self.kill_aura_bot_check);
        self.push_bool(&mut out, "killAuraPlayers", self.kill_aura_players);
        self.push_bool(&mut out, "killAuraBosses", self.kill_aura_bosses);
        self.push_bool(&mut out, "killAuraMobs", self.kill_aura_mobs);
        self.push_bool(&mut out, "killAuraAnimals", self.kill_aura_animals);
        self.push_bool(&mut out, "killAuraGolems", self.kill_aura_golems);
        self.push_bool(&mut out, "killAuraSilverfish", self.kill_aura_silverfish);
        self.push_bool(&mut out, "killAuraTeams", self.kill_aura_teams);
        self.push_text(&mut out, "killAuraShowTarget", &self.kill_aura_show_target);
        self.push_text(&mut out, "killAuraDebug", &self.kill_aura_debug);
        self.push_bool(&mut out, "killAuraRandomize", self.kill_aura_randomize);
        self.push_float(
            &mut out,
            "killAuraRandomizeRange",
            self.kill_aura_randomize_range,
        );
        self.push_float(&mut out, "killAuraYRandomize", self.kill_aura_y_randomize);
        self.push_float(
            &mut out,
            "killAuraLbHorizontalSpeed",
            self.kill_aura_lb_horizontal_speed,
        );
        self.push_float(
            &mut out,
            "killAuraLbVerticalSpeed",
            self.kill_aura_lb_vertical_speed,
        );
        self.push_float(&mut out, "killAuraLbSmooth", self.kill_aura_lb_smooth);
        self.push_bool(&mut out, "killAuraLbPredict", self.kill_aura_lb_predict);
        self.push_float(
            &mut out,
            "killAuraLbPredictSize",
            self.kill_aura_lb_predict_size,
        );
        self.push_bool(&mut out, "killAuraLbRandomize", self.kill_aura_lb_randomize);
        self.push_float(
            &mut out,
            "killAuraLbRandomRange",
            self.kill_aura_lb_random_range,
        );
        self.push_float(
            &mut out,
            "killAuraLbHorizontalSearch",
            self.kill_aura_lb_horizontal_search,
        );
        self.push_float(&mut out, "killAuraLbBodyMin", self.kill_aura_lb_body_min);
        self.push_float(&mut out, "killAuraLbBodyMax", self.kill_aura_lb_body_max);
        self.push_bool(&mut out, "speedBridge", self.speed_bridge);
        self.push_bool(
            &mut out,
            "speedBridgeBlockOnly",
            self.speed_bridge_block_only,
        );
        self.push_bool(&mut out, "speedBridgeDelayMs", self.speed_bridge_delay_ms);
        self.push_bool(
            &mut out,
            "speedBridgeHoldingShiftOnly",
            self.speed_bridge_holding_shift_only,
        );
        self.push_bool(
            &mut out,
            "speedBridgeLookingDownOnly",
            self.speed_bridge_looking_down_only,
        );
        self.push_bool(&mut out, "gtbHelper", self.gtb_helper);
        self.push_bool(&mut out, "pixelPartyAssist", self.pixel_party_assist);
        self.push_int(
            &mut out,
            "pixelPartyScanRadius",
            self.pixel_party_scan_radius,
        );
        self.push_bool(&mut out, "pixelPartyAutoLook", self.pixel_party_auto_look);
        self.push_bool(&mut out, "pixelPartyAutoWalk", self.pixel_party_auto_walk);
        self.push_text(&mut out, "gtbHint", &self.gtb_hint);
        self.push_int(&mut out, "gtbCount", self.gtb_count);
        self.push_text(&mut out, "gtbPreview", &self.gtb_preview);
        self.push_bool(&mut out, "nametags", self.nametags);
        self.push_bool(&mut out, "nickHiderEnabled", self.nick_hider_enabled);
        self.push_text(&mut out, "nickHiderAlias", &self.nick_hider_alias);
        self.push_bool(&mut out, "showModuleList", self.show_module_list);
        self.push_int(&mut out, "moduleListStyle", self.module_list_style);
        self.push_bool(&mut out, "showLogo", self.show_logo);
        self.push_text(&mut out, "guiTheme", &self.gui_theme);
        out.push_str(",\"supportsStatePatches\":true");
        self.push_bool(&mut out, "closestPlayerInfo", self.closest_player_info);
        self.push_bool(&mut out, "fightStatus", self.fight_status);
        self.push_bool(&mut out, "nametagShowHealth", self.nametag_show_health);
        self.push_bool(&mut out, "nametagShowArmor", self.nametag_show_armor);
        self.push_bool(&mut out, "nametagShowHeldItem", self.nametag_show_held_item);
        self.push_bool(&mut out, "nametagHideVanilla", self.nametag_hide_vanilla);
        self.push_int(&mut out, "reloadMappingsNonce", self.reload_mappings_nonce);
        self.push_int(&mut out, "nametagRange", self.nametag_range);
        self.push_bool(&mut out, "chestEsp", self.chest_esp);
        self.push_int(&mut out, "chestEspRange", self.chest_esp_range);
        self.push_bool(&mut out, "chestStealerEnabled", self.chest_stealer_enabled);
        self.push_int(&mut out, "chestStealerDelayMs", self.chest_stealer_delay_ms);
        self.push_bool(
            &mut out,
            "chestStealerMenuCheck",
            self.chest_stealer_menu_check,
        );
        self.push_bool(
            &mut out,
            "chestStealerTitleCheck",
            self.chest_stealer_title_check,
        );
        self.push_bool(
            &mut out,
            "chestStealerCustomItemsCheck",
            self.chest_stealer_custom_items_check,
        );
        self.push_bool(
            &mut out,
            "chestStealerPhysicalCheck",
            self.chest_stealer_physical_check,
        );
        self.push_bool(&mut out, "refillEnabled", self.refill_enabled);
        self.push_int(&mut out, "refillDelayMs", self.refill_delay_ms);
        self.push_bool(&mut out, "blockEspEnabled", self.block_esp_enabled);
        self.push_bool(&mut out, "blockEspBoxes", self.block_esp_boxes);
        self.push_bool(&mut out, "blockEspTracers", self.block_esp_tracers);
        self.push_bool(&mut out, "blockEspHud", self.block_esp_hud);
        self.push_int(&mut out, "blockEspMaxCount", self.block_esp_max_count);
        self.push_int(&mut out, "blockEspRange", self.block_esp_range);
        self.push_text(&mut out, "blockEspBlocks", &self.block_esp_blocks);
        self.push_bool(&mut out, "bedPlatesEnabled", self.bed_plates_enabled);
        self.push_bool(
            &mut out,
            "bedPlatesShowDistance",
            self.bed_plates_show_distance,
        );
        self.push_int(&mut out, "bedPlatesRange", self.bed_plates_range);
        self.push_bool(&mut out, "reachEnabled", self.reach_enabled);
        self.push_float(&mut out, "reachMin", self.reach_min);
        self.push_float(&mut out, "reachMax", self.reach_max);
        self.push_int(&mut out, "reachChance", self.reach_chance);
        self.push_int(&mut out, "reachChanceMode", self.reach_chance_mode);
        self.push_bool(
            &mut out,
            "reachOnlyWhileSprinting",
            self.reach_only_while_sprinting,
        );
        self.push_bool(&mut out, "reachDisableInWater", self.reach_disable_in_water);
        self.push_bool(&mut out, "reachVerticalCheck", self.reach_vertical_check);
        self.push_bool(&mut out, "velocityEnabled", self.velocity_enabled);
        self.push_int(&mut out, "velocityHorizontal", self.velocity_horizontal);
        self.push_int(&mut out, "velocityVertical", self.velocity_vertical);
        self.push_int(&mut out, "velocityChance", self.velocity_chance);
        self.push_bool(&mut out, "autoTotemEnabled", self.auto_totem_enabled);
        self.push_int(&mut out, "autoTotemMode", self.auto_totem_mode);
        self.push_int(&mut out, "autoTotemHealth", self.auto_totem_health);
        self.push_bool(&mut out, "autoTotemElytra", self.auto_totem_elytra);
        self.push_int(&mut out, "autoTotemDelay", self.auto_totem_delay);
        self.push_int(
            &mut out,
            "autoTotemBehaviorMode",
            self.auto_totem_behavior_mode,
        );
        self.push_bool(&mut out, "autoRodEnabled", self.auto_rod_enabled);
        self.push_int(&mut out, "autoRodSlotMode", self.auto_rod_slot_mode);
        self.push_bool(
            &mut out,
            "autoRodVerifyForcedSlot",
            self.auto_rod_verify_forced_slot,
        );
        self.push_int(
            &mut out,
            "autoRodExtensionTicks",
            self.auto_rod_extension_ticks,
        );
        self.push_bool(
            &mut out,
            "autoRodHoldToExtend",
            self.auto_rod_hold_to_extend,
        );
        self.push_bool(&mut out, "throwpotEnabled", self.throwpot_enabled);
        self.push_bool(&mut out, "autoHealEnabled", self.auto_heal_enabled);
        self.push_int(&mut out, "autoHealHealth", self.auto_heal_health);
        self.push_int(&mut out, "autoHealDelayMs", self.auto_heal_delay_ms);
        self.push_bool(&mut out, "autoToolEnabled", self.auto_tool_enabled);
        self.push_bool(&mut out, "autoToolSwapWeapon", self.auto_tool_swap_weapon);
        self.push_bool(&mut out, "autoToolInstantSwap", self.auto_tool_instant_swap);
        self.push_int(
            &mut out,
            "autoToolSwapToDelay",
            self.auto_tool_swap_to_delay,
        );
        self.push_bool(&mut out, "autoToolSwapBack", self.auto_tool_swap_back);
        self.push_int(
            &mut out,
            "autoToolSwapBackDelay",
            self.auto_tool_swap_back_delay,
        );
        self.push_bool(
            &mut out,
            "autoToolRequireMouseDown",
            self.auto_tool_require_mouse_down,
        );
        self.push_bool(
            &mut out,
            "autoToolOnlySneaking",
            self.auto_tool_only_sneaking,
        );
        self.push_bool(&mut out, "autoToolBedwarsMode", self.auto_tool_bedwars_mode);
        self.push_bool(&mut out, "antiDebuffEnabled", self.anti_debuff_enabled);
        self.push_bool(&mut out, "hitDelayFixEnabled", self.hit_delay_fix_enabled);
        self.push_bool(&mut out, "fireballWarning", self.fireball_warning);
        self.push_bool(&mut out, "fireballWarningBox", self.fireball_warning_box);
        self.push_bool(
            &mut out,
            "fireballWarningArrow",
            self.fireball_warning_arrow,
        );
        self.push_bool(
            &mut out,
            "fireballWarningSound",
            self.fireball_warning_sound,
        );
        self.push_bool(&mut out, "fastPlace", self.fast_place);
        self.push_bool(&mut out, "noHurtCam", self.no_hurt_cam);
        self.push_bool(&mut out, "sprint", self.sprint);
        self.push_bool(&mut out, "tntTimer", self.tnt_timer);
        self.push_bool(&mut out, "wTap", self.w_tap);
        self.push_bool(&mut out, "jumpReset", self.jump_reset);
        self.push_bool(&mut out, "hitSelect", self.hit_select);
        self.push_bool(&mut out, "trajectories", self.trajectories);
        self.push_bool(&mut out, "murderMystery", self.murder_mystery);
        self.push_bool(&mut out, "breakProgress", self.break_progress);
        self.push_int(&mut out, "jumpResetChance", self.jump_reset_chance);
        self.push_int(&mut out, "keybindAutoclicker", self.keybind_autoclicker);
        self.push_int(&mut out, "keybindRightClick", self.keybind_right_click);
        self.push_int(&mut out, "keybindAimAssist", self.keybind_aim_assist);
        self.push_int(&mut out, "keybindTriggerbot", self.keybind_triggerbot);
        self.push_int(&mut out, "keybindKillAura", self.keybind_kill_aura);
        self.push_int(&mut out, "keybindSpeedBridge", self.keybind_speed_bridge);
        self.push_int(&mut out, "keybindGtbHelper", self.keybind_gtb_helper);
        self.push_int(&mut out, "keybindNametags", self.keybind_nametags);
        self.push_int(
            &mut out,
            "keybindClosestPlayer",
            self.keybind_closest_player,
        );
        self.push_int(&mut out, "keybindFightStatus", self.keybind_fight_status);
        self.push_int(&mut out, "keybindChestEsp", self.keybind_chest_esp);
        self.push_int(&mut out, "keybindChestStealer", self.keybind_chest_stealer);
        self.push_int(&mut out, "keybindRefill", self.keybind_refill);
        self.push_int(&mut out, "keybindBlockEsp", self.keybind_block_esp);
        self.push_int(&mut out, "keybindBedPlates", self.keybind_bed_plates);
        self.push_int(
            &mut out,
            "keybindPixelPartyAssist",
            self.keybind_pixel_party_assist,
        );
        self.push_int(&mut out, "keybindAutoRod", self.keybind_auto_rod);
        self.push_int(&mut out, "keybindThrowpot", self.keybind_throwpot);
        self.push_int(&mut out, "keybindAutoheal", self.keybind_autoheal);
        self.push_int(&mut out, "keybindAutoTool", self.keybind_auto_tool);
        self.push_int(
            &mut out,
            "keybindFireballWarning",
            self.keybind_fireball_warning,
        );
        self.push_int(&mut out, "keybindFastPlace", self.keybind_fast_place);
        self.push_int(&mut out, "keybindNoHurtCam", self.keybind_no_hurt_cam);
        self.push_int(&mut out, "keybindSprint", self.keybind_sprint);
        self.push_int(&mut out, "keybindTntTimer", self.keybind_tnt_timer);
        self.push_int(&mut out, "keybindWTap", self.keybind_w_tap);
        self.push_int(&mut out, "keybindJumpReset", self.keybind_jump_reset);
        self.push_int(&mut out, "keybindHitSelect", self.keybind_hit_select);
        self.push_int(&mut out, "keybindTrajectories", self.keybind_trajectories);
        self.push_int(
            &mut out,
            "keybindMurderMystery",
            self.keybind_murder_mystery,
        );
        self.push_int(
            &mut out,
            "keybindBreakProgress",
            self.keybind_break_progress,
        );
        self.push_bool(&mut out, "hudEditor", self.hud_editor);
        out.push_str(",\"hudLayout\":");
        hud_layout.write_wire(&mut out);
        out.push_str("}\n");
        out
    }
}

/// Every scalar key the payload emits, in emission order.
pub const CONFIG_KEYS: &[&str] = &[
    "perfDiagnostics",
    "armed",
    "clicking",
    "minCPS",
    "maxCPS",
    "left",
    "right",
    "rightMinCPS",
    "rightMaxCPS",
    "rightBlock",
    "breakBlocks",
    "jitter",
    "randomizationMode",
    "clickInChests",
    "aimAssist",
    "aimAssistFov",
    "aimAssistRange",
    "aimAssistStrength",
    "triggerbot",
    "killAura",
    "killAuraCpsMode",
    "killAuraMode",
    "killAuraSort",
    "killAuraAutoBlock",
    "killAuraAttackTick",
    "killAuraAutoBlockRequirePress",
    "killAuraAutoBlockCps",
    "killAuraAutoBlockRange",
    "killAuraSwingRange",
    "killAuraAttackRange",
    "killAuraFov",
    "killAuraMinCps",
    "killAuraMaxCps",
    "killAuraSwitchDelay",
    "killAuraRotations",
    "killAuraDeadZone",
    "killAuraMaxTurnSpeed",
    "killAuraMinTurnSpeed",
    "killAuraAcceleration",
    "killAuraDeceleration",
    "killAuraUseOvershoot",
    "killAuraOvershootStrength",
    "killAuraOvershootRecovery",
    "killAuraNoiseStrength",
    "killAuraVisualizeAim",
    "killAuraSmoothBack",
    "killAuraMoveFix",
    "killAuraSmoothing",
    "killAuraRavenSmoothing",
    "killAuraRavenPredictTicks",
    "killAuraRavenYawRandom",
    "killAuraGrokMaxSkew",
    "killAuraAngleStep",
    "killAuraThroughWalls",
    "killAuraRequirePress",
    "killAuraAllowMining",
    "killAuraWeaponsOnly",
    "killAuraAllowTools",
    "killAuraInventoryCheck",
    "killAuraBotCheck",
    "killAuraPlayers",
    "killAuraBosses",
    "killAuraMobs",
    "killAuraAnimals",
    "killAuraGolems",
    "killAuraSilverfish",
    "killAuraTeams",
    "killAuraShowTarget",
    "killAuraDebug",
    "killAuraRandomize",
    "killAuraRandomizeRange",
    "killAuraYRandomize",
    "killAuraLbHorizontalSpeed",
    "killAuraLbVerticalSpeed",
    "killAuraLbSmooth",
    "killAuraLbPredict",
    "killAuraLbPredictSize",
    "killAuraLbRandomize",
    "killAuraLbRandomRange",
    "killAuraLbHorizontalSearch",
    "killAuraLbBodyMin",
    "killAuraLbBodyMax",
    "speedBridge",
    "speedBridgeBlockOnly",
    "speedBridgeDelayMs",
    "speedBridgeHoldingShiftOnly",
    "speedBridgeLookingDownOnly",
    "gtbHelper",
    "pixelPartyAssist",
    "pixelPartyScanRadius",
    "pixelPartyAutoLook",
    "pixelPartyAutoWalk",
    "gtbHint",
    "gtbCount",
    "gtbPreview",
    "nametags",
    "nickHiderEnabled",
    "nickHiderAlias",
    "showModuleList",
    "moduleListStyle",
    "showLogo",
    "guiTheme",
    "supportsStatePatches",
    "closestPlayerInfo",
    "fightStatus",
    "nametagShowHealth",
    "nametagShowArmor",
    "nametagShowHeldItem",
    "nametagHideVanilla",
    "reloadMappingsNonce",
    "nametagRange",
    "chestEsp",
    "chestEspRange",
    "chestStealerEnabled",
    "chestStealerDelayMs",
    "chestStealerMenuCheck",
    "chestStealerTitleCheck",
    "chestStealerCustomItemsCheck",
    "chestStealerPhysicalCheck",
    "refillEnabled",
    "refillDelayMs",
    "blockEspEnabled",
    "blockEspBoxes",
    "blockEspTracers",
    "blockEspHud",
    "blockEspMaxCount",
    "blockEspRange",
    "blockEspBlocks",
    "bedPlatesEnabled",
    "bedPlatesShowDistance",
    "bedPlatesRange",
    "reachEnabled",
    "reachMin",
    "reachMax",
    "reachChance",
    "reachChanceMode",
    "reachOnlyWhileSprinting",
    "reachDisableInWater",
    "reachVerticalCheck",
    "velocityEnabled",
    "velocityHorizontal",
    "velocityVertical",
    "velocityChance",
    "autoTotemEnabled",
    "autoTotemMode",
    "autoTotemHealth",
    "autoTotemElytra",
    "autoTotemDelay",
    "autoTotemBehaviorMode",
    "autoRodEnabled",
    "autoRodSlotMode",
    "autoRodVerifyForcedSlot",
    "autoRodExtensionTicks",
    "autoRodHoldToExtend",
    "throwpotEnabled",
    "autoHealEnabled",
    "autoHealHealth",
    "autoHealDelayMs",
    "autoToolEnabled",
    "autoToolSwapWeapon",
    "autoToolInstantSwap",
    "autoToolSwapToDelay",
    "autoToolSwapBack",
    "autoToolSwapBackDelay",
    "autoToolRequireMouseDown",
    "autoToolOnlySneaking",
    "autoToolBedwarsMode",
    "antiDebuffEnabled",
    "hitDelayFixEnabled",
    "fireballWarning",
    "fireballWarningBox",
    "fireballWarningArrow",
    "fireballWarningSound",
    "fastPlace",
    "noHurtCam",
    "sprint",
    "tntTimer",
    "wTap",
    "jumpReset",
    "hitSelect",
    "trajectories",
    "murderMystery",
    "breakProgress",
    "jumpResetChance",
    "keybindAutoclicker",
    "keybindRightClick",
    "keybindAimAssist",
    "keybindTriggerbot",
    "keybindKillAura",
    "keybindSpeedBridge",
    "keybindGtbHelper",
    "keybindNametags",
    "keybindClosestPlayer",
    "keybindFightStatus",
    "keybindChestEsp",
    "keybindChestStealer",
    "keybindRefill",
    "keybindBlockEsp",
    "keybindBedPlates",
    "keybindPixelPartyAssist",
    "keybindAutoRod",
    "keybindThrowpot",
    "keybindAutoheal",
    "keybindAutoTool",
    "keybindFireballWarning",
    "keybindFastPlace",
    "keybindNoHurtCam",
    "keybindSprint",
    "keybindTntTimer",
    "keybindWTap",
    "keybindJumpReset",
    "keybindHitSelect",
    "keybindTrajectories",
    "keybindMurderMystery",
    "keybindBreakProgress",
    "hudEditor",
];

impl ConfigPayload {
    fn push_bool(&self, out: &mut String, key: &str, value: bool) {
        push_key(out, key);
        out.push_str(if value { "true" } else { "false" });
    }

    fn push_int(&self, out: &mut String, key: &str, value: i32) {
        push_key(out, key);
        out.push_str(value.to_string().as_str());
    }

    fn push_float(&self, out: &mut String, key: &str, value: f32) {
        push_key(out, key);
        out.push_str(format_f32(value).as_str());
    }

    fn push_text(&self, out: &mut String, key: &str, value: &str) {
        push_key(out, key);
        out.push_str(escape_json_string(value).as_str());
    }
}

fn push_key(out: &mut String, key: &str) {
    out.push_str(",\"");
    out.push_str(key);
    out.push_str("\":");
}

/// `System.Text.Json` writes a `float` in shortest round-trip form with no
/// trailing `.0`; Rust's `Display` for `f32` matches that for every value the
/// config payload can carry.
fn format_f32(value: f32) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    format!("{value}")
}

/// JSON string literal, escaping exactly as `System.Text.Json` escapes the
/// printable ASCII the payload carries.
fn escape_json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

/// `GameStateClient.ModuleListStyleToIndex`.
#[must_use]
pub fn module_list_style_to_index(style_name: Option<&str>) -> i32 {
    let Some(style_name) = style_name else {
        return 0;
    };
    let normalized = style_name.trim().to_lowercase();
    if normalized.is_empty() {
        return 0;
    }

    match normalized.as_str() {
        "default" => 0,
        "minimal" => 1,
        "outlined" => 2,
        "glass" => 3,
        "bold" => 4,
        "classic" => 5,
        "modern" => 6,
        _ => 0,
    }
}

/// `GameStateClient.NormalizeDetectedVersion`.
///
/// Returns the bridge-family version token the host keys its capability
/// fallback and DLL choice on, or `None` when the text carries no usable token.
#[must_use]
pub fn normalize_detected_version(raw: Option<&str>) -> Option<&'static str> {
    let raw = raw?;
    let value = raw.trim().to_lowercase();
    if value.is_empty() {
        return None;
    }

    if contains_version_token(&value, "26.3") {
        return Some("26.3");
    }
    if contains_version_token(&value, "26.2") {
        return Some("26.2");
    }
    if contains_version_token(&value, "26.1")
        || value == "26"
        || value == "minecraft 26"
        || value.starts_with("26.")
    {
        return Some("26.1");
    }
    if contains_version_token(&value, "1.21") || value == "1.21" {
        return Some("1.21");
    }
    if contains_version_token(&value, "1.8.9") || value == "1.8" || value.starts_with("1.8.") {
        return Some("1.8.9");
    }

    None
}

/// Whether `version` appears as a version token, not as part of another number.
///
/// Equivalent to the managed `(?<![\d.])ESC(?![0-9])`, so a library version such
/// as log4j `2.26.0` never matches `26.2`.
fn contains_version_token(value: &str, version: &str) -> bool {
    for (index, _) in value.char_indices() {
        let Some(rest) = value[index..].strip_prefix(version) else {
            continue;
        };

        let before_is_boundary = match value[..index].chars().next_back() {
            None => true,
            Some(previous) => !previous.is_ascii_digit() && previous != '.',
        };
        let after_is_boundary = rest
            .chars()
            .next()
            .is_none_or(|next| !next.is_ascii_digit());

        if before_is_boundary && after_is_boundary {
            return true;
        }
    }

    false
}

/// `GameStateClient.ResolveBridgeDllName`: `26.*` and `1.21*` share the modern DLL.
#[must_use]
pub fn resolve_bridge_dll_name(resolved_version: &str) -> &'static str {
    let version = resolved_version.trim();
    if version.is_empty() {
        return "bridge.dll";
    }

    let lowered = version.to_lowercase();
    if lowered.starts_with("26.") || lowered.starts_with("1.21") {
        return "bridge_261.dll";
    }

    "bridge.dll"
}

/// `moduleAction` press frame for Auto Rod. `slotMode` is clamped to `[0, 9]` and
/// `extensionTicks` to `[1, 40]`, exactly as the managed builder does.
#[must_use]
pub fn auto_rod_action_message(
    slot_mode: i32,
    verify_forced_slot: bool,
    extension_ticks: i32,
    hold_to_extend: bool,
    request_id: i32,
) -> String {
    format!(
        "{{\"type\":\"moduleAction\",\"action\":\"autoRod\",\"phase\":\"press\",\
         \"enabled\":true,\"requestId\":{request_id},\"slotMode\":{},\
         \"verifyForcedSlot\":{verify_forced_slot},\"extensionTicks\":{},\
         \"holdToExtend\":{hold_to_extend}}}\n",
        slot_mode.clamp(0, 9),
        extension_ticks.clamp(1, 40),
    )
}

/// `moduleAction` release frame for Auto Rod. Carries no `requestId`: the bridge
/// does not acknowledge a release.
#[must_use]
pub fn auto_rod_release_message() -> String {
    "{\"type\":\"moduleAction\",\"action\":\"autoRod\",\"phase\":\"release\"}\n".to_owned()
}

/// `moduleAction` press frame for Throwpot.
#[must_use]
pub fn throw_pot_action_message(request_id: i32) -> String {
    format!(
        "{{\"type\":\"moduleAction\",\"action\":\"throwPot\",\"phase\":\"press\",\
         \"enabled\":true,\"requestId\":{request_id}}}\n"
    )
}
