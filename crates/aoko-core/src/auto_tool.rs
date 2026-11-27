//! AutoTool: settings, clamps and the profile round-trip.
//!
//! Like AutoRod, AutoTool has **no loop in the managed host**. The feature is a
//! native-side tool/weapon switcher; the managed side only publishes its settings
//! in the config payload (`GameStateClient.cs:1482-1490`, keys declared in
//! `aoko_protocol::config_payload`) and toggles a module flag. What is portable is
//! the settings surface with its clamps from `Clicker.cs:1860-1935`, plus the
//! timeouts the native side is documented to honour.
//!
//! Deferred, with reason: the bridge's swap *behaviour* (which tool is best for the
//! block being looked at) lives in the native mod, not in this repository's managed
//! code, so there is nothing behavioural to port here.

use std::time::Duration;

use aoko_protocol::profile::Profile;

/// `AutoToolSwapToDelay` clamp (`Clicker.cs:1874`).
pub const SWAP_TO_DELAY_RANGE: (i32, i32) = (0, 500);
/// `AutoToolSwapBackDelay` clamp (`Clicker.cs:1901`).
pub const SWAP_BACK_DELAY_RANGE: (i32, i32) = (0, 1_000);

/// The settings the config payload carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoToolSettings {
    /// `AutoToolEnabled`.
    pub enabled: bool,
    /// `AutoToolSwapWeapon`: also swap to a weapon, not just a tool.
    pub swap_weapon: bool,
    /// `AutoToolInstantSwap`: swap on the first press frame instead of after
    /// [`Self::swap_to_delay`].
    pub instant_swap: bool,
    /// `AutoToolSwapToDelay`, milliseconds.
    pub swap_to_delay: i32,
    /// `AutoToolSwapBack`.
    pub swap_back: bool,
    /// `AutoToolSwapBackDelay`, milliseconds.
    pub swap_back_delay: i32,
    /// `AutoToolRequireMouseDown`.
    pub require_mouse_down: bool,
    /// `AutoToolOnlySneaking`.
    pub only_sneaking: bool,
    /// `AutoToolBedwarsMode`.
    pub bedwars_mode: bool,
}

impl Default for AutoToolSettings {
    fn default() -> Self {
        // `Profile.cs:198-206`; every default matches the profile the app ships.
        Self {
            enabled: false,
            swap_weapon: true,
            instant_swap: true,
            swap_to_delay: 50,
            swap_back: false,
            swap_back_delay: 350,
            require_mouse_down: true,
            only_sneaking: false,
            bedwars_mode: true,
        }
    }
}

impl AutoToolSettings {
    /// Applies the two property clamps.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.swap_to_delay = self
            .swap_to_delay
            .clamp(SWAP_TO_DELAY_RANGE.0, SWAP_TO_DELAY_RANGE.1);
        self.swap_back_delay = self
            .swap_back_delay
            .clamp(SWAP_BACK_DELAY_RANGE.0, SWAP_BACK_DELAY_RANGE.1);
        self
    }

    /// Delay before swapping to the tool, zero when the swap is instant.
    #[must_use]
    pub fn swap_to_delay_duration(self) -> Duration {
        if self.instant_swap {
            Duration::ZERO
        } else {
            Duration::from_millis(self.swap_to_delay.max(0) as u64)
        }
    }

    /// Delay before swapping back, `None` when swap-back is off.
    #[must_use]
    pub fn swap_back_delay_duration(self) -> Option<Duration> {
        if self.swap_back {
            Some(Duration::from_millis(self.swap_back_delay.max(0) as u64))
        } else {
            None
        }
    }

    /// Reads the settings out of a profile.
    #[must_use]
    pub fn from_profile(profile: &Profile) -> Self {
        Self {
            enabled: profile.auto_tool_enabled,
            swap_weapon: profile.auto_tool_swap_weapon,
            instant_swap: profile.auto_tool_instant_swap,
            swap_to_delay: profile.auto_tool_swap_to_delay,
            swap_back: profile.auto_tool_swap_back,
            swap_back_delay: profile.auto_tool_swap_back_delay,
            require_mouse_down: profile.auto_tool_require_mouse_down,
            only_sneaking: profile.auto_tool_only_sneaking,
            bedwars_mode: profile.auto_tool_bedwars_mode,
        }
        .clamped()
    }

    /// Writes the settings into a profile, applying the clamps.
    pub fn apply_to(self, profile: &mut Profile) {
        let clamped = self.clamped();
        profile.auto_tool_enabled = clamped.enabled;
        profile.auto_tool_swap_weapon = clamped.swap_weapon;
        profile.auto_tool_instant_swap = clamped.instant_swap;
        profile.auto_tool_swap_to_delay = clamped.swap_to_delay;
        profile.auto_tool_swap_back = clamped.swap_back;
        profile.auto_tool_swap_back_delay = clamped.swap_back_delay;
        profile.auto_tool_require_mouse_down = clamped.require_mouse_down;
        profile.auto_tool_only_sneaking = clamped.only_sneaking;
        profile.auto_tool_bedwars_mode = clamped.bedwars_mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AutoToolManagedTests.ModelAndProfile_DefaultsAndClampValues`.
    #[test]
    fn defaults_and_clamps_match_the_managed_properties() {
        let defaults = AutoToolSettings::default();
        assert!(!defaults.enabled);
        assert!(defaults.swap_weapon);
        assert!(defaults.instant_swap);
        assert_eq!(defaults.swap_to_delay, 50);
        assert!(!defaults.swap_back);
        assert_eq!(defaults.swap_back_delay, 350);
        assert!(defaults.require_mouse_down);
        assert!(!defaults.only_sneaking);
        assert!(defaults.bedwars_mode);

        // The managed setter clamps at the property, not in the payload.
        let clamped = AutoToolSettings {
            enabled: true,
            swap_weapon: false,
            instant_swap: false,
            swap_to_delay: -50,
            swap_back: true,
            swap_back_delay: 5_000,
            require_mouse_down: false,
            only_sneaking: true,
            bedwars_mode: false,
        }
        .clamped();

        assert!(clamped.enabled);
        assert!(!clamped.swap_weapon);
        assert!(!clamped.instant_swap);
        assert_eq!(clamped.swap_to_delay, 0, "`Math.Clamp(value, 0, 500)`");
        assert!(clamped.swap_back);
        assert_eq!(
            clamped.swap_back_delay, 1_000,
            "`Math.Clamp(value, 0, 1000)`"
        );
        assert!(!clamped.require_mouse_down);
        assert!(clamped.only_sneaking);
        assert!(!clamped.bedwars_mode);

        // An instant swap ignores the delay entirely.
        assert_eq!(clamped.swap_to_delay_duration(), Duration::ZERO);
        assert_eq!(
            AutoToolSettings {
                instant_swap: false,
                swap_to_delay: 150,
                ..AutoToolSettings::default()
            }
            .swap_to_delay_duration(),
            Duration::from_millis(150)
        );
        assert_eq!(
            AutoToolSettings::default().swap_back_delay_duration(),
            None,
            "swap-back is off by default"
        );
        assert_eq!(
            AutoToolSettings {
                swap_back: true,
                swap_back_delay: 500,
                ..AutoToolSettings::default()
            }
            .swap_back_delay_duration(),
            Some(Duration::from_millis(500))
        );
    }

    /// `AutoToolManagedTests.Profile_CreateAndApplyRoundTrip`.
    #[test]
    fn a_profile_round_trips_the_tool_settings() {
        let settings = AutoToolSettings {
            enabled: true,
            swap_weapon: false,
            instant_swap: false,
            swap_to_delay: 150,
            swap_back: true,
            swap_back_delay: 500,
            require_mouse_down: false,
            only_sneaking: true,
            bedwars_mode: false,
        };

        let mut profile = Profile::default();
        settings.apply_to(&mut profile);
        assert_eq!(AutoToolSettings::from_profile(&profile), settings);

        let json = serde_json::to_string(&profile).expect("profile serializes");
        let restored: Profile = serde_json::from_str(&json).expect("profile parses");
        assert_eq!(AutoToolSettings::from_profile(&restored), settings);

        // A profile that stores out-of-range values is clamped on read.
        let mut raw = Profile::default();
        raw.auto_tool_swap_to_delay = 9_999;
        raw.auto_tool_swap_back_delay = -5;
        let clamped = AutoToolSettings::from_profile(&raw);
        assert_eq!(clamped.swap_to_delay, 500);
        assert_eq!(clamped.swap_back_delay, 0);
    }
}
