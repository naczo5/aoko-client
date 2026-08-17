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
    ExpectEq(
        1,
        lc::ModernStateNeedsFast(true, false, false, false, false, false) ? 1u : 0u,
        "clicking needs fast state");
    ExpectEq(
        0,
        lc::ModernStateNeedsFast(false, false, false, false, false, false) ? 1u : 0u,
        "idle state does not need fast state");
    ExpectEq(
        1,
        lc::ModernFullStateDue(100, 0, true) ? 1u : 0u,
        "first fast-state iteration emits a full state");
    ExpectEq(
        0,
        lc::ModernFullStateDue(110, 100, true) ? 1u : 0u,
        "fast state does not emit full state before cadence");
    ExpectEq(
        1,
        lc::ModernFullStateDue(125, 100, true) ? 1u : 0u,
        "fast state emits full state at cadence");
    ExpectEq(
        1,
        lc::ModernFullStateDue(110, 100, false) ? 1u : 0u,
        "normal state is always full");
    ExpectEq(
        0,
        lc::MsUntilDue(1100, 1000, 100),
        "elapsed interval is due immediately");
    ExpectEq(
        25,
        lc::MsUntilDue(1000, 1000, 25),
        "fresh timestamp waits the full interval");
    ExpectEq(
        1,
        lc::MsUntilDue(23, 0xfffffff0u, 40),
        "due wait survives tick-count wrap");

    unsigned int sleepMs = lc::kWorkerIdleIntervalMs;
    lc::ConsiderJobSleep(&sleepMs, 1002, 1000, lc::kSpeedBridgeIntervalMs, true);
    lc::ConsiderJobSleep(&sleepMs, 1002, 1000, lc::kChestEspScanIntervalMs, true);
    ExpectEq(
        3,
        sleepMs,
        "shared worker sleeps until the soonest independent job");

    ExpectEq(
        lc::kPlayerOverlayIntervalMs,
        lc::PlayerOverlayIntervalMs(false),
        "nametag overlay scan stays on the slow overlay interval");
    ExpectEq(
        lc::kModuleFastIntervalMs,
        lc::PlayerOverlayIntervalMs(true),
        "aim-assist overlay scan uses the fast overlay interval");

    unsigned int withSpeedBridge = lc::kWorkerIdleIntervalMs;
    unsigned int withoutSpeedBridge = lc::kWorkerIdleIntervalMs;
    lc::ConsiderJobSleep(
        &withoutSpeedBridge, 1000, 1000, lc::kPlayerOverlayIntervalMs, true);
    lc::ConsiderJobSleep(
        &withSpeedBridge, 1000, 1000, lc::kPlayerOverlayIntervalMs, true);
    lc::ConsiderJobSleep(
        &withSpeedBridge, 1000, 1000, lc::kSpeedBridgeIntervalMs, true);
    ExpectEq(
        lc::kPlayerOverlayIntervalMs,
        withoutSpeedBridge,
        "overlay-only worker waits the overlay interval");
    ExpectEq(
        lc::kSpeedBridgeIntervalMs,
        withSpeedBridge,
        "speedbridge shortens only its own wait, not the overlay interval constant");

    if (g_failures != 0) {
        std::cerr << "telemetry_schedule_tests: "
                  << g_failures << " failure(s)" << std::endl;
        return 1;
    }

    std::cout << "telemetry_schedule_tests: all passed" << std::endl;
    return 0;
}
