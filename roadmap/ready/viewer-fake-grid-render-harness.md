---
id: viewer-fake-grid-render-harness
title: ViewerHarness — the real viewer against the fake grid, read back as pixels
topic: viewer
status: ready
origin: test-harness plan (2026-08-30)
points: 8
refs: [viewer-fake-grid-login-smoke, viewer-render-readback-tier, viewer-screenshot-wait-for-quiescence]
blocked_by: [test-fake-grid-terrain-layerdata, test-fake-grid-render-fixtures, viewer-plugin-groups, viewer-screenshot-wait-for-quiescence, viewer-render-pixel-oracle]
---

Context: [context/testing.md](../context/testing.md).

All five blockers cleared (2026-09-01), the last being
[[test-fake-grid-render-fixtures]]: `sl_fake_grid::catalogue()` is the
fixture to start(), and `fixtures::catalogue::entry(name)` gives each
subject's local id and region position for the projection oracle.

The full-stack tier. In-process, inside the viewer library's tests
(`src/full_stack_test.rs`, `sl-fake-grid` and `tokio` as dev-dependencies)
because the readback types are crate-private and markers and world
queries need the `App`: the grid on a harness-owned runtime; the viewer
app built from the readback base (no window, no winit, no log) plus
`ViewerWorldPlugins`, `ViewerInputPlugins`, `ViewerRenderPlugins` and
`SlClientPlugin` (no UI or shell groups, no CEF), with the readback target
installed on the `ViewerCamera`; wall-clock stepping with deadlines that
dump the last events on timeout, as the login smoke does.

`ViewerHarness::{start(fixture), login, run_until, wait_event,
wait_marker, wait_quiet, capture -> Option<Frame>, project, grid(fut),
teleport_to, cross_to, logout}`; every test takes `gpu_lock()` and returns
`Ok` with a log line when `capture()` finds no adapter.

First tests: a login renders terrain, water, sky and the stock prim (band
classification, prim disc not background); a textured prim shows its
checker over `GetTexture`; a mesh over `GetMesh2`; `KillObject` after a
marker empties the disc. The subprocess route (`--login-uri` +
`--screenshot-dir`) stays as the manual / Firestorm path only.
