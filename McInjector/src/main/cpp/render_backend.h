#pragma once

#include <windows.h>

enum class GfxBackendKind {
    Unknown = 0,
    OpenGL,
    Vulkan
};

typedef void (*RenderBackendLogFn)(const char* msg);
void RenderBackend_SetLogFn(RenderBackendLogFn fn);

// Install Vulkan present hooks (OpenGL hook lives in bridge_261.cpp). Safe to call repeatedly.
bool RenderBackend_InitHooks();
void RenderBackend_Shutdown();

GfxBackendKind RenderBackend_GetActiveKind();
void RenderBackend_NotifyOpenGlFrame(HWND hwnd);
void RenderBackend_NotifyVulkanFrame();

// Shared overlay draw (implemented in bridge_261.cpp).
void Bridge_BeginOverlayImGuiFrame(int winW, int winH);
void Bridge_RenderOverlayPanels(bool inWorld);
void Bridge_EndOverlayImGuiFrame_OpenGL();
void Bridge_EndOverlayImGuiFrame_VulkanAuxGL();
bool Bridge_RenderVulkanOverlayFrame(HWND hwnd, int width, int height);

bool RenderBackend_RenderVulkanOverlayFrame(HWND hwnd, int width, int height);
