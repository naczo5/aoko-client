#pragma once
// visuals_hooks.h
//
// Which game methods get a guard/value prologue (see method_hook_rewriter.h) for each game
// generation. The callbacks are static methods of com.aoko.visuals.Hooks; names and
// descriptors must stay in sync with that class. Method names list MCP then SRG for
// 1.8.9; 26.x classes carry Mojang names.

#include "method_hook_rewriter.h"
#include "visuals_config.h"

#include <string>

#include <cstddef>

namespace visuals {

struct HookClass {
    const char* internalName;
    const mhook::HookSpec* specs;
    size_t count;
};

static const char kHelperInternalName[] = "com/aoko/visuals/Hooks";
static const char kHelperClassPrefix[] = "com.aoko.visuals.";
static const unsigned kForbidBridge = 0x1040;     // ACC_SYNTHETIC | ACC_BRIDGE

// ---- 1.8.9 -----------------------------------------------------------------------------

static const char* const kLgCelestialNames[] = { "getCelestialAngle", "func_72826_c", nullptr };
static const char* const kLgSkyColorNames[] = { "getSkyColor", "func_72833_a", nullptr };
static const char* const kLgCloudNames[] = { "getCloudColour", "func_72824_f", nullptr };
static const char* const kLgStarNames[] = { "getStarBrightness", "func_72880_h", nullptr };
static const char* const kLgMoonNames[] = { "getMoonPhase", "func_72853_d", nullptr };
static const char* const kLgRainNames[] = { "getRainStrength", "func_72867_j", nullptr };
static const char* const kLgThunderNames[] = { "getThunderStrength", "func_72819_i", nullptr };
static const char* const kLgFogNames[] = { "getFogColor", "func_76562_b", nullptr };
static const char* const kLgSunsetNames[] = { "calcSunriseSunsetColors", "func_76560_a", nullptr };
static const char* const kLgRenderSkyNames[] = { "renderSky", "func_174976_a", nullptr };
static const char* const kLgLayerNames[] = { "doRenderLayer", "func_177141_a", nullptr };
static const char* const kLgCapeNames[] = { "getLocationCape", "func_110303_q", nullptr };

static const mhook::HookSpec kLegacyWorldHooks[] = {
    { kLgCelestialNames, "(F)F",     "lgCelestial", "lvCelestial", 0 },
    { kLgSkyColorNames,  "(L*;F)L*;", "lgSkyColor",  "lvSkyColor",  0 },
    { kLgCloudNames,     "(F)L*;",   "lgCloud",     "lvCloud",     0 },
    { kLgStarNames,      "(F)F",     "lgStars",     "lvStars",     0 },
    { kLgMoonNames,      "()I",      "lgMoon",      "lvMoon",      0 },
    { kLgRainNames,      "(F)F",     "lgRain",      "lvRain",      0 },
    { kLgThunderNames,   "(F)F",     "lgThunder",   "lvThunder",   0 },
};
static const mhook::HookSpec kLegacyProviderHooks[] = {
    { kLgFogNames,       "(FF)L*;",  "lgFog",       "lvFog",       0 },
    { kLgSunsetNames,    "(FF)L*;",  "lgSunset",    "lvSunset",    0 },
};
static const mhook::HookSpec kLegacyRenderGlobalHooks[] = {
    { kLgRenderSkyNames, "(FI)V",    "lgSky",       nullptr,       0 },
};
static const mhook::HookSpec kLegacyLayerHooks[] = {
    { kLgLayerNames,     "(L*;FFFFFFF)V", "lgLayer", nullptr,      kForbidBridge },
};
static const mhook::HookSpec kLegacyPlayerHooks[] = {
    { kLgCapeNames,      "()L*;",    "lgCape",      "lvCape",      0 },
};

static const HookClass kLegacyClasses[] = {
    { "net/minecraft/world/World",                           kLegacyWorldHooks,        sizeof(kLegacyWorldHooks) / sizeof(kLegacyWorldHooks[0]) },
    { "net/minecraft/world/WorldProvider",                   kLegacyProviderHooks,     sizeof(kLegacyProviderHooks) / sizeof(kLegacyProviderHooks[0]) },
    { "net/minecraft/client/renderer/RenderGlobal",          kLegacyRenderGlobalHooks, sizeof(kLegacyRenderGlobalHooks) / sizeof(kLegacyRenderGlobalHooks[0]) },
    { "net/minecraft/client/renderer/entity/layers/LayerCape", kLegacyLayerHooks,      sizeof(kLegacyLayerHooks) / sizeof(kLegacyLayerHooks[0]) },
    { "net/minecraft/client/entity/AbstractClientPlayer",    kLegacyPlayerHooks,       sizeof(kLegacyPlayerHooks) / sizeof(kLegacyPlayerHooks[0]) },
};

// ---- 26.x -------------------------------------------------------------------------------

static const char* const kMgGetValueNames[] = { "getValue", nullptr };
static const char* const kMgRainNames[] = { "getRainLevel", nullptr };
static const char* const kMgThunderNames[] = { "getThunderLevel", nullptr };
static const char* const kMgSubmitNames[] = { "submit", nullptr };

static const mhook::HookSpec kModernProbeHooks[] = {
    { kMgGetValueNames,  "(L*;F)L*;", "mgAttr",     "mvAttr",      0 },
};
static const mhook::HookSpec kModernLevelHooks[] = {
    { kMgRainNames,      "(F)F",      "mgRain",     "mvRain",      0 },
    { kMgThunderNames,   "(F)F",      "mgThunder",  "mvThunder",   0 },
};
static const mhook::HookSpec kModernCapeHooks[] = {
    { kMgSubmitNames,    "(L*;L*;IL*;FF)V", "mgCape", nullptr,     kForbidBridge },
};

static const HookClass kModernClasses[] = {
    { "net/minecraft/world/attribute/EnvironmentAttributeProbe", kModernProbeHooks, sizeof(kModernProbeHooks) / sizeof(kModernProbeHooks[0]) },
    { "net/minecraft/world/level/Level",                         kModernLevelHooks, sizeof(kModernLevelHooks) / sizeof(kModernLevelHooks[0]) },
    { "net/minecraft/client/renderer/entity/layers/CapeLayer",   kModernCapeHooks,  sizeof(kModernCapeHooks) / sizeof(kModernCapeHooks[0]) },
};

// Descriptors of the Hooks methods a spec needs. Every hooked method is an instance method, so
// the receiver is passed first as Object; references erase to Object (see method_hook_rewriter.h).
inline void ExpectedHelperDescriptors(const mhook::HookSpec& spec, std::string* guard, std::string* value) {
    std::string params = "(Ljava/lang/Object;";
    const char* p = spec.shape + 1;
    while (*p && *p != ')') {
        if (p[0] == 'L' && p[1] == '*' && p[2] == ';') { params += "Ljava/lang/Object;"; p += 3; }
        else { params += *p; ++p; }
    }
    params += ")";
    const char* ret = (*p == ')') ? p + 1 : "V";
    *guard = params + "Z";
    *value = spec.valueName ? params + (std::string(ret) == "L*;" ? std::string("Ljava/lang/Object;") : std::string(ret)) : std::string();
}

inline const HookClass* HookClasses(Generation g, size_t* count) {
    if (g == Generation::Legacy189) { *count = sizeof(kLegacyClasses) / sizeof(kLegacyClasses[0]); return kLegacyClasses; }
    *count = sizeof(kModernClasses) / sizeof(kModernClasses[0]);
    return kModernClasses;
}

} // namespace visuals
