//! Port of `Aoko.Tests/FightStatusWiringTests.cs`.
//!
//! The managed suite mixed four source pins with one live toggle. The pins on
//! `GameStateClient.cs` and `bridge_capabilities.h` are kept because they describe the
//! wire, which is still ours to match. `LocalToggle` and `Panic_Reset` drive
//! `Clicker`/`InputHooks`, which belong to `aoko-core`, so their contract is pinned as
//! source text instead.
//!
//! The managed files are read from the frozen copies in `tests/fixtures/managed/` and
//! compiled in with `include_str!`; only `bridge_capabilities.h` is still read from the
//! repository, because the native bridge is still shipped and still writes the wire. See
//! that directory's README before touching either the fixtures or these pins.
//!
//! One pin was retired with the copies rather than kept: `Profile.cs` carried a
//! `ProfileManager` <-> `Clicker.FightStatusEnabled` mapping that this port never had, so
//! the test certified managed history instead of our code. Its only still-live fact — the
//! `fightstatus` keybind default — is asserted against `Profile::default()` below, and the
//! profile-to-payload wiring it was standing in for now has a real test in `aoko-core`
//! (`the_config_payload_carries_the_profile_fight_status`).

use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::config_payload::ConfigPayload;
use aoko_protocol::hud::HudLayout;
use aoko_protocol::profile::{load_profile_from_json, to_profile_json, Profile};

/// `Aoko/Core/GameStateClient.cs`, frozen at the commit named in
/// `tests/fixtures/managed/README.md`.
///
/// `include_str!` rather than a runtime read: the evidence lives inside the crate now, so
/// deleting `Aoko/` cannot take it, and removing the fixture is a compile error instead of
/// a panic inside `read_to_string` on the sign-off commit.
const GAME_STATE_CLIENT_CS: &str = include_str!("fixtures/managed/GameStateClient.cs");
/// `Aoko/Core/Clicker.cs`, frozen the same way.
const CLICKER_CS: &str = include_str!("fixtures/managed/Clicker.cs");

/// A native bridge source file, which is still shipped and still read from the repository.
fn native_file(relative: &str) -> String {
    let path = format!("{}/../../{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
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
    let source = GAME_STATE_CLIENT_CS;

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
    let header = native_file("McInjector/src/main/cpp/bridge_capabilities.h");
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

/// The managed panic reset, pinned as text because the behaviour now belongs to `aoko-core`.
///
/// **The two hosts carry fight status differently, and a reader of the C# below needs to
/// know which line of ours it means.** The managed host keeps one `Clicker.FightStatusEnabled`
/// flag: `LocalToggle` flips it through `InputHooks.ToggleModule` and `TriggerPanicAsync`
/// clears it, which is the `FightStatusEnabled = false;` this test finds. This port has no
/// such flag. Fight status here is the `"fightstatus"` module key, and the panic reset is
/// that key appearing in `PANIC_OFF` in `aoko-core/src/clicker/toggles.rs`, cleared by
/// `panic_reset`. So the line of ours this pin corresponds to is the `"fightstatus"` entry
/// in that list, not a field anywhere.
///
/// The pin is kept because the managed file is the only written record of *what* the reset
/// does; it is not evidence that a `FightStatusEnabled` flag exists in this port.
#[test]
fn local_toggle_and_panic_reset_are_pinned_in_the_managed_host() {
    let clicker = CLICKER_CS;
    let start = clicker
        .find("public async Task TriggerPanicAsync()")
        .expect("panic handler");
    let end = clicker
        .find("private void StartAimAssistLoop()")
        .unwrap_or(clicker.len());
    assert!(end > start);

    assert!(clicker[start..end].contains("FightStatusEnabled = false;"));
}
