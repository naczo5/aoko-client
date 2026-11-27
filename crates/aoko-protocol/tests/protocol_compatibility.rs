//! Port of `Aoko.Tests/ProtocolCompatibilityTests.cs`.
//!
//! Representative V1 messages captured from both bridge families. The fixtures keep
//! an unknown future field so loader/bridge version skew cannot silently become a
//! protocol break.

use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::state::GameState;
use serde_json::Value;

const LEGACY_STATE_FIXTURE: &str = r#"{
  "type": "state",
  "mapped": true,
  "inWorld": true,
  "guiOpen": true,
  "screenName": "GuiChest",
  "actionBar": "\u00a7aReady",
  "health": 19.5,
  "posX": 12.25,
  "posY": 64.0,
  "posZ": -3.75,
  "pitch": 8.0,
  "holdingBlock": true,
  "lookingAtBlock": true,
  "lookingAtEntity": false,
  "lookingAtEntityLatched": false,
  "breakingBlock": false,
  "attackCooldown": 0.8,
  "attackCooldownPerTick": 0.08,
  "killAuraUnavailableReason": "",
  "killAuraHasTarget": false,
  "killAuraBlocking": false,
  "stateMs": 123456,
  "chestStealerState": {
    "ready": true,
    "physical": true,
    "windowId": 4,
    "screenWidth": 854,
    "screenHeight": 480,
    "slots": [
      { "index": 4, "slotNumber": 4, "x": 381, "y": 128 }
    ]
  },
  "pixelPartyTargetFound": false,
  "pixelPartyTargetYaw": 0.0,
  "pixelPartyTargetDist": -1.0,
  "pixelPartyYawDelta": 0.0,
  "entities": [
    { "sx": 401.5, "sy": 219.0, "dist": 3.25, "name": "Steve", "hp": 20.0,
      "x": 14.0, "y": 64.0, "z": -5.0 }
  ]
}"#;

const MODERN_STATE_FIXTURE: &str = r#"{
  "type": "state",
  "inWorld": true,
  "guiOpen": false,
  "screenName": "",
  "actionBar": "",
  "health": 20.0,
  "posX": 0.0,
  "posY": 0.0,
  "posZ": 0.0,
  "fov": 70.0,
  "viewportWidth": 1920,
  "viewportHeight": 1080,
  "holdingBlock": false,
  "lookingAtBlock": false,
  "lookingAtEntity": true,
  "lookingAtEntityLatched": true,
  "breakingBlock": false,
  "stateMs": 987654,
  "attackCooldown": 1.0,
  "attackCooldownPerTick": 0.08,
  "killAuraUnavailableReason": "",
  "killAuraHasTarget": true,
  "killAuraBlocking": false,
  "chestStealerState": null,
  "pixelPartyTargetFound": true,
  "pixelPartyTargetYaw": 181.25,
  "pixelPartyTargetDist": 6.5,
  "pixelPartyYawDelta": -12.5,
  "entities": [
    { "sx": 960.0, "sy": 540.0, "dist": 3.0, "name": "Alex", "hp": 18.0 }
  ],
  "futureStateField": { "schema": 2 }
}"#;

fn state(text: &str) -> GameState {
    serde_json::from_str(text).expect("state fixture decodes")
}

#[test]
fn legacy_bridge_fixture_deserializes_complete_v1_state() {
    let state = state(LEGACY_STATE_FIXTURE);

    assert!(state.mapped);
    assert!(state.in_world);
    assert!(state.gui_open);
    assert_eq!(state.screen_name, "GuiChest");
    assert_eq!(state.health, 19.5);
    assert_eq!(state.pos_x, 12.25);
    assert!(state.holding_block);
    assert_eq!(state.state_ms, 123456);

    let chest = state.chest_stealer_state.expect("chest stealer state");
    assert_eq!(chest.slots.len(), 1);
    assert_eq!(chest.window_id, 4);
    assert_eq!(chest.screen_width, 854);

    assert_eq!(state.entities.len(), 1);
    assert_eq!(state.entities[0].name, "Steve");
    assert_eq!(state.entities[0].z, -5.0);
    assert_eq!(state.entities[0].sx, 401.5);
    assert_eq!(state.entities[0].distance(), 3.25);
}

#[test]
fn modern_bridge_fixture_deserializes_and_ignores_unknown_future_fields() {
    let state = state(MODERN_STATE_FIXTURE);

    assert!(state.in_world);
    assert!(!state.gui_open);
    assert_eq!(state.viewport_width, 1920);
    assert_eq!(state.viewport_height, 1080);
    assert!(state.looking_at_entity_latched);
    assert!(state.kill_aura_has_target);
    assert!(state.pixel_party_target_found);
    assert_eq!(state.pixel_party_yaw_delta, -12.5);
    assert!(state.chest_stealer_state.is_none());
    assert_eq!(state.entities.len(), 1);
    assert_eq!(state.entities[0].name, "Alex");
}

#[test]
fn loader_newer_bridge_older_absent_optional_fields_use_safe_defaults() {
    let state = state(
        r#"{ "type": "state", "mapped": true, "inWorld": false, "guiOpen": false,
             "screenName": "unknown", "actionBar": "", "health": -1, "entities": [] }"#,
    );

    assert!(!state.in_world);
    assert_eq!(state.health, -1.0);
    assert_eq!(state.fov, 70.0);
    assert_eq!(state.attack_cooldown, 1.0);
    assert_eq!(state.attack_cooldown_per_tick, 0.08);
    assert!(!state.looking_at_entity_latched);
    assert!(state.entities.is_empty());
}

#[test]
fn bridge_newer_loader_older_unknown_capability_entries_remain_tolerated() {
    let fallback = BridgeCapabilities::for_version_fallback(Some("26.2"));
    let payload: Value = serde_json::from_str(
        r#"{
          "type": "capabilities",
          "modules": ["TRIGGERBOT", "future_module"],
          "settings": ["AimAssistFov", "future_setting"],
          "state": ["ActionBar", "future_state_field"],
          "futureCapabilityBlock": { "version": 2 }
        }"#,
    )
    .expect("capabilities fixture");

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert!(parsed.supports_module("triggerbot"));
    assert!(parsed.supports_setting("aimassistfov"));
    assert!(parsed.supports_state_field("actionbar"));
    assert!(parsed.supports_module("future_module"));
    assert!(parsed.supports_setting("future_setting"));
    assert!(parsed.supports_state_field("future_state_field"));

    // An omitted array retains the known fallback surface.
    let sparse_payload: Value =
        serde_json::from_str(r#"{ "type": "capabilities", "modules": ["triggerbot"] }"#)
            .expect("sparse fixture");
    let sparse = BridgeCapabilities::from_payload(&sparse_payload, &fallback);

    assert!(sparse.supports_setting("mincps"));
    assert!(sparse.supports_state_field("inworld"));
}

#[test]
fn capability_fallbacks_expose_published_state_and_held_item_settings() {
    let modern = BridgeCapabilities::for_version_fallback(Some("26.2"));
    let legacy = BridgeCapabilities::for_version_fallback(Some("1.8.9"));

    assert!(modern.supports_state_field("holdingblock"));
    assert!(modern.supports_state_field("lookingatblock"));
    assert!(modern.supports_state_field("fov"));
    assert!(modern.supports_state_field("viewportwidth"));
    assert!(modern.supports_state_field("viewportheight"));
    assert!(modern.supports_setting("autototembehaviormode"));
    assert!(legacy.supports_setting("nametagshowhelditem"));
}

#[test]
fn a_legacy_state_line_without_a_type_field_still_decodes() {
    // The 1.8.9 bridge sends `{"mapped":true,...}` with no `type` at all. A strict
    // discriminator would reject the whole legacy wire, so decoding must not depend
    // on it.
    let state = state(
        r#"{"mapped":true,"inWorld":true,"guiOpen":false,"screenName":"","actionBar":"","health":20.0,"entities":[]}"#,
    );

    assert!(state.mapped);
    assert!(state.in_world);
    assert_eq!(state.health, 20.0);
}

#[test]
fn last_duplicate_json_key_wins_on_the_wire_path() {
    // `System.Text.Json` keeps the last occurrence, and `WIRE_CONTRACT.md` makes that
    // the arbitration for duplicate keys. The decode path is line -> `Value` ->
    // `GameState`, and `Value` keeps the last key, so the decision holds there. A
    // direct `GameState` deserialisation is stricter and rejects the document, which
    // is why the decoder must keep going through `Value`.
    let value: Value = serde_json::from_str(r#"{"type":"state","health":1.0,"health":20.0}"#)
        .expect("json with duplicate keys");
    assert_eq!(value["health"], 20.0);

    let state: GameState = serde_json::from_value(value).expect("value to state");
    assert_eq!(state.health, 20.0);
    assert!(serde_json::from_str::<GameState>(r#"{"health":1.0,"health":20.0}"#).is_err());
}

#[test]
fn a_null_collection_decodes_as_empty() {
    let state = state(r#"{"type":"state","entities":null,"chestStealerState":null}"#);

    assert!(state.entities.is_empty());
    assert!(state.chest_stealer_state.is_none());
}
