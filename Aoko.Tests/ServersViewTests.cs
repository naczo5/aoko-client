using System;
using System.Text.RegularExpressions;
using Aoko.Core.Servers;
using Aoko.Views;

namespace Aoko.Tests;

/// <summary>
/// The Servers tab's decisions live in <see cref="ServersViewLogic"/> and
/// <see cref="SavedServerRow"/> so they can be checked without WPF: ordering,
/// selection retention, chip wording and tone, the join gate, and the fact that
/// pings are allowed to run alongside the exclusive operations.
/// </summary>
public class ServersViewTests
{
    private static readonly string Xaml = TestRepo.Read("Aoko", "Views", "ServersView.xaml");
    private static readonly string CodeBehind = TestRepo.Read("Aoko", "Views", "ServersView.xaml.cs");

    private static SavedServer Server(
        string id,
        string name,
        string address,
        DateTimeOffset? joined = null,
        DateTimeOffset? added = null)
        => new()
        {
            Id = id,
            Name = name,
            Address = address,
            LastJoinedAt = joined,
            AddedAt = added ?? new DateTimeOffset(2024, 1, 1, 0, 0, 0, TimeSpan.Zero)
        };

    [Fact]
    public void SortForDisplay_PutsMostRecentlyJoinedFirstAndNeverJoinedLast()
    {
        DateTimeOffset now = DateTimeOffset.UtcNow;
        SavedServer older = Server("1", "Older", "older.example.com", now.AddDays(-5));
        SavedServer never = Server("2", "Never", "never.example.com");
        SavedServer newer = Server("3", "Newer", "newer.example.com", now.AddHours(-2));

        List<SavedServer> ordered = ServersViewLogic.SortForDisplay(new[] { older, never, newer });

        Assert.Equal(new[] { "3", "1", "2" }, ordered.Select(server => server.Id));
    }

    [Fact]
    public void SortForDisplay_FallsBackToAddedAtThenName()
    {
        SavedServer oldest = Server("a", "Zulu", "z.example.com", added: new DateTimeOffset(2024, 1, 1, 0, 0, 0, TimeSpan.Zero));
        SavedServer newest = Server("b", "Alpha", "a.example.com", added: new DateTimeOffset(2024, 2, 1, 0, 0, 0, TimeSpan.Zero));
        SavedServer twin = Server("c", "Beta", "b.example.com", added: new DateTimeOffset(2024, 2, 1, 0, 0, 0, TimeSpan.Zero));

        List<SavedServer> ordered = ServersViewLogic.SortForDisplay(new[] { oldest, twin, newest });

        Assert.Equal(new[] { "b", "c", "a" }, ordered.Select(server => server.Id));
    }

    [Fact]
    public void ResolveSelectionIndex_KeepsTheSelectedIdAcrossReloads()
    {
        string[] ids = { "one", "two", "three" };

        Assert.Equal(2, ServersViewLogic.ResolveSelectionIndex(ids, "three", fallbackIndex: 0));
        Assert.Equal(0, ServersViewLogic.ResolveSelectionIndex(ids, "one", fallbackIndex: 2));
    }

    [Fact]
    public void ResolveSelectionIndex_FallsBackToTheNeighbourIndexWhenTheIdIsGone()
    {
        string[] ids = { "one", "three" };

        Assert.Equal(1, ServersViewLogic.ResolveSelectionIndex(ids, "two", fallbackIndex: 1));
        Assert.Equal(1, ServersViewLogic.ResolveSelectionIndex(ids, "two", fallbackIndex: 9));
        Assert.Equal(0, ServersViewLogic.ResolveSelectionIndex(ids, null, fallbackIndex: -1));
    }

    [Fact]
    public void ResolveSelectionIndex_ReturnsNothingForAnEmptyList()
        => Assert.Equal(-1, ServersViewLogic.ResolveSelectionIndex(Array.Empty<string>(), "one", fallbackIndex: 3));

    [Theory]
    [InlineData(true, true, ServerJoinState.Ready)]
    [InlineData(true, false, ServerJoinState.Unsupported)]
    [InlineData(false, true, ServerJoinState.Disconnected)]
    [InlineData(false, false, ServerJoinState.Disconnected)]
    public void EvaluateJoinState_NeedsBothTheConnectionAndTheBridgeAction(
        bool isConnected,
        bool supportsJoin,
        ServerJoinState expected)
        => Assert.Equal(expected, ServersViewLogic.EvaluateJoinState(isConnected, supportsJoin));

    [Fact]
    public void JoinBlockedMessage_IsEmptyOnlyWhenTheJoinIsPossible()
    {
        Assert.Equal("", ServersViewLogic.JoinBlockedMessage(ServerJoinState.Ready));
        Assert.Equal(TabCopy.UnsupportedServerJoin, ServersViewLogic.JoinBlockedMessage(ServerJoinState.Unsupported));
        Assert.Equal(TabCopy.NotConnected, ServersViewLogic.JoinBlockedMessage(ServerJoinState.Disconnected));
        Assert.DoesNotContain("Rebuild", ServersViewLogic.JoinBlockedMessage(ServerJoinState.Unsupported));
    }

    [Fact]
    public void PrimaryHint_ExplainsADisabledJoin()
    {
        Assert.Equal(
            TabCopy.UnsupportedServerJoin,
            ServersViewLogic.PrimaryHint(ServerJoinState.Unsupported, hasRows: true, hasSelection: true));
        Assert.Equal(
            TabCopy.NotConnected,
            ServersViewLogic.PrimaryHint(ServerJoinState.Disconnected, hasRows: true, hasSelection: true));
        Assert.Equal(
            TabCopy.NoServersHint,
            ServersViewLogic.PrimaryHint(ServerJoinState.Ready, hasRows: false, hasSelection: false));
        Assert.Equal(
            TabCopy.SelectServerHint,
            ServersViewLogic.PrimaryHint(ServerJoinState.Ready, hasRows: true, hasSelection: false));
        Assert.Equal(
            "",
            ServersViewLogic.PrimaryHint(ServerJoinState.Ready, hasRows: true, hasSelection: true));
    }

    // ----- Gates: pings are independent of the exclusive lock -----------------

    [Theory]
    [InlineData(false, true, true, true, true)]
    [InlineData(true, false, true, true, false)]
    [InlineData(false, true, false, true, false)]
    [InlineData(false, true, true, false, false)]
    public void CanJoinSelected_OnlyDependsOnTheExclusiveLock(
        bool busy,
        bool hasSelection,
        bool connected,
        bool supported,
        bool expected)
    {
        ServerJoinState state = ServersViewLogic.EvaluateJoinState(connected, supported);

        Assert.Equal(expected, ServersViewLogic.CanJoinSelected(busy, state, hasSelection));
    }

    [Fact]
    public void PingsNeverShareTheExclusiveLock()
    {
        // CanStartPing has no busy parameter on purpose: a ping pass and a join/save/
        // delete can run at the same time, and only another ping pass blocks a ping.
        Assert.True(ServersViewLogic.CanStartPing(pingInFlight: false, rows: 3));
        Assert.False(ServersViewLogic.CanStartPing(pingInFlight: true, rows: 3));
        Assert.False(ServersViewLogic.CanStartPing(pingInFlight: false, rows: 0));

        Assert.True(ServersViewLogic.CanStartMutation(busy: false));
        Assert.False(ServersViewLogic.CanStartMutation(busy: true));
    }

    [Fact]
    public void ValidationMessage_PassesTheServiceTextThrough()
    {
        ServerTarget invalid = ServerJoinService.ValidateTarget("   ");
        Assert.False(invalid.Ok);
        Assert.Equal(ServerJoinService.InvalidAddressMessage, ServersViewLogic.ValidationMessage(invalid));

        ServerTarget valid = ServerJoinService.ValidateTarget("play.example.com:25566");
        Assert.True(valid.Ok);
        Assert.Equal("", ServersViewLogic.ValidationMessage(valid));
        Assert.Equal("play.example.com:25566", valid.Address.Display);
    }

    [Fact]
    public void JoinStartedText_NamesTheResolvedTarget()
    {
        ServerAddress withPort = ServerJoinService.ValidateTarget("play.example.com:25566").Address;
        Assert.Equal("Joining play.example.com:25566.", ServersViewLogic.JoinStartedText(withPort));

        ServerAddress defaultPort = ServerJoinService.ValidateTarget("play.example.com").Address;
        Assert.Equal("Joining play.example.com.", ServersViewLogic.JoinStartedText(defaultPort));
    }

    [Fact]
    public void PingSummaryText_StaysCalmForOneServer()
    {
        Assert.Equal("", ServersViewLogic.PingSummaryText(0, 0));
        Assert.Equal("The server answered the ping.", ServersViewLogic.PingSummaryText(1, 1));
        Assert.Equal("The server did not answer the ping.", ServersViewLogic.PingSummaryText(1, 0));
        Assert.Equal("Pinged 3 servers: 2 online, 1 offline.", ServersViewLogic.PingSummaryText(3, 2));
    }

    [Fact]
    public void MotdText_CollapsesWhitespaceAndCapsTheLength()
    {
        Assert.Equal("", ServersViewLogic.MotdText(null));
        Assert.Equal("", ServersViewLogic.MotdText("   "));
        Assert.Equal("Welcome to the server", ServersViewLogic.MotdText("  Welcome\nto\t the \r\n server "));

        string longMotd = new string('a', ServersViewLogic.MaximumMotdLength + 40);
        Assert.Equal(
            ServersViewLogic.MaximumMotdLength,
            ServersViewLogic.MotdText(longMotd).Length);
    }

    [Fact]
    public void LastJoinedLabel_ReadsAsASentence()
    {
        Assert.Equal("Not joined yet", ServersViewLogic.LastJoinedLabel(null));
        Assert.Equal(
            "Last joined 3d ago",
            ServersViewLogic.LastJoinedLabel(DateTimeOffset.UtcNow.AddDays(-3).AddMinutes(-30)));
    }

    // ----- Chip wording: no raw exception names, one voice --------------------

    [Theory]
    [InlineData("", "Offline")]
    [InlineData("Offline", "Offline")]
    [InlineData("Timed out", "No response")]
    [InlineData("Timeout", "No response")]
    [InlineData("Cancelled", "Not pinged")]
    [InlineData("Ping failed.", "Ping failed")]
    [InlineData("Unexpected ping response.", "Ping failed")]
    [InlineData("SocketException", "Ping failed")]
    [InlineData("IOException", "Ping failed")]
    [InlineData("JsonException", "Ping failed")]
    [InlineData("Enter a server as host or host:port.", "Invalid address")]
    public void ErrorLabel_NeverLeaksAnExceptionTypeName(string? error, string expected)
        => Assert.Equal(expected, ServersViewLogic.ErrorLabel(error));

    [Fact]
    public void ErrorLabel_HasNoTerminalPunctuation()
    {
        foreach (string error in new[] { "Timed out", "Cancelled", "Ping failed.", "Unexpected ping response." })
            Assert.DoesNotContain(".", ServersViewLogic.ErrorLabel(error));
    }

    [Theory]
    [InlineData(true, 23, ServerStatusTone.Accent)]
    [InlineData(true, 99, ServerStatusTone.Accent)]
    [InlineData(true, 150, ServerStatusTone.Warn)]
    [InlineData(true, 300, ServerStatusTone.Warn)]
    [InlineData(true, 480, ServerStatusTone.Critical)]
    [InlineData(false, 0, ServerStatusTone.Critical)]
    public void PingTone_FollowsLatencyAndFailure(bool online, int latency, ServerStatusTone expected)
        => Assert.Equal(expected, ServersViewLogic.PingTone(online, latency));

    [Fact]
    public void PingLabel_IsLatencyForAnAnswerAndAPhraseOtherwise()
    {
        Assert.Equal("24 ms", ServersViewLogic.PingLabel(new ServerPingResult { Online = true, LatencyMs = 24 }));
        Assert.Equal("No response", ServersViewLogic.PingLabel(new ServerPingResult { Error = "Timed out" }));
    }

    // ----- Row behaviour ------------------------------------------------------

    [Fact]
    public void SavedServerRow_AppliesAnOnlinePing()
    {
        var row = new SavedServerRow(Server("one", "Hub", "hub.example.com:25566"));
        Assert.Equal(SavedServerRow.NotPingedLabel, row.StatusText);
        Assert.Equal(ServerStatusTone.Neutral, row.StatusTone);
        Assert.False(row.HasPlayers);
        Assert.False(row.HasMotd);

        row.ApplyPing(new ServerPingResult
        {
            Online = true,
            LatencyMs = 23,
            OnlinePlayers = 12,
            MaxPlayers = 100,
            Description = "A quiet\nlobby"
        });

        Assert.Equal("23 ms", row.StatusText);
        Assert.Equal(ServerStatusTone.Accent, row.StatusTone);
        Assert.Equal("12/100", row.PlayersLabel);
        Assert.True(row.HasPlayers);
        Assert.Equal("A quiet lobby", row.Motd);
    }

    [Fact]
    public void SavedServerRow_AppliesAnOfflinePing()
    {
        var row = new SavedServerRow(Server("one", "Hub", "hub.example.com"));
        row.ApplyPing(new ServerPingResult { Online = true, LatencyMs = 23, OnlinePlayers = 12, MaxPlayers = 100 });

        row.ApplyPing(new ServerPingResult { Error = "Timed out" });

        Assert.Equal("No response", row.StatusText);
        Assert.Equal(ServerStatusTone.Critical, row.StatusTone);
        Assert.Equal("", row.PlayersLabel);
        Assert.False(row.HasPlayers);
    }

    [Fact]
    public void SavedServerRow_FallsBackToOfflineWhenTheBridgeSaysNothing()
    {
        var row = new SavedServerRow(Server("one", "Hub", "hub.example.com"));

        row.ApplyPingFailure("");

        Assert.Equal("Offline", row.StatusText);
        Assert.Equal(ServerStatusTone.Critical, row.StatusTone);
    }

    [Fact]
    public void SavedServerRow_MarksAnUnparseableAddress()
    {
        var row = new SavedServerRow(Server("one", "Hub", "not an address"));

        row.ApplyInvalidAddress();

        Assert.Equal(ServersViewLogic.InvalidAddressChipLabel, row.StatusText);
        Assert.Equal(ServerStatusTone.Critical, row.StatusTone);
    }

    [Fact]
    public void SavedServerRow_CarriesTheChipThroughAPingingResetAndAReload()
    {
        var row = new SavedServerRow(Server("one", "Hub", "hub.example.com"));

        row.MarkPinging();
        Assert.Equal(SavedServerRow.PingingLabel, row.StatusText);

        row.ApplyPing(new ServerPingResult { Online = true, LatencyMs = 41, OnlinePlayers = 3, MaxPlayers = 20 });
        var reloaded = new SavedServerRow(Server("one", "Hub", "hub.example.com"));
        reloaded.CopyPingStateFrom(row);

        Assert.Equal("41 ms", reloaded.StatusText);
        Assert.Equal(ServerStatusTone.Accent, reloaded.StatusTone);
        Assert.Equal("3/20", reloaded.PlayersLabel);

        reloaded.MarkNotPinged();
        Assert.Equal(SavedServerRow.NotPingedLabel, reloaded.StatusText);
        Assert.Equal(ServerStatusTone.Neutral, reloaded.StatusTone);
        Assert.False(reloaded.HasPlayers);
    }

    [Fact]
    public void SavedServerRow_AnnouncesSelection()
    {
        var row = new SavedServerRow(Server("one", "Hub", "hub.example.com"));
        Assert.Equal("", row.SelectionStatus);

        row.IsSelected = true;
        Assert.Equal("selected", row.SelectionStatus);
    }

    [Fact]
    public void SavedServerRow_UsesTheAddressWhenTheNameIsBlank()
    {
        var row = new SavedServerRow(Server("one", "  ", "hub.example.com:25566"));

        Assert.Equal("hub.example.com:25566", row.Name);
        Assert.Equal("hub.example.com:25566", row.Address);
    }

    // ----- XAML / code-behind contract --------------------------------------

    [Theory]
    [InlineData("ServersList")]
    [InlineData("DirectAddressBox")]
    [InlineData("DirectJoinButton")]
    [InlineData("DirectValidationText")]
    [InlineData("EmptyState")]
    [InlineData("AddFormCard")]
    [InlineData("AddNameBox")]
    [InlineData("AddAddressBox")]
    [InlineData("AddFormAddButton")]
    [InlineData("AddFormCancelButton")]
    [InlineData("AddFormValidationText")]
    [InlineData("BusyBar")]
    [InlineData("StatusLine")]
    [InlineData("RecoveryText")]
    [InlineData("PrimaryHint")]
    [InlineData("JoinSelectedButton")]
    [InlineData("AddServerButton")]
    [InlineData("PingAllButton")]
    [InlineData("DeleteButton")]
    public void Xaml_DeclaresEveryNamedElementTheCodeTouches(string name)
        => Assert.Contains($"x:Name=\"{name}\"", Xaml);

    [Fact]
    public void Xaml_HandlersAllExistInTheCodeBehind()
    {
        MatchCollection handlers = Regex.Matches(
            Xaml,
            @"(?<![\w:])(?:Click|DragEnter|DragOver|DragLeave|Drop|TextChanged|KeyDown|MouseLeftButtonDown|MouseDoubleClick|GotKeyboardFocus|SelectionChanged)=""([A-Za-z_][A-Za-z0-9_]*)""");

        Assert.NotEmpty(handlers);
        foreach (Match handler in handlers)
        {
            string signature = $"void {handler.Groups[1].Value}(";
            Assert.Contains(signature, CodeBehind);
        }
    }

    [Fact]
    public void Xaml_KeepsTheRowAndChipAccessible()
    {
        Assert.Contains("KeyboardNavigation.TabNavigation=\"Once\"", Xaml);
        Assert.Contains("AutomationProperties.Name=\"{Binding AutomationName}\"", Xaml);
        Assert.Contains("Property=\"AutomationProperties.Name\" Value=\"Ping again\"", Xaml);
        Assert.Contains("MouseDoubleClick=\"ServerCard_DoubleClick\"", Xaml);
        Assert.Contains("Property=\"MaxWidth\" Value=\"110\"", Xaml);
        Assert.Contains("TextTrimming=\"CharacterEllipsis\"", Xaml);
    }

    [Fact]
    public void Xaml_DropsTheDuplicateSaveAndPrimaryStyle()
    {
        Assert.DoesNotContain("DirectSaveButton", Xaml);
        Assert.DoesNotContain("ServerPrimaryButton", Xaml);
        Assert.DoesNotContain("ServerSecondaryButton", Xaml);
        Assert.DoesNotContain("ServerDangerButton", Xaml);
        Assert.DoesNotContain("BasedOn=\"{StaticResource DarkButton}\"", Xaml);
        Assert.DoesNotContain("Rebuild the bridge DLLs", Xaml);
        Assert.DoesNotContain("Rebuild the bridge DLLs", CodeBehind);
    }

    [Fact]
    public void Xaml_ShowsTheSharedSelectionWashAndRule()
    {
        Assert.Contains("Style=\"{StaticResource TabCardWash}\"", Xaml);
        Assert.Contains("Style=\"{StaticResource TabSelectionRule}\"", Xaml);
        Assert.Contains("Opacity\" Value=\"0.06\"", Xaml);
        Assert.Contains("Opacity\" Value=\"0.12\"", Xaml);
    }
}
