#pragma once

#include <atomic>
#include <cstddef>

namespace lc {

enum NativePerfMetric
{
    PERF_SCAN_LOOP = 0,
    PERF_PLAYER_SCAN,
    PERF_CHEST_SCAN,
    PERF_BLOCK_SCAN,
    PERF_STATE_PUBLISH,
    PERF_OVERLAY_RENDER,
    PERF_METRIC_COUNT
};

struct NativePerfMetricSnapshot
{
    unsigned long long count;
    unsigned long long totalTicks;
    unsigned long long maxTicks;
    unsigned long long units;

    NativePerfMetricSnapshot()
        : count(0), totalTicks(0), maxTicks(0), units(0)
    {
    }
};

struct NativePerfSummary
{
    NativePerfMetricSnapshot metrics[PERF_METRIC_COUNT];
    unsigned int windowMs;

    NativePerfSummary() : windowMs(0) {}
};

class NativePerfDiagnostics
{
public:
    NativePerfDiagnostics(bool enabled, unsigned int intervalMs, unsigned int startMs)
        : _enabled(enabled),
          _intervalMs(intervalMs > 0 ? intervalMs : 1),
          _windowStartedMs(startMs)
    {
        for (std::size_t i = 0; i < PERF_METRIC_COUNT; ++i) {
            _metrics[i].count.store(0);
            _metrics[i].totalTicks.store(0);
            _metrics[i].maxTicks.store(0);
            _metrics[i].units.store(0);
        }
    }

    bool Enabled() const
    {
        return _enabled.load(std::memory_order_relaxed);
    }

    void SetEnabled(bool enabled, unsigned int nowMs)
    {
        const bool wasEnabled = _enabled.exchange(enabled, std::memory_order_acq_rel);
        if (!enabled || wasEnabled) return;

        _windowStartedMs.store(nowMs, std::memory_order_relaxed);
        for (std::size_t i = 0; i < PERF_METRIC_COUNT; ++i) {
            _metrics[i].count.store(0, std::memory_order_relaxed);
            _metrics[i].totalTicks.store(0, std::memory_order_relaxed);
            _metrics[i].maxTicks.store(0, std::memory_order_relaxed);
            _metrics[i].units.store(0, std::memory_order_relaxed);
        }
    }

    void Record(
        NativePerfMetric metric,
        unsigned long long elapsedTicks,
        unsigned long long units = 0)
    {
        if (!Enabled() || metric < 0 || metric >= PERF_METRIC_COUNT) return;

        MetricCounter& counter = _metrics[metric];
        counter.count.fetch_add(1, std::memory_order_relaxed);
        counter.totalTicks.fetch_add(elapsedTicks, std::memory_order_relaxed);
        counter.units.fetch_add(units, std::memory_order_relaxed);

        unsigned long long observed = counter.maxTicks.load(std::memory_order_relaxed);
        while (elapsedTicks > observed &&
               !counter.maxTicks.compare_exchange_weak(
                   observed,
                   elapsedTicks,
                   std::memory_order_relaxed,
                   std::memory_order_relaxed)) {
        }
    }

    bool TryTakeSummary(unsigned int nowMs, NativePerfSummary& summary)
    {
        if (!Enabled()) return false;

        unsigned int started = _windowStartedMs.load(std::memory_order_relaxed);
        const unsigned int elapsed = static_cast<unsigned int>(nowMs - started);
        if (elapsed < _intervalMs) return false;
        if (!_windowStartedMs.compare_exchange_strong(
                started,
                nowMs,
                std::memory_order_acq_rel,
                std::memory_order_relaxed)) {
            return false;
        }

        summary.windowMs = elapsed;
        for (std::size_t i = 0; i < PERF_METRIC_COUNT; ++i) {
            summary.metrics[i].count =
                _metrics[i].count.exchange(0, std::memory_order_relaxed);
            summary.metrics[i].totalTicks =
                _metrics[i].totalTicks.exchange(0, std::memory_order_relaxed);
            summary.metrics[i].maxTicks =
                _metrics[i].maxTicks.exchange(0, std::memory_order_relaxed);
            summary.metrics[i].units =
                _metrics[i].units.exchange(0, std::memory_order_relaxed);
        }
        return true;
    }

private:
    struct MetricCounter
    {
        std::atomic<unsigned long long> count;
        std::atomic<unsigned long long> totalTicks;
        std::atomic<unsigned long long> maxTicks;
        std::atomic<unsigned long long> units;
    };

    std::atomic<bool> _enabled;
    unsigned int _intervalMs;
    std::atomic<unsigned int> _windowStartedMs;
    MetricCounter _metrics[PERF_METRIC_COUNT];
};

} // namespace lc
