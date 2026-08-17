#pragma once

namespace autotool {

static const int kHotbarSlots = 9;
static const int kInvalidSlot = -1;

struct AutoToolConfig {
    bool enabled               = false;
    bool swapWeapon            = true;
    bool instantSwap           = true;
    int  swapToDelayMs         = 50;
    bool swapBack              = false;
    int  swapBackDelayMs       = 350;
    bool requireMouseDown      = true;
    bool onlyWhileSneaking     = false;
    bool bedwarsMode           = true;
};

struct AutoToolState {
    int                originalSlot       = -1;
    bool               swapped            = false;
    bool               swapPending        = false;
    unsigned long long swapTimerMs        = 0;
    int                pendingTargetSlot  = -1;
    bool               swapBackPending    = false;
    unsigned long long swapBackTimerMs    = 0;

    void Reset() {
        originalSlot      = -1;
        swapped           = false;
        swapPending       = false;
        swapTimerMs       = 0;
        pendingTargetSlot = -1;
        swapBackPending   = false;
        swapBackTimerMs   = 0;
    }
};

struct AutoToolInput {
    unsigned long long nowMs          = 0;
    bool               inWorld        = false;
    bool               guiOpen        = false;
    bool               mouseDown      = false;
    bool               isSneaking     = false;
    int                currentSlot    = 0;
    bool               isBlockHit     = false;
    bool               isChestHit     = false;
    bool               isEntityHit    = false;
    int                bestToolSlot   = -1;
    int                bestWeaponSlot = -1;
    // Auto Rod (and similar exclusive hotbar owners) must keep the selected
    // slot for the whole cast. Freeze here so a held LMB cannot snap back.
    bool               pauseExclusive = false;
};

struct AutoToolAction {
    bool switchSlot;
    int  targetSlot;

    AutoToolAction() : switchSlot(false), targetSlot(-1) {}
    AutoToolAction(bool s, int t) : switchSlot(s), targetSlot(t) {}
};

inline AutoToolAction UpdateAutoToolState(
    AutoToolState& state,
    const AutoToolConfig& cfg,
    const AutoToolInput& input)
{
    if (!cfg.enabled || !input.inWorld || input.guiOpen) {
        if (!input.inWorld || input.guiOpen) {
            state.swapPending = false;
            state.swapBackPending = false;
        }
        return { false, -1 };
    }

    if (input.pauseExclusive) {
        return { false, -1 };
    }

    if (input.currentSlot < 0 || input.currentSlot >= kHotbarSlots) {
        return { false, -1 };
    }

    int desiredSlot = -1;
    // Prefer a weapon while an entity is under the crosshair so PvP is not
    // overridden by a block behind / under the opponent.
    if (input.isEntityHit && cfg.swapWeapon && input.bestWeaponSlot >= 0 && input.bestWeaponSlot < kHotbarSlots) {
        desiredSlot = input.bestWeaponSlot;
    } else if (input.isBlockHit && input.bestToolSlot >= 0 && input.bestToolSlot < kHotbarSlots) {
        // Bedwars mode: in Bedwars, players punch (left click) chests to quickly deposit
        // resources (iron, gold, diamonds, emeralds). Prevent changing to a tool when
        // punching chests so resource deposits are not interrupted by tool swaps.
        if (!(cfg.bedwarsMode && input.isChestHit)) {
            desiredSlot = input.bestToolSlot;
        }
    }

    // requireMouseDown applies to mining only. Weapon swaps happen on hover so a
    // short PvP click is not missed by the poll loop.
    if (cfg.requireMouseDown && !input.mouseDown && !input.isEntityHit) {
        desiredSlot = -1;
    }
    if (cfg.onlyWhileSneaking && !input.isSneaking) {
        desiredSlot = -1;
    }

    // Trigger swap-back when no longer mining / attacking
    if (state.swapped && cfg.swapBack && !state.swapBackPending && desiredSlot == -1 && state.originalSlot != -1) {
        state.swapBackPending = true;
        state.swapBackTimerMs = input.nowMs;
    }

    // Process pending swap-back
    if (state.swapBackPending) {
        if (desiredSlot != -1) {
            // Player resumed breaking/attacking; abort swap back
            state.swapBackPending = false;
        } else if (state.swapped && (input.nowMs >= state.swapBackTimerMs) &&
                   (input.nowMs - state.swapBackTimerMs >= static_cast<unsigned long long>(cfg.swapBackDelayMs))) {
            int slotToRestore = state.originalSlot;
            state.swapped = false;
            state.swapBackPending = false;
            state.originalSlot = -1;
            state.swapPending = false;
            state.pendingTargetSlot = -1;
            if (slotToRestore >= 0 && slotToRestore < kHotbarSlots && slotToRestore != input.currentSlot) {
                return { true, slotToRestore };
            }
            return { false, -1 };
        } else {
            return { false, -1 };
        }
    }

    // Process swap to target tool/weapon
    if (desiredSlot != -1 && desiredSlot != input.currentSlot) {
        if (!state.swapPending || state.pendingTargetSlot != desiredSlot) {
            state.swapPending = true;
            state.swapTimerMs = input.nowMs;
            state.pendingTargetSlot = desiredSlot;
        }
    } else {
        state.swapPending = false;
        state.pendingTargetSlot = -1;
    }

    if (state.swapPending) {
        bool shouldSwap = (input.nowMs >= state.swapTimerMs) &&
                          (input.nowMs - state.swapTimerMs >= static_cast<unsigned long long>(cfg.swapToDelayMs));
        if (input.isEntityHit && cfg.swapWeapon && cfg.instantSwap) {
            shouldSwap = true;
        }
        // Already mining: retarget as soon as the looked-at block needs a different tool.
        // Waiting on swapToDelay here is what made held-LMB feel like it needed a re-click.
        if (input.isBlockHit && input.mouseDown && (state.swapped || state.originalSlot != -1)) {
            shouldSwap = true;
        }
        if (shouldSwap) {
            if (state.originalSlot == -1) {
                state.originalSlot = input.currentSlot;
            }
            int target = state.pendingTargetSlot;
            state.swapPending = false;
            state.pendingTargetSlot = -1;
            if (target >= 0 && target < kHotbarSlots && target != input.currentSlot) {
                state.swapped = true;
                return { true, target };
            }
        }
    }

    return { false, -1 };
}

} // namespace autotool
