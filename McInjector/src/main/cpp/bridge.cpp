#include <winsock2.h>
#include <windows.h>
#include <jni.h>
#include <jvmti.h>
#include <GL/gl.h>
#include <ws2tcpip.h>
#include <string>
#include <vector>
#include <algorithm>
#include <fstream>
#include <sstream>
#include <cstring>
#include <cstdio>
#include <cmath>
#include <cctype>
#include <unordered_map>
#include "gl_loader.h"
#include "MinHook.h"
#include "imgui.h"
#include "imgui_impl_win32.h"
#include "imgui_impl_opengl3.h"
#include "json_config_reader.h"
#include "bridge_capabilities.h"
#include "nick_hider.h"
#include "nick_hider_jvmti.h"
#include "hud_layout.h"
#include "block_esp_common.h"
#include "silent_aura_aim.h"
#include "kill_aura_core.h"
#include "kill_aura_premotion.h"
#include "jni_core/scoped_env.h"
#include "jni_core/local_frame.h"
#include "jni_core/matrix_reader.h"
#include "jni_core/helper_bridge.h"

// Forward declare ImGui Win32 WndProc handler (declaration is in #if 0 block in header)
extern LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

// MinGW's <GL/gl.h> may not declare modern GL enums used while preserving
// Minecraft's render state around ImGui backend initialization.
#ifndef GL_CURRENT_PROGRAM
#define GL_CURRENT_PROGRAM 0x8B8D
#endif
#ifndef GL_ACTIVE_TEXTURE
#define GL_ACTIVE_TEXTURE 0x84E0
#endif
#ifndef GL_TEXTURE_BINDING_2D
#define GL_TEXTURE_BINDING_2D 0x8069
#endif
#ifndef GL_ARRAY_BUFFER
#define GL_ARRAY_BUFFER 0x8892
#endif
#ifndef GL_ELEMENT_ARRAY_BUFFER
#define GL_ELEMENT_ARRAY_BUFFER 0x8893
#endif
#ifndef GL_ARRAY_BUFFER_BINDING
#define GL_ARRAY_BUFFER_BINDING 0x8894
#endif
#ifndef GL_ELEMENT_ARRAY_BUFFER_BINDING
#define GL_ELEMENT_ARRAY_BUFFER_BINDING 0x8895
#endif
#ifndef GL_VERTEX_ARRAY_BINDING
#define GL_VERTEX_ARRAY_BINDING 0x85B5
#endif
#ifndef GL_PIXEL_UNPACK_BUFFER
#define GL_PIXEL_UNPACK_BUFFER 0x88EC
#endif
#ifndef GL_PIXEL_UNPACK_BUFFER_BINDING
#define GL_PIXEL_UNPACK_BUFFER_BINDING 0x88EF
#endif

// Custom extension in our vendored imgui_impl_opengl3.cpp.
void ImGui_ImplOpenGL3_SetSkipGLDeletes(bool skip);
// Custom Mutex for MinGW win32 threads
class Mutex {
    CRITICAL_SECTION cs;
public:
    Mutex() { InitializeCriticalSection(&cs); }
    ~Mutex() { DeleteCriticalSection(&cs); }
    void lock() { EnterCriticalSection(&cs); }
    bool try_lock() { return TryEnterCriticalSection(&cs) != 0; }
    void unlock() { LeaveCriticalSection(&cs); }
};

class LockGuard {
    Mutex& m;
public:
    LockGuard(Mutex& m) : m(m) { m.lock(); }
    ~LockGuard() { m.unlock(); }
};

class TryLockGuard {
    Mutex& m;
    bool owns;
public:
    TryLockGuard(Mutex& m) : m(m), owns(m.try_lock()) {}
    ~TryLockGuard() { if (owns) m.unlock(); }
    bool owns_lock() const { return owns; }
};

// ===================== GLOBALS =====================
JavaVM* g_jvm = nullptr;
bool g_running = true;
// Per-subsystem JNI locks (finer-grained than the old single JNI lock).
// Render thread (SwapBuffers) holds g_renderJniMutex; LegoBridge thread holds
// g_stateJniMutex.  Reach state is shared by the LegoBridge thread and WndProc,
// so both reach paths serialize on g_stateJniMutex.
static Mutex g_renderJniMutex;  // nametags / chest ESP / closest-player (render thread)
static Mutex g_stateJniMutex;   // ReadGameState / reach / velocity (LegoBridge thread)
SOCKET g_serverSocket = INVALID_SOCKET;
SOCKET g_clientSocket = INVALID_SOCKET;
static volatile LONG g_heavyDiscoveryInProgress = 0;

// Rendering
static GLuint g_fontTexture = 0;
static bool g_glInitialized = false;
static bool g_imguiPhase1Done = false;
static bool g_imguiInitialized = false;
static bool g_imguiGlBackendReady = false;
static int g_imguiWarmupFrames = 0;
static HGLRC g_imguiGlrc = nullptr;
static bool g_imguiPendingBackendReset = false;
static HGLRC g_imguiPendingGlrc = nullptr;
static bool g_minhookInitialized = false;
static HWND g_imguiHwnd = nullptr;
static WNDPROC g_origWndProc = nullptr;
static HWND g_gameHwnd = nullptr;
static HWND g_wndProcHookedHwnd = nullptr;

struct Config {
    bool leftClick = true, rightClick = false;
    bool jitter = true;
    bool rightBlockOnly = false;
    bool breakBlocks = false;
    bool clickInChests = false;
    bool aimAssist = false;
    bool triggerbot = false;
    bool speedBridge = false;
    bool speedBridgeBlockOnly = true;
    int speedBridgeDelayMs = 85;
    bool speedBridgeHoldingShiftOnly = true;
    bool speedBridgeLookingDownOnly = true;
    bool gtbHelper = false;
    bool nametags = false;
    bool nickHiderEnabled = false;
    std::string nickHiderAlias;
    bool closestPlayerInfo = false;
    bool nametagShowHealth = true;
    bool nametagShowArmor = true;
    bool nametagHideVanilla = false;
    int nametagMaxCount = 8;
    bool chestEsp = false;
    int chestEspMaxCount = 5;
    bool chestStealer = false;
    int chestStealerDelayMs = 120;
    bool blockEsp = false;
    bool blockEspBoxes = true;
    bool blockEspTracers = false;
    bool blockEspHud = true;
    int blockEspMaxCount = 64;
    int blockEspRange = 4;
    std::string gtbHint;
    int gtbCount = 0;
    std::string gtbPreview;
    bool reachEnabled = false;
    float reachMin = 3.0f;
    float reachMax = 3.0f;
    int reachChance = 100;
    bool velocityEnabled = false;
    int velocityHorizontal = 100;
    int velocityVertical = 100;
    int velocityChance = 100;
    bool antiDebuffEnabled = false;
    bool hitDelayFixEnabled = false;
    bool showModuleList = true;
    int moduleListStyle = 0;
    bool showLogo = true;
    std::string guiTheme = "Default";
    float minCPS = 10, maxCPS = 14;
    float rightMinCPS = 10, rightMaxCPS = 14;
    bool armed = false; // "Self Destruct" / Enable toggle
    bool clicking = false; // Internal state
    // Per-module keybinds (VK codes; 0 = unbound)
    int keybindAutoclicker = 0xC0; // backtick default
    int keybindSpeedBridge = 0;
    int keybindNametags = 0;
    int keybindClosestPlayer = 0;
    int keybindChestEsp = 0;
    int keybindChestStealer = 0;
    int keybindBlockEsp = 0;
    bool pixelPartyAssist = false;
    int pixelPartyScanRadius = 28;
    bool pixelPartyAutoLook = false;
    bool pixelPartyAutoWalk = false;
    bool hudEditor = false;
    bool  killAura = false;
    bool  killAuraRecordCps = false;
    int   killAuraMode = killaura::TARGET_SWITCH;
    int   killAuraSort = killaura::SORT_HEALTH;
    int   killAuraAutoBlock = killaura::BLOCK_HYPIXEL;
    int   killAuraAttackTick = 0;
    bool  killAuraAutoBlockRequirePress = false;
    float killAuraAutoBlockCps = 8.0f;
    float killAuraAutoBlockRange = 6.0f;
    float killAuraSwingRange = 3.5f;
    float killAuraAttackRange = 3.0f;
    int   killAuraFov = 360;
    int   killAuraMinCps = 14;
    int   killAuraMaxCps = 14;
    int   killAuraSwitchDelayMs = 150;
    int   killAuraRotMode = killaura::ROT_SILENT;
    float killAuraDeadZone = 0.5f;
    float killAuraMaxTurnSpeed = 25.0f;
    float killAuraMinTurnSpeed = 5.0f;
    float killAuraAcceleration = 2.5f;
    float killAuraDeceleration = 1.5f;
    bool  killAuraUseOvershoot = true;
    float killAuraOvershootStrength = 5.0f;
    float killAuraOvershootRecovery = 0.2f;
    float killAuraNoiseStrength = 0.2f;
    bool  killAuraVisualizeAim = true;
    bool  killAuraSmoothBack = true;
    int   killAuraMoveFix = killaura::MOVE_FIX_SILENT;
    int   killAuraSmoothing = 0;
    int   killAuraRavenSmoothing = 0;
    int   killAuraRavenPredictTicks = 0;
    int   killAuraRavenYawRandom = 0;
    int   killAuraAngleStep = 90;
    bool  killAuraThroughWalls = true;
    bool  killAuraRequirePress = false;
    bool  killAuraAllowMining = true;
    bool  killAuraWeaponsOnly = true;
    bool  killAuraAllowTools = false;
    bool  killAuraInventoryCheck = true;
    bool  killAuraBotCheck = true;
    bool  killAuraPlayers = true;
    bool  killAuraBosses = false;
    bool  killAuraMobs = false;
    bool  killAuraAnimals = false;
    bool  killAuraGolems = false;
    bool  killAuraSilverfish = false;
    bool  killAuraTeams = true;
    bool  killAuraShowTarget = false;
    bool  killAuraDebugHealth = false;
    bool  killAuraRandomize = true;
    float killAuraRandomizeRange = 0.4f;
    float killAuraYRandomize = 0.3f;
    float killAuraLbHorizontalSpeed = 180.0f;
    float killAuraLbVerticalSpeed = 180.0f;
    float killAuraLbSmooth = 0.5f;
    bool  killAuraLbPredict = true;
    float killAuraLbPredictSize = 1.0f;
    bool  killAuraLbRandomize = true;
    float killAuraLbRandomRange = 0.5f;
    float killAuraLbHorizontalSearch = 0.5f;
    float killAuraLbBodyMin = 0.1f;
    float killAuraLbBodyMax = 0.9f;
};
static Config g_config;
static Mutex g_configMutex;
static lc::HudLayout g_hudLayout = lc::HudLayout::DefaultLayout();
static lc::HudEditorState g_hudEditor;

// ── Block ESP / X-ray shared state (legacy 1.8.9) ──
struct BlockEspData18 { double x, y, z; unsigned int color; double dist; };
static std::vector<BlockEspData18> g_blockEspList;
static Mutex g_blockEspListMutex;
static std::vector<lc::BlockEspTargetDef> g_blockEspTargets;
static Mutex g_blockEspTargetsMutex;
static volatile LONG g_blockEspTargetsVersion = 0;
static DWORD g_lastBlockEspScanMs = 0;

// Game State
struct GameState {
    bool mapped = false;
    bool guiOpen = false;
    std::string screenName;
    std::string actionBar;
    float health = 20.0f;
    double posX = 0, posY = 0, posZ = 0;
    float pitch = 0.0f;
    float rotationYaw = 0.0f;
    bool holdingBlock = false;
    bool lookingAtBlock = false;
    bool lookingAtEntity = false;
    bool lookingAtEntityLatched = false;
    bool breakingBlock = false;
    float attackCooldown = 1.0f;
    float attackCooldownPerTick = 0.08f;
    unsigned long long stateMs = 0;
    std::string chestStealerStateJson;
    std::string killAuraUnavailableReason;
    bool killAuraHasTarget = false;
    bool killAuraBlocking = false;
};
static GameState g_gameState;
static Mutex g_stateMutex;
static Mutex g_socketMutex; // Protects socket writes
static Mutex g_jsonMutex;   // Protects shared JSON buffer
static std::string g_pendingJson; // Data from Render Thread
static unsigned long long g_lastEntitySeenMs = 0;

struct PixelPartySnap {
    bool active = false;
    bool holdingValid = false;
    bool targetFound = false;
    double tx = 0, ty = 0, tz = 0;
    double dist = -1.0;
    float targetYaw = 0.0f;
    std::string colorLabel;
    std::string status;
};
static PixelPartySnap g_pixelPartySnap;
static Mutex g_pixelPartyMutex;
static DWORD g_lastPixelPartyUpdateMs = 0;
// Cached colored-floor Y so the scan stays locked to the platform while the player
// jumps (auto-walk pulses jump to gain speed). Without this the target flickers off
// every jump because the scan band tracked the live player Y.
static int  g_pixelPartyPlatformY = 0;
static bool g_pixelPartyPlatformYValid = false;
static jmethodID g_blockItemGetBlockMethod = nullptr;
static jmethodID g_blockGetUnlocalizedNameMethod = nullptr;
static bool g_loggedPixelPartyResolveFail = false;

// 1.8.9 GTB/action-bar extraction mappings
enum ActionBarFieldKind {
    ActionBarFieldString = 0,
    ActionBarFieldChatComponent = 1
};

struct ActionBarFieldRef {
    jfieldID field = nullptr;
    ActionBarFieldKind kind = ActionBarFieldString;
};

static jfieldID g_ingameGuiField = nullptr;
static jmethodID g_chatComponentGetTextMethod = nullptr;
static std::vector<ActionBarFieldRef> g_actionBarFields;
static DWORD g_lastActionBarProbeMs = 0;
static int g_lastActionBarFieldCountLogged = -1;

struct UiState {
    float accentHue = 0.46f;      // muted teal default
    float accentSat = 0.55f;
    float accentVal = 0.78f;
    bool chromaText = true;
    float chromaSpeed = 0.06f;
};
static UiState g_uiState;

// Font
// Font (High Resolution)
#define CHAR_W 16
#define CHAR_H 32
#define ATLAS_COLS 16
#define ATLAS_ROWS 8
#define ATLAS_W (ATLAS_COLS * CHAR_W) // 256
#define ATLAS_H (ATLAS_ROWS * CHAR_H) // 256
static float g_glyphAdvance[128] = {};

// Hook
typedef BOOL(WINAPI* SwapBuffersFn)(HDC);
static SwapBuffersFn g_origSwapBuffers = nullptr;


// JNI cached refs
static jclass g_mcClass = nullptr;
static jobject g_mcInstance = nullptr;
static jobject g_gameClassLoader = nullptr;
static jfieldID g_thePlayerField = nullptr;
static jfieldID g_currentScreenField = nullptr;
static jmethodID g_getHealthMethod = nullptr;
static jfieldID g_posXField = nullptr, g_posYField = nullptr, g_posZField = nullptr;
static bool g_mapped = false;

// Native GUI handling
static jclass g_guiChatClass = nullptr;
static jmethodID g_guiChatConstructor = nullptr;
static jmethodID g_displayGuiScreenMethod = nullptr;

// Nametags / ESP globals
static jfieldID g_theWorldField = nullptr;
static jfieldID g_playerEntitiesField = nullptr;
static jfieldID g_loadedTileEntityListField = nullptr;
static jmethodID g_getNameMethod = nullptr;
static jmethodID g_getGameProfileMethod = nullptr;
static jclass g_gameProfileClass = nullptr;
static jmethodID g_gameProfileGetNameMethod = nullptr;
static jfieldID g_gameProfileNameField = nullptr;
static std::string g_nickHiderOriginalProfileName;
static bool g_nickHiderProfileAliasApplied = false;
static jmethodID g_legacyMcGetNetHandlerMethod = nullptr;
static jmethodID g_legacyNetHandlerGetPlayerInfoByNameMethod = nullptr;
static jmethodID g_legacyNetworkPlayerInfoGetProfileMethod = nullptr;
static jfieldID g_legacyNetworkPlayerInfoDisplayNameField = nullptr;
static jmethodID g_legacyGetChatGuiMethod = nullptr;
static jfieldID g_legacyChatLinesField = nullptr;
static jfieldID g_legacyDrawnChatLinesField = nullptr;
static jfieldID g_legacyChatLineComponentField = nullptr;
static jfieldID g_legacyChatTextField = nullptr;
static jmethodID g_legacyComponentSiblingsMethod = nullptr;
struct LegacyNickHiderTextMutation { jobject component = nullptr; std::string original; };
static std::vector<LegacyNickHiderTextMutation> g_legacyNickHiderTextMutations;
static bool g_loggedLegacyNickHiderFallback = false;
static bool g_loggedLegacyNickHiderProfileReady = false;
static bool g_loggedLegacyNickHiderTabReady = false;
static bool g_loggedLegacyNickHiderTabDisplayReady = false;
static bool g_loggedLegacyNickHiderChatReady = false;
static bool g_loggedLegacyNickHiderProfileFailure = false;
static bool g_loggedLegacyNickHiderChatFailure = false;
static jmethodID g_worldGetScoreboardMethod = nullptr;
static jclass g_scoreboardClassLegacy = nullptr;
static jclass g_scorePlayerTeamClassLegacy = nullptr;
static jclass g_teamEnumVisibleClassLegacy = nullptr;
static jmethodID g_scoreboardGetTeamMethodLegacy = nullptr;
static jmethodID g_scoreboardCreateTeamMethodLegacy = nullptr;
static jmethodID g_scoreboardRemoveTeamMethodLegacy = nullptr;
static jmethodID g_scoreboardAddPlayerToTeamMethodLegacy = nullptr;
static jmethodID g_scoreboardGetPlayersTeamMethodLegacy = nullptr;
static jmethodID g_scoreboardRemovePlayerFromTeamsMethodLegacy = nullptr;
static jmethodID g_scorePlayerTeamGetRegisteredNameMethodLegacy = nullptr;
static jmethodID g_scorePlayerTeamSetNameTagVisibilityMethodLegacy = nullptr;
static jobject g_teamEnumVisibleNeverLegacy = nullptr;
static bool g_legacyNametagSuppressionActive = false;
static bool g_loggedLegacyNametagSuppressionUnavailable = false;
static std::unordered_map<std::string, std::string> g_hiddenNametagOriginalTeamByPlayerLegacy;
static jobject g_lastLegacyNametagSuppressionWorld = nullptr;
static jmethodID g_objectHashCodeMethod = nullptr;
static jfieldID g_rotationYawField = nullptr;
static jfieldID g_rotationPitchField = nullptr;
static jmethodID g_listSizeMethod = nullptr;
static jmethodID g_listGetMethod = nullptr;
static jclass g_listClass = nullptr; // java/util/List
static jfieldID g_gameSettingsField = nullptr;
static jfieldID g_fovSettingField = nullptr;
static jfieldID g_keyBindSneakField = nullptr;
static jclass g_keyBindingClass = nullptr;
static jmethodID g_keyBindingGetKeyCodeMethod = nullptr;
static jmethodID g_keyBindingSetKeyBindStateMethod = nullptr;
static jfieldID g_keyBindingKeyCodeField = nullptr;
static jfieldID g_keyBindingPressedField = nullptr;
static jclass g_lwjglKeyboardClass = nullptr;
static jmethodID g_keyboardIsKeyDownMethod = nullptr;
static jclass g_lwjglMouseClass = nullptr;
static jmethodID g_mouseIsButtonDownMethod = nullptr;
static jfieldID g_tileEntityPosField = nullptr;
static jclass g_blockPosClass = nullptr;
static jmethodID g_blockPosIntCtor = nullptr;
static jmethodID g_blockPosGetX = nullptr;
static jmethodID g_blockPosGetY = nullptr;
static jmethodID g_blockPosGetZ = nullptr;
static jmethodID g_worldGetBlockStateMethod = nullptr;
static jmethodID g_blockStateGetBlockMethod = nullptr;
static jmethodID g_blockGetMaterialMethod = nullptr;
static jmethodID g_materialIsSolidMethod = nullptr;
static jclass g_tileEntityChestClass = nullptr;
static jclass g_tileEntityEnderChestClass = nullptr;
static jfieldID g_playerControllerField = nullptr;
static jmethodID g_windowClickMethod = nullptr;
static jclass g_guiChestClass = nullptr;
static jfieldID g_guiContainerInventorySlotsField = nullptr;
static jfieldID g_containerWindowIdField = nullptr;
static jfieldID g_containerInventorySlotsField = nullptr;
static jmethodID g_slotGetHasStackMethod = nullptr;
static jfieldID g_slotSlotNumberField = nullptr;
static jfieldID g_slotXDisplayPositionField = nullptr;
static jfieldID g_slotYDisplayPositionField = nullptr;
static jfieldID g_guiLeftField = nullptr;
static jfieldID g_guiTopField = nullptr;
static jfieldID g_guiWidthField = nullptr;
static jfieldID g_guiHeightField = nullptr;

// Interpolation globals
static jfieldID g_timerField = nullptr;
static jfieldID g_renderPartialTicksField = nullptr;
static jfieldID g_lastTickPosXField = nullptr;
static jfieldID g_lastTickPosYField = nullptr;
static jfieldID g_lastTickPosZField = nullptr;

// Inventory / Held Item Globals
static jfieldID  g_inventoryField          = nullptr; // EntityPlayer.inventory
static jmethodID g_getCurrentItemMethod    = nullptr; // InventoryPlayer.getCurrentItem()
static jclass    g_itemBlockClass          = nullptr; // ItemBlock class (instanceof check)
static jmethodID g_getHeldItemMethod       = nullptr; // EntityLivingBase.getHeldItem()
static jmethodID g_getTotalArmorValueMethod= nullptr; // EntityPlayer.getTotalArmorValue()
static jclass    g_itemSwordClass          = nullptr; // ItemSword class (instanceof check)
static jmethodID g_getRenderItemFromMcMethod          = nullptr;
static jmethodID g_renderItemAndEffectIntoGUIMethod   = nullptr;
static jmethodID g_renderItemIntoGUIMethod            = nullptr;

static jfieldID g_objectMouseOverField = nullptr;
static jfieldID g_leftClickCounterField = nullptr;
static jfieldID g_pointedEntityField = nullptr;
static jclass g_movingObjectPositionClass = nullptr;
static jfieldID g_typeOfHitField = nullptr;
static jmethodID g_enumNameMethod = nullptr;
static jfieldID g_entityHitField = nullptr;
static jclass g_movingObjectTypeClass = nullptr;
static jobject g_mopEntityTypeConst = nullptr;
static jmethodID g_mopEntityCtor = nullptr;
static bool g_mopCtorNeedsVec3 = false;
static jclass g_vec3Class = nullptr;
static jmethodID g_vec3Ctor = nullptr;
static jfieldID g_motionXField = nullptr;
static jfieldID g_motionYField = nullptr;
static jfieldID g_motionZField = nullptr;
static jfieldID g_hurtTimeField = nullptr;

// KillAura (legacy 1.8.9) — OpenMyau-Plus aligned packet order:
// Credit: https://github.com/IamNespola/OpenMyau-Plus
// swingItem (C0A) → syncCurrentPlayItem → C02 ATTACK → local hit, fired pre-C03 via Premotion.
static jmethodID g_killAuraSwingMethod = nullptr;          // EntityPlayer.swingItem
static jmethodID g_killAuraSyncItemMethod = nullptr;       // PlayerControllerMP.syncCurrentPlayItem
static jmethodID g_killAuraAttackLocalMethod = nullptr;    // EntityPlayer.attackTargetEntityWithCurrentItem
static jmethodID g_killAuraAddToSendQueue = nullptr;       // NetHandlerPlayClient.addToSendQueue
static jclass    g_killAuraUseEntityClass = nullptr;       // C02PacketUseEntity
static jmethodID g_killAuraUseEntityCtor = nullptr;        // (Entity, Action)
static jobject   g_killAuraAttackAction = nullptr;         // Action.ATTACK global ref
static jclass    g_killAuraC07Class = nullptr;
static jmethodID g_killAuraC07Ctor = nullptr;
static jobject   g_killAuraReleaseAction = nullptr;
static jobject   g_killAuraOriginPos = nullptr;
static jobject   g_killAuraFacingDown = nullptr;
static jclass    g_killAuraC08Class = nullptr;
static jmethodID g_killAuraC08Ctor = nullptr;
static jclass    g_killAuraC09Class = nullptr;
static jmethodID g_killAuraC09Ctor = nullptr;
static jmethodID g_killAuraSetItemInUse = nullptr;
static jmethodID g_killAuraStopUsingItem = nullptr;
static jmethodID g_killAuraIsUsingItem = nullptr;
static jfieldID  g_killAuraCurrentItemField = nullptr;
static jmethodID g_killAuraGetStackInSlot = nullptr;
static jfieldID  g_killAuraAttackKeyField = nullptr;
static jfieldID  g_killAuraUseKeyField = nullptr;
static jmethodID g_killAuraKeyIsDown = nullptr;
static jmethodID g_killAuraCanSeeEntity = nullptr;
static jmethodID g_killAuraIsSameTeam = nullptr;
static killaura::AutoBlockState g_killAuraAutoBlockState;
static std::string g_killAuraUnavailableReason;
static Mutex g_killAuraUnavailableMutex;
static bool g_killAuraHasTarget = false;
static bool g_killAuraBlocking = false;
static jclass    g_killAuraC03Class = nullptr;             // C03PacketPlayer (global)
static jfieldID  g_killAuraC03Yaw = nullptr;
static jfieldID  g_killAuraC03Pitch = nullptr;
static jfieldID  g_killAuraC03Rotating = nullptr;
static bool      g_killAuraSendQueueArmed = false;
static jfieldID  g_rotationYawHeadField = nullptr;
static jfieldID  g_renderYawOffsetField = nullptr;
static jfieldID  g_mouseSensitivityField = nullptr;
static jfieldID  g_onGroundField = nullptr;
static float     g_killAuraMouseSens = 0.5f;
static bool      g_killAuraSensResolved = false;
static bool      g_loggedKillAuraAttackFail = false;
static bool      g_loggedKillAuraSilentNoPremotion = false;
static bool      g_loggedKillAuraClickSilentIncompat = false;
static DWORD     g_killAuraLastScanMs = 0;
static DWORD     g_killAuraLastAttackMs = 0;
static DWORD     g_killAuraNextIntervalMs = 0;
static int       g_killAuraAttackDelayMs = 0;
static size_t    g_killAuraPatternIndex = 0;
static std::string g_killAuraCurrentTarget;
static DWORD     g_killAuraLastSwitchMs = 0;
static float     g_killAuraCombatYaw = 0.0f;
static float     g_killAuraCombatPitch = 0.0f;
static bool      g_killAuraCombatValid = false;
static float     g_killAuraYawVel = 0.0f;
static float     g_killAuraPitchVel = 0.0f;
static float     g_killAuraBodyYaw = 0.0f;
static bool      g_killAuraBodyValid = false;
static bool      g_killAuraOvershootActive = false;
static float     g_killAuraOvershootYaw = 0.0f;
static float     g_killAuraOvershootPitch = 0.0f;
static float     g_killAuraAimFracX = 0.5f;
static float     g_killAuraAimFracY = 0.55f;
static float     g_killAuraAimFracZ = 0.5f;
static DWORD     g_killAuraNextAimPointMs = 0;
static double    g_killAuraPrevTX = 0.0;
static double    g_killAuraPrevTY = 0.0;
static double    g_killAuraPrevTZ = 0.0;
static bool      g_killAuraPrevValid = false;
static DWORD     g_killAuraNextResolveMs = 0;
static DWORD     g_killAuraNextPremotionRefreshMs = 0;
static int g_lastHurtTime = 0;

// ---- AntiDebuff (module-owned; removes debuff visuals client-side) ----
// 1.8.9: EntityLivingBase.removePotionEffect(int id). Potion IDs: blindness=15, nausea/confusion=9.
static jmethodID g_removePotionEffectMethod = nullptr;
static bool g_antiDebuffMethodResolved = false;
static bool g_loggedAntiDebuffResolveFail = false;
static DWORD g_lastAntiDebuffTickMs = 0;          // Throttle to ~20 tps (matches bridge_261)
// AntiDebuff approach A: BLINDNESS is never removed (keeps sprint/crit gating in sync with
// the server). Instead we detect it on the bg JNI thread and tell the glFogf render hook to
// neutralize the blindness fog. g_blindFogSuppress is written by the bg thread and read by
// the render thread, hence the interlocked LONG.
static volatile LONG g_blindFogSuppress = 0;      // 1 while blindness fog should be hidden
static jmethodID g_isPotionActiveMethod = nullptr; // EntityLivingBase.isPotionActive(Potion)
static jobject   g_blindnessPotion = nullptr;      // global ref to static Potion.blindness
static bool g_blindDetectResolved = false;
static bool g_loggedBlindDetectFail = false;
static bool g_reachClickPrevDown = false;
static bool g_reachClickPrevSynthetic = false;
static bool g_reachRawInputPrevDown = false;
static bool g_reachAllowCurrentClick = false;
static double g_reachCurrentClickRange = 3.0;
static DWORD g_reachClickWindowUntilMs = 0;
static DWORD g_reachLastRollMs = 0;
static DWORD g_lastReachDebugLogMs = 0;
static jobject g_reachCurrentTarget = nullptr;
static bool g_speedBridgeManagingSneak = false;
static bool g_speedBridgeHaveLastPos = false;
static double g_speedBridgeLastPosX = 0.0;
static double g_speedBridgeLastPosZ = 0.0;
static int g_speedBridgeDirX = 0;
static int g_speedBridgeDirZ = 0;
static DWORD g_chestStealerNextClickMs = 0;
static int g_chestStealerWindowId = -1;
static int g_chestStealerLastSlotCount = -1;
static DWORD g_chestStealerWindowOpenedMs = 0;
static bool g_chestStealerWindowCompleted = false;
static std::vector<int> g_chestStealerSlots;
static unsigned int g_chestStealerRng = 0xA0C0123u;
static DWORD g_lastChestStealerMappingLogMs = 0;
static DWORD g_lastChestStealerSkipLogMs = 0;
// NOTE: g_getItemMethod, g_getDisplayNameMethod, g_getUnlocalizedNameMethod,
// g_getDamageVsEntityMethod are intentionally NOT cached globally — they must
// be fetched from the actual object's class each call to avoid calling a
// method ID from the wrong class (causes JVM crash when entity classes differ).

// OpenGL Matrix Globals
static jclass g_activeRenderInfoClass = nullptr;
static jfieldID g_modelViewField = nullptr;
static jfieldID g_projectionField = nullptr;
static jmethodID g_floatBufferGet = nullptr; // FloatBuffer.get(I)F

struct Matrix4x4 {
    float m[16];
};
static Matrix4x4 g_lastCapturedModelView = {0};
static Matrix4x4 g_lastCapturedProjection = {0};
static bool g_hasCapturedRenderMatrices = false;

// RenderManager Globals
static jfieldID g_renderManagerField = nullptr; // Minecraft.renderManager
static jfieldID g_viewerPosXField = nullptr;
static jfieldID g_viewerPosYField = nullptr;
static jfieldID g_viewerPosZField = nullptr;

struct TagSmoothingState {
    float x = 0.0f;
    float y = 0.0f;
    float vx = 0.0f;
    float vy = 0.0f;
    int lastFrame = 0;
    bool init = false;
};
static std::unordered_map<int, TagSmoothingState> g_tagSmoothing;
static int g_tagFrameCounter = 0;


static std::string g_logPath = "bridge_debug.log";
static bool g_privateMinecraftiaLoaded = false;
static std::string g_privateMinecraftiaPath;
static bool g_traceEnabled = false;

// Forward declarations for player-name filtering helpers used by reach/telemetry paths.
static bool LooksLikeFakePlayerLine(const std::string& rawName);
static std::string GetStablePlayerName(JNIEnv* env, jobject playerObj);

// ===================== LOGGING =====================
void InitLogPath(HMODULE hModule) {
    char dllPath[MAX_PATH] = {};
    if (!GetModuleFileNameA(hModule, dllPath, MAX_PATH)) {
        return;
    }

    std::string path = dllPath;
    size_t sep = path.find_last_of("\\/");
    if (sep == std::string::npos) {
        return;
    }
    g_logPath = path.substr(0, sep + 1) + "bridge_debug.log";

    char traceEnv[16] = {};
    DWORD traceLen = GetEnvironmentVariableA("LC_BRIDGE_TRACE", traceEnv, sizeof(traceEnv));
    if (traceLen > 0) {
        g_traceEnabled = (traceEnv[0] == '1'
            || traceEnv[0] == 'y' || traceEnv[0] == 'Y'
            || traceEnv[0] == 't' || traceEnv[0] == 'T');
    }
}

void Log(const std::string& msg) {
    std::ofstream f(g_logPath.c_str(), std::ios_base::app);
    f << "[Bridge] " << msg << std::endl;
}

static void TraceValue(const char* fn, const char* key, const std::string& value) {
    if (!g_traceEnabled) return;
    Log(std::string("TRACE|") + (fn ? fn : "?") + "|" + (key ? key : "?") + "|" + value);
}

static void TraceBranch(const char* fn, const char* branch, bool taken) {
    if (!g_traceEnabled) return;
    Log(std::string("TRACE|") + (fn ? fn : "?") + "|" + (branch ? branch : "?") + "|" + (taken ? "1" : "0"));
}

static bool TraceDecision(const char* fn, const char* branch, bool decision) {
    TraceBranch(fn, branch, decision);
    return decision;
}

#define TRACE_PATH(pathValue) TraceValue(__FUNCTION__, "path", (pathValue))
#define TRACE_VALUE(key, value) TraceValue(__FUNCTION__, (key), (value))
#define TRACE_BRANCH(branch, taken) TraceBranch(__FUNCTION__, (branch), (taken))
#define TRACE_IF(branch, expr) TraceDecision(__FUNCTION__, (branch), (expr))

static std::string GetBridgeDir() {
    size_t sep = g_logPath.find_last_of("\\/");
    if (sep == std::string::npos) return ".";
    return g_logPath.substr(0, sep);
}

static bool FileExistsLocal(const std::string& path) {
    std::ifstream f(path.c_str(), std::ios::binary);
    return f.good();
}

static bool IsLikelyFontBinary(const std::string& path) {
    std::ifstream f(path.c_str(), std::ios::binary);
    if (!f.good()) return false;
    unsigned char hdr[4] = { 0 };
    f.read((char*)hdr, 4);
    if (f.gcount() < 4) return false;
    if (hdr[0] == 0x00 && hdr[1] == 0x01 && hdr[2] == 0x00 && hdr[3] == 0x00) return true;
    if (hdr[0] == 'O' && hdr[1] == 'T' && hdr[2] == 'T' && hdr[3] == 'O') return true;
    if (hdr[0] == 't' && hdr[1] == 'r' && hdr[2] == 'u' && hdr[3] == 'e') return true;
    if (hdr[0] == 't' && hdr[1] == 't' && hdr[2] == 'c' && hdr[3] == 'f') return true;
    return false;
}

static void LoadMinecraftiaPrivateFont() {
    if (g_privateMinecraftiaLoaded) return;

    std::string bridgeDir = GetBridgeDir();
    std::vector<std::string> candidates = {
        bridgeDir + "\\minecraftia.ttf",
        bridgeDir + "\\Minecraftia.ttf",
        bridgeDir + "\\Data\\minecraftia.ttf",
        bridgeDir + "\\Data\\Minecraftia.ttf",
        "C:\\Windows\\Fonts\\minecraftia.ttf",
        "C:\\Windows\\Fonts\\Minecraftia.ttf"
    };

    for (const std::string& path : candidates) {
        if (!FileExistsLocal(path)) continue;
        if (!IsLikelyFontBinary(path)) {
            Log("Skipping invalid Minecraftia font candidate: " + path);
            continue;
        }

        int loadedCount = AddFontResourceExA(path.c_str(), FR_PRIVATE, nullptr);
        if (loadedCount > 0) {
            g_privateMinecraftiaLoaded = true;
            g_privateMinecraftiaPath = path;
            Log("Loaded private Minecraftia font: " + path);
            return;
        }
    }

    Log("Minecraftia font file not found or failed to register privately; using system fallback.");
}

static void UnloadMinecraftiaPrivateFont() {
    if (!g_privateMinecraftiaLoaded || g_privateMinecraftiaPath.empty()) return;
    if (RemoveFontResourceExA(g_privateMinecraftiaPath.c_str(), FR_PRIVATE, nullptr)) {
        Log("Unloaded private Minecraftia font: " + g_privateMinecraftiaPath);
    }
    g_privateMinecraftiaLoaded = false;
    g_privateMinecraftiaPath.clear();
}

std::string JsonEscape(const std::string& s) {
    std::string out;
    out.reserve(s.size() + 8);
    for (char c : s) {
        switch (c) {
        case '\\': out += "\\\\"; break;
        case '"': out += "\\\""; break;
        case '\n': out += "\\n"; break;
        case '\r': out += "\\r"; break;
        case '\t': out += "\\t"; break;
        default: out += c; break;
        }
    }
    return out;
}

// ===================== JNI HELPERS =====================
jobject GetGameClassLoader(JNIEnv* env) {
    jclass cThread = env->FindClass("java/lang/Thread");
    jmethodID mGetAll = env->GetStaticMethodID(cThread, "getAllStackTraces", "()Ljava/util/Map;");
    jobject map = env->CallStaticObjectMethod(cThread, mGetAll);
    jclass cMap = env->FindClass("java/util/Map");
    jobject set = env->CallObjectMethod(map, env->GetMethodID(cMap, "keySet", "()Ljava/util/Set;"));
    jclass cSet = env->FindClass("java/util/Set");
    jobjectArray threads = (jobjectArray)env->CallObjectMethod(set, env->GetMethodID(cSet, "toArray", "()[Ljava/lang/Object;"));
    jsize count = env->GetArrayLength(threads);
    jmethodID mName = env->GetMethodID(cThread, "getName", "()Ljava/lang/String;");
    jmethodID mGetCL = env->GetMethodID(cThread, "getContextClassLoader", "()Ljava/lang/ClassLoader;");
    for (int i = 0; i < count; i++) {
        jobject t = env->GetObjectArrayElement(threads, i);
        jstring jn = (jstring)env->CallObjectMethod(t, mName);
        const char* cn = env->GetStringUTFChars(jn, nullptr);
        bool isClient = strstr(cn, "Client thread") != nullptr;
        env->ReleaseStringUTFChars(jn, cn);
        if (isClient) return env->CallObjectMethod(t, mGetCL);
    }
    return nullptr;
}

jobject EnsureGameClassLoader(JNIEnv* env) {
    if (!env) return nullptr;
    if (g_gameClassLoader) return g_gameClassLoader;

    jobject local = GetGameClassLoader(env);
    if (!local) return nullptr;

    g_gameClassLoader = env->NewGlobalRef(local);
    env->DeleteLocalRef(local);
    if (g_gameClassLoader) {
        Log("Cached game classloader");
    }
    return g_gameClassLoader;
}

std::string GetClassNameFromClass(JNIEnv* env, jclass cls) {
    if (!env || !cls) return "";
    jclass cClass = env->FindClass("java/lang/Class");
    if (!cClass) { if (env->ExceptionCheck()) env->ExceptionClear(); return ""; }
    jmethodID m = env->GetMethodID(cClass, "getName", "()Ljava/lang/String;");
    if (!m) { env->DeleteLocalRef(cClass); if (env->ExceptionCheck()) env->ExceptionClear(); return ""; }
    jstring jn = (jstring)env->CallObjectMethod(cls, m);
    env->DeleteLocalRef(cClass);
    if (!jn) { if (env->ExceptionCheck()) env->ExceptionClear(); return ""; }
    const char* cn = env->GetStringUTFChars(jn, nullptr);
    std::string r = cn ? cn : "";
    if (cn) env->ReleaseStringUTFChars(jn, cn);
    env->DeleteLocalRef(jn);
    return r;
}

jclass LoadClassWithLoader(JNIEnv* env, jobject cl, const char* name) {
    jclass cCL = env->FindClass("java/lang/ClassLoader");
    jmethodID m = env->GetMethodID(cCL, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;");
    std::string dn = name; std::replace(dn.begin(), dn.end(), '/', '.');
    jstring jdn = env->NewStringUTF(dn.c_str());
    jclass cls = (jclass)env->CallObjectMethod(cl, m, jdn);
    env->DeleteLocalRef(jdn);
    env->DeleteLocalRef(cCL);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return nullptr; }
    return cls;
}

// ===================== CLASS DISCOVERY =====================
bool DiscoverMappings(JNIEnv* env) {
    TRACE_PATH("enter");
    Log("Starting dynamic class discovery...");
    jvmtiEnv* jvmti = nullptr;
    bool jvmtiReady = TRACE_IF("jvmtiReady", (g_jvm->GetEnv((void**)&jvmti, JVMTI_VERSION_1_2) == JNI_OK && jvmti));
    if (!jvmtiReady) {
        Log("ERROR: Failed to get JVMTI"); return false;
    }
    jint classCount = 0; jclass* classes = nullptr;
    jvmti->GetLoadedClasses(&classCount, &classes);
    Log("Loaded classes: " + std::to_string(classCount));

    jobject gcl = EnsureGameClassLoader(env);
    TRACE_BRANCH("gameClassLoaderAvailable", gcl != nullptr);
    if (!gcl) { Log("ERROR: No game classloader"); return false; }

    jclass cClass = env->FindClass("java/lang/Class");
    jmethodID mGetName = env->GetMethodID(cClass, "getName", "()Ljava/lang/String;");
    jmethodID mGetFields = env->GetMethodID(cClass, "getDeclaredFields", "()[Ljava/lang/reflect/Field;");
    jmethodID mGetMethods = env->GetMethodID(cClass, "getDeclaredMethods", "()[Ljava/lang/reflect/Method;");
    jmethodID mGetSuper = env->GetMethodID(cClass, "getSuperclass", "()Ljava/lang/Class;");
    
    jclass cField = env->FindClass("java/lang/reflect/Field");
    jmethodID mFType = env->GetMethodID(cField, "getType", "()Ljava/lang/Class;");
    jmethodID mFName = env->GetMethodID(cField, "getName", "()Ljava/lang/String;");
    jmethodID mFMod = env->GetMethodID(cField, "getModifiers", "()I");
    
    jclass cMethod = env->FindClass("java/lang/reflect/Method");
    jmethodID mMName = env->GetMethodID(cMethod, "getName", "()Ljava/lang/String;");
    jmethodID mMRet = env->GetMethodID(cMethod, "getReturnType", "()Ljava/lang/Class;");
    jmethodID mMParams = env->GetMethodID(cMethod, "getParameterTypes", "()[Ljava/lang/Class;");
    jmethodID mMMod = env->GetMethodID(cMethod, "getModifiers", "()I");
    
    jclass cMod = env->FindClass("java/lang/reflect/Modifier");
    jmethodID mIsStatic = env->GetStaticMethodID(cMod, "isStatic", "(I)Z");

    // Try known names first
    jclass mcClass = nullptr;
    std::string mcName;
    const char* known[] = {"net.minecraft.client.Minecraft", nullptr};
    for (int i = 0; known[i]; i++) {
        jclass c = LoadClassWithLoader(env, gcl, known[i]);
        TRACE_BRANCH("mcLookupKnownNameHit", c != nullptr);
        if (c) { mcClass = c; mcName = known[i]; TRACE_VALUE("mcClassSource", "known-name"); Log("Found MC by name: " + mcName); break; }
    }

    // Scan for singleton pattern
    TRACE_BRANCH("needSingletonScanForMcClass", mcClass == nullptr);
    if (!mcClass) {
        TRACE_PATH("scan-for-mc-singleton");
        for (int i = 0; i < classCount; i++) {
            jclass cls = classes[i];
            if (env->ExceptionCheck()) { env->ExceptionClear(); continue; }
            jstring jn = (jstring)env->CallObjectMethod(cls, mGetName);
            if (!jn || env->ExceptionCheck()) { env->ExceptionClear(); continue; }
            const char* cn = env->GetStringUTFChars(jn, nullptr);
            std::string name = cn; env->ReleaseStringUTFChars(jn, cn);
            if (name.find("java.") == 0 || name.find("sun.") == 0 || name.find("javax.") == 0 ||
                name.find("com.sun.") == 0 || name.find("org.") == 0 || name.find("jdk.") == 0 ||
                name.find("com.google.") == 0 || name.find("io.") == 0 || name[0] == '[') continue;

            jobjectArray fields = (jobjectArray)env->CallObjectMethod(cls, mGetFields);
            if (!fields || env->ExceptionCheck()) { env->ExceptionClear(); continue; }
            jsize fc = env->GetArrayLength(fields);
            bool hasSelf = false; int objCount = 0;
            for (int f = 0; f < fc; f++) {
                jobject fld = env->GetObjectArrayElement(fields, f);
                if (!fld) continue;
                jint mod = env->CallIntMethod(fld, mFMod);
                bool isS = env->CallStaticBooleanMethod(cMod, mIsStatic, mod);
                jclass ft = (jclass)env->CallObjectMethod(fld, mFType);
                if (!ft || env->ExceptionCheck()) { env->ExceptionClear(); continue; }
                std::string tn = GetClassNameFromClass(env, ft);
                if (isS && tn == name) hasSelf = true;
                if (!isS) objCount++;
            }
            if (hasSelf && objCount > 15) {
                mcClass = (jclass)env->NewGlobalRef(cls);
                mcName = name;
                TRACE_VALUE("mcClassSource", "singleton-scan");
                Log("Found MC class: " + name + " fields=" + std::to_string(objCount));
                break;
            }
        }
    }
    if (!mcClass) { Log("ERROR: MC class not found"); jvmti->Deallocate((unsigned char*)classes); return false; }

    // Find singleton field
    jobjectArray mcFields = (jobjectArray)env->CallObjectMethod(mcClass, mGetFields);
    jsize mcFC = env->GetArrayLength(mcFields);
    jfieldID singletonField = nullptr;
    std::string playerType;

    for (int f = 0; f < mcFC; f++) {
        jobject fld = env->GetObjectArrayElement(mcFields, f);
        if (!fld) continue;
        jint mod = env->CallIntMethod(fld, mFMod);
        bool isS = env->CallStaticBooleanMethod(cMod, mIsStatic, mod);
        jclass ft = (jclass)env->CallObjectMethod(fld, mFType);
        if (!ft || env->ExceptionCheck()) { env->ExceptionClear(); continue; }
        std::string tn = GetClassNameFromClass(env, ft);
        jstring jfn = (jstring)env->CallObjectMethod(fld, mFName);
        const char* cfn = env->GetStringUTFChars(jfn, nullptr);
        std::string fn = cfn; env->ReleaseStringUTFChars(jfn, cfn);

        if (isS && tn == mcName) {
            std::string sig = "L" + mcName + ";"; std::replace(sig.begin(), sig.end(), '.', '/');
            singletonField = env->GetStaticFieldID(mcClass, fn.c_str(), sig.c_str());
            if (env->ExceptionCheck()) env->ExceptionClear();
            Log("Singleton: " + fn);
        }
    }
    TRACE_BRANCH("singletonFieldResolved", singletonField != nullptr);
    if (!singletonField) { Log("ERROR: No singleton"); jvmti->Deallocate((unsigned char*)classes); return false; }

    jobject mcInst = env->GetStaticObjectField(mcClass, singletonField);
    TRACE_BRANCH("mcInstanceAvailable", mcInst != nullptr);
    if (!mcInst) { Log("ERROR: MC null"); jvmti->Deallocate((unsigned char*)classes); return false; }
    Log("Got MC instance");

    // Find player & screen fields
    for (int f = 0; f < mcFC; f++) {
        jobject fld = env->GetObjectArrayElement(mcFields, f);
        if (!fld) continue;
        jint mod = env->CallIntMethod(fld, mFMod);
        if (env->CallStaticBooleanMethod(cMod, mIsStatic, mod)) continue;
        jclass ft = (jclass)env->CallObjectMethod(fld, mFType);
        if (!ft || env->ExceptionCheck()) { env->ExceptionClear(); continue; }
        std::string tn = GetClassNameFromClass(env, ft);
        jstring jfn = (jstring)env->CallObjectMethod(fld, mFName);
        const char* cfn = env->GetStringUTFChars(jfn, nullptr);
        std::string fn = cfn; env->ReleaseStringUTFChars(jfn, cfn);

        jclass walk = ft; int depth = 0;
        bool isPlayer = false, isScreen = false;
        while (walk && depth < 10) {
            jobjectArray methods = (jobjectArray)env->CallObjectMethod(walk, mGetMethods);
            if (methods && !env->ExceptionCheck()) {
                jsize mc2 = env->GetArrayLength(methods);
                for (int m2 = 0; m2 < mc2; m2++) {
                    jobject mth = env->GetObjectArrayElement(methods, m2);
                    if (!mth) continue;
                    jstring jmn = (jstring)env->CallObjectMethod(mth, mMName);
                    const char* cmn = env->GetStringUTFChars(jmn, nullptr);
                    jclass rt = (jclass)env->CallObjectMethod(mth, mMRet);
                    std::string rtn = rt ? GetClassNameFromClass(env, rt) : "";
                    if (std::string(cmn) == "getHealth" && rtn == "float") isPlayer = true;
                    if (std::string(cmn) == "drawScreen" || std::string(cmn) == "drawDefaultBackground") isScreen = true;
                    env->ReleaseStringUTFChars(jmn, cmn);
                }
            }
            if (env->ExceptionCheck()) env->ExceptionClear();
            walk = (jclass)env->CallObjectMethod(walk, mGetSuper);
            if (env->ExceptionCheck()) { env->ExceptionClear(); break; }
            depth++;
        }
        if (isPlayer && !g_thePlayerField) {
            std::string sig = "L" + tn + ";"; std::replace(sig.begin(), sig.end(), '.', '/');
            g_thePlayerField = env->GetFieldID(mcClass, fn.c_str(), sig.c_str());
            if (env->ExceptionCheck()) env->ExceptionClear();
            playerType = tn;
            Log("Player field: " + fn + " type=" + tn);
        }
        if (isScreen && !g_currentScreenField) {
            std::string sig = "L" + tn + ";"; std::replace(sig.begin(), sig.end(), '.', '/');
            g_currentScreenField = env->GetFieldID(mcClass, fn.c_str(), sig.c_str());
            if (env->ExceptionCheck()) env->ExceptionClear();
            Log("Screen field: " + fn + " type=" + tn);
        }
    }

    // Forge production jars keep the 1.8.9 SRG names, so their player class
    // does not advertise the deobfuscated getHealth name used by the heuristic
    // above. Resolve the stable Minecraft fields directly before giving up.
    if (!g_thePlayerField) {
        g_thePlayerField = env->GetFieldID(mcClass, "thePlayer", "Lnet/minecraft/client/entity/EntityPlayerSP;");
        if (!g_thePlayerField) {
            env->ExceptionClear();
            g_thePlayerField = env->GetFieldID(mcClass, "field_71439_g", "Lnet/minecraft/client/entity/EntityPlayerSP;");
        }
        if (g_thePlayerField) {
            playerType = "net.minecraft.client.entity.EntityPlayerSP";
            Log("Player field: resolved direct 1.8.9/Forge mapping");
        } else {
            env->ExceptionClear();
        }
    }
    if (!g_currentScreenField) {
        g_currentScreenField = env->GetFieldID(mcClass, "currentScreen", "Lnet/minecraft/client/gui/GuiScreen;");
        if (!g_currentScreenField) {
            env->ExceptionClear();
            g_currentScreenField = env->GetFieldID(mcClass, "field_71462_r", "Lnet/minecraft/client/gui/GuiScreen;");
        }
        if (g_currentScreenField) {
            Log("Screen field: resolved direct 1.8.9/Forge mapping");
        } else {
            env->ExceptionClear();
        }
    }

    // Find getHealth and position fields on player hierarchy
    if (g_thePlayerField && !playerType.empty()) {
        TRACE_VALUE("playerType", playerType);
        jclass pc = LoadClassWithLoader(env, gcl, playerType.c_str());
        if (!pc) { std::string sn = playerType; std::replace(sn.begin(), sn.end(), '.', '/');
            pc = env->FindClass(sn.c_str()); if (env->ExceptionCheck()) env->ExceptionClear(); }
        jclass wc = pc; int d = 0;
        while (wc && d < 10) {
            std::string wcn = GetClassNameFromClass(env, wc);
            if (!g_getHealthMethod) {
                jobjectArray ms = (jobjectArray)env->CallObjectMethod(wc, mGetMethods);
                if (ms && !env->ExceptionCheck()) {
                    jsize mc3 = env->GetArrayLength(ms);
                    for (int m3 = 0; m3 < mc3; m3++) {
                        jobject mt = env->GetObjectArrayElement(ms, m3);
                        if (!mt) continue;
                        jstring jmn = (jstring)env->CallObjectMethod(mt, mMName);
                        const char* cmn = env->GetStringUTFChars(jmn, nullptr);
                        jclass rt = (jclass)env->CallObjectMethod(mt, mMRet);
                        std::string rtn = rt ? GetClassNameFromClass(env, rt) : "";
                        jobjectArray ps = (jobjectArray)env->CallObjectMethod(mt, mMParams);
                        jsize pc2 = ps ? env->GetArrayLength(ps) : 0;
                        if ((std::string(cmn) == "getHealth" || std::string(cmn) == "func_110143_aJ") && rtn == "float" && pc2 == 0) {
                            g_getHealthMethod = env->GetMethodID(wc, cmn, "()F");
                            if (env->ExceptionCheck()) env->ExceptionClear();
                            if (g_getHealthMethod) Log("getHealth() in " + wcn);
                        }
                        env->ReleaseStringUTFChars(jmn, cmn);
                    }
                }
                if (env->ExceptionCheck()) env->ExceptionClear();
            }
            if (!g_posXField || !g_posYField || !g_posZField) {
                // Prefer canonical field names (including inherited Entity fields) over reflection-order fallbacks.
                if (!g_posXField) {
                    g_posXField = env->GetFieldID(wc, "posX", "D");
                    TRACE_BRANCH("posXDirectCanonicalHit", g_posXField != nullptr);
                    if (!g_posXField) {
                        env->ExceptionClear();
                        g_posXField = env->GetFieldID(wc, "field_70165_t", "D");
                        TRACE_BRANCH("posXDirectObfHit", g_posXField != nullptr);
                    }
                    if (!g_posXField) env->ExceptionClear();
                    else Log("posX: resolved by direct lookup");
                }
                if (!g_posYField) {
                    g_posYField = env->GetFieldID(wc, "posY", "D");
                    TRACE_BRANCH("posYDirectCanonicalHit", g_posYField != nullptr);
                    if (!g_posYField) {
                        env->ExceptionClear();
                        g_posYField = env->GetFieldID(wc, "field_70163_u", "D");
                        TRACE_BRANCH("posYDirectObfHit", g_posYField != nullptr);
                    }
                    if (!g_posYField) env->ExceptionClear();
                    else Log("posY: resolved by direct lookup");
                }
                if (!g_posZField) {
                    g_posZField = env->GetFieldID(wc, "posZ", "D");
                    TRACE_BRANCH("posZDirectCanonicalHit", g_posZField != nullptr);
                    if (!g_posZField) {
                        env->ExceptionClear();
                        g_posZField = env->GetFieldID(wc, "field_70161_v", "D");
                        TRACE_BRANCH("posZDirectObfHit", g_posZField != nullptr);
                    }
                    if (!g_posZField) env->ExceptionClear();
                    else Log("posZ: resolved by direct lookup");
                }

                jobjectArray fs = (jobjectArray)env->CallObjectMethod(wc, mGetFields);
                if (fs && !env->ExceptionCheck()) {
                    jsize fc2 = env->GetArrayLength(fs);
                    for (int f2 = 0; f2 < fc2; f2++) {
                        jobject fl = env->GetObjectArrayElement(fs, f2);
                        if (!fl) continue;
                        jclass ftt = (jclass)env->CallObjectMethod(fl, mFType);
                        if (!ftt || env->ExceptionCheck()) { env->ExceptionClear(); continue; }
                        jint mod = env->CallIntMethod(fl, mFMod);
                        if (env->CallStaticBooleanMethod(cMod, mIsStatic, mod)) continue;
                        if (GetClassNameFromClass(env, ftt) == "double") {
                            jstring jfn = (jstring)env->CallObjectMethod(fl, mFName);
                            const char* cfn = env->GetStringUTFChars(jfn, nullptr);
                            std::string n = cfn ? cfn : "";
                            env->ReleaseStringUTFChars(jfn, cfn);

                            if (!g_posXField && (n == "posX" || n == "field_70165_t")) {
                                g_posXField = env->GetFieldID(wc, n.c_str(), "D");
                                if (env->ExceptionCheck()) { env->ExceptionClear(); g_posXField = nullptr; }
                                else Log("posX: " + n);
                            }
                            if (!g_posYField && (n == "posY" || n == "field_70163_u")) {
                                g_posYField = env->GetFieldID(wc, n.c_str(), "D");
                                if (env->ExceptionCheck()) { env->ExceptionClear(); g_posYField = nullptr; }
                                else Log("posY: " + n);
                            }
                            if (!g_posZField && (n == "posZ" || n == "field_70161_v")) {
                                g_posZField = env->GetFieldID(wc, n.c_str(), "D");
                                if (env->ExceptionCheck()) { env->ExceptionClear(); g_posZField = nullptr; }
                                else Log("posZ: " + n);
                            }
                        }
                        env->DeleteLocalRef(ftt);
                        env->DeleteLocalRef(fl);
                    }
                    env->DeleteLocalRef(fs);
                }
                if (env->ExceptionCheck()) env->ExceptionClear();
            }
            wc = (jclass)env->CallObjectMethod(wc, mGetSuper);
            if (env->ExceptionCheck()) { env->ExceptionClear(); break; }
            d++;
        }
    }

    // Find GuiChat class to use for native cursor release
    jclass chatClass = LoadClassWithLoader(env, gcl, "net.minecraft.client.gui.GuiChat");
    if (chatClass) {
        g_guiChatClass = (jclass)env->NewGlobalRef(chatClass);
        // GuiChat in 1.8.9 takes a default string
        g_guiChatConstructor = env->GetMethodID(chatClass, "<init>", "(Ljava/lang/String;)V");
        if (g_guiChatConstructor) {
             Log("Found GuiChat constructor");
        } else {
             Log("ERROR: Could not find GuiChat constructor (String)");
        }
    } else {
        Log("ERROR: Could not find GuiChat class");
    }

    // Find displayGuiScreen method in Minecraft
    if (mcClass) {
         // Scan methods for (Lnet/minecraft/client/gui/GuiScreen;)V
         // Name is usually displayGuiScreen or func_147108_a
         jobjectArray methods = (jobjectArray)env->CallObjectMethod(mcClass, mGetMethods);
         jsize mc2 = env->GetArrayLength(methods);
         for (int m2 = 0; m2 < mc2; m2++) {
             jobject mth = env->GetObjectArrayElement(methods, m2);
             if (!mth) continue;
             jstring jmn = (jstring)env->CallObjectMethod(mth, mMName);
             const char* cmn = env->GetStringUTFChars(jmn, nullptr);
             jstring jsig = (jstring)env->CallObjectMethod(mth, env->GetMethodID(cMethod, "toString", "()Ljava/lang/String;")); // Or scan params
             
             // Simpler: Just get it by name if possible, or mapping
             // Let's rely on standard names first
             env->ReleaseStringUTFChars(jmn, cmn);
         }
         
         // Try standard names
         g_displayGuiScreenMethod = env->GetMethodID(mcClass, "displayGuiScreen", "(Lnet/minecraft/client/gui/GuiScreen;)V");
         if (!g_displayGuiScreenMethod) {
             g_displayGuiScreenMethod = env->GetMethodID(mcClass, "func_147108_a", "(Lnet/minecraft/client/gui/GuiScreen;)V");
         }
         if (!g_displayGuiScreenMethod) {
             // Fallback: look for method taking GuiScreen
             // ... for now assume one of the above works or logging error
             Log("WARNING: Could not find displayGuiScreen method");
         } else {
             Log("Found displayGuiScreen method");
         }
    }

    g_mcClass = (jclass)env->NewGlobalRef(mcClass);
    g_mcInstance = env->NewGlobalRef(mcInst);
    if (g_mcInstance && g_thePlayerField)
        ka_premotion::BindMcPlayerLookup(g_mcInstance, g_thePlayerField);
    
    // Nametags Discovery
    if (g_mcClass) {
         // Find theWorld
         jobjectArray fs = (jobjectArray)env->CallObjectMethod(g_mcClass, mGetFields);
         // Simplified search for WorldClient field
         jint worldCount = 0;
         jsize fc = env->GetArrayLength(fs);
         for(int i=0; i<fc; i++) {
             jobject f = env->GetObjectArrayElement(fs, i);
             if(!f) continue;
             jclass ft = (jclass)env->CallObjectMethod(f, mFType);
             if (ft && GetClassNameFromClass(env, ft).find("WorldClient") != std::string::npos) {
                  g_theWorldField = env->FromReflectedField(f);
                  Log("Found theWorld field");
                  break;
             }
         }
         if (!g_theWorldField) {
              g_theWorldField = env->GetFieldID(mcClass, "theWorld", "Lnet/minecraft/client/multiplayer/WorldClient;");
              if (!g_theWorldField) g_theWorldField = env->GetFieldID(mcClass, "field_71441_e", "Lnet/minecraft/client/multiplayer/WorldClient;");
         }
    }

    if (g_theWorldField && g_mcInstance) {
        jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
        if (world) {
             jclass worldClass = env->GetObjectClass(world);
             g_playerEntitiesField = env->GetFieldID(worldClass, "playerEntities", "Ljava/util/List;");
             if (!g_playerEntitiesField) {
                 env->ExceptionClear();
                 g_playerEntitiesField = env->GetFieldID(worldClass, "field_73010_i", "Ljava/util/List;");
             }
             if (!g_playerEntitiesField) env->ExceptionClear();
             g_loadedTileEntityListField = env->GetFieldID(worldClass, "loadedTileEntityList", "Ljava/util/List;");
             if (!g_loadedTileEntityListField) {
                 env->ExceptionClear();
                 g_loadedTileEntityListField = env->GetFieldID(worldClass, "field_147482_g", "Ljava/util/List;");
             }
             if (!g_loadedTileEntityListField) env->ExceptionClear();
              
             if (g_playerEntitiesField) Log("Found playerEntities field");
             if (g_loadedTileEntityListField) Log("Found loadedTileEntityList field");
             env->DeleteLocalRef(worldClass);
             env->DeleteLocalRef(world);
        }
    }

    jclass tileEntityClass = LoadClassWithLoader(env, gcl, "net/minecraft/tileentity/TileEntity");
    if (!tileEntityClass) {
        tileEntityClass = env->FindClass("net/minecraft/tileentity/TileEntity");
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (tileEntityClass) {
        g_tileEntityPosField = env->GetFieldID(tileEntityClass, "pos", "Lnet/minecraft/util/BlockPos;");
        if (!g_tileEntityPosField) {
            env->ExceptionClear();
            g_tileEntityPosField = env->GetFieldID(tileEntityClass, "field_174879_c", "Lnet/minecraft/util/BlockPos;");
        }
        if (!g_tileEntityPosField) env->ExceptionClear();
    }

    jclass blockPosClass = LoadClassWithLoader(env, gcl, "net/minecraft/util/BlockPos");
    if (!blockPosClass) {
        blockPosClass = env->FindClass("net/minecraft/util/BlockPos");
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (blockPosClass) {
        if (!g_blockPosClass) g_blockPosClass = (jclass)env->NewGlobalRef(blockPosClass);
        g_blockPosIntCtor = env->GetMethodID(blockPosClass, "<init>", "(III)V");
        if (!g_blockPosIntCtor) env->ExceptionClear();

        g_blockPosGetX = env->GetMethodID(blockPosClass, "getX", "()I");
        if (!g_blockPosGetX) { env->ExceptionClear(); g_blockPosGetX = env->GetMethodID(blockPosClass, "func_177958_n", "()I"); }
        if (!g_blockPosGetX) env->ExceptionClear();

        g_blockPosGetY = env->GetMethodID(blockPosClass, "getY", "()I");
        if (!g_blockPosGetY) { env->ExceptionClear(); g_blockPosGetY = env->GetMethodID(blockPosClass, "func_177956_o", "()I"); }
        if (!g_blockPosGetY) env->ExceptionClear();

        g_blockPosGetZ = env->GetMethodID(blockPosClass, "getZ", "()I");
        if (!g_blockPosGetZ) { env->ExceptionClear(); g_blockPosGetZ = env->GetMethodID(blockPosClass, "func_177952_p", "()I"); }
        if (!g_blockPosGetZ) env->ExceptionClear();
    }

    jclass chestCls = LoadClassWithLoader(env, gcl, "net/minecraft/tileentity/TileEntityChest");
    if (!chestCls) chestCls = env->FindClass("net/minecraft/tileentity/TileEntityChest");
    if (chestCls && !env->ExceptionCheck()) g_tileEntityChestClass = (jclass)env->NewGlobalRef(chestCls);
    if (env->ExceptionCheck()) env->ExceptionClear();

    jclass enderChestCls = LoadClassWithLoader(env, gcl, "net/minecraft/tileentity/TileEntityEnderChest");
    if (!enderChestCls) enderChestCls = env->FindClass("net/minecraft/tileentity/TileEntityEnderChest");
    if (enderChestCls && !env->ExceptionCheck()) g_tileEntityEnderChestClass = (jclass)env->NewGlobalRef(enderChestCls);
    if (env->ExceptionCheck()) env->ExceptionClear();

    // Game Settings & FOV
    g_gameSettingsField = env->GetFieldID(mcClass, "gameSettings", "Lnet/minecraft/client/settings/GameSettings;");
    if(!g_gameSettingsField) g_gameSettingsField = env->GetFieldID(mcClass, "field_71474_y", "Lnet/minecraft/client/settings/GameSettings;");

    if (g_gameSettingsField) {
        jobject gs = env->GetObjectField(g_mcInstance, g_gameSettingsField);
        if (gs) {
            jclass gsClass = env->GetObjectClass(gs);
            g_fovSettingField = env->GetFieldID(gsClass, "fovSetting", "F");
            if (!g_fovSettingField) g_fovSettingField = env->GetFieldID(gsClass, "field_74334_X", "F");
            if (g_fovSettingField) Log("Found fovSetting field");
            g_keyBindSneakField = env->GetFieldID(gsClass, "keyBindSneak", "Lnet/minecraft/client/settings/KeyBinding;");
            if (!g_keyBindSneakField) {
                env->ExceptionClear();
                g_keyBindSneakField = env->GetFieldID(gsClass, "field_74311_E", "Lnet/minecraft/client/settings/KeyBinding;");
            }
            if (!g_keyBindSneakField) env->ExceptionClear();
            else Log("Found keyBindSneak field");

            if (g_keyBindSneakField) {
                jobject sneakKey = env->GetObjectField(gs, g_keyBindSneakField);
                if (sneakKey && !env->ExceptionCheck()) {
                    jclass kbClass = env->GetObjectClass(sneakKey);
                    if (kbClass) {
                        if (!g_keyBindingClass) g_keyBindingClass = (jclass)env->NewGlobalRef(kbClass);
                        g_keyBindingGetKeyCodeMethod = env->GetMethodID(kbClass, "getKeyCode", "()I");
                        if (!g_keyBindingGetKeyCodeMethod) {
                            env->ExceptionClear();
                            g_keyBindingGetKeyCodeMethod = env->GetMethodID(kbClass, "func_151463_i", "()I");
                        }
                        if (!g_keyBindingGetKeyCodeMethod) env->ExceptionClear();

                        g_keyBindingKeyCodeField = env->GetFieldID(kbClass, "keyCode", "I");
                        if (!g_keyBindingKeyCodeField) {
                            env->ExceptionClear();
                            g_keyBindingKeyCodeField = env->GetFieldID(kbClass, "field_74512_d", "I");
                        }
                        if (!g_keyBindingKeyCodeField) env->ExceptionClear();

                        g_keyBindingPressedField = env->GetFieldID(kbClass, "pressed", "Z");
                        if (!g_keyBindingPressedField) {
                            env->ExceptionClear();
                            g_keyBindingPressedField = env->GetFieldID(kbClass, "field_74513_e", "Z");
                        }
                        if (!g_keyBindingPressedField) env->ExceptionClear();

                        g_keyBindingSetKeyBindStateMethod = env->GetStaticMethodID(kbClass, "setKeyBindState", "(IZ)V");
                        if (!g_keyBindingSetKeyBindStateMethod) {
                            env->ExceptionClear();
                            g_keyBindingSetKeyBindStateMethod = env->GetStaticMethodID(kbClass, "func_74510_a", "(IZ)V");
                        }
                        if (!g_keyBindingSetKeyBindStateMethod) env->ExceptionClear();

                        env->DeleteLocalRef(kbClass);
                    }
                    env->DeleteLocalRef(sneakKey);
                } else if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                }
            }
            env->DeleteLocalRef(gsClass);
            env->DeleteLocalRef(gs);
        }
    }

    jclass keyboardClass = LoadClassWithLoader(env, gcl, "org.lwjgl.input.Keyboard");
    if (!keyboardClass) {
        keyboardClass = env->FindClass("org/lwjgl/input/Keyboard");
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (keyboardClass && !env->ExceptionCheck()) {
        g_lwjglKeyboardClass = (jclass)env->NewGlobalRef(keyboardClass);
        g_keyboardIsKeyDownMethod = env->GetStaticMethodID(keyboardClass, "isKeyDown", "(I)Z");
        if (!g_keyboardIsKeyDownMethod) env->ExceptionClear();
        env->DeleteLocalRef(keyboardClass);
    } else if (env->ExceptionCheck()) {
        env->ExceptionClear();
    }

    jclass mouseClass = LoadClassWithLoader(env, gcl, "org.lwjgl.input.Mouse");
    if (!mouseClass) {
        mouseClass = env->FindClass("org/lwjgl/input/Mouse");
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (mouseClass && !env->ExceptionCheck()) {
        g_lwjglMouseClass = (jclass)env->NewGlobalRef(mouseClass);
        g_mouseIsButtonDownMethod = env->GetStaticMethodID(mouseClass, "isButtonDown", "(I)Z");
        if (!g_mouseIsButtonDownMethod) env->ExceptionClear();
        env->DeleteLocalRef(mouseClass);
    } else if (env->ExceptionCheck()) {
        env->ExceptionClear();
    }

    // objectMouseOver
    if (mcClass) {
        g_objectMouseOverField = env->GetFieldID(mcClass, "objectMouseOver", "Lnet/minecraft/util/MovingObjectPosition;");
        if (!g_objectMouseOverField) { env->ExceptionClear(); g_objectMouseOverField = env->GetFieldID(mcClass, "field_71476_x", "Lnet/minecraft/util/MovingObjectPosition;"); }
        if (!g_objectMouseOverField) env->ExceptionClear();
        else Log("Found objectMouseOver field");

        g_leftClickCounterField = env->GetFieldID(mcClass, "leftClickCounter", "I");
        if (!g_leftClickCounterField) { env->ExceptionClear(); g_leftClickCounterField = env->GetFieldID(mcClass, "field_71429_W", "I"); }
        if (!g_leftClickCounterField) Log("HitDelayFix: leftClickCounter mapping unavailable; module will remain idle.");
        else Log("HitDelayFix: found leftClickCounter field");

        g_pointedEntityField = env->GetFieldID(mcClass, "pointedEntity", "Lnet/minecraft/entity/Entity;");
        if (!g_pointedEntityField) { env->ExceptionClear(); g_pointedEntityField = env->GetFieldID(mcClass, "field_147125_j", "Lnet/minecraft/entity/Entity;"); }
        if (!g_pointedEntityField) env->ExceptionClear();
        else Log("Found pointedEntity field");
    }

    jclass mopClass = LoadClassWithLoader(env, gcl, "net.minecraft.util.MovingObjectPosition");
    if (!mopClass) mopClass = env->FindClass("net/minecraft/util/MovingObjectPosition");
    if (mopClass && !env->ExceptionCheck()) {
        g_movingObjectPositionClass = (jclass)env->NewGlobalRef(mopClass);
        g_typeOfHitField = env->GetFieldID(mopClass, "typeOfHit", "Lnet/minecraft/util/MovingObjectPosition$MovingObjectType;");
        if (!g_typeOfHitField) { env->ExceptionClear(); g_typeOfHitField = env->GetFieldID(mopClass, "field_72313_a", "Lnet/minecraft/util/MovingObjectPosition$MovingObjectType;"); }
        if (!g_typeOfHitField) env->ExceptionClear();
        else Log("Found typeOfHit field");

        g_entityHitField = env->GetFieldID(mopClass, "entityHit", "Lnet/minecraft/entity/Entity;");
        if (!g_entityHitField) { env->ExceptionClear(); g_entityHitField = env->GetFieldID(mopClass, "field_72308_g", "Lnet/minecraft/entity/Entity;"); }
        if (!g_entityHitField) env->ExceptionClear();
        else Log("Found entityHit field");
    }
    if (env->ExceptionCheck()) env->ExceptionClear();

    if (mopClass && !g_movingObjectTypeClass) {
        jclass mopTypeLocal = env->FindClass("net/minecraft/util/MovingObjectPosition$MovingObjectType");
        if (!mopTypeLocal) {
            env->ExceptionClear();
            mopTypeLocal = LoadClassWithLoader(env, gcl, "net.minecraft.util.MovingObjectPosition$MovingObjectType");
        }
        if (mopTypeLocal && !env->ExceptionCheck()) {
            g_movingObjectTypeClass = (jclass)env->NewGlobalRef(mopTypeLocal);

            jfieldID entityConstField = env->GetStaticFieldID(mopTypeLocal, "ENTITY", "Lnet/minecraft/util/MovingObjectPosition$MovingObjectType;");
            if (!entityConstField) {
                env->ExceptionClear();
                entityConstField = env->GetStaticFieldID(mopTypeLocal, "field_72310_e", "Lnet/minecraft/util/MovingObjectPosition$MovingObjectType;");
            }
            if (entityConstField) {
                jobject entityConstLocal = env->GetStaticObjectField(mopTypeLocal, entityConstField);
                if (!env->ExceptionCheck() && entityConstLocal) {
                    g_mopEntityTypeConst = env->NewGlobalRef(entityConstLocal);
                    env->DeleteLocalRef(entityConstLocal);
                    Log("Found MovingObjectType.ENTITY constant");
                } else if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                }
            } else {
                env->ExceptionClear();
            }

            env->DeleteLocalRef(mopTypeLocal);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }
    
    jclass enumClass = env->FindClass("java/lang/Enum");
    if (enumClass) {
        g_enumNameMethod = env->GetMethodID(enumClass, "name", "()Ljava/lang/String;");
        env->DeleteLocalRef(enumClass);
    }





    if (!g_getRenderItemFromMcMethod) {
        g_getRenderItemFromMcMethod = env->GetMethodID(mcClass, "getRenderItem", "()Lnet/minecraft/client/renderer/entity/RenderItem;");
        if (!g_getRenderItemFromMcMethod) {
            env->ExceptionClear();
            g_getRenderItemFromMcMethod = env->GetMethodID(mcClass, "func_175599_af", "()Lnet/minecraft/client/renderer/entity/RenderItem;");
        }
        if (!g_getRenderItemFromMcMethod) env->ExceptionClear();
        else Log("Found Minecraft.getRenderItem");
    }

    if (!g_renderItemAndEffectIntoGUIMethod || !g_renderItemIntoGUIMethod) {
        jclass renderItemClass = LoadClassWithLoader(env, gcl, "net/minecraft/client/renderer/entity/RenderItem");
        if (!renderItemClass) renderItemClass = env->FindClass("net/minecraft/client/renderer/entity/RenderItem");
        if (renderItemClass && !env->ExceptionCheck()) {
            if (!g_renderItemAndEffectIntoGUIMethod) {
                g_renderItemAndEffectIntoGUIMethod = env->GetMethodID(renderItemClass, "renderItemAndEffectIntoGUI", "(Lnet/minecraft/item/ItemStack;II)V");
                if (!g_renderItemAndEffectIntoGUIMethod) {
                    env->ExceptionClear();
                    g_renderItemAndEffectIntoGUIMethod = env->GetMethodID(renderItemClass, "func_180450_b", "(Lnet/minecraft/item/ItemStack;II)V");
                }
                if (!g_renderItemAndEffectIntoGUIMethod) env->ExceptionClear();
                else Log("Found RenderItem.renderItemAndEffectIntoGUI");
            }
            if (!g_renderItemIntoGUIMethod) {
                g_renderItemIntoGUIMethod = env->GetMethodID(renderItemClass, "renderItemIntoGUI", "(Lnet/minecraft/item/ItemStack;II)V");
                if (!g_renderItemIntoGUIMethod) {
                    env->ExceptionClear();
                    g_renderItemIntoGUIMethod = env->GetMethodID(renderItemClass, "func_175042_a", "(Lnet/minecraft/item/ItemStack;II)V");
                }
                if (!g_renderItemIntoGUIMethod) env->ExceptionClear();
                else Log("Found RenderItem.renderItemIntoGUI");
            }
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
    }

    // Find rotation fields on Player class (we already found g_posXField's class 'wc')
    // We need to find the class that has rotationYaw/Pitch. It is Entity.
    // 'wc' in previous loop was playerType or super.
    // Let's re-scan if we have playerType.
    // Check for ItemBlock class
    if (!g_itemBlockClass) {
        jclass ibClass = LoadClassWithLoader(env, gcl, "net.minecraft.item.ItemBlock");
        if (!ibClass) ibClass = env->FindClass("net/minecraft/item/ItemBlock");
        
        // Robust Scan: Find class extending Item that has a field of type Block
        if (!ibClass) {
             Log("ItemBlock not found by name, scanning...");
             // First find Item and Block classes
             jclass itemCls = LoadClassWithLoader(env, gcl, "net.minecraft.item.Item");
             if (!itemCls) itemCls = env->FindClass("net/minecraft/item/Item");
             jclass blockCls = LoadClassWithLoader(env, gcl, "net.minecraft.block.Block");
             if (!blockCls) blockCls = env->FindClass("net/minecraft/block/Block");

             if (itemCls && blockCls) {
                 for (int i = 0; i < classCount; i++) {
                     jclass cls = classes[i];
                     if (env->ExceptionCheck()) { env->ExceptionClear(); continue; }
                     
                     // Check superclass
                     jclass super = (jclass)env->CallObjectMethod(cls, mGetSuper);
                     if (!super || !env->IsSameObject(super, itemCls)) continue;

                     // Check fields for one of type Block
                     jobjectArray fs = (jobjectArray)env->CallObjectMethod(cls, mGetFields);
                     bool hasBlock = false;
                     jsize fc = fs ? env->GetArrayLength(fs) : 0;
                     for(int f=0; f<fc; f++) {
                          jobject fld = env->GetObjectArrayElement(fs, f);
                          jclass ft = (jclass)env->CallObjectMethod(fld, mFType);
                          if (ft && env->IsSameObject(ft, blockCls)) {
                              hasBlock = true;
                              break;
                          }
                     }
                     if (hasBlock) {
                         ibClass = cls;
                         Log("Found ItemBlock candidate by signature");
                         break;
                     }
                 }
             }
        }

        if (ibClass && !env->ExceptionCheck()) {
             g_itemBlockClass = (jclass)env->NewGlobalRef(ibClass);
             Log("Found ItemBlock class");
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
    }

    if (!g_itemSwordClass) {
        jclass swordClass = LoadClassWithLoader(env, gcl, "net.minecraft.item.ItemSword");
        if (!swordClass) swordClass = env->FindClass("net/minecraft/item/ItemSword");
        if (swordClass && !env->ExceptionCheck()) {
            g_itemSwordClass = (jclass)env->NewGlobalRef(swordClass);
            Log("Found ItemSword class");
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
    }

    if (!playerType.empty()) {
        jclass pc = LoadClassWithLoader(env, gcl, playerType.c_str());

        if (!pc) {
             std::string slashName = playerType;
             std::replace(slashName.begin(), slashName.end(), '.', '/');
             pc = env->FindClass(slashName.c_str()); 
        }
        if (!pc) Log("ERROR: Could not find class " + playerType);

        jclass wc = pc; int d = 0;
        while (wc && d < 10 && (!g_rotationYawField || !g_rotationPitchField || !g_getNameMethod || !g_getHeldItemMethod || !g_getTotalArmorValueMethod)) {
             std::string clsName = GetClassNameFromClass(env, wc);
             Log("Scanning hierarchy: " + clsName);

             if (!g_rotationYawField) {
                 g_rotationYawField = env->GetFieldID(wc, "rotationYaw", "F");
                 if(!g_rotationYawField) { env->ExceptionClear(); g_rotationYawField = env->GetFieldID(wc, "field_70177_z", "F"); }
                 if(!g_rotationYawField) env->ExceptionClear();
                 else Log("Found rotationYaw in " + clsName);
             }
             if (!g_rotationPitchField) {
                 g_rotationPitchField = env->GetFieldID(wc, "rotationPitch", "F");
                 if(!g_rotationPitchField) { env->ExceptionClear(); g_rotationPitchField = env->GetFieldID(wc, "field_70125_A", "F"); }
                 if(!g_rotationPitchField) env->ExceptionClear();
                 else Log("Found rotationPitch in " + clsName);
             }
             if (!g_getNameMethod) {
                 g_getNameMethod = env->GetMethodID(wc, "getName", "()Ljava/lang/String;");
                 if(!g_getNameMethod) { env->ExceptionClear(); g_getNameMethod = env->GetMethodID(wc, "func_70005_c_", "()Ljava/lang/String;"); }
                 if(!g_getNameMethod) env->ExceptionClear();
                 else Log("Found getName in " + clsName);
             }
             if (!g_getHeldItemMethod) {
                 g_getHeldItemMethod = env->GetMethodID(wc, "getHeldItem", "()Lnet/minecraft/item/ItemStack;");
                 if(!g_getHeldItemMethod) { env->ExceptionClear(); g_getHeldItemMethod = env->GetMethodID(wc, "func_70694_bm", "()Lnet/minecraft/item/ItemStack;"); }
                 if(!g_getHeldItemMethod) env->ExceptionClear();
                 else Log("Found getHeldItem in " + clsName);
             }
             if (!g_getTotalArmorValueMethod) {
                 g_getTotalArmorValueMethod = env->GetMethodID(wc, "getTotalArmorValue", "()I");
                 if(!g_getTotalArmorValueMethod) { env->ExceptionClear(); g_getTotalArmorValueMethod = env->GetMethodID(wc, "func_70658_aO", "()I"); }
                 if(!g_getTotalArmorValueMethod) env->ExceptionClear();
                 else Log("Found getTotalArmorValue in " + clsName);
             }

             // Last Tick Pos for Interpolation
             if (!g_lastTickPosXField) {
                 g_lastTickPosXField = env->GetFieldID(wc, "lastTickPosX", "D");
                 if(!g_lastTickPosXField) { env->ExceptionClear(); g_lastTickPosXField = env->GetFieldID(wc, "field_70142_S", "D"); }
                 if(!g_lastTickPosXField) { env->ExceptionClear(); g_lastTickPosXField = env->GetFieldID(wc, "prevPosX", "D"); }
                 if(g_lastTickPosXField) Log("Found lastTickPosX");
                 else env->ExceptionClear();
             }
             if (!g_lastTickPosYField) {
                 g_lastTickPosYField = env->GetFieldID(wc, "lastTickPosY", "D");
                 if(!g_lastTickPosYField) { env->ExceptionClear(); g_lastTickPosYField = env->GetFieldID(wc, "field_70137_T", "D"); }
                 if(!g_lastTickPosYField) { env->ExceptionClear(); g_lastTickPosYField = env->GetFieldID(wc, "prevPosY", "D"); }
                 if(g_lastTickPosYField) Log("Found lastTickPosY");
                 else env->ExceptionClear();
             }
             if (!g_lastTickPosZField) {
                 g_lastTickPosZField = env->GetFieldID(wc, "lastTickPosZ", "D");
                 if(!g_lastTickPosZField) { env->ExceptionClear(); g_lastTickPosZField = env->GetFieldID(wc, "field_70136_U", "D"); }
                 if(!g_lastTickPosZField) { env->ExceptionClear(); g_lastTickPosZField = env->GetFieldID(wc, "prevPosZ", "D"); }
                 if(g_lastTickPosZField) Log("Found lastTickPosZ");
                 else env->ExceptionClear();
             }

             wc = (jclass)env->CallObjectMethod(wc, mGetSuper);
             d++;
        }
        
        // Find Inventory
        if (g_thePlayerField) {
             // We need to look at EntityPlayer (superclass of EntityPlayerSP)
             // But we can just scan fields of the player object instance's class hierarchy again
             // or assume it's "inventory" / "field_71071_by"
             
             // Quick scan on player class for InventoryPlayer
             if (!g_inventoryField) {
                  jobject pObj = env->GetObjectField(g_mcInstance, g_thePlayerField);
                  if (pObj) {
                       jclass pClass = env->GetObjectClass(pObj);
                       jclass w = pClass; int depth = 0;
                       while (w && depth < 5) {
                            jobjectArray fs = (jobjectArray)env->CallObjectMethod(w, mGetFields);
                            jsize fc = fs ? env->GetArrayLength(fs) : 0;
                            for (int i=0; i<fc; i++) {
                                 jobject f = env->GetObjectArrayElement(fs, i);
                                 jclass ft = (jclass)env->CallObjectMethod(f, mFType);
                                 std::string ftn = ft ? GetClassNameFromClass(env, ft) : "";
                                 if (ftn.find("InventoryPlayer") != std::string::npos) {
                                      g_inventoryField = env->FromReflectedField(f);
                                      Log("Found inventory field in " + GetClassNameFromClass(env, w));
                                      break;
                                 }
                            }
                            if (g_inventoryField) break;
                            w = (jclass)env->CallObjectMethod(w, mGetSuper);
                            depth++;
                       }
                       env->DeleteLocalRef(pObj);
                  }
                  
                  if (!g_inventoryField) {
                       // Try Common names if reflection failed (e.g. if we didn't have instance yet?)
                       // Actually we have g_mcInstance, so we should be good.
                       // Fallback
                  }
             }

             if (g_inventoryField && !g_getCurrentItemMethod) {
                  jobject pObj = env->GetObjectField(g_mcInstance, g_thePlayerField);
                  if (!env->ExceptionCheck() && pObj) {
                      jobject invObj = env->GetObjectField(pObj, g_inventoryField);
                      if (!env->ExceptionCheck() && invObj) {
                          jclass invClass = env->GetObjectClass(invObj);
                          if (invClass) {
                              g_getCurrentItemMethod = env->GetMethodID(invClass, "getCurrentItem", "()Lnet/minecraft/item/ItemStack;");
                              if (!g_getCurrentItemMethod) {
                                  env->ExceptionClear();
                                  g_getCurrentItemMethod = env->GetMethodID(invClass, "func_70448_g", "()Lnet/minecraft/item/ItemStack;");
                              }
                              if (!g_getCurrentItemMethod) env->ExceptionClear();
                              else Log("Found InventoryPlayer.getCurrentItem");
                              env->DeleteLocalRef(invClass);
                          } else if (env->ExceptionCheck()) {
                              env->ExceptionClear();
                          }
                          env->DeleteLocalRef(invObj);
                      } else if (env->ExceptionCheck()) {
                          env->ExceptionClear();
                      }
                      env->DeleteLocalRef(pObj);
                  } else if (env->ExceptionCheck()) {
                      env->ExceptionClear();
                  }
             }
        }
    }

    // Timer Discovery
    if (g_mcClass) {
        g_timerField = env->GetFieldID(g_mcClass, "timer", "Lnet/minecraft/util/Timer;");
        if (!g_timerField) { env->ExceptionClear(); g_timerField = env->GetFieldID(g_mcClass, "field_71428_T", "Lnet/minecraft/util/Timer;"); }
        if (!g_timerField) env->ExceptionClear();
        
        if (g_timerField) {
             Log("Found timer field");
             jobject timerObj = env->GetObjectField(g_mcInstance, g_timerField);
             if (timerObj) {
                 jclass timerClass = env->GetObjectClass(timerObj);
                 g_renderPartialTicksField = env->GetFieldID(timerClass, "renderPartialTicks", "F");
                 if (!g_renderPartialTicksField) { env->ExceptionClear(); g_renderPartialTicksField = env->GetFieldID(timerClass, "field_74281_c", "F"); }
                 if (!g_renderPartialTicksField) { env->ExceptionClear(); g_renderPartialTicksField = env->GetFieldID(timerClass, "elapsedPartialTicks", "F"); }
                 
                 if (g_renderPartialTicksField) Log("Found renderPartialTicks");
                 else { Log("ERROR: Could not find renderPartialTicks"); env->ExceptionClear(); }
                 
                 env->DeleteLocalRef(timerClass);
                 env->DeleteLocalRef(timerObj);
             }
        } else {
             Log("ERROR: Could not find timer field in Minecraft");
        }
    }

    // ActiveRenderInfo Discovery (Dynamic)
    g_activeRenderInfoClass = env->FindClass("net/minecraft/client/renderer/ActiveRenderInfo");
    TRACE_BRANCH("activeRenderInfoCanonicalFindClassHit", g_activeRenderInfoClass != nullptr);
    if (!g_activeRenderInfoClass) {
        env->ExceptionClear();
        Log("ActiveRenderInfo not found by name, scanning classes...");
        TRACE_PATH("scan-for-active-render-info");
        for (int i = 0; i < classCount; i++) {
            jclass cls = classes[i];
            if (env->ExceptionCheck()) { env->ExceptionClear(); continue; }
            
            // Check static fields signature: 3 FloatBuffers, 1 IntBuffer
            jobjectArray fs = (jobjectArray)env->CallObjectMethod(cls, mGetFields);
            if (!fs || env->ExceptionCheck()) { env->ExceptionClear(); continue; }
            
            int fbCount = 0;
            int ibCount = 0;
            jsize fc = env->GetArrayLength(fs);
            for (int f = 0; f < fc; f++) {
                 jobject fld = env->GetObjectArrayElement(fs, f);
                 if (!fld) continue;
                 jint mod = env->CallIntMethod(fld, mFMod);
                 if (!env->CallStaticBooleanMethod(cMod, mIsStatic, mod)) continue; // Must be static
                 
                 jclass ft = (jclass)env->CallObjectMethod(fld, mFType);
                 if (!ft) continue;
                 std::string ftn = GetClassNameFromClass(env, ft);
                 if (ftn == "java.nio.FloatBuffer") fbCount++;
                 else if (ftn == "java.nio.IntBuffer") ibCount++;
            }
            
            if (fbCount >= 2 && ibCount >= 1) { // At least 2 FB (ModelView, Projection) and 1 IB (Viewport)
                 jstring jn = (jstring)env->CallObjectMethod(cls, mGetName);
                 const char* cn = env->GetStringUTFChars(jn, nullptr);
                 Log("Found ActiveRenderInfo candidate: " + std::string(cn));
                 g_activeRenderInfoClass = (jclass)env->NewGlobalRef(cls);
                 env->ReleaseStringUTFChars(jn, cn);
                 break;
            }
        }
    } else {
        g_activeRenderInfoClass = (jclass)env->NewGlobalRef(g_activeRenderInfoClass);
    }

    if (g_activeRenderInfoClass) {
        g_modelViewField = env->GetStaticFieldID(g_activeRenderInfoClass, "MODELVIEW", "Ljava/nio/FloatBuffer;");
        TRACE_BRANCH("modelViewCanonicalHit", g_modelViewField != nullptr);
        if (!g_modelViewField) env->ExceptionClear();
        
        g_projectionField = env->GetStaticFieldID(g_activeRenderInfoClass, "PROJECTION", "Ljava/nio/FloatBuffer;");
        TRACE_BRANCH("projectionCanonicalHit", g_projectionField != nullptr);
        if (!g_projectionField) env->ExceptionClear();
        
        if (!g_modelViewField || !g_projectionField) {
             TRACE_PATH("scan-for-model-projection-fallback");
             Log("Scanning fields for ModelView/Projection fallback...");
             jobjectArray fs = (jobjectArray)env->CallObjectMethod(g_activeRenderInfoClass, mGetFields);
             jsize fc = env->GetArrayLength(fs);
             int fbIdx = 0;
             for (int f = 0; f < fc; f++) {
                 jobject fld = env->GetObjectArrayElement(fs, f);
                 jint mod = env->CallIntMethod(fld, mFMod);
                 if (!env->CallStaticBooleanMethod(cMod, mIsStatic, mod)) continue;
                 
                 jclass ft = (jclass)env->CallObjectMethod(fld, mFType);
                 if (GetClassNameFromClass(env, ft) == "java.nio.FloatBuffer") {
                      if (fbIdx == 0) {
                           g_modelViewField = env->FromReflectedField(fld);
                           Log("Assumed MODELVIEW (idx 0)");
                      } else if (fbIdx == 1) {
                           g_projectionField = env->FromReflectedField(fld);
                           Log("Assumed PROJECTION (idx 1)");
                      }
                      fbIdx++;
                 }
             }
        }

        if (g_modelViewField && g_projectionField) Log("Found OpenGL Matrix fields");
        else Log("ERROR: Could not find OpenGL Matrix fields");
    } else {
        Log("ERROR: Could not find ActiveRenderInfo class");
        env->ExceptionClear();
    }
    
    // FloatBuffer helper
    jclass floatBufferClass = env->FindClass("java/nio/FloatBuffer");
    if (floatBufferClass) {
        g_floatBufferGet = env->GetMethodID(floatBufferClass, "get", "(I)F");
        env->DeleteLocalRef(floatBufferClass);
    }

    // RenderManager Discovery
    if (g_mcClass) {
        g_renderManagerField = env->GetFieldID(g_mcClass, "renderManager", "Lnet/minecraft/client/renderer/entity/RenderManager;");
        if (!g_renderManagerField) { env->ExceptionClear(); g_renderManagerField = env->GetFieldID(g_mcClass, "field_175616_W", "Lnet/minecraft/client/renderer/entity/RenderManager;"); }
        if (!g_renderManagerField) env->ExceptionClear();

        if (g_renderManagerField) {
             Log("Found renderManager field");
             jobject rmObj = env->GetObjectField(g_mcInstance, g_renderManagerField);
             if (rmObj) {
                 jclass rmClass = env->GetObjectClass(rmObj);
                 
                 g_viewerPosXField = env->GetFieldID(rmClass, "viewerPosX", "D");
                 if (!g_viewerPosXField) { env->ExceptionClear(); g_viewerPosXField = env->GetFieldID(rmClass, "field_78725_b", "D"); } // o
                 if (!g_viewerPosXField) { env->ExceptionClear(); g_viewerPosXField = env->GetFieldID(rmClass, "o", "D"); }
                 
                 g_viewerPosYField = env->GetFieldID(rmClass, "viewerPosY", "D");
                 if (!g_viewerPosYField) { env->ExceptionClear(); g_viewerPosYField = env->GetFieldID(rmClass, "field_78726_c", "D"); } // p
                 if (!g_viewerPosYField) { env->ExceptionClear(); g_viewerPosYField = env->GetFieldID(rmClass, "p", "D"); }

                 g_viewerPosZField = env->GetFieldID(rmClass, "viewerPosZ", "D");
                 if (!g_viewerPosZField) { env->ExceptionClear(); g_viewerPosZField = env->GetFieldID(rmClass, "field_78723_d", "D"); } // q
                 if (!g_viewerPosZField) { env->ExceptionClear(); g_viewerPosZField = env->GetFieldID(rmClass, "q", "D"); }

                 if (g_viewerPosXField) Log("Found RenderManager viewerPos fields");
                 else Log("ERROR: Could not find RenderManager viewerPos fields");
                 
                 env->DeleteLocalRef(rmClass);
                 env->DeleteLocalRef(rmObj);
             }
        }
    }

    // List methods
    jclass listCls = env->FindClass("java/util/List");
    if (listCls) {
        g_listSizeMethod = env->GetMethodID(listCls, "size", "()I");
        g_listGetMethod = env->GetMethodID(listCls, "get", "(I)Ljava/lang/Object;");
        g_listClass = (jclass)env->NewGlobalRef(listCls);
    }

    jclass objectCls = env->FindClass("java/lang/Object");
    if (objectCls) {
        g_objectHashCodeMethod = env->GetMethodID(objectCls, "hashCode", "()I");
        env->DeleteLocalRef(objectCls);
    }

    g_mapped = (g_thePlayerField != nullptr);
    TRACE_BRANCH("mappedReady", g_mapped);
    Log("=== Mapping Report ===");
    Log("Player: " + std::string(g_thePlayerField ? "YES" : "NO"));
    Log("World: " + std::string(g_theWorldField ? "YES" : "NO"));
    Log("Entities: " + std::string(g_playerEntitiesField ? "YES" : "NO"));
    Log("TileEntities: " + std::string(g_loadedTileEntityListField ? "YES" : "NO"));
    Log("TilePos: " + std::string(g_tileEntityPosField ? "YES" : "NO"));
    Log("BlockPosXYZ: " + std::string((g_blockPosGetX && g_blockPosGetY && g_blockPosGetZ) ? "YES" : "NO"));
    Log("Yaw: " + std::string(g_rotationYawField ? "YES" : "NO"));
    Log("Pitch: " + std::string(g_rotationPitchField ? "YES" : "NO"));
    Log("Name: " + std::string(g_getNameMethod ? "YES" : "NO"));
    Log("Inventory: " + std::string(g_inventoryField ? "YES" : "NO"));
    Log("ItemBlock: " + std::string(g_itemBlockClass ? "YES" : "NO"));
    Log("=== End Report ===");
    if (g_mcInstance && g_thePlayerField)
        ka_premotion::BindMcPlayerLookup(g_mcInstance, g_thePlayerField);
    jvmti->Deallocate((unsigned char*)classes);
    return g_mapped;
}

void TryResolveWorldMappings(JNIEnv* env) {
    TRACE_PATH("enter");
    bool prerequisites = (env && g_mcInstance && g_theWorldField);
    TRACE_BRANCH("prerequisitesMet", prerequisites);
    if (!prerequisites) return;
    bool alreadyResolved = (g_playerEntitiesField && g_loadedTileEntityListField);
    TRACE_BRANCH("alreadyResolved", alreadyResolved);
    if (alreadyResolved) return;

    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    TRACE_BRANCH("worldInstanceAvailable", world != nullptr);
    if (!world) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return;
    }

    jclass worldClass = env->GetObjectClass(world);
    if (worldClass) {
        if (!g_playerEntitiesField) {
            g_playerEntitiesField = env->GetFieldID(worldClass, "playerEntities", "Ljava/util/List;");
            TRACE_BRANCH("playerEntitiesCanonicalHit", g_playerEntitiesField != nullptr);
            if (!g_playerEntitiesField) {
                env->ExceptionClear();
                g_playerEntitiesField = env->GetFieldID(worldClass, "field_73010_i", "Ljava/util/List;");
                TRACE_BRANCH("playerEntitiesSrgHit", g_playerEntitiesField != nullptr);
            }
            if (!g_playerEntitiesField) {
                env->ExceptionClear();
            } else {
                Log("Late-bound playerEntities field");
            }
        }

        if (!g_loadedTileEntityListField) {
            g_loadedTileEntityListField = env->GetFieldID(worldClass, "loadedTileEntityList", "Ljava/util/List;");
            TRACE_BRANCH("loadedTileEntityListCanonicalHit", g_loadedTileEntityListField != nullptr);
            if (!g_loadedTileEntityListField) {
                env->ExceptionClear();
                g_loadedTileEntityListField = env->GetFieldID(worldClass, "field_147482_g", "Ljava/util/List;");
                TRACE_BRANCH("loadedTileEntityListSrgHit", g_loadedTileEntityListField != nullptr);
            }
            if (!g_loadedTileEntityListField) {
                env->ExceptionClear();
            } else {
                Log("Late-bound loadedTileEntityList field");
            }
        }

        env->DeleteLocalRef(worldClass);
    }
    env->DeleteLocalRef(world);
}

void TryResolveRenderMappings(JNIEnv* env, bool includeActionBarMappings = false) {
    TRACE_PATH("enter");
    TRACE_BRANCH("includeActionBarMappings", includeActionBarMappings);
    bool prerequisites = (env && g_mcInstance);
    TRACE_BRANCH("prerequisitesMet", prerequisites);
    if (!prerequisites) return;

    jobject gcl = EnsureGameClassLoader(env);

    // Prefer canonical ActiveRenderInfo by exact class name when it becomes loadable.
    {
        jclass canonicalAri = nullptr;
        if (gcl) {
            canonicalAri = LoadClassWithLoader(env, gcl, "net.minecraft.client.renderer.ActiveRenderInfo");
            TRACE_BRANCH("canonicalAriLoadClassHit", canonicalAri != nullptr);
        }
        if (!canonicalAri) {
            canonicalAri = env->FindClass("net/minecraft/client/renderer/ActiveRenderInfo");
            TRACE_BRANCH("canonicalAriFindClassHit", canonicalAri != nullptr);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }

        if (canonicalAri) {
            bool replaceAri = (g_activeRenderInfoClass == nullptr)
                || (env->IsSameObject(g_activeRenderInfoClass, canonicalAri) != JNI_TRUE);
            TRACE_BRANCH("replaceActiveRenderInfoBinding", replaceAri);
            if (replaceAri) {
                std::string oldName = g_activeRenderInfoClass ? GetClassNameFromClass(env, g_activeRenderInfoClass) : "";
                std::string newName = GetClassNameFromClass(env, canonicalAri);
                if (g_activeRenderInfoClass) {
                    env->DeleteGlobalRef(g_activeRenderInfoClass);
                    g_activeRenderInfoClass = nullptr;
                }
                g_activeRenderInfoClass = (jclass)env->NewGlobalRef(canonicalAri);
                g_modelViewField = nullptr;
                g_projectionField = nullptr;
                if (!newName.empty()) {
                    Log(oldName.empty()
                        ? ("Late-bound ActiveRenderInfo class: " + newName)
                        : ("Rebound ActiveRenderInfo class: " + oldName + " -> " + newName));
                } else {
                    Log("Rebound ActiveRenderInfo class");
                }
            }
            env->DeleteLocalRef(canonicalAri);
        } else if (g_activeRenderInfoClass) {
            std::string boundName = GetClassNameFromClass(env, g_activeRenderInfoClass);
            if (boundName.find("net.optifine.shaders.Shaders") != std::string::npos) {
                if (!g_modelViewField || !g_projectionField) {
                    Log("WARNING: ActiveRenderInfo still bound to Shaders fallback; waiting for canonical class.");
                }
            }
        }
    }

    if (!g_activeRenderInfoClass) {
        TRACE_PATH("late-bind-active-render-info");
        jclass ariLocal = nullptr;
        if (gcl) {
            ariLocal = LoadClassWithLoader(env, gcl, "net.minecraft.client.renderer.ActiveRenderInfo");
            TRACE_BRANCH("lateBindAriLoadClassHit", ariLocal != nullptr);
        }
        if (!ariLocal) {
            ariLocal = env->FindClass("net/minecraft/client/renderer/ActiveRenderInfo");
            TRACE_BRANCH("lateBindAriFindClassHit", ariLocal != nullptr);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }
        if (!ariLocal) {
            if (env->ExceptionCheck()) env->ExceptionClear();
        } else {
            g_activeRenderInfoClass = (jclass)env->NewGlobalRef(ariLocal);
            env->DeleteLocalRef(ariLocal);
            Log("Late-bound ActiveRenderInfo class");
        }
    }

    if (g_activeRenderInfoClass) {
        if (!g_modelViewField) {
            g_modelViewField = env->GetStaticFieldID(g_activeRenderInfoClass, "MODELVIEW", "Ljava/nio/FloatBuffer;");
            TRACE_BRANCH("lateBindModelViewCanonicalHit", g_modelViewField != nullptr);
            if (!g_modelViewField) {
                env->ExceptionClear();
            } else {
                Log("Late-bound MODELVIEW field");
            }
        }

        if (!g_projectionField) {
            g_projectionField = env->GetStaticFieldID(g_activeRenderInfoClass, "PROJECTION", "Ljava/nio/FloatBuffer;");
            TRACE_BRANCH("lateBindProjectionCanonicalHit", g_projectionField != nullptr);
            if (!g_projectionField) {
                env->ExceptionClear();
            } else {
                Log("Late-bound PROJECTION field");
            }
        }

        if (includeActionBarMappings) {
            TRACE_PATH("resolve-actionbar-mappings");
            // Ingame GUI / action-bar text mappings (for GTB on 1.8.9)
            if (!g_ingameGuiField) {
                g_ingameGuiField = env->GetFieldID(g_mcClass, "ingameGUI", "Lnet/minecraft/client/gui/GuiIngame;");
                TRACE_BRANCH("ingameGuiCanonicalHit", g_ingameGuiField != nullptr);
                if (!g_ingameGuiField) env->ExceptionClear();
                else Log("Found ingameGUI field");
            }

            if (g_ingameGuiField) {
                DWORD nowMs = GetTickCount();
                bool shouldProbe = g_actionBarFields.empty() || (nowMs - g_lastActionBarProbeMs) > 4000;
                TRACE_BRANCH("actionBarProbeDue", shouldProbe);
                if (shouldProbe) {
                    g_lastActionBarProbeMs = nowMs;
                    jobject ingameGui = env->GetObjectField(g_mcInstance, g_ingameGuiField);
                    if (ingameGui && !env->ExceptionCheck()) {
                        jclass hudClass = env->GetObjectClass(ingameGui);
                        if (hudClass) {
                            std::vector<ActionBarFieldRef> discovered;
                            auto addFieldByName = [&](const char* name, ActionBarFieldKind kind) {
                                const char* sig = (kind == ActionBarFieldChatComponent)
                                    ? "Lnet/minecraft/util/IChatComponent;"
                                    : "Ljava/lang/String;";
                                jfieldID fid = env->GetFieldID(hudClass, name, sig);
                                if (env->ExceptionCheck()) { env->ExceptionClear(); fid = nullptr; }
                                if (!fid) return;
                                for (size_t i = 0; i < discovered.size(); i++) {
                                    if (discovered[i].field == fid) return;
                                }
                                ActionBarFieldRef ref;
                                ref.field = fid;
                                ref.kind = kind;
                                discovered.push_back(ref);
                            };

                            const char* actionBarNameCandidates[] = {
                                // 1.8.9 often stores actionbar-like text in recordPlaying (String).
                                "recordPlaying",
                                "overlayMessage",
                                "field_73838_g",
                                // Title/subtitle paths used by some plugins/clients.
                                "displayedTitle",
                                "field_73845_h",
                                nullptr
                            };
                            for (int i = 0; actionBarNameCandidates[i]; i++) {
                                // Try both signatures on each candidate name because 1.8.9 forks
                                // differ on whether overlay text is String vs IChatComponent.
                                addFieldByName(actionBarNameCandidates[i], ActionBarFieldString);
                                addFieldByName(actionBarNameCandidates[i], ActionBarFieldChatComponent);
                            }

                            {
                                jclass cClass = env->FindClass("java/lang/Class");
                                jclass cField = env->FindClass("java/lang/reflect/Field");
                                jclass cMod = env->FindClass("java/lang/reflect/Modifier");
                                if (env->ExceptionCheck()) {
                                    env->ExceptionClear();
                                    cClass = nullptr;
                                    cField = nullptr;
                                    cMod = nullptr;
                                }

                                if (cClass && cField && cMod) {
                                    jmethodID mGetFields = env->GetMethodID(cClass, "getDeclaredFields", "()[Ljava/lang/reflect/Field;");
                                    jmethodID mFType = env->GetMethodID(cField, "getType", "()Ljava/lang/Class;");
                                    jmethodID mFMod = env->GetMethodID(cField, "getModifiers", "()I");
                                    jmethodID mIsStatic = env->GetStaticMethodID(cMod, "isStatic", "(I)Z");
                                    if (env->ExceptionCheck()) {
                                        env->ExceptionClear();
                                        mGetFields = nullptr;
                                    }

                                    if (mGetFields && mFType && mFMod && mIsStatic) {
                                        jobjectArray fields = (jobjectArray)env->CallObjectMethod(hudClass, mGetFields);
                                        if (env->ExceptionCheck()) { env->ExceptionClear(); fields = nullptr; }
                                        if (fields) {
                                            jsize fieldCount = env->GetArrayLength(fields);
                                            for (jsize i = 0; i < fieldCount; i++) {
                                                jobject fld = env->GetObjectArrayElement(fields, i);
                                                if (!fld) continue;

                                                jint mod = env->CallIntMethod(fld, mFMod);
                                                if (env->ExceptionCheck()) { env->ExceptionClear(); env->DeleteLocalRef(fld); continue; }
                                                if (env->CallStaticBooleanMethod(cMod, mIsStatic, mod) == JNI_TRUE) {
                                                    env->DeleteLocalRef(fld);
                                                    continue;
                                                }

                                                jclass ft = (jclass)env->CallObjectMethod(fld, mFType);
                                                if (env->ExceptionCheck()) { env->ExceptionClear(); ft = nullptr; }
                                                std::string typeName = ft ? GetClassNameFromClass(env, ft) : "";
                                                if (ft) env->DeleteLocalRef(ft);

                                                ActionBarFieldKind kind;
                                                bool typeOk = false;
                                                if (typeName == "net.minecraft.util.IChatComponent") {
                                                    kind = ActionBarFieldChatComponent;
                                                    typeOk = true;
                                                } else if (typeName == "java.lang.String") {
                                                    kind = ActionBarFieldString;
                                                    typeOk = true;
                                                }

                                                if (typeOk) {
                                                    jfieldID fid = env->FromReflectedField(fld);
                                                    if (env->ExceptionCheck()) { env->ExceptionClear(); fid = nullptr; }
                                                    if (fid) {
                                                        bool exists = false;
                                                        for (size_t k = 0; k < discovered.size(); k++) {
                                                            if (discovered[k].field == fid) { exists = true; break; }
                                                        }
                                                        if (!exists) {
                                                            ActionBarFieldRef ref;
                                                            ref.field = fid;
                                                            ref.kind = kind;
                                                            discovered.push_back(ref);
                                                        }
                                                    }
                                                }

                                                env->DeleteLocalRef(fld);
                                            }
                                            env->DeleteLocalRef(fields);
                                        }
                                    }
                                }

                                if (cClass) env->DeleteLocalRef(cClass);
                                if (cField) env->DeleteLocalRef(cField);
                                if (cMod) env->DeleteLocalRef(cMod);
                            }

                            if (!discovered.empty()) {
                                std::vector<ActionBarFieldRef> prioritized;
                                prioritized.reserve(discovered.size());

                                auto appendKind = [&](ActionBarFieldKind kind) {
                                    for (size_t i = 0; i < discovered.size(); i++) {
                                        if (discovered[i].kind == kind) {
                                            prioritized.push_back(discovered[i]);
                                        }
                                    }
                                };

                                // 1.8.9 GTB usually uses title/string path for the masked word.
                                appendKind(ActionBarFieldString);
                                appendKind(ActionBarFieldChatComponent);

                                if (!prioritized.empty()) {
                                    g_actionBarFields = prioritized;
                                } else {
                                    g_actionBarFields = discovered;
                                }
                                if ((int)g_actionBarFields.size() != g_lastActionBarFieldCountLogged) {
                                    int chatCount = 0;
                                    int stringCount = 0;
                                    for (size_t i = 0; i < g_actionBarFields.size(); i++) {
                                        if (g_actionBarFields[i].kind == ActionBarFieldChatComponent) chatCount++;
                                        else if (g_actionBarFields[i].kind == ActionBarFieldString) stringCount++;
                                    }
                                    Log("Found actionbar field candidates on GuiIngame: total="
                                        + std::to_string((int)g_actionBarFields.size())
                                        + " chat=" + std::to_string(chatCount)
                                        + " string=" + std::to_string(stringCount));
                                    g_lastActionBarFieldCountLogged = (int)g_actionBarFields.size();
                                }
                            } else if (g_lastActionBarFieldCountLogged != 0) {
                                Log("WARNING: no actionbar-like fields found on GuiIngame");
                                g_lastActionBarFieldCountLogged = 0;
                                TRACE_PATH("actionbar-discovery-empty");
                            }
                        }
                        if (hudClass) env->DeleteLocalRef(hudClass);
                        env->DeleteLocalRef(ingameGui);
                    } else if (env->ExceptionCheck()) {
                        env->ExceptionClear();
                    }
                }
            }

            if (!g_chatComponentGetTextMethod) {
                jclass chatCompClass = LoadClassWithLoader(env, gcl, "net.minecraft.util.IChatComponent");
                TRACE_BRANCH("chatComponentLoadClassHit", chatCompClass != nullptr);
                if (!chatCompClass) {
                    chatCompClass = env->FindClass("net/minecraft/util/IChatComponent");
                    TRACE_BRANCH("chatComponentFindClassHit", chatCompClass != nullptr);
                    if (env->ExceptionCheck()) env->ExceptionClear();
                }
                if (chatCompClass) {
                    g_chatComponentGetTextMethod = env->GetMethodID(chatCompClass, "getUnformattedText", "()Ljava/lang/String;");
                    TRACE_BRANCH("chatGetTextCanonicalHit", g_chatComponentGetTextMethod != nullptr);
                    if (!g_chatComponentGetTextMethod) {
                        env->ExceptionClear();
                        g_chatComponentGetTextMethod = env->GetMethodID(chatCompClass, "func_150260_c", "()Ljava/lang/String;");
                        TRACE_BRANCH("chatGetTextObfHit", g_chatComponentGetTextMethod != nullptr);
                    }
                    if (!g_chatComponentGetTextMethod) env->ExceptionClear();
                    else Log("Found IChatComponent#getUnformattedText method");
                }
            }
        }
    }

    if (!g_renderManagerField && g_mcClass) {
        g_renderManagerField = env->GetFieldID(g_mcClass, "renderManager", "Lnet/minecraft/client/renderer/entity/RenderManager;");
        if (!g_renderManagerField) {
            env->ExceptionClear();
            g_renderManagerField = env->GetFieldID(g_mcClass, "field_175616_W", "Lnet/minecraft/client/renderer/entity/RenderManager;");
        }
        if (!g_renderManagerField) {
            env->ExceptionClear();
        } else {
            Log("Late-bound renderManager field");
        }
    }

    if (g_renderManagerField && (!g_viewerPosXField || !g_viewerPosYField || !g_viewerPosZField)) {
        jobject rmObj = env->GetObjectField(g_mcInstance, g_renderManagerField);
        if (rmObj) {
            jclass rmClass = env->GetObjectClass(rmObj);
            if (rmClass) {
                if (!g_viewerPosXField) {
                    g_viewerPosXField = env->GetFieldID(rmClass, "viewerPosX", "D");
                    if (!g_viewerPosXField) {
                        env->ExceptionClear();
                        g_viewerPosXField = env->GetFieldID(rmClass, "field_78725_b", "D");
                    }
                    if (!g_viewerPosXField) {
                        env->ExceptionClear();
                        g_viewerPosXField = env->GetFieldID(rmClass, "o", "D");
                    }
                    if (!g_viewerPosXField) env->ExceptionClear();
                }

                if (!g_viewerPosYField) {
                    g_viewerPosYField = env->GetFieldID(rmClass, "viewerPosY", "D");
                    if (!g_viewerPosYField) {
                        env->ExceptionClear();
                        g_viewerPosYField = env->GetFieldID(rmClass, "field_78726_c", "D");
                    }
                    if (!g_viewerPosYField) {
                        env->ExceptionClear();
                        g_viewerPosYField = env->GetFieldID(rmClass, "p", "D");
                    }
                    if (!g_viewerPosYField) env->ExceptionClear();
                }

                if (!g_viewerPosZField) {
                    g_viewerPosZField = env->GetFieldID(rmClass, "viewerPosZ", "D");
                    if (!g_viewerPosZField) {
                        env->ExceptionClear();
                        g_viewerPosZField = env->GetFieldID(rmClass, "field_78723_d", "D");
                    }
                    if (!g_viewerPosZField) {
                        env->ExceptionClear();
                        g_viewerPosZField = env->GetFieldID(rmClass, "q", "D");
                    }
                    if (!g_viewerPosZField) env->ExceptionClear();
                }

                if (g_viewerPosXField && g_viewerPosYField && g_viewerPosZField) {
                    Log("Late-bound RenderManager viewerPos fields");
                }

                env->DeleteLocalRef(rmClass);
            }
            env->DeleteLocalRef(rmObj);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }
}

static bool NeedsCoreMappingRefresh() {
    bool need = !g_mapped
        || !g_mcInstance
        || !g_theWorldField;
    TRACE_BRANCH("needsCoreRefresh", need);
    return need;
}

static bool RunHeavyDiscovery(JNIEnv* env, const char* reason) {
    TRACE_PATH("enter");
    if (!env) return false;
    DWORD startedAt = GetTickCount();
    InterlockedExchange(&g_heavyDiscoveryInProgress, 1);
    Log(std::string("Heavy mapping discovery start: ") + (reason ? reason : "unspecified"));
    bool ok = DiscoverMappings(env);
    DWORD elapsedMs = GetTickCount() - startedAt;
    InterlockedExchange(&g_heavyDiscoveryInProgress, 0);
    Log(std::string("Heavy mapping discovery ")
        + (ok ? "complete" : "failed")
        + " in " + std::to_string(elapsedMs) + "ms");
    return ok;
}

static void TryResolveHoldingBlockMappings(JNIEnv* env);
static void TryResolvePlayerCoreMappings(JNIEnv* env);
static void TryResolveChestEspMappings(JNIEnv* env);

static void TryResolveScreenFieldDirect(JNIEnv* env) {
    TRACE_PATH("enter");
    bool prerequisites = (env && g_mcClass);
    TRACE_BRANCH("prerequisitesMet", prerequisites);
    if (!prerequisites) return;

    jfieldID direct = env->GetFieldID(g_mcClass, "currentScreen", "Lnet/minecraft/client/gui/GuiScreen;");
    TRACE_BRANCH("currentScreenCanonicalHit", direct != nullptr);
    if (!direct) {
        env->ExceptionClear();
        direct = env->GetFieldID(g_mcClass, "field_71462_r", "Lnet/minecraft/client/gui/GuiScreen;");
        TRACE_BRANCH("currentScreenSrgHit", direct != nullptr);
        if (!direct) {
            env->ExceptionClear();
            return;
        }
    }

    if (g_currentScreenField != direct) {
        bool hadPrevious = (g_currentScreenField != nullptr);
        g_currentScreenField = direct;
        Log(hadPrevious ? "Rebound currentScreen field" : "Late-bound currentScreen field");
    }
}

static bool NeedsRenderRecoveryMappings() {
    bool need = !g_playerEntitiesField
        || !g_loadedTileEntityListField
        || !g_activeRenderInfoClass
        || !g_modelViewField
        || !g_projectionField
        || !g_renderManagerField
        || !g_viewerPosXField
        || !g_viewerPosYField
        || !g_viewerPosZField
        || !g_tileEntityPosField
        || !g_blockPosGetX
        || !g_blockPosGetY
        || !g_blockPosGetZ;
    TRACE_BRANCH("needsRenderRecovery", need);
    return need;
}

static void TryFallbackRenderRecovery(JNIEnv* env, bool hasReadyPlayer) {
    TRACE_PATH("enter");
    TRACE_BRANCH("hasReadyPlayer", hasReadyPlayer);
    if (!env || !hasReadyPlayer) return;
    bool needRecovery = NeedsRenderRecoveryMappings();
    TRACE_BRANCH("needRecovery", needRecovery);
    if (!needRecovery) return;

    static DWORD nextRenderFallbackAt = 0;
    DWORD now = GetTickCount();
    bool cooldownElapsed = now >= nextRenderFallbackAt;
    TRACE_BRANCH("fallbackCooldownElapsed", cooldownElapsed);
    if (!cooldownElapsed) return;

    Log("Render mappings incomplete, running lightweight fallback recovery...");
    TryResolveScreenFieldDirect(env);
    TryResolveHoldingBlockMappings(env);
    TryResolvePlayerCoreMappings(env);
    TryResolveChestEspMappings(env);
    TryResolveWorldMappings(env);
    TryResolveRenderMappings(env, false);

    bool stillMissing = NeedsRenderRecoveryMappings();
    if (stillMissing) {
        nextRenderFallbackAt = now + 60000;
        Log("Render fallback partial; heavy discovery disabled by core-only policy; next retry in 60000ms");
    } else {
        nextRenderFallbackAt = now + 60000;
        Log("Render fallback recovery complete");
    }
}

static void TryResolveHoldingBlockMappings(JNIEnv* env) {
    TRACE_PATH("enter");
    TRACE_BRANCH("envAvailable", env != nullptr);
    if (!env) return;

    // Late-resolve ItemBlock class
    if (!g_itemBlockClass) {
        jobject gcl = EnsureGameClassLoader(env);
        jclass ibClass = nullptr;
        if (gcl) {
            ibClass = LoadClassWithLoader(env, gcl, "net.minecraft.item.ItemBlock");
            TRACE_BRANCH("itemBlockLoadClassHit", ibClass != nullptr);
        }
        if (!ibClass) {
            ibClass = env->FindClass("net/minecraft/item/ItemBlock");
            TRACE_BRANCH("itemBlockFindClassHit", ibClass != nullptr);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }
        if (ibClass && !env->ExceptionCheck()) {
            g_itemBlockClass = (jclass)env->NewGlobalRef(ibClass);
            Log("Late-bound ItemBlock class");
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
    }

    // Late-resolve g_inventoryField (THE KEY FIX: this was missing before!)
    if (!g_inventoryField && g_mcInstance && g_thePlayerField) {
        jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
        if (!env->ExceptionCheck() && player) {
            // Get reflection helpers
            jclass cClass = env->FindClass("java/lang/Class");
            jclass cField = env->FindClass("java/lang/reflect/Field");
            if (cClass && cField) {
                jmethodID mGetFields = env->GetMethodID(cClass, "getDeclaredFields", "()[Ljava/lang/reflect/Field;");
                jmethodID mGetSuper = env->GetMethodID(cClass, "getSuperclass", "()Ljava/lang/Class;");
                jmethodID mFType = env->GetMethodID(cField, "getType", "()Ljava/lang/Class;");
                
                if (mGetFields && mGetSuper && mFType) {
                    jclass pClass = env->GetObjectClass(player);
                    jclass walk = pClass;
                    int depth = 0;
                    while (walk && depth < 5 && !g_inventoryField) {
                        jobjectArray fields = (jobjectArray)env->CallObjectMethod(walk, mGetFields);
                        if (fields && !env->ExceptionCheck()) {
                            jsize fc = env->GetArrayLength(fields);
                            for (int i = 0; i < fc; i++) {
                                jobject f = env->GetObjectArrayElement(fields, i);
                                if (!f) continue;
                                jclass ft = (jclass)env->CallObjectMethod(f, mFType);
                                if (ft && !env->ExceptionCheck()) {
                                    std::string ftn = GetClassNameFromClass(env, ft);
                                    if (ftn.find("InventoryPlayer") != std::string::npos) {
                                        g_inventoryField = env->FromReflectedField(f);
                                        Log("Late-bound inventory field");
                                        break;
                                    }
                                    env->DeleteLocalRef(ft);
                                } else if (env->ExceptionCheck()) {
                                    env->ExceptionClear();
                                }
                                env->DeleteLocalRef(f);
                            }
                            env->DeleteLocalRef(fields);
                        } else if (env->ExceptionCheck()) {
                            env->ExceptionClear();
                        }
                        jclass nextWalk = (jclass)env->CallObjectMethod(walk, mGetSuper);
                        if (walk != pClass) env->DeleteLocalRef(walk);
                        walk = nextWalk;
                        depth++;
                    }
                    if (walk && walk != pClass) env->DeleteLocalRef(walk);
                    if (pClass) env->DeleteLocalRef(pClass);
                }
            }
            if (env->ExceptionCheck()) env->ExceptionClear();
            env->DeleteLocalRef(player);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    // Late-resolve getCurrentItem method (only if we now have inventoryField)
    if (!g_getCurrentItemMethod && g_mcInstance && g_thePlayerField && g_inventoryField) {
        TRACE_PATH("resolve-getCurrentItem");
        jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
        if (!env->ExceptionCheck() && player) {
            jobject inventory = env->GetObjectField(player, g_inventoryField);
            if (!env->ExceptionCheck() && inventory) {
                jclass invClass = env->GetObjectClass(inventory);
                if (invClass) {
                    g_getCurrentItemMethod = env->GetMethodID(invClass, "getCurrentItem", "()Lnet/minecraft/item/ItemStack;");
                    TRACE_BRANCH("getCurrentItemCanonicalHit", g_getCurrentItemMethod != nullptr);
                    if (!g_getCurrentItemMethod) {
                        env->ExceptionClear();
                        g_getCurrentItemMethod = env->GetMethodID(invClass, "func_70448_g", "()Lnet/minecraft/item/ItemStack;");
                        TRACE_BRANCH("getCurrentItemObfHit", g_getCurrentItemMethod != nullptr);
                    }
                    if (!g_getCurrentItemMethod) env->ExceptionClear();
                    else Log("Late-bound InventoryPlayer.getCurrentItem");
                    env->DeleteLocalRef(invClass);
                } else if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                }
                env->DeleteLocalRef(inventory);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
            env->DeleteLocalRef(player);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }
}

static void TryResolvePlayerCoreMappings(JNIEnv* env) {
    TRACE_PATH("enter");
    bool prerequisites = (env && g_mcInstance && g_thePlayerField);
    TRACE_BRANCH("prerequisitesMet", prerequisites);
    if (!prerequisites) return;

    const bool needPlayerCore =
        !g_getHealthMethod ||
        !g_posXField || !g_posYField || !g_posZField ||
        !g_rotationYawField || !g_rotationPitchField ||
        !g_getNameMethod || !g_getHeldItemMethod || !g_getTotalArmorValueMethod ||
        !g_lastTickPosXField || !g_lastTickPosYField || !g_lastTickPosZField;
    TRACE_BRANCH("needPlayerCore", needPlayerCore);
    if (!needPlayerCore) return;

    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (!player) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return;
    }

    jclass startClass = env->GetObjectClass(player);
    jclass walk = startClass;
    jclass classClass = env->FindClass("java/lang/Class");
    jmethodID mGetSuper = classClass ? env->GetMethodID(classClass, "getSuperclass", "()Ljava/lang/Class;") : nullptr;
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        mGetSuper = nullptr;
    }
    int depth = 0;
    while (walk && depth < 10 && mGetSuper) {
        if (!g_getHealthMethod) {
            g_getHealthMethod = env->GetMethodID(walk, "getHealth", "()F");
            TRACE_BRANCH("playerCoreGetHealthCanonicalHit", g_getHealthMethod != nullptr);
            if (!g_getHealthMethod) {
                env->ExceptionClear();
                g_getHealthMethod = env->GetMethodID(walk, "func_110143_aJ", "()F");
                TRACE_BRANCH("playerCoreGetHealthObfHit", g_getHealthMethod != nullptr);
            }
            if (!g_getHealthMethod) env->ExceptionClear();
        }
        if (!g_posXField) {
            g_posXField = env->GetFieldID(walk, "posX", "D");
            TRACE_BRANCH("playerCorePosXCanonicalHit", g_posXField != nullptr);
            if (!g_posXField) {
                env->ExceptionClear();
                g_posXField = env->GetFieldID(walk, "field_70165_t", "D");
                TRACE_BRANCH("playerCorePosXObfHit", g_posXField != nullptr);
            }
            if (!g_posXField) env->ExceptionClear();
        }
        if (!g_posYField) {
            g_posYField = env->GetFieldID(walk, "posY", "D");
            TRACE_BRANCH("playerCorePosYCanonicalHit", g_posYField != nullptr);
            if (!g_posYField) {
                env->ExceptionClear();
                g_posYField = env->GetFieldID(walk, "field_70163_u", "D");
                TRACE_BRANCH("playerCorePosYObfHit", g_posYField != nullptr);
            }
            if (!g_posYField) env->ExceptionClear();
        }
        if (!g_posZField) {
            g_posZField = env->GetFieldID(walk, "posZ", "D");
            TRACE_BRANCH("playerCorePosZCanonicalHit", g_posZField != nullptr);
            if (!g_posZField) {
                env->ExceptionClear();
                g_posZField = env->GetFieldID(walk, "field_70161_v", "D");
                TRACE_BRANCH("playerCorePosZObfHit", g_posZField != nullptr);
            }
            if (!g_posZField) env->ExceptionClear();
        }
        if (!g_rotationYawField) {
            g_rotationYawField = env->GetFieldID(walk, "rotationYaw", "F");
            if (!g_rotationYawField) {
                env->ExceptionClear();
                g_rotationYawField = env->GetFieldID(walk, "field_70177_z", "F");
            }
            if (!g_rotationYawField) env->ExceptionClear();
        }
        if (!g_rotationPitchField) {
            g_rotationPitchField = env->GetFieldID(walk, "rotationPitch", "F");
            if (!g_rotationPitchField) {
                env->ExceptionClear();
                g_rotationPitchField = env->GetFieldID(walk, "field_70125_A", "F");
            }
            if (!g_rotationPitchField) env->ExceptionClear();
        }
        if (!g_getNameMethod) {
            g_getNameMethod = env->GetMethodID(walk, "getName", "()Ljava/lang/String;");
            if (!g_getNameMethod) {
                env->ExceptionClear();
                g_getNameMethod = env->GetMethodID(walk, "func_70005_c_", "()Ljava/lang/String;");
            }
            if (!g_getNameMethod) env->ExceptionClear();
        }
        if (!g_getHeldItemMethod) {
            g_getHeldItemMethod = env->GetMethodID(walk, "getHeldItem", "()Lnet/minecraft/item/ItemStack;");
            if (!g_getHeldItemMethod) {
                env->ExceptionClear();
                g_getHeldItemMethod = env->GetMethodID(walk, "func_70694_bm", "()Lnet/minecraft/item/ItemStack;");
            }
            if (!g_getHeldItemMethod) env->ExceptionClear();
        }
        if (!g_getTotalArmorValueMethod) {
            g_getTotalArmorValueMethod = env->GetMethodID(walk, "getTotalArmorValue", "()I");
            if (!g_getTotalArmorValueMethod) {
                env->ExceptionClear();
                g_getTotalArmorValueMethod = env->GetMethodID(walk, "func_70658_aO", "()I");
            }
            if (!g_getTotalArmorValueMethod) env->ExceptionClear();
        }
        if (!g_lastTickPosXField) {
            g_lastTickPosXField = env->GetFieldID(walk, "lastTickPosX", "D");
            if (!g_lastTickPosXField) {
                env->ExceptionClear();
                g_lastTickPosXField = env->GetFieldID(walk, "field_70142_S", "D");
            }
            if (!g_lastTickPosXField) {
                env->ExceptionClear();
                g_lastTickPosXField = env->GetFieldID(walk, "prevPosX", "D");
            }
            if (!g_lastTickPosXField) env->ExceptionClear();
        }
        if (!g_lastTickPosYField) {
            g_lastTickPosYField = env->GetFieldID(walk, "lastTickPosY", "D");
            if (!g_lastTickPosYField) {
                env->ExceptionClear();
                g_lastTickPosYField = env->GetFieldID(walk, "field_70137_T", "D");
            }
            if (!g_lastTickPosYField) {
                env->ExceptionClear();
                g_lastTickPosYField = env->GetFieldID(walk, "prevPosY", "D");
            }
            if (!g_lastTickPosYField) env->ExceptionClear();
        }
        if (!g_lastTickPosZField) {
            g_lastTickPosZField = env->GetFieldID(walk, "lastTickPosZ", "D");
            if (!g_lastTickPosZField) {
                env->ExceptionClear();
                g_lastTickPosZField = env->GetFieldID(walk, "field_70136_U", "D");
            }
            if (!g_lastTickPosZField) {
                env->ExceptionClear();
                g_lastTickPosZField = env->GetFieldID(walk, "prevPosZ", "D");
            }
            if (!g_lastTickPosZField) env->ExceptionClear();
        }

        if (g_getHealthMethod &&
            g_posXField && g_posYField && g_posZField &&
            g_rotationYawField && g_rotationPitchField &&
            g_getNameMethod && g_getHeldItemMethod && g_getTotalArmorValueMethod &&
            g_lastTickPosXField && g_lastTickPosYField && g_lastTickPosZField) {
            break;
        }

        jclass nextWalk = (jclass)env->CallObjectMethod(walk, mGetSuper);
        if (env->ExceptionCheck()) {
            env->ExceptionClear();
            break;
        }
        if (walk != startClass) env->DeleteLocalRef(walk);
        walk = nextWalk;
        depth++;
    }
    if (classClass) env->DeleteLocalRef(classClass);
    if (walk && walk != startClass) env->DeleteLocalRef(walk);
    if (startClass) env->DeleteLocalRef(startClass);
    env->DeleteLocalRef(player);
}

static std::string Utf8FromJStringLegacy(JNIEnv* env, jstring js) {
    if (!env || !js) return "";
    const char* c = env->GetStringUTFChars(js, nullptr);
    std::string out = c ? c : "";
    if (c) env->ReleaseStringUTFChars(js, c);
    return out;
}

static void ResetLegacyNametagSuppressionState(JNIEnv* env, const char* reason) {
    if (g_lastLegacyNametagSuppressionWorld && env) {
        env->DeleteGlobalRef(g_lastLegacyNametagSuppressionWorld);
        g_lastLegacyNametagSuppressionWorld = nullptr;
    }
    g_hiddenNametagOriginalTeamByPlayerLegacy.clear();
    g_legacyNametagSuppressionActive = false;
    g_loggedLegacyNametagSuppressionUnavailable = false;
    if (reason && *reason) {
        Log(std::string("NametagHideVanilla: legacy suppression state reset (") + reason + ").");
    }
}

static void TrackLegacySuppressionWorldContext(JNIEnv* env, jobject worldObj) {
    if (!env || !worldObj) return;
    if (!g_lastLegacyNametagSuppressionWorld) {
        g_lastLegacyNametagSuppressionWorld = env->NewGlobalRef(worldObj);
        return;
    }

    bool changed = (env->IsSameObject(worldObj, g_lastLegacyNametagSuppressionWorld) == JNI_FALSE);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        changed = false;
    }
    if (changed) {
        if (g_lastLegacyNametagSuppressionWorld) {
            env->DeleteGlobalRef(g_lastLegacyNametagSuppressionWorld);
            g_lastLegacyNametagSuppressionWorld = nullptr;
        }
        g_lastLegacyNametagSuppressionWorld = env->NewGlobalRef(worldObj);
        if (g_legacyNametagSuppressionActive || !g_hiddenNametagOriginalTeamByPlayerLegacy.empty()) {
            g_hiddenNametagOriginalTeamByPlayerLegacy.clear();
            g_legacyNametagSuppressionActive = false;
            g_loggedLegacyNametagSuppressionUnavailable = false;
            Log("NametagHideVanilla: legacy world context changed; dropped suppression cache to avoid cross-world scoreboard mutations.");
        }
    }
}

static bool EnsureLegacyNametagTeamMappings(JNIEnv* env, jobject worldObj) {
    if (!env || !worldObj) return false;

    if (!g_scoreboardClassLegacy) {
        jclass c = nullptr;
        jobject gcl = EnsureGameClassLoader(env);
        if (gcl) c = LoadClassWithLoader(env, gcl, "net.minecraft.scoreboard.Scoreboard");
        if (!c) {
            c = env->FindClass("net/minecraft/scoreboard/Scoreboard");
            if (env->ExceptionCheck()) { env->ExceptionClear(); c = nullptr; }
        }
        if (c) {
            g_scoreboardClassLegacy = (jclass)env->NewGlobalRef(c);
            env->DeleteLocalRef(c);
        }
    }
    if (!g_scorePlayerTeamClassLegacy) {
        jclass c = nullptr;
        jobject gcl = EnsureGameClassLoader(env);
        if (gcl) c = LoadClassWithLoader(env, gcl, "net.minecraft.scoreboard.ScorePlayerTeam");
        if (!c) {
            c = env->FindClass("net/minecraft/scoreboard/ScorePlayerTeam");
            if (env->ExceptionCheck()) { env->ExceptionClear(); c = nullptr; }
        }
        if (c) {
            g_scorePlayerTeamClassLegacy = (jclass)env->NewGlobalRef(c);
            env->DeleteLocalRef(c);
        }
    }
    if (!g_teamEnumVisibleClassLegacy) {
        jclass c = nullptr;
        jobject gcl = EnsureGameClassLoader(env);
        if (gcl) c = LoadClassWithLoader(env, gcl, "net.minecraft.scoreboard.Team$EnumVisible");
        if (!c) {
            c = env->FindClass("net/minecraft/scoreboard/Team$EnumVisible");
            if (env->ExceptionCheck()) { env->ExceptionClear(); c = nullptr; }
        }
        if (c) {
            g_teamEnumVisibleClassLegacy = (jclass)env->NewGlobalRef(c);
            env->DeleteLocalRef(c);
        }
    }

    if (!g_worldGetScoreboardMethod) {
        jclass worldCls = env->GetObjectClass(worldObj);
        if (worldCls && !env->ExceptionCheck()) {
            const char* names[] = { "getScoreboard", "func_96441_U", nullptr };
            const char* sigs[] = { "()Lnet/minecraft/scoreboard/Scoreboard;", nullptr };
            for (int ni = 0; names[ni] && !g_worldGetScoreboardMethod; ni++) {
                for (int si = 0; sigs[si] && !g_worldGetScoreboardMethod; si++) {
                    g_worldGetScoreboardMethod = env->GetMethodID(worldCls, names[ni], sigs[si]);
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_worldGetScoreboardMethod = nullptr; }
                }
            }
            env->DeleteLocalRef(worldCls);
        } else {
            env->ExceptionClear();
        }
    }

    if (g_scoreboardClassLegacy) {
        if (!g_scoreboardGetTeamMethodLegacy) {
            const char* names[] = { "getTeam", "func_96508_e", nullptr };
            for (int i = 0; names[i] && !g_scoreboardGetTeamMethodLegacy; i++) {
                g_scoreboardGetTeamMethodLegacy = env->GetMethodID(g_scoreboardClassLegacy, names[i], "(Ljava/lang/String;)Lnet/minecraft/scoreboard/ScorePlayerTeam;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_scoreboardGetTeamMethodLegacy = nullptr; }
            }
        }
        if (!g_scoreboardCreateTeamMethodLegacy) {
            const char* names[] = { "createTeam", "func_96527_f", nullptr };
            for (int i = 0; names[i] && !g_scoreboardCreateTeamMethodLegacy; i++) {
                g_scoreboardCreateTeamMethodLegacy = env->GetMethodID(g_scoreboardClassLegacy, names[i], "(Ljava/lang/String;)Lnet/minecraft/scoreboard/ScorePlayerTeam;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_scoreboardCreateTeamMethodLegacy = nullptr; }
            }
        }
        if (!g_scoreboardRemoveTeamMethodLegacy) {
            const char* names[] = { "removeTeam", "func_96511_d", nullptr };
            for (int i = 0; names[i] && !g_scoreboardRemoveTeamMethodLegacy; i++) {
                g_scoreboardRemoveTeamMethodLegacy = env->GetMethodID(g_scoreboardClassLegacy, names[i], "(Lnet/minecraft/scoreboard/ScorePlayerTeam;)V");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_scoreboardRemoveTeamMethodLegacy = nullptr; }
            }
        }
        if (!g_scoreboardAddPlayerToTeamMethodLegacy) {
            const char* names[] = { "addPlayerToTeam", "func_151392_a", nullptr };
            for (int i = 0; names[i] && !g_scoreboardAddPlayerToTeamMethodLegacy; i++) {
                g_scoreboardAddPlayerToTeamMethodLegacy = env->GetMethodID(g_scoreboardClassLegacy, names[i], "(Ljava/lang/String;Ljava/lang/String;)Z");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_scoreboardAddPlayerToTeamMethodLegacy = nullptr; }
            }
        }
        if (!g_scoreboardGetPlayersTeamMethodLegacy) {
            const char* names[] = { "getPlayersTeam", "func_96509_i", nullptr };
            for (int i = 0; names[i] && !g_scoreboardGetPlayersTeamMethodLegacy; i++) {
                g_scoreboardGetPlayersTeamMethodLegacy = env->GetMethodID(g_scoreboardClassLegacy, names[i], "(Ljava/lang/String;)Lnet/minecraft/scoreboard/ScorePlayerTeam;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_scoreboardGetPlayersTeamMethodLegacy = nullptr; }
            }
        }
        if (!g_scoreboardRemovePlayerFromTeamsMethodLegacy) {
            const char* names[] = { "removePlayerFromTeams", "func_96524_g", nullptr };
            for (int i = 0; names[i] && !g_scoreboardRemovePlayerFromTeamsMethodLegacy; i++) {
                g_scoreboardRemovePlayerFromTeamsMethodLegacy = env->GetMethodID(g_scoreboardClassLegacy, names[i], "(Ljava/lang/String;)Z");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_scoreboardRemovePlayerFromTeamsMethodLegacy = nullptr; }
            }
        }
    }

    if (g_scorePlayerTeamClassLegacy) {
        if (!g_scorePlayerTeamGetRegisteredNameMethodLegacy) {
            const char* names[] = { "getRegisteredName", "func_96661_b", nullptr };
            for (int i = 0; names[i] && !g_scorePlayerTeamGetRegisteredNameMethodLegacy; i++) {
                g_scorePlayerTeamGetRegisteredNameMethodLegacy = env->GetMethodID(g_scorePlayerTeamClassLegacy, names[i], "()Ljava/lang/String;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_scorePlayerTeamGetRegisteredNameMethodLegacy = nullptr; }
            }
        }
        if (!g_scorePlayerTeamSetNameTagVisibilityMethodLegacy) {
            const char* names[] = { "setNameTagVisibility", "func_178772_a", nullptr };
            for (int i = 0; names[i] && !g_scorePlayerTeamSetNameTagVisibilityMethodLegacy; i++) {
                g_scorePlayerTeamSetNameTagVisibilityMethodLegacy = env->GetMethodID(g_scorePlayerTeamClassLegacy, names[i], "(Lnet/minecraft/scoreboard/Team$EnumVisible;)V");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_scorePlayerTeamSetNameTagVisibilityMethodLegacy = nullptr; }
            }
        }
    }

    if (g_teamEnumVisibleClassLegacy && !g_teamEnumVisibleNeverLegacy) {
        jfieldID neverField = env->GetStaticFieldID(g_teamEnumVisibleClassLegacy, "NEVER", "Lnet/minecraft/scoreboard/Team$EnumVisible;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); neverField = nullptr; }
        if (neverField) {
            jobject neverObj = env->GetStaticObjectField(g_teamEnumVisibleClassLegacy, neverField);
            if (env->ExceptionCheck()) { env->ExceptionClear(); neverObj = nullptr; }
            if (neverObj) {
                g_teamEnumVisibleNeverLegacy = env->NewGlobalRef(neverObj);
                env->DeleteLocalRef(neverObj);
            }
        }
        if (!g_teamEnumVisibleNeverLegacy) {
            jmethodID valuesMid = env->GetStaticMethodID(g_teamEnumVisibleClassLegacy, "values", "()[Lnet/minecraft/scoreboard/Team$EnumVisible;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); valuesMid = nullptr; }
            if (valuesMid) {
                jobjectArray vals = (jobjectArray)env->CallStaticObjectMethod(g_teamEnumVisibleClassLegacy, valuesMid);
                if (env->ExceptionCheck()) { env->ExceptionClear(); vals = nullptr; }
                if (vals) {
                    jsize len = env->GetArrayLength(vals);
                    if (len > 1) {
                        jobject neverObj = env->GetObjectArrayElement(vals, 1);
                        if (neverObj) {
                            g_teamEnumVisibleNeverLegacy = env->NewGlobalRef(neverObj);
                            env->DeleteLocalRef(neverObj);
                        }
                    }
                    env->DeleteLocalRef(vals);
                }
            }
        }
    }

    return g_worldGetScoreboardMethod
        && g_scoreboardGetTeamMethodLegacy
        && g_scoreboardCreateTeamMethodLegacy
        && g_scoreboardRemoveTeamMethodLegacy
        && g_scoreboardAddPlayerToTeamMethodLegacy
        && g_scoreboardGetPlayersTeamMethodLegacy
        && g_scoreboardRemovePlayerFromTeamsMethodLegacy
        && g_scorePlayerTeamGetRegisteredNameMethodLegacy
        && g_scorePlayerTeamSetNameTagVisibilityMethodLegacy
        && g_teamEnumVisibleNeverLegacy;
}

static jobject GetLegacyScoreboard(JNIEnv* env, jobject worldObj) {
    if (!env || !worldObj || !g_worldGetScoreboardMethod) return nullptr;
    jobject scoreboardObj = env->CallObjectMethod(worldObj, g_worldGetScoreboardMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); scoreboardObj = nullptr; }
    return scoreboardObj;
}

static jobject EnsureLegacyHideTeam(JNIEnv* env, jobject scoreboardObj) {
    if (!env || !scoreboardObj || !g_scoreboardGetTeamMethodLegacy || !g_scoreboardCreateTeamMethodLegacy) return nullptr;
    static const char* kHideTeamName = "lc_hide_tags";

    jstring jTeamName = env->NewStringUTF(kHideTeamName);
    if (!jTeamName) return nullptr;

    jobject teamObj = env->CallObjectMethod(scoreboardObj, g_scoreboardGetTeamMethodLegacy, jTeamName);
    if (env->ExceptionCheck()) { env->ExceptionClear(); teamObj = nullptr; }
    if (!teamObj) {
        teamObj = env->CallObjectMethod(scoreboardObj, g_scoreboardCreateTeamMethodLegacy, jTeamName);
        if (env->ExceptionCheck()) { env->ExceptionClear(); teamObj = nullptr; }
    }

    if (teamObj && g_scorePlayerTeamSetNameTagVisibilityMethodLegacy && g_teamEnumVisibleNeverLegacy) {
        env->CallVoidMethod(teamObj, g_scorePlayerTeamSetNameTagVisibilityMethodLegacy, g_teamEnumVisibleNeverLegacy);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }

    env->DeleteLocalRef(jTeamName);
    return teamObj;
}

static bool ApplyLegacyVanillaNametagSuppression(JNIEnv* env, jobject scoreboardObj, jobject hideTeamObj, const std::string& playerName) {
    if (!env || !scoreboardObj || !hideTeamObj || playerName.empty()) return false;
    static const char* kHideTeamName = "lc_hide_tags";

    jstring jPlayerName = env->NewStringUTF(playerName.c_str());
    if (!jPlayerName) return false;

    if (g_hiddenNametagOriginalTeamByPlayerLegacy.find(playerName) == g_hiddenNametagOriginalTeamByPlayerLegacy.end()) {
        std::string originalTeamName;
        if (g_scoreboardGetPlayersTeamMethodLegacy && g_scorePlayerTeamGetRegisteredNameMethodLegacy) {
            jobject oldTeam = env->CallObjectMethod(scoreboardObj, g_scoreboardGetPlayersTeamMethodLegacy, jPlayerName);
            if (env->ExceptionCheck()) { env->ExceptionClear(); oldTeam = nullptr; }
            if (oldTeam) {
                jstring jOldTeamName = (jstring)env->CallObjectMethod(oldTeam, g_scorePlayerTeamGetRegisteredNameMethodLegacy);
                if (env->ExceptionCheck()) { env->ExceptionClear(); jOldTeamName = nullptr; }
                if (jOldTeamName) {
                    originalTeamName = Utf8FromJStringLegacy(env, jOldTeamName);
                    env->DeleteLocalRef(jOldTeamName);
                }
                env->DeleteLocalRef(oldTeam);
            }
        }
        g_hiddenNametagOriginalTeamByPlayerLegacy[playerName] = originalTeamName;
    }

    jstring jHideTeamName = env->NewStringUTF(kHideTeamName);
    if (!jHideTeamName) {
        env->DeleteLocalRef(jPlayerName);
        return false;
    }
    jboolean applied = env->CallBooleanMethod(scoreboardObj, g_scoreboardAddPlayerToTeamMethodLegacy, jPlayerName, jHideTeamName);
    bool ok = !env->ExceptionCheck();
    if (!ok) env->ExceptionClear();

    env->DeleteLocalRef(jHideTeamName);
    env->DeleteLocalRef(jPlayerName);
    return ok && (applied == JNI_TRUE || applied == JNI_FALSE);
}

static void RestoreLegacyVanillaNametagSuppression(JNIEnv* env, jobject scoreboardObj) {
    if (!env || !scoreboardObj || !g_scoreboardRemovePlayerFromTeamsMethodLegacy) {
        g_hiddenNametagOriginalTeamByPlayerLegacy.clear();
        return;
    }
    for (const auto& entry : g_hiddenNametagOriginalTeamByPlayerLegacy) {
        const std::string& playerName = entry.first;
        const std::string& originalTeamName = entry.second;
        if (playerName.empty()) continue;

        jstring jPlayerName = env->NewStringUTF(playerName.c_str());
        if (!jPlayerName) continue;

        env->CallBooleanMethod(scoreboardObj, g_scoreboardRemovePlayerFromTeamsMethodLegacy, jPlayerName);
        if (env->ExceptionCheck()) env->ExceptionClear();

        if (!originalTeamName.empty() && g_scoreboardAddPlayerToTeamMethodLegacy) {
            jstring jOriginalTeamName = env->NewStringUTF(originalTeamName.c_str());
            if (jOriginalTeamName) {
                env->CallBooleanMethod(scoreboardObj, g_scoreboardAddPlayerToTeamMethodLegacy, jPlayerName, jOriginalTeamName);
                if (env->ExceptionCheck()) env->ExceptionClear();
                env->DeleteLocalRef(jOriginalTeamName);
            }
        }
        env->DeleteLocalRef(jPlayerName);
    }
    g_hiddenNametagOriginalTeamByPlayerLegacy.clear();

    if (g_scoreboardGetTeamMethodLegacy && g_scoreboardRemoveTeamMethodLegacy) {
        jstring jHideTeamName = env->NewStringUTF("lc_hide_tags");
        if (jHideTeamName) {
            jobject hideTeam = env->CallObjectMethod(scoreboardObj, g_scoreboardGetTeamMethodLegacy, jHideTeamName);
            if (env->ExceptionCheck()) { env->ExceptionClear(); hideTeam = nullptr; }
            if (hideTeam) {
                env->CallVoidMethod(scoreboardObj, g_scoreboardRemoveTeamMethodLegacy, hideTeam);
                if (env->ExceptionCheck()) env->ExceptionClear();
                env->DeleteLocalRef(hideTeam);
            }
            env->DeleteLocalRef(jHideTeamName);
        }
    }
}

static void TryResolveChestEspMappings(JNIEnv* env) {
    TRACE_PATH("enter");
    TRACE_BRANCH("envAvailable", env != nullptr);
    if (!env) return;

    bool alreadyResolved = (g_tileEntityPosField && g_blockPosGetX && g_blockPosGetY && g_blockPosGetZ);
    TRACE_BRANCH("alreadyResolved", alreadyResolved);
    if (alreadyResolved) return;

    jobject gcl = EnsureGameClassLoader(env);

    if (!g_tileEntityPosField) {
        jclass teClass = nullptr;
        if (gcl) teClass = LoadClassWithLoader(env, gcl, "net.minecraft.tileentity.TileEntity");
        TRACE_BRANCH("tileEntityLoadClassHit", teClass != nullptr);
        if (!teClass) {
            teClass = env->FindClass("net/minecraft/tileentity/TileEntity");
            TRACE_BRANCH("tileEntityFindClassHit", teClass != nullptr);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }
        if (teClass) {
            g_tileEntityPosField = env->GetFieldID(teClass, "pos", "Lnet/minecraft/util/BlockPos;");
            TRACE_BRANCH("tileEntityPosCanonicalHit", g_tileEntityPosField != nullptr);
            if (!g_tileEntityPosField) {
                env->ExceptionClear();
                g_tileEntityPosField = env->GetFieldID(teClass, "field_174879_c", "Lnet/minecraft/util/BlockPos;");
                TRACE_BRANCH("tileEntityPosSrgHit", g_tileEntityPosField != nullptr);
            }
            if (!g_tileEntityPosField) env->ExceptionClear();
        }
    }

    if (!g_blockPosGetX || !g_blockPosGetY || !g_blockPosGetZ) {
        jclass bpClass = nullptr;
        if (gcl) bpClass = LoadClassWithLoader(env, gcl, "net.minecraft.util.BlockPos");
        TRACE_BRANCH("blockPosLoadClassHit", bpClass != nullptr);
        if (!bpClass) {
            bpClass = env->FindClass("net/minecraft/util/BlockPos");
            TRACE_BRANCH("blockPosFindClassHit", bpClass != nullptr);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }
        if (bpClass) {
            if (!g_blockPosGetX) {
                g_blockPosGetX = env->GetMethodID(bpClass, "getX", "()I");
                if (!g_blockPosGetX) {
                    env->ExceptionClear();
                    g_blockPosGetX = env->GetMethodID(bpClass, "func_177958_n", "()I");
                }
                if (!g_blockPosGetX) env->ExceptionClear();
            }
            if (!g_blockPosGetY) {
                g_blockPosGetY = env->GetMethodID(bpClass, "getY", "()I");
                if (!g_blockPosGetY) {
                    env->ExceptionClear();
                    g_blockPosGetY = env->GetMethodID(bpClass, "func_177956_o", "()I");
                }
                if (!g_blockPosGetY) env->ExceptionClear();
            }
            if (!g_blockPosGetZ) {
                g_blockPosGetZ = env->GetMethodID(bpClass, "getZ", "()I");
                if (!g_blockPosGetZ) {
                    env->ExceptionClear();
                    g_blockPosGetZ = env->GetMethodID(bpClass, "func_177952_p", "()I");
                }
                if (!g_blockPosGetZ) env->ExceptionClear();
            }
        }
    }
}

static bool NeedsInWorldCriticalRefresh() {
    bool need = !g_thePlayerField
        || !g_getHealthMethod
        || !g_posXField
        || !g_posYField
        || !g_posZField
        || !g_currentScreenField
        || !g_rotationYawField
        || !g_rotationPitchField
        || !g_getNameMethod
        || (!g_objectMouseOverField && !g_pointedEntityField)
        || !g_typeOfHitField
        || !g_enumNameMethod
        || !g_inventoryField
        || !g_getHeldItemMethod;
    TRACE_BRANCH("needsInWorldCriticalRefresh", need);
    return need;
}

static bool HasLiveWorld(JNIEnv* env) {
    if (!env || !g_mcInstance || !g_theWorldField) return false;
    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    bool hasWorld = (world != nullptr);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        hasWorld = false;
    }
    if (world) env->DeleteLocalRef(world);
    return hasWorld;
}

static bool HasLivePlayer(JNIEnv* env) {
    if (!env || !g_mcInstance || !g_thePlayerField) return false;
    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    bool hasPlayer = (player != nullptr);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        hasPlayer = false;
    }
    if (player) env->DeleteLocalRef(player);
    return hasPlayer;
}

void MaybeRefreshMappings(JNIEnv* env) {
    TRACE_PATH("enter");
    bool prerequisites = TRACE_IF("prerequisitesMet", (env && g_jvm));
    if (!prerequisites) return;
    TryResolveScreenFieldDirect(env);
    TryResolveHoldingBlockMappings(env);
    TryResolvePlayerCoreMappings(env);
    TryResolveChestEspMappings(env);

    static DWORD nextCoreRetryAt = 0;
    static DWORD coreRetryDelayMs = 10000;
    static DWORD nextInWorldWarnAt = 0;
    static bool hadWorld = false;
    static bool hadReadyPlayer = false;

    bool hasWorld = HasLiveWorld(env);
    bool hasReadyPlayer = hasWorld && HasLivePlayer(env);
    TRACE_BRANCH("hasWorld", hasWorld);
    TRACE_BRANCH("hasReadyPlayer", hasReadyPlayer);
    bool worldBecameAvailable = hasWorld && !hadWorld;
    bool playerReadyBecameAvailable = hasReadyPlayer && !hadReadyPlayer;
    hadWorld = hasWorld;
    hadReadyPlayer = hasReadyPlayer;

    if (worldBecameAvailable) {
        Log("World detected; running lightweight mapping recovery.");
    }
    if (playerReadyBecameAvailable) {
        Log("Local player ready; running lightweight mapping recovery.");
    }

    bool needCoreRefresh = NeedsCoreMappingRefresh();
    bool needRenderFallback = hasReadyPlayer && NeedsRenderRecoveryMappings();
    bool needInWorldRecovery = hasReadyPlayer && NeedsInWorldCriticalRefresh();
    TRACE_BRANCH("needCoreRefresh", needCoreRefresh);
    TRACE_BRANCH("needRenderFallback", needRenderFallback);
    TRACE_BRANCH("needInWorldRecovery", needInWorldRecovery);

    if (!needCoreRefresh) {
        if (needInWorldRecovery) {
            DWORD now = GetTickCount();
            if (now >= nextInWorldWarnAt) {
                nextInWorldWarnAt = now + 30000;
                Log("In-world mappings incomplete; using lightweight recovery only.");
            }
        }
        if (needRenderFallback) {
            TryFallbackRenderRecovery(env, hasReadyPlayer);
        }
        return;
    }

    DWORD now = GetTickCount();
    bool coreRetryDue = now >= nextCoreRetryAt;
    TRACE_BRANCH("coreRetryDue", coreRetryDue);
    if (!coreRetryDue) return;

    Log("Core mappings incomplete, retrying heavy discovery...");
    bool ok = RunHeavyDiscovery(env, "core-refresh");
    TryResolveScreenFieldDirect(env);
    TryResolveHoldingBlockMappings(env);
    TryResolvePlayerCoreMappings(env);
    TryResolveChestEspMappings(env);
    TryResolveWorldMappings(env);
    TryResolveRenderMappings(env, false);

    bool stillNeedCore = NeedsCoreMappingRefresh();
    if (stillNeedCore) {
        coreRetryDelayMs = (std::min)(coreRetryDelayMs * 2, (DWORD)120000);
        Log((ok ? "Mapping refresh partial" : "Mapping refresh failed")
            + std::string("; next retry in ")
            + std::to_string(coreRetryDelayMs) + "ms");
    } else {
        coreRetryDelayMs = 10000;
        Log("Mapping refresh complete");
        if (needRenderFallback) {
            TryFallbackRenderRecovery(env, hasReadyPlayer);
        }
    }
    nextCoreRetryAt = now + coreRetryDelayMs;
}

static void EnsureVelocityMappings(JNIEnv* env, jobject playerObj) {
    if (!env || !playerObj) return;
    if (g_motionXField && g_motionYField && g_motionZField && g_hurtTimeField) return;

    jclass playerCls = env->GetObjectClass(playerObj);
    if (!playerCls) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return;
    }

    if (!g_motionXField) {
        g_motionXField = env->GetFieldID(playerCls, "motionX", "D");
        if (!g_motionXField) {
            env->ExceptionClear();
            g_motionXField = env->GetFieldID(playerCls, "field_70159_w", "D");
        }
        if (!g_motionXField) env->ExceptionClear();
    }

    if (!g_motionYField) {
        g_motionYField = env->GetFieldID(playerCls, "motionY", "D");
        if (!g_motionYField) {
            env->ExceptionClear();
            g_motionYField = env->GetFieldID(playerCls, "field_70181_x", "D");
        }
        if (!g_motionYField) env->ExceptionClear();
    }

    if (!g_motionZField) {
        g_motionZField = env->GetFieldID(playerCls, "motionZ", "D");
        if (!g_motionZField) {
            env->ExceptionClear();
            g_motionZField = env->GetFieldID(playerCls, "field_70179_y", "D");
        }
        if (!g_motionZField) env->ExceptionClear();
    }

    if (!g_hurtTimeField) {
        g_hurtTimeField = env->GetFieldID(playerCls, "hurtTime", "I");
        if (!g_hurtTimeField) {
            env->ExceptionClear();
            g_hurtTimeField = env->GetFieldID(playerCls, "field_70737_aN", "I");
        }
        if (!g_hurtTimeField) env->ExceptionClear();
    }

    env->DeleteLocalRef(playerCls);
}

static void UpdateVelocity(JNIEnv* env, const Config& cfg) {
    if (!env || !g_mcInstance || !g_thePlayerField) return;

    jobject selfObj = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        selfObj = nullptr;
    }
    if (!selfObj) {
        g_lastHurtTime = 0;
        return;
    }

    EnsureVelocityMappings(env, selfObj);
    if (!g_motionXField || !g_motionYField || !g_motionZField || !g_hurtTimeField) {
        env->DeleteLocalRef(selfObj);
        return;
    }

    int hurtTime = env->GetIntField(selfObj, g_hurtTimeField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        env->DeleteLocalRef(selfObj);
        return;
    }

    bool newHit = (hurtTime > 0 && hurtTime > g_lastHurtTime);
    if (cfg.velocityEnabled && newHit) {
        int rv = rand() % 100;
        if (rv < cfg.velocityChance) {
            double vx = env->GetDoubleField(selfObj, g_motionXField);
            double vy = env->GetDoubleField(selfObj, g_motionYField);
            double vz = env->GetDoubleField(selfObj, g_motionZField);
            if (!env->ExceptionCheck()) {
                double horizMag = std::sqrt(vx * vx + vz * vz);
                bool looksLikeKnockback = (horizMag > 0.18 || std::fabs(vy) > 0.14);
                if (looksLikeKnockback && (cfg.velocityHorizontal != 100 || cfg.velocityVertical != 100)) {
                    double hScale = (double)cfg.velocityHorizontal / 100.0;
                    double vScale = (double)cfg.velocityVertical / 100.0;
                    env->SetDoubleField(selfObj, g_motionXField, vx * hScale);
                    env->SetDoubleField(selfObj, g_motionYField, vy * vScale);
                    env->SetDoubleField(selfObj, g_motionZField, vz * hScale);
                    if (env->ExceptionCheck()) env->ExceptionClear();
                }
            } else {
                env->ExceptionClear();
            }
        }
    }

    g_lastHurtTime = hurtTime;
    env->DeleteLocalRef(selfObj);
}

// ---- KillAura (legacy 1.8.9: OpenMyau C0A→C02 pre-C03 via Premotion) ----
static float KillAuraRand01() {
    return (float)(rand() % 10001) / 10000.0f;
}
static float KillAuraRandSigned01() {
    return KillAuraRand01() * 2.0f - 1.0f;
}

static void KillAuraResetState() {
    g_killAuraCombatValid = false;
    g_killAuraYawVel = 0.0f;
    g_killAuraPitchVel = 0.0f;
    g_killAuraBodyValid = false;
    g_killAuraOvershootActive = false;
    g_killAuraOvershootYaw = 0.0f;
    g_killAuraOvershootPitch = 0.0f;
    g_killAuraAimFracX = 0.5f;
    g_killAuraAimFracY = 0.55f;
    g_killAuraAimFracZ = 0.5f;
    g_killAuraNextAimPointMs = 0;
    g_killAuraPrevValid = false;
    ka_premotion::SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);
}

static bool KillAuraPacketAttackReady() {
    return g_killAuraSwingMethod && g_killAuraSyncItemMethod && g_killAuraAttackLocalMethod
        && g_killAuraAddToSendQueue && g_killAuraUseEntityClass && g_killAuraUseEntityCtor
        && g_killAuraAttackAction && g_playerControllerField;
}

static void KillAuraTryArmSendQueue(JNIEnv* env, jclass nhCls) {
    if (!env || !nhCls || g_killAuraSendQueueArmed) return;
    if (!g_killAuraC03Class || !g_killAuraC03Yaw || !g_killAuraC03Pitch) return;
    ka_premotion::BindC03LookFields(
        g_killAuraC03Class, g_killAuraC03Yaw, g_killAuraC03Pitch, g_killAuraC03Rotating);
    if (ka_premotion::ArmSendQueueHook(env, nhCls))
        g_killAuraSendQueueArmed = true;
}

static void EnsureKillAuraJniLegacy(JNIEnv* env) {
    if (!env || !g_mcInstance || !g_mcClass) return;
    if (KillAuraPacketAttackReady() && g_rotationYawField && g_rotationPitchField && g_killAuraSensResolved
        && g_killAuraSendQueueArmed) {
        ka_premotion::BindRotationFields(
            g_rotationYawField, g_rotationPitchField,
            g_rotationYawHeadField, g_renderYawOffsetField);
        if (g_mcInstance && g_thePlayerField)
            ka_premotion::BindMcPlayerLookup(g_mcInstance, g_thePlayerField);
        return;
    }

    DWORD now = GetTickCount();
    if (now < g_killAuraNextResolveMs) return;
    g_killAuraNextResolveMs = now + 1000;

    if (!g_playerControllerField) {
        g_playerControllerField = env->GetFieldID(g_mcClass, "playerController",
            "Lnet/minecraft/client/multiplayer/PlayerControllerMP;");
        if (env->ExceptionCheck() || !g_playerControllerField) {
            env->ExceptionClear();
            g_playerControllerField = env->GetFieldID(g_mcClass, "field_71442_b",
                "Lnet/minecraft/client/multiplayer/PlayerControllerMP;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_playerControllerField = nullptr; }
        }
    }

    jobject gcl = EnsureGameClassLoader(env);

    // PlayerControllerMP.syncCurrentPlayItem
    if (!g_killAuraSyncItemMethod && g_playerControllerField) {
        jclass modeCls = nullptr;
        if (gcl) {
            modeCls = LoadClassWithLoader(env, gcl, "net.minecraft.client.multiplayer.PlayerControllerMP");
            if (env->ExceptionCheck()) { env->ExceptionClear(); modeCls = nullptr; }
        }
        if (!modeCls) {
            jobject controller = env->GetObjectField(g_mcInstance, g_playerControllerField);
            if (!env->ExceptionCheck() && controller) {
                modeCls = env->GetObjectClass(controller);
                if (env->ExceptionCheck()) { env->ExceptionClear(); modeCls = nullptr; }
                env->DeleteLocalRef(controller);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
        }
        if (modeCls) {
            g_killAuraSyncItemMethod = env->GetMethodID(modeCls, "syncCurrentPlayItem", "()V");
            if (env->ExceptionCheck() || !g_killAuraSyncItemMethod) {
                env->ExceptionClear();
                g_killAuraSyncItemMethod = env->GetMethodID(modeCls, "func_78765_e", "()V");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraSyncItemMethod = nullptr; }
            }
            env->DeleteLocalRef(modeCls);
            if (g_killAuraSyncItemMethod)
                Log("KillAura: resolved PlayerControllerMP.syncCurrentPlayItem");
        }
    }

    // EntityPlayer.swingItem + attackTargetEntityWithCurrentItem
    if ((!g_killAuraSwingMethod || !g_killAuraAttackLocalMethod) && g_thePlayerField) {
        jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
        if (!env->ExceptionCheck() && player) {
            jclass walk = env->GetObjectClass(player);
            for (int d = 0; walk && d < 8; d++) {
                if (!g_killAuraSwingMethod) {
                    g_killAuraSwingMethod = env->GetMethodID(walk, "swingItem", "()V");
                    if (env->ExceptionCheck() || !g_killAuraSwingMethod) {
                        env->ExceptionClear();
                        g_killAuraSwingMethod = env->GetMethodID(walk, "func_71038_i", "()V");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraSwingMethod = nullptr; }
                    }
                }
                if (!g_killAuraAttackLocalMethod) {
                    g_killAuraAttackLocalMethod = env->GetMethodID(walk, "attackTargetEntityWithCurrentItem",
                        "(Lnet/minecraft/entity/Entity;)V");
                    if (env->ExceptionCheck() || !g_killAuraAttackLocalMethod) {
                        env->ExceptionClear();
                        g_killAuraAttackLocalMethod = env->GetMethodID(walk, "func_71059_n",
                            "(Lnet/minecraft/entity/Entity;)V");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraAttackLocalMethod = nullptr; }
                    }
                }
                jclass superCls = (jclass)env->GetSuperclass(walk);
                env->DeleteLocalRef(walk);
                walk = superCls;
                if (g_killAuraSwingMethod && g_killAuraAttackLocalMethod) break;
            }
            if (walk) env->DeleteLocalRef(walk);
            env->DeleteLocalRef(player);
            if (g_killAuraSwingMethod && g_killAuraAttackLocalMethod)
                Log("KillAura: resolved swingItem + attackTargetEntityWithCurrentItem");
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    // C02PacketUseEntity(Entity, Action.ATTACK)
    if (!g_killAuraUseEntityClass || !g_killAuraUseEntityCtor || !g_killAuraAttackAction) {
        jclass useCls = nullptr;
        if (gcl) {
            useCls = LoadClassWithLoader(env, gcl, "net.minecraft.network.play.client.C02PacketUseEntity");
            if (env->ExceptionCheck()) { env->ExceptionClear(); useCls = nullptr; }
        }
        if (!useCls) {
            useCls = env->FindClass("net/minecraft/network/play/client/C02PacketUseEntity");
            if (env->ExceptionCheck()) { env->ExceptionClear(); useCls = nullptr; }
        }
        jclass actionCls = nullptr;
        if (gcl) {
            actionCls = LoadClassWithLoader(env, gcl, "net.minecraft.network.play.client.C02PacketUseEntity$Action");
            if (env->ExceptionCheck()) { env->ExceptionClear(); actionCls = nullptr; }
        }
        if (!actionCls) {
            actionCls = env->FindClass("net/minecraft/network/play/client/C02PacketUseEntity$Action");
            if (env->ExceptionCheck()) { env->ExceptionClear(); actionCls = nullptr; }
        }
        if (useCls && actionCls) {
            jmethodID ctor = env->GetMethodID(useCls, "<init>",
                "(Lnet/minecraft/entity/Entity;Lnet/minecraft/network/play/client/C02PacketUseEntity$Action;)V");
            if (env->ExceptionCheck() || !ctor) {
                env->ExceptionClear();
                ctor = nullptr;
            }
            jfieldID attackField = env->GetStaticFieldID(actionCls, "ATTACK",
                "Lnet/minecraft/network/play/client/C02PacketUseEntity$Action;");
            if (env->ExceptionCheck() || !attackField) {
                env->ExceptionClear();
                // SRG enum constants vary; try common obfuscated ordinal field names via values().
                attackField = nullptr;
                jmethodID values = env->GetStaticMethodID(actionCls, "values",
                    "()[Lnet/minecraft/network/play/client/C02PacketUseEntity$Action;");
                if (env->ExceptionCheck() || !values) {
                    env->ExceptionClear();
                    values = nullptr;
                }
                if (values) {
                    jobjectArray arr = (jobjectArray)env->CallStaticObjectMethod(actionCls, values);
                    if (!env->ExceptionCheck() && arr) {
                        // Action order in 1.8.9: INTERACT=0, ATTACK=1, INTERACT_AT=2
                        const jsize len = env->GetArrayLength(arr);
                        if (len > 1) {
                            jobject attack = env->GetObjectArrayElement(arr, 1);
                            if (attack) {
                                if (g_killAuraAttackAction) env->DeleteGlobalRef(g_killAuraAttackAction);
                                g_killAuraAttackAction = env->NewGlobalRef(attack);
                                env->DeleteLocalRef(attack);
                            }
                        }
                        env->DeleteLocalRef(arr);
                    } else if (env->ExceptionCheck()) {
                        env->ExceptionClear();
                    }
                }
            } else {
                jobject attack = env->GetStaticObjectField(actionCls, attackField);
                if (!env->ExceptionCheck() && attack) {
                    if (g_killAuraAttackAction) env->DeleteGlobalRef(g_killAuraAttackAction);
                    g_killAuraAttackAction = env->NewGlobalRef(attack);
                    env->DeleteLocalRef(attack);
                } else if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                }
            }
            if (ctor && g_killAuraAttackAction) {
                if (g_killAuraUseEntityClass) env->DeleteGlobalRef(g_killAuraUseEntityClass);
                g_killAuraUseEntityClass = (jclass)env->NewGlobalRef(useCls);
                g_killAuraUseEntityCtor = ctor;
                Log("KillAura: resolved C02PacketUseEntity ATTACK");
            }
        }
        if (useCls) env->DeleteLocalRef(useCls);
        if (actionCls) env->DeleteLocalRef(actionCls);
    }

    if (!g_killAuraAddToSendQueue) {
        jobject netHandler = nullptr;
        if (g_legacyMcGetNetHandlerMethod) {
            netHandler = env->CallObjectMethod(g_mcInstance, g_legacyMcGetNetHandlerMethod);
            if (env->ExceptionCheck()) { env->ExceptionClear(); netHandler = nullptr; }
        }
        if (!netHandler && g_mcClass) {
            jmethodID getNh = env->GetMethodID(g_mcClass, "getNetHandler",
                "()Lnet/minecraft/client/network/NetHandlerPlayClient;");
            if (env->ExceptionCheck() || !getNh) {
                env->ExceptionClear();
                getNh = env->GetMethodID(g_mcClass, "func_147114_u",
                    "()Lnet/minecraft/client/network/NetHandlerPlayClient;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); getNh = nullptr; }
            }
            if (getNh) {
                g_legacyMcGetNetHandlerMethod = getNh;
                netHandler = env->CallObjectMethod(g_mcInstance, getNh);
                if (env->ExceptionCheck()) { env->ExceptionClear(); netHandler = nullptr; }
            }
        }
        if (netHandler) {
            jclass nhCls = env->GetObjectClass(netHandler);
            if (nhCls) {
                g_killAuraAddToSendQueue = env->GetMethodID(nhCls, "addToSendQueue",
                    "(Lnet/minecraft/network/Packet;)V");
                if (env->ExceptionCheck() || !g_killAuraAddToSendQueue) {
                    env->ExceptionClear();
                    g_killAuraAddToSendQueue = env->GetMethodID(nhCls, "func_147297_a",
                        "(Lnet/minecraft/network/Packet;)V");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraAddToSendQueue = nullptr; }
                }
                if (g_killAuraAddToSendQueue)
                    Log("KillAura: resolved NetHandlerPlayClient.addToSendQueue");
                KillAuraTryArmSendQueue(env, nhCls);
                env->DeleteLocalRef(nhCls);
            }
            env->DeleteLocalRef(netHandler);
        }
    }

    // C03PacketPlayer yaw/pitch for Silent look patch in send-queue Premotion.
    if (!g_killAuraC03Class || !g_killAuraC03Yaw || !g_killAuraC03Pitch) {
        jobject gclC03 = EnsureGameClassLoader(env);
        jclass c03 = nullptr;
        if (gclC03) {
            c03 = LoadClassWithLoader(env, gclC03, "net.minecraft.network.play.client.C03PacketPlayer");
            if (env->ExceptionCheck()) { env->ExceptionClear(); c03 = nullptr; }
        }
        if (!c03) {
            c03 = env->FindClass("net/minecraft/network/play/client/C03PacketPlayer");
            if (env->ExceptionCheck()) { env->ExceptionClear(); c03 = nullptr; }
        }
        if (c03) {
            if (!g_killAuraC03Yaw) {
                g_killAuraC03Yaw = env->GetFieldID(c03, "yaw", "F");
                if (env->ExceptionCheck() || !g_killAuraC03Yaw) {
                    env->ExceptionClear();
                    g_killAuraC03Yaw = env->GetFieldID(c03, "field_149476_e", "F");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraC03Yaw = nullptr; }
                }
            }
            if (!g_killAuraC03Pitch) {
                g_killAuraC03Pitch = env->GetFieldID(c03, "pitch", "F");
                if (env->ExceptionCheck() || !g_killAuraC03Pitch) {
                    env->ExceptionClear();
                    g_killAuraC03Pitch = env->GetFieldID(c03, "field_149473_f", "F");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraC03Pitch = nullptr; }
                }
            }
            if (!g_killAuraC03Rotating) {
                g_killAuraC03Rotating = env->GetFieldID(c03, "rotating", "Z");
                if (env->ExceptionCheck() || !g_killAuraC03Rotating) {
                    env->ExceptionClear();
                    g_killAuraC03Rotating = env->GetFieldID(c03, "field_149481_i", "Z");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraC03Rotating = nullptr; }
                }
            }
            if (g_killAuraC03Yaw && g_killAuraC03Pitch) {
                if (!g_killAuraC03Class)
                    g_killAuraC03Class = (jclass)env->NewGlobalRef(c03);
                Log("KillAura: resolved C03PacketPlayer look fields");
            }
            env->DeleteLocalRef(c03);
        }
    }

    // Retry arm if addToSendQueue was resolved earlier but C03/retransform lagged.
    if (!g_killAuraSendQueueArmed && g_killAuraAddToSendQueue && g_killAuraC03Class) {
        jobject netHandlerArm = nullptr;
        if (g_legacyMcGetNetHandlerMethod) {
            netHandlerArm = env->CallObjectMethod(g_mcInstance, g_legacyMcGetNetHandlerMethod);
            if (env->ExceptionCheck()) { env->ExceptionClear(); netHandlerArm = nullptr; }
        }
        if (netHandlerArm) {
            jclass nhClsArm = env->GetObjectClass(netHandlerArm);
            if (nhClsArm) {
                KillAuraTryArmSendQueue(env, nhClsArm);
                env->DeleteLocalRef(nhClsArm);
            }
            env->DeleteLocalRef(netHandlerArm);
        }
    }

    if ((!g_rotationYawHeadField || !g_renderYawOffsetField || !g_onGroundField) && g_thePlayerField) {
        jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
        if (!env->ExceptionCheck() && player) {
            jclass walk = env->GetObjectClass(player);
            for (int d = 0; walk && d < 8; d++) {
                if (!g_rotationYawHeadField) {
                    g_rotationYawHeadField = env->GetFieldID(walk, "rotationYawHead", "F");
                    if (env->ExceptionCheck() || !g_rotationYawHeadField) {
                        env->ExceptionClear();
                        g_rotationYawHeadField = env->GetFieldID(walk, "field_70759_as", "F");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_rotationYawHeadField = nullptr; }
                    }
                }
                if (!g_renderYawOffsetField) {
                    g_renderYawOffsetField = env->GetFieldID(walk, "renderYawOffset", "F");
                    if (env->ExceptionCheck() || !g_renderYawOffsetField) {
                        env->ExceptionClear();
                        g_renderYawOffsetField = env->GetFieldID(walk, "field_70761_aq", "F");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_renderYawOffsetField = nullptr; }
                    }
                }
                if (!g_onGroundField) {
                    g_onGroundField = env->GetFieldID(walk, "onGround", "Z");
                    if (env->ExceptionCheck() || !g_onGroundField) {
                        env->ExceptionClear();
                        g_onGroundField = env->GetFieldID(walk, "field_70122_E", "Z");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_onGroundField = nullptr; }
                    }
                }
                jclass superCls = (jclass)env->GetSuperclass(walk);
                env->DeleteLocalRef(walk);
                walk = superCls;
                if (g_rotationYawHeadField && g_renderYawOffsetField && g_onGroundField) break;
            }
            if (walk) env->DeleteLocalRef(walk);
            env->DeleteLocalRef(player);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    if (!g_killAuraSensResolved && g_gameSettingsField) {
        g_killAuraSensResolved = true;
        jobject gs = env->GetObjectField(g_mcInstance, g_gameSettingsField);
        if (!env->ExceptionCheck() && gs) {
            jclass gsCls = env->GetObjectClass(gs);
            if (gsCls) {
                if (!g_mouseSensitivityField) {
                    g_mouseSensitivityField = env->GetFieldID(gsCls, "mouseSensitivity", "F");
                    if (env->ExceptionCheck() || !g_mouseSensitivityField) {
                        env->ExceptionClear();
                        g_mouseSensitivityField = env->GetFieldID(gsCls, "field_74341_c", "F");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_mouseSensitivityField = nullptr; }
                    }
                }
                env->DeleteLocalRef(gsCls);
            }
            if (g_mouseSensitivityField) {
                float sens = env->GetFloatField(gs, g_mouseSensitivityField);
                if (!env->ExceptionCheck() && std::isfinite(sens) && sens > 0.0f && sens <= 1.0f)
                    g_killAuraMouseSens = sens;
                else
                    env->ExceptionClear();
            }
            env->DeleteLocalRef(gs);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    ka_premotion::BindRotationFields(
        g_rotationYawField, g_rotationPitchField,
        g_rotationYawHeadField, g_renderYawOffsetField);
    if (g_mcInstance && g_thePlayerField)
        ka_premotion::BindMcPlayerLookup(g_mcInstance, g_thePlayerField);
}

static void SetKillAuraPlayerLookLegacy(JNIEnv* env, jobject player, float yaw, float pitch, float bodyYaw) {
    if (!env || !player) return;
    float writeYaw = saaim::NormalizeAngle(yaw);
    float writePitch = saaim::Clamp(pitch, -90.0f, 90.0f);
    float writeBody = saaim::NormalizeAngle(bodyYaw);
    if (g_rotationYawField) {
        env->SetFloatField(player, g_rotationYawField, writeYaw);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (g_rotationPitchField) {
        env->SetFloatField(player, g_rotationPitchField, writePitch);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (g_rotationYawHeadField) {
        env->SetFloatField(player, g_rotationYawHeadField, writeYaw);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
    if (g_renderYawOffsetField) {
        env->SetFloatField(player, g_renderYawOffsetField, writeBody);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
}

static void EnsureKillAuraAutoBlockLegacy(JNIEnv* env, jobject player) {
    if (!env || !player) return;
    jobject gcl = g_gameClassLoader;
    if (!gcl) gcl = EnsureGameClassLoader(env);
    if (!g_killAuraC08Class) {
        jclass cls = gcl ? LoadClassWithLoader(env, gcl, "net.minecraft.network.play.client.C08PacketPlayerBlockPlacement") : nullptr;
        if (cls) {
            g_killAuraC08Ctor = env->GetMethodID(cls, "<init>", "(Lnet/minecraft/item/ItemStack;)V");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraC08Ctor = nullptr; }
            if (g_killAuraC08Ctor) g_killAuraC08Class = (jclass)env->NewGlobalRef(cls);
            env->DeleteLocalRef(cls);
        }
    }
    if (!g_killAuraC09Class) {
        jclass cls = gcl ? LoadClassWithLoader(env, gcl, "net.minecraft.network.play.client.C09PacketHeldItemChange") : nullptr;
        if (cls) {
            g_killAuraC09Ctor = env->GetMethodID(cls, "<init>", "(I)V");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraC09Ctor = nullptr; }
            if (g_killAuraC09Ctor) g_killAuraC09Class = (jclass)env->NewGlobalRef(cls);
            env->DeleteLocalRef(cls);
        }
    }
    if (!g_killAuraC07Class) {
        jclass packetCls = gcl ? LoadClassWithLoader(env, gcl, "net.minecraft.network.play.client.C07PacketPlayerDigging") : nullptr;
        jclass actionCls = gcl ? LoadClassWithLoader(env, gcl, "net.minecraft.network.play.client.C07PacketPlayerDigging$Action") : nullptr;
        jclass posCls = gcl ? LoadClassWithLoader(env, gcl, "net.minecraft.util.BlockPos") : nullptr;
        jclass facingCls = gcl ? LoadClassWithLoader(env, gcl, "net.minecraft.util.EnumFacing") : nullptr;
        if (packetCls && actionCls && posCls && facingCls) {
            g_killAuraC07Ctor = env->GetMethodID(packetCls, "<init>",
                "(Lnet/minecraft/network/play/client/C07PacketPlayerDigging$Action;Lnet/minecraft/util/BlockPos;Lnet/minecraft/util/EnumFacing;)V");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraC07Ctor = nullptr; }
            jfieldID release = env->GetStaticFieldID(actionCls, "RELEASE_USE_ITEM",
                "Lnet/minecraft/network/play/client/C07PacketPlayerDigging$Action;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); release = nullptr; }
            const char* originNames[] = { "ORIGIN", "field_177992_a", nullptr };
            jfieldID origin = nullptr;
            for (int i = 0; originNames[i] && !origin; ++i) {
                origin = env->GetStaticFieldID(posCls, originNames[i], "Lnet/minecraft/util/BlockPos;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); origin = nullptr; }
            }
            jfieldID down = env->GetStaticFieldID(facingCls, "DOWN", "Lnet/minecraft/util/EnumFacing;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); down = nullptr; }
            jobject releaseObj = release ? env->GetStaticObjectField(actionCls, release) : nullptr;
            jobject originObj = origin ? env->GetStaticObjectField(posCls, origin) : nullptr;
            jobject downObj = down ? env->GetStaticObjectField(facingCls, down) : nullptr;
            if (g_killAuraC07Ctor && releaseObj && originObj && downObj) {
                g_killAuraC07Class = (jclass)env->NewGlobalRef(packetCls);
                g_killAuraReleaseAction = env->NewGlobalRef(releaseObj);
                g_killAuraOriginPos = env->NewGlobalRef(originObj);
                g_killAuraFacingDown = env->NewGlobalRef(downObj);
            }
            if (releaseObj) env->DeleteLocalRef(releaseObj);
            if (originObj) env->DeleteLocalRef(originObj);
            if (downObj) env->DeleteLocalRef(downObj);
        }
        if (packetCls) env->DeleteLocalRef(packetCls);
        if (actionCls) env->DeleteLocalRef(actionCls);
        if (posCls) env->DeleteLocalRef(posCls);
        if (facingCls) env->DeleteLocalRef(facingCls);
    }
    jclass playerCls = env->GetObjectClass(player);
    if (playerCls) {
        if (!g_killAuraSetItemInUse) {
            const char* names[] = { "setItemInUse", "func_71008_a", nullptr };
            for (int i = 0; names[i] && !g_killAuraSetItemInUse; ++i) {
                g_killAuraSetItemInUse = env->GetMethodID(playerCls, names[i], "(Lnet/minecraft/item/ItemStack;I)V");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraSetItemInUse = nullptr; }
            }
        }
        if (!g_killAuraStopUsingItem) {
            const char* names[] = { "stopUsingItem", "func_71034_by", nullptr };
            for (int i = 0; names[i] && !g_killAuraStopUsingItem; ++i) {
                g_killAuraStopUsingItem = env->GetMethodID(playerCls, names[i], "()V");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraStopUsingItem = nullptr; }
            }
        }
        if (!g_killAuraIsUsingItem) {
            const char* names[] = { "isUsingItem", "func_71039_bw", nullptr };
            for (int i = 0; names[i] && !g_killAuraIsUsingItem; ++i) {
                g_killAuraIsUsingItem = env->GetMethodID(playerCls, names[i], "()Z");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraIsUsingItem = nullptr; }
            }
        }
        env->DeleteLocalRef(playerCls);
    }
    if (g_inventoryField && (!g_killAuraCurrentItemField || !g_killAuraGetStackInSlot)) {
        jobject inventory = env->GetObjectField(player, g_inventoryField);
        if (!env->ExceptionCheck() && inventory) {
            jclass invCls = env->GetObjectClass(inventory);
            if (invCls && !g_killAuraCurrentItemField) {
                const char* names[] = { "currentItem", "field_70461_c", nullptr };
                for (int i = 0; names[i] && !g_killAuraCurrentItemField; ++i) {
                    g_killAuraCurrentItemField = env->GetFieldID(invCls, names[i], "I");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraCurrentItemField = nullptr; }
                }
            }
            if (invCls && !g_killAuraGetStackInSlot) {
                const char* names[] = { "getStackInSlot", "func_70301_a", nullptr };
                for (int i = 0; names[i] && !g_killAuraGetStackInSlot; ++i) {
                    g_killAuraGetStackInSlot = env->GetMethodID(invCls, names[i], "(I)Lnet/minecraft/item/ItemStack;");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraGetStackInSlot = nullptr; }
                }
            }
            if (invCls) env->DeleteLocalRef(invCls);
            env->DeleteLocalRef(inventory);
        } else env->ExceptionClear();
    }
}

static bool KillAuraSendPacketLegacy(JNIEnv* env, jobject packet) {
    if (!env || !packet || !g_killAuraAddToSendQueue || !g_legacyMcGetNetHandlerMethod) return false;
    jobject netHandler = env->CallObjectMethod(g_mcInstance, g_legacyMcGetNetHandlerMethod);
    if (env->ExceptionCheck() || !netHandler) { env->ExceptionClear(); return false; }
    env->CallVoidMethod(netHandler, g_killAuraAddToSendQueue, packet);
    bool ok = !env->ExceptionCheck();
    if (!ok) env->ExceptionClear();
    env->DeleteLocalRef(netHandler);
    return ok;
}

static jobject KillAuraHeldStackLegacy(JNIEnv* env, jobject player) {
    if (!env || !player) return nullptr;
    if (g_getHeldItemMethod) {
        jobject stack = env->CallObjectMethod(player, g_getHeldItemMethod);
        if (!env->ExceptionCheck()) return stack;
        env->ExceptionClear();
    }
    if (g_inventoryField && g_getCurrentItemMethod) {
        jobject inventory = env->GetObjectField(player, g_inventoryField);
        if (!env->ExceptionCheck() && inventory) {
            jobject stack = env->CallObjectMethod(inventory, g_getCurrentItemMethod);
            if (env->ExceptionCheck()) { env->ExceptionClear(); stack = nullptr; }
            env->DeleteLocalRef(inventory);
            return stack;
        }
        env->ExceptionClear();
    }
    return nullptr;
}

static bool KillAuraStartBlockLegacy(JNIEnv* env, jobject player) {
    jobject stack = KillAuraHeldStackLegacy(env, player);
    if (!stack || !g_killAuraC08Class || !g_killAuraC08Ctor || !g_killAuraSetItemInUse) {
        if (stack) env->DeleteLocalRef(stack);
        return false;
    }
    jobject controller = env->GetObjectField(g_mcInstance, g_playerControllerField);
    if (!env->ExceptionCheck() && controller) {
        env->CallVoidMethod(controller, g_killAuraSyncItemMethod);
        if (env->ExceptionCheck()) env->ExceptionClear();
        env->DeleteLocalRef(controller);
    } else env->ExceptionClear();
    jobject packet = env->NewObject(g_killAuraC08Class, g_killAuraC08Ctor, stack);
    bool ok = packet && !env->ExceptionCheck() && KillAuraSendPacketLegacy(env, packet);
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (packet) env->DeleteLocalRef(packet);
    if (ok) {
        env->CallVoidMethod(player, g_killAuraSetItemInUse, stack, (jint)72000);
        if (env->ExceptionCheck()) { env->ExceptionClear(); ok = false; }
    }
    env->DeleteLocalRef(stack);
    return ok;
}

static bool KillAuraStopBlockLegacy(JNIEnv* env, jobject player) {
    if (!g_killAuraC07Class || !g_killAuraC07Ctor || !g_killAuraReleaseAction ||
        !g_killAuraOriginPos || !g_killAuraFacingDown || !g_killAuraStopUsingItem) return false;
    jobject packet = env->NewObject(g_killAuraC07Class, g_killAuraC07Ctor,
        g_killAuraReleaseAction, g_killAuraOriginPos, g_killAuraFacingDown);
    bool ok = packet && !env->ExceptionCheck() && KillAuraSendPacketLegacy(env, packet);
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (packet) env->DeleteLocalRef(packet);
    if (ok) {
        env->CallVoidMethod(player, g_killAuraStopUsingItem);
        if (env->ExceptionCheck()) { env->ExceptionClear(); ok = false; }
    }
    return ok;
}

static bool KillAuraSendSlotLegacy(JNIEnv* env, int slot) {
    if (!g_killAuraC09Class || !g_killAuraC09Ctor) return false;
    jobject packet = env->NewObject(g_killAuraC09Class, g_killAuraC09Ctor, (jint)slot);
    bool ok = packet && !env->ExceptionCheck() && KillAuraSendPacketLegacy(env, packet);
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (packet) env->DeleteLocalRef(packet);
    return ok;
}

static int KillAuraCurrentSlotLegacy(JNIEnv* env, jobject player) {
    if (!g_inventoryField || !g_killAuraCurrentItemField) return -1;
    jobject inv = env->GetObjectField(player, g_inventoryField);
    if (env->ExceptionCheck() || !inv) { env->ExceptionClear(); return -1; }
    int slot = env->GetIntField(inv, g_killAuraCurrentItemField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); slot = -1; }
    env->DeleteLocalRef(inv);
    return slot;
}

static int KillAuraFindSlotLegacy(JNIEnv* env, jobject player, int current, bool swordOnly) {
    if (!g_inventoryField || !g_killAuraGetStackInSlot) return -1;
    jobject inv = env->GetObjectField(player, g_inventoryField);
    if (env->ExceptionCheck() || !inv) { env->ExceptionClear(); return -1; }
    int fallback = -1;
    for (int slot = 0; slot < 9; ++slot) {
        if (slot == current) continue;
        jobject stack = env->CallObjectMethod(inv, g_killAuraGetStackInSlot, (jint)slot);
        if (env->ExceptionCheck()) { env->ExceptionClear(); stack = nullptr; }
        if (!stack && !swordOnly) { env->DeleteLocalRef(inv); return slot; }
        if (stack && swordOnly) {
            jclass stackCls = env->GetObjectClass(stack);
            jmethodID getItem = stackCls ? env->GetMethodID(stackCls, "getItem", "()Lnet/minecraft/item/Item;") : nullptr;
            if (env->ExceptionCheck() || !getItem) {
                env->ExceptionClear();
                getItem = stackCls ? env->GetMethodID(stackCls, "func_77973_b", "()Lnet/minecraft/item/Item;") : nullptr;
                if (env->ExceptionCheck()) { env->ExceptionClear(); getItem = nullptr; }
            }
            jobject item = getItem ? env->CallObjectMethod(stack, getItem) : nullptr;
            bool sword = item && g_itemSwordClass && env->IsInstanceOf(item, g_itemSwordClass) == JNI_TRUE;
            if (item) env->DeleteLocalRef(item);
            if (stackCls) env->DeleteLocalRef(stackCls);
            if (sword) { env->DeleteLocalRef(stack); env->DeleteLocalRef(inv); return slot; }
        } else if (stack && fallback < 0) fallback = slot;
        if (stack) env->DeleteLocalRef(stack);
    }
    env->DeleteLocalRef(inv);
    return swordOnly ? -1 : (fallback >= 0 ? fallback : ((current + 8) % 9));
}

static void KillAuraSendClickLegacy() {
    INPUT in[2] = {};
    in[0].type = INPUT_MOUSE;
    in[0].mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
    in[1].type = INPUT_MOUSE;
    in[1].mi.dwFlags = MOUSEEVENTF_LEFTUP;
    SendInput(2, in, sizeof(INPUT));
}

// OpenMyau performAttack packet order: C0A → sync → C02 → local hit (no attackEntity).
static bool KillAuraPerformAttackLegacy(JNIEnv* env, jobject selfObj, jobject targetObj) {
    if (!env || !selfObj || !targetObj || !g_mcInstance) return false;
    if (!KillAuraPacketAttackReady()) return false;

    env->CallVoidMethod(selfObj, g_killAuraSwingMethod);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        return false;
    }

    jobject controller = env->GetObjectField(g_mcInstance, g_playerControllerField);
    if (env->ExceptionCheck() || !controller) {
        env->ExceptionClear();
        return false;
    }
    env->CallVoidMethod(controller, g_killAuraSyncItemMethod);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        env->DeleteLocalRef(controller);
        return false;
    }
    env->DeleteLocalRef(controller);

    jobject packet = env->NewObject(
        g_killAuraUseEntityClass, g_killAuraUseEntityCtor, targetObj, g_killAuraAttackAction);
    if (env->ExceptionCheck() || !packet) {
        env->ExceptionClear();
        return false;
    }

    jobject netHandler = nullptr;
    if (g_legacyMcGetNetHandlerMethod) {
        netHandler = env->CallObjectMethod(g_mcInstance, g_legacyMcGetNetHandlerMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); netHandler = nullptr; }
    }
    if (!netHandler) {
        env->DeleteLocalRef(packet);
        return false;
    }

    env->CallVoidMethod(netHandler, g_killAuraAddToSendQueue, packet);
    bool ok = !env->ExceptionCheck();
    if (!ok) env->ExceptionClear();
    env->DeleteLocalRef(netHandler);
    env->DeleteLocalRef(packet);
    if (!ok) return false;

    env->CallVoidMethod(selfObj, g_killAuraAttackLocalMethod, targetObj);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        // Network attack already sent; local hit failure is non-fatal.
    }
    return true;
}

static bool KillAuraPremotionAttackHandler(JNIEnv* env, jobject selfPlayer, jobject target) {
    return KillAuraPerformAttackLegacy(env, selfPlayer, target);
}

static bool KillAuraHoldingWeaponLegacy(JNIEnv* env, jobject player) {
    if (!env || !player) return false;

    jobject stack = nullptr;
    if (g_inventoryField && g_getCurrentItemMethod) {
        jobject inv = env->GetObjectField(player, g_inventoryField);
        if (!env->ExceptionCheck() && inv) {
            stack = env->CallObjectMethod(inv, g_getCurrentItemMethod);
            if (env->ExceptionCheck()) { env->ExceptionClear(); stack = nullptr; }
            env->DeleteLocalRef(inv);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }
    if (!stack && g_getHeldItemMethod) {
        stack = env->CallObjectMethod(player, g_getHeldItemMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); stack = nullptr; }
    }
    if (!stack) return false;

    bool ok = false;
    jclass stCls = env->GetObjectClass(stack);
    jmethodID getItem = nullptr;
    if (stCls) {
        getItem = env->GetMethodID(stCls, "getItem", "()Lnet/minecraft/item/Item;");
        if (env->ExceptionCheck() || !getItem) {
            env->ExceptionClear();
            getItem = env->GetMethodID(stCls, "func_77973_b", "()Lnet/minecraft/item/Item;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); getItem = nullptr; }
        }
        env->DeleteLocalRef(stCls);
    }
    if (getItem) {
        jobject item = env->CallObjectMethod(stack, getItem);
        if (!env->ExceptionCheck() && item) {
            if (g_itemSwordClass && env->IsInstanceOf(item, g_itemSwordClass) == JNI_TRUE) {
                ok = true;
            } else {
                jclass itemCls = env->GetObjectClass(item);
                std::string cn = itemCls ? GetClassNameFromClass(env, itemCls) : "";
                if (itemCls) env->DeleteLocalRef(itemCls);
                for (size_t i = 0; i < cn.size(); i++)
                    cn[i] = (char)std::tolower((unsigned char)cn[i]);
                ok = cn.find("sword") != std::string::npos || cn.find("axe") != std::string::npos;
            }
            env->DeleteLocalRef(item);
        } else {
            env->ExceptionClear();
        }
    }
    env->DeleteLocalRef(stack);
    return ok;
}

static void SetKillAuraUnavailableReasonLegacy(const std::string& reason) {
    LockGuard lk(g_killAuraUnavailableMutex);
    if (g_killAuraUnavailableReason != reason) {
        g_killAuraUnavailableReason = reason;
        if (!reason.empty()) Log("KillAura unavailable: " + reason);
    }
    if (!reason.empty()) { g_killAuraHasTarget = false; g_killAuraBlocking = false; }
}

static bool KillAuraReadKeyLegacy(JNIEnv* env, bool useKey, bool* mapped) {
    if (mapped) *mapped = false;
    if (!env || !g_mcInstance || !g_gameSettingsField) return false;
    jobject settings = env->GetObjectField(g_mcInstance, g_gameSettingsField);
    if (env->ExceptionCheck() || !settings) { env->ExceptionClear(); return false; }
    jclass settingsCls = env->GetObjectClass(settings);
    if (settingsCls && !(useKey ? g_killAuraUseKeyField : g_killAuraAttackKeyField)) {
        const char* names[] = {
            useKey ? "keyBindUseItem" : "keyBindAttack",
            useKey ? "field_74313_G" : "field_74312_F",
            nullptr
        };
        for (int i = 0; names[i]; ++i) {
            jfieldID field = env->GetFieldID(settingsCls, names[i], "Lnet/minecraft/client/settings/KeyBinding;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); field = nullptr; }
            if (field) {
                if (useKey) g_killAuraUseKeyField = field; else g_killAuraAttackKeyField = field;
                break;
            }
        }
    }
    jfieldID field = useKey ? g_killAuraUseKeyField : g_killAuraAttackKeyField;
    jobject key = field ? env->GetObjectField(settings, field) : nullptr;
    if (env->ExceptionCheck()) { env->ExceptionClear(); key = nullptr; }
    if (settingsCls) env->DeleteLocalRef(settingsCls);
    env->DeleteLocalRef(settings);
    if (!key) return false;
    jclass keyCls = env->GetObjectClass(key);
    if (keyCls && !g_killAuraKeyIsDown) {
        const char* names[] = { "isKeyDown", "func_151470_d", nullptr };
        for (int i = 0; names[i] && !g_killAuraKeyIsDown; ++i) {
            g_killAuraKeyIsDown = env->GetMethodID(keyCls, names[i], "()Z");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraKeyIsDown = nullptr; }
        }
    }
    bool down = false;
    if (g_killAuraKeyIsDown) {
        down = env->CallBooleanMethod(key, g_killAuraKeyIsDown) == JNI_TRUE;
        if (env->ExceptionCheck()) { env->ExceptionClear(); down = false; }
        else if (mapped) *mapped = true;
    }
    if (keyCls) env->DeleteLocalRef(keyCls);
    env->DeleteLocalRef(key);
    return down;
}

static bool KillAuraLookingAtBlockLegacy(JNIEnv* env) {
    if (!env || !g_mcInstance || !g_objectMouseOverField || !g_typeOfHitField || !g_enumNameMethod) return false;
    jobject hit = env->GetObjectField(g_mcInstance, g_objectMouseOverField);
    if (env->ExceptionCheck() || !hit) { env->ExceptionClear(); return false; }
    jobject type = env->GetObjectField(hit, g_typeOfHitField);
    if (env->ExceptionCheck() || !type) { env->ExceptionClear(); type = nullptr; }
    bool block = false;
    if (type) {
        jstring name = (jstring)env->CallObjectMethod(type, g_enumNameMethod);
        if (!env->ExceptionCheck() && name) {
            const char* chars = env->GetStringUTFChars(name, nullptr);
            block = chars && std::strcmp(chars, "BLOCK") == 0;
            if (chars) env->ReleaseStringUTFChars(name, chars);
            env->DeleteLocalRef(name);
        } else env->ExceptionClear();
        env->DeleteLocalRef(type);
    }
    env->DeleteLocalRef(hit);
    return block;
}

static void ResolveKillAuraCandidateFiltersLegacy(JNIEnv* env, jobject selfObj) {
    if (!env || !selfObj || (g_killAuraCanSeeEntity && g_killAuraIsSameTeam)) return;
    jclass walk = env->GetObjectClass(selfObj);
    for (int depth = 0; walk && depth < 10; ++depth) {
        if (!g_killAuraCanSeeEntity) {
            g_killAuraCanSeeEntity = env->GetMethodID(walk, "canEntityBeSeen",
                "(Lnet/minecraft/entity/Entity;)Z");
            if (env->ExceptionCheck() || !g_killAuraCanSeeEntity) {
                env->ExceptionClear();
                g_killAuraCanSeeEntity = env->GetMethodID(walk, "func_70685_l",
                    "(Lnet/minecraft/entity/Entity;)Z");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraCanSeeEntity = nullptr; }
            }
        }
        if (!g_killAuraIsSameTeam) {
            g_killAuraIsSameTeam = env->GetMethodID(walk, "isOnSameTeam",
                "(Lnet/minecraft/entity/EntityLivingBase;)Z");
            if (env->ExceptionCheck() || !g_killAuraIsSameTeam) {
                env->ExceptionClear();
                g_killAuraIsSameTeam = env->GetMethodID(walk, "func_142014_c",
                    "(Lnet/minecraft/entity/EntityLivingBase;)Z");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_killAuraIsSameTeam = nullptr; }
            }
        }
        jclass parent = (jclass)env->GetSuperclass(walk);
        env->DeleteLocalRef(walk);
        walk = parent;
    }
    if (walk) env->DeleteLocalRef(walk);
}

static void UpdateKillAuraLegacy(JNIEnv* env, const Config& cfg) {
    if (!cfg.killAura) {
        g_killAuraLastScanMs = 0;
        g_killAuraLastAttackMs = 0;
        g_killAuraCurrentTarget.clear();
        g_killAuraLastSwitchMs = 0;
        g_killAuraPatternIndex = 0;
        g_killAuraNextIntervalMs = 0;
        g_killAuraAttackDelayMs = 0;
        SetKillAuraUnavailableReasonLegacy("");
        { LockGuard lk(g_killAuraUnavailableMutex); g_killAuraHasTarget = false; g_killAuraBlocking = false; }
        KillAuraResetState();
        ka_premotion::ClearPendingAttack(env);
        return;
    }
    if (!env || !g_mcInstance) return;

    DWORD now = GetTickCount();
    if (now - g_killAuraLastScanMs < 50) return;
    g_killAuraLastScanMs = now;
    if (g_killAuraAttackDelayMs > 0) g_killAuraAttackDelayMs -= 50;

    static std::string s_lastKillAuraConfigSummary;
    std::ostringstream kaSummary;
    kaSummary << "rot=" << cfg.killAuraRotMode
              << " pre=" << (int)ka_premotion::GetBackend()
              << " block=" << cfg.killAuraAutoBlock
              << " players=" << cfg.killAuraPlayers
              << " mobs=" << cfg.killAuraMobs
              << " animals=" << cfg.killAuraAnimals
              << " bosses=" << cfg.killAuraBosses
              << " require=" << cfg.killAuraRequirePress
              << " useRequire=" << cfg.killAuraAutoBlockRequirePress
              << " allowMining=" << cfg.killAuraAllowMining;
    if (kaSummary.str() != s_lastKillAuraConfigSummary) {
        s_lastKillAuraConfigSummary = kaSummary.str();
        Log("KillAura config: " + s_lastKillAuraConfigSummary);
    }

    if (!cfg.killAuraPlayers) {
        SetKillAuraUnavailableReasonLegacy("Player targeting is disabled; selected non-player categories are not mapped on this legacy runtime.");
        return;
    }
    if (cfg.killAuraWeaponsOnly && cfg.killAuraAllowTools) {
        SetKillAuraUnavailableReasonLegacy("Allow-tools class mappings are not validated on this legacy runtime.");
        return;
    }
    std::string premotionReasonLegacy;
    if ((cfg.killAuraRotMode == killaura::ROT_SILENT ||
         cfg.killAuraRotMode == killaura::ROT_LIQUID_BOUNCE ||
         cfg.killAuraRotMode == killaura::ROT_HYPIXEL) &&
        !ka_premotion::IsOperational()) {
        premotionReasonLegacy = ka_premotion::GetBackend() == ka_premotion::HOOK_PACKET_RETRANSFORM
            ? "PRE packet hook is armed but no movement callback heartbeat was observed."
            : "Selected server-side rotation requires PRE motion hooks unavailable on this JVM; None, Legit, and LockView remain available.";
    }
    if (!premotionReasonLegacy.empty()) {
        SetKillAuraUnavailableReasonLegacy(premotionReasonLegacy);
        return;
    }

    bool attackKeyMapped = false, useKeyMapped = false;
    const bool attackHeld = KillAuraReadKeyLegacy(env, false, &attackKeyMapped);
    const bool useHeld = KillAuraReadKeyLegacy(env, true, &useKeyMapped);
    if (cfg.killAuraRequirePress && !attackKeyMapped) {
        SetKillAuraUnavailableReasonLegacy("Missing legacy attack KeyBinding.isKeyDown mapping.");
        return;
    }
    if (cfg.killAuraAutoBlockRequirePress && !useKeyMapped) {
        SetKillAuraUnavailableReasonLegacy("Missing legacy use-item KeyBinding.isKeyDown mapping.");
        return;
    }
    if ((cfg.killAuraRequirePress && !attackHeld) ||
        (cfg.killAuraAutoBlockRequirePress && !useHeld) ||
        (!cfg.killAuraAllowMining && attackHeld && KillAuraLookingAtBlockLegacy(env))) {
        ka_premotion::ClearPendingAttack(env);
        return;
    }

    if (cfg.killAuraInventoryCheck && g_currentScreenField) {
        jobject screen = env->GetObjectField(g_mcInstance, g_currentScreenField);
        if (env->ExceptionCheck()) { env->ExceptionClear(); screen = nullptr; }
        if (screen) {
            env->DeleteLocalRef(screen);
            ka_premotion::SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);
            ka_premotion::ClearPendingAttack(env);
            return;
        }
    }

    EnsureKillAuraJniLegacy(env);
    TryResolvePlayerCoreMappings(env);
    if (now >= g_killAuraNextPremotionRefreshMs) {
        g_killAuraNextPremotionRefreshMs = now + 1500;
        ka_premotion::RefreshTargets(env);
    }
    if (!KillAuraPacketAttackReady() || !g_rotationYawField || !g_rotationPitchField ||
        !g_theWorldField || !g_thePlayerField || !g_playerEntitiesField ||
        !g_posXField || !g_posYField || !g_posZField || !g_getHealthMethod ||
        !g_listSizeMethod || !g_listGetMethod) {
        if (!g_loggedKillAuraAttackFail && !KillAuraPacketAttackReady()) {
            g_loggedKillAuraAttackFail = true;
            Log("KillAura: OpenMyau attack JNI unresolved (swing/C02/sync/local)");
        }
        ka_premotion::SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);
        return;
    }

    jobject worldObj = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); worldObj = nullptr; }
    jobject selfObj = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); selfObj = nullptr; }
    if (!worldObj || !selfObj) {
        if (worldObj) env->DeleteLocalRef(worldObj);
        if (selfObj) env->DeleteLocalRef(selfObj);
        ka_premotion::SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);
        return;
    }
    ResolveKillAuraCandidateFiltersLegacy(env, selfObj);
    if (!cfg.killAuraThroughWalls && !g_killAuraCanSeeEntity) {
        SetKillAuraUnavailableReasonLegacy("Missing EntityLivingBase.canEntityBeSeen(Entity) mapping required by Through Walls=false.");
        env->DeleteLocalRef(worldObj); env->DeleteLocalRef(selfObj); return;
    }
    if (cfg.killAuraTeams && !g_killAuraIsSameTeam) {
        SetKillAuraUnavailableReasonLegacy("Missing EntityPlayer.isOnSameTeam(EntityLivingBase) mapping required by Teams filter.");
        env->DeleteLocalRef(worldObj); env->DeleteLocalRef(selfObj); return;
    }
    SetKillAuraUnavailableReasonLegacy("");
    EnsureKillAuraAutoBlockLegacy(env, selfObj);

    if (cfg.killAuraWeaponsOnly && !KillAuraHoldingWeaponLegacy(env, selfObj)) {
        env->DeleteLocalRef(worldObj);
        env->DeleteLocalRef(selfObj);
        ka_premotion::SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);
        return;
    }

    jobject listObj = env->GetObjectField(worldObj, g_playerEntitiesField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); listObj = nullptr; }
    if (!listObj) {
        env->DeleteLocalRef(worldObj);
        env->DeleteLocalRef(selfObj);
        ka_premotion::SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);
        return;
    }

    double selfX = env->GetDoubleField(selfObj, g_posXField);
    double selfY = env->GetDoubleField(selfObj, g_posYField);
    double selfZ = env->GetDoubleField(selfObj, g_posZField);
    float clientYaw = env->GetFloatField(selfObj, g_rotationYawField);
    float clientPitch = env->GetFloatField(selfObj, g_rotationPitchField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        clientYaw = 0.0f;
        clientPitch = 0.0f;
    }

    int count = env->CallIntMethod(listObj, g_listSizeMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); count = 0; }

    double attackRange = (double)cfg.killAuraAttackRange;
    double aimRange = (double)(std::max)(cfg.killAuraAutoBlockRange,
        (std::max)(cfg.killAuraSwingRange, cfg.killAuraAttackRange));
    if (aimRange < attackRange) aimRange = attackRange;
    double aimRangeSq = aimRange * aimRange;
    float fovHalf = (float)lc::ClampInt(cfg.killAuraFov, 30, 360) * 0.5f;

    double bestScore = 1e18;
    std::string bestName;
    double bestX = 0, bestY = 0, bestZ = 0, bestDistSq = 0;
    bool haveBest = false;
    int bestRangeTier = 3;
    bool haveCur = false;
    int curRangeTier = 3;
    double curX = 0, curY = 0, curZ = 0, curDistSq = 0;

    for (int i = 0; i < count; i++) {
        jobject candidate = env->CallObjectMethod(listObj, g_listGetMethod, (jint)i);
        if (env->ExceptionCheck()) { env->ExceptionClear(); break; }
        if (!candidate) continue;
        if (env->IsSameObject(candidate, selfObj) == JNI_TRUE) {
            env->DeleteLocalRef(candidate);
            continue;
        }
        if (!cfg.killAuraThroughWalls) {
            jboolean visible = env->CallBooleanMethod(selfObj, g_killAuraCanSeeEntity, candidate);
            if (env->ExceptionCheck()) { env->ExceptionClear(); visible = JNI_FALSE; }
            if (visible != JNI_TRUE) { env->DeleteLocalRef(candidate); continue; }
        }
        if (cfg.killAuraTeams) {
            jboolean teammate = env->CallBooleanMethod(selfObj, g_killAuraIsSameTeam, candidate);
            if (env->ExceptionCheck()) { env->ExceptionClear(); teammate = JNI_FALSE; }
            if (teammate == JNI_TRUE) { env->DeleteLocalRef(candidate); continue; }
        }

        float health = env->CallFloatMethod(candidate, g_getHealthMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); health = 0.0f; }
        if (!std::isfinite(health) || health <= 0.1f) {
            env->DeleteLocalRef(candidate);
            continue;
        }

        double x = env->GetDoubleField(candidate, g_posXField);
        double y = env->GetDoubleField(candidate, g_posYField);
        double z = env->GetDoubleField(candidate, g_posZField);
        double dx = x - selfX, dy = y - selfY, dz = z - selfZ;
        double distSq = dx * dx + dy * dy + dz * dz;
        if (!std::isfinite(distSq) || distSq > aimRangeSq) {
            env->DeleteLocalRef(candidate);
            continue;
        }

        double horiz = std::sqrt(dx * dx + dz * dz);
        if (horiz > 1e-6) {
            float yawTo = (float)(std::atan2(-dx, dz) * 57.29577951308232);
            if (std::abs(saaim::ShortestYawDelta(clientYaw, yawTo)) > fovHalf) {
                env->DeleteLocalRef(candidate);
                continue;
            }
        }

        std::string stableName = GetStablePlayerName(env, candidate);
        if (stableName.empty() || (cfg.killAuraBotCheck && LooksLikeFakePlayerLine(stableName))) {
            env->DeleteLocalRef(candidate);
            continue;
        }

        int armor = g_getTotalArmorValueMethod
            ? env->CallIntMethod(candidate, g_getTotalArmorValueMethod) : 0;
        if (env->ExceptionCheck()) { env->ExceptionClear(); armor = 0; }
        float healthScore = armor == 0 ? INFINITY : health * (20.0f / (float)armor);
        int hurtTime = g_hurtTimeField ? env->GetIntField(candidate, g_hurtTimeField) : 0;
        if (env->ExceptionCheck()) { env->ExceptionClear(); hurtTime = 0; }
        float fovScore = horiz > 1e-6
            ? std::abs(killaura::Shortest(clientYaw,
                (float)(std::atan2(-dx, dz) * 57.29577951308232))) : 0.0f;
        double distance = std::sqrt(distSq);
        int rangeTier = distance <= cfg.killAuraAttackRange ? 0
            : distance <= cfg.killAuraSwingRange ? 1 : 2;
        double sortScore = distSq;
        if (cfg.killAuraSort == killaura::SORT_HEALTH) sortScore = std::isfinite(healthScore)
            ? (double)healthScore * 10000.0 + distSq : 1.0e15 + distSq;
        else if (cfg.killAuraSort == killaura::SORT_HURT_TIME) sortScore = (double)hurtTime * 10000.0 + distSq;
        else if (cfg.killAuraSort == killaura::SORT_FOV) sortScore = (double)fovScore * 10000.0 + distSq;
        double score = (double)rangeTier * 1.0e12 + sortScore;
        if (score < bestScore) {
            bestScore = score;
            bestName = stableName;
            bestX = x; bestY = y; bestZ = z; bestDistSq = distSq;
            bestRangeTier = rangeTier;
            haveBest = true;
        }
        if (!g_killAuraCurrentTarget.empty() && stableName == g_killAuraCurrentTarget) {
            haveCur = true;
            curX = x; curY = y; curZ = z; curDistSq = distSq;
            curRangeTier = rangeTier;
        }
        env->DeleteLocalRef(candidate);
    }

    bool haveChosen = false;
    double chX = 0, chY = 0, chZ = 0, chDistSq = 0;
    std::string chosenName;
    if (haveCur) {
        DWORD switchDelay = (DWORD)std::max(0, cfg.killAuraSwitchDelayMs);
        bool switchAllowed = cfg.killAuraMode == killaura::TARGET_SWITCH &&
            haveBest && bestName != g_killAuraCurrentTarget &&
            (g_killAuraLastSwitchMs == 0 || now - g_killAuraLastSwitchMs >= switchDelay);
        if (switchAllowed) {
            g_killAuraCurrentTarget = bestName;
            g_killAuraLastSwitchMs = now;
            chX = bestX; chY = bestY; chZ = bestZ; chDistSq = bestDistSq;
            chosenName = bestName;
            KillAuraResetState();
        } else {
            chX = curX; chY = curY; chZ = curZ; chDistSq = curDistSq;
            chosenName = g_killAuraCurrentTarget;
        }
        haveChosen = true;
    } else if (haveBest) {
        if (bestName != g_killAuraCurrentTarget) {
            g_killAuraCurrentTarget = bestName;
            g_killAuraLastSwitchMs = now;
            KillAuraResetState();
        }
        chX = bestX; chY = bestY; chZ = bestZ; chDistSq = bestDistSq;
        chosenName = bestName;
        haveChosen = true;
    } else {
        g_killAuraCurrentTarget.clear();
        KillAuraResetState();
    }

    jobject chosenEnt = nullptr;
    if (haveChosen && !chosenName.empty()) {
        for (int i = 0; i < count && !chosenEnt; i++) {
            jobject candidate = env->CallObjectMethod(listObj, g_listGetMethod, (jint)i);
            if (env->ExceptionCheck()) { env->ExceptionClear(); break; }
            if (!candidate) continue;
            std::string n = GetStablePlayerName(env, candidate);
            if (n == chosenName)
                chosenEnt = candidate;
            else
                env->DeleteLocalRef(candidate);
        }
    }
    static std::string s_loggedKillAuraTarget;
    if (chosenEnt && chosenName != s_loggedKillAuraTarget) {
        s_loggedKillAuraTarget = chosenName;
        Log("KillAura: target acquired: " + chosenName);
    } else if (!chosenEnt) {
        s_loggedKillAuraTarget.clear();
    }
    { LockGuard lk(g_killAuraUnavailableMutex);
        g_killAuraHasTarget = haveChosen && chosenEnt;
        g_killAuraBlocking = g_killAuraAutoBlockState.isBlocking;
    }
    if (haveCur && haveBest && curRangeTier > bestRangeTier) haveCur = false;

    bool autoBlockAllowAttack = true;
    if (haveChosen && chosenEnt && cfg.killAuraAutoBlock != killaura::BLOCK_NONE) {
        bool usingItem = false;
        if (g_killAuraIsUsingItem) {
            usingItem = env->CallBooleanMethod(selfObj, g_killAuraIsUsingItem) == JNI_TRUE;
            if (env->ExceptionCheck()) { env->ExceptionClear(); usingItem = false; }
        }
        int currentSlot = KillAuraCurrentSlotLegacy(env, selfObj);
        int emptySlot = KillAuraFindSlotLegacy(env, selfObj, currentSlot, false);
        int swordSlot = KillAuraFindSlotLegacy(env, selfObj, currentSlot, true);
        int attackDelayMs = g_killAuraAttackDelayMs;
        killaura::AutoBlockInput input;
        input.mode = (killaura::AutoBlockMode)cfg.killAuraAutoBlock;
        input.hasTarget = chDistSq <= (double)cfg.killAuraAutoBlockRange * cfg.killAuraAutoBlockRange;
        input.canBlock = KillAuraHoldingWeaponLegacy(env, selfObj);
        input.playerBlocking = usingItem;
        input.usingItem = usingItem;
        input.diggingOrPlacing = false;
        input.slotsSynced = currentSlot >= 0;
        input.hasSecondSword = swordSlot >= 0;
        input.attackDelayMs = attackDelayMs;
        killaura::AutoBlockResult result = killaura::StepAutoBlock(g_killAuraAutoBlockState, input);
        { LockGuard lk(g_killAuraUnavailableMutex); g_killAuraBlocking = g_killAuraAutoBlockState.isBlocking; }
        autoBlockAllowAttack = result.allowAttack;
        if ((result.actions & killaura::ACTION_BLINK_ON) != 0) {
            SetKillAuraUnavailableReasonLegacy(
                "Selected legacy mode requires ordered Blink packet buffering; broker is not armed.");
            autoBlockAllowAttack = false;
        } else {
            if ((result.actions & killaura::ACTION_STOP_BLOCK) != 0)
                autoBlockAllowAttack = KillAuraStopBlockLegacy(env, selfObj) && autoBlockAllowAttack;
            if ((result.actions & killaura::ACTION_SWAP_EMPTY) != 0 && emptySlot >= 0) {
                bool slotsOk = KillAuraSendSlotLegacy(env, emptySlot);
                if (cfg.killAuraAutoBlock == killaura::BLOCK_SPOOF)
                    slotsOk = KillAuraSendSlotLegacy(env, currentSlot) && slotsOk;
                autoBlockAllowAttack = slotsOk && autoBlockAllowAttack;
            }
            if ((result.actions & killaura::ACTION_SWAP_SWORD) != 0 && swordSlot >= 0)
                autoBlockAllowAttack = KillAuraSendSlotLegacy(env, swordSlot) && autoBlockAllowAttack;
            if ((result.actions & killaura::ACTION_START_BLOCK) != 0)
                autoBlockAllowAttack = KillAuraStartBlockLegacy(env, selfObj) && autoBlockAllowAttack;
        }
    }

    if (haveChosen && chosenEnt) {
        const double eyeHeight = 1.62;
        double eyeX = selfX, eyeY = selfY + eyeHeight, eyeZ = selfZ;
        float randomization = cfg.killAuraRandomize ? cfg.killAuraRandomizeRange * 100.0f : 0.0f;
        float wander = saaim::AimWanderStrength(randomization);

        if (now >= g_killAuraNextAimPointMs) {
            saaim::AimPointFracs fracs;
            if (saaim::SelectAimPointFracs(
                    eyeX, eyeY, eyeZ, chX, chY, chZ, attackRange, wander,
                    KillAuraRand01, &fracs)) {
                g_killAuraAimFracX = fracs.x;
                g_killAuraAimFracY = fracs.y;
                g_killAuraAimFracZ = fracs.z;
            }
            g_killAuraNextAimPointMs = now + 180 + (DWORD)(rand() % 220);
        }

        double aimFeetX = chX, aimFeetY = chY, aimFeetZ = chZ;
        killaura::Vec3 lastEntity = { g_killAuraPrevValid ? g_killAuraPrevTX : chX,
                                     g_killAuraPrevValid ? g_killAuraPrevTY : chY,
                                     g_killAuraPrevValid ? g_killAuraPrevTZ : chZ };
        if (g_killAuraPrevValid) {
            aimFeetX += (chX - g_killAuraPrevTX) * 0.55;
            aimFeetY += (chY - g_killAuraPrevTY) * 0.25;
            aimFeetZ += (chZ - g_killAuraPrevTZ) * 0.55;
        }
        g_killAuraPrevTX = chX;
        g_killAuraPrevTY = chY;
        g_killAuraPrevTZ = chZ;
        g_killAuraPrevValid = true;

        double minX, minY, minZ, maxX, maxY, maxZ;
        saaim::PlayerAabb(aimFeetX, aimFeetY, aimFeetZ, &minX, &minY, &minZ, &maxX, &maxY, &maxZ);
        saaim::Vec3 aimPoint = saaim::PointFromFracs(
            minX, minY, minZ, maxX, maxY, maxZ,
            g_killAuraAimFracX, g_killAuraAimFracY, g_killAuraAimFracZ);
        saaim::Angles targetAng = saaim::AnglesToPoint(eyeX, eyeY, eyeZ, aimPoint.x, aimPoint.y, aimPoint.z);

        if (std::isfinite(targetAng.yaw) && std::isfinite(targetAng.pitch)) {
            int rotMode = cfg.killAuraRotMode;
            float fromYaw = g_killAuraCombatValid ? g_killAuraCombatYaw : clientYaw;
            float fromPitch = g_killAuraCombatValid ? g_killAuraCombatPitch : clientPitch;
            killaura::Angles current = { fromYaw, fromPitch };
            killaura::Angles desired = { targetAng.yaw, targetAng.pitch };
            killaura::Vec3 eyes = { eyeX, eyeY, eyeZ };
            killaura::Box targetBox = { minX, minY, minZ, maxX, maxY, maxZ };
            if (rotMode == killaura::ROT_NONE) {
                desired.yaw = clientYaw;
                desired.pitch = clientPitch;
            } else if (rotMode == killaura::ROT_HYPIXEL) {
                killaura::Vec3 entity = { chX, chY, chZ };
                desired = killaura::RavenRaw(eyes, 0.0f, current.yaw, current.pitch,
                    entity, lastEntity, 1.62f, cfg.killAuraRavenPredictTicks);
                desired.yaw += KillAuraRandSigned01() * cfg.killAuraRavenYawRandom;
                desired = killaura::RavenSmooth(desired, current,
                    cfg.killAuraRavenSmoothing, 0.0f, 0.0f);
            } else if (rotMode == killaura::ROT_LIQUID_BOUNCE) {
                desired = killaura::LimitAngleChange(current, desired,
                    cfg.killAuraLbHorizontalSpeed, cfg.killAuraLbVerticalSpeed,
                    cfg.killAuraLbSmooth);
            } else {
                float smooth = (float)cfg.killAuraSmoothing / 100.0f;
                desired = killaura::StandardRotation(eyes, targetBox, current,
                    (float)cfg.killAuraAngleStep + KillAuraRandSigned01() * 5.0f, smooth,
                    KillAuraRandSigned01() * 0.1f, KillAuraRandSigned01() * 0.1f);
                desired.yaw += KillAuraRandSigned01() * 2.5f;
                desired.pitch += KillAuraRandSigned01() * 1.5f;
            }
            desired = killaura::GcdPatch(desired, current, g_killAuraMouseSens);

            saaim::StepResult step = {};
            step.yaw = desired.yaw;
            step.pitch = desired.pitch;
            step.bodyYaw = desired.yaw;

            g_killAuraCombatYaw = step.yaw;
            g_killAuraCombatPitch = step.pitch;
            g_killAuraCombatValid = true;
            g_killAuraYawVel = step.yawVel;
            g_killAuraPitchVel = step.pitchVel;
            g_killAuraBodyYaw = step.bodyYaw;
            g_killAuraBodyValid = true;
            g_killAuraOvershootActive = step.overshootActive;
            g_killAuraOvershootYaw = step.overshootYaw;
            g_killAuraOvershootPitch = step.overshootPitch;

            // None has no rotation; Legit and LockView are visible. Only the
            // server-side modes depend on the PRE movement hook.
            const bool silentMode = rotMode == killaura::ROT_SILENT ||
                rotMode == killaura::ROT_LIQUID_BOUNCE ||
                rotMode == killaura::ROT_HYPIXEL;
            if (silentMode) {
                ka_premotion::SetSilentCombatAngles(
                    true, step.yaw, step.pitch, step.bodyYaw, true);
            } else {
                ka_premotion::SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);
                SetKillAuraPlayerLookLegacy(env, selfObj, step.yaw, step.pitch, step.bodyYaw);
            }

            float gateRand = (rotMode == 2) ? randomization : (std::min)(randomization, 12.0f);
            bool onTarget = saaim::IsOnTarget(
                eyeX, eyeY, eyeZ, chX, chY, chZ, step.yaw, step.pitch, gateRand);
            if (rotMode == killaura::ROT_NONE) onTarget = true;
            double hitRange = attackRange * 0.92;
            bool inAttackRange = chDistSq <= hitRange * hitRange;
            if (rotMode == 2)
                onTarget = onTarget || (std::abs(saaim::ShortestYawDelta(step.yaw, targetAng.yaw)) < 8.0f);

            if (onTarget && inAttackRange && autoBlockAllowAttack) {
                    const bool cpsReady = g_killAuraAttackDelayMs <= 0;

                    if (cpsReady) {
                        // Packet/Cooldown: queue C0A→C02 for Premotion (send-queue hook fires pre-C03).
                        // Prefer queue whenever Premotion is ready (Silent or Legit/Lock).
                        // Without Premotion: Silent waits; Legit/Lock fires immediately (degraded).
                        bool queuedOrFired = false;
                        if (ka_premotion::IsOperational()) {
                            queuedOrFired = ka_premotion::QueueAttack(env, chosenEnt);
                        } else if (!silentMode) {
                            queuedOrFired = KillAuraPerformAttackLegacy(env, selfObj, chosenEnt);
                        } else if (!g_loggedKillAuraSilentNoPremotion) {
                            g_loggedKillAuraSilentNoPremotion = true;
                            Log("KillAura: Premotion not armed yet; Silent packet attacks waiting for send-queue hook");
                        }
                        if (queuedOrFired) {
                            g_killAuraLastAttackMs = now;
                            g_killAuraAttackDelayMs = (int)killaura::AttackDelayMs(
                                g_killAuraAutoBlockState.isBlocking, cfg.killAuraAutoBlockCps,
                                cfg.killAuraRecordCps, g_killAuraPatternIndex,
                                cfg.killAuraMinCps, cfg.killAuraMaxCps, KillAuraRand01());
                            if (cfg.killAuraRecordCps) ++g_killAuraPatternIndex;
                            g_killAuraNextIntervalMs = (DWORD)g_killAuraAttackDelayMs;
                        }
                    }
            }
        }
    } else {
        ka_premotion::SetSilentCombatAngles(false, 0.0f, 0.0f, 0.0f, false);
        ka_premotion::ClearPendingAttack(env);
    }

    if (chosenEnt) env->DeleteLocalRef(chosenEnt);
    env->DeleteLocalRef(listObj);
    env->DeleteLocalRef(worldObj);
    env->DeleteLocalRef(selfObj);
}

static void UpdateAntiDebuffLegacy(JNIEnv* env, const Config& cfg) {
    if (!env || !g_mcInstance || !g_thePlayerField) return;
    if (!cfg.antiDebuffEnabled) {
        // Module off: make sure the fog hook stops overriding so blindness renders normally.
        InterlockedExchange(&g_blindFogSuppress, 0);
        return;
    }

    DWORD nowMs = GetTickCount();
    if (nowMs - g_lastAntiDebuffTickMs < 50) return; // ~20 tps
    g_lastAntiDebuffTickMs = nowMs;

    jobject selfObj = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); selfObj = nullptr; }
    if (!selfObj) return;

    if (!g_removePotionEffectMethod && !g_antiDebuffMethodResolved) {
        jclass cls = env->GetObjectClass(selfObj);
        if (cls) {
            // removePotionEffect(int) lives on EntityLivingBase; resolvable from the player class.
            g_removePotionEffectMethod = env->GetMethodID(cls, "removePotionEffect", "(I)V");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_removePotionEffectMethod = nullptr; }
            if (!g_removePotionEffectMethod) {
                g_removePotionEffectMethod = env->GetMethodID(cls, "func_82170_o", "(I)V");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_removePotionEffectMethod = nullptr; }
            }
            env->DeleteLocalRef(cls);
        }
        g_antiDebuffMethodResolved = (g_removePotionEffectMethod != nullptr);
        if (g_antiDebuffMethodResolved) {
            Log("AntiDebuff: JNI resolved (removePotionEffect).");
        } else if (!g_loggedAntiDebuffResolveFail) {
            g_loggedAntiDebuffResolveFail = true;
            Log("AntiDebuff: removePotionEffect mapping unavailable (module idle until resolved).");
        }
    }

    if (g_removePotionEffectMethod) {
        // Approach A (anticheat-safe): keep BLINDNESS (id 15) in the effect map so its
        // client-side sprint and critical-hit gating stays in sync with the server.
        // Stripping it would let you sprint/crit while the server still thinks you are
        // blind -> movement/combat desync that anticheats flag. The blindness *fog* is a
        // pure visual and is neutralized separately via the glFogf hook (see
        // HookedGlFogf / g_blindFogSuppress). Nausea/Confusion (id 9) gates nothing, so
        // removing it is anticheat-safe and clears the screen-warp fully.
        static const int kIds[] = { 9 };
        for (int id : kIds) {
            env->CallVoidMethod(selfObj, g_removePotionEffectMethod, id);
            if (env->ExceptionCheck()) { env->ExceptionClear(); }
        }
    }

    // --- Blindness fog suppression (approach A) ------------------------------------
    // We deliberately leave BLINDNESS applied, so detect it here each tick and drive the
    // glFogf render hook. Resolve isPotionActive(Potion) + the static Potion.blindness once.
    if (!g_blindDetectResolved && !g_loggedBlindDetectFail) {
        jclass cls = env->GetObjectClass(selfObj);
        if (cls) {
            g_isPotionActiveMethod = env->GetMethodID(cls, "isPotionActive", "(Lnet/minecraft/potion/Potion;)Z");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_isPotionActiveMethod = nullptr; }
            if (!g_isPotionActiveMethod) {
                g_isPotionActiveMethod = env->GetMethodID(cls, "func_70644_a", "(Lnet/minecraft/potion/Potion;)Z");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_isPotionActiveMethod = nullptr; }
            }
            env->DeleteLocalRef(cls);
        }

        jobject gcl = EnsureGameClassLoader(env);
        jclass potionCls = gcl ? LoadClassWithLoader(env, gcl, "net.minecraft.potion.Potion") : nullptr;
        if (potionCls) {
            jfieldID blindField = env->GetStaticFieldID(potionCls, "blindness", "Lnet/minecraft/potion/Potion;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); blindField = nullptr; }
            if (!blindField) {
                blindField = env->GetStaticFieldID(potionCls, "field_76440_q", "Lnet/minecraft/potion/Potion;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); blindField = nullptr; }
            }
            if (blindField) {
                jobject h = env->GetStaticObjectField(potionCls, blindField);
                if (env->ExceptionCheck()) { env->ExceptionClear(); h = nullptr; }
                if (h) { g_blindnessPotion = env->NewGlobalRef(h); env->DeleteLocalRef(h); }
            }
            env->DeleteLocalRef(potionCls);
        }

        if (g_isPotionActiveMethod && g_blindnessPotion) {
            g_blindDetectResolved = true;
            Log("AntiDebuff: blindness-fog detection resolved (isPotionActive + Potion.blindness).");
        } else if (!g_loggedBlindDetectFail) {
            g_loggedBlindDetectFail = true;
            Log(std::string("AntiDebuff: blindness-fog detection mappings unavailable - isPotionActive=")
                + (g_isPotionActiveMethod ? "1" : "0")
                + " blindness=" + (g_blindnessPotion ? "1" : "0")
                + " (fog suppression idle).");
        }
    }

    if (g_blindDetectResolved) {
        jboolean blind = env->CallBooleanMethod(selfObj, g_isPotionActiveMethod, g_blindnessPotion);
        if (env->ExceptionCheck()) { env->ExceptionClear(); blind = JNI_FALSE; }
        InterlockedExchange(&g_blindFogSuppress, blind ? 1 : 0);
    } else {
        InterlockedExchange(&g_blindFogSuppress, 0);
    }

    env->DeleteLocalRef(selfObj);
}

static bool EnsureReachMappings(JNIEnv* env) {
    if (!env || !g_mcClass || !g_movingObjectPositionClass) return false;

    if (!g_mopEntityCtor) {
        g_mopEntityCtor = env->GetMethodID(g_movingObjectPositionClass, "<init>", "(Lnet/minecraft/entity/Entity;)V");
        g_mopCtorNeedsVec3 = false;
        if (!g_mopEntityCtor) {
            env->ExceptionClear();
            g_mopEntityCtor = env->GetMethodID(g_movingObjectPositionClass, "<init>", "(Lnet/minecraft/entity/Entity;Lnet/minecraft/util/Vec3;)V");
            g_mopCtorNeedsVec3 = (g_mopEntityCtor != nullptr);
        }
        if (!g_mopEntityCtor) env->ExceptionClear();
    }

    if (g_mopCtorNeedsVec3) {
        if (!g_vec3Class) {
            jclass vec3Local = env->FindClass("net/minecraft/util/Vec3");
            if (!vec3Local) {
                env->ExceptionClear();
                jobject gcl = EnsureGameClassLoader(env);
                if (gcl) vec3Local = LoadClassWithLoader(env, gcl, "net.minecraft.util.Vec3");
            }
            if (vec3Local && !env->ExceptionCheck()) {
                g_vec3Class = (jclass)env->NewGlobalRef(vec3Local);
                env->DeleteLocalRef(vec3Local);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
        }
        if (g_vec3Class && !g_vec3Ctor) {
            g_vec3Ctor = env->GetMethodID(g_vec3Class, "<init>", "(DDD)V");
            if (!g_vec3Ctor) env->ExceptionClear();
        }
    }

    if (!g_entityHitField) {
        g_entityHitField = env->GetFieldID(g_movingObjectPositionClass, "entityHit", "Lnet/minecraft/entity/Entity;");
        if (!g_entityHitField) {
            env->ExceptionClear();
            g_entityHitField = env->GetFieldID(g_movingObjectPositionClass, "field_72308_g", "Lnet/minecraft/entity/Entity;");
        }
        if (!g_entityHitField) env->ExceptionClear();
    }

    if (!g_mopEntityTypeConst) {
        jclass mopTypeLocal = env->FindClass("net/minecraft/util/MovingObjectPosition$MovingObjectType");
        if (!mopTypeLocal) {
            env->ExceptionClear();
            jobject gcl = EnsureGameClassLoader(env);
            if (gcl) mopTypeLocal = LoadClassWithLoader(env, gcl, "net.minecraft.util.MovingObjectPosition$MovingObjectType");
        }
        if (mopTypeLocal && !env->ExceptionCheck()) {
            jfieldID entityConstField = env->GetStaticFieldID(mopTypeLocal, "ENTITY", "Lnet/minecraft/util/MovingObjectPosition$MovingObjectType;");
            if (!entityConstField) {
                env->ExceptionClear();
                entityConstField = env->GetStaticFieldID(mopTypeLocal, "field_72310_e", "Lnet/minecraft/util/MovingObjectPosition$MovingObjectType;");
            }
            if (entityConstField) {
                jobject entityConstLocal = env->GetStaticObjectField(mopTypeLocal, entityConstField);
                if (!env->ExceptionCheck() && entityConstLocal) {
                    g_mopEntityTypeConst = env->NewGlobalRef(entityConstLocal);
                    env->DeleteLocalRef(entityConstLocal);
                } else if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                }
            } else {
                env->ExceptionClear();
            }
            env->DeleteLocalRef(mopTypeLocal);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    return g_objectMouseOverField != nullptr;
}

static void UpdateReach(JNIEnv* env, const Config& cfg, const GameState& state, bool forceClick = false) {
    if (!env) return;

    DWORD nowMs = GetTickCount();
    bool lmbPhysicalDown = (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0;
    bool syntheticClicking = cfg.clicking && cfg.leftClick;
    bool clickEdge = forceClick
        || (lmbPhysicalDown && !g_reachClickPrevDown)
        || (syntheticClicking && !g_reachClickPrevSynthetic);
    g_reachClickPrevDown = lmbPhysicalDown;
    g_reachClickPrevSynthetic = syntheticClicking;

    if (!cfg.reachEnabled) {
        g_reachAllowCurrentClick = false;
        g_reachCurrentClickRange = 3.0;
        g_reachClickWindowUntilMs = 0;
        g_reachLastRollMs = 0;
        g_reachClickPrevSynthetic = false;
        if (g_reachCurrentTarget) {
            env->DeleteGlobalRef(g_reachCurrentTarget);
            g_reachCurrentTarget = nullptr;
        }
        return;
    }

    if (clickEdge || ((lmbPhysicalDown || syntheticClicking) && (nowMs - g_reachLastRollMs) >= 120)) {
        g_reachAllowCurrentClick = ((rand() % 100) < cfg.reachChance);
        float reachSpan = cfg.reachMax - cfg.reachMin;
        if (reachSpan < 0.0f) reachSpan = 0.0f;
        float rfrac = (float)rand() / (float)RAND_MAX;
        g_reachCurrentClickRange = (double)(cfg.reachMin + (rfrac * reachSpan));
        if (g_reachCurrentClickRange < 3.0) g_reachCurrentClickRange = 3.0;
        g_reachLastRollMs = nowMs;
        g_reachClickWindowUntilMs = nowMs + 120;
    }

    bool clickWindowActive = forceClick || lmbPhysicalDown || syntheticClicking || nowMs <= g_reachClickWindowUntilMs;
    if (!clickWindowActive) {
        g_reachAllowCurrentClick = false;
        if (g_reachCurrentTarget) {
            env->DeleteGlobalRef(g_reachCurrentTarget);
            g_reachCurrentTarget = nullptr;
        }
        return;
    }

    auto maybeLogReach = [&](const std::string& msg) {
        DWORD t = GetTickCount();
        if (t - g_lastReachDebugLogMs >= 1000) {
            g_lastReachDebugLogMs = t;
            Log(std::string("Reach: ") + msg);
        }
    };

    if (!g_reachAllowCurrentClick) {
        maybeLogReach("roll denied this click");
        return;
    }
    if (!g_mcInstance || !g_thePlayerField || (!g_objectMouseOverField && !g_pointedEntityField)) return;
    if (!EnsureReachMappings(env)) return;
    if (!state.mapped || state.guiOpen) return;

    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        player = nullptr;
    }
    if (!player) {
        maybeLogReach("no local player");
        return;
    }

    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        world = nullptr;
    }
    if (!world) {
        maybeLogReach("no world");
        env->DeleteLocalRef(player);
        return;
    }

    if (!g_playerEntitiesField || !g_listSizeMethod || !g_listGetMethod) {
        TryResolveWorldMappings(env);
    }
    if (!g_playerEntitiesField || !g_listSizeMethod || !g_listGetMethod || !g_posXField || !g_posYField || !g_posZField) {
        maybeLogReach("missing mappings: players/list/pos");
        env->DeleteLocalRef(world);
        env->DeleteLocalRef(player);
        return;
    }

    jobject list = env->GetObjectField(world, g_playerEntitiesField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        list = nullptr;
    }
    if (!list) {
        maybeLogReach("player list unavailable");
        env->DeleteLocalRef(world);
        env->DeleteLocalRef(player);
        return;
    }

    int size = env->CallIntMethod(list, g_listSizeMethod);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        size = 0;
    }

    double px = env->GetDoubleField(player, g_posXField);
    double py = env->GetDoubleField(player, g_posYField);
    double pz = env->GetDoubleField(player, g_posZField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        size = 0;
    }

    float localYaw = 0.0f;
    float localPitch = 0.0f;
    bool haveViewAngles = false;
    if (g_rotationYawField && g_rotationPitchField) {
        localYaw = env->GetFloatField(player, g_rotationYawField);
        localPitch = env->GetFloatField(player, g_rotationPitchField);
        if (!env->ExceptionCheck()) {
            haveViewAngles = true;
        } else {
            env->ExceptionClear();
        }
    }

    const double kPi = 3.14159265358979323846;
    double yawRad = localYaw * (kPi / 180.0);
    double pitchRad = localPitch * (kPi / 180.0);
    double lookX = -std::sin(yawRad) * std::cos(pitchRad);
    double lookY = -std::sin(pitchRad);
    double lookZ = std::cos(yawRad) * std::cos(pitchRad);
    double lookLenSq = lookX * lookX + lookY * lookY + lookZ * lookZ;
    if (lookLenSq > 1e-9) {
        double invLen = 1.0 / std::sqrt(lookLenSq);
        lookX *= invLen;
        lookY *= invLen;
        lookZ *= invLen;
    } else {
        haveViewAngles = false;
    }

    double eyeX = px;
    double eyeY = py + 1.62;
    double eyeZ = pz;

    double sampleReach = g_reachCurrentClickRange;

    jobject bestEntity = nullptr;
    double bestDistSq = sampleReach * sampleReach;
    double bestPerpSq = 1e18;
    double bestForward = 1e18;

    for (int i = 0; i < size; i++) {
        jobject entity = env->CallObjectMethod(list, g_listGetMethod, i);
        if (env->ExceptionCheck()) {
            env->ExceptionClear();
            break;
        }
        if (!entity) continue;
        if (env->IsSameObject(entity, player)) {
            env->DeleteLocalRef(entity);
            continue;
        }

        std::string stableName = GetStablePlayerName(env, entity);
        if (stableName.empty()) {
            env->DeleteLocalRef(entity);
            continue;
        }

        double ex = env->GetDoubleField(entity, g_posXField);
        double ey = env->GetDoubleField(entity, g_posYField);
        double ez = env->GetDoubleField(entity, g_posZField);
        if (env->ExceptionCheck()) {
            env->ExceptionClear();
            env->DeleteLocalRef(entity);
            continue;
        }

        double targetX = ex;
        double targetY = ey + 1.62;
        double targetZ = ez;

        double dx = targetX - eyeX;
        double dy = targetY - eyeY;
        double dz = targetZ - eyeZ;
        double distSq = dx * dx + dy * dy + dz * dz;

        bool candidate = false;
        double candidatePerpSq = 0.0;
        double candidateForward = 0.0;

        if (haveViewAngles) {
            candidateForward = dx * lookX + dy * lookY + dz * lookZ;
            if (candidateForward > 0.0 && candidateForward <= sampleReach) {
                candidatePerpSq = distSq - (candidateForward * candidateForward);
                if (candidatePerpSq < 0.0) candidatePerpSq = 0.0;

                // Approximates a permissive hit cylinder around the crosshair ray.
                // This keeps targeting intuitive while still requiring aim direction.
                double maxPerp = 0.95;
                if (candidatePerpSq <= (maxPerp * maxPerp)) {
                    candidate = true;
                }
            }
        } else {
            if (distSq <= (sampleReach * sampleReach)) {
                candidate = true;
            }
        }

        if (candidate) {
            bool better = false;
            if (haveViewAngles) {
                if (candidatePerpSq + 1e-6 < bestPerpSq) {
                    better = true;
                } else if (std::abs(candidatePerpSq - bestPerpSq) <= 1e-6 && candidateForward < bestForward) {
                    better = true;
                }
            } else if (distSq <= bestDistSq) {
                better = true;
            }

            if (better) {
                if (bestEntity) env->DeleteLocalRef(bestEntity);
                bestEntity = entity;
                bestDistSq = distSq;
                bestPerpSq = candidatePerpSq;
                bestForward = candidateForward;
                continue;
            }
        }

        if (!haveViewAngles && distSq <= bestDistSq) {
            if (bestEntity) env->DeleteLocalRef(bestEntity);
            bestEntity = entity;
            bestDistSq = distSq;
            continue;
        }

        env->DeleteLocalRef(entity);
    }

    if (bestEntity) {
        if (g_reachCurrentTarget) {
            env->DeleteGlobalRef(g_reachCurrentTarget);
            g_reachCurrentTarget = nullptr;
        }
        g_reachCurrentTarget = env->NewGlobalRef(bestEntity);

        jobject mop = nullptr;
        if (g_mopCtorNeedsVec3) {
            if (g_vec3Class && g_vec3Ctor) {
                double ex = env->GetDoubleField(bestEntity, g_posXField);
                double ey = env->GetDoubleField(bestEntity, g_posYField);
                double ez = env->GetDoubleField(bestEntity, g_posZField);
                if (!env->ExceptionCheck()) {
                    jobject vec = env->NewObject(g_vec3Class, g_vec3Ctor, ex, ey + 1.62, ez);
                    if (!env->ExceptionCheck() && vec) {
                        mop = env->NewObject(g_movingObjectPositionClass, g_mopEntityCtor, bestEntity, vec);
                        env->DeleteLocalRef(vec);
                    } else if (env->ExceptionCheck()) {
                        env->ExceptionClear();
                    }
                } else {
                    env->ExceptionClear();
                }
            }
            if (!mop && !env->ExceptionCheck()) {
                // Fallback: entity-only ctor may still exist on some forks.
                jobject fallback = env->NewObject(g_movingObjectPositionClass, g_mopEntityCtor, bestEntity);
                if (!env->ExceptionCheck() && fallback) {
                    mop = fallback;
                } else if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                }
            }
        } else {
            mop = env->NewObject(g_movingObjectPositionClass, g_mopEntityCtor, bestEntity);
        }

        bool pointedPatched = false;
        if (g_pointedEntityField) {
            env->SetObjectField(g_mcInstance, g_pointedEntityField, bestEntity);
            if (!env->ExceptionCheck()) {
                pointedPatched = true;
            } else {
                env->ExceptionClear();
            }
        }

        if (!mop && !env->ExceptionCheck() && g_objectMouseOverField) {
            jobject curMop = env->GetObjectField(g_mcInstance, g_objectMouseOverField);
            if (!env->ExceptionCheck() && curMop) {
                if (g_entityHitField) {
                    env->SetObjectField(curMop, g_entityHitField, bestEntity);
                    if (!env->ExceptionCheck()) {
                        if (g_typeOfHitField && g_mopEntityTypeConst) {
                            env->SetObjectField(curMop, g_typeOfHitField, g_mopEntityTypeConst);
                            if (env->ExceptionCheck()) env->ExceptionClear();
                        }
                        env->SetObjectField(g_mcInstance, g_objectMouseOverField, curMop);
                        maybeLogReach(std::string("applied (entityHit patch") + (pointedPatched ? "+pointedEntity" : "") + ") range=" + std::to_string(sampleReach) + " dist=" + std::to_string(std::sqrt(bestDistSq)));
                    } else {
                        env->ExceptionClear();
                    }
                } else {
                    maybeLogReach("entityHit field unresolved");
                }
                env->DeleteLocalRef(curMop);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
        }

        if (!env->ExceptionCheck() && mop) {
            env->SetObjectField(g_mcInstance, g_objectMouseOverField, mop);
            if (env->ExceptionCheck()) env->ExceptionClear();
            env->DeleteLocalRef(mop);
            maybeLogReach(std::string("applied (mop") + (pointedPatched ? "+pointedEntity" : "") + ") range=" + std::to_string(sampleReach) + " dist=" + std::to_string(std::sqrt(bestDistSq)));
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
            maybeLogReach("failed to build/apply MOP");
        } else if (pointedPatched) {
            maybeLogReach(std::string("applied (pointedEntity only) range=") + std::to_string(sampleReach) + " dist=" + std::to_string(std::sqrt(bestDistSq)));
        }
        env->DeleteLocalRef(bestEntity);
    } else {
        if (g_reachCurrentTarget) {
            env->DeleteGlobalRef(g_reachCurrentTarget);
            g_reachCurrentTarget = nullptr;
        }
        maybeLogReach(std::string("no target in range=") + std::to_string(sampleReach));
    }

    env->DeleteLocalRef(list);
    env->DeleteLocalRef(world);
    env->DeleteLocalRef(player);
}

static jobject GetSneakKeyBinding(JNIEnv* env) {
    if (!env || !g_mcInstance || !g_gameSettingsField || !g_keyBindSneakField) return nullptr;
    jobject gs = env->GetObjectField(g_mcInstance, g_gameSettingsField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return nullptr; }
    if (!gs) return nullptr;

    jobject key = env->GetObjectField(gs, g_keyBindSneakField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); key = nullptr; }
    env->DeleteLocalRef(gs);
    return key;
}

static int GetSneakKeyCode(JNIEnv* env) {
    jobject key = GetSneakKeyBinding(env);
    if (!key) return 0;

    int keyCode = 0;
    if (g_keyBindingGetKeyCodeMethod) {
        keyCode = env->CallIntMethod(key, g_keyBindingGetKeyCodeMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); keyCode = 0; }
    } else if (g_keyBindingKeyCodeField) {
        keyCode = env->GetIntField(key, g_keyBindingKeyCodeField);
        if (env->ExceptionCheck()) { env->ExceptionClear(); keyCode = 0; }
    }

    env->DeleteLocalRef(key);
    return keyCode;
}

static bool IsConfiguredSneakPhysicallyDown(JNIEnv* env) {
    int keyCode = GetSneakKeyCode(env);
    if (keyCode >= 0) {
        if (!g_lwjglKeyboardClass || !g_keyboardIsKeyDownMethod) return false;
        jboolean down = env->CallStaticBooleanMethod(g_lwjglKeyboardClass, g_keyboardIsKeyDownMethod, (jint)keyCode);
        if (env->ExceptionCheck()) { env->ExceptionClear(); return false; }
        return down == JNI_TRUE;
    }

    if (!g_lwjglMouseClass || !g_mouseIsButtonDownMethod) return false;
    int mouseButton = keyCode + 100;
    if (mouseButton < 0) return false;
    jboolean down = env->CallStaticBooleanMethod(g_lwjglMouseClass, g_mouseIsButtonDownMethod, (jint)mouseButton);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return false; }
    return down == JNI_TRUE;
}

static bool SetSneakKeyBindingState(JNIEnv* env, bool pressed) {
    if (!env) return false;
    jobject key = GetSneakKeyBinding(env);
    if (!key) return false;

    int keyCode = 0;
    if (g_keyBindingGetKeyCodeMethod) {
        keyCode = env->CallIntMethod(key, g_keyBindingGetKeyCodeMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); keyCode = 0; }
    } else if (g_keyBindingKeyCodeField) {
        keyCode = env->GetIntField(key, g_keyBindingKeyCodeField);
        if (env->ExceptionCheck()) { env->ExceptionClear(); keyCode = 0; }
    }

    bool ok = false;
    if (g_keyBindingClass && g_keyBindingSetKeyBindStateMethod) {
        env->CallStaticVoidMethod(g_keyBindingClass, g_keyBindingSetKeyBindStateMethod, (jint)keyCode, pressed ? JNI_TRUE : JNI_FALSE);
        ok = !env->ExceptionCheck();
        if (!ok) env->ExceptionClear();
    }

    if (!ok && g_keyBindingPressedField) {
        env->SetBooleanField(key, g_keyBindingPressedField, pressed ? JNI_TRUE : JNI_FALSE);
        ok = !env->ExceptionCheck();
        if (!ok) env->ExceptionClear();
    }

    env->DeleteLocalRef(key);
    return ok;
}

static void SetSpeedBridgeSneak(JNIEnv* env, bool pressed) {
    if (SetSneakKeyBindingState(env, pressed)) {
        g_speedBridgeManagingSneak = true;
    }
}

static void ReleaseSpeedBridgeSneak(JNIEnv* env) {
    if (!g_speedBridgeManagingSneak) return;
    if (IsConfiguredSneakPhysicallyDown(env)) {
        g_speedBridgeManagingSneak = false;
        return;
    }

    SetSneakKeyBindingState(env, false);
    g_speedBridgeManagingSneak = false;
}

static void ResetSpeedBridgeMovementTracking() {
    g_speedBridgeHaveLastPos = false;
    g_speedBridgeDirX = 0;
    g_speedBridgeDirZ = 0;
}

static void UpdateSpeedBridgeDirection(const GameState& state) {
    if (!g_speedBridgeHaveLastPos) {
        g_speedBridgeHaveLastPos = true;
        g_speedBridgeLastPosX = state.posX;
        g_speedBridgeLastPosZ = state.posZ;
        return;
    }

    double dx = state.posX - g_speedBridgeLastPosX;
    double dz = state.posZ - g_speedBridgeLastPosZ;
    g_speedBridgeLastPosX = state.posX;
    g_speedBridgeLastPosZ = state.posZ;

    const double movementEpsilon = 0.0008;
    double ax = std::abs(dx);
    double az = std::abs(dz);
    if (ax < movementEpsilon && az < movementEpsilon) return;

    int dirX = 0;
    int dirZ = 0;
    if (ax >= az * 0.65) dirX = (dx > 0.0) ? 1 : -1;
    if (az >= ax * 0.65) dirZ = (dz > 0.0) ? 1 : -1;
    g_speedBridgeDirX = dirX;
    g_speedBridgeDirZ = dirZ;
}

static double SpeedBridgeSupportProbeDistance(const Config& cfg) {
    double t = ((double)cfg.speedBridgeDelayMs - 20.0) / 230.0;
    if (t < 0.0) t = 0.0;
    if (t > 1.0) t = 1.0;
    return 0.31 + (0.14 * t);
}

static bool IsSolidBlockAt(JNIEnv* env, double x, double y, double z) {
    if (!env || !g_mcInstance || !g_theWorldField || !g_blockPosClass || !g_blockPosIntCtor) return false;

    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return false; }
    if (!world) return false;

    if (!g_worldGetBlockStateMethod) {
        jclass worldCls = env->GetObjectClass(world);
        if (worldCls && !env->ExceptionCheck()) {
            g_worldGetBlockStateMethod = env->GetMethodID(worldCls, "getBlockState", "(Lnet/minecraft/util/BlockPos;)Lnet/minecraft/block/state/IBlockState;");
            if (!g_worldGetBlockStateMethod) {
                env->ExceptionClear();
                g_worldGetBlockStateMethod = env->GetMethodID(worldCls, "func_180495_p", "(Lnet/minecraft/util/BlockPos;)Lnet/minecraft/block/state/IBlockState;");
            }
            if (!g_worldGetBlockStateMethod) env->ExceptionClear();
            env->DeleteLocalRef(worldCls);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    if (!g_worldGetBlockStateMethod) {
        env->DeleteLocalRef(world);
        return false;
    }

    int bx = (int)std::floor(x);
    int by = (int)std::floor(y);
    int bz = (int)std::floor(z);
    jobject pos = env->NewObject(g_blockPosClass, g_blockPosIntCtor, (jint)bx, (jint)by, (jint)bz);
    if (env->ExceptionCheck() || !pos) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        env->DeleteLocalRef(world);
        return false;
    }

    jobject state = env->CallObjectMethod(world, g_worldGetBlockStateMethod, pos);
    env->DeleteLocalRef(pos);
    env->DeleteLocalRef(world);
    if (env->ExceptionCheck() || !state) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }

    if (!g_blockStateGetBlockMethod) {
        jclass stateCls = env->GetObjectClass(state);
        if (stateCls && !env->ExceptionCheck()) {
            g_blockStateGetBlockMethod = env->GetMethodID(stateCls, "getBlock", "()Lnet/minecraft/block/Block;");
            if (!g_blockStateGetBlockMethod) {
                env->ExceptionClear();
                g_blockStateGetBlockMethod = env->GetMethodID(stateCls, "func_177230_c", "()Lnet/minecraft/block/Block;");
            }
            if (!g_blockStateGetBlockMethod) env->ExceptionClear();
            env->DeleteLocalRef(stateCls);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    if (!g_blockStateGetBlockMethod) {
        env->DeleteLocalRef(state);
        return false;
    }

    jobject block = env->CallObjectMethod(state, g_blockStateGetBlockMethod);
    env->DeleteLocalRef(state);
    if (env->ExceptionCheck() || !block) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }

    if (!g_blockGetMaterialMethod) {
        jclass blockCls = env->GetObjectClass(block);
        if (blockCls && !env->ExceptionCheck()) {
            g_blockGetMaterialMethod = env->GetMethodID(blockCls, "getMaterial", "()Lnet/minecraft/block/material/Material;");
            if (!g_blockGetMaterialMethod) {
                env->ExceptionClear();
                g_blockGetMaterialMethod = env->GetMethodID(blockCls, "func_149688_o", "()Lnet/minecraft/block/material/Material;");
            }
            if (!g_blockGetMaterialMethod) env->ExceptionClear();
            env->DeleteLocalRef(blockCls);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    if (!g_blockGetMaterialMethod) {
        env->DeleteLocalRef(block);
        return false;
    }

    jobject material = env->CallObjectMethod(block, g_blockGetMaterialMethod);
    env->DeleteLocalRef(block);
    if (env->ExceptionCheck() || !material) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }

    if (!g_materialIsSolidMethod) {
        jclass materialCls = env->GetObjectClass(material);
        if (materialCls && !env->ExceptionCheck()) {
            g_materialIsSolidMethod = env->GetMethodID(materialCls, "isSolid", "()Z");
            if (!g_materialIsSolidMethod) {
                env->ExceptionClear();
                g_materialIsSolidMethod = env->GetMethodID(materialCls, "func_76220_a", "()Z");
            }
            if (!g_materialIsSolidMethod) env->ExceptionClear();
            env->DeleteLocalRef(materialCls);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    bool solid = false;
    if (g_materialIsSolidMethod) {
        solid = env->CallBooleanMethod(material, g_materialIsSolidMethod) == JNI_TRUE;
        if (env->ExceptionCheck()) { env->ExceptionClear(); solid = false; }
    }
    env->DeleteLocalRef(material);
    return solid;
}

static std::string LegacyGetBlockUnlocalizedName(JNIEnv* env, jobject block) {
    if (!env || !block) return "";
    jclass blockCls = env->GetObjectClass(block);
    if (!blockCls) return "";
    if (!g_blockGetUnlocalizedNameMethod) {
        g_blockGetUnlocalizedNameMethod = env->GetMethodID(blockCls, "getUnlocalizedName", "()Ljava/lang/String;");
        if (!g_blockGetUnlocalizedNameMethod) {
            env->ExceptionClear();
            g_blockGetUnlocalizedNameMethod = env->GetMethodID(blockCls, "func_149732_F", "()Ljava/lang/String;");
        }
        if (!g_blockGetUnlocalizedNameMethod) env->ExceptionClear();
    }
    env->DeleteLocalRef(blockCls);
    if (!g_blockGetUnlocalizedNameMethod) return "";
    jstring js = (jstring)env->CallObjectMethod(block, g_blockGetUnlocalizedNameMethod);
    if (env->ExceptionCheck() || !js) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return "";
    }
    const char* utf = env->GetStringUTFChars(js, nullptr);
    std::string out = utf ? utf : "";
    if (utf) env->ReleaseStringUTFChars(js, utf);
    env->DeleteLocalRef(js);
    return out;
}

static std::string LegacyPixelPartyFormatColorLabel(const std::string& unloc) {
    if (unloc.empty()) return "clay";
    std::string lower = unloc;
    for (char& c : lower) c = (char)tolower((unsigned char)c);
    size_t dot = lower.rfind('.');
    std::string name = (dot != std::string::npos) ? lower.substr(dot + 1) : lower;
    if (name == "name" && dot != std::string::npos) {
        size_t prev = lower.rfind('.', dot - 1);
        if (prev != std::string::npos) name = lower.substr(prev + 1, dot - prev - 1);
    }
    if (name.find("clayhardenedstained") != std::string::npos) return "clay";
    if (name == "clayhardened" || name == "hardenedclay") return "white";
    return name;
}

static bool LegacyPixelPartyIsClayDesc(const std::string& unloc) {
    std::string lower = unloc;
    for (char& c : lower) c = (char)tolower((unsigned char)c);
    return lower.find("terracotta") != std::string::npos
        || lower.find("clayhardenedstained") != std::string::npos
        || lower.find("hardenedclay") != std::string::npos
        || lower.find("stained_hardened_clay") != std::string::npos;
}

static bool LegacyGetBlockAtIsNonAir(JNIEnv* env, int bx, int by, int bz, jobject* outBlock, std::string* outUnloc) {
    if (outBlock) *outBlock = nullptr;
    if (outUnloc) outUnloc->clear();
    if (!IsSolidBlockAt(env, (double)bx + 0.5, (double)by + 0.5, (double)bz + 0.5)) return false;

    if (!env || !g_mcInstance || !g_theWorldField || !g_blockPosClass || !g_blockPosIntCtor) return false;
    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return false; }
    if (!world) return false;

    if (!g_worldGetBlockStateMethod) {
        jclass worldCls = env->GetObjectClass(world);
        if (worldCls) {
            g_worldGetBlockStateMethod = env->GetMethodID(worldCls, "getBlockState", "(Lnet/minecraft/util/BlockPos;)Lnet/minecraft/block/state/IBlockState;");
            if (!g_worldGetBlockStateMethod) {
                env->ExceptionClear();
                g_worldGetBlockStateMethod = env->GetMethodID(worldCls, "func_180495_p", "(Lnet/minecraft/util/BlockPos;)Lnet/minecraft/block/state/IBlockState;");
            }
            if (!g_worldGetBlockStateMethod) env->ExceptionClear();
            env->DeleteLocalRef(worldCls);
        }
    }
    if (!g_worldGetBlockStateMethod) { env->DeleteLocalRef(world); return false; }

    jobject pos = env->NewObject(g_blockPosClass, g_blockPosIntCtor, (jint)bx, (jint)by, (jint)bz);
    if (env->ExceptionCheck() || !pos) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        env->DeleteLocalRef(world);
        return false;
    }
    jobject state = env->CallObjectMethod(world, g_worldGetBlockStateMethod, pos);
    env->DeleteLocalRef(pos);
    env->DeleteLocalRef(world);
    if (env->ExceptionCheck() || !state) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }

    if (!g_blockStateGetBlockMethod) {
        jclass stateCls = env->GetObjectClass(state);
        if (stateCls) {
            g_blockStateGetBlockMethod = env->GetMethodID(stateCls, "getBlock", "()Lnet/minecraft/block/Block;");
            if (!g_blockStateGetBlockMethod) {
                env->ExceptionClear();
                g_blockStateGetBlockMethod = env->GetMethodID(stateCls, "func_177230_c", "()Lnet/minecraft/block/Block;");
            }
            if (!g_blockStateGetBlockMethod) env->ExceptionClear();
            env->DeleteLocalRef(stateCls);
        }
    }
    if (!g_blockStateGetBlockMethod) { env->DeleteLocalRef(state); return false; }

    jobject block = env->CallObjectMethod(state, g_blockStateGetBlockMethod);
    env->DeleteLocalRef(state);
    if (env->ExceptionCheck() || !block) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }
    if (outUnloc) *outUnloc = LegacyGetBlockUnlocalizedName(env, block);
    if (outBlock) *outBlock = block;
    else env->DeleteLocalRef(block);
    return true;
}

static bool LegacyResolveHeldClayBlock(JNIEnv* env, jobject player,
    jobject* outBlock, std::string* outUnloc, std::string* outLabel) {
    if (outBlock) *outBlock = nullptr;
    if (outUnloc) outUnloc->clear();
    if (outLabel) outLabel->clear();
    if (!env || !player || !g_getHeldItemMethod) return false;

    if (!g_itemBlockClass) return false;
    if (!g_blockItemGetBlockMethod) {
        jclass biCls = g_itemBlockClass;
        g_blockItemGetBlockMethod = env->GetMethodID(biCls, "getBlock", "()Lnet/minecraft/block/Block;");
        if (!g_blockItemGetBlockMethod) {
            env->ExceptionClear();
            g_blockItemGetBlockMethod = env->GetMethodID(biCls, "func_179223_d", "()Lnet/minecraft/block/Block;");
        }
        if (!g_blockItemGetBlockMethod) {
            if (!g_loggedPixelPartyResolveFail) {
                g_loggedPixelPartyResolveFail = true;
                Log("PixelParty legacy: BlockItem.getBlock unresolved");
            }
            env->ExceptionClear();
            return false;
        }
    }

    jobject stack = env->CallObjectMethod(player, g_getHeldItemMethod);
    if (env->ExceptionCheck() || !stack) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }
    jclass stCls = env->GetObjectClass(stack);
    jmethodID getItem = stCls ? env->GetMethodID(stCls, "getItem", "()Lnet/minecraft/item/Item;") : nullptr;
    if (!getItem && stCls) {
        env->ExceptionClear();
        getItem = env->GetMethodID(stCls, "func_77973_b", "()Lnet/minecraft/item/Item;");
    }
    if (stCls) env->DeleteLocalRef(stCls);
    if (!getItem) { env->DeleteLocalRef(stack); return false; }

    jobject item = env->CallObjectMethod(stack, getItem);
    env->DeleteLocalRef(stack);
    if (env->ExceptionCheck() || !item) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }
    if (env->IsInstanceOf(item, g_itemBlockClass) != JNI_TRUE) {
        env->DeleteLocalRef(item);
        return false;
    }
    jobject block = env->CallObjectMethod(item, g_blockItemGetBlockMethod);
    env->DeleteLocalRef(item);
    if (env->ExceptionCheck() || !block) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }
    std::string unloc = LegacyGetBlockUnlocalizedName(env, block);
    if (!LegacyPixelPartyIsClayDesc(unloc)) {
        env->DeleteLocalRef(block);
        return false;
    }
    if (outUnloc) *outUnloc = unloc;
    if (outLabel) *outLabel = LegacyPixelPartyFormatColorLabel(unloc);
    if (outBlock) *outBlock = block;
    else env->DeleteLocalRef(block);
    return true;
}

static bool LegacyBlocksMatch(JNIEnv* env, jobject heldBlock, const std::string& heldUnloc,
    jobject worldBlock, const std::string& worldUnloc) {
    if (!env || !heldBlock || !worldBlock) return false;
    if (env->IsSameObject(heldBlock, worldBlock) == JNI_TRUE) return true;
    if (!heldUnloc.empty() && heldUnloc == worldUnloc) return true;
    return false;
}

static void UpdatePixelPartyAssistLegacy(JNIEnv* env, const Config& cfg, const GameState& state) {
    DWORD now = GetTickCount();
    if (now - g_lastPixelPartyUpdateMs < 100) return;
    g_lastPixelPartyUpdateMs = now;

    PixelPartySnap snap;
    snap.active = true;

    if (!env || !g_mapped || !g_mcInstance || state.guiOpen) {
        snap.status = state.guiOpen ? "Close menu" : "Waiting for world...";
        LockGuard lk(g_pixelPartyMutex);
        g_pixelPartySnap = snap;
        return;
    }

    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (!player) {
        snap.status = "No player";
        LockGuard lk(g_pixelPartyMutex);
        g_pixelPartySnap = snap;
        return;
    }

    double px = state.posX, py = state.posY, pz = state.posZ;
    jobject heldBlock = nullptr;
    std::string heldUnloc;
    snap.holdingValid = LegacyResolveHeldClayBlock(env, player, &heldBlock, &heldUnloc, &snap.colorLabel);
    env->DeleteLocalRef(player);

    if (!snap.holdingValid || !heldBlock) {
        if (heldBlock) env->DeleteLocalRef(heldBlock);
        snap.status = "Hold matching terracotta";
        LockGuard lk(g_pixelPartyMutex);
        g_pixelPartySnap = snap;
        return;
    }

    int radius = cfg.pixelPartyScanRadius;
    if (radius < 8) radius = 8;
    if (radius > 48) radius = 48;

    // Anchor the scan to the platform floor instead of the live player Y. While the
    // player jumps (auto-walk pulses jump to gain speed) py rises a full block, which
    // previously pushed the colored floor out of the scanned band and made the target
    // flicker off every jump. Re-lock the platform only when the player is genuinely
    // resting on a floor (a solid block directly beneath the feet), and keep scanning
    // that locked level while airborne.
    //
    // NOTE: the earlier heuristic re-locked whenever the feet fraction was near zero
    // (pyFrac < 0.05). That also fired mid-jump every time py crossed an integer Y
    // (e.g. passing through the apex), re-anchoring the band one level too high and
    // dropping the real floor for a split second — the residual stutter being fixed here.
    int feetY = (int)std::floor(py);
    bool grounded = IsSolidBlockAt(env, px, py - 0.1, pz);
    if (grounded) {
        g_pixelPartyPlatformY = feetY - 1;
        g_pixelPartyPlatformYValid = true;
    }
    int platformY;
    if (g_pixelPartyPlatformYValid &&
        (feetY - g_pixelPartyPlatformY) >= 0 &&
        (feetY - g_pixelPartyPlatformY) <= 4) {
        platformY = g_pixelPartyPlatformY;
    } else {
        platformY = feetY - 1;
        g_pixelPartyPlatformY = platformY;
        g_pixelPartyPlatformYValid = true;
    }
    int floorYs[2] = { platformY, platformY + 1 };
    int pxBlock = (int)std::floor(px);
    int pzBlock = (int)std::floor(pz);

    double bestDist = -1.0;
    int bestBx = 0, bestBy = platformY, bestBz = 0;

    for (int yi = 0; yi < 2; yi++) {
        int scanY = floorYs[yi];
        for (int dx = -radius; dx <= radius; dx++) {
            for (int dz = -radius; dz <= radius; dz++) {
                int bx = pxBlock + dx;
                int bz = pzBlock + dz;
                jobject worldBlock = nullptr;
                std::string worldUnloc;
                if (!LegacyGetBlockAtIsNonAir(env, bx, scanY, bz, &worldBlock, &worldUnloc)) continue;
                if (!LegacyBlocksMatch(env, heldBlock, heldUnloc, worldBlock, worldUnloc)) {
                    env->DeleteLocalRef(worldBlock);
                    continue;
                }
                double cx = bx + 0.5 - px;
                double cz = bz + 0.5 - pz;
                double hDist = std::sqrt(cx * cx + cz * cz);
                env->DeleteLocalRef(worldBlock);
                if (bestDist < 0.0 || hDist < bestDist) {
                    bestDist = hDist;
                    bestBx = bx;
                    bestBy = scanY;
                    bestBz = bz;
                }
            }
        }
    }

    env->DeleteLocalRef(heldBlock);

    if (bestDist < 0.0) {
        snap.status = "No match in range";
        LockGuard lk(g_pixelPartyMutex);
        g_pixelPartySnap = snap;
        return;
    }

    snap.targetFound = true;
    snap.tx = bestBx + 0.5;
    snap.ty = bestBy + 0.5;
    snap.tz = bestBz + 0.5;
    snap.dist = bestDist;
    double toX = snap.tx - px;
    double toZ = snap.tz - pz;
    snap.targetYaw = (float)(std::atan2(-toX, toZ) * 57.29577951308232);
    snap.status = "";

    LockGuard lk(g_pixelPartyMutex);
    g_pixelPartySnap = snap;
}

static bool IsSpeedBridgeEdgeUnsupported(JNIEnv* env, const Config& cfg, const GameState& state) {
    if (g_speedBridgeDirX == 0 && g_speedBridgeDirZ == 0) return false;
    double probe = SpeedBridgeSupportProbeDistance(cfg);
    double sx = state.posX + (double)g_speedBridgeDirX * probe;
    double sz = state.posZ + (double)g_speedBridgeDirZ * probe;
    double sy = state.posY - 0.05;
    return !IsSolidBlockAt(env, sx, sy, sz);
}

static void UpdateSpeedBridge(JNIEnv* env, const Config& cfg, const GameState& state) {
    bool shouldRun =
        cfg.speedBridge &&
        state.mapped &&
        !state.guiOpen &&
        (!cfg.speedBridgeBlockOnly || state.holdingBlock) &&
        (!cfg.speedBridgeLookingDownOnly || state.pitch >= 60.0f);

    if (shouldRun && cfg.speedBridgeHoldingShiftOnly && !IsConfiguredSneakPhysicallyDown(env)) {
        shouldRun = false;
    }

    if (!shouldRun) {
        ResetSpeedBridgeMovementTracking();
        ReleaseSpeedBridgeSneak(env);
        return;
    }

    UpdateSpeedBridgeDirection(state);
    bool shouldSneak = IsSpeedBridgeEdgeUnsupported(env, cfg, state);
    SetSpeedBridgeSneak(env, shouldSneak);
}

static unsigned int ChestStealerRand() {
    if (g_chestStealerRng == 0xA0C0123u) {
        g_chestStealerRng ^= (unsigned int)GetTickCount();
        if (g_chestStealerRng == 0) g_chestStealerRng = 0x6D2B79F5u;
    }
    unsigned int x = g_chestStealerRng;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    g_chestStealerRng = x ? x : 0x6D2B79F5u;
    return g_chestStealerRng;
}

static int ChestStealerRandRange(int minVal, int maxVal) {
    if (maxVal <= minVal) return minVal;
    unsigned int span = (unsigned int)(maxVal - minVal + 1);
    return minVal + (int)(ChestStealerRand() % span);
}

static void ShuffleChestStealerSlots(std::vector<int>& slots) {
    for (int i = (int)slots.size() - 1; i > 0; --i) {
        int j = ChestStealerRandRange(0, i);
        std::swap(slots[i], slots[j]);
    }
}

static int NextChestStealerDelayMs(const Config& cfg) {
    int baseDelay = (std::max)(50, (std::min)(500, cfg.chestStealerDelayMs));
    baseDelay = (std::max)(180, baseDelay);
    int variance = (std::max)(70, baseDelay / 2);
    int delay = baseDelay + ChestStealerRandRange(-variance, variance);
    return (std::max)(160, (std::min)(900, delay));
}

static void ResetChestStealerRuntime() {
    g_chestStealerNextClickMs = 0;
    g_chestStealerWindowId = -1;
    g_chestStealerLastSlotCount = -1;
    g_chestStealerWindowOpenedMs = 0;
    g_chestStealerWindowCompleted = false;
    g_chestStealerSlots.clear();
}

static void LogChestStealerMappingMissing(const char* detail) {
    DWORD now = GetTickCount();
    if (now - g_lastChestStealerMappingLogMs < 5000) return;
    g_lastChestStealerMappingLogMs = now;
    Log(std::string("ChestStealer JNI unresolved: ") + (detail ? detail : "unknown"));
}

static std::string ChestStealerLower(std::string s) {
    for (char& ch : s) ch = (char)std::tolower((unsigned char)ch);
    return s;
}

// Module-owned, client-side only: Minecraft 1.8.9 applies a short click lockout
// after a MovingObjectPosition MISS. Reset only that counter while the current
// crosshair result remains a MISS; do not invoke combat methods or send packets.
static void UpdateHitDelayFixLegacy(JNIEnv* env, const Config& cfg, const GameState& state) {
    if (!env || !cfg.hitDelayFixEnabled || !state.mapped || state.guiOpen
        || !g_mcInstance || !g_leftClickCounterField || !g_objectMouseOverField
        || !g_typeOfHitField || !g_enumNameMethod) {
        return;
    }

    jobject mop = env->GetObjectField(g_mcInstance, g_objectMouseOverField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); mop = nullptr; }
    if (!mop) return;

    bool isMiss = false;
    jobject typeOfHit = env->GetObjectField(mop, g_typeOfHitField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); typeOfHit = nullptr; }
    if (typeOfHit) {
        jstring name = (jstring)env->CallObjectMethod(typeOfHit, g_enumNameMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); name = nullptr; }
        if (name) {
            const char* chars = env->GetStringUTFChars(name, nullptr);
            if (chars) {
                isMiss = strcmp(chars, "MISS") == 0;
                env->ReleaseStringUTFChars(name, chars);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
            env->DeleteLocalRef(name);
        }
        env->DeleteLocalRef(typeOfHit);
    }
    env->DeleteLocalRef(mop);

    if (!isMiss) return;

    jint delay = env->GetIntField(g_mcInstance, g_leftClickCounterField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return; }
    if (delay != 0) {
        env->SetIntField(g_mcInstance, g_leftClickCounterField, 0);
        if (env->ExceptionCheck()) env->ExceptionClear();
    }
}

static void LogChestStealerSkippedMenu(const std::string& title) {
    DWORD now = GetTickCount();
    if (now - g_lastChestStealerSkipLogMs < 5000) return;
    g_lastChestStealerSkipLogMs = now;
    Log("ChestStealer skipped non-physical GuiChest title=\"" + title + "\"");
}

static bool ResolveChestStealerMappings(JNIEnv* env, jobject currentScreen) {
    if (!env || !g_mcInstance || !g_mcClass) return false;
    jobject gcl = EnsureGameClassLoader(env);

    if (!g_guiChestClass && gcl) {
        jclass cls = LoadClassWithLoader(env, gcl, "net.minecraft.client.gui.inventory.GuiChest");
        if (env->ExceptionCheck()) { env->ExceptionClear(); cls = nullptr; }
        if (cls) {
            g_guiChestClass = (jclass)env->NewGlobalRef(cls);
            env->DeleteLocalRef(cls);
        }
    }

    if (!g_playerControllerField) {
        g_playerControllerField = env->GetFieldID(g_mcClass, "playerController", "Lnet/minecraft/client/multiplayer/PlayerControllerMP;");
        if (env->ExceptionCheck() || !g_playerControllerField) {
            env->ExceptionClear();
            g_playerControllerField = env->GetFieldID(g_mcClass, "field_71442_b", "Lnet/minecraft/client/multiplayer/PlayerControllerMP;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_playerControllerField = nullptr; }
        }
    }

    jobject controller = g_playerControllerField ? env->GetObjectField(g_mcInstance, g_playerControllerField) : nullptr;
    if (env->ExceptionCheck()) { env->ExceptionClear(); controller = nullptr; }
    if (controller && !g_windowClickMethod) {
        jclass controllerClass = env->GetObjectClass(controller);
        if (controllerClass) {
            g_windowClickMethod = env->GetMethodID(controllerClass, "windowClick", "(IIIILnet/minecraft/entity/player/EntityPlayer;)Lnet/minecraft/item/ItemStack;");
            if (env->ExceptionCheck() || !g_windowClickMethod) {
                env->ExceptionClear();
                g_windowClickMethod = env->GetMethodID(controllerClass, "func_78753_a", "(IIIILnet/minecraft/entity/player/EntityPlayer;)Lnet/minecraft/item/ItemStack;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_windowClickMethod = nullptr; }
            }
            env->DeleteLocalRef(controllerClass);
        }
    }
    if (controller) env->DeleteLocalRef(controller);

    if (currentScreen && (!g_guiContainerInventorySlotsField || !g_guiLeftField || !g_guiTopField || !g_guiWidthField || !g_guiHeightField)) {
        jclass screenClass = env->GetObjectClass(currentScreen);
        if (screenClass) {
            g_guiContainerInventorySlotsField = env->GetFieldID(screenClass, "inventorySlots", "Lnet/minecraft/inventory/Container;");
            if (env->ExceptionCheck() || !g_guiContainerInventorySlotsField) {
                env->ExceptionClear();
                g_guiContainerInventorySlotsField = env->GetFieldID(screenClass, "field_147002_h", "Lnet/minecraft/inventory/Container;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_guiContainerInventorySlotsField = nullptr; }
            }
            if (!g_guiLeftField) {
                g_guiLeftField = env->GetFieldID(screenClass, "guiLeft", "I");
                if (env->ExceptionCheck() || !g_guiLeftField) {
                    env->ExceptionClear();
                    g_guiLeftField = env->GetFieldID(screenClass, "field_147003_i", "I");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_guiLeftField = nullptr; }
                }
            }
            if (!g_guiTopField) {
                g_guiTopField = env->GetFieldID(screenClass, "guiTop", "I");
                if (env->ExceptionCheck() || !g_guiTopField) {
                    env->ExceptionClear();
                    g_guiTopField = env->GetFieldID(screenClass, "field_147009_r", "I");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_guiTopField = nullptr; }
                }
            }
            if (!g_guiWidthField) {
                g_guiWidthField = env->GetFieldID(screenClass, "width", "I");
                if (env->ExceptionCheck() || !g_guiWidthField) {
                    env->ExceptionClear();
                    g_guiWidthField = env->GetFieldID(screenClass, "field_146294_l", "I");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_guiWidthField = nullptr; }
                }
            }
            if (!g_guiHeightField) {
                g_guiHeightField = env->GetFieldID(screenClass, "height", "I");
                if (env->ExceptionCheck() || !g_guiHeightField) {
                    env->ExceptionClear();
                    g_guiHeightField = env->GetFieldID(screenClass, "field_146295_m", "I");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_guiHeightField = nullptr; }
                }
            }
            env->DeleteLocalRef(screenClass);
        }
    }

    if (currentScreen && g_guiContainerInventorySlotsField) {
        jobject container = env->GetObjectField(currentScreen, g_guiContainerInventorySlotsField);
        if (env->ExceptionCheck()) { env->ExceptionClear(); container = nullptr; }
        if (container) {
            jclass containerClass = env->GetObjectClass(container);
            if (containerClass) {
                if (!g_containerWindowIdField) {
                    g_containerWindowIdField = env->GetFieldID(containerClass, "windowId", "I");
                    if (env->ExceptionCheck() || !g_containerWindowIdField) {
                        env->ExceptionClear();
                        g_containerWindowIdField = env->GetFieldID(containerClass, "field_75152_c", "I");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_containerWindowIdField = nullptr; }
                    }
                }
                if (!g_containerInventorySlotsField) {
                    g_containerInventorySlotsField = env->GetFieldID(containerClass, "inventorySlots", "Ljava/util/List;");
                    if (env->ExceptionCheck() || !g_containerInventorySlotsField) {
                        env->ExceptionClear();
                        g_containerInventorySlotsField = env->GetFieldID(containerClass, "field_75151_b", "Ljava/util/List;");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_containerInventorySlotsField = nullptr; }
                    }
                }
                env->DeleteLocalRef(containerClass);
            }

            if (g_containerInventorySlotsField && g_listSizeMethod && g_listGetMethod &&
                (!g_slotGetHasStackMethod || !g_slotSlotNumberField || !g_slotXDisplayPositionField || !g_slotYDisplayPositionField)) {
                jobject slotsList = env->GetObjectField(container, g_containerInventorySlotsField);
                if (env->ExceptionCheck()) { env->ExceptionClear(); slotsList = nullptr; }
                if (slotsList) {
                    int size = env->CallIntMethod(slotsList, g_listSizeMethod);
                    if (env->ExceptionCheck()) { env->ExceptionClear(); size = 0; }
                    if (size > 0) {
                        jobject slot = env->CallObjectMethod(slotsList, g_listGetMethod, 0);
                        if (env->ExceptionCheck()) { env->ExceptionClear(); slot = nullptr; }
                        if (slot) {
                            jclass slotClass = env->GetObjectClass(slot);
                            if (slotClass) {
                                if (!g_slotGetHasStackMethod) {
                                    g_slotGetHasStackMethod = env->GetMethodID(slotClass, "getHasStack", "()Z");
                                    if (env->ExceptionCheck() || !g_slotGetHasStackMethod) {
                                        env->ExceptionClear();
                                        g_slotGetHasStackMethod = env->GetMethodID(slotClass, "func_75216_d", "()Z");
                                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_slotGetHasStackMethod = nullptr; }
                                    }
                                }
                                if (!g_slotSlotNumberField) {
                                    g_slotSlotNumberField = env->GetFieldID(slotClass, "slotNumber", "I");
                                    if (env->ExceptionCheck() || !g_slotSlotNumberField) {
                                        env->ExceptionClear();
                                        g_slotSlotNumberField = env->GetFieldID(slotClass, "field_75222_d", "I");
                                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_slotSlotNumberField = nullptr; }
                                    }
                                }
                                if (!g_slotXDisplayPositionField) {
                                    g_slotXDisplayPositionField = env->GetFieldID(slotClass, "xDisplayPosition", "I");
                                    if (env->ExceptionCheck() || !g_slotXDisplayPositionField) {
                                        env->ExceptionClear();
                                        g_slotXDisplayPositionField = env->GetFieldID(slotClass, "field_75223_e", "I");
                                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_slotXDisplayPositionField = nullptr; }
                                    }
                                }
                                if (!g_slotYDisplayPositionField) {
                                    g_slotYDisplayPositionField = env->GetFieldID(slotClass, "yDisplayPosition", "I");
                                    if (env->ExceptionCheck() || !g_slotYDisplayPositionField) {
                                        env->ExceptionClear();
                                        g_slotYDisplayPositionField = env->GetFieldID(slotClass, "field_75221_f", "I");
                                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_slotYDisplayPositionField = nullptr; }
                                    }
                                }
                                env->DeleteLocalRef(slotClass);
                            }
                            env->DeleteLocalRef(slot);
                        }
                    }
                    env->DeleteLocalRef(slotsList);
                }
            }
            env->DeleteLocalRef(container);
        }
    }

    return g_guiContainerInventorySlotsField && g_containerWindowIdField &&
        g_containerInventorySlotsField && g_slotGetHasStackMethod &&
        g_slotSlotNumberField && g_slotXDisplayPositionField && g_slotYDisplayPositionField &&
        g_guiLeftField && g_guiTopField && g_guiWidthField && g_guiHeightField &&
        g_listSizeMethod && g_listGetMethod;
}

static bool IsLegacyChestScreen(JNIEnv* env, jobject currentScreen) {
    if (!env || !currentScreen) return false;
    if (g_guiChestClass && env->IsInstanceOf(currentScreen, g_guiChestClass)) return true;
    jclass cls = env->GetObjectClass(currentScreen);
    if (!cls) return false;
    std::string name = GetClassNameFromClass(env, cls);
    env->DeleteLocalRef(cls);
    if (env->ExceptionCheck()) env->ExceptionClear();
    return name.find("GuiChest") != std::string::npos;
}

static bool EnsureLegacyChatTextMethod(JNIEnv* env) {
    if (!env) return false;
    if (g_chatComponentGetTextMethod) return true;
    jobject gcl = EnsureGameClassLoader(env);
    jclass chatCompClass = gcl ? LoadClassWithLoader(env, gcl, "net.minecraft.util.IChatComponent") : nullptr;
    if (env->ExceptionCheck()) { env->ExceptionClear(); chatCompClass = nullptr; }
    if (!chatCompClass) {
        chatCompClass = env->FindClass("net/minecraft/util/IChatComponent");
        if (env->ExceptionCheck()) { env->ExceptionClear(); chatCompClass = nullptr; }
    }
    if (!chatCompClass) return false;

    g_chatComponentGetTextMethod = env->GetMethodID(chatCompClass, "getUnformattedText", "()Ljava/lang/String;");
    if (env->ExceptionCheck() || !g_chatComponentGetTextMethod) {
        env->ExceptionClear();
        g_chatComponentGetTextMethod = env->GetMethodID(chatCompClass, "func_150260_c", "()Ljava/lang/String;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); g_chatComponentGetTextMethod = nullptr; }
    }
    env->DeleteLocalRef(chatCompClass);
    return g_chatComponentGetTextMethod != nullptr;
}

static std::string ReadLegacyChatText(JNIEnv* env, jobject chatComponent) {
    if (!env || !chatComponent || !EnsureLegacyChatTextMethod(env)) return "";
    jstring js = (jstring)env->CallObjectMethod(chatComponent, g_chatComponentGetTextMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return ""; }
    std::string text = Utf8FromJStringLegacy(env, js);
    if (js) env->DeleteLocalRef(js);
    return text;
}

static std::string ReadInventoryDisplayName(JNIEnv* env, jobject inventory) {
    if (!env || !inventory) return "";
    jclass invClass = env->GetObjectClass(inventory);
    if (!invClass) return "";

    jmethodID displayName = env->GetMethodID(invClass, "getDisplayName", "()Lnet/minecraft/util/IChatComponent;");
    if (env->ExceptionCheck() || !displayName) {
        env->ExceptionClear();
        displayName = env->GetMethodID(invClass, "func_145748_c_", "()Lnet/minecraft/util/IChatComponent;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); displayName = nullptr; }
    }
    env->DeleteLocalRef(invClass);
    if (!displayName) return "";

    jobject textObj = env->CallObjectMethod(inventory, displayName);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return ""; }
    std::string title = ReadLegacyChatText(env, textObj);
    if (textObj) env->DeleteLocalRef(textObj);
    return title;
}

static std::string GetLegacyChestScreenTitle(JNIEnv* env, jobject currentScreen) {
    if (!env || !currentScreen) return "";
    jclass screenClass = env->GetObjectClass(currentScreen);
    if (!screenClass) return "";

    jclass classClass = env->FindClass("java/lang/Class");
    jclass fieldClass = env->FindClass("java/lang/reflect/Field");
    jmethodID getFields = classClass ? env->GetMethodID(classClass, "getDeclaredFields", "()[Ljava/lang/reflect/Field;") : nullptr;
    jmethodID getType = fieldClass ? env->GetMethodID(fieldClass, "getType", "()Ljava/lang/Class;") : nullptr;
    std::string bestTitle;

    if (getFields && getType) {
        jobjectArray fields = (jobjectArray)env->CallObjectMethod(screenClass, getFields);
        if (env->ExceptionCheck()) { env->ExceptionClear(); fields = nullptr; }
        if (fields) {
            jsize count = env->GetArrayLength(fields);
            for (int i = 0; i < count; ++i) {
                jobject field = env->GetObjectArrayElement(fields, i);
                if (!field) continue;
                jclass fieldType = (jclass)env->CallObjectMethod(field, getType);
                if (env->ExceptionCheck()) { env->ExceptionClear(); fieldType = nullptr; }
                std::string typeName = fieldType ? GetClassNameFromClass(env, fieldType) : "";
                if (fieldType) env->DeleteLocalRef(fieldType);

                if (typeName.find("IInventory") != std::string::npos) {
                    jfieldID fid = env->FromReflectedField(field);
                    if (fid) {
                        jobject inv = env->GetObjectField(currentScreen, fid);
                        if (env->ExceptionCheck()) { env->ExceptionClear(); inv = nullptr; }
                        if (inv) {
                            std::string title = ReadInventoryDisplayName(env, inv);
                            env->DeleteLocalRef(inv);
                            std::string lower = ChestStealerLower(title);
                            if (!title.empty() && lower != "inventory") {
                                bestTitle = title;
                            } else if (bestTitle.empty() && !title.empty()) {
                                bestTitle = title;
                            }
                        }
                    }
                }
                env->DeleteLocalRef(field);
            }
            env->DeleteLocalRef(fields);
        }
    }

    if (fieldClass) env->DeleteLocalRef(fieldClass);
    if (classClass) env->DeleteLocalRef(classClass);
    env->DeleteLocalRef(screenClass);
    if (env->ExceptionCheck()) env->ExceptionClear();
    return bestTitle;
}

static bool IsChestTileEntityLegacy(JNIEnv* env, jobject te) {
    if (!env || !te) return false;
    if (g_tileEntityChestClass && env->IsInstanceOf(te, g_tileEntityChestClass)) return true;
    if (g_tileEntityEnderChestClass && env->IsInstanceOf(te, g_tileEntityEnderChestClass)) return true;
    jclass teClass = env->GetObjectClass(te);
    if (!teClass) return false;
    std::string clsName = GetClassNameFromClass(env, teClass);
    env->DeleteLocalRef(teClass);
    if (env->ExceptionCheck()) env->ExceptionClear();
    return clsName.find("Chest") != std::string::npos;
}

static bool HasPhysicalChestNearPlayer(JNIEnv* env, jobject player) {
    if (!env || !g_mcInstance || !player) return false;
    TryResolveWorldMappings(env);
    TryResolveChestEspMappings(env);

    if (!g_theWorldField || !g_loadedTileEntityListField || !g_tileEntityPosField ||
        !g_blockPosGetX || !g_blockPosGetY || !g_blockPosGetZ ||
        !g_posXField || !g_posYField || !g_posZField ||
        !g_listSizeMethod || !g_listGetMethod) {
        LogChestStealerMappingMissing("physical chest validation mappings");
        return false;
    }

    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); world = nullptr; }
    if (!world) return false;
    jobject tileList = env->GetObjectField(world, g_loadedTileEntityListField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); tileList = nullptr; }
    env->DeleteLocalRef(world);
    if (!tileList) return false;

    double px = env->GetDoubleField(player, g_posXField);
    double py = env->GetDoubleField(player, g_posYField);
    double pz = env->GetDoubleField(player, g_posZField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); env->DeleteLocalRef(tileList); return false; }

    int size = env->CallIntMethod(tileList, g_listSizeMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); env->DeleteLocalRef(tileList); return false; }

    bool found = false;
    const double maxDistSq = 6.5 * 6.5;
    for (int i = 0; i < size && !found; ++i) {
        jobject te = env->CallObjectMethod(tileList, g_listGetMethod, i);
        if (env->ExceptionCheck()) { env->ExceptionClear(); break; }
        if (!te) continue;

        if (IsChestTileEntityLegacy(env, te)) {
            jobject posObj = env->GetObjectField(te, g_tileEntityPosField);
            if (env->ExceptionCheck()) { env->ExceptionClear(); posObj = nullptr; }
            if (posObj) {
                int bx = env->CallIntMethod(posObj, g_blockPosGetX);
                int by = env->CallIntMethod(posObj, g_blockPosGetY);
                int bz = env->CallIntMethod(posObj, g_blockPosGetZ);
                env->DeleteLocalRef(posObj);
                if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                } else {
                    double dx = ((double)bx + 0.5) - px;
                    double dy = ((double)by + 0.5) - py;
                    double dz = ((double)bz + 0.5) - pz;
                    found = (dx * dx + dy * dy + dz * dz) <= maxDistSq;
                }
            }
        }
        env->DeleteLocalRef(te);
    }

    env->DeleteLocalRef(tileList);
    return found;
}

static std::string BuildChestStealerStateJson(JNIEnv* env, bool enabled) {
    if (!enabled || !env || !g_mcInstance || !g_currentScreenField || !g_thePlayerField) return "null";

    if (env->PushLocalFrame(128) < 0) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return "null";
    }

    jobject currentScreen = env->GetObjectField(g_mcInstance, g_currentScreenField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); currentScreen = nullptr; }
    if (!currentScreen || !IsLegacyChestScreen(env, currentScreen)) {
        env->PopLocalFrame(nullptr);
        return "null";
    }

    if (!ResolveChestStealerMappings(env, currentScreen)) {
        LogChestStealerMappingMissing("container/slot geometry mappings");
        env->PopLocalFrame(nullptr);
        return "null";
    }

    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    jobject container = env->GetObjectField(currentScreen, g_guiContainerInventorySlotsField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); player = nullptr; container = nullptr; }
    if (!player || !container) {
        env->PopLocalFrame(nullptr);
        return "null";
    }

    int windowId = env->GetIntField(container, g_containerWindowIdField);
    int guiLeft = env->GetIntField(currentScreen, g_guiLeftField);
    int guiTop = env->GetIntField(currentScreen, g_guiTopField);
    int screenWidth = env->GetIntField(currentScreen, g_guiWidthField);
    int screenHeight = env->GetIntField(currentScreen, g_guiHeightField);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        env->PopLocalFrame(nullptr);
        return "null";
    }

    bool physical = HasPhysicalChestNearPlayer(env, player);
    if (!physical) {
        std::string title = GetLegacyChestScreenTitle(env, currentScreen);
        if (title.empty()) title = "unknown";
        LogChestStealerSkippedMenu(title);
        std::ostringstream skipped;
        skipped << "{\"ready\":false,\"physical\":false,\"windowId\":" << windowId
                << ",\"screenWidth\":" << screenWidth
                << ",\"screenHeight\":" << screenHeight
                << ",\"slots\":[]}";
        env->PopLocalFrame(nullptr);
        return skipped.str();
    }

    jobject slotsList = env->GetObjectField(container, g_containerInventorySlotsField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); slotsList = nullptr; }
    if (!slotsList) {
        env->PopLocalFrame(nullptr);
        return "null";
    }

    int size = env->CallIntMethod(slotsList, g_listSizeMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); size = 0; }
    int chestSlotCount = size - 36;
    if (chestSlotCount <= 0 || screenWidth <= 0 || screenHeight <= 0) {
        env->PopLocalFrame(nullptr);
        return "null";
    }

    std::ostringstream slotsJson;
    int count = 0;
    for (int i = 0; i < chestSlotCount; ++i) {
        jobject slot = env->CallObjectMethod(slotsList, g_listGetMethod, i);
        if (env->ExceptionCheck()) { env->ExceptionClear(); slot = nullptr; }
        if (!slot) continue;

        bool hasStack = env->CallBooleanMethod(slot, g_slotGetHasStackMethod) != JNI_FALSE;
        if (env->ExceptionCheck()) { env->ExceptionClear(); hasStack = false; }
        if (hasStack) {
            int slotNumber = env->GetIntField(slot, g_slotSlotNumberField);
            int slotX = env->GetIntField(slot, g_slotXDisplayPositionField);
            int slotY = env->GetIntField(slot, g_slotYDisplayPositionField);
            if (env->ExceptionCheck()) {
                env->ExceptionClear();
            } else {
                if (count > 0) slotsJson << ",";
                slotsJson << "{\"index\":" << i
                          << ",\"slotNumber\":" << slotNumber
                          << ",\"x\":" << (guiLeft + slotX + 8)
                          << ",\"y\":" << (guiTop + slotY + 8)
                          << "}";
                count++;
            }
        }
        env->DeleteLocalRef(slot);
    }

    std::ostringstream out;
    out << "{\"ready\":" << (count > 0 ? "true" : "false")
        << ",\"physical\":true"
        << ",\"windowId\":" << windowId
        << ",\"screenWidth\":" << screenWidth
        << ",\"screenHeight\":" << screenHeight
        << ",\"slots\":[" << slotsJson.str() << "]}";

    env->PopLocalFrame(nullptr);
    return out.str();
}

static void UpdateChestStealer(JNIEnv* env, const Config& cfg) {
    if (!cfg.chestStealer) {
        ResetChestStealerRuntime();
        return;
    }
    if (!env || !g_mcInstance || !g_currentScreenField || !g_thePlayerField) return;

    DWORD now = GetTickCount();
    if (now < g_chestStealerNextClickMs) return;

    if (env->PushLocalFrame(128) < 0) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return;
    }

    jobject currentScreen = env->GetObjectField(g_mcInstance, g_currentScreenField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); currentScreen = nullptr; }
    if (!currentScreen || !IsLegacyChestScreen(env, currentScreen)) {
        ResetChestStealerRuntime();
        env->PopLocalFrame(nullptr);
        return;
    }

    if (!ResolveChestStealerMappings(env, currentScreen)) {
        LogChestStealerMappingMissing("playerController/windowClick/container/slot mappings");
        env->PopLocalFrame(nullptr);
        return;
    }

    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    jobject controller = env->GetObjectField(g_mcInstance, g_playerControllerField);
    jobject container = env->GetObjectField(currentScreen, g_guiContainerInventorySlotsField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); player = nullptr; controller = nullptr; container = nullptr; }
    if (!player || !controller || !container) {
        ResetChestStealerRuntime();
        env->PopLocalFrame(nullptr);
        return;
    }

    int windowId = env->GetIntField(container, g_containerWindowIdField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); ResetChestStealerRuntime(); env->PopLocalFrame(nullptr); return; }

    if (!HasPhysicalChestNearPlayer(env, player)) {
        std::string title = GetLegacyChestScreenTitle(env, currentScreen);
        if (title.empty()) title = "unknown";
        LogChestStealerSkippedMenu(title);
        g_chestStealerWindowId = windowId;
        g_chestStealerWindowCompleted = true;
        g_chestStealerNextClickMs = now + 1000;
        env->PopLocalFrame(nullptr);
        return;
    }

    jobject slotsList = env->GetObjectField(container, g_containerInventorySlotsField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); slotsList = nullptr; }
    if (!slotsList) {
        ResetChestStealerRuntime();
        env->PopLocalFrame(nullptr);
        return;
    }

    int size = env->CallIntMethod(slotsList, g_listSizeMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); size = 0; }
    int chestSlotCount = size - 36;
    if (chestSlotCount <= 0) {
        ResetChestStealerRuntime();
        env->PopLocalFrame(nullptr);
        return;
    }

    bool newWindow = (windowId != g_chestStealerWindowId || chestSlotCount != g_chestStealerLastSlotCount);
    if (newWindow) {
        g_chestStealerWindowId = windowId;
        g_chestStealerLastSlotCount = chestSlotCount;
        g_chestStealerWindowOpenedMs = now;
        g_chestStealerWindowCompleted = false;
        g_chestStealerNextClickMs = now + (DWORD)ChestStealerRandRange(350, 700);
        g_chestStealerSlots.clear();
        env->PopLocalFrame(nullptr);
        return;
    }

    if (g_chestStealerWindowCompleted) {
        env->PopLocalFrame(nullptr);
        return;
    }

    if (g_chestStealerWindowOpenedMs > 0 && now - g_chestStealerWindowOpenedMs < 350) {
        g_chestStealerNextClickMs = g_chestStealerWindowOpenedMs + 350;
        env->PopLocalFrame(nullptr);
        return;
    }

    if (g_chestStealerSlots.empty()) {
        g_chestStealerSlots.clear();
        for (int i = 0; i < chestSlotCount; ++i) {
            jobject slot = env->CallObjectMethod(slotsList, g_listGetMethod, i);
            if (env->ExceptionCheck()) { env->ExceptionClear(); slot = nullptr; }
            if (!slot) continue;

            bool hasStack = env->CallBooleanMethod(slot, g_slotGetHasStackMethod) != JNI_FALSE;
            if (env->ExceptionCheck()) { env->ExceptionClear(); hasStack = false; }
            if (hasStack) {
                int slotNumber = g_slotSlotNumberField ? env->GetIntField(slot, g_slotSlotNumberField) : i;
                if (env->ExceptionCheck()) { env->ExceptionClear(); slotNumber = i; }
                g_chestStealerSlots.push_back(slotNumber);
            }
            env->DeleteLocalRef(slot);
        }
        ShuffleChestStealerSlots(g_chestStealerSlots);
    }

    if (g_chestStealerSlots.empty()) {
        g_chestStealerWindowCompleted = true;
        env->PopLocalFrame(nullptr);
        return;
    }

    int slotNumber = g_chestStealerSlots.back();
    g_chestStealerSlots.pop_back();

    (void)controller;
    (void)slotNumber;
    // Chest Stealer uses C# physical mouse input. Do not emit direct windowClick packets here.
    if (g_chestStealerSlots.empty()) {
        g_chestStealerWindowCompleted = true;
    }
    g_chestStealerNextClickMs = now + (DWORD)NextChestStealerDelayMs(cfg);
    env->PopLocalFrame(nullptr);
}

GameState ReadGameState(JNIEnv* env) {
    GameState s = {};
    static DWORD nextRefreshAt = 0;
    DWORD now = GetTickCount();
    if (now >= nextRefreshAt || !g_mapped) {
        MaybeRefreshMappings(env);
        nextRefreshAt = now + 2000;
    }
    s.mapped = g_mapped;
    if (!g_mapped || !g_mcInstance) return s;

    TryResolveWorldMappings(env);

    bool gtbHelperEnabled = false;
    bool shouldCheckHoldingBlock = false;
    bool chestStealerEnabled = false;
    {
        LockGuard lk(g_configMutex);
        gtbHelperEnabled = g_config.gtbHelper;
        chestStealerEnabled = g_config.chestStealer;
        shouldCheckHoldingBlock =
            (g_config.rightClick && g_config.rightBlockOnly) ||
            (g_config.speedBridge && g_config.speedBridgeBlockOnly);
    }
    TryResolveRenderMappings(env, gtbHelperEnabled);
    TryResolveHoldingBlockMappings(env);

    if (g_currentScreenField) {
        jobject scr = env->GetObjectField(g_mcInstance, g_currentScreenField);
        s.guiOpen = (scr != nullptr);
        if (scr) {
            jclass c = env->GetObjectClass(scr);
            jclass classClass = env->FindClass("java/lang/Class");
            if (classClass) {
                jmethodID m = env->GetMethodID(classClass, "getSimpleName", "()Ljava/lang/String;");
                if (m) {
                    jstring jn = (jstring)env->CallObjectMethod(c, m);
                    if (jn) {
                        const char* cn = env->GetStringUTFChars(jn, nullptr); 
                        s.screenName = cn;
                        env->ReleaseStringUTFChars(jn, cn);
                        env->DeleteLocalRef(jn);
                    }
                }
                env->DeleteLocalRef(classClass);
            }
            env->DeleteLocalRef(c);
            env->DeleteLocalRef(scr);
        } else {
            s.screenName = "none";
        }
    }
    
        jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
        if (player) {
            if (g_getHealthMethod) { s.health = env->CallFloatMethod(player, g_getHealthMethod); if (env->ExceptionCheck()) env->ExceptionClear(); }
            if (g_posXField) s.posX = env->GetDoubleField(player, g_posXField);
            if (g_posYField) s.posY = env->GetDoubleField(player, g_posYField);
            if (g_posZField) s.posZ = env->GetDoubleField(player, g_posZField);
            if (g_rotationPitchField) s.pitch = env->GetFloatField(player, g_rotationPitchField);
            if (g_rotationYawField) s.rotationYaw = env->GetFloatField(player, g_rotationYawField);

            // Check current item
            s.holdingBlock = false;
            if (shouldCheckHoldingBlock && g_inventoryField) {
                jobject inventory = env->GetObjectField(player, g_inventoryField);
                if (inventory) {
                    if (!g_getCurrentItemMethod) {
                        jclass invClass = env->GetObjectClass(inventory);
                        if (invClass) {
                            g_getCurrentItemMethod = env->GetMethodID(invClass, "getCurrentItem", "()Lnet/minecraft/item/ItemStack;");
                            if (!g_getCurrentItemMethod) {
                                env->ExceptionClear();
                                g_getCurrentItemMethod = env->GetMethodID(invClass, "func_70448_g", "()Lnet/minecraft/item/ItemStack;");
                            }
                            if (!g_getCurrentItemMethod) {
                                env->ExceptionClear();
                            }
                            env->DeleteLocalRef(invClass);
                        } else if (env->ExceptionCheck()) {
                            env->ExceptionClear();
                        }
                    }

                    if (g_getCurrentItemMethod) {
                        jobject itemStack = env->CallObjectMethod(inventory, g_getCurrentItemMethod);
                        if (itemStack) {
                            jclass stackClass = env->GetObjectClass(itemStack);
                            jmethodID getItem = env->GetMethodID(stackClass, "getItem", "()Lnet/minecraft/item/Item;");
                            if (!getItem) { env->ExceptionClear(); getItem = env->GetMethodID(stackClass, "func_77973_b", "()Lnet/minecraft/item/Item;"); }
                            if (!getItem) env->ExceptionClear();
                            env->DeleteLocalRef(stackClass);
                            
                            if (getItem) {
                                jobject item = env->CallObjectMethod(itemStack, getItem);
                                if (item) {
                                    bool isBlock = false;

                                    // 1. Instance Check (Fastest)
                                    if (g_itemBlockClass && env->IsInstanceOf(item, g_itemBlockClass)) {
                                        isBlock = true;
                                    }

                                    // 2. Name Check (Fallback & Special Items)
                                    if (!isBlock) {
                                        jclass ic = env->GetObjectClass(item);
                                        std::string inm = GetClassNameFromClass(env, ic);
                                        
                                        // Debug Log for Item Class (throttled)
                                        static int itemLogCtr = 0;
                                        if (itemLogCtr++ % 200 == 0) {
                                            Log("Held Item Class: " + inm);
                                        }

                                        if (inm.find("ItemBlock") != std::string::npos || 
                                            inm.find("Block") != std::string::npos ||
                                            inm.find("ItemReed") != std::string::npos || // Sugar Cane
                                            inm.find("ItemRedstone") != std::string::npos || // Redstone Dust
                                            inm.find("ItemSkull") != std::string::npos) { // Heads
                                            isBlock = true; 
                                        }
                                        env->DeleteLocalRef(ic);
                                    }

                                    s.holdingBlock = isBlock;
                                    env->DeleteLocalRef(item);
                                }
                            }
                            env->DeleteLocalRef(itemStack);
                        }
                    }
                    env->DeleteLocalRef(inventory);
                }
            }
            
            env->DeleteLocalRef(player);
    }

    // Check objectMouseOver
    s.lookingAtBlock = false;
    s.lookingAtEntity = false;
    s.lookingAtEntityLatched = false;
    s.breakingBlock = false;
    s.attackCooldown = 1.0f;
    s.attackCooldownPerTick = 0.08f;
    s.stateMs = (unsigned long long)GetTickCount64();
    s.chestStealerStateJson = BuildChestStealerStateJson(env, chestStealerEnabled);
    { LockGuard lk(g_killAuraUnavailableMutex);
        s.killAuraUnavailableReason = g_killAuraUnavailableReason;
        s.killAuraHasTarget = g_killAuraHasTarget;
        s.killAuraBlocking = g_killAuraBlocking;
    }
    s.actionBar.clear();
    if (g_objectMouseOverField && g_typeOfHitField && g_enumNameMethod) {
        jobject mop = env->GetObjectField(g_mcInstance, g_objectMouseOverField);
        if (mop) {
            jobject typeOfHit = env->GetObjectField(mop, g_typeOfHitField);
            if (typeOfHit) {
                jstring nameStr = (jstring)env->CallObjectMethod(typeOfHit, g_enumNameMethod);
                if (nameStr && !env->ExceptionCheck()) {
                    const char* nameChars = env->GetStringUTFChars(nameStr, nullptr);
                    if (nameChars) {
                        if (strcmp(nameChars, "BLOCK") == 0) {
                            s.lookingAtBlock = true;
                            bool lmbDown = (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0;
                            s.breakingBlock = lmbDown;
                        } else if (strcmp(nameChars, "ENTITY") == 0) {
                            s.lookingAtEntity = true;
                            s.lookingAtEntityLatched = true;
                            g_lastEntitySeenMs = s.stateMs;
                        }
                        env->ReleaseStringUTFChars(nameStr, nameChars);
                    }
                    env->DeleteLocalRef(nameStr);
                }
                if (env->ExceptionCheck()) env->ExceptionClear();
                env->DeleteLocalRef(typeOfHit);
            }
            env->DeleteLocalRef(mop);
        }
    }

    if (!s.lookingAtEntity) {
        unsigned long long nowMs = s.stateMs;
        unsigned long long delta = nowMs - g_lastEntitySeenMs;
        if (delta <= 120ULL) {
            s.lookingAtEntityLatched = true;
        }
    }

    // Read action-bar overlay text (used by GTB helper on C# side).
    if (gtbHelperEnabled && g_ingameGuiField && !g_actionBarFields.empty()) {
        int bestActionBarScore = -1;
        auto tryUseActionBarCandidate = [&](const std::string& cand) {
            if (cand.empty()) return;
            int score = (int)cand.size();
            if (cand.find('_') != std::string::npos) score += 1000;
            std::string lower = cand;
            for (size_t i = 0; i < lower.size(); i++) {
                lower[i] = (char)std::tolower((unsigned char)lower[i]);
            }
            if (lower.find("theme") != std::string::npos) score += 200;
            if (lower.find("guess") != std::string::npos) score += 120;
            if (lower.find("word") != std::string::npos) score += 120;
            if (lower.find("_ _") != std::string::npos || lower.find("__") != std::string::npos) score += 180;
            if (lower.find("record") != std::string::npos) score -= 60;
            if (score > bestActionBarScore) {
                s.actionBar = cand;
                bestActionBarScore = score;
            }
        };

        static std::string s_lastActionBarLog;
        static DWORD s_lastActionBarLogAt = 0;

        jobject ingameGui = env->GetObjectField(g_mcInstance, g_ingameGuiField);
        if (ingameGui && !env->ExceptionCheck()) {
            for (size_t i = 0; i < g_actionBarFields.size(); i++) {
                const ActionBarFieldRef& ref = g_actionBarFields[i];
                if (!ref.field) continue;

                if (ref.kind == ActionBarFieldChatComponent) {
                    if (!g_chatComponentGetTextMethod) continue;
                    jobject chatComp = env->GetObjectField(ingameGui, ref.field);
                    if (!env->ExceptionCheck() && chatComp) {
                        jstring jtxt = (jstring)env->CallObjectMethod(chatComp, g_chatComponentGetTextMethod);
                        if (!env->ExceptionCheck() && jtxt) {
                            const char* c = env->GetStringUTFChars(jtxt, nullptr);
                            if (c) {
                                std::string cand = c;
                                env->ReleaseStringUTFChars(jtxt, c);
                                tryUseActionBarCandidate(cand);
                            }
                            env->DeleteLocalRef(jtxt);
                        } else if (env->ExceptionCheck()) {
                            env->ExceptionClear();
                        }
                        env->DeleteLocalRef(chatComp);
                    } else if (env->ExceptionCheck()) {
                        env->ExceptionClear();
                    }
                } else {
                    jstring jtxt = (jstring)env->GetObjectField(ingameGui, ref.field);
                    if (!env->ExceptionCheck() && jtxt) {
                        const char* c = env->GetStringUTFChars(jtxt, nullptr);
                        if (c) {
                            std::string cand = c;
                            env->ReleaseStringUTFChars(jtxt, c);
                            tryUseActionBarCandidate(cand);
                        }
                        env->DeleteLocalRef(jtxt);
                    } else if (env->ExceptionCheck()) {
                        env->ExceptionClear();
                    }
                }
            }
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
        if (ingameGui) env->DeleteLocalRef(ingameGui);

        DWORD nowMs = GetTickCount();
        if ((nowMs - s_lastActionBarLogAt) > 2000) {
            s_lastActionBarLogAt = nowMs;
            if (s.actionBar != s_lastActionBarLog) {
                Log(std::string("ActionBar sample: '") + s.actionBar + "'");
                s_lastActionBarLog = s.actionBar;
            }
        }
    }

    if (env->ExceptionCheck()) env->ExceptionClear();
    return s;
}

// ===================== DETACH =====================
static void CleanupImGuiAndHooks() {
    if (g_wndProcHookedHwnd && g_origWndProc) {
        SetWindowLongPtrA(g_wndProcHookedHwnd, GWLP_WNDPROC, (LONG_PTR)g_origWndProc);
        g_wndProcHookedHwnd = nullptr;
        g_origWndProc = nullptr;
    }

    if (g_imguiPhase1Done) {
        ImGui_ImplWin32_Shutdown();
        g_imguiPhase1Done = false;
    }
    if (g_imguiGlBackendReady || g_imguiInitialized) {
        ImGui_ImplOpenGL3_SetSkipGLDeletes(true);
        ImGui_ImplOpenGL3_Shutdown();
        ImGui_ImplOpenGL3_SetSkipGLDeletes(false);
    }
    if (ImGui::GetCurrentContext()) {
        ImGui::DestroyContext();
    }

    g_imguiInitialized = false;
    g_imguiGlBackendReady = false;
    g_imguiPendingBackendReset = false;
    g_imguiPendingGlrc = nullptr;
    g_imguiGlrc = nullptr;
    g_imguiHwnd = nullptr;
    g_glInitialized = false;
    g_gameHwnd = nullptr;

    if (g_minhookInitialized) {
        MH_DisableHook(MH_ALL_HOOKS);
        MH_Uninitialize();
        g_minhookInitialized = false;
    }
}

static void ResetImGuiBackendsForReinit(const char* reason) {
    if (reason && *reason) {
        Log(std::string("ImGui legacy: resetting backends for reinit (") + reason + ").");
    }

    if (g_wndProcHookedHwnd && g_origWndProc) {
        SetWindowLongPtrA(g_wndProcHookedHwnd, GWLP_WNDPROC, (LONG_PTR)g_origWndProc);
        g_wndProcHookedHwnd = nullptr;
        g_origWndProc = nullptr;
    }

    if (g_imguiPhase1Done) {
        ImGui_ImplWin32_Shutdown();
    }
    if (g_imguiGlBackendReady || g_imguiInitialized) {
        ImGui_ImplOpenGL3_SetSkipGLDeletes(true);
        ImGui_ImplOpenGL3_Shutdown();
        ImGui_ImplOpenGL3_SetSkipGLDeletes(false);
    }
    if (ImGui::GetCurrentContext()) {
        ImGui::DestroyContext();
    }

    g_imguiPhase1Done = false;
    g_imguiInitialized = false;
    g_imguiGlBackendReady = false;
    g_imguiPendingBackendReset = false;
    g_imguiPendingGlrc = nullptr;
    g_imguiGlrc = nullptr;
    g_imguiHwnd = nullptr;
    g_imguiWarmupFrames = 0;
    g_glInitialized = false;
}

extern "C" __declspec(dllexport) void Detach() {
    Log("Detach requested");
    g_running = false;

    if (g_clientSocket != INVALID_SOCKET) {
        closesocket(g_clientSocket);
        g_clientSocket = INVALID_SOCKET;
    }
    if (g_serverSocket != INVALID_SOCKET) {
        closesocket(g_serverSocket);
        g_serverSocket = INVALID_SOCKET;
    }
    
    CleanupImGuiAndHooks();
    
    // Restore SwapBuffers (simplified: just unhook IAT if possible, but honestly
    // safely unhooking IAT without race conditions is hard. 
    // For now we just stop rendering logic via g_running flag and let DLL stay loaded but dormant?)
    // A true unload requires FreeLibraryAndExitThread.
    
    // Create a thread to free library safely
    CreateThread(nullptr, 0, [](LPVOID) -> DWORD {
        Sleep(100);
        FreeLibraryAndExitThread(GetModuleHandleA("bridge.dll"), 0);
        return 0;
    }, nullptr, 0, nullptr);
}

// ===================== IMGUI RENDERING =====================
static int ColorByte(float v) {
    if (v < 0.0f) v = 0.0f;
    if (v > 1.0f) v = 1.0f;
    return (int)(v * 255.0f + 0.5f);
}

static ImU32 ImColorFromFloats(float r, float g, float b, float a) {
    return IM_COL32(ColorByte(r), ColorByte(g), ColorByte(b), ColorByte(a));
}

static float LegacyTextPixelSize(float scale) {
    return (std::max)(1.0f, (float)CHAR_H * scale);
}

static float LegacyTextScale(float scale) {
    if (!ImGui::GetCurrentContext()) return (float)CHAR_W * scale;
    float fontSize = ImGui::GetFontSize();
    if (fontSize <= 0.0f) fontSize = 16.0f;
    return LegacyTextPixelSize(scale) / fontSize;
}

void InitFont() {
    // Font atlas creation is now owned by ImGui's OpenGL backend.
    g_glInitialized = g_imguiInitialized;
}

void DrawRect(float x, float y, float w, float h, float r, float g, float b, float a) {
    if (!ImGui::GetCurrentContext() || w <= 0.0f || h <= 0.0f || a <= 0.0f) return;
    ImGui::GetForegroundDrawList()->AddRectFilled(
        ImVec2(x, y),
        ImVec2(x + w, y + h),
        ImColorFromFloats(r, g, b, a));
}

void DrawText2D(float x, float y, const char* text, float r, float g, float b, float a, float scale = 1.0f) {
    if (!ImGui::GetCurrentContext() || !text || !*text || a <= 0.0f) return;
    ImGui::GetForegroundDrawList()->AddText(
        ImGui::GetFont(),
        LegacyTextPixelSize(scale),
        ImVec2(x, y),
        ImColorFromFloats(r, g, b, a),
        text);
}

float TextWidth(const char* text, float scale = 1.0f) {
    if (!text || !*text) return 0.0f;
    if (!ImGui::GetCurrentContext()) return (float)strlen(text) * (float)CHAR_W * scale * 0.5f;
    return ImGui::CalcTextSize(text).x * LegacyTextScale(scale);
}

// Text with shadow for readability.
void DrawTextShadow(float x, float y, const char* text, float r, float g, float b, float a, float scale = 1.0f) {
    DrawText2D(x + 1, y + 1, text, 0, 0, 0, a * 0.65f, scale);
    DrawText2D(x, y, text, r, g, b, a, scale);
}

struct Color3 {
    float r, g, b;
};

float Clamp01(float v) {
    if (v < 0.0f) return 0.0f;
    if (v > 1.0f) return 1.0f;
    return v;
}

Color3 HsvToRgb(float h, float s, float v) {
    h = std::fmod(h, 1.0f);
    if (h < 0.0f) h += 1.0f;
    s = Clamp01(s);
    v = Clamp01(v);

    float c = v * s;
    float x = c * (1.0f - std::fabs(std::fmod(h * 6.0f, 2.0f) - 1.0f));
    float m = v - c;

    float r1 = 0.0f, g1 = 0.0f, b1 = 0.0f;
    int segment = (int)(h * 6.0f);
    switch (segment) {
    case 0: r1 = c; g1 = x; b1 = 0.0f; break;
    case 1: r1 = x; g1 = c; b1 = 0.0f; break;
    case 2: r1 = 0.0f; g1 = c; b1 = x; break;
    case 3: r1 = 0.0f; g1 = x; b1 = c; break;
    case 4: r1 = x; g1 = 0.0f; b1 = c; break;
    default: r1 = c; g1 = 0.0f; b1 = x; break;
    }

    Color3 out = { r1 + m, g1 + m, b1 + m };
    return out;
}

Color3 AccentColor(float offset = 0.0f) {
    return HsvToRgb(g_uiState.accentHue + offset, g_uiState.accentSat, g_uiState.accentVal);
}

Color3 ChromaTextColor(float offset = 0.0f) {
    if (!g_uiState.chromaText) return AccentColor(0.0f);
    double t = (double)GetTickCount64() / 1000.0;
    float hue = g_uiState.accentHue + (float)(t * g_uiState.chromaSpeed) + offset;
    return HsvToRgb(hue, 0.70f, 0.95f);
}

std::string StripMinecraftFormatting(const std::string& in) {
    std::string out;
    out.reserve(in.size());
    for (size_t i = 0; i < in.size(); i++) {
        unsigned char c = (unsigned char)in[i];
        // UTF-8 section sign sequence (§) -> C2 A7, followed by format code char.
        if (c == 0xC2 && i + 2 < in.size() && (unsigned char)in[i + 1] == 0xA7) {
            i += 2;
            continue;
        }
        // Raw section sign byte (§) followed by format code char.
        if (c == 0xA7) {
            if (i + 1 < in.size()) i++;
            continue;
        }
        if (c >= 32 && c <= 126) out.push_back((char)c);
    }
    if (out.size() > 96) out.resize(96);
    return out;
}

static std::string NormalizeNameSpaces(const std::string& in) {
    std::string out;
    out.reserve(in.size());
    bool wasSpace = false;
    for (size_t i = 0; i < in.size(); i++) {
        unsigned char c = (unsigned char)in[i];
        if (c < 32) continue;
        bool isSpace = std::isspace(c) != 0;
        if (isSpace) {
            if (!wasSpace && !out.empty()) out.push_back(' ');
        }
        else {
            out.push_back((char)c);
        }
        wasSpace = isSpace;
    }
    while (!out.empty() && out.back() == ' ') out.pop_back();
    return out;
}

static bool IsLikelyProfileName(const std::string& name) {
    if (name.size() < 3 || name.size() > 16) return false;
    for (char ch : name) {
        unsigned char c = (unsigned char)ch;
        if (std::isalnum(c) || c == '_') continue;
        return false;
    }
    return true;
}

static bool LooksLikeFakePlayerLine(const std::string& rawName) {
    std::string name = NormalizeNameSpaces(StripMinecraftFormatting(rawName));
    if (name.empty()) return true;

    bool anyAlnum = false;
    bool anyNonDecor = false;
    int alnumCount = 0;
    for (char ch : name) {
        unsigned char c = (unsigned char)ch;
        if (std::isalnum(c)) {
            anyAlnum = true;
            anyNonDecor = true;
            alnumCount++;
            continue;
        }
        if (ch == '_' || ch == ' ') {
            anyNonDecor = true;
            continue;
        }
        if (ch == '-' || ch == '=' || ch == '[' || ch == ']' || ch == '(' || ch == ')' || ch == '<' || ch == '>' || ch == '|' || ch == '*' || ch == '~' || ch == ':') {
            continue;
        }
        anyNonDecor = true;
    }

    if (!anyNonDecor) return true;
    if (!anyAlnum && name != "_") return true;
    if (alnumCount < 2 && !IsLikelyProfileName(name)) return true;
    return false;
}

static void EnsureGameProfileCaches(JNIEnv* env, jobject anyPlayerObj) {
    if (!env) return;
    if (!g_gameProfileClass) {
        jclass c = env->FindClass("com/mojang/authlib/GameProfile");
        if (env->ExceptionCheck()) { env->ExceptionClear(); c = nullptr; }
        if (c) {
            g_gameProfileClass = (jclass)env->NewGlobalRef(c);
            env->DeleteLocalRef(c);
        }
    }
    if (g_gameProfileClass && !g_gameProfileGetNameMethod) {
        g_gameProfileGetNameMethod = env->GetMethodID(g_gameProfileClass, "getName", "()Ljava/lang/String;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); g_gameProfileGetNameMethod = nullptr; }
    }
    if (!g_getGameProfileMethod && anyPlayerObj && g_gameProfileClass) {
        jclass pCls = env->GetObjectClass(anyPlayerObj);
        if (pCls && !env->ExceptionCheck()) {
            g_getGameProfileMethod = env->GetMethodID(pCls, "getGameProfile", "()Lcom/mojang/authlib/GameProfile;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_getGameProfileMethod = nullptr; }
            if (!g_getGameProfileMethod) {
                g_getGameProfileMethod = env->GetMethodID(pCls, "func_146103_bH", "()Lcom/mojang/authlib/GameProfile;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_getGameProfileMethod = nullptr; }
            }
            env->DeleteLocalRef(pCls);
        }
        else {
            env->ExceptionClear();
        }
    }
}

static std::string GetStablePlayerName(JNIEnv* env, jobject playerObj) {
    if (!env || !playerObj) return "";

    std::string name;
    jmethodID getNameMethod = g_getNameMethod;
    if (!getNameMethod) {
        jclass pCls = env->GetObjectClass(playerObj);
        if (pCls && !env->ExceptionCheck()) {
            getNameMethod = env->GetMethodID(pCls, "getName", "()Ljava/lang/String;");
            if (!getNameMethod) {
                env->ExceptionClear();
                getNameMethod = env->GetMethodID(pCls, "func_70005_c_", "()Ljava/lang/String;");
            }
            if (!getNameMethod) {
                env->ExceptionClear();
            } else {
                g_getNameMethod = getNameMethod;
                Log("Late-bound player getName method");
            }
            env->DeleteLocalRef(pCls);
        } else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    if (getNameMethod) {
        jstring js = (jstring)env->CallObjectMethod(playerObj, getNameMethod);
        if (!env->ExceptionCheck() && js) {
            const char* cs = env->GetStringUTFChars(js, nullptr);
            if (cs) {
                name = cs;
                env->ReleaseStringUTFChars(js, cs);
            }
            env->DeleteLocalRef(js);
        }
        else if (env->ExceptionCheck()) {
            env->ExceptionClear();
        }
    }

    auto trim = [](std::string& s) {
        while (!s.empty() && (s.back() == ' ' || s.back() == '\t' || s.back() == '\r' || s.back() == '\n')) s.pop_back();
        size_t i = 0;
        while (i < s.size() && (s[i] == ' ' || s[i] == '\t' || s[i] == '\r' || s[i] == '\n')) i++;
        if (i > 0) s.erase(0, i);
    };

    trim(name);
    std::string cleanDisplay = NormalizeNameSpaces(StripMinecraftFormatting(name));
    if (!LooksLikeFakePlayerLine(cleanDisplay)) return cleanDisplay;

    EnsureGameProfileCaches(env, playerObj);
    if (!g_getGameProfileMethod || !g_gameProfileGetNameMethod) return "";

    jobject gp = env->CallObjectMethod(playerObj, g_getGameProfileMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); gp = nullptr; }
    if (!gp) return "";

    jstring js = (jstring)env->CallObjectMethod(gp, g_gameProfileGetNameMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); js = nullptr; }

    std::string profileName;
    if (js) {
        const char* cs = env->GetStringUTFChars(js, nullptr);
        if (cs) {
            profileName = cs;
            env->ReleaseStringUTFChars(js, cs);
        }
        env->DeleteLocalRef(js);
    }
    env->DeleteLocalRef(gp);

    trim(profileName);
    if (IsLikelyProfileName(profileName)) return profileName;
    if (!cleanDisplay.empty() && !LooksLikeFakePlayerLine(cleanDisplay)) return cleanDisplay;
    return "";
}

static std::string ReadNickHiderJavaString(JNIEnv* env, jstring value) {
    if (!env || !value) return "";
    const char* chars = env->GetStringUTFChars(value, nullptr);
    if (!chars) { if (env->ExceptionCheck()) env->ExceptionClear(); return ""; }
    std::string result(chars);
    env->ReleaseStringUTFChars(value, chars);
    return result;
}

static void RestoreLegacyNickHiderTextMutations(JNIEnv* env) {
    if (!env || !g_legacyChatTextField) return;
    for (size_t i = 0; i < g_legacyNickHiderTextMutations.size(); ++i) {
        LegacyNickHiderTextMutation& entry = g_legacyNickHiderTextMutations[i];
        if (!entry.component) continue;
        jstring original = env->NewStringUTF(entry.original.c_str());
        if (original) {
            env->SetObjectField(entry.component, g_legacyChatTextField, original);
            if (env->ExceptionCheck()) env->ExceptionClear();
            env->DeleteLocalRef(original);
        }
        env->DeleteGlobalRef(entry.component);
    }
    g_legacyNickHiderTextMutations.clear();
}

static LegacyNickHiderTextMutation* FindLegacyNickHiderTextMutation(JNIEnv* env, jobject component) {
    for (size_t i = 0; i < g_legacyNickHiderTextMutations.size(); ++i)
        if (env->IsSameObject(component, g_legacyNickHiderTextMutations[i].component) == JNI_TRUE)
            return &g_legacyNickHiderTextMutations[i];
    return nullptr;
}

static void ApplyLegacyNickHiderComponent(JNIEnv* env, jobject component, const Config& cfg, int depth) {
    if (!env || !component || depth > 8) return;
    jclass componentClass = env->GetObjectClass(component);
    if (!componentClass || env->ExceptionCheck()) { env->ExceptionClear(); return; }

    if (!g_legacyChatTextField) {
        g_legacyChatTextField = env->GetFieldID(componentClass, "text", "Ljava/lang/String;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyChatTextField = nullptr; }
        if (!g_legacyChatTextField) {
            g_legacyChatTextField = env->GetFieldID(componentClass, "field_150267_b", "Ljava/lang/String;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyChatTextField = nullptr; }
        }
    }

    if (g_legacyChatTextField) {
        jstring currentJava = (jstring)env->GetObjectField(component, g_legacyChatTextField);
        if (env->ExceptionCheck()) { env->ExceptionClear(); currentJava = nullptr; }
        std::string current = ReadNickHiderJavaString(env, currentJava);
        if (currentJava) env->DeleteLocalRef(currentJava);
        const std::string replaced = lc::ReplaceNickHiderText(current, cfg.nickHiderEnabled,
            g_nickHiderOriginalProfileName, cfg.nickHiderAlias);
        if (replaced != current) {
            if (!FindLegacyNickHiderTextMutation(env, component) && g_legacyNickHiderTextMutations.size() < 512) {
                LegacyNickHiderTextMutation entry;
                entry.component = env->NewGlobalRef(component);
                entry.original = current;
                if (entry.component) g_legacyNickHiderTextMutations.push_back(entry);
            }
            jstring next = env->NewStringUTF(replaced.c_str());
            if (next) {
                env->SetObjectField(component, g_legacyChatTextField, next);
                if (env->ExceptionCheck()) env->ExceptionClear();
                env->DeleteLocalRef(next);
            }
        }
    }

    if (!g_legacyComponentSiblingsMethod) {
        g_legacyComponentSiblingsMethod = env->GetMethodID(componentClass, "getSiblings", "()Ljava/util/List;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyComponentSiblingsMethod = nullptr; }
        if (!g_legacyComponentSiblingsMethod) {
            g_legacyComponentSiblingsMethod = env->GetMethodID(componentClass, "func_150253_a", "()Ljava/util/List;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyComponentSiblingsMethod = nullptr; }
        }
    }
    if (g_legacyComponentSiblingsMethod && g_listSizeMethod && g_listGetMethod) {
        jobject siblings = env->CallObjectMethod(component, g_legacyComponentSiblingsMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); siblings = nullptr; }
        if (siblings) {
            const int count = (std::min)(16, (int)env->CallIntMethod(siblings, g_listSizeMethod));
            if (env->ExceptionCheck()) env->ExceptionClear();
            for (int i = 0; i < count; ++i) {
                jobject child = env->CallObjectMethod(siblings, g_listGetMethod, i);
                if (env->ExceptionCheck()) { env->ExceptionClear(); child = nullptr; }
                if (child) { ApplyLegacyNickHiderComponent(env, child, cfg, depth + 1); env->DeleteLocalRef(child); }
            }
            env->DeleteLocalRef(siblings);
        }
    }
    env->DeleteLocalRef(componentClass);
}

static bool SetLegacyNickHiderProfileName(JNIEnv* env, jobject profile, const std::string& desired) {
    if (!env || !profile || !g_gameProfileNameField || desired.empty()) return false;
    jstring replacement = env->NewStringUTF(desired.c_str());
    if (!replacement) return false;
    env->SetObjectField(profile, g_gameProfileNameField, replacement);
    const bool ok = !env->ExceptionCheck();
    if (!ok) env->ExceptionClear();
    env->DeleteLocalRef(replacement);
    return ok;
}

static jobject GetLegacyNickHiderTabProfile(JNIEnv* env, const std::string& originalName, const Config& cfg) {
    if (!env || !g_mcInstance || !g_mcClass || originalName.empty()) return nullptr;
    if (!g_legacyMcGetNetHandlerMethod) {
        const char* names[] = { "getNetHandler", "func_147114_u", nullptr };
        for (int i = 0; names[i] && !g_legacyMcGetNetHandlerMethod; ++i) {
            g_legacyMcGetNetHandlerMethod = env->GetMethodID(g_mcClass, names[i],
                "()Lnet/minecraft/client/network/NetHandlerPlayClient;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyMcGetNetHandlerMethod = nullptr; }
        }
    }
    if (!g_legacyMcGetNetHandlerMethod) return nullptr;
    jobject netHandler = env->CallObjectMethod(g_mcInstance, g_legacyMcGetNetHandlerMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); netHandler = nullptr; }
    if (!netHandler) return nullptr;
    jclass netClass = env->GetObjectClass(netHandler);
    if (netClass && !g_legacyNetHandlerGetPlayerInfoByNameMethod) {
        const char* names[] = { "getPlayerInfo", "func_175104_a", nullptr };
        for (int i = 0; names[i] && !g_legacyNetHandlerGetPlayerInfoByNameMethod; ++i) {
            g_legacyNetHandlerGetPlayerInfoByNameMethod = env->GetMethodID(netClass, names[i],
                "(Ljava/lang/String;)Lnet/minecraft/client/network/NetworkPlayerInfo;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyNetHandlerGetPlayerInfoByNameMethod = nullptr; }
        }
    }
    jstring nameJava = env->NewStringUTF(originalName.c_str());
    jobject info = (g_legacyNetHandlerGetPlayerInfoByNameMethod && nameJava)
        ? env->CallObjectMethod(netHandler, g_legacyNetHandlerGetPlayerInfoByNameMethod, nameJava) : nullptr;
    if (env->ExceptionCheck()) { env->ExceptionClear(); info = nullptr; }
    if (nameJava) env->DeleteLocalRef(nameJava);
    if (netClass) env->DeleteLocalRef(netClass);
    env->DeleteLocalRef(netHandler);
    if (!info) return nullptr;
    jclass infoClass = env->GetObjectClass(info);
    if (infoClass && !g_legacyNetworkPlayerInfoGetProfileMethod) {
        const char* names[] = { "getGameProfile", "func_178845_a", nullptr };
        for (int i = 0; names[i] && !g_legacyNetworkPlayerInfoGetProfileMethod; ++i) {
            g_legacyNetworkPlayerInfoGetProfileMethod = env->GetMethodID(infoClass, names[i],
                "()Lcom/mojang/authlib/GameProfile;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyNetworkPlayerInfoGetProfileMethod = nullptr; }
        }
    }
    if (infoClass && !g_legacyNetworkPlayerInfoDisplayNameField) {
        const char* fieldNames[] = { "displayName", "field_178862_s", nullptr };
        for (int i = 0; fieldNames[i] && !g_legacyNetworkPlayerInfoDisplayNameField; ++i) {
            g_legacyNetworkPlayerInfoDisplayNameField = env->GetFieldID(infoClass, fieldNames[i],
                "Lnet/minecraft/util/IChatComponent;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyNetworkPlayerInfoDisplayNameField = nullptr; }
        }
    }
    if (g_legacyNetworkPlayerInfoDisplayNameField) {
        jobject displayName = env->GetObjectField(info, g_legacyNetworkPlayerInfoDisplayNameField);
        if (env->ExceptionCheck()) { env->ExceptionClear(); displayName = nullptr; }
        if (displayName) {
            const size_t before = g_legacyNickHiderTextMutations.size();
            ApplyLegacyNickHiderComponent(env, displayName, cfg, 0);
            if (g_legacyNickHiderTextMutations.size() > before && !g_loggedLegacyNickHiderTabDisplayReady) {
                g_loggedLegacyNickHiderTabDisplayReady = true;
                Log("NickHider fallback: tab displayName component alias applied.");
            }
            env->DeleteLocalRef(displayName);
        }
    }
    jobject profile = g_legacyNetworkPlayerInfoGetProfileMethod
        ? env->CallObjectMethod(info, g_legacyNetworkPlayerInfoGetProfileMethod) : nullptr;
    if (env->ExceptionCheck()) { env->ExceptionClear(); profile = nullptr; }
    if (infoClass) env->DeleteLocalRef(infoClass);
    env->DeleteLocalRef(info);
    return profile;
}

static void ApplyLegacyNickHiderProfileFallback(JNIEnv* env, const Config& cfg) {
    if (!env || !g_mcInstance || !g_thePlayerField) return;
    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); player = nullptr; }
    if (!player) return;
    EnsureGameProfileCaches(env, player);
    if (!g_gameProfileClass || !g_getGameProfileMethod || !g_gameProfileGetNameMethod) {
        if (cfg.nickHiderEnabled && !g_loggedLegacyNickHiderProfileFailure) {
            g_loggedLegacyNickHiderProfileFailure = true;
            Log("NickHider fallback: local GameProfile methods unresolved.");
        }
        env->DeleteLocalRef(player);
        return;
    }
    if (!g_gameProfileNameField) {
        g_gameProfileNameField = env->GetFieldID(g_gameProfileClass, "name", "Ljava/lang/String;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); g_gameProfileNameField = nullptr; }
    }
    jobject profile = env->CallObjectMethod(player, g_getGameProfileMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); profile = nullptr; }
    if (!profile || !g_gameProfileNameField) {
        if (cfg.nickHiderEnabled && !g_loggedLegacyNickHiderProfileFailure) {
            g_loggedLegacyNickHiderProfileFailure = true;
            Log("NickHider fallback: GameProfile.name field unresolved.");
        }
        if (profile) env->DeleteLocalRef(profile);
        env->DeleteLocalRef(player);
        return;
    }
    jstring currentJava = (jstring)env->CallObjectMethod(profile, g_gameProfileGetNameMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); currentJava = nullptr; }
    const std::string current = ReadNickHiderJavaString(env, currentJava);
    if (currentJava) env->DeleteLocalRef(currentJava);
    const bool applyAlias = cfg.nickHiderEnabled && !cfg.nickHiderAlias.empty();
    if (applyAlias && !g_nickHiderProfileAliasApplied && !current.empty()) g_nickHiderOriginalProfileName = current;
    const std::string desired = applyAlias ? cfg.nickHiderAlias : g_nickHiderOriginalProfileName;
    bool localChanged = false;
    if (!desired.empty() && current != desired) localChanged = SetLegacyNickHiderProfileName(env, profile, desired);
    else localChanged = !desired.empty();

    jobject tabProfile = GetLegacyNickHiderTabProfile(env, g_nickHiderOriginalProfileName, cfg);
    bool tabChanged = false;
    if (tabProfile && !desired.empty()) {
        tabChanged = SetLegacyNickHiderProfileName(env, tabProfile, desired);
        env->DeleteLocalRef(tabProfile);
    }
    if (applyAlias && localChanged && !g_loggedLegacyNickHiderProfileReady) {
        g_loggedLegacyNickHiderProfileReady = true;
        Log("NickHider fallback: local profile alias applied.");
    }
    if (applyAlias && tabChanged && !g_loggedLegacyNickHiderTabReady) {
        g_loggedLegacyNickHiderTabReady = true;
        Log("NickHider fallback: tab NetworkPlayerInfo alias applied.");
    }
    g_nickHiderProfileAliasApplied = applyAlias;
    if (!applyAlias) g_nickHiderOriginalProfileName.clear();
    env->DeleteLocalRef(profile);
    env->DeleteLocalRef(player);
}

static void ApplyLegacyNickHiderChatFallback(JNIEnv* env, const Config& cfg) {
    if (env && g_mcClass && !g_ingameGuiField) {
        const char* fieldNames[] = { "ingameGUI", "field_71456_v", nullptr };
        for (int i = 0; fieldNames[i] && !g_ingameGuiField; ++i) {
            g_ingameGuiField = env->GetFieldID(g_mcClass, fieldNames[i], "Lnet/minecraft/client/gui/GuiIngame;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_ingameGuiField = nullptr; }
        }
        if (g_ingameGuiField) Log("NickHider fallback: Minecraft.ingameGUI resolved.");
    }
    if (!env || !g_mcInstance || !g_ingameGuiField || !g_listSizeMethod || !g_listGetMethod) {
        if (cfg.nickHiderEnabled && !g_loggedLegacyNickHiderChatFailure) {
            g_loggedLegacyNickHiderChatFailure = true;
            Log("NickHider fallback: GuiIngame/List mappings unavailable for chat.");
        }
        return;
    }
    if (!cfg.nickHiderEnabled) { RestoreLegacyNickHiderTextMutations(env); return; }
    jobject ingameGui = env->GetObjectField(g_mcInstance, g_ingameGuiField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); ingameGui = nullptr; }
    if (!ingameGui) return;
    jclass guiClass = env->GetObjectClass(ingameGui);
    if (!g_legacyGetChatGuiMethod && guiClass) {
        g_legacyGetChatGuiMethod = env->GetMethodID(guiClass, "getChatGUI", "()Lnet/minecraft/client/gui/GuiNewChat;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyGetChatGuiMethod = nullptr; }
        if (!g_legacyGetChatGuiMethod) {
            g_legacyGetChatGuiMethod = env->GetMethodID(guiClass, "func_146158_b", "()Lnet/minecraft/client/gui/GuiNewChat;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyGetChatGuiMethod = nullptr; }
        }
    }
    jobject chatGui = g_legacyGetChatGuiMethod ? env->CallObjectMethod(ingameGui, g_legacyGetChatGuiMethod) : nullptr;
    if (env->ExceptionCheck()) { env->ExceptionClear(); chatGui = nullptr; }
    if (guiClass) env->DeleteLocalRef(guiClass);
    env->DeleteLocalRef(ingameGui);
    if (!chatGui) {
        if (!g_loggedLegacyNickHiderChatFailure) {
            g_loggedLegacyNickHiderChatFailure = true;
            Log("NickHider fallback: GuiIngame.getChatGUI unresolved.");
        }
        return;
    }
    jclass chatClass = env->GetObjectClass(chatGui);
    if (chatClass && !g_legacyChatLinesField) {
        g_legacyChatLinesField = env->GetFieldID(chatClass, "chatLines", "Ljava/util/List;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyChatLinesField = nullptr; }
        if (!g_legacyChatLinesField) {
            g_legacyChatLinesField = env->GetFieldID(chatClass, "field_146253_i", "Ljava/util/List;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyChatLinesField = nullptr; }
        }
        g_legacyDrawnChatLinesField = env->GetFieldID(chatClass, "drawnChatLines", "Ljava/util/List;");
        if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyDrawnChatLinesField = nullptr; }
        if (!g_legacyDrawnChatLinesField) {
            g_legacyDrawnChatLinesField = env->GetFieldID(chatClass, "field_146252_h", "Ljava/util/List;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyDrawnChatLinesField = nullptr; }
        }
        if (!g_legacyChatLinesField && !g_legacyDrawnChatLinesField && !g_loggedLegacyNickHiderChatFailure) {
            g_loggedLegacyNickHiderChatFailure = true;
            Log("NickHider fallback: GuiNewChat line lists unresolved.");
        }
    }
    jfieldID lists[] = { g_legacyChatLinesField, g_legacyDrawnChatLinesField };
    for (int listIndex = 0; listIndex < 2; ++listIndex) {
        if (!lists[listIndex]) continue;
        jobject lines = env->GetObjectField(chatGui, lists[listIndex]);
        if (env->ExceptionCheck()) { env->ExceptionClear(); lines = nullptr; }
        if (!lines) continue;
        const int count = (std::min)(200, (int)env->CallIntMethod(lines, g_listSizeMethod));
        if (env->ExceptionCheck()) env->ExceptionClear();
        for (int i = 0; i < count; ++i) {
            jobject line = env->CallObjectMethod(lines, g_listGetMethod, i);
            if (env->ExceptionCheck()) { env->ExceptionClear(); line = nullptr; }
            if (!line) continue;
            jclass lineClass = env->GetObjectClass(line);
            if (lineClass && !g_legacyChatLineComponentField) {
                g_legacyChatLineComponentField = env->GetFieldID(lineClass, "lineString", "Lnet/minecraft/util/IChatComponent;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyChatLineComponentField = nullptr; }
                if (!g_legacyChatLineComponentField) {
                    g_legacyChatLineComponentField = env->GetFieldID(lineClass, "field_74541_b", "Lnet/minecraft/util/IChatComponent;");
                    if (env->ExceptionCheck()) { env->ExceptionClear(); g_legacyChatLineComponentField = nullptr; }
                }
                if (!g_legacyChatLineComponentField && !g_loggedLegacyNickHiderChatFailure) {
                    g_loggedLegacyNickHiderChatFailure = true;
                    Log("NickHider fallback: ChatLine.lineString unresolved.");
                }
            }
            jobject component = g_legacyChatLineComponentField ? env->GetObjectField(line, g_legacyChatLineComponentField) : nullptr;
            if (env->ExceptionCheck()) { env->ExceptionClear(); component = nullptr; }
            if (component) {
                const size_t before = g_legacyNickHiderTextMutations.size();
                ApplyLegacyNickHiderComponent(env, component, cfg, 0);
                if (g_legacyNickHiderTextMutations.size() > before && !g_loggedLegacyNickHiderChatReady) {
                    g_loggedLegacyNickHiderChatReady = true;
                    Log("NickHider fallback: chat component alias applied.");
                }
                env->DeleteLocalRef(component);
            }
            if (lineClass) env->DeleteLocalRef(lineClass);
            env->DeleteLocalRef(line);
        }
        env->DeleteLocalRef(lines);
    }
    if (chatClass) env->DeleteLocalRef(chatClass);
    env->DeleteLocalRef(chatGui);
}

static void ApplyLegacyNickHiderFallback(JNIEnv* env, const Config& cfg) {
    if (lc::IsNickHiderJvmtiInstalled()) return;
    ApplyLegacyNickHiderProfileFallback(env, cfg);
    ApplyLegacyNickHiderChatFallback(env, cfg);
    if (!g_loggedLegacyNickHiderFallback) {
        g_loggedLegacyNickHiderFallback = true;
        Log("NickHider: Forge JVMTI fallback active (local tab profile + chat components).");
    }
}

std::string ToLowerAscii(std::string s) {
    for (char& ch : s) ch = (char)std::tolower((unsigned char)ch);
    return s;
}

struct Color4 {
    float r, g, b, a;
};

static ImU32 ToImU32(const Color4& c) {
    return ImColorFromFloats(c.r, c.g, c.b, c.a);
}

static ImU32 WithAlpha(const Color4& c, int alpha) {
    return IM_COL32(ColorByte(c.r), ColorByte(c.g), ColorByte(c.b), alpha);
}

static Color4 MakeColor4(int r, int g, int b, int a) {
    Color4 c;
    c.r = (float)r / 255.0f;
    c.g = (float)g / 255.0f;
    c.b = (float)b / 255.0f;
    c.a = (float)a / 255.0f;
    return c;
}

struct OverlayTheme {
    Color4 accentPrimary;
    Color4 accentSecondary;
    Color4 accentTertiary;
    Color4 logoColor;
    Color4 logoShadow;
    Color4 moduleBg;
    Color4 moduleBorder;
    Color4 moduleText;
    Color4 moduleTextShadow;
    Color4 moduleMinimalBg;
    Color4 moduleOutlinedBg;
    Color4 moduleGlassBorder;
    Color4 moduleBoldText;
};

static OverlayTheme ResolveOverlayTheme(const std::string& guiTheme) {
    std::string key = ToLowerAscii(guiTheme);

    // Each theme uses a SINGLE accent (matches the external GUI's AccentBrush).
    // moduleBg / moduleBorder mirror PanelColor / SliderBgColor from App.xaml so
    // the in-game module list reads as the same surface the WPF window uses.

    if (key == "ink") {
        // Ink: pure mono, white-grey accent
        return {
            MakeColor4(176, 182, 192, 255), // accentPrimary
            MakeColor4(176, 182, 192, 255), // accentSecondary (same)
            MakeColor4(176, 182, 192, 255), // accentTertiary (same)
            MakeColor4(232, 234, 238, 255), // logoColor (text)
            MakeColor4(0, 0, 0, 200),       // logoShadow
            MakeColor4(16, 17, 21, 200),    // moduleBg (#101115 panel)
            MakeColor4(22, 24, 28, 200),    // moduleBorder (#16181C)
            MakeColor4(232, 234, 238, 240), // moduleText
            MakeColor4(0, 0, 0, 200),       // moduleTextShadow
            MakeColor4(22, 24, 28, 130),    // moduleMinimalBg
            MakeColor4(16, 17, 21, 200),    // moduleOutlinedBg
            MakeColor4(176, 182, 192, 90),  // moduleGlassBorder
            MakeColor4(232, 234, 238, 245)  // moduleBoldText
        };
    }
    if (key == "graphite") {
        // Graphite: warm mono, beige accent
        return {
            MakeColor4(184, 155, 130, 255),
            MakeColor4(184, 155, 130, 255),
            MakeColor4(184, 155, 130, 255),
            MakeColor4(232, 232, 234, 255),
            MakeColor4(0, 0, 0, 200),
            MakeColor4(19, 19, 22, 200),    // #131316
            MakeColor4(25, 25, 28, 200),    // #19191C
            MakeColor4(232, 232, 234, 240),
            MakeColor4(0, 0, 0, 200),
            MakeColor4(25, 25, 28, 130),
            MakeColor4(19, 19, 22, 200),
            MakeColor4(184, 155, 130, 90),
            MakeColor4(232, 232, 234, 245)
        };
    }
    if (key == "steel") {
        // Steel: cool blue-grey, steel-blue accent
        return {
            MakeColor4(107, 141, 171, 255),
            MakeColor4(107, 141, 171, 255),
            MakeColor4(107, 141, 171, 255),
            MakeColor4(229, 232, 238, 255),
            MakeColor4(0, 0, 0, 200),
            MakeColor4(15, 18, 24, 200),    // #0F1218
            MakeColor4(22, 26, 33, 200),    // #161A21
            MakeColor4(229, 232, 238, 240),
            MakeColor4(0, 0, 0, 200),
            MakeColor4(22, 26, 33, 130),
            MakeColor4(15, 18, 24, 200),
            MakeColor4(107, 141, 171, 90),
            MakeColor4(229, 232, 238, 245)
        };
    }

    // Default = Slate (monochrome navy + coral accent)
    return {
        MakeColor4(199, 98, 90, 255),   // coral #C7625A
        MakeColor4(199, 98, 90, 255),
        MakeColor4(199, 98, 90, 255),
        MakeColor4(232, 234, 238, 255), // logoColor (text)
        MakeColor4(0, 0, 0, 200),
        MakeColor4(18, 20, 26, 200),    // moduleBg (#12141A panel)
        MakeColor4(24, 27, 34, 200),    // moduleBorder (#181B22 slider-bg)
        MakeColor4(232, 234, 238, 240),
        MakeColor4(0, 0, 0, 200),
        MakeColor4(24, 27, 34, 130),    // moduleMinimalBg
        MakeColor4(18, 20, 26, 200),    // moduleOutlinedBg
        MakeColor4(199, 98, 90, 90),    // moduleGlassBorder (coral w/alpha)
        MakeColor4(232, 234, 238, 245)
    };
}

float SwordDamageFromUnlocalizedName(const std::string& unlocLower) {
    if (unlocLower.find("sword") == std::string::npos) return 0.0f;
    if (unlocLower.find("stone") != std::string::npos) return 5.0f;
    if (unlocLower.find("iron") != std::string::npos) return 6.0f;
    if (unlocLower.find("diamond") != std::string::npos) return 7.0f;
    if (unlocLower.find("wood") != std::string::npos || unlocLower.find("gold") != std::string::npos) return 4.0f;
    return 0.0f;
}

std::string NormalizeSpaces(const std::string& s) {
    std::string out;
    out.reserve(s.size());
    bool wasSpace = false;
    for (char ch : s) {
        bool isSpace = std::isspace((unsigned char)ch) != 0;
        if (isSpace) {
            if (!wasSpace) out.push_back(' ');
        }
        else {
            out.push_back(ch);
        }
        wasSpace = isSpace;
    }
    while (!out.empty() && out.front() == ' ') out.erase(out.begin());
    while (!out.empty() && out.back() == ' ') out.pop_back();
    return out;
}

bool ContainsItemKeyword(const std::string& lower) {
    return lower.find("sword") != std::string::npos ||
        lower.find("axe") != std::string::npos ||
        lower.find("pick") != std::string::npos ||
        lower.find("bow") != std::string::npos ||
        lower.find("rod") != std::string::npos ||
        lower.find("potion") != std::string::npos ||
        lower.find("pearl") != std::string::npos ||
        lower.find("block") != std::string::npos;
}

std::string CleanupHeldBaseName(const std::string& rawName) {
    std::string clean = NormalizeSpaces(rawName);
    if (clean.empty()) return clean;

    std::string lower = ToLowerAscii(clean);

    // Common leftover from MC formatting removal: leading single color/style code letter.
    while (clean.size() >= 2) {
        char first = (char)std::tolower((unsigned char)clean[0]);
        unsigned char second = (unsigned char)clean[1];
        bool looksLikeCode = ((first >= '0' && first <= '9') || (first >= 'a' && first <= 'f') || (first >= 'k' && first <= 'o') || first == 'r');
        if (looksLikeCode && (std::isupper(second) || second == '[' || second == '(')) {
            clean.erase(clean.begin());
            clean = NormalizeSpaces(clean);
        }
        else {
            break;
        }
    }

    size_t possessive = lower.find("'s ");
    if (possessive != std::string::npos && possessive < 20) {
        std::string right = clean.substr(possessive + 3);
        if (ContainsItemKeyword(ToLowerAscii(right))) {
            clean = right;
            lower = ToLowerAscii(clean);
        }
    }

    while (!clean.empty() && !std::isalnum((unsigned char)clean.front())) {
        clean.erase(clean.begin());
    }

    // Strip short numeric junk prefixes like "7(" or "3 -".
    while (!clean.empty() && std::isdigit((unsigned char)clean.front())) {
        size_t p = 1;
        while (p < clean.size() && !std::isalpha((unsigned char)clean[p])) p++;
        if (p > 0 && p < clean.size() && std::isalpha((unsigned char)clean[p])) {
            clean.erase(0, p);
            while (!clean.empty() && !std::isalnum((unsigned char)clean.front())) clean.erase(clean.begin());
        }
        else {
            break;
        }
    }

    return NormalizeSpaces(clean);
}

int RomanValue(char c) {
    switch (std::tolower((unsigned char)c)) {
    case 'i': return 1;
    case 'v': return 5;
    case 'x': return 10;
    case 'l': return 50;
    case 'c': return 100;
    case 'd': return 500;
    case 'm': return 1000;
    default: return 0;
    }
}

int ParseRoman(const std::string& token) {
    if (token.empty()) return 0;
    int total = 0;
    int prev = 0;
    for (int i = (int)token.size() - 1; i >= 0; --i) {
        int v = RomanValue(token[(size_t)i]);
        if (v == 0) return 0;
        if (v < prev) total -= v;
        else total += v;
        prev = v;
    }
    return total;
}

int ExtractSharpnessLevel(const std::string& lowerText) {
    size_t sharp = lowerText.find("sharpness");
    if (sharp != std::string::npos) {
        size_t p = sharp + 9;
        while (p < lowerText.size() && !std::isalnum((unsigned char)lowerText[p])) p++;
        if (p < lowerText.size()) {
            if (std::isdigit((unsigned char)lowerText[p])) {
                int lvl = 0;
                while (p < lowerText.size() && std::isdigit((unsigned char)lowerText[p])) {
                    lvl = lvl * 10 + (lowerText[p] - '0');
                    p++;
                }
                if (lvl > 0) return lvl;
            }
            else {
                size_t start = p;
                while (p < lowerText.size() && std::isalpha((unsigned char)lowerText[p])) p++;
                int roman = ParseRoman(lowerText.substr(start, p - start));
                if (roman > 0) return roman;
            }
        }
    }

    size_t swordPos = lowerText.find("sword");
    if (swordPos != std::string::npos) {
        size_t p = swordPos + 5;
        while (p < lowerText.size() && lowerText[p] == ' ') p++;
        if (p < lowerText.size() && std::isdigit((unsigned char)lowerText[p])) {
            int lvl = 0;
            while (p < lowerText.size() && std::isdigit((unsigned char)lowerText[p])) {
                lvl = lvl * 10 + (lowerText[p] - '0');
                p++;
            }
            if (lvl > 0) return lvl;
        }
    }

    return 0;
}

std::string BuildCappedHeldText(const std::string& rawName, float swordDmg) {
    std::string base = CleanupHeldBaseName(rawName);
    if (base.empty()) base = "Item";

    const int maxTotal = 36;
    std::string suffix;
    if (swordDmg > 0.0f) {
        char dmgBuf[24];
        snprintf(dmgBuf, sizeof(dmgBuf), " (%.1f dmg)", swordDmg);
        suffix = dmgBuf;
    }

    int allowedBase = maxTotal - (int)suffix.size();
    if (allowedBase < 8) allowedBase = 8;
    if ((int)base.size() > allowedBase) {
        if (allowedBase > 3) base = base.substr(0, (size_t)(allowedBase - 3)) + "...";
        else base = base.substr(0, (size_t)allowedBase);
    }

    return base + suffix;
}

void DrawHeldItemIcon(float x, float y, float size, const std::string& iconCode, float alpha) {
    DrawRect(x - 0.8f, y - 0.8f, size + 1.6f, size + 1.6f, 0.0f, 0.0f, 0.0f, 0.55f * alpha);
    DrawRect(x, y, size, size, 0.14f, 0.14f, 0.16f, 0.95f * alpha);
    DrawRect(x, y, size, 1.0f, 0.72f, 0.90f, 1.0f, 0.90f * alpha);

    if (iconCode == "SW") {
        DrawRect(x + size * 0.58f, y + size * 0.12f, size * 0.10f, size * 0.58f, 0.85f, 0.87f, 0.91f, alpha);
        DrawRect(x + size * 0.47f, y + size * 0.56f, size * 0.32f, size * 0.10f, 0.72f, 0.56f, 0.35f, alpha);
        DrawRect(x + size * 0.59f, y + size * 0.67f, size * 0.08f, size * 0.20f, 0.62f, 0.44f, 0.26f, alpha);
    }
    else if (iconCode == "PK") {
        DrawRect(x + size * 0.18f, y + size * 0.23f, size * 0.64f, size * 0.10f, 0.80f, 0.83f, 0.88f, alpha);
        DrawRect(x + size * 0.46f, y + size * 0.30f, size * 0.10f, size * 0.48f, 0.68f, 0.50f, 0.31f, alpha);
    }
    else if (iconCode == "AX") {
        DrawRect(x + size * 0.48f, y + size * 0.20f, size * 0.10f, size * 0.58f, 0.66f, 0.48f, 0.29f, alpha);
        DrawRect(x + size * 0.30f, y + size * 0.22f, size * 0.26f, size * 0.20f, 0.80f, 0.84f, 0.89f, alpha);
    }
    else if (iconCode == "BW") {
        DrawRect(x + size * 0.26f, y + size * 0.14f, size * 0.08f, size * 0.72f, 0.67f, 0.50f, 0.30f, alpha);
        DrawRect(x + size * 0.62f, y + size * 0.18f, size * 0.04f, size * 0.64f, 0.84f, 0.86f, 0.90f, alpha);
    }
    else if (iconCode == "BL") {
        DrawRect(x + size * 0.26f, y + size * 0.26f, size * 0.48f, size * 0.48f, 0.46f, 0.70f, 0.45f, alpha);
        DrawRect(x + size * 0.30f, y + size * 0.30f, size * 0.40f, size * 0.40f, 0.34f, 0.56f, 0.34f, alpha);
    }
    else {
        DrawRect(x + size * 0.33f, y + size * 0.33f, size * 0.34f, size * 0.34f, 0.78f, 0.80f, 0.86f, alpha);
    }
}

bool DrawMinecraftHeldItemIcon(JNIEnv* env, jobject heldStack, float x, float y, float size, float alpha) {
    // DISABLED: Calling Minecraft's renderItemAndEffectIntoGUI from SwapBuffers hook causes
    // state corruption and crashes. Minecraft's item renderer expects to be called from
    // within the normal render pipeline, not from an external overlay context.
    // The GL state (framebuffers, textures, matrices) cannot be safely isolated.
    return false;
}

std::string DetermineHeldIconCode(const std::string& lowerText, bool isBlock) {
    if (isBlock) return "BL";
    if (lowerText.find("sword") != std::string::npos) return "SW";
    if (lowerText.find("pick") != std::string::npos) return "PK";
    if (lowerText.find("axe") != std::string::npos) return "AX";
    if (lowerText.find("bow") != std::string::npos) return "BW";
    if (lowerText.find("rod") != std::string::npos) return "RD";
    if (lowerText.find("potion") != std::string::npos) return "PT";
    if (lowerText.find("pearl") != std::string::npos) return "EP";
    return "IT";
}

jobject GetEntityHeldItemStack(JNIEnv* env, jobject entity) {
    if (!env || !entity || !g_getHeldItemMethod) return nullptr;
    jobject heldStack = env->CallObjectMethod(entity, g_getHeldItemMethod);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        return nullptr;
    }
    return heldStack;
}

std::string GetHeldItemInfoFromStack(JNIEnv* env, jobject heldStack, std::string* iconCodeOut) {
    if (!env || !heldStack) return "";

    std::string heldRawName;
    bool isBlock = false;
    std::string unlocalizedLower;
    float itemBaseDamage = 0.0f;

    // --- ItemStack.getDisplayName() ---
    // Look up fresh from this stack's actual class every call.
    {
        jclass stackClass = env->GetObjectClass(heldStack);
        jmethodID getDisplayName = env->GetMethodID(stackClass, "getDisplayName", "()Ljava/lang/String;");
        if (!getDisplayName) {
            env->ExceptionClear();
            getDisplayName = env->GetMethodID(stackClass, "func_82833_r", "()Ljava/lang/String;");
        }
        if (!getDisplayName) env->ExceptionClear();
        env->DeleteLocalRef(stackClass);

        if (getDisplayName) {
            jstring heldName = (jstring)env->CallObjectMethod(heldStack, getDisplayName);
            if (!env->ExceptionCheck() && heldName) {
                const char* heldChars = env->GetStringUTFChars(heldName, nullptr);
                if (heldChars) {
                    heldRawName = StripMinecraftFormatting(heldChars);
                    env->ReleaseStringUTFChars(heldName, heldChars);
                }
                env->DeleteLocalRef(heldName);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
        }
    }

    // --- ItemStack.getItem() -> Item ---
    {
        jclass stackClass2 = env->GetObjectClass(heldStack);
        jmethodID getItem = env->GetMethodID(stackClass2, "getItem", "()Lnet/minecraft/item/Item;");
        if (!getItem) {
            env->ExceptionClear();
            getItem = env->GetMethodID(stackClass2, "func_77973_b", "()Lnet/minecraft/item/Item;");
        }
        if (!getItem) env->ExceptionClear();
        env->DeleteLocalRef(stackClass2);

        if (getItem) {
            jobject heldItem = env->CallObjectMethod(heldStack, getItem);
            if (!env->ExceptionCheck() && heldItem) {
                isBlock = (g_itemBlockClass && env->IsInstanceOf(heldItem, g_itemBlockClass));

                jclass itemClass = env->GetObjectClass(heldItem);

                // --- Item.getUnlocalizedName() ---
                jmethodID getUnloc = env->GetMethodID(itemClass, "getUnlocalizedName", "()Ljava/lang/String;");
                if (!getUnloc) {
                    env->ExceptionClear();
                    getUnloc = env->GetMethodID(itemClass, "func_77658_a", "()Ljava/lang/String;");
                }
                if (!getUnloc) env->ExceptionClear();
                if (getUnloc) {
                    jstring unloc = (jstring)env->CallObjectMethod(heldItem, getUnloc);
                    if (!env->ExceptionCheck() && unloc) {
                        const char* u = env->GetStringUTFChars(unloc, nullptr);
                        if (u) { unlocalizedLower = ToLowerAscii(u); env->ReleaseStringUTFChars(unloc, u); }
                        env->DeleteLocalRef(unloc);
                    } else if (env->ExceptionCheck()) { env->ExceptionClear(); }
                }

                // --- Item.getDamageVsEntity() ---
                jmethodID getDmg = env->GetMethodID(itemClass, "getDamageVsEntity", "()F");
                if (!getDmg) {
                    env->ExceptionClear();
                    getDmg = env->GetMethodID(itemClass, "func_150931_i", "()F");
                }
                if (!getDmg) env->ExceptionClear();
                if (getDmg) {
                    float rawDmg = env->CallFloatMethod(heldItem, getDmg);
                    if (!env->ExceptionCheck() && rawDmg > 0.0f && rawDmg < 20.0f)
                        itemBaseDamage = rawDmg;
                    else if (env->ExceptionCheck()) env->ExceptionClear();
                }

                env->DeleteLocalRef(itemClass);
                env->DeleteLocalRef(heldItem);
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
        }
    }

    float finalSwordDamage = 0.0f;
    std::string lowerHeldName = ToLowerAscii(heldRawName);
    bool isSwordName = lowerHeldName.find("sword") != std::string::npos;
    if (isSwordName) {
        finalSwordDamage = SwordDamageFromUnlocalizedName(unlocalizedLower);
        if (finalSwordDamage <= 0.0f && itemBaseDamage > 0.0f) finalSwordDamage = itemBaseDamage;

        int sharpLevel = ExtractSharpnessLevel(lowerHeldName);
        if (sharpLevel > 0) {
            finalSwordDamage += 1.25f * (float)sharpLevel;
        }
    }

    std::string heldText = BuildCappedHeldText(heldRawName, finalSwordDamage);

    if (iconCodeOut) {
        std::string iconSrc = !unlocalizedLower.empty() ? unlocalizedLower : ToLowerAscii(heldText);
        *iconCodeOut = DetermineHeldIconCode(iconSrc, isBlock);
    }

    return heldText;
}

std::string GetEntityHeldItemInfo(JNIEnv* env, jobject entity, std::string* iconCodeOut) {
    jobject heldStack = GetEntityHeldItemStack(env, entity);
    if (!heldStack) return "";
    std::string out = GetHeldItemInfoFromStack(env, heldStack, iconCodeOut);
    env->DeleteLocalRef(heldStack);
    return out;
}

std::string RelativeDirectionText(float localYawDeg, double toX, double toZ) {
    const double radToDeg = 57.29577951308232;
    float targetYaw = (float)(std::atan2(-toX, toZ) * radToDeg);
    float delta = targetYaw - localYawDeg;
    while (delta > 180.0f) delta -= 360.0f;
    while (delta < -180.0f) delta += 360.0f;

    float ad = std::fabs(delta);
    if (ad <= 32.0f) return "Front";
    if (ad >= 148.0f) return "Back";
    return delta < 0.0f ? "Left" : "Right";
}

static bool IsChatScreenName(const std::string& screenName) {
    if (screenName.empty()) return false;
    return screenName.find("GuiChat") != std::string::npos
        || screenName.find("ChatScreen") != std::string::npos
        || screenName.find("class_408") != std::string::npos;
}

static bool ShouldHideWorldRenderModules(const GameState& state) {
    return state.guiOpen && !IsChatScreenName(state.screenName);
}

// ===================== HUD RENDERING =====================
static void UpdateHudEditor(int winW, int winH) {
    if (!g_hudEditor.active) return;
    if (winW <= 0 || winH <= 0) return;

    // Dragging only works when a GUI/chat is open (OS cursor available).
    bool chatOpen = false;
    { LockGuard lk(g_stateMutex); chatOpen = g_gameState.guiOpen; }

    ImGuiIO& io = ImGui::GetIO();
    ImVec2 mouse = io.MousePos;
    bool mouseDown = chatOpen && io.MouseDown[0];
    bool mouseClicked = chatOpen && ImGui::IsMouseClicked(0);
    bool mouseReleased = ImGui::IsMouseReleased(0);

    // Struct for editor-visible elements
    struct EditorElem { std::string id; ImVec2 tl; float w, h; };
    std::vector<EditorElem> elems;

    lc::HudLayout layoutSnap;
    { LockGuard lk(g_configMutex); layoutSnap = g_hudLayout; }

    // Only screen-anchored panels are editable. Nametags and chest ESP boxes
    // stay attached to the player/chest in the world and are NOT listed here.
    struct BaseSize { const char* id; float w, h; };
    static const BaseSize baseSizes[] = {
        { lc::ELEM_MODULELIST,    120.0f, 200.0f },
        { lc::ELEM_CLOSESTPLAYER, 220.0f,  80.0f },
        { lc::ELEM_PIXELPARTY,    200.0f,  70.0f },
        { lc::ELEM_GTBHINT,       200.0f,  80.0f },
        { nullptr, 0, 0 }
    };
    for (int i = 0; baseSizes[i].id; ++i) {
        lc::HudElementLayout el = layoutSnap.Resolve(baseSizes[i].id);
        float sw = baseSizes[i].w * el.scale;
        float sh = baseSizes[i].h * el.scale;
        ImVec2 tl = lc::HudElementPixelPos(el, sw, sh, winW, winH);
        elems.push_back({ baseSizes[i].id, tl, sw, sh });
    }

    // 1. Begin gesture: hit-test on click
    if (mouseClicked && g_hudEditor.grabbedId.empty()) {
        const float gripSize = 14.0f;
        for (const auto& e : elems) {
            lc::HudElementLayout el = layoutSnap.Resolve(e.id);
            if (!el.movable && !el.resizable) continue;

            ImVec2 gripTL(e.tl.x + e.w - gripSize, e.tl.y + e.h - gripSize);
            ImVec2 gripBR(e.tl.x + e.w, e.tl.y + e.h);
            bool inGrip = el.resizable &&
                mouse.x >= gripTL.x && mouse.x <= gripBR.x &&
                mouse.y >= gripTL.y && mouse.y <= gripBR.y;
            bool inBody = el.movable &&
                mouse.x >= e.tl.x && mouse.x <= e.tl.x + e.w &&
                mouse.y >= e.tl.y && mouse.y <= e.tl.y + e.h;

            if (inGrip) {
                g_hudEditor.grabbedId = e.id;
                g_hudEditor.mode = lc::HudEditorState::Mode::Resize;
                break;
            } else if (inBody) {
                g_hudEditor.grabbedId = e.id;
                g_hudEditor.mode = lc::HudEditorState::Mode::Move;
                g_hudEditor.grabOffset = ImVec2(mouse.x - e.tl.x, mouse.y - e.tl.y);
                break;
            }
        }
    }

    // 2. Apply gesture while held
    if (!g_hudEditor.grabbedId.empty() && mouseDown) {
        LockGuard lk(g_configMutex);
        lc::HudElementLayout& el = g_hudLayout.elements[g_hudEditor.grabbedId];
        if (g_hudEditor.mode == lc::HudEditorState::Mode::Move) {
            el.x = lc::ClampHudCoord((mouse.x - g_hudEditor.grabOffset.x) / (float)winW);
            el.y = lc::ClampHudCoord((mouse.y - g_hudEditor.grabOffset.y) / (float)winH);
        } else if (g_hudEditor.mode == lc::HudEditorState::Mode::Resize) {
            float baseW = 120.0f;
            static const BaseSize bs[] = {
                { lc::ELEM_MODULELIST, 120, 200 }, { lc::ELEM_CLOSESTPLAYER, 220, 80 },
                { lc::ELEM_PIXELPARTY, 200, 70 }, { lc::ELEM_GTBHINT, 200, 80 }, { nullptr, 0, 0 }
            };
            for (int i = 0; bs[i].id; ++i) {
                if (g_hudEditor.grabbedId == bs[i].id) { baseW = bs[i].w; break; }
            }
            float anchorPx = el.x * (float)winW;
            float desiredW = mouse.x - anchorPx;
            el.scale = lc::ClampHudScale(desiredW / baseW);
        }
    }

    // 3. End gesture: mark dirty
    if (mouseReleased && !g_hudEditor.grabbedId.empty()) {
        g_hudEditor.grabbedId.clear();
        g_hudEditor.mode = lc::HudEditorState::Mode::None;
        g_hudEditor.layoutDirty = true;
    }

    // 4. Draw bounding boxes for each element
    ImDrawList* fg = ImGui::GetForegroundDrawList();
    lc::HudLayout layoutDraw;
    { LockGuard lk(g_configMutex); layoutDraw = g_hudLayout; }
    for (const auto& e : elems) {
        lc::HudElementLayout el = layoutDraw.Resolve(e.id);
        float sw = e.w; // already scaled
        float sh = e.h;
        ImVec2 tl = lc::HudElementPixelPos(el, sw, sh, winW, winH);
        ImVec2 br(tl.x + sw, tl.y + sh);

        bool grabbed = (g_hudEditor.grabbedId == e.id);
        ImU32 boxCol = grabbed ? IM_COL32(255, 200, 50, 200) : IM_COL32(80, 180, 255, 140);
        fg->AddRect(tl, br, boxCol, 4.0f, 0, grabbed ? 2.0f : 1.5f);

        // Label
        fg->AddText(ImVec2(tl.x + 4, tl.y + 3), IM_COL32(255, 255, 255, 200), e.id.c_str());

        // Corner resize grip
        const float gripSz = 12.0f;
        ImVec2 gripTL(br.x - gripSz, br.y - gripSz);
        fg->AddRectFilled(gripTL, br, boxCol, 2.0f);
    }
}

void RenderHUD(int winW, int winH) {

    Config cfg; { LockGuard lk(g_configMutex); cfg = g_config; }

    GameState state;
    { LockGuard lk(g_stateMutex); state = g_gameState; }
    if (ShouldHideWorldRenderModules(state)) {
        return;
    }

    if (!cfg.showModuleList) {
        return;
    }

    OverlayTheme theme = ResolveOverlayTheme(cfg.guiTheme);
    ImDrawList* fg = ImGui::GetForegroundDrawList();
    ImGuiIO& io = ImGui::GetIO();

    struct ModLine { std::string text; ImU32 accent; float width; };
    std::vector<ModLine> mods;
    mods.reserve(16);

    auto pushMod = [&](const std::string& text, ImU32 accent) {
        if (text.empty()) return;
        mods.push_back({ text, accent, ImGui::CalcTextSize(text.c_str()).x });
    };

    char acBuf[64];
    if (cfg.armed) {
        int lo = (int)cfg.minCPS;
        int hi = (int)cfg.maxCPS;
        if (hi < lo) std::swap(hi, lo);
        snprintf(acBuf, sizeof(acBuf), "Autoclicker %d-%d", lo, hi);
        pushMod(acBuf, ToImU32(theme.accentPrimary));
    }
    if (cfg.clickInChests)     pushMod("Click in Chests", ToImU32(theme.accentTertiary));
    if (cfg.closestPlayerInfo) pushMod("Closest Player", ToImU32(theme.accentSecondary));
    if (cfg.rightClick)        pushMod("Rightclick", ToImU32(theme.accentTertiary));
    if (cfg.aimAssist)         pushMod("Aim Assist", ToImU32(theme.accentPrimary));
    if (cfg.triggerbot)        pushMod("Triggerbot", ToImU32(theme.accentSecondary));
    if (cfg.killAura)          pushMod("Kill Aura", ToImU32(theme.accentSecondary));
    if (cfg.speedBridge)       pushMod("SpeedBridge", ToImU32(theme.accentPrimary));
    if (cfg.chestEsp)          pushMod("Chest ESP", ToImU32(theme.accentSecondary));
    if (cfg.blockEsp)          pushMod("Block ESP", ToImU32(theme.accentSecondary));
    if (cfg.chestStealer)      pushMod("Chest Stealer", ToImU32(theme.accentTertiary));
    if (cfg.nametags)          pushMod("Nametags", ToImU32(theme.accentPrimary));
    if (cfg.nickHiderEnabled)  pushMod("Nick Hider", ToImU32(theme.accentTertiary));
    if (cfg.gtbHelper)         pushMod("GTB Helper", ToImU32(theme.accentTertiary));
    if (cfg.pixelPartyAssist)  pushMod("Pixel Party", ToImU32(theme.accentSecondary));
    if (cfg.jitter)            pushMod("Jitter", ToImU32(theme.accentSecondary));
    if (cfg.breakBlocks)       pushMod("Break Blocks", ToImU32(theme.accentTertiary));
    if (cfg.reachEnabled)      pushMod("Reach", ToImU32(theme.accentPrimary));
    if (cfg.velocityEnabled)   pushMod("Velocity", ToImU32(theme.accentTertiary));
    if (cfg.antiDebuffEnabled) pushMod("AntiDebuff", ToImU32(theme.accentPrimary));
    if (cfg.hitDelayFixEnabled) pushMod("Hit Delay Fix", ToImU32(theme.accentPrimary));

    std::sort(mods.begin(), mods.end(), [](const ModLine& a, const ModLine& b) {
        if (a.width != b.width) return a.width > b.width;
        return a.text < b.text;
    });

    // Honor the HUD layout anchor + scale so the module list can be moved AND
    // resized in the HUD editor. The block is anchored via the shared
    // HudElementPixelPos helper (same as the editor box and the other panels),
    // and all metrics are multiplied by the element scale. Previously scale was
    // ignored, so resizing the module list in the editor had no visible effect.
    lc::HudElementLayout modLayout;
    { LockGuard lk2(g_configMutex); modLayout = g_hudLayout.Resolve(lc::ELEM_MODULELIST); }
    const float scale = modLayout.scale;

    const float padX       = 8.0f * scale;
    const float padY       = 3.0f * scale;
    const float barW       = 3.0f * scale;
    const float gapY       = 2.0f * scale;
    const float fontH      = ImGui::GetFontSize() * scale;
    const float shadowOff  = (std::max)(1.0f, scale);
    const float rightInset = 6.0f * scale; // keeps a small gap when flush-right
    const int style = (std::max)(0, (std::min)(4, cfg.moduleListStyle));
    ImFont* font = ImGui::GetFont();

    const char* logoText = "aoko client";
    const float boxH = padY + fontH + padY;

    // ── Measuring pass: compute the block bounds (right-aligned bars). ──
    float blockW = 0.0f;
    float blockH = 0.0f;
    float logoW = 0.0f, logoH = 0.0f, logoGap = 0.0f;
    if (cfg.showLogo) {
        logoW   = ImGui::CalcTextSize(logoText).x * scale;
        logoH   = fontH;
        logoGap = 8.0f * scale;
        blockW  = (std::max)(blockW, logoW + rightInset);
        blockH += logoH + logoGap;
    }
    for (size_t i = 0; i < mods.size(); i++) {
        float textW = mods[i].width * scale;
        float boxW  = barW + padX + textW + padX;
        blockW = (std::max)(blockW, boxW + rightInset);
        blockH += boxH;
        if (i + 1 < mods.size()) blockH += gapY;
    }

    if (!mods.empty() || cfg.showLogo) {
        ImVec2 tl = lc::HudElementPixelPos(modLayout, blockW, blockH, winW, winH);
        const float x1 = tl.x + blockW - rightInset; // right edge of bars
        float y = tl.y;

        if (cfg.showLogo) {
            float logoX = x1 - logoW;
            fg->AddText(font, fontH, ImVec2(logoX + shadowOff, y + shadowOff), ToImU32(theme.logoShadow), logoText);
            fg->AddText(font, fontH, ImVec2(logoX, y), ToImU32(theme.logoColor), logoText);
            y += logoH + logoGap;
        }

        for (size_t i = 0; i < mods.size(); i++) {
            const ModLine& m = mods[i];
            float textW = m.width * scale;
            float boxW  = barW + padX + textW + padX;
            float x0 = x1 - boxW;
            float y0 = y;
            float y1 = y + boxH;

            if (style == 0) {
                fg->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1), ToImU32(theme.moduleBg));
                fg->AddRectFilled(ImVec2(x0, y0), ImVec2(x0 + barW, y1), m.accent);
                fg->AddRect(ImVec2(x0, y0), ImVec2(x1, y1), ToImU32(theme.moduleBorder));
                ImVec2 tx = ImVec2(x0 + barW + padX, y0 + padY);
                fg->AddText(font, fontH, ImVec2(tx.x + shadowOff, tx.y + shadowOff), ToImU32(theme.moduleTextShadow), m.text.c_str());
                fg->AddText(font, fontH, tx, ToImU32(theme.moduleText), m.text.c_str());
            } else if (style == 1) {
                fg->AddRectFilled(ImVec2(x1 - textW - 4.0f * scale, y0), ImVec2(x1, y1), ToImU32(theme.moduleMinimalBg));
                fg->AddRectFilled(ImVec2(x1 - 2.0f * scale, y0), ImVec2(x1, y1), m.accent);
                ImVec2 tx = ImVec2(x1 - textW - 2.0f * scale, y0 + padY);
                fg->AddText(font, fontH, ImVec2(tx.x + shadowOff, tx.y + shadowOff), ToImU32(theme.moduleTextShadow), m.text.c_str());
                fg->AddText(font, fontH, tx, m.accent, m.text.c_str());
            } else if (style == 2) {
                fg->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1), ToImU32(theme.moduleOutlinedBg));
                fg->AddRect(ImVec2(x0, y0), ImVec2(x1, y1), m.accent, 4.0f, 0, 1.5f);
                ImVec2 tx = ImVec2(x0 + barW + padX, y0 + padY);
                fg->AddText(font, fontH, ImVec2(tx.x + shadowOff, tx.y + shadowOff), ToImU32(theme.moduleTextShadow), m.text.c_str());
                fg->AddText(font, fontH, tx, m.accent, m.text.c_str());
            } else if (style == 3) {
                fg->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1), ToImU32(theme.moduleMinimalBg), 4.0f);
                fg->AddRect(ImVec2(x0, y0), ImVec2(x1, y1), ToImU32(theme.moduleGlassBorder), 4.0f, 0, 1.0f);
                fg->AddRectFilled(ImVec2(x0 + 1.0f * scale, y0 + 1.0f * scale), ImVec2(x0 + barW + 1.0f * scale, y1 - 1.0f * scale), m.accent);
                ImVec2 tx = ImVec2(x0 + barW + padX, y0 + padY);
                fg->AddText(font, fontH, ImVec2(tx.x + shadowOff, tx.y + shadowOff), ToImU32(theme.moduleTextShadow), m.text.c_str());
                fg->AddText(font, fontH, tx, ToImU32(theme.moduleText), m.text.c_str());
            } else {
                fg->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1), m.accent, 4.0f);
                fg->AddRect(ImVec2(x0, y0), ImVec2(x1, y1), ToImU32(theme.moduleBorder), 4.0f, 0, 1.0f);
                ImVec2 tx = ImVec2(x0 + barW + padX, y0 + padY);
                fg->AddText(font, fontH, ImVec2(tx.x + shadowOff, tx.y + shadowOff), ToImU32(theme.moduleTextShadow), m.text.c_str());
                fg->AddText(font, fontH, tx, ToImU32(theme.moduleBoldText), m.text.c_str());
            }

            y += boxH + gapY;
        }
    }

    if (cfg.gtbHelper) {
        std::string hint = cfg.gtbHint;
        std::string preview = cfg.gtbPreview;
        if (hint.empty() || hint == "-") hint = "waiting for hint...";
        if (preview == "-") preview.clear();

        std::vector<std::string> lines;
        if (!preview.empty()) {
            std::stringstream pss(preview);
            std::string line;
            while (std::getline(pss, line, ',')) {
                size_t b = line.find_first_not_of(' ');
                size_t e = line.find_last_not_of(' ');
                if (b == std::string::npos || e == std::string::npos) continue;
                std::string clean = line.substr(b, e - b + 1);
                if (clean.size() > 56) clean = clean.substr(0, 56) + "...";
                lines.push_back(clean);
            }
            if (lines.empty()) lines.push_back(preview);
        }

        const float lineH = ImGui::GetFontSize() + 2.0f;
        const float headerH = 18.0f + lineH;
        const float maxPanelH = io.DisplaySize.y - 20.0f;
        const float availableH = maxPanelH - headerH - 16.0f;
        size_t maxLinesPerCol = (size_t)(availableH / lineH);
        if (maxLinesPerCol < 1) maxLinesPerCol = 1;

        size_t totalLines = lines.size();
        size_t numCols = (totalLines + maxLinesPerCol - 1) / maxLinesPerCol;
        if (numCols < 1) numCols = 1;
        if (numCols > 6) numCols = 6;
        size_t linesPerCol = (totalLines + numCols - 1) / numCols;
        if (linesPerCol > maxLinesPerCol) linesPerCol = maxLinesPerCol;

        const float colPad = 8.0f;
        float colW = 200.0f;
        const float padXGtb = 10.0f;
        float panelW = padXGtb * 2.0f + colW * (float)numCols + colPad * (float)(numCols > 0 ? numCols - 1 : 0);
        const float maxPanelW = io.DisplaySize.x - 20.0f;
        if (panelW > maxPanelW) {
            panelW = maxPanelW;
            float availW = panelW - padXGtb * 2.0f - colPad * (float)(numCols - 1);
            if (availW < colW * (float)numCols && numCols > 0)
                colW = availW / (float)numCols;
            if (colW < 80.0f) colW = 80.0f;
        }
        if (panelW < 180.0f) panelW = 180.0f;

        float panelH = headerH + lineH * (float)linesPerCol + 16.0f;
        if (panelH > maxPanelH) panelH = maxPanelH;

        const float x1 = io.DisplaySize.x - 10.0f;
        const float y1 = io.DisplaySize.y - 10.0f;
        const float x0 = (std::max)(10.0f, x1 - panelW);
        const float y0 = (std::max)(10.0f, y1 - panelH);

        fg->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1), ToImU32(theme.moduleBg), 5.0f);
        fg->AddRect(ImVec2(x0, y0), ImVec2(x1, y1), WithAlpha(theme.accentTertiary, 220), 5.0f, 0, 1.2f);

        char hintBuf[320];
        snprintf(hintBuf, sizeof(hintBuf), "GTB: %s (%d)", hint.c_str(), (std::max)(0, cfg.gtbCount));
        fg->AddText(ImVec2(x0 + 9.0f, y0 + 8.0f), WithAlpha(theme.accentTertiary, 245), hintBuf);

        size_t visiblePerCol = linesPerCol;
        if (visiblePerCol < 1) visiblePerCol = 1;
        for (size_t col = 0; col < numCols; col++) {
            size_t startIdx = col * linesPerCol;
            size_t endIdx = (std::min)(startIdx + linesPerCol, totalLines);
            if (startIdx >= totalLines) break;

            float cx = x0 + padXGtb + (colW + colPad) * (float)col;
            float ty = y0 + 8.0f + lineH + 6.0f;

            for (size_t i = startIdx; i < endIdx; i++) {
                std::string row = "- " + lines[i];
                fg->AddText(ImVec2(cx, ty), ToImU32(theme.moduleText), row.c_str());
                ty += lineH;
            }
        }
    }
}

// ===================== NAMETAGS =====================
#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

struct LegoVec3 { double x, y, z; };

bool WorldToScreen(LegoVec3 pos, LegoVec3 cam, float yaw, float pitch, float fov, int winW, int winH, float* sx, float* sy) {
    // Relative position
    double dx = pos.x - cam.x;
    double dy = pos.y - cam.y;
    double dz = pos.z - cam.z;

    // Convert to radians
    float yawRad = yaw * (float)M_PI / 180.0f;
    float pitchRad = pitch * (float)M_PI / 180.0f;

    // Rotate around Y (Yaw)
    // Note: MC Yaw 0 = +Z (South). We want to align with -Z (Forward in OpenGL view).
    // The rotation formula must match MC's coordinate system.
    // Let's use the standard rotation that works for most ESPs:
    double x1 = dx * std::cos(yawRad) + dz * std::sin(yawRad); 
    double z1 = dz * std::cos(yawRad) - dx * std::sin(yawRad); 
    
    // Rotate 180 degrees to align with camera forward
    double x2 = -x1; 
    double z2 = -z1;

    // Rotate around X (Pitch)
    double y3 = dy * std::cos(pitchRad) + z2 * std::sin(pitchRad);
    double z3 = z2 * std::cos(pitchRad) - dy * std::sin(pitchRad);

    // In this transformed space, -Z is forward.
    // If z3 > 0, it is behind the camera.
    if (z3 > 0) return false;

    // Projection
    float aspect = (float)winW / (float)winH;
    float fovRad = fov * (float)M_PI / 180.0f;
    
    // Perspective division
    // screenX = x / -z * scale
    // scale = height / (2 * tan(fov/2))
    double dist = -z3;
    if (dist < 0.1) return false;

    double scale = (double)winH / (2.0 * std::tan(fovRad / 2.0));
    
    double screenX = x2 / dist * scale;
    double screenY = y3 / dist * scale;

    *sx = (float)winW / 2.0f - (float)screenX; 
    *sy = (float)winH / 2.0f + (float)screenY; 
    
    return true;
}

// ScopedJNIEnv replaced by JniEnv::Get() from jni_core/scoped_env.h.
// Kept as a thin wrapper so existing call sites compile unchanged.
struct ScopedJNIEnv {
    JNIEnv* env;
    explicit ScopedJNIEnv(JavaVM* vm) : env(JniEnv::Get(vm)) {}
    operator JNIEnv*() const { return env; }
    JNIEnv* operator->() const { return env; }
};

static float MatrixMagnitude(const Matrix4x4& m) {
    float sum = 0.0f;
    for (int i = 0; i < 16; i++) sum += std::fabs(m.m[i]);
    return sum;
}

static bool MatrixProjectionUsable(const Matrix4x4& view, const Matrix4x4& proj) {
    return MatrixMagnitude(view) > 0.0001f && MatrixMagnitude(proj) > 0.0001f;
}

static bool IsLikelyUiOrthoMatrix(const Matrix4x4& view, const Matrix4x4& proj) {
    // Typical UI ortho signatures:
    // - view stays identity-ish
    // - projection has tiny scale terms (~2/w and ~2/h), m[15] ~= 1, m[11] ~= 0
    bool identityView =
        std::fabs(view.m[0] - 1.0f) < 0.02f &&
        std::fabs(view.m[5] - 1.0f) < 0.02f &&
        std::fabs(view.m[10] - 1.0f) < 0.02f &&
        std::fabs(view.m[15] - 1.0f) < 0.02f;
    bool orthoProjection =
        std::fabs(proj.m[15] - 1.0f) < 0.02f &&
        std::fabs(proj.m[11]) < 0.02f &&
        std::fabs(proj.m[0]) > 0.0001f && std::fabs(proj.m[0]) < 0.02f &&
        std::fabs(proj.m[5]) > 0.0001f && std::fabs(proj.m[5]) < 0.02f;
    return identityView && orthoProjection;
}

static void CaptureCurrentRenderMatrices() {
    Matrix4x4 model = {0};
    Matrix4x4 proj = {0};
    glGetFloatv(GL_MODELVIEW_MATRIX, model.m);
    glGetFloatv(GL_PROJECTION_MATRIX, proj.m);

    if (MatrixProjectionUsable(model, proj)) {
        g_lastCapturedModelView = model;
        g_lastCapturedProjection = proj;
        g_hasCapturedRenderMatrices = true;
    }
}

static bool TryUseCapturedRenderMatrices(Matrix4x4& view, Matrix4x4& proj) {
    if (!g_hasCapturedRenderMatrices) return false;
    if (!MatrixProjectionUsable(g_lastCapturedModelView, g_lastCapturedProjection)) return false;
    view = g_lastCapturedModelView;
    proj = g_lastCapturedProjection;
    return true;
}

Matrix4x4 GetMatrix(JNIEnv* env, jfieldID field) {
    Matrix4x4 m = {0};
    if (!env || !g_activeRenderInfoClass || !field) return m;
    
    jobject floatBuffer = env->GetStaticObjectField(g_activeRenderInfoClass, field);
    if (!floatBuffer) return m;

    // Fast path: direct buffer address or array region copy (1 JNI call or memcpy).
    // Falls back to CallFloatMethod×16 only if both fast paths fail.
    if (!ReadFloatBuffer16(env, floatBuffer, m.m, g_floatBufferGet)) {
        env->DeleteLocalRef(floatBuffer);
        Matrix4x4 zero = {0};
        return zero;
    }
    
    env->DeleteLocalRef(floatBuffer);
    return m;
}

bool WorldToScreen(const double x, const double y, const double z, const Matrix4x4& view, const Matrix4x4& proj, int w, int h, float& outX, float& outY) {
    // 1. View Transformation
    float vX = x * view.m[0] + y * view.m[4] + z * view.m[8] + view.m[12];
    float vY = x * view.m[1] + y * view.m[5] + z * view.m[9] + view.m[13];
    float vZ = x * view.m[2] + y * view.m[6] + z * view.m[10] + view.m[14];
    float vW = x * view.m[3] + y * view.m[7] + z * view.m[11] + view.m[15];

    // 2. Project Transformation
    float pX = vX * proj.m[0] + vY * proj.m[4] + vZ * proj.m[8] + proj.m[12];
    float pY = vX * proj.m[1] + vY * proj.m[5] + vZ * proj.m[9] + proj.m[13];
    float pZ = vX * proj.m[2] + vY * proj.m[6] + vZ * proj.m[10] + proj.m[14];
    float pW = vX * proj.m[3] + vY * proj.m[7] + vZ * proj.m[11] + proj.m[15];

    if (pW < 0.02f) return false; // Behind camera

    // 3. Perspective Divide
    float ndcX = pX / pW;
    float ndcY = pY / pW;

    // 4. Viewport Map
    outX = (ndcX + 1.0f) * 0.5f * w;
    outY = (1.0f - ndcY) * 0.5f * h;

    return true;
}
void RenderNametags(int w, int h) {
    TRACE_PATH("enter");
   static bool warnedMissingMappings = false;
    bool showHealth = true;
    bool showArmor = true;
    bool hideVanillaTags = false;
    bool nametagsEnabled = false;
    bool nickHiderEnabled = false;
    bool entityTelemetryNeeded = false;
    int nametagMaxCount = 8;
    std::string guiTheme = "Default";
    std::string nickHiderAlias;
    {
         LockGuard lk(g_configMutex);
         nametagsEnabled = g_config.nametags;
         nickHiderEnabled = g_config.nickHiderEnabled;
         nickHiderAlias = g_config.nickHiderAlias;
         entityTelemetryNeeded = g_config.nametags || g_config.nickHiderEnabled || g_config.closestPlayerInfo || g_config.aimAssist || g_config.nametagHideVanilla || g_legacyNametagSuppressionActive;
         TRACE_BRANCH("entityTelemetryNeeded", entityTelemetryNeeded);
          if (!entityTelemetryNeeded) return;
         showHealth = g_config.nametagShowHealth;
         showArmor = g_config.nametagShowArmor;
         hideVanillaTags = g_config.nametagHideVanilla;
         nametagMaxCount = (std::max)(1, (std::min)(20, g_config.nametagMaxCount));
         guiTheme = g_config.guiTheme;
    }
    OverlayTheme nametagTheme = ResolveOverlayTheme(guiTheme);

    // Default to empty telemetry each run so stale entities are not reused.
    {
        LockGuard lk(g_jsonMutex);
        g_pendingJson = "[]";
    }
   bool mappedReady = (g_mapped && g_mcInstance);
   TRACE_BRANCH("mappedReady", mappedReady);
   if (!mappedReady) return;

    ScopedJNIEnv env(g_jvm);
    TRACE_BRANCH("jniEnvAvailable", env != nullptr);
    if (!env) return;

    TryResolveScreenFieldDirect(env);
    TryResolvePlayerCoreMappings(env);
    TryResolveWorldMappings(env);
    TryResolveRenderMappings(env, false);

    if (!g_theWorldField || !g_listSizeMethod || !g_listGetMethod ||
        !g_thePlayerField || !g_posXField || !g_posYField || !g_posZField) {
        TRACE_PATH("missing-core-player-mappings");
        if (!warnedMissingMappings) {
            warnedMissingMappings = true;
            Log("Nametags missing core/player mappings.");
        }
        return;
    }

    if (!g_playerEntitiesField) {
        TRACE_PATH("missing-world-entity-mappings");
        if (!warnedMissingMappings) {
            warnedMissingMappings = true;
            Log("Nametags waiting for JNI world mappings.");
        }
        return;
    }

    if (!g_activeRenderInfoClass || !g_modelViewField || !g_projectionField) {
        TRACE_PATH("missing-render-mappings");
        if (!warnedMissingMappings) {
            warnedMissingMappings = true;
            Log("Nametags waiting for render mappings.");
        }
        return;
    }

   warnedMissingMappings = false;
   g_tagFrameCounter++;

    // Safety check: Don't render if we have pending exceptions
    if (env->ExceptionCheck()) { env->ExceptionClear(); return; }
    
    // Ensure we can create local references (limit to 256 for this frame)
    if (env->PushLocalFrame(256) < 0) {
        env->ExceptionClear();
        return;
    }
    
    // Ensure C locale for dot decimals
    setlocale(LC_NUMERIC, "C");

    // 1. Get Matrices
    Matrix4x4 view = GetMatrix(env, g_modelViewField);
    Matrix4x4 proj = GetMatrix(env, g_projectionField);
    bool matrixProjectionUsable = MatrixProjectionUsable(view, proj);
    TRACE_BRANCH("matrixProjectionUsableInitial", matrixProjectionUsable);
    bool usedCapturedMatrices = false;
    if (!matrixProjectionUsable) {
        TRACE_PATH("matrix-fallback-captured");
        usedCapturedMatrices = TryUseCapturedRenderMatrices(view, proj);
        matrixProjectionUsable = usedCapturedMatrices;
    }
    TRACE_BRANCH("matrixProjectionUsableAfterFallback", matrixProjectionUsable);

    static int logctr = 0;
    static bool loggedCapturedMatrixFallback = false;
    static bool loggedUiMatrixReject = false;
    if (logctr++ % 600 == 0) {
         Log("Matrix Debug - View[0]: " + std::to_string(view.m[0]) + " Proj[0]: " + std::to_string(proj.m[0]));
         if (!matrixProjectionUsable) Log("WARNING: Matrix projection unavailable.");
         if (usedCapturedMatrices && !loggedCapturedMatrixFallback) {
             loggedCapturedMatrixFallback = true;
             Log("Nametags using captured GL matrix fallback.");
         }
    }

    if (matrixProjectionUsable && IsLikelyUiOrthoMatrix(view, proj)) {
        TRACE_PATH("matrix-ui-ortho-rebind-attempt");
        TryResolveRenderMappings(env, false);
        view = GetMatrix(env, g_modelViewField);
        proj = GetMatrix(env, g_projectionField);
        matrixProjectionUsable = MatrixProjectionUsable(view, proj);
        usedCapturedMatrices = false;
        if (!matrixProjectionUsable) {
            TRACE_PATH("matrix-ui-ortho-second-fallback");
            usedCapturedMatrices = TryUseCapturedRenderMatrices(view, proj);
            matrixProjectionUsable = usedCapturedMatrices;
        }
        if (matrixProjectionUsable && IsLikelyUiOrthoMatrix(view, proj)) {
            matrixProjectionUsable = false;
            if (!loggedUiMatrixReject) {
                loggedUiMatrixReject = true;
                Log("Nametags rejected UI-space matrix projection after rebind attempt.");
            }
        }
    }

    // 2. Get Partial Ticks
    float pt = 1.0f;
    if (g_timerField && g_renderPartialTicksField) {
        jobject timer = env->GetObjectField(g_mcInstance, g_timerField);
        if (timer) {
             pt = env->GetFloatField(timer, g_renderPartialTicksField);
             env->DeleteLocalRef(timer);
        }
    }

    // 3. Get RenderManager Viewer Position (the matrix path expects camera-relative coords)
    double vX = 0.0, vY = 0.0, vZ = 0.0;
    if (g_renderManagerField && g_viewerPosXField && g_viewerPosYField && g_viewerPosZField) {
        jobject rm = env->GetObjectField(g_mcInstance, g_renderManagerField);
        if (rm) {
            vX = env->GetDoubleField(rm, g_viewerPosXField);
            vY = env->GetDoubleField(rm, g_viewerPosYField);
            vZ = env->GetDoubleField(rm, g_viewerPosZField);
            env->DeleteLocalRef(rm);
        }
    }

    // 4. Iterate Entities
    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (!world) {
        if (g_legacyNametagSuppressionActive || !g_hiddenNametagOriginalTeamByPlayerLegacy.empty() || g_lastLegacyNametagSuppressionWorld) {
            ResetLegacyNametagSuppressionState(env, "world-null");
        }
        if (logctr % 600 == 0) Log("WARNING: World is null");
        env->PopLocalFrame(nullptr);
        return;
    }
    TrackLegacySuppressionWorldContext(env, world);
    
    jobject startList = env->GetObjectField(world, g_playerEntitiesField);
    if (!startList) {
        if (logctr % 600 == 0) Log("WARNING: playerEntities list is null");
        env->PopLocalFrame(nullptr);
        return;
    }

    int size = env->CallIntMethod(startList, g_listSizeMethod);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        env->PopLocalFrame(nullptr);
        return;
    }
    if (logctr % 600 == 0) Log("Entity List Size: " + std::to_string(size));
    
    int count = 0;

    bool guiOpen = false; // Internal ClickGUI was removed; only Minecraft screens suppress tags.
    std::string screenName = "none";
    if (g_currentScreenField) {
        jobject currentScreen = env->GetObjectField(g_mcInstance, g_currentScreenField);
        if (currentScreen) {
            guiOpen = true;
            jclass cls = env->GetObjectClass(currentScreen);
            if (cls) {
                jclass cc = env->GetObjectClass(cls);
                jmethodID mGetName = cc ? env->GetMethodID(cc, "getName", "()Ljava/lang/String;") : nullptr;
                if (mGetName) {
                    jstring jn = (jstring)env->CallObjectMethod(cls, mGetName);
                    if (env->ExceptionCheck()) {
                        env->ExceptionClear();
                    } else if (jn) {
                        const char* cn = env->GetStringUTFChars(jn, nullptr);
                        if (cn) {
                            screenName = cn;
                            env->ReleaseStringUTFChars(jn, cn);
                        } else if (env->ExceptionCheck()) {
                            env->ExceptionClear();
                        }
                        env->DeleteLocalRef(jn);
                    }
                }
                if (cc) env->DeleteLocalRef(cc);
                env->DeleteLocalRef(cls);
            }
            env->DeleteLocalRef(currentScreen);
        }
    }

    std::stringstream ss;
    ss << "[";
    constexpr int kEntityJsonCap = 20;
    const int entityProcessCap = (hideVanillaTags || g_legacyNametagSuppressionActive)
        ? (std::max)(1, size)
        : nametagsEnabled
            ? (std::max)(1, (std::min)(20, nametagMaxCount))
            : kEntityJsonCap;
    bool suppressionAppliedThisPass = false;
    bool suppressionAttemptedThisPass = false;
    
    // Get Local Player for distance & health debug
    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    const std::string localAccountName = player ? GetStablePlayerName(env, player) : "";
    jobject hideScoreboardObj = nullptr;
    jobject hideTeamObj = nullptr;
    if ((hideVanillaTags || g_legacyNametagSuppressionActive) && !EnsureLegacyNametagTeamMappings(env, world) && !g_loggedLegacyNametagSuppressionUnavailable) {
        g_loggedLegacyNametagSuppressionUnavailable = true;
        Log("NametagHideVanilla: legacy team-visibility mappings unresolved; fail-open (vanilla nametags remain visible).");
    }
    if (!hideVanillaTags && g_legacyNametagSuppressionActive) {
        jobject restoreScoreboard = GetLegacyScoreboard(env, world);
        if (restoreScoreboard) {
            RestoreLegacyVanillaNametagSuppression(env, restoreScoreboard);
            env->DeleteLocalRef(restoreScoreboard);
        } else {
            g_hiddenNametagOriginalTeamByPlayerLegacy.clear();
        }
        g_legacyNametagSuppressionActive = false;
    }
    if (hideVanillaTags) {
        hideScoreboardObj = GetLegacyScoreboard(env, world);
        if (hideScoreboardObj) {
            hideTeamObj = EnsureLegacyHideTeam(env, hideScoreboardObj);
            if (!hideTeamObj && !g_loggedLegacyNametagSuppressionUnavailable) {
                g_loggedLegacyNametagSuppressionUnavailable = true;
                Log("NametagHideVanilla: legacy hide team unavailable; fail-open (vanilla nametags remain visible).");
            }
        } else if (!g_loggedLegacyNametagSuppressionUnavailable) {
            g_loggedLegacyNametagSuppressionUnavailable = true;
            Log("NametagHideVanilla: legacy scoreboard unavailable; fail-open (vanilla nametags remain visible).");
        }
    }
    float fallbackYaw = 0.0f;
    float fallbackPitch = 0.0f;
    float fallbackFov = 70.0f;
    
    double localPX = 0.0, localPY = 0.0, localPZ = 0.0;
    bool haveLocalPos = false;
    if (player) {
        localPX = env->GetDoubleField(player, g_posXField);
        localPY = env->GetDoubleField(player, g_posYField);
        localPZ = env->GetDoubleField(player, g_posZField);
        haveLocalPos = true;
    }
    if (!haveLocalPos) {
        env->DeleteLocalRef(startList);
        env->DeleteLocalRef(world);
        goto exit_frame;
    }

    if (player) {
        if (g_rotationYawField) {
            fallbackYaw = env->GetFloatField(player, g_rotationYawField);
            if (env->ExceptionCheck()) { env->ExceptionClear(); fallbackYaw = 0.0f; }
        }
        if (g_rotationPitchField) {
            fallbackPitch = env->GetFloatField(player, g_rotationPitchField);
            if (env->ExceptionCheck()) { env->ExceptionClear(); fallbackPitch = 0.0f; }
        }
    }
    if (g_gameSettingsField && g_fovSettingField) {
        jobject gs = env->GetObjectField(g_mcInstance, g_gameSettingsField);
        if (gs) {
            float fov = env->GetFloatField(gs, g_fovSettingField);
            if (!env->ExceptionCheck() && fov >= 10.0f && fov <= 170.0f) {
                fallbackFov = fov;
            } else if (env->ExceptionCheck()) {
                env->ExceptionClear();
            }
            env->DeleteLocalRef(gs);
        }
    }

    for (int i = 0; i < size && count < entityProcessCap; i++) {
        jobject entity = env->CallObjectMethod(startList, g_listGetMethod, i);
        if (env->ExceptionCheck()) {
            env->ExceptionClear();
            break;
        }
        if (!entity) continue;

        if (env->IsSameObject(entity, player)) {
            env->DeleteLocalRef(entity);
            continue;
        }

        // Positions
        double ex = env->GetDoubleField(entity, g_posXField);
        double ey = env->GetDoubleField(entity, g_posYField);
        double ez = env->GetDoubleField(entity, g_posZField);
        
        double lx = g_lastTickPosXField ? env->GetDoubleField(entity, g_lastTickPosXField) : ex;
        double ly = g_lastTickPosYField ? env->GetDoubleField(entity, g_lastTickPosYField) : ey;
        double lz = g_lastTickPosZField ? env->GetDoubleField(entity, g_lastTickPosZField) : ez;

        // Interpolate
        double iX = lx + (ex - lx) * pt;
        double iY = ly + (ey - ly) * pt;
        double iZ = lz + (ez - lz) * pt;
        double rX = iX - vX;
        double rY = iY - vY;
        double rZ = iZ - vZ;

        // Name with fake/bot line filtering parity
        std::string displayName = GetStablePlayerName(env, entity);
        displayName = lc::ReplaceNickHiderText(displayName, nickHiderEnabled, localAccountName, nickHiderAlias);
        if (displayName.empty() || LooksLikeFakePlayerLine(displayName)) {
            env->DeleteLocalRef(entity);
            continue;
        }
        if (hideVanillaTags && hideScoreboardObj && hideTeamObj) {
            suppressionAttemptedThisPass = true;
            if (ApplyLegacyVanillaNametagSuppression(env, hideScoreboardObj, hideTeamObj, displayName)) {
                suppressionAppliedThisPass = true;
            }
        }
        
        // Health
        float health = 20.0f;
        if (g_getHealthMethod) {
            health = env->CallFloatMethod(entity, g_getHealthMethod);
            if (env->ExceptionCheck()) env->ExceptionClear();
        }

        // Project (prefer matrix when stable, otherwise fallback to angle/FOV)
        float sX = 0, sY = 0;
        bool matrixProjected = false;
        float matrixSX = 0, matrixSY = 0;
        if (matrixProjectionUsable) {
            matrixProjected = WorldToScreen(rX, rY + 2.3, rZ, view, proj, w, h, matrixSX, matrixSY);
        }

        LegoVec3 camPos = { localPX, localPY + 1.62, localPZ };

        bool projected = false;
        if (matrixProjected) {
            sX = matrixSX;
            sY = matrixSY;
            projected = true;
        }
        // Do not use angle/FOV fallback here. On 1.8.9 this path can be misaligned and causes ghost tracking.

        // Aim-assist target point: closest projected point on a player body box
        // to the current crosshair center. Keep nametag anchor separate.
        float aimSX = sX;
        float aimSY = sY;
        bool aimProjected = projected;
        {
            const double halfW = 0.30;
            const double xOffsets[3] = { -halfW, 0.0, halfW };
            const double zOffsets[3] = { -halfW, 0.0, halfW };
            const double yOffsets[5] = { 0.15, 0.55, 0.95, 1.35, 1.75 };
            const double centerX = w * 0.5;
            const double centerY = h * 0.5;

            double bestScore = 1e30;
            float bestSx = aimSX;
            float bestSy = aimSY;
            bool bestFound = false;

            for (double yo : yOffsets) {
                for (double xo : xOffsets) {
                    for (double zo : zOffsets) {
                        float tx = 0.0f;
                        float ty = 0.0f;
                        bool sampleProjected = false;

                        if (matrixProjectionUsable) {
                            sampleProjected = WorldToScreen(
                                (iX + xo) - vX,
                                (iY + yo) - vY,
                                (iZ + zo) - vZ,
                                view,
                                proj,
                                w,
                                h,
                                tx,
                                ty);
                        }

                        if (!sampleProjected) continue;

                        double dxCenter = tx - centerX;
                        double dyCenter = ty - centerY;
                        double score = dxCenter * dxCenter + dyCenter * dyCenter;
                        if (score < bestScore) {
                            bestScore = score;
                            bestSx = tx;
                            bestSy = ty;
                            bestFound = true;
                        }
                    }
                }
            }

            if (bestFound) {
                aimSX = bestSx;
                aimSY = bestSy;
                aimProjected = true;
            }
        }

        if (projected) {
             if (logctr % 600 == 0 && count == 0) {
                 Log("Tagged: " + displayName + " SX: " + std::to_string(sX) + " SY: " + std::to_string(sY));
             }
             double dx = iX - localPX;
             double dy = iY - localPY;
             double dz = iZ - localPZ;
             double dist = sqrt(dx*dx + dy*dy + dz*dz);
             if (dist > 48.0) {
                 env->DeleteLocalRef(entity);
                 continue;
             }
             if (sX < -64.0f || sX > (float)w + 64.0f || sY < -64.0f || sY > (float)h + 64.0f) {
                 env->DeleteLocalRef(entity);
                 continue;
             }
              if (!displayName.empty()) {
                  float hpClampedSimple = health < 0 ? 0 : (health > 40 ? 40 : health);

                   if (!nametagsEnabled) {
                       if (count < kEntityJsonCap) {
                           if (count > 0) ss << ",";
                           ss << "{";
                           ss << "\"sx\":" << (aimProjected ? aimSX : sX) << ",";
                           ss << "\"sy\":" << (aimProjected ? aimSY : sY) << ",";
                           ss << "\"dist\":" << dist << ",";
                           ss << "\"name\":\"" << JsonEscape(displayName) << "\",";
                           ss << "\"hp\":" << hpClampedSimple;
                           ss << "}";
                      }
                      count++;
                      if (count >= entityProcessCap) {
                          env->DeleteLocalRef(entity);
                          break;
                      }
                      continue;
                  }

                 float hpClamped = health < 0 ? 0 : (health > 40 ? 40 : health);
                 float hpBarValue = health < 0 ? 0 : (health > 20 ? 20 : health);
                 float hpPct = hpBarValue / 20.0f;
                 if (hpPct < 0.0f) hpPct = 0.0f;
                 if (hpPct > 1.0f) hpPct = 1.0f;

                 int armorPoints = -1;
                 if (showArmor && g_getTotalArmorValueMethod) {
                     armorPoints = env->CallIntMethod(entity, g_getTotalArmorValueMethod);
                     if (env->ExceptionCheck()) { env->ExceptionClear(); armorPoints = -1; }
                 }

                 std::string statsText;
                 if (showHealth) {
                     char hpBuf[32];
                     snprintf(hpBuf, sizeof(hpBuf), "%.0f HP", hpClamped);
                     statsText += hpBuf;
                 }
                 if (showArmor && armorPoints >= 0) {
                     if (!statsText.empty()) statsText += " | ";
                     char armorBuf[32];
                     snprintf(armorBuf, sizeof(armorBuf), "%d ARM", armorPoints);
                     statsText += armorBuf;
                 }

                 std::string heldText = GetEntityHeldItemInfo(env, entity, nullptr);

                 int key = 0;
                 if (g_objectHashCodeMethod) {
                     key = env->CallIntMethod(entity, g_objectHashCodeMethod);
                     if (env->ExceptionCheck()) { env->ExceptionClear(); key = 0; }
                 }
                 if (key == 0) {
                     key = (int)(iX * 17.0 + iZ * 31.0) ^ i;
                 }

                 auto calcTextScaled = [](const char* text, float fontSize) -> ImVec2 {
                     ImVec2 sz = ImGui::CalcTextSize(text ? text : "");
                     float base = ImGui::GetFontSize();
                     if (base <= 0.0f) base = 16.0f;
                     float scale = fontSize / base;
                     sz.x *= scale;
                     sz.y *= scale;
                     return sz;
                 };

                 float nameScale = 1.0f - (float)(dist / 64.0);
                 if (nameScale < 0.65f) nameScale = 0.65f;
                 if (nameScale > 1.0f) nameScale = 1.0f;
                 const float nameFontSize = std::floor(ImGui::GetFontSize() * nameScale);
                 const float infoFontSize = std::floor(nameFontSize * 0.85f);

                 ImVec2 nameSz = calcTextScaled(displayName.c_str(), nameFontSize);
                 ImVec2 statsSz = statsText.empty() ? ImVec2(0, 0) : calcTextScaled(statsText.c_str(), infoFontSize);
                 ImVec2 itemSz = heldText.empty() ? ImVec2(0, 0) : calcTextScaled(heldText.c_str(), infoFontSize);

                 float maxW = (std::max)(nameSz.x, (std::max)(statsSz.x, itemSz.x));
                 float totalH = nameSz.y;
                 if (statsSz.y > 0.0f) totalH += statsSz.y + 2.0f;
                 if (itemSz.y > 0.0f) totalH += itemSz.y + 2.0f;

                 float pad = std::floor(4.0f * nameScale);
                 float px = std::floor(sX - maxW * 0.5f) - pad;
                 float py = std::floor(sY - totalH - pad * 2.0f);
                 auto& smooth = g_tagSmoothing[key];
                 if (!smooth.init) {
                     smooth.x = px;
                     smooth.y = py;
                     smooth.vx = 0.0f;
                     smooth.vy = 0.0f;
                     smooth.init = true;
                 }
                 else {
                     float dxs = px - smooth.x;
                     float dys = py - smooth.y;
                     float deltaSq = dxs * dxs + dys * dys;

                     if (deltaSq > 24000.0f) {
                         smooth.x = px;
                         smooth.y = py;
                         smooth.vx = 0.0f;
                         smooth.vy = 0.0f;
                     }
                     else {
                         float blend = 0.12f;
                         if (dist < 12.0) blend = 0.14f;
                         if (deltaSq < 6.0f) blend = 0.22f;
                         smooth.x += dxs * blend;
                         smooth.y += dys * blend;
                     }
                 }
                 smooth.lastFrame = g_tagFrameCounter;
                 px = smooth.x;
                 py = smooth.y;

                 ImDrawList* fg = ImGui::GetForegroundDrawList();
                 ImVec2 pMin(px, py);
                 ImVec2 pMax(px + maxW + pad * 2.0f, py + totalH + pad * 2.0f + 2.0f);
                 fg->AddRectFilled(pMin, pMax, IM_COL32(0, 0, 0, 160), 3.0f);

                 float curY = py + pad;
                 float centerX = px + (maxW + pad * 2.0f) * 0.5f;
                 float nameX = std::floor(centerX - nameSz.x * 0.5f);
                 fg->AddText(ImGui::GetFont(), nameFontSize, ImVec2(nameX + 1, curY + 1), IM_COL32(0, 0, 0, 255), displayName.c_str());
                 fg->AddText(ImGui::GetFont(), nameFontSize, ImVec2(nameX, curY), ToImU32(nametagTheme.moduleText), displayName.c_str());
                 curY += nameSz.y + 2.0f;

                 if (!statsText.empty()) {
                     float statsX = std::floor(centerX - statsSz.x * 0.5f);
                     ImU32 statCol = hpClamped <= 8.0f ? IM_COL32(255, 100, 100, 250) : IM_COL32(200, 220, 255, 250);
                     fg->AddText(ImGui::GetFont(), infoFontSize, ImVec2(statsX + 1, curY + 1), IM_COL32(0, 0, 0, 255), statsText.c_str());
                     fg->AddText(ImGui::GetFont(), infoFontSize, ImVec2(statsX, curY), statCol, statsText.c_str());
                     curY += statsSz.y + 2.0f;
                 }

                 if (!heldText.empty()) {
                     float heldX = std::floor(centerX - itemSz.x * 0.5f);
                     fg->AddText(ImGui::GetFont(), infoFontSize, ImVec2(heldX + 1, curY + 1), IM_COL32(0, 0, 0, 255), heldText.c_str());
                     fg->AddText(ImGui::GetFont(), infoFontSize, ImVec2(heldX, curY), IM_COL32(255, 200, 80, 250), heldText.c_str());
                 }

                 if (showHealth) {
                     float barW = (pMax.x - pMin.x) * hpPct;
                     ImU32 hpCol = IM_COL32((int)(255 * (1.0f - hpPct)), (int)(220 * hpPct + 35), 60, 255);
                     fg->AddRectFilled(ImVec2(pMin.x, pMax.y),
                                       ImVec2(pMin.x + barW, pMax.y + std::floor(3.0f * nameScale)), hpCol);
                 }
                 if (count < kEntityJsonCap) {
                     if (count > 0) ss << ",";
                     ss << "{";
                     ss << "\"sx\":" << (aimProjected ? aimSX : sX) << ",";
                     ss << "\"sy\":" << (aimProjected ? aimSY : sY) << ",";
                     ss << "\"dist\":" << dist << ",";
                     ss << "\"name\":\"" << JsonEscape(displayName) << "\",";
                     ss << "\"hp\":" << hpClamped;
                     ss << "}";
                 }

                  count++;
                  if (count >= entityProcessCap) {
                      env->DeleteLocalRef(entity);
                      break;
                  }
             }
        }

        env->DeleteLocalRef(entity);
    }
    if (hideTeamObj) env->DeleteLocalRef(hideTeamObj);
    if (hideScoreboardObj) env->DeleteLocalRef(hideScoreboardObj);
    if (hideVanillaTags && suppressionAppliedThisPass) {
        g_legacyNametagSuppressionActive = true;
    } else if (hideVanillaTags && suppressionAttemptedThisPass && !suppressionAppliedThisPass && !g_loggedLegacyNametagSuppressionUnavailable) {
        g_loggedLegacyNametagSuppressionUnavailable = true;
        Log("NametagHideVanilla: legacy player->hide-team assignment failed; fail-open on this runtime.");
    }
    
    // Cleanup stale smoothing state
    for (auto it = g_tagSmoothing.begin(); it != g_tagSmoothing.end(); ) {
        if (g_tagFrameCounter - it->second.lastFrame > 45) {
            it = g_tagSmoothing.erase(it);
        } else {
            ++it;
        }
    }

    // Close entities array
    ss << "]";

    env->DeleteLocalRef(startList);
    env->DeleteLocalRef(world);

    // Store entities JSON for ServerLoop injection
    {
        std::string json = ss.str();
        {
            LockGuard lk(g_jsonMutex);
            g_pendingJson = json;
        }
    }
    
exit_frame:
    env->PopLocalFrame(nullptr);
}

void RenderPixelPartyAssist(int w, int h) {
    bool enabled = false;
    bool closestAlso = false;
    {
        LockGuard lk(g_configMutex);
        enabled = g_config.pixelPartyAssist;
        closestAlso = g_config.closestPlayerInfo;
    }
    if (!enabled) return;

    PixelPartySnap ppSnap;
    { LockGuard lk(g_pixelPartyMutex); ppSnap = g_pixelPartySnap; }

    GameState state;
    { LockGuard lk(g_stateMutex); state = g_gameState; }

    char dirArrow = '^';
    if (ppSnap.targetFound) {
        double toX = ppSnap.tx - state.posX;
        double toZ = ppSnap.tz - state.posZ;
        std::string dirText = RelativeDirectionText(state.rotationYaw, toX, toZ);
        if (dirText == "Back") dirArrow = 'v';
        else if (dirText == "Left") dirArrow = '<';
        else if (dirText == "Right") dirArrow = '>';
    }

    char mainRow[128];
    const char* colorName = ppSnap.colorLabel.empty() ? "?" : ppSnap.colorLabel.c_str();
    if (ppSnap.targetFound) {
        snprintf(mainRow, sizeof(mainRow), "%c %s  %.0fm", dirArrow, colorName, (float)ppSnap.dist);
    } else if (!ppSnap.holdingValid) {
        snprintf(mainRow, sizeof(mainRow), "Hold terracotta");
    } else {
        snprintf(mainRow, sizeof(mainRow), "%s", ppSnap.status.empty() ? "No match" : ppSnap.status.c_str());
    }

    std::string titleRow = "Pixel Party";
    std::string subRow;
    if (!ppSnap.status.empty() && !ppSnap.targetFound)
        subRow = ppSnap.status;

    ImDrawList* fg = ImGui::GetForegroundDrawList();
    const float fontSz = ImGui::GetFontSize();
    const float smallSz = std::floor(fontSz * 0.82f);
    const float padX = 10.0f;
    const float padY = 6.0f;
    const float gapRow = 3.0f;
    const float boxW = 240.0f;

    ImVec2 titleSz = ImGui::CalcTextSize(titleRow.c_str());
    ImVec2 mainSz = ImGui::CalcTextSize(mainRow);
    ImVec2 subSz = subRow.empty() ? ImVec2(0, 0) : ImGui::CalcTextSize(subRow.c_str());

    float contentW = (std::max)(boxW, (std::max)(titleSz.x + padX * 2.0f, (std::max)(mainSz.x + padX * 2.0f, subSz.x + padX * 2.0f)));
    float contentH = padY + smallSz + gapRow + fontSz + gapRow;
    if (!subRow.empty()) contentH += smallSz + gapRow;
    contentH += padY;

    float cx = (float)w * 0.5f;
    float bottomOffset = closestAlso ? 200.0f : 120.0f;
    float by = (float)h - bottomOffset;
    float rx = std::floor(cx - contentW * 0.5f);
    float ry = std::floor(by - contentH);

    fg->AddRectFilled(ImVec2(rx, ry), ImVec2(rx + contentW, ry + contentH), IM_COL32(10, 10, 18, 210), 6.0f);
    fg->AddRect(ImVec2(rx, ry), ImVec2(rx + contentW, ry + contentH), IM_COL32(255, 180, 80, 120), 6.0f, 0, 1.0f);

    float curY = ry + padY;
    float ttx = std::floor(cx - titleSz.x * 0.5f);
    fg->AddText(ImGui::GetFont(), smallSz, ImVec2(ttx + 1, curY + 1), IM_COL32(0, 0, 0, 160), titleRow.c_str());
    fg->AddText(ImGui::GetFont(), smallSz, ImVec2(ttx, curY), IM_COL32(255, 200, 120, 245), titleRow.c_str());
    curY += smallSz + gapRow;

    float mtx = std::floor(cx - mainSz.x * 0.5f);
    fg->AddText(ImVec2(mtx + 1, curY + 1), IM_COL32(0, 0, 0, 160), mainRow);
    fg->AddText(ImVec2(mtx, curY), IM_COL32(255, 220, 140, 245), mainRow);
    curY += fontSz + gapRow;

    if (!subRow.empty()) {
        float stx = std::floor(cx - subSz.x * 0.5f);
        fg->AddText(ImGui::GetFont(), smallSz, ImVec2(stx + 1, curY + 1), IM_COL32(0, 0, 0, 160), subRow.c_str());
        fg->AddText(ImGui::GetFont(), smallSz, ImVec2(stx, curY), IM_COL32(180, 200, 220, 220), subRow.c_str());
    }
}

void RenderClosestPlayerInfo(int w, int h) {
    bool enabled = false;
    bool showHealth = true;
    bool showArmor = true;
    std::string guiTheme = "Default";
    {
        LockGuard lk(g_configMutex);
        enabled = g_config.closestPlayerInfo;
        showHealth = g_config.nametagShowHealth;
        showArmor = g_config.nametagShowArmor;
        guiTheme = g_config.guiTheme;
    }
    if (!enabled) return;
    if (!g_mapped || !g_mcInstance || !g_theWorldField || !g_listSizeMethod || !g_listGetMethod) return;
    if (!g_thePlayerField || !g_posXField || !g_posYField || !g_posZField) return;

    ScopedJNIEnv env(g_jvm);
    if (!env) return;

    TryResolveWorldMappings(env);
    if (!g_playerEntitiesField) return;

    if (env->ExceptionCheck()) { env->ExceptionClear(); return; }
    if (env->PushLocalFrame(256) < 0) { env->ExceptionClear(); return; }

    auto finish = [&]() {
        env->PopLocalFrame(nullptr);
    };

    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (!world) { finish(); return; }
    jobject list = env->GetObjectField(world, g_playerEntitiesField);
    if (!list) { finish(); return; }

    jobject localPlayer = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (!localPlayer) { finish(); return; }

    double lpx = env->GetDoubleField(localPlayer, g_posXField);
    double lpy = env->GetDoubleField(localPlayer, g_posYField);
    double lpz = env->GetDoubleField(localPlayer, g_posZField);
    float localYaw = 0.0f;
    if (g_rotationYawField) localYaw = env->GetFloatField(localPlayer, g_rotationYawField);

    int size = env->CallIntMethod(list, g_listSizeMethod);
    if (env->ExceptionCheck()) { env->ExceptionClear(); finish(); return; }

    int closestIndex = -1;
    double bestDist = 99999.0;
    double bestDx = 0.0, bestDz = 0.0;

    for (int i = 0; i < size; i++) {
        jobject entity = env->CallObjectMethod(list, g_listGetMethod, i);
        if (env->ExceptionCheck()) { env->ExceptionClear(); break; }
        if (!entity) continue;
        if (env->IsSameObject(entity, localPlayer)) { env->DeleteLocalRef(entity); continue; }

        double ex = env->GetDoubleField(entity, g_posXField);
        double ey = env->GetDoubleField(entity, g_posYField);
        double ez = env->GetDoubleField(entity, g_posZField);
        double dx = ex - lpx;
        double dy = ey - lpy;
        double dz = ez - lpz;
        double dist = std::sqrt(dx * dx + dy * dy + dz * dz);
        if (dist < bestDist) {
            std::string candidateName = GetStablePlayerName(env, entity);
            if (!candidateName.empty() && !LooksLikeFakePlayerLine(candidateName)) {
                bestDist = dist;
                closestIndex = i;
                bestDx = dx;
                bestDz = dz;
            }
        }

        env->DeleteLocalRef(entity);
    }

    if (closestIndex < 0 || bestDist > 96.0) { finish(); return; }

    jobject closest = env->CallObjectMethod(list, g_listGetMethod, closestIndex);
    if (env->ExceptionCheck()) { env->ExceptionClear(); finish(); return; }
    if (!closest) { finish(); return; }

    std::string name = GetStablePlayerName(env, closest);
    if (name.empty() || LooksLikeFakePlayerLine(name)) {
        finish();
        return;
    }

    float health = 20.0f;
    if (showHealth && g_getHealthMethod) {
        health = env->CallFloatMethod(closest, g_getHealthMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); health = 20.0f; }
    }

    int armorPoints = -1;
    if (showArmor && g_getTotalArmorValueMethod) {
        armorPoints = env->CallIntMethod(closest, g_getTotalArmorValueMethod);
        if (env->ExceptionCheck()) { env->ExceptionClear(); armorPoints = -1; }
    }

    std::string heldText = GetEntityHeldItemInfo(env, closest, nullptr);

    std::string statsText;
    if (showHealth) {
        char hp[24];
        float hpClamped = health < 0 ? 0 : (health > 40 ? 40 : health);
        snprintf(hp, sizeof(hp), "%.0f HP", hpClamped);
        statsText += hp;
    }
    if (showArmor && armorPoints >= 0) {
        if (!statsText.empty()) statsText += " | ";
        char ar[24];
        snprintf(ar, sizeof(ar), "%d ARM", armorPoints);
        statsText += ar;
    }

    char dirArrow = '^';
    std::string dirText = RelativeDirectionText(localYaw, bestDx, bestDz);
    if (dirText == "Back") dirArrow = 'v';
    else if (dirText == "Left") dirArrow = '<';
    else if (dirText == "Right") dirArrow = '>';

    char nameRow[96];
    snprintf(nameRow, sizeof(nameRow), "%c %s  %.0fm", dirArrow, name.c_str(), (float)bestDist);

    std::string statsRow;
    if (showHealth) {
        char hb[24]; snprintf(hb, sizeof(hb), "HP %.0f/20", health);
        statsRow += hb;
    }
    if (showArmor && armorPoints > 0) {
        if (!statsRow.empty()) statsRow += "  |  ";
        char ab[16]; snprintf(ab, sizeof(ab), "ARM %d", armorPoints);
        statsRow += ab;
    }
    if (!heldText.empty()) {
        if (!statsRow.empty()) statsRow += "  ";
        statsRow += heldText;
    }

    ImDrawList* fg = ImGui::GetForegroundDrawList();
    OverlayTheme theme = ResolveOverlayTheme(guiTheme);
    const float fontSz = ImGui::GetFontSize();
    const float smallSz = std::floor(fontSz * 0.82f);
    const float padX = 10.0f;
    const float padY = 6.0f;
    const float boxW = 220.0f;
    const float hpBarH = 4.0f;
    const float gapRow = 3.0f;

    ImVec2 nameSz = ImGui::CalcTextSize(nameRow);
    ImVec2 statsSz = statsRow.empty() ? ImVec2(0, 0) : ImGui::CalcTextSize(statsRow.c_str());

    lc::HudElementLayout cpLayout;
    { LockGuard lkHud(g_configMutex); cpLayout = g_hudLayout.Resolve(lc::ELEM_CLOSESTPLAYER); }

    float contentW = (std::max)(boxW, (std::max)(nameSz.x + padX * 2.0f, statsSz.x + padX * 2.0f));
    contentW *= cpLayout.scale;
    float contentH = padY + fontSz + gapRow + hpBarH + gapRow;
    if (!statsRow.empty()) contentH += smallSz + gapRow;
    contentH += padY;
    contentH *= cpLayout.scale;

    ImVec2 cpTL = lc::HudElementPixelPos(cpLayout, contentW, contentH, w, h);
    float rx = cpTL.x;
    float ry = cpTL.y;
    float cx = rx + contentW * 0.5f;

    ImVec2 pMin(rx, ry);
    ImVec2 pMax(rx + contentW, ry + contentH);
    fg->AddRectFilled(pMin, pMax, IM_COL32(10, 10, 18, 210), 6.0f);
    fg->AddRect(pMin, pMax, WithAlpha(theme.accentPrimary, 120), 6.0f, 0, 1.0f);

    float curY = ry + padY;
    float ntx = std::floor(cx - nameSz.x * 0.5f);
    fg->AddText(ImVec2(ntx + 1, curY + 1), IM_COL32(0, 0, 0, 160), nameRow);
    fg->AddText(ImVec2(ntx, curY), ToImU32(theme.moduleText), nameRow);
    curY += fontSz + gapRow;

    float hpPct = (std::max)(0.0f, (std::min)(health / 20.0f, 1.0f));
    float barX0 = rx + padX;
    float barX1 = rx + contentW - padX;
    float barFill = barX0 + (barX1 - barX0) * hpPct;
    ImU32 barCol = IM_COL32((int)(255 * (1.0f - hpPct)), (int)(220 * hpPct + 35), 60, 255);
    fg->AddRectFilled(ImVec2(barX0, curY), ImVec2(barX1, curY + hpBarH), IM_COL32(40, 40, 40, 200), 2.0f);
    fg->AddRectFilled(ImVec2(barX0, curY), ImVec2(barFill, curY + hpBarH), barCol, 2.0f);
    curY += hpBarH + gapRow;

    if (!statsRow.empty()) {
        float stx = std::floor(cx - statsSz.x * 0.5f);
        fg->AddText(ImGui::GetFont(), smallSz, ImVec2(stx + 1, curY + 1), IM_COL32(0, 0, 0, 160), statsRow.c_str());
        ImU32 sCol = health <= 6.0f ? IM_COL32(255, 100, 100, 240) : IM_COL32(160, 200, 255, 230);
        fg->AddText(ImGui::GetFont(), smallSz, ImVec2(stx, curY), sCol, statsRow.c_str());
    }

    finish();
}

void RenderChestESP(int w, int h) {
    TRACE_PATH("enter");
    static bool warnedChestMappings = false;
    int chestEspMaxCount = 5;
    {
        LockGuard lk(g_configMutex);
        TRACE_BRANCH("chestEspEnabled", g_config.chestEsp);
        if (!g_config.chestEsp) return;
        chestEspMaxCount = (std::max)(1, (std::min)(20, g_config.chestEspMaxCount));
    }
    bool mappedReady = (g_mapped && g_mcInstance);
    TRACE_BRANCH("mappedReady", mappedReady);
    if (!mappedReady) return;

    ScopedJNIEnv env(g_jvm);
    TRACE_BRANCH("jniEnvAvailable", env != nullptr);
    if (!env) return;

    TryResolveScreenFieldDirect(env);
    TryResolvePlayerCoreMappings(env);
    TryResolveChestEspMappings(env);
    TryResolveWorldMappings(env);
    TryResolveRenderMappings(env, false);

    if (!g_theWorldField || !g_listSizeMethod || !g_listGetMethod ||
        !g_thePlayerField || !g_posXField || !g_posYField || !g_posZField ||
        !g_tileEntityPosField || !g_blockPosGetX || !g_blockPosGetY || !g_blockPosGetZ) {
        TRACE_PATH("missing-core-player-chest-mappings");
        if (!warnedChestMappings) {
            warnedChestMappings = true;
            Log("ChestESP waiting for core/player/chest mappings.");
        }
        return;
    }

    if (!g_loadedTileEntityListField) {
        TRACE_PATH("missing-world-tile-list");
        if (!warnedChestMappings) {
            warnedChestMappings = true;
            Log("ChestESP waiting for JNI world mappings.");
        }
        return;
    }
    if (!g_activeRenderInfoClass || !g_modelViewField || !g_projectionField) {
        TRACE_PATH("missing-render-mappings");
        if (!warnedChestMappings) {
            warnedChestMappings = true;
            Log("ChestESP waiting for render mappings.");
        }
        return;
    }

    warnedChestMappings = false;

    if (env->ExceptionCheck()) { env->ExceptionClear(); return; }
    if (env->PushLocalFrame(256) < 0) { env->ExceptionClear(); return; }

    Matrix4x4 view = GetMatrix(env, g_modelViewField);
    Matrix4x4 proj = GetMatrix(env, g_projectionField);
    bool matrixProjectionUsable = MatrixProjectionUsable(view, proj);
    TRACE_BRANCH("matrixProjectionUsableInitial", matrixProjectionUsable);
    bool usedCapturedMatrices = false;
    if (!matrixProjectionUsable) {
        TRACE_PATH("matrix-fallback-captured");
        usedCapturedMatrices = TryUseCapturedRenderMatrices(view, proj);
        matrixProjectionUsable = usedCapturedMatrices;
    }
    TRACE_BRANCH("matrixProjectionUsableAfterFallback", matrixProjectionUsable);
    static bool loggedChestCapturedMatrixFallback = false;
    static bool loggedChestUiMatrixReject = false;
    if (usedCapturedMatrices && !loggedChestCapturedMatrixFallback) {
        loggedChestCapturedMatrixFallback = true;
        Log("ChestESP using captured GL matrix fallback.");
    }
    if (matrixProjectionUsable && IsLikelyUiOrthoMatrix(view, proj)) {
        TRACE_PATH("matrix-ui-ortho-rebind-attempt");
        TryResolveRenderMappings(env, false);
        view = GetMatrix(env, g_modelViewField);
        proj = GetMatrix(env, g_projectionField);
        matrixProjectionUsable = MatrixProjectionUsable(view, proj);
        usedCapturedMatrices = false;
        if (!matrixProjectionUsable) {
            TRACE_PATH("matrix-ui-ortho-second-fallback");
            usedCapturedMatrices = TryUseCapturedRenderMatrices(view, proj);
            matrixProjectionUsable = usedCapturedMatrices;
        }
        if (matrixProjectionUsable && IsLikelyUiOrthoMatrix(view, proj)) {
            matrixProjectionUsable = false;
            if (!loggedChestUiMatrixReject) {
                loggedChestUiMatrixReject = true;
                Log("ChestESP rejected UI-space matrix projection after rebind attempt.");
            }
        }
    }
    if (!matrixProjectionUsable) {
        static bool warnedChestMatrixUnavailable = false;
        if (!warnedChestMatrixUnavailable) {
            warnedChestMatrixUnavailable = true;
            Log("ChestESP waiting for usable projection matrices.");
        }
        env->PopLocalFrame(nullptr);
        return;
    }

    double vX = 0.0, vY = 0.0, vZ = 0.0;
    if (g_renderManagerField && g_viewerPosXField && g_viewerPosYField && g_viewerPosZField) {
        jobject rm = env->GetObjectField(g_mcInstance, g_renderManagerField);
        if (rm) {
            vX = env->GetDoubleField(rm, g_viewerPosXField);
            vY = env->GetDoubleField(rm, g_viewerPosYField);
            vZ = env->GetDoubleField(rm, g_viewerPosZField);
            env->DeleteLocalRef(rm);
        }
    }

    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (!world) { env->PopLocalFrame(nullptr); return; }
    jobject tileList = env->GetObjectField(world, g_loadedTileEntityListField);
    if (!tileList) {
        env->PopLocalFrame(nullptr);
        return;
    }

    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (!player) {
        env->PopLocalFrame(nullptr);
        return;
    }

    double localPX = env->GetDoubleField(player, g_posXField);
    double localPY = env->GetDoubleField(player, g_posYField);
    double localPZ = env->GetDoubleField(player, g_posZField);

    int size = env->CallIntMethod(tileList, g_listSizeMethod);
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        env->PopLocalFrame(nullptr);
        return;
    }

    int drawn = 0;
    for (int i = 0; i < size && drawn < chestEspMaxCount; i++) {
        jobject te = env->CallObjectMethod(tileList, g_listGetMethod, i);
        if (env->ExceptionCheck()) { env->ExceptionClear(); break; }
        if (!te) continue;

        bool isChest = false;
        if (g_tileEntityChestClass && env->IsInstanceOf(te, g_tileEntityChestClass)) isChest = true;
        if (!isChest && g_tileEntityEnderChestClass && env->IsInstanceOf(te, g_tileEntityEnderChestClass)) isChest = true;
        if (!isChest) {
            jclass teClass = env->GetObjectClass(te);
            if (teClass) {
                std::string clsName = GetClassNameFromClass(env, teClass);
                if (clsName.find("Chest") != std::string::npos) isChest = true;
                env->DeleteLocalRef(teClass);
            }
        }
        if (!isChest) {
            env->DeleteLocalRef(te);
            continue;
        }

        jobject posObj = env->GetObjectField(te, g_tileEntityPosField);
        if (!posObj) {
            env->DeleteLocalRef(te);
            continue;
        }

        int bx = env->CallIntMethod(posObj, g_blockPosGetX);
        int by = env->CallIntMethod(posObj, g_blockPosGetY);
        int bz = env->CallIntMethod(posObj, g_blockPosGetZ);
        env->DeleteLocalRef(posObj);
        if (env->ExceptionCheck()) {
            env->ExceptionClear();
            env->DeleteLocalRef(te);
            continue;
        }

        double cx = (double)bx + 0.5;
        double cy = (double)by + 0.5;
        double cz = (double)bz + 0.5;
        double dx = cx - localPX;
        double dy = cy - localPY;
        double dz = cz - localPZ;
        double dist = sqrt(dx*dx + dy*dy + dz*dz);
        if (dist > 64.0) {
            env->DeleteLocalRef(te);
            continue;
        }

        const double minX = (double)bx;
        const double minY = (double)by;
        const double minZ = (double)bz;
        const double maxX = minX + 1.0;
        const double maxY = minY + 1.0;
        const double maxZ = minZ + 1.0;

        double corners[8][3] = {
            {minX, minY, minZ}, {maxX, minY, minZ}, {maxX, maxY, minZ}, {minX, maxY, minZ},
            {minX, minY, maxZ}, {maxX, minY, maxZ}, {maxX, maxY, maxZ}, {minX, maxY, maxZ}
        };

        float left = 1e9f, top = 1e9f, right = -1e9f, bottom = -1e9f;
        bool projected = true;
        for (int c = 0; c < 8; c++) {
            float sx = 0.0f, sy = 0.0f;
            if (!WorldToScreen(corners[c][0] - vX, corners[c][1] - vY, corners[c][2] - vZ, view, proj, w, h, sx, sy)) {
                projected = false;
                break;
            }
            if (sx < left) left = sx;
            if (sy < top) top = sy;
            if (sx > right) right = sx;
            if (sy > bottom) bottom = sy;
        }

        if (projected && right > left && bottom > top) {
            left = (std::max)(left, 0.0f);
            top = (std::max)(top, 0.0f);
            right = (std::min)(right, (float)w);
            bottom = (std::min)(bottom, (float)h);
            if (right <= left || bottom <= top) {
                env->DeleteLocalRef(te);
                continue;
            }

            float t = (std::min)((float)(dist / 40.0), 1.0f);
            ImU32 boxColor = IM_COL32(255, 165 + (int)(90 * t), 0 + (int)(80 * t), (int)(220 - 40 * t));
            ImDrawList* fg = ImGui::GetForegroundDrawList();
            fg->AddRectFilled(ImVec2(left, top), ImVec2(right, bottom), IM_COL32(0, 0, 0, 90));
            fg->AddRect(ImVec2(left, top), ImVec2(right, bottom), boxColor, 0.0f, 0, 1.5f);
            drawn++;
        }

        env->DeleteLocalRef(te);
    }

    env->PopLocalFrame(nullptr);
}

// ===================== BLOCK ESP / X-RAY (legacy 1.8.9) =====================
static jclass    g_blockClass18           = nullptr; // net/minecraft/block/Block
static jmethodID g_blockGetFromName18     = nullptr; // static Block.getBlockFromName(String)->Block
static jmethodID g_worldGetChunk18        = nullptr; // World.getChunkFromChunkCoords(II)->Chunk
static jmethodID g_chunkGetStorageArray18 = nullptr; // Chunk.getBlockStorageArray()->[ExtendedBlockStorage
static jclass    g_storageClass18         = nullptr; // ExtendedBlockStorage
static jmethodID g_storageGet18           = nullptr; // ExtendedBlockStorage.get(III)->IBlockState

struct BlockEspTargetBlock18 { jobject block; unsigned int color; };
static std::vector<BlockEspTargetBlock18> g_blockEspTargetBlocks18;
static LONG g_blockEspTargetBlocksVersion18 = -1;

struct BlockEspChunkCache18 { std::vector<BlockEspData18> hits; };
static std::map<long long, BlockEspChunkCache18> g_blockEspChunkCache18;

static long long BlockEspChunkKey18(int cx, int cz) {
    return ((long long)(unsigned int)cx << 32) | (unsigned int)cz;
}
static int BlockEspAbsI18(int v) { return v < 0 ? -v : v; }

static void EnsureBlockClass18(JNIEnv* env) {
    if (g_blockClass18) return;
    jobject gcl = g_gameClassLoader;
    jclass c = nullptr;
    if (gcl) c = LoadClassWithLoader(env, gcl, "net/minecraft/block/Block");
    if (!c) { c = env->FindClass("net/minecraft/block/Block"); if (env->ExceptionCheck()) { env->ExceptionClear(); c = nullptr; } }
    if (c) { g_blockClass18 = (jclass)env->NewGlobalRef(c); env->DeleteLocalRef(c); }
    if (g_blockClass18 && !g_blockGetFromName18) {
        const char* names[] = { "getBlockFromName", "func_149684_b", nullptr };
        for (int i = 0; names[i] && !g_blockGetFromName18; i++) {
            g_blockGetFromName18 = env->GetStaticMethodID(g_blockClass18, names[i], "(Ljava/lang/String;)Lnet/minecraft/block/Block;");
            if (env->ExceptionCheck()) { env->ExceptionClear(); g_blockGetFromName18 = nullptr; }
        }
    }
}

// Rebuild the resolved target Block singletons whenever the config target set changes.
static void EnsureBlockEspTargetBlocks18(JNIEnv* env) {
    LONG ver = g_blockEspTargetsVersion;
    if (ver == g_blockEspTargetBlocksVersion18) return;

    for (auto& t : g_blockEspTargetBlocks18) if (t.block) env->DeleteGlobalRef(t.block);
    g_blockEspTargetBlocks18.clear();
    g_blockEspChunkCache18.clear();

    std::vector<lc::BlockEspTargetDef> targets;
    { LockGuard tlk(g_blockEspTargetsMutex); targets = g_blockEspTargets; }

    EnsureBlockClass18(env);
    if (g_blockClass18 && g_blockGetFromName18) {
        for (const auto& tg : targets) {
            jobject blk = nullptr;
            const std::string tryNames[2] = { tg.id, std::string("minecraft:") + tg.id };
            for (int n = 0; n < 2 && !blk; n++) {
                jstring js = env->NewStringUTF(tryNames[n].c_str());
                if (!js) continue;
                jobject b = env->CallStaticObjectMethod(g_blockClass18, g_blockGetFromName18, js);
                if (env->ExceptionCheck()) { env->ExceptionClear(); b = nullptr; }
                env->DeleteLocalRef(js);
                if (b) { blk = b; }
            }
            if (blk) {
                g_blockEspTargetBlocks18.push_back({ env->NewGlobalRef(blk), tg.color });
                env->DeleteLocalRef(blk);
            }
        }
    }
    g_blockEspTargetBlocksVersion18 = ver;
}

static void UpdateBlockEspListLegacy(JNIEnv* env, const Config& cfg) {
    DWORD now = GetTickCount();
    if (now - g_lastBlockEspScanMs < 120) return;
    g_lastBlockEspScanMs = now;

    if (!g_mcInstance || !g_theWorldField || !g_thePlayerField || !g_posXField || !g_posYField || !g_posZField) return;

    EnsureBlockEspTargetBlocks18(env);
    if (g_blockEspTargetBlocks18.empty()) {
        { LockGuard lk(g_blockEspListMutex); g_blockEspList.clear(); }
        g_blockEspChunkCache18.clear();
        return;
    }

    int range = (std::max)(1, (std::min)(8, cfg.blockEspRange));
    int maxCount = (std::max)(1, (std::min)(512, cfg.blockEspMaxCount));

    jobject world = env->GetObjectField(g_mcInstance, g_theWorldField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); world = nullptr; }
    if (!world) return;
    jobject player = env->GetObjectField(g_mcInstance, g_thePlayerField);
    if (env->ExceptionCheck()) { env->ExceptionClear(); player = nullptr; }
    if (!player) { env->DeleteLocalRef(world); return; }

    double px = env->GetDoubleField(player, g_posXField);
    double py = env->GetDoubleField(player, g_posYField);
    double pz = env->GetDoubleField(player, g_posZField);
    env->DeleteLocalRef(player);

    // Resolve chunk/storage methods lazily.
    if (!g_worldGetChunk18) {
        jclass wcls = env->GetObjectClass(world);
        if (wcls) {
            const char* names[] = { "getChunkFromChunkCoords", "func_72964_e", "getChunk", nullptr };
            for (int i = 0; names[i] && !g_worldGetChunk18; i++) {
                g_worldGetChunk18 = env->GetMethodID(wcls, names[i], "(II)Lnet/minecraft/world/chunk/Chunk;");
                if (env->ExceptionCheck()) { env->ExceptionClear(); g_worldGetChunk18 = nullptr; }
            }
            env->DeleteLocalRef(wcls);
        }
    }
    if (!g_worldGetChunk18) { env->DeleteLocalRef(world); return; }

    int pcx = (int)std::floor(px) >> 4;
    int pcz = (int)std::floor(pz) >> 4;

    // Evict out-of-range cache entries.
    for (auto it = g_blockEspChunkCache18.begin(); it != g_blockEspChunkCache18.end(); ) {
        int ccx = (int)(it->first >> 32);
        int ccz = (int)(it->first & 0xffffffff);
        if (BlockEspAbsI18(ccx - pcx) > range || BlockEspAbsI18(ccz - pcz) > range)
            it = g_blockEspChunkCache18.erase(it);
        else ++it;
    }

    // Round-robin: scan at most one missing chunk per tick.
    const int CHUNK_BUDGET = 1;
    int scanned = 0;
    for (int dx = -range; dx <= range && scanned < CHUNK_BUDGET; dx++) {
        for (int dz = -range; dz <= range && scanned < CHUNK_BUDGET; dz++) {
            int cx = pcx + dx, cz = pcz + dz;
            long long key = BlockEspChunkKey18(cx, cz);
            if (g_blockEspChunkCache18.find(key) != g_blockEspChunkCache18.end()) continue;

            BlockEspChunkCache18 cache;
            jobject chunk = env->CallObjectMethod(world, g_worldGetChunk18, cx, cz);
            if (env->ExceptionCheck()) { env->ExceptionClear(); chunk = nullptr; }
            if (!chunk) continue;

            if (!g_chunkGetStorageArray18) {
                jclass ccls = env->GetObjectClass(chunk);
                if (ccls) {
                    const char* names[] = { "getBlockStorageArray", "func_76587_i", nullptr };
                    for (int i = 0; names[i] && !g_chunkGetStorageArray18; i++) {
                        g_chunkGetStorageArray18 = env->GetMethodID(ccls, names[i], "()[Lnet/minecraft/world/chunk/storage/ExtendedBlockStorage;");
                        if (env->ExceptionCheck()) { env->ExceptionClear(); g_chunkGetStorageArray18 = nullptr; }
                    }
                    env->DeleteLocalRef(ccls);
                }
            }
            if (!g_chunkGetStorageArray18) { env->DeleteLocalRef(chunk); break; }

            jobjectArray storages = (jobjectArray)env->CallObjectMethod(chunk, g_chunkGetStorageArray18);
            if (env->ExceptionCheck()) { env->ExceptionClear(); storages = nullptr; }
            if (storages) {
                jsize nsec = env->GetArrayLength(storages);
                for (jsize si = 0; si < nsec; si++) {
                    jobject sec = env->GetObjectArrayElement(storages, si);
                    if (env->ExceptionCheck()) { env->ExceptionClear(); sec = nullptr; }
                    if (!sec) continue; // null = empty section in 1.8.9

                    if (!g_storageGet18) {
                        jclass scls = env->GetObjectClass(sec);
                        if (scls) {
                            if (!g_storageClass18) g_storageClass18 = (jclass)env->NewGlobalRef(scls);
                            const char* names[] = { "get", "func_177485_a", nullptr };
                            for (int i = 0; names[i] && !g_storageGet18; i++) {
                                g_storageGet18 = env->GetMethodID(scls, names[i], "(III)Lnet/minecraft/block/state/IBlockState;");
                                if (env->ExceptionCheck()) { env->ExceptionClear(); g_storageGet18 = nullptr; }
                            }
                            env->DeleteLocalRef(scls);
                        }
                    }
                    if (g_storageGet18) {
                        int sectionMinY = (int)si * 16;
                        for (int ly = 0; ly < 16; ly++) {
                            for (int lz = 0; lz < 16; lz++) {
                                for (int lx = 0; lx < 16; lx++) {
                                    jobject state = env->CallObjectMethod(sec, g_storageGet18, lx, ly, lz);
                                    if (env->ExceptionCheck()) { env->ExceptionClear(); state = nullptr; }
                                    if (!state) continue;
                                    if (!g_blockStateGetBlockMethod) {
                                        jclass stcls = env->GetObjectClass(state);
                                        if (stcls) {
                                            const char* names[] = { "getBlock", "func_177230_c", nullptr };
                                            for (int i = 0; names[i] && !g_blockStateGetBlockMethod; i++) {
                                                g_blockStateGetBlockMethod = env->GetMethodID(stcls, names[i], "()Lnet/minecraft/block/Block;");
                                                if (env->ExceptionCheck()) { env->ExceptionClear(); g_blockStateGetBlockMethod = nullptr; }
                                            }
                                            env->DeleteLocalRef(stcls);
                                        }
                                    }
                                    if (g_blockStateGetBlockMethod) {
                                        jobject block = env->CallObjectMethod(state, g_blockStateGetBlockMethod);
                                        if (env->ExceptionCheck()) { env->ExceptionClear(); block = nullptr; }
                                        if (block) {
                                            for (const auto& tb : g_blockEspTargetBlocks18) {
                                                if (env->IsSameObject(tb.block, block)) {
                                                    double wx = (double)cx * 16 + lx + 0.5;
                                                    double wy = (double)sectionMinY + ly + 0.5;
                                                    double wz = (double)cz * 16 + lz + 0.5;
                                                    cache.hits.push_back({ wx, wy, wz, tb.color, 0.0 });
                                                    break;
                                                }
                                            }
                                            env->DeleteLocalRef(block);
                                        }
                                    }
                                    env->DeleteLocalRef(state);
                                }
                            }
                        }
                    }
                    env->DeleteLocalRef(sec);
                }
                env->DeleteLocalRef(storages);
            }
            env->DeleteLocalRef(chunk);
            g_blockEspChunkCache18[key] = std::move(cache);
            scanned++;
        }
    }

    env->DeleteLocalRef(world);

    std::vector<BlockEspData18> merged;
    for (auto& kv : g_blockEspChunkCache18) {
        for (auto& h : kv.second.hits) {
            double ddx = h.x - px, ddy = h.y - py, ddz = h.z - pz;
            BlockEspData18 d = h;
            d.dist = std::sqrt(ddx * ddx + ddy * ddy + ddz * ddz);
            merged.push_back(d);
        }
    }
    std::sort(merged.begin(), merged.end(),
              [](const BlockEspData18& a, const BlockEspData18& b) { return a.dist < b.dist; });
    if ((int)merged.size() > maxCount) merged.resize(maxCount);
    { LockGuard lk(g_blockEspListMutex); g_blockEspList.swap(merged); }
}

void RenderBlockESP(int w, int h) {
    {
        LockGuard lk(g_configMutex);
        if (!g_config.blockEsp) return;
    }
    if (!g_mapped || !g_mcInstance) return;
    ScopedJNIEnv env(g_jvm);
    if (!env) return;

    TryResolvePlayerCoreMappings(env);
    TryResolveRenderMappings(env, false);
    if (!g_thePlayerField || !g_posXField || !g_posYField || !g_posZField) return;
    if (!g_activeRenderInfoClass || !g_modelViewField || !g_projectionField) return;

    bool boxes, tracers, hud;
    int maxCount;
    { LockGuard lk(g_configMutex); boxes = g_config.blockEspBoxes; tracers = g_config.blockEspTracers; hud = g_config.blockEspHud; maxCount = g_config.blockEspMaxCount; }

    std::vector<BlockEspData18> blocks;
    { LockGuard lk(g_blockEspListMutex); blocks = g_blockEspList; }
    if (blocks.empty()) return;
    std::vector<lc::BlockEspTargetDef> targets;
    { LockGuard tlk(g_blockEspTargetsMutex); targets = g_blockEspTargets; }

    if (env->ExceptionCheck()) { env->ExceptionClear(); return; }
    if (env->PushLocalFrame(64) < 0) { env->ExceptionClear(); return; }

    Matrix4x4 view = GetMatrix(env, g_modelViewField);
    Matrix4x4 proj = GetMatrix(env, g_projectionField);
    bool usable = MatrixProjectionUsable(view, proj);
    if (!usable) usable = TryUseCapturedRenderMatrices(view, proj);
    if (!usable || IsLikelyUiOrthoMatrix(view, proj)) { env->PopLocalFrame(nullptr); return; }

    double vX = 0, vY = 0, vZ = 0;
    if (g_renderManagerField && g_viewerPosXField && g_viewerPosYField && g_viewerPosZField) {
        jobject rm = env->GetObjectField(g_mcInstance, g_renderManagerField);
        if (rm) {
            vX = env->GetDoubleField(rm, g_viewerPosXField);
            vY = env->GetDoubleField(rm, g_viewerPosYField);
            vZ = env->GetDoubleField(rm, g_viewerPosZField);
            env->DeleteLocalRef(rm);
        }
    }

    ImDrawList* fg = ImGui::GetForegroundDrawList();

    struct ColorAgg18 { unsigned int color; int count; double nearest; float nsx, nsy; bool hasScreen; };
    std::vector<ColorAgg18> aggs;
    auto findAgg = [&](unsigned int col) -> ColorAgg18& {
        for (auto& a : aggs) if (a.color == col) return a;
        aggs.push_back({ col, 0, 1e9, 0, 0, false });
        return aggs.back();
    };

    for (const auto& b : blocks) {
        ColorAgg18& agg = findAgg(b.color);
        agg.count++;

        float csx = 0, csy = 0;
        bool okc = WorldToScreen(b.x - vX, b.y - vY, b.z - vZ, view, proj, w, h, csx, csy);
        if (okc && b.dist < agg.nearest) { agg.nearest = b.dist; agg.nsx = csx; agg.nsy = csy; agg.hasScreen = true; }

        if (boxes) {
            const double minX = b.x - 0.5, minY = b.y - 0.5, minZ = b.z - 0.5;
            const double maxX = b.x + 0.5, maxY = b.y + 0.5, maxZ = b.z + 0.5;
            double corners[8][3] = {
                {minX, minY, minZ}, {maxX, minY, minZ}, {maxX, maxY, minZ}, {minX, maxY, minZ},
                {minX, minY, maxZ}, {maxX, minY, maxZ}, {maxX, maxY, maxZ}, {minX, maxY, maxZ}
            };
            float left = 1e9f, top = 1e9f, right = -1e9f, bottom = -1e9f;
            bool projected = true;
            for (int c = 0; c < 8; c++) {
                float sx = 0, sy = 0;
                if (!WorldToScreen(corners[c][0] - vX, corners[c][1] - vY, corners[c][2] - vZ, view, proj, w, h, sx, sy)) { projected = false; break; }
                if (sx < left) left = sx; if (sy < top) top = sy;
                if (sx > right) right = sx; if (sy > bottom) bottom = sy;
            }
            if (projected && right > left && bottom > top) {
                left = (std::max)(left, 0.0f); top = (std::max)(top, 0.0f);
                right = (std::min)(right, (float)w); bottom = (std::min)(bottom, (float)h);
                if (right > left && bottom > top) {
                    fg->AddRectFilled(ImVec2(left, top), ImVec2(right, bottom), IM_COL32(0, 0, 0, 70));
                    fg->AddRect(ImVec2(left, top), ImVec2(right, bottom), b.color, 0.0f, 0, 1.5f);
                }
            }
        }
    }

    if (tracers) {
        ImVec2 origin((float)w * 0.5f, (float)h);
        for (const auto& a : aggs) {
            if (!a.hasScreen) continue;
            fg->AddLine(origin, ImVec2(a.nsx, a.nsy), a.color, 1.4f);
        }
    }

    if (hud && !aggs.empty()) {
        auto prettyId = [&](unsigned int col) -> std::string {
            for (const auto& t : targets) if (t.color == col) {
                std::string s = t.id;
                for (char& ch : s) if (ch == '_') ch = ' ';
                if (!s.empty()) s[0] = (char)std::toupper((unsigned char)s[0]);
                return s;
            }
            return "Block";
        };
        const float padX = 8.0f, padY = 6.0f, rowH = ImGui::GetFontSize() + 4.0f, sw = 10.0f;
        std::vector<std::string> rows; std::vector<unsigned int> rowColors;
        float maxTextW = 0.0f;
        for (const auto& a : aggs) {
            char buf[96];
            snprintf(buf, sizeof(buf), "%s  x%d  %.0fm", prettyId(a.color).c_str(), a.count, a.nearest);
            std::string row = buf;
            maxTextW = (std::max)(maxTextW, ImGui::CalcTextSize(row.c_str()).x);
            rows.push_back(row); rowColors.push_back(a.color);
        }
        std::string title = "Block ESP";
        float titleW = ImGui::CalcTextSize(title.c_str()).x;
        float contentW = (std::max)(titleW, sw + 6.0f + maxTextW) + padX * 2;
        float contentH = padY + rowH + rowH * (float)rows.size() + padY;

        lc::HudElementLayout beLayout;
        { LockGuard lk(g_configMutex); beLayout = g_hudLayout.Resolve(lc::ELEM_BLOCKESPLIST); }
        contentW *= beLayout.scale; contentH *= beLayout.scale;
        ImVec2 beTL = lc::HudElementPixelPos(beLayout, contentW, contentH, w, h);
        ImVec2 pMin(beTL.x, beTL.y), pMax(beTL.x + contentW, beTL.y + contentH);
        fg->AddRectFilled(pMin, pMax, IM_COL32(12, 14, 20, 210), 6.0f);
        fg->AddRect(pMin, pMax, IM_COL32(70, 80, 100, 220), 6.0f, 0, 1.0f);

        float curY = beTL.y + padY;
        fg->AddText(ImVec2(beTL.x + padX + 1, curY + 1), IM_COL32(0, 0, 0, 160), title.c_str());
        fg->AddText(ImVec2(beTL.x + padX, curY), IM_COL32(220, 230, 245, 255), title.c_str());
        curY += rowH;
        for (size_t i = 0; i < rows.size(); i++) {
            float swY = curY + (rowH - sw) * 0.5f;
            fg->AddRectFilled(ImVec2(beTL.x + padX, swY), ImVec2(beTL.x + padX + sw, swY + sw), rowColors[i], 2.0f);
            fg->AddText(ImVec2(beTL.x + padX + sw + 6.0f + 1, curY + 1), IM_COL32(0, 0, 0, 160), rows[i].c_str());
            fg->AddText(ImVec2(beTL.x + padX + sw + 6.0f, curY), IM_COL32(225, 228, 235, 255), rows[i].c_str());
            curY += rowH;
        }
    }

    env->PopLocalFrame(nullptr);
}

// ===================== INPUT HOOK =====================
// WndProc stays installed for reach's left-click edge detection.
LRESULT CALLBACK HookedWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    // HUD editor: only intercept mouse when editor active AND a GUI/chat is open
    // (chat open = OS cursor is available, so dragging works naturally).
    if (g_hudEditor.active) {
        bool chatOpen = false;
        { LockGuard lk(g_stateMutex); chatOpen = g_gameState.guiOpen; }
        if (chatOpen) {
            bool isMouseMsg = (msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP || msg == WM_MOUSEMOVE);
            if (isMouseMsg) {
                ImGui_ImplWin32_WndProcHandler(hwnd, msg, wParam, lParam);
                // Consume clicks while a drag is active so the game GUI doesn't react
                if (!g_hudEditor.grabbedId.empty() || msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP) {
                    return 0;
                }
            }
        }
    }

    bool leftNowDown = (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0;
    bool rawInputClickEdge = (msg == WM_INPUT) && leftNowDown && !g_reachRawInputPrevDown;
    if (msg == WM_INPUT) {
        g_reachRawInputPrevDown = leftNowDown;
    }

    if (msg == WM_LBUTTONDOWN || rawInputClickEdge) {
        Config cfgSnapshot;
        {
            LockGuard lk(g_configMutex);
            cfgSnapshot = g_config;
        }

        if (cfgSnapshot.reachEnabled) {
            GameState stateSnapshot;
            {
                LockGuard lk(g_stateMutex);
                stateSnapshot = g_gameState;
            }

            JNIEnv* env = JniEnv::Get(g_jvm);
            if (env) {
                TryLockGuard jniTry(g_stateJniMutex);
                if (jniTry.owns_lock()) {
                    UpdateReach(env, cfgSnapshot, stateSnapshot, true);
                }
            }
        }
    }

    return CallWindowProcA(g_origWndProc, hwnd, msg, wParam, lParam);
}

void EnsureWndProcHook(HWND targetHwnd) {
    if (!targetHwnd) return;
    if (g_wndProcHookedHwnd == targetHwnd && g_origWndProc) return;

    if (g_wndProcHookedHwnd && g_origWndProc) {
        SetWindowLongPtrA(g_wndProcHookedHwnd, GWLP_WNDPROC, (LONG_PTR)g_origWndProc);
    }

    WNDPROC previous = (WNDPROC)SetWindowLongPtrA(targetHwnd, GWLP_WNDPROC, (LONG_PTR)HookedWndProc);
    if (previous) {
        g_origWndProc = previous;
        g_wndProcHookedHwnd = targetHwnd;
        Log("WndProc hooked/refreshed");
    }
}

static BOOL CallOriginalSwapBuffers(HDC hdc) {
    if (!g_origSwapBuffers) {
        HMODULE hGdi = GetModuleHandleA("gdi32.dll");
        if (hGdi) g_origSwapBuffers = (SwapBuffersFn)GetProcAddress(hGdi, "SwapBuffers");
    }
    return g_origSwapBuffers ? g_origSwapBuffers(hdc) : FALSE;
}

static void ConfigureImGuiFontAndStyle(HWND hwnd) {
    ImGuiIO& io = ImGui::GetIO();
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
    io.IniFilename = nullptr;

    float dpiScale = 1.0f;
    UINT dpi = 96;
    HMODULE hUser32 = GetModuleHandleA("user32.dll");
    if (hUser32) {
        typedef UINT (WINAPI* FnGetDpiForWindow)(HWND);
        FnGetDpiForWindow fn = (FnGetDpiForWindow)GetProcAddress(hUser32, "GetDpiForWindow");
        if (fn && hwnd) dpi = fn(hwnd);
    }
    dpiScale = (dpi > 0) ? ((float)dpi / 96.0f) : 1.0f;
    if (dpiScale < 0.75f) dpiScale = 0.75f;
    if (dpiScale > 2.5f) dpiScale = 2.5f;

    io.Fonts->Clear();
    ImFontConfig fontCfg;
    fontCfg.RasterizerDensity = 1.0f;
    fontCfg.SizePixels = 16.0f * dpiScale;
    fontCfg.OversampleH = 3;
    fontCfg.OversampleV = 2;
    fontCfg.PixelSnapH = true;

    ImFont* loadedFont = nullptr;
    std::string bridgeDir = GetBridgeDir();
    std::vector<std::string> fontCandidates = {
        bridgeDir + "\\minecraftia.ttf",
        bridgeDir + "\\Minecraftia.ttf",
        bridgeDir + "\\Data\\minecraftia.ttf",
        bridgeDir + "\\Data\\Minecraftia.ttf",
        "C:\\Windows\\Fonts\\minecraftia.ttf",
        "C:\\Windows\\Fonts\\Minecraftia.ttf"
    };

    for (const std::string& fontPath : fontCandidates) {
        if (!FileExistsLocal(fontPath) || !IsLikelyFontBinary(fontPath)) continue;
        loadedFont = io.Fonts->AddFontFromFileTTF(fontPath.c_str(), fontCfg.SizePixels, &fontCfg);
        if (loadedFont) {
            Log("Loaded ImGui font: " + fontPath);
            break;
        }
    }
    if (!loadedFont) {
        io.Fonts->AddFontDefault(&fontCfg);
        Log("Minecraftia not found, using default ImGui font.");
    }

    io.FontGlobalScale = 1.0f;
    ImGuiStyle& st = ImGui::GetStyle();
    st.WindowRounding = 0.0f;
    st.ChildRounding = 0.0f;
    st.FrameRounding = 2.0f;
    st.GrabRounding = 2.0f;
    st.ScrollbarRounding = 0.0f;
    st.ScaleAllSizes(dpiScale);
}

// ===================== SWAPBUFFERS HOOK =====================
BOOL WINAPI HookedSwapBuffers(HDC hdc) {
    if (!hdc) return CallOriginalSwapBuffers(hdc);

    HGLRC currentRc = wglGetCurrentContext();
    if (!currentRc) return CallOriginalSwapBuffers(hdc);

    HWND currentHwnd = WindowFromDC(hdc);
    if (!currentHwnd || !IsWindow(currentHwnd)) return CallOriginalSwapBuffers(hdc);
    g_gameHwnd = currentHwnd;

    if (g_imguiPhase1Done && g_imguiHwnd && currentHwnd != g_imguiHwnd) {
        ResetImGuiBackendsForReinit("window changed");
        return CallOriginalSwapBuffers(hdc);
    }

    if (!g_imguiPhase1Done) {
        ImGui::CreateContext();
        ConfigureImGuiFontAndStyle(currentHwnd);
        ImGui_ImplWin32_InitForOpenGL(currentHwnd);
        EnsureWndProcHook(currentHwnd);

        g_imguiPhase1Done = true;
        g_imguiWarmupFrames = 3;
        g_imguiGlrc = currentRc;
        g_imguiHwnd = currentHwnd;
        Log("ImGui phase-1 done for legacy bridge (context + Win32).");
        return CallOriginalSwapBuffers(hdc);
    }

    EnsureWndProcHook(currentHwnd);

    if (g_imguiWarmupFrames > 0) {
        g_imguiWarmupFrames--;
        return CallOriginalSwapBuffers(hdc);
    }

    if (g_imguiInitialized && g_imguiGlBackendReady && g_imguiGlrc && currentRc != g_imguiGlrc) {
        Log("OpenGL context changed; scheduling legacy ImGui backend reset.");
        g_imguiPendingBackendReset = true;
        g_imguiPendingGlrc = currentRc;
        g_imguiWarmupFrames = 3;
        return CallOriginalSwapBuffers(hdc);
    }

    if (g_imguiPendingBackendReset) {
        Log("ImGui legacy: executing deferred OpenGL backend shutdown (skip GL deletes)");
        ImGui_ImplOpenGL3_SetSkipGLDeletes(true);
        ImGui_ImplOpenGL3_Shutdown();
        ImGui_ImplOpenGL3_SetSkipGLDeletes(false);
        g_imguiGlBackendReady = false;
        g_imguiInitialized = false;
        g_glInitialized = false;
        g_imguiGlrc = g_imguiPendingGlrc;
        g_imguiPendingBackendReset = false;
        g_imguiWarmupFrames = 3;
        return CallOriginalSwapBuffers(hdc);
    }

    if (g_imguiInitialized && !ImGui::GetCurrentContext()) {
        ResetImGuiBackendsForReinit("missing ImGui context");
        return CallOriginalSwapBuffers(hdc);
    }

    if (!g_imguiInitialized) {
        static bool glModernLoadedLogged = false;
        if (!glModernLoadedLogged) {
            bool ok = LoadModernOpenGL();
            Log(std::string("Legacy ImGui OpenGL loader: ") + (ok ? "ok" : "FAILED"));
            glModernLoadedLogged = true;
        }

        GLint last_program = 0;
        GLint last_active_texture = 0;
        GLint last_texture_2d = 0;
        GLint last_array_buffer = 0;
        GLint last_element_array_buffer = 0;
        GLint last_vertex_array = 0;
        GLint last_pixel_unpack_buffer = 0;

        glGetIntegerv(GL_CURRENT_PROGRAM, &last_program);
        glGetIntegerv(GL_ACTIVE_TEXTURE, &last_active_texture);
        glGetIntegerv(GL_TEXTURE_BINDING_2D, &last_texture_2d);
        glGetIntegerv(GL_ARRAY_BUFFER_BINDING, &last_array_buffer);
        glGetIntegerv(GL_ELEMENT_ARRAY_BUFFER_BINDING, &last_element_array_buffer);
        if (glBindVertexArray_) glGetIntegerv(GL_VERTEX_ARRAY_BINDING, &last_vertex_array);
        if (glBindBuffer_) glGetIntegerv(GL_PIXEL_UNPACK_BUFFER_BINDING, &last_pixel_unpack_buffer);

        // Let the backend choose the GLSL version for 1.8.9's older GL context.
        ImGui_ImplOpenGL3_Init(nullptr);

        if (glUseProgram_) glUseProgram_((GLuint)last_program);
        if (glActiveTexture_) glActiveTexture_((GLenum)last_active_texture);
        glBindTexture(GL_TEXTURE_2D, (GLuint)last_texture_2d);
        if (glBindBuffer_) {
            glBindBuffer_(GL_ARRAY_BUFFER, (GLuint)last_array_buffer);
            glBindBuffer_(GL_ELEMENT_ARRAY_BUFFER, (GLuint)last_element_array_buffer);
            glBindBuffer_(GL_PIXEL_UNPACK_BUFFER, (GLuint)last_pixel_unpack_buffer);
        }
        if (glBindVertexArray_) glBindVertexArray_((GLuint)last_vertex_array);

        g_imguiGlBackendReady = true;
        g_imguiInitialized = true;
        g_glInitialized = true;
        g_imguiGlrc = currentRc;
        g_imguiWarmupFrames = 3;
        Log("ImGui phase-2 done for legacy bridge (OpenGL backend).");
        return CallOriginalSwapBuffers(hdc);
    }

    RECT rect{};
    if (!GetClientRect(currentHwnd, &rect)) return CallOriginalSwapBuffers(hdc);
    int w = rect.right - rect.left;
    int h = rect.bottom - rect.top;
    if (w <= 0 || h <= 0) return CallOriginalSwapBuffers(hdc);

    GameState state;
    { LockGuard lk(g_stateMutex); state = g_gameState; }

    static bool s_wasNonChatMinecraftScreen = false;
    bool nonChatScreenOpen = ShouldHideWorldRenderModules(state);
    if (nonChatScreenOpen) {
        s_wasNonChatMinecraftScreen = true;
    } else if (s_wasNonChatMinecraftScreen && g_imguiInitialized) {
        s_wasNonChatMinecraftScreen = false;
        ResetImGuiBackendsForReinit("minecraft screen closed");
        return CallOriginalSwapBuffers(hdc);
    }

    // Capture game-space matrices before ImGui renders and restores GL state.
    CaptureCurrentRenderMatrices();

    ImGui_ImplOpenGL3_NewFrame();
    ImGui_ImplWin32_NewFrame();
    ImGui::NewFrame();

    UpdateHudEditor(w, h);
    RenderHUD(w, h);
    if (!ShouldHideWorldRenderModules(state)) {
        TryLockGuard jniTry(g_renderJniMutex);
        if (jniTry.owns_lock()) {
            RenderNametags(w, h);
            RenderClosestPlayerInfo(w, h);
            RenderPixelPartyAssist(w, h);
            RenderChestESP(w, h);
            RenderBlockESP(w, h);
        } else {
            static DWORD nextJniBusyLogAt = 0;
            DWORD now = GetTickCount();
            if (now >= nextJniBusyLogAt) {
                nextJniBusyLogAt = now + 15000;
                bool heavy = (InterlockedCompareExchange(&g_heavyDiscoveryInProgress, 0, 0) != 0);
                Log(std::string("Skipped world overlay frame: JNI busy")
                    + (heavy ? " (heavy discovery active)" : ""));
            }
        }
    }

    ImGui::Render();
    ImGuiIO& io = ImGui::GetIO();
    if (io.DisplaySize.x > 1.0f && io.DisplaySize.y > 1.0f) {
        ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
    }
    glFlush();

    return CallOriginalSwapBuffers(hdc);
}

// IAT hook: patch import table entries for SwapBuffers in a specific module
bool PatchIAT(HMODULE hModule, const char* targetDll, const char* funcName, void* hookFunc, void** origFunc) {
    BYTE* base = (BYTE*)hModule;
    IMAGE_DOS_HEADER* dosHdr = (IMAGE_DOS_HEADER*)base;
    if (dosHdr->e_magic != IMAGE_DOS_SIGNATURE) return false;
    IMAGE_NT_HEADERS* ntHdr = (IMAGE_NT_HEADERS*)(base + dosHdr->e_lfanew);
    if (ntHdr->Signature != IMAGE_NT_SIGNATURE) return false;

    IMAGE_DATA_DIRECTORY* importDir = &ntHdr->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (!importDir->VirtualAddress || !importDir->Size) return false;

    IMAGE_IMPORT_DESCRIPTOR* imp = (IMAGE_IMPORT_DESCRIPTOR*)(base + importDir->VirtualAddress);
    for (; imp->Name; imp++) {
        const char* dllName = (const char*)(base + imp->Name);
        if (lstrcmpiA(dllName, targetDll) != 0) continue;

        IMAGE_THUNK_DATA* origThunk = (IMAGE_THUNK_DATA*)(base + imp->OriginalFirstThunk);
        IMAGE_THUNK_DATA* firstThunk = (IMAGE_THUNK_DATA*)(base + imp->FirstThunk);

        for (; origThunk->u1.AddressOfData; origThunk++, firstThunk++) {
            if (origThunk->u1.Ordinal & IMAGE_ORDINAL_FLAG64) continue;
            IMAGE_IMPORT_BY_NAME* importByName = (IMAGE_IMPORT_BY_NAME*)(base + origThunk->u1.AddressOfData);
            if (strcmp((const char*)importByName->Name, funcName) == 0) {
                *origFunc = (void*)firstThunk->u1.Function;
                DWORD oldProt;
                VirtualProtect(&firstThunk->u1.Function, sizeof(void*), PAGE_EXECUTE_READWRITE, &oldProt);
                firstThunk->u1.Function = (ULONGLONG)hookFunc;
                VirtualProtect(&firstThunk->u1.Function, sizeof(void*), oldProt, &oldProt);
                return true;
            }
        }
    }
    return false;
}

// AntiDebuff approach A: neutralize the blindness fog at the OpenGL layer without removing
// the BLINDNESS effect (so sprint/crit gating stays in sync with the server). 1.8.9 uses
// fixed-function fog: when blind, EntityRenderer.setupFog sets GL_FOG_START ~0 and
// GL_FOG_END to a few blocks, darkening the view. While the effect is active (flag set by
// the bg JNI thread) we push the linear fog band far beyond render distance so it is not
// visible. All other fog (and all calls while not blind) pass through untouched.
typedef void (WINAPI* glFogfFn)(GLenum, GLfloat);
static glFogfFn g_origGlFogf = nullptr;

void WINAPI HookedGlFogf(GLenum pname, GLfloat param) {
    if (g_blindFogSuppress && g_origGlFogf) {
        if (pname == GL_FOG_START) { g_origGlFogf(GL_FOG_START, 900.0f);  return; }
        if (pname == GL_FOG_END)   { g_origGlFogf(GL_FOG_END,   1000.0f); return; }
    }
    if (g_origGlFogf) g_origGlFogf(pname, param);
}

void InstallSwapBuffersHook() {
    MH_STATUS mhInit = MH_Initialize();
    if (mhInit == MH_OK || mhInit == MH_ERROR_ALREADY_INITIALIZED) {
        g_minhookInitialized = true;

        // Hook opengl32!glFogf so AntiDebuff can hide blindness fog without removing the
        // effect. Installed here (before the gdi32 hook returns on success) so it is set up
        // whenever MinHook initializes.
        if (!g_origGlFogf) {
            HMODULE hGl = GetModuleHandleA("opengl32.dll");
            if (hGl) {
                void* pFogf = (void*)GetProcAddress(hGl, "glFogf");
                if (pFogf) {
                    MH_STATUS cs = MH_CreateHook(pFogf, (void*)HookedGlFogf, (void**)&g_origGlFogf);
                    MH_STATUS es = (cs == MH_OK) ? MH_EnableHook(pFogf) : cs;
                    if (cs == MH_OK && es == MH_OK) {
                        Log("MinHook hooked opengl32!glFogf for AntiDebuff blindness-fog suppression.");
                    } else {
                        Log("WARNING: glFogf hook failed create=" + std::to_string((int)cs)
                            + " enable=" + std::to_string((int)es));
                    }
                }
            }
        }

        HMODULE hGdiHook = GetModuleHandleA("gdi32.dll");
        if (hGdiHook) {
            void* pSwap = (void*)GetProcAddress(hGdiHook, "SwapBuffers");
            if (pSwap) {
                MH_STATUS createStatus = MH_CreateHook(pSwap, (void*)HookedSwapBuffers, (void**)&g_origSwapBuffers);
                MH_STATUS enableStatus = (createStatus == MH_OK) ? MH_EnableHook(pSwap) : createStatus;
                if (createStatus == MH_OK && enableStatus == MH_OK) {
                    Log("MinHook hooked gdi32!SwapBuffers for legacy ImGui rendering.");
                    return;
                }
                Log("WARNING: MinHook SwapBuffers hook failed, falling back to IAT scan. create="
                    + std::to_string((int)createStatus) + " enable=" + std::to_string((int)enableStatus));
            }
        }
    } else {
        Log("WARNING: MinHook init failed, falling back to IAT scan. status=" + std::to_string((int)mhInit));
    }

    // Enumerate ALL loaded modules and find the one that imports SwapBuffers
    HANDLE hProc = GetCurrentProcess();
    HMODULE hMods[1024];
    DWORD cbNeeded = 0;
    bool hooked = false;

    // Use K32EnumProcessModules (available since Windows 7)
    typedef BOOL(WINAPI* EnumModsFn)(HANDLE, HMODULE*, DWORD, LPDWORD);
    HMODULE hPsapi = GetModuleHandleA("kernel32.dll");
    EnumModsFn enumMods = (EnumModsFn)GetProcAddress(hPsapi, "K32EnumProcessModules");
    if (!enumMods) {
        hPsapi = LoadLibraryA("psapi.dll");
        if (hPsapi) enumMods = (EnumModsFn)GetProcAddress(hPsapi, "EnumProcessModules");
    }

    if (enumMods && enumMods(hProc, hMods, sizeof(hMods), &cbNeeded)) {
        int modCount = cbNeeded / sizeof(HMODULE);
        Log("Scanning " + std::to_string(modCount) + " modules for SwapBuffers import...");

        for (int i = 0; i < modCount && !hooked; i++) {
            // Skip our own DLL and known system DLLs that we don't want to hook
            char modName[MAX_PATH] = "";
            GetModuleFileNameA(hMods[i], modName, MAX_PATH);
            std::string name = modName;
            
            // Convert to lowercase for comparison
            std::string nameLower = name;
            std::transform(nameLower.begin(), nameLower.end(), nameLower.begin(), ::tolower);
            
            // Skip system DLLs that won't have SwapBuffers
            if (nameLower.find("\\windows\\") != std::string::npos &&
                nameLower.find("opengl32.dll") == std::string::npos) continue;

            void* origFunc = nullptr;
            const char* gdiVariants[] = { "gdi32.dll", "GDI32.dll", "GDI32.DLL", "Gdi32.dll", nullptr };
            for (int g = 0; gdiVariants[g] && !hooked; g++) {
                if (PatchIAT(hMods[i], gdiVariants[g], "SwapBuffers", (void*)HookedSwapBuffers, &origFunc)) {
                    g_origSwapBuffers = (SwapBuffersFn)origFunc;
                    // Get just the filename for logging
                    std::string shortName = name;
                    size_t lastSlash = shortName.find_last_of("\\/");
                    if (lastSlash != std::string::npos) shortName = shortName.substr(lastSlash + 1);
                    Log("IAT hooked SwapBuffers in: " + shortName + " (via " + gdiVariants[g] + ")");
                    hooked = true;
                }
            }
        }
    } else {
        Log("WARNING: EnumProcessModules failed, trying known modules...");
        // Fallback to known module names
        const char* modules[] = { "opengl32.dll", "lwjgl.dll", "lwjgl64.dll", nullptr };
        for (int i = 0; modules[i] && !hooked; i++) {
            HMODULE hMod = GetModuleHandleA(modules[i]);
            if (!hMod) continue;
            void* origFunc = nullptr;
            if (PatchIAT(hMod, "gdi32.dll", "SwapBuffers", (void*)HookedSwapBuffers, &origFunc)) {
                g_origSwapBuffers = (SwapBuffersFn)origFunc;
                Log(std::string("IAT hooked SwapBuffers in ") + modules[i]);
                hooked = true;
            }
        }
    }

    if (!hooked) {
        HMODULE hGdi = GetModuleHandleA("gdi32.dll");
        g_origSwapBuffers = (SwapBuffersFn)GetProcAddress(hGdi, "SwapBuffers");
        Log("WARNING: Could not install IAT hook on any module. HUD will not render.");
    } else {
        Log("SwapBuffers hook installed successfully");
    }
}

// ===================== TCP SERVER =====================
void ParseConfig(const std::string& line) {
    lc::SimpleJsonConfigReader reader(line);

    std::string type = reader.GetString("type");
    if (type == "config") {
        LockGuard lk(g_configMutex);
        g_config.armed = reader.GetBool("armed");
        g_config.clicking = reader.GetBool("clicking");
        g_config.minCPS = reader.GetFloat("minCPS");
        g_config.maxCPS = reader.GetFloat("maxCPS");
        g_config.leftClick = reader.GetBool("left");
        g_config.rightClick = reader.GetBool("right");
        g_config.rightMinCPS = reader.GetFloat("rightMinCPS");
        g_config.rightMaxCPS = reader.GetFloat("rightMaxCPS");
        g_config.rightBlockOnly = reader.GetBool("rightBlock");
        g_config.breakBlocks = reader.GetBool("breakBlocks");
        g_config.jitter = reader.GetBool("jitter");
        g_config.clickInChests = reader.GetBool("clickInChests");
        g_config.aimAssist = reader.GetBool("aimAssist");
        g_config.triggerbot = reader.GetBool("triggerbot");
        g_config.killAura = reader.GetBool("killAura");
        g_config.killAuraRecordCps = reader.GetString("killAuraCpsMode") == "record";
        g_config.killAuraMode = reader.GetString("killAuraMode") == "single" ? killaura::TARGET_SINGLE : killaura::TARGET_SWITCH;
        { std::string v = reader.GetString("killAuraSort"); g_config.killAuraSort = v == "distance" ? 0 : v == "hurttime" ? 2 : v == "fov" ? 3 : 1; }
        {
            static const char* modes[] = {"none","vanilla","spoof","hypixel","blink","interact","swap","legit","fake","morden"};
            std::string v = reader.GetString("killAuraAutoBlock"); g_config.killAuraAutoBlock = killaura::BLOCK_HYPIXEL;
            for (int i = 0; i < 10; ++i) if (v == modes[i]) { g_config.killAuraAutoBlock = i; break; }
        }
        g_config.killAuraAttackTick = lc::ClampInt(reader.GetInt("killAuraAttackTick", 0), 0, 5);
        g_config.killAuraAutoBlockRequirePress = reader.GetBool("killAuraAutoBlockRequirePress");
        g_config.killAuraAutoBlockCps = lc::ClampFloat(reader.GetFloat("killAuraAutoBlockCps", 8), 1, 10);
        g_config.killAuraAutoBlockRange = lc::ClampFloat(reader.GetFloat("killAuraAutoBlockRange", 6), 3, 8);
        g_config.killAuraSwingRange = lc::ClampFloat(reader.GetFloat("killAuraSwingRange", 3.5f), 3, 6);
        g_config.killAuraAttackRange = lc::ClampFloat(reader.GetFloat("killAuraAttackRange", 3), 3, 6);
        g_config.killAuraFov = lc::ClampInt(reader.GetInt("killAuraFov", 360), 30, 360);
        g_config.killAuraMinCps = lc::ClampInt(reader.GetInt("killAuraMinCps", 14), 1, 20);
        g_config.killAuraMaxCps = lc::ClampInt(reader.GetInt("killAuraMaxCps", 14), 1, 20);
        if (g_config.killAuraMinCps > g_config.killAuraMaxCps) g_config.killAuraMinCps = g_config.killAuraMaxCps;
        g_config.killAuraSwitchDelayMs = lc::ClampInt(reader.GetInt("killAuraSwitchDelay", 150), 0, 1000);
        { std::string v = reader.GetString("killAuraRotations"); g_config.killAuraRotMode = v == "none" ? 0 : v == "legit" ? 1 : v == "lockview" ? 3 : v == "liquidbounce" ? 4 : v == "hypixel" ? 5 : 2; }
        g_config.killAuraDeadZone = lc::ClampFloat(reader.GetFloat("killAuraDeadZone", .5f), 0, 2);
        g_config.killAuraMaxTurnSpeed = lc::ClampFloat(reader.GetFloat("killAuraMaxTurnSpeed", 25), 5, 180);
        g_config.killAuraMinTurnSpeed = lc::ClampFloat(reader.GetFloat("killAuraMinTurnSpeed", 5), 1, 90);
        g_config.killAuraAcceleration = lc::ClampFloat(reader.GetFloat("killAuraAcceleration", 2.5f), .1f, 10);
        g_config.killAuraDeceleration = lc::ClampFloat(reader.GetFloat("killAuraDeceleration", 1.5f), .1f, 10);
        g_config.killAuraUseOvershoot = reader.GetBool("killAuraUseOvershoot", true);
        g_config.killAuraOvershootStrength = lc::ClampFloat(reader.GetFloat("killAuraOvershootStrength", 5), 0, 20);
        g_config.killAuraOvershootRecovery = lc::ClampFloat(reader.GetFloat("killAuraOvershootRecovery", .2f), .01f, 1);
        g_config.killAuraNoiseStrength = lc::ClampFloat(reader.GetFloat("killAuraNoiseStrength", .2f), 0, 2);
        g_config.killAuraVisualizeAim = reader.GetBool("killAuraVisualizeAim", true);
        g_config.killAuraSmoothBack = reader.GetBool("killAuraSmoothBack", true);
        { std::string v = reader.GetString("killAuraMoveFix"); g_config.killAuraMoveFix = v == "none" ? 0 : v == "strict" ? 2 : 1; }
        g_config.killAuraSmoothing = lc::ClampInt(reader.GetInt("killAuraSmoothing", 0), 0, 100);
        g_config.killAuraRavenSmoothing = lc::ClampInt(reader.GetInt("killAuraRavenSmoothing", 0), 0, 10);
        g_config.killAuraRavenPredictTicks = lc::ClampInt(reader.GetInt("killAuraRavenPredictTicks", 0), 0, 5);
        g_config.killAuraRavenYawRandom = lc::ClampInt(reader.GetInt("killAuraRavenYawRandom", 0), 0, 5);
        g_config.killAuraAngleStep = lc::ClampInt(reader.GetInt("killAuraAngleStep", 90), 30, 180);
        g_config.killAuraThroughWalls = reader.GetBool("killAuraThroughWalls", true);
        g_config.killAuraRequirePress = reader.GetBool("killAuraRequirePress");
        g_config.killAuraAllowMining = reader.GetBool("killAuraAllowMining", true);
        g_config.killAuraWeaponsOnly = reader.GetBool("killAuraWeaponsOnly", true);
        g_config.killAuraAllowTools = reader.GetBool("killAuraAllowTools");
        g_config.killAuraInventoryCheck = reader.GetBool("killAuraInventoryCheck", true);
        g_config.killAuraBotCheck = reader.GetBool("killAuraBotCheck", true);
        g_config.killAuraPlayers = reader.GetBool("killAuraPlayers", true);
        g_config.killAuraBosses = reader.GetBool("killAuraBosses");
        g_config.killAuraMobs = reader.GetBool("killAuraMobs");
        g_config.killAuraAnimals = reader.GetBool("killAuraAnimals");
        g_config.killAuraGolems = reader.GetBool("killAuraGolems");
        g_config.killAuraSilverfish = reader.GetBool("killAuraSilverfish");
        g_config.killAuraTeams = reader.GetBool("killAuraTeams", true);
        g_config.killAuraShowTarget = reader.GetString("killAuraShowTarget") == "default";
        g_config.killAuraDebugHealth = reader.GetString("killAuraDebug") == "health";
        g_config.killAuraRandomize = reader.GetBool("killAuraRandomize", true);
        g_config.killAuraRandomizeRange = lc::ClampFloat(reader.GetFloat("killAuraRandomizeRange", .4f), 0, 1);
        g_config.killAuraYRandomize = lc::ClampFloat(reader.GetFloat("killAuraYRandomize", .3f), 0, 1);
        g_config.killAuraLbHorizontalSpeed = lc::ClampFloat(reader.GetFloat("killAuraLbHorizontalSpeed", 180), 1, 180);
        g_config.killAuraLbVerticalSpeed = lc::ClampFloat(reader.GetFloat("killAuraLbVerticalSpeed", 180), 1, 180);
        g_config.killAuraLbSmooth = lc::ClampFloat(reader.GetFloat("killAuraLbSmooth", .5f), .1f, 1);
        g_config.killAuraLbPredict = reader.GetBool("killAuraLbPredict", true);
        g_config.killAuraLbPredictSize = lc::ClampFloat(reader.GetFloat("killAuraLbPredictSize", 1), 0, 3);
        g_config.killAuraLbRandomize = reader.GetBool("killAuraLbRandomize", true);
        g_config.killAuraLbRandomRange = lc::ClampFloat(reader.GetFloat("killAuraLbRandomRange", .5f), 0, 1);
        g_config.killAuraLbHorizontalSearch = lc::ClampFloat(reader.GetFloat("killAuraLbHorizontalSearch", .5f), 0, 1);
        g_config.killAuraLbBodyMin = lc::ClampFloat(reader.GetFloat("killAuraLbBodyMin", .1f), 0, 1);
        g_config.killAuraLbBodyMax = lc::ClampFloat(reader.GetFloat("killAuraLbBodyMax", .9f), 0, 1);
        g_config.speedBridge = reader.GetBool("speedBridge");
        g_config.speedBridgeBlockOnly = reader.GetBool("speedBridgeBlockOnly");
        g_config.speedBridgeHoldingShiftOnly = reader.GetBool("speedBridgeHoldingShiftOnly");
        g_config.speedBridgeLookingDownOnly = reader.GetBool("speedBridgeLookingDownOnly");
        g_config.gtbHelper = reader.GetBool("gtbHelper");
        g_config.nametags = reader.GetBool("nametags");
        g_config.nickHiderEnabled = reader.GetBool("nickHiderEnabled");
        g_config.nickHiderAlias = lc::NormalizeNickHiderAlias(reader.GetString("nickHiderAlias"));
        if (g_config.nickHiderAlias.empty()) g_config.nickHiderEnabled = false;
        lc::ConfigureNickHiderJvmti(g_config.nickHiderEnabled, g_config.nickHiderAlias);
        static bool lastNickHiderEnabled = false;
        static std::string lastNickHiderAlias;
        if (g_config.nickHiderEnabled != lastNickHiderEnabled || g_config.nickHiderAlias != lastNickHiderAlias) {
            lastNickHiderEnabled = g_config.nickHiderEnabled;
            lastNickHiderAlias = g_config.nickHiderAlias;
            Log("NickHider config: enabled=" + std::string(g_config.nickHiderEnabled ? "true" : "false")
                + " aliasLength=" + std::to_string(g_config.nickHiderAlias.size()));
        }
        g_config.closestPlayerInfo = reader.GetBool("closestPlayerInfo");
        g_config.nametagShowHealth = reader.GetBool("nametagShowHealth");
        g_config.nametagShowArmor = reader.GetBool("nametagShowArmor");
        g_config.nametagHideVanilla = reader.GetBool("nametagHideVanilla");
        g_config.chestEsp = reader.GetBool("chestEsp");
        g_config.chestStealer = reader.GetBool("chestStealerEnabled");
        g_config.blockEsp = reader.GetBool("blockEspEnabled");
        g_config.blockEspBoxes = reader.GetBool("blockEspBoxes", true);
        g_config.blockEspTracers = reader.GetBool("blockEspTracers");
        g_config.blockEspHud = reader.GetBool("blockEspHud", true);
        {
            int v = reader.GetInt("blockEspMaxCount", -1);
            if (v >= 1) g_config.blockEspMaxCount = (v > 512) ? 512 : v;
            int r = reader.GetInt("blockEspRange", -1);
            if (r >= 1) g_config.blockEspRange = (r > 8) ? 8 : r;
            std::string blocksRaw = reader.GetString("blockEspBlocks");
            std::vector<lc::BlockEspTargetDef> parsed = lc::ParseBlockEspTargets(blocksRaw);
            LockGuard tlk(g_blockEspTargetsMutex);
            // The loader re-sends the full config ~5x/sec. Only bump the version (which
            // invalidates the per-chunk scan cache) when the target set actually changes;
            // otherwise the round-robin scan never accumulates before the next wipe and the
            // module appears to do nothing.
            bool changed = parsed.size() != g_blockEspTargets.size();
            if (!changed) {
                for (size_t i = 0; i < parsed.size(); i++) {
                    if (parsed[i].id != g_blockEspTargets[i].id ||
                        parsed[i].color != g_blockEspTargets[i].color) { changed = true; break; }
                }
            }
            if (changed) {
                g_blockEspTargets.swap(parsed);
                InterlockedIncrement(&g_blockEspTargetsVersion);
            }
        }
        g_config.reachEnabled = reader.GetBool("reachEnabled");
        g_config.velocityEnabled = reader.GetBool("velocityEnabled");
        g_config.hitDelayFixEnabled = reader.GetBool("hitDelayFixEnabled");

        std::string showModuleListRaw = reader.GetString("showModuleList");
        g_config.showModuleList = showModuleListRaw.empty() ? true : (showModuleListRaw == "true");

        int style = reader.GetInt("moduleListStyle", -1);
        if (style >= 0) g_config.moduleListStyle = (std::max)(0, (std::min)(4, style));

        std::string showLogoRaw = reader.GetString("showLogo");
        g_config.showLogo = showLogoRaw.empty() ? true : (showLogoRaw == "true");

        std::string guiThemeRaw = reader.GetString("guiTheme");
        g_config.guiTheme = guiThemeRaw.empty() ? "Default" : guiThemeRaw;

        g_config.nametagShowHealth = reader.GetBool("nametagShowHealth");
        g_config.nametagShowArmor = reader.GetBool("nametagShowArmor");
        g_config.nametagHideVanilla = reader.GetBool("nametagHideVanilla");

        int nametagMaxCount = reader.GetInt("nametagMaxCount", -1);
        if (nametagMaxCount < 1) nametagMaxCount = g_config.nametagMaxCount;
        if (nametagMaxCount > 20) nametagMaxCount = 20;
        g_config.nametagMaxCount = nametagMaxCount;

        int chestEspMaxCount = reader.GetInt("chestEspMaxCount", -1);
        if (chestEspMaxCount < 1) chestEspMaxCount = g_config.chestEspMaxCount;
        if (chestEspMaxCount > 20) chestEspMaxCount = 20;
        g_config.chestEspMaxCount = chestEspMaxCount;

        int reachChance = reader.GetInt("reachChance", -1);
        if (reachChance < 1) reachChance = g_config.reachChance;
        if (reachChance > 100) reachChance = 100;
        g_config.reachChance = reachChance;

        int velocityHorizontal = reader.GetInt("velocityHorizontal", -1);
        if (velocityHorizontal < 1) velocityHorizontal = g_config.velocityHorizontal;
        if (velocityHorizontal > 100) velocityHorizontal = 100;
        g_config.velocityHorizontal = velocityHorizontal;

        int velocityVertical = reader.GetInt("velocityVertical", -1);
        if (velocityVertical < 1) velocityVertical = g_config.velocityVertical;
        if (velocityVertical > 100) velocityVertical = 100;
        g_config.velocityVertical = velocityVertical;

        int velocityChance = reader.GetInt("velocityChance", -1);
        if (velocityChance < 1) velocityChance = g_config.velocityChance;
        if (velocityChance > 100) velocityChance = 100;
        g_config.velocityChance = velocityChance;

        g_config.antiDebuffEnabled = reader.GetBool("antiDebuffEnabled");

        int speedBridgeDelayMs = reader.GetInt("speedBridgeDelayMs", -1);
        if (speedBridgeDelayMs < 20) speedBridgeDelayMs = g_config.speedBridgeDelayMs;
        if (speedBridgeDelayMs > 250) speedBridgeDelayMs = 250;
        g_config.speedBridgeDelayMs = speedBridgeDelayMs;

        int chestStealerDelayMs = reader.GetInt("chestStealerDelayMs", -1);
        if (chestStealerDelayMs < 50) chestStealerDelayMs = g_config.chestStealerDelayMs;
        if (chestStealerDelayMs > 500) chestStealerDelayMs = 500;
        g_config.chestStealerDelayMs = chestStealerDelayMs;

        std::string gtbHint = reader.GetString("gtbHint");
        int gtbCount = reader.GetInt("gtbCount", g_config.gtbCount);
        if (gtbCount < 0) gtbCount = g_config.gtbCount;
        std::string gtbPreview = reader.GetString("gtbPreview");
        if (!gtbHint.empty()) g_config.gtbHint = gtbHint;
        g_config.gtbCount = gtbCount;
        if (!gtbPreview.empty()) g_config.gtbPreview = gtbPreview;

        float reachMin = reader.GetFloat("reachMin");
        float reachMax = reader.GetFloat("reachMax");
        if (reachMin <= 0.0f) reachMin = g_config.reachMin;
        if (reachMax < reachMin) reachMax = reachMin;
        g_config.reachMin = reachMin;
        g_config.reachMax = reachMax;

        static bool lastNametagsLogged = false;
        if (g_config.nametags != lastNametagsLogged) {
            lastNametagsLogged = g_config.nametags;
            Log(std::string("Config: nametags=") + (g_config.nametags ? "true" : "false"));
        }

        static bool loggedExtendedFields = false;
        if (!loggedExtendedFields) {
            loggedExtendedFields = true;
            Log("Config parser accepts extended parity fields (GTB/Reach/Velocity/Caps).");
        }

        // Per-module keybinds (-1 means absent / don't override)
        { int v = reader.GetInt("keybindAutoclicker", -1);  if (v >= 0) g_config.keybindAutoclicker  = v; }
        { int v = reader.GetInt("keybindSpeedBridge", -1);  if (v >= 0) g_config.keybindSpeedBridge  = v; }
        { int v = reader.GetInt("keybindNametags", -1);      if (v >= 0) g_config.keybindNametags      = v; }
        { int v = reader.GetInt("keybindClosestPlayer", -1); if (v >= 0) g_config.keybindClosestPlayer = v; }
        { int v = reader.GetInt("keybindChestEsp", -1);      if (v >= 0) g_config.keybindChestEsp      = v; }
        { int v = reader.GetInt("keybindChestStealer", -1);  if (v >= 0) g_config.keybindChestStealer  = v; }
        { int v = reader.GetInt("keybindBlockEsp", -1);      if (v >= 0) g_config.keybindBlockEsp      = v; }

        g_config.pixelPartyAssist = reader.GetBool("pixelPartyAssist");
        int ppRadius = reader.GetInt("pixelPartyScanRadius", g_config.pixelPartyScanRadius);
        if (ppRadius < 8) ppRadius = 8;
        if (ppRadius > 48) ppRadius = 48;
        g_config.pixelPartyScanRadius = ppRadius;
        g_config.pixelPartyAutoLook = reader.GetBool("pixelPartyAutoLook");
        g_config.pixelPartyAutoWalk = reader.GetBool("pixelPartyAutoWalk");

        g_config.hudEditor = reader.GetBool("hudEditor");
        if (!g_config.hudEditor) {
            g_hudEditor.grabbedId.clear();
            g_hudEditor.mode = lc::HudEditorState::Mode::None;
        }
        g_hudEditor.active = g_config.hudEditor;
        // Parse hudLayout (under same g_configMutex lock since we're already in it).
        // Don't stomp an element the user is actively dragging in the HUD editor: keep
        // its live anchor/scale until the gesture ends (release sends the final layout
        // back out). Otherwise the loader's periodic config push would reset it mid-drag.
        {
            lc::HudLayout incoming = lc::ParseHudLayout(line);
            if (!g_hudEditor.grabbedId.empty()) {
                auto it = g_hudLayout.elements.find(g_hudEditor.grabbedId);
                if (it != g_hudLayout.elements.end())
                    incoming.elements[g_hudEditor.grabbedId] = it->second;
            }
            g_hudLayout = incoming;
        }
    }
}

bool TrySendCapabilities(SOCKET sock) {
    if (sock == INVALID_SOCKET) return false;

    const char* capabilitiesJson = lc::LegacyCapabilitiesJson();
    int sent = send(sock, capabilitiesJson, (int)strlen(capabilitiesJson), 0);
    if (sent == SOCKET_ERROR) {
        int err = WSAGetLastError();
        if (err == WSAEWOULDBLOCK) return false;
        Log("Capabilities send failed, err=" + std::to_string(err));
        return false;
    }
    return true;
}

void ServerLoop() {
    WSADATA wsaData; WSAStartup(MAKEWORD(2, 2), &wsaData);
    g_serverSocket = socket(AF_INET, SOCK_STREAM, 0);
    int opt = 1; setsockopt(g_serverSocket, SOL_SOCKET, SO_REUSEADDR, (const char*)&opt, sizeof(opt));
    sockaddr_in addr = {}; addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY; addr.sin_port = htons(25590);
    bind(g_serverSocket, (sockaddr*)&addr, sizeof(addr));
    listen(g_serverSocket, 1);

    // FIX: Force C locale for correct JSON float formatting (dots not commas)
    setlocale(LC_NUMERIC, "C"); 

    JNIEnv* env; JavaVMAttachArgs args;
    args.version = JNI_VERSION_1_8; args.name = (char*)"LegoBridge"; args.group = nullptr;
    g_jvm->AttachCurrentThread((void**)&env, &args);
    lc::InstallNickHiderJvmti(g_jvm, lc::NickHiderJvmtiGeneration::Legacy189, Log);
    ka_premotion::Install(g_jvm, Log);
    ka_premotion::SetAttackHandler(KillAuraPremotionAttackHandler);

    Log("Discovering classes...");
    bool mapped = DiscoverMappings(env);
    Log(mapped ? "Discovery OK" : "Discovery FAILED");

    while (g_running) {
        Log("Waiting for client...");
        g_clientSocket = accept(g_serverSocket, nullptr, nullptr);
        if (g_clientSocket == INVALID_SOCKET) { if (!g_running) break; continue; }
        Log("Client connected");

        // Set non-blocking for reading config from C#
        u_long mode = 1; ioctlsocket(g_clientSocket, FIONBIO, &mode);

        std::string readBuf;
        bool capabilitiesSent = false;
        while (g_running) {
            if (!capabilitiesSent) {
                capabilitiesSent = TrySendCapabilities(g_clientSocket);
                if (capabilitiesSent) Log("Sent bridge capabilities packet");
            }

            
            // Check config
            bool nametagsEnabled = false;
            bool needEntityTelemetry = false;
            Config cfgSnapshot;
            {
                LockGuard lk(g_configMutex);
                nametagsEnabled = g_config.nametags;
                needEntityTelemetry = g_config.nametags || g_config.closestPlayerInfo || g_config.aimAssist;
                cfgSnapshot = g_config;
            }

            // ALWAYS Read Game State to update global state (for block detection etc.)
            GameState state;
            {
                LockGuard jniLk(g_stateJniMutex);
                if (g_mcInstance && g_thePlayerField) {
                    jobject localPlayer = env->GetObjectField(g_mcInstance, g_thePlayerField);
                    if (env->ExceptionCheck()) { env->ExceptionClear(); localPlayer = nullptr; }
                    if (localPlayer) {
                        lc::SetNickHiderJvmtiLocalName(GetStablePlayerName(env, localPlayer));
                        env->DeleteLocalRef(localPlayer);
                    }
                }
                ApplyLegacyNickHiderFallback(env, cfgSnapshot);
                state = ReadGameState(env);
                if (cfgSnapshot.pixelPartyAssist)
                    UpdatePixelPartyAssistLegacy(env, cfgSnapshot, state);
                UpdateSpeedBridge(env, cfgSnapshot, state);
                if (cfgSnapshot.reachEnabled || g_reachAllowCurrentClick || g_reachClickPrevDown) {
                    UpdateReach(env, cfgSnapshot, state);
                }
                if (cfgSnapshot.velocityEnabled || g_lastHurtTime > 0) {
                    UpdateVelocity(env, cfgSnapshot);
                }
                if (cfgSnapshot.killAura) {
                    UpdateKillAuraLegacy(env, cfgSnapshot);
                }
                if (cfgSnapshot.antiDebuffEnabled) {
                    UpdateAntiDebuffLegacy(env, cfgSnapshot);
                }
                UpdateHitDelayFixLegacy(env, cfgSnapshot, state);
                if (cfgSnapshot.blockEsp) {
                    UpdateBlockEspListLegacy(env, cfgSnapshot);
                }
            }
            if (!cfgSnapshot.reachEnabled) {
                g_reachAllowCurrentClick = false;
                g_reachCurrentClickRange = 3.0;
                if (g_reachCurrentTarget) {
                    LockGuard jniLk(g_stateJniMutex);
                    env->DeleteGlobalRef(g_reachCurrentTarget);
                    g_reachCurrentTarget = nullptr;
                }
            }
            if (!cfgSnapshot.velocityEnabled) {
                g_lastHurtTime = 0;
            }
            { LockGuard lk(g_stateMutex); g_gameState = state; }

            // Standard Logic: Build JSON from state
            std::string jsonToSend = "{";
            jsonToSend += "\"mapped\":" + std::string(state.mapped ? "true" : "false") + ",";
            jsonToSend += "\"guiOpen\":" + std::string(state.guiOpen ? "true" : "false") + ",";
            jsonToSend += "\"screenName\":\"" + JsonEscape(state.screenName) + "\",";
            jsonToSend += "\"actionBar\":\"" + JsonEscape(state.actionBar) + "\",";
            jsonToSend += "\"health\":" + std::to_string(state.health) + ",";
            jsonToSend += "\"posX\":" + std::to_string(state.posX) + ",";
            jsonToSend += "\"posY\":" + std::to_string(state.posY) + ",";
            jsonToSend += "\"posZ\":" + std::to_string(state.posZ) + ",";
            jsonToSend += "\"pitch\":" + std::to_string(state.pitch) + ",";
            jsonToSend += "\"holdingBlock\":" + std::string(state.holdingBlock ? "true" : "false") + ",";
            jsonToSend += "\"lookingAtBlock\":" + std::string(state.lookingAtBlock ? "true" : "false") + ",";
            jsonToSend += "\"lookingAtEntity\":" + std::string(state.lookingAtEntity ? "true" : "false") + ",";
            jsonToSend += "\"lookingAtEntityLatched\":" + std::string(state.lookingAtEntityLatched ? "true" : "false") + ",";
            jsonToSend += "\"breakingBlock\":" + std::string(state.breakingBlock ? "true" : "false") + ",";
            jsonToSend += "\"attackCooldown\":" + std::to_string(state.attackCooldown) + ",";
            jsonToSend += "\"attackCooldownPerTick\":" + std::to_string(state.attackCooldownPerTick) + ",";
            jsonToSend += "\"killAuraUnavailableReason\":\"" + JsonEscape(state.killAuraUnavailableReason) + "\",";
            jsonToSend += "\"killAuraHasTarget\":" + std::string(state.killAuraHasTarget ? "true" : "false") + ",";
            jsonToSend += "\"killAuraBlocking\":" + std::string(state.killAuraBlocking ? "true" : "false") + ",";
            jsonToSend += "\"stateMs\":" + std::to_string(state.stateMs) + ",";
            jsonToSend += "\"chestStealerState\":";
            jsonToSend += state.chestStealerStateJson.empty() ? "null" : state.chestStealerStateJson;

            PixelPartySnap ppSnap;
            { LockGuard lk(g_pixelPartyMutex); ppSnap = g_pixelPartySnap; }
            float ppYawDelta = 0.0f;
            if (ppSnap.targetFound) {
                ppYawDelta = ppSnap.targetYaw - state.rotationYaw;
                while (ppYawDelta > 180.0f) ppYawDelta -= 360.0f;
                while (ppYawDelta < -180.0f) ppYawDelta += 360.0f;
            }
            jsonToSend += ",\"pixelPartyTargetFound\":";
            jsonToSend += ppSnap.targetFound ? "true" : "false";
            jsonToSend += ",\"pixelPartyTargetYaw\":";
            jsonToSend += std::to_string(ppSnap.targetYaw);
            jsonToSend += ",\"pixelPartyTargetDist\":";
            jsonToSend += std::to_string((float)ppSnap.dist);
            jsonToSend += ",\"pixelPartyYawDelta\":";
            jsonToSend += std::to_string(ppYawDelta);

            jsonToSend += ",\"entities\":";

            std::string entitiesJson = "[]";

            if (needEntityTelemetry) {
                LockGuard lk(g_jsonMutex);
                if (!g_pendingJson.empty()) {
                    entitiesJson = g_pendingJson;
                    g_pendingJson.clear();
                }
            }
            jsonToSend += entitiesJson;
            {
                LockGuard lk(g_configMutex);
                if (g_hudEditor.layoutDirty) {
                    jsonToSend += ",\"hudLayout\":";
                    jsonToSend += lc::SerializeHudLayout(g_hudLayout);
                    g_hudEditor.layoutDirty = false;
                }
            }
            jsonToSend += "}\n";

            // Send if we have data
            if (!jsonToSend.empty()) {
                LockGuard lk(g_socketMutex);
                int sent = send(g_clientSocket, jsonToSend.c_str(), (int)jsonToSend.length(), 0);
                if (sent == SOCKET_ERROR) {
                   if (WSAGetLastError() != WSAEWOULDBLOCK) break; 
                }
            }
            
            // Keep telemetry responsive without pegging CPU.
            // 14ms (~71 Hz) avoids JNI mutex contention with the render thread
            // while staying well within the 220ms aim-assist freshness window.
            Sleep(14);
            static DWORD lastNickHiderRefreshMs = 0;
            const DWORD nickHiderNow = GetTickCount();
            if (nickHiderNow - lastNickHiderRefreshMs >= 2000) {
                lastNickHiderRefreshMs = nickHiderNow;
                lc::RefreshNickHiderJvmtiTargets();
                lc::FlushNickHiderJvmtiDiagnostics();
                ka_premotion::RefreshTargets(env);
            }
            // Read config from C# (non-blocking)
            char buf[2048];
            int r = recv(g_clientSocket, buf, sizeof(buf) - 1, 0);
            if (r > 0) {
                buf[r] = 0; readBuf += buf;
                size_t pos;
                while ((pos = readBuf.find('\n')) != std::string::npos) {
                    std::string line = readBuf.substr(0, pos);
                    readBuf.erase(0, pos + 1);
                    if (!line.empty()) ParseConfig(line);
                }
            } else if (r == 0) {
                break;
            } else {
                int err = WSAGetLastError();
                if (err != WSAEWOULDBLOCK) break;
            }

            Sleep(needEntityTelemetry ? 10 : 25);
        }
        {
            LockGuard jniLk(g_stateJniMutex);
            ReleaseSpeedBridgeSneak(env);
            ResetSpeedBridgeMovementTracking();
        }
        closesocket(g_clientSocket); g_clientSocket = INVALID_SOCKET;
        Log("Client disconnected");
    }
    {
        LockGuard jniLk(g_stateJniMutex);
        ReleaseSpeedBridgeSneak(env);
        ResetSpeedBridgeMovementTracking();
        HelperBridge::Unload(env);
        lc::ShutdownNickHiderJvmti(env);
        ka_premotion::Shutdown(env);
    }
    g_jvm->DetachCurrentThread();
    closesocket(g_serverSocket); WSACleanup();
}

// ===================== MAIN THREAD & DLLMAIN =====================
DWORD WINAPI MainThread(LPVOID lpParam) {
    Log("MainThread started | build 2026-03-29 14:40 reach-clickedge-wndproc");
    HMODULE hJvm = GetModuleHandleA("jvm.dll");
    if (!hJvm) { Log("ERROR: jvm.dll not found"); return 0; }
    typedef jint(JNICALL* FnGetVMs)(JavaVM**, jsize, jsize*);
    FnGetVMs fn = (FnGetVMs)GetProcAddress(hJvm, "JNI_GetCreatedJavaVMs");
    if (!fn) { Log("ERROR: GetCreatedJavaVMs not found"); return 0; }
    jsize cnt; jint res = fn(&g_jvm, 1, &cnt);
    if (res != JNI_OK || cnt == 0) { Log("ERROR: No JVM"); return 0; }
    Log("JVM found");

    // Install rendering hook
    InstallSwapBuffersHook();

    // Start TCP server
    Log("Starting server...");
    ServerLoop();
    return 0;
}

extern "C" __declspec(dllexport) void Dummy() {}

BOOL APIENTRY DllMain(HMODULE hModule, DWORD reason, LPVOID lpReserved) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(hModule);
        InitLogPath(hModule);
        std::ofstream f(g_logPath.c_str(), std::ios_base::trunc);
        f << "[Bridge] DLL_PROCESS_ATTACH" << std::endl; f.close();
        Log("Log path: " + g_logPath);
        CreateThread(nullptr, 0, MainThread, nullptr, 0, nullptr);
    } else if (reason == DLL_PROCESS_DETACH) {
        CleanupImGuiAndHooks();
        UnloadMinecraftiaPrivateFont();
        Log("DLL_PROCESS_DETACH");
    }
    return TRUE;
}
