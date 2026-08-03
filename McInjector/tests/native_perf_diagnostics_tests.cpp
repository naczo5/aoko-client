#include <iostream>

#include "../src/main/cpp/native_perf_diagnostics.h"

static int g_failures = 0;

static void ExpectTrue(bool value, const char* message)
{
    if (!value) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectEq(
    unsigned long long expected,
    unsigned long long actual,
    const char* message)
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
    lc::NativePerfDiagnostics disabled(false, 100, 0);
    disabled.Record(lc::PERF_SCAN_LOOP, 10, 5);
    lc::NativePerfSummary disabledSummary;
    ExpectTrue(
        !disabled.TryTakeSummary(1000, disabledSummary),
        "disabled diagnostics never emit");

    disabled.SetEnabled(true, 500);
    disabled.Record(lc::PERF_SCAN_LOOP, 25, 2);
    ExpectTrue(
        !disabled.TryTakeSummary(599, disabledSummary),
        "dynamic enable resets summary window");
    ExpectTrue(
        disabled.TryTakeSummary(600, disabledSummary),
        "dynamic enable emits after interval");
    ExpectEq(1, disabledSummary.metrics[lc::PERF_SCAN_LOOP].count, "dynamic enable count");
    ExpectEq(25, disabledSummary.metrics[lc::PERF_SCAN_LOOP].totalTicks, "dynamic enable ticks");
    ExpectEq(2, disabledSummary.metrics[lc::PERF_SCAN_LOOP].units, "dynamic enable units");
    disabled.SetEnabled(false, 700);
    disabled.Record(lc::PERF_SCAN_LOOP, 100, 10);
    ExpectTrue(
        !disabled.TryTakeSummary(1000, disabledSummary),
        "dynamic disable stops summaries");

    lc::NativePerfDiagnostics diagnostics(true, 100, 1000);
    diagnostics.Record(lc::PERF_STATE_PUBLISH, 10, 100);
    diagnostics.Record(lc::PERF_STATE_PUBLISH, 25, 250);
    diagnostics.Record(lc::PERF_OVERLAY_RENDER, 8);

    lc::NativePerfSummary early;
    ExpectTrue(
        !diagnostics.TryTakeSummary(1099, early),
        "summary waits for interval");

    lc::NativePerfSummary summary;
    ExpectTrue(
        diagnostics.TryTakeSummary(1100, summary),
        "summary emits at interval");
    ExpectEq(100, summary.windowMs, "summary window");
    ExpectEq(2, summary.metrics[lc::PERF_STATE_PUBLISH].count, "state count");
    ExpectEq(35, summary.metrics[lc::PERF_STATE_PUBLISH].totalTicks, "state total");
    ExpectEq(25, summary.metrics[lc::PERF_STATE_PUBLISH].maxTicks, "state max");
    ExpectEq(350, summary.metrics[lc::PERF_STATE_PUBLISH].units, "state bytes");
    ExpectEq(1, summary.metrics[lc::PERF_OVERLAY_RENDER].count, "render count");

    lc::NativePerfSummary reset;
    ExpectTrue(
        diagnostics.TryTakeSummary(1200, reset),
        "next interval emits");
    ExpectEq(0, reset.metrics[lc::PERF_STATE_PUBLISH].count, "summary resets count");

    lc::NativePerfDiagnostics wrapped(true, 40, 0xfffffff0u);
    wrapped.Record(lc::PERF_SCAN_LOOP, 1);
    lc::NativePerfSummary wrappedSummary;
    ExpectTrue(
        wrapped.TryTakeSummary(25, wrappedSummary),
        "summary interval survives tick-count wrap");

    if (g_failures != 0) {
        std::cerr << "native_perf_diagnostics_tests: "
                  << g_failures << " failure(s)" << std::endl;
        return 1;
    }

    std::cout << "native_perf_diagnostics_tests: all passed" << std::endl;
    return 0;
}
