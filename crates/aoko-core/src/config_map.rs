//! The one mapping from [`Profile`] and the controller's switches to the `config` frame's
//! [`ConfigPayload`].
//!
//! The managed host built the frame from `Clicker` properties. Those properties had two
//! writers: `Profile.ApplyToClicker` (`Profile.cs:727-903`) filled them from the saved
//! settings, and the switch buttons changed them at runtime, which is why the frame reported
//! live state. This port keeps the two halves apart — the profile is the saved settings, the
//! [`EnabledFlag`] set is what is running now — so the frame reads both and writes neither
//! back:
//!
//! * a **module boolean** comes from the switch set, keyed by
//!   [`module_catalog::MODULE_WIRE_KEYS`], because a switch press is what the managed
//!   `Clicker` property changed. `armed` is one of these: the managed `Clicker.IsArmed` is
//!   this port's `LeftClick` switch. The frame used to send the profile's saved boolean here,
//!   which meant a module the user switched on told the bridge it was off;
//! * everything else comes from the profile, including the sub-settings a module owns
//!   (`pixelPartyAutoLook`, `nametagHideVanilla`, `left`, ...).
//!
//! The struct literal below is deliberately exhaustive — there is no
//! `..ConfigPayload::default()` — so adding a payload field is a compile error until it has a
//! source here. `every_payload_key_has_a_live_source` is the runtime half: it serialises a
//! profile with every setting off its default and a switch set with every switch on, and
//! requires every key to move.
//!
//! Three fields are derived rather than copied, because the managed `Clicker` derived them:
//!
//! * `jitter` is `Clicker.JitterEnabled`, which is `RandomizationMode != Basic`
//!   (`Clicker.cs:350`), so it follows the mode. This is the one key a switch *could* feed —
//!   `EnabledFlag::Jitter` exists for it — and it deliberately does not: nothing in the app
//!   writes that switch, so the frame would disagree with the clicker that jitters. See
//!   `module_catalog::MODULE_WIRE_KEY_EXCEPTIONS`.
//! * `nick_hider_enabled` is `Clicker.IsNickHiderActive`, which is `NickHiderEnabled`
//!   **and** a non-empty normalised alias (`Clicker.cs:981`): the switch from the module set,
//!   the alias from the profile.
//! * `block_esp_blocks` is `Clicker.BlockEspBlocksSerialized`, the delimited target string
//!   built from the profile's targets (`Clicker.cs:1343`).
//!
//! Two parameters are runtime state rather than profile settings, so they arrive from the
//! controller instead of a field:
//!
//! * `hudEditor` — the managed frame read `Clicker.HudEditorActive`, which the window toggled
//!   and no profile field ever carried. `AppController::set_hud_editor` owns it.
//! * `reloadMappingsNonce` — the managed host bumped a counter when the user asked to reload
//!   mappings (`MainWindow.xaml.cs:1397-1404`), and the bridge reloads when it sees a new
//!   value. `AppController::request_mapping_reload` owns it.
//!
//! # Keys with no source at all
//!
//! Five keys are runtime state the managed host read from `Clicker` and this port has nowhere
//! else to take them from yet: `perfDiagnostics`, `clicking`, `gtbHint`, `gtbCount`,
//! `gtbPreview`. They keep the payload default, and the test pins that list in both directions
//! so a key cannot quietly join it.
//!
//! A sixth, `supportsStatePatches`, is not a setting and never will be: `to_line` writes the
//! literal `true` for every frame, as the managed host did (`GameStateClient.cs:1426`: the
//! modern bridge only emits partial fast-state packets after this explicit opt-in, so an older
//! loader that does not read the key keeps receiving complete V1-compatible state documents).
//! The opt-in is the host's, not the user's, so a profile has nothing to say about it: a wired
//! frame and `ConfigPayload::default()` carry the identical string.

use aoko_protocol::block_esp::{self, BlockEspTarget, BlockEspTargetData};
use aoko_protocol::config_payload::{module_list_style_to_index, ConfigPayload};
use aoko_protocol::module_catalog::EnabledFlag;
use aoko_protocol::nick_hider;
use aoko_protocol::profile::{ClickRandomizationMode, Profile};

/// The profile field that stores each module switch, by its JSON key.
///
/// This is the other half of `Profile.ApplyToClicker` (`Profile.cs:727-903`): the managed apply
/// step copied these into the `Clicker`, and the switches changed them afterwards, so the frame
/// started out reporting the saved settings. `AppController` seeds its switch set from here
/// when a profile is built or adopted, which is why the frame itself never reads this table: a
/// module boolean comes from the switch, and a saved value only reaches the wire by becoming
/// one.
///
/// The keys are the profile's serde names, and the test below pins both halves: every key is a
/// boolean on the profile, and setting it seeds exactly the switch it is paired with.
pub(crate) const SWITCH_PROFILE_KEYS: [(EnabledFlag, &str); 39] = [
    (EnabledFlag::LeftClick, "isArmed"),
    (EnabledFlag::RightClick, "rightClickEnabled"),
    (EnabledFlag::BreakBlocks, "breakBlocksEnabled"),
    (EnabledFlag::ClickInChests, "clickInChests"),
    (EnabledFlag::AimAssist, "aimAssistEnabled"),
    (EnabledFlag::Triggerbot, "triggerbotEnabled"),
    (EnabledFlag::KillAura, "killAuraEnabled"),
    (EnabledFlag::SpeedBridge, "speedBridgeEnabled"),
    (EnabledFlag::GtbHelper, "gtbHelperEnabled"),
    (EnabledFlag::PixelPartyAssist, "pixelPartyAssistEnabled"),
    (EnabledFlag::Nametags, "nametagsEnabled"),
    (EnabledFlag::NickHider, "nickHiderEnabled"),
    (EnabledFlag::ClosestPlayerInfo, "closestPlayerInfoEnabled"),
    (EnabledFlag::FightStatus, "fightStatusEnabled"),
    (EnabledFlag::ChestEsp, "chestEspEnabled"),
    (EnabledFlag::ChestStealer, "chestStealerEnabled"),
    (EnabledFlag::Refill, "refillEnabled"),
    (EnabledFlag::BlockEsp, "blockEspEnabled"),
    (EnabledFlag::BedPlates, "bedPlatesEnabled"),
    (EnabledFlag::Reach, "reachEnabled"),
    (EnabledFlag::Velocity, "velocityEnabled"),
    (EnabledFlag::AutoTotem, "autoTotemEnabled"),
    (EnabledFlag::AutoRod, "autoRodEnabled"),
    (EnabledFlag::Throwpot, "throwpotEnabled"),
    (EnabledFlag::AutoHeal, "autoHealEnabled"),
    (EnabledFlag::AutoTool, "autoToolEnabled"),
    (EnabledFlag::AntiDebuff, "antiDebuffEnabled"),
    (EnabledFlag::HitDelayFix, "hitDelayFixEnabled"),
    (EnabledFlag::FireballWarning, "fireballWarningEnabled"),
    (EnabledFlag::FastPlace, "fastPlaceEnabled"),
    (EnabledFlag::NoHurtCam, "noHurtCamEnabled"),
    (EnabledFlag::Sprint, "sprintEnabled"),
    (EnabledFlag::TntTimer, "tntTimerEnabled"),
    (EnabledFlag::WTap, "wTapEnabled"),
    (EnabledFlag::JumpReset, "jumpResetEnabled"),
    (EnabledFlag::HitSelect, "hitSelectEnabled"),
    (EnabledFlag::Trajectories, "trajectoriesEnabled"),
    (EnabledFlag::MurderMystery, "murderMysteryEnabled"),
    (EnabledFlag::BreakProgress, "breakProgressEnabled"),
];

/// The switches a profile carries, as the controller's switch set holds them.
///
/// One direction only: the profile is the saved state, the returned set is what the app is
/// running, and nothing writes the set back. A profile that does not serialise yields no
/// switches rather than a panic — the caller then reports every module as off, which is the
/// truth about an app that failed to read its own settings.
pub(crate) fn profile_switches(profile: &Profile) -> Vec<EnabledFlag> {
    let Ok(settings) = serde_json::to_value(profile) else {
        return Vec::new();
    };
    SWITCH_PROFILE_KEYS
        .iter()
        .filter(|(_, key)| {
            settings
                .get(*key)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .map(|(flag, _)| *flag)
        .collect()
}

/// The frame the profile and the controller's switches produce.
///
/// Two sources, both one direction: the profile owns the settings, the switch set owns which
/// modules are live. The managed host had a `Clicker` property for each, filled from the
/// profile on apply and changed by the switches at runtime, so the frame read live state; this
/// port has the two halves apart, and the frame reads the same two.
pub(crate) fn payload(
    profile: &Profile,
    modules: &[EnabledFlag],
    hud_editor: bool,
    reload_nonce: i32,
) -> ConfigPayload {
    let enabled = |flag: EnabledFlag| modules.contains(&flag);
    let keybind = |module: &str| profile.module_keys.get(module).copied().unwrap_or(0);
    let nick_hider_alias = nick_hider::normalize_alias(Some(&profile.nick_hider_alias));
    let nick_hider_enabled = enabled(EnabledFlag::NickHider) && !nick_hider_alias.is_empty();
    let block_esp_blocks = block_esp::serialize_targets(&block_esp_targets(profile));

    ConfigPayload {
        perf_diagnostics: false,
        armed: enabled(EnabledFlag::LeftClick),
        clicking: false,
        min_cps: profile.min_cps,
        max_cps: profile.max_cps,
        left: profile.left_click_enabled,
        right: enabled(EnabledFlag::RightClick),
        right_min_cps: profile.right_min_cps,
        right_max_cps: profile.right_max_cps,
        right_block: profile.right_click_only_block,
        break_blocks: enabled(EnabledFlag::BreakBlocks),
        jitter: profile.randomization_mode() != ClickRandomizationMode::Basic,
        randomization_mode: profile.randomization_mode().as_i32(),
        click_in_chests: enabled(EnabledFlag::ClickInChests),
        aim_assist: enabled(EnabledFlag::AimAssist),
        aim_assist_fov: profile.aim_assist_fov,
        aim_assist_range: profile.aim_assist_range,
        aim_assist_strength: profile.aim_assist_strength,
        triggerbot: enabled(EnabledFlag::Triggerbot),
        kill_aura: enabled(EnabledFlag::KillAura),
        kill_aura_cps_mode: profile.kill_aura_settings.cps_mode().to_owned(),
        kill_aura_mode: profile.kill_aura_settings.mode().to_owned(),
        kill_aura_sort: profile.kill_aura_settings.sort().to_owned(),
        kill_aura_auto_block: profile.kill_aura_settings.auto_block().to_owned(),
        kill_aura_attack_tick: profile.kill_aura_settings.attack_tick(),
        kill_aura_auto_block_require_press: profile.kill_aura_settings.auto_block_require_press(),
        kill_aura_auto_block_cps: profile.kill_aura_settings.auto_block_cps(),
        kill_aura_auto_block_range: profile.kill_aura_settings.auto_block_range(),
        kill_aura_swing_range: profile.kill_aura_settings.swing_range(),
        kill_aura_attack_range: profile.kill_aura_settings.attack_range(),
        kill_aura_fov: profile.kill_aura_settings.fov(),
        kill_aura_min_cps: profile.kill_aura_settings.min_cps(),
        kill_aura_max_cps: profile.kill_aura_settings.max_cps(),
        kill_aura_switch_delay: profile.kill_aura_settings.switch_delay(),
        kill_aura_rotations: profile.kill_aura_settings.rotations().to_owned(),
        kill_aura_dead_zone: profile.kill_aura_settings.dead_zone_size(),
        kill_aura_max_turn_speed: profile.kill_aura_settings.max_turn_speed(),
        kill_aura_min_turn_speed: profile.kill_aura_settings.min_turn_speed(),
        kill_aura_acceleration: profile.kill_aura_settings.acceleration(),
        kill_aura_deceleration: profile.kill_aura_settings.deceleration(),
        kill_aura_use_overshoot: profile.kill_aura_settings.use_overshoot(),
        kill_aura_overshoot_strength: profile.kill_aura_settings.overshoot_strength(),
        kill_aura_overshoot_recovery: profile.kill_aura_settings.overshoot_recovery(),
        kill_aura_noise_strength: profile.kill_aura_settings.noise_strength(),
        kill_aura_visualize_aim: profile.kill_aura_settings.visualize_aim(),
        kill_aura_smooth_back: profile.kill_aura_settings.smooth_back(),
        kill_aura_move_fix: profile.kill_aura_settings.move_fix().to_owned(),
        kill_aura_smoothing: profile.kill_aura_settings.smoothing(),
        kill_aura_raven_smoothing: profile.kill_aura_settings.raven_smoothing(),
        kill_aura_raven_predict_ticks: profile.kill_aura_settings.raven_predict_ticks(),
        kill_aura_raven_yaw_random: profile.kill_aura_settings.raven_yaw_random(),
        kill_aura_grok_max_skew: profile.kill_aura_settings.grok_max_skew(),
        kill_aura_angle_step: profile.kill_aura_settings.angle_step(),
        kill_aura_through_walls: profile.kill_aura_settings.through_walls(),
        kill_aura_require_press: profile.kill_aura_settings.require_press(),
        kill_aura_allow_mining: profile.kill_aura_settings.allow_mining(),
        kill_aura_weapons_only: profile.kill_aura_settings.weapons_only(),
        kill_aura_allow_tools: profile.kill_aura_settings.allow_tools(),
        kill_aura_inventory_check: profile.kill_aura_settings.inventory_check(),
        kill_aura_bot_check: profile.kill_aura_settings.bot_check(),
        kill_aura_players: profile.kill_aura_settings.players(),
        kill_aura_bosses: profile.kill_aura_settings.bosses(),
        kill_aura_mobs: profile.kill_aura_settings.mobs(),
        kill_aura_animals: profile.kill_aura_settings.animals(),
        kill_aura_golems: profile.kill_aura_settings.golems(),
        kill_aura_silverfish: profile.kill_aura_settings.silverfish(),
        kill_aura_teams: profile.kill_aura_settings.teams(),
        kill_aura_show_target: profile.kill_aura_settings.show_target().to_owned(),
        kill_aura_debug: profile.kill_aura_settings.debug_log().to_owned(),
        kill_aura_randomize: profile.kill_aura_settings.randomize(),
        kill_aura_randomize_range: profile.kill_aura_settings.randomize_range(),
        kill_aura_y_randomize: profile.kill_aura_settings.y_randomize_strength(),
        kill_aura_lb_horizontal_speed: profile.kill_aura_settings.liquid_bounce_horizontal_speed(),
        kill_aura_lb_vertical_speed: profile.kill_aura_settings.liquid_bounce_vertical_speed(),
        kill_aura_lb_smooth: profile.kill_aura_settings.liquid_bounce_smooth_factor(),
        kill_aura_lb_predict: profile.kill_aura_settings.liquid_bounce_predict(),
        kill_aura_lb_predict_size: profile.kill_aura_settings.liquid_bounce_predict_size(),
        kill_aura_lb_randomize: profile.kill_aura_settings.liquid_bounce_randomize(),
        kill_aura_lb_random_range: profile.kill_aura_settings.liquid_bounce_randomize_range(),
        kill_aura_lb_horizontal_search: profile
            .kill_aura_settings
            .liquid_bounce_horizontal_search(),
        kill_aura_lb_body_min: profile.kill_aura_settings.liquid_bounce_body_point_min(),
        kill_aura_lb_body_max: profile.kill_aura_settings.liquid_bounce_body_point_max(),
        speed_bridge: enabled(EnabledFlag::SpeedBridge),
        speed_bridge_block_only: profile.speed_bridge_block_only,
        speed_bridge_delay_ms: profile.speed_bridge_delay_ms,
        speed_bridge_holding_shift_only: profile.speed_bridge_holding_shift_only,
        speed_bridge_looking_down_only: profile.speed_bridge_looking_down_only,
        gtb_helper: enabled(EnabledFlag::GtbHelper),
        pixel_party_assist: enabled(EnabledFlag::PixelPartyAssist),
        pixel_party_scan_radius: profile.pixel_party_scan_radius,
        pixel_party_auto_look: profile.pixel_party_auto_look_enabled,
        pixel_party_auto_walk: profile.pixel_party_auto_walk_enabled,
        gtb_hint: "-".to_owned(),
        gtb_count: 0,
        gtb_preview: "-".to_owned(),
        nametags: enabled(EnabledFlag::Nametags),
        nick_hider_enabled,
        nick_hider_alias: nick_hider_alias.clone(),
        show_module_list: profile.show_module_list,
        module_list_style: module_list_style_to_index(Some(&profile.module_list_style)),
        show_logo: profile.show_logo,
        gui_theme: profile.gui_theme.to_owned(),
        closest_player_info: enabled(EnabledFlag::ClosestPlayerInfo),
        fight_status: enabled(EnabledFlag::FightStatus),
        nametag_show_health: profile.nametag_show_health,
        nametag_show_armor: profile.nametag_show_armor,
        nametag_show_held_item: profile.nametag_show_held_item,
        nametag_hide_vanilla: profile.nametag_hide_vanilla,
        reload_mappings_nonce: reload_nonce,
        nametag_range: profile.nametag_range,
        chest_esp: enabled(EnabledFlag::ChestEsp),
        chest_esp_range: profile.chest_esp_range,
        chest_stealer_enabled: enabled(EnabledFlag::ChestStealer),
        chest_stealer_delay_ms: profile.chest_stealer_delay_ms,
        chest_stealer_menu_check: profile.chest_stealer_menu_check(),
        chest_stealer_title_check: profile.chest_stealer_title_check(),
        chest_stealer_custom_items_check: profile.chest_stealer_custom_items_check(),
        chest_stealer_physical_check: profile.chest_stealer_physical_check(),
        refill_enabled: enabled(EnabledFlag::Refill),
        refill_delay_ms: profile.refill_delay_ms,
        block_esp_enabled: enabled(EnabledFlag::BlockEsp),
        block_esp_boxes: profile.block_esp_boxes,
        block_esp_tracers: profile.block_esp_tracers,
        block_esp_hud: profile.block_esp_hud,
        block_esp_max_count: profile.block_esp_max_count,
        block_esp_range: profile.block_esp_range,
        block_esp_blocks,
        bed_plates_enabled: enabled(EnabledFlag::BedPlates),
        bed_plates_show_distance: profile.bed_plates_show_distance,
        bed_plates_range: profile.bed_plates_range,
        reach_enabled: enabled(EnabledFlag::Reach),
        reach_min: profile.reach_min,
        reach_max: profile.reach_max,
        reach_chance: profile.reach_chance,
        reach_chance_mode: profile.reach_chance_mode,
        reach_only_while_sprinting: profile.reach_only_while_sprinting,
        reach_disable_in_water: profile.reach_disable_in_water,
        reach_vertical_check: profile.reach_vertical_check,
        velocity_enabled: enabled(EnabledFlag::Velocity),
        velocity_horizontal: profile.velocity_horizontal,
        velocity_vertical: profile.velocity_vertical,
        velocity_chance: profile.velocity_chance,
        auto_totem_enabled: enabled(EnabledFlag::AutoTotem),
        auto_totem_mode: profile.auto_totem_mode,
        auto_totem_health: profile.auto_totem_health,
        auto_totem_elytra: profile.auto_totem_elytra,
        auto_totem_delay: profile.auto_totem_delay,
        auto_totem_behavior_mode: profile.auto_totem_behavior_mode,
        auto_rod_enabled: enabled(EnabledFlag::AutoRod),
        auto_rod_slot_mode: profile.auto_rod_slot_mode(),
        auto_rod_verify_forced_slot: profile.auto_rod_verify_forced_slot,
        auto_rod_extension_ticks: profile.auto_rod_extension_ticks(),
        auto_rod_hold_to_extend: profile.auto_rod_hold_to_extend,
        throwpot_enabled: enabled(EnabledFlag::Throwpot),
        auto_heal_enabled: enabled(EnabledFlag::AutoHeal),
        auto_heal_health: profile.auto_heal_health,
        auto_heal_delay_ms: profile.auto_heal_delay_ms,
        auto_tool_enabled: enabled(EnabledFlag::AutoTool),
        auto_tool_swap_weapon: profile.auto_tool_swap_weapon,
        auto_tool_instant_swap: profile.auto_tool_instant_swap,
        auto_tool_swap_to_delay: profile.auto_tool_swap_to_delay,
        auto_tool_swap_back: profile.auto_tool_swap_back,
        auto_tool_swap_back_delay: profile.auto_tool_swap_back_delay,
        auto_tool_require_mouse_down: profile.auto_tool_require_mouse_down,
        auto_tool_only_sneaking: profile.auto_tool_only_sneaking,
        auto_tool_bedwars_mode: profile.auto_tool_bedwars_mode,
        anti_debuff_enabled: enabled(EnabledFlag::AntiDebuff),
        hit_delay_fix_enabled: enabled(EnabledFlag::HitDelayFix),
        fireball_warning: enabled(EnabledFlag::FireballWarning),
        fireball_warning_box: profile.fireball_warning_box,
        fireball_warning_arrow: profile.fireball_warning_arrow,
        fireball_warning_sound: profile.fireball_warning_sound,
        fast_place: enabled(EnabledFlag::FastPlace),
        no_hurt_cam: enabled(EnabledFlag::NoHurtCam),
        sprint: enabled(EnabledFlag::Sprint),
        tnt_timer: enabled(EnabledFlag::TntTimer),
        w_tap: enabled(EnabledFlag::WTap),
        jump_reset: enabled(EnabledFlag::JumpReset),
        hit_select: enabled(EnabledFlag::HitSelect),
        trajectories: enabled(EnabledFlag::Trajectories),
        murder_mystery: enabled(EnabledFlag::MurderMystery),
        break_progress: enabled(EnabledFlag::BreakProgress),
        jump_reset_chance: profile.jump_reset_chance,
        keybind_autoclicker: keybind("autoclicker"),
        keybind_right_click: keybind("rightclick"),
        keybind_aim_assist: keybind("aimassist"),
        keybind_triggerbot: keybind("triggerbot"),
        keybind_kill_aura: keybind("killaura"),
        keybind_speed_bridge: keybind("speedbridge"),
        keybind_gtb_helper: keybind("gtbhelper"),
        keybind_nametags: keybind("nametags"),
        keybind_closest_player: keybind("closestplayer"),
        keybind_fight_status: keybind("fightstatus"),
        keybind_chest_esp: keybind("chestesp"),
        keybind_chest_stealer: keybind("cheststealer"),
        keybind_refill: keybind("refill"),
        keybind_block_esp: keybind("blockesp"),
        keybind_bed_plates: keybind("bedplates"),
        keybind_pixel_party_assist: keybind("pixelpartyassist"),
        keybind_auto_rod: keybind("autorod"),
        keybind_throwpot: keybind("throwpot"),
        keybind_autoheal: keybind("autoheal"),
        keybind_auto_tool: keybind("autotool"),
        keybind_fireball_warning: keybind("fireballwarning"),
        keybind_fast_place: keybind("fastplace"),
        keybind_no_hurt_cam: keybind("nohurtcam"),
        keybind_sprint: keybind("sprint"),
        keybind_tnt_timer: keybind("tnttimer"),
        keybind_w_tap: keybind("wtap"),
        keybind_jump_reset: keybind("jumpreset"),
        keybind_hit_select: keybind("hitselect"),
        keybind_trajectories: keybind("trajectories"),
        keybind_murder_mystery: keybind("murdermystery"),
        keybind_break_progress: keybind("breakprogress"),
        hud_editor,
    }
}

/// `Profile.BuildBlockEspTargets`: blank ids are dropped, and a profile with no usable target
/// falls back to the curated presets.
fn block_esp_targets(profile: &Profile) -> Vec<BlockEspTarget> {
    let mut targets: Vec<BlockEspTarget> = profile
        .block_esp_targets
        .iter()
        .filter(|data| !data.registry_id.trim().is_empty())
        .map(BlockEspTargetData::to_target)
        .collect();
    if targets.is_empty() {
        targets = block_esp::build_default_targets();
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_protocol::config_payload::CONFIG_KEYS;
    use aoko_protocol::hud::HudLayout;
    use aoko_protocol::module_catalog::{MODULE_WIRE_KEYS, MODULE_WIRE_KEY_EXCEPTIONS};
    use aoko_protocol::profile::load_profile_from_json;
    use serde_json::Value;

    /// The keys with no source at all; see the module docs. `hudEditor` and
    /// `reloadMappingsNonce` are not here: the controller owns both, and the fixture turns
    /// them on. `supportsStatePatches` is here because it is a protocol constant rather than
    /// a field — `to_line` writes `true` unconditionally — so the guard's "must move" arm can
    /// never pass for it, and no field should be invented to make it pass.
    const NO_PROFILE_SOURCE: [&str; 6] = [
        "supportsStatePatches",
        "perfDiagnostics",
        "clicking",
        "gtbHint",
        "gtbCount",
        "gtbPreview",
    ];

    /// A profile with every setting moved off the payload's default for that key.
    ///
    /// Rather than list a hundred assignments, this walks the serialised default profile and
    /// moves every scalar in place — booleans to `true`, numbers to `1`, strings to `"guard"` —
    /// so a setting added to `Profile` is covered without touching this test. The overrides are
    /// the two values whose payload key is *composed*, and cannot be driven by one scalar:
    /// `moduleListStyle` feeds an index where every unknown name is `0`, and the block list is
    /// derived from the targets.
    fn fixture_profile() -> Profile {
        let mut json =
            serde_json::to_value(Profile::default()).expect("the default profile serialises");
        move_every_scalar(&mut json);

        let object = json
            .as_object_mut()
            .expect("a profile serialises as an object");
        object.insert("moduleListStyle".to_owned(), Value::from("glass"));
        object.insert(
            "blockEspTargets".to_owned(),
            serde_json::json!([{
                "registryId": "minecraft:emerald_block",
                "displayName": "Emerald Block",
                "colorHex": "00FF88",
                "enabled": true,
            }]),
        );

        load_profile_from_json(&json.to_string()).expect("the fixture profile loads")
    }

    fn move_every_scalar(node: &mut Value) {
        match node {
            Value::Bool(flag) => *flag = true,
            Value::Number(_) => *node = Value::from(1),
            Value::String(text) => *text = "guard".to_owned(),
            Value::Array(items) => items.iter_mut().for_each(move_every_scalar),
            Value::Object(map) => map.values_mut().for_each(move_every_scalar),
            Value::Null => {}
        }
    }

    /// Every switch that feeds a frame key, as the controller holds them with every module
    /// on. The exceptions in [`MODULE_WIRE_KEY_EXCEPTIONS`] are not here: neither feeds a key
    /// of its own.
    fn fixture_modules() -> Vec<EnabledFlag> {
        MODULE_WIRE_KEYS.iter().map(|(flag, _)| *flag).collect()
    }

    fn keys(line: &str) -> serde_json::Map<String, Value> {
        serde_json::from_str::<Value>(line)
            .expect("the frame is JSON")
            .as_object()
            .expect("the frame is an object")
            .clone()
    }

    /// The completeness guard: every emitted key either moves when its source moves — the
    /// fixture profile, the fixture switch set, the controller's HUD editor mode, or its
    /// mapping-reload nonce — or is on the no-source list and stays put.
    ///
    /// A key added to `ConfigPayload` and left at its default fails the first arm. A key wired
    /// to a profile field that the fixture does not move fails it too. A key that gains a
    /// source without leaving the no-source list fails the second.
    #[test]
    fn every_payload_key_has_a_live_source() {
        let wired = keys(
            &payload(&fixture_profile(), &fixture_modules(), true, 7).to_line(&HudLayout::new()),
        );
        let defaults = keys(&ConfigPayload::default().to_line(&HudLayout::new()));

        for key in CONFIG_KEYS {
            assert!(
                wired.contains_key(*key),
                "{key} is a config key but the fixture frame does not carry it"
            );
            let moved = wired.get(*key) != defaults.get(*key);
            if NO_PROFILE_SOURCE.contains(key) {
                assert!(
                    !moved,
                    "{key} has no profile source, so it must keep the payload default"
                );
            } else {
                assert!(
                    moved,
                    "{key} did not move off its default: nothing in the mapping feeds it"
                );
            }
        }

        for key in NO_PROFILE_SOURCE {
            assert!(
                CONFIG_KEYS.contains(&key),
                "{key} is on the no-source list but is not a config key"
            );
        }
    }

    /// The block list is a derived string, so it gets a behavioural test of its own: an enabled
    /// target reaches the frame in the wire spelling, a disabled one does not reach it at all,
    /// and a repeated id appears once.
    ///
    /// The spelling is the normalised path token, not the registry id: `serialize_targets` runs
    /// `block_esp::normalize_id`, which drops the namespace, exactly as the managed
    /// `BlockEspConfig.Serialize` did — its own test pins `diamond_ore=00E5FF` from a
    /// `minecraft:diamond_ore` target (`Aoko.Tests/BlockEspTests.cs`). So the entry is
    /// `emerald_block=00FF88`, and asserting the whole entry is the stronger claim: it pins the
    /// id, the `=` separator the bridge's naive parser needs, and the uppercase colour.
    #[test]
    fn the_block_list_follows_the_profiles_enabled_targets() {
        let target = |id: &str, enabled: bool| BlockEspTargetData {
            registry_id: id.to_owned(),
            display_name: "Any".to_owned(),
            color_hex: "00FF88".to_owned(),
            enabled,
        };

        let mut profile = Profile::default();
        profile.block_esp_targets = vec![target("minecraft:emerald_block", true)];
        let enabled = payload(&profile, &[], false, 0).block_esp_blocks;
        assert_eq!(
            enabled, "emerald_block=00FF88",
            "an enabled target has to reach the frame in the normalised wire spelling"
        );

        profile.block_esp_targets = vec![target("minecraft:emerald_block", false)];
        let disabled = payload(&profile, &[], false, 0).block_esp_blocks;
        assert_eq!(
            disabled, "",
            "a disabled target must not reach the frame at all"
        );

        profile.block_esp_targets = vec![
            target("minecraft:emerald_block", true),
            target("minecraft:emerald_block", true),
        ];
        let repeated = payload(&profile, &[], false, 0).block_esp_blocks;
        assert_eq!(
            repeated, "emerald_block=00FF88",
            "a repeated id is serialised once"
        );
    }

    /// The switch-to-key table is only worth having if the frame obeys it: one switch at a
    /// time, the frame must move exactly the key the table names.
    ///
    /// This is the guard the profile guard cannot be. A module boolean left reading the
    /// profile would still move under `every_payload_key_has_a_live_source`, because the
    /// fixture profile moves every field — the frame would just keep ignoring the switch. Here
    /// the profile is fixed and only the switch set changes, so a key that does not follow its
    /// switch fails, and a key wired to the wrong switch fails twice: it does not move for its
    /// own switch, and it moves for someone else's.
    #[test]
    fn one_switch_moves_exactly_its_own_key() {
        // The nick hider's key needs an alias to be allowed on; that half is the profile's,
        // and it must not decide whether the switch reaches the wire.
        let mut profile = Profile::default();
        profile.nick_hider_alias = "Player".to_owned();
        let off = keys(&payload(&profile, &[], false, 0).to_line(&HudLayout::new()));

        for (flag, key) in MODULE_WIRE_KEYS {
            let on = keys(&payload(&profile, &[flag], false, 0).to_line(&HudLayout::new()));
            let moved: Vec<&str> = CONFIG_KEYS
                .iter()
                .copied()
                .filter(|candidate| on.get(*candidate) != off.get(*candidate))
                .collect();
            assert_eq!(
                moved,
                vec![key],
                "{flag:?} must move exactly {key} and nothing else"
            );
        }

        // The exceptions have no key at all, so their switches must leave the frame alone.
        for flag in MODULE_WIRE_KEY_EXCEPTIONS {
            let on = keys(&payload(&profile, &[flag], false, 0).to_line(&HudLayout::new()));
            assert_eq!(
                on, off,
                "{flag:?} feeds no key, so it cannot move the frame"
            );
        }
    }

    /// The seeding table, guarded from both ends: it covers exactly the switches the frame
    /// reads, every key it names is a boolean on the profile, and setting one key seeds
    /// exactly its own switch. A renamed profile field, a key paired with the wrong flag, or a
    /// switch with no saved field fails here rather than at connect, where the frame would
    /// quietly report the module off for a user who switched it on.
    #[test]
    fn every_switch_is_seeded_from_its_own_profile_field() {
        for (flag, _) in MODULE_WIRE_KEYS {
            assert!(
                SWITCH_PROFILE_KEYS
                    .iter()
                    .any(|(seeded, _)| *seeded == flag),
                "{flag:?} feeds a frame key but nothing seeds it from the profile"
            );
        }
        for (index, (flag, key)) in SWITCH_PROFILE_KEYS.iter().enumerate() {
            assert!(
                MODULE_WIRE_KEYS.iter().any(|(mapped, _)| mapped == flag),
                "{flag:?} is seeded but feeds no frame key"
            );
            assert!(
                !SWITCH_PROFILE_KEYS[..index]
                    .iter()
                    .any(|(_, other)| other == key),
                "{key} is read twice"
            );
        }

        let base: Value =
            serde_json::to_value(Profile::default()).expect("the default profile serialises");
        for (flag, key) in SWITCH_PROFILE_KEYS {
            let mut json = base.clone();
            match json.get_mut(key) {
                Some(slot) if slot.is_boolean() => *slot = Value::Bool(true),
                _ => panic!("{key} is not a boolean field on the profile"),
            }
            let profile =
                load_profile_from_json(&json.to_string()).expect("the seeded profile loads");
            assert_eq!(
                profile_switches(&profile),
                vec![flag],
                "{key} has to seed exactly {flag:?}"
            );
        }
    }
}
