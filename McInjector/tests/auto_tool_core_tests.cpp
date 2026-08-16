#include <iostream>
#include "../src/main/cpp/auto_tool_core.h"

static int g_failures = 0;

static void ExpectEq(int expected, int actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected
                  << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void ExpectBool(bool expected, bool actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << (expected ? "true" : "false")
                  << " actual=" << (actual ? "true" : "false") << std::endl;
        ++g_failures;
    }
}

int main()
{
    autotool::AutoToolConfig cfg;
    cfg.enabled = true;
    cfg.swapWeapon = true;
    cfg.instantSwap = true;
    cfg.swapToDelayMs = 50;
    cfg.swapBack = true;
    cfg.swapBackDelayMs = 200;
    cfg.requireMouseDown = true;
    cfg.onlyWhileSneaking = false;

    autotool::AutoToolState state;

    // Test 1: Disabled / GUI open / Not in world -> No action
    autotool::AutoToolInput input;
    input.nowMs = 1000;
    input.inWorld = true;
    input.guiOpen = true;
    input.mouseDown = true;
    input.currentSlot = 0;
    input.isBlockHit = true;
    input.bestToolSlot = 2;

    autotool::AutoToolAction act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(false, act.switchSlot, "GuiOpen suppresses tool switch");

    // Test 2: Hovering block with mouseDown=false when requireMouseDown=true -> No action
    input.guiOpen = false;
    input.mouseDown = false;
    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(false, act.switchSlot, "RequireMouseDown suppresses tool switch when not clicked");

    // Test 3: Hovering block with mouseDown=true -> pending swap starts, switches after swapToDelayMs
    input.mouseDown = true;
    input.nowMs = 1000;
    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(false, act.switchSlot, "Swap delay waits before switching");
    ExpectBool(true, state.swapPending, "Swap is marked pending");

    input.nowMs = 1050; // 50ms elapsed
    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(true, act.switchSlot, "Switches to tool after delay");
    ExpectEq(2, act.targetSlot, "Switches to bestToolSlot (2)");
    ExpectEq(0, state.originalSlot, "Stores originalSlot (0)");
    ExpectBool(true, state.swapped, "State marked as swapped");

    // Test 4: Continuing to mine on slot 2 -> No action needed
    input.currentSlot = 2;
    input.nowMs = 1100;
    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(false, act.switchSlot, "Already on best tool slot");

    // Test 5: Releasing mouse / block broken (mouseDown=false) -> swapBackPending starts
    input.mouseDown = false;
    input.nowMs = 1200;
    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(false, act.switchSlot, "Swap back delay starts");
    ExpectBool(true, state.swapBackPending, "swapBackPending is true");

    input.nowMs = 1350; // 150ms elapsed (< 200ms)
    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(false, act.switchSlot, "Swap back waiting for delay");

    input.nowMs = 1400; // 200ms elapsed
    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(true, act.switchSlot, "Swaps back after swapBackDelayMs");
    ExpectEq(0, act.targetSlot, "Swaps back to originalSlot (0)");
    ExpectBool(false, state.swapped, "swapped flag cleared");
    ExpectEq(-1, state.originalSlot, "originalSlot cleared");

    // Test 6: Entity hit with instantSwap=true -> swaps immediately to weapon without delay
    state.Reset();
    input.currentSlot = 3;
    input.mouseDown = true;
    input.isBlockHit = false;
    input.isEntityHit = true;
    input.bestWeaponSlot = 1;
    input.nowMs = 2000;

    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(true, act.switchSlot, "Instant swap switches to weapon immediately");
    ExpectEq(1, act.targetSlot, "Target slot is weapon slot (1)");
    ExpectEq(3, state.originalSlot, "Original slot saved as 3");

    // Test 7: onlyWhileSneaking check
    state.Reset();
    cfg.onlyWhileSneaking = true;
    input.isSneaking = false;
    input.isBlockHit = true;
    input.bestToolSlot = 4;
    input.nowMs = 3000;

    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(false, act.switchSlot, "onlyWhileSneaking blocks swap when not sneaking");

    input.isSneaking = true;
    input.nowMs = 3050;
    act = autotool::UpdateAutoToolState(state, cfg, input);
    ExpectBool(true, act.switchSlot, "onlyWhileSneaking allows swap when sneaking");
    ExpectEq(4, act.targetSlot, "Swaps to slot 4");

    if (g_failures != 0) {
        std::cerr << "Auto Tool core tests failed: " << g_failures << std::endl;
        return 1;
    }
    std::cout << "Auto Tool core tests passed." << std::endl;
    return 0;
}
