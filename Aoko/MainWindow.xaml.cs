using System;
using System.ComponentModel;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Threading;
using Aoko.Core;

namespace Aoko;

public partial class MainWindow : Window
{
    private sealed class GuiPalette
    {
        public required Color Background { get; init; }
        public required Color Panel { get; init; }
        public required Color SliderBackground { get; init; }
        public required Color SliderForeground { get; init; }
        public required Color Accent { get; init; }
        public required Color Text { get; init; }
        public required Color DimText { get; init; }
        public required Color TabSelected { get; init; }
        public required Color TabHover { get; init; }
    }

    /// <summary>Display model for a custom-palette preview card in the Settings tab.</summary>
    private sealed class CustomPaletteCard
    {
        public required string Name { get; init; }
        public required Brush BackgroundSwatch { get; init; }
        public required Brush PanelSwatch { get; init; }
        public required Brush SliderSwatch { get; init; }
        public required Brush AccentSwatch { get; init; }
    }

    // Built-in palette names that may not be overwritten or removed by the user.
    private static readonly HashSet<string> BuiltInPaletteNames =
        new(new[] { "Slate", "Ink", "Graphite", "Steel" }, StringComparer.OrdinalIgnoreCase);

    private readonly ObservableCollection<CustomPaletteCard> _customPaletteCards = new();

    private const int DwmaUseImmersiveDarkMode = 20;
    private const int DwmaBorderColor = 34;
    private const int DwmaCaptionColor = 35;
    private const int DwmaTextColor = 36;

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int dwAttribute, ref int pvAttribute, int cbAttribute);

    private bool _controlMode;
    private string? _pendingKeybindModuleId;
    private int _uiUpdateQueued;
    private readonly CancellationTokenSource _updateCheckCancellation = new();
    private string? _latestReleaseUrl;
    private bool _updateCheckInProgress;
    private static readonly Dictionary<string, string> ModuleTitles = new()
    {
        ["autoclicker"] = "AutoClicker",
        ["rightclick"] = "Right Click",
        ["jitter"] = "Jitter",
        ["clickinchests"] = "Click in Chests",
        ["breakblocks"] = "Break Blocks",
        ["aimassist"] = "Aim Assist",
        ["triggerbot"] = "Triggerbot",
        ["silentaura"] = "Silent Aura",
        ["speedbridge"] = "SpeedBridge",
        ["gtbhelper"] = "GTB Helper",
        ["pixelpartyassist"] = "Pixel Party Assist",
        ["nametags"] = "Nametags",
        ["chestesp"] = "Chest ESP",
        ["cheststealer"] = "Chest Stealer",
        ["blockesp"] = "Block ESP",
        ["closestplayer"] = "Closest Player",
        ["reach"] = "Reach",
        ["velocity"] = "Velocity",
        ["panic"] = "Panic"
    };

    private static readonly Dictionary<string, GuiPalette> GuiPalettes = new(StringComparer.OrdinalIgnoreCase)
    {
        ["Slate"] = new GuiPalette
        {
            Background = (Color)ColorConverter.ConvertFromString("#0A0B0F"),
            Panel = (Color)ColorConverter.ConvertFromString("#12141A"),
            SliderBackground = (Color)ColorConverter.ConvertFromString("#181B22"),
            SliderForeground = (Color)ColorConverter.ConvertFromString("#2A2F38"),
            Accent = (Color)ColorConverter.ConvertFromString("#C7625A"),
            Text = (Color)ColorConverter.ConvertFromString("#E8EAEE"),
            DimText = (Color)ColorConverter.ConvertFromString("#7A8290"),
            TabSelected = (Color)ColorConverter.ConvertFromString("#1F2229"),
            TabHover = (Color)ColorConverter.ConvertFromString("#181B22")
        },
        ["Ink"] = new GuiPalette
        {
            Background = (Color)ColorConverter.ConvertFromString("#08090B"),
            Panel = (Color)ColorConverter.ConvertFromString("#101115"),
            SliderBackground = (Color)ColorConverter.ConvertFromString("#16181C"),
            SliderForeground = (Color)ColorConverter.ConvertFromString("#262830"),
            Accent = (Color)ColorConverter.ConvertFromString("#B0B6C0"),
            Text = (Color)ColorConverter.ConvertFromString("#E8EAEE"),
            DimText = (Color)ColorConverter.ConvertFromString("#7A828F"),
            TabSelected = (Color)ColorConverter.ConvertFromString("#1D1F23"),
            TabHover = (Color)ColorConverter.ConvertFromString("#16181C")
        },
        ["Graphite"] = new GuiPalette
        {
            Background = (Color)ColorConverter.ConvertFromString("#0B0B0D"),
            Panel = (Color)ColorConverter.ConvertFromString("#131316"),
            SliderBackground = (Color)ColorConverter.ConvertFromString("#19191C"),
            SliderForeground = (Color)ColorConverter.ConvertFromString("#2A2A2D"),
            Accent = (Color)ColorConverter.ConvertFromString("#B89B82"),
            Text = (Color)ColorConverter.ConvertFromString("#E8E8EA"),
            DimText = (Color)ColorConverter.ConvertFromString("#82827E"),
            TabSelected = (Color)ColorConverter.ConvertFromString("#222226"),
            TabHover = (Color)ColorConverter.ConvertFromString("#19191C")
        },
        ["Steel"] = new GuiPalette
        {
            Background = (Color)ColorConverter.ConvertFromString("#08090C"),
            Panel = (Color)ColorConverter.ConvertFromString("#0F1218"),
            SliderBackground = (Color)ColorConverter.ConvertFromString("#161A21"),
            SliderForeground = (Color)ColorConverter.ConvertFromString("#262C35"),
            Accent = (Color)ColorConverter.ConvertFromString("#6B8DAB"),
            Text = (Color)ColorConverter.ConvertFromString("#E5E8EE"),
            DimText = (Color)ColorConverter.ConvertFromString("#7286A0"),
            TabSelected = (Color)ColorConverter.ConvertFromString("#1F232C"),
            TabHover = (Color)ColorConverter.ConvertFromString("#161A21")
        }
    };

    private static void LogUi(string msg)
    {
        try
        {
            string path = System.IO.Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "loader_ui_debug.log");
            System.IO.File.AppendAllText(path, $"[{DateTime.Now:HH:mm:ss.fff}] {msg}\r\n");
        }
        catch { }
    }

    public MainWindow()
    {
        InitializeComponent();
        LoadCustomPalettes();
        SourceInitialized += (_, _) => ApplyNativeTitleBarTheme();
        Loaded += async (_, _) => await CheckForUpdatesAsync();
        Activated += (_, _) => QueueRenderRefresh();
        StateChanged += (_, _) =>
        {
            if (WindowState != WindowState.Minimized)
                QueueRenderRefresh();
        };

        DataContext = Clicker.Instance;
        
        // Subscribe to connection state changes
        GameStateClient.Instance.StateUpdated += OnGameStateUpdated;
        GameStateClient.Instance.PropertyChanged += OnGameStateClientPropertyChanged;
        InputHooks.OnStateChanged += InputHooks_OnStateChanged;
        InputHooks.OnKeyCaptured += InputHooks_OnKeyCaptured;
        
        // Load Config
        var profile = ProfileManager.LoadProfile("config");
        if (profile != null)
        {
            ProfileManager.ApplyToClicker(profile);
        }

        if (DataContext is Clicker clicker)
        {
            ApplyGuiTheme(clicker.GuiTheme);
            clicker.ModuleListStyle = NormalizeModuleListStyle(clicker.ModuleListStyle);
        }

        DiscordRichPresenceService.Instance.Start();
        StatsTracker.Instance.StartSessionTimer();

        // Initial UI state
        UpdateGameStateUI();
        UpdateKeybindButtons();
        RefreshConfigList();
    }

    private void ToggleArmed_Click(object sender, RoutedEventArgs e)
    {
        Clicker.Instance.ToggleArmed();
    }

    internal void ShowControlCenterFromBridge()
    {
        // Window should be present at all times; this just brings it to front.
        if (!_controlMode)
            EnterControlMode();

        if (WindowState == WindowState.Minimized)
            WindowState = WindowState.Normal;

        Show();
        Activate();
        QueueRenderRefresh();
    }

    private void QueueRenderRefresh()
    {
        if (Dispatcher.HasShutdownStarted || Dispatcher.HasShutdownFinished) return;

        Dispatcher.BeginInvoke(() =>
        {
            if (!IsVisible) return;

            InvalidateVisual();
            if (Content is UIElement content)
                content.InvalidateVisual();

            UpdateLayout();
        }, DispatcherPriority.ContextIdle);
    }

    private void EnterControlMode()
    {
        if (_controlMode) return;
        _controlMode = true;

        LogUi("EnterControlMode()");

        Title = "aoko client";
        Width = 1020;
        Height = 760;
        ResizeMode = ResizeMode.CanResizeWithGrip;
        ApplyNativeTitleBarTheme();

        LoaderPanel.Visibility = Visibility.Collapsed;
        ControlPanel.Visibility = Visibility.Visible;

        ShowInTaskbar = true;
        Show();
        QueueRenderRefresh();
    }

    private void ApplyNativeTitleBarTheme()
    {
        IntPtr hwnd = new WindowInteropHelper(this).Handle;
        if (hwnd == IntPtr.Zero) return;

        int darkMode = 1;
        _ = DwmSetWindowAttribute(hwnd, DwmaUseImmersiveDarkMode, ref darkMode, sizeof(int));

        if (TryFindResource("PanelColor") is Color panelColor)
        {
            int caption = ToColorRef(panelColor);
            _ = DwmSetWindowAttribute(hwnd, DwmaCaptionColor, ref caption, sizeof(int));
            int border = ToColorRef(panelColor);
            _ = DwmSetWindowAttribute(hwnd, DwmaBorderColor, ref border, sizeof(int));
        }

        if (TryFindResource("TextColor") is Color textColor)
        {
            int text = ToColorRef(textColor);
            _ = DwmSetWindowAttribute(hwnd, DwmaTextColor, ref text, sizeof(int));
        }

    }

    private static int ToColorRef(Color color)
    {
        return color.R | (color.G << 8) | (color.B << 16);
    }

    private void EnsureControlModeIfNeeded(GameStateClient gs)
    {
        if (gs.IsConnected)
        {
            Dispatcher.Invoke(EnterControlMode);
        }
    }

    private static bool IsModuleSupported(string moduleId)
        => string.Equals(moduleId, "panic", StringComparison.OrdinalIgnoreCase)
            || GameStateClient.Instance.SupportsModule(moduleId);

    private static string GetUnavailableModuleReason(string moduleId)
    {
        if (string.Equals(moduleId, "triggerbot", StringComparison.OrdinalIgnoreCase))
            return "Unavailable on 1.8.9 (cooldown-era PvP only)";
        if (string.Equals(moduleId, "silentaura", StringComparison.OrdinalIgnoreCase))
            return "Unavailable on 1.8.9 (modern interaction API only)";

        return "Unavailable on current bridge";
    }

    private void UpdateVersionAvailabilityUi()
    {
        bool aimAssistSupported = IsModuleSupported("aimassist");
        bool triggerbotSupported = IsModuleSupported("triggerbot");
        bool silentAuraSupported = IsModuleSupported("silentaura");
        bool speedBridgeSupported = IsModuleSupported("speedbridge");
        bool gtbSupported = IsModuleSupported("gtbhelper");
        bool pixelPartySupported = IsModuleSupported("pixelpartyassist");
        bool chestStealerSupported = IsModuleSupported("cheststealer");
        bool reachSupported = IsModuleSupported("reach");
        bool velocitySupported = IsModuleSupported("velocity");
        bool autoTotemSupported = IsModuleSupported("autototem");
        bool antiDebuffSupported = IsModuleSupported("antidebuff");
        bool reloadMappingsSupported = GameStateClient.Instance.SupportsSetting("reloadMappingsNonce");

        AimAssistCard.IsEnabled = aimAssistSupported;
        TriggerbotCard.IsEnabled = triggerbotSupported;
        SilentAuraCard.IsEnabled = silentAuraSupported;
        SpeedBridgeCard.IsEnabled = speedBridgeSupported;
        GtbHelperCard.IsEnabled = gtbSupported;
        PixelPartyAssistCard.IsEnabled = pixelPartySupported;
        ChestStealerCard.IsEnabled = chestStealerSupported;
        ReachCard.IsEnabled = reachSupported;
        VelocityCard.IsEnabled = velocitySupported;
        AutoTotemCard.IsEnabled = autoTotemSupported;
        AntiDebuffCard.IsEnabled = antiDebuffSupported;

        var clicker = Clicker.Instance;
        if (!aimAssistSupported && clicker.AimAssistEnabled) clicker.AimAssistEnabled = false;
        if (!triggerbotSupported && clicker.TriggerbotEnabled) clicker.TriggerbotEnabled = false;
        if (!silentAuraSupported && clicker.SilentAuraEnabled) clicker.SilentAuraEnabled = false;
        if (!speedBridgeSupported && clicker.SpeedBridgeEnabled) clicker.SpeedBridgeEnabled = false;
        if (!gtbSupported && clicker.GtbHelperEnabled) clicker.GtbHelperEnabled = false;
        if (!pixelPartySupported && clicker.PixelPartyAssistEnabled) clicker.PixelPartyAssistEnabled = false;
        if (!chestStealerSupported && clicker.ChestStealerEnabled) clicker.ChestStealerEnabled = false;
        if (!reachSupported && clicker.ReachEnabled) clicker.ReachEnabled = false;
        if (!velocitySupported && clicker.VelocityEnabled) clicker.VelocityEnabled = false;
        if (!autoTotemSupported && clicker.AutoTotemEnabled) clicker.AutoTotemEnabled = false;
        if (!antiDebuffSupported && clicker.AntiDebuffEnabled) clicker.AntiDebuffEnabled = false;

        // Update availability text - only show unavailable message for Triggerbot (intentionally 1.21-only)
        AimAssistAvailabilityText.Text = aimAssistSupported ? "Available" : "Unavailable on current bridge";
        TriggerbotAvailabilityText.Text = triggerbotSupported ? "Available" : "Unavailable on 1.8.9 (cooldown-era PvP only)";
        SilentAuraAvailabilityText.Text = silentAuraSupported ? "Available" : "Unavailable on 1.8.9 (modern interaction API only)";
        SpeedBridgeAvailabilityText.Text = speedBridgeSupported ? "Available" : "Unavailable on current bridge";
        ReachAvailabilityText.Text = reachSupported ? "Available" : "Unavailable on current bridge";
        VelocityAvailabilityText.Text = velocitySupported ? "Available" : "Unavailable on current bridge";
        AutoTotemAvailabilityText.Text = autoTotemSupported ? "Available" : "Unavailable on current bridge";
        AntiDebuffAvailabilityText.Text = antiDebuffSupported ? "Available" : "Unavailable on current bridge";
        ChestStealerAvailabilityText.Text = chestStealerSupported ? "Available" : "Unavailable on current bridge";
        GtbHelperAvailabilityText.Text = gtbSupported
            ? "Hypixel Guess The Build helper using action-bar hints."
            : "Unavailable on current bridge";
        PixelPartyAssistAvailabilityText.Text = pixelPartySupported
            ? "Hypixel Pixel Party: finds the closest matching terracotta on the floor."
            : "Unavailable on current bridge";

        KeybindAimAssistButton.IsEnabled = aimAssistSupported;
        KeybindTriggerbotButton.IsEnabled = triggerbotSupported;
        KeybindSilentAuraButton.IsEnabled = silentAuraSupported;
        KeybindSpeedBridgeButton.IsEnabled = speedBridgeSupported;
        KeybindGtbHelperButton.IsEnabled = gtbSupported;
        KeybindPixelPartyAssistButton.IsEnabled = pixelPartySupported;
        KeybindChestStealerButton.IsEnabled = chestStealerSupported;
        KeybindReachButton.IsEnabled = reachSupported;
        KeybindVelocityButton.IsEnabled = velocitySupported;
        KeybindAutoTotemButton.IsEnabled = autoTotemSupported;
        KeybindAntiDebuffButton.IsEnabled = antiDebuffSupported;
        KeybindPanicButton.IsEnabled = true;
        ReloadMappingsButton.IsEnabled = reloadMappingsSupported;
        ReloadMappingsAvailabilityText.Text = reloadMappingsSupported
            ? "Available"
            : "Unavailable on 1.8.9 bridge";
    }
    
    private void TitleBar_MouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ClickCount == 1)
            DragMove();
    }


    private void GuiTheme_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (DataContext is Clicker clicker)
        {
            ApplyGuiTheme(clicker.GuiTheme);
        }
    }

    private void GuiPaletteButton_Click(object sender, RoutedEventArgs e)
    {
        if (DataContext is not Clicker clicker || sender is not Button { Tag: string themeName })
            return;

        clicker.GuiTheme = themeName;
        ApplyGuiTheme(themeName);
    }

    private void ModuleStyleButton_Click(object sender, RoutedEventArgs e)
    {
        if (DataContext is not Clicker clicker || sender is not Button { Tag: string styleName })
            return;

        clicker.ModuleListStyle = NormalizeModuleListStyle(styleName);
    }

    // ── Configs ─────────────────────────────────────────────────────────────────

    /// <summary>Reloads the config dropdown from disk, preserving the selection when possible.</summary>
    private void RefreshConfigList(string? selectName = null)
    {
        if (ConfigComboBox is null)
            return;

        string? previous = selectName ?? ConfigComboBox.SelectedItem as string;

        List<string> names;
        try
        {
            names = ProfileManager.GetConfigNames();
        }
        catch (Exception ex)
        {
            SetConfigStatus($"Could not list configs: {ex.Message}");
            return;
        }

        ConfigComboBox.ItemsSource = names;

        if (previous != null && names.Contains(previous))
            ConfigComboBox.SelectedItem = previous;
        else if (names.Count > 0)
            ConfigComboBox.SelectedIndex = 0;
    }

    private void SetConfigStatus(string message)
    {
        if (ConfigStatusText != null)
            ConfigStatusText.Text = message;
    }

    private void LoadConfigButton_Click(object sender, RoutedEventArgs e)
    {
        if (ConfigComboBox?.SelectedItem is not string name || string.IsNullOrWhiteSpace(name))
        {
            SetConfigStatus("Select a config to load.");
            return;
        }

        Profile? profile = ProfileManager.LoadProfile(name);
        if (profile == null)
        {
            SetConfigStatus($"Failed to load config '{name}'.");
            RefreshConfigList();
            return;
        }

        ApplyLoadedProfile(profile);
        SetConfigStatus($"Loaded config '{name}'.");
    }

    /// <summary>Applies a loaded profile and refreshes the dependent UI surfaces.</summary>
    private void ApplyLoadedProfile(Profile profile)
    {
        ProfileManager.ApplyToClicker(profile);

        if (DataContext is Clicker clicker)
        {
            ApplyGuiTheme(clicker.GuiTheme);
            clicker.ModuleListStyle = NormalizeModuleListStyle(clicker.ModuleListStyle);
        }

        UpdateKeybindButtons();
    }

    private void SaveConfigButton_Click(object sender, RoutedEventArgs e)
    {
        if (ConfigComboBox?.SelectedItem is not string name || string.IsNullOrWhiteSpace(name))
        {
            SetConfigStatus("Select a config to overwrite, or create a new one.");
            return;
        }

        if (!TrySaveCurrentAs(name, out string error))
        {
            SetConfigStatus(error);
            return;
        }

        SetConfigStatus($"Saved current settings to '{name}'.");
    }

    private void DeleteConfigButton_Click(object sender, RoutedEventArgs e)
    {
        if (ConfigComboBox?.SelectedItem is not string name || string.IsNullOrWhiteSpace(name))
        {
            SetConfigStatus("Select a config to delete.");
            return;
        }

        var confirm = MessageBox.Show(
            $"Delete config '{name}'? This cannot be undone.",
            "Delete Config",
            MessageBoxButton.YesNo,
            MessageBoxImage.Warning);
        if (confirm != MessageBoxResult.Yes)
            return;

        try
        {
            ProfileManager.DeleteProfile(name);
        }
        catch (Exception ex)
        {
            SetConfigStatus($"Could not delete '{name}': {ex.Message}");
            return;
        }

        RefreshConfigList();
        SetConfigStatus($"Deleted config '{name}'.");
    }

    private void CreateConfigButton_Click(object sender, RoutedEventArgs e)
    {
        string raw = NewConfigNameBox?.Text ?? string.Empty;
        string name = ProfileManager.SanitizeConfigName(raw);

        if (name.Length == 0)
        {
            SetConfigStatus("Enter a valid config name (letters, numbers, spaces, - or _).");
            return;
        }

        if (string.Equals(name, ProfileManager.AutoSaveConfigName, StringComparison.OrdinalIgnoreCase))
        {
            SetConfigStatus($"'{name}' is a reserved name. Choose another.");
            return;
        }

        if (ProfileManager.ConfigExists(name))
        {
            var confirm = MessageBox.Show(
                $"Config '{name}' already exists. Overwrite it?",
                "Overwrite Config",
                MessageBoxButton.YesNo,
                MessageBoxImage.Question);
            if (confirm != MessageBoxResult.Yes)
                return;
        }

        if (!TrySaveCurrentAs(name, out string error))
        {
            SetConfigStatus(error);
            return;
        }

        if (NewConfigNameBox != null)
            NewConfigNameBox.Text = string.Empty;

        RefreshConfigList(name);
        SetConfigStatus($"Created config '{name}'.");
    }

    private void NewConfigNameBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter)
        {
            CreateConfigButton_Click(sender, e);
            e.Handled = true;
        }
    }

    /// <summary>Captures the current clicker state and writes it to a config file.</summary>
    private static bool TrySaveCurrentAs(string name, out string error)
    {
        string sanitized = ProfileManager.SanitizeConfigName(name);
        if (sanitized.Length == 0)
        {
            error = "Invalid config name.";
            return false;
        }

        try
        {
            Profile profile = ProfileManager.CreateFromClicker();
            profile.Name = sanitized;
            ProfileManager.SaveProfile(profile);
            error = string.Empty;
            return true;
        }
        catch (Exception ex)
        {
            error = $"Could not save config: {ex.Message}";
            return false;
        }
    }

    private void OpenConfigsFolderButton_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            string dir = ProfileManager.ConfigsDirectory;
            Directory.CreateDirectory(dir);
            Process.Start(new ProcessStartInfo
            {
                FileName = dir,
                UseShellExecute = true
            });
        }
        catch (Exception ex)
        {
            SetConfigStatus($"Could not open configs folder: {ex.Message}");
        }
    }

    // ── Custom palettes ─────────────────────────────────────────────────────────

    /// <summary>Loads saved custom palettes, registers them as selectable themes, and builds the cards.</summary>
    private void LoadCustomPalettes()
    {
        List<CustomPalette> palettes = PaletteStore.LoadAll();

        foreach (CustomPalette p in palettes)
            RegisterPalette(p);

        if (CustomPalettesItems != null)
            CustomPalettesItems.ItemsSource = _customPaletteCards;

        RebuildCustomPaletteCards(palettes);
    }

    /// <summary>Adds (or replaces) a custom palette in the in-memory theme dictionaries.</summary>
    private static void RegisterPalette(CustomPalette p)
    {
        if (string.IsNullOrWhiteSpace(p.Name) || BuiltInPaletteNames.Contains(p.Name))
            return;

        Color bg       = ParseHexOr(p.Background, Colors.Black);
        Color panel    = ParseHexOr(p.Panel, bg);
        Color sliderBg = ParseHexOr(p.SliderBackground, Lighten(panel, 0.06));
        Color sliderFg = ParseHexOr(p.SliderForeground, Lighten(panel, 0.16));
        Color accent   = ParseHexOr(p.Accent, Colors.Gray);
        Color text     = ParseHexOr(p.Text, Colors.White);
        Color dimText  = ParseHexOr(p.DimText, Blend(text, bg, 0.45));
        Color tabSel   = ParseHexOr(p.TabSelected, Lighten(panel, 0.10));
        Color tabHov   = ParseHexOr(p.TabHover, Lighten(panel, 0.04));

        GuiPalettes[p.Name] = new GuiPalette
        {
            Background = bg,
            Panel = panel,
            SliderBackground = sliderBg,
            SliderForeground = sliderFg,
            Accent = accent,
            Text = text,
            DimText = dimText,
            TabSelected = tabSel,
            TabHover = tabHov,
        };

        ThemeManager.Themes[p.Name] = new ThemeColors
        {
            Background = bg,
            Panel = panel,
            SliderBg = sliderBg,
            SliderFg = sliderFg,
            Accent = accent,
            Text = text,
            DimText = dimText,
        };
    }

    private void RebuildCustomPaletteCards(IEnumerable<CustomPalette> palettes)
    {
        _customPaletteCards.Clear();
        foreach (CustomPalette p in palettes)
        {
            _customPaletteCards.Add(new CustomPaletteCard
            {
                Name = p.Name,
                BackgroundSwatch = new SolidColorBrush(ParseHexOr(p.Background, Colors.Black)),
                PanelSwatch = new SolidColorBrush(ParseHexOr(p.Panel, Colors.Black)),
                SliderSwatch = new SolidColorBrush(ParseHexOr(p.SliderBackground, Colors.Black)),
                AccentSwatch = new SolidColorBrush(ParseHexOr(p.Accent, Colors.Gray)),
            });
        }
    }

    private void SetPaletteStatus(string message)
    {
        if (PaletteStatusText != null)
            PaletteStatusText.Text = message;
    }

    private void PaletteHexBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        UpdatePalettePreview(PaletteBgBox, PaletteBgPreview);
        UpdatePalettePreview(PalettePanelBox, PalettePanelPreview);
        UpdatePalettePreview(PaletteAccentBox, PaletteAccentPreview);
        UpdatePalettePreview(PaletteTextBox, PaletteTextPreview);
    }

    /// <summary>Opens the color picker for the swatch's bound text box and writes back the chosen hex.</summary>
    private void ColorSwatch_MouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (sender is not FrameworkElement { Tag: TextBox target })
            return;

        Color initial = TryParseHex(target.Text, out Color c) ? c : Colors.Black;

        if (ColorPickerDialog.TryPick(this, initial, out Color picked))
            target.Text = ColorToHex(picked); // triggers PaletteHexBox_TextChanged -> preview refresh
    }

    private static void UpdatePalettePreview(TextBox? box, Border? preview)
    {
        if (box is null || preview is null)
            return;

        if (TryParseHex(box.Text, out Color color))
            preview.Background = new SolidColorBrush(color);
    }

    private void AddCustomPaletteButton_Click(object sender, RoutedEventArgs e)
    {
        string name = (NewPaletteNameBox?.Text ?? string.Empty).Trim();

        if (!IsValidPaletteName(name))
        {
            SetPaletteStatus("Enter a palette name (letters, numbers, spaces, - or _; max 32).");
            return;
        }

        if (BuiltInPaletteNames.Contains(name))
        {
            SetPaletteStatus($"'{name}' is a built-in palette name. Choose another.");
            return;
        }

        if (!TryParseHex(PaletteBgBox?.Text, out Color bg) ||
            !TryParseHex(PalettePanelBox?.Text, out Color panel) ||
            !TryParseHex(PaletteAccentBox?.Text, out Color accent) ||
            !TryParseHex(PaletteTextBox?.Text, out Color text))
        {
            SetPaletteStatus("One or more colors are invalid. Use hex like #1A2B3C.");
            return;
        }

        bool exists = PaletteStore.LoadAll()
            .Any(p => string.Equals(p.Name, name, StringComparison.OrdinalIgnoreCase));
        if (exists)
        {
            var confirm = MessageBox.Show(
                $"A custom palette named '{name}' already exists. Overwrite it?",
                "Overwrite Palette",
                MessageBoxButton.YesNo,
                MessageBoxImage.Question);
            if (confirm != MessageBoxResult.Yes)
                return;
        }

        // Derive the supporting roles from the four user-picked colors.
        var palette = new CustomPalette
        {
            Name = name,
            Background = ColorToHex(bg),
            Panel = ColorToHex(panel),
            Accent = ColorToHex(accent),
            Text = ColorToHex(text),
            SliderBackground = ColorToHex(Lighten(panel, 0.06)),
            SliderForeground = ColorToHex(Lighten(panel, 0.16)),
            DimText = ColorToHex(Blend(text, bg, 0.45)),
            TabSelected = ColorToHex(Lighten(panel, 0.10)),
            TabHover = ColorToHex(Lighten(panel, 0.04)),
        };

        List<CustomPalette> all;
        try
        {
            all = PaletteStore.Upsert(palette);
        }
        catch (Exception ex)
        {
            SetPaletteStatus($"Could not save palette: {ex.Message}");
            return;
        }

        RegisterPalette(palette);
        RebuildCustomPaletteCards(all);

        // Apply the new palette immediately.
        if (DataContext is Clicker clicker)
            clicker.GuiTheme = name;
        ApplyGuiTheme(name);

        if (NewPaletteNameBox != null)
            NewPaletteNameBox.Text = string.Empty;

        SetPaletteStatus($"Saved and applied palette '{name}'.");
    }

    private void DeleteCustomPaletteButton_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button { Tag: string name } || string.IsNullOrWhiteSpace(name))
            return;

        var confirm = MessageBox.Show(
            $"Delete custom palette '{name}'?",
            "Delete Palette",
            MessageBoxButton.YesNo,
            MessageBoxImage.Warning);
        if (confirm != MessageBoxResult.Yes)
            return;

        List<CustomPalette> all;
        try
        {
            all = PaletteStore.Delete(name);
        }
        catch (Exception ex)
        {
            SetPaletteStatus($"Could not delete palette: {ex.Message}");
            return;
        }

        GuiPalettes.Remove(name);
        ThemeManager.Themes.Remove(name);
        RebuildCustomPaletteCards(all);

        // If the deleted palette was active, fall back to the default theme.
        if (DataContext is Clicker clicker &&
            string.Equals(clicker.GuiTheme, name, StringComparison.OrdinalIgnoreCase))
        {
            clicker.GuiTheme = "Slate";
            ApplyGuiTheme("Slate");
        }

        SetPaletteStatus($"Deleted palette '{name}'.");
    }

    private static bool IsValidPaletteName(string name)
    {
        if (string.IsNullOrWhiteSpace(name) || name.Length > 32)
            return false;

        foreach (char c in name)
        {
            if (!char.IsLetterOrDigit(c) && c != ' ' && c != '-' && c != '_')
                return false;
        }
        return true;
    }

    /// <summary>Parses <c>#RGB</c>, <c>#RRGGBB</c>, or the same without the leading '#'.</summary>
    private static bool TryParseHex(string? text, out Color color)
    {
        color = Colors.Black;
        if (string.IsNullOrWhiteSpace(text))
            return false;

        string s = text.Trim();
        if (s.StartsWith("#", StringComparison.Ordinal))
            s = s.Substring(1);

        if (s.Length == 3)
        {
            // Expand shorthand (#RGB -> #RRGGBB).
            s = string.Concat(s[0], s[0], s[1], s[1], s[2], s[2]);
        }

        if (s.Length != 6)
            return false;

        if (!byte.TryParse(s.AsSpan(0, 2), System.Globalization.NumberStyles.HexNumber, null, out byte r) ||
            !byte.TryParse(s.AsSpan(2, 2), System.Globalization.NumberStyles.HexNumber, null, out byte g) ||
            !byte.TryParse(s.AsSpan(4, 2), System.Globalization.NumberStyles.HexNumber, null, out byte b))
            return false;

        color = Color.FromRgb(r, g, b);
        return true;
    }

    private static Color ParseHexOr(string? text, Color fallback)
        => TryParseHex(text, out Color c) ? c : fallback;

    private static string ColorToHex(Color c) => $"#{c.R:X2}{c.G:X2}{c.B:X2}";

    /// <summary>Moves a color toward white by <paramref name="amount"/> in [0,1].</summary>
    private static Color Lighten(Color c, double amount)
    {
        amount = Math.Clamp(amount, 0.0, 1.0);
        byte L(byte v) => (byte)Math.Round(v + (255 - v) * amount);
        return Color.FromRgb(L(c.R), L(c.G), L(c.B));
    }

    /// <summary>Linear blend: <paramref name="t"/>=0 returns <paramref name="a"/>, 1 returns <paramref name="b"/>.</summary>
    private static Color Blend(Color a, Color b, double t)
    {
        t = Math.Clamp(t, 0.0, 1.0);
        byte M(byte x, byte y) => (byte)Math.Round(x + (y - x) * t);
        return Color.FromRgb(M(a.R, b.R), M(a.G, b.G), M(a.B, b.B));
    }

    private static string NormalizeModuleListStyle(string? styleName)
    {
        if (string.IsNullOrWhiteSpace(styleName))
            return "Default";

        return styleName.Trim().ToLowerInvariant() switch
        {
            "default" => "Default",
            "minimal" => "Minimal",
            "outlined" => "Outlined",
            "glass" => "Glass",
            "bold" => "Bold",
            _ => "Default"
        };
    }

    private static string NormalizeThemeName(string? themeName)
    {
        if (string.IsNullOrWhiteSpace(themeName))
            return "Slate";

        foreach (var name in GuiPalettes.Keys)
        {
            if (name.Equals(themeName.Trim(), StringComparison.OrdinalIgnoreCase))
                return name;
        }

        return "Slate";
    }

    private void ApplyGuiTheme(string? themeName)
    {
        string normalized = NormalizeThemeName(themeName);
        if (!GuiPalettes.TryGetValue(normalized, out GuiPalette? palette))
            palette = GuiPalettes["Slate"];

        SetColorResource("BgColor", palette.Background);
        SetColorResource("PanelColor", palette.Panel);
        SetColorResource("SliderBgColor", palette.SliderBackground);
        SetColorResource("SliderFgColor", palette.SliderForeground);
        SetColorResource("AccentColor", palette.Accent);
        SetColorResource("TextColor", palette.Text);
        SetColorResource("DimTextColor", palette.DimText);

        SetBrushColor("BgBrush", palette.Background);
        SetBrushColor("PanelBrush", palette.Panel);
        SetBrushColor("SliderBgBrush", palette.SliderBackground);
        SetBrushColor("SliderFgBrush", palette.SliderForeground);
        SetBrushColor("AccentBrush", palette.Accent);
        SetBrushColor("TextBrush", palette.Text);
        SetBrushColor("DimTextBrush", palette.DimText);
        SetBrushColor("ControlCenterTabSelectedBrush", palette.TabSelected, this.Resources);
        SetBrushColor("ControlCenterTabHoverBrush", palette.TabHover, this.Resources);

        if (DataContext is Clicker clicker)
            clicker.GuiTheme = normalized;

        ApplyNativeTitleBarTheme();
        UpdateGameStateUI();
    }

    private static void SetColorResource(string key, Color value)
    {
        var app = Application.Current;
        if (app == null) return;
        app.Resources[key] = value;
    }

    private static void SetBrushColor(string key, Color value, ResourceDictionary? dictionary = null)
    {
        var target = dictionary ?? Application.Current?.Resources;
        if (target == null) return;

        if (target[key] is SolidColorBrush brush)
        {
            if (!brush.IsFrozen)
            {
                brush.Color = value;
                return;
            }

            target[key] = new SolidColorBrush(value);
            return;
        }

        target[key] = new SolidColorBrush(value);
    }

    private void CloseButton_Click(object sender, RoutedEventArgs e)
    {
        // Save Config
        var profile = ProfileManager.CreateFromClicker();
        profile.Name = "config";
        ProfileManager.SaveProfile(profile);

        Application.Current.Shutdown();
    }

    private void PanicButton_Click(object sender, RoutedEventArgs e)
    {
        Clicker.Instance.TriggerPanic();
    }

    private void ReloadMappingsButton_Click(object sender, RoutedEventArgs e)
    {
        if (!GameStateClient.Instance.SupportsSetting("reloadMappingsNonce"))
            return;

        GameStateClient.Instance.RequestBridgeMappingReload();
    }

    private void EditHudButton_Click(object sender, RoutedEventArgs e)
    {
        Clicker.Instance.HudEditorActive = !Clicker.Instance.HudEditorActive;
    }

    private void ResetHudLayoutButton_Click(object sender, RoutedEventArgs e)
    {
        Clicker.Instance.HudLayout.ResetAll();
        ProfileManager.SaveProfile(ProfileManager.CreateFromClicker());
    }

    internal void EnterPanicStealthMode()
    {
        if (!Dispatcher.CheckAccess())
        {
            Dispatcher.Invoke(EnterPanicStealthMode);
            return;
        }

        _pendingKeybindModuleId = null;
        InputHooks.StopKeyCapture();
        ShowInTaskbar = false;
        WindowState = WindowState.Minimized;
        Hide();
    }
    
    private async void InjectButton_Click(object sender, RoutedEventArgs e)
    {
        await RunInjectionAsync();
    }

    private async void CustomInjectButton_Click(object sender, RoutedEventArgs e)
    {
        var picker = new WindowPickerDialog
        {
            Owner = this
        };

        if (picker.ShowDialog() != true || picker.SelectedTarget == null)
            return;

        await RunInjectionAsync(
            picker.SelectedVersion,
            picker.SelectedTarget.ProcessId,
            picker.SelectedTarget.Hwnd);
    }

    private async Task RunInjectionAsync(string version = "auto", int? targetPid = null, IntPtr? targetHwnd = null)
    {
        SetInjectionButtonsEnabled(false);
        InjectButton.Content = "...";
        InjectionStatusText.Text = "Status: Connecting...";

        LogUi($"RunInjectionAsync: version={version} pid={targetPid?.ToString() ?? "auto"}");
        bool success = await GameStateClient.Instance.InjectAsync(version, targetPid, targetHwnd);

        LogUi($"InjectAsync returned: success={success} IsConnected={GameStateClient.Instance.IsConnected} InjectedVersion={GameStateClient.Instance.InjectedVersion}");

        if (success)
            EnterControlMode();

        InjectButton.Content = success ? "Connected" : "Inject into Lunar Client";
        SetInjectionButtonsEnabled(!success);

        UpdateGameStateUI();
    }

    private void SetInjectionButtonsEnabled(bool enabled)
    {
        InjectButton.IsEnabled = enabled;
        CustomInjectButton.IsEnabled = enabled;
    }

    private void UpdateGameStateUI()
    {
        if (Dispatcher.HasShutdownStarted || Dispatcher.HasShutdownFinished) return;
        if (Interlocked.Exchange(ref _uiUpdateQueued, 1) != 0) return;

        try
        {
            Dispatcher.BeginInvoke(() =>
            {
                try
                {
                    var gs = GameStateClient.Instance;

                    if (gs.IsConnected)
                    {
                        InjectionStatusText.Text = "Status: Connected & Injected";
                        InjectionStatusText.Foreground = (Brush)(TryFindResource("AccentBrush") ?? new SolidColorBrush(Color.FromRgb(167, 125, 255)));
                        InjectionProgressBar.Visibility = Visibility.Collapsed;
                        InjectionProgressBar.Value = 100;
                        InjectButton.Content = "Connected";
                        SetInjectionButtonsEnabled(false);

                        EnsureControlModeIfNeeded(gs);
                        UpdateVersionAvailabilityUi();
                    }
                    else
                    {
                        InjectionStatusText.Text = $"Status: {gs.StatusMessage}";
                        InjectionStatusText.Foreground = new SolidColorBrush(Color.FromRgb(200, 200, 200));
                        InjectionProgressBar.Visibility = gs.IsInjectionInProgress ? Visibility.Visible : Visibility.Collapsed;
                        InjectionProgressBar.Value = gs.IsInjectionInProgress ? gs.InjectionProgress : 0;

                        if (!InjectButton.IsEnabled && !gs.IsInjected)
                        {
                            SetInjectionButtonsEnabled(true);
                            InjectButton.Content = "Inject into Lunar Client";
                        }
                    }

                    if (!gs.IsConnected)
                    {
                        UpdateVersionAvailabilityUi();
                    }
                }
                finally
                {
                    Interlocked.Exchange(ref _uiUpdateQueued, 0);
                }
            });
        }
        catch
        {
            Interlocked.Exchange(ref _uiUpdateQueued, 0);
            throw;
        }
    }

    private void OnGameStateUpdated()
    {
        UpdateGameStateUI();
    }

    private void OnGameStateClientPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(GameStateClient.IsConnected) ||
            e.PropertyName == nameof(GameStateClient.StatusMessage) ||
            e.PropertyName == nameof(GameStateClient.IsInjectionInProgress) ||
            e.PropertyName == nameof(GameStateClient.InjectionProgress) ||
            e.PropertyName == nameof(GameStateClient.IsInjected) ||
            e.PropertyName == nameof(GameStateClient.InjectedVersion) ||
            e.PropertyName == nameof(GameStateClient.Capabilities))
        {
            UpdateGameStateUI();
        }
    }

    protected override void OnClosed(EventArgs e)
    {
        _updateCheckCancellation.Cancel();
        DiscordRichPresenceService.Instance.Stop();
        StatsTracker.Instance.StopSessionTimer();

        var profile = ProfileManager.CreateFromClicker();
        profile.Name = "config";
        ProfileManager.SaveProfile(profile);

        InputHooks.OnStateChanged -= InputHooks_OnStateChanged;
        InputHooks.OnKeyCaptured -= InputHooks_OnKeyCaptured;
        GameStateClient.Instance.StateUpdated -= OnGameStateUpdated;
        GameStateClient.Instance.PropertyChanged -= OnGameStateClientPropertyChanged;
        base.OnClosed(e);
    }

    private async void UpdateStatusButton_Click(object sender, RoutedEventArgs e)
    {
        if (!string.IsNullOrWhiteSpace(_latestReleaseUrl))
        {
            try
            {
                Process.Start(new ProcessStartInfo(_latestReleaseUrl) { UseShellExecute = true });
            }
            catch (Exception ex)
            {
                Debug.WriteLine($"Failed to open release page: {ex.Message}");
            }

            return;
        }

        await CheckForUpdatesAsync();
    }

    private async Task CheckForUpdatesAsync()
    {
        if (_updateCheckInProgress || _updateCheckCancellation.IsCancellationRequested)
            return;

        _updateCheckInProgress = true;
        _latestReleaseUrl = null;
        SetUpdateStatus("Checking for updates...", $"Current v{UpdateService.CurrentVersion}");

        try
        {
            UpdateCheckResult result = await UpdateService.CheckAsync(_updateCheckCancellation.Token);
            if (result.IsUpdateAvailable)
            {
                _latestReleaseUrl = result.ReleaseUrl;
                SetUpdateStatus($"Update v{result.LatestVersion} available", $"Current v{result.CurrentVersion} • click to view");
            }
            else
            {
                SetUpdateStatus("Up to date", $"Current v{result.CurrentVersion}");
            }
        }
        catch (OperationCanceledException) when (_updateCheckCancellation.IsCancellationRequested)
        {
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"Update check failed: {ex.Message}");
            SetUpdateStatus("Update check unavailable", $"Current v{UpdateService.CurrentVersion} • click to retry");
        }
        finally
        {
            _updateCheckInProgress = false;
        }
    }

    private void SetUpdateStatus(string status, string version)
    {
        ControlPanel.ApplyTemplate();
        if (ControlPanel.Template.FindName("UpdateStatusText", ControlPanel) is TextBlock statusText)
            statusText.Text = status;
        if (ControlPanel.Template.FindName("CurrentVersionText", ControlPanel) is TextBlock versionText)
            versionText.Text = version;
    }

    private void KeybindButton_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button btn || btn.Tag is not string moduleId) return;
        if (!IsModuleSupported(moduleId)) return;
        _pendingKeybindModuleId = moduleId;
        InputHooks.StartKeyCapture();
        UpdateKeybindButtons();
    }

    private void ResetStatsButton_Click(object sender, RoutedEventArgs e)
    {
        StatsTracker.Instance.Reset();
    }

    // ── Block ESP block-list editing ────────────────────────────────────────────

    private void BlockEspAdd_Click(object sender, RoutedEventArgs e) => AddBlockEspFromBox();

    private void BlockEspAddBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter)
        {
            AddBlockEspFromBox();
            e.Handled = true;
        }
    }

    private void AddBlockEspFromBox()
    {
        string raw = BlockEspAddBox?.Text ?? string.Empty;
        string id = BlockEspConfig.NormalizeId(raw);
        if (id.Length == 0)
            return;

        var targets = Clicker.Instance.BlockEspTargets;
        if (targets.Any(t => BlockEspConfig.NormalizeId(t.RegistryId) == id))
        {
            // Already present: just enable it.
            BlockEspTarget existing = targets.First(t => BlockEspConfig.NormalizeId(t.RegistryId) == id);
            existing.Enabled = true;
        }
        else
        {
            targets.Add(new BlockEspTarget($"minecraft:{id}", id.Replace('_', ' '), BlockEspConfig.DefaultColorHex, true));
        }

        if (BlockEspAddBox != null)
            BlockEspAddBox.Text = string.Empty;
    }

    private void BlockEspRemove_Click(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement fe && fe.DataContext is BlockEspTarget target)
            Clicker.Instance.BlockEspTargets.Remove(target);
    }

    private void BlockEspColor_Click(object sender, MouseButtonEventArgs e)
    {
        if (sender is not FrameworkElement fe || fe.DataContext is not BlockEspTarget target)
            return;

        Color initial = Colors.Cyan;
        try
        {
            string hex = "#" + BlockEspConfig.NormalizeColor(target.ColorHex);
            initial = (Color)ColorConverter.ConvertFromString(hex);
        }
        catch { /* fall back to cyan */ }

        if (ColorPickerDialog.TryPick(this, initial, out Color picked))
            target.ColorHex = $"{picked.R:X2}{picked.G:X2}{picked.B:X2}";
    }

    private void InputHooks_OnKeyCaptured(int vkCode)
    {
        string? moduleId = _pendingKeybindModuleId;
        if (string.IsNullOrWhiteSpace(moduleId)) return;

        int finalVk = (vkCode == 0x1B) ? 0 : vkCode; // ESC unbinds.
        InputHooks.SetModuleKey(moduleId, finalVk);
        _pendingKeybindModuleId = null;
        Dispatcher.BeginInvoke(UpdateKeybindButtons);
    }

    private void InputHooks_OnStateChanged()
    {
        Dispatcher.BeginInvoke(UpdateKeybindButtons);
    }

    private void UpdateKeybindButtons()
    {
        SetKeybindButtonContent(KeybindAutoclickerButton, "autoclicker");
        SetKeybindButtonContent(KeybindRightClickButton, "rightclick");
        SetKeybindButtonContent(KeybindJitterButton, "jitter");
        SetKeybindButtonContent(KeybindClickInChestsButton, "clickinchests");
        SetKeybindButtonContent(KeybindBreakBlocksButton, "breakblocks");
        SetKeybindButtonContent(KeybindAimAssistButton, "aimassist");
        SetKeybindButtonContent(KeybindTriggerbotButton, "triggerbot");
        SetKeybindButtonContent(KeybindSilentAuraButton, "silentaura");
        SetKeybindButtonContent(KeybindSpeedBridgeButton, "speedbridge");
        SetKeybindButtonContent(KeybindGtbHelperButton, "gtbhelper");
        SetKeybindButtonContent(KeybindPixelPartyAssistButton, "pixelpartyassist");
        SetKeybindButtonContent(KeybindNametagsButton, "nametags");
        SetKeybindButtonContent(KeybindChestEspButton, "chestesp");
        SetKeybindButtonContent(KeybindChestStealerButton, "cheststealer");
        SetKeybindButtonContent(KeybindBlockEspButton, "blockesp");
        SetKeybindButtonContent(KeybindClosestPlayerButton, "closestplayer");
        SetKeybindButtonContent(KeybindReachButton, "reach");
        SetKeybindButtonContent(KeybindVelocityButton, "velocity");
        SetKeybindButtonContent(KeybindAutoTotemButton, "autototem");
        SetKeybindButtonContent(KeybindAntiDebuffButton, "antidebuff");
        SetKeybindButtonContent(KeybindPanicButton, "panic");
    }

    private void SetKeybindButtonContent(Button btn, string moduleId)
    {
        string title = ModuleTitles.TryGetValue(moduleId, out string? n) ? n : moduleId;
        if (!IsModuleSupported(moduleId))
        {
            btn.Content = $"{title}: {GetUnavailableModuleReason(moduleId)}";
            return;
        }

        if (_pendingKeybindModuleId == moduleId)
            btn.Content = $"{title}: [Press key...]";
        else
            btn.Content = $"{title}: {FormatVirtualKey(InputHooks.GetModuleKey(moduleId))}";
    }

    private static string FormatVirtualKey(int vk)
    {
        if (vk <= 0) return "Unbound";
        if (vk >= 0x70 && vk <= 0x7B) return $"F{vk - 0x6F}";
        if (vk == 0xC0) return "`";
        if (vk >= 0x30 && vk <= 0x39) return ((char)vk).ToString();
        if (vk >= 0x41 && vk <= 0x5A) return ((char)vk).ToString();
        return ((Key)KeyInterop.KeyFromVirtualKey(vk)).ToString();
    }
}

/// <summary>
/// Converts a 6-digit RRGGBB hex string (with or without '#') to a <see cref="SolidColorBrush"/>
/// for the Block ESP color swatches. Falls back to cyan on invalid input.
/// </summary>
public sealed class HexColorToBrushConverter : System.Windows.Data.IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, System.Globalization.CultureInfo culture)
    {
        string hex = value as string ?? "";
        if (!hex.StartsWith("#", StringComparison.Ordinal))
            hex = "#" + hex;
        try
        {
            return new SolidColorBrush((Color)ColorConverter.ConvertFromString(hex));
        }
        catch
        {
            return new SolidColorBrush(Colors.Cyan);
        }
    }

    public object ConvertBack(object value, Type targetType, object parameter, System.Globalization.CultureInfo culture)
        => throw new NotSupportedException();
}
