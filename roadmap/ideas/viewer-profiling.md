---
id: viewer-profiling
title: Viewer profiling story
topic: viewer
status: ideas
origin: user request (2026-07-22), during the Vintage-parity coverage audit
refs: [viewer-statistics-floater, viewer-debug-consoles]
---

Context: [context/viewer.md](../context/viewer.md).

Some sort of profiling for the viewer — the reference's Fast Timers
(Ctrl+Shift+9) answers "which subsystem ate this frame" in-viewer;
we currently only have FPS + pipeline counters. Candidate shapes, to be
weighed when picked up:

- **Tracy via Bevy** — Bevy's `trace_tracy` feature exports every system
  span to the external Tracy profiler; near-zero work, excellent detail,
  but out-of-process (dev-only workflow, fine for us).
- **In-viewer frame breakdown** — a Fast-Timers-like floater over Bevy's
  built-in `SystemInformationDiagnosticsPlugin` / span timings: per-stage
  bars with history, click-to-expand; more work, usable by any user
  reporting a performance problem.
- **GPU timings** — wgpu timestamp queries for the render passes
  (Bevy exposes render diagnostics), since "CPU or GPU bound" is the
  first question.
- Plus the asset-pipeline counters already in the statistics floater
  ([[viewer-statistics-floater]]) and consoles
  ([[viewer-debug-consoles]]).

Likely answer: Tracy for developers now (documentation + feature flag),
the in-viewer breakdown later if user-facing need appears.

Reference (Firestorm, read-only): `llfasttimerview`
(`floater_fast_timers.xml`).
