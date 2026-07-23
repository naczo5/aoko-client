// Pure tests for the body projection selector shared by legacy and modern bridges.
// Built and run by run_tests.bat. Standalone C++11.

#include <cmath>
#include <iostream>

#include "../src/main/cpp/aim_assist_projection.h"

static int g_failures = 0;

static void ExpectTrue(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectNear(float expected, float actual, float epsilon, const char* message)
{
    if (std::abs(expected - actual) > epsilon) {
        std::cerr << "FAIL: " << message
                  << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void TestSelectsClosestBodySample()
{
    auto projector = [](double x, double y, double, float* sx, float* sy) {
        *sx = static_cast<float>(100.0 + x * 10.0);
        *sy = static_cast<float>(100.0 + y * 10.0);
        return true;
    };
    float x = 0.0f, y = 0.0f;
    bool found = lc::aimassist::SelectClosestBodyPoint(
        0.0, 0.0, 0.0, 200, 200, projector, &x, &y);

    ExpectTrue(found, "closest sample found");
    ExpectNear(100.0f, x, 0.001f, "center x sample selected");
    ExpectNear(101.5f, y, 0.001f, "lowest body y sample is closest");
}

static void TestPartialAndCompleteProjectionFailure()
{
    auto oneSample = [](double x, double y, double z, float* sx, float* sy) {
        if (std::abs(x - 0.30) > 0.001 || std::abs(y - 0.95) > 0.001 || std::abs(z) > 0.001)
            return false;
        *sx = 75.0f;
        *sy = 80.0f;
        return true;
    };
    float x = -1.0f, y = -1.0f;
    bool found = lc::aimassist::SelectClosestBodyPoint(
        0.0, 0.0, 0.0, 200, 200, oneSample, &x, &y);
    ExpectTrue(found, "single projectable sample found");
    ExpectNear(75.0f, x, 0.001f, "single sample x preserved");
    ExpectNear(80.0f, y, 0.001f, "single sample y preserved");

    auto none = [](double, double, double, float*, float*) { return false; };
    x = 12.0f;
    y = 34.0f;
    found = lc::aimassist::SelectClosestBodyPoint(
        0.0, 0.0, 0.0, 200, 200, none, &x, &y);
    ExpectTrue(!found, "all projection failures return false");
    ExpectNear(12.0f, x, 0.001f, "failure leaves output x unchanged");
    ExpectNear(34.0f, y, 0.001f, "failure leaves output y unchanged");
}

static void TestMovingBaseAndViewportScaling()
{
    auto projector = [](double x, double y, double, float* sx, float* sy) {
        *sx = static_cast<float>(x * 100.0);
        *sy = static_cast<float>(y * 100.0);
        return true;
    };
    float firstX = 0.0f, firstY = 0.0f;
    float movedX = 0.0f, movedY = 0.0f;
    bool first = lc::aimassist::SelectClosestBodyPoint(
        1.0, 0.0, 0.0, 200, 200, projector, &firstX, &firstY);
    bool moved = lc::aimassist::SelectClosestBodyPoint(
        1.5, 0.0, 0.0, 300, 200, projector, &movedX, &movedY);

    ExpectTrue(first && moved, "moving samples project");
    ExpectNear(100.0f, firstX, 0.001f, "first viewport center selected");
    ExpectNear(150.0f, movedX, 0.001f, "moved viewport center selected");
    ExpectNear(firstY, movedY, 0.001f, "same vertical sample remains selected");
}

static void TestOffscreenAndNonFiniteSamples()
{
    auto offscreen = [](double, double, double, float* sx, float* sy) {
        *sx = -50.0f;
        *sy = -25.0f;
        return true;
    };
    float x = 0.0f, y = 0.0f;
    bool found = lc::aimassist::SelectClosestBodyPoint(
        0.0, 0.0, 0.0, 200, 200, offscreen, &x, &y);
    ExpectTrue(found, "valid offscreen projection remains available for caller filtering");
    ExpectNear(-50.0f, x, 0.001f, "offscreen x preserved");

    auto nonFinite = [](double, double, double, float* sx, float* sy) {
        *sx = INFINITY;
        *sy = 0.0f;
        return true;
    };
    found = lc::aimassist::SelectClosestBodyPoint(
        0.0, 0.0, 0.0, 200, 200, nonFinite, &x, &y);
    ExpectTrue(!found, "non-finite projections are rejected");
}

static void TestTiesKeepFirstProjectedSample()
{
    int calls = 0;
    auto projector = [&calls](double, double, double, float* sx, float* sy) {
        ++calls;
        if (calls == 1) {
            *sx = 90.0f;
            *sy = 100.0f;
            return true;
        }
        if (calls == 2) {
            *sx = 110.0f;
            *sy = 100.0f;
            return true;
        }
        return false;
    };
    float x = 0.0f, y = 0.0f;
    bool found = lc::aimassist::SelectClosestBodyPoint(
        0.0, 0.0, 0.0, 200, 200, projector, &x, &y);

    ExpectTrue(found, "tie samples found");
    ExpectNear(90.0f, x, 0.001f, "first equal-score sample wins");
}

static void TestEquivalentBridgeProjectors()
{
    auto legacyRelative = [](double x, double y, double z, float* sx, float* sy) {
        *sx = static_cast<float>(400.0 + x * 25.0 + z * 5.0);
        *sy = static_cast<float>(300.0 - y * 20.0);
        return true;
    };
    const double cameraX = 10.0;
    const double cameraY = 4.0;
    const double cameraZ = -2.0;
    auto modernWorld = [=](double x, double y, double z, float* sx, float* sy) {
        return legacyRelative(x - cameraX, y - cameraY, z - cameraZ, sx, sy);
    };

    float legacyX = 0.0f, legacyY = 0.0f;
    float modernX = 0.0f, modernY = 0.0f;
    bool legacyFound = lc::aimassist::SelectClosestBodyPoint(
        2.0, 1.0, 6.0, 800, 600, legacyRelative, &legacyX, &legacyY);
    bool modernFound = lc::aimassist::SelectClosestBodyPoint(
        12.0, 5.0, 4.0, 800, 600, modernWorld, &modernX, &modernY);

    ExpectTrue(legacyFound && modernFound, "equivalent bridge projectors find samples");
    ExpectNear(legacyX, modernX, 0.001f, "bridge-equivalent x matches");
    ExpectNear(legacyY, modernY, 0.001f, "bridge-equivalent y matches");
}

int main()
{
    TestSelectsClosestBodySample();
    TestPartialAndCompleteProjectionFailure();
    TestMovingBaseAndViewportScaling();
    TestOffscreenAndNonFiniteSamples();
    TestTiesKeepFirstProjectedSample();
    TestEquivalentBridgeProjectors();

    if (g_failures != 0) {
        std::cerr << "aim_assist_projection_tests: " << g_failures << " failure(s)" << std::endl;
        return 1;
    }
    std::cout << "aim_assist_projection_tests: all passed" << std::endl;
    return 0;
}
