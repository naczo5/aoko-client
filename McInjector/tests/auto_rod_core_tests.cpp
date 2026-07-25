#include <iostream>

#include "../src/main/cpp/auto_rod_core.h"

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
    bool rods[autorod::kHotbarSlots] = {};
    rods[6] = true;
    rods[2] = true;

    ExpectEq(2, autorod::SelectTargetSlot(4, 0, true, rods),
             "auto selects lowest rod slot");
    ExpectEq(6, autorod::SelectTargetSlot(4, 7, true, rods),
             "verified forced accepts rod");
    ExpectEq(autorod::kInvalidSlot, autorod::SelectTargetSlot(4, 5, true, rods),
             "verified forced rejects non-rod");
    ExpectEq(4, autorod::SelectTargetSlot(3, 5, false, nullptr),
             "unverified forced accepts arbitrary slot without rod data");
    ExpectEq(autorod::kInvalidSlot, autorod::SelectTargetSlot(-1, 0, true, rods),
             "negative original slot rejected");
    ExpectEq(autorod::kInvalidSlot, autorod::SelectTargetSlot(9, 0, true, rods),
             "out-of-range original slot rejected");
    ExpectEq(autorod::kInvalidSlot, autorod::SelectTargetSlot(0, -1, false, rods),
             "negative mode rejected");
    ExpectEq(autorod::kInvalidSlot, autorod::SelectTargetSlot(0, 10, false, rods),
             "out-of-range mode rejected");

    bool empty[autorod::kHotbarSlots] = {};
    ExpectEq(autorod::kInvalidSlot, autorod::SelectTargetSlot(0, 0, true, empty),
             "auto rejects missing rod");
    ExpectEq(autorod::kInvalidSlot, autorod::SelectTargetSlot(0, 0, false, nullptr),
             "auto always requires rod data");

    ExpectEq(0, autorod::HasElapsedTicks(101, 100, autorod::kSelectToUseTicks) ? 1 : 0,
             "use waits for two completed ticks after selection");
    ExpectEq(1, autorod::HasElapsedTicks(102, 100, autorod::kSelectToUseTicks) ? 1 : 0,
             "use advances after two ticks");
    ExpectEq(0, autorod::ShouldRestoreAfterUse(200, 200, 1, false, false) ? 1 : 0,
             "one-tick extension does not restore immediately");
    ExpectEq(1, autorod::ShouldRestoreAfterUse(201, 200, 1, false, false) ? 1 : 0,
             "one-tick extension restores after one tick");
    ExpectEq(0, autorod::ShouldRestoreAfterUse(211, 200, 12, false, false) ? 1 : 0,
             "twelve-tick extension remains selected before deadline");
    ExpectEq(1, autorod::ShouldRestoreAfterUse(212, 200, 12, false, false) ? 1 : 0,
             "twelve-tick extension restores at deadline");
    ExpectEq(0, autorod::ShouldRestoreAfterUse(239, 200, 40, false, false) ? 1 : 0,
             "forty-tick extension remains selected before deadline");
    ExpectEq(1, autorod::ShouldRestoreAfterUse(240, 200, 40, false, false) ? 1 : 0,
             "forty-tick extension restores at deadline");
    ExpectEq(0, autorod::ShouldRestoreAfterUse(250, 200, 4, true, false) ? 1 : 0,
             "hold mode remains selected until release");
    ExpectEq(0, autorod::ShouldRestoreAfterUse(200, 200, 4, true, true) ? 1 : 0,
             "hold release still waits at least one tick after use");
    ExpectEq(1, autorod::ShouldRestoreAfterUse(201, 200, 4, true, true) ? 1 : 0,
             "hold release restores after minimum tick");
    ExpectEq(0, autorod::HasElapsedTicks(300, 300, autorod::kRestoreSettleTicks) ? 1 : 0,
             "restored slot gets a vanilla synchronization tick");
    ExpectEq(1, autorod::HasElapsedTicks(301, 300, autorod::kRestoreSettleTicks) ? 1 : 0,
             "transaction completes after restore settle tick");
    ExpectEq(0, autorod::HasElapsedTicks(5, 100, autorod::kSelectToUseTicks) ? 1 : 0,
             "tick counter rollback does not advance stale transaction");


    if (g_failures != 0) {
        std::cerr << "Auto Rod core tests failed: " << g_failures << std::endl;
        return 1;
    }
    std::cout << "Auto Rod core tests passed." << std::endl;
    return 0;
}
