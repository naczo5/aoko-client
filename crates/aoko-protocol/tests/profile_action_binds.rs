//! The profile-level half of the bind-conflict rule.
//!
//! `Profile.ApplyTo` applies the general module binds first, then the dedicated
//! Auto Rod / Throwpot action binds. `InputHooks.SetAutoRodActionKey` refuses a key
//! that any module bind already holds, and `ApplyTo` then resets the stored value to
//! `0` — the general bind wins, the toggle bind is preserved. That predicate lives
//! here so the app layer can apply it without reaching into `aoko-core`.

use aoko_protocol::profile::{load_profile_from_json, to_profile_json, Profile};

fn with_binds(pairs: &[(&str, i32)], auto_rod: i32, throwpot: i32) -> Profile {
    let mut profile = Profile::default();
    for (module, key) in pairs {
        profile.module_keys.insert((*module).to_owned(), *key);
    }
    profile.auto_rod_action_key = auto_rod;
    profile.throwpot_action_key = throwpot;
    profile
}

#[test]
fn a_general_auto_rod_bind_clears_the_dedicated_action_key() {
    // `moduleKeys["autorod"]` holds the key the dedicated action bind also wants.
    let mut profile = with_binds(&[("autorod", 0x2E)], 0x2E, 0);

    assert!(profile.has_action_bind_collision());
    assert!(profile.normalize_action_binds());

    assert_eq!(
        profile.auto_rod_action_key, 0,
        "the dedicated bind is refused"
    );
    assert_eq!(
        profile.module_keys.get("autorod"),
        Some(&0x2E),
        "the general bind is preserved"
    );
}

#[test]
fn a_collision_with_any_other_module_bind_also_clears_the_action_key() {
    // The managed check is `ModuleKeys.Values.Contains(vk)`, not a lookup by id, so a
    // collision with any module bind refuses the dedicated key just the same.
    for module in ["panic", "hudeditor", "killaura"] {
        let mut profile = with_binds(&[(module, 0x2E)], 0x2E, 0);

        assert!(profile.normalize_action_binds(), "{module}");
        assert_eq!(profile.auto_rod_action_key, 0, "{module}");
        assert_eq!(profile.module_keys.get(module), Some(&0x2E), "{module}");
    }
}

#[test]
fn the_throwpot_action_key_obeys_the_same_rule() {
    let mut profile = with_binds(&[("throwpot", 0x2F)], 0, 0x2F);

    assert!(profile.normalize_action_binds());
    assert_eq!(profile.throwpot_action_key, 0);
    assert_eq!(profile.module_keys.get("throwpot"), Some(&0x2F));
    assert_eq!(profile.auto_rod_action_key, 0, "untouched");
}

#[test]
fn both_action_keys_can_be_cleared_in_one_pass() {
    let mut profile = with_binds(&[("autorod", 0x2E), ("throwpot", 0x2F)], 0x2E, 0x2F);

    assert!(profile.normalize_action_binds());
    assert_eq!(profile.auto_rod_action_key, 0);
    assert_eq!(profile.throwpot_action_key, 0);
}

#[test]
fn distinct_keys_are_left_alone() {
    let mut profile = with_binds(&[("autorod", 0x2E)], 0x31, 0x32);

    assert!(!profile.has_action_bind_collision());
    assert!(!profile.normalize_action_binds());

    assert_eq!(profile.auto_rod_action_key, 0x31);
    assert_eq!(profile.throwpot_action_key, 0x32);
    assert_eq!(profile.module_keys.get("autorod"), Some(&0x2E));
}

#[test]
fn the_default_profile_needs_no_normalisation() {
    // Every module defaults to key 0 and both action binds are 0, so a fresh profile is
    // already consistent and the call is a cheap no-op.
    let mut profile = Profile::default();

    assert!(!profile.has_action_bind_collision());
    assert!(!profile.normalize_action_binds());
    assert_eq!(profile, Profile::default());
}

#[test]
fn a_zero_or_negative_action_key_never_collides() {
    // `SetAutoRodActionKey` only guards `vk > 0`, so 0 passes through and a negative
    // value (which the managed host also lets through) is never treated as a collision.
    let mut zero = with_binds(&[("panic", 0)], 0, 0);
    let mut negative = with_binds(&[("panic", 0)], -1, -1);

    assert!(!zero.normalize_action_binds());
    assert!(!negative.normalize_action_binds());
    assert_eq!(negative.auto_rod_action_key, -1);
}

#[test]
fn normalisation_survives_a_profile_round_trip() {
    // The rule is applied at apply time, not at load time, so a stored collision is
    // still there after a reload — and normalising before a save persists the fix.
    let stored = to_profile_json(&with_binds(&[("autorod", 0x2E)], 0x2E, 0));
    let mut reloaded = load_profile_from_json(&stored).expect("profile");
    assert_eq!(
        reloaded.auto_rod_action_key, 0x2E,
        "load does not normalise"
    );

    assert!(reloaded.normalize_action_binds());
    let saved = load_profile_from_json(&to_profile_json(&reloaded)).expect("profile");
    assert_eq!(saved.auto_rod_action_key, 0);
    assert_eq!(saved.module_keys.get("autorod"), Some(&0x2E));
}
