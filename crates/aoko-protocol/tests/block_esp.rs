//! Port of `Aoko.Tests/BlockEspTests.cs`.
//!
//! The managed suite also touched `Clicker.Instance`, a WPF observable collection.
//! That binding layer belongs to `aoko-core`; the serialisation it exposed is covered
//! here directly. Profile persistence uses the real profile JSON codec.

use aoko_protocol::block_esp::{
    build_default_targets, normalize_color, normalize_id, parse_targets, serialize_targets,
    BlockEspTarget, BlockEspTargetData, DEFAULT_COLOR_HEX, MAX_SERIALIZED_LENGTH,
};
use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::profile::{load_profile_from_json, to_profile_json, Profile};

#[test]
fn normalize_id_reduces_to_path_token() {
    let cases = [
        ("minecraft:diamond_ore", "diamond_ore"),
        ("diamond_ore", "diamond_ore"),
        ("block.minecraft.diamond_ore", "diamond_ore"),
        ("  Minecraft:Diamond_Ore  ", "diamond_ore"),
        ("modid:custom_block", "custom_block"),
        ("", ""),
        ("   ", ""),
    ];

    for (input, expected) in cases {
        assert_eq!(normalize_id(Some(input)), expected, "input {input:?}");
    }

    assert_eq!(normalize_id(None), "");
}

#[test]
fn normalize_color_produces_six_digit_hex_or_default() {
    let cases = [
        (Some("#00e5ff"), "00E5FF"),
        (Some("00E5FF"), "00E5FF"),
        (Some("ffd700"), "FFD700"),
        (Some("xyz"), "00E5FF"),
        (Some("12345"), "00E5FF"),
        (None, "00E5FF"),
    ];

    for (input, expected) in cases {
        assert_eq!(normalize_color(input), expected, "input {input:?}");
    }

    assert_eq!(DEFAULT_COLOR_HEX, "00E5FF");
}

#[test]
fn serialize_only_includes_enabled_entries() {
    let targets = [
        BlockEspTarget::new("minecraft:diamond_ore", "Diamond Ore", Some("00E5FF"), true),
        BlockEspTarget::new("minecraft:gold_ore", "Gold Ore", Some("FFD700"), false),
        BlockEspTarget::new("minecraft:iron_ore", "Iron Ore", Some("D8AF93"), true),
    ];

    assert_eq!(
        serialize_targets(&targets),
        "diamond_ore=00E5FF;iron_ore=D8AF93"
    );
}

#[test]
fn serialize_dedupes_by_normalized_id() {
    let targets = [
        BlockEspTarget::new("minecraft:diamond_ore", "A", Some("00E5FF"), true),
        BlockEspTarget::new("diamond_ore", "B", Some("FFFFFF"), true),
    ];

    assert_eq!(serialize_targets(&targets), "diamond_ore=00E5FF");
}

#[test]
fn serialize_parse_round_trips() {
    let targets = [
        BlockEspTarget::new("minecraft:diamond_ore", "Diamond Ore", Some("00E5FF"), true),
        BlockEspTarget::new(
            "minecraft:ancient_debris",
            "Ancient Debris",
            Some("9B6A4A"),
            true,
        ),
    ];

    let serialized = serialize_targets(&targets);
    let parsed = parse_targets(Some(&serialized));

    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0], ("diamond_ore".to_owned(), "00E5FF".to_owned()));
    assert_eq!(
        parsed[1],
        ("ancient_debris".to_owned(), "9B6A4A".to_owned())
    );
}

#[test]
fn parse_skips_malformed_entries() {
    let parsed = parse_targets(Some("diamond_ore=00E5FF;;garbage;=AABBCC;gold_ore=FFD700"));

    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].0, "diamond_ore");
    assert_eq!(parsed[1].0, "gold_ore");
}

#[test]
fn parse_handles_a_missing_setting() {
    assert!(parse_targets(None).is_empty());
    assert!(parse_targets(Some("")).is_empty());
    assert!(parse_targets(Some("   ")).is_empty());
}

#[test]
fn serialize_caps_length() {
    let targets: Vec<BlockEspTarget> = (0..2000)
        .map(|index| {
            BlockEspTarget::new(
                format!("minecraft:block_{index}"),
                "x",
                Some("AABBCC"),
                true,
            )
        })
        .collect();

    let serialized = serialize_targets(&targets);

    assert!(
        serialized.len() <= MAX_SERIALIZED_LENGTH,
        "{}",
        serialized.len()
    );
}

#[test]
fn default_targets_have_diamond_ore_enabled_only() {
    let targets = build_default_targets();

    let enabled: Vec<&BlockEspTarget> = targets.iter().filter(|target| target.enabled()).collect();

    assert_eq!(enabled.len(), 1);
    assert_eq!(normalize_id(Some(enabled[0].registry_id())), "diamond_ore");
    assert_eq!(targets.len(), 20, "every preset is present");
}

#[test]
fn default_block_esp_serialization_reflects_defaults() {
    // What the managed `Clicker.BlockEspBlocksSerialized` produced for the defaults.
    assert_eq!(
        serialize_targets(&build_default_targets()),
        "diamond_ore=00E5FF"
    );
}

#[test]
fn profile_block_esp_defaults_contain_diamond_ore() {
    let profile = Profile::default();

    assert!(!profile.block_esp_enabled);
    assert!(profile.block_esp_boxes);
    assert_eq!(profile.block_esp_max_count, 64);
    assert_eq!(profile.block_esp_range, 4);
    assert!(profile
        .block_esp_targets
        .iter()
        .any(|target| normalize_id(Some(&target.registry_id)) == "diamond_ore" && target.enabled));
}

#[test]
fn profile_block_esp_serializes_round_trip() {
    let mut profile = Profile::default();
    profile.block_esp_enabled = true;
    profile.block_esp_tracers = true;
    profile.block_esp_max_count = 128;
    profile.block_esp_range = 6;
    profile.block_esp_targets = vec![
        BlockEspTargetData {
            registry_id: "minecraft:gold_ore".to_owned(),
            display_name: "Gold Ore".to_owned(),
            color_hex: "FFD700".to_owned(),
            enabled: true,
        },
        BlockEspTargetData {
            registry_id: "minecraft:iron_ore".to_owned(),
            display_name: "Iron Ore".to_owned(),
            color_hex: "D8AF93".to_owned(),
            enabled: false,
        },
    ];

    let json = to_profile_json(&profile);
    let round = load_profile_from_json(&json).expect("profile");

    assert!(round.block_esp_enabled);
    assert!(round.block_esp_tracers);
    assert_eq!(round.block_esp_max_count, 128);
    assert_eq!(round.block_esp_range, 6);
    assert_eq!(round.block_esp_targets.len(), 2);
    assert_eq!(round.block_esp_targets[0].registry_id, "minecraft:gold_ore");
    assert_eq!(round.block_esp_targets[0].color_hex, "FFD700");
    assert!(round.block_esp_targets[0].enabled);
    assert!(!round.block_esp_targets[1].enabled);
    assert!(
        json.contains("\"blockEspTargets\""),
        "camelCase key: {}",
        &json[..json.len().min(200)]
    );
}

#[test]
fn capabilities_expose_block_esp_on_all_versions() {
    let modern = BridgeCapabilities::for_version_fallback(Some("26.1"));
    let legacy = BridgeCapabilities::for_version_fallback(Some("1.8.9"));

    assert!(modern.supports_module("blockesp"));
    assert!(modern.supports_setting("blockespenabled"));
    assert!(modern.supports_setting("blockespblocks"));
    assert!(legacy.supports_module("blockesp"));
    assert!(legacy.supports_setting("blockespenabled"));
    assert!(legacy.supports_setting("blockespblocks"));
}

#[test]
fn target_data_converts_through_normalisation() {
    let data = BlockEspTargetData {
        registry_id: "minecraft:gold_ore".to_owned(),
        display_name: "Gold Ore".to_owned(),
        color_hex: "#ffd700".to_owned(),
        enabled: false,
    };

    let target = data.to_target();

    // The registry id is kept verbatim, exactly as `BlockEspTarget.RegistryId` is;
    // only the colour is normalised on construction. Serialisation is what reduces the
    // id to its path token.
    assert_eq!(target.registry_id(), "minecraft:gold_ore");
    assert_eq!(normalize_id(Some(target.registry_id())), "gold_ore");
    assert_eq!(target.color_hex(), "FFD700");
    assert!(!target.enabled());
}
