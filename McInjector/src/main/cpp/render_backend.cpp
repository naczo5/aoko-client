// render_backend.cpp — Vulkan overlay backend for bridge_261.dll (Lunar 26.2+).
//
// The game may present frames through OpenGL (wglSwapBuffers, hooked in bridge_261.cpp)
// or Vulkan (vulkan-1.dll). We auto-detect at runtime: whichever present path fires
// first owns the overlay for the session.
//
// IMPORTANT hooking note:
// Minecraft/Lunar (via LWJGL) resolves device-level Vulkan functions through
// vkGetDeviceProcAddr and caches the pointers. Those pointers are the loader's *dispatch
// trampolines*, which are NOT the same addresses as the symbols exported by vulkan-1.dll.
// Therefore hooking GetProcAddress(vulkan-1.dll, "vkQueuePresentKHR") never intercepts the
// game's calls. Instead we create a throwaway instance+device, resolve the trampoline
// addresses via vkGetDeviceProcAddr, and hook THOSE — they are shared process-wide, so they
// catch the game's cached calls even when we inject after the device already exists.
//
// Because injection typically happens after the device/swapchain are created, we recover the
// live objects at runtime: vkAcquireNextImageKHR gives us the VkDevice + VkSwapchainKHR every
// frame, vkQueuePresentKHR gives us the VkQueue, and vkCreateSwapchainKHR (fires on the next
// swapchain recreation) gives authoritative format + extent. Physical device + graphics queue
// family come from our throwaway instance (same GPU => consistent memory types).

#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>

#include "render_backend.h"
#include "vk_overlay_helpers.h"

#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <vector>
#include <string>

#include "MinHook.h"
#include "imgui.h"
#include "imgui_impl_win32.h"
#include "imgui_impl_vulkan.h"

// ============================================================================
// Logging / config
// ============================================================================

static RenderBackendLogFn g_logFn = nullptr;

static void RbLog(const char* msg) { if (g_logFn) g_logFn(msg); }
static void RbLog(const std::string& msg) { if (g_logFn) g_logFn(msg.c_str()); }

void RenderBackend_SetLogFn(RenderBackendLogFn fn) { g_logFn = fn; }

// Auto-detect is the default: the Vulkan hooks are armed unless explicitly disabled via
// AOKO_BRIDGE261_VULKAN=0. Arming is harmless on pure-OpenGL clients — backend arbitration
// makes the OpenGL present path win and the Vulkan present hook bails. The env is retained as
// a field kill-switch.
static bool IsVulkanHookEnabled() {
    static int cached = -1;
    if (cached >= 0) return cached != 0;
    char buf[8] = {};
    DWORD n = GetEnvironmentVariableA("AOKO_BRIDGE261_VULKAN", buf, sizeof(buf));
    cached = (n > 0 && buf[0] == '0') ? 0 : 1;  // default enabled; only "0" disables
    return cached != 0;
}

// ============================================================================
// Backend arbitration (first present wins) — implements vkoverlay::ArbitrateBackend.
// ============================================================================

static volatile LONG g_activeBackend = (LONG)GfxBackendKind::Unknown;

GfxBackendKind RenderBackend_GetActiveKind() {
    return (GfxBackendKind)InterlockedCompareExchange(&g_activeBackend, 0, 0);
}

static bool ClaimBackend(GfxBackendKind kind) {
    LONG prev = InterlockedCompareExchange(&g_activeBackend, (LONG)kind, (LONG)GfxBackendKind::Unknown);
    return prev == (LONG)GfxBackendKind::Unknown || prev == (LONG)kind;
}

void RenderBackend_NotifyOpenGlFrame(HWND /*hwnd*/) {
    if (ClaimBackend(GfxBackendKind::OpenGL)) {
        static bool logged = false;
        if (!logged) { logged = true; RbLog("Active render backend claimed: OpenGL (wglSwapBuffers)."); }
    }
}

// ============================================================================
// Dynamically-loaded Vulkan entry points
// ============================================================================

static HMODULE g_vulkanDll = nullptr;
static PFN_vkGetInstanceProcAddr g_vkGetInstanceProcAddr = nullptr;
static PFN_vkGetDeviceProcAddr   g_vkGetDeviceProcAddr = nullptr;

// Instance-level (resolved via the throwaway instance)
static PFN_vkGetPhysicalDeviceQueueFamilyProperties p_vkGetPhysicalDeviceQueueFamilyProperties = nullptr;

// Device-level (resolved via the game's device once captured)
static PFN_vkGetDeviceQueue        p_vkGetDeviceQueue = nullptr;
static PFN_vkGetSwapchainImagesKHR p_vkGetSwapchainImagesKHR = nullptr;
static PFN_vkCreateImageView       p_vkCreateImageView = nullptr;
static PFN_vkDestroyImageView      p_vkDestroyImageView = nullptr;
static PFN_vkCreateRenderPass      p_vkCreateRenderPass = nullptr;
static PFN_vkDestroyRenderPass     p_vkDestroyRenderPass = nullptr;
static PFN_vkCreateFramebuffer     p_vkCreateFramebuffer = nullptr;
static PFN_vkDestroyFramebuffer    p_vkDestroyFramebuffer = nullptr;
static PFN_vkCreateCommandPool     p_vkCreateCommandPool = nullptr;
static PFN_vkDestroyCommandPool    p_vkDestroyCommandPool = nullptr;
static PFN_vkAllocateCommandBuffers p_vkAllocateCommandBuffers = nullptr;
static PFN_vkFreeCommandBuffers    p_vkFreeCommandBuffers = nullptr;
static PFN_vkCreateSemaphore       p_vkCreateSemaphore = nullptr;
static PFN_vkDestroySemaphore      p_vkDestroySemaphore = nullptr;
static PFN_vkCreateFence           p_vkCreateFence = nullptr;
static PFN_vkDestroyFence          p_vkDestroyFence = nullptr;
static PFN_vkWaitForFences         p_vkWaitForFences = nullptr;
static PFN_vkResetFences           p_vkResetFences = nullptr;
static PFN_vkResetCommandBuffer    p_vkResetCommandBuffer = nullptr;
static PFN_vkBeginCommandBuffer    p_vkBeginCommandBuffer = nullptr;
static PFN_vkEndCommandBuffer      p_vkEndCommandBuffer = nullptr;
static PFN_vkCmdBeginRenderPass    p_vkCmdBeginRenderPass = nullptr;
static PFN_vkCmdEndRenderPass      p_vkCmdEndRenderPass = nullptr;
static PFN_vkQueueSubmit           p_vkQueueSubmit = nullptr;
static PFN_vkDeviceWaitIdle        p_vkDeviceWaitIdle = nullptr;

// Hooked dispatch trampolines (originals)
static PFN_vkQueuePresentKHR     o_vkQueuePresentKHR = nullptr;
static PFN_vkAcquireNextImageKHR o_vkAcquireNextImageKHR = nullptr;
static PFN_vkCreateSwapchainKHR  o_vkCreateSwapchainKHR = nullptr;
static PFN_vkDestroySwapchainKHR o_vkDestroySwapchainKHR = nullptr;
static PFN_vkDestroyDevice       o_vkDestroyDevice = nullptr;

static const int kMaxHooks = 5;
static void* g_hookTargets[kMaxHooks] = {};

// ============================================================================
// Captured Vulkan state + overlay resources
// ============================================================================

static CRITICAL_SECTION g_vkLock;
static bool g_vkLockInit = false;

struct VkLockGuard {
    VkLockGuard()  { if (g_vkLockInit) EnterCriticalSection(&g_vkLock); }
    ~VkLockGuard() { if (g_vkLockInit) LeaveCriticalSection(&g_vkLock); }
};

// From the throwaway instance/device (kept alive for the session).
static VkInstance       g_instance = VK_NULL_HANDLE;        // dummy instance (memory-type queries)
static uint32_t         g_instanceApiVersion = 0;
static VkPhysicalDevice g_physicalDevice = VK_NULL_HANDLE;  // dummy physical device (same GPU)
static uint32_t         g_queueFamily = vkoverlay::kInvalidFamily;
static VkDevice         g_dummyDevice = VK_NULL_HANDLE;      // destroyed after resolving trampolines

// Captured live from the game.
static VkDevice         g_device = VK_NULL_HANDLE;
static VkQueue          g_queue = VK_NULL_HANDLE;
static VkSwapchainKHR   g_swapchain = VK_NULL_HANDLE;
static bool             g_deviceFuncsLoaded = false;

static VkFormat   g_swapFormat = VK_FORMAT_UNDEFINED;
static bool       g_swapFormatAuthoritative = false;   // true once vkCreateSwapchainKHR observed
static VkExtent2D g_swapExtent = {0, 0};
static bool       g_swapExtentKnown = false;

static VkRenderPass g_renderPass = VK_NULL_HANDLE;
static std::vector<VkImage>         g_images;
static std::vector<VkImageView>     g_imageViews;
static std::vector<VkFramebuffer>   g_framebuffers;
static VkCommandPool                g_cmdPool = VK_NULL_HANDLE;
static std::vector<VkCommandBuffer> g_cmdBuffers;
static std::vector<VkFence>         g_fences;
static std::vector<VkSemaphore>     g_renderCompleteSems;
static vkoverlay::SwapchainKey      g_builtKey;

static bool g_imguiVulkanInit = false;
static bool g_diagPresent = false, g_diagAcquire = false, g_diagCreateSc = false, g_diagBuilt = false;

// ============================================================================
// Loader helpers
// ============================================================================

static PFN_vkVoidFunction VKAPI_PTR ImGuiVkLoader(const char* name, void* user) {
    (void)user;
    if (!g_vkGetInstanceProcAddr) return nullptr;
    return g_vkGetInstanceProcAddr(g_instance, name);
}

static bool LoadDeviceFuncs(VkDevice device) {
    if (!g_vkGetDeviceProcAddr || device == VK_NULL_HANDLE) return false;
#define LOAD_DEV(f) p_##f = (PFN_##f)g_vkGetDeviceProcAddr(device, #f); if (!p_##f) { RbLog("Vulkan: failed to load device function " #f); return false; }
    LOAD_DEV(vkGetDeviceQueue);
    LOAD_DEV(vkGetSwapchainImagesKHR);
    LOAD_DEV(vkCreateImageView);
    LOAD_DEV(vkDestroyImageView);
    LOAD_DEV(vkCreateRenderPass);
    LOAD_DEV(vkDestroyRenderPass);
    LOAD_DEV(vkCreateFramebuffer);
    LOAD_DEV(vkDestroyFramebuffer);
    LOAD_DEV(vkCreateCommandPool);
    LOAD_DEV(vkDestroyCommandPool);
    LOAD_DEV(vkAllocateCommandBuffers);
    LOAD_DEV(vkFreeCommandBuffers);
    LOAD_DEV(vkCreateSemaphore);
    LOAD_DEV(vkDestroySemaphore);
    LOAD_DEV(vkCreateFence);
    LOAD_DEV(vkDestroyFence);
    LOAD_DEV(vkWaitForFences);
    LOAD_DEV(vkResetFences);
    LOAD_DEV(vkResetCommandBuffer);
    LOAD_DEV(vkBeginCommandBuffer);
    LOAD_DEV(vkEndCommandBuffer);
    LOAD_DEV(vkCmdBeginRenderPass);
    LOAD_DEV(vkCmdEndRenderPass);
    LOAD_DEV(vkQueueSubmit);
    LOAD_DEV(vkDeviceWaitIdle);
#undef LOAD_DEV
    return true;
}

// ============================================================================
// Swapchain resource build / destroy
// ============================================================================

static void DestroySwapchainResources() {
    if (g_device == VK_NULL_HANDLE) return;
    if (p_vkDeviceWaitIdle) p_vkDeviceWaitIdle(g_device);

    for (VkFramebuffer fb : g_framebuffers) if (fb && p_vkDestroyFramebuffer) p_vkDestroyFramebuffer(g_device, fb, nullptr);
    g_framebuffers.clear();
    for (VkImageView iv : g_imageViews) if (iv && p_vkDestroyImageView) p_vkDestroyImageView(g_device, iv, nullptr);
    g_imageViews.clear();
    for (VkFence f : g_fences) if (f && p_vkDestroyFence) p_vkDestroyFence(g_device, f, nullptr);
    g_fences.clear();
    for (VkSemaphore s : g_renderCompleteSems) if (s && p_vkDestroySemaphore) p_vkDestroySemaphore(g_device, s, nullptr);
    g_renderCompleteSems.clear();
    if (!g_cmdBuffers.empty() && g_cmdPool && p_vkFreeCommandBuffers)
        p_vkFreeCommandBuffers(g_device, g_cmdPool, (uint32_t)g_cmdBuffers.size(), g_cmdBuffers.data());
    g_cmdBuffers.clear();
    if (g_cmdPool && p_vkDestroyCommandPool) { p_vkDestroyCommandPool(g_device, g_cmdPool, nullptr); g_cmdPool = VK_NULL_HANDLE; }
    g_images.clear();
    g_builtKey = vkoverlay::SwapchainKey();
}

static bool CreateRenderPass(VkFormat format) {
    if (g_renderPass != VK_NULL_HANDLE && p_vkDestroyRenderPass) {
        p_vkDestroyRenderPass(g_device, g_renderPass, nullptr);
        g_renderPass = VK_NULL_HANDLE;
    }
    VkAttachmentDescription color = {};
    color.format = format;
    color.samples = VK_SAMPLE_COUNT_1_BIT;
    color.loadOp = VK_ATTACHMENT_LOAD_OP_LOAD;
    color.storeOp = VK_ATTACHMENT_STORE_OP_STORE;
    color.stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE;
    color.stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE;
    color.initialLayout = VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;
    color.finalLayout = VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;

    VkAttachmentReference colorRef = {};
    colorRef.attachment = 0;
    colorRef.layout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;

    VkSubpassDescription subpass = {};
    subpass.pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS;
    subpass.colorAttachmentCount = 1;
    subpass.pColorAttachments = &colorRef;

    VkSubpassDependency dep = {};
    dep.srcSubpass = VK_SUBPASS_EXTERNAL;
    dep.dstSubpass = 0;
    dep.srcStageMask = VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT;
    dep.dstStageMask = VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT;
    dep.srcAccessMask = 0;
    dep.dstAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT;

    VkRenderPassCreateInfo rp = {};
    rp.sType = VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO;
    rp.attachmentCount = 1;
    rp.pAttachments = &color;
    rp.subpassCount = 1;
    rp.pSubpasses = &subpass;
    rp.dependencyCount = 1;
    rp.pDependencies = &dep;
    return p_vkCreateRenderPass(g_device, &rp, nullptr, &g_renderPass) == VK_SUCCESS;
}

static bool BuildSwapchainResources(VkFormat format, VkExtent2D extent) {
    DestroySwapchainResources();

    uint32_t imageCount = 0;
    if (p_vkGetSwapchainImagesKHR(g_device, g_swapchain, &imageCount, nullptr) != VK_SUCCESS || imageCount == 0)
        return false;
    g_images.resize(imageCount);
    if (p_vkGetSwapchainImagesKHR(g_device, g_swapchain, &imageCount, g_images.data()) != VK_SUCCESS)
        return false;

    bool formatChanged = (g_swapFormat != format) || g_renderPass == VK_NULL_HANDLE;
    g_swapFormat = format;
    g_swapExtent = extent;
    if (formatChanged && !CreateRenderPass(format)) { RbLog("Vulkan: CreateRenderPass failed."); return false; }

    g_imageViews.resize(imageCount, VK_NULL_HANDLE);
    for (uint32_t i = 0; i < imageCount; ++i) {
        VkImageViewCreateInfo iv = {};
        iv.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
        iv.image = g_images[i];
        iv.viewType = VK_IMAGE_VIEW_TYPE_2D;
        iv.format = format;
        iv.components.r = VK_COMPONENT_SWIZZLE_R;
        iv.components.g = VK_COMPONENT_SWIZZLE_G;
        iv.components.b = VK_COMPONENT_SWIZZLE_B;
        iv.components.a = VK_COMPONENT_SWIZZLE_A;
        iv.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
        iv.subresourceRange.levelCount = 1;
        iv.subresourceRange.layerCount = 1;
        if (p_vkCreateImageView(g_device, &iv, nullptr, &g_imageViews[i]) != VK_SUCCESS) return false;
    }

    g_framebuffers.resize(imageCount, VK_NULL_HANDLE);
    for (uint32_t i = 0; i < imageCount; ++i) {
        VkFramebufferCreateInfo fb = {};
        fb.sType = VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO;
        fb.renderPass = g_renderPass;
        fb.attachmentCount = 1;
        fb.pAttachments = &g_imageViews[i];
        fb.width = extent.width;
        fb.height = extent.height;
        fb.layers = 1;
        if (p_vkCreateFramebuffer(g_device, &fb, nullptr, &g_framebuffers[i]) != VK_SUCCESS) return false;
    }

    VkCommandPoolCreateInfo cp = {};
    cp.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    cp.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    cp.queueFamilyIndex = g_queueFamily;
    if (p_vkCreateCommandPool(g_device, &cp, nullptr, &g_cmdPool) != VK_SUCCESS) return false;

    g_cmdBuffers.resize(imageCount, VK_NULL_HANDLE);
    VkCommandBufferAllocateInfo cba = {};
    cba.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    cba.commandPool = g_cmdPool;
    cba.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    cba.commandBufferCount = imageCount;
    if (p_vkAllocateCommandBuffers(g_device, &cba, g_cmdBuffers.data()) != VK_SUCCESS) return false;

    g_fences.resize(imageCount, VK_NULL_HANDLE);
    g_renderCompleteSems.resize(imageCount, VK_NULL_HANDLE);
    for (uint32_t i = 0; i < imageCount; ++i) {
        VkFenceCreateInfo fi = {};
        fi.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
        fi.flags = VK_FENCE_CREATE_SIGNALED_BIT;
        if (p_vkCreateFence(g_device, &fi, nullptr, &g_fences[i]) != VK_SUCCESS) return false;
        VkSemaphoreCreateInfo si = {};
        si.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;
        if (p_vkCreateSemaphore(g_device, &si, nullptr, &g_renderCompleteSems[i]) != VK_SUCCESS) return false;
    }

    g_builtKey = vkoverlay::SwapchainKey((uint64_t)g_swapchain, extent.width, extent.height, (int32_t)format, imageCount);
    if (!g_diagBuilt) {
        g_diagBuilt = true;
        char buf[192];
        _snprintf(buf, sizeof(buf), "Vulkan: overlay resources built (format=%d %ux%u images=%u authoritative=%d).",
                  (int)format, extent.width, extent.height, imageCount, g_swapFormatAuthoritative ? 1 : 0);
        RbLog(buf);
    }
    return true;
}

// ============================================================================
// Dear ImGui Vulkan backend init
// ============================================================================

static void ImGuiVkCheckResult(VkResult err) {
    if (err != VK_SUCCESS) { char b[64]; _snprintf(b, sizeof(b), "Vulkan: ImGui backend VkResult=%d", (int)err); RbLog(b); }
}

static bool InitImGuiVulkanIfNeeded() {
    if (g_imguiVulkanInit) return true;
    if (g_renderPass == VK_NULL_HANDLE || g_images.empty()) return false;

    if (!ImGui_ImplVulkan_LoadFunctions(g_instanceApiVersion, ImGuiVkLoader, nullptr)) {
        RbLog("Vulkan: ImGui_ImplVulkan_LoadFunctions failed.");
        return false;
    }
    uint32_t imageCount = (uint32_t)g_images.size();
    ImGui_ImplVulkan_InitInfo ii = {};
    ii.ApiVersion = g_instanceApiVersion;
    ii.Instance = g_instance;
    ii.PhysicalDevice = g_physicalDevice;
    ii.Device = g_device;
    ii.QueueFamily = g_queueFamily;
    ii.Queue = g_queue;
    ii.DescriptorPoolSize = IMGUI_IMPL_VULKAN_MINIMUM_IMAGE_SAMPLER_POOL_SIZE;
    ii.MinImageCount = vkoverlay::NormalizeMinImageCount(imageCount);
    ii.ImageCount = imageCount < ii.MinImageCount ? ii.MinImageCount : imageCount;
    ii.PipelineInfoMain.RenderPass = g_renderPass;
    ii.UseDynamicRendering = false;
    ii.CheckVkResultFn = ImGuiVkCheckResult;

    if (!ImGui_ImplVulkan_Init(&ii)) { RbLog("Vulkan: ImGui_ImplVulkan_Init failed."); return false; }
    g_imguiVulkanInit = true;
    RbLog("Vulkan: ImGui_ImplVulkan initialized (fonts upload lazily on first render).");
    return true;
}

static void ShutdownImGuiVulkan() {
    if (g_imguiVulkanInit) {
        if (p_vkDeviceWaitIdle && g_device) p_vkDeviceWaitIdle(g_device);
        ImGui_ImplVulkan_Shutdown();
        g_imguiVulkanInit = false;
    }
}

// ============================================================================
// GLFW window lookup + per-frame overlay render
// ============================================================================

static HWND FindGlfwGameWindow() {
    HWND found = nullptr;
    EnumWindows([](HWND hwnd, LPARAM lp) -> BOOL {
        char cls[256] = {};
        if (GetClassNameA(hwnd, cls, sizeof(cls)) && strcmp(cls, "GLFW30") == 0) { *(HWND*)lp = hwnd; return FALSE; }
        return TRUE;
    }, (LPARAM)&found);
    return found;
}

// Ensure overlay resources exist for the current device/swapchain. Requires device funcs,
// queue, physical device + family, and a known format/extent. Returns true when ready.
static bool EnsureOverlayResources(HWND hwnd) {
    if (g_device == VK_NULL_HANDLE || g_swapchain == VK_NULL_HANDLE ||
        g_queue == VK_NULL_HANDLE || g_physicalDevice == VK_NULL_HANDLE ||
        g_queueFamily == vkoverlay::kInvalidFamily || !g_deviceFuncsLoaded)
        return false;

    // Determine format: authoritative (from vkCreateSwapchainKHR) preferred, else a sane
    // Windows default (self-corrects when a real swapchain creation is later observed).
    VkFormat fmt = g_swapFormatAuthoritative ? g_swapFormat
                 : (g_swapFormat != VK_FORMAT_UNDEFINED ? g_swapFormat : VK_FORMAT_B8G8R8A8_UNORM);

    // Determine extent: authoritative from create-info, else the window client rect.
    VkExtent2D ext = g_swapExtent;
    if (!g_swapExtentKnown || ext.width == 0 || ext.height == 0) {
        RECT rc = {};
        if (hwnd && GetClientRect(hwnd, &rc)) {
            ext.width = (uint32_t)(rc.right - rc.left);
            ext.height = (uint32_t)(rc.bottom - rc.top);
        }
    }
    if (ext.width <= 1 || ext.height <= 1) return false;

    vkoverlay::SwapchainKey want((uint64_t)g_swapchain, ext.width, ext.height, (int32_t)fmt, g_builtKey.imageCount);
    // Rebuild if handle/format/extent differ from what we built (imageCount is filled by build).
    bool needBuild = g_framebuffers.empty() ||
                     g_builtKey.handle != (uint64_t)g_swapchain ||
                     g_builtKey.format != (int32_t)fmt ||
                     g_builtKey.width != ext.width || g_builtKey.height != ext.height;
    if (needBuild) {
        ShutdownImGuiVulkan();
        if (!BuildSwapchainResources(fmt, ext)) return false;
    }
    return InitImGuiVulkanIfNeeded();
}

static bool RenderOverlayIntoImage(HWND hwnd, uint32_t imageIndex,
                                   const VkSemaphore* appWaitSems, uint32_t appWaitCount,
                                   VkSemaphore* outSignal) {
    if (!Bridge_EnsureImGuiPlatform(hwnd)) return false;
    if (!EnsureOverlayResources(hwnd)) return false;
    if (imageIndex >= g_framebuffers.size()) return false;

    ImGui_ImplVulkan_NewFrame();
    ImDrawData* drawData = Bridge_BuildOverlayDrawData((int)g_swapExtent.width, (int)g_swapExtent.height);
    if (!drawData) return false;

    VkFence fence = g_fences[imageIndex];
    p_vkWaitForFences(g_device, 1, &fence, VK_TRUE, UINT64_MAX);
    p_vkResetFences(g_device, 1, &fence);

    VkCommandBuffer cmd = g_cmdBuffers[imageIndex];
    p_vkResetCommandBuffer(cmd, 0);

    VkCommandBufferBeginInfo bi = {};
    bi.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    bi.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    if (p_vkBeginCommandBuffer(cmd, &bi) != VK_SUCCESS) return false;

    VkRenderPassBeginInfo rp = {};
    rp.sType = VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO;
    rp.renderPass = g_renderPass;
    rp.framebuffer = g_framebuffers[imageIndex];
    rp.renderArea.extent = g_swapExtent;
    rp.clearValueCount = 0;
    p_vkCmdBeginRenderPass(cmd, &rp, VK_SUBPASS_CONTENTS_INLINE);

    ImGui_ImplVulkan_RenderDrawData(drawData, cmd, VK_NULL_HANDLE);

    p_vkCmdEndRenderPass(cmd);
    if (p_vkEndCommandBuffer(cmd) != VK_SUCCESS) return false;

    std::vector<VkPipelineStageFlags> waitStages(appWaitCount, VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT);
    VkSemaphore signal = g_renderCompleteSems[imageIndex];

    VkSubmitInfo si = {};
    si.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
    si.waitSemaphoreCount = appWaitCount;
    si.pWaitSemaphores = appWaitCount ? appWaitSems : nullptr;
    si.pWaitDstStageMask = appWaitCount ? waitStages.data() : nullptr;
    si.commandBufferCount = 1;
    si.pCommandBuffers = &cmd;
    si.signalSemaphoreCount = 1;
    si.pSignalSemaphores = &signal;
    if (p_vkQueueSubmit(g_queue, 1, &si, fence) != VK_SUCCESS) return false;

    *outSignal = signal;
    return true;
}

// ============================================================================
// Hooks (installed on the loader dispatch trampolines)
// ============================================================================

static void CaptureLiveDevice(VkDevice device) {
    if (g_device == device) return;
    g_device = device;
    g_deviceFuncsLoaded = LoadDeviceFuncs(device);
    if (g_queue == VK_NULL_HANDLE && g_deviceFuncsLoaded && p_vkGetDeviceQueue &&
        g_queueFamily != vkoverlay::kInvalidFamily) {
        p_vkGetDeviceQueue(device, g_queueFamily, 0, &g_queue);
    }
}

static VkResult VKAPI_PTR Hooked_vkAcquireNextImageKHR(
        VkDevice device, VkSwapchainKHR swapchain, uint64_t timeout,
        VkSemaphore semaphore, VkFence fence, uint32_t* pImageIndex) {
    {
        VkLockGuard lk;
        if (g_device == VK_NULL_HANDLE) CaptureLiveDevice(device);
        if (device == g_device) g_swapchain = swapchain;
        if (!g_diagAcquire) {
            g_diagAcquire = true;
            char b[128];
            _snprintf(b, sizeof(b), "Vulkan: vkAcquireNextImageKHR firing (device+swapchain captured, funcs=%d).",
                      g_deviceFuncsLoaded ? 1 : 0);
            RbLog(b);
        }
    }
    return o_vkAcquireNextImageKHR(device, swapchain, timeout, semaphore, fence, pImageIndex);
}

static VkResult VKAPI_PTR Hooked_vkCreateSwapchainKHR(
        VkDevice device, const VkSwapchainCreateInfoKHR* pCreateInfo,
        const VkAllocationCallbacks* pAllocator, VkSwapchainKHR* pSwapchain) {
    VkResult r = o_vkCreateSwapchainKHR(device, pCreateInfo, pAllocator, pSwapchain);
    if (r == VK_SUCCESS && pSwapchain && pCreateInfo) {
        VkLockGuard lk;
        if (g_device == VK_NULL_HANDLE) CaptureLiveDevice(device);
        if (device == g_device) {
            ShutdownImGuiVulkan();
            DestroySwapchainResources();
            g_swapchain = *pSwapchain;
            g_swapFormat = pCreateInfo->imageFormat;
            g_swapFormatAuthoritative = true;
            g_swapExtent = pCreateInfo->imageExtent;
            g_swapExtentKnown = true;
        }
        if (!g_diagCreateSc) {
            g_diagCreateSc = true;
            char b[192];
            _snprintf(b, sizeof(b), "Vulkan: vkCreateSwapchainKHR firing (format=%d extent=%ux%u minImages=%u).",
                      (int)pCreateInfo->imageFormat, pCreateInfo->imageExtent.width,
                      pCreateInfo->imageExtent.height, pCreateInfo->minImageCount);
            RbLog(b);
        }
    }
    return r;
}

static VkResult VKAPI_PTR Hooked_vkQueuePresentKHR(VkQueue queue, const VkPresentInfoKHR* pPresentInfo) {
    if (RenderBackend_GetActiveKind() == GfxBackendKind::OpenGL || !pPresentInfo)
        return o_vkQueuePresentKHR(queue, pPresentInfo);

    ClaimBackend(GfxBackendKind::Vulkan);
    if (!g_diagPresent) { g_diagPresent = true; RbLog("Vulkan: vkQueuePresentKHR firing (Vulkan present path active)."); }

    VkSemaphore overlaySignal = VK_NULL_HANDLE;
    bool overlayOk = false;
    {
        VkLockGuard lk;
        if (g_queue == VK_NULL_HANDLE) g_queue = queue;   // capture the present/graphics queue

        if (g_device != VK_NULL_HANDLE && pPresentInfo->swapchainCount > 0) {
            uint32_t imageIndex = UINT32_MAX;
            VkSwapchainKHR sc = VK_NULL_HANDLE;
            for (uint32_t i = 0; i < pPresentInfo->swapchainCount; ++i) {
                if (g_swapchain == VK_NULL_HANDLE || pPresentInfo->pSwapchains[i] == g_swapchain) {
                    sc = pPresentInfo->pSwapchains[i];
                    imageIndex = pPresentInfo->pImageIndices[i];
                    break;
                }
            }
            if (sc != VK_NULL_HANDLE) {
                g_swapchain = sc;
                HWND hwnd = FindGlfwGameWindow();
                if (hwnd)
                    overlayOk = RenderOverlayIntoImage(hwnd, imageIndex,
                        pPresentInfo->pWaitSemaphores, pPresentInfo->waitSemaphoreCount, &overlaySignal);
            }
        }
    }

    if (overlayOk && overlaySignal != VK_NULL_HANDLE) {
        VkPresentInfoKHR present = *pPresentInfo;
        present.waitSemaphoreCount = 1;
        present.pWaitSemaphores = &overlaySignal;
        return o_vkQueuePresentKHR(queue, &present);
    }
    return o_vkQueuePresentKHR(queue, pPresentInfo);
}

static void VKAPI_PTR Hooked_vkDestroySwapchainKHR(
        VkDevice device, VkSwapchainKHR swapchain, const VkAllocationCallbacks* pAllocator) {
    {
        VkLockGuard lk;
        if (device == g_device && swapchain == g_swapchain) {
            ShutdownImGuiVulkan();
            DestroySwapchainResources();
            g_swapchain = VK_NULL_HANDLE;
            RbLog("Vulkan: swapchain destroyed; overlay resources released.");
        }
    }
    o_vkDestroySwapchainKHR(device, swapchain, pAllocator);
}

static void VKAPI_PTR Hooked_vkDestroyDevice(VkDevice device, const VkAllocationCallbacks* pAllocator) {
    {
        VkLockGuard lk;
        if (device == g_device) {
            ShutdownImGuiVulkan();
            DestroySwapchainResources();
            if (g_renderPass && p_vkDestroyRenderPass) { p_vkDestroyRenderPass(g_device, g_renderPass, nullptr); g_renderPass = VK_NULL_HANDLE; }
            g_swapchain = VK_NULL_HANDLE;
            g_swapFormat = VK_FORMAT_UNDEFINED;
            g_swapFormatAuthoritative = false;
            g_swapExtentKnown = false;
            g_device = VK_NULL_HANDLE;
            g_queue = VK_NULL_HANDLE;
            g_deviceFuncsLoaded = false;
            RbLog("Vulkan: device destroyed; overlay fully torn down.");
        }
    }
    o_vkDestroyDevice(device, pAllocator);
}

// ============================================================================
// Throwaway instance/device -> resolve dispatch trampolines + physical device/family
// ============================================================================

static bool CreateDummyObjectsAndResolveTrampolines(
        void** presentAddr, void** acquireAddr, void** createScAddr,
        void** destroyScAddr, void** destroyDevAddr) {
    PFN_vkCreateInstance createInstance =
        (PFN_vkCreateInstance)g_vkGetInstanceProcAddr(nullptr, "vkCreateInstance");
    if (!createInstance) { RbLog("Vulkan: vkCreateInstance (global) not found."); return false; }

    VkApplicationInfo app = {};
    app.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    app.apiVersion = VK_API_VERSION_1_1;
    const char* instExt[] = { "VK_KHR_surface" };
    VkInstanceCreateInfo ici = {};
    ici.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    ici.pApplicationInfo = &app;
    ici.enabledExtensionCount = 1;
    ici.ppEnabledExtensionNames = instExt;
    if (createInstance(&ici, nullptr, &g_instance) != VK_SUCCESS || g_instance == VK_NULL_HANDLE) {
        // Retry with no extensions (some minimal loaders).
        ici.enabledExtensionCount = 0; ici.ppEnabledExtensionNames = nullptr;
        if (createInstance(&ici, nullptr, &g_instance) != VK_SUCCESS || g_instance == VK_NULL_HANDLE) {
            RbLog("Vulkan: dummy vkCreateInstance failed.");
            return false;
        }
    }
    g_instanceApiVersion = app.apiVersion;

    PFN_vkEnumeratePhysicalDevices enumPhys =
        (PFN_vkEnumeratePhysicalDevices)g_vkGetInstanceProcAddr(g_instance, "vkEnumeratePhysicalDevices");
    PFN_vkGetPhysicalDeviceProperties getProps =
        (PFN_vkGetPhysicalDeviceProperties)g_vkGetInstanceProcAddr(g_instance, "vkGetPhysicalDeviceProperties");
    p_vkGetPhysicalDeviceQueueFamilyProperties =
        (PFN_vkGetPhysicalDeviceQueueFamilyProperties)g_vkGetInstanceProcAddr(g_instance, "vkGetPhysicalDeviceQueueFamilyProperties");
    PFN_vkCreateDevice createDevice =
        (PFN_vkCreateDevice)g_vkGetInstanceProcAddr(g_instance, "vkCreateDevice");
    g_vkGetDeviceProcAddr =
        (PFN_vkGetDeviceProcAddr)g_vkGetInstanceProcAddr(g_instance, "vkGetDeviceProcAddr");
    if (!enumPhys || !p_vkGetPhysicalDeviceQueueFamilyProperties || !createDevice || !g_vkGetDeviceProcAddr) {
        RbLog("Vulkan: failed to resolve instance functions for dummy device.");
        return false;
    }

    uint32_t gpuCount = 0;
    enumPhys(g_instance, &gpuCount, nullptr);
    if (gpuCount == 0) { RbLog("Vulkan: no physical devices."); return false; }
    std::vector<VkPhysicalDevice> gpus(gpuCount);
    enumPhys(g_instance, &gpuCount, gpus.data());

    // Prefer a discrete GPU; fall back to the first.
    g_physicalDevice = gpus[0];
    if (getProps) {
        for (VkPhysicalDevice pd : gpus) {
            VkPhysicalDeviceProperties props = {};
            getProps(pd, &props);
            if (props.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU) { g_physicalDevice = pd; break; }
        }
    }

    // First graphics-capable queue family.
    uint32_t famCount = 0;
    p_vkGetPhysicalDeviceQueueFamilyProperties(g_physicalDevice, &famCount, nullptr);
    std::vector<VkQueueFamilyProperties> fams(famCount);
    p_vkGetPhysicalDeviceQueueFamilyProperties(g_physicalDevice, &famCount, fams.data());
    std::vector<uint32_t> flags(famCount);
    for (uint32_t i = 0; i < famCount; ++i) flags[i] = (uint32_t)fams[i].queueFlags;
    g_queueFamily = vkoverlay::SelectGraphicsQueueFamily(flags.empty() ? nullptr : flags.data(), famCount, nullptr, 0);
    if (g_queueFamily == vkoverlay::kInvalidFamily) { RbLog("Vulkan: no graphics queue family."); return false; }

    // Create the dummy device with the swapchain extension so vkGetDeviceProcAddr returns the
    // swapchain trampolines. Those addresses are shared with the game's device.
    float prio = 1.0f;
    VkDeviceQueueCreateInfo qci = {};
    qci.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO;
    qci.queueFamilyIndex = g_queueFamily;
    qci.queueCount = 1;
    qci.pQueuePriorities = &prio;
    const char* devExt[] = { "VK_KHR_swapchain" };
    VkDeviceCreateInfo dci = {};
    dci.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO;
    dci.queueCreateInfoCount = 1;
    dci.pQueueCreateInfos = &qci;
    dci.enabledExtensionCount = 1;
    dci.ppEnabledExtensionNames = devExt;
    if (createDevice(g_physicalDevice, &dci, nullptr, &g_dummyDevice) != VK_SUCCESS || g_dummyDevice == VK_NULL_HANDLE) {
        RbLog("Vulkan: dummy vkCreateDevice failed.");
        return false;
    }

    *presentAddr    = (void*)g_vkGetDeviceProcAddr(g_dummyDevice, "vkQueuePresentKHR");
    *acquireAddr    = (void*)g_vkGetDeviceProcAddr(g_dummyDevice, "vkAcquireNextImageKHR");
    *createScAddr   = (void*)g_vkGetDeviceProcAddr(g_dummyDevice, "vkCreateSwapchainKHR");
    *destroyScAddr  = (void*)g_vkGetDeviceProcAddr(g_dummyDevice, "vkDestroySwapchainKHR");
    *destroyDevAddr = (void*)g_vkGetDeviceProcAddr(g_dummyDevice, "vkDestroyDevice");

    char b[160];
    _snprintf(b, sizeof(b), "Vulkan: dummy device ready (queueFamily=%u present=%p acquire=%p createSc=%p).",
              g_queueFamily, *presentAddr, *acquireAddr, *createScAddr);
    RbLog(b);
    return *presentAddr && *acquireAddr && *createScAddr && *destroyScAddr && *destroyDevAddr;
}

// ============================================================================
// Hook installation
// ============================================================================

static bool InstallHook(void* target, void* detour, void** original, int slot) {
    if (!target) return false;
    if (MH_CreateHook(target, detour, original) != MH_OK) { RbLog("Vulkan: MH_CreateHook failed (slot)"); return false; }
    if (MH_EnableHook(target) != MH_OK) { RbLog("Vulkan: MH_EnableHook failed (slot)"); return false; }
    g_hookTargets[slot] = target;
    return true;
}

bool RenderBackend_InitHooks() {
    if (!IsVulkanHookEnabled()) return false;
    if (g_hookTargets[0]) return true;

    g_vulkanDll = GetModuleHandleA("vulkan-1.dll");
    if (!g_vulkanDll) g_vulkanDll = LoadLibraryA("vulkan-1.dll");
    if (!g_vulkanDll) { RbLog("Vulkan: vulkan-1.dll not present; Vulkan overlay disabled."); return false; }

    g_vkGetInstanceProcAddr = (PFN_vkGetInstanceProcAddr)GetProcAddress(g_vulkanDll, "vkGetInstanceProcAddr");
    if (!g_vkGetInstanceProcAddr) { RbLog("Vulkan: vkGetInstanceProcAddr export missing."); return false; }

    if (!g_vkLockInit) { InitializeCriticalSection(&g_vkLock); g_vkLockInit = true; }

    void *present = nullptr, *acquire = nullptr, *createSc = nullptr, *destroySc = nullptr, *destroyDev = nullptr;
    if (!CreateDummyObjectsAndResolveTrampolines(&present, &acquire, &createSc, &destroySc, &destroyDev)) {
        RbLog("Vulkan: could not resolve dispatch trampolines; Vulkan overlay disabled.");
        return false;
    }

    bool ok = true;
    ok &= InstallHook(present,    (void*)Hooked_vkQueuePresentKHR,     (void**)&o_vkQueuePresentKHR,     0);
    ok &= InstallHook(acquire,    (void*)Hooked_vkAcquireNextImageKHR, (void**)&o_vkAcquireNextImageKHR, 1);
    ok &= InstallHook(createSc,   (void*)Hooked_vkCreateSwapchainKHR,  (void**)&o_vkCreateSwapchainKHR,  2);
    ok &= InstallHook(destroySc,  (void*)Hooked_vkDestroySwapchainKHR, (void**)&o_vkDestroySwapchainKHR, 3);
    ok &= InstallHook(destroyDev, (void*)Hooked_vkDestroyDevice,       (void**)&o_vkDestroyDevice,       4);

    if (ok) RbLog("Vulkan: dispatch-trampoline hooks installed (auto-detect armed).");
    else    RbLog("Vulkan: one or more dispatch hooks failed; Vulkan overlay may be inactive.");
    return ok;
}

void RenderBackend_Shutdown() {
    {
        VkLockGuard lk;
        ShutdownImGuiVulkan();
        DestroySwapchainResources();
        if (g_renderPass && p_vkDestroyRenderPass && g_device) { p_vkDestroyRenderPass(g_device, g_renderPass, nullptr); g_renderPass = VK_NULL_HANDLE; }
    }
    for (int i = 0; i < kMaxHooks; ++i) {
        if (g_hookTargets[i]) { MH_DisableHook(g_hookTargets[i]); g_hookTargets[i] = nullptr; }
    }
    o_vkQueuePresentKHR = nullptr;
    o_vkAcquireNextImageKHR = nullptr;
    o_vkCreateSwapchainKHR = nullptr;
    o_vkDestroySwapchainKHR = nullptr;
    o_vkDestroyDevice = nullptr;
    // Note: the throwaway instance/device are intentionally left alive; g_physicalDevice is
    // used for ImGui memory-type queries and the trampoline addresses remain valid regardless.
}
