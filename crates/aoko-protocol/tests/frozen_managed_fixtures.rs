//! Guards the frozen copies of the managed host in `tests/fixtures/managed/`.
//!
//! Six tests keep pinning what the managed C# host did — the config subscriptions it
//! wired, the flag it carried to the native bridges, the fight-status mapping, the panic
//! reset. Those pins used to read `Aoko/Core/*.cs` at run time, which meant they would
//! have panicked inside `read_to_string` on the commit that deletes `Aoko/`. The files
//! are copied into `tests/fixtures/managed/` and read with `include_str!`, so they are
//! compiled in and their absence is a compile error.
//!
//! They are evidence, not source. Editing one to make an assertion pass would make the
//! suite certify a host that never existed, so this file hashes them and
//! `tests/fixtures/managed/README.md` states the rule.

const GAME_STATE_CLIENT_CS: &str = include_str!("fixtures/managed/GameStateClient.cs");
const PROFILE_CS: &str = include_str!("fixtures/managed/Profile.cs");
const CLICKER_CS: &str = include_str!("fixtures/managed/Clicker.cs");

/// `Aoko/Core/GameStateClient.cs` at `011984b`, content hash as captured.
const GAME_STATE_CLIENT_DIGEST: u64 = 0x30e4_518b_4d86_6a06;
/// `Aoko/Core/Profile.cs` at `011984b`, content hash as captured.
const PROFILE_DIGEST: u64 = 0x4b5b_4f4d_98cf_adb2;
/// `Aoko/Core/Clicker.cs` at `011984b`, content hash as captured.
const CLICKER_DIGEST: u64 = 0x9964_783f_60e1_f15d;

/// FNV-1a over the file's content, with carriage returns dropped.
///
/// Dropping `\r` is what lets the pin survive the repository's `eol=crlf` rule, which
/// rewrites line endings on checkout: hashing the bytes as they sit on disk would pass on
/// the machine that captured the file and fail on a fresh clone. Any other character
/// changes the digest.
fn frozen_digest(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        if byte == b'\r' {
            continue;
        }
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[test]
fn frozen_managed_fixtures_are_unchanged_since_capture() {
    for (name, text, captured) in [
        (
            "GameStateClient.cs",
            GAME_STATE_CLIENT_CS,
            GAME_STATE_CLIENT_DIGEST,
        ),
        ("Profile.cs", PROFILE_CS, PROFILE_DIGEST),
        ("Clicker.cs", CLICKER_CS, CLICKER_DIGEST),
    ] {
        assert!(!text.is_empty(), "{name} is empty");
        assert_eq!(
            frozen_digest(text),
            captured,
            "{name} was edited after it was captured. These files are evidence of what the \
             managed host did; they are not source. Change the assertion that reads it and \
             say why, or delete the test and the fixture together — do not edit the fixture."
        );
    }
}
