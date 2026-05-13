// jni_core/resolver.cpp
#include "resolver.h"
#include <string>
#include <algorithm>

// Load a class by dot-separated name, trying classLoader first then FindClass.
static jclass LoadClass(JNIEnv* env, const char* dotName, jobject classLoader) {
    if (!env || !dotName) return nullptr;

    // Convert dots to slashes for FindClass
    std::string slash = dotName;
    std::replace(slash.begin(), slash.end(), '.', '/');

    jclass cls = nullptr;

    if (classLoader) {
        // Use ClassLoader.loadClass(String) — required for Fabric/Lunar class loaders
        jclass clClass = env->FindClass("java/lang/ClassLoader");
        if (env->ExceptionCheck()) { env->ExceptionClear(); clClass = nullptr; }
        if (clClass) {
            jmethodID mLoad = env->GetMethodID(clClass, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); mLoad = nullptr; }
            if (mLoad) {
                jstring jname = env->NewStringUTF(dotName);
                if (jname) {
                    cls = (jclass)env->CallObjectMethod(classLoader, mLoad, jname);
                    if (env->ExceptionCheck()) { env->ExceptionClear(); cls = nullptr; }
                    env->DeleteLocalRef(jname);
                }
            }
            env->DeleteLocalRef(clClass);
        }
    }

    if (!cls) {
        cls = env->FindClass(slash.c_str());
        if (env->ExceptionCheck()) { env->ExceptionClear(); cls = nullptr; }
    }

    return cls; // local ref or nullptr
}

// ── ClassDesc ────────────────────────────────────────────────────────────────

jclass Resolve(JNIEnv* env, ClassDesc& desc, jobject classLoader) {
    if (desc.cached) return desc.cached;
    if (!env || !desc.classNames) return nullptr;

    for (int i = 0; desc.classNames[i]; i++) {
        jclass local = LoadClass(env, desc.classNames[i], classLoader);
        if (local) {
            desc.cached = (jclass)env->NewGlobalRef(local);
            env->DeleteLocalRef(local);
            return desc.cached;
        }
    }
    return nullptr;
}

// ── FieldDesc ────────────────────────────────────────────────────────────────

jfieldID Resolve(JNIEnv* env, FieldDesc& desc, jobject classLoader) {
    if (desc.cached) return desc.cached;

    jclass cls = Resolve(env, desc.owner, classLoader);
    if (!cls) return nullptr;

    for (int ni = 0; desc.fieldNames[ni]; ni++) {
        for (int si = 0; desc.fieldSigs[si]; si++) {
            jfieldID fid = desc.isStatic
                ? env->GetStaticFieldID(cls, desc.fieldNames[ni], desc.fieldSigs[si])
                : env->GetFieldID(cls, desc.fieldNames[ni], desc.fieldSigs[si]);
            if (env->ExceptionCheck()) { env->ExceptionClear(); fid = nullptr; }
            if (fid) { desc.cached = fid; return fid; }
        }
    }
    return nullptr;
}

// ── MethodDesc ───────────────────────────────────────────────────────────────

jmethodID Resolve(JNIEnv* env, MethodDesc& desc, jobject classLoader) {
    if (desc.cached) return desc.cached;

    jclass cls = Resolve(env, desc.owner, classLoader);
    if (!cls) return nullptr;

    for (int ni = 0; desc.methodNames[ni]; ni++) {
        for (int si = 0; desc.methodSigs[si]; si++) {
            jmethodID mid = desc.isStatic
                ? env->GetStaticMethodID(cls, desc.methodNames[ni], desc.methodSigs[si])
                : env->GetMethodID(cls, desc.methodNames[ni], desc.methodSigs[si]);
            if (env->ExceptionCheck()) { env->ExceptionClear(); mid = nullptr; }
            if (mid) { desc.cached = mid; return mid; }
        }
    }
    return nullptr;
}

// ── Reset ────────────────────────────────────────────────────────────────────

void ResetDesc(ClassDesc& desc, JNIEnv* env) {
    if (desc.cached && env) env->DeleteGlobalRef(desc.cached);
    desc.cached = nullptr;
}

void ResetDesc(FieldDesc& desc) {
    desc.cached = nullptr;
    // jfieldID is not a reference; no DeleteGlobalRef needed.
}

void ResetDesc(MethodDesc& desc) {
    desc.cached = nullptr;
}
