#include <cmath>
#include <iostream>
#include <vector>

#include "../src/main/cpp/kill_aura_core.h"

static int failures = 0;
static void Check(bool value, const char* message) { if (!value) { std::cerr << "FAIL: " << message << std::endl; ++failures; } }
static void Near(double expected, double actual, double eps, const char* message) { Check(std::abs(expected - actual) <= eps, message); }

static void TestGeometry()
{
    killaura::Box box = { 2, 0, -0.5, 3, 2, 0.5 };
    killaura::Vec3 eyes = { 0, 1.6, 0 };
    Near(2.0, killaura::DistanceToBox(eyes, box), 0.0001, "distance to nearest box point");
    Near(180.0, killaura::AngleToEntity(eyes, 0, 2, 0), 0.001, "OpenMyau doubled FOV angle");
    Check(killaura::RayIntersectsBox(eyes, {-90, 0}, box, 4.0), "ray reaches target box");
    Check(!killaura::RayIntersectsBox(eyes, {90, 0}, box, 4.0), "opposite ray misses target box");
}

static void TestTargets()
{
    std::vector<killaura::TargetCandidate> values;
    values.push_back({"far", 5, 1, 1, 2, false, false, false});
    values.push_back({"swing", 3.4, 20, 5, 10, true, false, false});
    values.push_back({"attack", 2.8, 10, 4, 8, true, true, false});
    values.push_back({"priority", 2.9, 30, 6, 9, true, true, true});
    killaura::ApplyRangeAndPriorityTiers(values);
    Check(values.size() == 1 && values[0].stableId == "priority", "range and priority tiers match upstream");

    values.clear();
    values.push_back({"distance", 2, 30, 5, 50, true, true, false});
    values.push_back({"health", 3, 1, 4, 10, true, true, false});
    killaura::SortTargets(values, killaura::SORT_HEALTH);
    Check(values[0].stableId == "health", "health sort");
    killaura::SortTargets(values, killaura::SORT_DISTANCE);
    Check(values[0].stableId == "distance", "distance tie-break/sort");
}

static void TestRotations()
{
    killaura::Angles desired = { 14.0f, 5.0f }, previous = { 0, 0 };
    killaura::Angles fixed = killaura::GcdPatch(desired, previous, 0.5f);
    float gcd = killaura::MouseGcd(0.5f);
    Near(0.0, std::fmod(std::abs(fixed.yaw), gcd), 0.0002, "yaw sensitivity grid");

    killaura::Angles back = killaura::SmoothBack({90, 30}, {0, 0});
    Near(63.0, back.yaw, 0.001, "smooth-back 30 percent yaw");

    killaura::Angles raven = killaura::RavenRaw({0,0,0}, 1.62f, 0, 0, {3,0,0}, {2.5,0,0}, 1.62f, 2);
    Check(raven.yaw < -80 && raven.yaw > -100, "Raven predicted yaw");
}

static void TestGrok()
{
    killaura::Angles client = { 0.0f, 0.0f };
    killaura::Angles farDesired = { 90.0f, 40.0f };
    killaura::Angles clamped = killaura::ClampAnglesToCone(farDesired, client, 12.0f, 8.0f);
    Near(12.0, clamped.yaw, 0.001, "cone clamps yaw to skew");
    Near(8.0, clamped.pitch, 0.001, "cone clamps pitch to skew");
    Check(killaura::AnglesWithinCone(clamped, client, 12.0f, 8.0f), "clamped angles stay in cone");
    Check(!killaura::AnglesWithinCone(farDesired, client, 12.0f, 8.0f), "far angles outside cone");

    killaura::Angles wrapClient = { 170.0f, 0.0f };
    killaura::Angles wrapDesired = { -170.0f, 0.0f };
    killaura::Angles wrapClamped = killaura::ClampAnglesToCone(wrapDesired, wrapClient, 12.0f, 8.0f);
    Near(12.0, std::abs(killaura::Shortest(wrapClient.yaw, wrapClamped.yaw)), 0.01,
         "cone uses shortest yaw path");
    Check(killaura::AnglesWithinCone(wrapClamped, wrapClient, 12.0f, 8.0f),
          "wrapped clamp stays within skew");

    killaura::GrokState state;
    killaura::GrokParams params = {};
    params.maxSkewYaw = 12.0f;
    params.maxSkewPitch = 8.0f;
    params.deadZone = 0.5f;
    params.minTurnSpeed = 5.0f;
    params.maxTurnSpeed = 25.0f;
    params.acceleration = 2.5f;
    params.deceleration = 1.5f;
    params.noiseStrength = 0.0f;
    params.clientFovHalf = 180.0f;

    killaura::Angles target = { 10.0f, 2.0f };
    killaura::GrokResult first = killaura::GrokStep(client, target, client, state, params, 0.5f, 0.0f);
    Check(killaura::AnglesWithinCone(first.desired, client, 14.0f, 10.0f), "Grok step stays near client cone");
    Check(first.engage, "near target engages silent stamp");
    Check(std::abs(first.yawVel - state.yawVel) <= params.maxTurnSpeed * 0.35f + 0.01f, "jerk limited yaw");

    // Dead zone: from already on intent with zero vel should barely move.
    state.yawVel = 0.0f;
    state.pitchVel = 0.0f;
    state.orbitPhase = 0.0f;
    killaura::Angles onTarget = { 0.1f, 0.0f };
    killaura::GrokResult settled = killaura::GrokStep(onTarget, onTarget, client, state, params, 0.4f, 0.0f);
    Check(std::abs(settled.desired.yaw - onTarget.yaw) < 3.0f, "dead zone leaves residual without snap");

    // Target far outside FOV must not engage.
    params.clientFovHalf = 30.0f;
    killaura::Angles behind = { 120.0f, 0.0f };
    killaura::GrokResult noEngage = killaura::GrokStep(client, behind, client, state, params, 0.5f, 0.0f);
    Check(!noEngage.engage, "out-of-FOV target disengages");
    Check(killaura::AnglesWithinCone(noEngage.desired, client, 14.0f, 10.0f),
          "disengaged step still cone-clamped");
}

static void TestCps()
{
    Check(killaura::RecordedPatternSize() > 1000, "full recorded trace retained");
    Check(killaura::RecordedDelayMs(0) == 16 && killaura::RecordedDelayMs(1) == 22, "record trace begins exactly");
    Check(killaura::RecordedDelayMs(killaura::RecordedPatternSize()) == 16, "record trace wraps");
    Check(killaura::NextNormalDelayMs(14, 14, 0.5) == 71, "normal CPS Java long sampling behavior");
    Check(killaura::AttackDelayMs(true, 8.0f, true, 0, 14, 14, 0.5) == 125,
          "blocking uses auto-block CPS ahead of recorded CPS");
    Check(killaura::AttackDelayMs(false, 8.0f, true, 0, 14, 14, 0.5) == 16,
          "record mode uses current trace entry when not blocking");
}

static void TestAutoBlock()
{
    killaura::AutoBlockState state;
    killaura::AutoBlockInput in = { killaura::BLOCK_HYPIXEL, true, true, false, false, false, true, true, 0 };
    killaura::AutoBlockResult first = killaura::StepAutoBlock(state, in);
    Check((first.actions & killaura::ACTION_START_BLOCK) != 0 && state.blockTick == 1, "Hypixel tick zero starts block");
    in.playerBlocking = true;
    killaura::AutoBlockResult second = killaura::StepAutoBlock(state, in);
    Check(!second.allowAttack && (second.actions & killaura::ACTION_STOP_BLOCK) != 0, "Hypixel tick one releases and cancels");

    state = killaura::AutoBlockState(); in.mode = killaura::BLOCK_MORDEN; in.playerBlocking = true;
    Check(!killaura::StepAutoBlock(state, in).allowAttack && state.mordenTick == 1, "Morden first hold tick");
    Check(!killaura::StepAutoBlock(state, in).allowAttack && state.mordenTick == 2, "Morden second hold tick");
    in.playerBlocking = false;
    killaura::AutoBlockResult third = killaura::StepAutoBlock(state, in);
    Check(third.allowAttack && (third.actions & killaura::ACTION_START_BLOCK) != 0 && state.mordenTick == 0, "Morden attack/block tick");
}

int main()
{
    TestGeometry(); TestTargets(); TestRotations(); TestGrok(); TestCps(); TestAutoBlock();
    if (failures) return 1;
    std::cout << "kill_aura_core_tests: all passed" << std::endl;
    return 0;
}
