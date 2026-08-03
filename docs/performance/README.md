# Performance run summaries

The performance plan asks for repeatable Phase 0 measurements and a baseline
comparison before behavior-affecting merges.  Keep raw captures outside Git if
they contain client/server details; check in only redacted summaries under
`docs/performance/runs/`.

## Summary format

`compare-performance.ps1` accepts any JSON object containing matching numeric
leaves.  Prefer an explicit `value`/`direction` object so the comparison cannot
guess whether a larger value is better:

```json
{
  "metadata": {
    "commit": "a56cb3f",
    "runtime": "Lunar 26.2",
    "renderer": "Vulkan",
    "scene": "normal-world",
    "durationSeconds": 60
  },
  "metrics": {
    "fpsP95": { "value": 238.4, "direction": "higher" },
    "frameTimeMsP99": { "value": 7.2, "direction": "lower" },
    "stateBandwidthBytesPerSecond": { "value": 18400, "direction": "lower" },
    "managedParseMsP95": { "value": 0.41, "direction": "lower" },
    "aimStateAgeMsP99": { "value": 18.0, "direction": "lower" }
  }
}
```

Numeric leaves without a direction default to `lower`; names containing `fps`,
`throughput`, `success`, or `availability` default to `higher`.  Metadata is
ignored.  Missing metrics are reported but do not fail a comparison.

## Compare two runs

From the repository root:

```powershell
powershell -NoProfile -File .\scripts\compare-performance.ps1 `
  -Baseline .\docs\performance\example-baseline.json `
  -Current .\docs\performance\example-current.json `
  -MaxRegressionPercent 5 `
  -Output .\docs\performance\example-comparison.json `
  -FailOnRegression
```

For real captures, copy the redacted summaries into `docs/performance/runs/`
and substitute those paths.  The checked-in example files are intentionally
synthetic and only verify that the comparison tooling is runnable.

The command prints a table with baseline/current values, absolute and relative
deltas, and a regression flag.  `-FailOnRegression` exits with code `2` when a
matched metric exceeds the allowed regression.  Use one summary per runtime
and scene rather than averaging incompatible renderers together.

## Required metadata

Record the Aoko commit, runtime/client version, bridge DLL family, renderer,
resolution, hardware, scene name, capture duration, and diagnostics setting.
Do not include player names, aliases, server addresses, profile secrets, or raw
bridge logs in a checked-in summary.  Diagnostics are opt-in. Set
`AOKO_PERF_DIAGNOSTICS=1` before starting the loader; it propagates the flag
through the normal configuration heartbeat so an already-running bridge enables
native counters on its next update. Summaries themselves are safe to keep when
redacted.

The plan's minimum comparison set is FPS/frame-time percentiles, process CPU,
working set, state bandwidth/message rate, managed parse time, native scan and
render time, and input jitter.  A run that lacks one of those metrics is still
useful for exploratory work but should not be used as the release gate.
