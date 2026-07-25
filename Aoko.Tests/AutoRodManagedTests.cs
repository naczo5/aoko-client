using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

public class AutoRodManagedTests
{
    private static readonly string RepoRoot = FindRepoRoot();

    [Fact]
    public void ModelAndProfile_DefaultsClampAndAllowVerificationOverride()
    {
        var defaults = new Profile();
        var profile = new Profile
        {
            AutoRodSlotMode = 42,
            AutoRodVerifyForcedSlot = false,
            AutoRodExtensionTicks = 99,
            AutoRodHoldToExtend = true
        };

        Assert.False(defaults.AutoRodEnabled);
        Assert.True(defaults.AutoRodVerifyForcedSlot);
        Assert.Equal(4, defaults.AutoRodExtensionTicks);
        Assert.False(defaults.AutoRodHoldToExtend);
        Assert.Equal(9, profile.AutoRodSlotMode);
        Assert.False(profile.AutoRodVerifyForcedSlot);
        Assert.Equal(40, profile.AutoRodExtensionTicks);
        Assert.True(profile.AutoRodHoldToExtend);
        Assert.Equal(0, profile.AutoRodActionKey);
        Assert.Equal(0, profile.ModuleKeys["autorod"]);

        Clicker clicker = Clicker.Instance;
        int savedSlot = clicker.AutoRodSlotMode;
        bool savedEnabled = clicker.AutoRodEnabled;
        bool savedVerify = clicker.AutoRodVerifyForcedSlot;
        int savedExtensionTicks = clicker.AutoRodExtensionTicks;
        bool savedHoldToExtend = clicker.AutoRodHoldToExtend;
        try
        {
            clicker.AutoRodEnabled = false;
            clicker.AutoRodSlotMode = -5;
            clicker.AutoRodVerifyForcedSlot = false;
            clicker.AutoRodExtensionTicks = 0;
            clicker.AutoRodHoldToExtend = true;
            Assert.False(clicker.AutoRodEnabled);
            Assert.Equal(0, clicker.AutoRodSlotMode);
            Assert.False(clicker.AutoRodVerifyForcedSlot);
            Assert.Equal(1, clicker.AutoRodExtensionTicks);
            Assert.True(clicker.AutoRodHoldToExtend);
            Assert.False(clicker.AutoRodUsesFixedExtension);
        }
        finally
        {
            clicker.AutoRodSlotMode = savedSlot;
            clicker.AutoRodEnabled = savedEnabled;
            clicker.AutoRodVerifyForcedSlot = savedVerify;
            clicker.AutoRodExtensionTicks = savedExtensionTicks;
            clicker.AutoRodHoldToExtend = savedHoldToExtend;
        }
    }

    [Fact]
    public void Profile_RoundTripPreservesAutoRodSettingsAndActionBind()
    {
        var profile = new Profile
        {
            AutoRodEnabled = true,
            AutoRodSlotMode = 7,
            AutoRodVerifyForcedSlot = false,
            AutoRodExtensionTicks = 12,
            AutoRodHoldToExtend = true,
            AutoRodActionKey = 0x05
        };
        var options = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };

        string json = JsonSerializer.Serialize(profile, options);
        Profile? restored = JsonSerializer.Deserialize<Profile>(json, options);

        Assert.NotNull(restored);
        Assert.True(restored!.AutoRodEnabled);
        Assert.Equal(7, restored.AutoRodSlotMode);
        Assert.False(restored.AutoRodVerifyForcedSlot);
        Assert.Equal(12, restored.AutoRodExtensionTicks);
        Assert.True(restored.AutoRodHoldToExtend);
        Assert.Equal(0x05, restored.AutoRodActionKey);
    }

    [Fact]
    public void BindConflictsRejectBothDirections()
    {
        int savedGeneral = InputHooks.GetModuleKey("autorod");
        int savedAction = InputHooks.AutoRodActionKey;
        try
        {
            InputHooks.SetAutoRodActionKey(0);
            Assert.True(InputHooks.SetModuleKey("autorod", 0x52));
            Assert.False(InputHooks.SetAutoRodActionKey(0x52));
            Assert.Equal(0, InputHooks.AutoRodActionKey);

            Assert.True(InputHooks.SetModuleKey("autorod", 0));
            Assert.True(InputHooks.SetAutoRodActionKey(0x52));
            Assert.False(InputHooks.SetModuleKey("autorod", 0x52));
            Assert.Equal(0, InputHooks.GetModuleKey("autorod"));
        }
        finally
        {
            InputHooks.SetAutoRodActionKey(0);
            InputHooks.SetModuleKey("autorod", savedGeneral);
            InputHooks.SetAutoRodActionKey(savedAction);
        }
    }

    [Fact]
    public void ProfileConflict_PreservesGeneralBindAndClearsActionBind()
    {
        Profile snapshot = ProfileManager.CreateFromClicker();
        try
        {
            var profile = new Profile { AutoRodActionKey = 0x52 };
            profile.ModuleKeys["autorod"] = 0x52;

            ProfileManager.ApplyToClicker(profile);

            Assert.Equal(0x52, InputHooks.GetModuleKey("autorod"));
            Assert.Equal(0, InputHooks.AutoRodActionKey);
            Assert.Equal(0, profile.AutoRodActionKey);
        }
        finally
        {
            ProfileManager.ApplyToClicker(snapshot);
        }
    }

    [Fact]
    public void ActionLatch_TriggersOnceUntilReleaseAndKeepsConsumptionPaired()
    {
        var latch = new InputHooks.PressLatch();

        Assert.True(latch.Begin(canConsume: true, out bool firstTrigger));
        Assert.True(firstTrigger);
        Assert.True(latch.Begin(canConsume: true, out bool repeatTrigger));
        Assert.False(repeatTrigger);
        Assert.True(latch.End());
        Assert.True(latch.Begin(canConsume: true, out bool secondTrigger));
        Assert.True(secondTrigger);

        var passThrough = new InputHooks.PressLatch();
        Assert.False(passThrough.Begin(canConsume: false, out bool blockedTrigger));
        Assert.False(blockedTrigger);
        Assert.False(passThrough.End());
    }

    [Theory]
    [InlineData(true, true, true, true, true, false, true)]
    [InlineData(false, true, true, true, true, false, false)]
    [InlineData(true, false, true, true, true, false, false)]
    [InlineData(true, true, false, true, true, false, false)]
    [InlineData(true, true, true, false, true, false, false)]
    [InlineData(true, true, true, true, false, false, false)]
    [InlineData(true, true, true, true, true, true, false)]
    public void ActionConsumption_RequiresEveryRuntimeGate(
        bool enabled, bool supported, bool connected, bool foreground,
        bool inWorld, bool anyScreenOpen, bool expected)
    {
        Assert.Equal(expected, InputHooks.ShouldConsumeAutoRodAction(
            enabled, supported, connected, foreground, inWorld, anyScreenOpen));
    }

    [Theory]
    [InlineData(false, "", false)]
    [InlineData(false, "none", false)]
    [InlineData(false, "unknown", false)]
    [InlineData(false, "ChatScreen", true)]
    [InlineData(false, "net.minecraft.client.gui.screens.ChatScreen|Screen", true)]
    [InlineData(true, "", true)]
    public void AnyGameScreenDetection_BlocksChatEvenWhenGuiFlagIsFalse(
        bool guiOpen, string screenName, bool expected)
    {
        var state = new GameState { GuiOpen = guiOpen, ScreenName = screenName };
        Assert.Equal(expected, InputHooks.IsAnyGameScreenOpen(state));
    }

    [Fact]
    public void ActionMessage_IsNewlineDelimitedAndHasExactContract()
    {
        string message = GameStateClient.BuildAutoRodActionMessage(12, true, 99, true);
        Assert.EndsWith("\n", message, StringComparison.Ordinal);

        JsonNode? node = JsonNode.Parse(message);
        Assert.Equal("moduleAction", node!["type"]!.GetValue<string>());
        Assert.Equal("autoRod", node["action"]!.GetValue<string>());
        Assert.Equal("press", node["phase"]!.GetValue<string>());
        Assert.True(node["enabled"]!.GetValue<bool>());
        Assert.Equal(9, node["slotMode"]!.GetValue<int>());
        Assert.True(node["verifyForcedSlot"]!.GetValue<bool>());
        Assert.Equal(40, node["extensionTicks"]!.GetValue<int>());
        Assert.True(node["holdToExtend"]!.GetValue<bool>());

        string release = GameStateClient.BuildAutoRodReleaseMessage();
        Assert.EndsWith("\n", release, StringComparison.Ordinal);
        JsonNode? releaseNode = JsonNode.Parse(release);
        Assert.Equal("moduleAction", releaseNode!["type"]!.GetValue<string>());
        Assert.Equal("autoRod", releaseNode["action"]!.GetValue<string>());
        Assert.Equal("release", releaseNode["phase"]!.GetValue<string>());
        Assert.Null(releaseNode["enabled"]);
    }

    [Fact]
    public void ManagedWiring_ContainsConfigFieldsSharedLockAndBothUiBinds()
    {
        string client = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "Core", "GameStateClient.cs"));
        string clicker = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "Core", "Clicker.cs"));
        string hooks = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "Core", "InputHooks.cs"));
        string windowCode = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "MainWindow.xaml.cs"));
        string xaml = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "MainWindow.xaml"));

        Assert.Contains("autoRodEnabled = clicker.AutoRodEnabled", client, StringComparison.Ordinal);
        Assert.Contains("autoRodSlotMode = clicker.AutoRodSlotMode", client, StringComparison.Ordinal);
        Assert.Contains("autoRodVerifyForcedSlot = clicker.AutoRodVerifyForcedSlot", client, StringComparison.Ordinal);
        Assert.Contains("autoRodExtensionTicks = clicker.AutoRodExtensionTicks", client, StringComparison.Ordinal);
        Assert.Contains("autoRodHoldToExtend = clicker.AutoRodHoldToExtend", client, StringComparison.Ordinal);
        Assert.Contains("SendAutoRodReleaseAsync", hooks, StringComparison.Ordinal);
        Assert.Contains("() => Clicker.Instance.AutoRodEnabled", client, StringComparison.Ordinal);
        Assert.Contains("state.InWorld", hooks, StringComparison.Ordinal);
        Assert.Contains("_sendLock.WaitAsync", client, StringComparison.Ordinal);
        Assert.Contains("AutoRodEnabled = false;", clicker, StringComparison.Ordinal);
        Assert.Contains("case \"autorod\":          c.AutoRodEnabled = !c.AutoRodEnabled; break;", hooks, StringComparison.Ordinal);
        Assert.Contains("WM_LBUTTONDOWN", hooks, StringComparison.Ordinal);
        Assert.Contains("WM_RBUTTONDOWN", hooks, StringComparison.Ordinal);
        Assert.Contains("WM_MBUTTONDOWN", hooks, StringComparison.Ordinal);
        Assert.Contains("WM_XBUTTONDOWN", hooks, StringComparison.Ordinal);
        Assert.Contains("VK_XBUTTON1", hooks, StringComparison.Ordinal);
        Assert.Contains("VK_XBUTTON2", hooks, StringComparison.Ordinal);
        Assert.Contains("vkCode == 0x1B", windowCode, StringComparison.Ordinal);
        Assert.Contains("x:Name=\"AutoRodCard\"", xaml, StringComparison.Ordinal);
        Assert.Contains("x:Name=\"AutoRodActionBindButton\"", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoRodExtensionTicks", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoRodHoldToExtend", xaml, StringComparison.Ordinal);
        Assert.Contains("x:Name=\"KeybindAutoRodButton\" Tag=\"autorod\"", xaml, StringComparison.Ordinal);
    }

    [Fact]
    public void NativeWiring_UsesYarnMainHandAndOnlyQueuesEnabledActions()
    {
        string legacy = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge.cpp"));
        string modern = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge_261.cpp"));

        Assert.Contains("\"syncCurrentPlayItem\", \"func_78765_e\"", legacy, StringComparison.Ordinal);
        Assert.Contains("reader.GetString(\"action\") == \"autoRod\"", legacy, StringComparison.Ordinal);
        Assert.Contains("phase == \"release\"", legacy, StringComparison.Ordinal);
        Assert.Contains("phase == \"release\"", modern, StringComparison.Ordinal);
        Assert.Contains("else if (reader.GetBool(\"enabled\"))", legacy, StringComparison.Ordinal);
        Assert.Contains("else if (reader.GetBool(\"enabled\"))", modern, StringComparison.Ordinal);
        Assert.Contains("autorod::ShouldRestoreAfterUse", legacy, StringComparison.Ordinal);
        Assert.Contains("autorod::ShouldRestoreAfterUse", modern, StringComparison.Ordinal);
        Assert.Contains("\"field_5808\", \"MAIN_HAND\"", modern, StringComparison.Ordinal);
        Assert.Contains("ScreenChainContainsClass121(sn, \"ChatScreen\")", modern, StringComparison.Ordinal);
        Assert.Contains("SendAutoRodUseInputLegacy", legacy, StringComparison.Ordinal);
        Assert.Contains("SendAutoRodUseInput121", modern, StringComparison.Ordinal);
        Assert.Contains("autorod::kRestoreSettleTicks", legacy, StringComparison.Ordinal);
        Assert.Contains("autorod::kRestoreSettleTicks", modern, StringComparison.Ordinal);
        Assert.DoesNotContain("CallBooleanMethod(\n                controller, g_autoRodSendUseItem18", legacy, StringComparison.Ordinal);
        Assert.DoesNotContain("gameMode, g_autoRodUseItem121, player, g_autoRodMainHand121", modern, StringComparison.Ordinal);
        Assert.DoesNotContain("CallVoidMethod(controller, g_autoRodSyncCurrentPlayItem18)", legacy, StringComparison.Ordinal);
        Assert.DoesNotContain("CallVoidMethod(gameMode, g_autoRodSyncSelectedSlot121)", modern, StringComparison.Ordinal);
    }

    private static string FindRepoRoot()
    {
        string? dir = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
        while (!string.IsNullOrEmpty(dir))
        {
            if (File.Exists(Path.Combine(dir, "Aoko", "MainWindow.xaml")))
                return dir;
            dir = Directory.GetParent(dir)?.FullName;
        }

        string cwd = Directory.GetCurrentDirectory();
        if (File.Exists(Path.Combine(cwd, "Aoko", "MainWindow.xaml")))
            return cwd;
        throw new InvalidOperationException("Could not locate repository root.");
    }
}
