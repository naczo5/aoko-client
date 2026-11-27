//! The `bind`-path → `Profile` field table, and the read/write pair behind it.
//!
//! Every slider, checkbox and combo in the card table carries the managed binding
//! path (`MinCPS`, `KillAuraSettings.CpsMode`, `AutoRodSlotMode`). In WPF that path
//! *was* the persistence: `Profile` had a property of the same name, and two-way
//! binding wrote it. GPUI has no binding, so the path has to be resolved by hand —
//! and the honest way to do that without 142 hand-copied accessors is to resolve it
//! against the `Profile` that `aoko-protocol` already serialises:
//!
//! * `to_profile_json` gives the exact wire shape, camelCase renames included.
//! * A path walks that JSON case-insensitively, so `MinCPS` finds `minCPS` and
//!   `KillAuraSettings.MinCps` finds `killAuraSettings.minCps`, and a name that no
//!   longer exists fails loudly instead of silently binding to nothing.
//! * `load_profile_from_json` rebuilds the profile after a write, so the struct is
//!   the single source of truth and no field is written by index.
//!
//! The table is therefore not a list of strings: it is the profile itself, and
//! [`unresolved_binds`] is the test that proves every control in `copy` has a field
//! behind it. A control that resolves to nothing is a parity gap, and the test says
//! which one by name.

use std::collections::BTreeMap;

use aoko_protocol::profile::{load_profile_from_json, to_profile_json, Profile};

use crate::views::copy::{self, Control, TabBody};

/// A control's value, whatever its widget.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlValue {
    /// A slider position.
    Number(f64),
    /// A checkbox.
    Toggle(bool),
    /// A combo's selected index.
    Index(usize),
    /// A combo whose setting is stored as text: the mode name, not the index.
    Text(String),
}

impl ControlValue {
    /// The number a slider paints, with the two other variants coerced.
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Number(value) => *value,
            Self::Toggle(on) => {
                if *on {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Index(index) => *index as f64,
            // A text-backed combo has no numeric value; a slider on one reads 0.
            Self::Text(_) => 0.0,
        }
    }
}

/// One `bind` path split into its parts.
#[must_use]
pub fn path_of(bind: &str) -> Vec<&str> {
    bind.split('.').filter(|part| !part.is_empty()).collect()
}

/// Bind paths whose managed property is derived rather than stored under that name.
///
/// `RandomizationModeIndex` is the slider the markup binds (`MainWindow.xaml:381`); the
/// profile stores the mode itself as `randomizationMode`, so the index reads and
/// writes that field.
pub const ALIASES: [(&str, &str); 1] = [("RandomizationModeIndex", "randomizationMode")];

/// Bind paths that name no profile field, with the reason.
///
/// Both are real controls in the markup; neither is a persisted setting. Only
/// `ConfigSelection` is a row in the card table — `Enabled` is the tracked-block row
/// template's checkbox (`MainWindow.xaml:697-698`), which the table now models as
/// [`copy::Control::BlockEspRows`], so no row carries that bind any more. This list is therefore
/// not a count of table controls: subtracting `NON_PROFILE_BINDS.len()` from [`all_binds`] gives
/// the wrong snapshot size, which is what the reconciliation test used to do.
pub const NON_PROFILE_BINDS: [(&str, &str); 2] = [
    (
        "ConfigSelection",
        "the Settings config combo is filled from the configs directory at runtime",
    ),
    (
        "Enabled",
        "the per-block checkbox inside the Block ESP item template",
    ),
];

/// The path a bind resolves through, following [`ALIASES`].
#[must_use]
pub fn canonical(bind: &str) -> &str {
    ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(bind))
        .map_or(bind, |(_, target)| *target)
}

/// Walks a profile's JSON to a bind path, case-insensitively.
///
/// `Some` only when the whole path exists; a miss anywhere returns `None`, which is
/// what [`unresolved_binds`] reports.
#[must_use]
pub fn lookup<'a>(root: &'a serde_json::Value, bind: &str) -> Option<&'a serde_json::Value> {
    let mut node = root;
    for part in path_of(canonical(bind)) {
        let object = node.as_object()?;
        let (_, value) = object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(part))?;
        node = value;
    }

    Some(node)
}

/// The value at a bind path, in the shape its widget needs.
#[must_use]
pub fn value_at(root: &serde_json::Value, bind: &str) -> Option<ControlValue> {
    let node = lookup(root, bind)?;
    if let Some(number) = node.as_f64() {
        return Some(ControlValue::Number(number));
    }

    if let Some(on) = node.as_bool() {
        return Some(ControlValue::Toggle(on));
    }

    // A combo whose setting is the option text (`KillAuraSettings.CpsMode` holds
    // `"Record"`) reads as text; a combo whose setting is an index (`AutoRodSlotMode`)
    // reads as a number and the view turns it into an index.
    node.as_str()
        .map(|text| ControlValue::Text(text.to_owned()))
}

/// Every control value in a profile, by bind path.
///
/// Built once and cached by the shell: the alternative is a JSON parse per painted
/// slider, and there are 72 of them.
#[must_use]
pub fn snapshot(profile: &Profile) -> BTreeMap<String, ControlValue> {
    let root: serde_json::Value =
        serde_json::from_str(&to_profile_json(profile)).unwrap_or(serde_json::Value::Null);

    let mut values = BTreeMap::new();
    for bind in all_binds().into_iter().chain(switch_binds()) {
        if let Some(value) = value_at(&root, bind) {
            values.insert(bind.to_owned(), value);
        }
    }

    values
}

/// Writes one control value into the profile.
///
/// `false` when the path does not exist or the value cannot be stored in the field
/// the path names, in which case the profile is untouched.
pub fn write(profile: &mut Profile, bind: &str, value: ControlValue) -> bool {
    let Ok(text) = serde_json::to_string(
        &serde_json::from_str::<serde_json::Value>(&to_profile_json(profile))
            .unwrap_or(serde_json::Value::Null),
    ) else {
        return false;
    };
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };

    let Some(target) = mutable_lookup(&mut root, bind) else {
        return false;
    };

    let stored = match (target.clone(), value) {
        (serde_json::Value::Bool(_), ControlValue::Toggle(on)) => serde_json::Value::Bool(on),
        (serde_json::Value::Bool(_), other) => serde_json::Value::Bool(other.as_f64() > 0.5),
        // An integer-typed field needs an integer JSON value: serde rejects
        // `15.0` for `i32`, so a whole slider position is written as a whole number.
        (serde_json::Value::Number(existing), ControlValue::Number(number)) => {
            if existing.as_i64().is_some() && number.fract() == 0.0 {
                serde_json::Value::from(number.round() as i64)
            } else {
                serde_json::Value::from(number)
            }
        }
        (serde_json::Value::Number(_), other) => serde_json::Value::from(other.as_f64()),
        (serde_json::Value::String(_), ControlValue::Text(text)) => serde_json::Value::from(text),
        (serde_json::Value::String(_), ControlValue::Index(_)) => return false,
        _ => return false,
    };
    *target = stored;

    match load_profile_from_json(&root.to_string()) {
        Some(updated) => {
            *profile = updated;
            true
        }
        None => false,
    }
}

/// Same walk as [`lookup`], but borrowing mutably so the leaf can be replaced.
fn mutable_lookup<'a>(
    root: &'a mut serde_json::Value,
    bind: &str,
) -> Option<&'a mut serde_json::Value> {
    let mut node = root;
    for part in path_of(canonical(bind)) {
        let object = node.as_object_mut()?;
        let key = object
            .keys()
            .find(|key| key.eq_ignore_ascii_case(part))?
            .clone();
        node = object.get_mut(&key)?;
    }

    Some(node)
}

/// Every slider, checkbox and combo bind path in the card table.
#[must_use]
pub fn all_binds() -> Vec<&'static str> {
    let mut binds = Vec::new();
    for tab in &copy::TABS {
        let TabBody::Cards(cards) = tab.body else {
            continue;
        };

        for card in cards {
            for control in card.controls {
                match control {
                    Control::Slider { bind, .. }
                    | Control::Check { bind, .. }
                    | Control::Combo { bind, .. } => binds.push(*bind),
                    _ => {}
                }
            }
        }
    }

    binds.sort_unstable();
    binds.dedup();
    binds
}

/// The six settings the markup binds to **two sliders each**.
///
/// 142 slider/check/combo controls resolve to 136 distinct bind paths because these
/// six names appear twice in `Aoko/MainWindow.xaml`: once in the Grok-rotation panel
/// (all of them on the single physical line 1257) and once in the always-visible
/// movement panel at 1263-1267. Both sliders edit the same setting, so the table has
/// one row per occurrence and the bind path is shared.
pub const SHARED_BINDS: [(&str, &str); 6] = [
    ("KillAuraSettings.DeadZoneSize", "1257 (Grok panel) + 1263"),
    ("KillAuraSettings.MinTurnSpeed", "1257 + 1264"),
    ("KillAuraSettings.MaxTurnSpeed", "1257 + 1264"),
    ("KillAuraSettings.Acceleration", "1257 + 1265"),
    ("KillAuraSettings.Deceleration", "1257 + 1265"),
    ("KillAuraSettings.NoiseStrength", "1257 + 1267"),
];

/// How many slider, checkbox and combo rows the card table holds, duplicates included.
#[must_use]
pub fn control_count() -> usize {
    copy::TABS
        .iter()
        .filter_map(|tab| match tab.body {
            TabBody::Cards(cards) => Some(cards),
            // A view, not a card table: `stats_view` paints counters and one button, and none
            // of them is a bound slider, check or combo.
            TabBody::Stats => None,
            TabBody::Accounts | TabBody::Servers => None,
        })
        .flatten()
        .flat_map(|card| card.controls.iter())
        .filter(|&control| {
            matches!(
                control,
                Control::Slider { .. } | Control::Check { .. } | Control::Combo { .. }
            )
        })
        .count()
}

/// How many rows carry one bind path.
#[must_use]
pub fn occurrences(bind: &str) -> usize {
    copy::TABS
        .iter()
        .filter_map(|tab| match tab.body {
            TabBody::Cards(cards) => Some(cards),
            // No bound row lives on the Stats view, so it contributes no occurrence.
            TabBody::Stats => None,
            TabBody::Accounts | TabBody::Servers => None,
        })
        .flatten()
        .flat_map(|card| card.controls.iter())
        .filter(|&control| match control {
            Control::Slider { bind: row, .. }
            | Control::Check { bind: row, .. }
            | Control::Combo { bind: row, .. } => *row == bind,
            _ => false,
        })
        .count()
}

/// Every `Control::Switch` bind.
///
/// Separate from [`all_binds`], which is the 136 slider/check/combo paths the
/// reconciliation test pins. A switch needs its own read entry: the five
/// profile-bound switches (`ShowLogo`, `DevMode`, `ShowModuleList`,
/// `DiscordRpcEnabled`, `AutoTotemElytra`) live in the profile but not in the
/// control table, so without this they painted OFF over a profile that says ON.
#[must_use]
pub fn switch_binds() -> Vec<&'static str> {
    let mut binds = Vec::new();
    for tab in &copy::TABS {
        let TabBody::Cards(cards) = tab.body else {
            continue;
        };

        for card in cards {
            for control in card.controls {
                if let Control::Switch { bind, .. } = control {
                    binds.push(*bind);
                }
            }
        }
    }

    binds.sort_unstable();
    binds.dedup();
    binds
}

/// Bind paths with no field behind them, for a given profile.
#[must_use]
pub fn unresolved_binds(profile: &Profile) -> Vec<&'static str> {
    let root: serde_json::Value =
        serde_json::from_str(&to_profile_json(profile)).unwrap_or(serde_json::Value::Null);

    all_binds()
        .into_iter()
        .filter(|bind| value_at(&root, bind).is_none())
        .filter(|bind| {
            !NON_PROFILE_BINDS
                .iter()
                .any(|(known, _)| known.eq_ignore_ascii_case(bind))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_profile() -> Profile {
        Profile::default()
    }

    #[test]
    fn every_control_bind_resolves_to_a_profile_field() {
        let missing = unresolved_binds(&default_profile());
        assert!(
            missing.is_empty(),
            "{} control binds have no Profile field: {missing:?}",
            missing.len()
        );
    }

    #[test]
    fn the_six_shared_binds_reconcile_142_controls_with_136_paths() {
        // 72 sliders + 57 checks + 13 combos, counted from the table and pinned by
        // `copy::tests::the_control_counts_match_the_markup`. The markup has 58 checkboxes;
        // the 58th is the tracked-block row template's (`MainWindow.xaml:697-698`), which the
        // table models as `Control::BlockEspRows` — one row per entry, and no bind path — so
        // it is not a bound control and 143 became 142 when that swap landed (`0ec75d7`).
        assert_eq!(control_count(), 142);
        assert_eq!(all_binds().len(), 136);
        assert_eq!(control_count() - all_binds().len(), SHARED_BINDS.len());

        for (bind, citation) in SHARED_BINDS {
            assert_eq!(occurrences(bind), 2, "{bind} at {citation}");
        }

        // Every path beyond the six occurs exactly once, so nothing else is shared.
        for bind in all_binds() {
            if SHARED_BINDS.iter().any(|(shared, _)| *shared == bind) {
                continue;
            }
            assert_eq!(occurrences(bind), 1, "{bind}");
        }
    }

    #[test]
    fn the_bind_list_is_the_control_count() {
        // 72 sliders + 57 checks + 13 combos, minus repeats: a slider and a check can share a
        // path only if the markup does, and the table then has one bind. 142 rows, 136 paths.
        let binds = all_binds();
        assert!(binds.len() >= 100, "{} binds", binds.len());
        assert!(
            binds.windows(2).all(|pair| pair[0] < pair[1]),
            "sorted and unique"
        );
    }

    #[test]
    fn a_bind_path_walks_case_insensitively() {
        let root = serde_json::json!({
            "minCPS": 12.0,
            "killAuraSettings": { "cpsMode": 1 },
            "aimAssistEnabled": true,
        });

        assert_eq!(
            value_at(&root, "MinCPS"),
            Some(ControlValue::Number(12.0)),
            "the managed property name finds the camelCase field"
        );
        assert_eq!(
            value_at(&root, "KillAuraSettings.CpsMode"),
            Some(ControlValue::Number(1.0)),
            "the JSON shape is the decoder's, the widget converts"
        );
        assert_eq!(
            value_at(&root, "AimAssistEnabled"),
            Some(ControlValue::Toggle(true))
        );
        assert_eq!(value_at(&root, "NoSuchField"), None);
        assert_eq!(value_at(&root, "KillAuraSettings.NoSuchField"), None);
    }

    #[test]
    fn writing_a_number_round_trips_through_the_profile() {
        let mut profile = default_profile();
        let before = profile.min_cps;

        assert!(write(&mut profile, "MinCPS", ControlValue::Number(13.0)));
        assert_eq!(profile.min_cps, 13.0);
        assert_ne!(profile.min_cps, before);

        let json = to_profile_json(&profile);
        assert!(json.contains("\"minCPS\": 13.0"), "{json}");
    }

    #[test]
    fn writing_a_toggle_and_an_index_reaches_the_nested_settings() {
        let mut profile = default_profile();

        assert!(write(
            &mut profile,
            "KillAuraSettings.CpsMode",
            ControlValue::Text("Record".to_owned())
        ));
        assert_eq!(
            profile.kill_aura_settings.cps_mode(),
            "record",
            "the wire form is lower-cased"
        );

        // The pair stays ordered, so the max goes up first: a min above the max is
        // clamped on load, which is the protocol's rule and not a bug here.
        assert!(write(
            &mut profile,
            "KillAuraSettings.MaxCps",
            ControlValue::Number(18.0)
        ));
        assert!(write(
            &mut profile,
            "KillAuraSettings.MinCps",
            ControlValue::Number(15.0)
        ));
        assert_eq!(profile.kill_aura_settings.min_cps(), 15, "an integer field");
        assert_eq!(profile.kill_aura_settings.max_cps(), 18);
    }

    #[test]
    fn a_missing_path_leaves_the_profile_alone() {
        let mut profile = default_profile();
        let before = profile.clone();

        assert!(!write(
            &mut profile,
            "NoSuchField",
            ControlValue::Number(1.0)
        ));
        assert_eq!(profile, before);
        assert!(!write(
            &mut profile,
            "KillAuraSettings.NoSuchField",
            ControlValue::Number(1.0)
        ));
        assert_eq!(profile, before);
    }

    #[test]
    fn the_snapshot_carries_one_entry_per_bind() {
        let profile = default_profile();
        let values = snapshot(&profile);

        let mut expected = all_binds();
        expected.extend(switch_binds());
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(expected.len(), 179, "136 slider/check/combo plus 43 switch");

        // Two of the 179 name no profile field, so they cannot appear and the snapshot holds
        // 177: `ConfigSelection`, which the Settings combo fills from the configs directory at
        // runtime, and `HudEditorActive`, the `hudeditor` module's toggle property
        // (`aoko_core::search`). Module state is not profile state: since 7abc858 the controller
        // owns it (`AppController::is_module_enabled`), reached through
        // `crate::runtime::Runtime`, so a switch on it writes there and not here.
        //
        // `Enabled` is a third name in the loop below and a different case: the tracked-block
        // row template's checkbox (`MainWindow.xaml:697-698`) used to be a table row and is now
        // `Control::BlockEspRows`, so nothing in the table names it at all. It is asserted
        // absent so a restore of that bogus bind is caught rather than silently re-counted.
        assert_eq!(
            values.len(),
            expected.len() - 2,
            "179 minus the two binds no profile field answers"
        );
        for bind in ["ConfigSelection", "Enabled", "HudEditorActive"] {
            assert!(
                !values.contains_key(bind),
                "{bind} has no profile field and must not be in the table"
            );
        }
        assert_eq!(
            values.get("MinCPS").cloned(),
            Some(ControlValue::Number(f64::from(profile.min_cps)))
        );
    }

    #[test]
    fn a_toggle_reads_as_one_or_zero_for_a_slider() {
        assert_eq!(ControlValue::Toggle(true).as_f64(), 1.0);
        assert_eq!(ControlValue::Toggle(false).as_f64(), 0.0);
        assert_eq!(ControlValue::Index(3).as_f64(), 3.0);
        assert_eq!(ControlValue::Number(4.5).as_f64(), 4.5);
    }
}
