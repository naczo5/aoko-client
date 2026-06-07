#pragma once
#include <map>
#include <string>
#include <algorithm>
#include <cmath>
#include <sstream>
#include "imgui.h"

namespace lc {

// ── Element id constants ───────────────────────────────────────────
static const char* ELEM_MODULELIST    = "modulelist";
static const char* ELEM_CLOSESTPLAYER = "closestplayer";
static const char* ELEM_PIXELPARTY    = "pixelparty";
static const char* ELEM_CHESTESPLIST  = "chestesplist";
static const char* ELEM_GTBHINT       = "gtbhint";
static const char* ELEM_NAMETAGS      = "nametags";

struct HudElementLayout {
    float x = 0.0f;     // normalized [0,1]
    float y = 0.0f;     // normalized [0,1]
    float scale = 1.0f; // [0.5, 2.0]
    bool movable = true;
    bool resizable = true;
};

struct HudLayout {
    std::map<std::string, HudElementLayout> elements;

    HudElementLayout Resolve(const std::string& id) const {
        auto it = elements.find(id);
        if (it != elements.end()) return it->second;
        return DefaultLayout().Resolve(id);
    }

    static HudLayout DefaultLayout() {
        HudLayout L;
        HudElementLayout ml; ml.x=0.985f; ml.y=0.020f; ml.scale=1.0f; ml.movable=true; ml.resizable=true;
        HudElementLayout cp; cp.x=0.500f; cp.y=0.780f; cp.scale=1.0f; cp.movable=true; cp.resizable=true;
        HudElementLayout pp; pp.x=0.500f; pp.y=0.660f; pp.scale=1.0f; pp.movable=true; pp.resizable=true;
        HudElementLayout ce; ce.x=0.015f; ce.y=0.300f; ce.scale=1.0f; ce.movable=true; ce.resizable=true;
        HudElementLayout gb; gb.x=0.500f; gb.y=0.620f; gb.scale=1.0f; gb.movable=true; gb.resizable=true;
        HudElementLayout nt; nt.x=0.000f; nt.y=0.000f; nt.scale=1.0f; nt.movable=false; nt.resizable=true;
        L.elements[ELEM_MODULELIST]    = ml;
        L.elements[ELEM_CLOSESTPLAYER] = cp;
        L.elements[ELEM_PIXELPARTY]    = pp;
        L.elements[ELEM_CHESTESPLIST]  = ce;
        L.elements[ELEM_GTBHINT]       = gb;
        L.elements[ELEM_NAMETAGS]      = nt;
        return L;
    }
};

struct HudEditorState {
    bool active = false;
    std::string grabbedId;
    enum class Mode { None, Move, Resize } mode = Mode::None;
    ImVec2 grabOffset{0,0};
    bool layoutDirty = false;
};

inline float ClampHudCoord(float v)  { return (std::max)(0.0f, (std::min)(1.0f, v)); }
inline float ClampHudScale(float v)  { return (std::max)(0.5f, (std::min)(2.0f, (std::isfinite(v) && v > 0.0f) ? v : 1.0f)); }

inline HudElementLayout ClampHudElement(HudElementLayout e) {
    e.x     = ClampHudCoord(e.x);
    e.y     = ClampHudCoord(e.y);
    e.scale = ClampHudScale(e.scale);
    return e;
}

// Parse a single element from a JSON substring: {"x":0.5,"y":0.7,"scale":1.0}
// Returns true if at least x or y was found.
inline bool ParseHudElement(const std::string& src, HudElementLayout& out, const HudElementLayout& def) {
    // Very simple key-value extraction, no library dependency
    auto readFloat = [&](const char* key, float fallback) -> float {
        std::string marker = std::string("\"") + key + "\":";
        size_t pos = src.find(marker);
        if (pos == std::string::npos) return fallback;
        pos += marker.size();
        if (pos >= src.size()) return fallback;
        try { return std::stof(src.substr(pos)); } catch (...) { return fallback; }
    };
    out.x     = ClampHudCoord(readFloat("x",     def.x));
    out.y     = ClampHudCoord(readFloat("y",     def.y));
    out.scale = ClampHudScale(readFloat("scale", def.scale));
    return true;
}

// Parse hudLayout JSON object: {"modulelist":{"x":...},...}
// Works with SimpleJsonConfigReader-style raw config line.
inline HudLayout ParseHudLayout(const std::string& fullLine) {
    HudLayout layout = HudLayout::DefaultLayout();

    // Find the hudLayout object in the line
    std::string marker = "\"hudLayout\":";
    size_t pos = fullLine.find(marker);
    if (pos == std::string::npos) return layout;
    pos += marker.size();
    if (pos >= fullLine.size() || fullLine[pos] != '{') return layout;

    // Extract the hudLayout object (find matching closing brace)
    size_t start = pos; // points at '{'
    int depth = 0;
    size_t end = start;
    for (size_t i = start; i < fullLine.size(); ++i) {
        if (fullLine[i] == '{') depth++;
        else if (fullLine[i] == '}') {
            depth--;
            if (depth == 0) { end = i; break; }
        }
    }
    if (end == start) return layout;
    std::string hudObj = fullLine.substr(start, end - start + 1);

    // For each known element id, find its sub-object
    static const char* ids[] = {
        ELEM_MODULELIST, ELEM_CLOSESTPLAYER, ELEM_PIXELPARTY,
        ELEM_CHESTESPLIST, ELEM_GTBHINT, ELEM_NAMETAGS, nullptr
    };
    for (int i = 0; ids[i]; ++i) {
        std::string idMarker = std::string("\"") + ids[i] + "\":{";
        size_t p = hudObj.find(idMarker);
        if (p == std::string::npos) continue;
        p += idMarker.size() - 1; // points at '{'
        int d = 0;
        size_t e = p;
        for (size_t j = p; j < hudObj.size(); ++j) {
            if (hudObj[j] == '{') d++;
            else if (hudObj[j] == '}') { d--; if (d == 0) { e = j; break; } }
        }
        if (e == p) continue;
        std::string elemStr = hudObj.substr(p, e - p + 1);
        auto& def = layout.elements[ids[i]];
        ParseHudElement(elemStr, def, def);
        layout.elements[ids[i]] = ClampHudElement(def);
    }
    return layout;
}

// Serialize hudLayout to JSON string (for outbound state)
inline std::string SerializeHudLayout(const HudLayout& L) {
    std::string s = "{";
    bool first = true;
    static const char* ids[] = {
        ELEM_MODULELIST, ELEM_CLOSESTPLAYER, ELEM_PIXELPARTY,
        ELEM_CHESTESPLIST, ELEM_GTBHINT, ELEM_NAMETAGS, nullptr
    };
    for (int i = 0; ids[i]; ++i) {
        auto it = L.elements.find(ids[i]);
        HudElementLayout e = (it != L.elements.end()) ? it->second : HudElementLayout{};
        if (!first) s += ",";
        first = false;
        char buf[128];
        snprintf(buf, sizeof(buf), "\"%s\":{\"x\":%.6f,\"y\":%.6f,\"scale\":%.6f}",
                 ids[i], e.x, e.y, e.scale);
        s += buf;
    }
    s += "}";
    return s;
}

// Compute top-left pixel position for an element from its normalized anchor.
// contentW/H = scaled dimensions of the element in pixels.
// Guards against winW/H == 0.
inline ImVec2 HudElementPixelPos(const HudElementLayout& l, float contentW, float contentH, int winW, int winH) {
    if (winW <= 0 || winH <= 0) return ImVec2(0, 0);
    float px = l.x * (float)winW;
    float py = l.y * (float)winH;
    float maxX = (std::max)(0.0f, (float)winW - contentW);
    float maxY = (std::max)(0.0f, (float)winH - contentH);
    px = (std::max)(0.0f, (std::min)(px, maxX));
    py = (std::max)(0.0f, (std::min)(py, maxY));
    return ImVec2(std::floor(px), std::floor(py));
}

} // namespace lc
