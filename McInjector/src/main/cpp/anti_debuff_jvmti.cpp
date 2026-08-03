#include "anti_debuff_jvmti.h"

#include "effect_check_rewriter.h"
#include "nick_hider_jvmti.h"

#include <windows.h>
#include <jvmti.h>
#include <cstring>
#include <vector>

namespace anti_debuff_jvmti {
namespace {

static void (*s_log)(const std::string&) = nullptr;
static JavaVM* s_vm = nullptr;
static volatile LONG s_enabled = 0;
static volatile LONG s_installed = 0;
static volatile LONG s_armed = 0;
static volatile LONG s_injected = 0;
static volatile LONG s_callbacksInFlight = 0;
static int s_hookToken = 0;
static jclass s_helperClass = nullptr;
static jobject s_blindness = nullptr;
static jmethodID s_hasEffect = nullptr;
static INIT_ONCE s_lockOnce = INIT_ONCE_STATIC_INIT;
static CRITICAL_SECTION s_lock;

static BOOL CALLBACK InitLock(PINIT_ONCE, PVOID, PVOID*) {
    InitializeCriticalSection(&s_lock);
    return TRUE;
}

static void EnsureLock() {
    InitOnceExecuteOnce(&s_lockOnce, InitLock, nullptr, nullptr);
}

static void Log(const std::string& message) {
    if (s_log) s_log(message);
}

static bool IsTargetClassName(const char* name) {
    if (!name) return false;
    return std::strcmp(name, "net/minecraft/class_758$class_7286") == 0 ||
        std::strcmp(name, "net/minecraft/class_7286") == 0 ||
        std::strcmp(name, "net/minecraft/client/renderer/FogRenderer$MobEffectFogFunction") == 0 ||
        std::strcmp(name, "net/minecraft/client/renderer/fog/environment/MobEffectFogEnvironment") == 0 ||
        std::strcmp(name, "net/minecraft/client/renderer/EntityRenderer") == 0;
}

static bool IsTargetSignature(const char* signature) {
    if (!signature || signature[0] != 'L') return false;
    std::string name(signature + 1);
    if (!name.empty() && name[name.size() - 1] == ';') name.resize(name.size() - 1);
    return IsTargetClassName(name.c_str());
}

static std::string DescribePendingException(JNIEnv* env) {
    if (!env || !env->ExceptionCheck()) return {};
    jthrowable ex = env->ExceptionOccurred();
    env->ExceptionClear();
    if (!ex) return "unknown exception";

    std::string result = "exception";
    jclass throwableClass = env->FindClass("java/lang/Throwable");
    if (throwableClass && !env->ExceptionCheck()) {
        jmethodID toString = env->GetMethodID(
            throwableClass, "toString", "()Ljava/lang/String;");
        if (toString && !env->ExceptionCheck()) {
            jstring text = (jstring)env->CallObjectMethod(ex, toString);
            if (!env->ExceptionCheck() && text) {
                const char* utf = env->GetStringUTFChars(text, nullptr);
                if (utf) {
                    result = utf;
                    env->ReleaseStringUTFChars(text, utf);
                }
                env->DeleteLocalRef(text);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
        env->DeleteLocalRef(throwableClass);
    } else if (env->ExceptionCheck()) {
        env->ExceptionClear();
    }
    env->DeleteLocalRef(ex);
    return result;
}

static jboolean JNICALL NativeCheckEffect(JNIEnv* env, jclass, jobject entity, jobject effect) {
    struct CallbackLease {
        CallbackLease() { InterlockedIncrement(&s_callbacksInFlight); }
        ~CallbackLease() { InterlockedDecrement(&s_callbacksInFlight); }
    } lease;
    if (!env || !entity || !effect) return JNI_TRUE;

    jobject blindness = nullptr;
    jmethodID hasEffect = nullptr;
    EnsureLock();
    EnterCriticalSection(&s_lock);
    if (s_blindness) blindness = env->NewLocalRef(s_blindness);
    hasEffect = s_hasEffect;
    LeaveCriticalSection(&s_lock);

    const bool suppress = InterlockedCompareExchange(&s_enabled, 0, 0) != 0 &&
        blindness && env->IsSameObject(effect, blindness);
    if (blindness) env->DeleteLocalRef(blindness);
    if (suppress) return JNI_FALSE;

    if (!hasEffect) return JNI_TRUE;
    jboolean active = env->CallBooleanMethod(entity, hasEffect, effect);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        return JNI_TRUE; // Fail open: preserve vanilla fog/effect behavior.
    }
    return active;
}

static void OnClassFileLoadHook(jvmtiEnv*, JNIEnv*, jclass, jobject, const char* name,
                                jobject, jint classDataLen, const unsigned char* classData,
                                jint* newClassDataLen, unsigned char** newClassData) {
    if (!IsTargetClassName(name) || !classData || classDataLen <= 0 ||
        !newClassDataLen || !newClassData) return;

    effectrewrite::RewriteResult rewritten = effectrewrite::RewriteRendererEffectChecks(
        classData, (size_t)classDataLen);
    if (!rewritten.ok) {
        Log(std::string("AntiDebuff render hook: rewrite failed for ") + name +
            ": " + rewritten.error);
        return;
    }

    unsigned char* memory = nullptr;
    if (!lc::SharedJvmtiAllocate((jlong)rewritten.bytes.size(), &memory) || !memory) {
        Log("AntiDebuff render hook: JVMTI allocation failed");
        return;
    }
    std::memcpy(memory, rewritten.bytes.data(), rewritten.bytes.size());
    *newClassData = memory;
    *newClassDataLen = (jint)rewritten.bytes.size();
    InterlockedIncrement(&s_injected);
    Log(std::string("AntiDebuff render hook: rewrote ") + name + " (" +
        std::to_string(rewritten.rewrittenCalls) + " effect check(s))");
}

static jclass FindOrDefineHelper(JNIEnv* env, jobject gameClassLoader) {
    if (!env) return nullptr;
    const std::vector<unsigned char> bytes = effectrewrite::BuildNativeEffectCheckClass();
    jclass helper = env->DefineClass(
        "lc/aoko/AntiDebuffRenderHook", gameClassLoader,
        reinterpret_cast<const jbyte*>(bytes.data()), (jsize)bytes.size());
    if (!env->ExceptionCheck() && helper) return helper;

    const std::string defineError = DescribePendingException(env);
    if (!gameClassLoader) {
        Log(std::string("AntiDebuff render hook: helper definition failed: ") + defineError);
        return nullptr;
    }

    jclass loaderClass = env->FindClass("java/lang/ClassLoader");
    if (!loaderClass || env->ExceptionCheck()) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        Log(std::string("AntiDebuff render hook: helper definition failed: ") + defineError);
        return nullptr;
    }
    jmethodID loadClass = env->GetMethodID(
        loaderClass, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;");
    jstring name = env->NewStringUTF("lc.aoko.AntiDebuffRenderHook");
    if (loadClass && name && !env->ExceptionCheck())
        helper = (jclass)env->CallObjectMethod(gameClassLoader, loadClass, name);
    if (name) env->DeleteLocalRef(name);
    env->DeleteLocalRef(loaderClass);
    if (env->ExceptionCheck()) {
        const std::string loadError = DescribePendingException(env);
        Log(std::string("AntiDebuff render hook: helper definition/load failed; define=") +
            defineError + "; load=" + loadError);
        return nullptr;
    }
    return helper;
}

static bool RestoreLoadedTargetsToVanilla() {
    jvmtiEnv* jvmti = lc::SharedJvmtiEnv();
    if (!jvmti || !lc::HasJvmtiRetransform()) return false;

    jint classCount = 0;
    jclass* classes = nullptr;
    if (jvmti->GetLoadedClasses(&classCount, &classes) != JVMTI_ERROR_NONE || !classes)
        return false;
    std::vector<jclass> targets;
    for (jint i = 0; i < classCount; ++i) {
        char* signature = nullptr;
        if (jvmti->GetClassSignature(classes[i], &signature, nullptr) == JVMTI_ERROR_NONE &&
            signature) {
            if (IsTargetSignature(signature)) targets.push_back(classes[i]);
            jvmti->Deallocate(reinterpret_cast<unsigned char*>(signature));
        }
    }
    const bool restored = targets.empty() ||
        lc::SharedJvmtiRetransformClasses(targets.data(), (jint)targets.size());
    jvmti->Deallocate(reinterpret_cast<unsigned char*>(classes));
    return restored;
}

static bool WaitForCallbacksToDrain(DWORD timeoutMs) {
    const DWORD started = GetTickCount();
    while (InterlockedCompareExchange(&s_callbacksInFlight, 0, 0) != 0) {
        if (GetTickCount() - started >= timeoutMs) return false;
        Sleep(1);
    }
    return true;
}

static bool HasTargetRendererFrames(JNIEnv* env, jvmtiEnv* jvmti, bool& queryOk) {
    queryOk = false;
    if (!env || !jvmti) return true;

    jint threadCount = 0;
    jthread* threads = nullptr;
    if (jvmti->GetAllThreads(&threadCount, &threads) != JVMTI_ERROR_NONE || !threads)
        return true;

    bool found = false;
    bool failed = false;
    for (jint i = 0; i < threadCount && !found; ++i) {
        jvmtiFrameInfo frames[128] = {};
        jint frameCount = 0;
        const jvmtiError stackError =
            jvmti->GetStackTrace(threads[i], 0, 128, frames, &frameCount);
        if (stackError != JVMTI_ERROR_NONE) {
            // Threads can terminate between enumeration and inspection. Those cannot
            // retain an obsolete renderer frame, so only unexpected live-thread errors
            // make the safety query fail.
            if (stackError != JVMTI_ERROR_THREAD_NOT_ALIVE) failed = true;
            continue;
        }
        for (jint f = 0; f < frameCount; ++f) {
            jclass declaringClass = nullptr;
            if (jvmti->GetMethodDeclaringClass(frames[f].method, &declaringClass) !=
                    JVMTI_ERROR_NONE || !declaringClass) {
                failed = true;
                continue;
            }
            char* signature = nullptr;
            if (jvmti->GetClassSignature(declaringClass, &signature, nullptr) ==
                    JVMTI_ERROR_NONE && signature) {
                found = IsTargetSignature(signature);
                jvmti->Deallocate(reinterpret_cast<unsigned char*>(signature));
            } else {
                failed = true;
            }
            env->DeleteLocalRef(declaringClass);
            if (found) break;
        }
    }
    for (jint i = 0; i < threadCount; ++i)
        env->DeleteLocalRef(threads[i]);
    jvmti->Deallocate(reinterpret_cast<unsigned char*>(threads));
    queryOk = !failed;
    return found;
}

static bool WaitForTargetRendererFramesToDrain(JNIEnv* env, DWORD timeoutMs) {
    jvmtiEnv* jvmti = lc::SharedJvmtiEnv();
    if (!env || !jvmti) return false;
    const DWORD started = GetTickCount();
    while (true) {
        bool queryOk = false;
        const bool hasTargetFrame = HasTargetRendererFrames(env, jvmti, queryOk);
        if (!queryOk) return false;
        if (!hasTargetFrame) return true;
        if (GetTickCount() - started >= timeoutMs) return false;
        Sleep(1);
    }
}

} // namespace

void Install(JavaVM* vm, void (*logger)(const std::string&)) {
    s_vm = vm;
    s_log = logger;
    EnsureLock();
    InterlockedExchange(&s_installed, vm ? 1 : 0);
}

bool Arm(JNIEnv* env, jobject gameClassLoader) {
    if (!env || !s_vm || InterlockedCompareExchange(&s_installed, 0, 0) == 0)
        return false;
    if (InterlockedCompareExchange(&s_armed, 0, 0) != 0) return true;
    if (!lc::IsNickHiderJvmtiInstalled() || !lc::HasJvmtiRetransform()) {
        Log("AntiDebuff render hook: JVMTI retransformation unavailable");
        return false;
    }

    if (!s_helperClass) {
        jclass helper = FindOrDefineHelper(env, gameClassLoader);
        if (!helper) return false;
        s_helperClass = (jclass)env->NewGlobalRef(helper);
        env->DeleteLocalRef(helper);
        if (!s_helperClass) return false;

        JNINativeMethod nativeMethod = {};
        nativeMethod.name = const_cast<char*>("checkEffect");
        nativeMethod.signature = const_cast<char*>("(Ljava/lang/Object;Ljava/lang/Object;)Z");
        nativeMethod.fnPtr = (void*)&NativeCheckEffect;
        if (env->RegisterNatives(s_helperClass, &nativeMethod, 1) != 0 ||
            env->ExceptionCheck()) {
            const std::string detail = DescribePendingException(env);
            Log(std::string("AntiDebuff render hook: RegisterNatives failed: ") + detail);
            // Do not leave a helper global behind: a later Arm() would otherwise skip
            // registration, rewrite the renderer, and invoke an unbound native method.
            env->UnregisterNatives(s_helperClass);
            if (env->ExceptionCheck()) env->ExceptionClear();
            env->DeleteGlobalRef(s_helperClass);
            s_helperClass = nullptr;
            return false;
        }
    }

    if (!s_hookToken) {
        s_hookToken = lc::RegisterClassFileLoadHook(OnClassFileLoadHook);
        if (!s_hookToken) {
            Log("AntiDebuff render hook: class-file broker registration failed");
            return false;
        }
    }

    jvmtiEnv* jvmti = lc::SharedJvmtiEnv();
    if (!jvmti) return false;
    jint classCount = 0;
    jclass* classes = nullptr;
    if (jvmti->GetLoadedClasses(&classCount, &classes) != JVMTI_ERROR_NONE || !classes)
        return false;

    std::vector<jclass> targets;
    for (jint i = 0; i < classCount; ++i) {
        char* signature = nullptr;
        if (jvmti->GetClassSignature(classes[i], &signature, nullptr) == JVMTI_ERROR_NONE &&
            signature) {
            if (IsTargetSignature(signature)) targets.push_back(classes[i]);
            jvmti->Deallocate(reinterpret_cast<unsigned char*>(signature));
        }
    }

    InterlockedExchange(&s_injected, 0);
    bool transformed = false;
    if (!targets.empty()) {
        transformed = lc::SharedJvmtiRetransformClasses(
            targets.data(), (jint)targets.size());
    }
    jvmti->Deallocate(reinterpret_cast<unsigned char*>(classes));
    if (!transformed || InterlockedCompareExchange(&s_injected, 0, 0) == 0) {
        Log(targets.empty()
            ? "AntiDebuff render hook: no supported fog-effect class is loaded"
            : "AntiDebuff render hook: retransformation completed without a rewrite");
        return false;
    }

    InterlockedExchange(&s_armed, 1);
    Log("AntiDebuff render hook: armed; blindness remains active outside fog rendering");
    return true;
}

void BindBlindness(JNIEnv* env, jobject blindnessHolder, jmethodID hasEffectMethod) {
    if (!env) return;
    EnsureLock();
    jobject replacement = blindnessHolder ? env->NewGlobalRef(blindnessHolder) : nullptr;
    EnterCriticalSection(&s_lock);
    jobject old = s_blindness;
    s_blindness = replacement;
    s_hasEffect = hasEffectMethod;
    LeaveCriticalSection(&s_lock);
    if (old) env->DeleteGlobalRef(old);
}

void SetEnabled(bool enabled) {
    InterlockedExchange(&s_enabled, enabled ? 1 : 0);
}

bool Shutdown(JNIEnv* env) {
    SetEnabled(false);
    if (s_hookToken) {
        lc::UnregisterClassFileLoadHook(s_hookToken);
        s_hookToken = 0;
    }

    // With our broker removed, retransformation restores vanilla bytecode while
    // preserving unrelated transformations still registered with the host. Never
    // unregister the native callback or permit DLL unload if restoration fails.
    const bool hasInjectedBytecode =
        InterlockedCompareExchange(&s_armed, 0, 0) != 0 ||
        InterlockedCompareExchange(&s_injected, 0, 0) != 0;
    if (hasInjectedBytecode && !RestoreLoadedTargetsToVanilla()) {
        Log("AntiDebuff render hook: vanilla restoration failed; keeping native bridge loaded for safety");
        return false;
    }
    // HotSpot may let an already-active frame finish the obsolete pre-retransform
    // bytecode. Wait until no targeted renderer frame can still reach our native helper.
    if (hasInjectedBytecode && !WaitForTargetRendererFramesToDrain(env, 2000)) {
        Log("AntiDebuff render hook: obsolete renderer frame did not drain; keeping native bridge loaded for safety");
        return false;
    }
    if (!WaitForCallbacksToDrain(2000)) {
        Log("AntiDebuff render hook: callback did not drain; keeping native bridge loaded for safety");
        return false;
    }

    EnsureLock();
    EnterCriticalSection(&s_lock);
    const bool needsJniCleanup = s_blindness != nullptr || s_helperClass != nullptr;
    LeaveCriticalSection(&s_lock);
    if (!env && needsJniCleanup) {
        Log("AntiDebuff render hook: JNI cleanup unavailable; keeping native bridge loaded for safety");
        return false;
    }

    EnterCriticalSection(&s_lock);
    jobject blindness = s_blindness;
    s_blindness = nullptr;
    s_hasEffect = nullptr;
    LeaveCriticalSection(&s_lock);
    if (env && blindness) env->DeleteGlobalRef(blindness);
    if (env && s_helperClass) {
        env->UnregisterNatives(s_helperClass);
        if (env->ExceptionCheck()) env->ExceptionClear();
        env->DeleteGlobalRef(s_helperClass);
    }
    s_helperClass = nullptr;
    InterlockedExchange(&s_armed, 0);
    InterlockedExchange(&s_injected, 0);
    InterlockedExchange(&s_installed, 0);
    s_vm = nullptr;
    s_log = nullptr;
    return true;
}

} // namespace anti_debuff_jvmti
