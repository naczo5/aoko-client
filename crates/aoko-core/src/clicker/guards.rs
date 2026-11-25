//! Pure click-path predicates lifted out of `Aoko/Core/Clicker.cs` and
//! `Aoko/Core/InputHooks.cs`.
//!
//! Everything here is a function of `GameState` plus a handful of booleans, which
//! is exactly what makes the click loop testable without a window, a bridge or a
//! synthetic-input backend. The click loop itself is in [`super::tick`]; these are
//! its guards.
//!
//! Screen matching is case-insensitive substring matching over the *mapped* class
//! names, because a bridge reports both Yarn (`ContainerScreen`, `class_481`) and
//! Mojmap (`GuiContainer`) spellings depending on the game version. The lists are
//! copied verbatim; shortening them would silently change which GUIs block clicks.

use aoko_protocol::state::GameState;

/// `HitSelectHurtTimeThreshold` (`Clicker.cs:3286`): one tick of slack over the
/// client's invulnerability window, covering the server round trip.
pub const HIT_SELECT_HURT_TIME_THRESHOLD: i32 = 1;

/// Default cooldown used when the bridge reports nothing usable, in seconds per
/// tick (`Clicker.cs:3056`).
pub const DEFAULT_ATTACK_COOLDOWN_PER_TICK: f32 = 0.08;
/// Lower clamp for a normalised attack cooldown (`Clicker.cs:3071`).
pub const MIN_ATTACK_COOLDOWN_PER_TICK: f32 = 0.025;
/// Upper clamp for a normalised attack cooldown (`Clicker.cs:3071`).
pub const MAX_ATTACK_COOLDOWN_PER_TICK: f32 = 0.25;

/// Containers that are always blocked, even with "Click in Chests" on: the
/// player's own inventory, crafting, furnaces and anvils (`Clicker.cs:3074-3087`).
const ALWAYS_BLOCKED_SCREENS: [&str; 13] = [
    "GuiInventory",
    "InventoryScreen",
    "class_490",
    "GuiCrafting",
    "CraftingScreen",
    "class_479",
    "GuiFurnace",
    "FurnaceScreen",
    "class_3871",
    "AbstractFurnace",
    "GuiRepair",
    "AnvilScreen",
    "class_471",
];

/// Chest-like containers: blocked unless "Click in Chests" allows them
/// (`Clicker.cs:3103-3115`).
const CHEST_SCREENS: [&str; 10] = [
    "GuiChest",
    "ContainerScreen",
    "class_481",
    "GuiContainer",
    "HopperScreen",
    "class_488",
    "ShulkerBox",
    "class_495",
    "HandledScreen",
    "class_465",
];

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    // ASCII-lowercase both sides without allocating: the needles and the mapped
    // screen names are ASCII in every bridge.
    let needle_len = needle.len();
    if haystack.len() < needle_len {
        return false;
    }
    haystack.as_bytes().windows(needle_len).any(|window| {
        window
            .iter()
            .zip(needle.as_bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

/// `Clicker.IsAlwaysBlockedGuiScreen` (`Clicker.cs:3074`).
#[must_use]
pub fn is_always_blocked_gui_screen(screen: &str) -> bool {
    ALWAYS_BLOCKED_SCREENS
        .iter()
        .any(|needle| contains_ignore_case(screen, needle))
}

/// `Clicker.IsChestGuiScreen` (`Clicker.cs:3103`).
#[must_use]
pub fn is_chest_gui_screen(screen: &str) -> bool {
    CHEST_SCREENS
        .iter()
        .any(|needle| contains_ignore_case(screen, needle))
}

/// `Clicker.IsChestClickAllowed` (`Clicker.cs:3095`).
///
/// With every Chest Stealer menu check off, any chest-like GUI is allowed. With
/// any check on, the bridge has to report the open container as a physical chest,
/// so shops and kit selectors that reuse the chest GUI are rejected.
#[must_use]
pub fn is_chest_click_allowed(state: &GameState, menu_check_enabled: bool) -> bool {
    if !menu_check_enabled {
        return true;
    }
    state
        .chest_stealer_state
        .as_ref()
        .is_some_and(|chest| chest.physical)
}

/// `Clicker.ShouldHitSelectSkip` (`Clicker.cs:3298`).
#[must_use]
pub fn should_hit_select_skip(looking_at_entity: bool, target_hurt_time: i32) -> bool {
    looking_at_entity && target_hurt_time > HIT_SELECT_HURT_TIME_THRESHOLD
}

/// `Clicker.NormalizeCooldownPerTick` (`Clicker.cs:3055-3072`).
///
/// The bridge may report seconds-per-attack, attacks-per-second or a raw
/// `attackCooldownPerTick`; all three collapse onto seconds per tick and clamp to
/// the legal vanilla window.
#[must_use]
pub fn normalize_cooldown_per_tick(raw: f32) -> f32 {
    if !raw.is_finite() || raw <= 0.0 {
        return DEFAULT_ATTACK_COOLDOWN_PER_TICK;
    }

    let normalized = if raw > 1.0 {
        let as_ticks = 1.0 / raw;
        let as_attack_speed = raw / 20.0;
        as_ticks.min(as_attack_speed)
    } else {
        raw
    };

    normalized.clamp(MIN_ATTACK_COOLDOWN_PER_TICK, MAX_ATTACK_COOLDOWN_PER_TICK)
}

/// `InputHooks.IsAnyGameScreenOpen` (`InputHooks.cs:166-175`).
///
/// A screen name the bridge could not resolve is `""`, `"none"` or `"unknown"`,
/// and none of those count as an open screen.
#[must_use]
pub fn is_any_game_screen_open(state: &GameState) -> bool {
    if state.gui_open {
        return true;
    }

    let screen = state.screen_name.trim();
    !screen.is_empty()
        && !screen.eq_ignore_ascii_case("none")
        && !screen.eq_ignore_ascii_case("unknown")
}

/// `InputHooks.ShouldConsumeAutoRodAction` (`InputHooks.cs:161-164`).
///
/// Every gate is required: the module is enabled, the bridge advertises it, the
/// transport is connected, the game window is foreground, the player is in a
/// world, and no GUI is open. AutoRod presses a real use-item through the OS input
/// path, so a single missing gate would act inside a menu.
#[must_use]
pub fn should_consume_auto_rod_action(
    enabled: bool,
    supported: bool,
    connected: bool,
    minecraft_foreground: bool,
    in_world: bool,
    any_screen_open: bool,
) -> bool {
    enabled && supported && connected && minecraft_foreground && in_world && !any_screen_open
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_protocol::state::{ChestStealerState, GameState};

    fn state_with_chest(ready: bool, physical: bool) -> GameState {
        GameState {
            chest_stealer_state: Some(ChestStealerState {
                ready,
                physical,
                ..ChestStealerState::default()
            }),
            ..GameState::default()
        }
    }

    /// `ChestClickCheckTests.IsChestClickAllowed_ChecksOff_AllowsAnyContainer`.
    #[test]
    fn chest_click_is_allowed_everywhere_when_the_menu_check_is_off() {
        assert!(is_chest_click_allowed(&GameState::default(), false));
    }

    /// `ChestClickCheckTests.IsChestClickAllowed_ChecksOn_RejectsContainerWithoutBridgeState`.
    #[test]
    fn chest_click_is_rejected_without_bridge_state() {
        assert!(!is_chest_click_allowed(&GameState::default(), true));
    }

    /// `ChestClickCheckTests.IsChestClickAllowed_ChecksOn_RejectsMenuContainer`.
    #[test]
    fn chest_click_is_rejected_for_a_virtual_menu_container() {
        let state = state_with_chest(false, false);
        assert!(!is_chest_click_allowed(&state, true));
    }

    /// `ChestClickCheckTests.IsChestClickAllowed_ChecksOn_AllowsPhysicalChest`.
    #[test]
    fn chest_click_is_allowed_for_a_physical_chest() {
        let state = state_with_chest(true, true);
        assert!(is_chest_click_allowed(&state, true));
    }

    #[test]
    fn chest_screen_detection_covers_both_mappings() {
        for screen in [
            "GuiChest",
            "net.minecraft.client.gui.screens.inventory.ContainerScreen",
            "class_481",
            "GuiContainer",
            "HopperScreen",
            "class_488",
            "ShulkerBoxScreen",
            "class_495",
            "HandledScreen",
            "class_465",
            "guichest", // case-insensitive
        ] {
            assert!(
                is_chest_gui_screen(screen),
                "{screen} should be a chest GUI"
            );
        }

        for screen in ["", "ChatScreen", "none", "GuiInventory"] {
            assert!(
                !is_chest_gui_screen(screen),
                "{screen} should not be a chest GUI"
            );
        }
    }

    #[test]
    fn always_blocked_screens_win_over_chest_matching() {
        for screen in [
            "GuiInventory",
            "InventoryScreen",
            "class_490",
            "GuiCrafting",
            "CraftingScreen",
            "class_479",
            "GuiFurnace",
            "FurnaceScreen",
            "class_3871",
            "AbstractFurnace",
            "GuiRepair",
            "AnvilScreen",
            "class_471",
        ] {
            assert!(is_always_blocked_gui_screen(screen), "{screen}");
        }
        assert!(!is_always_blocked_gui_screen("GuiChest"));
        assert!(!is_always_blocked_gui_screen(""));
    }

    /// `Clicker.ShouldHitSelectSkip`: one tick of slack above the hurt window.
    #[test]
    fn hit_select_skips_only_inside_the_hurt_window() {
        assert!(should_hit_select_skip(true, 2));
        assert!(should_hit_select_skip(true, 10));
        assert!(!should_hit_select_skip(true, 1));
        assert!(!should_hit_select_skip(true, 0));
        assert!(!should_hit_select_skip(false, 10));
    }

    #[test]
    fn cooldown_normalisation_matches_the_managed_bridge_units() {
        // Missing / garbage readings fall back to the default.
        assert_eq!(
            normalize_cooldown_per_tick(0.0),
            DEFAULT_ATTACK_COOLDOWN_PER_TICK
        );
        assert_eq!(
            normalize_cooldown_per_tick(-1.0),
            DEFAULT_ATTACK_COOLDOWN_PER_TICK
        );
        assert_eq!(
            normalize_cooldown_per_tick(f32::NAN),
            DEFAULT_ATTACK_COOLDOWN_PER_TICK
        );
        assert_eq!(
            normalize_cooldown_per_tick(f32::INFINITY),
            DEFAULT_ATTACK_COOLDOWN_PER_TICK
        );

        // A seconds-per-tick value passes through and clamps.
        assert!((normalize_cooldown_per_tick(0.1) - 0.1).abs() < f32::EPSILON);
        assert_eq!(
            normalize_cooldown_per_tick(0.001),
            MIN_ATTACK_COOLDOWN_PER_TICK
        );
        assert_eq!(
            normalize_cooldown_per_tick(0.9),
            MAX_ATTACK_COOLDOWN_PER_TICK
        );

        // Above 1.0 it is read as attacks-per-second or as the inverse: the smaller
        // of the two readings wins, then clamps.
        // 20 is attacks-per-second, so a tick is 1/20 = 0.05 s; that is above the
        // 0.025 minimum (`Clicker.cs:3059-3069`).
        assert!((normalize_cooldown_per_tick(20.0) - 0.05).abs() < f32::EPSILON);
        assert!((normalize_cooldown_per_tick(4.0) - 0.2).abs() < f32::EPSILON);
    }

    /// `AutoRodManagedTests.AnyGameScreenDetection_BlocksChatEvenWhenGuiFlagIsFalse`.
    #[test]
    fn any_game_screen_detection_blocks_chat_even_when_the_gui_flag_is_false() {
        let cases = [
            (false, "", false),
            (false, "none", false),
            (false, "unknown", false),
            (false, "ChatScreen", true),
            (
                false,
                "net.minecraft.client.gui.screens.ChatScreen|Screen",
                true,
            ),
            (true, "", true),
        ];

        for (gui_open, screen_name, expected) in cases {
            let state = GameState {
                gui_open,
                screen_name: screen_name.to_owned(),
                ..GameState::default()
            };
            assert_eq!(
                is_any_game_screen_open(&state),
                expected,
                "gui_open={gui_open} screen={screen_name:?}"
            );
        }
    }

    /// `AutoRodManagedTests.ActionConsumption_RequiresEveryRuntimeGate`.
    #[test]
    fn auto_rod_action_requires_every_runtime_gate() {
        let cases = [
            (true, true, true, true, true, false, true),
            (false, true, true, true, true, false, false),
            (true, false, true, true, true, false, false),
            (true, true, false, true, true, false, false),
            (true, true, true, false, true, false, false),
            (true, true, true, true, false, false, false),
            (true, true, true, true, true, true, false),
        ];

        for (enabled, supported, connected, foreground, in_world, any_screen, expected) in cases {
            assert_eq!(
                should_consume_auto_rod_action(
                    enabled, supported, connected, foreground, in_world, any_screen
                ),
                expected,
                "{enabled}/{supported}/{connected}/{foreground}/{in_world}/{any_screen}"
            );
        }
    }
}
