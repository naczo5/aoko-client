#pragma once

// Pure Chest ESP helpers. Header-only, no JNI/ImGui.
// Chunk range is Chebyshev distance in chunk coords (same as Block ESP).

#include <algorithm>
#include <cmath>
#include <vector>

namespace lc {

inline int ClampOverlayChunkRange(int range) {
    if (range < 1) return 1;
    if (range > 8) return 8;
    return range;
}

inline int ClampChestEspRange(int range) {
    return ClampOverlayChunkRange(range);
}

inline bool InChunkRange(int blockX, int blockZ, double playerX, double playerZ, int rangeChunks) {
    const int range = ClampOverlayChunkRange(rangeChunks);
    const int pcx = ((int)std::floor(playerX)) >> 4;
    const int pcz = ((int)std::floor(playerZ)) >> 4;
    const int ccx = blockX >> 4;
    const int ccz = blockZ >> 4;
    int dx = pcx - ccx; if (dx < 0) dx = -dx;
    int dz = pcz - ccz; if (dz < 0) dz = -dz;
    return dx <= range && dz <= range;
}

inline bool ChestEspInChunkRange(int blockX, int blockZ, double playerX, double playerZ, int rangeChunks) {
    return InChunkRange(blockX, blockZ, playerX, playerZ, rangeChunks);
}

// Held-item JNI (ItemStack.getDisplayName) is only worth it up close.
constexpr double kNametagHeldItemMaxDist = 24.0;
constexpr double kEntityJsonMaxDist = 48.0;
constexpr double kClosestPlayerMaxDist = 96.0;

inline bool NametagShouldFetchHeldItem(bool showHeldItem, double dist) {
    return showHeldItem && dist == dist && dist <= kNametagHeldItemMaxDist;
}

// String/class-name chest fallback is for mapping failures, not the per-tile hot path.
inline bool ChestEspNeedNameFallback(int resolvedTypedCount, int requiredTypedCount) {
    return resolvedTypedCount < requiredTypedCount;
}

inline bool ChestEspNeedClassNameFallback(bool haveChestClass, bool haveEnderChestClass) {
    return ChestEspNeedNameFallback(
        (haveChestClass ? 1 : 0) + (haveEnderChestClass ? 1 : 0), 2);
}

inline bool OverlayEntityNeedsFullScan(
    bool nametagsEnabled,
    bool inNametagRange,
    bool fightStatusEnabled,
    bool hideVanilla,
    bool closestPlayerInfo,
    double dist,
    bool jsonSlotOpen)
{
    if (hideVanilla) return true;
    if (dist != dist) return false;
    if (fightStatusEnabled) return true;
    if (nametagsEnabled && inNametagRange) return true;
    if (closestPlayerInfo && dist <= kClosestPlayerMaxDist) return true;
    if (jsonSlotOpen && dist <= kEntityJsonMaxDist) return true;
    return false;
}

struct ChestEspCandidate {
    int bx;
    int by;
    int bz;
    double distSq;
};

inline void ChestEspSortNearestFirst(std::vector<ChestEspCandidate>& items) {
    std::sort(items.begin(), items.end(),
        [](const ChestEspCandidate& a, const ChestEspCandidate& b) {
            const bool aOk = a.distSq == a.distSq;
            const bool bOk = b.distSq == b.distSq;
            if (aOk != bOk) return aOk;
            return a.distSq < b.distSq;
        });
}

} // namespace lc
