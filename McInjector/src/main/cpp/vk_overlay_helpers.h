#pragma once

// Pure, renderer-agnostic helpers for the Vulkan overlay backend.
// Deliberately free of Vulkan/Windows headers so they can be unit-tested in the
// native harness. Values that mirror Vulkan enums are passed as plain integers.

#include <stdint.h>

namespace vkoverlay {

// Mirrors VK_QUEUE_GRAPHICS_BIT.
static const uint32_t kQueueGraphicsBit = 0x00000001u;

// Sentinel for "no suitable queue family".
static const uint32_t kInvalidFamily = 0xFFFFFFFFu;

// Pick a queue family that (a) supports graphics and (b) was among the families the
// logical device actually created queues for (we can only vkGetDeviceQueue on those).
// Prefers a requested + graphics-capable family. If the requested list is empty/unknown,
// falls back to the first graphics-capable family. Returns kInvalidFamily on failure.
inline uint32_t SelectGraphicsQueueFamily(
        const uint32_t* familyQueueFlags, uint32_t familyCount,
        const uint32_t* requestedFamilies, uint32_t requestedCount)
{
    if (!familyQueueFlags || familyCount == 0)
        return kInvalidFamily;

    for (uint32_t r = 0; r < requestedCount; ++r) {
        const uint32_t fam = requestedFamilies ? requestedFamilies[r] : r;
        if (fam < familyCount && (familyQueueFlags[fam] & kQueueGraphicsBit) != 0u)
            return fam;
    }

    if (requestedCount == 0) {
        for (uint32_t f = 0; f < familyCount; ++f)
            if ((familyQueueFlags[f] & kQueueGraphicsBit) != 0u)
                return f;
    }
    return kInvalidFamily;
}

// Identity of a swapchain's resource-affecting properties. When any field changes we must
// rebuild the per-image Vulkan resources (image views, framebuffers, command buffers, ...).
struct SwapchainKey {
    uint64_t handle;      // VkSwapchainKHR as an opaque handle value
    uint32_t width;
    uint32_t height;
    int32_t  format;      // VkFormat
    uint32_t imageCount;

    SwapchainKey() : handle(0), width(0), height(0), format(0), imageCount(0) {}
    SwapchainKey(uint64_t h, uint32_t w, uint32_t hgt, int32_t fmt, uint32_t ic)
        : handle(h), width(w), height(hgt), format(fmt), imageCount(ic) {}
};

inline bool SwapchainKeyEqual(const SwapchainKey& a, const SwapchainKey& b) {
    return a.handle == b.handle && a.width == b.width && a.height == b.height &&
           a.format == b.format && a.imageCount == b.imageCount;
}

// True when 'b' requires a resource rebuild relative to the currently-built 'a'.
inline bool SwapchainNeedsRebuild(const SwapchainKey& built, const SwapchainKey& incoming) {
    return !SwapchainKeyEqual(built, incoming);
}

// True when only the format changed between two keys (requires render-pass + pipeline rebuild,
// not just framebuffers). Assumes a rebuild is already known to be needed.
inline bool SwapchainFormatChanged(const SwapchainKey& built, const SwapchainKey& incoming) {
    return built.format != incoming.format;
}

// Dear ImGui's Vulkan backend requires MinImageCount >= 2.
inline uint32_t NormalizeMinImageCount(uint32_t imageCount) {
    return imageCount < 2u ? 2u : imageCount;
}

// Backend arbitration: the first non-Unknown present path to fire owns the session, and
// keeps ownership thereafter (no flip-flopping). Values mirror GfxBackendKind:
//   0 = Unknown, 1 = OpenGL, 2 = Vulkan.
// Returns the backend that owns the session after observing 'incoming'.
inline int ArbitrateBackend(int current, int incoming) {
    if (incoming == 0) return current;          // an Unknown observation changes nothing
    return current == 0 ? incoming : current;   // first claim wins; later claims are ignored
}

} // namespace vkoverlay
