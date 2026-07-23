#include "kill_aura_premotion.h"
#include "nick_hider_jvmti.h"
#include "kill_aura_core.h"
#include "classfile_entry_injector.h"

#include <jvmti.h>
#include <windows.h>
#include <cmath>
#include <cstring>

namespace ka_premotion {
namespace {

static void (*s_log)(const std::string&) = nullptr;
static AttackHandler s_attackHandler = nullptr;
static volatile LONG s_handlersRegistered = 0;
static volatile LONG s_ready = 0; // walking-player breakpoint path
static volatile LONG s_sendQueueReady = 0;
static volatile LONG s_sendQueueHookRegistered = 0;
static volatile LONG s_suspended = 0; // world-change / reconfig: NativeOnPacket no-ops
static int s_sendQueueHookToken = 0;
static volatile LONG s_sendQueueInjected = 0;
static volatile LONG s_inSendQueueNative = 0;
static volatile LONG s_loggedSendQueueReady = 0;
static volatile LONGLONG s_sendQueueArmedMs = 0;
static volatile LONGLONG s_lastMovementCallbackMs = 0;
static volatile LONG s_installFailed = 0;
static volatile LONG s_loggedFail = 0;
static volatile LONG s_loggedReady = 0;
static volatile LONG s_loggedNoLocals = 0;
static volatile LONG s_loggedReturnSites = 0;
static volatile LONG s_loggedHeartbeat = 0;
static volatile LONG s_loggedQueuedAttack = 0;
static volatile LONG s_loggedFiredAttack = 0;

static jclass s_c03Class = nullptr; // global
static jfieldID s_c03Yaw = nullptr;
static jfieldID s_c03Pitch = nullptr;
static jfieldID s_c03HasRot = nullptr;
static jclass s_hookClass = nullptr; // global

static jmethodID s_walkingMethod = nullptr;
static jfieldID s_yawField = nullptr;
static jfieldID s_pitchField = nullptr;
static jfieldID s_yawHeadField = nullptr;
static jfieldID s_bodyField = nullptr;
static jmethodID s_getYawMethod = nullptr;
static jmethodID s_getPitchMethod = nullptr;
static jmethodID s_setYawMethod = nullptr;
static jmethodID s_setPitchMethod = nullptr;
static jmethodID s_setBodyYawMethod = nullptr;

static jobject s_mcInstance = nullptr; // global ref owned by bridge; do not delete
static jfieldID s_thePlayerField = nullptr;

static volatile LONG s_silentEngaged = 0;
static volatile LONG s_combatValid = 0;
static float s_combatYaw = 0.0f;
static float s_combatPitch = 0.0f;
static float s_combatBody = 0.0f;

static volatile LONG s_inStamp = 0;
static jobject s_stampedPlayer = nullptr; // global ref while stamp active
static float s_savedYaw = 0.0f;
static float s_savedPitch = 0.0f;
static float s_savedHead = 0.0f;
static float s_savedBody = 0.0f;
static volatile LONG s_savedHeadValid = 0;
static volatile LONG s_savedBodyValid = 0;

static CRITICAL_SECTION s_pendingCs;
static volatile LONG s_pendingCsInit = 0;
static jobject s_pendingTarget = nullptr; // global ref
static volatile LONG s_pendingAttack = 0;

static const int kMaxReturnSites = 32;
static jlocation s_returnSites[kMaxReturnSites] = {};
static int s_returnSiteCount = 0;

static void EnsurePendingCs()
{
    if (InterlockedCompareExchange(&s_pendingCsInit, 1, 0) == 0)
        InitializeCriticalSection(&s_pendingCs);
}

static void Log(const std::string& msg)
{
    if (s_log) s_log(msg);
}

static bool SignatureIsClientPlayer(const char* signature)
{
    if (!signature) return false;
    return std::strcmp(signature, "Lnet/minecraft/client/entity/EntityPlayerSP;") == 0
        || std::strcmp(signature, "Lnet/minecraft/client/network/ClientPlayerEntity;") == 0
        || std::strcmp(signature, "Lnet/minecraft/class_746;") == 0
        || std::strcmp(signature, "Lnet/minecraft/client/player/LocalPlayer;") == 0;
}

static bool MethodIsSendMovementPackets(const char* name, const char* signature)
{
    if (!name || !signature) return false;
    if (std::strcmp(signature, "()V") != 0) return false;
    return std::strcmp(name, "onUpdateWalkingPlayer") == 0
        || std::strcmp(name, "func_175161_p") == 0
        || std::strcmp(name, "sendMovementPackets") == 0
        || std::strcmp(name, "method_3136") == 0
        || std::strcmp(name, "sendPosition") == 0;
}

static bool HasLookWriters()
{
    return (s_yawField && s_pitchField) || (s_setYawMethod && s_setPitchMethod);
}

static bool HasLookReaders()
{
    return (s_yawField && s_pitchField) || (s_getYawMethod && s_getPitchMethod);
}

static bool ReadLook(JNIEnv* env, jobject player, float* yaw, float* pitch)
{
    if (!env || !player || !yaw || !pitch) return false;
    if (s_yawField && s_pitchField) {
        *yaw = env->GetFloatField(player, s_yawField);
        *pitch = env->GetFloatField(player, s_pitchField);
        if (env->ExceptionCheck()) {
            env->ExceptionClear();
            return false;
        }
        return true;
    }
    if (s_getYawMethod && s_getPitchMethod) {
        *yaw = env->CallFloatMethod(player, s_getYawMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); return false; }
        *pitch = env->CallFloatMethod(player, s_getPitchMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); return false; }
        return true;
    }
    return false;
}

static void WriteLook(JNIEnv* env, jobject player, float yaw, float pitch, float bodyYaw, bool writeBody)
{
    if (!env || !player) return;
    const float writeYaw = killaura::Wrap(yaw);
    const float writePitch = killaura::Clamp(pitch, -90.0f, 90.0f);
    if (s_yawField) {
        env->SetFloatField(player, s_yawField, writeYaw);
        if (env->ExceptionCheck()) env->ExceptionClear();
    } else if (s_setYawMethod) {
        env->CallVoidMethod(player, s_setYawMethod, writeYaw);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (s_pitchField) {
        env->SetFloatField(player, s_pitchField, writePitch);
        if (env->ExceptionCheck()) env->ExceptionClear();
    } else if (s_setPitchMethod) {
        env->CallVoidMethod(player, s_setPitchMethod, writePitch);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (s_yawHeadField) {
        env->SetFloatField(player, s_yawHeadField, writeYaw);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (writeBody && s_bodyField) {
        env->SetFloatField(player, s_bodyField, killaura::Wrap(bodyYaw));
        if (env->ExceptionCheck()) env->ExceptionClear();
    } else if (writeBody && s_setBodyYawMethod) {
        env->CallVoidMethod(player, s_setBodyYawMethod, killaura::Wrap(bodyYaw));
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
}

static jobject TakePendingTarget(JNIEnv* env)
{
    if (!env) return nullptr;
    EnsurePendingCs();
    EnterCriticalSection(&s_pendingCs);
    jobject target = nullptr;
    if (InterlockedExchange(&s_pendingAttack, 0) != 0 && s_pendingTarget) {
        target = s_pendingTarget;
        s_pendingTarget = nullptr;
    }
    LeaveCriticalSection(&s_pendingCs);
    return target;
}

static void FirePendingAttack(JNIEnv* env, jobject player)
{
    if (!env || !player || !s_attackHandler) return;
    jobject targetGlobal = TakePendingTarget(env);
    if (!targetGlobal) return;

    s_attackHandler(env, player, targetGlobal);
    if (InterlockedCompareExchange(&s_loggedFiredAttack, 1, 0) == 0)
        Log("KillAura PreMotion: fired queued attack before movement send");
    env->DeleteGlobalRef(targetGlobal);
}

// Prefer JVMTI locals (this); fall back to Minecraft.thePlayer when local-vars cap is missing.
static jobject ResolvePlayer(jvmtiEnv* jvmti, JNIEnv* env, jthread thread)
{
    if (!env) return nullptr;

    if (lc::HasJvmtiLocalVariables() && jvmti && thread) {
        jobject player = nullptr;
        if (jvmti->GetLocalObject(thread, 0, 0, &player) == JVMTI_ERROR_NONE && player)
            return player;
        if (env->ExceptionCheck()) env->ExceptionClear();
    } else if (InterlockedCompareExchange(&s_loggedNoLocals, 1, 0) == 0) {
        Log("KillAura PreMotion: resolving player via Minecraft.thePlayer (no JVMTI local-vars)");
    }

    if (!s_mcInstance || !s_thePlayerField) return nullptr;
    jobject player = env->GetObjectField(s_mcInstance, s_thePlayerField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        return nullptr;
    }
    return player;
}

static void ClearReturnSiteBreakpoints()
{
    if (!s_walkingMethod) {
        s_returnSiteCount = 0;
        return;
    }
    for (int i = 0; i < s_returnSiteCount; ++i)
        lc::SharedJvmtiClearBreakpoint(s_walkingMethod, s_returnSites[i]);
    s_returnSiteCount = 0;
}

static bool InstallReturnSiteBreakpoints(jvmtiEnv* jvmti, jmethodID method)
{
    ClearReturnSiteBreakpoints();
    if (!jvmti || !method || !lc::HasJvmtiGetBytecodes()) return false;

    jint bytecodeCount = 0;
    unsigned char* bytecodes = nullptr;
    if (jvmti->GetBytecodes(method, &bytecodeCount, &bytecodes) != JVMTI_ERROR_NONE || !bytecodes)
        return false;

    // Void method: look for return (0xb1) and athrow (0xbf) opcodes.
    // Skip wide prefixes / multi-byte opcodes with a minimal walker.
    int installed = 0;
    for (jint i = 0; i < bytecodeCount; ) {
        const unsigned char op = bytecodes[i];
        if (op == 0xb1 || op == 0xbf) {
            if (installed < kMaxReturnSites &&
                lc::SharedJvmtiSetBreakpoint(method, (jlocation)i)) {
                s_returnSites[installed++] = (jlocation)i;
            }
            ++i;
            continue;
        }
        // Minimal opcode size table for common 1.8 bytecodes (enough to not skip returns).
        static const unsigned char kSizes[256] = {
            /* 0x00-0x0f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x10-0x1f */ 2,3,2,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x20-0x2f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x30-0x3f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x40-0x4f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x50-0x5f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x60-0x6f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x70-0x7f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x80-0x8f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0x90-0x9f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0xa0-0xaf */ 3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,
            /* 0xb0-0xbf */ 1,1,1,1,1,1,1,1,1,3,2,1,1,1,1,1,
            /* 0xc0-0xcf */ 2,2,3,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0xd0-0xdf */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0xe0-0xef */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
            /* 0xf0-0xff */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        };
        unsigned char step = kSizes[op];
        if (op == 0xc4) { // wide
            if (i + 1 >= bytecodeCount) break;
            const unsigned char wop = bytecodes[i + 1];
            step = (wop == 0x84) ? 6 : 4; // iinc vs load/store
        } else if (op == 0xaa) { // tableswitch — align + skip; rare in walking player
            break;
        } else if (op == 0xab) { // lookupswitch
            break;
        }
        if (step < 1) step = 1;
        i += step;
    }

    jvmti->Deallocate(bytecodes);
    s_returnSiteCount = installed;
    return installed > 0;
}

static bool TryBeginSilentStamp(jvmtiEnv* jvmti, JNIEnv* env, jthread thread, jobject player)
{
    if (!InterlockedCompareExchange(&s_silentEngaged, 0, 0)) return false;
    if (!InterlockedCompareExchange(&s_combatValid, 0, 0)) return false;
    if (!HasLookWriters() || !HasLookReaders()) return false;
    if (InterlockedCompareExchange(&s_inStamp, 0, 0)) return false;

    float yaw = 0.0f, pitch = 0.0f;
    if (!ReadLook(env, player, &yaw, &pitch))
        return false;

    s_savedYaw = yaw;
    s_savedPitch = pitch;
    s_savedHeadValid = 0;
    s_savedBodyValid = 0;
    if (s_yawHeadField) {
        s_savedHead = env->GetFloatField(player, s_yawHeadField);
        if (env->ExceptionCheck()) env->ExceptionClear();
        else s_savedHeadValid = 1;
    }
    if (s_bodyField) {
        s_savedBody = env->GetFloatField(player, s_bodyField);
        if (env->ExceptionCheck()) env->ExceptionClear();
        else s_savedBodyValid = 1;
    }

    WriteLook(env, player, s_combatYaw, s_combatPitch, s_combatBody, true);

    // Prefer frame-pop restore; otherwise return-site breakpoints handle exit.
    const bool armedFramePop = lc::SharedJvmtiNotifyFramePop(thread, 0);
    if (!armedFramePop && s_returnSiteCount <= 0) {
        // No reliable restore path — undo stamp so camera is not left on combat angles.
        WriteLook(env, player, s_savedYaw, s_savedPitch,
                  s_savedBodyValid ? s_savedBody : s_savedYaw, s_savedBodyValid != 0);
        if (s_yawHeadField && s_savedHeadValid) {
            env->SetFloatField(player, s_yawHeadField, s_savedHead);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }
        return false;
    }

    if (s_stampedPlayer) {
        env->DeleteGlobalRef(s_stampedPlayer);
        s_stampedPlayer = nullptr;
    }
    s_stampedPlayer = env->NewGlobalRef(player);
    InterlockedExchange(&s_inStamp, 1);
    return true;
}

static void OnWalkingEntry(jvmtiEnv* jvmti, JNIEnv* env, jthread thread)
{
    if (!env || !thread || !jvmti) return;
    if (InterlockedCompareExchange(&s_suspended, 0, 0) != 0) return;

    const bool silentWanted = InterlockedCompareExchange(&s_silentEngaged, 0, 0) != 0
        && InterlockedCompareExchange(&s_combatValid, 0, 0) != 0;
    const bool pendingWanted = InterlockedCompareExchange(&s_pendingAttack, 0, 0) != 0;
    if (!silentWanted && !pendingWanted) return;

    jobject player = ResolvePlayer(jvmti, env, thread);
    if (!player) return;

    bool stamped = false;
    if (silentWanted)
        stamped = TryBeginSilentStamp(jvmti, env, thread, player);

    // OpenMyau: attack in pre-motion (before this method sends C03).
    if (pendingWanted) {
        if (!silentWanted || stamped)
            FirePendingAttack(env, player);
    }

    env->DeleteLocalRef(player);
}

static void OnWalkingExit(JNIEnv* env)
{
    if (!env) return;
    if (!InterlockedCompareExchange(&s_inStamp, 0, 0)) return;

    jobject player = s_stampedPlayer;
    if (player && HasLookWriters()) {
        WriteLook(env, player, s_savedYaw, s_savedPitch,
                  s_savedBodyValid ? s_savedBody : s_savedYaw, false);
        if (s_yawHeadField && s_savedHeadValid) {
            env->SetFloatField(player, s_yawHeadField, s_savedHead);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }
        if (s_bodyField && s_savedBodyValid) {
            env->SetFloatField(player, s_bodyField, s_savedBody);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }
    }
    if (s_stampedPlayer) {
        env->DeleteGlobalRef(s_stampedPlayer);
        s_stampedPlayer = nullptr;
    }

    InterlockedExchange(&s_inStamp, 0);
}

static bool IsReturnSite(jlocation location)
{
    for (int i = 0; i < s_returnSiteCount; ++i)
        if (s_returnSites[i] == location) return true;
    return false;
}

static void JNICALL OnBreakpoint(jvmtiEnv* jvmti, JNIEnv* env, jthread thread,
                                 jmethodID method, jlocation location)
{
    if (!s_walkingMethod || method != s_walkingMethod) return;
    if (location == 0)
        OnWalkingEntry(jvmti, env, thread);
    else if (IsReturnSite(location))
        OnWalkingExit(env);
}

static void JNICALL OnFramePop(jvmtiEnv*, JNIEnv* env, jthread,
                               jmethodID method, jboolean)
{
    if (!s_walkingMethod || method != s_walkingMethod) return;
    OnWalkingExit(env);
}

#if 1 // Retransformation backend for JVMs that deny late-attach breakpoint capabilities.
static bool IsMotionLookPacket(JNIEnv* env, jobject packet)
{
    if (!env || !packet || !s_c03Class) return false;
    return env->IsInstanceOf(packet, s_c03Class) == JNI_TRUE;
}

static void PatchMotionLook(JNIEnv* env, jobject packet, float yaw, float pitch)
{
    if (!env || !packet || !s_c03Yaw || !s_c03Pitch) return;
    if (s_c03HasRot) {
        env->SetBooleanField(packet, s_c03HasRot, JNI_TRUE);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    env->SetFloatField(packet, s_c03Yaw, killaura::Wrap(yaw));
    if (env->ExceptionCheck()) env->ExceptionClear();
    env->SetFloatField(packet, s_c03Pitch, killaura::Clamp(pitch, -90.0f, 90.0f));
    if (env->ExceptionCheck()) env->ExceptionClear();
}

static jobject ResolveMcPlayer(JNIEnv* env)
{
    if (!env || !s_mcInstance || !s_thePlayerField) return nullptr;
    jobject player = env->GetObjectField(s_mcInstance, s_thePlayerField);
    if (env->ExceptionCheck() || !player) {
        env->ExceptionClear();
        return nullptr;
    }
    return player;
}

// Temporary stamp for GameMode.attack when send-queue path fires without walking stamp.
static bool BeginTempAttackStamp(JNIEnv* env, jobject player, float* outYaw, float* outPitch)
{
    if (!env || !player || !outYaw || !outPitch) return false;
    if (!HasLookWriters() || !HasLookReaders()) return false;
    if (!ReadLook(env, player, outYaw, outPitch)) return false;
    WriteLook(env, player, s_combatYaw, s_combatPitch, s_combatBody, true);
    return true;
}

static void EndTempAttackStamp(JNIEnv* env, jobject player, float yaw, float pitch)
{
    if (!env || !player) return;
    WriteLook(env, player, yaw, pitch, yaw, false);
}

static void JNICALL NativeOnPacket(JNIEnv* env, jclass, jobject packet)
{
    if (!env || !packet) return;
    // World/reconfig transitions delete C03/player JNI while this hook stays
    // injected into ClientPacketListener.send — bail before any IsInstanceOf.
    if (InterlockedCompareExchange(&s_suspended, 0, 0) != 0) return;
    if (InterlockedCompareExchange(&s_inSendQueueNative, 1, 0) != 0)
        return; // ignore re-entrant packets we enqueue ourselves

    const bool silentWanted = InterlockedCompareExchange(&s_silentEngaged, 0, 0) != 0
        && InterlockedCompareExchange(&s_combatValid, 0, 0) != 0;
    const bool isLookPkt = IsMotionLookPacket(env, packet);

    if (isLookPkt) {
        InterlockedExchange64(&s_lastMovementCallbackMs, (LONGLONG)GetTickCount64());
        if (InterlockedCompareExchange(&s_loggedHeartbeat, 1, 0) == 0)
            Log("KillAura PreMotion: movement callback heartbeat received");
    }

    if (isLookPkt && silentWanted)
        PatchMotionLook(env, packet, s_combatYaw, s_combatPitch);

    if (isLookPkt && InterlockedCompareExchange(&s_pendingAttack, 0, 0) != 0) {
        jobject player = ResolveMcPlayer(env);
        if (player) {
            const bool alreadyStamped = InterlockedCompareExchange(&s_inStamp, 0, 0) != 0;
            float savedYaw = 0.0f, savedPitch = 0.0f;
            bool tempStamped = false;
            if (silentWanted && !alreadyStamped)
                tempStamped = BeginTempAttackStamp(env, player, &savedYaw, &savedPitch);

            FirePendingAttack(env, player);

            if (tempStamped)
                EndTempAttackStamp(env, player, savedYaw, savedPitch);
            env->DeleteLocalRef(player);
        }
    }

    InterlockedExchange(&s_inSendQueueNative, 0);
}

static bool IsSendHookTargetClass(const char* name)
{
    if (!name) return false;
    return std::strcmp(name, "net/minecraft/client/network/NetHandlerPlayClient") == 0
        || std::strcmp(name, "net/minecraft/client/network/ClientPlayNetworkHandler") == 0
        || std::strcmp(name, "net/minecraft/class_634") == 0
        || std::strcmp(name, "net/minecraft/client/multiplayer/ClientPacketListener") == 0
        || std::strcmp(name, "net/minecraft/network/ClientConnection") == 0
        || std::strcmp(name, "net/minecraft/class_2535") == 0
        || std::strcmp(name, "net/minecraft/network/Connection") == 0;
}

static cfinject::InjectResult TryInjectSendHook(const unsigned char* classData, size_t classDataLen)
{
    const std::vector<std::string> methods = {
        "addToSendQueue", "func_147297_a", "sendPacket", "method_2883",
        "send", "m_129507_"
    };
    return cfinject::InjectPacketCallbackAtEntry(
        classData, classDataLen, methods, "lc/aoko/NativePacketHook");
}

static void OnClassFileLoadHook(jvmtiEnv* /*jvmti*/, JNIEnv* /*env*/, jclass /*classBeingRedefined*/,
                                jobject /*loader*/, const char* name, jobject /*protectionDomain*/,
                                jint classDataLen, const unsigned char* classData,
                                jint* newClassDataLen, unsigned char** newClassData)
{
    if (!name || !classData || classDataLen <= 0 || !newClassDataLen || !newClassData)
        return;
    if (!IsSendHookTargetClass(name))
        return;

    cfinject::InjectResult injected = TryInjectSendHook(classData, (size_t)classDataLen);
    if (!injected.ok) {
        Log(std::string("KillAura PreMotion: send-hook inject failed on ") + name + ": " + injected.error);
        return;
    }

    unsigned char* mem = nullptr;
    if (!lc::SharedJvmtiAllocate((jlong)injected.bytes.size(), &mem) || !mem) {
        Log("KillAura PreMotion: JVMTI Allocate failed for rewritten class");
        return;
    }
    std::memcpy(mem, injected.bytes.data(), injected.bytes.size());
    *newClassData = mem;
    *newClassDataLen = (jint)injected.bytes.size();
    InterlockedExchange(&s_sendQueueInjected, 1);
}

#endif

static bool TryBindWalkingMethod(JNIEnv* env, jclass playerSpCls)
{
    if (!env || !playerSpCls) return false;
    jvmtiEnv* jvmti = lc::SharedJvmtiEnv();
    if (!jvmti || !lc::HasJvmtiBreakpoints()) return false;

    jint methodCount = 0;
    jmethodID* methods = nullptr;
    if (jvmti->GetClassMethods(playerSpCls, &methodCount, &methods) != JVMTI_ERROR_NONE || !methods)
        return false;

    jmethodID found = nullptr;
    for (jint i = 0; i < methodCount; ++i) {
        char* name = nullptr;
        char* signature = nullptr;
        if (jvmti->GetMethodName(methods[i], &name, &signature, nullptr) != JVMTI_ERROR_NONE) continue;
        const bool match = MethodIsSendMovementPackets(name, signature);
        if (name) jvmti->Deallocate(reinterpret_cast<unsigned char*>(name));
        if (signature) jvmti->Deallocate(reinterpret_cast<unsigned char*>(signature));
        if (match) {
            found = methods[i];
            break;
        }
    }
    jvmti->Deallocate(reinterpret_cast<unsigned char*>(methods));
    if (!found) return false;

    if (s_walkingMethod && s_walkingMethod != found) {
        ClearReturnSiteBreakpoints();
        lc::SharedJvmtiClearBreakpoint(s_walkingMethod, 0);
    }

    if (!lc::SharedJvmtiSetBreakpoint(found, 0))
        return false;

    s_walkingMethod = found;

    // Frame-pop is preferred; otherwise plant return-site breakpoints for restore.
    if (!lc::HasJvmtiFramePop()) {
        if (InstallReturnSiteBreakpoints(jvmti, found)) {
            if (InterlockedCompareExchange(&s_loggedReturnSites, 1, 0) == 0)
                Log("KillAura PreMotion: using return-site breakpoints (no frame-pop cap)");
        } else if (InterlockedCompareExchange(&s_loggedFail, 1, 0) == 0) {
            Log("KillAura PreMotion: no frame-pop and no return-site breakpoints — Silent stamp disabled");
        }
    }

    InterlockedExchange(&s_ready, 1);
    InterlockedExchange(&s_installFailed, 0);
    if (InterlockedCompareExchange(&s_loggedReady, 1, 0) == 0)
        Log("KillAura PreMotion: hooked client sendMovementPackets / onUpdateWalkingPlayer");
    return true;
}

} // namespace

void Install(JavaVM* /*vm*/, void (*logger)(const std::string&))
{
    s_log = logger;
    EnsurePendingCs();
    if (!lc::IsNickHiderJvmtiInstalled()) {
        if (InterlockedCompareExchange(&s_loggedFail, 1, 0) == 0)
            Log("KillAura PreMotion: JVMTI host unavailable");
        InterlockedExchange(&s_installFailed, 1);
        InterlockedExchange(&s_ready, 0);
        InterlockedExchange(&s_sendQueueReady, 0);
        return;
    }

    if (!lc::HasJvmtiBreakpoints() && !lc::HasJvmtiRetransform()) {
        if (InterlockedCompareExchange(&s_loggedFail, 1, 0) == 0)
            Log("KillAura PreMotion: neither breakpoints nor retransform available");
        InterlockedExchange(&s_installFailed, 1);
        return;
    }

    if (lc::HasJvmtiBreakpoints() && InterlockedCompareExchange(&s_handlersRegistered, 1, 0) == 0) {
        lc::RegisterJvmtiBreakpointHandler(OnBreakpoint);
        lc::RegisterJvmtiFramePopHandler(OnFramePop);
    }
    InterlockedExchange(&s_installFailed, 0);
}

void Shutdown(JNIEnv* env)
{
    ClearReturnSiteBreakpoints();
    if (s_walkingMethod)
        lc::SharedJvmtiClearBreakpoint(s_walkingMethod, 0);
    s_walkingMethod = nullptr;
    if (env && s_stampedPlayer) {
        env->DeleteGlobalRef(s_stampedPlayer);
        s_stampedPlayer = nullptr;
    }
    if (env && s_hookClass) {
        env->DeleteGlobalRef(s_hookClass);
        s_hookClass = nullptr;
    }
    // s_c03Class is owned by bridge (global) — do not delete here.
    s_c03Class = nullptr;
    s_c03Yaw = nullptr;
    s_c03Pitch = nullptr;
    s_c03HasRot = nullptr;
    if (s_sendQueueHookToken) {
        lc::UnregisterClassFileLoadHook(s_sendQueueHookToken);
        s_sendQueueHookToken = 0;
    }
    s_getYawMethod = nullptr;
    s_getPitchMethod = nullptr;
    s_setYawMethod = nullptr;
    s_setPitchMethod = nullptr;
    s_setBodyYawMethod = nullptr;
    ClearPendingAttack(env);
    InterlockedExchange(&s_ready, 0);
    InterlockedExchange(&s_sendQueueReady, 0);
    InterlockedExchange(&s_suspended, 0);
    InterlockedExchange64(&s_sendQueueArmedMs, 0);
    InterlockedExchange64(&s_lastMovementCallbackMs, 0);
    InterlockedExchange(&s_silentEngaged, 0);
    InterlockedExchange(&s_combatValid, 0);
    InterlockedExchange(&s_inStamp, 0);
    InterlockedExchange(&s_inSendQueueNative, 0);
    InterlockedExchange(&s_loggedHeartbeat, 0);
    InterlockedExchange(&s_loggedQueuedAttack, 0);
    InterlockedExchange(&s_loggedFiredAttack, 0);
}

void RefreshTargets(JNIEnv* env)
{
    if (!env) return;
    if (!lc::IsNickHiderJvmtiInstalled()) {
        InterlockedExchange(&s_installFailed, 1);
        InterlockedExchange(&s_ready, 0);
        return;
    }
    // Walking stamp is OpenMyau-faithful; keep trying even when send-queue is armed.
    if (!lc::HasJvmtiBreakpoints())
        return;
    if (InterlockedCompareExchange(&s_handlersRegistered, 1, 0) == 0) {
        lc::RegisterJvmtiBreakpointHandler(OnBreakpoint);
        lc::RegisterJvmtiFramePopHandler(OnFramePop);
    }
    if (InterlockedCompareExchange(&s_ready, 0, 0) && s_walkingMethod)
        return;

    jvmtiEnv* jvmti = lc::SharedJvmtiEnv();
    if (!jvmti) return;

    jint classCount = 0;
    jclass* classes = nullptr;
    if (jvmti->GetLoadedClasses(&classCount, &classes) != JVMTI_ERROR_NONE || !classes) return;

    bool bound = false;
    for (jint i = 0; i < classCount && !bound; ++i) {
        char* signature = nullptr;
        if (jvmti->GetClassSignature(classes[i], &signature, nullptr) != JVMTI_ERROR_NONE || !signature)
            continue;
        if (SignatureIsClientPlayer(signature))
            bound = TryBindWalkingMethod(env, classes[i]);
        jvmti->Deallocate(reinterpret_cast<unsigned char*>(signature));
    }
    jvmti->Deallocate(reinterpret_cast<unsigned char*>(classes));
}

void BindRotationFields(jfieldID yaw, jfieldID pitch, jfieldID yawHead, jfieldID renderYawOffset)
{
    s_yawField = yaw;
    s_pitchField = pitch;
    s_yawHeadField = yawHead;
    s_bodyField = renderYawOffset;
}

void BindRotationMethods(jmethodID getYaw, jmethodID getPitch,
                         jmethodID setYaw, jmethodID setPitch,
                         jmethodID setBodyYaw)
{
    s_getYawMethod = getYaw;
    s_getPitchMethod = getPitch;
    s_setYawMethod = setYaw;
    s_setPitchMethod = setPitch;
    s_setBodyYawMethod = setBodyYaw;
}

void BindMcPlayerLookup(jobject mcInstanceGlobal, jfieldID thePlayerField)
{
    s_mcInstance = mcInstanceGlobal;
    s_thePlayerField = thePlayerField;
}

void BindC03LookFields(jclass c03ClassGlobal, jfieldID yawField, jfieldID pitchField, jfieldID hasRotField)
{
    s_c03Class = c03ClassGlobal;
    s_c03Yaw = yawField;
    s_c03Pitch = pitchField;
    s_c03HasRot = hasRotField;
}

static std::string DescribePendingException(JNIEnv* env)
{
    if (!env || !env->ExceptionCheck()) return {};
    jthrowable ex = env->ExceptionOccurred();
    env->ExceptionClear();
    if (!ex) return "unknown";

    std::string msg = "exception";
    jclass throwableCls = env->FindClass("java/lang/Throwable");
    if (throwableCls && !env->ExceptionCheck()) {
        jmethodID toString = env->GetMethodID(throwableCls, "toString", "()Ljava/lang/String;");
        if (toString && !env->ExceptionCheck()) {
            jstring jmsg = (jstring)env->CallObjectMethod(ex, toString);
            if (!env->ExceptionCheck() && jmsg) {
                const char* utf = env->GetStringUTFChars(jmsg, nullptr);
                if (utf) {
                    msg = utf;
                    env->ReleaseStringUTFChars(jmsg, utf);
                }
                env->DeleteLocalRef(jmsg);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
        env->DeleteLocalRef(throwableCls);
    } else if (env->ExceptionCheck()) {
        env->ExceptionClear();
    }
    env->DeleteLocalRef(ex);
    return msg;
}

bool ArmSendQueueHook(JNIEnv* env, jclass netHandlerClass)
{
    if (!env || !netHandlerClass) return false;
    // Prefer the exact walking-method breakpoint backend when the JVM grants
    // late-attach breakpoint capabilities. Otherwise use packet retransformation.
    RefreshTargets(env);
    if (IsReady()) return true;
#if 1
    if (InterlockedCompareExchange(&s_sendQueueReady, 0, 0)) return true;
    if (!lc::IsNickHiderJvmtiInstalled() || !lc::HasJvmtiRetransform()) {
        Log("KillAura PreMotion: retransform unavailable; cannot arm send-queue hook");
        return false;
    }

    if (InterlockedCompareExchange(&s_sendQueueHookRegistered, 1, 0) == 0) {
        s_sendQueueHookToken = lc::RegisterClassFileLoadHook(OnClassFileLoadHook);
        if (!s_sendQueueHookToken) {
            Log("KillAura PreMotion: class-file broker registration failed");
            return false;
        }
    }

    if (!s_hookClass) {
        // Prefer the game classloader so NetHandlerPlayClient can resolve lc/aoko/SendQueueHook.
        jobject loader = nullptr;
        jclass classCls = env->FindClass("java/lang/Class");
        if (classCls) {
            jmethodID getCl = env->GetMethodID(classCls, "getClassLoader", "()Ljava/lang/ClassLoader;");
            if (getCl && !env->ExceptionCheck())
                loader = env->CallObjectMethod(netHandlerClass, getCl);
            if (env->ExceptionCheck()) { env->ExceptionClear(); loader = nullptr; }
            env->DeleteLocalRef(classCls);
        }

        const std::vector<unsigned char> helperBytes =
            cfinject::BuildNativeCallbackClass("lc/aoko/NativePacketHook");
        jclass defined = env->DefineClass(
            "lc/aoko/NativePacketHook", loader,
            reinterpret_cast<const jbyte*>(helperBytes.data()),
            (jsize)helperBytes.size());
        if (env->ExceptionCheck() || !defined) {
            const std::string detail = DescribePendingException(env);
            defined = nullptr;
            // Fallback: bootstrap loader (null). Parent-first loaders can still see it.
            defined = env->DefineClass(
                "lc/aoko/NativePacketHook", nullptr,
                reinterpret_cast<const jbyte*>(helperBytes.data()),
                (jsize)helperBytes.size());
            if (env->ExceptionCheck() || !defined) {
                const std::string detail2 = DescribePendingException(env);
                if (loader) env->DeleteLocalRef(loader);
                Log(std::string("KillAura PreMotion: DefineClass(NativePacketHook) failed")
                    + (detail.empty() ? "" : ("; loader=" + detail))
                    + (detail2.empty() ? "" : ("; bootstrap=" + detail2)));
                return false;
            }
            Log(std::string("KillAura PreMotion: DefineClass(NativePacketHook) via bootstrap")
                + (detail.empty() ? "" : (" (game loader failed: " + detail + ")")));
        }
        if (loader) env->DeleteLocalRef(loader);
        s_hookClass = (jclass)env->NewGlobalRef(defined);
        env->DeleteLocalRef(defined);

        JNINativeMethod nm = {};
        nm.name = const_cast<char*>("onPacket");
        nm.signature = const_cast<char*>("(Ljava/lang/Object;)V");
        nm.fnPtr = (void*)&NativeOnPacket;
        if (env->RegisterNatives(s_hookClass, &nm, 1) != 0 || env->ExceptionCheck()) {
            const std::string detail = DescribePendingException(env);
            Log(std::string("KillAura PreMotion: RegisterNatives(NativePacketHook) failed")
                + (detail.empty() ? "" : (": " + detail)));
            return false;
        }
    }

    jclass globalNh = (jclass)env->NewGlobalRef(netHandlerClass);
    if (!globalNh) return false;
    InterlockedExchange(&s_sendQueueInjected, 0);
    const bool ok = lc::SharedJvmtiRetransformClasses(&globalNh, 1);
    env->DeleteGlobalRef(globalNh);
    if (!ok) {
        Log("KillAura PreMotion: RetransformClasses(NetHandlerPlayClient) failed");
        return false;
    }
    if (!InterlockedCompareExchange(&s_sendQueueInjected, 0, 0)) {
        Log("KillAura PreMotion: retransform completed but send method was not rewritten");
        return false;
    }

    InterlockedExchange(&s_sendQueueReady, 1);
    InterlockedExchange64(&s_sendQueueArmedMs, (LONGLONG)GetTickCount64());
    InterlockedExchange(&s_installFailed, 0);
    if (InterlockedCompareExchange(&s_loggedSendQueueReady, 1, 0) == 0)
        Log("KillAura PreMotion: packet retransformation backend armed");
    return true;
#endif
}

void SetAttackHandler(AttackHandler handler)
{
    s_attackHandler = handler;
}

void SetSilentCombatAngles(bool silentEngaged, float yaw, float pitch, float bodyYaw, bool valid)
{
    s_combatYaw = yaw;
    s_combatPitch = pitch;
    s_combatBody = bodyYaw;
    InterlockedExchange(&s_combatValid, valid ? 1 : 0);
    InterlockedExchange(&s_silentEngaged, (silentEngaged && valid) ? 1 : 0);
    if (!silentEngaged || !valid)
        InterlockedExchange(&s_inStamp, 0);
}

bool QueueAttack(JNIEnv* env, jobject target)
{
    if (!env || !target) return false;
    if (InterlockedCompareExchange(&s_suspended, 0, 0) != 0) return false;
    jobject global = env->NewGlobalRef(target);
    if (!global) return false;

    EnsurePendingCs();
    EnterCriticalSection(&s_pendingCs);
    if (s_pendingTarget) {
        env->DeleteGlobalRef(s_pendingTarget);
        s_pendingTarget = nullptr;
    }
    s_pendingTarget = global;
    InterlockedExchange(&s_pendingAttack, 1);
    LeaveCriticalSection(&s_pendingCs);
    if (InterlockedCompareExchange(&s_loggedQueuedAttack, 1, 0) == 0)
        Log("KillAura PreMotion: queued attack for next movement packet");
    return true;
}

void ClearPendingAttack(JNIEnv* env)
{
    EnsurePendingCs();
    EnterCriticalSection(&s_pendingCs);
    if (env && s_pendingTarget) {
        env->DeleteGlobalRef(s_pendingTarget);
        s_pendingTarget = nullptr;
    } else {
        s_pendingTarget = nullptr;
    }
    InterlockedExchange(&s_pendingAttack, 0);
    LeaveCriticalSection(&s_pendingCs);
}

void SuspendForWorldChange(JNIEnv* env)
{
    // Arm first so in-flight NativeOnPacket sees suspended before we clear JNI.
    const bool wasSuspended = InterlockedCompareExchange(&s_suspended, 1, 0) != 0;
    ClearPendingAttack(env);
    SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);

    // Drop stamp player global we own; do not DeleteGlobalRef C03 (bridge-owned).
    if (env && s_stampedPlayer) {
        env->DeleteGlobalRef(s_stampedPlayer);
        s_stampedPlayer = nullptr;
    }
    InterlockedExchange(&s_inStamp, 0);

    s_c03Class = nullptr;
    s_c03Yaw = nullptr;
    s_c03Pitch = nullptr;
    s_c03HasRot = nullptr;
    s_mcInstance = nullptr;
    s_thePlayerField = nullptr;

    if (!wasSuspended)
        Log("KillAura PreMotion: suspended for world change");
}

void ResumeAfterWorldChange()
{
    if (InterlockedCompareExchange(&s_suspended, 0, 1) != 0)
        Log("KillAura PreMotion: resumed after world change");
}

bool IsSuspended()
{
    return InterlockedCompareExchange(&s_suspended, 0, 0) != 0;
}

bool IsReady()
{
    if (InterlockedCompareExchange(&s_suspended, 0, 0) != 0) return false;
    return InterlockedCompareExchange(&s_sendQueueReady, 0, 0) != 0
        || InterlockedCompareExchange(&s_ready, 0, 0) != 0;
}

HookBackend GetBackend()
{
    if (InterlockedCompareExchange(&s_ready, 0, 0) != 0) return HOOK_BREAKPOINT;
    if (InterlockedCompareExchange(&s_sendQueueReady, 0, 0) != 0) return HOOK_PACKET_RETRANSFORM;
    return HOOK_UNAVAILABLE;
}

unsigned long long LastMovementCallbackMs()
{
    return (unsigned long long)InterlockedCompareExchange64(&s_lastMovementCallbackMs, 0, 0);
}

bool IsOperational()
{
    if (InterlockedCompareExchange(&s_suspended, 0, 0) != 0) return false;
    const HookBackend backend = GetBackend();
    if (backend == HOOK_BREAKPOINT) return true;
    if (backend != HOOK_PACKET_RETRANSFORM) return false;
    const unsigned long long now = GetTickCount64();
    const unsigned long long armed = (unsigned long long)InterlockedCompareExchange64(&s_sendQueueArmedMs, 0, 0);
    const unsigned long long callback = LastMovementCallbackMs();
    if (callback != 0) return now - callback <= 2500ULL;
    return armed != 0 && now - armed <= 4000ULL;
}

bool IsSendQueueHookReady()
{
    return InterlockedCompareExchange(&s_sendQueueReady, 0, 0) != 0;
}

bool InstallFailed()
{
    return InterlockedCompareExchange(&s_installFailed, 0, 0) != 0
        && !IsReady();
}

} // namespace ka_premotion
