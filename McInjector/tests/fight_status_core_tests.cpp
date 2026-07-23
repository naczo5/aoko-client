#include <cmath>
#include <iostream>
#include <limits>

#include "../src/main/cpp/fight_status_core.h"

using namespace fightstatus;

static int g_failures = 0;

static void Check(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void Near(float expected, float actual, float epsilon, const char* message)
{
    if (std::abs(expected - actual) > epsilon) {
        std::cerr << "FAIL: " << message << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static CombatantTelemetry Fighter(float health, float absorption = 0.0f,
                                  const ArmorProfile& armor = ArmorProfile::None())
{
    return CombatantTelemetry(health, absorption, armor, 20.0f);
}

static void TestHealthArmorAndClamping()
{
    CombatantTelemetry unarmored = Fighter(16.0f, 4.0f);
    Near(20.0f, TotalHealth(unarmored), 0.001f,
         "health and absorption contribute to total");
    Near(20.0f, EffectiveSurvivability(unarmored), 0.001f,
         "unarmored effective survivability");

    CombatantTelemetry legacy = Fighter(20.0f, 0.0f,
        ArmorProfile::Legacy(20.0f));
    Near(100.0f, EffectiveSurvivability(legacy), 0.01f,
         "legacy armor applies eighty percent reduction");

    CombatantTelemetry modern = Fighter(20.0f, 0.0f,
        ArmorProfile::Modern(20.0f, 8.0f));
    Near(76.9231f, EffectiveSurvivability(modern, 6.0f), 0.02f,
         "modern armor uses damage and toughness profile");

    CombatantTelemetry protectedLegacy = Fighter(20.0f, 0.0f,
        ArmorProfile::Legacy(20.0f, 20.0f));
    Near(500.0f, EffectiveSurvivability(protectedLegacy), 0.1f,
         "protection is applied after armor");

    CombatantTelemetry clamped(50.0f, -5.0f,
        ArmorProfile::Legacy(500.0f, 500.0f), 20.0f);
    Near(20.0f, TotalHealth(clamped), 0.001f,
         "health and negative absorption are clamped");
    Check(ArmorDamageMultiplier(clamped.armor) >= 0.04f,
          "extreme armor remains finitely clamped");

    ArmorProfile missingArmor;
    CombatantTelemetry fallback = Fighter(12.0f, 2.0f, missingArmor);
    Near(14.0f, EffectiveSurvivability(fallback), 0.001f,
         "missing armor safely falls back to raw health");

    CombatantTelemetry missingHealth;
    Check(!HasUsableHealth(missingHealth), "default telemetry is missing");
    Near(0.0f, EffectiveSurvivability(missingHealth), 0.001f,
         "missing health has no survivability");

    const float nan = std::numeric_limits<float>::quiet_NaN();
    CombatantTelemetry nonFinite(nan, nan, ArmorProfile::Modern(nan, nan), nan);
    Check(!HasUsableHealth(nonFinite), "non-finite health is missing telemetry");
    Near(1.0f, ArmorDamageMultiplier(ArmorProfile::Modern(nan, nan)), 0.001f,
         "non-finite modern armor sanitizes safely");
}

static void TestDeadbandAndTargetHelpers()
{
    Check(CompareSurvivability(108.0f, 100.0f) == FIGHT_EVEN,
          "eight percent boundary is even");
    Check(CompareSurvivability(108.1f, 100.0f) == FIGHT_WINNING,
          "above eight percent is winning");
    Check(CompareSurvivability(100.0f, 108.1f) == FIGHT_LOSING,
          "below negative eight percent is losing");

    Check(IsRecent(6000, 1000), "target is recent at five seconds");
    Check(!IsRecent(6001, 1000), "target expires after five seconds");
    Check(!IsRecent(999, 1000), "backward timestamp is not recent");
    Check(IsWithinRange(3.0f, 3.0f), "range endpoint is inclusive");
    Check(!IsWithinRange(-1.0f, 3.0f), "negative distance is invalid");
    Check(!IsWithinRange(3.1f, 3.0f), "target outside range is rejected");
    Near(64.0f, ClampTrackingRange(1000.0f), 0.001f,
         "tracking range has a safe upper clamp");

    TargetMemory target;
    target.Observe(1000, 42, 3.5f);
    Check(target.IsRecentAt(6000), "remembered target remains valid for five seconds");
    Check(target.IsUsableAt(2000, 4.0f), "recent in-range target is usable");
    Check(!target.IsUsableAt(2000, 3.0f), "recent out-of-range target is unusable");
    target.Observe(3000, 99, -1.0f);
    Check(target.stableId == 42, "invalid target observation is ignored");
}

static void TestRollingCombatAndReadiness()
{
    FightStatusTracker tracker;
    FightStatusResult result = tracker.Observe(1000, Fighter(20), Fighter(20), 7, 3.0f);
    Check(!result.ready && result.state == FIGHT_EVEN,
          "new fight begins even and not ready");

    result = tracker.Observe(1500, Fighter(20), Fighter(18), 7, 3.0f);
    Check(result.targetDamageTakenEvents == 1,
          "target health decrease records one damage event");
    result = tracker.Observe(2100, Fighter(18), Fighter(18), 7, 3.0f);
    Check(result.selfDamageTakenEvents == 1,
          "self health decrease records one damage event");
    Check(!result.ready, "two events are insufficient before 1500 milliseconds");

    result = tracker.Observe(2500, Fighter(18), Fighter(14), 7, 3.0f);
    Check(result.ready, "fight is ready after 1500 milliseconds and two events");
    Check(result.state == FIGHT_WINNING && result.winner == WINNER_SELF,
          "higher survivability reports self as winner");
    Check(result.confidence >= 50 && result.confidence <= 95,
          "winner confidence is clamped from fifty to ninety-five");
    Check(result.selfHitFrequencyHz > result.targetHitFrequencyHz,
          "hit frequency assigns target damage to self attacks");
    Check(result.momentum > 0.0f,
          "dealing more damage produces positive momentum");

    result = tracker.Observe(5600, Fighter(18), Fighter(14), 7, 3.0f);
    Check(result.rollingSampleCount == 3,
          "samples older than the four-second window roll out");
    Check(result.selfDamageTakenEvents == 1 && result.targetDamageTakenEvents == 1,
          "damage events older than the four-second window roll out");

    result = tracker.GetStatus(10601);
    Check(!result.targetRecent && !result.ready,
          "readiness drops when target has not been seen for five seconds");
}

static void TestHealingMissingTelemetryAndReset()
{
    FightStatusTracker tracker;
    tracker.Observe(0, Fighter(20), Fighter(20), 11, 2.0f);
    tracker.Observe(500, Fighter(18), Fighter(17), 11, 2.0f);
    FightStatusResult result = tracker.Observe(1000, Fighter(20), Fighter(19), 11, 2.0f);
    Check(result.selfDamageTakenEvents == 1 && result.targetDamageTakenEvents == 1,
          "healing is not counted as damage");

    CombatantTelemetry missing;
    result = tracker.Observe(1200, Fighter(20), missing, 11, 2.0f);
    Check(!result.telemetryValid && !result.ready && result.state == FIGHT_EVEN,
          "missing telemetry suppresses status and readiness");
    const std::size_t eventsBeforeRecovery = tracker.DamageEventCount();
    tracker.Observe(1400, Fighter(20), Fighter(10), 11, 2.0f);
    Check(tracker.DamageEventCount() == eventsBeforeRecovery,
          "recovery after missing telemetry does not invent damage");
    tracker.Observe(1500, Fighter(20), Fighter(9), 11, 2.0f);
    Check(tracker.DamageEventCount() == eventsBeforeRecovery + 1,
          "damage detection resumes from recovered baseline");

    result = tracker.Observe(1600, Fighter(20), Fighter(20), 12, 2.0f);
    Check(tracker.DamageEventCount() == 0 && tracker.SampleCount() == 1,
          "target identity change resets rolling fight data");
    Check(!result.ready, "new target starts a fresh readiness period");

    tracker.Reset();
    Check(tracker.DamageEventCount() == 0 && tracker.SampleCount() == 0,
          "explicit reset clears all rolling state");
    Check(!tracker.Target().hasTarget, "explicit reset clears target memory");

    tracker.Observe(5000, Fighter(20), Fighter(20), 1, 1.0f);
    tracker.Observe(4000, Fighter(15), Fighter(15), 1, 1.0f);
    Check(tracker.DamageEventCount() == 0 && tracker.SampleCount() == 1,
          "backward time resets instead of creating stale damage");
}

static void TestCourseOverridesImmediateState()
{
    FightStatusTracker tracker;
    tracker.Observe(0, Fighter(12), Fighter(20), 21, 2.0f);
    tracker.Observe(500, Fighter(12), Fighter(18), 21, 2.0f);
    tracker.Observe(1000, Fighter(11), Fighter(17), 21, 2.0f);
    FightStatusResult result = tracker.Observe(
        1500, Fighter(10), Fighter(15), 21, 2.0f);

    Check(result.ready, "course override has enough temporal evidence");
    Check(result.state == FIGHT_LOSING,
          "immediate state remains based on effective survivability");
    Check(result.winner == WINNER_SELF && result.predictionScore > 0.0f,
          "better fight course can override an immediately losing state");
    Near(10.0f, result.selfHealthPool, 0.001f,
         "self raw health pool is retained for observed TTK");
    Near(15.0f, result.targetHealthPool, 0.001f,
         "target raw health pool is retained for observed TTK");
    Near(7.5f, result.selfTimeToDefeatSeconds, 0.001f,
         "self TTK uses raw pool divided by observed post-mitigation DPS");
    Near(4.5f, result.targetTimeToDefeatSeconds, 0.001f,
         "target TTK uses raw pool divided by observed post-mitigation DPS");
    Check(result.confidence >= 50 && result.confidence <= 95,
          "override confidence remains clamped");
}

static void TestEvenStatePredictsWinner()
{
    FightStatusTracker tracker;
    tracker.Observe(0, Fighter(17), Fighter(20), 22, 2.0f);
    tracker.Observe(500, Fighter(17), Fighter(18), 22, 2.0f);
    tracker.Observe(1000, Fighter(16), Fighter(17), 22, 2.0f);
    FightStatusResult result = tracker.Observe(
        1500, Fighter(15), Fighter(15), 22, 2.0f);

    Check(result.ready && result.state == FIGHT_EVEN,
          "equal current pools retain an even immediate state");
    Check(result.winner == WINNER_SELF && result.predictionScore > 0.0f,
          "temporal advantage predicts a winner from an even state");
}

static void TestNoHistoryStaysUnready()
{
    FightStatusTracker tracker;
    tracker.Observe(0, Fighter(20), Fighter(20), 23, 2.0f);
    FightStatusResult result = tracker.Observe(
        3000, Fighter(20), Fighter(20), 23, 2.0f);

    Check(result.state == FIGHT_EVEN, "no-history state remains even");
    Check(!result.ready && result.winner == WINNER_NONE,
          "elapsed time without damage history does not become ready");
    Check(result.selfDamageTakenEvents == 0
            && result.targetDamageTakenEvents == 0,
          "no-history status has no invented damage evidence");
}

int main()
{
    TestHealthArmorAndClamping();
    TestDeadbandAndTargetHelpers();
    TestRollingCombatAndReadiness();
    TestHealingMissingTelemetryAndReset();
    TestCourseOverridesImmediateState();
    TestEvenStatePredictsWinner();
    TestNoHistoryStaysUnready();

    if (g_failures != 0) {
        std::cerr << "fight_status_core_tests: " << g_failures
                  << " failure(s)" << std::endl;
        return 1;
    }
    std::cout << "fight_status_core_tests: all passed" << std::endl;
    return 0;
}
