// Native harness tests for bedplates_common.h (Vape BedPlates layer logic).

#include <iostream>
#include <string>
#include <vector>

#include "../src/main/cpp/bedplates_common.h"

using namespace lc;

static int g_failures = 0;

static void ExpectTrue(bool cond, const char* message) {
    if (!cond) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectEqI(int expected, int actual, const char* message) {
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void ExpectEqS(const std::string& expected, const std::string& actual, const char* message) {
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=\"" << expected
                  << "\" actual=\"" << actual << "\"" << std::endl;
        ++g_failures;
    }
}

static void TestIsBedBlockId() {
    ExpectTrue(IsBedBlockId("bed"), "plain bed");
    ExpectTrue(IsBedBlockId("minecraft:bed"), "namespaced bed");
    ExpectTrue(IsBedBlockId("tile.bed"), "1.8.9 unlocalized");
    ExpectTrue(IsBedBlockId("tile.bed.name"), "1.8.9 unlocalized.name");
    ExpectTrue(IsBedBlockId("red_bed"), "red_bed");
    ExpectTrue(IsBedBlockId("block.minecraft.white_bed"), "translation key white_bed");
    ExpectTrue(IsBedBlockId("BedBlock"), "BedBlock class-ish");
    ExpectTrue(!IsBedBlockId("bedrock"), "bedrock is not a bed");
    ExpectTrue(!IsBedBlockId("red_bed_block"), "suffix must be _bed");
    ExpectTrue(!IsIgnorableBedPlateBlockId("wool"), "wool kept");
    ExpectTrue(IsIgnorableBedPlateBlockId("air"), "air ignored");
    ExpectTrue(IsIgnorableBedPlateBlockId("air_name"), "air_name ignored");
    ExpectTrue(IsIgnorableBedPlateBlockId("tile.air.name"), "tile.air.name ignored");
    ExpectTrue(IsIgnorableBedPlateBlockId("cave_air"), "cave_air ignored");
    ExpectTrue(!IsIgnorableBedPlateBlockId("stairs"), "stairs kept");
    ExpectTrue(!IsIgnorableBedPlateBlockId("barrier"), "barrier kept");
}

static void TestFacingOffsets() {
    ExpectEqI(kBedFacingNorth, BedFacingFromFootOffset(0, 1), "foot +Z -> NORTH");
    ExpectEqI(kBedFacingSouth, BedFacingFromFootOffset(0, -1), "foot -Z -> SOUTH");
    ExpectEqI(kBedFacingWest, BedFacingFromFootOffset(1, 0), "foot +X -> WEST");
    ExpectEqI(kBedFacingEast, BedFacingFromFootOffset(-1, 0), "foot -X -> EAST");
    ExpectEqI(kBedFacingNorth, BedFacingFromFootOffset(0, 0), "zero -> NORTH default");

    int ox = 0, oz = 0;
    BedFootOffsetFromFacing(kBedFacingNorth, &ox, &oz);
    ExpectEqI(0, ox, "NORTH ox");
    ExpectEqI(1, oz, "NORTH oz");
    BedFootOffsetFromFacing(kBedFacingEast, &ox, &oz);
    ExpectEqI(-1, ox, "EAST ox");
    ExpectEqI(0, oz, "EAST oz");
}

static void TestCanonicalHalf() {
    ExpectTrue(BedPlatesIsCanonicalHalf(false, false), "unpaired kept");
    ExpectTrue(!BedPlatesIsCanonicalHalf(true, false), "negX skipped");
    ExpectTrue(!BedPlatesIsCanonicalHalf(false, true), "negZ skipped");
}

static void TestLayerSamplesAndCounts() {
    std::vector<BedPlateSample> samples;
    CollectBedPlateSamples(0, 64, 0, kBedFacingNorth, samples);
    ExpectTrue(!samples.empty(), "samples generated");

    // Every sample must land in layers 1..3.
    for (size_t i = 0; i < samples.size(); ++i) {
        ExpectTrue(samples[i].layer >= 1 && samples[i].layer <= 3, "layer in 1..3");
    }

    BedPlateCountState state;
    state.x = 0; state.y = 64; state.z = 0;
    // Simulate wool at layer-1 shell positions and end_stone deeper.
    for (size_t i = 0; i < samples.size(); ++i) {
        if (samples[i].layer == 1)
            state.incrementBlock(1, "white_wool");
        else if (samples[i].layer == 2)
            state.incrementBlock(2, "end_stone");
        else
            state.incrementBlock(3, "obsidian");
    }
    state.incrementBlock(1, "air"); // ignored
    state.incrementBlock(1, "red_bed"); // ignored
    state.sortLayersByFrequency();

    std::vector<std::string> visible;
    state.collectVisibleBlockIds(visible);
    ExpectEqI(3, (int)visible.size(), "three unique defense blocks");
    ExpectEqS("white_wool", visible[0], "layer1 first");
    ExpectEqS("end_stone", visible[1], "layer2 second");
    ExpectEqS("obsidian", visible[2], "layer3 third");
}

static void TestFrequencySortWithinLayer() {
    BedPlateCountState state;
    state.incrementBlock(1, "glass");
    state.incrementBlock(1, "wool");
    state.incrementBlock(1, "wool");
    state.incrementBlock(1, "wool");
    state.incrementBlock(1, "wood");
    state.incrementBlock(1, "wood");
    state.sortLayersByFrequency();
    std::vector<std::string> visible;
    state.collectVisibleBlockIds(visible);
    ExpectTrue(visible.size() >= 2, "at least wool+wood");
    ExpectEqS("wool", visible[0], "most frequent first");
    ExpectEqS("wood", visible[1], "second frequency");
}

static void TestPrettyLabel() {
    ExpectEqS("White Wool", BedPlatePrettyLabel("minecraft:white_wool"), "pretty wool");
    ExpectEqS("End Stone", BedPlatePrettyLabel("end_stone"), "pretty end stone");
    ExpectEqS("Bed", BedPlatePrettyLabel("bed"), "pretty bed");
    ExpectEqS("Obs", BedPlateShortLabel("obsidian"), "short obs");
    ExpectEqS("Gls", BedPlateShortLabel("white_stained_glass"), "short glass");
    ExpectEqS("WWl", BedPlateShortLabel("white_wool"), "short white wool");
    ExpectEqS("ES", BedPlateShortLabel("end_stone"), "short end stone");
}

int main() {
    TestIsBedBlockId();
    TestFacingOffsets();
    TestCanonicalHalf();
    TestLayerSamplesAndCounts();
    TestFrequencySortWithinLayer();
    TestPrettyLabel();

    if (g_failures != 0) {
        std::cerr << g_failures << " bedplates test(s) failed" << std::endl;
        return 1;
    }
    std::cout << "All bedplates_tests passed" << std::endl;
    return 0;
}
