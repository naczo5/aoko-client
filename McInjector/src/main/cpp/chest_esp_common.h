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

struct ChestEspCandidate {
    int bx;
    int by;
    int bz;
    double distSq;
};

inline void ChestEspSortNearestFirst(std::vector<ChestEspCandidate>& items) {
    std::sort(items.begin(), items.end(),
        [](const ChestEspCandidate& a, const ChestEspCandidate& b) {
            return a.distSq < b.distSq;
        });
}

} // namespace lc
