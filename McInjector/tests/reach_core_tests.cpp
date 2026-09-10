#include <iostream>
#include <cmath>

#include "../src/main/cpp/reach_core.h"

static int g_failures = 0;

static void ExpectTrue(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectEq(int expected, int actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void ExpectNear(double expected, double actual, double eps, const char* message)
{
    if (std::abs(expected - actual) > eps) {
        std::cerr << "FAIL: " << message << " expected=" << expected
                  << " actual=" << actual << " diff=" << std::abs(expected - actual) << std::endl;
        ++g_failures;
    }
}

int main()
{
    // 1. Vec3 Tests
    reach::Vec3 v1(0.0, 0.0, 0.0);
    reach::Vec3 v2(3.0, 4.0, 0.0);
    ExpectNear(5.0, v1.distanceTo(v2), 1e-6, "Vec3 distanceTo calculates 3-4-5 triangle");
    ExpectNear(25.0, v1.squareDistanceTo(v2), 1e-6, "Vec3 squareDistanceTo calculates 25");

    reach::Vec3 vAdded = v1.addVector(1.0, 2.0, 3.0);
    ExpectNear(1.0, vAdded.x, 1e-6, "Vec3 addVector x");
    ExpectNear(2.0, vAdded.y, 1e-6, "Vec3 addVector y");
    ExpectNear(3.0, vAdded.z, 1e-6, "Vec3 addVector z");

    reach::Vec3 vInter;
    bool hasInter = v1.getIntermediateWithXValue(v2, 1.5, vInter);
    ExpectTrue(hasInter, "Vec3 getIntermediateWithXValue succeeds for midpoint");
    ExpectNear(1.5, vInter.x, 1e-6, "Intermediate x");
    ExpectNear(2.0, vInter.y, 1e-6, "Intermediate y");
    ExpectNear(0.0, vInter.z, 1e-6, "Intermediate z");

    // 2. AxisAlignedBB Tests
    reach::AxisAlignedBB aabb(-1.0, -1.0, -1.0, 1.0, 1.0, 1.0);
    ExpectTrue(aabb.isVecInside(reach::Vec3(0.0, 0.0, 0.0)), "Origin is inside [-1, 1] AABB");
    ExpectTrue(!aabb.isVecInside(reach::Vec3(2.0, 0.0, 0.0)), "(2,0,0) is outside [-1, 1] AABB");

    reach::AxisAlignedBB expanded = aabb.expand(0.5, 0.5, 0.5);
    ExpectNear(-1.5, expanded.minX, 1e-6, "Expanded minX");
    ExpectNear(1.5, expanded.maxX, 1e-6, "Expanded maxX");

    reach::AxisAlignedBB coordAdded = aabb.addCoord(2.0, -1.0, 0.0);
    ExpectNear(-1.0, coordAdded.minX, 1e-6, "addCoord minX");
    ExpectNear(3.0, coordAdded.maxX, 1e-6, "addCoord maxX");
    ExpectNear(-2.0, coordAdded.minY, 1e-6, "addCoord minY");
    ExpectNear(1.0, coordAdded.maxY, 1e-6, "addCoord maxY");

    // 3. Intercept Tests (Slab Ray-AABB)
    // Ray along X-axis hitting front face of a box at x=3
    reach::AxisAlignedBB targetBox(3.0, -1.0, -1.0, 5.0, 1.0, 1.0);
    reach::Vec3 rayOrigin(0.0, 0.0, 0.0);
    reach::Vec3 rayEnd(6.0, 0.0, 0.0);
    reach::Vec3 hit;
    bool hitOk = targetBox.calculateIntercept(rayOrigin, rayEnd, hit);
    ExpectTrue(hitOk, "Ray along X hits target box");
    ExpectNear(3.0, hit.x, 1e-5, "Hit position X matches minX");
    ExpectNear(0.0, hit.y, 1e-5, "Hit position Y is centered");
    ExpectNear(0.0, hit.z, 1e-5, "Hit position Z is centered");

    // Ray pointing away from box
    reach::Vec3 rayBack(-6.0, 0.0, 0.0);
    hitOk = targetBox.calculateIntercept(rayOrigin, rayBack, hit);
    ExpectTrue(!hitOk, "Ray pointing away does not hit target box");

    // Ray that falls short (e.g. range 2.5 vs box at x=3.0)
    reach::Vec3 rayShort(2.5, 0.0, 0.0);
    hitOk = targetBox.calculateIntercept(rayOrigin, rayShort, hit);
    ExpectTrue(!hitOk, "Ray falling short does not hit target box");

    // Ray starting inside box
    reach::Vec3 insideOrigin(4.0, 0.0, 0.0);
    ExpectTrue(targetBox.isVecInside(insideOrigin), "insideOrigin is inside targetBox");

    // 4. Reach Allowed Gate Tests
    reach::ReachSettings cfg;
    cfg.enabled = true;
    cfg.onlyWhileSprinting = true;
    cfg.disableInWater = true;

    ExpectTrue(!reach::IsReachAllowed(cfg, false, false), "Disallowed when not sprinting and onlyWhileSprinting=true");
    ExpectTrue(reach::IsReachAllowed(cfg, true, false), "Allowed when sprinting and not in water");
    ExpectTrue(!reach::IsReachAllowed(cfg, true, true), "Disallowed when in water and disableInWater=true");

    cfg.enabled = false;
    ExpectTrue(!reach::IsReachAllowed(cfg, true, false), "Disallowed when module disabled");

    // 5. Reach Engine Tests
    reach::ReachEngine engine;

    // Normal Mode
    reach::ReachSettings normalCfg;
    normalCfg.enabled = true;
    normalCfg.minRange = 3.5f;
    normalCfg.maxRange = 4.5f;
    normalCfg.chance = 100;
    normalCfg.chanceMode = 0; // Normal

    double rDist = engine.GetReachDistance(normalCfg, true);
    ExpectTrue(rDist >= 3.5 && rDist <= 4.5, "Reach distance in [3.5, 4.5] for 100% chance");

    normalCfg.chance = 0;
    ExpectNear(3.0, engine.GetReachDistance(normalCfg, true), 1e-6, "Reach distance is vanilla 3.0 when chance=0");

    // Advanced Mode
    reach::ReachSettings advCfg;
    advCfg.enabled = true;
    advCfg.minRange = 3.8f;
    advCfg.maxRange = 3.8f;
    advCfg.chance = 100;
    advCfg.chanceMode = 1; // Advanced

    engine.Reset();
    // Before tick tracking, reach is not active
    ExpectNear(3.0, engine.GetReachDistance(advCfg, true), 1e-6, "Advanced mode: inactive before tick tracking");

    // Tick 1: target acquired (id 42)
    engine.OnTick(true, 100, 42);
    ExpectNear(3.8, engine.GetReachDistance(advCfg, true), 1e-6, "Advanced mode: reach active after target lock");

    // OnAttack: resets reachActive
    engine.OnAttack();
    ExpectNear(3.0, engine.GetReachDistance(advCfg, true), 1e-6, "Advanced mode: attack resets reachActive");

    // Target change: id changes to 99 -> clears last target
    engine.OnTick(true, 100, 99);
    ExpectNear(3.0, engine.GetReachDistance(advCfg, true), 1e-6, "Advanced mode: target change resets lock");

    // 6. Vertical Check Math
    double playerY = 64.0;
    double targetCloseY = 64.15;
    double targetFarY = 64.25;
    ExpectTrue(std::abs(targetCloseY - playerY) <= 0.2, "Vertical delta 0.15 passes <= 0.2 check");
    ExpectTrue(std::abs(targetFarY - playerY) > 0.2, "Vertical delta 0.25 fails <= 0.2 check");

    if (g_failures == 0) {
        std::cout << "All reach core tests passed!" << std::endl;
        return 0;
    } else {
        std::cerr << g_failures << " tests failed!" << std::endl;
        return 1;
    }
}
