#include <iostream>
#include <string>
#include <vector>

#include "../src/main/cpp/block_esp_common.h"
#include "../src/main/cpp/json_config_reader.h"
#include "../src/main/cpp/bridge_capabilities.h"

static int g_failures = 0;

static void ExpectTrue(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectEqStr(const std::string& expected, const std::string& actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected='" << expected << "' actual='" << actual << "'" << std::endl;
        ++g_failures;
    }
}

static void ExpectEqU32(unsigned int expected, unsigned int actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void TestNormalizeId()
{
    ExpectEqStr("diamond_ore", lc::BlockEspNormalizeId("minecraft:diamond_ore"), "namespaced id");
    ExpectEqStr("diamond_ore", lc::BlockEspNormalizeId("diamond_ore"), "bare path");
    ExpectEqStr("diamond_ore", lc::BlockEspNormalizeId("block.minecraft.diamond_ore"), "translation key");
    ExpectEqStr("diamond_ore", lc::BlockEspNormalizeId("  Minecraft:Diamond_Ore "), "trim + lowercase");
    ExpectEqStr("custom_block", lc::BlockEspNormalizeId("modid:custom_block"), "custom namespace");
    ExpectEqStr("", lc::BlockEspNormalizeId(""), "empty");
}

static void TestParseColor()
{
    // IM_COL32 default packing: r | g<<8 | b<<16 | a<<24
    unsigned int expectedCyan = (0x00u | (0xE5u << 8) | (0xFFu << 16) | (0xFFu << 24));
    ExpectEqU32(expectedCyan, lc::BlockEspParseColor("00E5FF"), "cyan parse");
    ExpectEqU32(lc::BlockEspDefaultColor(), lc::BlockEspParseColor("xyz"), "invalid color -> default");
    ExpectEqU32(lc::BlockEspDefaultColor(), lc::BlockEspParseColor("12345"), "wrong length -> default");

    unsigned int gold = (0xFFu | (0xD7u << 8) | (0x00u << 16) | (0xFFu << 24));
    ExpectEqU32(gold, lc::BlockEspParseColor("FFD700"), "gold parse");
}

static void TestParseTargets()
{
    auto t = lc::ParseBlockEspTargets("diamond_ore=00E5FF;gold_ore=FFD700");
    ExpectTrue(t.size() == 2, "two targets");
    if (t.size() == 2) {
        ExpectEqStr("diamond_ore", t[0].id, "target 0 id");
        ExpectEqStr("gold_ore", t[1].id, "target 1 id");
    }

    // Malformed entries skipped, dedupe by normalized id.
    auto t2 = lc::ParseBlockEspTargets("minecraft:diamond_ore=00E5FF;;garbage;=AABBCC;diamond_ore=FFFFFF");
    ExpectTrue(t2.size() == 1, "dedupe + skip malformed -> one target");
    if (!t2.empty()) {
        ExpectEqStr("diamond_ore", t2[0].id, "deduped id");
        unsigned int expectedCyan = (0x00u | (0xE5u << 8) | (0xFFu << 16) | (0xFFu << 24));
        ExpectEqU32(expectedCyan, t2[0].color, "first color wins");
    }

    auto t3 = lc::ParseBlockEspTargets("");
    ExpectTrue(t3.empty(), "empty string -> no targets");
}

static void TestParsesFromQuotedConfigValue()
{
    // The whole list is delivered as a quoted JSON string; SimpleJsonConfigReader must
    // return it intact (commas/semicolons/colons inside quotes are preserved).
    const std::string line =
        "{\"type\":\"config\",\"blockEspBlocks\":\"diamond_ore=00E5FF;gold_ore=FFD700\",\"blockEspRange\":6}";
    lc::SimpleJsonConfigReader reader(line);
    std::string raw = reader.GetString("blockEspBlocks");
    ExpectEqStr("diamond_ore=00E5FF;gold_ore=FFD700", raw, "quoted list survives reader");
    auto t = lc::ParseBlockEspTargets(raw);
    ExpectTrue(t.size() == 2, "parsed from config value");
}

static void TestCapabilitiesAdvertiseBlockEsp()
{
    const std::string legacy = lc::LegacyCapabilitiesJson();
    const std::string modern = lc::ModernCapabilitiesJson();
    ExpectTrue(legacy.find("\"blockesp\"") != std::string::npos, "legacy advertises blockesp module");
    ExpectTrue(modern.find("\"blockesp\"") != std::string::npos, "modern advertises blockesp module");
    ExpectTrue(legacy.find("\"blockespblocks\"") != std::string::npos, "legacy advertises blockespblocks setting");
    ExpectTrue(modern.find("\"blockespblocks\"") != std::string::npos, "modern advertises blockespblocks setting");
}

int main()
{
    TestNormalizeId();
    TestParseColor();
    TestParseTargets();
    TestParsesFromQuotedConfigValue();
    TestCapabilitiesAdvertiseBlockEsp();

    if (g_failures != 0) {
        std::cerr << "Native block-esp tests failed: " << g_failures << std::endl;
        return 1;
    }

    std::cout << "Native block-esp tests passed." << std::endl;
    return 0;
}
