using System;
using System.Collections.Generic;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;
using Aoko.Core;

namespace Aoko.Views;

/// <summary>
/// TextBlock that paints the characters matched by the module command search in
/// <see cref="HighlightBrush"/> (optionally over <see cref="HighlightBackground"/>).
/// The source string arrives through <see cref="SourceText"/> so the built-in Text/Inlines sync never
/// fights a binding; ranges outside the text are ignored, so a stale range can never throw.
/// </summary>
public sealed class HighlightTextBlock : TextBlock
{
    public static readonly DependencyProperty SourceTextProperty = DependencyProperty.Register(
        nameof(SourceText),
        typeof(string),
        typeof(HighlightTextBlock),
        new PropertyMetadata(string.Empty, OnVisualPropertyChanged));

    public static readonly DependencyProperty MatchRangesProperty = DependencyProperty.Register(
        nameof(MatchRanges),
        typeof(IReadOnlyList<ModuleSearchRange>),
        typeof(HighlightTextBlock),
        new PropertyMetadata(null, OnVisualPropertyChanged));

    public static readonly DependencyProperty HighlightBrushProperty = DependencyProperty.Register(
        nameof(HighlightBrush),
        typeof(Brush),
        typeof(HighlightTextBlock),
        new PropertyMetadata(null, OnVisualPropertyChanged));

    public static readonly DependencyProperty HighlightBackgroundProperty = DependencyProperty.Register(
        nameof(HighlightBackground),
        typeof(Brush),
        typeof(HighlightTextBlock),
        new PropertyMetadata(null, OnVisualPropertyChanged));

    private bool _rebuilding;

    public string SourceText
    {
        get => (string)GetValue(SourceTextProperty);
        set => SetValue(SourceTextProperty, value);
    }

    public IReadOnlyList<ModuleSearchRange>? MatchRanges
    {
        get => (IReadOnlyList<ModuleSearchRange>?)GetValue(MatchRangesProperty);
        set => SetValue(MatchRangesProperty, value);
    }

    public Brush? HighlightBrush
    {
        get => (Brush?)GetValue(HighlightBrushProperty);
        set => SetValue(HighlightBrushProperty, value);
    }

    /// <summary>Optional wash behind the matched characters so they read as matches on any row state.</summary>
    public Brush? HighlightBackground
    {
        get => (Brush?)GetValue(HighlightBackgroundProperty);
        set => SetValue(HighlightBackgroundProperty, value);
    }

    private static void OnVisualPropertyChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
        => ((HighlightTextBlock)d).Rebuild();

    private void Rebuild()
    {
        if (_rebuilding)
            return;

        string text = SourceText ?? string.Empty;
        List<ModuleSearchRange> ranges = Normalize(MatchRanges, text.Length);

        _rebuilding = true;
        try
        {
            Inlines.Clear();

            if (ranges.Count == 0)
            {
                if (text.Length > 0)
                    Inlines.Add(new Run(text));
                return;
            }

            int cursor = 0;
            foreach (ModuleSearchRange range in ranges)
            {
                if (range.Start > cursor)
                    Inlines.Add(new Run(text[cursor..range.Start]));

                Inlines.Add(new Run(text.Substring(range.Start, range.Length))
                {
                    Foreground = HighlightBrush ?? Foreground,
                    Background = HighlightBackground,
                    FontWeight = FontWeights.SemiBold,
                });

                cursor = range.Start + range.Length;
            }

            if (cursor < text.Length)
                Inlines.Add(new Run(text[cursor..]));
        }
        finally
        {
            _rebuilding = false;
        }
    }

    private static List<ModuleSearchRange> Normalize(IReadOnlyList<ModuleSearchRange>? ranges, int length)
    {
        var result = new List<ModuleSearchRange>();
        if (ranges == null || length == 0)
            return result;

        foreach (ModuleSearchRange range in ranges)
        {
            int start = Math.Max(0, range.Start);
            int end = Math.Min(length, range.Start + range.Length);
            if (end <= start)
                continue;

            if (result.Count > 0)
            {
                ModuleSearchRange last = result[^1];
                int lastEnd = last.Start + last.Length;
                if (start <= lastEnd)
                {
                    if (end > lastEnd)
                        result[^1] = new ModuleSearchRange(last.Start, end - last.Start);
                    continue;
                }
            }

            result.Add(new ModuleSearchRange(start, end - start));
        }

        return result;
    }
}
