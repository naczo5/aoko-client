using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Peers;
using System.Windows.Automation.Provider;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Threading;
using Aoko.Core;

namespace Aoko.Views;

/// <summary>
/// Command search overlay for the external GUI: fuzzy module search, in-place toggles, and jumps to
/// the owning tab. View-only; the window supplies row state and performs the toggle/jump.
/// </summary>
public partial class CommandSearchOverlay : UserControl
{
    private const double CardMaxWidth = 620;
    private const double CardMinWidth = 340;
    private const double CardTopRatio = 0.30;

    // Margin="-20" on the overlay means the client gutter is this minus 20. 3 sigma of the BlurRadius 28 shadow
    // is ~42 px and ShadowDepth 8 adds 8 downward, so 72 keeps the halo's edge off the window at every size.
    private const double CardEdgeGutter = 72;
    private const double ResultsMaxHeight = 300;
    private const double ResultsVerticalMargin = 12;

    private readonly ModuleSearchIndex _index = ModuleSearchIndex.Default;
    private readonly Dictionary<string, CommandSearchItem> _items = new(StringComparer.Ordinal);
    private List<CommandSearchItem> _visibleRows = new();
    private CommandSearchItem? _selected;
    private bool _suppressQueryChange;
    private bool _closing;
    private bool _placed;

    public CommandSearchOverlay()
    {
        InitializeComponent();
        foreach (ModuleSearchEntry entry in _index.Entries)
        {
            var item = new CommandSearchItem(entry) { SwitchToggled = OnRowSwitchToggled };
            _items[entry.Id] = item;
        }

        SizeChanged += (_, _) => LayoutCard();
        CardVisual.SizeChanged += (_, _) => LayoutCard();
    }

    /// <summary>Raised when a row is activated. The window jumps, then the overlay closes itself.</summary>
    public event EventHandler<ModuleSearchEntry>? JumpRequested;

    /// <summary>Raised when a row switch is pressed. The window toggles the module, the overlay stays open.</summary>
    public event EventHandler<ModuleSearchEntry>? ToggleRequested;

    /// <summary>Raised after the close animation finished and the overlay is collapsed.</summary>
    public event EventHandler? Closed;

    public bool IsOpen { get; private set; }

    /// <summary>Resolves live state for a row (availability, keybind text, current switch value).</summary>
    public Func<ModuleSearchEntry, CommandSearchRowState>? StateProvider { get; set; }

    public void Open(string? initialQuery)
    {
        _closing = false;
        _placed = false;
        IsOpen = true;

        _suppressQueryChange = true;
        QueryBox.Text = initialQuery ?? string.Empty;
        QueryBox.CaretIndex = QueryBox.Text.Length;
        _suppressQueryChange = false;

        UpdatePlaceholder();
        RebuildResults(QueryBox.Text);

        Visibility = Visibility.Visible;
        UpdateLayout();
        LayoutCard();
        AnimateIn();
        QueryBox.Focus();
    }

    public void Close()
    {
        if (!IsOpen || _closing)
            return;

        _closing = true;
        IsOpen = false;

        if (!SystemParameters.ClientAreaAnimation)
        {
            FinishClose();
            return;
        }

        var duration = TimeSpan.FromMilliseconds(100);
        var ease = new QuadraticEase { EasingMode = EasingMode.EaseOut };

        Scrim.Opacity = 0;
        CardVisual.Opacity = 0;
        CardSlide.Y = 6;

        var scrimOut = new DoubleAnimation(1, 0, duration)
        {
            EasingFunction = ease,
            FillBehavior = FillBehavior.Stop,
        };
        scrimOut.Completed += (_, _) => FinishClose();

        Scrim.BeginAnimation(OpacityProperty, scrimOut);
        CardVisual.BeginAnimation(OpacityProperty, new DoubleAnimation(1, 0, duration)
        {
            EasingFunction = ease,
            FillBehavior = FillBehavior.Stop,
        });
        CardSlide.BeginAnimation(TranslateTransform.YProperty, new DoubleAnimation(0, 6, duration)
        {
            EasingFunction = ease,
            FillBehavior = FillBehavior.Stop,
        });
    }

    private void FinishClose()
    {
        if (!_closing)
            return;

        _closing = false;
        Visibility = Visibility.Collapsed;
        Closed?.Invoke(this, EventArgs.Empty);
    }

    private void AnimateIn()
    {
        Scrim.Opacity = 1;
        CardVisual.Opacity = 1;
        CardSlide.Y = 0;

        if (!SystemParameters.ClientAreaAnimation)
            return;

        var cardDuration = TimeSpan.FromMilliseconds(140);
        var cardEase = new QuarticEase { EasingMode = EasingMode.EaseOut };

        Scrim.BeginAnimation(OpacityProperty, new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(180))
        {
            EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut },
            FillBehavior = FillBehavior.Stop,
        });
        CardVisual.BeginAnimation(OpacityProperty, new DoubleAnimation(0, 1, cardDuration)
        {
            EasingFunction = cardEase,
            FillBehavior = FillBehavior.Stop,
        });
        CardSlide.BeginAnimation(TranslateTransform.YProperty, new DoubleAnimation(8, 0, cardDuration)
        {
            EasingFunction = cardEase,
            FillBehavior = FillBehavior.Stop,
        });
    }

    private void LayoutCard()
    {
        double width = ActualWidth;
        double height = ActualHeight;
        if (width <= 0 || height <= 0)
            return;

        double cardWidth = Math.Min(CardMaxWidth, Math.Max(CardMinWidth, width - (CardEdgeGutter * 2)));
        if (double.IsNaN(CardVisual.Width) || Math.Abs(CardVisual.Width - cardWidth) > 0.5)
            CardVisual.Width = cardWidth;

        // Top-anchored at CardTopRatio: the query field keeps its place while the list is filtered, so the
        // only thing that moves while typing is the list. The cap below keeps the card inside the gutters,
        // which is what makes this a single placement instead of a clamp that slides the field.
        double top = Math.Max(CardEdgeGutter, height * CardTopRatio);
        LimitResultsHeight(height - top - CardEdgeGutter);
        PlaceCard(top);
    }

    /// <summary>
    /// Caps the list at the room below the card top. A short window scrolls instead of sliding the card up,
    /// which is what used to move the query field on every keystroke once the clamp engaged.
    /// </summary>
    private void LimitResultsHeight(double roomForCard)
    {
        double cardHeight = CardVisual.ActualHeight;
        if (cardHeight <= 0)
            return;

        // Whatever the card adds around the results area is fixed, so it is the term to subtract.
        double chrome = cardHeight - Math.Max(ResultsArea.MinHeight, ResultsArea.ActualHeight);
        double max = Math.Clamp(roomForCard - chrome - ResultsVerticalMargin, ResultsArea.MinHeight, ResultsMaxHeight);

        if (Math.Abs(ResultsScroll.MaxHeight - max) > 0.5)
            ResultsScroll.MaxHeight = max;
    }

    /// <summary>Moves the card without snapping: the first placement jumps, later ones ease over 120 ms.</summary>
    private void PlaceCard(double offset)
    {
        bool animate = _placed && SystemParameters.ClientAreaAnimation;
        _placed = true;

        double from = CardPosition.Y;
        if (Math.Abs(from - offset) < 0.5)
            return;

        CardPosition.Y = offset;
        if (!animate)
            return;

        CardPosition.BeginAnimation(TranslateTransform.YProperty, new DoubleAnimation(from, offset, TimeSpan.FromMilliseconds(120))
        {
            EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut },
            FillBehavior = FillBehavior.Stop,
        });
    }

    private void RebuildResults(string query)
    {
        IReadOnlyList<ModuleSearchHit> hits = _index.Rows(query, entry => ResolveState(entry).IsVisible);

        var rows = new List<CommandSearchItem>(hits.Count);
        foreach (ModuleSearchHit hit in hits)
        {
            CommandSearchItem item = _items[hit.Entry.Id];
            item.ApplyMatch(hit);
            item.ApplyState(ResolveState(hit.Entry));
            rows.Add(item);
        }

        // Same rows in the same order: leave the containers alone so hover, scroll and the
        // per-keystroke path stay free of layout work.
        bool reordered = !SameRows(_visibleRows, rows);
        if (reordered)
        {
            _visibleRows = rows;
            ResultsList.ItemsSource = rows;
        }

        bool empty = rows.Count == 0;
        ResultsScroll.Visibility = empty ? Visibility.Collapsed : Visibility.Visible;
        EmptyText.Visibility = empty ? Visibility.Visible : Visibility.Collapsed;
        ResultsCount.Visibility = empty ? Visibility.Collapsed : Visibility.Visible;
        if (empty)
            EmptyText.Text = BuildEmptyMessage(query);

        // The list keeps its accessible name; the count is help text, and the live region announces it.
        string summary = BuildResultsSummary(rows.Count);
        ResultsCount.Text = summary;
        AutomationProperties.SetHelpText(ResultsList, summary);

        // Selection follows the ranking: the first visible row is also what Enter acts on. Only a changed
        // row set scrolls, so plain typing does not queue a BringIntoView the viewport already satisfies.
        SetSelection(rows.Count > 0 ? rows[0] : null, scrollIntoView: reordered);
        AnnounceResults(empty);
    }

    private void AnnounceResults(bool empty)
    {
        // Polite live region: only announce when a screen reader already owns a peer for the target.
        FrameworkElement target = empty ? EmptyText : ResultsList;
        if (UIElementAutomationPeer.FromElement(target) is AutomationPeer peer)
            peer.RaiseAutomationEvent(AutomationEvents.LiveRegionChanged);
    }

    private static bool SameRows(List<CommandSearchItem> left, List<CommandSearchItem> right)
    {
        if (left.Count != right.Count)
            return false;

        for (int i = 0; i < left.Count; i++)
        {
            if (!ReferenceEquals(left[i], right[i]))
                return false;
        }

        return true;
    }

    private CommandSearchRowState ResolveState(ModuleSearchEntry entry)
        => StateProvider?.Invoke(entry)
           ?? CommandSearchRowPolicy.Resolve(entry, devMode: true, supported: true, isOn: false, keybindText: string.Empty);

    /// <summary>
    /// Re-reads live state for the visible rows without re-ranking: a keybind toggle or a capability change
    /// while the overlay is open must reach the switches, or a row keeps showing a state the module left.
    /// </summary>
    public void RefreshRows()
    {
        if (!IsOpen)
            return;

        foreach (CommandSearchItem item in _visibleRows)
            item.ApplyState(ResolveState(item.Entry));
    }

    private static string BuildResultsSummary(int count)
        => count switch
        {
            0 => "No results",
            1 => "1 result",
            _ => $"{count} results",
        };

    private static string BuildEmptyMessage(string query)
    {
        string trimmed = (query ?? string.Empty).Trim();
        if (trimmed.Length > 24)
            trimmed = trimmed[..24] + "...";

        return $"No module matches \"{trimmed}\". Try \"esp\", \"aim\", or a tab name.";
    }

    private void UpdatePlaceholder()
        => PlaceholderText.Visibility = QueryBox.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

    private void MoveSelection(int delta)
    {
        if (_visibleRows.Count == 0)
            return;

        int index = _selected == null ? -1 : _visibleRows.IndexOf(_selected);
        int next = index < 0
            ? (delta > 0 ? 0 : _visibleRows.Count - 1)
            : (index + delta + _visibleRows.Count) % _visibleRows.Count;

        SetSelection(_visibleRows[next], scrollIntoView: true);
    }

    private void SetSelection(CommandSearchItem? item, bool scrollIntoView)
    {
        if (!ReferenceEquals(_selected, item))
        {
            _selected?.SetSelected(false);
            _selected = item;
            _selected?.SetSelected(true);
            AnnounceSelection();
        }

        if (scrollIntoView && item != null)
            ScrollRowIntoView(item);
    }

    /// <summary>
    /// Raises the selection event the row's SelectionItem pattern advertises. Arrow keys move the highlight
    /// without touching focus, so this event is the only thing a screen reader hears. Posted: a changed row set
    /// has no containers yet when the selection moves, and the peer is created on demand because a fresh row
    /// has none until a client walks the tree.
    /// </summary>
    private void AnnounceSelection()
    {
        if (_selected == null)
            return;

        CommandSearchItem item = _selected;
        Dispatcher.BeginInvoke(DispatcherPriority.Background, new Action(() =>
        {
            if (!ReferenceEquals(_selected, item))
                return;

            if (ResultsList.ItemContainerGenerator.ContainerFromItem(item) is not DependencyObject container
                || CommandSearchOverlay.FindDescendant<CommandSearchRow>(container) is not CommandSearchRow row)
                return;

            UIElementAutomationPeer.CreatePeerForElement(row)
                .RaiseAutomationEvent(AutomationEvents.SelectionItemPatternOnElementSelected);
        }));
    }

    internal static T? FindDescendant<T>(DependencyObject node) where T : DependencyObject
    {
        int count = VisualTreeHelper.GetChildrenCount(node);
        for (int i = 0; i < count; i++)
        {
            DependencyObject child = VisualTreeHelper.GetChild(node, i);
            if (child is T match)
                return match;

            T? found = FindDescendant<T>(child);
            if (found != null)
                return found;
        }

        return null;
    }

    /// <summary>Selection requested by the row automation peer (screen reader selection).</summary>
    internal void SelectRow(CommandSearchItem? item)
    {
        if (item != null && _visibleRows.Contains(item))
            SetSelection(item, scrollIntoView: true);
    }

    private void ScrollRowIntoView(CommandSearchItem item)
    {
        Dispatcher.BeginInvoke(DispatcherPriority.Background, new Action(() =>
        {
            if (ResultsList.ItemContainerGenerator.ContainerFromItem(item) is FrameworkElement container)
                container.BringIntoView();
        }));
    }

    private void Jump(CommandSearchItem item)
    {
        JumpRequested?.Invoke(this, item.Entry);
        Close();
    }

    private void Toggle(CommandSearchItem item)
    {
        // The switch is disabled on an unavailable row, and the keyboard path refuses the same rows.
        if (!item.ShowSwitch || !item.IsAvailable)
            return;

        ToggleRequested?.Invoke(this, item.Entry);

        // Re-read after the toggle: the window may have refused it, and the row must then show the real state.
        item.ApplyState(ResolveState(item.Entry));
    }

    private void QueryBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        UpdatePlaceholder();
        if (_suppressQueryChange)
            return;

        RebuildResults(QueryBox.Text);
    }

    private void QueryBox_PreviewKeyDown(object sender, KeyEventArgs e)
    {
        switch (e.Key)
        {
            case Key.Down:
                e.Handled = true;
                MoveSelection(1);
                break;

            case Key.Up:
                e.Handled = true;
                MoveSelection(-1);
                break;

            case Key.Enter:
                e.Handled = true;
                if (_selected != null)
                    Jump(_selected);
                break;

            case Key.Space:
                // Space always types a space; Ctrl+Space is the toggle, so no query can arm a module by accident.
                if ((Keyboard.Modifiers & ModifierKeys.Control) == 0)
                    break;

                e.Handled = true;
                ToggleSelected();
                break;

            case Key.F2:
                // Same toggle without a chord the CJK IMEs claim: Ctrl+Space is their on/off switch, so
                // keyboard users outside ASCII layouts lose it. The legend advertises F2 for that reason.
                e.Handled = true;
                ToggleSelected();
                break;

            case Key.Tab:
                e.Handled = true;
                break;
        }
    }

    private void ToggleSelected()
    {
        if (_selected != null)
            Toggle(_selected);
    }

    private void Scrim_MouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        e.Handled = true;
        Close();
    }

    private void CloseChip_Click(object sender, RoutedEventArgs e) => Close();

    private void Row_MouseLeftButtonUp(object sender, MouseButtonEventArgs e)
    {
        if (sender is not FrameworkElement { DataContext: CommandSearchItem item })
            return;

        // The row switch owns its own clicks.
        if (e.OriginalSource is DependencyObject source && FindAncestor<CheckBox>(source) != null)
            return;

        e.Handled = true;
        SetSelection(item, scrollIntoView: false);
        Jump(item);
    }

    /// <summary>
    /// A row switch was clicked: the TwoWay binding already wrote <see cref="CommandSearchItem.IsOn"/>, so the
    /// window toggles the module and the re-read state either confirms the thumb or puts it back.
    /// </summary>
    private void OnRowSwitchToggled(CommandSearchItem item)
    {
        SetSelection(item, scrollIntoView: false);
        Toggle(item);
    }

    private static T? FindAncestor<T>(DependencyObject? node) where T : DependencyObject
    {
        while (node != null)
        {
            if (node is T match)
                return match;

            node = node is Visual visual ? VisualTreeHelper.GetParent(visual) : LogicalTreeHelper.GetParent(node);
        }

        return null;
    }
}

/// <summary>One command search row, kept alive across queries so filtering never rebuilds it.</summary>
public sealed class CommandSearchItem : INotifyPropertyChanged
{
    private string _hintText = string.Empty;
    private string _keybindText = string.Empty;
    private IReadOnlyList<ModuleSearchRange> _matchRanges = Array.Empty<ModuleSearchRange>();
    private string? _matchedAlias;
    private bool _showKeybind;
    private bool _showSwitch;
    private bool _isAvailable = true;
    private bool _isOn;
    private bool _isSelected;

    public CommandSearchItem(ModuleSearchEntry entry)
    {
        Entry = entry;
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public ModuleSearchEntry Entry { get; }

    public string DisplayName => Entry.DisplayName;

    public string HintText
    {
        get => _hintText;
        private set => SetField(ref _hintText, value, nameof(HintText));
    }

    public string KeybindText
    {
        get => _keybindText;
        private set => SetField(ref _keybindText, value, nameof(KeybindText));
    }

    public IReadOnlyList<ModuleSearchRange> MatchRanges
    {
        get => _matchRanges;
        private set => SetField(ref _matchRanges, value, nameof(MatchRanges));
    }

    public bool ShowKeybind
    {
        get => _showKeybind;
        private set => SetField(ref _showKeybind, value, nameof(ShowKeybind));
    }

    public bool ShowSwitch
    {
        get => _showSwitch;
        private set => SetField(ref _showSwitch, value, nameof(ShowSwitch));
    }

    public bool IsAvailable
    {
        get => _isAvailable;
        private set => SetField(ref _isAvailable, value, nameof(IsAvailable));
    }

    /// <summary>
    /// Switch value. The overlay writes it from live state; a user click writes through this setter, which is
    /// what keeps the TwoWay binding alive so a later re-read can put the thumb back on the true value.
    /// </summary>
    public bool IsOn
    {
        get => _isOn;
        set
        {
            if (_isOn == value)
                return;

            _isOn = value;
            SwitchToggled?.Invoke(this);
        }
    }

    /// <summary>Raised when the switch, not the overlay, changed <see cref="IsOn"/>.</summary>
    public Action<CommandSearchItem>? SwitchToggled { get; set; }

    public bool IsSelected
    {
        get => _isSelected;
        private set => SetField(ref _isSelected, value, nameof(IsSelected));
    }

    /// <summary>Which characters of the display name matched, for the highlighted run.</summary>
    public void ApplyMatch(ModuleSearchHit hit)
    {
        MatchRanges = hit.NameRanges;
        _matchedAlias = hit.MatchedAlias;
    }

    public void ApplyState(CommandSearchRowState state)
    {
        KeybindText = state.KeybindText;
        ShowKeybind = Entry.HasKeybind && state.KeybindText.Length > 0;
        // Every toggleable row gets a switch; an unavailable one gets a disabled switch instead of a hidden
        // one, so the row states the truth (off, not offered) rather than dropping the control.
        ShowSwitch = state.HasToggle;
        IsAvailable = state.IsAvailable;
        // Straight to the field: live state is not a user click, so it must not re-enter the toggle path.
        SetField(ref _isOn, state.IsOn, nameof(IsOn));
        HintText = BuildHint(state);
    }

    public void SetSelected(bool value) => IsSelected = value;

    /// <summary>Tab plus, for alias and compact matches, the text that actually matched.</summary>
    private string BuildHint(CommandSearchRowState state)
    {
        if (!state.IsAvailable)
            return state.UnavailableReason;

        string lead = Entry.IsDestination ? $"Open the {Entry.Tab} tab" : Entry.Tab;
        return _matchedAlias == null ? lead : $"{lead} · matches \"{_matchedAlias}\"";
    }

    private void SetField<T>(ref T field, T value, string propertyName)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
            return;

        field = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }
}

/// <summary>
/// Row host. Border alone gets no automation peer, so rows sit in a control that exposes a list
/// item with a name, on/off state and selection.
/// </summary>
public sealed class CommandSearchRow : ContentControl
{
    protected override AutomationPeer OnCreateAutomationPeer() => new CommandSearchRowAutomationPeer(this);
}

/// <summary>Results list host: a real list peer that surfaces the row peers.</summary>
public sealed class CommandSearchResults : ItemsControl
{
    protected override AutomationPeer OnCreateAutomationPeer() => new CommandSearchResultsAutomationPeer(this);
}

internal sealed class CommandSearchResultsAutomationPeer : FrameworkElementAutomationPeer, ISelectionProvider
{
    public CommandSearchResultsAutomationPeer(CommandSearchResults owner)
        : base(owner)
    {
    }

    protected override string GetClassNameCore() => "CommandSearchResults";

    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.List;

    public override object? GetPattern(PatternInterface patternInterface)
        => patternInterface == PatternInterface.Selection ? this : base.GetPattern(patternInterface);

    /// <summary>Rows advertise this peer as their selection container, so it has to answer for the selection.</summary>
    IRawElementProviderSimple[] ISelectionProvider.GetSelection()
    {
        var owner = (ItemsControl)Owner;
        foreach (object item in owner.Items)
        {
            if (item is not CommandSearchItem { IsSelected: true }
                || owner.ItemContainerGenerator.ContainerFromItem(item) is not DependencyObject container
                || CommandSearchOverlay.FindDescendant<CommandSearchRow>(container) is not CommandSearchRow row)
            {
                continue;
            }

            return [ProviderFromPeer(UIElementAutomationPeer.CreatePeerForElement(row))];
        }

        return [];
    }

    bool ISelectionProvider.CanSelectMultiple => false;

    bool ISelectionProvider.IsSelectionRequired => true;

    /// <summary>
    /// ItemsControl only reports containers that own a peer, and its containers are plain
    /// ContentPresenters, so the row peers are collected from the generated containers instead.
    /// </summary>
    protected override List<AutomationPeer>? GetChildrenCore()
    {
        var owner = (ItemsControl)Owner;
        var children = new List<AutomationPeer>();

        foreach (object item in owner.Items)
        {
            if (owner.ItemContainerGenerator.ContainerFromItem(item) is not DependencyObject container)
                continue;

            if (CommandSearchOverlay.FindDescendant<CommandSearchRow>(container) is CommandSearchRow row
                && UIElementAutomationPeer.CreatePeerForElement(row) is AutomationPeer peer)
            {
                children.Add(peer);
            }
        }

        return children.Count == 0 ? null : children;
    }
}

internal sealed class CommandSearchRowAutomationPeer : FrameworkElementAutomationPeer, ISelectionItemProvider
{
    public CommandSearchRowAutomationPeer(CommandSearchRow owner)
        : base(owner)
    {
    }

    private CommandSearchItem? Item => ((FrameworkElement)Owner).DataContext as CommandSearchItem;

    protected override string GetClassNameCore() => "CommandSearchRow";

    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.ListItem;

    protected override string GetItemStatusCore()
    {
        CommandSearchItem? item = Item;
        if (item == null)
            return string.Empty;

        if (!item.IsAvailable)
            return "Unavailable";

        return item.ShowSwitch ? (item.IsOn ? "On" : "Off") : "Tab";
    }

    public override object? GetPattern(PatternInterface patternInterface)
        => patternInterface == PatternInterface.SelectionItem ? this : base.GetPattern(patternInterface);

    bool ISelectionItemProvider.IsSelected => Item?.IsSelected == true;

    IRawElementProviderSimple ISelectionItemProvider.SelectionContainer
    {
        get
        {
            AutomationPeer? list = UIElementAutomationPeer.FromElement(FindAncestor<CommandSearchResults>());
            // Null is legal while a regenerated row has no list peer yet.
            return list == null ? null! : ProviderFromPeer(list);
        }
    }

    void ISelectionItemProvider.Select() => FindAncestor<CommandSearchOverlay>()?.SelectRow(Item);

    void ISelectionItemProvider.AddToSelection() => ((ISelectionItemProvider)this).Select();

    void ISelectionItemProvider.RemoveFromSelection()
    {
        // Single-select list: a row is never removed from the selection, only replaced.
    }

    private T? FindAncestor<T>() where T : DependencyObject
    {
        DependencyObject? node = Owner;
        while (node != null)
        {
            if (node is T match)
                return match;

            node = node is Visual visual ? VisualTreeHelper.GetParent(visual) : LogicalTreeHelper.GetParent(node);
        }

        return null;
    }
}
