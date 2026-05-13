#pragma once
// jni_core/matrix_reader.h
// Reads a 4×4 float matrix from a java.nio.FloatBuffer with minimal JNI calls.
//
// Strategy (fastest to slowest):
//   1. GetDirectBufferAddress — zero JNI calls after the first; just a memcpy.
//   2. GetFloatArrayRegion on the backing array — 1 JNI call for all 16 floats.
//   3. CallFloatMethod(get, i) × 16 — original fallback, kept for safety.
//
// Usage:
//   float m[16];
//   bool ok = ReadFloatBuffer16(env, floatBufferObj, m);

#include <jni.h>
#include <cstring>

// Reads 16 floats from a java.nio.FloatBuffer into out[16].
// Returns true on success.  floatBufGet is the FloatBuffer.get(I)F method ID
// (used only for the slow fallback; may be nullptr to skip that path).
inline bool ReadFloatBuffer16(JNIEnv* env, jobject buf, float out[16],
                               jmethodID floatBufGet = nullptr) {
    if (!env || !buf) return false;

    // ── Path 1: direct buffer (fastest) ──────────────────────────────────────
    void* addr = env->GetDirectBufferAddress(buf);
    if (addr) {
        jlong cap = env->GetDirectBufferCapacity(buf);
        if (cap >= 16) {
            memcpy(out, addr, 16 * sizeof(float));
            return true;
        }
    }
    if (env->ExceptionCheck()) env->ExceptionClear();

    // ── Path 2: backing array region copy ────────────────────────────────────
    // FloatBuffer.hasArray() / array() — try to get the backing float[]
    {
        jclass fbCls = env->GetObjectClass(buf);
        if (fbCls) {
            jmethodID mHasArray = env->GetMethodID(fbCls, "hasArray", "()Z");
            if (env->ExceptionCheck()) { env->ExceptionClear(); mHasArray = nullptr; }
            if (mHasArray) {
                jboolean has = env->CallBooleanMethod(buf, mHasArray);
                if (!env->ExceptionCheck() && has) {
                    jmethodID mArray = env->GetMethodID(fbCls, "array", "()[F");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); mArray = nullptr; }
                    jmethodID mOffset = env->GetMethodID(fbCls, "arrayOffset", "()I");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); mOffset = nullptr; }
                    if (mArray) {
                        jfloatArray arr = (jfloatArray)env->CallObjectMethod(buf, mArray);
                        if (!env->ExceptionCheck() && arr) {
                            jint offset = 0;
                            if (mOffset) {
                                offset = env->CallIntMethod(buf, mOffset);
                                if (env->ExceptionCheck()) { env->ExceptionClear(); offset = 0; }
                            }
                            jsize len = env->GetArrayLength(arr);
                            if (len >= offset + 16) {
                                env->GetFloatArrayRegion(arr, offset, 16, out);
                                if (!env->ExceptionCheck()) {
                                    env->DeleteLocalRef(arr);
                                    env->DeleteLocalRef(fbCls);
                                    return true;
                                }
                                env->ExceptionClear();
                            }
                            env->DeleteLocalRef(arr);
                        } else if (env->ExceptionCheck()) {
                            env->ExceptionClear();
                        }
                    }
                } else if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                }
            }
            env->DeleteLocalRef(fbCls);
        }
    }

    // ── Path 3: CallFloatMethod × 16 (original safe fallback) ────────────────
    if (floatBufGet) {
        for (int i = 0; i < 16; i++) {
            out[i] = env->CallFloatMethod(buf, floatBufGet, i);
            if (env->ExceptionCheck()) {
                env->ExceptionClear();
                return false;
            }
        }
        return true;
    }

    return false;
}
