#pragma once
// jni_core/scoped_env.h
// Thread-lifetime JNI env: attaches once per thread (TLS), never detaches mid-call.
// Usage:
//   JNIEnv* env = JniEnv::Get(g_jvm);   // attach if needed, returns nullptr on failure
//   JniEnv::DetachThisThread(g_jvm);     // call at thread exit (worker threads only)
//
// The render thread (SwapBuffers) calls Get() every frame — cost is one TLS read after
// the first call, vs. GetEnv()+AttachCurrentThread() on every ScopedJNIEnv construction.

#include <jni.h>
#include <windows.h>

namespace JniEnv {

namespace detail {
    inline DWORD& TlsSlot() {
        static DWORD s_slot = []{ DWORD d = TlsAlloc(); return d; }();
        return s_slot;
    }
} // namespace detail

// Returns the JNIEnv* for the calling thread, attaching if necessary.
// Returns nullptr only if the JVM is gone or attach fails.
inline JNIEnv* Get(JavaVM* jvm) {
    if (!jvm) return nullptr;
    DWORD slot = detail::TlsSlot();
    if (slot == TLS_OUT_OF_INDEXES) return nullptr;

    JNIEnv* env = static_cast<JNIEnv*>(TlsGetValue(slot));
    if (env) return env;

    // Not yet attached on this thread.
    jint rc = jvm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_8);
    if (rc == JNI_OK && env) {
        TlsSetValue(slot, env);
        return env;
    }
    if (rc == JNI_EDETACHED) {
        if (jvm->AttachCurrentThread(reinterpret_cast<void**>(&env), nullptr) == JNI_OK && env) {
            TlsSetValue(slot, env);
            return env;
        }
    }
    return nullptr;
}

// Call at the end of a worker thread's lifetime (NOT the render thread — the JVM
// owns that attachment).  Clears the TLS slot so Get() re-attaches if the thread
// is somehow reused.
inline void DetachThisThread(JavaVM* jvm) {
    if (!jvm) return;
    DWORD slot = detail::TlsSlot();
    if (slot == TLS_OUT_OF_INDEXES) return;
    JNIEnv* env = static_cast<JNIEnv*>(TlsGetValue(slot));
    if (env) {
        jvm->DetachCurrentThread();
        TlsSetValue(slot, nullptr);
    }
}

} // namespace JniEnv
