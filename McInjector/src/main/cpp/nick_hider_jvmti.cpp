#include "nick_hider_jvmti.h"
#include "nick_hider.h"
#include "nick_hider_jvmti_targets.h"

#include <jvmti.h>
#include <windows.h>
#include <cstring>

namespace lc {
namespace {

const int kMaxSurfaces = 24;
const int kMaxMethods = 192;
const int kMaxStringSlots = 6;

struct SurfaceEntry {
    jclass clazz;
    const char* label;
    volatile LONG replacements;
};

struct RenderMethodEntry {
    jmethodID method;
    jint stringSlots[kMaxStringSlots];
    int stringSlotCount;
};

struct ConfigSnapshot {
    volatile LONG version;
    volatile LONG enabled;
    char localName[64];
    char alias[128];
};

static JavaVM* s_vm = nullptr;
static jvmtiEnv* s_jvmti = nullptr;
static NickHiderJvmtiGeneration s_generation = NickHiderJvmtiGeneration::Legacy189;
static void (*s_log)(const std::string&) = nullptr;
static volatile LONG s_installed = 0;
static volatile LONG s_surfaceCount = 0;
static volatile LONG s_methodCount = 0;
static SurfaceEntry s_surfaces[kMaxSurfaces] = {};
static RenderMethodEntry s_methods[kMaxMethods] = {};
static ConfigSnapshot s_config = {};
static volatile LONG s_loggedUnsupported = 0;
static JvmtiBreakpointHandler s_extraBreakpoint = nullptr;
static JvmtiFramePopHandler s_extraFramePop = nullptr;
static volatile LONG s_framePopEnabled = 0;
static volatile LONG s_hasBreakpoints = 0;
static volatile LONG s_hasFramePop = 0;
static volatile LONG s_hasLocalVars = 0;
static volatile LONG s_hasGetBytecodes = 0;
static volatile LONG s_hasRetransform = 0;
static volatile LONG s_hasRetransformAny = 0;

typedef void (*JvmtiClassFileLoadHookFn)(jvmtiEnv* jvmti, JNIEnv* env, jclass classBeingRedefined,
                                         jobject loader, const char* name, jobject protectionDomain,
                                         jint classDataLen, const unsigned char* classData,
                                         jint* newClassDataLen, unsigned char** newClassData);
const int kMaxClassFileHooks = 8;
static JvmtiClassFileLoadHookFn s_classFileHooks[kMaxClassFileHooks] = {};
static SRWLOCK s_classFileHookLock = SRWLOCK_INIT;

static void Log(const std::string& message)
{
    if (s_log) s_log(message);
}

static bool IsLegacySurface(const char* signature, const char*& label)
{
    label = NickHiderJvmtiSurfaceForSignature(false, signature);
    return label != nullptr;
}

static bool IsModernSurface(const char* signature, const char*& label)
{
    label = NickHiderJvmtiSurfaceForSignature(true, signature);
    return label != nullptr;
}

static bool IsRendererClass(const char* signature)
{
    return IsNickHiderJvmtiRendererSignature(s_generation == NickHiderJvmtiGeneration::Modern121, signature);
}

static bool IsSurfaceClass(JNIEnv* env, jclass clazz, SurfaceEntry*& result)
{
    const LONG count = InterlockedCompareExchange(&s_surfaceCount, 0, 0);
    for (LONG i = 0; i < count; ++i) {
        if (s_surfaces[i].clazz && env->IsSameObject(clazz, s_surfaces[i].clazz) == JNI_TRUE) {
            result = &s_surfaces[i];
            return true;
        }
    }
    return false;
}

static RenderMethodEntry* FindRenderMethod(jmethodID method)
{
    const LONG count = InterlockedCompareExchange(&s_methodCount, 0, 0);
    for (LONG i = 0; i < count; ++i)
        if (s_methods[i].method == method) return &s_methods[i];
    return nullptr;
}

static bool IsWideDescriptor(char type)
{
    return type == 'J' || type == 'D';
}

static int FindStringParameterSlots(const char* signature, jint modifiers, jint* slots, int capacity)
{
    if (!signature || signature[0] != '(') return 0;
    // ACC_STATIC from the JVM class-file access flags. jvmti.h does not expose
    // the JVM_ACC_* constants on every MinGW/JDK header combination.
    int slot = (modifiers & 0x0008) ? 0 : 1;
    int found = 0;
    const char* p = signature + 1;
    while (*p && *p != ')') {
        if (*p == 'L') {
            const char* end = std::strchr(p, ';');
            if (!end) return found;
            if (std::strncmp(p, "Ljava/lang/String;", 18) == 0 && found < capacity)
                slots[found++] = slot;
            ++slot;
            p = end + 1;
        } else if (*p == '[') {
            while (*p == '[') ++p;
            if (*p == 'L') { const char* end = std::strchr(p, ';'); if (!end) return found; p = end + 1; }
            else if (*p) ++p;
            ++slot;
        } else {
            const char type = *p++;
            slot += IsWideDescriptor(type) ? 2 : 1;
        }
    }
    return found;
}

static void AddSurface(JNIEnv* env, jclass clazz, const char* label)
{
    const LONG count = InterlockedCompareExchange(&s_surfaceCount, 0, 0);
    for (LONG i = 0; i < count; ++i)
        if (env->IsSameObject(clazz, s_surfaces[i].clazz) == JNI_TRUE) return;
    if (count >= kMaxSurfaces) return;
    jclass global = static_cast<jclass>(env->NewGlobalRef(clazz));
    if (!global) return;
    s_surfaces[count].clazz = global;
    s_surfaces[count].label = label;
    InterlockedIncrement(&s_surfaceCount);
    Log(std::string("NickHider interceptor resolved surface: ") + label);
}

static void AddRendererMethods(jclass clazz)
{
    jint methodCount = 0;
    jmethodID* methods = nullptr;
    if (s_jvmti->GetClassMethods(clazz, &methodCount, &methods) != JVMTI_ERROR_NONE || !methods) return;
    int installed = 0;
    for (jint i = 0; i < methodCount; ++i) {
        const LONG existing = InterlockedCompareExchange(&s_methodCount, 0, 0);
        bool known = false;
        for (LONG n = 0; n < existing; ++n) if (s_methods[n].method == methods[i]) { known = true; break; }
        if (known || existing >= kMaxMethods) continue;

        char* name = nullptr;
        char* signature = nullptr;
        jint modifiers = 0;
        s_jvmti->GetMethodName(methods[i], &name, &signature, nullptr);
        s_jvmti->GetMethodModifiers(methods[i], &modifiers);
        RenderMethodEntry entry = {};
        entry.method = methods[i];
        entry.stringSlotCount = FindStringParameterSlots(signature, modifiers, entry.stringSlots, kMaxStringSlots);
        if (name) s_jvmti->Deallocate(reinterpret_cast<unsigned char*>(name));
        if (signature) s_jvmti->Deallocate(reinterpret_cast<unsigned char*>(signature));
        if (entry.stringSlotCount == 0) continue;
        if (s_jvmti->SetBreakpoint(entry.method, 0) != JVMTI_ERROR_NONE) continue;
        s_methods[existing] = entry;
        InterlockedIncrement(&s_methodCount);
        ++installed;
    }
    if (methods) s_jvmti->Deallocate(reinterpret_cast<unsigned char*>(methods));
    if (installed > 0)
        Log("NickHider interceptor armed " + std::to_string(installed) + " text render entry points.");
}

static bool Snapshot(ConfigSnapshot& out)
{
    for (int attempt = 0; attempt < 3; ++attempt) {
        const LONG before = InterlockedCompareExchange(&s_config.version, 0, 0);
        if (before & 1) continue;
        std::memcpy(&out, &s_config, sizeof(out));
        const LONG after = InterlockedCompareExchange(&s_config.version, 0, 0);
        if (before == after && !(after & 1)) return out.enabled != 0 && out.localName[0] && out.alias[0];
    }
    return false;
}

static SurfaceEntry* FindCallingSurface(JNIEnv* env, jthread thread)
{
    jvmtiFrameInfo frames[32] = {};
    jint count = 0;
    if (s_jvmti->GetStackTrace(thread, 1, 32, frames, &count) != JVMTI_ERROR_NONE) return nullptr;
    for (jint i = 0; i < count; ++i) {
        jclass clazz = nullptr;
        if (s_jvmti->GetMethodDeclaringClass(frames[i].method, &clazz) != JVMTI_ERROR_NONE || !clazz) continue;
        SurfaceEntry* surface = nullptr;
        const bool matched = IsSurfaceClass(env, clazz, surface);
        env->DeleteLocalRef(clazz);
        if (matched) return surface;
    }
    return nullptr;
}

static void JNICALL OnBreakpoint(jvmtiEnv* jvmti, JNIEnv* env, jthread thread, jmethodID method, jlocation location)
{
    if (s_extraBreakpoint)
        s_extraBreakpoint(jvmti, env, thread, method, location);

    if (!env || !thread) return;
    RenderMethodEntry* renderer = FindRenderMethod(method);
    if (!renderer) return;
    ConfigSnapshot config = {};
    if (!Snapshot(config)) return;
    SurfaceEntry* surface = FindCallingSurface(env, thread);
    if (!surface) return;

    for (int i = 0; i < renderer->stringSlotCount; ++i) {
        jobject local = nullptr;
        if (s_jvmti->GetLocalObject(thread, 0, renderer->stringSlots[i], &local) != JVMTI_ERROR_NONE || !local) continue;
        jstring text = static_cast<jstring>(local);
        const char* raw = env->GetStringUTFChars(text, nullptr);
        if (!raw) { if (env->ExceptionCheck()) env->ExceptionClear(); env->DeleteLocalRef(text); continue; }
        const std::string original(raw);
        env->ReleaseStringUTFChars(text, raw);
        const std::string replaced = ReplaceNickHiderText(original, true, config.localName, config.alias);
        if (replaced != original) {
            jstring next = env->NewStringUTF(replaced.c_str());
            if (next) {
                if (s_jvmti->SetLocalObject(thread, 0, renderer->stringSlots[i], next) == JVMTI_ERROR_NONE)
                    InterlockedIncrement(&surface->replacements);
                env->DeleteLocalRef(next);
            }
            if (env->ExceptionCheck()) env->ExceptionClear();
        }
        env->DeleteLocalRef(text);
    }
}

static void JNICALL OnFramePop(jvmtiEnv* jvmti, JNIEnv* env, jthread thread, jmethodID method,
                               jboolean wasPoppedByException)
{
    if (s_extraFramePop)
        s_extraFramePop(jvmti, env, thread, method, wasPoppedByException);
}

static void JNICALL OnClassFileLoadHook(jvmtiEnv* jvmti, JNIEnv* env,
                                        jclass classBeingRedefined, jobject loader,
                                        const char* name, jobject protectionDomain,
                                        jint classDataLen, const unsigned char* classData,
                                        jint* newClassDataLen, unsigned char** newClassData)
{
    JvmtiClassFileLoadHookFn snapshot[kMaxClassFileHooks] = {};
    AcquireSRWLockShared(&s_classFileHookLock);
    for (int i = 0; i < kMaxClassFileHooks; ++i) snapshot[i] = s_classFileHooks[i];
    ReleaseSRWLockShared(&s_classFileHookLock);

    const unsigned char* currentData = classData;
    jint currentLen = classDataLen;
    unsigned char* ownedData = nullptr;
    for (int i = 0; i < kMaxClassFileHooks; ++i) {
        if (!snapshot[i]) continue;
        jint nextLen = 0;
        unsigned char* nextData = nullptr;
        snapshot[i](jvmti, env, classBeingRedefined, loader, name, protectionDomain,
                    currentLen, currentData, &nextLen, &nextData);
        if (nextData && nextLen > 0) {
            if (ownedData) jvmti->Deallocate(ownedData);
            ownedData = nextData;
            currentData = nextData;
            currentLen = nextLen;
        }
    }
    if (ownedData && newClassDataLen && newClassData) {
        *newClassDataLen = currentLen;
        *newClassData = ownedData;
    } else if (ownedData) {
        jvmti->Deallocate(ownedData);
    }
}

} // namespace

void ConfigureNickHiderJvmti(bool enabled, const std::string& alias)
{
    const std::string normalized = NormalizeNickHiderAlias(alias);
    InterlockedIncrement(&s_config.version);
    s_config.enabled = enabled && !normalized.empty() ? 1 : 0;
    std::memset(s_config.alias, 0, sizeof(s_config.alias));
    if (!normalized.empty()) std::strncpy(s_config.alias, normalized.c_str(), sizeof(s_config.alias) - 1);
    InterlockedIncrement(&s_config.version);
}

void SetNickHiderJvmtiLocalName(const std::string& localName)
{
    InterlockedIncrement(&s_config.version);
    std::memset(s_config.localName, 0, sizeof(s_config.localName));
    if (localName.size() < sizeof(s_config.localName))
        std::strncpy(s_config.localName, localName.c_str(), sizeof(s_config.localName) - 1);
    InterlockedIncrement(&s_config.version);
}

static bool TryAddCapabilityBit(jvmtiCapabilities* pot, bool (*isSet)(const jvmtiCapabilities*),
                                void (*setBit)(jvmtiCapabilities*), const char* name)
{
    if (!pot || !isSet(pot)) {
        Log(std::string("JVMTI capability not potential: ") + name);
        return false;
    }
    jvmtiCapabilities req = {};
    setBit(&req);
    const jvmtiError err = s_jvmti->AddCapabilities(&req);
    if (err != JVMTI_ERROR_NONE) {
        Log(std::string("JVMTI capability denied: ") + name + " (" + std::to_string((int)err) + ")");
        return false;
    }
    Log(std::string("JVMTI capability acquired: ") + name);
    return true;
}

bool InstallNickHiderJvmti(JavaVM* vm, NickHiderJvmtiGeneration generation, void (*logger)(const std::string&))
{
    if (InterlockedCompareExchange(&s_installed, 0, 0)) return true;
    if (!vm) return false;
    s_vm = vm;
    s_generation = generation;
    s_log = logger;
    if (vm->GetEnv(reinterpret_cast<void**>(&s_jvmti), JVMTI_VERSION_1_2) != JNI_OK || !s_jvmti) {
        Log("JVMTI host unavailable: GetEnv(JVMTI) failed on this JVM.");
        return false;
    }

    // AddCapabilities is all-or-nothing per call. Request each bit separately so a
    // denied local-vars/frame-pop bit cannot poison breakpoints (Premotion's need).
    jvmtiCapabilities pot = {};
    if (s_jvmti->GetPotentialCapabilities(&pot) != JVMTI_ERROR_NONE)
        std::memset(&pot, 0, sizeof(pot));

    InterlockedExchange(&s_hasBreakpoints, TryAddCapabilityBit(
        &pot,
        [](const jvmtiCapabilities* c) { return c->can_generate_breakpoint_events != 0; },
        [](jvmtiCapabilities* c) { c->can_generate_breakpoint_events = 1; },
        "can_generate_breakpoint_events") ? 1 : 0);
    InterlockedExchange(&s_hasFramePop, TryAddCapabilityBit(
        &pot,
        [](const jvmtiCapabilities* c) { return c->can_generate_frame_pop_events != 0; },
        [](jvmtiCapabilities* c) { c->can_generate_frame_pop_events = 1; },
        "can_generate_frame_pop_events") ? 1 : 0);
    InterlockedExchange(&s_hasLocalVars, TryAddCapabilityBit(
        &pot,
        [](const jvmtiCapabilities* c) { return c->can_access_local_variables != 0; },
        [](jvmtiCapabilities* c) { c->can_access_local_variables = 1; },
        "can_access_local_variables") ? 1 : 0);
    InterlockedExchange(&s_hasGetBytecodes, TryAddCapabilityBit(
        &pot,
        [](const jvmtiCapabilities* c) { return c->can_get_bytecodes != 0; },
        [](jvmtiCapabilities* c) { c->can_get_bytecodes = 1; },
        "can_get_bytecodes") ? 1 : 0);
    InterlockedExchange(&s_hasRetransform, TryAddCapabilityBit(
        &pot,
        [](const jvmtiCapabilities* c) { return c->can_retransform_classes != 0; },
        [](jvmtiCapabilities* c) { c->can_retransform_classes = 1; },
        "can_retransform_classes") ? 1 : 0);
    InterlockedExchange(&s_hasRetransformAny, TryAddCapabilityBit(
        &pot,
        [](const jvmtiCapabilities* c) { return c->can_retransform_any_class != 0; },
        [](jvmtiCapabilities* c) { c->can_retransform_any_class = 1; },
        "can_retransform_any_class") ? 1 : 0);
    // Diagnostic only — often solo like breakpoints.
    TryAddCapabilityBit(
        &pot,
        [](const jvmtiCapabilities* c) { return c->can_generate_method_entry_events != 0; },
        [](jvmtiCapabilities* c) { c->can_generate_method_entry_events = 1; },
        "can_generate_method_entry_events");

    const bool haveBreakpoints = InterlockedCompareExchange(&s_hasBreakpoints, 0, 0) != 0;
    const bool haveRetransform = InterlockedCompareExchange(&s_hasRetransform, 0, 0) != 0;
    if (!haveBreakpoints && !haveRetransform) {
        Log("JVMTI host: neither breakpoints nor retransform available — Premotion disabled.");
        return false;
    }
    if (!haveBreakpoints)
        Log("JVMTI host: breakpoint events unavailable (solo/exclusive); using retransform Premotion path.");

    jvmtiEventCallbacks callbacks = {};
    callbacks.Breakpoint = OnBreakpoint;
    callbacks.FramePop = OnFramePop;
    callbacks.ClassFileLoadHook = OnClassFileLoadHook;
    if (s_jvmti->SetEventCallbacks(&callbacks, sizeof(callbacks)) != JVMTI_ERROR_NONE) {
        Log("JVMTI host: SetEventCallbacks failed.");
        return false;
    }
    if (haveBreakpoints) {
        if (s_jvmti->SetEventNotificationMode(JVMTI_ENABLE, JVMTI_EVENT_BREAKPOINT, nullptr) != JVMTI_ERROR_NONE) {
            Log("JVMTI host: breakpoint event enable failed.");
            InterlockedExchange(&s_hasBreakpoints, 0);
        }
    }
    if (haveRetransform) {
        if (s_jvmti->SetEventNotificationMode(JVMTI_ENABLE, JVMTI_EVENT_CLASS_FILE_LOAD_HOOK, nullptr) != JVMTI_ERROR_NONE) {
            Log("JVMTI host: ClassFileLoadHook enable failed.");
            InterlockedExchange(&s_hasRetransform, 0);
            if (!InterlockedCompareExchange(&s_hasBreakpoints, 0, 0)) {
                Log("JVMTI host: no usable Premotion capability after event setup.");
                return false;
            }
        }
    }

    InterlockedExchange(&s_installed, 1);
    if (!InterlockedCompareExchange(&s_hasLocalVars, 0, 0))
        Log("JVMTI host: local-variable access unavailable — NickHider text rewrite idle.");
    if (haveBreakpoints && !InterlockedCompareExchange(&s_hasFramePop, 0, 0))
        Log("JVMTI host: frame-pop unavailable — walking Premotion will use return-site breakpoints.");
    RefreshNickHiderJvmtiTargets();
    return true;
}

bool IsNickHiderJvmtiInstalled()
{
    return InterlockedCompareExchange(&s_installed, 0, 0) != 0;
}

bool HasJvmtiBreakpoints()
{
    return InterlockedCompareExchange(&s_hasBreakpoints, 0, 0) != 0
        && InterlockedCompareExchange(&s_installed, 0, 0) != 0;
}

bool HasJvmtiFramePop()
{
    return InterlockedCompareExchange(&s_hasFramePop, 0, 0) != 0
        && InterlockedCompareExchange(&s_installed, 0, 0) != 0;
}

bool HasJvmtiLocalVariables()
{
    return InterlockedCompareExchange(&s_hasLocalVars, 0, 0) != 0
        && InterlockedCompareExchange(&s_installed, 0, 0) != 0;
}

bool HasJvmtiGetBytecodes()
{
    return InterlockedCompareExchange(&s_hasGetBytecodes, 0, 0) != 0
        && InterlockedCompareExchange(&s_installed, 0, 0) != 0;
}

bool HasJvmtiRetransform()
{
    return InterlockedCompareExchange(&s_hasRetransform, 0, 0) != 0
        && InterlockedCompareExchange(&s_installed, 0, 0) != 0;
}

int RegisterClassFileLoadHook(ClassFileLoadHookFn handler)
{
    if (!handler) return 0;
    const JvmtiClassFileLoadHookFn typed = reinterpret_cast<JvmtiClassFileLoadHookFn>(handler);
    AcquireSRWLockExclusive(&s_classFileHookLock);
    for (int i = 0; i < kMaxClassFileHooks; ++i) {
        if (s_classFileHooks[i] == typed) {
            ReleaseSRWLockExclusive(&s_classFileHookLock);
            return i + 1;
        }
    }
    for (int i = 0; i < kMaxClassFileHooks; ++i) {
        if (!s_classFileHooks[i]) {
            s_classFileHooks[i] = typed;
            ReleaseSRWLockExclusive(&s_classFileHookLock);
            return i + 1;
        }
    }
    ReleaseSRWLockExclusive(&s_classFileHookLock);
    Log("JVMTI host: class-file transformation subscriber limit reached.");
    return 0;
}

void UnregisterClassFileLoadHook(int token)
{
    if (token <= 0 || token > kMaxClassFileHooks) return;
    AcquireSRWLockExclusive(&s_classFileHookLock);
    s_classFileHooks[token - 1] = nullptr;
    ReleaseSRWLockExclusive(&s_classFileHookLock);
}

bool SharedJvmtiRetransformClasses(jclass* classes, jint count)
{
    if (!s_jvmti || !classes || count <= 0 || !InterlockedCompareExchange(&s_installed, 0, 0))
        return false;
    if (!InterlockedCompareExchange(&s_hasRetransform, 0, 0))
        return false;
    const jvmtiError err = s_jvmti->RetransformClasses(count, classes);
    if (err != JVMTI_ERROR_NONE) {
        Log("JVMTI RetransformClasses failed (" + std::to_string((int)err) + ")");
        return false;
    }
    return true;
}

bool SharedJvmtiAllocate(jlong size, unsigned char** memPtr)
{
    if (!s_jvmti || !memPtr || size <= 0) return false;
    return s_jvmti->Allocate(size, memPtr) == JVMTI_ERROR_NONE;
}

void RefreshNickHiderJvmtiTargets()
{
    if (!s_jvmti || !InterlockedCompareExchange(&s_installed, 0, 0)) return;
    jint classCount = 0;
    jclass* classes = nullptr;
    if (s_jvmti->GetLoadedClasses(&classCount, &classes) != JVMTI_ERROR_NONE || !classes) return;
    const bool canRewriteLocals = InterlockedCompareExchange(&s_hasLocalVars, 0, 0) != 0;
    for (jint i = 0; i < classCount; ++i) {
        char* signature = nullptr;
        if (s_jvmti->GetClassSignature(classes[i], &signature, nullptr) != JVMTI_ERROR_NONE || !signature) continue;
        const char* label = nullptr;
        const bool surface = s_generation == NickHiderJvmtiGeneration::Legacy189
            ? IsLegacySurface(signature, label) : IsModernSurface(signature, label);
        if (surface) {
            JNIEnv* env = nullptr;
            if (s_vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_8) == JNI_OK && env)
                AddSurface(env, classes[i], label);
        }
        // Render breakpoints only help when we can rewrite String locals.
        if (canRewriteLocals && IsRendererClass(signature)) AddRendererMethods(classes[i]);
        s_jvmti->Deallocate(reinterpret_cast<unsigned char*>(signature));
    }
    s_jvmti->Deallocate(reinterpret_cast<unsigned char*>(classes));
    if (InterlockedCompareExchange(&s_surfaceCount, 0, 0) == 0 &&
        InterlockedCompareExchange(&s_loggedUnsupported, 1, 0) == 0)
        Log("NickHider interceptor: vanilla HUD mappings are not loaded yet; retrying after menu/world transitions.");
}

void FlushNickHiderJvmtiDiagnostics()
{
    for (LONG i = 0; i < InterlockedCompareExchange(&s_surfaceCount, 0, 0); ++i) {
        const LONG count = InterlockedExchange(&s_surfaces[i].replacements, 0);
        if (count > 0) Log(std::string("NickHider interceptor replaced ") + std::to_string(count) + " text value(s) on " + s_surfaces[i].label + ".");
    }
}

void ShutdownNickHiderJvmti(JNIEnv* env)
{
    if (s_jvmti) {
        s_jvmti->SetEventNotificationMode(JVMTI_DISABLE, JVMTI_EVENT_BREAKPOINT, nullptr);
        s_jvmti->SetEventNotificationMode(JVMTI_DISABLE, JVMTI_EVENT_FRAME_POP, nullptr);
        s_jvmti->SetEventNotificationMode(JVMTI_DISABLE, JVMTI_EVENT_CLASS_FILE_LOAD_HOOK, nullptr);
    }
    if (env) {
        for (LONG i = 0; i < InterlockedCompareExchange(&s_surfaceCount, 0, 0); ++i)
            if (s_surfaces[i].clazz) env->DeleteGlobalRef(s_surfaces[i].clazz);
    }
    InterlockedExchange(&s_surfaceCount, 0);
    InterlockedExchange(&s_methodCount, 0);
    InterlockedExchange(&s_framePopEnabled, 0);
    InterlockedExchange(&s_installed, 0);
    s_extraBreakpoint = nullptr;
    s_extraFramePop = nullptr;
}

void RegisterJvmtiBreakpointHandler(JvmtiBreakpointHandler handler)
{
    s_extraBreakpoint = handler;
}

void RegisterJvmtiFramePopHandler(JvmtiFramePopHandler handler)
{
    s_extraFramePop = handler;
    if (handler && s_jvmti && InterlockedCompareExchange(&s_installed, 0, 0) &&
        InterlockedCompareExchange(&s_hasFramePop, 0, 0) &&
        InterlockedCompareExchange(&s_framePopEnabled, 1, 0) == 0) {
        s_jvmti->SetEventNotificationMode(JVMTI_ENABLE, JVMTI_EVENT_FRAME_POP, nullptr);
    }
}

jvmtiEnv* SharedJvmtiEnv()
{
    return InterlockedCompareExchange(&s_installed, 0, 0) ? s_jvmti : nullptr;
}

bool SharedJvmtiSetBreakpoint(jmethodID method, jlocation location)
{
    if (!s_jvmti || !method || !InterlockedCompareExchange(&s_installed, 0, 0)) return false;
    return s_jvmti->SetBreakpoint(method, location) == JVMTI_ERROR_NONE;
}

bool SharedJvmtiClearBreakpoint(jmethodID method, jlocation location)
{
    if (!s_jvmti || !method || !InterlockedCompareExchange(&s_installed, 0, 0)) return false;
    return s_jvmti->ClearBreakpoint(method, location) == JVMTI_ERROR_NONE;
}

bool SharedJvmtiNotifyFramePop(jthread thread, jint depth)
{
    if (!s_jvmti || !thread || !InterlockedCompareExchange(&s_installed, 0, 0)) return false;
    if (!InterlockedCompareExchange(&s_hasFramePop, 0, 0)) return false;
    return s_jvmti->NotifyFramePop(thread, depth) == JVMTI_ERROR_NONE;
}

} // namespace lc
