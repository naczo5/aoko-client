using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Text.RegularExpressions;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// The command search overlay's contract in two halves: the row-state rules that decide whether a switch is
/// offered and what it shows (WPF-free, so they run anywhere), and the XAML/code shapes those rules depend on.
/// The ranking itself lives in ModuleSearchIndexTests; the palette table and MainWindow.xaml stay the source of
/// truth for what these tests read.
/// </summary>
public class CommandSearchOverlayTests
{
    private static readonly ModuleSearchIndex Index = ModuleSearchIndex.Default;
    private static readonly string OverlayXaml = TestRepo.Read("Aoko", "Views", "CommandSearchOverlay.xaml");
    private static readonly string OverlayCode = TestRepo.Read("Aoko", "Views", "CommandSearchOverlay.xaml.cs");
    private static readonly string MainWindowCode = TestRepo.Read("Aoko", "MainWindow.xaml.cs");
    private static readonly string AppXaml = TestRepo.Read("Aoko", "App.xaml");

    // ── The switch invariant ──────────────────────────────────────────────────

    /// <summary>
    /// The defect this pins: a row offered a switch for a module the current bridge does not run, the click
    /// went through InputHooks.ToggleModule, that returned without doing anything, and the thumb stayed on.
    /// An unsupported module therefore gets a disabled switch that reads off.
    /// </summary>
    [Fact]
    public void UnsupportedModule_OffersADisabledSwitchThatReadsOff()
    {
        CommandSearchRowState state = CommandSearchRowPolicy.Resolve(
            Entry("nametags"), devMode: false, supported: false, isOn: true, keybindText: "V");

        Assert.True(state.HasToggle, "The control stays: a hidden switch cannot say why the module is off.");
        Assert.False(state.IsAvailable, "An unsupported module must not offer a live switch.");
        Assert.False(state.IsOn, "An unsupported module is not running, so the switch reads off.");
        Assert.NotEmpty(state.UnavailableReason);
    }

    [Fact]
    public void SupportedModule_TogglingFlipsTheModuleAndTheSwitch()
    {
        var module = new FakeModule(supported: true);
        ModuleSearchEntry clicker = Entry("autoclicker");

        Assert.False(CommandSearchRowPolicy.Resolve(clicker, false, module.Supported, module.Enabled, "").IsOn);

        module.Toggle();

        CommandSearchRowState after = CommandSearchRowPolicy.Resolve(clicker, false, module.Supported, module.Enabled, "");
        Assert.True(module.Enabled);
        Assert.True(after.IsOn, "The switch has to follow the module it just toggled.");
        Assert.True(after.IsAvailable);
    }

    [Fact]
    public void RefusedToggle_CannotLeaveTheSwitchShowingTheOppositeState()
    {
        // Same shape as InputHooks.ToggleModule: the module the bridge does not advertise is left untouched.
        var module = new FakeModule(supported: false, enabled: true);
        ModuleSearchEntry nametags = Entry("nametags");

        module.Toggle();
        Assert.True(module.Enabled, "The refused toggle changed nothing, which is why the UI has to correct itself.");

        CommandSearchRowState state = CommandSearchRowPolicy.Resolve(nametags, false, module.Supported, module.Enabled, "");
        Assert.False(state.IsAvailable);
        Assert.False(state.IsOn);
    }

    /// <summary>Every entry, both bridge answers, DevMode on and off: no row may show a live switch for a module
    /// that is not running, and no row without a switch may report one as on.</summary>
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void NoRow_ShowsALiveSwitchForAModuleTheBridgeDoesNotRun(bool devMode)
    {
        foreach (ModuleSearchEntry entry in Index.Entries)
        {
            bool visible = !entry.DevOnly || devMode;
            bool hasSwitch = !entry.IsDestination && entry.ToggleProperty.Length > 0;

            CommandSearchRowState unsupported = CommandSearchRowPolicy.Resolve(entry, devMode, supported: false, isOn: true, "");
            Assert.False(unsupported.IsOn, $"'{entry.Id}' reads on while unsupported.");
            if (!entry.IsDestination)
                Assert.False(unsupported.IsAvailable, $"'{entry.Id}' is available while unsupported.");

            CommandSearchRowState supported = CommandSearchRowPolicy.Resolve(entry, devMode, supported: true, isOn: true, "");
            Assert.True(hasSwitch == supported.HasToggle, $"'{entry.Id}' switch visibility does not match its toggle.");
            Assert.True(visible == supported.IsAvailable, $"'{entry.Id}' availability does not match its visibility.");
            Assert.True((visible && hasSwitch) == supported.IsOn, $"'{entry.Id}' switch does not match its module.");
        }
    }

    [Fact]
    public void Destinations_AreTabsAndAreNotGatedByTheBridgeModuleList()
    {
        foreach (ModuleSearchEntry destination in Index.Entries.Where(entry => entry.IsDestination))
        {
            CommandSearchRowState state = CommandSearchRowPolicy.Resolve(destination, true, supported: false, isOn: true, "");

            Assert.True(state.IsAvailable, $"'{destination.Id}' is a tab, not a module the bridge advertises.");
            Assert.False(state.HasToggle);
            Assert.False(state.IsOn);
        }
    }

    [Fact]
    public void DevOnlyRows_AreHiddenUntilDevMode()
    {
        ModuleSearchEntry killAura = Entry("killaura");
        Assert.True(killAura.DevOnly);

        Assert.False(CommandSearchRowPolicy.Resolve(killAura, devMode: false, supported: true, isOn: true, "").IsVisible);
        Assert.True(CommandSearchRowPolicy.Resolve(killAura, devMode: true, supported: true, isOn: true, "").IsVisible);
    }

    [Fact]
    public void UnavailableReason_NamesTheVersionGate()
    {
        Assert.Contains("1.8.9", CommandSearchRowPolicy.UnavailableReason("triggerbot"));
        Assert.DoesNotContain("1.8.9", CommandSearchRowPolicy.UnavailableReason("nametags"));
    }

    [Fact]
    public void RowSwitch_BindsOnlyWhatThePolicyDecides()
    {
        // IsAvailable drives IsEnabled (a refusal is visible, not clickable) and IsOn drives the thumb. TwoWay
        // is deliberate: a click must write to the row, because a OneWay target written locally loses its
        // binding and no later correction could reach the thumb.
        Assert.Contains("IsEnabled=\"{Binding IsAvailable}\"", OverlayXaml, StringComparison.Ordinal);
        Assert.Contains("IsChecked=\"{Binding IsOn, Mode=TwoWay}\"", OverlayXaml, StringComparison.Ordinal);
        Assert.DoesNotContain("SetCurrentValue(ToggleButton", OverlayCode, StringComparison.Ordinal);

        // The click path goes through the row, and live state goes straight to the field without re-entering it.
        Assert.Contains("SwitchToggled = OnRowSwitchToggled", OverlayCode, StringComparison.Ordinal);
        Assert.Contains("SwitchToggled?.Invoke(this)", OverlayCode, StringComparison.Ordinal);
        Assert.Contains("SetField(ref _isOn, state.IsOn", OverlayCode, StringComparison.Ordinal);
    }

    [Fact]
    public void OpenOverlay_RefreshesWhenTheWindowReportsState()
    {
        // A keybind toggle during search must reach the rows, and the window must ask only while it is open.
        Assert.Contains("public void RefreshRows()", OverlayCode, StringComparison.Ordinal);
        Assert.Contains("CommandSearchHost.RefreshRows()", MainWindowCode, StringComparison.Ordinal);
        Assert.Contains("InputHooks.OnStateChanged += InputHooks_OnStateChanged", MainWindowCode, StringComparison.Ordinal);
    }

    // ── Motion, shadow, chrome ────────────────────────────────────────────────

    [Fact]
    public void OverlaySwitch_HasNoStoryboard()
    {
        // EnterActions on IsChecked replay on template application, so every ON row animated on every keystroke.
        Assert.DoesNotContain("Storyboard", OverlayXaml, StringComparison.Ordinal);
        Assert.Contains("x:Key=\"RowSwitchStyle\"", OverlayXaml, StringComparison.Ordinal);
        Assert.Contains("BasedOn=\"{StaticResource ModuleSwitch}\"", OverlayXaml, StringComparison.Ordinal);
    }

    [Fact]
    public void EveryOverlayAnimation_IsGatedOnSystemAnimationSettings()
    {
        foreach (string method in new[] { "AnimateIn", "Close", "PlaceCard" })
        {
            Assert.Contains("SystemParameters.ClientAreaAnimation", MethodBody(OverlayCode, method), StringComparison.Ordinal);
        }

        Assert.Contains(
            "SystemParameters.ClientAreaAnimation",
            MethodBody(MainWindowCode, "PulseModuleCard"),
            StringComparison.Ordinal);
    }

    [Fact]
    public void ShadowPlate_PaintsNoFrame()
    {
        Match plate = Regex.Match(OverlayXaml, @"<!-- Shadow plate:(?<body>.*?)</Border>", RegexOptions.Singleline);
        Assert.True(plate.Success, "The shadow plate is gone.");

        string body = plate.Groups["body"].Value;
        Assert.Contains("Background=\"#02060F\"", body, StringComparison.Ordinal);
        Assert.DoesNotContain("BorderThickness", body, StringComparison.Ordinal);
        Assert.DoesNotContain("BorderBrush", body, StringComparison.Ordinal);
        Assert.DoesNotContain("Margin", body, StringComparison.Ordinal);   // coincident with the card: no painted ring
        Assert.Contains("BlurRadius=\"28\"", body, StringComparison.Ordinal);

        // The card keeps its own translucency; the plate is a shadow carrier, not a replacement fill.
        Assert.Contains("Background=\"{DynamicResource CommandSearchGlassBrush}\"", OverlayXaml, StringComparison.Ordinal);
        Assert.Contains("Color.FromArgb(0xE4,", MainWindowCode, StringComparison.Ordinal);
    }

    [Fact]
    public void OverlaySwitch_KeepsTheProductDisabledLook()
    {
        // Same geometry is not enough: the disabled state is the one that has to agree, because an unavailable
        // row shows a disabled switch beside an already dimmed name.
        string product = Regex.Match(AppXaml, @"x:Key=""ModuleSwitch""(?<body>.*?)</Style>", RegexOptions.Singleline).Groups["body"].Value;
        string overlay = Regex.Match(OverlayXaml, @"x:Key=""RowSwitchStyle""(?<body>.*?)</Style>", RegexOptions.Singleline).Groups["body"].Value;

        string productOpacity = DisabledOpacity(product);
        Assert.False(string.IsNullOrEmpty(productOpacity), "ModuleSwitch has no disabled opacity.");
        Assert.Equal(productOpacity, DisabledOpacity(overlay));
        Assert.Contains("Width=\"46\" Height=\"24\"", overlay, StringComparison.Ordinal);
    }

    private static string DisabledOpacity(string style)
        => Regex.Match(
            style,
            @"Property=""IsEnabled"" Value=""False""\s*>\s*<Setter Property=""Opacity"" Value=""(?<value>[0-9.]+)""").Groups["value"].Value;

    [Fact]
    public void WindowFloor_KeepsTheCardInsideItsGutters()
    {
        double cardMin = Number(OverlayCode, @"CardMinWidth = (?<value>[0-9.]+);");
        double gutter = Number(OverlayCode, @"CardEdgeGutter = (?<value>[0-9.]+);");
        double minWidth = Number(MainWindowCode, @"MinWidth = (?<value>[0-9.]+);");
        double minHeight = Number(MainWindowCode, @"MinHeight = (?<value>[0-9.]+);");

        // The card never bleeds past the window, and the footer legend keeps its room below it.
        Assert.True(minWidth >= cardMin + (gutter * 2), $"MinWidth {minWidth} clips a {cardMin} px card with {gutter} px gutters.");
        Assert.True(minHeight >= 460, $"MinHeight {minHeight} is below the card plus its legend.");

        // A short window shows fewer rows instead of sliding the card, so the query field never moves.
        Assert.Contains("LimitResultsHeight", OverlayCode, StringComparison.Ordinal);
        Assert.Contains("x:Name=\"ResultsArea\"", OverlayXaml, StringComparison.Ordinal);
    }

    [Fact]
    public void Caret_ComesFromTheThemeNotTheAccent()
    {
        // The accent measures 2.68:1 on Aubergine, under the 3:1 non-text floor; TextBrush is above 14:1 there.
        Assert.Contains("CaretBrush=\"{DynamicResource TextBrush}\"", OverlayXaml, StringComparison.Ordinal);
        Assert.DoesNotContain("CaretBrush=\"{DynamicResource AccentBrush}\"", OverlayXaml, StringComparison.Ordinal);
    }

    [Fact]
    public void ResultsList_KeepsItsNameAndReportsTheCountAsHelpText()
    {
        Assert.Contains("AutomationProperties.Name=\"Search results\"", OverlayXaml, StringComparison.Ordinal);
        Assert.DoesNotContain("AutomationProperties.SetName(ResultsList", OverlayCode, StringComparison.Ordinal);
        Assert.Contains("AutomationProperties.SetHelpText(ResultsList, summary)", OverlayCode, StringComparison.Ordinal);
    }

    [Fact]
    public void Rows_AreNotTabStopsAndAnnounceSelection()
    {
        // Rows cannot take focus (Tab belongs to the query field), so the selection event is all a reader gets.
        Assert.Contains("Focusable=\"False\"", RowTemplate(), StringComparison.Ordinal);
        Assert.Contains("IsTabStop=\"False\"", RowTemplate(), StringComparison.Ordinal);
        Assert.Contains(
            "AutomationEvents.SelectionItemPatternOnElementSelected",
            OverlayCode,
            StringComparison.Ordinal);
    }

    [Fact]
    public void ToggleChord_SurvivesAnIme()
    {
        // Ctrl+Space is the CJK IME on/off chord, so the legend advertises F2 and the key handler accepts both.
        Assert.Contains("case Key.F2:", OverlayCode, StringComparison.Ordinal);
        Assert.Contains("<Run Text=\"f2\"", OverlayXaml, StringComparison.Ordinal);
        Assert.Contains("F2 toggles it", OverlayXaml, StringComparison.Ordinal);
    }

    [Fact]
    public void SpaceStillTypesEvenWithAModuleSelected()
    {
        // Kept from the earlier round: only a chorded Space toggles, so no query can arm a module.
        string keyHandler = MethodBody(OverlayCode, "QueryBox_PreviewKeyDown");
        Assert.Contains("case Key.Space:", keyHandler, StringComparison.Ordinal);
        Assert.Contains("(Keyboard.Modifiers & ModifierKeys.Control) == 0", keyHandler, StringComparison.Ordinal);
    }

    // ── Card tints and their contrast ─────────────────────────────────────────

    [Fact]
    public void CardTints_AreDerivedFromThePalette()
    {
        // A fixed light tint inverts on a light custom palette; PaletteStore accepts arbitrary hex.
        foreach (string key in new[]
                 {
                     "CommandSearchMatchBrush", "CommandSearchMatchPillBrush",
                     "CommandSearchPlaceholderBrush", "CommandSearchSubtleTextBrush",
                 })
        {
            Assert.Contains($"SetBrushColor(\"{key}\"", MainWindowCode, StringComparison.Ordinal);
        }

        Assert.DoesNotContain("#F6F9FD", OverlayXaml, StringComparison.Ordinal);
        Assert.DoesNotContain("#9CAFBC", OverlayXaml, StringComparison.Ordinal);
        Assert.Contains(
            "HighlightBrush=\"{DynamicResource CommandSearchMatchBrush}\"",
            OverlayXaml,
            StringComparison.Ordinal);
    }

    /// <summary>
    /// The measured contrast of every text composite the card draws, on every shipped palette and on a light
    /// custom one. This is the card's half of PaletteContrastTests: the numbers live in a test, not a comment.
    /// </summary>
    [Fact]
    public void CardText_PassesAaOnEveryPaletteIncludingALightCustomOne()
    {
        Dictionary<string, Dictionary<string, string>> palettes = ShippedPalettes();
        Assert.Equal(18, palettes.Count);

        double matchBlend = Number(MainWindowCode, @"CommandSearchMatchBrush"", Blend\(palette\.Text, palette\.Accent, (?<value>[0-9.]+)\)");
        double placeholderBlend = Number(MainWindowCode, @"CommandSearchPlaceholderBrush"", Blend\(palette\.DimText, palette\.Text, (?<value>[0-9.]+)\)");
        double subtleBlend = Number(MainWindowCode, @"CommandSearchSubtleTextBrush"", Blend\(palette\.DimText, palette\.Text, (?<value>[0-9.]+)\)");
        double pillAlpha = Convert.ToInt32(
            Regex.Match(MainWindowCode, @"CommandSearchMatchPillBrush"", Color\.FromArgb\((?<value>0x[0-9A-Fa-f]+),").Groups["value"].Value,
            16) / 255.0;

        // The washes are TextBrush at a fixed opacity in the XAML; assert the constants the maths uses.
        Assert.Contains("Value=\"0.14\"", OverlayXaml, StringComparison.Ordinal);
        Assert.Contains("Value=\"0.08\"", OverlayXaml, StringComparison.Ordinal);

        var lightCustom = new Dictionary<string, string>
        {
            ["Panel"] = "#FFFFFF",
            ["SliderBackground"] = "#EEF0F4",
            ["Accent"] = "#2B6CB0",
            ["Text"] = "#11151C",
            ["DimText"] = "#5A6472",
        };

        foreach ((string name, Dictionary<string, string> palette) in palettes.Select(pair => (pair.Key, pair.Value)).Concat(new[] { ("CustomLight", lightCustom) }))
        {
            string card = palette["Panel"];
            string wash = Over(palette["Text"], 0.14, card);
            string hover = Over(palette["Text"], 0.08, card);
            string pill = Over(palette["Accent"], pillAlpha, card);
            string selectedPill = Over(palette["Accent"], pillAlpha, wash);
            string match = Blend(palette["Text"], palette["Accent"], matchBlend);
            string placeholder = Blend(palette["DimText"], palette["Text"], placeholderBlend);
            string subtle = Blend(palette["DimText"], palette["Text"], subtleBlend);

            Check(name, "matched run on the pill", Ratio(match, pill), 4.5);
            Check(name, "matched run on a selected row", Ratio(match, selectedPill), 4.5);
            Check(name, "placeholder", Ratio(placeholder, card), 4.5);
            Check(name, "keybind chip", Ratio(subtle, palette["SliderBackground"]), 4.5);
            Check(name, "hint on a selected row", Ratio(subtle, wash), 4.5);
            Check(name, "hint on a hovered row", Ratio(subtle, hover), 4.5);

            // Both washes cost contrast, which is why the secondary text lifts while the row has one under it.
            Assert.True(
                Ratio(palette["DimText"], hover) < Ratio(palette["DimText"], card),
                $"{name}: the hover wash does not cost contrast, so the lift trigger is dead weight.");
        }

        // Aubergine is the shipped palette where a plain DimText hint would drop below AA once hovered (3.93:1).
        Dictionary<string, string> aubergine = palettes["Aubergine"];
        double plainHintWhileHovered = Ratio(aubergine["DimText"], Over(aubergine["Text"], 0.08, aubergine["Panel"]));
        Assert.True(plainHintWhileHovered < 4.5, $"Hover no longer dims the hint ({plainHintWhileHovered:0.00}:1); the trigger's reason is gone.");
    }

    [Fact]
    public void FixedLightTints_WouldHaveFailedOnALightPalette()
    {
        // The bug this replaced: the same tints on a white card erased the matched run (~1.06:1).
        const string card = "#FFFFFF";
        string pill = Over("#FFFFFF", 0x2E / 255.0, card);
        string wash = Over("#11151C", 0.14, card);

        Assert.True(Ratio("#F6F9FD", pill) < 2.0, "The old match tint is legible on a light card; the derivation test above is stale.");
        Assert.True(Ratio("#9CAFBC", card) < 3.0);
        Assert.True(Ratio("#C9D2DC", wash) < 2.0);
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private sealed class FakeModule(bool supported, bool enabled = false)
    {
        public bool Supported { get; } = supported;

        public bool Enabled { get; private set; } = enabled;

        /// <summary>Mirrors InputHooks.ToggleModule: a module the bridge does not advertise is left alone.</summary>
        public void Toggle()
        {
            if (Supported)
                Enabled = !Enabled;
        }
    }

    private static ModuleSearchEntry Entry(string id)
        => Index.Entries.Single(entry => string.Equals(entry.Id, id, StringComparison.OrdinalIgnoreCase));

    private static string RowTemplate()
        => Regex.Match(OverlayXaml, @"<views:CommandSearchRow(?<body>.*?)</views:CommandSearchRow>", RegexOptions.Singleline)
            .Groups["body"].Value;

    private static double Number(string source, string pattern)
    {
        Match match = Regex.Match(source, pattern);
        Assert.True(match.Success, $"No match for {pattern}.");
        return double.Parse(match.Groups["value"].Value, NumberStyles.Any, CultureInfo.InvariantCulture);
    }

    private static Dictionary<string, Dictionary<string, string>> ShippedPalettes()
    {
        var palettes = new Dictionary<string, Dictionary<string, string>>();

        // Brace matching, not a line pattern: the last palette is written without a trailing comma.
        foreach (Match start in Regex.Matches(MainWindowCode, @"\[""(?<name>[^""]+)""\] = new GuiPalette\s*\{"))
        {
            int open = start.Index + start.Length - 1;
            int depth = 0;
            int end = open;
            for (; end < MainWindowCode.Length; end++)
            {
                if (MainWindowCode[end] == '{') depth++;
                else if (MainWindowCode[end] == '}' && --depth == 0) break;
            }

            var colours = Regex.Matches(
                    MainWindowCode[(open + 1)..end],
                    @"(?<key>\w+) = \(Color\)ColorConverter\.ConvertFromString\(""(?<hex>#[0-9A-Fa-f]{6})""\)")
                .Cast<Match>()
                .ToDictionary(match => match.Groups["key"].Value, match => match.Groups["hex"].Value);

            palettes[start.Groups["name"].Value] = colours;
        }

        return palettes;
    }

    private static void Check(string palette, string surface, double ratio, double floor)
        => Assert.True(ratio >= floor, $"{surface} is {ratio:0.00}:1 on {palette}.");

    /// <summary>Body of a method, so a guard cannot be satisfied by another method's copy of the same string.</summary>
    private static string MethodBody(string source, string name)
    {
        Match signature = Regex.Match(source, $@"\b{name}\([^)]*\)\s*\r?\n?\s*\{{");
        Assert.True(signature.Success, $"No method named {name}.");

        int depth = 0;
        for (int i = signature.Index + signature.Length - 1; i < source.Length; i++)
        {
            if (source[i] == '{') depth++;
            else if (source[i] == '}' && --depth == 0) return source[signature.Index..(i + 1)];
        }

        Assert.Fail($"Unbalanced braces in {name}.");
        return string.Empty;
    }

    private static double Ratio(string a, string b)
    {
        double la = Luminance(a);
        double lb = Luminance(b);
        return (Math.Max(la, lb) + 0.05) / (Math.Min(la, lb) + 0.05);
    }

    private static double Luminance(string hex)
    {
        string value = hex.TrimStart('#');
        double r = Channel(value, 0);
        double g = Channel(value, 2);
        double b = Channel(value, 4);
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    }

    private static double Channel(string hex, int offset)
    {
        double s = Convert.ToInt32(hex.Substring(offset, 2), 16) / 255.0;
        return s <= 0.04045 ? s / 12.92 : Math.Pow((s + 0.055) / 1.055, 2.4);
    }

    private static string Blend(string top, string bottom, double amount)
    {
        int[] upper = Rgb(top);
        int[] lower = Rgb(bottom);
        var mixed = new int[3];
        for (int i = 0; i < 3; i++)
            mixed[i] = (int)Math.Round(upper[i] + ((lower[i] - upper[i]) * amount));

        return $"#{mixed[0]:X2}{mixed[1]:X2}{mixed[2]:X2}";
    }

    private static string Over(string top, double alpha, string bottom)
    {
        int[] upper = Rgb(top);
        int[] lower = Rgb(bottom);
        var mixed = new int[3];
        for (int i = 0; i < 3; i++)
            mixed[i] = (int)Math.Round((alpha * upper[i]) + ((1 - alpha) * lower[i]));

        return $"#{mixed[0]:X2}{mixed[1]:X2}{mixed[2]:X2}";
    }

    private static int[] Rgb(string hex)
    {
        string value = hex.TrimStart('#');
        return
        [
            Convert.ToInt32(value.Substring(0, 2), 16),
            Convert.ToInt32(value.Substring(2, 2), 16),
            Convert.ToInt32(value.Substring(4, 2), 16),
        ];
    }
}
