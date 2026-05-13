#pragma once
// jni_core/local_frame.h
// RAII wrapper for JNI PushLocalFrame / PopLocalFrame.
// Usage:
//   LocalFrame frame(env, 64);
//   if (!frame.ok()) return;
//   // ... create local refs freely ...
//   // PopLocalFrame called automatically on scope exit.

#include <jni.h>

class LocalFrame {
    JNIEnv* m_env;
    bool    m_ok;
public:
    explicit LocalFrame(JNIEnv* env, jint capacity = 64) : m_env(env), m_ok(false) {
        if (env && env->PushLocalFrame(capacity) == 0)
            m_ok = true;
        else if (env)
            env->ExceptionClear();
    }
    ~LocalFrame() {
        if (m_ok) m_env->PopLocalFrame(nullptr);
    }
    bool ok() const { return m_ok; }

    // Non-copyable
    LocalFrame(const LocalFrame&) = delete;
    LocalFrame& operator=(const LocalFrame&) = delete;
};
