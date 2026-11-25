using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Threading;
using Aoko.Core;
using Aoko.Core.Servers;

namespace Aoko.Views;

/// <summary>
/// Servers tab: direct connect, saved-server bookmarks, and server list pings. Only
/// joining and editing the store take the exclusive lock; pings run alongside them
/// with their own in-flight flag, so opening the tab never greys out the actions.
/// </summary>
public partial class ServersView : UserControl
{
    private const int PingParallelism = 4;

    private readonly ServerStore _store = new();
    private readonly ObservableCollection<SavedServerRow> _rows = new();

    private CancellationTokenSource? _lifetime;
    private ServerJoinState _joinState = ServerJoinState.Disconnected;
    private string? _selectedId;
    private int _selectedIndex = -1;
    private bool _busy;
    private bool _pingInFlight;
    private bool _autoPinged;

    public ServersView()
    {
        InitializeComponent();

        ServersList.ItemsSource = _rows;
        ReloadServers(null, -1);
        RefreshAvailability();
    }

    /// <summary>Re-evaluates capability gating after a reconnect or bridge change.</summary>
    public void RefreshAvailability()
    {
        GameStateClient client = GameStateClient.Instance;
        _joinState = ServersViewLogic.EvaluateJoinState(
            client.IsConnected,
            client.Capabilities.SupportsAction(BridgeActions.ServerJoin));

        UpdateActionStates();
    }

    private void View_Loaded(object sender, RoutedEventArgs e)
    {
        RefreshAvailability();
        RefreshRecovery();

        if (_autoPinged || _rows.Count == 0)
            return;

        _autoPinged = true;
        // After the first paint: the tab is usable while the pings run.
        Dispatcher.BeginInvoke(
            DispatcherPriority.Background,
            new Action(() => _ = PingSavedAsync(_rows.ToArray(), announce: true)));
    }

    private void View_Unloaded(object sender, RoutedEventArgs e)
    {
        CancellationTokenSource? lifetime = _lifetime;
        _lifetime = null;
        lifetime?.Cancel();
    }

    // === Store <-> rows ===

    private void ReloadServers(string? selectId, int fallbackIndex)
    {
        var previous = new Dictionary<string, SavedServerRow>(StringComparer.Ordinal);
        foreach (SavedServerRow row in _rows)
            previous[row.Id] = row;

        string? wantedId = selectId ?? _selectedId;
        List<SavedServer> sorted = ServersViewLogic.SortForDisplay(_store.Servers);

        _rows.Clear();
        foreach (SavedServer server in sorted)
        {
            var row = new SavedServerRow(server);
            if (previous.TryGetValue(row.Id, out SavedServerRow? old))
                row.CopyPingStateFrom(old);

            _rows.Add(row);
        }

        _selectedIndex = ServersViewLogic.ResolveSelectionIndex(Ids(), wantedId, fallbackIndex);
        _selectedId = _selectedIndex >= 0 ? _rows[_selectedIndex].Id : null;

        ApplySelection();
        EmptyState.Visibility = _rows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        UpdateActionStates();
    }

    private List<string> Ids()
    {
        var ids = new List<string>(_rows.Count);
        foreach (SavedServerRow row in _rows)
            ids.Add(row.Id);

        return ids;
    }

    private void ApplySelection()
    {
        for (int i = 0; i < _rows.Count; i++)
            _rows[i].IsSelected = i == _selectedIndex;
    }

    private SavedServer? FindSaved(ServerAddress address)
    {
        foreach (SavedServer saved in _store.Servers)
        {
            if (!ServerAddress.TryParse(saved.Address, out ServerAddress other))
                continue;

            if (other.Port == address.Port
                && string.Equals(other.Host, address.Host, StringComparison.OrdinalIgnoreCase))
            {
                return saved;
            }
        }

        return null;
    }

    // === Action state ===

    private void UpdateActionStates()
    {
        bool hasSelection = _selectedIndex >= 0 && _selectedIndex < _rows.Count;
        bool canJoin = ServersViewLogic.CanJoinDirect(_busy, _joinState);
        string blocked = ServersViewLogic.JoinBlockedMessage(_joinState);

        DirectJoinButton.IsEnabled = canJoin;
        JoinSelectedButton.IsEnabled = ServersViewLogic.CanJoinSelected(_busy, _joinState, hasSelection);
        DirectJoinButton.ToolTip = blocked.Length > 0 ? blocked : "Join this address. Press Enter in the box to do the same.";
        JoinSelectedButton.ToolTip = blocked.Length > 0 ? blocked : "Join the selected saved server";

        AddServerButton.IsEnabled = ServersViewLogic.CanStartMutation(_busy);
        AddFormAddButton.IsEnabled = ServersViewLogic.CanStartMutation(_busy);
        AddFormCancelButton.IsEnabled = true;
        PingAllButton.IsEnabled = ServersViewLogic.CanStartPing(_pingInFlight, _rows.Count);
        PingAllButton.ToolTip = _rows.Count == 0
            ? TabCopy.NoServersHint
            : _pingInFlight ? "A ping pass is already running" : "Ping every saved server";
        DeleteButton.IsEnabled = ServersViewLogic.CanStartMutation(_busy) && hasSelection;
        DeleteButton.ToolTip = hasSelection
            ? "Remove this server from the saved list"
            : TabCopy.SelectServerHint;

        string hint = ServersViewLogic.PrimaryHint(_joinState, _rows.Count > 0, hasSelection);
        PrimaryHint.Text = hint;
        PrimaryHint.Visibility = hint.Length == 0 ? Visibility.Collapsed : Visibility.Visible;

        BusyBar.Visibility = _busy ? Visibility.Visible : Visibility.Collapsed;
    }

    private bool TryBeginBusy()
    {
        if (!ServersViewLogic.CanStartMutation(_busy))
            return false;

        _busy = true;
        UpdateActionStates();
        return true;
    }

    private void EndBusy()
    {
        _busy = false;
        UpdateActionStates();
    }

    private CancellationToken LifetimeToken
    {
        get
        {
            if (_lifetime is null || _lifetime.IsCancellationRequested)
                _lifetime = new CancellationTokenSource();

            return _lifetime.Token;
        }
    }

    // === Status and validation lines ===

    private void SetStatus(string text, StatusTone tone)
    {
        StatusLine.SetResourceReference(TextBlock.ForegroundProperty, TabCopy.BrushKey(tone));
        StatusLine.Text = TabCopy.Shorten(text);
        StatusLine.ToolTip = text;
        StatusLine.Visibility = string.IsNullOrWhiteSpace(text) ? Visibility.Collapsed : Visibility.Visible;
    }

    private void RefreshRecovery()
    {
        string? notice = StoreRecovery.Notice(
            StoreRecovery.DefaultDirectory,
            StoreRecovery.ServersFileName,
            _store,
            StoreRecovery.SessionStartUtc);

        RecoveryText.Text = notice ?? "";
        RecoveryText.Visibility = notice is null ? Visibility.Collapsed : Visibility.Visible;
    }

    private void ShowDirectValidation(string message)
        => SetInlineValidation(DirectValidationText, message);

    private void ShowAddValidation(string message)
        => SetInlineValidation(AddFormValidationText, message);

    private static void SetInlineValidation(TextBlock target, string message)
    {
        target.Text = message;
        target.Visibility = string.IsNullOrWhiteSpace(message) ? Visibility.Collapsed : Visibility.Visible;
    }

    // === Direct connect ===

    private void DirectAddressBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter)
            return;

        e.Handled = true;
        JoinDirect();
    }

    private async void JoinDirect()
    {
        try
        {
            ServerTarget target = ServerJoinService.ValidateTarget(DirectAddressBox.Text);
            if (!target.Ok)
            {
                ShowDirectValidation(ServersViewLogic.ValidationMessage(target));
                return;
            }

            ShowDirectValidation("");
            await JoinTargetAsync(target.Address);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[ServersView] Direct join failed: {ex.GetType().Name}.");
            SetStatus("The join could not be started. Try again.", StatusTone.Critical);
        }
    }

    private void DirectJoinButton_Click(object sender, RoutedEventArgs e) => JoinDirect();

    // === Join ===

    private async void JoinSelectedButton_Click(object sender, RoutedEventArgs e) => await JoinSelectedAsync();

    private async Task JoinSelectedAsync()
    {
        try
        {
            if (!ServersViewLogic.CanStartMutation(_busy))
                return;

            if (_selectedIndex < 0 || _selectedIndex >= _rows.Count)
            {
                SetStatus(ServersViewLogic.EmptySelectionMessage, StatusTone.Neutral);
                return;
            }

            SavedServerRow row = _rows[_selectedIndex];
            ServerTarget target = ServerJoinService.ValidateTarget(row.Address);
            if (!target.Ok)
            {
                SetStatus(ServersViewLogic.ValidationMessage(target), StatusTone.Critical);
                return;
            }

            await JoinTargetAsync(target.Address);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[ServersView] Join selected failed: {ex.GetType().Name}.");
            SetStatus("The join could not be started. Try again.", StatusTone.Critical);
        }
    }

    private async Task JoinTargetAsync(ServerAddress address)
    {
        if (_joinState != ServerJoinState.Ready)
        {
            SetStatus(ServersViewLogic.JoinBlockedMessage(_joinState), StatusTone.Critical);
            return;
        }

        if (!ServerJoinService.TryBuildJoinRequest(address, out string host, out int port))
        {
            ShowDirectValidation(ServerJoinService.InvalidAddressMessage);
            return;
        }

        if (!TryBeginBusy())
            return;

        bool ok = false;
        string message;
        try
        {
            BridgeCommandResult result = await GameStateClient.Instance
                .JoinServerAsync(host, port, LifetimeToken)
                .ConfigureAwait(true);

            ok = result.Ok;
            message = ok ? ServersViewLogic.JoinStartedText(address) : result.Message;
        }
        catch (OperationCanceledException)
        {
            message = "The join was cancelled before the game answered.";
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[ServersView] Join failed: {ex.GetType().Name}.");
            message = "The join could not be started. Try again.";
        }
        finally
        {
            EndBusy();
        }

        if (ok)
        {
            SavedServer? saved = FindSaved(address);
            if (saved != null)
            {
                _store.MarkJoined(saved);
                ReloadServers(saved.Id, _selectedIndex);
            }
        }

        SetStatus(message, ok ? StatusTone.Success : StatusTone.Critical);
    }

    // === Saved server rows ===

    private void ServerCard_Click(object sender, RoutedEventArgs e)
    {
        if (((FrameworkElement)sender).DataContext is not SavedServerRow row)
            return;

        int index = _rows.IndexOf(row);
        if (index < 0 || index == _selectedIndex)
            return;

        SelectRow(index);
    }

    /// <summary>Arrow-key focus moves the selection, exactly like the Accounts list.</summary>
    private void ServerCard_GotKeyboardFocus(object sender, KeyboardFocusChangedEventArgs e)
    {
        if (((FrameworkElement)sender).DataContext is not SavedServerRow row)
            return;

        int index = _rows.IndexOf(row);
        if (index >= 0 && index != _selectedIndex)
            SelectRow(index);
    }

    /// <summary>Second click, the natural "do it" gesture, joins the row.</summary>
    private async void ServerCard_DoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (((FrameworkElement)sender).DataContext is not SavedServerRow row)
            return;

        int index = _rows.IndexOf(row);
        if (index < 0)
            return;

        SelectRow(index);
        e.Handled = true;
        await JoinSelectedAsync();
    }

    private void SelectRow(int index)
    {
        _selectedIndex = index;
        _selectedId = _rows[index].Id;
        ApplySelection();
        UpdateActionStates();
    }

    private void LatencyChip_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            if (!ServersViewLogic.CanStartPing(_pingInFlight, _rows.Count))
                return;

            if (((FrameworkElement)sender).DataContext is not SavedServerRow row)
                return;

            _ = PingSavedAsync(new[] { row.Id }, announce: true);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[ServersView] Chip ping failed: {ex.GetType().Name}.");
            SetStatus("The ping could not finish. Try again.", StatusTone.Critical);
        }
    }

    private void PingAllButton_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            if (!ServersViewLogic.CanStartPing(_pingInFlight, _rows.Count))
                return;

            _ = PingSavedAsync(rows: _rows.ToArray(), announce: true);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[ServersView] Ping all failed: {ex.GetType().Name}.");
            SetStatus("The ping could not finish. Try again.", StatusTone.Critical);
        }
    }

    private void DeleteButton_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            if (!ServersViewLogic.CanStartMutation(_busy))
                return;

            if (_selectedIndex < 0 || _selectedIndex >= _rows.Count)
            {
                SetStatus(ServersViewLogic.EmptySelectionMessage, StatusTone.Neutral);
                return;
            }

            SavedServerRow row = _rows[_selectedIndex];
            string question =
                $"Delete {row.Name}? This only removes the bookmark. Your running game is not affected.";
            Window? owner = Window.GetWindow(this);
            MessageBoxResult answer = owner is null
                ? MessageBox.Show(question, "Delete server", MessageBoxButton.OKCancel, MessageBoxImage.Warning)
                : MessageBox.Show(owner, question, "Delete server", MessageBoxButton.OKCancel, MessageBoxImage.Warning);

            if (answer != MessageBoxResult.OK)
                return;

            int index = _selectedIndex;
            _store.Remove(row.Id);
            ReloadServers(null, index);
            SetStatus(ServersViewLogic.DeletedText(row.Name), StatusTone.Success);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[ServersView] Delete failed: {ex.GetType().Name}.");
            SetStatus("The server could not be deleted. Try again.", StatusTone.Critical);
        }
    }

    // === Add form ===

    private void AddServerButton_Click(object sender, RoutedEventArgs e)
    {
        if (!ServersViewLogic.CanStartMutation(_busy))
            return;

        AddFormValidationText.Visibility = Visibility.Collapsed;
        AddFormCard.Visibility = Visibility.Visible;

        if (string.IsNullOrWhiteSpace(AddAddressBox.Text))
            AddAddressBox.Text = DirectAddressBox.Text.Trim();

        AddAddressBox.Focus();
        AddAddressBox.SelectAll();
    }

    private void AddFormCancelButton_Click(object sender, RoutedEventArgs e) => HideAddForm();

    private void AddAddressBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter)
            return;

        e.Handled = true;
        AddFromForm();
    }

    private void AddFormAddButton_Click(object sender, RoutedEventArgs e) => AddFromForm();

    private void AddFromForm()
    {
        try
        {
            if (!ServersViewLogic.CanStartMutation(_busy))
                return;

            ServerTarget target = ServerJoinService.ValidateTarget(AddAddressBox.Text);
            if (!target.Ok)
            {
                ShowAddValidation(ServersViewLogic.ValidationMessage(target));
                return;
            }

            string name = AddNameBox.Text.Trim();
            SavedServer saved = _store.Add(name.Length == 0 ? null : name, target.Address.Display);

            HideAddForm();
            ReloadServers(saved.Id, _selectedIndex);
            SetStatus(ServersViewLogic.AddedText(saved), StatusTone.Success);
            _ = PingSavedAsync(new[] { saved.Id }, announce: false);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[ServersView] Add failed: {ex.GetType().Name}.");
            SetStatus("The server could not be saved. Try again.", StatusTone.Critical);
        }
    }

    private void HideAddForm()
    {
        AddFormCard.Visibility = Visibility.Collapsed;
        AddFormValidationText.Visibility = Visibility.Collapsed;
        AddNameBox.Text = "";
        AddAddressBox.Text = "";
    }

    // === Ping ===

    /// <summary>
    /// Pings never take the exclusive lock: they carry their own in-flight flag, so
    /// join/add/delete stay usable while a 20-server pass is running.
    /// </summary>
    private async Task PingSavedAsync(IReadOnlyList<SavedServerRow> rows, bool announce)
    {
        if (rows.Count == 0 || _pingInFlight)
            return;

        _pingInFlight = true;
        UpdateActionStates();
        try
        {
            var targets = new List<ServerAddress>(rows.Count);
            var pinged = new List<SavedServerRow>(rows.Count);
            var unparsed = new List<SavedServerRow>(rows.Count);

            foreach (SavedServerRow row in rows)
            {
                if (!ServerAddress.TryParse(row.Address, out ServerAddress address))
                {
                    unparsed.Add(row);
                    continue;
                }

                targets.Add(address);
                pinged.Add(row);
            }

            OnUi(() =>
            {
                foreach (SavedServerRow row in unparsed)
                    row.ApplyInvalidAddress();

                foreach (SavedServerRow row in pinged)
                    row.MarkPinging();
            });

            if (targets.Count == 0)
                return;

            CancellationToken token = LifetimeToken;
            IReadOnlyList<ServerPingResult> results;
            try
            {
                results = await ServerPing.PingManyAsync(targets, PingParallelism, token).ConfigureAwait(true);
            }
            catch (OperationCanceledException)
            {
                return;
            }

            OnUi(() =>
            {
                if (token.IsCancellationRequested)
                {
                    // Cancelled from Unloaded: leave a plain chip rather than "Cancelled".
                    foreach (SavedServerRow row in pinged)
                        row.MarkNotPinged();

                    return;
                }

                int online = 0;
                for (int i = 0; i < pinged.Count; i++)
                {
                    ServerPingResult result = i < results.Count && results[i] is not null
                        ? results[i]
                        : new ServerPingResult { Error = "Offline" };

                    if (!result.Online)
                        Debug.WriteLine($"[ServersView] Ping failed for {pinged[i].Address}: {result.Error}");

                    pinged[i].ApplyPing(result);
                    if (result.Online)
                        online++;
                }

                if (announce)
                {
                    SetStatus(
                        ServersViewLogic.PingSummaryText(pinged.Count, online),
                        online == pinged.Count ? StatusTone.Success : StatusTone.Critical);
                }
            });
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[ServersView] Ping failed: {ex.GetType().Name}.");
            OnUi(() => SetStatus("The ping could not finish. Try again.", StatusTone.Critical));
        }
        finally
        {
            OnUi(() =>
            {
                _pingInFlight = false;
                UpdateActionStates();
            });
        }
    }

    private async Task PingSavedAsync(IReadOnlyList<string> ids, bool announce)
    {
        var rows = new List<SavedServerRow>(ids.Count);
        foreach (string id in ids)
        {
            SavedServerRow? row = _rows.FirstOrDefault(candidate =>
                string.Equals(candidate.Id, id, StringComparison.Ordinal));
            if (row != null)
                rows.Add(row);
        }

        await PingSavedAsync(rows, announce).ConfigureAwait(true);
    }

    // === Dispatcher ===

    private void OnUi(Action action)
    {
        ArgumentNullException.ThrowIfNull(action);

        if (Dispatcher.CheckAccess())
        {
            action();
            return;
        }

        if (Dispatcher.HasShutdownStarted || Dispatcher.HasShutdownFinished)
            return;

        try
        {
            Dispatcher.Invoke(action);
        }
        catch (InvalidOperationException)
        {
            // The dispatcher shut down between the check and the call.
        }
    }
}
