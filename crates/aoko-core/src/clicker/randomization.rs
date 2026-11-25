//! Click randomization mode and its derived `jitterEnabled` view.
//!
//! Port of `Aoko/Core/ClickRandomizationMode.cs` plus the three properties that
//! `Clicker` derives from it (`Aoko/Core/Clicker.cs:319-359`):
//!
//! - `RandomizationModeIndex`: `(int)mode`, and the setter clamps to `0..=2`
//!   (`Clicker.cs:336-340`).
//! - `RandomizationModeText`: `Basic` / `Medium` / `Advanced`, with a `_ => "Medium"`
//!   fallback arm (`Clicker.cs:342-348`). That fallback is unreachable through the
//!   typed enum but reachable through a persisted integer, so it is kept as
//!   [`ClickRandomizationMode::text_for_raw`].
//! - `JitterEnabled`: `mode != Basic`; setting it `false` forces `Basic`, and
//!   setting it `true` only promotes `Basic` back to `Medium`
//!   (`Clicker.cs:350-360`).
//!
//! The default is `Medium` in both `Clicker` (`Clicker.cs:122`) and `Profile`
//! (`Profile.cs:29`).

/// Which CPS model the click loop uses.
///
/// The discriminants are wire-visible: `randomizationMode` is sent to the bridge
/// as an integer (`docs/rust/WIRE_CONTRACT.md`, `config` frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
#[repr(u8)]
pub enum ClickRandomizationMode {
    /// Uniform CPS across the configured bounds.
    Basic = 0,
    /// Gaussian CPS around the midpoint. The default everywhere.
    #[default]
    Medium = 1,
    /// [`crate::clicker::AdvancedClickTimingState`]: fatigue, butterfly doublets,
    /// drift and micro-hesitations.
    Advanced = 2,
}

impl ClickRandomizationMode {
    /// All three modes in index order.
    pub const ALL: [Self; 3] = [Self::Basic, Self::Medium, Self::Advanced];

    /// Integer value, as used by `RandomizationModeIndex` and the wire payload.
    #[must_use]
    pub const fn index(self) -> i64 {
        self as i64
    }

    /// `Clicker.RandomizationModeIndex` setter: clamps instead of wrapping.
    #[must_use]
    pub fn from_index(index: i64) -> Self {
        match index.clamp(0, 2) {
            0 => Self::Basic,
            2 => Self::Advanced,
            _ => Self::Medium,
        }
    }

    /// `RandomizationModeText`.
    #[must_use]
    pub const fn text(self) -> &'static str {
        // Same arms as the C# switch, and the same `_ => "Medium"` default.
        match self {
            Self::Basic => "Basic",
            Self::Medium => "Medium",
            Self::Advanced => "Advanced",
        }
    }

    /// `JitterEnabled` getter.
    #[must_use]
    pub const fn jitter_enabled(self) -> bool {
        !matches!(self, Self::Basic)
    }

    /// `JitterEnabled` setter: `false` forces `Basic`, `true` only promotes `Basic`.
    #[must_use]
    pub const fn with_jitter_enabled(self, enabled: bool) -> Self {
        if !enabled {
            Self::Basic
        } else {
            match self {
                Self::Basic => Self::Medium,
                other => other,
            }
        }
    }

    /// Maps a persisted integer, `None` when it is not one of the three modes.
    ///
    /// The C# setter clamps and the persisted field does not, so a profile that
    /// stores `7` keeps `7` in memory and renders as `Medium`
    /// (`Clicker.cs:346`). Use [`Self::text_for_raw`] for that path.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Option<Self> {
        match raw {
            0 => Some(Self::Basic),
            1 => Some(Self::Medium),
            2 => Some(Self::Advanced),
            _ => None,
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ClickerRandomizationTests.Clicker_RandomizationMode_DefaultsToMedium_AndJitterEnabledIsTrue`.
    #[test]
    fn the_default_is_medium_with_jitter_enabled() {
        let mode = ClickRandomizationMode::default();
        assert_eq!(mode, ClickRandomizationMode::Medium);
        assert_eq!(mode.index(), 1);
        assert_eq!(mode.text(), "Medium");
        assert!(mode.jitter_enabled());
    }

    /// `ClickerRandomizationTests.Clicker_RandomizationMode_MappingMatchesExpectedProperties`.
    #[test]
    fn every_mode_maps_to_its_index_text_and_jitter_flag() {
        let expected = [
            (ClickRandomizationMode::Basic, 0, "Basic", false),
            (ClickRandomizationMode::Medium, 1, "Medium", true),
            (ClickRandomizationMode::Advanced, 2, "Advanced", true),
        ];

        for (mode, index, text, jitter) in expected {
            assert_eq!(mode.index(), index);
            assert_eq!(mode.text(), text);
            assert_eq!(mode.jitter_enabled(), jitter);
            assert_eq!(ClickRandomizationMode::from_index(index), mode);
            assert_eq!(ClickRandomizationMode::from_raw(index), Some(mode));
        }
    }

    /// `ClickerRandomizationTests.Clicker_JitterEnabled_SettingFalse_SetsModeToBasic`.
    #[test]
    fn disabling_jitter_forces_basic() {
        for mode in ClickRandomizationMode::ALL {
            let forced = mode.with_jitter_enabled(false);
            assert_eq!(forced, ClickRandomizationMode::Basic);
            assert_eq!(forced.index(), 0);
            assert_eq!(forced.text(), "Basic");
            assert!(!forced.jitter_enabled());
        }
    }

    /// `ClickerRandomizationTests.Clicker_JitterEnabled_SettingTrue_WhenBasic_RestoresMedium`.
    #[test]
    fn enabling_jitter_only_promotes_basic() {
        assert_eq!(
            ClickRandomizationMode::Basic.with_jitter_enabled(true),
            ClickRandomizationMode::Medium
        );
        // An already-jittering mode is left alone.
        assert_eq!(
            ClickRandomizationMode::Advanced.with_jitter_enabled(true),
            ClickRandomizationMode::Advanced
        );
        assert_eq!(
            ClickRandomizationMode::Medium.with_jitter_enabled(true),
            ClickRandomizationMode::Medium
        );
    }

    /// `ClickerRandomizationTests.Clicker_RandomizationModeIndex_ClampsValues`.
    #[test]
    fn the_index_setter_clamps_instead_of_wrapping() {
        assert_eq!(
            ClickRandomizationMode::from_index(-5),
            ClickRandomizationMode::Basic
        );
        assert_eq!(
            ClickRandomizationMode::from_index(0),
            ClickRandomizationMode::Basic
        );
        assert_eq!(
            ClickRandomizationMode::from_index(1),
            ClickRandomizationMode::Medium
        );
        assert_eq!(
            ClickRandomizationMode::from_index(2),
            ClickRandomizationMode::Advanced
        );
        assert_eq!(
            ClickRandomizationMode::from_index(99),
            ClickRandomizationMode::Advanced
        );
        assert_eq!(
            ClickRandomizationMode::from_index(i64::MAX),
            ClickRandomizationMode::Advanced
        );
    }

    /// The `_ => "Medium"` arm of `Clicker.RandomizationModeText` is only reachable
    /// through an unvalidated persisted integer, so it has its own entry point.
    #[test]
    fn an_out_of_range_persisted_value_still_renders_as_medium() {
        assert_eq!(ClickRandomizationMode::from_raw(7), None);
        assert_eq!(ClickRandomizationMode::text_for_raw(7), "Medium");
        assert_eq!(ClickRandomizationMode::text_for_raw(-1), "Medium");
        assert!(ClickRandomizationMode::jitter_enabled_for_raw(7));
        assert!(!ClickRandomizationMode::jitter_enabled_for_raw(0));
        assert_eq!(ClickRandomizationMode::text_for_raw(0), "Basic");
        assert_eq!(ClickRandomizationMode::text_for_raw(2), "Advanced");
    }
}
