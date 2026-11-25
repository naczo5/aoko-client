//! Port of `Aoko.Tests/GameStatePatchMergerTests.cs`.

use aoko_protocol::state::{
    ChestStealerSlot, ChestStealerState, EntityInfo, GameState, RefillState,
};
use aoko_protocol::state_patch::{apply, is_partial};
use serde_json::{Map, Value};

fn parse(text: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(text)
        .expect("fixture json")
        .as_object()
        .expect("fixture object")
        .clone()
}

fn value(text: &str) -> Value {
    serde_json::from_str(text).expect("fixture json")
}

#[test]
fn unmarked_state_is_not_treated_as_partial() {
    assert!(!is_partial(&value(r#"{"type":"state"}"#)));
}

#[test]
fn explicit_patch_preserves_fields_that_are_not_present() {
    let previous = GameState {
        in_world: true,
        screen_name: "GuiChat".to_owned(),
        action_bar: "keep me".to_owned(),
        attack_cooldown: 0.35,
        entities: vec![EntityInfo {
            name: "Target".to_owned(),
            dist: 3.0,
            ..EntityInfo::default()
        }],
        ..GameState::default()
    };

    let merged = apply(
        &previous,
        &parse(r#"{"type":"statePatch","inWorld":true,"attackCooldown":0.9}"#),
    );

    assert!(merged.in_world);
    assert_eq!(merged.screen_name, "GuiChat");
    assert_eq!(merged.action_bar, "keep me");
    assert_eq!(merged.attack_cooldown, 0.9);
    assert_eq!(merged.entities.len(), 1);
    assert_eq!(merged.entities[0].name, "Target");
}

#[test]
fn world_exit_patch_clears_transient_targets() {
    let previous = GameState {
        in_world: true,
        entities: vec![EntityInfo {
            name: "Target".to_owned(),
            ..EntityInfo::default()
        }],
        chest_stealer_state: Some(ChestStealerState {
            ready: true,
            ..ChestStealerState::default()
        }),
        ..GameState::default()
    };

    let merged = apply(&previous, &parse(r#"{"partial":true,"inWorld":false}"#));

    assert!(merged.entities.is_empty());
    assert!(merged.chest_stealer_state.is_none());
    assert!(merged.refill_state.is_none());
}

#[test]
fn partial_patch_preserves_refill_state() {
    let previous = GameState {
        in_world: true,
        refill_state: Some(RefillState {
            ready: true,
            window_id: 1,
            slots: vec![ChestStealerSlot {
                index: 9,
                slot_number: 9,
                x: 10,
                y: 10,
            }],
            ..RefillState::default()
        }),
        ..GameState::default()
    };

    let merged = apply(
        &previous,
        &parse(r#"{"type":"statePatch","inWorld":true,"attackCooldown":1.0}"#),
    );

    let refill = merged.refill_state.expect("refill survives the patch");
    assert!(refill.ready);
    assert_eq!(refill.slots.len(), 1);
    assert_eq!(refill.slots[0].slot_number, 9);
}

#[test]
fn explicit_null_entities_clears_the_previous_snapshot() {
    let previous = GameState {
        entities: vec![EntityInfo {
            name: "Target".to_owned(),
            ..EntityInfo::default()
        }],
        ..GameState::default()
    };

    let merged = apply(&previous, &parse(r#"{"partial":true,"entities":null}"#));

    assert!(merged.entities.is_empty());
}

#[test]
fn boolean_partial_marker_is_recognized() {
    assert!(is_partial(&value(r#"{"partial":true}"#)));
    assert!(!is_partial(&value(r#"{"partial":false}"#)));
}

#[test]
fn non_string_type_is_not_partial() {
    // The managed method reads `type` first and returns early on a non-string value;
    // `partial` is never consulted. Pinned because a numeric `type` must not turn a
    // full snapshot into a merge.
    assert!(!is_partial(&value(r#"{"type":1,"partial":true}"#)));
}

#[test]
fn partial_patch_keeps_unknown_and_typed_fields_untouched() {
    let previous = GameState {
        in_world: true,
        health: 18.5,
        ..GameState::default()
    };

    let merged = apply(
        &previous,
        &parse(r#"{"partial":true,"futureField":1,"inWorld":false}"#),
    );

    assert!(!merged.in_world);
    assert_eq!(merged.health, 18.5, "absent fields keep the previous value");
}
