# Migration parity ledger: `Aoko/` (C#/.NET 8 WPF) → `crates/*` (Rust + GPUI)

Zero-silent-feature-drops record. `Aoko/` and `Aoko.Tests/` stay in-tree as ground truth;
`McInjector/` (C++ bridge) is **not** rewritten and is out of scope except where the host must keep
its wire contract.

Everything below is transcribed from the files named in each section, at base commit
`f1a9f5f9362cd274a97531bc0c10a6e86d291ca0` (branch `rust`). Where a value could not be read out of
the source it says `unknown` rather than guessing.

Read `MIGRATION_ARCHITECTURE.md` section 8 before implementing anything here. It records the
eight places where the sources disagree with themselves and the decided resolution, including:
`MainWindow.GuiPalettes` (10 roles) is the normative theme table and `ThemeManager.Themes` is a
derived 8-role view; the capability `state` arrays must never gate decoding; the C#
`ForVersionFallback` quirks are preserved rather than reconciled to `bridge_capabilities.h`;
and `ModuleCatalog.Entry.ManagedOnly` stays `false` because no entry sets it.

## Conventions

### Surface flags (`ModuleCatalog.Surfaces`)

| Flag | Bit | What it means |
| --- | --- | --- |
| `KeybindGui` | 1 | Keybind `Button` on the module card in `MainWindow.xaml` (`Tag=id`) plus `ModuleTitles` / `UpdateKeybindButtons` |
| `KeybindMaps` | 2 | `InputHooks.ModuleKeys`, `Profile.ModuleKeys` defaults, `ToggleModule` switch |
| `DiscordRpc` | 4 | Counted by Discord Rich Presence when enabled |
| `OverlayList` | 8 | Shown in the in-game top-right module list (bridge `pushMod`) |
| `Capability` | 16 | Listed in the bridge capability payloads (`BridgeCapabilities` fallback + `bridge_capabilities.h`) |

`StandardSurfaces` = all five. `ModuleCatalog.All` holds **42** entries. `ManagedOnly` is declared on
`Entry` but **no** entry sets it, so every row below is `no`.

### Target crates

| Crate | Owns |
| --- | --- |
| `aoko-protocol` | OS/UI-free data: wire messages, `GameState`, `Profile`, `ModuleCatalog`, capability lists |
| `aoko-platform` | OS interop: process/window enumeration, injection, input hooks, bridge TCP transport, Discord IPC |
| `aoko-core` | Headless behaviour with injected time/IO: controllers, schedulers, stores, services, search ranking |
| `aoko-app` | GPUI surfaces: windows, tabs, cards, styles, palettes, view models, XAML-shaped glue |

Crate descriptions are from `crates/*/Cargo.toml`. Placement of any file that straddles two
crates follows the placement rule in `MIGRATION_ARCHITECTURE.md`: `aoko-protocol` may compute,
parse and persist but may not open a socket, spawn a process or call the OS. At the time of
writing the trait seams exist (`aoko-protocol::transport`, the `aoko-platform` traits and
capability report); the ports themselves are Phase 1.

---

## Module registry

Source: [`Aoko/Core/ModuleCatalog.cs`](Aoko/Core/ModuleCatalog.cs) (42 entries).
Numeric `SettingSpec`s are `(wire key / property / min / max / default)`; `—` means the entry declares no
numeric settings.

| # | Module id | Display name | Overlay probe | Required surfaces | DevOnly | ManagedOnly | Beta | SettingSpecs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `autoclicker` | AutoClicker | `cfg.armed` | standard | no | no | no | — |
| 2 | `rightclick` | Right Click | `cfg.rightClick` | standard | no | no | no | — |
| 3 | `jitter` | Jitter | `cfg.jitter` | `DiscordRpc\|OverlayList\|Capability` | no | no | no | — |
| 4 | `clickinchests` | Click in Chests | `cfg.clickInChests` | `DiscordRpc\|OverlayList\|Capability` | no | no | no | — |
| 5 | `breakblocks` | Break Blocks | `cfg.breakBlocks` | `DiscordRpc\|OverlayList\|Capability` | no | no | no | — |
| 6 | `aimassist` | Aim Assist | `cfg.aimAssist` | standard | no | no | no | — |
| 7 | `triggerbot` | Triggerbot | `cfg.triggerbot` | standard | no | no | no | — |
| 8 | `killaura` | Kill Aura | `cfg.killAura` | standard | **yes** | no | **yes** | — |
| 9 | `speedbridge` | SpeedBridge | `cfg.speedBridge` | standard | no | no | no | — |
| 10 | `gtbhelper` | GTB Helper | `cfg.gtbHelper` | standard | no | no | no | — |
| 11 | `pixelpartyassist` | Pixel Party Assist | `cfg.pixelPartyAssist` | standard | no | no | no | — |
| 12 | `nametags` | Nametags | `cfg.nametags` | standard | no | no | no | — |
| 13 | `nickhider` | Nick Hider | `cfg.nickHiderEnabled` | standard | no | no | no | — |
| 14 | `closestplayer` | Closest Player | `cfg.closestPlayer` | standard | no | no | no | — |
| 15 | `fightstatus` | Fight Status | `cfg.fightStatus` | standard | no | no | no | — |
| 16 | `chestesp` | Chest ESP | `cfg.chestEsp` | standard | no | no | no | — |
| 17 | `cheststealer` | Chest Stealer | `cfg.chestStealer` | standard | no | no | no | — |
| 18 | `refill` | Refill | `cfg.refill` | standard | no | no | no | — |
| 19 | `blockesp` | Block ESP | `cfg.blockEsp` | standard | no | no | no | — |
| 20 | `bedplates` | BedPlates | `cfg.bedPlates` | standard | no | no | no | — |
| 21 | `reach` | Reach | `cfg.reachEnabled` | standard | no | no | no | — |
| 22 | `velocity` | Velocity | `cfg.velocityEnabled` | standard | no | no | no | `velocityHorizontal` / `VelocityHorizontal` / 1 / 100 / 100; `velocityVertical` / `VelocityVertical` / 1 / 100 / 100; `velocityChance` / `VelocityChance` / 1 / 100 / 100 |
| 23 | `autototem` | AutoTotem | `cfg.autoTotemEnabled` | standard | no | no | no | — |
| 24 | `autorod` | Auto Rod | `cfg.autoRodEnabled` | standard | no | no | no | — |
| 25 | `throwpot` | Throwpot | `cfg.throwpotEnabled` | standard | no | no | no | — |
| 26 | `autoheal` | AutoHeal | `cfg.autoHealEnabled` | standard | no | no | no | — |
| 27 | `autotool` | AutoTool | `cfg.autoToolEnabled` | standard | no | no | no | — |
| 28 | `antidebuff` | AntiDebuff | `cfg.antiDebuffEnabled` | standard | no | no | no | — |
| 29 | `hitdelayfix` | Hit Delay Fix | `cfg.hitDelayFixEnabled` | standard | no | no | no | — |
| 30 | `fireballwarning` | Fireball Warning | `cfg.fireballWarning` | standard | no | no | no | — |
| 31 | `fastplace` | Fast Place | `cfg.fastPlace` | standard | no | no | **yes** | — |
| 32 | `nohurtcam` | No Hurt Cam | `cfg.noHurtCam` | standard | no | no | **yes** | — |
| 33 | `sprint` | Sprint | `cfg.sprint` | standard | no | no | **yes** | — |
| 34 | `tnttimer` | TNT Timer | `cfg.tntTimer` | standard | no | no | **yes** | — |
| 35 | `wtap` | WTap | `cfg.wTap` | standard | no | no | **yes** | — |
| 36 | `jumpreset` | Jump Reset | `cfg.jumpReset` | standard | no | no | **yes** | `jumpResetChance` / `JumpResetChance` / 0 / 100 / 80 |
| 37 | `hitselect` | Hit Select | `cfg.hitSelect` | standard | no | no | **yes** | — |
| 38 | `trajectories` | Trajectories | `cfg.trajectories` | standard | no | no | **yes** | — |
| 39 | `murdermystery` | Murder Mystery | `cfg.murderMystery` | standard | no | no | **yes** | — |
| 40 | `breakprogress` | Break Progress | `cfg.breakProgress` | standard | no | no | **yes** | — |
| 41 | `panic` | Panic | *(empty)* | `KeybindGui\|KeybindMaps` | no | no | no | — |
| 42 | `hudeditor` | HUD Editor | *(empty)* | `KeybindMaps\|Capability` | no | no | no | — |

Notes carried by the source, not derived:

- `panic`: "Keybind-only emergency action — not a toggleable feature module." No overlay probe, no
  Discord RPC, no overlay list entry, no capability list entry — but it *does* have a card (`PanicCard`)
  with `KeybindPanicButton` (`Tag="panic"`), so it occupies a `KeybindGui` row.
- `hudeditor`: "capability + keybind maps, not RPC/overlay/GUI keybind grid." Its card (`HudEditorCard`)
  binds `HudEditorActive` **without** a keybind button; `UpdateKeybindButtons()` has no `hudeditor` line.
- `jitter`, `clickinchests`, `breakblocks` are AutoClicker sub-settings: listed in overlay/RPC/capability
  but not keybindable modules.
- The window's own support predicate is
  `IsModuleSupported(id) => id == "panic" || GameStateClient.Instance.SupportsModule(id)`, so `panic` is
  always clickable while every other module is gated by the bridge module list.

---

## Module settings surface

Source: [`Aoko/MainWindow.xaml`](Aoko/MainWindow.xaml) and
[`Aoko/MainWindow.xaml.cs`](Aoko/MainWindow.xaml.cs).
`DataContext` is `Clicker.Instance` (set in the constructor), so every `{Binding X}` below targets a
`Clicker` property unless stated otherwise; `KillAuraSettings.*` targets the nested
`Clicker.KillAuraSettings`, and the Stats tab overrides `DataContext` with `StatsTracker.Instance`.
Defaults are the `Clicker` / `KillAuraSettings` field initialisers. "—" in the `x:Name` column means the
control is unnamed in XAML (identified by its label text instead).

Global conventions for every card in this file: the `Keybind<Module>Button` is a `Button` with
`Tag="<module id>"` bound to `KeybindButton_Click`, with content written by
`SetKeybindButtonContent(btn, id)` from `InputHooks.GetModuleKey(id)` and captured through
`InputHooks.StartKeyCapture(allowMouse: true)` → `InputHooks_OnKeyCaptured`; `Esc` (VK 0x1B) unbinds.
Availability `TextBlock`s are filled by `UpdateVersionAvailabilityUi()`; unsupported cards get
`IsEnabled = false` and their module flag is forced false.

### Combat tab

| Module | Control (`x:Name`) | Type | Bind target / handler | Range, default |
| --- | --- | --- | --- | --- |
| autoclicker | `KeybindAutoclickerButton` | Button | `KeybindButton_Click` (`Tag=autoclicker`) | — |
| autoclicker | — ("AutoClicker") | CheckBox `ModuleSwitch` | `IsArmed` OneWay + `ToggleArmed_Click` → `Clicker.ToggleArmed()` | default off |
| autoclicker | — (`{Binding StatusText}`) | TextBlock | `Clicker.StatusText` | — |
| autoclicker | — ("Min CPS") | Slider | `MinCPS` TwoWay | 1–25, default 8.0 |
| autoclicker | — ("Max CPS") | Slider | `MaxCPS` TwoWay | 1–25, default 12.0 |
| autoclicker/jitter | — ("Randomization") | Slider snap | `RandomizationModeIndex` TwoWay → `RandomizationMode` | 0–2 (Basic/Medium/Advanced), default Medium (1); label via `RandomizationModeText` |
| clickinchests | — ("Click in Chests") | CheckBox | `ClickInChests` TwoWay | default false |
| breakblocks | — ("Pause when breaking") | CheckBox | `BreakBlocksEnabled` TwoWay | default false |
| rightclick | `KeybindRightClickButton` | Button | `KeybindButton_Click` (`Tag=rightclick`) | — |
| rightclick | — ("Enable Right Click") | CheckBox `ModuleSwitch` | `RightClickEnabled` TwoWay | default false |
| rightclick | — ("Right Min CPS") | Slider | `RightMinCPS` TwoWay | 1–25, default 10.0 |
| rightclick | — ("Right Max CPS") | Slider | `RightMaxCPS` TwoWay | 1–25, default 14.0 |
| rightclick | — ("Only when holding block") | CheckBox | `RightClickOnlyBlock` TwoWay | default false |
| aimassist | `KeybindAimAssistButton` | Button | `KeybindButton_Click` (`Tag=aimassist`) | — |
| aimassist | — ("Aim Assist") | CheckBox `ModuleSwitch` | `AimAssistEnabled` TwoWay | default false |
| aimassist | `AimAssistAvailabilityText` | TextBlock | set by `UpdateVersionAvailabilityUi` | "Available" / unsupported text |
| aimassist | — ("FOV") | Slider | `AimAssistFov` TwoWay | 1–180, default 30.0 |
| aimassist | — ("Range") | Slider | `AimAssistRange` TwoWay | 1–12, default 4.5 |
| aimassist | — ("Strength (%)") | Slider snap | `AimAssistStrength` TwoWay | 1–100, default 40 |
| triggerbot | `KeybindTriggerbotButton` | Button | `KeybindButton_Click` (`Tag=triggerbot`) | — |
| triggerbot | — ("Triggerbot") | CheckBox `ModuleSwitch` | `TriggerbotEnabled` TwoWay | default false |
| triggerbot | `TriggerbotAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| triggerbot | — ("Cooldown Threshold (%)") | Slider snap | `TriggerbotCooldownThreshold` TwoWay | 1–100, default 92 |
| triggerbot | — ("Only attack when crosshair is on entity") | CheckBox | `TriggerbotOnlyCrosshair` TwoWay | default true |
| triggerbot | — ("Only attack if target is in real hit range") | CheckBox | `TriggerbotOnlyIfCanAttack` TwoWay | default true |
| triggerbot | — ("Hit Chance (%)") | Slider snap | `TriggerbotHitChance` TwoWay | 1–100, default 100 |
| triggerbot | — ("Only while holding left click") | CheckBox | `TriggerbotRequireClick` TwoWay | default true |
| autorod | `KeybindAutoRodButton` | Button | `KeybindButton_Click` (`Tag=autorod`) | — |
| autorod | — ("Auto Rod") | CheckBox `ModuleSwitch` | `AutoRodEnabled` TwoWay | default false |
| autorod | `AutoRodAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| autorod | — ("Slot mode") | ComboBox (Auto, Slot 1…9) | `SelectedIndex` → `AutoRodSlotMode` TwoWay | 0–9, default 0 (Auto) |
| autorod | — ("Verify forced slot contains a fishing rod") | CheckBox | `AutoRodVerifyForcedSlot` TwoWay; `IsEnabled` ← `AutoRodUsesForcedSlot` (`AutoRodSlotMode > 0`) | default true |
| autorod | — ("Hold action bind to extend cast") | CheckBox | `AutoRodHoldToExtend` TwoWay | default false |
| autorod | — ("Extension delay (ticks)") | Slider snap | `AutoRodExtensionTicks` TwoWay; `IsEnabled` ← `AutoRodUsesFixedExtension` (= `!AutoRodHoldToExtend`) | 1–40, default 4 |
| autorod | `AutoRodActionBindButton` | Button | `AutoRodActionBindButton_Click` → `InputHooks.SetAutoRodActionKey`; content from `InputHooks.AutoRodActionKey` | "Action: Unbound" |
| autorod | `AutoRodActionBindStatusText` | TextBlock | written by the capture handler (assigned / conflict text) | — |
| throwpot | `KeybindThrowpotButton` | Button | `KeybindButton_Click` (`Tag=throwpot`) | — |
| throwpot | — ("Throwpot") | CheckBox `ModuleSwitch` | `ThrowpotEnabled` TwoWay | default false |
| throwpot | `ThrowpotAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| throwpot | `ThrowpotActionBindButton` | Button | `ThrowpotActionBindButton_Click` → `InputHooks.SetThrowpotActionKey` | "Action: Unbound" |
| throwpot | `ThrowpotActionBindStatusText` | TextBlock | capture handler | — |
| autoheal | `KeybindAutoHealButton` | Button | `KeybindButton_Click` (`Tag=autoheal`) | — |
| autoheal | — ("AutoHeal") | CheckBox `ModuleSwitch` | `AutoHealEnabled` TwoWay | default false |
| autoheal | `AutoHealAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| autoheal | — ("Health threshold (HP)") | Slider snap | `AutoHealHealth` TwoWay | 1–20, default 17 |
| autoheal | — ("Re-throw cooldown (ms)") | Slider snap | `AutoHealDelayMs` TwoWay | 50–2000 step 10, default 500 |
| hitdelayfix | `KeybindHitDelayFixButton` | Button | `KeybindButton_Click` (`Tag=hitdelayfix`) | — |
| hitdelayfix | `HitDelayFixAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| hitdelayfix | — ("Enable Hit Delay Fix") | CheckBox `ModuleSwitch` | `HitDelayFixEnabled` TwoWay | default false |

### Render tab

| Module | Control (`x:Name`) | Type | Bind target / handler | Range, default |
| --- | --- | --- | --- | --- |
| nametags | `KeybindNametagsButton` | Button | `KeybindButton_Click` (`Tag=nametags`) | — |
| nametags | — ("Nametags") | CheckBox `ModuleSwitch` | `NametagsEnabled` TwoWay | default false |
| nametags | — ("Show Health") | CheckBox | `NametagShowHealth` TwoWay | default true |
| nametags | — ("Show Armor") | CheckBox | `NametagShowArmor` TwoWay | default true |
| nametags | — ("Hide Vanilla Tags") | CheckBox | `NametagHideVanilla` TwoWay | default false |
| nametags | — ("Show Item") | CheckBox | `NametagShowHeldItem` TwoWay | default true |
| nametags | — ("Scan Range (chunks)") | Slider snap | `NametagRange` TwoWay | 1–8, default 4 |
| nickhider | `KeybindNickHiderButton` | Button | `KeybindButton_Click` (`Tag=nickhider`) | — |
| nickhider | — ("Nick Hider") | CheckBox `ModuleSwitch` | `NickHiderEnabled` TwoWay | default false |
| nickhider | — ("Shown as") | TextBox | `NickHiderAlias` TwoWay, `MaxLength=32` | default `""`; 1–32 plain-text chars |
| fightstatus | `KeybindFightStatusButton` | Button | `KeybindButton_Click` (`Tag=fightstatus`) | — |
| fightstatus | — ("Fight Status") | CheckBox `ModuleSwitch` | `FightStatusEnabled` TwoWay | default false |
| chestesp | `KeybindChestEspButton` | Button | `KeybindButton_Click` (`Tag=chestesp`) | — |
| chestesp | — ("Chest ESP") | CheckBox `ModuleSwitch` | `ChestEspEnabled` TwoWay | default false |
| chestesp | — ("Scan Range (chunks)") | Slider snap | `ChestEspRange` TwoWay | 1–8, default 4 |
| blockesp | `KeybindBlockEspButton` | Button | `KeybindButton_Click` (`Tag=blockesp`) | — |
| blockesp | — ("Block ESP (X-ray)") | CheckBox `ModuleSwitch` | `BlockEspEnabled` TwoWay | default false |
| blockesp | — ("Boxes") | CheckBox | `BlockEspBoxes` TwoWay | default true |
| blockesp | — ("Tracers") | CheckBox | `BlockEspTracers` TwoWay | default false |
| blockesp | — ("HUD List") | CheckBox | `BlockEspHud` TwoWay | default true |
| blockesp | — ("Max Blocks") | Slider snap | `BlockEspMaxCount` TwoWay | 1–512, default 64 |
| blockesp | — ("Scan Range (chunks)") | Slider snap | `BlockEspRange` TwoWay | 1–8, default 4 |
| blockesp | `BlockEspTargets` (via `ItemsSource`) | ItemsControl | `Clicker.BlockEspTargets`, row template below | — |
| blockesp | row `Enabled` | CheckBox | `BlockEspTarget.Enabled` TwoWay | per target |
| blockesp | row `✕` | Button | `BlockEspRemove_Click` → `BlockEspTargets.Remove` | — |
| blockesp | row colour swatch | Border (`MouseLeftButtonUp`) | `BlockEspColor_Click` → `ColorPickerDialog.TryPick` → `BlockEspTarget.ColorHex` | default `BlockEspConfig.DefaultColorHex` |
| blockesp | row `DisplayName` | TextBlock | `BlockEspTarget.DisplayName` | — |
| blockesp | `BlockEspAddBox` | TextBox | `BlockEspAddBox_KeyDown` (Enter) and `BlockEspAdd_Click` → `AddBlockEspFromBox()` | free text, e.g. `minecraft:diamond_ore` |
| bedplates | `KeybindBedPlatesButton` | Button | `KeybindButton_Click` (`Tag=bedplates`) | — |
| bedplates | — ("BedPlates") | CheckBox `ModuleSwitch` | `BedPlatesEnabled` TwoWay | default false |
| bedplates | — ("Show distance") | CheckBox | `BedPlatesShowDistance` TwoWay | default true |
| bedplates | — ("Scan Range (chunks)") | Slider snap | `BedPlatesRange` TwoWay | 1–8, default 4 |
| fireballwarning | `KeybindFireballWarningButton` | Button | `KeybindButton_Click` (`Tag=fireballwarning`) | — |
| fireballwarning | — ("Fireball Warning") | CheckBox `ModuleSwitch` | `FireballWarningEnabled` TwoWay | default false |
| fireballwarning | — ("Box warning") | CheckBox | `FireballWarningBox` TwoWay | default true |
| fireballwarning | — ("Arrow warning") | CheckBox | `FireballWarningArrow` TwoWay | default true |
| fireballwarning | — ("Sound warning") | CheckBox | `FireballWarningSound` TwoWay | default true |
| closestplayer | `KeybindClosestPlayerButton` | Button | `KeybindButton_Click` (`Tag=closestplayer`) | — |
| closestplayer | — ("Closest Player") | CheckBox `ModuleSwitch` | `ClosestPlayerInfoEnabled` TwoWay | default false |
| *(overlay)* | — ("Show module list (top-right)") | CheckBox `ModuleSwitch` | `ShowModuleList` TwoWay | default true |
| hudeditor | `HudEditorCard` / — ("Edit HUD") | CheckBox `ModuleSwitch` | `HudEditorActive` TwoWay | default false |
| hudeditor | — ("Reset layout") | Button | `ResetHudLayoutButton_Click` | — |
| antidebuff | `KeybindAntiDebuffButton` | Button | `KeybindButton_Click` (`Tag=antidebuff`) | — |
| antidebuff | `AntiDebuffAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| antidebuff | — ("Enable AntiDebuff") | CheckBox `ModuleSwitch` | `AntiDebuffEnabled` TwoWay | default false |

### Utility tab

| Module | Control (`x:Name`) | Type | Bind target / handler | Range, default |
| --- | --- | --- | --- | --- |
| speedbridge | `KeybindSpeedBridgeButton` | Button | `KeybindButton_Click` (`Tag=speedbridge`) | — |
| speedbridge | — ("SpeedBridge") | CheckBox `ModuleSwitch` | `SpeedBridgeEnabled` TwoWay | default false |
| speedbridge | `SpeedBridgeAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| speedbridge | — ("Safety") | Slider snap | `SpeedBridgeDelayMs` TwoWay | 20–250, default 85 |
| speedbridge | — ("Block only") | CheckBox | `SpeedBridgeBlockOnly` TwoWay | default true |
| speedbridge | — ("Holding sneak only") | CheckBox | `SpeedBridgeHoldingShiftOnly` TwoWay | default true |
| speedbridge | — ("Looking down only") | CheckBox | `SpeedBridgeLookingDownOnly` TwoWay | default true |
| *(utility)* | — ("Discord Rich Presence") | CheckBox `ModuleSwitch` | `DiscordRpcEnabled` TwoWay | default true |
| *(utility)* | — (`{Binding DiscordRpcStatusText}`) | TextBlock | `Clicker.DiscordRpcStatusText` | "Ready" |
| gtbhelper | `KeybindGtbHelperButton` | Button | `KeybindButton_Click` (`Tag=gtbhelper`) | — |
| gtbhelper | — ("GTB Helper") | CheckBox `ModuleSwitch` | `GtbHelperEnabled` TwoWay | default false |
| gtbhelper | `GtbHelperAvailabilityText` | TextBlock | static hint ("Hypixel Guess The Build helper using action-bar hints.") | — |
| pixelpartyassist | `KeybindPixelPartyAssistButton` | Button | `KeybindButton_Click` (`Tag=pixelpartyassist`) | — |
| pixelpartyassist | — ("Pixel Party Assist") | CheckBox `ModuleSwitch` | `PixelPartyAssistEnabled` TwoWay | default false |
| pixelpartyassist | `PixelPartyAssistAvailabilityText` | TextBlock | static hint ("Hypixel Pixel Party: finds the closest matching terracotta on the floor.") | — |
| pixelpartyassist | — ("Scan radius (blocks)") | Slider snap | `PixelPartyScanRadius` TwoWay | 8–48, default 28 |
| pixelpartyassist | — ("Auto-look toward target") | CheckBox | `PixelPartyAutoLookEnabled` TwoWay | default false |
| pixelpartyassist | — ("Auto-walk toward target") | CheckBox | `PixelPartyAutoWalkEnabled` TwoWay | default false |
| cheststealer | `KeybindChestStealerButton` | Button | `KeybindButton_Click` (`Tag=cheststealer`) | — |
| cheststealer | `ChestStealerAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| cheststealer | — ("Enable Chest Stealer") | CheckBox `ModuleSwitch` | `ChestStealerEnabled` TwoWay | default false |
| cheststealer | — ("Delay (ms)") | Slider snap | `ChestStealerDelayMs` TwoWay | 50–500 step 10, default 120 |
| cheststealer | — ("Title check") | CheckBox | `ChestStealerTitleCheck` TwoWay | default true |
| cheststealer | — ("Custom item check") | CheckBox | `ChestStealerCustomItemsCheck` TwoWay | default true |
| cheststealer | — ("Physical chest check") | CheckBox | `ChestStealerPhysicalCheck` TwoWay | default true |
| refill | `KeybindRefillButton` | Button | `KeybindButton_Click` (`Tag=refill`) | — |
| refill | `RefillAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| refill | — ("Enable Refill") | CheckBox `ModuleSwitch` | `RefillEnabled` TwoWay | default false |
| refill | — ("Delay (ms)") | Slider snap | `RefillDelayMs` TwoWay | 50–500 step 10, default 120 |
| autotool | `KeybindAutoToolButton` | Button | `KeybindButton_Click` (`Tag=autotool`) | — |
| autotool | `AutoToolAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| autotool | — ("Enable AutoTool") | CheckBox `ModuleSwitch` | `AutoToolEnabled` TwoWay | default false |
| autotool | — ("Swap delay (ms)") | Slider snap | `AutoToolSwapToDelay` TwoWay | 0–500 step 10, default 50 |
| autotool | — ("Swap back to original item") | CheckBox | `AutoToolSwapBack` TwoWay | default false |
| autotool | — ("Swap back delay (ms)") | Slider snap | `AutoToolSwapBackDelay` TwoWay | 0–1000 step 25, default 350 |
| autotool | — ("Swap weapon") | CheckBox | `AutoToolSwapWeapon` TwoWay | default true |
| autotool | — ("Instant weapon swap") | CheckBox | `AutoToolInstantSwap` TwoWay | default true |
| autotool | — ("Require mouse down") | CheckBox | `AutoToolRequireMouseDown` TwoWay | default true |
| autotool | — ("Only while sneaking") | CheckBox | `AutoToolOnlySneaking` TwoWay | default false |
| autotool | — ("Bedwars mode") | CheckBox | `AutoToolBedwarsMode` TwoWay | default true |
| panic | `KeybindPanicButton` | Button | `KeybindButton_Click` (`Tag=panic`) | — |
| panic | — ("Trigger Panic Now") | Button | `PanicButton_Click` | — |

Code-only setting with no control: `Clicker.ChestStealerMenuCheck` (default `true`) is serialised to the
bridge (`chestStealerMenuCheck`) and consumed by `ChestStealerController`, but `MainWindow.xaml` does
**not** bind it — only `ChestStealerTitleCheck` / `CustomItemsCheck` / `PhysicalCheck` are exposed. Treat
it as a deliberately invisible setting, not a UI omission.

### Risky tab

| Module | Control (`x:Name`) | Type | Bind target / handler | Range, default |
| --- | --- | --- | --- | --- |
| reach | `KeybindReachButton` | Button | `KeybindButton_Click` (`Tag=reach`) | — |
| reach | `ReachAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| reach | — ("Enable Reach") | CheckBox `ModuleSwitch` | `ReachEnabled` TwoWay | default false |
| reach | — ("Min Range") | Slider snap | `ReachMin` TwoWay | 3.0–6.0 step 0.1, default 3.0 |
| reach | — ("Max Range") | Slider snap | `ReachMax` TwoWay | 3.0–6.0 step 0.1, default 3.0 |
| reach | — ("Chance (%)") | Slider snap | `ReachChance` TwoWay | 0–100, default 100 |
| reach | — ("Chance Mode") | ComboBox (Normal / Advanced) | `SelectedIndex` → `ReachChanceMode` TwoWay | 0–1, default 0 |
| reach | — ("Only While Sprinting") | CheckBox | `ReachOnlyWhileSprinting` TwoWay | default false |
| reach | — ("Disable in Water") | CheckBox | `ReachDisableInWater` TwoWay | default false |
| reach | — ("Vertical Check") | CheckBox | `ReachVerticalCheck` TwoWay | default false |
| velocity | `KeybindVelocityButton` | Button | `KeybindButton_Click` (`Tag=velocity`) | — |
| velocity | `VelocityAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| velocity | — ("Enable Velocity") | CheckBox `ModuleSwitch` | `VelocityEnabled` TwoWay | default false |
| velocity | — ("Horizontal (%)") | Slider snap | `VelocityHorizontal` TwoWay | 1–100, default 100 |
| velocity | — ("Vertical (%)") | Slider snap | `VelocityVertical` TwoWay | 1–100, default 100 |
| velocity | — ("Chance (%)") | Slider snap | `VelocityChance` TwoWay | 1–100, default 100 |
| autototem | `KeybindAutoTotemButton` | Button | `KeybindButton_Click` (`Tag=autototem`) | — |
| autototem | `AutoTotemAvailabilityText` | TextBlock | `UpdateVersionAvailabilityUi` | — |
| autototem | — ("Enable AutoTotem") | CheckBox `ModuleSwitch` | `AutoTotemEnabled` TwoWay | default false |
| autototem | — ("Mode") | ComboBox (Smart (health + elytra) / Strict (always hold)) | `SelectedIndex` → `AutoTotemMode` TwoWay | 0–1, default 0 |
| autototem | — ("Health Threshold") | Slider snap | `AutoTotemHealth` TwoWay | 0–36, default 10 |
| autototem | — ("Delay (ticks)") | Slider snap | `AutoTotemDelay` TwoWay | 0–20, default 0 |
| autototem | — ("Elytra") | CheckBox `ModuleSwitch` | `AutoTotemElytra` TwoWay | default true |
| autototem | — ("Behavior") | ComboBox (Ghost (inventory only) / Anarchy (may flag on servers)) | `SelectedIndex` → `AutoTotemBehaviorMode` TwoWay | 0–1, default 0 |
| autototem | — (GrimAC/Matrix warning) | TextBlock with `DataTrigger` on `AutoTotemBehaviorMode == 1` | visibility only; text is `Foreground="#FFA500"` | — |

### Beta tab

Every Beta card is `Border x:Name="<Module>Card"` with the shared `ControlCardStyle`. Cards with a
keybind button follow the standard `Keybind<Module>Button` + `<Module>AvailabilityText` pattern; the
checkbox is always `<Module>Enabled` TwoWay, default false.

| Module | Controls | Notes |
| --- | --- | --- |
| killaura | `KillAuraCard` — `Visibility` bound to `DevMode` through `BooleanToVisibilityConverter`; `KeybindKillAuraButton`; CheckBox "Kill Aura" → `KillAuraEnabled`; `KillAuraAvailabilityText`; warning `Border` (`#33E0533D` fill, `#E0533D` border); five `Expander`s (see sub-table) | DevOnly: hidden unless `Clicker.DevMode`; card also force-disabled when the bridge does not support it |
| jumpreset | `JumpResetCard`; `KeybindJumpResetButton`; CheckBox → `JumpResetEnabled`; `JumpResetAvailabilityText`; Slider snap 0–100 → `JumpResetChance` (default 80) | — |
| hitselect | `HitSelectCard`; `KeybindHitSelectButton`; CheckBox → `HitSelectEnabled`; `HitSelectAvailabilityText` | no numeric settings |
| wtap | `WTapCard`; `KeybindWTapButton`; CheckBox → `WTapEnabled`; `WTapAvailabilityText` | — |
| trajectories | `TrajectoriesCard`; `KeybindTrajectoriesButton`; CheckBox → `TrajectoriesEnabled`; `TrajectoriesAvailabilityText` | — |
| murdermystery | `MurderMysteryCard`; `KeybindMurderMysteryButton`; CheckBox → `MurderMysteryEnabled`; `MurderMysteryAvailabilityText` | — |
| breakprogress | `BreakProgressCard`; `KeybindBreakProgressButton`; CheckBox → `BreakProgressEnabled`; `BreakProgressAvailabilityText` | — |
| nohurtcam | `NoHurtCamCard`; `KeybindNoHurtCamButton`; CheckBox → `NoHurtCamEnabled`; `NoHurtCamAvailabilityText` | — |
| tnttimer | `TntTimerCard`; `KeybindTntTimerButton`; CheckBox → `TntTimerEnabled`; `TntTimerAvailabilityText` | — |
| fastplace | `FastPlaceCard`; `KeybindFastPlaceButton`; CheckBox → `FastPlaceEnabled`; `FastPlaceAvailabilityText` | — |
| sprint | `SprintCard`; `KeybindSprintButton`; CheckBox → `SprintEnabled`; `SprintAvailabilityText` | — |

#### Kill Aura sub-surface (`Clicker.KillAuraSettings`, model `Aoko/Core/KillAuraSettings.cs`)

All controls are unnamed; each binds a `KillAuraSettings` property TwoWay. Defaults are the
`KillAuraSettings` field initialisers (mirror of OpenMyau-Plus `fc7c95d`, per the class comment).

| Expander | Label | Type | Bind target | Range, default |
| --- | --- | --- | --- | --- |
| Combat and targeting | CPS mode | ComboBox (`Tag`: normal, record) | `CpsMode` | default `normal` |
| Combat and targeting | Target mode | ComboBox (single, switch) | `Mode` | default `switch` |
| Combat and targeting | Sort | ComboBox (distance, health, hurttime, fov) | `Sort` | default `health` |
| Combat and targeting | Min CPS | Slider snap | `MinCps` | 1–20, default 14 (raises `MaxCps` when needed) |
| Combat and targeting | Max CPS | Slider snap | `MaxCps` | 1–20, default 14 (lowers `MinCps` when needed) |
| Combat and targeting | Attack range | Slider snap | `AttackRange` | 3–6 step 0.1, default 3.0 |
| Combat and targeting | Swing range | Slider snap | `SwingRange` | 3–6 step 0.1, default 3.5 |
| Combat and targeting | FOV | Slider snap | `Fov` | 30–360 step 10, default 360 |
| Combat and targeting | Switch delay | Slider snap | `SwitchDelay` | 0–1000 step 50, default 150 |
| Auto block | Mode | ComboBox (none, vanilla, spoof, hypixel, blink, interact, swap, legit, fake, morden) | `AutoBlock` | default `hypixel` |
| Auto block | Require use-item held | CheckBox | `AutoBlockRequirePress` | default false |
| Auto block | Auto-block CPS | Slider snap | `AutoBlockCps` | 1–10 step 0.5, default 8.0 |
| Auto block | Block range | Slider snap | `AutoBlockRange` | 3–8 step 0.1, default 6.0 |
| Auto block | Attack tick | Slider snap, group visible when `IsSwapAutoBlock` | `AttackTick` | 0–5, default 0 |
| Rotations | Mode | ComboBox (none, legit, silent, lockview, liquidbounce, hypixel, grok) | `Rotations` | default `silent` |
| Rotations | Move fix | ComboBox (none, silent, strict) | `MoveFix` | default `silent` |
| Rotations | Angle step | Slider snap | `AngleStep` | 30–180 step 5, default 90 |
| Rotations | Smoothing | Slider snap | `Smoothing` | 0–100 step 5, default 0 |
| Rotations | Hypixel smoothing | Slider snap, group visible when `IsHypixelRotation` | `RavenSmoothing` | 0–10, default 0 |
| Rotations | Predict ticks | Slider snap (same group) | `RavenPredictTicks` | 0–5, default 0 |
| Rotations | Yaw random | Slider snap (same group) | `RavenYawRandom` | 0–5, default 0 |
| Rotations | Max silent skew | Slider, group visible when `IsGrokRotation` | `GrokMaxSkew` | 6–25 step 0.5, default 12.0 |
| Rotations | Dead zone | Slider (Grok) | `DeadZoneSize` | 0–2 step 0.1, default 0.5 |
| Rotations | Min / max turn | Slider ×2 (Grok) | `MinTurnSpeed` / `MaxTurnSpeed` | 1–90 / 5–180, default 5.0 / 25.0 |
| Rotations | Accel / decel | Slider ×2 (Grok) | `Acceleration` / `Deceleration` | 0.1–10, default 2.5 / 1.5 |
| Rotations | Noise | Slider (Grok) | `NoiseStrength` | 0–2, default 0.2 |
| LiquidBounce controls | Dead zone | Slider, wrapper visible when `IsLiquidBounce` | `DeadZoneSize` | same property as Grok |
| LiquidBounce controls | Min / max speed | Slider ×2 | `MinTurnSpeed` / `MaxTurnSpeed` | same properties |
| LiquidBounce controls | Acceleration / deceleration | Slider ×2 | `Acceleration` / `Deceleration` | same properties |
| LiquidBounce controls | Overshoot | CheckBox | `UseOvershoot` | default true |
| LiquidBounce controls | Strength / recovery | Slider ×2, `IsEnabled` ← `OvershootControlsEnabled` | `OvershootStrength` / `OvershootRecovery` | 0–20 / 0.01–1, default 5.0 / 0.2 |
| LiquidBounce controls | Noise | Slider | `NoiseStrength` | shared with Grok |
| LiquidBounce controls | Randomize | CheckBox | `Randomize` | default true |
| LiquidBounce controls | Random / Y range | Slider ×2, `IsEnabled` ← `RandomizeControlsEnabled` | `RandomizeRange` / `YRandomizeStrength` | 0–1, default 0.4 / 0.3 |
| LiquidBounce controls | LB horizontal / vertical | Slider ×2 | `LiquidBounceHorizontalSpeed` / `LiquidBounceVerticalSpeed` | 1–180, default 180 / 180 |
| LiquidBounce controls | LB smooth | Slider | `LiquidBounceSmoothFactor` | 0.1–1, default 0.5 |
| LiquidBounce controls | LB prediction | CheckBox | `LiquidBouncePredict` | default true |
| LiquidBounce controls | *(predict size, unlabelled)* | Slider, `IsEnabled` ← `PredictControlsEnabled` | `LiquidBouncePredictSize` | 0–3, default 1.0 |
| LiquidBounce controls | LB randomize | CheckBox | `LiquidBounceRandomize` | default true |
| LiquidBounce controls | *(randomize range, unlabelled)* | Slider, `IsEnabled` ← `LiquidRandomizeControlsEnabled` | `LiquidBounceRandomizeRange` | 0–1, default 0.5 |
| LiquidBounce controls | Horizontal search | Slider | `LiquidBounceHorizontalSearch` | 0–1, default 0.5 |
| LiquidBounce controls | Body min / max | Slider ×2 | `LiquidBounceBodyPointMin` / `LiquidBounceBodyPointMax` | 0–1, default 0.1 / 0.9 |
| Filters and display | Through walls | CheckBox | `ThroughWalls` | default true |
| Filters and display | Require attack held | CheckBox | `RequirePress` | default false |
| Filters and display | Allow mining | CheckBox | `AllowMining` | default true |
| Filters and display | Weapons only | CheckBox | `WeaponsOnly` | default true |
| Filters and display | Allow tools | CheckBox, `IsEnabled` ← `AllowToolsEnabled` | `AllowTools` | default false |
| Filters and display | Inventory check | CheckBox | `InventoryCheck` | default true |
| Filters and display | Bot check | CheckBox | `BotCheck` | default true |
| Filters and display | Teams | CheckBox | `Teams` | default true |
| Filters and display | Players | CheckBox | `Players` | default true |
| Filters and display | Bosses | CheckBox | `Bosses` | default false |
| Filters and display | Mobs | CheckBox | `Mobs` | default false |
| Filters and display | Animals | CheckBox | `Animals` | default false |
| Filters and display | Golems | CheckBox | `Golems` | default false |
| Filters and display | Silverfish | CheckBox | `Silverfish` | default false |
| Filters and display | Visualize aim | CheckBox | `VisualizeAim` | default true |
| Filters and display | Smooth back | CheckBox | `SmoothBack` | default true |
| Filters and display | Show target | ComboBox (none, default) | `ShowTarget` | default `none` |
| Filters and display | Debug | ComboBox (none, health) | `DebugLog` | default `none` |

Derived/conditional members that must also exist for the GUI to behave: `IsLiquidBounce`,
`IsHypixelRotation`, `IsGrokRotation`, `IsSwapAutoBlock`, `OvershootControlsEnabled`,
`RandomizeControlsEnabled`, `PredictControlsEnabled`, `LiquidRandomizeControlsEnabled`,
`AllowToolsEnabled`, plus `Clone()` / `CopyFrom()` used by the profile round-trip. `SetMode` normalises
and falls back to the first allowed value; `Set` raises `PropertyChanged` + `Changed`.

### Stats tab (`DataContext = StatsTracker.Instance`)

| Control (`x:Name`) | Type | Bind target | Range, default |
| --- | --- | --- | --- |
| — ("Playtime") | TextBlock in `ControlCardStyle` Border | `TotalPlaytimeText` | — |
| — ("Clicks") | TextBlock | `TotalClicks`, plus `LeftClicks` / `RightClicks` runs | — |
| — ("Avg CPS") | TextBlock | `AverageCpsText`, plus `PeakCpsText` | — |
| — ("Reset Stats") | Button | `ResetStatsButton_Click` → `StatsTracker.Instance.Reset()` | — |
| — (`CpsBuckets`) | ItemsControl (`StackPanel Orientation=Horizontal`) | `CpsBuckets`; item binds `BarHeight`, `Label`, `CountText`, `Visibility` | bar row height 124 |
| — ("No samples yet") | TextBlock `DataTrigger` | visible when `HasCpsSamples == false` | — |
| — (sample count) | TextBlock runs | `CpsSampleCount` | — |

### Settings tab

| Panel | Control (`x:Name`) | Type | Bind target / handler | Notes |
| --- | --- | --- | --- | --- |
| Configs | `ConfigComboBox` | ComboBox | filled by `RefreshConfigList()` | `DarkComboBox`, MinWidth 160 |
| Configs | — ("Load") | Button | `LoadConfigButton_Click` → `ApplyLoadedProfile(Profile)` | — |
| Configs | — ("Save") | Button | `SaveConfigButton_Click` | — |
| Configs | — ("Delete") | Button | `DeleteConfigButton_Click` | — |
| Configs | `NewConfigNameBox` | TextBox | `NewConfigNameBox_KeyDown` (Enter) / `CreateConfigButton_Click` | — |
| Configs | — ("Create") | Button | `CreateConfigButton_Click` | — |
| Configs | — ("Open Configs Folder") | Button | `OpenConfigsFolderButton_Click` | — |
| Configs | `ConfigStatusText` | TextBlock | `SetConfigStatus(string)` | — |
| Reload Mappings | `ReloadMappingsAvailabilityText` | TextBlock | gated on `GameStateClient.SupportsSetting("reloadMappingsNonce")` | — |
| Reload Mappings | `ReloadMappingsButton` | Button | `ReloadMappingsButton_Click` | — |
| GUI Customization | 18 palette Buttons (`Tag` = Slate, Ink, Graphite, Steel, Blend, Lush, Water, Lime Water, Digital Horizon, Coral, Magic, Blossom, Pastel, Sunkist, Nord, Cherry, Aubergine, Snowy Sky) | Button `PreviewCardButtonStyle` | `GuiPaletteButton_Click` → `ApplyGuiTheme(name)` + `Clicker.GuiTheme` | Width 170; each shows a 4-swatch preview |
| GUI Customization | — ("Current palette: ") | TextBlock runs | `Clicker.GuiTheme` | — |
| GUI Customization | `CustomPalettesItems` | ItemsControl (`WrapPanel`) | cards built by `RebuildCustomPaletteCards`; item binds `Name`, `BackgroundSwatch`, `PanelSwatch`, `SliderSwatch`, `AccentSwatch` | — |
| GUI Customization | custom-card `✕` | Button | `DeleteCustomPaletteButton_Click` (`Tag` = name) | refuses built-in names |
| GUI Customization | custom-card preview | Button | `GuiPaletteButton_Click` (`Tag` = name) | — |
| GUI Customization | `NewPaletteNameBox` | TextBox | read by `AddCustomPaletteButton_Click` | — |
| GUI Customization | `PaletteBgBox` / `PalettePanelBox` / `PaletteAccentBox` / `PaletteTextBox` | TextBox ×4 | `PaletteHexBox_TextChanged` → `Palette*Preview.Background` | initial `#0A0B0F`, `#12141A`, `#C7625A`, `#E8EAEE` |
| GUI Customization | `PaletteBgPreview` / `PalettePanelPreview` / `PaletteAccentPreview` / `PaletteTextPreview` | Border ×4 | `ColorSwatch_MouseLeftButtonDown` → `ColorPickerDialog.TryPick` (`Tag` = target TextBox) | click-to-pick |
| GUI Customization | — ("Add palette") | Button | `AddCustomPaletteButton_Click` → `PaletteStore.Upsert` | — |
| GUI Customization | `PaletteStatusText` | TextBlock | `SetPaletteStatus(string)` | — |
| GUI Customization | 7 module-list style Buttons (`Tag` = Default, Minimal, Outlined, Glass, Bold, Classic, Modern) | Button `PreviewCardButtonStyle` | `ModuleStyleButton_Click` → `Clicker.ModuleListStyle` + `NormalizeModuleListStyle` | Width 170; each shows a miniature module-list row |
| GUI Customization | — ("Current module list style: ") | TextBlock runs | `Clicker.ModuleListStyle` | — |
| GUI Customization | — ("Show client watermark (latency · aoko · FPS)") | CheckBox `ModuleSwitch` | `ShowLogo` TwoWay | default true |
| GUI Customization | — ("Dev Mode") | CheckBox `ModuleSwitch` | `DevMode` TwoWay | default false; enables the Kill Aura card and DevOnly search rows |

### Left rail (outside the tab bodies)

| Control (`x:Name`) | Type | Bind target / handler | Notes |
| --- | --- | --- | --- |
| `ModuleSearchButton` | Button (ghost style: search glyph + "Search" + "ctrl k") | `ModuleSearchButton_Click` → `OpenCommandSearch(null)` | ToolTip "Search modules and tabs (Ctrl+K)" |
| `UpdateStatusButton` | Button | `UpdateStatusButton_Click` → opens `_latestReleaseUrl` or `CheckForUpdatesAsync()` | — |
| `UpdateStatusText` | TextBlock | `SetUpdateStatus(status, version)` via `ControlPanel.Template.FindName` | initial "Checking for updates..." |
| `CurrentVersionText` | TextBlock | same | — |
| `LoaderPanel` | StackPanel | visible before `EnterControlMode()` | holds `InjectButton`, `InjectionStatusText`, `InjectionProgressBar` |
| `InjectButton` | Button | `InjectButton_Click` → `RunInjectionAsync` | `DarkButton` |
| `InjectionStatusText` | TextBlock | `UpdateGameStateUI()` | "Status: Not Injected" |
| `InjectionProgressBar` | ProgressBar | `GameStateClient.InjectionProgress` | 0–100, `Collapsed` |
| `ControlPanel` | TabControl | the 9 tabs | `TabStripPlacement=Left`, `ControlCenterTabControlStyle` |
| `CommandSearchHost` | `views:CommandSearchOverlay` | `StateProvider`, `JumpRequested`, `ToggleRequested`, `Closed` wired in the constructor | `Margin="-20"`, overlays the tab control |

Window chrome: `Title="aoko client"`, loader 360×310 `NoResize`; `EnterControlMode()` switches to
1020×760, `MinWidth=560`, `MinHeight=460`, `CanResizeWithGrip`, and repaints the DWM title bar through
`DwmSetWindowAttribute` (dark mode 20, border 34, caption 35, text 36, colours from the palette).

---

## View surface

### Tabs in `MainWindow.xaml` (9 total)

| # | Tab header | Panels (in order) | Notes |
| --- | --- | --- | --- |
| 1 | Combat | `AutoClickerCard`, `RightClickCard`, `AimAssistCard`, `TriggerbotCard`, `AutoRodCard`, `ThrowpotCard`, `AutoHealCard`, `HitDelayFixCard` | `ScrollViewer` with `HorizontalContentAlignment=Stretch` |
| 2 | Render | `NametagsCard`, `NickHiderCard`, `FightStatusCard`, `ChestEspCard`, `BlockEspCard`, `BedPlatesCard`, `FireballWarningCard`, `ClosestPlayerCard`, module-list card (unnamed), `HudEditorCard`, `AntiDebuffCard` | — |
| 3 | Utility | `SpeedBridgeCard`, Discord RPC card (unnamed), `GtbHelperCard`, `PixelPartyAssistCard`, `ChestStealerCard`, `RefillCard`, `AutoToolCard`, `PanicCard` | — |
| 4 | Risky | `ReachCard`, `VelocityCard`, `AutoTotemCard` | — |
| 5 | Beta | Beta banner card (unnamed), `KillAuraCard` (DevOnly), `JumpResetCard`, `HitSelectCard`, `WTapCard`, `TrajectoriesCard`, `MurderMysteryCard`, `BreakProgressCard`, `NoHurtCamCard`, `TntTimerCard`, `FastPlaceCard`, `SprintCard` | banner text: "These modules were not thoroughly tested. Use them at your own risk." |
| 6 | Accounts | `views:AccountsView x:Name="AccountsTab"` | the whole tab is the view |
| 7 | Servers | `views:ServersView x:Name="ServersTab"` | the whole tab is the view |
| 8 | Stats | 3 summary cards (`Playtime`, `Clicks`, `Avg CPS`), Reset Stats button, CPS-distribution card | `DataContext="{x:Static core:StatsTracker.Instance}"` |
| 9 | Settings | Configs card, Reload Mappings card, GUI Customization card | largest panel in the app |

Cross-cutting styles defined in `Window.Resources`: `PreviewCardButtonStyle` (palette/style preview
buttons, `CornerRadius=6`, hover repaints to `PanelBrush` + `AccentBrush`), `ControlCardStyle`
(`PanelBrush`, `CornerRadius=6`, `Padding=12`, hover border `SliderFgBrush`),
`ControlCenterTabItemStyle` (150 wide, 50 min height, hover scale 1.035, selected accent bar,
`BackEase` amplitude 0.6, drop shadow 0.55/16), `ControlCenterTabControlStyle` (176 px rail + 10 gap +
content), plus `HexColorToBrushConverter`, `ControlCenterTabSelectedBrush` `#1F2229`,
`ControlCenterTabHoverBrush` `#181B22`.

### Separate views and helpers under `Aoko/Views/`

| File | Purpose | Key controls | Delegates to |
| --- | --- | --- | --- |
| `AccountsView.xaml` / `AccountsView.xaml.cs` (515 / 1169 lines) | Saved-login tab: import, card grid, session strip, Lunar mirror | `CurrentDot`, `SourceChip`, `SourceTint`, `ImportFileButton`("Import file"), `PasteImportButton`("Paste"), `AddOfflineButton`("Add offline"), `PastePanel` + `PasteBox` + `PastePlaceholder` + `PasteValidationText` + `PasteAuthenticateButton` + `PasteCancelButton`, `OfflinePanel` + `OfflineNameBox` + `OfflineValidationText` + `OfflineAddButton` + `OfflineCancelButton`, `SessionStrip` with `ChipOffline` / `ChipMainMenu` / `ChipWorld`, `RefreshSessionButton`("Refresh"), `SessionNameText`, `SessionUuidText`, `CardsControl` (ListBox; styles `AccountCardItem`, `AccountCardList`, `AccountCardTemplate`), `EmptyState`, `DropHint`, `BusyBar`, `PrimaryHint`, `DeleteButton`("Delete"), `UseAccountButton`("Use account"), `RestoreSessionButton`("Restore launch account"), `StatusText`, `RecoveryText`, `StoragePathText`, `OpenFolderButton`("Open folder"), `LunarAccountsCheck`("Write saved accounts into Lunar"), `LunarStatusText` | `AccountsViewLogic` (chips, gating, labels), `AccountStore`, `AccountImportService`, `CookieAuth`, `CookieParser`, `LunarAccountManager`, `AccountSettings`, `StoreRecovery`, `RelativeTime`, `TabCopy`, `BridgeCommandDispatcher` via `GameStateClient`. Handlers include `View_Loaded`/`View_Unloaded`, `Client_PropertyChanged`, `Client_StateUpdated`, `RefreshSession_Click`, `UseAccount_Click`, `RestoreSession_Click`, `ImportFile_Click`, `PasteImport_Click`, `PasteAuthenticate_Click`, `AddOffline_Click`, `OfflineAdd_Click`, `CardsControl_SelectionChanged`, `DeleteAccount_Click`, `OpenFolder_Click`, `LunarAccounts_Click`, `View_DragEnter/DragOver/DragLeave/Drop`, `RunExclusiveAsync`, `SetBusy`, `UpdateActionStates`, `SetStatus` |
| `ServersView.xaml` / `ServersView.xaml.cs` (457 / 683 lines) | Saved-server tab: direct join, add form, ping, per-row latency chip | `DirectAddressBox` + `DirectJoinButton`("Join") + `DirectValidationText`, `ServersList` (ListBox, `ServerCardButton` row template with `Card`, `Tint`, `HoverWash`, selection rule), `EmptyState`, `AddFormCard` + `AddNameBox` + `AddAddressBox` + `AddFormAddButton`("Add") + `AddFormCancelButton`("Cancel") + `AddFormValidationText`, `AddServerButton`("Add server"), `JoinSelectedButton`("Join selected"), `PingAllButton`("Ping all"), `DeleteButton`("Delete"), `BusyBar`, `StatusLine`, `RecoveryText`, `PrimaryHint`; row-level latency chip button, double-click join, keyboard focus/selection | `ServersViewLogic` + `SavedServerRow` view model, `ServerStore`, `ServerAddress`, `ServerJoinService`, `ServerPing`, `StoreRecovery`, `TabCopy`, `BridgeCommandProtocol`. Handlers: `View_Loaded`/`View_Unloaded`, `ReloadServers`, `ApplySelection`, `UpdateActionStates`, `JoinDirect`, `JoinSelectedAsync`, `JoinTargetAsync`, `ServerCard_Click`, `ServerCard_GotKeyboardFocus`, `ServerCard_DoubleClick`, `SelectRow`, `LatencyChip_Click`, `PingAllButton_Click`, `DeleteButton_Click`, `AddServerButton_Click`, `AddFormAddButton_Click`, `AddFormCancelButton_Click`, `AddAddressBox_KeyDown`, `DirectAddressBox_KeyDown`, `PingSavedAsync` (rows + ids overloads) |
| `CommandSearchOverlay.xaml` / `CommandSearchOverlay.xaml.cs` (477 / 801 lines) | Ctrl+K / type-to-open module + tab finder, drawn as a scrim over the tab control | `Scrim`, `CardVisual`, `CardPosition`, `CardSlide`, `Track`, `Thumb`, `Accent`, `Chip` (`EscChipStyle`), `PlaceholderText`, `QueryBox`, `ResultsArea`, `ResultsScroll`, `ResultsList`, `RowName`, `EmptyText`, `ResultsCount`; resources `GlassHairlineBrush` `#2E8EB1CC`, `GlassSpecularBrush` `#14FFFFFF`, `EscChipStyle`, `RowSwitchStyle` (ModuleSwitch geometry with plain setters — deliberately no `IsChecked` storyboard) | `ModuleSearchIndex` (ranking + entries), `CommandSearchRowPolicy` (row state), `HighlightTextBlock` (match ranges), `MainWindow.GetCommandSearchRowState` provider; brushes `CommandSearchMatchBrush`, `CommandSearchMatchPillBrush`, `CommandSearchSubtleTextBrush`, `CommandSearchPlaceholderBrush` are built in `MainWindow.ApplyGuiTheme`. Public API: `IsOpen`, `Open(seed)`, `Close()`, `RefreshRows()`, events `JumpRequested` / `ToggleRequested` / `Closed` |
| `HighlightTextBlock.cs` (148 lines) | `TextBlock` subclass that paints matched character ranges | DPs `SourceText`, `MatchRanges` (`IReadOnlyList<ModuleSearchRange>`), `HighlightBrush`, `HighlightBackground`; rebuilds `Inlines` on change and ignores out-of-range ranges | `Aoko.Core.ModuleSearchRange` |
| `RelativeTime.cs` (27 lines) | One relative-timestamp formatter for both tabs | static `Format(DateTimeOffset)` → "just now", `Nm ago`, `Nh ago`, `Nd ago`, `Nmo ago`, `Ny ago` | `AccountsViewLogic.LastUsedLabel`, `ServersViewLogic.LastJoinedLabel` |
| `TabCopy.cs` (64 lines) | Shared refusal/hint sentences with a single source of truth | consts `SelectAccountHint`, `SelectServerHint`, `NoServersHint`, `UnsupportedAccountSwitch`, `UnsupportedServerJoin`, `StatusLimit = 110`; `NotConnected` and `ReturnToMainMenu` derived from `BridgeCommandResult.Failure(...)`; `BrushKey(StatusTone)` → `AccentBrush` / `CriticalBrush` / `DimTextBrush`; `Shorten` cuts at a word boundary and appends `…` | `BridgeCommandResult`, `BridgeActions` |
| `StoreRecovery.cs` (144 lines) | "A corrupt store was quarantined this session" notice for both tabs | consts `AccountsFileName`, `ServersFileName`, `QuarantineMarker = ".corrupt-"`; `DefaultDirectory`, `SessionStartUtc` (process start time), `Message(path)`, `FromStore(object?)` (reflective `RecoveredFrom` hook), `FindQuarantined(...)` | `AccountStore.DefaultDirectory`, `Process.GetCurrentProcess()` |
| `TabStyles.xaml` (269 lines) | One button/chip language for the Accounts + Servers tabs | brushes `CriticalBrush` `#F43F5E`, `WarnBrush` `#F5BE6A`; styles `TabFocusVisual`, `TabPrimaryButton`, `TabSecondaryButton`, `TabDangerButton`, `TabTextBox`, `TabChip`, `TabChipTint`, `TabChipText`, `TabCardWash`, `TabSelectionRule`, `TabSectionTitle`, `TabCaption`, `TabStatusText`, `TabEmptyTitle`, `TabEmptyBody` | merged by both views as `<ResourceDictionary Source="TabStyles.xaml"/>`; the destructive red is fixed on purpose so "delete"/"failed" cannot be repainted into decoration |
| `AccountsViewLogic.cs` (145 lines) | WPF-free decisions for the Accounts tab | enums `SessionChipTone`, `AccountSourceTone`; `InvalidOfflineName` copy; `SessionChipToneFor`, `CanSwitchSessions`, `CanUseSelectedAccount`, `SelectionIndexAfterLoad` (never falls back — returns `-1`), `SessionSwitchRefusal`, `PrimaryHint`, `SessionCaption`, `SourceChipLabel`, `SourceToneFor`, `IsCurrentSession`, `FormatUuid`, `LastUsedLabel`, `CardAutomationName` | `TabCopy`, `AccountStore.SameUuid`, `RelativeTime`, `AccountSources` |
| `ServersViewLogic.cs` (374 lines) | WPF-free decisions plus the `SavedServerRow` view model | enums `ServerStatusTone`, `ServerJoinState`; `SavedServerRow` (`INotifyPropertyChanged`; `Motd`, `StatusText`, `StatusTone`, `PlayersLabel`, `IsSelected`, `SelectionStatus`, `AutomationName`, `MarkPinging`, `MarkNotPinged`, `ApplyPing`, `ApplyPingFailure`, `ApplyInvalidAddress`, `CopyPingStateFrom`); consts `MaximumMotdLength = 160`, `EmptySelectionMessage`, `InvalidAddressChipLabel`, `NotPingedLabel` = "Not pinged", `PingingLabel` = "Pinging…"; functions `SortForDisplay`, `ResolveSelectionIndex`, `EvaluateJoinState`, `JoinBlockedMessage`, `PrimaryHint`, `CanStartPing`, `CanStartMutation`, `CanJoinDirect`, `CanJoinSelected`, `ValidationMessage`, `LastJoinedLabel`, `JoinStartedText`, `PingSummaryText`, `MotdText`, `PingLabel`, `PingTone`, `ErrorLabel`, `AddedText`, `DeletedText` | `TabCopy`, `RelativeTime`, `ServerPingResult`, `ServerAddress`, `ServerJoinService` |

### Dialogs, theming and application shell

| File | Purpose | Key controls | Delegates to / notes |
| --- | --- | --- | --- |
| `ColorPickerDialog.xaml` / `.xaml.cs` (113 lines) | Modal themed HSV picker: SV square + hue bar + RGB sliders + hex field | `SvCanvas`, `SvHueLayer`, `SvThumb`, `HueCanvas`, `HueThumb`, `PreviewSwatch`, `HexBox`, `RValue`/`RSlider`, `GValue`/`GSlider`, `BValue`/`BSlider` | API `ColorPickerDialog(Color)`, `SelectedColor`, static `TryPick(Window owner, Color initial, out Color result)`. Geometry: SV 220×180, hue 180, window 360 wide, `SizeToContent=Height`, `CenterOwner`. Used by Block ESP row colours and the custom-palette swatches |
| `WindowPickerDialog.xaml` / `.xaml.cs` (104 lines) | Injection target chooser when several Java/Minecraft windows are running | `WindowList` (ListBox), `RefreshButton`, `VersionCombo` | `InjectionTargetDiscovery.ListTargets()`, `InjectionTarget`; exposes `SelectedTarget` and `SelectedVersion` (`"auto"` default); "Inject" refuses to close without a selection |
| `Aoko/Core/ThemeManager.cs` (258 lines) | Curated theme table + live application into `Application.Resources` | 18 themes (`Slate`, `Ink`, `Graphite`, `Steel`, `Blend`, `Lush`, `Water`, `Lime Water`, `Digital Horizon`, `Coral`, `Magic`, `Blossom`, `Pastel`, `Sunkist`, `Nord`, `Cherry`, `Aubergine`, `Snowy Sky`), each with `Background`, `Panel`, `SliderBg`, `SliderFg`, `Accent`, `AccentSecondary?`, `Text`, `DimText` | `CurrentTheme`, `ThemeChanged` event, `GetThemeNames()`, `ApplyTheme(name)` writes the seven `*Color` and seven `*Brush` resources. Note: `MainWindow` keeps its **own** richer `GuiPalettes` table (adds `SliderForeground`, `TabSelected`, `TabHover`) and `ApplyGuiTheme` is the path the GUI actually uses — two tables, must be reconciled rather than merged blindly |
| `Aoko/Core/CustomPalette.cs` (109 lines) | User palette model + `%AppData%\Aoko\palettes.json` store | `CustomPalette`: `Name`, `Background`, `Panel`, `SliderBackground`, `SliderForeground`, `Accent`, `Text`, `DimText`, `TabSelected`, `TabHover` — all `#RRGGBB` strings, camelCase JSON, `WriteIndented` | `PaletteStore.FilePath`, `LoadAll()`, `SaveAll()`, `Upsert(palette)` (case-insensitive), `Delete(name)`; every failure degrades to an empty set |
| `App.xaml` (441 lines) | Application resources: theme tokens and shared control styles | colours `BgColor` `#0A0B0F`, `PanelColor` `#12141A`, `SliderBgColor` `#181B22`, `SliderFgColor` `#2A2F38`, `AccentColor` `#C7625A`, `TextColor` `#E8EAEE`, `DimTextColor` `#7A8290` + matching brushes; styles `DarkComboBox`, `DarkCheckBox`, `ModuleSwitch`, `DarkSlider`, `DarkButton`; `BooleanToVisibilityConverter` (`StartupUri="MainWindow.xaml"`) | `ThemeManager` / `MainWindow.ApplyGuiTheme` overwrite the brush keys at runtime |
| `App.xaml.cs` | Process wiring | — | `AOKO_SOFTWARE_RENDERING=1` → `RenderMode.SoftwareOnly`; `OnStartup` installs `InputHooks`; `OnExit` stops Discord RPC, uninstalls hooks, stops `Clicker` |

---

## Test suite parity

`Aoko.Tests/Aoko.Tests.csproj` plus 52 `.cs` files: 50 assertion files (one per row below, using
`[Fact]` / `[Theory]`) and 2 infrastructure files. Counts are `public void` / `public async Task` test
methods declared in the file (a theory contributes one method plus N `InlineData` rows). The crate
column is derived from the subject the file exercises.

| Test file | ~methods | What it asserts | Rust crate |
| --- | --- | --- | --- |
| `AccountImportTests.cs` | 39 | Cookie/token/launcher-file parsing shapes (labelled + bare `localts`, seven-field Netscape, `HttpOnly` prefix, space-separated Netscape, semicolon header, bare + bearer + JSON-wrapped MC access token, JWT without profile rejected); offline UUID equals `nameUUIDFromBytes("OfflinePlayer:"+name)`; `AccountStore` camelCase and newline-safe round-trip; quarantine on a corrupt file; upsert by dashed/undashed UUID; cracked + premium coexistence; no source downgrade; `UpdateTokens` / `MarkUsed` stamps; Lunar upsert preserves unknown keys and refuses offline accounts; import retry/backoff honouring `Retry-After`; batch failure aggregation; `ThrowIfMcaExpired` | `aoko-core` |
| `AccountStoreFailureTests.cs` | 10 | A locked, directory-shaped or unreadable `accounts.json` never loses already-loaded credentials and is never rewritten from an empty list; an import during a read failure must not clobber; `AccountSettings` keeps its loaded value on a read failure | `aoko-core` |
| `AccountsViewTests.cs` | 29 | `AccountSettings` defaults (`lunarWrites` disabled), camelCase + atomic round-trip, case-insensitive property names, corrupt-file fallback; session chip / caption / source chip / tone / refusal / primary-hint rules; nothing is armed without a selection; the XAML keeps its named elements and handlers | `aoko-app` |
| `AimAssistMotionControllerTests.cs` | 11 | Version-independent motion: equivalent telemetry produces equivalent movement, strength only pushes toward the modern baseline, FOV eligibility plus a safe default for an invalid FOV, resolution scaling, range/screen-bounds exclusion, looking-at-entity suppression and residual clearing, latch vs immediate reacquisition, target handoff, smoothing direction preservation, reset when no target qualifies | `aoko-core` |
| `AutoRodManagedTests.cs` | 10 | Model/profile defaults, clamps and the verification override; profile round-trip preserves settings + action bind; bind conflicts rejected in both directions; a profile conflict keeps the general bind and clears the action bind | `aoko-protocol` |
| `AutoToolManagedTests.cs` | 5 | Defaults and clamps, `Profile` create/apply round-trip, capability lists include AutoTool on every supported version, both bridges contain the native wiring, the XAML card is configured | `aoko-protocol` |
| `BlockEspTests.cs` | 12 | `NormalizeId` reduces to a path token, `NormalizeColor` yields six-digit hex or the default, serialisation includes only enabled entries and dedupes by normalised id | `aoko-protocol` |
| `BoundedLineReaderTests.cs` | 5 | CRLF and unterminated final line, line exactly at the limit accepted, oversized line discarded and reading continues, oversized-at-EOF reported once | `aoko-protocol` |
| `BridgeCapabilitiesActionsTests.cs` | 12 | The `actions` array is its own list (never modules/settings/state); case-insensitive parse; only advertised actions honoured; a missing array inherits the fallback; an empty document still returns the fallback | `aoko-protocol` |
| `BridgeCapabilitiesTests.cs` | 12 | Modern fallback exposes 26.2 features; legacy fallback exposes Pixel Party Assist, Kill Aura (but not silent aura), Chest Stealer, AutoRod, AntiDebuff, Fight Status; legacy has no Triggerbot; payload merge/normalise and fallback when arrays are missing | `aoko-protocol` |
| `BridgeCommandDispatcherTests.cs` | 13 | `requestId` correlation: reply matching, reply without action keeps the requested action, not-connected fails without sending, refused send reported, timeout, connection-drop failure, cancellation — all through injected send/connected seams | `aoko-core` |
| `BridgeCommandDispatchOrderTests.cs` | 5 | A `commandResult` frame routes to the reply handler and never to the state branch (any object deserialises into an empty `GameState`); capabilities frames route to the capabilities branch; a spaced reply still completes the request | `aoko-platform` |
| `BridgeCommandProtocolTests.cs` | 13 | Literal wire lines for `getSession` / `setSession` / `restoreSession` / `join`, blank refresh-token omission, one JSON object per line terminated by a newline, credential-free `ToString` | `aoko-protocol` |
| `BridgeCommandResultTests.cs` | 13 | `commandResult` decoding: contract reply, join-failure reply, missing optional fields use safe defaults, unknown action still decodes, error-code to user-sentence mapping | `aoko-protocol` |
| `ChestClickCheckTests.cs` | 4 | `IsChestClickAllowed`: checks off allows any container, no bridge state refuses, a menu container is refused, a physical chest is allowed | `aoko-core` |
| `ClickDeadlineSchedulerTests.cs` | 4 | First deadline one interval out, minor overshoot compensated on the next deadline, a missed interval resets without a catch-up burst, a fractional interval rounds the delay up | `aoko-core` |
| `ClickerRandomizationTests.cs` | 12 | `RandomizationMode` defaults to Medium with `JitterEnabled` true; index/property mapping; `JitterEnabled` false → Basic, true → restore Medium; index clamping; `Profile` default, round-trip and legacy `jitterEnabled` migration; advanced timing bounds; fatigue accumulation/recovery; butterfly doublets | `aoko-core` |
| `CoalescedCallbackTests.cs` | 2 | `CalculateDueTime` enforces the minimum interval; a burst collapses into one trailing callback | `aoko-core` |
| `CoalescedLatestValueTests.cs` | 3 | Pending publishes collapse to the latest value; one trailing callback for a value published during consumption; a disallowed schedule still retains the latest value | `aoko-core` |
| `CommandSearchOverlayTests.cs` | 23 | Row state: an unsupported module offers a disabled switch that reads off, a supported row flips module and switch together, a refused toggle cannot leave the switch inverted, destinations are tabs and are not bridge-gated, DevOnly rows hidden until DevMode, `UnavailableReason` names the version gate, the row switch binds only what the policy allows. Overlay shape: refresh on state report, no `IsChecked` storyboard, every animation gated on system animation settings, shadow plate paints no frame, product disabled look, window floor keeps the card in its gutters, caret from the theme not the accent, results list name and count help text, rows are not tab stops and announce selection, Ctrl+Space survives an IME, space still types with a row selected, card tints derived from the palette, AA contrast on every palette including a light custom one | `aoko-app` |
| `CookieParserClaimTests.cs` | 4 | Non-string JWT claims fail as a domain error; a malformed Netscape line never quotes the cookie value; a no-parse-failure message echoes what the user pasted; a valid token with string claims still parses | `aoko-core` |
| `FightStatusWiringTests.cs` | 5 | `ProfileManager` maps Fight Status both ways, the TCP config payload carries the flag and its keybind, native capability payloads advertise setting + keybind, the local toggle updates the flag | `aoko-protocol` |
| `GameStateAndProfileTests.cs` | 20 | `GameState` contract (string and `JsonNode` parity), Chest Stealer state, coordinate mapper scaling and out-of-bounds rejection, `Profile` defaults stable, `chestStealerMenuCheck` + sub-checks defaults/round-trip and legacy migration, camelCase keys, Fight Status/AntiDebuff/new-module defaults and round-trips, `ProfileManager` Aoko profile dir + legacy copy + no overwrite + fail-open write, `ThemeManager` curated themes, `ModuleListStyle` index mapping | `aoko-protocol` |
| `GameStateClientInjectionTests.cs` | 2 | `ResolveInjectionTarget` returns the current process when the pid matches, and null for an invalid pid | `aoko-platform` |
| `GameStateClientSeamTests.cs` | 4 | `NormalizeDetectedVersion`, `ResolveBridgeDllName` maps modern versions to `bridge_261`, launcher command-line version parsing, `ModuleListStyleToIndex` maps styles safely | `aoko-platform` |
| `GameStatePatchMergerTests.cs` | 6 | Unmarked state is not treated as partial; an explicit patch preserves absent fields; a world-exit patch clears transient targets; a partial patch preserves Refill state | `aoko-protocol` |
| `GtbWordSolverTests.cs` | 3 | Mask extraction and expected-word matching, `maxResults` limit respected, `TryLearnSolvedWord` adds a word for future solves | `aoko-core` |
| `InjectionTargetDiscoveryTests.cs` | 3 | `Describe` recognises Lunar and a command-line MC version; the version is used as evidence for an unidentified Java process; `SortTargets` prefers likely clients and visible windows | `aoko-platform` |
| `KillAuraSettingsTests.cs` | 5 | Defaults match OpenMyau-Plus `fc7c95d`, `Profile` round-trip preserves nested settings, `Clone` is independent and retains conditional state, Grok rotation is accepted and exposes the skew property | `aoko-protocol` |
| `LegacyRenderOptimizationTests.cs` | 7 | **Source-text assertions on `McInjector/` C++**: menu close does not destroy valid renderer resources, the closest-player renderer consumes a prepared snapshot without JNI, the render entry captures config + HUD layout together, Chest ESP sorts by distance and uses chunk range, nametags use chunk range instead of a count cap, section scans share one chunk visitor, the held-item nametag setting is parsed and applied | `none` — the native bridge stays as-is; keep it as a C++ gate |
| `MainWindowStartupTests.cs` | 1 | `MainWindow` can be constructed and closed (STA) — the only end-to-end WPF smoke test | `aoko-app` |
| `ManagedTransportDiagnosticsTests.cs` | 11 | Diagnostics flag reflects config; config send policy (revision or heartbeat); serialisation reuses unchanged payloads; change tracking covers properties, nested settings and keybinds; the flag is carried to both native bridges; modern fast-state uses an explicit partial envelope; the entity producer clears its snapshot when consumers turn off; native float config uses validated defaults; disabled diagnostics produce no snapshots; a snapshot atomically reports and resets; log lines carry rates but no payload content | `aoko-protocol` |
| `ModuleRegistrationTests.cs` | 19 | Catalog ids unique; capability modules match modern bridge capabilities and the native modern capabilities JSON; stable managed fields advertised; and every catalog surface is satisfied: keybind GUI buttons/handlers, keybind maps, Discord RPC counting, overlay list probes, `SettingSpec` ↔ `Profile` ↔ TCP config payload ↔ capability lists ↔ both bridges' `ParseConfig` | `aoko-protocol` |
| `ModuleSearchIndexTests.cs` | 18 | Ranking: empty query returns every entry in catalog order, no-results returns empty, prefix > word-prefix > substring > subsequence tiers; every catalog GUI module has a search row whose tab, card `x:Name` and toggle property exist in `MainWindow.xaml` | `aoko-core` |
| `NickHiderConfigTests.cs` | 4 | `NormalizeAlias` trims plain text, `NormalizeInput` keeps typing whitespace until send, invalid text rejected, `Profile.NickHider` round-trip | `aoko-protocol` |
| `OwnedAsyncLoopTests.cs` | 3 | Stop-then-start runs only the latest generation, an unexpected completion is restarted and reported, a failure callback does not prevent cleanup | `aoko-core` |
| `PaletteContrastTests.cs` | 5 | Contrast measured on the real palette tables: primary labels pass AA on the background brush, chip labels pass AA on their tint layers, status tones pass AA on the tab background, card edge/wash values meet recorded lower bounds | `aoko-app` |
| `ProtocolCompatibilityTests.cs` | 5 | Captured V1 messages from both bridge families deserialise; the modern fixture ignores unknown future fields; loader-newer/bridge-older absent optionals use safe defaults; bridge-newer/loader-older unknown capability entries are tolerated; capability fallbacks expose the published state and held-item settings | `aoko-protocol` |
| `ServerAddressTests.cs` | 15 | Accepted forms, rejected forms, a trailing colon keeps a bare host, the 256-character cap, `DefaultPort = 25565` | `aoko-protocol` (parse half; the SRV query half is tested in `aoko-platform`) |
| `ServerPingTests.cs` | 15 | `ParseStatus` reads players and version, survives a minimal response, labels fall back to the error, `FlattenMotd` joins components and strips colour codes | `aoko-platform` |
| `ServerStoreTests.cs` | 12 | Store path under `%AppData%\Aoko`, `Add` dedupes by parsed host+port, camelCase indented atomic write, load restores entries and join stamps, `Remove` reports whether it removed | `aoko-core` |
| `ServersViewTests.cs` | 31 | `ServersViewLogic` / `SavedServerRow`: sort order (most recent join first, never-joined last, then `AddedAt`, then name), selection index retention and neighbour fallback, chip wording/tone thresholds (<100 ms accent, ≤300 warn, else critical), join gate states, pings allowed alongside the exclusive lock, MOTD trimming, error-label mapping | `aoko-app` |
| `SrvLookupCacheTests.cs` | 5 | An SRV failure expires (10-minute TTL) so the next lookup asks again, a success is reused for one hour (1-hour TTL), a later success replaces a cached failure, hosts are matched case-insensitively | `aoko-platform` |
| `StatsTrackerTests.cs` | 5 | `RecordClick` updates CPS buckets/average/peak, the detailed CPS window is tight, left/right counts are separated, `RecordPlaytimeSample` only advances for active play | `aoko-protocol` |
| `StoreRecoveryTests.cs` | 6 | `Message` names the store and the quarantine copy, `FromStore` reads the optional `RecoveredFrom` hook, `FindQuarantined` only counts this session's files and ignores other stores and missing folders | `aoko-app` |
| `TabCopyTests.cs` | 6 | Both tabs explain a missing capability with identical words taken from the bridge layer; not-connected and return-to-main-menu copy follow the bridge message; capacity refusals differ only by the action refused | `aoko-app` |
| `TcpPortHelperTests.cs` | 2 | The listening-port regex matches expected `netstat` lines; `TryGetListeningProcessId` returns null or a positive pid | `aoko-protocol` |
| `TestAssembly.cs` | 0 | Assembly-level attribute only (test infrastructure) | `none` — test infrastructure |
| `TestRepo.cs` | 0 | Locates the repository root via `CallerFilePath` so source-assertion tests can read `Aoko/**` and `McInjector/**`; throws when `Aoko/Core/GameStateClient.cs` cannot be found | `none` — test infrastructure |
| `UpdateServiceTests.cs` | 2 | `IsNewerRelease` compares semantic versions; `ParseReleaseVersion` accepts a GitHub `v` tag prefix and build metadata | `aoko-core` |
| `WindowDetectionTests.cs` | 2 | `SortWindowTargets` puts JVM entries first then alphabetical by title; `WindowTarget.DisplayLabel` marks JVM targets | `aoko-platform` |
| `XamlBindingTests.cs` | 4 | Every `StaticResource` key used by `AccountsView.xaml`, `ServersView.xaml` and `TabStyles.xaml` is defined; every `DynamicResource` key is defined; server-row bindings match `SavedServerRow`; account-card bindings match the card model | `aoko-app` |

---

## Unpushed local commit coverage

`git log --oneline -1` → `f1a9f5f feat(gui): add module search, accounts, and servers tabs`.
`git status -sb` on `rust` shows no `origin/rust`, so this commit (84 files, +21626/−64) exists only
locally. None of it may be dropped.

### Features introduced by `f1a9f5f` → Rust target

| Feature | Source files added/changed | Rust crate | Migration note |
| --- | --- | --- | --- |
| Module search overlay (window, scrim, card geometry, keyboard model, announcements) | `Aoko/Views/CommandSearchOverlay.xaml`, `.xaml.cs` | `aoko-app` | GPUI overlay layer above the tab body: `Scrim`, `CardVisual`, `Track`/`Thumb`, `QueryBox`, `ResultsList`, `EmptyText`, `ResultsCount`. Keep Ctrl+K, type-to-open (printable char outside text fields), Esc close, arrow move, Enter jump, Ctrl+Space toggle, focused-element restore on close, cached blur on `ControlPanel` |
| Module search ranking + entries | `Aoko/Core/ModuleSearchIndex.cs` (`ModuleSearchRange`, `ModuleSearchEntry`, `ModuleSearchHit`, `ModuleSearchIndex`, `GuiRow`, `DestinationRow`) | `aoko-core` | Rank tiers `0/10/20/25/30/40/50/60`; `GroupOrder = Combat, Render, Utility, Risky, Beta, Tabs`; per-entry `Tab` / `CardName` / `ToggleProperty` / `AvailabilityLabel` / `Synonyms` / `HasKeybind` / `DevOnly` / `IsDestination` / `Order`. The `CardName` strings must reconcile with whatever `aoko-app` names its cards |
| Row-state policy | `CommandSearchRowState`, `CommandSearchRowPolicy.Resolve` / `.UnavailableReason` in `ModuleSearchIndex.cs` | `aoko-core` | An unsupported module reads off; `UnavailableReason` distinguishes `triggerbot` ("Unavailable on 1.8.9 (cooldown-era PvP only)") from the generic "Unavailable on current bridge" |
| Rail search button, shortcuts, jump/toggle plumbing, card pulse | `Aoko/MainWindow.xaml` (`ModuleSearchButton`), `Aoko/MainWindow.xaml.cs` (`ModuleSearchButton_Click`, `Window_PreviewTextInput`, `Window_PreviewKeyDown`, `CanOpenCommandSearch`, `OpenCommandSearch`, `CloseCommandSearch`, `CommandSearchHost_*`, `GetCommandSearchRowState`, `TryReadModuleEnabled`, `CreateModuleStateGetters`, `FindModuleCard`, `SelectTab`, `PulseModuleCard`, `RemoveCardPulse`, `CardPulseAdorner`) | `aoko-app` | Pulse = 1.2 s accent ring drawn through an adorner, skipped when `SystemParameters.ClientAreaAnimation` is false |
| Match highlighting control | `Aoko/Views/HighlightTextBlock.cs` | `aoko-app` | DPs `SourceText`, `MatchRanges`, `HighlightBrush`, `HighlightBackground`; out-of-range ranges ignored so a stale range cannot throw |
| Palette-derived search tints | `MainWindow.ApplyGuiTheme` (`CommandSearchMatchBrush`, `CommandSearchMatchPillBrush`, `CommandSearchSubtleTextBrush`, `CommandSearchPlaceholderBrush`) | `aoko-app` | Tints must derive from the live palette, never hardcoded — `CommandSearchOverlayTests` asserts AA on every palette including a light custom one |
| Accounts: store + models | `Aoko/Core/Accounts/AccountStore.cs`, `StoredAccount.cs` (+`AccountSources`), `MinecraftProfile.cs`, `OfflineAccount.cs`, `AccountSettings.cs` | `aoko-core` | `%AppData%\Aoko\accounts.json` and `accounts-settings.json`; temp-file atomic writes; quarantine rename instead of delete; camelCase JSON; loaded-value retention on read failure; offline UUID = MD5 of `OfflinePlayer:<name>`; `SameUuid` compares dashed/undashed forms |
| Accounts: credential parsing | `Aoko/Core/Accounts/CookieParser.cs` (`CookieEntry`, `ParsedCookies`, `FromPath`, `FromText`, `ToSisuCookieHeader`, `Pair`) | `aoko-core` | Netscape, cookie header, `localts` token, bare/bearer/JSON MC token; a failure message must never quote a cookie value |
| Accounts: auth chain | `Aoko/Core/Accounts/CookieAuth.cs`, `AccountImportService.cs` (`AccountImportResult`, `AccountImportFailure`, `AccountImportProgress`, `AccountBatchImportResult`) | `aoko-core` | SISU redirect walk from `login.live.com`, launcher client id, `t=` ticket prefix, retry/backoff honouring `Retry-After` clamped to two minutes, three attempts, batch failures aggregated by file name, `ThrowIfMcaExpired` |
| Accounts: Lunar mirror | `Aoko/Core/Accounts/LunarAccountManager.cs` (`LunarAccountWriteResult`, `Upsert`, `UpsertAll`) | `aoko-core` | Writes Lunar's own `accounts.json`: undashed local ids, Xbox XUID remote ids, MSA refresh tokens when known, unknown keys preserved, offline accounts refused, one pending active account per batch, errors carry no filesystem path |
| Accounts tab UI | `Aoko/Views/AccountsView.xaml`, `.xaml.cs`, `AccountsViewLogic.cs` | `aoko-app` | `PastePanel` / `OfflinePanel`, session strip chips, card grid (`AccountCardTemplate`), drag-drop import, `LunarAccountsCheck`, `RecoveryText`, `StoragePathText`, exclusive-work guard (`RunExclusiveAsync`) |
| Servers: address model + SRV | `Aoko/Core/Servers/ServerAddress.cs` (`ServerAddress`, `TryParse`, `DefaultPort = 25565`, `MaximumTextLength = 256`), SRV lookup cache | `aoko-protocol` for the pure parse/validate half; `aoko-platform` for `ResolveForPingAsync` and the SRV cache, which open a `UdpClient` | Failure TTL 10 min, success cached for one hour, case-insensitive hosts |
| Servers: ping | `Aoko/Core/Servers/ServerPing.cs` (`ServerPingResult`, `QueryAsync`, `PingManyAsync`, `ParseStatus`, `FlattenMotd`, `ProtocolVersion = 767`) | `aoko-platform` (opens the socket) | Server List Ping handshake, status JSON decode, MOTD flattening |
| Servers: store | `Aoko/Core/Servers/ServerStore.cs`, `SavedServer.cs` | `aoko-core` | `%AppData%\Aoko\servers.json`, camelCase indented atomic write, dedupe by parsed host+port, `MarkJoined`, `Remove`, JSON shape stays compatible with lunar-cookies' `servers.json` |
| Servers: join validation | `Aoko/Core/Servers/ServerJoinService.cs` (`ServerTarget`, `ValidateTarget`, `TryBuildJoinRequest`, `MaximumHostLength = 256`) | `aoko-core` | Validation only, no socket; the join itself is a bridge command |
| Servers tab UI | `Aoko/Views/ServersView.xaml`, `.xaml.cs`, `ServersViewLogic.cs` (`SavedServerRow`) | `aoko-app` | Direct join, add form, ping all, per-row re-ping, latency chip tones, selection retention, pings never take the exclusive lock |
| Bridge command protocol (outbound) | `Aoko/Core/BridgeCommandProtocol.cs` (`BridgeActions`: `getSession`/`setSession`/`restoreSession`/`join`; advertised `account.getSession`/`account.setSession`/`account.restoreSession`/`server.join`; types `accountCommand`/`serverCommand`/`commandResult`; line terminator; `AccountCommand`, `ServerCommand`) | `aoko-protocol` | One JSON object per line on port 25590; credential-carrying commands must not print their payload |
| Bridge command result (inbound) | `Aoko/Core/BridgeCommandResult.cs` | `aoko-protocol` | Client codes `not_connected`, `send_failed`, `timeout`, `connection_lost`, `invalid_address`, `invalid_session_data`; bridge codes `in_world`, `unsupported_session_constructor`, `session_write_not_observed`, `game_thread_timeout`, `no_launch_session`, `connect_path_not_found`; each maps to one user sentence, with a safe default for unmapped codes |
| Bridge command dispatcher | `Aoko/Core/BridgeCommandDispatcher.cs` | `aoko-core` | `requestId` correlation, 20 s timeout, pending requests fail when the connection drops, send/connected seams injected so no socket is needed |
| Capability `actions` gating | `Aoko/Core/BridgeCapabilities.cs` (changed), `McInjector/src/main/cpp/bridge_capabilities.h` (changed) | `aoko-protocol` | Actions are a separate list from modules/settings/state; a missing action leaves the matching tab control disabled behind an "update aoko" message — never infer support from module or setting lists |
| Transport wiring for commands | `Aoko/Core/GameStateClient.cs` (changed) | `aoko-platform` | Command send path, `commandResult` routing that must not reach the state branch, capability refresh |
| Module toggle path change | `Aoko/Core/InputHooks.cs` (3-line change) | `aoko-platform` | Search rows toggle through the same path as keybinds so the two cannot drift |
| Shared tab chrome | `Aoko/Views/TabCopy.cs`, `TabStyles.xaml`, `StoreRecovery.cs`, `RelativeTime.cs` | `aoko-app` | One button/chip language, fixed destructive red, "corrupt store quarantined this session" notice, one relative-time formatter |
| Docs | `AGENTS.md`, `README.md`, `website/src/content/docs/gui/{accounts,module-search,servers}.md`, `website/astro.config.mjs`, `website/src/landing.html` | `none` — docs | Must describe the Rust build once the GUI moves |
| Native side | `McInjector/src/main/cpp/{bridge,bridge_261}.cpp`, `bridge_capabilities.h`, `jni_core/account_command_router.h`, `jni_core/helper_bridge.cpp`, `jni_core/session_helper_bridge.{h,cpp}`, `jni_core/scoped_env.h`, `jni_core/session_switcher.inc`, `src/main/java/com/aoko/helper/SessionSwitcher.java`, `gen_java_helper.sh`, `McInjector/.gitattributes`, `build*.bat`, `run_tests.bat`, `McInjector/tests/*` (including `tests/java/run_tests.sh`) | `none` — `McInjector/` is not rewritten | The Rust host must speak this exact protocol; `account_command_router_tests.cpp`, `session_helper_format_tests.cpp` and `SessionSwitcherStructuralTest.java` remain the native gates |

### Test files introduced by `f1a9f5f` → Rust target

| New test file | Target crate | First Rust port must cover |
| --- | --- | --- |
| `AccountImportTests.cs` | `aoko-core` | Parsers, offline UUID, store round-trip/quarantine/upsert, Lunar mirror, retry/backoff |
| `AccountStoreFailureTests.cs` | `aoko-core` | Locked/absent/corrupt store never loses loaded credentials |
| `AccountsViewTests.cs` | `aoko-app` | `AccountSettings`, session strip / chip / gate rules, XAML element names |
| `BridgeCapabilitiesActionsTests.cs` | `aoko-protocol` | `actions` list parsing, fallback inheritance, case-insensitivity |
| `BridgeCommandDispatchOrderTests.cs` | `aoko-platform` | `commandResult` never deserialises into `GameState` |
| `BridgeCommandDispatcherTests.cs` | `aoko-core` | Correlation, timeout, connection drop, cancellation |
| `BridgeCommandProtocolTests.cs` | `aoko-protocol` | Literal outbound lines |
| `BridgeCommandResultTests.cs` | `aoko-protocol` | Reply decode + sentence mapping |
| `CommandSearchOverlayTests.cs` | `aoko-app` | Row-state invariants, overlay geometry/animation, accessibility, contrast |
| `CookieParserClaimTests.cs` | `aoko-core` | Untrusted claims, no cookie value in messages |
| `ModuleSearchIndexTests.cs` | `aoko-core` | Rank tiers and every catalog module having a GUI row with a real card/toggle |
| `PaletteContrastTests.cs` | `aoko-app` | AA contrast on the real palette tables |
| `ServerAddressTests.cs` | `aoko-protocol` | Parse accepted/rejected forms, 256-char cap |
| `ServerPingTests.cs` | `aoko-platform` | Status parse, MOTD flatten, error fallback |
| `ServerStoreTests.cs` | `aoko-core` | Path, dedupe, atomic camelCase write, join stamps |
| `ServersViewTests.cs` | `aoko-app` | Sort, selection retention, chip wording/tone, dual locks |
| `SrvLookupCacheTests.cs` | `aoko-platform` | Failure TTL vs success reuse |
| `StoreRecoveryTests.cs` | `aoko-app` | Quarantine notice only for this session |
| `TabCopyTests.cs` | `aoko-app` | Shared refusal copy sourced from the bridge layer |
| `TestRepo.cs` | `none` — test infrastructure | Repo-root locator for the C# suite; no Rust port, but the Rust tests need an equivalent workspace-relative helper for fixtures |
| `XamlBindingTests.cs` | `aoko-app` | Every resource key and row/card binding path resolves |
| `ModuleRegistrationTests.cs` (modified, not added) | `aoko-protocol` | One-line change in this commit; the surface contract itself predates it |

---

## Unresolved / `unknown`

- `ManagedOnly` is never set to `true` in `ModuleCatalog`, so that column is uniformly `no`. A Rust port
  that starts using the flag must first decide what it means for surface checking.
- Two palette tables exist: `MainWindow.GuiPalettes` (10 roles, adds `TabSelected` / `TabHover`) and
  `Core/ThemeManager.Themes` (8 roles). Which one is canonical for the Rust theme model is
  **undetermined by the source** — `MainWindow.ApplyGuiTheme` uses its own table, while
  `ThemeManager.ApplyTheme` writes the same resource keys. Keep both observable until decided.
- `Aoko/Views/AccountsView.xaml.cs` (1169 lines) and `ServersView.xaml.cs` (683 lines) were read for
  handler names, fields and the panel set, not line-by-line for every message string; the copy itself
  lives in `TabCopy` / `AccountsViewLogic` / `ServersViewLogic` / `StoreRecovery`, which were read in
  full.
- `CommandSearchOverlay.xaml` (477 lines), `AccountsView.xaml` (515 lines) and `ServersView.xaml`
  (457 lines) were read for named elements, styles, `Content` strings and template keys. Individual
  `Grid` / `Border` metrics beyond the ones recorded here (search card 340 wide, overlay gutters 56,
  switch 46×24, thumb 18) are `unknown` and need a pixel pass.
- `PaletteContrastTests` records specific contrast ratios that are not quoted here; the thresholds live
  in that test and in `TabStyles.xaml` comments (for example the accent label "4.99:1", AA at 12 px).
  Port the test, not the numbers, unless the design is frozen.
- `MainWindow.xaml.cs` lines 450–1590 (palette/config/injection/update handlers) were read for handler
  identity and behaviour; the injection progress state machine itself lives in `GameStateClient`
  (`aoko-platform`) and is not enumerated control-by-control here.
- `Aoko/Core/GameStateClient.cs` (1677 lines), `Aoko/Core/Clicker.cs` (3360 lines) and
  `Aoko/Core/Profile.cs` (977 lines) were read for the fields, defaults, payload keys and handler seams
  referenced above, not exhaustively line-by-line; per-property TCP payload ordering inside
  `BuildConfigPayload` is `unknown` from this pass and must be re-derived from the bridge
  `ParseConfig` when the Rust protocol layer is written.
