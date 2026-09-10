#pragma once

#include <atomic>
#include <cmath>
#include <cstdlib>

namespace reach {

struct Vec3 {
    double x;
    double y;
    double z;

    Vec3() : x(0.0), y(0.0), z(0.0) {}
    Vec3(double x_, double y_, double z_) : x(x_), y(y_), z(z_) {}

    Vec3 addVector(double x_, double y_, double z_) const {
        return Vec3(x + x_, y + y_, z + z_);
    }

    double distanceTo(const Vec3& o) const {
        double dx = o.x - x;
        double dy = o.y - y;
        double dz = o.z - z;
        return std::sqrt(dx * dx + dy * dy + dz * dz);
    }

    double squareDistanceTo(const Vec3& o) const {
        double dx = o.x - x;
        double dy = o.y - y;
        double dz = o.z - z;
        return dx * dx + dy * dy + dz * dz;
    }

    bool getIntermediateWithXValue(const Vec3& target, double targetX, Vec3& out) const {
        double dx = target.x - x;
        if (dx * dx < 1.0000000116860974E-7) return false;
        double factor = (targetX - x) / dx;
        if (factor < 0.0 || factor > 1.0) return false;
        out = Vec3(targetX, y + (target.y - y) * factor, z + (target.z - z) * factor);
        return true;
    }

    bool getIntermediateWithYValue(const Vec3& target, double targetY, Vec3& out) const {
        double dy = target.y - y;
        if (dy * dy < 1.0000000116860974E-7) return false;
        double factor = (targetY - y) / dy;
        if (factor < 0.0 || factor > 1.0) return false;
        out = Vec3(x + (target.x - x) * factor, targetY, z + (target.z - z) * factor);
        return true;
    }

    bool getIntermediateWithZValue(const Vec3& target, double targetZ, Vec3& out) const {
        double dz = target.z - z;
        if (dz * dz < 1.0000000116860974E-7) return false;
        double factor = (targetZ - z) / dz;
        if (factor < 0.0 || factor > 1.0) return false;
        out = Vec3(x + (target.x - x) * factor, y + (target.y - y) * factor, targetZ);
        return true;
    }
};

struct AxisAlignedBB {
    double minX, minY, minZ;
    double maxX, maxY, maxZ;

    AxisAlignedBB() : minX(0), minY(0), minZ(0), maxX(0), maxY(0), maxZ(0) {}
    AxisAlignedBB(double x1, double y1, double z1, double x2, double y2, double z2)
        : minX(x1), minY(y1), minZ(z1), maxX(x2), maxY(y2), maxZ(z2) {}

    AxisAlignedBB expand(double x, double y, double z) const {
        return AxisAlignedBB(minX - x, minY - y, minZ - z, maxX + x, maxY + y, maxZ + z);
    }

    AxisAlignedBB addCoord(double x, double y, double z) const {
        double d0 = minX;
        double d1 = minY;
        double d2 = minZ;
        double d3 = maxX;
        double d4 = maxY;
        double d5 = maxZ;
        if (x < 0.0) d0 += x;
        if (x > 0.0) d3 += x;
        if (y < 0.0) d1 += y;
        if (y > 0.0) d4 += y;
        if (z < 0.0) d2 += z;
        if (z > 0.0) d5 += z;
        return AxisAlignedBB(d0, d1, d2, d3, d4, d5);
    }

    bool isVecInside(const Vec3& vec) const {
        return vec.x > minX && vec.x < maxX &&
               vec.y > minY && vec.y < maxY &&
               vec.z > minZ && vec.z < maxZ;
    }

    bool isVecInYZ(const Vec3& vec) const {
        return vec.y >= minY && vec.y <= maxY && vec.z >= minZ && vec.z <= maxZ;
    }

    bool isVecInXZ(const Vec3& vec) const {
        return vec.x >= minX && vec.x <= maxX && vec.z >= minZ && vec.z <= maxZ;
    }

    bool isVecInXY(const Vec3& vec) const {
        return vec.x >= minX && vec.x <= maxX && vec.y >= minY && vec.y <= maxY;
    }

    bool calculateIntercept(const Vec3& vecA, const Vec3& vecB, Vec3& outIntercept) const {
        Vec3 vXmin, vXmax, vYmin, vYmax, vZmin, vZmax;
        bool hasXmin = vecA.getIntermediateWithXValue(vecB, minX, vXmin) && isVecInYZ(vXmin);
        bool hasXmax = vecA.getIntermediateWithXValue(vecB, maxX, vXmax) && isVecInYZ(vXmax);
        bool hasYmin = vecA.getIntermediateWithYValue(vecB, minY, vYmin) && isVecInXZ(vYmin);
        bool hasYmax = vecA.getIntermediateWithYValue(vecB, maxY, vYmax) && isVecInXZ(vYmax);
        bool hasZmin = vecA.getIntermediateWithZValue(vecB, minZ, vZmin) && isVecInXY(vZmin);
        bool hasZmax = vecA.getIntermediateWithZValue(vecB, maxZ, vZmax) && isVecInXY(vZmax);

        Vec3 closest;
        bool found = false;
        double minDistSq = 0.0;

        auto test = [&](bool has, const Vec3& v) {
            if (has) {
                double distSq = vecA.squareDistanceTo(v);
                if (!found || distSq < minDistSq) {
                    minDistSq = distSq;
                    closest = v;
                    found = true;
                }
            }
        };

        test(hasXmin, vXmin);
        test(hasXmax, vXmax);
        test(hasYmin, vYmin);
        test(hasYmax, vYmax);
        test(hasZmin, vZmin);
        test(hasZmax, vZmax);

        if (found) {
            outIntercept = closest;
            return true;
        }
        return false;
    }
};

struct ReachSettings {
    bool enabled;
    float minRange;
    float maxRange;
    int chance;
    int chanceMode; // 0 = Normal, 1 = Advanced
    bool onlyWhileSprinting;
    bool disableInWater;
    bool verticalCheck;

    ReachSettings()
        : enabled(false),
          minRange(3.0f),
          maxRange(3.0f),
          chance(100),
          chanceMode(0),
          onlyWhileSprinting(false),
          disableInWater(false),
          verticalCheck(false) {}
};

inline bool IsReachAllowed(const ReachSettings& settings, bool isSprinting, bool inWaterOrLava) {
    if (!settings.enabled) return false;
    if (settings.disableInWater && inWaterOrLava) return false;
    if (settings.onlyWhileSprinting && !isSprinting) return false;
    return true;
}

class ReachEngine {
private:
    std::atomic<int> hitCooldown;
    std::atomic<bool> reachActive;
    std::atomic<int> lastTargetId;

public:
    ReachEngine() : hitCooldown(0), reachActive(false), lastTargetId(0) {}

    ReachEngine(const ReachEngine& other)
        : hitCooldown(other.hitCooldown.load()),
          reachActive(other.reachActive.load()),
          lastTargetId(other.lastTargetId.load()) {}

    ReachEngine& operator=(const ReachEngine& other) {
        if (this != &other) {
            hitCooldown.store(other.hitCooldown.load());
            reachActive.store(other.reachActive.load());
            lastTargetId.store(other.lastTargetId.load());
        }
        return *this;
    }

    void Reset() {
        hitCooldown.store(0);
        reachActive.store(false);
        lastTargetId.store(0);
    }

    void OnAttack() {
        reachActive.store(false);
    }

    void OnTick(bool isAllowed, int chance, int currentTargetId) {
        int cd = hitCooldown.load();
        if (cd > 0) {
            hitCooldown.store(cd - 1);
        }
        int lastId = lastTargetId.load();
        if (currentTargetId == 0 || (lastId != 0 && currentTargetId != lastId)) {
            lastTargetId.store(0);
            return;
        }
        if (isAllowed && hitCooldown.load() == 0) {
            bool active = (rand() % 100) < chance;
            reachActive.store(active);
            if (active) {
                hitCooldown.store(10);
            }
        }
        lastTargetId.store(currentTargetId);
    }

    bool ShouldExtendReach(const ReachSettings& settings) {
        if (settings.chanceMode == 1) { // Advanced
            return reachActive.load();
        }
        // Normal
        return (rand() % 100) < settings.chance;
    }

    double GetReachDistance(const ReachSettings& settings, bool isAllowed) {
        if (!isAllowed || !ShouldExtendReach(settings)) {
            return 3.0;
        }
        if (settings.maxRange <= settings.minRange) {
            return (double)settings.minRange;
        }
        double r = (double)rand() / (double)RAND_MAX;
        return (double)settings.minRange + r * (double)(settings.maxRange - settings.minRange);
    }
};

} // namespace reach
