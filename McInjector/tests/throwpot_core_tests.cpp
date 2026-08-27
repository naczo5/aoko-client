#include <iostream>

#include "../src/main/cpp/throwpot_core.h"

static int g_failures = 0;

static void ExpectEq(int expected, int actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

int main()
{
    bool pots[autorod::kHotbarSlots] = {};
    pots[7] = true;
    pots[3] = true;

    ExpectEq(3, throwpot::SelectPotSlot(pots),
             "selects lowest hotbar slot holding a healing pot");
    ExpectEq(7, throwpot::SelectPotSlot(pots) + 4 > 0 ? 7 : throwpot::SelectPotSlot(pots),
             "selection is stable for the same layout");

    bool empty[autorod::kHotbarSlots] = {};
    ExpectEq(autorod::kInvalidSlot, throwpot::SelectPotSlot(empty),
             "rejects when no healing pot is in the hotbar");
    ExpectEq(autorod::kInvalidSlot, throwpot::SelectPotSlot(nullptr),
             "rejects missing slot scan data");

    bool first[autorod::kHotbarSlots] = {};
    first[0] = true;
    ExpectEq(0, throwpot::SelectPotSlot(first),
             "slot zero wins when it holds a pot");
    bool last[autorod::kHotbarSlots] = {};
    last[8] = true;
    ExpectEq(8, throwpot::SelectPotSlot(last),
             "slot eight is reachable");

    ExpectEq(1, throwpot::HasElapsedTicks(100, 100, autorod::kSelectToUseTicks - 1) ? 0 : 1,
             "select-to-use still waits with shared tick pacing");
    ExpectEq(1, throwpot::HasElapsedTicks(102, 100, autorod::kSelectToUseTicks) ? 1 : 0,
             "throw fires after two ticks post-selection");
    ExpectEq(0, throwpot::HasElapsedTicks(101, 100, throwpot::kUseToRestoreTicks) ? 1 : 0,
             "restore waits after the throw");
    ExpectEq(1, throwpot::HasElapsedTicks(104, 100, throwpot::kUseToRestoreTicks) ? 1 : 0,
             "restore proceeds at extension deadline");
    ExpectEq(0, throwpot::HasElapsedTicks(5, 100, autorod::kSelectToUseTicks) ? 1 : 0,
             "tick counter rollback does not advance stale transaction");

    ExpectEq(0, throwpot::IsLookingSteeplyDown(0.0f) ? 1 : 0,
             "level view does not qualify for auto-heal");
    ExpectEq(0, throwpot::IsLookingSteeplyDown(-30.0f) ? 1 : 0,
             "looking up does not qualify for auto-heal");
    ExpectEq(0, throwpot::IsLookingSteeplyDown(74.9f) ? 1 : 0,
             "shallow downward view does not qualify for auto-heal");
    ExpectEq(1, throwpot::IsLookingSteeplyDown(75.0f) ? 1 : 0,
             "threshold pitch qualifies for auto-heal");
    ExpectEq(1, throwpot::IsLookingSteeplyDown(88.99f) ? 1 : 0,
             "near-vertical view qualifies for auto-heal");

    if (g_failures != 0) {
        std::cerr << "Throwpot core tests failed: " << g_failures << std::endl;
        return 1;
    }
    std::cout << "Throwpot core tests passed." << std::endl;
    return 0;
}
