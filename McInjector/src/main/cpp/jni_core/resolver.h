#pragma once
// jni_core/resolver.h
// Typed, cached JNI descriptor resolution.
//
// Each descriptor holds a null-terminated array of candidate names/signatures
// (Yarn first, Mojmap second, obfuscated last).  Resolve() tries each combination
// in order, caches the first hit, and returns it on every subsequent call with
// zero JNI overhead.
//
// Usage:
//   // Declare once (in mappings_*.h):
//   extern FieldDesc FIELD_PLAYER;
//
//   // Resolve (idempotent, cheap after first call):
//   jfieldID fid = Resolve(env, FIELD_PLAYER, classLoader);
//
// Thread safety: descriptors are written once (during discovery, under the remap
// mutex) and read-only thereafter.  No additional locking needed for reads.

#include <jni.h>

// ── Class descriptor ────────────────────────────────────────────────────────
// classNames: null-terminated list of dot-separated class names to try.
struct ClassDesc {
    const char* const* classNames; // {"net.minecraft.client.Minecraft", "net.minecraft.class_310", nullptr}
    mutable jclass     cached;     // global ref, set on first successful Resolve

    ClassDesc(const char* const* names) : classNames(names), cached(nullptr) {}
};

// ── Field descriptor ─────────────────────────────────────────────────────────
// Tries every (fieldName, fieldSig) pair on the resolved owner class.
struct FieldDesc {
    ClassDesc&         owner;
    const char* const* fieldNames; // {"player", "field_1724", "f_91074_", nullptr}
    const char* const* fieldSigs;  // {"Lnet/.../LocalPlayer;", "Lnet/minecraft/class_746;", nullptr}
    bool               isStatic;
    mutable jfieldID   cached;

    FieldDesc(ClassDesc& owner,
              const char* const* names,
              const char* const* sigs,
              bool isStatic = false)
        : owner(owner), fieldNames(names), fieldSigs(sigs),
          isStatic(isStatic), cached(nullptr) {}
};

// ── Method descriptor ────────────────────────────────────────────────────────
struct MethodDesc {
    ClassDesc&         owner;
    const char* const* methodNames;
    const char* const* methodSigs;
    bool               isStatic;
    mutable jmethodID  cached;

    MethodDesc(ClassDesc& owner,
               const char* const* names,
               const char* const* sigs,
               bool isStatic = false)
        : owner(owner), methodNames(names), methodSigs(sigs),
          isStatic(isStatic), cached(nullptr) {}
};

// ── Resolution functions ─────────────────────────────────────────────────────
// classLoader may be nullptr; FindClass is used as fallback.

jclass    Resolve(JNIEnv* env, ClassDesc&  desc, jobject classLoader = nullptr);
jfieldID  Resolve(JNIEnv* env, FieldDesc&  desc, jobject classLoader = nullptr);
jmethodID Resolve(JNIEnv* env, MethodDesc& desc, jobject classLoader = nullptr);

// Invalidate all cached values (call during remap / world reload).
void ResetDesc(ClassDesc&  desc, JNIEnv* env);
void ResetDesc(FieldDesc&  desc);
void ResetDesc(MethodDesc& desc);
