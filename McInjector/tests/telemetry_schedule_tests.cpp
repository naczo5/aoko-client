#include <iostream>

#include "../src/main/cpp/telemetry_schedule.h"

static int g_failures = 0;

static void ExpectEq(unsigned int expected, unsigned int actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message
                  << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

int main()
{
    ExpectEq(
        lc::kModernStateNormalIntervalMs,
        lc::ModernStateIntervalMs(false, false, false, false, false, false),
        "idle telemetry uses normal interval");
    ExpectEq(
        lc::kModernStateFastIntervalMs,
        lc::ModernStateIntervalMs(true, false, false, false, false, false),
        "active clicking uses fast interval");
    ExpectEq(
        lc::kModernStateFastIntervalMs,
        lc::ModernStateIntervalMs(false, true, false, false, false, false),
        "aim assist uses fast interval");
    ExpectEq(
        lc::kModernStateFastIntervalMs,
        lc::ModernStateIntervalMs(false, false, true, false, false, false),
        "triggerbot uses fast interval");
    ExpectEq(
        lc::kModernStateFastIntervalMs,
        lc::ModernStateIntervalMs(false, false, false, true, false, false),
        "pixel party look uses fast interval");
    ExpectEq(
        lc::kModernStateFastIntervalMs,
        lc::ModernStateIntervalMs(false, false, false, false, true, false),
        "pixel party walk uses fast interval");
    ExpectEq(
        lc::kModernStateFastIntervalMs,
        lc::ModernStateIntervalMs(false, false, false, false, false, true),
        "HUD editor uses fast interval");
    ExpectEq(
        0,
        lc::IsTelemetryIntervalDue(1099, 1000, 100) ? 1u : 0u,
        "deadline remains pending before interval");
    ExpectEq(
        1,
        lc::IsTelemetryIntervalDue(1100, 1000, 100) ? 1u : 0u,
        "deadline fires at interval");
    ExpectEq(
        1,
        lc::IsTelemetryIntervalDue(25, 0xfffffff0u, 40) ? 1u : 0u,
        "deadline comparison survives tick-count wrap");

    if (g_failures != 0) {
        std::cerr << "telemetry_schedule_tests: "
                  << g_failures << " failure(s)" << std::endl;
        return 1;
    }

    std::cout << "telemetry_schedule_tests: all passed" << std::endl;
    return 0;
}
