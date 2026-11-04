#pragma once
// ─────────────────────────────────────────────────────────────────────────────
// Aoko overlay UI — shared, renderer-agnostic ImGui draw layer.
//
// Header-only on purpose: both native bridges compile their own copy, and the
// whole surface is immediate-mode drawing with no ABI or deployment weight.
// If a third consumer ever appears, promote this file to a static library.
// ─────────────────────────────────────────────────────────────────────────────
#include "imgui.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <map>
#include <string>
#include <vector>

namespace aoko {
namespace ui {

// ── Color helpers ───────────────────────────────────────────────────────────
inline ImU32 Col(int r, int g, int b, int a = 255) {
    return IM_COL32(r, g, b, a);
}

inline int AlphaOf(ImU32 c) {
    return (int)((c >> IM_COL32_A_SHIFT) & 0xFF);
}

inline ImU32 WithAlpha(ImU32 c, int a) {
    if (a < 0) a = 0;
    if (a > 255) a = 255;
    return (c & ~IM_COL32_A_MASK) | ((ImU32)a << IM_COL32_A_SHIFT);
}

inline ImU32 ScaleAlpha(ImU32 c, float mul) {
    if (mul <= 0.0f) return WithAlpha(c, 0);
    if (mul > 1.0f) mul = 1.0f;
    return WithAlpha(c, (int)(AlphaOf(c) * mul + 0.5f));
}

inline ImU32 Mix(ImU32 a, ImU32 b, float t) {
    if (t <= 0.0f) return a;
    if (t >= 1.0f) return b;
    const int ar = (a >> IM_COL32_R_SHIFT) & 0xFF;
    const int ag = (a >> IM_COL32_G_SHIFT) & 0xFF;
    const int ab = (a >> IM_COL32_B_SHIFT) & 0xFF;
    const int aa = (a >> IM_COL32_A_SHIFT) & 0xFF;
    const int br = (b >> IM_COL32_R_SHIFT) & 0xFF;
    const int bg = (b >> IM_COL32_G_SHIFT) & 0xFF;
    const int bb = (b >> IM_COL32_B_SHIFT) & 0xFF;
    const int ba = (b >> IM_COL32_A_SHIFT) & 0xFF;
    return IM_COL32(
        (int)(ar + (br - ar) * t + 0.5f),
        (int)(ag + (bg - ag) * t + 0.5f),
        (int)(ab + (bb - ab) * t + 0.5f),
        (int)(aa + (ba - aa) * t + 0.5f));
}

inline ImU32 SemanticGood() { return IM_COL32(128, 216, 178, 255); }
inline ImU32 SemanticWarn() { return IM_COL32(236, 194, 112, 255); }
inline ImU32 SemanticBad()  { return IM_COL32(238, 112, 112, 255); }

inline ImU32 PingColor(int pingMs) {
    if (pingMs < 0) return IM_COL32(150, 164, 178, 230);
    if (pingMs < 70) return SemanticGood();
    if (pingMs < 130) return SemanticWarn();
    return SemanticBad();
}

inline ImU32 FpsColor(int fps) {
    if (fps <= 0) return IM_COL32(150, 164, 178, 230);
    if (fps >= 144) return SemanticGood();
    if (fps >= 60) return IM_COL32(226, 234, 242, 250);
    if (fps >= 30) return SemanticWarn();
    return SemanticBad();
}

inline ImU32 HealthColor(float pct) {
    pct = pct < 0.0f ? 0.0f : (pct > 1.0f ? 1.0f : pct);
    if (pct < 0.5f) return Mix(SemanticBad(), SemanticWarn(), pct * 2.0f);
    return Mix(SemanticWarn(), SemanticGood(), (pct - 0.5f) * 2.0f);
}

inline ImU32 PartyColor(const std::string& rawLabel) {
    std::string s;
    s.reserve(rawLabel.size());
    for (size_t i = 0; i < rawLabel.size(); ++i)
        s.push_back((char)std::tolower((unsigned char)rawLabel[i]));
    if (s.find("red") != std::string::npos) return IM_COL32(232, 92, 92, 255);
    if (s.find("orange") != std::string::npos) return IM_COL32(238, 148, 76, 255);
    if (s.find("yellow") != std::string::npos) return IM_COL32(236, 208, 96, 255);
    if (s.find("lime") != std::string::npos || s.find("green") != std::string::npos)
        return IM_COL32(126, 214, 138, 255);
    if (s.find("cyan") != std::string::npos || s.find("aqua") != std::string::npos)
        return IM_COL32(98, 214, 220, 255);
    if (s.find("blue") != std::string::npos) return IM_COL32(112, 150, 246, 255);
    if (s.find("purple") != std::string::npos || s.find("violet") != std::string::npos)
        return IM_COL32(170, 128, 238, 255);
    if (s.find("pink") != std::string::npos || s.find("magenta") != std::string::npos)
        return IM_COL32(232, 128, 198, 255);
    if (s.find("white") != std::string::npos) return IM_COL32(232, 238, 244, 255);
    if (s.find("black") != std::string::npos || s.find("gray") != std::string::npos ||
        s.find("grey") != std::string::npos) return IM_COL32(118, 126, 138, 255);
    return IM_COL32(190, 202, 214, 255);
}

// ── Motion ──────────────────────────────────────────────────────────────────
inline float Approach(float current, float target, float rate, float dt) {
    if (dt <= 0.0f) return current;
    if (dt > 0.10f) dt = 0.10f;
    const float k = 1.0f - std::exp(-rate * dt);
    return current + (target - current) * k;
}

inline float EaseOutCubic(float t) {
    if (t <= 0.0f) return 0.0f;
    if (t >= 1.0f) return 1.0f;
    const float u = 1.0f - t;
    return 1.0f - u * u * u;
}

inline float Clamp01(float v) {
    return v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v);
}

// ── Theme ───────────────────────────────────────────────────────────────────
struct Theme {
    ImU32 accent;
    ImU32 accent2;
    ImU32 accent3;
    ImU32 text;
    ImU32 textDim;
    ImU32 textFaint;
    ImU32 panel;
    ImU32 panelTop;
    ImU32 border;
    ImU32 highlight;
    ImU32 shadow;
    bool gradient;
    bool triColor;

    Theme()
        : accent(IM_COL32(199, 98, 90, 255)),
          accent2(IM_COL32(199, 98, 90, 255)),
          accent3(IM_COL32(199, 98, 90, 255)),
          text(IM_COL32(237, 243, 247, 245)),
          textDim(IM_COL32(156, 175, 188, 230)),
          textFaint(IM_COL32(122, 138, 152, 190)),
          panel(IM_COL32(9, 18, 30, 214)),
          panelTop(IM_COL32(17, 31, 47, 214)),
          border(IM_COL32(142, 177, 204, 46)),
          highlight(IM_COL32(255, 255, 255, 30)),
          shadow(IM_COL32(0, 0, 0, 150)),
          gradient(false),
          triColor(false) {}

    ImU32 AccentAt(float t) const {
        if (!gradient) return accent;
        if (!triColor) return Mix(accent, accent2, Clamp01(t));
        if (t <= 0.5f) return Mix(accent, accent2, Clamp01(t * 2.0f));
        return Mix(accent2, accent3, Clamp01((t - 0.5f) * 2.0f));
    }
};

// ── Geometry primitives ─────────────────────────────────────────────────────
inline void Panel(ImDrawList* dl, const Theme& th, ImVec2 a, ImVec2 b,
                  float rounding, float glow = 0.0f, bool sheen = true) {
    if (glow > 0.01f) {
        for (int i = 3; i >= 1; --i) {
            const float e = (float)i * 1.7f;
            const int alpha = (int)(glow * (8.0f + (float)(3 - i) * 5.0f));
            dl->AddRectFilled(ImVec2(a.x - e, a.y - e), ImVec2(b.x + e, b.y + e),
                              WithAlpha(th.accent, alpha), rounding + e);
        }
    }
    dl->AddRectFilled(a, b, th.panel, rounding);
    const float topH = (b.y - a.y) * 0.52f;
    if (topH > 1.0f && AlphaOf(th.panelTop) > 0) {
        dl->AddRectFilledMultiColor(
            ImVec2(a.x + 1.0f, a.y + 1.0f), ImVec2(b.x - 1.0f, a.y + topH),
            th.panelTop, th.panelTop, th.panel, th.panel);
    }
    dl->AddRect(a, b, th.border, rounding, 0, 1.0f);
    if (sheen) {
        dl->AddLine(ImVec2(a.x + rounding, a.y + 0.5f),
                    ImVec2(b.x - rounding, a.y + 0.5f), th.highlight, 1.0f);
    }
}

inline void InsetPanel(ImDrawList* dl, ImVec2 a, ImVec2 b, ImU32 fill, ImU32 border,
                       float rounding) {
    dl->AddRectFilled(a, b, fill, rounding);
    dl->AddRect(a, b, border, rounding, 0, 1.0f);
}

inline void VerticalRule(ImDrawList* dl, float x, float y0, float y1, ImU32 color) {
    dl->AddLine(ImVec2(x, y0), ImVec2(x, y1), color, 1.0f);
}

inline void Diamond(ImDrawList* dl, ImVec2 c, float r, ImU32 color) {
    dl->AddQuadFilled(ImVec2(c.x, c.y - r), ImVec2(c.x + r, c.y),
                      ImVec2(c.x, c.y + r), ImVec2(c.x - r, c.y), color);
}

inline void Spark(ImDrawList* dl, ImVec2 c, float r, ImU32 color) {
    if (r <= 0.0f) return;
    // Two crossed kites: a clean four-point sparkle without concave-polygon artifacts.
    dl->AddQuadFilled(ImVec2(c.x, c.y - r), ImVec2(c.x + r * 0.24f, c.y),
                      ImVec2(c.x, c.y + r), ImVec2(c.x - r * 0.24f, c.y), color);
    dl->AddQuadFilled(ImVec2(c.x - r, c.y), ImVec2(c.x, c.y - r * 0.24f),
                      ImVec2(c.x + r, c.y), ImVec2(c.x, c.y + r * 0.24f), color);
}

inline void Chevron(ImDrawList* dl, ImVec2 c, char dir, float r, ImU32 color) {
    ImVec2 p[3];
    if (dir == 'v') {
        p[0] = ImVec2(c.x - r, c.y - r * 0.55f);
        p[1] = ImVec2(c.x + r, c.y - r * 0.55f);
        p[2] = ImVec2(c.x, c.y + r * 0.70f);
    } else if (dir == '<') {
        p[0] = ImVec2(c.x + r * 0.55f, c.y - r);
        p[1] = ImVec2(c.x + r * 0.55f, c.y + r);
        p[2] = ImVec2(c.x - r * 0.70f, c.y);
    } else if (dir == '>') {
        p[0] = ImVec2(c.x - r * 0.55f, c.y - r);
        p[1] = ImVec2(c.x - r * 0.55f, c.y + r);
        p[2] = ImVec2(c.x + r * 0.70f, c.y);
    } else {
        p[0] = ImVec2(c.x - r, c.y + r * 0.55f);
        p[1] = ImVec2(c.x + r, c.y + r * 0.55f);
        p[2] = ImVec2(c.x, c.y - r * 0.70f);
    }
    dl->AddTriangleFilled(p[0], p[1], p[2], color);
}

inline void SignalBars(ImDrawList* dl, ImVec2 tl, float width, float height,
                       int strength, ImU32 color) {
    const int bars = 4;
    const float gap = (std::max)(1.0f, width * 0.10f);
    const float bw = (width - gap * (float)(bars - 1)) / (float)bars;
    for (int i = 0; i < bars; ++i) {
        const float bh = height * (0.26f + 0.25f * (float)i);
        const bool lit = i < strength;
        dl->AddRectFilled(
            ImVec2(tl.x + (bw + gap) * (float)i, tl.y + height - bh),
            ImVec2(tl.x + (bw + gap) * (float)i + bw, tl.y + height),
            lit ? color : WithAlpha(color, 42), 0.8f);
    }
}

inline float TrackedWidth(const char* text, float size, float tracking) {
    if (!text || !*text) return 0.0f;
    const float base = ImGui::CalcTextSize(text).x * (size / ImGui::GetFontSize());
    const int n = (int)std::strlen(text);
    return base + tracking * (float)(n > 0 ? n - 1 : 0);
}

inline void TrackedText(ImDrawList* dl, ImVec2 pos, const char* text, float size,
                        ImU32 color, float tracking) {
    if (!text || !*text) return;
    ImFont* font = ImGui::GetFont();
    float x = pos.x;
    for (const char* p = text; *p; ++p) {
        char glyph[2] = { *p, 0 };
        dl->AddText(font, size, ImVec2(x, pos.y), color, glyph);
        x += ImGui::CalcTextSize(glyph).x * (size / ImGui::GetFontSize()) + tracking;
    }
}

inline void Text(ImDrawList* dl, ImVec2 pos, const char* text, float size, ImU32 color) {
    dl->AddText(ImGui::GetFont(), size, pos, color, text);
}

inline void TextShadowed(ImDrawList* dl, ImVec2 pos, const char* text, float size,
                         ImU32 color, ImU32 shadow, float offset) {
    dl->AddText(ImGui::GetFont(), size, ImVec2(pos.x + offset, pos.y + offset), shadow, text);
    dl->AddText(ImGui::GetFont(), size, pos, color, text);
}

inline void TextRight(ImDrawList* dl, ImVec2 rightTop, const char* text, float size,
                      ImU32 color) {
    const float w = ImGui::CalcTextSize(text).x * (size / ImGui::GetFontSize());
    dl->AddText(ImGui::GetFont(), size, ImVec2(rightTop.x - w, rightTop.y), color, text);
}

inline void TextRightShadowed(ImDrawList* dl, ImVec2 rightTop, const char* text, float size,
                              ImU32 color, ImU32 shadow, float offset) {
    TextShadowed(dl, ImVec2(rightTop.x - ImGui::CalcTextSize(text).x * (size / ImGui::GetFontSize()),
                            rightTop.y), text, size, color, shadow, offset);
}

inline void TextCentered(ImDrawList* dl, float centerX, float y, const char* text,
                         float size, ImU32 color) {
    const float w = ImGui::CalcTextSize(text).x * (size / ImGui::GetFontSize());
    dl->AddText(ImGui::GetFont(), size, ImVec2(centerX - w * 0.5f, y), color, text);
}

inline float TextWidth(const char* text, float size) {
    return ImGui::CalcTextSize(text).x * (size / ImGui::GetFontSize());
}

inline const char* PrettyElementId(const char* id) {
    if (!id) return "";
    if (std::strcmp(id, "modulelist") == 0) return "MODULE LIST";
    if (std::strcmp(id, "closestplayer") == 0) return "CLOSEST PLAYER";
    if (std::strcmp(id, "pixelparty") == 0) return "PIXEL PARTY";
    if (std::strcmp(id, "chestesplist") == 0) return "CHEST ESP";
    if (std::strcmp(id, "gtbhint") == 0) return "GTB HINT";
    if (std::strcmp(id, "nametags") == 0) return "NAMETAGS";
    if (std::strcmp(id, "blockesplist") == 0) return "BLOCK ESP";
    return id;
}

// Corner brackets + a small title chip. Used by the HUD editor.
inline void EditorOutline(ImDrawList* dl, ImVec2 a, ImVec2 b, ImU32 accent,
                          const char* label, bool grabbed, float scale) {
    const float len = (std::min)(14.0f * scale, (b.x - a.x) * 0.22f);
    const float th = grabbed ? 2.0f : 1.4f;
    const ImU32 col = grabbed ? IM_COL32(255, 206, 96, 235) : WithAlpha(accent, 210);

    dl->AddRectFilled(a, b, WithAlpha(accent, grabbed ? 20 : 9), 3.0f * scale);
    dl->AddRect(a, b, WithAlpha(col, grabbed ? 170 : 80), 3.0f * scale, 0, 1.0f);

    const float o = 1.0f;
    // Four L-shaped brackets.
    dl->AddLine(ImVec2(a.x - o, a.y - o), ImVec2(a.x + len, a.y - o), col, th);
    dl->AddLine(ImVec2(a.x - o, a.y - o), ImVec2(a.x - o, a.y + len), col, th);
    dl->AddLine(ImVec2(b.x + o, a.y - o), ImVec2(b.x - len, a.y - o), col, th);
    dl->AddLine(ImVec2(b.x + o, a.y - o), ImVec2(b.x + o, a.y + len), col, th);
    dl->AddLine(ImVec2(a.x - o, b.y + o), ImVec2(a.x + len, b.y + o), col, th);
    dl->AddLine(ImVec2(a.x - o, b.y + o), ImVec2(a.x - o, b.y - len), col, th);
    dl->AddLine(ImVec2(b.x + o, b.y + o), ImVec2(b.x - len, b.y + o), col, th);
    dl->AddLine(ImVec2(b.x + o, b.y + o), ImVec2(b.x + o, b.y - len), col, th);

    const float chipH = 13.0f * scale;
    const float padX = 5.0f * scale;
    const float tw = TextWidth(label, 9.0f * scale);
    ImVec2 ca(a.x, a.y - chipH - 3.0f * scale);
    ImVec2 cb(a.x + tw + padX * 2.0f, ca.y + chipH);
    if (cb.x > b.x) cb.x = b.x;
    dl->AddRectFilled(ca, cb, IM_COL32(5, 11, 19, 220), 2.0f * scale);
    dl->AddRect(ca, cb, WithAlpha(col, 170), 2.0f * scale, 0, 1.0f);
    dl->AddText(ImGui::GetFont(), 9.0f * scale,
                ImVec2(ca.x + padX, ca.y + (chipH - 9.0f * scale) * 0.5f), col, label);

    // Resize grip (bottom-right).
    const float g = 9.0f * scale;
    for (int i = 0; i < 3; ++i) {
        const float d = (float)i * 3.0f * scale;
        dl->AddLine(ImVec2(b.x - g + d, b.y - 2.0f * scale),
                    ImVec2(b.x - 2.0f * scale, b.y - g + d),
                    WithAlpha(col, 150 - i * 38), 1.0f);
    }
}

// ── Watermark / header ──────────────────────────────────────────────────────
struct Telemetry {
    int pingMs;
    int fps;
    Telemetry() : pingMs(-1), fps(0) {}
};

struct HeaderMetrics {
    float width;
    float height;
};

inline float HeaderHeight(float scale) {
    return ImGui::GetFontSize() * scale * 2.08f + 10.0f * scale;
}

inline HeaderMetrics MeasureHeader(float scale) {
    const float fs = ImGui::GetFontSize() * scale;
    const float leftW = (std::max)(78.0f * scale, fs * 4.4f);
    const float rightW = (std::max)(74.0f * scale, fs * 4.6f);
    const float centerW = (std::max)(74.0f * scale,
        TrackedWidth("aoko", fs * 1.08f, 2.7f * scale) + fs * 1.15f);
    HeaderMetrics m;
    m.width = leftW + centerW + rightW + 10.0f * scale;
    m.height = HeaderHeight(scale);
    return m;
}

// Draws the watermark. Returns its height. `x1` is the right edge.
inline float DrawHeader(ImDrawList* dl, const Theme& th, float x1, float y,
                        float width, float scale, const Telemetry& tm,
                        float time, float alpha) {
    const float h = HeaderHeight(scale);
    const float fs = ImGui::GetFontSize() * scale;
    const float micro = std::floor(fs * 0.60f);
    const float valueSize = std::floor(fs * 0.94f);
    const float unitSize = std::floor(fs * 0.78f);
    const float pad = 7.0f * scale;
    const float a = Clamp01(alpha);
    if (a <= 0.0f) return h * a;

    ImVec2 p0(x1 - width, y);
    ImVec2 p1(x1, y + h);

    // Ambient under-glow gives the bar its own light without looking like a row.
    for (int i = 3; i >= 1; --i) {
        const float e = (float)i * 1.6f;
        const int ga = (int)(a * (7.0f + (float)(3 - i) * 4.0f));
        dl->AddRectFilledMultiColor(
            ImVec2(p0.x + e, p1.y + 1.0f),
            ImVec2(p1.x - e, p1.y + e + 1.0f),
            WithAlpha(th.accent, 0), WithAlpha(th.accent, 0),
            WithAlpha(th.accent, ga), WithAlpha(th.accent, ga));
    }

    const ImU32 panel = ScaleAlpha(th.panel, a);
    const ImU32 border = ScaleAlpha(Mix(th.border, th.accent, 0.22f), a);
    dl->AddRectFilled(p0, p1, panel, 6.0f * scale);
    dl->AddRectFilledMultiColor(
        ImVec2(p0.x + 1.0f, p0.y + 1.0f), ImVec2(p1.x - 1.0f, p1.y - 1.0f),
        ScaleAlpha(th.panelTop, a), ScaleAlpha(th.panelTop, a),
        ScaleAlpha(Mix(th.panel, th.accent, 0.05f), a),
        ScaleAlpha(Mix(th.panel, th.accent, 0.05f), a));
    dl->AddRect(p0, p1, border, 6.0f * scale, 0, 1.0f);
    dl->AddLine(ImVec2(p0.x + 6.0f * scale, p0.y + 0.5f),
                ImVec2(p1.x - 6.0f * scale, p0.y + 0.5f),
                ScaleAlpha(th.highlight, a), 1.0f);

    const float leftW = (std::max)(78.0f * scale, fs * 4.4f);
    const float rightW = (std::max)(74.0f * scale, fs * 4.6f);
    const float sep1 = p0.x + leftW + 1.0f * scale;
    const float sep2 = p1.x - rightW - 1.0f * scale;
    const ImU32 sepCol = ScaleAlpha(WithAlpha(th.text, 34), a);
    VerticalRule(dl, sep1, p0.y + 6.0f * scale, p1.y - 6.0f * scale, sepCol);
    VerticalRule(dl, sep2, p0.y + 6.0f * scale, p1.y - 6.0f * scale, sepCol);
    Diamond(dl, ImVec2(sep1, p0.y + h * 0.5f), 1.6f * scale, ScaleAlpha(th.accent, a * 0.52f));
    Diamond(dl, ImVec2(sep2, p0.y + h * 0.5f), 1.6f * scale, ScaleAlpha(th.accent, a * 0.52f));

    const float microY = p0.y + 5.5f * scale;
    const float valueY = microY + micro + 0.8f * scale;

    // ── Left: network latency ──────────────────────────────────────────────
    TrackedText(dl, ImVec2(p0.x + pad, microY), "LATENCY", micro,
                ScaleAlpha(th.textFaint, a), 1.1f * scale);
    const ImU32 pingCol = ScaleAlpha(PingColor(tm.pingMs), a);
    const float barsW = 9.0f * scale;
    const float barsH = fs * 0.62f;
    const float barsY = valueY + (fs * 0.88f - barsH) * 0.5f;
    int strength = 4;
    if (tm.pingMs < 0) strength = 0;
    else if (tm.pingMs >= 160) strength = 1;
    else if (tm.pingMs >= 110) strength = 2;
    else if (tm.pingMs >= 60) strength = 3;
    SignalBars(dl, ImVec2(p0.x + pad, barsY), barsW, barsH, strength, pingCol);

    char pingBuf[24];
    if (tm.pingMs >= 0) {
        const int shownPing = tm.pingMs > 999 ? 999 : tm.pingMs;
        snprintf(pingBuf, sizeof(pingBuf), "%d", shownPing);
    } else {
        snprintf(pingBuf, sizeof(pingBuf), "--");
    }
    const float pingX = p0.x + pad + barsW + 4.0f * scale;
    Text(dl, ImVec2(pingX, valueY), pingBuf, valueSize, pingCol);
    const float pingW = TextWidth(pingBuf, valueSize);
    Text(dl, ImVec2(pingX + pingW + 2.0f * scale, valueY + (valueSize - unitSize)), "ms",
         unitSize, ScaleAlpha(th.textDim, a));

    // ── Right: frames per second ───────────────────────────────────────────
    TrackedText(dl, ImVec2(p1.x - pad - TrackedWidth("FRAMES", micro, 1.1f * scale), microY),
                "FRAMES", micro, ScaleAlpha(th.textFaint, a), 1.1f * scale);
    char fpsBuf[24];
    snprintf(fpsBuf, sizeof(fpsBuf), "%d", tm.fps > 0 ? tm.fps : 0);
    const ImU32 fpsCol = ScaleAlpha(FpsColor(tm.fps), a);
    const float fpsUnitW = TextWidth("fps", unitSize);
    const float fpsRight = p1.x - pad;
    TextRight(dl, ImVec2(fpsRight, valueY + (valueSize - unitSize)), "fps",
              unitSize, ScaleAlpha(th.textDim, a));
    TextRight(dl, ImVec2(fpsRight - fpsUnitW - 2.0f * scale, valueY), fpsBuf,
              valueSize, fpsCol);

    // ── Center: the mark ───────────────────────────────────────────────────
    const float cx = (sep1 + sep2) * 0.5f;
    const float brandSize = std::floor(fs * 1.06f);
    const float brandW = TrackedWidth("aoko", brandSize, 2.7f * scale);
    const float microW = TrackedWidth("CLIENT", micro, 2.3f * scale);
    const float starR = fs * 0.30f;
    const float blockW = brandW + starR * 2.0f + 4.0f * scale;
    const float blockH = brandSize + micro + 1.0f * scale;
    const float blockY = p0.y + (h - blockH) * 0.5f;
    const float brandX = cx - blockW * 0.5f + starR * 2.0f + 4.0f * scale;

    const float pulse = 0.72f + 0.28f * (0.5f + 0.5f * std::sin(time * 2.1f));
    Spark(dl, ImVec2(cx - blockW * 0.5f + starR, blockY + brandSize * 0.52f),
          starR, ScaleAlpha(th.accent, a * pulse));

    // Letter-by-letter accent→starlight wash.
    float lx = brandX;
    const char* brand = "aoko";
    const int n = 4;
    for (int i = 0; i < n; ++i) {
        char glyph[2] = { brand[i], 0 };
        const ImU32 c = ScaleAlpha(Mix(th.accent, th.text, 0.25f + 0.75f * (float)i / 3.0f), a);
        dl->AddText(ImGui::GetFont(), brandSize, ImVec2(lx, blockY), c, glyph);
        lx += TextWidth(glyph, brandSize) + 2.7f * scale;
    }
    TrackedText(dl, ImVec2(cx - microW * 0.5f, blockY + brandSize + 1.0f * scale),
                "CLIENT", micro, ScaleAlpha(th.textFaint, a), 2.3f * scale);

    // Slow light sweep across the lower hairline.
    const ImU32 baseLine = ScaleAlpha(WithAlpha(th.text, 26), a);
    dl->AddLine(ImVec2(p0.x + 6.0f * scale, p1.y - 0.5f),
                ImVec2(p1.x - 6.0f * scale, p1.y - 0.5f), baseLine, 1.0f);
    const float sweepW = width * 0.34f;
    const float sweepX = p0.x - sweepW + std::fmod(time * width * 0.16f, width + sweepW);
    const float sxa = (std::max)(p0.x + 4.0f * scale, sweepX);
    const float sxb = (std::min)(p1.x - 4.0f * scale, sweepX + sweepW);
    if (sxb > sxa) {
        dl->AddLine(ImVec2(sxa, p1.y - 0.5f), ImVec2(sxb, p1.y - 0.5f),
                    ScaleAlpha(th.accent, a * 0.60f), 1.0f);
        dl->AddLine(ImVec2(sxa, p1.y - 1.5f), ImVec2(sxb, p1.y - 1.5f),
                    ScaleAlpha(th.accent, a * 0.12f), 2.0f);
    }
    return h;
}

// ── Module list ─────────────────────────────────────────────────────────────
struct ModuleEntry {
    std::string name;
    std::string tag;
    ImU32 accent;
    float nameW;
    float tagW;
    float sortW;
    ModuleEntry() : accent(0), nameW(0.0f), tagW(0.0f), sortW(0.0f) {}
    ModuleEntry(const std::string& n, const std::string& t, ImU32 a)
        : name(n), tag(t), accent(a), nameW(0.0f), tagW(0.0f), sortW(0.0f) {}
};

struct ModuleListRow {
    std::string name;
    std::string tag;
    ImU32 accent;
    float nameW;
    float tagW;
    float presence;
    bool active;
    ModuleListRow()
        : accent(0), nameW(0.0f), tagW(0.0f), presence(0.0f), active(false) {}
};

inline std::string LowerAscii(const std::string& s) {
    std::string out = s;
    for (size_t i = 0; i < out.size(); ++i)
        out[i] = (char)std::tolower((unsigned char)out[i]);
    return out;
}

class ModuleList {
public:
    ModuleList() : scale_(1.0f), style_(0), showHeader_(true), headerPresence_(0.0f),
                   targetW_(0.0f), targetH_(0.0f) {}

    void Reset() {
        rows_.clear();
        visible_.clear();
        headerPresence_ = 0.0f;
        targetW_ = targetH_ = 0.0f;
    }

    // Updates presence/size state and returns the current target bounds.
    ImVec2 Update(const std::vector<ModuleEntry>& entries, float scale, bool showHeader,
                  int style, float dt) {
        if (scale < 0.1f) scale = 0.1f;
        if (dt <= 0.0f) dt = 1.0f / 60.0f;
        scale_ = scale;
        style_ = style < 0 ? 0 : (style > 6 ? 6 : style);
        showHeader_ = showHeader;

        for (std::map<std::string, ModuleListRow>::iterator it = rows_.begin(); it != rows_.end(); ++it)
            it->second.active = false;

        for (size_t i = 0; i < entries.size(); ++i) {
            const ModuleEntry& e = entries[i];
            if (e.name.empty()) continue;
            ModuleListRow& r = rows_[e.name];
            r.name = e.name;
            r.tag = e.tag;
            r.accent = e.accent;
            r.active = true;
            r.nameW = e.nameW > 0.0f ? e.nameW : ImGui::CalcTextSize(e.name.c_str()).x;
            r.tagW = e.tag.empty() ? 0.0f
                : (e.tagW > 0.0f ? e.tagW : ImGui::CalcTextSize(e.tag.c_str()).x);
        }

        for (std::map<std::string, ModuleListRow>::iterator it = rows_.begin(); it != rows_.end();) {
            ModuleListRow& r = it->second;
            r.presence = Approach(r.presence, r.active ? 1.0f : 0.0f,
                                  r.active ? 17.0f : 11.0f, dt);
            if (!r.active && r.presence < 0.006f) {
                it = rows_.erase(it);
            } else {
                ++it;
            }
        }

        headerPresence_ = Approach(headerPresence_, showHeader ? 1.0f : 0.0f, 14.0f, dt);

        RefreshVisible();
        const std::vector<ModuleListRow*>& visible = visible_;
        float maxContent = HeaderWidth(scale);
        for (size_t i = 0; i < visible.size(); ++i)
            maxContent = (std::max)(maxContent, RowWidth(*visible[i], scale));
        if (!showHeader && headerPresence_ < 0.001f) {
            maxContent = 0.0f;
            for (size_t i = 0; i < visible.size(); ++i)
                maxContent = (std::max)(maxContent, RowWidth(*visible[i], scale));
        }

        const float rightInset = RightInset(scale);
        targetW_ = maxContent > 0.0f ? maxContent + rightInset : 0.0f;

        float h = 0.0f;
        if (headerPresence_ > 0.001f) {
            // Keep the full header height while it fades. Rows never overlap the
            // panel during the show/hide transition.
            h += HeaderHeight(scale);
            if (!visible.empty()) h += RowGap(scale);
        }
        const float rowH = RowHeight(scale);
        const float gap = RowGap(scale);
        for (size_t i = 0; i < visible.size(); ++i) {
            h += rowH * visible[i]->presence;
            if (i + 1 < visible.size()) h += gap * visible[i]->presence;
        }
        targetH_ = targetW_ > 0.0f ? h : 0.0f;
        return ImVec2(targetW_, targetH_);
    }

    void Draw(ImDrawList* dl, ImVec2 topLeft, const Theme& th, const Telemetry& tm,
              float time) {
        if (targetW_ <= 0.0f || targetH_ <= 0.0f) return;

        const float rightInset = RightInset(scale_);
        const float x1 = topLeft.x + targetW_ - rightInset;
        float y = topLeft.y;

        const std::vector<ModuleListRow*>& visible = visible_;

        if (headerPresence_ > 0.001f) {
            const float slide = (1.0f - headerPresence_) * 4.0f * scale_;
            DrawHeader(dl, th, x1, y + slide, targetW_ - rightInset, scale_, tm, time,
                       headerPresence_);
            y += HeaderHeight(scale_);
            if (!visible.empty()) y += RowGap(scale_);
        }

        const float rowH = RowHeight(scale_);
        const float gap = RowGap(scale_);

        for (size_t i = 0; i < visible.size(); ++i) {
            ModuleListRow& r = *visible[i];
            const float p = Clamp01(r.presence);
            const float h = rowH * p;
            if (h < 0.5f) continue;
            const float w = RowWidth(r, scale_);
            const float slide = (1.0f - p) * 5.0f * scale_;
            const float x0 = x1 - w + slide;
            const float y0 = y;
            const float y1 = y + h;

            dl->PushClipRect(ImVec2(x0 - 1.0f, y0), ImVec2(x1 + 1.0f, y1), true);
            DrawRow(dl, th, r, x0, y0, x1, y1, scale_, p, time);
            dl->PopClipRect();

            y += h;
            if (i + 1 < visible.size()) y += gap * p;
        }
    }

    float Width() const { return targetW_; }
    float Height() const { return targetH_; }

private:
    std::map<std::string, ModuleListRow> rows_;
    std::vector<ModuleListRow*> visible_;
    float scale_;
    int style_;
    bool showHeader_;
    float headerPresence_;
    float targetW_;
    float targetH_;

    static float RowHeight(float scale) {
        return ImGui::GetFontSize() * scale + 10.0f * scale;
    }
    static float RowGap(float scale) { return 2.5f * scale; }
    static float RightInset(float scale) { return 6.0f * scale; }

    float HeaderWidth(float scale) const {
        return MeasureHeader(scale).width;
    }

    float RowWidth(const ModuleListRow& r, float scale) const {
        const float fs = ImGui::GetFontSize() * scale;
        const float nameW = r.nameW * scale;
        const float tagW = r.tag.empty() ? 0.0f
            : (r.tagW * scale * 0.82f + 8.0f * scale);
        const float padL = (style_ == 1 || style_ == 5) ? 6.0f * scale : 10.0f * scale;
        const float padR = (style_ == 1 || style_ == 5) ? 6.0f * scale : 8.0f * scale;
        const float rail = (style_ == 0 || style_ == 2 || style_ == 3 || style_ == 4)
            ? 5.0f * scale : 0.0f;
        (void)fs;
        return padL + rail + nameW + (r.tag.empty() ? 0.0f : 6.0f * scale + tagW) + padR;
    }

    void RefreshVisible() {
        visible_.clear();
        for (std::map<std::string, ModuleListRow>::iterator it = rows_.begin(); it != rows_.end(); ++it) {
            if (it->second.presence > 0.005f) visible_.push_back(&it->second);
        }
        std::sort(visible_.begin(), visible_.end(),
                  [](const ModuleListRow* a, const ModuleListRow* b) {
                      const float aw = a->nameW + (a->tag.empty() ? 0.0f : a->tagW * 0.82f + 14.0f);
                      const float bw = b->nameW + (b->tag.empty() ? 0.0f : b->tagW * 0.82f + 14.0f);
                      if (aw != bw) return aw > bw;
                      return a->name < b->name;
                  });
    }

    void DrawRow(ImDrawList* dl, const Theme& th, const ModuleListRow& r,
                 float x0, float y0, float x1, float y1, float scale,
                 float alpha, float time) {
        const float fs = ImGui::GetFontSize() * scale;
        const float p = Clamp01(alpha);
        const float rounding = 4.0f * scale;
        const ImU32 accent = r.accent ? r.accent : th.accent;
        const ImU32 nameCol = ScaleAlpha(th.text, p);
        const ImU32 tagCol = ScaleAlpha(Mix(accent, th.text, 0.20f), p * 0.92f);

        const float padL = (style_ == 1 || style_ == 5) ? 6.0f * scale : 10.0f * scale;
        const float padR = (style_ == 1 || style_ == 5) ? 6.0f * scale : 8.0f * scale;
        const float railW = 2.0f * scale;
        const float railInset = 3.0f * scale;
        const float textY = y0 + (y1 - y0 - fs) * 0.5f;

        if (style_ == 0) {
            // Signature Aoko glass: quiet fill, top sheen, inset lunar rail, tag chip.
            dl->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1),
                              ScaleAlpha(Mix(th.panel, accent, 0.045f), p * 0.97f), rounding);
            dl->AddRectFilledMultiColor(
                ImVec2(x0 + 1.0f, y0 + 1.0f), ImVec2(x1 - 1.0f, y0 + (y1 - y0) * 0.62f),
                ScaleAlpha(th.panelTop, p * 0.58f), ScaleAlpha(th.panelTop, p * 0.58f),
                ScaleAlpha(th.panel, 0.0f), ScaleAlpha(th.panel, 0.0f));
            dl->AddRect(ImVec2(x0, y0), ImVec2(x1, y1), ScaleAlpha(th.border, p), rounding, 0, 1.0f);
            dl->AddLine(ImVec2(x0 + rounding, y0 + 0.5f), ImVec2(x1 - rounding, y0 + 0.5f),
                        ScaleAlpha(th.highlight, p), 1.0f);
            dl->AddRectFilledMultiColor(
                ImVec2(x0 + railInset, y0 + railInset), ImVec2(x0 + railInset + railW, y1 - railInset),
                ScaleAlpha(accent, p * 0.95f), ScaleAlpha(th.accent2, p * 0.75f),
                ScaleAlpha(th.accent2, p * 0.35f), ScaleAlpha(accent, p * 0.55f));
        } else if (style_ == 1) {
            // Minimal: no chrome, a single lunar tick and air.
            dl->AddRectFilledMultiColor(
                ImVec2(x0, y0 + 1.0f), ImVec2(x1, y1 - 1.0f),
                ScaleAlpha(th.panel, p * 0.42f), ScaleAlpha(th.panel, 0.0f),
                ScaleAlpha(th.panel, 0.0f), ScaleAlpha(th.panel, p * 0.16f));
            dl->AddRectFilled(ImVec2(x1 - 2.0f * scale, y0 + (y1 - y0) * 0.22f),
                              ImVec2(x1, y1 - (y1 - y0) * 0.22f),
                              ScaleAlpha(accent, p * 0.90f), 1.0f * scale);
        } else if (style_ == 2) {
            // Outlined: transparent with a precise accent hairline.
            dl->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1),
                              ScaleAlpha(th.panel, p * 0.28f), rounding);
            dl->AddRect(ImVec2(x0, y0), ImVec2(x1, y1),
                        ScaleAlpha(Mix(accent, th.border, 0.35f), p), rounding, 0, 1.2f);
            dl->AddRectFilledMultiColor(
                ImVec2(x0 + railInset, y0 + railInset), ImVec2(x0 + railInset + railW, y1 - railInset),
                ScaleAlpha(accent, p), ScaleAlpha(th.accent2, p * 0.45f),
                ScaleAlpha(th.accent2, p * 0.25f), ScaleAlpha(accent, p * 0.45f));
        } else if (style_ == 3) {
            // Glass: doubled surface, specular rim, cool inner light.
            dl->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1),
                              ScaleAlpha(Mix(th.panel, th.accent3, 0.03f), p * 0.88f), rounding);
            dl->AddRectFilledMultiColor(
                ImVec2(x0 + 1.0f, y0 + 1.0f), ImVec2(x1 - 1.0f, y1 - 1.0f),
                ScaleAlpha(th.panelTop, p * 0.38f), ScaleAlpha(th.panelTop, p * 0.10f),
                ScaleAlpha(th.panel, 0.0f), ScaleAlpha(th.panel, p * 0.16f));
            dl->AddRect(ImVec2(x0, y0), ImVec2(x1, y1),
                        ScaleAlpha(Mix(th.border, th.accent3, 0.25f), p), rounding, 0, 1.0f);
            dl->AddLine(ImVec2(x0 + rounding, y0 + 0.5f), ImVec2(x1 - rounding, y0 + 0.5f),
                        ScaleAlpha(th.highlight, p), 1.0f);
            dl->AddRectFilled(ImVec2(x0 + railInset, y0 + railInset),
                              ImVec2(x0 + railInset + railW, y1 - railInset),
                              ScaleAlpha(accent, p * 0.95f), 1.0f);
        } else if (style_ == 4) {
            // Bold: accent-washed surface, high-contrast type.
            dl->AddRectFilledMultiColor(
                ImVec2(x0, y0), ImVec2(x1, y1),
                ScaleAlpha(Mix(th.panel, accent, 0.24f), p * 0.94f),
                ScaleAlpha(Mix(th.panel, th.accent2, 0.10f), p * 0.94f),
                ScaleAlpha(Mix(th.panel, accent, 0.08f), p * 0.94f),
                ScaleAlpha(Mix(th.panel, accent, 0.16f), p * 0.94f));
            dl->AddRect(ImVec2(x0, y0), ImVec2(x1, y1),
                        ScaleAlpha(Mix(accent, th.text, 0.18f), p * 0.85f), rounding, 0, 1.0f);
            dl->AddRectFilled(ImVec2(x0, y0 + railInset), ImVec2(x0 + 3.0f * scale, y1 - railInset),
                              ScaleAlpha(accent, p), 1.0f * scale);
        } else if (style_ == 5) {
            // Classic: flat shade, right-hand light, accent name.
            dl->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1),
                              ScaleAlpha(Mix(th.panel, IM_COL32(0, 0, 0, 255), 0.28f), p * 0.96f), 2.0f * scale);
            dl->AddRectFilled(ImVec2(x1 - 2.0f * scale, y0), ImVec2(x1, y1),
                              ScaleAlpha(accent, p * 0.95f));
            dl->AddLine(ImVec2(x0, y0 + 0.5f), ImVec2(x1, y0 + 0.5f),
                        ScaleAlpha(th.highlight, p * 0.5f), 1.0f);
        } else {
            // Modern: lowercase pill, side light, quiet tag.
            dl->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1),
                              ScaleAlpha(Mix(th.panel, th.accent3, 0.04f), p * 0.66f), 3.0f * scale);
            dl->AddRect(ImVec2(x0, y0), ImVec2(x1, y1), ScaleAlpha(th.border, p * 0.75f),
                        3.0f * scale, 0, 1.0f);
            dl->AddRectFilled(ImVec2(x1 - 2.0f * scale, y0), ImVec2(x1, y1),
                              ScaleAlpha(accent, p * 0.90f), 1.0f);
        }

        std::string lowerName;
        const std::string* displayName = &r.name;
        if (style_ == 6) {
            lowerName = LowerAscii(r.name);
            displayName = &lowerName;
        }
        ImU32 displayColor = nameCol;
        if (style_ == 1 || style_ == 5 || style_ == 6)
            displayColor = ScaleAlpha(Mix(accent, th.text, 0.18f), p);
        if (style_ == 4) displayColor = ScaleAlpha(Mix(th.text, accent, 0.12f), p);

        const float textX = x0 + padL + ((style_ == 0 || style_ == 2 || style_ == 3 || style_ == 4)
            ? 5.0f * scale : 0.0f);
        TextShadowed(dl, ImVec2(textX, textY), displayName->c_str(), fs, displayColor,
                     ScaleAlpha(th.shadow, p * 0.55f), (std::max)(1.0f, scale));

        if (!r.tag.empty()) {
            const float tagSize = std::floor(fs * 0.82f);
            const float chipW = r.tagW * scale * 0.84f + 7.0f * scale;
            const float chipH = fs * 0.96f;
            const float cx1 = x1 - padR;
            const float cx0 = cx1 - chipW;
            const float cy0 = y0 + (y1 - y0 - chipH) * 0.5f;
            if (style_ != 1 && style_ != 5) {
                dl->AddRectFilled(ImVec2(cx0, cy0), ImVec2(cx1, cy0 + chipH),
                                  ScaleAlpha(accent, p * 0.11f), 2.0f * scale);
            }
            const float tagY = cy0 + (chipH - tagSize) * 0.5f;
            TextRight(dl, ImVec2(cx1 - 3.5f * scale, tagY), r.tag.c_str(), tagSize, tagCol);
        }

        // Rising spark when a module first appears.
        if (p < 0.98f) {
            const float flash = 1.0f - p;
            const float sx = x1 - 3.0f * scale;
            const float sy = y0 + (y1 - y0) * 0.5f;
            Spark(dl, ImVec2(sx, sy), (2.0f + 4.0f * flash) * scale,
                  ScaleAlpha(Mix(accent, th.text, 0.2f), flash * 0.55f));
        }
        (void)time;
    }
};


// ── Beta world overlays (Trajectories, Murder Mystery, Break Progress) ──────
// Shared by both bridges. project(x, y, z, &sx, &sy) maps a world point to
// screen pixels and returns false when it is behind the camera.

struct WorldPoint { double x, y, z; };

template <typename PointVec, typename Point, typename ProjectFn>
inline void DrawTrajectory(ImDrawList* dl, const Theme& th, const PointVec& points,
                           bool hitBlock, const Point& impact, ProjectFn project) {
    if (!dl || points.size() < 2) return;
    bool havePrev = false;
    float prevX = 0.0f, prevY = 0.0f;
    const size_t n = points.size();
    for (size_t i = 0; i < n; ++i) {
        float sx = 0.0f, sy = 0.0f;
        const bool ok = project(points[i].x, points[i].y, points[i].z, &sx, &sy);
        if (ok && havePrev) {
            const float t = (float)i / (float)(n - 1);
            dl->AddLine(ImVec2(prevX, prevY), ImVec2(sx, sy), WithAlpha(th.AccentAt(t), 220), 2.0f);
        }
        havePrev = ok;
        prevX = sx;
        prevY = sy;
    }
    float ix = 0.0f, iy = 0.0f;
    if (project(impact.x, impact.y, impact.z, &ix, &iy)) {
        const ImU32 col = hitBlock ? SemanticGood() : SemanticWarn();
        dl->AddCircle(ImVec2(ix, iy), 7.0f, WithAlpha(col, 230), 20, 2.0f);
        Diamond(dl, ImVec2(ix, iy), 3.0f, col);
    }
}

struct MurderTag {
    std::string name;
    double x, y, z;
    float dist;
    int role; // 1 = bow holder, 2 = murderer (lc::beta::MurderRole)
};

// Role tags above players plus a murderer alert near the top of the screen.
template <typename ProjectFn>
inline void DrawMurderTags(ImDrawList* dl, int screenW, const std::vector<MurderTag>& tags,
                           ProjectFn project) {
    if (!dl) return;
    const ImU32 murdererCol = IM_COL32(238, 92, 92, 255);
    const ImU32 bowCol = IM_COL32(104, 156, 246, 255);
    std::string alert;
    for (size_t i = 0; i < tags.size(); ++i) {
        const MurderTag& r = tags[i];
        const bool murderer = r.role == 2;
        if (murderer) {
            char buf[96];
            snprintf(buf, sizeof(buf), "%s%s (%.0fm)", alert.empty() ? "" : ", ", r.name.c_str(), r.dist);
            alert += buf;
        }
        float sx = 0.0f, sy = 0.0f;
        if (!project(r.x, r.y + 2.45, r.z, &sx, &sy)) continue;
        const char* label = murderer ? "MURDERER" : "BOW";
        const ImU32 col = murderer ? murdererCol : bowCol;
        const ImVec2 ts = ImGui::CalcTextSize(label);
        const ImVec2 a(sx - ts.x * 0.5f - 6.0f, sy - ts.y * 0.5f - 3.0f);
        const ImVec2 b(sx + ts.x * 0.5f + 6.0f, sy + ts.y * 0.5f + 3.0f);
        dl->AddRectFilled(a, b, IM_COL32(10, 15, 24, 215), 4.0f);
        dl->AddRect(a, b, WithAlpha(col, 200), 4.0f);
        dl->AddText(ImVec2(sx - ts.x * 0.5f, sy - ts.y * 0.5f), col, label);
    }
    if (!alert.empty()) {
        const std::string text = "Murderer: " + alert;
        const ImVec2 ts = ImGui::CalcTextSize(text.c_str());
        const float x = (float)screenW * 0.5f - ts.x * 0.5f;
        const float y = 64.0f;
        const ImVec2 a(x - 10.0f, y - 5.0f), b(x + ts.x + 10.0f, y + ts.y + 5.0f);
        dl->AddRectFilled(a, b, IM_COL32(24, 8, 10, 220), 5.0f);
        dl->AddRect(a, b, WithAlpha(murdererCol, 210), 5.0f);
        dl->AddText(ImVec2(x, y), murdererCol, text.c_str());
    }
}

struct BreakTag {
    double x, y, z; // block center
    int percent;
    bool self;
    std::string breaker;
};

template <typename ProjectFn>
inline void DrawBreakTags(ImDrawList* dl, const std::vector<BreakTag>& tags, ProjectFn project) {
    if (!dl) return;
    for (size_t i = 0; i < tags.size(); ++i) {
        const BreakTag& r = tags[i];
        float sx = 0.0f, sy = 0.0f;
        if (!project(r.x, r.y, r.z, &sx, &sy)) continue;
        char pct[16];
        snprintf(pct, sizeof(pct), "%d%%", r.percent);
        const float t = Clamp01((float)r.percent / 100.0f);
        // Other players' progress shifts from amber to red; own mining stays neutral.
        const ImU32 col = r.self ? IM_COL32(226, 234, 242, 250) : Mix(SemanticWarn(), SemanticBad(), t);
        const float small = ImGui::GetFontSize() * 0.85f;
        const ImVec2 ts = ImGui::CalcTextSize(pct);
        const float nameW = r.breaker.empty() ? 0.0f : TextWidth(r.breaker.c_str(), small);
        const float wBox = (std::max)(ts.x, nameW) + 12.0f;
        const float hBox = ts.y + (r.breaker.empty() ? 0.0f : small + 2.0f) + 8.0f;
        const ImVec2 a(sx - wBox * 0.5f, sy - hBox * 0.5f);
        const ImVec2 b(sx + wBox * 0.5f, sy + hBox * 0.5f);
        dl->AddRectFilled(a, b, IM_COL32(10, 15, 24, 210), 4.0f);
        dl->AddRect(a, b, WithAlpha(col, 170), 4.0f);
        dl->AddRectFilled(ImVec2(a.x + 2.0f, b.y - 3.0f),
                          ImVec2(a.x + 2.0f + (wBox - 4.0f) * t, b.y - 1.0f), col, 1.0f);
        dl->AddText(ImVec2(sx - ts.x * 0.5f, a.y + 3.0f), col, pct);
        if (!r.breaker.empty())
            TextCentered(dl, sx, a.y + 3.0f + ts.y + 1.0f, r.breaker.c_str(), small, IM_COL32(190, 202, 214, 230));
    }
}

} // namespace ui
} // namespace aoko
