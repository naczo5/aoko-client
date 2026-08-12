#pragma once

// BedPlates icon helpers: Minecraft jar texture path candidates, procedural
// fallback icons, and an ImGui ImTextureData cache (works with OpenGL + Vulkan
// backends via RegisterUserTexture / RendererHasTextures).

#include "bedplates_common.h"

#include "imgui.h"
#include "imgui_internal.h"

#include <jni.h>
#include <algorithm>
#include <map>
#include <string>
#include <vector>
#include <cstring>

namespace lc {

inline unsigned int BedPlateIconColorForId(const std::string& rawId) {
    std::string id = BlockEspNormalizeId(rawId);
    // Packed as IM_COL32(r,g,b,a) = r | g<<8 | b<<16 | a<<24
    auto col = [](int r, int g, int b) -> unsigned int {
        return (unsigned int)(r | (g << 8) | (b << 16) | (0xFFu << 24));
    };
    if (id.find("obsidian") != std::string::npos) return col(20, 18, 29);
    if (id.find("end_stone") != std::string::npos || id == "whitestone") return col(219, 222, 158);
    if (id.find("glass") != std::string::npos) return col(160, 200, 220);
    if (id.find("iron") != std::string::npos) return col(180, 180, 185);
    if (id.find("wool") != std::string::npos || id == "cloth") {
        if (id.find("red") != std::string::npos) return col(160, 39, 34);
        if (id.find("blue") != std::string::npos) return col(37, 49, 146);
        if (id.find("green") != std::string::npos) return col(55, 86, 25);
        if (id.find("yellow") != std::string::npos) return col(186, 174, 42);
        if (id.find("black") != std::string::npos) return col(20, 21, 25);
        if (id.find("orange") != std::string::npos) return col(210, 118, 42);
        if (id.find("pink") != std::string::npos) return col(210, 128, 150);
        return col(234, 234, 234);
    }
    if (id.find("terracotta") != std::string::npos || id.find("hardenedclay") != std::string::npos
        || id.find("clayhardened") != std::string::npos || id.find("stainedclay") != std::string::npos) {
        if (id.find("white") != std::string::npos) return col(210, 178, 161);
        if (id.find("black") != std::string::npos) return col(37, 23, 16);
        if (id.find("orange") != std::string::npos) return col(162, 84, 38);
        return col(152, 94, 67);
    }
    if (id.find("plank") != std::string::npos || id.find("wood") != std::string::npos
        || id.find("log") != std::string::npos) return col(162, 130, 78);
    if (id.find("gravel") != std::string::npos) return col(136, 126, 126);
    if (id.find("dirt") != std::string::npos) return col(134, 96, 67);
    if (id.find("sand") != std::string::npos) return col(219, 207, 163);
    if (id.find("stone") != std::string::npos || id.find("cobble") != std::string::npos) return col(125, 125, 125);
    if (id.find("slab") != std::string::npos || id.find("stair") != std::string::npos) return col(140, 110, 70);
    if (id.find("water") != std::string::npos) return col(40, 80, 180);
    if (id.find("lava") != std::string::npos) return col(200, 80, 20);
    return col(120, 120, 120);
}

// Fill a 16x16 RGBA buffer with a simple Minecraft-like top-face block icon.
inline void BedPlateFillProceduralIcon(const std::string& id, unsigned char* rgba16) {
    unsigned int c = BedPlateIconColorForId(id);
    int r = (int)(c & 0xFF);
    int g = (int)((c >> 8) & 0xFF);
    int b = (int)((c >> 16) & 0xFF);
    for (int y = 0; y < 16; y++) {
        for (int x = 0; x < 16; x++) {
            int idx = (y * 16 + x) * 4;
            // Border darkening + slight checker noise so icons aren't flat squares.
            bool border = (x == 0 || y == 0 || x == 15 || y == 15);
            bool corner = (x < 2 || y < 2);
            int noise = ((x * 3 + y * 7) & 3) - 1;
            int rr = r + noise * 4;
            int gg = g + noise * 4;
            int bb = b + noise * 4;
            if (corner) { rr = (rr * 85) / 100; gg = (gg * 85) / 100; bb = (bb * 85) / 100; }
            if (border) { rr = (rr * 70) / 100; gg = (gg * 70) / 100; bb = (bb * 70) / 100; }
            if (rr < 0) rr = 0; if (rr > 255) rr = 255;
            if (gg < 0) gg = 0; if (gg > 255) gg = 255;
            if (bb < 0) bb = 0; if (bb > 255) bb = 255;
            rgba16[idx + 0] = (unsigned char)rr;
            rgba16[idx + 1] = (unsigned char)gg;
            rgba16[idx + 2] = (unsigned char)bb;
            rgba16[idx + 3] = 255;
        }
    }
}

// Candidate jar-relative texture paths for a normalized block id (modern + 1.8.9).
inline void BedPlateTexturePathCandidates(const std::string& rawId, std::vector<std::string>& out) {
    out.clear();
    std::string id = BlockEspNormalizeId(rawId);
    if (id.empty() || IsIgnorableBedPlateBlockId(id)) return;

    // Modern (1.13+): assets/minecraft/textures/block/<id>.png
    out.push_back("assets/minecraft/textures/block/" + id + ".png");
    // Legacy (1.8.9): assets/minecraft/textures/blocks/<name>.png
    out.push_back("assets/minecraft/textures/blocks/" + id + ".png");

    // Common renames / aliases between eras.
    if (id == "cloth" || id == "wool") {
        out.push_back("assets/minecraft/textures/blocks/wool_colored_white.png");
        out.push_back("assets/minecraft/textures/block/white_wool.png");
    }
    if (id.find("wool") != std::string::npos) {
        // white_wool → wool_colored_white
        std::string color = id;
        const std::string suf = "_wool";
        if (color.size() > suf.size() && color.compare(color.size() - suf.size(), suf.size(), suf) == 0)
            color = color.substr(0, color.size() - suf.size());
        if (!color.empty() && color != "wool")
            out.push_back("assets/minecraft/textures/blocks/wool_colored_" + color + ".png");
    }
    if (id == "whitestone" || id == "end_stone") {
        out.push_back("assets/minecraft/textures/blocks/end_stone.png");
        out.push_back("assets/minecraft/textures/block/end_stone.png");
    }
    if (id == "woodenplanks" || id == "wood" || id == "planks") {
        out.push_back("assets/minecraft/textures/blocks/planks_oak.png");
        out.push_back("assets/minecraft/textures/block/oak_planks.png");
    }
    if (id.find("planks") != std::string::npos) {
        out.push_back("assets/minecraft/textures/blocks/planks_oak.png");
    }
    if (id == "ironbars" || id == "iron_bars") {
        out.push_back("assets/minecraft/textures/blocks/iron_bars.png");
        out.push_back("assets/minecraft/textures/block/iron_bars.png");
    }
    if (id.find("hardenedclay") != std::string::npos || id.find("clayhardened") != std::string::npos
        || id == "stainedclay" || id == "terracotta") {
        out.push_back("assets/minecraft/textures/blocks/hardened_clay.png");
        out.push_back("assets/minecraft/textures/block/terracotta.png");
    }
    if (id.find("terracotta") != std::string::npos) {
        out.push_back("assets/minecraft/textures/blocks/hardened_clay.png");
        if (id != "terracotta") {
            std::string color = id;
            const std::string suf = "_terracotta";
            if (color.size() > suf.size() && color.compare(color.size() - suf.size(), suf.size(), suf) == 0)
                color = color.substr(0, color.size() - suf.size());
            out.push_back("assets/minecraft/textures/blocks/hardened_clay_stained_" + color + ".png");
        }
    }
    if (id == "glass" || id == "glasspane" || id == "glass_pane") {
        out.push_back("assets/minecraft/textures/blocks/glass.png");
        out.push_back("assets/minecraft/textures/block/glass.png");
    }
}

struct BedPlateIconEntry {
    ImTextureData* tex = nullptr;
    bool ready = false;
};

struct BedPlateIconCache {
    std::map<std::string, BedPlateIconEntry> icons;
    int nextUniqueId = 9000;

    void clear() {
        for (std::map<std::string, BedPlateIconEntry>::iterator it = icons.begin();
             it != icons.end(); ++it) {
            if (it->second.tex) {
                ImGuiContext* ctx = ImGui::GetCurrentContext();
                if (ctx) ImGui::UnregisterUserTexture(it->second.tex);
                it->second.tex->WantDestroyNextFrame = true;
                it->second.tex->SetStatus(ImTextureStatus_WantDestroy);
                // Keep pixels until backend destroys; leak on shutdown is fine for DLL unload.
            }
        }
        icons.clear();
    }

    ImTextureData* ensureProcedural(const std::string& rawId) {
        std::string id = BlockEspNormalizeId(rawId);
        if (id.empty()) return nullptr;
        BedPlateIconEntry& e = icons[id];
        if (e.tex) return e.tex;
        if (!ImGui::GetCurrentContext()) return nullptr;

        ImTextureData* tex = IM_NEW(ImTextureData)();
        tex->UniqueID = nextUniqueId++;
        tex->Create(ImTextureFormat_RGBA32, 16, 16);
        BedPlateFillProceduralIcon(id, (unsigned char*)tex->GetPixels());
        tex->UseColors = true;
        ImGui::RegisterUserTexture(tex);
        e.tex = tex;
        e.ready = true;
        return tex;
    }

    // Install pre-decoded RGBA8 pixels (any size; will be nearest-scaled to 16x16).
    ImTextureData* ensureFromRgba(const std::string& rawId, const unsigned char* src,
                                  int srcW, int srcH) {
        std::string id = BlockEspNormalizeId(rawId);
        if (id.empty() || !src || srcW <= 0 || srcH <= 0) return ensureProcedural(rawId);
        BedPlateIconEntry& e = icons[id];
        if (e.tex) return e.tex;
        if (!ImGui::GetCurrentContext()) return nullptr;

        ImTextureData* tex = IM_NEW(ImTextureData)();
        tex->UniqueID = nextUniqueId++;
        tex->Create(ImTextureFormat_RGBA32, 16, 16);
        unsigned char* dst = (unsigned char*)tex->GetPixels();
        for (int y = 0; y < 16; y++) {
            int sy = y * srcH / 16;
            for (int x = 0; x < 16; x++) {
                int sx = x * srcW / 16;
                const unsigned char* p = src + (sy * srcW + sx) * 4;
                unsigned char* d = dst + (y * 16 + x) * 4;
                d[0] = p[0]; d[1] = p[1]; d[2] = p[2]; d[3] = p[3] ? p[3] : 255;
            }
        }
        tex->UseColors = true;
        ImGui::RegisterUserTexture(tex);
        e.tex = tex;
        e.ready = true;
        return tex;
    }

    ImTextureRef getRef(const std::string& rawId) {
        ImTextureData* tex = ensureProcedural(rawId);
        if (!tex) {
            ImTextureRef empty;
            return empty;
        }
        return tex->GetTexRef();
    }
};

// Vape-like plate metrics. Chips are wider than square icons so short labels fit.
static const float kBedPlateIconSize = 18.0f;
static const float kBedPlateIconW = 28.0f;
static const float kBedPlateIconPad = 3.0f;
static const float kBedPlatePanelPadX = 4.0f;
static const float kBedPlatePanelPadY = 3.0f;

inline void BedPlateComputePanelSize(int iconCount, bool showDistance, float distTextW,
                                     float* outW, float* outH, float* outIconsY) {
    int n = iconCount > 0 ? iconCount : 1;
    float iconsW = (float)n * kBedPlateIconW + (float)(n - 1) * kBedPlateIconPad;
    float w = iconsW + kBedPlatePanelPadX * 2.0f;
    if (showDistance) w = (std::max)(w, distTextW + 10.0f);
    float h = kBedPlateIconSize + kBedPlatePanelPadY * 2.0f;
    float iconsY = kBedPlatePanelPadY;
    if (showDistance) {
        h += 10.0f;
        iconsY += 10.0f;
    }
    if (outW) *outW = w;
    if (outH) *outH = h;
    if (outIconsY) *outIconsY = iconsY;
}

// Draw a readable block chip: colored face + short label (Gls/Obs/WWl/...).
// Vape uses real Minecraft item icons via ItemIconRenderer; we cannot safely do
// ImGui RegisterUserTexture on 26.2 Vulkan (crashes present), so labels replace
// unreadable procedural "textures".
inline void BedPlateDrawIcon(ImDrawList* dl, float x, float y, const std::string& rawId) {
    if (!dl) return;
    unsigned int packed = BedPlateIconColorForId(rawId);
    int r = (int)(packed & 0xFF);
    int g = (int)((packed >> 8) & 0xFF);
    int b = (int)((packed >> 16) & 0xFF);
    auto shade = [](int c, int pct) -> int {
        int v = (c * pct) / 100;
        if (v < 0) v = 0;
        if (v > 255) v = 255;
        return v;
    };
    // Luminance for contrasting text.
    int lum = (r * 299 + g * 587 + b * 114) / 1000;
    ImU32 face = IM_COL32(r, g, b, 255);
    ImU32 top = IM_COL32(shade(r, 120), shade(g, 120), shade(b, 120), 255);
    ImU32 border = IM_COL32(shade(r, 35), shade(g, 35), shade(b, 35), 255);
    ImU32 textCol = lum > 140 ? IM_COL32(20, 20, 20, 255) : IM_COL32(245, 245, 245, 255);
    ImU32 textShadow = lum > 140 ? IM_COL32(255, 255, 255, 90) : IM_COL32(0, 0, 0, 160);

    const float w = kBedPlateIconW;
    const float h = kBedPlateIconSize;
    dl->AddRectFilled(ImVec2(x, y), ImVec2(x + w, y + h), face, 3.0f);
    dl->AddRectFilled(ImVec2(x + 1, y + 1), ImVec2(x + w - 1, y + 4), top, 2.0f);
    dl->AddRect(ImVec2(x, y), ImVec2(x + w, y + h), border, 3.0f, 0, 1.0f);

    std::string label = BedPlateShortLabel(rawId);
    ImVec2 ts = ImGui::CalcTextSize(label.c_str());
    float tx = std::floor(x + (w - ts.x) * 0.5f);
    float ty = std::floor(y + (h - ts.y) * 0.5f + 0.5f);
    dl->AddText(ImVec2(tx + 1, ty + 1), textShadow, label.c_str());
    dl->AddText(ImVec2(tx, ty), textCol, label.c_str());
}

// Load a PNG from the Minecraft jar via ImageIO into an RGBA buffer.
inline bool BedPlateLoadPngViaImageIO(JNIEnv* env, jobject classLoader, const std::string& path,
                                      std::vector<unsigned char>& outRgba, int* outW, int* outH) {
    outRgba.clear();
    if (outW) *outW = 0;
    if (outH) *outH = 0;
    if (!env || !classLoader || path.empty()) return false;

    jclass loaderCls = env->GetObjectClass(classLoader);
    if (!loaderCls) return false;
    jmethodID getStream = env->GetMethodID(loaderCls, "getResourceAsStream", "(Ljava/lang/String;)Ljava/io/InputStream;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); getStream = nullptr; }
    env->DeleteLocalRef(loaderCls);
    if (!getStream) return false;

    jstring jpath = env->NewStringUTF(path.c_str());
    if (!jpath) return false;
    jobject stream = env->CallObjectMethod(classLoader, getStream, jpath);
    env->DeleteLocalRef(jpath);
    if (env->ExceptionCheck()) { env->ExceptionClear(); stream = nullptr; }
    if (!stream) return false;

    jclass imageIo = env->FindClass("javax/imageio/ImageIO");
    if (env->ExceptionCheck()) { env->ExceptionClear(); imageIo = nullptr; }
    if (!imageIo) { env->DeleteLocalRef(stream); return false; }
    jmethodID read = env->GetStaticMethodID(imageIo, "read", "(Ljava/io/InputStream;)Ljava/awt/image/BufferedImage;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); read = nullptr; }
    if (!read) { env->DeleteLocalRef(imageIo); env->DeleteLocalRef(stream); return false; }

    jobject image = env->CallStaticObjectMethod(imageIo, read, stream);
    env->DeleteLocalRef(imageIo);
    {
        jclass isCls = env->GetObjectClass(stream);
        if (isCls) {
            jmethodID close = env->GetMethodID(isCls, "close", "()V");
            if (!env->ExceptionCheck() && close) env->CallVoidMethod(stream, close);
            if (env->ExceptionCheck()) env->ExceptionClear();
            env->DeleteLocalRef(isCls);
        }
    }
    env->DeleteLocalRef(stream);
    if (env->ExceptionCheck()) { env->ExceptionClear(); image = nullptr; }
    if (!image) return false;

    jclass imgCls = env->GetObjectClass(image);
    if (!imgCls) { env->DeleteLocalRef(image); return false; }
    jmethodID getW = env->GetMethodID(imgCls, "getWidth", "()I");
    jmethodID getH = env->GetMethodID(imgCls, "getHeight", "()I");
    jmethodID getRGB = env->GetMethodID(imgCls, "getRGB", "(IIII[III)[I");
    if (env->ExceptionCheck()) { env->ExceptionClear(); getW = getH = getRGB = nullptr; }
    if (!getW || !getH || !getRGB) {
        env->DeleteLocalRef(imgCls);
        env->DeleteLocalRef(image);
        return false;
    }
    int w = env->CallIntMethod(image, getW);
    int h = env->CallIntMethod(image, getH);
    if (env->ExceptionCheck() || w <= 0 || h <= 0 || w > 256 || h > 256) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        env->DeleteLocalRef(imgCls);
        env->DeleteLocalRef(image);
        return false;
    }

    jintArray jpixels = env->NewIntArray(w * h);
    if (!jpixels) {
        env->DeleteLocalRef(imgCls);
        env->DeleteLocalRef(image);
        return false;
    }
    jobject ignored = env->CallObjectMethod(image, getRGB, 0, 0, w, h, jpixels, 0, w);
    if (ignored) env->DeleteLocalRef(ignored);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        env->DeleteLocalRef(jpixels);
        env->DeleteLocalRef(imgCls);
        env->DeleteLocalRef(image);
        return false;
    }

    jint* argb = env->GetIntArrayElements(jpixels, nullptr);
    if (!argb) {
        env->DeleteLocalRef(jpixels);
        env->DeleteLocalRef(imgCls);
        env->DeleteLocalRef(image);
        return false;
    }
    outRgba.resize((size_t)w * (size_t)h * 4);
    for (int i = 0; i < w * h; i++) {
        unsigned int p = (unsigned int)argb[i];
        outRgba[(size_t)i * 4 + 0] = (unsigned char)((p >> 16) & 0xFF);
        outRgba[(size_t)i * 4 + 1] = (unsigned char)((p >> 8) & 0xFF);
        outRgba[(size_t)i * 4 + 2] = (unsigned char)(p & 0xFF);
        outRgba[(size_t)i * 4 + 3] = (unsigned char)((p >> 24) & 0xFF);
    }
    env->ReleaseIntArrayElements(jpixels, argb, JNI_ABORT);
    env->DeleteLocalRef(jpixels);
    env->DeleteLocalRef(imgCls);
    env->DeleteLocalRef(image);
    if (outW) *outW = w;
    if (outH) *outH = h;
    return true;
}

inline ImTextureRef BedPlateResolveIcon(JNIEnv* env, jobject classLoader, BedPlateIconCache& cache,
                                        const std::string& rawId) {
    std::string id = BlockEspNormalizeId(rawId);
    if (id.empty()) {
        ImTextureRef empty;
        return empty;
    }
    std::map<std::string, BedPlateIconEntry>::iterator it = cache.icons.find(id);
    if (it != cache.icons.end() && it->second.tex)
        return it->second.tex->GetTexRef();

    if (env && classLoader) {
        std::vector<std::string> paths;
        BedPlateTexturePathCandidates(id, paths);
        for (size_t i = 0; i < paths.size(); i++) {
            std::vector<unsigned char> rgba;
            int w = 0, h = 0;
            if (!BedPlateLoadPngViaImageIO(env, classLoader, paths[i], rgba, &w, &h)) continue;
            ImTextureData* tex = cache.ensureFromRgba(id, rgba.data(), w, h);
            if (tex) return tex->GetTexRef();
        }
    }
    return cache.getRef(id);
}

} // namespace lc
