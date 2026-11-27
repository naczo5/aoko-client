//! One tick of the click loop, as a pure decision.
//!
//! The C# `Clicker.ClickLoop` (`Aoko/Core/Clicker.cs:3117-3300`) mixes four things:
//! guards that say "do not click, poll again later", the CPS model, the synthetic
//! click, and the deadline scheduler. Only the first is a pure function of state,
//! so it is extracted here and the loop that owns the thread is left as glue:
//! every branch below returns the exact `Task.Delay` the managed loop used, which
//! is what keeps the poll cadences (100 / 25 / 10 / 45 ms) identical.
//!
//! Order matters and is preserved: menu safety first, then "only hold block", then
//! break-blocks, then hit-select. Reordering any of them changes which guard wins
//! when two apply, which is observable in game.

use std::time::Duration;

use aoko_protocol::state::GameState;

use super::guards::{
    is_always_blocked_gui_screen, is_chest_click_allowed, is_chest_gui_screen,
    should_hit_select_skip,
};

/// Poll intervals the managed loop used, by guard (`Clicker.cs:3136-3226`).
pub const MENU_BLOCK_POLL: Duration = Duration::from_millis(100);
pub const NOT_FOREGROUND_POLL: Duration = Duration::from_millis(100);
pub const RIGHT_CLICK_BLOCK_POLL: Duration = Duration::from_millis(100);
pub const BREAK_BLOCKS_POLL: Duration = Duration::from_millis(25);
pub const HIT_SELECT_POLL: Duration = Duration::from_millis(10);

/// Clicker settings the guard chain reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClickConfig {
    /// `ClickInChests`.
    pub click_in_chests: bool,
    /// `ChestStealerMenuCheck`: any of the Chest Stealer menu checks enabled.
    pub chest_stealer_menu_check: bool,
    /// `BreakBlocksEnabled`.
    pub break_blocks_enabled: bool,
    /// `HitSelectEnabled`.
    pub hit_select_enabled: bool,
    /// `RightClickOnlyBlock`.
    pub right_click_only_block: bool,
    /// Whether the bridge is injected at all; "fail open" only applies when it is
    /// not (`Clicker.cs:3184-3190`).
    pub injected: bool,
}

/// What the loop should do with this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickDecision {
    /// Click now, then schedule the next deadline.
    Click,
    /// Do not click; poll again after this delay.
    Wait(Duration),
}

/// External conditions the loop cannot read from [`GameState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickContext {
    /// `WindowDetection.IsMinecraftActive()`.
    pub minecraft_active: bool,
    /// `WindowDetection.IsCursorVisible()`: the OS cursor, not the game's.
    pub cursor_visible: bool,
}

/// `Clicker.ClickLoop` guard chain for one frame.
///
/// `left_button` selects the left or right click engine, as the two managed loops
/// do.
#[must_use]
pub fn decide(
    state: &GameState,
    config: &ClickConfig,
    context: TickContext,
    left_button: bool,
) -> TickDecision {
    if !context.minecraft_active {
        return TickDecision::Wait(NOT_FOREGROUND_POLL);
    }

    // Menu and inventory safety (`Clicker.cs:3130-3178`).
    if state.is_connected {
        if state.gui_open {
            let screen = state.screen_name.as_str();
            let always_block = is_always_blocked_gui_screen(screen);
            let is_chest = is_chest_gui_screen(screen);

            let mut should_block = false;
            // A GUI flag with a hidden cursor is a ghosted GUI: the player is
            // really in-game, so nothing is blocked.
            if context.cursor_visible {
                if always_block {
                    should_block = true;
                } else if is_chest {
                    if !config.click_in_chests
                        || !is_chest_click_allowed(state, config.chest_stealer_menu_check)
                    {
                        should_block = true;
                    }
                } else {
                    // Escape, settings, chat, and every other menu.
                    should_block = true;
                }
            }

            if should_block {
                return TickDecision::Wait(MENU_BLOCK_POLL);
            }
        }
    } else if context.cursor_visible {
        // Not connected: the cursor is the only signal available.
        return TickDecision::Wait(NOT_FOREGROUND_POLL);
    }

    // Right click: "Only hold block" fails open only when not injected at all
    // (`Clicker.cs:3183-3191`).
    if !left_button
        && config.right_click_only_block
        && config.injected
        && (!state.is_connected || !state.holding_block)
    {
        return TickDecision::Wait(RIGHT_CLICK_BLOCK_POLL);
    }

    // Break blocks: never mine through a chest-GUI click that the menu checks allow,
    // and never left-click while the crosshair is on a block.
    if left_button && config.break_blocks_enabled && state.is_connected {
        let allow_chest_clicks = state.gui_open
            && context.cursor_visible
            && config.click_in_chests
            && is_chest_gui_screen(&state.screen_name)
            && is_chest_click_allowed(state, config.chest_stealer_menu_check);

        if !allow_chest_clicks
            && (state.looking_at_block || state.breaking_block)
            && !state.looking_at_entity
            && !state.looking_at_entity_latched
        {
            return TickDecision::Wait(BREAK_BLOCKS_POLL);
        }
    }

    // Hit select: skip clicks the server would ignore (`Clicker.cs:3218-3227`).
    if left_button
        && config.hit_select_enabled
        && state.is_connected
        && should_hit_select_skip(state.looking_at_entity, state.looking_at_entity_hurt_time)
    {
        return TickDecision::Wait(HIT_SELECT_POLL);
    }

    TickDecision::Click
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_protocol::state::ChestStealerState;

    fn context() -> TickContext {
        TickContext {
            minecraft_active: true,
            cursor_visible: false,
        }
    }

    fn connected_state() -> GameState {
        GameState {
            is_connected: true,
            ..GameState::default()
        }
    }

    fn physical_chest(state: &mut GameState) {
        state.chest_stealer_state = Some(ChestStealerState {
            ready: true,
            physical: true,
            ..ChestStealerState::default()
        });
    }

    #[test]
    fn an_inactive_window_never_clicks() {
        let state = connected_state();
        let ctx = TickContext {
            minecraft_active: false,
            cursor_visible: false,
        };
        assert_eq!(
            decide(&state, &ClickConfig::default(), ctx, true),
            TickDecision::Wait(NOT_FOREGROUND_POLL)
        );
    }

    #[test]
    fn a_visible_cursor_with_no_bridge_connection_blocks() {
        let state = GameState {
            is_connected: false,
            ..GameState::default()
        };
        let ctx = TickContext {
            minecraft_active: true,
            cursor_visible: true,
        };
        assert_eq!(
            decide(&state, &ClickConfig::default(), ctx, true),
            TickDecision::Wait(NOT_FOREGROUND_POLL)
        );
    }

    #[test]
    fn a_ghosted_gui_does_not_block_when_the_cursor_is_hidden() {
        let mut state = connected_state();
        state.gui_open = true;
        state.screen_name = "GuiChest".to_owned();
        assert_eq!(
            decide(&state, &ClickConfig::default(), context(), true),
            TickDecision::Click
        );
    }

    #[test]
    fn menus_block_when_the_cursor_is_visible() {
        let mut state = connected_state();
        state.gui_open = true;
        state.screen_name = "ChatScreen".to_owned();
        let ctx = TickContext {
            minecraft_active: true,
            cursor_visible: true,
        };
        assert_eq!(
            decide(&state, &ClickConfig::default(), ctx, true),
            TickDecision::Wait(MENU_BLOCK_POLL)
        );
    }

    #[test]
    fn the_inventory_is_blocked_even_with_click_in_chests() {
        let mut state = connected_state();
        physical_chest(&mut state);
        state.gui_open = true;
        state.screen_name = "GuiInventory".to_owned();
        let mut config = ClickConfig {
            click_in_chests: true,
            ..ClickConfig::default()
        };
        config.injected = true;
        let ctx = TickContext {
            minecraft_active: true,
            cursor_visible: true,
        };
        assert_eq!(
            decide(&state, &config, ctx, true),
            TickDecision::Wait(MENU_BLOCK_POLL)
        );
    }

    #[test]
    fn a_physical_chest_is_clickable_when_click_in_chests_is_on() {
        let mut state = connected_state();
        physical_chest(&mut state);
        state.gui_open = true;
        state.screen_name = "GuiChest".to_owned();
        let config = ClickConfig {
            click_in_chests: true,
            chest_stealer_menu_check: true,
            ..ClickConfig::default()
        };
        let ctx = TickContext {
            minecraft_active: true,
            cursor_visible: true,
        };
        assert_eq!(decide(&state, &config, ctx, true), TickDecision::Click);
    }

    #[test]
    fn a_virtual_chest_is_blocked_when_the_menu_check_is_on() {
        let mut state = connected_state();
        state.chest_stealer_state = Some(ChestStealerState {
            ready: true,
            physical: false,
            ..ChestStealerState::default()
        });
        state.gui_open = true;
        state.screen_name = "GuiChest".to_owned();
        let config = ClickConfig {
            click_in_chests: true,
            chest_stealer_menu_check: true,
            ..ClickConfig::default()
        };
        let ctx = TickContext {
            minecraft_active: true,
            cursor_visible: true,
        };
        assert_eq!(
            decide(&state, &config, ctx, true),
            TickDecision::Wait(MENU_BLOCK_POLL)
        );
    }

    #[test]
    fn a_chest_click_outranks_break_blocks() {
        let mut state = connected_state();
        physical_chest(&mut state);
        state.gui_open = true;
        state.screen_name = "GuiChest".to_owned();
        state.looking_at_block = true;
        let config = ClickConfig {
            click_in_chests: true,
            break_blocks_enabled: true,
            ..ClickConfig::default()
        };
        let ctx = TickContext {
            minecraft_active: true,
            cursor_visible: true,
        };
        assert_eq!(decide(&state, &config, ctx, true), TickDecision::Click);
    }

    #[test]
    fn break_blocks_holds_the_left_button_off_a_block() {
        let mut state = connected_state();
        state.looking_at_block = true;
        let config = ClickConfig {
            break_blocks_enabled: true,
            ..ClickConfig::default()
        };
        assert_eq!(
            decide(&state, &config, context(), true),
            TickDecision::Wait(BREAK_BLOCKS_POLL)
        );

        // A latched crosshair also suppresses, matching the C# condition.
        let mut latched = connected_state();
        latched.breaking_block = true;
        latched.looking_at_entity_latched = true;
        assert_eq!(
            decide(&latched, &config, context(), true),
            TickDecision::Click
        );

        // The right button has no break-blocks guard.
        let mut right = connected_state();
        right.looking_at_block = true;
        assert_eq!(
            decide(&right, &config, context(), false),
            TickDecision::Click
        );
    }

    #[test]
    fn only_hold_block_fails_open_when_not_injected() {
        let mut state = connected_state();
        state.holding_block = false;

        let injected = ClickConfig {
            right_click_only_block: true,
            injected: true,
            ..ClickConfig::default()
        };
        assert_eq!(
            decide(&state, &injected, context(), false),
            TickDecision::Wait(RIGHT_CLICK_BLOCK_POLL)
        );

        let not_injected = ClickConfig {
            right_click_only_block: true,
            injected: false,
            ..ClickConfig::default()
        };
        assert_eq!(
            decide(&state, &not_injected, context(), false),
            TickDecision::Click
        );

        state.holding_block = true;
        assert_eq!(
            decide(&state, &injected, context(), false),
            TickDecision::Click
        );
    }

    #[test]
    fn hit_select_only_skips_inside_the_hurt_window() {
        let mut state = connected_state();
        state.looking_at_entity = true;
        state.looking_at_entity_hurt_time = 5;
        let config = ClickConfig {
            hit_select_enabled: true,
            ..ClickConfig::default()
        };

        assert_eq!(
            decide(&state, &config, context(), true),
            TickDecision::Wait(HIT_SELECT_POLL)
        );

        state.looking_at_entity_hurt_time = 1;
        assert_eq!(
            decide(&state, &config, context(), true),
            TickDecision::Click
        );

        // Right clicks are unaffected by hit select.
        state.looking_at_entity_hurt_time = 5;
        assert_eq!(
            decide(&state, &config, context(), false),
            TickDecision::Click
        );
    }

    #[test]
    fn the_menu_guard_outranks_hit_select() {
        let mut state = connected_state();
        state.gui_open = true;
        state.screen_name = "GuiInventory".to_owned();
        state.looking_at_entity = true;
        state.looking_at_entity_hurt_time = 9;
        let config = ClickConfig {
            hit_select_enabled: true,
            ..ClickConfig::default()
        };
        let ctx = TickContext {
            minecraft_active: true,
            cursor_visible: true,
        };
        assert_eq!(
            decide(&state, &config, ctx, true),
            TickDecision::Wait(MENU_BLOCK_POLL)
        );
    }
}
