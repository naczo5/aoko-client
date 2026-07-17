#pragma once

#include <cstring>

namespace lc {

// Pure mapping table kept separate from the JVMTI runtime so it can be tested
// without a JVM. The result is intentionally a HUD surface only; menus and
// arbitrary screen classes are never accepted here.
inline const char* NickHiderJvmtiSurfaceForSignature(bool modern, const char* signature)
{
    if (!signature) return nullptr;
    struct Candidate { const char* signature; const char* surface; };
    static const Candidate legacy[] = {
        { "Lnet/minecraft/client/gui/GuiNewChat;", "chat" },
        { "Lnet/minecraft/client/gui/GuiPlayerTabOverlay;", "tab" },
        { "Lnet/minecraft/client/gui/GuiIngame;", "hud" },
        { "Lnet/minecraft/client/renderer/entity/RenderPlayer;", "nametag" },
        { "Lnet/minecraft/client/renderer/entity/RendererLivingEntity;", "nametag" },
        { "Lbct;", "chat" }, { "Lbck;", "tab" }, { "Lbfl;", "hud" },
        { "Lbop;", "nametag" }, { "Lboi;", "nametag" }
    };
    static const Candidate current[] = {
        { "Lnet/minecraft/class_338;", "chat" },
        { "Lnet/minecraft/client/gui/hud/ChatHud;", "chat" },
        { "Lnet/minecraft/client/gui/components/ChatComponent;", "chat" },
        { "Lnet/minecraft/class_355;", "tab" },
        { "Lnet/minecraft/client/gui/hud/PlayerListHud;", "tab" },
        { "Lnet/minecraft/client/gui/components/PlayerTabOverlay;", "tab" },
        { "Lnet/minecraft/class_329;", "hud" },
        { "Lnet/minecraft/client/gui/hud/InGameHud;", "hud" },
        { "Lnet/minecraft/client/gui/Gui;", "hud" },
        { "Lnet/minecraft/class_1007;", "nametag" },
        { "Lnet/minecraft/client/render/entity/PlayerEntityRenderer;", "nametag" },
        { "Lnet/minecraft/client/renderer/entity/player/PlayerRenderer;", "nametag" },
        { "Lnet/minecraft/class_922;", "nametag" },
        { "Lnet/minecraft/client/render/entity/LivingEntityRenderer;", "nametag" },
        { "Lnet/minecraft/client/renderer/entity/LivingEntityRenderer;", "nametag" }
    };
    const Candidate* entries = modern ? current : legacy;
    const size_t count = modern ? sizeof(current) / sizeof(current[0]) : sizeof(legacy) / sizeof(legacy[0]);
    for (size_t i = 0; i < count; ++i)
        if (std::strcmp(signature, entries[i].signature) == 0) return entries[i].surface;
    return nullptr;
}

inline bool IsNickHiderJvmtiRendererSignature(bool modern, const char* signature)
{
    if (!signature) return false;
    static const char* legacy[] = { "Lnet/minecraft/client/gui/FontRenderer;", "Lbfr;" };
    static const char* current[] = {
        "Lnet/minecraft/class_327;", "Lnet/minecraft/client/font/TextRenderer;",
        "Lnet/minecraft/client/gui/Font;", "Lnet/minecraft/util/StringDecomposer;"
    };
    const char* const* entries = modern ? current : legacy;
    const size_t count = modern ? sizeof(current) / sizeof(current[0]) : sizeof(legacy) / sizeof(legacy[0]);
    for (size_t i = 0; i < count; ++i)
        if (std::strcmp(signature, entries[i]) == 0) return true;
    return false;
}

} // namespace lc
