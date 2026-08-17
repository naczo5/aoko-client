---
name: add-client-module
description: >-
  Step-by-step workflow for adding or updating a module or setting across the full
  Aoko stack: ModuleCatalog, WPF UI, Profile, TCP config, string-list capabilities,
  overlay list, keybinds, and native bridges. Use when creating cheats, overlays,
  or utilities. Combat, hotbar, inventory, and JNI-mutation modules must also follow
  packet-and-call-hygiene.
---

# Adding a Client Module to Aoko

The split C# loader + native bridge means a feature is unfinished until every
surface `ModuleCatalog` declares is wired. `Aoko.Tests/ModuleRegistrationTests.cs`
is the contract.

If the module writes held items, uses items, attacks, opens inventories, or
otherwise mutates gameplay, stop and follow **packet-and-call-hygiene** first
(AutoRod / AutoTool / AutoTotem / KillAura conflicts). Overlay-only modules skip
that skill.

If the behavior is being reproduced from in-JVM Java (event bus, Mixins, mapping
wrappers), follow **java-to-jni-port** before writing JNI. Unwrap to vanilla
APIs; do not copy their types into the game JVM.

---

## 0. Register in `ModuleCatalog` first

Canonical file: `Aoko/Core/ModuleCatalog.cs`.

1. Add an `Entry` with a stable `Id` (lowercase, matches JSON keys).
2. Set `RequiredSurfaces` (`KeybindGui`, `KeybindMaps`, `DiscordRpc`,
   `OverlayList`, `Capability`). Use `StandardSurfaces` unless the module is
   keybind-only, overlay-only, or similar.
3. Set `OverlayProbe` to a unique string that must appear in the modern bridge
   `pushMod` block (e.g. `"cfg.autoToolEnabled"`).
4. Mark `DevOnly` or `ManagedOnly` when that is actually true.

Then satisfy every surface the entry requires. Do not start with XAML alone.

---

## 1. Architecture flow

```
ModuleCatalog.cs ──> Clicker.cs + Profile.cs
        │
        ├── MainWindow.xaml (card + keybind button Tag=id)
        ├── InputHooks.cs (toggle / action binds)
        └── GameStateClient.cs (config JSON, port 25590)
                    │
                    ▼
              BridgeCapabilities.cs  (string sets, not bitflags)
              bridge_capabilities.h  (LegacyCapabilitiesJson / ModernCapabilitiesJson)
                    │
              bridge.cpp / bridge_261.cpp
                    │
              inbound state ──> GameState.cs
```

Capabilities are **JSON name lists** (`modules`, `settings`, `state`), duplicated
in `BridgeCapabilities.ForVersionFallback` and `bridge_capabilities.h`. There are
no `CAP_*` bitflags.

---

## 2. Implementation layers

### Layer 1 — Clicker state (`Aoko/Core/Clicker.cs`)
Backing field + property that raises `OnPropertyChanged()`. Clamp in the setter.
If the feature is a C# loop (`SendInput`, aim), add a `CancellationToken` loop.

### Layer 2 — Profile (`Aoko/Core/Profile.cs`)
Persist to `%AppData%\Aoko\profiles\`. Round-trip in `Aoko.Tests`.

### Layer 3 — WPF (`Aoko/MainWindow.xaml`)
Category tab, `DynamicResource` styling, `Mode=TwoWay`,
`UpdateSourceTrigger=PropertyChanged`. Keybind button on the **module card**
(`Tag` = catalog id), not a separate keybind page. Gate visibility with
capabilities / `DevMode` like neighboring modules.

### Layer 4 — TCP config (`Aoko/Core/GameStateClient.cs`)
If the bridge or overlay needs the setting, add it to the serialized config
object. Keep key names stable; `ModuleRegistrationTests` greps this payload.

### Layer 5 — Capabilities
* `Aoko/Core/BridgeCapabilities.cs` — add the module id and every setting/state
  field name to the correct version set (legacy vs 1.21/26.x).
* `McInjector/src/main/cpp/bridge_capabilities.h` — same strings inside
  `LegacyCapabilitiesJson()` and/or `ModernCapabilitiesJson()`.
* Gate UI from the live capabilities payload, not from assumed bit masks.

### Layer 6 — Keybinds (`Aoko/Core/InputHooks.cs`)
If `KeybindMaps` is set: `ModuleKeys`, profile defaults, and `ToggleModule`.
Action binds (AutoRod use key) are extra and must not collide with the toggle.

### Layer 7 — Overlay list
Modern (and legacy if advertised) `pushMod` / module-list code must contain the
catalog `OverlayProbe`. Overlay-only modules still parse config in `ParseConfig()`.

### Layer 8 — Native bridges
* 1.8.9: `McInjector/src/main/cpp/bridge.cpp` — `ParseConfig()`, mappings,
  tick/premotion logic or ImGui overlay.
* 1.21 / 26.1 / 26.2: `bridge_261.cpp` — Yarn-first then Mojmap arrays.
* Shared sequencing belongs in a header (`auto_rod_core.h`, `auto_tool_core.h`)
  plus `McInjector/tests`. Do not fork the lockout rules per bridge.
* Overlay drawing is draw-only unless the module owns mutation.
* Menu-injection (1.8.9): mappings must recover after injecting on the title screen.

### Layer 9 — Inbound game state (when the loader needs new JVM fields)
Bridge JSON → `Aoko/Core/GameState.cs` property + capabilities `state` list +
parser. Config C#→bridge is not enough for values the overlay/loader reads back.

### Layer 10 — Tests
* `Aoko.Tests` and `McInjector\run_tests.bat` for shared native headers.

### Layer 11 — Docs
User-visible modules: Starlight page, `astro.config.mjs` sidebar, README feature
line if it is a headline module. Follow **write-docs** (match current GUI; delete
old mentions instead of documenting removals).

---

## 3. Branch: overlay vs managed input vs native mutation

| Kind | Typical work | Extra skill |
| :--- | :--- | :--- |
| Overlay / ESP / HUD | ParseConfig + ImGui + WorldToScreen | none |
| C# input (clicker, aim, SpeedBridge) | Clicker loop + config so ClickGUI matches | none unless it also swaps items |
| Native JNI / packets / hotbar | Both bridges + exclusive-resource lockout | **packet-and-call-hygiene** |
| Port from in-JVM Java | Unwrap to vanilla APIs, then pick a row above | **java-to-jni-port** |

---

## 4. Verification

```powershell
.\McInjector\run_tests.bat
dotnet test .\Aoko.Tests\Aoko.Tests.csproj
.\McInjector\build.bat
.\McInjector\build_261.bat
dotnet build .\Aoko\Aoko.csproj
```
