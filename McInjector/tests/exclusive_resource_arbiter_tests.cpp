#include <iostream>
#include "../src/main/cpp/exclusive_resource_arbiter.h"

static int g_failures = 0;

static void ExpectBool(bool expected, bool actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << (expected ? "true" : "false")
                  << " actual=" << (actual ? "true" : "false") << std::endl;
        ++g_failures;
    }
}

static void ExpectInt(int expected, int actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

int main()
{
    using lc::ExclusiveOwner;
    using lc::ExclusiveResource;
    using lc::ExclusiveResourceArbiter;

    ExclusiveResourceArbiter arbiter;

    // A free resource can be claimed without a conflict.
    auto first = arbiter.ObserveAttempt(ExclusiveResource::Hotbar, ExclusiveOwner::AutoRod);
    ExpectBool(false, first.conflict, "first claim has no conflict");
    ExpectInt(static_cast<int>(ExclusiveOwner::AutoRod),
              static_cast<int>(arbiter.OwnerOf(ExclusiveResource::Hotbar)),
              "AutoRod owns the hotbar");

    // A second owner is reported, but shadow mode does not steal the claim.
    auto second = arbiter.ObserveAttempt(ExclusiveResource::Hotbar, ExclusiveOwner::AutoTool);
    ExpectBool(true, second.conflict, "second owner conflicts");
    ExpectInt(static_cast<int>(ExclusiveOwner::AutoRod),
              static_cast<int>(second.previous), "previous owner is reported");
    ExpectInt(static_cast<int>(ExclusiveOwner::AutoRod),
              static_cast<int>(arbiter.OwnerOf(ExclusiveResource::Hotbar)),
              "shadow mode preserves the first owner");
    ExpectInt(1, static_cast<int>(arbiter.ConflictCount(ExclusiveResource::Hotbar)),
              "conflict counter increments");

    // Releasing a non-owner is a no-op.
    arbiter.Release(ExclusiveResource::Hotbar, ExclusiveOwner::AutoTool);
    ExpectInt(static_cast<int>(ExclusiveOwner::AutoRod),
              static_cast<int>(arbiter.OwnerOf(ExclusiveResource::Hotbar)),
              "non-owner release does not clear the claim");

    // The actual owner can release, after which another module may claim it.
    arbiter.Release(ExclusiveResource::Hotbar, ExclusiveOwner::AutoRod);
    ExpectInt(static_cast<int>(ExclusiveOwner::None),
              static_cast<int>(arbiter.OwnerOf(ExclusiveResource::Hotbar)),
              "owner release clears the claim");
    auto third = arbiter.ObserveAttempt(ExclusiveResource::Hotbar, ExclusiveOwner::AutoTool);
    ExpectBool(false, third.conflict, "claim after release has no conflict");

    // Resources are independent.
    auto use = arbiter.ObserveAttempt(ExclusiveResource::UseItem, ExclusiveOwner::KillAuraBlock);
    ExpectBool(false, use.conflict, "use-item is independent from hotbar");
    ExpectInt(static_cast<int>(ExclusiveOwner::AutoTool),
              static_cast<int>(arbiter.OwnerOf(ExclusiveResource::Hotbar)),
              "hotbar owner is unchanged");

    // Reset clears owners and counters.
    arbiter.Reset();
    ExpectInt(static_cast<int>(ExclusiveOwner::None),
              static_cast<int>(arbiter.OwnerOf(ExclusiveResource::UseItem)),
              "reset clears use-item");
    ExpectInt(0, static_cast<int>(arbiter.ConflictCount(ExclusiveResource::Hotbar)),
              "reset clears conflict counters");

    if (g_failures != 0) {
        std::cerr << g_failures << " exclusive resource arbiter test(s) failed." << std::endl;
        return 1;
    }
    std::cout << "Exclusive resource arbiter tests passed." << std::endl;
    return 0;
}
