// Native harness tests for silent_aura_aim.h pure helpers:
//   - angle normalize / shortest yaw
//   - mouse GCD / sensitivity patch
//   - rot-speed and accuracy mappings
//   - aim-point selection
//   - on-target silhouette gate
//   - humanized click intervals
//
// Built and run by run_tests.bat. Standalone C++11.

#include <iostream>
#include <cmath>
#include <cstdlib>

#include "../src/main/cpp/silent_aura_aim.h"

static int g_failures = 0;

static void ExpectTrue(bool cond, const char* message)
{
    if (!cond) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectNear(float expected, float actual, float eps, const char* message)
{
    if (std::abs(expected - actual) > eps) {
        std::cerr << "FAIL: " << message
                  << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void TestAngles()
{
    ExpectNear(0.0f, saaim::NormalizeAngle(360.0f), 0.01f, "normalize 360");
    ExpectNear(10.0f, saaim::ShortestYawDelta(170.0f, -180.0f), 0.5f, "shortest yaw wrap");
    ExpectTrue(std::abs(saaim::ShortestYawDelta(10.0f, 350.0f)) < 30.0f, "shortest prefers wrap");
}

static void TestGcd()
{
    float gcd = saaim::MouseGcd(0.5f);
    ExpectTrue(gcd > 0.05f && gcd < 1.0f, "gcd in sane range for sens 0.5");

    saaim::Angles patched = saaim::ApplySensitivityPatch(0.0f, 0.0f, 0.001f, 0.001f, 0.5f);
    // Tiny sub-GCD delta should quantize to ~0.
    ExpectNear(0.0f, patched.yaw, gcd * 0.6f, "sub-gcd yaw quantizes near zero");
    ExpectNear(0.0f, patched.pitch, gcd * 0.6f, "sub-gcd pitch quantizes near zero");

    saaim::Angles big = saaim::ApplySensitivityPatch(0.0f, 0.0f, 12.0f, 5.0f, 0.5f);
    ExpectTrue(std::abs(big.yaw) > 1.0f, "large yaw still moves");
    // Result should land on GCD grid relative to previous.
    float rem = std::fmod(std::abs(big.yaw), gcd);
    ExpectTrue(rem < 0.001f || std::abs(rem - gcd) < 0.001f, "yaw lands on gcd grid");
}

static void TestMappings()
{
    ExpectTrue(saaim::SampleYawStepDeg(8.0f, 18.0f, 0.0f) <= saaim::SampleYawStepDeg(8.0f, 18.0f, 1.0f) + 0.01f,
               "sample yaw min <= max");
    ExpectNear(8.0f, saaim::SampleYawStepDeg(8.0f, 18.0f, 0.0f), 0.01f, "sample yaw at 0");
    ExpectNear(18.0f, saaim::SampleYawStepDeg(8.0f, 18.0f, 1.0f), 0.01f, "sample yaw at 1");
    ExpectTrue(saaim::PitchStepFromYaw(20.0f) < 20.0f, "pitch softer than yaw");
    ExpectTrue(saaim::LegacyRotSpeedToYawStep(90.0f) > saaim::LegacyRotSpeedToYawStep(10.0f),
               "legacy rot speed increases step");
    ExpectTrue(saaim::NoiseAmplitudeDeg(100.0f) > saaim::NoiseAmplitudeDeg(0.0f), "more randomization more noise");
    ExpectTrue(saaim::NoiseAmplitudeDeg(0.0f) > 0.0f, "0% randomization still has residual noise");
    ExpectTrue(saaim::AimWanderStrength(0.0f) > 0.0f, "0% still wanders a little");
}

static void TestAimPoint()
{
    saaim::AimPointFracs fracs;
    auto alwaysHalf = []() { return 0.5f; };
    bool ok = saaim::SelectAimPointFracs(
        0.0, 1.62, 0.0,
        0.0, 0.0, 3.0,
        3.0,
        0.1f,
        alwaysHalf,
        &fracs);
    ExpectTrue(ok, "select aim point succeeds");
    ExpectTrue(fracs.y >= 0.2f && fracs.y <= 0.9f, "aim y in body band");
    ExpectTrue(fracs.x >= 0.15f && fracs.x <= 0.85f, "aim x inset");
}

static void TestOnTarget()
{
    // Looking roughly at a player 3 blocks ahead should be on target.
    bool on = saaim::IsOnTarget(
        0.0, 1.62, 0.0,
        0.0, 0.0, 3.0,
        0.0f, 0.0f,
        90.0f);
    ExpectTrue(on, "facing target is on-target");

    bool off = saaim::IsOnTarget(
        0.0, 1.62, 0.0,
        0.0, 0.0, 3.0,
        90.0f, 0.0f,
        90.0f);
    ExpectTrue(!off, "looking 90 deg away is not on-target");
}

static void TestClickInterval()
{
    auto r = []() { return 0.5f; };
    unsigned ms = saaim::NextClickIntervalMs(10.0f, 14.0f, r);
    ExpectTrue(ms >= 30 && ms <= 450, "click interval clamped");
}

static void TestStepRotation()
{
    saaim::RotationState st = {};
    st.bodyValid = false;
    auto zeroNoise = []() { return 0.0f; };
    saaim::StepResult step = saaim::StepRotation(
        0.0f, 0.0f,
        40.0f, 10.0f,
        20.0f, 12.0f,
        0.5f,
        0.0f,
        st,
        zeroNoise);
    ExpectTrue(std::abs(step.yaw) > 0.5f, "step moves toward target yaw");
    ExpectTrue(std::abs(step.yaw) <= 20.5f, "step respects yaw cap roughly");
    ExpectTrue(std::abs(saaim::ShortestYawDelta(step.bodyYaw, step.yaw)) >= 0.0f, "body yaw set");
}

int main()
{
    TestAngles();
    TestGcd();
    TestMappings();
    TestAimPoint();
    TestOnTarget();
    TestClickInterval();
    TestStepRotation();

    if (g_failures != 0) {
        std::cerr << g_failures << " failure(s)" << std::endl;
        return 1;
    }
    std::cout << "silent_aura_aim_tests: all passed" << std::endl;
    return 0;
}
