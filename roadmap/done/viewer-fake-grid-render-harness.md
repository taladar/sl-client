---
id: viewer-fake-grid-render-harness
title: ViewerHarness — the real viewer against the fake grid, read back as pixels
topic: viewer
status: done
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

Done (2026-09-03): `sl-client-bevy-viewer/src/full_stack_test.rs`, five
tests, all passing on RADV. `cross_to` is the one part of the planned API
that is **not** here — see below.

**What the tests decide.** The band classification came out stronger than
"terrain, water, sky are on screen": from 60 m over the middle of a flat
25 m region looking level north, the frame's three bands have boundaries
that are *geometry* — the horizon is the view axis, and the far shore is
where the region's own ground stops, projected through the camera that
drew the frame. Sky above the horizon, the endless sea from the horizon
down to the shore, the ground below it. The claim is that the three are
three different things and none is black: no colour is named, because the
sea's colour is the region's water settings and the ground's is its
detail textures, and both are content. A sea that failed to render reads
as the sky; a ground that failed reads as the sea.

The other three subjects are decided by the catalogue's checker being
**red-and-green marker cells**, so "the texture arrived" is a claim about
two dominant-channel coverages inside a projected disc and not about a
shade. Each subject is framed from ground level aiming *up* past it, so
everything behind it is sky and the disc is the subject alone.

Four things the plan did not settle.

**`cross_to` is not implemented, because there is nothing to call.** The
fake grid has no region crossing — no `CrossedRegion`, no neighbour child
agents — and adding one is [[test-fake-grid-neighbours-crossing]], which
is blocked on this task. `teleport_to` is here and tested; the crossing
half belongs to the task that builds the crossing.

**A marker needed a sender.** The plan has the harness *waiting* for a
timeline `Marker`, and the timeline ([[test-fake-grid-timeline]]) is
blocked on this task — so the wait would have had nothing to wait for.
`sl-fake-grid` grew `marker` / `marker_name` / `MARKER_METHOD`
(`src/marker.rs`): a `GenericMessage` on the method
`sl-fake-grid-marker`, inert in every client that does not know it,
carrying its name as the single parameter blob. The harness has both
halves — `mark(name)` sends one, `wait_marker(name)` waits for it — so
the kill test synchronises on an observation and never on a sleep. UDP is
ordered per circuit, so a marker sent after a `KillObject` arrives after
it. The timeline will emit the same message from a scenario step.

**Quiet is two things, and the plan named one.** The readback rig's
`settle()` answers "the *render* is quiet" — no pipeline compiling, every
probe captured. A full-stack scene also streams assets, and a frame taken
while a texture is still decoding shows an untextured face. So
`wait_quiet()` waits for `SceneQuiescence::is_quiet` (every asset store's
in-flight count, which
[[viewer-screenshot-wait-for-quiescence]] already built and documented as
"the full-stack harness's") *and then* settles the render. The outstanding
count goes into the timeout report, so a wait that never ends says
whether the session is working or stuck.

**The frame's alpha is the glow mask, not opacity.** Every capture in this
tier is `all_transparent` and correct — the composited output's alpha
channel carries the glow mask, the same channel `screenshot.rs` drops
before writing a PNG. So the health check here asserts only the black
half, and every band comparison discards alpha. Asserting the readback
tier's `FrameHealth { all_transparent: false }` would fail every capture.

Two traps worth keeping, both of which cost a run each:

- **The readback observer must be attached to its own entity.** The full
  viewer runs other readbacks — the GPU pick lifts its ID buffer back the
  same way, the GPU avatar pipeline its palettes — so a global
  `On<ReadbackComplete>` drains whichever fired last into the frame slot.
  The symptom is misleading rather than obviously wrong: the buffer is
  the wrong length for a 256² frame and the capture reports "the readback
  and the render target disagree about the frame size".
- **`App::finish` / `cleanup` are the harness's job.** A plain `update`
  loop never calls them, so the render app is never built, `RenderDevice`
  never reaches the main world, and Bevy's own batching systems fail
  parameter validation on the first frame.

And the ordinary cost of standing a subset of the viewer up: the world and
render groups *read* a dozen resources and two dozen message channels
whose owners are in the UI, shell and edit groups. Bevy fails a system's
parameter validation on a missing one and names neither the system nor
the resource without a debug rebuild, so they are inserted deliberately in
one block (the fixture world's list, plus `MapTracking` and
`CursorGrabAllowed`) rather than discovered one panic at a time. When one
is missed, `RUST_BACKTRACE=full` names the failing system by its whole
parameter list in the `run_unsafe` frame, which is the only way to read
that message without a `bevy/debug` rebuild.
