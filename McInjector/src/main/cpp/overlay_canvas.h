#pragma once
// C++11 draw-list facade over aris-ui. Bridge code keeps ImVec2 / ImDrawList
// call shapes; this header does not include aris-ui or glad.

#include <windows.h>

struct ImVec2 {
    float x, y;
    ImVec2() : x(0.f), y(0.f) {}
    ImVec2(float x_, float y_) : x(x_), y(y_) {}
};

typedef unsigned int ImU32;
struct ImFont {};
struct ImGuiContext;
struct ImDrawData;

struct ImFontConfig {
    float RasterizerDensity;
    float SizePixels;
    int OversampleH;
    int OversampleV;
    bool PixelSnapH;
    ImFontConfig()
        : RasterizerDensity(1.f), SizePixels(16.f), OversampleH(1), OversampleV(1), PixelSnapH(false) {}
};

struct ImFontAtlas {
    void Clear();
    ImFont* AddFontDefault(ImFontConfig* config);
    ImFont* AddFontFromFileTTF(const char* filename, float sizePixels, ImFontConfig* config);
};

struct ImGuiStyle {
    float WindowRounding, ChildRounding, FrameRounding, GrabRounding, ScrollbarRounding;
    ImGuiStyle()
        : WindowRounding(0), ChildRounding(0), FrameRounding(0), GrabRounding(0), ScrollbarRounding(0) {}
    void ScaleAllSizes(float) {}
};

struct ImGuiIO {
    ImVec2 MousePos;
    bool MouseDown[5];
    ImVec2 DisplaySize;
    int ConfigFlags;
    const char* IniFilename;
    float FontGlobalScale;
    ImFontAtlas* Fonts;
    ImGuiIO();
};

enum ImGuiConfigFlags_ {
    ImGuiConfigFlags_NavEnableKeyboard = 1 << 0
};

#define IM_COL32_R_SHIFT 0
#define IM_COL32_G_SHIFT 8
#define IM_COL32_B_SHIFT 16
#define IM_COL32_A_SHIFT 24
#define IM_COL32(r, g, b, a) \
    ((ImU32)((ImU32)(r) | ((ImU32)(g) << 8) | ((ImU32)(b) << 16) | ((ImU32)(a) << 24)))

struct ImDrawList {
    void AddLine(const ImVec2& a, const ImVec2& b, ImU32 col, float thickness = 1.f);
    void AddRect(const ImVec2& a, const ImVec2& b, ImU32 col, float rounding = 0.f, int flags = 0, float thickness = 1.f);
    void AddRectFilled(const ImVec2& a, const ImVec2& b, ImU32 col, float rounding = 0.f, int flags = 0);
    void AddTriangle(const ImVec2& a, const ImVec2& b, const ImVec2& c, ImU32 col, float thickness = 1.f);
    void AddTriangleFilled(const ImVec2& a, const ImVec2& b, const ImVec2& c, ImU32 col);
    void AddCircle(const ImVec2& center, float radius, ImU32 col, int numSegments = 0, float thickness = 1.f);
    void AddCircleFilled(const ImVec2& center, float radius, ImU32 col, int numSegments = 0);
    void AddText(const ImVec2& pos, ImU32 col, const char* text);
    void AddText(const ImFont* font, float fontSize, const ImVec2& pos, ImU32 col, const char* textBegin, const char* textEnd = 0);
};

namespace ImGui {
    ImGuiContext* GetCurrentContext();
    ImGuiContext* CreateContext();
    void DestroyContext(ImGuiContext* ctx = 0);
    ImGuiIO& GetIO();
    ImGuiStyle& GetStyle();
    ImDrawList* GetForegroundDrawList();
    ImFont* GetFont();
    float GetFontSize();
    ImVec2 CalcTextSize(const char* text, const char* textEnd = 0, bool hideTextAfterDoubleHash = false, float wrapWidth = -1.f);
    bool IsMouseClicked(int button, bool repeat = false);
    bool IsMouseReleased(int button);
    void NewFrame();
    void Render();
    ImDrawData* GetDrawData();
}

void ImGui_ImplWin32_Init(HWND hwnd);
void ImGui_ImplWin32_InitForOpenGL(HWND hwnd);
void ImGui_ImplWin32_Shutdown();
void ImGui_ImplWin32_NewFrame();
LRESULT ImGui_ImplWin32_WndProcHandler(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam);

bool ImGui_ImplOpenGL3_Init(const char* glslVersion);
void ImGui_ImplOpenGL3_Shutdown();
void ImGui_ImplOpenGL3_NewFrame();
void ImGui_ImplOpenGL3_RenderDrawData(void* drawData);
void ImGui_ImplOpenGL3_SetSkipGLDeletes(bool skip);

struct OverlayVkCreate {
    unsigned apiVersion;
    void* instance;
    void* physicalDevice;
    void* device;
    unsigned queueFamily;
    void* queue;
    void* renderPass;
    void* (*resolve)(const char* name, void* user);
    void* resolveUser;
};

typedef void (*OverlayLogFn)(const char* message);
void Overlay_SetLog(OverlayLogFn fn);

bool Overlay_InitVulkan(const OverlayVkCreate* info);
void Overlay_ShutdownVulkan();
void Overlay_PrepareVulkan();
void Overlay_RecordVulkan(void* commandBuffer);
