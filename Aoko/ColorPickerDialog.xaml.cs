using System;
using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace Aoko;

/// <summary>
/// A small themed HSV color picker: saturation/value square, hue bar, RGB sliders,
/// and an editable hex field. Use <see cref="TryPick"/> for a modal pick.
/// </summary>
public partial class ColorPickerDialog : Window
{
    private const double SvWidth = 220.0;
    private const double SvHeight = 180.0;
    private const double HueHeight = 180.0;

    // Current selection in HSV. Hue 0..360, sat/val 0..1.
    private double _hue;
    private double _sat;
    private double _val;

    private bool _updating;
    private bool _draggingSv;
    private bool _draggingHue;

    public Color SelectedColor { get; private set; }

    public ColorPickerDialog(Color initial)
    {
        InitializeComponent();
        SetColor(initial);
    }

    /// <summary>Shows the picker modally. Returns true and the chosen color when accepted.</summary>
    public static bool TryPick(Window owner, Color initial, out Color result)
    {
        var dialog = new ColorPickerDialog(initial) { Owner = owner };
        bool ok = dialog.ShowDialog() == true;
        result = ok ? dialog.SelectedColor : initial;
        return ok;
    }

    // ── State updates ────────────────────────────────────────────────────────────

    private void SetColor(Color c)
    {
        RgbToHsv(c, out _hue, out _sat, out _val);
        SelectedColor = c;
        RefreshAll();
    }

    /// <summary>Recomputes the color from HSV and refreshes every control.</summary>
    private void RefreshFromHsv()
    {
        SelectedColor = HsvToRgb(_hue, _sat, _val);
        RefreshAll();
    }

    private void RefreshAll()
    {
        _updating = true;
        try
        {
            Color pure = HsvToRgb(_hue, 1.0, 1.0);
            if (SvHueLayer != null)
                SvHueLayer.Fill = new SolidColorBrush(pure);

            if (SvThumb != null)
            {
                Canvas.SetLeft(SvThumb, _sat * SvWidth - SvThumb.Width / 2.0);
                Canvas.SetTop(SvThumb, (1.0 - _val) * SvHeight - SvThumb.Height / 2.0);
            }

            if (HueThumb != null)
                Canvas.SetTop(HueThumb, _hue / 360.0 * HueHeight - HueThumb.Height / 2.0);

            if (PreviewSwatch != null)
                PreviewSwatch.Background = new SolidColorBrush(SelectedColor);

            if (HexBox != null)
                HexBox.Text = $"#{SelectedColor.R:X2}{SelectedColor.G:X2}{SelectedColor.B:X2}";

            if (RSlider != null) RSlider.Value = SelectedColor.R;
            if (GSlider != null) GSlider.Value = SelectedColor.G;
            if (BSlider != null) BSlider.Value = SelectedColor.B;

            if (RValue != null) RValue.Text = SelectedColor.R.ToString(CultureInfo.InvariantCulture);
            if (GValue != null) GValue.Text = SelectedColor.G.ToString(CultureInfo.InvariantCulture);
            if (BValue != null) BValue.Text = SelectedColor.B.ToString(CultureInfo.InvariantCulture);
        }
        finally
        {
            _updating = false;
        }
    }

    // ── Saturation / Value square ─────────────────────────────────────────────────

    private void SvCanvas_MouseDown(object sender, MouseButtonEventArgs e)
    {
        _draggingSv = true;
        SvCanvas.CaptureMouse();
        UpdateSvFromMouse(e.GetPosition(SvCanvas));
    }

    private void SvCanvas_MouseMove(object sender, MouseEventArgs e)
    {
        if (_draggingSv)
            UpdateSvFromMouse(e.GetPosition(SvCanvas));
    }

    private void SvCanvas_MouseUp(object sender, MouseButtonEventArgs e)
    {
        _draggingSv = false;
        SvCanvas.ReleaseMouseCapture();
    }

    private void UpdateSvFromMouse(Point p)
    {
        _sat = Math.Clamp(p.X / SvWidth, 0.0, 1.0);
        _val = Math.Clamp(1.0 - p.Y / SvHeight, 0.0, 1.0);
        RefreshFromHsv();
    }

    // ── Hue bar ────────────────────────────────────────────────────────────────────

    private void HueCanvas_MouseDown(object sender, MouseButtonEventArgs e)
    {
        _draggingHue = true;
        HueCanvas.CaptureMouse();
        UpdateHueFromMouse(e.GetPosition(HueCanvas));
    }

    private void HueCanvas_MouseMove(object sender, MouseEventArgs e)
    {
        if (_draggingHue)
            UpdateHueFromMouse(e.GetPosition(HueCanvas));
    }

    private void HueCanvas_MouseUp(object sender, MouseButtonEventArgs e)
    {
        _draggingHue = false;
        HueCanvas.ReleaseMouseCapture();
    }

    private void UpdateHueFromMouse(Point p)
    {
        _hue = Math.Clamp(p.Y / HueHeight, 0.0, 1.0) * 360.0;
        RefreshFromHsv();
    }

    // ── RGB sliders ────────────────────────────────────────────────────────────────

    private void RgbSlider_ValueChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (_updating || RSlider == null || GSlider == null || BSlider == null)
            return;

        var c = Color.FromRgb((byte)RSlider.Value, (byte)GSlider.Value, (byte)BSlider.Value);
        RgbToHsv(c, out _hue, out _sat, out _val);
        SelectedColor = c;
        RefreshAll();
    }

    // ── Hex field ────────────────────────────────────────────────────────────────

    private void HexBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter)
        {
            CommitHex();
            e.Handled = true;
        }
    }

    private void HexBox_LostFocus(object sender, RoutedEventArgs e) => CommitHex();

    private void CommitHex()
    {
        if (_updating)
            return;

        if (TryParseHex(HexBox.Text, out Color c))
            SetColor(c);
        else
            RefreshAll(); // revert to last valid value
    }

    // ── Buttons ──────────────────────────────────────────────────────────────────

    private void OkButton_Click(object sender, RoutedEventArgs e)
    {
        DialogResult = true;
        Close();
    }

    private void CancelButton_Click(object sender, RoutedEventArgs e)
    {
        DialogResult = false;
        Close();
    }

    // ── Color math ─────────────────────────────────────────────────────────────────

    private static Color HsvToRgb(double h, double s, double v)
    {
        h = ((h % 360.0) + 360.0) % 360.0;
        s = Math.Clamp(s, 0.0, 1.0);
        v = Math.Clamp(v, 0.0, 1.0);

        double c = v * s;
        double x = c * (1.0 - Math.Abs((h / 60.0) % 2.0 - 1.0));
        double m = v - c;

        double r, g, b;
        if (h < 60) { r = c; g = x; b = 0; }
        else if (h < 120) { r = x; g = c; b = 0; }
        else if (h < 180) { r = 0; g = c; b = x; }
        else if (h < 240) { r = 0; g = x; b = c; }
        else if (h < 300) { r = x; g = 0; b = c; }
        else { r = c; g = 0; b = x; }

        return Color.FromRgb(
            (byte)Math.Round((r + m) * 255.0),
            (byte)Math.Round((g + m) * 255.0),
            (byte)Math.Round((b + m) * 255.0));
    }

    private static void RgbToHsv(Color color, out double h, out double s, out double v)
    {
        double r = color.R / 255.0;
        double g = color.G / 255.0;
        double b = color.B / 255.0;

        double max = Math.Max(r, Math.Max(g, b));
        double min = Math.Min(r, Math.Min(g, b));
        double delta = max - min;

        v = max;
        s = max <= 0.0 ? 0.0 : delta / max;

        if (delta <= 0.0)
        {
            h = 0.0;
            return;
        }

        if (max == r)
            h = 60.0 * (((g - b) / delta) % 6.0);
        else if (max == g)
            h = 60.0 * ((b - r) / delta + 2.0);
        else
            h = 60.0 * ((r - g) / delta + 4.0);

        if (h < 0.0)
            h += 360.0;
    }

    private static bool TryParseHex(string? text, out Color color)
    {
        color = Colors.Black;
        if (string.IsNullOrWhiteSpace(text))
            return false;

        string s = text.Trim();
        if (s.StartsWith("#", StringComparison.Ordinal))
            s = s.Substring(1);

        if (s.Length == 3)
            s = string.Concat(s[0], s[0], s[1], s[1], s[2], s[2]);

        if (s.Length != 6)
            return false;

        if (!byte.TryParse(s.AsSpan(0, 2), NumberStyles.HexNumber, null, out byte r) ||
            !byte.TryParse(s.AsSpan(2, 2), NumberStyles.HexNumber, null, out byte g) ||
            !byte.TryParse(s.AsSpan(4, 2), NumberStyles.HexNumber, null, out byte b))
            return false;

        color = Color.FromRgb(r, g, b);
        return true;
    }
}
