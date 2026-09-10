using System;
using System.Diagnostics;

namespace Aoko.Core;

/// <summary>
/// Human-like click timing generator inspired by Vape V4 and physical Minecraft PvP mechanics.
/// Simulates butterfly clicking doublets, stamina drop-off (fatigue), macro CPS drift,
/// engagement rush bursts, debounce repeat chance, and micro-hesitations.
/// </summary>
public sealed class AdvancedClickTimingState
{
    private const double MaxFatigue = 0.25; // Up to 25% drop-off during continuous sustained clicking
    private const double FatiguePerClick = 0.012;
    private const double FatigueRecoveryPerSecond = 0.35;
    private const double IdleThresholdSeconds = 0.8;

    private long _lastClickTimestamp;
    private double _previousDelayMs;
    private double _currentCps;
    private double _targetCps;
    private long _nextDriftTimestamp;
    private double _fatigue;
    private int _burstClicksRemaining;
    private bool _isButterflySecondClick;
    private int _consecutiveButterflyClicks;

    public double Fatigue => _fatigue;
    public int BurstClicksRemaining => _burstClicksRemaining;
    public bool IsButterflySecondClick => _isButterflySecondClick;

    public void Reset()
    {
        _lastClickTimestamp = 0;
        _previousDelayMs = 0;
        _currentCps = 0;
        _targetCps = 0;
        _nextDriftTimestamp = 0;
        _fatigue = 0;
        _burstClicksRemaining = 0;
        _isButterflySecondClick = false;
        _consecutiveButterflyClicks = 0;
    }

    /// <summary>
    /// Computes the delay in milliseconds for the next click based on the given CPS bounds.
    /// </summary>
    /// <param name="minCps">Minimum configured CPS</param>
    /// <param name="maxCps">Maximum configured CPS</param>
    /// <param name="simulatedTimestamp">Optional custom timestamp (in Stopwatch ticks) for deterministic testing</param>
    /// <returns>Calculated target delay in milliseconds</returns>
    public double CalculateNextDelay(float minCps, float maxCps, long? simulatedTimestamp = null)
    {
        float effectiveMin = Math.Max(1.0f, Math.Min(minCps, maxCps));
        float effectiveMax = Math.Max(effectiveMin, maxCps);

        long now = simulatedTimestamp ?? Stopwatch.GetTimestamp();
        long elapsedTicks = _lastClickTimestamp == 0 ? 0 : now - _lastClickTimestamp;
        double elapsedSeconds = _lastClickTimestamp == 0 ? double.PositiveInfinity : (double)elapsedTicks / Stopwatch.Frequency;

        // Check for idle pause / reset
        if (_lastClickTimestamp == 0 || elapsedSeconds >= IdleThresholdSeconds)
        {
            if (elapsedSeconds < double.PositiveInfinity)
            {
                double recovery = elapsedSeconds * FatigueRecoveryPerSecond;
                _fatigue = Math.Max(0.0, _fatigue - recovery);
                if (elapsedSeconds >= 2.0)
                    _fatigue = 0.0;
            }
            else
            {
                _fatigue = 0.0;
            }

            // Engagement rush: 2 to 5 rapid burst clicks upon engaging
            _burstClicksRemaining = Random.Shared.Next(2, 6);
            _isButterflySecondClick = false;
            _consecutiveButterflyClicks = 0;

            // Pick initial target CPS
            double mid = (effectiveMin + effectiveMax) * 0.5;
            double halfSpan = Math.Max(0.5, (effectiveMax - effectiveMin) * 0.5);
            _targetCps = Math.Clamp(mid + SampleGaussian() * (halfSpan * 0.4), effectiveMin, effectiveMax);
            _currentCps = _targetCps;
            _nextDriftTimestamp = now + (long)((1.5 + Random.Shared.NextDouble() * 2.0) * Stopwatch.Frequency);
        }

        // Periodic target CPS drift
        if (now >= _nextDriftTimestamp)
        {
            double mid = (effectiveMin + effectiveMax) * 0.5;
            double halfSpan = Math.Max(0.5, (effectiveMax - effectiveMin) * 0.5);
            _targetCps = Math.Clamp(mid + (Random.Shared.NextDouble() * 2.0 - 1.0) * (halfSpan * 0.7), effectiveMin, effectiveMax);

            double drift = SampleGaussian() * 0.4;
            _currentCps = Math.Clamp(_currentCps + 0.3 * (_targetCps - _currentCps) + drift, effectiveMin, effectiveMax);

            _nextDriftTimestamp = now + (long)((1.2 + Random.Shared.NextDouble() * 1.8) * Stopwatch.Frequency);
        }

        double effectiveCps = _currentCps;

        // Initial burst rush bonus
        if (_burstClicksRemaining > 0)
        {
            effectiveCps *= 1.0 + (0.05 + Random.Shared.NextDouble() * 0.07);
            _burstClicksRemaining--;
        }

        // Stamina drop-off from fatigue
        effectiveCps *= (1.0 - _fatigue);

        // Clamping bounds: allow drop-off dip below min CPS during prolonged fatigue, but never exceed max
        double lowerBound = Math.Max(1.0, effectiveMin * 0.85);
        double upperBound = Math.Max(lowerBound, (double)effectiveMax);
        effectiveCps = Math.Clamp(effectiveCps, lowerBound, upperBound);

        double baseDelayMs = 1000.0 / effectiveCps;
        double delayMs = baseDelayMs;

        // Butterfly clicking doublet simulation:
        // Alternates between the two fingers with asymmetric timing.
        // Finger 1 -> Finger 2: rapid tap (0.72x - 0.84x base delay)
        // Finger 2 -> Finger 1: recovery reset (1.16x - 1.28x base delay)
        // Pairs average to 1.0x base delay.
        if (_consecutiveButterflyClicks > 8 + Random.Shared.Next(8))
        {
            // Organic rhythm break: single regular tap
            _consecutiveButterflyClicks = 0;
            _isButterflySecondClick = false;
        }
        else
        {
            _consecutiveButterflyClicks++;
            if (_isButterflySecondClick)
            {
                double ratio = 0.72 + Random.Shared.NextDouble() * 0.12;
                delayMs = baseDelayMs * ratio;
                _isButterflySecondClick = false;
            }
            else
            {
                double ratio = 1.16 + Random.Shared.NextDouble() * 0.12;
                delayMs = baseDelayMs * ratio;
                _isButterflySecondClick = true;
            }
        }

        // Debounce / muscle memory repeat chance (~6%)
        if (_previousDelayMs > 0 && Random.Shared.NextDouble() < 0.06)
        {
            delayMs = _previousDelayMs + (Random.Shared.NextDouble() * 4.0 - 2.0);
        }

        // Micro-hesitation / drop-off pauses (3.5% base, scales with fatigue)
        double pauseChance = 0.035 + 0.18 * _fatigue;
        if (Random.Shared.NextDouble() < pauseChance)
        {
            delayMs += 35.0 + Random.Shared.NextDouble() * 50.0;
        }

        // Fatigue accumulation per click
        _fatigue = Math.Min(MaxFatigue, _fatigue + FatiguePerClick);

        // Gaussian micro-jitter (±2ms)
        delayMs += SampleGaussian() * 2.0;
        delayMs = Math.Max(10.0, delayMs);

        _lastClickTimestamp = now;
        _previousDelayMs = delayMs;
        return delayMs;
    }

    private static double SampleGaussian()
    {
        double u1 = Math.Max(1e-7, Random.Shared.NextDouble());
        double u2 = Random.Shared.NextDouble();
        return Math.Sqrt(-2.0 * Math.Log(u1)) * Math.Cos(2.0 * Math.PI * u2);
    }
}
