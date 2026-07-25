#pragma once

namespace autorod {

static const int kHotbarSlots = 9;
static const int kAutoSlotMode = 0;
static const int kInvalidSlot = -1;
static const int kSelectToUseTicks = 2;
static const int kUseToRestoreTicks = 4;
static const int kMinExtensionTicks = 1;
static const int kMaxExtensionTicks = 40;
static const int kRestoreSettleTicks = 1;

inline bool HasElapsedTicks(int currentTick, int phaseStartTick, int requiredTicks)
{
    if (requiredTicks < 0) return false;
    const long long delta = static_cast<long long>(currentTick) - phaseStartTick;
    return delta >= requiredTicks && delta < 1000000;
}

inline bool ShouldRestoreAfterUse(int currentTick, int phaseStartTick,
                                  int extensionTicks, bool holdToExtend,
                                  bool releaseRequested)
{
    const int requiredTicks = holdToExtend ? kMinExtensionTicks : extensionTicks;
    return (!holdToExtend || releaseRequested) &&
        HasElapsedTicks(currentTick, phaseStartTick, requiredTicks);
}

// slotMode: 0 = lowest hotbar slot containing a rod, 1..9 = forced hotbar slot.
// Forced slots may bypass rod verification when verifyForcedSlot is false.
inline int SelectTargetSlot(int originalSlot, int slotMode, bool verifyForcedSlot,
                            const bool rodSlots[kHotbarSlots])
{
    if (originalSlot < 0 || originalSlot >= kHotbarSlots) return kInvalidSlot;
    if (slotMode < 0 || slotMode > kHotbarSlots) return kInvalidSlot;

    if (slotMode == kAutoSlotMode) {
        if (!rodSlots) return kInvalidSlot;
        for (int slot = 0; slot < kHotbarSlots; ++slot) {
            if (rodSlots[slot]) return slot;
        }
        return kInvalidSlot;
    }

    const int forcedSlot = slotMode - 1;
    if (!verifyForcedSlot) return forcedSlot;
    return rodSlots && rodSlots[forcedSlot] ? forcedSlot : kInvalidSlot;
}

} // namespace autorod
