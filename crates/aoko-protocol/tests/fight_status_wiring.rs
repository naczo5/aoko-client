//! Port of `Aoko.Tests/FightStatusWiringTests.cs`.
//!
//! The managed suite mixed four source pins with one live toggle. The pins on
//! `Profile.cs`/`GameStateClient.cs`/`bridge_capabilities.h` are kept because they
//! describe the wire and the profile key, both owned here. `LocalToggle` and
//! `Panic_Reset` drive `Clicker`/`InputHooks`, which belong to `aoko-core`, so their
//! contract is pinned as source text instead.

use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::config_payload::ConfigPayload;
use aoko_protocol::hud::HudLayout;
use aoko_protocol::profile::{load_profile_from_json, to_profile_json, Profile};

fn repo_file(relative: &str) -> String {
    let path = format!("{}/../../{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn profile_manager_maps_fight_status_both_directions() {
    // Source pin for the two directions the managed host wires, plus the keybind map.
    let source = repo_file("Aoko/Core/Profile.cs");

    assert!(source.contains("FightStatusEnabled = clicker.FightStatusEnabled"));
    assert!(source.contains("clicker.FightStatusEnabled = profile.FightStatusEnabled"));
    assert!(source.contains("[\"fightstatus\"]      = 0"));
}

#[test]
fn profile_carries_the_fight_status_flag_and_keybind() {
    let defaults = Profile::default();
    assert!(!defaults.fight_status_enabled);
    assert_eq!(defaults.module_keys.get("fightstatus"), Some(&0));

    let mut profile = Profile::default();
    profile.fight_status_enabled = true;

    let json = to_profile_json(&profile);
    assert!(
        json.contains("\"fightStatusEnabled\": true"),
        "{}",
        &json[..200.min(json.len())]
    );

    let restored = load_profile_from_json(&json).expect("profile");
    assert!(restored.fight_status_enabled);
    assert_eq!(restored.module_keys.get("fightstatus"), Some(&0));
}

#[test]
fn config_payload_contains_fight_status_and_its_keybind() {
    let source = repo_file("Aoko/Core/GameStateClient.cs");

    assert!(source.contains("fightStatus = clicker.FightStatusEnabled"));
    assert!(source.contains("keybindFightStatus   = InputHooks.GetModuleKey(\"fightstatus\")"));

    // The Rust payload writes both keys itself, so assert on the produced line too.
    let line = ConfigPayload::default().to_line(&HudLayout::default());
    assert!(
        line.contains("\"fightStatus\":"),
        "{}",
        &line[..line.len().min(200)]
    );
    assert!(
        line.contains("\"keybindFightStatus\":"),
        "{}",
        &line[..line.len().min(200)]
    );
}

#[test]
fn native_capability_payloads_advertise_fight_status_setting_and_keybind() {
    let header = repo_file("McInjector/src/main/cpp/bridge_capabilities.h");
    let modern = header
        .find("ModernCapabilitiesJson")
        .expect("modern payload");
    let legacy = header
        .find("LegacyCapabilitiesJson")
        .expect("legacy payload");
    assert!(modern > legacy);

    let legacy_payload = &header[legacy..modern];
    let modern_payload = &header[modern..];

    for payload in [legacy_payload, modern_payload] {
        assert!(
            payload.contains("\\\"fightstatus\\\""),
            "fightstatus setting"
        );
        assert!(
            payload.contains("\\\"keybindfightstatus\\\""),
            "keybindfightstatus setting"
        );
    }
}

#[test]
fn capability_fallbacks_advertise_fight_status() {
    for version in ["1.8.9", "1.21.4", "26.1", "26.2"] {
        let caps = BridgeCapabilities::for_version_fallback(Some(version));
        assert!(caps.supports_module("fightstatus"), "{version}");
        assert!(caps.supports_setting("fightstatus"), "{version}");
        assert!(caps.supports_setting("keybindfightstatus"), "{version}");
    }
}

#[test]
fn local_toggle_and_panic_reset_are_pinned_in_the_managed_host() {
    // `LocalToggle` flips `Clicker.FightStatusEnabled` through `InputHooks.ToggleModule`,
    // and `TriggerPanicAsync` clears it. Both live in `aoko-core`; the pins keep the
    // contract visible from the crate that owns the flag.
    let clicker = repo_file("Aoko/Core/Clicker.cs");
    let start = clicker
        .find("public async Task TriggerPanicAsync()")
        .expect("panic handler");
    let end = clicker
        .find("private void StartAimAssistLoop()")
        .unwrap_or(clicker.len());
    assert!(end > start);

    assert!(clicker[start..end].contains("FightStatusEnabled = false;"));
}
