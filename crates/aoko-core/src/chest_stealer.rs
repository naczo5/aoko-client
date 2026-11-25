//! Chest Stealer: coordinate mapping, readiness gate, slot choice and cadence.
//!
//! Port of `Aoko/Core/ChestStealerController.cs` and
//! `Aoko/Core/ChestStealerCoordinateMapper.cs`, minus the two `user32` calls the
//! controller performed itself (`SetCursorPos` and the synthetic Shift press).
//! Those are I/O and belong to `aoko-platform::InputBackend`; everything that
//! *decides* what to do is here, so the interesting behaviour is testable
//! headlessly. The loop that owns the thread is the same shape as the C# one:
//!
//! ```text
//! gate -> nearest live slot -> click -> block that slot for 85 ms
//!      -> recompute distance -> sleep (interval - click elapsed)
//! ```
//!
//! `NextIntervalMs` is a distance-adaptive cadence: a slot near the cursor is
//! clicked faster than one across the window, and the whole thing stays inside
//! `35..=750 ms` so neither the client nor the server sees a burst.

use std::collections::BTreeMap;

use aoko_protocol::state::{ChestStealerSlot, ChestStealerState, GameState};

use crate::clock::Rng;

/// A chest whose state is older than this is ignored (`ChestStealerController.cs:76`).
pub const FRESH_STATE_MS: u64 = 300;
/// A clicked slot is skipped for this long, so one slot cannot be double-clicked
/// (`ChestStealerController.cs:77`).
pub const SLOT_RETRY_COOLDOWN_MS: u64 = 85;
/// Poll interval while the container is not usable (`:174`).
pub const IDLE_POLL_MS: u64 = 45;
/// Poll interval while every slot is on cooldown (`:200`).
pub const NO_SLOT_POLL_MS: u64 = 20;
/// Back-off after an unexpected error (`:254`).
pub const ERROR_BACKOFF_MS: u64 = 150;
/// Configured delay clamp (`:249`).
pub const BASE_DELAY_RANGE_MS: (i32, i32) = (50, 500);
/// Final interval clamp (`:259`).
pub const INTERVAL_RANGE_MS: (i32, i32) = (35, 750);

/// A window client rect in screen coordinates, the four edges the C# `RECT`
/// carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    /// Left edge.
    pub left: i32,
    /// Top edge.
    pub top: i32,
    /// Right edge (exclusive by subtraction, inclusive as a coordinate).
    pub right: i32,
    /// Bottom edge.
    pub bottom: i32,
}

impl ScreenRect {
    /// Width, floored at zero.
    #[must_use]
    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    /// Height, floored at zero.
    #[must_use]
    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    /// Centre of the rect, used when the cursor position is unavailable
    /// (`ChestStealerController.cs:369-378`).
    #[must_use]
    pub fn centre(self) -> (i32, i32) {
        ((self.left + self.right) / 2, (self.top + self.bottom) / 2)
    }
}

/// `ChestStealerCoordinateMapper.TryMapScaledPoint` (`:7-27`).
///
/// A slot coordinate is expressed in the game's own screen space; the click has to
/// land in OS screen space, so it is scaled by the client rect. Out-of-range input
/// is rejected rather than clamped, because a clamped click would hit a different
/// slot.
#[must_use]
pub fn map_scaled_point(
    screen_width: i32,
    screen_height: i32,
    slot_x: i32,
    slot_y: i32,
    rect: ScreenRect,
) -> Option<(i32, i32)> {
    if screen_width <= 0 || screen_height <= 0 || rect.width() <= 0 || rect.height() <= 0 {
        return None;
    }
    if slot_x < 0 || slot_x >= screen_width || slot_y < 0 || slot_y >= screen_height {
        return None;
    }

    let scale_x = f64::from(rect.width()) / f64::from(screen_width);
    let scale_y = f64::from(rect.height()) / f64::from(screen_height);

    // `(int)Math.Round(...)`: midpoint-to-even, like the rest of the movement code.
    let x = f64::from(rect.left) + (f64::from(slot_x) * scale_x).round_ties_even();
    let y = f64::from(rect.top) + (f64::from(slot_y) * scale_y).round_ties_even();
    Some((x as i32, y as i32))
}

/// Why the stealer is not clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotReady {
    /// The module is off.
    Disabled,
    /// The bridge is not connected.
    Disconnected,
    /// The game window is not foreground.
    Background,
    /// No GUI is open and the cursor is hidden.
    NoGui,
    /// The bridge has never sent state.
    NeverUpdated,
    /// The snapshot is older than [`FRESH_STATE_MS`].
    Stale,
    /// The bridge has no chest state for the open container.
    NoContainer,
    /// The container is not ready to click yet.
    NotReady,
    /// A menu check is on and the container is not a physical chest.
    VirtualContainer,
    /// The container reports no slots.
    NoSlots,
}

/// The C# gate (`ChestStealerController.cs:157-176`), as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChestGate {
    /// `ChestStealerEnabled`.
    pub enabled: bool,
    /// Transport connected.
    pub connected: bool,
    /// Game window foreground.
    pub foreground: bool,
    /// `GuiOpen || cursor visible`.
    pub gui_active: bool,
    /// Age of the snapshot, `None` when it was never updated.
    pub state_age_ms: Option<u64>,
    /// `ChestStealerMenuCheck`: require a physical chest.
    pub menu_check: bool,
}

impl ChestGate {
    /// Evaluates the gate against a snapshot.
    pub fn check<'a>(&self, state: &'a GameState) -> Result<&'a ChestStealerState, NotReady> {
        if !self.enabled {
            return Err(NotReady::Disabled);
        }
        if !self.connected {
            return Err(NotReady::Disconnected);
        }
        if !self.foreground {
            return Err(NotReady::Background);
        }
        if !self.gui_active {
            return Err(NotReady::NoGui);
        }
        let Some(age) = self.state_age_ms else {
            return Err(NotReady::NeverUpdated);
        };
        if age > FRESH_STATE_MS {
            return Err(NotReady::Stale);
        }

        let Some(chest) = state.chest_stealer_state.as_ref() else {
            return Err(NotReady::NoContainer);
        };
        if !chest.ready {
            return Err(NotReady::NotReady);
        }
        if self.menu_check && !chest.physical {
            return Err(NotReady::VirtualContainer);
        }
        if chest.slots.is_empty() {
            return Err(NotReady::NoSlots);
        }

        Ok(chest)
    }
}

/// Which slot the next click targets.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotChoice {
    /// Index into the slot list.
    pub index: usize,
    /// Distance from the cursor in client pixels.
    pub distance: f64,
}

/// Picks the live slot nearest the cursor.
///
/// `blocked` carries the per-slot retry cooldowns. Slots the bridge no longer
/// reports are pruned by [`prune_retry_blocks`] before this runs, so a stale entry
/// cannot hide a slot that came back.
#[must_use]
pub fn nearest_live_slot(
    screen_width: i32,
    screen_height: i32,
    slots: &[ChestStealerSlot],
    blocked: &RetryBlocks,
    cursor: (i32, i32),
    rect: ScreenRect,
    now_ms: u64,
) -> Option<SlotChoice> {
    let mut best: Option<SlotChoice> = None;

    for (index, slot) in slots.iter().enumerate() {
        if blocked.is_blocked(slot.slot_number, now_ms) {
            continue;
        }
        let Some((x, y)) = map_scaled_point(screen_width, screen_height, slot.x, slot.y, rect)
        else {
            continue;
        };

        let dx = f64::from(x - cursor.0);
        let dy = f64::from(y - cursor.1);
        let distance = (dx * dx + dy * dy).sqrt();

        if best
            .as_ref()
            .is_none_or(|current| distance < current.distance)
        {
            best = Some(SlotChoice { index, distance });
        }
    }

    best
}

/// Per-slot retry cooldowns, keyed by the bridge's slot number.
#[derive(Debug, Default)]
pub struct RetryBlocks {
    blocked_until: BTreeMap<i32, u64>,
}

impl RetryBlocks {
    /// Empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Blocks a slot for the chest-stealer cooldown, [`SLOT_RETRY_COOLDOWN_MS`].
    pub fn block(&mut self, slot_number: i32, now_ms: u64) {
        self.block_for(slot_number, now_ms, SLOT_RETRY_COOLDOWN_MS);
    }

    /// Blocks a slot for an explicit cooldown. Refill uses 150 ms rather than the
    /// chest stealer's 85 ms (`RefillController.cs:83`).
    pub fn block_for(&mut self, slot_number: i32, now_ms: u64, cooldown_ms: u64) {
        self.blocked_until
            .insert(slot_number, now_ms.saturating_add(cooldown_ms));
    }

    /// Whether a slot is still cooling down.
    #[must_use]
    pub fn is_blocked(&self, slot_number: i32, now_ms: u64) -> bool {
        self.blocked_until
            .get(&slot_number)
            .is_some_and(|until| *until > now_ms)
    }

    /// Number of tracked slots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blocked_until.len()
    }

    /// Whether nothing is tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocked_until.is_empty()
    }

    /// Drops tracking for slots the container no longer reports
    /// (`ChestStealerController.cs:344-367`).
    pub fn prune(&mut self, live: &[ChestStealerSlot]) {
        if self.blocked_until.is_empty() {
            return;
        }
        self.blocked_until
            .retain(|slot_number, _| live.iter().any(|slot| slot.slot_number == *slot_number));
    }

    /// Forgets everything, e.g. when the container window changes (`:188`).
    pub fn clear(&mut self) {
        self.blocked_until.clear();
    }
}

/// `ChestStealerController.NextIntervalMs` (`:381-397`).
///
/// Distance-adaptive multiplier times the configured delay, plus up to ±1/6 of the
/// base as jitter, clamped to [`INTERVAL_RANGE_MS`].
pub fn next_interval_ms(configured_delay_ms: i32, distance_px: f64, rng: &mut dyn Rng) -> i32 {
    let base = configured_delay_ms.clamp(BASE_DELAY_RANGE_MS.0, BASE_DELAY_RANGE_MS.1);
    let base_f = f64::from(base);

    let multiplier = if distance_px <= 0.0 {
        1.0
    } else if distance_px <= 24.0 {
        0.78 + rng.next_f64() * 0.14
    } else if distance_px <= 70.0 {
        0.90 + rng.next_f64() * 0.16
    } else if distance_px <= 140.0 {
        1.00 + rng.next_f64() * 0.18
    } else if distance_px <= 240.0 {
        1.10 + rng.next_f64() * 0.22
    } else {
        1.22 + rng.next_f64() * 0.26
    };

    // `Math.Max(8, baseDelay / 6)`, then a symmetric jitter draw.
    let jitter = (base / 6).max(8);
    let jitter_draw = rng.next_range(-f64::from(jitter), f64::from(jitter) + 1.0);
    let interval = (base_f * multiplier).round_ties_even() + jitter_draw.round_ties_even();

    #[allow(clippy::cast_possible_truncation)]
    let interval = interval as i32;
    interval.clamp(INTERVAL_RANGE_MS.0, INTERVAL_RANGE_MS.1)
}

/// Stores the freshness bookkeeping the loop needs between iterations.
#[derive(Debug, Default)]
pub struct WindowTracker {
    active_window_id: i32,
    click_count: u32,
}

impl WindowTracker {
    /// A tracker that has not seen a window yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Notices a container change, resetting the retry set like the C# loop does.
    /// Returns whether the window changed.
    pub fn observe(&mut self, window_id: i32, blocked: &mut RetryBlocks) -> bool {
        if window_id == self.active_window_id {
            return false;
        }
        self.active_window_id = window_id;
        self.click_count = 0;
        blocked.clear();
        true
    }

    /// Forgets the active window, used whenever the gate closes (`:177-184`).
    pub fn reset(&mut self, blocked: &mut RetryBlocks) {
        self.active_window_id = -1;
        self.click_count = 0;
        blocked.clear();
    }

    /// Counts one accepted click.
    pub fn note_click(&mut self) {
        self.click_count = self.click_count.saturating_add(1);
    }

    /// Clicks accepted in the current window.
    #[must_use]
    pub fn click_count(&self) -> u32 {
        self.click_count
    }

    /// Active container window id, `-1` when none.
    #[must_use]
    pub fn active_window_id(&self) -> i32 {
        self.active_window_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SplitMix64;

    fn rect() -> ScreenRect {
        ScreenRect {
            left: 100,
            top: 50,
            right: 1700,
            bottom: 950,
        }
    }

    fn slot(index: i32, slot_number: i32, x: i32, y: i32) -> ChestStealerSlot {
        ChestStealerSlot {
            index,
            slot_number,
            x,
            y,
        }
    }

    fn chest(ready: bool, physical: bool, slots: Vec<ChestStealerSlot>) -> ChestStealerState {
        ChestStealerState {
            ready,
            physical,
            window_id: 7,
            screen_width: 1600,
            screen_height: 900,
            slots,
        }
    }

    #[test]
    fn coordinate_mapping_scales_into_screen_space() {
        // 1600x900 game space into a 1600x900 client rect: identity, offset by the rect.
        let point = map_scaled_point(1600, 900, 800, 450, rect());
        assert_eq!(point, Some((900, 500)));

        // Half-size client rect: coordinates halve.
        let small = ScreenRect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 450,
        };
        assert_eq!(
            map_scaled_point(1600, 900, 800, 450, small),
            Some((400, 225))
        );
    }

    #[test]
    fn coordinate_mapping_rejects_out_of_range_input() {
        assert_eq!(map_scaled_point(0, 900, 1, 1, rect()), None);
        assert_eq!(map_scaled_point(1600, 0, 1, 1, rect()), None);
        assert_eq!(
            map_scaled_point(
                1600,
                900,
                1,
                1,
                ScreenRect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0
                }
            ),
            None
        );
        // Exactly at the edge is out of range, matching `slot.X >= ScreenWidth`.
        assert_eq!(map_scaled_point(1600, 900, 1600, 0, rect()), None);
        assert_eq!(map_scaled_point(1600, 900, 0, 900, rect()), None);
        assert_eq!(map_scaled_point(1600, 900, -1, 0, rect()), None);
    }

    #[test]
    fn the_gate_requires_every_condition_in_order() {
        let slots = vec![slot(0, 0, 100, 100), slot(1, 1, 200, 200)];
        let mut state = GameState {
            chest_stealer_state: Some(chest(true, true, slots)),
            ..GameState::default()
        };

        let ready = ChestGate {
            enabled: true,
            connected: true,
            foreground: true,
            gui_active: true,
            state_age_ms: Some(10),
            menu_check: true,
        };
        assert!(ready.check(&state).is_ok());

        assert_eq!(
            ChestGate {
                enabled: false,
                ..ready
            }
            .check(&state),
            Err(NotReady::Disabled)
        );
        assert_eq!(
            ChestGate {
                connected: false,
                ..ready
            }
            .check(&state),
            Err(NotReady::Disconnected)
        );
        assert_eq!(
            ChestGate {
                foreground: false,
                ..ready
            }
            .check(&state),
            Err(NotReady::Background)
        );
        assert_eq!(
            ChestGate {
                gui_active: false,
                ..ready
            }
            .check(&state),
            Err(NotReady::NoGui)
        );
        assert_eq!(
            ChestGate {
                state_age_ms: None,
                ..ready
            }
            .check(&state),
            Err(NotReady::NeverUpdated)
        );
        assert_eq!(
            ChestGate {
                state_age_ms: Some(FRESH_STATE_MS + 1),
                ..ready
            }
            .check(&state),
            Err(NotReady::Stale)
        );
        // Exactly at the freshness limit is still usable.
        assert!(ChestGate {
            state_age_ms: Some(FRESH_STATE_MS),
            ..ready
        }
        .check(&state)
        .is_ok());

        // No container at all.
        let empty = GameState::default();
        assert_eq!(ready.check(&empty), Err(NotReady::NoContainer));

        // Virtual container with the menu check on.
        state.chest_stealer_state = Some(chest(true, false, vec![slot(0, 0, 10, 10)]));
        assert_eq!(ready.check(&state), Err(NotReady::VirtualContainer));
        assert!(ChestGate {
            menu_check: false,
            ..ready
        }
        .check(&state)
        .is_ok());

        // Not ready / no slots.
        state.chest_stealer_state = Some(chest(false, true, vec![slot(0, 0, 10, 10)]));
        assert_eq!(ready.check(&state), Err(NotReady::NotReady));
        state.chest_stealer_state = Some(chest(true, true, Vec::new()));
        assert_eq!(ready.check(&state), Err(NotReady::NoSlots));
    }

    #[test]
    fn the_nearest_live_slot_wins() {
        let slots = vec![
            slot(0, 10, 100, 100),
            slot(1, 11, 800, 450),
            slot(2, 12, 1500, 850),
        ];
        let blocked = RetryBlocks::new();
        // Cursor near the middle slot.
        let choice = nearest_live_slot(1600, 900, &slots, &blocked, (900, 500), rect(), 0)
            .expect("a slot is reachable");
        assert_eq!(choice.index, 1);
        assert!(choice.distance < 10.0);
    }

    #[test]
    fn retry_blocks_hide_a_slot_until_the_cooldown_expires() {
        let slots = vec![slot(0, 10, 800, 450), slot(1, 11, 1500, 850)];
        let mut blocked = RetryBlocks::new();
        blocked.block(10, 1_000);

        assert!(blocked.is_blocked(10, 1_000));
        assert!(blocked.is_blocked(10, 1_084));
        assert!(!blocked.is_blocked(10, 1_000 + SLOT_RETRY_COOLDOWN_MS));

        // The blocked slot is skipped, so the far one is chosen.
        let choice = nearest_live_slot(1600, 900, &slots, &blocked, (900, 500), rect(), 1_000)
            .expect("the second slot is still live");
        assert_eq!(choice.index, 1);

        // Once everything is blocked there is nothing to click.
        blocked.block(11, 1_000);
        assert!(
            nearest_live_slot(1600, 900, &slots, &blocked, (900, 500), rect(), 1_000).is_none()
        );
    }

    #[test]
    fn pruning_forgets_slots_the_container_stopped_reporting() {
        let mut blocked = RetryBlocks::new();
        blocked.block(10, 0);
        blocked.block(11, 0);
        blocked.block(12, 0);
        assert_eq!(blocked.len(), 3);

        blocked.prune(&[slot(0, 10, 0, 0), slot(1, 12, 0, 0)]);
        assert_eq!(blocked.len(), 2);
        assert!(!blocked.is_blocked(11, 0));
        assert!(blocked.is_blocked(10, 0));
    }

    #[test]
    fn the_window_tracker_resets_on_a_container_change() {
        let mut tracker = WindowTracker::new();
        let mut blocked = RetryBlocks::new();
        blocked.block(10, 0);

        assert!(tracker.observe(7, &mut blocked));
        assert_eq!(tracker.active_window_id(), 7);
        assert!(blocked.is_empty(), "a new window clears retry blocks");

        tracker.note_click();
        tracker.note_click();
        assert_eq!(tracker.click_count(), 2);

        // Same window: nothing resets.
        assert!(!tracker.observe(7, &mut blocked));
        assert_eq!(tracker.click_count(), 2);

        // Different window: counters and blocks reset.
        blocked.block(11, 0);
        assert!(tracker.observe(8, &mut blocked));
        assert_eq!(tracker.click_count(), 0);
        assert!(blocked.is_empty());

        tracker.note_click();
        tracker.reset(&mut blocked);
        assert_eq!(tracker.active_window_id(), -1);
        assert_eq!(tracker.click_count(), 0);
    }

    #[test]
    fn the_interval_is_distance_adaptive_and_clamped() {
        let mut rng = SplitMix64::new(7);

        // Long distance: the multiplier is above 1.
        for _ in 0..200 {
            let interval = next_interval_ms(120, 400.0, &mut rng);
            assert!(
                (INTERVAL_RANGE_MS.0..=INTERVAL_RANGE_MS.1).contains(&interval),
                "{interval} out of range"
            );
            assert!(
                interval > 100,
                "far slots should not be clicked at full speed: {interval}"
            );
        }

        // Near distance: the multiplier is below 1.
        let near: Vec<i32> = (0..50)
            .map(|_| next_interval_ms(120, 10.0, &mut rng))
            .collect();
        assert!(near
            .iter()
            .all(|interval| (INTERVAL_RANGE_MS.0..INTERVAL_RANGE_MS.1).contains(interval)));
        assert!(
            near.iter().sum::<i32>() / near.len() as i32 <= 120,
            "near slots should be clicked no slower than the configured delay"
        );

        // Zero distance means the close-slot branch is skipped (multiplier 1.0).
        for _ in 0..50 {
            let interval = next_interval_ms(120, 0.0, &mut rng);
            assert!((INTERVAL_RANGE_MS.0..=INTERVAL_RANGE_MS.1).contains(&interval));
        }

        // Clamps hold for absurd configuration.
        assert!(next_interval_ms(0, 0.0, &mut rng) >= INTERVAL_RANGE_MS.0);
        assert!(next_interval_ms(i32::MAX, 1000.0, &mut rng) <= INTERVAL_RANGE_MS.1);
    }

    #[test]
    fn a_rect_with_no_area_has_no_centre_offset() {
        let degenerate = ScreenRect {
            left: 10,
            top: 20,
            right: 10,
            bottom: 20,
        };
        assert_eq!(degenerate.width(), 0);
        assert_eq!(degenerate.height(), 0);
        assert_eq!(degenerate.centre(), (10, 20));
    }
}
