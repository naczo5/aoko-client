# Phase 2 UI inventory: WPF surface → GPUI (`aoko-app`)

Inventory for the UI layer of the C#/WPF → Rust/GPUI migration. This document does not design the
Rust code; it enumerates what exists, what it is built out of, and where the pinned GPUI revision
does not give an equivalent.

Read `MIGRATION_ARCHITECTURE.md` §2 (crate map, line 34; placement rule, line 36), §6 (UI performance
rules, line 196), §7 (pinning) and §8 (arbitrations, line 218) before acting on anything here.
`GPUI_API_NOTES.md` is the API contract for the pinned revision; every GPUI claim below was
re-verified against the local checkout at
`~/.cargo/git/checkouts/zed-a70e2ad075855582/5526444/` and carries a `crates/...` path.

Editorial rules, matching `docs/rust/WIRE_CONTRACT.md`:

- Numbers are quoted from source, not estimated.
- `file:line` means I read that line in this pass.
- `unverified` means I did not confirm it.
- `needs verification` means a GPUI API would be required but I did not prove it exists at the pin.
  Do not build against a `needs verification` item without compiling a probe first.
- GPUI types are cited by their path inside the zed checkout, e.g. `gpui/src/window.rs:3268`.

---

## 1. Component inventory

Every distinct widget the app draws, its first real use, the WPF mechanism, and the GPUI equivalent.
"First real use" is the first place the widget is instantiated with real content, not where its
`Style`/`ControlTemplate` is declared; the style declaration is given in the mechanism column.

Instance counts, for sizing the component library (`grep -o` over `Aoko/MainWindow.xaml` and the
`Aoko/Views/` files): **50** `ControlCardStyle` cards, **72** `DarkSlider` sliders, **58**
`DarkCheckBox` checkboxes, **43** `ModuleSwitch` toggles, **54** `DarkButton` buttons, **13**
`DarkComboBox` dropdowns, **38** `KeybindButton_Click` keybind buttons, **8** `TabPrimaryButton`, plus
`PreviewCardButtonStyle`, `TabSecondaryButton`, `TabDangerButton` and the overlay's `EscChipStyle`.
That is ~340 call sites behind ~15 builder functions — which is why the split in §7 is worth having.

Four of these were re-derived during Phase 2 and the original figures were overcounts: cards 49 -> 50,
sliders 75 -> 72, switches 44 -> 43, dropdowns 14 -> 13. `ConfigComboBox` exists as a style key but is
referenced nowhere, and every one of the 13 `<ComboBox` openings carries `DarkComboBox`. The eight
`TabPrimaryButton` uses all live in `AccountsView.xaml` and `ServersView.xaml`, not the shell.

| # | Widget | First real use | WPF mechanism | GPUI equivalent | Risk |
| --- | --- | --- | --- | --- | --- |
| 1 | App shell, two-state (loader ↔ control panel) | `Aoko/MainWindow.xaml:306` (`LoaderPanel`), `:344` (`ControlPanel`) | Two siblings in one `Grid`, `Visibility` swapped in code (`MainWindow.xaml.cs:469-492`) | Two branches in `Render`. `Render::render` returns one concrete `impl IntoElement`, so both branches must produce the same type — `.into_any_element()` at the top level, or two root entities | medium |
| 2 | Tab bar (left rail) | `Aoko/MainWindow.xaml:186` (rail `Border`), `:298` (items host) | `TabControl` with `ControlCenterTabControlStyle` (`:176`) + `ItemContainerStyle` (`ControlCenterTabItemStyle`, `:66`). Rail 176 px + 10 px gap + content (`:181-183`); each item 150 wide, `MinHeight=50`, radius 10 (`:82-86`) | No `TabControl`. A `flex_col` of `.id(..)` divs, selected state in the entity, `hover()`/`group_hover()` for the wash. See §5.2 | medium |
| 3 | Rail search button | `Aoko/MainWindow.xaml:199` (`ModuleSearchButton`) | Inline `Button` `Style` with `ControlTemplate`, search glyph as `Path` geometry (`:242-244`), `ctrl k` hint | `div()` + an SVG asset for the glyph (`svg().path(..)`, `GPUI_API_NOTES.md` §7) + text. No `Path`-geometry element is documented at the pin — `needs verification`. Ghost hover is `hover()` | low |
| 4 | Update status button | `Aoko/MainWindow.xaml:265` (`UpdateStatusButton`), text at `:272`, `:278` | `Button` with two stacked `TextBlock`s, filled by `SetUpdateStatus` | Same shape; copy is dynamic, see §4 | low |
| 5 | Module card | `Aoko/MainWindow.xaml:356` (`AutoClickerCard`) | `Border` + `ControlCardStyle` (`:53`): `PanelBrush`, radius 6, padding 12, 1 px `SliderBgBrush` border, hover border → `SliderFgBrush` | `div()` with `bg`/`rounded`/`border_1`/`p_3` + `hover(\|style\| ..)` (`gpui/src/elements/div.rs:866`) for the border lift. No template needed | low |
| 6 | Module toggle switch (46×24, 18 px thumb) | `Aoko/MainWindow.xaml:360` | `CheckBox` + `ModuleSwitch` style (`Aoko/App.xaml:152`). Track 46×24 (`:164`), thumb `Ellipse` 18×18 (`:169`), thumb translate 0→22 over 0.2 s quadratic ease-out (`:183-186`), accent overlay opacity 0→1 (`:187-188`), thumb fill → `TextBrush` when checked (`:204`). Second, deliberately un-animated variant for overlay rows: `Aoko/Views/CommandSearchOverlay.xaml:46-89` | `div()` track + inner thumb div; animate with `Animation::new(Duration).with_easing(..)` (`gpui/src/elements/animation.rs:36,64`) + `AnimationExt::with_animation` (`:98`). Quadratic ease-out is a closure, not a named easing | medium |
| 7 | Small checkbox (14×14, `✓` glyph) | `Aoko/MainWindow.xaml:386` (`Click in Chests`) | `CheckBox` + `DarkCheckBox` (`Aoko/App.xaml:119`); border 14×14 radius 4 (`:127-130`), `✓` `TextBlock` at `:131` toggled by trigger | Hand-drawn div + text glyph. No `CheckBox` in `gpui` (`needs verification`: nothing beyond §4 of the API notes is documented) | low |
| 8 | Slider | `Aoko/MainWindow.xaml:368` (`Min CPS`), 75 instances across Combat/Render/Utility/Risky/Beta | `Slider` + `DarkSlider` (`Aoko/App.xaml:219`): 6 px track radius 3 (`:232`), 4 px end dot (`:234`), thumb 6×16 radius 3 with `DropShadowEffect` (`:265-273`), hover scale 1.4/1.15 (`:281-282`), drag scale 1.5/1.2 (`:298`), `IsMoveToPointEnabled` (`:223`), `IsSnapToTickEnabled` at call sites (`:381`) | No slider widget at this revision (no `Slider` type anywhere in `gpui/src`). Build on `on_mouse_down` (`gpui/src/elements/div.rs:126`), `on_mouse_move` (`:303`) and element bounds. Snap/round/coalesce must be ported by hand | high |
| 9 | Text field | first real use `Aoko/MainWindow.xaml:717` (`BlockEspAddBox`); shared style `Aoko/Views/TabStyles.xaml:155` (`TabTextBox`) used at `Aoko/Views/ServersView.xaml:215`, `:366`, `:373` | `TextBox` with `ControlTemplate`: caret and selection brushes (`TabStyles.xaml:161-163`), accent border on hover and `IsKeyboardFocusWithin` (`:182-189`). Ad-hoc unstyled variants: `MainWindow.xaml:717` (`BlockEspAddBox`), `:1952` (`NewPaletteNameBox`), `:1963` (`PaletteBgBox`); overlay `CommandSearchOverlay.xaml:215` (`QueryBox`, 18 px, transparent) | **No text-field widget at the pin.** The only seam is `EntityInputHandler` (`gpui/src/input.rs:26`). Every field is hand-built: shape `.id()` + `track_focus` (`div.rs:812`) + `EntityInputHandler` impl + drawn caret/selection | high |
| 10 | Combo / dropdown | `Aoko/MainWindow.xaml:504` (`Auto Rod` slot mode), also `:1063`, `:1138`, `:1166`, `:1228-1230` (Kill Aura, `SelectedValuePath=Tag`), `:1540` (`ConfigComboBox`), `Aoko/WindowPickerDialog.xaml:72` | `ComboBox` + `DarkComboBox` (`Aoko/App.xaml:24`): `ToggleButton` + chevron `Path` (`:48`), `Popup` with `ScrollViewer`, `MaxHeight=200` (`:70-87`), and a separate `ComboBoxItem` template (`:95-116`) with highlighted/selected row fills | No dropdown widget. Compose: trigger div + `anchored()` (`gpui/src/elements/anchored.rs:39`) with `.position(..)` (`:59`), or a `deferred()` layer (`gpui/src/elements/deferred.rs:7`). Selection, keyboard move, click-outside and scroll cap are all ours | high |
| 11 | Buttons — 5 shared variants + inline ones | `Aoko/App.xaml:329` (`DarkButton`) first used at `MainWindow.xaml:318` (`InjectButton`) | `DarkButton`: radius 8, hover accent overlay opacity 0→1 over 0.15 s, press opacity (`App.xaml:337-360`). `PreviewCardButtonStyle` (`MainWindow.xaml:21`) for palette/style previews. `TabPrimaryButton` / `TabSecondaryButton` / `TabDangerButton` (`Aoko/Views/TabStyles.xaml:29`, `:68`, `:114`) with defined rest/hover/pressed/disabled opacities. `EscChipStyle` (`CommandSearchOverlay.xaml:22`). A per-instance inline style at `MainWindow.xaml:207-236` | One component with a `Variant` enum; hover/pressed via `hover()` and `on_mouse_down`/`on_mouse_up` | low |
| 12 | Tooltip | `Aoko/MainWindow.xaml:205` (`Search modules and tabs (Ctrl+K)`); 55 occurrences in `MainWindow.xaml`, 10 in `Aoko/Views/AccountsView.xaml`, 6 in `ServersView.xaml` | `ToolTip` attached property; `ToolTipService.ShowOnDisabled` is set explicitly so a disabled action still explains itself (`TabStyles.xaml:37`, `:76`, `:122`; `AccountsView.xaml:498`) | `.tooltip(\|window, cx\| AnyView)` (`gpui/src/elements/div.rs:722`) and `.tooltip_show_delay(Duration)` (`:759`). GPUI has no disabled state, so "tooltip on a refused action" must keep the element interactive and gate the click instead | medium |
| 13 | Context menu | **does not exist** — zero `ContextMenu` in `Aoko/**/*.xaml` or `*.cs` | none used; destructive actions are explicit buttons (`Delete`, `✕` at `MainWindow.xaml:699`, `:1920`) | Nothing to port. A menu would be a new dependency (`context_menu`/`popover_menu` live in zed's `ui` crate, not in `gpui`) | none |
| 14 | List / scroll container | `Aoko/MainWindow.xaml:354` (first tab `ScrollViewer`); list-shaped: `Aoko/Views/AccountsView.xaml:381` (`CardsControl`), `Aoko/Views/ServersView.xaml:240` (`ServersList`) | `ScrollViewer` per tab; `ListBox` for accounts (`AccountsView.xaml:76-95` template, `:97` `DataTemplate`); `ItemsControl` + `ScrollViewer` for servers (`ServersView.xaml:238-330`). Global 4 px scrollbar with 2 px track and 4 px accent thumb (`Aoko/App.xaml:390-439`) | `overflow_y_scroll()` requires `.id()` first (`GPUI_API_NOTES.md` §5). Rows: `uniform_list(id, len, ..)` (`gpui/src/elements/uniform_list.rs:22`). No scrollbar styling API in the notes — `needs verification` | medium |
| 15 | Image / bitmap | **none in the UI** | No `<Image>`, no PNG/ICO/DrawingImage in any XAML (grep over `Aoko/**/*.xaml`). The only image asset is `logo.ico` as `ApplicationIcon` (`Aoko/Aoko.csproj:8`), i.e. window/taskbar icon. In-UI glyphs are `Path` geometry (`App.xaml:48`, `MainWindow.xaml:242`) and text glyphs (`✓` `App.xaml:131`, `✕` `MainWindow.xaml:699`) | `img()`/`svg()` exist (`GPUI_API_NOTES.md` §7) but there is nothing to port. Window icon is a platform concern, `needs verification` for Linux | low |
| 16 | Overlay (command search) | `Aoko/MainWindow.xaml:2107` (`CommandSearchHost`, `Margin="-20"`); body in `Aoko/Views/CommandSearchOverlay.xaml` | Sibling `UserControl` over the `TabControl`, `Visibility=Collapsed`; scrim `LinearGradientBrush` 2 stops (`:117-124`); 760 px radial "moon haze" (`:132-140`); card `CardMinWidth=340`/`CardMaxWidth=620` (`CommandSearchOverlay.xaml.cs:24-25`); 48 px rows (`:269`); results `MaxHeight=300` (`:250-251`); background blur via one cached `BlurEffect` radius 12 on `ControlPanel` (`MainWindow.xaml.cs:83-88`, applied `:1679`) | `deferred()` (`gpui/src/elements/deferred.rs:7`) for z-order. **No backdrop blur and no radial gradient** — see §5.4 and §5.5. This is the single hardest widget in the app | high |
| 17 | Chip (status / motd / source) | `Aoko/Views/ServersView.xaml:292` (latency chip button), `Aoko/Views/AccountsView.xaml:142` (`SourceChip`) | `Border` + `TabChip`/`TabChipTint`/`TabChipText` (`TabStyles.xaml:199`, `:207`, `:213`); row-level `ServerChipButton` (`ServersView.xaml:73`) with `Tint` + `HoverWash` layers | `div()` + `rounded` + `px`/`py` + text. Trivial | low |
| 18 | Progress bar (determinate + indeterminate) | `Aoko/MainWindow.xaml:333` (`InjectionProgressBar`, 0–100); `Aoko/Views/AccountsView.xaml:429` (`BusyBar`, indeterminate, 80×3); `Aoko/Views/ServersView.xaml:405` with a custom `Storyboard` at `:135-160` | `ProgressBar`; the servers one re-templated with a looping `DoubleAnimation` | No widget. A filled `div()` whose width is a fraction; indeterminate = `Animation::new(..).repeat()` (`gpui/src/elements/animation.rs:48`) | low |
| 19 | Expander | `Aoko/MainWindow.xaml:1226` (`Combat and targeting`), `:1240`, `:1250`, `:1261`, `:1278` | `Expander` with `IsExpanded`; local `Style TargetType="Expander"` at `:1212` | A `bool` on the entity + conditional children. Good fit for GPUI | low |
| 20 | Colour swatch / click-to-pick | `Aoko/MainWindow.xaml:702` (`BlockEsp` row swatch, 22×14), `:1961` (`PaletteBgPreview`, 22×22) | `Border` with `Background` from `HexColorToBrushConverter` (`MainWindow.xaml:17`) or a bound brush; `MouseLeftButtonUp`/`MouseLeftButtonDown` opens `ColorPickerDialog.TryPick` | `div()` with `bg()` + `on_click`, then a modal entity. Fine | medium |
| 21 | Colour picker dialog | `Aoko/ColorPickerDialog.xaml:23` (`SvCanvas` 220×180) | `Canvas` + stacked `Rectangle`s: hue layer (`:26`), white→transparent and black overlays (`:27-42`), thumb `Ellipse` (`:43`); hue bar 22×180 (`:52-68`); `HexBox` (`:83`); R/G/B `Slider`s (`:94`, `:99`, `:104`); 360 wide, `SizeToContent=Height` | **Canvas semantics and radial/3-stop gradients are absent.** The SV square is built from layered gradients; GPUI offers 2-stop linear only (§5.5). Plan for a custom-paint element or a row-based approximation, and treat this dialog as a separate mini-project | high |
| 22 | Bar chart (CPS histogram) | `Aoko/MainWindow.xaml:1480` (`CpsBuckets` `ItemsControl`), template `:1486-1499` | `ItemsControl` with horizontal `StackPanel` panel (`:1483`), 18 px wide bars, 124 px row, `BarHeight` binding | `flex_row` of divs with computed heights. Straightforward, the data half already exists | low |
| 23 | Highlighted text runs (search match) | `Aoko/Views/CommandSearchOverlay.xaml:327` (`RowName`, `SourceText`/`MatchRanges`) | Custom `TextBlock` subclass (`Aoko/Views/HighlightTextBlock.cs`) that rebuilds `Inlines` and ignores out-of-range ranges | `StyledText::with_highlights(Vec<(Range<usize>, HighlightStyle)>)` (`gpui/src/elements/text.rs:409`) with `HighlightStyle { color, background_color, .. }` (`gpui/src/style.rs:580-600`). Near 1:1 — the only trap is that GPUI's ranges are byte offsets and must be char-boundary safe (same discipline as the C# control) | low |
| 24 | Selection rule + hover wash + card pulse | `Aoko/Views/TabStyles.xaml:220` (`TabCardWash`), `:226` (`TabSelectionRule`); pulse `Aoko/MainWindow.xaml.cs:1824` (`CardPulseAdorner`) | Layered `Border`s inside the item template, plus an `Adorner` painting a 1.2 s accent ring around the card (keyframes `MainWindow.xaml.cs:1797-1811`, gate `:1785`) | Layered divs; pulse = `with_animation` on a border overlay keyed by module id. No animation-preference API in GPUI — read the preference ourselves (§5.3) | medium |
| 25 | Modal dialog window | `Aoko/WindowPickerDialog.xaml:40` (`WindowList`), `Aoko/ColorPickerDialog.xaml:5` (360 wide) | Separate `Window` with `Owner`, `WindowStartupLocation`, `IsDefault`/`IsCancel` (`ColorPickerDialog.xaml:109-110`) | A second `cx.open_window` is possible but there is no modal/owner relationship in the notes — `needs verification`. Preferred: in-window overlay layer, see §5.11 | medium |
| 26 | Window chrome (custom title bar / DWM) | `Aoko/MainWindow.xaml:8-12` (initial 360×310, `SingleBorderWindow`, `NoResize`), `MainWindow.xaml.cs:494-517` (`ApplyNativeTitleBarTheme`) | `WindowStyle`/`ResizeMode` changed at runtime (`:474-483`); DWM attributes 20/34/35/36 set to `PanelColor`/`TextColor` (`:500-513`) | `WindowOptions::window_decorations` (`gpui/src/platform.rs:871`) + `request_decorations` (`gpui/src/window.rs:2850`), `TitlebarOptions { appears_transparent }` (macOS/Windows only per the API notes). **No DWM title-bar colouring equivalent.** Min size is creation-time only (§5.12) | medium |

Count: **26 widgets**. Of those:

- **6 have no usable GPUI equivalent and must be built from primitives:** slider (8), text field (9),
  dropdown (10), the overlay's glass treatment (16), the colour-picker canvas (21), and the modal
  window relationship (25).
- **3 are partial:** tooltip-on-disabled, because GPUI has no disabled state (12); scrollbar styling,
  which is not documented at the pin (14); window chrome, where DWM caption/border/text colouring has
  no equivalent (26).
- Everything else maps onto `div` + `hover`/`when` + `with_animation`.
- **Highest risk (5):** 8, 9, 10, 16, 21. Row 26 is the highest *non-obvious* risk, because it also
  constrains what a screenshot can show (§6).

### Correction: the "340 width" module card

The brief describes a module card with an arbitrated 340 width. That width does not exist on module
cards. The only `340` in the UI source is the **command-search card's minimum width**
(`Aoko/Views/CommandSearchOverlay.xaml.cs:25`, `CardMinWidth = 340`, with `CardMaxWidth = 620` at
`:24`), which is also the reason for the window width floor:

> `// Floor below which the command search card and its footer legend would clip: the card is 340 wide`
> `// plus the overlay's 56 px gutters, and 460 tall fits the header, three rows and the legend.`
> — `Aoko/MainWindow.xaml.cs:479-480`

`MinWidth = 560`, `MinHeight = 460` (`:481-482`). Note the comment and the code disagree: the gutter
constant is `CardEdgeGutter = 72` (`CommandSearchOverlay.xaml.cs:30`), not 56, and the card is sized as
`clamp(340, available − 2 × 72, 620)` (`CommandSearchOverlay.xaml.cs:177`). So an un-clipped card needs
340 + 144 = 484 px, and the `Margin="-20"` (`MainWindow.xaml:2107`) gives the overlay 40 px more than
the window — making 560 a comfortable, not a derived, floor. Do not carry the "56" into Rust; carry
`CardMinWidth 340`, `CardMaxWidth 620`, `CardEdgeGutter 72`, `CardTopRatio 0.30`, `ResultsMaxHeight 300`
(`CommandSearchOverlay.xaml.cs:24-31`).

Module cards in tab bodies have no explicit width at all; they stretch inside a 12 px `StackPanel`
margin (`MainWindow.xaml:355`). The only cards with a fixed width are the three Stats summary cards at
`Width="150"` (`MainWindow.xaml:1442`, `:1448`, `:1457`).

Consequence for Phase 2: if the 340 figure is treated as a card width, the shell will be laid out
wrong. It belongs to the overlay and to the window's minimum size.

---

## 2. The nine tabs

Tab order is fixed by `Aoko/MainWindow.xaml`; `TabControl` is `TabStripPlacement="Left"` (`:346`).
`DataContext` for tabs 1–5 and 9 is `Clicker.Instance`; tab 8 overrides it with
`StatsTracker.Instance` (`:1438`); tabs 6 and 7 are self-contained `UserControl`s.

| # | Tab | XAML region | Hosted modules / cards | Static or state-driven |
| --- | --- | --- | --- | --- |
| 1 | Combat | `MainWindow.xaml:353-593` | `AutoClickerCard` `:356`, `RightClickCard` `:392`, `AimAssistCard` `:416`, `TriggerbotCard` `:450`, `AutoRodCard` `:494`, `ThrowpotCard` `:530`, `AutoHealCard` `:547`, `HitDelayFixCard` `:578` | state-driven: every control binds `Clicker` (`IsArmed`, `MinCPS`, `AutoRodSlotMode`, …); labels are literals |
| 2 | Render | `MainWindow.xaml:595-801` | `NametagsCard` `:598`, `NickHiderCard` `:620`, `FightStatusCard` `:636`, `ChestEspCard` `:647`, `BlockEspCard` `:663`, `BedPlatesCard` `:726`, `FireballWarningCard` `:745`, `ClosestPlayerCard` `:761`, unnamed module-list card `~:770`, `HudEditorCard` `:778`, `AntiDebuffCard` `:786` | state-driven; also host to the only item collection inside a card: `BlockEspTargets` (`:693`) with per-row enable/colour/remove |
| 3 | Utility | `MainWindow.xaml:803-1022` | `SpeedBridgeCard` `:806`, unnamed Discord RPC card `~:850`, `GtbHelperCard` `:851`, `PixelPartyAssistCard` `:861`, `ChestStealerCard` `:896`, `RefillCard` `:936`, `AutoToolCard` `:960`, `PanicCard` `:1002` | state-driven; `PanicCard` holds the only "act now" button (`:1013`, `Trigger Panic Now`) |
| 4 | Risky | `MainWindow.xaml:1024-1192` | `ReachCard` `:1027`, `VelocityCard` `:1087`, `AutoTotemCard` `:1126` | state-driven; contains the only `DataTrigger`-driven warning text (`:1178-1181`, on `AutoTotemBehaviorMode == 1`) |
| 5 | Beta | `MainWindow.xaml:1194-1428` | banner card `:1196`, `KillAuraCard` `:1205`, `JumpResetCard` `:1290`, `HitSelectCard` `:1310`, `WTapCard` `:1323`, `TrajectoriesCard` `:1336`, `MurderMysteryCard` `:1349`, `BreakProgressCard` `:1362`, `NoHurtCamCard` `:1375`, `TntTimerCard` `:1388`, `FastPlaceCard` `:1401`, `SprintCard` `:1414` | state-driven, plus **visibility**-driven: `KillAuraCard` is `Visibility="{Binding DevMode, Converter=BooleanToVisibilityConverter}"` (`:1205-1206`). `KillAuraCard` alone expands to five `Expander`s and ~60 controls (`:1226-1286`) |
| 6 | Accounts | `MainWindow.xaml:1430-1433` → `Aoko/Views/AccountsView.xaml` (515 lines) | import (file/paste), offline-add, session strip, account card list, footer actions, Lunar mirror toggle | state-driven and **async**: guarded by `RunExclusiveAsync` (`AccountsView.xaml.cs:1030`), busy bar, drag-drop import (`:807-819`) |
| 7 | Servers | `MainWindow.xaml:1434-1437` → `Aoko/Views/ServersView.xaml` (457 lines) | direct join row, saved list with latency chip, add form, ping-all / join-selected / delete footer | state-driven and **async**; single-click select, double-click join (`ServersView.xaml.cs:382`), keyboard selection (`:371`) |
| 8 | Stats | `MainWindow.xaml:1438-1521` | three 150-wide summary cards (`:1442`, `:1448`, `:1457`), `Reset Stats` (`:1466`), CPS histogram card (`:1472-1514`) with "No samples yet" empty state (`:1501`) | state-driven from `StatsTracker`; the histogram is a pure projection of `CpsBuckets` |
| 9 | Settings | `MainWindow.xaml:1523-2104` | Configs card (`ConfigComboBox` `:1540`), Reload Mappings card, GUI Customization card: 18 palette preview buttons (`:1589-1901`), custom palette cards (`:1909-1948`), four hex boxes + swatches (`:1950-2004`), 7 module-list-style previews, `ShowLogo`/`DevMode` switches | mixed: 18 built-in preview swatches carry **hardcoded hex in XAML** (e.g. `#BB377D`/`#FBD3E9` at `:1856`, `:1857`); custom cards bind brushes; the rest binds `Clicker` |

Largest panel by far is Settings; the heaviest single module is Kill Aura on Beta. Both belong to the
shell builder, not the component builder.

---

## 3. Palette and theme

### Normative table: `MainWindow.GuiPalettes`

`MainWindow.xaml.cs:29-41` declares 10 colour roles; `MainWindow.xaml.cs:137-373` defines 18 palettes.
Per `MIGRATION_ARCHITECTURE.md:218` (arbitration 1) this table is normative and the Rust `Theme` has
10 roles.

**One of the ten is dead.** `AccentSecondary` (`MainWindow.xaml.cs:35`) is populated for all 18
built-in palettes and is read by nothing: no `palette.AccentSecondary` read exists in `Aoko/` or
`Aoko.Tests/`, no XAML resource key is fed from it, and the persisted user type
`Aoko/Core/CustomPalette.cs:14-25` has only 9 colour fields. A custom palette leaves it `null`
(`MainWindow.xaml.cs:1019-1032` sets every role except that one). So the live role count is 9, and
`ApplyGuiTheme` never touches it. Keep the 10th field in the Rust struct to satisfy arbitration 1 and
the JSON shape, mark it inert, and never wire it.

Column order below is Background, Panel, SliderBackground, SliderForeground, Accent, AccentSecondary
(inert), Text, DimText, TabSelected, TabHover. All values are 6-digit RGB `#RRGGBB` as written with
`ColorConverter.ConvertFromString` (`MainWindow.xaml.cs:140-149` onwards).

| Palette | Background | Panel | SliderBg | SliderFg | Accent | AccentSecondary | Text | DimText | TabSelected | TabHover |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Slate | `#0A0B0F` | `#12141A` | `#181B22` | `#2A2F38` | `#C7625A` | `#C7625A` | `#E8EAEE` | `#7A8290` | `#1F2229` | `#181B22` |
| Ink | `#08090B` | `#101115` | `#16181C` | `#262830` | `#B0B6C0` | `#B0B6C0` | `#E8EAEE` | `#7A828F` | `#1D1F23` | `#16181C` |
| Graphite | `#0B0B0D` | `#131316` | `#19191C` | `#2A2A2D` | `#B89B82` | `#B89B82` | `#E8E8EA` | `#82827E` | `#222226` | `#19191C` |
| Steel | `#08090C` | `#0F1218` | `#161A21` | `#262C35` | `#6B8DAB` | `#6B8DAB` | `#E5E8EE` | `#7286A0` | `#1F232C` | `#161A21` |
| Blend | `#070B14` | `#0D1424` | `#131E33` | `#1C2B49` | `#4794FD` | `#47FDA0` | `#EDF3F7` | `#7B94B5` | `#16233B` | `#10192A` |
| Lush | `#080D09` | `#0F1710` | `#17241A` | `#223325` | `#A8E063` | `#56AB2F` | `#EEF5EE` | `#7A9C7D` | `#1B291D` | `#131D15` |
| Water | `#060B12` | `#0B1624` | `#102136` | `#18304D` | `#0CE8C7` | `#0CA3E8` | `#EAF7F8` | `#709CB8` | `#12253D` | `#0E1D30` |
| Lime Water | `#060E0E` | `#0B1B1B` | `#112828` | `#1A3A3A` | `#12FFF7` | `#B3FFAB` | `#F0FAF9` | `#70A5A0` | `#142E2E` | `#0E2222` |
| Digital Horizon | `#090A14` | `#121324` | `#1B1D33` | `#262947` | `#5FC3E4` | `#E55D87` | `#F3EDF5` | `#8E7FA8` | `#1E213D` | `#15172C` |
| Coral | `#0A0B0F` | `#14151C` | `#1E202B` | `#2D3040` | `#F4A896` | `#348597` | `#F7EDEB` | `#A08785` | `#222430` | `#191B24` |
| Magic | `#060714` | `#0D0F24` | `#151838` | `#202554` | `#7F9EFF` | `#8E2DE2` | `#EDF1FF` | `#808EC7` | `#171B3D` | `#10132B` |
| Blossom | `#0B0812` | `#161121` | `#221B30` | `#302742` | `#E2D0F9` | `#317773` | `#F7F2FD` | `#9E8CAE` | `#251D36` | `#1A1426` |
| Pastel | `#0D0A10` | `#18131E` | `#241D2B` | `#332A3D` | `#F39BB2` | `#CFC4F3` | `#FBF3F6` | `#A8909D` | `#292131` | `#1C1722` |
| Sunkist | `#0E0B07` | `#1A150D` | `#262014` | `#382F1E` | `#F2C94C` | `#F2994A` | `#FFF9ED` | `#A89675` | `#2E2517` | `#1E1810` |
| Nord | `#0B0E12` | `#12181F` | `#1B242E` | `#263340` | `#8FBCBB` | `#A3BE8C` | `#ECEFF4` | `#7B889B` | `#1E2833` | `#151C24` |
| Cherry | `#0F080C` | `#1C1018` | `#2B1925` | `#3D2434` | `#BB377D` | `#FBD3E9` | `#FCF2F7` | `#A87C93` | `#301D2A` | `#21141D` |
| Aubergine | `#0E060D` | `#1A0C18` | `#281325` | `#3B1D37` | `#AA076B` | `#61045F` | `#FAEBF7` | `#9E6E95` | `#2F162B` | `#200F1E` |
| Snowy Sky | `#060C10` | `#0C1820` | `#132430` | `#1D3445` | `#01ABB3` | `#12E8E8` | `#EAF8FA` | `#75A2AC` | `#152836` | `#0E1B24` |

`Slate` is the fallback: unknown or blank names normalise to it (`MainWindow.xaml.cs:1308-1311`,
`:1293-1306`).

### What `ApplyGuiTheme` actually writes

`MainWindow.xaml.cs:1307-1356`. Seven `Color` keys and twenty-one brush writes across two resource
scopes — application and this window. Both scopes must be written, because XAML `DynamicResource`
resolves the nearest scope first and the tab brushes exist only on the window.

| Scope | Key | Value | Line |
| --- | --- | --- | --- |
| App | `BgColor` `PanelColor` `SliderBgColor` `SliderFgColor` `AccentColor` `TextColor` `DimTextColor` | the 7 roles, as `Color` | `:1314-1320` |
| App | `BgBrush` `PanelBrush` `SliderBgBrush` `SliderFgBrush` `AccentBrush` `TextBrush` `DimTextBrush` | the same 7 roles, as `SolidColorBrush` | `:1322-1328` |
| App | `CommandSearchGlassBrush` | `Panel` at alpha `0xE4` | `:1331` |
| App | `CommandSearchMatchBrush` | `Blend(Text, Accent, 0.28)` | `:1336` |
| App | `CommandSearchMatchPillBrush` | `Accent` at alpha `0x2E` | `:1337` |
| App | `CommandSearchPlaceholderBrush` | `Blend(DimText, Text, 0.35)` | `:1338` |
| App | `CommandSearchSubtleTextBrush` | `Blend(DimText, Text, 0.55)` | `:1339` |
| Window | `BgBrush` `PanelBrush` `SliderBgBrush` `SliderFgBrush` `AccentBrush` `TextBrush` `DimTextBrush` | the 7 roles again | `:1341-1347` |
| Window | `ControlCenterTabSelectedBrush` | `TabSelected` | `:1348` |
| Window | `ControlCenterTabHoverBrush` | `TabHover` | `:1349` |

Two more palette consumers outside the resource keys: `ApplyNativeTitleBarTheme()` (`:1352`, body
`:494-517`, DWM attributes 20/34/35/36 from `PanelColor` and `TextColor`), and
`UpdateGameStateUI()` (`:1353`).

`Blend(a, b, t)` is a per-channel linear interpolation with `Math.Round` (`MainWindow.xaml.cs:1268-1273`);
`Lighten(color, amount)` moves each channel toward 255 by `amount` clamped to 0..1 (`:1262-1265`). Both
are needed to derive a custom palette's missing roles (`:1011-1016`): `SliderBg = Lighten(panel, 0.06)`,
`SliderFg = Lighten(panel, 0.16)`, `DimText = Blend(text, bg, 0.45)`,
`TabSelected = Lighten(panel, 0.10)`, `TabHover = Lighten(panel, 0.04)`.

### Roles with no obvious GPUI analogue

| Role | Problem | Recommendation |
| --- | --- | --- |
| `SliderForeground` | In WPF it is a *brush used as a border and as a solid shape* (`App.xaml:170` thumb fill, `:129` checkbox border). GPUI has one `border_color` and one `bg`, so the model maps — but the app also uses it as a 6 px slider fill for the *unfilled* portion, which is a widget concern, not a token | Keep as a token; no issue |
| `TabSelected` / `TabHover` | Only consumed by two WPF keys (`ControlCenterTabSelectedBrush`, `ControlCenterTabHoverBrush`). Nothing in GPUI needs a resource key — the tab component reads the token directly | Keep; delete the keys, not the roles |
| `AccentSecondary` | Dead (above). No consumer, no analogue, no persistence | Keep the field, mark inert, never read |
| *(derived)* `CommandSearchMatchBrush` | `Blend(Text, Accent, 0.28)` is a computed tint, not a stored role. `PaletteContrastTests` asserts AA on it for every palette including a light custom one | Compute in Rust from the 10 roles; do not store |
| *(derived)* `CommandSearchGlassBrush` | Alpha `0xE4` over `Panel`, and its whole purpose is showing blurred content through — which GPUI cannot do (§5.4) | Keep the token; expect the visual to differ unless §5.4 is solved |

Also flag: 121 hardcoded colour literals in `MainWindow.xaml` (18 palette preview swatches × ~6 plus
`#40FFFFFF`, `#20000000`, `#16000000`, `#FFA500` at `:1173`). The 18 built-in previews duplicate the
palette table by hand. In Rust these must be generated from the theme table, or the two will drift.

---

## 4. Strings and copy

The brief states copy lives in `TabCopy`, `AccountsViewLogic`, `ServersViewLogic` and `StoreRecovery`
and not in the XAML. That is true for the two data tabs' *decisions and refusals*, and false for
everything else. Measured counts (method: `grep -o` for `Text=`, `Content=`, `ToolTip=` and `Header=`
whose value does not begin with `{`, per file):

| File | Literal user-visible strings | What they are |
| --- | --- | --- |
| `Aoko/MainWindow.xaml` | **478** | module names, every slider label, every checkbox label, all 55 tooltips, tab headers, Beta banner, Stats captions, Settings section headings, palette names |
| `Aoko/MainWindow.xaml.cs` | ~399 distinct literals ≥3 chars, ~90 of them sentences | `ModuleTitles` (`:95-134`), keybind button content, injection/update status, palette and config status, `NormalizeModuleListStyle` labels (`:1275-1291`) |
| `Aoko/Views/AccountsView.xaml` | 38 | buttons, empty state, the unencrypted-storage caveat (`:508`) |
| `Aoko/Views/ServersView.xaml` | 22 | buttons, empty state |
| `Aoko/Views/CommandSearchOverlay.xaml` | 13 | placeholder, `esc`, footer legend, `UNAVAILABLE` |
| `Aoko/ColorPickerDialog.xaml` | 10 | channel labels, Cancel/OK |
| `Aoko/WindowPickerDialog.xaml` | 12 | version choices, buttons |

Owning symbols for the copy that *is* centralised:

| Group | Owner | Volume |
| --- | --- | --- |
| Shared refusals and hints for Accounts + Servers | `Aoko/Views/TabCopy.cs:20-31` — `SelectAccountHint`, `SelectServerHint`, `NoServersHint`, `UnsupportedAccountSwitch`, `UnsupportedServerJoin` | 5 sentences, ~200 chars |
| Bridge-worded copy, derived not copied | `TabCopy.NotConnected` / `TabCopy.ReturnToMainMenu` (`:36`, `:39`), both built from `BridgeCommandResult.Failure(..)` | 2 derived strings; the strings themselves live in `aoko-protocol` |
| Status truncation rule | `TabCopy.Shorten` + `StatusLimit = 110` (`:33`, `:49-62`), cuts at a word boundary and appends `…` | 1 rule, 1 glyph |
| Tone → brush | `TabCopy.BrushKey(StatusTone)` (`:41-46`) | 3 mappings |
| Accounts decisions and labels | `Aoko/Views/AccountsViewLogic.cs` — `InvalidOfflineName` (`:31`), `SessionCaption` (`:92-97`), `SourceChipLabel` (`:99-110`, 5 labels: Cracked/Localts/Token/Cookie/Saved), `PrimaryHint` (`:82-90`), `SessionSwitchRefusal` (`:72-80`), `LastUsedLabel` (`:133-134`) | ~10 strings |
| Servers decisions and labels | `Aoko/Views/ServersViewLogic.cs` — `NotPingedLabel` / `PingingLabel` (`:32`, `:35`), `EmptySelectionMessage` (`:209`), `InvalidAddressChipLabel` (`:211`), `PingLabel`/`PingTone` (`:324-340`), `ErrorLabel` (`:345-369`, 5 outcome labels), `LastJoinedLabel` (`:296`), `JoinStartedText` (`:298`), `PingSummaryText` (`:309`), `MotdText` (`:312-321`), `AddedText`/`DeletedText` (`:371`, `:373`) | ~16 strings + 3 format templates |
| Corrupt-store notices | `Aoko/Views/StoreRecovery.cs` — `Message(path)` (`:43-55`), `PersistenceBlocked` wording (`:130`), `AccountsFileName`/`ServersFileName` (`:19-20`) | 2 sentences + 2 file names |
| Relative timestamps | `Aoko/Views/RelativeTime.cs:12-26` — `just now`, `Nm ago`, `Nh ago`, `Nd ago`, `Nmo ago`, `Ny ago` | 6 formats |
| Per-module availability text | `MainWindow.UpdateVersionAvailabilityUi`, exposed through `*AvailabilityText` blocks; the search overlay's `UnavailableReason` lives in `ModuleSearchIndex.cs` per the parity ledger | ~40 generated strings |

Porting rule for the builder: copy strings **verbatim**, including the ellipsis character in
`Pinging…`, the en dash in `Use 3–16 letters, numbers or underscores.`, and the two-line Beta banner
(`MainWindow.xaml:1199-1200`). `Aoko.Tests/TabCopyTests.cs`, `AccountsViewTests.cs`,
`ServersViewTests.cs` and `StoreRecoveryTests.cs` assert wording, so a paraphrase fails a test rather
than a screenshot.

Practical consequence: the copy surface is **~1000 strings**, most of it inline in
`MainWindow.xaml`. Whoever ports the shell owns the bulk of it; that is not a task that can be
pushed into a shared "strings" module up front without moving 478 literals out of XAML by hand.

---

## 5. WPF features with no direct GPUI analogue

Fourteen items. Each says what WPF gave for free, what the pinned revision does or does not have
(verified against the checkout), and the concrete choice I recommend.

### 5.1 Data binding and `INotifyPropertyChanged` auto-refresh

- **WPF gave:** `{Binding Path}` on `Clicker`/`StatsTracker`; a `PropertyChanged` raise repaints only
  the bound target. 303 `{Binding` occurrences in `MainWindow.xaml` alone (`grep -o` count).
- **GPUI has:** nothing. State lives in entities; `cx.notify()` marks an entity dirty
  (`GPUI_API_NOTES.md` §3); `cx.observe(&other, ..)` gives cross-entity updates with a `Subscription`
  cancelled on drop. `MIGRATION_ARCHITECTURE.md:196` forbids per-frame cloning or scoped notify
  beyond the entities whose fields changed.
- **Recommendation:** split the god-object. `Clicker` is 3360 lines of settings + engine; replicate
  that as a handful of `Entity`s grouped by tab (combat, render, utility, risky, beta, settings,
  stats, accounts, servers), and have each card observe only its group. Design the settings entity to
  expose a per-field version counter so a slider drag notifies one card, not the tab.

### 5.2 Style triggers, `ControlTemplate`, `VisualStateManager`

- **WPF gave:** hover/checked/focus visuals declared once in a `ControlTemplate`; retemplating a
  control kind globally (`DarkSlider` at `App.xaml:219` restyles all 75 sliders).
- **GPUI has:** no templates and no visual states. Per-element closures: `hover(|StyleRefinement| ..)`
  (`gpui/src/elements/div.rs:866`), `group`/`group_hover` (`:797`, `:876`), and `.when(cond, ..)`
  (`gpui/src/util.rs:21`).
- **Recommendation:** a `components/` module of plain builder functions (`switch(is_on), slider(..),
  field(..), button(variant, label)`) that take tokens. That is the direct replacement for
  retemplating, and it is what makes the two-builder split work (§7).

### 5.3 Storyboard animations

- **WPF gave:** declarative `Storyboard` per trigger, with easing, delays, and `RepeatBehavior`;
  globally disabled by `SystemParameters.ClientAreaAnimation`, which the app checks in four places
  (`CommandSearchOverlay.xaml.cs:98`, `:147`, `:210`, `MainWindow.xaml.cs:1785`).
- **GPUI has:** `Animation::new(Duration)` with `.repeat()` (`gpui/src/elements/animation.rs:36-48`),
  `.with_easing(fn(f32) -> f32)` (`:64`), `.with_max_fps(..)` (`:73`), and
  `AnimationExt::with_animation(id, ..)` / `with_animations(..)` / `with_spring(..)` (`:98`, `:116`,
  `:138`). One animation per element id; timings live in code.
- **Recommendation:** a `theme::motion` module holding the durations the C# uses (0.12, 0.15, 0.18,
  0.2, 0.22 s from the templates, plus the card pulse's 160 ms / 820 ms hold / 1200 ms keyframe run at
  `MainWindow.xaml.cs:1797-1810`) and the four easings (`QuadraticEase EaseOut` at `App.xaml:185`,
  `CubicEase EaseOut` at `MainWindow.xaml.cs:1800`, `BackEase amplitude 0.6` at `MainWindow.xaml:143`,
  linear). Note the keyframe shape: `with_animation` supplies progress 0→1, so the pulse's
  rise–hold–fall has to be produced by clamping `t` inside the closure. **Animation-preference
  gating does not exist in GPUI — `needs verification`.** Either read the OS preference ourselves
  (Windows: `SPI_GETCLIENTAREAANIMATION`; Linux: `gtk-enable-animations` / reduce-motion portals) or
  accept that animations always run. Decide before the component library is written, because the gate
  is threaded through every animated widget.

### 5.4 Drop shadows, blur, and the glass overlay

- **WPF gave:** `DropShadowEffect` with separate `BlurRadius`, `ShadowDepth`, `Direction`, `Opacity`
  (`MainWindow.xaml:95`, `CommandSearchOverlay.xaml:157-160`), and one `BlurEffect` applied to the
  whole tab control to make the overlay read as glass over a blurred GUI (`MainWindow.xaml.cs:83-88`,
  `:1679`).
- **GPUI has:** `box_shadow` on `Style` (`gpui/src/style.rs:292`) with a `BoxShadow` type (`:349`)
  whose `blur_radius` (`:355`) is *shadow* blur, plus generated `box_shadow_style_methods` on
  `Styled` (`gpui/src/styled.rs:34`). Shadows: yes. **Backdrop blur: no.** `Window::blur`
  (`gpui/src/window.rs:2785`) defocuses the window, it does not blur content. There is no
  `backdrop-filter`, no scene blur, no offscreen-composite API in the notes or in `style.rs`.
- **Recommendation:** keep `box_shadow` for card/popup shadows (the 18-colour preview cards and the
  search card both need it) and **drop the blurred-GUI idea**. Compensate with what the source already
  has: `CommandSearchGlassBrush` is `Panel` at 89 % alpha (nearly opaque) and the scrim is a two-stop
  vertical dark gradient (`CommandSearchOverlay.xaml:117-124`), which GPUI can do. The visual
  difference will be a slightly less "frosted" overlay. Do not attempt a manual blur pass this phase;
  it would mean a render-to-texture path GPUI does not expose in the notes.

### 5.5 Gradients, `OpacityMask`, and the colour picker

- **WPF gave:** arbitrary `LinearGradientBrush`/`RadialGradientBrush` stop counts,
  `Border.OpacityMask` with a gradient (`CommandSearchOverlay.xaml:179-190`), and `Canvas` with
  absolute positioning plus stacked layers for the HSV square (`ColorPickerDialog.xaml:23-50`).
- **GPUI has:** `Background` with tags `Solid`, `LinearGradient`, `PatternSlash`, `Checkerboard`
  (`gpui/src/color.rs:745-750`) and the constructor
  `linear_gradient(angle: f32, from: impl Into<LinearColorStop>, to: impl Into<LinearColorStop>)`
  (`color.rs:865`) — **exactly two colour stops**. `linear_color_stop(color, percentage)` exists
  (`:893`). No radial gradient, no conic gradient, no `< 2` or `> 2` stops, and no opacity mask.
- **Recommendation:**
  - Scrim (2 stops) ports directly.
  - The 760 px radial "moon haze" (`CommandSearchOverlay.xaml:132-140`, 3 stops) has no equivalent.
    Replace with either a flat translucent disc, or omit it. Do not fake a radial gradient with
    concentric divs; it will band.
  - The 4-stop accent hairline with a gradient `OpacityMask` (`:179-190`) becomes a 2-stop linear
    gradient in which the outer stops are pre-multiplied to alpha 0 — that reproduces the visible
    effect because the mask is a horizontal fade to transparent at both ends.
  - The colour picker's SV square **cannot** be built from 2-stop linear gradients plus a white and a
    black overlay, because the overlays themselves need alpha ramps and the composite needs a
    `multiply`-like blend. Recommend custom paint: an `Element` that fills a quad per row (or per
    cell) in `prepaint`/`paint`. `needs verification` for how much of `paint` is public at this rev —
    verify before committing to the dialog.

### 5.6 `ContextMenu`

- **WPF gave:** nothing here — there is no `ContextMenu` in the app.
- **GPUI has:** no in-window context-menu widget. `Menu` at `gpui/src/platform/app_menu.rs:4` is the
  OS application menu bar. Popover/context menus live in zed's `ui` crate
  (`crates/ui/src/components/context_menu.rs`), which is not a workspace dependency and drags in
  `component`, `theme`, `settings`, `icons`, `ui_macros`, `menu`, `gpui_util` (`crates/ui/Cargo.toml:15-32`).
- **Recommendation:** nothing to port, and do not add the dependency for it. If a right-click menu is
  ever wanted, build it on `anchored()` (§5.7).

### 5.7 `ComboBox`, popups, and the anchored layer

- **WPF gave:** `Popup` with `Placement=Bottom`, `AllowsTransparency`, `PopupAnimation=Slide`,
  auto-sized to the target, `MaxHeight` + scroll (`App.xaml:70-87`).
- **GPUI has:** `anchored()` returning `Anchored` with `.position(Point<Pixels>)`
  (`gpui/src/elements/anchored.rs:39`, `:59`) and `deferred(child)` for paint-above-siblings ordering
  (`gpui/src/elements/deferred.rs:7`). Dismiss-on-outside-click exists as
  `on_mouse_down_out` / `on_mouse_up_out` (`gpui/src/elements/div.rs:263`, `:283`).
- **Recommendation:** one `components::popup` used by every dropdown, by the colour-picker trigger and
  by any future menu. Compute the position from the trigger's bounds; there is no automatic placement
  flip at the pin (`needs verification`), so clamp to the window rect yourself — the same discipline
  the overlay already uses for `CardEdgeGutter` (`CommandSearchOverlay.xaml.cs:28-30`).

### 5.8 `DataGrid`

- **WPF gave:** nothing here — the app has no `DataGrid`. Its list-shaped surfaces are
  `ListBox` (accounts, `AccountsView.xaml:381`), `ItemsControl` (servers,
  `ServersView.xaml:240`; Block ESP targets, `MainWindow.xaml:693`; palettes, `:1909`; CPS bars,
  `:1480`).
- **GPUI has:** `uniform_list(id, len, |range, window, cx| Vec<AnyElement>)`
  (`gpui/src/elements/uniform_list.rs:22`), plus `.into_any_element()` for mixed rows.
- **Recommendation:** `uniform_list` for accounts and servers (both are uniform fixed-height rows:
  account cards via a 2-row `Grid` at `AccountsView.xaml:99-160`, server rows `MinHeight="64"` at
  `ServersView.xaml:247`). Plain `flex_col` for the small collections (Block ESP targets, palette
  cards, CPS bars) — virtualization there would cost more than it saves.

### 5.9 Drag and drop (file import)

- **WPF gave:** `AllowDrop="True"` (`AccountsView.xaml:9`) plus `View_DragEnter/DragOver/DragLeave/Drop`
  (`AccountsView.xaml.cs:807-819`), a visible dashed drop target (`AccountsView.xaml:404-422`), and
  `DataFormats.FileDrop` extraction.
- **GPUI has:** `on_drag`/`on_drop`/`drag_over`/`on_drag_move` (`gpui/src/elements/div.rs:639`, `:592`,
  `:1238`, `:1072`) and `ExternalPaths(SmallVec<[PathBuf; 2]>)` for OS file drops
  (`gpui/src/interactive.rs:715`), with `FileDropEvent` referenced from the public surface
  (`gpui/src/window.rs:8534`).
- **Recommendation:** `on_drop::<ExternalPaths>` + `drag_over` for the highlight. The C# hint state is
  one `bool` (`SetDropHint`, `AccountsView.xaml.cs:807`); keep it that way.

### 5.10 Clipboard

- **WPF gave:** `Clipboard.GetText()` / `ContainsText()` for the paste panel and copy affordances.
- **GPUI has:** `App::read_from_clipboard() -> Option<ClipboardItem>`
  (`gpui/src/app.rs:1988`), `read_from_clipboard_async(..)` (`:1999`),
  `write_to_clipboard(item)` (`:2022`).
- **Recommendation:** use the async read for the Accounts paste panel, and keep the parsed-token
  failure path free of cookie values (that rule is a test: `CookieParserClaimTests.cs`, and
  `TabCopyTests`). Mark: reading the clipboard on Linux may be empty under Wayland without a
  clipboard manager — `unverified`, treat "empty paste" as a normal state.

### 5.11 DPI, scaling, and pixel snapping

- **WPF gave:** device-independent units plus `SnapsToDevicePixels` and `UseLayoutRounding` on the
  overlay (`CommandSearchOverlay.xaml:8-9`), `DpiX`-aware rendering, and per-monitor DPI changes
  handled by the framework.
- **GPUI has:** logical `px`/`rem` units throughout, `Window::scale_factor() -> f32`
  (`gpui/src/window.rs:3506`) for the current factor, and
  `HeadlessAppContext::simulate_window_scale_factor_change` (`gpui/src/app/headless_app_context.rs:134`)
  in tests. No `SnapsToDevicePixels` equivalent.
- **Recommendation:** never write sub-pixel sizes for the 1 px hairlines (`SliderBgBrush` borders,
  the 1 px divider at `CommandSearchOverlay.xaml:244`, the 2 px rail at `App.xaml:399`); GPUI rounds
  at paint time and a 0.5 px border can vanish at 100 % or thicken at 150 %. Fix a 1 px token and a
  separate 1.5 px token for high-DPI, or accept uniform 1 px. Validate at 1.0, 1.25 and 2.0 with the
  scale-factor hook in the capture harness (§6).

### 5.12 Keyboard focus, tab order, and keybind capture

- **WPF gave:** `KeyboardNavigation.TabIndex` (`MainWindow.xaml:266`, `ServersView.xaml:240`),
  `TabNavigation="Once"` / `DirectionalNavigation` (`AccountsView.xaml:81-82`,
  `ServersView.xaml:241-242`), `Focusable`/`IsTabStop` (overlay rows are explicitly **not** tab stops,
  `CommandSearchOverlay.xaml:266-268`), `IsKeyboardFocusWithin` for accent borders
  (`TabStyles.xaml:186`), and a window-wide `PreviewKeyDown`/`PreviewTextInput` for the type-to-open
  search (`MainWindow.xaml:13-14`, handlers `MainWindow.xaml.cs:1623`, `:1641`).
- **GPUI has:** `FocusHandle` with `.track_focus(&handle)` (`gpui/src/elements/div.rs:812`) and
  `FocusHandle::tab_stop(bool)` to opt a handle out of the order (`gpui/src/window.rs:597`, inside
  `impl FocusHandle` at `:555`); `Window::focus_next()`/`focus_prev()`
  (`gpui/src/window.rs:2807`, `:2818`); and `on_key_down`, whose closure receives `&mut App` — so
  mutating the view from a key handler needs an `Entity` handle, per `GPUI_API_NOTES.md` §6. The
  tab-stop bookkeeping itself (`gpui/src/tab_stop.rs:13`) is `pub(crate)`, so traversal is only
  reachable through the two `focus_*` calls.
- **Recommendation:** the keybind-capture button (`MainWindow.xaml:359` and its 37 siblings) becomes
  `Entity<Self>` + `track_focus` + `on_key_down` + `cx.update`, storing the pending module id in the
  entity instead of `_pendingKeybindModuleId` (`MainWindow.xaml.cs:74`). For the overlay's
  "not a tab stop, announce selection" rule (`AutomationProperties` at
  `CommandSearchOverlay.xaml:267-268`), use `aria_selected` (§5.13) and simply do not call
  `track_focus` on rows.

### 5.13 Accessibility

- **WPF gave:** `AutomationProperties.Name` / `.HelpText` / `.ItemStatus` / `.LiveSetting`
  (`MainWindow.xaml:206`, `AccountsView.xaml:491`, `ServersView.xaml:251-252`,
  `CommandSearchOverlay.xaml:227-228`, `:267-268`, `TabStyles.xaml:251`), and `AutomationProperties.LiveSetting=Polite`
  for status text that must be announced.
- **GPUI has:** accesskit through `role(accesskit::Role)`, `aria_label`, `aria_description`,
  `aria_keyshortcuts`, `aria_active_descendant`, `aria_selected`, `aria_expanded`, `aria_toggled`,
  `aria_numeric_value`, `aria_numeric_value_step` (`gpui/src/elements/div.rs:1357`, `:1379`, `:1388`,
  `:1398`, `:1418`, `:1441`, `:1447`, `:1453`, `:1459`, `:1466`). Debug support lives in
  `gpui/src/window/a11y/debug.rs` (the module is `pub(crate)`, `gpui/src/window.rs:71`).
- **Recommendation:** this is a near 1:1 port and cheaper than expected — annotate every custom
  widget with `role()` + `aria_label()`. Live-region announcements for status lines have **no
  analogue found** (`needs verification`): `aria_live` is not in the list above. If announcements
  matter, plan an explicit "nudge" call into accesskit or accept the gap.

### 5.14 Window chrome, custom title bar, min size, and the shell's two window modes

- **WPF gave:** `WindowStyle="SingleBorderWindow"`, `ResizeMode="NoResize"` at 360×310, then at
  runtime `Width=1020 Height=760 MinWidth=560 MinHeight=460 ResizeMode=CanResizeWithGrip`
  (`MainWindow.xaml.cs:469-484`), plus DWM colouring of the caption/border/text
  (`:494-517`, attributes 20/34/35/36 constants at `:65-68`).
- **GPUI has:** `WindowOptions::window_decorations` / `window_min_size` (`gpui/src/platform.rs:2769`,
  `:2844`), `Window::request_decorations()` (`gpui/src/window.rs:2850`), `Window::resize(Size)`
  (`:3296`), `WindowDecorations::{Server, Client}` (`gpui/src/platform.rs:871`), and
  `TitlebarOptions { title, appears_transparent, traffic_light_position }` which hides the system
  titlebar **on macOS and Windows only** per `GPUI_API_NOTES.md` §2. `window_min_size` is a
  **creation-time field only** — no runtime setter exists in `window.rs` or `platform.rs`.
- **Recommendation:** do not keep the two-mode window. Open the single window at the control-mode
  geometry with `window_min_size = 560×460`, and make the loader/control switch a `Visibility` swap
  inside one view (which is what the C# does anyway, `MainWindow.xaml:306`/`:344`). That removes the
  runtime min-size and resize-mode changes, both of which GPUI cannot do.
  For chrome: decide between (a) `WindowDecorations::Server` and lose the palette-tinted caption, or
  (b) `WindowDecorations::Client` and draw the titlebar ourselves — note that the headless capture
  path cannot render client chrome (§6). DWM tinting has no equivalent in either case.

---

## 6. Screenshot capture harness

### What the C# did

There is no in-app capture code. The visual-verification story is a Windows PowerShell script,
`scripts/New-GuiScreenshots.ps1` (also the `gui-media-showcase` skill), which:

1. Loads the built `Aoko.dll` into a PowerShell process and constructs `Aoko.App` and `Aoko.MainWindow`
   directly (`:122-133`). **This is why the capture story works at all: it drives the real view tree
   in-process, with no game and no bridge.**
2. Fabricates state by reflection: `GameStateClient.Capabilities = forVersionFallback($Version)`,
   `IsConnected = true`, `IsInjected = true`, `InjectedVersion = $Version` (`:142-158`), then seeds
   `Clicker` settings (`:160-172`) and 165 `StatsTracker.RecordClick` calls (`:174-186`).
3. Calls the private `EnterControlMode()` and `ApplyGuiTheme($Theme)` by reflection (`:186-190`),
   default theme **Steel** (`:58`).
4. Pumps the dispatcher until settled (`Pump-Events`, `:205-218`).
5. Captures with `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)` into a `System.Drawing.Bitmap` sized
   from `DwmGetWindowAttribute(hwnd, 9, …)`, where `9` is `DWMWA_EXTENDED_FRAME_BOUNDS` (`:223-240`).
6. Selects tabs by index and captures each (`ControlPanel.SelectedIndex = i`, `:265-284`).
7. For video, scrubs `ScrollToVerticalOffset` in eased steps and records frames, switching theme
   mid-scroll to exercise the palette, then hands frames to `ffmpeg` (`:330-444`).

Committed artifacts: `screenshots/121_gui.png` (1006×753), `gui.mp4`, `gui.gif`
(`ls screenshots/`). The 1006×753 measured size for a 1020×760 window is the extended-frame-bounds
crop, i.e. the client area including a ~7 px non-client edge per side.

**Two defects to not inherit:**

- The per-tab name array `@("combat","render","utility","risky","beta","stats","settings")`
  (`:273`) has 7 entries against 9 tabs. `SelectedIndex` 5 and 6 are **Accounts** and **Servers**, so
  `gui-stats.png` holds the Accounts tab and `gui-settings.png` holds the Servers tab; Stats (7) and
  Settings (8) are never captured by `-IncludeTabs`.
- The video tour has the same offset: `# --- TAB 5: Stats` sets `SelectedIndex = 5` (`:414-416`) and
  `# --- TAB 6: Settings` sets 6 (`:422-424`) with content already visible at 5. Accounts and Servers are
  absent from the tour, and Stats/Settings are mislabelled.

### What the Rust harness must do to be comparable

`MIGRATION_ARCHITECTURE.md:247` requires `cargo run -p aoko-app -- --capture-screenshots <out_dir>`.
Minimum for a fair comparison:

1. **Same fabricated state, same order.** Capability set = the `ForVersionFallback` equivalent for a
   chosen version (default `1.21`), `connected`/`injected` true, `DevMode` **off**,
   `KillAuraCard` hidden, the same Clicker defaults the script sets (`MinCPS 12`, `MaxCPS 16`,
   `RandomizationModeIndex 2`, `AimAssist` on at FOV 45/range 4.5/strength 40, `SpeedBridge` on at
   150 ms) and the same 120 + 45 synthetic clicks. Without this the images are not comparable at all.
2. **Theme Steel, then the same tour themes** (Ink, Slate→Lush, Water→Magic, Graphite→Coral,
   Ink→Steel, Blend→Digital Horizon, Lime Water→Blossom→Steel per `:349-437`).
3. **1020×760 content size**, which is the size the C# control mode uses (`MainWindow.xaml.cs:474-475`).
   A real GPUI window has already booted at 1260×820 in this environment, so 1020×760 is achievable
   live; pick whichever path below you adopt.
4. **One PNG per tab, named after the tab, with the nine-tab order** — and assert the count is 9 so
   the offset bug cannot recur.
5. **Deterministic time**, because the switch/tab animations are 0.12–0.22 s and the pulse is 1.2 s.

### Verified capture API at the pin

- `Window::render_to_image(&self) -> anyhow::Result<image::RgbaImage>`, gated
  `#[cfg(any(test, feature = "test-support"))]` (`gpui/src/window.rs:3267-3268`). Renders the current
  frame's scene to a texture without presenting it.
- `HeadlessAppContext` (`gpui/src/app/headless_app_context.rs`, exported at `gpui/src/app.rs:33`):
  `with_platform(text_system, asset_source, renderer_factory)` (`:65`),
  `open_window(&mut self, size: Size<Pixels>, build_root) -> Result<WindowHandle<V>>` (`:104`),
  `run_until_parked()` (`:129`), `simulate_window_scale_factor_change(..)` (`:134`),
  `advance_clock(Duration)` (`:143`),
  `capture_screenshot(&mut self, window: AnyWindowHandle) -> Result<RgbaImage>` (`:177`).
- `gpui_platform::current_headless_renderer() -> anyhow::Result<Option<Box<dyn PlatformHeadlessRenderer>>>`
  (`crates/gpui_platform/src/gpui_platform.rs:102`); `gpui_platform::application()` at `:16`,
  `headless()` at `:20`.
- The `test-support` feature pulls `proptest`, `postage`, `futures-concurrency`, `web-time`
  (`crates/gpui/Cargo.toml:21-31`) — production bloat if enabled unconditionally.

Three consequences the plan must absorb:

1. **`test-support` gating.** `render_to_image` needs the feature on `gpui`. Put the harness behind an
   `aoko-app` feature (e.g. `capture`) that enables `gpui/test-support` and `gpui_platform/test-support`,
   so the shipped binary stays lean. That is a `Cargo.toml` decision that must be made once, by one
   builder, before either writes capture code.
2. **`HeadlessAppContext::open_window` takes a size, not `WindowOptions`** — it hardcodes
   `focus: false, show: false, ..Default::default()` (`:114-122`). So the headless path cannot request
   `window_decorations: Client`. Client-drawn chrome (§5.14) will be missing from headless captures.
   Either accept captures without the titlebar (and document it), or capture a live window through
   `render_to_image` on a real `Window`.
3. **`image` crate version.** `capture_screenshot` returns `image::RgbaImage`; `image 0.25.10` is
   already in `Cargo.lock:2433-2435`, pulled transitively by gpui. Declare
   `image = "0.25"` (with the `png` feature) so feature unification keeps one version, otherwise the
   returned type is not the type you can save. Verify at first build.

### Environment note

Lavapipe Vulkan plus Xvfb are known to work here and a real GPUI window has already booted at
1260×820 (`GPUI_API_NOTES.md` §10 records both the `xvfb-run` + `VK_ICD_FILENAMES=.../lvp_icd.json`
recipe and the `LD_LIBRARY_PATH` trap that breaks Vulkan with `GLIBCXX_3.4.32 not found`,
§11 item 8). So the live-window path is the low-risk fallback if the headless renderer misbehaves; the
headless path is still preferred for determinism.

---

## 7. Recommended split of work into two builders

Proposed: Builder 4 = theme, assets, component library, dialogs, capture harness; Builder 5 = shell,
tabs, views, wiring.

**Verdict: right split, with two corrections.** The dependency graph really is
theme → components → cards → tab bodies → shell, and the first two layers are dense, unglamorous and
independent of `aoko-core`. The corrections:

1. **Dialogs should go to Builder 5, not Builder 4.** Both dialogs are product surfaces with their own
   state machines (HSV coupling in `ColorPickerDialog.xaml.cs`, target/version selection in
   `WindowPickerDialog.xaml.cs`), and both are *invoked from* the shell: Block ESP rows
   (`MainWindow.xaml:705`), palette swatches (`:1962`, `:1972`), injection (`MainWindow.xaml.cs`
   `InjectButton_Click`). Keeping them with Builder 4 means Builder 4 must also own the Block ESP
   collection UI and the Settings palette editor, which is shell work. Alternatively, if Builder 4
   keeps the *colour picker widget*, Builder 5 owns the dialog host, the OK/Cancel flow and the
   wiring.
2. **The capture harness cannot be finished by Builder 4 alone.** It must screenshot Builder 5's
   views at nine tab positions with fabricated state. Builder 4 should own the harness *mechanism*
   (headless context, clock, scale factor, PNG encode, ffmpeg-free stills) and a `Vec<Screen>` registry
   with **one** placeholder screen; Builder 5 fills the registry with the nine tabs and the state
   recipe from §6. Do not let Builder 4 hardcode tab names: that is exactly how the C#
   index-offset bug survived.

### Shared boundaries where the two collide

| Boundary | Why it collides | Owner |
| --- | --- | --- |
| `aoko-app/src/main.rs` | One `main`. Both want window options; Builder 5 wants to mount the shell, Builder 4 wants to mount the capture path and parse `--capture-screenshots` | **Builder 4** writes `main.rs` once, with an explicit `Shell` root type and an argument parse, and Builder 5 only implements the root view |
| `theme/` tokens | Both consume; only one may define | Builder 4. Builder 5 must never write a hex literal — the shell alone has 121 (`MainWindow.xaml`), and that dual source is already a defect |
| `components/` | Builder 5 will want to "just tweak" a switch or slider inline | Builder 4. Changes arrive as component API requests; Builder 5 must not patch components in passing |
| Copy strings | ~1000 strings, 478 of them inline in `MainWindow.xaml` (§4) | **Builder 5** owns card/tab copy (it is where the labels live); **Builder 4** owns component-internal copy (overlay placeholder, `esc`, dialog labels) and imports `TabCopy` equivalents from `aoko-core`/`aoko-protocol` |
| Animation preference gate (§5.3) | Threaded through every animated widget, and neither knows it needs deciding | Decide before either starts; Builder 4 implements the gate in `theme::motion`, Builder 5 calls it |
| `Cargo.toml` / workspace deps | Workspace file says only the workspace owner edits it (`Cargo.toml:39-41`); capture needs `image` + a feature flag | **Builder 4** lands one preparatory commit adding `image`, the `capture` feature and `gpui/test-support` wiring |
| Overlay layer | Builder 4 draws the overlay component; Builder 5 owns its open/close state, Ctrl+K, row actions and the pulse on jump (`MainWindow.xaml.cs:1623-1824`) | Split by interface: Builder 4 = `CommandSearchOverlay` view + row rendering; Builder 5 = `CommandSearchState` entity |

### What Builder 4 must hand Builder 5 to unblock parallel work

A single commit, early, containing:

1. `theme/`: the 10-role `Theme` struct (with `AccentSecondary` inert), all 18 palettes, `Blend`,
   `Lighten`, the derived command-search tints, and a `Theme::by_name(&str) -> Theme` with Slate
   fallback.
2. `theme::motion`: the durations, the four easings, and the animation-preference gate.
3. `components/`: `button(variant)`, `card`, `switch(is_on)`, `checkbox`, `slider(..)`, `field(..)`,
   `combo(..)`, `chip(tone)`, `progress(..)`, `tooltip` wrapper, `popup/anchored`, `highlight_text`,
   `scroll(id)`, `expander`. Signatures plus working visuals; behaviour may be minimal.
4. `assets/`: `AssetSource` impl and the font install (`GPUI_API_NOTES.md` §8-9). **Fonts are not
   neutral here:** the two data tabs ask for `Segoe UI` (`AccountsView.xaml:5`, `ServersView.xaml:5`)
   and monospace chips ask for `Consolas` (three sites), both Windows-only. Pick the families now —
   UI plus monospace — and record them, because font choice changes every measurement and the
   C# screenshots are a Segoe UI baseline.
5. `main.rs` + `Shell` root type stub + `--capture-screenshots` argument parse.
6. The capture harness mechanism + registry type.

Then Builder 5 can build tabs against `components::*` immediately and register screens; Builder 4 can
finish the overlay, the colour picker and the scale-factor/determinism work without touching tab code.

---

## 8. Open questions a human must decide

Only questions the source cannot settle. Six.

1. **Does `aoko-app` get to depend on zed's `ui` / `text` crates?** Neither is a workspace dependency
   today (`crates/aoko-app/Cargo.toml` depends only on `gpui` + `gpui_platform`). They would give ready
   `Tooltip`, `popover_menu`, `data_table`, `tab_bar`, `list` (`crates/ui/src/components/`), but they
   pull `component`, `theme`, `settings`, `icons`, `ui_macros`, `menu`, `gpui_util`, `schemars`, `strum`
   (`crates/ui/Cargo.toml:15-32`) — a second theme system beside ours, which is the exact dual-source
   problem arbitration 1 exists to prevent. Also note there is **no slider widget** and **no text-field
   widget** anywhere in the pinned tree (`struct Slider` appears in neither `crates/ui` nor
   `crates/text`; the only `TextInput` is the platform IME event type), so the two hardest components
   are hand-built either way. My recommendation: bare `gpui` only.

2. **Is the frosted-glass overlay a required visual, or is "opaque card over a dark scrim" acceptable?**
   GPUI cannot blur content behind an element (§5.4). The C# deliberately blurs the whole tab control
   (`MainWindow.xaml.cs:1679`) and the card is a near-opaque panel over it. If the blur is product
   identity, Phase 2 needs a render-to-texture path that the pin does not expose; if it is not, we
   ship the scrim version.

3. **Do animations stay, and does the OS animation preference still gate them?** GPUI has no
   reduce-motion hook (§5.3), and the C# gates four animation sites on
   `SystemParameters.ClientAreaAnimation`. Either we read the preference ourselves per platform, or the
   animation setting becomes a client setting, or animations always run.

4. **One window or two?** The C# opens a 360×310 loader and mutates itself into a 1020×760 control
   window at runtime (`MainWindow.xaml.cs:469-484`). GPUI's min size is creation-time only (§5.14), so
   the faithful-two-mode design forces a reopen (flicker, lost position) or a single creation-time
   geometry. My recommendation: one window at control geometry with the loader as an in-view state —
   confirm the loader-only first-run experience is acceptable at 1020×760.

5. **Is the custom title bar in scope?** DWM tinting of the caption has no GPUI equivalent. Options:
   `Server` decorations (OS-drawn, unthemed, consistent with the screenshot being cropped to client
   area anyway), or `Client` decorations (themed, must be drawn and hit-tested by us, and never appears
   in headless captures). This changes what "comparable screenshot" means and must be settled before
   the capture harness is written.

6. **Font.** `MainWindow.xaml` never sets a family and inherits the system UI font, but
   `AccountsView.xaml:5` and `ServersView.xaml:5` explicitly request `FontFamily="Segoe UI"`, and
   monospace chips use `Consolas` (`CommandSearchOverlay.xaml:240`, `:369`;
   `MainWindow.xaml:2072`). Both are Windows-only families, so the two tabs will silently reflow on
   Linux. GPUI requires an explicit installed family (`TextSystem::add_fonts`, `GPUI_API_NOTES.md` §8),
   and a silent fallback
   is invisible in code and obvious in a screenshot. Pick the family (and whether the in-game overlay's
   font must match) before any layout metrics are frozen.
