// visuals_config_tests.cpp
// Pure checks for the settings, id and status helpers shared by the bridges.
#include "../src/main/cpp/visuals_config.h"

#include <cstdio>
#include <string>

static int g_failures = 0;
static int g_checks = 0;

#define CHECK(cond) do { ++g_checks; if (!(cond)) { ++g_failures; std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #cond); } } while (0)

int main() {
    using namespace visuals;
    CHECK(ValidId("aoko-glass") && ValidId("a") && ValidId("0day_x") && ValidId(std::string(32, 'a')));
    CHECK(!ValidId("") && !ValidId("-x") && !ValidId("_x") && !ValidId("Aoko") && !ValidId("a b") && !ValidId("../x") &&
          !ValidId("a/b") && !ValidId("a\\b") && !ValidId(std::string(33, 'a')) && !ValidId("a.b") && !ValidId(std::string("a\0b", 3)));
    CHECK(CleanId("good") == "good" && CleanId("Bad") == "");

    CHECK(ClampSkyTime(-7) == -2 && ClampSkyTime(-2) == -2 && ClampSkyTime(-1) == -1 && ClampSkyTime(5) == 5 && ClampSkyTime(99999) == 23999);
    CHECK(ValidDir("C:\\Users\\a\\AppData\\Roaming\\Aoko\\visuals") && !ValidDir("") && !ValidDir(std::string(1025, 'x')) && !ValidDir("a\nb"));

    CHECK(JsonEscape("a\"b\\c\n") == "\"a\\\"b\\\\c\\u000a\"");
    // Non-ASCII leaves as \\uXXXX; supplementary characters as surrogate pairs; bad UTF-8 as U+FFFD.
    CHECK(JsonEscape("Jos\xC3\xA9") == "\"Jos\\u00e9\"");
    CHECK(JsonEscape("\xE2\x82\xAC") == "\"\\u20ac\"");
    CHECK(JsonEscape("\xF0\x9F\x98\x80") == "\"\\ud83d\\ude00\"");
    CHECK(JsonEscape("a\xFFz") == "\"a\\ufffdz\"");
    CHECK(JsonEscape("\xC3") == "\"\\ufffd\"");
    CHECK(JsonEscape("\xC0\xAF") == "\"\\ufffd\\ufffd\"");
    CHECK(JsonEscape("C:\\x") == "\"C:\\\\x\"");

    // Decoding what System.Text.Json wrote for a Windows path with a non-ASCII user name.
    std::string decoded;
    CHECK(UnescapeJsonString("C:\\\\Users\\\\Jos\\u00e9\\\\visuals", decoded) && decoded == "C:\\Users\\Jos\xC3\xA9\\visuals");
    CHECK(UnescapeJsonString("\\ud83d\\ude00", decoded) && decoded == "\xF0\x9F\x98\x80");
    CHECK(UnescapeJsonString("a\\/b\\\"c\\n", decoded) && decoded == "a/b\"c\n");
    CHECK(!UnescapeJsonString("\\ud83d", decoded) && !UnescapeJsonString("\\ud83dx", decoded) && !UnescapeJsonString("\\ude00", decoded));
    CHECK(!UnescapeJsonString("\\u12", decoded) && !UnescapeJsonString("\\uZZZZ", decoded) && !UnescapeJsonString("\\q", decoded) && !UnescapeJsonString("\\", decoded));
    CHECK(!UnescapeJsonString("bad\x01", decoded));
    CHECK(UnescapeJsonString("", decoded) && decoded.empty());
    // Round trip.
    const std::string path = "C:\\Users\\Jos\xC3\xA9 \xF0\x9F\x98\x80\\Aoko";
    const std::string esc = JsonEscape(path);
    CHECK(UnescapeJsonString(esc.substr(1, esc.size() - 2), decoded) && decoded == path);

    Settings s;
    CHECK(!s.Wanted());
    s.cosmeticsEnabled = true;
    s.cape = "aoko-glass";
    s.hat = "Bad Id";
    s.back = "angel-wings";
    s.skyEnabled = true;
    s.skyPreset = "tsukihime";
    s.skyTime = 99999;
    s.skyClearWeather = true;
    s.dir = "C:\\Users\\a\\visuals";
    CHECK(s.Wanted());
    const std::string json = BuildConfigJson(s);
    CHECK(json == "{\"cosmetics\":true,\"cape\":\"aoko-glass\",\"hat\":\"\",\"back\":\"angel-wings\",\"sky\":true,"
                  "\"preset\":\"tsukihime\",\"time\":23999,\"clear\":true,\"dir\":\"C:\\\\Users\\\\a\\\\visuals\"}");
    Settings t = s;
    CHECK(t == s);
    t.skyTime = -1;
    CHECK(t != s);
    Settings off;
    CHECK(BuildConfigJson(off) == "{\"cosmetics\":false,\"cape\":\"\",\"hat\":\"\",\"back\":\"\",\"sky\":false,\"preset\":\"\",\"time\":-2,\"clear\":false,\"dir\":\"\"}");

    CHECK(SanitizeStatus("ok") == "ok");
    CHECK(SanitizeStatus("partial:cape 'x' not found; sky (a/b)") == "partial:cape 'x' not found; sky (a/b)");
    CHECK(SanitizeStatus("error:bad \"quote\" \\ back\nline") == "error:bad _quote_ _ back_line");
    CHECK(SanitizeStatus("") == "off");
    CHECK(SanitizeStatus(std::string(500, 'a')).size() == 220);

    if (g_failures != 0) { std::printf("%d of %d checks failed\n", g_failures, g_checks); return 1; }
    std::printf("visuals_config_tests OK (%d checks)\n", g_checks);
    return 0;
}
