#pragma once

#include <windows.h>

// Graphics backend the game is presenting with, decided at runtime by whichever
// present path fires first (wglSwapBuffers -> OpenGL, vkQueuePresentKHR -> Vulkan).
enum class GfxBackendKind {
    Unknown = 0,
    OpenGL,
    Vulkan
};

typedef void (*RenderBackendLogFn)(const char* msg);
void RenderBackend_SetLogFn(RenderBackendLogFn fn);

// Install the Vulkan present/device/swapchain hooks. The OpenGL hook (wglSwapBuffers)
// lives in bridge_261.cpp. Safe to call repeatedly. Returns true if the Vulkan hooks
// were installed (vulkan-1.dll present + hooks armed).
bool RenderBackend_InitHooks();
void RenderBackend_Shutdown();

// Backend arbitration (first present wins for the session).
GfxBackendKind RenderBackend_GetActiveKind();
void RenderBackend_NotifyOpenGlFrame(HWND hwnd);   // called from the wglSwapBuffers hook

// -----------------------------------------------------------------------------
// Renderer-neutral overlay seams implemented in bridge_261.cpp.
// These own the ImGui context, Win32 platform backend, fonts and panel drawing,
// independent of the renderer (OpenGL3 vs Vulkan) that ultimately rasterizes them.
// -----------------------------------------------------------------------------

// Create the ImGui context + load fonts (DPI-aware) + init the Win32 platform backend.
// Idempotent; renderer-neutral (performs no OpenGL/Vulkan calls). Returns true when ready.
bool Bridge_EnsureImGuiPlatform(HWND hwnd);

// True once Bridge_EnsureImGuiPlatform has completed successfully.
bool Bridge_IsImGuiPlatformReady();

// Build one overlay frame: platform NewFrame + ImGui::NewFrame + game-state update +
// panel draw + ImGui::Render(). MUST be called AFTER the active renderer backend's own
// NewFrame() for the frame. Returns the finalized ImDrawData, or nullptr when the frame
// should be skipped (context not ready, world transition, degenerate display size).
struct ImDrawData;
ImDrawData* Bridge_BuildOverlayDrawData(int winW, int winH);

// Shared OpenGL overlay frame entry points (used by the wglSwapBuffers path).
void Bridge_BeginOverlayImGuiFrame(int winW, int winH);
void Bridge_RenderOverlayPanels(bool inWorld);
void Bridge_EndOverlayImGuiFrame_OpenGL();
