#include <iostream>
#include <string>
#include <vector>

#include "../src/main/cpp/chest_esp_common.h"
#include "../src/main/cpp/bridge_capabilities.h"

static int g_failures = 0;

static void ExpectTrue(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void TestClampRange()
{
    ExpectTrue(lc::ClampOverlayChunkRange(0) == 1, "range min 1");
    ExpectTrue(lc::ClampChestEspRange(4) == 4, "range default 4");
    ExpectTrue(lc::ClampChestEspRange(9) == 8, "range max 8");
}

static void TestChunkRange()
{
    // Player at origin: chunk (0,0). Range 1 includes chunks -1..1.
    ExpectTrue(lc::ChestEspInChunkRange(0, 0, 0.5, 0.5, 1), "same chunk");
    ExpectTrue(lc::ChestEspInChunkRange(16, 0, 0.5, 0.5, 1), "adjacent chunk");
    ExpectTrue(!lc::ChestEspInChunkRange(32, 0, 0.5, 0.5, 1), "two chunks away excluded at range 1");
    ExpectTrue(lc::ChestEspInChunkRange(32, 0, 0.5, 0.5, 2), "two chunks away included at range 2");
}

static void TestNearestFirstIgnoresListOrder()
{
    // Tile-entity lists often yield far chests first. A count cap without a
    // distance sort would draw those and skip the player-adjacent chest.
    std::vector<lc::ChestEspCandidate> items;
    items.push_back({ 80, 64, 80, 80.0 * 80.0 + 80.0 * 80.0 });
    items.push_back({ 1, 64, 1, 1.0 + 1.0 });
    items.push_back({ 40, 64, 0, 40.0 * 40.0 });
    lc::ChestEspSortNearestFirst(items);
    ExpectTrue(items.size() == 3, "three candidates");
    ExpectTrue(items[0].bx == 1 && items[0].bz == 1, "nearest chest is first after sort");
    ExpectTrue(items[2].bx == 80 && items[2].bz == 80, "farthest chest is last after sort");
}

static void TestSortSurvivesNaN()
{
    volatile double zero = 0.0;
    std::vector<lc::ChestEspCandidate> items;
    items.push_back({ 8, 64, 8, 128.0 });
    items.push_back({ 1, 64, 1, 2.0 });
    items.push_back({ 99, 64, 99, zero / zero });
    lc::ChestEspSortNearestFirst(items);
    ExpectTrue(items.size() == 3, "nan sort keeps three candidates");
    ExpectTrue(items[0].bx == 1 && items[0].bz == 1, "finite nearest stays first with nan present");
}

static void TestOverlayScanAndHeldItemGates()
{
    ExpectTrue(lc::NametagShouldFetchHeldItem(true, 12.0), "held item at 12m");
    ExpectTrue(lc::NametagShouldFetchHeldItem(true, 24.0), "held item at 24m");
    ExpectTrue(!lc::NametagShouldFetchHeldItem(true, 24.1), "no held item past 24m");
    ExpectTrue(!lc::NametagShouldFetchHeldItem(false, 8.0), "held item disabled");
    volatile double zero = 0.0;
    ExpectTrue(!lc::NametagShouldFetchHeldItem(true, zero / zero), "no held item for nan dist");

    ExpectTrue(lc::ChestEspNeedClassNameFallback(false, false), "fallback when no typed classes");
    ExpectTrue(lc::ChestEspNeedClassNameFallback(true, false), "fallback when ender class missing");
    ExpectTrue(!lc::ChestEspNeedClassNameFallback(true, true), "no fallback when both typed classes exist");
    ExpectTrue(lc::ChestEspNeedNameFallback(3, 4), "modern fallback until all typed classes resolve");
    ExpectTrue(!lc::ChestEspNeedNameFallback(4, 4), "modern skip fallback when typed classes complete");

    ExpectTrue(lc::OverlayEntityNeedsFullScan(true, true, false, false, false, 10.0, false),
        "nametags in range need full scan");
    ExpectTrue(!lc::OverlayEntityNeedsFullScan(true, false, false, false, false, 80.0, false),
        "nametags out of range skip full scan");
    ExpectTrue(lc::OverlayEntityNeedsFullScan(true, false, false, false, true, 70.0, false),
        "closest-player scan keeps 70m entities");
    ExpectTrue(lc::OverlayEntityNeedsFullScan(false, false, false, true, false, 200.0, false),
        "hide-vanilla still walks every player");
    ExpectTrue(!lc::OverlayEntityNeedsFullScan(false, false, false, false, false, 80.0, true),
        "json slots ignore far entities");
    ExpectTrue(lc::OverlayEntityNeedsFullScan(false, false, false, false, false, 20.0, true),
        "json slots keep nearby entities");
}

static void TestCapabilitiesAdvertiseRangeNotCount()
{
    const std::string legacy = lc::LegacyCapabilitiesJson();
    const std::string modern = lc::ModernCapabilitiesJson();
    ExpectTrue(legacy.find("\"chesteprange\"") != std::string::npos, "legacy advertises chesteprange");
    ExpectTrue(modern.find("\"chesteprange\"") != std::string::npos, "modern advertises chesteprange");
    ExpectTrue(legacy.find("\"nametagsrange\"") != std::string::npos, "legacy advertises nametagsrange");
    ExpectTrue(modern.find("\"nametagsrange\"") != std::string::npos, "modern advertises nametagsrange");
    ExpectTrue(legacy.find("\"chestespmaxcount\"") == std::string::npos, "legacy dropped chestespmaxcount");
    ExpectTrue(modern.find("\"chestespmaxcount\"") == std::string::npos, "modern dropped chestespmaxcount");
    ExpectTrue(legacy.find("\"nametagmaxcount\"") == std::string::npos, "legacy dropped nametagmaxcount");
    ExpectTrue(modern.find("\"nametagmaxcount\"") == std::string::npos, "modern dropped nametagmaxcount");
}

int main()
{
    TestClampRange();
    TestChunkRange();
    TestNearestFirstIgnoresListOrder();
    TestSortSurvivesNaN();
    TestOverlayScanAndHeldItemGates();
    TestCapabilitiesAdvertiseRangeNotCount();

    if (g_failures != 0) {
        std::cerr << "Native chest-esp tests failed: " << g_failures << std::endl;
        return 1;
    }

    std::cout << "Native chest-esp tests passed." << std::endl;
    return 0;
}
