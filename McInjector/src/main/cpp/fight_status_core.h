#pragma once

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace fightstatus {

static const std::uint64_t kDefaultWindowMs = 4000;
static const std::uint64_t kReadinessDelayMs = 1500;
static const std::uint64_t kTargetRecentMs = 5000;
static const float kStateDeadband = 0.08f;

enum ArmorModel {
    ARMOR_NONE = 0,
    ARMOR_LEGACY = 1,
    ARMOR_MODERN = 2
};

enum FightState {
    FIGHT_LOSING = -1,
    FIGHT_EVEN = 0,
    FIGHT_WINNING = 1
};

enum FightWinner {
    WINNER_TARGET = -1,
    WINNER_NONE = 0,
    WINNER_SELF = 1
};

enum DamageRecipient {
    DAMAGE_TO_SELF = 0,
    DAMAGE_TO_TARGET = 1
};

inline float Clamp(float value, float low, float high)
{
    return std::max(low, std::min(high, value));
}

inline bool IsFinite(float value)
{
    return std::isfinite(static_cast<double>(value)) != 0;
}

struct ArmorProfile {
    ArmorModel model;
    float armorPoints;
    float toughness;
    float protection;
    bool available;

    ArmorProfile()
        : model(ARMOR_NONE), armorPoints(0.0f), toughness(0.0f),
          protection(0.0f), available(false) {}

    static ArmorProfile None()
    {
        ArmorProfile result;
        result.available = true;
        return result;
    }

    static ArmorProfile Legacy(float armorPointsValue, float protectionValue = 0.0f)
    {
        ArmorProfile result;
        result.model = ARMOR_LEGACY;
        result.armorPoints = armorPointsValue;
        result.protection = protectionValue;
        result.available = true;
        return result;
    }

    static ArmorProfile Modern(float armorPointsValue, float toughnessValue,
                               float protectionValue = 0.0f)
    {
        ArmorProfile result;
        result.model = ARMOR_MODERN;
        result.armorPoints = armorPointsValue;
        result.toughness = toughnessValue;
        result.protection = protectionValue;
        result.available = true;
        return result;
    }
};

struct CombatantTelemetry {
    float health;
    float absorption;
    float maxHealth;
    ArmorProfile armor;
    bool healthAvailable;

    CombatantTelemetry()
        : health(0.0f), absorption(0.0f), maxHealth(20.0f),
          armor(), healthAvailable(false) {}

    CombatantTelemetry(float healthValue, float absorptionValue,
                       const ArmorProfile& armorValue,
                       float maxHealthValue = 20.0f)
        : health(healthValue), absorption(absorptionValue), maxHealth(maxHealthValue),
          armor(armorValue), healthAvailable(true) {}
};

inline float SanitizedMaxHealth(const CombatantTelemetry& telemetry)
{
    if (!IsFinite(telemetry.maxHealth) || telemetry.maxHealth <= 0.0f) return 20.0f;
    return Clamp(telemetry.maxHealth, 1.0f, 2048.0f);
}

inline bool HasUsableHealth(const CombatantTelemetry& telemetry)
{
    return telemetry.healthAvailable && IsFinite(telemetry.health);
}

inline float TotalHealth(const CombatantTelemetry& telemetry)
{
    if (!HasUsableHealth(telemetry)) return 0.0f;
    const float health = Clamp(telemetry.health, 0.0f, SanitizedMaxHealth(telemetry));
    const float absorption = IsFinite(telemetry.absorption)
        ? Clamp(telemetry.absorption, 0.0f, 2048.0f) : 0.0f;
    return health + absorption;
}

inline float ArmorDamageMultiplier(const ArmorProfile& profile,
                                   float representativeHitDamage = 6.0f)
{
    if (!profile.available || profile.model == ARMOR_NONE) return 1.0f;

    const float armor = IsFinite(profile.armorPoints)
        ? Clamp(profile.armorPoints, 0.0f, 30.0f) : 0.0f;
    float armorReduction = 0.0f;
    if (profile.model == ARMOR_LEGACY) {
        armorReduction = Clamp(armor * 0.04f, 0.0f, 0.80f);
    } else if (profile.model == ARMOR_MODERN) {
        const float toughness = IsFinite(profile.toughness)
            ? Clamp(profile.toughness, 0.0f, 20.0f) : 0.0f;
        const float hitDamage = IsFinite(representativeHitDamage)
            ? Clamp(representativeHitDamage, 0.1f, 100.0f) : 6.0f;
        const float mitigatedArmor = Clamp(
            armor - hitDamage / (2.0f + toughness / 4.0f),
            armor * 0.20f,
            20.0f);
        armorReduction = mitigatedArmor / 25.0f;
    }

    const float protection = IsFinite(profile.protection)
        ? Clamp(profile.protection, 0.0f, 20.0f) : 0.0f;
    const float protectionReduction = Clamp(protection * 0.04f, 0.0f, 0.80f);
    return Clamp((1.0f - armorReduction) * (1.0f - protectionReduction), 0.04f, 1.0f);
}

// Effective incoming raw damage required to consume current health and absorption.
// If armor telemetry is unavailable, raw health is deliberately used as a safe fallback.
inline float EffectiveSurvivability(const CombatantTelemetry& telemetry,
                                    float representativeHitDamage = 6.0f)
{
    if (!HasUsableHealth(telemetry)) return 0.0f;
    return TotalHealth(telemetry)
        / ArmorDamageMultiplier(telemetry.armor, representativeHitDamage);
}

inline FightState CompareSurvivability(float selfValue, float targetValue,
                                       float deadband = kStateDeadband)
{
    const float selfSafe = IsFinite(selfValue) ? std::max(0.0f, selfValue) : 0.0f;
    const float targetSafe = IsFinite(targetValue) ? std::max(0.0f, targetValue) : 0.0f;
    const float band = Clamp(IsFinite(deadband) ? deadband : kStateDeadband, 0.0f, 0.50f);
    if (selfSafe > targetSafe * (1.0f + band)) return FIGHT_WINNING;
    if (targetSafe > selfSafe * (1.0f + band)) return FIGHT_LOSING;
    return FIGHT_EVEN;
}

inline bool IsRecent(std::uint64_t nowMs, std::uint64_t lastSeenMs,
                     std::uint64_t maxAgeMs = kTargetRecentMs)
{
    return nowMs >= lastSeenMs && nowMs - lastSeenMs <= maxAgeMs;
}

inline float ClampTrackingRange(float range)
{
    return IsFinite(range) ? Clamp(range, 0.0f, 64.0f) : 0.0f;
}

inline bool IsWithinRange(float distance, float maximumRange)
{
    return IsFinite(distance) && distance >= 0.0f
        && distance <= ClampTrackingRange(maximumRange);
}

inline bool IsTargetUsable(bool targetPresent, std::uint64_t nowMs,
                           std::uint64_t lastSeenMs, float distance,
                           float maximumRange,
                           std::uint64_t maxAgeMs = kTargetRecentMs)
{
    return targetPresent && IsRecent(nowMs, lastSeenMs, maxAgeMs)
        && IsWithinRange(distance, maximumRange);
}

struct TargetMemory {
    std::uint64_t stableId;
    std::uint64_t lastSeenMs;
    float lastDistance;
    bool hasTarget;

    TargetMemory()
        : stableId(0), lastSeenMs(0), lastDistance(0.0f), hasTarget(false) {}

    void Reset()
    {
        stableId = 0;
        lastSeenMs = 0;
        lastDistance = 0.0f;
        hasTarget = false;
    }

    void Observe(std::uint64_t nowMs, std::uint64_t id, float distance,
                 bool valid = true)
    {
        if (!valid || id == 0 || !IsFinite(distance) || distance < 0.0f) return;
        stableId = id;
        lastSeenMs = nowMs;
        lastDistance = distance;
        hasTarget = true;
    }

    bool IsRecentAt(std::uint64_t nowMs,
                    std::uint64_t maxAgeMs = kTargetRecentMs) const
    {
        return hasTarget && IsRecent(nowMs, lastSeenMs, maxAgeMs);
    }

    bool IsUsableAt(std::uint64_t nowMs, float maximumRange,
                    std::uint64_t maxAgeMs = kTargetRecentMs) const
    {
        return IsTargetUsable(hasTarget, nowMs, lastSeenMs, lastDistance,
                              maximumRange, maxAgeMs);
    }
};

struct FightSample {
    std::uint64_t timestampMs;
    float selfSurvivability;
    float targetSurvivability;
};

struct DamageEvent {
    std::uint64_t timestampMs;
    DamageRecipient recipient;
    float amount;
};

inline float NormalizedLead(float selfValue, float targetValue)
{
    const float selfSafe = IsFinite(selfValue) ? std::max(0.0f, selfValue) : 0.0f;
    const float targetSafe = IsFinite(targetValue) ? std::max(0.0f, targetValue) : 0.0f;
    const float largest = std::max(selfSafe, targetSafe);
    return largest > 0.001f ? (selfSafe - targetSafe) / largest : 0.0f;
}

inline float ObservedTimeToDefeat(float healthPool, float damageTakenPerSecond)
{
    if (!IsFinite(healthPool) || !IsFinite(damageTakenPerSecond)
        || healthPool < 0.0f || damageTakenPerSecond <= 0.001f) {
        return 0.0f;
    }
    return Clamp(healthPool / damageTakenPerSecond, 0.0f, 3600.0f);
}

struct FightStatusResult {
    FightState state;
    FightWinner winner;
    int confidence;
    bool ready;
    bool telemetryValid;
    bool targetRecent;
    float selfHealthPool;
    float targetHealthPool;
    float selfSurvivability;
    float targetSurvivability;
    unsigned int selfDamageTakenEvents;
    unsigned int targetDamageTakenEvents;
    float selfHitFrequencyHz;
    float targetHitFrequencyHz;
    float selfDamageTakenPerSecond;
    float targetDamageTakenPerSecond;
    float selfTimeToDefeatSeconds;
    float targetTimeToDefeatSeconds;
    float momentum;
    float predictionScore;
    std::size_t rollingSampleCount;

    FightStatusResult()
        : state(FIGHT_EVEN), winner(WINNER_NONE), confidence(50), ready(false),
          telemetryValid(false), targetRecent(false), selfHealthPool(0.0f),
          targetHealthPool(0.0f), selfSurvivability(0.0f),
          targetSurvivability(0.0f), selfDamageTakenEvents(0),
          targetDamageTakenEvents(0), selfHitFrequencyHz(0.0f),
          targetHitFrequencyHz(0.0f), selfDamageTakenPerSecond(0.0f),
          targetDamageTakenPerSecond(0.0f), selfTimeToDefeatSeconds(0.0f),
          targetTimeToDefeatSeconds(0.0f), momentum(0.0f),
          predictionScore(0.0f), rollingSampleCount(0) {}
};

class FightStatusTracker {
public:
    explicit FightStatusTracker(std::uint64_t windowMs = kDefaultWindowMs,
                                float representativeHitDamage = 6.0f)
        : windowMs_(std::max<std::uint64_t>(1000, windowMs)),
          representativeHitDamage_(IsFinite(representativeHitDamage)
              ? Clamp(representativeHitDamage, 0.1f, 100.0f) : 6.0f)
    {
        Reset();
    }

    void Reset()
    {
        samples_.clear();
        damageEvents_.clear();
        targetMemory_.Reset();
        fightStartMs_ = 0;
        lastTimestampMs_ = 0;
        activeTargetId_ = 0;
        previousSelfHealth_ = 0.0f;
        previousTargetHealth_ = 0.0f;
        currentSelfHealthPool_ = 0.0f;
        currentTargetHealthPool_ = 0.0f;
        currentSelfSurvivability_ = 0.0f;
        currentTargetSurvivability_ = 0.0f;
        hasFightStart_ = false;
        hasTimestamp_ = false;
        hasActiveTarget_ = false;
        hasPreviousSelf_ = false;
        hasPreviousTarget_ = false;
        currentTelemetryValid_ = false;
        currentSelfArmorAvailable_ = false;
        currentTargetArmorAvailable_ = false;
    }

    // Clears rolling combat evidence while retaining the remembered target.
    void ResetFight()
    {
        const TargetMemory rememberedTarget = targetMemory_;
        const std::uint64_t rememberedTargetId = activeTargetId_;
        const bool hadTarget = hasActiveTarget_;
        Reset();
        targetMemory_ = rememberedTarget;
        activeTargetId_ = rememberedTargetId;
        hasActiveTarget_ = hadTarget;
    }

    FightStatusResult Observe(std::uint64_t nowMs,
                              const CombatantTelemetry& self,
                              const CombatantTelemetry& target,
                              std::uint64_t stableTargetId = 0,
                              float targetDistance = 0.0f)
    {
        if (hasTimestamp_ && nowMs < lastTimestampMs_) Reset();

        if (stableTargetId != 0 && hasActiveTarget_
            && stableTargetId != activeTargetId_) {
            Reset();
        }
        if (stableTargetId != 0) {
            activeTargetId_ = stableTargetId;
            hasActiveTarget_ = true;
        }

        lastTimestampMs_ = nowMs;
        hasTimestamp_ = true;

        const bool selfValid = HasUsableHealth(self);
        const bool targetValid = HasUsableHealth(target);
        currentTelemetryValid_ = selfValid && targetValid;
        currentSelfHealthPool_ = selfValid ? TotalHealth(self) : 0.0f;
        currentTargetHealthPool_ = targetValid ? TotalHealth(target) : 0.0f;
        currentSelfArmorAvailable_ = selfValid && self.armor.available;
        currentTargetArmorAvailable_ = targetValid && target.armor.available;

        if (targetValid) {
            // ID zero is valid for callers that do not expose entity IDs; recency still works.
            targetMemory_.hasTarget = true;
            targetMemory_.stableId = stableTargetId;
            targetMemory_.lastSeenMs = nowMs;
            targetMemory_.lastDistance = IsFinite(targetDistance)
                ? std::max(0.0f, targetDistance) : 0.0f;
        }

        ObserveDamage(nowMs, selfValid, currentSelfHealthPool_, DAMAGE_TO_SELF,
                      hasPreviousSelf_, previousSelfHealth_);
        ObserveDamage(nowMs, targetValid, currentTargetHealthPool_, DAMAGE_TO_TARGET,
                      hasPreviousTarget_, previousTargetHealth_);

        if (currentTelemetryValid_) {
            currentSelfSurvivability_ = EffectiveSurvivability(
                self, representativeHitDamage_);
            currentTargetSurvivability_ = EffectiveSurvivability(
                target, representativeHitDamage_);
            if (!hasFightStart_) {
                fightStartMs_ = nowMs;
                hasFightStart_ = true;
            }
            FightSample sample = {
                nowMs, currentSelfSurvivability_, currentTargetSurvivability_
            };
            if (!samples_.empty() && samples_.back().timestampMs == nowMs) {
                samples_.back() = sample;
            } else {
                samples_.push_back(sample);
            }
        }

        Prune(nowMs);
        return GetStatus(nowMs);
    }

    FightStatusResult GetStatus(std::uint64_t nowMs) const
    {
        FightStatusResult result;
        result.telemetryValid = currentTelemetryValid_;
        result.targetRecent = targetMemory_.IsRecentAt(nowMs);
        result.selfHealthPool = currentSelfHealthPool_;
        result.targetHealthPool = currentTargetHealthPool_;
        result.selfSurvivability = currentSelfSurvivability_;
        result.targetSurvivability = currentTargetSurvivability_;
        result.state = currentTelemetryValid_
            ? CompareSurvivability(currentSelfSurvivability_,
                                   currentTargetSurvivability_)
            : FIGHT_EVEN;

        const std::uint64_t cutoff = nowMs > windowMs_ ? nowMs - windowMs_ : 0;
        std::size_t sampleCount = 0;
        for (std::size_t i = 0; i < samples_.size(); ++i) {
            if (samples_[i].timestampMs >= cutoff && samples_[i].timestampMs <= nowMs)
                ++sampleCount;
        }
        result.rollingSampleCount = sampleCount;

        float selfDamage = 0.0f;
        float targetDamage = 0.0f;
        for (std::size_t i = 0; i < damageEvents_.size(); ++i) {
            const DamageEvent& event = damageEvents_[i];
            if (event.timestampMs < cutoff || event.timestampMs > nowMs) continue;
            if (event.recipient == DAMAGE_TO_SELF) {
                ++result.selfDamageTakenEvents;
                selfDamage += event.amount;
            } else {
                ++result.targetDamageTakenEvents;
                targetDamage += event.amount;
            }
        }

        std::uint64_t measuredMs = windowMs_;
        if (hasFightStart_ && nowMs >= fightStartMs_)
            measuredMs = std::min(windowMs_, nowMs - fightStartMs_);
        const float measuredSeconds = std::max(0.001f,
            static_cast<float>(measuredMs) / 1000.0f);

        // Health deltas are observed after the game has applied mitigation. Armor
        // must not be applied again to these DPS or time-to-defeat values.
        result.selfHitFrequencyHz = result.targetDamageTakenEvents / measuredSeconds;
        result.targetHitFrequencyHz = result.selfDamageTakenEvents / measuredSeconds;
        result.selfDamageTakenPerSecond = selfDamage / measuredSeconds;
        result.targetDamageTakenPerSecond = targetDamage / measuredSeconds;
        result.selfTimeToDefeatSeconds = ObservedTimeToDefeat(
            result.selfHealthPool, result.selfDamageTakenPerSecond);
        result.targetTimeToDefeatSeconds = ObservedTimeToDefeat(
            result.targetHealthPool, result.targetDamageTakenPerSecond);
        result.momentum = result.targetDamageTakenPerSecond
            - result.selfDamageTakenPerSecond;

        const unsigned int eventCount = result.selfDamageTakenEvents
            + result.targetDamageTakenEvents;
        const bool oldEnough = hasFightStart_ && nowMs >= fightStartMs_
            && nowMs - fightStartMs_ >= kReadinessDelayMs;
        result.ready = currentTelemetryValid_ && result.targetRecent
            && oldEnough && eventCount >= 2;

        if (result.ready) {
            const float survivabilityLead = NormalizedLead(
                result.selfSurvivability, result.targetSurvivability);
            const bool hasTtkLead = result.selfTimeToDefeatSeconds > 0.0f
                && result.targetTimeToDefeatSeconds > 0.0f;
            const float ttkLead = hasTtkLead ? NormalizedLead(
                result.selfTimeToDefeatSeconds, result.targetTimeToDefeatSeconds) : 0.0f;
            const float totalHitFrequency = result.selfHitFrequencyHz
                + result.targetHitFrequencyHz;
            const bool hasHitFrequency = totalHitFrequency > 0.001f;
            const float hitFrequencyLead = hasHitFrequency
                ? (result.selfHitFrequencyHz - result.targetHitFrequencyHz)
                    / totalHitFrequency
                : 0.0f;
            const float totalDps = result.selfDamageTakenPerSecond
                + result.targetDamageTakenPerSecond;
            const bool hasMomentum = totalDps > 0.001f;
            const float momentumLead = hasMomentum ? result.momentum / totalDps : 0.0f;

            const float survivabilityWeight = 0.25f;
            const float ttkWeight = 0.35f;
            const float hitFrequencyWeight = 0.15f;
            const float momentumWeight = 0.25f;
            float weightedScore = survivabilityLead * survivabilityWeight;
            float usedWeight = survivabilityWeight;
            if (hasTtkLead) {
                weightedScore += ttkLead * ttkWeight;
                usedWeight += ttkWeight;
            }
            if (hasHitFrequency) {
                weightedScore += hitFrequencyLead * hitFrequencyWeight;
                usedWeight += hitFrequencyWeight;
            }
            if (hasMomentum) {
                weightedScore += momentumLead * momentumWeight;
                usedWeight += momentumWeight;
            }
            result.predictionScore = weightedScore / usedWeight;

            if (result.predictionScore > 0.0001f) {
                result.winner = WINNER_SELF;
            } else if (result.predictionScore < -0.0001f) {
                result.winner = WINNER_TARGET;
            } else if (result.state != FIGHT_EVEN) {
                result.winner = result.state == FIGHT_WINNING
                    ? WINNER_SELF : WINNER_TARGET;
            } else {
                // Exact ties are uncommon once damage exists; use event balance as
                // a deterministic final course-of-fight tiebreaker.
                result.winner = result.targetDamageTakenEvents
                    >= result.selfDamageTakenEvents ? WINNER_SELF : WINNER_TARGET;
            }

            const float winnerDirection = result.winner == WINNER_SELF ? 1.0f : -1.0f;
            float disagreementWeight = 0.0f;
            if (survivabilityLead * winnerDirection < -0.0001f)
                disagreementWeight += survivabilityWeight;
            if (hasTtkLead && ttkLead * winnerDirection < -0.0001f)
                disagreementWeight += ttkWeight;
            if (hasHitFrequency && hitFrequencyLead * winnerDirection < -0.0001f)
                disagreementWeight += hitFrequencyWeight;
            if (hasMomentum && momentumLead * winnerDirection < -0.0001f)
                disagreementWeight += momentumWeight;

            const float evidenceStrength = std::min(1.0f, eventCount / 6.0f);
            const int missingArmorSignals = (currentSelfArmorAvailable_ ? 0 : 1)
                + (currentTargetArmorAvailable_ ? 0 : 1);
            float confidence = 55.0f + std::abs(result.predictionScore) * 28.0f
                + evidenceStrength * 8.0f
                - (disagreementWeight / usedWeight) * 12.0f
                - missingArmorSignals * 3.0f;
            const bool immediateDisagrees =
                (result.state == FIGHT_WINNING && result.winner == WINNER_TARGET)
                || (result.state == FIGHT_LOSING && result.winner == WINNER_SELF);
            if (immediateDisagrees) confidence -= 4.0f;
            result.confidence = static_cast<int>(
                Clamp(confidence, 50.0f, 95.0f) + 0.5f);
        }
        return result;
    }

    const TargetMemory& Target() const { return targetMemory_; }
    std::size_t SampleCount() const { return samples_.size(); }
    std::size_t DamageEventCount() const { return damageEvents_.size(); }
    std::uint64_t WindowMs() const { return windowMs_; }

private:
    static void ObserveDamage(std::uint64_t nowMs, bool valid, float currentHealth,
                              DamageRecipient recipient, bool& hasPrevious,
                              float& previousHealth,
                              std::vector<DamageEvent>& events)
    {
        if (!valid) {
            hasPrevious = false;
            return;
        }
        if (hasPrevious && previousHealth - currentHealth > 0.01f) {
            DamageEvent event = { nowMs, recipient, previousHealth - currentHealth };
            events.push_back(event);
        }
        previousHealth = currentHealth;
        hasPrevious = true;
    }

    void ObserveDamage(std::uint64_t nowMs, bool valid, float currentHealth,
                       DamageRecipient recipient, bool& hasPrevious,
                       float& previousHealth)
    {
        ObserveDamage(nowMs, valid, currentHealth, recipient, hasPrevious,
                      previousHealth, damageEvents_);
    }

    void Prune(std::uint64_t nowMs)
    {
        const std::uint64_t cutoff = nowMs > windowMs_ ? nowMs - windowMs_ : 0;
        while (!samples_.empty() && samples_.front().timestampMs < cutoff)
            samples_.erase(samples_.begin());
        while (!damageEvents_.empty() && damageEvents_.front().timestampMs < cutoff)
            damageEvents_.erase(damageEvents_.begin());
    }

    std::uint64_t windowMs_;
    float representativeHitDamage_;
    std::vector<FightSample> samples_;
    std::vector<DamageEvent> damageEvents_;
    TargetMemory targetMemory_;
    std::uint64_t fightStartMs_;
    std::uint64_t lastTimestampMs_;
    std::uint64_t activeTargetId_;
    float previousSelfHealth_;
    float previousTargetHealth_;
    float currentSelfHealthPool_;
    float currentTargetHealthPool_;
    float currentSelfSurvivability_;
    float currentTargetSurvivability_;
    bool hasFightStart_;
    bool hasTimestamp_;
    bool hasActiveTarget_;
    bool hasPreviousSelf_;
    bool hasPreviousTarget_;
    bool currentTelemetryValid_;
    bool currentSelfArmorAvailable_;
    bool currentTargetArmorAvailable_;
};

} // namespace fightstatus
