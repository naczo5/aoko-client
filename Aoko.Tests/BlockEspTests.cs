using System.Linq;
using System.Text.Json;
using Aoko.Core;

namespace Aoko.Tests;

public class BlockEspTests
{
    // ── Id normalization ─────────────────────────────────────────────────────────

    [Theory]
    [InlineData("minecraft:diamond_ore", "diamond_ore")]
    [InlineData("diamond_ore", "diamond_ore")]
    [InlineData("block.minecraft.diamond_ore", "diamond_ore")]
    [InlineData("  Minecraft:Diamond_Ore  ", "diamond_ore")]
    [InlineData("modid:custom_block", "custom_block")]
    [InlineData("", "")]
    [InlineData("   ", "")]
    public void NormalizeId_ReducesToPathToken(string input, string expected)
    {
        Assert.Equal(expected, BlockEspConfig.NormalizeId(input));
    }

    // ── Color normalization ──────────────────────────────────────────────────────

    [Theory]
    [InlineData("#00e5ff", "00E5FF")]
    [InlineData("00E5FF", "00E5FF")]
    [InlineData("ffd700", "FFD700")]
    [InlineData("xyz", "00E5FF")]   // invalid -> default
    [InlineData("12345", "00E5FF")] // wrong length -> default
    [InlineData(null, "00E5FF")]
    public void NormalizeColor_ProducesSixDigitHexOrDefault(string? input, string expected)
    {
        Assert.Equal(expected, BlockEspConfig.NormalizeColor(input));
    }

    // ── Serialize / Parse ────────────────────────────────────────────────────────

    [Fact]
    public void Serialize_OnlyIncludesEnabledEntries()
    {
        var targets = new[]
        {
            new BlockEspTarget("minecraft:diamond_ore", "Diamond Ore", "00E5FF", true),
            new BlockEspTarget("minecraft:gold_ore", "Gold Ore", "FFD700", false),
            new BlockEspTarget("minecraft:iron_ore", "Iron Ore", "D8AF93", true),
        };

        string s = BlockEspConfig.Serialize(targets);

        Assert.Equal("diamond_ore=00E5FF;iron_ore=D8AF93", s);
    }

    [Fact]
    public void Serialize_DedupesByNormalizedId()
    {
        var targets = new[]
        {
            new BlockEspTarget("minecraft:diamond_ore", "A", "00E5FF", true),
            new BlockEspTarget("diamond_ore", "B", "FFFFFF", true),
        };

        string s = BlockEspConfig.Serialize(targets);

        Assert.Equal("diamond_ore=00E5FF", s);
    }

    [Fact]
    public void SerializeParse_RoundTrips()
    {
        var targets = new[]
        {
            new BlockEspTarget("minecraft:diamond_ore", "Diamond Ore", "00E5FF", true),
            new BlockEspTarget("minecraft:ancient_debris", "Ancient Debris", "9B6A4A", true),
        };

        string s = BlockEspConfig.Serialize(targets);
        var parsed = BlockEspConfig.Parse(s);

        Assert.Equal(2, parsed.Count);
        Assert.Equal(("diamond_ore", "00E5FF"), parsed[0]);
        Assert.Equal(("ancient_debris", "9B6A4A"), parsed[1]);
    }

    [Fact]
    public void Parse_SkipsMalformedEntries()
    {
        var parsed = BlockEspConfig.Parse("diamond_ore=00E5FF;;garbage;=AABBCC;gold_ore=FFD700");

        Assert.Equal(2, parsed.Count);
        Assert.Equal("diamond_ore", parsed[0].Id);
        Assert.Equal("gold_ore", parsed[1].Id);
    }

    [Fact]
    public void Serialize_CapsLength()
    {
        var targets = Enumerable.Range(0, 2000)
            .Select(i => new BlockEspTarget($"minecraft:block_{i}", "x", "AABBCC", true))
            .ToArray();

        string s = BlockEspConfig.Serialize(targets);

        Assert.True(s.Length <= BlockEspConfig.MaxSerializedLength);
    }

    // ── Presets / defaults ───────────────────────────────────────────────────────

    [Fact]
    public void DefaultTargets_HaveDiamondOreEnabledOnly()
    {
        var targets = BlockEspPresets.BuildDefaultTargets();

        var enabled = targets.Where(t => t.Enabled).ToList();
        Assert.Single(enabled);
        Assert.Equal("diamond_ore", BlockEspConfig.NormalizeId(enabled[0].RegistryId));
    }

    // ── Clicker integration ──────────────────────────────────────────────────────

    [Fact]
    public void Clicker_BlockEspBlocksSerialized_ReflectsDefaults()
    {
        var clicker = Clicker.Instance;
        clicker.BlockEspTargets = new System.Collections.ObjectModel.ObservableCollection<BlockEspTarget>(
            BlockEspPresets.BuildDefaultTargets());

        Assert.Equal("diamond_ore=00E5FF", clicker.BlockEspBlocksSerialized);
    }

    // ── Profile persistence ──────────────────────────────────────────────────────

    [Fact]
    public void Profile_BlockEsp_DefaultsContainDiamondOre()
    {
        var profile = new Profile();

        Assert.False(profile.BlockEspEnabled);
        Assert.True(profile.BlockEspBoxes);
        Assert.Equal(64, profile.BlockEspMaxCount);
        Assert.Equal(4, profile.BlockEspRange);
        Assert.Contains(profile.BlockEspTargets,
            t => BlockEspConfig.NormalizeId(t.RegistryId) == "diamond_ore" && t.Enabled);
    }

    [Fact]
    public void Profile_BlockEsp_SerializesRoundTrip()
    {
        var profile = new Profile
        {
            BlockEspEnabled = true,
            BlockEspTracers = true,
            BlockEspMaxCount = 128,
            BlockEspRange = 6,
            BlockEspTargets = new()
            {
                new BlockEspTargetData { RegistryId = "minecraft:gold_ore", DisplayName = "Gold Ore", ColorHex = "FFD700", Enabled = true },
                new BlockEspTargetData { RegistryId = "minecraft:iron_ore", DisplayName = "Iron Ore", ColorHex = "D8AF93", Enabled = false },
            },
        };
        var options = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };

        string json = JsonSerializer.Serialize(profile, options);
        Profile? round = JsonSerializer.Deserialize<Profile>(json, options);

        Assert.NotNull(round);
        Assert.True(round!.BlockEspEnabled);
        Assert.True(round.BlockEspTracers);
        Assert.Equal(128, round.BlockEspMaxCount);
        Assert.Equal(6, round.BlockEspRange);
        Assert.Equal(2, round.BlockEspTargets.Count);
        Assert.Equal("minecraft:gold_ore", round.BlockEspTargets[0].RegistryId);
        Assert.Equal("FFD700", round.BlockEspTargets[0].ColorHex);
        Assert.True(round.BlockEspTargets[0].Enabled);
        Assert.False(round.BlockEspTargets[1].Enabled);
    }

    // ── Capabilities ─────────────────────────────────────────────────────────────

    [Fact]
    public void Capabilities_ExposeBlockEsp_AllVersions()
    {
        BridgeCapabilities modern = BridgeCapabilities.ForVersionFallback("26.1");
        BridgeCapabilities legacy = BridgeCapabilities.ForVersionFallback("1.8.9");

        Assert.True(modern.SupportsModule("blockesp"));
        Assert.True(modern.SupportsSetting("blockespenabled"));
        Assert.True(modern.SupportsSetting("blockespblocks"));
        Assert.True(legacy.SupportsModule("blockesp"));
        Assert.True(legacy.SupportsSetting("blockespenabled"));
        Assert.True(legacy.SupportsSetting("blockespblocks"));
    }
}
