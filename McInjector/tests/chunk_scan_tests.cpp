#include <iostream>
#include <map>
#include <string>
#include <vector>

#include "../src/main/cpp/chunk_scan_common.h"

static int g_failures = 0;

static void ExpectTrue(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void TestRangeAndKey()
{
    ExpectTrue(lc::ClampChunkRange(0) == 1, "clamp min");
    ExpectTrue(lc::ClampChunkRange(9) == 8, "clamp max");
    ExpectTrue(lc::ChunkInChebyshevRange(1, 0, 0, 0, 1), "adjacent in range 1");
    ExpectTrue(!lc::ChunkInChebyshevRange(2, 0, 0, 0, 1), "two away out of range 1");
    ExpectTrue(lc::ChunkKey(1, 2) != lc::ChunkKey(2, 1), "keys distinguish axes");
}

static void TestMaxRangeAndBudget()
{
    ExpectTrue(lc::MaxEnabledChunkRange(true, 2, true, 6) == 6, "max of two ranges");
    ExpectTrue(lc::MaxEnabledChunkRange(false, 8, true, 3) == 3, "disabled side ignored");
    ExpectTrue(lc::MaxEnabledChunkRange3(true, 2, true, 4, true, 1) == 4, "max of three");
    ExpectTrue(lc::CombinedSectionChunkBudget(false, false) == 0, "budget none");
    ExpectTrue(lc::CombinedSectionChunkBudget(true, false) == 1, "budget block only");
    ExpectTrue(lc::CombinedSectionChunkBudget(false, true) == 4, "budget beds only");
    ExpectTrue(lc::CombinedSectionChunkBudget(true, true) == 4, "budget both uses max");
    ExpectTrue(lc::ChestEspChunkScanBudget() == 9, "chest scan budget fills 3x3 first");
}

static void TestNearToFarOrder()
{
    std::vector<std::pair<int, int> > offsets;
    lc::BuildNearToFarChunkOffsets(2, offsets);
    ExpectTrue(offsets.size() == 25, "5x5 ring");
    ExpectTrue(offsets[0].first == 0 && offsets[0].second == 0, "origin first");
    int last = 0;
    for (size_t i = 0; i < offsets.size(); i++) {
        int d = lc::AbsI(offsets[i].first) + lc::AbsI(offsets[i].second);
        ExpectTrue(d >= last, "manhattan nondecreasing");
        last = d;
    }
}

static void TestEvict()
{
    std::map<long long, int> cache;
    cache[lc::ChunkKey(0, 0)] = 1;
    cache[lc::ChunkKey(3, 0)] = 1;
    lc::EvictChunksOutsideRange(cache, 0, 0, 1);
    ExpectTrue(cache.size() == 1, "far chunk evicted");
    ExpectTrue(cache.find(lc::ChunkKey(0, 0)) != cache.end(), "origin kept");
}

int main()
{
    TestRangeAndKey();
    TestMaxRangeAndBudget();
    TestNearToFarOrder();
    TestEvict();

    if (g_failures != 0) {
        std::cerr << "Native chunk-scan tests failed: " << g_failures << std::endl;
        return 1;
    }
    std::cout << "Native chunk-scan tests passed." << std::endl;
    return 0;
}
