//! Port of `Aoko.Tests/GameStateAndProfileTests.cs`.
//!
//! Two managed cases are out of scope for this crate and live elsewhere:
//! `ChestStealerCoordinateMapper_*` (Win32 client-rect scaling — `aoko-platform`) and
//! `ThemeManager_ContainsCuratedThemes` (palette tables — `aoko-core`).
//! `ProfileManager_*` splits: the directory helpers are pure path building and are
//! pinned here, while `InitializeProfileDirectory`/`TryWriteProfile` do file I/O and
//! belong to `aoko-platform`.

use aoko_protocol::config_payload::{module_list_style_to_index, ConfigPayload};
use aoko_protocol::hud::HudLayout;
use aoko_protocol::profile::{
    get_legacy_profiles_directory, get_profiles_directory, load_profile_from_json, to_profile_json,
    Profile,
};
use aoko_protocol::state::{ChestStealerSlot, ChestStealerState, EntityInfo, GameState};
use serde_json::Value;
use std::path::Path;

fn state(text: &str) -> GameState {
    serde_json::from_str(text).expect("state decodes")
}

fn profile_node(profile: &Profile) -> Value {
    serde_json::from_str(&to_profile_json(profile)).expect("profile json")
}

#[test]
fn game_state_deserializes_expected_contract() {
    let state = state(
        r#"{
          "mapped": true,
          "guiOpen": false,
          "screenName": "HUD",
          "actionBar": "The theme is C_T",
          "health": 18.5,
          "viewportWidth": 1920,
          "viewportHeight": 1001,
          "entities": [
            { "sx": 120.5, "sy": 44.2, "dist": 3.25, "name": "Steve", "hp": 20.0 }
          ]
        }"#,
    );

    assert!(state.mapped);
    assert!(!state.gui_open);
    assert_eq!(state.screen_name, "HUD");
    assert_eq!(state.action_bar, "The theme is C_T");
    assert_eq!(state.health, 18.5);
    assert_eq!(state.viewport_width, 1920);
    assert_eq!(state.viewport_height, 1001);
    assert_eq!(state.entities.len(), 1);
    assert_eq!(state.entities[0].name, "Steve");
    assert_eq!(state.entities[0].distance(), 3.25);
}

#[test]
fn game_state_value_deserialization_matches_string_deserialization() {
    for hud_layout in [
        "",
        r#", "hudLayout": { "moduleList": { "x": 14, "y": 28 } }"#,
    ] {
        let json = format!(
            r#"{{
              "mapped": true,
              "guiOpen": false,
              "screenName": "HUD",
              "actionBar": "The theme is C_T",
              "health": 18.5,
              "viewportWidth": 1920,
              "viewportHeight": 1001,
              "entities": [
                {{ "sx": 120.5, "sy": 44.2, "dist": 3.25, "name": "Steve", "hp": 20.0 }}
              ]
              {hud_layout}
            }}"#
        );

        let from_string = state(&json);
        let node: Value = serde_json::from_str(&json).expect("json");
        let from_node: GameState = serde_json::from_value(node.clone()).expect("value to state");

        assert_eq!(from_string.mapped, from_node.mapped);
        assert_eq!(from_string.gui_open, from_node.gui_open);
        assert_eq!(from_string.screen_name, from_node.screen_name);
        assert_eq!(from_string.action_bar, from_node.action_bar);
        assert_eq!(from_string.health, from_node.health);
        assert_eq!(from_string.viewport_width, from_node.viewport_width);
        assert_eq!(from_string.viewport_height, from_node.viewport_height);
        assert_eq!(from_string.entities.len(), from_node.entities.len());
        assert_eq!(from_string.entities[0].name, from_node.entities[0].name);
        assert_eq!(
            from_string.entities[0].distance(),
            from_node.entities[0].distance()
        );
        assert_eq!(!hud_layout.is_empty(), node.get("hudLayout").is_some());
    }
}

#[test]
fn game_state_deserializes_chest_stealer_state() {
    let state = state(
        r#"{
          "mapped": true,
          "guiOpen": true,
          "screenName": "GuiChest",
          "chestStealerState": {
            "ready": true,
            "physical": true,
            "windowId": 2,
            "screenWidth": 854,
            "screenHeight": 480,
            "slots": [
              { "index": 4, "slotNumber": 4, "x": 381, "y": 128 }
            ]
          },
          "entities": []
        }"#,
    );

    let chest = state.chest_stealer_state.expect("chest stealer state");
    assert!(chest.ready);
    assert!(chest.physical);
    assert_eq!(chest.window_id, 2);
    assert_eq!(chest.slots.len(), 1);
    assert_eq!(chest.slots[0].x, 381);
}

#[test]
fn chest_stealer_state_deserializes_without_a_refill_payload() {
    // The slot and state defaults are the ones the native writer omits when they are
    // zero, so a sparse payload must still produce a usable snapshot.
    let state = state(r#"{"chestStealerState":{"ready":true,"slots":[{"slotNumber":9}]}}"#);

    let chest = state.chest_stealer_state.expect("chest stealer state");
    assert!(chest.ready);
    assert_eq!(
        chest.window_id, -1,
        "the managed default for a missing window"
    );
    assert_eq!(chest.slots[0].slot_number, 9);
    assert_eq!(chest.slots[0].index, 0);

    let empty = ChestStealerState::default();
    let slot = ChestStealerSlot {
        index: 0,
        slot_number: 0,
        x: 0,
        y: 0,
    };
    assert!(!empty.ready);
    assert!(empty.slots.is_empty());
    assert_eq!(slot.slot_number, 0);
    assert_eq!(EntityInfo::default().name, "");
}

#[test]
fn profile_default_values_remain_stable() {
    let profile = Profile::default();

    assert_eq!(profile.name, "Default");
    assert_eq!(profile.min_cps, 8.0);
    assert_eq!(profile.max_cps, 12.0);
    assert!(profile.left_click_enabled);
    assert!(!profile.triggerbot_enabled);
    assert!(!profile.kill_aura_enabled);
    assert_eq!(profile.kill_aura_range, 2.9);
    assert_eq!(profile.kill_aura_aim_range, 3.5);
    assert_eq!(profile.kill_aura_min_turn_speed, 10.0);
    assert_eq!(profile.kill_aura_max_turn_speed, 20.0);
    assert_eq!(profile.kill_aura_target_mode, "distance");
    assert_eq!(profile.kill_aura_switch_delay_ms, 350);
    assert_eq!(profile.kill_aura_randomization, 18);
    assert_eq!(profile.kill_aura_click_mode, "cooldown");
    assert_eq!(profile.kill_aura_min_cps, 10.0);
    assert_eq!(profile.kill_aura_max_cps, 14.0);
    assert_eq!(profile.kill_aura_fov, 120);
    assert_eq!(profile.kill_aura_rot_mode, "legit");
    assert!(!profile.kill_aura_require_press);
    assert!(profile.kill_aura_weapons_only);
    assert!(!profile.chest_stealer_enabled);
    assert_eq!(profile.chest_stealer_delay_ms, 120);
    assert!(profile.chest_stealer_menu_check());
    assert!(profile.chest_stealer_title_check());
    assert!(profile.chest_stealer_custom_items_check());
    assert!(profile.chest_stealer_physical_check());
    assert_eq!(profile.reach_chance, 100);
    assert_eq!(profile.module_keys.get("autoclicker"), Some(&0));
    assert!(profile.module_keys.contains_key("cheststealer"));
}

#[test]
fn profile_chest_stealer_menu_check_defaults_true_and_round_trips() {
    assert!(Profile::default().chest_stealer_menu_check());

    let mut profile = Profile::default();
    assert!(profile.set_chest_stealer_menu_check(false));

    let node = profile_node(&profile);
    assert_eq!(node["chestStealerMenuCheck"], false);

    let round = load_profile_from_json(&to_profile_json(&profile)).expect("profile");
    assert!(!round.chest_stealer_menu_check());
}

#[test]
fn profile_chest_stealer_sub_checks_default_true_and_round_trip() {
    let defaults = Profile::default();
    assert!(defaults.chest_stealer_title_check());
    assert!(defaults.chest_stealer_custom_items_check());
    assert!(defaults.chest_stealer_physical_check());

    let mut profile = Profile::default();
    profile.set_chest_stealer_title_check(false);
    profile.set_chest_stealer_custom_items_check(true);
    profile.set_chest_stealer_physical_check(false);

    let node = profile_node(&profile);
    assert_eq!(node["chestStealerTitleCheck"], false);
    assert_eq!(node["chestStealerCustomItemsCheck"], true);
    assert_eq!(node["chestStealerPhysicalCheck"], false);

    let round = load_profile_from_json(&to_profile_json(&profile)).expect("profile");
    assert!(!round.chest_stealer_title_check());
    assert!(round.chest_stealer_custom_items_check());
    assert!(!round.chest_stealer_physical_check());
}

#[test]
fn profile_legacy_menu_check_sets_sub_checks_when_omitted() {
    // The setter cascade from `WIRE_CONTRACT.md`: clearing the menu check clears the
    // three sub-checks it gates, even when the document omits them.
    let legacy_json = r#"{
        "name": "Legacy",
        "chestStealerMenuCheck": false
    }"#;

    let profile = load_profile_from_json(legacy_json).expect("profile");

    assert_eq!(profile.name, "Legacy");
    assert!(!profile.chest_stealer_menu_check());
    assert!(!profile.chest_stealer_title_check());
    assert!(!profile.chest_stealer_custom_items_check());
    assert!(!profile.chest_stealer_physical_check());
}

#[test]
fn profile_menu_check_setter_restores_the_sub_checks() {
    let mut profile = Profile::default();
    profile.set_chest_stealer_menu_check(false);
    assert!(!profile.chest_stealer_title_check());

    profile.set_chest_stealer_menu_check(true);
    assert!(profile.chest_stealer_menu_check());
    assert!(profile.chest_stealer_title_check());
    assert!(profile.chest_stealer_custom_items_check());
    assert!(profile.chest_stealer_physical_check());
}

#[test]
fn profile_camel_case_serialization_uses_expected_keys() {
    let mut profile = Profile::default();
    profile.name = "PvP".to_owned();
    profile.min_cps = 9.5;
    profile.triggerbot_enabled = true;
    profile.kill_aura_enabled = true;

    let node = profile_node(&profile);

    // `.NET`'s camelCase policy lowercases only the leading run of capitals, so a
    // trailing `CPS` stays upper case: the profile file has `minCPS`, not `minCps`.
    assert!(node.get("minCPS").is_some(), "{node}");
    assert!(node.get("minCps").is_none());
    assert_eq!(node["name"], "PvP");
    assert_eq!(node["minCPS"], 9.5);
    assert_eq!(node["triggerbotEnabled"], true);
    assert_eq!(node["killAuraEnabled"], true);
}

#[test]
fn profile_fight_status_defaults_disabled_and_serializes_round_trip() {
    let defaults = Profile::default();
    assert!(!defaults.fight_status_enabled);
    assert_eq!(defaults.module_keys.get("fightstatus"), Some(&0));

    let mut profile = Profile::default();
    profile.fight_status_enabled = true;

    let node = profile_node(&profile);
    assert_eq!(node["fightStatusEnabled"], true);

    let round = load_profile_from_json(&to_profile_json(&profile)).expect("profile");
    assert!(round.fight_status_enabled);
}

#[test]
fn profile_anti_debuff_defaults_false() {
    assert!(!Profile::default().anti_debuff_enabled);
}

#[test]
fn profile_anti_debuff_serializes_round_trip() {
    let mut profile = Profile::default();
    profile.anti_debuff_enabled = true;

    let node = profile_node(&profile);
    assert_eq!(node["antiDebuffEnabled"], true);

    let round = load_profile_from_json(&to_profile_json(&profile)).expect("profile");
    assert!(round.anti_debuff_enabled);
}

#[test]
fn profile_new_modules_default_false_and_serialize_round_trip() {
    #[derive(Clone, Copy)]
    enum Field {
        FastPlace,
        NoHurtCam,
        Sprint,
        TntTimer,
        WTap,
        JumpReset,
        HitSelect,
        Trajectories,
        MurderMystery,
        BreakProgress,
    }

    let cases = [
        (Field::FastPlace, "fastPlaceEnabled"),
        (Field::NoHurtCam, "noHurtCamEnabled"),
        (Field::Sprint, "sprintEnabled"),
        (Field::TntTimer, "tntTimerEnabled"),
        (Field::WTap, "wTapEnabled"),
        (Field::JumpReset, "jumpResetEnabled"),
        (Field::HitSelect, "hitSelectEnabled"),
        (Field::Trajectories, "trajectoriesEnabled"),
        (Field::MurderMystery, "murderMysteryEnabled"),
        (Field::BreakProgress, "breakProgressEnabled"),
    ];

    for (field, json_key) in cases {
        let is_enabled = |profile: &Profile| match field {
            Field::FastPlace => profile.fast_place_enabled,
            Field::NoHurtCam => profile.no_hurt_cam_enabled,
            Field::Sprint => profile.sprint_enabled,
            Field::TntTimer => profile.tnt_timer_enabled,
            Field::WTap => profile.wtap_enabled,
            Field::JumpReset => profile.jump_reset_enabled,
            Field::HitSelect => profile.hit_select_enabled,
            Field::Trajectories => profile.trajectories_enabled,
            Field::MurderMystery => profile.murder_mystery_enabled,
            Field::BreakProgress => profile.break_progress_enabled,
        };
        let set_enabled = |profile: &mut Profile, value: bool| match field {
            Field::FastPlace => profile.fast_place_enabled = value,
            Field::NoHurtCam => profile.no_hurt_cam_enabled = value,
            Field::Sprint => profile.sprint_enabled = value,
            Field::TntTimer => profile.tnt_timer_enabled = value,
            Field::WTap => profile.wtap_enabled = value,
            Field::JumpReset => profile.jump_reset_enabled = value,
            Field::HitSelect => profile.hit_select_enabled = value,
            Field::Trajectories => profile.trajectories_enabled = value,
            Field::MurderMystery => profile.murder_mystery_enabled = value,
            Field::BreakProgress => profile.break_progress_enabled = value,
        };

        let default_profile = Profile::default();
        assert!(
            !is_enabled(&default_profile),
            "{json_key} must default false"
        );

        let mut enabled_profile = Profile::default();
        set_enabled(&mut enabled_profile, true);

        let node = profile_node(&enabled_profile);
        assert_eq!(node[json_key], true, "{json_key}");

        let round = load_profile_from_json(&to_profile_json(&enabled_profile)).expect("profile");
        assert!(is_enabled(&round), "{json_key} must survive the round trip");
    }
}

#[test]
fn config_payload_defaults_dash_the_gtb_strings() {
    // `Clicker` initialises `_gtbCurrentHint` and `_gtbMatchesPreview` to `"-"`, not to
    // an empty string: the shipped UI renders a dash until the solver has a hint, and
    // the bridge parses whatever config it is given.
    let payload = ConfigPayload::default();

    assert_eq!(payload.gtb_hint, "-");
    assert_eq!(payload.gtb_preview, "-");
    assert_eq!(payload.gtb_count, 0);

    let line = payload.to_line(&HudLayout::default());
    assert!(
        line.contains("\"gtbHint\":\"-\""),
        "{}",
        &line[..line.len().min(200)]
    );
    assert!(
        line.contains("\"gtbPreview\":\"-\""),
        "{}",
        &line[..line.len().min(200)]
    );
    assert!(
        line.contains("\"gtbCount\":0"),
        "{}",
        &line[..line.len().min(200)]
    );
}

#[test]
fn profile_directories_use_the_aoko_and_legacy_folders() {
    let app_data = Path::new("/tmp/aoko-appdata");

    assert_eq!(
        get_profiles_directory(app_data),
        app_data.join("Aoko").join("profiles")
    );
    assert_eq!(
        get_legacy_profiles_directory(app_data),
        app_data.join("LegoClicker").join("profiles")
    );
}

#[test]
fn profile_unknown_fields_are_ignored_and_missing_fields_default() {
    let profile = load_profile_from_json(
        r#"{"name":"Kept","futureField":{"nested":1},"futureList":[1,2,3]}"#,
    )
    .expect("profile");

    assert_eq!(profile.name, "Kept");
    assert_eq!(profile.min_cps, 8.0, "missing fields keep their default");
    assert!(profile.left_click_enabled);

    let round = load_profile_from_json(&to_profile_json(&profile)).expect("profile");
    assert_eq!(round.name, "Kept");
}

#[test]
fn existing_profiles_round_trip_byte_stable() {
    // Indented output plus a stable key order means an untouched profile is rewritten
    // identically, which is what lets the shipped app reload its own file.
    let mut profile = Profile::default();
    profile.name = "Round Trip".to_owned();
    profile.kill_aura_settings.set_rotations("grok");
    profile.set_auto_rod_slot_mode(2);

    let first = to_profile_json(&profile);
    let reloaded = load_profile_from_json(&first).expect("profile");
    let second = to_profile_json(&reloaded);

    assert_eq!(first, second);
    assert!(first.contains("\n  \""), "output is indented");
    // `File.WriteAllText` writes the serialiser output verbatim, and
    // `System.Text.Json` does not terminate with a newline.
    assert!(!first.ends_with('\n'), "no trailing newline");
    assert!(first.ends_with('}'), "one JSON object, no wrapper");
}

#[test]
fn module_list_style_to_index_maps_all_styles() {
    let cases = [
        ("Default", 0),
        ("default", 0),
        ("Minimal", 1),
        ("minimal", 1),
        ("Outlined", 2),
        ("outlined", 2),
        ("Glass", 3),
        ("glass", 3),
        ("Bold", 4),
        ("bold", 4),
        ("Classic", 5),
        ("classic", 5),
        ("Modern", 6),
        ("modern", 6),
        ("Unknown", 0),
    ];

    for (style, expected) in cases {
        assert_eq!(
            module_list_style_to_index(Some(style)),
            expected,
            "style {style:?}"
        );
    }

    assert_eq!(module_list_style_to_index(None), 0);
}

#[test]
fn auto_rod_slot_mode_clamps_like_the_managed_setter() {
    let mut profile = Profile::default();

    profile.set_auto_rod_slot_mode(99);
    assert_eq!(profile.auto_rod_slot_mode(), 9, "clamped to the hotbar");
    assert!(profile.auto_rod_extension_ticks() == 0 || profile.auto_rod_extension_ticks() > 0);
}
