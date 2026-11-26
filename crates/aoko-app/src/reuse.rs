//! Render strings that are the same on every frame.
//!
//! `MIGRATION_ARCHITECTURE.md` section 6 requires "static tokens precomputed" and "no
//! per-frame cloning in `Render::render`". The shell's element ids were the largest
//! source of per-frame allocations in the crate: every id was `format!`ed inside
//! `render`, which is one heap allocation per id per frame, and every one of them is
//! built from `&'static str` in the copy tables and can never change. `perf::tests`
//! measures the difference.
//!
//! The shape here is deliberately small: a table keyed by the pieces the id is built
//! from, filled on the first frame and read afterwards. A steady-state frame takes a
//! lock, hashes two or three `&'static str`s, and clones a `SharedString` — which is a
//! memcpy for the short ones and an atomic increment for the long ones, never an
//! allocation.
//!
//! Ids that end in a number use [`indexed`] instead, which returns
//! `ElementId::NamedInteger` and allocates nothing at all, not even on the first frame.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gpui::{ElementId, SharedString};

/// The memo for two-part strings.
fn two_parts() -> &'static Mutex<HashMap<(&'static str, &'static str), SharedString>> {
    static TABLE: OnceLock<Mutex<HashMap<(&'static str, &'static str), SharedString>>> =
        OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The memo for three-part strings, keyed by all three pieces so `-combo-` and `-` cannot
/// collide on the same card and bind.
fn three_parts() -> &'static Mutex<HashMap<[&'static str; 3], SharedString>> {
    static TABLE: OnceLock<Mutex<HashMap<[&'static str; 3], SharedString>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A poisoned lock cannot happen here — nothing panics while holding one — but if it did,
/// the table is still structurally valid, so recovering beats propagating a panic into a
/// paint.
fn lock<T>(table: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    match table.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// `a + b`, built once. Both pieces must be `&'static str`: that is what makes the key
/// sound, because the string a key maps to can never be stale.
pub fn joined(a: &'static str, b: &'static str) -> SharedString {
    let mut table = lock(two_parts());
    table
        .entry((a, b))
        .or_insert_with(|| SharedString::from(format!("{a}{b}")))
        .clone()
}

/// `a + separator + b`, built once. The separator is a part of the key rather than a
/// constant so `"{card}-{bind}"` and `"{card}-combo-{bind}"` stay distinct ids.
pub fn stitched(a: &'static str, separator: &'static str, b: &'static str) -> SharedString {
    let mut table = lock(three_parts());
    table
        .entry([a, separator, b])
        .or_insert_with(|| SharedString::from(format!("{a}{separator}{b}")))
        .clone()
}

/// An id that ends in a number: `ElementId::NamedInteger` carries the prefix and the
/// number separately, so nothing is formatted and nothing is allocated.
pub fn indexed(prefix: &'static str, index: usize) -> ElementId {
    ElementId::NamedInteger(SharedString::new_static(prefix), index as u64)
}

/// The last string built for each slot, with the text it was built from.
fn last_text() -> &'static Mutex<HashMap<&'static str, (String, SharedString)>> {
    static TABLE: OnceLock<Mutex<HashMap<&'static str, (String, SharedString)>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A label that is a pure function of one number — `Bind: F1`, `Action: F4`, `12`.
///
/// The number is the key, so the string is built once per distinct value instead of once
/// per frame. Every caller's domain is small (virtual-key codes, snapped slider values),
/// but the table is capped anyway: past [`NUMBER_MEMO_LIMIT`] entries it is cleared,
/// which costs one rebuild each rather than growing without bound during a long drag.
pub fn by_number(slot: &'static str, number: u64, build: impl FnOnce() -> String) -> SharedString {
    let mut table = lock(number_memos());
    let key = (slot, number);
    if let Some(text) = table.get(&key) {
        return SharedString::clone(text);
    }

    let text = SharedString::from(build());
    // Evict this slot only. A whole-table clear would let one pathological slot throw
    // away every other label in the process, and with it the guarantee that a frame
    // allocates nothing.
    let held = table.keys().filter(|key| key.0 == slot).count();
    if held >= NUMBER_MEMO_LIMIT {
        table.retain(|key, _| key.0 != slot);
    }
    table.insert(key, text.clone());
    text
}

/// Ceiling on [`by_number`]'s table, which is what a drag can grow.
pub const NUMBER_MEMO_LIMIT: usize = 1024;

fn number_memos() -> &'static Mutex<HashMap<(&'static str, u64), SharedString>> {
    static TABLE: OnceLock<Mutex<HashMap<(&'static str, u64), SharedString>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A label that shows a piece of state verbatim — a status line, a config name.
///
/// The slot names the place it is shown; the text decides whether it is rebuilt. A
/// steady-state frame compares two `&str`s and clones a `SharedString`.
pub fn text(slot: &'static str, source: &str) -> SharedString {
    let mut table = lock(last_text());
    if let Some((cached, text)) = table.get(slot) {
        if cached == source {
            return SharedString::clone(text);
        }
    }

    let text = SharedString::from(source);
    table.insert(slot, (source.to_owned(), text.clone()));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_joined_string_is_the_string_it_names_and_is_built_once() {
        // Long enough that a `SharedString` is heap-backed: a short one copies its bytes
        // inline on clone, so two clones would not share an address even when the memo
        // did its job.
        let first = joined("rail-search-button-label-for-", "the-command-palette");
        let second = joined("rail-search-button-label-for-", "the-command-palette");
        assert_eq!(
            first.as_str(),
            "rail-search-button-label-for-the-command-palette"
        );
        // The same key yields the same allocation, not merely an equal string.
        assert_eq!(first.as_ptr(), second.as_ptr());
    }

    #[test]
    fn the_separator_is_part_of_the_identity() {
        // Same card, same bind, different id shape: these must not collapse.
        assert_eq!(
            stitched("Combat", "-", "AimAssistToggle").as_str(),
            "Combat-AimAssistToggle"
        );
        assert_eq!(
            stitched("Combat", "-combo-", "AimAssistToggle").as_str(),
            "Combat-combo-AimAssistToggle"
        );
        assert_ne!(
            stitched("Combat", "-", "AimAssistToggle").as_ptr(),
            stitched("Combat", "-combo-", "AimAssistToggle").as_ptr()
        );
    }

    #[test]
    fn an_indexed_id_keeps_its_prefix_and_its_number() {
        assert_eq!(
            indexed("rail-tab", 3),
            ElementId::NamedInteger(SharedString::new_static("rail-tab"), 3)
        );
        assert_ne!(indexed("rail-tab", 3), indexed("rail-tab", 4));
    }
}

#[cfg(test)]
mod memo_tests {
    use super::*;

    #[test]
    fn a_numbered_label_is_built_once_per_value() {
        let mut builds = 0;
        // A unique slot: other tests share the table, and eviction is per slot.
        let first = by_number("memo-test-probe", 7, || {
            builds += 1;
            "seven".to_owned()
        });
        let second = by_number("memo-test-probe", 7, || {
            builds += 1;
            "seven".to_owned()
        });
        assert_eq!(builds, 1, "the second frame must not rebuild the label");
        assert_eq!(first.as_str(), "seven");
        assert_eq!(second.as_str(), "seven");

        // A different value is a different label.
        let other = by_number("memo-test-probe", 8, || "eight".to_owned());
        assert_eq!(other.as_str(), "eight");
    }

    #[test]
    fn a_slot_rebuilds_only_when_its_text_changes() {
        // Long enough to be heap-backed, so "same allocation" is observable through the
        // pointer: a short `SharedString` copies its bytes inline and two clones of it
        // have different addresses even when the memo did its job.
        const SAVED: &str = "configs saved to the profile directory";
        let first = text("status-probe", SAVED);
        let second = text("status-probe", SAVED);
        assert_eq!(first.as_str(), SAVED);
        assert_eq!(
            first.as_ptr(),
            second.as_ptr(),
            "the same text must come back as the same allocation"
        );

        let changed = text("status-probe", "failed");
        assert_eq!(changed.as_str(), "failed");
        assert_ne!(first.as_ptr(), changed.as_ptr());
    }

    #[test]
    fn the_number_memo_stops_growing() {
        // A neighbouring slot that must survive the other slot's eviction.
        let neighbour = by_number("cap-neighbour", 1, || "kept".to_owned());
        assert_eq!(neighbour.as_str(), "kept");

        for value in 0..(NUMBER_MEMO_LIMIT as u64 + 16) {
            let _ = by_number("cap", value, || value.to_string());
        }

        let table = lock(number_memos());
        let held = table.keys().filter(|key| key.0 == "cap").count();
        assert!(
            held <= NUMBER_MEMO_LIMIT,
            "held {held} entries for one slot"
        );
        assert!(
            table.contains_key(&("cap-neighbour", 1)),
            "one slot's eviction must not clear another's labels"
        );
    }
}
