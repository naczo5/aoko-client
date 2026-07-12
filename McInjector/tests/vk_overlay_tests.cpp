// Native harness tests for the Vulkan overlay pure helpers (vk_overlay_helpers.h):
//   - graphics queue-family selection across NVIDIA / AMD / Intel-style layouts
//   - swapchain resource-rebuild / format-change detection
//   - min image count normalization
//   - backend arbitration state machine (first present wins)
//
// Built and run by run_tests.bat. Standalone C++11, no Vulkan/Windows headers required.

#include <iostream>
#include <stdint.h>

#include "../src/main/cpp/vk_overlay_helpers.h"

using namespace vkoverlay;

static int g_failures = 0;

static void ExpectEqU(uint32_t expected, uint32_t actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void ExpectEqI(int expected, int actual, const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message << " expected=" << expected << " actual=" << actual << std::endl;
        ++g_failures;
    }
}

static void ExpectTrue(bool cond, const char* message)
{
    if (!cond) { std::cerr << "FAIL: " << message << std::endl; ++g_failures; }
}

// Queue flag bit constants (mirror VkQueueFlagBits).
static const uint32_t GFX = 0x1;   // VK_QUEUE_GRAPHICS_BIT
static const uint32_t CMP = 0x2;   // VK_QUEUE_COMPUTE_BIT
static const uint32_t XFR = 0x4;   // VK_QUEUE_TRANSFER_BIT

static void TestQueueFamilySelection()
{
    // NVIDIA-like: family 0 is graphics+compute+transfer, device requested family 0.
    {
        uint32_t flags[] = { GFX | CMP | XFR, XFR };
        uint32_t requested[] = { 0 };
        ExpectEqU(0u, SelectGraphicsQueueFamily(flags, 2, requested, 1), "nvidia family0 graphics");
    }
    // AMD-like: family 0 graphics, separate async-compute (1) and transfer (2). Requested {0,1,2}.
    {
        uint32_t flags[] = { GFX | CMP | XFR, CMP, XFR };
        uint32_t requested[] = { 0, 1, 2 };
        ExpectEqU(0u, SelectGraphicsQueueFamily(flags, 3, requested, 3), "amd picks graphics family0");
    }
    // Intel-like: single universal queue family, requested {0}.
    {
        uint32_t flags[] = { GFX | CMP | XFR };
        uint32_t requested[] = { 0 };
        ExpectEqU(0u, SelectGraphicsQueueFamily(flags, 1, requested, 1), "intel single family");
    }
    // Requested list leads with a compute-only family; must skip to the graphics-capable one.
    {
        uint32_t flags[] = { CMP, GFX | CMP };
        uint32_t requested[] = { 0, 1 };
        ExpectEqU(1u, SelectGraphicsQueueFamily(flags, 2, requested, 2), "skip compute-only requested family");
    }
    // Requested family is not graphics-capable and no other requested family qualifies.
    {
        uint32_t flags[] = { CMP, XFR };
        uint32_t requested[] = { 0 };
        ExpectEqU(kInvalidFamily, SelectGraphicsQueueFamily(flags, 2, requested, 1), "no graphics among requested");
    }
    // Unknown requested list (count 0): fall back to first graphics-capable family.
    {
        uint32_t flags[] = { XFR, GFX | CMP };
        ExpectEqU(1u, SelectGraphicsQueueFamily(flags, 2, nullptr, 0), "fallback first graphics family");
    }
    // Degenerate inputs.
    ExpectEqU(kInvalidFamily, SelectGraphicsQueueFamily(nullptr, 0, nullptr, 0), "no families -> invalid");
}

static void TestSwapchainRebuildDetection()
{
    SwapchainKey a((uint64_t)0x1000, 1920, 1080, 44 /*B8G8R8A8_UNORM-ish*/, 3);

    // Identical -> no rebuild.
    ExpectTrue(!SwapchainNeedsRebuild(a, SwapchainKey((uint64_t)0x1000, 1920, 1080, 44, 3)),
               "identical swapchain no rebuild");

    // New handle (recreated swapchain) -> rebuild, format unchanged.
    {
        SwapchainKey b((uint64_t)0x2000, 1920, 1080, 44, 3);
        ExpectTrue(SwapchainNeedsRebuild(a, b), "new handle rebuild");
        ExpectTrue(!SwapchainFormatChanged(a, b), "new handle same format");
    }
    // Resize -> rebuild, format unchanged.
    {
        SwapchainKey b((uint64_t)0x1000, 1280, 720, 44, 2);
        ExpectTrue(SwapchainNeedsRebuild(a, b), "resize rebuild");
        ExpectTrue(!SwapchainFormatChanged(a, b), "resize same format");
    }
    // Format change -> rebuild AND format changed (needs render pass + pipeline rebuild).
    {
        SwapchainKey b((uint64_t)0x1000, 1920, 1080, 50, 3);
        ExpectTrue(SwapchainNeedsRebuild(a, b), "format change rebuild");
        ExpectTrue(SwapchainFormatChanged(a, b), "format change flagged");
    }
}

static void TestMinImageCount()
{
    ExpectEqU(2u, NormalizeMinImageCount(0), "min image count floor 0->2");
    ExpectEqU(2u, NormalizeMinImageCount(1), "min image count floor 1->2");
    ExpectEqU(2u, NormalizeMinImageCount(2), "min image count 2");
    ExpectEqU(3u, NormalizeMinImageCount(3), "min image count 3");
}

static void TestBackendArbitration()
{
    // 0=Unknown, 1=OpenGL, 2=Vulkan.
    ExpectEqI(1, ArbitrateBackend(0, 1), "unknown -> opengl claims");
    ExpectEqI(2, ArbitrateBackend(0, 2), "unknown -> vulkan claims");

    // First claim wins; subsequent different claims are ignored.
    ExpectEqI(1, ArbitrateBackend(1, 2), "opengl owns, vulkan ignored");
    ExpectEqI(2, ArbitrateBackend(2, 1), "vulkan owns, opengl ignored");

    // Unknown observations never change ownership.
    ExpectEqI(0, ArbitrateBackend(0, 0), "unknown stays unknown");
    ExpectEqI(2, ArbitrateBackend(2, 0), "vulkan stays on unknown observation");

    // Idempotent re-claim by the same backend.
    ExpectEqI(1, ArbitrateBackend(1, 1), "opengl re-claim idempotent");
}

int main()
{
    TestQueueFamilySelection();
    TestSwapchainRebuildDetection();
    TestMinImageCount();
    TestBackendArbitration();

    if (g_failures == 0) {
        std::cout << "vk_overlay_tests: all assertions passed." << std::endl;
        return 0;
    }
    std::cerr << "vk_overlay_tests: " << g_failures << " failure(s)." << std::endl;
    return 1;
}
