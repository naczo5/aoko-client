using System;
using System.Text.RegularExpressions;

namespace Aoko.Tests;

/// <summary>
/// The contrast claims the tabs rely on, checked against the real palette instead of
/// a comment. Decorative values (card edges, washes) are recorded with lower bounds
/// because WCAG has no 3:1 target the Slate palette can reach without going chalky.
/// </summary>
public class PaletteContrastTests
{
    private static readonly string AppXaml = TestRepo.Read("Aoko", "App.xaml");
    private static readonly string Styles = TestRepo.Read("Aoko", "Views", "TabStyles.xaml");

    private static string Color(string key)
    {
        Match match = Regex.Match(
            AppXaml,
            $@"<Color x:Key=""{key}"">(#[0-9A-Fa-f]{{6}})</Color>");

        Assert.True(match.Success, $"App.xaml has no Color named {key}.");
        return match.Groups[1].Value;
    }

    private static string Tone(string name) => name switch
    {
        "Critical" => FixedBrush("CriticalBrush"),
        "Warn" => FixedBrush("WarnBrush"),
        _ => Color("AccentColor")
    };

    private static string FixedBrush(string key)
    {
        Match match = Regex.Match(
            Styles,
            $@"<SolidColorBrush x:Key=""{key}"" Color=""(#[0-9A-Fa-f]{{6}})""/>");

        Assert.True(match.Success, $"TabStyles.xaml has no fixed brush named {key}.");
        return match.Groups[1].Value;
    }

    [Fact]
    public void PrimaryLabel_PassesAaWithTheBackgroundBrush()
    {
        // White on the coral accent is 3.95:1 and fails; BgBrush is 4.99:1.
        Assert.True(
            Ratio(Color("BgColor"), Color("AccentColor")) >= 4.5,
            $"BgBrush on accent is {Ratio(Color("BgColor"), Color("AccentColor")):0.00}:1.");
        Assert.True(
            Ratio("#FFFFFF", Color("AccentColor")) < 4.5,
            "White on the accent is no longer below AA; the primary label choice needs review.");
    }

    [Theory]
    [InlineData(0.16, "Accent")]
    [InlineData(0.20, "Critical")]
    [InlineData(0.16, "Warn")]
    public void ChipLabels_PassAaOnTheirTintLayers(double opacity, string toneName)
    {
        string tint = Blend(opacity, Tone(toneName), Color("PanelColor"));

        Assert.True(
            Ratio(Color("TextColor"), tint) >= 4.5,
            $"TextBrush on the {toneName} tint is {Ratio(Color("TextColor"), tint):0.00}:1.");
    }

    [Fact]
    public void StatusTones_PassAaOnTheTabBackground()
    {
        Assert.True(Ratio(Color("DimTextColor"), Color("BgColor")) >= 4.5);
        Assert.True(Ratio(Color("AccentColor"), Color("BgColor")) >= 4.5);
        Assert.True(Ratio(Tone("Critical"), Color("BgColor")) >= 4.5);
        Assert.True(Ratio(Color("AccentColor"), Color("PanelColor")) >= 4.5);
    }

    [Fact]
    public void CardEdgeAndWashes_AreMeasurablyStrongerThanTheOldHairline()
    {
        double border = Ratio(Color("SliderFgColor"), Color("PanelColor"));
        double rest = Ratio(Color("PanelColor"), Color("BgColor"));
        double hover = Ratio(Blend(0.06, Color("AccentColor"), Color("PanelColor")), Color("PanelColor"));
        double selected = Ratio(Blend(0.12, Color("AccentColor"), Color("PanelColor")), Color("PanelColor"));

        // The old edge was 1.07:1; SliderFg is the perceptible hairline the review asked for.
        Assert.True(border > rest, $"Card border {border:0.00}:1 is not stronger than the fill {rest:0.00}:1.");
        Assert.True(border >= 1.3, $"Card border is only {border:0.00}:1.");
        Assert.True(selected > hover, "The selected wash must be stronger than the hover wash.");
        Assert.True(hover >= 1.05, $"Hover wash is only {hover:0.00}:1.");
        Assert.True(Ratio(Color("TextColor"), Color("PanelColor")) >= 4.5);
    }

    [Fact]
    public void SharedStyles_CarryTheContract()
    {
        Assert.Equal(3, Regex.Matches(Styles, "ToolTipService.ShowOnDisabled").Count);
        Assert.Contains("Foreground\" Value=\"{DynamicResource BgBrush}\"", Styles);
        Assert.DoesNotContain("Foreground\" Value=\"#FFFFFF\"", Styles);
        Assert.Contains("BorderThickness\" Value=\"2\"", Styles);
        Assert.Contains("x:Key=\"TabFocusVisual\"", Styles);
        Assert.Contains("BorderBrush\" Value=\"{DynamicResource AccentBrush}\"", Styles);
    }

    private static double Ratio(string a, string b)
    {
        double la = Luminance(a);
        double lb = Luminance(b);
        double lighter = Math.Max(la, lb);
        double darker = Math.Min(la, lb);
        return (lighter + 0.05) / (darker + 0.05);
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

    private static string Blend(double alpha, string top, string bottom)
    {
        int[] upper = Rgb(top);
        int[] lower = Rgb(bottom);
        int[] mixed = new int[3];

        for (int i = 0; i < 3; i++)
            mixed[i] = (int)Math.Round(alpha * upper[i] + (1 - alpha) * lower[i]);

        return $"#{mixed[0]:X2}{mixed[1]:X2}{mixed[2]:X2}";
    }

    private static int[] Rgb(string hex)
    {
        string value = hex.TrimStart('#');
        return new[]
        {
            Convert.ToInt32(value.Substring(0, 2), 16),
            Convert.ToInt32(value.Substring(2, 2), 16),
            Convert.ToInt32(value.Substring(4, 2), 16)
        };
    }
}
