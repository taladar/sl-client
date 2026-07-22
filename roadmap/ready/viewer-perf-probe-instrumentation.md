---
id: viewer-perf-probe-instrumentation
title: Reflection-probe cost instrumentation (capture / blit / filter buckets)
topic: viewer
status: ready
origin: reflection-probe performance planning round (2026-07-22)
refs: [viewer-profiling, viewer-p33-1, viewer-p33-2]
---

Context: [context/viewer.md](../context/viewer.md).

Make the probe system's cost visible before optimizing it. Today the only
signal is `FrameTimeDiagnosticsPlugin`'s whole-frame number: the capture
cameras' render cost, the `copy_probe_faces` blit, and the
`GeneratedEnvironmentMapLight` filter chains are indistinguishable inside
it. Every other task in this perf round states its acceptance criteria in
the numbers this task produces, so it lands first.

Deliberately narrow — the viewer-wide profiling story (Tracy, sampling
profilers, memory) stays in [[viewer-profiling]]; this is only the probe
slice:

- Bevy `RenderDiagnosticsPlugin` rows plus custom render-world time spans
  around `copy_probe_faces` (`probes.rs`). Where per-node spans cannot
  attribute cost to a single capture face or filter chain, add A/B
  kill-switches for attribution: one debug toggle forcing every rig
  camera `is_active: false`, one removing the
  `GeneratedEnvironmentMapLight` components — "frame time with / without"
  isolates each bucket. (Debug-only toggles, so env vars are fine; the
  user-facing switches are [[viewer-perf-probe-quality-knobs]].)
- Counters: filter-chain invocations per frame (expected today: one per
  live rig, every frame — confirming the standing-refilter finding), face
  renders per frame (expected ≤ 1), face blit copies per frame.
- Surface the readings in the debug overlay / status bar and in a
  once-per-second structured log line, so the offline screenshot and
  `render_readback` harnesses can capture them headlessly.
- Commit a baseline table with the task: per-face capture ms, per-probe
  filter ms, blit ms — at 1 and at 5 live rigs, in a gallery scene and in
  a busy live scene.

Verification: the numbers move as expected — a second live probe adds
about one filter-chain of cost; a capture-burst frame shows exactly one
face render; the baseline table is committed.

Suggested landing order for the round: this task, then
[[viewer-perf-probe-filter-on-capture]] →
[[viewer-perf-probe-capture-content]] →
[[viewer-perf-probe-capture-shadows]] →
[[viewer-perf-probe-scheduling]] → [[viewer-perf-probe-quality-knobs]];
the ideas tier after.
