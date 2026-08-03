#include <cmath>
#include <iostream>
#include <string>

#include "../src/main/cpp/bridge_capabilities.h"
#include "../src/main/cpp/json_config_reader.h"

static int g_failures = 0;

static void ExpectTrue(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectEq(const std::string& expected, const std::string& actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected='" << expected << "' actual='" << actual << "'" << std::endl;
        ++g_failures;
    }
}

static void ExpectEq(int expected, int actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void ExpectNear(float expected, float actual, float epsilon, const char* message)
{
    if (std::fabs(expected - actual) > epsilon) {
        std::cerr << "FAIL: " << message << " expected=" << expected << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void TestReadsQuotedAndUnquotedValues()
{
    const std::string line =
        "{\"type\":\"config\",\"armed\":true,\"minCPS\":8.5,\"moduleListStyle\": 3,\"guiTheme\":\"Dark Blue\"}";
    lc::SimpleJsonConfigReader reader(line);

    ExpectEq("config", reader.GetString("type"), "type");
    ExpectEq("Dark Blue", reader.GetString("guiTheme"), "guiTheme");
    ExpectTrue(reader.GetBool("armed"), "armed bool");
    ExpectNear(8.5f, reader.GetFloat("minCPS"), 0.0001f, "minCPS float");
    ExpectEq(3, reader.GetInt("moduleListStyle"), "moduleListStyle int");
}

static void TestFallbacksOnMissingOrInvalidValues()
{
    const std::string line =
        "{\"type\":\"config\",\"velocityHorizontal\":\"oops\",\"velocityVertical\":,"
        "\"partialInt\":\"12junk\",\"partialFloat\":\"3.5ms\",\"notFinite\":\"NaN\","
        "\"infinity\":\"Infinity\",\"overflow\":\"999999999999999999999999\","
        "\"invalidBool\":\"yes\",\"missingBool\":false}";
    lc::SimpleJsonConfigReader reader(line);

    ExpectEq("config", reader.GetString("type"), "type");
    ExpectEq(77, reader.GetInt("velocityHorizontal", 77), "int fallback on invalid");
    ExpectNear(9.25f, reader.GetFloat("velocityVertical", 9.25f), 0.0001f, "float fallback on invalid");
    ExpectEq(13, reader.GetInt("partialInt", 13), "int fallback on trailing junk");
    ExpectNear(4.25f, reader.GetFloat("partialFloat", 4.25f), 0.0001f, "float fallback on trailing junk");
    ExpectNear(5.25f, reader.GetFloat("notFinite", 5.25f), 0.0001f, "float fallback on NaN");
    ExpectNear(6.25f, reader.GetFloat("infinity", 6.25f), 0.0001f, "float fallback on infinity");
    ExpectEq(14, reader.GetInt("overflow", 14), "int fallback on overflow");
    ExpectTrue(reader.GetBool("invalidBool", true), "bool fallback on invalid token");
    ExpectTrue(reader.GetBool("missingField", true), "bool fallback when missing");
    ExpectTrue(!reader.GetBool("missingBool", true), "bool explicit false");
}

static void TestAcceptsWhitespaceAroundNumbers()
{
    const std::string line =
        "{\"type\":\"config\",\"integer\": 42  ,\"decimal\": -1.25  }";
    lc::SimpleJsonConfigReader reader(line);

    ExpectEq(42, reader.GetInt("integer", 0), "int with surrounding whitespace");
    ExpectNear(-1.25f, reader.GetFloat("decimal", 0.0f), 0.0001f, "float with surrounding whitespace");
}

static void TestClampUtilities()
{
    ExpectEq(1, lc::ClampInt(-5, 1, 20), "ClampInt lower bound");
    ExpectEq(20, lc::ClampInt(50, 1, 20), "ClampInt upper bound");
    ExpectEq(7, lc::ClampInt(7, 1, 20), "ClampInt passthrough");

    ExpectNear(1.0f, lc::ClampFloat(-0.5f, 1.0f, 6.0f), 0.0001f, "ClampFloat lower bound");
    ExpectNear(6.0f, lc::ClampFloat(8.0f, 1.0f, 6.0f), 0.0001f, "ClampFloat upper bound");
    ExpectNear(3.5f, lc::ClampFloat(3.5f, 1.0f, 6.0f), 0.0001f, "ClampFloat passthrough");
}

static void TestCapabilitiesPayloads()
{
    const std::string legacy = lc::LegacyCapabilitiesJson();
    const std::string modern = lc::ModernCapabilitiesJson();

    ExpectTrue(legacy.find("\"triggerbot\"") == std::string::npos, "legacy should not advertise triggerbot module");
    ExpectTrue(modern.find("\"triggerbot\"") != std::string::npos, "modern should advertise triggerbot module");
    ExpectTrue(modern.find("\"reloadmappingsnonce\"") != std::string::npos, "modern should advertise reloadMappings nonce");
    ExpectTrue(legacy.find("\"keybindautoclicker\"") != std::string::npos, "legacy should advertise keybind setting");
    ExpectTrue(legacy.find("\"autorod\"") != std::string::npos, "legacy should advertise Auto Rod module");
    ExpectTrue(modern.find("\"autorod\"") != std::string::npos, "modern should advertise Auto Rod module");
    ExpectTrue(legacy.find("\"autorodenabled\"") != std::string::npos, "legacy should advertise Auto Rod enabled setting");
    ExpectTrue(modern.find("\"autorodslotmode\"") != std::string::npos, "modern should advertise Auto Rod slot setting");
    ExpectTrue(legacy.find("\"autorodverifyforcedslot\"") != std::string::npos, "legacy should advertise Auto Rod verification setting");
    ExpectTrue(legacy.find("\"autorodextensionticks\"") != std::string::npos, "legacy should advertise Auto Rod extension setting");
    ExpectTrue(modern.find("\"autorodextensionticks\"") != std::string::npos, "modern should advertise Auto Rod extension setting");
    ExpectTrue(legacy.find("\"autorodholdtoextend\"") != std::string::npos, "legacy should advertise Auto Rod hold setting");
    ExpectTrue(modern.find("\"autorodholdtoextend\"") != std::string::npos, "modern should advertise Auto Rod hold setting");
    ExpectTrue(modern.find("\"keybindautorod\"") != std::string::npos, "modern should advertise Auto Rod keybind setting");
    ExpectTrue(legacy.find("\"inworld\"") != std::string::npos, "legacy should advertise in-world state");
    ExpectTrue(modern.find("\"inworld\"") != std::string::npos, "modern should advertise in-world state");
    ExpectTrue(legacy.find("\"state\":[") != std::string::npos, "legacy should include state array");
    ExpectTrue(modern.find("\"state\":[") != std::string::npos, "modern should include state array");
}

int main()
{
    TestReadsQuotedAndUnquotedValues();
    TestFallbacksOnMissingOrInvalidValues();
    TestAcceptsWhitespaceAroundNumbers();
    TestClampUtilities();
    TestCapabilitiesPayloads();

    if (g_failures != 0) {
        std::cerr << "Native config-reader tests failed: " << g_failures << std::endl;
        return 1;
    }

    std::cout << "Native config-reader tests passed." << std::endl;
    return 0;
}
