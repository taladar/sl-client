---
id: viewer-render-test-harness
title: 3D render test harness — render one object without a grid, assert automatically, catch regressions
topic: viewer
status: ready
origin: asked for alongside viewer-ui-test-harness (2026-07), as the 3D counterpart of the same mechanism
refs: [viewer-ui-test-harness, viewer-screenshot-wait-for-quiescence, viewer-ui-radial-menu]
---

Context: [context/viewer.md](../context/viewer.md).

The 3D counterpart of [[viewer-ui-test-harness]]. Every argument that task makes
applies here, and applies harder: rendering bugs are found by a human logging
into OpenSim, rezzing an object, flying the camera to it and squinting — and the
`R*` bug list in `bugs/` is what that process misses.

The `viewer-r22-avatar-render-wip` memory records the cost directly: R22 was
split into seven sub-items, three of which were "committed but do NOT visibly
fix" — because the only way to tell was another login, another bake, another
screenshot. That is the loop this task ends.

Three halves, matching the UI harness, and the same reasoning behind each.

## 1. Render one object without a grid

The precondition for everything else. Today, seeing a prim / mesh / sculpt /
avatar means: start OpenSim, log in, provision the object by OAR import (the
UUID is regenerated, so look it up in `bin/OpenSim.db` first), fly the camera
there. That is minutes per iteration, needs a human, and half of it is grid
administration rather than rendering.

Instead: a binary that takes an **asset** — a decoded prim, an `.llm`, a mesh
blob, a sculpt map, an `avatar_lad.xml` body, a `.anim` — puts it in an empty
scene with a known camera and known lighting, and renders it. No login, no
region, no OAR, no UUID lookup. The gallery of [[viewer-ui-test-harness]], for
geometry.

The corresponding constraint is the same one, and it must be stated up front
because retrofitting it is the expensive part: **an object must be
constructible without a session**. A mesh that can only be spawned by a
`Session` handing it an `ObjectUpdate` is a mesh that can never be tested. The
decode path (bytes → geometry) has to be separable from the transport path
(grid → bytes), with fixture assets standing in for the grid. Some of this
already holds — `sl-mesh`, `sl-sculpt`, `sl-terrain` are sans-I/O — so the work
is mostly in `sl-client-bevy-viewer`'s spawn systems, which currently reach for
live `ObjectState`.

## 2. Automatic assertions

The hard half, and the reason this is not just "a screenshot tool". What can a
machine actually check about a rendered object? More than it looks like, and
none of it needs a human:

- **Geometry invariants, no rendering at all.** Vertex count matches the LOD's
  header; no NaN / infinite positions; normals are unit length; UVs inside
  `[0,1]` unless the face's texgen says otherwise; the bounding box matches the
  prim's declared scale; a rigged mesh's skin weights sum to 1 (**this one has
  already shipped a real bug** — `sl-client-rigged-mesh-skinning` records that
  Bevy does not renormalise, which is exactly the R1 distortion); no vertex
  bound to a joint outside the render-list (the R13 armpit spike).
- **Render-target readback.** Render to a texture headlessly and assert on the
  pixels — not "does it look right" but the things that *are* decidable: the
  object covers a plausible fraction of frame; nothing is NaN / black / fully
  transparent when it should not be; an alpha-masked face is masked; the
  silhouette is symmetric where the source geometry is symmetric.
- **Cross-checks between paths.** The CPU-skinning reference in
  `sl-client-rigged-mesh-skinning` exists precisely to compare against the GPU
  result — make that a standing test rather than a debug affordance.

Expect the same discovery as the UI harness: the first honest run finds real
bugs in things nobody suspected. It did, within minutes, for text.

## 3. Regressions

The [[viewer-ui-test-harness]] work established that the tiers are *universal
invariants*, *declared intents*, and — the gap it did not fill —
**recorded baselines**. This is where baselines matter most.

Rendering has properties that must not drift even though no invariant forbids
drift: a **pie menu option's angle** ([[viewer-ui-radial-menu]]; muscle memory —
a user who has opened the same menu ten thousand times should not have to look),
an attachment point's
offset, a HUD element's screen position, the camera's default framing, an
avatar joint's rest pose. Nothing is "wrong" if these move; they are simply not
allowed to move by accident.

So: record the geometry (angles, positions, sizes) of named things into a
committed baseline, compare on every run, and require a **deliberate** baseline
update to change one. The signal is the diff in review — "this moved the Sit
option 12°" is a sentence somebody can object to, which is the whole point.

Note the conformance suite (`records/`) is a different thing and not a
substitute: it tests the *protocol* against a live grid. This tests *rendering*
against no grid at all.

## What to reuse

- [[viewer-ui-test-harness]] is the template: the registry (one list, so checks
  × objects compound), the two check tiers, the isolated binary, and the rule
  that construction is separable from wiring.
  `sl-client-bevy-viewer/src/ui_test.rs`, `ui_element.rs` and `gallery.rs` are
  the worked example; the module docs argue the design.
- The headless-app trick it proved: Bevy's render internals are `pub`, so a
  downstream crate can stand up the pipeline in a `cargo test` with a dummy
  render target. The UI half needed no fork; check whether the render half does
  before assuming it.
- The viewer's existing debug affordances already solve "put the camera in a
  known place and capture": the absolute camera-pose CLI
  (`--camera-position` / `--camera-look-at` / `--camera-spin`) and the
  `--screenshot-dir` sequence. This task removes the login from underneath them.
- [[viewer-screenshot-wait-for-quiescence]] is a hard dependency in spirit: a
  render assertion taken before the asset pipeline settles is a flake. Without a
  grid, quiescence is much easier to define — everything is local.

## Costs to accept up front

- **Headless GPU.** Render-target readback needs a real adapter; a machine
  without one (or CI) can still run the geometry-invariant tier, which is where
  most of the value is anyway. Split the two so the cheap tier never depends on
  the expensive one.
- **Fixture assets must be committed**, and some are large. Prefer the smallest
  asset that reproduces a class (one `.llm`, one sculpt map, one rigged mesh)
  over a library, and generate procedurally where possible.
- **Pixel assertions are brittle** across drivers. Assert on decidable
  properties (coverage, symmetry, absence of NaN), not on golden images — or the
  suite becomes a driver-version detector.
