#pragma once

#include "auto_rod_core.h"

// Throwpot: keybind-triggered one-shot action that swaps to a healing splash
// potion in the hotbar, throws it via the physical use input, and restores
// the original slot. Slot selection + tick pacing reuse the AutoRod helpers;
// this namespace only adds the potion-specific selection rule.
namespace throwpot {

// Fixed restore delay after the throw (ticks), mirroring AutoRod's default.
static const int kUseToRestoreTicks = autorod::kUseToRestoreTicks;

inline bool HasElapsedTicks(int currentTick, int phaseStartTick, int requiredTicks)
{
    return autorod::HasElapsedTicks(currentTick, phaseStartTick, requiredTicks);
}

// Picks the lowest hotbar slot holding a healing splash potion.
inline int SelectPotSlot(const bool potSlots[autorod::kHotbarSlots])
{
    if (!potSlots) return autorod::kInvalidSlot;
    for (int slot = 0; slot < autorod::kHotbarSlots; ++slot) {
        if (potSlots[slot]) return slot;
    }
    return autorod::kInvalidSlot;
}

// AutoHeal only throws while the player is already looking steeply down so the
// splash lands at their own feet instead of flying toward an opponent.
// Minecraft pitch is positive when looking down.
static const float kMinDownwardPitch = 75.0f;

inline bool IsLookingSteeplyDown(float pitch)
{
    return pitch >= kMinDownwardPitch;
}

} // namespace throwpot
