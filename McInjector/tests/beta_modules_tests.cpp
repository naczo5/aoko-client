// Native harness tests for beta_modules_core.h, module_status.h and combat_target.h.

#include <iostream>
#include <string>

#include "../src/main/cpp/beta_modules_core.h"
#include "../src/main/cpp/combat_target.h"
#include "../src/main/cpp/module_status.h"

using namespace lc;

static int g_failures = 0;

static void ExpectTrue(bool cond, const char* message) {
    if (!cond) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void TestJumpReset() {
    beta::JumpResetInput in = { 10, 0, true, true, false, 100, 50 };
    ExpectTrue(beta::ShouldJumpReset(in), "fresh hit on ground jumps");
    in.prevHurtTime = 10;
    ExpectTrue(!beta::ShouldJumpReset(in), "same hit does not jump twice");
    in.prevHurtTime = 0; in.onGround = false;
    ExpectTrue(!beta::ShouldJumpReset(in), "airborne does not jump");
    in.onGround = true; in.movingForward = false;
    ExpectTrue(!beta::ShouldJumpReset(in), "requires W");
    in.movingForward = true; in.guiOpen = true;
    ExpectTrue(!beta::ShouldJumpReset(in), "gui open blocks");
    in.guiOpen = false; in.chance = 40; in.roll = 39;
    ExpectTrue(beta::ShouldJumpReset(in), "roll under chance jumps");
    in.roll = 40;
    ExpectTrue(!beta::ShouldJumpReset(in), "roll at chance does not jump");
    in.chance = 0; in.roll = 0;
    ExpectTrue(!beta::ShouldJumpReset(in), "zero chance never jumps");
}

static void TestBowPower() {
    ExpectTrue(beta::BowPower(0) == 0.0, "no draw no power");
    ExpectTrue(beta::BowPower(20) == 1.0, "full draw at 20 ticks");
    ExpectTrue(beta::BowPower(40) == 1.0, "clamped at 1");
    ExpectTrue(!beta::ParamsFor(beta::PROJECTILE_BOW, 1).valid, "under 0.1 power is not fired");
    beta::ProjectileParams p = beta::ParamsFor(beta::PROJECTILE_BOW, 20);
    ExpectTrue(p.valid && p.speed == 3.0 && p.gravity == 0.05, "full bow params");
    ExpectTrue(beta::ParamsFor(beta::PROJECTILE_POTION, 0).pitchOffset == -20.0f, "potion pitch offset");
}

static void TestTrajectoryFlatGround() {
    beta::ProjectileParams p = beta::ParamsFor(beta::PROJECTILE_THROWABLE, 0);
    // Floor at y < 64 is solid.
    beta::TrajectoryResult r = beta::SimulateTrajectory(
        0.0, 65.62, 0.0, 0.0f, 0.0f, p, [](int, int y, int) { return y < 64; });
    ExpectTrue(r.hitBlock, "level throw lands on the floor");
    ExpectTrue(r.impact.z > 10.0 && r.impact.z < 40.0, "snowball lands 10-40 blocks out");
    ExpectTrue(r.impact.y >= 63.0 && r.impact.y < 64.0, "impact inside the floor block");
    ExpectTrue(r.points.size() >= 3, "arc has several points");
}

static void TestTrajectoryWall() {
    beta::ProjectileParams p = beta::ParamsFor(beta::PROJECTILE_BOW, 20);
    beta::TrajectoryResult r = beta::SimulateTrajectory(
        0.5, 65.0, 0.5, 0.0f, 0.0f, p, [](int, int, int z) { return z == 2; });
    ExpectTrue(r.hitBlock, "arrow stops at a 1-block wall");
    ExpectTrue(r.impact.z >= 2.0 && r.impact.z < 3.0, "no tunnelling through the wall");
}

static void TestTrajectoryNoHit() {
    beta::ProjectileParams p = beta::ParamsFor(beta::PROJECTILE_THROWABLE, 0);
    beta::TrajectoryResult r = beta::SimulateTrajectory(
        0.0, 65.0, 0.0, 90.0f, -30.0f, p, [](int, int, int) { return false; }, 400, -64.0);
    ExpectTrue(!r.hitBlock, "void throw has no impact");
    ExpectTrue(r.impact.y < -64.0, "void throw falls below floor");
    ExpectTrue(r.impact.x < 0.0, "yaw 90 throws toward -X");
}

static void TestMurderMystery() {
    ExpectTrue(beta::IsMurderWeaponKey("item.swordIron"), "legacy iron sword");
    ExpectTrue(beta::IsMurderWeaponKey("item.minecraft.iron_sword"), "modern iron sword");
    ExpectTrue(beta::IsMurderWeaponKey("tile.sponge"), "legacy sponge block");
    ExpectTrue(beta::IsMurderWeaponKey("block.minecraft.dead_bush"), "modern dead bush");
    ExpectTrue(beta::IsMurderWeaponKey("item.fish.cod.raw"), "legacy fish variant");
    ExpectTrue(beta::IsMurderWeaponKey("item.minecraft.red_dye"), "modern dye suffix");
    ExpectTrue(beta::IsMurderWeaponKey("item.minecraft.spruce_boat"), "modern boat suffix");
    ExpectTrue(!beta::IsMurderWeaponKey("item.ingotGold"), "gold ingot is not a weapon");
    ExpectTrue(!beta::IsMurderWeaponKey("item.minecraft.bow"), "bow is not a knife");
    ExpectTrue(!beta::IsMurderWeaponKey(""), "empty");
    ExpectTrue(beta::IsBowKey("item.bow") && beta::IsBowKey("item.minecraft.bow"), "bow keys");
    ExpectTrue(beta::ClassifyMurderRole("item.minecraft.bow", true) == beta::MM_ROLE_MURDERER,
               "murderer stays marked");
    ExpectTrue(beta::ClassifyMurderRole("item.minecraft.bow", false) == beta::MM_ROLE_BOW, "bow role");
    ExpectTrue(beta::ClassifyMurderRole("item.minecraft.map", false) == beta::MM_ROLE_NONE, "innocent");
}

static void TestBreakPercent() {
    ExpectTrue(beta::BreakStageToPercent(0) == 10, "stage 0 is 10%");
    ExpectTrue(beta::BreakStageToPercent(9) == 100, "stage 9 is 100%");
    ExpectTrue(beta::BreakStageToPercent(-1) == 0, "negative stage");
    ExpectTrue(beta::BreakDamageToPercent(0.456f) == 46, "damage rounding");
    ExpectTrue(beta::BreakDamageToPercent(0.0f) == 0, "no damage");
}

static void TestModuleStatus() {
    ModuleStatusMap m;
    ExpectTrue(m.ToJson() == "{}", "empty map");
    ExpectTrue(m.Set("fastplace", "Missing \"rightClickDelay\""), "set changes");
    ExpectTrue(!m.Set("fastplace", "Missing \"rightClickDelay\""), "same value is no change");
    m.Set("hitselect", "No crosshair entity");
    ExpectTrue(m.ToJson() == "{\"fastplace\":\"Missing \\\"rightClickDelay\\\"\",\"hitselect\":\"No crosshair entity\"}",
               "json escapes and sorts");
    m.Clear("fastplace");
    ExpectTrue(m.Get("fastplace").empty(), "clear removes");
}

static void TestCombatTarget() {
    target::Facts f;
    f.stableName = "Notch";
    target::Policy p;
    ExpectTrue(target::IsValid(f, p), "plain player is valid");
    f.teammate = true;
    ExpectTrue(target::Evaluate(f, p) == target::TARGET_TEAMMATE, "teammate rejected");
    p.skipTeammates = false;
    ExpectTrue(target::IsValid(f, p), "teams off allows teammate");
    f.botLike = true;
    ExpectTrue(target::Evaluate(f, p) == target::TARGET_BOT, "bot rejected");
    p.skipBots = false;
    ExpectTrue(target::IsValid(f, p), "bot check off allows");
    f.health = 0.0f;
    ExpectTrue(target::Evaluate(f, p) == target::TARGET_DEAD, "dead rejected");
    f.health = 20.0f; f.stableName.clear();
    ExpectTrue(target::Evaluate(f, p) == target::TARGET_NO_NAME, "nameless rejected");
    f.isSelf = true;
    ExpectTrue(target::Evaluate(f, p) == target::TARGET_SELF, "self rejected first");
}

int main() {
    TestJumpReset();
    TestBowPower();
    TestTrajectoryFlatGround();
    TestTrajectoryWall();
    TestTrajectoryNoHit();
    TestMurderMystery();
    TestBreakPercent();
    TestModuleStatus();
    TestCombatTarget();
    if (g_failures == 0) {
        std::cout << "beta_modules_tests: all passed" << std::endl;
        return 0;
    }
    std::cerr << "beta_modules_tests: " << g_failures << " failure(s)" << std::endl;
    return 1;
}
