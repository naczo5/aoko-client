//! Partial state merging, ported from `Aoko/Core/GameStatePatchMerger.cs`.
//!
//! Only an explicitly marked document takes this path: `type == "statePatch"`
//! (never emitted by a shipped bridge, kept for forward compatibility) or
//! `partial == true` (what `bridge_261.cpp` writes on fast frames).
//!
//! A value that fails to parse is ignored so the previous value survives; it is
//! never zeroed. The one behavioural extra is the world-exit scrub.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::state::{ChestStealerState, EntityInfo, GameState, RefillState};

/// Whether the document is a partial update rather than a complete state.
///
/// Mirrors the managed implementation exactly, including the quirk that a
/// non-string `type` aborts the check before `partial` is inspected.
#[must_use]
pub fn is_partial(node: &Value) -> bool {
    let Value::Object(object) = node else {
        return false;
    };

    if let Some(value) = object.get("type") {
        match value.as_str() {
            Some("statePatch") => return true,
            Some(_) => {}
            // `GetValue<string>()` throws for a non-string; the host swallows it
            // and reports "not partial" without consulting `partial`.
            None => return false,
        }
    }

    match object.get("partial") {
        None => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => false,
    }
}

/// Decodes one field out of the borrowed document.
///
/// `None` means the value did not parse, so the caller keeps what it had. Nothing is
/// copied on the way: a scalar costs no allocation and a collection costs exactly the
/// collection it builds.
fn parse_value<T: serde::de::DeserializeOwned>(value: &Value) -> Option<T> {
    T::deserialize(value).ok()
}

// Parses `$key` out of `$patch` straight into `$target`. The previous value survives
// an absent, `null`, or unparseable one.
macro_rules! apply_scalar {
    ($patch:expr, $target:expr, $key:literal, $ty:ty) => {
        if let Some(value) = $patch.get($key) {
            if !value.is_null() {
                if let Some(parsed) = parse_value::<$ty>(value) {
                    $target = parsed;
                }
            }
        }
    };
}

/// Merges `patch` into `state` in place.
///
/// This is the path the contract in `MIGRATION_ARCHITECTURE.md` section 6 names:
/// mutate the live snapshot, then publish it behind an `Arc`. A field the patch
/// omits is never touched, so its `String`, `Vec` or map keeps its allocation.
/// Only a field the patch actually carries can allocate, and then only for the
/// value it replaces.
pub fn apply_into(state: &mut GameState, patch: &Map<String, Value>) {
    for (name, value) in patch {
        match name.as_str() {
            "type" | "partial" => {}
            "mapped" => apply_scalar!(patch, state.mapped, "mapped", bool),
            "guiOpen" => apply_scalar!(patch, state.gui_open, "guiOpen", bool),
            "inWorld" => apply_scalar!(patch, state.in_world, "inWorld", bool),
            "screenName" => apply_scalar!(patch, state.screen_name, "screenName", String),
            "actionBar" => apply_scalar!(patch, state.action_bar, "actionBar", String),
            "health" => apply_scalar!(patch, state.health, "health", f32),
            "fov" => apply_scalar!(patch, state.fov, "fov", f32),
            "viewportWidth" => {
                apply_scalar!(patch, state.viewport_width, "viewportWidth", i32);
            }
            "viewportHeight" => {
                apply_scalar!(patch, state.viewport_height, "viewportHeight", i32);
            }
            "holdingBlock" => apply_scalar!(patch, state.holding_block, "holdingBlock", bool),
            "lookingAtBlock" => {
                apply_scalar!(patch, state.looking_at_block, "lookingAtBlock", bool);
            }
            "lookingAtEntity" => {
                apply_scalar!(patch, state.looking_at_entity, "lookingAtEntity", bool);
            }
            "lookingAtEntityLatched" => {
                apply_scalar!(
                    patch,
                    state.looking_at_entity_latched,
                    "lookingAtEntityLatched",
                    bool
                );
            }
            "lookingAtEntityHurtTime" => {
                apply_scalar!(
                    patch,
                    state.looking_at_entity_hurt_time,
                    "lookingAtEntityHurtTime",
                    i32
                );
            }
            "moduleStatus" => {
                if value.is_null() {
                    state.module_status.clear();
                } else if let Some(parsed) = parse_value::<BTreeMap<String, String>>(value) {
                    state.module_status = parsed;
                }
            }
            "breakingBlock" => apply_scalar!(patch, state.breaking_block, "breakingBlock", bool),
            "attackCooldown" => {
                apply_scalar!(patch, state.attack_cooldown, "attackCooldown", f32);
            }
            "attackCooldownPerTick" => {
                apply_scalar!(
                    patch,
                    state.attack_cooldown_per_tick,
                    "attackCooldownPerTick",
                    f32
                );
            }
            "killAuraUnavailableReason" => {
                apply_scalar!(
                    patch,
                    state.kill_aura_unavailable_reason,
                    "killAuraUnavailableReason",
                    String
                );
            }
            "killAuraHasTarget" => {
                apply_scalar!(patch, state.kill_aura_has_target, "killAuraHasTarget", bool);
            }
            "killAuraBlocking" => {
                apply_scalar!(patch, state.kill_aura_blocking, "killAuraBlocking", bool);
            }
            "lastActionId" => apply_scalar!(patch, state.last_action_id, "lastActionId", i32),
            "lastActionResult" => {
                apply_scalar!(patch, state.last_action_result, "lastActionResult", String);
            }
            "stateMs" => apply_scalar!(patch, state.state_ms, "stateMs", u64),
            "posX" => apply_scalar!(patch, state.pos_x, "posX", f64),
            "posY" => apply_scalar!(patch, state.pos_y, "posY", f64),
            "posZ" => apply_scalar!(patch, state.pos_z, "posZ", f64),
            "pitch" => apply_scalar!(patch, state.pitch, "pitch", f32),
            "entities" => {
                if value.is_null() {
                    state.entities.clear();
                } else if let Some(parsed) = parse_value::<Vec<EntityInfo>>(value) {
                    state.entities = parsed;
                }
            }
            "chestStealerState" => {
                if value.is_null() {
                    state.chest_stealer_state = None;
                } else if let Some(parsed) = parse_value::<ChestStealerState>(value) {
                    state.chest_stealer_state = Some(parsed);
                }
            }
            "refillState" => {
                if value.is_null() {
                    state.refill_state = None;
                } else if let Some(parsed) = parse_value::<RefillState>(value) {
                    state.refill_state = Some(parsed);
                }
            }
            "pixelPartyTargetFound" => {
                apply_scalar!(
                    patch,
                    state.pixel_party_target_found,
                    "pixelPartyTargetFound",
                    bool
                );
            }
            "pixelPartyTargetYaw" => {
                apply_scalar!(
                    patch,
                    state.pixel_party_target_yaw,
                    "pixelPartyTargetYaw",
                    f32
                );
            }
            "pixelPartyTargetDist" => {
                apply_scalar!(
                    patch,
                    state.pixel_party_target_dist,
                    "pixelPartyTargetDist",
                    f32
                );
            }
            "pixelPartyYawDelta" => {
                apply_scalar!(
                    patch,
                    state.pixel_party_yaw_delta,
                    "pixelPartyYawDelta",
                    f32
                );
            }
            // Unknown keys are ignored: the gate is the key name, exactly as in C#.
            _ => {}
        }
    }

    // A transition packet may omit expensive producer payloads, but it must not
    // leave combat/stealer targets from the previous world visible to input
    // modules for the next state notification.
    if matches!(patch.get("inWorld"), Some(Value::Bool(false))) {
        state.entities.clear();
        state.chest_stealer_state = None;
        state.refill_state = None;
    }
}

/// Applies `patch` to `previous`, returning the merged snapshot.
///
/// A copy-shaped convenience over [`apply_into`] for callers that need a new value
/// rather than to update a live one; it costs one full copy of `previous` plus
/// whatever [`apply_into`] allocates. The transport path uses [`apply_into`].
#[must_use]
pub fn apply(previous: &GameState, patch: &Map<String, Value>) -> GameState {
    let mut merged = previous.clone();
    apply_into(&mut merged, patch);
    merged
}
