#include <iostream>
#include "../src/main/cpp/nick_hider.h"
#include "../src/main/cpp/nick_hider_jvmti_targets.h"

static int failures = 0;

static void ExpectEq(const std::string& expected, const std::string& actual, const char* label)
{
    if (expected == actual) return;
    std::cerr << "FAIL " << label << ": expected '" << expected << "', got '" << actual << "'\n";
    ++failures;
}

int main()
{
    using namespace lc;
    ExpectEq("Alias joined Alias", ReplaceNickHiderText("Steve joined Steve", true, "Steve", "Alias"), "repeated matches");
    ExpectEq("Steve_2 SteveX (Alias)", ReplaceNickHiderText("Steve_2 SteveX (Steve)", true, "Steve", "Alias"), "name boundaries");
    ExpectEq("\xC2\xA7" "aAlias" "\xC2\xA7" "r", ReplaceNickHiderText("\xC2\xA7" "aSteve" "\xC2\xA7" "r", true, "Steve", "Alias"), "style boundaries");
    ExpectEq("Steve", ReplaceNickHiderText("Steve", false, "Steve", "Alias"), "disabled");
    ExpectEq("Steve", ReplaceNickHiderText("Steve", true, "Steve", ""), "empty alias");
    ExpectEq("Żółw", NormalizeNickHiderAlias("  Żółw  "), "utf8 alias");
    ExpectEq("", NormalizeNickHiderAlias("bad\nname"), "control rejected");
    ExpectEq("", NormalizeNickHiderAlias("\xC2\xA7" "aBlue"), "formatting rejected");
    const char* legacyChat = NickHiderJvmtiSurfaceForSignature(false, "Lbct;");
    const char* modernHud = NickHiderJvmtiSurfaceForSignature(true, "Lnet/minecraft/class_329;");
    const char* inventory = NickHiderJvmtiSurfaceForSignature(true, "Lnet/minecraft/client/gui/screens/inventory/InventoryScreen;");
    ExpectEq("chat", legacyChat ? legacyChat : "", "legacy chat target");
    ExpectEq("hud", modernHud ? modernHud : "", "modern hud target");
    ExpectEq("", inventory ? inventory : "", "menu excluded");
    if (!IsNickHiderJvmtiRendererSignature(false, "Lbfr;") || !IsNickHiderJvmtiRendererSignature(true, "Lnet/minecraft/class_327;")) {
        std::cerr << "FAIL renderer target tables\n";
        ++failures;
    }

    if (failures == 0) std::cout << "Nick hider helper tests passed.\n";
    return failures == 0 ? 0 : 1;
}
