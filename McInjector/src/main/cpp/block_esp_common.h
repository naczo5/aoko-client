#pragma once

// Shared Block ESP / X-ray helpers for both bridges (1.8.9 + modern) and the native
// test harness. Mirrors the C# Aoko.Core.BlockEspConfig encoding:
//   "blockEspBlocks" : "<id>=<RRGGBB>;<id>=<RRGGBB>"
// ids are normalized to a lowercase path token (e.g. "diamond_ore"); colors are 6-digit
// RRGGBB hex. Intentionally header-only and free of imgui/JNI dependencies so the harness
// can include it. Colors are packed in the default ImGui IM_COL32 byte order
// (r | g<<8 | b<<16 | a<<24) with alpha forced to 0xFF.

#include <string>
#include <vector>
#include <cctype>

namespace lc {

struct BlockEspTargetDef {
    std::string id;          // normalized path token, e.g. "diamond_ore"
    unsigned int color;      // 0xAABBGGRR packed like IM_COL32(r,g,b,a)
};

// Default swatch color = cyan 00E5FF -> IM_COL32(0x00,0xE5,0xFF,0xFF).
inline unsigned int BlockEspDefaultColor() {
    return (unsigned int)(0x00u | (0xE5u << 8) | (0xFFu << 16) | (0xFFu << 24));
}

// Reduce any id form (minecraft:diamond_ore | diamond_ore | block.minecraft.diamond_ore)
// to a lowercase path token. Returns "" if nothing valid remains.
inline std::string BlockEspNormalizeId(const std::string& raw) {
    std::string v;
    v.reserve(raw.size());
    for (char c : raw) {
        if (c == ' ' || c == '\t' || c == '\r' || c == '\n') continue;
        v.push_back((char)std::tolower((unsigned char)c));
    }
    if (v.rfind("block.", 0) == 0) v = v.substr(6);

    size_t colon = v.find(':');
    if (colon != std::string::npos) {
        v = v.substr(colon + 1);
    } else {
        size_t dot = v.find('.');
        if (dot != std::string::npos) v = v.substr(dot + 1);
    }

    std::string out;
    out.reserve(v.size());
    for (char c : v) {
        if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_') out.push_back(c);
        else if (c == '.' || c == '/') out.push_back('_');
    }
    size_t b = out.find_first_not_of('_');
    if (b == std::string::npos) return "";
    size_t e = out.find_last_not_of('_');
    return out.substr(b, e - b + 1);
}

// Parse a 6-digit RRGGBB hex string into a packed color. Returns the default cyan on error.
inline unsigned int BlockEspParseColor(const std::string& hex6) {
    auto hv = [](char c) -> int {
        if (c >= '0' && c <= '9') return c - '0';
        if (c >= 'a' && c <= 'f') return c - 'a' + 10;
        if (c >= 'A' && c <= 'F') return c - 'A' + 10;
        return -1;
    };
    if (hex6.size() != 6) return BlockEspDefaultColor();
    int r1 = hv(hex6[0]), r2 = hv(hex6[1]);
    int g1 = hv(hex6[2]), g2 = hv(hex6[3]);
    int b1 = hv(hex6[4]), b2 = hv(hex6[5]);
    if (r1 < 0 || r2 < 0 || g1 < 0 || g2 < 0 || b1 < 0 || b2 < 0) return BlockEspDefaultColor();
    unsigned int r = (unsigned int)(r1 * 16 + r2);
    unsigned int g = (unsigned int)(g1 * 16 + g2);
    unsigned int b = (unsigned int)(b1 * 16 + b2);
    return (r | (g << 8) | (b << 16) | (0xFFu << 24));
}

// Parse the delimited wire string into normalized, de-duplicated targets.
inline std::vector<BlockEspTargetDef> ParseBlockEspTargets(const std::string& serialized) {
    std::vector<BlockEspTargetDef> out;
    size_t i = 0;
    while (i < serialized.size()) {
        size_t semi = serialized.find(';', i);
        std::string entry = (semi == std::string::npos)
            ? serialized.substr(i)
            : serialized.substr(i, semi - i);
        i = (semi == std::string::npos) ? serialized.size() : semi + 1;

        if (entry.empty()) continue;
        size_t eq = entry.find('=');
        if (eq == std::string::npos || eq == 0) continue;

        std::string id = BlockEspNormalizeId(entry.substr(0, eq));
        if (id.empty()) continue;

        bool dup = false;
        for (const auto& t : out) { if (t.id == id) { dup = true; break; } }
        if (dup) continue;

        unsigned int color = BlockEspParseColor(entry.substr(eq + 1));
        out.push_back({ id, color });
    }
    return out;
}

} // namespace lc
