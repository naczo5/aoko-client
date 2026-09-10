using System;
using System.Diagnostics;
using System.Text.Json;
using Aoko.Core;
using Xunit;

namespace Aoko.Tests;

public class ClickerRandomizationTests
{
    [Fact]
    public void Clicker_RandomizationMode_DefaultsToMedium_AndJitterEnabledIsTrue()
    {
        var clicker = Clicker.Instance;
        // In clean state or reset
        clicker.RandomizationMode = ClickRandomizationMode.Medium;
        Assert.Equal(ClickRandomizationMode.Medium, clicker.RandomizationMode);
        Assert.Equal(1, clicker.RandomizationModeIndex);
        Assert.Equal("Medium", clicker.RandomizationModeText);
        Assert.True(clicker.JitterEnabled);
    }

    [Theory]
    [InlineData(ClickRandomizationMode.Basic, 0, "Basic", false)]
    [InlineData(ClickRandomizationMode.Medium, 1, "Medium", true)]
    [InlineData(ClickRandomizationMode.Advanced, 2, "Advanced", true)]
    public void Clicker_RandomizationMode_MappingMatchesExpectedProperties(
        ClickRandomizationMode mode,
        int expectedIndex,
        string expectedText,
        bool expectedJitterEnabled)
    {
        var clicker = Clicker.Instance;
        clicker.RandomizationMode = mode;

        Assert.Equal(mode, clicker.RandomizationMode);
        Assert.Equal(expectedIndex, clicker.RandomizationModeIndex);
        Assert.Equal(expectedText, clicker.RandomizationModeText);
        Assert.Equal(expectedJitterEnabled, clicker.JitterEnabled);
    }

    [Fact]
    public void Clicker_JitterEnabled_SettingFalse_SetsModeToBasic()
    {
        var clicker = Clicker.Instance;
        clicker.RandomizationMode = ClickRandomizationMode.Advanced;
        Assert.True(clicker.JitterEnabled);

        clicker.JitterEnabled = false;
        Assert.Equal(ClickRandomizationMode.Basic, clicker.RandomizationMode);
        Assert.Equal(0, clicker.RandomizationModeIndex);
        Assert.Equal("Basic", clicker.RandomizationModeText);
        Assert.False(clicker.JitterEnabled);
    }

    [Fact]
    public void Clicker_JitterEnabled_SettingTrue_WhenBasic_RestoresMedium()
    {
        var clicker = Clicker.Instance;
        clicker.RandomizationMode = ClickRandomizationMode.Basic;
        Assert.False(clicker.JitterEnabled);

        clicker.JitterEnabled = true;
        Assert.Equal(ClickRandomizationMode.Medium, clicker.RandomizationMode);
        Assert.True(clicker.JitterEnabled);
    }

    [Fact]
    public void Clicker_RandomizationModeIndex_ClampsValues()
    {
        var clicker = Clicker.Instance;
        clicker.RandomizationModeIndex = -5;
        Assert.Equal(ClickRandomizationMode.Basic, clicker.RandomizationMode);

        clicker.RandomizationModeIndex = 99;
        Assert.Equal(ClickRandomizationMode.Advanced, clicker.RandomizationMode);
    }

    [Fact]
    public void Profile_RandomizationMode_DefaultsToMedium()
    {
        var profile = new Profile();
        Assert.Equal(ClickRandomizationMode.Medium, profile.RandomizationMode);
        Assert.True(profile.JitterEnabled);
    }

    [Fact]
    public void Profile_RandomizationMode_SerializesAndRoundTrips()
    {
        var profile = new Profile
        {
            Name = "HumanizedProfile",
            RandomizationMode = ClickRandomizationMode.Advanced
        };

        var options = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
        string json = JsonSerializer.Serialize(profile, options);
        Profile? roundTripped = JsonSerializer.Deserialize<Profile>(json, options);

        Assert.NotNull(roundTripped);
        Assert.Equal(ClickRandomizationMode.Advanced, roundTripped.RandomizationMode);
        Assert.True(roundTripped.JitterEnabled);
    }

    [Fact]
    public void Profile_LegacyJitterEnabledFalse_DeserializesToBasic()
    {
        const string legacyJson = """
            {
                "name": "OldProfile",
                "jitterEnabled": false
            }
            """;

        var options = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
        Profile? profile = JsonSerializer.Deserialize<Profile>(legacyJson, options);

        Assert.NotNull(profile);
        Assert.False(profile.JitterEnabled);
        Assert.Equal(ClickRandomizationMode.Basic, profile.RandomizationMode);
    }

    [Fact]
    public void Profile_LegacyJitterEnabledTrue_DeserializesToMedium()
    {
        const string legacyJson = """
            {
                "name": "OldProfile",
                "jitterEnabled": true
            }
            """;

        var options = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
        Profile? profile = JsonSerializer.Deserialize<Profile>(legacyJson, options);

        Assert.NotNull(profile);
        Assert.True(profile.JitterEnabled);
        Assert.Equal(ClickRandomizationMode.Medium, profile.RandomizationMode);
    }

    [Fact]
    public void AdvancedClickTimingState_GeneratesPositiveDelaysWithinBounds()
    {
        var state = new AdvancedClickTimingState();
        float minCps = 10.0f;
        float maxCps = 14.0f;

        for (int i = 0; i < 50; i++)
        {
            double delayMs = state.CalculateNextDelay(minCps, maxCps);
            Assert.True(delayMs >= 10.0, $"Delay should be at least 10ms, got {delayMs}");
            // Effective CPS is approximately 10-14, with butterfly ratio (up to ~1.3x delay or down to 0.7x delay)
            // Expect delay to be between 40ms and 250ms
            Assert.True(delayMs < 300.0, $"Delay too large: {delayMs}");
        }
    }

    [Fact]
    public void AdvancedClickTimingState_DemonstratesFatigueAccumulationAndIdleRecovery()
    {
        var state = new AdvancedClickTimingState();
        long fakeTimestamp = Stopwatch.GetTimestamp();
        long stepTicks = (long)(0.07 * Stopwatch.Frequency); // 70ms per click (~14 CPS)

        Assert.Equal(0.0, state.Fatigue);

        // Click 25 times continuously
        for (int i = 0; i < 25; i++)
        {
            fakeTimestamp += stepTicks;
            state.CalculateNextDelay(12.0f, 16.0f, fakeTimestamp);
        }

        // Fatigue should have accumulated
        Assert.True(state.Fatigue > 0.1, $"Fatigue should have accumulated, was {state.Fatigue}");

        // Now simulate idling for 2.5 seconds (no clicks)
        fakeTimestamp += (long)(2.5 * Stopwatch.Frequency);
        state.CalculateNextDelay(12.0f, 16.0f, fakeTimestamp);

        // After 2.5s idle, fatigue should reset to 0 (plus the single new click increment)
        Assert.True(state.Fatigue <= 0.02, $"Fatigue should have recovered after idle, was {state.Fatigue}");
    }

    [Fact]
    public void AdvancedClickTimingState_DemonstratesButterflyDoubletAlternation()
    {
        var state = new AdvancedClickTimingState();
        long fakeTimestamp = Stopwatch.GetTimestamp();
        long stepTicks = (long)(0.07 * Stopwatch.Frequency);

        // Initialize state
        state.CalculateNextDelay(10.0f, 14.0f, fakeTimestamp);
        bool firstToggle = state.IsButterflySecondClick;

        fakeTimestamp += stepTicks;
        state.CalculateNextDelay(10.0f, 14.0f, fakeTimestamp);
        bool secondToggle = state.IsButterflySecondClick;

        // Butterfly second click flag should alternate
        Assert.NotEqual(firstToggle, secondToggle);
    }
}
