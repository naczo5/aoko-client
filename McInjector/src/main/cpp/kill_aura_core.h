#pragma once

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace killaura {

enum TargetMode { TARGET_SINGLE = 0, TARGET_SWITCH = 1 };
enum SortMode { SORT_DISTANCE = 0, SORT_HEALTH = 1, SORT_HURT_TIME = 2, SORT_FOV = 3 };
enum RotationMode { ROT_NONE = 0, ROT_LEGIT = 1, ROT_SILENT = 2, ROT_LOCK_VIEW = 3, ROT_LIQUID_BOUNCE = 4, ROT_HYPIXEL = 5, ROT_GROK = 6 };
enum MoveFixMode { MOVE_FIX_NONE = 0, MOVE_FIX_SILENT = 1, MOVE_FIX_STRICT = 2 };
enum AutoBlockMode { BLOCK_NONE = 0, BLOCK_VANILLA = 1, BLOCK_SPOOF = 2, BLOCK_HYPIXEL = 3, BLOCK_BLINK = 4, BLOCK_INTERACT = 5, BLOCK_SWAP = 6, BLOCK_LEGIT = 7, BLOCK_FAKE = 8, BLOCK_MORDEN = 9 };

struct Vec3 { double x, y, z; };
struct Box { double minX, minY, minZ, maxX, maxY, maxZ; };
struct Angles { float yaw, pitch; };

inline float Clamp(float value, float lo, float hi) { return std::max(lo, std::min(hi, value)); }
inline float Wrap(float angle)
{
    angle = std::fmod(angle, 360.0f);
    if (angle >= 180.0f) angle -= 360.0f;
    if (angle < -180.0f) angle += 360.0f;
    return angle;
}
inline float Shortest(float from, float to) { return Wrap(to - from); }

inline Vec3 ClampToBox(const Vec3& point, const Box& box)
{
    Vec3 out;
    out.x = std::max(box.minX, std::min(box.maxX, point.x));
    out.y = std::max(box.minY, std::min(box.maxY, point.y));
    out.z = std::max(box.minZ, std::min(box.maxZ, point.z));
    return out;
}

inline double DistanceToBox(const Vec3& eyes, const Box& box)
{
    Vec3 p = ClampToBox(eyes, box);
    double dx = p.x - eyes.x, dy = p.y - eyes.y, dz = p.z - eyes.z;
    return std::sqrt(dx * dx + dy * dy + dz * dz);
}

inline bool RayIntersectsBox(const Vec3& origin, const Angles& rotation,
                             const Box& box, double maxDistance)
{
    const double yaw = rotation.yaw * 3.14159265358979323846 / 180.0;
    const double pitch = rotation.pitch * 3.14159265358979323846 / 180.0;
    const Vec3 direction = { -std::sin(yaw) * std::cos(pitch),
                             -std::sin(pitch),
                              std::cos(yaw) * std::cos(pitch) };
    double tMin = 0.0, tMax = maxDistance;
    const double origins[3] = { origin.x, origin.y, origin.z };
    const double dirs[3] = { direction.x, direction.y, direction.z };
    const double mins[3] = { box.minX, box.minY, box.minZ };
    const double maxs[3] = { box.maxX, box.maxY, box.maxZ };
    for (int axis = 0; axis < 3; ++axis) {
        if (std::abs(dirs[axis]) < 1.0e-9) {
            if (origins[axis] < mins[axis] || origins[axis] > maxs[axis]) return false;
            continue;
        }
        double t1 = (mins[axis] - origins[axis]) / dirs[axis];
        double t2 = (maxs[axis] - origins[axis]) / dirs[axis];
        if (t1 > t2) std::swap(t1, t2);
        tMin = std::max(tMin, t1);
        tMax = std::min(tMax, t2);
        if (tMin > tMax) return false;
    }
    return tMax >= 0.0 && tMin <= maxDistance;
}

inline float AngleToEntity(const Vec3& eyes, float playerYaw, double entityX, double entityZ)
{
    double dx = entityX - eyes.x, dz = entityZ - eyes.z;
    float target = (float)(std::atan2(dz, dx) * 180.0 / 3.14159265358979323846) - 90.0f;
    return std::abs(Wrap(target - playerYaw)) * 2.0f;
}

inline float MouseGcd(float sensitivity)
{
    float value = sensitivity * 0.6f + 0.2f;
    return value * value * value * 1.2f;
}

inline Angles GcdPatch(const Angles& desired, const Angles& previous, float sensitivity)
{
    float mult = MouseGcd(sensitivity);
    Angles out;
    out.yaw = previous.yaw + std::round((desired.yaw - previous.yaw) / mult) * mult;
    out.pitch = Clamp(previous.pitch + std::round((desired.pitch - previous.pitch) / mult) * mult, -90.0f, 90.0f);
    return out;
}

inline float SmoothAngle(float delta, float smoothFactor, float randomMinusPointOneToPointOne)
{
    float randomized = Clamp(smoothFactor + randomMinusPointOneToPointOne, 0.0f, 1.0f);
    return delta * (0.5f + 0.5f * (1.0f - randomized));
}

inline Angles StandardRotation(const Vec3& eyes, const Box& box, const Angles& current,
                               float maxAngle, float smoothFactor,
                               float yawSmoothRandom, float pitchSmoothRandom)
{
    double minY = box.minY + 0.05 * (box.maxY - box.minY);
    double maxY = box.minY + 0.75 * (box.maxY - box.minY);
    double dx = (box.minX + box.maxX) * 0.5 - eyes.x;
    double dy = eyes.y >= maxY ? maxY - eyes.y : (eyes.y <= minY ? minY - eyes.y : 0.0);
    double dz = (box.minZ + box.maxZ) * 0.5 - eyes.z;
    double horizontal = std::sqrt(dx * dx + dz * dz);
    float yawDelta = Wrap((float)(std::atan2(dz, dx) * 180.0 / 3.14159265358979323846) - 90.0f - current.yaw);
    float pitchDelta = Wrap((float)(-std::atan2(dy, horizontal) * 180.0 / 3.14159265358979323846) - current.pitch);
    yawDelta = std::abs(yawDelta) <= 1.0f ? 0.0f : SmoothAngle(Clamp(yawDelta, -maxAngle, maxAngle), smoothFactor, yawSmoothRandom);
    pitchDelta = std::abs(pitchDelta) <= 1.0f ? 0.0f : SmoothAngle(Clamp(pitchDelta, -maxAngle, maxAngle), smoothFactor, pitchSmoothRandom);
    Angles out = {
        (float)((double)(current.yaw + yawDelta) - std::fmod((double)(current.yaw + yawDelta), (double)0.0096f)),
        (float)((double)(current.pitch + pitchDelta) - std::fmod((double)(current.pitch + pitchDelta), (double)0.0096f))
    };
    return out;
}

inline Angles LimitAngleChange(const Angles& current, const Angles& target,
                               float horizontalSpeed, float verticalSpeed, float smoothFactor)
{
    float yaw = Clamp(Wrap(target.yaw - current.yaw), -horizontalSpeed, horizontalSpeed) * smoothFactor;
    float pitch = Clamp(Wrap(target.pitch - current.pitch), -verticalSpeed, verticalSpeed) * smoothFactor;
    Angles out = { current.yaw + yaw, Clamp(current.pitch + pitch, -90.0f, 90.0f) };
    return out;
}

inline Angles RavenRaw(const Vec3& player, float playerEyeHeight, float playerYaw, float playerPitch,
                       const Vec3& entity, const Vec3& entityLast, float entityEyeHeight, int predictTicks)
{
    double x = entity.x + (entity.x - entityLast.x) * predictTicks;
    double z = entity.z + (entity.z - entityLast.z) * predictTicks;
    double dx = x - player.x, dz = z - player.z;
    double dy = entity.y + entityEyeHeight * 0.9 - (player.y + playerEyeHeight);
    float yaw = playerYaw + Wrap((float)(std::atan2(dz, dx) * 57.295780181884766) - 90.0f - playerYaw);
    float pitch = Clamp(playerPitch + Wrap((float)(-std::atan2(dy, std::sqrt(dx * dx + dz * dz)) * 57.295780181884766) - playerPitch) + 3.0f, -90.0f, 90.0f);
    Angles out = { yaw, pitch };
    return out;
}

inline Angles RavenSmooth(const Angles& desired, const Angles& server, int smoothing, float strafe, float motionY)
{
    float unwrappedYaw = server.yaw + Wrap(desired.yaw - server.yaw);
    float deltaYaw = unwrappedYaw - server.yaw;
    float deltaPitch = desired.pitch - server.pitch;
    float yawSmoothing = (float)smoothing;
    float pitchSmoothing = yawSmoothing;
    if ((strafe < 0 && deltaYaw < 0) || (strafe > 0 && deltaYaw > 0)) yawSmoothing = std::max(1.0f, yawSmoothing / 2.0f);
    if ((motionY > 0 && deltaPitch > 0) || (motionY < 0 && deltaPitch < 0)) pitchSmoothing = std::max(1.0f, pitchSmoothing / 2.0f);
    Angles out = { server.yaw + deltaYaw / std::max(1.0f, yawSmoothing), server.pitch + deltaPitch / std::max(1.0f, pitchSmoothing) };
    return out;
}

inline Angles SmoothBack(const Angles& current, const Angles& target)
{
    float yawDiff = Wrap(target.yaw - current.yaw);
    float pitchDiff = Wrap(target.pitch - current.pitch);
    float speed = std::max(5.0f, std::abs(yawDiff) * 0.3f);
    Angles out = { current.yaw + Clamp(yawDiff, -speed, speed), current.pitch + Clamp(pitchDiff, -speed, speed) };
    return out;
}

// Grok: silent rotations constrained to a cone around the real camera so Premotion
// look stamps stay prediction-safe (Hypixel lag-patch movement simulation).
struct GrokState {
    float yawVel;
    float pitchVel;
    float orbitPhase;
    bool engaged;
    GrokState() : yawVel(0), pitchVel(0), orbitPhase(0), engaged(false) {}
};

struct GrokParams {
    float maxSkewYaw;
    float maxSkewPitch;
    float deadZone;
    float minTurnSpeed;
    float maxTurnSpeed;
    float acceleration;
    float deceleration;
    float noiseStrength;
    float clientFovHalf;
};

struct GrokResult {
    Angles desired;
    float yawVel;
    float pitchVel;
    float orbitPhase;
    bool engage;
};

inline Angles ClampAnglesToCone(const Angles& desired, const Angles& client,
                                float maxSkewYaw, float maxSkewPitch)
{
    float dy = Clamp(Shortest(client.yaw, desired.yaw), -maxSkewYaw, maxSkewYaw);
    float dp = Clamp(desired.pitch - client.pitch, -maxSkewPitch, maxSkewPitch);
    Angles out = { client.yaw + dy, Clamp(client.pitch + dp, -90.0f, 90.0f) };
    return out;
}

inline bool AnglesWithinCone(const Angles& angles, const Angles& client,
                             float maxSkewYaw, float maxSkewPitch)
{
    return std::abs(Shortest(client.yaw, angles.yaw)) <= maxSkewYaw + 1.0e-3f
        && std::abs(angles.pitch - client.pitch) <= maxSkewPitch + 1.0e-3f;
}

inline Angles GrokOrbitIntent(const Angles& target, float phase, float yawAmp, float pitchAmp)
{
    Angles out = {
        Wrap(target.yaw + std::sin(phase) * yawAmp),
        Clamp(target.pitch + std::cos(phase * 0.73f) * pitchAmp, -90.0f, 90.0f)
    };
    return out;
}

// Second-order impulse tracker with soft orbit, dead zone, jerk limit, and
// hard client-cone clamp. engage==false means do not stamp silent look / attack.
inline GrokResult GrokStep(const Angles& from, const Angles& target, const Angles& client,
                           const GrokState& state, const GrokParams& params,
                           float rand01, float randSigned01)
{
    const float skewYaw = Clamp(params.maxSkewYaw, 6.0f, 25.0f) * (0.92f + Clamp(rand01, 0.0f, 1.0f) * 0.16f);
    const float skewPitch = Clamp(params.maxSkewPitch, 4.0f, 20.0f)
        * (0.92f + Clamp(1.0f - rand01, 0.0f, 1.0f) * 0.16f);
    const float yawAmp = std::min(skewYaw * 0.35f, 2.8f);
    const float pitchAmp = std::min(skewPitch * 0.35f, 1.6f);
    const float phase = state.orbitPhase + 0.11f + Clamp(rand01, 0.0f, 1.0f) * 0.04f;
    const Angles intent = GrokOrbitIntent(target, phase, yawAmp, pitchAmp);

    float yawErr = Shortest(from.yaw, intent.yaw);
    float pitchErr = intent.pitch - from.pitch;
    float dist = std::sqrt(yawErr * yawErr + pitchErr * pitchErr);

    float yawVel = state.yawVel;
    float pitchVel = state.pitchVel;
    const float maxYaw = std::max(params.minTurnSpeed, params.maxTurnSpeed);
    const float maxPitch = maxYaw / 1.6f;
    const float dead = Clamp(params.deadZone, 0.0f, 2.0f);

    if (dist <= dead) {
        yawVel *= 0.55f;
        pitchVel *= 0.55f;
        yawVel += randSigned01 * params.noiseStrength * 0.15f;
        pitchVel += randSigned01 * params.noiseStrength * 0.10f;
    } else {
        float desiredYawVel = Clamp(yawErr, -maxYaw, maxYaw);
        float desiredPitchVel = Clamp(pitchErr, -maxPitch, maxPitch);
        float accel = Clamp(params.acceleration * 0.15f, 0.05f, 0.9f);
        float decel = Clamp(params.deceleration * 0.12f, 0.05f, 0.85f);
        if (dist > 18.0f) {
            yawVel += (desiredYawVel - yawVel) * accel;
            pitchVel += (desiredPitchVel - pitchVel) * accel;
        } else if (dist > 4.0f) {
            yawVel += (desiredYawVel - yawVel) * 0.45f;
            pitchVel += (desiredPitchVel - pitchVel) * 0.45f;
        } else {
            yawVel *= (1.0f - decel);
            pitchVel *= (1.0f - decel);
            yawVel += desiredYawVel * decel;
            pitchVel += desiredPitchVel * decel;
        }
        yawVel = Clamp(yawVel, -maxYaw, maxYaw);
        pitchVel = Clamp(pitchVel, -maxPitch, maxPitch);
    }

    // Jerk limit — avoids linear / constant-accel rotation streams.
    const float maxJerkYaw = maxYaw * 0.35f;
    const float maxJerkPitch = maxJerkYaw / 1.6f;
    yawVel = state.yawVel + Clamp(yawVel - state.yawVel, -maxJerkYaw, maxJerkYaw);
    pitchVel = state.pitchVel + Clamp(pitchVel - state.pitchVel, -maxJerkPitch, maxJerkPitch);

    if (rand01 < 0.06f) {
        yawVel *= 0.15f;
        pitchVel *= 0.15f;
    }

    Angles desired = {
        from.yaw + yawVel + randSigned01 * params.noiseStrength,
        Clamp(from.pitch + pitchVel + randSigned01 * params.noiseStrength * 0.55f, -90.0f, 90.0f)
    };
    desired = ClampAnglesToCone(desired, client, skewYaw, skewPitch);

    const float fovHalf = std::max(15.0f, params.clientFovHalf);
    const bool targetInFov = std::abs(Shortest(client.yaw, target.yaw)) <= fovHalf
        && std::abs(target.pitch - client.pitch) <= fovHalf;
    const bool engage = AnglesWithinCone(desired, client, skewYaw, skewPitch) && targetInFov;

    GrokResult out;
    out.desired = desired;
    out.yawVel = yawVel;
    out.pitchVel = pitchVel;
    out.orbitPhase = phase;
    out.engage = engage;
    return out;
}

struct TargetCandidate
{
    std::string stableId;
    double distance;
    float healthScore;
    int hurtTime;
    float fov;
    bool inSwingRange;
    bool inAttackRange;
    bool priorityTarget;
};

inline void ApplyRangeAndPriorityTiers(std::vector<TargetCandidate>& targets)
{
    bool anySwing = false, anyAttack = false, anyPriority = false;
    for (size_t i = 0; i < targets.size(); ++i) {
        anySwing |= targets[i].inSwingRange;
        anyAttack |= targets[i].inAttackRange;
        anyPriority |= targets[i].priorityTarget;
    }
    targets.erase(std::remove_if(targets.begin(), targets.end(), [=](const TargetCandidate& t) {
        return (anySwing && !t.inSwingRange) || (anyAttack && !t.inAttackRange) || (anyPriority && !t.priorityTarget);
    }), targets.end());
}

inline void SortTargets(std::vector<TargetCandidate>& targets, SortMode mode)
{
    std::stable_sort(targets.begin(), targets.end(), [=](const TargetCandidate& a, const TargetCandidate& b) {
        if (mode == SORT_HEALTH && a.healthScore != b.healthScore) return a.healthScore < b.healthScore;
        if (mode == SORT_HURT_TIME && a.hurtTime != b.hurtTime) return a.hurtTime < b.hurtTime;
        if (mode == SORT_FOV && a.fov != b.fov) return a.fov < b.fov;
        return a.distance < b.distance;
    });
}

inline uint32_t NextNormalDelayMs(int minCps, int maxCps, double random01)
{
    if (minCps > maxCps) std::swap(minCps, maxCps);
    minCps = std::max(1, minCps); maxCps = std::max(minCps, maxCps);
    double sampled = minCps + random01 * ((maxCps + 1.0) - minCps);
    long cps = (long)sampled;
    return (uint32_t)(1000L / std::max(1L, cps));
}

// Exact OpenMyau-Plus recorded CPS trace. Kept as data, including zero/one ms entries.
static const uint8_t kRecordedPattern[] = {
16,22,14,46,18,8,8,63,25,25,12,39,26,18,6,62,26,18,21,40,26,8,16,46,26,20,15,50,25,10,11,43,
25,11,37,39,25,12,18,54,25,25,15,41,27,9,1,66,26,17,21,48,27,8,6,62,28,19,13,47,26,7,14,53,
27,16,29,38,27,8,6,60,27,22,19,45,26,10,10,62,25,20,28,22,26,19,11,57,26,16,32,36,26,9,9,66,
27,19,27,38,26,9,10,61,26,25,15,34,26,20,10,52,26,22,28,29,27,8,3,63,26,21,27,38,26,10,11,38,
27,15,31,39,25,13,10,45,27,14,27,40,26,10,6,51,26,18,31,27,27,11,14,47,26,23,21,35,26,12,13,41,
26,15,31,36,27,16,9,44,27,14,30,39,25,14,10,46,28,10,24,45,26,7,5,46,26,20,6,50,26,8,6,51,
26,17,20,40,27,25,1,32,26,20,9,46,25,15,12,30,26,11,25,46,27,13,10,36,27,20,15,41,26,8,6,41,
26,12,29,44,26,13,11,44,26,12,27,36,26,23,4,39,26,24,12,47,26,9,2,65,26,16,27,34,26,25,0,53,
26,16,3,47,27,16,10,41,26,18,25,38,26,11,10,50,27,20,20,29,26,11,7,66,26,20,18,31,26,21,21,28,
26,21,29,25,27,15,12,43,28,11,31,32,27,23,0,49,27,20,30,30,25,32,0,50,26,12,25,34,27,11,11,44,
27,23,26,25,27,16,11,46,26,13,32,35,28,9,5,48,26,21,29,37,26,10,7,48,27,20,21,41,24,7,18,46,
25,22,22,33,25,10,5,59,26,21,19,29,26,11,10,46,25,22,29,31,25,11,12,50,24,20,28,40,25,10,4,56,
25,16,36,30,24,10,9,63,25,22,22,32,25,9,8,58,27,10,43,30,26,8,3,60,26,24,14,42,26,12,9,49,
25,11,32,38,27,8,8,50,26,20,26,32,25,10,4,66,25,18,28,24,26,10,8,54,25,16,32,34,24,9,12,54,
25,18,18,41,28,9,16,50,28,15,21,46,27,9,8,49,26,21,18,36,26,15,10,54,27,22,27,32,25,9,15,48,
28,19,26,35,27,9,13,48,25,21,23,33,27,8,3,65,26,19,23,39,25,9,13,44,26,25,19,35,26,14,6,63,
27,15,23,32,28,8,2,65,26,19,24,34,27,12,0,49,26,21,34,34,26,8,9,60,26,23,19,34,26,10,5,59,
26,12,36,39,26,11,11,44,26,25,5,47,25,9,10,49,27,19,24,31,26,10,4,60,27,25,9,41,26,20,7,54,
24,11,35,35,26,9,5,67,26,17,19,43,26,24,17,39,25,16,11,45,25,9,3,60,25,25,16,37,28,9,5,55,
26,15,12,49,25,17,8,39,25,15,16,48,25,12,9,37,25,17,31,38,27,8,8,62,26,23,14,38,27,16,10,45,
26,13,25,42,25,9,8,57,27,12,36,38,27,13,11,30,27,21,24,47,25,10,6,54,26,13,28,42,25,10,5,47,
26,21,22,44,26,10,8,50,28,17,26,33,26,10,14,55,27,14,30,29,25,13,1,70,26,14,30,26,27,12,14,67,
25,21,4,33,25,11,5,48,26,21,21,39,25,11,1,55,26,11,29,32,26,12,10,50,27,16,26,36,27,23,3,57,
27,11,23,37,26,9,16,37,26,16,38,37,26,9,2,60,27,22,16,38,27,9,5,53,26,14,33,30,25,13,11,46,
25,23,22,43,24,10,13,51,25,21,25,35,27,8,16,48,25,21,19,42,25,12,12,49,26,21,18,42,25,12,13,51,
27,16,25,37,26,11,12,47,27,21,13,39,27,5,9,61,25,24,11,39,26,10,9,52,26,15,33,28,38,0,9,55,
26,14,39,24,25,10,9,52,27,13,29,36,25,12,9,49,25,22,30,26,26,10,2,66,27,17,30,31,26,14,7,64,
28,16,31,28,24,13,14,54,25,12,29,35,27,10,8,49,27,18,26,38,25,8,14,46,26,23,15,36,26,11,5,61,
27,23,8,42,25,9,10,57,26,11,29,37,25,11,9,56,27,11,32,35,26,12,6,62,27,20,33,27,27,10,14,50,
27,17,28,40,25,9,8,46,26,23,16,44,26,11,13,47,28,19,19,36,26,8,7,55,26,15,24,39,26,12,9,56,
26,15,28,36,25,10,10,51,25,17,32,36,25,9,7,58,26,11,31,32,26,7,14,57,26,13,22,25,24,9,14,42,
26,12,27,31,25,9,2,62,27,23,12,33,26,8,18,46,25,24,14,33,24,10,14,50,25,20,21,38,26,9,1,61,
25,11,30,35,26,10,10,53,25,18,22,35,25,8,4,44,25,25,21,37,24,13,6,35,27,11,34,32,25,9,10,51,
26,17,18,31,24,11,8,53,26,16,30,35,26,8,10,60,25,11,32,29,25,22,2,53,26,16,30,33,27,9,11,57,
25,13,32,30,25,14,10,67,24,21,29,35,27,8,12,70,26,14,19,42,27,22,0,57,27,12,31,33,25,9,12,62,
27,23,14,43,25,11,2,71,28,12,33,31,27,8,12,71,26,15,23,42,28,9,8,63,26,22,22,37,27,7,4,78,
27,20,26,34,25,9,15,64,27,21,23,32,26,12,11,77,25,11,32,29,26,9,15,63,27,19,23,38,26,10,15,57,
26,14,37,14,26,18,6,67,26,13,31,33,26,19,1,60,27,25,22,24,27,22,2,55,26,13,25,34,26,24,0,68,
25,20,22,31,25,11,4,80,24,22,22,29,26,16,8,81,25,11,22,38,27,10,11,50,27,18,35,32,26,10,5,76,
26,23,22,30,24,21,8,67,27,24,16,42,27,8,3
};

inline size_t RecordedPatternSize() { return sizeof(kRecordedPattern) / sizeof(kRecordedPattern[0]); }
inline uint32_t RecordedDelayMs(size_t index) { return kRecordedPattern[index % RecordedPatternSize()]; }
inline uint32_t AttackDelayMs(bool blocking, float autoBlockCps, bool recorded,
                              size_t patternIndex, int minCps, int maxCps, double random01)
{
    if (blocking) return (uint32_t)(1000.0f / Clamp(autoBlockCps, 1.0f, 10.0f));
    return recorded ? RecordedDelayMs(patternIndex)
                    : NextNormalDelayMs(minCps, maxCps, random01);
}

enum AutoBlockAction : uint32_t {
    ACTION_NONE = 0, ACTION_START_BLOCK = 1u << 0, ACTION_STOP_BLOCK = 1u << 1,
    ACTION_SWAP_EMPTY = 1u << 2, ACTION_SWAP_SWORD = 1u << 3, ACTION_INTERACT = 1u << 4,
    ACTION_BLINK_ON = 1u << 5, ACTION_BLINK_OFF = 1u << 6, ACTION_CANCEL_ATTACK = 1u << 7
};

struct AutoBlockState { int blockTick; int mordenTick; bool isBlocking; bool fakeBlocking; bool blinkReset; AutoBlockState() : blockTick(0), mordenTick(0), isBlocking(false), fakeBlocking(false), blinkReset(false) {} };
struct AutoBlockInput { AutoBlockMode mode; bool hasTarget; bool canBlock; bool playerBlocking; bool usingItem; bool diggingOrPlacing; bool slotsSynced; bool hasSecondSword; int attackDelayMs; };
struct AutoBlockResult { uint32_t actions; bool allowAttack; };

inline AutoBlockResult StepAutoBlock(AutoBlockState& state, const AutoBlockInput& in)
{
    AutoBlockResult out = { ACTION_NONE, true };
    if (!in.hasTarget || !in.canBlock) {
        state = AutoBlockState(); out.actions = ACTION_BLINK_OFF; return out;
    }
    state.isBlocking = true;
    switch (in.mode) {
    case BLOCK_NONE:
        state.isBlocking = in.usingItem;
        state.fakeBlocking = false;
        out.actions |= ACTION_BLINK_OFF;
        if (in.usingItem && !in.playerBlocking && !in.diggingOrPlacing) out.actions |= ACTION_START_BLOCK;
        if (!in.usingItem && in.playerBlocking && !in.diggingOrPlacing) out.actions |= ACTION_STOP_BLOCK;
        break;
    case BLOCK_VANILLA:
        state.fakeBlocking = false; out.actions |= ACTION_BLINK_OFF;
        if (!in.playerBlocking && !in.diggingOrPlacing) out.actions |= ACTION_START_BLOCK;
        break;
    case BLOCK_SPOOF:
        state.fakeBlocking = false; out.actions |= ACTION_BLINK_OFF;
        if (!in.diggingOrPlacing && in.slotsSynced && !(in.playerBlocking && state.blockTick != 0) && !(in.attackDelayMs > 0 && in.attackDelayMs <= 50)) {
            out.actions |= ACTION_SWAP_EMPTY | ACTION_START_BLOCK; state.blockTick = 1;
        } else state.blockTick = 0;
        break;
    case BLOCK_HYPIXEL:
    case BLOCK_BLINK:
    case BLOCK_LEGIT:
        state.fakeBlocking = in.mode != BLOCK_LEGIT;
        if (in.mode == BLOCK_BLINK) out.actions |= ACTION_BLINK_ON;
        else out.actions |= ACTION_BLINK_OFF;
        if (in.diggingOrPlacing) break;
        if (state.blockTick == 0) {
            if (!in.playerBlocking) out.actions |= ACTION_START_BLOCK;
            state.blockTick = 1;
        } else {
            if (in.playerBlocking) { out.actions |= ACTION_STOP_BLOCK | ACTION_CANCEL_ATTACK; out.allowAttack = false; }
            if (in.attackDelayMs <= 50) state.blockTick = 0;
        }
        break;
    case BLOCK_INTERACT:
        state.fakeBlocking = true; out.actions |= ACTION_BLINK_ON;
        if (!in.diggingOrPlacing && in.slotsSynced) {
            if (state.blockTick == 0) { if (!in.playerBlocking) out.actions |= ACTION_START_BLOCK; state.blockTick = 1; }
            else { if (in.playerBlocking) { out.actions |= ACTION_SWAP_EMPTY | ACTION_CANCEL_ATTACK; out.allowAttack = false; } if (in.attackDelayMs <= 50) state.blockTick = 0; }
        }
        break;
    case BLOCK_SWAP:
        state.fakeBlocking = true; out.actions |= ACTION_BLINK_OFF;
        if (!in.hasSecondSword || !in.slotsSynced) { state = AutoBlockState(); break; }
        if (state.blockTick == 0) { if (!in.playerBlocking) out.actions |= ACTION_START_BLOCK; state.blockTick = 1; }
        else if (!in.playerBlocking) out.actions |= ACTION_START_BLOCK;
        else if (in.attackDelayMs <= 50) { out.actions |= ACTION_SWAP_SWORD | ACTION_START_BLOCK | ACTION_CANCEL_ATTACK; out.allowAttack = false; state.blockTick = 0; }
        break;
    case BLOCK_FAKE:
        state.isBlocking = false; state.fakeBlocking = true; out.actions |= ACTION_BLINK_OFF;
        if (in.usingItem && !in.playerBlocking && !in.diggingOrPlacing) out.actions |= ACTION_START_BLOCK;
        break;
    case BLOCK_MORDEN:
        state.fakeBlocking = true; out.actions |= ACTION_BLINK_ON;
        if (in.diggingOrPlacing) { out.allowAttack = false; out.actions |= ACTION_CANCEL_ATTACK; break; }
        if (state.mordenTick == 0 || state.mordenTick == 1) {
            if (in.playerBlocking) out.actions |= ACTION_STOP_BLOCK;
            out.actions |= ACTION_CANCEL_ATTACK; out.allowAttack = false; ++state.mordenTick;
        } else {
            if (!in.playerBlocking) out.actions |= ACTION_START_BLOCK;
            state.mordenTick = 0;
        }
        break;
    }
    return out;
}

} // namespace killaura
