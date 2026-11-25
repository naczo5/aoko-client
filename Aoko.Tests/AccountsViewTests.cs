using System;
using System.IO;
using System.Text.RegularExpressions;
using Aoko.Core.Accounts;
using Aoko.Views;

namespace Aoko.Tests;

/// <summary>
/// Covers the Accounts tab's pure rules: the settings file, the session strip, the
/// source chips, the capability gate, and the rule that nothing is armed until the
/// user picks a card. Also keeps the XAML honest about named elements and handlers.
/// </summary>
public class AccountsViewTests
{
    private static readonly string Xaml = TestRepo.Read("Aoko", "Views", "AccountsView.xaml");
    private static readonly string ServersXaml = TestRepo.Read("Aoko", "Views", "ServersView.xaml");
    private static readonly string CodeBehind = TestRepo.Read("Aoko", "Views", "AccountsView.xaml.cs");
    private static readonly string Styles = TestRepo.Read("Aoko", "Views", "TabStyles.xaml");

    // ----- AccountSettings ---------------------------------------------------

    [Fact]
    public void AccountSettings_MissingFileStartsWithLunarWritesDisabled()
    {
        using var temp = new TempDirectory();
        var settings = new AccountSettings(Path.Combine(temp.Path, "accounts-settings.json"));

        Assert.False(settings.PopulateLunarAccountManager);
        Assert.Empty(Directory.GetFiles(temp.Path));
    }

    [Fact]
    public void AccountSettings_RoundTripsCamelCaseJsonAtomically()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts-settings.json");

        var settings = new AccountSettings(path);
        settings.PopulateLunarAccountManager = true;
        settings.Save();

        string json = File.ReadAllText(path);
        Assert.Contains("\"populateLunarAccountManager\": true", json);
        Assert.DoesNotContain("DefaultPath", json);
        Assert.False(File.Exists(path + ".tmp"));

        var reloaded = new AccountSettings(path);
        Assert.True(reloaded.PopulateLunarAccountManager);

        reloaded.PopulateLunarAccountManager = false;
        reloaded.Save();
        Assert.False(new AccountSettings(path).PopulateLunarAccountManager);
    }

    [Fact]
    public void AccountSettings_ReadsPropertyNamesCaseInsensitively()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts-settings.json");
        File.WriteAllText(path, "{ \"PopulateLunarAccountManager\": true }");

        Assert.True(new AccountSettings(path).PopulateLunarAccountManager);
    }

    [Fact]
    public void AccountSettings_CorruptFileFallsBackToDefaultsAndIsKept()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts-settings.json");
        File.WriteAllText(path, "{ not json");

        var settings = new AccountSettings(path);

        Assert.False(settings.PopulateLunarAccountManager);
        Assert.False(File.Exists(path));
        string quarantined = Assert.Single(Directory.GetFiles(temp.Path, "accounts-settings.json.corrupt-*"));
        Assert.Equal("{ not json", File.ReadAllText(quarantined));

        settings.PopulateLunarAccountManager = true;
        settings.Save();
        Assert.True(new AccountSettings(path).PopulateLunarAccountManager);
    }

    [Fact]
    public void AccountSettings_IgnoresUnusablePayloadShapes()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts-settings.json");
        File.WriteAllText(path, "[1,2,3]");

        Assert.False(new AccountSettings(path).PopulateLunarAccountManager);
    }

    // ----- Relative time -----------------------------------------------------

    [Fact]
    public void RelativeTime_FormatsEachBucket()
    {
        DateTimeOffset now = DateTimeOffset.UtcNow;

        Assert.Equal("just now", RelativeTime.Format(now));
        Assert.Equal("just now", RelativeTime.Format(now.AddHours(1)));
        Assert.Equal("5m ago", RelativeTime.Format(now.AddMinutes(-5)));
        Assert.Equal("3h ago", RelativeTime.Format(now.AddHours(-3)));
        Assert.Equal("3d ago", RelativeTime.Format(now.AddDays(-3)));
        Assert.Equal("2mo ago", RelativeTime.Format(now.AddDays(-60)));
        Assert.Equal("1y ago", RelativeTime.Format(now.AddDays(-400)));
    }

    [Fact]
    public void LastUsedLabel_UsesSentenceCaseAndNeverSaysNeverUsedTwice()
    {
        Assert.Equal("Never used", AccountsViewLogic.LastUsedLabel(null));
        Assert.Equal(
            "Last used 3d ago",
            AccountsViewLogic.LastUsedLabel(DateTimeOffset.UtcNow.AddDays(-3).AddMinutes(-30)));
    }

    // ----- Source chip mapping ----------------------------------------------

    [Theory]
    [InlineData(AccountSources.Offline, false, "Cracked")]
    [InlineData(AccountSources.Localts, true, "Localts")]
    [InlineData(AccountSources.Localts, false, "Localts")]
    [InlineData(AccountSources.Token, true, "Localts")]
    [InlineData(AccountSources.Token, false, "Token")]
    [InlineData(AccountSources.Cookie, false, "Cookie")]
    [InlineData(AccountSources.Unknown, false, "Saved")]
    [InlineData(null, false, "Saved")]
    public void SourceChipLabel_MapsStoredSource(string? source, bool hasRefresh, string expected)
        => Assert.Equal(expected, AccountsViewLogic.SourceChipLabel(source, hasRefresh));

    [Fact]
    public void SourceChipLabel_IgnoresCase()
    {
        Assert.Equal("Localts", AccountsViewLogic.SourceChipLabel("LOCALTS", false));
        Assert.Equal("Cracked", AccountsViewLogic.SourceChipLabel("Offline", true));
        Assert.Equal("Cookie", AccountsViewLogic.SourceChipLabel("Cookie", false));
    }

    [Theory]
    [InlineData(AccountSources.Offline, false, "Offline")]
    [InlineData(AccountSources.Localts, false, "Localts")]
    [InlineData(AccountSources.Token, true, "Localts")]
    [InlineData(AccountSources.Token, false, "Neutral")]
    [InlineData(AccountSources.Cookie, false, "Neutral")]
    [InlineData(null, false, "Neutral")]
    // The enum is internal to the view assembly, so it cannot sit in a public test signature.
    public void SourceToneFor_SelectsChipTint(string? source, bool hasRefresh, string expected)
        => Assert.Equal(expected, AccountsViewLogic.SourceToneFor(source, hasRefresh).ToString());

    // ----- Session strip -----------------------------------------------------

    [Theory]
    [InlineData(false, false, "Offline")]
    [InlineData(false, true, "Offline")]
    [InlineData(true, false, "MainMenu")]
    [InlineData(true, true, "InWorld")]
    public void SessionChipToneFor_OrdersConnectionBeforeWorld(bool connected, bool inWorld,
        string expected)
        => Assert.Equal(expected, AccountsViewLogic.SessionChipToneFor(connected, inWorld).ToString());

    [Fact]
    public void SessionCaption_NeverAsksForADisabledRefresh()
    {
        Assert.Contains("Inject the client", AccountsViewLogic.SessionCaption(connected: false, supportsSessionRead: true));
        Assert.Contains("cannot read", AccountsViewLogic.SessionCaption(connected: true, supportsSessionRead: false));
        Assert.Equal("Refresh to read it.", AccountsViewLogic.SessionCaption(connected: true, supportsSessionRead: true));
    }

    [Fact]
    public void FormatUuid_AddsDashesOnlyForCompactIds()
    {
        Assert.Equal("b50ad385-829d-3141-a216-7e7d7539ba7f",
            AccountsViewLogic.FormatUuid("b50ad385829d3141a2167e7d7539ba7f"));
        Assert.Equal("b50ad385-829d-3141-a216-7e7d7539ba7f",
            AccountsViewLogic.FormatUuid("b50ad385-829d-3141-a216-7e7d7539ba7f"));
        Assert.Equal("", AccountsViewLogic.FormatUuid(null));
        Assert.Equal("offline", AccountsViewLogic.FormatUuid("offline"));
    }

    [Fact]
    public void IsCurrentSession_IgnoresDashesAndCase()
    {
        Assert.True(AccountsViewLogic.IsCurrentSession("b50ad385829d3141a2167e7d7539ba7f",
            "B50AD385-829D-3141-A216-7E7D7539BA7F"));
        Assert.False(AccountsViewLogic.IsCurrentSession("b50ad385829d3141a2167e7d7539ba7f", ""));
        Assert.False(AccountsViewLogic.IsCurrentSession("", "b50ad385829d3141a2167e7d7539ba7f"));
        Assert.False(AccountsViewLogic.IsCurrentSession(null, "b50ad385829d3141a2167e7d7539ba7f"));
    }

    [Fact]
    public void CardAutomationName_AddsInUseAndKeepsTheSource()
    {
        Assert.Equal("Steve, Localts, in use", AccountsViewLogic.CardAutomationName("Steve", "Localts", true));
        Assert.Equal("Steve, Cookie", AccountsViewLogic.CardAutomationName("Steve", "Cookie", false));
    }

    // ----- Capability gate and selection gating ------------------------------

    [Theory]
    [InlineData(true, true, true)]
    [InlineData(true, false, false)]
    [InlineData(false, true, false)]
    [InlineData(false, false, false)]
    public void CanSwitchSessions_NeedsConnectedAndCapableBridge(bool connected, bool supported, bool expected)
        => Assert.Equal(expected, AccountsViewLogic.CanSwitchSessions(connected, supported));

    [Theory]
    [InlineData(true, true, false, true, true)]
    [InlineData(true, true, true, true, false)]
    [InlineData(true, true, false, false, false)]
    [InlineData(true, false, false, true, false)]
    [InlineData(false, true, false, true, false)]
    public void CanUseSelectedAccount_NeedsIdleCapabilityAndAnExplicitSelection(
        bool connected,
        bool supported,
        bool busy,
        bool hasSelection,
        bool expected)
    {
        Assert.Equal(
            expected,
            AccountsViewLogic.CanUseSelectedAccount(connected, supported, busy, hasSelection));
    }

    [Fact]
    public void SelectionIndexAfterLoad_NeverFallsBackToTheFirstCard()
    {
        string[] ids = { "one", "two", "three" };

        Assert.Equal(-1, AccountsViewLogic.SelectionIndexAfterLoad(ids, keepId: null));
        Assert.Equal(-1, AccountsViewLogic.SelectionIndexAfterLoad(ids, keepId: "gone"));
        Assert.Equal(2, AccountsViewLogic.SelectionIndexAfterLoad(ids, keepId: "three"));
        Assert.Equal(-1, AccountsViewLogic.SelectionIndexAfterLoad(Array.Empty<string>(), keepId: "one"));
    }

    [Fact]
    public void PrimaryHint_ExplainsADisabledPrimaryAction()
    {
        Assert.Equal(
            TabCopy.NotConnected,
            AccountsViewLogic.PrimaryHint(connected: false, supportsSetSession: true, hasSelection: true));
        Assert.Equal(
            TabCopy.UnsupportedAccountSwitch,
            AccountsViewLogic.PrimaryHint(connected: true, supportsSetSession: false, hasSelection: true));
        Assert.Equal(
            TabCopy.SelectAccountHint,
            AccountsViewLogic.PrimaryHint(connected: true, supportsSetSession: true, hasSelection: false));
        Assert.Equal(
            "",
            AccountsViewLogic.PrimaryHint(connected: true, supportsSetSession: true, hasSelection: true));
    }

    [Fact]
    public void SessionSwitchRefusal_ExplainsEveryBlockedState()
    {
        Assert.Equal(
            TabCopy.UnsupportedAccountSwitch,
            AccountsViewLogic.SessionSwitchRefusal(connected: true, supportsSetSession: false, inWorld: false));
        Assert.Equal(
            TabCopy.ReturnToMainMenu,
            AccountsViewLogic.SessionSwitchRefusal(connected: true, supportsSetSession: true, inWorld: true));
        Assert.Null(AccountsViewLogic.SessionSwitchRefusal(connected: true, supportsSetSession: true, inWorld: false));

        string? offline = AccountsViewLogic.SessionSwitchRefusal(connected: false, supportsSetSession: true, inWorld: false);
        Assert.Equal(TabCopy.NotConnected, offline);
    }

    // ----- XAML / code-behind contract --------------------------------------

    [Theory]
    [InlineData("StatusText")]
    [InlineData("PrimaryHint")]
    [InlineData("RecoveryText")]
    [InlineData("BusyBar")]
    [InlineData("SessionNameText")]
    [InlineData("SessionUuidText")]
    [InlineData("ChipOffline")]
    [InlineData("ChipMainMenu")]
    [InlineData("ChipWorld")]
    [InlineData("RefreshSessionButton")]
    [InlineData("CardsControl")]
    [InlineData("EmptyState")]
    [InlineData("DropHint")]
    [InlineData("SessionStrip")]
    [InlineData("UseAccountButton")]
    [InlineData("RestoreSessionButton")]
    [InlineData("DeleteButton")]
    [InlineData("ImportFileButton")]
    [InlineData("PasteImportButton")]
    [InlineData("AddOfflineButton")]
    [InlineData("PastePanel")]
    [InlineData("PasteBox")]
    [InlineData("PastePlaceholder")]
    [InlineData("PasteValidationText")]
    [InlineData("PasteAuthenticateButton")]
    [InlineData("PasteCancelButton")]
    [InlineData("OfflinePanel")]
    [InlineData("OfflineNameBox")]
    [InlineData("OfflineValidationText")]
    [InlineData("OfflineAddButton")]
    [InlineData("OfflineCancelButton")]
    [InlineData("StoragePathText")]
    [InlineData("OpenFolderButton")]
    [InlineData("LunarAccountsCheck")]
    [InlineData("LunarStatusText")]
    public void Xaml_DeclaresEveryNamedElementTheCodeTouches(string name)
        => Assert.Contains($"x:Name=\"{name}\"", Xaml);

    [Fact]
    public void Xaml_HandlersAllExistInTheCodeBehind()
    {
        // The lookbehind keeps AllowDrop="True" from being read as a Drop handler.
        MatchCollection handlers = Regex.Matches(
            Xaml,
            @"(?<![\w:])(?:Click|DragEnter|DragOver|DragLeave|Drop|TextChanged|KeyDown|MouseLeftButtonDown|MouseDoubleClick|SelectionChanged)=""([A-Za-z_][A-Za-z0-9_]*)""");

        Assert.NotEmpty(handlers);
        foreach (Match handler in handlers)
        {
            string signature = $"void {handler.Groups[1].Value}(";
            Assert.Contains(signature, CodeBehind);
        }
    }

    [Fact]
    public void Xaml_UsesAListBoxSoTheGridIsKeyboardReachable()
    {
        Assert.Contains("<ListBox x:Name=\"CardsControl\"", Xaml);
        Assert.Contains("ItemContainerStyle=\"{StaticResource AccountCardItem}\"", Xaml);
        Assert.Contains("SelectionMode=\"Single\"", Xaml);
        Assert.Contains("IsKeyboardFocused", Xaml);
        Assert.DoesNotContain("MouseLeftButtonDown", Xaml);
    }

    [Fact]
    public void Xaml_OfflinePanelQuotesTheOfflineAccountValidationMessage()
    {
        Assert.False(OfflineAccount.TryCreate("bad name", out _, out _, out string message));

        Assert.Contains(message, Xaml);
        Assert.Contains("AccountsViewLogic.InvalidOfflineName", CodeBehind);
        Assert.Equal("Use 3–16 letters, numbers or underscores.", AccountsViewLogic.InvalidOfflineName);
    }

    [Fact]
    public void Xaml_UsesDynamicThemeBrushesOnly()
    {
        // Static resources on a theme brush would freeze the colour when the palette
        // switches. Fixed tone brushes (critical/warn) are the documented exception.
        Assert.DoesNotMatch(
            @"\{StaticResource (?:Accent|Bg|Panel|SliderBg|SliderFg|Text|DimText)Brush\}",
            Xaml);
        Assert.DoesNotMatch(
            @"\{StaticResource (?:Accent|Bg|Panel|SliderBg|SliderFg|Text|DimText)Brush\}",
            Styles);
    }

    [Fact]
    public void Xaml_KeepsTintsThemeAware()
    {
        // Accent-derived washes have to come from {DynamicResource AccentBrush} on a
        // layer with Opacity; a pre-baked tint (or the default accent hex) stays warm
        // red under the other ~20 GUI palettes.
        Assert.DoesNotContain("C7625A", Xaml, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("C7625A", Styles, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotMatch(@"#(?:[0-9A-Fa-f]{8})(?![0-9A-Fa-f])", Xaml);
        Assert.DoesNotMatch(@"#(?:[0-9A-Fa-f]{8})(?![0-9A-Fa-f])", Styles);

        Assert.Contains("Value=\"{DynamicResource AccentBrush}\"", Styles);
        Assert.Contains("Property=\"Opacity\" Value=\"0.06\"", Xaml);
        Assert.Contains("Property=\"Opacity\" Value=\"0.12\"", Xaml);

        // The same two washes carry the Servers row, so both tabs read as one app.
        Assert.Contains("Property=\"Opacity\" Value=\"0.06\"", ServersXaml);
        Assert.Contains("Property=\"Opacity\" Value=\"0.12\"", ServersXaml);
    }

    [Fact]
    public void Xaml_DisabledButtonsExplainThemselvesAndKeepAVisibleLine()
    {
        // Both shared button styles and each view's primary action must surface a
        // reason: ShowOnDisabled for the tooltip, plus a persistent footer line.
        Assert.Contains("ToolTipService.ShowOnDisabled", Styles);
        Assert.Contains("x:Name=\"PrimaryHint\"", Xaml);
        Assert.Contains("AccountsViewLogic.PrimaryHint", CodeBehind);
    }

    [Fact]
    public void Xaml_DefinesNoDuplicateButtonStyles()
    {
        Assert.DoesNotContain("x:Key=\"PrimaryButton\"", Xaml);
        Assert.DoesNotContain("x:Key=\"GhostButton\"", Xaml);
        Assert.DoesNotContain("x:Key=\"DangerButton\"", Xaml);
        Assert.DoesNotContain("x:Key=\"TabPrimaryButton\"", Xaml);
        Assert.DoesNotContain("x:Key=\"TabSecondaryButton\"", Xaml);
        Assert.DoesNotContain("x:Key=\"TabDangerButton\"", Xaml);

        Assert.Contains("x:Key=\"TabPrimaryButton\"", Styles);
        Assert.Contains("x:Key=\"TabSecondaryButton\"", Styles);
        Assert.Contains("x:Key=\"TabDangerButton\"", Styles);
    }

    private sealed class TempDirectory : IDisposable
    {
        public TempDirectory()
        {
            Path = System.IO.Path.Combine(
                System.IO.Path.GetTempPath(),
                "aoko-accounts-view-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path);
        }

        public string Path { get; }

        public void Dispose()
        {
            try
            {
                Directory.Delete(Path, recursive: true);
            }
            catch (IOException)
            {
                // Test cleanup only.
            }
            catch (UnauthorizedAccessException)
            {
                // Test cleanup only.
            }
        }
    }
}
