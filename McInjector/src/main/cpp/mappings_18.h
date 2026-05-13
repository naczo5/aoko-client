#pragma once
// mappings_18.h
// Descriptor tables for MC 1.8.9 (Forge/Lunar legacy bridge).
// Candidate arrays are tried in order: canonical name first, then MCP obfuscated.
// Include this in bridge.cpp only.

#include "jni_core/resolver.h"

namespace mc18 {

// ── Classes ──────────────────────────────────────────────────────────────────

inline const char* kMcClassNames[] = {
    "net.minecraft.client.Minecraft", nullptr
};
inline ClassDesc CLS_MC{ kMcClassNames };

inline const char* kEntityNames[] = {
    "net.minecraft.entity.Entity", nullptr
};
inline ClassDesc CLS_ENTITY{ kEntityNames };

inline const char* kEntityPlayerSPNames[] = {
    "net.minecraft.client.entity.EntityPlayerSP", nullptr
};
inline ClassDesc CLS_PLAYER{ kEntityPlayerSPNames };

inline const char* kWorldClientNames[] = {
    "net.minecraft.client.multiplayer.WorldClient", nullptr
};
inline ClassDesc CLS_WORLD{ kWorldClientNames };

inline const char* kBlockPosNames[] = {
    "net.minecraft.util.BlockPos", nullptr
};
inline ClassDesc CLS_BLOCKPOS{ kBlockPosNames };

inline const char* kTileEntityNames[] = {
    "net.minecraft.tileentity.TileEntity", nullptr
};
inline ClassDesc CLS_TILE_ENTITY{ kTileEntityNames };

inline const char* kGuiChatNames[] = {
    "net.minecraft.client.gui.GuiChat", nullptr
};
inline ClassDesc CLS_GUI_CHAT{ kGuiChatNames };

inline const char* kGuiScreenNames[] = {
    "net.minecraft.client.gui.GuiScreen", nullptr
};
inline ClassDesc CLS_GUI_SCREEN{ kGuiScreenNames };

inline const char* kActiveRenderInfoNames[] = {
    "net.minecraft.client.renderer.ActiveRenderInfo", nullptr
};
inline ClassDesc CLS_ACTIVE_RENDER_INFO{ kActiveRenderInfoNames };

inline const char* kFloatBufferNames[] = {
    "java.nio.FloatBuffer", nullptr
};
inline ClassDesc CLS_FLOAT_BUFFER{ kFloatBufferNames };

// ── Fields on Minecraft ───────────────────────────────────────────────────────

inline const char* kPlayerFieldNames[]  = { "thePlayer",       "field_71439_g", nullptr };
inline const char* kPlayerFieldSigs[]   = {
    "Lnet/minecraft/client/entity/EntityPlayerSP;", nullptr
};
inline FieldDesc FIELD_PLAYER{ CLS_MC, kPlayerFieldNames, kPlayerFieldSigs };

inline const char* kScreenFieldNames[]  = { "currentScreen",   "field_71462_r", nullptr };
inline const char* kScreenFieldSigs[]   = {
    "Lnet/minecraft/client/gui/GuiScreen;", nullptr
};
inline FieldDesc FIELD_SCREEN{ CLS_MC, kScreenFieldNames, kScreenFieldSigs };

inline const char* kWorldFieldNames[]   = { "theWorld",        "field_71441_e", nullptr };
inline const char* kWorldFieldSigs[]    = {
    "Lnet/minecraft/client/multiplayer/WorldClient;", nullptr
};
inline FieldDesc FIELD_WORLD{ CLS_MC, kWorldFieldNames, kWorldFieldSigs };

// ── Fields on Entity ─────────────────────────────────────────────────────────

inline const char* kPosXNames[]  = { "posX",  "field_70165_t", nullptr };
inline const char* kPosXSigs[]   = { "D", nullptr };
inline FieldDesc FIELD_POS_X{ CLS_ENTITY, kPosXNames, kPosXSigs };

inline const char* kPosYNames[]  = { "posY",  "field_70163_u", nullptr };
inline const char* kPosYSigs[]   = { "D", nullptr };
inline FieldDesc FIELD_POS_Y{ CLS_ENTITY, kPosYNames, kPosYSigs };

inline const char* kPosZNames[]  = { "posZ",  "field_70161_v", nullptr };
inline const char* kPosZSigs[]   = { "D", nullptr };
inline FieldDesc FIELD_POS_Z{ CLS_ENTITY, kPosZNames, kPosZSigs };

inline const char* kRotYawNames[]   = { "rotationYaw",   "field_70177_z", nullptr };
inline const char* kRotYawSigs[]    = { "F", nullptr };
inline FieldDesc FIELD_ROT_YAW{ CLS_ENTITY, kRotYawNames, kRotYawSigs };

inline const char* kRotPitchNames[] = { "rotationPitch", "field_70125_A", nullptr };
inline const char* kRotPitchSigs[]  = { "F", nullptr };
inline FieldDesc FIELD_ROT_PITCH{ CLS_ENTITY, kRotPitchNames, kRotPitchSigs };

// ── Fields on WorldClient ─────────────────────────────────────────────────────

inline const char* kPlayerEntitiesNames[] = { "playerEntities", nullptr };
inline const char* kPlayerEntitiesSigs[]  = { "Ljava/util/List;", nullptr };
inline FieldDesc FIELD_PLAYER_ENTITIES{ CLS_WORLD, kPlayerEntitiesNames, kPlayerEntitiesSigs };

inline const char* kTileEntityListNames[] = { "loadedTileEntityList", nullptr };
inline const char* kTileEntityListSigs[]  = { "Ljava/util/List;", nullptr };
inline FieldDesc FIELD_TILE_ENTITY_LIST{ CLS_WORLD, kTileEntityListNames, kTileEntityListSigs };

// ── Fields on TileEntity ──────────────────────────────────────────────────────

inline const char* kTEPosNames[] = { "pos", nullptr };
inline const char* kTEPosSigs[]  = { "Lnet/minecraft/util/BlockPos;", nullptr };
inline FieldDesc FIELD_TE_POS{ CLS_TILE_ENTITY, kTEPosNames, kTEPosSigs };

// ── Methods on Entity / LivingEntity ─────────────────────────────────────────

inline const char* kGetHealthNames[] = { "getHealth",       "func_110143_aJ", nullptr };
inline const char* kGetHealthSigs[]  = { "()F", nullptr };
inline MethodDesc METHOD_GET_HEALTH{ CLS_PLAYER, kGetHealthNames, kGetHealthSigs };

// ── Methods on BlockPos ───────────────────────────────────────────────────────

inline const char* kGetXNames[] = { "getX", "func_177958_n", nullptr };
inline const char* kGetXSigs[]  = { "()I", nullptr };
inline MethodDesc METHOD_BLOCKPOS_X{ CLS_BLOCKPOS, kGetXNames, kGetXSigs };

inline const char* kGetYNames[] = { "getY", "func_177956_o", nullptr };
inline const char* kGetYSigs[]  = { "()I", nullptr };
inline MethodDesc METHOD_BLOCKPOS_Y{ CLS_BLOCKPOS, kGetYNames, kGetYSigs };

inline const char* kGetZNames[] = { "getZ", "func_177952_p", nullptr };
inline const char* kGetZSigs[]  = { "()I", nullptr };
inline MethodDesc METHOD_BLOCKPOS_Z{ CLS_BLOCKPOS, kGetZNames, kGetZSigs };

// ── Methods on Minecraft ──────────────────────────────────────────────────────

inline const char* kDisplayGuiScreenNames[] = { "displayGuiScreen", "func_147108_a", nullptr };
inline const char* kDisplayGuiScreenSigs[]  = {
    "(Lnet/minecraft/client/gui/GuiScreen;)V", nullptr
};
inline MethodDesc METHOD_DISPLAY_GUI_SCREEN{ CLS_MC, kDisplayGuiScreenNames, kDisplayGuiScreenSigs };

// ── Methods on FloatBuffer ────────────────────────────────────────────────────

inline const char* kFloatBufGetNames[] = { "get", nullptr };
inline const char* kFloatBufGetSigs[]  = { "(I)F", nullptr };
inline MethodDesc METHOD_FLOAT_BUF_GET{ CLS_FLOAT_BUFFER, kFloatBufGetNames, kFloatBufGetSigs };

// ── Reset all (call during remap) ─────────────────────────────────────────────

inline void ResetAll(JNIEnv* env) {
    ResetDesc(CLS_MC, env);
    ResetDesc(CLS_ENTITY, env);
    ResetDesc(CLS_PLAYER, env);
    ResetDesc(CLS_WORLD, env);
    ResetDesc(CLS_BLOCKPOS, env);
    ResetDesc(CLS_TILE_ENTITY, env);
    ResetDesc(CLS_GUI_CHAT, env);
    ResetDesc(CLS_GUI_SCREEN, env);
    ResetDesc(CLS_ACTIVE_RENDER_INFO, env);
    ResetDesc(CLS_FLOAT_BUFFER, env);

    ResetDesc(FIELD_PLAYER);   ResetDesc(FIELD_SCREEN);
    ResetDesc(FIELD_WORLD);    ResetDesc(FIELD_POS_X);
    ResetDesc(FIELD_POS_Y);    ResetDesc(FIELD_POS_Z);
    ResetDesc(FIELD_ROT_YAW);  ResetDesc(FIELD_ROT_PITCH);
    ResetDesc(FIELD_PLAYER_ENTITIES); ResetDesc(FIELD_TILE_ENTITY_LIST);
    ResetDesc(FIELD_TE_POS);

    ResetDesc(METHOD_GET_HEALTH);
    ResetDesc(METHOD_BLOCKPOS_X); ResetDesc(METHOD_BLOCKPOS_Y); ResetDesc(METHOD_BLOCKPOS_Z);
    ResetDesc(METHOD_DISPLAY_GUI_SCREEN);
    ResetDesc(METHOD_FLOAT_BUF_GET);
}

} // namespace mc18
