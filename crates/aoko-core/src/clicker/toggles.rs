//! Module switches: the authoritative toggle map and the panic reset.
//!
//! Port of `InputHooks.ToggleModule` (`Aoko/Core/InputHooks.cs:254-317`) and
//! `Clicker.TriggerPanicAsync` (`Aoko/Core/Clicker.cs:530-617`), minus the two
//! managed-runtime steps that have no headless analogue: dispatching onto the WPF
//! thread, and putting the main window into stealth mode.
//!
//! Both entry points are *decisions* here. Keybinds (`InputHooks`), the external
//! GUI's command search (`MainWindow.xaml.cs:1723`) and the bridge command
//! dispatcher all funnel into [`toggle`], which is why the ordering of its four
//! guards matters:
//!
//! 1. `panic` is matched case-insensitively before anything else, because it must
//!    work even when the bridge reports no support for it and even in a release
//!    build with dev mode off.
//! 2. An unsupported module is ignored (`GameStateClient.SupportsModule`), so a
//!    keybind whose feature the attached bridge does not implement does nothing
//!    instead of desynchronising the two sides.
//! 3. A dev-only module is ignored unless dev mode is on.
//! 4. An unknown id is ignored, matching the C# `switch` with no `default` arm.

use std::collections::BTreeSet;

use aoko_protocol::module_catalog::ALL;
use aoko_protocol::profile::Profile;

/// The arm-only module: `ToggleModule("autoclicker")` calls `ToggleArmed` rather
/// than flipping a boolean (`InputHooks.cs:272`).
pub const ARM_MODULE: &str = "autoclicker";
/// The panic pseudo-module (`InputHooks.cs:256`).
pub const PANIC_MODULE: &str = "panic";

/// Toggle keys that are AutoClicker sub-settings rather than catalog modules, so
/// they have no keybind of their own but panic must still clear them
/// (`Clicker.cs:570-576`).
pub const PANIC_EXTRA_KEYS: [&str; 3] = [
    "pixelpartyautolook",
    "pixelpartyautowalk",
    "nametaghidevanilla",
];

/// What a module id resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreReason {
    /// The bridge does not implement this module.
    Unsupported,
    /// Dev-only module while dev mode is off.
    DevModeOff,
    /// No such module in the catalog.
    Unknown,
}

/// The result of one toggle request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToggleOutcome {
    /// The caller should run the panic reset.
    Panic,
    /// `autoclicker`: the armed flag flipped to this value.
    Armed(bool),
    /// A module boolean flipped to this value.
    Toggled {
        /// Canonical catalog id.
        module: &'static str,
        /// New value.
        enabled: bool,
    },
    /// Nothing happened.
    Ignored(IgnoreReason),
}

/// The module boolean surface, keyed by canonical catalog id.
///
/// Managed code keeps one `bool` property per module on `Clicker`; a keyed set
/// keeps the same information without 42 near-identical fields, and it makes the
/// panic reset a data list instead of 40 assignments that can silently drift.
#[derive(Debug, Default)]
pub struct ModuleToggles {
    armed: bool,
    enabled: BTreeSet<&'static str>,
}

impl ModuleToggles {
    /// Everything off, not armed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `Clicker.IsArmed`.
    #[must_use]
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// `Clicker.ToggleArmed` (`Clicker.cs:443-459`): a plain flip.
    pub fn toggle_armed(&mut self) {
        self.armed = !self.armed;
    }

    /// Sets the armed flag, as `Disarm` does.
    pub fn set_armed(&mut self, armed: bool) {
        self.armed = armed;
    }

    /// Whether one key is on.
    #[must_use]
    pub fn is_enabled(&self, module: &str) -> bool {
        self.enabled.contains(module)
    }

    /// Sets one key. Accepts non-catalog keys so the panic reset can clear the
    /// AutoClicker sub-settings too.
    pub fn set_enabled(&mut self, module: &'static str, enabled: bool) {
        if enabled {
            let _ = self.enabled.insert(module);
        } else {
            let _ = self.enabled.remove(module);
        }
    }

    /// Ids that are currently on, in catalog order.
    #[must_use]
    pub fn enabled_ids(&self) -> Vec<&'static str> {
        ALL.iter()
            .filter(|entry| self.enabled.contains(entry.id))
            .map(|entry| entry.id)
            .collect()
    }

    /// Number of catalog modules that are on.
    #[must_use]
    pub fn enabled_count(&self) -> usize {
        ALL.iter()
            .filter(|entry| self.enabled.contains(entry.id))
            .count()
    }
}

/// Canonicalises a caller-supplied id, `None` when the catalog has no such module.
#[must_use]
pub fn canonical_id(module_id: &str) -> Option<&'static str> {
    ALL.iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(module_id))
        .map(|entry| entry.id)
}

/// `InputHooks.ToggleModule` (`InputHooks.cs:254-317`).
///
/// `supports` is `GameStateClient.SupportsModule`: whether the attached bridge
/// advertises the feature. Passing a closure keeps the capability lookup out of
/// this crate, which has no transport.
pub fn toggle(
    toggles: &mut ModuleToggles,
    module_id: &str,
    dev_mode: bool,
    supports: impl Fn(&str) -> bool,
) -> ToggleOutcome {
    if module_id.eq_ignore_ascii_case(PANIC_MODULE) {
        return ToggleOutcome::Panic;
    }

    let Some(canonical) = canonical_id(module_id) else {
        return ToggleOutcome::Ignored(IgnoreReason::Unknown);
    };

    if !supports(canonical) {
        return ToggleOutcome::Ignored(IgnoreReason::Unsupported);
    }

    let dev_only = ALL
        .iter()
        .any(|entry| entry.id == canonical && entry.dev_only);
    if dev_only && !dev_mode {
        return ToggleOutcome::Ignored(IgnoreReason::DevModeOff);
    }

    if canonical == ARM_MODULE {
        toggles.toggle_armed();
        return ToggleOutcome::Armed(toggles.armed());
    }

    let enabled = !toggles.is_enabled(canonical);
    toggles.set_enabled(canonical, enabled);
    ToggleOutcome::Toggled {
        module: canonical,
        enabled,
    }
}

/// Modules panic switches **on**: only the left clicker, so the app keeps a
/// usable state after the rest is torn down (`Clicker.cs:543`).
pub const PANIC_ON: [&str; 1] = [ARM_MODULE];

/// Modules and sub-settings panic switches **off**, in the managed order
/// (`Clicker.cs:544-613`). `pixelpartyautolook`, `pixelpartyautowalk` and
/// `nametaghidevanilla` are sub-settings, not catalog modules.
pub const PANIC_OFF: [&str; 42] = [
    "rightclick",
    "jitter",
    "clickinchests",
    "breakblocks",
    "aimassist",
    "triggerbot",
    "killaura",
    "speedbridge",
    "gtbhelper",
    "pixelpartyassist",
    "pixelpartyautolook",
    "pixelpartyautowalk",
    "nametags",
    "nametaghidevanilla",
    "closestplayer",
    "fightstatus",
    "chestesp",
    "cheststealer",
    "refill",
    "blockesp",
    "bedplates",
    "reach",
    "velocity",
    "autototem",
    "autorod",
    "throwpot",
    "autoheal",
    "autotool",
    "antidebuff",
    "hitdelayfix",
    "fireballwarning",
    "fastplace",
    "nohurtcam",
    "sprint",
    "tnttimer",
    "wtap",
    "jumpreset",
    "hitselect",
    "trajectories",
    "murdermystery",
    "breakprogress",
    // `HudEditorActive` is not in the managed panic list; keeping it off matches
    // the "every feature off" intent without changing any published behaviour,
    // because the editor is a modal the panic path closes with the window.
    "hudeditor",
];

/// How long panic waits before dropping the bridge link
/// (`Clicker.cs:586-587`: only when connected).
pub const PANIC_DISCONNECT_DELAY_MS: u64 = 250;

/// Whether panic should wait before disconnecting.
#[must_use]
pub fn panic_disconnect_delay_ms(connected: bool) -> Option<u64> {
    if connected {
        Some(PANIC_DISCONNECT_DELAY_MS)
    } else {
        None
    }
}

/// Applies the panic reset to the switches and the profile.
///
/// Cleared: the armed flag, every module except the left clicker, the overlay list,
/// the logo and Discord RPC. The two timing states are reset by their owners; this
/// function has no handle on them, so the caller resets them alongside.
pub fn panic_reset(toggles: &mut ModuleToggles, profile: &mut Profile) {
    toggles.set_armed(false);

    for module in PANIC_ON {
        toggles.set_enabled(module, true);
    }
    for module in PANIC_OFF {
        toggles.set_enabled(module, false);
    }

    profile.show_module_list = false;
    profile.show_logo = false;
    profile.discord_rpc_enabled = false;
    // `Clicker.FightStatusEnabled = false` in the managed panic (`Clicker.cs:564`). This
    // port has no Clicker property layer, so the profile is where the flag lives, exactly
    // as for the three above: leaving it set would keep `fightStatus` in the config frame
    // true after a panic, which the managed host never sent.
    profile.fight_status_enabled = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn everything_on() -> ModuleToggles {
        let mut toggles = ModuleToggles::new();
        toggles.set_armed(true);
        for entry in ALL.iter() {
            // `panic` is a catalog entry with no boolean behind it: it is matched by
            // name in `toggle` and is never an enabled module.
            if entry.id != PANIC_MODULE {
                toggles.set_enabled(entry.id, true);
            }
        }
        for key in PANIC_EXTRA_KEYS {
            toggles.set_enabled(key, true);
        }
        toggles
    }

    #[test]
    fn panic_is_matched_before_the_support_check() {
        let mut toggles = ModuleToggles::new();
        // Nothing is supported and dev mode is off, yet panic still fires.
        assert_eq!(
            toggle(&mut toggles, "panic", false, |_| false),
            ToggleOutcome::Panic
        );
        assert_eq!(
            toggle(&mut toggles, "PANIC", false, |_| false),
            ToggleOutcome::Panic
        );
        assert_eq!(
            toggle(&mut toggles, "Panic", false, |_| false),
            ToggleOutcome::Panic
        );
    }

    #[test]
    fn the_four_guards_run_in_the_managed_order() {
        let mut toggles = ModuleToggles::new();

        // Unknown ids are ignored, and are checked before support.
        assert_eq!(
            toggle(&mut toggles, "nonsense", true, |_| true),
            ToggleOutcome::Ignored(IgnoreReason::Unknown)
        );

        // Unsupported beats dev-only: `killaura` is dev-only, but this bridge does
        // not offer it, so the reason reported is unsupported.
        assert_eq!(
            toggle(&mut toggles, "killaura", false, |_| false),
            ToggleOutcome::Ignored(IgnoreReason::Unsupported)
        );

        // Supported but dev-only with dev mode off.
        assert_eq!(
            toggle(&mut toggles, "killaura", false, |_| true),
            ToggleOutcome::Ignored(IgnoreReason::DevModeOff)
        );

        // Dev mode on: it toggles.
        assert_eq!(
            toggle(&mut toggles, "killaura", true, |_| true),
            ToggleOutcome::Toggled {
                module: "killaura",
                enabled: true
            }
        );
        assert!(toggles.is_enabled("killaura"));
    }

    #[test]
    fn the_arm_module_toggles_armed_instead_of_a_boolean() {
        let mut toggles = ModuleToggles::new();
        assert!(!toggles.armed());

        assert_eq!(
            toggle(&mut toggles, "autoclicker", false, |_| true),
            ToggleOutcome::Armed(true)
        );
        assert!(toggles.armed());
        // It must not also appear as an enabled module.
        assert!(!toggles.is_enabled("autoclicker"));

        assert_eq!(
            toggle(&mut toggles, "AutoClicker", false, |_| true),
            ToggleOutcome::Armed(false)
        );
        assert!(!toggles.armed());
    }

    #[test]
    fn module_ids_are_case_insensitive_and_canonicalised() {
        let mut toggles = ModuleToggles::new();
        assert_eq!(
            toggle(&mut toggles, "ChestStealer", false, |_| true),
            ToggleOutcome::Toggled {
                module: "cheststealer",
                enabled: true
            }
        );
        assert!(toggles.is_enabled("cheststealer"));
        assert_eq!(canonical_id("REFILL"), Some("refill"));
        assert_eq!(canonical_id("refil"), None);
    }

    #[test]
    fn toggling_twice_returns_to_off() {
        let mut toggles = ModuleToggles::new();
        let _ = toggle(&mut toggles, "sprint", false, |_| true);
        assert_eq!(
            toggle(&mut toggles, "sprint", false, |_| true),
            ToggleOutcome::Toggled {
                module: "sprint",
                enabled: false
            }
        );
        assert!(!toggles.is_enabled("sprint"));
        assert_eq!(toggles.enabled_count(), 0);
    }

    // `Profile` keeps eight cascade-managed fields private, so struct-update syntax
    // is unavailable from outside `aoko-protocol`; the field assignments are the
    // only way to build this fixture.
    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn panic_turns_everything_off_except_the_left_clicker() {
        let mut toggles = everything_on();
        let mut profile = Profile::default();
        profile.show_module_list = true;
        profile.show_logo = true;
        profile.discord_rpc_enabled = true;
        profile.fight_status_enabled = true;
        // Dev mode survives: panic disarms the automation, it does not change the
        // developer's preferences.
        profile.dev_mode = true;

        assert!(toggles.armed());
        assert!(toggles.is_enabled("cheststealer"));

        panic_reset(&mut toggles, &mut profile);

        assert!(!toggles.armed());
        for module in PANIC_OFF {
            assert!(!toggles.is_enabled(module), "{module} must be off");
        }
        for key in PANIC_EXTRA_KEYS {
            assert!(!toggles.is_enabled(key), "{key} must be off");
        }
        // The left clicker is left on so the user can keep clicking. `nickhider` is
        // deliberately *not* in the managed panic list, so it stays on: hiding your
        // nickname has nothing to do with stopping automation.
        assert!(toggles.is_enabled(ARM_MODULE));
        assert_eq!(toggles.enabled_ids(), vec![ARM_MODULE, "nickhider"]);

        assert!(!profile.show_module_list);
        assert!(!profile.show_logo);
        assert!(!profile.discord_rpc_enabled);
        assert!(!profile.fight_status_enabled);
        assert!(profile.dev_mode);
    }

    #[test]
    fn panic_is_idempotent() {
        let mut toggles = everything_on();
        let mut profile = Profile::default();
        panic_reset(&mut toggles, &mut profile);
        let first: Vec<_> = toggles.enabled_ids();
        panic_reset(&mut toggles, &mut profile);
        assert_eq!(toggles.enabled_ids(), first);
    }

    #[test]
    fn the_disconnect_wait_only_applies_when_connected() {
        assert_eq!(panic_disconnect_delay_ms(true), Some(250));
        assert_eq!(panic_disconnect_delay_ms(false), None);
    }
}
