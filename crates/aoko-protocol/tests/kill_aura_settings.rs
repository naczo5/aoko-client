//! Port of `Aoko.Tests/KillAuraSettingsTests.cs`.
//!
//! The managed suite used object-initialiser syntax; every one of those properties
//! is a validating setter, so here the same values go through `set_*`.

use aoko_protocol::kill_aura::KillAuraSettings;
use aoko_protocol::profile::{load_profile_from_json, to_profile_json, Profile};
use aoko_protocol::state::GameState;

#[test]
fn defaults_match_openmyau_plus_fc7c95d() {
    let settings = KillAuraSettings::default();

    assert_eq!(settings.cps_mode(), "normal");
    assert_eq!(settings.mode(), "switch");
    assert_eq!(settings.sort(), "health");
    assert_eq!(settings.auto_block(), "hypixel");
    assert_eq!(settings.attack_tick(), 0);
    assert_eq!(settings.auto_block_cps(), 8.0);
    assert_eq!(settings.auto_block_range(), 6.0);
    assert_eq!(settings.swing_range(), 3.5);
    assert_eq!(settings.attack_range(), 3.0);
    assert_eq!(settings.fov(), 360);
    assert_eq!(settings.min_cps(), 14);
    assert_eq!(settings.max_cps(), 14);
    assert_eq!(settings.switch_delay(), 150);
    assert_eq!(settings.rotations(), "silent");
    assert_eq!(settings.move_fix(), "silent");
    assert!(settings.players());
    assert!(settings.teams());
    assert!(!settings.mobs());
}

#[test]
fn profile_round_trip_preserves_nested_settings() {
    let mut profile = Profile::default();
    profile.kill_aura_settings.set_auto_block("morden");
    profile.kill_aura_settings.set_rotations("liquidbounce");
    profile.kill_aura_settings.set_attack_range(4.2);
    profile.kill_aura_settings.set_bosses(true);

    let json = to_profile_json(&profile);
    let restored = load_profile_from_json(&json).expect("profile");

    assert_eq!(restored.kill_aura_settings.auto_block(), "morden");
    assert_eq!(restored.kill_aura_settings.rotations(), "liquidbounce");
    assert_eq!(restored.kill_aura_settings.attack_range(), 4.2);
    assert!(restored.kill_aura_settings.bosses());
}

#[test]
fn clone_is_independent_and_retains_conditional_state() {
    let mut original = KillAuraSettings::default();
    original.set_rotations("hypixel");
    original.set_raven_predict_ticks(4);

    let mut clone = original.clone();
    clone.set_raven_predict_ticks(1);

    assert!(original.is_hypixel_rotation());
    assert_eq!(original.raven_predict_ticks(), 4);
    assert_eq!(clone.raven_predict_ticks(), 1);
}

#[test]
fn grok_rotation_is_accepted_and_exposes_skew() {
    let mut settings = KillAuraSettings::default();
    settings.set_rotations("grok");
    settings.set_grok_max_skew(15.0);

    assert_eq!(settings.rotations(), "grok");
    assert!(settings.is_grok_rotation());
    assert!(!settings.is_hypixel_rotation());
    assert_eq!(settings.grok_max_skew(), 15.0);

    settings.set_grok_max_skew(100.0);
    assert_eq!(settings.grok_max_skew(), 25.0);
    settings.set_grok_max_skew(1.0);
    assert_eq!(settings.grok_max_skew(), 6.0);

    settings.set_rotations("not-a-mode");
    assert_eq!(settings.rotations(), "none");
}

#[test]
fn every_mode_setter_rejects_values_outside_its_list() {
    let mut settings = KillAuraSettings::default();

    // `SetMode` falls back to the *first* allowed value, never to the previous one.
    assert!(settings.set_mode("nonsense"));
    assert_eq!(settings.mode(), "single");
    assert!(!settings.set_mode("single"), "already the fallback");
    assert!(settings.set_mode(" Switch "));
    assert_eq!(settings.mode(), "switch", "trimmed and lowercased");

    // `cpsMode` already *is* the first allowed value, so the fallback is a no-op.
    assert!(!settings.set_cps_mode("double"));
    assert_eq!(settings.cps_mode(), "normal");
    assert!(settings.set_cps_mode("record"));
    assert_eq!(settings.cps_mode(), "record");

    assert!(settings.set_sort("nonsense"));
    assert_eq!(settings.sort(), "distance");
    assert!(settings.set_move_fix("nonsense"));
    assert_eq!(settings.move_fix(), "none");
    // Both already hold their first allowed value, so the fallback is a no-op.
    assert!(!settings.set_show_target("nonsense"));
    assert_eq!(settings.show_target(), "none");
    assert!(!settings.set_debug_log("nonsense"));
    assert_eq!(settings.debug_log(), "none");
    assert!(settings.set_debug_log("health"));
    assert_eq!(settings.debug_log(), "health");
    assert!(settings.set_auto_block("nonsense"));
    assert_eq!(settings.auto_block(), "none");
}

#[test]
fn setters_report_whether_the_value_changed() {
    let mut settings = KillAuraSettings::default();

    assert!(!settings.set_fov(360), "already 360");
    assert!(settings.set_fov(180));
    assert!(!settings.set_fov(180));
    assert!(settings.set_fov(5), "clamped to 30, which differs from 180");
    assert_eq!(settings.fov(), 30);
}

#[test]
fn cps_pair_keeps_min_leq_max_in_both_directions() {
    let mut settings = KillAuraSettings::default();

    settings.set_min_cps(18);
    assert_eq!(
        settings.max_cps(),
        18,
        "raising the minimum raises the maximum"
    );

    settings.set_max_cps(4);
    assert_eq!(
        settings.min_cps(),
        4,
        "lowering the maximum lowers the minimum"
    );
}

#[test]
fn turn_speed_pair_keeps_min_leq_max_in_both_directions() {
    let mut settings = KillAuraSettings::default();

    settings.set_min_turn_speed(80.0);
    assert_eq!(settings.max_turn_speed(), 80.0);

    settings.set_max_turn_speed(10.0);
    assert_eq!(settings.min_turn_speed(), 10.0);
}

#[test]
fn body_point_pair_keeps_min_leq_max_in_both_directions() {
    let mut settings = KillAuraSettings::default();

    assert_eq!(settings.liquid_bounce_body_point_min(), 0.1);
    assert_eq!(settings.liquid_bounce_body_point_max(), 0.9);

    settings.set_liquid_bounce_body_point_min(0.8);
    assert_eq!(
        settings.liquid_bounce_body_point_max(),
        0.9,
        "already above the new minimum"
    );

    settings.set_liquid_bounce_body_point_min(1.0);
    assert_eq!(settings.liquid_bounce_body_point_max(), 1.0);

    settings.set_liquid_bounce_body_point_max(0.2);
    assert_eq!(settings.liquid_bounce_body_point_min(), 0.2);
}

#[test]
fn every_clamped_property_pins_its_bounds() {
    let mut settings = KillAuraSettings::default();

    settings.set_attack_tick(99);
    assert_eq!(settings.attack_tick(), 5);
    settings.set_attack_tick(-3);
    assert_eq!(settings.attack_tick(), 0);

    settings.set_auto_block_cps(0.0);
    assert_eq!(settings.auto_block_cps(), 1.0);
    settings.set_auto_block_range(99.0);
    assert_eq!(settings.auto_block_range(), 8.0);
    settings.set_swing_range(0.0);
    assert_eq!(settings.swing_range(), 3.0);
    settings.set_attack_range(99.0);
    assert_eq!(settings.attack_range(), 6.0);
    settings.set_switch_delay(5000);
    assert_eq!(settings.switch_delay(), 1000);
    settings.set_dead_zone_size(9.0);
    assert_eq!(settings.dead_zone_size(), 2.0);
    settings.set_acceleration(0.0);
    assert_eq!(settings.acceleration(), 0.1);
    settings.set_deceleration(99.0);
    assert_eq!(settings.deceleration(), 10.0);
    settings.set_overshoot_strength(99.0);
    assert_eq!(settings.overshoot_strength(), 20.0);
    settings.set_overshoot_recovery(0.0);
    assert_eq!(settings.overshoot_recovery(), 0.01);
    settings.set_noise_strength(99.0);
    assert_eq!(settings.noise_strength(), 2.0);
    settings.set_smoothing(1000);
    assert_eq!(settings.smoothing(), 100);
    settings.set_raven_smoothing(1000);
    assert_eq!(settings.raven_smoothing(), 10);
    settings.set_raven_predict_ticks(1000);
    assert_eq!(settings.raven_predict_ticks(), 5);
    settings.set_raven_yaw_random(1000);
    assert_eq!(settings.raven_yaw_random(), 5);
    settings.set_angle_step(1);
    assert_eq!(settings.angle_step(), 30);
    settings.set_randomize_range(9.0);
    assert_eq!(settings.randomize_range(), 1.0);
    settings.set_y_randomize_strength(9.0);
    assert_eq!(settings.y_randomize_strength(), 1.0);
    settings.set_liquid_bounce_horizontal_speed(1.0);
    assert_eq!(settings.liquid_bounce_horizontal_speed(), 1.0);
    settings.set_liquid_bounce_vertical_speed(999.0);
    assert_eq!(settings.liquid_bounce_vertical_speed(), 180.0);
    settings.set_liquid_bounce_smooth_factor(0.0);
    assert_eq!(settings.liquid_bounce_smooth_factor(), 0.1);
    settings.set_liquid_bounce_predict_size(99.0);
    assert_eq!(settings.liquid_bounce_predict_size(), 3.0);
    settings.set_liquid_bounce_randomize_range(99.0);
    assert_eq!(settings.liquid_bounce_randomize_range(), 1.0);
    settings.set_liquid_bounce_horizontal_search(99.0);
    assert_eq!(settings.liquid_bounce_horizontal_search(), 1.0);
}

#[test]
fn derived_flags_track_the_mode_they_describe() {
    let mut settings = KillAuraSettings::default();

    assert!(!settings.is_liquid_bounce());
    assert!(!settings.overshoot_controls_enabled());

    settings.set_rotations("liquidbounce");
    assert!(settings.is_liquid_bounce());
    // The managed defaults are `UseOvershoot = true` and `Randomize = true`, so both
    // controls become live as soon as the rotation is liquidbounce.
    assert!(settings.overshoot_controls_enabled());
    assert!(settings.randomize_controls_enabled());

    settings.set_liquid_bounce_predict(true);
    settings.set_liquid_bounce_randomize(true);

    assert!(settings.overshoot_controls_enabled());
    assert!(settings.randomize_controls_enabled());
    assert!(settings.predict_controls_enabled());
    assert!(settings.liquid_randomize_controls_enabled());

    settings.set_rotations("silent");
    assert!(
        !settings.overshoot_controls_enabled(),
        "gated on the rotation"
    );
    assert!(!settings.randomize_controls_enabled());
}

#[test]
fn allow_tools_enabled_mirrors_weapons_only() {
    let mut settings = KillAuraSettings::default();

    settings.set_weapons_only(false);
    assert!(!settings.allow_tools_enabled());
    settings.set_weapons_only(true);
    assert!(settings.allow_tools_enabled());
}

#[test]
fn is_swap_auto_block_tracks_the_auto_block_mode() {
    let mut settings = KillAuraSettings::default();

    assert!(!settings.is_swap_auto_block());
    settings.set_auto_block("swap");
    assert!(settings.is_swap_auto_block());
}

#[test]
fn copy_from_runs_every_setter() {
    let mut source = KillAuraSettings::default();
    source.set_rotations("liquidbounce");
    source.set_grok_max_skew(25.0);

    let mut target = KillAuraSettings::default();
    target.copy_from(&source);

    assert_eq!(target, source);
}

#[test]
fn wire_round_trip_keeps_every_data_field() {
    let mut settings = KillAuraSettings::default();
    settings.set_rotations("liquidbounce");
    settings.set_grok_max_skew(20.0);
    settings.set_auto_block("swap");
    settings.set_bosses(true);
    settings.set_fov(120);

    let json = serde_json::to_string(&settings).expect("serialise");
    let restored: KillAuraSettings = serde_json::from_str(&json).expect("deserialise");

    assert_eq!(restored, settings);
    assert!(json.contains("\"grokMaxSkew\":20"), "{json}");
    assert!(json.contains("\"rotations\":\"liquidbounce\""), "{json}");
}

#[test]
fn wire_deserialisation_clamps_like_the_setters() {
    let json = r#"{"grokMaxSkew":999.0,"fov":1,"rotations":"nonsense","minCps":19}"#;

    let restored: KillAuraSettings = serde_json::from_str(json).expect("deserialise");

    assert_eq!(restored.grok_max_skew(), 25.0);
    assert_eq!(restored.fov(), 30);
    assert_eq!(restored.rotations(), "none");
    assert_eq!(restored.min_cps(), 19);
    assert_eq!(restored.max_cps(), 19);
}

#[test]
fn game_state_deserializes_dynamic_unavailable_reason() {
    let state: GameState = serde_json::from_str(
        r#"{"killAuraUnavailableReason":"Held sword does not support native blocking."}"#,
    )
    .expect("state");

    assert_eq!(
        state.kill_aura_unavailable_reason,
        "Held sword does not support native blocking."
    );
}
