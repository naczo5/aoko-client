//! Port of `Aoko.Tests/NickHiderConfigTests.cs`.

use aoko_protocol::nick_hider::{normalize_alias, normalize_input, MAX_ALIAS_LENGTH};
use aoko_protocol::profile::{load_profile_from_json, to_profile_json, Profile};

#[test]
fn normalize_alias_trims_plain_text() {
    assert_eq!(normalize_alias(Some("  Stream Alias  ")), "Stream Alias");
}

#[test]
fn normalize_input_preserves_typing_whitespace_until_send() {
    assert_eq!(normalize_input(Some("Stream ")), "Stream ");
    assert_eq!(normalize_alias(Some("Stream ")), "Stream");
}

#[test]
fn normalize_alias_rejects_invalid_text() {
    for alias in ["", "bad\nname", "\u{00A7}aBlue"] {
        assert_eq!(normalize_alias(Some(alias)), "", "alias {alias:?}");
    }
}

#[test]
fn normalize_alias_rejects_missing_input() {
    assert_eq!(normalize_alias(None), "");
    assert_eq!(normalize_input(None), "");
}

#[test]
fn normalize_alias_accepts_an_alias_exactly_at_the_limit() {
    let alias = "a".repeat(MAX_ALIAS_LENGTH);

    assert_eq!(normalize_alias(Some(&alias)), alias);
    assert_eq!(normalize_alias(Some(&format!("{alias}b"))), "");
}

#[test]
fn normalize_alias_counts_utf16_units_like_the_managed_host() {
    // The managed check is `value.Length`, which counts UTF-16 code units: an
    // astral character is two, so 16 emoji are exactly 32 and 17 are over.
    let sixteen = "🐈".repeat(16);
    let seventeen = "🐈".repeat(17);

    assert_eq!(normalize_alias(Some(&sixteen)), sixteen);
    assert_eq!(normalize_alias(Some(&seventeen)), "");
}

#[test]
fn profile_nick_hider_round_trips() {
    let mut profile = Profile::default();
    profile.nick_hider_enabled = true;
    profile.nick_hider_alias = "Alias".to_owned();

    let json = to_profile_json(&profile);
    let restored = load_profile_from_json(&json).expect("profile");

    assert!(restored.nick_hider_enabled);
    assert_eq!(restored.nick_hider_alias, "Alias");
}
