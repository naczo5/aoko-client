//! Canonical registry of product modules and which surfaces each must appear on.
//!
//! Ported from `Aoko/Core/ModuleCatalog.cs`. When adding a module, register it
//! here first, then satisfy the surfaces the entry requires.
//!
//! The managed entry carried a `Func<Clicker, bool>`; this crate must not depend
//! on the clicker, so each entry names an [`EnabledFlag`] instead and the caller
//! resolves it against its own state. `ModuleRegistrationTests` checks that
//! contract, and so does [`crate::module_catalog`]'s own test suite.

/// Which surfaces a module must appear on.
///
/// A tiny hand-rolled bitset keeps this crate dependency-free while preserving
/// the managed flag semantics (combine with [`Surfaces::union`], test with
/// [`Surfaces::contains`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Surfaces(u8);

impl Surfaces {
    /// No surface.
    pub const NONE: Self = Self(0);
    /// Keybind button on each module card (`KeybindGui`).
    pub const KEYBIND_GUI: Self = Self(1 << 0);
    /// Input-hook module keys, profile defaults, and the toggle switch (`KeybindMaps`).
    pub const KEYBIND_MAPS: Self = Self(1 << 1);
    /// Counted by Discord Rich Presence when enabled (`DiscordRpc`).
    pub const DISCORD_RPC: Self = Self(1 << 2);
    /// Shown in the in-game top-right module list (`OverlayList`).
    pub const OVERLAY_LIST: Self = Self(1 << 3);
    /// Listed in the bridge capability payloads (`Capability`).
    pub const CAPABILITY: Self = Self(1 << 4);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, surface: Self) -> bool {
        self.0 & surface.0 == surface.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// The surface set almost every toggleable module requires.
pub const STANDARD_SURFACES: Surfaces = Surfaces::KEYBIND_GUI
    .union(Surfaces::KEYBIND_MAPS)
    .union(Surfaces::DISCORD_RPC)
    .union(Surfaces::OVERLAY_LIST)
    .union(Surfaces::CAPABILITY);

/// Which clicker toggle an entry reads.
///
/// `Never` is the managed `_ => false`, used by the keybind-only Panic action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnabledFlag {
    Never,
    LeftClick,
    RightClick,
    Jitter,
    ClickInChests,
    BreakBlocks,
    AimAssist,
    Triggerbot,
    KillAura,
    SpeedBridge,
    GtbHelper,
    PixelPartyAssist,
    Nametags,
    NickHider,
    ClosestPlayerInfo,
    FightStatus,
    ChestEsp,
    ChestStealer,
    Refill,
    BlockEsp,
    BedPlates,
    Reach,
    Velocity,
    AutoTotem,
    AutoRod,
    Throwpot,
    AutoHeal,
    AutoTool,
    AntiDebuff,
    HitDelayFix,
    FireballWarning,
    FastPlace,
    NoHurtCam,
    Sprint,
    TntTimer,
    WTap,
    JumpReset,
    HitSelect,
    Trajectories,
    MurderMystery,
    BreakProgress,
    HudEditorActive,
}

/// The switch behind a catalog id, matched case-insensitively.
///
/// `None` for an id the catalog does not carry, and for the entries whose flag is
/// [`EnabledFlag::Never`] — `panic` is the only one — because those have no stored boolean.
/// The panic reset and the app's own id lookups need the same rule, so it lives beside the
/// catalog rather than being re-derived per caller.
#[must_use]
pub fn flag_for_id(module_id: &str) -> Option<EnabledFlag> {
    ALL.iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(module_id))
        .map(|entry| entry.enabled_flag)
        .filter(|flag| *flag != EnabledFlag::Never)
}

/// The `config` frame key each module switch feeds.
///
/// This is the last hop of the chain `flag_for_id` starts: module id -> [`EnabledFlag`] ->
/// frame key. `aoko-core`'s `config_map` reads the switch, not this table, for each key it
/// writes; the table exists so a test can prove the two agree one switch at a time, and so a
/// module added to the catalogue without a frame key fails a test instead of silently
/// shipping a stale boolean.
///
/// Every switch the catalogue carries appears here exactly once, except the two listed in
/// [`MODULE_WIRE_KEY_EXCEPTIONS`]; the keybind-only `panic` entry carries no boolean at all,
/// so it has neither a switch nor a key.
pub const MODULE_WIRE_KEYS: [(EnabledFlag, &str); 39] = [
    // `Clicker.IsArmed` (`GameStateClient.cs`), which is the switch this port calls
    // `LeftClick`: arming is what the left clicker's card toggles.
    (EnabledFlag::LeftClick, "armed"),
    (EnabledFlag::RightClick, "right"),
    (EnabledFlag::BreakBlocks, "breakBlocks"),
    (EnabledFlag::ClickInChests, "clickInChests"),
    (EnabledFlag::AimAssist, "aimAssist"),
    (EnabledFlag::Triggerbot, "triggerbot"),
    (EnabledFlag::KillAura, "killAura"),
    (EnabledFlag::SpeedBridge, "speedBridge"),
    (EnabledFlag::GtbHelper, "gtbHelper"),
    (EnabledFlag::PixelPartyAssist, "pixelPartyAssist"),
    (EnabledFlag::Nametags, "nametags"),
    (EnabledFlag::NickHider, "nickHiderEnabled"),
    (EnabledFlag::ClosestPlayerInfo, "closestPlayerInfo"),
    (EnabledFlag::FightStatus, "fightStatus"),
    (EnabledFlag::ChestEsp, "chestEsp"),
    (EnabledFlag::ChestStealer, "chestStealerEnabled"),
    (EnabledFlag::Refill, "refillEnabled"),
    (EnabledFlag::BlockEsp, "blockEspEnabled"),
    (EnabledFlag::BedPlates, "bedPlatesEnabled"),
    (EnabledFlag::Reach, "reachEnabled"),
    (EnabledFlag::Velocity, "velocityEnabled"),
    (EnabledFlag::AutoTotem, "autoTotemEnabled"),
    (EnabledFlag::AutoRod, "autoRodEnabled"),
    (EnabledFlag::Throwpot, "throwpotEnabled"),
    (EnabledFlag::AutoHeal, "autoHealEnabled"),
    (EnabledFlag::AutoTool, "autoToolEnabled"),
    (EnabledFlag::AntiDebuff, "antiDebuffEnabled"),
    (EnabledFlag::HitDelayFix, "hitDelayFixEnabled"),
    (EnabledFlag::FireballWarning, "fireballWarning"),
    (EnabledFlag::FastPlace, "fastPlace"),
    (EnabledFlag::NoHurtCam, "noHurtCam"),
    (EnabledFlag::Sprint, "sprint"),
    (EnabledFlag::TntTimer, "tntTimer"),
    (EnabledFlag::WTap, "wTap"),
    (EnabledFlag::JumpReset, "jumpReset"),
    (EnabledFlag::HitSelect, "hitSelect"),
    (EnabledFlag::Trajectories, "trajectories"),
    (EnabledFlag::MurderMystery, "murderMystery"),
    (EnabledFlag::BreakProgress, "breakProgress"),
];

/// The switches [`MODULE_WIRE_KEYS`] deliberately leaves out.
///
/// * [`EnabledFlag::Jitter`] — the managed `Clicker.JitterEnabled` was itself a derived
///   property, `RandomizationMode != Basic` (`Clicker.cs:350`), so the frame's `jitter`
///   follows the profile's mode. The switch is never written by the app, so making the frame
///   read it would make the frame disagree with the clicker.
/// * [`EnabledFlag::HudEditorActive`] — the mode is not a stored switch: the controller holds
///   it (`AppController::set_hud_editor`) and passes it to the mapping as a parameter, which
///   is also why it must not enter the switch set (the Discord presence counts that set).
pub const MODULE_WIRE_KEY_EXCEPTIONS: [EnabledFlag; 2] =
    [EnabledFlag::Jitter, EnabledFlag::HudEditorActive];

/// One integer module setting: wire key, profile property, clamp range, default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    /// Key in the `config` frame.
    pub wire_key: &'static str,
    /// Property name on the clicker and on the profile.
    pub property: &'static str,
    pub min: i32,
    pub max: i32,
    pub default: i32,
}

/// One catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub id: &'static str,
    pub display_name: &'static str,
    /// Unique probe string that must appear in `bridge_261.cpp`'s `pushMod` block.
    pub overlay_probe: &'static str,
    pub enabled_flag: EnabledFlag,
    pub required_surfaces: Surfaces,
    /// Hidden in the GUI unless Dev Mode is on.
    pub dev_only: bool,
    /// Declared by the managed host but **set by no entry**. Kept `false`
    /// everywhere: honouring it would invent gating behaviour the shipped app
    /// never had (architecture decision 7).
    pub managed_only: bool,
    /// Card lives in the Beta tab.
    pub beta: bool,
    pub settings: &'static [SettingSpec],
}

const NO_SETTINGS: &[SettingSpec] = &[];

const VELOCITY_SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        wire_key: "velocityHorizontal",
        property: "VelocityHorizontal",
        min: 1,
        max: 100,
        default: 100,
    },
    SettingSpec {
        wire_key: "velocityVertical",
        property: "VelocityVertical",
        min: 1,
        max: 100,
        default: 100,
    },
    SettingSpec {
        wire_key: "velocityChance",
        property: "VelocityChance",
        min: 1,
        max: 100,
        default: 100,
    },
];

const JUMP_RESET_SETTINGS: &[SettingSpec] = &[SettingSpec {
    wire_key: "jumpResetChance",
    property: "JumpResetChance",
    min: 0,
    max: 100,
    default: 80,
}];

/// Surfaces used by the AutoClicker sub-settings: overlay/RPC when active, but
/// not keybindable modules.
const SUB_SETTING_SURFACES: Surfaces = Surfaces::DISCORD_RPC
    .union(Surfaces::OVERLAY_LIST)
    .union(Surfaces::CAPABILITY);

/// Keybind-only emergency action plus the in-game HUD layout editor.
const KEYBIND_ONLY: Surfaces = Surfaces::KEYBIND_GUI.union(Surfaces::KEYBIND_MAPS);

const fn entry(
    id: &'static str,
    display_name: &'static str,
    overlay_probe: &'static str,
    enabled_flag: EnabledFlag,
) -> Entry {
    Entry {
        id,
        display_name,
        overlay_probe,
        enabled_flag,
        required_surfaces: STANDARD_SURFACES,
        dev_only: false,
        managed_only: false,
        beta: false,
        settings: NO_SETTINGS,
    }
}

const fn with_surfaces(mut base: Entry, required_surfaces: Surfaces) -> Entry {
    base.required_surfaces = required_surfaces;
    base
}

const fn with_beta(mut base: Entry) -> Entry {
    base.beta = true;
    base
}

const fn with_dev_only(mut base: Entry) -> Entry {
    base.dev_only = true;
    base
}

const fn with_settings(mut base: Entry, settings: &'static [SettingSpec]) -> Entry {
    base.settings = settings;
    base
}

impl Entry {
    #[must_use]
    pub fn requires(&self, surface: Surfaces) -> bool {
        self.required_surfaces.contains(surface)
    }
}

/// Every registered module, in catalog order.
pub const ALL: [Entry; 42] = [
    entry(
        "autoclicker",
        "AutoClicker",
        "cfg.armed",
        EnabledFlag::LeftClick,
    ),
    entry(
        "rightclick",
        "Right Click",
        "cfg.rightClick",
        EnabledFlag::RightClick,
    ),
    // AutoClicker sub-settings - shown in overlay/RPC when active, but not
    // keybindable modules.
    with_surfaces(
        entry("jitter", "Jitter", "cfg.jitter", EnabledFlag::Jitter),
        SUB_SETTING_SURFACES,
    ),
    with_surfaces(
        entry(
            "clickinchests",
            "Click in Chests",
            "cfg.clickInChests",
            EnabledFlag::ClickInChests,
        ),
        SUB_SETTING_SURFACES,
    ),
    with_surfaces(
        entry(
            "breakblocks",
            "Break Blocks",
            "cfg.breakBlocks",
            EnabledFlag::BreakBlocks,
        ),
        SUB_SETTING_SURFACES,
    ),
    entry(
        "aimassist",
        "Aim Assist",
        "cfg.aimAssist",
        EnabledFlag::AimAssist,
    ),
    entry(
        "triggerbot",
        "Triggerbot",
        "cfg.triggerbot",
        EnabledFlag::Triggerbot,
    ),
    with_beta(with_dev_only(entry(
        "killaura",
        "Kill Aura",
        "cfg.killAura",
        EnabledFlag::KillAura,
    ))),
    entry(
        "speedbridge",
        "SpeedBridge",
        "cfg.speedBridge",
        EnabledFlag::SpeedBridge,
    ),
    entry(
        "gtbhelper",
        "GTB Helper",
        "cfg.gtbHelper",
        EnabledFlag::GtbHelper,
    ),
    entry(
        "pixelpartyassist",
        "Pixel Party Assist",
        "cfg.pixelPartyAssist",
        EnabledFlag::PixelPartyAssist,
    ),
    entry(
        "nametags",
        "Nametags",
        "cfg.nametags",
        EnabledFlag::Nametags,
    ),
    entry(
        "nickhider",
        "Nick Hider",
        "cfg.nickHiderEnabled",
        EnabledFlag::NickHider,
    ),
    entry(
        "closestplayer",
        "Closest Player",
        "cfg.closestPlayer",
        EnabledFlag::ClosestPlayerInfo,
    ),
    entry(
        "fightstatus",
        "Fight Status",
        "cfg.fightStatus",
        EnabledFlag::FightStatus,
    ),
    entry(
        "chestesp",
        "Chest ESP",
        "cfg.chestEsp",
        EnabledFlag::ChestEsp,
    ),
    entry(
        "cheststealer",
        "Chest Stealer",
        "cfg.chestStealer",
        EnabledFlag::ChestStealer,
    ),
    entry("refill", "Refill", "cfg.refill", EnabledFlag::Refill),
    entry(
        "blockesp",
        "Block ESP",
        "cfg.blockEsp",
        EnabledFlag::BlockEsp,
    ),
    entry(
        "bedplates",
        "BedPlates",
        "cfg.bedPlates",
        EnabledFlag::BedPlates,
    ),
    entry("reach", "Reach", "cfg.reachEnabled", EnabledFlag::Reach),
    with_settings(
        entry(
            "velocity",
            "Velocity",
            "cfg.velocityEnabled",
            EnabledFlag::Velocity,
        ),
        VELOCITY_SETTINGS,
    ),
    entry(
        "autototem",
        "AutoTotem",
        "cfg.autoTotemEnabled",
        EnabledFlag::AutoTotem,
    ),
    entry(
        "autorod",
        "Auto Rod",
        "cfg.autoRodEnabled",
        EnabledFlag::AutoRod,
    ),
    entry(
        "throwpot",
        "Throwpot",
        "cfg.throwpotEnabled",
        EnabledFlag::Throwpot,
    ),
    entry(
        "autoheal",
        "AutoHeal",
        "cfg.autoHealEnabled",
        EnabledFlag::AutoHeal,
    ),
    entry(
        "autotool",
        "AutoTool",
        "cfg.autoToolEnabled",
        EnabledFlag::AutoTool,
    ),
    entry(
        "antidebuff",
        "AntiDebuff",
        "cfg.antiDebuffEnabled",
        EnabledFlag::AntiDebuff,
    ),
    entry(
        "hitdelayfix",
        "Hit Delay Fix",
        "cfg.hitDelayFixEnabled",
        EnabledFlag::HitDelayFix,
    ),
    entry(
        "fireballwarning",
        "Fireball Warning",
        "cfg.fireballWarning",
        EnabledFlag::FireballWarning,
    ),
    with_beta(entry(
        "fastplace",
        "Fast Place",
        "cfg.fastPlace",
        EnabledFlag::FastPlace,
    )),
    with_beta(entry(
        "nohurtcam",
        "No Hurt Cam",
        "cfg.noHurtCam",
        EnabledFlag::NoHurtCam,
    )),
    with_beta(entry("sprint", "Sprint", "cfg.sprint", EnabledFlag::Sprint)),
    with_beta(entry(
        "tnttimer",
        "TNT Timer",
        "cfg.tntTimer",
        EnabledFlag::TntTimer,
    )),
    with_beta(entry("wtap", "WTap", "cfg.wTap", EnabledFlag::WTap)),
    with_beta(with_settings(
        entry(
            "jumpreset",
            "Jump Reset",
            "cfg.jumpReset",
            EnabledFlag::JumpReset,
        ),
        JUMP_RESET_SETTINGS,
    )),
    with_beta(entry(
        "hitselect",
        "Hit Select",
        "cfg.hitSelect",
        EnabledFlag::HitSelect,
    )),
    with_beta(entry(
        "trajectories",
        "Trajectories",
        "cfg.trajectories",
        EnabledFlag::Trajectories,
    )),
    with_beta(entry(
        "murdermystery",
        "Murder Mystery",
        "cfg.murderMystery",
        EnabledFlag::MurderMystery,
    )),
    with_beta(entry(
        "breakprogress",
        "Break Progress",
        "cfg.breakProgress",
        EnabledFlag::BreakProgress,
    )),
    // Keybind-only emergency action - not a toggleable feature module.
    with_surfaces(
        entry("panic", "Panic", "", EnabledFlag::Never),
        KEYBIND_ONLY,
    ),
    // In-game HUD layout editor - capability + keybind maps, not RPC/overlay/GUI
    // keybind grid.
    with_surfaces(
        entry("hudeditor", "HUD Editor", "", EnabledFlag::HudEditorActive),
        Surfaces::KEYBIND_MAPS.union(Surfaces::CAPABILITY),
    ),
];

/// Entries requiring every bit of `surface`.
pub fn requiring(surface: Surfaces) -> impl Iterator<Item = &'static Entry> {
    ALL.iter().filter(move |entry| entry.requires(surface))
}

/// Whether a module id is `DevOnly`, case-insensitive.
#[must_use]
pub fn is_dev_only(module_id: &str) -> bool {
    ALL.iter()
        .any(|entry| entry.dev_only && entry.id.eq_ignore_ascii_case(module_id))
}

/// Hidden in the external GUI unless Dev Mode is on.
#[must_use]
pub fn is_gui_visible(entry: &Entry, dev_mode: bool) -> bool {
    !entry.dev_only || dev_mode
}

/// Counts enabled modules that Discord Rich Presence reports.
pub fn count_enabled_for_discord(
    dev_mode: bool,
    is_enabled: impl Fn(EnabledFlag) -> bool,
) -> usize {
    requiring(Surfaces::DISCORD_RPC)
        .filter(|entry| is_gui_visible(entry, dev_mode) && is_enabled(entry.enabled_flag))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_payload::CONFIG_KEYS;

    /// The switch-to-frame-key table, guarded the way the payload guard guards itself: every
    /// switch the catalogue carries is mapped exactly once or listed as an exception, nothing
    /// is mapped twice, every mapped key is a real frame key, and every exception is a switch
    /// the catalogue actually carries. A module added without a frame key fails here rather
    /// than shipping a boolean nobody updates.
    #[test]
    fn every_catalog_switch_has_exactly_one_frame_key() {
        for entry in ALL {
            let flag = entry.enabled_flag;
            if flag == EnabledFlag::Never {
                assert!(
                    !MODULE_WIRE_KEYS.iter().any(|(mapped, _)| *mapped == flag),
                    "{} has no boolean, so it cannot feed a frame key",
                    entry.id
                );
                continue;
            }
            if MODULE_WIRE_KEY_EXCEPTIONS.contains(&flag) {
                assert!(
                    !MODULE_WIRE_KEYS.iter().any(|(mapped, _)| *mapped == flag),
                    "{} is an exception, so the table must not carry its switch",
                    entry.id
                );
                continue;
            }
            let mapped = MODULE_WIRE_KEYS
                .iter()
                .filter(|(mapped, _)| *mapped == flag)
                .count();
            assert_eq!(
                mapped, 1,
                "{} ({flag:?}) must feed exactly one frame key, and feeds {mapped}",
                entry.id
            );
        }

        for (index, (flag, key)) in MODULE_WIRE_KEYS.iter().enumerate() {
            assert!(
                CONFIG_KEYS.contains(key),
                "{key} is not a config frame key, so {flag:?} would feed nothing"
            );
            assert!(
                !MODULE_WIRE_KEYS[..index]
                    .iter()
                    .any(|(_, other)| other == key),
                "{key} is mapped twice"
            );
        }

        for flag in MODULE_WIRE_KEY_EXCEPTIONS {
            assert!(
                ALL.iter().any(|entry| entry.enabled_flag == flag),
                "{flag:?} is an exception the catalogue does not carry"
            );
        }
    }
}
