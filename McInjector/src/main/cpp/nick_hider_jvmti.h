#pragma once

#include <jni.h>
#include <jvmti.h>
#include <string>

namespace lc {

enum class NickHiderJvmtiGeneration {
    Legacy189,
    Modern121
};

typedef void (*JvmtiBreakpointHandler)(jvmtiEnv* jvmti, JNIEnv* env, jthread thread,
                                       jmethodID method, jlocation location);
typedef void (*JvmtiFramePopHandler)(jvmtiEnv* jvmti, JNIEnv* env, jthread thread,
                                     jmethodID method, jboolean wasPoppedByException);

// The render callback reads this cache without taking bridge locks.  Call these
// only from the TCP/config or background JNI threads, never from a render path.
void ConfigureNickHiderJvmti(bool enabled, const std::string& alias);
void SetNickHiderJvmtiLocalName(const std::string& localName);

// Installs a shared JVMTI host used by NickHider + KillAura PreMotion.
// Capabilities are acquired one-at-a-time from GetPotentialCapabilities so a
// single denied bit (e.g. local-vars) cannot poison breakpoint acquisition.
bool InstallNickHiderJvmti(JavaVM* vm, NickHiderJvmtiGeneration generation,
                           void (*logger)(const std::string&));
bool IsNickHiderJvmtiInstalled();
bool HasJvmtiBreakpoints();
bool HasJvmtiFramePop();
bool HasJvmtiLocalVariables();
bool HasJvmtiGetBytecodes();
bool HasJvmtiRetransform();
void RefreshNickHiderJvmtiTargets();
void FlushNickHiderJvmtiDiagnostics();
void ShutdownNickHiderJvmti(JNIEnv* env);

// Shared JVMTI helpers for other 1.8 modules (KillAura PreMotion Silent).
// Handlers run from the same Breakpoint/FramePop/ClassFileLoadHook callbacks as NickHider.
void RegisterJvmtiBreakpointHandler(JvmtiBreakpointHandler handler);
void RegisterJvmtiFramePopHandler(JvmtiFramePopHandler handler);

typedef void (*ClassFileLoadHookFn)(jvmtiEnv* jvmti, JNIEnv* env, jclass classBeingRedefined,
                                    jobject loader, const char* name, jobject protectionDomain,
                                    jint classDataLen, const unsigned char* classData,
                                    jint* newClassDataLen, unsigned char** newClassData);
// Registers a transformation subscriber and returns a positive token. Subscribers
// are chained in registration order so independent modules cannot replace each
// other's ClassFileLoadHook callback.
int RegisterClassFileLoadHook(ClassFileLoadHookFn handler);
void UnregisterClassFileLoadHook(int token);

jvmtiEnv* SharedJvmtiEnv();
bool SharedJvmtiSetBreakpoint(jmethodID method, jlocation location);
bool SharedJvmtiClearBreakpoint(jmethodID method, jlocation location);
bool SharedJvmtiNotifyFramePop(jthread thread, jint depth);
bool SharedJvmtiRetransformClasses(jclass* classes, jint count);
bool SharedJvmtiAllocate(jlong size, unsigned char** memPtr);

} // namespace lc
