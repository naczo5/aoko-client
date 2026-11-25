using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Security;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Threading;
using Aoko.Core;
using Aoko.Core.Accounts;

namespace Aoko.Views;

/// <summary>
/// Accounts tab: saved logins, session switching, and the storage footer. One
/// exclusive operation runs at a time; no action is armed until the user picks a
/// card, and every failure is shown as a failure.
/// </summary>
public partial class AccountsView : UserControl
{
    private readonly ObservableCollection<Card> _cards = new();
    private readonly GameStateClient _client;

    private CancellationTokenSource _lifetime = new();
    private AccountStore? _store;
    private AccountSettings? _settings;
    private LunarAccountManager? _lunar;
    private bool _busy;
    private bool _sessionRefreshInFlight;
    private bool _sessionKnown;
    private bool _lastInWorld;
    private bool _dragActive;
    private string? _sessionName;
    private string _sessionUuid = "";

    public AccountsView()
    {
        InitializeComponent();

        _client = GameStateClient.Instance;
        _client.PropertyChanged += Client_PropertyChanged;
        _client.StateUpdated += Client_StateUpdated;

        CardsControl.ItemsSource = _cards;
        StoragePathText.Text = AccountStore.DefaultPath;
        UpdatePastePlaceholder();

        Loaded += View_Loaded;
        Unloaded += View_Unloaded;

        _lastInWorld = _client.CurrentState.InWorld;
        ApplySessionChip();
        ApplySessionText();
        UpdateActionStates();
    }

    /// <summary>Re-evaluates capability gating after a reconnect or bridge change.</summary>
    public void RefreshAvailability()
    {
        RunOnUi(() =>
        {
            ApplySessionChip();
            ApplySessionText();
            UpdateActionStates();

            if (SupportsSessionRead && !_sessionKnown)
                _ = RefreshSessionAsync(announce: false);
        });
    }

    // === Wiring ===

    private bool SupportsSetSession
        => _client.IsConnected && _client.Capabilities.SupportsAction(BridgeActions.AccountSetSession);

    private bool SupportsSessionRead
        => _client.IsConnected && _client.Capabilities.SupportsAction(BridgeActions.AccountGetSession);

    private Card? SelectedCard => CardsControl.SelectedItem as Card;

    private void View_Loaded(object sender, RoutedEventArgs e)
    {
        if (_lifetime.IsCancellationRequested)
        {
            _lifetime = new CancellationTokenSource();
            _client.PropertyChanged += Client_PropertyChanged;
            _client.StateUpdated += Client_StateUpdated;
        }

        RefreshAvailability();

        if (!TryEnsureStores(out string error))
        {
            SetStatus(error, StatusTone.Critical);
            ShowEmptyState(true);
            return;
        }

        LoadAccounts();
        RefreshRecovery();
    }

    private void View_Unloaded(object sender, RoutedEventArgs e)
    {
        _lifetime.Cancel();
        _client.PropertyChanged -= Client_PropertyChanged;
        _client.StateUpdated -= Client_StateUpdated;
    }

    private void Client_PropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is not (nameof(GameStateClient.IsConnected) or nameof(GameStateClient.Capabilities)))
            return;

        RunOnUi(() =>
        {
            // A dropped bridge invalidates the shown session; the next injection
            // reads it again.
            if (!_client.IsConnected)
                _sessionKnown = false;

            RefreshAvailability();
        });
    }

    private void Client_StateUpdated()
    {
        bool inWorld = _client.CurrentState.InWorld;
        if (inWorld == _lastInWorld)
            return;

        _lastInWorld = inWorld;
        RunOnUi(() =>
        {
            ApplySessionChip();
            UpdateActionStates();
        });
    }

    private void RunOnUi(Action action)
    {
        Dispatcher? dispatcher = Dispatcher;
        if (dispatcher == null || dispatcher.HasShutdownStarted || dispatcher.HasShutdownFinished)
            return;

        if (dispatcher.CheckAccess())
            action();
        else
            dispatcher.BeginInvoke(action);
    }

    // === Session strip ===

    private void ApplySessionChip()
    {
        SessionChipTone tone = AccountsViewLogic.SessionChipToneFor(
            _client.IsConnected,
            _client.CurrentState.InWorld);

        ChipOffline.Visibility = tone == SessionChipTone.Offline ? Visibility.Visible : Visibility.Collapsed;
        ChipMainMenu.Visibility = tone == SessionChipTone.MainMenu ? Visibility.Visible : Visibility.Collapsed;
        ChipWorld.Visibility = tone == SessionChipTone.InWorld ? Visibility.Visible : Visibility.Collapsed;
    }

    private void ApplySessionText()
    {
        if (!_sessionKnown)
        {
            SessionNameText.Text = "Session unknown";
            SessionUuidText.Text = AccountsViewLogic.SessionCaption(_client.IsConnected, SupportsSessionRead);
            return;
        }

        if (string.IsNullOrWhiteSpace(_sessionName))
        {
            SessionNameText.Text = "No account signed in";
            SessionUuidText.Text = "The game reports no active session.";
            return;
        }

        SessionNameText.Text = _sessionName;
        SessionUuidText.Text = string.IsNullOrWhiteSpace(_sessionUuid)
            ? "UUID unavailable"
            : AccountsViewLogic.FormatUuid(_sessionUuid);
    }

    private async void RefreshSession_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            await RefreshSessionAsync(announce: true);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] Refresh failed: {ex.GetType().Name}.");
            SetStatus("The session could not be read. Try again from the main menu.", StatusTone.Critical);
        }
    }

    private async Task RefreshSessionAsync(bool announce)
    {
        if (_sessionRefreshInFlight)
            return;

        if (!_client.IsConnected || !SupportsSessionRead)
        {
            if (announce)
            {
                SetStatus(
                    AccountsViewLogic.SessionCaption(_client.IsConnected, SupportsSessionRead),
                    StatusTone.Critical);
            }

            return;
        }

        _sessionRefreshInFlight = true;
        UpdateActionStates();
        if (announce)
            SetStatus("Reading the session from the game…", StatusTone.Neutral);

        try
        {
            BridgeCommandResult result = await _client.GetSessionAsync(_lifetime.Token);
            ApplySessionResult(result, announce);
        }
        catch (OperationCanceledException)
        {
            if (announce)
                SetStatus("The session read was cancelled.", StatusTone.Neutral);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] GetSession failed: {ex.Message}");
            if (announce)
                SetStatus("The session could not be read. Try again from the main menu.", StatusTone.Critical);
        }
        finally
        {
            _sessionRefreshInFlight = false;
            UpdateActionStates();
        }
    }

    private void ApplySessionResult(BridgeCommandResult result, bool announce)
    {
        if (result.Ok)
        {
            _sessionKnown = true;
            _sessionName = string.IsNullOrWhiteSpace(result.Username) ? null : result.Username.Trim();
            _sessionUuid = result.Uuid;
            ApplySessionText();
            LoadAccounts(SelectedCard?.Id);
        }

        if (announce || !result.Ok)
            SetStatus(result.Message, result.Ok ? StatusTone.Success : StatusTone.Critical);
    }

    // === Account cards ===

    private bool TryEnsureStores(out string error)
    {
        error = "";
        try
        {
            _lunar ??= new LunarAccountManager();
            _store ??= new AccountStore(AccountStore.DefaultPath);
            if (_settings == null)
            {
                _settings = new AccountSettings(AccountSettings.DefaultPath);
                LunarAccountsCheck.IsChecked = _settings.PopulateLunarAccountManager;
            }
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or NotSupportedException or ArgumentException or SecurityException)
        {
            Debug.WriteLine($"[AccountsView] Account storage unavailable: {ex.GetType().Name}.");
            error = "The accounts folder could not be opened. Check permission for %AppData%\\Aoko.";
            return false;
        }
    }

    /// <summary>
    /// Rebuilds the grid. A selection survives by id when the caller names one (import,
    /// delete, switch); otherwise nothing is selected, so opening the tab never arms
    /// a destructive or consequential button on the user's behalf.
    /// </summary>
    private void LoadAccounts(string? selectId = null)
    {
        if (_store == null)
            return;

        string? keep = selectId ?? SelectedCard?.Id;
        _cards.Clear();

        foreach (StoredAccount account in _store.Accounts)
        {
            _cards.Add(new Card(
                account.Id,
                account.Name,
                InitialOf(account.Name),
                AccountsViewLogic.SourceChipLabel(account.Source, account.HasRefresh),
                AccountsViewLogic.SourceToneFor(account.Source, account.HasRefresh),
                AccountsViewLogic.LastUsedLabel(account.LastUsedAt),
                $"UUID: {AccountsViewLogic.FormatUuid(account.Uuid)}",
                AccountsViewLogic.IsCurrentSession(account.Uuid, _sessionUuid),
                AccountsViewLogic.CardAutomationName(
                    account.Name,
                    AccountsViewLogic.SourceChipLabel(account.Source, account.HasRefresh),
                    AccountsViewLogic.IsCurrentSession(account.Uuid, _sessionUuid))));
        }

        Card? target = null;
        int index = AccountsViewLogic.SelectionIndexAfterLoad(
            _cards.Select(card => card.Id).ToList(),
            keep);

        if (index >= 0)
            target = _cards[index];

        CardsControl.SelectedItem = target;
        if (target != null)
            CardsControl.ScrollIntoView(target);

        ShowEmptyState(_cards.Count == 0);
        UpdateActionStates();
    }

    private void CardsControl_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        UpdateActionStates();
        if (CardsControl.SelectedItem is Card card)
            CardsControl.ScrollIntoView(card);
    }

    private void ShowEmptyState(bool empty)
    {
        EmptyState.Visibility = empty ? Visibility.Visible : Visibility.Collapsed;
        CardsControl.Visibility = empty ? Visibility.Collapsed : Visibility.Visible;
    }

    private static string InitialOf(string? name)
    {
        string trimmed = (name ?? "").Trim();
        return trimmed.Length == 0 ? "?" : trimmed[..1].ToUpperInvariant();
    }

    private void RefreshRecovery()
    {
        string? notice = StoreRecovery.Notice(
            AccountStore.DefaultDirectory,
            StoreRecovery.AccountsFileName,
            _store,
            StoreRecovery.SessionStartUtc);

        RecoveryText.Text = notice ?? "";
        RecoveryText.Visibility = notice is null ? Visibility.Collapsed : Visibility.Visible;
    }

    // === Switching ===

    private async void UseAccount_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            await UseSelectedAccountAsync();
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] Use account failed: {ex.GetType().Name}.");
            SetStatus("The switch did not finish. Try again from the main menu.", StatusTone.Critical);
        }
    }

    private async Task UseSelectedAccountAsync()
    {
        if (_busy)
            return;

        Card? card = SelectedCard;
        if (card == null)
        {
            SetStatus(TabCopy.SelectAccountHint, StatusTone.Neutral);
            return;
        }

        string? refusal = AccountsViewLogic.SessionSwitchRefusal(
            _client.IsConnected,
            SupportsSetSession,
            _client.CurrentState.InWorld);

        if (refusal != null)
        {
            SetStatus(refusal, StatusTone.Critical);
            return;
        }

        if (!TryEnsureStores(out string error))
        {
            SetStatus(error, StatusTone.Critical);
            return;
        }

        StoredAccount? account = _store?.Accounts.FirstOrDefault(a => a.Id == card.Id);
        if (account == null)
        {
            SetStatus("That account is no longer saved.", StatusTone.Critical);
            LoadAccounts();
            return;
        }

        SetStatus($"Switching to {card.Name}…", StatusTone.Neutral);

        await RunExclusiveAsync(async token =>
        {
            MinecraftProfile profile;
            try
            {
                profile = account.IsOffline
                    ? OfflineProfile(account)
                    : await RefreshProfileAsync(account, token);
            }
            catch (CookieAuthException ex)
            {
                SetStatus(ex.Message, StatusTone.Critical);
                return;
            }
            catch (HttpRequestException)
            {
                SetStatus("Network authentication failed. Check the connection and retry.", StatusTone.Critical);
                return;
            }
            catch (OperationCanceledException) when (!token.IsCancellationRequested)
            {
                SetStatus("Authentication timed out. Retry from the main menu.", StatusTone.Critical);
                return;
            }

            // Microsoft rotates the refresh token on every sign-in. Persist the rotation
            // before the session write, so a refused or timed-out switch cannot leave the
            // saved account with a token the service has already replaced.
            if (!string.IsNullOrWhiteSpace(profile.RefreshToken))
                _store?.PersistRefreshToken(account, profile.RefreshToken);

            BridgeCommandResult result = await _client.SetSessionAsync(
                profile.Name,
                profile.Uuid,
                profile.AccessToken,
                profile.RefreshToken,
                token);
            if (!result.Ok)
            {
                SetStatus(result.Message, StatusTone.Critical);
                return;
            }

            _store?.UpdateTokens(account, profile);
            _sessionKnown = true;
            _sessionName = profile.Name;
            _sessionUuid = string.IsNullOrWhiteSpace(result.Uuid) ? profile.Uuid : result.Uuid;
            ApplySessionText();
            LoadAccounts(account.Id);
            SetStatus(result.Message, StatusTone.Success);
            ShowLunarStatus(WriteLunarMirror(_sessionUuid));
        });
    }

    private async void RestoreSession_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            await RestoreLaunchAccountAsync();
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] Restore failed: {ex.GetType().Name}.");
            SetStatus("The restore did not finish. Try again from the main menu.", StatusTone.Critical);
        }
    }

    private async Task RestoreLaunchAccountAsync()
    {
        if (_busy)
            return;

        string? refusal = AccountsViewLogic.SessionSwitchRefusal(
            _client.IsConnected,
            SupportsSetSession,
            _client.CurrentState.InWorld);

        if (refusal != null)
        {
            SetStatus(refusal, StatusTone.Critical);
            return;
        }

        SetStatus("Restoring the launch account…", StatusTone.Neutral);

        await RunExclusiveAsync(async token =>
        {
            BridgeCommandResult result = await _client.RestoreSessionAsync(token);
            if (!result.Ok)
            {
                SetStatus(result.Message, StatusTone.Critical);
                return;
            }

            _sessionKnown = true;
            _sessionName = string.IsNullOrWhiteSpace(result.Username) ? null : result.Username.Trim();
            _sessionUuid = result.Uuid;
            ApplySessionText();
            LoadAccounts(SelectedCard?.Id);
            SetStatus(result.Message, StatusTone.Success);
            ShowLunarStatus(WriteLunarMirror(_sessionUuid));
        });
    }

    private static MinecraftProfile OfflineProfile(StoredAccount account)
    {
        string uuid = OfflineAccount.IsValidName(account.Name)
            ? OfflineAccount.ComputeOfflineUuid(account.Name.Trim())
            : account.Uuid;

        return new MinecraftProfile(account.Name, uuid, OfflineAccount.OfflineToken,
            refreshToken: null, AccountSources.Offline);
    }

    /// <summary>
    /// Refreshes a saved login through the same path the importer uses: an MSA
    /// refresh token when the entry has one, otherwise the stored Minecraft JWT.
    /// </summary>
    private static async Task<MinecraftProfile> RefreshProfileAsync(StoredAccount account, CancellationToken token)
    {
        if (account.HasRefresh)
            return await CookieAuth.ProfileFromRefreshTokenAsync(account.RefreshToken, token);

        if (!string.IsNullOrWhiteSpace(account.AccessToken))
            return await CookieAuth.ProfileFromAccessTokenAsync(account.AccessToken, token);

        throw new CookieAuthException("The saved account has no usable token. Re-import it.", isTransient: false);
    }

    // === Importing ===

    private async void ImportFile_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            if (_busy)
                return;

            var dialog = new Microsoft.Win32.OpenFileDialog
            {
                Title = "Import account files",
                Filter = "Account files (*.txt;*.cookie;*.cookies;*.json)|*.txt;*.cookie;*.cookies;*.json|All files (*.*)|*.*",
                Multiselect = true,
                CheckFileExists = true
            };

            Window? owner = Window.GetWindow(this);
            bool? picked = owner != null ? dialog.ShowDialog(owner) : dialog.ShowDialog();
            if (picked != true)
                return;

            await ImportPathsAsync(dialog.FileNames);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] Import dialog failed: {ex.GetType().Name}.");
            SetStatus("The import did not start. Try again.", StatusTone.Critical);
        }
    }

    private void PasteImport_Click(object sender, RoutedEventArgs e) => TogglePanel(PastePanel, OfflinePanel);

    private async void PasteAuthenticate_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            if (_busy)
                return;

            string text = PasteBox.Text;
            if (string.IsNullOrWhiteSpace(text))
            {
                ShowPasteValidation("Paste a token or cookie export first.");
                return;
            }

            ShowPasteValidation("");

            if (!TryEnsureStores(out string error))
            {
                SetStatus(error, StatusTone.Critical);
                return;
            }

            AccountImportService service = CreateImportService();
            await RunExclusiveAsync(async token =>
            {
                AccountImportResult result = await service.ImportTextAsync(text, token);
                if (!result.Ok)
                {
                    SetStatus(result.Detail, StatusTone.Critical);
                    return;
                }

                PasteBox.Clear();
                PastePanel.Visibility = Visibility.Collapsed;
                SessionStrip.Visibility = Visibility.Visible;
                LoadAccounts(result.Account?.Id);
                SetStatus(result.Detail, StatusTone.Success);
                // Importing must not claim Lunar's active account: the client may be signed in
                // as someone else. Only an actual switch passes the live session uuid.
                ShowLunarStatus(WriteLunarMirror(null));
            });
        }
        catch (OperationCanceledException)
        {
            SetStatus("The import was cancelled.", StatusTone.Neutral);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] Paste import failed: {ex.GetType().Name}.");
            SetStatus("The import did not finish. Try again.", StatusTone.Critical);
        }
    }

    private void PasteCancel_Click(object sender, RoutedEventArgs e) => HidePanel(PastePanel);

    private void PasteBox_TextChanged(object sender, TextChangedEventArgs e) => UpdatePastePlaceholder();

    private void UpdatePastePlaceholder()
        => PastePlaceholder.Visibility = PasteBox.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

    private void AddOffline_Click(object sender, RoutedEventArgs e) => TogglePanel(OfflinePanel, PastePanel);

    private void OfflineAdd_Click(object sender, RoutedEventArgs e) => AddOfflineAccount();

    private void AddOfflineAccount()
    {
        try
        {
            if (_busy)
                return;

            if (!TryEnsureStores(out string error))
            {
                SetStatus(error, StatusTone.Critical);
                return;
            }

            if (!OfflineAccount.TryCreate(OfflineNameBox.Text, out string name, out string uuid, out _))
            {
                ShowOfflineValidation(AccountsViewLogic.InvalidOfflineName);
                return;
            }

            ShowOfflineValidation("");
            StoredAccount account = _store!.Upsert(
                new MinecraftProfile(name, uuid, OfflineAccount.OfflineToken, refreshToken: null, AccountSources.Offline),
                AccountSources.Offline);

            OfflineNameBox.Clear();
            OfflinePanel.Visibility = Visibility.Collapsed;
            SessionStrip.Visibility = Visibility.Visible;
            LoadAccounts(account.Id);
            SetStatus($"{name} was added as an offline account.", StatusTone.Success);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] Offline add failed: {ex.GetType().Name}.");
            SetStatus("The offline account could not be saved. Try again.", StatusTone.Critical);
        }
    }

    private void OfflineCancel_Click(object sender, RoutedEventArgs e) => HidePanel(OfflinePanel);

    private void OfflineNameBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter)
            return;

        AddOfflineAccount();
        e.Handled = true;
    }

    private void TogglePanel(Border panel, Border other)
    {
        bool show = panel.Visibility != Visibility.Visible;
        other.Visibility = Visibility.Collapsed;

        if (!show)
        {
            HidePanel(panel);
            return;
        }

        ShowPasteValidation("");
        ShowOfflineValidation("");
        panel.Visibility = Visibility.Visible;
        // The panels are scratch surfaces: collapse the strip and the empty state so
        // opening one does not halve the visible grid.
        SessionStrip.Visibility = Visibility.Collapsed;
        EmptyState.Visibility = Visibility.Collapsed;

        if (ReferenceEquals(panel, PastePanel))
            PasteBox.Focus();
        else
            OfflineNameBox.Focus();
    }

    private void HidePanel(Border panel)
    {
        panel.Visibility = Visibility.Collapsed;
        SessionStrip.Visibility = Visibility.Visible;
        ShowEmptyState(_cards.Count == 0);
        ShowPasteValidation("");
        ShowOfflineValidation("");
    }

    private AccountImportService CreateImportService()
    {
        LunarAccountManager? lunar = _settings?.PopulateLunarAccountManager == true ? _lunar : null;
        return new AccountImportService(_store!, lunar);
    }

    private async Task ImportPathsAsync(IReadOnlyList<string> paths)
    {
        if (_busy || paths.Count == 0)
            return;

        if (!TryEnsureStores(out string error))
        {
            SetStatus(error, StatusTone.Critical);
            return;
        }

        AccountImportService service = CreateImportService();
        bool finished = false;
        var progress = new Progress<AccountImportProgress>(update =>
        {
            // Late posts must not overwrite the batch summary once it is shown.
            if (!finished)
                SetStatus($"Importing {update.Index} of {update.Total}: {update.FileName}", StatusTone.Neutral);
        });

        await RunExclusiveAsync(async token =>
        {
            AccountBatchImportResult batch = await service.ImportFilesAsync(
                paths,
                spacingDelay: null,
                progress: progress,
                cancellationToken: token);

            finished = true;
            LoadAccounts();

            if (batch.Total == 0 || (batch.Imported == 0 && batch.Failures.Count == 0))
                return;

            string headline = batch.Imported == batch.Total
                ? $"Imported {batch.Imported} account file{(batch.Imported == 1 ? "" : "s")}."
                : $"Imported {batch.Imported} of {batch.Total} files.";

            string detail = string.Join(
                Environment.NewLine,
                batch.Failures.Select(failure => $"{failure.FileName}: {failure.Detail}"));

            SetStatus(
                Summarize(headline, batch.Failures),
                batch.Failures.Count == 0 ? StatusTone.Success : StatusTone.Critical,
                batch.Failures.Count == 0 ? null : $"{headline}{Environment.NewLine}{detail}");

            foreach (AccountImportFailure failure in batch.Failures)
                Debug.WriteLine($"[AccountsView] {failure.FileName}: {failure.Detail}");

            if (batch.Imported > 0)
                ShowLunarStatus(WriteLunarMirror(_sessionUuid));
        });
    }

    /// <summary>
    /// Keeps the status line to one sentence: the first failure stays inline,
    /// the rest live in the tooltip.
    /// </summary>
    private static string Summarize(string headline, IReadOnlyList<AccountImportFailure> failures)
    {
        if (failures.Count == 0)
            return headline;

        string first = $"{failures[0].FileName}: {failures[0].Detail}";
        return failures.Count == 1 ? $"{headline} {first}" : $"{headline} {first} (+{failures.Count - 1} more)";
    }

    // === Drag and drop ===

    private void View_DragEnter(object sender, DragEventArgs e) => SetDropHint(DroppedFiles(e) is { Length: > 0 });

    private void View_DragOver(object sender, DragEventArgs e)
    {
        string[]? files = DroppedFiles(e);
        e.Effects = !_busy && files is { Length: > 0 } ? DragDropEffects.Copy : DragDropEffects.None;
        SetDropHint(files is { Length: > 0 });
        e.Handled = true;
    }

    private void View_DragLeave(object sender, DragEventArgs e) => SetDropHint(false);

    private async void View_Drop(object sender, DragEventArgs e)
    {
        try
        {
            e.Handled = true;
            SetDropHint(false);

            string[] files = DroppedFiles(e) ?? Array.Empty<string>();
            if (files.Length == 0)
            {
                SetStatus(
                    AnyDroppedPath(e)
                        ? "Folders are not imported. Drop the cookie file itself."
                        : "That drop was not a file. Drop a cookie export or token file.",
                    StatusTone.Neutral);
                return;
            }

            if (_busy)
            {
                SetStatus("Wait for the current account operation to finish, then drop the files again.", StatusTone.Neutral);
                return;
            }

            await ImportPathsAsync(files);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] Drop failed: {ex.GetType().Name}.");
            SetStatus("The dropped file could not be imported. Try again.", StatusTone.Critical);
        }
    }

    private void SetDropHint(bool visible)
    {
        if (_dragActive == visible)
            return;

        _dragActive = visible;
        DropHint.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
    }

    private static string[]? DroppedFiles(DragEventArgs e)
        => e.Data.GetDataPresent(DataFormats.FileDrop) && e.Data.GetData(DataFormats.FileDrop) is string[] files
            ? files.Where(File.Exists).ToArray()
            : null;

    private static bool AnyDroppedPath(DragEventArgs e)
        => e.Data.GetDataPresent(DataFormats.FileDrop)
           && e.Data.GetData(DataFormats.FileDrop) is string[] paths
           && paths.Any(path => Directory.Exists(path));

    // === Delete ===

    private void DeleteAccount_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            if (_busy)
                return;

            Card? card = SelectedCard;
            if (card == null)
            {
                SetStatus(TabCopy.SelectAccountHint, StatusTone.Neutral);
                return;
            }

            if (!TryEnsureStores(out string error))
            {
                SetStatus(error, StatusTone.Critical);
                return;
            }

            string question =
                $"Delete {card.Name}? This removes the saved credentials only. " +
                "It does not change the account currently active in the game.";

            Window? owner = Window.GetWindow(this);
            MessageBoxResult answer = owner != null
                ? MessageBox.Show(owner, question, "Delete account", MessageBoxButton.OKCancel, MessageBoxImage.Warning, MessageBoxResult.Cancel)
                : MessageBox.Show(question, "Delete account", MessageBoxButton.OKCancel, MessageBoxImage.Warning, MessageBoxResult.Cancel);

            if (answer != MessageBoxResult.OK)
                return;

            int index = _cards.IndexOf(card);
            string? neighbourId = null;
            if (_cards.Count > 1 && index >= 0)
                neighbourId = index < _cards.Count - 1 ? _cards[index + 1].Id : _cards[index - 1].Id;

            if (_store == null || !_store.Remove(card.Id))
            {
                SetStatus("That account was already removed.", StatusTone.Neutral);
                LoadAccounts();
                return;
            }

            LoadAccounts(neighbourId);
            SetStatus($"{card.Name} was deleted from this PC.", StatusTone.Success);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] Delete failed: {ex.GetType().Name}.");
            SetStatus("The account could not be deleted. Try again.", StatusTone.Critical);
        }
    }

    // === Storage footer ===

    private void OpenFolder_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            string folder = AccountStore.DefaultDirectory;
            Directory.CreateDirectory(folder);
            Process.Start(new ProcessStartInfo
            {
                FileName = "explorer.exe",
                Arguments = $"\"{folder}\"",
                UseShellExecute = true
            });
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or Win32Exception or ArgumentException)
        {
            SetStatus("The accounts folder could not be opened.", StatusTone.Critical);
        }
    }

    private void LunarAccounts_Click(object sender, RoutedEventArgs e)
    {
        if (!TryEnsureStores(out string error))
        {
            SetStatus(error, StatusTone.Critical);
            return;
        }

        bool enabled = LunarAccountsCheck.IsChecked == true;
        bool previous = _settings!.PopulateLunarAccountManager;
        _settings.PopulateLunarAccountManager = enabled;

        try
        {
            _settings.Save();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or NotSupportedException)
        {
            // Keep the checkbox honest when the file cannot be written.
            _settings.PopulateLunarAccountManager = previous;
            LunarAccountsCheck.IsChecked = previous;
            SetStatus("The account setting could not be saved.", StatusTone.Critical);
            Debug.WriteLine($"[AccountsView] Settings save failed: {ex.GetType().Name}.");
            return;
        }

        if (!enabled)
        {
            ShowLunarStatus(null);
            SetStatus("Lunar's accounts.json is left alone.", StatusTone.Neutral);
            return;
        }

        ShowLunarStatus(WriteLunarMirror(_sessionUuid));
        SetStatus("Saved accounts are now written into Lunar. Relaunch Lunar to read them.", StatusTone.Success);
    }

    /// <summary>
    /// Mirrors every refresh-backed account into Lunar's accounts.json. The result
    /// carries its own ok/fail flag, so the footer never infers tone from a string.
    /// </summary>
    private LunarAccountWriteResult? WriteLunarMirror(string? activeUuid)
    {
        if (_settings?.PopulateLunarAccountManager != true || _store == null || _lunar == null)
            return null;

        var rows = _store.Accounts
            .Where(account => !account.IsOffline
                              && !string.IsNullOrWhiteSpace(account.Name)
                              && !string.IsNullOrWhiteSpace(account.Uuid)
                              && !string.IsNullOrWhiteSpace(account.AccessToken))
            .Select(account => (account.Name, account.Uuid, account.AccessToken, account.RefreshToken))
            .ToList();

        if (rows.Count == 0)
            return null;

        LunarAccountWriteResult result = _lunar.UpsertAll(rows, activeUuid);
        Debug.WriteLine($"[AccountsView] Lunar write: success={result.Success} detail={result.Detail}");
        return result;
    }

    private void ShowLunarStatus(LunarAccountWriteResult? result)
    {
        if (result is null)
        {
            LunarStatusText.Text = "";
            LunarStatusText.Visibility = Visibility.Collapsed;
            return;
        }

        LunarStatusText.SetResourceReference(
            TextBlock.ForegroundProperty,
            result.Success ? "AccentBrush" : "CriticalBrush");
        LunarStatusText.Text = result.Success
            ? "Lunar's accounts.json now includes the saved accounts. Relaunch Lunar to read it."
            : "Lunar's accounts.json could not be written. Close Lunar and try again.";
        LunarStatusText.Visibility = Visibility.Visible;
    }

    // === Busy discipline and status ===

    private async Task RunExclusiveAsync(Func<CancellationToken, Task> work)
    {
        if (_busy)
            return;

        using var operation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);
        SetBusy(true);
        try
        {
            await work(operation.Token);
        }
        catch (OperationCanceledException)
        {
            SetStatus("The operation was cancelled.", StatusTone.Neutral);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[AccountsView] {ex}");
            SetStatus("The operation did not finish. Try again in a moment.", StatusTone.Critical);
        }
        finally
        {
            SetBusy(false);
        }
    }

    private void SetBusy(bool busy)
    {
        _busy = busy;
        BusyBar.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        UpdateActionStates();
    }

    private void UpdateActionStates()
    {
        bool idle = !_busy;
        bool canSwitch = idle && AccountsViewLogic.CanSwitchSessions(_client.IsConnected, SupportsSetSession);
        bool hasSelection = SelectedCard != null;

        UseAccountButton.IsEnabled = AccountsViewLogic.CanUseSelectedAccount(
            _client.IsConnected,
            SupportsSetSession,
            _busy,
            hasSelection);
        RestoreSessionButton.IsEnabled = canSwitch;
        DeleteButton.IsEnabled = idle && hasSelection;
        ImportFileButton.IsEnabled = idle;
        PasteImportButton.IsEnabled = idle;
        AddOfflineButton.IsEnabled = idle;
        PasteAuthenticateButton.IsEnabled = idle;
        OfflineAddButton.IsEnabled = idle;
        LunarAccountsCheck.IsEnabled = idle;
        RefreshSessionButton.IsEnabled = idle && !_sessionRefreshInFlight && SupportsSessionRead;

        string hint = AccountsViewLogic.PrimaryHint(_client.IsConnected, SupportsSetSession, hasSelection);
        PrimaryHint.Text = hint;
        PrimaryHint.Visibility = hint.Length == 0 ? Visibility.Collapsed : Visibility.Visible;

        string? refusal = AccountsViewLogic.SessionSwitchRefusal(
            _client.IsConnected,
            SupportsSetSession,
            inWorld: false);

        UseAccountButton.ToolTip = refusal
            ?? (hasSelection ? "Switch the running game to this account" : TabCopy.SelectAccountHint);
        RestoreSessionButton.ToolTip = refusal ?? "Return to the account the game was started with";
        DeleteButton.ToolTip = hasSelection
            ? "Remove the saved credentials for the selected account"
            : TabCopy.SelectAccountHint;
    }

    private void SetStatus(string text, StatusTone tone, string? detail = null)
    {
        StatusText.SetResourceReference(TextBlock.ForegroundProperty, TabCopy.BrushKey(tone));
        StatusText.Text = TabCopy.Shorten(text);
        StatusText.ToolTip = detail ?? text;
        StatusText.Visibility = string.IsNullOrWhiteSpace(text) ? Visibility.Collapsed : Visibility.Visible;
    }

    private void ShowPasteValidation(string message) => SetInlineValidation(PasteValidationText, message);

    private void ShowOfflineValidation(string message) => SetInlineValidation(OfflineValidationText, message);

    private static void SetInlineValidation(TextBlock target, string message)
    {
        target.Text = message;
        target.Visibility = string.IsNullOrWhiteSpace(message) ? Visibility.Collapsed : Visibility.Visible;
    }

    // === Card model ===

    /// <summary>One card in the grid. Public so the data template binds by reflection.</summary>
    public sealed class Card
    {
        internal Card(
            string id,
            string name,
            string initial,
            string sourceLabel,
            AccountSourceTone sourceTone,
            string lastUsedText,
            string uuidTooltip,
            bool isCurrent,
            string automationName)
        {
            Id = id;
            Name = name;
            Initial = initial;
            SourceLabel = sourceLabel;
            IsOffline = sourceTone == AccountSourceTone.Offline;
            IsLocalts = sourceTone == AccountSourceTone.Localts;
            LastUsedText = lastUsedText;
            UuidTooltip = uuidTooltip;
            IsCurrent = isCurrent;
            AutomationName = automationName;
        }

        public string Id { get; }

        public string Name { get; }

        public string Initial { get; }

        public string SourceLabel { get; }

        public bool IsOffline { get; }

        public bool IsLocalts { get; }

        public string LastUsedText { get; }

        public string UuidTooltip { get; }

        public bool IsCurrent { get; }

        public string AutomationName { get; }

        public string AutomationStatus => IsCurrent ? "in use" : "";
    }
}
