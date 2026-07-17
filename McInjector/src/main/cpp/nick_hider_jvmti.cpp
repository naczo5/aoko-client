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

static void JNICALL OnBreakpoint(jvmtiEnv*, JNIEnv* env, jthread thread, jmethodID method, jlocation)
{
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

bool InstallNickHiderJvmti(JavaVM* vm, NickHiderJvmtiGeneration generation, void (*logger)(const std::string&))
{
    if (InterlockedCompareExchange(&s_installed, 0, 0)) return true;
    if (!vm) return false;
    s_vm = vm;
    s_generation = generation;
    s_log = logger;
    if (vm->GetEnv(reinterpret_cast<void**>(&s_jvmti), JVMTI_VERSION_1_2) != JNI_OK || !s_jvmti) {
        Log("NickHider interceptor unavailable: JVMTI environment was not provided by this JVM.");
        return false;
    }
    jvmtiCapabilities capabilities = {};
    capabilities.can_access_local_variables = 1;
    capabilities.can_generate_breakpoint_events = 1;
    const jvmtiError capResult = s_jvmti->AddCapabilities(&capabilities);
    if (capResult != JVMTI_ERROR_NONE) {
        Log("NickHider interceptor unavailable: JVMTI breakpoint/local-variable capability denied (" + std::to_string((int)capResult) + ").");
        return false;
    }
    jvmtiEventCallbacks callbacks = {};
    callbacks.Breakpoint = OnBreakpoint;
    if (s_jvmti->SetEventCallbacks(&callbacks, sizeof(callbacks)) != JVMTI_ERROR_NONE ||
        s_jvmti->SetEventNotificationMode(JVMTI_ENABLE, JVMTI_EVENT_BREAKPOINT, nullptr) != JVMTI_ERROR_NONE) {
        Log("NickHider interceptor unavailable: JVMTI breakpoint callback registration failed.");
        return false;
    }
    InterlockedExchange(&s_installed, 1);
    RefreshNickHiderJvmtiTargets();
    return true;
}

bool IsNickHiderJvmtiInstalled()
{
    return InterlockedCompareExchange(&s_installed, 0, 0) != 0;
}

void RefreshNickHiderJvmtiTargets()
{
    if (!s_jvmti || !InterlockedCompareExchange(&s_installed, 0, 0)) return;
    jint classCount = 0;
    jclass* classes = nullptr;
    if (s_jvmti->GetLoadedClasses(&classCount, &classes) != JVMTI_ERROR_NONE || !classes) return;
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
        if (IsRendererClass(signature)) AddRendererMethods(classes[i]);
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
    if (s_jvmti) s_jvmti->SetEventNotificationMode(JVMTI_DISABLE, JVMTI_EVENT_BREAKPOINT, nullptr);
    if (env) {
        for (LONG i = 0; i < InterlockedCompareExchange(&s_surfaceCount, 0, 0); ++i)
            if (s_surfaces[i].clazz) env->DeleteGlobalRef(s_surfaces[i].clazz);
    }
    InterlockedExchange(&s_surfaceCount, 0);
    InterlockedExchange(&s_methodCount, 0);
    InterlockedExchange(&s_installed, 0);
}

} // namespace lc
