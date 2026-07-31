#pragma once

namespace lc {

static const unsigned int kModernStateFastIntervalMs = 5;
static const unsigned int kModernStateNormalIntervalMs = 25;
static const unsigned int kChestEspScanIntervalMs = 100;
static const unsigned int kBlockEspScanIntervalMs = 150;

inline bool IsTelemetryIntervalDue(
    unsigned int nowMs,
    unsigned int lastRunMs,
    unsigned int intervalMs)
{
    return static_cast<unsigned int>(nowMs - lastRunMs) >= intervalMs;
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
