---
id: viewer-screenshot-wait-for-quiescence
title: Screenshot mode should wait for the scene to load, not for a fixed delay
topic: viewer
status: ready
origin: found while live-testing viewer-p34-3 on aditi
points: 3
refs: [viewer-p34-3, viewer-fake-grid-render-harness, viewer-render-gpu-serialisation]
---

Context: [context/viewer.md](../context/viewer.md),
[context/testing.md](../context/testing.md).

Scope widened (2026-08-30): the predicate is a shared foundation of the
test harness. Build `scene_is_quiet(world)` in
`sl-viewer-world-view/src/quiescence.rs` (texture, mesh, wearable and
animation managers' in-flight/deferred counts, pending patch rebuilds, the
mesh upload backlog; held for 30 consecutive frames); the screenshot mode
polls it with the delay demoted to a timeout, and the full-stack harness's
`wait_quiet()` is the same call. The readback rig's `settle()` (pipelines,
probes, held clock) is the no-grid counterpart and lives with
[[viewer-render-gpu-serialisation]].

The offline screenshot harness (`SL_VIEWER_SCREENSHOT_DIR`, `screenshot.rs`)
fires after a fixed `SL_VIEWER_SCREENSHOT_DELAY` (default 25 s). That makes two
runs **incomparable**, which is exactly what an A/B capture needs them to be:
whatever has not finished streaming by the deadline differs between runs — a
mesh still at a coarse LOD, a texture still undecoded, a bake not yet assembled,
an avatar that had not even spawned. Live-testing [[viewer-p34-3]] this cost
several aditi logins: the first pair of captures differed in
*sun position, water, and other avatars*, and a later pair framed a nearby
avatar whose mesh body had not loaded at all by the 60 s mark.

Shape: capture when the scene goes **quiet** instead — no in-flight mesh /
texture / wearable fetches, no pending bakes, no rigged mesh awaiting its skin —
held for a short settle window (say 2 s), with the existing delay demoted to a
*timeout* (capture anyway, and say so in the log, so a permanently-busy scene
still produces a frame rather than hanging). Each manager already knows its own
in-flight count, so the condition is a cheap sum; the harness only needs a
`scene_is_quiet()` predicate to poll.

This makes every future rendering A/B (and the R-item screenshot debugging that
the headless debug-camera flow relies on) reproducible by construction, rather
than by picking a delay long enough and hoping.
