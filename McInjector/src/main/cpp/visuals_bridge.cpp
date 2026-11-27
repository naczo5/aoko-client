#include "visuals_bridge.h"

#include "nick_hider_jvmti.h"
#include "visuals_hooks.h"

#include <windows.h>
#include <jvmti.h>

#include <cstring>
#include <vector>

#include "jni_core/visuals_helper.inc"   // VisualsClassEntry, kVisualsClasses[], kVisualsClassCount

namespace visuals {
namespace {

const DWORD kMaintainIntervalMs = 500;
const DWORD kWaitIntervalMs = 1000;

void (*s_log)(const std::string&) = nullptr;
JavaVM* s_vm = nullptr;
Generation s_generation = Generation::Modern26;

CRITICAL_SECTION s_lock;          // settings, status and issue text
CRITICAL_SECTION s_tickLock;      // held for a whole Tick and by Shutdown, so teardown never races a JNI call
INIT_ONCE s_lockOnce = INIT_ONCE_STATIC_INIT;
std::string s_lastLogged;

Settings s_settings;
unsigned s_settingsVersion = 0;
unsigned s_appliedVersion = 0;
std::string s_status = "off";
std::string s_rewriteIssue;            // guarded by s_lock

bool s_installed = false;
bool s_unsupported = false;            // permanent for this attach: the game version cannot be driven
bool s_helperLoaded = false;
bool s_armed = false;
int s_hookToken = 0;
DWORD s_lastWaitMs = 0;
DWORD s_lastMaintainMs = 0;

jobject s_loaderRef = nullptr;         // global ref: the loader the helper lives in; hooks apply only to its classes
int s_armFailures = 0;
DWORD s_lastArmMs = 0;
const int kMaxArmFailures = 5;
jclass s_visualsClass = nullptr;       // global ref
jclass s_hooksClass = nullptr;         // global ref
jmethodID s_mBootstrap = nullptr, s_mConfigure = nullptr, s_mMaintain = nullptr, s_mShutdown = nullptr;
volatile LONG s_rewritten = 0;

BOOL CALLBACK InitLock(PINIT_ONCE, PVOID, PVOID*) {
    InitializeCriticalSection(&s_lock);
    InitializeCriticalSection(&s_tickLock);
    return TRUE;
}

void EnsureLock() { InitOnceExecuteOnce(&s_lockOnce, InitLock, nullptr, nullptr); }

struct Guard {
    Guard() { EnsureLock(); EnterCriticalSection(&s_lock); }
    ~Guard() { LeaveCriticalSection(&s_lock); }
};

void Log(const std::string& message) {
    if (s_log) s_log("Visuals: " + message);
}

// Logs a state once: the worker retries every second and must not repeat itself.
void LogOnce(const std::string& message) {
    if (message == s_lastLogged) return;
    s_lastLogged = message;
    Log(message);
}

void SetStatus(const std::string& status) {
    Guard g;
    s_status = SanitizeStatus(status);
}

// ---- class-file broker ---------------------------------------------------------------------

void OnClassFileLoad(jvmtiEnv*, JNIEnv* env, jclass, jobject loader, const char* name, jobject, jint classDataLen,
                     const unsigned char* classData, jint* newClassDataLen, unsigned char** newClassData) {
    if (!name || !classData || classDataLen <= 0 || !newClassDataLen || !newClassData) return;
    size_t count = 0;
    const HookClass* classes = HookClasses(s_generation, &count);
    for (size_t i = 0; i < count; ++i) {
        if (std::strcmp(name, classes[i].internalName) != 0) continue;

        // A prologue calls com.aoko.visuals.Hooks by name. Only rewrite classes defined by the loader
        // that holds the helper; anywhere else the call would throw NoClassDefFoundError into the game.
        if (!env || !loader || !s_loaderRef || !env->IsSameObject(loader, s_loaderRef)) {
            Guard g;
            s_rewriteIssue = std::string(name) + ": defined by a different class loader";
            Log("skipping " + std::string(name) + ": defined by a different class loader than the helper");
            return;
        }

        mhook::RewriteResult r = mhook::Rewrite(classData, (size_t)classDataLen, classes[i].specs, classes[i].count,
                                                kHelperInternalName);
        if (!r.ok) {
            Guard g;
            s_rewriteIssue = std::string(name) + ": " + r.error;
            Log("hook rewrite failed for " + s_rewriteIssue);
            return;
        }
        if (!r.changed) {
            if (r.alreadyHooked == 0) {
                Guard g;
                s_rewriteIssue = std::string(name) + ": no matching methods";
                Log("no hook target found in " + std::string(name));
            }
            return;
        }
        unsigned char* memory = nullptr;
        if (!lc::SharedJvmtiAllocate((jlong)r.bytes.size(), &memory) || !memory) {
            Log("JVMTI allocation failed for " + std::string(name));
            return;
        }
        std::memcpy(memory, r.bytes.data(), r.bytes.size());
        *newClassData = memory;
        *newClassDataLen = (jint)r.bytes.size();
        InterlockedIncrement(&s_rewritten);
        if ((size_t)r.hooked != classes[i].count) {
            Guard g;
            s_rewriteIssue = std::string(name) + ": " + std::to_string(r.hooked) + " of " + std::to_string(classes[i].count) + " hooks";
        }
        Log("hooked " + std::string(name) + " (" + std::to_string(r.hooked) + ")");
        return;
    }
}

bool IsHookSignature(const char* signature, const HookClass* classes, size_t count) {
    if (!signature || signature[0] != 'L') return false;
    std::string name(signature + 1);
    if (!name.empty() && name[name.size() - 1] == ';') name.resize(name.size() - 1);
    for (size_t i = 0; i < count; ++i) {
        if (name == classes[i].internalName) return true;
    }
    return false;
}

// Loaded classes that match `matcher`. The caller frees nothing: jclass values are local refs
// owned by the calling frame.
template <typename Match>
void CollectLoaded(jvmtiEnv* jvmti, Match match, std::vector<jclass>& out) {
    jint classCount = 0;
    jclass* classes = nullptr;
    if (jvmti->GetLoadedClasses(&classCount, &classes) != JVMTI_ERROR_NONE || !classes) return;
    for (jint i = 0; i < classCount; ++i) {
        char* signature = nullptr;
        if (jvmti->GetClassSignature(classes[i], &signature, nullptr) == JVMTI_ERROR_NONE && signature) {
            if (match(signature)) out.push_back(classes[i]);
            jvmti->Deallocate(reinterpret_cast<unsigned char*>(signature));
        }
    }
    jvmti->Deallocate(reinterpret_cast<unsigned char*>(classes));
}

// ---- JNI helpers ----------------------------------------------------------------------------

std::string Describe(JNIEnv* env) {
    if (!env->ExceptionCheck()) return std::string();
    jthrowable ex = env->ExceptionOccurred();
    env->ExceptionClear();
    std::string text = "exception";
    if (ex) {
        jclass thr = env->FindClass("java/lang/Throwable");
        jmethodID toString = thr ? env->GetMethodID(thr, "toString", "()Ljava/lang/String;") : nullptr;
        if (env->ExceptionCheck()) env->ExceptionClear();
        if (toString) {
            jstring s = static_cast<jstring>(env->CallObjectMethod(ex, toString));
            if (env->ExceptionCheck()) env->ExceptionClear();
            if (s) {
                const char* utf = env->GetStringUTFChars(s, nullptr);
                if (utf) { text = utf; env->ReleaseStringUTFChars(s, utf); }
                env->DeleteLocalRef(s);
            }
        }
        if (thr) env->DeleteLocalRef(thr);
        env->DeleteLocalRef(ex);
    }
    return text;
}

std::string TakeString(JNIEnv* env, jobject o) {
    if (!o) { if (env->ExceptionCheck()) env->ExceptionClear(); return std::string(); }
    std::string out;
    const char* utf = env->GetStringUTFChars(static_cast<jstring>(o), nullptr);
    if (utf) { out = utf; env->ReleaseStringUTFChars(static_cast<jstring>(o), utf); }
    else if (env->ExceptionCheck()) env->ExceptionClear();
    env->DeleteLocalRef(o);
    return out;
}

// Every guard and value method the hook tables call must exist in the loaded Hooks class with the
// exact descriptor. A helper left over from a different build fails here instead of throwing
// NoSuchMethodError from inside a hooked game method.
bool VerifyHookMethods(JNIEnv* env, std::string& error) {
    if (!s_hooksClass) { error = "Hooks class missing from the helper"; return false; }
    size_t count = 0;
    const HookClass* classes = HookClasses(s_generation, &count);
    for (size_t i = 0; i < count; ++i) {
        for (size_t k = 0; k < classes[i].count; ++k) {
            std::string guard, value;
            ExpectedHelperDescriptors(classes[i].specs[k], &guard, &value);
            const char* names[2] = { classes[i].specs[k].guardName, classes[i].specs[k].valueName };
            const std::string descs[2] = { guard, value };
            for (int m = 0; m < 2; ++m) {
                if (!names[m]) continue;
                jmethodID id = env->GetStaticMethodID(s_hooksClass, names[m], descs[m].c_str());
                if (!id || env->ExceptionCheck()) {
                    if (env->ExceptionCheck()) env->ExceptionClear();
                    error = std::string("a helper from a different Aoko build is already loaded (missing ") + names[m] +
                            "); restart Minecraft";
                    return false;
                }
            }
        }
    }
    return true;
}

// Defines every embedded class in `loader`. A class that is already there (a second attach, or a
// retry after a partial failure) is looked up through the loader instead.
bool DefineHelperClasses(JNIEnv* env, jobject loader, std::string& error) {
    jclass loaderClass = env->FindClass("java/lang/ClassLoader");
    jmethodID loadClass = loaderClass ? env->GetMethodID(loaderClass, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;") : nullptr;
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (loaderClass) env->DeleteLocalRef(loaderClass);

    for (int i = 0; i < kVisualsClassCount; ++i) {
        const VisualsClassEntry& e = kVisualsClasses[i];
        jclass defined = env->DefineClass(e.name, loader, reinterpret_cast<const jbyte*>(e.bytes), (jsize)e.length);
        if (!defined || env->ExceptionCheck()) {
            const std::string why = Describe(env);
            defined = nullptr;
            if (loadClass) {
                std::string dotted(e.name);
                for (size_t k = 0; k < dotted.size(); ++k) if (dotted[k] == '/') dotted[k] = '.';
                jstring jn = env->NewStringUTF(dotted.c_str());
                if (jn) {
                    defined = static_cast<jclass>(env->CallObjectMethod(loader, loadClass, jn));
                    if (env->ExceptionCheck()) { env->ExceptionClear(); defined = nullptr; }
                    env->DeleteLocalRef(jn);
                }
            }
            if (!defined) { error = std::string("define ") + e.name + ": " + why; return false; }
        }
        if (std::strcmp(e.name, "com/aoko/visuals/Visuals") == 0 && !s_visualsClass) {
            s_visualsClass = static_cast<jclass>(env->NewGlobalRef(defined));
        }
        if (std::strcmp(e.name, kHelperInternalName) == 0 && !s_hooksClass) {
            s_hooksClass = static_cast<jclass>(env->NewGlobalRef(defined));
        }
        env->DeleteLocalRef(defined);
    }
    if (!s_visualsClass) { error = "Visuals class missing from the embedded helper"; return false; }

    s_mBootstrap = env->GetStaticMethodID(s_visualsClass, "bootstrap", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); s_mBootstrap = nullptr; }
    s_mConfigure = env->GetStaticMethodID(s_visualsClass, "configure", "(Ljava/lang/String;)Ljava/lang/String;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); s_mConfigure = nullptr; }
    s_mMaintain = env->GetStaticMethodID(s_visualsClass, "maintain", "()Ljava/lang/String;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); s_mMaintain = nullptr; }
    s_mShutdown = env->GetStaticMethodID(s_visualsClass, "shutdown", "()V");
    if (env->ExceptionCheck()) { env->ExceptionClear(); s_mShutdown = nullptr; }
    if (!s_mBootstrap || !s_mConfigure || !s_mMaintain || !s_mShutdown) { error = "helper entry points not found"; return false; }
    if (!VerifyHookMethods(env, error)) return false;
    return true;
}

std::string CallString(JNIEnv* env, jmethodID m, jstring a, jstring b, bool two) {
    jobject r = two ? env->CallStaticObjectMethod(s_visualsClass, m, a, b)
                    : (a ? env->CallStaticObjectMethod(s_visualsClass, m, a) : env->CallStaticObjectMethod(s_visualsClass, m));
    if (env->ExceptionCheck()) return std::string("error:") + Describe(env);
    return TakeString(env, r);
}

// ---- Arming ----------------------------------------------------------------------------------

bool ArmHooks(JNIEnv* env, std::string& error) {
    if (!lc::IsNickHiderJvmtiInstalled() || !lc::HasJvmtiRetransform()) {
        error = "JVMTI retransformation unavailable";
        return false;
    }
    jvmtiEnv* jvmti = lc::SharedJvmtiEnv();
    if (!jvmti) { error = "JVMTI unavailable"; return false; }

    if (!s_hookToken) {
        s_hookToken = lc::RegisterClassFileLoadHook(OnClassFileLoad);
        if (!s_hookToken) { error = "class-file broker registration failed"; return false; }
    }
    size_t count = 0;
    const HookClass* classes = HookClasses(s_generation, &count);
    std::vector<jclass> targets;
    CollectLoaded(jvmti, [&](const char* sig) { return IsHookSignature(sig, classes, count); }, targets);
    InterlockedExchange(&s_rewritten, 0);
    if (!targets.empty() && !lc::SharedJvmtiRetransformClasses(targets.data(), (jint)targets.size())) {
        error = "retransformation of loaded game classes failed";
        for (size_t i = 0; i < targets.size(); ++i) env->DeleteLocalRef(targets[i]);
        return false;
    }
    for (size_t i = 0; i < targets.size(); ++i) env->DeleteLocalRef(targets[i]);
    Log("armed: " + std::to_string(targets.size()) + " target class(es) loaded, "
        + std::to_string((long)InterlockedCompareExchange(&s_rewritten, 0, 0)) + " rewritten");
    return true;
}

jobject FindGameLoader(JNIEnv* env) {
    jvmtiEnv* jvmti = lc::SharedJvmtiEnv();
    if (!jvmti) return nullptr;
    std::vector<jclass> found;
    CollectLoaded(jvmti, [](const char* sig) { return std::strcmp(sig, "Lnet/minecraft/client/Minecraft;") == 0; }, found);
    jobject loader = nullptr;
    for (size_t i = 0; i < found.size(); ++i) {
        if (!loader) {
            jobject l = nullptr;
            if (jvmti->GetClassLoader(found[i], &l) == JVMTI_ERROR_NONE && l) loader = l;
        }
        env->DeleteLocalRef(found[i]);
    }
    return loader;      // local ref, may be null (bootstrap loader or not loaded yet)
}

} // namespace

// ---- Public API -------------------------------------------------------------------------------

void Install(JavaVM* vm, Generation generation, void (*logger)(const std::string&)) {
    EnsureLock();
    Guard g;
    s_vm = vm;
    s_generation = generation;
    s_log = logger;
    s_installed = vm != nullptr;
}

void SetSettings(const Settings& settings) {
    Guard g;
    if (settings != s_settings) {
        s_settings = settings;
        ++s_settingsVersion;
    }
}

std::string Status() {
    Guard g;
    return s_status;
}

void Tick(JNIEnv* env) {
    if (!env) return;
    EnsureLock();
    struct TickGuard {
        TickGuard() { EnterCriticalSection(&s_tickLock); }
        ~TickGuard() { LeaveCriticalSection(&s_tickLock); }
    } tickGuard;
    Settings settings;
    unsigned version = 0;
    {
        Guard g;
        if (!s_installed || s_unsupported) return;
        settings = s_settings;
        version = s_settingsVersion;
        if (!settings.Wanted() && !s_helperLoaded) {
            s_status = "off";
            return;
        }
    }
    if (env->PushLocalFrame(256) != 0) { if (env->ExceptionCheck()) env->ExceptionClear(); return; }
    const DWORD now = GetTickCount();
    do {
        if (!s_helperLoaded) {
            if (!ValidDir(settings.dir)) { SetStatus("error:no visuals directory"); break; }
            if (now - s_lastWaitMs < kWaitIntervalMs && s_lastWaitMs != 0) break;
            s_lastWaitMs = now;
            if (!lc::IsNickHiderJvmtiInstalled() || !lc::HasJvmtiRetransform()) {
                SetStatus("error:JVMTI retransformation unavailable");
                break;
            }
            jobject loader = FindGameLoader(env);
            if (!loader) { SetStatus("partial:waiting for the game to load"); break; }
            std::string error;
            if (!s_loaderRef) s_loaderRef = env->NewGlobalRef(loader);
            if (!DefineHelperClasses(env, loader, error)) {
                LogOnce(error);
                SetStatus("error:" + error);
                break;
            }
            // The JSON is pure ASCII (non-ASCII is \uXXXX escaped), so NewStringUTF is exact.
            jstring gen = env->NewStringUTF(s_generation == Generation::Legacy189 ? "legacy" : "modern");
            const std::string configJson = BuildConfigJson(settings);
            jstring cfg = env->NewStringUTF(configJson.c_str());
            std::string r = (gen && cfg) ? CallString(env, s_mBootstrap, gen, cfg, true) : std::string("error:out of memory");
            if (gen) env->DeleteLocalRef(gen);
            if (cfg) env->DeleteLocalRef(cfg);
            LogOnce("helper bootstrap: " + r);
            if (r.compare(0, 11, "unsupported") == 0) {
                Guard g;
                s_unsupported = true;
                s_status = SanitizeStatus(r);
                s_helperLoaded = false;
                break;
            }
            if (r.compare(0, 5, "error") == 0) { SetStatus(r); break; }
            s_helperLoaded = true;
            s_appliedVersion = 0;
        }
        if (s_helperLoaded && !s_armed) {
            // Arming walks every loaded class and retransforms the biggest game classes, so it is
            // retried slowly and given up on after a few failures.
            if (s_armFailures >= kMaxArmFailures) break;
            if (s_lastArmMs != 0 && now - s_lastArmMs < 3 * kWaitIntervalMs) break;
            s_lastArmMs = now;
            std::string error;
            if (!ArmHooks(env, error)) {
                ++s_armFailures;
                LogOnce(error);
                SetStatus("error:" + error + (s_armFailures >= kMaxArmFailures ? " (giving up until the game restarts)" : ""));
                break;
            }
            s_armed = true;
        }

        std::string status;
        bool called = false;
        if (s_appliedVersion != version) {
            const std::string json = BuildConfigJson(settings);
            jstring js = env->NewStringUTF(json.c_str());
            if (js) {
                status = CallString(env, s_mConfigure, js, nullptr, false);
                env->DeleteLocalRef(js);
                called = true;
                s_appliedVersion = version;
                s_lastMaintainMs = now;
            }
        } else if (settings.Wanted() && now - s_lastMaintainMs >= kMaintainIntervalMs) {
            s_lastMaintainMs = now;
            status = CallString(env, s_mMaintain, nullptr, nullptr, false);
            called = true;
        }
        if (called) {
            std::string issue;
            { Guard g; issue = s_rewriteIssue; }
            if (!issue.empty() && settings.Wanted() && status.compare(0, 5, "error") != 0) {
                status = "partial:hooks (" + issue + ")" + (status == "ok" || status == "off" ? "" : "; " + status);
            }
            SetStatus(status.empty() ? "error:no status from helper" : status);
        }
    } while (false);
    env->PopLocalFrame(nullptr);
}

bool Shutdown(JNIEnv* env) {
    EnsureLock();
    EnterCriticalSection(&s_tickLock);      // waits for a Tick in flight; none can start a JNI call after this
    struct Leave { ~Leave() { LeaveCriticalSection(&s_tickLock); } } leave;
    bool hadHelper = false;
    {
        Guard g;
        hadHelper = s_helperLoaded;
    }
    // Without JNI the helper cannot be switched off, so keep the broker and the bridge alive and let
    // the caller retry the detach instead of leaving overrides running with no off switch.
    if (!env && hadHelper) return false;
    {
        Guard g;
        s_installed = false;
    }
    if (env && hadHelper && s_visualsClass && s_mShutdown) {
        env->CallStaticVoidMethod(s_visualsClass, s_mShutdown);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (s_hookToken) {
        lc::UnregisterClassFileLoadHook(s_hookToken);
        s_hookToken = 0;
    }
    if (s_armed && env && env->PushLocalFrame(512) == 0) {
        // With the broker gone, retransformation puts the vanilla bytecode back. The callbacks
        // are Java methods, so a failed restore leaves harmless no-op prologues behind.
        jvmtiEnv* jvmti = lc::SharedJvmtiEnv();
        if (jvmti && lc::HasJvmtiRetransform()) {
            size_t count = 0;
            const HookClass* classes = HookClasses(s_generation, &count);
            std::vector<jclass> targets;
            CollectLoaded(jvmti, [&](const char* sig) { return IsHookSignature(sig, classes, count); }, targets);
            if (!targets.empty() && !lc::SharedJvmtiRetransformClasses(targets.data(), (jint)targets.size()))
                Log("could not restore vanilla bytecode; the hooks stay inert");
            for (size_t i = 0; i < targets.size(); ++i) env->DeleteLocalRef(targets[i]);
        }
        env->PopLocalFrame(nullptr);
    }
    if (env && s_visualsClass) env->DeleteGlobalRef(s_visualsClass);
    if (env && s_hooksClass) env->DeleteGlobalRef(s_hooksClass);
    if (env && s_loaderRef) env->DeleteGlobalRef(s_loaderRef);
    Guard g;
    s_visualsClass = nullptr;
    s_hooksClass = nullptr;
    s_loaderRef = nullptr;
    s_armFailures = 0;
    s_lastArmMs = 0;
    s_mBootstrap = s_mConfigure = s_mMaintain = s_mShutdown = nullptr;
    s_helperLoaded = false;
    s_armed = false;
    s_unsupported = false;
    s_appliedVersion = 0;
    s_status = "off";
    return true;
}

} // namespace visuals
