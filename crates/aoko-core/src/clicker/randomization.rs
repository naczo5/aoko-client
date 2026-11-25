//! Click randomization: the view the settings UI and the click loop derive from
//! [`ClickRandomizationMode`].
//!
//! The enum itself lives in `aoko-protocol` (`crates/aoko-protocol/src/profile.rs`),
//! because it is part of the persisted profile and the config wire payload. This
//! module keeps only the *derived* surface that `Aoko/Core/Clicker.cs:319-359`
//! exposes to the view-model:
//!
//! - `RandomizationModeIndex`: `(int)mode`, and the setter clamps to `0..=2`
//!   (`Clicker.cs:336-340`).
//! - `RandomizationModeText`: `Basic` / `Medium` / `Advanced`, with a
//!   `_ => "Medium"` fallback arm (`Clicker.cs:342-348`). That arm is unreachable
//!   through the typed enum but reachable through a persisted integer, so it is
//!   kept as [`text_for_raw`].
//! - `JitterEnabled`: `mode != Basic`; setting it `false` forces `Basic`, and
//!   setting it `true` only promotes `Basic` back to `Medium`
//!   (`Clicker.cs:350-360`).
//!
//! The default is `Medium` in both `Clicker` (`Clicker.cs:122`) and `Profile`
//! (`Profile.cs:29`).

pub use aoko_protocol::profile::ClickRandomizationMode;

/// All three modes in index order.
pub const ALL_MODES: [ClickRandomizationMode; 3] = [
    ClickRandomizationMode::Basic,
    ClickRandomizationMode::Medium,
    ClickRandomizationMode::Advanced,
];

/// `RandomizationModeIndex` getter: the mode as an integer.
#[must_use]
pub fn index(mode: ClickRandomizationMode) -> i64 {
    i64::from(mode.as_i32())
}

/// `RandomizationModeIndex` setter: clamps instead of wrapping.
#[must_use]
pub fn from_index(value: i64) -> ClickRandomizationMode {
    match value.clamp(0, 2) {
        0 => ClickRandomizationMode::Basic,
        2 => ClickRandomizationMode::Advanced,
        _ => ClickRandomizationMode::Medium,
    }
}

/// `RandomizationModeText`.
#[must_use]
pub const fn text(mode: ClickRandomizationMode) -> &'static str {
    match mode {
        ClickRandomizationMode::Basic => "Basic",
        ClickRandomizationMode::Medium => "Medium",
        ClickRandomizationMode::Advanced => "Advanced",
    }
}

/// `JitterEnabled` getter.
#[must_use]
pub const fn jitter_enabled(mode: ClickRandomizationMode) -> bool {
    !matches!(mode, ClickRandomizationMode::Basic)
}

/// `JitterEnabled` setter: `false` forces `Basic`, `true` only promotes `Basic`.
#[must_use]
pub const fn with_jitter_enabled(
    mode: ClickRandomizationMode,
    enabled: bool,
) -> ClickRandomizationMode {
    if !enabled {
        ClickRandomizationMode::Basic
    } else {
        match mode {
            ClickRandomizationMode::Basic => ClickRandomizationMode::Medium,
            other => other,
        }
    }
}

/// Maps a persisted integer, `None` when it is not one of the three modes.
///
/// The C# setter clamps and the persisted field does not, so a profile that stores
/// `7` keeps `7` in memory and renders as `Medium` (`Clicker.cs:346`). Use
/// [`text_for_raw`] for that path.
#[must_use]
pub fn from_raw(raw: i64) -> Option<ClickRandomizationMode> {
    ClickRandomizationMode::from_i64(raw)
}

/// `RandomizationModeText` for any integer, including one that is not a mode.
#[must_use]
pub const fn text_for_raw(raw: i64) -> &'static str {
    match raw {
        0 => "Basic",
        2 => "Advanced",
        _ => "Medium",
    }
}

/// `JitterEnabled` for any integer, including one that is not a mode.
#[must_use]
pub const fn jitter_enabled_for_raw(raw: i64) -> bool {
    raw != 0
}

/// Method-call form of the free functions above, so view-model code reads like the
/// managed properties it replaces.
pub trait RandomizationModeView {
    /// See [`index`].
    fn mode_index(self) -> i64;
    /// See [`text`].
    fn mode_text(self) -> &'static str;
    /// See [`jitter_enabled`].
    fn jitter_enabled(self) -> bool;
    /// See [`with_jitter_enabled`].
    fn with_jitter_enabled(self, enabled: bool) -> Self;
}

impl RandomizationModeView for ClickRandomizationMode {
    fn mode_index(self) -> i64 {
        index(self)
    }

    fn mode_text(self) -> &'static str {
        text(self)
    }

    fn jitter_enabled(self) -> bool {
        jitter_enabled(self)
    }

    fn with_jitter_enabled(self, enabled: bool) -> Self {
        with_jitter_enabled(self, enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_protocol::profile::Profile;

    fn profile_from(json: &str) -> Profile {
        serde_json::from_str(json).expect("the managed profile shape parses")
    }

    /// `ClickerRandomizationTests.Clicker_RandomizationMode_DefaultsToMedium_AndJitterEnabledIsTrue`.
    #[test]
    fn the_default_is_medium_with_jitter_enabled() {
        let mode = ClickRandomizationMode::default();
        assert_eq!(mode, ClickRandomizationMode::Medium);
        assert_eq!(mode.mode_index(), 1);
        assert_eq!(mode.mode_text(), "Medium");
        assert!(mode.jitter_enabled());
    }

    /// `ClickerRandomizationTests.Clicker_RandomizationMode_MappingMatchesExpectedProperties`.
    #[test]
    fn every_mode_maps_to_its_index_text_and_jitter_flag() {
        let expected = [
            (ClickRandomizationMode::Basic, 0_i64, "Basic", false),
            (ClickRandomizationMode::Medium, 1, "Medium", true),
            (ClickRandomizationMode::Advanced, 2, "Advanced", true),
        ];

        for (mode, expected_index, expected_text, expected_jitter) in expected {
            assert_eq!(index(mode), expected_index);
            assert_eq!(mode.mode_index(), expected_index);
            assert_eq!(text(mode), expected_text);
            assert_eq!(mode.mode_text(), expected_text);
            assert_eq!(jitter_enabled(mode), expected_jitter);
            assert_eq!(from_index(expected_index), mode);
            assert_eq!(from_raw(expected_index), Some(mode));
        }

        assert_eq!(ALL_MODES.len(), 3);
    }

    /// `ClickerRandomizationTests.Clicker_JitterEnabled_SettingFalse_SetsModeToBasic`.
    #[test]
    fn disabling_jitter_forces_basic() {
        for mode in ALL_MODES {
            let forced = with_jitter_enabled(mode, false);
            assert_eq!(forced, ClickRandomizationMode::Basic);
            assert_eq!(index(forced), 0);
            assert_eq!(text(forced), "Basic");
            assert!(!jitter_enabled(forced));
        }
    }

    /// `ClickerRandomizationTests.Clicker_JitterEnabled_SettingTrue_WhenBasic_RestoresMedium`.
    #[test]
    fn enabling_jitter_only_promotes_basic() {
        assert_eq!(
            with_jitter_enabled(ClickRandomizationMode::Basic, true),
            ClickRandomizationMode::Medium
        );
        // An already-jittering mode is left alone.
        assert_eq!(
            with_jitter_enabled(ClickRandomizationMode::Advanced, true),
            ClickRandomizationMode::Advanced
        );
        assert_eq!(
            with_jitter_enabled(ClickRandomizationMode::Medium, true),
            ClickRandomizationMode::Medium
        );
    }

    /// `ClickerRandomizationTests.Clicker_RandomizationModeIndex_ClampsValues`.
    #[test]
    fn the_index_setter_clamps_instead_of_wrapping() {
        assert_eq!(from_index(-5), ClickRandomizationMode::Basic);
        assert_eq!(from_index(0), ClickRandomizationMode::Basic);
        assert_eq!(from_index(1), ClickRandomizationMode::Medium);
        assert_eq!(from_index(2), ClickRandomizationMode::Advanced);
        assert_eq!(from_index(99), ClickRandomizationMode::Advanced);
        assert_eq!(from_index(i64::MAX), ClickRandomizationMode::Advanced);
    }

    /// The `_ => "Medium"` arm of `Clicker.RandomizationModeText` is only reachable
    /// through an unvalidated persisted integer, so it has its own entry point.
    #[test]
    fn an_out_of_range_persisted_value_still_renders_as_medium() {
        assert_eq!(from_raw(7), None);
        assert_eq!(text_for_raw(7), "Medium");
        assert_eq!(text_for_raw(-1), "Medium");
        assert!(jitter_enabled_for_raw(7));
        assert!(!jitter_enabled_for_raw(0));
        assert_eq!(text_for_raw(0), "Basic");
        assert_eq!(text_for_raw(2), "Advanced");
    }

    /// `ClickerRandomizationTests.Profile_RandomizationMode_DefaultsToMedium`.
    #[test]
    fn a_profile_defaults_to_medium_with_jitter_enabled() {
        let profile = Profile::default();
        assert_eq!(profile.randomization_mode(), ClickRandomizationMode::Medium);
        assert!(profile.jitter_enabled());
    }

    /// `ClickerRandomizationTests.Profile_RandomizationMode_SerializesAndRoundTrips`.
    #[test]
    fn a_profile_round_trips_the_advanced_mode() {
        let mut profile = Profile::default();
        profile.name = String::from("HumanizedProfile");
        profile.set_randomization_mode(ClickRandomizationMode::Advanced);

        let json = serde_json::to_string(&profile).expect("the profile serializes");
        let round_tripped = profile_from(&json);

        assert_eq!(
            round_tripped.randomization_mode(),
            ClickRandomizationMode::Advanced
        );
        assert!(round_tripped.jitter_enabled());
        // The wire keeps the integer discriminant, not the variant name.
        assert!(
            json.contains("\"randomizationMode\":2"),
            "expected an integer mode in {json}"
        );
    }

    /// `ClickerRandomizationTests.Profile_LegacyJitterEnabledFalse_DeserializesToBasic`.
    #[test]
    fn a_legacy_jitter_false_profile_becomes_basic() {
        let profile = profile_from(r#"{"name":"OldProfile","jitterEnabled":false}"#);
        assert!(!profile.jitter_enabled());
        assert_eq!(profile.randomization_mode(), ClickRandomizationMode::Basic);
    }

    /// `ClickerRandomizationTests.Profile_LegacyJitterEnabledTrue_DeserializesToMedium`.
    #[test]
    fn a_legacy_jitter_true_profile_becomes_medium() {
        let profile = profile_from(r#"{"name":"OldProfile","jitterEnabled":true}"#);
        assert!(profile.jitter_enabled());
        assert_eq!(profile.randomization_mode(), ClickRandomizationMode::Medium);
    }
}
