---
name: gui-media-showcase
description: >-
  Regenerate external GUI screenshots, smooth MP4 video tours, and animated GIFs
  using scripts\New-GuiScreenshots.ps1. Use when verifying GUI changes, modifying themes,
  refreshing documentation or README assets, or recording UI showcases.
---

# External GUI Media Showcase

Operational runbook for capturing canonical external GUI screenshots (`121_gui.png`),
high-framerate H.264 video tours (`gui.mp4`), and animated showcase GIFs (`gui.gif`)
using `scripts\New-GuiScreenshots.ps1`.

---

## 1. Quick Start

Run from repository root using PowerShell:

```powershell
# Full run: 121_gui.png, gui.mp4 (30fps), and gui.gif (15fps)
pwsh .\scripts\New-GuiScreenshots.ps1

# Include individual screenshots for each tab (gui-combat.png, gui-render.png, etc.)
pwsh .\scripts\New-GuiScreenshots.ps1 -IncludeTabs

# Video only (fast, skips GIF encoding)
pwsh .\scripts\New-GuiScreenshots.ps1 -NoGif

# Screenshots only (skips all video/GIF recordings)
pwsh .\scripts\New-GuiScreenshots.ps1 -NoVideo -NoGif
```

Outputs are automatically synchronized across:
- `screenshots/` (canonical repo root asset store)
- `website/public/screenshots/` (marketing site public static assets)
- `website/dist/screenshots/` (built website output, if present)

---

## 2. Script Parameters

| Parameter | Type | Default | Description |
|---|---|---|---|
| `-Theme` | string | `"Steel"` | Base GUI theme palette to apply (`Steel`, `Slate`, `Water`, `Graphite`, `Blend`, etc.) |
| `-Version` | string | `"1.21"` | Mock client version capabilities to emulate (`1.21`, `1.8.9`) |
| `-VideoFps` | int | `30` | Framerate for the MP4 video tour |
| `-GifFps` | int | `15` | Framerate for the animated GIF |
| `-GifWidth` | int | `850` | Output width for GIF in pixels (`0` preserves master 1006px resolution) |
| `-NoVideo` | switch | `false` | Skip MP4 video generation |
| `-NoGif` | switch | `false` | Skip GIF generation |
| `-IncludeTabs` | switch | `false` | Export individual PNG captures for every tab |
| `-OutDir` | string | `screenshots` | Destination folder for repo artifacts |
| `-WebsiteDir` | string | `website/public/screenshots` | Destination folder for website public assets |

---

## 3. Architecture & Execution Mechanics

### A. WPF STA Environment & Mock State
The script requires an STA (`Single-Threaded Apartment`) thread. If invoked in MTA, it
automatically relaunches itself via `pwsh -STA -File ...`.

1. Loads `Aoko.dll` in-process using Reflection.
2. Cancels background update checks (`_updateCheckCancellation.Cancel()`) to freeze clean status (`"Up to date"`, `"Current v0.11.4"`).
3. Injects mock `BridgeCapabilities` via `GameStateClient.Instance.Capabilities`.
4. Switches window into `EnterControlMode()`.
5. Seeds representative stats into `StatsTracker.Instance` (165 clicks, average ~14.1 CPS, peak ~16.3 CPS) to render an authentic CPS histogram bar chart.

### B. Frame Bounds & Offscreen Window Capture
WPF windows with custom chrome can suffer from drop-shadow and title bar frame mismatch.
The script uses Win32 `DwmGetWindowAttribute` with `DWMWA_EXTENDED_FRAME_BOUNDS` (9) to obtain
the exact rendered window rectangle (`1006 x 753`), and `PrintWindow` with `PW_RENDERFULLCONTENT` (2)
to capture crisp frames without desktop occlusion or cursor interference.

### C. High-Frequency Live Theme Cycling Across Tabs & Mid-Scroll
Rather than staying monochromatic or only changing on tab select, the recording
cycles distinct built-in themes twice as often (both upon tab entry and mid-scroll/mid-view),
showcasing the dynamic real-time theme engine in motion:
1. **Combat**: Starts in `Steel` (original dark charcoal `#08090C`, `#0F1218` panels, steel-blue `#6B8DAB` accent) -> switches mid-scroll to `Ink` (monochrome silver).
2. **Render**: Starts in `Slate` (coral `#C7625A` accent) -> switches mid-scroll to `Lush` (forest green / lime accent).
3. **Utility**: Starts in `Water` (luminous cyan `#0CE8C7` accent) -> switches mid-scroll to `Magic` (purple/indigo blue).
4. **Risky**: Starts in `Graphite` (champagne/bronze `#B89B82` accent) -> switches mid-scroll to `Coral` (peach/teal accent).
5. **Stats**: Starts in `Blend` (deep night, royal blue `#4794FD` accent) -> switches mid-view to `Digital Horizon` (cyber pink/cyan accent).
6. **Settings**: Starts in `Lime Water` (neon aqua `#12FFF7` / lime) -> switches mid-scroll to `Blossom` (lavender/teal) -> switches at palette cards to `Steel`.
7. **Loop back**: Seamless return to Combat tab in canonical `Steel`.

### D. Single-Pass Top-to-Bottom Scrolling
Each tab scrolls smoothly from top (`0`) to bottom (`ScrollableHeight`) with cubic easing
(`$t * $t * (3.0 - 2.0 * $t)`), holds the bottom view, and immediately transitions to the next tab.
Redundant reverse-scrolling back to the top of each tab is skipped to keep the video concise,
dynamic, and focused.

---

## 4. Safety & Presentation Rules

- **DevMode Must Be Disabled**: Dev-only modules (such as `KillAura`) must never appear in public screenshots, README showcases, or marketing video tours. The script enforces `$clicker.DevMode = $false`, `$clicker.DisableDevOnlyModules()`, and explicitly collapses `KillAuraCard.Visibility = Collapsed`.
- **No Simulated Cursor Artifacts**: Do not synthesize or draw artificial cursor overlays. Direct WPF UI events and programmatic scrolling produce cleaner, higher-signal captures.
- **Aspect Ratio & libx264 Even Dimensions**: The master window height is 753px. H.264 requires macroblock-divisible dimensions; ffmpeg must include `-vf "pad=ceil(iw/2)*2:ceil(ih/2)*2"` so the video renders at `1006 x 754` without encoder failure.
- **GIF Optimization**: To maintain reasonable file sizes (< 8 MB) for web embedding, the GIF pipeline applies Lanczos downscaling (`scale=850:-1`), 15 fps rate-limiting, and differential palette generation (`palettegen=max_colors=128:stats_mode=diff` with Bayer dither).

---

## 5. Verification Checklist

When updating GUI styles, layouts, or new modules:
1. Compile the C# project: `dotnet build Aoko\Aoko.csproj`.
2. Run test suite: `dotnet test Aoko.Tests\Aoko.Tests.csproj`.
3. Run `pwsh .\scripts\New-GuiScreenshots.ps1`.
4. Check that `screenshots\121_gui.png` displays correct title bar and Steel palette.
5. Check `screenshots\gui.mp4` with a media player or ffmpeg frame extraction:
   - Risky tab shows Reach / Velocity with no KillAura.
   - Stats tab shows the populated histogram.
   - Themes cycle across tabs and seamlessly return to Steel.
