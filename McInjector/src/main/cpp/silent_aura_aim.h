#pragma once

// Pure SilentAura aiming helpers (Rise / OpenMyau-Plus inspired).
// Credit: https://github.com/IamNespola/OpenMyau-Plus
// Free of JNI/Windows headers so they can be unit-tested in the native harness.

#include <cmath>
#include <algorithm>

namespace saaim {

struct Vec3 {
    double x;
    double y;
    double z;
};

struct Angles {
    float yaw;
    float pitch;
};

struct AimPointFracs {
    float x; // 0..1 across AABB width
    float y; // 0..1 across AABB height
    float z; // 0..1 across AABB depth
};

struct RotationState {
    float yawVel;
    float pitchVel;
    float bodyYaw;
    bool bodyValid;
    bool overshootActive;
    float overshootYaw;
    float overshootPitch;
};

struct StepResult {
    float yaw;
    float pitch;
    float bodyYaw;
    float yawVel;
    float pitchVel;
    bool overshootActive;
    float overshootYaw;
    float overshootPitch;
};

inline float Clamp(float v, float lo, float hi) {
    if (v < lo) return lo;
    if (v > hi) return hi;
    return v;
}

inline float NormalizeAngle(float angle) {
    while (angle > 180.0f) angle -= 360.0f;
    while (angle < -180.0f) angle += 360.0f;
    return angle;
}

inline float ShortestYawDelta(float from, float to) {
    return NormalizeAngle(to - from);
}

// Minecraft mouse GCD: f = sens*0.6+0.2; step = f^3 * 1.2  (= f^3 * 8 * 0.15)
inline float MouseGcd(float sensitivity) {
    float sens = Clamp(sensitivity, 0.01f, 1.0f);
    float f = sens * 0.6f + 0.2f;
    return f * f * f * 1.2f;
}

// Quantize a rotation toward target onto the mouse GCD grid (Rise applySensitivityPatch).
inline Angles ApplySensitivityPatch(
        float prevYaw, float prevPitch,
        float newYaw, float newPitch,
        float sensitivity) {
    float gcd = MouseGcd(sensitivity);
    if (gcd < 1e-6f) gcd = 0.15f;

    float dyaw = ShortestYawDelta(prevYaw, newYaw);
    float dpitch = newPitch - prevPitch;

    float qYaw = prevYaw + (float)std::round(dyaw / gcd) * gcd;
    float qPitch = prevPitch + (float)std::round(dpitch / gcd) * gcd;
    qPitch = Clamp(qPitch, -90.0f, 90.0f);

    Angles out;
    out.yaw = qYaw;
    out.pitch = qPitch;
    return out;
}

// Sample yaw step (deg per ~20 ms scan) between min/max turn speed (Rise-style).
inline float SampleYawStepDeg(float minTurn, float maxTurn, float rand01) {
    float lo = Clamp(minTurn, 4.0f, 40.0f);
    float hi = Clamp(maxTurn, 4.0f, 40.0f);
    if (lo > hi) {
        float tmp = lo;
        lo = hi;
        hi = tmp;
    }
    float u = Clamp(rand01, 0.0f, 1.0f);
    return lo + u * (hi - lo);
}

inline float PitchStepFromYaw(float yawStep) {
    return yawStep * 0.65f;
}

// Legacy single rot-speed (10..90) -> mid yaw step used for profile migration.
inline float LegacyRotSpeedToYawStep(float rotSpeed) {
    float t = (Clamp(rotSpeed, 10.0f, 90.0f) - 10.0f) / 80.0f;
    return 4.0f + t * 28.0f;
}

// Randomization 0..100 -> noise amplitude. Never zero (tiny residual at 0).
inline float NoiseAmplitudeDeg(float randomization) {
    float t = Clamp(randomization, 0.0f, 100.0f) / 100.0f;
    return 0.08f + t * 2.4f; // ~0.08 .. 2.48 deg
}

// Randomization 0..100 -> aim-point wander (fraction of hitbox). Tiny residual at 0.
inline float AimWanderStrength(float randomization) {
    float t = Clamp(randomization, 0.0f, 100.0f) / 100.0f;
    return 0.04f + t * 0.28f; // ~0.04 .. 0.32
}

inline Angles AnglesToPoint(
        double eyeX, double eyeY, double eyeZ,
        double pointX, double pointY, double pointZ) {
    double dx = pointX - eyeX;
    double dy = pointY - eyeY;
    double dz = pointZ - eyeZ;
    double horiz = std::sqrt(dx * dx + dz * dz);
    Angles out;
    out.yaw = 0.0f;
    out.pitch = 0.0f;
    if (!std::isfinite(horiz) || horiz < 1e-6) return out;

    const double RAD = 57.29577951308232;
    out.yaw = (float)(std::atan2(-dx, dz) * RAD);
    out.pitch = Clamp((float)(std::atan2(-dy, horiz) * RAD), -90.0f, 90.0f);
    return out;
}

// Player AABB defaults (1.8-tall player).
inline void PlayerAabb(double feetX, double feetY, double feetZ,
                       double* minX, double* minY, double* minZ,
                       double* maxX, double* maxY, double* maxZ) {
    const double halfW = 0.3;
    *minX = feetX - halfW;
    *maxX = feetX + halfW;
    *minY = feetY;
    *maxY = feetY + 1.8;
    *minZ = feetZ - halfW;
    *maxZ = feetZ + halfW;
}

inline Vec3 PointFromFracs(double minX, double minY, double minZ,
                           double maxX, double maxY, double maxZ,
                           float fx, float fy, float fz) {
    Vec3 p;
    p.x = minX + (maxX - minX) * (double)Clamp(fx, 0.0f, 1.0f);
    p.y = minY + (maxY - minY) * (double)Clamp(fy, 0.0f, 1.0f);
    p.z = minZ + (maxZ - minZ) * (double)Clamp(fz, 0.0f, 1.0f);
    return p;
}

// Score an aim point: prefer closer points in the body band with light noise.
inline double ScoreAimPoint(
        double eyeX, double eyeY, double eyeZ,
        double pointX, double pointY, double pointZ,
        double attackRange,
        double scoreNoise) {
    double dx = pointX - eyeX;
    double dy = pointY - eyeY;
    double dz = pointZ - eyeZ;
    double dist = std::sqrt(dx * dx + dy * dy + dz * dz);
    if (!std::isfinite(dist)) return 1e18;
    double score = dist;
    if (dist > attackRange) score += 10.0;
    // Prefer mid-torso slightly.
    score += std::abs(dy - 0.0) * 0.05;
    score += scoreNoise;
    return score;
}

// Pick best sample on the hitbox. Samples cover body band y in [0.2, 0.9].
// outFracs receives the winning fractions. Returns false if no valid sample.
template <typename Rand01>
inline bool SelectAimPointFracs(
        double eyeX, double eyeY, double eyeZ,
        double feetX, double feetY, double feetZ,
        double attackRange,
        float wander,
        Rand01 rand01,
        AimPointFracs* outFracs) {
    if (!outFracs) return false;

    double minX, minY, minZ, maxX, maxY, maxZ;
    PlayerAabb(feetX, feetY, feetZ, &minX, &minY, &minZ, &maxX, &maxY, &maxZ);

    const float yMin = 0.20f;
    const float yMax = 0.90f;
    const float hInset = 0.15f; // keep away from extreme edges a bit
    const float hMin = hInset;
    const float hMax = 1.0f - hInset;

    double bestScore = 1e18;
    bool found = false;
    AimPointFracs best = { 0.5f, 0.55f, 0.5f };

    for (float fy = yMin; fy <= yMax + 1e-4f; fy += 0.20f) {
        for (float fx = hMin; fx <= hMax + 1e-4f; fx += 0.25f) {
            for (float fz = hMin; fz <= hMax + 1e-4f; fz += 0.25f) {
                Vec3 p = PointFromFracs(minX, minY, minZ, maxX, maxY, maxZ, fx, fy, fz);
                double noise = (double)rand01() * (double)wander * 5.0;
                double score = ScoreAimPoint(eyeX, eyeY, eyeZ, p.x, p.y, p.z, attackRange, noise);
                if (score < bestScore) {
                    bestScore = score;
                    best.x = fx;
                    best.y = fy;
                    best.z = fz;
                    found = true;
                }
            }
        }
    }

    if (!found) return false;
    outFracs->x = best.x;
    outFracs->y = best.y;
    outFracs->z = best.z;
    return true;
}

// Speed-capped step with accel/decel, optional overshoot, noise, GCD, and lagged body yaw.
template <typename RandSigned01>
inline StepResult StepRotation(
        float currentYaw, float currentPitch,
        float targetYaw, float targetPitch,
        float maxYawStep, float maxPitchStep,
        float sensitivity,
        float noiseAmp,
        const RotationState& state,
        RandSigned01 randSigned01) {
    float yawErr = ShortestYawDelta(currentYaw, targetYaw);
    float pitchErr = targetPitch - currentPitch;
    float dist = std::sqrt(yawErr * yawErr + pitchErr * pitchErr);

    // Accel toward max when far; decelerate when close.
    float yawVel = state.yawVel;
    float pitchVel = state.pitchVel;
    float accel = 1.35f;
    float decel = 0.72f;

    float desiredYaw = Clamp(yawErr, -maxYawStep, maxYawStep);
    float desiredPitch = Clamp(pitchErr, -maxPitchStep, maxPitchStep);

    if (dist > 18.0f) {
        yawVel += (desiredYaw - yawVel) * accel * 0.35f;
        pitchVel += (desiredPitch - pitchVel) * accel * 0.35f;
    } else if (dist > 4.0f) {
        yawVel += (desiredYaw - yawVel) * 0.45f;
        pitchVel += (desiredPitch - pitchVel) * 0.45f;
    } else {
        yawVel *= decel;
        pitchVel *= decel;
        yawVel += desiredYaw * (1.0f - decel);
        pitchVel += desiredPitch * (1.0f - decel);
    }

    yawVel = Clamp(yawVel, -maxYawStep, maxYawStep);
    pitchVel = Clamp(pitchVel, -maxPitchStep, maxPitchStep);

    bool overshootActive = state.overshootActive;
    float overshootYaw = state.overshootYaw;
    float overshootPitch = state.overshootPitch;

    // Start a short overshoot when closing a large angle.
    if (!overshootActive && dist > 22.0f && dist < 55.0f && std::abs(yawErr) > 12.0f) {
        float strength = 2.0f + std::abs(randSigned01()) * 4.0f;
        float sign = (yawErr >= 0.0f) ? 1.0f : -1.0f;
        overshootYaw = sign * strength;
        overshootPitch = randSigned01() * strength * 0.35f;
        overshootActive = true;
    }

    float aimYaw = targetYaw;
    float aimPitch = targetPitch;
    if (overshootActive) {
        aimYaw = targetYaw + overshootYaw;
        aimPitch = Clamp(targetPitch + overshootPitch, -90.0f, 90.0f);
        overshootYaw *= 0.78f;
        overshootPitch *= 0.78f;
        if (std::abs(overshootYaw) < 0.35f && std::abs(overshootPitch) < 0.25f)
            overshootActive = false;
        // Retarget deltas toward overshot aim.
        yawErr = ShortestYawDelta(currentYaw, aimYaw);
        pitchErr = aimPitch - currentPitch;
        desiredYaw = Clamp(yawErr, -maxYawStep, maxYawStep);
        desiredPitch = Clamp(pitchErr, -maxPitchStep, maxPitchStep);
        yawVel = Clamp(yawVel * 0.7f + desiredYaw * 0.3f, -maxYawStep, maxYawStep);
        pitchVel = Clamp(pitchVel * 0.7f + desiredPitch * 0.3f, -maxPitchStep, maxPitchStep);
    }

    float nextYaw = currentYaw + yawVel;
    float nextPitch = Clamp(currentPitch + pitchVel, -90.0f, 90.0f);

    // Continuous micro-noise (never perfect).
    nextYaw += randSigned01() * noiseAmp;
    nextPitch = Clamp(nextPitch + randSigned01() * noiseAmp * 0.55f, -90.0f, 90.0f);

    Angles patched = ApplySensitivityPatch(currentYaw, currentPitch, nextYaw, nextPitch, sensitivity);
    nextYaw = patched.yaw;
    nextPitch = patched.pitch;

    // Lag body yaw behind head (~40% of delta).
    float bodyYaw = state.bodyValid ? state.bodyYaw : currentYaw;
    float bodyDelta = ShortestYawDelta(bodyYaw, nextYaw);
    bodyYaw = NormalizeAngle(bodyYaw + bodyDelta * 0.40f);

    StepResult out;
    out.yaw = nextYaw;
    out.pitch = nextPitch;
    out.bodyYaw = bodyYaw;
    out.yawVel = yawVel;
    out.pitchVel = pitchVel;
    out.overshootActive = overshootActive;
    out.overshootYaw = overshootYaw;
    out.overshootPitch = overshootPitch;
    return out;
}

// Silhouette hit test with randomization-scaled margin (no fixed +2° always-on pad).
inline bool IsOnTarget(
        double eyeX, double eyeY, double eyeZ,
        double feetX, double feetY, double feetZ,
        float currentYaw, float currentPitch,
        float randomization) {
    double dx = feetX - eyeX;
    double dz = feetZ - eyeZ;
    double horiz = std::sqrt(dx * dx + dz * dz);
    if (!std::isfinite(horiz) || horiz < 1e-6) return false;

    const double RAD = 57.29577951308232;
    const double boxHalfWidth = 0.30;
    const double aimCenterH = 0.95;
    const double aimHalfHeight = 0.70;

    float yawCenter = (float)(std::atan2(-dx, dz) * RAD);
    double dyCenter = (feetY + aimCenterH) - eyeY;
    float pitchCenter = (float)(std::atan2(-dyCenter, horiz) * RAD);

    float yawHalf = (float)(std::atan2(boxHalfWidth, horiz) * RAD);
    float pitchHalf = (float)(std::atan2(aimHalfHeight, horiz) * RAD);

    // Higher randomization -> slightly looser click gate.
    float t = Clamp(randomization, 0.0f, 100.0f) / 100.0f;
    float margin = 0.15f + t * 1.1f; // ~0.15 .. 1.25

    float yawErr = std::abs(ShortestYawDelta(yawCenter, currentYaw));
    float pitchErr = std::abs(currentPitch - pitchCenter);
    return (yawErr <= yawHalf + margin) && (pitchErr <= pitchHalf + margin);
}

// Humanized click interval from CPS range (ms). Adds jitter + occasional longer pause.
template <typename Rand01>
inline unsigned NextClickIntervalMs(float minCps, float maxCps, Rand01 rand01) {
    float lo = Clamp(minCps, 1.0f, 20.0f);
    float hi = Clamp(maxCps, 1.0f, 20.0f);
    if (lo > hi) lo = hi;
    float cps = lo;
    if (hi > lo)
        cps = lo + rand01() * (hi - lo);

    float interval = 1000.0f / std::max(1.0f, cps);
    // ±12% jitter
    interval *= (0.88f + rand01() * 0.24f);
    // Occasional longer pause (~8%)
    if (rand01() < 0.08f)
        interval *= (1.35f + rand01() * 0.55f);

    if (interval < 30.0f) interval = 30.0f;
    if (interval > 450.0f) interval = 450.0f;
    return (unsigned)interval;
}

} // namespace saaim
