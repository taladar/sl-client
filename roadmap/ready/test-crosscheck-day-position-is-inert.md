---
id: test-crosscheck-day-position-is-inert
title: --day-position asks for a keyframe the stock cycle does not have
topic: test
status: ready
origin: pointing Firestorm at the fake grid ([[test-firestorm-fake-grid-crosscheck]], 2026-09-10)
points: 2
refs: [test-firestorm-fake-grid-crosscheck, test-fake-grid-sky-without-density-profiles]
---

Context: [context/testing.md](../context/testing.md).

`sl-crosscheck --day-position 0.5` does nothing against the stock region, and
the reference harness says so once, in its own log, where nobody was looking:

```text
day cycle has no sky at position 0.5; leaving environment alone
```

`FSTestHarness::applyEnvironment` pins the sun with
`LLSettingsDay::getSkyAtKeyframe(position, TRACK_GROUND_LEVEL)`, which finds
the frame **at exactly that keyframe** rather than evaluating the track there.
The fake grid's stock environment is a single-keyframe cycle at `0.0`
(deliberately — a cycle with one frame is a sky that does not move with the
region clock, which is what makes two captures minutes apart comparable), so
every position but `0.0` finds nothing and the sky is left alone. A run then
photographs whatever sky the region already had, and two runs at different
`--day-position` values come back identical, which reads as "the sun does not
matter here" rather than as "the knob is not connected".

Until 2026-09-10 this was invisible behind a larger fault — the day cycle was
being rejected outright ([[test-fake-grid-sky-without-density-profiles]]), so
there was no region cycle to sample at any position. With that fixed the
keyframe lookup is what is left.

Two ways out, and they are not exclusive:

- **Evaluate the track instead of indexing it.** Asking for "the sky at day
  position `p`" should blend the frames either side of `p`, which is what a
  running viewer does every frame anyway. A fork change
  (`~/devel/3rdparty/phoenix-firestorm`, branch `test-harness`), and it wants
  the same treatment in this workspace's own harness so the pair still agrees
  — a `--day-position` that moves one viewer's sun and not the other's would
  be worse than one that moves neither.
- **Give a scene a day cycle worth sampling.** `RegionConfig::environment`
  already takes a whole `EnvironmentSettings`, so a scenario that wants
  dawn / noon / dusk can carry frames at those keyframes and the exact lookup
  finds them. That is the cheaper half and does not touch either viewer.

Whichever is done, the flag must **fail loudly** when it cannot be honoured:
a capture whose lighting was not the lighting that was asked for is not a
capture of the requested scene, and a warning buried in a viewer log is not
a report. The runner reads `harness-status.json`, which is where an
unhonoured pin belongs.
