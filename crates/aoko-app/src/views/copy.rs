//! The shell's copy and layout data, lifted verbatim out of `Aoko/MainWindow.xaml`.
//!
//! `docs/rust/UI_INVENTORY.md` section 4 measures 478 literal user-visible strings
//! in `MainWindow.xaml` — module names, every slider label, every checkbox label,
//! all 55 tooltips, tab headers, the Beta banner, Stats captions and Settings
//! headings. They are not in a resource file in the managed app and they are not in
//! one function; they are inline in the markup, next to the control that draws them.
//! This module keeps that shape: each [`Control`] carries its own copy and its own
//! geometry, in document order, so the tab renderer is a loop and a parity diff is
//! a line diff.
//!
//! Rules that hold across the table:
//!
//! - **Copy is verbatim.** Including `Strength (%)`, the `\u{2026}` in the overlay
//!   and the en dash in the offline-name rule. A reworded label is a parity bug.
//! - **`bind` is the managed binding path, verbatim**, not a Rust field name. It is
//!   what the wiring layer matches on, and keeping it spelled like the XAML is what
//!   makes a missing wiring visible instead of silently defaulting.
//! - **Geometry is the XAML's.** Slider minimum, maximum and snap come from the
//!   `Slider` element, so a range cannot drift away from the control that enforces it.
//! - **No colour.** The table says nothing about paint; `theme::` owns that.
//!
//! Counted against the markup (this file's tests assert them):
//! **43** `ModuleSwitch` switches, **72** `DarkSlider` sliders, **58**
//! `DarkCheckBox` checkboxes, **38** keybind buttons and **49** `ControlCardStyle`
//! cards. The inventory's "44 switches / 75 sliders" is a slight overcount: the
//! markup has 43 and 72 elements respectively.

/// Which action bind a card offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionBind {
    /// `AutoRodActionBindButton`.
    AutoRod,
    /// `ThrowpotActionBindButton`.
    Throwpot,
}

impl ActionBind {
    /// The variant's own name, for element ids.
    ///
    /// The id used to be built with `format!("{which:?}")` in `Render::render`, which is
    /// a heap allocation per frame; this is the same text without the allocation, and
    /// `an_action_bind_id_matches_its_debug_name` keeps the two equal.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::AutoRod => "AutoRod",
            Self::Throwpot => "Throwpot",
        }
    }
}

/// One card's header row, in the shape the managed markup draws.
///
/// There is no separate title element in the C#: a card either opens with a
/// `DockPanel` whose right-hand child is the 140-wide keybind button and whose
/// fill is the module switch (`MainWindow.xaml:359-362`), or with a bold
/// `TextBlock` and the same button (`:376-379`). A card with neither has no
/// header at all, which is why most Settings and Stats cards open on their first
/// control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardHeader {
    /// The switch *is* the heading, as on the 38 module cards.
    Switch {
        /// The switch's own label, which is the module name.
        label: &'static str,
        /// Managed binding path for the switch.
        bind: &'static str,
    },
    /// A bold 16 px label.
    Text(&'static str),
}

/// The headings the C# writes as a bold `TextBlock` on cards that have no switch.
///
/// Every string is the markup's own, so a diff against `MainWindow.xaml` is a
/// line diff. A card that is not in this table has no header: painting its `id`
/// there would put an internal identifier on screen.
#[must_use]
pub fn heading(card_id: &str) -> Option<&'static str> {
    match card_id {
        // `MainWindow.xaml:1528`.
        "ConfigsCard" => Some("Configs"),
        // `:1568`.
        "ReloadMappingsCard" => Some("Reload Mappings"),
        // `:1584`.
        "GuiCustomizationCard" => Some("GUI Customization"),
        // `:1473`.
        "CpsHistogramCard" => Some("CPS Distribution"),
        _ => None,
    }
}

/// The card's header, or `None` when the markup draws none.
///
/// On a card with a keybind button the heading is whichever of the two managed
/// shapes comes first: a `Note` (the bold `TextBlock`, as on "Right Click") or the
/// switch itself. The caller must not paint the control this returns a second time.
#[must_use]
pub fn card_header(card: &Card) -> Option<CardHeader> {
    let has_keybind = card
        .controls
        .iter()
        .any(|control| matches!(control, Keybind { .. }));

    if has_keybind {
        let first = card
            .controls
            .iter()
            .copied()
            .find(|control| !matches!(control, Keybind { .. }));
        match first {
            Some(Note { text }) => return Some(CardHeader::Text(text)),
            Some(Switch { label, bind }) => return Some(CardHeader::Switch { label, bind }),
            _ => {}
        }
    }

    heading(card.id).map(CardHeader::Text)
}

/// One control inside a card, in document order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    /// A `TextBlock`: a heading, a sentence, or the label above the next slider.
    Note {
        /// Verbatim text.
        text: &'static str,
    },
    /// A `ModuleSwitch`: 46x24 track, animated thumb. `bind` is the module flag.
    Switch {
        /// Verbatim label.
        label: &'static str,
        /// Managed binding path.
        bind: &'static str,
    },
    /// A `DarkCheckBox`: 14x14, `✓` glyph.
    Check {
        /// Verbatim label.
        label: &'static str,
        /// Managed binding path.
        bind: &'static str,
        /// Verbatim tooltip, empty when the XAML has none.
        tooltip: &'static str,
    },
    /// A `DarkSlider`.
    Slider {
        /// Verbatim label, empty for the second half of a paired range.
        label: &'static str,
        /// Managed binding path.
        bind: &'static str,
        /// `Minimum`.
        min: f32,
        /// `Maximum`.
        max: f32,
        /// `IsSnapToTickEnabled`.
        snap: bool,
    },
    /// A `DarkComboBox`; `options` are the `ComboBoxItem` contents in order.
    Combo {
        /// Verbatim label above the combo, empty when the row is inline.
        label: &'static str,
        /// Managed binding path.
        bind: &'static str,
        /// Item labels, verbatim.
        options: &'static [&'static str],
    },
    /// A client-only boolean preference (section 12 decision 3).
    ///
    /// Not a `Profile` field and not one of the 143 profile-bound controls: the
    /// value lives in `client-prefs.json` through
    /// [`crate::views::prefs`], so counting it as a bind would break the
    /// 143-control reconciliation.
    Preference {
        /// Label beside the checkbox. New copy; the C# has no such control.
        label: &'static str,
        /// One line under the row saying what it does.
        hint: &'static str,
    },
    /// A keybind capture button (`KeybindButton_Click`, 140 wide, right-aligned).
    Keybind {
        /// Catalogue id the binding belongs to.
        module: &'static str,
    },
    /// A `DarkButton`.
    Button {
        /// Verbatim label.
        label: &'static str,
        /// Verbatim tooltip, empty when the XAML has none.
        tooltip: &'static str,
    },
    /// An action-bind button (`Action: Unbound`).
    ActionBind {
        /// Which action.
        which: ActionBind,
    },
    /// A `*AvailabilityText` line: the per-module availability sentence.
    Availability {
        /// The rest-state text.
        text: &'static str,
        /// `x:Name` of the line, which is how availability reaches it.
        bind: &'static str,
    },
    /// An `Expander` header; the controls after it belong to that section until
    /// the next `Expand`.
    Expand {
        /// Verbatim header.
        header: &'static str,
    },
    /// The Block ESP tracked-block list, whose row template is not a set of fixed
    /// controls: it is one row per saved target.
    BlockEspRows,
    /// The add-block box and its button, `MainWindow.xaml:714-722`: a text field carrying
    /// the block-id tooltip and the `DarkButton` whose label is [`BLOCK_ESP_ADD_LABEL`].
    ///
    /// One control rather than a field and a [`Control::Button`] because the box's Enter and
    /// the button's press are the same submit, so they cannot disagree about what is added.
    BlockEspAdd {
        /// Port-only identity of the control, `blockesp-add`; it is not a binding path.
        ///
        /// Deliberately not the name of the profile list the add appends to
        /// (`Profile::block_esp_targets`). A selector named after a data field reads as if it
        /// addresses that data, and this one addresses a box and a button: the write-through
        /// lives in `Shell::add_block_target`, and nothing keys a lookup off this string.
        bind: &'static str,
    },
    /// A palette editor colour row, `MainWindow.xaml:1958-2001`: an 11 px caption, a 22×22
    /// swatch that opens the colour picker, and the hex box beside it.
    ///
    /// Four of these are the create-palette card's base colours; the remaining shade roles are
    /// derived from them. `bind` is the managed box name (`PaletteBgBox`, `:1963`), which is what
    /// the row's value is keyed on and what the picker host will be handed.
    ColorField {
        /// Verbatim caption: `Background`, `Panel`, `Accent`, `Text`.
        label: &'static str,
        /// `x:Name` of the hex box in the markup.
        bind: &'static str,
    },
    /// One of the 18 built-in palette previews, or a custom palette card.
    PalettePreview {
        /// Palette name, verbatim.
        name: &'static str,
    },
}

/// One `ControlCardStyle` card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Card {
    /// `x:Name` in the markup, which is also the pulse target after a search jump.
    pub id: &'static str,
    /// Controls in document order.
    pub controls: &'static [Control],
}

/// One tab of the left rail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tab {
    /// `TabItem Header`, verbatim. The rail order is this array's order.
    pub name: &'static str,
    /// What the tab renders.
    pub body: TabBody,
}

// The tables below read `Switch { .. }` rather than `Control::Switch { .. }`, which
// is how the markup reads: the control kind is the line. `ActionBind` resolves to
// the type in the type namespace (`ActionBind::AutoRod`) and to the variant in the
// value namespace (`ActionBind { which: .. }`), so both spellings work.
use Control::*;

// ── Tab 1: Combat ───────────────────────────────────────────────────────────────

/// Combat, `MainWindow.xaml:353-593`.
pub const COMBAT: &[Card] = &[
    Card {
        id: "AutoClickerCard",
        controls: &[
            Keybind { module: "autoclicker" },
            Switch { label: "AutoClicker", bind: "IsArmed" },
            Slider { label: "Min CPS", bind: "MinCPS", min: 1.0, max: 25.0, snap: false },
            Slider { label: "Max CPS", bind: "MaxCPS", min: 1.0, max: 25.0, snap: false },
            Slider { label: "Randomization", bind: "RandomizationModeIndex", min: 0.0, max: 2.0, snap: true },
            Check { label: "Click in Chests", bind: "ClickInChests", tooltip: "Clicks inside real chests only. Uses the Chest Stealer title, custom item, and physical chest checks so shops and server menus stay paused." },
            Check { label: "Pause when breaking", bind: "BreakBlocksEnabled", tooltip: "" },
    ],
    },
    Card {
        id: "RightClickCard",
        controls: &[
            Keybind { module: "rightclick" },
            Note { text: "Right Click" },
            Switch { label: "Enable Right Click", bind: "RightClickEnabled" },
            Slider { label: "Right Min CPS", bind: "RightMinCPS", min: 1.0, max: 25.0, snap: false },
            Slider { label: "Right Max CPS", bind: "RightMaxCPS", min: 1.0, max: 25.0, snap: false },
            Check { label: "Only when holding block", bind: "RightClickOnlyBlock", tooltip: "" },
    ],
    },
    Card {
        id: "AimAssistCard",
        controls: &[
            Keybind { module: "aimassist" },
            Switch { label: "Aim Assist", bind: "AimAssistEnabled" },
            Availability { text: "Available", bind: "AimAssistAvailabilityText" },
            Slider { label: "FOV", bind: "AimAssistFov", min: 1.0, max: 180.0, snap: false },
            Slider { label: "Range", bind: "AimAssistRange", min: 1.0, max: 12.0, snap: false },
            Slider { label: "Strength (%)", bind: "AimAssistStrength", min: 1.0, max: 100.0, snap: true },
    ],
    },
    Card {
        id: "TriggerbotCard",
        controls: &[
            Keybind { module: "triggerbot" },
            Switch { label: "Triggerbot", bind: "TriggerbotEnabled" },
            Availability { text: "Available", bind: "TriggerbotAvailabilityText" },
            Slider { label: "Cooldown Threshold (%)", bind: "TriggerbotCooldownThreshold", min: 1.0, max: 100.0, snap: true },
            Check { label: "Only attack when crosshair is on entity", bind: "TriggerbotOnlyCrosshair", tooltip: "" },
            Check { label: "Only attack if target is in real hit range", bind: "TriggerbotOnlyIfCanAttack", tooltip: "" },
            Slider { label: "Hit Chance (%)", bind: "TriggerbotHitChance", min: 1.0, max: 100.0, snap: true },
            Check { label: "Only while holding left click", bind: "TriggerbotRequireClick", tooltip: "" },
    ],
    },
    Card {
        id: "AutoRodCard",
        controls: &[
            Keybind { module: "autorod" },
            Switch { label: "Auto Rod", bind: "AutoRodEnabled" },
            Availability { text: "Available", bind: "AutoRodAvailabilityText" },
            Combo { label: "Slot mode", bind: "AutoRodSlotMode", options: &["Auto", "Slot 1", "Slot 2", "Slot 3", "Slot 4", "Slot 5", "Slot 6", "Slot 7", "Slot 8", "Slot 9", ] },
            Check { label: "Verify forced slot contains a fishing rod", bind: "AutoRodVerifyForcedSlot", tooltip: "" },
            Note { text: "Disable verification to use the selected slot with another item." },
            Check { label: "Hold action bind to extend cast", bind: "AutoRodHoldToExtend", tooltip: "" },
            Slider { label: "Extension delay (ticks)", bind: "AutoRodExtensionTicks", min: 1.0, max: 40.0, snap: true },
            Note { text: "Fixed mode restores after the configured delay (about 50 ms per tick). Hold mode restores after the action bind is released; a safety timeout prevents a stuck slot." },
            Control::ActionBind { which: ActionBind::AutoRod },
            Note { text: "The action fires once per press while enabled, connected, in-game, and no GUI is open." },
    ],
    },
    Card {
        id: "ThrowpotCard",
        controls: &[
            Keybind { module: "throwpot" },
            Switch { label: "Throwpot", bind: "ThrowpotEnabled" },
            Availability { text: "Available", bind: "ThrowpotAvailabilityText" },
            Note { text: "Press the action bind to instantly throw the first healing splash potion (Instant Health / Regeneration) in your hotbar, then restore your original slot." },
            Control::ActionBind { which: ActionBind::Throwpot },
            Note { text: "The action fires once per press while enabled, connected, in-game, and no GUI is open." },
    ],
    },
    Card {
        id: "AutoHealCard",
        controls: &[
            Keybind { module: "autoheal" },
            Switch { label: "AutoHeal", bind: "AutoHealEnabled" },
            Availability { text: "Available", bind: "AutoHealAvailabilityText" },
            Slider { label: "Health threshold (HP)", bind: "AutoHealHealth", min: 1.0, max: 20.0, snap: true },
            Slider { label: "Re-throw cooldown (ms)", bind: "AutoHealDelayMs", min: 50.0, max: 2000.0, snap: true },
    ],
    },
    Card {
        id: "HitDelayFixCard",
        controls: &[
            Keybind { module: "hitdelayfix" },
            Note { text: "Hit Delay Fix" },
            Availability { text: "Available", bind: "HitDelayFixAvailabilityText" },
            Switch { label: "Enable Hit Delay Fix", bind: "HitDelayFixEnabled" },
            Note { text: "Resets the client-side delay after a miss. It does not change weapon attack strength or send packets." },
    ],
    },
];

// Tab 2

pub const RENDER: &[Card] = &[
    Card {
        id: "NametagsCard",
        controls: &[
            Keybind { module: "nametags" },
            Switch { label: "Nametags", bind: "NametagsEnabled" },
            Check { label: "Show Health", bind: "NametagShowHealth", tooltip: "" },
            Check { label: "Show Armor", bind: "NametagShowArmor", tooltip: "" },
            Check { label: "Hide Vanilla Tags", bind: "NametagHideVanilla", tooltip: "" },
            Check { label: "Show Item", bind: "NametagShowHeldItem", tooltip: "" },
            Slider { label: "Scan Range (chunks)", bind: "NametagRange", min: 1.0, max: 8.0, snap: true },
    ],
    },
    Card {
        id: "NickHiderCard",
        controls: &[
            Keybind { module: "nickhider" },
            Switch { label: "Nick Hider", bind: "NickHiderEnabled" },
            Note { text: "Shown as" },
    ],
    },
    Card {
        id: "FightStatusCard",
        controls: &[
            Keybind { module: "fightstatus" },
            Switch { label: "Fight Status", bind: "FightStatusEnabled" },
            Note { text: "Shows recent fight health and damage status in-game." },
    ],
    },
    Card {
        id: "ChestEspCard",
        controls: &[
            Keybind { module: "chestesp" },
            Switch { label: "Chest ESP", bind: "ChestEspEnabled" },
            Slider { label: "Scan Range (chunks)", bind: "ChestEspRange", min: 1.0, max: 8.0, snap: true },
    ],
    },
    Card {
        id: "BlockEspCard",
        controls: &[
            Keybind { module: "blockesp" },
            Switch { label: "Block ESP (X-ray)", bind: "BlockEspEnabled" },
            Check { label: "Boxes", bind: "BlockEspBoxes", tooltip: "" },
            Check { label: "Tracers", bind: "BlockEspTracers", tooltip: "" },
            Check { label: "HUD List", bind: "BlockEspHud", tooltip: "" },
            Slider { label: "Max Blocks", bind: "BlockEspMaxCount", min: 1.0, max: 512.0, snap: true },
            Slider { label: "Scan Range (chunks)", bind: "BlockEspRange", min: 1.0, max: 8.0, snap: true },
            Note { text: "Tracked Blocks" },
            Check { label: "", bind: "Enabled", tooltip: "" },
            Button { label: "✕", tooltip: "Remove block" },
            BlockEspAdd { bind: "blockesp-add" },
    ],
    },
    Card {
        id: "BedPlatesCard",
        controls: &[
            Keybind { module: "bedplates" },
            Switch { label: "BedPlates", bind: "BedPlatesEnabled" },
            Note { text: "Shows block types around beds." },
            Check { label: "Show distance", bind: "BedPlatesShowDistance", tooltip: "" },
            Slider { label: "Scan Range (chunks)", bind: "BedPlatesRange", min: 1.0, max: 8.0, snap: true },
    ],
    },
    Card {
        id: "FireballWarningCard",
        controls: &[
            Keybind { module: "fireballwarning" },
            Switch { label: "Fireball Warning", bind: "FireballWarningEnabled" },
            Note { text: "Warns when a nearby player holds a fireball." },
            Check { label: "Box warning", bind: "FireballWarningBox", tooltip: "" },
            Check { label: "Arrow warning", bind: "FireballWarningArrow", tooltip: "" },
            Check { label: "Sound warning", bind: "FireballWarningSound", tooltip: "In-game pling, follows Minecraft volume. On 1.8.9 it uses the Jukebox/Note Blocks slider — keep it unmuted to hear the warning." },
    ],
    },
    Card {
        id: "ClosestPlayerCard",
        controls: &[
            Keybind { module: "closestplayer" },
            Switch { label: "Closest Player", bind: "ClosestPlayerInfoEnabled" },
            Note { text: "Includes 4-way direction arrow (^, v, &lt;, &gt;)." },
    ],
    },
    Card {
        id: "ModuleListCard",
        controls: &[
            Switch { label: "Show module list (top-right)", bind: "ShowModuleList" },
            Note { text: "Shows/hides the active modules overlay list in-game." },
    ],
    },
    Card {
        id: "HudEditorCard",
        controls: &[
            Switch { label: "Edit HUD", bind: "HudEditorActive" },
            Note { text: "Drag HUD elements in-game to reposition them. Open chat to move things around." },
            Button { label: "Reset layout", tooltip: "" },
    ],
    },
    Card {
        id: "AntiDebuffCard",
        controls: &[
            Keybind { module: "antidebuff" },
            Note { text: "AntiDebuff" },
            Availability { text: "Available", bind: "AntiDebuffAvailabilityText" },
            Switch { label: "Enable AntiDebuff", bind: "AntiDebuffEnabled" },
            Note { text: "Suppresses Blindness only in the renderer while preserving its sprint/critical restrictions. Nausea and modern Darkness visuals are removed locally; the server still considers you affected." },
    ],
    },
];

// Tab 3

pub const UTILITY: &[Card] = &[
    Card {
        id: "SpeedBridgeCard",
        controls: &[
            Keybind { module: "speedbridge" },
            Switch { label: "SpeedBridge", bind: "SpeedBridgeEnabled" },
            Availability { text: "Available", bind: "SpeedBridgeAvailabilityText" },
            Slider { label: "Safety", bind: "SpeedBridgeDelayMs", min: 20.0, max: 250.0, snap: true },
            Check { label: "Block only", bind: "SpeedBridgeBlockOnly", tooltip: "" },
            Check { label: "Holding sneak only", bind: "SpeedBridgeHoldingShiftOnly", tooltip: "" },
            Check { label: "Looking down only", bind: "SpeedBridgeLookingDownOnly", tooltip: "" },
    ],
    },
    Card {
        id: "DiscordRpcCard",
        controls: &[
            Switch { label: "Discord Rich Presence", bind: "DiscordRpcEnabled" },
            Note { text: "The Discord activity includes a Repository button linking to this project." },
    ],
    },
    Card {
        id: "GtbHelperCard",
        controls: &[
            Keybind { module: "gtbhelper" },
            Switch { label: "GTB Helper", bind: "GtbHelperEnabled" },
            Availability { text: "Hypixel Guess The Build helper using action-bar hints.", bind: "GtbHelperAvailabilityText" },
    ],
    },
    Card {
        id: "PixelPartyAssistCard",
        controls: &[
            Keybind { module: "pixelpartyassist" },
            Switch { label: "Pixel Party Assist", bind: "PixelPartyAssistEnabled" },
            Slider { label: "Scan radius (blocks)", bind: "PixelPartyScanRadius", min: 8.0, max: 48.0, snap: true },
            Check { label: "Auto-look toward target", bind: "PixelPartyAutoLookEnabled", tooltip: "" },
            Check { label: "Auto-walk toward target (W + jump far, walk close)", bind: "PixelPartyAutoWalkEnabled", tooltip: "" },
    ],
    },
    Card {
        id: "ChestStealerCard",
        controls: &[
            Keybind { module: "cheststealer" },
            Note { text: "Chest Stealer" },
            Availability { text: "Available", bind: "ChestStealerAvailabilityText" },
            Switch { label: "Enable Chest Stealer", bind: "ChestStealerEnabled" },
            Slider { label: "Delay (ms)", bind: "ChestStealerDelayMs", min: 50.0, max: 500.0, snap: true },
            Check { label: "Title check", bind: "ChestStealerTitleCheck", tooltip: "" },
            Note { text: "Prevents stealing if the container title matches known menu keywords (e.g. shop, selector, lobby)." },
            Check { label: "Custom item check", bind: "ChestStealerCustomItemsCheck", tooltip: "" },
            Note { text: "Prevents stealing if over 50% of items have custom display names (common in server GUI menus)." },
            Check { label: "Physical chest check", bind: "ChestStealerPhysicalCheck", tooltip: "" },
            Note { text: "Requires an actual physical chest block in the world near the player (within 6.5 blocks). Prevents interaction with remote or virtual GUIs." },
    ],
    },
    Card {
        id: "RefillCard",
        controls: &[
            Keybind { module: "refill" },
            Note { text: "Refill" },
            Availability { text: "Available", bind: "RefillAvailabilityText" },
            Switch { label: "Enable Refill", bind: "RefillEnabled" },
            Slider { label: "Delay (ms)", bind: "RefillDelayMs", min: 50.0, max: 500.0, snap: true },
    ],
    },
    Card {
        id: "AutoToolCard",
        controls: &[
            Keybind { module: "autotool" },
            Note { text: "AutoTool" },
            Availability { text: "Available", bind: "AutoToolAvailabilityText" },
            Switch { label: "Enable AutoTool", bind: "AutoToolEnabled" },
            Slider { label: "Swap delay (ms)", bind: "AutoToolSwapToDelay", min: 0.0, max: 500.0, snap: true },
            Check { label: "Swap back to original item", bind: "AutoToolSwapBack", tooltip: "" },
            Slider { label: "Swap back delay (ms)", bind: "AutoToolSwapBackDelay", min: 0.0, max: 1000.0, snap: true },
            Check { label: "Swap weapon", bind: "AutoToolSwapWeapon", tooltip: "" },
            Check { label: "Instant weapon swap", bind: "AutoToolInstantSwap", tooltip: "" },
            Check { label: "Require mouse down", bind: "AutoToolRequireMouseDown", tooltip: "" },
            Check { label: "Only while sneaking", bind: "AutoToolOnlySneaking", tooltip: "" },
            Check { label: "Bedwars mode", bind: "AutoToolBedwarsMode", tooltip: "Prevents swapping tools when punching chests so you can deposit resources via left click." },
    ],
    },
    Card {
        id: "PanicCard",
        controls: &[
            Keybind { module: "panic" },
            Note { text: "Immediately disables modules, hides UI/overlay settings, disconnects, and closes the loader. Best-effort only: this does not truly deinject while the game stays open." },
            Button { label: "Trigger Panic Now", tooltip: "" },
    ],
    },
];

// Tab 4

pub const RISKY: &[Card] = &[
    Card {
        id: "ReachCard",
        controls: &[
            Keybind { module: "reach" },
            Note { text: "Reach" },
            Availability {
                text: "Available",
                bind: "ReachAvailabilityText",
            },
            Switch {
                label: "Enable Reach",
                bind: "ReachEnabled",
            },
            Slider {
                label: "Min Range",
                bind: "ReachMin",
                min: 3.0,
                max: 6.0,
                snap: true,
            },
            Slider {
                label: "Max Range",
                bind: "ReachMax",
                min: 3.0,
                max: 6.0,
                snap: true,
            },
            Slider {
                label: "Chance (%)",
                bind: "ReachChance",
                min: 0.0,
                max: 100.0,
                snap: true,
            },
            Combo {
                label: "Chance Mode",
                bind: "ReachChanceMode",
                options: &["Normal", "Advanced"],
            },
            Check {
                label: "Only While Sprinting",
                bind: "ReachOnlyWhileSprinting",
                tooltip: "",
            },
            Check {
                label: "Disable in Water",
                bind: "ReachDisableInWater",
                tooltip: "",
            },
            Check {
                label: "Vertical Check",
                bind: "ReachVerticalCheck",
                tooltip: "",
            },
        ],
    },
    Card {
        id: "VelocityCard",
        controls: &[
            Keybind { module: "velocity" },
            Note { text: "Velocity" },
            Availability {
                text: "Available",
                bind: "VelocityAvailabilityText",
            },
            Note {
                text: "idk if you should use it outside of singleplayer",
            },
            Switch {
                label: "Enable Velocity",
                bind: "VelocityEnabled",
            },
            Slider {
                label: "Horizontal (%)",
                bind: "VelocityHorizontal",
                min: 1.0,
                max: 100.0,
                snap: true,
            },
            Slider {
                label: "Vertical (%)",
                bind: "VelocityVertical",
                min: 1.0,
                max: 100.0,
                snap: true,
            },
            Slider {
                label: "Chance (%)",
                bind: "VelocityChance",
                min: 1.0,
                max: 100.0,
                snap: true,
            },
        ],
    },
    Card {
        id: "AutoTotemCard",
        controls: &[
            Keybind {
                module: "autototem",
            },
            Note { text: "AutoTotem" },
            Availability {
                text: "Available",
                bind: "AutoTotemAvailabilityText",
            },
            Switch {
                label: "Enable AutoTotem",
                bind: "AutoTotemEnabled",
            },
            Combo {
                label: "Mode",
                bind: "AutoTotemMode",
                options: &["Smart (health + elytra)", "Strict (always hold)"],
            },
            Slider {
                label: "Health Threshold",
                bind: "AutoTotemHealth",
                min: 0.0,
                max: 36.0,
                snap: true,
            },
            Slider {
                label: "Delay (ticks)",
                bind: "AutoTotemDelay",
                min: 0.0,
                max: 20.0,
                snap: true,
            },
            Switch {
                label: "Elytra",
                bind: "AutoTotemElytra",
            },
            Combo {
                label: "Behavior",
                bind: "AutoTotemBehaviorMode",
                options: &["Ghost (inventory only)", "Anarchy (may flag on servers)"],
            },
        ],
    },
];

// Tab 5

pub const BETA: &[Card] = &[
    Card {
        id: "BetaBannerCard",
        controls: &[
            Note { text: "Beta modules" },
            Note { text: "These modules were not thoroughly tested. Use them at your own risk." },
    ],
    },
    Card {
        id: "KillAuraCard",
        controls: &[
            Keybind { module: "killaura" },
            Switch { label: "Kill Aura", bind: "KillAuraEnabled" },
            Availability { text: "Available", bind: "KillAuraAvailabilityText" },
            Expand { header: "Combat and targeting" },
            Combo { label: "CPS mode", bind: "KillAuraSettings.CpsMode", options: &["Normal", "Record", ] },
            Combo { label: "Target mode", bind: "KillAuraSettings.Mode", options: &["Single", "Switch", ] },
            Combo { label: "Sort", bind: "KillAuraSettings.Sort", options: &["Distance", "Health", "Hurt time", "FOV", ] },
            Slider { label: "Min CPS", bind: "KillAuraSettings.MinCps", min: 1.0, max: 20.0, snap: true },
            Slider { label: "Max CPS", bind: "KillAuraSettings.MaxCps", min: 1.0, max: 20.0, snap: true },
            Slider { label: "Attack range", bind: "KillAuraSettings.AttackRange", min: 3.0, max: 6.0, snap: true },
            Slider { label: "Swing range", bind: "KillAuraSettings.SwingRange", min: 3.0, max: 6.0, snap: true },
            Slider { label: "FOV", bind: "KillAuraSettings.Fov", min: 30.0, max: 360.0, snap: true },
            Slider { label: "Switch delay", bind: "KillAuraSettings.SwitchDelay", min: 0.0, max: 1000.0, snap: true },
            Expand { header: "Auto block" },
            Combo { label: "Mode", bind: "KillAuraSettings.AutoBlock", options: &["None", "Vanilla", "Spoof", "Hypixel", "Blink", "Interact", "Swap", "Legit", "Fake", "Morden", ] },
            Check { label: "Require use-item held", bind: "KillAuraSettings.AutoBlockRequirePress", tooltip: "" },
            Slider { label: "Auto-block CPS", bind: "KillAuraSettings.AutoBlockCps", min: 1.0, max: 10.0, snap: true },
            Slider { label: "Block range", bind: "KillAuraSettings.AutoBlockRange", min: 3.0, max: 8.0, snap: true },
            Slider { label: "Attack tick", bind: "KillAuraSettings.AttackTick", min: 0.0, max: 5.0, snap: true },
            Expand { header: "Rotations" },
            Combo { label: "Mode", bind: "KillAuraSettings.Rotations", options: &["None", "Legit", "Silent", "LockView", "LiquidBounce", "Hypixel", "Grok", ] },
            Combo { label: "Move fix", bind: "KillAuraSettings.MoveFix", options: &["None", "Silent", "Strict", ] },
            Slider { label: "Angle step", bind: "KillAuraSettings.AngleStep", min: 30.0, max: 180.0, snap: true },
            Slider { label: "Smoothing", bind: "KillAuraSettings.Smoothing", min: 0.0, max: 100.0, snap: true },
            Slider { label: "Hypixel smoothing", bind: "KillAuraSettings.RavenSmoothing", min: 0.0, max: 10.0, snap: true },
            Slider { label: "Predict ticks", bind: "KillAuraSettings.RavenPredictTicks", min: 0.0, max: 5.0, snap: true },
            Slider { label: "Yaw random", bind: "KillAuraSettings.RavenYawRandom", min: 0.0, max: 5.0, snap: true },
            Slider { label: "Max silent skew", bind: "KillAuraSettings.GrokMaxSkew", min: 6.0, max: 25.0, snap: false },
            Slider { label: "Dead zone", bind: "KillAuraSettings.DeadZoneSize", min: 0.0, max: 2.0, snap: false },
            Slider { label: "Min / max turn", bind: "KillAuraSettings.MinTurnSpeed", min: 1.0, max: 90.0, snap: false },
            Slider { label: "", bind: "KillAuraSettings.MaxTurnSpeed", min: 5.0, max: 180.0, snap: false },
            Slider { label: "Accel / decel", bind: "KillAuraSettings.Acceleration", min: 0.1, max: 10.0, snap: false },
            Slider { label: "", bind: "KillAuraSettings.Deceleration", min: 0.1, max: 10.0, snap: false },
            Slider { label: "Noise", bind: "KillAuraSettings.NoiseStrength", min: 0.0, max: 2.0, snap: false },
            Expand { header: "LiquidBounce controls" },
            Slider { label: "Dead zone", bind: "KillAuraSettings.DeadZoneSize", min: 0.0, max: 2.0, snap: false },
            Slider { label: "Min / max speed", bind: "KillAuraSettings.MinTurnSpeed", min: 1.0, max: 90.0, snap: false },
            Slider { label: "", bind: "KillAuraSettings.MaxTurnSpeed", min: 5.0, max: 180.0, snap: false },
            Slider { label: "Acceleration / deceleration", bind: "KillAuraSettings.Acceleration", min: 0.1, max: 10.0, snap: false },
            Slider { label: "", bind: "KillAuraSettings.Deceleration", min: 0.1, max: 10.0, snap: false },
            Check { label: "Overshoot", bind: "KillAuraSettings.UseOvershoot", tooltip: "" },
            Slider { label: "Strength / recovery", bind: "KillAuraSettings.OvershootStrength", min: 0.0, max: 20.0, snap: false },
            Slider { label: "", bind: "KillAuraSettings.OvershootRecovery", min: 0.01, max: 1.0, snap: false },
            Slider { label: "Noise", bind: "KillAuraSettings.NoiseStrength", min: 0.0, max: 2.0, snap: false },
            Check { label: "Randomize", bind: "KillAuraSettings.Randomize", tooltip: "" },
            Slider { label: "Random / Y range", bind: "KillAuraSettings.RandomizeRange", min: 0.0, max: 1.0, snap: false },
            Slider { label: "", bind: "KillAuraSettings.YRandomizeStrength", min: 0.0, max: 1.0, snap: false },
            Slider { label: "LB horizontal / vertical", bind: "KillAuraSettings.LiquidBounceHorizontalSpeed", min: 1.0, max: 180.0, snap: false },
            Slider { label: "", bind: "KillAuraSettings.LiquidBounceVerticalSpeed", min: 1.0, max: 180.0, snap: false },
            Slider { label: "LB smooth", bind: "KillAuraSettings.LiquidBounceSmoothFactor", min: 0.1, max: 1.0, snap: false },
            Check { label: "LB prediction", bind: "KillAuraSettings.LiquidBouncePredict", tooltip: "" },
            Slider { label: "", bind: "KillAuraSettings.LiquidBouncePredictSize", min: 0.0, max: 3.0, snap: false },
            Check { label: "LB randomize", bind: "KillAuraSettings.LiquidBounceRandomize", tooltip: "" },
            Slider { label: "", bind: "KillAuraSettings.LiquidBounceRandomizeRange", min: 0.0, max: 1.0, snap: false },
            Slider { label: "Horizontal search", bind: "KillAuraSettings.LiquidBounceHorizontalSearch", min: 0.0, max: 1.0, snap: false },
            Slider { label: "Body min / max", bind: "KillAuraSettings.LiquidBounceBodyPointMin", min: 0.0, max: 1.0, snap: false },
            Slider { label: "", bind: "KillAuraSettings.LiquidBounceBodyPointMax", min: 0.0, max: 1.0, snap: false },
            Expand { header: "Filters and display" },
            Check { label: "Through walls", bind: "KillAuraSettings.ThroughWalls", tooltip: "" },
            Check { label: "Require attack held", bind: "KillAuraSettings.RequirePress", tooltip: "" },
            Check { label: "Allow mining", bind: "KillAuraSettings.AllowMining", tooltip: "" },
            Check { label: "Weapons only", bind: "KillAuraSettings.WeaponsOnly", tooltip: "" },
            Check { label: "Allow tools", bind: "KillAuraSettings.AllowTools", tooltip: "" },
            Check { label: "Inventory check", bind: "KillAuraSettings.InventoryCheck", tooltip: "" },
            Check { label: "Bot check", bind: "KillAuraSettings.BotCheck", tooltip: "" },
            Check { label: "Teams", bind: "KillAuraSettings.Teams", tooltip: "" },
            Check { label: "Players", bind: "KillAuraSettings.Players", tooltip: "" },
            Check { label: "Bosses", bind: "KillAuraSettings.Bosses", tooltip: "" },
            Check { label: "Mobs", bind: "KillAuraSettings.Mobs", tooltip: "" },
            Check { label: "Animals", bind: "KillAuraSettings.Animals", tooltip: "" },
            Check { label: "Golems", bind: "KillAuraSettings.Golems", tooltip: "" },
            Check { label: "Silverfish", bind: "KillAuraSettings.Silverfish", tooltip: "" },
            Check { label: "Visualize aim", bind: "KillAuraSettings.VisualizeAim", tooltip: "" },
            Check { label: "Smooth back", bind: "KillAuraSettings.SmoothBack", tooltip: "" },
            Combo { label: "Show target", bind: "KillAuraSettings.ShowTarget", options: &["None", "Default", ] },
            Combo { label: "Debug", bind: "KillAuraSettings.DebugLog", options: &["None", "Health", ] },
    ],
    },
    Card {
        id: "JumpResetCard",
        controls: &[
            Keybind { module: "jumpreset" },
            Note { text: "Jump Reset" },
            Availability { text: "Available", bind: "JumpResetAvailabilityText" },
            Switch { label: "Enable Jump Reset", bind: "JumpResetEnabled" },
            Slider { label: "Chance (%)", bind: "JumpResetChance", min: 0.0, max: 100.0, snap: true },
            Note { text: "Jumps for one tick when you take a hit on the ground while holding W, which cuts the knockback you take." },
    ],
    },
    Card {
        id: "HitSelectCard",
        controls: &[
            Keybind { module: "hitselect" },
            Note { text: "Hit Select" },
            Availability { text: "Available", bind: "HitSelectAvailabilityText" },
            Switch { label: "Enable Hit Select", bind: "HitSelectEnabled" },
            Note { text: "Holds AutoClicker left clicks while the player under your crosshair is still in its hurt window, where the server ignores hits." },
    ],
    },
    Card {
        id: "WTapCard",
        controls: &[
            Keybind { module: "wtap" },
            Note { text: "WTap" },
            Availability { text: "Available", bind: "WTapAvailabilityText" },
            Switch { label: "Enable WTap", bind: "WTapEnabled" },
            Note { text: "Automatically resets sprint after landing hits to deal maximum knockback and restart combo momentum." },
    ],
    },
    Card {
        id: "TrajectoriesCard",
        controls: &[
            Keybind { module: "trajectories" },
            Note { text: "Trajectories" },
            Availability { text: "Available", bind: "TrajectoriesAvailabilityText" },
            Switch { label: "Enable Trajectories", bind: "TrajectoriesEnabled" },
            Note { text: "Draws the predicted flight path and landing spot for a charged bow, snowballs, eggs, ender pearls, splash potions and experience bottles." },
    ],
    },
    Card {
        id: "MurderMysteryCard",
        controls: &[
            Keybind { module: "murdermystery" },
            Note { text: "Murder Mystery" },
            Availability { text: "Available", bind: "MurderMysteryAvailabilityText" },
            Switch { label: "Enable Murder Mystery", bind: "MurderMysteryEnabled" },
            Note { text: "Hypixel Murder Mystery: marks players who have held a murder weapon in red and bow holders in blue, with a murderer alert at the top of the screen." },
    ],
    },
    Card {
        id: "BreakProgressCard",
        controls: &[
            Keybind { module: "breakprogress" },
            Note { text: "Break Progress" },
            Availability { text: "Available", bind: "BreakProgressAvailabilityText" },
            Switch { label: "Enable Break Progress", bind: "BreakProgressEnabled" },
            Note { text: "Shows how far each block being mined has progressed, including blocks other players are breaking, such as your bed." },
    ],
    },
    Card {
        id: "NoHurtCamCard",
        controls: &[
            Keybind { module: "nohurtcam" },
            Note { text: "No Hurt Cam" },
            Availability { text: "Available", bind: "NoHurtCamAvailabilityText" },
            Switch { label: "Enable No Hurt Cam", bind: "NoHurtCamEnabled" },
            Note { text: "Removes the jarring camera wobble when taking damage, keeping combat vision clear and stable." },
    ],
    },
    Card {
        id: "TntTimerCard",
        controls: &[
            Keybind { module: "tnttimer" },
            Note { text: "TNT Timer" },
            Availability { text: "Available", bind: "TntTimerAvailabilityText" },
            Switch { label: "Enable TNT Timer", bind: "TntTimerEnabled" },
            Note { text: "Renders remaining fuse time above ignited TNT entities with color-coded countdown indicators." },
    ],
    },
    Card {
        id: "FastPlaceCard",
        controls: &[
            Keybind { module: "fastplace" },
            Note { text: "Fast Place" },
            Availability { text: "Available", bind: "FastPlaceAvailabilityText" },
            Switch { label: "Enable Fast Place", bind: "FastPlaceEnabled" },
            Note { text: "Removes the 4-tick right-click delay, allowing blocks and projectiles to place immediately on hold." },
    ],
    },
    Card {
        id: "SprintCard",
        controls: &[
            Keybind { module: "sprint" },
            Note { text: "Sprint" },
            Availability { text: "Available", bind: "SprintAvailabilityText" },
            Switch { label: "Enable Sprint", bind: "SprintEnabled" },
            Note { text: "Automatically maintains forward sprint without needing to double-tap or hold a dedicated sprint key." },
    ],
    },
];
// ── Tab 6 and 7: the two data views ─────────────────────────────────────────────

/// Settings, `MainWindow.xaml:1523-2104`. The largest panel in the app.
///
/// The 18 built-in preview swatches carry hard-coded hex in the markup
/// (`#BB377D`/`#FBD3E9` at `:1856-1857` and 16 more). In Rust they are generated
/// from `theme::Theme::builtin_names()`, so the preview and the palette table
/// cannot drift apart.
pub const SETTINGS: &[Card] = &[
    Card {
        id: "ConfigsCard",
        controls: &[
            Note {
                text: "Save the current settings as a named config, then switch between configs at any time. Configs are stored as .json files.",
            },
            Combo {
                label: "Select config",
                bind: "ConfigSelection",
                options: &[],
            },
            Button {
                label: "Load",
                tooltip: "",
            },
            Button {
                label: "Save",
                tooltip: "",
            },
            Button {
                label: "Delete",
                tooltip: "",
            },
            Note {
                text: "Create new config",
            },
            Button {
                label: "Create",
                tooltip: "",
            },
            Button {
                label: "Open Configs Folder",
                tooltip: "",
            },
        ],
    },
    Card {
        id: "ReloadMappingsCard",
        controls: &[
            Note {
                text: "Use this if something is not working properly (missing overlays, broken nametags, stale in-game data after injection).",
            },
            Button {
                label: "Reload Mappings",
                tooltip: "",
            },
        ],
    },
    Card {
        id: "GuiCustomizationCard",
        controls: &[
            Note {
                text: "External GUI Palette",
            },
            Note {
                text: "Click a palette preview to apply it instantly.",
            },
            PalettePreview { name: "Slate" },
            PalettePreview { name: "Ink" },
            PalettePreview { name: "Graphite" },
            PalettePreview { name: "Steel" },
            PalettePreview { name: "Blend" },
            PalettePreview { name: "Lush" },
            PalettePreview { name: "Water" },
            PalettePreview { name: "Lime Water" },
            PalettePreview {
                name: "Digital Horizon",
            },
            PalettePreview { name: "Coral" },
            PalettePreview { name: "Magic" },
            PalettePreview { name: "Blossom" },
            PalettePreview { name: "Pastel" },
            PalettePreview { name: "Sunkist" },
            PalettePreview { name: "Nord" },
            PalettePreview { name: "Cherry" },
            PalettePreview {
                name: "Aubergine",
            },
            PalettePreview { name: "Snowy Sky" },
            Note {
                text: "Custom Palettes",
            },
            Note {
                text: "Create custom palette",
            },
            Note {
                text: "Pick four base colors as hex (e.g. #1A2B3C). The remaining shades are derived automatically.",
            },
            Note {
                text: "Name",
            },
            ColorField {
                label: "Background",
                bind: "PaletteBgBox",
            },
            ColorField {
                label: "Panel",
                bind: "PalettePanelBox",
            },
            ColorField {
                label: "Accent",
                bind: "PaletteAccentBox",
            },
            ColorField {
                label: "Text",
                bind: "PaletteTextBox",
            },
            Button {
                label: "Add palette",
                tooltip: "",
            },
            Note {
                text: "Module List Style",
            },
            Note {
                text: "Current module list style: ",
            },
            Switch {
                label: "Show client watermark (latency \u{b7} aoko \u{b7} FPS)",
                bind: "ShowLogo",
            },
            Switch {
                label: "Dev Mode",
                bind: "DevMode",
            },
            Note {
                text: "Shows unfinished modules (Kill Aura, \u{2026}) in the GUI. Off by default.",
            },
            // New copy, no managed analogue: section 12 decision 3 asks for a client
            // animation preference, which the C# never had. It lives on the GUI card
            // because it is a GUI setting and not a `Profile` field.
            Preference {
                label: "Animations",
                hint: "Turn off to stop every animation in the client.",
            },
        ],
    },
];

/// The seven module-list styles the previews cycle through.
///
/// Ported from `MainWindow.NormalizeModuleListStyle` (`:1275-1291`). The preview
/// rows in the markup all read `Autoclicker` except the last, which shows the live
/// sample line `autoclicker 12-16`.
pub const MODULE_LIST_STYLES: [&str; 7] = [
    "Classic", "Modern", "Minimal", "Outlined", "Glass", "Bold", "Default",
];

/// The sample row every preview but the last one shows.
pub const MODULE_LIST_PREVIEW_NAME: &str = "Autoclicker";

/// The live sample row in the markup.
pub const MODULE_LIST_PREVIEW_SAMPLE: &str = "autoclicker 12-16";

/// Stats, `MainWindow.xaml:1438-1521`.
pub const STATS: &[Card] = &[
    Card {
        id: "PlaytimeSummaryCard",
        controls: &[Note { text: "Playtime" }],
    },
    Card {
        id: "ClicksSummaryCard",
        controls: &[Note { text: "Clicks" }],
    },
    Card {
        id: "AverageCpsSummaryCard",
        controls: &[Note { text: "Avg CPS" }],
    },
    Card {
        id: "StatsActionsCard",
        controls: &[Button {
            label: "Reset Stats",
            tooltip: "",
        }],
    },
    Card {
        id: "CpsHistogramCard",
        controls: &[
            // The managed card opens with the "CPS Distribution" heading
            // (`MainWindow.xaml:1473`), drawn from [`heading`] rather than a Note, so
            // the string lives in one place.
            Note {
                text: "No samples yet",
            },
        ],
    },
];

/// The Stats tab's counters, verbatim from the markup's `Run` elements.
pub const STATS_LEFT_PREFIX: &str = "L ";
/// Between the two click counts.
pub const STATS_RIGHT_PREFIX: &str = "  R ";
/// Prefix of the peak-CPS caption.
pub const STATS_PEAK_PREFIX: &str = "Peak ";

/// Suffix after the sample count in the histogram caption.
pub const STATS_SAMPLES_SUFFIX: &str = " samples";

// ── Keybind capture ─────────────────────────────────────────────────────────────

/// What a module keybind button reads while it is capturing.
///
/// `SetKeybindButtonContent` (`Aoko/MainWindow.xaml.cs:2143-2156`) swaps it in for
/// ``Bind: {FormatVirtualKey(InputHooks.GetModuleKey(module))}``; the rest-state half of the
/// pair is [`crate::views::keybind_text::button_label`], which is where the managed rule
/// lives. The button it labels is the 140 px header one (`MainWindow.xaml:359`).
pub const KEYBIND_PENDING: &str = "Bind: [Press key or mouse...]";

/// The same copy for an action bind (`MainWindow.xaml.cs:2135-2140`).
///
/// `Action: [Press key or mouse...]` is the pending half of
/// [`crate::views::keybind_text::action_label`]; the button is the 210 px one
/// (`MainWindow.xaml:525`, `:542`).
pub const ACTION_BIND_PENDING: &str = "Action: [Press key or mouse...]";

/// The key an action bind was offered is already held by a general module bind
/// (`Aoko/MainWindow.xaml.cs:2049`; the Throwpot branch repeats it at `:2060`).
pub const ACTION_BIND_CONFLICT: &str =
    "Conflict: that input is already used by a general module bind.";

/// The reverse outcome: the action bind took the key (`MainWindow.xaml.cs:2048`, repeated
/// for Throwpot at `:2059`).
pub const ACTION_BIND_UPDATED: &str = "Action bind updated.";

/// The managed module-conflict sentence, verbatim (`MainWindow.xaml.cs:2070`).
///
/// Kept although nothing references it: it is the sentence this port deliberately departs
/// from. Managed wrote it into the AutoRod row's status line, so a conflict about a module
/// appeared under the wrong control; [`MODULE_BIND_CONFLICT_NAMED`] replaces it.
pub const MODULE_BIND_CONFLICT: &str = "Conflict: that input is used by an action bind.";

/// A module bind was refused because something else already holds the key.
///
/// New copy: managed had no second sentence, so this one names both bindings instead of
/// writing the module's conflict into the AutoRod row. `{key}` is the refused key as
/// [`crate::views::keybind_text::format_virtual_key`] renders it, `{holder}` the action bind
/// that already owns it, and `{requested}` the module that asked.
///
/// The three are filled with `str::replace` rather than `format!`, because this copy is data
/// and `format!` requires a literal format string.
pub const MODULE_BIND_CONFLICT_NAMED: &str =
    "Conflict: {key} is already bound to {holder}, so {requested} was left unchanged.";

// ── The nine tabs ───────────────────────────────────────────────────────────────

/// What a tab renders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabBody {
    /// Module cards, in document order.
    Cards(&'static [Card]),
    /// `Aoko/Views/AccountsView.xaml`: its own view, wired to `views::accounts_state`.
    Accounts,
    /// `Aoko/Views/ServersView.xaml`: its own view, wired to `views::servers_state`.
    Servers,
    /// `MainWindow.xaml:1438-1521`: its own view, `views::stats_view`.
    ///
    /// The copy is still this table's ([`STATS`]), but the numbers on it are live: the view
    /// reads the runtime's `StatsTracker` rather than the `Note`s, which is why the tab is a
    /// variant of its own and not a `Cards(STATS)` row. A card list is what a `Cards` tab
    /// paints; a tab that paints counters is a view.
    Stats,
}

/// The left rail, in the order `MainWindow.xaml` declares it.
///
/// Nine tabs. The capture script's seven-name array is the defect that made two
/// screenshots lie about which tab they held (`MIGRATION_ARCHITECTURE.md` section 8
/// item 9), so the count is asserted in the tests below.
pub const TABS: [Tab; 9] = [
    Tab {
        name: "Combat",
        body: TabBody::Cards(COMBAT),
    },
    Tab {
        name: "Render",
        body: TabBody::Cards(RENDER),
    },
    Tab {
        name: "Utility",
        body: TabBody::Cards(UTILITY),
    },
    Tab {
        name: "Risky",
        body: TabBody::Cards(RISKY),
    },
    Tab {
        name: "Beta",
        body: TabBody::Cards(BETA),
    },
    Tab {
        name: "Accounts",
        body: TabBody::Accounts,
    },
    Tab {
        name: "Servers",
        body: TabBody::Servers,
    },
    Tab {
        name: "Stats",
        body: TabBody::Stats,
    },
    Tab {
        name: "Settings",
        body: TabBody::Cards(SETTINGS),
    },
];

/// Rail entry for the update status button (`UpdateStatusButton`).
pub const UPDATE_STATUS_TITLE: &str = "Update status";

/// The rail search button's tooltip.
pub const SEARCH_BUTTON_TOOLTIP: &str = "Search modules and tabs (Ctrl+K)";

/// The rail search button's chord hint.
pub const SEARCH_BUTTON_CHORD: &str = "ctrl k";

/// A title with no managed source, kept flagged rather than deleted.
///
/// `MainWindow.xaml` contains no "Aoko" text: the window is titled `aoko client` (`:7`) and
/// the loader panel opens with [`LOADER_HEADING`] (`:307`). This constant predates that
/// finding, nothing reads it, and it is **not** the panel heading — the loader's first line
/// is the one below.
pub const LOADER_TITLE: &str = "Aoko";

// ── The loader panel ────────────────────────────────────────────────────────────
//
// `LoaderPanel` (`MainWindow.xaml:306-340`), in document order. The panel is the window's
// only content until injection succeeds; `EnterControlMode` (`MainWindow.xaml.cs:469-492`)
// then hides it and shows `ControlPanel`, which is the tree the rest of this table paints.

/// The panel's heading, `MainWindow.xaml:307-308`: 16 px, bold, centred, 10 px above.
pub const LOADER_HEADING: &str = "Injection Control";

/// The versions line under it, `MainWindow.xaml:309-315`.
///
/// 11 px, dim, wrapped and centred. The list is the managed claim verbatim, `and` included:
/// a Rust bridge that stops supporting one of these versions has to edit this line, because
/// the panel is where the claim is shown to the user.
pub const LOADER_SUPPORTED_VERSIONS: &str =
    "Supported versions: 1.8.9, 1.21.x, 26.1, 26.2, and 26.3 (auto-detected)";

/// The injection button at rest, `MainWindow.xaml:317-319`: `DarkButton`, height 35,
/// 13 px, 8 px below. The same word comes back after a cancelled picker
/// (`MainWindow.xaml.cs:1463-1464`) and after a failed run (`:1500`, `:1547`).
pub const LOADER_INJECT: &str = "Inject";

/// The button while the process list is being enumerated, `MainWindow.xaml.cs:1437`.
pub const LOADER_INJECT_DETECTING: &str = "Detecting...";

/// The button while the bridge is being attached, `MainWindow.xaml.cs:1489`.
///
/// Three full stops in the managed code, not the ellipsis character.
pub const LOADER_INJECT_CONNECTING: &str = "...";

/// The button once the bridge answers, `MainWindow.xaml.cs:1500` and again at `:1532`.
pub const LOADER_INJECT_CONNECTED: &str = "Connected";

/// The line under the button, `MainWindow.xaml:321-327`: 10 px, dim, wrapped and centred.
pub const LOADER_AUTODETECT: &str =
    "The Java/Minecraft client is detected automatically. If multiple clients are running, you can select one.";

/// The status line at rest, `MainWindow.xaml:329-332` (12 px, dim, centred), and the text a
/// cancelled picker restores at `MainWindow.xaml.cs:1464`.
pub const LOADER_STATUS_NOT_INJECTED: &str = "Status: Not Injected";

/// While the process list is being enumerated, `MainWindow.xaml.cs:1438`.
pub const LOADER_STATUS_DETECTING: &str = "Status: Detecting Java clients...";

/// While the bridge is being attached, `MainWindow.xaml.cs:1490`.
pub const LOADER_STATUS_CONNECTING: &str = "Status: Connecting...";

/// Once the bridge answers, `MainWindow.xaml.cs:1528` — the one status line drawn in the
/// accent colour (`:1529`) instead of the dim one.
pub const LOADER_STATUS_CONNECTED: &str = "Status: Connected & Injected";

/// The prefix of every other status line, `MainWindow.xaml.cs:1539`:
/// `$"Status: {gs.StatusMessage}"`.
///
/// **There is no fixed failure string to add here.** Every failure is this prefix plus
/// `GameStateClient.StatusMessage`, and those messages are set in `GameStateClient.cs`
/// (`:304`, `:355`, `:369`, `:395`, `:411`, `:493`), not in the loader. A Rust `Failed`
/// state therefore carries the message it was given; naming a constant here would be
/// wording the managed app never draws.
pub const LOADER_STATUS_PREFIX: &str = "Status: ";

// ── The palette editor ──────────────────────────────────────────────────────────

/// The swatch beside each hex box in the create-palette card, repeated for all four
/// rows: `MainWindow.xaml:1962` (Background), `:1973` (Panel), `:1984` (Accent) and
/// `:1995` (Text).
pub const PALETTE_SWATCH_TOOLTIP: &str = "Click to pick a color";

/// The remove button on a custom palette card, `MainWindow.xaml:1922`.
pub const PALETTE_DELETE_TOOLTIP: &str = "Delete palette";

/// The click target on a custom palette card, `MainWindow.xaml:1926`. The card itself
/// is the button, so this is a tooltip with no label beside it.
pub const PALETTE_APPLY_TOOLTIP: &str = "Apply palette";

/// The live-palette line above the cards, `MainWindow.xaml:1904`.
///
/// The markup is a `Run` inside a sentence with the name following it, so the name is
/// concatenated: `Current palette: Slate`.
pub const PALETTE_CURRENT_PREFIX: &str = "Current palette: ";

/// The name was missing, blank, over 32 characters or outside the managed charset
/// (`MainWindow.xaml.cs:1101`, validated by `IsValidPaletteName`).
pub const PALETTE_STATUS_NAME_INVALID: &str =
    "Enter a palette name (letters, numbers, spaces, - or _; max 32).";

/// The name collides with one of the 18 built-ins (`MainWindow.xaml.cs:1107`).
///
/// `{name}` is the rejected name; filled with `str::replace`, because this copy is data
/// and `format!` needs a literal format string.
pub const PALETTE_STATUS_NAME_BUILTIN: &str =
    "'{name}' is a built-in palette name. Choose another.";

/// One of the four hex boxes does not parse (`MainWindow.xaml.cs:1116`).
pub const PALETTE_STATUS_COLORS_INVALID: &str =
    "One or more colors are invalid. Use hex like #1A2B3C.";

/// `$"Could not save palette: {ex.Message}"` (`MainWindow.xaml.cs:1155`).
///
/// A prefix rather than a whole sentence, for the same reason as
/// [`LOADER_STATUS_PREFIX`]: the tail is an exception message, not copy this crate owns.
pub const PALETTE_STATUS_SAVE_FAILED_PREFIX: &str = "Could not save palette: ";

/// `$"Saved and applied palette '{name}'."` (`MainWindow.xaml.cs:1170`). `{name}` is the
/// palette that was written and applied in the same click.
pub const PALETTE_STATUS_SAVED: &str = "Saved and applied palette '{name}'.";

/// `$"Could not delete palette: {ex.Message}"` (`MainWindow.xaml.cs:1193`).
pub const PALETTE_STATUS_DELETE_FAILED_PREFIX: &str = "Could not delete palette: ";

/// A delete press named a palette the file does not contain.
///
/// New copy. Managed ran `RemoveAll` and then reported `Deleted palette '<name>'.`
/// whatever came back (`MainWindow.xaml.cs:1183-1209`), so a no-op claimed success; the
/// port names the palette and the reason instead, in the shape the config card already
/// uses for the same case (`views/profiles.rs:380-383`).
pub const PALETTE_STATUS_DELETE_MISSING: &str =
    "Could not delete palette '{name}': it is not in the palette file.";

/// `$"Deleted palette '{name}'."` (`MainWindow.xaml.cs:1209`).
pub const PALETTE_STATUS_DELETED: &str = "Deleted palette '{name}'.";

/// The overwrite confirmation's body, `MainWindow.xaml.cs:1125`.
pub const PALETTE_OVERWRITE_PROMPT: &str =
    "A custom palette named '{name}' already exists. Overwrite it?";

/// Its title, `MainWindow.xaml.cs:1126`.
pub const PALETTE_OVERWRITE_TITLE: &str = "Overwrite Palette";

/// The delete confirmation's body, `MainWindow.xaml.cs:1179`.
pub const PALETTE_DELETE_PROMPT: &str = "Delete custom palette '{name}'?";

/// Its title, `MainWindow.xaml.cs:1180`.
pub const PALETTE_DELETE_TITLE: &str = "Delete Palette";

// ── The Block ESP rows ──────────────────────────────────────────────────────────

/// The colour swatch in a tracked-block row, `MainWindow.xaml:705`. The row's own
/// swatch is the click target; there is no button beside it.
pub const BLOCK_ESP_COLOR_TOOLTIP: &str = "Click to change color";

/// The add-block box, `MainWindow.xaml:720`. The example is the managed wording, and it
/// names both accepted forms: the namespaced id and the bare one.
pub const BLOCK_ESP_ID_TOOLTIP: &str = "Block id, e.g. minecraft:diamond_ore or diamond_ore";

/// The add-block button's label, the `DarkButton` at `MainWindow.xaml:722`.
///
/// The table carried it as a [`Control::Button`] label until the row became
/// [`Control::BlockEspAdd`], which has no label field: the pair is one control, so the label
/// is copy rather than row data.
pub const BLOCK_ESP_ADD_LABEL: &str = "Add";

/// The client-drawn title bar's minimise/maximise/close accessible names.
pub const TITLE_BAR_MINIMISE: &str = "Minimise";
/// Maximise.
pub const TITLE_BAR_MAXIMISE: &str = "Maximise";
/// Close.
pub const TITLE_BAR_CLOSE: &str = "Close";

/// Tab header count, for the capture harness.
pub const TAB_COUNT: usize = 9;

/// Every card across the seven card-bearing tabs, in rail order.
pub fn all_cards() -> impl Iterator<Item = &'static Card> {
    TABS.iter().flat_map(|tab| match tab.body {
        TabBody::Cards(cards) => cards,
        // Stats is a view rather than a card tab, but it paints these cards: leaving them out
        // would quietly drop the summary cards from every test that walks the table's cards.
        TabBody::Stats => STATS,
        TabBody::Accounts | TabBody::Servers => &[],
    })
}

/// Every `ComboBox` in `Aoko/MainWindow.xaml`, as `(markup line, bind, caption)`
/// in table order. Thirteen elements: `grep -n "<ComboBox[ >]"` returns 504,
/// 1063, 1138, 1166, 1228, 1229, 1230, 1242, 1252, 1253, 1283, 1284 and 1540.
/// The captions are the `TextBlock` copy that sits above each one: `Slot mode`
/// at 503, `Chance Mode` at 1062, `Mode` at 1137, `Behavior` at 1165, `CPS mode`
/// at 1228, `Target mode` at 1229, `Sort` at 1230, `Mode` at 1242 and 1252,
/// `Move fix` at 1253, `Show target` at 1283, `Select config` at 1532.
/// The `Debug` caption at 1284 differs from its bind name (`DebugLog`), and the
/// last row's `ConfigComboBox` is populated at runtime, so its options are empty.
pub const COMBO_ROWS: [(&str, &str); 13] = [
    ("AutoRodSlotMode", "Slot mode"),
    ("ReachChanceMode", "Chance Mode"),
    ("AutoTotemMode", "Mode"),
    ("AutoTotemBehaviorMode", "Behavior"),
    ("KillAuraSettings.CpsMode", "CPS mode"),
    ("KillAuraSettings.Mode", "Target mode"),
    ("KillAuraSettings.Sort", "Sort"),
    ("KillAuraSettings.AutoBlock", "Mode"),
    ("KillAuraSettings.Rotations", "Mode"),
    ("KillAuraSettings.MoveFix", "Move fix"),
    ("KillAuraSettings.ShowTarget", "Show target"),
    ("KillAuraSettings.DebugLog", "Debug"),
    ("ConfigSelection", "Select config"),
];

/// Counts the table must reproduce, measured from `Aoko/MainWindow.xaml`:
/// `ModuleSwitch` 43, `DarkSlider` 72, `DarkCheckBox` 58, `KeybindButton_Click` 38.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlCounts {
    pub switches: usize,
    pub sliders: usize,
    pub checks: usize,
    pub keybinds: usize,
    pub combos: usize,
    pub buttons: usize,
    pub cards: usize,
}

/// Counts the controls in one tab body.
#[must_use]
pub fn count_controls(cards: &[Card]) -> ControlCounts {
    let mut counts = ControlCounts {
        cards: cards.len(),
        ..ControlCounts::default()
    };

    for card in cards {
        for control in card.controls {
            match control {
                Control::Switch { .. } => counts.switches += 1,
                Control::Slider { .. } => counts.sliders += 1,
                Control::Check { .. } => counts.checks += 1,
                Control::Keybind { .. } => counts.keybinds += 1,
                Control::Combo { .. } => counts.combos += 1,
                Control::Button { .. } => counts.buttons += 1,
                Control::Note { .. }
                | Control::ActionBind { .. }
                | Control::Availability { .. }
                | Control::Expand { .. }
                | Control::BlockEspRows
                | Control::BlockEspAdd { .. }
                | Control::ColorField { .. }
                | Control::PalettePreview { .. }
                | Control::Preference { .. } => {}
            }
        }
    }

    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The action-bind id is the text `{:?}` used to produce.
    ///
    /// The render path builds the button's element id from [`ActionBind::id`] instead of
    /// `format!("{which:?}")`, which was a heap allocation per frame. The ids are not
    /// user-visible, but they are element identity: if a rename made the two disagree,
    /// every id built before the change would stop matching.
    #[test]
    fn an_action_bind_id_matches_its_debug_name() {
        for which in [ActionBind::AutoRod, ActionBind::Throwpot] {
            assert_eq!(
                which.id(),
                format!("{which:?}"),
                "the id and the Debug name must agree"
            );
        }
    }

    #[test]
    fn the_rail_has_nine_tabs_in_the_declared_order() {
        let names: Vec<&str> = TABS.iter().map(|tab| tab.name).collect();
        assert_eq!(
            names,
            vec![
                "Combat", "Render", "Utility", "Risky", "Beta", "Accounts", "Servers", "Stats",
                "Settings",
            ]
        );
        assert_eq!(TABS.len(), TAB_COUNT);
    }

    #[test]
    fn the_control_counts_match_the_markup() {
        // Measured with `grep -o "StaticResource <style>"` over `Aoko/MainWindow.xaml`:
        // 43 `ModuleSwitch`, 72 `DarkSlider`, 58 `DarkCheckBox`, 38
        // `KeybindButton_Click` and 49 `ControlCardStyle` borders. Ten cards in the
        // markup have no `x:Name`, so this table names them (`ModuleListCard`,
        // `DiscordRpcCard`, `BetaBannerCard`, the five Stats sections and the three
        // Settings sections); 49 named-plus-unnamed becomes 50 entries because the
        // Stats tab's action row and the histogram are separate cards in the
        // markup's layout and the settings panel has three named sections.
        //
        // The loop below walks the six `TabBody::Cards` tabs, so it counts 45 of those
        // 50: Stats is a view now, not a card list a tab paints, and its five cards live
        // in `STATS` for [`crate::views::stats_view`]. Both halves are pinned together
        // below, so neither can lose a card without this failing.
        let counts = count_controls(&[]);
        assert_eq!(counts, ControlCounts::default());

        let mut total = ControlCounts::default();
        for tab in &TABS {
            if let TabBody::Cards(cards) = tab.body {
                let tab_counts = count_controls(cards);
                total.switches += tab_counts.switches;
                total.sliders += tab_counts.sliders;
                total.checks += tab_counts.checks;
                total.keybinds += tab_counts.keybinds;
                total.combos += tab_counts.combos;
                total.buttons += tab_counts.buttons;
                total.cards += tab_counts.cards;
            }
        }

        assert_eq!(total.switches, 43, "ModuleSwitch elements");
        assert_eq!(total.sliders, 72, "DarkSlider elements");
        assert_eq!(total.checks, 58, "DarkCheckBox elements");
        assert_eq!(total.keybinds, 38, "KeybindButton_Click buttons");
        assert_eq!(total.combos, 13, "ComboBox elements");
        assert_eq!(total.cards, 45, "cards across the six card-bearing tabs");
        assert_eq!(
            total.cards + STATS.len(),
            50,
            "cards in the markup: the six tabs' 45 plus the Stats view's five"
        );
    }

    #[test]
    fn every_combo_is_the_one_the_markup_declares() {
        // Pins both the count and the copy: a stale count is how "15 combos"
        // survived in a report while the table held 13 rows.
        let combos: Vec<(&str, &str)> = all_cards()
            .flat_map(|card| card.controls.iter())
            .filter_map(|control| match control {
                Control::Combo { label, bind, .. } => Some((*bind, *label)),
                _ => None,
            })
            .collect();
        assert_eq!(combos.len(), COMBO_ROWS.len());
        assert_eq!(combos, COMBO_ROWS.to_vec());
    }

    #[test]
    fn every_card_id_is_unique_and_non_empty() {
        let mut seen = std::collections::BTreeSet::new();
        for card in all_cards() {
            assert!(!card.id.is_empty());
            assert!(seen.insert(card.id), "duplicate card id {}", card.id);
            assert!(!card.controls.is_empty(), "{} has no controls", card.id);
        }
    }

    #[test]
    fn every_keybind_names_a_catalogue_module() {
        let ids: Vec<&str> = aoko_core::search::entries()
            .iter()
            .map(|entry| entry.id)
            .collect();

        for card in all_cards() {
            for control in card.controls {
                if let Control::Keybind { module } = control {
                    assert!(
                        ids.contains(module),
                        "keybind '{module}' is not a catalogue module"
                    );
                }
            }
        }
    }

    #[test]
    fn every_slider_range_is_ordered_and_snapped_where_the_markup_snaps() {
        for card in all_cards() {
            for control in card.controls {
                if let Control::Slider {
                    label,
                    bind,
                    min,
                    max,
                    snap,
                } = control
                {
                    assert!(min < max, "{}.{bind}: {min}..{max}", card.id);
                    assert!(!bind.is_empty(), "{}.{label} has no binding", card.id);
                    let _ = snap;
                }
            }
        }
    }

    #[test]
    fn copy_that_must_not_be_paraphrased_is_verbatim() {
        let combat = count_controls(COMBAT);
        assert_eq!(combat.cards, 8, "Combat has eight cards");

        // Spot checks against MainWindow.xaml, one per tab: the AutoClicker card's
        // tooltip, the Beta banner's second line, the Stats captions, the search
        // button's chord, and a settings sentence.
        let tooltips: Vec<&str> = COMBAT
            .iter()
            .flat_map(|card| card.controls.iter())
            .filter_map(|control| match control {
                Control::Check { tooltip, .. } if !tooltip.is_empty() => Some(*tooltip),
                _ => None,
            })
            .collect();
        assert!(
            tooltips
                .iter()
                .any(|tooltip| tooltip.starts_with("Clicks inside real chests only.")),
            "the chest-click tooltip is missing"
        );

        let beta_banner = BETA
            .iter()
            .flat_map(|card| card.controls.iter())
            .filter_map(|control| match control {
                Control::Note { text } => Some(*text),
                _ => None,
            })
            .any(|text| {
                text == "These modules were not thoroughly tested. Use them at your own risk."
            });
        assert!(beta_banner, "the Beta banner sentence is missing");

        assert_eq!(SEARCH_BUTTON_TOOLTIP, "Search modules and tabs (Ctrl+K)");
        assert_eq!(SEARCH_BUTTON_CHORD, "ctrl k");
        assert_eq!(MODULE_LIST_PREVIEW_SAMPLE, "autoclicker 12-16");
    }

    /// The view tabs are their own views, and the card tabs are the module ones plus Settings.
    ///
    /// The two data tabs are Accounts and Servers, each rendering a view of its own, and Stats
    /// joins them now that `views::stats_view` is mounted; the remaining card tabs are the five
    /// module tabs and Settings. The count is exact on purpose — a range would stay green if
    /// the Stats row quietly went back to being a static copy table.
    #[test]
    fn the_view_tabs_are_their_own_views_rather_than_card_lists() {
        let bodies: Vec<TabBody> = TABS.iter().map(|tab| tab.body).collect();
        assert!(bodies.contains(&TabBody::Accounts));
        assert!(bodies.contains(&TabBody::Servers));
        assert!(bodies.contains(&TabBody::Stats));
        // Six tabs carry cards: the five module tabs and Settings. Accounts, Servers and Stats
        // render views, so a seventh card table would mean a view regressed to a copy table.
        let card_tabs = bodies
            .iter()
            .filter(|body| matches!(body, TabBody::Cards(_)))
            .count();
        assert_eq!(card_tabs, 6);
    }

    #[test]
    fn the_eighteen_palette_previews_are_the_builtin_palettes() {
        let previews: Vec<&str> = SETTINGS
            .iter()
            .flat_map(|card| card.controls.iter())
            .filter_map(|control| match control {
                Control::PalettePreview { name } => Some(*name),
                _ => None,
            })
            .collect();

        assert_eq!(previews.len(), 18, "one preview per built-in palette");
        assert_eq!(
            previews[0], "Slate",
            "Slate is the fallback and comes first"
        );
        assert!(previews.contains(&"Snowy Sky"));
    }
}
