#pragma once

#include <jni.h>
#include <string>

namespace ka_premotion {

// PreMotion Silent for KillAura — OpenMyau-Plus aligned
// (https://github.com/IamNespola/OpenMyau-Plus):
// OpenMyau Silent temporarily overrides look only while the walking/motion
// packet is built, then restores the client camera. LockView alone moves the camera.
//
// Shared neutral JVMTI broker: use a walking-method breakpoint when available;
// late-attached JVMs use a schema-preserving outbound packet retransformation.
// The tiny native callback class is generated in memory (no Java-agent artifact).

typedef bool (*AttackHandler)(JNIEnv* env, jobject selfPlayer, jobject target);

enum HookBackend {
    HOOK_UNAVAILABLE = 0,
    HOOK_BREAKPOINT = 1,
    HOOK_PACKET_RETRANSFORM = 2
};

void Install(JavaVM* vm, void (*logger)(const std::string&));
void Shutdown(JNIEnv* env);
void RefreshTargets(JNIEnv* env);

// Bind rotation field IDs resolved by the bridge (may be null for optional head/body).
void BindRotationFields(jfieldID yaw, jfieldID pitch, jfieldID yawHead, jfieldID renderYawOffset);

// Modern bridges often expose getYRot/setYRot instead of raw fields.
// Used when BindRotationFields yaw/pitch are null.
void BindRotationMethods(jmethodID getYaw, jmethodID getPitch,
                         jmethodID setYaw, jmethodID setPitch,
                         jmethodID setBodyYaw /* may be null */);

// When local-variable JVMTI access is unavailable, walking Premotion reads MC.thePlayer.
void BindMcPlayerLookup(jobject mcInstanceGlobal, jfieldID thePlayerField);

// Retained adapter binding for mapping diagnostics; the clean broker stamps the player.
void BindC03LookFields(jclass c03ClassGlobal, jfieldID yawField, jfieldID pitchField,
                       jfieldID hasRotField /* may be null */);

// Bridge registers attack handler (1.8.9: OpenMyau C0A→sync→C02→local;
// modern: MultiPlayerGameMode.attack).
void SetAttackHandler(AttackHandler handler);

// Resolves/arms the breakpoint or packet-retransformation backend.
bool ArmSendQueueHook(JNIEnv* env, jclass netHandlerClass);

// Called from the KillAura update loop (TCP/JNI thread). Not from JVMTI callbacks.
void SetSilentCombatAngles(bool silentEngaged, float yaw, float pitch, float bodyYaw, bool valid);

// Queue a packet attack to fire on the next outbound look/move packet (or walking entry).
bool QueueAttack(JNIEnv* env, jobject target);
void ClearPendingAttack(JNIEnv* env);

// World/server switches (config phase, loading screens) tear down player/C03 JNI
// while the send-queue hook can still fire. Suspend no-ops NativeOnPacket and
// clears pending/silent state before the bridge deletes C03 globals.
void SuspendForWorldChange(JNIEnv* env);
void ResumeAfterWorldChange();
bool IsSuspended();

bool IsReady();
bool IsOperational();
HookBackend GetBackend();
unsigned long long LastMovementCallbackMs();
bool InstallFailed();
bool IsSendQueueHookReady();

} // namespace ka_premotion
