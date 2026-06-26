#pragma once
// mappings_121.h
// Descriptor tables for MC 1.21.x / Lunar 26.1 (Fabric/Mojmap bridge).
// Candidate order: Yarn intermediary → Mojmap official → obfuscated fallback.
// Include this in bridge_261.cpp only.

#include "jni_core/resolver.h"

namespace mc121 {

// ── Classes ──────────────────────────────────────────────────────────────────

inline const char* kMcClassNames[] = {
    "net.minecraft.class_310",                  // Yarn
    "net.minecraft.client.Minecraft",           // Mojmap
    "net.minecraft.client.MinecraftClient",     // Fabric alt
    nullptr
};
inline ClassDesc CLS_MC{ kMcClassNames };

inline const char* kEntityNames[] = {
    "net.minecraft.class_1297",                 // Yarn
    "net.minecraft.world.entity.Entity",        // Mojmap
    nullptr
};
inline ClassDesc CLS_ENTITY{ kEntityNames };

inline const char* kLivingEntityNames[] = {
    "net.minecraft.class_1309",                 // Yarn
    "net.minecraft.world.entity.LivingEntity",  // Mojmap
    nullptr
};
inline ClassDesc CLS_LIVING_ENTITY{ kLivingEntityNames };

inline const char* kLocalPlayerNames[] = {
    "net.minecraft.class_746",                  // Yarn
    "net.minecraft.client.player.LocalPlayer",  // Mojmap
    nullptr
};
inline ClassDesc CLS_LOCAL_PLAYER{ kLocalPlayerNames };

inline const char* kClientWorldNames[] = {
    "net.minecraft.class_638",                  // Yarn
    "net.minecraft.client.multiplayer.ClientLevel", // Mojmap
    nullptr
};
inline ClassDesc CLS_WORLD{ kClientWorldNames };

inline const char* kScreenNames[] = {
    "net.minecraft.class_437",                  // Yarn
    "net.minecraft.client.gui.screens.Screen",  // Mojmap
    "net.minecraft.client.gui.screen.Screen",   // Fabric alt
    nullptr
};
inline ClassDesc CLS_SCREEN{ kScreenNames };

inline const char* kChatScreenNames[] = {
    "net.minecraft.class_408",                  // Yarn
    "net.minecraft.client.gui.screens.ChatScreen", // Mojmap
    nullptr
};
inline ClassDesc CLS_CHAT_SCREEN{ kChatScreenNames };

inline const char* kBlockPosNames[] = {
    "net.minecraft.class_2338",                 // Yarn
    "net.minecraft.core.BlockPos",              // Mojmap
    nullptr
};
inline ClassDesc CLS_BLOCKPOS{ kBlockPosNames };

inline const char* kBlockEntityNames[] = {
    "net.minecraft.class_2586",                 // Yarn
    "net.minecraft.world.level.block.entity.BlockEntity", // Mojmap
    nullptr
};
inline ClassDesc CLS_BLOCK_ENTITY{ kBlockEntityNames };

inline const char* kVec3dNames[] = {
    "net.minecraft.class_243",                  // Yarn
    "net.minecraft.world.phys.Vec3",            // Mojmap
    nullptr
};
inline ClassDesc CLS_VEC3D{ kVec3dNames };

inline const char* kItemStackNames[] = {
    "net.minecraft.class_1799",                 // Yarn
    "net.minecraft.world.item.ItemStack",       // Mojmap
    nullptr
};
inline ClassDesc CLS_ITEM_STACK{ kItemStackNames };

inline const char* kBlockItemNames[] = {
    "net.minecraft.class_1747",                 // Yarn
    "net.minecraft.world.item.BlockItem",       // Mojmap
    nullptr
};
inline ClassDesc CLS_BLOCK_ITEM{ kBlockItemNames };

inline const char* kGameProfileNames[] = {
    "com.mojang.authlib.GameProfile", nullptr
};
inline ClassDesc CLS_GAME_PROFILE{ kGameProfileNames };

// ── Fields on Minecraft ───────────────────────────────────────────────────────

inline const char* kPlayerFieldNames[] = { "field_1724", "player",  "f_91074_", nullptr };
inline const char* kPlayerFieldSigs[]  = {
    "Lnet/minecraft/class_746;",
    "Lnet/minecraft/client/player/LocalPlayer;",
    nullptr
};
inline FieldDesc FIELD_PLAYER{ CLS_MC, kPlayerFieldNames, kPlayerFieldSigs };

inline const char* kScreenFieldNames[] = { "field_1755", "screen", "currentScreen", nullptr };
inline const char* kScreenFieldSigs[]  = {
    "Lnet/minecraft/class_437;",
    "Lnet/minecraft/client/gui/screens/Screen;",
    "Lnet/minecraft/client/gui/screen/Screen;",
    nullptr
};
inline FieldDesc FIELD_SCREEN{ CLS_MC, kScreenFieldNames, kScreenFieldSigs };

inline const char* kWorldFieldNames[]  = { "field_1687", "level", "world", nullptr };
inline const char* kWorldFieldSigs[]   = {
    "Lnet/minecraft/class_638;",
    "Lnet/minecraft/client/multiplayer/ClientLevel;",
    nullptr
};
inline FieldDesc FIELD_WORLD{ CLS_MC, kWorldFieldNames, kWorldFieldSigs };

// ── Fields on Entity ─────────────────────────────────────────────────────────

inline const char* kEntityPosNames[] = { "field_5961", "position", "pos", nullptr };
inline const char* kEntityPosSigs[]  = {
    "Lnet/minecraft/class_243;",
    "Lnet/minecraft/world/phys/Vec3;",
    nullptr
};
inline FieldDesc FIELD_ENTITY_POS{ CLS_ENTITY, kEntityPosNames, kEntityPosSigs };

// ── Fields on Vec3d ───────────────────────────────────────────────────────────

inline const char* kVec3XNames[] = { "field_1352", "x", nullptr };
inline const char* kVec3XSigs[]  = { "D", nullptr };
inline FieldDesc FIELD_VEC3_X{ CLS_VEC3D, kVec3XNames, kVec3XSigs };

inline const char* kVec3YNames[] = { "field_1351", "y", nullptr };
inline const char* kVec3YSigs[]  = { "D", nullptr };
inline FieldDesc FIELD_VEC3_Y{ CLS_VEC3D, kVec3YNames, kVec3YSigs };

inline const char* kVec3ZNames[] = { "field_1350", "z", nullptr };
inline const char* kVec3ZSigs[]  = { "D", nullptr };
inline FieldDesc FIELD_VEC3_Z{ CLS_VEC3D, kVec3ZNames, kVec3ZSigs };

// ── Methods on Entity ─────────────────────────────────────────────────────────

inline const char* kGetXNames[] = { "method_23317", "getX", nullptr };
inline const char* kGetXSigs[]  = { "()D", nullptr };
inline MethodDesc METHOD_GET_X{ CLS_ENTITY, kGetXNames, kGetXSigs };

inline const char* kGetYNames[] = { "method_23318", "getY", nullptr };
inline const char* kGetYSigs[]  = { "()D", nullptr };
inline MethodDesc METHOD_GET_Y{ CLS_ENTITY, kGetYNames, kGetYSigs };

inline const char* kGetZNames[] = { "method_23321", "getZ", nullptr };
inline const char* kGetZSigs[]  = { "()D", nullptr };
inline MethodDesc METHOD_GET_Z{ CLS_ENTITY, kGetZNames, kGetZSigs };

// ── Methods on LivingEntity ───────────────────────────────────────────────────

inline const char* kGetHealthNames[] = { "method_6032", "getHealth", nullptr };
inline const char* kGetHealthSigs[]  = { "()F", nullptr };
inline MethodDesc METHOD_GET_HEALTH{ CLS_LIVING_ENTITY, kGetHealthNames, kGetHealthSigs };

// ── AntiDebuff: LivingEntity.removeEffect(Holder<MobEffect>) -> boolean ─────────
// Yarn:   method_6016(class_6880)            -> removeStatusEffect(RegistryEntry)
// Mojmap: removeEffect(net.minecraft.core.Holder)
// Targeted effect holders are static fields on StatusEffects/MobEffects:
//   BLINDNESS = field_5919 / BLINDNESS
//   NAUSEA    = field_5916 / CONFUSION   (registry id "nausea")
//   DARKNESS  = field_38092 / DARKNESS   (1.19+)
// Resolved inline in bridge_261.cpp (EnsureAntiDebuffJni) using these names.
inline const char* kRemoveEffectNames[] = { "method_6016", "removeEffect", nullptr };
inline const char* kRemoveEffectSigs[]  = {
    "(Lnet/minecraft/class_6880;)Z",
    "(Lnet/minecraft/core/Holder;)Z",
    nullptr
};
inline MethodDesc METHOD_REMOVE_EFFECT{ CLS_LIVING_ENTITY, kRemoveEffectNames, kRemoveEffectSigs };

// ── Methods on Minecraft ──────────────────────────────────────────────────────

inline const char* kSetScreenNames[] = { "method_1507", "setScreen", nullptr };
inline const char* kSetScreenSigs[]  = {
    "(Lnet/minecraft/class_437;)V",
    "(Lnet/minecraft/client/gui/screens/Screen;)V",
    nullptr
};
inline MethodDesc METHOD_SET_SCREEN{ CLS_MC, kSetScreenNames, kSetScreenSigs };

// ── Methods on GameProfile ────────────────────────────────────────────────────

inline const char* kGetNameNames[] = { "getName", nullptr };
inline const char* kGetNameSigs[]  = { "()Ljava/lang/String;", nullptr };
inline MethodDesc METHOD_GAME_PROFILE_GET_NAME{ CLS_GAME_PROFILE, kGetNameNames, kGetNameSigs };

// ── Reset all (call during remap) ─────────────────────────────────────────────

inline void ResetAll(JNIEnv* env) {
    ResetDesc(CLS_MC, env);
    ResetDesc(CLS_ENTITY, env);
    ResetDesc(CLS_LIVING_ENTITY, env);
    ResetDesc(CLS_LOCAL_PLAYER, env);
    ResetDesc(CLS_WORLD, env);
    ResetDesc(CLS_SCREEN, env);
    ResetDesc(CLS_CHAT_SCREEN, env);
    ResetDesc(CLS_BLOCKPOS, env);
    ResetDesc(CLS_BLOCK_ENTITY, env);
    ResetDesc(CLS_VEC3D, env);
    ResetDesc(CLS_ITEM_STACK, env);
    ResetDesc(CLS_BLOCK_ITEM, env);
    ResetDesc(CLS_GAME_PROFILE, env);

    ResetDesc(FIELD_PLAYER);  ResetDesc(FIELD_SCREEN);
    ResetDesc(FIELD_WORLD);   ResetDesc(FIELD_ENTITY_POS);
    ResetDesc(FIELD_VEC3_X);  ResetDesc(FIELD_VEC3_Y);  ResetDesc(FIELD_VEC3_Z);

    ResetDesc(METHOD_GET_X);  ResetDesc(METHOD_GET_Y);  ResetDesc(METHOD_GET_Z);
    ResetDesc(METHOD_GET_HEALTH);
    ResetDesc(METHOD_REMOVE_EFFECT);
    ResetDesc(METHOD_SET_SCREEN);
    ResetDesc(METHOD_GAME_PROFILE_GET_NAME);
}

} // namespace mc121
