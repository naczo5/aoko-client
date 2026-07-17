#pragma once

#include <jni.h>
#include <string>

namespace lc {

enum class NickHiderJvmtiGeneration {
    Legacy189,
    Modern121
};

// The render callback reads this cache without taking bridge locks.  Call these
// only from the TCP/config or background JNI threads, never from a render path.
void ConfigureNickHiderJvmti(bool enabled, const std::string& alias);
void SetNickHiderJvmtiLocalName(const std::string& localName);

// Installs a narrow JVMTI breakpoint interceptor.  It does not modify loaded
// game classes or GameProfile; text is replaced only while a known vanilla HUD
// surface appears in the current render call stack.
bool InstallNickHiderJvmti(JavaVM* vm, NickHiderJvmtiGeneration generation,
                           void (*logger)(const std::string&));
bool IsNickHiderJvmtiInstalled();
void RefreshNickHiderJvmtiTargets();
void FlushNickHiderJvmtiDiagnostics();
void ShutdownNickHiderJvmti(JNIEnv* env);

} // namespace lc
