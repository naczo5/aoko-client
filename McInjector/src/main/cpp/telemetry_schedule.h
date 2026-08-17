#pragma once

namespace lc {

static const unsigned int kModernStateFastIntervalMs = 5;
static const unsigned int kModernStateNormalIntervalMs = 25;
static const unsigned int kChestEspScanIntervalMs = 100;
static const unsigned int kBlockEspScanIntervalMs = 150;
static const unsigned int kBedPlatesScanIntervalMs = 350;

// Independent wall-clock cadences. Enabling one job must not change another
// job's interval, and none of these are frame/present ticks.
static const unsigned int kModuleFastIntervalMs = 5;
static const unsigned int kWorkerIdleIntervalMs = 50;
static const unsigned int kFastPollIntervalMs = 5;
static const unsigned int kStateReadIntervalMs = 25;
static const unsigned int kEntityPublishIntervalMs = 5;
static const unsigned int kSpeedBridgeIntervalMs = 5;
static const unsigned int kKillAuraWorkerIntervalMs = 50;
static const unsigned int kAutoRodIntervalMs = 5;
static const unsigned int kAutoToolIntervalMs = 5;
static const unsigned int kHitDelayFixIntervalMs = 5;
static const unsigned int kReachIntervalMs = 5;
static const unsigned int kVelocityIntervalMs = 5;
static const unsigned int kAutoTotemIntervalMs = 5;
static const unsigned int kAntiDebuffIntervalMs = 5;
static const unsigned int kPixelPartyIntervalMs = 5;
static const unsigned int kPlayerOverlayIntervalMs = 50;
static const unsigned int kClosestPlayerIntervalMs = 50;
static const unsigned int kOverlayCameraIntervalMs = 25;

inline bool IsTelemetryIntervalDue(
    unsigned int nowMs,
    unsigned int lastRunMs,
    unsigned int intervalMs)
{
    return static_cast<unsigned int>(nowMs - lastRunMs) >= intervalMs;
}

inline unsigned int MsUntilDue(
    unsigned int nowMs,
    unsigned int lastRunMs,
    unsigned int intervalMs)
{
    if (intervalMs == 0) return 0;
    const unsigned int elapsed = static_cast<unsigned int>(nowMs - lastRunMs);
    if (elapsed >= intervalMs) return 0;
    return intervalMs - elapsed;
}

inline void ConsiderJobSleep(
    unsigned int* sleepMs,
    unsigned int nowMs,
    unsigned int lastRunMs,
    unsigned int intervalMs,
    bool enabled)
{
    if (!sleepMs || !enabled) return;
    const unsigned int waitMs = MsUntilDue(nowMs, lastRunMs, intervalMs);
    if (waitMs < *sleepMs) *sleepMs = waitMs;
}

inline unsigned int PlayerOverlayIntervalMs(bool latencySensitive)
{
    return latencySensitive ? kModuleFastIntervalMs : kPlayerOverlayIntervalMs;
}

inline unsigned int ModernStateIntervalMs(
    bool clicking,
    bool aimAssist,
    bool triggerbot,
    bool pixelPartyAutoLook,
    bool pixelPartyAutoWalk,
    bool hudEditor)
{
    const bool latencySensitive =
        clicking ||
        aimAssist ||
        triggerbot ||
        pixelPartyAutoLook ||
        pixelPartyAutoWalk ||
        hudEditor;
    return latencySensitive
        ? kModernStateFastIntervalMs
        : kModernStateNormalIntervalMs;
}

inline bool ModernStateNeedsFast(
    bool clicking,
    bool aimAssist,
    bool triggerbot,
    bool pixelPartyAutoLook,
    bool pixelPartyAutoWalk,
    bool hudEditor)
{
    return ModernStateIntervalMs(
        clicking,
        aimAssist,
        triggerbot,
        pixelPartyAutoLook,
        pixelPartyAutoWalk,
        hudEditor) == kModernStateFastIntervalMs;
}

inline bool ModernFullStateDue(
    unsigned int nowMs,
    unsigned int lastFullStateMs,
    bool fastState,
    unsigned int fullIntervalMs = kModernStateNormalIntervalMs)
{
    return !fastState || lastFullStateMs == 0 ||
        IsTelemetryIntervalDue(nowMs, lastFullStateMs, fullIntervalMs);
}

} // namespace lc
