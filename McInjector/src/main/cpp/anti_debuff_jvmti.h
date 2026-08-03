#pragma once

#include <jni.h>
#include <string>

namespace anti_debuff_jvmti {

void Install(JavaVM* vm, void (*logger)(const std::string&));
bool Arm(JNIEnv* env, jobject gameClassLoader);
void BindBlindness(JNIEnv* env, jobject blindnessHolder, jmethodID hasEffectMethod);
void SetEnabled(bool enabled);
// Returns false when vanilla renderer bytecode or an in-flight native callback could
// not be made safe. Callers must keep the DLL and shared JVMTI host loaded in that case.
bool Shutdown(JNIEnv* env);

} // namespace anti_debuff_jvmti
