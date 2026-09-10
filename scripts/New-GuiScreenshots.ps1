<#
    .SYNOPSIS
        Regenerates external GUI screenshots, video showcase (MP4), and showcase GIF for aoko client.

    .DESCRIPTION
        Launches the Aoko WPF GUI in a controlled mock environment, applies the specified
        visual theme, puts the interface into control mode (1.21 capabilities), and captures:
          - 121_gui.png (canonical external GUI screenshot)
          - gui.mp4 (smooth H.264 video tour covering every tab top to bottom + theme showcase)
          - gui.gif (compact animated showcase GIF for README and markdown previews)
          - gui-<tab>.png (optional individual screenshots for every tab with -IncludeTabs)

        Outputs are automatically synchronized to:
          - screenshots/
          - website/public/screenshots/
          - website/dist/screenshots/ (if present)

    .PARAMETER OutDir
        Directory where primary screenshot artifacts are saved. Defaults to <repoRoot>\screenshots.

    .PARAMETER WebsiteDir
        Directory where website public screenshot assets are saved. Defaults to <repoRoot>\website\public\screenshots.

    .PARAMETER Theme
        GUI theme palette to apply. Defaults to "Steel" (matching original client palette).

    .PARAMETER Version
        Injected version capabilities to emulate. Defaults to "1.21".

    .PARAMETER VideoFps
        Framerate for the MP4 video. Defaults to 30.

    .PARAMETER GifFps
        Framerate for the animated GIF. Defaults to 15.

    .PARAMETER GifWidth
        Width in pixels for the animated GIF (0 to keep full resolution). Defaults to 850.

    .PARAMETER NoVideo
        Switch to skip video generation.

    .PARAMETER NoGif
        Switch to skip animated GIF generation.

    .PARAMETER IncludeTabs
        Switch to also save individual PNGs for each tab (gui-combat.png, gui-render.png, etc.).

    .EXAMPLE
        pwsh .\scripts\New-GuiScreenshots.ps1

    .EXAMPLE
        pwsh .\scripts\New-GuiScreenshots.ps1 -IncludeTabs
#>
[CmdletBinding()]
param(
    [string]$OutDir,
    [string]$WebsiteDir,
    [string]$Theme = "Steel",
    [string]$Version = "1.21",
    [int]$VideoFps = 30,
    [int]$GifFps = 15,
    [int]$GifWidth = 850,
    [switch]$NoVideo,
    [switch]$NoGif,
    [switch]$IncludeTabs
)

$ErrorActionPreference = "Stop"

# Ensure script runs on an STA (Single-Threaded Apartment) thread required by WPF.
if ([System.Threading.Thread]::CurrentThread.GetApartmentState() -ne [System.Threading.ApartmentState]::STA) {
    Write-Host "Relaunching in STA mode..." -ForegroundColor Cyan
    & pwsh -STA -File $PSCommandPath @args
    exit $LASTEXITCODE
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $OutDir) { $OutDir = Join-Path $repoRoot 'screenshots' }
else { $OutDir = [System.IO.Path]::GetFullPath($OutDir) }

if (-not $WebsiteDir) { $WebsiteDir = Join-Path $repoRoot 'website\public\screenshots' }
else { $WebsiteDir = [System.IO.Path]::GetFullPath($WebsiteDir) }

$DistDir = Join-Path $repoRoot 'website\dist\screenshots'

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
if (-not (Test-Path $WebsiteDir)) { New-Item -ItemType Directory -Path $WebsiteDir -Force | Out-Null }

# Build Aoko if dll is missing
$dllPath = Join-Path $repoRoot "Aoko\bin\Debug\net8.0-windows\Aoko.dll"
if (-not (Test-Path $dllPath)) {
    Write-Host "Building Aoko (Debug)..." -ForegroundColor Cyan
    & dotnet build (Join-Path $repoRoot "Aoko\Aoko.csproj") -c Debug
    if ($LASTEXITCODE -ne 0) { throw "Build failed." }
}

# Win32 Native Helpers for DWM bounds and PrintWindow
$win32Source = @"
using System;
using System.Runtime.InteropServices;

public class NativeGuiCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hwnd, int dwAttribute, out RECT pvAttribute, int cbAttribute);

    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdcBlt, uint nFlags);
}
"@
if (-not ([System.Management.Automation.PSTypeName]'NativeGuiCapture').Type) {
    Add-Type -TypeDefinition $win32Source
}

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName PresentationFramework
Add-Type -AssemblyName PresentationCore
Add-Type -AssemblyName WindowsBase
Add-Type -Path $dllPath

Write-Host ("Initializing Aoko GUI in mock mode (Version: {0}, Theme: {1})..." -f $Version, $Theme) -ForegroundColor Cyan

$app = [Aoko.App]::new()
$app.InitializeComponent()

$win = [Aoko.MainWindow]::new()

# Cancel background update check so it cannot overwrite our status text
$ctsField = [Aoko.MainWindow].GetField("_updateCheckCancellation", [System.Reflection.BindingFlags]"NonPublic,Instance")
if ($ctsField) {
    $cts = $ctsField.GetValue($win)
    if ($cts) { $cts.Cancel() }
}

# Configure mock GameStateClient capabilities & connection
$propCaps = [Aoko.Core.GameStateClient].GetProperty("Capabilities", [System.Reflection.BindingFlags]"Public,NonPublic,Instance")
$caps = [Aoko.Core.BridgeCapabilities]::ForVersionFallback($Version)
$propCaps.SetValue([Aoko.Core.GameStateClient]::Instance, $caps)

$connProp = [Aoko.Core.GameStateClient].GetProperty("IsConnected", [System.Reflection.BindingFlags]"Public,NonPublic,Instance")
$connProp.SetValue([Aoko.Core.GameStateClient]::Instance, $true)

$injProp = [Aoko.Core.GameStateClient].GetProperty("IsInjected", [System.Reflection.BindingFlags]"Public,NonPublic,Instance")
$injProp.SetValue([Aoko.Core.GameStateClient]::Instance, $true)

$verProp = [Aoko.Core.GameStateClient].GetProperty("InjectedVersion", [System.Reflection.BindingFlags]"Public,NonPublic,Instance")
$verProp.SetValue([Aoko.Core.GameStateClient]::Instance, $Version)

# Configure representative module settings (DevMode is OFF: KillAura hidden)
$clicker = [Aoko.Core.Clicker]::Instance
$clicker.DevMode = $false
$clicker.DisableDevOnlyModules()
if (-not $clicker.IsArmed) { $clicker.ToggleArmed() }
$clicker.MinCPS = 12
$clicker.MaxCPS = 16
$clicker.RandomizationModeIndex = 2
$clicker.RightClickEnabled = $false
$clicker.RightMinCPS = 13
$clicker.RightMaxCPS = 16
$clicker.AimAssistEnabled = $true
$clicker.AimAssistFov = 45
$clicker.AimAssistRange = 4.5
$clicker.AimAssistStrength = 40
$clicker.SpeedBridgeEnabled = $true
$clicker.SpeedBridgeDelayMs = 150

# Seed representative stats for Stats tab showcase
$stats = [Aoko.Core.StatsTracker]::Instance
for ($i = 0; $i -lt 120; $i++) {
    $cps = 13.5 + ([Math]::Sin($i * 0.2) * 2.0) + (([double]($i % 5)) * 0.2)
    $stats.RecordClick([float]$cps, $true)
}
for ($i = 0; $i -lt 45; $i++) {
    $cps = 14.0 + (([double]($i % 4)) * 0.3)
    $stats.RecordClick([float]$cps, $false)
}

# Put window into external control mode
$enterControl = [Aoko.MainWindow].GetMethod("EnterControlMode", [System.Reflection.BindingFlags]"NonPublic,Instance")
$enterControl.Invoke($win, $null)

$applyTheme = [Aoko.MainWindow].GetMethod("ApplyGuiTheme", [System.Reflection.BindingFlags]"NonPublic,Instance")
$applyTheme.Invoke($win, @($Theme))

$updUi = [Aoko.MainWindow].GetMethod("UpdateVersionAvailabilityUi", [System.Reflection.BindingFlags]"NonPublic,Instance")
$updUi.Invoke($win, $null)

$setUpd = [Aoko.MainWindow].GetMethod("SetUpdateStatus", [System.Reflection.BindingFlags]"NonPublic,Instance")
if ($setUpd) {
    $setUpd.Invoke($win, @("Up to date", "Current v0.11.4"))
}

# Explicitly ensure KillAura card is collapsed
$card = $win.FindName("KillAuraCard")
if ($card) { $card.Visibility = [System.Windows.Visibility]::Collapsed }

$win.Show()

function Pump-Events([int]$count = 2) {
    for ($i = 0; $i -lt $count; $i++) {
        $frame = [System.Windows.Threading.DispatcherFrame]::new()
        [System.Windows.Threading.Dispatcher]::CurrentDispatcher.BeginInvoke(
            [System.Windows.Threading.DispatcherPriority]::Background,
            [Action[object]]{ param($f) $f.Continue = $false },
            $frame
        ) | Out-Null
        [System.Windows.Threading.Dispatcher]::PushFrame($frame)
        [System.Threading.Thread]::Sleep(4)
    }
}

Pump-Events 30

$helper = [System.Windows.Interop.WindowInteropHelper]::new($win)
$hwnd = $helper.Handle

$rect = [NativeGuiCapture+RECT]::new()
[NativeGuiCapture]::DwmGetWindowAttribute($hwnd, 9, [ref]$rect, 16)
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top

$cp = $win.FindName("ControlPanel")

# Helper to capture the current window state to a file
function Save-WindowSnapshot([string]$destPath) {
    $bmp = [System.Drawing.Bitmap]::new($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [NativeGuiCapture]::PrintWindow($hwnd, $hdc, 2)
    $g.ReleaseHdc($hdc)
    $g.Dispose()

    $dir = Split-Path $destPath -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    $bmp.Save($destPath, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

function Sync-File([string]$sourcePath, [string]$fileName) {
    $targets = @(
        (Join-Path $OutDir $fileName),
        (Join-Path $WebsiteDir $fileName)
    )
    if (Test-Path (Split-Path $DistDir -Parent)) {
        $targets += (Join-Path $DistDir $fileName)
    }

    foreach ($t in $targets) {
        $p = Split-Path $t -Parent
        if (-not (Test-Path $p)) { New-Item -ItemType Directory -Path $p -Force | Out-Null }
        Copy-Item -Path $sourcePath -Destination $t -Force
        Write-Host ("  -> {0}" -f $t) -ForegroundColor DarkGray
    }
}

# 1. Capture primary still screenshot: 121_gui.png (Combat tab)
$cp.SelectedIndex = 0
Pump-Events 20
$tempStill = [System.IO.Path]::GetTempFileName() + ".png"
Save-WindowSnapshot $tempStill
Write-Host "Generated 121_gui.png" -ForegroundColor Green
Sync-File $tempStill "121_gui.png"

# 2. Capture individual tab screenshots if requested
if ($IncludeTabs) {
    $tabNames = @("combat", "render", "utility", "risky", "stats", "settings")
    for ($idx = 0; $idx -lt $tabNames.Length; $idx++) {
        $tName = $tabNames[$idx]
        $cp.SelectedIndex = $idx
        Pump-Events 15
        $tabPath = [System.IO.Path]::GetTempFileName() + ".png"
        Save-WindowSnapshot $tabPath
        $outName = "gui-$tName.png"
        Write-Host "Generated $outName" -ForegroundColor Green
        Sync-File $tabPath $outName
        Remove-Item $tabPath -ErrorAction SilentlyContinue
    }
    # Reset back to Combat tab
    $cp.SelectedIndex = 0
    Pump-Events 10
}

# 3. Generate Media (MP4 video and GIF showcase tour)
if (-not $NoVideo -or -not $NoGif) {
    $ffmpegCmd = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if (-not $ffmpegCmd) {
        Write-Warning "ffmpeg was not found in PATH; skipping media generation."
    } else {
        Write-Host "Recording showcase tour covering all tabs top-to-bottom..." -ForegroundColor Cyan
        $framesDir = Join-Path ([System.IO.Path]::GetTempPath()) ("aoko-frames-" + [System.Guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Path $framesDir -Force | Out-Null

        $global:frameIdx = 0

        function Record-Frame() {
            $bmp = [System.Drawing.Bitmap]::new($w, $h)
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            $hdc = $g.GetHdc()
            [NativeGuiCapture]::PrintWindow($hwnd, $hdc, 2)
            $g.ReleaseHdc($hdc)
            $g.Dispose()

            $fn = [string]::Format("frame_{0:D4}.png", $global:frameIdx)
            $p = Join-Path $framesDir $fn
            $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png)
            $bmp.Dispose()
            $global:frameIdx++
        }

        function Ease-InOut([double]$t) {
            return $t * $t * (3.0 - 2.0 * $t)
        }

        function Hold-View([int]$frames) {
            for ($i = 0; $i -lt $frames; $i++) {
                Pump-Events 1
                Record-Frame
            }
        }

        function Smooth-Scroll([int]$tabIdx, [double]$fromY, [double]$toY, [int]$frames, [string]$midTheme = "") {
            $tab = $cp.Items[$tabIdx]
            $sv = $tab.Content
            $switchAt = if ($midTheme) { [int]($frames * 0.45) } else { -1 }
            for ($i = 0; $i -lt $frames; $i++) {
                if ($i -eq $switchAt) {
                    Set-Theme $midTheme
                }
                $t = [double]$i / [double]$frames
                $e = Ease-InOut $t
                $y = $fromY + ($toY - $fromY) * $e
                $sv.ScrollToVerticalOffset($y)
                Pump-Events 1
                Record-Frame
            }
        }

        function Set-Theme([string]$themeName) {
            $applyTheme.Invoke($win, @($themeName))
            Pump-Events 6
        }

        try {
            # --- TAB 0: Combat (Theme: Steel -> mid-scroll Ink) ---
            Set-Theme "Steel"
            $cp.SelectedIndex = 0
            $tab0 = $cp.Items[0]
            $tab0.Content.ScrollToVerticalOffset(0)
            Pump-Events 15
            Hold-View 18
            $h0 = $tab0.Content.ScrollableHeight
            Smooth-Scroll 0 0 $h0 52 "Ink"
            Hold-View 16

            # --- TAB 1: Render (Theme: Slate -> mid-scroll Lush) ---
            Set-Theme "Slate"
            $cp.SelectedIndex = 1
            $tab1 = $cp.Items[1]
            $tab1.Content.ScrollToVerticalOffset(0)
            Pump-Events 15
            Hold-View 18
            $h1 = $tab1.Content.ScrollableHeight
            Smooth-Scroll 1 0 $h1 48 "Lush"
            Hold-View 16

            # --- TAB 2: Utility (Theme: Water -> mid-scroll Magic) ---
            Set-Theme "Water"
            $cp.SelectedIndex = 2
            $tab2 = $cp.Items[2]
            $tab2.Content.ScrollToVerticalOffset(0)
            Pump-Events 15
            Hold-View 18
            $h2 = $tab2.Content.ScrollableHeight
            Smooth-Scroll 2 0 $h2 50 "Magic"
            Hold-View 16

            # --- TAB 3: Risky (Theme: Graphite -> mid-scroll Coral, DevMode OFF: KillAura hidden) ---
            $card = $win.FindName("KillAuraCard")
            if ($card) { $card.Visibility = [System.Windows.Visibility]::Collapsed }
            Set-Theme "Graphite"
            $cp.SelectedIndex = 3
            $tab3 = $cp.Items[3]
            $tab3.Content.ScrollToVerticalOffset(0)
            Pump-Events 15
            Hold-View 18
            $h3 = $tab3.Content.ScrollableHeight
            if ($h3 -gt 0) {
                Smooth-Scroll 3 0 $h3 46 "Coral"
                Hold-View 16
            } else {
                Hold-View 18
                Set-Theme "Coral"
                Hold-View 18
            }

            # --- TAB 4: Stats (Theme: Blend -> mid-view Digital Horizon) ---
            Set-Theme "Blend"
            $cp.SelectedIndex = 4
            Pump-Events 15
            Hold-View 20
            Set-Theme "Digital Horizon"
            Hold-View 20

            # --- TAB 5: Settings (Theme: Lime Water -> mid-scroll Blossom -> bottom Steel) ---
            Set-Theme "Lime Water"
            $cp.SelectedIndex = 5
            $tab5 = $cp.Items[5]
            $tab5.Content.ScrollToVerticalOffset(0)
            Pump-Events 15
            Hold-View 18
            $h5 = $tab5.Content.ScrollableHeight
            Smooth-Scroll 5 0 $h5 48 "Blossom"
            Hold-View 16

            # Switch back to canonical Steel theme at the end of Settings
            Set-Theme "Steel"
            Hold-View 20

            # --- Loop smoothly back to Combat tab (Steel) ---
            $cp.SelectedIndex = 0
            $tab0.Content.ScrollToVerticalOffset(0)
            Pump-Events 15
            Hold-View 24

            Write-Host ("Captured {0} master animation frames." -f $global:frameIdx) -ForegroundColor Cyan
            $inputPattern = Join-Path $framesDir "frame_%04d.png"

            # 1. Output MP4 video if requested
            if (-not $NoVideo) {
                Write-Host "Encoding silky-smooth MP4 video..." -ForegroundColor Cyan
                $tempMp4 = [System.IO.Path]::GetTempFileName() + ".mp4"
                $mp4Args = @(
                    "-framerate", "$VideoFps",
                    "-i", "$inputPattern",
                    "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                    "-c:v", "libx264",
                    "-pix_fmt", "yuv420p",
                    "-crf", "18",
                    "-preset", "slow",
                    "-movflags", "+faststart",
                    "-y",
                    "$tempMp4"
                )
                & ffmpeg @mp4Args | Out-Null
                if ($LASTEXITCODE -eq 0 -and (Test-Path $tempMp4)) {
                    $mp4Size = (Get-Item $tempMp4).Length
                    Write-Host ("Generated gui.mp4 ({0:N1} MB)" -f ($mp4Size / 1MB)) -ForegroundColor Green
                    Sync-File $tempMp4 "gui.mp4"
                } else {
                    Write-Warning "ffmpeg failed to encode MP4 video."
                }
                Remove-Item $tempMp4 -ErrorAction SilentlyContinue
            }

            # 2. Output GIF if requested
            if (-not $NoGif) {
                Write-Host "Encoding high-fidelity showcase GIF..." -ForegroundColor Cyan
                $tempGif = [System.IO.Path]::GetTempFileName() + ".gif"
                $scaleFilter = ""
                if ($GifWidth -gt 0) {
                    $scaleFilter = "scale=${GifWidth}:-1:flags=lanczos,"
                }
                $filterStr = "[0:v] fps=$GifFps,${scaleFilter}split [a][b];[a] palettegen=max_colors=128:stats_mode=diff [p];[b][p] paletteuse=dither=bayer:bayer_scale=3"
                $gifArgs = @(
                    "-framerate", "$VideoFps",
                    "-i", "$inputPattern",
                    "-filter_complex", "$filterStr",
                    "-y",
                    "$tempGif"
                )
                & ffmpeg @gifArgs | Out-Null
                if ($LASTEXITCODE -eq 0 -and (Test-Path $tempGif)) {
                    $gifSize = (Get-Item $tempGif).Length
                    Write-Host ("Generated gui.gif ({0:N1} MB)" -f ($gifSize / 1MB)) -ForegroundColor Green
                    Sync-File $tempGif "gui.gif"
                } else {
                    Write-Warning "ffmpeg failed to encode GIF."
                }
                Remove-Item $tempGif -ErrorAction SilentlyContinue
            }
        }
        finally {
            Remove-Item -Recurse -Force $framesDir -ErrorAction SilentlyContinue
        }
    }
}

$win.Close()
Remove-Item $tempStill -ErrorAction SilentlyContinue
Write-Host "Successfully generated all media and refreshed screenshots!" -ForegroundColor Green

