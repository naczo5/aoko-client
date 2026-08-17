#pragma once

// Shared chunk-ring helpers for Block ESP, BedPlates, and Chest ESP.
// Chebyshev distance in chunk coords; each module still keeps its own range.

#include <algorithm>
#include <utility>
#include <vector>

namespace lc {

inline int AbsI(int v) { return v < 0 ? -v : v; }

inline int ClampChunkRange(int range) {
    if (range < 1) return 1;
    if (range > 8) return 8;
    return range;
}

inline long long ChunkKey(int cx, int cz) {
    return ((long long)(unsigned int)cx << 32) | (unsigned int)cz;
}

inline bool ChunkInChebyshevRange(int cx, int cz, int pcx, int pcz, int range) {
    range = ClampChunkRange(range);
    return AbsI(cx - pcx) <= range && AbsI(cz - pcz) <= range;
}

inline int MaxEnabledChunkRange(bool aOn, int aRange, bool bOn, int bRange) {
    int m = 0;
    if (aOn) m = ClampChunkRange(aRange);
    if (bOn) {
        int b = ClampChunkRange(bRange);
        if (b > m) m = b;
    }
    return m;
}

inline int MaxEnabledChunkRange3(bool aOn, int aRange, bool bOn, int bRange, bool cOn, int cRange) {
    int m = MaxEnabledChunkRange(aOn, aRange, bOn, bRange);
    if (cOn) {
        int c = ClampChunkRange(cRange);
        if (c > m) m = c;
    }
    return m;
}

// Block ESP round-robins 1 chunk/tick; BedPlates 4. Combined visit uses the max.
// Pass enabled-module flags (not due flags) so a due tick can fill both caches.
inline int CombinedSectionChunkBudget(bool scanBlockEsp, bool scanBedPlates) {
    int budget = 0;
    if (scanBlockEsp) budget = 1;
    if (scanBedPlates && budget < 4) budget = 4;
    return budget;
}

// Chest ESP walks block-entity maps (cheaper than a full section scan).
// 9 chunks/tick fills the player's 3x3 first, then round-robins the rest.
inline int ChestEspChunkScanBudget() {
    return 9;
}

inline void BuildNearToFarChunkOffsets(int range, std::vector<std::pair<int, int> >& out) {
    range = ClampChunkRange(range);
    out.clear();
    out.reserve((size_t)(2 * range + 1) * (size_t)(2 * range + 1));
    for (int dx = -range; dx <= range; dx++)
        for (int dz = -range; dz <= range; dz++)
            out.push_back(std::make_pair(dx, dz));
    std::sort(out.begin(), out.end(),
              [](const std::pair<int, int>& a, const std::pair<int, int>& b) {
                  int da = AbsI(a.first) + AbsI(a.second);
                  int db = AbsI(b.first) + AbsI(b.second);
                  return da < db;
              });
}

template<typename CacheMap>
inline void EvictChunksOutsideRange(CacheMap& cache, int pcx, int pcz, int range) {
    for (auto it = cache.begin(); it != cache.end(); ) {
        int ccx = (int)(it->first >> 32);
        int ccz = (int)(it->first & 0xffffffff);
        if (!ChunkInChebyshevRange(ccx, ccz, pcx, pcz, range))
            it = cache.erase(it);
        else
            ++it;
    }
}

} // namespace lc
