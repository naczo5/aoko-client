#pragma once

// Pure BedPlates helpers (Vape V4.21 BedPlates port). Header-only, no JNI/ImGui.
// Layer shell sampling + frequency aggregation around a bed head/foot pair.
// Facing indices match Minecraft EnumFacing.getIndex(): NORTH=2 SOUTH=3 WEST=4 EAST=5.

#include <algorithm>
#include <cmath>
#include <string>
#include <utility>
#include <vector>
#include <map>
#include <cctype>

#include "block_esp_common.h"

namespace lc {

enum BedFacingIndex {
    kBedFacingNorth = 2,
    kBedFacingSouth = 3,
    kBedFacingWest  = 4,
    kBedFacingEast  = 5
};

inline bool IsBedBlockId(const std::string& rawId) {
    std::string id = BlockEspNormalizeId(rawId);
    if (id.empty()) return false;
    if (id == "bed" || id == "bedblock") return true;
    // Colored beds: red_bed, white_bed, ...
    const std::string suffix = "_bed";
    if (id.size() > suffix.size()
        && id.compare(id.size() - suffix.size(), suffix.size(), suffix) == 0) {
        return true;
    }
    // Some runtimes surface "tile.bed.name" → "bed_name" after normalize.
    if (id == "bed_name" || id.rfind("bed_", 0) == 0) return true;
    return false;
}

// Blocks that must never appear on a BedPlate (air variants, empty).
// 1.8.9 Forge sometimes surfaces "tile.air.name" → normalized "air_name" ("Air Name").
inline bool IsIgnorableBedPlateBlockId(const std::string& rawId) {
    std::string id = BlockEspNormalizeId(rawId);
    if (id.empty()) return true;
    if (id == "air" || id == "air_name" || id == "cave_air" || id == "void_air") return true;
    if (id == "empty" || id == "null" || id == "none") return true;
    // Catch any remaining air* token without eating unrelated ids like "stairs".
    if (id.size() >= 3 && id.compare(0, 3, "air") == 0
        && (id.size() == 3 || id[3] == '_')) return true;
    if (id.size() > 4 && id.compare(id.size() - 4, 4, "_air") == 0) return true;
    return false;
}

// Map foot offset (relative to head) to Vape/Minecraft facing index.
// Returns 2..5, or 2 (NORTH) when offset is zero/unknown.
inline int BedFacingFromFootOffset(int ox, int oz) {
    if (oz > 0) return kBedFacingNorth;
    if (oz < 0) return kBedFacingSouth;
    if (ox > 0) return kBedFacingWest;
    if (ox < 0) return kBedFacingEast;
    return kBedFacingNorth;
}

inline void BedFootOffsetFromFacing(int facing, int* outOx, int* outOz) {
    int ox = 0, oz = 0;
    if (facing == kBedFacingNorth) oz = 1;
    else if (facing == kBedFacingSouth) oz = -1;
    else if (facing == kBedFacingWest) ox = 1;
    else if (facing == kBedFacingEast) ox = -1;
    // facing == 0 (or anything else): unpaired / unknown — no foot offset.
    if (outOx) *outOx = ox;
    if (outOz) *outOz = oz;
}

// Prefer the "canonical" half of a bed pair so each bed is counted once.
// Keep the block that has no adjacent bed at -X/-Z (lower key of the pair).
inline bool BedPlatesIsCanonicalHalf(bool hasNegXBed, bool hasNegZBed) {
    return !(hasNegXBed || hasNegZBed);
}

inline double BedPlatesHorizDist(int x0, int z0, int x1, int z1) {
    double dx = (double)(x0 - x1);
    double dz = (double)(z0 - z1);
    return std::sqrt(dx * dx + dz * dz);
}

struct BedPlateSample {
    int x, y, z;
    int layer;
};

// Generate hollow-shell samples around a bed for layers 1..3 (Vape updateCountStates).
// headX/Y/Z is the canonical bed position; facing expands the AABB over the foot.
inline void CollectBedPlateSamples(int headX, int headY, int headZ, int facing,
                                   std::vector<BedPlateSample>& out) {
    out.clear();
    int bedOffsetX = 0, bedOffsetZ = 0;
    BedFootOffsetFromFacing(facing, &bedOffsetX, &bedOffsetZ);

    const int layerLimit = 4;
    for (int radius = 1; radius < layerLimit; ++radius) {
        int minX = -radius;
        int minZ = -radius;
        int maxX = radius;
        int maxZ = radius;
        int ox = bedOffsetX;
        int oz = bedOffsetZ;
        if (facing == kBedFacingNorth) { ++maxZ; }
        if (facing == kBedFacingSouth) { --minZ; }
        if (facing == kBedFacingWest)  { ++maxX; }
        if (facing == kBedFacingEast)  { --minX; }

        for (int offsetY = 0; offsetY <= radius; ++offsetY) {
            for (int offsetX = minX; offsetX <= maxX; ++offsetX) {
                for (int offsetZ = minZ; offsetZ <= maxZ; ++offsetZ) {
                    if (offsetX != minX && offsetX != maxX
                        && offsetZ != minZ && offsetZ != maxZ
                        && std::abs(offsetY) != radius) {
                        continue;
                    }
                    double distanceFromFoot =
                        BedPlatesHorizDist(headX, headZ, headX + offsetX, headZ + offsetZ)
                        + (double)offsetY;
                    double distanceFromHead =
                        BedPlatesHorizDist(headX + ox, headZ + oz,
                                           headX + offsetX, headZ + offsetZ)
                        + (double)offsetY;
                    bool belongsToOuterLayer =
                        distanceFromFoot > (double)radius && distanceFromHead > (double)radius;
                    int targetLayer = belongsToOuterLayer ? radius + 1 : radius;
                    if (targetLayer >= layerLimit) continue;
                    BedPlateSample s;
                    s.x = headX + offsetX;
                    s.y = headY + offsetY;
                    s.z = headZ + offsetZ;
                    s.layer = targetLayer;
                    out.push_back(s);
                }
            }
        }
    }
}

struct BedPlateCountState {
    int x = 0, y = 0, z = 0;
    // layer -> (blockId -> count)
    std::map<int, std::map<std::string, int> > layerCounts;
    // layer -> ids sorted by descending frequency
    std::map<int, std::vector<std::string> > sortedLayers;

    void clearCounts() {
        layerCounts.clear();
        sortedLayers.clear();
    }

    void incrementBlock(int layer, const std::string& blockId) {
        if (IsIgnorableBedPlateBlockId(blockId)) return;
        if (IsBedBlockId(blockId)) return;
        std::string id = BlockEspNormalizeId(blockId);
        if (id.empty()) return;
        layerCounts[layer][id] += 1;
    }

    void sortLayersByFrequency() {
        sortedLayers.clear();
        for (std::map<int, std::map<std::string, int> >::const_iterator it = layerCounts.begin();
             it != layerCounts.end(); ++it) {
            std::vector<std::pair<std::string, int> > entries(
                it->second.begin(), it->second.end());
            std::sort(entries.begin(), entries.end(),
                      [](const std::pair<std::string, int>& a,
                         const std::pair<std::string, int>& b) {
                          if (a.second != b.second) return a.second > b.second;
                          return a.first < b.first;
                      });
            std::vector<std::string>& sorted = sortedLayers[it->first];
            sorted.reserve(entries.size());
            for (size_t i = 0; i < entries.size(); ++i)
                sorted.push_back(entries[i].first);
        }
    }

    // Unique block ids across layers 1..3 (first appearance wins), matching Vape renderPlate.
    void collectVisibleBlockIds(std::vector<std::string>& out) const {
        out.clear();
        for (int layer = 1; layer < 4; ++layer) {
            std::map<int, std::vector<std::string> >::const_iterator it = sortedLayers.find(layer);
            if (it == sortedLayers.end()) break;
            int visibleInLayer = 0;
            for (size_t i = 0; i < it->second.size(); ++i) {
                const std::string& id = it->second[i];
                if (id.empty()) continue;
                ++visibleInLayer;
                bool already = false;
                for (size_t j = 0; j < out.size(); ++j) {
                    if (out[j] == id) { already = true; break; }
                }
                if (!already) out.push_back(id);
            }
            if (visibleInLayer == 0) break;
        }
    }
};

// "white_wool" -> "White Wool", "end_stone" -> "End Stone", "bed" -> "Bed"
inline std::string BedPlatePrettyLabel(const std::string& rawId) {
    std::string id = BlockEspNormalizeId(rawId);
    if (id.empty()) return "?";
    std::string out;
    out.reserve(id.size() + 4);
    bool cap = true;
    for (size_t i = 0; i < id.size(); ++i) {
        char c = id[i];
        if (c == '_') {
            out.push_back(' ');
            cap = true;
            continue;
        }
        if (cap) {
            out.push_back((char)std::toupper((unsigned char)c));
            cap = false;
        } else {
            out.push_back(c);
        }
    }
    return out;
}

// Compact plate chip label (readable without textures). Prefer known bedwars blocks.
inline std::string BedPlateShortLabel(const std::string& rawId) {
    std::string id = BlockEspNormalizeId(rawId);
    if (id.empty()) return "?";
    if (id.find("obsidian") != std::string::npos) return "Obs";
    if (id.find("end_stone") != std::string::npos || id == "whitestone") return "ES";
    if (id.find("glass") != std::string::npos) return "Gls";
    if (id.find("ladder") != std::string::npos) return "Ldr";
    if (id.find("water") != std::string::npos) return "H2O";
    if (id.find("lava") != std::string::npos) return "Lav";
    if (id.find("sandstone") != std::string::npos) return "SS";
    if (id.find("concrete") != std::string::npos) return "Cnc";
    if (id.find("terracotta") != std::string::npos || id.find("hardenedclay") != std::string::npos
        || id.find("stainedclay") != std::string::npos || id.find("clayhardened") != std::string::npos)
        return "Ter";
    if (id.find("plank") != std::string::npos || id.find("wood") != std::string::npos
        || id.find("log") != std::string::npos) return "Wd";
    if (id.find("iron") != std::string::npos) return "Irn";
    if (id.find("wool") != std::string::npos || id == "cloth") {
        if (id.find("white") != std::string::npos) return "WWl";
        if (id.find("red") != std::string::npos) return "RWl";
        if (id.find("blue") != std::string::npos) return "BWl";
        if (id.find("green") != std::string::npos) return "GWl";
        if (id.find("yellow") != std::string::npos) return "YWl";
        if (id.find("orange") != std::string::npos) return "OWl";
        if (id.find("pink") != std::string::npos) return "PWl";
        if (id.find("black") != std::string::npos) return "KWl";
        if (id.find("cyan") != std::string::npos) return "CWl";
        if (id.find("lime") != std::string::npos) return "LWl";
        return "Wl";
    }
    if (id.find("bed") != std::string::npos) return "Bed";
    if (id.find("stone") != std::string::npos) return "Stn";
    if (id.find("dirt") != std::string::npos) return "Drt";
    if (id.find("sand") != std::string::npos) return "Snd";
    if (id.find("gravel") != std::string::npos) return "Grv";
    if (id.find("slab") != std::string::npos) return "Slb";
    if (id.find("stair") != std::string::npos) return "Str";
    // Fallback: last underscore token, first 3 chars, capitalized.
    size_t us = id.find_last_of('_');
    std::string tok = (us == std::string::npos) ? id : id.substr(us + 1);
    if (tok.empty()) tok = id;
    std::string out;
    for (size_t i = 0; i < tok.size() && out.size() < 3; ++i) {
        char c = tok[i];
        if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9'))
            out.push_back(out.empty() ? (char)std::toupper((unsigned char)c) : (char)std::tolower((unsigned char)c));
    }
    return out.empty() ? "?" : out;
}

} // namespace lc
