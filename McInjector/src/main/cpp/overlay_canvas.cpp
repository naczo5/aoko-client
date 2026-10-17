#include "overlay_canvas.h"

#ifdef DrawText
#undef DrawText
#endif

#include "glad/gl.h"
#include "ui/aris_ui.hpp"
#include "ui/renderer/vulkan_renderer.hpp"

#include <cmath>
#include <exception>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

namespace {

ui::Context* g_ctx = nullptr;
std::unique_ptr<ui::VulkanRenderer> g_vk;
ui::FontAtlas g_fontAtlas;
bool g_fontReady = false;
float g_fontPx = 16.f;
std::string g_fontPng;
std::string g_fontJson;
bool g_glSkipDeletes = false;
bool g_glObjectsOrphaned = false;
bool g_glReady = false;
HWND g_hwnd = nullptr;
ImFont g_font;
ImFontAtlas g_atlas;
ImGuiStyle g_style;
ImDrawList g_draw;
int g_alive = 0;
bool g_down = false;
bool g_pendingClick = false;
bool g_pendingRelease = false;
bool g_clickFrame = false;
bool g_releaseFrame = false;

ImGuiIO g_io;
OverlayLogFn g_log = nullptr;

void OverlayLog(const char* message) {
    if (g_log && message) g_log(message);
}

std::string ModuleDir() {
    HMODULE mod = nullptr;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                           GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       (LPCSTR)&ModuleDir, &mod);
    char path[MAX_PATH] = {};
    GetModuleFileNameA(mod, path, MAX_PATH);
    std::string s(path);
    size_t slash = s.find_last_of("\\/");
    return slash == std::string::npos ? std::string() : s.substr(0, slash);
}

bool FileExists(const std::string& path) {
    DWORD attr = GetFileAttributesA(path.c_str());
    return attr != INVALID_FILE_ATTRIBUTES && (attr & FILE_ATTRIBUTE_DIRECTORY) == 0;
}

void RememberFontPair(const std::string& png, const std::string& json) {
    if (FileExists(png) && FileExists(json)) {
        g_fontPng = png;
        g_fontJson = json;
    }
}

void EnsureContext() {
    if (!g_ctx) g_ctx = new ui::Context();
    g_alive = 1;
}

DWORD g_fontRetryAt = 0;
bool g_fontFailureLogged = false;

void EnsureFont() {
    EnsureContext();
    if (g_fontReady) return;
    // Every ImDrawList command (and CalcTextSize) calls this, so a missing or
    // unreadable atlas must not re-hit the filesystem per draw call: a full
    // Load attempt (open + parse + decode) costs hundreds of microseconds and
    // dropped the game to single-digit FPS. Re-attach an already loaded atlas
    // for free, otherwise retry at most once every five seconds.
    if (g_fontAtlas.Loaded()) {
        g_ctx->SetFont(&g_fontAtlas);
        g_fontReady = true;
        return;
    }
    const DWORD now = GetTickCount();
    if (g_fontRetryAt != 0 && (LONG)(now - g_fontRetryAt) < 0) return;
    g_fontRetryAt = now + 5000;

    if (g_fontPng.empty()) {
        const std::string dir = ModuleDir();
        RememberFontPair(dir + "\\inter.png", dir + "\\inter.json");
        if (g_fontPng.empty())
            RememberFontPair(dir + "\\assets\\font\\inter.png", dir + "\\assets\\font\\inter.json");
    }
    if (g_fontPng.empty()) return;
    if (g_fontAtlas.Load(g_fontPng, g_fontJson)) {
        g_ctx->SetFont(&g_fontAtlas);
        g_fontReady = true;
        OverlayLog("font atlas loaded");
        return;
    }
    if (!g_fontFailureLogged) {
        g_fontFailureLogged = true;
        const std::string msg = "font atlas failed to load: " + g_fontJson;
        OverlayLog(msg.c_str());
    }
}

ui::Color Unpack(ImU32 col) {
    return ui::Color((uint8_t)(col & 255), (uint8_t)((col >> 8) & 255),
                     (uint8_t)((col >> 16) & 255), (uint8_t)((col >> 24) & 255));
}

ui::Rect FromCorners(ImVec2 a, ImVec2 b) {
    float x0 = a.x < b.x ? a.x : b.x;
    float y0 = a.y < b.y ? a.y : b.y;
    float x1 = a.x > b.x ? a.x : b.x;
    float y1 = a.y > b.y ? a.y : b.y;
    return ui::Rect(x0, y0, x1 - x0, y1 - y0);
}

void NoteButton(bool down) {
    if (down && !g_down) g_pendingClick = true;
    if (!down && g_down) g_pendingRelease = true;
    g_down = down;
}

void PublishDisplaySize() {
    if (!g_hwnd) return;
    RECT rc = {};
    if (!GetClientRect(g_hwnd, &rc)) return;
    g_io.DisplaySize.x = (float)(rc.right - rc.left);
    g_io.DisplaySize.y = (float)(rc.bottom - rc.top);
}

} // namespace

ImGuiIO::ImGuiIO()
    : ConfigFlags(0), IniFilename(nullptr), FontGlobalScale(1.f), Fonts(&g_atlas) {
    MousePos = ImVec2();
    DisplaySize = ImVec2();
    for (int i = 0; i < 5; ++i) MouseDown[i] = false;
}

void ImFontAtlas::Clear() {}

ImFont* ImFontAtlas::AddFontDefault(ImFontConfig* config) {
    if (config && config->SizePixels > 0.f) g_fontPx = config->SizePixels;
    return &g_font;
}

ImFont* ImFontAtlas::AddFontFromFileTTF(const char* filename, float sizePixels, ImFontConfig* config) {
    if (sizePixels > 0.f) g_fontPx = sizePixels;
    else if (config && config->SizePixels > 0.f) g_fontPx = config->SizePixels;
    if (filename && *filename) {
        std::string path(filename);
        size_t dot = path.find_last_of('.');
        std::string stem = dot == std::string::npos ? path : path.substr(0, dot);
        RememberFontPair(stem + ".png", stem + ".json");
    }
    return &g_font;
}

void ImDrawList::AddLine(const ImVec2& a, const ImVec2& b, ImU32 col, float thickness) {
    EnsureFont();
    if (((col >> 24) & 255) == 0) return;
    g_ctx->DrawLine(ui::Vec2(a.x, a.y), ui::Vec2(b.x, b.y), thickness, Unpack(col));
}

void ImDrawList::AddRect(const ImVec2& a, const ImVec2& b, ImU32 col, float rounding, int, float thickness) {
    EnsureFont();
    if (((col >> 24) & 255) == 0) return;
    ui::RectStyle st;
    // ImGui's AddRect is outline-only: the aris defaults fill to opaque white,
    // so it must be cleared explicitly or every border paints a white plate.
    st.fill = ui::Color::Transparent();
    st.borderColor = Unpack(col);
    st.borderWidth = thickness;
    st.cornerRadii = ui::Vec4(rounding);
    g_ctx->DrawRect(FromCorners(a, b), st);
}

void ImDrawList::AddRectFilled(const ImVec2& a, const ImVec2& b, ImU32 col, float rounding, int) {
    EnsureFont();
    if (((col >> 24) & 255) == 0) return;
    ui::RectStyle st;
    st.fill = Unpack(col);
    st.cornerRadii = ui::Vec4(rounding);
    g_ctx->DrawRect(FromCorners(a, b), st);
}

void ImDrawList::AddTriangle(const ImVec2& a, const ImVec2& b, const ImVec2& c, ImU32 col, float thickness) {
    EnsureFont();
    g_ctx->DrawTriangle(ui::Vec2(a.x, a.y), ui::Vec2(b.x, b.y), ui::Vec2(c.x, c.y), Unpack(col), thickness);
}

void ImDrawList::AddTriangleFilled(const ImVec2& a, const ImVec2& b, const ImVec2& c, ImU32 col) {
    EnsureFont();
    g_ctx->DrawTriangle(ui::Vec2(a.x, a.y), ui::Vec2(b.x, b.y), ui::Vec2(c.x, c.y), Unpack(col), 0.f);
}

void ImDrawList::AddCircle(const ImVec2& center, float radius, ImU32 col, int, float thickness) {
    EnsureFont();
    g_ctx->DrawCircle(ui::Vec2(center.x, center.y), radius, Unpack(col), thickness);
}

void ImDrawList::AddCircleFilled(const ImVec2& center, float radius, ImU32 col, int) {
    EnsureFont();
    g_ctx->DrawCircle(ui::Vec2(center.x, center.y), radius, Unpack(col), 0.f);
}

void ImDrawList::AddText(const ImVec2& pos, ImU32 col, const char* text) {
    AddText(nullptr, g_fontPx, pos, col, text, nullptr);
}

void ImDrawList::AddText(const ImFont*, float fontSize, const ImVec2& pos, ImU32 col,
                         const char* textBegin, const char* textEnd) {
    EnsureFont();
    if (!textBegin || !*textBegin || ((col >> 24) & 255) == 0) return;
    std::string text = textEnd ? std::string(textBegin, textEnd) : std::string(textBegin);
    float size = fontSize > 0.f ? fontSize : g_fontPx;
    g_ctx->DrawText(ui::Vec2(pos.x, pos.y), text, Unpack(col), size);
}

namespace ImGui {

ImGuiContext* GetCurrentContext() { return g_alive ? (ImGuiContext*)1 : nullptr; }

ImGuiContext* CreateContext() {
    EnsureContext();
    EnsureFont();
    return GetCurrentContext();
}

void DestroyContext(ImGuiContext*) {
    if (g_ctx && !g_glSkipDeletes && !g_glObjectsOrphaned) {
        g_ctx->SetDrawBackend(nullptr);
        g_ctx->ReleaseGraphics();
        delete g_ctx;
    }
    g_ctx = nullptr;
    g_alive = 0;
    g_fontReady = false;
    g_glReady = false;
    g_glObjectsOrphaned = false;
}

ImGuiIO& GetIO() { return g_io; }
ImGuiStyle& GetStyle() { return g_style; }
ImDrawList* GetForegroundDrawList() { return &g_draw; }
ImFont* GetFont() { return &g_font; }
float GetFontSize() { return g_fontPx; }

ImVec2 CalcTextSize(const char* text, const char* textEnd, bool, float) {
    EnsureFont();
    if (!text || !g_fontReady) return ImVec2(0, g_fontPx);
    std::string s = textEnd ? std::string(text, textEnd) : std::string(text);
    ui::Vec2 m = g_ctx->MeasureText(s, g_fontPx);
    return ImVec2(m.x, m.y);
}

bool IsMouseClicked(int button, bool) { return button == 0 && g_clickFrame; }
bool IsMouseReleased(int button) { return button == 0 && g_releaseFrame; }

void NewFrame() {
    EnsureFont();
    PublishDisplaySize();
    g_clickFrame = g_pendingClick;
    g_releaseFrame = g_pendingRelease;
    g_pendingClick = g_pendingRelease = false;
    g_io.MouseDown[0] = g_down;
    g_ctx->BeginFrame(1.f / 60.f, g_io.DisplaySize.x, g_io.DisplaySize.y, 1.f);
}

void Render() {
    if (!g_ctx) return;
    if (g_io.DisplaySize.x <= 1.f || g_io.DisplaySize.y <= 1.f) g_ctx->DiscardFrame();
}

ImDrawData* GetDrawData() { return g_alive ? (ImDrawData*)1 : nullptr; }

} // namespace ImGui

void ImGui_ImplWin32_Init(HWND hwnd) { g_hwnd = hwnd; }
void ImGui_ImplWin32_InitForOpenGL(HWND hwnd) { g_hwnd = hwnd; }
void ImGui_ImplWin32_Shutdown() { g_hwnd = nullptr; }

void ImGui_ImplWin32_NewFrame() { PublishDisplaySize(); }

LRESULT ImGui_ImplWin32_WndProcHandler(HWND hwnd, UINT msg, WPARAM, LPARAM lParam) {
    if (msg == WM_MOUSEMOVE || msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP) {
        g_io.MousePos = ImVec2((float)(short)LOWORD(lParam), (float)(short)HIWORD(lParam));
    }
    if (msg == WM_LBUTTONDOWN) NoteButton(true);
    if (msg == WM_LBUTTONUP) NoteButton(false);
    (void)hwnd;
    return 0;
}

bool ImGui_ImplOpenGL3_Init(const char*) {
    EnsureContext();
    EnsureFont();
    g_glReady = true;
    return true;
}

void ImGui_ImplOpenGL3_Shutdown() {
    if (g_ctx && !g_glSkipDeletes) g_ctx->ReleaseGraphics();
    else if (g_ctx) g_glObjectsOrphaned = true;
    g_glReady = false;
}

void ImGui_ImplOpenGL3_NewFrame() { EnsureFont(); }

void ImGui_ImplOpenGL3_RenderDrawData(void*) {
    if (!g_ctx) return;
    if (g_io.DisplaySize.x <= 1.f || g_io.DisplaySize.y <= 1.f) {
        g_ctx->DiscardFrame();
        return;
    }
    if (!glad_glCreateShader) {
        OverlayLog("loading OpenGL entry points");
        if (gladLoaderLoadGL() == 0) {
            OverlayLog("OpenGL entry points failed to load");
            g_ctx->DiscardFrame();
            return;
        }
        const char* ver = reinterpret_cast<const char*>(glGetString(GL_VERSION));
        char buf[192];
        std::snprintf(buf, sizeof(buf), "GL_VERSION %s", ver ? ver : "(null)");
        OverlayLog(buf);
    }
    static int frames = 0;
    if (frames < 2) OverlayLog("present begin");
    g_ctx->SetDrawBackend(nullptr);
    try {
        g_ctx->Render();
    } catch (const std::exception& ex) {
        char buf[256];
        std::snprintf(buf, sizeof(buf), "present exception: %s", ex.what());
        OverlayLog(buf);
        g_ctx->DiscardFrame();
    }
    if (frames < 2) OverlayLog("present end");
    ++frames;
}

void Overlay_SetLog(OverlayLogFn fn) { g_log = fn; }

void ImGui_ImplOpenGL3_SetSkipGLDeletes(bool skip) { g_glSkipDeletes = skip; }

bool Overlay_InitVulkan(const OverlayVkCreate* info) {
    if (!info || !info->device || !info->resolve) return false;
    EnsureContext();
    EnsureFont();
    g_vk.reset(new ui::VulkanRenderer());
    ui::VulkanCreateInfo ci;
    ci.apiVersion = info->apiVersion;
    ci.instance = (VkInstance)info->instance;
    ci.physicalDevice = (VkPhysicalDevice)info->physicalDevice;
    ci.device = (VkDevice)info->device;
    ci.queueFamily = info->queueFamily;
    ci.queue = (VkQueue)info->queue;
    ci.renderPass = (VkRenderPass)info->renderPass;
    ci.loader.resolve = (PFN_vkVoidFunction(*)(const char*, void*))info->resolve;
    ci.loader.user = info->resolveUser;
    if (!g_vk->Init(ci)) {
        g_vk.reset();
        return false;
    }
    g_ctx->SetDrawBackend(g_vk.get());
    return true;
}

void Overlay_ShutdownVulkan() {
    if (g_ctx) g_ctx->SetDrawBackend(nullptr);
    if (g_vk) {
        g_vk->Shutdown();
        g_vk.reset();
    }
}

void Overlay_PrepareVulkan() {
    EnsureFont();
    if (g_vk) g_vk->Prepare(g_fontReady ? &g_fontAtlas : nullptr);
}

void Overlay_RecordVulkan(void* commandBuffer) {
    if (!g_ctx || !g_vk || !commandBuffer) return;
    if (g_io.DisplaySize.x <= 1.f || g_io.DisplaySize.y <= 1.f) {
        g_ctx->DiscardFrame();
        return;
    }
    g_ctx->SetDrawBackend(g_vk.get());
    g_vk->SetCommandBuffer((VkCommandBuffer)commandBuffer);
    g_ctx->Render();
}
