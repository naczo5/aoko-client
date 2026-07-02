#include "render_backend.h"

#include <windows.h>
#include <stdint.h>

#ifndef VKAPI_PTR
#define VKAPI_CALL __stdcall
#define VKAPI_PTR VKAPI_CALL
#endif

#include "MinHook.h"

enum class GfxBackendKindInternal {
    Unknown = 0,
    OpenGL,
    Vulkan
};

static GfxBackendKindInternal g_activeBackend = GfxBackendKindInternal::Unknown;

typedef int32_t VkResult;
typedef void* VkQueue;

struct VkPresentInfoKHR {
    uint32_t sType;
    const void* pNext;
    uint32_t waitSemaphoreCount;
    const void* pWaitSemaphores;
    uint32_t swapchainCount;
    const void* pSwapchains;
    const uint32_t* pImageIndices;
    VkResult* pResults;
};

typedef VkResult (VKAPI_PTR *PFN_vkQueuePresentKHR)(VkQueue queue, const VkPresentInfoKHR* pPresentInfo);
static PFN_vkQueuePresentKHR o_vkQueuePresentKHR = nullptr;
static void* g_vkPresentTarget = nullptr;
static RenderBackendLogFn g_logFn = nullptr;
static bool g_loggedFirstVulkanPresent = false;

// Lunar 26.1 renders via OpenGL (wglSwapBuffers). The Vulkan present hook creates a
// second GL context on the GLFW window and shares ImGui globals with the OpenGL path,
// which races at startup and has caused 0xC0000005 crashes in nvoglv64. Disabled until
// the backends use separate ImGui state. Set env AOKO_BRIDGE261_VULKAN=1 to opt in.
static bool IsVulkanPresentHookEnabled() {
    static int cached = -1;
    if (cached >= 0) return cached != 0;
    char buf[8] = {};
    DWORD n = GetEnvironmentVariableA("AOKO_BRIDGE261_VULKAN", buf, sizeof(buf));
    cached = (n > 0 && buf[0] == '1') ? 1 : 0;
    return cached != 0;
}

static void RbLog(const char* msg) {
    if (g_logFn) g_logFn(msg);
}

void RenderBackend_SetLogFn(RenderBackendLogFn fn) {
    g_logFn = fn;
}

static HWND FindGlfwGameWindow() {
    HWND found = nullptr;
    EnumWindows([](HWND hwnd, LPARAM lp) -> BOOL {
        char cls[256] = {};
        if (GetClassNameA(hwnd, cls, sizeof(cls)) && strcmp(cls, "GLFW30") == 0) {
            *(HWND*)lp = hwnd;
            return FALSE;
        }
        return TRUE;
    }, (LPARAM)&found);
    return found;
}

static VkResult VKAPI_PTR HvkQueuePresentKHR(VkQueue queue, const VkPresentInfoKHR* pPresentInfo) {
    (void)pPresentInfo;
    if (g_activeBackend != GfxBackendKindInternal::OpenGL) {
        g_activeBackend = GfxBackendKindInternal::Vulkan;
        if (!g_loggedFirstVulkanPresent) {
            g_loggedFirstVulkanPresent = true;
            RbLog("Vulkan vkQueuePresentKHR hook firing (first frame).");
        }
        HWND hwnd = FindGlfwGameWindow();
        if (hwnd) {
            RECT rc = {};
            if (GetClientRect(hwnd, &rc)) {
                int w = rc.right - rc.left;
                int h = rc.bottom - rc.top;
                Bridge_RenderVulkanOverlayFrame(hwnd, w, h);
            }
        } else {
            static bool s_warnedNoGlfw = false;
            if (!s_warnedNoGlfw) {
                s_warnedNoGlfw = true;
                RbLog("WARNING: Vulkan present hook active but GLFW game window not found.");
            }
        }
    }
    return o_vkQueuePresentKHR ? o_vkQueuePresentKHR(queue, pPresentInfo) : 0;
}

bool RenderBackend_InitHooks() {
    if (!IsVulkanPresentHookEnabled()) return false;
    if (g_vkPresentTarget) return true;

    HMODULE hVulkan = GetModuleHandleA("vulkan-1.dll");
    if (!hVulkan) hVulkan = LoadLibraryA("vulkan-1.dll");
    if (!hVulkan) {
        return false;
    }

    void* pPresent = (void*)GetProcAddress(hVulkan, "vkQueuePresentKHR");
    if (!pPresent) {
        RbLog("WARNING: vulkan-1.dll loaded but vkQueuePresentKHR export missing.");
        return false;
    }

    if (MH_CreateHook(pPresent, (void*)HvkQueuePresentKHR, (void**)&o_vkQueuePresentKHR) == MH_OK) {
        g_vkPresentTarget = pPresent;
        MH_EnableHook(pPresent);
        RbLog("Vulkan vkQueuePresentKHR hook installed.");
        return true;
    }
    RbLog("WARNING: Failed to hook vkQueuePresentKHR.");
    return false;
}

void RenderBackend_Shutdown() {
    if (g_vkPresentTarget) {
        MH_DisableHook(g_vkPresentTarget);
        g_vkPresentTarget = nullptr;
        o_vkQueuePresentKHR = nullptr;
    }
}

GfxBackendKind RenderBackend_GetActiveKind() {
    switch (g_activeBackend) {
    case GfxBackendKindInternal::OpenGL: return GfxBackendKind::OpenGL;
    case GfxBackendKindInternal::Vulkan: return GfxBackendKind::Vulkan;
    default: return GfxBackendKind::Unknown;
    }
}

void RenderBackend_NotifyOpenGlFrame(HWND /*hwnd*/) {
    g_activeBackend = GfxBackendKindInternal::OpenGL;
}

void RenderBackend_NotifyVulkanFrame() {
    g_activeBackend = GfxBackendKindInternal::Vulkan;
}

bool RenderBackend_RenderVulkanOverlayFrame(HWND hwnd, int width, int height) {
    return Bridge_RenderVulkanOverlayFrame(hwnd, width, height);
}
