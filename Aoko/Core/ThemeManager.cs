using System;
using System.Collections.Generic;
using System.Windows;
using System.Windows.Media;

namespace Aoko.Core;

public static class ThemeManager
{
    public static string CurrentTheme { get; private set; } = "Slate";

    public static event Action? ThemeChanged;

    public static readonly Dictionary<string, ThemeColors> Themes = new(StringComparer.OrdinalIgnoreCase)
    {
        ["Slate"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0A, 0x0B, 0x0F),
            Panel = Color.FromRgb(0x12, 0x14, 0x1A),
            SliderBg = Color.FromRgb(0x18, 0x1B, 0x22),
            SliderFg = Color.FromRgb(0x2A, 0x2F, 0x38),
            Accent = Color.FromRgb(0xC7, 0x62, 0x5A),
            AccentSecondary = Color.FromRgb(0xC7, 0x62, 0x5A),
            Text = Color.FromRgb(0xE8, 0xEA, 0xEE),
            DimText = Color.FromRgb(0x7A, 0x82, 0x90)
        },
        ["Ink"] = new ThemeColors
        {
            Background = Color.FromRgb(0x08, 0x09, 0x0B),
            Panel = Color.FromRgb(0x10, 0x11, 0x15),
            SliderBg = Color.FromRgb(0x16, 0x18, 0x1C),
            SliderFg = Color.FromRgb(0x26, 0x28, 0x30),
            Accent = Color.FromRgb(0xB0, 0xB6, 0xC0),
            AccentSecondary = Color.FromRgb(0xB0, 0xB6, 0xC0),
            Text = Color.FromRgb(0xE8, 0xEA, 0xEE),
            DimText = Color.FromRgb(0x7A, 0x82, 0x8F)
        },
        ["Graphite"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0B, 0x0B, 0x0D),
            Panel = Color.FromRgb(0x13, 0x13, 0x16),
            SliderBg = Color.FromRgb(0x19, 0x19, 0x1C),
            SliderFg = Color.FromRgb(0x2A, 0x2A, 0x2D),
            Accent = Color.FromRgb(0xB8, 0x9B, 0x82),
            AccentSecondary = Color.FromRgb(0xB8, 0x9B, 0x82),
            Text = Color.FromRgb(0xE8, 0xE8, 0xEA),
            DimText = Color.FromRgb(0x82, 0x82, 0x7E)
        },
        ["Steel"] = new ThemeColors
        {
            Background = Color.FromRgb(0x08, 0x09, 0x0C),
            Panel = Color.FromRgb(0x0F, 0x12, 0x18),
            SliderBg = Color.FromRgb(0x16, 0x1A, 0x21),
            SliderFg = Color.FromRgb(0x26, 0x2C, 0x35),
            Accent = Color.FromRgb(0x6B, 0x8D, 0xAB),
            AccentSecondary = Color.FromRgb(0x6B, 0x8D, 0xAB),
            Text = Color.FromRgb(0xE5, 0xE8, 0xEE),
            DimText = Color.FromRgb(0x72, 0x86, 0xA0)
        },
        ["Blend"] = new ThemeColors
        {
            Background = Color.FromRgb(0x07, 0x0B, 0x14),
            Panel = Color.FromRgb(0x0D, 0x14, 0x24),
            SliderBg = Color.FromRgb(0x13, 0x1E, 0x33),
            SliderFg = Color.FromRgb(0x1C, 0x2B, 0x49),
            Accent = Color.FromRgb(0x47, 0x94, 0xFD),
            AccentSecondary = Color.FromRgb(0x47, 0xFD, 0xA0),
            Text = Color.FromRgb(0xED, 0xF3, 0xF7),
            DimText = Color.FromRgb(0x7B, 0x94, 0xB5)
        },
        ["Lush"] = new ThemeColors
        {
            Background = Color.FromRgb(0x08, 0x0D, 0x09),
            Panel = Color.FromRgb(0x0F, 0x17, 0x10),
            SliderBg = Color.FromRgb(0x17, 0x24, 0x1A),
            SliderFg = Color.FromRgb(0x22, 0x33, 0x25),
            Accent = Color.FromRgb(0xA8, 0xE0, 0x63),
            AccentSecondary = Color.FromRgb(0x56, 0xAB, 0x2F),
            Text = Color.FromRgb(0xEE, 0xF5, 0xEE),
            DimText = Color.FromRgb(0x7A, 0x9C, 0x7D)
        },
        ["Water"] = new ThemeColors
        {
            Background = Color.FromRgb(0x06, 0x0B, 0x12),
            Panel = Color.FromRgb(0x0B, 0x16, 0x24),
            SliderBg = Color.FromRgb(0x10, 0x21, 0x36),
            SliderFg = Color.FromRgb(0x18, 0x30, 0x4D),
            Accent = Color.FromRgb(0x0C, 0xE8, 0xC7),
            AccentSecondary = Color.FromRgb(0x0C, 0xA3, 0xE8),
            Text = Color.FromRgb(0xEA, 0xF7, 0xF8),
            DimText = Color.FromRgb(0x70, 0x9C, 0xB8)
        },
        ["Lime Water"] = new ThemeColors
        {
            Background = Color.FromRgb(0x06, 0x0E, 0x0E),
            Panel = Color.FromRgb(0x0B, 0x1B, 0x1B),
            SliderBg = Color.FromRgb(0x11, 0x28, 0x28),
            SliderFg = Color.FromRgb(0x1A, 0x3A, 0x3A),
            Accent = Color.FromRgb(0x12, 0xFF, 0xF7),
            AccentSecondary = Color.FromRgb(0xB3, 0xFF, 0xAB),
            Text = Color.FromRgb(0xF0, 0xFA, 0xF9),
            DimText = Color.FromRgb(0x70, 0xA5, 0xA0)
        },
        ["Digital Horizon"] = new ThemeColors
        {
            Background = Color.FromRgb(0x09, 0x0A, 0x14),
            Panel = Color.FromRgb(0x12, 0x13, 0x24),
            SliderBg = Color.FromRgb(0x1B, 0x1D, 0x33),
            SliderFg = Color.FromRgb(0x26, 0x29, 0x47),
            Accent = Color.FromRgb(0x5F, 0xC3, 0xE4),
            AccentSecondary = Color.FromRgb(0xE5, 0x5D, 0x87),
            Text = Color.FromRgb(0xF3, 0xED, 0xF5),
            DimText = Color.FromRgb(0x8E, 0x7F, 0xA8)
        },
        ["Coral"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0A, 0x0B, 0x0F),
            Panel = Color.FromRgb(0x14, 0x15, 0x1C),
            SliderBg = Color.FromRgb(0x1E, 0x20, 0x2B),
            SliderFg = Color.FromRgb(0x2D, 0x30, 0x40),
            Accent = Color.FromRgb(0xF4, 0xA8, 0x96),
            AccentSecondary = Color.FromRgb(0x34, 0x85, 0x97),
            Text = Color.FromRgb(0xF7, 0xED, 0xEB),
            DimText = Color.FromRgb(0xA0, 0x87, 0x85)
        },
        ["Magic"] = new ThemeColors
        {
            Background = Color.FromRgb(0x06, 0x07, 0x14),
            Panel = Color.FromRgb(0x0D, 0x0F, 0x24),
            SliderBg = Color.FromRgb(0x15, 0x18, 0x38),
            SliderFg = Color.FromRgb(0x20, 0x25, 0x54),
            Accent = Color.FromRgb(0x7F, 0x9E, 0xFF),
            AccentSecondary = Color.FromRgb(0x8E, 0x2D, 0xE2),
            Text = Color.FromRgb(0xED, 0xF1, 0xFF),
            DimText = Color.FromRgb(0x80, 0x8E, 0xC7)
        },
        ["Blossom"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0B, 0x08, 0x12),
            Panel = Color.FromRgb(0x16, 0x11, 0x21),
            SliderBg = Color.FromRgb(0x22, 0x1B, 0x30),
            SliderFg = Color.FromRgb(0x30, 0x27, 0x42),
            Accent = Color.FromRgb(0xE2, 0xD0, 0xF9),
            AccentSecondary = Color.FromRgb(0x31, 0x77, 0x73),
            Text = Color.FromRgb(0xF7, 0xF2, 0xFD),
            DimText = Color.FromRgb(0x9E, 0x8C, 0xAE)
        },
        ["Pastel"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0D, 0x0A, 0x10),
            Panel = Color.FromRgb(0x18, 0x13, 0x1E),
            SliderBg = Color.FromRgb(0x24, 0x1D, 0x2B),
            SliderFg = Color.FromRgb(0x33, 0x2A, 0x3D),
            Accent = Color.FromRgb(0xF3, 0x9B, 0xB2),
            AccentSecondary = Color.FromRgb(0xCF, 0xC4, 0xF3),
            Text = Color.FromRgb(0xFB, 0xF3, 0xF6),
            DimText = Color.FromRgb(0xA8, 0x90, 0x9D)
        },
        ["Sunkist"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0E, 0x0B, 0x07),
            Panel = Color.FromRgb(0x1A, 0x15, 0x0D),
            SliderBg = Color.FromRgb(0x26, 0x20, 0x14),
            SliderFg = Color.FromRgb(0x38, 0x2F, 0x1E),
            Accent = Color.FromRgb(0xF2, 0xC9, 0x4C),
            AccentSecondary = Color.FromRgb(0xF2, 0x99, 0x4A),
            Text = Color.FromRgb(0xFF, 0xF9, 0xED),
            DimText = Color.FromRgb(0xA8, 0x96, 0x75)
        },
        ["Nord"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0B, 0x0E, 0x12),
            Panel = Color.FromRgb(0x12, 0x18, 0x1F),
            SliderBg = Color.FromRgb(0x1B, 0x24, 0x2E),
            SliderFg = Color.FromRgb(0x26, 0x33, 0x40),
            Accent = Color.FromRgb(0x8F, 0xBC, 0xBB),
            AccentSecondary = Color.FromRgb(0xA3, 0xBE, 0x8C),
            Text = Color.FromRgb(0xEC, 0xEF, 0xF4),
            DimText = Color.FromRgb(0x7B, 0x88, 0x9B)
        },
        ["Cherry"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0F, 0x08, 0x0C),
            Panel = Color.FromRgb(0x1C, 0x10, 0x18),
            SliderBg = Color.FromRgb(0x2B, 0x19, 0x25),
            SliderFg = Color.FromRgb(0x3D, 0x24, 0x34),
            Accent = Color.FromRgb(0xBB, 0x37, 0x7D),
            AccentSecondary = Color.FromRgb(0xFB, 0xD3, 0xE9),
            Text = Color.FromRgb(0xFC, 0xF2, 0xF7),
            DimText = Color.FromRgb(0xA8, 0x7C, 0x93)
        },
        ["Aubergine"] = new ThemeColors
        {
            Background = Color.FromRgb(0x0E, 0x06, 0x0D),
            Panel = Color.FromRgb(0x1A, 0x0C, 0x18),
            SliderBg = Color.FromRgb(0x28, 0x13, 0x25),
            SliderFg = Color.FromRgb(0x3B, 0x1D, 0x37),
            Accent = Color.FromRgb(0xAA, 0x07, 0x6B),
            AccentSecondary = Color.FromRgb(0x61, 0x04, 0x5F),
            Text = Color.FromRgb(0xFA, 0xEB, 0xF7),
            DimText = Color.FromRgb(0x9E, 0x6E, 0x95)
        },
        ["Snowy Sky"] = new ThemeColors
        {
            Background = Color.FromRgb(0x06, 0x0C, 0x10),
            Panel = Color.FromRgb(0x0C, 0x18, 0x20),
            SliderBg = Color.FromRgb(0x13, 0x24, 0x30),
            SliderFg = Color.FromRgb(0x1D, 0x34, 0x45),
            Accent = Color.FromRgb(0x01, 0xAB, 0xB3),
            AccentSecondary = Color.FromRgb(0x12, 0xE8, 0xE8),
            Text = Color.FromRgb(0xEA, 0xF8, 0xFA),
            DimText = Color.FromRgb(0x75, 0xA2, 0xAC)
        }
    };

    public static List<string> GetThemeNames() => new(Themes.Keys);

    public static void ApplyTheme(string themeName)
    {
        if (!Themes.TryGetValue(themeName, out var colors))
            return;

        CurrentTheme = themeName;

        var app = Application.Current;
        if (app == null) return;

        app.Resources["BgColor"] = colors.Background;
        app.Resources["PanelColor"] = colors.Panel;
        app.Resources["SliderBgColor"] = colors.SliderBg;
        app.Resources["SliderFgColor"] = colors.SliderFg;
        app.Resources["AccentColor"] = colors.Accent;
        app.Resources["TextColor"] = colors.Text;
        app.Resources["DimTextColor"] = colors.DimText;

        app.Resources["BgBrush"] = new SolidColorBrush(colors.Background);
        app.Resources["PanelBrush"] = new SolidColorBrush(colors.Panel);
        app.Resources["SliderBgBrush"] = new SolidColorBrush(colors.SliderBg);
        app.Resources["SliderFgBrush"] = new SolidColorBrush(colors.SliderFg);
        app.Resources["AccentBrush"] = new SolidColorBrush(colors.Accent);
        app.Resources["TextBrush"] = new SolidColorBrush(colors.Text);
        app.Resources["DimTextBrush"] = new SolidColorBrush(colors.DimText);

        ThemeChanged?.Invoke();
    }
}

public class ThemeColors
{
    public Color Background { get; set; }
    public Color Panel { get; set; }
    public Color SliderBg { get; set; }
    public Color SliderFg { get; set; }
    public Color Accent { get; set; }
    public Color? AccentSecondary { get; set; }
    public Color Text { get; set; }
    public Color DimText { get; set; }
}
