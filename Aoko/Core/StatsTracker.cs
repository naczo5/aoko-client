using System;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Globalization;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Threading;
using System.Windows;

namespace Aoko.Core;

public sealed class StatsHistogramBucket : INotifyPropertyChanged
{
    private int _count;
    private double _barHeight;
    private Visibility _visibility;

    public StatsHistogramBucket(string label, Visibility visibility = Visibility.Collapsed)
    {
        Label = label;
        _visibility = visibility;
    }

    public string Label { get; }

    public int Count
    {
        get => _count;
        private set
        {
            if (_count == value) return;
            _count = value;
            OnPropertyChanged();
            OnPropertyChanged(nameof(CountText));
        }
    }

    public string CountText => Count.ToString(CultureInfo.InvariantCulture);

    public double BarHeight
    {
        get => _barHeight;
        private set
        {
            if (Math.Abs(_barHeight - value) < 0.01) return;
            _barHeight = value;
            OnPropertyChanged();
        }
    }

    public Visibility Visibility
    {
        get => _visibility;
        private set
        {
            if (_visibility == value) return;
            _visibility = value;
            OnPropertyChanged();
        }
    }

    internal void SetCount(int count)
        => Count = Math.Max(0, count);

    internal void SetBarHeight(double height)
        => BarHeight = Math.Max(0.0, height);

    internal void SetVisible(bool visible)
        => Visibility = visible ? Visibility.Visible : Visibility.Collapsed;

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}

public sealed class StatsTracker : INotifyPropertyChanged
{
    private const double MinCpsBucket = 1.0;
    private const double MaxCpsBucket = 25.0;
    private const double CpsBucketSize = 0.5;
    private const int CpsWindowPadding = 2;
    private const double MaxBarHeight = 118.0;

    private static StatsTracker? _instance;
    public static StatsTracker Instance => _instance ??= new StatsTracker();

    private readonly object _lock = new();
    private readonly int[] _cpsCounts = new int[(int)((MaxCpsBucket - MinCpsBucket) / CpsBucketSize) + 1];
    private Timer? _timer;
    private int _timerRunning;
    private int _totalClicks;
    private int _leftClicks;
    private int _rightClicks;
    private double _cpsTotal;
    private double _peakCps;
    private TimeSpan _totalPlaytime;

    public StatsTracker()
    {
        CpsBuckets = new ObservableCollection<StatsHistogramBucket>(
            Enumerable.Range(0, _cpsCounts.Length)
                .Select(index =>
                {
                    double value = MinCpsBucket + (index * CpsBucketSize);
                    return new StatsHistogramBucket(value.ToString("0.0", CultureInfo.InvariantCulture));
                }));
    }

    public ObservableCollection<StatsHistogramBucket> CpsBuckets { get; }

    public int TotalClicks => _totalClicks;
    public int LeftClicks => _leftClicks;
    public int RightClicks => _rightClicks;
    public int CpsSampleCount => _totalClicks;
    public bool HasCpsSamples => _totalClicks > 0;
    public TimeSpan TotalPlaytime => _totalPlaytime;
    public string TotalPlaytimeText => FormatDuration(_totalPlaytime);
    public string AverageCpsText => HasCpsSamples ? (_cpsTotal / _totalClicks).ToString("0.0", CultureInfo.InvariantCulture) : "n/a";
    public string PeakCpsText => HasCpsSamples ? _peakCps.ToString("0.0", CultureInfo.InvariantCulture) : "n/a";

    public void StartSessionTimer()
    {
        if (Interlocked.Exchange(ref _timerRunning, 1) == 1) return;
        _timer = new Timer(_ => Tick(), null, TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(1));
    }

    public void StopSessionTimer()
    {
        Interlocked.Exchange(ref _timerRunning, 0);
        Interlocked.Exchange(ref _timer, null)?.Dispose();
    }

    public void RecordClick(float cps, bool leftButton)
    {
        if (cps <= 0 || float.IsNaN(cps) || float.IsInfinity(cps)) return;

        lock (_lock)
        {
            _totalClicks++;
            if (leftButton) _leftClicks++;
            else _rightClicks++;

            _cpsTotal += cps;
            if (cps > _peakCps) _peakCps = cps;

            int bucket = Math.Clamp((int)Math.Floor((cps - MinCpsBucket) / CpsBucketSize), 0, _cpsCounts.Length - 1);
            _cpsCounts[bucket]++;
            RefreshCpsBuckets();
        }

        RaiseClickProperties();
    }

    public void RecordPlaytimeSample(bool minecraftActive, bool bridgeConnected, GameState state, bool cursorVisible)
    {
        if (!minecraftActive) return;
        if (bridgeConnected && IsBlockingGui(state, cursorVisible)) return;

        lock (_lock)
        {
            _totalPlaytime = _totalPlaytime.Add(TimeSpan.FromSeconds(1));
        }

        OnPropertyChanged(nameof(TotalPlaytime));
        OnPropertyChanged(nameof(TotalPlaytimeText));
    }

    public void Reset()
    {
        lock (_lock)
        {
            _totalClicks = 0;
            _leftClicks = 0;
            _rightClicks = 0;
            _cpsTotal = 0.0;
            _peakCps = 0.0;
            _totalPlaytime = TimeSpan.Zero;
            Array.Clear(_cpsCounts);
            RefreshCpsBuckets();
        }

        RaiseClickProperties();
        OnPropertyChanged(nameof(TotalPlaytime));
        OnPropertyChanged(nameof(TotalPlaytimeText));
    }

    private void Tick()
    {
        if (Interlocked.CompareExchange(ref _timerRunning, 1, 1) != 1) return;

        try
        {
            var client = GameStateClient.Instance;
            bool cursorVisible = false;
            if (client.IsConnected && client.CurrentState.GuiOpen)
                cursorVisible = WindowDetection.IsCursorVisible();

            RecordPlaytimeSample(
                WindowDetection.IsMinecraftActive(),
                client.IsConnected,
                client.CurrentState,
                cursorVisible);
        }
        catch
        {
        }
    }

    private static bool IsBlockingGui(GameState state, bool cursorVisible)
        => state.GuiOpen && cursorVisible;

    private void RefreshCpsBuckets()
    {
        int first = -1;
        int last = -1;
        int max = 0;
        for (int i = 0; i < _cpsCounts.Length; i++)
        {
            int count = _cpsCounts[i];
            if (count <= 0) continue;
            if (first < 0) first = i;
            last = i;
            if (count > max) max = count;
        }

        int visibleStart = first < 0 ? -1 : Math.Max(0, first - CpsWindowPadding);
        int visibleEnd = last < 0 ? -1 : Math.Min(_cpsCounts.Length - 1, last + CpsWindowPadding);

        for (int i = 0; i < CpsBuckets.Count && i < _cpsCounts.Length; i++)
        {
            int count = _cpsCounts[i];
            CpsBuckets[i].SetCount(count);
            CpsBuckets[i].SetVisible(i >= visibleStart && i <= visibleEnd);
            double height = max <= 0 ? 0.0 : MaxBarHeight * count / max;
            if (count > 0 && height < 4.0) height = 4.0;
            CpsBuckets[i].SetBarHeight(height);
        }
    }

    private void RaiseClickProperties()
    {
        OnPropertyChanged(nameof(TotalClicks));
        OnPropertyChanged(nameof(LeftClicks));
        OnPropertyChanged(nameof(RightClicks));
        OnPropertyChanged(nameof(CpsSampleCount));
        OnPropertyChanged(nameof(HasCpsSamples));
        OnPropertyChanged(nameof(AverageCpsText));
        OnPropertyChanged(nameof(PeakCpsText));
    }

    private static string FormatDuration(TimeSpan duration)
    {
        if (duration.TotalHours >= 1.0)
            return $"{(int)duration.TotalHours:0}:{duration.Minutes:00}:{duration.Seconds:00}";

        return $"{duration.Minutes:00}:{duration.Seconds:00}";
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
