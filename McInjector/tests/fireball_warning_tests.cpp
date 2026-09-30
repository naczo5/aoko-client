// Native harness tests for fireball_warning_common.h.

#include <iostream>
#include <string>

#include "../src/main/cpp/fireball_warning_common.h"

using namespace lc;

static int g_failures = 0;

static void ExpectTrue(bool cond, const char* message) {
    if (!cond) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void TestFireballMatches() {
    ExpectTrue(IsFireballHolderText("Fire Charge"), "Fire Charge display");
    ExpectTrue(IsFireballHolderText("Fireball"), "Fireball display");
    ExpectTrue(IsFireballHolderText("fireball"), "lowercase fireball");
    ExpectTrue(IsFireballHolderText("minecraft:fire_charge"), "registry fire_charge");
    ExpectTrue(IsFireballHolderText("item.fireCharge"), "unlocalized fireCharge");
    ExpectTrue(IsFireballHolderText("Fire Charge (1/1)"), "with durability suffix");
    ExpectTrue(IsFireballHolderText("Fire Ball"), "spaced variant");
}

static void TestFireballRejects() {    ExpectTrue(!IsFireballHolderText(""), "empty");
    ExpectTrue(!IsFireballHolderText("Diamond Sword"), "sword");
    ExpectTrue(!IsFireballHolderText("Firework Rocket"), "firework is not fireball");
    ExpectTrue(!IsFireballHolderText("Flint and Steel"), "flint and steel");
    ExpectTrue(!IsFireballHolderText("Bow"), "bow");
    ExpectTrue(!IsFireballHolderText("Fire Aspect"), "enchant-like name");
}

static void TestFireballSoundDue() {
    ExpectTrue(FireballWarningSoundDue(5000, 0, true, true), "due after cooldown from zero");
    ExpectTrue(!FireballWarningSoundDue(1000, 0, true, true), "not due inside cooldown");
    ExpectTrue(!FireballWarningSoundDue(5000, 4000, true, true), "not due when recently played");
    ExpectTrue(FireballWarningSoundDue(6000, 4000, true, true), "due exactly at cooldown");
    ExpectTrue(!FireballWarningSoundDue(5000, 0, false, true), "not due without holder");
    ExpectTrue(!FireballWarningSoundDue(5000, 0, true, false), "not due when sound disabled");
    ExpectTrue(FireballWarningSoundDue(100u, 4294964896u, true, true), "wrap-around arithmetic");
}

int main() {
    TestFireballMatches();
    TestFireballRejects();
    TestFireballSoundDue();

    if (g_failures != 0) {
        std::cerr << g_failures << " fireball_warning test(s) failed" << std::endl;
        return 1;
    }
    std::cout << "All fireball_warning_tests passed" << std::endl;
    return 0;
}
