#pragma once

// Pure Fireball Warning helpers. Header-only, no JNI/ImGui.
// Detects whether a player's held-item display string corresponds to a
// fireball / fire charge (BedWars fireball, vanilla fire charge).

#include <cctype>
#include <string>

namespace lc {

inline std::string FireballWarningLower(const std::string& value) {    std::string out;
    out.reserve(value.size());
    for (size_t i = 0; i < value.size(); ++i)
        out.push_back((char)std::tolower((unsigned char)value[i]));
    return out;
}

// Matches display names ("Fire Charge", "Fireball"), registry ids
// ("minecraft:fire_charge", "fireball"), and unlocalized keys
// ("item.fireCharge", "tile.fireball") while avoiding false positives
// like "Firework Rocket" or "Fire Aspect".
inline bool IsFireballHolderText(const std::string& heldItemText) {
    std::string lower = FireballWarningLower(heldItemText);
    if (lower.empty()) return false;
    if (lower.find("fire_charge") != std::string::npos) return true;
    if (lower.find("fire charge") != std::string::npos) return true;
    if (lower.find("firecharge") != std::string::npos) return true;
    if (lower.find("fireball") != std::string::npos) return true;
    if (lower.find("fire ball") != std::string::npos) return true;
    return false;
}

// Minimum time between warning sounds. The background scan calls this with
// the monotonic tick count; unsigned arithmetic stays correct across wrap.
static const unsigned int kFireballWarningSoundCooldownMs = 2000;

inline bool FireballWarningSoundDue(unsigned int nowMs, unsigned int lastPlayMs,
                                    bool holderPresent, bool soundEnabled,
                                    unsigned int cooldownMs = kFireballWarningSoundCooldownMs) {
    if (!soundEnabled || !holderPresent) return false;
    return (nowMs - lastPlayMs) >= cooldownMs;
}

} // namespace lc
