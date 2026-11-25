//! Canonical registry of product modules and which surfaces each must appear on.
//!
//! Ported from `Aoko/Core/ModuleCatalog.cs`. When adding a module, register it
//! here first, then satisfy the surfaces the entry requires.
//!
//! The managed entry carried a `Func<Clicker, bool>`; this crate must not depend
//! on the clicker, so each entry names an [`EnabledFlag`] instead and the caller
//! resolves it against its own state. `ModuleRegistrationTests` checks that
//! contract, and so does [`module_catalog`]'s own test suite.

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
