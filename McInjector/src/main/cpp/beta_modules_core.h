#pragma once
// Pure logic shared by bridge.cpp (1.8.9) and bridge_261.cpp (1.21 / 26.x) for the
// Beta modules: Jump Reset, Trajectories, Murder Mystery and Break Progress.
// No JNI here; the bridges feed plain values in. Covered by tests/beta_modules_tests.cpp.

#include <cmath>
#include <cstddef>
#include <string>
#include <vector>

namespace lc {
namespace beta {

// ── Jump Reset ──────────────────────────────────────────────────────────────
struct JumpResetInput {
    int hurtTime;       // current local-player hurtTime
    int prevHurtTime;   // hurtTime seen on the previous Jump Reset sample
    bool onGround;
    bool movingForward; // W held
    bool guiOpen;
    int chance;         // 0..100
    int roll;           // uniform 0..99
};

// True on the first sample of a new hit (hurtTime rose), when a jump would
// actually leave the ground and the chance roll passes.
inline bool ShouldJumpReset(const JumpResetInput& in) {
    if (in.guiOpen || !in.onGround || !in.movingForward) return false;
    if (in.hurtTime <= 0 || in.hurtTime <= in.prevHurtTime) return false;
    if (in.chance <= 0) return false;
    if (in.chance >= 100) return true;
    return in.roll < in.chance;
}

// Hold the jump key long enough for one client tick to read it (50 ms tick).
static const unsigned int kJumpResetHoldMs = 70;

// ── Trajectories ────────────────────────────────────────────────────────────
enum ProjectileKind {
    PROJECTILE_NONE = 0,
    PROJECTILE_BOW,
    PROJECTILE_THROWABLE,  // snowball, egg, ender pearl
    PROJECTILE_POTION,     // splash / lingering potion
    PROJECTILE_EXP_BOTTLE,
};

struct ProjectileParams {
    double speed;        // blocks per tick at launch
    double gravity;      // subtracted from motionY each tick
    double drag;         // velocity multiplier each tick (air)
    float pitchOffset;   // added to pitch for the launch direction (potions throw upward)
    bool valid;
};

// ItemBow / BowItem pull power from ticks the bow has been drawn.
inline double BowPower(int useTicks) {
    double f = (double)useTicks / 20.0;
    f = (f * f + f * 2.0) / 3.0;
    if (f > 1.0) f = 1.0;
    return f;
}

inline ProjectileParams ParamsFor(ProjectileKind kind, int bowUseTicks) {
    ProjectileParams p = { 0.0, 0.0, 0.99, 0.0f, false };
    switch (kind) {
    case PROJECTILE_BOW: {
        const double power = BowPower(bowUseTicks);
        if (power < 0.1) return p; // vanilla does not fire below 0.1
        p.speed = power * 3.0;
        p.gravity = 0.05;
        p.valid = true;
        return p;
    }
    case PROJECTILE_THROWABLE:
        p.speed = 1.5; p.gravity = 0.03; p.valid = true; return p;
    case PROJECTILE_POTION:
        p.speed = 0.5; p.gravity = 0.05; p.pitchOffset = -20.0f; p.valid = true; return p;
    case PROJECTILE_EXP_BOTTLE:
        p.speed = 0.7; p.gravity = 0.07; p.pitchOffset = -20.0f; p.valid = true; return p;
    default:
        return p;
    }
}

struct TrajPoint { double x, y, z; };

struct TrajectoryResult {
    std::vector<TrajPoint> points; // launch point first
    bool hitBlock;
    TrajPoint impact;
};

// Launch direction in Minecraft angles (yaw 0 = +Z, pitch + = down).
inline void LaunchDirection(float yawDeg, float pitchDeg, float pitchOffsetDeg,
                            double* dx, double* dy, double* dz) {
    const double r = 3.14159265358979323846 / 180.0;
    const double yaw = yawDeg * r;
    const double pitch = pitchDeg * r;
    double x = -std::sin(yaw) * std::cos(pitch);
    double y = -std::sin((pitchDeg + pitchOffsetDeg) * r);
    double z = std::cos(yaw) * std::cos(pitch);
    const double len = std::sqrt(x * x + y * y + z * z);
    if (len > 1e-9) { x /= len; y /= len; z /= len; }
    *dx = x; *dy = y; *dz = z;
}

// Steps the projectile like the vanilla entity tick (move, drag, gravity) and
// stops at the first solid block, below the world floor, or after maxSteps.
// isSolid(bx, by, bz) is only called when the block coordinate changes.
template <typename SolidFn>
inline TrajectoryResult SimulateTrajectory(double startX, double startY, double startZ,
                                           float yawDeg, float pitchDeg,
                                           const ProjectileParams& params,
                                           SolidFn isSolid, int maxSteps = 160,
                                           double minY = -64.0) {
    TrajectoryResult out;
    out.hitBlock = false;
    out.impact.x = startX; out.impact.y = startY; out.impact.z = startZ;
    if (!params.valid) return out;

    double dx = 0, dy = 0, dz = 0;
    LaunchDirection(yawDeg, pitchDeg, params.pitchOffset, &dx, &dy, &dz);
    double mx = dx * params.speed, my = dy * params.speed, mz = dz * params.speed;
    double x = startX, y = startY, z = startZ;
    out.points.push_back({ x, y, z });

    long long lastBx = (long long)std::floor(x), lastBy = (long long)std::floor(y), lastBz = (long long)std::floor(z);
    for (int step = 0; step < maxSteps; ++step) {
        // Sub-sample the segment so fast arrows do not tunnel through 1-block walls.
        const double segLen = std::sqrt(mx * mx + my * my + mz * mz);
        const int sub = segLen > 0.5 ? (int)std::ceil(segLen / 0.5) : 1;
        for (int s = 1; s <= sub; ++s) {
            const double t = (double)s / (double)sub;
            const double px = x + mx * t, py = y + my * t, pz = z + mz * t;
            const long long bx = (long long)std::floor(px);
            const long long by = (long long)std::floor(py);
            const long long bz = (long long)std::floor(pz);
            if (bx != lastBx || by != lastBy || bz != lastBz) {
                lastBx = bx; lastBy = by; lastBz = bz;
                if (isSolid((int)bx, (int)by, (int)bz)) {
                    out.hitBlock = true;
                    out.impact.x = px; out.impact.y = py; out.impact.z = pz;
                    out.points.push_back(out.impact);
                    return out;
                }
            }
        }
        x += mx; y += my; z += mz;
        mx *= params.drag; my *= params.drag; mz *= params.drag;
        my -= params.gravity;
        out.points.push_back({ x, y, z });
        if (y < minY) break;
    }
    out.impact = out.points.back();
    return out;
}

// ── Murder Mystery ──────────────────────────────────────────────────────────
// Reduces "item.swordIron", "tile.sponge", "item.minecraft.iron_sword" or
// "block.minecraft.sponge" to the trailing id ("swordIron", "iron_sword").
inline std::string NormalizeItemKey(const std::string& raw) {
    std::string s = raw;
    const char* prefixes[] = { "item.minecraft.", "block.minecraft.", "minecraft:", "item.", "tile.", nullptr };
    for (int i = 0; prefixes[i]; ++i) {
        const std::string p = prefixes[i];
        if (s.compare(0, p.size(), p) == 0) { s = s.substr(p.size()); break; }
    }
    // Legacy metadata variants ("fish.cod.raw", "dyePowder.black") keep the base name.
    const size_t dot = s.find('.');
    if (dot != std::string::npos) s = s.substr(0, dot);
    return s;
}

inline bool EndsWith(const std::string& s, const char* suffix) {
    const std::string suf = suffix;
    return s.size() >= suf.size() && s.compare(s.size() - suf.size(), suf.size(), suf) == 0;
}

// Default Hypixel knife skins (Raven bS list), in 1.8.9 unlocalized and modern id form.
inline bool IsMurderWeaponKey(const std::string& rawKey) {
    const std::string k = NormalizeItemKey(rawKey);
    if (k.empty()) return false;
    static const char* kIds[] = {
        // 1.8.9 unlocalized names
        "swordIron", "swordStone", "shovelIron", "stick", "hatchetWood", "swordWood",
        "shovelStone", "blazeRod", "shovelDiamond", "netherquartz", "pumpkinPie",
        "pickaxeGold", "apple", "nameTag", "carrotOnAStick", "bone", "carrots",
        "carrotGolden", "cookie", "hatchetDiamond", "prismarineShard", "beefCooked",
        "netherbrick", "chickenCooked", "swordGold", "swordDiamond", "hoeDiamond",
        "shears", "fish", "dyePowder", "boat", "speckledMelon", "book", "doublePlant",
        "sponge", "deadbush",
        // modern registry ids
        "iron_sword", "stone_sword", "iron_shovel", "wooden_axe", "wooden_sword",
        "stone_shovel", "blaze_rod", "diamond_shovel", "quartz", "pumpkin_pie",
        "golden_pickaxe", "name_tag", "carrot_on_a_stick", "carrot", "golden_carrot",
        "diamond_axe", "prismarine_shard", "cooked_beef", "nether_brick", "cooked_chicken",
        "golden_sword", "diamond_sword", "netherite_sword", "diamond_hoe", "cod", "salmon",
        "cooked_cod", "cooked_salmon", "tropical_fish", "pufferfish", "ink_sac",
        "lapis_lazuli", "cocoa_beans", "bone_meal", "glistering_melon_slice", "sunflower",
        "lilac", "rose_bush", "peony", "tall_grass", "large_fern", "dead_bush",
        nullptr
    };
    for (int i = 0; kIds[i]; ++i) {
        if (k == kIds[i]) return true;
    }
    return EndsWith(k, "_dye") || EndsWith(k, "_boat");
}

inline bool IsBowKey(const std::string& rawKey) {
    return NormalizeItemKey(rawKey) == "bow";
}

enum MurderRole { MM_ROLE_NONE = 0, MM_ROLE_BOW = 1, MM_ROLE_MURDERER = 2 };

// A player stays marked as murderer once seen with a knife (the knife is only
// held while attacking). A bow holder is re-evaluated each scan.
inline MurderRole ClassifyMurderRole(const std::string& heldKey, bool previouslyMurderer) {
    if (previouslyMurderer || IsMurderWeaponKey(heldKey)) return MM_ROLE_MURDERER;
    if (IsBowKey(heldKey)) return MM_ROLE_BOW;
    return MM_ROLE_NONE;
}

// ── Break Progress ──────────────────────────────────────────────────────────
// Destroy stage 0..9 (vanilla crack texture index) to a display percentage.
inline int BreakStageToPercent(int stage) {
    if (stage < 0) return 0;
    if (stage > 9) stage = 9;
    return (stage + 1) * 10;
}

inline int BreakDamageToPercent(float damage) {
    if (!(damage > 0.0f)) return 0;
    if (damage >= 1.0f) return 100;
    return (int)(damage * 100.0f + 0.5f);
}

} // namespace beta
} // namespace lc
