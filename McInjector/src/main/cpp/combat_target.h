#pragma once
// One shared answer to "is this entity a valid combat target?" for every module
// that picks players (KillAura in both bridges, Murder Mystery markers). Bridges
// gather the facts with JNI and call Evaluate, so a rule change lands everywhere.

#include <cmath>
#include <string>

namespace lc {
namespace target {

struct Facts {
    bool isSelf = false;
    bool isPlayer = true;
    bool teammate = false;     // EntityPlayer.isOnSameTeam / Player.isAlliedTo
    float health = 20.0f;
    bool healthKnown = true;
    std::string stableName;    // GameProfile name, formatting stripped
    bool botLike = false;      // bridge LooksLikeFakePlayerLine(stableName)
};

struct Policy {
    bool requirePlayer = true;
    bool skipTeammates = true;
    bool skipBots = true;
    bool requireName = true;
};

enum Verdict {
    TARGET_OK = 0,
    TARGET_SELF,
    TARGET_NOT_PLAYER,
    TARGET_DEAD,
    TARGET_TEAMMATE,
    TARGET_NO_NAME,
    TARGET_BOT,
};

inline Verdict Evaluate(const Facts& f, const Policy& p) {
    if (f.isSelf) return TARGET_SELF;
    if (p.requirePlayer && !f.isPlayer) return TARGET_NOT_PLAYER;
    if (f.healthKnown && (!std::isfinite(f.health) || f.health <= 0.1f)) return TARGET_DEAD;
    if (p.skipTeammates && f.teammate) return TARGET_TEAMMATE;
    if (p.requireName && f.stableName.empty()) return TARGET_NO_NAME;
    if (p.skipBots && f.botLike) return TARGET_BOT;
    return TARGET_OK;
}

inline bool IsValid(const Facts& f, const Policy& p) { return Evaluate(f, p) == TARGET_OK; }

} // namespace target
} // namespace lc
